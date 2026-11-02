//! Associative View Carrier in Model Space.
//!
//! Encapsulates derived 2D views (sections, elevations, floor plans) placed as
//! native CAD entities in the model space, attached with `OPENCAD_AEC` XDATA
//! provenance and child handle tracking.

use uuid::Uuid;

use acadrust::entities::hatch::{BoundaryEdge, BoundaryPath, PolylineEdge};
use acadrust::entities::{Hatch, HatchPattern, Line, LwPolyline, LwVertex, MText, Point};
use acadrust::types::{Vector2, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, EntityType, Handle};

use super::model_view::{ModelView, UpdatePolicy, ViewRevisions};
use super::provenance::{
    DerivedPrimitive, ElementRef, FeatureKey, Geometry2d, PrimitiveRole,
};
use super::viewpoint::{ViewKind, Viewpoint};
use super::{ElementId, ModelId, ModelViewId, Point2, Revision, ValidationError, ViewpointId};
use crate::modules::aec::engine::owner_index;
use crate::modules::aec::engine::xdata::{ensure_app_id, AEC_APPID};

pub const VIEW_CARRIER_TAG: &str = "VIEW_CARRIER";
pub const VIEW_CARRIER_VP_TAG: &str = "VIEW_CARRIER_VP";
pub const VIEW_CARRIER_MV_TAG: &str = "VIEW_CARRIER_MV";
pub const DERIVED_PRIMITIVE_TAG: &str = "DERIVED_PRIMITIVE";
pub const VIEW_CARRIER_BADGE_TAG: &str = "VIEW_CARRIER_BADGE";
const BADGE_HALF: f64 = 0.40;

/// An associative carrier entity in model space hosting derived 2D drawing primitives.
#[derive(Clone, Debug, PartialEq)]
pub struct DerivedViewCarrier {
    pub handle: Handle,
    pub viewpoint_id: ViewpointId,
    pub model_view_id: ModelViewId,
    pub insertion_point: Point2,
    pub scale: f64,
    pub rotation_rad: f64,
    pub is_outdated: bool,
    pub update_policy: UpdatePolicy,
    pub last_updated_revisions: ViewRevisions,
    pub derived_handles: Vec<Handle>,
    pub marker_handle: Option<Handle>,
}

impl DerivedViewCarrier {
    pub fn new(
        handle: Handle,
        viewpoint_id: ViewpointId,
        model_view_id: ModelViewId,
        insertion_point: Point2,
    ) -> Self {
        Self {
            handle,
            viewpoint_id,
            model_view_id,
            insertion_point,
            scale: 1.0,
            rotation_rad: 0.0,
            is_outdated: false,
            update_policy: UpdatePolicy::Automatic,
            last_updated_revisions: ViewRevisions::default(),
            derived_handles: Vec::new(),
            marker_handle: None,
        }
    }
}

/// Reads a `DerivedViewCarrier` from an entity's `OPENCAD_AEC` XDATA record.
pub fn read_view_carrier(doc: &CadDocument, entity: &EntityType) -> Option<DerivedViewCarrier> {
    let mut carrier_rec: Option<&ExtendedDataRecord> = None;
    for rec in entity.common().extended_data.records() {
        if rec.application_name == AEC_APPID
            && matches!(rec.values.first(), Some(XDataValue::String(s)) if s == VIEW_CARRIER_TAG)
        {
            carrier_rec = Some(rec);
            break;
        }
    }
    let rec = carrier_rec?;
    let values = &rec.values;

    let viewpoint_uuid_str = match values.get(1) {
        Some(XDataValue::String(s)) => s,
        _ => return None,
    };
    let model_view_uuid_str = match values.get(2) {
        Some(XDataValue::String(s)) => s,
        _ => return None,
    };

    let viewpoint_id = ViewpointId::from_uuid(Uuid::parse_str(viewpoint_uuid_str).ok()?);
    let model_view_id = ModelViewId::from_uuid(Uuid::parse_str(model_view_uuid_str).ok()?);

    let ins_x = match values.get(3) {
        Some(XDataValue::Real(v)) => *v,
        _ => 0.0,
    };
    let ins_y = match values.get(4) {
        Some(XDataValue::Real(v)) => *v,
        _ => 0.0,
    };
    let scale = match values.get(5) {
        Some(XDataValue::Real(v)) => *v,
        _ => 1.0,
    };
    let rotation_rad = match values.get(6) {
        Some(XDataValue::Real(v)) => *v,
        _ => 0.0,
    };
    let is_outdated = match values.get(7) {
        Some(XDataValue::Integer32(v)) => *v != 0,
        Some(XDataValue::Integer16(v)) => *v != 0,
        _ => false,
    };
    let update_policy = match values.get(8) {
        Some(XDataValue::Integer32(1)) | Some(XDataValue::Integer16(1)) => UpdatePolicy::Manual,
        _ => UpdatePolicy::Automatic,
    };

    let geom_rev = match values.get(9) {
        Some(XDataValue::Integer32(v)) if *v >= 0 => Revision(*v as u64),
        _ => Revision::default(),
    };
    let style_rev = match values.get(10) {
        Some(XDataValue::Integer32(v)) if *v >= 0 => Revision(*v as u64),
        _ => Revision::default(),
    };
    let pres_rev = match values.get(11) {
        Some(XDataValue::Integer32(v)) if *v >= 0 => Revision(*v as u64),
        _ => Revision::default(),
    };

    let handle = entity.common().handle;
    let derived_handles = owner_index::children_of(doc, handle);
    let marker_handle = owner_index::peers_of(doc, handle)
        .first()
        .copied()
        .or_else(|| match values.get(12) {
            Some(XDataValue::Handle(h)) => Some(*h),
            _ => None,
        });

    Some(DerivedViewCarrier {
        handle,
        viewpoint_id,
        model_view_id,
        insertion_point: Point2::new(ins_x, ins_y),
        scale,
        rotation_rad,
        is_outdated,
        update_policy,
        last_updated_revisions: ViewRevisions {
            geometry: geom_rev,
            style: style_rev,
            presentation: pres_rev,
        },
        derived_handles,
        marker_handle,
    })
}

/// Writes `DerivedViewCarrier` XDATA to `carrier.handle` in the document.
pub fn write_view_carrier(doc: &mut CadDocument, carrier: &DerivedViewCarrier) -> bool {
    ensure_app_id(doc);
    let app_handle = doc.app_ids.get(AEC_APPID).map(|a| a.handle.value());

    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String(VIEW_CARRIER_TAG.to_string()));
    record.add_value(XDataValue::String(carrier.viewpoint_id.as_uuid().to_string()));
    record.add_value(XDataValue::String(carrier.model_view_id.as_uuid().to_string()));
    record.add_value(XDataValue::Real(carrier.insertion_point.x_m));
    record.add_value(XDataValue::Real(carrier.insertion_point.y_m));
    record.add_value(XDataValue::Real(carrier.scale));
    record.add_value(XDataValue::Real(carrier.rotation_rad));
    record.add_value(XDataValue::Integer32(if carrier.is_outdated { 1 } else { 0 }));
    record.add_value(XDataValue::Integer32(match carrier.update_policy {
        UpdatePolicy::Automatic => 0,
        UpdatePolicy::Manual => 1,
    }));
    record.add_value(XDataValue::Integer32(carrier.last_updated_revisions.geometry.0 as i32));
    record.add_value(XDataValue::Integer32(carrier.last_updated_revisions.style.0 as i32));
    record.add_value(XDataValue::Integer32(carrier.last_updated_revisions.presentation.0 as i32));
    if let Some(mh) = carrier.marker_handle {
        record.add_value(XDataValue::Handle(mh));
    }

    let Some(entity) = doc.get_entity_mut(carrier.handle) else {
        return false;
    };

    let xd = &mut entity.common_mut().extended_data;
    let kept: Vec<_> = xd
        .records()
        .iter()
        .filter(|r| {
            if r.application_name != AEC_APPID {
                return true;
            }
            !matches!(r.values.first(), Some(XDataValue::String(s)) if s == VIEW_CARRIER_TAG)
        })
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

    owner_index::set_children(doc, carrier.handle, &carrier.derived_handles);
    if let Some(mh) = carrier.marker_handle {
        owner_index::link_peers(doc, carrier.handle, mh);
    }
    sync_carrier_badge(doc, carrier);
    true
}

/// The carrier that owns a badge label, if `entity` is that label.
pub fn badge_carrier_handle(entity: &EntityType) -> Option<Handle> {
    for rec in entity.common().extended_data.records() {
        if rec.application_name == AEC_APPID
            && matches!(rec.values.first(), Some(XDataValue::String(s)) if s == VIEW_CARRIER_BADGE_TAG)
        {
            if let Some(XDataValue::String(raw)) = rec.values.get(1) {
                return raw.parse::<u64>().ok().map(Handle::new);
            }
        }
    }
    None
}

/// Resolves a badge label to its carrier. Other handles are returned unchanged.
pub fn resolve_carrier_handle(doc: &CadDocument, handle: Handle) -> Handle {
    doc.get_entity(handle)
        .and_then(badge_carrier_handle)
        .unwrap_or(handle)
}

fn sync_carrier_badge(doc: &mut CadDocument, carrier: &DerivedViewCarrier) {
    install_anchor(doc, carrier);
    sync_badge_label(doc, carrier);
}

fn install_anchor(doc: &mut CadDocument, carrier: &DerivedViewCarrier) {
    let Some(old) = doc.get_entity(carrier.handle) else {
        return;
    };
    let mut common = old.common().clone();
    common.layer = "AEC_VIEW_CARRIER".into();
    common.color = acadrust::types::Color::Index(4);
    common.line_weight = acadrust::types::LineWeight::from_value(50);

    let mut pl = LwPolyline::new();
    pl.is_closed = true;
    let (cos_r, sin_r) = (carrier.rotation_rad.cos(), carrier.rotation_rad.sin());
    let ox = carrier.insertion_point.x_m;
    let oy = carrier.insertion_point.y_m;
    for (x, y) in [
        (0.0, BADGE_HALF),
        (BADGE_HALF, 0.0),
        (0.0, -BADGE_HALF),
        (-BADGE_HALF, 0.0),
    ] {
        let rx = ox + x * cos_r - y * sin_r;
        let ry = oy + x * sin_r + y * cos_r;
        pl.add_vertex(LwVertex::new(Vector2::new(rx, ry)));
    }
    let mut entity = EntityType::LwPolyline(pl);
    *entity.common_mut() = common;
    if let Some(slot) = doc.get_entity_mut(carrier.handle) {
        *slot = entity;
    }
}

fn badge_caption(doc: &CadDocument, carrier: &DerivedViewCarrier) -> String {
    let Some((vp, mv)) = read_view_carrier_definitions(doc, carrier.handle) else {
        return "AEC".to_string();
    };
    let kind = match vp.kind {
        ViewKind::Section { .. } => "Schnitt",
        ViewKind::Elevation { .. } => "Ansicht",
        ViewKind::FloorPlan { .. } => "Grundriss",
        ViewKind::ReflectedCeilingPlan { .. } => "Deckenplan",
    };
    let scale = mv.scale.get();
    let denom = if (scale - scale.round()).abs() < 1e-6 {
        format!("{}", scale.round() as i64)
    } else {
        format!("{scale}")
    };
    let name = vp.name.trim();
    if name.is_empty() {
        format!("{kind}  1:{denom}")
    } else {
        format!("{kind} {name}  1:{denom}")
    }
}

fn badge_text_point(carrier: &DerivedViewCarrier) -> (f64, f64) {
    let (cos_r, sin_r) = (carrier.rotation_rad.cos(), carrier.rotation_rad.sin());
    let lx = BADGE_HALF + 0.15;
    let ly = 0.08;
    (
        carrier.insertion_point.x_m + lx * cos_r - ly * sin_r,
        carrier.insertion_point.y_m + lx * sin_r + ly * cos_r,
    )
}

fn find_badge_handle(doc: &CadDocument, carrier_handle: Handle) -> Option<Handle> {
    let wanted = carrier_handle.value().to_string();
    for entity in doc.entities() {
        for rec in entity.common().extended_data.records() {
            if rec.application_name == AEC_APPID
                && matches!(rec.values.first(), Some(XDataValue::String(s)) if s == VIEW_CARRIER_BADGE_TAG)
                && matches!(rec.values.get(1), Some(XDataValue::String(s)) if s == &wanted)
            {
                return Some(entity.common().handle);
            }
        }
    }
    None
}

fn write_badge_tag(entity: &mut EntityType, carrier_handle: Handle) {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String(VIEW_CARRIER_BADGE_TAG.to_string()));
    record.add_value(XDataValue::String(carrier_handle.value().to_string()));
    let xd = &mut entity.common_mut().extended_data;
    let kept: Vec<_> = xd
        .records()
        .iter()
        .filter(|r| {
            !(r.application_name == AEC_APPID
                && matches!(r.values.first(), Some(XDataValue::String(s)) if s == VIEW_CARRIER_BADGE_TAG))
        })
        .cloned()
        .collect();
    xd.clear();
    for kept_record in kept {
        xd.add_record(kept_record);
    }
    xd.add_record(record);
}

fn sync_badge_label(doc: &mut CadDocument, carrier: &DerivedViewCarrier) {
    ensure_app_id(doc);
    let caption = badge_caption(doc, carrier);
    let (tx, ty) = badge_text_point(carrier);
    if let Some(handle) = find_badge_handle(doc, carrier.handle) {
        if let Some(EntityType::MText(text)) = doc.get_entity_mut(handle) {
            text.value = caption;
            text.insertion_point = Vector3::new(tx, ty, 0.0);
            text.rotation = carrier.rotation_rad;
            text.height = 0.22;
            return;
        }
    }

    let mut text = MText::new();
    text.value = caption;
    text.insertion_point = Vector3::new(tx, ty, 0.0);
    text.height = 0.22;
    text.rotation = carrier.rotation_rad;
    text.attachment_point = acadrust::entities::mtext::AttachmentPoint::MiddleLeft;
    let mut entity = EntityType::MText(Box::new(text));
    entity.common_mut().layer = "AEC_VIEW_CARRIER".into();
    entity.common_mut().color = acadrust::types::Color::Index(4);
    write_badge_tag(&mut entity, carrier.handle);
    let _ = doc.add_entity(entity);
}

/// Creates a new `DerivedViewCarrier` in `doc` anchored at `insertion_point`.
pub fn create_view_carrier(
    doc: &mut CadDocument,
    viewpoint_id: ViewpointId,
    model_view_id: ModelViewId,
    insertion_point: Point2,
    scale: f64,
    rotation_rad: f64,
    update_policy: UpdatePolicy,
) -> Result<DerivedViewCarrier, ValidationError> {
    ensure_app_id(doc);

    // Anchor point entity in model space
    let pt = Point::from_coords(insertion_point.x_m, insertion_point.y_m, 0.0);
    let mut entity = EntityType::Point(pt);
    let handle = doc.allocate_handle();
    entity.common_mut().handle = handle;
    entity.common_mut().layer = "AEC_VIEW_CARRIER".into();

    let _ = doc.add_entity(entity);

    let carrier = DerivedViewCarrier {
        handle,
        viewpoint_id,
        model_view_id,
        insertion_point,
        scale,
        rotation_rad,
        is_outdated: false,
        update_policy,
        last_updated_revisions: ViewRevisions::default(),
        derived_handles: Vec::new(),
        marker_handle: None,
    };

    if !write_view_carrier(doc, &carrier) {
        return Err(ValidationError("failed to write view carrier XDATA".into()));
    }

    Ok(carrier)
}

/// Collects all `DerivedViewCarrier`s found in the document.
pub fn all_view_carriers(doc: &CadDocument) -> Vec<DerivedViewCarrier> {
    let mut carriers = Vec::new();
    for entity in doc.entities() {
        if let Some(carrier) = read_view_carrier(doc, entity) {
            carriers.push(carrier);
        }
    }
    carriers
}

/// Finds a `DerivedViewCarrier` by handle.
pub fn find_view_carrier_by_handle(doc: &CadDocument, handle: Handle) -> Option<DerivedViewCarrier> {
    let entity = doc.get_entity(handle)?;
    read_view_carrier(doc, entity)
}

/// Finds all `DerivedViewCarrier`s referencing a specific `ModelViewId`.
pub fn find_view_carriers_for_model_view(
    doc: &CadDocument,
    model_view_id: ModelViewId,
) -> Vec<DerivedViewCarrier> {
    all_view_carriers(doc)
        .into_iter()
        .filter(|c| c.model_view_id == model_view_id)
        .collect()
}

/// Finds the first `DerivedViewCarrier` referencing a specific `ViewpointId`.
pub fn find_view_carrier_by_viewpoint(
    doc: &CadDocument,
    viewpoint_id: ViewpointId,
) -> Option<DerivedViewCarrier> {
    all_view_carriers(doc)
        .into_iter()
        .find(|c| c.viewpoint_id == viewpoint_id)
}

/// Finds the `DerivedViewCarrier` that owns a derived CAD entity.
pub fn find_view_carrier_owning_handle(doc: &CadDocument, child: Handle) -> Option<DerivedViewCarrier> {
    if let Some(entity) = doc.get_entity(child) {
        if let Some(meta) = read_derived_primitive_metadata(entity) {
            if let Some(carrier_h) = meta.carrier_handle {
                if let Some(c) = find_view_carrier_by_handle(doc, carrier_h) {
                    return Some(c);
                }
            }
        }
    }
    // Check owner index and carrier handle lists
    for ent in doc.entities() {
        if let Some(carrier) = read_view_carrier(doc, ent) {
            if carrier.derived_handles.contains(&child) {
                return Some(carrier);
            }
        }
    }
    None
}

/// Stores the full `Viewpoint` and `ModelView` definitions on the carrier entity.
pub fn write_view_carrier_definitions(
    doc: &mut CadDocument,
    carrier_handle: Handle,
    viewpoint: &Viewpoint,
    model_view: &ModelView,
) -> bool {
    ensure_app_id(doc);
    let app_handle = doc.app_ids.get(AEC_APPID).map(|a| a.handle.value());
    let Ok(vp_json) = serde_json::to_string(viewpoint) else { return false; };
    let Ok(mv_json) = serde_json::to_string(model_view) else { return false; };

    let mut vp_rec = ExtendedDataRecord::new(AEC_APPID);
    vp_rec.add_value(XDataValue::String(VIEW_CARRIER_VP_TAG.to_string()));
    vp_rec.add_value(XDataValue::String(vp_json));

    let mut mv_rec = ExtendedDataRecord::new(AEC_APPID);
    mv_rec.add_value(XDataValue::String(VIEW_CARRIER_MV_TAG.to_string()));
    mv_rec.add_value(XDataValue::String(mv_json));

    {
        let Some(entity) = doc.get_entity_mut(carrier_handle) else {
            return false;
        };

        let xd = &mut entity.common_mut().extended_data;
        let kept: Vec<_> = xd
            .records()
            .iter()
            .filter(|r| {
                if r.application_name != AEC_APPID {
                    return true;
                }
                !matches!(r.values.first(), Some(XDataValue::String(s)) if s == VIEW_CARRIER_VP_TAG || s == VIEW_CARRIER_MV_TAG)
            })
            .cloned()
            .collect();
        xd.clear();
        for r in kept {
            xd.add_record(r);
        }
        xd.add_record(vp_rec);
        xd.add_record(mv_rec);

        if let Some(ah) = app_handle {
            xd.raw_dwg_eed.retain(|(a, _)| *a != ah);
        }
    }
    if let Some(carrier) = doc
        .get_entity(carrier_handle)
        .and_then(|entity| read_view_carrier(doc, entity))
    {
        sync_carrier_badge(doc, &carrier);
    }
    true
}

/// Reads the embedded `Viewpoint` and `ModelView` definitions from the carrier entity.
pub fn read_view_carrier_definitions(
    doc: &CadDocument,
    carrier_handle: Handle,
) -> Option<(Viewpoint, ModelView)> {
    let entity = doc.get_entity(carrier_handle)?;
    let mut vp_opt = None;
    let mut mv_opt = None;

    for r in entity.common().extended_data.records() {
        if r.application_name != AEC_APPID {
            continue;
        }
        if matches!(r.values.first(), Some(XDataValue::String(s)) if s == VIEW_CARRIER_VP_TAG) {
            if let Some(XDataValue::String(json_str)) = r.values.get(1) {
                if let Ok(vp) = serde_json::from_str::<Viewpoint>(json_str) {
                    vp_opt = Some(vp);
                }
            }
        }
        if matches!(r.values.first(), Some(XDataValue::String(s)) if s == VIEW_CARRIER_MV_TAG) {
            if let Some(XDataValue::String(json_str)) = r.values.get(1) {
                if let Ok(mv) = serde_json::from_str::<ModelView>(json_str) {
                    mv_opt = Some(mv);
                }
            }
        }
    }

    match (vp_opt, mv_opt) {
        (Some(vp), Some(mv)) => Some((vp, mv)),
        _ => None,
    }
}

/// Synchronizes a carrier's derived primitives with native CAD entities in `doc`.
/// Erases old child entities without modifying unrelated user geometry or annotations.
pub fn sync_carrier_primitives(
    doc: &mut CadDocument,
    carrier: &mut DerivedViewCarrier,
    primitives: &[DerivedPrimitive],
) -> Result<(), ValidationError> {
    ensure_app_id(doc);
    ensure_document_linetypes(doc);

    // 1. Erase old child handles
    for child in &carrier.derived_handles {
        let _ = doc.remove_entity(*child);
    }
    carrier.derived_handles.clear();

    let cos_r = carrier.rotation_rad.cos();
    let sin_r = carrier.rotation_rad.sin();
    let ins_x = carrier.insertion_point.x_m;
    let ins_y = carrier.insertion_point.y_m;
    let s = carrier.scale;

    let scale_denom = read_view_carrier_definitions(doc, carrier.handle)
        .map(|(_, mv)| mv.scale.get())
        .unwrap_or(100.0);
    let lt_scale = ((scale_denom / 1000.0) * carrier.scale).max(0.01);

    let transform_pt = |p: Point2| -> Point2 {
        let sx = p.x_m * s;
        let sy = p.y_m * s;
        let rx = cos_r * sx - sin_r * sy;
        let ry = sin_r * sx + cos_r * sy;
        Point2::new(ins_x + rx, ins_y + ry)
    };

    let mut new_handles = Vec::new();

    // 2. Generate native CAD entities for each primitive
    for prim in primitives {
        let layer_name = prim
            .style
            .layer_name
            .as_deref()
            .unwrap_or_else(|| match prim.role {
                PrimitiveRole::CutBoundary => "AEC_SECTION_CUT",
                PrimitiveRole::CutFill => "AEC_SECTION_HATCH",
                PrimitiveRole::ProjectedEdge => "AEC_SECTION_ELEV",
                PrimitiveRole::HiddenEdge => "AEC_SECTION_HIDDEN",
                PrimitiveRole::SurfaceFill => "AEC_SECTION_SURFACE",
                PrimitiveRole::Symbol => "AEC_SECTION_SYMBOLS",
            });

        let is_hidden_or_overhead = prim.role == PrimitiveRole::HiddenEdge
            || layer_name == "AEC_OVERHEAD"
            || layer_name.contains("HIDDEN");
        ensure_carrier_layer(doc, layer_name, is_hidden_or_overhead);

        match &prim.geometry {
            Geometry2d::Line { start, end } => {
                let p0 = transform_pt(*start);
                let p1 = transform_pt(*end);
                let line = Line::from_points(
                    Vector3::new(p0.x_m, p0.y_m, 0.0),
                    Vector3::new(p1.x_m, p1.y_m, 0.0),
                );
                let h = doc.allocate_handle();
                let mut ent = EntityType::Line(line);
                ent.common_mut().handle = h;
                apply_primitive_style_to_entity(&mut ent, prim, carrier.handle, layer_name, lt_scale);
                attach_primitive_xdata(doc, &mut ent, prim, carrier.handle);
                let _ = doc.add_entity(ent);
                new_handles.push(h);
            }
            Geometry2d::Polyline { vertices, closed } => {
                let mut pl = LwPolyline::new();
                for v in vertices {
                    let tv = transform_pt(*v);
                    pl.add_vertex(LwVertex::new(Vector2::new(tv.x_m, tv.y_m)));
                }
                pl.is_closed = *closed;
                let h = doc.allocate_handle();
                let mut ent = EntityType::LwPolyline(pl);
                ent.common_mut().handle = h;
                apply_primitive_style_to_entity(&mut ent, prim, carrier.handle, layer_name, lt_scale);
                attach_primitive_xdata(doc, &mut ent, prim, carrier.handle);
                let _ = doc.add_entity(ent);
                new_handles.push(h);
            }
            Geometry2d::Region { outer, holes } => {
                let is_cut_fill = prim.role == PrimitiveRole::CutFill;
                let is_surface_fill_with_pattern = prim.role == PrimitiveRole::SurfaceFill
                    && (prim.style.hatch_pattern.is_some() || prim.style.fill_color.is_some());

                if (is_cut_fill || is_surface_fill_with_pattern) && outer.len() >= 3 {
                    let mut hatch = Hatch::new();
                    let pat_name = prim
                        .style
                        .hatch_pattern
                        .clone()
                        .unwrap_or_else(|| "ANSI31".to_string());
                    if pat_name.is_empty() || pat_name.eq_ignore_ascii_case("SOLID") {
                        hatch.is_solid = true;
                    } else {
                        hatch.is_solid = false;
                        hatch.pattern = HatchPattern::new(&pat_name);
                        hatch.pattern_scale = prim.style.hatch_scale.unwrap_or(1.0).max(1e-4);
                        hatch.pattern_angle = prim
                            .style
                            .hatch_angle_rad
                            .unwrap_or(0.0)
                            + carrier.rotation_rad;
                    }

                    let outer_pts: Vec<Vector2> = outer
                        .iter()
                        .map(|v| {
                            let tv = transform_pt(*v);
                            Vector2::new(tv.x_m, tv.y_m)
                        })
                        .collect();
                    let mut outer_path = BoundaryPath::external();
                    outer_path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(outer_pts, true)));
                    hatch.paths.push(outer_path);

                    for hole in holes {
                        let hole_pts: Vec<Vector2> = hole
                            .iter()
                            .map(|v| {
                                let tv = transform_pt(*v);
                                Vector2::new(tv.x_m, tv.y_m)
                            })
                            .collect();
                        if hole_pts.len() >= 3 {
                            let mut hole_path = BoundaryPath::new();
                            hole_path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(hole_pts, true)));
                            hatch.paths.push(hole_path);
                        }
                    }

                    let hh = doc.allocate_handle();
                    let mut hent = EntityType::Hatch(Box::new(hatch));
                    hent.common_mut().handle = hh;
                    apply_primitive_style_to_entity(&mut hent, prim, carrier.handle, layer_name, lt_scale);
                    attach_primitive_xdata(doc, &mut hent, prim, carrier.handle);
                    let _ = doc.add_entity(hent);
                    new_handles.push(hh);
                } else if prim.role != PrimitiveRole::SurfaceFill {
                    // Outer boundary polyline
                    let mut pl = LwPolyline::new();
                    for v in outer {
                        let tv = transform_pt(*v);
                        pl.add_vertex(LwVertex::new(Vector2::new(tv.x_m, tv.y_m)));
                    }
                    pl.is_closed = true;
                    let h = doc.allocate_handle();
                    let mut ent = EntityType::LwPolyline(pl);
                    ent.common_mut().handle = h;
                    apply_primitive_style_to_entity(&mut ent, prim, carrier.handle, layer_name, lt_scale);
                    attach_primitive_xdata(doc, &mut ent, prim, carrier.handle);
                    let _ = doc.add_entity(ent);
                    new_handles.push(h);

                    // Hole boundaries
                    for hole in holes {
                        let mut hole_pl = LwPolyline::new();
                        for v in hole {
                            let tv = transform_pt(*v);
                            hole_pl.add_vertex(LwVertex::new(Vector2::new(tv.x_m, tv.y_m)));
                        }
                        hole_pl.is_closed = true;
                        let hh = doc.allocate_handle();
                        let mut hent = EntityType::LwPolyline(hole_pl);
                        hent.common_mut().handle = hh;
                        apply_primitive_style_to_entity(&mut hent, prim, carrier.handle, layer_name, lt_scale);
                        attach_primitive_xdata(doc, &mut hent, prim, carrier.handle);
                        let _ = doc.add_entity(hent);
                        new_handles.push(hh);
                    }
                }
            }
            Geometry2d::CircularArc { center, radius, start_rad, sweep_rad } => {
                // Approximate arc as polyline segments
                let steps = 16;
                let mut pl = LwPolyline::new();
                for i in 0..=steps {
                    let t = *start_rad + *sweep_rad * (i as f64 / steps as f64);
                    let p = Point2::new(
                        center.x_m + radius.get() * t.cos(),
                        center.y_m + radius.get() * t.sin(),
                    );
                    let tp = transform_pt(p);
                    pl.add_vertex(LwVertex::new(Vector2::new(tp.x_m, tp.y_m)));
                }
                let h = doc.allocate_handle();
                let mut ent = EntityType::LwPolyline(pl);
                ent.common_mut().handle = h;
                apply_primitive_style_to_entity(&mut ent, prim, carrier.handle, layer_name, lt_scale);
                attach_primitive_xdata(doc, &mut ent, prim, carrier.handle);
                let _ = doc.add_entity(ent);
                new_handles.push(h);
            }
        }
    }

    carrier.derived_handles = new_handles;
    carrier.is_outdated = false;
    write_view_carrier(doc, carrier);
    owner_index::set_children(doc, carrier.handle, &carrier.derived_handles);

    Ok(())
}

/// Ensures that standard linetypes including `"DASHED"` are populated in the CAD document.
pub fn ensure_document_linetypes(doc: &mut CadDocument) {
    if !doc.line_types.iter().any(|lt| lt.name.eq_ignore_ascii_case("DASHED")) {
        let mut lt = acadrust::tables::LineType::new("DASHED");
        lt.handle = doc.allocate_handle();
        lt.description = "Dashed __ __ __ __ __ __ __ __ __ __ __ __ __ _".to_string();
        lt.add_element(acadrust::tables::LineTypeElement::dash(12.7));
        lt.add_element(acadrust::tables::LineTypeElement::dash(-6.35));
        let _ = doc.line_types.add(lt);
    }
}

/// Ensures a carrier layer exists in the document with standard defaults.
pub fn ensure_carrier_layer(doc: &mut CadDocument, layer_name: &str, is_hidden_or_overhead: bool) {
    if !doc.layers.contains(layer_name) {
        let mut layer = acadrust::tables::Layer::new(layer_name);
        layer.handle = doc.allocate_handle();
        if is_hidden_or_overhead {
            layer.line_type = "DASHED".to_string();
            layer.color = acadrust::types::Color::Index(8); // Gray
        } else {
            layer.line_type = "Continuous".to_string();
        }
        let _ = doc.layers.add(layer);
    }
}

/// Applies primitive style attributes (carrier reactor, linetype, linetype_scale, lineweight, color) to a CAD entity.
fn apply_primitive_style_to_entity(
    ent: &mut EntityType,
    prim: &DerivedPrimitive,
    carrier_handle: Handle,
    layer_name: &str,
    linetype_scale: f64,
) {
    let is_hatch = matches!(ent, EntityType::Hatch(_));
    let common = ent.common_mut();
    if !common.reactors.contains(&carrier_handle) {
        common.reactors.push(carrier_handle);
    }
    common.layer = layer_name.to_string();

    // Linetype
    if let Some(lt) = &prim.style.line_type {
        common.linetype = lt.clone();
    } else if prim.role == PrimitiveRole::HiddenEdge
        || layer_name == "AEC_OVERHEAD"
        || layer_name.contains("HIDDEN")
    {
        common.linetype = "DASHED".to_string();
    }

    if !common.linetype.is_empty() && !common.linetype.eq_ignore_ascii_case("Continuous") {
        common.linetype_scale = linetype_scale;
    }

    // Lineweight (convert mm to hundredths of mm as i16)
    let lw_val = (prim.style.line_weight_mm.get() * 100.0).round() as i16;
    common.line_weight = acadrust::types::LineWeight::from_value(lw_val);

    // Color
    if is_hatch {
        if let Some(c) = prim.style.fill_color.or(prim.style.line_color) {
            common.color = acadrust::types::Color::Rgb {
                r: c.r,
                g: c.g,
                b: c.b,
            };
        }
    } else if let Some(c) = prim.style.line_color {
        common.color = acadrust::types::Color::Rgb {
            r: c.r,
            g: c.g,
            b: c.b,
        };
    }
}

/// Typed metadata parsed from an entity's `DERIVED_PRIMITIVE` XDATA.
#[derive(Clone, Debug, PartialEq)]
pub struct DerivedPrimitiveMetadata {
    pub view_id: ModelViewId,
    pub index: u32,
    pub role: PrimitiveRole,
    pub element_ref: Option<ElementRef>,
    pub source_handle: Option<Handle>,
    pub feature: Option<FeatureKey>,
    pub xref_name: Option<String>,
    pub carrier_handle: Option<Handle>,
}

/// Reads typed `DerivedPrimitiveMetadata` from an entity's `DERIVED_PRIMITIVE` XDATA record.
pub fn read_derived_primitive_metadata(entity: &EntityType) -> Option<DerivedPrimitiveMetadata> {
    let rec = entity.common().extended_data.records().iter().find(|r| {
        r.application_name == AEC_APPID
            && matches!(r.values.first(), Some(XDataValue::String(s)) if s == DERIVED_PRIMITIVE_TAG)
    })?;

    let values = &rec.values;
    let view_id_str = match values.get(1) {
        Some(XDataValue::String(s)) => s,
        _ => return None,
    };
    let uuid = Uuid::parse_str(view_id_str).ok()?;
    let view_id = ModelViewId::from_uuid(uuid);

    let index = match values.get(2) {
        Some(XDataValue::Integer32(idx)) => *idx as u32,
        _ => 0,
    };

    let role = match values.get(3) {
        Some(XDataValue::String(s)) => match s.as_str() {
            "CutBoundary" => PrimitiveRole::CutBoundary,
            "CutFill" => PrimitiveRole::CutFill,
            "ProjectedEdge" => PrimitiveRole::ProjectedEdge,
            "HiddenEdge" => PrimitiveRole::HiddenEdge,
            "SurfaceFill" => PrimitiveRole::SurfaceFill,
            "Symbol" => PrimitiveRole::Symbol,
            _ => PrimitiveRole::ProjectedEdge,
        },
        _ => PrimitiveRole::ProjectedEdge,
    };

    let mut element_ref = None;
    if let (Some(XDataValue::String(m_str)), Some(XDataValue::String(e_str))) = (values.get(4), values.get(5)) {
        if let (Ok(m_uuid), Ok(e_uuid)) = (Uuid::parse_str(m_str), Uuid::parse_str(e_str)) {
            element_ref = Some(ElementRef::new(ModelId::from_uuid(m_uuid), ElementId::from_uuid(e_uuid)));
        }
    }

    let feature = values.get(6).and_then(|v| match v {
        XDataValue::String(s) => match s.as_str() {
            "WallTop" => Some(FeatureKey::WallTop),
            "WallBase" => Some(FeatureKey::WallBase),
            "OpeningSill" => Some(FeatureKey::OpeningSill),
            "OpeningHead" => Some(FeatureKey::OpeningHead),
            "OpeningLeftJamb" => Some(FeatureKey::OpeningLeftJamb),
            "OpeningRightJamb" => Some(FeatureKey::OpeningRightJamb),
            "LayerFace" => Some(FeatureKey::LayerFace),
            "SlabTop" => Some(FeatureKey::SlabTop),
            "SlabBottom" => Some(FeatureKey::SlabBottom),
            _ => None,
        },
        _ => None,
    });

    let mut xref_name = None;
    let mut carrier_handle = None;
    let mut source_handle = None;

    let mut handles_seen = Vec::new();
    for val in values.iter().skip(7) {
        match val {
            XDataValue::Handle(h) => handles_seen.push(*h),
            XDataValue::String(s) if !s.is_empty() => {
                if xref_name.is_none() {
                    xref_name = Some(s.clone());
                }
            }
            _ => {}
        }
    }

    if handles_seen.len() >= 2 {
        source_handle = Some(handles_seen[0]);
        carrier_handle = Some(handles_seen[1]);
    } else if handles_seen.len() == 1 {
        carrier_handle = Some(handles_seen[0]);
    }

    if source_handle.is_none() {
        if let Some(ref el) = element_ref {
            source_handle = super::scene_collector::element_id_to_handle(el.element_id);
        }
    }

    Some(DerivedPrimitiveMetadata {
        view_id,
        index,
        role,
        element_ref,
        source_handle,
        feature,
        xref_name,
        carrier_handle,
    })
}

fn attach_primitive_xdata(_doc: &CadDocument, entity: &mut EntityType, prim: &DerivedPrimitive, carrier_handle: Handle) {
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.add_value(XDataValue::String(DERIVED_PRIMITIVE_TAG.to_string()));
    rec.add_value(XDataValue::String(prim.id.view_id.as_uuid().to_string()));
    rec.add_value(XDataValue::Integer32(prim.id.index as i32));

    let role_str = match prim.role {
        PrimitiveRole::CutBoundary => "CutBoundary",
        PrimitiveRole::CutFill => "CutFill",
        PrimitiveRole::ProjectedEdge => "ProjectedEdge",
        PrimitiveRole::HiddenEdge => "HiddenEdge",
        PrimitiveRole::SurfaceFill => "SurfaceFill",
        PrimitiveRole::Symbol => "Symbol",
    };
    rec.add_value(XDataValue::String(role_str.to_string()));

    let mut source_handle = None;
    if let Some(src) = prim.sources.as_slice().first() {
        rec.add_value(XDataValue::String(src.element.model_id.as_uuid().to_string()));
        rec.add_value(XDataValue::String(src.element.element_id.as_uuid().to_string()));
        source_handle = super::scene_collector::element_id_to_handle(src.element.element_id);
        if let Some(feat) = &src.feature {
            let feat_str = match feat {
                FeatureKey::WallTop => "WallTop",
                FeatureKey::WallBase => "WallBase",
                FeatureKey::OpeningSill => "OpeningSill",
                FeatureKey::OpeningHead => "OpeningHead",
                FeatureKey::OpeningLeftJamb => "OpeningLeftJamb",
                FeatureKey::OpeningRightJamb => "OpeningRightJamb",
                FeatureKey::LayerFace => "LayerFace",
                FeatureKey::SlabTop => "SlabTop",
                FeatureKey::SlabBottom => "SlabBottom",
                _ => "Other",
            };
            rec.add_value(XDataValue::String(feat_str.to_string()));
        } else {
            rec.add_value(XDataValue::String("None".to_string()));
        }
        if let Some(xref) = &src.xref_source {
            rec.add_value(XDataValue::String(xref.block_record_name.clone()));
        }
    }

    if let Some(sh) = source_handle {
        rec.add_value(XDataValue::Handle(sh));
    }
    rec.add_value(XDataValue::Handle(carrier_handle));
    entity.common_mut().extended_data.add_record(rec);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::views::provenance::{
        DepthInterval, ElementRef, ResolvedStyle, SourceReference, SourceSet,
    };
    use crate::modules::aec::engine::views::{ElementId, ModelId, PaperMm};

    #[test]
    fn test_create_and_read_carrier() {
        let mut doc = CadDocument::new();
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let carrier = create_view_carrier(
            &mut doc,
            vp_id,
            mv_id,
            Point2::new(10.0, 20.0),
            1.0,
            0.0,
            UpdatePolicy::Automatic,
        )
        .expect("create carrier");

        let read = find_view_carrier_by_handle(&doc, carrier.handle).expect("read carrier");
        assert_eq!(read.viewpoint_id, vp_id);
        assert_eq!(read.model_view_id, mv_id);
        assert_eq!(read.insertion_point, Point2::new(10.0, 20.0));
        assert_eq!(read.update_policy, UpdatePolicy::Automatic);
        assert!(!read.is_outdated);
    }

    #[test]
    fn test_sync_carrier_primitives() {
        let mut doc = CadDocument::new();
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let mut carrier = create_view_carrier(
            &mut doc,
            vp_id,
            mv_id,
            Point2::new(0.0, 0.0),
            1.0,
            0.0,
            UpdatePolicy::Automatic,
        )
        .expect("create carrier");

        let elem_ref = ElementRef::new(ModelId::new(), ElementId::new());
        let src = SourceReference {
            element: elem_ref,
            xref_source: None,
            component_id: None,
            feature: Some(FeatureKey::WallTop),
            display_component_id: None,
        };

        let prim = DerivedPrimitive {
            id: super::super::provenance::PrimitiveId::new(mv_id, 1, 1),
            role: PrimitiveRole::CutBoundary,
            geometry: Geometry2d::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(5.0, 0.0),
            },
            sources: SourceSet::new(vec![src]).unwrap(),
            depth: DepthInterval::new(0.0, 0.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.35).unwrap()),
            edit_handles: vec![],
        };

        sync_carrier_primitives(&mut doc, &mut carrier, &[prim]).expect("sync primitives");
        assert_eq!(carrier.derived_handles.len(), 1);

        let child_handle = carrier.derived_handles[0];
        let child_ent = doc.get_entity(child_handle).expect("child exists");
        assert!(matches!(child_ent, EntityType::Line(_)));

        // Read carrier again
        let read = find_view_carrier_by_handle(&doc, carrier.handle).expect("carrier exists");
        assert_eq!(read.derived_handles, vec![child_handle]);

        let meta = read_derived_primitive_metadata(child_ent).expect("metadata present");
        assert_eq!(meta.feature, Some(FeatureKey::WallTop));
        assert_eq!(meta.carrier_handle, Some(carrier.handle));

        let owning_carrier = find_view_carrier_owning_handle(&doc, child_handle).expect("carrier found");
        assert_eq!(owning_carrier.handle, carrier.handle);
    }

    #[test]
    fn test_sync_carrier_hidden_edge_attributes() {
        let mut doc = CadDocument::new();
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let mut carrier = create_view_carrier(
            &mut doc,
            vp_id,
            mv_id,
            Point2::new(0.0, 0.0),
            1.0,
            0.0,
            UpdatePolicy::Automatic,
        )
        .expect("create carrier");

        let elem_ref = ElementRef::new(ModelId::new(), ElementId::new());
        let src = SourceReference {
            element: elem_ref,
            xref_source: None,
            component_id: None,
            feature: Some(FeatureKey::OpeningHead),
            display_component_id: None,
        };

        // Test fallback with line_type: None on a HiddenEdge
        let hidden_prim = DerivedPrimitive {
            id: super::super::provenance::PrimitiveId::new(mv_id, 1, 1),
            role: PrimitiveRole::HiddenEdge,
            geometry: Geometry2d::Line {
                start: Point2::new(1.0, 2.0),
                end: Point2::new(3.0, 2.0),
            },
            sources: SourceSet::new(vec![src]).unwrap(),
            depth: DepthInterval::new(0.0, 1.0).unwrap(),
            style: ResolvedStyle {
                line_weight_mm: PaperMm::new(0.25).unwrap(),
                line_color: Some(crate::modules::aec::engine::views::provenance::Rgba8::new(200, 100, 50, 255)),
                line_type: None, // Test fallback to DASHED!
                fill_color: None,
                hatch_pattern: None,
                hatch_scale: None,
                hatch_angle_rad: None,
                layer_name: Some("AEC_OVERHEAD".to_string()),
            },
            edit_handles: vec![],
        };

        sync_carrier_primitives(&mut doc, &mut carrier, &[hidden_prim]).expect("sync primitives");
        assert_eq!(carrier.derived_handles.len(), 1);

        let child_handle = carrier.derived_handles[0];
        let child_ent = doc.get_entity(child_handle).expect("child exists");

        // Verify valid CAD ownership and viewport visibility
        assert_eq!(
            child_ent.common().owner_handle,
            doc.header.model_space_block_handle,
            "Child entity must be owned by model space block record, NOT point carrier"
        );
        assert!(
            child_ent.common().reactors.contains(&carrier.handle),
            "Child entity must reference carrier in reactors"
        );
        assert_eq!(
            crate::modules::aec::engine::owner_index::children_of(&doc, carrier.handle),
            vec![child_handle],
            "Carrier owner_index must list the derived child"
        );

        let mut scene = crate::scene::Scene::new();
        scene.document = doc.clone().into();
        assert!(
            scene.entity_belongs_to_current_layout(child_handle),
            "Derived entity must belong to current layout / visible block"
        );

        // Verify linetype fallback and attributes
        assert_eq!(child_ent.common().linetype, "DASHED");
        assert!((child_ent.common().linetype_scale - 0.1).abs() < 1e-4);
        assert_eq!(child_ent.common().layer, "AEC_OVERHEAD");
        assert_eq!(child_ent.common().line_weight, acadrust::types::LineWeight::from_value(25));
        assert_eq!(
            child_ent.common().color,
            acadrust::types::Color::Rgb { r: 200, g: 100, b: 50 }
        );

        // Verify document linetypes table contains DASHED
        assert!(doc.line_types.iter().any(|lt| lt.name.eq_ignore_ascii_case("DASHED")));
        // Verify document layers table contains AEC_OVERHEAD
        assert!(doc.layers.contains("AEC_OVERHEAD"));
    }

    #[test]
    fn test_carrier_metadata_source_handle_and_marker_peer_link() {
        let mut doc = CadDocument::new();
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let mut carrier = create_view_carrier(
            &mut doc,
            vp_id,
            mv_id,
            Point2::new(0.0, 0.0),
            1.0,
            0.0,
            UpdatePolicy::Automatic,
        )
        .expect("create carrier");

        // Simulate an original CAD wall entity with handle 42
        let wall_handle = Handle::new(42);
        let elem_id = super::super::scene_collector::handle_to_element_id(wall_handle);
        let elem_ref = ElementRef::new(ModelId::new(), elem_id);
        let src = SourceReference {
            element: elem_ref,
            xref_source: None,
            component_id: None,
            feature: Some(FeatureKey::WallBase),
            display_component_id: None,
        };

        let prim = DerivedPrimitive {
            id: super::super::provenance::PrimitiveId::new(mv_id, 1, 1),
            role: PrimitiveRole::CutBoundary,
            geometry: Geometry2d::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(5.0, 0.0),
            },
            sources: SourceSet::new(vec![src]).unwrap(),
            depth: DepthInterval::new(0.0, 0.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.35).unwrap()),
            edit_handles: vec![],
        };

        sync_carrier_primitives(&mut doc, &mut carrier, &[prim]).expect("sync primitives");
        assert_eq!(carrier.derived_handles.len(), 1);

        let child_handle = carrier.derived_handles[0];
        let child_ent = doc.get_entity(child_handle).expect("child exists");

        let meta = read_derived_primitive_metadata(child_ent).expect("metadata present");
        assert_eq!(meta.source_handle, Some(wall_handle));
        assert_eq!(meta.carrier_handle, Some(carrier.handle));
        assert_eq!(meta.feature, Some(FeatureKey::WallBase));

        // Test marker handle peer linkage with an actual entity in doc
        let mut marker_ent = EntityType::Point(Point::from_coords(0.0, 0.0, 0.0));
        let marker_handle = doc.allocate_handle();
        marker_ent.common_mut().handle = marker_handle;
        let _ = doc.add_entity(marker_ent);

        carrier.marker_handle = Some(marker_handle);
        assert!(write_view_carrier(&mut doc, &carrier));

        let read_carrier = find_view_carrier_by_handle(&doc, carrier.handle).expect("carrier exists");
        assert_eq!(read_carrier.marker_handle, Some(marker_handle));
        assert_eq!(
            crate::modules::aec::engine::owner_index::peers_of(&doc, carrier.handle),
            vec![marker_handle]
        );
        assert_eq!(
            crate::modules::aec::engine::owner_index::peers_of(&doc, marker_handle),
            vec![carrier.handle]
        );
    }

    #[test]
    fn test_carrier_hatch_scale_angle_and_bylayer_color() {
        let mut doc = CadDocument::new();
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let mut carrier = create_view_carrier(
            &mut doc,
            vp_id,
            mv_id,
            Point2::new(10.0, 20.0),
            2.0, // scale factor 2
            0.5, // rotation 0.5 rad
            UpdatePolicy::Automatic,
        )
        .expect("create carrier");

        let elem_ref = ElementRef::new(ModelId::new(), ElementId::new());
        let src = SourceReference {
            element: elem_ref,
            xref_source: None,
            component_id: None,
            feature: Some(FeatureKey::LayerFace),
            display_component_id: None,
        };

        let prim = DerivedPrimitive {
            id: super::super::provenance::PrimitiveId::new(mv_id, 1, 1),
            role: PrimitiveRole::CutFill,
            geometry: Geometry2d::Region {
                outer: vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(5.0, 0.0),
                    Point2::new(5.0, 2.0),
                    Point2::new(0.0, 2.0),
                ],
                holes: vec![],
            },
            sources: SourceSet::new(vec![src]).unwrap(),
            depth: DepthInterval::cut_plane(),
            style: ResolvedStyle {
                line_weight_mm: PaperMm::new(0.18).unwrap(),
                line_color: None, // Must become ByLayer in CAD
                line_type: None,
                fill_color: None,
                hatch_pattern: Some("ANSI31".to_string()),
                hatch_scale: Some(0.5),
                hatch_angle_rad: Some(0.785398), // 45 deg
                layer_name: Some("AEC_WALL_HATCH".to_string()),
            },
            edit_handles: vec![],
        };

        sync_carrier_primitives(&mut doc, &mut carrier, &[prim]).expect("sync primitives");
        assert_eq!(carrier.derived_handles.len(), 1);

        let hatch_handle = carrier.derived_handles[0];
        let hatch_ent = doc.get_entity(hatch_handle).expect("hatch exists");

        // Color must be ByLayer when style has None for color
        assert_eq!(hatch_ent.common().color, acadrust::types::Color::ByLayer);

        if let EntityType::Hatch(hatch) = hatch_ent {
            assert!(!hatch.is_solid);
            assert_eq!(hatch.pattern.name, "ANSI31");
            // Placement scale does not change the model-space hatch scale.
            assert!((hatch.pattern_scale - 0.5).abs() < 1e-4);
            // pattern_angle = hatch_angle_rad + carrier.rotation_rad = 0.785398 + 0.5 = 1.285398
            assert!((hatch.pattern_angle - (0.785398 + 0.5)).abs() < 1e-4);
        } else {
            panic!("Expected Hatch entity");
        }
    }

    #[test]
    fn test_carrier_preserves_resolved_hatch_fill_color() {
        let mut doc = CadDocument::new();
        let mv_id = ModelViewId::new();
        let mut carrier = create_view_carrier(
            &mut doc,
            ViewpointId::new(),
            mv_id,
            Point2::new(0.0, 0.0),
            1.0,
            0.0,
            UpdatePolicy::Automatic,
        )
        .expect("create carrier");
        let source = SourceReference {
            element: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            component_id: None,
            feature: Some(FeatureKey::LayerFace),
            display_component_id: None,
        };
        let prim = DerivedPrimitive {
            id: super::super::provenance::PrimitiveId::new(mv_id, 1, 1),
            role: PrimitiveRole::CutFill,
            geometry: Geometry2d::Region {
                outer: vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(2.0, 0.0),
                    Point2::new(2.0, 1.0),
                    Point2::new(0.0, 1.0),
                ],
                holes: vec![],
            },
            sources: SourceSet::new(vec![source]).unwrap(),
            depth: DepthInterval::cut_plane(),
            style: ResolvedStyle {
                line_weight_mm: PaperMm::new(0.18).unwrap(),
                line_color: None,
                line_type: None,
                fill_color: Some(super::super::provenance::Rgba8::new(80, 100, 120, 255)),
                hatch_pattern: Some("ANSI31".to_string()),
                hatch_scale: Some(0.5),
                hatch_angle_rad: Some(0.0),
                layer_name: Some("AEC_WALL_HATCH".to_string()),
            },
            edit_handles: vec![],
        };

        sync_carrier_primitives(&mut doc, &mut carrier, &[prim]).expect("sync primitives");
        let hatch = doc.get_entity(carrier.derived_handles[0]).expect("hatch exists");
        assert_eq!(
            hatch.common().color,
            acadrust::types::Color::Rgb {
                r: 80,
                g: 100,
                b: 120
            }
        );
    }

    #[test]
    fn carrier_anchor_is_a_labeled_diamond_that_moves_with_the_insertion() {
        use super::super::model_view::{DetailPolicy, ModelView, ViewRevisions};
        use super::super::viewpoint::{SectionPath, VerticalRange, ViewAxes, ViewKind, ViewOrigin};
        use super::super::{Metres, Point3, ProjectionMapping, ScaleDenominator, Vector2};

        let mut doc = CadDocument::new();
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();
        let mut carrier = create_view_carrier(
            &mut doc,
            vp_id,
            mv_id,
            Point2::new(4.0, 2.0),
            1.0,
            0.0,
            UpdatePolicy::Automatic,
        )
        .expect("create carrier");

        let anchor = doc.get_entity(carrier.handle).expect("anchor");
        let EntityType::LwPolyline(pl) = anchor else {
            panic!("carrier anchor must be a closed diamond, not a point");
        };
        assert!(pl.is_closed);
        assert_eq!(pl.vertices.len(), 4);
        let cx = pl.vertices.iter().map(|v| v.location.x).sum::<f64>() / 4.0;
        let cy = pl.vertices.iter().map(|v| v.location.y).sum::<f64>() / 4.0;
        assert!((cx - 4.0).abs() < 1e-6 && (cy - 2.0).abs() < 1e-6);

        let vp = Viewpoint {
            id: vp_id,
            name: "Schnitt A".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::section(Vector2::new(1.0, 0.0)).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(8.0, 0.0)),
                far_depth: None,
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };
        let mv = ModelView {
            id: mv_id,
            name: "Schnitt A".into(),
            viewpoint_id: vp_id,
            profile_id: super::super::ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };
        assert!(write_view_carrier_definitions(&mut doc, carrier.handle, &vp, &mv));

        let label = doc
            .entities()
            .find_map(|entity| match entity {
                EntityType::MText(text) if text.value.contains("Schnitt A") && text.value.contains("1:50") => {
                    Some(text.insertion_point)
                }
                _ => None,
            })
            .expect("labeled badge");
        assert!((label.x - (4.0 + BADGE_HALF + 0.15)).abs() < 1e-6);

        carrier.insertion_point = Point2::new(9.0, 3.0);
        assert!(write_view_carrier(&mut doc, &carrier));
        let moved = doc.get_entity(carrier.handle).expect("moved anchor");
        let EntityType::LwPolyline(pl) = moved else {
            panic!("anchor stayed a polyline");
        };
        let cx = pl.vertices.iter().map(|v| v.location.x).sum::<f64>() / 4.0;
        let cy = pl.vertices.iter().map(|v| v.location.y).sum::<f64>() / 4.0;
        assert!((cx - 9.0).abs() < 1e-6 && (cy - 3.0).abs() < 1e-6);
        let moved_label = doc
            .entities()
            .find_map(|entity| match entity {
                EntityType::MText(text) if text.value.contains("1:50") => Some(text.insertion_point),
                _ => None,
            })
            .expect("label moved");
        assert!((moved_label.x - (9.0 + BADGE_HALF + 0.15)).abs() < 1e-6);
        assert!((moved_label.y - (3.0 + 0.08)).abs() < 1e-6);
    }
}
