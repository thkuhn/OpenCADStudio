//! Slab and SlabOpening entity XDATA serialization and deserialization.
//!
//! Encodes and decodes [`Slab`] and [`SlabOpening`] records under the `OPENCAD_AEC`
//! APPID, preserving multi-layer snapshots, justification, phase, control plane
//! references, derived child entity handles, and opening association handles.

use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, EntityType, Handle};
use uuid::Uuid;

use crate::scene::Scene;

use super::control_plane::ControlPlaneFacet;
use super::display_component::ComponentStyleOverride;
use super::plan_view::PlanPhase;
use super::slab::{Slab, SlabJustification, SlabLayer};
use super::slab_opening::{SlabOpening, SlabOpeningDepth, SlabOpeningKind};
use super::slab_package::resolve_slab_package;
use super::wall_style::LayerFunction;
use super::xdata::{encode_plane_ref, parse_plane_ref, read_aec_record, write_aec_record, AEC_APPID};

pub fn layer_function_to_str(f: &LayerFunction) -> String {
    match f {
        LayerFunction::Structural => "Structural".to_string(),
        LayerFunction::Insulation => "Insulation".to_string(),
        LayerFunction::Finish => "Finish".to_string(),
        LayerFunction::Other(s) => s.clone(),
    }
}

pub fn layer_function_from_str(s: &str) -> LayerFunction {
    match s {
        "Structural" | "structural" => LayerFunction::Structural,
        "Insulation" | "insulation" => LayerFunction::Insulation,
        "Finish" | "finish" => LayerFunction::Finish,
        other => LayerFunction::Other(other.to_string()),
    }
}

/// Builds the `XDataValue` list for a `SLAB` record.
pub fn slab_record_for_slab(slab: &Slab) -> Vec<XDataValue> {
    let mut values = Vec::new();
    values.push(XDataValue::String("SLAB".to_string()));
    values.push(XDataValue::String(slab.style_id.clone()));
    values.push(XDataValue::Integer32(slab.storey_id as i32));
    values.push(XDataValue::String(slab.justification.as_str().to_string()));
    values.push(XDataValue::String(slab.phase.as_str().to_string()));

    values.push(XDataValue::Integer32(slab.layers.len() as i32));
    for layer in &slab.layers {
        values.push(XDataValue::String(layer.material.clone()));
        values.push(XDataValue::Distance(layer.thickness));
        values.push(XDataValue::String(layer_function_to_str(&layer.function)));
    }

    values.push(XDataValue::Integer32(slab.derived_handles.len() as i32));
    for h in &slab.derived_handles {
        values.push(XDataValue::Handle(*h));
    }

    values.push(XDataValue::Integer32(slab.opening_handles.len() as i32));
    for h in &slab.opening_handles {
        values.push(XDataValue::Handle(*h));
    }

    // Trailing layer extras
    values.push(XDataValue::String("slab_layer_extras".to_string()));
    for layer in &slab.layers {
        values.push(XDataValue::Distance(layer.vertical_offset));
        values.push(XDataValue::String(
            layer.layer_override.clone().unwrap_or_default(),
        ));
        values.push(XDataValue::String(
            layer.hatch_override.clone().unwrap_or_default(),
        ));
        values.push(XDataValue::String(
            layer.role_tag.clone().unwrap_or_default(),
        ));
        values.push(XDataValue::String(layer.layer_id.to_string()));
    }

    // Trailing control planes
    encode_slab_planes(&mut values, slab);

    // Trailing hatch override
    values.push(XDataValue::String("hatch_override".to_string()));
    match &slab.hatch_override {
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

    values
}

fn encode_slab_planes(values: &mut Vec<XDataValue>, slab: &Slab) {
    values.push(XDataValue::String("planes".to_string()));
    values.push(XDataValue::String(encode_plane_ref(
        slab.base_plane_id,
        slab.base_plane_name.as_deref(),
    )));
    values.push(XDataValue::String(encode_plane_ref(
        slab.top_plane_id,
        slab.top_plane_name.as_deref(),
    )));
    values.push(XDataValue::Distance(slab.base_offset));
    values.push(XDataValue::Distance(slab.top_offset));

    for c in slab.base_origin {
        values.push(XDataValue::Distance(c));
    }
    for c in slab.base_normal {
        values.push(XDataValue::Distance(c));
    }
    for c in slab.top_origin {
        values.push(XDataValue::Distance(c));
    }
    for c in slab.top_normal {
        values.push(XDataValue::Distance(c));
    }

    let base_facets_json = if slab.base_facets.is_empty() {
        String::new()
    } else {
        serde_json::to_string(&slab.base_facets).unwrap_or_default()
    };
    let top_facets_json = if slab.top_facets.is_empty() {
        String::new()
    } else {
        serde_json::to_string(&slab.top_facets).unwrap_or_default()
    };
    values.push(XDataValue::String(base_facets_json));
    values.push(XDataValue::String(top_facets_json));
}

/// Parses a `SLAB` XDATA record from raw `XDataValue`s.
pub fn slab_from_values(v: &[XDataValue]) -> Option<Slab> {
    if v.len() < 6 {
        return None;
    }
    let XDataValue::String(kind) = &v[0] else {
        return None;
    };
    if kind != "SLAB" {
        return None;
    }

    let style_id = if let XDataValue::String(s) = &v[1] {
        s.clone()
    } else {
        return None;
    };
    let storey_id = if let XDataValue::Integer32(i) = v[2] {
        i as u32
    } else {
        return None;
    };
    let justification = if let XDataValue::String(s) = &v[3] {
        SlabJustification::from_str(s)
    } else {
        SlabJustification::Top
    };
    let phase = if let XDataValue::String(s) = &v[4] {
        PlanPhase::from_str(s)
    } else {
        PlanPhase::New
    };

    let layer_count = if let XDataValue::Integer32(i) = v[5] {
        i as usize
    } else {
        return None;
    };

    let mut cursor = 6;
    if v.len() < cursor + layer_count * 3 {
        return None;
    }

    let mut layers = Vec::with_capacity(layer_count);
    for _ in 0..layer_count {
        let mat = if let XDataValue::String(s) = &v[cursor] {
            s.clone()
        } else {
            return None;
        };
        let thick = if let XDataValue::Distance(d) = v[cursor + 1] {
            d
        } else {
            return None;
        };
        let func = if let XDataValue::String(s) = &v[cursor + 2] {
            layer_function_from_str(s)
        } else {
            LayerFunction::Structural
        };
        cursor += 3;

        layers.push(SlabLayer {
            material: mat,
            thickness: thick,
            function: func,
            vertical_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            role_tag: None,
            layer_id: Uuid::new_v4(),
        });
    }

    // Derived handles
    let mut derived_handles = Vec::new();
    if cursor < v.len() {
        if let XDataValue::Integer32(count) = v[cursor] {
            cursor += 1;
            let n = count as usize;
            for _ in 0..n {
                if cursor < v.len() {
                    if let XDataValue::Handle(h) = v[cursor] {
                        derived_handles.push(h);
                    }
                    cursor += 1;
                }
            }
        }
    }

    // Opening handles
    let mut opening_handles = Vec::new();
    if cursor < v.len() {
        if let XDataValue::Integer32(count) = v[cursor] {
            cursor += 1;
            let n = count as usize;
            for _ in 0..n {
                if cursor < v.len() {
                    if let XDataValue::Handle(h) = v[cursor] {
                        opening_handles.push(h);
                    }
                    cursor += 1;
                }
            }
        }
    }

    // Trailing layer extras
    if cursor < v.len() {
        if let XDataValue::String(tag) = &v[cursor] {
            if tag == "slab_layer_extras" {
                cursor += 1;
                for l in &mut layers {
                    if cursor < v.len() {
                        if let XDataValue::Distance(d) = v[cursor] {
                            l.vertical_offset = d;
                        }
                        cursor += 1;
                    }
                    if cursor < v.len() {
                        if let XDataValue::String(s) = &v[cursor] {
                            l.layer_override = if s.is_empty() { None } else { Some(s.clone()) };
                        }
                        cursor += 1;
                    }
                    if cursor < v.len() {
                        if let XDataValue::String(s) = &v[cursor] {
                            l.hatch_override = if s.is_empty() { None } else { Some(s.clone()) };
                        }
                        cursor += 1;
                    }
                    if cursor < v.len() {
                        if let XDataValue::String(s) = &v[cursor] {
                            l.role_tag = if s.is_empty() { None } else { Some(s.clone()) };
                        }
                        cursor += 1;
                    }
                    if cursor < v.len() {
                        if let XDataValue::String(s) = &v[cursor] {
                            if let Ok(id) = Uuid::parse_str(s) {
                                l.layer_id = id;
                            }
                        }
                        cursor += 1;
                    }
                }
            }
        }
    }

    // Planes trailer
    let (
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
        base_facets,
        top_facets,
    ) = parse_slab_planes_trailer(&v[cursor..]);

    // Hatch override trailer
    let hatch_override = parse_slab_hatch_override(&v[cursor..]);

    Some(Slab {
        style_id,
        storey_id,
        layers,
        derived_handles,
        opening_handles,
        justification,
        phase,
        hatch_override,
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
        base_facets,
        top_facets,
    })
}

fn parse_slab_planes_trailer(
    v: &[XDataValue],
) -> (
    Option<Uuid>,
    Option<Uuid>,
    Option<String>,
    Option<String>,
    f64,
    f64,
    [f64; 3],
    [f64; 3],
    [f64; 3],
    [f64; 3],
    Vec<ControlPlaneFacet>,
    Vec<ControlPlaneFacet>,
) {
    let Some(pos) = v.iter().position(|val| {
        matches!(val, XDataValue::String(s) if s == "planes")
    }) else {
        return (
            None, None, None, None, 0.0, 0.0,
            [0.0, 0.0, 0.0], [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0], [0.0, 0.0, 1.0],
            Vec::new(), Vec::new(),
        );
    };

    let p = &v[pos + 1..];
    let (base_id, base_name) = match p.first() {
        Some(XDataValue::String(s)) => parse_plane_ref(s),
        _ => (None, None),
    };
    let (top_id, top_name) = match p.get(1) {
        Some(XDataValue::String(s)) => parse_plane_ref(s),
        _ => (None, None),
    };
    let bo = match p.get(2) {
        Some(XDataValue::Distance(d)) => *d,
        _ => 0.0,
    };
    let to = match p.get(3) {
        Some(XDataValue::Distance(d)) => *d,
        _ => 0.0,
    };

    let read_3 = |start: usize, fallback: [f64; 3]| -> [f64; 3] {
        if p.len() >= start + 3 {
            let mut arr = [0.0; 3];
            for i in 0..3 {
                if let Some(XDataValue::Distance(d)) = p.get(start + i) {
                    arr[i] = *d;
                } else {
                    return fallback;
                }
            }
            arr
        } else {
            fallback
        }
    };

    let base_orig = read_3(4, [0.0, 0.0, 0.0]);
    let base_norm = read_3(7, [0.0, 0.0, 1.0]);
    let top_orig = read_3(10, [0.0, 0.0, 0.0]);
    let top_norm = read_3(13, [0.0, 0.0, 1.0]);

    let base_facets = match p.get(16) {
        Some(XDataValue::String(s)) if !s.is_empty() => {
            serde_json::from_str::<Vec<ControlPlaneFacet>>(s).unwrap_or_default()
        }
        _ => Vec::new(),
    };
    let top_facets = match p.get(17) {
        Some(XDataValue::String(s)) if !s.is_empty() => {
            serde_json::from_str::<Vec<ControlPlaneFacet>>(s).unwrap_or_default()
        }
        _ => Vec::new(),
    };

    (
        base_id, top_id, base_name, top_name, bo, to,
        base_orig, base_norm, top_orig, top_norm,
        base_facets, top_facets,
    )
}

fn parse_slab_hatch_override(v: &[XDataValue]) -> Option<ComponentStyleOverride> {
    let pos = v.iter().position(|val| {
        matches!(val, XDataValue::String(s) if s == "hatch_override")
    })?;
    let tail = &v[pos + 1..];
    if tail.len() < 3 {
        return None;
    }
    let flag = match &tail[0] {
        XDataValue::String(s) => s.as_str(),
        _ => "0",
    };
    if flag != "1" {
        return None;
    }
    let angle = match &tail[1] {
        XDataValue::Distance(d) if !d.is_nan() => Some(*d),
        _ => None,
    };
    let rel = match &tail[2] {
        XDataValue::String(s) if s == "true" => Some(true),
        XDataValue::String(s) if s == "false" => Some(false),
        _ => None,
    };
    Some(ComponentStyleOverride {
        hatch_angle: angle,
        hatch_angle_relative: rel,
        ..Default::default()
    })
}

/// Reads a [`Slab`] from an entity's AEC XDATA record.
pub fn slab_from_entity(entity: &EntityType) -> Option<Slab> {
    let record = read_aec_record(entity)?;
    slab_from_values(&record.values)
}

/// Writes a [`Slab`] record onto `handle` in `doc`.
pub fn write_slab_record(doc: &mut CadDocument, handle: Handle, slab: &Slab) -> bool {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = slab_record_for_slab(slab);
    write_aec_record(doc, handle, record)
}

/// Overwrites only `style_id` and the layer snapshot of a slab's `SLAB`
/// XDATA record, keeping planes, offsets, openings, derived handles, phase,
/// and justification intact.
pub fn write_slab_style(
    scene: &mut Scene,
    slab_handle: Handle,
    style_id: &str,
    layers: Vec<SlabLayer>,
) -> bool {
    let slab_handle = resolve_slab_package(scene, slab_handle);
    let Some(entity) = scene.document.get_entity(slab_handle) else {
        return false;
    };
    let Some(mut slab) = slab_from_entity(entity) else {
        return false;
    };
    slab.style_id = style_id.to_string();
    slab.layers = layers;
    write_slab_record(&mut scene.document, slab_handle, &slab)
}

/// Overwrites only the layer snapshot of a slab's `SLAB` XDATA record.
pub fn write_slab_layers(scene: &mut Scene, slab_handle: Handle, layers: Vec<SlabLayer>) -> bool {
    let slab_handle = resolve_slab_package(scene, slab_handle);
    let Some(entity) = scene.document.get_entity(slab_handle) else {
        return false;
    };
    let Some(mut slab) = slab_from_entity(entity) else {
        return false;
    };
    slab.layers = layers;
    write_slab_record(&mut scene.document, slab_handle, &slab)
}

/// Overwrites only the `justification` field of a slab's `SLAB` XDATA record.
pub fn write_slab_justification(
    scene: &mut Scene,
    slab_handle: Handle,
    justification: SlabJustification,
) -> bool {
    let slab_handle = resolve_slab_package(scene, slab_handle);
    let Some(entity) = scene.document.get_entity(slab_handle) else {
        return false;
    };
    let Some(mut slab) = slab_from_entity(entity) else {
        return false;
    };
    slab.justification = justification;
    write_slab_record(&mut scene.document, slab_handle, &slab)
}

/// Overwrites only the `phase` field of a slab's `SLAB` XDATA record.
pub fn write_slab_phase(scene: &mut Scene, slab_handle: Handle, phase: PlanPhase) -> bool {
    let slab_handle = resolve_slab_package(scene, slab_handle);
    let Some(entity) = scene.document.get_entity(slab_handle) else {
        return false;
    };
    let Some(mut slab) = slab_from_entity(entity) else {
        return false;
    };
    slab.phase = phase;
    write_slab_record(&mut scene.document, slab_handle, &slab)
}

/// Overwrites base and/or top plane offsets, keeping all other fields intact.
pub fn write_slab_plane_offsets(
    scene: &mut Scene,
    slab_handle: Handle,
    base_offset: Option<f64>,
    top_offset: Option<f64>,
) -> bool {
    let slab_handle = resolve_slab_package(scene, slab_handle);
    let Some(entity) = scene.document.get_entity(slab_handle) else {
        return false;
    };
    let Some(mut slab) = slab_from_entity(entity) else {
        return false;
    };
    if let Some(b) = base_offset {
        slab.base_offset = b;
    }
    if let Some(t) = top_offset {
        slab.top_offset = t;
    }
    write_slab_record(&mut scene.document, slab_handle, &slab)
}

/// Writes a fully updated [`Slab`] while resolving package ownership first.
pub fn write_slab_model(scene: &mut Scene, slab_handle: Handle, slab: &Slab) -> bool {
    let slab_handle = resolve_slab_package(scene, slab_handle);
    write_slab_record(&mut scene.document, slab_handle, slab)
}

// ── Slab Openings ────────────────────────────────────────────────────────────

/// Builds the `ExtendedDataRecord` for a [`SlabOpening`].
pub fn slab_opening_record(opening: &SlabOpening) -> ExtendedDataRecord {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("SLAB_OPENING".to_string()));
    record.add_value(XDataValue::Handle(opening.host_slab));
    record.add_value(XDataValue::String(opening.kind.as_str().to_string()));
    record.add_value(XDataValue::String(opening.depth.as_str().to_string()));
    record.add_value(XDataValue::Distance(
        opening.depth.depth_value().unwrap_or(-1.0),
    ));

    record.add_value(XDataValue::Integer32(opening.boundary.len() as i32));
    for p in &opening.boundary {
        record.add_value(XDataValue::Distance(p.0));
        record.add_value(XDataValue::Distance(p.1));
    }

    record.add_value(XDataValue::Integer32(opening.derived_handles.len() as i32));
    for h in &opening.derived_handles {
        record.add_value(XDataValue::Handle(*h));
    }

    record
}

/// Parses a [`SlabOpening`] from raw `XDataValue`s.
pub fn slab_opening_from_values(v: &[XDataValue]) -> Option<SlabOpening> {
    if v.len() < 6 {
        return None;
    }
    let XDataValue::String(tag) = &v[0] else {
        return None;
    };
    if tag != "SLAB_OPENING" {
        return None;
    }

    let host_slab = if let XDataValue::Handle(h) = v[1] {
        h
    } else {
        return None;
    };

    let kind = if let XDataValue::String(s) = &v[2] {
        SlabOpeningKind::from_str(s)
    } else {
        SlabOpeningKind::Stairwell
    };

    let depth_mode = if let XDataValue::String(s) = &v[3] {
        s.as_str()
    } else {
        "ThroughHole"
    };

    let depth_val = if let XDataValue::Distance(d) = v[4] {
        d
    } else {
        -1.0
    };
    let depth = SlabOpeningDepth::from_mode_and_depth(depth_mode, depth_val);

    let point_count = if let XDataValue::Integer32(count) = v[5] {
        count as usize
    } else {
        return None;
    };

    let mut cursor = 6;
    if v.len() < cursor + point_count * 2 {
        return None;
    }

    let mut boundary = Vec::with_capacity(point_count);
    for _ in 0..point_count {
        let x = if let XDataValue::Distance(d) = v[cursor] {
            d
        } else {
            return None;
        };
        let y = if let XDataValue::Distance(d) = v[cursor + 1] {
            d
        } else {
            return None;
        };
        cursor += 2;
        boundary.push((x, y));
    }

    let mut derived_handles = Vec::new();
    if cursor < v.len() {
        if let XDataValue::Integer32(count) = v[cursor] {
            cursor += 1;
            let n = count as usize;
            for _ in 0..n {
                if cursor < v.len() {
                    if let XDataValue::Handle(h) = v[cursor] {
                        derived_handles.push(h);
                    }
                    cursor += 1;
                }
            }
        }
    }

    Some(SlabOpening {
        host_slab,
        kind,
        depth,
        boundary,
        derived_handles,
    })
}

/// Reads a [`SlabOpening`] from an entity's AEC XDATA record.
pub fn slab_opening_from_entity(entity: &EntityType) -> Option<SlabOpening> {
    let record = read_aec_record(entity)?;
    slab_opening_from_values(&record.values)
}

/// Writes a [`SlabOpening`] record onto `handle` in `doc`.
pub fn write_slab_opening_record(
    doc: &mut CadDocument,
    handle: Handle,
    opening: &SlabOpening,
) -> bool {
    let record = slab_opening_record(opening);
    write_aec_record(doc, handle, record)
}

/// Writes a [`SlabOpening`] onto `handle` in a scene document.
pub fn write_slab_opening_model(scene: &mut Scene, handle: Handle, opening: &SlabOpening) -> bool {
    write_slab_opening_record(&mut scene.document, handle, opening)
}

/// Overwrites only the opening kind, keeping host/depth/boundary/handles intact.
pub fn write_slab_opening_kind(
    scene: &mut Scene,
    opening_handle: Handle,
    kind: SlabOpeningKind,
) -> bool {
    let Some(entity) = scene.document.get_entity(opening_handle) else {
        return false;
    };
    let Some(mut opening) = slab_opening_from_entity(entity) else {
        return false;
    };
    opening.kind = kind;
    write_slab_opening_record(&mut scene.document, opening_handle, &opening)
}

/// Overwrites only the opening depth mode/value.
pub fn write_slab_opening_depth(
    scene: &mut Scene,
    opening_handle: Handle,
    depth: SlabOpeningDepth,
) -> bool {
    let Some(entity) = scene.document.get_entity(opening_handle) else {
        return false;
    };
    let Some(mut opening) = slab_opening_from_entity(entity) else {
        return false;
    };
    opening.depth = depth;
    write_slab_opening_record(&mut scene.document, opening_handle, &opening)
}

/// Appends `opening_handle` to the host slab's XDATA opening handles list.
pub fn add_slab_opening_handle(
    doc: &mut CadDocument,
    slab_handle: Handle,
    opening_handle: Handle,
) -> bool {
    let Some(entity) = doc.get_entity(slab_handle) else {
        return false;
    };
    let Some(mut slab) = slab_from_entity(entity) else {
        return false;
    };
    if !slab.opening_handles.contains(&opening_handle) {
        slab.opening_handles.push(opening_handle);
    }
    write_slab_record(doc, slab_handle, &slab)
}

/// Removes `opening_handle` from the host slab's XDATA opening handles list.
pub fn remove_slab_opening_handle(
    doc: &mut CadDocument,
    slab_handle: Handle,
    opening_handle: Handle,
) -> bool {
    let Some(entity) = doc.get_entity(slab_handle) else {
        return false;
    };
    let Some(mut slab) = slab_from_entity(entity) else {
        return false;
    };
    slab.opening_handles.retain(|&h| h != opening_handle);
    write_slab_record(doc, slab_handle, &slab)
}

/// Updates the `derived_handles` of a [`Slab`].
pub fn set_slab_derived_handles(
    doc: &mut CadDocument,
    slab_handle: Handle,
    derived: &[Handle],
) -> bool {
    let Some(entity) = doc.get_entity(slab_handle) else {
        return false;
    };
    let Some(mut slab) = slab_from_entity(entity) else {
        return false;
    };
    slab.derived_handles = derived.to_vec();
    write_slab_record(doc, slab_handle, &slab)
}

/// Updates the `derived_handles` of a [`SlabOpening`].
pub fn set_slab_opening_derived_handles(
    doc: &mut CadDocument,
    opening_handle: Handle,
    derived: &[Handle],
) -> bool {
    let Some(entity) = doc.get_entity(opening_handle) else {
        return false;
    };
    let Some(mut opening) = slab_opening_from_entity(entity) else {
        return false;
    };
    opening.derived_handles = derived.to_vec();
    write_slab_opening_record(doc, opening_handle, &opening)
}

/// Resolve a slab style's effective layers into concrete [`SlabLayer`]s.
pub fn resolve_slab_style_layers(
    lib: &super::library::StyleLibrary,
    style_id: &str,
    thickness: Option<f64>,
) -> Option<Vec<SlabLayer>> {
    resolve_slab_style_layers_ex(lib, style_id, thickness, true)
}

/// Same as [`resolve_slab_style_layers`] but keeps material ids instead of display names.
pub fn resolve_slab_style_layers_ids(
    lib: &super::library::StyleLibrary,
    style_id: &str,
    thickness: Option<f64>,
) -> Option<Vec<SlabLayer>> {
    resolve_slab_style_layers_ex(lib, style_id, thickness, false)
}

pub(crate) fn resolve_slab_style_layers_ex(
    lib: &super::library::StyleLibrary,
    style_id: &str,
    thickness: Option<f64>,
    use_material_names: bool,
) -> Option<Vec<SlabLayer>> {
    use std::collections::HashMap;
    use super::slab_style::{base_thickness_from_layers, effective_layers, effective_layers_for_slab_vars, slab_vars, SlabStyle};

    let style_map: HashMap<String, SlabStyle> = lib
        .slab_styles
        .iter()
        .map(|s| (s.style.id.clone(), s.clone()))
        .collect();

    let unresolved = effective_layers(&style_map, style_id).ok()?;
    let nom_thick = thickness.unwrap_or_else(|| base_thickness_from_layers(&unresolved));
    let vars = slab_vars(nom_thick);
    let resolved = effective_layers_for_slab_vars(&style_map, style_id, &vars).ok()?;

    Some(
        resolved
            .into_iter()
            .map(|layer| {
                let mat_name = if use_material_names {
                    lib.materials
                        .iter()
                        .find(|m| m.id == layer.material_id)
                        .map(|m| m.name.clone())
                        .unwrap_or_else(|| layer.material_id.clone())
                } else {
                    layer.material_id.clone()
                };
                layer.to_slab_layer(&mat_name)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slab_round_trip() {
        let mut slab = Slab::new("style_slab_concrete_20", 2);
        slab.justification = SlabJustification::StructuralTop;
        slab.phase = PlanPhase::Existing;
        slab.layers = vec![
            SlabLayer::new("Tiles", 0.02, LayerFunction::Finish),
            SlabLayer::new("Concrete", 0.20, LayerFunction::Structural),
        ];
        slab.derived_handles = vec![Handle::from(101u64), Handle::from(102u64)];
        slab.opening_handles = vec![Handle::from(201u64)];
        slab.base_offset = -0.05;
        slab.top_offset = 0.10;
        slab.top_plane_id = Some(Uuid::new_v4());
        slab.top_plane_name = Some("Roof Plane".to_string());

        let values = slab_record_for_slab(&slab);
        let back = slab_from_values(&values).expect("parse slab");

        assert_eq!(back.style_id, slab.style_id);
        assert_eq!(back.storey_id, slab.storey_id);
        assert_eq!(back.justification, slab.justification);
        assert_eq!(back.phase, slab.phase);
        assert_eq!(back.layers.len(), slab.layers.len());
        assert_eq!(back.layers[0].material, "Tiles");
        assert_eq!(back.layers[1].material, "Concrete");
        assert_eq!(back.derived_handles, slab.derived_handles);
        assert_eq!(back.opening_handles, slab.opening_handles);
        assert_eq!(back.top_plane_id, slab.top_plane_id);
        assert_eq!(back.top_plane_name, slab.top_plane_name);
        assert!((back.base_offset - slab.base_offset).abs() < 1e-6);
        assert!((back.top_offset - slab.top_offset).abs() < 1e-6);
    }

    #[test]
    fn test_slab_opening_round_trip() {
        let opening = SlabOpening::new_recess(
            Handle::from(42u64),
            SlabOpeningKind::Shaft,
            0.15,
            vec![(1.0, 1.0), (3.0, 1.0), (3.0, 2.0), (1.0, 2.0)],
        );

        let rec = slab_opening_record(&opening);
        let back = slab_opening_from_values(&rec.values).expect("parse slab opening");

        assert_eq!(back.host_slab, opening.host_slab);
        assert_eq!(back.kind, SlabOpeningKind::Shaft);
        assert_eq!(back.depth, SlabOpeningDepth::Recess(0.15));
        assert_eq!(back.boundary, opening.boundary);
        assert_eq!(back.derived_handles, opening.derived_handles);
    }

    #[test]
    fn write_slab_style_preserves_planes_offsets_and_openings() {
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2;
        use crate::scene::Scene;

        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(4.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(4.0, 3.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 3.0)));
        pl.is_closed = true;
        let handle = scene.add_entity(EntityType::LwPolyline(pl));

        let plane_id = Uuid::new_v4();
        let opening_h = Handle::from(99u64);
        let derived_h = Handle::from(77u64);
        let mut slab = Slab::new("old_style", 2);
        slab.layers = vec![SlabLayer::new("Concrete", 0.20, LayerFunction::Structural)];
        slab.justification = SlabJustification::StructuralTop;
        slab.phase = PlanPhase::Existing;
        slab.base_offset = 0.05;
        slab.top_offset = 0.10;
        slab.top_plane_id = Some(plane_id);
        slab.top_plane_name = Some("Roof".into());
        slab.opening_handles = vec![opening_h];
        slab.derived_handles = vec![derived_h];
        assert!(write_slab_record(&mut scene.document, handle, &slab));

        let new_layers = vec![
            SlabLayer::new("Tiles", 0.02, LayerFunction::Finish),
            SlabLayer::new("Concrete", 0.24, LayerFunction::Structural),
        ];
        assert!(write_slab_style(
            &mut scene,
            handle,
            "new_style",
            new_layers.clone()
        ));

        let entity = scene.document.get_entity(handle).expect("entity");
        let back = slab_from_entity(entity).expect("slab");
        assert_eq!(back.style_id, "new_style");
        assert_eq!(back.layers, new_layers);
        assert_eq!(back.justification, SlabJustification::StructuralTop);
        assert_eq!(back.phase, PlanPhase::Existing);
        assert!((back.base_offset - 0.05).abs() < 1e-9);
        assert!((back.top_offset - 0.10).abs() < 1e-9);
        assert_eq!(back.top_plane_id, Some(plane_id));
        assert_eq!(back.top_plane_name.as_deref(), Some("Roof"));
        assert_eq!(back.opening_handles, vec![opening_h]);
        assert_eq!(back.derived_handles, vec![derived_h]);
        assert_eq!(back.storey_id, 2);
    }

    #[test]
    fn write_slab_justification_and_phase_preserve_style_and_planes() {
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2;
        use crate::scene::Scene;

        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(2.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(2.0, 2.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 2.0)));
        pl.is_closed = true;
        let handle = scene.add_entity(EntityType::LwPolyline(pl));

        let plane_id = Uuid::new_v4();
        let mut slab = Slab::new("style_a", 1);
        slab.layers = vec![SlabLayer::new("Concrete", 0.20, LayerFunction::Structural)];
        slab.justification = SlabJustification::Top;
        slab.phase = PlanPhase::New;
        slab.base_plane_id = Some(plane_id);
        slab.base_plane_name = Some("FFL".into());
        assert!(write_slab_record(&mut scene.document, handle, &slab));

        assert!(write_slab_justification(
            &mut scene,
            handle,
            SlabJustification::Bottom
        ));
        assert!(write_slab_phase(&mut scene, handle, PlanPhase::Demolition));

        let entity = scene.document.get_entity(handle).expect("entity");
        let back = slab_from_entity(entity).expect("slab");
        assert_eq!(back.style_id, "style_a");
        assert_eq!(back.justification, SlabJustification::Bottom);
        assert_eq!(back.phase, PlanPhase::Demolition);
        assert_eq!(back.base_plane_id, Some(plane_id));
        assert_eq!(back.base_plane_name.as_deref(), Some("FFL"));
        assert_eq!(back.layers.len(), 1);
    }
}
