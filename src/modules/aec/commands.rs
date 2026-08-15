//! AEC commands — wall/room/storey creation, room schedule, IFC export.
//!
//! `AEC_WALL` is an interactive multi-point drawing command (analogous to
//! `PLINE`); the rest remain non-interactive scaffold commands (matching the
//! former plugin's pragmatic behaviour) that operate directly on `&mut Scene`
//! / the document and report feedback via the command line.

use std::sync::Mutex;

use acadrust::entities::{LwPolyline, LwVertex, Table};
use acadrust::tables::AppId;
use acadrust::types::{Vector2, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, WorkingPlane};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

use super::engine::{
    self, find_closed_loop, Room, Storey, StyleLibrary, Wall,
    join::{self, JoinKind, JoinError},
};
use super::engine::library::load_or_seed;
use super::engine::material::Material;
use super::engine::style::Style;
use super::engine::wall_style::{effective_layers, Layer, LayerFunction, WallStyle};
use std::collections::HashMap;

/// APPID used for all AEC XDATA records (must stay stable for round-trip).
pub const AEC_APPID: &str = "OPENCAD_AEC";

/// In-memory storey store for the scaffold (persistence via document XDATA
/// is a follow-up; matches the former plugin's pragmatism).
static STOREYS: Mutex<Vec<Storey>> = Mutex::new(Vec::new());

/// Register `OPENCAD_AEC` in the APPID table if missing so XDATA survives
/// DWG/DXF round-trip.
fn ensure_app_id(doc: &mut CadDocument) {
    if !doc.app_ids.contains(AEC_APPID) {
        let mut app = AppId::new(AEC_APPID);
        app.handle = doc.allocate_handle();
        let _ = doc.app_ids.add(app);
    }
}

/// Attach (or replace) an `OPENCAD_AEC` XDATA record on `handle`.
fn write_aec_record(doc: &mut CadDocument, handle: Handle, record: ExtendedDataRecord) -> bool {
    ensure_app_id(doc);
    let app_handle = doc.app_ids.get(AEC_APPID).map(|a| a.handle.value());
    let Some(entity) = doc.get_entity_mut(handle) else {
        return false;
    };
    let xd = &mut entity.common_mut().extended_data;
    let kept: Vec<_> = xd
        .records()
        .iter()
        .filter(|r| r.application_name != AEC_APPID)
        .cloned()
        .collect();
    xd.clear();
    for r in kept {
        xd.add_record(r);
    }
    xd.add_record(record);
    if let Some(ah) = app_handle {
        xd.raw_dwg_eed.retain(|(a, _)| *a != ah);
    }
    true
}

/// Read the `OPENCAD_AEC` record on `entity`, if any.
fn read_aec_record(entity: &EntityType) -> Option<&ExtendedDataRecord> {
    entity.common().extended_data.get_record(AEC_APPID)
}

/// Tag `handle` as a "derived" entity of the wall axis at `axis_handle`
/// (contour/hatch/solid entities created by [`regenerate_wall_representation`]).
/// Used by [`resolve_wall_package`] to resolve a click on a wall's visible
/// representation back to the underlying axis entity for selection/move/grip
/// editing.
fn write_wall_derived_tag(scene: &mut Scene, handle: Handle, axis_handle: Handle) {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("WALL_DERIVED".to_string()));
    record.add_value(XDataValue::Handle(axis_handle));
    write_aec_record(&mut scene.document, handle, record);
}

/// Overwrites only the `height` field of a wall's `WALL_V2` XDATA record,
/// keeping `style_id`/`layers`/`storey_id`/`derived_handles`/`justification` intact.
/// Used by the Properties panel's editable "Height" row (single or
/// multi-selected walls).
pub fn write_wall_v2_height(scene: &mut Scene, wall_handle: Handle, height: f64) -> bool {
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(mut wall_v2) = wall_v2_from_entity(entity) else {
        return false;
    };
    wall_v2.height = height;
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    for v in wall_v2_record(
        &wall_v2.style_id,
        wall_v2.height,
        wall_v2.storey_id,
        &wall_v2.layers,
        &wall_v2.derived_handles,
        wall_v2.justification,
    ) {
        record.add_value(v);
    }
    write_aec_record(&mut scene.document, wall_handle, record)
}

/// Overwrites only the layer-snapshot portion of a wall's `WALL_V2` XDATA record,
/// keeping `style_id`/`height`/`storey_id`/`derived_handles`/`justification` intact.
pub fn write_wall_v2_layers(scene: &mut Scene, wall_handle: Handle, layers: Vec<WallLayer>) -> bool {
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(mut wall_v2) = wall_v2_from_entity(entity) else {
        return false;
    };
    wall_v2.layers = layers;
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    for v in wall_v2_record(
        &wall_v2.style_id,
        wall_v2.height,
        wall_v2.storey_id,
        &wall_v2.layers,
        &wall_v2.derived_handles,
        wall_v2.justification,
    ) {
        record.add_value(v);
    }
    write_aec_record(&mut scene.document, wall_handle, record)
}

/// Resolves `clicked` to the wall axis handle it belongs to, if `clicked`
/// carries a `WALL_DERIVED` XDATA record (i.e. it's a contour/hatch/solid
/// entity generated by [`regenerate_wall_representation`]). Non-wall
/// entities, or derived entities whose axis handle no longer exists in the
/// document, resolve to `clicked` unchanged.
pub fn resolve_wall_package(scene: &Scene, clicked: Handle) -> Handle {
    let Some(entity) = scene.document.get_entity(clicked) else {
        return clicked;
    };
    let Some(record) = read_aec_record(entity) else {
        return clicked;
    };
    match record.values.as_slice() {
        [XDataValue::String(kind), XDataValue::Handle(axis)] if kind == "WALL_DERIVED" => {
            if scene.document.get_entity(*axis).is_some() {
                *axis
            } else {
                clicked
            }
        }
        _ => clicked,
    }
}

/// True when `handle` is a wall-derived contour/hatch/solid entity — i.e. a
/// visible-layer entity generated by [`regenerate_wall_representation`] that
/// carries a `WALL_DERIVED` XDATA record pointing back at its axis. Used by
/// [`wall_axis_snap_wires`] to keep such entities out of the generic snap
/// candidate set (Bug 2): walls must always connect at their axes, never at
/// a shell/hatch/solid outline.
fn is_wall_derived_non_axis(scene: &Scene, handle: Handle) -> bool {
    resolve_wall_package(scene, handle) != handle
}

/// True when `handle` is the wall axis itself — the (normally invisible,
/// `AEC_WALL_AXIS_LAYER`) `LwPolyline` carrying the `WALL`/`WALL_V2` XDATA
/// record. Axis entities must remain snap candidates even though their
/// layer is turned off (Bug 2).
fn is_wall_axis_entity(entity: &EntityType) -> bool {
    entity.common().layer == AEC_WALL_AXIS_LAYER
        && (wall_v2_from_entity(entity).is_some() || wall_from_entity(entity).is_some())
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

/// Extracts vertices from a wall's axis polyline.
fn get_wall_vertices(scene: &Scene, handle: Handle) -> Vec<DVec3> {
    if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(handle) {
        pl.vertices.iter().map(|v| DVec3::new(v.location.x, v.location.y, 0.0)).collect()
    } else {
        Vec::new()
    }
}

/// Updates a wall's axis polyline vertices.
fn update_wall_vertices(scene: &mut Scene, handle: Handle, vertices: &[DVec3]) {
    if let Some(entity) = scene.document.get_entity_mut(handle) {
        if let EntityType::LwPolyline(pl) = entity {
            pl.vertices = vertices
                .iter()
                .map(|v| acadrust::entities::LwVertex::new(acadrust::types::Vector2::new(v.x, v.y)))
                .collect();
        }
    }
}

/// Collect baseline segments of every `WALL`-tagged `LwPolyline`.
fn collect_wall_segments(doc: &CadDocument) -> Vec<((f64, f64), (f64, f64))> {
    let mut segments = Vec::new();
    for entity in doc.entities() {
        let EntityType::LwPolyline(pl) = entity else {
            continue;
        };
        let is_wall = matches!(
            read_aec_record(entity).and_then(|r| r.values.first()),
            Some(XDataValue::String(kind)) if kind == "WALL" || kind == "WALL_V2"
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

/// Build the `WALL` XDATA record for `wall` (shared by the interactive draw
/// command and the properties-panel edit path).
fn wall_record(wall: &Wall) -> ExtendedDataRecord {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("WALL".to_string()));
    record.add_value(XDataValue::Distance(wall.thickness));
    record.add_value(XDataValue::Distance(wall.height));
    record.add_value(XDataValue::String(
        wall.material_ref.clone().unwrap_or_default(),
    ));
    record.add_value(XDataValue::Integer32(wall.storey_id as i32));
    record
}

/// Layer name used for the (invisible) wall axis / centerline reference
/// geometry. `AEC_ROOM` / loop-detection and the `WALL_V2` XDATA carrier keep
/// living on this layer once the visible contour/hatch/solid representation
/// is regenerated.
pub const AEC_WALL_AXIS_LAYER: &str = "AEC_WALL_AXIS";

/// In-memory representation of a `WALL_V2` record.
#[derive(Debug, Clone, PartialEq)]
pub struct WallV2 {
    pub style_id: String,
    pub height: f64,
    pub storey_id: u32,
    pub layers: Vec<WallLayer>,
    /// Handles of the contour/hatch/solid entities most recently derived
    /// from this wall's axis, so they can be cleanly replaced or removed.
    pub derived_handles: Vec<Handle>,
    /// Informational field: which justification was used when drawing.
    pub justification: WallJustification,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WallLayer {
    pub material: String,
    pub thickness: f64,
    pub function: String,
    pub gap_before: f64,
    pub bottom_offset: f64,
    pub top_offset: f64,
    pub layer_override: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallJustification {
    Interior,
    Center,
    Exterior,
}

impl WallJustification {
    pub fn next(self) -> Self {
        match self {
            WallJustification::Interior => WallJustification::Center,
            WallJustification::Center => WallJustification::Exterior,
            WallJustification::Exterior => WallJustification::Interior,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            WallJustification::Interior => "Interior",
            WallJustification::Center => "Center",
            WallJustification::Exterior => "Exterior",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "Interior" => WallJustification::Interior,
            "Exterior" => WallJustification::Exterior,
            _ => WallJustification::Center,
        }
    }

    pub fn offset(&self, total_thickness: f64) -> f64 {
        match self {
            WallJustification::Center => 0.0,
            WallJustification::Interior => total_thickness * -0.5,
            WallJustification::Exterior => total_thickness * 0.5,
        }
    }
}

impl WallV2 {
    pub fn total_thickness(&self) -> f64 {
        self.layers.iter().map(|l| l.thickness + l.gap_before).sum()
    }
}

/// Build a `WALL_V2` XDATA record's values.
///
/// `derived_handles` is appended as a trailing `count` + `Handle` block so
/// records written before this field existed (no trailing block) still
/// parse back with an empty list.
pub fn wall_v2_record(
    style_id: &str,
    height: f64,
    storey_id: u32,
    layers: &[WallLayer],
    derived_handles: &[Handle],
    justification: WallJustification,
) -> Vec<XDataValue> {
    let mut values = Vec::new();
    values.push(XDataValue::String("WALL_V2".to_string()));
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

    // Trailing layer extras: gap_before, bottom_offset, top_offset for each layer
    for layer in layers {
        values.push(XDataValue::Distance(layer.gap_before));
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
    values
}

/// Parse a `WALL_V2` XDATA record back into a [`WallV2`].
pub fn wall_v2_from_entity(entity: &EntityType) -> Option<WallV2> {
    let record = read_aec_record(entity)?;
    let v = &record.values;
    if v.len() < 5 {
        return None;
    }
    let XDataValue::String(kind) = &v[0] else {
        return None;
    };
    if kind != "WALL_V2" {
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
            gap_before: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
        });
    }

    // Trailing `derived_handles` block: absent on records written before
    // this field existed, so a missing/short tail just means "none".
    let tail = 5 + layer_count * 3;
    let mut derived_handles = Vec::new();
    let mut justification = WallJustification::Center;

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

                    // Optional layer extras (gaps and offsets) might follow justification
                    let extras_tail = tail + 1 + count + 1;
                    if v.len() >= extras_tail + layer_count * 3 {
                        for i in 0..layer_count {
                            let base = extras_tail + i * 3;
                            if let XDataValue::Distance(g) = v[base] {
                                layers[i].gap_before = g;
                            }
                            if let XDataValue::Distance(b) = v[base + 1] {
                                layers[i].bottom_offset = b;
                            }
                            if let XDataValue::Distance(t) = v[base + 2] {
                                layers[i].top_offset = t;
                            }
                        }

                        // Optional layer_override block might follow the gap/offset extras.
                        let override_tail = extras_tail + layer_count * 3;
                        if v.len() >= override_tail + layer_count {
                            for i in 0..layer_count {
                                if let XDataValue::String(s) = &v[override_tail + i] {
                                    layers[i].layer_override =
                                        if s.is_empty() { None } else { Some(s.clone()) };
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Some(WallV2 {
        style_id,
        height,
        storey_id,
        layers,
        derived_handles,
        justification,
    })
}

/// Unified helper to get total thickness, height, and storey_id for any wall entity
/// (supports both `WALL` and `WALL_V2`).
pub fn wall_thickness_and_height(entity: &EntityType) -> Option<(f64, f64, u32)> {
    let record = read_aec_record(entity)?;
    match record.values.first() {
        Some(XDataValue::String(kind)) if kind == "WALL" => {
            let wall = wall_from_entity(entity)?;
            Some((wall.thickness, wall.height, wall.storey_id))
        }
        Some(XDataValue::String(kind)) if kind == "WALL_V2" => {
            let wall = wall_v2_from_entity(entity)?;
            Some((wall.total_thickness(), wall.height, wall.storey_id))
        }
        _ => None,
    }
}

/// Parameters for a 3D extrusion of a wall layer.
///
/// Contains the 2D footprint (a closed polygon loop) and the height
/// to extrude it by.
#[derive(Debug, Clone, PartialEq)]
pub struct WallLayerExtrusion {
    pub footprint: Vec<(f64, f64)>,
    pub height: f64,
    pub base_offset: f64,
}

/// Extracts a wall's centerline points from its [`LwPolyline`] geometry and
/// computes parallel boundary lines for each layer.
///
/// Returns N+1 boundary lines for N layers.
pub fn wall_layer_contour_polylines(
    wall_entity: &EntityType,
    layers: &[WallLayer],
) -> Vec<(Vec<(f64, f64)>, Vec<(f64, f64)>)> {
    let EntityType::LwPolyline(pl) = wall_entity else {
        return Vec::new();
    };
    let centerline: Vec<(f64, f64)> = pl
        .vertices
        .iter()
        .map(|v| (v.location.x, v.location.y))
        .collect();
    let layer_data: Vec<(f64, f64)> = layers.iter().map(|l| (l.thickness, l.gap_before)).collect();
    engine::contour::layer_contours(&centerline, &layer_data)
}

/// Produces the parameters needed to create an extruded solid for each wall layer.
///
/// This implementation uses the "layer footprint" approach: it builds a closed
/// 2D polygon per layer by combining consecutive boundary offsets and returns
/// it along with the wall height.
///
/// Scoping Decision: This function returns plain data ([`WallLayerExtrusion`]).
/// A future step can wire this to the host's `Solid3D` entity creation calls
/// (e.g., using `sweep_model::extruded`).
pub fn wall_layer_extrusions(
    wall_entity: &EntityType,
    layers: &[WallLayer],
    height: f64,
) -> Vec<WallLayerExtrusion> {
    let boundaries = wall_layer_contour_polylines(wall_entity, layers);
    if boundaries.is_empty() {
        return Vec::new();
    }

    let mut extrusions = Vec::with_capacity(layers.len());
    for (i, (b1, b2)) in boundaries.into_iter().enumerate() {
        // Create a closed loop: forward along b1, then backward along b2.
        let mut footprint = Vec::with_capacity(b1.len() + b2.len());
        footprint.extend(b1.iter().cloned());
        footprint.extend(b2.iter().rev().cloned());

        let layer = &layers[i];
        let effective_height = (height - layer.bottom_offset - layer.top_offset).max(0.0);
        let base_offset = layer.bottom_offset;

        extrusions.push(WallLayerExtrusion {
            footprint,
            height: effective_height,
            base_offset,
        });
    }
    extrusions
}

/// Parse a `WALL` XDATA record back into a [`Wall`] (inverse of
/// [`wall_record`]). Returns `None` if `entity` isn't `WALL`-tagged or the
/// record doesn't have the expected shape.
pub fn wall_from_entity(entity: &EntityType) -> Option<Wall> {
    let record = read_aec_record(entity)?;
    match record.values.as_slice() {
        [XDataValue::String(kind), XDataValue::Distance(thickness), XDataValue::Distance(height), XDataValue::String(material), XDataValue::Integer32(storey_id)]
            if kind == "WALL" =>
        {
            Some(Wall {
                thickness: *thickness,
                height: *height,
                material_ref: if material.is_empty() {
                    None
                } else {
                    Some(material.clone())
                },
                storey_id: *storey_id as u32,
            })
        }
        [XDataValue::String(kind), ..] if kind == "WALL_V2" => {
            let v2 = wall_v2_from_entity(entity)?;
            Some(Wall {
                thickness: v2.total_thickness(),
                height: v2.height,
                material_ref: v2.layers.first().map(|l| l.material.clone()),
                storey_id: v2.storey_id,
            })
        }
        _ => None,
    }
}

/// Write `wall` back into `handle`'s `WALL` XDATA record, replacing the
/// previous one (used by the properties-panel edit path). Reuses
/// [`wall_record`] so the field layout stays in one place.
pub fn write_wall_properties(doc: &mut CadDocument, handle: Handle, wall: &Wall) -> bool {
    write_aec_record(doc, handle, wall_record(wall))
}

/// Register the `AEC_WALL_AXIS` layer (invisible / non-printable) if it
/// isn't already in the document's layer table.
pub fn ensure_wall_axis_layer(scene: &mut Scene) {
    scene.ensure_layer(AEC_WALL_AXIS_LAYER);
    if let Some(layer) = scene.document.layers.get_mut(AEC_WALL_AXIS_LAYER) {
        layer.is_plottable = false;
        layer.flags.off = true;
    }
}

/// Pack a single closed 2D ring into a [`HatchModel`]'s relative-boundary
/// representation (anchor at the ring's first vertex, f32 offsets from it),
/// mirroring the shape `pack_rings` builds in `draw/hatch.rs` for a single
/// loop with no holes.
fn pack_wall_ring(ring: &[(f64, f64)]) -> (Vec<[f32; 2]>, [f64; 2], Vec<[f64; 2]>) {
    let origin = ring.first().copied().unwrap_or((0.0, 0.0));
    let origin = [origin.0, origin.1];
    let rel: Vec<[f32; 2]> = ring
        .iter()
        .map(|&(x, y)| [(x - origin[0]) as f32, (y - origin[1]) as f32])
        .collect();
    let wcs: Vec<[f64; 2]> = ring.iter().map(|&(x, y)| [x, y]).collect();
    (rel, origin, wcs)
}

/// Convert a DXF-style `0xRRGGBB` color into a normalized RGBA color with
/// the alpha the AEC hatch representation uses.
fn wall_hatch_color(rgb: u32) -> [f32; 4] {
    let r = ((rgb >> 16) & 0xFF) as f32 / 255.0;
    let g = ((rgb >> 8) & 0xFF) as f32 / 255.0;
    let b = (rgb & 0xFF) as f32 / 255.0;
    [r, g, b, 0.85]
}

/// Error returned by [`regenerate_wall_representation`]; never a panic —
/// malformed axis/XDATA just leaves the wall without a rebuilt
/// representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallRegenError {
    /// `wall_handle` doesn't resolve to an entity carrying `WALL`/`WALL_V2`
    /// XDATA.
    NotAWall,
    /// The wall has no material layers to build a representation from.
    NoLayers,
    /// The axis geometry didn't yield usable layer contours (e.g. fewer
    /// than two vertices).
    NoContours,
}

/// (Re)build the visible 2D contour + hatch and 3D solid representation for
/// the wall at `wall_handle`, from its axis polyline + `WALL`/`WALL_V2`
/// XDATA.
///
/// The axis polyline is moved onto the invisible `AEC_WALL_AXIS` layer (kept
/// as reference geometry for `AEC_ROOM` / loop detection and as the XDATA
/// carrier). Every entity handle previously recorded in `derived_handles` is
/// erased first, so calling this repeatedly on the same wall never
/// accumulates duplicates. `WALL` (v1) walls are treated as a single
/// structural layer built from their `thickness`/`material_ref`; only
/// `WALL_V2` walls persist the new derived handles (v1 records have no slot
/// for them, so their representation is rebuilt but not tracked across
/// calls).
pub fn regenerate_wall_representation(
    scene: &mut Scene,
    wall_handle: Handle,
) -> Result<(), WallRegenError> {
    regenerate_wall_representation_with_corner(scene, wall_handle, None)
}

/// Like [`regenerate_wall_representation`], but lets a caller supply a
/// corner-extension hint: `(vertex_index, extended_position)` moves one axis
/// vertex further out — past a joined corner and into the other wall's
/// footprint — for contour/hatch/solid generation only. The persisted axis
/// polyline (and therefore `AEC_ROOM` loop detection, which depends on the
/// exact trimmed corner) is left untouched; only the *visible representation*
/// uses the extended point.
///
/// Used by [`join_two_walls_in_document`] (Bug 4) so an L/T join's two walls
/// overlap into the shared corner instead of leaving a seam where their
/// independently-capped rectangles merely touch.
pub fn regenerate_wall_representation_with_corner(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
) -> Result<(), WallRegenError> {
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return Err(WallRegenError::NotAWall);
    };

    let (layers, height, old_derived, is_v2) = if let Some(v2) = wall_v2_from_entity(entity) {
        (v2.layers, v2.height, v2.derived_handles, true)
    } else if let Some(wall) = wall_from_entity(entity) {
        let mat = wall.material_ref.clone().unwrap_or_default();
        (
            vec![WallLayer {
                material: mat,
                thickness: wall.thickness,
                function: "Structural".to_string(),
                gap_before: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
            }],
            wall.height,
            Vec::new(),
            false,
        )
    } else {
        return Err(WallRegenError::NotAWall);
    };

    if layers.is_empty() {
        return Err(WallRegenError::NoLayers);
    }

    // Clean up whatever this wall derived last time before rebuilding.
    if !old_derived.is_empty() {
        scene.erase_entities(&old_derived);
    }

    // The axis is reference geometry from here on: invisible, its own layer.
    ensure_wall_axis_layer(scene);
    if let Some(e) = scene.document.get_entity_mut(wall_handle) {
        e.as_entity_mut().set_layer(AEC_WALL_AXIS_LAYER.to_string());
    }

    let axis_entity = scene
        .document
        .get_entity(wall_handle)
        .cloned()
        .ok_or(WallRegenError::NotAWall)?;

    // The visible representation is built from a (possibly) corner-extended
    // copy of the axis; the real axis entity above stays untouched.
    let mut contour_axis_entity = axis_entity.clone();
    if let (Some((idx, pos)), EntityType::LwPolyline(pl)) =
        (corner_override, &mut contour_axis_entity)
    {
        if let Some(v) = pl.vertices.get_mut(idx) {
            v.location = Vector2::new(pos.x, pos.y);
        }
    }

    let contours = wall_layer_contour_polylines(&contour_axis_entity, &layers);
    if contours.is_empty() {
        if is_v2 {
            let _ = set_wall_v2_derived_handles(scene, wall_handle, &[]);
        }
        return Err(WallRegenError::NoContours);
    }
    let extrusions = wall_layer_extrusions(&contour_axis_entity, &layers, height);
    let library = load_or_seed();

    let mut new_derived: Vec<Handle> = Vec::new();
    for (i, layer) in layers.iter().enumerate() {
        let (b1, b2) = &contours[i];
        let mat_name = &layer.material;
        let mut footprint: Vec<(f64, f64)> = Vec::with_capacity(b1.len() + b2.len());
        footprint.extend(b1.iter().copied());
        footprint.extend(b2.iter().rev().copied());
        if footprint.len() < 3 {
            continue;
        }

        // Visible closed contour polyline for this layer.
        let mut pl = LwPolyline::new();
        for &(x, y) in &footprint {
            pl.add_vertex(LwVertex::new(Vector2::new(x, y)));
        }
        pl.is_closed = true;
        let contour_entity = EntityType::LwPolyline(pl);
        let contour_handle = scene.add_entity(contour_entity.clone());
        if let Some(layer_name) = layer.layer_override.as_deref().filter(|s| !s.is_empty()) {
            scene.ensure_layer(layer_name);
            if let Some(e) = scene.document.get_entity_mut(contour_handle) {
                e.as_entity_mut().set_layer(layer_name.to_string());
            }
        }
        write_wall_derived_tag(scene, contour_handle, wall_handle);
        new_derived.push(contour_handle);

        // Material-driven hatch over the same footprint.
        let material = library.materials.iter().find(|m| &m.id == mat_name);
        let pattern_name = material
            .map(|m| m.hatch_pattern.clone())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| "ANSI31".to_string());
        let color = material
            .map(|m| wall_hatch_color(m.line_color))
            .unwrap_or([0.6, 0.6, 0.6, 0.85]);
        let (rel, origin, wcs) = pack_wall_ring(&footprint);
        let families = crate::scene::model::hatch_patterns::find(&pattern_name)
            .and_then(|e| {
                if let crate::scene::model::hatch_model::HatchPattern::Pattern(f) = &e.gpu {
                    Some(f.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let hatch_model = crate::scene::model::hatch_model::HatchModel {
            boundary: std::sync::Arc::new(rel),
            pattern: crate::scene::model::hatch_model::HatchPattern::Pattern(families),
            name: pattern_name,
            color,
            aci: 0,
            line_weight_px: 1.0,
            angle_offset: 0.0,
            scale: 1.0,
            world_origin: origin,
            boundary_wcs: Some(std::sync::Arc::new(wcs)),
            draw_depth: 0.0,
        };
        let hatch_handle = scene.add_hatch(hatch_model);
        if let Some(layer_name) = layer.layer_override.as_deref().filter(|s| !s.is_empty()) {
            scene.ensure_layer(layer_name);
            if let Some(e) = scene.document.get_entity_mut(hatch_handle) {
                e.as_entity_mut().set_layer(layer_name.to_string());
            }
        }
        write_wall_derived_tag(scene, hatch_handle, wall_handle);
        new_derived.push(hatch_handle);

        // Extruded solid for this layer.
        if let Some(ext) = extrusions.get(i) {
            if ext.height.abs() > 1e-9 {
                let to_extrude = if ext.base_offset.abs() > 1e-9 {
                    let mut clone = contour_entity.clone();
                    if let EntityType::LwPolyline(ref mut pl) = clone {
                        pl.elevation = ext.base_offset;
                    }
                    Some(clone)
                } else {
                    None
                };
                let entity_to_use = to_extrude.as_ref().unwrap_or(&contour_entity);

                if let Some(body) =
                    crate::scene::model::sweep_model::extruded(entity_to_use, ext.height)
                {
                    let mut s3d = acadrust::entities::Solid3D::new();
                    s3d.wires = crate::scene::model::solid_model::edge_wires(&body);
                    let solid_handle = scene.add_entity(EntityType::Solid3D(s3d));
                    scene.register_solid_model(solid_handle, body);
                    write_wall_derived_tag(scene, solid_handle, wall_handle);
                    new_derived.push(solid_handle);
                }
            }
        }
    }

    if is_v2 {
        let _ = set_wall_v2_derived_handles(scene, wall_handle, &new_derived);
    }
    Ok(())
}

/// Rewrite the `derived_handles` tail of `wall_handle`'s `WALL_V2` record,
/// keeping every other field unchanged. No-op (returns `false`) if the
/// entity doesn't carry a `WALL_V2` record.
pub fn set_wall_v2_derived_handles(
    scene: &mut Scene,
    wall_handle: Handle,
    derived_handles: &[Handle],
) -> bool {
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(v2) = wall_v2_from_entity(entity) else {
        return false;
    };
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_v2_record(
        &v2.style_id,
        v2.height,
        v2.storey_id,
        &v2.layers,
        derived_handles,
        v2.justification,
    );
    write_aec_record(&mut scene.document, wall_handle, record)
}

/// Changes an existing `WALL_V2` wall's justification (Interior/Center/
/// Exterior), shifting its axis polyline sideways by the delta between the
/// old and new justification offsets (same `WallJustification::offset` math
/// used by [`WallCommand::build_entity`]), then regenerates its
/// contour/hatch/solid representation. No-op (returns `false`) if
/// `wall_handle` doesn't carry a `WALL_V2` record.
pub fn change_wall_justification(
    scene: &mut Scene,
    wall_handle: Handle,
    new_justification: WallJustification,
) -> bool {
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(v2) = wall_v2_from_entity(entity) else {
        return false;
    };

    let total_thickness = v2.total_thickness();
    let old_offset = v2.justification.offset(total_thickness);
    let new_offset = new_justification.offset(total_thickness);
    let delta = new_offset - old_offset;

    if delta.abs() > 1e-9 {
        let vertices = get_wall_vertices(scene, wall_handle);
        if vertices.len() >= 2 {
            let points: Vec<(f64, f64)> = vertices.iter().map(|v| (v.x, v.y)).collect();
            let directions = engine::get_offset_directions(&points);
            let shifted: Vec<DVec3> = points
                .iter()
                .zip(directions.iter())
                .map(|(&(x, y), &(dx, dy))| DVec3::new(x + dx * delta, y + dy * delta, 0.0))
                .collect();
            update_wall_vertices(scene, wall_handle, &shifted);
        }
    }

    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_v2_record(
        &v2.style_id,
        v2.height,
        v2.storey_id,
        &v2.layers,
        &v2.derived_handles,
        new_justification,
    );
    if !write_aec_record(&mut scene.document, wall_handle, record) {
        return false;
    }

    let _ = regenerate_wall_representation(scene, wall_handle);
    true
}

/// Register the `OPENCAD_AEC` APPID up front so an interactive `AEC_WALL`
/// command can embed XDATA directly on entities it builds (it has no
/// `&mut CadDocument` while collecting points).
pub fn ensure_wall_app_id(doc: &mut CadDocument) {
    ensure_app_id(doc);
}

/// Default wall height (metres) offered by the command-line prompt after
/// the point chain is finished.
const DEFAULT_WALL_HEIGHT: f64 = 2.8;
/// Default wall thickness (metres) offered by the command-line prompt after
/// the height has been entered.
const DEFAULT_WALL_THICKNESS: f64 = 0.2;
const WALL_JOIN_SNAP_RADIUS: f64 = 0.3;

/// Drawing phase of an in-progress `AEC_WALL` command.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WallPhase {
    /// Collecting click points, like `PLINE`.
    Drawing,
    /// Point chain finished; waiting for a style selection.
    AskStyle,
    /// Point chain finished; waiting for a height value on the command line.
    AskHeight,
    /// Height entered; waiting for a thickness value on the command line.
    AskThickness,
}

/// `AEC_WALL` — interactive multi-point wall polyline drawing, analogous to
/// `PLINE`. Once the point chain is finished (Enter/Escape), the command
/// prompts for height and thickness on the command line (defaults 2.8 / 0.2)
/// before writing the final `WALL` XDATA record and finalizing the entity.
pub struct WallCommand {
    vertices: Vec<DVec3>,
    live_handle: Option<Handle>,
    live_contour_handle: Option<Handle>,
    plane: WorkingPlane,
    wall: Wall,
    phase: WallPhase,
    library: Option<StyleLibrary>,
    style_id: Option<String>,
    resolved_layers: Option<Vec<WallLayer>>,
    justification: WallJustification,
    ctrl_was_down: bool,
    /// Set once the wall height was explicitly edited via the live
    /// Properties-panel field while drawing; when both this and `style_id`
    /// are set, the point chain can finish immediately without the
    /// command-line style/height/thickness fallback prompts.
    height_live_set: bool,
    /// Set when the user tried to finish the wall (Enter/Escape) while a
    /// style selection was mandatory but not made yet; `prompt()` shows a
    /// hint until a style is picked.
    no_style_warning: bool,
    /// Remembers which wall handle was snapped-to for that point index.
    snapped_wall_at_point: HashMap<usize, Handle>,
}

impl WallCommand {
    pub fn new() -> Self {
        // Always load a usable library: `load_or_seed` transparently creates
        // a small default library (materials + wall styles) on first use so
        // the style-selection prompt has something to offer without
        // requiring the user to define materials/styles first.
        Self::new_with_library(Some(engine::library::load_or_seed()))
    }

    pub fn new_with_library(library: Option<StyleLibrary>) -> Self {
        Self {
            vertices: Vec::new(),
            live_handle: None,
            live_contour_handle: None,
            plane: WorkingPlane::default(),
            wall: Wall::new(DEFAULT_WALL_THICKNESS, DEFAULT_WALL_HEIGHT, 0),
            phase: WallPhase::Drawing,
            library,
            style_id: None,
            resolved_layers: None,
            justification: WallJustification::Center,
            ctrl_was_down: false,
            height_live_set: false,
            no_style_warning: false,
            snapped_wall_at_point: HashMap::new(),
        }
    }

    /// Like [`Self::new`], but pre-fills the style/height with the given
    /// session defaults (typically the values used by the last wall
    /// finished this session) instead of the hardcoded fallback defaults.
    pub fn new_with_defaults(last_style_id: Option<&str>, last_height: Option<f64>) -> Self {
        let mut cmd = Self::new();
        if let Some(h) = last_height {
            cmd.wall.height = h;
            cmd.height_live_set = true;
        }
        if let Some(id) = last_style_id {
            if let Some(lib) = &cmd.library {
                if let Some(style) = lib.wall_styles.iter().find(|s| s.style.id == id) {
                    let mut style_map = HashMap::new();
                    for s in &lib.wall_styles {
                        style_map.insert(s.style.id.clone(), s.clone());
                    }
                    if let Ok(layers) = effective_layers(&style_map, &style.style.id) {
                        let resolved = layers
                            .into_iter()
                            .map(|layer| {
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
                                    gap_before: layer.gap_before,
                                    bottom_offset: layer.bottom_offset,
                                    top_offset: layer.top_offset,
                                    layer_override: layer.layer_override.clone(),
                                }
                            })
                            .collect();
                        cmd.style_id = Some(style.style.id.clone());
                        cmd.resolved_layers = Some(resolved);
                    }
                }
            }
        }
        cmd
    }

    /// Parse a command-line value, falling back to `default` for an empty
    /// input; rejects non-positive/invalid input by keeping the default.
    fn parse_dimension(text: &str, default: f64) -> f64 {
        let t = text.trim();
        if t.is_empty() {
            return default;
        }
        match t.parse::<f64>() {
            Ok(v) if v > 0.0 => v,
            _ => default,
        }
    }

    /// True once a style library with at least one wall style is loaded, in
    /// which case a wall style selection is mandatory before finishing (there
    /// is something to choose, so silently falling back to a styleless V1
    /// wall would be surprising). No library / an empty library means there
    /// is nothing to pick, so the plain height/thickness V1 wall stays valid.
    fn requires_style_selection(&self) -> bool {
        self.library
            .as_ref()
            .is_some_and(|lib| !lib.wall_styles.is_empty())
    }

    /// Begin prompting for the wall's height/thickness once the point chain
    /// is done; returns the result that keeps the command active for the
    /// command-line follow-up.
    fn start_dimension_prompt(&mut self) -> CmdResult {
        // Enter/Escape are global "finalize" keys in this app and fire even
        // while the user is typing in the live Properties-panel height field
        // (see `sync_live_if_previewable`) before a second point has been
        // placed. There's nothing to finalize yet in that case — keep the
        // command running instead of cancelling the whole wall.
        if self.vertices.len() < 2 {
            return CmdResult::NeedPoint;
        }
        if self.live_handle.is_none() {
            return CmdResult::Cancel;
        }
        if self.style_id.is_none() && self.requires_style_selection() {
            // Refuse to finish without an assigned wall style; keep the
            // command running so the point chain/preview isn't lost, and
            // surface the hint via `prompt()` (NeedPoint re-prints it).
            self.no_style_warning = true;
            return CmdResult::NeedPoint;
        }
        // Style/height are always visible and editable in the live
        // Properties-panel section shown while this command is drawing (see
        // `live_properties`/`apply_live_property`), so the point chain can
        // finish immediately with whatever is currently set (defaults if the
        // user never touched the panel) instead of repeating the same
        // choices as command-line prompts.
        self.sync_live(true)
    }

    fn build_entity(&self) -> Option<EntityType> {
        if self.vertices.len() < 2 {
            return None;
        }

        let total_thickness = if let Some(layers) = &self.resolved_layers {
            layers.iter().map(|l| l.thickness + l.gap_before).sum()
        } else {
            self.wall.thickness
        };

        let offset = self.justification.offset(total_thickness);

        let points: Vec<(f64, f64)> = self.vertices
            .iter()
            .map(|pt| {
                let local = self.plane.to_local(*pt);
                (local.x, local.y)
            })
            .collect();

        let final_points = if offset.abs() > 1e-9 {
            let directions = engine::get_offset_directions(&points);
            points
                .iter()
                .zip(directions.iter())
                .map(|(&(x, y), &(dx, dy))| (x + dx * offset, y + dy * offset))
                .collect()
        } else {
            points
        };

        let mut pl = LwPolyline::new();
        for (x, y) in final_points {
            pl.add_vertex(LwVertex::new(Vector2::new(x, y)));
        }
        let mut entity = self.plane.place_entity(EntityType::LwPolyline(pl));

        let record = if let (Some(style_id), Some(layers)) = (&self.style_id, &self.resolved_layers) {
            let mut rec = ExtendedDataRecord::new(AEC_APPID);
            rec.values = wall_v2_record(
                style_id,
                self.wall.height,
                self.wall.storey_id,
                layers,
                &[],
                self.justification,
            );
            rec
        } else {
            wall_record(&self.wall)
        };

        entity.common_mut().extended_data.add_record(record);
        Some(entity)
    }

    fn build_contour_entity(&self) -> Option<EntityType> {
        if self.vertices.len() < 2 {
            return None;
        }
        // Follow the same thickness fallback as `build_entity`: once a style
        // is picked, use its resolved layers' total thickness; before that
        // (or if there are no layers), fall back to the default wall
        // thickness so the outline preview always follows the cursor,
        // regardless of whether a style has been chosen yet.
        let total_thickness: f64 = match self.resolved_layers.as_ref() {
            Some(layers) if !layers.is_empty() => {
                layers.iter().map(|l| l.thickness + l.gap_before).sum()
            }
            _ => self.wall.thickness,
        };
        let centerline_offset = self.justification.offset(total_thickness);

        let points: Vec<(f64, f64)> = self.vertices
            .iter()
            .map(|pt| {
                let local = self.plane.to_local(*pt);
                (local.x, local.y)
            })
            .collect();

        let contour_points =
            engine::contour::outer_contour(&points, total_thickness, centerline_offset);

        let mut pl = LwPolyline::new();
        pl.is_closed = true;
        for (x, y) in contour_points {
            pl.add_vertex(LwVertex::new(Vector2::new(x, y)));
        }

        let mut entity = self.plane.place_entity(EntityType::LwPolyline(pl));
        // Contour is a visual helper; no XDATA needed (axis carries the truth).
        Some(entity)
    }

    fn sync_live(&self, finish: bool) -> CmdResult {
        let axis = self.build_entity();
        let contour = self.build_contour_entity();

        match (axis, self.live_handle) {
            (Some(a), Some(h_axis)) => {
                if let (Some(c), None) = (&contour, self.live_contour_handle) {
                    // We just gained a contour (e.g. style assigned mid-draw);
                    // replace the single axis with both axis + contour.
                    CmdResult::ReplaceEntity(h_axis, vec![a, c.clone()])
                } else if let (None, Some(h_contour)) = (&contour, self.live_contour_handle) {
                    // Style removed mid-draw? Rare, but handle it by replacing
                    // both with just the axis.
                    CmdResult::ReplaceManyContinue(vec![(h_axis, vec![a]), (h_contour, vec![])])
                } else {
                    let mut updates = vec![(h_axis, a)];
                    if let (Some(c), Some(h_contour)) = (contour, self.live_contour_handle) {
                        updates.push((h_contour, c));
                    }
                    CmdResult::UpdateLiveEntities { updates, finish }
                }
            }
            (Some(a), None) => {
                let mut entities = vec![a];
                if let Some(c) = contour {
                    entities.push(c);
                }
                CmdResult::CommitLiveEntities(entities)
            }
            (None, _) => CmdResult::Cancel,
        }
    }

    /// Like [`Self::sync_live`], but used for edits coming from the live
    /// Properties-panel fields (style/height), which can legitimately fire
    /// before there is anything to preview yet (e.g. only the start point has
    /// been placed). In that case there is no live entity to update/cancel —
    /// just keep the command running and wait for the next point.
    fn sync_live_if_previewable(&self, finish: bool) -> CmdResult {
        if self.vertices.len() < 2 {
            return CmdResult::NeedPoint;
        }
        self.sync_live(finish)
    }

    fn undo_last_vertex(&mut self) -> CmdResult {
        if self.vertices.is_empty() {
            return CmdResult::NeedPoint;
        }
        self.vertices.pop();
        match self.vertices.len() {
            0 => CmdResult::NeedPoint,
            1 => match self.live_handle.take() {
                Some(h) => CmdResult::RemoveLiveEntity(h),
                None => CmdResult::NeedPoint,
            },
            _ => self.sync_live(false),
        }
    }
}

impl CadCommand for WallCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "AEC_WALL"
    }

    fn prompt(&self) -> String {
        match self.phase {
            WallPhase::Drawing if self.vertices.is_empty() => {
                format!(
                    "AEC_WALL  Specify start point (Justification: {}):",
                    self.justification.as_str()
                )
            }
            WallPhase::Drawing if self.no_style_warning => {
                "AEC_WALL  Please select a wall style in the Properties panel before finishing."
                    .to_string()
            }
            WallPhase::Drawing => {
                format!(
                    "AEC_WALL  Next pt (Justification: {}) [{}pts]:",
                    self.justification.as_str(),
                    self.vertices.len()
                )
            }
            WallPhase::AskStyle => {
                let styles = self.library.as_ref().map(|l| &l.wall_styles).unwrap();
                let names: Vec<_> = styles.iter().map(|s| s.style.name.as_str()).collect();
                let default = names.first().copied().unwrap_or("");
                format!("AEC_WALL  Select wall style [{}] <{}>:", names.join("/"), default)
            }
            WallPhase::AskHeight => {
                format!("AEC_WALL  Specify wall height <{DEFAULT_WALL_HEIGHT}>:")
            }
            WallPhase::AskThickness => {
                format!("AEC_WALL  Specify wall thickness <{DEFAULT_WALL_THICKNESS}>:")
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.phase {
            WallPhase::Drawing if self.vertices.is_empty() => Vec::new(),
            WallPhase::Drawing => vec![CmdOption::new("Undo", "U"), CmdOption::enter("Done")],
            WallPhase::AskStyle => {
                if let Some(lib) = &self.library {
                    lib.wall_styles
                        .iter()
                        .map(|s| CmdOption::new(&s.style.name, &s.style.name))
                        .collect()
                } else {
                    Vec::new()
                }
            }
            WallPhase::AskHeight | WallPhase::AskThickness => Vec::new(),
        }
    }

    fn set_ctrl(&mut self, ctrl: bool) {
        if ctrl && !self.ctrl_was_down {
            self.justification = self.justification.next();
            // In a real CLI we would use CmdResult::Log, but set_ctrl doesn't
            // return it. Toggling the prompt is the next best thing for
            // live feedback.
        }
        self.ctrl_was_down = ctrl;
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        if self.phase != WallPhase::Drawing || self.vertices.is_empty() {
            return vec![];
        }

        // Axis rubber band: pending segment from the last placed point to
        // the cursor (the committed vertices already render as the live
        // axis polyline, same convention as `PlineCommand::on_mouse_move`).
        let last_world = *self.vertices.last().unwrap();
        let axis_wire = WireModel::solid(
            "rubber_band_axis".into(),
            vec![
                last_world.as_vec3().to_array(),
                pt.as_vec3().to_array(),
            ],
            WireModel::CYAN,
            false,
        );
        let mut wires = vec![axis_wire];

        // Outline rubber band: the wall's outer contour, computed on the
        // committed vertices plus the not-yet-placed cursor point, so the
        // outline is visible and follows the cursor from the first point
        // onward — even before a wall style has been chosen (falls back to
        // the default thickness, same as `build_contour_entity`).
        let mut temp_vertices = self.vertices.clone();
        temp_vertices.push(pt);
        if temp_vertices.len() >= 2 {
            let total_thickness: f64 = match self.resolved_layers.as_ref() {
                Some(layers) if !layers.is_empty() => {
                    layers.iter().map(|l| l.thickness + l.gap_before).sum()
                }
                _ => self.wall.thickness,
            };
            let centerline_offset = self.justification.offset(total_thickness);
            let points: Vec<(f64, f64)> = temp_vertices
                .iter()
                .map(|p| {
                    let local = self.plane.to_local(*p);
                    (local.x, local.y)
                })
                .collect();
            let contour_points =
                engine::contour::outer_contour(&points, total_thickness, centerline_offset);
            if !contour_points.is_empty() {
                let mut world_pts: Vec<[f32; 3]> = contour_points
                    .iter()
                    .map(|&(x, y)| self.plane.to_world(DVec3::new(x, y, 0.0)).as_vec3().to_array())
                    .collect();
                if let Some(first) = world_pts.first().copied() {
                    world_pts.push(first);
                }
                wires.push(WireModel::solid(
                    "rubber_band_contour".into(),
                    world_pts,
                    WireModel::CYAN,
                    false,
                ));
            }
        }

        wires
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.phase != WallPhase::Drawing {
            // Height/thickness prompt is active; ignore stray clicks.
            return CmdResult::NeedPoint;
        }
        self.vertices.push(pt);
        if self.style_id.is_none() && self.requires_style_selection() {
            self.no_style_warning = true;
        }
        if self.vertices.len() >= 2 {
            self.sync_live(false)
        } else {
            CmdResult::NeedPoint
        }
    }

    fn set_live_handles(&mut self, handles: Vec<Handle>) {
        self.live_handle = handles.first().copied();
        self.live_contour_handle = handles.get(1).copied();
    }

    fn on_entity_replaced(&mut self, old: Handle, new_handles: &[Handle]) {
        if Some(old) == self.live_handle {
            self.live_handle = new_handles.first().copied();
            if new_handles.len() > 1 {
                self.live_contour_handle = Some(new_handles[1]);
            }
        } else if Some(old) == self.live_contour_handle {
            if new_handles.is_empty() {
                self.live_contour_handle = None;
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.phase {
            WallPhase::Drawing => self.start_dimension_prompt(),
            WallPhase::AskStyle | WallPhase::AskHeight | WallPhase::AskThickness => {
                self.on_text_input("").unwrap_or(CmdResult::Cancel)
            }
        }
    }

    fn enter_accepts_default_start(&self) -> bool {
        self.phase == WallPhase::Drawing && self.vertices.is_empty()
    }

    fn on_escape(&mut self) -> CmdResult {
        self.on_enter()
    }

    fn on_space_change(&mut self) -> CmdResult {
        self.on_enter()
    }

    fn wants_text_input(&self) -> bool {
        !self.vertices.is_empty() || self.phase != WallPhase::Drawing
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.phase == WallPhase::Drawing && !self.vertices.is_empty()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match self.phase {
            WallPhase::Drawing => match text.trim().to_uppercase().as_str() {
                "U" | "UNDO" => Some(self.undo_last_vertex()),
                _ => None,
            },
            WallPhase::AskStyle => {
                let lib = self.library.as_ref()?;
                let selected = if text.trim().is_empty() {
                    lib.wall_styles.first()
                } else {
                    lib.wall_styles.iter().find(|s| {
                        s.style.name.eq_ignore_ascii_case(text.trim())
                            || s.style.id.eq_ignore_ascii_case(text.trim())
                    })
                };

                if let Some(style) = selected {
                    self.style_id = Some(style.style.id.clone());

                    // Resolve layers
                    let mut style_map = HashMap::new();
                    for s in &lib.wall_styles {
                        style_map.insert(s.style.id.clone(), s.clone());
                    }

                    if let Ok(layers) = effective_layers(&style_map, &style.style.id) {
                        let mut resolved = Vec::new();
                        for layer in layers {
                            let mat_name = lib
                                .materials
                                .iter()
                                .find(|m| m.id == layer.material_id)
                                .map(|m| m.name.clone())
                                .unwrap_or_else(|| layer.material_id.clone());

                            let func_str = match &layer.function {
                                LayerFunction::Structural => "Structural".to_string(),
                                LayerFunction::Insulation => "Insulation".to_string(),
                                LayerFunction::Finish => "Finish".to_string(),
                                LayerFunction::Other(s) => s.clone(),
                            };
                            resolved.push(WallLayer {
                                material: mat_name,
                                thickness: layer.thickness,
                                function: func_str,
                                gap_before: layer.gap_before,
                                bottom_offset: layer.bottom_offset,
                                top_offset: layer.top_offset,
                                layer_override: layer.layer_override.clone(),
                            });
                        }
                        self.resolved_layers = Some(resolved);
                    }

                    // Style doesn't have height, so go to AskHeight
                    self.phase = WallPhase::AskHeight;
                    Some(CmdResult::NeedPoint)
                } else {
                    // Invalid style name, stay here
                    Some(CmdResult::NeedPoint)
                }
            }
            WallPhase::AskHeight => {
                self.wall.height = Self::parse_dimension(text, DEFAULT_WALL_HEIGHT);
                if self.style_id.is_some() {
                    // We have a style, so we skip AskThickness
                    Some(self.sync_live(true))
                } else {
                    self.phase = WallPhase::AskThickness;
                    Some(CmdResult::NeedPoint)
                }
            }
            WallPhase::AskThickness => {
                self.wall.thickness = Self::parse_dimension(text, DEFAULT_WALL_THICKNESS);
                Some(self.sync_live(true))
            }
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if self.phase == WallPhase::Drawing && !self.vertices.is_empty() {
            Some(self.undo_last_vertex())
        } else {
            None
        }
    }

    fn live_properties(&self) -> Option<crate::command::LiveCommandProperties> {
        use crate::command::{LiveCommandField, LiveCommandProperties, LiveFieldValue};

        if self.phase != WallPhase::Drawing {
            return None;
        }

        let style_name = match &self.style_id {
            Some(id) => self
                .library
                .as_ref()
                .and_then(|lib| lib.wall_styles.iter().find(|ws| &ws.style.id == id))
                .map(|ws| ws.style.name.clone())
                .unwrap_or_else(|| id.clone()),
            None => String::new(),
        };

        Some(LiveCommandProperties {
            title: crate::t!("Wall").into_owned(),
            fields: vec![
                LiveCommandField {
                    label: crate::t!("Style").into_owned(),
                    field_id: "wall_style",
                    value: LiveFieldValue::Picker(style_name),
                },
                LiveCommandField {
                    label: crate::t!("Height").into_owned(),
                    field_id: "wall_height",
                    value: LiveFieldValue::Number(self.wall.height),
                },
            ],
        })
    }

    fn live_property_id(&self, field_id: &str) -> Option<String> {
        match field_id {
            "wall_style" => self.style_id.clone(),
            _ => None,
        }
    }

    fn apply_live_property(
        &mut self,
        field_id: &str,
        value: crate::command::LiveFieldValue,
    ) -> CmdResult {
        use crate::command::LiveFieldValue;

        match (field_id, value) {
            ("wall_style", LiveFieldValue::Picker(style_id)) => {
                let Some(lib) = &self.library else {
                    return CmdResult::NeedPoint;
                };
                let Some(style) = lib
                    .wall_styles
                    .iter()
                    .find(|s| s.style.id == style_id)
                else {
                    return CmdResult::NeedPoint;
                };

                self.style_id = Some(style.style.id.clone());
                self.no_style_warning = false;

                let mut style_map = HashMap::new();
                for s in &lib.wall_styles {
                    style_map.insert(s.style.id.clone(), s.clone());
                }

                if let Ok(layers) = effective_layers(&style_map, &style.style.id) {
                    let mut resolved = Vec::new();
                    for layer in layers {
                        let mat_name = lib
                            .materials
                            .iter()
                            .find(|m| m.id == layer.material_id)
                            .map(|m| m.name.clone())
                            .unwrap_or_else(|| layer.material_id.clone());

                        let func_str = layer_function_to_str(&layer.function);
                        resolved.push(WallLayer {
                            material: mat_name,
                            thickness: layer.thickness,
                            function: func_str,
                            gap_before: layer.gap_before,
                            bottom_offset: layer.bottom_offset,
                            top_offset: layer.top_offset,
                            layer_override: layer.layer_override.clone(),
                        });
                    }
                    self.resolved_layers = Some(resolved);
                }

                self.sync_live_if_previewable(false)
            }
            ("wall_height", LiveFieldValue::Number(h)) => {
                self.wall.height = h;
                self.height_live_set = true;
                self.sync_live_if_previewable(false)
            }
            _ => CmdResult::NeedPoint,
        }
    }
}

/// Converts a [`LayerFunction`] to the plain string used in XDATA/dispatch.
fn layer_function_to_str(f: &LayerFunction) -> String {
    match f {
        LayerFunction::Structural => "Structural".to_string(),
        LayerFunction::Insulation => "Insulation".to_string(),
        LayerFunction::Finish => "Finish".to_string(),
        LayerFunction::Other(s) => s.clone(),
    }
}

/// Parses a plain string (as entered on the command line) into a
/// [`LayerFunction`], defaulting to `Structural` for empty input and falling
/// back to `Other(..)` for anything unrecognized.
fn parse_layer_function(s: &str) -> LayerFunction {
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

/// Step of an in-progress `AEC_MATERIAL` command.
enum MaterialStep {
    Name,
    Hatch { name: String },
    Color { name: String, hatch: String },
    LineType { name: String, hatch: String, color: u32 },
}

/// `AEC_MATERIAL` — create (or update) a material in the AEC style library,
/// prompting step by step for name, hatch pattern, line color and line type.
pub struct MaterialCommand {
    step: MaterialStep,
}

impl MaterialCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            step: MaterialStep::Name,
        }
    }
}

impl CadCommand for MaterialCommand {
    fn name(&self) -> &'static str {
        "AEC_MATERIAL"
    }

    fn prompt(&self) -> String {
        match &self.step {
            MaterialStep::Name => "AEC_MATERIAL  Enter material name:".to_string(),
            MaterialStep::Hatch { .. } => {
                "AEC_MATERIAL  Enter hatch pattern <ANSI31>:".to_string()
            }
            MaterialStep::Color { .. } => {
                "AEC_MATERIAL  Enter line color as hex RRGGBB <000000>:".to_string()
            }
            MaterialStep::LineType { .. } => {
                "AEC_MATERIAL  Enter line type <Continuous>:".to_string()
            }
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        match &self.step {
            MaterialStep::Name => {
                if t.is_empty() {
                    // A material needs a name; keep prompting.
                    return Some(CmdResult::NeedPoint);
                }
                self.step = MaterialStep::Hatch {
                    name: t.to_string(),
                };
                Some(CmdResult::NeedPoint)
            }
            MaterialStep::Hatch { name } => {
                let hatch = if t.is_empty() {
                    "ANSI31".to_string()
                } else {
                    t.to_string()
                };
                self.step = MaterialStep::Color {
                    name: name.clone(),
                    hatch,
                };
                Some(CmdResult::NeedPoint)
            }
            MaterialStep::Color { name, hatch } => {
                let color = u32::from_str_radix(t.trim_start_matches('#'), 16).unwrap_or(0);
                self.step = MaterialStep::LineType {
                    name: name.clone(),
                    hatch: hatch.clone(),
                    color,
                };
                Some(CmdResult::NeedPoint)
            }
            MaterialStep::LineType { name, hatch, color } => {
                let line_type = if t.is_empty() {
                    "Continuous".to_string()
                } else {
                    t.to_string()
                };
                Some(CmdResult::Dispatch(format!(
                    "AEC_MATERIAL_ADD {name}|{hatch}|{color:06X}|{line_type}"
                )))
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").unwrap_or(CmdResult::Cancel)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// `AEC_MATERIAL_ADD name|hatch|color_hex|line_type` — the non-interactive
/// handler `MaterialCommand` dispatches to once all fields are collected;
/// upserts the material (by name → stable id) into the style library and
/// persists it.
pub fn aec_material_add(command_line: &mut CommandLine, args: &str) {
    let parts: Vec<&str> = args.split('|').collect();
    let [name, hatch, color_hex, line_type] = parts.as_slice() else {
        command_line.push_error("AEC_MATERIAL_ADD: malformed arguments.");
        return;
    };
    let color = u32::from_str_radix(color_hex, 16).unwrap_or(0);
    let id = format!("mat_{}", slugify(name));

    let mut lib = load_or_seed();
    lib.upsert_material(Material::new(
        id,
        name.to_string(),
        hatch.to_string(),
        color,
        line_type.to_string(),
    ));
    match engine::library::save_to_default_path(&lib) {
        Ok(()) => command_line.push_info(&format!(
            "AEC: Material '{name}' saved (hatch {hatch}, color #{color:06X}, linetype {line_type})."
        )),
        Err(e) => command_line.push_error(&format!("AEC_MATERIAL: failed to save library: {e}")),
    }
}

/// Step of an in-progress `AEC_STYLE` command.
enum StyleStep {
    Name,
    Parent {
        name: String,
    },
    /// Collecting layers; `layers` accumulates `(material_name, thickness, function)`.
    LayerMaterial {
        name: String,
        parent: Option<String>,
        layers: Vec<(String, f64, LayerFunction)>,
    },
    LayerThickness {
        name: String,
        parent: Option<String>,
        layers: Vec<(String, f64, LayerFunction)>,
        material: String,
    },
    LayerFunctionStep {
        name: String,
        parent: Option<String>,
        layers: Vec<(String, f64, LayerFunction)>,
        material: String,
        thickness: f64,
    },
}

/// `AEC_STYLE` — create (or update) a wall style in the AEC style library:
/// name, optional parent style (for single-parent inheritance), then a loop
/// collecting material/thickness/function per layer (blank material name
/// ends the loop).
pub struct StyleCommand {
    step: StyleStep,
}

impl StyleCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            step: StyleStep::Name,
        }
    }

    fn finish(name: &str, parent: &Option<String>, layers: &[(String, f64, LayerFunction)]) -> CmdResult {
        let parent_part = parent.clone().unwrap_or_default();
        let layers_part = layers
            .iter()
            .map(|(mat, thick, func)| format!("{mat}:{thick}:{}", layer_function_to_str(func)))
            .collect::<Vec<_>>()
            .join(";");
        CmdResult::Dispatch(format!("AEC_STYLE_ADD {name}|{parent_part}|{layers_part}"))
    }
}

impl CadCommand for StyleCommand {
    fn name(&self) -> &'static str {
        "AEC_STYLE"
    }

    fn prompt(&self) -> String {
        match &self.step {
            StyleStep::Name => "AEC_STYLE  Enter wall style name:".to_string(),
            StyleStep::Parent { .. } => {
                "AEC_STYLE  Enter parent style name (blank = none):".to_string()
            }
            StyleStep::LayerMaterial { layers, .. } => format!(
                "AEC_STYLE  Add layer {} — material name (blank = finish style):",
                layers.len() + 1
            ),
            StyleStep::LayerThickness { material, .. } => {
                format!("AEC_STYLE  Layer '{material}' — thickness <0.2>:")
            }
            StyleStep::LayerFunctionStep { material, .. } => format!(
                "AEC_STYLE  Layer '{material}' — function [Structural/Insulation/Finish] <Structural>:"
            ),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        match &self.step {
            StyleStep::Name => {
                if t.is_empty() {
                    return Some(CmdResult::NeedPoint);
                }
                self.step = StyleStep::Parent {
                    name: t.to_string(),
                };
                Some(CmdResult::NeedPoint)
            }
            StyleStep::Parent { name } => {
                let parent = if t.is_empty() {
                    None
                } else {
                    Some(t.to_string())
                };
                self.step = StyleStep::LayerMaterial {
                    name: name.clone(),
                    parent,
                    layers: Vec::new(),
                };
                Some(CmdResult::NeedPoint)
            }
            StyleStep::LayerMaterial { name, parent, layers } => {
                if t.is_empty() {
                    // No (more) layers: finish, possibly inheriting layers from
                    // the parent style if none were entered here.
                    return Some(Self::finish(name, parent, layers));
                }
                self.step = StyleStep::LayerThickness {
                    name: name.clone(),
                    parent: parent.clone(),
                    layers: layers.clone(),
                    material: t.to_string(),
                };
                Some(CmdResult::NeedPoint)
            }
            StyleStep::LayerThickness {
                name,
                parent,
                layers,
                material,
            } => {
                let thickness = WallCommand::parse_dimension(t, 0.2);
                self.step = StyleStep::LayerFunctionStep {
                    name: name.clone(),
                    parent: parent.clone(),
                    layers: layers.clone(),
                    material: material.clone(),
                    thickness,
                };
                Some(CmdResult::NeedPoint)
            }
            StyleStep::LayerFunctionStep {
                name,
                parent,
                layers,
                material,
                thickness,
            } => {
                let function = parse_layer_function(t);
                let mut layers = layers.clone();
                layers.push((material.clone(), *thickness, function));
                self.step = StyleStep::LayerMaterial {
                    name: name.clone(),
                    parent: parent.clone(),
                    layers,
                };
                Some(CmdResult::NeedPoint)
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").unwrap_or(CmdResult::Cancel)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// `AEC_STYLE_ADD name|parent|mat1:thick1:func1;mat2:thick2:func2...` — the
/// non-interactive handler `StyleCommand` dispatches to once all fields are
/// collected; upserts the wall style (by name → stable id) into the style
/// library and persists it. An empty layer list inherits layers from the
/// parent style at resolution time (see `effective_layers`).
pub fn aec_style_add(command_line: &mut CommandLine, args: &str) {
    let mut parts = args.splitn(3, '|');
    let (Some(name), Some(parent_raw), Some(layers_raw)) =
        (parts.next(), parts.next(), parts.next())
    else {
        command_line.push_error("AEC_STYLE_ADD: malformed arguments.");
        return;
    };

    let mut lib = load_or_seed();

    let parent_style_id = if parent_raw.is_empty() {
        None
    } else {
        match lib
            .wall_styles
            .iter()
            .find(|s| s.style.name.eq_ignore_ascii_case(parent_raw))
        {
            Some(p) => Some(p.style.id.clone()),
            None => {
                command_line.push_error(&format!(
                    "AEC_STYLE: unknown parent style '{parent_raw}', creating without a parent."
                ));
                None
            }
        }
    };

    let mut layers = Vec::new();
    if !layers_raw.is_empty() {
        for entry in layers_raw.split(';') {
            let fields: Vec<&str> = entry.splitn(3, ':').collect();
            let [mat_name, thick_str, func_str] = fields.as_slice() else {
                continue;
            };
            let material_id = lib
                .materials
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case(mat_name))
                .map(|m| m.id.clone())
                .unwrap_or_else(|| format!("mat_{}", slugify(mat_name)));
            let thickness: f64 = thick_str.parse().unwrap_or(0.2);
            layers.push(Layer {
                material_id,
                thickness,
                function: parse_layer_function(func_str),
                gap_before: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
            });
        }
    }

    let id = format!("style_{}", slugify(name));
    lib.upsert_wall_style(WallStyle {
        style: Style {
            id,
            name: name.to_string(),
            object_kind: "Wall".to_string(),
            parent_style_id,
        },
        layers,
    });

    match engine::library::save_to_default_path(&lib) {
        Ok(()) => command_line.push_info(&format!(
            "AEC: Wall style '{name}' saved with {} layer(s).",
            lib.wall_styles
                .iter()
                .find(|s| s.style.name == name)
                .map(|s| s.layers.len())
                .unwrap_or(0)
        )),
        Err(e) => command_line.push_error(&format!("AEC_STYLE: failed to save library: {e}")),
    }
}

/// `AEC_ROOM` — detect a closed wall loop (or demo rectangle) + ROOM XDATA.
pub fn aec_room(scene: &mut Scene, command_line: &mut CommandLine) {
    let wall_segments = collect_wall_segments(&scene.document);
    let (pts, detected) = match find_closed_loop(&wall_segments, 1e-3) {
        Some(loop_pts) => (loop_pts, true),
        None => (
            vec![(0.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 3.0)],
            false,
        ),
    };

    let room = Room::from_polygon("Office 101", &pts, 2.8, 0);

    let mut pl = LwPolyline::new();
    pl.is_closed = true;
    for (x, y) in &pts {
        pl.add_vertex(LwVertex::new(Vector2::new(*x, *y)));
    }

    let handle = scene.add_entity(EntityType::LwPolyline(pl));

    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("ROOM".to_string()));
    record.add_value(XDataValue::String(room.name.clone()));
    record.add_value(XDataValue::Real(room.area));
    record.add_value(XDataValue::Real(room.perimeter));
    record.add_value(XDataValue::Real(room.volume));
    record.add_value(XDataValue::Integer32(room.storey_id as i32));

    write_aec_record(&mut scene.document, handle, record);
    scene.bump_geometry();
    if detected {
        command_line.push_info(&format!(
            "AEC: Detected closed wall loop, created room '{}' ({} vertices) at {handle}",
            room.name,
            pts.len()
        ));
    } else {
        command_line.push_info(&format!(
            "AEC: No closed wall loop found, created demo room '{}' at {handle}",
            room.name
        ));
    }
}

/// `AEC_STOREY` — append an in-memory storey and report it.
pub fn aec_storey(_scene: &mut Scene, command_line: &mut CommandLine) {
    let mut storeys = STOREYS.lock().unwrap();
    let next_id = storeys.len() as u32;
    let new_storey = Storey::new(
        format!("Level {}", next_id + 1),
        (next_id as f64) * 3.0,
        3.0,
    );
    storeys.push(new_storey.clone());

    command_line.push_info(&format!(
        "AEC: Added storey '{}' at elevation {}",
        new_storey.name, new_storey.elevation
    ));
}

/// Expand an erase/delete selection with each wall's `derived_handles`
/// (contour/hatch/solid entities generated by [`regenerate_wall_representation`])
/// so deleting a `WALL`/`WALL_V2` entity also removes its rendered representation.
///
/// Deduplicates and skips any handle not present in the document (already erased).
pub fn expand_with_wall_derived_handles(scene: &Scene, handles: &mut Vec<Handle>) {
    let mut extra = Vec::new();
    for handle in handles.iter() {
        let Some(entity) = scene.document.get_entity(*handle) else {
            continue;
        };
        let Some(record) = read_aec_record(entity) else {
            continue;
        };
        let is_wall = matches!(
            record.values.first(),
            Some(XDataValue::String(kind)) if kind == "WALL" || kind == "WALL_V2"
        );
        if !is_wall {
            continue;
        }
        if let Some(v2) = wall_v2_from_entity(entity) {
            extra.extend(v2.derived_handles.iter().copied());
        }
    }
    for h in extra {
        if !handles.contains(&h) && scene.document.get_entity(h).is_some() {
            handles.push(h);
        }
    }
}

/// `AEC_WALL_REFRESH` — migration path for walls created before the
/// contour/hatch/solid representation existed: rebuild it for every
/// `WALL`/`WALL_V2` entity in the document that doesn't already carry a
/// `derived_handles` list (new walls skip a redundant rebuild).
pub fn aec_wall_refresh(scene: &mut Scene, command_line: &mut CommandLine) {
    let candidates: Vec<Handle> = scene
        .document
        .entities()
        .filter_map(|entity| {
            let record = read_aec_record(entity)?;
            match record.values.first() {
                Some(XDataValue::String(kind)) if kind == "WALL" => Some(entity.common().handle),
                Some(XDataValue::String(kind)) if kind == "WALL_V2" => {
                    let v2 = wall_v2_from_entity(entity)?;
                    if v2.derived_handles.is_empty() {
                        Some(entity.common().handle)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        })
        .collect();

    let mut refreshed = 0usize;
    for handle in candidates {
        if regenerate_wall_representation(scene, handle).is_ok() {
            refreshed += 1;
        }
    }
    scene.bump_geometry();
    command_line.push_info(&format!(
        "AEC_WALL_REFRESH: rebuilt representation for {refreshed} wall(s)."
    ));
}

/// `AEC_ROOMSCHEDULE` — scan ROOM XDATA and build a real TABLE entity.
pub fn aec_room_schedule(scene: &mut Scene, command_line: &mut CommandLine) {
    let mut rooms = Vec::new();
    for entity in scene.document.entities() {
        let Some(record) = read_aec_record(entity) else {
            continue;
        };
        if let Some(XDataValue::String(kind)) = record.values.first() {
            if kind == "ROOM" && record.values.len() >= 6 {
                // 0: "ROOM", 1: name, 2: area, 3: perim, 4: vol, 5: storey_id
                let name = if let XDataValue::String(s) = &record.values[1] {
                    s.clone()
                } else {
                    "Unknown".to_string()
                };
                let area = if let XDataValue::Real(r) = record.values[2] {
                    r
                } else {
                    0.0
                };
                let storey_id = if let XDataValue::Integer32(i) = record.values[5] {
                    i
                } else {
                    0
                };
                rooms.push((name, area, storey_id));
            }
        }
    }

    if rooms.is_empty() {
        command_line.push_info("AEC: No rooms found in document.");
        return;
    }

    // Real TABLE entity: header row + one row per room.
    let row_count = rooms.len() + 1;
    let mut table = Table::new(Vector3::ZERO, row_count, 3);
    table.set_cell_text(0, 0, "Name");
    table.set_cell_text(0, 1, "Area");
    table.set_cell_text(0, 2, "Storey ID");
    for (row, (name, area, storey)) in rooms.iter().enumerate() {
        table.set_cell_text(row + 1, 0, name);
        table.set_cell_text(row + 1, 1, &format!("{area:.2}"));
        table.set_cell_text(row + 1, 2, &storey.to_string());
    }

    let handle = scene.add_entity(EntityType::Table(table));
    scene.bump_geometry();
    command_line.push_info(&format!(
        "AEC: Room schedule table with {} room(s) created at {handle}",
        rooms.len()
    ));
}

/// `AEC_IFCEXPORT` — collect walls/rooms/storeys and emit IFC4 SPF (in-memory).
pub fn aec_ifc_export(scene: &mut Scene, command_line: &mut CommandLine) {
    let mut ifc_scene = engine::Scene::default();

    // Add in-memory storeys
    {
        let storeys = STOREYS.lock().unwrap();
        for (i, s) in storeys.iter().enumerate() {
            ifc_scene.storeys.push((i as u32, s.clone()));
        }
    }

    // Collect walls and rooms from document XDATA
    for entity in scene.document.entities() {
        let Some(record) = read_aec_record(entity) else {
            continue;
        };
        match record.values.first() {
            Some(XDataValue::String(kind)) if kind == "WALL" || kind == "WALL_V2" => {
                if let Some((thickness, height, storey_id)) = wall_thickness_and_height(entity) {
                    let material_ref = if kind == "WALL" {
                        if let Some(XDataValue::String(s)) = record.values.get(3) {
                            if s.is_empty() { None } else { Some(s.clone()) }
                        } else { None }
                    } else {
                        // For V2, just take the first layer's material as representative for IFC export for now
                        if let Some(XDataValue::String(s)) = record.values.get(5) {
                            if s.is_empty() { None } else { Some(s.clone()) }
                        } else { None }
                    };
                    ifc_scene.walls.push(Wall {
                        thickness,
                        height,
                        material_ref,
                        storey_id,
                    });
                }
            }
            Some(XDataValue::String(kind)) if kind == "ROOM" => {
                // 0: "ROOM", 1: name, 2: area, 3: perim, 4: vol, 5: storey_id
                if record.values.len() >= 6 {
                    let name = if let XDataValue::String(s) = &record.values[1] {
                        s.clone()
                    } else {
                        "Unknown".to_string()
                    };
                    let area = if let XDataValue::Real(r) = record.values[2] {
                        r
                    } else {
                        0.0
                    };
                    let perimeter = if let XDataValue::Real(r) = record.values[3] {
                        r
                    } else {
                        0.0
                    };
                    let volume = if let XDataValue::Real(r) = record.values[4] {
                        r
                    } else {
                        0.0
                    };
                    let storey_id = if let XDataValue::Integer32(i) = record.values[5] {
                        i as u32
                    } else {
                        0
                    };
                    ifc_scene.rooms.push(Room {
                        name,
                        area,
                        perimeter,
                        volume,
                        storey_id,
                    });
                }
            }
            _ => {}
        }
    }

    let ifc_data = engine::ifc::write_spf(&ifc_scene);
    command_line.push_info(&format!(
        "AEC: Exported IFC4 SPF ({} bytes)",
        ifc_data.len()
    ));
    command_line.push_info("(Note: Real file-save dialog is a future step)");
}

/// `AEC_WALLJOIN` — interactive front-end: pick two wall entities (clicking a
/// derived contour/hatch/solid resolves to its axis, like every other wall
/// selection, via [`resolve_wall_package`]), then delegate the actual
/// geometry join to [`aec_walljoin_do`] via [`CmdResult::Dispatch`] once both
/// picks are in — the same "gather interactively, execute non-interactively
/// with full scene access" split used by [`MaterialCommand`]/`AEC_MATERIAL_ADD`.
pub struct WallJoinCommand {
    selected: Vec<Handle>,
}

impl WallJoinCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            selected: Vec::new(),
        }
    }
}

impl CadCommand for WallJoinCommand {
    fn name(&self) -> &'static str {
        "AEC_WALLJOIN"
    }

    fn prompt(&self) -> String {
        match self.selected.len() {
            0 => "AEC_WALLJOIN  Select first wall to join:".to_string(),
            _ => "AEC_WALLJOIN  Select second wall to join:".to_string(),
        }
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.selected.push(handle);
        if self.selected.len() == 2 {
            let a = self.selected[0];
            let b = self.selected[1];
            CmdResult::Dispatch(format!("AEC_WALLJOIN_DO {}|{}", a.value(), b.value()))
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// Returns the index (`0` or the last vertex) of the endpoint that
/// [`join::join_wall_axes`] moved, comparing `old` against `new`, or `None`
/// if neither endpoint changed (shouldn't happen for a successful join, but
/// guards against float-identical corners).
fn changed_endpoint(old: &[DVec3], new: &[DVec3]) -> Option<usize> {
    const TOL: f64 = 1e-9;
    if old.is_empty() || new.len() != old.len() {
        return None;
    }
    if old[0].distance(new[0]) > TOL {
        return Some(0);
    }
    let last = old.len() - 1;
    if old[last].distance(new[last]) > TOL {
        return Some(last);
    }
    None
}

/// The point `ext_len` further out from `axis[idx]`, continuing in the same
/// direction the wall's end segment already runs (i.e. straight past the
/// corner, away from the wall's interior). Used to build the corner-
/// extension hint for [`regenerate_wall_representation_with_corner`] (Bug 4).
fn extended_endpoint(axis: &[DVec3], idx: usize, ext_len: f64) -> DVec3 {
    let dir = if idx == 0 {
        (axis[0] - axis[1]).normalize_or_zero()
    } else {
        (axis[idx] - axis[idx - 1]).normalize_or_zero()
    };
    axis[idx] + dir * ext_len
}

pub fn join_two_walls_in_document(scene: &mut Scene, h_a: Handle, h_b: Handle) -> Result<JoinKind, JoinError> {
    let axis_a = get_wall_vertices(scene, h_a);
    let axis_b = get_wall_vertices(scene, h_b);
    match join::join_wall_axes(&axis_a, &axis_b) {
        Ok((new_a, new_b, kind)) => {
            // Bug 4: hint each wall's contour to extend past the shared
            // corner into the other wall's footprint by the other wall's
            // half thickness (a pragmatic approximation that's exact for the
            // common Center-justified case), so the two walls' independent
            // contours overlap at the joint instead of leaving a visible
            // seam where they merely touch.
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
            let override_a = changed_endpoint(&axis_a, &new_a).and_then(|idx| {
                thickness_b.map(|t| (idx, extended_endpoint(&new_a, idx, t * 0.5)))
            });
            let override_b = changed_endpoint(&axis_b, &new_b).and_then(|idx| {
                thickness_a.map(|t| (idx, extended_endpoint(&new_b, idx, t * 0.5)))
            });

            update_wall_vertices(scene, h_a, &new_a);
            update_wall_vertices(scene, h_b, &new_b);
            let _ = regenerate_wall_representation_with_corner(scene, h_a, override_a);
            let _ = regenerate_wall_representation_with_corner(scene, h_b, override_b);
            Ok(kind)
        }
        Err(e) => Err(e),
    }
}

/// `AEC_WALLJOIN_DO handle_a|handle_b` — the non-interactive handler
/// `WallJoinCommand` dispatches to once both walls are picked; resolves each
/// pick to its wall axis, joins the two axes with [`join::join_wall_axes`],
/// writes the trimmed/extended axes back and regenerates both walls'
/// representation. Reports [`JoinError`] via the command line instead of
/// panicking.
pub fn aec_walljoin_do(scene: &mut Scene, command_line: &mut CommandLine, args: &str) {
    let parts: Vec<&str> = args.split('|').collect();
    let [a, b] = parts.as_slice() else {
        command_line.push_error("AEC_WALLJOIN: malformed arguments.");
        return;
    };
    let (Ok(a), Ok(b)) = (a.parse::<u64>(), b.parse::<u64>()) else {
        command_line.push_error("AEC_WALLJOIN: malformed handles.");
        return;
    };
    let h_a = resolve_wall_package(scene, Handle::new(a));
    let h_b = resolve_wall_package(scene, Handle::new(b));
    if h_a == h_b {
        command_line.push_error("AEC_WALLJOIN: select two different walls.");
        return;
    }
    let is_wall = |scene: &Scene, h: Handle| {
        scene.document.get_entity(h).is_some_and(|e| {
            matches!(
                read_aec_record(e).and_then(|r| r.values.first()),
                Some(XDataValue::String(kind)) if kind == "WALL" || kind == "WALL_V2"
            )
        })
    };
    if !is_wall(scene, h_a) || !is_wall(scene, h_b) {
        command_line.push_error("AEC_WALLJOIN: select two wall entities.");
        return;
    }

    match join_two_walls_in_document(scene, h_a, h_b) {
        Ok(_kind) => {
            command_line.push_info("AEC_WALLJOIN: walls joined.");
        }
        Err(e) => {
            command_line.push_error(&format!("AEC_WALLJOIN: {}", e));
        }
    }
}

/// `AEC_WALLEXTEND` — interactive front-end: pick a wall, then either click a
/// target point (extends the nearer axis endpoint to it) or type `W` to
/// switch to picking a target wall (extends to the L-/T-intersection of the
/// two axes, reusing [`join::join_wall_axes`]). Delegates the actual write to
/// [`aec_wallextend_do`] via [`CmdResult::Dispatch`].
pub struct WallExtendCommand {
    wall: Option<Handle>,
    target_is_wall: bool,
}

impl WallExtendCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            wall: None,
            target_is_wall: false,
        }
    }
}

impl CadCommand for WallExtendCommand {
    fn name(&self) -> &'static str {
        "AEC_WALLEXTEND"
    }

    fn prompt(&self) -> String {
        if self.wall.is_none() {
            "AEC_WALLEXTEND  Select wall to extend:".to_string()
        } else {
            "AEC_WALLEXTEND  Specify extend point or [Wall]:".to_string()
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.wall.is_some() && !self.target_is_wall {
            vec![CmdOption::new("Wall", "W")]
        } else {
            Vec::new()
        }
    }

    fn wants_text_input(&self) -> bool {
        self.wall.is_some() && !self.target_is_wall
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.wall.is_some() && !self.target_is_wall && text.trim().eq_ignore_ascii_case("w") {
            self.target_is_wall = true;
            return Some(CmdResult::NeedPoint);
        }
        None
    }

    fn needs_entity_pick(&self) -> bool {
        self.wall.is_none() || self.target_is_wall
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        if self.wall.is_none() {
            self.wall = Some(handle);
            CmdResult::NeedPoint
        } else if self.target_is_wall {
            let wall = self.wall.unwrap();
            CmdResult::Dispatch(format!(
                "AEC_WALLEXTEND_DO {}|WALL|{}",
                wall.value(),
                handle.value()
            ))
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let Some(wall) = self.wall {
            if !self.target_is_wall {
                return CmdResult::Dispatch(format!(
                    "AEC_WALLEXTEND_DO {}|PT|{}|{}|{}",
                    wall.value(),
                    pt.x,
                    pt.y,
                    pt.z
                ));
            }
        }
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// `AEC_WALLEXTEND_DO wall|PT|x|y|z` or `AEC_WALLEXTEND_DO wall|WALL|target`
/// — the non-interactive handler `WallExtendCommand` dispatches to once the
/// target is picked. Resolves the wall pick(s) to their axis, extends the
/// nearer endpoint of `wall`'s axis (to the point, or to the intersection
/// with `target`'s axis via [`join::join_wall_axes`]), writes it back and
/// regenerates the wall's representation. Reports [`JoinError`] via the
/// command line instead of panicking.
pub fn aec_wallextend_do(scene: &mut Scene, command_line: &mut CommandLine, args: &str) {
    let mut parts = args.splitn(2, '|');
    let (Some(wall_str), Some(rest)) = (parts.next(), parts.next()) else {
        command_line.push_error("AEC_WALLEXTEND: malformed arguments.");
        return;
    };
    let Ok(wall_val) = wall_str.parse::<u64>() else {
        command_line.push_error("AEC_WALLEXTEND: malformed handle.");
        return;
    };
    let wall_handle = resolve_wall_package(scene, Handle::new(wall_val));
    let mut axis = get_wall_vertices(scene, wall_handle);
    if axis.len() < 2 {
        command_line.push_error("AEC_WALLEXTEND: select a wall axis with at least two points.");
        return;
    }

    if let Some(pt_args) = rest.strip_prefix("PT|") {
        let coords: Vec<&str> = pt_args.split('|').collect();
        let [x, y, z] = coords.as_slice() else {
            command_line.push_error("AEC_WALLEXTEND: malformed point.");
            return;
        };
        let (Ok(x), Ok(y), Ok(z)) = (x.parse::<f64>(), y.parse::<f64>(), z.parse::<f64>()) else {
            command_line.push_error("AEC_WALLEXTEND: malformed point.");
            return;
        };
        let pt = DVec3::new(x, y, z);
        let d1 = axis[0].distance(pt);
        let d2 = axis.last().unwrap().distance(pt);
        // Project the picked point onto the wall's existing direction line
        // instead of using it directly as the new endpoint — this preserves
        // the wall's original direction exactly, even if `pt` isn't
        // perfectly collinear (e.g. a slightly imprecise pick).
        if d1 < d2 {
            let anchor = *axis.last().unwrap();
            let near = axis[0];
            let dir = near - anchor;
            if dir.length_squared() > 1e-12 {
                let t = (pt - anchor).dot(dir) / dir.length_squared();
                axis[0] = anchor + dir * t;
            } else {
                axis[0] = pt;
            }
        } else {
            let last = axis.len() - 1;
            let anchor = axis[0];
            let near = axis[last];
            let dir = near - anchor;
            if dir.length_squared() > 1e-12 {
                let t = (pt - anchor).dot(dir) / dir.length_squared();
                axis[last] = anchor + dir * t;
            } else {
                axis[last] = pt;
            }
        }
        update_wall_vertices(scene, wall_handle, &axis);
        let _ = regenerate_wall_representation(scene, wall_handle);
        command_line.push_info("AEC_WALLEXTEND: wall extended.");
    } else if let Some(target_str) = rest.strip_prefix("WALL|") {
        let Ok(target_val) = target_str.parse::<u64>() else {
            command_line.push_error("AEC_WALLEXTEND: malformed target handle.");
            return;
        };
        let target_handle = resolve_wall_package(scene, Handle::new(target_val));
        if target_handle == wall_handle {
            command_line.push_error("AEC_WALLEXTEND: select a different target wall.");
            return;
        }
        let axis_b = get_wall_vertices(scene, target_handle);
        match join::join_wall_axes(&axis, &axis_b) {
            Ok((new_a, _new_b, _kind)) => {
                update_wall_vertices(scene, wall_handle, &new_a);
                let _ = regenerate_wall_representation(scene, wall_handle);
                command_line.push_info("AEC_WALLEXTEND: wall extended to target wall.");
            }
            Err(e) => {
                command_line.push_error(&format!("AEC_WALLEXTEND: {}", e));
            }
        }
    } else {
        command_line.push_error("AEC_WALLEXTEND: malformed arguments.");
    }
}

#[cfg(test)]
mod wall_command_tests {
    use super::*;
    use acadrust::Handle;
    use glam::DVec3;

    fn wall_xdata(entity: &EntityType) -> Option<&ExtendedDataRecord> {
        entity.common().extended_data.get_record(AEC_APPID)
    }

    fn wl(material: &str, thickness: f64, function: &str) -> WallLayer {
        WallLayer {
            material: material.to_string(),
            thickness,
            function: function.to_string(),
            gap_before: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
        }
    }

    #[test]
    fn first_point_only_waits_for_the_next_one() {
        let mut cmd = WallCommand::new_with_library(None);
        assert!(matches!(
            cmd.on_point(DVec3::new(0.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));
    }

    #[test]
    fn second_point_commits_a_two_vertex_wall_polyline_with_xdata() {
        let mut cmd = WallCommand::new_with_library(None);
        assert!(matches!(
            cmd.on_point(DVec3::new(0.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));

        match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitLiveEntities(entities) => {
                // Axis + outline contour are committed together from the
                // second point on, even before a wall style is chosen.
                assert_eq!(entities.len(), 2);
                match &entities[0] {
                    EntityType::LwPolyline(pl) => assert_eq!(pl.vertices.len(), 2),
                    _ => panic!("expected a live wall polyline"),
                }
            }
            _ => panic!("second point should commit a live wall polyline"),
        }
    }

    #[test]
    fn later_points_update_the_same_live_polyline_as_a_wall_chain() {
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        let committed = cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        let (entity, contour_entities) = match committed {
            CmdResult::CommitLiveEntities(mut entities) => {
                assert_eq!(entities.len(), 2);
                let contour = entities.split_off(1);
                (entities.remove(0), contour)
            }
            _ => panic!("expected CommitLiveEntities"),
        };
        assert!(
            wall_xdata(&entity).is_some(),
            "committed wall segment should carry OPENCAD_AEC/WALL xdata"
        );
        assert_eq!(contour_entities.len(), 1);

        let handle = Handle::new(7);
        let contour_handle = Handle::new(8);
        cmd.set_live_handles(vec![handle, contour_handle]);
        match cmd.on_point(DVec3::new(5.0, 3.0, 0.0)) {
            CmdResult::UpdateLiveEntities { updates, finish } => {
                assert_eq!(updates.len(), 2);
                let (updated, entity) = &updates[0];
                assert_eq!(*updated, handle);
                match entity {
                    EntityType::LwPolyline(pl) => assert_eq!(pl.vertices.len(), 3),
                    _ => panic!("expected a live wall polyline"),
                }
                assert!(!finish);
            }
            _ => panic!("a third point should extend the same live wall chain"),
        }
    }

    #[test]
    fn undo_drops_the_last_vertex_and_removes_the_live_entity_below_two_points() {
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        cmd.set_live_handles(vec![Handle::new(3), Handle::new(4)]);

        match cmd.on_text_input("U") {
            Some(CmdResult::RemoveLiveEntity(h)) => assert_eq!(h, Handle::new(3)),
            _ => panic!("undoing back to a single vertex should remove the live entity"),
        }
    }

    #[test]
    fn enter_before_two_points_keeps_the_command_running() {
        // Enter/Escape are global "finalize" keys that can fire while the
        // user is still only editing the live height/style panel fields
        // before the axis has a second point — there is nothing to finalize
        // yet, so the command must stay alive instead of cancelling.
        let mut enter_cmd = WallCommand::new_with_library(None);
        assert!(matches!(enter_cmd.on_enter(), CmdResult::NeedPoint));

        let mut escape_cmd = WallCommand::new_with_library(None);
        assert!(matches!(escape_cmd.on_escape(), CmdResult::NeedPoint));

        let mut one_point_cmd = WallCommand::new_with_library(None);
        one_point_cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        assert!(matches!(one_point_cmd.on_enter(), CmdResult::NeedPoint));
    }

    #[test]
    fn enter_after_the_point_chain_finalizes_immediately_with_defaults() {
        // Height/thickness are always visible+editable in the live
        // Properties-panel section while drawing (see `live_properties`), so
        // Enter after the point chain finalizes right away with whatever is
        // currently set (defaults, if the panel wasn't touched), instead of
        // falling back to separate command-line height/thickness prompts.
        let handle = Handle::new(11);
        let contour_handle = Handle::new(12);
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        cmd.set_live_handles(vec![handle, contour_handle]);

        match cmd.on_enter() {
            CmdResult::UpdateLiveEntities { updates, finish } => {
                assert_eq!(updates.len(), 2);
                let (updated, entity) = &updates[0];
                assert_eq!(*updated, handle);
                let pl = match entity {
                    EntityType::LwPolyline(pl) => pl,
                    _ => panic!("expected a live wall polyline"),
                };
                assert!(finish);
                let record = pl
                    .common
                    .extended_data
                    .get_record(AEC_APPID)
                    .expect("finalized wall should carry WALL xdata");
                assert!(
                    matches!(record.values[1], XDataValue::Distance(t) if (t - DEFAULT_WALL_THICKNESS).abs() < 1e-9)
                );
                assert!(
                    matches!(record.values[2], XDataValue::Distance(h) if (h - DEFAULT_WALL_HEIGHT).abs() < 1e-9)
                );
            }
            _ => panic!("expected Enter after the point chain to finalize the live wall"),
        }
    }

    #[test]
    fn live_height_edit_before_enter_finalizes_with_the_edited_value() {
        use crate::command::LiveFieldValue;

        let handle = Handle::new(4);
        let contour_handle = Handle::new(5);
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        cmd.set_live_handles(vec![handle, contour_handle]);

        cmd.apply_live_property("wall_height", LiveFieldValue::Number(3.5));

        match cmd.on_enter() {
            CmdResult::UpdateLiveEntities { updates, finish } => {
                assert_eq!(updates.len(), 2);
                let pl = match &updates[0].1 {
                    EntityType::LwPolyline(pl) => pl,
                    _ => panic!("expected a live wall polyline"),
                };
                assert!(finish);
                let record = pl.common.extended_data.get_record(AEC_APPID).unwrap();
                assert!(
                    matches!(record.values[1], XDataValue::Distance(t) if (t - DEFAULT_WALL_THICKNESS).abs() < 1e-9)
                );
                assert!(matches!(record.values[2], XDataValue::Distance(h) if (h - 3.5).abs() < 1e-9));
            }
            _ => panic!("expected the live-edited height to finalize the live wall"),
        }
    }

    /// Draw four wall segments through `WallCommand` exactly as the
    /// interactive host would (point chain, then height/thickness prompt),
    /// then commit each finalized entity into a real `Scene`. Regression
    /// check for `AEC_ROOM`'s closed-loop detection against interactively
    /// drawn walls (previously only exercised against demo geometry).
    #[test]
    fn aec_room_detects_a_closed_loop_from_interactively_drawn_walls() {
        use crate::ui::command_line::CommandLine;

        let mut scene = Scene::new();
        let corners = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(4.0, 0.0, 0.0),
            DVec3::new(4.0, 3.0, 0.0),
            DVec3::new(0.0, 3.0, 0.0),
            DVec3::new(0.0, 0.0, 0.0),
        ];

        for pair in corners.windows(2) {
            let mut cmd = WallCommand::new_with_library(None);
            let committed = match cmd.on_point(pair[0]) {
                CmdResult::NeedPoint => cmd.on_point(pair[1]),
                other => other,
            };
            let (entity, contour_entity) = match committed {
                CmdResult::CommitLiveEntities(mut entities) => {
                    let contour = if entities.len() > 1 {
                        Some(entities.remove(1))
                    } else {
                        None
                    };
                    (entities.remove(0), contour)
                }
                _ => panic!("two points should commit a live wall segment"),
            };
            let handle = scene.add_entity(entity);
            let mut handles = vec![handle];
            if let Some(c) = contour_entity {
                handles.push(scene.add_entity(c));
            }
            cmd.set_live_handles(handles);

            // Finish the point chain, then accept default height/thickness
            // (Drawing -> AskHeight -> AskThickness -> finalize).
            cmd.on_enter();
            cmd.on_enter();
            let finalized = cmd.on_enter();
            match finalized {
                CmdResult::UpdateLiveEntities { updates, .. } => {
                    for (h, entity) in updates {
                        if let Some(slot) = scene.document.get_entity_mut(h) {
                            *slot = entity;
                        }
                    }
                }
                _ => panic!("height/thickness prompt should finalize the wall segment"),
            }
        }

        let mut command_line = CommandLine::default();
        aec_room(&mut scene, &mut command_line);

        let room_record = scene
            .document
            .entities()
            .filter_map(read_aec_record)
            .find(|r| matches!(r.values.first(), Some(XDataValue::String(k)) if k == "ROOM"))
            .expect("aec_room should have written a ROOM xdata record");
        let area = match room_record.values.get(2) {
            Some(XDataValue::Real(a)) => *a,
            _ => panic!("ROOM record should carry an area value"),
        };
        assert!(
            (area - 12.0).abs() < 1e-6,
            "expected the detected 4x3 wall loop to yield area 12.0, got {area}"
        );
    }

    /// `wall_from_entity` is the inverse of `wall_record` — the properties
    /// panel reads a `Wall` this way to populate the height/thickness/material
    /// fields for a WALL-tagged entity.
    #[test]
    fn wall_from_entity_reads_back_a_finalized_wall_record() {
        use crate::command::LiveFieldValue;

        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        cmd.set_live_handles(vec![Handle::new(9), Handle::new(10)]);
        cmd.apply_live_property("wall_height", LiveFieldValue::Number(3.5));
        let entity = match cmd.on_enter() {
            CmdResult::UpdateLiveEntities { mut updates, .. } => updates.remove(0).1,
            _ => panic!("expected Enter to finalize the live wall"),
        };

        let wall = wall_from_entity(&entity).expect("finalized entity should read back as a Wall");
        assert!((wall.thickness - DEFAULT_WALL_THICKNESS).abs() < 1e-9);
        assert!((wall.height - 3.5).abs() < 1e-9);
        assert!(wall.material_ref.is_none());
    }

    /// A plain (non-WALL-tagged) entity must not be misread as a wall — this
    /// is what keeps the properties-panel Wall section from appearing on
    /// regular polylines.
    #[test]
    fn wall_from_entity_returns_none_for_a_plain_polyline() {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(1.0, 0.0)));
        let entity = EntityType::LwPolyline(pl);
        assert!(wall_from_entity(&entity).is_none());
    }

    /// `write_wall_properties` is the properties-panel writeback path: it
    /// must reuse `wall_record`'s exact layout so `wall_from_entity` and
    /// `collect_wall_segments`/`aec_room` keep working after an edit.
    #[test]
    fn write_wall_properties_updates_the_wall_xdata_in_place() {
        let mut scene = Scene::new();
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitLiveEntities(mut entities) => entities.remove(0),
            _ => panic!("two points should commit a live wall segment"),
        };
        let handle = scene.add_entity(entity);

        let mut wall = wall_from_entity(scene.document.get_entity(handle).unwrap())
            .expect("committed segment should already carry WALL xdata");
        wall.height = 3.2;
        wall.thickness = 0.25;
        wall.material_ref = Some("Concrete".to_string());
        assert!(write_wall_properties(&mut scene.document, handle, &wall));

        let updated = wall_from_entity(scene.document.get_entity(handle).unwrap())
            .expect("entity should still read back as a wall after the edit");
        assert!((updated.height - 3.2).abs() < 1e-9);
        assert!((updated.thickness - 0.25).abs() < 1e-9);
        assert_eq!(updated.material_ref.as_deref(), Some("Concrete"));

        // The AEC_ROOM segment collector still sees this wall after the edit.
        let segments = collect_wall_segments(&scene.document);
        assert_eq!(segments.len(), 1);
    }

    #[test]
    fn wall_v2_round_trip() {
        let layers = vec![
            wl("Finish", 0.02, "Finish"),
            wl("Brick", 0.10, "Structural"),
            wl("Finish", 0.02, "Finish"),
        ];
        let values = wall_v2_record("style1", 3.0, 1, &layers, &[], WallJustification::Center);
        let pl = LwPolyline::new();
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = values;
        entity.common_mut().extended_data.add_record(record);

        let wall = wall_v2_from_entity(&entity).expect("Should parse WALL_V2");
        assert_eq!(wall.style_id, "style1");
        assert_eq!(wall.height, 3.0);
        assert_eq!(wall.storey_id, 1);
        assert_eq!(wall.layers.len(), 3);
        assert_eq!(wall.layers[1].material, "Brick");
        assert_eq!(wall.layers[1].thickness, 0.10);
        assert_eq!(wall.layers[1].function, "Structural");
        assert_eq!(wall.total_thickness(), 0.14);
    }

    #[test]
    fn wall_v2_from_entity_returns_none_for_legacy_wall() {
        let w = Wall {
            thickness: 0.2,
            height: 2.8,
            material_ref: Some("Concrete".to_string()),
            storey_id: 1,
        };
        let pl = LwPolyline::new();
        let mut entity = EntityType::LwPolyline(pl);
        entity.common_mut().extended_data.add_record(wall_record(&w));

        assert!(wall_v2_from_entity(&entity).is_none());
    }

    #[test]
    fn wall_thickness_and_height_supports_both_versions() {
        let pl_v1 = LwPolyline::new();
        let mut e_v1 = EntityType::LwPolyline(pl_v1);
        let w1 = Wall {
            thickness: 0.2,
            height: 2.8,
            material_ref: None,
            storey_id: 0,
        };
        e_v1.common_mut().extended_data.add_record(wall_record(&w1));

        let res1 = wall_thickness_and_height(&e_v1).expect("Should read V1");
        assert_eq!(res1, (0.2, 2.8, 0));

        let pl_v2 = LwPolyline::new();
        let mut e_v2 = EntityType::LwPolyline(pl_v2);
        let layers = vec![wl("Mat", 0.15, "Func")];
        let mut rec2 = ExtendedDataRecord::new(AEC_APPID);
        rec2.values = wall_v2_record("style2", 3.2, 2, &layers, &[], WallJustification::Center);
        e_v2.common_mut().extended_data.add_record(rec2);

        let res2 = wall_thickness_and_height(&e_v2).expect("Should read V2");
        assert_eq!(res2, (0.15, 3.2, 2));
    }

    #[test]
    fn aec_room_detects_a_closed_loop_from_mixed_wall_versions() {
        use crate::ui::command_line::CommandLine;

        let mut scene = Scene::new();
        let corners = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(4.0, 0.0, 0.0),
            DVec3::new(4.0, 3.0, 0.0),
            DVec3::new(0.0, 3.0, 0.0),
            DVec3::new(0.0, 0.0, 0.0),
        ];

        for (i, pair) in corners.windows(2).enumerate() {
            let mut pl = LwPolyline::new();
            pl.add_vertex(LwVertex::new(Vector2::new(pair[0].x, pair[0].y)));
            pl.add_vertex(LwVertex::new(Vector2::new(pair[1].x, pair[1].y)));
            let mut entity = EntityType::LwPolyline(pl);

            let record = if i % 2 == 0 {
                // Version 1
                let w = Wall {
                    thickness: 0.2,
                    height: 2.8,
                    material_ref: None,
                    storey_id: 0,
                };
                wall_record(&w)
            } else {
                // Version 2
                let layers = vec![wl("Brick", 0.2, "Structural")];
                let mut r = ExtendedDataRecord::new(AEC_APPID);
                r.values = wall_v2_record("style1", 2.8, 0, &layers, &[], WallJustification::Center);
                r
            };
            entity.common_mut().extended_data.add_record(record);
            scene.add_entity(entity);
        }

        let mut command_line = CommandLine::default();
        aec_room(&mut scene, &mut command_line);

        let room_record = scene
            .document
            .entities()
            .filter_map(read_aec_record)
            .find(|r| matches!(r.values.first(), Some(XDataValue::String(k)) if k == "ROOM"))
            .expect("aec_room should have written a ROOM xdata record");
        let area = match room_record.values.get(2) {
            Some(XDataValue::Real(a)) => *a,
            _ => panic!("ROOM record should carry an area value"),
        };
        assert!((area - 12.0).abs() < 1e-6);
    }

    #[test]
    fn wall_command_with_library_uses_ask_style_and_finalizes_v2() {
        use crate::command::LiveFieldValue;
        use crate::modules::aec::engine::material::Material;
        use crate::modules::aec::engine::style::Style;
        use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, WallStyle};

        let material = Material::new(
            "brick_id".to_string(),
            "Brick Material".to_string(),
            "ANSI31".to_string(),
            0xFF0000,
            "Continuous".to_string(),
        );
        let style = WallStyle {
            style: Style {
                id: "style1".to_string(),
                name: "Standard Wall".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: vec![Layer {
                material_id: "brick_id".to_string(),
                thickness: 0.25,
                function: LayerFunction::Structural,
                gap_before: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
            }],
        };
        let lib = StyleLibrary {
            materials: vec![material],
            wall_styles: vec![style],
        };

        let mut cmd = WallCommand::new_with_library(Some(lib));
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        let handle = Handle::new(100);
        cmd.set_live_handles(vec![handle]);

        // Style/height are picked live via the Properties-panel fields while
        // drawing (see `apply_live_property`), so Enter after the point
        // chain finalizes immediately with a V2 record — no more separate
        // command-line AskStyle/AskHeight prompts.
        match cmd.apply_live_property(
            "wall_style",
            LiveFieldValue::Picker("style1".to_string()),
        ) {
            CmdResult::ReplaceEntity(old, new_entities) => {
                // Gaining a contour preview replaces the axis-only live
                // entity with axis + contour; simulate the host echoing the
                // newly assigned handles back to the command.
                let new_handles: Vec<Handle> =
                    (0..new_entities.len()).map(|i| Handle::new(300 + i as u64)).collect();
                cmd.on_entity_replaced(old, &new_handles);
            }
            _ => panic!("expected style assignment to replace the live entity"),
        }
        cmd.apply_live_property("wall_height", LiveFieldValue::Number(3.0));

        match cmd.on_enter() {
            CmdResult::UpdateLiveEntities { updates, finish } => {
                assert_eq!(updates.len(), 2);
                let pl = match &updates[0].1 {
                    EntityType::LwPolyline(pl) => pl,
                    _ => panic!("expected a live wall polyline"),
                };
                assert!(finish);
                let record = pl.common.extended_data.get_record(AEC_APPID).unwrap();
                assert_eq!(record.values[0], XDataValue::String("WALL_V2".to_string()));
                assert_eq!(record.values[1], XDataValue::String("style1".to_string()));
                assert!(
                    matches!(record.values[2], XDataValue::Distance(h) if (h - 3.0).abs() < 1e-9)
                );
                // Material name "Brick Material" should be used, not "brick_id"
                assert_eq!(
                    record.values[5],
                    XDataValue::String("Brick Material".to_string())
                );
                assert!(
                    matches!(record.values[6], XDataValue::Distance(t) if (t - 0.25).abs() < 1e-9)
                );
                assert_eq!(
                    record.values[7],
                    XDataValue::String("Structural".to_string())
                );
            }
            _ => panic!("Expected Enter to finalize wall with V2 record"),
        }
    }

    #[test]
    fn wall_command_live_properties_reports_style_and_height_while_drawing() {
        use crate::command::LiveFieldValue;

        let mut cmd = WallCommand::new_with_library(None);
        // Still in the Drawing phase (no points yet): live_properties should
        // be available with the default height and an empty style name.
        let live = cmd.live_properties().expect("Drawing phase should expose live properties");
        assert_eq!(live.title, "Wall");
        assert_eq!(live.fields.len(), 2);
        assert_eq!(live.fields[0].field_id, "wall_style");
        assert_eq!(live.fields[1].field_id, "wall_height");
        assert!(matches!(&live.fields[1].value, LiveFieldValue::Number(h) if (*h - DEFAULT_WALL_HEIGHT).abs() < 1e-9));

        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        cmd.set_live_handles(vec![Handle::new(200)]);

        cmd.apply_live_property("wall_height", LiveFieldValue::Number(3.5));
        assert!((cmd.wall.height - 3.5).abs() < 1e-9);

        let live_after = cmd.live_properties().expect("still drawing");
        assert!(matches!(&live_after.fields[1].value, LiveFieldValue::Number(h) if (*h - 3.5).abs() < 1e-9));
    }

    #[test]
    fn wall_command_apply_live_property_updates_style_and_resolved_layers() {
        use crate::command::LiveFieldValue;
        use crate::modules::aec::engine::material::Material;
        use crate::modules::aec::engine::style::Style;
        use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, WallStyle};

        let material = Material::new(
            "brick_id".to_string(),
            "Brick Material".to_string(),
            "ANSI31".to_string(),
            0xFF0000,
            "Continuous".to_string(),
        );
        let style = WallStyle {
            style: Style {
                id: "style1".to_string(),
                name: "Standard Wall".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: vec![Layer {
                material_id: "brick_id".to_string(),
                thickness: 0.25,
                function: LayerFunction::Structural,
                gap_before: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
            }],
        };
        let lib = StyleLibrary {
            materials: vec![material],
            wall_styles: vec![style],
        };

        let mut cmd = WallCommand::new_with_library(Some(lib));
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        cmd.set_live_handles(vec![Handle::new(201)]);

        cmd.apply_live_property(
            "wall_style",
            LiveFieldValue::Picker("style1".to_string()),
        );

        assert_eq!(cmd.style_id.as_deref(), Some("style1"));
        let layers = cmd.resolved_layers.clone().expect("style pick should resolve layers");
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].material, "Brick Material");
        assert!((layers[0].thickness - 0.25).abs() < 1e-9);
        assert_eq!(layers[0].function, "Structural");

        let live = cmd.live_properties().expect("still drawing");
        assert!(matches!(&live.fields[0].value, LiveFieldValue::Picker(s) if s == "Standard Wall"));
    }

    #[test]
    fn wall_command_refuses_to_finish_without_a_style_when_styles_are_available() {
        use crate::modules::aec::engine::material::Material;
        use crate::modules::aec::engine::style::Style;
        use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, WallStyle};

        let material = Material::new(
            "brick_id".to_string(),
            "Brick Material".to_string(),
            "ANSI31".to_string(),
            0xFF0000,
            "Continuous".to_string(),
        );
        let style = WallStyle {
            style: Style {
                id: "style1".to_string(),
                name: "Standard Wall".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: vec![Layer {
                material_id: "brick_id".to_string(),
                thickness: 0.25,
                function: LayerFunction::Structural,
                gap_before: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
            }],
        };
        let lib = StyleLibrary {
            materials: vec![material],
            wall_styles: vec![style],
        };

        let mut cmd = WallCommand::new_with_library(Some(lib));
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        cmd.set_live_handles(vec![Handle::new(202)]);

        // No style picked yet -> Enter must not finalize; it must keep the
        // command running and surface a hint instead.
        assert!(matches!(cmd.on_enter(), CmdResult::NeedPoint));
        assert!(cmd.prompt().to_lowercase().contains("style"));

        // Picking a style afterwards clears the warning and lets Enter
        // finalize normally.
        match cmd.apply_live_property(
            "wall_style",
            crate::command::LiveFieldValue::Picker("style1".to_string()),
        ) {
            CmdResult::ReplaceEntity(old, new_entities) => {
                let new_handles: Vec<Handle> =
                    (0..new_entities.len()).map(|i| Handle::new(400 + i as u64)).collect();
                cmd.on_entity_replaced(old, &new_handles);
            }
            _ => panic!("expected style assignment to replace the live entity"),
        }
        assert!(!cmd.prompt().to_lowercase().contains("please select"));
        match cmd.on_enter() {
            CmdResult::UpdateLiveEntities { finish, .. } => assert!(finish),
            _ => panic!("expected Enter to finalize once a style was picked"),
        }
    }

    #[test]
    fn wall_command_with_no_library_falls_back_to_v1_record() {
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        let handle = Handle::new(101);
        let contour_handle = Handle::new(102);
        cmd.set_live_handles(vec![handle, contour_handle]);

        // With no style library, Enter after the point chain finalizes
        // immediately with a plain V1 WALL record using the current
        // height/thickness defaults (also editable live via the Properties
        // panel, see `apply_live_property`).
        match cmd.on_enter() {
            CmdResult::UpdateLiveEntities { updates, finish } => {
                assert_eq!(updates.len(), 2);
                let pl = match &updates[0].1 {
                    EntityType::LwPolyline(pl) => pl,
                    _ => panic!("expected a live wall polyline"),
                };
                assert!(finish);
                let record = pl.common.extended_data.get_record(AEC_APPID).unwrap();
                assert_eq!(record.values[0], XDataValue::String("WALL".to_string()));
            }
            _ => panic!("Expected Enter to finalize wall with V1 record"),
        }
    }

    #[test]
    fn material_command_collects_fields_and_dispatches_add_command() {
        let mut cmd = MaterialCommand::new();
        assert!(cmd.prompt().contains("name"));

        assert!(matches!(
            cmd.on_text_input("Sichtbeton"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(cmd.prompt().contains("hatch"));

        assert!(matches!(
            cmd.on_text_input(""), // blank -> default hatch
            Some(CmdResult::NeedPoint)
        ));
        assert!(cmd.prompt().contains("color"));

        assert!(matches!(
            cmd.on_text_input("A0A0A0"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(cmd.prompt().contains("line type"));

        match cmd.on_text_input("") {
            Some(CmdResult::Dispatch(dispatch)) => {
                assert_eq!(
                    dispatch,
                    "AEC_MATERIAL_ADD Sichtbeton|ANSI31|A0A0A0|Continuous"
                );
            }
            _ => panic!("expected the final field to dispatch AEC_MATERIAL_ADD"),
        }
    }

    #[test]
    fn material_command_requires_a_non_empty_name() {
        let mut cmd = MaterialCommand::new();
        assert!(matches!(cmd.on_text_input(""), Some(CmdResult::NeedPoint)));
        // Still on the name step.
        assert!(cmd.prompt().contains("name") && !cmd.prompt().contains("hatch"));
    }

    #[test]
    fn style_command_collects_two_layers_and_dispatches_add_command() {
        let mut cmd = StyleCommand::new();
        assert!(cmd.prompt().contains("name"));

        assert!(matches!(
            cmd.on_text_input("Testwand"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(cmd.prompt().contains("parent"));

        assert!(matches!(
            cmd.on_text_input(""), // no parent
            Some(CmdResult::NeedPoint)
        ));
        assert!(cmd.prompt().contains("material name"));

        // Layer 1
        assert!(matches!(
            cmd.on_text_input("Putz"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            cmd.on_text_input("0.015"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            cmd.on_text_input("Finish"),
            Some(CmdResult::NeedPoint)
        ));

        // Layer 2
        assert!(matches!(
            cmd.on_text_input("Mauerwerk"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            cmd.on_text_input("0.24"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            cmd.on_text_input("Structural"),
            Some(CmdResult::NeedPoint)
        ));

        // Blank material name ends the layer loop and dispatches.
        match cmd.on_text_input("") {
            Some(CmdResult::Dispatch(dispatch)) => {
                assert_eq!(
                    dispatch,
                    "AEC_STYLE_ADD Testwand||Putz:0.015:Finish;Mauerwerk:0.24:Structural"
                );
            }
            _ => panic!("expected the final blank layer entry to dispatch AEC_STYLE_ADD"),
        }
    }

    #[test]
    fn style_command_with_no_layers_dispatches_empty_layer_list_for_inheritance() {
        let mut cmd = StyleCommand::new();
        cmd.on_text_input("Kind Wand");
        cmd.on_text_input("Standard Wall"); // parent

        match cmd.on_text_input("") {
            Some(CmdResult::Dispatch(dispatch)) => {
                assert_eq!(dispatch, "AEC_STYLE_ADD Kind Wand|Standard Wall|");
            }
            _ => panic!("expected an empty layer list to still dispatch AEC_STYLE_ADD"),
        }
    }

    #[test]
    fn layer_function_round_trips_through_its_plain_string_form() {
        for f in [
            LayerFunction::Structural,
            LayerFunction::Insulation,
            LayerFunction::Finish,
        ] {
            let s = layer_function_to_str(&f);
            assert_eq!(parse_layer_function(&s), f);
        }
        assert_eq!(parse_layer_function(""), LayerFunction::Structural);
        assert_eq!(
            parse_layer_function("Custom"),
            LayerFunction::Other("Custom".to_string())
        );
    }

    #[test]
    fn wall_justification_conversion() {
        // Horizontal wall along X axis from 0 to 10.
        // Thickness = 0.2.
        // Points picked at Interior (Y=+0.1 if drawing left to right).
        // Centerline should be at Y=0.

        let mut cmd = WallCommand::new();
        cmd.justification = WallJustification::Interior;
        cmd.wall.thickness = 0.2;

        cmd.on_point(DVec3::new(0.0, 0.1, 0.0));
        cmd.on_point(DVec3::new(10.0, 0.1, 0.0));

        let entity = cmd.build_entity().expect("Should build entity");
        let EntityType::LwPolyline(pl) = entity else {
            panic!("Expected LwPolyline")
        };

        assert_eq!(pl.vertices.len(), 2);
        // Interior justification for a segment (0, 0.1) -> (10, 0.1)
        // normal is (0, 1).
        // offset in build_entity is -0.1.
        // Centerline = Picked + (0, 1) * -0.1 = (0, 0).
        assert!((pl.vertices[0].location.x - 0.0).abs() < 1e-9);
        assert!((pl.vertices[0].location.y - 0.0).abs() < 1e-9);
        assert!((pl.vertices[1].location.x - 10.0).abs() < 1e-9);
        assert!((pl.vertices[1].location.y - 0.0).abs() < 1e-9);
    }

    #[test]
    fn wall_justification_toggle_cycle() {
        let mut cmd = WallCommand::new();
        assert_eq!(cmd.justification, WallJustification::Center);

        cmd.set_ctrl(true);
        assert_eq!(cmd.justification, WallJustification::Exterior);
        cmd.set_ctrl(true); // Should not toggle again while held
        assert_eq!(cmd.justification, WallJustification::Exterior);

        cmd.set_ctrl(false);
        cmd.set_ctrl(true);
        assert_eq!(cmd.justification, WallJustification::Interior);

        cmd.set_ctrl(false);
        cmd.set_ctrl(true);
        assert_eq!(cmd.justification, WallJustification::Center);
    }

    #[test]
    fn slugify_normalizes_names_into_stable_ids() {
        assert_eq!(slugify("Wand Stahlbeton 20cm"), "wand_stahlbeton_20cm");
        assert_eq!(slugify("  spaced  out  "), "spaced_out");
        assert_eq!(slugify(""), "item");
    }

    #[test]
    fn wall_v2_round_trip_with_derived_handles() {
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let derived = vec![Handle::new(10), Handle::new(11), Handle::new(12)];
        let values = wall_v2_record("style1", 3.0, 0, &layers, &derived, WallJustification::Center);
        let pl = LwPolyline::new();
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = values;
        entity.common_mut().extended_data.add_record(record);

        let wall = wall_v2_from_entity(&entity).expect("Should parse WALL_V2");
        assert_eq!(wall.derived_handles, derived);
    }

    #[test]
    fn wall_v2_without_derived_handles_tail_still_parses() {
        // Simulate an old record written before `derived_handles` existed:
        // build it with an empty list and confirm it reads back empty, not
        // an error, keeping legacy records readable.
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let values = wall_v2_record("style1", 3.0, 0, &layers, &[], WallJustification::Center);
        let pl = LwPolyline::new();
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = values;
        entity.common_mut().extended_data.add_record(record);

        let wall = wall_v2_from_entity(&entity).expect("Should parse WALL_V2");
        assert!(wall.derived_handles.is_empty());
    }

    /// Build a two-layer `WALL_V2` axis polyline in `scene` and return its
    /// handle.
    fn add_multi_layer_wall(scene: &mut Scene) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![
            wl("Concrete", 0.2, "Structural"),
            wl("Insulation", 0.05, "Insulation"),
        ];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_v2_record("style1", 3.0, 0, &layers, &[], WallJustification::Center);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }

    /// Mirrors the core write-back logic of the `AecStylePickerConfirm`
    /// handler for `StylePickerTarget::WallPropertiesStyle` (see
    /// `src/app/update/mod.rs`): resolve `effective_layers()` for the newly
    /// chosen style from the currently loaded library, then write the new
    /// `style_id` + resolved layer snapshot back into the wall's `WALL_V2`
    /// XDATA and regenerate its representation.
    #[test]
    fn changing_wall_properties_style_updates_style_id_and_layer_snapshot() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);

        let mut wall_styles: HashMap<String, WallStyle> = HashMap::new();
        wall_styles.insert(
            "style1".to_string(),
            WallStyle {
                style: Style {
                    id: "style1".to_string(),
                    name: "Style One".to_string(),
                    object_kind: "Wall".to_string(),
                    parent_style_id: None,
                },
                layers: vec![Layer {
                    material_id: "Concrete".to_string(),
                    thickness: 0.2,
                    function: LayerFunction::Structural,
                    gap_before: 0.0,
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    layer_override: None,
                }],
            },
        );
        wall_styles.insert(
            "style2".to_string(),
            WallStyle {
                style: Style {
                    id: "style2".to_string(),
                    name: "Style Two".to_string(),
                    object_kind: "Wall".to_string(),
                    parent_style_id: None,
                },
                layers: vec![
                    Layer {
                        material_id: "Brick".to_string(),
                        thickness: 0.1,
                        function: LayerFunction::Finish,
                        gap_before: 0.0,
                        bottom_offset: 0.0,
                        top_offset: 0.0,
                        layer_override: None,
                    },
                    Layer {
                        material_id: "Insulation".to_string(),
                        thickness: 0.06,
                        function: LayerFunction::Insulation,
                        gap_before: 0.0,
                        bottom_offset: 0.0,
                        top_offset: 0.0,
                        layer_override: None,
                    },
                ],
            },
        );

        let new_style_id = "style2".to_string();
        let layers = effective_layers(&wall_styles, &new_style_id).expect("style2 should resolve");
        let wall_layers: Vec<WallLayer> = layers
            .into_iter()
            .map(|l| WallLayer {
                material: l.material_id.clone(),
                thickness: l.thickness,
                function: match &l.function {
                    LayerFunction::Structural => "Structural".to_string(),
                    LayerFunction::Insulation => "Insulation".to_string(),
                    LayerFunction::Finish => "Finish".to_string(),
                    LayerFunction::Other(s) => s.clone(),
                },
                gap_before: l.gap_before,
                bottom_offset: l.bottom_offset,
                top_offset: l.top_offset,
                layer_override: l.layer_override.clone(),
            })
            .collect();

        let mut wall_v2 = wall_v2_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should parse as WALL_V2");
        wall_v2.style_id = new_style_id.clone();
        wall_v2.layers = wall_layers.clone();

        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_v2_record(
            &wall_v2.style_id,
            wall_v2.height,
            wall_v2.storey_id,
            &wall_v2.layers,
            &wall_v2.derived_handles,
            wall_v2.justification,
        );

        let entity = scene.document.get_entity_mut(wall_handle).unwrap();
        let xd = &mut entity.common_mut().extended_data;
        let kept: Vec<_> = xd
            .records()
            .iter()
            .filter(|r| r.application_name != AEC_APPID)
            .cloned()
            .collect();
        xd.clear();
        for r in kept {
            xd.add_record(r);
        }
        xd.add_record(record);

        regenerate_wall_representation(&mut scene, wall_handle)
            .expect("regeneration should succeed after style change");

        let updated = wall_v2_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should still parse as WALL_V2");
        assert_eq!(updated.style_id, "style2");
        assert_eq!(updated.layers.len(), 2);
        assert_eq!(updated.layers[0].material, "Brick");
        assert_eq!(updated.layers[1].material, "Insulation");
    }

    #[test]
    fn resolve_wall_package_returns_axis_for_a_derived_entity() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, wall_handle)
            .expect("regeneration should succeed for a valid two-layer wall");

        let derived = wall_v2_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should still read back as WALL_V2")
            .derived_handles;
        assert!(!derived.is_empty());

        for handle in derived {
            assert_eq!(resolve_wall_package(&scene, handle), wall_handle);
        }
    }

    #[test]
    fn resolve_wall_package_returns_itself_for_a_non_wall_entity() {
        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(1.0, 0.0)));
        let handle = scene.add_entity(EntityType::LwPolyline(pl));

        assert_eq!(resolve_wall_package(&scene, handle), handle);
    }

    #[test]
    fn resolve_wall_package_falls_back_to_clicked_when_axis_is_missing() {
        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        let handle = scene.add_entity(EntityType::LwPolyline(pl));

        // Tag it as derived from a handle that doesn't exist in the document.
        let stale_axis = Handle::new(999_999);
        write_wall_derived_tag(&mut scene, handle, stale_axis);

        assert_eq!(resolve_wall_package(&scene, handle), handle);
    }

    /// Bug 2: only the wall AXIS should ever be a snap candidate — never its
    /// derived shell/hatch/solid contours.
    #[test]
    fn wall_axis_snap_wires_excludes_derived_and_includes_axis() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, wall_handle)
            .expect("regeneration should succeed for a valid two-layer wall");

        let wall = wall_v2_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should still read back as WALL_V2");
        assert!(!wall.derived_handles.is_empty());

        // Sanity: the axis lives on the invisible axis layer; its derived
        // contour does not.
        assert_eq!(
            scene.document.get_entity(wall_handle).unwrap().common().layer,
            AEC_WALL_AXIS_LAYER
        );
        let derived_handle = wall.derived_handles[0];
        assert_ne!(
            scene
                .document
                .get_entity(derived_handle)
                .unwrap()
                .common()
                .layer,
            AEC_WALL_AXIS_LAYER
        );

        // Simulate the generic (render-shared) wire set: since the axis
        // layer is off, only the derived entity's wire is present there —
        // the axis itself is entirely absent.
        let derived_entity = scene.document.get_entity(derived_handle).unwrap().clone();
        let raw_wires = std::sync::Arc::new(scene.wires_for_entities(&[derived_entity]));
        assert!(raw_wires
            .iter()
            .any(|w| w.name == derived_handle.value().to_string()));

        let filtered = wall_axis_snap_wires(&scene, raw_wires);

        assert!(
            !filtered
                .iter()
                .any(|w| w.name == derived_handle.value().to_string()),
            "wall-derived contour/hatch/solid wires must be excluded from snap candidates"
        );
        assert!(
            filtered
                .iter()
                .any(|w| w.name == wall_handle.value().to_string()),
            "the wall axis must be included as a snap candidate even though its layer is off"
        );
    }

    /// Bug 4: after two walls are joined, each wall's contour must extend
    /// past the shared corner into the other wall's footprint — verifying
    /// the pragmatic corner-overlap fix in `join_two_walls_in_document` /
    /// `regenerate_wall_representation_with_corner`.
    #[test]
    fn join_two_walls_extends_contours_into_shared_corner() {
        let mut scene = Scene::new();
        // Wall A: (0,0) -> (5,0), two layers, total thickness 0.25.
        let wall_a = add_multi_layer_wall(&mut scene);

        // Wall B: (6,1) -> (6,10), one layer, thickness 0.3. Meets wall A in
        // an L-junction at (6,0) — mirrors `join::tests::test_l_join`.
        let mut pl_b = LwPolyline::new();
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
        let mut entity_b = EntityType::LwPolyline(pl_b);
        let layers_b = vec![wl("Concrete", 0.3, "Structural")];
        let mut record_b = ExtendedDataRecord::new(AEC_APPID);
        record_b.values = wall_v2_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center);
        entity_b.common_mut().extended_data.add_record(record_b);
        let wall_b = scene.add_entity(entity_b);

        regenerate_wall_representation(&mut scene, wall_a)
            .expect("wall A regeneration should succeed");
        regenerate_wall_representation(&mut scene, wall_b)
            .expect("wall B regeneration should succeed");

        let kind = join_two_walls_in_document(&mut scene, wall_a, wall_b)
            .expect("the two axes should join as an L-corner");
        assert_eq!(kind, JoinKind::L);

        // The trimmed axes must still meet exactly at the corner — the
        // corner-extension hint must not leak into the persisted axis.
        let axis_a = get_wall_vertices(&scene, wall_a);
        let axis_b = get_wall_vertices(&scene, wall_b);
        assert_eq!(*axis_a.last().unwrap(), DVec3::new(6.0, 0.0, 0.0));
        assert_eq!(*axis_b.first().unwrap(), DVec3::new(6.0, 0.0, 0.0));

        // Wall A's contour (thickness 0.25) should reach past x=6 by (at
        // least close to) wall B's half thickness (0.15), overlapping into
        // wall B's own footprint instead of stopping flush at the corner.
        let wall_a_v2 = wall_v2_from_entity(scene.document.get_entity(wall_a).unwrap())
            .expect("wall A should still read back as WALL_V2");
        let max_x = wall_a_v2
            .derived_handles
            .iter()
            .filter_map(|h| scene.document.get_entity(*h))
            .filter_map(|e| match e {
                EntityType::LwPolyline(pl) => Some(pl),
                _ => None,
            })
            .flat_map(|pl| pl.vertices.iter().map(|v| v.location.x))
            .fold(f64::MIN, f64::max);
        assert!(
            max_x > 6.0 + 1e-6,
            "wall A's contour should extend past the corner into wall B's footprint, got max_x={max_x}"
        );
    }

    #[test]
    fn regenerate_wall_representation_builds_layers_and_avoids_duplication() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        let before = scene.document.entities().count();

        regenerate_wall_representation(&mut scene, wall_handle)
            .expect("regeneration should succeed for a valid two-layer wall");
        let after_first = scene.document.entities().count();
        assert!(
            after_first > before,
            "regeneration should have created new contour/hatch/solid entities"
        );

        let wall = wall_v2_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("wall should still be readable as WALL_V2 after regeneration");
        let derived_after_first = wall.derived_handles.clone();
        assert!(!derived_after_first.is_empty());

        // Calling it again must replace, not accumulate, the derived
        // entities.
        regenerate_wall_representation(&mut scene, wall_handle)
            .expect("second regeneration should also succeed");
        let after_second = scene.document.entities().count();
        assert_eq!(
            after_first, after_second,
            "regenerating twice should not duplicate derived entities"
        );

        let wall2 = wall_v2_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("wall should still be readable as WALL_V2 after second regeneration");
        assert_eq!(wall2.derived_handles.len(), derived_after_first.len());
    }

    #[test]
    fn regenerate_wall_representation_respects_layer_override() {
        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut overridden = wl("Concrete", 0.2, "Structural");
        overridden.layer_override = Some("AEC_OVERRIDE_LAYER".to_string());
        let default_layer = wl("Insulation", 0.05, "Insulation");
        let layers = vec![overridden, default_layer];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_v2_record("style1", 3.0, 0, &layers, &[], WallJustification::Center);
        entity.common_mut().extended_data.add_record(record);
        let wall_handle = scene.add_entity(entity);

        regenerate_wall_representation(&mut scene, wall_handle)
            .expect("regeneration should succeed for a valid two-layer wall");

        let derived = wall_v2_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should still read back as WALL_V2")
            .derived_handles;
        assert!(!derived.is_empty());

        let mut saw_override_layer = false;
        let mut saw_default_layer = false;
        for h in derived {
            let e = scene.document.get_entity(h).unwrap();
            if e.common().layer == "AEC_OVERRIDE_LAYER" {
                saw_override_layer = true;
            } else if e.common().layer == "0" || !e.common().layer.is_empty() {
                saw_default_layer = true;
            }
        }
        assert!(
            saw_override_layer,
            "at least one derived entity of the overridden layer should be placed on AEC_OVERRIDE_LAYER"
        );
        assert!(
            saw_default_layer,
            "the non-overridden layer's derived entities should keep using the default layer"
        );
        assert!(scene.document.layers.contains("AEC_OVERRIDE_LAYER"));
    }

    #[test]
    fn wall_axis_still_detected_by_room_loop_after_regeneration() {
        use crate::ui::command_line::CommandLine;

        let mut scene = Scene::new();
        let corners = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(4.0, 0.0, 0.0),
            DVec3::new(4.0, 3.0, 0.0),
            DVec3::new(0.0, 3.0, 0.0),
            DVec3::new(0.0, 0.0, 0.0),
        ];

        let mut handles = Vec::new();
        for pair in corners.windows(2) {
            let mut pl = LwPolyline::new();
            pl.add_vertex(LwVertex::new(Vector2::new(pair[0].x, pair[0].y)));
            pl.add_vertex(LwVertex::new(Vector2::new(pair[1].x, pair[1].y)));
            let mut entity = EntityType::LwPolyline(pl);
            let layers = vec![wl("Concrete", 0.2, "Structural")];
            let mut record = ExtendedDataRecord::new(AEC_APPID);
            record.values = wall_v2_record("style1", 2.8, 0, &layers, &[], WallJustification::Center);
            entity.common_mut().extended_data.add_record(record);
            handles.push(scene.add_entity(entity));
        }

        // Regenerate every wall: this moves each axis polyline onto the
        // dedicated AEC_WALL_AXIS layer.
        for h in &handles {
            regenerate_wall_representation(&mut scene, *h)
                .expect("regeneration should succeed for every wall segment");
        }
        for h in &handles {
            let e = scene.document.get_entity(*h).unwrap();
            assert_eq!(e.common().layer, AEC_WALL_AXIS_LAYER);
        }

        // AEC_ROOM / collect_wall_segments must still see the closed loop.
        let segments = collect_wall_segments(&scene.document);
        assert_eq!(segments.len(), 4);

        let mut command_line = CommandLine::default();
        aec_room(&mut scene, &mut command_line);
        let room_record = scene
            .document
            .entities()
            .filter_map(read_aec_record)
            .find(|r| matches!(r.values.first(), Some(XDataValue::String(k)) if k == "ROOM"))
            .expect("aec_room should have written a ROOM xdata record");
        let area = match room_record.values.get(2) {
            Some(XDataValue::Real(a)) => *a,
            _ => panic!("ROOM record should carry an area value"),
        };
        assert!((area - 12.0).abs() < 1e-6);
    }

    #[test]
    fn wall_layer_extrusions_calculates_offsets_and_effective_height() {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        let entity = EntityType::LwPolyline(pl);
        let layers = vec![WallLayer {
            material: "brick".to_string(),
            thickness: 0.2,
            function: "Structural".to_string(),
            gap_before: 0.0,
            bottom_offset: 0.5,
            top_offset: 0.3,
            layer_override: None,
        }];
        let height = 3.0;
        let extrusions = wall_layer_extrusions(&entity, &layers, height);
        assert_eq!(extrusions.len(), 1);
        assert!((extrusions[0].height - 2.2).abs() < 1e-9); // 3.0 - 0.5 - 0.3 = 2.2
        assert!((extrusions[0].base_offset - 0.5).abs() < 1e-9);
    }

    #[test]
    fn regenerate_wall_representation_respects_vertical_offsets() {
        use acadrust::types::Vector2;
        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut layer = wl("Concrete", 0.2, "Structural");
        layer.bottom_offset = 0.5;
        layer.top_offset = 0.3;
        let layers = vec![layer];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_v2_record("style1", 3.0, 0, &layers, &[], WallJustification::Center);
        entity.common_mut().extended_data.add_record(record);
        let wall_handle = scene.add_entity(entity);

        regenerate_wall_representation(&mut scene, wall_handle)
            .expect("regeneration should succeed");

        let wall = wall_v2_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();

        let mut saw_bottom_z = false;
        let mut saw_top_z = false;
        for h in wall.derived_handles {
            let e = scene.document.get_entity(h).unwrap();
            if let EntityType::Solid3D(s3d) = e {
                for wire in &s3d.wires {
                    for pt in &wire.points {
                        if (pt.z - 0.5).abs() < 1e-9 {
                            saw_bottom_z = true;
                        }
                        if (pt.z - 2.7).abs() < 1e-9 {
                            saw_top_z = true;
                        }
                    }
                }
            }
        }
        assert!(saw_bottom_z, "solid should have wires at Z=0.5");
        assert!(saw_top_z, "solid should have wires at Z=2.7");
    }

    #[test]
    fn wall_regeneration_reflects_updated_layers() {
        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);

        let initial_layers = vec![wl("Brick", 0.1, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values =
            wall_v2_record("style1", 3.0, 0, &initial_layers, &[], WallJustification::Center);
        entity.common_mut().extended_data.add_record(record);
        let wall_handle = scene.add_entity(entity);

        // Initial regeneration
        regenerate_wall_representation(&mut scene, wall_handle).unwrap();

        // Update layers
        let updated_layers = vec![wl("Brick", 0.5, "Structural")];
        assert!(write_wall_v2_layers(&mut scene, wall_handle, updated_layers));

        // Regenerate again
        regenerate_wall_representation(&mut scene, wall_handle).unwrap();

        let wall = wall_v2_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();
        assert_eq!(wall.layers[0].thickness, 0.5);

        // Check geometry (thickness is reflected in Solid3D width)
        let mut max_y = 0.0;
        for h in wall.derived_handles {
            let e = scene.document.get_entity(h).unwrap();
            if let EntityType::Solid3D(s3d) = e {
                for wire in &s3d.wires {
                    for pt in &wire.points {
                        if pt.y.abs() > max_y {
                            max_y = pt.y.abs();
                        }
                    }
                }
            }
        }
        // For Center justification, half of 0.5 should be at Y=0.25 and Y=-0.25
        assert!((max_y - 0.25).abs() < 1e-6);
    }

    #[test]
    fn aec_wallextend_do_extends_to_an_explicit_point() {
        use crate::ui::command_line::CommandLine;

        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene); // axis: (0,0) -> (5,0)
        let mut command_line = CommandLine::default();

        aec_wallextend_do(
            &mut scene,
            &mut command_line,
            &format!("{}|PT|8|0|0", wall_handle.value()),
        );

        let axis = get_wall_vertices(&scene, wall_handle);
        assert_eq!(axis.len(), 2);
        // The nearer endpoint (5,0) should have moved to the target (8,0);
        // the far endpoint (0,0) stays put.
        assert!(axis.iter().any(|p| (p.x - 8.0).abs() < 1e-9 && p.y.abs() < 1e-9));
        assert!(axis.iter().any(|p| p.x.abs() < 1e-9 && p.y.abs() < 1e-9));

        // Both the 2D (contour/hatch) and 3D (solid) derived representation
        // must reflect the new, extended axis length — not just the axis
        // polyline itself.
        let wall = wall_v2_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();
        let mut max_x_2d: f64 = 0.0;
        let mut max_x_3d: f64 = 0.0;
        for h in &wall.derived_handles {
            match scene.document.get_entity(*h).unwrap() {
                EntityType::LwPolyline(pl) => {
                    for v in &pl.vertices {
                        max_x_2d = max_x_2d.max(v.location.x);
                    }
                }
                EntityType::Solid3D(s3d) => {
                    for wire in &s3d.wires {
                        for pt in &wire.points {
                            max_x_3d = max_x_3d.max(pt.x as f64);
                        }
                    }
                }
                _ => {}
            }
        }
        assert!(
            max_x_2d > 7.9,
            "2D contour should extend to the new endpoint, got max_x={max_x_2d}"
        );
        assert!(
            max_x_3d > 7.9,
            "3D solid should extend to the new endpoint, got max_x={max_x_3d}"
        );
    }

    #[test]
    fn aec_wallextend_do_preserves_direction_for_an_off_line_target() {
        use crate::ui::command_line::CommandLine;

        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene); // axis: (0,0) -> (5,0)
        let mut command_line = CommandLine::default();

        // Target point is NOT collinear with the wall's (0,0)->(5,0) axis
        // (it has a non-zero Y). Extending must not bend the wall towards
        // this point — it must project onto the original direction line.
        aec_wallextend_do(
            &mut scene,
            &mut command_line,
            &format!("{}|PT|8|2|0", wall_handle.value()),
        );

        let axis = get_wall_vertices(&scene, wall_handle);
        assert_eq!(axis.len(), 2);
        // Original direction was purely along +X (Y=0 for both points);
        // the new endpoint must keep the same direction, i.e. still Y=0.
        assert!(
            axis.iter().any(|p| (p.x - 8.0).abs() < 1e-9 && p.y.abs() < 1e-9),
            "extended endpoint should stay on the original direction line, got {:?}",
            axis
        );
        assert!(axis.iter().any(|p| p.x.abs() < 1e-9 && p.y.abs() < 1e-9));
    }

    #[test]
    fn aec_wallextend_do_extends_to_intersect_another_wall() {
        use crate::ui::command_line::CommandLine;

        let mut scene = Scene::new();
        // Wall A: (0,0) -> (5,0), stopping short of Wall B's axis.
        let wall_a = add_multi_layer_wall(&mut scene);
        // Wall B: a "through" wall running vertically at x=8, so extending A
        // towards it produces a T-junction trim on A only.
        let mut pl_b = LwPolyline::new();
        pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, -5.0)));
        pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, 5.0)));
        let mut entity_b = EntityType::LwPolyline(pl_b);
        let mut record_b = ExtendedDataRecord::new(AEC_APPID);
        record_b.values = wall_v2_record(
            "style1",
            3.0,
            0,
            &vec![wl("Concrete", 0.2, "Structural")],
            &[],
            WallJustification::Center,
        );
        entity_b.common_mut().extended_data.add_record(record_b);
        let wall_b = scene.add_entity(entity_b);

        let mut command_line = CommandLine::default();
        aec_wallextend_do(
            &mut scene,
            &mut command_line,
            &format!("{}|WALL|{}", wall_a.value(), wall_b.value()),
        );

        let axis_a = get_wall_vertices(&scene, wall_a);
        // Wall A should now end exactly at the intersection with wall B's axis.
        assert!(axis_a
            .iter()
            .any(|p| (p.x - 8.0).abs() < 1e-6 && p.y.abs() < 1e-6));
        // Wall B (the "through" wall) stays unchanged.
        let axis_b = get_wall_vertices(&scene, wall_b);
        assert_eq!(axis_b.len(), 2);
        assert!((axis_b[0].y - (-5.0)).abs() < 1e-9);
        assert!((axis_b[1].y - 5.0).abs() < 1e-9);
    }

    #[test]
    fn change_wall_justification_shifts_axis_by_expected_distance() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene); // total thickness 0.25
        let axis_before = get_wall_vertices(&scene, wall_handle);
        assert!(axis_before.iter().all(|p| p.y.abs() < 1e-9));

        assert!(change_wall_justification(
            &mut scene,
            wall_handle,
            WallJustification::Interior,
        ));

        let wall = wall_v2_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();
        assert_eq!(wall.justification, WallJustification::Interior);

        let axis_after = get_wall_vertices(&scene, wall_handle);
        // Center -> Interior delta is -0.5 * total_thickness = -0.125; the
        // offset direction for a straight horizontal axis is +Y, so the axis
        // should have shifted to Y = -0.125.
        for p in &axis_after {
            assert!((p.y - (-0.125)).abs() < 1e-6);
        }
    }
}
