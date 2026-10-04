//! Room carrier package resolution, derived children lifecycle, and selection expansion.

#![allow(unused_imports)]
use std::collections::HashMap;

use acadrust::entities::{LwPolyline, LwVertex, MText, Point};
use acadrust::tables::AppId;
use acadrust::types::{Vector2, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, EntityType, Handle};

use crate::scene::Scene;

use super::owner_index;
use super::room::Room;
use super::xdata::{
    aec_value_as_handle, ensure_app_id, read_aec_record, write_aec_record, AEC_APPID,
};

/// Display-child roles written on `ROOM_REP` XDATA.
pub const ROOM_REP_ROLE_CONTOUR: &str = "contour";
pub const ROOM_REP_ROLE_STAMP: &str = "stamp";
pub const ROOM_REP_ROLE_HATCH: &str = "hatch";
pub const ROOM_REP_ROLE_CEILING_HATCH: &str = "ceiling_hatch";
pub const ROOM_REP_ROLE_THRESHOLD: &str = "threshold";
pub const ROOM_REP_ROLE_SOLID: &str = "solid";

/// Default layer names for room carrier and derived representations.
pub const AEC_ROOM_CARRIER_LAYER: &str = "AEC_ROOMS";
pub const AEC_ROOM_STAMP_LAYER: &str = "AEC_ROOM_STAMP";
pub const AEC_ROOM_HATCH_LAYER: &str = "AEC_ROOM_HATCH";
pub const AEC_ROOM_CEILING_HATCH_LAYER: &str = "AEC_CEILING_HATCH";
pub const AEC_ROOM_THRESHOLD_LAYER: &str = "AEC_OPENING_THRESHOLD";
pub const AEC_ROOM_SOLID_LAYER: &str = "AEC_ROOM_SOLID";
pub const AEC_ROOM_SCHEDULE_LAYER: &str = "AEC_SCHEDULE";
pub const ROOM_SCHEDULE_XDATA_TAG: &str = "ROOM_SCHEDULE";

/// Extracts the owner room handle from a `ROOM_REP` or `ROOM_DERIVED` entity.
pub fn room_rep_owner_from_entity(entity: &EntityType) -> Option<Handle> {
    for record in entity.common().extended_data.records() {
        if record.application_name != AEC_APPID {
            continue;
        }
        let kind = match record.values.first() {
            Some(XDataValue::String(s)) => s.as_str(),
            _ => continue,
        };
        if kind != "ROOM_REP" && kind != "ROOM_DERIVED" {
            continue;
        }
        return record.values.get(1).and_then(aec_value_as_handle);
    }
    None
}

/// Tag `handle` as a display child of the host room at `room_handle` (`ROOM_REP` + owner handle + role).
pub fn write_room_display_tag(
    scene: &mut Scene,
    handle: Handle,
    room_handle: Handle,
    role: &str,
) {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("ROOM_REP".to_string()));
    record.add_value(XDataValue::Handle(room_handle));
    record.add_value(XDataValue::String(role.to_string()));
    write_aec_record(&mut scene.document, handle, record);
}

/// Collects all derived display child handles for a room carrier entity.
pub fn collect_room_display_children(scene: &Scene, owner: Handle) -> Vec<Handle> {
    let indexed = owner_index::children_of(&scene.document, owner);
    if !indexed.is_empty() {
        return indexed;
    }
    let mut out = Vec::new();
    for entity in scene.document.entities() {
        if room_rep_owner_from_entity(entity) == Some(owner) {
            out.push(entity.common().handle);
        }
    }
    out
}

/// Removes all derived display child entities of a room from the scene document.
pub fn remove_room_display_children(scene: &mut Scene, owner: Handle) {
    let children = collect_room_display_children(scene, owner);
    if !children.is_empty() {
        scene.erase_entities(&children);
    }
    owner_index::set_children(&mut scene.document, owner, &[]);
}

/// Resolves an entity handle to its carrier room handle.
pub fn resolve_room_package(entity: &EntityType) -> Option<Handle> {
    if let Some(owner) = room_rep_owner_from_entity(entity) {
        return Some(owner);
    }
    if is_room_carrier(entity) {
        return Some(entity.common().handle);
    }
    None
}

/// Resolves an entity handle or derived child handle to the carrier room handle.
pub fn resolve_room_package_handle(scene: &Scene, handle: Handle) -> Handle {
    if let Some(entity) = scene.document.get_entity(handle) {
        if let Some(owner) = resolve_room_package(entity) {
            return owner;
        }
    }
    handle
}

/// Returns all handles that belong to a room package (carrier and derived representations).
pub fn room_package_handles(scene: &Scene, room_handle: Handle) -> Vec<Handle> {
    let mut handles = vec![room_handle];
    for child in collect_room_display_children(scene, room_handle) {
        if scene.document.get_entity(child).is_some() && !handles.contains(&child) {
            handles.push(child);
        }
    }
    for child in owner_index::children_of(&scene.document, room_handle) {
        if scene.document.get_entity(child).is_some() && !handles.contains(&child) {
            handles.push(child);
        }
    }
    handles
}

/// Expand `handles` so each room owner is accompanied by its display children (stamp, hatch, 3D).
pub fn expand_handles_for_room_packages(scene: &Scene, handles: &[Handle]) -> Vec<Handle> {
    let mut out = Vec::with_capacity(handles.len());
    let mut seen = rustc_hash::FxHashSet::default();
    for &handle in handles {
        let owner = resolve_room_package_handle(scene, handle);
        for package in room_package_handles(scene, owner) {
            if seen.insert(package) {
                out.push(package);
            }
        }
    }
    out
}

/// Expand an erase selection with each room's derived handles.
pub fn expand_with_room_derived_handles(scene: &Scene, handles: &mut Vec<Handle>) {
    let mut extra = Vec::new();
    for handle in handles.iter() {
        let owner = resolve_room_package_handle(scene, *handle);
        if owner != *handle {
            extra.push(owner);
        }
        for child in collect_room_display_children(scene, owner) {
            extra.push(child);
        }
    }
    handles.extend(extra);
}

/// Collect every room carrier handle in the document.
pub fn all_room_carrier_handles(scene: &Scene) -> Vec<Handle> {
    scene
        .document
        .entities()
        .filter(|e| is_room_carrier(e))
        .map(|e| e.common().handle)
        .collect()
}

/// Returns true if the entity is an AEC room carrier entity (`ROOM` XDATA).
pub fn is_room_carrier(entity: &EntityType) -> bool {
    for record in entity.common().extended_data.records() {
        if record.application_name != AEC_APPID {
            continue;
        }
        if matches!(record.values.first(), Some(XDataValue::String(s)) if s == "ROOM") {
            return true;
        }
    }
    false
}

/// Returns true if the entity is an AEC room schedule table.
pub fn is_room_schedule(entity: &EntityType) -> bool {
    for record in entity.common().extended_data.records() {
        if record.application_name != AEC_APPID {
            continue;
        }
        if matches!(record.values.first(), Some(XDataValue::String(s)) if s == ROOM_SCHEDULE_XDATA_TAG) {
            return true;
        }
    }
    false
}

/// Tag `handle` as an AEC room schedule table (`ROOM_SCHEDULE`).
pub fn write_room_schedule_tag(scene: &mut Scene, handle: Handle) {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String(ROOM_SCHEDULE_XDATA_TAG.to_string()));
    write_aec_record(&mut scene.document, handle, record);
}

/// Returns true if the entity is a derived representation child of a room.
pub fn is_room_derived(entity: &EntityType) -> bool {
    room_rep_owner_from_entity(entity).is_some()
}

/// Finds the handle of the room stamp MText for a room carrier entity.
pub fn room_stamp_handle(scene: &Scene, owner: Handle) -> Option<Handle> {
    for child in collect_room_display_children(scene, owner) {
        if let Some(entity) = scene.document.get_entity(child) {
            for record in entity.common().extended_data.records() {
                if record.application_name == AEC_APPID
                    && record.values.get(2) == Some(&XDataValue::String(ROOM_REP_ROLE_STAMP.to_string()))
                {
                    return Some(child);
                }
            }
        }
    }
    None
}
