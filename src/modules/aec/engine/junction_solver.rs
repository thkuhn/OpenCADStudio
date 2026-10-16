//! Unified wall-junction classification, axis trim, and layer footprints.
//!
//! [`solve`] / [`solve_pair`] are the single source for L/T/N classification,
//! End vs Mid, trimmed axes, and per-layer miter/cutout polygons.

use std::collections::HashSet;

use glam::DVec3;

use super::join::{
    apply_junction_to_axes, detect_junctions_with_bulges, join_wall_axes_as_l_with_bulges,
    join_wall_axes_with_bulges, junction_rays, merge_partner_junction_override, JoinError,
    JoinKind, Junction, JunctionOverride, JunctionRole, LayerRef, JUNCTION_TOLERANCE,
};
use super::miter::{
    is_structural_function, junction_wall_geoms,
    merge_crossing_stems_local, merge_end_footprints, merge_through_cutout_preserving_miter,
    mitered_junction_layer_footprints_with_overrides,
    mitered_layer_footprints_with_override_and_bulges,
    through_wall_cutout_footprints_with_gaps,
    JunctionWallGeom, MiterLayer,
};

/// One wall as seen by [`solve`].
#[derive(Debug, Clone)]
pub struct WallJoinInput {
    pub axis: Vec<DVec3>,
    pub bulges: Vec<f64>,
    pub layers: Vec<MiterLayer>,
    /// Override at vertex 0, if any.
    pub override_start: Option<JunctionOverride>,
    /// Override at the last vertex, if any.
    pub override_end: Option<JunctionOverride>,
    /// T-junction layer gaps stored on the through wall (no endpoint).
    pub override_span: Option<JunctionOverride>,
}

impl WallJoinInput {
    pub fn from_axis(axis: Vec<DVec3>) -> Self {
        Self {
            axis,
            bulges: Vec::new(),
            layers: Vec::new(),
            override_start: None,
            override_end: None,
            override_span: None,
        }
    }

    fn override_at(&self, end: usize) -> Option<&JunctionOverride> {
        if end == 0 {
            self.override_start.as_ref()
        } else {
            self.override_end.as_ref()
        }
    }
}

/// L-corner, T (stem + through), X (4-way / crossing), or N-way (≥3 participants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolvedJoinKind {
    L,
    T,
    X,
    NWay,
}

/// Result of classifying and trimming one detected junction.
#[derive(Debug, Clone)]
pub struct SolvedJunction {
    pub junction: Junction,
    pub kind: SolvedJoinKind,
    /// One polyline per input wall (through-walls left unshortened).
    pub trimmed_axes: Vec<Vec<DVec3>>,
    /// Per input wall, per layer (`None` → unmatched-layer corner fallback).
    pub footprints: Vec<Vec<Option<Vec<(f64, f64)>>>>,
}

/// Pairwise L/T join used by document `AEC_WALLJOIN` / auto-join.
#[derive(Debug, Clone)]
pub struct SolvedPair {
    pub axis_a: Vec<DVec3>,
    pub axis_b: Vec<DVec3>,
    pub kind: JoinKind,
    pub end_a: Option<usize>,
    pub end_b: Option<usize>,
    pub footprints_a: Vec<Option<Vec<(f64, f64)>>>,
    pub footprints_b: Vec<Option<Vec<(f64, f64)>>>,
}

/// Solved multi-junction state for a network/cluster of walls.
#[derive(Debug, Clone)]
pub struct SolvedWallNetwork {
    /// One trimmed polyline per input wall.
    pub trimmed_axes: Vec<Vec<DVec3>>,
    /// Combined multi-end and span footprints per input wall (indexed `[wall_idx][layer_idx]`).
    pub footprints: Vec<Vec<Option<Vec<(f64, f64)>>>>,
    /// All solved junctions detected among the input walls.
    pub junctions: Vec<SolvedJunction>,
}

/// Detect and solve all junctions among `walls` as a unified network,
/// returning combined trimmed axes and merged multi-end + span footprints for every wall.
pub fn solve_network(walls: &[WallJoinInput], tol: f64) -> SolvedWallNetwork {
    let cluster_tol = if tol > 0.0 { tol } else { JUNCTION_TOLERANCE };
    let solved_junctions = solve(walls, cluster_tol);
    solve_network_from_solved_junctions(walls, solved_junctions, None)
}

/// Solve a pre-filtered list of junctions among `walls` as a unified network.
/// If `allowed_trim_ends` is specified, only the designated `(wall_index, endpoint)`
/// pairs are modified in `trimmed_axes`.
pub fn solve_network_with_junctions(
    walls: &[WallJoinInput],
    junctions: Vec<Junction>,
    allowed_trim_ends: Option<&HashSet<(usize, usize)>>,
) -> SolvedWallNetwork {
    let mut solved_junctions = Vec::with_capacity(junctions.len());
    for junction in junctions {
        let kind = classify_kind(walls, &junction);
        let trimmed_axes = trim_axes_for_kind(walls, &junction, kind);
        let footprints = compute_footprints(walls, &junction, kind, &trimmed_axes);
        solved_junctions.push(SolvedJunction {
            junction,
            kind,
            trimmed_axes,
            footprints,
        });
    }
    solve_network_from_solved_junctions(walls, solved_junctions, allowed_trim_ends)
}

fn solve_network_from_solved_junctions(
    walls: &[WallJoinInput],
    solved_junctions: Vec<SolvedJunction>,
    allowed_trim_ends: Option<&HashSet<(usize, usize)>>,
) -> SolvedWallNetwork {
    let mut trimmed_axes: Vec<Vec<DVec3>> = walls.iter().map(|w| w.axis.clone()).collect();

    // 1. Accumulate trimmed endpoints from all detected junctions
    for sj in &solved_junctions {
        for p in &sj.junction.participants {
            if p.wall_index >= trimmed_axes.len() || p.wall_index >= sj.trimmed_axes.len() {
                continue;
            }
            let orig_len = walls[p.wall_index].axis.len();
            let j_axis = &sj.trimmed_axes[p.wall_index];
            if j_axis.is_empty() {
                continue;
            }
            match p.role {
                JunctionRole::Endpoint(0) => {
                    let allow = allowed_trim_ends.map_or(true, |set| set.contains(&(p.wall_index, 0)));
                    if allow {
                        trimmed_axes[p.wall_index][0] = j_axis[0];
                    }
                }
                JunctionRole::Endpoint(last) if last == orig_len.saturating_sub(1) => {
                    let allow = allowed_trim_ends.map_or(true, |set| set.contains(&(p.wall_index, last)));
                    if allow {
                        let cur_last = trimmed_axes[p.wall_index].len().saturating_sub(1);
                        let j_last = j_axis.len().saturating_sub(1);
                        trimmed_axes[p.wall_index][cur_last] = j_axis[j_last];
                    }
                }
                _ => {}
            }
        }
    }

    // 2. Recompute footprints for all junctions on the globally trimmed axes
    let trimmed_walls: Vec<WallJoinInput> = walls
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let mut tw = w.clone();
            tw.axis = trimmed_axes[i].clone();
            tw
        })
        .collect();

    let final_solved_junctions: Vec<SolvedJunction> = solved_junctions
        .into_iter()
        .map(|mut sj| {
            sj.trimmed_axes = trimmed_axes.clone();
            sj.footprints = compute_footprints(&trimmed_walls, &sj.junction, sj.kind, &trimmed_axes);
            sj
        })
        .collect();

    // 3. Compute combined footprints for each wall
    let mut all_wall_footprints: Vec<Vec<Option<Vec<(f64, f64)>>>> =
        Vec::with_capacity(walls.len());

    for (wi, w) in walls.iter().enumerate() {
        let w_axis = &trimmed_axes[wi];
        let w_axis_2d = axis_2d(w_axis);
        let offsets: Vec<(f64, f64)> = w.layers.iter().map(|l| (l.thickness, l.axis_offset)).collect();
        let base_contours = super::contour::layer_contours_with_bulges(&w_axis_2d, &w.bulges, &offsets);
        let base_footprints: Vec<Vec<(f64, f64)>> = base_contours
            .iter()
            .map(|(c1, c2)| super::contour::closed_layer_footprint(c1, c2).points)
            .collect();

        let mut end0_footprints: Vec<Option<Vec<(f64, f64)>>> = vec![None; w.layers.len()];
        let mut end1_footprints: Vec<Option<Vec<(f64, f64)>>> = vec![None; w.layers.len()];
        let mut span_cutouts: Vec<Vec<Option<Vec<(f64, f64)>>>> = Vec::new();
        let mut span_uses_local_x_cutouts = false;
        let mut is_x_crossing_stem = false;
        let mut x_crossing_stem_footprints: Vec<Option<Vec<(f64, f64)>>> = vec![None; w.layers.len()];

        for sj in &final_solved_junctions {
            let Some(p) = sj.junction.participants.iter().find(|part| part.wall_index == wi) else {
                continue;
            };
            let Some(fps) = sj.footprints.get(wi) else {
                continue;
            };

            match p.role {
                JunctionRole::Endpoint(0) => {
                    if sj.kind == SolvedJoinKind::X
                        && sj.junction.participants.len() == 2
                        && x_priority_pair(walls, &sj.junction).is_some_and(|(_, cross)| cross == wi)
                    {
                        is_x_crossing_stem = true;
                        for (li, stem_fp) in fps.iter().enumerate() {
                            if let Some(sfp) = stem_fp {
                                if li < x_crossing_stem_footprints.len() {
                                    x_crossing_stem_footprints[li] = Some(match &x_crossing_stem_footprints[li] {
                                        Some(prev) => merge_crossing_stems_local(Some(prev.as_slice()), sfp),
                                        None => sfp.clone(),
                                    });
                                }
                            }
                        }
                    } else {
                        for (li, fp) in fps.iter().enumerate() {
                            if li < end0_footprints.len() {
                                end0_footprints[li] = fp.clone();
                            }
                        }
                    }
                }
                JunctionRole::Endpoint(last) if last == w.axis.len().saturating_sub(1) && last > 0 => {
                    if sj.kind == SolvedJoinKind::X
                        && sj.junction.participants.len() == 2
                        && x_priority_pair(walls, &sj.junction).is_some_and(|(_, cross)| cross == wi)
                    {
                        is_x_crossing_stem = true;
                        for (li, stem_fp) in fps.iter().enumerate() {
                            if let Some(sfp) = stem_fp {
                                if li < x_crossing_stem_footprints.len() {
                                    x_crossing_stem_footprints[li] = Some(match &x_crossing_stem_footprints[li] {
                                        Some(prev) => merge_crossing_stems_local(Some(prev.as_slice()), sfp),
                                        None => sfp.clone(),
                                    });
                                }
                            }
                        }
                    } else {
                        for (li, fp) in fps.iter().enumerate() {
                            if li < end1_footprints.len() {
                                end1_footprints[li] = fp.clone();
                            }
                        }
                    }
                }
                JunctionRole::Endpoint(_) => {
                    for (li, fp) in fps.iter().enumerate() {
                        if li < end0_footprints.len() {
                            end0_footprints[li] = fp.clone();
                        }
                    }
                }
                JunctionRole::Through(_) => {
                    if sj.kind == SolvedJoinKind::X && sj.junction.participants.len() == 2 {
                        if let Some((pri, cross)) = x_priority_pair(walls, &sj.junction) {
                            if cross == wi {
                                is_x_crossing_stem = true;
                                for (li, stem_fp) in fps.iter().enumerate() {
                                    if let Some(sfp) = stem_fp {
                                        if li < x_crossing_stem_footprints.len() {
                                            x_crossing_stem_footprints[li] = Some(match &x_crossing_stem_footprints[li] {
                                                Some(prev) => merge_crossing_stems_local(Some(prev.as_slice()), sfp),
                                                None => sfp.clone(),
                                            });
                                        }
                                    }
                                }
                            } else if pri == wi {
                                if walls[wi].axis.len() > 2 {
                                    // Full-axis pair footprints cannot be
                                    // half-plane-merged across a second
                                    // crossing; keep only the intersected
                                    // segment's notch chords.
                                    span_uses_local_x_cutouts = true;
                                    for cut in x_priority_local_cutouts(
                                        walls,
                                        &sj.junction,
                                        pri,
                                        cross,
                                    ) {
                                        span_cutouts.push(cut);
                                    }
                                } else {
                                    span_cutouts.push(fps.clone());
                                }
                            }
                        } else {
                            span_cutouts.push(fps.clone());
                        }
                    } else {
                        span_cutouts.push(fps.clone());
                    }
                }
            }
        }

        let mut combined: Vec<Option<Vec<(f64, f64)>>> = vec![None; w.layers.len()];
        for li in 0..w.layers.len() {
            let base_fp = base_footprints.get(li);
            if is_x_crossing_stem && x_crossing_stem_footprints.get(li).and_then(|o| o.as_ref()).is_some() {
                let stem_fp = x_crossing_stem_footprints[li].clone();
                let e0 = end0_footprints.get(li).and_then(|o| o.as_ref());
                let e1 = end1_footprints.get(li).and_then(|o| o.as_ref());
                if let (Some(base), Some(stem)) = (base_fp, &stem_fp) {
                    let end_merged = merge_end_footprints(base, e0, e1);
                    if let Some(em) = end_merged {
                        combined[li] = Some(merge_through_cutout_preserving_miter(Some(&em), stem));
                    } else {
                        combined[li] = Some(stem.clone());
                    }
                } else {
                    combined[li] = stem_fp;
                }
            } else {
                let e0 = end0_footprints.get(li).and_then(|o| o.as_ref());
                let e1 = end1_footprints.get(li).and_then(|o| o.as_ref());
                combined[li] = if let Some(base) = base_fp {
                    merge_end_footprints(base, e0, e1)
                } else {
                    e0.cloned().or_else(|| e1.cloned())
                };
            }
        }

        for cutouts in &span_cutouts {
            for li in 0..w.layers.len() {
                if let Some(Some(cut_fp)) = cutouts.get(li) {
                    let cur = combined[li].as_deref().or_else(|| base_footprints.get(li).map(|v| v.as_slice()));
                    if let Some(c) = cur {
                        combined[li] = Some(if span_uses_local_x_cutouts {
                            merge_crossing_stems_local(Some(c), cut_fp)
                        } else {
                            merge_through_cutout_preserving_miter(Some(c), cut_fp)
                        });
                    } else {
                        combined[li] = Some(cut_fp.clone());
                    }
                }
            }
        }

        all_wall_footprints.push(combined);
    }

    SolvedWallNetwork {
        trimmed_axes,
        footprints: all_wall_footprints,
        junctions: final_solved_junctions,
    }
}

/// Detect and classify junctions among `walls`.
///
/// `tol` is the endpoint clustering radius (defaults to [`JUNCTION_TOLERANCE`]
/// when non-positive). End vs Mid uses [`END_MID_TOLERANCE`] inside
/// [`detect_junctions`].
pub fn solve(walls: &[WallJoinInput], tol: f64) -> Vec<SolvedJunction> {
    let cluster_tol = if tol > 0.0 { tol } else { JUNCTION_TOLERANCE };
    let axes: Vec<&[DVec3]> = walls.iter().map(|w| w.axis.as_slice()).collect();
    let bulges: Vec<&[f64]> = walls.iter().map(|w| w.bulges.as_slice()).collect();
    let detected = detect_junctions_with_bulges(&axes, &bulges, cluster_tol);

    let mut out = Vec::with_capacity(detected.len());
    for junction in detected {
        let kind = classify_kind(walls, &junction);
        let _rays = junction_rays(&axes, &junction);
        let trimmed_axes = trim_axes_for_kind(walls, &junction, kind);
        let footprints = compute_footprints(walls, &junction, kind, &trimmed_axes);
        out.push(SolvedJunction {
            junction,
            kind,
            trimmed_axes,
            footprints,
        });
    }
    out
}

/// Recompute layer footprints after a snap-point override of [`Junction::point`].
pub fn footprints_for(
    walls: &[WallJoinInput],
    junction: &Junction,
    kind: SolvedJoinKind,
    trimmed_axes: &[Vec<DVec3>],
) -> Vec<Vec<Option<Vec<(f64, f64)>>>> {
    compute_footprints(walls, junction, kind, trimmed_axes)
}

/// Two-wall join used by document L/T (`force_l` = `AEC_WALLJOIN`).
///
/// Uses [`join_wall_axes_as_l_with_bulges`] / [`join_wall_axes_with_bulges`]
/// so walls that do not yet cluster (need extension) still meet. Classification
/// of already-clustered T vs L still honours [`END_MID_TOLERANCE`] inside
/// those helpers.
pub fn solve_pair(
    a: &WallJoinInput,
    b: &WallJoinInput,
    force_l: bool,
) -> Result<SolvedPair, JoinError> {
    let (axis_a, axis_b, kind, end_a, end_b) = if force_l {
        join_wall_axes_as_l_with_bulges(&a.axis, &a.bulges, &b.axis, &b.bulges)?
    } else {
        join_wall_axes_with_bulges(&a.axis, &a.bulges, &b.axis, &b.bulges)?
    };
    let (footprints_a, footprints_b) =
        pair_footprints(a, b, &axis_a, &axis_b, kind, end_a, end_b);
    Ok(SolvedPair {
        axis_a,
        axis_b,
        kind,
        end_a,
        end_b,
        footprints_a,
        footprints_b,
    })
}

fn is_opposed_stems(walls: &[WallJoinInput], junction: &Junction) -> bool {
    let through_p = junction
        .participants
        .iter()
        .find(|p| matches!(p.role, JunctionRole::Through(_)));
    let endpoint_ps: Vec<_> = junction
        .participants
        .iter()
        .filter(|p| matches!(p.role, JunctionRole::Endpoint(_)))
        .collect();
    if through_p.is_none() || endpoint_ps.len() != 2 {
        return false;
    }
    let tp = through_p.unwrap();
    let Some(through_wall) = walls.get(tp.wall_index) else {
        return false;
    };
    let JunctionRole::Through(seg_idx) = tp.role else {
        return false;
    };
    if seg_idx + 1 >= through_wall.axis.len() {
        return false;
    }
    let p_start = through_wall.axis[seg_idx];
    let p_end = through_wall.axis[seg_idx + 1];
    let td = DVec3::new(p_end.x - p_start.x, p_end.y - p_start.y, 0.0);
    if td.length_squared() < 1e-12 {
        return false;
    }
    let normal = DVec3::new(-td.y, td.x, 0.0).normalize();

    let mut side_signs = Vec::new();
    for ep in endpoint_ps {
        let Some(stem_wall) = walls.get(ep.wall_index) else {
            return false;
        };
        let JunctionRole::Endpoint(end_idx) = ep.role else {
            return false;
        };
        if stem_wall.axis.len() < 2 {
            return false;
        }
        let (p_junc, p_inward) = if end_idx == 0 {
            (stem_wall.axis[0], stem_wall.axis[1])
        } else {
            (
                stem_wall.axis[stem_wall.axis.len() - 1],
                stem_wall.axis[stem_wall.axis.len() - 2],
            )
        };
        let stem_dir = DVec3::new(p_inward.x - p_junc.x, p_inward.y - p_junc.y, 0.0);
        let dot = stem_dir.dot(normal);
        side_signs.push(dot);
    }
    if side_signs.len() == 2 {
        side_signs[0] * side_signs[1] < -1e-6
    } else {
        false
    }
}

fn classify_kind(walls: &[WallJoinInput], junction: &Junction) -> SolvedJoinKind {
    let through_count = junction
        .participants
        .iter()
        .filter(|p| matches!(p.role, JunctionRole::Through(_)))
        .count();
    let endpoint_count = junction.endpoint_count();

    if junction.participants.len() == 2 {
        if through_count == 2 {
            SolvedJoinKind::X
        } else if through_count == 1 {
            SolvedJoinKind::T
        } else {
            SolvedJoinKind::L
        }
    } else if through_count == 1 && endpoint_count == 2 && junction.participants.len() == 3 {
        if is_opposed_stems(walls, junction) {
            SolvedJoinKind::X
        } else {
            SolvedJoinKind::NWay
        }
    } else {
        SolvedJoinKind::NWay
    }
}

fn trim_axes_for_kind(
    walls: &[WallJoinInput],
    junction: &Junction,
    kind: SolvedJoinKind,
) -> Vec<Vec<DVec3>> {
    let axes: Vec<&[DVec3]> = walls.iter().map(|w| w.axis.as_slice()).collect();
    match kind {
        SolvedJoinKind::L if junction.participants.len() == 2 => {
            let i0 = junction.participants[0].wall_index;
            let i1 = junction.participants[1].wall_index;
            let a = &walls[i0];
            let b = &walls[i1];
            match join_wall_axes_as_l_with_bulges(&a.axis, &a.bulges, &b.axis, &b.bulges) {
                Ok((new_a, new_b, _, _, _)) => {
                    let mut out: Vec<Vec<DVec3>> = walls.iter().map(|w| w.axis.clone()).collect();
                    out[i0] = new_a;
                    out[i1] = new_b;
                    out
                }
                Err(_) => apply_junction_to_axes(&axes, junction),
            }
        }
        SolvedJoinKind::L | SolvedJoinKind::T | SolvedJoinKind::X | SolvedJoinKind::NWay => {
            apply_junction_to_axes(&axes, junction)
        }
    }
}

pub(crate) fn wall_through_priority(w: &WallJoinInput) -> (f64, f64) {
    let core_thick = w
        .layers
        .iter()
        .filter(|l| is_structural_function(&l.function))
        .map(|l| l.thickness)
        .sum::<f64>();
    let total_thick = w.layers.iter().map(|l| l.thickness).sum::<f64>();
    (core_thick, total_thick)
}

fn split_axis_and_bulges_at_point(
    axis: &[DVec3],
    bulges: &[f64],
    jp: DVec3,
    known_seg: Option<usize>,
) -> ((Vec<DVec3>, Vec<f64>), (Vec<DVec3>, Vec<f64>)) {
    if axis.len() < 2 {
        return (
            (vec![jp, jp], vec![0.0]),
            (vec![jp, jp], vec![0.0]),
        );
    }
    let best_seg = if let Some(seg) = known_seg {
        if seg < axis.len() - 1 {
            seg
        } else {
            find_split_seg(axis, bulges, jp)
        }
    } else {
        find_split_seg(axis, bulges, jp)
    };

    let old_b = bulges.get(best_seg).copied().unwrap_or(0.0);
    let p_seg_start = (axis[best_seg].x, axis[best_seg].y);
    let p_seg_end = (axis[best_seg + 1].x, axis[best_seg + 1].y);
    let p_jp = (jp.x, jp.y);

    let b_sub1 = if old_b.abs() > 1e-6 {
        super::arc::retarget_bulge(p_seg_start, p_seg_end, old_b, p_seg_start, p_jp)
    } else {
        0.0
    };
    let b_sub2 = if old_b.abs() > 1e-6 {
        super::arc::retarget_bulge(p_seg_start, p_seg_end, old_b, p_jp, p_seg_end)
    } else {
        0.0
    };

    let mut stem1 = Vec::new();
    let mut stem1_bulges = Vec::new();
    for i in 0..=best_seg {
        stem1.push(axis[i]);
        if i < best_seg {
            stem1_bulges.push(bulges.get(i).copied().unwrap_or(0.0));
        }
    }
    if stem1.last().map(|p| p.distance(jp) > 1e-6).unwrap_or(true) {
        stem1.push(jp);
        stem1_bulges.push(b_sub1);
    }
    if stem1.len() < 2 {
        stem1 = vec![axis[0], jp];
        stem1_bulges = vec![0.0];
    }

    let mut stem2 = Vec::new();
    let mut stem2_bulges = Vec::new();
    stem2.push(jp);
    stem2_bulges.push(b_sub2);
    for i in (best_seg + 1)..axis.len() {
        if axis[i].distance(jp) > 1e-6 {
            stem2.push(axis[i]);
            if i < axis.len() - 1 {
                stem2_bulges.push(bulges.get(i).copied().unwrap_or(0.0));
            }
        }
    }
    if stem2.len() < 2 {
        stem2 = vec![jp, *axis.last().unwrap()];
        stem2_bulges = vec![0.0];
    }

    ((stem1, stem1_bulges), (stem2, stem2_bulges))
}

fn find_split_seg(axis: &[DVec3], bulges: &[f64], jp: DVec3) -> usize {
    let mut best_seg = 0;
    let mut best_dist = f64::INFINITY;
    for i in 0..axis.len() - 1 {
        let a = axis[i];
        let b = axis[i + 1];
        let bulge = bulges.get(i).copied().unwrap_or(0.0);
        let d = if bulge.abs() > 1e-6 {
            if let Some(arc) = super::arc::bulge_to_arc((a.x, a.y), (b.x, b.y), bulge) {
                if super::arc::point_on_segment_interior((a.x, a.y), (b.x, b.y), (jp.x, jp.y), bulge, 1e-3) {
                    ((jp.x - arc.center.0).hypot(jp.y - arc.center.1) - arc.radius).abs()
                } else {
                    f64::INFINITY
                }
            } else {
                f64::INFINITY
            }
        } else {
            let abx = b.x - a.x;
            let aby = b.y - a.y;
            let len2 = abx * abx + aby * aby;
            if len2 < 1e-24 {
                continue;
            }
            let t = (((jp.x - a.x) * abx + (jp.y - a.y) * aby) / len2).clamp(0.0, 1.0);
            let proj = DVec3::new(a.x + abx * t, a.y + aby * t, a.z);
            proj.distance(jp)
        };
        if d < best_dist {
            best_dist = d;
            best_seg = i;
        }
    }
    best_seg
}

fn axis_2d(axis: &[DVec3]) -> Vec<(f64, f64)> {
    axis.iter().map(|p| (p.x, p.y)).collect()
}

fn layer_refs(layers: &[MiterLayer]) -> Vec<LayerRef> {
    layers
        .iter()
        .enumerate()
        .map(|(i, l)| LayerRef {
            material_id: l.material.clone(),
            role_tag: None,
            index: i,
            layer_id: Some(l.layer_id),
        })
        .collect()
}

fn pair_footprints(
    a: &WallJoinInput,
    b: &WallJoinInput,
    axis_a: &[DVec3],
    axis_b: &[DVec3],
    kind: JoinKind,
    end_a: Option<usize>,
    end_b: Option<usize>,
) -> (
    Vec<Option<Vec<(f64, f64)>>>,
    Vec<Option<Vec<(f64, f64)>>>,
) {
    let a2 = axis_2d(axis_a);
    let b2 = axis_2d(axis_b);
    let refs_a = layer_refs(&a.layers);
    let refs_b = layer_refs(&b.layers);
    let ov_a_raw = end_a.and_then(|e| a.override_at(e));
    let ov_b_raw = end_b.and_then(|e| b.override_at(e));
    let ov_a_merged = merge_partner_junction_override(ov_a_raw, ov_b_raw);
    let ov_b_merged = merge_partner_junction_override(ov_b_raw, ov_a_raw);
    let ov_a = ov_a_merged.as_ref();
    let ov_b = ov_b_merged.as_ref();

    let gaps_a = {
        let mut g = Vec::new();
        if let Some(ov) = a.override_span.as_ref() {
            g.extend(ov.layer_gaps.iter().cloned());
        }
        if let Some(ov) = ov_b_raw {
            g.extend(ov.layer_gaps.iter().cloned());
        }
        g
    };
    let gaps_b = {
        let mut g = Vec::new();
        if let Some(ov) = b.override_span.as_ref() {
            g.extend(ov.layer_gaps.iter().cloned());
        }
        if let Some(ov) = ov_a_raw {
            g.extend(ov.layer_gaps.iter().cloned());
        }
        g
    };

    let fps_a = if let Some(ea) = end_a {
        mitered_layer_footprints_with_override_and_bulges(
            &a2,
            &a.layers,
            &refs_a,
            ea,
            &b2,
            &b.layers,
            end_b,
            kind,
            ov_a,
            &a.bulges,
            &b.bulges,
        )
    } else if let Some(eb) = end_b {
        through_wall_cutout_footprints_with_gaps(
            &a2,
            &a.layers,
            &b2,
            &b.layers,
            eb,
            &a.bulges,
            &b.bulges,
            &gaps_a,
        )
    } else {
        vec![None; a.layers.len()]
    };

    let fps_b = if let Some(eb) = end_b {
        mitered_layer_footprints_with_override_and_bulges(
            &b2,
            &b.layers,
            &refs_b,
            eb,
            &a2,
            &a.layers,
            end_a,
            kind,
            ov_b,
            &b.bulges,
            &a.bulges,
        )
    } else if let Some(ea) = end_a {
        through_wall_cutout_footprints_with_gaps(
            &b2,
            &b.layers,
            &a2,
            &a.layers,
            ea,
            &b.bulges,
            &a.bulges,
            &gaps_b,
        )
    } else {
        vec![None; b.layers.len()]
    };

    (fps_a, fps_b)
}

/// `(priority wall index, crossing wall index)` of a 2-wall X crossing.
///
/// The priority (through) wall keeps its core; the other wall is split into
/// two T stems. Walls containing any curved segment cannot yet be represented
/// without losing bulges in the polygon footprint.
pub(crate) fn x_priority_pair(
    walls: &[WallJoinInput],
    junction: &Junction,
) -> Option<(usize, usize)> {
    if junction.participants.len() != 2 {
        return None;
    }
    let (p0, p1) = (&junction.participants[0], &junction.participants[1]);
    let (i0, i1) = (p0.wall_index, p1.wall_index);
    if i0 >= walls.len() || i1 >= walls.len() {
        return None;
    }
    // The X footprint representation currently carries polygon vertices but
    // not arc bulges. Even when the straight wall wins through priority, a
    // curved stem (or an unrelated curved leg on the priority wall) would
    // be flattened to chord strips.
    if [i0, i1].into_iter().any(|i| walls[i].bulges.iter().any(|b| b.abs() > 1e-12)) {
        return None;
    }
    if wall_through_priority(&walls[i0]) >= wall_through_priority(&walls[i1]) {
        Some((i0, i1))
    } else {
        Some((i1, i0))
    }
}

/// `true` when an X junction cannot be resolved without distorting a wall.
pub(crate) fn x_junction_is_unsupported(walls: &[WallJoinInput], junction: &Junction) -> bool {
    junction.participants.len() == 2
        && junction
            .participants
            .iter()
            .all(|p| matches!(p.role, JunctionRole::Through(_)))
        && junction.participants.iter().any(|p| {
            walls.get(p.wall_index).is_some_and(|wall| wall.bulges.iter().any(|b| b.abs() > 1e-12))
        })
}

/// Local, opposite-side finish cutouts on the intersected *straight segment*.
/// Taking the entire polyline's first and last contour points here would
/// construct a diagonal notch across unrelated legs.
pub(crate) fn x_priority_local_cutouts(
    walls: &[WallJoinInput],
    junction: &Junction,
    pri_wi: usize,
    cross_wi: usize,
) -> Vec<Vec<Option<Vec<(f64, f64)>>>> {
    let priority = &walls[pri_wi];
    let crossing = &walls[cross_wi];
    let Some(seg) = junction.participants.iter().find_map(|p| {
        (p.wall_index == pri_wi).then_some(p.role).and_then(|r| match r {
            JunctionRole::Through(i) => Some(i),
            _ => None,
        })
    }) else { return Vec::new() };
    let Some(end) = priority.axis.get(seg + 1) else { return Vec::new() };
    let local_axis = axis_2d(&[priority.axis[seg], *end]);
    let known_seg = junction.participants.iter().find_map(|p| {
        (p.wall_index == cross_wi).then_some(p.role).and_then(|r| match r {
            JunctionRole::Through(i) => Some(i),
            _ => None,
        })
    });
    let ((s1, b1), (s2, b2)) = split_axis_and_bulges_at_point(
        &crossing.axis, &crossing.bulges, junction.point, known_seg,
    );
    let mut gaps = Vec::new();
    if let Some(ov) = &priority.override_span { gaps.extend(ov.layer_gaps.iter().cloned()); }
    if let Some(ov) = &crossing.override_span { gaps.extend(ov.layer_gaps.iter().cloned()); }
    [(s1, b1, true), (s2, b2, false)].into_iter().map(|(axis, bulges, at_end)| {
        through_wall_cutout_footprints_with_gaps(
            &local_axis, &priority.layers, &axis_2d(&axis), &crossing.layers,
            if at_end { axis.len().saturating_sub(1) } else { 0 },
            &[], &bulges, &gaps,
        )
    }).collect()
}

type FaceLine = ((f64, f64), (f64, f64));

/// Per-layer result of one logical X stem butting the priority wall.
struct XStem {
    layers: Vec<Option<Vec<(f64, f64)>>>,
    /// The stem clears the near face of at least one priority layer.
    valid: bool,
}

fn line_distance(line: FaceLine, p: (f64, f64)) -> Option<f64> {
    let (dx, dy) = (line.1 .0 - line.0 .0, line.1 .1 - line.0 .1);
    let len = dx.hypot(dy);
    if len < 1e-9 {
        return None;
    }
    Some((p.0 - line.0 .0) * (-dy / len) + (p.1 - line.0 .1) * (dx / len))
}

/// The stem's far point is strictly beyond `target` (away from `jp`).
fn x_target_reached(target: FaceLine, far_pt: (f64, f64), jp: DVec3) -> bool {
    let (Some(d_jp), Some(d_far)) = (
        line_distance(target, (jp.x, jp.y)),
        line_distance(target, far_pt),
    ) else {
        return false;
    };
    if d_jp.abs() > 1e-6 {
        d_far * d_jp.signum() < -1e-4
    } else {
        let to_stem = (far_pt.0 - jp.x, far_pt.1 - jp.y);
        let line_dir = (target.1 .0 - target.0 .0, target.1 .1 - target.0 .1);
        let len = line_dir.0.hypot(line_dir.1);
        let n = (-line_dir.1 / len, line_dir.0 / len);
        let dot = to_stem.0 * n.0 + to_stem.1 * n.1;
        dot.abs() > 1e-6 && d_far * dot.signum() > 1e-4
    }
}

/// Auto-target check: the ring stays on the stem's side of `target`
/// (no vertex penetrates back towards the junction point).
fn x_ring_valid_auto(fp: &[(f64, f64)], target: FaceLine, far_pt: (f64, f64), jp: DVec3) -> bool {
    let (Some(d_jp), Some(d_far)) = (
        line_distance(target, (jp.x, jp.y)),
        line_distance(target, far_pt),
    ) else {
        return false;
    };
    let finite = || fp.iter().filter(|v| v.0.is_finite() && v.1.is_finite());
    if d_jp.abs() > 1e-6 {
        let s = d_jp.signum();
        if d_far * s >= -1e-4 {
            return false;
        }
        !finite().any(|&v| line_distance(target, v).is_none_or(|d| d * s > 1e-4))
    } else {
        let line_dir = (target.1 .0 - target.0 .0, target.1 .1 - target.0 .1);
        let len = line_dir.0.hypot(line_dir.1);
        let n = (-line_dir.1 / len, line_dir.0 / len);
        let dot = (far_pt.0 - jp.x) * n.0 + (far_pt.1 - jp.y) * n.1;
        if dot.abs() > 1e-6 {
            let s = dot.signum();
            if d_far * s <= 1e-4 {
                return false;
            }
            if finite().any(|&v| line_distance(target, v).is_none_or(|d| d * s < -1e-4)) {
                return false;
            }
        }
        true
    }
}

/// Override-target check: the ring stays on the stem's side of the explicit
/// face line (which may lie beyond the junction point for FarFace).
fn x_ring_on_stem_side(fp: &[(f64, f64)], target: FaceLine, far_pt: (f64, f64)) -> bool {
    let Some(d_far) = line_distance(target, far_pt) else {
        return false;
    };
    if d_far.abs() <= 1e-4 {
        return false;
    }
    let s = d_far.signum();
    !fp.iter()
        .filter(|v| v.0.is_finite() && v.1.is_finite())
        .any(|&v| line_distance(target, v).is_none_or(|d| d * s < -1e-4))
}

/// Resolve one logical stem (`stem_at_start`: the stem begins at the junction
/// point, otherwise it ends there) of the crossing wall against the priority
/// wall.
///
/// Validation looks at the *first segment next to the junction only*, so a
/// polyline stem that bends back across the priority wall elsewhere (U-shaped
/// crossing wall) is not rejected for geometry that belongs to another
/// crossing. The footprint of the whole stem is still returned.
///
/// The crossing wall's manual override (`override_span`) decides the face a
/// layer is validated / clipped against: `NearFace`/`FarFace`/`Butt` honour
/// their own face line, `NoExtend` is accepted as-is.
#[allow(clippy::too_many_arguments)]
fn solve_x_stem(
    pri_wall: &WallJoinInput,
    pri_axis: &[DVec3],
    pri_contours: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    cross_wall: &WallJoinInput,
    stem_axis: &[DVec3],
    stem_bulges: &[f64],
    stem_at_start: bool,
    jp: DVec3,
) -> XStem {
    use super::miter::StemOverrideTarget;

    let nl = cross_wall.layers.len();
    let mut result = XStem {
        layers: vec![None; nl],
        valid: false,
    };
    if stem_axis.len() < 2 {
        return result;
    }
    let stem_end = if stem_at_start { 0 } else { stem_axis.len() - 1 };
    let pri_axis_2d = axis_2d(pri_axis);
    let ov = cross_wall.override_span.clone();
    let make_input = |axis: Vec<DVec3>, bulges: Vec<f64>| WallJoinInput {
        axis,
        bulges,
        layers: cross_wall.layers.clone(),
        override_start: if stem_at_start { ov.clone() } else { None },
        override_end: if stem_at_start { None } else { ov.clone() },
        override_span: None,
    };
    let fps_full = pair_footprints(
        pri_wall,
        &make_input(stem_axis.to_vec(), stem_bulges.to_vec()),
        pri_axis,
        stem_axis,
        JoinKind::T,
        None,
        Some(stem_end),
    )
    .1;

    let n = stem_axis.len();
    let is_simple = n == 2;
    let (local_axis, local_bulges, local_end): (Vec<DVec3>, Vec<f64>, usize) = if is_simple {
        (stem_axis.to_vec(), stem_bulges.to_vec(), stem_end)
    } else if stem_at_start {
        (
            stem_axis[..2].to_vec(),
            vec![stem_bulges.first().copied().unwrap_or(0.0)],
            0,
        )
    } else {
        (
            stem_axis[n - 2..].to_vec(),
            vec![stem_bulges.get(n - 2).copied().unwrap_or(0.0)],
            1,
        )
    };
    let fps_local = if is_simple {
        fps_full.clone()
    } else {
        pair_footprints(
            pri_wall,
            &make_input(local_axis.clone(), local_bulges.clone()),
            pri_axis,
            &local_axis,
            JoinKind::T,
            None,
            Some(local_end),
        )
        .1
    };

    let local_axis_2d = axis_2d(&local_axis);
    let offsets: Vec<(f64, f64)> = cross_wall
        .layers
        .iter()
        .map(|l| (l.thickness, l.axis_offset))
        .collect();
    let local_raw =
        super::contour::layer_contours_with_bulges(&local_axis_2d, &local_bulges, &offsets);
    let local_contours: Vec<(Vec<(f64, f64)>, Vec<(f64, f64)>)> = local_raw
        .iter()
        .map(|(a, b)| (a.points.clone(), b.points.clone()))
        .collect();
    let outer_target = super::miter::through_outer_near_face_line(
        &local_axis_2d,
        local_end,
        &pri_axis_2d,
        &pri_wall.layers,
        pri_contours,
    );
    let core_pair = super::miter::pair_structural_cores(&cross_wall.layers, &pri_wall.layers);
    let face_of = |li: usize, j: usize| {
        let (ref a_b1, ref a_b2) = local_contours[li];
        let (ref b_b1, ref b_b2) = pri_contours[j];
        super::miter::near_face_line(
            a_b1,
            a_b2,
            local_end,
            b_b1,
            b_b2,
            None,
            &local_axis_2d,
            &pri_axis_2d,
        )
        .or(outer_target)
    };
    let auto_target = |li: usize| -> Option<FaceLine> {
        if li >= nl || li >= local_contours.len() {
            return outer_target;
        }
        let is_structural = super::miter::is_structural_function(&cross_wall.layers[li].function)
            || core_pair.is_some_and(|(core_a, _)| li == core_a);
        if is_structural {
            match core_pair {
                Some((_, core_b)) if core_b < pri_contours.len() => face_of(li, core_b),
                _ => outer_target,
            }
        } else if let Some(j) = super::miter::approach_finish_match(
            li,
            &cross_wall.layers,
            &pri_wall.layers,
            core_pair,
            &local_axis_2d,
            local_end,
            &pri_axis_2d,
        ) {
            if j < pri_contours.len() {
                face_of(li, j)
            } else {
                outer_target
            }
        } else {
            outer_target
        }
    };
    let refs = layer_refs(&cross_wall.layers);
    let override_of = |li: usize| -> StemOverrideTarget {
        match (&ov, refs.get(li)) {
            (Some(o), Some(r)) => super::miter::t_stem_override_target(
                li,
                r,
                o,
                &local_axis_2d,
                &cross_wall.layers,
                local_end,
                &pri_axis_2d,
                &pri_wall.layers,
                &local_contours,
                pri_contours,
                outer_target,
            ),
            _ => StemOverrideTarget::Auto,
        }
    };

    let leaving = if local_end == 0 {
        local_axis_2d[1]
    } else {
        local_axis_2d[0]
    };
    let autos: Vec<Option<FaceLine>> = (0..nl).map(auto_target).collect();
    result.valid = autos
        .iter()
        .flatten()
        .any(|&t| x_target_reached(t, leaving, jp));

    let base_ring = |li: usize| -> Option<Vec<(f64, f64)>> {
        let (b1, b2) = local_raw.get(li)?;
        Some(super::contour::closed_layer_footprint(b1, b2).points)
    };
    let full_base_ring = |li: usize| -> Option<Vec<(f64, f64)>> {
        let stem_axis_2d = axis_2d(stem_axis);
        let raw = super::contour::layer_contours_with_bulges(&stem_axis_2d, stem_bulges, &offsets);
        let (b1, b2) = raw.get(li)?;
        Some(super::contour::closed_layer_footprint(b1, b2).points)
    };
    for li in 0..nl {
        let auto = autos[li];
        let cand = fps_local.get(li).and_then(|x| x.as_ref());
        let ovt = override_of(li);
        let line_override = match ovt {
            StemOverrideTarget::Line(l) => Some(l),
            _ => None,
        };
        let ok = match ovt {
            StemOverrideTarget::NoExtend => cand.is_some(),
            StemOverrideTarget::Line(l) => {
                auto.is_some_and(|t| x_target_reached(t, leaving, jp))
                    && cand.is_some_and(|fp| x_ring_on_stem_side(fp, l, leaving))
            }
            _ => auto.is_some_and(|t| {
                cand.is_some_and(|fp| x_ring_valid_auto(fp, t, leaving, jp))
            }),
        };
        if ok {
            result.layers[li] = fps_full.get(li).cloned().flatten();
            continue;
        }
        // Capped fallback: cut the plain stem ring at the target face.
        if ovt == StemOverrideTarget::NoExtend {
            continue;
        }
        let Some(target) = line_override.or(auto) else {
            continue;
        };
        if !auto.is_some_and(|t| x_target_reached(t, leaving, jp)) {
            continue;
        }
        let Some(local_base) = base_ring(li) else {
            continue;
        };
        let clipped =
            super::miter::clip_closed_to_halfplane(&local_base, target.0, target.1, leaving);
        if is_simple {
            let valid = clipped.len() >= 3
                && if line_override.is_some() {
                    x_ring_on_stem_side(&clipped, target, leaving)
                } else {
                    x_ring_valid_auto(&clipped, target, leaving, jp)
                };
            if valid {
                result.layers[li] = Some(clipped);
            }
            continue;
        }
        // Polyline stem: an infinite half-plane clip could cut unrelated legs
        // that bend back across the face line. Cut the full ring along the
        // *finite* butt chord of the first segment instead and drop only the
        // piece on the junction side.
        if clipped.len() < 3 || !(line_override.is_none() || x_ring_on_stem_side(&clipped, target, leaving)) {
            continue;
        }
        let on_line: Vec<(f64, f64)> = clipped
            .iter()
            .copied()
            .filter(|&v| line_distance(target, v).is_some_and(|d| d.abs() < 1e-6))
            .collect();
        let dir = (target.1 .0 - target.0 .0, target.1 .1 - target.0 .1);
        let dl = dir.0.hypot(dir.1);
        if on_line.len() < 2 || dl < 1e-9 {
            continue;
        }
        let along = |v: (f64, f64)| ((v.0 - target.0 .0) * dir.0 + (v.1 - target.0 .1) * dir.1) / dl;
        let q_lo = on_line
            .iter()
            .copied()
            .min_by(|x, y| along(*x).partial_cmp(&along(*y)).unwrap_or(std::cmp::Ordering::Equal));
        let q_hi = on_line
            .iter()
            .copied()
            .max_by(|x, y| along(*x).partial_cmp(&along(*y)).unwrap_or(std::cmp::Ordering::Equal));
        let (Some(q0), Some(q1), Some(full_base)) = (q_lo, q_hi, full_base_ring(li)) else {
            continue;
        };
        let jp_side = line_distance(target, (jp.x, jp.y)).unwrap_or(0.0);
        let pieces: Vec<Vec<(f64, f64)>> = super::miter::split_ring_along_chord(&full_base, q0, q1)
            .into_iter()
            .filter(|piece| {
                // drop the junction-side piece (all vertices on jp's side)
                let on_jp_side = |v: &(f64, f64)| {
                    line_distance(target, *v).is_some_and(|d| {
                        if jp_side.abs() > 1e-6 {
                            d * jp_side.signum() >= -1e-4
                        } else {
                            false
                        }
                    })
                };
                !piece.iter().all(on_jp_side)
            })
            .collect();
        if pieces.len() == 1 {
            result.layers[li] = pieces.into_iter().next();
        }
    }
    result
}

fn compute_footprints(
    walls: &[WallJoinInput],
    junction: &Junction,
    kind: SolvedJoinKind,
    trimmed: &[Vec<DVec3>],
) -> Vec<Vec<Option<Vec<(f64, f64)>>>> {
    let mut out: Vec<Vec<Option<Vec<(f64, f64)>>>> = walls
        .iter()
        .map(|w| vec![None; w.layers.len()])
        .collect();
    if walls.is_empty() || junction.participants.is_empty() {
        return out;
    }

    if junction.participants.len() == 2 {
        let i0 = junction.participants[0].wall_index;
        let i1 = junction.participants[1].wall_index;
        if i0 >= walls.len() || i1 >= walls.len() {
            return out;
        }

        if kind == SolvedJoinKind::X {
            let Some((pri_wi, cross_wi)) = x_priority_pair(walls, junction) else {
                // Curved axis crossing: no safe footprint, leave both
                // walls untouched instead of flattening arcs to chords.
                return out;
            };
            let pri_wall = &walls[pri_wi];
            let cross_wall = &walls[cross_wi];
            let pri_axis = trimmed.get(pri_wi).map(|a| a.as_slice()).unwrap_or(&pri_wall.axis);
            let cross_axis = trimmed.get(cross_wi).map(|a| a.as_slice()).unwrap_or(&cross_wall.axis);
            let jp = junction.point;

            let known_seg = junction
                .participants
                .iter()
                .find(|p| p.wall_index == cross_wi)
                .and_then(|p| match p.role {
                    JunctionRole::Through(seg) => Some(seg),
                    _ => None,
                });
            let ((stem1_axis, stem1_bulges), (stem2_axis, stem2_bulges)) =
                split_axis_and_bulges_at_point(cross_axis, &cross_wall.bulges, jp, known_seg);
            let stem1_end = stem1_axis.len().saturating_sub(1);
            let stem2_end = 0;

            let pri_axis_2d = axis_2d(pri_axis);
            let pri_seg = junction.participants.iter().find_map(|p| {
                (p.wall_index == pri_wi).then_some(p.role).and_then(|r| match r {
                    JunctionRole::Through(seg) => Some(seg),
                    _ => None,
                })
            });
            let local_pri_axis = pri_seg.and_then(|seg| pri_axis.get(seg..seg + 2))
                .unwrap_or(pri_axis);
            let local_pri_axis_2d = axis_2d(local_pri_axis);
            let stem1_axis_2d = axis_2d(&stem1_axis);
            let stem2_axis_2d = axis_2d(&stem2_axis);
            let pri_layer_offsets: Vec<(f64, f64)> = pri_wall
                .layers
                .iter()
                .map(|l| (l.thickness, l.axis_offset))
                .collect();
            let pri_contours: Vec<(Vec<(f64, f64)>, Vec<(f64, f64)>)> =
                super::contour::layer_contours_with_bulges(
                    &local_pri_axis_2d,
                    &[],
                    &pri_layer_offsets,
                )
                .into_iter()
                .map(|(a, b)| (a.points, b.points))
                .collect();

            let stem1 = solve_x_stem(
                pri_wall,
                local_pri_axis,
                &pri_contours,
                cross_wall,
                &stem1_axis,
                &stem1_bulges,
                false,
                jp,
            );
            let stem2 = solve_x_stem(
                pri_wall,
                local_pri_axis,
                &pri_contours,
                cross_wall,
                &stem2_axis,
                &stem2_bulges,
                true,
                jp,
            );
            let (stem1_valid, stem2_valid) = (stem1.valid, stem2.valid);

            for li in 0..cross_wall.layers.len() {
                let f1 = stem1.layers.get(li).and_then(|x| x.as_ref());
                let f2 = stem2.layers.get(li).and_then(|x| x.as_ref());
                out[cross_wi][li] = match (f1, f2) {
                    (Some(r1), Some(r2)) => {
                        let mut c = r1.clone();
                        c.push((f64::NAN, f64::NAN));
                        c.extend(r2.iter().cloned());
                        Some(c)
                    }
                    (Some(r), None) | (None, Some(r)) => Some(r.clone()),
                    (None, None) => None,
                };
            }

            // Cutouts on priority through wall from both stems
            let mut gaps_1 = Vec::new();
            if let Some(ov) = pri_wall.override_span.as_ref() {
                gaps_1.extend(ov.layer_gaps.iter().cloned());
            }
            let mut gaps_2 = gaps_1.clone();
            if let Some(ov) = cross_wall.override_span.as_ref() {
                gaps_1.extend(ov.layer_gaps.iter().cloned());
                gaps_2.extend(ov.layer_gaps.iter().cloned());
            }

            let cutouts_stem1 = if stem1_valid {
                through_wall_cutout_footprints_with_gaps(
                    &pri_axis_2d,
                    &pri_wall.layers,
                    &stem1_axis_2d,
                    &cross_wall.layers,
                    stem1_end,
                    &pri_wall.bulges,
                    &stem1_bulges,
                    &gaps_1,
                )
            } else {
                vec![None; pri_wall.layers.len()]
            };
            let cutouts_stem2 = if stem2_valid {
                through_wall_cutout_footprints_with_gaps(
                    &pri_axis_2d,
                    &pri_wall.layers,
                    &stem2_axis_2d,
                    &cross_wall.layers,
                    stem2_end,
                    &pri_wall.bulges,
                    &stem2_bulges,
                    &gaps_2,
                )
            } else {
                vec![None; pri_wall.layers.len()]
            };

            if pri_axis.len() > 2 {
                let cuts = x_priority_local_cutouts(walls, junction, pri_wi, cross_wi);
                let offsets: Vec<_> = pri_wall.layers.iter().map(|l| (l.thickness, l.axis_offset)).collect();
                let raw = super::contour::layer_contours_with_bulges(&pri_axis_2d, &pri_wall.bulges, &offsets);
                for li in 0..pri_wall.layers.len() {
                    let Some((b1, b2)) = raw.get(li) else { continue };
                    let mut fp = None;
                    for (si, cut) in cuts.iter().enumerate() {
                        if (si == 0 && !stem1_valid) || (si == 1 && !stem2_valid) { continue; }
                        if let Some(Some(cut_fp)) = cut.get(li) {
                            let base = super::contour::closed_layer_footprint(b1, b2).points;
                            fp = Some(super::miter::merge_crossing_stems_local(
                                fp.as_deref().or(Some(base.as_slice())), cut_fp,
                            ));
                        }
                    }
                    out[pri_wi][li] = fp;
                }
                return out;
            }

            for li in 0..pri_wall.layers.len() {
                let c1 = cutouts_stem1.get(li).and_then(|x| x.as_ref());
                let c2 = cutouts_stem2.get(li).and_then(|x| x.as_ref());
                let merged = match (c1, c2) {
                    (Some(r1), Some(r2)) => {
                        Some(super::miter::merge_through_cutout_preserving_miter(Some(r1), r2))
                    }
                    (Some(r1), None) => Some(r1.clone()),
                    (None, Some(r2)) => Some(r2.clone()),
                    (None, None) => None,
                };
                out[pri_wi][li] = merged;
            }

            return out;
        }

        let join_kind = match kind {
            SolvedJoinKind::T => JoinKind::T,
            SolvedJoinKind::X => JoinKind::X,
            _ => JoinKind::L,
        };
        let end = |role: JunctionRole| match role {
            JunctionRole::Endpoint(e) => Some(e),
            JunctionRole::Through(_) => None,
        };
        let (fa, fb) = pair_footprints(
            &walls[i0],
            &walls[i1],
            trimmed.get(i0).unwrap_or(&walls[i0].axis),
            trimmed.get(i1).unwrap_or(&walls[i1].axis),
            join_kind,
            end(junction.participants[0].role),
            end(junction.participants[1].role),
        );
        out[i0] = fa;
        out[i1] = fb;
        return out;
    }

    let axes_2d: Vec<Vec<(f64, f64)>> = trimmed.iter().map(|a| axis_2d(a)).collect();
    let layers: Vec<Vec<MiterLayer>> = walls.iter().map(|w| w.layers.clone()).collect();
    let geoms: Vec<JunctionWallGeom> = junction_wall_geoms(junction, &axes_2d, &layers);
    let refs: Vec<Vec<LayerRef>> = junction
        .participants
        .iter()
        .map(|p| {
            walls
                .get(p.wall_index)
                .map(|w| layer_refs(&w.layers))
                .unwrap_or_default()
        })
        .collect();
    let overrides: Vec<Option<JunctionOverride>> = junction
        .participants
        .iter()
        .map(|p| match p.role {
            JunctionRole::Endpoint(e) => walls
                .get(p.wall_index)
                .and_then(|w| w.override_at(e))
                .cloned(),
            JunctionRole::Through(_) => None,
        })
        .collect();
    let all_fps =
        mitered_junction_layer_footprints_with_overrides(junction, &geoms, &refs, &overrides);

    for (pi, part) in junction.participants.iter().enumerate() {
        if part.wall_index >= out.len() {
            continue;
        }
        let mut fps = all_fps.get(pi).cloned().unwrap_or_default();
        if matches!(part.role, JunctionRole::Through(_)) {
            let through = &walls[part.wall_index];
            let t_axis = trimmed
                .get(part.wall_index)
                .map(|a| axis_2d(a))
                .unwrap_or_else(|| axis_2d(&through.axis));
            let mut accumulated_cutouts: Vec<Option<Vec<(f64, f64)>>> = vec![None; through.layers.len()];

            for stem in junction
                .participants
                .iter()
                .filter(|p| matches!(p.role, JunctionRole::Endpoint(_)))
            {
                if let JunctionRole::Endpoint(stem_end) = stem.role {
                    let stem_w = &walls[stem.wall_index];
                    let s_axis = trimmed
                        .get(stem.wall_index)
                        .map(|a| axis_2d(a))
                        .unwrap_or_else(|| axis_2d(&stem_w.axis));
                    let mut gaps = Vec::new();
                    if let Some(ov) = through.override_span.as_ref() {
                        gaps.extend(ov.layer_gaps.iter().cloned());
                    }
                    if let Some(ov) = stem_w.override_at(stem_end) {
                        gaps.extend(ov.layer_gaps.iter().cloned());
                    }
                    let stem_cutouts = through_wall_cutout_footprints_with_gaps(
                        &t_axis,
                        &through.layers,
                        &s_axis,
                        &stem_w.layers,
                        stem_end,
                        &through.bulges,
                        &stem_w.bulges,
                        &gaps,
                    );
                    for (li, stem_co) in stem_cutouts.into_iter().enumerate() {
                        if let Some(co) = stem_co {
                            if let Some(prev) = &accumulated_cutouts[li] {
                                accumulated_cutouts[li] = Some(super::miter::merge_through_cutout_preserving_miter(Some(prev.as_slice()), &co));
                            } else {
                                accumulated_cutouts[li] = Some(co);
                            }
                        }
                    }
                }
            }
            fps = accumulated_cutouts;
        }
        out[part.wall_index] = fps;
    }
    out
}

/// Re-export so callers can use the same End/Mid cutoff the solver relies on.
pub use super::join::END_MID_TOLERANCE as SOLVER_END_MID_TOLERANCE;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::join::{JoinOverrideStyle, JunctionRole};
    use crate::modules::aec::engine::miter;

    fn p(x: f64, y: f64) -> DVec3 {
        DVec3::new(x, y, 0.0)
    }

    fn layer(th: f64, off: f64, mat: &str) -> MiterLayer {
        MiterLayer::with_id(th, off, mat, "Structural", uuid::Uuid::new_v4())
    }

    fn wall_with_layers(axis: Vec<DVec3>, layers: Vec<MiterLayer>) -> WallJoinInput {
        WallJoinInput {
            axis,
            bulges: Vec::new(),
            layers,
            override_start: None,
            override_end: None,
            override_span: None,
        }
    }

    #[test]
    fn solve_l_corner_two_endpoints() {
        let walls = vec![
            WallJoinInput::from_axis(vec![p(0.0, 0.0), p(5.0, 0.0)]),
            WallJoinInput::from_axis(vec![p(5.0, 0.0), p(5.0, 5.0)]),
        ];
        let solved = solve(&walls, JUNCTION_TOLERANCE);
        assert_eq!(solved.len(), 1);
        let j = &solved[0];
        assert_eq!(j.kind, SolvedJoinKind::L);
        assert_eq!(j.junction.participants.len(), 2);
        assert!(j
            .junction
            .participants
            .iter()
            .all(|p| matches!(p.role, JunctionRole::Endpoint(_))));
        assert!(j.junction.point.distance(p(5.0, 0.0)) < 1e-9);
        assert_eq!(j.trimmed_axes[0][1], p(5.0, 0.0));
        assert_eq!(j.trimmed_axes[1][0], p(5.0, 0.0));
    }

    #[test]
    fn solve_t_stem_through_not_endpoint() {
        let walls = vec![
            WallJoinInput::from_axis(vec![p(0.0, 0.0), p(10.0, 0.0)]),
            WallJoinInput::from_axis(vec![p(5.0, 0.0), p(5.0, 10.0)]),
        ];
        let solved = solve(&walls, JUNCTION_TOLERANCE);
        let t = solved
            .iter()
            .find(|s| s.kind == SolvedJoinKind::T)
            .expect("expected a T junction");
        let roles: Vec<_> = t
            .junction
            .participants
            .iter()
            .map(|p| (p.wall_index, p.role))
            .collect();
        assert!(
            roles
                .iter()
                .any(|(wi, r)| *wi == 0 && matches!(r, JunctionRole::Through(_))),
            "through wall must stay Through, got {roles:?}"
        );
        assert!(
            roles
                .iter()
                .any(|(wi, r)| *wi == 1 && matches!(r, JunctionRole::Endpoint(_))),
            "stem must be Endpoint, got {roles:?}"
        );
        assert_eq!(t.trimmed_axes[0], walls[0].axis);
        assert!(t.trimmed_axes[1][0].distance(p(5.0, 0.0)) < 1e-9);
    }

    #[test]
    fn solve_end_near_hit_is_l_not_t() {
        let near = SOLVER_END_MID_TOLERANCE * 0.4;
        let walls = vec![
            WallJoinInput::from_axis(vec![p(0.0, 0.0), p(10.0, 0.0)]),
            WallJoinInput::from_axis(vec![p(10.0 - near, 0.0), p(10.0 - near, 8.0)]),
        ];
        let solved = solve(&walls, SOLVER_END_MID_TOLERANCE);
        assert_eq!(solved.len(), 1, "expected one clustered junction, got {solved:?}");
        assert_eq!(solved[0].kind, SolvedJoinKind::L);
        assert!(solved[0]
            .junction
            .participants
            .iter()
            .all(|p| matches!(p.role, JunctionRole::Endpoint(_))));
        assert!(!solved[0]
            .junction
            .participants
            .iter()
            .any(|p| matches!(p.role, JunctionRole::Through(_))));
    }

    #[test]
    fn solve_n_way_cluster_one_pass() {
        let e = vec![p(0.0, 0.0), p(10.0, 0.0)];
        let n = vec![p(0.0, 0.0), p(0.0, 10.0)];
        let w = vec![p(0.0, 0.0), p(-10.0, 0.0)];
        let s = vec![p(0.0, 0.0), p(0.0, -10.0)];
        let walls: Vec<_> = [e, n, w, s]
            .into_iter()
            .map(WallJoinInput::from_axis)
            .collect();
        let solved = solve(&walls, JUNCTION_TOLERANCE);
        assert_eq!(solved.len(), 1);
        let j = &solved[0];
        assert_eq!(j.kind, SolvedJoinKind::NWay);
        assert_eq!(j.junction.participants.len(), 4);
        assert!(j.junction.is_multi_wall());
        assert_eq!(j.junction.endpoint_count(), 4);
        assert_eq!(j.trimmed_axes.len(), 4);
        for axis in &j.trimmed_axes {
            assert!(axis[0].distance(p(0.0, 0.0)) < 1e-9);
        }
    }

    #[test]
    fn solve_pair_force_l_trims_overhang() {
        let through = WallJoinInput::from_axis(vec![p(0.0, 0.0), p(10.0, 0.0)]);
        let stem = WallJoinInput::from_axis(vec![p(5.0, 1.0), p(5.0, 4.0)]);
        let l = solve_pair(&through, &stem, true).unwrap();
        assert_eq!(l.kind, JoinKind::L);
        assert_eq!(l.axis_a[1], p(5.0, 0.0));
        assert_eq!(l.axis_b[0], p(5.0, 0.0));
        let t = solve_pair(&through, &stem, false).unwrap();
        assert_eq!(t.kind, JoinKind::T);
        assert_eq!(t.axis_a, through.axis);
    }

    #[test]
    fn solve_l_matched_layers_have_miter_unmatched_are_none() {
        let matched = layer(0.2, -0.1, "core");
        let extra = layer(0.1, 0.1, "finish");
        let walls = vec![
            wall_with_layers(
                vec![p(0.0, 0.0), p(10.0, 0.0)],
                vec![matched.clone(), extra],
            ),
            wall_with_layers(vec![p(10.0, 0.0), p(10.0, 10.0)], vec![matched]),
        ];
        let solved = solve(&walls, JUNCTION_TOLERANCE);
        assert_eq!(solved.len(), 1);
        let fps0 = &solved[0].footprints[0];
        assert_eq!(fps0.len(), 2);
        assert!(fps0[0].is_some(), "matched core layer should miter");
        assert!(
            fps0[1].is_none(),
            "unmatched finish layer must stay None for corner-extension fallback"
        );
    }

    #[test]
    fn solve_t_through_cutout_and_stem_butt() {
        let core = layer(0.2, -0.2, "core");
        let extra = MiterLayer::with_id(0.2, 0.0, "other", "Finish", uuid::Uuid::new_v4());
        let walls = vec![
            wall_with_layers(
                vec![p(0.0, 0.0), p(10.0, 0.0)],
                vec![core.clone(), extra],
            ),
            wall_with_layers(vec![p(5.0, 0.0), p(5.0, 8.0)], vec![core]),
        ];
        let solved = solve(&walls, JUNCTION_TOLERANCE);
        let t = solved
            .iter()
            .find(|s| s.kind == SolvedJoinKind::T)
            .expect("T");
        assert!(
            t.footprints[1][0].is_some(),
            "stem should have a T-butt footprint"
        );
        assert_eq!(t.trimmed_axes[0], walls[0].axis);
        assert!(
            t.footprints[0][0].is_none(),
            "through core stays rectangular, got {:?}",
            t.footprints[0]
        );
        assert!(
            t.footprints[0][1].is_some(),
            "approach finish should receive a cutout, got {:?}",
            t.footprints[0]
        );
    }

    #[test]
    fn solve_pair_arc_bulge_differs_from_chord() {
        let ly = layer(0.2, -0.1, "core");
        let mut a = wall_with_layers(vec![p(0.0, 0.0), p(10.0, 0.0)], vec![ly.clone()]);
        let b = wall_with_layers(vec![p(10.0, 0.0), p(10.0, 10.0)], vec![ly]);
        let straight = solve_pair(&a, &b, true).unwrap();
        a.bulges = vec![0.5];
        let arced = solve_pair(&a, &b, true).unwrap();
        assert_ne!(
            straight.footprints_a[0], arced.footprints_a[0],
            "bulge-aware miter must not equal the chord miter"
        );
    }

    #[test]
    fn solve_pair_near_face_override_vs_clear() {
        let stem_l = layer(0.2, -0.1, "core");
        let through_layers = vec![
            layer(0.2, -0.2, "core"),
            layer(0.2, 0.0, "other"),
        ];
        let mut stem = wall_with_layers(vec![p(5.0, 0.0), p(5.0, 8.0)], vec![stem_l]);
        let through = wall_with_layers(vec![p(0.0, 0.0), p(10.0, 0.0)], through_layers);
        let auto = solve_pair(&stem, &through, false).unwrap();
        stem.override_start = Some(JunctionOverride {
            default_style: Some(JoinOverrideStyle::FarFace),
            layer_pairs: Vec::new(),
            layer_gaps: Vec::new(),
        });
        let far = solve_pair(&stem, &through, false).unwrap();
        stem.override_start = Some(JunctionOverride {
            default_style: Some(JoinOverrideStyle::NearFace),
            layer_pairs: Vec::new(),
            layer_gaps: Vec::new(),
        });
        let near = solve_pair(&stem, &through, false).unwrap();
        stem.override_start = None;
        let cleared = solve_pair(&stem, &through, false).unwrap();
        assert_eq!(
            auto.footprints_a[0], cleared.footprints_a[0],
            "clearing the override must restore automatic miter"
        );
        assert_ne!(
            far.footprints_a[0], auto.footprints_a[0],
            "FarFace override must change the stem footprint"
        );
        assert_eq!(near.footprints_b, auto.footprints_b);
        assert_eq!(far.footprints_b, auto.footprints_b);
        assert_eq!(near.axis_b, through.axis);
    }

    #[test]
    fn persisted_layer_pair_no_extend_matches_in_pair_and_n_way() {
        let stem_layer = layer(0.2, -0.1, "core");
        let stem_layer_id = stem_layer.layer_id;
        let through = wall_with_layers(
            vec![p(0.0, 0.0), p(10.0, 0.0)],
            vec![layer(0.2, -0.2, "core")],
        );
        let mut stem = wall_with_layers(
            vec![p(5.0, 0.0), p(5.0, 8.0)],
            vec![stem_layer],
        );
        let persisted_style = JunctionOverride {
            default_style: None,
            layer_pairs: vec![super::super::join::LayerPairOverride {
                layer_a: LayerRef {
                    material_id: "core".to_string(),
                    role_tag: None,
                    index: 0,
                    layer_id: Some(stem_layer_id),
                },
                layer_b: None,
                style: JoinOverrideStyle::NoExtend,
            }],
            layer_gaps: Vec::new(),
        };

        let automatic = solve_pair(&stem, &through, false).unwrap();
        stem.override_start = Some(persisted_style.clone());
        let overridden = solve_pair(&stem, &through, false).unwrap();
        assert_ne!(
            automatic.footprints_a[0], overridden.footprints_a[0],
            "persisted layer_pairs NoExtend must override the automatic pair footprint"
        );

        let n_layer = layer(0.2, -0.1, "core");
        let n_layer_id = n_layer.layer_id;
        let mut n_walls = vec![
            wall_with_layers(vec![p(0.0, 0.0), p(10.0, 0.0)], vec![n_layer]),
            wall_with_layers(
                vec![p(0.0, 0.0), p(0.0, 10.0)],
                vec![layer(0.2, -0.1, "core")],
            ),
            wall_with_layers(
                vec![p(0.0, 0.0), p(-10.0, 0.0)],
                vec![layer(0.2, -0.1, "core")],
            ),
        ];
        let n_automatic = solve(&n_walls, JUNCTION_TOLERANCE);
        assert_eq!(n_automatic.len(), 1);
        n_walls[0].override_start = Some(JunctionOverride {
            layer_pairs: vec![super::super::join::LayerPairOverride {
                layer_a: LayerRef {
                    material_id: "core".to_string(),
                    role_tag: None,
                    index: 0,
                    layer_id: Some(n_layer_id),
                },
                layer_b: None,
                style: JoinOverrideStyle::NoExtend,
            }],
            ..JunctionOverride::default()
        });
        let n_overridden = solve(&n_walls, JUNCTION_TOLERANCE);
        assert_eq!(n_overridden.len(), 1);
        assert_ne!(
            n_automatic[0].footprints[0][0], n_overridden[0].footprints[0][0],
            "persisted layer_pairs NoExtend must override the automatic N-way footprint"
        );
    }

    fn close(a: (f64, f64), b: (f64, f64), eps: f64) -> bool {
        (a.0 - b.0).abs() < eps && (a.1 - b.1).abs() < eps
    }

    #[test]
    fn layer_pair_miter_on_one_wall_trims_both_stacks() {
        let id_a0 = uuid::Uuid::new_v4();
        let id_a1 = uuid::Uuid::new_v4();
        let id_b0 = uuid::Uuid::new_v4();
        let id_b1 = uuid::Uuid::new_v4();
        let mut wall_a = wall_with_layers(
            vec![p(0.0, 0.0), p(10.0, 0.0)],
            vec![
                MiterLayer::with_id(0.1, -0.2, "inner", "Finish", id_a0),
                MiterLayer::with_id(0.1, 0.1, "outer", "Finish", id_a1),
            ],
        );
        let wall_b = wall_with_layers(
            vec![p(10.0, 0.0), p(10.0, 10.0)],
            vec![
                MiterLayer::with_id(0.1, -0.2, "inner", "Finish", id_b0),
                MiterLayer::with_id(0.1, 0.1, "outer", "Finish", id_b1),
            ],
        );
        wall_a.override_end = Some(JunctionOverride {
            default_style: None,
            layer_pairs: vec![super::super::join::LayerPairOverride {
                layer_a: LayerRef {
                    material_id: "inner".into(),
                    role_tag: None,
                    index: 0,
                    layer_id: Some(id_a0),
                },
                layer_b: Some(LayerRef {
                    material_id: "outer".into(),
                    role_tag: None,
                    index: 1,
                    layer_id: Some(id_b1),
                }),
                style: JoinOverrideStyle::Miter,
            }],
            layer_gaps: Vec::new(),
        });
        let solved = solve_pair(&wall_a, &wall_b, true).unwrap();
        let a0 = solved.footprints_a[0].as_ref().expect("A inner mitered");
        let b1 = solved.footprints_b[1].as_ref().expect("B outer must miter too");
        let shared = a0
            .iter()
            .any(|pa| b1.iter().any(|pb| close(*pa, *pb, 1e-5)));
        assert!(
            shared,
            "reciprocal miter must share a vertex, A={a0:?} B={b1:?}"
        );
    }

    #[test]
    fn solve_n_way_one_pass_consistent_footprints() {
        let ly = layer(0.2, -0.1, "core");
        let walls = vec![
            wall_with_layers(vec![p(0.0, 0.0), p(10.0, 0.0)], vec![ly.clone()]),
            wall_with_layers(vec![p(0.0, 0.0), p(0.0, 10.0)], vec![ly.clone()]),
            wall_with_layers(vec![p(0.0, 0.0), p(-10.0, 0.0)], vec![ly]),
        ];
        let solved = solve(&walls, JUNCTION_TOLERANCE);
        assert_eq!(solved.len(), 1);
        assert_eq!(solved[0].kind, SolvedJoinKind::NWay);
        for (i, fps) in solved[0].footprints.iter().enumerate() {
            assert_eq!(fps.len(), 1, "wall {i}");
            assert!(
                fps[0].is_some(),
                "N-way wall {i} should get a miter in one pass, not pairwise overwrite"
            );
        }
    }

    fn layer_with_fn(th: f64, off: f64, mat: &str, func: &str) -> MiterLayer {
        MiterLayer::with_id(th, off, mat, func, uuid::Uuid::new_v4())
    }

    #[test]
    fn solve_x_crossing_two_continuous_walls_orthogonal() {
        let wall_h = wall_with_layers(
            vec![p(-10.0, 0.0), p(10.0, 0.0)],
            vec![
                layer_with_fn(0.02, -0.12, "Plaster", "Finish"),
                layer_with_fn(0.20, -0.10, "Concrete", "Structural"),
                layer_with_fn(0.02, 0.10, "Plaster", "Finish"),
            ],
        );
        let wall_v = wall_with_layers(
            vec![p(0.0, -10.0), p(0.0, 10.0)],
            vec![
                layer_with_fn(0.02, -0.12, "Plaster", "Finish"),
                layer_with_fn(0.20, -0.10, "Concrete", "Structural"),
                layer_with_fn(0.02, 0.10, "Plaster", "Finish"),
            ],
        );
        let walls = vec![wall_h, wall_v];
        let solved = solve(&walls, JUNCTION_TOLERANCE);
        assert_eq!(solved.len(), 1, "expected 1 X-crossing junction, got {solved:?}");
        let j = &solved[0];
        assert_eq!(j.kind, SolvedJoinKind::X);
        assert_eq!(j.junction.participants.len(), 2);
        assert_eq!(j.footprints.len(), 2);

        // Wall 0 (priority through horizontal wall):
        // Core layer (1) must remain continuous and rectangular (None).
        assert!(j.footprints[0][1].is_none(), "through wall core must be None (intact)");
        // South finish (0) and north finish (2) must be notched into 2 rings.
        let south_cutout = j.footprints[0][0].as_ref().expect("south finish cutout");
        let north_cutout = j.footprints[0][2].as_ref().expect("north finish cutout");
        let south_rings = miter::split_footprint_rings(south_cutout);
        let north_rings = miter::split_footprint_rings(north_cutout);
        assert_eq!(south_rings.len(), 2, "south cutout must be 2 separate rings");
        assert_eq!(north_rings.len(), 2, "north cutout must be 2 separate rings");

        // Verify notch gap width on south finish (cutout between x=-0.10 and x=0.10)
        let s_ring0_max_x = south_rings[0].iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        let s_ring1_min_x = south_rings[1].iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        assert!((s_ring0_max_x - (-0.10)).abs() < 1e-4, "south finish ring 0 max x={s_ring0_max_x}");
        assert!((s_ring1_min_x - 0.10).abs() < 1e-4, "south finish ring 1 min x={s_ring1_min_x}");

        // Wall 1 (crossing vertical wall):
        // All layers should be split into 2 rings separated by NaN.
        let cross_south_finish = j.footprints[1][0].as_ref().expect("crossing west finish");
        let cross_core = j.footprints[1][1].as_ref().expect("crossing core");
        let _cross_north_finish = j.footprints[1][2].as_ref().expect("crossing east finish");

        let core_rings = miter::split_footprint_rings(cross_core);
        assert_eq!(core_rings.len(), 2, "crossing core must split into 2 distinct rings");

        // Stem 1 (south ring): max y = -0.10 (butts at south face of through core)
        let stem1_max_y = core_rings[0].iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        let stem1_min_y = core_rings[0].iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        assert!((stem1_max_y - (-0.10)).abs() < 1e-4, "stem 1 core max y={stem1_max_y} vs -0.10");
        assert!((stem1_min_y - (-10.0)).abs() < 1e-4, "stem 1 core min y={stem1_min_y} vs -10.0");
        assert!(core_rings[0].iter().all(|p| p.1 <= -0.10 + 1e-4), "no stem 1 vertex inside through core");

        // Stem 2 (north ring): min y = +0.10 (butts at north face of through core)
        let stem2_min_y = core_rings[1].iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        let stem2_max_y = core_rings[1].iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        assert!((stem2_min_y - 0.10).abs() < 1e-4, "stem 2 core min y={stem2_min_y} vs 0.10");
        assert!((stem2_max_y - 10.0).abs() < 1e-4, "stem 2 core max y={stem2_max_y} vs 10.0");
        assert!(core_rings[1].iter().all(|p| p.1 >= 0.10 - 1e-4), "no stem 2 vertex inside through core");

        // Crossing finish rings stop at approach-side outer finish face y = -0.12 and y = 0.12
        let finish_rings = miter::split_footprint_rings(cross_south_finish);
        assert_eq!(finish_rings.len(), 2);
        let f_stem1_max_y = finish_rings[0].iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        let f_stem2_min_y = finish_rings[1].iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        assert!((f_stem1_max_y - (-0.12)).abs() < 1e-4, "finish stem 1 max y={f_stem1_max_y} vs -0.12");
        assert!((f_stem2_min_y - 0.12).abs() < 1e-4, "finish stem 2 min y={f_stem2_min_y} vs 0.12");
    }

    #[test]
    fn solve_x_crossing_priority_by_core_thickness() {
        // Wall 0 (thin core 10cm) vs Wall 1 (thick core 24cm)
        let wall_0 = wall_with_layers(
            vec![p(-10.0, 0.0), p(10.0, 0.0)],
            vec![
                layer_with_fn(0.015, -0.065, "Plaster", "Finish"),
                layer_with_fn(0.10, -0.05, "Concrete", "Structural"),
                layer_with_fn(0.015, 0.05, "Plaster", "Finish"),
            ],
        );
        let wall_1 = wall_with_layers(
            vec![p(0.0, -10.0), p(0.0, 10.0)],
            vec![
                layer_with_fn(0.015, -0.135, "Plaster", "Finish"),
                layer_with_fn(0.24, -0.12, "Concrete", "Structural"),
                layer_with_fn(0.015, 0.12, "Plaster", "Finish"),
            ],
        );
        let walls = vec![wall_0, wall_1];
        let solved = solve(&walls, JUNCTION_TOLERANCE);
        assert_eq!(solved.len(), 1);
        let j = &solved[0];
        assert_eq!(j.kind, SolvedJoinKind::X);

        // Wall 1 has thicker structural core (24cm > 10cm), so Wall 1 must win priority (Through).
        assert!(j.footprints[1][1].is_none(), "wall 1 (thicker core) must have intact core footprint");
        // Wall 0 (crossing) must have split core footprint.
        let w0_core = j.footprints[0][1].as_ref().expect("wall 0 core");
        let rings = miter::split_footprint_rings(w0_core);
        assert_eq!(rings.len(), 2, "wall 0 core must be split into 2 rings by wall 1");
    }

    #[test]
    fn solve_x_crossing_diagonal() {
        let d1 = wall_with_layers(
            vec![p(-10.0, -10.0), p(10.0, 10.0)],
            vec![
                layer_with_fn(0.02, -0.12, "Plaster", "Finish"),
                layer_with_fn(0.20, -0.10, "Concrete", "Structural"),
                layer_with_fn(0.02, 0.10, "Plaster", "Finish"),
            ],
        );
        let d2 = wall_with_layers(
            vec![p(-10.0, 10.0), p(10.0, -10.0)],
            vec![
                layer_with_fn(0.02, -0.12, "Plaster", "Finish"),
                layer_with_fn(0.20, -0.10, "Concrete", "Structural"),
                layer_with_fn(0.02, 0.10, "Plaster", "Finish"),
            ],
        );
        let walls = vec![d1, d2];
        let solved = solve(&walls, JUNCTION_TOLERANCE);
        assert_eq!(solved.len(), 1);
        let j = &solved[0];
        assert_eq!(j.kind, SolvedJoinKind::X);

        // Priority wall (0) core is intact (None)
        assert!(j.footprints[0][1].is_none(), "priority diagonal wall core is intact");
        // Crossing wall (1) core is split into 2 rings
        let cross_core = j.footprints[1][1].as_ref().expect("crossing diagonal core");
        let rings = miter::split_footprint_rings(cross_core);
        assert_eq!(rings.len(), 2, "crossing diagonal core is split into 2 rings");

        // The butt line for diagonal stems is angled: verify butt vertices lie near junction
        let stem1_butt_pts: Vec<_> = rings[0].iter().filter(|p| p.0 > -1.0 && p.1 < 1.0).collect();
        assert!(!stem1_butt_pts.is_empty(), "stem 1 must have butt vertices near junction");
    }

    #[test]
    fn solve_x_crossing_one_through_and_two_opposite_stems() {
        let through = wall_with_layers(
            vec![p(-10.0, 0.0), p(10.0, 0.0)],
            vec![
                layer_with_fn(0.02, -0.12, "Plaster", "Finish"),
                layer_with_fn(0.20, -0.10, "Concrete", "Structural"),
                layer_with_fn(0.02, 0.10, "Plaster", "Finish"),
            ],
        );
        let stem_south = wall_with_layers(
            vec![p(0.0, -10.0), p(0.0, 0.0)],
            vec![
                layer_with_fn(0.02, -0.12, "Plaster", "Finish"),
                layer_with_fn(0.20, -0.10, "Concrete", "Structural"),
                layer_with_fn(0.02, 0.10, "Plaster", "Finish"),
            ],
        );
        let stem_north = wall_with_layers(
            vec![p(0.0, 10.0), p(0.0, 0.0)],
            vec![
                layer_with_fn(0.02, -0.12, "Plaster", "Finish"),
                layer_with_fn(0.20, -0.10, "Concrete", "Structural"),
                layer_with_fn(0.02, 0.10, "Plaster", "Finish"),
            ],
        );
        let walls = vec![through, stem_south, stem_north];
        let solved = solve(&walls, JUNCTION_TOLERANCE);
        assert_eq!(solved.len(), 1);
        let j = &solved[0];
        assert_eq!(j.kind, SolvedJoinKind::X);
        assert_eq!(j.junction.participants.len(), 3);

        // Through wall: core intact (None), south finish and north finish notched
        assert!(j.footprints[0][1].is_none(), "through wall core intact");
        assert!(j.footprints[0][0].is_some(), "through south finish notched");
        assert!(j.footprints[0][2].is_some(), "through north finish notched");

        // Stem south (wall 1): core butts at y = -0.10
        let s_core = j.footprints[1][1].as_ref().expect("south stem core");
        let s_core_max_y = s_core.iter().map(|pt| pt.1).fold(f64::NEG_INFINITY, f64::max);
        assert!((s_core_max_y - (-0.10)).abs() < 1e-4, "south stem core max y={s_core_max_y} vs -0.10");

        // Stem north (wall 2): core butts at y = 0.10
        let n_core = j.footprints[2][1].as_ref().expect("north stem core");
        let n_core_min_y = n_core.iter().map(|pt| pt.1).fold(f64::INFINITY, f64::min);
        assert!((n_core_min_y - 0.10).abs() < 1e-4, "north stem core min y={n_core_min_y} vs 0.10");
    }

    #[test]
    fn solve_x_crossing_same_side_stems_classified_as_nway() {
        let through = wall_with_layers(
            vec![p(-10.0, 0.0), p(10.0, 0.0)],
            vec![
                layer_with_fn(0.02, -0.12, "Plaster", "Finish"),
                layer_with_fn(0.20, -0.10, "Concrete", "Structural"),
                layer_with_fn(0.02, 0.10, "Plaster", "Finish"),
            ],
        );
        // Both stems approach from the north (+Y side) at different angles
        let stem1 = wall_with_layers(
            vec![p(-5.0, 5.0), p(0.0, 0.0)],
            vec![layer(0.20, -0.10, "Concrete")],
        );
        let stem2 = wall_with_layers(
            vec![p(5.0, 5.0), p(0.0, 0.0)],
            vec![layer(0.20, -0.10, "Concrete")],
        );
        let walls = vec![through, stem1, stem2];
        let solved = solve(&walls, JUNCTION_TOLERANCE);
        assert_eq!(solved.len(), 1);
        let j = &solved[0];
        assert_eq!(j.kind, SolvedJoinKind::NWay, "same-side stems must be classified as NWay, not X");
    }

    #[test]
    fn solve_x_crossing_acute_angle_near_parallel() {
        // Two walls crossing at < 5 degrees (atan2(1.0, 20.0) ~ 2.86 degrees)
        let w1 = wall_with_layers(
            vec![p(-10.0, 0.0), p(10.0, 0.0)],
            vec![
                layer_with_fn(0.02, -0.12, "Plaster", "Finish"),
                layer_with_fn(0.20, -0.10, "Concrete", "Structural"),
                layer_with_fn(0.02, 0.10, "Plaster", "Finish"),
            ],
        );
        let w2 = wall_with_layers(
            vec![p(-10.0, -0.5), p(10.0, 0.5)],
            vec![
                layer_with_fn(0.02, -0.12, "Plaster", "Finish"),
                layer_with_fn(0.20, -0.10, "Concrete", "Structural"),
                layer_with_fn(0.02, 0.10, "Plaster", "Finish"),
            ],
        );
        let solved = solve(&[w1, w2], JUNCTION_TOLERANCE);
        assert_eq!(solved.len(), 1);
        let j = &solved[0];
        assert_eq!(j.kind, SolvedJoinKind::X);

        // Crossing wall must have both halves preserved without vanishing
        let cross_fps = &j.footprints[1];
        for (li, fp) in cross_fps.iter().enumerate() {
            let fp = fp.as_ref().expect(&format!("layer {li} must be present"));
            let rings = miter::split_footprint_rings(fp);
            assert_eq!(rings.len(), 2, "layer {li} must have both halves present in acute X-crossing");
        }

        // Verify that crossing core rings are capped strictly outside through core band y in (-0.10, 0.10)
        let core_fp = cross_fps[1].as_ref().unwrap();
        let core_rings = miter::split_footprint_rings(core_fp);
        assert_eq!(core_rings.len(), 2);
        let stem1_max_y = core_rings[0].iter().map(|pt| pt.1).fold(f64::NEG_INFINITY, f64::max);
        let stem2_min_y = core_rings[1].iter().map(|pt| pt.1).fold(f64::INFINITY, f64::min);
        assert!(stem1_max_y <= -0.10 + 1e-4, "stem 1 core max y={stem1_max_y} must be <= -0.10");
        assert!(stem2_min_y >= 0.10 - 1e-4, "stem 2 core min y={stem2_min_y} must be >= 0.10");
        assert!(
            core_rings[0].iter().all(|pt| pt.1 <= -0.10 + 1e-4),
            "no stem 1 core vertex inside through core"
        );
        assert!(
            core_rings[1].iter().all(|pt| pt.1 >= 0.10 - 1e-4),
            "no stem 2 core vertex inside through core"
        );
    }

    #[test]
    fn solve_x_crossing_curved_arc_wall() {
        // Through wall along X axis from (-10, 0) to (10, 0), core y in [-0.10, 0.10]
        let through = wall_with_layers(
            vec![p(-10.0, 0.0), p(10.0, 0.0)],
            vec![
                layer_with_fn(0.02, -0.12, "Plaster", "Finish"),
                layer_with_fn(0.20, -0.10, "Concrete", "Structural"),
                layer_with_fn(0.02, 0.10, "Plaster", "Finish"),
            ],
        );
        // Arc wall: (-2, -4) to (-2, 4) with bulge 1.0 (semicircle with center at (-2, 0), radius 4, apex at (2, 0))
        // True arc crosses y = 0 at (2.0, 0.0). Its chord is at x = -2 (does not cross (2, 0)).
        let mut cross = wall_with_layers(
            vec![p(-2.0, -4.0), p(-2.0, 4.0)],
            vec![
                layer_with_fn(0.02, -0.12, "Plaster", "Finish"),
                layer_with_fn(0.20, -0.10, "Concrete", "Structural"),
                layer_with_fn(0.02, 0.10, "Plaster", "Finish"),
            ],
        );
        cross.bulges = vec![1.0];
        let walls = [through, cross.clone()];
        let solved = solve(&walls, JUNCTION_TOLERANCE);
        assert_eq!(solved.len(), 1);
        let j = &solved[0];
        assert_eq!(j.kind, SolvedJoinKind::X);
        assert!(
            (j.junction.point.x - 2.0).hypot(j.junction.point.y - 0.0) < 1e-4,
            "junction point must be at true arc intersection (2.0, 0.0), got {:?}",
            j.junction.point
        );

        assert!(x_junction_is_unsupported(&walls, &j.junction));
        assert!(j.footprints.iter().flatten().all(Option::is_none), "must not flatten curved layers");
        assert_eq!(j.trimmed_axes[1], cross.axis, "must not trim curved wall");

        // Sub-bulge retargeting preserves exact curvature (each half-arc has bulge = tan(pi/8) = sqrt(2)-1)
        let ((_, stem1_b), (_, stem2_b)) = split_axis_and_bulges_at_point(&cross.axis, &cross.bulges, j.junction.point, Some(0));
        assert!((stem1_b[0] - (2.0_f64.sqrt() - 1.0)).abs() < 1e-4, "stem 1 bulge must preserve circle curvature");
        assert!((stem2_b[0] - (2.0_f64.sqrt() - 1.0)).abs() < 1e-4, "stem 2 bulge must preserve circle curvature");
    }

    #[test]
    fn solve_x_crossing_degenerate_stub_fallback_dropped() {
        // Horizontal through wall from (-10, 0) to (10, 0), core y in [-0.10, 0.10], finish y in [-0.12, 0.12]
        let through = wall_with_layers(
            vec![p(-10.0, 0.0), p(10.0, 0.0)],
            vec![
                layer_with_fn(0.02, -0.12, "Plaster", "Finish"),
                layer_with_fn(0.20, -0.10, "Concrete", "Structural"),
                layer_with_fn(0.02, 0.10, "Plaster", "Finish"),
            ],
        );
        // Crossing wall along Y from (0, -5.0) to (0, 0.05).
        // It crosses the through wall axis at (0, 0), but only extends 0.05m north (ending at y = 0.05),
        // which is inside the through core (core extends up to y = 0.10, finish up to y = 0.12).
        // Stem 2 (y in [0, 0.05]) cannot reach through wall north face (y >= 0.10) and is dropped.
        let cross = wall_with_layers(
            vec![p(0.0, -5.0), p(0.0, 0.05)],
            vec![
                layer_with_fn(0.02, -0.12, "Plaster", "Finish"),
                layer_with_fn(0.20, -0.10, "Concrete", "Structural"),
                layer_with_fn(0.02, 0.10, "Plaster", "Finish"),
            ],
        );
        let solved = solve(&[through, cross], JUNCTION_TOLERANCE);
        assert_eq!(solved.len(), 1);
        let j = &solved[0];
        assert_eq!(j.kind, SolvedJoinKind::X);

        // Crossing wall core must only contain the valid south stem capped at y = -0.10;
        // degenerate north stub must NOT produce an untrimmed polygon overlapping the through core.
        let cross_core = j.footprints[1][1].as_ref().expect("crossing core footprint");
        let core_rings = miter::split_footprint_rings(cross_core);
        assert_eq!(core_rings.len(), 1, "degenerate stub inside core must be dropped, leaving 1 valid ring");
        for ring in &core_rings {
            for pt in ring {
                assert!(pt.1 <= -0.10 + 1e-4, "no vertex of crossing wall can penetrate through core band [-0.10, 0.10], got y={}", pt.1);
            }
        }
    }

    #[test]
    fn solve_x_crossing_asymmetrical_multilayer() {
        // Multi-layer exterior through wall:
        // South: WDVS (0.16) + Plaster (0.015)
        // Core: Concrete (0.24, offset -0.040 to 0.200)
        // North: Interior Plaster (0.015, offset 0.200 to 0.215)
        let through = wall_with_layers(
            vec![p(-10.0, 0.0), p(10.0, 0.0)],
            vec![
                layer_with_fn(0.015, -0.215, "PlasterExterior", "Finish"),
                layer_with_fn(0.16, -0.200, "WDVS", "Thermal"),
                layer_with_fn(0.24, -0.040, "Concrete", "Structural"),
                layer_with_fn(0.015, 0.200, "PlasterInterior", "Finish"),
            ],
        );
        let cross = wall_with_layers(
            vec![p(0.0, -10.0), p(0.0, 10.0)],
            vec![
                layer_with_fn(0.015, -0.115, "PlasterInterior", "Finish"),
                layer_with_fn(0.20, -0.100, "Concrete", "Structural"),
                layer_with_fn(0.015, 0.100, "PlasterInterior", "Finish"),
            ],
        );
        let walls = vec![through, cross];
        let solved = solve(&walls, JUNCTION_TOLERANCE);
        assert_eq!(solved.len(), 1);
        let j = &solved[0];
        assert_eq!(j.kind, SolvedJoinKind::X);

        // Through wall: core intact (None), finishes and insulation notched
        assert!(j.footprints[0][2].is_none(), "through core is intact");
        assert!(j.footprints[0][0].is_some(), "exterior plaster is notched");
        assert!(j.footprints[0][1].is_some(), "WDVS insulation is notched");
        assert!(j.footprints[0][3].is_some(), "interior plaster is notched");

        // Crossing wall core must butt at south core face (y = -0.040) and north core face (y = 0.200)
        let c_core = j.footprints[1][1].as_ref().expect("crossing core");
        let core_rings = miter::split_footprint_rings(c_core);
        assert_eq!(core_rings.len(), 2);
        let stem1_core_max_y = core_rings[0].iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        let stem2_core_min_y = core_rings[1].iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        assert!((stem1_core_max_y - (-0.040)).abs() < 1e-4, "stem 1 core max y={stem1_core_max_y} vs -0.040");
        assert!((stem2_core_min_y - 0.200).abs() < 1e-4, "stem 2 core min y={stem2_core_min_y} vs 0.200");
    }

    #[test]
    fn solve_x_crossing_oblique_degenerate_stubs() {
        // Horizontal through wall from (-10, 0) to (10, 0), core y in [-0.10, 0.10], finish y in [-0.12, 0.12]
        let through = wall_with_layers(
            vec![p(-10.0, 0.0), p(10.0, 0.0)],
            vec![
                layer_with_fn(0.02, -0.12, "Plaster", "Finish"),
                layer_with_fn(0.20, -0.10, "Concrete", "Structural"),
                layer_with_fn(0.02, 0.10, "Plaster", "Finish"),
            ],
        );

        // Case 1: 30 degrees, length 0.15m (stem 2 far end y = 0.15 * sin 30° = 0.075 < 0.10 core face)
        let angle_30 = std::f64::consts::PI / 6.0;
        let (cos_30, sin_30) = (angle_30.cos(), angle_30.sin());
        let cross_30 = wall_with_layers(
            vec![
                p(-5.0 * cos_30, -5.0 * sin_30),
                p(0.15 * cos_30, 0.15 * sin_30),
            ],
            vec![
                layer_with_fn(0.02, -0.12, "Plaster", "Finish"),
                layer_with_fn(0.20, -0.10, "Concrete", "Structural"),
                layer_with_fn(0.02, 0.10, "Plaster", "Finish"),
            ],
        );
        let solved_30 = solve(&[through.clone(), cross_30], JUNCTION_TOLERANCE);
        assert_eq!(solved_30.len(), 1);
        let j_30 = &solved_30[0];
        assert_eq!(j_30.kind, SolvedJoinKind::X);
        for (li, fp) in j_30.footprints[1].iter().enumerate() {
            let fp = fp.as_ref().expect(&format!("30 deg layer {li} must be present"));
            let rings = miter::split_footprint_rings(fp);
            assert_eq!(rings.len(), 1, "30 deg stub must drop invalid north half for layer {li}");
            for pt in &rings[0] {
                assert!(
                    pt.1 <= -0.10 + 1e-4,
                    "30 deg crossing layer {li} vertex must not penetrate through core [-0.10, 0.10], got y={}",
                    pt.1
                );
            }
        }

        // Case 2: 10 degrees, length 0.50m (stem 2 far end y = 0.50 * sin 10° ≈ 0.0868 < 0.10 core face)
        let angle_10 = 10.0 * std::f64::consts::PI / 180.0;
        let (cos_10, sin_10) = (angle_10.cos(), angle_10.sin());
        let cross_10 = wall_with_layers(
            vec![
                p(-5.0 * cos_10, -5.0 * sin_10),
                p(0.50 * cos_10, 0.50 * sin_10),
            ],
            vec![
                layer_with_fn(0.02, -0.12, "Plaster", "Finish"),
                layer_with_fn(0.20, -0.10, "Concrete", "Structural"),
                layer_with_fn(0.02, 0.10, "Plaster", "Finish"),
            ],
        );
        let solved_10 = solve(&[through.clone(), cross_10], JUNCTION_TOLERANCE);
        assert_eq!(solved_10.len(), 1);
        let j_10 = &solved_10[0];
        assert_eq!(j_10.kind, SolvedJoinKind::X);
        for (li, fp) in j_10.footprints[1].iter().enumerate() {
            let fp = fp.as_ref().expect(&format!("10 deg layer {li} must be present"));
            let rings = miter::split_footprint_rings(fp);
            assert_eq!(rings.len(), 1, "10 deg stub must drop invalid north half for layer {li}");
            for pt in &rings[0] {
                assert!(
                    pt.1 <= -0.10 + 1e-4,
                    "10 deg crossing layer {li} vertex must not penetrate through core [-0.10, 0.10], got y={}",
                    pt.1
                );
            }
        }

        // Case 3: 60 degrees, length 0.13m (stem 2 far end y = 0.13 * sin 60° ≈ 0.1126: reaches core > 0.10, but not finish < 0.12)
        let angle_60 = std::f64::consts::PI / 3.0;
        let (cos_60, sin_60) = (angle_60.cos(), angle_60.sin());
        let cross_60 = wall_with_layers(
            vec![
                p(-5.0 * cos_60, -5.0 * sin_60),
                p(0.13 * cos_60, 0.13 * sin_60),
            ],
            vec![
                layer_with_fn(0.02, -0.12, "Plaster", "Finish"),
                layer_with_fn(0.20, -0.10, "Concrete", "Structural"),
                layer_with_fn(0.02, 0.10, "Plaster", "Finish"),
            ],
        );
        let solved_60 = solve(&[through, cross_60], JUNCTION_TOLERANCE);
        assert_eq!(solved_60.len(), 1);
        let j_60 = &solved_60[0];
        assert_eq!(j_60.kind, SolvedJoinKind::X);

        // Core layer (li = 1): valid on both sides (south <= -0.10, north >= 0.10)
        let core_fp_60 = j_60.footprints[1][1].as_ref().expect("60 deg core");
        let core_rings_60 = miter::split_footprint_rings(core_fp_60);
        assert_eq!(core_rings_60.len(), 2, "60 deg core reaches core face on both sides");
        for pt in &core_rings_60[0] {
            assert!(pt.1 <= -0.10 + 1e-4, "60 deg south core vertex must be <= -0.10, got y={}", pt.1);
        }
        for pt in &core_rings_60[1] {
            assert!(pt.1 >= 0.10 - 1e-4, "60 deg north core vertex must be >= 0.10, got y={}", pt.1);
        }

        // Finish layers (li = 0, 2): north stub does not reach finish face y = 0.12, so only 1 ring on south
        for &li in &[0, 2] {
            let fin_fp = j_60.footprints[1][li].as_ref().expect(&format!("60 deg finish layer {li}"));
            let fin_rings = miter::split_footprint_rings(fin_fp);
            assert_eq!(fin_rings.len(), 1, "60 deg finish layer {li} must drop north stub that does not reach y=0.12");
            for pt in &fin_rings[0] {
                assert!(pt.1 <= -0.10 + 1e-4, "60 deg south finish vertex must be <= -0.10, got y={}", pt.1);
            }
        }
    }

    fn x_probe_walls(style: Option<JoinOverrideStyle>) -> Vec<WallJoinInput> {
        let layers = || {
            vec![
                layer_with_fn(0.02, -0.12, "Plaster", "Finish"),
                layer_with_fn(0.20, -0.10, "Concrete", "Structural"),
                layer_with_fn(0.02, 0.10, "Plaster", "Finish"),
            ]
        };
        let wall_h = wall_with_layers(vec![p(-10.0, 0.0), p(10.0, 0.0)], layers());
        let mut wall_v = wall_with_layers(vec![p(0.0, -10.0), p(0.0, 10.0)], layers());
        wall_v.override_span = style.map(|s| JunctionOverride {
            default_style: Some(s),
            ..Default::default()
        });
        vec![wall_h, wall_v]
    }

    fn ring_extent(fp: Option<&Vec<(f64, f64)>>, axis_y: bool) -> Vec<(f64, f64)> {
        let Some(fp) = fp else { return Vec::new() };
        miter::split_footprint_rings(fp)
            .iter()
            .map(|r| {
                let v = |pt: &(f64, f64)| if axis_y { pt.1 } else { pt.0 };
                (
                    r.iter().map(v).fold(f64::INFINITY, f64::min),
                    r.iter().map(v).fold(f64::NEG_INFINITY, f64::max),
                )
            })
            .collect()
    }

    #[test]
    fn solve_x_crossing_overrides_are_honoured_not_ignored() {
        let ends = |style: Option<JoinOverrideStyle>, li: usize| {
            let solved = solve(&x_probe_walls(style), JUNCTION_TOLERANCE);
            assert_eq!(solved.len(), 1);
            assert_eq!(solved[0].kind, SolvedJoinKind::X);
            ring_extent(solved[0].footprints[1][li].as_ref(), true)
        };
        let near = |a: f64, b: f64| (a - b).abs() < 1e-3;

        // Baseline (no override): core butts at the core faces, finishes at
        // the outer finish faces.
        let auto_core = ends(None, 1);
        assert!(near(auto_core[0].1, -0.10) && near(auto_core[1].0, 0.10), "{auto_core:?}");
        let auto_fin = ends(None, 0);
        assert!(near(auto_fin[0].1, -0.12) && near(auto_fin[1].0, 0.12), "{auto_fin:?}");

        // Butt / NearFace keep the near-face butt.
        for style in [JoinOverrideStyle::Butt, JoinOverrideStyle::NearFace] {
            let core = ends(Some(style), 1);
            assert!(near(core[0].1, -0.10) && near(core[1].0, 0.10), "{style:?} core {core:?}");
            let fin = ends(Some(style), 0);
            assert!(near(fin[0].1, -0.12) && near(fin[1].0, 0.12), "{style:?} finish {fin:?}");
        }

        // FarFace must reach the far face of the matched through layer: the core
        // stems cross the whole through core, the finish stems reach the
        // core-side face of the approach finish. Both differ from auto.
        let far_core = ends(Some(JoinOverrideStyle::FarFace), 1);
        assert_eq!(far_core.len(), 2, "{far_core:?}");
        assert!(near(far_core[0].1, 0.10), "south core stem reaches far face +0.10: {far_core:?}");
        assert!(near(far_core[1].0, -0.10), "north core stem reaches far face -0.10: {far_core:?}");
        let far_fin = ends(Some(JoinOverrideStyle::FarFace), 0);
        assert!(near(far_fin[0].1, -0.10) && near(far_fin[1].0, 0.10), "{far_fin:?}");

        // NoExtend stops on the axis point instead of silently butting.
        let none_core = ends(Some(JoinOverrideStyle::NoExtend), 1);
        assert!(near(none_core[0].1, 0.0) && near(none_core[1].0, 0.0), "{none_core:?}");
    }

    #[test]
    fn solve_x_crossing_override_leaves_through_core_intact() {
        let solved = solve(&x_probe_walls(Some(JoinOverrideStyle::FarFace)), JUNCTION_TOLERANCE);
        assert!(solved[0].footprints[0][1].is_none(), "through core stays untouched");
    }

    fn arc_wall(layers: Vec<MiterLayer>, a: (f64, f64), b: (f64, f64), bulge: f64) -> WallJoinInput {
        let mut w = wall_with_layers(vec![p(a.0, a.1), p(b.0, b.1)], layers);
        w.bulges = vec![bulge];
        w
    }

    fn core_layers(core: f64) -> Vec<MiterLayer> {
        vec![
            layer_with_fn(0.02, -core * 0.5 - 0.02, "Plaster", "Finish"),
            layer_with_fn(core, -core * 0.5, "Concrete", "Structural"),
            layer_with_fn(0.02, core * 0.5, "Plaster", "Finish"),
        ]
    }

    #[test]
    fn solve_x_curved_wall_never_becomes_priority_even_with_thicker_core() {
        // Arc (apex (0,-1)) with the *thicker* core crossed by a straight wall.
        let arc = arc_wall(core_layers(0.30), (-5.0, 0.0), (5.0, 0.0), 0.2);
        let straight = wall_with_layers(vec![p(0.0, -5.0), p(0.0, 5.0)], core_layers(0.20));
        let walls = [arc, straight];
        let solved = solve(&walls, JUNCTION_TOLERANCE);
        assert_eq!(solved.len(), 1, "{solved:?}");
        let j = &solved[0];
        assert_eq!(j.kind, SolvedJoinKind::X);
        assert!(
            (j.junction.point.x).hypot(j.junction.point.y + 1.0) < 1e-3,
            "junction at the arc apex, got {:?}",
            j.junction.point
        );

        assert!(x_junction_is_unsupported(&walls, &j.junction));
        assert!(j.footprints.iter().flatten().all(Option::is_none));
        assert_eq!(j.trimmed_axes[0], walls[0].axis);
        assert_eq!(j.trimmed_axes[1], walls[1].axis);
    }

    #[test]
    fn solve_x_two_curved_walls_is_unsupported_and_leaves_walls_untouched() {
        let a = arc_wall(core_layers(0.20), (-5.0, 0.0), (5.0, 0.0), 0.2);
        let b = arc_wall(core_layers(0.20), (0.0, -5.0), (0.0, 5.0), 0.1);
        let walls = [a, b];
        let solved = solve(&walls, JUNCTION_TOLERANCE);
        assert_eq!(solved.len(), 1, "{solved:?}");
        let j = &solved[0];
        assert_eq!(j.kind, SolvedJoinKind::X);
        assert!(x_junction_is_unsupported(&walls, &j.junction));
        assert!(
            j.footprints.iter().flatten().all(|fp| fp.is_none()),
            "unsupported crossing must not produce any (chord) geometry: {:?}",
            j.footprints
        );
    }

    #[test]
    fn solve_x_polyline_stem_keeps_finish_layers_on_both_sides() {
        // Thin finishes on a polyline (U) crossing wall: the mitered finish
        // candidate is rejected (diagonal reaches past the outer face), so the
        // finite-chord fallback must keep every leg for *all* layers.
        let layers = || {
            vec![
                layer_with_fn(0.015, -0.135, "Plaster", "Finish"),
                layer_with_fn(0.24, -0.12, "Concrete", "Structural"),
                layer_with_fn(0.015, 0.12, "Plaster", "Finish"),
            ]
        };
        let through = wall_with_layers(vec![p(0.0, 0.0), p(20.0, 0.0)], layers());
        let u = wall_with_layers(
            vec![p(5.0, -8.0), p(5.0, 8.0), p(15.0, 8.0), p(15.0, -8.0)],
            layers(),
        );
        let solved = solve(&[through, u], JUNCTION_TOLERANCE);
        assert_eq!(solved.len(), 2);
        for j in &solved {
            assert_eq!(j.kind, SolvedJoinKind::X);
            for li in 0..3 {
                let fp = j.footprints[1][li].as_ref().expect("layer footprint");
                let rings = miter::split_footprint_rings(fp);
                assert_eq!(rings.len(), 2, "layer {li} at {:?}: {rings:?}", j.junction.point);
                let limit = if li == 1 { 0.12 } else { 0.135 };
                for ring in &rings {
                    // Every vertex that is near the crossing stays outside the
                    // through face of its side; no vertex lands in the band.
                    for v in ring.iter().filter(|v| (v.0 - j.junction.point.x).abs() < 0.3 && v.1.abs() < 1.0) {
                        assert!(v.1.abs() >= limit - 1e-3, "layer {li} vertex {v:?} inside band");
                    }
                }
            }
        }
    }

    #[test]
    fn solve_network_chain_u_shape_miters_both_ends_of_middle_wall() {
        let layers = core_layers(0.20);
        let w0 = wall_with_layers(vec![p(0.0, 0.0), p(10.0, 0.0)], layers.clone());
        let w1 = wall_with_layers(vec![p(10.0, 0.0), p(10.0, 10.0)], layers.clone());
        let w2 = wall_with_layers(vec![p(10.0, 10.0), p(0.0, 10.0)], layers);

        let net = solve_network(&[w0, w1, w2], JUNCTION_TOLERANCE);
        assert_eq!(net.junctions.len(), 2, "must detect 2 L-corners");
        assert_eq!(net.footprints.len(), 3);

        // Middle wall (w1) must have mitered footprints at BOTH ends:
        // end 0 (y=0) and end 1 (y=10)
        let w1_core = net.footprints[1][1].as_ref().expect("w1 core footprint");
        let has_end0_miter = w1_core.iter().any(|pt| pt.1 < 0.0);
        let has_end1_miter = w1_core.iter().any(|pt| pt.1 > 10.0);
        assert!(has_end0_miter, "w1 must have miter extending below y=0 for junction with w0: {w1_core:?}");
        assert!(has_end1_miter, "w1 must have miter extending above y=10 for junction with w2: {w1_core:?}");
    }

    #[test]
    fn solve_network_closed_room_cycle_miters_all_four_corners() {
        let layers = core_layers(0.20);
        let w0 = wall_with_layers(vec![p(0.0, 0.0), p(10.0, 0.0)], layers.clone());
        let w1 = wall_with_layers(vec![p(10.0, 0.0), p(10.0, 10.0)], layers.clone());
        let w2 = wall_with_layers(vec![p(10.0, 10.0), p(0.0, 10.0)], layers.clone());
        let w3 = wall_with_layers(vec![p(0.0, 10.0), p(0.0, 0.0)], layers);

        let net = solve_network(&[w0, w1, w2, w3], JUNCTION_TOLERANCE);
        assert_eq!(net.junctions.len(), 4, "must detect all 4 corners in closed room");
        assert_eq!(net.footprints.len(), 4);

        for (wi, wall_fps) in net.footprints.iter().enumerate() {
            let core = wall_fps[1].as_ref().unwrap_or_else(|| panic!("wall {wi} core"));
            assert!(!core.is_empty(), "wall {wi} core must not be empty");
        }
    }
}
