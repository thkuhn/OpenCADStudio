//! Plan-view display children for wall openings (`OPENING_REP`).
//!
//! Parametric generators bake world geometry in the opening's local box
//! (X along the host axis, Y across wall thickness).

use std::collections::HashMap;

use acadrust::entities::{LwPolyline, LwVertex};
use acadrust::types::Vector2;
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::modules::aec::engine::display_component::{ComponentRuleSet, OpeningComponentSlot};
use crate::modules::aec::engine::opening_shape::OpeningShape;
use crate::modules::aec::engine::construction_grid::{
    omits_threshold, resolve_grid, BarAxis, ConstructionGrid, ResolvedGrid, TrackEnd, TrackMeeting,
};
use crate::modules::aec::engine::opening_style::{
    apply_plan_visibility, construction_from_division, default_slots_for_kind,
    default_style_for_kind, effective_slots_for_plan, project_division_sized, BlockPlacementMode,
    DivisionGrid, DivisionLevel,
    ElevationFrameLevel, OpeningParts, OpeningPlanProfiles, OpeningRepresentationLevels,
    opening_component_applies, opening_component_should_render, OpeningLookRole, PlanFrameLevel, SwingGlyph,
    OpeningGenerator, OpeningStyle, SlotGeometry, LeafOperation, DEFAULT_FRAME_THICKNESS,
    DEFAULT_OPENING_ANGLE_DEG,
};
use crate::modules::aec::engine::section_profile::{resolve_section, ResolvedSection};
use crate::modules::aec::engine::opening_xdata::{
    opening_from_entity, openings_for_host_wall, write_opening_instance,
};
use crate::modules::aec::engine::openings::{
    point_and_tangent_at_distance, Opening, OpeningKind, SwingSide,
};
use crate::modules::aec::engine::wall_package::resolve_wall_package;
use crate::modules::aec::engine::xdata::{
    get_wall_vertices, wall_from_entity, write_aec_record, AEC_APPID,
};
use crate::modules::aec::engine::{self, StyleLibrary};
use crate::scene::model::hatch_model::{HatchModel, HatchPattern};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;

/// XDATA kind tag on opening display children.
pub const OPENING_REP_TAG: &str = "OPENING_REP";

/// Number of chords used to approximate a swing arc.
const SWING_CHORD_COUNT: usize = 16;

/// One baked 2D primitive in **local** opening coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct BakedPath {
    pub slot: OpeningComponentSlot,
    /// Optional role override for view-specific preview geometry.
    pub look_role: Option<OpeningLookRole>,
    pub points: Vec<(f64, f64)>,
    /// Unprojected wire points used by the opening style manager's 3D canvas.
    pub points_3d: Option<Vec<[f64; 3]>>,
    pub closed: bool,
    /// When true the closed path is also emitted as a solid hatch.
    pub filled: bool,
}

impl BakedPath {
    pub fn resolved_look_role(&self) -> Option<OpeningLookRole> {
        self.look_role
            .or_else(|| OpeningLookRole::for_slot(self.slot))
    }

    fn with_look_role(mut self, role: OpeningLookRole) -> Self {
        self.look_role = Some(role);
        self
    }
}

/// Parameters that drive generator bake (instance box + absolute profile).
#[derive(Debug, Clone, Copy)]
pub struct OpeningBakeParams {
    pub width: f64,
    pub height: f64,
    pub sill_height: f64,
    pub thickness: f64,
    pub frame_thickness: f64,
    pub cross_axis_offset: f64,
    pub wall_y_min: f64,
    pub wall_y_max: f64,
    pub hinge: LeafOperation,
    pub swing_side: SwingSide,
    pub shape: OpeningShape,
    pub spring_height: f64,
    pub opening_angle_deg: f64,
    pub kind: OpeningKind,
}

impl OpeningBakeParams {
    pub fn from_opening(opening: &Opening, thickness: f64, style: Option<&OpeningStyle>) -> Self {
        let (frame_thickness, angle, swing_side) = if let Some(style) = style {
            (
                style.parts.fixed_frame.face_width,
                style.opening_angle_deg,
                style.swing_side,
            )
        } else if opening.kind == OpeningKind::Breakthrough {
            (0.0, 0.0, opening.swing_side)
        } else {
            (
                DEFAULT_FRAME_THICKNESS,
                DEFAULT_OPENING_ANGLE_DEG,
                opening.swing_side,
            )
        };
        let ht = thickness * 0.5;
        Self {
            width: opening.width,
            height: opening.height,
            sill_height: opening.sill_height,
            thickness,
            frame_thickness,
            cross_axis_offset: opening.cross_axis_offset,
            wall_y_min: -ht,
            wall_y_max: ht,
            hinge: opening.hinge,
            swing_side,
            shape: opening.shape,
            spring_height: opening.spring_height,
            opening_angle_deg: angle,
            kind: opening.kind,
        }
    }

    pub fn with_wall_bounds(mut self, y_min: f64, y_max: f64) -> Self {
        self.wall_y_min = y_min;
        self.wall_y_max = y_max;
        self
    }
}

mod bake_plan;
pub use bake_plan::*;
mod bake_generators;
pub use bake_generators::*;
mod build_3d;
pub use build_3d::*;
mod emit_3d;
pub use emit_3d::*;
mod world_tags;
pub use world_tags::*;
mod regen;
pub use regen::*;
#[cfg(test)]
mod tests;
