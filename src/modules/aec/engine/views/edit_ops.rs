//! Semantic backward editing of 3D BIM model entities from 2D derived views.
//!
//! Provides capability checks, direction validation, delta lifting from view-space
//! to 3D world parameters, and transactional mutation execution for walls, openings, and slabs.

use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, Handle};

use super::model_view::ModelView;
use super::provenance::{EditHandle, ElementRef, FeatureKey, ModelEditAction, ModelParameter};
use super::scene_collector::SceneGeometryCollector;
use super::update_ops::{mark_elements_modified, update_outdated_automatic_carriers};
use super::viewpoint::Viewpoint;
use super::Point2;
use crate::modules::aec::engine::openings::Opening;
use crate::modules::aec::engine::wall::Wall;
use crate::modules::aec::engine::xdata::{ensure_app_id, write_aec_record, AEC_APPID};

/// Errors encountered during semantic view-to-model backward edits.
#[derive(Clone, Debug, PartialEq)]
pub enum EditOpError {
    ValidationError(String),
    ElementNotFound(ElementRef),
    InvalidCapability(String),
    DegenerateProjection(String),
    BoundsExceeded(String),
    XrefReadOnly(String),
}

impl std::fmt::Display for EditOpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ValidationError(msg) => write!(f, "Validation error: {}", msg),
            Self::ElementNotFound(elem) => {
                write!(f, "Element not found in scene: {:?}", elem)
            }
            Self::InvalidCapability(msg) => write!(f, "Invalid capability: {}", msg),
            Self::DegenerateProjection(msg) => write!(f, "Degenerate projection: {}", msg),
            Self::BoundsExceeded(msg) => write!(f, "Parameter bounds exceeded: {}", msg),
            Self::XrefReadOnly(msg) => write!(f, "External reference is read-only: {}", msg),
        }
    }
}

impl std::error::Error for EditOpError {}

/// A validated semantic mutation ready to be applied to the BIM model.
#[derive(Clone, Debug, PartialEq)]
pub enum SemanticMutation {
    SetWallHeight {
        element: ElementRef,
        handle: Handle,
        new_height: f64,
        delta_z: f64,
    },
    SetOpeningSillHeight {
        element: ElementRef,
        handle: Handle,
        new_sill_height: f64,
        delta_z: f64,
    },
    SetOpeningHeadHeight {
        element: ElementRef,
        handle: Handle,
        new_height: f64,
        delta_z: f64,
    },
    MoveOpeningAlongHost {
        element: ElementRef,
        handle: Handle,
        new_distance: f64,
        delta_s: f64,
    },
    SetSlabElevation {
        element: ElementRef,
        handle: Handle,
        new_elevation: f64,
        delta_z: f64,
    },
    SetSlabThickness {
        element: ElementRef,
        handle: Handle,
        new_thickness: f64,
        delta_thickness: f64,
    },
}

impl SemanticMutation {
    pub fn element(&self) -> ElementRef {
        match self {
            Self::SetWallHeight { element, .. }
            | Self::SetOpeningSillHeight { element, .. }
            | Self::SetOpeningHeadHeight { element, .. }
            | Self::MoveOpeningAlongHost { element, .. }
            | Self::SetSlabElevation { element, .. }
            | Self::SetSlabThickness { element, .. } => *element,
        }
    }

    pub fn handle(&self) -> Handle {
        match self {
            Self::SetWallHeight { handle, .. }
            | Self::SetOpeningSillHeight { handle, .. }
            | Self::SetOpeningHeadHeight { handle, .. }
            | Self::MoveOpeningAlongHost { handle, .. }
            | Self::SetSlabElevation { handle, .. }
            | Self::SetSlabThickness { handle, .. } => *handle,
        }
    }
}

/// Resolves an interactive edit handle drag into a typed `SemanticMutation`.
///
/// Performs direction checks, angle validations, and parameter bounds verification.
pub fn resolve_handle_drag(
    handle: &EditHandle,
    viewpoint: &Viewpoint,
    view_delta: Point2,
    scene: &SceneGeometryCollector,
) -> Result<SemanticMutation, EditOpError> {
    let elem_ref = handle.source.element;

    // Check if target element is from an XREF source
    if let Some(xref) = &handle.source.xref_source {
        return Err(EditOpError::XrefReadOnly(format!(
            "Element {:?} belongs to external reference block '{}'; direct in-place editing requires host document editing",
            elem_ref, xref.block_record_name
        )));
    }

    // Check viewpoint axes for vertical lift capability
    let up_z = viewpoint.axes.up.z;
    let is_vertical_view = up_z.abs() > 0.5;

    match (&handle.action, handle.target_feature.as_ref()) {
        // 1. Wall Top / Wall Base / Wall Height
        (ModelEditAction::SetWallHeight, _)
        | (ModelEditAction::ChangeParameter(ModelParameter::WallHeight), _)
        | (ModelEditAction::ChangeParameter(ModelParameter::WallTopOffset), _)
        | (_, Some(FeatureKey::WallTop | FeatureKey::WallBase)) => {
            if !is_vertical_view {
                return Err(EditOpError::DegenerateProjection(
                    "Cannot adjust vertical wall height in a horizontal plan view".into(),
                ));
            }

            let wall = scene
                .walls
                .iter()
                .find(|w| w.element_ref == elem_ref)
                .ok_or(EditOpError::ElementNotFound(elem_ref))?;

            // View delta dy represents vertical change
            let factor = if matches!(handle.target_feature, Some(FeatureKey::WallBase)) {
                -1.0
            } else {
                1.0
            };
            let delta_z = view_delta.y_m * up_z.signum() * factor;
            let new_height = wall.height + delta_z;

            if new_height < 0.05 {
                return Err(EditOpError::BoundsExceeded(format!(
                    "New wall height {:.3}m is below minimum 0.05m",
                    new_height
                )));
            }
            if new_height > 50.0 {
                return Err(EditOpError::BoundsExceeded(format!(
                    "New wall height {:.3}m exceeds maximum 50.0m",
                    new_height
                )));
            }

            Ok(SemanticMutation::SetWallHeight {
                element: elem_ref,
                handle: wall.handle,
                new_height,
                delta_z,
            })
        }

        // 2. Opening Sill Height
        (ModelEditAction::SetOpeningSillHeight, _)
        | (ModelEditAction::ChangeParameter(ModelParameter::OpeningSillHeight), _)
        | (_, Some(FeatureKey::OpeningSill)) => {
            if !is_vertical_view {
                return Err(EditOpError::DegenerateProjection(
                    "Cannot adjust opening sill height in a horizontal plan view".into(),
                ));
            }

            let (wall, opening) = find_opening_and_host(scene, elem_ref)?;

            let delta_z = view_delta.y_m * up_z.signum();
            let new_sill = opening.sill_height + delta_z;

            if new_sill < 0.0 {
                return Err(EditOpError::BoundsExceeded(format!(
                    "New sill height {:.3}m cannot be negative",
                    new_sill
                )));
            }
            if new_sill + opening.height > wall.height + 2.0 {
                return Err(EditOpError::BoundsExceeded(format!(
                    "New sill height {:.3}m causes opening to exceed wall height {:.3}m",
                    new_sill, wall.height
                )));
            }

            Ok(SemanticMutation::SetOpeningSillHeight {
                element: elem_ref,
                handle: opening.handle,
                new_sill_height: new_sill,
                delta_z,
            })
        }

        // 3. Opening Head / Height
        (ModelEditAction::ChangeParameter(ModelParameter::OpeningHeadHeight), _)
        | (_, Some(FeatureKey::OpeningHead)) => {
            if !is_vertical_view {
                return Err(EditOpError::DegenerateProjection(
                    "Cannot adjust opening head height in a horizontal plan view".into(),
                ));
            }

            let (_wall, opening) = find_opening_and_host(scene, elem_ref)?;

            let delta_z = view_delta.y_m * up_z.signum();
            let new_height = opening.height + delta_z;

            if new_height < 0.05 {
                return Err(EditOpError::BoundsExceeded(format!(
                    "New opening height {:.3}m is below minimum 0.05m",
                    new_height
                )));
            }

            Ok(SemanticMutation::SetOpeningHeadHeight {
                element: elem_ref,
                handle: opening.handle,
                new_height,
                delta_z,
            })
        }

        // 4. Move Opening Along Host
        (ModelEditAction::MoveAlongHost { .. }, _)
        | (_, Some(FeatureKey::OpeningLeftJamb | FeatureKey::OpeningRightJamb)) => {
            let (wall, opening) = find_opening_and_host(scene, elem_ref)?;

            // Host wall axis direction
            let axis_pts = &wall.axis_points;
            if axis_pts.len() < 2 {
                return Err(EditOpError::InvalidCapability("Wall axis is too short".into()));
            }
            let host_len: f64 = axis_pts
                .windows(2)
                .map(|w| ((w[1].x_m - w[0].x_m).powi(2) + (w[1].y_m - w[0].y_m).powi(2)).sqrt())
                .sum();

            let delta_s = if is_vertical_view {
                view_delta.x_m
            } else {
                let dx = axis_pts[1].x_m - axis_pts[0].x_m;
                let dy = axis_pts[1].y_m - axis_pts[0].y_m;
                let len = (dx * dx + dy * dy).sqrt();
                if len > 1e-6 {
                    let dir_x = dx / len;
                    let dir_y = dy / len;
                    view_delta.x_m * dir_x + view_delta.y_m * dir_y
                } else {
                    view_delta.x_m
                }
            };
            let new_distance = opening.distance_along_axis + delta_s;

            if new_distance < 0.0 || new_distance > host_len {
                return Err(EditOpError::BoundsExceeded(format!(
                    "New distance {:.3}m is outside host wall bounds [0.0, {:.3}]m",
                    new_distance, host_len
                )));
            }

            Ok(SemanticMutation::MoveOpeningAlongHost {
                element: elem_ref,
                handle: opening.handle,
                new_distance,
                delta_s,
            })
        }

        // 5. Slab Elevation
        (ModelEditAction::SetSlabElevation, _)
        | (ModelEditAction::ChangeParameter(ModelParameter::SlabOffset), _)
        | (_, Some(FeatureKey::SlabTop | FeatureKey::SlabBottom)) => {
            if !is_vertical_view {
                return Err(EditOpError::DegenerateProjection(
                    "Cannot adjust slab elevation in a horizontal plan view".into(),
                ));
            }

            let slab = scene
                .slabs
                .iter()
                .find(|s| s.element_ref == elem_ref)
                .ok_or(EditOpError::ElementNotFound(elem_ref))?;

            let delta_z = view_delta.y_m * up_z.signum();
            let new_elevation = slab.base_z + delta_z;

            Ok(SemanticMutation::SetSlabElevation {
                element: elem_ref,
                handle: slab.handle,
                new_elevation,
                delta_z,
            })
        }

        // Fallback translation
        (ModelEditAction::TranslateInViewPlane, _) => {
            Err(EditOpError::InvalidCapability(
                "Generic view plane translation has no unconstrained 3D inverse".into(),
            ))
        }

        _ => Err(EditOpError::InvalidCapability(format!(
            "Unsupported edit action {:?} for feature {:?}",
            handle.action, handle.target_feature
        ))),
    }
}

/// Helper locating an opening and its host wall in the collected scene.
fn find_opening_and_host<'a>(
    scene: &'a SceneGeometryCollector,
    elem_ref: ElementRef,
) -> Result<
    (
        &'a super::scene_collector::CollectedWall,
        &'a super::scene_collector::CollectedOpening,
    ),
    EditOpError,
> {
    for wall in &scene.walls {
        for op in &wall.openings {
            if op.element_ref == elem_ref {
                return Ok((wall, op));
            }
        }
    }
    Err(EditOpError::ElementNotFound(elem_ref))
}

/// Applies a validated `SemanticMutation` directly to the `CadDocument`.
///
/// Updates entity XDATA records and clears any bound plane constraints.
pub fn apply_semantic_mutation(
    doc: &mut CadDocument,
    mutation: &SemanticMutation,
) -> Result<Handle, EditOpError> {
    ensure_app_id(doc);
    let handle = mutation.handle();

    let entity = doc
        .get_entity_mut(handle)
        .ok_or_else(|| EditOpError::ValidationError(format!("Handle {:?} not found", handle)))?;

    match mutation {
        SemanticMutation::SetWallHeight { new_height, .. } => {
            let mut wall: Option<Wall> = None;
            for rec in entity.common().extended_data.records() {
                if rec.application_name == AEC_APPID
                    && matches!(rec.values.first(), Some(XDataValue::String(s)) if s == "WALL")
                {
                    wall = crate::modules::aec::engine::xdata::wall_from_entity(entity);
                    break;
                }
            }

            let Some(mut wall) = wall else {
                return Err(EditOpError::ValidationError(format!(
                    "Entity {:?} does not carry WALL XDATA",
                    handle
                )));
            };

            wall.height = *new_height;
            wall.base_plane_id = None;
            wall.top_plane_id = None;

            let mut record = ExtendedDataRecord::new(AEC_APPID);
            record.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
            write_aec_record(doc, handle, record);
        }

        SemanticMutation::SetOpeningSillHeight { new_sill_height, .. } => {
            let mut opening: Option<Opening> = None;
            for rec in entity.common().extended_data.records() {
                if rec.application_name == AEC_APPID
                    && matches!(rec.values.first(), Some(XDataValue::String(s)) if s == "OPENING")
                {
                    opening = crate::modules::aec::engine::opening_xdata::opening_from_entity(
                        entity, handle,
                    );
                    break;
                }
            }

            let Some(mut opening) = opening else {
                return Err(EditOpError::ValidationError(format!(
                    "Entity {:?} does not carry OPENING XDATA",
                    handle
                )));
            };

            opening.sill_height = *new_sill_height;
            opening.sill_plane_id = None;

            let record = crate::modules::aec::engine::opening_xdata::opening_record(&opening);
            write_aec_record(doc, handle, record);
        }

        SemanticMutation::SetOpeningHeadHeight { new_height, .. } => {
            let mut opening: Option<Opening> = None;
            for rec in entity.common().extended_data.records() {
                if rec.application_name == AEC_APPID
                    && matches!(rec.values.first(), Some(XDataValue::String(s)) if s == "OPENING")
                {
                    opening = crate::modules::aec::engine::opening_xdata::opening_from_entity(
                        entity, handle,
                    );
                    break;
                }
            }

            let Some(mut opening) = opening else {
                return Err(EditOpError::ValidationError(format!(
                    "Entity {:?} does not carry OPENING XDATA",
                    handle
                )));
            };

            opening.height = *new_height;
            opening.head_plane_id = None;

            let record = crate::modules::aec::engine::opening_xdata::opening_record(&opening);
            write_aec_record(doc, handle, record);
        }

        SemanticMutation::MoveOpeningAlongHost { new_distance, .. } => {
            let mut opening: Option<Opening> = None;
            for rec in entity.common().extended_data.records() {
                if rec.application_name == AEC_APPID
                    && matches!(rec.values.first(), Some(XDataValue::String(s)) if s == "OPENING")
                {
                    opening = crate::modules::aec::engine::opening_xdata::opening_from_entity(
                        entity, handle,
                    );
                    break;
                }
            }

            let Some(mut opening) = opening else {
                return Err(EditOpError::ValidationError(format!(
                    "Entity {:?} does not carry OPENING XDATA",
                    handle
                )));
            };

            opening.distance_along_axis = *new_distance;

            let record = crate::modules::aec::engine::opening_xdata::opening_record(&opening);
            write_aec_record(doc, handle, record);
        }

        SemanticMutation::SetSlabElevation { new_elevation, .. } => {
            let slab = crate::modules::aec::engine::slab_xdata::slab_from_entity(entity);
            let Some(mut slab) = slab else {
                return Err(EditOpError::ValidationError(format!(
                    "Entity {:?} does not carry SLAB XDATA",
                    handle
                )));
            };

            slab.base_origin[2] = *new_elevation;
            slab.top_origin[2] = *new_elevation;
            slab.base_plane_id = None;
            slab.top_plane_id = None;

            crate::modules::aec::engine::slab_xdata::write_slab_record(doc, handle, &slab);
        }

        SemanticMutation::SetSlabThickness { new_thickness, .. } => {
            let slab = crate::modules::aec::engine::slab_xdata::slab_from_entity(entity);
            let Some(mut slab) = slab else {
                return Err(EditOpError::ValidationError(format!(
                    "Entity {:?} does not carry SLAB XDATA",
                    handle
                )));
            };

            if let Some(first_layer) = slab.layers.first_mut() {
                first_layer.thickness = *new_thickness;
            }

            crate::modules::aec::engine::slab_xdata::write_slab_record(doc, handle, &slab);
        }
    }

    Ok(handle)
}

/// Applies a semantic handle drag, updates affected carriers, and re-generates automatic views.
pub fn commit_view_drag_and_update(
    doc: &mut CadDocument,
    handle: &EditHandle,
    viewpoint: &Viewpoint,
    view_delta: Point2,
    scene: &SceneGeometryCollector,
    viewpoints: &[Viewpoint],
    model_views: &[ModelView],
) -> Result<SemanticMutation, EditOpError> {
    let mutation = resolve_handle_drag(handle, viewpoint, view_delta, scene)?;
    apply_semantic_mutation(doc, &mutation)?;

    let modified_elem = mutation.element();
    mark_elements_modified(doc, &[modified_elem], viewpoints, model_views, scene);
    let _ = update_outdated_automatic_carriers(doc, viewpoints, model_views, scene);

    Ok(mutation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::EntityType;
    use crate::modules::aec::engine::openings::OpeningKind;
    use crate::modules::aec::engine::plan_view::PlanPhase;
    use crate::modules::aec::engine::views::provenance::{SourceReference, XrefSourceRef};
    use crate::modules::aec::engine::views::scene_collector::{CollectedOpening, CollectedWall};
    use crate::modules::aec::engine::views::viewpoint::{SectionPath, VerticalRange, ViewAxes, ViewOrigin};
    use crate::modules::aec::engine::views::{ElementId, Metres, ModelId, Point2, Point3, Revision, Vector2};
    use crate::modules::aec::engine::wall::WallJustification;

    fn make_test_viewpoint() -> Viewpoint {
        Viewpoint {
            id: super::super::ViewpointId::new(),
            name: "Section A-A".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::section(Vector2::new(1.0, 0.0)).unwrap(),
            kind: super::super::viewpoint::ViewKind::Section {
                path: SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
                far_depth: None,
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: super::super::ProjectionMapping::default(),
            crop: None,
        }
    }

    #[test]
    fn test_resolve_wall_height_drag() {
        let elem = ElementRef::new(ModelId::new(), ElementId::new());
        let wall = CollectedWall {
            handle: Handle::new(10),
            element_ref: elem,
            xref_source: None,
            style_id: "W1".into(),
            height: 2.8,
            base_z: 0.0,
            top_z: 2.8,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(5.0, 0.0)],
            layers: vec![],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: vec![],
            phase: PlanPhase::Existing,
        };

        let mut scene = SceneGeometryCollector::default();
        scene.walls.push(wall);

        let vp = make_test_viewpoint();
        let edit_handle = EditHandle {
            id: "wall_top_1".into(),
            position: Point2::new(2.5, 2.8),
            action: ModelEditAction::SetWallHeight,
            direction_hint: Vector2::new(0.0, 1.0),
            target_feature: Some(FeatureKey::WallTop),
            source: SourceReference::new(elem),
        };

        // Drag up by 0.5m
        let mutation = resolve_handle_drag(&edit_handle, &vp, Point2::new(0.0, 0.5), &scene).unwrap();
        assert_eq!(
            mutation,
            SemanticMutation::SetWallHeight {
                element: elem,
                handle: Handle::new(10),
                new_height: 3.3,
                delta_z: 0.5,
            }
        );

        // Drag down to negative height must fail bounds check
        let err = resolve_handle_drag(&edit_handle, &vp, Point2::new(0.0, -3.0), &scene).unwrap_err();
        assert!(matches!(err, EditOpError::BoundsExceeded(_)));
    }

    #[test]
    fn test_resolve_opening_sill_and_move_drag() {
        let wall_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let op_elem = ElementRef::new(ModelId::new(), ElementId::new());

        let opening = CollectedOpening {
            handle: Handle::new(20),
            element_ref: op_elem,
            xref_source: None,
            host_wall_handle: Handle::new(10),
            kind: OpeningKind::Window,
            style_id: None,
            distance_along_axis: 2.0,
            width: 1.2,
            height: 1.5,
            sill_height: 0.9,
            spring_height: 0.0,
            shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
            hinge: crate::modules::aec::engine::opening_style::HingeSide::Left,
            reference_side: crate::modules::aec::engine::openings::OpeningReferenceSide::Start,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(2.0, 0.0, 1.5),
            axis_span_local: (1.4, 2.6),
        };

        let wall = CollectedWall {
            handle: Handle::new(10),
            element_ref: wall_elem,
            xref_source: None,
            style_id: "W1".into(),
            height: 2.8,
            base_z: 0.0,
            top_z: 2.8,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(5.0, 0.0)],
            layers: vec![],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: vec![opening],
            phase: PlanPhase::Existing,
        };

        let mut scene = SceneGeometryCollector::default();
        scene.walls.push(wall);

        let vp = make_test_viewpoint();

        // 1. Sill height drag up by 0.2m
        let sill_handle = EditHandle {
            id: "op_sill_1".into(),
            position: Point2::new(2.6, 0.9),
            action: ModelEditAction::SetOpeningSillHeight,
            direction_hint: Vector2::new(0.0, 1.0),
            target_feature: Some(FeatureKey::OpeningSill),
            source: SourceReference::new(op_elem),
        };
        let sill_mut = resolve_handle_drag(&sill_handle, &vp, Point2::new(0.0, 0.2), &scene).unwrap();
        assert_eq!(
            sill_mut,
            SemanticMutation::SetOpeningSillHeight {
                element: op_elem,
                handle: Handle::new(20),
                new_sill_height: 1.1,
                delta_z: 0.2,
            }
        );

        // 2. Move along host by +0.5m
        let move_handle = EditHandle {
            id: "op_jamb_1".into(),
            position: Point2::new(2.0, 1.5),
            action: ModelEditAction::MoveAlongHost {
                host: wall_elem,
                axis_world: super::super::Vector3::new(1.0, 0.0, 0.0),
            },
            direction_hint: Vector2::new(1.0, 0.0),
            target_feature: Some(FeatureKey::OpeningLeftJamb),
            source: SourceReference::new(op_elem),
        };
        let move_mut = resolve_handle_drag(&move_handle, &vp, Point2::new(0.5, 0.0), &scene).unwrap();
        assert_eq!(
            move_mut,
            SemanticMutation::MoveOpeningAlongHost {
                element: op_elem,
                handle: Handle::new(20),
                new_distance: 2.5,
                delta_s: 0.5,
            }
        );
    }

    #[test]
    fn test_xref_source_drag_is_rejected() {
        let elem = ElementRef::new(ModelId::new(), ElementId::new());
        let xref = XrefSourceRef::with_translation("EG_Block", Some("EG.dwg".into()), 0.0, 0.0, 2.8);

        let vp = make_test_viewpoint();
        let edit_handle = EditHandle {
            id: "wall_top_xref".into(),
            position: Point2::new(2.5, 2.8),
            action: ModelEditAction::SetWallHeight,
            direction_hint: Vector2::new(0.0, 1.0),
            target_feature: Some(FeatureKey::WallTop),
            source: SourceReference::new(elem).with_xref(xref),
        };

        let scene = SceneGeometryCollector::default();
        let err = resolve_handle_drag(&edit_handle, &vp, Point2::new(0.0, 0.5), &scene).unwrap_err();
        assert!(matches!(err, EditOpError::XrefReadOnly(_)));
    }

    #[test]
    fn test_apply_wall_height_mutation() {
        let mut doc = CadDocument::new();
        let wall = Wall::new("W1", 2.8, 0);
        let pl = acadrust::entities::LwPolyline::new();
        let mut entity = EntityType::LwPolyline(pl);
        let h = doc.allocate_handle();
        entity.common_mut().handle = h;
        entity.common_mut().layer = "AEC_WALL".into();

        let mut rec = ExtendedDataRecord::new(AEC_APPID);
        rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
        entity.common_mut().extended_data.add_record(rec);
        let _ = doc.add_entity(entity);

        let elem = ElementRef::new(ModelId::new(), ElementId::new());
        let mutation = SemanticMutation::SetWallHeight {
            element: elem,
            handle: h,
            new_height: 3.2,
            delta_z: 0.4,
        };

        let res = apply_semantic_mutation(&mut doc, &mutation);
        assert_eq!(res.unwrap(), h);

        let updated_entity = doc.get_entity(h).unwrap();
        let updated_wall = crate::modules::aec::engine::xdata::wall_from_entity(updated_entity).unwrap();
        assert_eq!(updated_wall.height, 3.2);
    }

    #[test]
    fn test_apply_opening_sill_mutation() {
        let mut doc = CadDocument::new();
        let op = Opening::new(Handle::new(10), Handle::new(5), 2.0, 1.2, 1.5, 0.9, OpeningKind::Window);
        let pt = acadrust::entities::Point::from_coords(2.0, 0.0, 0.0);
        let mut entity = EntityType::Point(pt);
        let h = doc.allocate_handle();
        entity.common_mut().handle = h;

        let rec = crate::modules::aec::engine::opening_xdata::opening_record(&op);
        entity.common_mut().extended_data.add_record(rec);
        let _ = doc.add_entity(entity);

        let elem = ElementRef::new(ModelId::new(), ElementId::new());
        let mutation = SemanticMutation::SetOpeningSillHeight {
            element: elem,
            handle: h,
            new_sill_height: 1.05,
            delta_z: 0.15,
        };

        let res = apply_semantic_mutation(&mut doc, &mutation);
        assert_eq!(res.unwrap(), h);

        let updated_entity = doc.get_entity(h).unwrap();
        let updated_op = crate::modules::aec::engine::opening_xdata::opening_from_entity(updated_entity, h).unwrap();
        assert_eq!(updated_op.sill_height, 1.05);
    }

    #[test]
    fn test_apply_slab_elevation_mutation() {
        use crate::modules::aec::engine::slab::{Slab, SlabLayer};
        use crate::modules::aec::engine::wall_style::LayerFunction;
        use codec::EntityType;
        let mut doc = CadDocument::new();
        let mut slab = Slab::new("S1", 0);
        slab.layers.push(SlabLayer::new(
            "Concrete",
            0.20,
            LayerFunction::Structural,
        ));
        slab.base_origin[2] = 2.8;
        slab.top_origin[2] = 2.8;

        let pl = acadrust::entities::LwPolyline::new();
        let mut entity = EntityType::LwPolyline(pl);
        let h = doc.allocate_handle();
        entity.common_mut().handle = h;

        let mut rec = ExtendedDataRecord::new(AEC_APPID);
        rec.values = crate::modules::aec::engine::slab_xdata::slab_record_for_slab(&slab);
        entity.common_mut().extended_data.add_record(rec);
        let _ = doc.add_entity(entity);

        let elem = ElementRef::new(ModelId::new(), ElementId::new());
        let mutation = SemanticMutation::SetSlabElevation {
            element: elem,
            handle: h,
            new_elevation: 3.1,
            delta_z: 0.3,
        };

        let res = apply_semantic_mutation(&mut doc, &mutation);
        assert_eq!(res.unwrap(), h);

        let updated_entity = doc.get_entity(h).unwrap();
        let updated_slab = crate::modules::aec::engine::slab_xdata::slab_from_entity(updated_entity).unwrap();
        assert_eq!(updated_slab.base_origin[2], 3.1);
    }

    #[test]
    fn test_derived_view_grip_drag_and_undo_cycle() {
        use crate::app::OpenCADStudio;
        use crate::modules::aec::engine::views::carrier::{
            create_view_carrier, find_view_carrier_by_handle,
            sync_carrier_primitives, write_view_carrier_definitions,
        };
        use crate::modules::aec::engine::views::cut_engine::{derive_section_view, CutEngineOptions};
        use crate::modules::aec::engine::views::model_view::{
            DetailPolicy, ModelView, UpdatePolicy, ViewRevisions,
        };
        use crate::modules::aec::engine::views::scene_collector::SceneGeometryCollector;
        use crate::modules::aec::engine::views::viewpoint::{SectionPath, VerticalRange, ViewAxes, ViewKind, ViewOrigin, Viewpoint};
        use crate::modules::aec::engine::views::{Metres, ModelId, ModelViewId, Point2, Point3, Revision, ScaleDenominator, Vector2, ViewProfileId, ViewpointId};
        use crate::modules::aec::properties::{append_derived_view_grips, DERIVED_VIEW_GRIP_BASE};

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;

        // 1. Create a 3D wall at y = 0
        let mut wall = Wall::new("W1", 2.8, 0);
        wall.layers.push(crate::modules::aec::engine::wall::WallLayer {
            material: "Concrete".into(),
            thickness: 0.24,
            function: "Structural".into(),
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
        });
        let mut pl = acadrust::entities::LwPolyline::new();
        pl.add_vertex(acadrust::entities::LwVertex::new(acadrust::types::Vector2::new(0.0, 0.0)));
        pl.add_vertex(acadrust::entities::LwVertex::new(acadrust::types::Vector2::new(10.0, 0.0)));
        let mut wall_entity = EntityType::LwPolyline(pl);
        wall_entity.common_mut().layer = "AEC_WALL".into();

        let mut rec = ExtendedDataRecord::new(AEC_APPID);
        rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
        wall_entity.common_mut().extended_data.add_record(rec);
        let wall_handle = app.tabs[tab].scene.document.add_entity(wall_entity).unwrap();

        // 2. Create Section Viewpoint and Carrier
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let carrier = create_view_carrier(
            &mut app.tabs[tab].scene.document,
            vp_id,
            mv_id,
            Point2::new(50.0, 0.0),
            1.0,
            0.0,
            UpdatePolicy::Automatic,
        )
        .unwrap();

        let vp = Viewpoint {
            id: vp_id,
            name: "Section A".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(5.0, -5.0, 0.0)),
            axes: ViewAxes::section(Vector2::new(0.0, 1.0)).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(5.0, -5.0), Point2::new(5.0, 5.0)),
                far_depth: None,
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(4.0).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };
        let mv = ModelView {
            id: mv_id,
            name: "Section A MV".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };
        write_view_carrier_definitions(&mut app.tabs[tab].scene.document, carrier.handle, &vp, &mv);

        // 3. Derive section primitives and sync carrier
        let collector = SceneGeometryCollector::collect_from_document(&app.tabs[tab].scene.document, ModelId::new());
        let prims = derive_section_view(&vp, &mv, &collector, &CutEngineOptions::default()).unwrap();
        let mut carrier_mut = find_view_carrier_by_handle(&app.tabs[tab].scene.document, carrier.handle).unwrap();
        sync_carrier_primitives(&mut app.tabs[tab].scene.document, &mut carrier_mut, &prims).unwrap();

        // 4. Find the derived wall top boundary entity
        let mut wall_top_handle = None;
        for &h in &carrier_mut.derived_handles {
            if let Some(ent) = app.tabs[tab].scene.document.get_entity(h) {
                if let Some(meta) = crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(ent) {
                    if meta.feature == Some(FeatureKey::WallTop) {
                        wall_top_handle = Some(h);
                        break;
                    }
                }
            }
        }
        let edge_handle = wall_top_handle.expect("found derived wall top line");

        // 5. Verify grip generation
        let mut grips = Vec::new();
        append_derived_view_grips(&app.tabs[tab].scene, edge_handle, &mut grips);
        assert_eq!(grips.len(), 1);
        assert_eq!(grips[0].id, DERIVED_VIEW_GRIP_BASE);

        // Prepare viewport for camera projection and hit testing
        {
            let tab_ref = &mut app.tabs[tab];
            tab_ref.scene.selection.borrow_mut().view.vp_size = (1920.0, 1080.0);
            tab_ref.scene.sync_tiles_from_panes(1920.0, 1080.0);
            tab_ref.scene.fit_all();
        }

        let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
        let doc_id = state["document_id"].as_u64().unwrap();

        // 6. Move wall top grip up by 0.5m via automation grip_drag
        let drag_req = serde_json::json!({
            "protocol": 1,
            "request_id": "req-grip-1",
            "document_id": doc_id,
            "op": "grip_drag",
            "handle": format!("0x{:X}", edge_handle.value()),
            "grip_id": DERIVED_VIEW_GRIP_BASE,
            "point": [grips[0].world.x, grips[0].world.y + 0.5, grips[0].world.z]
        });
        let drag_res = app.automation_op(&drag_req.to_string());
        assert_eq!(drag_res["ok"], true, "grip_drag failed: {drag_res}");

        // 7. Verify the 3D wall height has changed to 3.30m
        let updated_wall_ent = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
        let updated_wall = crate::modules::aec::engine::xdata::wall_from_entity(updated_wall_ent).unwrap();
        assert!((updated_wall.height - 3.3).abs() < 1e-3, "wall height updated to 3.3m: got {}", updated_wall.height);

        // 8. Verify single undo step restores wall height to 2.80m
        let undo_req = serde_json::json!({
            "protocol": 1,
            "request_id": "req-undo-1",
            "document_id": doc_id,
            "op": "undo"
        });
        let undo_res = app.automation_op(&undo_req.to_string());
        assert_eq!(undo_res["ok"], true, "undo failed: {undo_res}");

        let restored_wall_ent = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
        let restored_wall = crate::modules::aec::engine::xdata::wall_from_entity(restored_wall_ent).unwrap();
        assert!((restored_wall.height - 2.8).abs() < 1e-3, "wall height reverted to 2.8m: got {}", restored_wall.height);
    }
}
