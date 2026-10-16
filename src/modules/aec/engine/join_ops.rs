//! Document-level wall join, auto-join, reverse, and junction rebuild.

#![allow(unused_imports)]
use std::collections::{HashMap, HashSet, VecDeque};
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

pub(crate) use super::join::WALL_JOIN_SNAP_RADIUS;

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
    for (end_index, pt) in [(0usize, axis[0]), (axis.len() - 1, *axis.last().unwrap())] {
        if !wall_end_has_nearby_partner(scene, wall_handle, pt) {
            remove_junction_override(scene, wall_handle, end_index);
        }
    }
}

pub(crate) fn wall_end_has_nearby_partner(scene: &Scene, wall_handle: Handle, pt: DVec3) -> bool {
    let tol = crate::modules::aec::engine::join::JUNCTION_TOLERANCE.max(1e-4);
    for entity in scene.document.entities() {
        let other = entity.common().handle;
        if other == wall_handle || !is_wall_axis_xdata(entity) {
            continue;
        }
        let other_axis = get_wall_vertices(scene, other);
        if other_axis.len() < 2 {
            continue;
        }
        if point_to_polyline_dist_2d(pt, &other_axis) <= tol {
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
    let index = WallCandidateIndex::build(scene);
    index.find_wall_to_auto_join(wall_handle, excluding)
}

/// Axis AABB in XY (`min_x, min_y, max_x, max_y`). Empty axes yield infinities.
pub(crate) fn axis_aabb_2d(axis: &[DVec3]) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for p in axis {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }
    (min_x, min_y, max_x, max_y)
}

/// True when two AABBs overlap after inflating each by `radius`.
pub(crate) fn aabb_overlaps_inflated(
    a: (f64, f64, f64, f64),
    b: (f64, f64, f64, f64),
    radius: f64,
) -> bool {
    a.0 - radius <= b.2 && a.2 + radius >= b.0 && a.1 - radius <= b.3 && a.3 + radius >= b.1
}

/// One-shot snapshot of every wall axis. Built in O(N); neighbor queries then
/// bbox-filter before exact geometric tests so a local cascade never reruns
/// `detect_junctions` / regen scans over the whole document.
pub(crate) struct WallCandidateIndex {
    pub handles: Vec<Handle>,
    axes: Vec<Vec<DVec3>>,
    bulges: Vec<Vec<f64>>,
    aabbs: Vec<(f64, f64, f64, f64)>,
}

impl WallCandidateIndex {
    pub fn build(scene: &Scene) -> Self {
        let handles = all_wall_axis_handles(scene);
        let mut axes = Vec::with_capacity(handles.len());
        let mut bulges = Vec::with_capacity(handles.len());
        let mut aabbs = Vec::with_capacity(handles.len());
        for &h in &handles {
            let axis = get_wall_vertices(scene, h);
            aabbs.push(axis_aabb_2d(&axis));
            axes.push(axis);
            bulges.push(get_wall_bulges(scene, h));
        }
        Self {
            handles,
            axes,
            bulges,
            aabbs,
        }
    }

    pub fn index_of(&self, handle: Handle) -> Option<usize> {
        self.handles.iter().position(|h| *h == handle)
    }

    pub fn axis_of(&self, handle: Handle) -> Option<&[DVec3]> {
        self.index_of(handle).map(|i| self.axes[i].as_slice())
    }

    pub fn bulges_of(&self, handle: Handle) -> Option<&[f64]> {
        self.index_of(handle).map(|i| self.bulges[i].as_slice())
    }

    /// Walls whose AABB is within `radius` of `handle` (excluding itself).
    pub fn bbox_neighbors(&self, handle: Handle, radius: f64) -> Vec<usize> {
        let Some(i) = self.index_of(handle) else {
            return Vec::new();
        };
        let aabb = self.aabbs[i];
        self.handles
            .iter()
            .enumerate()
            .filter(|(j, h)| *j != i && **h != handle && aabb_overlaps_inflated(aabb, self.aabbs[*j], radius))
            .map(|(j, _)| j)
            .collect()
    }

    /// Walls whose AABB overlaps a point inflated by `radius`.
    pub fn bbox_neighbors_for_point(&self, pt: DVec3, radius: f64) -> Vec<usize> {
        let pt_aabb = (pt.x, pt.y, pt.x, pt.y);
        self.handles
            .iter()
            .enumerate()
            .filter(|(j, _)| aabb_overlaps_inflated(pt_aabb, self.aabbs[*j], radius))
            .map(|(j, _)| j)
            .collect()
    }

    /// Peer links plus geometrically connected walls (endpoint snap or actual
    /// L/T/X intersection) at either end and along the span.
    pub fn connected_neighbors(&self, scene: &Scene, handle: Handle, radius: f64) -> Vec<Handle> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for peer in engine::owner_index::peers_of(&scene.document, handle) {
            if seen.insert(peer) {
                out.push(peer);
            }
        }
        let Some(i) = self.index_of(handle) else {
            return out;
        };
        for j in self.bbox_neighbors(handle, radius) {
            let other = self.handles[j];
            if !seen.insert(other) {
                continue;
            }
            if walls_geometrically_connected(
                &self.axes[i],
                &self.bulges[i],
                &self.axes[j],
                &self.bulges[j],
                radius,
            ) {
                out.push(other);
            }
        }
        out
    }

    pub fn find_wall_to_auto_join(&self, wall_handle: Handle, excluding: &[Handle]) -> Option<Handle> {
        let Some(i) = self.index_of(wall_handle) else {
            return None;
        };
        let axis = &self.axes[i];
        let bulges = &self.bulges[i];
        if axis.len() < 2 {
            return None;
        }
        let mut best: Option<(Handle, u8, f64)> = None;
        for j in self.bbox_neighbors(wall_handle, WALL_JOIN_SNAP_RADIUS) {
            let other = self.handles[j];
            if excluding.contains(&other) {
                continue;
            }
            let other_axis = &self.axes[j];
            if other_axis.len() < 2 {
                continue;
            }
            let dist = wall_endpoint_to_axis_dist(axis, other_axis);
            if dist > WALL_JOIN_SNAP_RADIUS {
                continue;
            }
            let other_bulges = &self.bulges[j];
            if join::join_wall_axes_with_bulges(axis, bulges, other_axis, other_bulges).is_err() {
                continue;
            }
            let endpoint_dist = wall_endpoint_to_endpoint_dist(axis, other_axis);
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

    pub fn walls_at_junction(
        &self,
        scene: &Scene,
        axis_handle: Handle,
        end_index: usize,
    ) -> Vec<JunctionParticipant> {
        let mut local_handles = vec![axis_handle];
        for h in self.connected_neighbors(scene, axis_handle, WALL_JOIN_SNAP_RADIUS) {
            if !local_handles.contains(&h) {
                local_handles.push(h);
            }
        }
        let axes: Vec<Vec<DVec3>> = local_handles
            .iter()
            .map(|h| self.axis_of(*h).map(|a| a.to_vec()).unwrap_or_else(|| get_wall_vertices(scene, *h)))
            .collect();
        let bulges: Vec<Vec<f64>> = local_handles
            .iter()
            .map(|h| self.bulges_of(*h).map(|b| b.to_vec()).unwrap_or_else(|| get_wall_bulges(scene, *h)))
            .collect();
        let axis_refs: Vec<&[DVec3]> = axes.iter().map(|a| a.as_slice()).collect();
        let bulge_refs: Vec<&[f64]> = bulges.iter().map(|b| b.as_slice()).collect();
        let tol = join::JUNCTION_TOLERANCE.max(1e-4);
        let junctions = join::detect_junctions_with_bulges(&axis_refs, &bulge_refs, tol);

        let layers_for = |handle: Handle| -> Vec<join::LayerRef> {
            scene
                .document
                .get_entity(handle)
                .and_then(wall_from_entity)
                .map(|w| layer_refs_from_materials(w.layers.iter().map(|l| (l.material.as_str(), l.layer_id))))
                .unwrap_or_default()
        };

        let Some(self_idx) = local_handles.iter().position(|h| *h == axis_handle) else {
            return vec![JunctionParticipant {
                axis_handle,
                end_index,
                layers: layers_for(axis_handle),
                is_through: false,
            }];
        };

        let normalize_end = |raw: usize| -> usize { if raw == 0 { 0 } else { 1 } };

        for junc in &junctions {
            let matches_self = junc.participants.iter().any(|p| {
                p.wall_index == self_idx
                    && matches!(p.role, join::JunctionRole::Endpoint(e) if normalize_end(e) == end_index)
            });
            if !matches_self {
                continue;
            }
            return junc
                .participants
                .iter()
                .filter_map(|p| {
                    let h = local_handles.get(p.wall_index).copied()?;
                    let is_through = matches!(p.role, join::JunctionRole::Through(_));
                    let end_idx = match p.role {
                        join::JunctionRole::Endpoint(raw) => normalize_end(raw),
                        join::JunctionRole::Through(_) => 0,
                    };
                    Some(JunctionParticipant {
                        axis_handle: h,
                        end_index: end_idx,
                        layers: layers_for(h),
                        is_through,
                    })
                })
                .collect();
        }

        vec![JunctionParticipant {
            axis_handle,
            end_index,
            layers: layers_for(axis_handle),
            is_through: false,
        }]
    }

    pub fn through_wall_at_junction(
        &self,
        scene: &Scene,
        axis_handle: Handle,
        end_index: usize,
    ) -> Option<JunctionParticipant> {
        self.walls_at_junction(scene, axis_handle, end_index)
            .into_iter()
            .find(|p| p.is_through)
    }
}

/// True when the walls share an endpoint/span within `radius` or their axes
/// actually join (L/T/X), including interior crossings that sit far from both
/// endpoint clusters.
fn walls_geometrically_connected(
    axis_a: &[DVec3],
    bulges_a: &[f64],
    axis_b: &[DVec3],
    bulges_b: &[f64],
    radius: f64,
) -> bool {
    if axis_a.len() < 2 || axis_b.len() < 2 {
        return false;
    }
    if wall_endpoint_to_axis_dist(axis_a, axis_b) <= radius {
        return true;
    }
    join::join_wall_axes_with_bulges(axis_a, bulges_a, axis_b, bulges_b).is_ok()
}

/// Collect all walls in the local neighborhood of `start_walls` up to depth 2
/// (the starting walls, their direct junction peers / geometric partners, and
/// those partners' opposite-end nodes). Uses BFS bounded by depth and a visited
/// set so rooms and other cycles cannot loop. Geometric partners are discovered
/// from a single [`WallCandidateIndex`] pass (O(N) build, bbox-filtered queries)
/// rather than a full-document scan per wall.
pub(crate) fn collect_local_wall_network(
    scene: &Scene,
    start_walls: &[Handle],
    snap_radius: f64,
) -> Vec<Handle> {
    let index = WallCandidateIndex::build(scene);
    collect_local_wall_network_with_index(scene, &index, start_walls, snap_radius)
        .into_iter()
        .map(|(h, _depth)| h)
        .collect()
}

/// `(handle, bfs_depth)` from `start_walls`. Depth 0 = seeds, 1 = direct
/// partners, 2 = halo (partners' opposite nodes) included in the solve set
/// but not regenerated.
pub(crate) fn collect_local_wall_network_with_index(
    scene: &Scene,
    index: &WallCandidateIndex,
    start_walls: &[Handle],
    snap_radius: f64,
) -> Vec<(Handle, usize)> {
    let mut visited: HashMap<Handle, usize> = HashMap::new();
    let mut queue = VecDeque::new();
    for &h in start_walls {
        if scene.document.get_entity(h).is_some_and(is_wall_axis_xdata) {
            if visited.insert(h, 0).is_none() {
                queue.push_back((h, 0usize));
            }
        }
    }

    const MAX_CASCADE_DEPTH: usize = 2;

    while let Some((current, depth)) = queue.pop_front() {
        if depth >= MAX_CASCADE_DEPTH {
            continue;
        }
        for peer in index.connected_neighbors(scene, current, snap_radius) {
            if scene.document.get_entity(peer).is_some_and(is_wall_axis_xdata)
                && !visited.contains_key(&peer)
            {
                visited.insert(peer, depth + 1);
                queue.push_back((peer, depth + 1));
            }
        }
    }

    let mut result: Vec<(Handle, usize)> = visited.into_iter().collect();
    result.sort_by_key(|(h, _)| h.value());
    result
}

/// Collect the bounded neighborhood of `seed_walls` and resolve it with
/// [`junction_solver::solve_network`]. `frozen_ends` (handle + vertex index)
/// are never rewritten — used for a user snap/anchor or a just-forced L —
/// and `trim_axes` is false for override-only rebuilds that must not move
/// geometry.
pub(crate) fn cascade_solve_and_rebuild_wall_network(
    scene: &mut Scene,
    seed_walls: &[Handle],
    frozen_ends: &[(Handle, usize)],
    trim_axes: bool,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Result<Vec<Handle>, JoinError> {
    if seed_walls.is_empty() {
        return Ok(Vec::new());
    }
    let index = WallCandidateIndex::build(scene);
    let members =
        collect_local_wall_network_with_index(scene, &index, seed_walls, WALL_JOIN_SNAP_RADIUS);
    let depth_map: HashMap<Handle, usize> = members.iter().copied().collect();
    let cluster: Vec<Handle> = members.iter().map(|(h, _)| *h).collect();
    // Depth 0–1 are actually rewritten; depth 2 is halo so opposite-end
    // miters of depth-1 walls stay complete in solve_network.
    let regen: Vec<Handle> = members
        .iter()
        .filter(|(h, d)| *d <= 1 || seed_walls.contains(h))
        .map(|(h, _)| *h)
        .collect();
    cascade_solve_cluster(
        scene,
        &cluster,
        &depth_map,
        seed_walls,
        &regen,
        frozen_ends,
        trim_axes,
        library_override,
        display_rules,
        style_substitutions,
    )
}

fn cascade_solve_cluster(
    scene: &mut Scene,
    cluster: &[Handle],
    depth_map: &HashMap<Handle, usize>,
    seed_walls: &[Handle],
    regen_handles: &[Handle],
    frozen_ends: &[(Handle, usize)],
    trim_axes: bool,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Result<Vec<Handle>, JoinError> {
    if cluster.is_empty() {
        return Ok(Vec::new());
    }
    let inputs: Vec<WallJoinInput> = cluster
        .iter()
        .map(|h| wall_join_input_from_scene(scene, *h))
        .collect();
    if inputs.iter().all(|w| w.axis.len() < 2) {
        return Ok(Vec::new());
    }

    let cluster_tol = if trim_axes {
        WALL_JOIN_SNAP_RADIUS
    } else {
        join::JUNCTION_TOLERANCE.max(1e-4)
    };
    let axes: Vec<&[DVec3]> = inputs.iter().map(|w| w.axis.as_slice()).collect();
    let bulges: Vec<&[f64]> = inputs.iter().map(|w| w.bulges.as_slice()).collect();
    let detected = join::detect_junctions_with_bulges(&axes, &bulges, cluster_tol);

    let strict_tol = join::JUNCTION_TOLERANCE.max(1e-4);
    let mut actionable_junctions: Vec<join::Junction> = Vec::new();
    let mut triggering_junction_indices: HashSet<usize> = HashSet::new();

    for mut junc in detected {
        let part_handles: Vec<Handle> = junc
            .participants
            .iter()
            .filter_map(|p| cluster.get(p.wall_index).copied())
            .collect();
        if part_handles.len() < 2 {
            continue;
        }

        let is_triggering = if trim_axes && !seed_walls.is_empty() {
            if seed_walls.len() == 1 {
                part_handles.contains(&seed_walls[0])
            } else if seed_walls.len() == 2 {
                part_handles.contains(&seed_walls[0]) && part_handles.contains(&seed_walls[1])
            } else {
                part_handles.iter().filter(|h| seed_walls.contains(h)).count() >= 2
            }
        } else {
            false
        };

        if is_triggering {
            // Requirement 1: Explicit pair/manual trigger must accept only seed walls
            // plus extra participants that are ALREADY peer-linked or strictly coincident.
            // Any unlinked, non-coincident third wall sitting within snap radius is dropped.
            junc.participants.retain(|p| {
                let Some(&h) = cluster.get(p.wall_index) else {
                    return false;
                };
                if seed_walls.contains(&h) {
                    return true;
                }
                let peers = engine::owner_index::peers_of(&scene.document, h);
                if part_handles.iter().any(|&other| seed_walls.contains(&other) && peers.contains(&other)) {
                    return true;
                }
                match p.role {
                    join::JunctionRole::Endpoint(e) => {
                        let pt = inputs[p.wall_index].axis.get(e);
                        pt.is_some_and(|pt| (pt.x - junc.point.x).hypot(pt.y - junc.point.y) <= strict_tol)
                    }
                    join::JunctionRole::Through(seg) => {
                        let axis = &inputs[p.wall_index].axis;
                        if seg + 1 < axis.len() {
                            let d = point_to_segment_dist_2d(junc.point, axis[seg], axis[seg + 1]);
                            d <= strict_tol
                        } else {
                            false
                        }
                    }
                }
            });

            if junc.participants.len() < 2 {
                continue;
            }

            // Recompute junc.point from the retained participants
            let mut sum_x = 0.0;
            let mut sum_y = 0.0;
            let mut count = 0.0;
            for p in &junc.participants {
                match p.role {
                    join::JunctionRole::Endpoint(e) => {
                        if let Some(pt) = inputs[p.wall_index].axis.get(e) {
                            sum_x += pt.x;
                            sum_y += pt.y;
                            count += 1.0;
                        }
                    }
                    join::JunctionRole::Through(_) => {}
                }
            }
            if count > 0.0 {
                junc.point = DVec3::new(sum_x / count, sum_y / count, 0.0);
            }

            let idx = actionable_junctions.len();
            actionable_junctions.push(junc);
            triggering_junction_indices.insert(idx);
        } else {
            // Non-trigger junction: if peer links exist, retain only peer-linked participants
            // (dropping unlinked nearby intruder walls), recompute junction point, and verify
            // that all retained participants are strictly coincident.
            let has_any_peer_links = part_handles.iter().enumerate().any(|(i, &h_i)| {
                let peers = engine::owner_index::peers_of(&scene.document, h_i);
                part_handles.iter().skip(i + 1).any(|h_j| peers.contains(h_j))
            });
            if has_any_peer_links {
                junc.participants.retain(|p| {
                    let Some(&h) = cluster.get(p.wall_index) else {
                        return false;
                    };
                    let peers = engine::owner_index::peers_of(&scene.document, h);
                    part_handles.iter().any(|&other| other != h && peers.contains(&other))
                });
            }

            if junc.participants.len() < 2 {
                continue;
            }

            // Recompute junc.point from the retained participants
            let mut sum_x = 0.0;
            let mut sum_y = 0.0;
            let mut count = 0.0;
            for p in &junc.participants {
                match p.role {
                    join::JunctionRole::Endpoint(e) => {
                        if let Some(pt) = inputs[p.wall_index].axis.get(e) {
                            sum_x += pt.x;
                            sum_y += pt.y;
                            count += 1.0;
                        }
                    }
                    join::JunctionRole::Through(_) => {}
                }
            }
            if count > 0.0 {
                junc.point = DVec3::new(sum_x / count, sum_y / count, 0.0);
            }

            let all_endpoints_strictly_coincident = junc.participants.iter().all(|p| {
                match p.role {
                    join::JunctionRole::Endpoint(e) => {
                        let pt = inputs[p.wall_index].axis.get(e);
                        pt.is_some_and(|pt| (pt.x - junc.point.x).hypot(pt.y - junc.point.y) <= strict_tol)
                    }
                    join::JunctionRole::Through(seg) => {
                        let axis = &inputs[p.wall_index].axis;
                        if seg + 1 < axis.len() {
                            let d = point_to_segment_dist_2d(junc.point, axis[seg], axis[seg + 1]);
                            d <= strict_tol
                        } else {
                            false
                        }
                    }
                }
            });

            if all_endpoints_strictly_coincident {
                actionable_junctions.push(junc);
            }
        }
    }

    let frozen: HashSet<(Handle, usize)> = frozen_ends.iter().copied().collect();
    let mut allowed_trim_indices: HashSet<(usize, usize)> = HashSet::new();
    let mut allowed_trim_ends: HashSet<(Handle, usize)> = HashSet::new();
    if trim_axes {
        for &j_idx in &triggering_junction_indices {
            if let Some(junc) = actionable_junctions.get(j_idx) {
                for p in &junc.participants {
                    if let join::JunctionRole::Endpoint(e) = p.role {
                        if let Some(&h) = cluster.get(p.wall_index) {
                            let depth = depth_map.get(&h).copied().unwrap_or(2);
                            if (depth <= 1 || seed_walls.contains(&h)) && !frozen.contains(&(h, e)) {
                                allowed_trim_indices.insert((p.wall_index, e));
                                allowed_trim_ends.insert((h, e));
                            }
                        }
                    }
                }
            }
        }
    }

    let net = junction_solver::solve_network_with_junctions(
        &inputs,
        actionable_junctions.clone(),
        Some(&allowed_trim_indices),
    );

    let mut axis_changed_handles: HashSet<Handle> = HashSet::new();

    if trim_axes {
        for (i, &h) in cluster.iter().enumerate() {
            let depth = depth_map.get(&h).copied().unwrap_or(2);
            if depth > 1 && !seed_walls.contains(&h) {
                // Read-only halo: never modify axis
                continue;
            }
            let Some(new_axis) = net.trimmed_axes.get(i) else {
                continue;
            };
            if new_axis.len() < 2 {
                continue;
            }
            let mut axis = inputs[i].axis.clone();
            if axis.len() != new_axis.len() {
                continue;
            }
            let mut changed = false;
            if allowed_trim_ends.contains(&(h, 0)) && !frozen.contains(&(h, 0)) && axis[0].distance(new_axis[0]) > 1e-9 {
                axis[0] = new_axis[0];
                changed = true;
            }
            let last = axis.len() - 1;
            if allowed_trim_ends.contains(&(h, last)) && !frozen.contains(&(h, last)) && axis[last].distance(new_axis[last]) > 1e-9 {
                axis[last] = new_axis[last];
                changed = true;
            }
            if changed {
                update_wall_vertices(scene, h, &axis);
                axis_changed_handles.insert(h);
            }
        }
    }

    if trim_axes {
        for &j_idx in &triggering_junction_indices {
            if let Some(sj) = net.junctions.get(j_idx) {
                let parts: Vec<Handle> = sj
                    .junction
                    .participants
                    .iter()
                    .filter_map(|p| cluster.get(p.wall_index).copied())
                    .collect();
                for i in 0..parts.len() {
                    for j in (i + 1)..parts.len() {
                        let d_i = depth_map.get(&parts[i]).copied().unwrap_or(2);
                        let d_j = depth_map.get(&parts[j]).copied().unwrap_or(2);
                        // Only link if neither is depth 2 (halo) and at least one is depth 0
                        if d_i <= 1 && d_j <= 1 && (d_i == 0 || d_j == 0) {
                            engine::owner_index::link_peers(&mut scene.document, parts[i], parts[j]);
                        }
                    }
                }
            }
        }
    }

    let active_walls: HashSet<Handle> = net
        .junctions
        .iter()
        .flat_map(|sj| sj.junction.participants.iter().filter_map(|p| cluster.get(p.wall_index).copied()))
        .collect();

    let mut regen_set: HashSet<Handle> = HashSet::new();
    // 1. All seed walls are regenerated
    for &h in seed_walls {
        regen_set.insert(h);
    }
    // 2. Any wall whose axis was changed is regenerated
    regen_set.extend(axis_changed_handles);
    // 3. Depth <= 1 walls are regenerated if they participate in an actionable junction
    for &h in regen_handles {
        let depth = depth_map.get(&h).copied().unwrap_or(2);
        if depth <= 1 && active_walls.contains(&h) {
            regen_set.insert(h);
        }
    }
    // 4. Halo (depth > 1) walls are strictly never regenerated unless they were explicit seed walls
    for (&h, &depth) in depth_map {
        if depth > 1 && !seed_walls.contains(&h) {
            regen_set.remove(&h);
        }
    }

    let mut touched = Vec::new();
    for (i, &h) in cluster.iter().enumerate() {
        if !regen_set.contains(&h) {
            continue;
        }
        let empty = Vec::new();
        let fps = net.footprints.get(i).unwrap_or(&empty);
        match regenerate_wall_representation_with_complete_network_miters(
            scene,
            h,
            None,
            fps,
            display_rules,
            style_substitutions,
            library_override,
        ) {
            Ok(t) => touched.extend(t),
            Err(_) => touched.push(h),
        }
    }
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    Ok(touched)
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

    // Symmetric peer unlinking for walls that are no longer nearby.
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

    let multi_touched = try_join_multi_wall_junctions(
        scene,
        wall_handle,
        library_override,
        display_rules,
        style_substitutions,
    );
    if !multi_touched.is_empty() {
        touched.extend(multi_touched);
        touched.sort_by_key(|h| h.value());
        touched.dedup();
        return touched;
    }

    let mut joined: Vec<Handle> = Vec::new();
    while let Some(partner) = find_wall_to_auto_join(scene, wall_handle, &joined) {
        if let Ok((_kind, t)) = join_two_walls_in_document(
            scene,
            wall_handle,
            partner,
            library_override,
            display_rules,
            style_substitutions,
        ) {
            touched.extend(t);
            joined.push(partner);
            if joined.len() >= 2 {
                break;
            }
        } else {
            joined.push(partner);
        }
    }

    if joined.is_empty() && touched.is_empty() {
        return Vec::new();
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
    if let Some(prev) = last_committed {
        if prev != wall_handle {
            let axis = get_wall_vertices(scene, wall_handle);
            let prev_axis = get_wall_vertices(scene, prev);
            if wall_endpoint_to_axis_dist(&axis, &prev_axis) <= WALL_JOIN_SNAP_RADIUS {
                let _ = join_two_walls_in_document(scene, wall_handle, prev, library, None, None);
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
    let handles = collect_local_wall_network(scene, &[wall_handle], WALL_JOIN_SNAP_RADIUS);
    if handles.len() < 2 {
        return Vec::new();
    }
    let inputs: Vec<WallJoinInput> = handles
        .iter()
        .map(|h| wall_join_input_from_scene(scene, *h))
        .collect();
    let solved = junction_solver::solve(&inputs, WALL_JOIN_SNAP_RADIUS);

    let self_idx = handles.iter().position(|h| *h == wall_handle);
    let Some(self_idx) = self_idx else {
        return Vec::new();
    };

    let self_peers = engine::owner_index::peers_of(&scene.document, wall_handle);
    let strict_tol = join::JUNCTION_TOLERANCE.max(1e-4);

    let mut touched = Vec::new();
    for sj in solved.into_iter().filter(|s| s.junction.is_multi_wall() || s.kind == junction_solver::SolvedJoinKind::X) {
        if !sj
            .junction
            .participants
            .iter()
            .any(|p| p.wall_index == self_idx)
        {
            continue;
        }

        if sj.kind == junction_solver::SolvedJoinKind::X {
            if junction_solver::x_junction_is_unsupported(&inputs, &sj.junction) {
                continue;
            }
            let part_handles: Vec<Handle> = sj
                .junction
                .participants
                .iter()
                .map(|p| handles[p.wall_index])
                .collect();
            if let Ok(t) = join_junction_in_document(
                scene,
                &part_handles,
                Some(sj.junction.point),
                library_override,
                display_rules,
                style_substitutions,
            ) {
                touched.extend(t);
            }
            continue;
        }

        let other_part_indices: Vec<usize> = sj
            .junction
            .participants
            .iter()
            .filter_map(|p| if p.wall_index != self_idx { Some(p.wall_index) } else { None })
            .collect();

        let is_existing_peer = other_part_indices
            .iter()
            .any(|&oi| self_peers.contains(&handles[oi]));

        if is_existing_peer {
            // Case A: This wall was already part of this junction and its vertex moved (e.g. grip edit).
            // Retain self plus any other participants that are existing peers or strictly coincident.
            let mut valid_part_indices = vec![self_idx];
            for &oi in &other_part_indices {
                let h_oi = handles[oi];
                let is_peer = self_peers.contains(&h_oi);
                let is_coincident = sj.junction.participants.iter().find(|p| p.wall_index == oi).map_or(false, |p| {
                    match p.role {
                        join::JunctionRole::Endpoint(e) => {
                            inputs[oi].axis.get(e).map_or(false, |pt| (pt.x - sj.junction.point.x).hypot(pt.y - sj.junction.point.y) <= strict_tol)
                        }
                        join::JunctionRole::Through(seg) => {
                            let axis = &inputs[oi].axis;
                            if seg + 1 < axis.len() {
                                point_to_segment_dist_2d(sj.junction.point, axis[seg], axis[seg + 1]) <= strict_tol
                            } else {
                                false
                            }
                        }
                    }
                });
                if is_peer || is_coincident {
                    valid_part_indices.push(oi);
                }
            }
            if valid_part_indices.len() < 2 {
                continue;
            }
            let part_handles: Vec<Handle> = valid_part_indices.iter().map(|&i| handles[i]).collect();
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
        } else {
            // Case B: This wall is newly placed or unlinked.
            // Find existing node among other participants: other participants that are peer-linked
            // to each other or strictly coincident (<= strict_tol) with each other.
            let mut existing_node_indices: Vec<usize> = Vec::new();
            for &oi in &other_part_indices {
                let h_oi = handles[oi];
                let oi_peers = engine::owner_index::peers_of(&scene.document, h_oi);
                let linked_to_other = other_part_indices
                    .iter()
                    .any(|&oj| oj != oi && oi_peers.contains(&handles[oj]));
                if linked_to_other {
                    existing_node_indices.push(oi);
                }
            }
            if existing_node_indices.len() < 2 {
                existing_node_indices.clear();
                for i in 0..other_part_indices.len() {
                    let oi = other_part_indices[i];
                    let pt_i = sj.junction.participants.iter().find(|p| p.wall_index == oi).and_then(|p| match p.role {
                        join::JunctionRole::Endpoint(e) => inputs[oi].axis.get(e).copied(),
                        join::JunctionRole::Through(_) => None,
                    });
                    if let Some(pi) = pt_i {
                        let count = other_part_indices.iter().filter(|&&oj| {
                            sj.junction.participants.iter().find(|p| p.wall_index == oj).and_then(|p| match p.role {
                                join::JunctionRole::Endpoint(e) => inputs[oj].axis.get(e).copied(),
                                join::JunctionRole::Through(_) => None,
                            }).map_or(false, |pj| (pi.x - pj.x).hypot(pi.y - pj.y) <= strict_tol)
                        }).count();
                        if count >= 2 {
                            existing_node_indices.push(oi);
                        }
                    }
                }
            }

            if existing_node_indices.len() < 2 {
                // Not an existing multi-wall node; pairwise auto-join handles it.
                continue;
            }

            // Compute anchor point from existing node participants
            let mut sum_x = 0.0;
            let mut sum_y = 0.0;
            let mut count = 0.0;
            for &oi in &existing_node_indices {
                if let Some(p) = sj.junction.participants.iter().find(|p| p.wall_index == oi) {
                    match p.role {
                        join::JunctionRole::Endpoint(e) => {
                            if let Some(pt) = inputs[oi].axis.get(e) {
                                sum_x += pt.x;
                                sum_y += pt.y;
                                count += 1.0;
                            }
                        }
                        join::JunctionRole::Through(_) => {}
                    }
                }
            }
            if count == 0.0 {
                continue;
            }
            let anchor = DVec3::new(sum_x / count, sum_y / count, 0.0);

            // Accepted participants: self + existing node participants
            let mut part_handles = vec![wall_handle];
            for &oi in &existing_node_indices {
                part_handles.push(handles[oi]);
            }
            if let Ok(t) = join_junction_in_document(
                scene,
                &part_handles,
                Some(anchor),
                library_override,
                display_rules,
                style_substitutions,
            ) {
                touched.extend(t);
            }
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
    let index = WallCandidateIndex::build(scene);
    index.walls_at_junction(scene, axis_handle, end_index)
}

/// Through-wall participant at `(axis_handle, end_index)`, if this node is a T.
pub fn through_wall_at_junction(
    scene: &Scene,
    axis_handle: Handle,
    end_index: usize,
) -> Option<JunctionParticipant> {
    let index = WallCandidateIndex::build(scene);
    index.through_wall_at_junction(scene, axis_handle, end_index)
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
    let index = WallCandidateIndex::build(scene);
    let junc_participants = index.walls_at_junction(scene, axis_handle, end_index);
    let is_through = index.through_wall_at_junction(scene, axis_handle, end_index).is_some();

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
            if is_through {
                stem.layer_gaps.clear();
            }
            if stem.is_empty() {
                remove_junction_override(scene, axis_handle, end_index);
            } else {
                write_junction_override(scene, axis_handle, end_index, &stem);
            }
            // Store the inverted pair on every other endpoint so a miter
            // picked on one wall also shortens the partner layer.
            for p in &junc_participants {
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
    let mut direct_participants = vec![axis_handle];
    for p in &junc_participants {
        if !direct_participants.contains(&p.axis_handle) {
            direct_participants.push(p.axis_handle);
        }
    }

    // Override-only: do not move axes. Combined end0/end1/span footprints
    // come from solve_network on the bounded cluster.
    let mut touched = cascade_solve_and_rebuild_wall_network(
        scene,
        &direct_participants,
        &[],
        false,
        library_override,
        display_rules,
        style_substitutions,
    )
    .unwrap_or_else(|_| direct_participants.clone());

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
    let mut solved_j = if let Some(snap_pt) = snap_point {
        let mut matching: Vec<_> = solved
            .into_iter()
            .filter(|s| {
                (s.junction.point.x - snap_pt.x).hypot(s.junction.point.y - snap_pt.y) <= tol.max(0.1)
            })
            .collect();
        matching.sort_by(|a, b| {
            let da = (a.junction.point.x - snap_pt.x).hypot(a.junction.point.y - snap_pt.y);
            let db = (b.junction.point.x - snap_pt.x).hypot(b.junction.point.y - snap_pt.y);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
        matching.into_iter().next().ok_or(JoinError::NoIntersection)?
    } else {
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
        if let Some(j) = multi_wall.into_iter().next() {
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
        }
    };
    if junction_solver::x_junction_is_unsupported(&inputs, &solved_j.junction) {
        return Err(JoinError::Unsupported);
    }
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

    // Symmetric pairwise peer links among all junction participants.
    for i in 0..handles.len() {
        for j in (i + 1)..handles.len() {
            engine::owner_index::link_peers(&mut scene.document, handles[i], handles[j]);
        }
    }

    // Freeze the ends we just snapped (user anchor / this junction) so the
    // cascade solver composes opposite-end + span footprints without moving
    // this node.
    let frozen: Vec<(Handle, usize)> = junc
        .participants
        .iter()
        .filter_map(|p| match p.role {
            join::JunctionRole::Endpoint(e) => Some((handles[p.wall_index], e)),
            join::JunctionRole::Through(_) => None,
        })
        .collect();
    let mut touched = cascade_solve_and_rebuild_wall_network(
        scene,
        handles,
        &frozen,
        true,
        library_override,
        display_rules,
        style_substitutions,
    )
    .unwrap_or_else(|_| handles.to_vec());

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
    // A true interior–interior crossing involving an arc must not go through
    // the pairwise L/T trim (the chord-based end classification would cut or
    // snap the arc): route pure X pairs straight to the X resolver.
    let curved = |w: &WallJoinInput| w.bulges.iter().any(|b| b.abs() > 1e-9);
    if !force_l && (curved(&input_a) || curved(&input_b)) {
        let tol = join::JUNCTION_TOLERANCE.max(1e-4);
        let probe = [input_a.clone(), input_b.clone()];
        let found = junction_solver::solve(&probe, tol);
        if !found.is_empty()
            && found
                .iter()
                .all(|s| s.kind == junction_solver::SolvedJoinKind::X && s.junction.participants.len() == 2)
        {
            if let Some(res) = resolve_pair_x_crossings(
                scene,
                h_a,
                h_b,
                probe,
                library_override,
                display_rules,
                style_substitutions,
            ) {
                return res;
            }
        }
    }
    let joined = junction_solver::solve_pair(&input_a, &input_b, force_l);
    match joined {
        Ok(pair) => {
            let new_a = pair.axis_a;
            let new_b = pair.axis_b;
            let kind = pair.kind;
            let end_a = pair.end_a;
            let end_b = pair.end_b;
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
                    let tol = join::JUNCTION_TOLERANCE.max(1e-4);
                    let index = WallCandidateIndex::build(scene);
                    for j in index.bbox_neighbors_for_point(pt, tol) {
                        let h = index.handles[j];
                        if h == h_a || h == h_b {
                            continue;
                        }
                        let axis = &index.axes[j];
                        if axis.len() < 2 {
                            continue;
                        }
                        let end_hit = [axis[0], *axis.last().unwrap()]
                            .iter()
                            .any(|e| (e.x - pt.x).hypot(e.y - pt.y) <= tol);
                        let through_hit = (0..axis.len() - 1).any(|i| {
                            let d = point_to_segment_dist_2d(pt, axis[i], axis[i + 1]);
                            d <= tol
                                && (axis[i].x - pt.x).hypot(axis[i].y - pt.y) > join::END_MID_TOLERANCE
                                && (axis[i + 1].x - pt.x).hypot(axis[i + 1].y - pt.y) > join::END_MID_TOLERANCE
                        });
                        if end_hit || through_hit {
                            participants.push(h);
                        }
                    }
                    if participants.len() > 2 {
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
            }

            // Symmetric peer links for the successful pairwise join.
            engine::owner_index::link_peers(&mut scene.document, h_a, h_b);

            let refs_a = layer_refs_from_materials(
                input_a.layers.iter().map(|l| (l.material.as_str(), l.layer_id)),
            );
            let refs_b = layer_refs_from_materials(
                input_b.layers.iter().map(|l| (l.material.as_str(), l.layer_id)),
            );

            if let Some(ea) = end_a {
                if let Some(ov) = read_junction_override(scene, h_a, ea) {
                    let _ = validate_and_persist_junction_override(
                        scene,
                        h_a,
                        ea,
                        ov,
                        &refs_a,
                        &refs_b,
                    );
                }
            }

            if let Some(eb) = end_b {
                if let Some(ov) = read_junction_override(scene, h_b, eb) {
                    let _ = validate_and_persist_junction_override(
                        scene,
                        h_b,
                        eb,
                        ov,
                        &refs_b,
                        &refs_a,
                    );
                }
            }

            let mut frozen = Vec::new();
            if let Some(ea) = end_a {
                frozen.push((h_a, ea));
            }
            if let Some(eb) = end_b {
                frozen.push((h_b, eb));
            }
            // Force-L must not let solve_network re-open the overhang as T.
            let trim_axes = !force_l;
            let mut touched = cascade_solve_and_rebuild_wall_network(
                scene,
                &[h_a, h_b],
                &frozen,
                trim_axes,
                library_override,
                display_rules,
                style_substitutions,
            )
            .unwrap_or_else(|_| vec![h_a, h_b]);

            touched.sort_by_key(|h| h.value());
            touched.dedup();
            Ok((kind, touched))
        }
        Err(e) => {
            if !force_l {
                if let Some(res) = resolve_pair_x_crossings(
                    scene,
                    h_a,
                    h_b,
                    [input_a, input_b],
                    library_override,
                    display_rules,
                    style_substitutions,
                ) {
                    return res;
                }
            }
            Err(e)
        }
    }
}

/// Resolve every interior–interior (X) crossing of the pair `h_a` / `h_b`.
///
/// `None` when the pair has no X crossing. `Some(Err(Unsupported))` when a
/// crossing cannot be resolved without distorting a wall (any curved participant);
/// nothing is modified in that case.
fn resolve_pair_x_crossings(
    scene: &mut Scene,
    h_a: Handle,
    h_b: Handle,
    inputs: [WallJoinInput; 2],
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Option<Result<(JoinKind, Vec<Handle>), JoinError>> {
    let tol = join::JUNCTION_TOLERANCE.max(1e-4);
    let inputs = inputs.to_vec();
    let solved = junction_solver::solve(&inputs, tol);
    let mut x_crossings: Vec<_> = solved
        .into_iter()
        .filter(|s| s.kind == junction_solver::SolvedJoinKind::X && s.junction.participants.len() == 2)
        .collect();
    if x_crossings.is_empty() {
        return None;
    }
    if x_crossings
        .iter()
        .any(|s| junction_solver::x_junction_is_unsupported(&inputs, &s.junction))
    {
        return Some(Err(JoinError::Unsupported));
    }
    // Deterministically sort and resolve all 2-wall X-crossings of the pair
    x_crossings.sort_by(|s1, s2| {
        s1.junction
            .point
            .x
            .partial_cmp(&s2.junction.point.x)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                s1.junction
                    .point
                    .y
                    .partial_cmp(&s2.junction.point.y)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    let mut all_touched = Vec::new();
    for sj in &x_crossings {
        if let Ok(touched) = join_junction_in_document(
            scene,
            &[h_a, h_b],
            Some(sj.junction.point),
            library_override,
            display_rules,
            style_substitutions,
        ) {
            all_touched.extend(touched);
        }
    }
    if all_touched.is_empty() {
        return None;
    }
    all_touched.sort_by_key(|h| h.value());
    all_touched.dedup();
    Some(Ok((JoinKind::X, all_touched)))
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

    let old_bulges = get_wall_bulges(scene, wall_handle);
    axis.reverse();
    update_wall_vertices(scene, wall_handle, &axis);

    let rev_bulges = engine::arc::reverse_bulges(&old_bulges, axis.len());
    if rev_bulges.iter().any(|b| b.abs() > 1e-12) {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity_mut(wall_handle) {
            for (v, &b) in pl.vertices.iter_mut().zip(rev_bulges.iter()) {
                v.bulge = b;
            }
        }
    }

    // Swap overrides on start (0) and end (axis.len() - 1) (keep span override intact).
    let last_idx = axis.len() - 1;
    let ov0 = read_junction_override(scene, wall_handle, 0);
    let ov_last = read_junction_override(scene, wall_handle, last_idx);
    remove_junction_override(scene, wall_handle, 0);
    remove_junction_override(scene, wall_handle, last_idx);
    if let Some(ov) = ov0 {
        write_junction_override(scene, wall_handle, last_idx, &ov);
    }
    if let Some(ov) = ov_last {
        write_junction_override(scene, wall_handle, 0, &ov);
    }

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
            record.values = wall_record_for_wall(&v2);
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
