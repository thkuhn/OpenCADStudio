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

use super::contour::layer_contours;
use super::join::{JoinKind, Junction, JunctionRole};

/// One wall layer as consumed by the miter matcher: geometry plus optional
/// identity fields used for cross-wall pairing.
#[derive(Debug, Clone, PartialEq)]
pub struct MiterLayer {
    pub thickness: f64,
    pub gap_before: f64,
    pub material: String,
    pub function: String,
}

impl MiterLayer {
    /// Geometry-only layer (empty material/function). Used by tests and
    /// callers that don't carry identity metadata.
    pub fn geom(thickness: f64, gap_before: f64) -> Self {
        Self {
            thickness,
            gap_before,
            material: String::new(),
            function: String::new(),
        }
    }

    pub fn with_id(
        thickness: f64,
        gap_before: f64,
        material: impl Into<String>,
        function: impl Into<String>,
    ) -> Self {
        Self {
            thickness,
            gap_before,
            material: material.into(),
            function: function.into(),
        }
    }

    fn as_geom(&self) -> (f64, f64) {
        (self.thickness, self.gap_before)
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
}

/// For each layer of wall A, return either a miter-clipped closed footprint
/// polygon, or `None` when that layer can't be matched against wall B
/// (caller should fall back to `corner_override` for that layer).
///
/// `end_a` is the joined vertex index on A. `end_b` is `Some` for an L-join
/// or the stem of a T-join; `None` means B is the through-wall of a T.
pub fn mitered_layer_footprints(
    axis_a: &[(f64, f64)],
    layers_a: &[MiterLayer],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    end_b: Option<usize>,
    _kind: JoinKind,
) -> Vec<Option<Vec<(f64, f64)>>> {
    if axis_a.len() < 2 || axis_b.len() < 2 || layers_a.is_empty() {
        return vec![None; layers_a.len()];
    }
    if end_a >= axis_a.len() {
        return vec![None; layers_a.len()];
    }

    let geom_a: Vec<(f64, f64)> = layers_a.iter().map(MiterLayer::as_geom).collect();
    let geom_b: Vec<(f64, f64)> = layers_b.iter().map(MiterLayer::as_geom).collect();
    let contours_a = layer_contours(axis_a, &geom_a);
    let contours_b = layer_contours(axis_b, &geom_b);
    if contours_a.is_empty() || contours_b.is_empty() {
        return vec![None; layers_a.len()];
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

/// Pair each layer of wall A to at most one layer of wall B.
///
/// Matching priority:
/// 1. `material` equality (and `function` equality when both are non-empty)
/// 2. closest cumulative offset-from-axis (tie-break / no material match)
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
        done_a[i] = true;
        if ambiguous {
            // Leave out[i] = None (fallback to corner_override).
            continue;
        }
        out[i] = Some(j);
        used_b[j] = true;
    }

    out
}

/// Identity match class: 0 = material+function, 1 = material only,
/// 2 = offset-only (no identity). Lower is better.
fn identity_class(a: &MiterLayer, b: &MiterLayer) -> u8 {
    let mat = !a.material.is_empty() && a.material == b.material;
    if !mat {
        return 2;
    }
    let fun = !a.function.is_empty() && !b.function.is_empty() && a.function == b.function;
    if fun {
        0
    } else {
        1
    }
}

/// Cumulative centre offset of each layer from the reference axis, matching
/// the stacking convention in [`layer_contours`] (centreline at mid-thickness,
/// layers stacked from the negative offset side outward).
fn layer_center_offsets(layers: &[MiterLayer]) -> Vec<f64> {
    let total: f64 = layers.iter().map(|l| l.thickness + l.gap_before).sum();
    let mut cur = -total * 0.5;
    let mut centers = Vec::with_capacity(layers.len());
    for l in layers {
        let start = cur + l.gap_before;
        let end = start + l.thickness;
        centers.push(0.5 * (start + end));
        cur = end;
    }
    centers
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
                            (Some((new_a1, new_a2)), None) => {
                                Some(rebuild_footprint(a_b1, a_b2, end_a, new_a1, new_a2))
                            }
                            (None, Some((new_a1, new_a2))) => {
                                Some(rebuild_footprint(a_b1, a_b2, end_a, new_a1, new_a2))
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
                (None, None) => None,
            };

            out[wi][li] = fp.filter(|p| p.len() >= 3);
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

    let prev_a = if end_a == 0 { 1 } else { end_a - 1 };
    let a1_line = extended_line(a_b1[prev_a], a_b1[end_a]);
    let a2_line = extended_line(a_b2[prev_a], a_b2[end_a]);
    let (b1_line, b2_line) = other_boundary_lines(b_b1, b_b2, end_b, axis_a, axis_b, end_a)?;

    let i_a1_b1 = intersect_lines_2d(a1_line.0, a1_line.1, b1_line.0, b1_line.1)?;
    let i_a1_b2 = intersect_lines_2d(a1_line.0, a1_line.1, b2_line.0, b2_line.1)?;
    let i_a2_b1 = intersect_lines_2d(a2_line.0, a2_line.1, b1_line.0, b1_line.1)?;
    let i_a2_b2 = intersect_lines_2d(a2_line.0, a2_line.1, b2_line.0, b2_line.1)?;

    let pair_direct = (i_a1_b1, i_a2_b2);
    let pair_cross = (i_a1_b2, i_a2_b1);
    let len_direct = dist(pair_direct.0, pair_direct.1);
    let len_cross = dist(pair_cross.0, pair_cross.1);
    let (new_a1, new_a2) = if (len_direct - len_cross).abs() <= 1e-9 {
        let bis = corner_bisector_dir(axis_a, end_a, axis_b, end_b);
        let score = |p: (f64, f64), q: (f64, f64)| -> f64 {
            let (dx, dy) = (q.0 - p.0, q.1 - p.1);
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-12 || bis == (0.0, 0.0) {
                return 0.0;
            }
            ((dx / len) * bis.0 + (dy / len) * bis.1).abs()
        };
        if score(pair_direct.0, pair_direct.1) >= score(pair_cross.0, pair_cross.1) {
            pair_direct
        } else {
            pair_cross
        }
    } else if len_direct < len_cross {
        pair_direct
    } else {
        pair_cross
    };
    Some((new_a1, new_a2))
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
    footprint
}

/// Build a closed footprint for one layer of wall A, with the joined end
/// replaced by the diagonal miter against wall B's matching layer.
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
    let (new_a1, new_a2) =
        miter_end_points(a_b1, a_b2, end_a, b_b1, b_b2, end_b, axis_a, axis_b)?;
    Some(rebuild_footprint(a_b1, a_b2, end_a, new_a1, new_a2))
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

/// Unit direction of the angle bisector at a join, formed from the two wall
/// directions that leave the corner along each axis. Used as a tie-break when
/// both miter pairings have equal segment length.
fn corner_bisector_dir(
    axis_a: &[(f64, f64)],
    end_a: usize,
    axis_b: &[(f64, f64)],
    end_b: Option<usize>,
) -> (f64, f64) {
    let leave = |axis: &[(f64, f64)], end: usize| -> (f64, f64) {
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
    };

    let la = leave(axis_a, end_a);
    let lb = if let Some(eb) = end_b {
        leave(axis_b, eb)
    } else if axis_b.len() >= 2 {
        // T through-wall: use the through-axis direction near A's join.
        let join = axis_a.get(end_a).copied().unwrap_or((0.0, 0.0));
        let mut best_seg = 0usize;
        let mut best_d = f64::INFINITY;
        for s in 0..axis_b.len() - 1 {
            let d = point_seg_dist(join, axis_b[s], axis_b[s + 1]);
            if d < best_d {
                best_d = d;
                best_seg = s;
            }
        }
        let (dx, dy) = (
            axis_b[best_seg + 1].0 - axis_b[best_seg].0,
            axis_b[best_seg + 1].1 - axis_b[best_seg].1,
        );
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-12 {
            (0.0, 0.0)
        } else {
            (dx / len, dy / len)
        }
    } else {
        (0.0, 0.0)
    };

    let (bx, by) = (la.0 + lb.0, la.1 + lb.1);
    let len = (bx * bx + by * by).sqrt();
    if len < 1e-12 {
        (0.0, 0.0)
    } else {
        (bx / len, by / len)
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::join::{JoinKind, JunctionParticipant};

    fn close(a: (f64, f64), b: (f64, f64), tol: f64) -> bool {
        dist(a, b) < tol
    }

    fn g(t: f64) -> MiterLayer {
        MiterLayer::geom(t, 0.0)
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
        // Stem A: (5,1)->(5,10) will join onto through wall B at (5,0).
        // After join the stem end is at (5,0); through wall unchanged.
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
        let fp_a = fps_a[0].as_ref().expect("stem layer should miter");

        // Through wall boundaries at y=±0.1; stem boundaries at x=5±0.1.
        // Short miter diagonal: (5.1, -0.1) and (4.9, 0.1) — or the
        // through-wall face cut at y=±0.1 depending on pairing. Either way
        // the stem's end vertices must lie on the through wall's layer band
        // (|y| ≈ 0.1) rather than stopping at the axis (y=0).
        let max_abs_y_at_end = fp_a
            .iter()
            .filter(|(x, _)| (*x - 5.0).abs() < 0.15)
            .map(|(_, y)| y.abs())
            .fold(0.0_f64, f64::max);
        assert!(
            max_abs_y_at_end > 0.05,
            "stem end should reach the through wall's layer face, got max |y|={max_abs_y_at_end}, fp={fp_a:?}"
        );
    }

    #[test]
    fn mismatched_layer_count_returns_none_for_unmatched() {
        let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
        let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
        // A has two layers, B has one — only one can match (by offset).
        let layers_a = vec![g(0.2), g(0.05)];
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
    fn same_composition_different_order_matches_by_material() {
        // A: Brick (outer), Insulation, Concrete (inner)
        // B: same materials reversed in the list — must still pair Brick↔Brick etc.
        let layers_a = vec![
            MiterLayer::with_id(0.1, 0.0, "Brick", "Finish"),
            MiterLayer::with_id(0.05, 0.0, "Insulation", "Insulation"),
            MiterLayer::with_id(0.2, 0.0, "Concrete", "Structural"),
        ];
        let layers_b = vec![
            MiterLayer::with_id(0.2, 0.0, "Concrete", "Structural"),
            MiterLayer::with_id(0.05, 0.0, "Insulation", "Insulation"),
            MiterLayer::with_id(0.1, 0.0, "Brick", "Finish"),
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
            MiterLayer::with_id(0.1, 0.0, "Brick", "Finish"),
            MiterLayer::with_id(0.2, 0.0, "Concrete", "Structural"),
        ];
        let layers_b = vec![MiterLayer::with_id(0.15, 0.0, "Insulation", "Insulation")];

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
            MiterLayer::with_id(0.1, 0.0, "Brick", "Finish"),
            MiterLayer::with_id(0.2, 0.0, "Concrete", "Structural"),
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
}
