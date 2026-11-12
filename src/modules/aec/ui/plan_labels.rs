//! Labels, enums and label parsing for the DisplayConfig (plan) manager.
//! Pure state/conversion helpers, kept apart from the widget code in
//! `aec_plan_manager.rs`.

use crate::modules::aec::engine::display_component::{RepresentationMode, WallComponentKind};
use crate::modules::aec::engine::plan_view::{PlanningStage, ViewType};
use crate::tr;

pub(super) fn planning_stage_label(stage: PlanningStage) -> String {
    match stage {
        PlanningStage::Permit => tr!("aec", "planning-stage-permit"),
        PlanningStage::Design => tr!("aec", "planning-stage-design"),
        PlanningStage::Execution => tr!("aec", "planning-stage-execution"),
    }
}

pub(super) fn view_type_label(view_type: ViewType) -> String {
    match view_type {
        ViewType::FloorPlan => tr!("aec", "view-type-floor-plan"),
        ViewType::Section => tr!("aec", "view-type-section"),
        ViewType::Elevation => tr!("aec", "view-type-elevation"),
    }
}

pub(super) const PLANNING_STAGES: [PlanningStage; 3] = [
    PlanningStage::Permit,
    PlanningStage::Design,
    PlanningStage::Execution,
];
pub(super) const VIEW_TYPES: [ViewType; 3] = [ViewType::FloorPlan, ViewType::Section, ViewType::Elevation];

/// Parses a planning stage label (as produced by [`planning_stage_label`]) back
/// into a [`PlanningStage`], defaulting to `Design` for anything unrecognized.
pub fn planning_stage_from_label(label: &str) -> PlanningStage {
    if label == planning_stage_label(PlanningStage::Permit) {
        PlanningStage::Permit
    } else if label == planning_stage_label(PlanningStage::Execution) {
        PlanningStage::Execution
    } else {
        PlanningStage::Design
    }
}

/// Parses a view-type label (as produced by [`view_type_label`]) back into
/// a [`ViewType`], defaulting to `FloorPlan` for anything unrecognized.
pub fn view_type_from_label(label: &str) -> ViewType {
    if label == view_type_label(ViewType::Section) {
        ViewType::Section
    } else if label == view_type_label(ViewType::Elevation) {
        ViewType::Elevation
    } else {
        ViewType::FloorPlan
    }
}

pub(super) fn kind_label(kind: WallComponentKind) -> String {
    match kind {
        WallComponentKind::Axis => tr!("aec", "kind-axis"),
        WallComponentKind::Layers2D => tr!("aec", "kind-layers-2d"),
        WallComponentKind::LayerHatch2D => tr!("aec", "kind-layer-hatch-2d"),
        WallComponentKind::Contour2D => tr!("aec", "kind-contour-2d"),
        WallComponentKind::ContourHatch2D => tr!("aec", "kind-contour-hatch-2d"),
        WallComponentKind::Layers3D => tr!("aec", "kind-layers-3d"),
        WallComponentKind::SurfaceStyle3D => tr!("aec", "kind-surface-3d"),
    }
}

pub(super) fn representation_label(mode: RepresentationMode) -> String {
    match mode {
        RepresentationMode::TwoD => "2D".to_string(),
        RepresentationMode::ThreeD => "3D".to_string(),
        RepresentationMode::All => tr!("aec", "representation-all"),
    }
}

pub(super) const REPRESENTATION_MODES: [RepresentationMode; 3] = [
    RepresentationMode::TwoD,
    RepresentationMode::ThreeD,
    RepresentationMode::All,
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum HatchAngleRelChoice {
    Inherit,
    Relative,
    Absolute,
}

pub(super) const HATCH_ANGLE_REL_CHOICES: [HatchAngleRelChoice; 3] = [
    HatchAngleRelChoice::Inherit,
    HatchAngleRelChoice::Relative,
    HatchAngleRelChoice::Absolute,
];

pub(super) fn hatch_angle_rel_choice(value: Option<bool>) -> HatchAngleRelChoice {
    match value {
        None => HatchAngleRelChoice::Inherit,
        Some(true) => HatchAngleRelChoice::Relative,
        Some(false) => HatchAngleRelChoice::Absolute,
    }
}

pub(super) fn hatch_angle_rel_value(choice: HatchAngleRelChoice) -> Option<bool> {
    match choice {
        HatchAngleRelChoice::Inherit => None,
        HatchAngleRelChoice::Relative => Some(true),
        HatchAngleRelChoice::Absolute => Some(false),
    }
}

pub(super) fn hatch_angle_relative_label(value: Option<bool>) -> String {
    match hatch_angle_rel_choice(value) {
        HatchAngleRelChoice::Inherit => tr!("aec", "inherit"),
        HatchAngleRelChoice::Relative => tr!("aec", "relative"),
        HatchAngleRelChoice::Absolute => tr!("aec", "absolute"),
    }
}

#[allow(dead_code)]
pub(super) fn hatch_angle_relative_from_label(label: &str) -> Option<bool> {
    match label {
        "Relativ" => Some(true),
        "Absolut" => Some(false),
        _ => None,
    }
}

#[allow(dead_code)]
pub(super) fn representation_from_label(label: &str) -> RepresentationMode {
    match label {
        "2D" => RepresentationMode::TwoD,
        "3D" => RepresentationMode::ThreeD,
        _ => RepresentationMode::All,
    }
}
