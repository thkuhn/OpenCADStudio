//! Unified wall-junction classification, axis trim, and layer footprints.
//!
//! [`solve`] / [`solve_pair`] are the single source for L/T/N classification,
//! End vs Mid, trimmed axes, and per-layer miter/cutout polygons.

use glam::DVec3;

use super::join::{
    apply_junction_to_axes, detect_junctions, join_wall_axes_as_l_with_bulges,
    join_wall_axes_with_bulges, junction_rays, END_MID_TOLERANCE, JoinError, JoinKind, Junction,
    JunctionOverride, JunctionRole, LayerRef, JUNCTION_TOLERANCE,
};
use super::miter::{
    junction_wall_geoms, mitered_junction_layer_footprints_with_overrides,
    mitered_layer_footprints_with_override_and_bulges, through_wall_cutout_footprints_with_bulges,
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
}

impl WallJoinInput {
    pub fn from_axis(axis: Vec<DVec3>) -> Self {
        Self {
            axis,
            bulges: Vec::new(),
            layers: Vec::new(),
            override_start: None,
            override_end: None,
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

/// L-corner, T (stem + through), or N-way (≥3 participants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolvedJoinKind {
    L,
    T,
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

/// Detect and classify junctions among `walls`.
///
/// `tol` is the endpoint clustering radius (defaults to [`JUNCTION_TOLERANCE`]
/// when non-positive). End vs Mid uses [`END_MID_TOLERANCE`] inside
/// [`detect_junctions`].
pub fn solve(walls: &[WallJoinInput], tol: f64) -> Vec<SolvedJunction> {
    let cluster_tol = if tol > 0.0 { tol } else { JUNCTION_TOLERANCE };
    let axes: Vec<&[DVec3]> = walls.iter().map(|w| w.axis.as_slice()).collect();
    let detected = detect_junctions(&axes, cluster_tol);

    let mut out = Vec::with_capacity(detected.len());
    for junction in detected {
        let kind = classify_kind(&junction);
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

fn classify_kind(junction: &Junction) -> SolvedJoinKind {
    if junction.participants.len() >= 3 {
        return SolvedJoinKind::NWay;
    }
    let has_through = junction
        .participants
        .iter()
        .any(|p| matches!(p.role, JunctionRole::Through(_)));
    if has_through {
        SolvedJoinKind::T
    } else {
        SolvedJoinKind::L
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
        SolvedJoinKind::L | SolvedJoinKind::T | SolvedJoinKind::NWay => {
            apply_junction_to_axes(&axes, junction)
        }
    }
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
    let ov_a = end_a.and_then(|e| a.override_at(e));
    let ov_b = end_b.and_then(|e| b.override_at(e));

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
        through_wall_cutout_footprints_with_bulges(
            &a2,
            &a.layers,
            &b2,
            &b.layers,
            eb,
            &a.bulges,
            &b.bulges,
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
        through_wall_cutout_footprints_with_bulges(
            &b2,
            &b.layers,
            &a2,
            &a.layers,
            ea,
            &b.bulges,
            &a.bulges,
        )
    } else {
        vec![None; b.layers.len()]
    };

    (fps_a, fps_b)
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
        let join_kind = match kind {
            SolvedJoinKind::T => JoinKind::T,
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
            if let Some(stem) = junction
                .participants
                .iter()
                .find(|p| matches!(p.role, JunctionRole::Endpoint(_)))
            {
                if let JunctionRole::Endpoint(stem_end) = stem.role {
                    let through = &walls[part.wall_index];
                    let stem_w = &walls[stem.wall_index];
                    let t_axis = trimmed
                        .get(part.wall_index)
                        .map(|a| axis_2d(a))
                        .unwrap_or_else(|| axis_2d(&through.axis));
                    let s_axis = trimmed
                        .get(stem.wall_index)
                        .map(|a| axis_2d(a))
                        .unwrap_or_else(|| axis_2d(&stem_w.axis));
                    fps = through_wall_cutout_footprints_with_bulges(
                        &t_axis,
                        &through.layers,
                        &s_axis,
                        &stem_w.layers,
                        stem_end,
                        &through.bulges,
                        &stem_w.bulges,
                    );
                }
            }
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
        let near = END_MID_TOLERANCE * 0.4;
        let walls = vec![
            WallJoinInput::from_axis(vec![p(0.0, 0.0), p(10.0, 0.0)]),
            WallJoinInput::from_axis(vec![p(10.0 - near, 0.0), p(10.0 - near, 8.0)]),
        ];
        let solved = solve(&walls, END_MID_TOLERANCE);
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
        });
        let far = solve_pair(&stem, &through, false).unwrap();
        stem.override_start = Some(JunctionOverride {
            default_style: Some(JoinOverrideStyle::NearFace),
            layer_pairs: Vec::new(),
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
}
