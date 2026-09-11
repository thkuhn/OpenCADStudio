//! Per-layer miter geometry for L/T and N-way wall joins.
//!
//! Given joined wall axes and their material-layer stacks, this module
//! computes a diagonal miter line at the shared corner and clips each wall's
//! matching layer footprint against it. Layers are paired first by
//! `material`/`function` equality, then by closest cumulative offset-from-axis
//! for ties or remaining unmatched candidates; layers that still cannot be
//! uniquely resolved return `None` so the caller can fall back to the
//! single-vertex `corner_override` path.
//!
//! Multi-wall junctions (3+ walls at one point) are handled by
//! [`mitered_junction_layer_footprints`], which orders participants by angle
//! and miters each endpoint wall against its angular neighbors.

use super::contour::{layer_contours, layer_contours_with_bulges};
use super::join::{JoinKind, JoinOverrideStyle, Junction, JunctionOverride, JunctionRole, LayerRef};

/// Minimum angle (5°) below which diagonal miters are rejected as degenerate.
/// At 5°, the miter intersection distance is ~23x the layer half-offset (1/sin(2.5°)).
/// Beyond this, numerical instability and extreme geometry extensions make
/// the miter unreliable; we fall back to a simple corner extension.
/// This also rejects angles near 180°, where walls are nearly collinear.
const MIN_MITER_SINE: f64 = 0.0872; // slightly more than sin(5°)

/// One wall layer as consumed by the miter matcher: geometry plus optional
/// identity fields used for cross-wall pairing.
#[derive(Debug, Clone, PartialEq)]
pub struct MiterLayer {
    pub thickness: f64,
    pub axis_offset: f64,
    pub material: String,
    pub function: String,
    pub layer_id: uuid::Uuid,
}

impl MiterLayer {
    /// Geometry-only layer (empty material/function). Used by tests and
    /// callers that don't carry identity metadata.
    pub fn geom(thickness: f64, axis_offset: f64) -> Self {
        Self {
            thickness,
            axis_offset,
            material: String::new(),
            function: String::new(),
            layer_id: uuid::Uuid::nil(),
        }
    }

    pub fn with_id(
        thickness: f64,
        axis_offset: f64,
        material: impl Into<String>,
        function: impl Into<String>,
        layer_id: uuid::Uuid,
    ) -> Self {
        Self {
            thickness,
            axis_offset,
            material: material.into(),
            function: function.into(),
            layer_id,
        }
    }

    fn as_geom(&self) -> (f64, f64) {
        (self.thickness, self.axis_offset)
    }
}

/// Context describing the *other* wall at a join, used when regenerating one
/// wall's visible representation with per-layer miters.
#[derive(Debug, Clone)]
pub struct JoinMiterContext {
    /// Vertex index on *this* wall's axis that sits at the join (0 or last).
    pub self_end: usize,
    /// Other wall's axis polyline as 2D points (already trimmed/extended by
    /// [`super::join::join_wall_axes`]).
    pub other_axis: Vec<(f64, f64)>,
    /// Other wall's layers in stack order (index 0 = first from the reference
    /// side), including material/function for matching.
    pub other_layers: Vec<MiterLayer>,
    /// Joined end on the other wall (`Some(0|last)` for L / T-stem). `None`
    /// when the other wall is the through-wall of a T-junction (no endpoint
    /// moved); the helper then locates the segment under the join point.
    pub other_end: Option<usize>,
    pub kind: JoinKind,
    /// Axis bulges on this wall (LWPOLYLINE convention). Empty = straight.
    pub self_bulges: Vec<f64>,
    /// Axis bulges on the other wall. Empty = straight.
    pub other_bulges: Vec<f64>,
    /// This wall is the through-wall of a T-junction: compute layer cutouts
    /// instead of stem miters.
    pub as_through: bool,
}

/// For each layer of wall A, return either a miter-clipped closed footprint
/// polygon, or `None` when that layer can't be matched against wall B
/// (caller should fall back to `corner_override` for that layer).
///
/// `end_a` is the joined vertex index on A. `end_b` is `Some` for an L-join
/// or the stem of a T-join; `None` means B is the through-wall of a T.
///
/// **L-joins** (`end_b = Some`) produce a diagonal per-layer miter.
/// **T-joins** against a through-wall (`end_b = None`, or `kind = T`) butt
/// the stem **Structural** layer to the near face of the through-wall's
/// Structural partner. Other stem layers still stop on the approach outer
/// face so finishes do not tunnel to the far side. Per-layer depth is
/// available via join overrides.
pub fn mitered_layer_footprints(
    axis_a: &[(f64, f64)],
    layers_a: &[MiterLayer],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    end_b: Option<usize>,
    kind: JoinKind,
) -> Vec<Option<Vec<(f64, f64)>>> {
    mitered_layer_footprints_with_bulges(
        axis_a, layers_a, end_a, axis_b, layers_b, end_b, kind, &[], &[],
    )
}

/// Like [`mitered_layer_footprints`], offsetting layer edges with axis bulges
/// so arc walls miter on concentric arcs rather than chords.
pub fn mitered_layer_footprints_with_bulges(
    axis_a: &[(f64, f64)],
    layers_a: &[MiterLayer],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    end_b: Option<usize>,
    kind: JoinKind,
    bulges_a: &[f64],
    bulges_b: &[f64],
) -> Vec<Option<Vec<(f64, f64)>>> {
    if axis_a.len() < 2 || axis_b.len() < 2 || layers_a.is_empty() {
        return vec![None; layers_a.len()];
    }
    if end_a >= axis_a.len() {
        return vec![None; layers_a.len()];
    }

    let geom_a: Vec<(f64, f64)> = layers_a.iter().map(MiterLayer::as_geom).collect();
    let geom_b: Vec<(f64, f64)> = layers_b.iter().map(MiterLayer::as_geom).collect();
    let contours_a = contours_xy(axis_a, bulges_a, &geom_a);
    let contours_b = contours_xy(axis_b, bulges_b, &geom_b);
    if contours_a.is_empty() || contours_b.is_empty() {
        return vec![None; layers_a.len()];
    }

    // T against a through-wall (`end_b = None`): straight butt extension,
    // not a diagonal L-style miter. `kind` is kept in the signature for
    // call-site clarity (L vs T) even though the end marker is authoritative.
    let _ = kind;
    if end_b.is_none() {
        return t_junction_layer_footprints(
            axis_a,
            layers_a,
            end_a,
            axis_b,
            layers_b,
            &contours_a,
            &contours_b,
        );
    }

    let pairing = match_layer_indices(layers_a, layers_b);
    let mut out = Vec::with_capacity(layers_a.len());

    for (i, b_idx) in pairing.into_iter().enumerate() {
        let Some(j) = b_idx else {
            out.push(None);
            continue;
        };
        if i >= contours_a.len() || j >= contours_b.len() {
            out.push(None);
            continue;
        }
        let (ref a_b1, ref a_b2) = contours_a[i];
        let (ref b_b1, ref b_b2) = contours_b[j];
        match miter_one_layer(a_b1, a_b2, end_a, b_b1, b_b2, end_b, axis_a, axis_b) {
            Some(fp) if fp.len() >= 3 => out.push(Some(fp)),
            _ => out.push(None),
        }
    }
    out
}

/// Resolve which [`JoinOverrideStyle`] applies to a layer identified by
/// `layer_ref`, given `override_data`. A [`super::LayerPairOverride`]
/// whose `layer_a` matches `layer_ref` (material_id + role_tag) takes
/// precedence over `default_style`; `None` means no override applies and the
/// caller should keep the automatic result for that layer.
fn resolve_layer_override_style<'a>(
    layer_ref: &LayerRef,
    override_data: &'a JunctionOverride,
) -> Option<&'a JoinOverrideStyle> {
    for pair in &override_data.layer_pairs {
        if layer_ref_matches_one(&pair.layer_a, layer_ref) {
            return Some(&pair.style);
        }
    }
    override_data.default_style.as_ref()
}

/// Compare two [`LayerRef`]s the same way `commands::layer_ref_matches`
/// does: prioritize the stable `layer_id` when both sides have one set,
/// otherwise fall back to the `material_id`/`role_tag`/`index` triple. This
/// keeps legacy/hand-written overrides (with `layer_id: None`) matching a
/// live layer that now carries a real ID.
fn layer_ref_matches_one(a: &LayerRef, b: &LayerRef) -> bool {
    match (a.layer_id, b.layer_id) {
        (Some(x), Some(y)) => x == y,
        _ => a.material_id == b.material_id && a.role_tag == b.role_tag && a.index == b.index,
    }
}

/// Override-aware variant of [`mitered_layer_footprints`].
///
/// `layer_refs_a` must be aligned 1:1 with `layers_a` and carries the
/// material_id/role_tag identity used to match `override_data`'s
/// `layer_pairs`. When `override_data` is `None` (or a layer has no
/// matching override and no `default_style` applies), the result for that
/// layer is byte-for-byte identical to [`mitered_layer_footprints`].
///
/// Per-layer precedence: matching `LayerPairOverride` > `default_style` >
/// automatic computation. See [`JoinOverrideStyle`] for how each style
/// translates into geometry.
pub fn mitered_layer_footprints_with_override(
    axis_a: &[(f64, f64)],
    layers_a: &[MiterLayer],
    layer_refs_a: &[LayerRef],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    end_b: Option<usize>,
    kind: JoinKind,
    override_data: Option<&JunctionOverride>,
) -> Vec<Option<Vec<(f64, f64)>>> {
    mitered_layer_footprints_with_override_and_bulges(
        axis_a,
        layers_a,
        layer_refs_a,
        end_a,
        axis_b,
        layers_b,
        end_b,
        kind,
        override_data,
        &[],
        &[],
    )
}

/// Override-aware miter using bulge-offset layer contours.
pub fn mitered_layer_footprints_with_override_and_bulges(
    axis_a: &[(f64, f64)],
    layers_a: &[MiterLayer],
    layer_refs_a: &[LayerRef],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    end_b: Option<usize>,
    kind: JoinKind,
    override_data: Option<&JunctionOverride>,
    bulges_a: &[f64],
    bulges_b: &[f64],
) -> Vec<Option<Vec<(f64, f64)>>> {
    let mut out = mitered_layer_footprints_with_bulges(
        axis_a, layers_a, end_a, axis_b, layers_b, end_b, kind, bulges_a, bulges_b,
    );
    let Some(ov) = override_data else {
        return out;
    };
    if axis_a.len() < 2 || axis_b.len() < 2 || layers_a.is_empty() || end_a >= axis_a.len() {
        return out;
    }

    let geom_a: Vec<(f64, f64)> = layers_a.iter().map(MiterLayer::as_geom).collect();
    let geom_b: Vec<(f64, f64)> = layers_b.iter().map(MiterLayer::as_geom).collect();
    let contours_a = contours_xy(axis_a, bulges_a, &geom_a);
    let contours_b = contours_xy(axis_b, bulges_b, &geom_b);
    if contours_a.is_empty() || contours_b.is_empty() {
        return out;
    }
    let pairing = match_layer_indices(layers_a, layers_b);
    let outer_target =
        through_outer_near_face_line(axis_a, end_a, axis_b, layers_b, &contours_b);

    for i in 0..layers_a.len() {
        let Some(layer_ref) = layer_refs_a.get(i) else {
            continue;
        };
        let Some(style) = resolve_layer_override_style(layer_ref, ov) else {
            continue;
        };
        if i >= contours_a.len() {
            continue;
        }
        let (ref a_b1, ref a_b2) = contours_a[i];
        let b_idx = pairing.get(i).copied().flatten().or_else(|| {
            closest_layer_index(&layers_a[i], layers_b)
        });
        let b_contour = b_idx.and_then(|j| contours_b.get(j));

        let new_fp = match style {
            // Keep the layer's own original (un-joined) boundary at `end_a`
            // instead of falling back to the whole-wall `corner_override`
            // extension the caller applies to unmatched (`None`) layers.
            JoinOverrideStyle::NoExtend => {
                if end_a < a_b1.len() && end_a < a_b2.len() {
                    Some(rebuild_footprint(a_b1, a_b2, end_a, a_b1[end_a], a_b2[end_a]))
                } else {
                    None
                }
            }
            JoinOverrideStyle::Miter => {
                if let (Some(eb), Some((b_b1, b_b2))) = (end_b, b_contour) {
                    miter_one_layer(a_b1, a_b2, end_a, b_b1, b_b2, Some(eb), axis_a, axis_b)
                } else {
                    out[i].clone()
                }
            }
            JoinOverrideStyle::Butt => {
                let target = if let Some((b_b1, b_b2)) = b_contour {
                    near_face_line(a_b1, a_b2, end_a, b_b1, b_b2, end_b, axis_a, axis_b)
                        .or(outer_target)
                } else {
                    outer_target
                };
                target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
            }
            JoinOverrideStyle::OuterFace => {
                outer_target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
            }
            JoinOverrideStyle::NearFace => {
                let target = if let Some((b_b1, b_b2)) = b_contour {
                    layer_face_line(
                        a_b1, a_b2, end_a, b_b1, b_b2, end_b, axis_a, axis_b, false,
                    )
                    .or(outer_target)
                } else {
                    outer_target
                };
                target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
            }
            JoinOverrideStyle::FarFace => {
                let target = if let Some((b_b1, b_b2)) = b_contour {
                    layer_face_line(
                        a_b1, a_b2, end_a, b_b1, b_b2, end_b, axis_a, axis_b, true,
                    )
                    .or_else(|| {
                        through_outer_face_line(
                            axis_a, end_a, axis_b, layers_b, &contours_b, true,
                        )
                    })
                } else {
                    through_outer_face_line(
                        axis_a, end_a, axis_b, layers_b, &contours_b, true,
                    )
                };
                target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
            }
        };

        if let Some(fp) = new_fp {
            if fp.len() >= 3 {
                out[i] = Some(fp);
            }
        }
    }
    out
}

/// T-junction footprints: the Structural/core stem layer butts the near
/// face of the paired through-wall Structural layer. Remaining stem layers
/// extend to the through-wall outer face on the approach side (no tunnel).
fn t_junction_layer_footprints(
    axis_a: &[(f64, f64)],
    layers_a: &[MiterLayer],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    contours_a: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    contours_b: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
) -> Vec<Option<Vec<(f64, f64)>>> {
    let outer_target =
        through_outer_near_face_line(axis_a, end_a, axis_b, layers_b, contours_b);
    let core_pair = pair_structural_cores(layers_a, layers_b);
    let mut out = Vec::with_capacity(layers_a.len());

    for i in 0..layers_a.len() {
        if i >= contours_a.len() {
            out.push(None);
            continue;
        }
        let (ref a_b1, ref a_b2) = contours_a[i];
        let fp = if let Some((core_a, core_b)) = core_pair {
            if i == core_a && core_b < contours_b.len() {
                let (ref b_b1, ref b_b2) = contours_b[core_b];
                let target = near_face_line(a_b1, a_b2, end_a, b_b1, b_b2, None, axis_a, axis_b)
                    .or(outer_target);
                target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
            } else if let Some(j) = approach_finish_match(i, layers_a, layers_b, core_pair, axis_a, end_a, axis_b)
            {
                if j < contours_b.len() {
                    let (ref b_b1, ref b_b2) = contours_b[j];
                    t_miter_finish_against_through(
                        a_b1, a_b2, end_a, b_b1, b_b2, axis_a, axis_b,
                    )
                } else {
                    outer_target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
                }
            } else {
                outer_target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
            }
        } else {
            outer_target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
        };

        match fp {
            Some(poly) if poly.len() >= 3 => out.push(Some(poly)),
            _ => out.push(None),
        }
    }
    out
}

fn approach_finish_match(
    stem_i: usize,
    layers_a: &[MiterLayer],
    layers_b: &[MiterLayer],
    core_pair: Option<(usize, usize)>,
    axis_a: &[(f64, f64)],
    end_a: usize,
    axis_b: &[(f64, f64)],
) -> Option<usize> {
    let stem = layers_a.get(stem_i)?;
    if is_structural_function(&stem.function) {
        return None;
    }
    if stem.material.is_empty() {
        return None;
    }
    let core_b = core_pair.map(|(_, b)| b);
    let toward_stem = through_offset_toward_stem(axis_a, end_a, axis_b);
    let core_center = core_b.map(|j| {
        let l = &layers_b[j];
        l.axis_offset + l.thickness * 0.5
    });
    let mut best: Option<(usize, f64)> = None;
    for (j, thru) in layers_b.iter().enumerate() {
        if Some(j) == core_b || is_structural_function(&thru.function) {
            continue;
        }
        if !thru.material.eq_ignore_ascii_case(&stem.material) {
            continue;
        }
        let center = thru.axis_offset + thru.thickness * 0.5;
        if let Some(cc) = core_center {
            if (center - cc) * toward_stem < -1e-9 {
                continue;
            }
        }
        let d = center * toward_stem;
        match best {
            None => best = Some((j, d)),
            Some((_, bd)) if d > bd => best = Some((j, d)),
            _ => {}
        }
    }
    best.map(|(j, _)| j)
}

fn through_offset_toward_stem(
    axis_a: &[(f64, f64)],
    end_a: usize,
    axis_b: &[(f64, f64)],
) -> f64 {
    if axis_a.len() < 2 || axis_b.len() < 2 || end_a >= axis_a.len() {
        return 1.0;
    }
    let join = axis_a[end_a];
    let prev = if end_a == 0 { 1 } else { end_a - 1 };
    let stem = (axis_a[prev].0 - join.0, axis_a[prev].1 - join.1);
    let mut best_seg = 0usize;
    let mut best_d = f64::INFINITY;
    for s in 0..axis_b.len().saturating_sub(1) {
        let d = point_seg_dist(join, axis_b[s], axis_b[s + 1]);
        if d < best_d {
            best_d = d;
            best_seg = s;
        }
    }
    let b0 = axis_b[best_seg];
    let b1 = axis_b[best_seg + 1];
    let bx = b1.0 - b0.0;
    let by = b1.1 - b0.1;
    let bl = (bx * bx + by * by).sqrt();
    if bl < 1e-12 {
        return 1.0;
    }
    let nx = -by / bl;
    let ny = bx / bl;
    let side = nx * stem.0 + ny * stem.1;
    if side >= 0.0 {
        1.0
    } else {
        -1.0
    }
}

fn t_miter_finish_against_through(
    a_b1: &[(f64, f64)],
    a_b2: &[(f64, f64)],
    end_a: usize,
    b_b1: &[(f64, f64)],
    b_b2: &[(f64, f64)],
    axis_a: &[(f64, f64)],
    axis_b: &[(f64, f64)],
) -> Option<Vec<(f64, f64)>> {
    if a_b1.len() < 2 || a_b2.len() < 2 || end_a >= a_b1.len() {
        return None;
    }
    let (b1_line, b2_line) = other_boundary_lines(b_b1, b_b2, None, axis_a, axis_b, end_a)?;
    let outer_b = near_face_line(a_b1, a_b2, end_a, b_b1, b_b2, None, axis_a, axis_b)?;
    let inner_b = if lines_equivalent(outer_b, b1_line) {
        b2_line
    } else {
        b1_line
    };
    let prev_a = if end_a == 0 { 1 } else { end_a - 1 };
    let a1_line = extended_line(a_b1[prev_a], a_b1[end_a]);
    let a2_line = extended_line(a_b2[prev_a], a_b2[end_a]);
    // Inner stem edge is closer to the wall axis; outer is farther out.
    let d1 = point_axis_offset(a_b1[end_a], axis_a);
    let d2 = point_axis_offset(a_b2[end_a], axis_a);
    let (inner_a, outer_a) = if d1 <= d2 {
        (a1_line, a2_line)
    } else {
        (a2_line, a1_line)
    };
    let new_inner = intersect_lines_2d(inner_a.0, inner_a.1, inner_b.0, inner_b.1)?;
    let new_outer = intersect_lines_2d(outer_a.0, outer_a.1, outer_b.0, outer_b.1)?;
    let (new_a1, new_a2) = if d1 <= d2 {
        (new_inner, new_outer)
    } else {
        (new_outer, new_inner)
    };
    Some(rebuild_footprint(a_b1, a_b2, end_a, new_a1, new_a2))
}

fn point_axis_offset(p: (f64, f64), axis: &[(f64, f64)]) -> f64 {
    if axis.len() < 2 {
        return 0.0;
    }
    let mut best = f64::INFINITY;
    for s in 0..axis.len() - 1 {
        let d = point_seg_dist(p, axis[s], axis[s + 1]);
        if d < best {
            best = d;
        }
    }
    best
}

fn lines_equivalent(
    a: ((f64, f64), (f64, f64)),
    b: ((f64, f64), (f64, f64)),
) -> bool {
    let da = point_seg_dist(a.0, b.0, b.1);
    let db = point_seg_dist(a.1, b.0, b.1);
    da < 1e-6 && db < 1e-6
}

fn is_structural_function(function: &str) -> bool {
    let f = function.trim();
    f.eq_ignore_ascii_case("structural") || f.eq_ignore_ascii_case("tragwerk")
}

/// Pair Structural cores: among Structural layers, prefer material match
/// (`match_layer_indices` restricted to those); else the thickest Structural
/// on each wall. With no Structural, fall back to the thickest layer.
fn pair_structural_cores(layers_a: &[MiterLayer], layers_b: &[MiterLayer]) -> Option<(usize, usize)> {
    let struct_a: Vec<usize> = layers_a
        .iter()
        .enumerate()
        .filter(|(_, l)| is_structural_function(&l.function))
        .map(|(i, _)| i)
        .collect();
    let struct_b: Vec<usize> = layers_b
        .iter()
        .enumerate()
        .filter(|(_, l)| is_structural_function(&l.function))
        .map(|(i, _)| i)
        .collect();

    if !struct_a.is_empty() && !struct_b.is_empty() {
        let sub_a: Vec<MiterLayer> = struct_a.iter().map(|&i| layers_a[i].clone()).collect();
        let sub_b: Vec<MiterLayer> = struct_b.iter().map(|&i| layers_b[i].clone()).collect();
        let pairing = match_layer_indices(&sub_a, &sub_b);
        for (local_a, maybe_b) in pairing.into_iter().enumerate() {
            if let Some(local_b) = maybe_b {
                return Some((struct_a[local_a], struct_b[local_b]));
            }
        }
        let thickest = |idxs: &[usize], layers: &[MiterLayer]| {
            idxs.iter()
                .copied()
                .max_by(|i, j| {
                    layers[*i]
                        .thickness
                        .partial_cmp(&layers[*j].thickness)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
        };
        return Some((thickest(&struct_a, layers_a)?, thickest(&struct_b, layers_b)?));
    }

    if layers_a.is_empty() || layers_b.is_empty() {
        return None;
    }
    let thickest = |layers: &[MiterLayer]| {
        layers
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                a.thickness
                    .partial_cmp(&b.thickness)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
    };
    Some((thickest(layers_a)?, thickest(layers_b)?))
}

/// Pair each layer of wall A to at most one layer of wall B.
///
/// Matching priority:
/// 1. identical non-nil `layer_id`
/// 2. `material` equality (and `function` equality when both are non-empty)
/// 3. closest offset-from-axis (tie-break / no material match)
///
/// A layer is left unmatched (`None`) when no unique candidate remains —
/// those fall back to the single-vertex corner extension at the caller.
pub fn match_layer_indices(layers_a: &[MiterLayer], layers_b: &[MiterLayer]) -> Vec<Option<usize>> {
    let n_a = layers_a.len();
    let n_b = layers_b.len();
    if n_a == 0 {
        return Vec::new();
    }
    if n_b == 0 {
        return vec![None; n_a];
    }

    let centers_a = layer_center_offsets(layers_a);
    let centers_b = layer_center_offsets(layers_b);
    let mut used_b = vec![false; n_b];
    let mut out = vec![None; n_a];
    let mut done_a = vec![false; n_a];

    // Greedy global assignment: repeatedly take the best remaining (i, j)
    // pair by (identity class, offset distance). Ambiguous A layers (two B
    // candidates with the same class and equal distance) stay unmatched.
    loop {
        let mut best: Option<(u8, f64, usize, usize)> = None; // (class, dist, i, j)
        for i in 0..n_a {
            if done_a[i] {
                continue;
            }
            for j in 0..n_b {
                if used_b[j] {
                    continue;
                }
                let class = identity_class(&layers_a[i], &layers_b[j]);
                let dist = (centers_a[i] - centers_b[j]).abs();
                match best {
                    None => best = Some((class, dist, i, j)),
                    Some((bc, bd, _, _)) => {
                        if class < bc || (class == bc && dist < bd - 1e-12) {
                            best = Some((class, dist, i, j));
                        }
                    }
                }
            }
        }
        let Some((class, dist, i, j)) = best else {
            break;
        };

        let mut ambiguous = false;
        for j2 in 0..n_b {
            if j2 == j || used_b[j2] {
                continue;
            }
            let c2 = identity_class(&layers_a[i], &layers_b[j2]);
            if c2 != class {
                continue;
            }
            let d2 = (centers_a[i] - centers_b[j2]).abs();
            if (d2 - dist).abs() < 1e-9 {
                ambiguous = true;
                break;
            }
        }
        if class >= 3 {
            // Offset-only pairs are not identity. Remaining layers are
            // matched by stack index below so different materials still join
            // when they occupy the same slot.
            break;
        }
        done_a[i] = true;
        if ambiguous {
            // Leave out[i] = None (fallback to corner_override).
            continue;
        }
        out[i] = Some(j);
        used_b[j] = true;
    }

    for i in 0..n_a {
        if out[i].is_some() {
            continue;
        }
        if i < n_b && !used_b[i] {
            out[i] = Some(i);
            used_b[i] = true;
        }
    }

    out
}

/// Identity match class: 0 = same layer_id, 1 = material+function,
/// 2 = material only, 3 = offset-only. Lower is better.
fn identity_class(a: &MiterLayer, b: &MiterLayer) -> u8 {
    if !a.layer_id.is_nil() && a.layer_id == b.layer_id {
        return 0;
    }
    let mat = !a.material.is_empty() && a.material == b.material;
    if !mat {
        return 3;
    }
    let fun = !a.function.is_empty() && !b.function.is_empty() && a.function == b.function;
    if fun {
        1
    } else {
        2
    }
}

/// Centre offset of each layer from the reference axis:
/// `center = axis_offset + thickness/2` (direct, no stacking).
fn layer_center_offsets(layers: &[MiterLayer]) -> Vec<f64> {
    layers
        .iter()
        .map(|l| l.axis_offset + l.thickness * 0.5)
        .collect()
}

/// One wall's geometry at a multi-wall junction (axis already snapped).
#[derive(Debug, Clone)]
pub struct JunctionWallGeom {
    pub axis: Vec<(f64, f64)>,
    pub layers: Vec<MiterLayer>,
    /// `Some(end_idx)` when this wall ends at the junction; `None` for a
    /// through-wall (T stem target) that only participates as a miter partner.
    pub end: Option<usize>,
}

/// Resolve per-layer mitered footprints for every wall at a multi-wall junction.
///
/// Endpoint walls are ordered by outgoing angle around the junction. Each is
/// mitered against its two angular neighbors (reusing [`match_layer_indices`]
/// pairwise). Through-walls only act as miter targets and receive an all-`None`
/// result (rectangular fallback at the caller). When a layer cannot be matched
/// on either side the entry stays `None` (single-vertex `corner_override`).
///
/// `walls` must be aligned with `junction.participants` (same length/order) —
/// typically built after [`super::join::apply_junction_to_axes`].
pub fn mitered_junction_layer_footprints(
    junction: &Junction,
    walls: &[JunctionWallGeom],
) -> Vec<Vec<Option<Vec<(f64, f64)>>>> {
    let n = walls.len();
    let mut out: Vec<Vec<Option<Vec<(f64, f64)>>>> = walls
        .iter()
        .map(|w| vec![None; w.layers.len()])
        .collect();
    if n == 0 || junction.participants.len() != n {
        return out;
    }

    // Fast path: fewer than 2 endpoint walls → nothing to miter together.
    let endpoint_indices: Vec<usize> = walls
        .iter()
        .enumerate()
        .filter_map(|(i, w)| w.end.map(|_| i))
        .collect();
    if endpoint_indices.is_empty() {
        return out;
    }

    // Two-wall (or one endpoint + through) → reuse pairwise helper.
    if n == 2 || (endpoint_indices.len() == 1 && n == 2) {
        for &ei in &endpoint_indices {
            let other = 1 - ei;
            let self_w = &walls[ei];
            let other_w = &walls[other];
            let Some(end_a) = self_w.end else { continue };
            let kind = if other_w.end.is_some() {
                JoinKind::L
            } else {
                JoinKind::T
            };
            out[ei] = mitered_layer_footprints(
                &self_w.axis,
                &self_w.layers,
                end_a,
                &other_w.axis,
                &other_w.layers,
                other_w.end,
                kind,
            );
        }
        return out;
    }

    // Build axis refs for ray ordering (DVec3-less 2D → temporary DVec3 via join helpers).
    // junction_rays expects &[&[DVec3]]; we work purely in 2D here and sort by angle ourselves.
    let jp = (junction.point.x, junction.point.y);
    let rays = ordered_junction_rays_2d(jp, walls);
    if rays.len() < 2 {
        return out;
    }

    // Pre-compute contours per wall.
    let contours: Vec<Vec<(Vec<(f64, f64)>, Vec<(f64, f64)>)>> = walls
        .iter()
        .map(|w| {
            let geom: Vec<(f64, f64)> = w.layers.iter().map(MiterLayer::as_geom).collect();
            layer_contours(&w.axis, &geom)
        })
        .collect();

    // For each ray that belongs to an endpoint wall, miter against angular neighbors.
    // Multiple rays can reference the same through-wall participant; endpoint walls
    // have exactly one ray.
    for (ri, ray) in rays.iter().enumerate() {
        let wi = ray.wall_index;
        let Some(end_a) = walls[wi].end else {
            // Through-wall ray: not rebuilding its own footprint.
            continue;
        };
        let n_rays = rays.len();
        let prev = &rays[(ri + n_rays - 1) % n_rays];
        let next = &rays[(ri + 1) % n_rays];
        // Skip self-adjacent (shouldn't happen with ≥2 distinct rays).
        if prev.wall_index == wi && next.wall_index == wi {
            continue;
        }

        // b1 = negative-offset side = CW / right of leave dir → prev in CCW order.
        // b2 = positive-offset side = CCW / left of leave dir → next in CCW order.
        let right_w = prev.wall_index;
        let left_w = next.wall_index;
        let right_collinear = is_collinear_opposite_endpoint(&walls[wi], &walls[right_w]);
        let left_collinear = is_collinear_opposite_endpoint(&walls[wi], &walls[left_w]);

        let pair_right = match_layer_indices(&walls[wi].layers, &walls[right_w].layers);
        let pair_left = match_layer_indices(&walls[wi].layers, &walls[left_w].layers);

        for li in 0..walls[wi].layers.len() {
            if li >= contours[wi].len() {
                out[wi][li] = None;
                continue;
            }
            let (ref a_b1, ref a_b2) = contours[wi][li];

            // Resolve each side independently; if only one neighbor matches,
            // miter both sides against that neighbor (same as pairwise).
            let right_j = pair_right.get(li).copied().flatten();
            let left_j = pair_left.get(li).copied().flatten();

            let fp = match (right_j, left_j) {
                (Some(rj), Some(_lj)) if right_w == left_w => {
                    // Both neighbors are the same wall (classic T against through).
                    if rj >= contours[right_w].len() {
                        None
                    } else {
                        let (ref b_b1, ref b_b2) = contours[right_w][rj];
                        miter_one_layer(
                            a_b1,
                            a_b2,
                            end_a,
                            b_b1,
                            b_b2,
                            walls[right_w].end,
                            &walls[wi].axis,
                            &walls[right_w].axis,
                        )
                    }
                }
                (Some(rj), Some(lj)) => {
                    if rj >= contours[right_w].len() || lj >= contours[left_w].len() {
                        None
                    } else {
                        let (ref r_b1, ref r_b2) = contours[right_w][rj];
                        let (ref l_b1, ref l_b2) = contours[left_w][lj];
                        let ends_r = miter_end_points(
                            a_b1,
                            a_b2,
                            end_a,
                            r_b1,
                            r_b2,
                            walls[right_w].end,
                            &walls[wi].axis,
                            &walls[right_w].axis,
                        );
                        let ends_l = miter_end_points(
                            a_b1,
                            a_b2,
                            end_a,
                            l_b1,
                            l_b2,
                            walls[left_w].end,
                            &walls[wi].axis,
                            &walls[left_w].axis,
                        );
                        match (ends_r, ends_l) {
                            // b1 from right neighbor, b2 from left neighbor.
                            (Some((new_a1, _)), Some((_, new_a2))) => {
                                Some(rebuild_footprint(a_b1, a_b2, end_a, new_a1, new_a2))
                            }
                            (Some((new_a1, new_a2)), None) if !left_collinear => {
                                Some(rebuild_footprint(a_b1, a_b2, end_a, new_a1, new_a2))
                            }
                            (None, Some((new_a1, new_a2))) if !right_collinear => {
                                Some(rebuild_footprint(a_b1, a_b2, end_a, new_a1, new_a2))
                            }
                            // Collinear opposite neighbor (180°): keep that
                            // side square so two same-axis walls do not both
                            // L-miter into the same square.
                            (Some((new_a1, _)), None) if left_collinear => {
                                Some(rebuild_footprint(a_b1, a_b2, end_a, new_a1, a_b2[end_a]))
                            }
                            (None, Some((_, new_a2))) if right_collinear => {
                                Some(rebuild_footprint(a_b1, a_b2, end_a, a_b1[end_a], new_a2))
                            }
                            _ => None,
                        }
                    }
                }
                (Some(rj), None) => {
                    if rj >= contours[right_w].len() {
                        None
                    } else {
                        let (ref b_b1, ref b_b2) = contours[right_w][rj];
                        miter_one_layer(
                            a_b1,
                            a_b2,
                            end_a,
                            b_b1,
                            b_b2,
                            walls[right_w].end,
                            &walls[wi].axis,
                            &walls[right_w].axis,
                        )
                    }
                }
                (None, Some(lj)) => {
                    if lj >= contours[left_w].len() {
                        None
                    } else {
                        let (ref b_b1, ref b_b2) = contours[left_w][lj];
                        miter_one_layer(
                            a_b1,
                            a_b2,
                            end_a,
                            b_b1,
                            b_b2,
                            walls[left_w].end,
                            &walls[wi].axis,
                            &walls[left_w].axis,
                        )
                    }
                }
                (None, None) => {
                    // No material match on either side. If one of the angular
                    // neighbors is a through-wall, extend to its outer face
                    // (mirrors the pairwise `t_junction_layer_footprints` path).
                    let through_w = if walls[right_w].end.is_none() {
                        Some(right_w)
                    } else if walls[left_w].end.is_none() {
                        Some(left_w)
                    } else {
                        None
                    };
                    through_w.and_then(|tw| {
                        let outer = through_outer_near_face_line(
                            &walls[wi].axis,
                            end_a,
                            &walls[tw].axis,
                            &walls[tw].layers,
                            &contours[tw],
                        )?;
                        t_extend_one_layer(a_b1, a_b2, end_a, outer)
                    })
                }
            };

            out[wi][li] = fp.filter(|p| p.len() >= 3);
        }
    }

    out
}

/// Override-aware variant of [`mitered_junction_layer_footprints`].
///
/// `layer_refs` and `overrides` must be aligned with `walls` /
/// `junction.participants` (one entry per participant). `layer_refs[wi]`
/// carries the material_id/role_tag identity for `walls[wi].layers`;
/// `overrides[wi]` is the [`JunctionOverride`] stored for that wall's end at
/// this junction (`None` when no override exists).
///
/// Endpoint walls with an override recompute overridden layers against
/// their angular right-hand neighbor (mirroring the automatic path's
/// pairwise fallback); layers without a matching override — and walls
/// without any override at all — stay byte-for-byte identical to
/// [`mitered_junction_layer_footprints`].
pub fn mitered_junction_layer_footprints_with_overrides(
    junction: &Junction,
    walls: &[JunctionWallGeom],
    layer_refs: &[Vec<LayerRef>],
    overrides: &[Option<JunctionOverride>],
) -> Vec<Vec<Option<Vec<(f64, f64)>>>> {
    let mut out = mitered_junction_layer_footprints(junction, walls);
    let n = walls.len();
    if n == 0 || junction.participants.len() != n {
        return out;
    }

    let jp = (junction.point.x, junction.point.y);
    let rays = ordered_junction_rays_2d(jp, walls);
    if rays.len() < 2 {
        return out;
    }

    let contours: Vec<Vec<(Vec<(f64, f64)>, Vec<(f64, f64)>)>> = walls
        .iter()
        .map(|w| {
            let geom: Vec<(f64, f64)> = w.layers.iter().map(MiterLayer::as_geom).collect();
            layer_contours(&w.axis, &geom)
        })
        .collect();

    for (ri, ray) in rays.iter().enumerate() {
        let wi = ray.wall_index;
        let Some(end_a) = walls[wi].end else {
            continue;
        };
        let Some(ov) = overrides.get(wi).and_then(|o| o.as_ref()) else {
            continue;
        };
        let Some(refs) = layer_refs.get(wi) else {
            continue;
        };
        let n_rays = rays.len();
        let prev = &rays[(ri + n_rays - 1) % n_rays];
        let other_w = prev.wall_index;
        if other_w == wi {
            continue;
        }
        let other_axis = &walls[other_w].axis;
        let other_contours = &contours[other_w];
        let pairing = match_layer_indices(&walls[wi].layers, &walls[other_w].layers);
        let outer_target = through_outer_near_face_line(
            &walls[wi].axis,
            end_a,
            other_axis,
            &walls[other_w].layers,
            other_contours,
        );

        for li in 0..walls[wi].layers.len() {
            let Some(layer_ref) = refs.get(li) else {
                continue;
            };
            let Some(style) = resolve_layer_override_style(layer_ref, ov) else {
                continue;
            };
            if li >= contours[wi].len() {
                continue;
            }
            let (ref a_b1, ref a_b2) = contours[wi][li];
            let b_idx = pairing.get(li).copied().flatten();
            let b_contour = b_idx.and_then(|j| other_contours.get(j));

            let new_fp = match style {
                // Keep the layer's own original (un-joined) boundary at
                // `end_a` instead of falling back to the whole-wall
                // `corner_override` extension applied to unmatched layers.
                JoinOverrideStyle::NoExtend => {
                    if end_a < a_b1.len() && end_a < a_b2.len() {
                        Some(rebuild_footprint(a_b1, a_b2, end_a, a_b1[end_a], a_b2[end_a]))
                    } else {
                        None
                    }
                }
                JoinOverrideStyle::Miter => {
                    if let (Some(eb), Some((b_b1, b_b2))) = (walls[other_w].end, b_contour) {
                        miter_one_layer(
                            a_b1,
                            a_b2,
                            end_a,
                            b_b1,
                            b_b2,
                            Some(eb),
                            &walls[wi].axis,
                            other_axis,
                        )
                    } else {
                        out[wi][li].clone()
                    }
                }
                JoinOverrideStyle::Butt => {
                    let target = if let Some((b_b1, b_b2)) = b_contour {
                        near_face_line(
                            a_b1,
                            a_b2,
                            end_a,
                            b_b1,
                            b_b2,
                            walls[other_w].end,
                            &walls[wi].axis,
                            other_axis,
                        )
                        .or(outer_target)
                    } else {
                        outer_target
                    };
                    target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
                }
                JoinOverrideStyle::OuterFace => {
                    outer_target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
                }
                JoinOverrideStyle::NearFace => {
                    let target = if let Some((b_b1, b_b2)) = b_contour {
                        layer_face_line(
                            a_b1,
                            a_b2,
                            end_a,
                            b_b1,
                            b_b2,
                            walls[other_w].end,
                            &walls[wi].axis,
                            other_axis,
                            false,
                        )
                        .or(outer_target)
                    } else {
                        outer_target
                    };
                    target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
                }
                JoinOverrideStyle::FarFace => {
                    let target = if let Some((b_b1, b_b2)) = b_contour {
                        layer_face_line(
                            a_b1,
                            a_b2,
                            end_a,
                            b_b1,
                            b_b2,
                            walls[other_w].end,
                            &walls[wi].axis,
                            other_axis,
                            true,
                        )
                        .or_else(|| {
                            through_outer_face_line(
                                &walls[wi].axis,
                                end_a,
                                other_axis,
                                &walls[other_w].layers,
                                other_contours,
                                true,
                            )
                        })
                    } else {
                        through_outer_face_line(
                            &walls[wi].axis,
                            end_a,
                            other_axis,
                            &walls[other_w].layers,
                            other_contours,
                            true,
                        )
                    };
                    target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
                }
            };

            if let Some(fp) = new_fp {
                if fp.len() >= 3 {
                    out[wi][li] = Some(fp);
                }
            }
        }
    }

    out
}

/// Build [`JunctionWallGeom`] list aligned with `junction.participants` from
/// already-snapped axis polylines (`axes[wall_index]`) and layer stacks.
pub fn junction_wall_geoms(
    junction: &Junction,
    axes: &[Vec<(f64, f64)>],
    layers: &[Vec<MiterLayer>],
) -> Vec<JunctionWallGeom> {
    junction
        .participants
        .iter()
        .map(|p| {
            let axis = axes
                .get(p.wall_index)
                .cloned()
                .unwrap_or_default();
            let ly = layers
                .get(p.wall_index)
                .cloned()
                .unwrap_or_default();
            let end = match p.role {
                JunctionRole::Endpoint(e) => Some(e),
                JunctionRole::Through(_) => None,
            };
            JunctionWallGeom {
                axis,
                layers: ly,
                end,
            }
        })
        .collect()
}

#[derive(Clone, Copy)]
struct Ray2 {
    wall_index: usize,
    angle: f64,
}

/// CCW-sorted outgoing rays at a junction (through-walls contribute two).
fn ordered_junction_rays_2d(jp: (f64, f64), walls: &[JunctionWallGeom]) -> Vec<Ray2> {
    let mut rays = Vec::new();
    for (wi, w) in walls.iter().enumerate() {
        if w.axis.len() < 2 {
            continue;
        }
        if let Some(end) = w.end {
            if end >= w.axis.len() {
                continue;
            }
            let interior = if end == 0 { 1 } else { end - 1 };
            let dx = w.axis[interior].0 - jp.0;
            let dy = w.axis[interior].1 - jp.1;
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-12 {
                continue;
            }
            rays.push(Ray2 {
                wall_index: wi,
                angle: dy.atan2(dx),
            });
        } else {
            // Through-wall: find segment closest to junction and emit two rays.
            let mut best_seg = 0usize;
            let mut best_d = f64::INFINITY;
            for s in 0..w.axis.len() - 1 {
                let d = point_seg_dist(jp, w.axis[s], w.axis[s + 1]);
                if d < best_d {
                    best_d = d;
                    best_seg = s;
                }
            }
            for target in [w.axis[best_seg], w.axis[best_seg + 1]] {
                let dx = target.0 - jp.0;
                let dy = target.1 - jp.1;
                let len = (dx * dx + dy * dy).sqrt();
                if len < 1e-12 {
                    continue;
                }
                rays.push(Ray2 {
                    wall_index: wi,
                    angle: dy.atan2(dx),
                });
            }
        }
    }
    rays.sort_by(|a, b| {
        a.angle
            .partial_cmp(&b.angle)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // Drop consecutive duplicate angles from the same wall.
    rays.dedup_by(|a, b| {
        a.wall_index == b.wall_index && (a.angle - b.angle).abs() < 1e-12
    });
    rays
}

/// Compute the two replaced end-cap points (new_a1 on b1, new_a2 on b2) for
/// one layer of wall A against wall B — shared by pairwise and N-way paths.
///
/// When `end_b` is `None` (B is a T through-wall), both end-caps butt against
/// the near face of B's layer (straight extension). Otherwise a diagonal
/// L-miter is formed.
fn miter_end_points(
    a_b1: &[(f64, f64)],
    a_b2: &[(f64, f64)],
    end_a: usize,
    b_b1: &[(f64, f64)],
    b_b2: &[(f64, f64)],
    end_b: Option<usize>,
    axis_a: &[(f64, f64)],
    axis_b: &[(f64, f64)],
) -> Option<((f64, f64), (f64, f64))> {
    if a_b1.len() < 2 || a_b2.len() < 2 || a_b1.len() != a_b2.len() {
        return None;
    }
    if end_a >= a_b1.len() {
        return None;
    }
    if b_b1.len() < 2 || b_b2.len() < 2 || b_b1.len() != b_b2.len() {
        return None;
    }

    // T through-wall partner: straight butt, not diagonal miter.
    if end_b.is_none() {
        let target = near_face_line(a_b1, a_b2, end_a, b_b1, b_b2, None, axis_a, axis_b)?;
        return t_extend_end_points(a_b1, a_b2, end_a, target);
    }

    let prev_a = if end_a == 0 { 1 } else { end_a - 1 };
    let a1_line = extended_line(a_b1[prev_a], a_b1[end_a]);
    let a2_line = extended_line(a_b2[prev_a], a_b2[end_a]);

    // Degenerate angle check: if the walls meet at a very shallow or very
    // sharp angle, the miter intersection point moves to infinity.
    // `end_b` is guaranteed `Some` here (the T through-wall case returned
    // early above), so `lb` is always the L/T-stem leave direction.
    let la = wall_leave_dir(axis_a, end_a);
    let lb = wall_leave_dir(axis_b, end_b.expect("end_b is Some past the T-join early return"));
    let det = la.0 * lb.1 - la.1 * lb.0;
    if det.abs() < MIN_MITER_SINE {
        return None;
    }

    let (b1_line, b2_line) = other_boundary_lines(b_b1, b_b2, end_b, axis_a, axis_b, end_a)?;

    let i_a1_b1 = intersect_lines_2d(a1_line.0, a1_line.1, b1_line.0, b1_line.1)?;
    let i_a1_b2 = intersect_lines_2d(a1_line.0, a1_line.1, b2_line.0, b2_line.1)?;
    let i_a2_b1 = intersect_lines_2d(a2_line.0, a2_line.1, b1_line.0, b1_line.1)?;
    let i_a2_b2 = intersect_lines_2d(a2_line.0, a2_line.1, b2_line.0, b2_line.1)?;

    let pair_direct = (i_a1_b1, i_a2_b2);
    let pair_cross = (i_a1_b2, i_a2_b1);

    // The correct pairing (`direct`: a1↔b1, a2↔b2, vs `cross`: a1↔b2,
    // a2↔b1) is a fixed geometric fact about which offset boundary
    // continues into which at the corner — it does NOT depend on layer
    // thickness/gap, so it must never be chosen by comparing the resulting
    // segment lengths (the historical approach): that heuristic only
    // happens to agree with the correct choice near 90°, where both
    // candidates are (near-)equidistant, but silently flips to the wrong
    // (beveled) pairing for acute angles where the wrong candidate becomes
    // shorter. See the `acute_*_deg_l_corner_*` regression tests below.
    //
    // `a1`/`b1` are each defined as the "right-hand" (negative) offset
    // relative to their own axis's stored point order (index 0 → last),
    // `a2`/`b2` the "left-hand" one (see `layer_contours`/`normal`). Whether
    // `direct` or `cross` is the true continuation flips both with the
    // corner's turn handedness (`la`/`lb`, the directions leaving the joint)
    // and with each axis's own stored point order relative to which end is
    // jointed (`fwd_a`/`fwd_b`, the axis's natural index-0→last direction).
    // Both `cross(la, lb)` and `cross(fwd_a, fwd_b)` individually flip sign
    // when swapping which wall is "self" vs "other" (or when an axis's
    // point order is reversed) — but their *product* does not, matching the
    // fact that "does a1 connect to b1" is a swap-independent physical fact.
    let fwd_a = {
        let (dx, dy) = (
            axis_a[axis_a.len() - 1].0 - axis_a[0].0,
            axis_a[axis_a.len() - 1].1 - axis_a[0].1,
        );
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-12 { (0.0, 0.0) } else { (dx / len, dy / len) }
    };
    let fwd_b = {
        let (dx, dy) = (
            axis_b[axis_b.len() - 1].0 - axis_b[0].0,
            axis_b[axis_b.len() - 1].1 - axis_b[0].1,
        );
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-12 { (0.0, 0.0) } else { (dx / len, dy / len) }
    };
    let cross_fwd = fwd_a.0 * fwd_b.1 - fwd_a.1 * fwd_b.0;
    let invariant = cross_fwd * det;
    let (new_a1, new_a2) = if invariant < 0.0 {
        pair_direct
    } else {
        pair_cross
    };
    Some((new_a1, new_a2))
}

/// Unit direction leaving a wall axis junction.
fn is_collinear_opposite_endpoint(a: &JunctionWallGeom, b: &JunctionWallGeom) -> bool {
    let (Some(ea), Some(eb)) = (a.end, b.end) else {
        return false;
    };
    let da = wall_leave_dir(&a.axis, ea);
    let db = wall_leave_dir(&b.axis, eb);
    da.0 * db.0 + da.1 * db.1 < -0.95
}

fn wall_leave_dir(axis: &[(f64, f64)], end: usize) -> (f64, f64) {
    if axis.len() < 2 || end >= axis.len() {
        return (0.0, 0.0);
    }
    let prev = if end == 0 { 1 } else { end - 1 };
    let (dx, dy) = (axis[prev].0 - axis[end].0, axis[prev].1 - axis[end].1);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-12 {
        (0.0, 0.0)
    } else {
        (dx / len, dy / len)
    }
}

fn rebuild_footprint(
    a_b1: &[(f64, f64)],
    a_b2: &[(f64, f64)],
    end_a: usize,
    new_a1: (f64, f64),
    new_a2: (f64, f64),
) -> Vec<(f64, f64)> {
    let n = a_b1.len();
    let mut footprint = Vec::with_capacity(n * 2);
    for i in 0..n {
        if i == end_a {
            footprint.push(new_a1);
        } else {
            footprint.push(a_b1[i]);
        }
    }
    for i in (0..n).rev() {
        if i == end_a {
            footprint.push(new_a2);
        } else {
            footprint.push(a_b2[i]);
        }
    }
    ensure_ccw(&mut footprint);
    footprint
}

fn ensure_ccw(ring: &mut Vec<(f64, f64)>) {
    if super::geometry::signed_area(ring) < 0.0 {
        ring.reverse();
    }
}

/// Split a footprint that may contain NaN separators into simple rings.
pub fn split_footprint_rings(fp: &[(f64, f64)]) -> Vec<Vec<(f64, f64)>> {
    let mut rings = Vec::new();
    let mut cur = Vec::new();
    for &p in fp {
        if !p.0.is_finite() || !p.1.is_finite() {
            if cur.len() >= 3 {
                rings.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        } else {
            cur.push(p);
        }
    }
    if cur.len() >= 3 {
        rings.push(cur);
    }
    if rings.is_empty() && fp.len() >= 3 && fp.iter().all(|p| p.0.is_finite() && p.1.is_finite()) {
        rings.push(fp.to_vec());
    }
    rings
}

/// Build a closed footprint for one layer of wall A, with the joined end
/// replaced by the diagonal miter against wall B's matching layer.
///
/// When `end_b` is `None` (T through-wall partner), uses straight butt
/// extension to the near face of B's layer instead of a diagonal miter.
fn miter_one_layer(
    a_b1: &[(f64, f64)],
    a_b2: &[(f64, f64)],
    end_a: usize,
    b_b1: &[(f64, f64)],
    b_b2: &[(f64, f64)],
    end_b: Option<usize>,
    axis_a: &[(f64, f64)],
    axis_b: &[(f64, f64)],
) -> Option<Vec<(f64, f64)>> {
    if end_b.is_none() {
        let target = near_face_line(a_b1, a_b2, end_a, b_b1, b_b2, None, axis_a, axis_b)?;
        return t_extend_one_layer(a_b1, a_b2, end_a, target);
    }
    let (new_a1, new_a2) =
        miter_end_points(a_b1, a_b2, end_a, b_b1, b_b2, end_b, axis_a, axis_b)?;
    Some(rebuild_footprint(a_b1, a_b2, end_a, new_a1, new_a2))
}

/// Straight-extend stem layer boundaries to a single target face line
/// (T-junction butt join). Both end-cap vertices lie on `target_line`.
fn t_extend_one_layer(
    a_b1: &[(f64, f64)],
    a_b2: &[(f64, f64)],
    end_a: usize,
    target_line: ((f64, f64), (f64, f64)),
) -> Option<Vec<(f64, f64)>> {
    let (new_a1, new_a2) = t_extend_end_points(a_b1, a_b2, end_a, target_line)?;
    Some(rebuild_footprint(a_b1, a_b2, end_a, new_a1, new_a2))
}

fn t_extend_end_points(
    a_b1: &[(f64, f64)],
    a_b2: &[(f64, f64)],
    end_a: usize,
    target_line: ((f64, f64), (f64, f64)),
) -> Option<((f64, f64), (f64, f64))> {
    if a_b1.len() < 2 || a_b2.len() < 2 || a_b1.len() != a_b2.len() {
        return None;
    }
    if end_a >= a_b1.len() {
        return None;
    }
    let prev_a = if end_a == 0 { 1 } else { end_a - 1 };
    let a1_line = extended_line(a_b1[prev_a], a_b1[end_a]);
    let a2_line = extended_line(a_b2[prev_a], a_b2[end_a]);
    let new_a1 = intersect_lines_2d(a1_line.0, a1_line.1, target_line.0, target_line.1)?;
    let new_a2 = intersect_lines_2d(a2_line.0, a2_line.1, target_line.0, target_line.1)?;
    Some((new_a1, new_a2))
}

/// Pick the face of through-wall layer B that the stem approaches first
/// (the near face). Both stem boundary lines butt against this single line.
fn near_face_line(
    a_b1: &[(f64, f64)],
    a_b2: &[(f64, f64)],
    end_a: usize,
    b_b1: &[(f64, f64)],
    b_b2: &[(f64, f64)],
    end_b: Option<usize>,
    axis_a: &[(f64, f64)],
    axis_b: &[(f64, f64)],
) -> Option<((f64, f64), (f64, f64))> {
    layer_face_line(a_b1, a_b2, end_a, b_b1, b_b2, end_b, axis_a, axis_b, false)
}

fn layer_face_line(
    _a_b1: &[(f64, f64)],
    _a_b2: &[(f64, f64)],
    end_a: usize,
    b_b1: &[(f64, f64)],
    b_b2: &[(f64, f64)],
    end_b: Option<usize>,
    axis_a: &[(f64, f64)],
    axis_b: &[(f64, f64)],
    far: bool,
) -> Option<((f64, f64), (f64, f64))> {
    let (b1_line, b2_line) = other_boundary_lines(b_b1, b_b2, end_b, axis_a, axis_b, end_a)?;
    let join_pt = *axis_a.get(end_a)?;
    let prev_a = if end_a == 0 { 1 } else { end_a - 1 };
    let stem_interior = *axis_a.get(prev_a)?;
    // Direction from the joint toward the stem body — the near face is the
    // boundary whose closest point to the joint lies further along this dir.
    let to_stem = (
        stem_interior.0 - join_pt.0,
        stem_interior.1 - join_pt.1,
    );
    let s1 = closest_point_on_line(join_pt, b1_line);
    let s2 = closest_point_on_line(join_pt, b2_line);
    let d1 = (s1.0 - join_pt.0) * to_stem.0 + (s1.1 - join_pt.1) * to_stem.1;
    let d2 = (s2.0 - join_pt.0) * to_stem.0 + (s2.1 - join_pt.1) * to_stem.1;
    let near_is_b1 = d1 >= d2;
    if far ^ near_is_b1 {
        Some(b1_line)
    } else {
        Some(b2_line)
    }
}

/// Outer face of the entire through-wall stack on the stem's approach side.
/// Used when a stem layer has no material match in the through wall.
fn through_outer_near_face_line(
    axis_a: &[(f64, f64)],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    contours_b: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
) -> Option<((f64, f64), (f64, f64))> {
    through_outer_face_line(axis_a, end_a, axis_b, layers_b, contours_b, false)
}

fn through_outer_face_line(
    axis_a: &[(f64, f64)],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    contours_b: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    far: bool,
) -> Option<((f64, f64), (f64, f64))> {
    if contours_b.is_empty() || axis_a.len() < 2 || end_a >= axis_a.len() {
        return None;
    }
    let mut min_i = 0usize;
    let mut max_i = 0usize;
    let mut min_off = f64::INFINITY;
    let mut max_off = f64::NEG_INFINITY;
    if layers_b.len() == contours_b.len() && !layers_b.is_empty() {
        for (i, l) in layers_b.iter().enumerate() {
            let start = l.axis_offset;
            let end = l.axis_offset + l.thickness;
            if start < min_off {
                min_off = start;
                min_i = i;
            }
            if end > max_off {
                max_off = end;
                max_i = i;
            }
        }
    } else {
        min_i = 0;
        max_i = contours_b.len() - 1;
    }
    let (ref first_b1, _) = contours_b[min_i];
    let (_, ref last_b2) = contours_b[max_i];
    layer_face_line(
        &[],
        &[],
        end_a,
        first_b1,
        last_b2,
        None,
        axis_a,
        axis_b,
        far,
    )
}

fn contours_xy(
    axis: &[(f64, f64)],
    bulges: &[f64],
    geom: &[(f64, f64)],
) -> Vec<(Vec<(f64, f64)>, Vec<(f64, f64)>)> {
    if bulges.iter().any(|b| b.abs() > 1e-12) {
        layer_contours_with_bulges(axis, bulges, geom)
            .into_iter()
            .map(|(a, b)| (a.points, b.points))
            .collect()
    } else {
        layer_contours(axis, geom)
    }
}

/// Through-wall footprints at a T-junction.
///
/// Approach-side through layers overlapped by the stem **core** receive a
/// C-notch; the through core (and far-side finish) stay rectangular (`None`).
pub fn through_wall_cutout_footprints(
    axis_through: &[(f64, f64)],
    layers_through: &[MiterLayer],
    axis_stem: &[(f64, f64)],
    layers_stem: &[MiterLayer],
    stem_end: usize,
) -> Vec<Option<Vec<(f64, f64)>>> {
    through_wall_cutout_footprints_with_bulges(
        axis_through,
        layers_through,
        axis_stem,
        layers_stem,
        stem_end,
        &[],
        &[],
    )
}

pub fn through_wall_cutout_footprints_with_bulges(
    axis_through: &[(f64, f64)],
    layers_through: &[MiterLayer],
    axis_stem: &[(f64, f64)],
    layers_stem: &[MiterLayer],
    stem_end: usize,
    bulges_through: &[f64],
    bulges_stem: &[f64],
) -> Vec<Option<Vec<(f64, f64)>>> {
    if axis_through.len() < 2 || axis_stem.len() < 2 || layers_through.is_empty() {
        return vec![None; layers_through.len()];
    }
    if stem_end >= axis_stem.len() {
        return vec![None; layers_through.len()];
    }
    let geom_t: Vec<(f64, f64)> = layers_through.iter().map(MiterLayer::as_geom).collect();
    let geom_s: Vec<(f64, f64)> = layers_stem.iter().map(MiterLayer::as_geom).collect();
    let contours_t = contours_xy(axis_through, bulges_through, &geom_t);
    let contours_s = contours_xy(axis_stem, bulges_stem, &geom_s);
    let Some((core_s, core_t)) = pair_structural_cores(layers_stem, layers_through) else {
        return vec![None; layers_through.len()];
    };
    if core_t >= contours_t.len() {
        return vec![None; layers_through.len()];
    }
    let (ref core_t1, ref core_t2) = contours_t[core_t];
    let toward = through_offset_toward_stem(axis_stem, stem_end, axis_through);
    let punchers = punchers_outside_in(layers_stem, layers_through, Some((core_s, core_t)), toward);
    let mut out = vec![None; layers_through.len()];
    for ti in 0..layers_through.len() {
        if ti == core_t || ti >= contours_t.len() {
            continue;
        }
        if is_structural_function(&layers_through[ti].function) {
            continue;
        }
        let (ref t1, ref t2) = contours_t[ti];
        if !through_layer_on_approach_side(
            t1, t2, core_t1, core_t2, axis_stem, stem_end,
        ) {
            continue;
        }
        let overlapping: Vec<usize> = punchers
            .get(ti)
            .map(|v| v.iter().copied().filter(|&si| si < contours_s.len()).collect())
            .unwrap_or_default();
        if overlapping.is_empty() {
            continue;
        }
        if let Some(fp) = notch_through_layer_from_stem_layers(
            t1,
            t2,
            &layers_through[ti],
            &overlapping,
            layers_stem,
            &contours_s,
            stem_end,
            axis_stem,
        ) {
            out[ti] = Some(fp);
        }
    }
    out
}

/// Leftmost and rightmost stem boundary polylines (full stem envelope).
#[allow(dead_code)]
fn stem_outer_boundaries(
    contours_s: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
) -> Option<(&[ (f64, f64) ], &[ (f64, f64) ])> {
    let first = contours_s.first()?;
    let last = contours_s.last()?;
    Some((first.0.as_slice(), last.1.as_slice()))
}

fn closest_layer_index(a: &MiterLayer, layers_b: &[MiterLayer]) -> Option<usize> {
    if layers_b.is_empty() {
        return None;
    }
    let ca = a.axis_offset + a.thickness * 0.5;
    let mut best = 0usize;
    let mut best_d = f64::INFINITY;
    for (i, b) in layers_b.iter().enumerate() {
        let cb = b.axis_offset + b.thickness * 0.5;
        let d = (ca - cb).abs();
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    Some(best)
}

fn contour_centroid(a: &[(f64, f64)], b: &[(f64, f64)]) -> (f64, f64) {
    let mut sx = 0.0;
    let mut sy = 0.0;
    let mut n = 0.0;
    for p in a.iter().chain(b.iter()) {
        sx += p.0;
        sy += p.1;
        n += 1.0;
    }
    if n < 1.0 {
        (0.0, 0.0)
    } else {
        (sx / n, sy / n)
    }
}

fn stem_to_body(axis_stem: &[(f64, f64)], stem_end: usize) -> Option<(f64, f64)> {
    if stem_end >= axis_stem.len() || axis_stem.len() < 2 {
        return None;
    }
    let prev = if stem_end == 0 { 1 } else { stem_end - 1 };
    let join = axis_stem[stem_end];
    let interior = axis_stem[prev];
    Some((interior.0 - join.0, interior.1 - join.1))
}

fn through_layer_on_approach_side(
    t1: &[(f64, f64)],
    t2: &[(f64, f64)],
    core1: &[(f64, f64)],
    core2: &[(f64, f64)],
    axis_stem: &[(f64, f64)],
    stem_end: usize,
) -> bool {
    let Some(to_stem) = stem_to_body(axis_stem, stem_end) else {
        return false;
    };
    let layer = contour_centroid(t1, t2);
    let core = contour_centroid(core1, core2);
    (layer.0 - core.0) * to_stem.0 + (layer.1 - core.1) * to_stem.1 > 1e-9
}

fn line_param(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-24 {
        0.0
    } else {
        ((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2
    }
}

/// Pair stem vs through from the approach outer face inward:
/// same material → miter at this through layer (consumed, does not punch deeper);
/// different material → continue inward if a same-material through layer (or the
/// core) still exists, otherwise butt here.
fn punchers_outside_in(
    layers_stem: &[MiterLayer],
    layers_through: &[MiterLayer],
    core_pair: Option<(usize, usize)>,
    toward_stem: f64,
) -> Vec<Vec<usize>> {
    let n_t = layers_through.len();
    let mut out = vec![Vec::new(); n_t];
    let core_t = core_pair.map(|(_, t)| t);
    let core_s = core_pair.map(|(s, _)| s);
    let mut order: Vec<usize> = (0..n_t).collect();
    order.sort_by(|&a, &b| {
        let da = layer_outerness(&layers_through[a], toward_stem);
        let db = layer_outerness(&layers_through[b], toward_stem);
        db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut consumed = vec![false; layers_stem.len()];
    for ti in order {
        if Some(ti) == core_t || is_structural_function(&layers_through[ti].function) {
            continue;
        }
        let thru = &layers_through[ti];
        let mut punch = Vec::new();
        for si in 0..layers_stem.len() {
            if consumed[si] {
                continue;
            }
            let stem = &layers_stem[si];
            let same_mat = !stem.material.is_empty()
                && stem.material.eq_ignore_ascii_case(&thru.material);
            if same_mat && !is_structural_function(&stem.function) {
                punch.push(si);
                consumed[si] = true;
                continue;
            }
            let deeper_same = layers_through.iter().enumerate().any(|(j, l)| {
                if j == ti || Some(j) == core_t {
                    return false;
                }
                if layer_outerness(l, toward_stem) >= layer_outerness(thru, toward_stem) - 1e-12 {
                    return false;
                }
                !stem.material.is_empty() && stem.material.eq_ignore_ascii_case(&l.material)
            });
            let is_core = core_s == Some(si);
            if is_core || deeper_same {
                punch.push(si);
            } else {
                punch.push(si);
                consumed[si] = true;
            }
        }
        out[ti] = punch;
    }
    out
}

fn layer_outerness(layer: &MiterLayer, toward_stem: f64) -> f64 {
    if toward_stem >= 0.0 {
        layer.axis_offset + layer.thickness
    } else {
        -layer.axis_offset
    }
}

fn notch_through_layer_from_stem_layers(
    t_b1: &[(f64, f64)],
    t_b2: &[(f64, f64)],
    thru: &MiterLayer,
    overlapping: &[usize],
    layers_stem: &[MiterLayer],
    contours_s: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    stem_end: usize,
    axis_stem: &[(f64, f64)],
) -> Option<Vec<(f64, f64)>> {
    if overlapping.is_empty() {
        return None;
    }
    let Some(to_stem) = stem_to_body(axis_stem, stem_end) else {
        return None;
    };
    let join = *axis_stem.get(stem_end)?;
    let t1_line = (*t_b1.first()?, *t_b1.last()?);
    let t2_line = (*t_b2.first()?, *t_b2.last()?);
    let s1 = closest_point_on_line(join, t1_line);
    let s2 = closest_point_on_line(join, t2_line);
    let d1 = (s1.0 - join.0) * to_stem.0 + (s1.1 - join.1) * to_stem.1;
    let d2 = (s2.0 - join.0) * to_stem.0 + (s2.1 - join.1) * to_stem.1;
    let (near, far) = if d1 >= d2 {
        (t_b1, t_b2)
    } else {
        (t_b2, t_b1)
    };
    let near_line = (*near.first()?, *near.last()?);
    let far_line = (*far.first()?, *far.last()?);

    let mut hits: Vec<(f64, usize, bool)> = Vec::new();
    for &si in overlapping {
        let (ref b1, ref b2) = contours_s[si];
        if stem_end >= b1.len() || stem_end >= b2.len() {
            continue;
        }
        let prev = if stem_end == 0 { 1 } else { stem_end - 1 };
        if prev >= b1.len() {
            continue;
        }
        for (is_b2, b) in [(false, b1), (true, b2)] {
            let line = extended_line(b[prev], b[stem_end]);
            if let Some(p) = intersect_lines_2d(near_line.0, near_line.1, line.0, line.1) {
                hits.push((line_param(p, near_line.0, near_line.1), si, is_b2));
            }
        }
    }
    if hits.len() < 2 {
        let (ref s1c, ref s2c) = contours_s[overlapping[0]];
        return notch_through_layer(t_b1, t_b2, s1c, s2c, stem_end, axis_stem);
    }
    hits.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let left = hits[0];
    let right = hits[hits.len() - 1];
    let (n0, f0) = through_cut_corners(
        thru,
        layers_stem,
        contours_s,
        left.1,
        left.2,
        near_line,
        far_line,
        stem_end,
        axis_stem,
    )?;
    let (n1, f1) = through_cut_corners(
        thru,
        layers_stem,
        contours_s,
        right.1,
        right.2,
        near_line,
        far_line,
        stem_end,
        axis_stem,
    )?;
    let mut n0 = n0;
    let mut n1 = n1;
    let mut f0 = f0;
    let mut f1 = f1;
    if line_param(n1, near_line.0, near_line.1) < line_param(n0, near_line.0, near_line.1) {
        std::mem::swap(&mut n0, &mut n1);
        std::mem::swap(&mut f0, &mut f1);
    }
    let mut left_ring = vec![near_line.0, n0, f0, far_line.0];
    let mut right_ring = vec![n1, near_line.1, far_line.1, f1];
    ensure_ccw(&mut left_ring);
    ensure_ccw(&mut right_ring);
    if left_ring.len() < 3 || right_ring.len() < 3 {
        return None;
    }
    let mut fp = left_ring;
    fp.push((f64::NAN, f64::NAN));
    fp.extend(right_ring);
    Some(fp)
}

fn through_cut_corners(
    thru: &MiterLayer,
    layers_stem: &[MiterLayer],
    contours_s: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    stem_i: usize,
    use_b2: bool,
    near_line: ((f64, f64), (f64, f64)),
    far_line: ((f64, f64), (f64, f64)),
    stem_end: usize,
    axis_stem: &[(f64, f64)],
) -> Option<((f64, f64), (f64, f64))> {
    let stem = layers_stem.get(stem_i)?;
    let (ref b1, ref b2) = contours_s.get(stem_i)?;
    if stem_end >= b1.len() || stem_end >= b2.len() {
        return None;
    }
    let prev = if stem_end == 0 { 1 } else { stem_end - 1 };
    if prev >= b1.len() {
        return None;
    }
    let l1 = extended_line(b1[prev], b1[stem_end]);
    let l2 = extended_line(b2[prev], b2[stem_end]);
    let miter = !stem.material.is_empty()
        && stem.material.eq_ignore_ascii_case(&thru.material)
        && !is_structural_function(&stem.function);
    if miter {
        let d1 = point_axis_offset(b1[stem_end], axis_stem);
        let d2 = point_axis_offset(b2[stem_end], axis_stem);
        let (inner, outer) = if d1 <= d2 { (l1, l2) } else { (l2, l1) };
        let n = intersect_lines_2d(near_line.0, near_line.1, outer.0, outer.1)?;
        let f = intersect_lines_2d(far_line.0, far_line.1, inner.0, inner.1)?;
        Some((n, f))
    } else {
        let line = if use_b2 { l2 } else { l1 };
        let n = intersect_lines_2d(near_line.0, near_line.1, line.0, line.1)?;
        let f = intersect_lines_2d(far_line.0, far_line.1, line.0, line.1)?;
        Some((n, f))
    }
}

fn notch_through_layer(
    t_b1: &[(f64, f64)],
    t_b2: &[(f64, f64)],
    s_b1: &[(f64, f64)],
    s_b2: &[(f64, f64)],
    stem_end: usize,
    axis_stem: &[(f64, f64)],
) -> Option<Vec<(f64, f64)>> {
    if t_b1.len() < 2 || t_b2.len() < 2 || s_b1.len() < 2 || s_b2.len() < 2 {
        return None;
    }
    if stem_end >= s_b1.len() || stem_end >= s_b2.len() {
        return None;
    }
    let prev = if stem_end == 0 { 1 } else { stem_end - 1 };
    if prev >= s_b1.len() || prev >= axis_stem.len() {
        return None;
    }
    let s1_line = extended_line(s_b1[prev], s_b1[stem_end]);
    let s2_line = extended_line(s_b2[prev], s_b2[stem_end]);
    let t1_line = (*t_b1.first()?, *t_b1.last()?);
    let t2_line = (*t_b2.first()?, *t_b2.last()?);
    let Some(to_stem) = stem_to_body(axis_stem, stem_end) else {
        return None;
    };
    let join = axis_stem[stem_end];
    let s1 = closest_point_on_line(join, t1_line);
    let s2 = closest_point_on_line(join, t2_line);
    let d1 = (s1.0 - join.0) * to_stem.0 + (s1.1 - join.1) * to_stem.1;
    let d2 = (s2.0 - join.0) * to_stem.0 + (s2.1 - join.1) * to_stem.1;
    let (near, far) = if d1 >= d2 {
        (t_b1, t_b2)
    } else {
        (t_b2, t_b1)
    };
    let near_line = (*near.first()?, *near.last()?);
    let far_line = (*far.first()?, *far.last()?);
    let mut n0 = intersect_lines_2d(near_line.0, near_line.1, s1_line.0, s1_line.1)?;
    let mut n1 = intersect_lines_2d(near_line.0, near_line.1, s2_line.0, s2_line.1)?;
    let mut f0 = intersect_lines_2d(far_line.0, far_line.1, s1_line.0, s1_line.1)?;
    let mut f1 = intersect_lines_2d(far_line.0, far_line.1, s2_line.0, s2_line.1)?;
    if line_param(n1, near_line.0, near_line.1) < line_param(n0, near_line.0, near_line.1) {
        std::mem::swap(&mut n0, &mut n1);
        std::mem::swap(&mut f0, &mut f1);
    }
    // Full-thickness cut through this shell: two disjoint remainders.
    // A single C-ring that travels the far edge twice self-overlaps and
    // tessellators fill the notch (2D hatch and 3D extrusion).
    let mut left = vec![near_line.0, n0, f0, far_line.0];
    let mut right = vec![n1, near_line.1, far_line.1, f1];
    ensure_ccw(&mut left);
    ensure_ccw(&mut right);
    if left.len() < 3 || right.len() < 3 {
        return None;
    }
    let mut fp = left;
    fp.push((f64::NAN, f64::NAN));
    fp.extend(right);
    Some(fp)
}

fn closest_point_on_line(
    p: (f64, f64),
    line: ((f64, f64), (f64, f64)),
) -> (f64, f64) {
    let (a, b) = line;
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-24 {
        return a;
    }
    let t = ((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2;
    (a.0 + t * dx, a.1 + t * dy)
}

/// Boundary line pair on the other wall at the join.
///
/// For an L / T-stem (`end_b = Some(idx)`) this is the end segment of each
/// boundary. For a T through-wall (`end_b = None`) it is the boundary segment
/// whose parameter range contains the join point projected from A's end.
fn other_boundary_lines(
    b_b1: &[(f64, f64)],
    b_b2: &[(f64, f64)],
    end_b: Option<usize>,
    axis_a: &[(f64, f64)],
    axis_b: &[(f64, f64)],
    end_a: usize,
) -> Option<(((f64, f64), (f64, f64)), ((f64, f64), (f64, f64)))> {
    let n = b_b1.len();
    if n < 2 {
        return None;
    }

    if let Some(eb) = end_b {
        if eb >= n {
            return None;
        }
        let prev_b = if eb == 0 { 1 } else { eb - 1 };
        return Some((
            extended_line(b_b1[prev_b], b_b1[eb]),
            extended_line(b_b2[prev_b], b_b2[eb]),
        ));
    }

    // T through-wall: find the segment of axis_b closest to A's join point.
    let join_pt = axis_a.get(end_a).copied()?;
    let mut best_seg = 0usize;
    let mut best_dist = f64::INFINITY;
    for s in 0..axis_b.len().saturating_sub(1) {
        let d = point_seg_dist(join_pt, axis_b[s], axis_b[s + 1]);
        if d < best_dist {
            best_dist = d;
            best_seg = s;
        }
    }
    let i0 = best_seg;
    let i1 = best_seg + 1;
    if i1 >= n {
        return None;
    }
    Some((
        extended_line(b_b1[i0], b_b1[i1]),
        extended_line(b_b2[i0], b_b2[i1]),
    ))
}

/// Extend the segment `from → to` well past `to` so line/line intersection
/// still finds corners that lie outside the original segment.
fn extended_line(from: (f64, f64), to: (f64, f64)) -> ((f64, f64), (f64, f64)) {
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-12 {
        return (from, to);
    }
    // Extend by a large multiple of the segment length past `to`.
    let scale = 100.0;
    let ext = (to.0 + dx / len * len * scale, to.1 + dy / len * len * scale);
    // Also extend slightly backward past `from` so near-end intersections work.
    let back = (
        from.0 - dx / len * len * scale,
        from.1 - dy / len * len * scale,
    );
    (back, ext)
}

fn intersect_lines_2d(
    p1: (f64, f64),
    p2: (f64, f64),
    p3: (f64, f64),
    p4: (f64, f64),
) -> Option<(f64, f64)> {
    let (x1, y1) = p1;
    let (x2, y2) = p2;
    let (x3, y3) = p3;
    let (x4, y4) = p4;
    let denom = (y4 - y3) * (x2 - x1) - (x4 - x3) * (y2 - y1);
    if denom.abs() < 1e-12 {
        return None;
    }
    let ua = ((x4 - x3) * (y1 - y3) - (y4 - y3) * (x1 - x3)) / denom;
    Some((x1 + ua * (x2 - x1), y1 + ua * (y2 - y1)))
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    (dx * dx + dy * dy).sqrt()
}

fn point_seg_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (px, py) = p;
    let (x1, y1) = a;
    let (x2, y2) = b;
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-24 {
        return dist(p, a);
    }
    let t = ((px - x1) * dx + (py - y1) * dy) / len2;
    let t = t.clamp(0.0, 1.0);
    dist(p, (x1 + t * dx, y1 + t * dy))
}

/// Merge two single-end mitered results for the *same* layer/base footprint
/// into one polygon that reflects both ends' joins at once.
///
/// Each of `end_a_result` / `end_b_result` is the output of a miter helper
/// that only ever modifies the vertices near *its own* end, leaving every
/// other vertex byte-for-byte identical to `base` (see
/// [`mitered_layer_footprints`] / [`mitered_layer_footprints_with_override`]
/// docs). That invariant makes the merge purely positional: for each vertex
/// index, prefer whichever of the two results actually differs from `base`
/// at that index (ties/both-differ favor `end_a_result`, both-same keeps
/// `base`). `None` inputs are treated as "no change from base".
///
/// Returns `None` (falls back to `base`) when a candidate's vertex count
/// doesn't match `base`'s — this can only happen if the two calls were not
/// actually built from the same axis/layer topology, which callers should
/// avoid.
pub fn merge_end_footprints(
    base: &[(f64, f64)],
    end_a_result: Option<&Vec<(f64, f64)>>,
    end_b_result: Option<&Vec<(f64, f64)>>,
) -> Option<Vec<(f64, f64)>> {
    let a = end_a_result.filter(|v| v.len() == base.len());
    let b = end_b_result.filter(|v| v.len() == base.len());
    match (a, b) {
        (None, None) => None,
        (Some(a), None) => Some(a.clone()),
        (None, Some(b)) => Some(b.clone()),
        (Some(a), Some(b)) => {
            let eps = 1e-9;
            let close = |p: (f64, f64), q: (f64, f64)| {
                (p.0 - q.0).abs() < eps && (p.1 - q.1).abs() < eps
            };
            let merged: Vec<(f64, f64)> = base
                .iter()
                .enumerate()
                .map(|(idx, &bp)| {
                    let av = a[idx];
                    let bv = b[idx];
                    if !close(av, bp) {
                        av
                    } else if !close(bv, bp) {
                        bv
                    } else {
                        bp
                    }
                })
                .collect();
            Some(merged)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::join::{JoinKind, JunctionParticipant, LayerPairOverride};

    fn close(a: (f64, f64), b: (f64, f64), tol: f64) -> bool {
        dist(a, b) < tol
    }

    /// Single-layer helper: centered on the axis (`axis_offset = -t/2`).
    fn g(t: f64) -> MiterLayer {
        MiterLayer::geom(t, -t * 0.5)
    }

    /// Multi-layer helper: centered stack matching the legacy gap_before=0 layout.
    fn gs(thicknesses: &[f64]) -> Vec<MiterLayer> {
        let pairs: Vec<(f64, f64)> = thicknesses.iter().map(|&th| (th, 0.0)).collect();
        let offsets = crate::modules::aec::engine::wall_style::migrate_gap_before_to_axis_offset(&pairs);
        thicknesses
            .iter()
            .zip(offsets)
            .map(|(&th, off)| MiterLayer::geom(th, off))
            .collect()
    }

    #[test]
    fn merge_end_footprints_picks_the_end_that_actually_changed() {
        let base = vec![(0.0, -0.1), (10.0, -0.1), (10.0, 0.1), (0.0, 0.1)];
        // End-0 result only moves the vertices near x=0.
        let end0 = vec![(-0.2, -0.1), (10.0, -0.1), (10.0, 0.1), (-0.2, 0.1)];
        // End-1 result only moves the vertices near x=10.
        let end1 = vec![(0.0, -0.1), (10.2, -0.1), (10.2, 0.1), (0.0, 0.1)];

        let merged = merge_end_footprints(&base, Some(&end0), Some(&end1))
            .expect("both ends supplied should merge");
        assert_eq!(
            merged,
            vec![(-0.2, -0.1), (10.2, -0.1), (10.2, 0.1), (-0.2, 0.1)],
            "merge should keep each end's own change and combine them, got {merged:?}"
        );

        // Only one side supplied falls back to it unchanged.
        assert_eq!(merge_end_footprints(&base, Some(&end0), None), Some(end0.clone()));
        assert_eq!(merge_end_footprints(&base, None, Some(&end1)), Some(end1.clone()));
        assert_eq!(merge_end_footprints(&base, None, None), None);

        // Mismatched vertex count is ignored (falls back to the other side).
        let bad = vec![(0.0, 0.0)];
        assert_eq!(merge_end_footprints(&base, Some(&bad), Some(&end1)), Some(end1.clone()));
    }

    #[test]
    fn l_corner_miter_shares_diagonal_endpoints() {
        // Wall A along +X ending at (10,0); wall B along +Y starting at (10,0).
        // Single layer, thickness 0.2 each → half = 0.1.
        let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
        let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
        let layers = vec![g(0.2)];

        let fps_a = mitered_layer_footprints(
            &axis_a,
            &layers,
            1, // end at (10,0)
            &axis_b,
            &layers,
            Some(0),
            JoinKind::L,
        );
        let fps_b = mitered_layer_footprints(
            &axis_b,
            &layers,
            0,
            &axis_a,
            &layers,
            Some(1),
            JoinKind::L,
        );

        let fp_a = fps_a[0].as_ref().expect("A layer 0 should miter");
        let fp_b = fps_b[0].as_ref().expect("B layer 0 should miter");

        // Expected outer/inner miter corners for equal 0.2 walls at 90°:
        // A boundaries y=±0.1, B boundaries x=10±0.1.
        // Pairing the short diagonal: (10.1, -0.1) and (9.9, 0.1).
        let c1 = (10.1, -0.1);
        let c2 = (9.9, 0.1);
        assert!(
            fp_a.iter().any(|p| close(*p, c1, 1e-6)) && fp_a.iter().any(|p| close(*p, c2, 1e-6)),
            "A footprint should contain both miter corners, got {fp_a:?}"
        );
        assert!(
            fp_b.iter().any(|p| close(*p, c1, 1e-6)) && fp_b.iter().any(|p| close(*p, c2, 1e-6)),
            "B footprint should share the same miter corners, got {fp_b:?}"
        );
    }

    #[test]
    fn t_corner_stem_miters_against_through_wall() {
        // Stem A: (5,0)->(5,10) joins onto through wall B at (5,0).
        // T-join butts the stem straight into the near face of the matching
        // through-wall layer (y = +0.1 from the +Y stem), not a diagonal miter.
        let axis_a = vec![(5.0, 0.0), (5.0, 10.0)];
        let axis_b = vec![(0.0, 0.0), (10.0, 0.0)];
        let layers = vec![g(0.2)];

        let fps_a = mitered_layer_footprints(
            &axis_a,
            &layers,
            0, // stem end at (5,0)
            &axis_b,
            &layers,
            None, // B is through-wall
            JoinKind::T,
        );
        let fp_a = fps_a[0].as_ref().expect("stem layer should extend");

        // Near face of through layer from +Y is y = +0.1. Both stem end-cap
        // vertices must lie on that face (square butt), not on a diagonal.
        let expected = [(4.9, 0.1), (5.1, 0.1)];
        for e in expected {
            assert!(
                fp_a.iter().any(|p| close(*p, e, 1e-6)),
                "stem footprint missing butt corner {e:?}, got {fp_a:?}"
            );
        }
        // Reject the old diagonal miter corners.
        assert!(
            !fp_a.iter().any(|p| close(*p, (5.1, -0.1), 1e-6))
                && !fp_a.iter().any(|p| close(*p, (4.9, -0.1), 1e-6)),
            "stem must not use a diagonal/far-face miter, got {fp_a:?}"
        );
    }

    #[test]
    fn t_corner_right_angle_single_layer_butts_near_face() {
        // Explicit regression for Bug A / Step 3: orthogonal T, one layer.
        let stem = vec![(5.0, 0.0), (5.0, 8.0)];
        let through = vec![(0.0, 0.0), (10.0, 0.0)];
        let layers = vec![g(0.2)];
        let fps = mitered_layer_footprints(
            &stem,
            &layers,
            0,
            &through,
            &layers,
            None,
            JoinKind::T,
        );
        let fp = fps[0].as_ref().expect("stem layer");
        assert!(fp.iter().any(|p| close(*p, (4.9, 0.1), 1e-6)));
        assert!(fp.iter().any(|p| close(*p, (5.1, 0.1), 1e-6)));
        // Join-end cap is horizontal (same y) — not diagonal.
        let end_ys: Vec<f64> = fp
            .iter()
            .filter(|(_, y)| y.abs() < 0.5) // near the joint, not the far end
            .map(|(_, y)| *y)
            .collect();
        assert!(
            end_ys.len() >= 2,
            "expected both stem join-end vertices, got {fp:?}"
        );
        let y0 = end_ys[0];
        assert!(
            end_ys.iter().all(|y| (y - y0).abs() < 1e-6),
            "T butt end-cap must be square (equal y), got {end_ys:?} in {fp:?}"
        );
        assert!(
            (y0 - 0.1).abs() < 1e-5,
            "join end must sit on near face y=+0.1, got {y0}"
        );
    }

    #[test]
    fn t_corner_skewed_60_deg_butts_without_crossing() {
        // Stem approaches through wall at 60° to the through axis.
        let angle = 60.0_f64.to_radians();
        let stem = vec![
            (5.0, 0.0),
            (5.0 + 8.0 * angle.cos(), 8.0 * angle.sin()),
        ];
        let through = vec![(0.0, 0.0), (10.0, 0.0)];
        let layers = vec![g(0.2)];
        let fps = mitered_layer_footprints(
            &stem,
            &layers,
            0,
            &through,
            &layers,
            None,
            JoinKind::T,
        );
        let fp = fps[0].as_ref().expect("skewed stem layer");
        // Both end-cap vertices must lie on the near through face y=+0.1.
        let end_pts: Vec<(f64, f64)> = fp
            .iter()
            .copied()
            .filter(|(_, y)| (*y - 0.1).abs() < 1e-5)
            .collect();
        assert!(
            end_pts.len() >= 2,
            "both stem ends should butt y=+0.1, got {fp:?}"
        );
        // No vertex on the far face (would indicate diagonal or overshoot).
        assert!(
            !fp.iter().any(|(_, y)| (*y + 0.1).abs() < 1e-5),
            "skewed T must not reach far face y=-0.1, got {fp:?}"
        );
    }

    #[test]
    fn t_corner_multi_layer_each_matched_layer_butts_independently() {
        // Through wall: two layers stacked about the axis (each 0.1 thick).
        //   layer0 (core):   y ∈ [-0.1,  0.0]
        //   layer1 (finish): y ∈ [ 0.0,  0.1]
        // Structural cores butt at the core near face; finish stays on the
        // approach outer face and must not tunnel to the far side.
        let stem = vec![(5.0, 0.0), (5.0, 8.0)];
        let through = vec![(0.0, 0.0), (10.0, 0.0)];
        let layers = vec![
            MiterLayer::with_id(0.1, -0.1, "core", "Structural", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.1, 0.0, "finish", "Finish", uuid::Uuid::new_v4()),
        ];
        let fps = mitered_layer_footprints(
            &stem,
            &layers,
            0,
            &through,
            &layers,
            None,
            JoinKind::T,
        );
        assert_eq!(fps.len(), 2);
        let fp0 = fps[0].as_ref().expect("core layer");
        let fp1 = fps[1].as_ref().expect("finish layer");

        let join_ys = |fp: &[(f64, f64)]| -> Vec<f64> {
            fp.iter()
                .filter(|(_, y)| y.abs() < 0.5)
                .map(|(_, y)| *y)
                .collect()
        };

        let y1 = join_ys(fp1);
        assert!(
            !y1.is_empty()
                && y1.iter().any(|y| (*y - 0.1).abs() < 1e-4)
                && y1.iter().all(|y| *y > -1e-4),
            "finish should miter on the approach side (touch y=+0.1, not far face), got {y1:?} in {fp1:?}"
        );
        // Inner→outer diagonal: inner stem edge meets inner through edge (y=0),
        // outer stem edge meets outer through edge (y=0.1).
        let has_inner = fp1.iter().any(|(x, y)| {
            y.abs() < 1e-4 && (*x - 5.0).abs() < 1e-3
        });
        let has_outer = fp1.iter().any(|(_, y)| (*y - 0.1).abs() < 1e-4);
        assert!(
            has_inner && has_outer,
            "plaster miter must run inner-to-outer, got {fp1:?}"
        );
        assert!(
            !fp1.iter().any(|(x, y)| (*y - 0.1).abs() < 1e-4 && (*x - 5.0).abs() < 1e-3),
            "wrong miter diagonal (outer at inner x), got {fp1:?}"
        );

        let y0 = join_ys(fp0);
        assert!(
            !y0.is_empty() && y0.iter().all(|y| y.abs() < 1e-5),
            "core layer should butt y=0.0, got {y0:?} in {fp0:?}"
        );
        assert!(
            !fp0.iter().any(|(_, y)| (*y + 0.1).abs() < 1e-5),
            "core must not reach far face y=-0.1, got {fp0:?}"
        );
    }

    #[test]
    fn t_corner_unmatched_stem_layer_extends_to_through_outer_face() {
        // Stem has an extra material ("orphan") with no counterpart on the
        // through wall. The through wall has TWO layers (brick + concrete)
        // so the outer face (y=+0.15, outer edge of concrete) is geometrically
        // distinct from the matched brick layer's near face (y=+0.05). The
        // unmatched orphan layer must reach the true outer face, not the
        // brick's inner face.
        let stem = vec![(5.0, 0.0), (5.0, 8.0)];
        let through = vec![(0.0, 0.0), (10.0, 0.0)];
        let layers_stem = vec![
            MiterLayer::with_id(0.2, -0.15, "brick", "Finish", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.1, 0.05, "orphan", "Other", uuid::Uuid::new_v4()),
        ];
        let layers_through = vec![
            MiterLayer::with_id(0.2, -0.15, "brick", "Finish", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.1, 0.05, "concrete", "Structure", uuid::Uuid::new_v4()),
        ];

        let fps = mitered_layer_footprints(
            &stem,
            &layers_stem,
            0,
            &through,
            &layers_through,
            None,
            JoinKind::T,
        );
        assert_eq!(fps.len(), 2);
        let fp_match = fps[0].as_ref().expect("matched brick layer");
        let fp_orphan = fps[1].as_ref().expect("unmatched orphan must still extend");

        let join_ys = |fp: &[(f64, f64)]| -> Vec<f64> {
            fp.iter()
                .filter(|(_, y)| y.abs() < 0.5)
                .map(|(_, y)| *y)
                .collect()
        };

        // Thickest fallback cores (no Structural): brick butts at brick near
        // face y=+0.05; unmatched orphan still reaches the outer face.
        let y_match = join_ys(fp_match);
        assert!(
            !y_match.is_empty() && y_match.iter().all(|y| (*y - 0.05).abs() < 1e-5),
            "matched layer should butt at brick's near face y=+0.05, got {y_match:?} in {fp_match:?}"
        );
        let y_orphan = join_ys(fp_orphan);
        assert!(
            !y_orphan.is_empty() && y_orphan.iter().all(|y| (*y - 0.15).abs() < 1e-5),
            "unmatched layer must reach through outer face y=+0.15 (all vertices), got {y_orphan:?} in {fp_orphan:?}"
        );
    }

    #[test]
    fn t_unmatched_outer_face_uses_geometric_extents_not_list_order() {
        // Finish layer listed first but sitting on the +offset side; core
        // listed second with a more negative offset. Outer face toward the
        // stem (+Y) must still be the finish outer edge, not the first list
        // entry's b1.
        let stem = vec![(5.0, 0.0), (5.0, 8.0)];
        let through = vec![(0.0, 0.0), (10.0, 0.0)];
        let layers_stem = vec![MiterLayer::with_id(
            0.1,
            0.05,
            "orphan",
            "Other",
            uuid::Uuid::new_v4(),
        )];
        let layers_through = vec![
            MiterLayer::with_id(0.1, 0.05, "finish", "Finish", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.2, -0.15, "core", "Structure", uuid::Uuid::new_v4()),
        ];
        let fps = mitered_layer_footprints(
            &stem,
            &layers_stem,
            0,
            &through,
            &layers_through,
            None,
            JoinKind::T,
        );
        let fp = fps[0].as_ref().expect("orphan");
        let y_join: Vec<f64> = fp
            .iter()
            .filter(|(_, y)| y.abs() < 0.5)
            .map(|(_, y)| *y)
            .collect();
        assert!(
            !y_join.is_empty() && y_join.iter().all(|y| (*y - 0.05).abs() < 1e-5),
            "no Structural: thickest through layer is core; stem butts its near face y=+0.05, got {y_join:?} in {fp:?}"
        );
    }

    #[test]
    fn mismatched_layer_count_returns_none_for_unmatched() {
        let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
        let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
        // A has two layers, B has one — only one can match (by offset).
        let layers_a = gs(&[0.2, 0.05]);
        let layers_b = vec![g(0.3)];

        let fps = mitered_layer_footprints(
            &axis_a,
            &layers_a,
            1,
            &axis_b,
            &layers_b,
            Some(0),
            JoinKind::L,
        );
        assert_eq!(fps.len(), 2);
        let matched = fps.iter().filter(|f| f.is_some()).count();
        let unmatched = fps.iter().filter(|f| f.is_none()).count();
        assert_eq!(matched, 1, "exactly one layer should miter, got {fps:?}");
        assert_eq!(
            unmatched, 1,
            "unmatched layer must fall back (None) to corner_override, got {fps:?}"
        );
    }

    #[test]
    fn miter_degenerate_angle_5_deg_falls_back() {
        // sin(5 deg) approx 0.087.
        // Wall A at 0 deg: (0,0) to (10,0)
        // Wall B at 5 deg: (0,0) to (10 * cos(5), 10 * sin(5))
        let angle = 5.0 * std::f64::consts::PI / 180.0;
        let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
        let axis_b = vec![(0.0, 0.0), (10.0 * angle.cos(), 10.0 * angle.sin())];
        let layers = vec![g(0.2)];

        let fps = mitered_layer_footprints(
            &axis_a,
            &layers,
            0,
            &axis_b,
            &layers,
            Some(0),
            JoinKind::L,
        );
        assert!(fps[0].is_none(), "5 degree join should be degenerate and fall back");
    }

    #[test]
    fn miter_degenerate_angle_175_deg_falls_back() {
        // sin(175 deg) = sin(5 deg) approx 0.087.
        let angle = 175.0 * std::f64::consts::PI / 180.0;
        let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
        let axis_b = vec![(0.0, 0.0), (10.0 * angle.cos(), 10.0 * angle.sin())];
        let layers = vec![g(0.2)];

        let fps = mitered_layer_footprints(
            &axis_a,
            &layers,
            0,
            &axis_b,
            &layers,
            Some(0),
            JoinKind::L,
        );
        assert!(fps[0].is_none(), "175 degree join should be degenerate and fall back");
    }

    #[test]
    fn mitered_junction_degenerate_angles() {
        // 3-way junction at (0,0).
        // Wall 0: (0,0) to (10,0)  [0 deg outgoing ray]
        // Wall 1: (0,0) to (10*cos(5), 10*sin(5)) [5 deg outgoing ray]
        // Wall 2: (0,0) to (0, 10) [90 deg outgoing ray]
        let a5 = 5.0 * std::f64::consts::PI / 180.0;
        let junction = Junction {
            point: glam::DVec3::new(0.0, 0.0, 0.0),
            participants: vec![
                JunctionParticipant { wall_index: 0, role: JunctionRole::Endpoint(0) },
                JunctionParticipant { wall_index: 1, role: JunctionRole::Endpoint(0) },
                JunctionParticipant { wall_index: 2, role: JunctionRole::Endpoint(0) },
            ],
        };
        let walls = vec![
            JunctionWallGeom { axis: vec![(0.0, 0.0), (10.0, 0.0)], layers: vec![g(0.2)], end: Some(0) },
            JunctionWallGeom { axis: vec![(0.0, 0.0), (10.0 * a5.cos(), 10.0 * a5.sin())], layers: vec![g(0.2)], end: Some(0) },
            JunctionWallGeom { axis: vec![(0.0, 0.0), (0.0, 10.0)], layers: vec![g(0.2)], end: Some(0) },
        ];

        let res = mitered_junction_layer_footprints(&junction, &walls);
        // Wall 0 (0 deg) has neighbors Wall 2 (90 deg, safe) and Wall 1 (5 deg, degenerate).
        // Wall 1 (5 deg) has neighbors Wall 0 (5 deg, degenerate) and Wall 2 (85 deg, safe).

        // Wall 1's miter with Wall 0 is degenerate.
        // It should not panic or produce NaN.
        let fp1 = &res[1][0];
        if let Some(fp) = fp1 {
            for &(x, y) in fp {
                assert!(!x.is_nan() && !y.is_nan());
                // Miter point shouldn't be extremely far away (e.g. > 100m)
                assert!(x.abs() < 100.0 && y.abs() < 100.0, "miter point too far: ({x}, {y})");
            }
        }
    }

    #[test]
    fn same_composition_different_order_matches_by_material() {
        // A: Brick (outer), Insulation, Concrete (inner)
        // B: same materials reversed in the list — must still pair Brick↔Brick etc.
        let layers_a = vec![
            MiterLayer::with_id(0.1, -0.17500000000000002, "Brick", "Finish", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.05, -0.07500000000000001, "Insulation", "Insulation", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.2, -0.02500000000000001, "Concrete", "Structural", uuid::Uuid::new_v4()),
        ];
        let layers_b = vec![
            MiterLayer::with_id(0.2, -0.17500000000000002, "Concrete", "Structural", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.05, 0.024999999999999994, "Insulation", "Insulation", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.1, 0.075, "Brick", "Finish", uuid::Uuid::new_v4()),
        ];
        let pairing = match_layer_indices(&layers_a, &layers_b);
        assert_eq!(pairing, vec![Some(2), Some(1), Some(0)]);

        let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
        let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
        let fps = mitered_layer_footprints(
            &axis_a,
            &layers_a,
            1,
            &axis_b,
            &layers_b,
            Some(0),
            JoinKind::L,
        );
        assert_eq!(fps.len(), 3);
        assert!(
            fps.iter().all(|f| f.is_some()),
            "all three material-matched layers should miter, got {fps:?}"
        );
    }

    #[test]
    fn different_materials_and_counts_fallback_only_unmatched() {
        // A: Brick + Concrete. B: only Insulation (no shared material).
        // Offset-only pairing may still match the closer layer; the other
        // must remain unmatched (None → corner_override).
        let layers_a = vec![
            MiterLayer::with_id(0.1, -0.15000000000000002, "Brick", "Finish", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.2, -0.05000000000000002, "Concrete", "Structural", uuid::Uuid::new_v4()),
        ];
        let layers_b = vec![MiterLayer::with_id(0.15, -0.075, "Insulation", "Insulation", uuid::Uuid::new_v4())];

        let pairing = match_layer_indices(&layers_a, &layers_b);
        assert_eq!(pairing.len(), 2);
        let matched = pairing.iter().filter(|p| p.is_some()).count();
        let unmatched = pairing.iter().filter(|p| p.is_none()).count();
        assert_eq!(matched, 1, "one offset-based pair expected, got {pairing:?}");
        assert_eq!(
            unmatched, 1,
            "genuinely unmatched layer stays None, got {pairing:?}"
        );

        let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
        let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
        let fps = mitered_layer_footprints(
            &axis_a,
            &layers_a,
            1,
            &axis_b,
            &layers_b,
            Some(0),
            JoinKind::L,
        );
        assert_eq!(fps.iter().filter(|f| f.is_some()).count(), 1);
        assert_eq!(fps.iter().filter(|f| f.is_none()).count(), 1);
    }

    fn two_layer() -> Vec<MiterLayer> {
        vec![
            MiterLayer::with_id(0.1, -0.15000000000000002, "Brick", "Finish", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.2, -0.05000000000000002, "Concrete", "Structural", uuid::Uuid::new_v4()),
        ]
    }

    #[test]
    fn x_crossing_four_walls_two_layers_all_miter() {
        // Four walls meeting at origin, each with 2 layers. Every endpoint
        // wall must get a consistent mitered footprint for both layers.
        let layers = two_layer();
        let geoms = vec![
            JunctionWallGeom {
                axis: vec![(0.0, 0.0), (10.0, 0.0)],
                layers: layers.clone(),
                end: Some(0),
            },
            JunctionWallGeom {
                axis: vec![(0.0, 0.0), (0.0, 10.0)],
                layers: layers.clone(),
                end: Some(0),
            },
            JunctionWallGeom {
                axis: vec![(0.0, 0.0), (-10.0, 0.0)],
                layers: layers.clone(),
                end: Some(0),
            },
            JunctionWallGeom {
                axis: vec![(0.0, 0.0), (0.0, -10.0)],
                layers: layers.clone(),
                end: Some(0),
            },
        ];
        let junction = Junction {
            point: glam::DVec3::ZERO,
            participants: (0..4)
                .map(|i| JunctionParticipant {
                    wall_index: i,
                    role: JunctionRole::Endpoint(0),
                })
                .collect(),
        };

        let all = mitered_junction_layer_footprints(&junction, &geoms);
        assert_eq!(all.len(), 4);
        for (wi, fps) in all.iter().enumerate() {
            assert_eq!(fps.len(), 2, "wall {wi} should have 2 layer slots");
            for (li, fp) in fps.iter().enumerate() {
                let poly = fp
                    .as_ref()
                    .unwrap_or_else(|| panic!("wall {wi} layer {li} should miter, got None"));
                assert!(
                    poly.len() >= 3,
                    "wall {wi} layer {li} footprint too short: {poly:?}"
                );
            }
        }

        // East wall (index 0) end-cap should sit near x = +half_thickness of
        // the outer layer stack (total 0.3 → half 0.15) after N-way miter
        // against N and S — i.e. not left at the axis (x=0).
        let east_outer = all[0][0].as_ref().unwrap();
        let max_abs_x_near_origin = east_outer
            .iter()
            .filter(|(x, y)| x.abs() < 0.5 && y.abs() < 0.5)
            .map(|(x, _)| x.abs())
            .fold(0.0_f64, f64::max);
        assert!(
            max_abs_x_near_origin > 0.05,
            "east wall end should leave the axis after N-way miter, got max |x|={max_abs_x_near_origin}, fp={east_outer:?}"
        );
    }

    #[test]
    fn n_way_collinear_pair_does_not_miter_into_each_other() {
        // Three endpoints at (8,22): horizontal stem + two collinear vertical
        // walls (the example "N-Wege einschalig"). The vertical pair must not
        // receive overlapping L-miters.
        let layers = vec![g(0.2)];
        let geoms = vec![
            JunctionWallGeom {
                axis: vec![(0.0, 22.0), (8.0, 22.0)],
                layers: layers.clone(),
                end: Some(1),
            },
            JunctionWallGeom {
                axis: vec![(8.0, 22.0), (8.0, 28.0)],
                layers: layers.clone(),
                end: Some(0),
            },
            JunctionWallGeom {
                axis: vec![(8.0, 22.0), (8.0, 16.0)],
                layers: layers.clone(),
                end: Some(0),
            },
        ];
        let junction = Junction {
            point: glam::DVec3::new(8.0, 22.0, 0.0),
            participants: (0..3)
                .map(|i| JunctionParticipant {
                    wall_index: i,
                    role: JunctionRole::Endpoint(if i == 0 { 1 } else { 0 }),
                })
                .collect(),
        };
        let all = mitered_junction_layer_footprints(&junction, &geoms);
        let north = all[1][0].as_ref().expect("north wall should miter against stem only");
        let south = all[2][0].as_ref().expect("south wall should miter against stem only");
        let north_ymin = north.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        let south_ymax = south.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        assert!(
            north_ymin > 21.85,
            "north wall must not L-miter through the collinear south wall, ymin={north_ymin} fp={north:?}"
        );
        assert!(
            south_ymax < 22.15,
            "south wall must not L-miter through the collinear north wall, ymax={south_ymax} fp={south:?}"
        );
        let stem = all[0][0].as_ref().expect("stem should still miter");
        assert!(stem.len() >= 3);
    }

    #[test]
    fn t_junction_with_third_wall_two_layers() {
        // Through wall on X, stem from +Y, third wall from NE — all meet at
        // (5,0). Stem and third are endpoints; through is Through.
        let layers = two_layer();
        let geoms = vec![
            JunctionWallGeom {
                axis: vec![(0.0, 0.0), (10.0, 0.0)],
                layers: layers.clone(),
                end: None, // through
            },
            JunctionWallGeom {
                axis: vec![(5.0, 0.0), (5.0, 10.0)],
                layers: layers.clone(),
                end: Some(0),
            },
            JunctionWallGeom {
                axis: vec![(5.0, 0.0), (10.0, 5.0)],
                layers: layers.clone(),
                end: Some(0),
            },
        ];
        let junction = Junction {
            point: glam::DVec3::new(5.0, 0.0, 0.0),
            participants: vec![
                JunctionParticipant {
                    wall_index: 0,
                    role: JunctionRole::Through(0),
                },
                JunctionParticipant {
                    wall_index: 1,
                    role: JunctionRole::Endpoint(0),
                },
                JunctionParticipant {
                    wall_index: 2,
                    role: JunctionRole::Endpoint(0),
                },
            ],
        };

        let all = mitered_junction_layer_footprints(&junction, &geoms);
        assert_eq!(all.len(), 3);

        // Through-wall: no endpoint miter of its own.
        assert!(
            all[0].iter().all(|f| f.is_none()),
            "through wall footprints stay None (rectangular fallback), got {:?}",
            all[0]
        );

        // Stem and third wall: both layers should resolve.
        for wi in [1usize, 2] {
            assert_eq!(all[wi].len(), 2);
            let matched = all[wi].iter().filter(|f| f.is_some()).count();
            assert!(
                matched >= 1,
                "wall {wi} should miter at least one layer at the 3-way junction, got {:?}",
                all[wi]
            );
            for fp in all[wi].iter().flatten() {
                assert!(fp.len() >= 3);
            }
        }
    }

    #[test]
    fn junction_two_wall_l_matches_pairwise() {
        // N-way path with exactly two endpoint walls must agree with pairwise.
        let layers = vec![g(0.2)];
        let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
        let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
        let pairwise = mitered_layer_footprints(
            &axis_a,
            &layers,
            1,
            &axis_b,
            &layers,
            Some(0),
            JoinKind::L,
        );
        let geoms = vec![
            JunctionWallGeom {
                axis: axis_a.clone(),
                layers: layers.clone(),
                end: Some(1),
            },
            JunctionWallGeom {
                axis: axis_b.clone(),
                layers: layers.clone(),
                end: Some(0),
            },
        ];
        let junction = Junction {
            point: glam::DVec3::new(10.0, 0.0, 0.0),
            participants: vec![
                JunctionParticipant {
                    wall_index: 0,
                    role: JunctionRole::Endpoint(1),
                },
                JunctionParticipant {
                    wall_index: 1,
                    role: JunctionRole::Endpoint(0),
                },
            ],
        };
        let nway = mitered_junction_layer_footprints(&junction, &geoms);
        let pa = pairwise[0].as_ref().expect("pairwise A");
        let na = nway[0][0].as_ref().expect("nway A");
        assert_eq!(pa.len(), na.len());
        for (p, n) in pa.iter().zip(na.iter()) {
            assert!(close(*p, *n, 1e-6), "mismatch pairwise={p:?} nway={n:?}");
        }
    }

    /// Build an L-corner test axis pair where wall A arrives from the west
    /// ending at the origin, and wall B leaves the origin at `interior_deg`
    /// measured as the angle between A's back-direction and B's forward
    /// direction (i.e. the physical wedge angle of the corner). 90° matches
    /// `l_corner_miter_shares_diagonal_endpoints`'s configuration (rotated).
    fn l_corner_axes_at_interior_angle(interior_deg: f64) -> (Vec<(f64, f64)>, Vec<(f64, f64)>) {
        let theta = (180.0 - interior_deg).to_radians();
        let axis_a = vec![(-10.0, 0.0), (0.0, 0.0)];
        let axis_b = vec![(0.0, 0.0), (10.0 * theta.cos(), 10.0 * theta.sin())];
        (axis_a, axis_b)
    }

    /// Ground-truth for the two candidate corner-boundary pairings at a
    /// mitered join, computed *independently* of the pairing-selection logic
    /// under test in `miter_end_points`: it only reuses generic 2D line
    /// intersection (`intersect_lines_2d`) plus the real per-layer offset
    /// boundaries from `layer_contours` (the exact same boundaries
    /// production feeds into `miter_one_layer`/`miter_end_points`).
    ///
    /// Returns `(direct1, direct2, cross1, cross2)` where `direct` is the
    /// `a_b1`\u{2194}`b_b1` / `a_b2`\u{2194}`b_b2` pairing and `cross` is
    /// `a_b1`\u{2194}`b_b2` / `a_b2`\u{2194}`b_b1`. The convention is
    /// anchored to the pre-existing, trusted right-angle test
    /// `l_corner_miter_shares_diagonal_endpoints`: its expected corners
    /// `(10.1,-0.1)`/`(9.9,0.1)` are exactly this `direct` pair for that
    /// axis configuration, confirmed below.
    fn direct_and_cross_corner_pairs(
        axis_a: &[(f64, f64)],
        axis_b: &[(f64, f64)],
        geom: &[(f64, f64)],
        layer_index: usize,
    ) -> ((f64, f64), (f64, f64), (f64, f64), (f64, f64)) {
        let contours_a = layer_contours(axis_a, geom);
        let contours_b = layer_contours(axis_b, geom);
        let (a_b1, a_b2) = &contours_a[layer_index];
        let (b_b1, b_b2) = &contours_b[layer_index];
        let a1b1 = intersect_lines_2d(a_b1[0], a_b1[1], b_b1[0], b_b1[1])
            .expect("a1/b1 boundaries must intersect for a non-degenerate corner");
        let a2b2 = intersect_lines_2d(a_b2[0], a_b2[1], b_b2[0], b_b2[1])
            .expect("a2/b2 boundaries must intersect for a non-degenerate corner");
        let a1b2 = intersect_lines_2d(a_b1[0], a_b1[1], b_b2[0], b_b2[1])
            .expect("a1/b2 boundaries must intersect for a non-degenerate corner");
        let a2b1 = intersect_lines_2d(a_b2[0], a_b2[1], b_b1[0], b_b1[1])
            .expect("a2/b1 boundaries must intersect for a non-degenerate corner");
        (a1b1, a2b2, a1b2, a2b1)
    }

    /// Assert that a mitered layer footprint contains the analytically
    /// correct sharp ("direct") corner pairing, and NOT the wrong
    /// ("cross"/beveled) one that the historical shortest-segment heuristic
    /// picked for acute angles. This is discriminating: it fails on the old
    /// buggy pairing selection and passes only once the true continuation of
    /// each boundary line is chosen.
    fn assert_sharp_miter(
        fp: &[(f64, f64)],
        axis_a: &[(f64, f64)],
        axis_b: &[(f64, f64)],
        geom: &[(f64, f64)],
        layer_index: usize,
    ) {
        let (direct1, direct2, cross1, cross2) =
            direct_and_cross_corner_pairs(axis_a, axis_b, geom, layer_index);
        let has_direct =
            fp.iter().any(|p| close(*p, direct1, 1e-6)) && fp.iter().any(|p| close(*p, direct2, 1e-6));
        let has_cross =
            fp.iter().any(|p| close(*p, cross1, 1e-6)) && fp.iter().any(|p| close(*p, cross2, 1e-6));
        assert!(
            has_direct && !has_cross,
            "expected the sharp ('direct') miter pairing {direct1:?}/{direct2:?}, but got the \
             beveled ('cross') pairing {cross1:?}/{cross2:?} instead; fp={fp:?}"
        );
    }

    #[test]
    fn acute_30_deg_l_corner_is_mitered_sharp_not_beveled() {
        let (axis_a, axis_b) = l_corner_axes_at_interior_angle(30.0);
        let layers = vec![g(0.2)];
        let fps = mitered_layer_footprints(
            &axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L,
        );
        let fp = fps[0].as_ref().expect("30 degree L-join should miter");
        assert_sharp_miter(fp, &axis_a, &axis_b, &[(0.2, -0.1)], 0);
    }

    #[test]
    fn acute_45_deg_l_corner_is_mitered_sharp_not_beveled() {
        let (axis_a, axis_b) = l_corner_axes_at_interior_angle(45.0);
        let layers = vec![g(0.2)];
        let fps = mitered_layer_footprints(
            &axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L,
        );
        let fp = fps[0].as_ref().expect("45 degree L-join should miter");
        assert_sharp_miter(fp, &axis_a, &axis_b, &[(0.2, -0.1)], 0);

        // The exact expected corner points, verified analytically: A's
        // right-hand (south, y=-0.1) boundary continues into B's right-hand
        // boundary (the true outer/convex spike), while A's left-hand
        // (north, y=+0.1) boundary is pulled back to B's left-hand boundary
        // (the concave/inner side) — i.e. the "direct" (same-handedness)
        // pairing, not the shorter "cross" pairing a naive distance
        // heuristic would pick.
        let outer = (0.1 * (1.0 + std::f64::consts::SQRT_2), -0.1);
        let inner = (-0.1 * (1.0 + std::f64::consts::SQRT_2), 0.1);
        assert!(
            fp.iter().any(|p| close(*p, outer, 1e-6)),
            "expected outer miter spike at {outer:?}, got {fp:?}"
        );
        assert!(
            fp.iter().any(|p| close(*p, inner, 1e-6)),
            "expected inner miter point at {inner:?}, got {fp:?}"
        );
    }

    #[test]
    fn acute_60_deg_l_corner_is_mitered_sharp_not_beveled() {
        let (axis_a, axis_b) = l_corner_axes_at_interior_angle(60.0);
        let layers = vec![g(0.2)];
        let fps = mitered_layer_footprints(
            &axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L,
        );
        let fp = fps[0].as_ref().expect("60 degree L-join should miter");
        assert_sharp_miter(fp, &axis_a, &axis_b, &[(0.2, -0.1)], 0);
    }

    #[test]
    fn right_angle_90_deg_l_corner_regression_unchanged() {
        // Same configuration as `l_corner_miter_shares_diagonal_endpoints`,
        // just re-expressed via the shared helper — must keep producing a
        // sharp miter (this already worked before the fix).
        let (axis_a, axis_b) = l_corner_axes_at_interior_angle(90.0);
        let layers = vec![g(0.2)];
        let fps = mitered_layer_footprints(
            &axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L,
        );
        let fp = fps[0].as_ref().expect("90 degree L-join should miter");
        assert_sharp_miter(fp, &axis_a, &axis_b, &[(0.2, -0.1)], 0);
    }

    #[test]
    fn obtuse_120_deg_l_corner_regression_unchanged() {
        let (axis_a, axis_b) = l_corner_axes_at_interior_angle(120.0);
        let layers = vec![g(0.2)];
        let fps = mitered_layer_footprints(
            &axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L,
        );
        let fp = fps[0].as_ref().expect("120 degree L-join should miter");
        assert_sharp_miter(fp, &axis_a, &axis_b, &[(0.2, -0.1)], 0);
    }

    #[test]
    fn acute_45_deg_l_corner_multi_layer_each_layer_sharp() {
        // Two matching layers on both walls at a 45° corner: each layer
        // must independently form its own sharp miter, not just the outer.
        let (axis_a, axis_b) = l_corner_axes_at_interior_angle(45.0);
        let layers = two_layer();
        let fps = mitered_layer_footprints(
            &axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L,
        );
        let geom: Vec<(f64, f64)> = layers.iter().map(MiterLayer::as_geom).collect();
        for (li, fp) in fps.iter().enumerate() {
            let fp = fp.as_ref().unwrap_or_else(|| panic!("layer {li} should miter"));
            assert_sharp_miter(fp, &axis_a, &axis_b, &geom, li);
        }
    }

    #[test]
    fn acute_45_deg_n_way_junction_endpoint_is_sharp_not_beveled() {
        // 3-way junction with one acute (45°) sub-angle between walls 0 and 1;
        // wall 2 is far away at 90° from wall 0 so it doesn't interfere.
        // Reuses the same corner/orientation as the pairwise 45° test so the
        // expected sharp-miter geometry between walls 0 and 1 is identical.
        let (axis_a, axis_b) = l_corner_axes_at_interior_angle(45.0);
        let layers = vec![g(0.2)];
        let junction = Junction {
            point: glam::DVec3::new(0.0, 0.0, 0.0),
            participants: vec![
                JunctionParticipant { wall_index: 0, role: JunctionRole::Endpoint(1) },
                JunctionParticipant { wall_index: 1, role: JunctionRole::Endpoint(0) },
                JunctionParticipant { wall_index: 2, role: JunctionRole::Endpoint(0) },
            ],
        };
        let walls = vec![
            JunctionWallGeom { axis: axis_a.clone(), layers: layers.clone(), end: Some(1) },
            JunctionWallGeom { axis: axis_b.clone(), layers: layers.clone(), end: Some(0) },
            // Far third wall, straight up — keeps wall 0/1 as pure angular
            // neighbors of each other on one side.
            JunctionWallGeom { axis: vec![(0.0, 0.0), (0.0, -10.0)], layers: layers.clone(), end: Some(0) },
        ];
        let res = mitered_junction_layer_footprints(&junction, &walls);
        let fp0 = res[0][0].as_ref().expect("wall 0 layer should miter against wall 1");

        // Wall 2 sits at a *different* 90° angle from wall 0 (south), so it
        // legitimately claims wall 0's other (north) boundary side in this
        // 3-way arrangement — only the boundary side that actually faces
        // wall 1 is meant to carry the acute miter under test here. Compute
        // the ground-truth direct/cross candidates for the wall0/wall1 pair
        // exactly like the pairwise tests above, and assert the correct
        // ("direct") corner is present while the wrong ("cross"/beveled)
        // one that the historical shortest-segment heuristic would have
        // picked for this acute angle is absent — genuinely exercising the
        // acute-angle path inside `mitered_junction_layer_footprints`
        // itself, not just the underlying pairwise helper.
        let (direct1, _direct2, cross1, _cross2) =
            direct_and_cross_corner_pairs(&axis_a, &axis_b, &[(0.2, -0.1)], 0);
        assert!(
            fp0.iter().any(|p| close(*p, direct1, 1e-6)),
            "expected the sharp miter corner {direct1:?} facing wall 1 in the n-way \
             footprint, got {fp0:?}"
        );
        assert!(
            !fp0.iter().any(|p| close(*p, cross1, 1e-6)),
            "n-way footprint contains the beveled/cross corner {cross1:?} instead of the \
             sharp one, got {fp0:?}"
        );

        // And it must agree with the plain pairwise helper on that same
        // corner, confirming the N-way path reuses the corrected geometry
        // rather than coincidentally landing on it.
        let pairwise = mitered_layer_footprints(
            &axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L,
        );
        let pfp0 = pairwise[0].as_ref().expect("pairwise wall 0 layer should miter");
        assert!(
            pfp0.iter().any(|p| close(*p, direct1, 1e-6)),
            "sanity: pairwise result should also contain {direct1:?}, got {pfp0:?}"
        );
    }

    #[test]
    fn junction_mixed_layer_stacks() {
        // 3-way junction at (0,0).
        // Wall 0: (0,0) to (10,0). Layers: [Brick, Concrete]
        // Wall 1: (0,0) to (0,10). Layers: [Brick, Concrete] (identical stack)
        // Wall 2: (0,0) to (-10,0). Layers: [Concrete] (different stack)

        let l_brick = MiterLayer::with_id(0.1, -0.15, "Brick", "Finish", uuid::Uuid::new_v4());
        let l_concrete = MiterLayer::with_id(0.2, -0.05, "Concrete", "Structural", uuid::Uuid::new_v4());
        let l_concrete_alone = MiterLayer::with_id(0.2, -0.1, "Concrete", "Structural", uuid::Uuid::new_v4());

        let junction = Junction {
            point: glam::DVec3::new(0.0, 0.0, 0.0),
            participants: vec![
                JunctionParticipant { wall_index: 0, role: JunctionRole::Endpoint(0) },
                JunctionParticipant { wall_index: 1, role: JunctionRole::Endpoint(0) },
                JunctionParticipant { wall_index: 2, role: JunctionRole::Endpoint(0) },
            ],
        };
        let walls = vec![
            JunctionWallGeom { axis: vec![(0.0, 0.0), (10.0, 0.0)], layers: vec![l_brick.clone(), l_concrete.clone()], end: Some(0) },
            JunctionWallGeom { axis: vec![(0.0, 0.0), (0.0, 10.0)], layers: vec![l_brick.clone(), l_concrete.clone()], end: Some(0) },
            JunctionWallGeom { axis: vec![(0.0, 0.0), (-10.0, 0.0)], layers: vec![l_concrete_alone], end: Some(0) },
        ];

        let res = mitered_junction_layer_footprints(&junction, &walls);
        // Wall 0 layers: [Brick, Concrete].
        // Neighbor W1: [Brick, Concrete]. Both should match.
        // Neighbor W2: [Concrete]. Only Concrete should match.

        // Wall 0 layer 0 (Brick):
        //   - matches W1 (Brick)
        //   - no match W2
        //   => Should miter against W1.
        assert!(res[0][0].is_some(), "W0 layer 0 (Brick) should miter against W1");

        // Wall 0 layer 1 (Concrete):
        //   - matches W1 (Concrete)
        //   - matches W2 (Concrete)
        //   => Should miter against both.
        assert!(res[0][1].is_some(), "W0 layer 1 (Concrete) should miter against W1 and W2");

        // Wall 2 layer 0 (Concrete):
        //   - matches W1 (Concrete)
        //   - matches W0 (Concrete)
        assert!(res[2][0].is_some(), "W2 layer 0 (Concrete) should miter against W1 and W0");

        // Now test a layer that matches NO neighbor.
        let l_brick_g = MiterLayer::with_id(0.1, -0.075, "Brick", "Finish", uuid::Uuid::new_v4());
        let l_glass = MiterLayer::with_id(0.05, 0.025, "Glass", "Finish", uuid::Uuid::new_v4());
        let l_brick_alone = MiterLayer::with_id(0.1, -0.05, "Brick", "Finish", uuid::Uuid::new_v4());
        let walls_mixed = vec![
            JunctionWallGeom { axis: vec![(0.0, 0.0), (10.0, 0.0)], layers: vec![l_brick_g, l_glass], end: Some(0) },
            JunctionWallGeom { axis: vec![(0.0, 0.0), (0.0, 10.0)], layers: vec![l_brick_alone.clone()], end: Some(0) },
            JunctionWallGeom { axis: vec![(0.0, 0.0), (-10.0, 0.0)], layers: vec![l_brick_alone], end: Some(0) },
        ];
        let res_mixed = mitered_junction_layer_footprints(&junction, &walls_mixed);
        // Wall 0 layer 1 (Glass) matches NO neighbor.
        assert!(res_mixed[0][1].is_none(), "Wall 0 layer 1 (Glass) should fall back when no neighbor matches");
    }

    #[test]
    fn nway_t_junction_unmatched_stem_layer_extends_to_through_outer_face() {
        // N-way (3+ walls) T-junction: a through-wall plus two endpoint
        // (stem) walls. One stem wall carries an extra "orphan" layer with
        // no material match in the through-wall's stack — it must extend to
        // the through-wall's outer face (parity with the pairwise
        // `t_junction_layer_footprints` path), not fall back to `None`.
        //
        // Through wall: (-10,0) to (10,0), layers [Brick(0.2), Concrete(0.1)]
        //   stacked from y=-0.15 to y=+0.15 (outer face on +y side is +0.15).
        // Stem wall 1 (endpoint): (0,0) to (0,10), layers [Brick, Orphan].
        // Stem wall 2 (endpoint): (0,0) to (5,-8), layers [Brick] only, at a
        //   distinct angle so this is a genuine 3-way (not 2-wall) junction.
        let through_layers = vec![
            MiterLayer::with_id(0.2, -0.15, "Brick", "Finish", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.1, 0.05, "Concrete", "Structural", uuid::Uuid::new_v4()),
        ];
        let stem1_layers = vec![
            MiterLayer::with_id(0.2, -0.125, "Brick", "Finish", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.05, 0.075, "Orphan", "Other", uuid::Uuid::new_v4()),
        ];
        let stem2_layers = vec![MiterLayer::with_id(0.2, -0.1, "Brick", "Finish", uuid::Uuid::new_v4())];

        let junction = Junction {
            point: glam::DVec3::new(0.0, 0.0, 0.0),
            participants: vec![
                JunctionParticipant { wall_index: 0, role: JunctionRole::Through(0) },
                JunctionParticipant { wall_index: 1, role: JunctionRole::Endpoint(0) },
                JunctionParticipant { wall_index: 2, role: JunctionRole::Endpoint(0) },
            ],
        };
        let walls = vec![
            JunctionWallGeom {
                axis: vec![(-10.0, 0.0), (10.0, 0.0)],
                layers: through_layers,
                end: None,
            },
            JunctionWallGeom {
                axis: vec![(0.0, 0.0), (0.0, 10.0)],
                layers: stem1_layers,
                end: Some(0),
            },
            JunctionWallGeom {
                axis: vec![(0.0, 0.0), (5.0, -8.0)],
                layers: stem2_layers,
                end: Some(0),
            },
        ];

        let res = mitered_junction_layer_footprints(&junction, &walls);
        assert_eq!(res.len(), 3);
        let fp_orphan = res[1][1]
            .as_ref()
            .expect("orphan layer must extend to through-wall outer face, not fall back to None");

        // Outer face of the through-wall stack on the +y approach side is
        // y=+0.15 (outer edge of the concrete layer).
        let join_ys: Vec<f64> = fp_orphan
            .iter()
            .filter(|(_, y)| y.abs() < 0.5)
            .map(|(_, y)| *y)
            .collect();
        assert!(
            !join_ys.is_empty() && join_ys.iter().all(|y| (*y - 0.15).abs() < 1e-5),
            "orphan layer should land on through-wall outer face y=+0.15, got {join_ys:?} in {fp_orphan:?}"
        );
    }

    // ---- Step 2: JunctionOverride integration -----------------------------

    fn lref_at(material: &str, index: usize) -> LayerRef {
        LayerRef {
            material_id: material.to_string(),
            role_tag: None,
            index,
        layer_id: None,
        }
    }

    fn lref(material: &str) -> LayerRef {
        lref_at(material, 0)
    }

    #[test]
    fn override_none_matches_automatic_l_corner_regression() {
        // No override at all: byte-for-byte identical to the plain helper.
        let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
        let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
        let layers = vec![g(0.2)];
        let refs = vec![lref("brick")];

        let automatic =
            mitered_layer_footprints(&axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L);
        let overridden = mitered_layer_footprints_with_override(
            &axis_a, &layers, &refs, 1, &axis_b, &layers, Some(0), JoinKind::L, None,
        );
        assert_eq!(automatic, overridden);
    }

    #[test]
    fn override_none_matches_automatic_t_junction_regression() {
        let stem = vec![(5.0, 0.0), (5.0, 8.0)];
        let through = vec![(0.0, 0.0), (10.0, 0.0)];
        let layers = vec![g(0.2)];
        let refs = vec![lref("brick")];

        let automatic =
            mitered_layer_footprints(&stem, &layers, 0, &through, &layers, None, JoinKind::T);
        let overridden = mitered_layer_footprints_with_override(
            &stem, &layers, &refs, 0, &through, &layers, None, JoinKind::T, None,
        );
        assert_eq!(automatic, overridden);
    }

    #[test]
    fn override_none_matches_automatic_n_way_junction_regression() {
        let l_brick = MiterLayer::with_id(0.1, -0.15, "Brick", "Finish", uuid::Uuid::new_v4());
        let l_concrete = MiterLayer::with_id(0.2, -0.05, "Concrete", "Structural", uuid::Uuid::new_v4());
        let l_concrete_alone = MiterLayer::with_id(0.2, -0.1, "Concrete", "Structural", uuid::Uuid::new_v4());
        let junction = Junction {
            point: glam::DVec3::new(0.0, 0.0, 0.0),
            participants: vec![
                JunctionParticipant { wall_index: 0, role: JunctionRole::Endpoint(0) },
                JunctionParticipant { wall_index: 1, role: JunctionRole::Endpoint(0) },
                JunctionParticipant { wall_index: 2, role: JunctionRole::Endpoint(0) },
            ],
        };
        let walls = vec![
            JunctionWallGeom { axis: vec![(0.0, 0.0), (10.0, 0.0)], layers: vec![l_brick.clone(), l_concrete.clone()], end: Some(0) },
            JunctionWallGeom { axis: vec![(0.0, 0.0), (0.0, 10.0)], layers: vec![l_brick.clone(), l_concrete.clone()], end: Some(0) },
            JunctionWallGeom { axis: vec![(0.0, 0.0), (-10.0, 0.0)], layers: vec![l_concrete_alone], end: Some(0) },
        ];
        let layer_refs = vec![
            vec![lref_at("Brick", 0), lref_at("Concrete", 1)],
            vec![lref_at("Brick", 0), lref_at("Concrete", 1)],
            vec![lref_at("Concrete", 0)],
        ];
        let overrides: Vec<Option<JunctionOverride>> = vec![None, None, None];

        let automatic = mitered_junction_layer_footprints(&junction, &walls);
        let overridden = mitered_junction_layer_footprints_with_overrides(
            &junction, &walls, &layer_refs, &overrides,
        );
        assert_eq!(automatic, overridden);
    }

    #[test]
    fn junction_override_default_outer_face_changes_t_join_from_butt() {
        // T-junction against a two-layer through wall; the stem's single
        // layer matches the *inner* through layer ("core", y in [-0.2, 0]),
        // so the automatic butt lands on the inner boundary (y = 0.0). A
        // `default_style: OuterFace` override must instead push it to the
        // true outer face of the whole through-wall stack (y = +0.2).
        let stem = vec![(5.0, 0.0), (5.0, 8.0)];
        let through = vec![(0.0, 0.0), (10.0, 0.0)];
        let stem_layers = vec![MiterLayer::with_id(0.2, -0.1, "core", "Structural", uuid::Uuid::new_v4())];
        let through_layers = vec![
            MiterLayer::with_id(0.2, -0.2, "core", "Structural", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.2, 0.0, "other", "Finish", uuid::Uuid::new_v4()),
        ];
        let refs = vec![lref("core")];

        let automatic = mitered_layer_footprints(
            &stem, &stem_layers, 0, &through, &through_layers, None, JoinKind::T,
        );
        let fp_auto = automatic[0].as_ref().expect("automatic butt should succeed");
        assert!(
            fp_auto.iter().any(|p| close(*p, (4.9, 0.0), 1e-6)),
            "expected automatic butt at the inner boundary y=0.0, got {fp_auto:?}"
        );

        let ov = JunctionOverride {
            default_style: Some(JoinOverrideStyle::OuterFace),
            layer_pairs: Vec::new(),
        };
        let overridden = mitered_layer_footprints_with_override(
            &stem, &stem_layers, &refs, 0, &through, &through_layers, None, JoinKind::T,
            Some(&ov),
        );
        let fp_over = overridden[0].as_ref().expect("outer-face override should succeed");
        assert!(
            fp_over.iter().any(|p| close(*p, (4.9, 0.2), 1e-5)),
            "expected outer-face butt at y=0.2, got {fp_over:?}"
        );
        assert!(
            !fp_over.iter().any(|p| close(*p, (4.9, 0.0), 1e-5)),
            "override result should not match the automatic near-face butt, got {fp_over:?}"
        );
    }

    #[test]
    fn junction_override_layer_pair_no_extend_keeps_one_layer_unjoined() {
        // L-corner with two layers; a `layer_pairs` NoExtend override on the
        // outer ("brick") layer only must leave that layer at its original,
        // un-joined boundary (endpoint still at x=10) while the inner layer
        // still auto-miters normally (endpoint pushed past x=10).
        let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
        let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
        let layers = vec![
            MiterLayer::with_id(0.1, -0.1, "concrete", "Structural", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.1, 0.0, "brick", "Finish", uuid::Uuid::new_v4()),
        ];
        let refs = vec![lref_at("concrete", 0), lref_at("brick", 1)];

        let automatic =
            mitered_layer_footprints(&axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L);
        assert!(automatic[0].is_some(), "sanity: inner layer should auto-miter");
        assert!(automatic[1].is_some(), "sanity: outer layer should auto-miter");

        let ov = JunctionOverride {
            default_style: None,
            layer_pairs: vec![LayerPairOverride {
                layer_a: lref_at("brick", 1),
                layer_b: None,
                style: JoinOverrideStyle::NoExtend,
            }],
        };
        let overridden = mitered_layer_footprints_with_override(
            &axis_a, &layers, &refs, 1, &axis_b, &layers, Some(0), JoinKind::L, Some(&ov),
        );
        assert_eq!(
            overridden[0], automatic[0],
            "layer without a matching override must auto-resolve unchanged"
        );
        let fp_brick = overridden[1]
            .as_ref()
            .expect("NoExtend must still yield the un-joined original footprint");
        assert_ne!(
            overridden[1], automatic[1],
            "NoExtend must differ from the automatic miter result"
        );
        assert!(
            fp_brick.iter().all(|(x, _)| *x <= 10.0 + 1e-9),
            "NoExtend brick layer must stay at its original (un-extended) boundary, got {fp_brick:?}"
        );
    }

    /// Regression for the reported bug: a wall with two layers that use the
    /// *same* material (e.g. two plaster layers) must let a `LayerPairOverride`
    /// target only one specific occurrence, identified by its `index`, not
    /// both indistinguishably (which would happen if only `material_id`
    /// were compared).
    #[test]
    fn junction_override_layer_pair_targets_only_matching_index_for_duplicate_material() {
        let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
        let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
        let layers = vec![
            MiterLayer::with_id(0.1, -0.1, "plaster", "Finish", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.1, 0.0, "plaster", "Finish", uuid::Uuid::new_v4()),
        ];
        // Both layers share the same material id; only their `index` differs.
        let refs = vec![lref_at("plaster", 0), lref_at("plaster", 1)];

        let automatic =
            mitered_layer_footprints(&axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L);

        // Override targets only the *second* "plaster" occurrence (index 1).
        let ov = JunctionOverride {
            default_style: None,
            layer_pairs: vec![LayerPairOverride {
                layer_a: lref_at("plaster", 1),
                layer_b: None,
                style: JoinOverrideStyle::NoExtend,
            }],
        };
        let overridden = mitered_layer_footprints_with_override(
            &axis_a, &layers, &refs, 1, &axis_b, &layers, Some(0), JoinKind::L, Some(&ov),
        );

        assert_eq!(
            overridden[0], automatic[0],
            "index 0's plaster layer must auto-resolve unchanged even though it shares a \
             material id with the overridden layer"
        );
        assert_ne!(
            overridden[1], automatic[1],
            "index 1's plaster layer must be affected by its own layer-pair override"
        );
    }

    #[test]
    fn junction_override_layer_pair_wins_over_default_style() {
        // Both a default_style and a layer-specific override are set; the
        // layer-pair override must win for its layer, while default_style
        // (Miter, same as automatic L result here) applies elsewhere.
        let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
        let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
        let layers = vec![
            MiterLayer::with_id(0.1, -0.1, "concrete", "Structural", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.1, 0.0, "brick", "Finish", uuid::Uuid::new_v4()),
        ];
        let refs = vec![lref_at("concrete", 0), lref_at("brick", 1)];

        let ov = JunctionOverride {
            default_style: Some(JoinOverrideStyle::Miter),
            layer_pairs: vec![LayerPairOverride {
                layer_a: lref_at("brick", 1),
                layer_b: None,
                style: JoinOverrideStyle::NoExtend,
            }],
        };
        let overridden = mitered_layer_footprints_with_override(
            &axis_a, &layers, &refs, 1, &axis_b, &layers, Some(0), JoinKind::L, Some(&ov),
        );
        let automatic =
            mitered_layer_footprints(&axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L);
        // Concrete layer: default_style Miter == automatic miter result.
        assert_eq!(overridden[0], automatic[0]);
        // Brick layer: layer-pair NoExtend wins over default_style Miter,
        // keeping the layer at its original (un-extended) boundary.
        assert_ne!(overridden[1], automatic[1]);
        let fp_brick = overridden[1]
            .as_ref()
            .expect("NoExtend must still yield the un-joined original footprint");
        assert!(fp_brick.iter().all(|(x, _)| *x <= 10.0 + 1e-9));
    }

    #[test]
    fn near_face_override_matches_automatic_t_butt() {
        let stem = vec![(5.0, 0.0), (5.0, 8.0)];
        let through = vec![(0.0, 0.0), (10.0, 0.0)];
        let stem_layers = vec![MiterLayer::with_id(0.2, -0.1, "core", "Structural", uuid::Uuid::new_v4())];
        let through_layers = vec![
            MiterLayer::with_id(0.2, -0.2, "core", "Structural", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.2, 0.0, "other", "Structural", uuid::Uuid::new_v4()),
        ];
        let refs = vec![lref("core")];
        let automatic = mitered_layer_footprints(
            &stem, &stem_layers, 0, &through, &through_layers, None, JoinKind::T,
        );
        let ov = JunctionOverride {
            default_style: Some(JoinOverrideStyle::NearFace),
            layer_pairs: Vec::new(),
        };
        let overridden = mitered_layer_footprints_with_override(
            &stem, &stem_layers, &refs, 0, &through, &through_layers, None, JoinKind::T,
            Some(&ov),
        );
        assert_eq!(overridden[0], automatic[0]);
    }

    #[test]
    fn far_face_override_extends_past_matched_near_face() {
        let stem = vec![(5.0, 0.0), (5.0, 8.0)];
        let through = vec![(0.0, 0.0), (10.0, 0.0)];
        let stem_layers = vec![MiterLayer::with_id(0.2, -0.1, "core", "Structural", uuid::Uuid::new_v4())];
        let through_layers = vec![
            MiterLayer::with_id(0.2, -0.2, "core", "Structural", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.2, 0.0, "other", "Structural", uuid::Uuid::new_v4()),
        ];
        let refs = vec![lref("core")];
        let ov = JunctionOverride {
            default_style: Some(JoinOverrideStyle::FarFace),
            layer_pairs: Vec::new(),
        };
        let overridden = mitered_layer_footprints_with_override(
            &stem, &stem_layers, &refs, 0, &through, &through_layers, None, JoinKind::T,
            Some(&ov),
        );
        let fp = overridden[0].as_ref().expect("far-face override");
        assert!(
            fp.iter().any(|p| close(*p, (4.9, -0.2), 1e-5) || close(*p, (5.1, -0.2), 1e-5)
                || p.1 < -0.05),
            "far face of matched core is y=-0.2, got {fp:?}"
        );
    }

    #[test]
    fn unmatched_stem_notches_through_layers() {
        let through = vec![(0.0, 0.0), (10.0, 0.0)];
        let stem = vec![(5.0, 0.0), (5.0, 8.0)];
        let through_layers = vec![
            MiterLayer::with_id(0.2, -0.1, "core", "Structural", uuid::Uuid::new_v4()),
        ];
        let stem_layers = vec![
            MiterLayer::with_id(0.2, -0.1, "core", "Structural", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.2, 0.1, "finish", "Finish", uuid::Uuid::new_v4()),
        ];
        let cut = through_wall_cutout_footprints(&through, &through_layers, &stem, &stem_layers, 0);
        assert!(
            cut[0].is_none(),
            "through core must stay rectangular, got {cut:?}"
        );
    }

    #[test]
    fn through_cutout_uses_full_stem_envelope_on_every_layer() {
        let through = vec![(0.0, 0.0), (10.0, 0.0)];
        let stem = vec![(5.0, 0.0), (5.0, 8.0)];
        let through_layers = vec![
            MiterLayer::with_id(0.015, -0.195, "putz", "Finish", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.24, -0.18, "mw", "Structural", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.12, 0.06, "ins", "Insulation", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.015, 0.18, "putz2", "Finish", uuid::Uuid::new_v4()),
        ];
        let stem_layers = through_layers.clone();
        let cut = through_wall_cutout_footprints(&through, &through_layers, &stem, &stem_layers, 0);
        assert_eq!(cut.len(), 4);
        assert!(cut[0].is_none(), "far-side putz must stay rectangular, got {cut:?}");
        assert!(cut[1].is_none(), "through core must stay rectangular, got {cut:?}");
        assert!(
            cut[2].is_some(),
            "approach insulation must be notched, got {cut:?}"
        );
        assert!(
            cut[3].is_some(),
            "approach putz should receive a stem-core pocket, got {cut:?}"
        );
        let fp = cut[3].as_ref().unwrap();
        let rings = split_footprint_rings(fp);
        assert_eq!(rings.len(), 2, "through-cut must be two remainders, got {fp:?}");
        // Same-material putz: cut edges are miters (inner/outer X differ).
        let mitered_edge = rings.iter().any(|r| {
            r.windows(2).any(|w| {
                (w[0].1 - 0.195).abs() < 0.02
                    && (w[1].1 - 0.18).abs() < 0.02
                    && (w[0].0 - w[1].0).abs() > 0.005
            }) || r.windows(2).any(|w| {
                (w[0].1 - 0.18).abs() < 0.02
                    && (w[1].1 - 0.195).abs() < 0.02
                    && (w[0].0 - w[1].0).abs() > 0.005
            })
        });
        assert!(
            mitered_edge,
            "approach putz pocket should miter with stem putz, got {rings:?}"
        );
        let in_ring = |ring: &[(f64, f64)], p: (f64, f64)| -> bool {
            let mut inside = false;
            let n = ring.len();
            let mut j = n - 1;
            for i in 0..n {
                let (xi, yi) = ring[i];
                let (xj, yj) = ring[j];
                if ((yi > p.1) != (yj > p.1))
                    && (p.0 < (xj - xi) * (p.1 - yi) / (yj - yi + 1e-30) + xi)
                {
                    inside = !inside;
                }
                j = i;
            }
            inside
        };
        let overlap = (5.0, 0.187);
        assert!(
            rings.iter().all(|r| !in_ring(r, overlap)),
            "approach putz must be open at the stem core, got {rings:?}"
        );
    }

    #[test]
    fn through_insulation_notched_by_core_not_full_envelope_on_3_to_4() {
        let through = vec![(0.0, 0.0), (10.0, 0.0)];
        let stem = vec![(5.0, 0.0), (5.0, 8.0)];
        let through_layers = vec![
            MiterLayer::with_id(0.015, -0.195, "putz", "Finish", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.24, -0.18, "mw", "Structural", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.12, 0.06, "ins", "Insulation", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.015, 0.18, "putz2", "Finish", uuid::Uuid::new_v4()),
        ];
        let stem_layers = vec![
            MiterLayer::with_id(0.015, -0.195, "putz", "Finish", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.36, -0.18, "mw", "Structural", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.015, 0.18, "putz2", "Finish", uuid::Uuid::new_v4()),
        ];
        let cut = through_wall_cutout_footprints(&through, &through_layers, &stem, &stem_layers, 0);
        let ins = cut[2].as_ref().expect("through insulation must be notched");
        let rings = split_footprint_rings(ins);
        assert_eq!(rings.len(), 2, "insulation cut must be two remainders, got {ins:?}");
        let mut xs: Vec<f64> = rings
            .iter()
            .flat_map(|r| r.iter().map(|p| p.0))
            .collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let left_inner = rings[0].iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max)
            .min(rings[1].iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max));
        let right_inner = rings[0].iter().map(|p| p.0).fold(f64::INFINITY, f64::min)
            .max(rings[1].iter().map(|p| p.0).fold(f64::INFINITY, f64::min));
        let gap = (right_inner - left_inner).abs();
        assert!(
            gap < 0.38,
            "insulation pocket must be stem-core width, not full stem, gap={gap} rings={rings:?}"
        );
        assert!(
            gap > 0.30,
            "insulation pocket should still clear the stem core, gap={gap} rings={rings:?}"
        );
        let butt = rings.iter().any(|r| {
            r.windows(2).any(|w| {
                (w[0].0 - w[1].0).abs() < 0.002
                    && (w[0].1 - w[1].1).abs() > 0.05
            })
        });
        assert!(
            butt,
            "unmatched insulation should butt (vertical cut), got {rings:?}"
        );
    }

    #[test]
    fn through_plaster_hatch_tessellation_leaves_stem_pocket() {
        let through = vec![(0.0, 0.0), (10.0, 0.0)];
        let stem = vec![(5.0, 0.0), (5.0, 8.0)];
        let through_layers = vec![
            MiterLayer::with_id(0.015, -0.195, "putz", "Finish", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.24, -0.18, "mw", "Structural", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.12, 0.06, "ins", "Insulation", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.015, 0.18, "putz", "Finish", uuid::Uuid::new_v4()),
        ];
        let stem_layers = vec![
            MiterLayer::with_id(0.015, -0.195, "putz", "Finish", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.36, -0.18, "mw", "Structural", uuid::Uuid::new_v4()),
            MiterLayer::with_id(0.015, 0.18, "putz", "Finish", uuid::Uuid::new_v4()),
        ];
        let cut = through_wall_cutout_footprints(&through, &through_layers, &stem, &stem_layers, 0);
        let plaster = cut[3]
            .as_ref()
            .or(cut[0].as_ref())
            .expect("approach plaster must be notched");
        let rings = split_footprint_rings(plaster);
        let closed: Vec<Vec<[f64; 2]>> = rings
            .iter()
            .map(|r| {
                let mut pts: Vec<[f64; 2]> = r.iter().map(|&(x, y)| [x, y]).collect();
                if pts.len() >= 3 {
                    let first = pts[0];
                    let last = *pts.last().unwrap();
                    if (first[0] - last[0]).abs() > 1e-9 || (first[1] - last[1]).abs() > 1e-9 {
                        pts.push(first);
                    }
                }
                pts
            })
            .collect();
        let (points, triangles) = cadkernel::geom2d::triangulate_rings(&closed);
        let pocket = [5.0, 0.187];
        let in_tri = |a: [f64; 2], b: [f64; 2], c: [f64; 2], p: [f64; 2]| {
            let s = (a[1] * c[0] - a[0] * c[1] + (c[1] - a[1]) * p[0] + (a[0] - c[0]) * p[1])
                .signum();
            let t = (a[0] * b[1] - a[1] * b[0] + (a[1] - b[1]) * p[0] + (b[0] - a[0]) * p[1])
                .signum();
            let area2 = -b[1] * c[0] + a[1] * (c[0] - b[0]) + a[0] * (b[1] - c[1]) + b[0] * c[1];
            if area2.abs() < 1e-18 {
                return false;
            }
            let bary_a = (b[1] * c[0] - b[0] * c[1] + (c[1] - b[1]) * p[0] + (b[0] - c[0]) * p[1])
                / area2;
            let bary_b = (a[0] * c[1] - a[1] * c[0] + (a[1] - c[1]) * p[0] + (c[0] - a[0]) * p[1])
                / area2;
            let bary_c = 1.0 - bary_a - bary_b;
            bary_a >= -1e-6 && bary_b >= -1e-6 && bary_c >= -1e-6 && s * t >= 0.0
        };
        let filled = triangles.iter().any(|&[i, j, k]| {
            in_tri(points[i], points[j], points[k], pocket)
        });
        assert!(
            !filled,
            "hatch tessellation must not fill the stem pocket, triangles={triangles:?} pts={points:?}"
        );
    }

    #[test]
    fn different_materials_same_index_still_pair() {
        let a = vec![MiterLayer::with_id(0.2, -0.1, "brick", "Finish", uuid::Uuid::new_v4())];
        let b = vec![MiterLayer::with_id(0.2, -0.1, "block", "Structure", uuid::Uuid::new_v4())];
        let pairing = match_layer_indices(&a, &b);
        assert_eq!(pairing, vec![Some(0)]);
    }

    #[test]
    fn arc_l_join_uses_concentric_offset_not_chord() {
        let arc = vec![(-1.0, 0.0), (1.0, 0.0)];
        let bulges = vec![1.0];
        let stem = vec![(1.0, 0.0), (1.0, 2.0)];
        let layers = vec![MiterLayer::geom(0.2, -0.1)];
        let chord = mitered_layer_footprints(
            &arc, &layers, 1, &stem, &layers, Some(0), JoinKind::L,
        );
        let radial = mitered_layer_footprints_with_bulges(
            &arc, &layers, 1, &stem, &layers, Some(0), JoinKind::L, &bulges, &[],
        );
        let chord_fp = chord[0].as_ref().expect("chord miter");
        let radial_fp = radial[0].as_ref().expect("radial miter");
        assert_ne!(
            chord_fp, radial_fp,
            "arc join must not reuse the diameter-chord offset"
        );
        assert!(
            radial_fp.iter().any(|(x, _)| (*x - 1.1).abs() < 0.08),
            "concentric offset at the join should sit near x=1.1, got {radial_fp:?}"
        );
    }
}
