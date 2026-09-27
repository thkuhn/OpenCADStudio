//! Slab carrier package resolution, derived children lifecycle, and selection expansion.

#![allow(unused_imports)]
use std::collections::HashMap;
use uuid::Uuid;

use acadrust::entities::{LwPolyline, LwVertex, Point};
use acadrust::tables::AppId;
use acadrust::types::{Vector2, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, EntityType, Handle};

use crate::scene::model::hatch_model::{HatchModel, HatchPattern};
use crate::scene::Scene;

use super::owner_index;
use super::slab::{Slab, SlabJustification, SlabLayer};
use super::slab_opening::{SlabOpening, SlabOpeningDepth, SlabOpeningKind};
use super::slab_xdata::{slab_from_entity, slab_opening_from_entity};
use super::xdata::{
    aec_value_as_handle, ensure_app_id, read_aec_record, write_aec_record, AEC_APPID,
};

/// Display-child roles written on `SLAB_REP` XDATA.
pub const SLAB_REP_ROLE_CONTOUR: &str = "contour";
pub const SLAB_REP_ROLE_CEILING: &str = "ceiling";
pub const SLAB_REP_ROLE_HATCH: &str = "hatch";
pub const SLAB_REP_ROLE_SOLID: &str = "solid";

/// Display-child roles written on `SLAB_OPENING_REP` XDATA.
pub const SLAB_REP_ROLE_OPENING_CONTOUR: &str = "opening_contour";
pub const SLAB_REP_ROLE_OPENING_SYMBOL: &str = "opening_symbol";

/// Default layer names for slab carrier and derived representations.
pub const AEC_SLAB_CARRIER_LAYER: &str = "AEC_SLABS";
pub const AEC_SLAB_CONTOUR_LAYER: &str = "AEC_SLAB_CONTOUR";
pub const AEC_SLAB_CEILING_LAYER: &str = "AEC_SLAB_CEILING";
pub const AEC_SLAB_HATCH_LAYER: &str = "AEC_SLAB_HATCH";
pub const AEC_SLAB_SOLID_LAYER: &str = "AEC_SLAB_SOLID";
pub const AEC_SLAB_OPENING_LAYER: &str = "AEC_SLAB_OPENINGS";

/// Extracts the owner slab handle from a `SLAB_REP` or `SLAB_DERIVED` entity.
pub fn slab_rep_owner_from_entity(entity: &EntityType) -> Option<Handle> {
    for record in entity.common().extended_data.records() {
        if record.application_name != AEC_APPID {
            continue;
        }
        let kind = match record.values.first() {
            Some(XDataValue::String(s)) => s.as_str(),
            _ => continue,
        };
        if kind != "SLAB_REP" && kind != "SLAB_DERIVED" {
            continue;
        }
        return record.values.get(1).and_then(aec_value_as_handle);
    }
    None
}

/// Extracts the owner slab opening handle from a `SLAB_OPENING_REP` entity.
pub fn slab_opening_rep_owner_from_entity(entity: &EntityType) -> Option<Handle> {
    for record in entity.common().extended_data.records() {
        if record.application_name != AEC_APPID {
            continue;
        }
        let kind = match record.values.first() {
            Some(XDataValue::String(s)) => s.as_str(),
            _ => continue,
        };
        if kind != "SLAB_OPENING_REP" && kind != "SLAB_OPENING_DERIVED" {
            continue;
        }
        return record.values.get(1).and_then(aec_value_as_handle);
    }
    None
}

/// Tag `handle` as a display child of the host slab at `slab_handle` (`SLAB_REP` + owner handle + role).
pub fn write_slab_display_tag(
    scene: &mut Scene,
    handle: Handle,
    slab_handle: Handle,
    role: &str,
) {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("SLAB_REP".to_string()));
    record.add_value(XDataValue::Handle(slab_handle));
    record.add_value(XDataValue::String(role.to_string()));
    write_aec_record(&mut scene.document, handle, record);
}

/// Tag `handle` as a display child of the opening at `opening_handle` (`SLAB_OPENING_REP` + owner handle + role).
pub fn write_slab_opening_display_tag(
    scene: &mut Scene,
    handle: Handle,
    opening_handle: Handle,
    role: &str,
) {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("SLAB_OPENING_REP".to_string()));
    record.add_value(XDataValue::Handle(opening_handle));
    record.add_value(XDataValue::String(role.to_string()));
    write_aec_record(&mut scene.document, handle, record);
}

/// Collects all derived display child handles for a slab carrier entity.
///
/// Filters out opening entities (`SlabOpening`) and opening representation entities so
/// that slab regeneration does not erase associative openings.
pub fn collect_slab_display_children(scene: &Scene, owner: Handle) -> Vec<Handle> {
    let mut out = Vec::new();
    if let Some(entity) = scene.document.get_entity(owner) {
        if let Some(slab) = slab_from_entity(entity) {
            out.extend(slab.derived_handles);
        }
    }
    for h in owner_index::children_of(&scene.document, owner) {
        if out.contains(&h) {
            continue;
        }
        if let Some(entity) = scene.document.get_entity(h) {
            // Keep opening entities and opening display children intact
            if slab_opening_from_entity(entity).is_some()
                || slab_opening_rep_owner_from_entity(entity).is_some()
            {
                continue;
            }
        }
        out.push(h);
    }
    for entity in scene.document.entities() {
        let handle = entity.common().handle;
        if handle == owner {
            continue;
        }
        if slab_rep_owner_from_entity(entity) == Some(owner) && !out.contains(&handle) {
            out.push(handle);
        }
    }
    out
}

/// Collects all derived display child handles for a slab opening entity.
pub fn collect_slab_opening_display_children(scene: &Scene, opening_handle: Handle) -> Vec<Handle> {
    let mut out = Vec::new();
    if let Some(entity) = scene.document.get_entity(opening_handle) {
        if let Some(opening) = slab_opening_from_entity(entity) {
            out.extend(opening.derived_handles);
        }
    }
    for h in owner_index::children_of(&scene.document, opening_handle) {
        if !out.contains(&h) {
            out.push(h);
        }
    }
    for entity in scene.document.entities() {
        let handle = entity.common().handle;
        if handle == opening_handle {
            continue;
        }
        if slab_opening_rep_owner_from_entity(entity) == Some(opening_handle)
            && !out.contains(&handle)
        {
            out.push(handle);
        }
    }
    out
}

/// Writes `pl` into an existing contour handle when possible to avoid leaving visual ghosts.
pub fn reuse_or_add_slab_contour(
    scene: &mut Scene,
    reusable: &mut Vec<Handle>,
    pl: LwPolyline,
    owner_handle: Option<Handle>,
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
    let mut entity = EntityType::LwPolyline(pl);
    if let Some(owner) = owner_handle {
        if !owner.is_null() && owner != scene.document.header.model_space_block_handle {
            entity.common_mut().owner_handle = owner;
        }
    }
    scene.add_entity(entity)
}

/// Resolves a clicked entity to its carrier slab owner handle if it is a derived slab entity.
pub fn resolve_slab_package(scene: &Scene, clicked: Handle) -> Handle {
    let Some(entity) = scene.document.get_entity(clicked) else {
        return clicked;
    };
    if slab_opening_from_entity(entity).is_some()
        || slab_opening_rep_owner_from_entity(entity).is_some()
    {
        return clicked;
    }
    if slab_from_entity(entity).is_some() {
        return clicked;
    }
    if let Some(owner) = slab_rep_owner_from_entity(entity) {
        if scene.document.get_entity(owner).is_some() {
            return owner;
        }
    }
    for entity in scene.document.entities() {
        let owner = entity.common().handle;
        if let Some(slab) = slab_from_entity(entity) {
            if slab.derived_handles.iter().any(|h| *h == clicked) {
                return owner;
            }
        }
        if owner_index::children_of(&scene.document, owner)
            .iter()
            .any(|h| *h == clicked)
            && slab_from_entity(entity).is_some()
            && slab_opening_from_entity(scene.document.get_entity(clicked).unwrap_or(entity))
                .is_none()
        {
            return owner;
        }
    }
    clicked
}

/// Returns all handles that belong to a slab package (carrier, derived representations, openings and their representations).
pub fn slab_package_handles(scene: &Scene, slab_handle: Handle) -> Vec<Handle> {
    let mut handles = vec![slab_handle];
    let Some(entity) = scene.document.get_entity(slab_handle) else {
        return handles;
    };
    let Some(slab) = slab_from_entity(entity) else {
        return handles;
    };
    for d in &slab.derived_handles {
        if scene.document.get_entity(*d).is_some() && !handles.contains(d) {
            handles.push(*d);
        }
    }
    for child in owner_index::children_of(&scene.document, slab_handle) {
        if scene.document.get_entity(child).is_some() && !handles.contains(&child) {
            handles.push(child);
        }
    }
    for op_handle in &slab.opening_handles {
        if scene.document.get_entity(*op_handle).is_some() && !handles.contains(op_handle) {
            handles.push(*op_handle);
            for op_d in collect_slab_opening_display_children(scene, *op_handle) {
                if !handles.contains(&op_d) {
                    handles.push(op_d);
                }
            }
        }
    }
    handles
}

/// Expand `handles` so each slab owner is accompanied by its display children and openings.
pub fn expand_handles_for_slab_packages(scene: &Scene, handles: &[Handle]) -> Vec<Handle> {
    let mut out = Vec::with_capacity(handles.len());
    let mut seen = rustc_hash::FxHashSet::default();
    for &handle in handles {
        let owner = resolve_slab_package(scene, handle);
        for package in slab_package_handles(scene, owner) {
            if seen.insert(package) {
                out.push(package);
            }
        }
    }
    out
}

/// True when `handle` is a slab-derived representation entity.
pub fn is_slab_derived(scene: &Scene, handle: Handle) -> bool {
    resolve_slab_package(scene, handle) != handle
}

/// True when `entity` carries a `SLAB` XDATA record.
pub fn is_slab_carrier_entity(entity: &EntityType) -> bool {
    slab_from_entity(entity).is_some()
}

/// True when `entity` carries a `SLAB_OPENING` XDATA record.
pub fn is_slab_opening_carrier_entity(entity: &EntityType) -> bool {
    slab_opening_from_entity(entity).is_some()
}

/// Collect every slab carrier handle in the document (excluding derived geometry).
pub fn all_slab_carrier_handles(scene: &Scene) -> Vec<Handle> {
    scene
        .document
        .entities()
        .filter(|e| is_slab_carrier_entity(e))
        .map(|e| e.common().handle)
        .collect()
}

/// Returns the slab-opening owner handle when `handle` is an opening carrier
/// or one of its derived representation children.
pub fn slab_opening_owner_if_any(scene: &Scene, handle: Handle) -> Option<Handle> {
    let entity = scene.document.get_entity(handle)?;
    if slab_opening_from_entity(entity).is_some() {
        return Some(handle);
    }
    if let Some(owner) = slab_opening_rep_owner_from_entity(entity) {
        if scene
            .document
            .get_entity(owner)
            .is_some_and(is_slab_opening_carrier_entity)
        {
            return Some(owner);
        }
    }
    None
}

/// Expand an erase selection with each slab's derived handles and associative openings
/// so deleting a slab entity also removes its representations and openings.
pub fn expand_with_slab_derived_handles(scene: &Scene, handles: &mut Vec<Handle>) {
    let mut extra = Vec::new();
    for handle in handles.iter() {
        let owner = resolve_slab_package(scene, *handle);
        extra.push(owner);
        let Some(entity) = scene.document.get_entity(owner) else {
            continue;
        };
        if let Some(slab) = slab_from_entity(entity) {
            extra.extend(slab.derived_handles.iter().copied());
            for child in owner_index::children_of(&scene.document, owner) {
                extra.push(child);
            }
            for op in &slab.opening_handles {
                extra.push(*op);
                extra.extend(collect_slab_opening_display_children(scene, *op));
            }
        }
        if let Some(opening) = slab_opening_from_entity(entity) {
            extra.extend(opening.derived_handles.iter().copied());
            for child in owner_index::children_of(&scene.document, *handle) {
                extra.push(child);
            }
        }
    }
    for h in extra {
        if !handles.contains(&h) && scene.document.get_entity(h).is_some() {
            handles.push(h);
        }
    }
}
