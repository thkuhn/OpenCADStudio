//! Opening styles (window / door / breakthrough) with absolute part dimensions
//! and a shared division grid.

use crate::modules::aec::engine::display_component::OpeningComponentSlot;
use crate::modules::aec::engine::opening_shape::OpeningShape;
use crate::modules::aec::engine::openings::OpeningKind;

use crate::modules::aec::engine::style::{resolve_chain, Style, StyleError, StyleId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Seed style id for the standard window.
pub const SEED_WINDOW_STYLE_ID: &str = "style_window_standard";
/// Seed style id for the double-leaf window.
pub const SEED_WINDOW_DOUBLE_STYLE_ID: &str = "style_window_double";
/// Seed style id for the arched window.
pub const SEED_WINDOW_ARCH_STYLE_ID: &str = "style_window_arch";
/// Seed style id for the round circular window.
pub const SEED_WINDOW_CIRCLE_STYLE_ID: &str = "style_window_circle";
/// Seed style id for the floor-to-ceiling French window.
pub const SEED_WINDOW_FLOOR_STYLE_ID: &str = "style_window_floor";
/// Seed style id for the standard door.
pub const SEED_DOOR_STYLE_ID: &str = "style_door_standard";
/// Seed style id for the right-hinged door.
pub const SEED_DOOR_RIGHT_STYLE_ID: &str = "style_door_right";
/// Seed style id for the double-leaf entrance door.
pub const SEED_DOOR_DOUBLE_STYLE_ID: &str = "style_door_double";
/// Seed style id for the standard breakthrough.
pub const SEED_BREAKTHROUGH_STYLE_ID: &str = "style_breakthrough_standard";
/// Seed style id for the round pipe/installation breakthrough.
pub const SEED_BREAKTHROUGH_PIPE_STYLE_ID: &str = "style_breakthrough_pipe";

/// Default frame / profile thickness in drawing units (metres).
pub const DEFAULT_FRAME_THICKNESS: f64 = 0.06;
/// Default swing angle for door generators.
pub const DEFAULT_OPENING_ANGLE_DEG: f64 = 90.0;

/// Hinge / strike side of a door or window leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum HingeSide {
    #[default]
    Left,
    Right,
}

impl HingeSide {
    pub fn as_str(self) -> &'static str {
        match self {
            HingeSide::Left => "Left",
            HingeSide::Right => "Right",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "Right" | "right" | "R" | "r" => HingeSide::Right,
            _ => HingeSide::Left,
        }
    }

    pub fn mirrored(self) -> Self {
        match self {
            HingeSide::Left => HingeSide::Right,
            HingeSide::Right => HingeSide::Left,
        }
    }
}

/// Placement mode for a reusable block in an opening display profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum BlockPlacementMode {
    /// Places the block 1:1 on the left reveal edge and horizontally mirrored 1:1 on the right reveal edge.
    #[default]
    JambPair,
    /// Stretches the block between the reveals along the opening width.
    StretchToFit,
    /// Centers the block unscaled in the opening width.
    CenterAnchor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PlanFrameLevel {
    Off,
    SingleLine,
    DoubleLine,
    #[default]
    Profile,
}

impl PlanFrameLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            PlanFrameLevel::Off => "Off",
            PlanFrameLevel::SingleLine => "SingleLine",
            PlanFrameLevel::DoubleLine => "DoubleLine",
            PlanFrameLevel::Profile => "Profile",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Off" => Some(Self::Off),
            "SingleLine" => Some(Self::SingleLine),
            "DoubleLine" => Some(Self::DoubleLine),
            "Profile" => Some(Self::Profile),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SwingGlyph {
    #[default]
    Off,
    Arc,
    Triangle,
}

impl SwingGlyph {
    pub fn as_str(self) -> &'static str {
        match self {
            SwingGlyph::Off => "Off",
            SwingGlyph::Arc => "Arc",
            SwingGlyph::Triangle => "Triangle",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Off" => Some(Self::Off),
            "Arc" => Some(Self::Arc),
            "Triangle" => Some(Self::Triangle),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ElevationFrameLevel {
    Off,
    Outline,
    FixedFrame,
    #[default]
    FixedAndLeaf,
}

impl ElevationFrameLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            ElevationFrameLevel::Off => "Off",
            ElevationFrameLevel::Outline => "Outline",
            ElevationFrameLevel::FixedFrame => "FixedFrame",
            ElevationFrameLevel::FixedAndLeaf => "FixedAndLeaf",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Off" => Some(Self::Off),
            "Outline" => Some(Self::Outline),
            "FixedFrame" => Some(Self::FixedFrame),
            "FixedAndLeaf" => Some(Self::FixedAndLeaf),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum DivisionLevel {
    #[default]
    Off,
    Posts,
    PostsAndMuntins,
}

impl DivisionLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            DivisionLevel::Off => "Off",
            DivisionLevel::Posts => "Posts",
            DivisionLevel::PostsAndMuntins => "PostsAndMuntins",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Off" => Some(Self::Off),
            "Posts" => Some(Self::Posts),
            "PostsAndMuntins" => Some(Self::PostsAndMuntins),
            _ => None,
        }
    }
}

/// Section roles the plan can switch independently of the floor-plan frame.
///
/// Absent plan values stay at this suggestion. Defaults are on, so a plan that
/// does not name a role does not hide it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionRoleSwitches {
    pub frame: bool,
    pub exterior_sill: bool,
    pub interior_board: bool,
    pub shutter_box: bool,
    pub threshold: bool,
    pub lintel: bool,
    pub glazing: bool,
}

impl Default for SectionRoleSwitches {
    fn default() -> Self {
        Self {
            frame: true,
            exterior_sill: true,
            interior_board: true,
            shutter_box: true,
            threshold: true,
            lintel: true,
            glazing: true,
        }
    }
}

/// Explicit plan levels. `None` means the style suggestion wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OpeningLevelOverride {
    pub plan_frame: Option<PlanFrameLevel>,
    pub swing: Option<SwingGlyph>,
    pub elevation_frame: Option<ElevationFrameLevel>,
    pub divisions: Option<DivisionLevel>,
    pub section: SectionRoleOverride,
}

/// Per-role section switches. `None` keeps the style suggestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SectionRoleOverride {
    pub frame: Option<bool>,
    pub exterior_sill: Option<bool>,
    pub interior_board: Option<bool>,
    pub shutter_box: Option<bool>,
    pub threshold: Option<bool>,
    pub lintel: Option<bool>,
    pub glazing: Option<bool>,
}

impl OpeningLevelOverride {
    pub fn is_explicit(self) -> bool {
        self.plan_frame.is_some()
            || self.swing.is_some()
            || self.elevation_frame.is_some()
            || self.divisions.is_some()
            || self.section.is_explicit()
    }

    pub fn coarse() -> Self {
        Self {
            plan_frame: Some(PlanFrameLevel::SingleLine),
            swing: Some(SwingGlyph::Off),
            elevation_frame: Some(ElevationFrameLevel::Outline),
            divisions: Some(DivisionLevel::Off),
            section: SectionRoleOverride::all(false),
        }
    }

    pub fn normal() -> Self {
        Self {
            plan_frame: Some(PlanFrameLevel::DoubleLine),
            swing: Some(SwingGlyph::Arc),
            elevation_frame: Some(ElevationFrameLevel::FixedFrame),
            divisions: Some(DivisionLevel::Posts),
            section: SectionRoleOverride::default(),
        }
    }

    pub fn detail() -> Self {
        Self {
            plan_frame: Some(PlanFrameLevel::Profile),
            swing: Some(SwingGlyph::Arc),
            elevation_frame: Some(ElevationFrameLevel::FixedAndLeaf),
            divisions: Some(DivisionLevel::PostsAndMuntins),
            section: SectionRoleOverride::all(true),
        }
    }

    /// Wall cut stays with the host. Swing and glazing are hidden; the lintel may stay.
    pub fn structural() -> Self {
        Self {
            plan_frame: Some(PlanFrameLevel::Off),
            swing: Some(SwingGlyph::Off),
            elevation_frame: Some(ElevationFrameLevel::Off),
            divisions: Some(DivisionLevel::Off),
            section: SectionRoleOverride {
                frame: Some(false),
                exterior_sill: Some(false),
                interior_board: Some(false),
                shutter_box: Some(false),
                threshold: Some(false),
                lintel: Some(true),
                glazing: Some(false),
            },
        }
    }

    pub fn elevation() -> Self {
        Self {
            plan_frame: Some(PlanFrameLevel::Off),
            swing: Some(SwingGlyph::Off),
            elevation_frame: Some(ElevationFrameLevel::FixedAndLeaf),
            divisions: Some(DivisionLevel::PostsAndMuntins),
            section: SectionRoleOverride::default(),
        }
    }

    pub fn model_3d() -> Self {
        Self {
            plan_frame: Some(PlanFrameLevel::Off),
            swing: Some(SwingGlyph::Off),
            elevation_frame: Some(ElevationFrameLevel::Off),
            divisions: Some(DivisionLevel::Off),
            section: SectionRoleOverride::default(),
        }
    }
}

impl SectionRoleOverride {
    pub fn is_explicit(self) -> bool {
        self.frame.is_some()
            || self.exterior_sill.is_some()
            || self.interior_board.is_some()
            || self.shutter_box.is_some()
            || self.threshold.is_some()
            || self.lintel.is_some()
            || self.glazing.is_some()
    }

    pub fn all(value: bool) -> Self {
        Self {
            frame: Some(value),
            exterior_sill: Some(value),
            interior_board: Some(value),
            shutter_box: Some(value),
            threshold: Some(value),
            lintel: Some(value),
            glazing: Some(value),
        }
    }

    pub fn resolve(self, suggestion: SectionRoleSwitches) -> SectionRoleSwitches {
        SectionRoleSwitches {
            frame: self.frame.unwrap_or(suggestion.frame),
            exterior_sill: self.exterior_sill.unwrap_or(suggestion.exterior_sill),
            interior_board: self.interior_board.unwrap_or(suggestion.interior_board),
            shutter_box: self.shutter_box.unwrap_or(suggestion.shutter_box),
            threshold: self.threshold.unwrap_or(suggestion.threshold),
            lintel: self.lintel.unwrap_or(suggestion.lintel),
            glazing: self.glazing.unwrap_or(suggestion.glazing),
        }
    }
}

impl OpeningRepresentationLevels {
    pub fn suggested_for(kind: OpeningKind) -> Self {
        Self {
            plan_frame: PlanFrameLevel::Profile,
            swing: if kind == OpeningKind::Door {
                SwingGlyph::Arc
            } else {
                SwingGlyph::Off
            },
            elevation_frame: ElevationFrameLevel::FixedAndLeaf,
            divisions: DivisionLevel::Off,
        }
    }

    pub fn resolve(self, plan: OpeningLevelOverride) -> Self {
        Self {
            plan_frame: plan.plan_frame.unwrap_or(self.plan_frame),
            swing: plan.swing.unwrap_or(self.swing),
            elevation_frame: plan.elevation_frame.unwrap_or(self.elevation_frame),
            divisions: plan.divisions.unwrap_or(self.divisions),
        }
    }
}

/// Known plan presets. Unknown ids return `None` so the style suggestion stays.
pub fn preset_opening_levels(preset: &str) -> Option<OpeningLevelOverride> {
    match preset {
        "coarse" | "entwurf-100" => Some(OpeningLevelOverride::coarse()),
        "normal" => Some(OpeningLevelOverride::normal()),
        "detail" | "werkplan-50" => Some(OpeningLevelOverride::detail()),
        "structural" | "statik-50" => Some(OpeningLevelOverride::structural()),
        "elevation" | "fassade-100" => Some(OpeningLevelOverride::elevation()),
        "model-3d" => Some(OpeningLevelOverride::model_3d()),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OpeningRepresentationLevels {
    pub plan_frame: PlanFrameLevel,
    pub swing: SwingGlyph,
    pub elevation_frame: ElevationFrameLevel,
    pub divisions: DivisionLevel,
}

impl Default for OpeningRepresentationLevels {
    fn default() -> Self {
        Self {
            plan_frame: PlanFrameLevel::Profile,
            swing: SwingGlyph::Off,
            elevation_frame: ElevationFrameLevel::FixedAndLeaf,
            divisions: DivisionLevel::Off,
        }
    }
}

impl BlockPlacementMode {
    pub fn as_str(self) -> &'static str {
        match self {
            BlockPlacementMode::JambPair => "JambPair",
            BlockPlacementMode::StretchToFit => "StretchToFit",
            BlockPlacementMode::CenterAnchor => "CenterAnchor",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "StretchToFit" => BlockPlacementMode::StretchToFit,
            "CenterAnchor" => BlockPlacementMode::CenterAnchor,
            _ => BlockPlacementMode::JambPair,
        }
    }
}

/// Parametric plan-symbol generator for one display slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum OpeningGenerator {
    #[default]
    None,
    FrameRect,
    DoorFrame,
    LeafLine,
    SwingArc,
    SillLines,
    /// Breakthrough: two diagonals.
    Cross,
    /// Breakthrough: one diagonal plus a fill of the opening area.
    DiagonalFill,
    GlazingLine,
    ThresholdLine,
    OpeningLabel,

    // Ansichts-Generatoren
    ElevationFrameRect,
    ElevationFrameArch,
    ElevationMuntinsSingle,
    ElevationMuntinsDouble,
    ElevationSwingTriangle,
    ElevationSillLine,

    // 3D-Generatoren
    SolidFrame,
    SolidLeaf,
    SolidGlazing,
}

impl OpeningGenerator {
    pub fn as_str(self) -> &'static str {
        match self {
            OpeningGenerator::None => "None",
            OpeningGenerator::FrameRect => "FrameRect",
            OpeningGenerator::DoorFrame => "DoorFrame",
            OpeningGenerator::LeafLine => "LeafLine",
            OpeningGenerator::SwingArc => "SwingArc",
            OpeningGenerator::SillLines => "SillLines",
            OpeningGenerator::Cross => "Cross",
            OpeningGenerator::DiagonalFill => "DiagonalFill",
            OpeningGenerator::GlazingLine => "GlazingLine",
            OpeningGenerator::ThresholdLine => "ThresholdLine",
            OpeningGenerator::OpeningLabel => "OpeningLabel",
            OpeningGenerator::ElevationFrameRect => "ElevationFrameRect",
            OpeningGenerator::ElevationFrameArch => "ElevationFrameArch",
            OpeningGenerator::ElevationMuntinsSingle => "ElevationMuntinsSingle",
            OpeningGenerator::ElevationMuntinsDouble => "ElevationMuntinsDouble",
            OpeningGenerator::ElevationSwingTriangle => "ElevationSwingTriangle",
            OpeningGenerator::ElevationSillLine => "ElevationSillLine",
            OpeningGenerator::SolidFrame => "SolidFrame",
            OpeningGenerator::SolidLeaf => "SolidLeaf",
            OpeningGenerator::SolidGlazing => "SolidGlazing",
        }
    }

    pub fn catalogue() -> &'static [OpeningGenerator] {
        &[
            OpeningGenerator::None,
            OpeningGenerator::FrameRect,
            OpeningGenerator::DoorFrame,
            OpeningGenerator::LeafLine,
            OpeningGenerator::SwingArc,
            OpeningGenerator::SillLines,
            OpeningGenerator::Cross,
            OpeningGenerator::DiagonalFill,
            OpeningGenerator::GlazingLine,
            OpeningGenerator::ThresholdLine,
            OpeningGenerator::OpeningLabel,
            OpeningGenerator::ElevationFrameRect,
            OpeningGenerator::ElevationFrameArch,
            OpeningGenerator::ElevationMuntinsSingle,
            OpeningGenerator::ElevationMuntinsDouble,
            OpeningGenerator::ElevationSwingTriangle,
            OpeningGenerator::ElevationSillLine,
            OpeningGenerator::SolidFrame,
            OpeningGenerator::SolidLeaf,
            OpeningGenerator::SolidGlazing,
        ]
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "FrameRect" => OpeningGenerator::FrameRect,
            "DoorFrame" => OpeningGenerator::DoorFrame,
            "LeafLine" => OpeningGenerator::LeafLine,
            "SwingArc" => OpeningGenerator::SwingArc,
            "SillLines" => OpeningGenerator::SillLines,
            "Cross" => OpeningGenerator::Cross,
            "DiagonalFill" => OpeningGenerator::DiagonalFill,
            "GlazingLine" => OpeningGenerator::GlazingLine,
            "ThresholdLine" => OpeningGenerator::ThresholdLine,
            "OpeningLabel" => OpeningGenerator::OpeningLabel,
            "ElevationFrameRect" => OpeningGenerator::ElevationFrameRect,
            "ElevationFrameArch" => OpeningGenerator::ElevationFrameArch,
            "ElevationMuntinsSingle" => OpeningGenerator::ElevationMuntinsSingle,
            "ElevationMuntinsDouble" => OpeningGenerator::ElevationMuntinsDouble,
            "ElevationSwingTriangle" => OpeningGenerator::ElevationSwingTriangle,
            "ElevationSillLine" => OpeningGenerator::ElevationSillLine,
            "SolidFrame" => OpeningGenerator::SolidFrame,
            "SolidLeaf" => OpeningGenerator::SolidLeaf,
            "SolidGlazing" => OpeningGenerator::SolidGlazing,
            _ => OpeningGenerator::None,
        }
    }
}

/// Geometry source for a legacy plan-level opening display override.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum SlotGeometry {
    Generator(OpeningGenerator),
    Block {
        block_name: String,
        #[serde(default)]
        placement: BlockPlacementMode,
    },
}

impl Default for SlotGeometry {
    fn default() -> Self {
        SlotGeometry::Generator(OpeningGenerator::None)
    }
}

/// Absolute dimensions of the fixed frame / jamb.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FixedFrameDimensions {
    #[serde(default = "default_opening_component_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub profile_id: Option<String>,
    pub face_width: f64,
    pub wall_depth: f64,
    pub axis_offset: f64,
}

impl Default for FixedFrameDimensions {
    fn default() -> Self {
        Self {
            enabled: true,
            profile_id: None,
            face_width: DEFAULT_FRAME_THICKNESS,
            wall_depth: DEFAULT_FRAME_THICKNESS,
            axis_offset: 0.0,
        }
    }
}

const fn default_opening_component_enabled() -> bool {
    true
}

/// Absolute dimensions of an operable leaf frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LeafFrameDimensions {
    #[serde(default = "default_opening_component_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub profile_id: Option<String>,
    pub face_width: f64,
    pub depth: f64,
    pub rebate_overlap: f64,
}

impl Default for LeafFrameDimensions {
    fn default() -> Self {
        Self {
            enabled: true,
            profile_id: None,
            face_width: 0.04,
            depth: 0.06,
            rebate_overlap: 0.01,
        }
    }
}

/// Absolute glazing dimensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlazingDimensions {
    #[serde(default = "default_opening_component_enabled")]
    pub enabled: bool,
    pub thickness: f64,
    pub inset: f64,
}

impl Default for GlazingDimensions {
    fn default() -> Self {
        Self {
            enabled: true,
            thickness: 0.02,
            inset: 0.01,
        }
    }
}

/// Absolute dimensions shared by an exterior sill or interior board.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SillDimensions {
    #[serde(default = "default_opening_component_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub profile_id: Option<String>,
    pub thickness: f64,
    pub overhang: f64,
    pub upstand: f64,
}

impl Default for SillDimensions {
    fn default() -> Self {
        Self {
            enabled: true,
            profile_id: None,
            thickness: 0.0,
            overhang: 0.0,
            upstand: 0.0,
        }
    }
}

/// Absolute dimensions of a door threshold.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThresholdDimensions {
    #[serde(default = "default_opening_component_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub profile_id: Option<String>,
    pub height: f64,
    pub width: f64,
}

impl Default for ThresholdDimensions {
    fn default() -> Self {
        Self {
            enabled: true,
            profile_id: None,
            height: 0.0,
            width: 0.0,
        }
    }
}

/// Absolute dimensions and position of a shutter box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShutterBoxDimensions {
    #[serde(default = "default_opening_component_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub profile_id: Option<String>,
    pub height: f64,
    pub depth: f64,
    pub seat_above_frame: f64,
    pub installation_depth: f64,
}

impl Default for ShutterBoxDimensions {
    fn default() -> Self {
        Self {
            enabled: true,
            profile_id: None,
            height: 0.0,
            depth: 0.0,
            seat_above_frame: 0.0,
            installation_depth: 0.0,
        }
    }
}

/// Absolute dimensions and material of a host lintel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LintelDimensions {
    #[serde(default = "default_opening_component_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub profile_id: Option<String>,
    pub height: f64,
    pub bearing_left: f64,
    pub bearing_right: f64,
    pub material_id: Option<String>,
}

impl Default for LintelDimensions {
    fn default() -> Self {
        Self {
            enabled: true,
            profile_id: None,
            height: 0.0,
            bearing_left: 0.0,
            bearing_right: 0.0,
            material_id: None,
        }
    }
}

/// Optional symbol source for one view family. The block does not own sizes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FamilySymbol {
    pub block_name: String,
    pub placement: FamilyPlacement,
}

impl Default for FamilySymbol {
    fn default() -> Self {
        Self {
            block_name: String::new(),
            placement: FamilyPlacement::Evaluated,
        }
    }
}

/// How a family block is placed. Stretching is not a placement: it would
/// scale the absolute frame width with the opening.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum FamilyPlacement {
    #[default]
    Evaluated,
    JambPair,
}

impl FamilyPlacement {
    pub fn as_str(self) -> &'static str {
        match self {
            FamilyPlacement::Evaluated => "Evaluated",
            FamilyPlacement::JambPair => "JambPair",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "JambPair" => FamilyPlacement::JambPair,
            _ => FamilyPlacement::Evaluated,
        }
    }
}

/// One optional block name per 2D family. 3D stays a generator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct OpeningFamilySymbols {
    pub plan: Option<FamilySymbol>,
    pub section: Option<FamilySymbol>,
    pub elevation: Option<FamilySymbol>,
}

impl OpeningFamilySymbols {
    pub fn for_family(&self, family: OpeningFamily) -> Option<&FamilySymbol> {
        match family {
            OpeningFamily::Plan => self.plan.as_ref(),
            OpeningFamily::Section => self.section.as_ref(),
            OpeningFamily::Elevation => self.elevation.as_ref(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpeningFamily {
    Plan,
    Section,
    Elevation,
}

/// Opening component sizes in drawing units; none scale with instance width.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct OpeningParts {
    pub fixed_frame: FixedFrameDimensions,
    pub leaf_frame: LeafFrameDimensions,
    pub glazing: GlazingDimensions,
    pub exterior_sill: SillDimensions,
    pub interior_board: SillDimensions,
    pub threshold: ThresholdDimensions,
    pub shutter_box: ShutterBoxDimensions,
    pub lintel: LintelDimensions,
}

pub fn opening_component_should_render(
    parts: &OpeningParts,
    kind: OpeningKind,
    role: OpeningLookRole,
) -> bool {
    if !opening_component_applies(kind, role) {
        return false;
    }
    match role {
        OpeningLookRole::FixedFrame => parts.fixed_frame.enabled,
        OpeningLookRole::Division => true,
        OpeningLookRole::Leaf | OpeningLookRole::Swing => {
            parts.leaf_frame.enabled
                && parts.leaf_frame.face_width > 0.0
                && parts.leaf_frame.depth > 0.0
        }
        OpeningLookRole::Glazing => parts.glazing.enabled && parts.glazing.thickness > 0.0,
        OpeningLookRole::ExteriorSill => {
            parts.exterior_sill.enabled && parts.exterior_sill.thickness > 0.0
        }
        OpeningLookRole::InteriorBoard => {
            parts.interior_board.enabled && parts.interior_board.thickness > 0.0
        }
        OpeningLookRole::Threshold => {
            parts.threshold.enabled && parts.threshold.width > 0.0 && parts.threshold.height > 0.0
        }
        OpeningLookRole::ShutterBox => {
            parts.shutter_box.enabled
                && parts.shutter_box.height > 0.0
                && parts.shutter_box.depth > 0.0
        }
        OpeningLookRole::Lintel => parts.lintel.enabled && parts.lintel.height > 0.0,
    }
}

/// A shared optical role for one or more opening display slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OpeningLookRole {
    FixedFrame,
    Leaf,
    Glazing,
    Swing,
    Division,
    ExteriorSill,
    InteriorBoard,
    Threshold,
    ShutterBox,
    Lintel,
}

impl OpeningLookRole {
    pub fn all() -> &'static [OpeningLookRole] {
        &[
            OpeningLookRole::FixedFrame,
            OpeningLookRole::Leaf,
            OpeningLookRole::Glazing,
            OpeningLookRole::Swing,
            OpeningLookRole::Division,
            OpeningLookRole::ExteriorSill,
            OpeningLookRole::InteriorBoard,
            OpeningLookRole::Threshold,
            OpeningLookRole::ShutterBox,
            OpeningLookRole::Lintel,
        ]
    }

    pub fn key(self) -> &'static str {
        match self {
            OpeningLookRole::FixedFrame => "FixedFrame",
            OpeningLookRole::Leaf => "Leaf",
            OpeningLookRole::Glazing => "Glazing",
            OpeningLookRole::Swing => "Swing",
            OpeningLookRole::Division => "Division",
            OpeningLookRole::ExteriorSill => "ExteriorSill",
            OpeningLookRole::InteriorBoard => "InteriorBoard",
            OpeningLookRole::Threshold => "Threshold",
            OpeningLookRole::ShutterBox => "ShutterBox",
            OpeningLookRole::Lintel => "Lintel",
        }
    }

    pub fn for_slot(slot: OpeningComponentSlot) -> Option<OpeningLookRole> {
        match slot {
            OpeningComponentSlot::Frame2D
            | OpeningComponentSlot::ElevationContour2D
            | OpeningComponentSlot::Frame3D => Some(OpeningLookRole::FixedFrame),
            OpeningComponentSlot::Leaf2D | OpeningComponentSlot::Leaf3D => {
                Some(OpeningLookRole::Leaf)
            }
            OpeningComponentSlot::Glazing2D | OpeningComponentSlot::Glazing3D => {
                Some(OpeningLookRole::Glazing)
            }
            OpeningComponentSlot::Swing2D | OpeningComponentSlot::ElevationSwing2D => {
                Some(OpeningLookRole::Swing)
            }
            OpeningComponentSlot::ElevationMuntins2D => Some(OpeningLookRole::Division),
            OpeningComponentSlot::Sill2D | OpeningComponentSlot::ElevationSill2D => {
                Some(OpeningLookRole::ExteriorSill)
            }
            OpeningComponentSlot::Threshold2D => Some(OpeningLookRole::Threshold),
            OpeningComponentSlot::HostCut2D
            | OpeningComponentSlot::BreakthroughSymbol2D
            | OpeningComponentSlot::OpeningLabel2D
            | OpeningComponentSlot::Mark2D
            | OpeningComponentSlot::Solid3D => None,
        }
    }
}

pub fn opening_component_applies(kind: OpeningKind, role: OpeningLookRole) -> bool {
    match role {
        OpeningLookRole::FixedFrame => matches!(kind, OpeningKind::Window | OpeningKind::Door),
        OpeningLookRole::Leaf | OpeningLookRole::Glazing | OpeningLookRole::Swing => {
            matches!(kind, OpeningKind::Window | OpeningKind::Door)
        }
        OpeningLookRole::Division => matches!(kind, OpeningKind::Window | OpeningKind::Door),
        OpeningLookRole::ExteriorSill | OpeningLookRole::ShutterBox => {
            kind == OpeningKind::Window
        }
        OpeningLookRole::InteriorBoard => matches!(kind, OpeningKind::Window | OpeningKind::Door),
        OpeningLookRole::Threshold => kind == OpeningKind::Door,
        OpeningLookRole::Lintel => true,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OpeningEditorGroup {
    #[default]
    FixedFrame,
    Leaf,
    Glazing,
    ExteriorSill,
    InteriorBoard,
    Threshold,
    ShutterBox,
    Lintel,
    Divisions,
    Symbol,
    Ratings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpeningProfilePart {
    FixedFrame,
    LeafFrame,
    Division,
    ExteriorSill,
    InteriorBoard,
    Threshold,
    ShutterBox,
    Lintel,
}

impl OpeningEditorGroup {
    pub fn all() -> &'static [OpeningEditorGroup] {
        &[
            OpeningEditorGroup::FixedFrame,
            OpeningEditorGroup::Leaf,
            OpeningEditorGroup::Glazing,
            OpeningEditorGroup::ExteriorSill,
            OpeningEditorGroup::InteriorBoard,
            OpeningEditorGroup::Threshold,
            OpeningEditorGroup::ShutterBox,
            OpeningEditorGroup::Lintel,
            OpeningEditorGroup::Divisions,
            OpeningEditorGroup::Symbol,
            OpeningEditorGroup::Ratings,
        ]
    }

    pub fn applies_to(self, kind: OpeningKind) -> bool {
        match self {
            OpeningEditorGroup::FixedFrame => {
                opening_component_applies(kind, OpeningLookRole::FixedFrame)
            }
            OpeningEditorGroup::Leaf => opening_component_applies(kind, OpeningLookRole::Leaf),
            OpeningEditorGroup::Glazing => {
                opening_component_applies(kind, OpeningLookRole::Glazing)
            }
            OpeningEditorGroup::ExteriorSill => {
                opening_component_applies(kind, OpeningLookRole::ExteriorSill)
            }
            OpeningEditorGroup::InteriorBoard => {
                opening_component_applies(kind, OpeningLookRole::InteriorBoard)
            }
            OpeningEditorGroup::Threshold => {
                opening_component_applies(kind, OpeningLookRole::Threshold)
            }
            OpeningEditorGroup::ShutterBox => {
                opening_component_applies(kind, OpeningLookRole::ShutterBox)
            }
            OpeningEditorGroup::Lintel => {
                opening_component_applies(kind, OpeningLookRole::Lintel)
            }
            OpeningEditorGroup::Divisions => {
                opening_component_applies(kind, OpeningLookRole::Division)
            }
            OpeningEditorGroup::Symbol => true,
            OpeningEditorGroup::Ratings => false,
        }
    }
}

/// Optional line overrides and material reference for one opening role.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OpeningComponentLook {
    pub line_type: Option<String>,
    pub line_color: Option<acadrust::types::Color>,
    pub material_id: Option<String>,
}

/// Optical attributes for every opening component role.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OpeningLooks {
    pub fixed_frame: OpeningComponentLook,
    pub leaf: OpeningComponentLook,
    pub glazing: OpeningComponentLook,
    pub swing: OpeningComponentLook,
    pub division: OpeningComponentLook,
    pub exterior_sill: OpeningComponentLook,
    pub interior_board: OpeningComponentLook,
    pub threshold: OpeningComponentLook,
    pub shutter_box: OpeningComponentLook,
    pub lintel: OpeningComponentLook,
}

impl OpeningLooks {
    pub fn get(&self, role: OpeningLookRole) -> &OpeningComponentLook {
        match role {
            OpeningLookRole::FixedFrame => &self.fixed_frame,
            OpeningLookRole::Leaf => &self.leaf,
            OpeningLookRole::Glazing => &self.glazing,
            OpeningLookRole::Swing => &self.swing,
            OpeningLookRole::Division => &self.division,
            OpeningLookRole::ExteriorSill => &self.exterior_sill,
            OpeningLookRole::InteriorBoard => &self.interior_board,
            OpeningLookRole::Threshold => &self.threshold,
            OpeningLookRole::ShutterBox => &self.shutter_box,
            OpeningLookRole::Lintel => &self.lintel,
        }
    }

    pub fn get_mut(&mut self, role: OpeningLookRole) -> &mut OpeningComponentLook {
        match role {
            OpeningLookRole::FixedFrame => &mut self.fixed_frame,
            OpeningLookRole::Leaf => &mut self.leaf,
            OpeningLookRole::Glazing => &mut self.glazing,
            OpeningLookRole::Swing => &mut self.swing,
            OpeningLookRole::Division => &mut self.division,
            OpeningLookRole::ExteriorSill => &mut self.exterior_sill,
            OpeningLookRole::InteriorBoard => &mut self.interior_board,
            OpeningLookRole::Threshold => &mut self.threshold,
            OpeningLookRole::ShutterBox => &mut self.shutter_box,
            OpeningLookRole::Lintel => &mut self.lintel,
        }
    }
}

impl OpeningStyle {
    /// Returns the role material, keeping the previous lintel field as a read-only fallback.
    pub fn material_id_for_look(&self, role: OpeningLookRole) -> Option<&str> {
        let look_material = self
            .looks
            .get(role)
            .material_id
            .as_deref()
            .filter(|id| !id.trim().is_empty());
        look_material.or_else(|| {
            (role == OpeningLookRole::Lintel)
                .then_some(self.parts.lintel.material_id.as_deref())
                .flatten()
                .filter(|id| !id.trim().is_empty())
        })
    }
}

/// What a cell of an opening division represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum OpeningCellKind {
    #[default]
    Glass,
    Opaque,
    DoorLeaf,
    Fixed,
}

impl OpeningCellKind {
    pub fn as_str(self) -> &'static str {
        match self {
            OpeningCellKind::Glass => "Glass",
            OpeningCellKind::Opaque => "Opaque",
            OpeningCellKind::DoorLeaf => "DoorLeaf",
            OpeningCellKind::Fixed => "Fixed",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "Opaque" => OpeningCellKind::Opaque,
            "DoorLeaf" => OpeningCellKind::DoorLeaf,
            "Fixed" => OpeningCellKind::Fixed,
            _ => OpeningCellKind::Glass,
        }
    }

    pub fn catalogue() -> &'static [OpeningCellKind] {
        &[
            OpeningCellKind::Glass,
            OpeningCellKind::Opaque,
            OpeningCellKind::DoorLeaf,
            OpeningCellKind::Fixed,
        ]
    }

    pub fn permits_hinge(self) -> bool {
        matches!(self, OpeningCellKind::Glass | OpeningCellKind::DoorLeaf)
    }

    fn for_opening_kind(kind: OpeningKind) -> Self {
        match kind {
            OpeningKind::Window => OpeningCellKind::Glass,
            OpeningKind::Door => OpeningCellKind::DoorLeaf,
            OpeningKind::Breakthrough => OpeningCellKind::Opaque,
        }
    }
}

/// One heterogeneous cell in the opening division grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OpeningCell {
    pub kind: OpeningCellKind,
    pub hinge: Option<HingeSide>,
    pub sill: f64,
}

impl Default for OpeningCell {
    fn default() -> Self {
        Self {
            kind: OpeningCellKind::Glass,
            hinge: None,
            sill: 0.0,
        }
    }
}

/// A division template shared by plan, section, elevation and 3D.
///
/// `vertical` contains column-width fractions summing to one. `horizontal`
/// contains transom heights above each cell's sill. An entirely empty grid is
/// an implicit single cell whose kind is derived from `OpeningKind`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DivisionGrid {
    pub vertical: Vec<f64>,
    pub horizontal: Vec<f64>,
    pub vertical_muntins: Vec<f64>,
    pub horizontal_muntins: Vec<f64>,
    #[serde(default)]
    pub profile_id: Option<String>,
    /// Cells in row-major order (bottom row first, then left to right).
    pub cells: Vec<OpeningCell>,
}

impl DivisionGrid {
    pub fn is_implicit_single_cell(&self) -> bool {
        self.vertical.is_empty()
            && self.horizontal.is_empty()
            && self.cells.is_empty()
    }

    pub fn resolved_cells(&self, kind: OpeningKind) -> Vec<OpeningCell> {
        if self.is_implicit_single_cell() {
            vec![OpeningCell {
                kind: OpeningCellKind::for_opening_kind(kind),
                ..OpeningCell::default()
            }]
        } else {
            self.cells.clone()
        }
    }

    pub fn validate(&self, opening_height: f64) -> Result<(), String> {
        if !opening_height.is_finite() || opening_height <= 0.0 {
            return Err("opening height must be positive and finite".to_string());
        }
        let column_count = if self.vertical.is_empty() {
            1
        } else {
            if self
                .vertical
                .iter()
                .any(|fraction| !fraction.is_finite() || *fraction <= 0.0)
            {
                return Err("column fractions must be positive and finite".to_string());
            }
            let total: f64 = self.vertical.iter().sum();
            if (total - 1.0).abs() > 1e-6 {
                return Err("column fractions must sum to one".to_string());
            }
            self.vertical.len()
        };

        if self.horizontal.iter().any(|height| {
            !height.is_finite() || *height <= 0.0 || *height >= opening_height
        }) || self.horizontal.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err("transom heights must be finite, ascending and inside the opening".to_string());
        }
        if self.vertical_muntins.iter().any(|fraction| {
            !fraction.is_finite() || *fraction <= 0.0 || *fraction >= 1.0
        }) || self
            .vertical_muntins
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err("muntin fractions must be finite, ascending and inside each cell".to_string());
        }
        if self.horizontal_muntins.iter().any(|height| {
            !height.is_finite() || *height <= 0.0
        }) || self
            .horizontal_muntins
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err("muntin heights must be finite, positive and ascending".to_string());
        }
        if self.is_implicit_single_cell() {
            if self
                .horizontal_muntins
                .last()
                .is_some_and(|muntin| *muntin >= opening_height)
            {
                return Err("horizontal muntin exceeds the opening".to_string());
            }
            return Ok(());
        }
        if self.cells.is_empty() {
            return Err("a divided grid must define its cells".to_string());
        }
        let expected_cells = column_count * (self.horizontal.len() + 1);
        if self.cells.len() != expected_cells {
            return Err("cell count must match the grid divisions".to_string());
        }
        for cell in &self.cells {
            if !cell.sill.is_finite() || cell.sill < 0.0 || cell.sill >= opening_height {
                return Err("cell sill must be inside the opening".to_string());
            }
            if cell.hinge.is_some() && !cell.kind.permits_hinge() {
                return Err("only operable cells may have a hinge".to_string());
            }
            if self
                .horizontal
                .last()
                .is_some_and(|transom| cell.sill + transom >= opening_height)
            {
                return Err("transom height plus cell sill exceeds the opening".to_string());
            }
        }
        let rows = self.horizontal.len() + 1;
        for (index, cell) in self.cells.iter().enumerate() {
            let row = index / column_count;
            let bottom = if row == 0 {
                cell.sill
            } else {
                cell.sill + self.horizontal[row - 1]
            };
            let top = if row < self.horizontal.len() {
                cell.sill + self.horizontal[row]
            } else {
                opening_height
            };
            let cell_height = top - bottom;
            if self
                .horizontal_muntins
                .last()
                .is_some_and(|muntin| *muntin >= cell_height)
            {
                return Err("horizontal muntin exceeds a cell height".to_string());
            }
            debug_assert!(row < rows);
        }
        Ok(())
    }

    pub fn layout(
        &self,
        width: f64,
        height: f64,
        kind: OpeningKind,
    ) -> Result<Vec<DivisionCellLayout>, String> {
        if !width.is_finite() || width <= 0.0 {
            return Err("opening width must be positive and finite".to_string());
        }
        self.validate(height)?;
        let fractions = if self.vertical.is_empty() {
            vec![1.0]
        } else {
            self.vertical.clone()
        };
        let cells = self.resolved_cells(kind);
        let columns = fractions.len();
        let rows = self.horizontal.len() + 1;
        let mut layout = Vec::with_capacity(cells.len());
        let mut left = -width * 0.5;
        for column in 0..columns {
            let right = left + width * fractions[column];
            for row in 0..rows {
                let cell = cells[row * columns + column].clone();
                let bottom = if row == 0 {
                    cell.sill
                } else {
                    cell.sill + self.horizontal[row - 1]
                };
                let top = if row < self.horizontal.len() {
                    cell.sill + self.horizontal[row]
                } else {
                    height
                };
                layout.push(DivisionCellLayout {
                    column,
                    row,
                    left,
                    right,
                    bottom,
                    top,
                    cell,
                });
            }
            left = right;
        }
        layout.sort_by_key(|cell| (cell.row, cell.column));
        Ok(layout)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DivisionCellLayout {
    pub column: usize,
    pub row: usize,
    pub left: f64,
    pub right: f64,
    pub bottom: f64,
    pub top: f64,
    pub cell: OpeningCell,
}

/// A named, reusable division template for a standard size/configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DivisionPreset {
    pub name: String,
    pub grid: DivisionGrid,
}

/// A library style for windows, doors, or breakthroughs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningStyle {
    pub style: Style,
    pub kind: OpeningKind,
    pub default_width: f64,
    pub default_height: f64,
    pub default_sill: f64,
    pub hinge: HingeSide,
    #[serde(default)]
    pub parts: OpeningParts,
    #[serde(default)]
    pub looks: OpeningLooks,
    #[serde(default)]
    pub grid: DivisionGrid,
    #[serde(default)]
    pub division_presets: Vec<DivisionPreset>,
    pub opening_angle_deg: f64,
    pub shape: OpeningShape,
    /// Arch Kämpfer above the sill. Unused for other shapes.
    pub spring_height: f64,
    /// Used when the plan does not set an explicit level.
    #[serde(default = "default_suggested_levels")]
    pub suggested_levels: OpeningRepresentationLevels,
    #[serde(default)]
    pub suggested_section: SectionRoleSwitches,
    /// Optional evaluated block per 2D family. Empty names are not a binding.
    #[serde(default)]
    pub family_symbols: OpeningFamilySymbols,
}

fn default_suggested_levels() -> OpeningRepresentationLevels {
    OpeningRepresentationLevels::suggested_for(OpeningKind::Window)
}

/// Opening-slot geometry/visibility override for one plan type or scale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OpeningDisplayProfile {
    #[serde(default)]
    pub slots: HashMap<OpeningComponentSlot, SlotGeometry>,
    /// Slot key → visible. Absent keys stay visible.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub visibility: HashMap<String, bool>,
}

impl OpeningStyle {
    pub fn standard_window() -> Self {
        Self {
            style: Style {
                id: SEED_WINDOW_STYLE_ID.to_string(),
                name: "Standardfenster".to_string(),
                category: crate::modules::aec::engine::style_common::CATEGORY_OPENING.to_string(),
                origin: Default::default(),
                parent: None,
            },
            kind: OpeningKind::Window,
            default_width: crate::modules::aec::engine::openings::DEFAULT_WINDOW_WIDTH,
            default_height: crate::modules::aec::engine::openings::DEFAULT_WINDOW_HEIGHT,
            default_sill: crate::modules::aec::engine::openings::DEFAULT_WINDOW_SILL,
            hinge: HingeSide::Left,
            parts: OpeningParts::default(),
            looks: OpeningLooks::default(),
            grid: DivisionGrid::default(),
            division_presets: Vec::new(),
            opening_angle_deg: DEFAULT_OPENING_ANGLE_DEG,
            shape: OpeningShape::Rectangle,
            spring_height: 0.0,
            suggested_levels: OpeningRepresentationLevels::suggested_for(OpeningKind::Window),
            suggested_section: SectionRoleSwitches::default(),
            family_symbols: OpeningFamilySymbols::default(),
        }
    }

    pub fn window_double() -> Self {
        Self {
            style: Style {
                id: SEED_WINDOW_DOUBLE_STYLE_ID.to_string(),
                name: "Zweiflügelfenster".to_string(),
                category: crate::modules::aec::engine::style_common::CATEGORY_OPENING.to_string(),
                origin: Default::default(),
                parent: None,
            },
            kind: OpeningKind::Window,
            default_width: 1.60,
            default_height: 1.40,
            default_sill: 0.90,
            hinge: HingeSide::Left,
            parts: OpeningParts::default(),
            looks: OpeningLooks::default(),
            grid: DivisionGrid {
                vertical: vec![0.5, 0.5],
                cells: vec![
                    OpeningCell {
                        kind: OpeningCellKind::Glass,
                        hinge: Some(HingeSide::Left),
                        sill: 0.0,
                    },
                    OpeningCell {
                        kind: OpeningCellKind::Glass,
                        hinge: Some(HingeSide::Right),
                        sill: 0.0,
                    },
                ],
                ..DivisionGrid::default()
            },
            division_presets: Vec::new(),
            opening_angle_deg: DEFAULT_OPENING_ANGLE_DEG,
            shape: OpeningShape::Rectangle,
            spring_height: 0.0,
            suggested_levels: OpeningRepresentationLevels::suggested_for(OpeningKind::Window),
            suggested_section: SectionRoleSwitches::default(),
            family_symbols: OpeningFamilySymbols::default(),
        }
    }

    pub fn window_arch() -> Self {
        Self {
            style: Style {
                id: SEED_WINDOW_ARCH_STYLE_ID.to_string(),
                name: "Rundbogenfenster".to_string(),
                category: crate::modules::aec::engine::style_common::CATEGORY_OPENING.to_string(),
                origin: Default::default(),
                parent: None,
            },
            kind: OpeningKind::Window,
            default_width: 1.00,
            default_height: 1.80,
            default_sill: 0.80,
            hinge: HingeSide::Left,
            parts: OpeningParts::default(),
            looks: OpeningLooks::default(),
            grid: DivisionGrid::default(),
            division_presets: Vec::new(),
            opening_angle_deg: DEFAULT_OPENING_ANGLE_DEG,
            shape: OpeningShape::Arch,
            spring_height: 1.30,
            suggested_levels: OpeningRepresentationLevels::suggested_for(OpeningKind::Window),
            suggested_section: SectionRoleSwitches::default(),
            family_symbols: OpeningFamilySymbols::default(),
        }
    }

    pub fn window_circle() -> Self {
        Self {
            style: Style {
                id: SEED_WINDOW_CIRCLE_STYLE_ID.to_string(),
                name: "Rundfenster".to_string(),
                category: crate::modules::aec::engine::style_common::CATEGORY_OPENING.to_string(),
                origin: Default::default(),
                parent: None,
            },
            kind: OpeningKind::Window,
            default_width: 0.90,
            default_height: 0.90,
            default_sill: 1.20,
            hinge: HingeSide::Left,
            parts: OpeningParts::default(),
            looks: OpeningLooks::default(),
            grid: DivisionGrid::default(),
            division_presets: Vec::new(),
            opening_angle_deg: 0.0,
            shape: OpeningShape::Circle,
            spring_height: 0.0,
            suggested_levels: OpeningRepresentationLevels::suggested_for(OpeningKind::Window),
            suggested_section: SectionRoleSwitches::default(),
            family_symbols: OpeningFamilySymbols::default(),
        }
    }

    pub fn window_floor() -> Self {
        Self {
            style: Style {
                id: SEED_WINDOW_FLOOR_STYLE_ID.to_string(),
                name: "Bodentiefes Fenster".to_string(),
                category: crate::modules::aec::engine::style_common::CATEGORY_OPENING.to_string(),
                origin: Default::default(),
                parent: None,
            },
            kind: OpeningKind::Window,
            default_width: 1.00,
            default_height: 2.20,
            default_sill: 0.00,
            hinge: HingeSide::Left,
            parts: OpeningParts::default(),
            looks: OpeningLooks::default(),
            grid: DivisionGrid::default(),
            division_presets: Vec::new(),
            opening_angle_deg: DEFAULT_OPENING_ANGLE_DEG,
            shape: OpeningShape::Rectangle,
            spring_height: 0.0,
            suggested_levels: OpeningRepresentationLevels::suggested_for(OpeningKind::Window),
            suggested_section: SectionRoleSwitches::default(),
            family_symbols: OpeningFamilySymbols::default(),
        }
    }

    pub fn standard_door() -> Self {
        Self {
            style: Style {
                id: SEED_DOOR_STYLE_ID.to_string(),
                name: "Standardtür".to_string(),
                category: crate::modules::aec::engine::style_common::CATEGORY_OPENING.to_string(),
                origin: Default::default(),
                parent: None,
            },
            kind: OpeningKind::Door,
            default_width: crate::modules::aec::engine::openings::DEFAULT_DOOR_WIDTH,
            default_height: crate::modules::aec::engine::openings::DEFAULT_DOOR_HEIGHT,
            default_sill: crate::modules::aec::engine::openings::DEFAULT_DOOR_SILL,
            hinge: HingeSide::Left,
            parts: OpeningParts::default(),
            looks: OpeningLooks::default(),
            grid: DivisionGrid::default(),
            division_presets: Vec::new(),
            opening_angle_deg: DEFAULT_OPENING_ANGLE_DEG,
            shape: OpeningShape::Rectangle,
            spring_height: 0.0,
            suggested_levels: OpeningRepresentationLevels::suggested_for(OpeningKind::Door),
            suggested_section: SectionRoleSwitches::default(),
            family_symbols: OpeningFamilySymbols::default(),
        }
    }

    pub fn door_right() -> Self {
        let mut door = Self::standard_door();
        door.style.id = SEED_DOOR_RIGHT_STYLE_ID.to_string();
        door.style.name = "Innentür rechts".to_string();
        door.hinge = HingeSide::Right;
        door
    }

    pub fn door_double() -> Self {
        Self {
            style: Style {
                id: SEED_DOOR_DOUBLE_STYLE_ID.to_string(),
                name: "Zweiflügelige Eingangstür".to_string(),
                category: crate::modules::aec::engine::style_common::CATEGORY_OPENING.to_string(),
                origin: Default::default(),
                parent: None,
            },
            kind: OpeningKind::Door,
            default_width: 1.80,
            default_height: 2.20,
            default_sill: 0.00,
            hinge: HingeSide::Left,
            parts: OpeningParts::default(),
            looks: OpeningLooks::default(),
            grid: DivisionGrid {
                vertical: vec![0.5, 0.5],
                cells: vec![
                    OpeningCell {
                        kind: OpeningCellKind::DoorLeaf,
                        hinge: Some(HingeSide::Left),
                        sill: 0.0,
                    },
                    OpeningCell {
                        kind: OpeningCellKind::DoorLeaf,
                        hinge: Some(HingeSide::Right),
                        sill: 0.0,
                    },
                ],
                ..DivisionGrid::default()
            },
            division_presets: Vec::new(),
            opening_angle_deg: DEFAULT_OPENING_ANGLE_DEG,
            shape: OpeningShape::Rectangle,
            spring_height: 0.0,
            suggested_levels: OpeningRepresentationLevels::suggested_for(OpeningKind::Door),
            suggested_section: SectionRoleSwitches::default(),
            family_symbols: OpeningFamilySymbols::default(),
        }
    }

    pub fn standard_breakthrough() -> Self {
        Self {
            style: Style {
                id: SEED_BREAKTHROUGH_STYLE_ID.to_string(),
                name: "Standarddurchbruch".to_string(),
                category: crate::modules::aec::engine::style_common::CATEGORY_OPENING.to_string(),
                origin: Default::default(),
                parent: None,
            },
            kind: OpeningKind::Breakthrough,
            default_width: crate::modules::aec::engine::openings::DEFAULT_BREAKTHROUGH_WIDTH,
            default_height: crate::modules::aec::engine::openings::DEFAULT_BREAKTHROUGH_HEIGHT,
            default_sill: crate::modules::aec::engine::openings::DEFAULT_BREAKTHROUGH_SILL,
            hinge: HingeSide::Left,
            parts: OpeningParts {
                fixed_frame: FixedFrameDimensions {
                    enabled: true,
                    profile_id: None,
                    face_width: 0.0,
                    wall_depth: 0.0,
                    axis_offset: 0.0,
                },
                ..OpeningParts::default()
            },
            looks: OpeningLooks::default(),
            grid: DivisionGrid::default(),
            division_presets: Vec::new(),
            opening_angle_deg: 0.0,
            shape: OpeningShape::Rectangle,
            spring_height: 0.0,
            suggested_levels: OpeningRepresentationLevels::suggested_for(OpeningKind::Breakthrough),
            suggested_section: SectionRoleSwitches::default(),
            family_symbols: OpeningFamilySymbols::default(),
        }
    }

    pub fn breakthrough_pipe() -> Self {
        Self {
            style: Style {
                id: SEED_BREAKTHROUGH_PIPE_STYLE_ID.to_string(),
                name: "Installationsdurchbruch rund".to_string(),
                category: crate::modules::aec::engine::style_common::CATEGORY_OPENING.to_string(),
                origin: Default::default(),
                parent: None,
            },
            kind: OpeningKind::Breakthrough,
            default_width: 0.40,
            default_height: 0.40,
            default_sill: 1.80,
            hinge: HingeSide::Left,
            parts: OpeningParts {
                fixed_frame: FixedFrameDimensions {
                    enabled: true,
                    profile_id: None,
                    face_width: 0.0,
                    wall_depth: 0.0,
                    axis_offset: 0.0,
                },
                ..OpeningParts::default()
            },
            looks: OpeningLooks::default(),
            grid: DivisionGrid::default(),
            division_presets: Vec::new(),
            opening_angle_deg: 0.0,
            shape: OpeningShape::Circle,
            spring_height: 0.0,
            suggested_levels: OpeningRepresentationLevels::suggested_for(OpeningKind::Breakthrough),
            suggested_section: SectionRoleSwitches::default(),
            family_symbols: OpeningFamilySymbols::default(),
        }
    }
}

/// Seed opening styles shipped with the default library.
pub fn seed_opening_styles() -> Vec<OpeningStyle> {
    vec![
        OpeningStyle::standard_window(),
        OpeningStyle::window_double(),
        OpeningStyle::window_arch(),
        OpeningStyle::window_circle(),
        OpeningStyle::window_floor(),
        OpeningStyle::standard_door(),
        OpeningStyle::door_right(),
        OpeningStyle::door_double(),
        OpeningStyle::standard_breakthrough(),
        OpeningStyle::breakthrough_pipe(),
    ]
}

/// Copy numeric/style identity defaults onto an instance (not plane ids).
pub fn apply_style_defaults(opening: &mut crate::modules::aec::engine::openings::Opening, style: &OpeningStyle) {
    opening.style_id = Some(style.style.id.clone());
    opening.kind = style.kind;
    opening.width = style.default_width;
    opening.height = style.default_height;
    opening.sill_height = style.default_sill;
    opening.hinge = style.hinge;
    opening.shape = style.shape;
    opening.spring_height = style.spring_height;
    let (w, h) = opening.shape.lock_size(opening.width, opening.height, true);
    opening.width = w;
    opening.height = h;
    if opening.shape == OpeningShape::Arch && opening.spring_height <= 1e-12 {
        opening.spring_height = OpeningShape::default_spring_height(opening.width, opening.height);
    }
}

/// Seed style id for `kind` (used when placing with a library).
pub fn seed_style_id_for_kind(kind: OpeningKind) -> &'static str {
    match kind {
        OpeningKind::Window => SEED_WINDOW_STYLE_ID,
        OpeningKind::Door => SEED_DOOR_STYLE_ID,
        OpeningKind::Breakthrough => SEED_BREAKTHROUGH_STYLE_ID,
    }
}

pub fn default_style_for_kind(kind: OpeningKind) -> OpeningStyle {
    match kind {
        OpeningKind::Window => OpeningStyle::standard_window(),
        OpeningKind::Door => OpeningStyle::standard_door(),
        OpeningKind::Breakthrough => OpeningStyle::standard_breakthrough(),
    }
}

/// Temporary kind-based display fallback used until all family bakers consume
/// the style parts and division grid.
pub fn default_slots_for_kind(
    kind: OpeningKind,
) -> HashMap<OpeningComponentSlot, SlotGeometry> {
    let mut slots = HashMap::new();
    let mut add = |slot, generator| {
        slots.insert(slot, SlotGeometry::Generator(generator));
    };
    match kind {
        OpeningKind::Window => {
            add(OpeningComponentSlot::Frame2D, OpeningGenerator::FrameRect);
            add(OpeningComponentSlot::Leaf2D, OpeningGenerator::LeafLine);
            add(OpeningComponentSlot::Glazing2D, OpeningGenerator::GlazingLine);
            add(OpeningComponentSlot::Sill2D, OpeningGenerator::SillLines);
            add(
                OpeningComponentSlot::OpeningLabel2D,
                OpeningGenerator::OpeningLabel,
            );
            add(
                OpeningComponentSlot::ElevationContour2D,
                OpeningGenerator::ElevationFrameRect,
            );
            add(
                OpeningComponentSlot::ElevationMuntins2D,
                OpeningGenerator::ElevationMuntinsSingle,
            );
            add(
                OpeningComponentSlot::ElevationSwing2D,
                OpeningGenerator::ElevationSwingTriangle,
            );
            add(
                OpeningComponentSlot::ElevationSill2D,
                OpeningGenerator::ElevationSillLine,
            );
            add(OpeningComponentSlot::Frame3D, OpeningGenerator::SolidFrame);
            add(OpeningComponentSlot::Leaf3D, OpeningGenerator::SolidLeaf);
            add(OpeningComponentSlot::Glazing3D, OpeningGenerator::SolidGlazing);
        }
        OpeningKind::Door => {
            add(OpeningComponentSlot::Frame2D, OpeningGenerator::FrameRect);
            add(OpeningComponentSlot::Leaf2D, OpeningGenerator::LeafLine);
            add(OpeningComponentSlot::Swing2D, OpeningGenerator::SwingArc);
            add(
                OpeningComponentSlot::Threshold2D,
                OpeningGenerator::ThresholdLine,
            );
            add(
                OpeningComponentSlot::OpeningLabel2D,
                OpeningGenerator::OpeningLabel,
            );
            add(
                OpeningComponentSlot::ElevationContour2D,
                OpeningGenerator::ElevationFrameRect,
            );
            add(
                OpeningComponentSlot::ElevationMuntins2D,
                OpeningGenerator::ElevationMuntinsSingle,
            );
            add(
                OpeningComponentSlot::ElevationSwing2D,
                OpeningGenerator::ElevationSwingTriangle,
            );
            add(OpeningComponentSlot::Frame3D, OpeningGenerator::SolidFrame);
            add(OpeningComponentSlot::Leaf3D, OpeningGenerator::SolidLeaf);
        }
        OpeningKind::Breakthrough => {
            add(
                OpeningComponentSlot::BreakthroughSymbol2D,
                OpeningGenerator::Cross,
            );
            add(OpeningComponentSlot::Mark2D, OpeningGenerator::Cross);
            add(
                OpeningComponentSlot::OpeningLabel2D,
                OpeningGenerator::OpeningLabel,
            );
            add(
                OpeningComponentSlot::ElevationContour2D,
                OpeningGenerator::ElevationFrameRect,
            );
        }
    }
    slots
}

/// Return the kind-based fallback map for a style.
pub fn effective_slots(
    opening_styles: &HashMap<StyleId, OpeningStyle>,
    id: &StyleId,
) -> Result<HashMap<OpeningComponentSlot, SlotGeometry>, StyleError> {
    let styles: HashMap<StyleId, Style> = opening_styles
        .iter()
        .map(|(k, v)| (k.clone(), v.style.clone()))
        .collect();
    let chain = resolve_chain(&styles, id)?;
    let kind = chain
        .last()
        .and_then(|style_id| opening_styles.get(style_id))
        .map(|style| style.kind)
        .unwrap_or(OpeningKind::Window);
    Ok(default_slots_for_kind(kind))
}

/// Plan-level opening overrides, keyed by opening `Style.id`.
pub type OpeningPlanProfiles = HashMap<StyleId, OpeningDisplayProfile>;

/// Kind-based fallback, then plan overrides from root to leaf.
pub fn effective_slots_for_plan(
    opening_styles: &HashMap<StyleId, OpeningStyle>,
    id: &StyleId,
    plan_profiles: Option<&OpeningPlanProfiles>,
) -> Result<HashMap<OpeningComponentSlot, SlotGeometry>, StyleError> {
    let mut slots = effective_slots(opening_styles, id)?;
    let styles: HashMap<StyleId, Style> = opening_styles
        .iter()
        .map(|(k, v)| (k.clone(), v.style.clone()))
        .collect();
    let chain = resolve_chain(&styles, id)?;
    for style_id in chain {
        let over = plan_profiles.and_then(|p| p.get(&style_id));
        if let Some(profile) = over {
            for (slot, geom) in &profile.slots {
                slots.insert(*slot, geom.clone());
            }
        }
    }
    Ok(slots)
}

/// Overlays plan slot visibility onto a rule set. A slot already hidden in
/// `visibility` (e.g. by
/// the session 2D/3D/All representation) stays hidden.
pub fn apply_plan_visibility(
    opening_styles: &HashMap<StyleId, OpeningStyle>,
    id: &StyleId,
    plan_profiles: Option<&OpeningPlanProfiles>,
    visibility: &mut HashMap<String, bool>,
) {
    let styles: HashMap<StyleId, Style> = opening_styles
        .iter()
        .map(|(k, v)| (k.clone(), v.style.clone()))
        .collect();
    let Ok(chain) = resolve_chain(&styles, id) else {
        return;
    };
    let mut resolved: HashMap<String, bool> = HashMap::new();
    for style_id in chain {
        let over = plan_profiles.and_then(|p| p.get(&style_id));
        if let Some(profile) = over {
            for (key, vis) in &profile.visibility {
                resolved.insert(key.clone(), *vis);
            }
        }
    }
    for (key, vis) in resolved {
        visibility
            .entry(key)
            .and_modify(|v| *v = *v && vis)
            .or_insert(vis);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::openings::{
        DEFAULT_BREAKTHROUGH_HEIGHT, DEFAULT_BREAKTHROUGH_SILL, DEFAULT_BREAKTHROUGH_WIDTH,
    };

    fn map_of(styles: Vec<OpeningStyle>) -> HashMap<StyleId, OpeningStyle> {
        styles
            .into_iter()
            .map(|s| (s.style.id.clone(), s))
            .collect()
    }

    #[test]
    fn seed_styles_cover_three_kinds() {
        let seeds = seed_opening_styles();
        assert_eq!(seeds.len(), 10);
        assert!(seeds.iter().any(|s| s.kind == OpeningKind::Window));
        assert!(seeds.iter().any(|s| s.kind == OpeningKind::Door));
        assert!(seeds.iter().any(|s| s.kind == OpeningKind::Breakthrough));
        assert!(approx(
            seeds[0].parts.fixed_frame.face_width,
            DEFAULT_FRAME_THICKNESS
        ));
        let br = seeds
            .iter()
            .find(|s| s.style.id == SEED_BREAKTHROUGH_STYLE_ID)
            .unwrap();
        assert!(approx(br.default_width, DEFAULT_BREAKTHROUGH_WIDTH));
        assert!(approx(br.default_height, DEFAULT_BREAKTHROUGH_HEIGHT));
        assert!(approx(br.default_sill, DEFAULT_BREAKTHROUGH_SILL));
        assert_eq!(OpeningStyle::window_double().grid.vertical, vec![0.5, 0.5]);
        assert_eq!(OpeningStyle::door_double().grid.vertical, vec![0.5, 0.5]);
    }

    #[test]
    fn frame_dimensions_are_independent_of_instance_width() {
        let mut style = OpeningStyle::standard_window();
        let frame_width = style.parts.fixed_frame.face_width;
        style.default_width *= 1.5;
        assert!(approx(style.parts.fixed_frame.face_width, frame_width));
    }

    #[test]
    fn opening_style_without_looks_deserializes_to_empty_looks() {
        let style = OpeningStyle::standard_window();
        let mut value = serde_json::to_value(style).unwrap();
        value.as_object_mut().unwrap().remove("looks");

        let restored: OpeningStyle = serde_json::from_value(value).unwrap();
        assert_eq!(restored.looks, OpeningLooks::default());
    }

    #[test]
    fn optional_component_enabled_defaults_on_for_legacy_json() {
        let legacy_frame: FixedFrameDimensions =
            serde_json::from_str(r#"{"face_width":0.04}"#).unwrap();
        assert!(legacy_frame.enabled);
        assert_eq!(legacy_frame.profile_id, None);
        let legacy_leaf: LeafFrameDimensions =
            serde_json::from_str(r#"{"face_width":0.04}"#).unwrap();
        assert!(legacy_leaf.enabled);
        assert_eq!(legacy_leaf.profile_id, None);
        assert!(serde_json::from_str::<GlazingDimensions>(r#"{"thickness":0.02}"#)
            .unwrap()
            .enabled);
        let legacy_sill: SillDimensions = serde_json::from_str(r#"{"thickness":0.1}"#).unwrap();
        assert!(legacy_sill.enabled);
        assert_eq!(legacy_sill.profile_id, None);
        let legacy_threshold: ThresholdDimensions =
            serde_json::from_str(r#"{"height":0.02}"#).unwrap();
        assert!(legacy_threshold.enabled);
        assert_eq!(legacy_threshold.profile_id, None);
        let legacy_shutter: ShutterBoxDimensions =
            serde_json::from_str(r#"{"height":0.2}"#).unwrap();
        assert!(legacy_shutter.enabled);
        assert_eq!(legacy_shutter.profile_id, None);
        let legacy_lintel: LintelDimensions = serde_json::from_str(r#"{"height":0.2}"#).unwrap();
        assert!(legacy_lintel.enabled);
        assert_eq!(legacy_lintel.profile_id, None);
        let legacy_grid: DivisionGrid = serde_json::from_str(r#"{"vertical":[0.5]}"#).unwrap();
        assert_eq!(legacy_grid.profile_id, None);
    }

    #[test]
    fn opening_components_are_limited_by_kind() {
        assert!(opening_component_applies(
            OpeningKind::Door,
            OpeningLookRole::Threshold
        ));
        assert!(!opening_component_applies(
            OpeningKind::Door,
            OpeningLookRole::ShutterBox
        ));
        assert!(!opening_component_applies(
            OpeningKind::Breakthrough,
            OpeningLookRole::Leaf
        ));
        assert!(opening_component_applies(
            OpeningKind::Breakthrough,
            OpeningLookRole::Lintel
        ));
        assert!(!OpeningEditorGroup::Ratings.applies_to(OpeningKind::Window));
        assert!(OpeningEditorGroup::Symbol.applies_to(OpeningKind::Breakthrough));
    }

    #[test]
    fn optional_component_rendering_requires_enabled_positive_dimensions() {
        let mut parts = OpeningParts::default();
        assert!(opening_component_should_render(
            &parts,
            OpeningKind::Window,
            OpeningLookRole::FixedFrame
        ));
        parts.fixed_frame.enabled = false;
        assert!(!opening_component_should_render(
            &parts,
            OpeningKind::Window,
            OpeningLookRole::FixedFrame
        ));

        parts.fixed_frame.enabled = true;
        parts.lintel.height = 0.2;
        assert!(opening_component_should_render(
            &parts,
            OpeningKind::Window,
            OpeningLookRole::Lintel
        ));

        parts.lintel.enabled = false;
        assert!(!opening_component_should_render(
            &parts,
            OpeningKind::Window,
            OpeningLookRole::Lintel
        ));

        parts.lintel.enabled = true;
        parts.lintel.height = 0.0;
        assert!(!opening_component_should_render(
            &parts,
            OpeningKind::Window,
            OpeningLookRole::Lintel
        ));
        assert!(!opening_component_should_render(
            &parts,
            OpeningKind::Door,
            OpeningLookRole::ShutterBox
        ));
    }

    #[test]
    fn lintel_look_material_precedes_legacy_lintel_material() {
        let mut style = OpeningStyle::standard_window();
        style.parts.lintel.material_id = Some("legacy".to_string());
        assert_eq!(
            style.material_id_for_look(OpeningLookRole::Lintel),
            Some("legacy")
        );

        style.looks.lintel.material_id = Some("look".to_string());
        assert_eq!(
            style.material_id_for_look(OpeningLookRole::Lintel),
            Some("look")
        );
    }

    #[test]
    fn opening_slots_map_to_shared_look_roles() {
        assert_eq!(
            OpeningLookRole::for_slot(OpeningComponentSlot::Frame2D),
            Some(OpeningLookRole::FixedFrame)
        );
        assert_eq!(
            OpeningLookRole::for_slot(OpeningComponentSlot::ElevationContour2D),
            Some(OpeningLookRole::FixedFrame)
        );
        assert_eq!(
            OpeningLookRole::for_slot(OpeningComponentSlot::Frame3D),
            Some(OpeningLookRole::FixedFrame)
        );
        assert_eq!(
            OpeningLookRole::for_slot(OpeningComponentSlot::ElevationSwing2D),
            Some(OpeningLookRole::Swing)
        );
        assert_eq!(
            OpeningLookRole::for_slot(OpeningComponentSlot::Sill2D),
            Some(OpeningLookRole::ExteriorSill)
        );
        assert_eq!(OpeningLookRole::for_slot(OpeningComponentSlot::HostCut2D), None);
    }

    #[test]
    fn division_grid_rejects_invalid_fraction_sums() {
        let grid = DivisionGrid {
            vertical: vec![0.3, 0.6],
            cells: vec![OpeningCell::default(), OpeningCell::default()],
            ..DivisionGrid::default()
        };
        assert!(grid.validate(1.2).is_err());
    }

    #[test]
    fn heterogeneous_door_window_grid_validates() {
        let grid = DivisionGrid {
            vertical: vec![0.4, 0.6],
            cells: vec![
                OpeningCell {
                    kind: OpeningCellKind::DoorLeaf,
                    hinge: Some(HingeSide::Left),
                    sill: 0.0,
                },
                OpeningCell {
                    kind: OpeningCellKind::Glass,
                    hinge: None,
                    sill: 0.9,
                },
            ],
            ..DivisionGrid::default()
        };
        assert!(grid.validate(2.2).is_ok());
        assert_eq!(grid.cells[0].kind, OpeningCellKind::DoorLeaf);
        assert_eq!(grid.cells[1].sill, 0.9);
    }

    #[test]
    fn muntins_are_cell_relative_and_separate_from_primary_divisions() {
        let grid = DivisionGrid {
            vertical: vec![0.4, 0.6],
            vertical_muntins: vec![0.5],
            horizontal_muntins: vec![0.4],
            cells: vec![OpeningCell::default(), OpeningCell::default()],
            ..DivisionGrid::default()
        };
        let cells = grid.layout(2.0, 1.2, OpeningKind::Window).unwrap();
        assert_eq!(cells.len(), 2);
        assert!((cells[0].left + 1.0).abs() < 1e-12);
        assert!((cells[0].right + 0.2).abs() < 1e-12);
        assert!((cells[1].left + 0.2).abs() < 1e-12);
        assert!((cells[1].right - 1.0).abs() < 1e-12);
        assert_eq!(grid.vertical_muntins, vec![0.5]);
        assert_eq!(grid.horizontal_muntins, vec![0.4]);
    }

    #[test]
    fn muntin_only_grid_uses_opening_kind_default_cell() {
        let grid = DivisionGrid {
            vertical_muntins: vec![0.5],
            ..DivisionGrid::default()
        };
        let cells = grid.layout(0.9, 2.0, OpeningKind::Door).unwrap();
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].cell.kind, OpeningCellKind::DoorLeaf);
    }

    #[test]
    fn serde_roundtrip_preserves_parts_and_grid() {
        let mut style = OpeningStyle::door_double();
        style.parts.shutter_box.height = 0.18;
        let json = serde_json::to_string(&style).unwrap();
        let back: OpeningStyle = serde_json::from_str(&json).unwrap();
        assert_eq!(style, back);
    }

    #[test]
    fn hinge_roundtrip() {
        assert_eq!(HingeSide::from_str("Right"), HingeSide::Right);
        assert_eq!(HingeSide::from_str(""), HingeSide::Left);
        assert_eq!(HingeSide::Left.mirrored(), HingeSide::Right);
    }

    #[test]
    fn plan_override_replaces_kind_default_slots_and_visibility() {
        let style = OpeningStyle::standard_window();
        let map = map_of(vec![style]);
        let id = SEED_WINDOW_STYLE_ID.to_string();

        let slots = effective_slots_for_plan(&map, &id, None).unwrap();
        assert_eq!(
            slots.get(&OpeningComponentSlot::Frame2D),
            Some(&SlotGeometry::Generator(OpeningGenerator::FrameRect))
        );
        let mut vis = HashMap::new();
        apply_plan_visibility(&map, &id, None, &mut vis);

        let mut over = OpeningDisplayProfile::default();
        over.slots.insert(
            OpeningComponentSlot::Swing2D,
            SlotGeometry::Generator(OpeningGenerator::SwingArc),
        );
        over.visibility
            .insert(OpeningComponentSlot::Sill2D.key().to_string(), true);
        over.visibility
            .insert(OpeningComponentSlot::Leaf2D.key().to_string(), false);
        let mut plan = OpeningPlanProfiles::new();
        plan.insert(id.clone(), over);
        let plan_slots = effective_slots_for_plan(&map, &id, Some(&plan)).unwrap();
        assert_eq!(
            plan_slots.get(&OpeningComponentSlot::Swing2D),
            Some(&SlotGeometry::Generator(OpeningGenerator::SwingArc))
        );
        let mut vis = HashMap::new();
        apply_plan_visibility(&map, &id, Some(&plan), &mut vis);
        assert_eq!(vis.get("Sill2D"), Some(&true));
        assert_eq!(vis.get("Leaf2D"), Some(&false));

        // A slot hidden by the session representation stays hidden.
        let mut vis = HashMap::new();
        vis.insert("Sill2D".to_string(), false);
        apply_plan_visibility(&map, &id, Some(&plan), &mut vis);
        assert_eq!(vis.get("Sill2D"), Some(&false));
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }
}
