//! Wall axis package resolve, derived children, and pick targets.

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
use super::wall_regen::*;
use super::join_ops::*;
use super::storey_xdata::*;
use super::opening_xdata::*;

pub(crate) fn wall_rep_owner_from_entity(entity: &EntityType) -> Option<Handle> {
    for record in entity.common().extended_data.records() {
        if record.application_name != AEC_APPID {
            continue;
        }
        let kind = match record.values.first() {
            Some(XDataValue::String(s)) => s.as_str(),
            _ => continue,
        };
        if kind != "WALL_REP" && kind != "WALL_DERIVED" {
            continue;
        }
        return record.values.get(1).and_then(aec_value_as_handle);
    }
    None
}

pub(crate) fn collect_wall_display_children(scene: &Scene, owner: Handle) -> Vec<Handle> {
    let mut out = Vec::new();
    if let Some(entity) = scene.document.get_entity(owner) {
        if let Some(wall) = wall_from_entity(entity) {
            out.extend(wall.derived_handles);
        }
    }
    for h in engine::owner_index::children_of(&scene.document, owner) {
        if !out.contains(&h) {
            out.push(h);
        }
    }
    for entity in scene.document.entities() {
        let handle = entity.common().handle;
        if handle == owner {
            continue;
        }
        if wall_rep_owner_from_entity(entity) == Some(owner) && !out.contains(&handle) {
            out.push(handle);
        }
    }
    out
}

/// Write `pl` into an existing contour handle when possible so the resident
/// wire cache retessellates the same outline instead of leaving a ghost.
pub(crate) fn reuse_or_add_wall_contour(
    scene: &mut Scene,
    reusable: &mut Vec<Handle>,
    pl: LwPolyline,
) -> Handle {
    while let Some(handle) = reusable.pop() {
        if scene.document.get_entity(handle).is_none() {
            continue;
        }
        if let Some(EntityType::LwPolyline(dst)) = scene.document.get_entity_mut(handle) {
            dst.vertices = pl.vertices;
            dst.is_closed = pl.is_closed;
            dst.elevation = pl.elevation;
        } else {
            scene.erase_entities(&[handle]);
            continue;
        }
        scene.bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
        return handle;
    }
    scene.add_entity(EntityType::LwPolyline(pl))
}

pub(crate) fn clicked_sample_xy(scene: &Scene, handle: Handle) -> Vec<(f64, f64)> {
    match scene.document.get_entity(handle) {
        Some(EntityType::LwPolyline(pl)) => {
            if pl.vertices.len() < 3 {
                return Vec::new();
            }
            pl.vertices
                .iter()
                .map(|v| (v.location.x, v.location.y))
                .collect()
        }
        Some(EntityType::Hatch(_)) => {
            let Some(model) = scene.hatches.get(&handle) else {
                return Vec::new();
            };
            let (ox, oy) = (model.world_origin[0], model.world_origin[1]);
            model
                .boundary
                .iter()
                .filter_map(|&[x, y]| {
                    if x.is_finite() && y.is_finite() {
                        Some((x as f64 + ox, y as f64 + oy))
                    } else {
                        None
                    }
                })
                .collect()
        }
        Some(EntityType::Solid3D(solid)) => {
            let mut samples = Vec::new();
            if let Some(mesh) = scene.meshes.get(&handle) {
                let [xmin, ymin, xmax, ymax] = mesh.world_aabb;
                if xmin.is_finite() && ymin.is_finite() && xmax.is_finite() && ymax.is_finite() {
                    samples.extend([
                        (xmin as f64, ymin as f64),
                        (xmax as f64, ymin as f64),
                        (xmax as f64, ymax as f64),
                        (xmin as f64, ymax as f64),
                        (((xmin + xmax) as f64) * 0.5, ((ymin + ymax) as f64) * 0.5),
                    ]);
                }
                if samples.len() < 3 {
                    if let Some(lod) = mesh.lods.first() {
                        samples.extend(lod.verts.iter().take(32).filter_map(|v| {
                            (v[0].is_finite() && v[1].is_finite()).then_some((v[0] as f64, v[1] as f64))
                        }));
                    }
                }
            }
            if samples.len() < 3 {
                for wire in &solid.wires {
                    for p in &wire.points {
                        if p.x.is_finite() && p.y.is_finite() {
                            samples.push((p.x, p.y));
                        }
                    }
                }
            }
            samples
        }
        _ => Vec::new(),
    }
}

pub(crate) fn resolve_wall_package_by_geometry(scene: &Scene, clicked: Handle) -> Option<Handle> {
    let samples = clicked_sample_xy(scene, clicked);
    if samples.len() < 3 {
        return None;
    }
    let mut best: Option<(Handle, f64)> = None;
    for entity in scene.document.entities() {
        let Some(wall) = wall_from_entity(entity) else {
            continue;
        };
        let owner = entity.common().handle;
        if owner == clicked {
            continue;
        }
        let axis = get_wall_vertices(scene, owner);
        if axis.len() < 2 {
            continue;
        }
        let max_ok = wall.total_thickness().max(1e-6) * 1.25 + 1e-4;
        let mut worst = 0.0_f64;
        let mut all_near = true;
        for &(x, y) in &samples {
            let p = DVec3::new(x, y, 0.0);
            let mut dmin = f64::MAX;
            for w in axis.windows(2) {
                dmin = dmin.min(point_to_segment_dist_2d(p, w[0], w[1]));
            }
            if dmin > max_ok {
                all_near = false;
                break;
            }
            worst = worst.max(dmin);
        }
        if !all_near {
            continue;
        }
        match best {
            None => best = Some((owner, worst)),
            Some((_, d)) if worst < d => best = Some((owner, worst)),
            _ => {}
        }
    }
    best.map(|(h, _)| h)
}

pub fn resolve_wall_package(scene: &Scene, clicked: Handle) -> Handle {
    let Some(entity) = scene.document.get_entity(clicked) else {
        return clicked;
    };
    if wall_from_entity(entity).is_some() {
        return clicked;
    }
    if let Some(axis) = wall_rep_owner_from_entity(entity) {
        if scene.document.get_entity(axis).is_some() {
            return axis;
        }
    }
    // Fallback: the clicked entity is listed on a wall's derived_handles /
    // CHILD_HANDLES even if child XDATA was stripped (DWG round-trip / merge).
    for entity in scene.document.entities() {
        let owner = entity.common().handle;
        if let Some(wall) = wall_from_entity(entity) {
            if wall.derived_handles.iter().any(|h| *h == clicked) {
                return owner;
            }
        }
        if engine::owner_index::children_of(&scene.document, owner)
            .iter()
            .any(|h| *h == clicked)
            && wall_from_entity(entity).is_some()
        {
            return owner;
        }
    }
    if let Some(owner) = resolve_wall_package_by_geometry(scene, clicked) {
        return owner;
    }
    clicked
}

/// Expand `handles` so each wall owner is accompanied by its display children.
/// Derived picks resolve to the owner first; non-wall handles pass through.
/// Used by hover, selection highlight, and MOVE/transform so the package
/// always travels together.
pub fn expand_handles_for_wall_packages(scene: &Scene, handles: &[Handle]) -> Vec<Handle> {
    let mut out = Vec::with_capacity(handles.len());
    let mut seen = rustc_hash::FxHashSet::default();
    for &handle in handles {
        let owner = resolve_wall_package(scene, handle);
        for package in wall_package_handles(scene, owner) {
            if seen.insert(package) {
                out.push(package);
            }
        }
    }
    out
}

/// True when `handle` is a wall-derived contour/hatch/solid entity — i.e. a
/// visible-layer entity generated by [`regenerate_wall_representation`] that
/// carries a `WALL_DERIVED` XDATA record pointing back at its axis. Used by
/// [`wall_axis_snap_wires`] to keep such entities out of the generic snap
/// candidate set (Bug 2): walls must always connect at their axes, never at
/// a shell/hatch/solid outline.
pub fn is_wall_derived_non_axis(scene: &Scene, handle: Handle) -> bool {
    resolve_wall_package(scene, handle) != handle
}

/// True when `handle` is the wall axis itself — the (normally invisible,
/// `AEC_WALL_AXIS_LAYER`) `LwPolyline` carrying the `WALL` XDATA
/// record. Axis entities must remain snap candidates even though their
/// layer is turned off (Bug 2).
pub(crate) fn is_wall_axis_entity(entity: &EntityType) -> bool {
    entity.common().layer == AEC_WALL_AXIS_LAYER && wall_from_entity(entity).is_some()
}

/// Snap-candidate carve-out for AEC walls (Bug 2).
///
/// The generic wire set used for OSNAP (`Scene::hit_test_wires` /
/// `Scene::entity_wires_arc`) is the same set the renderer draws from, so it
/// naturally excludes entities on the off/invisible `AEC_WALL_AXIS_LAYER`
/// (the wall axis) while including every visible wall-derived contour/
/// hatch/solid. Left alone, that means:
/// - the wall axis can never be snapped to at all, and
/// - a new/edited wall would snap onto the neighbour's shell/hatch outline
///   instead of its axis, producing walls that don't actually connect at a
///   shared axis point.
///
/// This filters `wires` down to only the axis for any wall-derived entity
/// (dropping its shell/hatch/solid siblings) and re-tessellates the axis
/// entities that the resident wire set skips outright, so they participate
/// in snapping despite their layer being off. Non-wall geometry passes
/// through completely unchanged. Cheap no-op when the document has no
/// `AEC_WALL_AXIS_LAYER` (i.e. no AEC walls have ever been drawn).
pub fn wall_axis_snap_wires(
    scene: &Scene,
    wires: std::sync::Arc<Vec<WireModel>>,
) -> std::sync::Arc<Vec<WireModel>> {
    if scene.document.layers.get(AEC_WALL_AXIS_LAYER).is_none() {
        return wires;
    }
    let mut filtered: Vec<WireModel> = wires
        .iter()
        .filter(|w| match w.name.parse::<u64>() {
            Ok(v) => !is_wall_derived_non_axis(scene, Handle::new(v)),
            Err(_) => true,
        })
        .cloned()
        .collect();

    let axis_entities: Vec<EntityType> = scene
        .document
        .entities()
        .filter(|e| is_wall_axis_entity(e))
        .cloned()
        .collect();
    if !axis_entities.is_empty() {
        filtered.extend(scene.wires_for_entities(&axis_entities));
    }
    std::sync::Arc::new(filtered)
}

/// True when `entity` carries a `WALL_REP` / `WALL_DERIVED` display tag.
pub(crate) fn is_wall_display_child_entity(entity: &EntityType) -> bool {
    let Some(record) = read_aec_record(entity) else {
        return false;
    };
    matches!(
        record.values.first(),
        Some(XDataValue::String(kind)) if kind == "WALL_REP" || kind == "WALL_DERIVED"
    )
}

/// Remove `wall` from every peer's `JOINED_PEERS` list and clear its own.
pub fn unlink_all_wall_peers(scene: &mut Scene, wall: Handle) {
    let peers = engine::owner_index::peers_of(&scene.document, wall);
    for peer in peers {
        engine::owner_index::unlink_peers(&mut scene.document, wall, peer);
    }
}

// aec_storey moved to project/storeys.rs


/// Expand an erase/delete selection with each wall's `derived_handles`
/// (contour/hatch/solid entities generated by [`regenerate_wall_representation`])
/// so deleting a `WALL` entity also removes its rendered representation.
///
/// Deduplicates and skips any handle not present in the document (already erased).
pub fn expand_with_wall_derived_handles(scene: &Scene, handles: &mut Vec<Handle>) {
    let mut extra = Vec::new();
    for handle in handles.iter() {
        let owner = resolve_wall_package(scene, *handle);
        extra.push(owner);
        let Some(entity) = scene.document.get_entity(owner) else {
            continue;
        };
        let Some(record) = read_aec_record(entity) else {
            continue;
        };
        let is_wall = matches!(
            record.values.first(),
            Some(XDataValue::String(kind)) if kind == "WALL"
        );
        if !is_wall {
            continue;
        }
        if let Some(v2) = wall_from_entity(entity) {
            extra.extend(v2.derived_handles.iter().copied());
        }
    }
    for h in extra {
        if !handles.contains(&h) && scene.document.get_entity(h).is_some() {
            handles.push(h);
        }
    }
}

// aec_wall_refresh moved to walls/refresh.rs


// aec_ifc_export moved to ifc/export.rs


// WallJoinCommand moved to walls/join.rs


/// True when `handle` resolves to a wall axis (or is itself a wall axis).
pub fn is_wall_pick_target(scene: &Scene, handle: Handle) -> bool {
    if handle.is_null() {
        return false;
    }
    let axis = resolve_wall_package(scene, handle);
    scene
        .document
        .get_entity(axis)
        .is_some_and(|e| wall_thickness_and_height(e).is_some())
}

/// True when `entity` is a wall *axis* (carries `WALL` XDATA), not a
/// derived contour/hatch/solid.
pub(crate) fn is_wall_axis_xdata(entity: &EntityType) -> bool {
    matches!(
        read_aec_record(entity).and_then(|r| r.values.first()),
        Some(XDataValue::String(kind)) if kind == "WALL"
    )
}

/// Collect every wall axis handle in the document (excluding derived geometry).
pub(crate) fn all_wall_axis_handles(scene: &Scene) -> Vec<Handle> {
    scene
        .document
        .entities()
        .filter(|e| is_wall_axis_xdata(e))
        .map(|e| e.common().handle)
        .collect()
}

/// Collect every handle that must be bumped after a wall edit: the axis plus
/// its current `derived_handles` (contour/hatch/solid). Used when a caller
/// needs the full package without regenerating.
pub fn wall_package_handles(scene: &Scene, wall_handle: Handle) -> Vec<Handle> {
    let mut handles = vec![wall_handle];
    for child in collect_wall_display_children(scene, wall_handle) {
        if !handles.contains(&child) {
            handles.push(child);
        }
    }
    handles
}
