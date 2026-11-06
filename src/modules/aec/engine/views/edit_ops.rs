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
use super::{Point2, Vector3};
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
    SetWallBaseOffset {
        element: ElementRef,
        handle: Handle,
        new_base_z: f64,
        new_height: f64,
        delta_z: f64,
    },
    ResizeOpeningWidth {
        element: ElementRef,
        handle: Handle,
        new_width: f64,
        new_distance: f64,
        delta_s: f64,
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
            | Self::SetWallBaseOffset { element, .. }
            | Self::ResizeOpeningWidth { element, .. }
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
            | Self::SetWallBaseOffset { handle, .. }
            | Self::ResizeOpeningWidth { handle, .. }
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
        // 1. Wall Base (Offset)
        (ModelEditAction::ChangeParameter(ModelParameter::WallBaseOffset), _)
        | (_, Some(FeatureKey::WallBase)) => {
            if !is_vertical_view {
                return Err(EditOpError::DegenerateProjection(
                    "Cannot adjust vertical wall base in a horizontal plan view".into(),
                ));
            }

            let wall = scene
                .walls
                .iter()
                .find(|w| w.element_ref == elem_ref)
                .ok_or(EditOpError::ElementNotFound(elem_ref))?;

            let delta_z = view_delta.y_m * up_z.signum();
            let new_base_z = wall.base_z + delta_z;
            let new_height = wall.height - delta_z;

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

            // Height invariance validation: Wall base cannot be raised above opening sill
            for op in &wall.openings {
                let new_sill = op.sill_height - delta_z;
                if new_sill < -1e-4 {
                    return Err(EditOpError::BoundsExceeded(format!(
                        "Wall base cannot be raised above opening sill (sill would be {:.3}m)",
                        new_sill
                    )));
                }
            }

            Ok(SemanticMutation::SetWallBaseOffset {
                element: elem_ref,
                handle: wall.handle,
                new_base_z,
                new_height,
                delta_z,
            })
        }

        // 2. Wall Top / Wall Height
        (ModelEditAction::SetWallHeight, _)
        | (ModelEditAction::ChangeParameter(ModelParameter::WallHeight), _)
        | (ModelEditAction::ChangeParameter(ModelParameter::WallTopOffset), _)
        | (_, Some(FeatureKey::WallTop)) => {
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

            let delta_z = view_delta.y_m * up_z.signum();
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

        // 4. Resize Opening Width (Stretch)
        (ModelEditAction::ChangeParameter(ModelParameter::OpeningWidth), _) => {
            let (wall, opening) = find_opening_and_host(scene, elem_ref)?;

            let axis_pts = &wall.axis_points;
            if axis_pts.len() < 2 {
                return Err(EditOpError::InvalidCapability("Wall axis is too short".into()));
            }
            let host_len: f64 = axis_pts
                .windows(2)
                .map(|w| ((w[1].x_m - w[0].x_m).powi(2) + (w[1].y_m - w[0].y_m).powi(2)).sqrt())
                .sum();

            let mut seg_dir = Vector3::new(1.0, 0.0, 0.0);
            let mut acc_dist = 0.0;
            for w in axis_pts.windows(2) {
                let seg_dx = w[1].x_m - w[0].x_m;
                let seg_dy = w[1].y_m - w[0].y_m;
                let seg_len = (seg_dx * seg_dx + seg_dy * seg_dy).sqrt();
                if seg_len > 1e-6 {
                    seg_dir = Vector3::new(seg_dx / seg_len, seg_dy / seg_len, 0.0);
                }
                if opening.distance_along_axis <= acc_dist + seg_len + 1e-4 {
                    break;
                }
                acc_dist += seg_len;
            }

            let delta_s = if is_vertical_view {
                let dot = seg_dir.dot(viewpoint.axes.right);
                let sign = if dot < -1e-6 { -1.0 } else { 1.0 };
                view_delta.x_m * sign
            } else {
                view_delta.x_m * seg_dir.x + view_delta.y_m * seg_dir.y
            };

            let is_left_jamb = matches!(handle.target_feature, Some(FeatureKey::OpeningLeftJamb));
            let (orig_start, orig_end) = opening.axis_span_local;
            let (new_start, new_end) = if is_left_jamb {
                (orig_start + delta_s, orig_end)
            } else {
                (orig_start, orig_end + delta_s)
            };

            let new_width = new_end - new_start;
            if new_width < 0.20 {
                return Err(EditOpError::BoundsExceeded(format!(
                    "New opening width {:.3}m is below minimum 0.20m",
                    new_width
                )));
            }
            if new_start < 0.0 || new_end > host_len {
                return Err(EditOpError::BoundsExceeded(format!(
                    "Opening span [{:.3}, {:.3}]m exceeds host wall bounds [0.0, {:.3}]m",
                    new_start, new_end, host_len
                )));
            }

            let new_distance = match opening.reference_side {
                crate::modules::aec::engine::openings::OpeningReferenceSide::Start => new_start,
                crate::modules::aec::engine::openings::OpeningReferenceSide::Center => new_start + new_width * 0.5,
                crate::modules::aec::engine::openings::OpeningReferenceSide::End => new_end,
            };

            Ok(SemanticMutation::ResizeOpeningWidth {
                element: elem_ref,
                handle: opening.handle,
                new_width,
                new_distance,
                delta_s,
            })
        }

        // 5. Move Opening Along Host
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

            let mut seg_dir = Vector3::new(1.0, 0.0, 0.0);
            let mut acc_dist = 0.0;
            for w in axis_pts.windows(2) {
                let seg_dx = w[1].x_m - w[0].x_m;
                let seg_dy = w[1].y_m - w[0].y_m;
                let seg_len = (seg_dx * seg_dx + seg_dy * seg_dy).sqrt();
                if seg_len > 1e-6 {
                    seg_dir = Vector3::new(seg_dx / seg_len, seg_dy / seg_len, 0.0);
                }
                if opening.distance_along_axis <= acc_dist + seg_len + 1e-4 {
                    break;
                }
                acc_dist += seg_len;
            }

            let delta_s = if is_vertical_view {
                let dot = seg_dir.dot(viewpoint.axes.right);
                let sign = if dot < -1e-6 { -1.0 } else { 1.0 };
                view_delta.x_m * sign
            } else {
                view_delta.x_m * seg_dir.x + view_delta.y_m * seg_dir.y
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

        // 5. Slab Thickness (Bottom)
        (ModelEditAction::SetSlabThickness, _)
        | (ModelEditAction::ChangeParameter(ModelParameter::SlabThickness), _)
        | (_, Some(FeatureKey::SlabBottom)) => {
            if !is_vertical_view {
                return Err(EditOpError::DegenerateProjection(
                    "Cannot adjust slab thickness in a horizontal plan view".into(),
                ));
            }

            let slab = scene
                .slabs
                .iter()
                .find(|s| s.element_ref == elem_ref)
                .ok_or(EditOpError::ElementNotFound(elem_ref))?;

            let delta_z = view_delta.y_m * up_z.signum();
            // Dragging down (delta_z < 0) increases thickness
            let delta_thickness = -delta_z;

            let target_layer = slab
                .layers
                .iter()
                .find(|l| l.is_structural)
                .or_else(|| {
                    slab.layers
                        .iter()
                        .max_by(|a, b| a.thickness.total_cmp(&b.thickness))
                });
            let current_layer_thickness = target_layer.map(|l| l.thickness).unwrap_or(slab.total_thickness);
            let new_thickness = current_layer_thickness + delta_thickness;

            if new_thickness < 0.02 {
                return Err(EditOpError::BoundsExceeded(format!(
                    "New layer thickness {:.3}m is below minimum 0.02m",
                    new_thickness
                )));
            }
            if new_thickness > 5.0 {
                return Err(EditOpError::BoundsExceeded(format!(
                    "New layer thickness {:.3}m exceeds maximum 5.0m",
                    new_thickness
                )));
            }

            Ok(SemanticMutation::SetSlabThickness {
                element: elem_ref,
                handle: slab.handle,
                new_thickness,
                delta_thickness,
            })
        }

        // 6. Slab Elevation (Top)
        (ModelEditAction::SetSlabElevation, _)
        | (ModelEditAction::ChangeParameter(ModelParameter::SlabOffset), _)
        | (_, Some(FeatureKey::SlabTop)) => {
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

        #[allow(unreachable_patterns)]
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

        SemanticMutation::SetWallBaseOffset { new_base_z, new_height, delta_z, .. } => {
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

            wall.base_origin[2] = *new_base_z;
            wall.base_offset += *delta_z;
            wall.height = *new_height;
            wall.base_plane_id = None;

            let mut record = ExtendedDataRecord::new(AEC_APPID);
            record.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
            write_aec_record(doc, handle, record);

            // Invariance: keep opening absolute sill height unchanged (compensate sill_height)
            let mut affected_openings: Vec<Handle> = crate::modules::aec::engine::owner_index::children_of(doc, handle);
            for e in doc.entities() {
                let h = e.common().handle;
                if let Some(op) = crate::modules::aec::engine::opening_xdata::opening_from_entity(e, h) {
                    if op.host_wall == handle && !affected_openings.contains(&h) {
                        affected_openings.push(h);
                    }
                }
            }
            for op_h in affected_openings {
                if let Some(ent) = doc.get_entity(op_h) {
                    if let Some(mut opening) = crate::modules::aec::engine::opening_xdata::opening_from_entity(ent, op_h) {
                        opening.sill_height -= *delta_z;
                        let record = crate::modules::aec::engine::opening_xdata::opening_record(&opening);
                        write_aec_record(doc, op_h, record);
                    }
                }
            }
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

            let record = crate::modules::aec::engine::opening_xdata::opening_record(&opening);
            write_aec_record(doc, handle, record);
        }

        SemanticMutation::ResizeOpeningWidth { new_width, new_distance, .. } => {
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

            opening.width = *new_width;
            opening.distance_along_axis = *new_distance;

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

            let target_idx = slab
                .structural_layer_index()
                .or_else(|| {
                    slab.layers
                        .iter()
                        .enumerate()
                        .max_by(|(_, a), (_, b)| a.thickness.total_cmp(&b.thickness))
                        .map(|(idx, _)| idx)
                })
                .unwrap_or(0);

            if let Some(target_layer) = slab.layers.get_mut(target_idx) {
                target_layer.thickness = *new_thickness;
            }

            if slab.base_plane_id.is_none() {
                slab.base_origin[2] = slab.top_origin[2] - slab.total_thickness();
            }
            slab.base_plane_id = None;

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
            division_override: None,
            distance_along_axis: 2.0,
            width: 1.2,
            height: 1.5,
            sill_height: 0.9,
            spring_height: 0.0,
            shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
            hinge: crate::modules::aec::engine::opening_style::HingeSide::Left,
            swing_side: Default::default(),
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

    #[test]
    fn test_resolve_wall_base_drag_and_opening_invariance() {
        let wall_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let op_elem = ElementRef::new(ModelId::new(), ElementId::new());

        let opening = CollectedOpening {
            handle: Handle::new(20),
            element_ref: op_elem,
            xref_source: None,
            host_wall_handle: Handle::new(10),
            kind: OpeningKind::Window,
            style_id: None,
            division_override: None,
            distance_along_axis: 2.0,
            width: 1.2,
            height: 1.2,
            sill_height: 1.0,
            spring_height: 0.0,
            shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
            hinge: crate::modules::aec::engine::opening_style::HingeSide::Left,
            swing_side: Default::default(),
            reference_side: crate::modules::aec::engine::openings::OpeningReferenceSide::Start,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(2.0, 0.0, 1.6),
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
        let base_handle = EditHandle {
            id: "wall_base_1".into(),
            position: Point2::new(2.5, 0.0),
            action: ModelEditAction::ChangeParameter(ModelParameter::WallBaseOffset),
            direction_hint: Vector2::new(0.0, -1.0),
            target_feature: Some(FeatureKey::WallBase),
            source: SourceReference::new(wall_elem),
        };

        // 1. Drag base down by 0.3m (into foundation)
        let mutation = resolve_handle_drag(&base_handle, &vp, Point2::new(0.0, -0.3), &scene).unwrap();
        match mutation {
            SemanticMutation::SetWallBaseOffset { new_base_z, new_height, delta_z, .. } => {
                assert!((new_base_z - (-0.3)).abs() < 1e-4);
                assert!((new_height - 3.1).abs() < 1e-4);
                assert!((delta_z - (-0.3)).abs() < 1e-4);
            }
            _ => panic!("Expected SetWallBaseOffset"),
        }

        // 2. Drag base up by 1.5m (exceeds opening sill 1.0m) must be rejected
        let err = resolve_handle_drag(&base_handle, &vp, Point2::new(0.0, 1.5), &scene).unwrap_err();
        assert!(matches!(err, EditOpError::BoundsExceeded(_)));

        // 3. Test apply_semantic_mutation on document with wall and opening
        let mut doc = CadDocument::new();
        let mut wall_obj = Wall::new("W1", 2.8, 0);
        wall_obj.base_origin[2] = 0.0;
        let mut wall_ent = EntityType::LwPolyline(acadrust::entities::LwPolyline::new());
        wall_ent.common_mut().handle = Handle::new(10);
        let mut wall_rec = ExtendedDataRecord::new(AEC_APPID);
        wall_rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall_obj);
        wall_ent.common_mut().extended_data.add_record(wall_rec);
        doc.add_entity(wall_ent).unwrap();

        let op_obj = Opening::new(Handle::new(20), Handle::new(10), 2.0, 1.2, 1.2, 1.0, OpeningKind::Window);
        let mut op_ent = EntityType::Point(acadrust::entities::Point::from_coords(2.0, 0.0, 0.0));
        op_ent.common_mut().handle = Handle::new(20);
        let op_rec = crate::modules::aec::engine::opening_xdata::opening_record(&op_obj);
        op_ent.common_mut().extended_data.add_record(op_rec);
        doc.add_entity(op_ent).unwrap();
        crate::modules::aec::engine::owner_index::add_child(&mut doc, Handle::new(10), Handle::new(20));

        let res = apply_semantic_mutation(&mut doc, &mutation).unwrap();
        assert_eq!(res, Handle::new(10));

        let updated_wall_ent = doc.get_entity(Handle::new(10)).unwrap();
        let updated_wall = crate::modules::aec::engine::xdata::wall_from_entity(updated_wall_ent).unwrap();
        assert!((updated_wall.height - 3.1).abs() < 1e-4);
        assert!((updated_wall.base_origin[2] - (-0.3)).abs() < 1e-4);

        let updated_op_ent = doc.get_entity(Handle::new(20)).unwrap();
        let updated_op = crate::modules::aec::engine::opening_xdata::opening_from_entity(updated_op_ent, Handle::new(20)).unwrap();
        // Opening sill height compensated: 1.0 - (-0.3) = 1.3
        assert!((updated_op.sill_height - 1.3).abs() < 1e-4);
        // Absolute sill elevation is invariant: -0.3 + 1.3 == 1.0
        assert!(((updated_wall.base_origin[2] + updated_op.sill_height) - 1.0).abs() < 1e-4);
        // Absolute wall top is invariant: -0.3 + 3.1 == 2.8
        assert!(((updated_wall.base_origin[2] + updated_wall.height) - 2.8).abs() < 1e-4);
    }

    #[test]
    fn test_resolve_slab_bottom_drag_and_structural_core() {
        use crate::modules::aec::engine::slab::{Slab, SlabLayer};
        use crate::modules::aec::engine::views::scene_collector::CollectedSlabLayer;
        use crate::modules::aec::engine::wall_style::LayerFunction;

        let slab_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let collected_slab = crate::modules::aec::engine::views::scene_collector::CollectedSlab {
            handle: Handle::new(50),
            element_ref: slab_elem,
            xref_source: None,
            style_id: "S_Multi".into(),
            base_z: 2.5,
            total_thickness: 0.30,
            polygon: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0), Point2::new(10.0, 10.0)],
            layers: vec![
                CollectedSlabLayer {
                    material: "Estrich".into(),
                    thickness: 0.06,
                    vertical_offset: 0.0,
                    function: "Finish".into(),
                    hatch_override: None,
                    layer_id: uuid::Uuid::new_v4(),
                    component_id: crate::modules::aec::engine::views::ComponentId::new(),
                    is_structural: false,
                },
                CollectedSlabLayer {
                    material: "Stahlbeton".into(),
                    thickness: 0.20,
                    vertical_offset: 0.0,
                    function: "Structural".into(),
                    hatch_override: None,
                    layer_id: uuid::Uuid::new_v4(),
                    component_id: crate::modules::aec::engine::views::ComponentId::new(),
                    is_structural: true,
                },
                CollectedSlabLayer {
                    material: "Putz".into(),
                    thickness: 0.04,
                    vertical_offset: 0.0,
                    function: "Substrate".into(),
                    hatch_override: None,
                    layer_id: uuid::Uuid::new_v4(),
                    component_id: crate::modules::aec::engine::views::ComponentId::new(),
                    is_structural: false,
                },
            ],
            justification: crate::modules::aec::engine::slab::SlabJustification::Top,
            openings: vec![],
            phase: PlanPhase::Existing,
        };

        let mut scene = SceneGeometryCollector::default();
        scene.slabs.push(collected_slab);

        let vp = make_test_viewpoint();
        let bottom_handle = EditHandle {
            id: "slab_bottom_1".into(),
            position: Point2::new(5.0, 2.5),
            action: ModelEditAction::SetSlabThickness,
            direction_hint: Vector2::new(0.0, -1.0),
            target_feature: Some(FeatureKey::SlabBottom),
            source: SourceReference::new(slab_elem),
        };

        // 1. Drag bottom down by 0.10m: thickness of structural core increases from 0.20 to 0.30
        let mutation = resolve_handle_drag(&bottom_handle, &vp, Point2::new(0.0, -0.10), &scene).unwrap();
        match mutation {
            SemanticMutation::SetSlabThickness { new_thickness, delta_thickness, .. } => {
                assert!((new_thickness - 0.30).abs() < 1e-4);
                assert!((delta_thickness - 0.10).abs() < 1e-4);
            }
            _ => panic!("Expected SetSlabThickness"),
        }

        // 2. Test apply_semantic_mutation on multi-layer slab
        let mut doc = CadDocument::new();
        let mut slab = Slab::new("S_Multi", 0);
        slab.layers.push(SlabLayer::new("Estrich", 0.06, LayerFunction::Finish));
        slab.layers.push(SlabLayer::new("Stahlbeton", 0.20, LayerFunction::Structural));
        slab.layers.push(SlabLayer::new("Putz", 0.04, LayerFunction::Finish));
        slab.top_origin[2] = 2.80;
        slab.base_origin[2] = 2.50;

        let mut ent = EntityType::LwPolyline(acadrust::entities::LwPolyline::new());
        ent.common_mut().handle = Handle::new(50);
        let mut rec = ExtendedDataRecord::new(AEC_APPID);
        rec.values = crate::modules::aec::engine::slab_xdata::slab_record_for_slab(&slab);
        ent.common_mut().extended_data.add_record(rec);
        doc.add_entity(ent).unwrap();

        apply_semantic_mutation(&mut doc, &mutation).unwrap();

        let updated_ent = doc.get_entity(Handle::new(50)).unwrap();
        let updated_slab = crate::modules::aec::engine::slab_xdata::slab_from_entity(updated_ent).unwrap();
        assert!((updated_slab.layers[0].thickness - 0.06).abs() < 1e-4); // Estrich unchanged
        assert!((updated_slab.layers[1].thickness - 0.30).abs() < 1e-4); // Stahlbeton grew by 0.10
        assert!((updated_slab.layers[2].thickness - 0.04).abs() < 1e-4); // Putz unchanged
        assert!((updated_slab.total_thickness() - 0.40).abs() < 1e-4);
        assert!((updated_slab.base_origin[2] - 2.40).abs() < 1e-4); // Base lowered by 0.10
        assert!((updated_slab.top_origin[2] - 2.80).abs() < 1e-4); // Top remained unchanged
    }

    #[test]
    fn test_resolve_opening_move_drag_directional_correction() {
        let wall_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let op_elem = ElementRef::new(ModelId::new(), ElementId::new());

        let opening = CollectedOpening {
            handle: Handle::new(20),
            element_ref: op_elem,
            xref_source: None,
            host_wall_handle: Handle::new(10),
            kind: OpeningKind::Door,
            style_id: None,
            division_override: None,
            distance_along_axis: 4.0,
            width: 1.0,
            height: 2.1,
            sill_height: 0.0,
            spring_height: 0.0,
            shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
            hinge: crate::modules::aec::engine::opening_style::HingeSide::Left,
            swing_side: Default::default(),
            reference_side: crate::modules::aec::engine::openings::OpeningReferenceSide::Start,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(4.0, 0.0, 1.05),
            axis_span_local: (3.5, 4.5),
        };

        // Wall runs from (10.0, 0.0) to (0.0, 0.0) - direction is (-1.0, 0.0, 0.0)
        let wall = CollectedWall {
            handle: Handle::new(10),
            element_ref: wall_elem,
            xref_source: None,
            style_id: "W1".into(),
            height: 2.8,
            base_z: 0.0,
            top_z: 2.8,
            axis_points: vec![Point2::new(10.0, 0.0), Point2::new(0.0, 0.0)],
            layers: vec![],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: vec![opening],
            phase: PlanPhase::Existing,
        };

        let mut scene = SceneGeometryCollector::default();
        scene.walls.push(wall);

        // Section viewpoint looking with axes.right = (1.0, 0.0, 0.0)
        let vp = make_test_viewpoint();

        let move_handle = EditHandle {
            id: "op_jamb_test".into(),
            position: Point2::new(4.0, 1.0),
            action: ModelEditAction::MoveAlongHost {
                host: wall_elem,
                axis_world: super::super::Vector3::new(1.0, 0.0, 0.0),
            },
            direction_hint: Vector2::new(1.0, 0.0),
            target_feature: Some(FeatureKey::OpeningLeftJamb),
            source: SourceReference::new(op_elem),
        };

        // Dragging to the right (+0.5 in view-space) when wall direction is opposite to view.right:
        // dot(wall_dir, view.right) is -1.0, so delta_s = +0.5 * (-1.0) = -0.5.
        // new_distance = 4.0 - 0.5 = 3.5.
        // Distance along axis starts at x=10 and goes to x=0; moving towards +X in world (right) decreases distance along wall.
        let mutation = resolve_handle_drag(&move_handle, &vp, Point2::new(0.5, 0.0), &scene).unwrap();
        match mutation {
            SemanticMutation::MoveOpeningAlongHost { new_distance, delta_s, .. } => {
                assert!((new_distance - 3.5).abs() < 1e-4, "new_distance was {}", new_distance);
                assert!((delta_s - (-0.5)).abs() < 1e-4, "delta_s was {}", delta_s);
            }
            _ => panic!("Expected MoveOpeningAlongHost"),
        }

        // Test in horizontal FloorPlan view with angled wall (0, 0) -> (0, 10) (along Y)
        let mut floorplan_scene = SceneGeometryCollector::default();
        let fp_op_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let fp_wall_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let fp_opening = CollectedOpening {
            handle: Handle::new(30),
            element_ref: fp_op_elem,
            xref_source: None,
            host_wall_handle: Handle::new(25),
            kind: OpeningKind::Door,
            style_id: None,
            division_override: None,
            distance_along_axis: 2.0,
            width: 1.0,
            height: 2.1,
            sill_height: 0.0,
            spring_height: 0.0,
            shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
            hinge: crate::modules::aec::engine::opening_style::HingeSide::Left,
            swing_side: Default::default(),
            reference_side: crate::modules::aec::engine::openings::OpeningReferenceSide::Start,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(0.0, 2.0, 1.05),
            axis_span_local: (1.5, 2.5),
        };
        let fp_wall = CollectedWall {
            handle: Handle::new(25),
            element_ref: fp_wall_elem,
            xref_source: None,
            style_id: "W_Y".into(),
            height: 2.8,
            base_z: 0.0,
            top_z: 2.8,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(0.0, 10.0)],
            layers: vec![],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: vec![fp_opening],
            phase: PlanPhase::Existing,
        };
        floorplan_scene.walls.push(fp_wall);

        let fp_vp = Viewpoint {
            id: super::super::ViewpointId::new(),
            name: "Floor Plan".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::floor_plan(),
            kind: super::super::viewpoint::ViewKind::FloorPlan {
                range: super::super::viewpoint::PlanRange {
                    above_cut: crate::modules::aec::engine::views::NonNegativeMetres::new(1.0).unwrap(),
                    below_cut: crate::modules::aec::engine::views::NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: crate::modules::aec::engine::views::NonNegativeMetres::new(0.0).unwrap(),
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: super::super::ProjectionMapping::default(),
            crop: None,
        };

        let fp_handle = EditHandle {
            id: "fp_jamb".into(),
            position: Point2::new(0.0, 2.0),
            action: ModelEditAction::MoveAlongHost {
                host: fp_wall_elem,
                axis_world: super::super::Vector3::new(0.0, 1.0, 0.0),
            },
            direction_hint: Vector2::new(0.0, 1.0),
            target_feature: Some(FeatureKey::OpeningLeftJamb),
            source: SourceReference::new(fp_op_elem),
        };

        // Drag along Y by +1.0m on vertical wall
        let fp_mutation = resolve_handle_drag(&fp_handle, &fp_vp, Point2::new(0.0, 1.0), &floorplan_scene).unwrap();
        match fp_mutation {
            SemanticMutation::MoveOpeningAlongHost { new_distance, delta_s, .. } => {
                assert!((new_distance - 3.0).abs() < 1e-4, "new_distance was {}", new_distance);
                assert!((delta_s - 1.0).abs() < 1e-4, "delta_s was {}", delta_s);
            }
            _ => panic!("Expected MoveOpeningAlongHost"),
        }
    }

    #[test]
    fn test_resolve_opening_resize_stretch_left_and_right() {
        let wall_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let op_elem = ElementRef::new(ModelId::new(), ElementId::new());

        // Opening: distance = 2.0m, width = 1.0m, span = [2.0, 3.0]
        let opening = CollectedOpening {
            handle: Handle::new(20),
            element_ref: op_elem,
            xref_source: None,
            host_wall_handle: Handle::new(10),
            kind: OpeningKind::Door,
            style_id: None,
            division_override: None,
            distance_along_axis: 2.0,
            width: 1.0,
            height: 2.1,
            sill_height: 0.0,
            spring_height: 0.0,
            shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
            hinge: crate::modules::aec::engine::opening_style::HingeSide::Left,
            swing_side: Default::default(),
            reference_side: crate::modules::aec::engine::openings::OpeningReferenceSide::Start,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(2.5, 0.0, 1.05),
            axis_span_local: (2.0, 3.0),
        };

        let wall = CollectedWall {
            handle: Handle::new(10),
            element_ref: wall_elem,
            xref_source: None,
            style_id: "W1".into(),
            height: 2.8,
            base_z: 0.0,
            top_z: 2.8,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
            layers: vec![],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: vec![opening],
            phase: PlanPhase::Existing,
        };

        let mut scene = SceneGeometryCollector::default();
        scene.walls.push(wall);
        let vp = make_test_viewpoint();

        // 1. Drag left jamb to the left by -0.2m:
        // Right jamb remains fixed at 3.0m; left jamb moves to 1.8m.
        // new_width = 1.2m, new_distance = 1.8m.
        let left_handle = EditHandle {
            id: "left_jamb".into(),
            position: Point2::new(2.0, 0.0),
            action: ModelEditAction::ChangeParameter(ModelParameter::OpeningWidth),
            direction_hint: Vector2::new(1.0, 0.0),
            target_feature: Some(FeatureKey::OpeningLeftJamb),
            source: SourceReference::new(op_elem),
        };

        let mutation_left = resolve_handle_drag(&left_handle, &vp, Point2::new(-0.2, 0.0), &scene).unwrap();
        match mutation_left {
            SemanticMutation::ResizeOpeningWidth { new_width, new_distance, delta_s, .. } => {
                assert!((new_width - 1.2).abs() < 1e-4, "new_width was {}", new_width);
                assert!((new_distance - 1.8).abs() < 1e-4, "new_distance was {}", new_distance);
                assert!((delta_s - (-0.2)).abs() < 1e-4);
            }
            _ => panic!("Expected ResizeOpeningWidth"),
        }

        // 2. Drag right jamb to the right by +0.3m:
        // Left jamb remains fixed at 2.0m; right jamb moves to 3.3m.
        // new_width = 1.3m, new_distance = 2.0m.
        let right_handle = EditHandle {
            id: "right_jamb".into(),
            position: Point2::new(3.0, 0.0),
            action: ModelEditAction::ChangeParameter(ModelParameter::OpeningWidth),
            direction_hint: Vector2::new(1.0, 0.0),
            target_feature: Some(FeatureKey::OpeningRightJamb),
            source: SourceReference::new(op_elem),
        };

        let mutation_right = resolve_handle_drag(&right_handle, &vp, Point2::new(0.3, 0.0), &scene).unwrap();
        match mutation_right {
            SemanticMutation::ResizeOpeningWidth { new_width, new_distance, delta_s, .. } => {
                assert!((new_width - 1.3).abs() < 1e-4, "new_width was {}", new_width);
                assert!((new_distance - 2.0).abs() < 1e-4, "new_distance was {}", new_distance);
                assert!((delta_s - 0.3).abs() < 1e-4);
            }
            _ => panic!("Expected ResizeOpeningWidth"),
        }

        // 3. Shrinking below 0.20m returns BoundsExceeded
        let err = resolve_handle_drag(&right_handle, &vp, Point2::new(-0.9, 0.0), &scene).unwrap_err();
        assert!(matches!(err, EditOpError::BoundsExceeded(_)));

        // 4. Test apply_semantic_mutation on document
        let mut doc = CadDocument::new();
        let op_obj = Opening::new(Handle::new(20), Handle::new(10), 2.0, 1.0, 2.1, 0.0, OpeningKind::Door);
        let mut op_ent = EntityType::Point(acadrust::entities::Point::from_coords(2.0, 0.0, 0.0));
        op_ent.common_mut().handle = Handle::new(20);
        let op_rec = crate::modules::aec::engine::opening_xdata::opening_record(&op_obj);
        op_ent.common_mut().extended_data.add_record(op_rec);
        doc.add_entity(op_ent).unwrap();

        apply_semantic_mutation(&mut doc, &mutation_left).unwrap();

        let updated_op_ent = doc.get_entity(Handle::new(20)).unwrap();
        let updated_op = crate::modules::aec::engine::opening_xdata::opening_from_entity(updated_op_ent, Handle::new(20)).unwrap();
        assert!((updated_op.width - 1.2).abs() < 1e-4);
        assert!((updated_op.distance_along_axis - 1.8).abs() < 1e-4);
    }
}
