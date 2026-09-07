//! AEC Display Configuration ("Plan") definitions.
//!
//! A [`DisplayConfig`] is a named, reusable library entry (analogous to
//! [`crate::modules::aec::engine::material::Material`] /
//! [`crate::modules::aec::engine::wall_style::WallStyle`]) that determines
//! how the same underlying element data (wall axis, layers, material) is
//! displayed in a given context — without ever mutating that element data.
//!
//! Fine-grained, per-slot display rules used to live directly on this
//! envelope (`component_rules` / `style_substitutions`); since Step 2 they
//! instead live style-centered on
//! [`crate::modules::aec::engine::wall_style::WallStyle::display_profiles`],
//! keyed by this type's `name` — see
//! [`crate::modules::aec::engine::library::resolve_effective_rule_set`].

use crate::modules::aec::engine::display_component::{
    ComponentStyleOverride, RepresentationMode, StyleDisplayOverlay, WallComponentKind,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Identifier of a wall style, used by `WallStyle::display_profiles` keys
/// on the *other* side of that map (this module only re-exports the type
/// alias for `style_substitutions`-shaped helper code elsewhere). Matches
/// the plain `id: String` already used by
/// [`crate::modules::aec::engine::style::Style`] / `WallStyle`.
pub type WallStyleRef = String;

/// The element-type identifier used for walls, matching the plain string
/// `"Wall"` already used elsewhere (e.g. `Style::object_kind`).
pub const WALL_ELEMENT_TYPE_ID: &str = "Wall";

/// Construction/planning phase of an element within a `DisplayConfig`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanPhase {
    /// Existing ("Bestand").
    Existing,
    /// To be demolished ("Abbruch").
    Demolition,
    /// Newly built ("Neu").
    New,
}

impl Default for PlanPhase {
    /// New elements default to "Neu" — the common case when drawing.
    fn default() -> Self {
        PlanPhase::New
    }
}

impl PlanPhase {
    /// Stable string tag used for `WALL` XDATA persistence.
    pub fn as_str(self) -> &'static str {
        match self {
            PlanPhase::Existing => "Existing",
            PlanPhase::Demolition => "Demolition",
            PlanPhase::New => "New",
        }
    }

    /// Inverse of [`PlanPhase::as_str`]; unknown/empty tags default to `New`
    /// so older `WALL` records without a phase field still parse.
    /// German UI labels (`Neubau`/`Bestand`/`Abbruch`) are accepted too.
    pub fn from_str(s: &str) -> Self {
        match s {
            "Existing" | "Bestand" => PlanPhase::Existing,
            "Demolition" | "Abbruch" => PlanPhase::Demolition,
            _ => PlanPhase::New,
        }
    }

    /// German UI label (Neubau / Bestand / Abbruch).
    pub fn display_label(self) -> &'static str {
        match self {
            PlanPhase::Existing => "Bestand",
            PlanPhase::Demolition => "Abbruch",
            PlanPhase::New => "Neubau",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PlanningStage {
    Permit,
    #[default]
    Design,
    Execution,
}

/// The kind of drawing view a `DisplayConfig` applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ViewType {
    /// Floor plan.
    #[default]
    FloorPlan,
    /// Section.
    Section,
    /// Elevation.
    Elevation,
}

/// A named, reusable display configuration ("Plan"), e.g. "Architekt 1:50"
/// or "Statik 1:50".
///
/// `scale` is purely informative (for sorting/display in a manager UI) and
/// never drives any rendering decision automatically — see Key Decision 7/8
/// in `.junie/plans/aec-plan-view-display-variants.md`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayConfig {
    /// Stable identity; names are labels and may be renamed.
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    /// Human-readable name, e.g. "Architekt 1:50".
    pub name: String,
    /// Freely chosen discipline label, e.g. "Architektur", "Statik".
    pub discipline: String,
    /// Informative drawing scale (e.g. `50.0` for 1:50). Never a control
    /// value — no automatic scale→config coupling exists (see Step 7,
    /// deliberately deferred).
    #[serde(default)]
    pub scale: Option<f64>,
    /// Planning stage (Permit/Design/Execution). Additive field: absent in
    /// `.ocsproj`/library files saved before this field existed, so it
    /// defaults to [`PlanningStage::Design`] on load instead of failing the
    /// whole project deserialization.
    #[serde(default)]
    pub planning_stage: PlanningStage,
    /// View type (floor plan / section / elevation). Additive field: absent in
    /// `.ocsproj`/library files saved before this field existed, so it
    /// defaults to [`ViewType::FloorPlan`] on load instead of failing the
    /// whole project deserialization.
    #[serde(default)]
    pub view_type: ViewType,
    /// Which [`PlanPhase`]s this config shows, plus extra style overrides for
    /// `Demolition`/`Existing` walls (e.g. dashed lines for demolition). `None`
    /// means "unfiltered" — every phase is shown, unchanged from before this
    /// field existed.
    #[serde(default)]
    pub phase_filter: Option<PhaseFilter>,
    /// Default 2D/3D/All filter for this plan type; session override wins.
    #[serde(default)]
    pub default_representation: RepresentationMode,
    /// Global component visibility (absent key = visible).
    #[serde(default)]
    pub component_visibility: HashMap<WallComponentKind, bool>,
    /// Sparse per-wall-style exceptions, keyed by `Style.id`.
    #[serde(default)]
    pub style_overlays: HashMap<String, StyleDisplayOverlay>,
    /// Global look for the 2D overall-contour hatch (`ContourHatch2D`).
    #[serde(default)]
    pub contour_hatch: Option<ComponentStyleOverride>,
}

/// Two-stage phase visibility/appearance filter for a [`DisplayConfig`]: which
/// [`PlanPhase`]s are visible at all, plus an optional extra style overlay
/// applied on top of the normal wall style resolution for `Demolition`
/// and `Existing` walls (e.g. dashed/grey lines).
/// Leaving a style `None` means "no extra overlay for that phase".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PhaseFilter {
    /// Phases shown by this config; a phase absent from this list is hidden
    /// entirely. An empty list means "show nothing" — callers building a
    /// UI for this should default to all phases selected.
    #[serde(default)]
    pub visible_phases: Vec<PlanPhase>,
    /// Extra style overlay applied to `Demolition`-phase walls, on top of
    /// their normal resolved style.
    #[serde(default)]
    pub demolition_style: Option<ComponentStyleOverride>,
    /// Extra style overlay applied to `Existing`-phase walls, on top of
    /// their normal resolved style.
    #[serde(default)]
    pub existing_style: Option<ComponentStyleOverride>,
}

/// Step 7 (auto scale coupling to the drawing scale): a single row of the
/// "when drawing scale X is active, suggest/activate DisplayConfig Y"
/// mapping table. Deliberately *not* a field of
/// [`DisplayConfig`] itself — per the plan wording this is a pure
/// comfort mechanism layered above the core model, so it lives as a
/// sibling collection on
/// [`crate::modules::aec::engine::library::DisplayConfigLibrary`] instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScaleDisplayConfigMapping {
    /// Drawing-scale name, e.g. `"1:50"`. Matches
    /// `doc.header.current_annotation_scale`.
    pub scale_name: String,
    /// Name of the [`DisplayConfig`] to suggest/activate for `scale_name`.
    pub display_config_name: String,
}

impl DisplayConfig {
    /// Creates a new `DisplayConfig` with no overrides yet.
    pub fn new(
        name: String,
        discipline: String,
        planning_stage: PlanningStage,
        view_type: ViewType,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            discipline,
            scale: None,
            planning_stage,
            view_type,
            phase_filter: None,
            default_representation: RepresentationMode::All,
            component_visibility: HashMap::new(),
            style_overlays: HashMap::new(),
            contour_hatch: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_config_construction_has_no_overrides() {
        let cfg = DisplayConfig::new(
            "Architekt 1:50".to_string(),
            "Architektur".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        assert_eq!(cfg.name, "Architekt 1:50");
        assert_eq!(cfg.discipline, "Architektur");
        assert_eq!(cfg.scale, None);
        assert_eq!(cfg.phase_filter, None);
    }

    #[test]
    fn display_config_roundtrip_serialization() {
        let mut cfg = DisplayConfig::new(
            "Statik 1:50".to_string(),
            "Statik".to_string(),
            PlanningStage::Design,
            ViewType::Section,
        );
        cfg.scale = Some(50.0);

        let serialized = serde_json::to_string(&cfg).unwrap();
        let deserialized: DisplayConfig = serde_json::from_str(&serialized).unwrap();
        assert_eq!(cfg, deserialized);
    }

    #[test]
    fn display_config_without_optional_fields_deserializes_with_defaults() {
        // Simulates an older/minimal record: missing `scale`/`phase_filter`
        // must default cleanly, not fail.
        let json = r#"{
            "name": "Architekt 1:50",
            "discipline": "Architektur",
            "planning_stage": "Design",
            "view_type": "FloorPlan"
        }"#;
        let cfg: DisplayConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.scale, None);
        assert_eq!(cfg.phase_filter, None);
        assert!(!cfg.id.is_nil());
        assert!(cfg.style_overlays.is_empty());
    }

    #[test]
    fn plan_phase_default_is_new() {
        assert_eq!(PlanPhase::default(), PlanPhase::New);
    }

    #[test]
    fn plan_phase_str_round_trip() {
        for phase in [PlanPhase::Existing, PlanPhase::Demolition, PlanPhase::New] {
            assert_eq!(PlanPhase::from_str(phase.as_str()), phase);
        }
        // Unknown/empty tags fall back to `New` so older records still parse.
        assert_eq!(PlanPhase::from_str(""), PlanPhase::New);
        assert_eq!(PlanPhase::from_str("bogus"), PlanPhase::New);
    }

    #[test]
    fn display_config_defaults_to_no_phase_filter() {
        let cfg = DisplayConfig::new(
            "Architekt 1:50".to_string(),
            "Architektur".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        assert_eq!(cfg.phase_filter, None);
    }

    #[test]
    fn display_config_without_phase_filter_field_deserializes_to_none() {
        // Records written before `phase_filter` existed must still parse,
        // behaving exactly as before (no filtering at all).
        let json = r#"{
            "name": "Architekt 1:50",
            "discipline": "Architektur",
            "planning_stage": "Design",
            "view_type": "FloorPlan"
        }"#;
        let cfg: DisplayConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.phase_filter, None);
    }

    #[test]
    fn display_config_phase_filter_roundtrip_serialization() {
        let mut cfg = DisplayConfig::new(
            "Abbruch/Bestand 1:50".to_string(),
            "Architektur".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        cfg.phase_filter = Some(PhaseFilter {
            visible_phases: vec![PlanPhase::New, PlanPhase::Demolition, PlanPhase::Existing],
            demolition_style: Some(ComponentStyleOverride {
                line_type: Some("Dashed".to_string()),
                ..Default::default()
            }),
            existing_style: Some(ComponentStyleOverride {
                line_color: Some(acadrust::types::Color::Rgb { r: 136, g: 136, b: 136 }),
                ..Default::default()
            }),
        });

        let serialized = serde_json::to_string(&cfg).unwrap();
        let deserialized: DisplayConfig = serde_json::from_str(&serialized).unwrap();
        assert_eq!(cfg, deserialized);
    }
}
