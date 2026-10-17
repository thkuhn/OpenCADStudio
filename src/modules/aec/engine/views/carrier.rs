//! Associative View Carrier in Model Space.
//!
//! Encapsulates derived 2D views (sections, elevations, floor plans) placed as
//! native CAD entities in the model space, attached with `OPENCAD_AEC` XDATA
//! provenance and child handle tracking.

use uuid::Uuid;

use acadrust::entities::{Line, LwPolyline, LwVertex, Point};
use acadrust::types::{Vector2, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, EntityType, Handle};

use super::model_view::{UpdatePolicy, ViewRevisions};
use super::provenance::{
    DerivedPrimitive, FeatureKey, Geometry2d, PrimitiveRole,
};
use super::{ModelViewId, Point2, Revision, ValidationError, ViewpointId};
use crate::modules::aec::engine::owner_index;
use crate::modules::aec::engine::xdata::{ensure_app_id, AEC_APPID};

pub const VIEW_CARRIER_TAG: &str = "VIEW_CARRIER";
pub const DERIVED_PRIMITIVE_TAG: &str = "DERIVED_PRIMITIVE";

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
    true
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

/// Synchronizes a carrier's derived primitives with native CAD entities in `doc`.
/// Erases old child entities without modifying unrelated user geometry or annotations.
pub fn sync_carrier_primitives(
    doc: &mut CadDocument,
    carrier: &mut DerivedViewCarrier,
    primitives: &[DerivedPrimitive],
) -> Result<(), ValidationError> {
    ensure_app_id(doc);

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
                ent.common_mut().layer = layer_name.to_string();
                attach_primitive_xdata(doc, &mut ent, prim);
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
                ent.common_mut().layer = layer_name.to_string();
                attach_primitive_xdata(doc, &mut ent, prim);
                let _ = doc.add_entity(ent);
                new_handles.push(h);
            }
            Geometry2d::Region { outer, holes } => {
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
                ent.common_mut().layer = layer_name.to_string();
                attach_primitive_xdata(doc, &mut ent, prim);
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
                    hent.common_mut().layer = layer_name.to_string();
                    attach_primitive_xdata(doc, &mut hent, prim);
                    let _ = doc.add_entity(hent);
                    new_handles.push(hh);
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
                ent.common_mut().layer = layer_name.to_string();
                attach_primitive_xdata(doc, &mut ent, prim);
                let _ = doc.add_entity(ent);
                new_handles.push(h);
            }
        }
    }

    carrier.derived_handles = new_handles;
    carrier.is_outdated = false;
    write_view_carrier(doc, carrier);

    Ok(())
}

fn attach_primitive_xdata(_doc: &CadDocument, entity: &mut EntityType, prim: &DerivedPrimitive) {
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

    if let Some(src) = prim.sources.as_slice().first() {
        rec.add_value(XDataValue::String(src.element.model_id.as_uuid().to_string()));
        rec.add_value(XDataValue::String(src.element.element_id.as_uuid().to_string()));
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
        }
        if let Some(xref) = &src.xref_source {
            rec.add_value(XDataValue::String(xref.block_record_name.clone()));
        }
    }

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
    }
}
