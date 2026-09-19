//! Document-level wall join, auto-join, reverse, and junction rebuild.

#![allow(unused_imports)]
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

use acadrust::entities::{LwPolyline, LwVertex, Point};
use acadrust::tables::AppId;
use acadrust::types::{Vector2, Vector3};
use acadrust::{CadDocument, EntityType, Handle};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use glam::DVec3;

use crate::scene::model::hatch_model::{HatchModel, HatchPattern};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

use super::{
    self as engine, Storey, StyleLibrary, Wall, WallJustification, WallLayer,
    join::{self, JoinError, JoinKind},
    junction_solver::{self, WallJoinInput},
    library::load_or_seed,
    plan_view::{PhaseFilter, PlanPhase},
    wall_style::{
        base_width_from_layers, effective_layers_for_wall_bb, migrate_gap_before_to_axis_offset,
        LayerFunction, ResolvedLayer, WallStyle,
    },
};

#[allow(unused_imports)]
use super::display_apply::*;
use super::junction_pick::*;
use super::xdata::*;
use super::wall_package::*;
use super::wall_regen::*;
use super::storey_xdata::*;
use super::opening_xdata::*;

pub(crate) const WALL_JOIN_SNAP_RADIUS: f64 = 0.3;

/// The point `ext_len` further out from `axis[idx]`, continuing in the same
/// direction the wall's end segment already runs (i.e. straight past the
/// corner, away from the wall's interior). Used to build the corner-
/// extension hint for [`regenerate_wall_representation_with_corner`] (Bug 4).
pub(crate) fn extended_endpoint(axis: &[DVec3], idx: usize, ext_len: f64) -> DVec3 {
    let dir = if idx == 0 {
        (axis[0] - axis[1]).normalize_or_zero()
    } else {
        (axis[idx] - axis[idx - 1]).normalize_or_zero()
    };
    axis[idx] + dir * ext_len
}

/// Shortest 2D distance from `p` to the finite segment `a`–`b`.
pub(crate) fn point_to_segment_dist_2d(p: DVec3, a: DVec3, b: DVec3) -> f64 {
    let ab = DVec3::new(b.x - a.x, b.y - a.y, 0.0);
    let ap = DVec3::new(p.x - a.x, p.y - a.y, 0.0);
    let len_sq = ab.length_squared();
    if len_sq < 1e-24 {
        return ap.length();
    }
    let t = (ap.dot(ab) / len_sq).clamp(0.0, 1.0);
    let closest = DVec3::new(a.x + ab.x * t, a.y + ab.y * t, 0.0);
    DVec3::new(p.x - closest.x, p.y - closest.y, 0.0).length()
}

/// Shortest 2D distance from `p` to any segment of the wall axis polyline.
pub(crate) fn point_to_polyline_dist_2d(p: DVec3, poly: &[DVec3]) -> f64 {
    let mut best = f64::INFINITY;
    for pair in poly.windows(2) {
        best = best.min(point_to_segment_dist_2d(p, pair[0], pair[1]));
    }
    best
}

/// Minimum 2D distance between either wall's endpoints and the other wall's
/// axis polyline. Used by auto-join snap detection.
pub(crate) fn wall_endpoint_to_axis_dist(axis_a: &[DVec3], axis_b: &[DVec3]) -> f64 {
    if axis_a.len() < 2 || axis_b.len() < 2 {
        return f64::INFINITY;
    }
    let mut best = f64::INFINITY;
    for end in [axis_a[0], *axis_a.last().unwrap()] {
        best = best.min(point_to_polyline_dist_2d(end, axis_b));
    }
    for end in [axis_b[0], *axis_b.last().unwrap()] {
        best = best.min(point_to_polyline_dist_2d(end, axis_a));
    }
    best
}

/// Drop junction overrides on ends that no longer sit next to any other wall.
pub(crate) fn drop_orphaned_junction_overrides(scene: &mut Scene, wall_handle: Handle) {
    let axis = get_wall_vertices(scene, wall_handle);
    if axis.len() < 2 {
        return;
    }
    for (end_index, pt) in [(0usize, axis[0]), (1usize, *axis.last().unwrap())] {
        if !wall_end_has_nearby_partner(scene, wall_handle, pt) {
            remove_junction_override(scene, wall_handle, end_index);
        }
    }
}

pub(crate) fn wall_end_has_nearby_partner(scene: &Scene, wall_handle: Handle, pt: DVec3) -> bool {
    for entity in scene.document.entities() {
        let other = entity.common().handle;
        if other == wall_handle || !is_wall_axis_xdata(entity) {
            continue;
        }
        let other_axis = get_wall_vertices(scene, other);
        if other_axis.len() < 2 {
            continue;
        }
        if point_to_polyline_dist_2d(pt, &other_axis) <= WALL_JOIN_SNAP_RADIUS {
            return true;
        }
    }
    false
}

/// Minimum 2D distance between either wall's endpoints and the other wall's
/// endpoints only (no interior/axis-mid points). A small distance here means
/// a clean End-End (L) match; used to rank join candidates above vaguer
/// End-Mid (T) matches at a similar overall distance.
pub(crate) fn wall_endpoint_to_endpoint_dist(axis_a: &[DVec3], axis_b: &[DVec3]) -> f64 {
    if axis_a.len() < 2 || axis_b.len() < 2 {
        return f64::INFINITY;
    }
    let mut best = f64::INFINITY;
    for end_a in [axis_a[0], *axis_a.last().unwrap()] {
        for end_b in [axis_b[0], *axis_b.last().unwrap()] {
            best = best.min(DVec3::new(end_a.x - end_b.x, end_a.y - end_b.y, 0.0).length());
        }
    }
    best
}

/// Find the closest other wall axis whose geometry is within
/// [`WALL_JOIN_SNAP_RADIUS`] of `wall_handle`'s axis and that can actually be
/// joined (L/T intersection exists). Returns `None` when nothing is in range —
/// a graceful no-op for callers.
///
/// Candidates are ranked with a clear-endpoint priority: an End-End (L) match
/// within the snap radius always wins over a vaguer End-Mid (T) match, even
/// if the T candidate happens to be nominally closer, since an exact endpoint
/// coincidence is the more deliberate, less error-prone user intent to snap
/// against. Ties within each tier fall back to plain distance.
///
/// `excluding` skips walls already being processed (the edited wall itself —
/// this also prevents a wall from ever auto-joining to itself while it is
/// still being drawn — plus any partners already joined in the same finalize
/// pass).
pub fn find_wall_to_auto_join(
    scene: &Scene,
    wall_handle: Handle,
    excluding: &[Handle],
) -> Option<Handle> {
    let axis = get_wall_vertices(scene, wall_handle);
    if axis.len() < 2 {
        return None;
    }
    // (priority tier, distance): tier 0 = clear endpoint-to-endpoint match
    // within the snap radius, tier 1 = endpoint-to-interior (T) match only.
    let mut best: Option<(Handle, u8, f64)> = None;
    for entity in scene.document.entities() {
        let other = entity.common().handle;
        if other == wall_handle || excluding.contains(&other) {
            continue;
        }
        if !is_wall_axis_xdata(entity) {
            continue;
        }
        let other_axis = get_wall_vertices(scene, other);
        if other_axis.len() < 2 {
            continue;
        }
        let dist = wall_endpoint_to_axis_dist(&axis, &other_axis);
        if dist > WALL_JOIN_SNAP_RADIUS {
            continue;
        }
        // Only accept candidates that the join engine can actually connect.
        let bulges = get_wall_bulges(scene, wall_handle);
        let other_bulges = get_wall_bulges(scene, other);
        if join::join_wall_axes_with_bulges(&axis, &bulges, &other_axis, &other_bulges).is_err() {
            continue;
        }
        let endpoint_dist = wall_endpoint_to_endpoint_dist(&axis, &other_axis);
        let tier = if endpoint_dist <= WALL_JOIN_SNAP_RADIUS {
            0
        } else {
            1
        };
        if best.map_or(true, |(_, best_tier, best_d)| {
            (tier, dist) < (best_tier, best_d)
        }) {
            best = Some((other, tier, dist));
        }
    }
    best.map(|(h, _, _)| h)
}

/// Attempt automatic L/T joins for `wall_handle` against nearby walls (up to
/// one join per endpoint). When 3+ walls meet at a shared point, resolves the
/// full junction together (N-way miter); otherwise falls back to pairwise
/// [`join_two_walls_in_document`]. Returns every axis + derived handle touched
/// so callers can refresh 2D and 3D in one `bump_entities` call. Never errors —
/// failed/no-candidate joins are silent no-ops.
pub fn try_auto_join_nearby_walls(
    scene: &mut Scene,
    wall_handle: Handle,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Vec<Handle> {
    let mut touched = Vec::new();

    // Step 4: Symmetric peer unlinking for walls that are no longer nearby.
    // When a wall vertex is dragged away from a junction, its peer links and
    // mitered footprints must be cleaned up on both sides.
    let old_peers = engine::owner_index::peers_of(&scene.document, wall_handle);
    let axis = get_wall_vertices(scene, wall_handle);
    for peer in old_peers {
        let peer_axis = get_wall_vertices(scene, peer);
        if wall_endpoint_to_axis_dist(&axis, &peer_axis) > WALL_JOIN_SNAP_RADIUS {
            engine::owner_index::unlink_peers(&mut scene.document, wall_handle, peer);
            drop_orphaned_junction_overrides(scene, wall_handle);
            drop_orphaned_junction_overrides(scene, peer);
            // Peer representation might be mitered against us; refresh it.
            if let Ok(t) = regenerate_wall_representation_with_rules_and_substitutions(
                scene,
                peer,
                display_rules,
                style_substitutions,
                library_override,
            ) {
                touched.extend(t);
            }
        }
    }

    let mut excluding = vec![wall_handle];

    // Prefer multi-wall junction resolution when 3+ walls already cluster at
    // an endpoint of `wall_handle` (or a nearby through-hit).
    let junction_touched = try_join_multi_wall_junctions(
        scene,
        wall_handle,
        library_override,
        display_rules,
        style_substitutions,
    );
    if !junction_touched.is_empty() {
        touched.extend(junction_touched.iter().copied());
        // Walls already rebuilt via the junction path shouldn't be pairwise-
        // joined again in this pass.
        for h in &junction_touched {
            if *h != wall_handle && !excluding.contains(h) {
                // Only treat axis handles as exclusions (derived handles are
                // also in the touched list).
                if scene
                    .document
                    .get_entity(*h)
                    .is_some_and(is_wall_axis_xdata)
                {
                    excluding.push(*h);
                }
            }
        }
    }

    // Pairwise fallback for remaining simple 2-wall L/T joins (at most two:
    // start endpoint + end endpoint against different walls).
    for _ in 0..2 {
        let Some(other) = find_wall_to_auto_join(scene, wall_handle, &excluding) else {
            break;
        };
        match join_two_walls_in_document(
            scene,
            wall_handle,
            other,
            library_override,
            display_rules,
            style_substitutions,
        ) {
            Ok((_kind, handles)) => {
                touched.extend(handles);
                excluding.push(other);
            }
            Err(_) => {
                // Candidate looked joinable at search time but failed now
                // (geometry race); skip it and stop rather than looping forever.
                excluding.push(other);
            }
        }
    }
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    touched
}

/// After a newly committed 2-point wall, join it to the previous chain
/// segment and any other walls within [`WALL_JOIN_SNAP_RADIUS`]. Join
/// failures are non-fatal (the new segment stays in the document).
pub(crate) fn auto_join_committed_wall_segment(
    scene: &mut Scene,
    wall_handle: Handle,
    last_committed: Option<Handle>,
    library: Option<&StyleLibrary>,
) {
    let axis = get_wall_vertices(scene, wall_handle);
    let ends: Vec<DVec3> = if axis.len() >= 2 {
        vec![axis[0], *axis.last().unwrap()]
    } else {
        Vec::new()
    };
    for end in ends {
        let mut cluster = vec![wall_handle];
        if let Some(prev) = last_committed {
            if prev != wall_handle {
                let prev_axis = get_wall_vertices(scene, prev);
                let prev_near = prev_axis.len() >= 2
                    && (prev_axis[0].distance(end) <= WALL_JOIN_SNAP_RADIUS
                        || prev_axis.last().unwrap().distance(end) <= WALL_JOIN_SNAP_RADIUS);
                if prev_near {
                    cluster.push(prev);
                }
            }
        }
        for h in all_wall_axis_handles(scene) {
            if h == wall_handle || cluster.contains(&h) {
                continue;
            }
            let other = get_wall_vertices(scene, h);
            if other.len() < 2 {
                continue;
            }
            let near = other[0].distance(end) <= WALL_JOIN_SNAP_RADIUS
                || other.last().unwrap().distance(end) <= WALL_JOIN_SNAP_RADIUS
                || (0..other.len() - 1).any(|i| {
                    point_to_segment_dist_2d(end, other[i], other[i + 1]) <= WALL_JOIN_SNAP_RADIUS
                });
            if near {
                cluster.push(h);
            }
        }
        cluster.sort_by_key(|h| h.value());
        cluster.dedup();
        if cluster.len() < 2 {
            continue;
        }
        match join_junction_in_document(
            scene,
            &cluster,
            Some(end),
            library,
            None,
            None,
        ) {
            Ok(_) => {}
            Err(JoinError::Ambiguous) => {
                if let Some(prev) = last_committed {
                    if let Err(err) = join_junction_in_document(
                        scene,
                        &[prev, wall_handle],
                        Some(end),
                        library,
                        None,
                        None,
                    ) {
                        queue_override_warning(format!("AEC_WALL join: {err:?}"));
                    }
                }
            }
            Err(err) => {
                queue_override_warning(format!("AEC_WALL join: {err:?}"));
            }
        }
    }
    let _ = try_auto_join_nearby_walls(scene, wall_handle, library, None, None);
}

/// Detect multi-wall junctions involving `wall_handle` and resolve them with
/// N-way miter. Returns touched handles (empty when no multi-wall junction).
pub(crate) fn try_join_multi_wall_junctions(
    scene: &mut Scene,
    wall_handle: Handle,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Vec<Handle> {
    let handles = all_wall_axis_handles(scene);
    if handles.len() < 3 {
        return Vec::new();
    }
    let inputs: Vec<WallJoinInput> = handles
        .iter()
        .map(|h| wall_join_input_from_scene(scene, *h))
        .collect();
    // Step 4: Use a larger tolerance for multi-wall junctions to ensure that
    // vertex-dragged walls still cluster with their former junction peers
    // (up to the snap radius) so the full junction is re-resolved together.
    let solved = junction_solver::solve(&inputs, WALL_JOIN_SNAP_RADIUS);

    let self_idx = handles.iter().position(|h| *h == wall_handle);
    let Some(self_idx) = self_idx else {
        return Vec::new();
    };

    let mut touched = Vec::new();
    for sj in solved.into_iter().filter(|s| s.junction.is_multi_wall()) {
        if !sj
            .junction
            .participants
            .iter()
            .any(|p| p.wall_index == self_idx)
        {
            continue;
        }
        // Restrict the participant set to walls in this junction.
        let part_handles: Vec<Handle> = sj
            .junction
            .participants
            .iter()
            .map(|p| handles[p.wall_index])
            .collect();
        // Use the moved wall's own (post-move) endpoint as the snap point so
        // the rebuilt junction lands exactly where the user dragged it,
        // rather than at the mean of all participants.
        let snap_point = sj
            .junction
            .participants
            .iter()
            .find(|p| p.wall_index == self_idx)
            .and_then(|p| match p.role {
                join::JunctionRole::Endpoint(end_idx) => inputs[self_idx].axis.get(end_idx).copied(),
                join::JunctionRole::Through(_) => None,
            });
        if let Ok(t) = join_junction_in_document(
            scene,
            &part_handles,
            snap_point,
            library_override,
            display_rules,
            style_substitutions,
        ) {
            touched.extend(t);
        }
    }
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    touched
}

/// A single wall's participation in a junction, as discovered by
/// [`walls_at_junction`]: which axis/end it is, and its material layers
/// (outer→inner) expressed as [`join::LayerRef`]s for use in
/// [`join::LayerPairOverride`] construction.
#[derive(Debug, Clone, PartialEq)]
pub struct JunctionParticipant {
    pub axis_handle: Handle,
    pub end_index: usize,
    pub layers: Vec<join::LayerRef>,
    /// `true` when this wall does not end at the junction but merely passes
    /// through it (a T-junction's "through" wall). Such walls have no
    /// editable junction end of their own, but must still be listed so the
    /// Junction Editor shows every connected wall, not just the stem.
    pub is_through: bool,
}

/// Find every wall participating in the same junction node as
/// `(axis_handle, end_index)`, i.e. every wall whose axis endpoint (or
/// through-hit) shares the same clustered point. Reuses the same
/// [`join::detect_junctions`] topology already used by N-way join
/// resolution (see [`try_join_multi_wall_junctions`] /
/// [`join_junction_in_document`]) so the Junction-Editor-Panel and the
/// N-way join resolver always agree on who participates.
///
/// Both [`join::JunctionRole::Endpoint`] and [`join::JunctionRole::Through`]
/// participants are returned — a T-junction's "through" wall has no
/// editable junction end of its own, but must still show up in the list so
/// the user can see it is connected (see `JunctionParticipant::is_through`).
/// When no cluster is found (e.g. an isolated wall end), a single-element
/// result containing just the queried wall is returned so the caller can
/// still build a `JunctionOverride` for it.
pub fn walls_at_junction(
    scene: &Scene,
    axis_handle: Handle,
    end_index: usize,
) -> Vec<JunctionParticipant> {
    let handles = all_wall_axis_handles(scene);
    let axes: Vec<Vec<DVec3>> = handles.iter().map(|h| get_wall_vertices(scene, *h)).collect();
    let axis_refs: Vec<&[DVec3]> = axes.iter().map(|a| a.as_slice()).collect();
    let tol = join::JUNCTION_TOLERANCE.max(1e-4);
    let junctions = join::detect_junctions(&axis_refs, tol);

    let layers_for = |handle: Handle| -> Vec<join::LayerRef> {
        scene
            .document
            .get_entity(handle)
            .and_then(wall_from_entity)
            .map(|w| layer_refs_from_materials(w.layers.iter().map(|l| (l.material.as_str(), l.layer_id))))
            .unwrap_or_default()
    };

    let Some(self_idx) = handles.iter().position(|h| *h == axis_handle) else {
        return vec![JunctionParticipant {
            axis_handle,
            end_index,
            layers: layers_for(axis_handle),
            is_through: false,
        }];
    };

    // `JunctionRole::Endpoint` carries the *raw* vertex index (`0` or
    // `axis.len() - 1`, i.e. potentially > 1 for multi-segment wall axes),
    // while `end_index` (both the parameter here and every other AEC
    // junction-override API, e.g. `write_junction_override`) uses the
    // normalized `0` = start / `1` = end convention. Comparing them
    // directly would never match for walls with more than two axis points,
    // which is exactly why the editor previously fell back to "just this
    // wall" for such walls despite a real junction existing.
    let normalize_end = |raw: usize| -> usize { if raw == 0 { 0 } else { 1 } };

    for junc in &junctions {
        let matches_self = junc.participants.iter().any(|p| {
            p.wall_index == self_idx
                && matches!(p.role, join::JunctionRole::Endpoint(e) if normalize_end(e) == end_index)
        });
        if !matches_self {
            continue;
        }
        let mut out: Vec<JunctionParticipant> = junc
            .participants
            .iter()
            .map(|p| match p.role {
                join::JunctionRole::Endpoint(e) => {
                    let h = handles[p.wall_index];
                    JunctionParticipant {
                        axis_handle: h,
                        end_index: normalize_end(e),
                        layers: layers_for(h),
                        is_through: false,
                    }
                }
                join::JunctionRole::Through(_) => {
                    let h = handles[p.wall_index];
                    JunctionParticipant {
                        axis_handle: h,
                        // A through-wall has no editable end at this
                        // junction; the index is unused for it (no override
                        // lookups are ever keyed by it), just kept out of
                        // the normalized 0/1 range so it can't accidentally
                        // be mistaken for a real endpoint.
                        end_index: usize::MAX,
                        layers: layers_for(h),
                        is_through: true,
                    }
                }
            })
            .collect();
        out.sort_by_key(|p| p.axis_handle.value());
        return out;
    }

    // No cluster found — fall back to just the queried wall.
    vec![JunctionParticipant {
        axis_handle,
        end_index,
        layers: layers_for(axis_handle),
        is_through: false,
    }]
}

/// Through-wall participant at `(axis_handle, end_index)`, if this node is a T.
pub fn through_wall_at_junction(
    scene: &Scene,
    axis_handle: Handle,
    end_index: usize,
) -> Option<JunctionParticipant> {
    walls_at_junction(scene, axis_handle, end_index)
        .into_iter()
        .find(|p| p.is_through)
}

/// Layer interruptions live on the through wall (span key), not the stem end.
pub fn read_through_layer_gaps(
    scene: &Scene,
    axis_handle: Handle,
    end_index: usize,
) -> Vec<join::LayerGapOverride> {
    let mut gaps = Vec::new();
    if let Some(through) = through_wall_at_junction(scene, axis_handle, end_index) {
        if let Some(ov) = read_junction_override(scene, through.axis_handle, THROUGH_SPAN_OVERRIDE_END)
        {
            gaps.extend(ov.layer_gaps);
        }
    }
    if gaps.is_empty() {
        if let Some(ov) = read_junction_override(scene, axis_handle, end_index) {
            gaps.extend(ov.layer_gaps);
        }
    }
    gaps
}

pub(crate) fn persist_through_layer_gaps(
    scene: &mut Scene,
    axis_handle: Handle,
    end_index: usize,
    gaps: &[join::LayerGapOverride],
) {
    let Some(through) = through_wall_at_junction(scene, axis_handle, end_index) else {
        return;
    };
    let mut ov = read_junction_override(scene, through.axis_handle, THROUGH_SPAN_OVERRIDE_END)
        .unwrap_or_default();
    ov.layer_gaps = gaps.to_vec();
    if ov.is_empty() {
        remove_junction_override(scene, through.axis_handle, THROUGH_SPAN_OVERRIDE_END);
    } else {
        write_junction_override(scene, through.axis_handle, THROUGH_SPAN_OVERRIDE_END, &ov);
    }
}

/// Persist a junction override (or clear it) and rebuild every wall in that
/// junction so layer-pair / default-style edits are visible immediately.
pub fn apply_junction_override_and_rebuild(
    scene: &mut Scene,
    axis_handle: Handle,
    end_index: usize,
    override_data: Option<&join::JunctionOverride>,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Vec<Handle> {
    match override_data {
        Some(ov) => {
            if ov.layer_gaps.is_empty() {
                // Pair-only edits do not carry gaps; keep interruptions on
                // the through wall so other ends can still be joined.
            } else {
                persist_through_layer_gaps(scene, axis_handle, end_index, &ov.layer_gaps);
            }
            let mut stem = ov.clone();
            // T-junctions store gaps on the through span; L-corners have no
            // through wall, so the stem end must keep them.
            if through_wall_at_junction(scene, axis_handle, end_index).is_some() {
                stem.layer_gaps.clear();
            }
            if stem.is_empty() {
                remove_junction_override(scene, axis_handle, end_index);
            } else {
                write_junction_override(scene, axis_handle, end_index, &stem);
            }
            // Store the inverted pair on every other endpoint so a miter
            // picked on one wall also shortens the partner layer.
            for p in walls_at_junction(scene, axis_handle, end_index) {
                if p.axis_handle == axis_handle || p.is_through {
                    continue;
                }
                let mut other =
                    read_junction_override(scene, p.axis_handle, p.end_index).unwrap_or_default();
                for pair in &stem.layer_pairs {
                    if let Some(inv) = join::invert_layer_pair(pair) {
                        join::upsert_layer_pair(&mut other.layer_pairs, inv);
                    }
                }
                if other.is_empty() {
                    remove_junction_override(scene, p.axis_handle, p.end_index);
                } else {
                    write_junction_override(scene, p.axis_handle, p.end_index, &other);
                }
            }
        }
        None => {
            persist_through_layer_gaps(scene, axis_handle, end_index, &[]);
            remove_junction_override(scene, axis_handle, end_index);
        }
    }
    let participants = walls_at_junction(scene, axis_handle, end_index);
    let handles: Vec<Handle> = {
        let mut hs: Vec<Handle> = participants.iter().map(|p| p.axis_handle).collect();
        if !hs.contains(&axis_handle) {
            hs.push(axis_handle);
        }
        hs.sort_by_key(|h| h.value());
        hs.dedup();
        hs
    };
    let mut touched = if handles.len() >= 2 {
        join_junction_in_document(
            scene,
            &handles,
            None,
            library_override,
            display_rules,
            style_substitutions,
        )
        .unwrap_or_else(|_| {
            refresh_wall_after_axis_edit(
                scene,
                axis_handle,
                library_override,
                display_rules,
                style_substitutions,
            )
        })
    } else {
        refresh_wall_after_axis_edit(
            scene,
            axis_handle,
            library_override,
            display_rules,
            style_substitutions,
        )
    };
    for h in &handles {
        if !touched.iter().any(|t| t == h) {
            match regenerate_wall_representation_with_rules_and_substitutions(
                scene,
                *h,
                display_rules,
                style_substitutions,
                library_override,
            ) {
                Ok(t) => touched.extend(t),
                Err(_) => touched.push(*h),
            }
        }
    }
    scene.bump_geometry();
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    touched
}

/// Snap all participants of a multi-wall junction to the shared point and
/// rebuild every endpoint wall with N-way mitered layer footprints.
///
/// `handles` are the wall axis handles participating in the junction (order
/// does not matter). Returns every touched axis + derived handle.
///
/// `snap_point`, when provided, overrides the computed junction point (which
/// is otherwise the mean of all participating endpoints) with the given exact
/// position, and widens the detection tolerance to `WALL_JOIN_SNAP_RADIUS` so
/// a vertex that was just dragged (and is therefore no longer exactly
/// coincident with its former peers) still clusters into the same junction.
pub fn join_junction_in_document(
    scene: &mut Scene,
    handles: &[Handle],
    snap_point: Option<DVec3>,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Result<Vec<Handle>, JoinError> {
    if handles.len() < 2 {
        return Err(JoinError::Degenerate);
    }
    let inputs: Vec<WallJoinInput> = handles
        .iter()
        .map(|h| wall_join_input_from_scene(scene, *h))
        .collect();
    if inputs.iter().any(|w| w.axis.len() < 2) {
        return Err(JoinError::Degenerate);
    }
    // Use a slightly looser tol than pure geometry equality so near-miss
    // endpoints from interactive drawing still cluster (snap radius scale).
    // When a snap point is supplied (vertex-move cascade), use the larger
    // snap radius so a just-dragged endpoint still clusters with its peers.
    let tol = if snap_point.is_some() {
        WALL_JOIN_SNAP_RADIUS
    } else {
        join::JUNCTION_TOLERANCE.max(1e-4)
    };
    let solved = junction_solver::solve(&inputs, tol);
    let multi_wall: Vec<_> = solved
        .iter()
        .filter(|s| s.junction.is_multi_wall())
        .collect();
    if multi_wall.len() > 1 {
        // Only one multi-wall junction should be present in the passed handles
        // to avoid ambiguity in which one to rebuild. The caller must filter
        // handles to a single junction's participants.
        return Err(JoinError::Ambiguous);
    }
    // Prefer N-way; otherwise resolve a single 2-wall L (End-End) or T (End-Mid).
    let mut solved_j = if let Some(j) = multi_wall.into_iter().next() {
        j.clone()
    } else {
        let two: Vec<_> = solved
            .into_iter()
            .filter(|s| s.junction.participants.len() == 2)
            .collect();
        if two.len() > 1 {
            return Err(JoinError::Ambiguous);
        }
        two.into_iter().next().ok_or(JoinError::NoIntersection)?
    };
    if let Some(pt) = snap_point {
        solved_j.junction.point = pt;
        let axis_refs: Vec<&[DVec3]> = inputs.iter().map(|w| w.axis.as_slice()).collect();
        solved_j.trimmed_axes = join::apply_junction_to_axes(&axis_refs, &solved_j.junction);
        solved_j.footprints = junction_solver::footprints_for(
            &inputs,
            &solved_j.junction,
            solved_j.kind,
            &solved_j.trimmed_axes,
        );
    }
    let junc = solved_j.junction.clone();
    let snapped = solved_j.trimmed_axes.clone();
    for p in &junc.participants {
        if matches!(p.role, join::JunctionRole::Endpoint(_)) {
            update_wall_vertices(scene, handles[p.wall_index], &snapped[p.wall_index]);
        }
    }

    // Persist cleaned overrides for the participating end only; footprints
    // already came from the solver using that same per-end override.
    for (pi, p) in junc.participants.iter().enumerate() {
        if let join::JunctionRole::Endpoint(end_idx) = p.role {
            if let Some(ov) = read_junction_override(scene, handles[p.wall_index], end_idx) {
                let self_layers = &inputs[p.wall_index].layers;
                let self_refs = layer_refs_from_materials(
                    self_layers.iter().map(|l| (l.material.as_str(), l.layer_id)),
                );
                let other_refs: Vec<join::LayerRef> = junc
                    .participants
                    .iter()
                    .enumerate()
                    .filter(|(oi, _)| *oi != pi)
                    .flat_map(|(_, op)| {
                        inputs.get(op.wall_index).into_iter().flat_map(|w| {
                            layer_refs_from_materials(
                                w.layers.iter().map(|l| (l.material.as_str(), l.layer_id)),
                            )
                        })
                    })
                    .collect();
                let _ = validate_and_persist_junction_override(
                    scene,
                    handles[p.wall_index],
                    end_idx,
                    ov,
                    &self_refs,
                    &other_refs,
                );
            }
        }
    }

    // Max thickness among participants — used for corner_override fallback.
    let thicknesses: Vec<f64> = handles
        .iter()
        .map(|h| {
            scene
                .document
                .get_entity(*h)
                .and_then(wall_thickness_and_height)
                .map(|(t, _, _)| t)
                .unwrap_or(0.0)
        })
        .collect();
    let max_other_half = |self_i: usize| -> f64 {
        thicknesses
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != self_i)
            .map(|(_, t)| t * 0.5)
            .fold(0.0_f64, f64::max)
    };

    let mut touched = Vec::new();
    for part in junc.participants.iter() {
        let h = handles[part.wall_index];
        let axis = &snapped[part.wall_index];
        let empty: Vec<Option<Vec<(f64, f64)>>> = Vec::new();
        let fps = solved_j
            .footprints
            .get(part.wall_index)
            .unwrap_or(&empty);

        let override_pt = match part.role {
            join::JunctionRole::Endpoint(end_idx) => {
                let half = max_other_half(part.wall_index);
                if half > 0.0 {
                    Some((end_idx, extended_endpoint(axis, end_idx, half)))
                } else {
                    None
                }
            }
            join::JunctionRole::Through(_) => None,
        };

        let result = regenerate_wall_representation_with_precomputed_miters_rules_and_substitutions(
            scene,
            h,
            override_pt,
            fps,
            display_rules,
            style_substitutions,
            library_override,
        );
        match result {
            Ok(t) => touched.extend(t),
            Err(_) => touched.push(h),
        }
    }

    // Symmetric pairwise peer links among all junction participants.
    for i in 0..handles.len() {
        for j in (i + 1)..handles.len() {
            engine::owner_index::link_peers(&mut scene.document, handles[i], handles[j]);
        }
    }

    touched.sort_by_key(|h| h.value());
    touched.dedup();
    Ok(touched)
}

/// Join two wall axes in the document, rebuild both representations with
/// mitered layer footprints, and return `(join_kind, every_touched_handle)`.
/// The handle set always includes both axes plus every newly created derived
/// entity so callers can refresh 2D (resident wires/hatches) and 3D (meshes)
/// together.
///
/// After the pairwise axis join, if a third (or more) wall already meets at
/// the join point the full junction is re-resolved with N-way miters so
/// pairwise overwrites cannot leave inconsistent footprints.
pub fn join_two_walls_in_document(
    scene: &mut Scene,
    h_a: Handle,
    h_b: Handle,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Result<(JoinKind, Vec<Handle>), JoinError> {
    join_two_walls_in_document_inner(
        scene,
        h_a,
        h_b,
        false,
        library_override,
        display_rules,
        style_substitutions,
    )
}

/// Like [`join_two_walls_in_document`], but always forms an L-corner
/// (both axes trimmed/extended to the intersection). Used by `AEC_WALLJOIN`.
pub fn join_two_walls_as_l_in_document(
    scene: &mut Scene,
    h_a: Handle,
    h_b: Handle,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Result<(JoinKind, Vec<Handle>), JoinError> {
    join_two_walls_in_document_inner(
        scene,
        h_a,
        h_b,
        true,
        library_override,
        display_rules,
        style_substitutions,
    )
}

pub(crate) fn wall_join_input_from_scene(scene: &Scene, handle: Handle) -> WallJoinInput {
    let axis = get_wall_vertices(scene, handle);
    let last = axis.len().saturating_sub(1);
    WallJoinInput {
        axis,
        bulges: get_wall_bulges(scene, handle),
        layers: wall_layer_data(scene, handle),
        override_start: read_junction_override(scene, handle, 0),
        override_end: if last == 0 {
            None
        } else {
            read_junction_override(scene, handle, last)
        },
        override_span: read_junction_override(scene, handle, THROUGH_SPAN_OVERRIDE_END),
    }
}

pub(crate) fn join_two_walls_in_document_inner(
    scene: &mut Scene,
    h_a: Handle,
    h_b: Handle,
    force_l: bool,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Result<(JoinKind, Vec<Handle>), JoinError> {
    let input_a = wall_join_input_from_scene(scene, h_a);
    let input_b = wall_join_input_from_scene(scene, h_b);
    let joined = junction_solver::solve_pair(&input_a, &input_b, force_l);
    match joined {
        Ok(pair) => {
            let new_a = pair.axis_a;
            let new_b = pair.axis_b;
            let kind = pair.kind;
            let end_a = pair.end_a;
            let end_b = pair.end_b;
            let fps_a = pair.footprints_a;
            let fps_b = pair.footprints_b;
            // Corner-extension fallback (unmatched layers) plus solver
            // precomputed miters (matched layers). Persisted axis vertices stay
            // exactly as the solver trimmed them — only the visible
            // footprint geometry changes.
            let thickness_a = scene
                .document
                .get_entity(h_a)
                .and_then(wall_thickness_and_height)
                .map(|(t, _, _)| t);
            let thickness_b = scene
                .document
                .get_entity(h_b)
                .and_then(wall_thickness_and_height)
                .map(|(t, _, _)| t);
            let override_a = end_a.and_then(|idx| {
                thickness_b.map(|t| (idx, extended_endpoint(&new_a, idx, t * 0.5)))
            });
            let override_b = end_b.and_then(|idx| {
                thickness_a.map(|t| (idx, extended_endpoint(&new_b, idx, t * 0.5)))
            });

            update_wall_vertices(scene, h_a, &new_a);
            update_wall_vertices(scene, h_b, &new_b);

            // Re-resolve the full junction (2-wall L/T or N-way) so pairwise
            // miter writes cannot overwrite each other. Extra walls already
            // meeting at the join point are included.
            let join_pt = end_a
                .map(|i| new_a[i])
                .or_else(|| end_b.map(|i| new_b[i]));
            // Forced L-corners must not go through the T/N-way classifier:
            // detect_junctions would re-open a head-wall overhang as T.
            if !force_l {
                if let Some(pt) = join_pt {
                    let mut participants = vec![h_a, h_b];
                    for entity in scene.document.entities() {
                        let h = entity.common().handle;
                        if h == h_a || h == h_b || !is_wall_axis_xdata(entity) {
                            continue;
                        }
                        let axis = get_wall_vertices(scene, h);
                        if axis.len() < 2 {
                            continue;
                        }
                        let end_hit = [axis[0], *axis.last().unwrap()]
                            .iter()
                            .any(|e| e.distance(pt) <= join::JUNCTION_TOLERANCE.max(1e-4));
                        let through_hit = (0..axis.len() - 1).any(|i| {
                            let d = point_to_segment_dist_2d(pt, axis[i], axis[i + 1]);
                            d <= join::JUNCTION_TOLERANCE.max(1e-4)
                                && axis[i].distance(pt) > join::END_MID_TOLERANCE
                                && axis[i + 1].distance(pt) > join::END_MID_TOLERANCE
                        });
                        if end_hit || through_hit {
                            participants.push(h);
                        }
                    }
                    if let Ok(touched) = join_junction_in_document(
                        scene,
                        &participants,
                        Some(pt),
                        library_override,
                        display_rules,
                        style_substitutions,
                    ) {
                        return Ok((kind, touched));
                    }
                }
            }

            // Symmetric peer links for the successful pairwise join.
            engine::owner_index::link_peers(&mut scene.document, h_a, h_b);

            let mut touched = Vec::new();

            match regenerate_wall_representation_with_precomputed_miters_rules_and_substitutions(
                scene,
                h_a,
                override_a,
                &fps_a,
                display_rules,
                style_substitutions,
                library_override,
            ) {
                Ok(t) => touched.extend(t),
                Err(_) => touched.push(h_a),
            }

            match regenerate_wall_representation_with_precomputed_miters_rules_and_substitutions(
                scene,
                h_b,
                override_b,
                &fps_b,
                display_rules,
                style_substitutions,
                library_override,
            ) {
                Ok(t) => touched.extend(t),
                Err(_) => touched.push(h_b),
            }

            touched.sort_by_key(|h| h.value());
            touched.dedup();
            Ok((kind, touched))
        }
        Err(e) => Err(e),
    }
}

/// Material-stack layers for the join-miter helper (geometry + identity).
/// Empty when `handle` isn't a wall.
pub(crate) fn wall_layer_data(scene: &Scene, handle: Handle) -> Vec<engine::miter::MiterLayer> {
    let Some(entity) = scene.document.get_entity(handle) else {
        return Vec::new();
    };
    if let Some(wall) = wall_from_entity(entity) {
        return wall
            .layers
            .iter()
            .map(|l| {
                engine::miter::MiterLayer::with_id(
                    l.thickness,
                    l.axis_offset,
                    l.material.clone(),
                    l.function.clone(),
                    l.layer_id,
                )
            })
            .collect();
    }
    Vec::new()
}

/// `AEC_WALLJOIN_DO handle_a|handle_b` — the non-interactive handler
/// `WallJoinCommand` dispatches to once both walls are picked; resolves each
/// pick to its wall axis, joins the two axes with [`join::join_wall_axes`],
/// writes the trimmed/extended axes back and regenerates both walls'
/// representation. Reports [`JoinError`] via the command line instead of
/// panicking.
pub fn aec_walljoin_do(
    scene: &mut Scene,
    command_line: &mut CommandLine,
    args: &str,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) {
    let parts: Vec<&str> = args.split('|').collect();
    let [a, b] = parts.as_slice() else {
        command_line.push_error(&crate::tr!("aec", "walljoin-malformed-args"));
        return;
    };
    let (Ok(a), Ok(b)) = (a.parse::<u64>(), b.parse::<u64>()) else {
        command_line.push_error(&crate::tr!("aec", "walljoin-malformed-handles"));
        return;
    };
    let h_a = resolve_wall_package(scene, Handle::new(a));
    let h_b = resolve_wall_package(scene, Handle::new(b));
    if h_a == h_b {
        command_line.push_error(&crate::tr!("aec", "walljoin-two-different"));
        return;
    }
    let is_wall = |scene: &Scene, h: Handle| {
        scene.document.get_entity(h).is_some_and(|e| {
            matches!(
                read_aec_record(e).and_then(|r| r.values.first()),
                Some(XDataValue::String(kind)) if kind == "WALL"
            )
        })
    };
    if !is_wall(scene, h_a) || !is_wall(scene, h_b) {
        command_line.push_error(&crate::tr!("aec", "walljoin-two-walls"));
        return;
    }

    match join_two_walls_as_l_in_document(
        scene,
        h_a,
        h_b,
        library_override,
        display_rules,
        style_substitutions,
    ) {
        Ok((_kind, touched)) => {
            let changes: Vec<_> = touched
                .into_iter()
                .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                .collect();
            if !changes.is_empty() {
                scene.bump_entities(&changes);
            }
            command_line.push_info(&crate::tr!("aec", "walljoin-ok"));
            for msg in take_pending_override_warnings() {
                command_line.push_info(&msg);
            }
        }
        Err(e) => {
            command_line.push_error(&format!("AEC_WALLJOIN: {}", e));
        }
    }
}

// WallExtendCommand moved to walls/extend.rs


// WallReverseCommand moved to walls/reverse.rs


/// Reverse a wall's axis vertex order and mirror its layer stack so the
/// absolute visible footprint (including which material sits on which world
/// side) stays pixel-identical while start/end and left/right-relative-to-
/// direction flip. Regenerates the representation and re-runs auto-join.
///
/// Returns every axis + derived handle touched (including any auto-joined
/// neighbours) so callers can bump 2D/3D together.
pub fn reverse_wall_in_document(
    scene: &mut Scene,
    wall_handle: Handle,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Result<Vec<Handle>, WallRegenError> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let mut axis = get_wall_vertices(scene, wall_handle);
    if axis.len() < 2 {
        return Err(WallRegenError::NotAWall);
    }

    // Capture pre-reverse layer-contour outer bounds for callers/tests that
    // want to assert footprint stability; the reverse itself only needs the
    // axis + layer list transform below.
    axis.reverse();
    update_wall_vertices(scene, wall_handle, &axis);

    // NOTE: the axis-direction flip above already inverts the offset normal
    // used by `layer_contours`, which on its own physically swaps every
    // layer to the opposite absolute side of the wall (this is the intended,
    // visible effect of "reverse direction" — same footprint, materials
    // swapped). We must NOT also mirror/reverse the stored layer list here:
    // doing so cancels the normal flip exactly, leaving the wall completely
    // unchanged (a previous bug). Interior/Exterior justification still
    // swaps with the direction since "interior"/"exterior" is direction-
    // relative.
    if let Some(entity) = scene.document.get_entity(wall_handle) {
        if let Some(mut v2) = wall_from_entity(entity) {
            v2.justification = match v2.justification {
                WallJustification::Interior => WallJustification::Exterior,
                WallJustification::Exterior => WallJustification::Interior,
                WallJustification::Center => WallJustification::Center,
            };
            let mut record = ExtendedDataRecord::new(AEC_APPID);
            for v in wall_record(
                &v2.style_id,
                v2.height,
                v2.storey_id,
                &v2.layers,
                &v2.derived_handles,
                v2.justification, v2.phase, v2.hatch_override.as_ref()) {
                record.add_value(v);
            }
            write_aec_record(&mut scene.document, wall_handle, record);
        }
    }

    let mut touched = regenerate_wall_representation_with_rules_and_substitutions(
        scene,
        wall_handle,
        display_rules,
        style_substitutions,
        library_override,
    )?;
    let joined = try_auto_join_nearby_walls(
        scene,
        wall_handle,
        library_override,
        display_rules,
        style_substitutions,
    );
    touched.extend(joined);
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    Ok(touched)
}
