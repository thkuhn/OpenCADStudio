//! Storey XDATA helpers.

#![allow(unused_imports)]
use std::collections::HashMap;
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
    self as engine, Storey, StoreyRef, StyleLibrary, Wall, WallJustification, WallLayer,
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

// MaterialCommand/StyleCommand moved to styles/


// aec_room moved to rooms/room.rs


/// Build a `STOREY` XDATA record (id + name/elevation/height + optional project UUID).
pub(crate) fn storey_record(
    storey_id: u32,
    storey: &Storey,
    project_id: Option<Uuid>,
) -> ExtendedDataRecord {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("STOREY".to_string()));
    record.add_value(XDataValue::Integer32(storey_id as i32));
    record.add_value(XDataValue::String(storey.name.clone()));
    record.add_value(XDataValue::Real(storey.elevation));
    record.add_value(XDataValue::Real(storey.height));
    if let Some(project_id) = project_id {
        record.add_value(XDataValue::String(project_id.to_string()));
    }
    record
}

/// Parse a `STOREY` XDATA record into `(storey_id, Storey, optional project UUID)`.
pub fn storey_from_entity(entity: &EntityType) -> Option<(u32, Storey, Option<Uuid>)> {
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
    let project_id = v.get(5).and_then(|value| match value {
        XDataValue::String(value) => Uuid::parse_str(value).ok(),
        _ => None,
    });
    Some((storey_id, Storey::new(name, elevation, height), project_id))
}

/// The first `STOREY` carrier linked to a project storey.
pub fn linked_storey(doc: &CadDocument) -> Option<(u32, Storey, Uuid)> {
    doc.entities().find_map(|entity| {
        let (storey_id, storey, project_id) = storey_from_entity(entity)?;
        Some((storey_id, storey, project_id?))
    })
}

/// Write or update the drawing's single project-storey link without changing
/// owner-index carriers that do not already carry a project UUID.
pub fn write_storey_link(scene: &mut Scene, storey_ref: &StoreyRef) -> Handle {
    let cache = Storey::new(
        storey_ref.name.clone(),
        storey_ref.derived_elevation(),
        storey_ref.derived_height(),
    );
    let existing_link = scene.document.entities().find_map(|entity| {
        let (storey_id, storey, project_id) = storey_from_entity(entity)?;
        project_id.map(|project_id| (storey_id, storey, project_id, entity.common().handle))
    });
    let (storey_id, handle) = match existing_link {
        Some((storey_id, existing, project_id, handle)) => {
            if project_id == storey_ref.id && existing == cache {
                return handle;
            }
            (storey_id, handle)
        }
        None => {
            let point = EntityType::Point(Point::at(Vector3::new(
                0.0,
                0.0,
                cache.elevation,
            )));
            (0, scene.add_entity(point))
        }
    };
    write_aec_record(
        &mut scene.document,
        handle,
        storey_record(storey_id, &cache, Some(storey_ref.id)),
    );
    handle
}

/// Document handle of the entity carrying `STOREY` XDATA for `storey_id`.
pub fn find_storey_handle(doc: &CadDocument, storey_id: u32) -> Option<Handle> {
    for entity in doc.entities() {
        if let Some((id, _, _)) = storey_from_entity(entity) {
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
    write_aec_record(&mut scene.document, handle, storey_record(storey_id, &s, None));
    handle
}

#[cfg(test)]
mod storey_link_tests {
    use super::*;

    #[test]
    fn write_and_read_project_storey_link() {
        let mut scene = Scene::new();
        let storey_ref = StoreyRef::new_with_height("First Floor", 3.25, 2.85, "first.dwg");

        let handle = write_storey_link(&mut scene, &storey_ref);
        let (storey_id, storey, project_id) = linked_storey(&scene.document).unwrap();

        assert_eq!(storey_id, 0);
        assert_eq!(project_id, storey_ref.id);
        assert_eq!(storey.name, "First Floor");
        assert_eq!(storey.elevation, storey_ref.derived_elevation());
        assert_eq!(storey.height, storey_ref.derived_height());
        assert_eq!(scene.document.get_entity(handle).unwrap().common().handle, handle);
        assert!(linked_storey(&Scene::new().document).is_none());
    }

    #[test]
    fn legacy_five_field_record_is_not_a_project_link() {
        let mut scene = Scene::new();
        let storey = Storey::new("Legacy", 1.5, 3.0);
        let handle = scene.add_entity(EntityType::Point(Point::at(Vector3::new(0.0, 0.0, 1.5))));
        write_aec_record(
            &mut scene.document,
            handle,
            storey_record(4, &storey, None),
        );

        assert_eq!(
            storey_from_entity(scene.document.get_entity(handle).unwrap()),
            Some((4, storey, None))
        );
        assert!(linked_storey(&scene.document).is_none());
    }

    #[test]
    fn owner_index_carrier_stays_unlinked_when_project_link_is_added() {
        let mut scene = Scene::new();
        let index_handle = ensure_storey_entity(&mut scene, 7, None);
        let storey_ref = StoreyRef::new("Ground", 0.0, "ground.dwg");

        let link_handle = write_storey_link(&mut scene, &storey_ref);

        assert_ne!(index_handle, link_handle);
        assert_eq!(storey_from_entity(scene.document.get_entity(index_handle).unwrap()).unwrap().2, None);
        assert_eq!(linked_storey(&scene.document).unwrap().2, storey_ref.id);
    }

    #[test]
    fn updating_project_link_preserves_its_numeric_owner_id() {
        let mut scene = Scene::new();
        let original_ref = StoreyRef::new("Old", 0.0, "old.dwg");
        let original = Storey::new("Cached old", 0.0, 3.0);
        let handle = scene.add_entity(EntityType::Point(Point::at(Vector3::new(0.0, 0.0, 0.0))));
        write_aec_record(
            &mut scene.document,
            handle,
            storey_record(12, &original, Some(original_ref.id)),
        );

        let updated_ref = StoreyRef::new_with_height("Updated", 4.0, 2.75, "updated.dwg");
        assert_eq!(write_storey_link(&mut scene, &updated_ref), handle);

        let (storey_id, storey, project_id) = linked_storey(&scene.document).unwrap();
        assert_eq!(storey_id, 12);
        assert_eq!(project_id, updated_ref.id);
        assert_eq!(storey.name, "Updated");
        assert_eq!(storey.elevation, updated_ref.derived_elevation());
        assert_eq!(storey.height, updated_ref.derived_height());
        assert_eq!(scene.document.entities().count(), 1);
    }
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
    record.values = wall_record_for_wall(&wall);
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
