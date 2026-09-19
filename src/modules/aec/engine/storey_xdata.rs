//! Storey XDATA helpers and in-memory storey registry.

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
use super::opening_xdata::*;

/// In-memory storey store for the scaffold (persistence via document XDATA
/// is a follow-up; matches the former plugin's pragmatism).
pub(crate) static STOREYS: Mutex<Vec<Storey>> = Mutex::new(Vec::new());

// MaterialCommand/StyleCommand moved to styles/


// aec_room moved to rooms/room.rs


/// Build a `STOREY` XDATA record (id + name/elevation/height).
pub(crate) fn storey_record(storey_id: u32, storey: &Storey) -> ExtendedDataRecord {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("STOREY".to_string()));
    record.add_value(XDataValue::Integer32(storey_id as i32));
    record.add_value(XDataValue::String(storey.name.clone()));
    record.add_value(XDataValue::Real(storey.elevation));
    record.add_value(XDataValue::Real(storey.height));
    record
}

/// Parse a `STOREY` XDATA record into `(storey_id, Storey)`.
pub fn storey_from_entity(entity: &EntityType) -> Option<(u32, Storey)> {
    let record = read_aec_record(entity)?;
    let v = &record.values;
    if v.len() < 5 {
        return None;
    }
    let XDataValue::String(kind) = &v[0] else {
        return None;
    };
    if kind != "STOREY" {
        return None;
    }
    let storey_id = match v[1] {
        XDataValue::Integer32(i) => i as u32,
        _ => return None,
    };
    let name = match &v[2] {
        XDataValue::String(s) => s.clone(),
        _ => return None,
    };
    let elevation = match v[3] {
        XDataValue::Real(r) => r,
        _ => return None,
    };
    let height = match v[4] {
        XDataValue::Real(r) => r,
        _ => return None,
    };
    Some((storey_id, Storey::new(name, elevation, height)))
}

/// Document handle of the entity carrying `STOREY` XDATA for `storey_id`.
pub fn find_storey_handle(doc: &CadDocument, storey_id: u32) -> Option<Handle> {
    for entity in doc.entities() {
        if let Some((id, _)) = storey_from_entity(entity) {
            if id == storey_id {
                return Some(entity.common().handle);
            }
        }
    }
    None
}

/// Ensure a storey entity exists for `storey_id`. Creates a POINT carrier with
/// `STOREY` XDATA when missing (using `storey` metadata, or a default Level N).
pub fn ensure_storey_entity(scene: &mut Scene, storey_id: u32, storey: Option<&Storey>) -> Handle {
    if let Some(h) = find_storey_handle(&scene.document, storey_id) {
        return h;
    }
    let s = storey.cloned().unwrap_or_else(|| {
        Storey::new(
            format!("Level {}", storey_id + 1),
            (storey_id as f64) * 3.0,
            3.0,
        )
    });
    let point = EntityType::Point(Point::at(Vector3::new(0.0, 0.0, s.elevation)));
    let handle = scene.add_entity(point);
    write_aec_record(&mut scene.document, handle, storey_record(storey_id, &s));
    handle
}

/// Wall axis handles currently indexed as children of the storey entity.
/// Filters `CHILD_HANDLES` down to entities that parse as `WALL`.
pub fn walls_for_storey(scene: &Scene, storey_handle: Handle) -> Vec<Handle> {
    let mut out = Vec::new();
    for child in engine::owner_index::children_of(&scene.document, storey_handle) {
        let Some(entity) = scene.document.get_entity(child) else {
            continue;
        };
        if wall_from_entity(entity).is_some() {
            out.push(child);
        }
    }
    out
}

/// Register `wall_handle` under its current `storey_id` owner index.
pub fn register_wall_in_storey(scene: &mut Scene, wall_handle: Handle) {
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return;
    };
    let Some(wall) = wall_from_entity(entity) else {
        return;
    };
    let storey_h = ensure_storey_entity(scene, wall.storey_id, None);
    engine::owner_index::add_child(&mut scene.document, storey_h, wall_handle);
}

/// Drop `wall_handle` from its current storey owner index (if any).
pub fn unregister_wall_from_storey(scene: &mut Scene, wall_handle: Handle) {
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return;
    };
    let Some(wall) = wall_from_entity(entity) else {
        return;
    };
    if let Some(storey_h) = find_storey_handle(&scene.document, wall.storey_id) {
        engine::owner_index::remove_child(&mut scene.document, storey_h, wall_handle);
    }
}

/// Change a wall's `storey_id`, reparenting the owner-index entry between
/// storey entities and rewriting WALL XDATA.
pub fn set_wall_storey(scene: &mut Scene, wall_handle: Handle, new_storey_id: u32) -> bool {
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(mut wall) = wall_from_entity(entity) else {
        return false;
    };
    if wall.storey_id == new_storey_id {
        // Still ensure membership is recorded.
        let storey_h = ensure_storey_entity(scene, new_storey_id, None);
        engine::owner_index::add_child(&mut scene.document, storey_h, wall_handle);
        return true;
    }
    if let Some(old_h) = find_storey_handle(&scene.document, wall.storey_id) {
        engine::owner_index::remove_child(&mut scene.document, old_h, wall_handle);
    }
    wall.storey_id = new_storey_id;
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    for v in wall_record(
        &wall.style_id,
        wall.height,
        wall.storey_id,
        &wall.layers,
        &wall.derived_handles,
        wall.justification, wall.phase, wall.hatch_override.as_ref()) {
        record.add_value(v);
    }
    if !write_aec_record(&mut scene.document, wall_handle, record) {
        return false;
    }
    let new_h = ensure_storey_entity(scene, new_storey_id, None);
    engine::owner_index::add_child(&mut scene.document, new_h, wall_handle);
    true
}

/// Before erasing entities, drop wall axes from their storey owner indexes
/// and clear symmetric join peer links.
pub fn unregister_walls_from_storeys(scene: &mut Scene, handles: &[Handle]) {
    for handle in handles {
        unregister_wall_from_storey(scene, *handle);
        unlink_all_wall_peers(scene, *handle);
    }
}
