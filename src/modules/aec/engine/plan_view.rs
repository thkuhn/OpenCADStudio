//! AEC Display Configuration ("Plan") definitions.
//!
//! A [`DisplayConfig`] is a named, reusable library entry (analogous to
//! [`crate::modules::aec::engine::material::Material`] /
//! [`crate::modules::aec::engine::wall_style::WallStyle`]) that determines
//! how the same underlying element data (wall axis, layers, material) is
//! displayed in a given context — without ever mutating that element data.
//!
//! Fine-grained, per-slot display rules live in
//! [`crate::modules::aec::engine::display_component`]; this module only
//! carries the outer envelope (`name`, `discipline`, `scale`, `phase`,
//! `view_type`) plus the two override maps (`component_rules` /
//! `style_substitutions`).

use crate::modules::aec::engine::display_component::ComponentRuleSet;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Identifier of an architectural element type (e.g. `"Wall"`), used to key
/// [`DisplayConfig::component_rules`]. Kept as a plain string rather than an
/// enum so new element types (windows/doors, ...) can be added later without
/// touching this module.
pub type ElementTypeId = String;

/// Identifier of a wall style, used by [`DisplayConfig::style_substitutions`].
/// Matches the plain `id: String` already used by
/// [`crate::modules::aec::engine::style::Style`] / `WallStyle`.
pub type WallStyleRef = String;

/// The [`ElementTypeId`] used for walls, matching the plain string
/// `"Wall"` already used elsewhere (e.g. `Style::object_kind`) so
/// `DisplayConfig::component_rules` keys stay consistent across the
/// codebase.
pub const WALL_ELEMENT_TYPE_ID: &str = "Wall";

/// Construction/planning phase of an element within a `DisplayConfig`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PlanPhase {
    /// Existing ("Bestand").
    Existing,
    /// To be demolished ("Abbruch").
    Demolition,
    /// Newly built ("Neu").
    New,
}

/// The kind of drawing view a `DisplayConfig` applies to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ViewType {
    /// Grundriss.
    FloorPlan,
    /// Schnitt.
    Section,
    /// Ansicht.
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
    /// Human-readable name, e.g. "Architekt 1:50".
    pub name: String,
    /// Freely chosen discipline label, e.g. "Architektur", "Statik".
    pub discipline: String,
    /// Informative drawing scale (e.g. `50.0` for 1:50). Never a control
    /// value — no automatic scale→config coupling exists (see Step 7,
    /// deliberately deferred).
    #[serde(default)]
    pub scale: Option<f64>,
    /// Planning phase (Bestand/Abbruch/Neu).
    pub phase: PlanPhase,
    /// View type (Grundriss/Schnitt/Ansicht).
    pub view_type: ViewType,
    /// Fine-grained ("Detailed") per-slot overrides, keyed by element type.
    #[serde(default)]
    pub component_rules: HashMap<ElementTypeId, ComponentRuleSet>,
    /// Coarse-grained ("StyleSubstitution") shortcut: source wall style id
    /// -> target wall style id. A wall using the source style is displayed
    /// as if it used the target style's layer material/hatch/color, while
    /// axis, thickness and joins remain unchanged. See Key Decision 8.
    #[serde(default)]
    pub style_substitutions: HashMap<WallStyleRef, WallStyleRef>,
}

/// Step 7 ("Auto-Maßstabskopplung an den Zeichnungsmaßstab"): a single row
/// of the "bei aktivem Zeichnungsmaßstab X automatisch `DisplayConfig` Y
/// vorschlagen/aktivieren" mapping table. Deliberately *not* a field of
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
    pub fn new(name: String, discipline: String, phase: PlanPhase, view_type: ViewType) -> Self {
        Self {
            name,
            discipline,
            scale: None,
            phase,
            view_type,
            component_rules: HashMap::new(),
            style_substitutions: HashMap::new(),
        }
    }

    /// Resolves the [`ComponentRuleSet`] this config wants applied to
    /// walls, i.e. `component_rules.get(WALL_ELEMENT_TYPE_ID)`. Returns
    /// `None` when the config has no wall-specific rules, in which case
    /// callers should fall back to the default (every slot visible,
    /// standard style) — see `ComponentRuleSet::default()`.
    pub fn wall_rules(&self) -> Option<&ComponentRuleSet> {
        self.component_rules.get(WALL_ELEMENT_TYPE_ID)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::display_component::{
        ComponentRuleSet, ComponentStyleOverride, LayerSelection,
    };

    #[test]
    fn display_config_construction_has_no_overrides() {
        let cfg = DisplayConfig::new(
            "Architekt 1:50".to_string(),
            "Architektur".to_string(),
            PlanPhase::New,
            ViewType::FloorPlan,
        );
        assert_eq!(cfg.name, "Architekt 1:50");
        assert_eq!(cfg.discipline, "Architektur");
        assert_eq!(cfg.scale, None);
        assert!(cfg.component_rules.is_empty());
        assert!(cfg.style_substitutions.is_empty());
    }

    #[test]
    fn display_config_roundtrip_serialization() {
        let mut cfg = DisplayConfig::new(
            "Statik 1:50".to_string(),
            "Statik".to_string(),
            PlanPhase::Existing,
            ViewType::Section,
        );
        cfg.scale = Some(50.0);
        cfg.style_substitutions
            .insert("mw24-architekt".to_string(), "mw24-statik".to_string());
        let mut rules = ComponentRuleSet::default();
        rules.visibility.insert("AxisLine".to_string(), false);
        rules.style_override.insert(
            "Contour2D".to_string(),
            ComponentStyleOverride {
                line_color: Some(0x000000),
                ..Default::default()
            },
        );
        rules.layer_filter = LayerSelection::All;
        cfg.component_rules.insert("Wall".to_string(), rules);

        let serialized = serde_json::to_string(&cfg).unwrap();
        let deserialized: DisplayConfig = serde_json::from_str(&serialized).unwrap();
        assert_eq!(cfg, deserialized);
    }

    #[test]
    fn display_config_without_component_rules_field_deserializes_with_defaults() {
        // Simulates an older/minimal record: missing `component_rules` and
        // `style_substitutions` must default to empty maps, not fail.
        let json = r#"{
            "name": "Architekt 1:50",
            "discipline": "Architektur",
            "phase": "New",
            "view_type": "FloorPlan"
        }"#;
        let cfg: DisplayConfig = serde_json::from_str(json).unwrap();
        assert!(cfg.component_rules.is_empty());
        assert!(cfg.style_substitutions.is_empty());
        assert_eq!(cfg.scale, None);
    }

    #[test]
    fn wall_rules_resolves_component_rules_for_wall_element_type() {
        let mut cfg = DisplayConfig::new(
            "Statik 1:50".to_string(),
            "Statik".to_string(),
            PlanPhase::Existing,
            ViewType::Section,
        );
        assert!(cfg.wall_rules().is_none());

        let mut rules = ComponentRuleSet::default();
        rules.visibility.insert("AxisLine".to_string(), false);
        cfg.component_rules
            .insert(WALL_ELEMENT_TYPE_ID.to_string(), rules.clone());

        assert_eq!(cfg.wall_rules(), Some(&rules));
    }
}
