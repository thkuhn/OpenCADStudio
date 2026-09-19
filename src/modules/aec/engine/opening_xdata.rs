//! Opening entity XDATA and host-wall place/remove.

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
use super::join_ops::*;
use super::storey_xdata::*;

// aec_wallreverse_do moved to walls (reverse.rs)


// ── Wall openings (window / door) ──────────────────────────────────────────

/// Build an `OPENING` XDATA record for a standalone opening entity.
pub(crate) fn opening_record(opening: &engine::openings::Opening) -> ExtendedDataRecord {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("OPENING".to_string()));
    record.add_value(XDataValue::Handle(opening.host_wall));
    record.add_value(XDataValue::Distance(opening.distance_along_axis));
    record.add_value(XDataValue::Distance(opening.width));
    record.add_value(XDataValue::Distance(opening.height));
    record.add_value(XDataValue::Distance(opening.sill_height));
    record.add_value(XDataValue::String(opening.kind.as_str().to_string()));
    record
}

/// Parse an `OPENING` XDATA record. `handle` is the entity that carries it.
pub fn opening_from_entity(entity: &EntityType, handle: Handle) -> Option<engine::openings::Opening> {
    let record = read_aec_record(entity)?;
    let v = &record.values;
    if v.len() < 7 {
        return None;
    }
    let XDataValue::String(kind) = &v[0] else {
        return None;
    };
    if kind != "OPENING" {
        return None;
    }
    let host_wall = match &v[1] {
        XDataValue::Handle(h) => *h,
        _ => return None,
    };
    let distance_along_axis = match v[2] {
        XDataValue::Distance(d) => d,
        _ => return None,
    };
    let width = match v[3] {
        XDataValue::Distance(d) => d,
        _ => return None,
    };
    let height = match v[4] {
        XDataValue::Distance(d) => d,
        _ => return None,
    };
    let sill_height = match v[5] {
        XDataValue::Distance(d) => d,
        _ => return None,
    };
    let opening_kind = match &v[6] {
        XDataValue::String(s) => engine::openings::OpeningKind::from_str(s),
        _ => engine::openings::OpeningKind::Window,
    };
    Some(engine::openings::Opening {
        handle,
        host_wall,
        distance_along_axis,
        width,
        height,
        sill_height,
        kind: opening_kind,
    })
}

/// All openings whose host wall is `wall_handle`, looked up via the wall's
/// `CHILD_HANDLES` owner index (no document scan).
pub fn openings_for_host_wall(scene: &Scene, wall_handle: Handle) -> Vec<engine::openings::Opening> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let mut out = Vec::new();
    for child in engine::owner_index::children_of(&scene.document, wall_handle) {
        let Some(entity) = scene.document.get_entity(child) else {
            continue;
        };
        if let Some(o) = opening_from_entity(entity, child) {
            out.push(o);
        }
    }
    out
}

/// Place a window/door opening on `wall_handle` at world point `pt`.
///
/// Creates a POINT entity carrying `OPENING` XDATA and regenerates the host
/// wall's 2D representation so the opening cut appears. Returns the new
/// opening handle plus every wall-derived handle touched.
pub fn place_wall_opening(
    scene: &mut Scene,
    wall_handle: Handle,
    pt: DVec3,
    kind: engine::openings::OpeningKind,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Result<(Handle, Vec<Handle>), String> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    if !is_wall_pick_target(scene, wall_handle) {
        return Err("select a wall entity".into());
    }
    let axis = get_wall_vertices(scene, wall_handle);
    if axis.len() < 2 {
        return Err("wall axis is degenerate".into());
    }
    let axis_2d: Vec<(f64, f64)> = axis.iter().map(|v| (v.x, v.y)).collect();
    let distance = engine::openings::distance_along_axis_from_point(&axis_2d, (pt.x, pt.y))
        .ok_or_else(|| "could not project point onto wall axis".to_string())?;

    let placeholder = engine::openings::Opening {
        handle: Handle::NULL,
        host_wall: wall_handle,
        distance_along_axis: distance,
        width: match kind {
            engine::openings::OpeningKind::Window => engine::openings::DEFAULT_WINDOW_WIDTH,
            engine::openings::OpeningKind::Door => engine::openings::DEFAULT_DOOR_WIDTH,
        },
        height: match kind {
            engine::openings::OpeningKind::Window => engine::openings::DEFAULT_WINDOW_HEIGHT,
            engine::openings::OpeningKind::Door => engine::openings::DEFAULT_DOOR_HEIGHT,
        },
        sill_height: match kind {
            engine::openings::OpeningKind::Window => engine::openings::DEFAULT_WINDOW_SILL,
            engine::openings::OpeningKind::Door => engine::openings::DEFAULT_DOOR_SILL,
        },
        kind,
    };

    // Anchor the POINT at the projected axis location (not the raw click).
    let (anchor, _) = engine::openings::point_and_tangent_at_distance(&axis_2d, distance)
        .unwrap_or(((pt.x, pt.y), (1.0, 0.0)));
    let point_entity = EntityType::Point(Point::at(Vector3::new(anchor.0, anchor.1, 0.0)));
    let opening_handle = scene.add_entity(point_entity);

    let mut opening = placeholder;
    opening.handle = opening_handle;
    write_aec_record(&mut scene.document, opening_handle, opening_record(&opening));
    engine::owner_index::add_child(&mut scene.document, wall_handle, opening_handle);

    let mut touched = match regenerate_wall_representation_with_rules_and_substitutions(
        scene,
        wall_handle,
        display_rules,
        style_substitutions,
        library_override,
    ) {
        Ok(t) => t,
        Err(_) => vec![wall_handle],
    };
    touched.push(opening_handle);
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    Ok((opening_handle, touched))
}

/// Remove an opening entity, drop it from the host wall's `CHILD_HANDLES`
/// index, and regenerate the host wall. Returns touched handles (wall +
/// former opening). No-op error when `opening_handle` is not an opening.
pub fn remove_wall_opening(
    scene: &mut Scene,
    opening_handle: Handle,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, String> {
    let Some(entity) = scene.document.get_entity(opening_handle).cloned() else {
        return Err("opening entity not found".into());
    };
    let Some(opening) = opening_from_entity(&entity, opening_handle) else {
        return Err("entity is not an opening".into());
    };
    let wall_handle = resolve_wall_package(scene, opening.host_wall);
    engine::owner_index::remove_child(&mut scene.document, wall_handle, opening_handle);
    scene.erase_entities(&[opening_handle]);
    let mut touched = match regenerate_wall_representation(scene, wall_handle, library_override) {
        Ok(t) => t,
        Err(_) => vec![wall_handle],
    };
    touched.push(opening_handle);
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    Ok(touched)
}

// WallOpeningCommand moved to walls/window.rs
