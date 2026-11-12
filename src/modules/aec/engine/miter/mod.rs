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
use super::join::{
    JoinKind, JoinOverrideStyle, Junction, JunctionOverride, JunctionRole, LayerGapOverride,
    LayerRef,
};
use super::openings::{axis_length, point_and_tangent_at_distance, remaining_axis_spans, Opening};

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

    let pairing = match_layer_indices_for_l_join(layers_a, layers_b, end_a, end_b);
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
/// `layer_ref`, given `override_data`. A [`LayerPairOverride`]
/// whose `layer_a` matches `layer_ref` takes precedence over `default_style`.
/// Returns the style and the optional explicit target layer (`layer_b`).
fn resolve_layer_override<'a>(
    layer_ref: &LayerRef,
    override_data: &'a JunctionOverride,
) -> Option<(&'a JoinOverrideStyle, Option<&'a LayerRef>)> {
    for pair in &override_data.layer_pairs {
        if layer_ref_matches_one(&pair.layer_a, layer_ref) {
            return Some((&pair.style, pair.layer_b.as_ref()));
        }
    }
    override_data.default_style.as_ref().map(|s| (s, None))
}

/// For an L-miter, a same-index `layer_b` (Innenputz↔Innenputz) can sit on
/// opposite world faces of the corner. Prefer the geometric pairing then.
fn l_miter_target_index(
    style: JoinOverrideStyle,
    end_b: Option<usize>,
    layer_a_index: usize,
    layer_b: Option<&LayerRef>,
    pairing_fb: Option<usize>,
    explicit: Option<usize>,
) -> Option<usize> {
    if style == JoinOverrideStyle::Miter && end_b.is_some() {
        if let (Some(paired), Some(picked)) = (pairing_fb, explicit) {
            if paired != picked && layer_b.is_some_and(|r| r.index == layer_a_index) {
                return Some(paired);
            }
        }
    }
    explicit.or(pairing_fb)
}

fn override_target_layer_index(
    layer_b: Option<&LayerRef>,
    layers_b: &[MiterLayer],
    layer_refs_b: Option<&[LayerRef]>,
    pairing_fallback: Option<usize>,
) -> Option<usize> {
    let Some(target) = layer_b else {
        return pairing_fallback;
    };
    if let Some(refs) = layer_refs_b {
        if let Some(i) = refs.iter().position(|r| layer_ref_matches_one(r, target)) {
            return Some(i);
        }
    }
    if let Some(id) = target.layer_id {
        if !id.is_nil() {
            if let Some(i) = layers_b.iter().position(|l| l.layer_id == id) {
                return Some(i);
            }
        }
    }
    if target.index < layers_b.len() {
        let l = &layers_b[target.index];
        if target.material_id.is_empty()
            || l.material.eq_ignore_ascii_case(&target.material_id)
        {
            return Some(target.index);
        }
    }
    let matches: Vec<usize> = layers_b
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            !target.material_id.is_empty()
                && l.material.eq_ignore_ascii_case(&target.material_id)
        })
        .map(|(i, _)| i)
        .collect();
    if matches.len() == 1 {
        Some(matches[0])
    } else {
        pairing_fallback
    }
}

/// Compare two [`LayerRef`]s the same way `display_apply::layer_ref_matches`
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

/// Compute the footprint for a single layer under a [`JunctionOverride`].
fn apply_layer_override_footprint(
    i: usize,
    layer_ref: &LayerRef,
    override_data: &JunctionOverride,
    axis_a: &[(f64, f64)],
    layers_a: &[MiterLayer],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    layer_refs_b: Option<&[LayerRef]>,
    end_b: Option<usize>,
    contours_a: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    contours_b: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    pairing_for_l: Option<&[Option<usize>]>,
    outer_target: Option<((f64, f64), (f64, f64))>,
    default_fp: Option<Vec<(f64, f64)>>,
) -> Option<Vec<(f64, f64)>> {
    let (style, layer_b) = resolve_layer_override(layer_ref, override_data)?;
    if i >= contours_a.len() {
        return None;
    }
    let (ref a_b1, ref a_b2) = contours_a[i];

    let b_idx = if end_b.is_some() {
        let pairing_fb = pairing_for_l
            .and_then(|p| p.get(i).copied().flatten())
            .or_else(|| closest_layer_index(&layers_a[i], layers_b));
        let explicit = override_target_layer_index(layer_b, layers_b, layer_refs_b, pairing_fb);
        l_miter_target_index(*style, end_b, i, layer_b, pairing_fb, explicit)
    } else if let Some(target) = layer_b {
        override_target_layer_index(Some(target), layers_b, layer_refs_b, None)
    } else {
        let core_pair = pair_structural_cores(layers_a, layers_b);
        let is_structural = is_structural_function(&layers_a[i].function)
            || core_pair.is_some_and(|(core_a, _)| i == core_a);
        if is_structural {
            core_pair.map(|(_, core_b)| core_b)
        } else {
            approach_finish_match(i, layers_a, layers_b, core_pair, axis_a, end_a, axis_b)
        }
    };

    let b_contour = b_idx.and_then(|j| contours_b.get(j));

    let new_fp = match style {
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
                default_fp
            }
        }
        JoinOverrideStyle::Butt
        | JoinOverrideStyle::OuterFace
        | JoinOverrideStyle::NearFace
        | JoinOverrideStyle::FarFace => override_target_line(
            *style,
            a_b1,
            a_b2,
            end_a,
            b_contour,
            end_b,
            axis_a,
            axis_b,
            layers_b,
            contours_b,
            outer_target,
        )
        .and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t)),
    };

    new_fp
}

/// Target face line a [`JoinOverrideStyle::Butt`] / `OuterFace` / `NearFace` /
/// `FarFace` override snaps layer A's joined end to. `None` for the other
/// styles or when no face can be resolved.
#[allow(clippy::too_many_arguments)]
fn override_target_line(
    style: JoinOverrideStyle,
    a_b1: &[(f64, f64)],
    a_b2: &[(f64, f64)],
    end_a: usize,
    b_contour: Option<&(Vec<(f64, f64)>, Vec<(f64, f64)>)>,
    end_b: Option<usize>,
    axis_a: &[(f64, f64)],
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    contours_b: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    outer_target: Option<((f64, f64), (f64, f64))>,
) -> Option<((f64, f64), (f64, f64))> {
    match style {
        JoinOverrideStyle::Butt => {
            if let Some((b_b1, b_b2)) = b_contour {
                near_face_line(a_b1, a_b2, end_a, b_b1, b_b2, end_b, axis_a, axis_b)
                    .or(outer_target)
            } else {
                outer_target
            }
        }
        JoinOverrideStyle::OuterFace => outer_target,
        JoinOverrideStyle::NearFace => {
            if let Some((b_b1, b_b2)) = b_contour {
                layer_face_line(a_b1, a_b2, end_a, b_b1, b_b2, end_b, axis_a, axis_b, false)
                    .or(outer_target)
            } else {
                outer_target
            }
        }
        JoinOverrideStyle::FarFace => {
            if let Some((b_b1, b_b2)) = b_contour {
                layer_face_line(a_b1, a_b2, end_a, b_b1, b_b2, end_b, axis_a, axis_b, true).or_else(
                    || {
                        if end_b.is_some() {
                            through_outer_face_line(axis_a, end_a, axis_b, layers_b, contours_b, true)
                        } else {
                            outer_target
                        }
                    },
                )
            } else if end_b.is_some() {
                through_outer_face_line(axis_a, end_a, axis_b, layers_b, contours_b, true)
            } else {
                outer_target
            }
        }
        JoinOverrideStyle::NoExtend | JoinOverrideStyle::Miter => None,
    }
}

/// What a manual override means for one stem layer of a T / X stem.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum StemOverrideTarget {
    /// No override (or `Miter`): the automatic near-face butt applies.
    Auto,
    /// `NoExtend`: the layer ends on the axis end, nothing is extended.
    NoExtend,
    /// Override snaps the layer end to this face line.
    Line(((f64, f64), (f64, f64))),
    /// Override style whose target face could not be resolved.
    Unresolved,
}

/// Effective target of the manual override on stem layer `i` when the stem
/// butts against a through wall (`end_b = None`). Mirrors the target
/// selection of [`apply_layer_override_footprint`] so X crossings can
/// validate / clip against the face the override really asks for.
#[allow(clippy::too_many_arguments)]
pub(crate) fn t_stem_override_target(
    i: usize,
    layer_ref: &LayerRef,
    override_data: &JunctionOverride,
    axis_a: &[(f64, f64)],
    layers_a: &[MiterLayer],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    contours_a: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    contours_b: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    outer_target: Option<((f64, f64), (f64, f64))>,
) -> StemOverrideTarget {
    let Some((style, layer_b)) = resolve_layer_override(layer_ref, override_data) else {
        return StemOverrideTarget::Auto;
    };
    if i >= contours_a.len() || i >= layers_a.len() {
        return StemOverrideTarget::Unresolved;
    }
    match style {
        JoinOverrideStyle::Miter => return StemOverrideTarget::Auto,
        JoinOverrideStyle::NoExtend => return StemOverrideTarget::NoExtend,
        _ => {}
    }
    let (ref a_b1, ref a_b2) = contours_a[i];
    let b_idx = if let Some(target) = layer_b {
        override_target_layer_index(Some(target), layers_b, None, None)
    } else {
        let core_pair = pair_structural_cores(layers_a, layers_b);
        let is_structural = is_structural_function(&layers_a[i].function)
            || core_pair.is_some_and(|(core_a, _)| i == core_a);
        if is_structural {
            core_pair.map(|(_, core_b)| core_b)
        } else {
            approach_finish_match(i, layers_a, layers_b, core_pair, axis_a, end_a, axis_b)
        }
    };
    let b_contour = b_idx.and_then(|j| contours_b.get(j));
    match override_target_line(
        *style,
        a_b1,
        a_b2,
        end_a,
        b_contour,
        None,
        axis_a,
        axis_b,
        layers_b,
        contours_b,
        outer_target,
    ) {
        Some(line) => StemOverrideTarget::Line(line),
        None => StemOverrideTarget::Unresolved,
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
    let pairing = if end_b.is_some() {
        Some(match_layer_indices_for_l_join(layers_a, layers_b, end_a, end_b))
    } else {
        None
    };
    let outer_target =
        through_outer_near_face_line(axis_a, end_a, axis_b, layers_b, &contours_b);

    for i in 0..layers_a.len() {
        let Some(layer_ref) = layer_refs_a.get(i) else {
            continue;
        };
        if let Some(fp) = apply_layer_override_footprint(
            i,
            layer_ref,
            ov,
            axis_a,
            layers_a,
            end_a,
            axis_b,
            layers_b,
            None,
            end_b,
            &contours_a,
            &contours_b,
            pairing.as_deref(),
            outer_target,
            out[i].clone(),
        ) {
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
        let is_structural = is_structural_function(&layers_a[i].function)
            || core_pair.is_some_and(|(core_a, _)| i == core_a);

        let fp = if is_structural {
            if let Some((_, core_b)) = core_pair {
                if core_b < contours_b.len() {
                    let (ref b_b1, ref b_b2) = contours_b[core_b];
                    let target = near_face_line(a_b1, a_b2, end_a, b_b1, b_b2, None, axis_a, axis_b)
                        .or(outer_target);
                    target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
                } else {
                    outer_target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
                }
            } else {
                outer_target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
            }
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
        };

        match fp {
            Some(poly) if poly.len() >= 3 => out.push(Some(poly)),
            _ => out.push(None),
        }
    }
    out
}

pub(crate) fn approach_finish_match(
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
    let ref_center = core_center.unwrap_or_else(|| {
        if layers_b.is_empty() {
            0.0
        } else {
            let mut min_off = f64::INFINITY;
            let mut max_off = f64::NEG_INFINITY;
            for l in layers_b {
                min_off = min_off.min(l.axis_offset);
                max_off = max_off.max(l.axis_offset + l.thickness);
            }
            (min_off + max_off) * 0.5
        }
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
        if (center - ref_center) * toward_stem < 1e-9 {
            continue;
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

pub(crate) fn is_structural_function(function: &str) -> bool {
    let f = function.trim();
    f.eq_ignore_ascii_case("structural")
        || f.eq_ignore_ascii_case("structure")
        || f.eq_ignore_ascii_case("tragwerk")
        || f.eq_ignore_ascii_case("tragschicht")
}

/// Pair Structural cores: among Structural layers, prefer material match
/// (`match_layer_indices` restricted to those); else the thickest Structural
/// on each wall. With no Structural, fall back to the thickest layer.
pub(crate) fn pair_structural_cores(layers_a: &[MiterLayer], layers_b: &[MiterLayer]) -> Option<(usize, usize)> {
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

/// When both walls use the same endpoint polarity at the joint (both start
/// vertices or both end vertices), they arrive from opposite circulation.
/// Matching against a mirrored copy of B then pairs finishes that share a
/// world-side of the L (inner plaster to the concave/convex bisector)
/// instead of cutting the innermost strip short of the corner.
fn l_join_mirror_other_stack(end_a: usize, end_b: Option<usize>) -> bool {
    let Some(end_b) = end_b else {
        return false;
    };
    (end_a == 0) == (end_b == 0)
}

fn mirror_layer_offsets_for_match(layers: &[MiterLayer]) -> Vec<MiterLayer> {
    layers
        .iter()
        .map(|layer| {
            let mut mirrored = layer.clone();
            mirrored.axis_offset = -(layer.axis_offset + layer.thickness);
            mirrored.layer_id = uuid::Uuid::nil();
            mirrored
        })
        .collect()
}

pub(crate) fn match_layer_indices_for_l_join(
    layers_a: &[MiterLayer],
    layers_b: &[MiterLayer],
    end_a: usize,
    end_b: Option<usize>,
) -> Vec<Option<usize>> {
    if l_join_mirror_other_stack(end_a, end_b) {
        match_layer_indices(layers_a, &mirror_layer_offsets_for_match(layers_b))
    } else {
        match_layer_indices(layers_a, layers_b)
    }
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
        let Some(refs) = layer_refs.get(wi) else {
            continue;
        };
        let n_rays = rays.len();
        let prev = &rays[(ri + n_rays - 1) % n_rays];
        let ov_self = overrides.get(wi).and_then(|o| o.as_ref());
        let ov_partner = overrides.get(prev.wall_index).and_then(|o| o.as_ref());
        let ov_merged = crate::modules::aec::engine::join::merge_partner_junction_override(
            ov_self, ov_partner,
        );
        let Some(ov) = ov_merged.as_ref() else {
            continue;
        };
        let other_w = prev.wall_index;
        if other_w == wi {
            continue;
        }
        let other_axis = &walls[other_w].axis;
        let other_contours = &contours[other_w];
        let pairing = if walls[other_w].end.is_some() {
            Some(match_layer_indices_for_l_join(
                &walls[wi].layers,
                &walls[other_w].layers,
                end_a,
                walls[other_w].end,
            ))
        } else {
            None
        };
        let outer_target = through_outer_near_face_line(
            &walls[wi].axis,
            end_a,
            other_axis,
            &walls[other_w].layers,
            other_contours,
        );
        let refs_b = layer_refs.get(other_w).map(|v| v.as_slice());

        for li in 0..walls[wi].layers.len() {
            let Some(layer_ref) = refs.get(li) else {
                continue;
            };
            if let Some(fp) = apply_layer_override_footprint(
                li,
                layer_ref,
                ov,
                &walls[wi].axis,
                &walls[wi].layers,
                end_a,
                other_axis,
                &walls[other_w].layers,
                refs_b,
                walls[other_w].end,
                &contours[wi],
                other_contours,
                pairing.as_deref(),
                outer_target,
                out[wi][li].clone(),
            ) {
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
    let mut fp = rebuild_footprint(a_b1, a_b2, end_a, new_a1, new_a2);
    if end_a < axis_a.len() && end_a < a_b1.len() {
        let prev = if end_a == 0 { 1 } else { end_a - 1 };
        let keep = if prev < a_b1.len() {
            a_b1[prev]
        } else {
            axis_a[if end_a == 0 { 1 } else { end_a - 1 }]
        };
        let clipped = clip_closed_to_halfplane(&fp, new_a1, new_a2, keep);
        if clipped.len() >= 3 {
            fp = clipped;
        }
    }
    Some(fp)
}

pub(crate) fn clip_closed_to_halfplane(
    poly: &[(f64, f64)],
    a: (f64, f64),
    b: (f64, f64),
    keep: (f64, f64),
) -> Vec<(f64, f64)> {
    let side = |p: (f64, f64)| (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
    let keep_s = side(keep);
    if keep_s.abs() < 1e-14 {
        return poly.to_vec();
    }
    let inside = |p: (f64, f64)| side(p) * keep_s >= -1e-12;
    let n = poly.len();
    if n < 3 {
        return poly.to_vec();
    }
    let mut out = Vec::with_capacity(n + 2);
    for i in 0..n {
        let cur = poly[i];
        let nxt = poly[(i + 1) % n];
        let cin = inside(cur);
        let nin = inside(nxt);
        if cin {
            out.push(cur);
        }
        if cin != nin {
            if let Some(hit) = intersect_lines_2d(cur, nxt, a, b) {
                if out.last().is_none_or(|p| dist(*p, hit) > 1e-12) {
                    out.push(hit);
                }
            }
        }
    }
    if let (Some(&first), Some(&last)) = (out.first(), out.last()) {
        if dist(first, last) < 1e-12 {
            out.pop();
        }
    }
    out
}

/// Intersect `subject` with convex CCW `clip` (Sutherland–Hodgman).
fn clip_subject_to_convex_clip(
    subject: &[(f64, f64)],
    clip: &[(f64, f64)],
) -> Vec<(f64, f64)> {
    let mut clip_poly = clip.to_vec();
    ensure_ccw(&mut clip_poly);
    if subject.len() < 3 || clip_poly.len() < 3 {
        return subject.to_vec();
    }
    let mut output = subject.to_vec();
    let n = clip_poly.len();
    for i in 0..n {
        let a = clip_poly[i];
        let b = clip_poly[(i + 1) % n];
        let input = std::mem::take(&mut output);
        if input.is_empty() {
            break;
        }
        let inside = |p: (f64, f64)| {
            (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0) >= -1e-12
        };
        for j in 0..input.len() {
            let cur = input[j];
            let prev = input[(j + input.len() - 1) % input.len()];
            let cin = inside(cur);
            let pin = inside(prev);
            if cin {
                if !pin {
                    if let Some(hit) = intersect_lines_2d(prev, cur, a, b) {
                        if output.last().is_none_or(|p| dist(*p, hit) > 1e-12) {
                            output.push(hit);
                        }
                    }
                }
                if output.last().is_none_or(|p| dist(*p, cur) > 1e-12) {
                    output.push(cur);
                }
            } else if pin {
                if let Some(hit) = intersect_lines_2d(prev, cur, a, b) {
                    if output.last().is_none_or(|p| dist(*p, hit) > 1e-12) {
                        output.push(hit);
                    }
                }
            }
        }
    }
    output
}

/// Keep a T-junction / manual gap cutout without discarding an L-miter
/// already computed for the same layer (square cutout caps at the wall
/// ends would otherwise replace the diagonal).
///
/// Each cutout ring is bounded by **every** neighbouring notch edge, not
/// just the closest one: a middle ring of a 3-ring (two crossings) has two
/// notches and must be clipped on both sides or it runs through the second
/// through-wall core. Outer square caps are *not* used as clip edges, so an
/// L-miter beyond the axis end survives on the end rings.
pub fn merge_through_cutout_preserving_miter(
    mitered: Option<&[(f64, f64)]>,
    cutout: &[(f64, f64)],
) -> Vec<(f64, f64)> {
    let Some(m) = mitered else {
        return cutout.to_vec();
    };
    let m_rings = split_footprint_rings(m);
    if m_rings.is_empty() {
        return cutout.to_vec();
    }
    let rings = split_footprint_rings(cutout);
    if rings.is_empty() {
        return m.to_vec();
    }
    let mut out = Vec::new();
    for (ri, ring) in rings.iter().enumerate() {
        if ring.len() < 3 {
            continue;
        }
        let n = ring.len() as f64;
        let keep = {
            let s = ring.iter().fold((0.0, 0.0), |acc, p| (acc.0 + p.0, acc.1 + p.1));
            (s.0 / n, s.1 / n)
        };
        // One notch edge per neighbouring ring so a middle span is bounded
        // on both sides. Duplicate edges (same neighbour pair) are skipped.
        let mut notches: Vec<((f64, f64), (f64, f64))> = Vec::new();
        for (rj, other) in rings.iter().enumerate() {
            if rj == ri {
                continue;
            }
            let mut best: Option<((f64, f64), (f64, f64), f64)> = None;
            for i in 0..ring.len() {
                let a = ring[i];
                let b = ring[(i + 1) % ring.len()];
                let mid = ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
                let d = other
                    .iter()
                    .map(|p| dist(*p, mid))
                    .fold(f64::INFINITY, f64::min);
                if best.is_none_or(|(_, _, bd)| d < bd) {
                    best = Some((a, b, d));
                }
            }
            if let Some((a, b, _)) = best {
                let dup = notches.iter().any(|&(ea, eb)| {
                    (dist(ea, a) < 1e-12 && dist(eb, b) < 1e-12)
                        || (dist(ea, b) < 1e-12 && dist(eb, a) < 1e-12)
                });
                if !dup {
                    notches.push((a, b));
                }
            }
        }
        for m_ring in &m_rings {
            if m_ring.len() < 3 {
                continue;
            }
            let clipped = if notches.is_empty() {
                clip_subject_to_convex_clip(m_ring, ring)
            } else {
                let mut cur = m_ring.clone();
                for &(ea, eb) in &notches {
                    if cur.len() < 3 {
                        break;
                    }
                    cur = clip_closed_to_halfplane(&cur, ea, eb, keep);
                }
                cur
            };
            if clipped.len() >= 3 {
                if !out.is_empty() {
                    out.push((f64::NAN, f64::NAN));
                }
                out.extend(clipped);
            }
        }
    }
    if out.is_empty() {
        cutout.to_vec()
    } else {
        out
    }
}

/// Split a simple ring along the finite chord `a`–`b` (extended by a few
/// micrometres so chord ends that sit exactly on the ring boundary still
/// cross it). Only boundary crossings *inside* the chord extent count, so
/// unrelated parts of a polyline footprint that happen to lie on the chord's
/// supporting line are left alone. Returns the resulting simple rings (the
/// input ring when the chord does not cut it).
pub(crate) fn split_ring_along_chord(
    ring: &[(f64, f64)],
    a: (f64, f64),
    b: (f64, f64),
) -> Vec<Vec<(f64, f64)>> {
    split_ring_along_chord_depth(ring, a, b, 0)
}

fn split_ring_along_chord_depth(
    ring: &[(f64, f64)],
    a: (f64, f64),
    b: (f64, f64),
    depth: usize,
) -> Vec<Vec<(f64, f64)>> {
    let n = ring.len();
    let len = dist(a, b);
    if n < 3 || depth > 16 || len < 1e-9 {
        return vec![ring.to_vec()];
    }
    let u = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    let nrm = (-u.1, u.0);
    let side = |p: (f64, f64)| (p.0 - a.0) * nrm.0 + (p.1 - a.1) * nrm.1;
    let along = |p: (f64, f64)| (p.0 - a.0) * u.0 + (p.1 - a.1) * u.1;
    let (t_lo, t_hi) = (-1e-5, len + 1e-5);
    let eps = 1e-7;
    let sg: Vec<i8> = ring
        .iter()
        .map(|&p| {
            let s = side(p);
            if s > eps {
                1
            } else if s < -eps {
                -1
            } else {
                0
            }
        })
        .collect();

    let mut aug: Vec<(f64, f64)> = Vec::with_capacity(n + 4);
    let mut marks: Vec<(usize, f64)> = Vec::new();
    for i in 0..n {
        let j = (i + 1) % n;
        aug.push(ring[i]);
        if sg[i] == 0 {
            let prev = sg[(i + n - 1) % n];
            let next = sg[j];
            if prev != 0 && next != 0 && prev * next < 0 {
                let t = along(ring[i]);
                if (t_lo..=t_hi).contains(&t) {
                    marks.push((aug.len() - 1, t));
                }
            }
        } else if sg[j] != 0 && sg[i] * sg[j] < 0 {
            let s0 = side(ring[i]);
            let s1 = side(ring[j]);
            let f = s0 / (s0 - s1);
            let q = (
                ring[i].0 + (ring[j].0 - ring[i].0) * f,
                ring[i].1 + (ring[j].1 - ring[i].1) * f,
            );
            let t = along(q);
            if (t_lo..=t_hi).contains(&t) {
                aug.push(q);
                marks.push((aug.len() - 1, t));
            }
        }
    }
    if marks.len() < 2 || marks.len() % 2 != 0 {
        return vec![ring.to_vec()];
    }
    marks.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap_or(std::cmp::Ordering::Equal));
    let (i0, i1) = (marks[0].0.min(marks[1].0), marks[0].0.max(marks[1].0));
    if i0 == i1 {
        return vec![ring.to_vec()];
    }
    let poly1: Vec<(f64, f64)> = aug[i0..=i1].to_vec();
    let poly2: Vec<(f64, f64)> = aug[i1..].iter().chain(aug[..=i0].iter()).copied().collect();
    let mut out = Vec::new();
    for poly in [poly1, poly2] {
        if poly.len() < 3 || super::geometry::area(&poly) < 1e-9 {
            continue;
        }
        out.extend(split_ring_along_chord_depth(&poly, a, b, depth + 1));
    }
    if out.is_empty() {
        vec![ring.to_vec()]
    } else {
        out
    }
}

/// Merge the two butt-jointed stem rings of an X crossing (as produced by the
/// junction solver) into the crossing wall's already-built layer footprint.
///
/// Unlike [`merge_through_cutout_preserving_miter`] this never uses infinite
/// half-planes: the footprint is **cut along the two finite butt chords** of the
/// stems and only the piece(s) lying wholly inside the through-wall band
/// between the chords are removed. A polyline crossing wall (U / zig-zag)
/// that crosses the same or another wall again therefore keeps every leg:
/// legs only lose the band slice at this crossing. Falls back to the legacy
/// merge when the cutout is not a clean pair of butting stems.
pub fn merge_crossing_stems_local(
    mitered: Option<&[(f64, f64)]>,
    cutout: &[(f64, f64)],
) -> Vec<(f64, f64)> {
    let Some(m) = mitered else {
        return cutout.to_vec();
    };
    let m_rings = split_footprint_rings(m);
    let rings = split_footprint_rings(cutout);
    if m_rings.is_empty() {
        return cutout.to_vec();
    }
    if rings.len() != 2 {
        return merge_through_cutout_preserving_miter(mitered, cutout);
    }
    // Butt edge: the ring edge nearest the other ring; the ring body lies on
    // the side its neighbouring vertices are on.
    let butt = |ring: &[(f64, f64)], other: &[(f64, f64)]| -> Option<((f64, f64), (f64, f64), f64)> {
        let k = ring.len();
        let mut best: Option<(usize, f64)> = None;
        for i in 0..k {
            let (p, q) = (ring[i], ring[(i + 1) % k]);
            let mid = ((p.0 + q.0) * 0.5, (p.1 + q.1) * 0.5);
            let d = other.iter().map(|o| dist(*o, mid)).fold(f64::INFINITY, f64::min);
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        let (i, _) = best?;
        let (p, q) = (ring[i], ring[(i + 1) % k]);
        let l = dist(p, q);
        if l < 1e-9 {
            return None;
        }
        let nrm = (-(q.1 - p.1) / l, (q.0 - p.0) / l);
        let sd = |v: (f64, f64)| (v.0 - p.0) * nrm.0 + (v.1 - p.1) * nrm.1;
        let body = sd(ring[(i + k - 1) % k]) + sd(ring[(i + 2) % k]);
        if body.abs() < 1e-9 {
            return None;
        }
        Some((p, q, body.signum()))
    };
    let (Some((a0, b0, s0)), Some((a1, b1, s1))) =
        (butt(&rings[0], &rings[1]), butt(&rings[1], &rings[0]))
    else {
        return merge_through_cutout_preserving_miter(mitered, cutout);
    };
    let sd_of = |a: (f64, f64), b: (f64, f64)| {
        let l = dist(a, b).max(1e-12);
        let nrm = (-(b.1 - a.1) / l, (b.0 - a.0) / l);
        move |v: (f64, f64)| (v.0 - a.0) * nrm.0 + (v.1 - a.1) * nrm.1
    };
    let sd0 = sd_of(a0, b0);
    let sd1 = sd_of(a1, b1);
    let mid0 = ((a0.0 + b0.0) * 0.5, (a0.1 + b0.1) * 0.5);
    let mid1 = ((a1.0 + b1.0) * 0.5, (a1.1 + b1.1) * 0.5);
    // A real gap (through-wall band) exists only when each butt edge lies
    // outside the other stem's half-plane; overlapping stems (explicit
    // FarFace / NoExtend overrides) keep the legacy behaviour.
    if sd1(mid0) * s1 >= -1e-6 || sd0(mid1) * s0 >= -1e-6 {
        return merge_through_cutout_preserving_miter(mitered, cutout);
    }
    let tol = 1e-5;
    let mut out: Vec<(f64, f64)> = Vec::new();
    for ring in &m_rings {
        let mut pieces = Vec::new();
        for piece in split_ring_along_chord(ring, a0, b0) {
            pieces.extend(split_ring_along_chord(&piece, a1, b1));
        }
        // A prior crossing may have already separated a distant leg into
        // its own ring. Never discard such a ring merely because it lies in
        // the infinite *extension* of the new notch band: only the finite
        // chords at this crossing are allowed to remove material.
        let untouched = pieces.len() == 1 && pieces[0] == *ring;
        for piece in pieces {
            if piece.len() < 3 {
                continue;
            }
            let in_band = !untouched && piece
                .iter()
                .all(|&v| sd0(v) * s0 <= tol && sd1(v) * s1 <= tol);
            if in_band {
                continue;
            }
            if !out.is_empty() {
                out.push((f64::NAN, f64::NAN));
            }
            out.extend(piece);
        }
    }
    if out.is_empty() {
        cutout.to_vec()
    } else {
        out
    }
}

/// Split a layer footprint (which may have mitered, extended, or butt-joined
/// ends) by wall openings.
///
/// For each remaining free span along the axis:
/// - If the span starts at the wall start (s0 ≈ 0), the start of the footprint
///   (including any join/miter at end 0) is preserved untouched.
///   Otherwise, it is sliced cleanly by the opening reveal perpendicular to the axis at s0.
/// - If the span ends at the wall end (s1 ≈ L), the end of the footprint
///   (including any join/miter at end L) is preserved untouched.
///   Otherwise, it is sliced cleanly by the opening reveal perpendicular to the axis at s1.
///
/// Multiple rings (e.g. from T-through cutouts) separated by NaN are supported.
pub fn split_mitered_footprint_by_openings(
    footprint: &[(f64, f64)],
    axis: &[(f64, f64)],
    openings: &[Opening],
) -> Vec<Vec<(f64, f64)>> {
    if openings.is_empty() || axis.len() < 2 || footprint.len() < 3 {
        let rings = split_footprint_rings(footprint);
        return if rings.is_empty() {
            vec![footprint.to_vec()]
        } else {
            rings
        };
    }
    let length = axis_length(axis);
    if length <= 1e-9 {
        return vec![footprint.to_vec()];
    }
    let through_openings: Vec<Opening> = openings
        .iter()
        .filter(|o| !(o.kind == super::openings::OpeningKind::Breakthrough && o.depth.is_some()))
        .cloned()
        .collect();
    let spans = remaining_axis_spans(length, &through_openings);
    let rings = split_footprint_rings(footprint);
    let mut pieces = Vec::new();

    for (s0, s1) in spans {
        if s1 - s0 <= 1e-6 {
            continue;
        }
        for ring in &rings {
            if ring.len() < 3 {
                continue;
            }
            let mut clipped = ring.clone();

            // 1. Cut at start of span (s0) if s0 is strictly after the wall start
            if s0 > 1e-4 {
                if let Some((p0, t0)) = point_and_tangent_at_distance(axis, s0) {
                    let n0 = (-t0.1, t0.0);
                    let a = p0;
                    let b = (p0.0 + n0.0, p0.1 + n0.1);
                    let keep = (p0.0 + t0.0, p0.1 + t0.1);
                    clipped = clip_closed_to_halfplane(&clipped, a, b, keep);
                }
            }

            // 2. Cut at end of span (s1) if s1 is strictly before the wall end
            if s1 < length - 1e-4 {
                if let Some((p1, t1)) = point_and_tangent_at_distance(axis, s1) {
                    let n1 = (-t1.1, t1.0);
                    let a = p1;
                    let b = (p1.0 + n1.0, p1.1 + n1.1);
                    let keep = (p1.0 - t1.0, p1.1 - t1.1);
                    clipped = clip_closed_to_halfplane(&clipped, a, b, keep);
                }
            }

            if clipped.len() >= 3 && super::geometry::area(&clipped) > 1e-6 {
                ensure_ccw(&mut clipped);
                pieces.push(clipped);
            }
        }
    }

    if pieces.is_empty() {
        rings
    } else {
        pieces
    }
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
pub(crate) fn near_face_line(
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
pub(crate) fn through_outer_near_face_line(
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
    through_wall_cutout_footprints_with_gaps(
        axis_through,
        layers_through,
        axis_stem,
        layers_stem,
        stem_end,
        bulges_through,
        bulges_stem,
        &[],
    )
}

/// Like [`through_wall_cutout_footprints_with_bulges`], then applies manual
/// [`LayerGapOverride`]s (user-defined interruptions between two stem layers).
pub fn through_wall_cutout_footprints_with_gaps(
    axis_through: &[(f64, f64)],
    layers_through: &[MiterLayer],
    axis_stem: &[(f64, f64)],
    layers_stem: &[MiterLayer],
    stem_end: usize,
    bulges_through: &[f64],
    bulges_stem: &[f64],
    gaps: &[LayerGapOverride],
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
    let mut out = vec![None; layers_through.len()];
    if let Some((core_s, core_t)) = pair_structural_cores(layers_stem, layers_through) {
        if core_t < contours_t.len() {
            let (ref core_t1, ref core_t2) = contours_t[core_t];
            let toward = through_offset_toward_stem(axis_stem, stem_end, axis_through);
            let punchers =
                punchers_outside_in(layers_stem, layers_through, Some((core_s, core_t)), toward);
            for ti in 0..layers_through.len() {
                if ti == core_t || ti >= contours_t.len() {
                    continue;
                }
                if is_structural_function(&layers_through[ti].function) {
                    continue;
                }
                let (ref t1, ref t2) = contours_t[ti];
                if !through_layer_on_approach_side(t1, t2, core_t1, core_t2, axis_stem, stem_end)
                {
                    continue;
                }
                let overlapping: Vec<usize> = punchers
                    .get(ti)
                    .map(|v| {
                        v.iter()
                            .copied()
                            .filter(|&si| si < contours_s.len())
                            .collect()
                    })
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
        }
    }
    apply_manual_layer_gaps(
        &mut out,
        gaps,
        layers_through,
        layers_stem,
        &contours_t,
        &contours_s,
        stem_end,
        axis_stem,
    );
    out
}

fn miter_layer_index(layers: &[MiterLayer], r: &LayerRef) -> Option<usize> {
    if let Some(id) = r.layer_id {
        if !id.is_nil() {
            if let Some(i) = layers.iter().position(|l| l.layer_id == id) {
                return Some(i);
            }
        }
    }
    layers.iter().enumerate().find_map(|(i, l)| {
        (l.material.eq_ignore_ascii_case(&r.material_id) && i == r.index).then_some(i)
    }).or_else(|| {
        layers
            .iter()
            .position(|l| l.material.eq_ignore_ascii_case(&r.material_id))
    })
}

fn apply_manual_layer_gaps(
    out: &mut [Option<Vec<(f64, f64)>>],
    gaps: &[LayerGapOverride],
    layers_through: &[MiterLayer],
    layers_stem: &[MiterLayer],
    contours_t: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    contours_s: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    stem_end: usize,
    axis_stem: &[(f64, f64)],
) {
    for gap in gaps {
        let Some(ti) = miter_layer_index(layers_through, &gap.layer) else {
            continue;
        };
        let Some(si0) = miter_layer_index(layers_stem, &gap.from) else {
            continue;
        };
        let Some(si1) = miter_layer_index(layers_stem, &gap.to) else {
            continue;
        };
        if ti >= contours_t.len() {
            continue;
        }
        let lo = si0.min(si1);
        let hi = si0.max(si1);
        let overlapping: Vec<usize> = (lo..=hi).filter(|&si| si < contours_s.len()).collect();
        if overlapping.is_empty() {
            continue;
        }
        let (ref t1, ref t2) = contours_t[ti];
        if let Some(fp) = notch_through_layer_from_stem_layers(
            t1,
            t2,
            &layers_through[ti],
            &overlapping,
            layers_stem,
            contours_s,
            stem_end,
            axis_stem,
        ) {
            out[ti] = Some(fp);
        }
    }
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
            let is_stem_structural = core_s == Some(si) || is_structural_function(&stem.function);
            let same_mat = !stem.material.is_empty()
                && stem.material.eq_ignore_ascii_case(&thru.material);
            if same_mat && !is_stem_structural {
                punch.push(si);
                consumed[si] = true;
                continue;
            }
            let deeper_same = layers_through.iter().enumerate().any(|(j, l)| {
                if j == ti || Some(j) == core_t || is_structural_function(&l.function) {
                    return false;
                }
                if layer_outerness(l, toward_stem) >= layer_outerness(thru, toward_stem) - 1e-12 {
                    return false;
                }
                !stem.material.is_empty() && stem.material.eq_ignore_ascii_case(&l.material)
            });
            if is_stem_structural || deeper_same {
                punch.push(si);
            } else {
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
mod tests;
