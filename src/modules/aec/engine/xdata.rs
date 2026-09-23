//! OPENCAD_AEC XDATA read/write for walls, junctions, and ids.

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
use super::wall_package::*;
use super::wall_regen::*;
use super::join_ops::*;
use super::storey_xdata::*;
use super::opening_xdata::*;

/// APPID used for all AEC XDATA records (must stay stable for round-trip).
pub const AEC_APPID: &str = "OPENCAD_AEC";

/// Register `OPENCAD_AEC` in the APPID table if missing so XDATA survives
/// DWG/DXF round-trip.
pub(crate) fn ensure_app_id(doc: &mut CadDocument) {
    if !doc.app_ids.contains(AEC_APPID) {
        let mut app = AppId::new(AEC_APPID);
        app.handle = doc.allocate_handle();
        let _ = doc.app_ids.add(app);
    }
}

/// Attach (or replace) an `OPENCAD_AEC` XDATA record on `handle`.
///
/// When `record` starts with a string kind tag (e.g. `"WALL"`, `"OPENING"`),
/// only an existing AEC record with the same tag is replaced — other AEC
/// kinds on the same entity (notably `CHILD_HANDLES` / `JOINED_PEERS` owner
/// indexes) are preserved. Records without a leading string tag still replace
/// every AEC record (legacy behaviour).
pub(crate) fn write_aec_record(doc: &mut CadDocument, handle: Handle, record: ExtendedDataRecord) -> bool {
    ensure_app_id(doc);
    let app_handle = doc.app_ids.get(AEC_APPID).map(|a| a.handle.value());
    let Some(entity) = doc.get_entity_mut(handle) else {
        return false;
    };
    let tag = match record.values.first() {
        Some(XDataValue::String(s)) => Some(s.as_str()),
        _ => None,
    };
    let xd = &mut entity.common_mut().extended_data;
    let kept: Vec<_> = xd
        .records()
        .iter()
        .filter(|r| {
            if r.application_name != AEC_APPID {
                return true;
            }
            match tag {
                Some(t) => {
                    !matches!(r.values.first(), Some(XDataValue::String(s)) if s == t)
                }
                None => false,
            }
        })
        .cloned()
        .collect();
    xd.clear();
    for r in kept {
        xd.add_record(r);
    }
    xd.add_record(record);
    // Verbatim DWG EED for this APPID wins on save over structured records.
    // Any AEC write must drop the stale blob so plane IDs and similar edits persist.
    if let Some(ah) = app_handle {
        xd.raw_dwg_eed.retain(|(a, _)| *a != ah);
    }
    true
}

/// Read the primary `OPENCAD_AEC` record on `entity` (WALL / OPENING / ROOM /
/// WALL_DERIVED / …), skipping pure index tags (`CHILD_HANDLES`,
/// `JOINED_PEERS`, `JOIN_OVERRIDE`) that may coexist on the same entity.
pub(crate) fn read_aec_record(entity: &EntityType) -> Option<&ExtendedDataRecord> {
    entity.common().extended_data.records().iter().find(|r| {
        if r.application_name != AEC_APPID {
            return false;
        }
        match r.values.first() {
            Some(XDataValue::String(s))
                if s == engine::owner_index::CHILD_HANDLES_TAG
                    || s == engine::owner_index::JOINED_PEERS_TAG
                    || s == JOIN_OVERRIDE_TAG =>
            {
                false
            }
            _ => true,
        }
    })
}

/// Display-child roles written on `WALL_REP` XDATA.
pub(crate) const WALL_REP_ROLE_CONTOUR: &str = "contour";

pub(crate) const WALL_REP_ROLE_HATCH: &str = "hatch";

pub(crate) const WALL_REP_ROLE_SOLID: &str = "solid";

/// Tag `handle` as a display child of the wall axis at `axis_handle`
/// (`WALL_REP` + owner handle + role). Legacy `WALL_DERIVED` is still
/// accepted by [`resolve_wall_package`].
pub(crate) fn write_wall_display_tag(scene: &mut Scene, handle: Handle, axis_handle: Handle, role: &str) {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("WALL_REP".to_string()));
    record.add_value(XDataValue::Handle(axis_handle));
    record.add_value(XDataValue::String(role.to_string()));
    write_aec_record(&mut scene.document, handle, record);
}

/// Legacy alias: untagged derived child (no role). Kept for tests that
/// synthesize orphan display entities.
#[cfg(test)]
pub(crate) fn write_wall_derived_tag(scene: &mut Scene, handle: Handle, axis_handle: Handle) {
    write_wall_display_tag(scene, handle, axis_handle, WALL_REP_ROLE_CONTOUR);
}

/// XDATA kind tag for a manual join-constraint override on a wall axis end
/// (a "junction"). A junction is identified by the wall axis's own handle
/// plus which end of its axis it sits at (`0` for the start vertex, `1` for
/// the last vertex) — mirroring the `end_a`/`end_b` vertex-index convention
/// already used by [`join::join_wall_axes`]. Multiple walls sharing a
/// junction point each store their own override on their own axis/end, since
/// XDATA lives on a single entity.
pub(crate) const JOIN_OVERRIDE_TAG: &str = "JOIN_OVERRIDE";

/// XDATA end key for a T-junction override stored on the **through** wall
/// (no axis endpoint at the node). Maps `usize::MAX` ↔ `-1`.
pub const THROUGH_SPAN_OVERRIDE_END: usize = usize::MAX;

pub(crate) fn join_override_end_key(end_index: usize) -> i32 {
    if end_index == THROUGH_SPAN_OVERRIDE_END {
        -1
    } else {
        end_index as i32
    }
}

/// Write (or replace) a [`join::JunctionOverride`] as XDATA on `axis_handle`,
/// tied to `end_index` (`0` = axis start, `1` = axis end). The payload is
/// serialized as JSON, matching the pattern used for other AEC XDATA blobs.
/// Only the record for the same `end_index` is replaced — an override on the
/// other end of the same axis is left untouched.
pub fn write_junction_override(
    scene: &mut Scene,
    axis_handle: Handle,
    end_index: usize,
    override_data: &join::JunctionOverride,
) -> bool {
    let Ok(json) = serde_json::to_string(override_data) else {
        return false;
    };
    ensure_app_id(&mut scene.document);
    let app_handle = scene
        .document
        .app_ids
        .get(AEC_APPID)
        .map(|a| a.handle.value());
    let Some(entity) = scene.document.get_entity_mut(axis_handle) else {
        return false;
    };
    let end_index = join_override_end_key(end_index);
    let xd = &mut entity.common_mut().extended_data;
    let kept: Vec<_> = xd
        .records()
        .iter()
        .filter(|r| {
            if r.application_name != AEC_APPID {
                return true;
            }
            !matches!(
                (r.values.first(), r.values.get(1)),
                (Some(XDataValue::String(s)), Some(XDataValue::Integer32(e)))
                    if s == JOIN_OVERRIDE_TAG && *e == end_index
            )
        })
        .cloned()
        .collect();
    xd.clear();
    for r in kept {
        xd.add_record(r);
    }
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String(JOIN_OVERRIDE_TAG.to_string()));
    record.add_value(XDataValue::Integer32(end_index));
    record.add_value(XDataValue::String(json));
    xd.add_record(record);
    if let Some(ah) = app_handle {
        xd.raw_dwg_eed.retain(|(a, _)| *a != ah);
    }
    true
}

/// Read the [`join::JunctionOverride`] stored on `axis_handle` for
/// `end_index` (`0` = axis start, `1` = axis end). Returns `None` when no
/// such XDATA exists — including on entities from drawings created before
/// this feature existed — and never panics on missing/malformed data.
pub fn read_junction_override(
    scene: &Scene,
    axis_handle: Handle,
    end_index: usize,
) -> Option<join::JunctionOverride> {
    let entity = scene.document.get_entity(axis_handle)?;
    let end_index = join_override_end_key(end_index);
    entity
        .common()
        .extended_data
        .records()
        .iter()
        .find(|r| {
            r.application_name == AEC_APPID
                && matches!(
                    (r.values.first(), r.values.get(1)),
                    (Some(XDataValue::String(s)), Some(XDataValue::Integer32(e)))
                        if s == JOIN_OVERRIDE_TAG && *e == end_index
                )
        })
        .and_then(|r| match r.values.get(2) {
            Some(XDataValue::String(json)) => serde_json::from_str(json).ok(),
            _ => None,
        })
}

/// Erase the [`join::JunctionOverride`] XDATA (if any) for `end_index` from
/// `axis_handle`. Used to fully clean up a degenerate override (no
/// `default_style` and no valid `layer_pairs` left) instead of persisting an
/// empty/meaningless record via [`write_junction_override`]. Returns `true`
/// when a record was actually removed.
pub fn remove_junction_override(scene: &mut Scene, axis_handle: Handle, end_index: usize) -> bool {
    let end_index = join_override_end_key(end_index);
    let Some(entity) = scene.document.get_entity_mut(axis_handle) else {
        return false;
    };
    let xd = &mut entity.common_mut().extended_data;
    let before = xd.records().len();
    let kept: Vec<_> = xd
        .records()
        .iter()
        .filter(|r| {
            !(r.application_name == AEC_APPID
                && matches!(
                    (r.values.first(), r.values.get(1)),
                    (Some(XDataValue::String(s)), Some(XDataValue::Integer32(e)))
                        if s == JOIN_OVERRIDE_TAG && *e == end_index
                ))
        })
        .cloned()
        .collect();
    let changed = kept.len() != before;
    xd.clear();
    for r in kept {
        xd.add_record(r);
    }
    changed
}

/// Overwrites only the `height` field of a wall's `WALL` XDATA record,
/// keeping `style_id`/`layers`/`storey_id`/`derived_handles`/`justification` intact.
/// Used by the Properties panel's editable "Height" row (single or
/// multi-selected walls).
pub fn write_wall_height(scene: &mut Scene, wall_handle: Handle, height: f64) -> bool {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(mut wall) = wall_from_entity(entity) else {
        return false;
    };
    wall.height = height;
    wall.base_plane_id = None;
    wall.top_plane_id = None;
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    write_aec_record(&mut scene.document, wall_handle, record)
}

pub fn write_wall_plane_offsets(
    scene: &mut Scene,
    wall_handle: Handle,
    base_offset: Option<f64>,
    top_offset: Option<f64>,
) -> bool {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(mut wall) = wall_from_entity(entity) else {
        return false;
    };
    wall.apply_plane_offsets(base_offset, top_offset);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    let ok = write_aec_record(&mut scene.document, wall_handle, record);
    if ok {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity_mut(wall_handle) {
            pl.elevation = wall.base_origin[2];
        }
    }
    ok
}

/// Overwrites only the layer-snapshot portion of a wall's `WALL` XDATA record,
/// keeping `style_id`/`height`/`storey_id`/`derived_handles`/`justification` intact.
pub fn write_wall_layers(scene: &mut Scene, wall_handle: Handle, layers: Vec<WallLayer>) -> bool {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(mut wall) = wall_from_entity(entity) else {
        return false;
    };
    wall.layers = layers;
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    write_aec_record(&mut scene.document, wall_handle, record)
}

/// Overwrites only the `phase` field of a wall's `WALL` XDATA record,
/// keeping every other field intact. Used by the Properties panel's
/// editable "Phase" dropdown (single or multi-selected walls). Purely a
/// metadata edit — the wall's geometry is unaffected, so no regeneration
/// is triggered here.
pub fn write_wall_phase(scene: &mut Scene, wall_handle: Handle, phase: PlanPhase) -> bool {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(mut wall) = wall_from_entity(entity) else {
        return false;
    };
    wall.phase = phase;
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    write_aec_record(&mut scene.document, wall_handle, record)
}

/// Overwrites only the `hatch_override` field of a wall's `WALL` XDATA
/// record, keeping every other field intact. Used by the Properties
/// panel's optional "Relativ zur Wand" checkbox + angle field (Step 6
/// hatch-angle chain: `Wall.hatch_override` > style-profile override >
/// `Material`). Passing `None` clears any existing per-wall override.
/// Purely a metadata edit — the wall's geometry is unaffected, so no
/// regeneration is triggered here.
pub fn write_wall_hatch_override(
    scene: &mut Scene,
    wall_handle: Handle,
    hatch_override: Option<engine::display_component::ComponentStyleOverride>,
) -> bool {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(mut wall) = wall_from_entity(entity) else {
        return false;
    };
    wall.hatch_override = hatch_override;
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    write_aec_record(&mut scene.document, wall_handle, record)
}

/// Every display child of `owner` (contour / hatch / solid): the persisted
/// `derived_handles` list plus a document scan for `WALL_REP` / `WALL_DERIVED`.
pub(crate) fn aec_value_as_handle(value: &XDataValue) -> Option<Handle> {
    match value {
        XDataValue::Handle(h) => Some(*h),
        XDataValue::Integer32(v) if *v >= 0 => Some(Handle::new(*v as u64)),
        XDataValue::Integer16(v) if *v >= 0 => Some(Handle::new(*v as u64)),
        XDataValue::String(s) => {
            let s = s.trim().trim_start_matches("0x").trim_start_matches("0X");
            u64::from_str_radix(s, 16)
                .ok()
                .or_else(|| s.parse().ok())
                .map(Handle::new)
        }
        _ => None,
    }
}

/// Extracts vertices from a wall's axis polyline.
pub(crate) fn get_wall_vertices(scene: &Scene, handle: Handle) -> Vec<DVec3> {
    if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(handle) {
        pl.vertices.iter().map(|v| DVec3::new(v.location.x, v.location.y, pl.elevation)).collect()
    } else {
        Vec::new()
    }
}

pub(crate) fn get_wall_bulges(scene: &Scene, handle: Handle) -> Vec<f64> {
    if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(handle) {
        pl.vertices.iter().map(|v| v.bulge).collect()
    } else {
        Vec::new()
    }
}

/// Updates a wall's axis polyline vertices.
pub(crate) fn update_wall_vertices(scene: &mut Scene, handle: Handle, vertices: &[DVec3]) {
    if let Some(entity) = scene.document.get_entity_mut(handle) {
        if let EntityType::LwPolyline(pl) = entity {
            // Keep bulge / start-width when only endpoint positions change so
            // a join/extend cannot leave a 2D contour with stale arc data.
            if pl.vertices.len() == vertices.len() {
                let old: Vec<(f64, f64, f64)> = pl
                    .vertices
                    .iter()
                    .map(|v| (v.location.x, v.location.y, v.bulge))
                    .collect();
                for (dst, src) in pl.vertices.iter_mut().zip(vertices.iter()) {
                    dst.location = acadrust::types::Vector2::new(src.x, src.y);
                }
                let n = pl.vertices.len();
                for i in 0..n.saturating_sub(1) {
                    let bulge = old[i].2;
                    if bulge.abs() <= 1e-12 {
                        continue;
                    }
                    let new_b = engine::arc::retarget_bulge(
                        (old[i].0, old[i].1),
                        (old[i + 1].0, old[i + 1].1),
                        bulge,
                        (vertices[i].x, vertices[i].y),
                        (vertices[i + 1].x, vertices[i + 1].y),
                    );
                    pl.vertices[i].bulge = new_b;
                }
            } else {
                pl.vertices = vertices
                    .iter()
                    .map(|v| {
                        acadrust::entities::LwVertex::new(acadrust::types::Vector2::new(v.x, v.y))
                    })
                    .collect();
            }
        }
    }
}

/// Collect baseline segments of every `WALL`-tagged `LwPolyline`.
pub(crate) fn collect_wall_segments(doc: &CadDocument) -> Vec<((f64, f64), (f64, f64))> {
    let mut segments = Vec::new();
    for entity in doc.entities() {
        let EntityType::LwPolyline(pl) = entity else {
            continue;
        };
        let is_wall = matches!(
            read_aec_record(entity).and_then(|r| r.values.first()),
            Some(XDataValue::String(kind)) if kind == "WALL"
        );
        if !is_wall {
            continue;
        }
        for pair in pl.vertices.windows(2) {
            let a = (pair[0].location.x, pair[0].location.y);
            let b = (pair[1].location.x, pair[1].location.y);
            segments.push((a, b));
        }
        if pl.is_closed {
            if let (Some(first), Some(last)) = (pl.vertices.first(), pl.vertices.last()) {
                segments.push((
                    (last.location.x, last.location.y),
                    (first.location.x, first.location.y),
                ));
            }
        }
    }
    segments
}

/// Build a `WALL` XDATA record's values.
///
/// `derived_handles` is appended as a trailing `count` + `Handle` block so
/// records written before this field existed (no trailing block) still
/// parse back with an empty list.
pub fn wall_record(
    style_id: &str,
    height: f64,
    storey_id: u32,
    layers: &[WallLayer],
    derived_handles: &[Handle],
    justification: WallJustification,
    phase: PlanPhase,
    hatch_override: Option<&engine::display_component::ComponentStyleOverride>,
) -> Vec<XDataValue> {
    let mut values = Vec::new();
    values.push(XDataValue::String("WALL".to_string()));
    values.push(XDataValue::String(style_id.to_string()));
    values.push(XDataValue::Distance(height));
    values.push(XDataValue::Integer32(storey_id as i32));
    values.push(XDataValue::Integer32(layers.len() as i32));
    for layer in layers {
        values.push(XDataValue::String(layer.material.clone()));
        values.push(XDataValue::Distance(layer.thickness));
        values.push(XDataValue::String(layer.function.clone()));
    }
    values.push(XDataValue::Integer32(derived_handles.len() as i32));
    for h in derived_handles {
        values.push(XDataValue::Handle(*h));
    }
    values.push(XDataValue::String(justification.as_str().to_string()));

    // Schema marker distinguishing absolute axis_offset extras from legacy
    // gap_before stacking values. Absent on older records.
    values.push(XDataValue::String("axis_offset".to_string()));

    // Trailing layer extras: axis_offset, bottom_offset, top_offset for each layer
    for layer in layers {
        values.push(XDataValue::Distance(layer.axis_offset));
        values.push(XDataValue::Distance(layer.bottom_offset));
        values.push(XDataValue::Distance(layer.top_offset));
    }
    // Trailing layer_override (drawing-layer override) for each layer, as an
    // empty string standing for `None`; appended last so records written
    // before this field existed (no trailing block) still parse back with
    // every layer defaulting to `None`.
    for layer in layers {
        values.push(XDataValue::String(layer.layer_override.clone().unwrap_or_default()));
    }
    // Trailing hatch_override (per-layer hatch pattern override) for each
    // layer, same empty-string-for-`None` convention, appended after
    // `layer_override` so records written before this field existed still
    // parse back with every layer defaulting to `None`.
    for layer in layers {
        values.push(XDataValue::String(layer.hatch_override.clone().unwrap_or_default()));
    }
    // Trailing layer_id (stable identity) for each layer, preceded by a
    // tag so it can be reliably distinguished from other trailing extras.
    values.push(XDataValue::String("layer_id".to_string()));
    for layer in layers {
        values.push(XDataValue::String(layer.layer_id.to_string()));
    }
    // Trailing `phase` tag, appended last so records written before this
    // field existed still parse back defaulting to `PlanPhase::New`.
    values.push(XDataValue::String(phase.as_str().to_string()));
    // Trailing per-wall-instance `hatch_override` (Step 3 hatch-angle chain:
    // `Wall.hatch_override` > style-profile override > `Material`), appended
    // last so records written before this field existed still parse back
    // with `None`. Encoded as a presence flag followed by the two hatch
    // fields (angle in degrees as a `Distance`, relative-flag as a string),
    // each using a sentinel when unset (`f64::NAN` / empty string).
    match hatch_override {
        Some(ov) => {
            values.push(XDataValue::String("1".to_string()));
            values.push(XDataValue::Distance(ov.hatch_angle.unwrap_or(f64::NAN)));
            values.push(XDataValue::String(match ov.hatch_angle_relative {
                Some(true) => "true".to_string(),
                Some(false) => "false".to_string(),
                None => String::new(),
            }));
        }
        None => {
            values.push(XDataValue::String("0".to_string()));
            values.push(XDataValue::Distance(f64::NAN));
            values.push(XDataValue::String(String::new()));
        }
    }
    encode_wall_planes(&mut values, None);
    values
}

pub(crate) fn encode_wall_planes(values: &mut Vec<XDataValue>, wall: Option<&Wall>) {
    values.push(XDataValue::String("planes".to_string()));
    let (base_id, top_id, bo, to, bo_o, bn, to_o, tn) = if let Some(w) = wall {
        (
            w.base_plane_id,
            w.top_plane_id,
            w.base_offset,
            w.top_offset,
            w.base_origin,
            w.base_normal,
            w.top_origin,
            w.top_normal,
        )
    } else {
        (
            None,
            None,
            0.0,
            0.0,
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
        )
    };
    values.push(XDataValue::String(encode_plane_ref(base_id, wall.and_then(|w| w.base_plane_name.as_deref()))));
    values.push(XDataValue::String(encode_plane_ref(top_id, wall.and_then(|w| w.top_plane_name.as_deref()))));
    values.push(XDataValue::Distance(bo));
    values.push(XDataValue::Distance(to));
    for c in bo_o {
        values.push(XDataValue::Distance(c));
    }
    for c in bn {
        values.push(XDataValue::Distance(c));
    }
    for c in to_o {
        values.push(XDataValue::Distance(c));
    }
    for c in tn {
        values.push(XDataValue::Distance(c));
    }
}

pub(crate) fn encode_plane_ref(id: Option<uuid::Uuid>, name: Option<&str>) -> String {
    match (id, name.filter(|n| !n.is_empty())) {
        (Some(id), Some(name)) => format!("{id}|{name}"),
        (Some(id), None) => id.to_string(),
        (None, Some(name)) => format!("|{name}"),
        (None, None) => String::new(),
    }
}

pub(crate) fn parse_plane_ref(s: &str) -> (Option<uuid::Uuid>, Option<String>) {
    let s = s.trim();
    if s.is_empty() {
        return (None, None);
    }
    if let Some((id_part, name_part)) = s.split_once('|') {
        let id = if id_part.is_empty() {
            None
        } else {
            uuid::Uuid::parse_str(id_part.trim()).ok()
        };
        let name = if name_part.trim().is_empty() {
            None
        } else {
            Some(name_part.trim().to_string())
        };
        (id, name)
    } else {
        (
            uuid::Uuid::parse_str(s).ok(),
            None,
        )
    }
}

pub fn wall_record_for_wall(wall: &Wall) -> Vec<XDataValue> {
    let mut values = wall_record(
        &wall.style_id,
        wall.height,
        wall.storey_id,
        &wall.layers,
        &wall.derived_handles,
        wall.justification,
        wall.phase,
        wall.hatch_override.as_ref(),
    );
    // wall_record already appended empty planes; replace the tail.
    if let Some(pos) = values.iter().rposition(|v| {
        matches!(v, XDataValue::String(s) if s == "planes")
    }) {
        values.truncate(pos);
    }
    encode_wall_planes(&mut values, Some(wall));
    values
}

/// Parse a `WALL` XDATA record back into a [`Wall`].
pub fn wall_from_entity(entity: &EntityType) -> Option<Wall> {
    let record = read_aec_record(entity)?;
    let v = &record.values;
    if v.len() < 5 {
        return None;
    }
    let XDataValue::String(kind) = &v[0] else {
        return None;
    };
    if kind != "WALL" {
        return None;
    }

    let style_id = if let XDataValue::String(s) = &v[1] {
        s.clone()
    } else {
        return None;
    };
    let height = if let XDataValue::Distance(d) = v[2] {
        d
    } else {
        return None;
    };
    let storey_id = if let XDataValue::Integer32(i) = v[3] {
        i as u32
    } else {
        return None;
    };
    let layer_count = if let XDataValue::Integer32(i) = v[4] {
        i as usize
    } else {
        return None;
    };

    if v.len() < 5 + layer_count * 3 {
        return None;
    }

    let mut layers = Vec::with_capacity(layer_count);
    for i in 0..layer_count {
        let base = 5 + i * 3;
        let mat = if let XDataValue::String(s) = &v[base] {
            s.clone()
        } else {
            return None;
        };
        let thick = if let XDataValue::Distance(d) = v[base + 1] {
            d
        } else {
            return None;
        };
        let func = if let XDataValue::String(s) = &v[base + 2] {
            s.clone()
        } else {
            return None;
        };
        layers.push(WallLayer {
            material: mat,
            thickness: thick,
            function: func,
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            // Placeholder; overwritten below from the "layer_id" tag when
            // present, or assigned a fresh stable ID otherwise (legacy wall).
            layer_id: Uuid::nil(),
        });
    }

    // Trailing `derived_handles` block: absent on records written before
    // this field existed, so a missing/short tail just means "none".
    let tail = 5 + layer_count * 3;
    let mut derived_handles = Vec::new();
    let mut justification = WallJustification::Center;
    let mut phase = PlanPhase::default();
    let mut wall_hatch_override: Option<engine::display_component::ComponentStyleOverride> = None;
    let mut extras_applied = false;

    if v.len() > tail {
        if let XDataValue::Integer32(count) = v[tail] {
            let count = count.max(0) as usize;
            if v.len() >= tail + 1 + count {
                for i in 0..count {
                    if let XDataValue::Handle(h) = v[tail + 1 + i] {
                        derived_handles.push(h);
                    }
                }
                // Justification might follow derived handles
                if v.len() > tail + 1 + count {
                    if let XDataValue::String(s) = &v[tail + 1 + count] {
                        justification = WallJustification::from_str(s);
                    }

                    // Optional layer extras (axis_offset/gap + vertical offsets) might follow justification.
                    // New records insert a "axis_offset" schema marker before the triples;
                    // legacy records store gap_before and must be migrated to absolute offsets.
                    let mut extras_tail = tail + 1 + count + 1;
                    let mut legacy_gap_before = true;
                    if v.len() > extras_tail {
                        if let XDataValue::String(marker) = &v[extras_tail] {
                            if marker == "axis_offset" {
                                legacy_gap_before = false;
                                extras_tail += 1;
                            }
                        }
                    }
                    if v.len() >= extras_tail + layer_count * 3 {
                        let mut legacy_gaps = vec![0.0; layer_count];
                        for i in 0..layer_count {
                            let base = extras_tail + i * 3;
                            if let XDataValue::Distance(g) = v[base] {
                                if legacy_gap_before {
                                    legacy_gaps[i] = g;
                                } else {
                                    layers[i].axis_offset = g;
                                }
                            }
                            if let XDataValue::Distance(b) = v[base + 1] {
                                layers[i].bottom_offset = b;
                            }
                            if let XDataValue::Distance(tv) = v[base + 2] {
                                layers[i].top_offset = tv;
                            }
                        }
                        if legacy_gap_before {
                            let pairs: Vec<(f64, f64)> = layers
                                .iter()
                                .zip(legacy_gaps.iter())
                                .map(|(l, g)| (l.thickness, *g))
                                .collect();
                            let offsets = migrate_gap_before_to_axis_offset(&pairs);
                            for (layer, offset) in layers.iter_mut().zip(offsets) {
                                layer.axis_offset = offset;
                            }
                        }
                        extras_applied = true;

                        // Optional layer_override block might follow the axis_offset/offset extras.
                        let override_tail = extras_tail + layer_count * 3;
                        if v.len() >= override_tail + layer_count {
                            for i in 0..layer_count {
                                if let XDataValue::String(s) = &v[override_tail + i] {
                                    layers[i].layer_override =
                                        if s.is_empty() { None } else { Some(s.clone()) };
                                }
                            }

                            // Optional hatch_override block might follow layer_override.
                            let hatch_tail = override_tail + layer_count;
                            if v.len() >= hatch_tail + layer_count {
                                for i in 0..layer_count {
                                    if let XDataValue::String(s) = &v[hatch_tail + i] {
                                        layers[i].hatch_override =
                                            if s.is_empty() { None } else { Some(s.clone()) };
                                    }
                                }

                                // Search for optional "layer_id" tag in trailing values,
                                // and note how many slots it occupies (tag + one UUID
                                // string per layer) so the subsequent `phase` tag can
                                // be located at the correct offset. Absent on records
                                // written before this field existed.
                                let mut layer_id_block_len = 0usize;
                                if hatch_tail + layer_count < v.len() {
                                    if let XDataValue::String(s) = &v[hatch_tail + layer_count] {
                                        if s == "layer_id" && hatch_tail + layer_count + 1 + layer_count <= v.len() {
                                            for i in 0..layer_count {
                                                if let XDataValue::String(id_str) =
                                                    &v[hatch_tail + layer_count + 1 + i]
                                                {
                                                    if let Ok(id) = Uuid::parse_str(id_str) {
                                                        layers[i].layer_id = id;
                                                    }
                                                }
                                            }
                                            layer_id_block_len = 1 + layer_count;
                                        }
                                    }
                                }

                                // Optional trailing `phase` tag might follow the
                                // hatch_override block (and the layer_id block, if
                                // present). Absent on older records, which default
                                // to `PlanPhase::New`.
                                let phase_tail = hatch_tail + layer_count + layer_id_block_len;
                                if v.len() > phase_tail {
                                    if let XDataValue::String(s) = &v[phase_tail] {
                                        phase = PlanPhase::from_str(s);
                                    }

                                    // Optional trailing per-wall-instance
                                    // `hatch_override` block might follow `phase`
                                    // (presence flag + angle + relative-flag).
                                    // Absent on older records, which default to
                                    // `None` (no per-wall override).
                                    let wall_hatch_tail = phase_tail + 1;
                                    if v.len() >= wall_hatch_tail + 3 {
                                        if let XDataValue::String(flag) = &v[wall_hatch_tail] {
                                            if flag == "1" {
                                                let angle = if let XDataValue::Distance(a) = v[wall_hatch_tail + 1] {
                                                    if a.is_nan() { None } else { Some(a) }
                                                } else {
                                                    None
                                                };
                                                let relative = if let XDataValue::String(r) = &v[wall_hatch_tail + 2] {
                                                    match r.as_str() {
                                                        "true" => Some(true),
                                                        "false" => Some(false),
                                                        _ => None,
                                                    }
                                                } else {
                                                    None
                                                };
                                                wall_hatch_override = Some(engine::display_component::ComponentStyleOverride {
                                                    hatch_angle: angle,
                                                    hatch_angle_relative: relative,
                                                    ..Default::default()
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Records without layer extras predate both gap_before and axis_offset
    // fields; treat them as gap_before = 0 and migrate to a centered stack so
    // geometry stays identical to the historical default.
    if !extras_applied && !layers.is_empty() {
        let pairs: Vec<(f64, f64)> = layers.iter().map(|l| (l.thickness, 0.0)).collect();
        let offsets = migrate_gap_before_to_axis_offset(&pairs);
        for (layer, offset) in layers.iter_mut().zip(offsets) {
            layer.axis_offset = offset;
        }
    }

    // Assign new IDs to legacy layers that lack a stable identity from XDATA.
    for l in &mut layers {
        if l.layer_id.is_nil() {
            l.layer_id = Uuid::new_v4();
        }
    }

    let mut base_plane_id = None;
    let mut top_plane_id = None;
    let mut base_plane_name = None;
    let mut top_plane_name = None;
    let mut base_offset = 0.0;
    let mut top_offset = 0.0;
    let mut base_origin = [0.0, 0.0, 0.0];
    let mut base_normal = [0.0, 0.0, 1.0];
    let mut top_origin = [0.0, 0.0, 0.0];
    let mut top_normal = [0.0, 0.0, 1.0];
    if let Some(pos) = v
        .iter()
        .rposition(|x| matches!(x, XDataValue::String(s) if s == "planes"))
    {
        if v.len() >= pos + 1 + 2 + 2 + 12 {
            if let XDataValue::String(s) = &v[pos + 1] {
                let (id, name) = parse_plane_ref(s);
                base_plane_id = id;
                base_plane_name = name;
            }
            if let XDataValue::String(s) = &v[pos + 2] {
                let (id, name) = parse_plane_ref(s);
                top_plane_id = id;
                top_plane_name = name;
            }
            let xf64 = |val: &XDataValue| -> Option<f64> {
                match val {
                    XDataValue::Distance(d) | XDataValue::Real(d) | XDataValue::ScaleFactor(d) => {
                        Some(*d)
                    }
                    _ => None,
                }
            };
            if let Some(d) = xf64(&v[pos + 3]) {
                base_offset = d;
            }
            if let Some(d) = xf64(&v[pos + 4]) {
                top_offset = d;
            }
            let read3 = |at: usize, dest: &mut [f64; 3]| {
                for i in 0..3 {
                    if let Some(d) = xf64(&v[at + i]) {
                        dest[i] = d;
                    }
                }
            };
            read3(pos + 5, &mut base_origin);
            read3(pos + 8, &mut base_normal);
            read3(pos + 11, &mut top_origin);
            read3(pos + 14, &mut top_normal);
        }
    }

    Some(Wall {
        style_id,
        height,
        storey_id,
        layers,
        derived_handles,
        justification,
        phase,
        hatch_override: wall_hatch_override,
        base_plane_id,
        top_plane_id,
        base_plane_name,
        top_plane_name,
        base_offset,
        top_offset,
        base_origin,
        base_normal,
        top_origin,
        top_normal,
    })
}

/// Reconstruct a [`StyleLibrary`] from every wall in `scene`.
pub fn extract_style_library_from_scene(scene: &Scene) -> StyleLibrary {
    let walls: Vec<Wall> = scene
        .document
        .entities()
        .filter_map(wall_from_entity)
        .collect();
    engine::library::extract_style_library_from_walls(&walls)
}

/// Rewrite the `derived_handles` tail of `wall_handle`'s `WALL` record,
/// keeping every other field unchanged. No-op (returns `false`) if the
/// entity doesn't carry a `WALL` record.
pub fn set_wall_derived_handles(
    scene: &mut Scene,
    wall_handle: Handle,
    derived_handles: &[Handle],
) -> bool {
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(v2) = wall_from_entity(entity) else {
        return false;
    };
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    let mut wall = v2;
    wall.derived_handles = derived_handles.to_vec();
    record.values = wall_record_for_wall(&wall);
    write_aec_record(&mut scene.document, wall_handle, record)
}

/// Register the `OPENCAD_AEC` APPID up front so an interactive `AEC_WALL`
/// command can embed XDATA directly on entities it builds (it has no
/// `&mut CadDocument` while collecting points).
pub fn ensure_wall_app_id(doc: &mut CadDocument) {
    ensure_app_id(doc);
}

pub use crate::modules::aec::walls::wall::WallCommand;
pub use crate::modules::aec::walls::join::WallJoinCommand;
pub use crate::modules::aec::walls::extend::WallExtendCommand;
pub use crate::modules::aec::walls::extend::aec_wallextend_do;
pub use crate::modules::aec::walls::refresh::aec_wall_refresh;
pub use crate::modules::aec::walls::window::{WallOpeningCommand, aec_wallopening_do};
pub use crate::modules::aec::rooms::room::aec_room;
pub use crate::modules::aec::rooms::schedule::aec_room_schedule;
pub use crate::modules::aec::styles::material_manager::{MaterialCommand, aec_material_add};
pub use crate::modules::aec::styles::wall_style_manager::{StyleCommand, aec_style_add};
pub use crate::modules::aec::project::storeys::aec_storey;
pub use crate::modules::aec::ifc::export::aec_ifc_export;
pub use crate::modules::aec::walls::reverse::{WallReverseCommand, aec_wallreverse_do};

// WallCommand moved to walls/wall.rs


/// Converts a [`LayerFunction`] to the plain string used in XDATA/dispatch.
pub(crate) fn layer_function_to_str(f: &LayerFunction) -> String {
    match f {
        LayerFunction::Structural => "Structural".to_string(),
        LayerFunction::Insulation => "Insulation".to_string(),
        LayerFunction::Finish => "Finish".to_string(),
        LayerFunction::Other(s) => s.clone(),
    }
}

/// Resolve a wall style's effective layers into concrete [`WallLayer`]s.
///
/// Formula thicknesses are evaluated with `"BB"` = `bb` when provided, otherwise
/// the sum of fixed layer thicknesses (+ gaps) from the style
/// ([`base_width_from_layers`]). Invalid formulas fall back to `0.0` thickness
/// (see [`ResolvedLayer::formula_error`]).
///
/// When `use_material_names` is true, material fields store the library display
/// name; otherwise the raw material id is kept (properties/style-picker path).
pub fn resolve_wall_style_layers(
    lib: &StyleLibrary,
    style_id: &str,
    bb: Option<f64>,
) -> Option<Vec<WallLayer>> {
    resolve_wall_style_layers_ex(lib, style_id, bb, true)
}

/// Same as [`resolve_wall_style_layers`] but keeps material ids instead of names.
pub fn resolve_wall_style_layers_ids(
    lib: &StyleLibrary,
    style_id: &str,
    bb: Option<f64>,
) -> Option<Vec<WallLayer>> {
    resolve_wall_style_layers_ex(lib, style_id, bb, false)
}

pub(crate) fn resolve_wall_style_layers_ex(
    lib: &StyleLibrary,
    style_id: &str,
    bb: Option<f64>,
    use_material_names: bool,
) -> Option<Vec<WallLayer>> {
    let style_map: HashMap<String, WallStyle> = lib
        .wall_styles
        .iter()
        .map(|s| (s.style.id.clone(), s.clone()))
        .collect();

    // Need unresolved layers first when BB is not supplied.
    let unresolved = crate::modules::aec::engine::wall_style::effective_layers(&style_map, &style_id.to_string())
        .ok()?;
    let bb = bb.unwrap_or_else(|| base_width_from_layers(&unresolved));
    let resolved = effective_layers_for_wall_bb(&style_map, &style_id.to_string(), bb).ok()?;

    Some(
        resolved
            .into_iter()
            .map(|layer| {
                if use_material_names {
                    resolved_layer_to_wall_layer(lib, layer)
                } else {
                    resolved_layer_to_wall_layer_raw(layer)
                }
            })
            .collect(),
    )
}

/// Map a [`ResolvedLayer`] to a runtime [`WallLayer`], preferring the library
/// material display name when available.
pub(crate) fn resolved_layer_to_wall_layer(lib: &StyleLibrary, layer: ResolvedLayer) -> WallLayer {
    let mat_name = lib
        .materials
        .iter()
        .find(|m| m.id == layer.material_id)
        .map(|m| m.name.clone())
        .unwrap_or_else(|| layer.material_id.clone());
    WallLayer {
        material: mat_name,
        thickness: layer.thickness,
        function: layer_function_to_str(&layer.function),
        axis_offset: layer.axis_offset,
        bottom_offset: layer.bottom_offset,
        top_offset: layer.top_offset,
        layer_override: layer.layer_override,
        hatch_override: layer.hatch_override,
        layer_id: layer.layer_id,
    }
}

/// Like [`resolved_layer_to_wall_layer`] but keeps `material_id` as the material
/// string (used by the properties/style-picker paths that store ids).
pub(crate) fn resolved_layer_to_wall_layer_raw(layer: ResolvedLayer) -> WallLayer {
    WallLayer {
        material: layer.material_id,
        thickness: layer.thickness,
        function: layer_function_to_str(&layer.function),
        axis_offset: layer.axis_offset,
        bottom_offset: layer.bottom_offset,
        top_offset: layer.top_offset,
        layer_override: layer.layer_override,
        hatch_override: layer.hatch_override,
        layer_id: layer.layer_id,
    }
}

/// Parses a plain string (as entered on the command line) into a
/// [`LayerFunction`], defaulting to `Structural` for empty input and falling
/// back to `Other(..)` for anything unrecognized.
pub(crate) fn parse_layer_function(s: &str) -> LayerFunction {
    match s.trim() {
        "" | "Structural" | "structural" => LayerFunction::Structural,
        "Insulation" | "insulation" => LayerFunction::Insulation,
        "Finish" | "finish" => LayerFunction::Finish,
        other => LayerFunction::Other(other.to_string()),
    }
}

/// Turns a human-entered name into a stable, filesystem/XDATA-safe id
/// (lowercase, non-alphanumeric runs collapsed to `_`).
pub(crate) fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut last_was_sep = true; // suppress a leading separator
    for c in name.trim().chars() {
        if c.is_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            slug.push('_');
            last_was_sep = true;
        }
    }
    while slug.ends_with('_') {
        slug.pop();
    }
    if slug.is_empty() {
        "item".to_string()
    } else {
        slug
    }
}

/// Builds a new, globally unique library id (`<prefix>_<slug>_<suffix>`) for
/// a freshly created material/wall style.
///
/// Ids used to be purely name-derived (`<prefix>_<slugified name>`), which
/// meant two libraries created independently (e.g. in different projects)
/// could end up with the *same* id for wall styles/materials that merely
/// share a name but have different layer definitions. If those libraries
/// are later mixed (project copied, wall pasted across drawings, ...), the
/// wrong entry silently wins on lookup. Appending a short random suffix
/// keeps ids human-readable while making cross-project collisions
/// practically impossible; existing ids (loaded from disk, or passed in
/// when editing) are left untouched so old data keeps resolving correctly.
pub(crate) fn unique_id(prefix: &str, name: &str) -> String {
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    format!("{prefix}_{}_{}", slugify(name), &suffix[..8])
}
