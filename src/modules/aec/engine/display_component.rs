//! Fine-grained display component slots for AEC
//! elements.
//!
//! Each element type (currently only walls; windows/doors are a deliberately
//! deferred later step, see `.junie/plans/aec-plan-view-display-variants.md`)
//! exposes a fixed catalogue of named slots (see [`WallComponentSlot`]). A
//! [`crate::modules::aec::engine::plan_view::DisplayConfig`] can then, per
//! slot, control visibility, override the style, and restrict which wall
//! layers feed into contour/solid slots — without ever mutating the
//! underlying wall data.
//!
//! Slot catalogues are additive: new variants can be added later without
//! invalidating existing `DisplayConfig` data, because `visibility` /
//! `style_override` are indexed by slot *name* (not position) and default to
//! "visible, standard style" when a slot is missing from the map (Key
//! Decision 7/8 non-regression guarantee).

use crate::modules::aec::engine::join::LayerRef;
use crate::modules::aec::engine::wall_style::{base_width_from_layers, WallStyle};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use uuid::Uuid;

/// Serialize `Option<acadrust::types::Color>` normally.
fn serialize_option_acad_color<S>(
    value: &Option<acadrust::types::Color>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(c) => serializer.serialize_some(c),
        None => serializer.serialize_none(),
    }
}

/// Accept either the native `AcadColor` serde shape or a legacy bare `u32`
/// (0xRRGGBB) from older TOML/JSON libraries — the latter becomes
/// `AcadColor::Rgb`.
fn deserialize_option_acad_color<'de, D>(
    deserializer: D,
) -> Result<Option<acadrust::types::Color>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OptColorVisitor;

    impl<'de> serde::de::Visitor<'de> for OptColorVisitor {
        type Value = Option<acadrust::types::Color>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("null, a legacy 0xRRGGBB integer, or an AcadColor value")
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
            let rgb = v as u32;
            Ok(Some(acadrust::types::Color::Rgb {
                r: ((rgb >> 16) & 0xFF) as u8,
                g: ((rgb >> 8) & 0xFF) as u8,
                b: (rgb & 0xFF) as u8,
            }))
        }

        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
            self.visit_u64(v as u64)
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
            acadrust::types::Color::deserialize(serde::de::value::StrDeserializer::new(v))
                .map(Some)
        }

        fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
            self.visit_str(&v)
        }

        fn visit_map<M: serde::de::MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
            acadrust::types::Color::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                .map(Some)
        }

        fn visit_enum<A: serde::de::EnumAccess<'de>>(self, data: A) -> Result<Self::Value, A::Error> {
            acadrust::types::Color::deserialize(serde::de::value::EnumAccessDeserializer::new(data))
                .map(Some)
        }
    }

    deserializer.deserialize_any(OptColorVisitor)
}

/// The catalogue of independently visible/stylable display components for a
/// wall (functional requirements a-i from the original request).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WallComponentSlot {
    /// a) Axis line.
    AxisLine,
    /// b) 2D overall contour.
    Contour2D,
    /// c) 2D hatch of the overall contour.
    ContourHatch2D,
    /// d) 2D wall layers.
    Layers2D,
    /// e) 2D hatches of the layers.
    LayerHatch2D,
    /// f) 3D solid body.
    Solid3D,
    /// g) 3D surface hatch/color.
    SurfaceStyle3D,
    /// h) Section representation.
    SectionRepresentation,
    /// i) Elevation representation.
    ElevationRepresentation,
}

impl WallComponentSlot {
    /// The stable string key used to index `ComponentRuleSet` maps, so that
    /// the enum's declaration order/position never affects persisted data.
    pub fn key(self) -> &'static str {
        match self {
            WallComponentSlot::AxisLine => "AxisLine",
            WallComponentSlot::Contour2D => "Contour2D",
            WallComponentSlot::ContourHatch2D => "ContourHatch2D",
            WallComponentSlot::Layers2D => "Layers2D",
            WallComponentSlot::LayerHatch2D => "LayerHatch2D",
            WallComponentSlot::Solid3D => "Solid3D",
            WallComponentSlot::SurfaceStyle3D => "SurfaceStyle3D",
            WallComponentSlot::SectionRepresentation => "SectionRepresentation",
            WallComponentSlot::ElevationRepresentation => "ElevationRepresentation",
        }
    }
}

/// Viewport / plan-type representation filter: 2D, 3D, or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum RepresentationMode {
    TwoD,
    ThreeD,
    #[default]
    All,
}

/// Geometry vs. hatch catalogue used by plan-type visibility (legacy
/// `SectionRepresentation`/`ElevationRepresentation` are ignored on load).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WallComponentKind {
    Axis,
    Layers2D,
    LayerHatch2D,
    Contour2D,
    ContourHatch2D,
    Layers3D,
    SurfaceStyle3D,
}

impl WallComponentKind {
    pub fn all() -> &'static [WallComponentKind] {
        &[
            WallComponentKind::Axis,
            WallComponentKind::Layers2D,
            WallComponentKind::LayerHatch2D,
            WallComponentKind::Contour2D,
            WallComponentKind::ContourHatch2D,
            WallComponentKind::Layers3D,
            WallComponentKind::SurfaceStyle3D,
        ]
    }

    pub fn is_2d(self) -> bool {
        matches!(
            self,
            WallComponentKind::Axis
                | WallComponentKind::Layers2D
                | WallComponentKind::LayerHatch2D
                | WallComponentKind::Contour2D
                | WallComponentKind::ContourHatch2D
        )
    }

    pub fn is_3d(self) -> bool {
        matches!(
            self,
            WallComponentKind::Layers3D | WallComponentKind::SurfaceStyle3D
        )
    }

    pub fn from_slot(slot: WallComponentSlot) -> Option<Self> {
        match slot {
            WallComponentSlot::AxisLine => Some(WallComponentKind::Axis),
            WallComponentSlot::Layers2D => Some(WallComponentKind::Layers2D),
            WallComponentSlot::LayerHatch2D => Some(WallComponentKind::LayerHatch2D),
            WallComponentSlot::Contour2D => Some(WallComponentKind::Contour2D),
            WallComponentSlot::ContourHatch2D => Some(WallComponentKind::ContourHatch2D),
            WallComponentSlot::Solid3D => Some(WallComponentKind::Layers3D),
            WallComponentSlot::SurfaceStyle3D => Some(WallComponentKind::SurfaceStyle3D),
            WallComponentSlot::SectionRepresentation | WallComponentSlot::ElevationRepresentation => {
                None
            }
        }
    }

    pub fn to_slot(self) -> WallComponentSlot {
        match self {
            WallComponentKind::Axis => WallComponentSlot::AxisLine,
            WallComponentKind::Layers2D => WallComponentSlot::Layers2D,
            WallComponentKind::LayerHatch2D => WallComponentSlot::LayerHatch2D,
            WallComponentKind::Contour2D => WallComponentSlot::Contour2D,
            WallComponentKind::ContourHatch2D => WallComponentSlot::ContourHatch2D,
            WallComponentKind::Layers3D => WallComponentSlot::Solid3D,
            WallComponentKind::SurfaceStyle3D => WallComponentSlot::SurfaceStyle3D,
        }
    }
}

impl RepresentationMode {
    pub fn allows(self, kind: WallComponentKind) -> bool {
        match self {
            RepresentationMode::All => true,
            RepresentationMode::TwoD => kind.is_2d(),
            RepresentationMode::ThreeD => kind.is_3d(),
        }
    }
}

/// Per-layer 2D/3D visibility in a style overlay (default: visible).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerVis {
    #[serde(default = "default_true")]
    pub visible_2d: bool,
    #[serde(default = "default_true")]
    pub visible_3d: bool,
}

fn default_true() -> bool {
    true
}

impl Default for LayerVis {
    fn default() -> Self {
        Self {
            visible_2d: true,
            visible_3d: true,
        }
    }
}

/// Sparse per-style exception stored on a plan type (`DisplayConfig`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StyleDisplayOverlay {
    #[serde(default)]
    pub layer_visibility: HashMap<Uuid, LayerVis>,
    #[serde(default)]
    pub contour_layers: Option<Vec<Uuid>>,
    #[serde(default)]
    pub layer_props: HashMap<Uuid, ComponentStyleOverride>,
    /// Optional look for the overall 2D contour hatch (`ContourHatch2D`).
    #[serde(default)]
    pub contour_hatch: Option<ComponentStyleOverride>,
}

/// A full, self-contained style override for a single slot or layer —
/// deliberately not a mere on/off flag or a reference to an existing style,
/// so e.g. "Statik 1:50" can define its own line/hatch/fill look for
/// `Layers2D`/`LayerHatch2D` independent of the wall's own material.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ComponentStyleOverride {
    /// Line type override (e.g. `"Continuous"`, `"Dashed"`).
    #[serde(default)]
    pub line_type: Option<String>,
    /// Line color override (`AcadColor`). Legacy bare `u32` 0xRRGGBB values
    /// still load as `AcadColor::Rgb`.
    #[serde(default)]
    #[serde(
        serialize_with = "serialize_option_acad_color",
        deserialize_with = "deserialize_option_acad_color"
    )]
    pub line_color: Option<acadrust::types::Color>,
    /// Hatch pattern name override.
    #[serde(default)]
    pub hatch_pattern: Option<String>,
    /// Hatch color override (`AcadColor`). Legacy bare `u32` 0xRRGGBB values
    /// still load as `AcadColor::Rgb`.
    #[serde(default)]
    #[serde(
        serialize_with = "serialize_option_acad_color",
        deserialize_with = "deserialize_option_acad_color"
    )]
    pub hatch_color: Option<acadrust::types::Color>,
    /// Hatch pattern scale. `None` inherits; `<= 0` is clamped to `0.01` at regen.
    #[serde(default)]
    pub hatch_scale: Option<f64>,
    /// Fill color override (`AcadColor`), used by 3D surface styling.
    /// Legacy bare `u32` 0xRRGGBB values still load as `AcadColor::Rgb`.
    #[serde(default)]
    #[serde(
        serialize_with = "serialize_option_acad_color",
        deserialize_with = "deserialize_option_acad_color"
    )]
    pub fill_color: Option<acadrust::types::Color>,
    /// Hatch angle override in degrees, part of the hatch-angle override
    /// chain `Wall.hatch_override` > style-profile `ComponentStyleOverride`
    /// > `Material.hatch_angle` (Step 3).
    #[serde(default)]
    pub hatch_angle: Option<f64>,
    /// Whether `hatch_angle` is relative to the wall's run direction
    /// (mirrors `Material.hatch_angle_relative`). Only meaningful together
    /// with `hatch_angle`.
    #[serde(default)]
    pub hatch_angle_relative: Option<bool>,
    /// Optional CAD layer name override.
    #[serde(default)]
    pub cad_layer: Option<String>,
}

impl ComponentStyleOverride {
    /// Copy every `Some` field from `src` onto `self` (phase extras win).
    pub fn overlay_from(&mut self, src: &Self) {
        if src.line_type.is_some() {
            self.line_type = src.line_type.clone();
        }
        if src.line_color.is_some() {
            self.line_color = src.line_color;
        }
        if src.hatch_pattern.is_some() {
            self.hatch_pattern = src.hatch_pattern.clone();
        }
        if src.hatch_color.is_some() {
            self.hatch_color = src.hatch_color;
        }
        if src.hatch_scale.is_some() {
            self.hatch_scale = src.hatch_scale;
        }
        if src.fill_color.is_some() {
            self.fill_color = src.fill_color;
        }
        if src.hatch_angle.is_some() {
            self.hatch_angle = src.hatch_angle;
        }
        if src.hatch_angle_relative.is_some() {
            self.hatch_angle_relative = src.hatch_angle_relative;
        }
        if src.cad_layer.is_some() {
            self.cad_layer = src.cad_layer.clone();
        }
    }
}

/// Which wall layers feed into layer-aggregating slots (`Contour2D`,
/// `Solid3D`), instead of a coarse All/Exterior/Interior choice.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LayerSelection {
    /// All layers of the wall style contribute.
    All,
    /// Only the explicitly listed layers contribute.
    Explicit(Vec<LayerRef>),
}

impl Default for LayerSelection {
    fn default() -> Self {
        LayerSelection::All
    }
}

/// A style override tied to a specific wall layer (identified the same
/// material-/role-based way `join::LayerPairOverride` already does), rather
/// than a whole slot.
///
/// Stored as a `Vec` (like `JunctionOverride::layer_pairs`) instead of a
/// `HashMap<LayerRef, _>` because `LayerRef` is a struct and JSON object
/// keys must be strings/numbers — a `HashMap` keyed by a struct cannot be
/// serialized by `serde_json` without extra indirection.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LayerStyleOverride {
    /// The wall layer this override applies to.
    pub layer: LayerRef,
    /// The style to apply to that layer.
    pub style: ComponentStyleOverride,
}

/// The set of "Detailed" per-slot overrides for one element type within a
/// `DisplayConfig`. Missing entries in any map fall back to the default
/// (visible, standard style, all layers) — this is what makes new slots
/// additive and non-breaking for existing configurations.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ComponentRuleSet {
    /// Slot key -> visible? Absent key means visible (default).
    #[serde(default)]
    pub visibility: HashMap<String, bool>,
    /// Slot key -> full style override. Absent key means standard style.
    #[serde(default)]
    pub style_override: HashMap<String, ComponentStyleOverride>,
    /// Individual wall layer -> style override, independent of slot.
    #[serde(default)]
    pub layer_style_override: Vec<LayerStyleOverride>,
    /// Slot key -> which layers feed that (aggregating) slot. Absent key
    /// means `All` — see [`Self::layer_filter_for`].
    #[serde(default)]
    pub layer_filter: HashMap<String, LayerSelection>,
}

impl ComponentRuleSet {
    /// Returns whether `slot` is visible under this rule set. Slots without
    /// an explicit entry are visible by default (non-regression guarantee).
    pub fn is_visible(&self, slot: WallComponentSlot) -> bool {
        self.visibility.get(slot.key()).copied().unwrap_or(true)
    }

    /// Returns the style override for `slot`, if any was configured.
    pub fn style_for(&self, slot: WallComponentSlot) -> Option<&ComponentStyleOverride> {
        self.style_override.get(slot.key())
    }

    /// Returns the layer filter configured for `slot` (typically
    /// `Contour2D`/`Solid3D`), defaulting to `LayerSelection::All` when the
    /// slot has no explicit entry — this is what makes a profile without an
    /// explicit per-slot filter behave exactly like today (non-regression
    /// guarantee, Key Decision 7).
    pub fn layer_filter_for(&self, slot: WallComponentSlot) -> &LayerSelection {
        static ALL: LayerSelection = LayerSelection::All;
        self.layer_filter.get(slot.key()).unwrap_or(&ALL)
    }
}

/// Consistency check for a `StyleSubstitution` per the plan's requirement
/// ("consistency check: same total thickness/axis position when creating
/// the substitution"): the source and target wall style must have the same
/// total layer thickness (sum of fixed thicknesses + gaps, formulas
/// contribute `0.0` — see [`base_width_from_layers`]), so that substituting
/// only the layer material/hatch/color source never shifts the wall's axis
/// position or overall footprint.
///
/// Returns `Err` with a human-readable message when the total thicknesses
/// diverge (beyond a small floating-point tolerance); `Ok(())` otherwise.
/// Does not (yet) validate per-layer axis offsets beyond the aggregate
/// thickness, since there is no UI/API yet to create actual substitutions.
pub fn validate_style_substitution(source: &WallStyle, target: &WallStyle) -> Result<(), String> {
    let source_thickness = base_width_from_layers(&source.layers);
    let target_thickness = base_width_from_layers(&target.layers);
    if (source_thickness - target_thickness).abs() > 1e-6 {
        return Err(format!(
            "StyleSubstitution inconsistent: source wall style '{}' has total thickness {source_thickness}, but target '{}' has {target_thickness} — axis/thickness would shift.",
            source.style.id, target.style.id
        ));
    }
    Ok(())
}

/// Upserts a `(source, target)` style-substitution row into the
/// Style-Substitutions-UI's edit buffer (`Vec<(WallStyleRef, WallStyleRef)>`,
/// see Key Decision 3 of the plan): if `source` already has a row, its
/// target is replaced in place (preserving the existing row order) instead
/// of appending a duplicate — matching the `HashMap<WallStyleRef,
/// WallStyleRef>`-backed persistence, which allows only one target per
/// source anyway. If `source` has no existing row, a new one is appended.
pub fn upsert_style_substitution(
    existing: &[(String, String)],
    source: String,
    target: String,
) -> Vec<(String, String)> {
    let mut result = existing.to_vec();
    match result.iter().position(|(s, _)| *s == source) {
        Some(pos) => result[pos].1 = target,
        None => result.push((source, target)),
    }
    result
}

/// Parses a style-override editor colour buffer into an `AcadColor`.
/// Accepts lossless editor tokens (`ACI…` / `ByLayer` / …), bare/`#`-prefixed
/// hex (historical RGB buffers), and plain ACI integers. Blank input means
/// "no override" (`None`); unparsable input also degrades to `None`.
pub fn parse_editor_hex_color(text: &str) -> Option<acadrust::types::Color> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.eq_ignore_ascii_case("ByLayer") {
        return Some(acadrust::types::Color::ByLayer);
    }
    if trimmed.eq_ignore_ascii_case("ByBlock") {
        return Some(acadrust::types::Color::ByBlock);
    }
    if trimmed.eq_ignore_ascii_case("None") {
        return Some(acadrust::types::Color::None);
    }
    let aci_body = trimmed
        .strip_prefix("ACI")
        .or_else(|| trimmed.strip_prefix("aci"))
        .or_else(|| trimmed.strip_prefix("I:"))
        .or_else(|| trimmed.strip_prefix("i:"));
    if let Some(body) = aci_body {
        if let Ok(n) = body.trim().parse::<i16>() {
            return Some(acadrust::types::Color::from_index(n));
        }
    }
    let digits = trimmed.trim_start_matches('#');
    if digits.len() == 6 {
        if let Ok(value) = u32::from_str_radix(digits, 16) {
            return Some(acadrust::types::Color::Rgb {
                r: ((value >> 16) & 0xFF) as u8,
                g: ((value >> 8) & 0xFF) as u8,
                b: (value & 0xFF) as u8,
            });
        }
    }
    if let Ok(n) = trimmed.parse::<i16>() {
        return Some(acadrust::types::Color::from_index(n));
    }
    None
}

/// Converts a `LayerSelection` into the Layer-Filter-UI's edit-buffer shape:
/// `(is_explicit_selection, selected_layers)`. `All` becomes `(false, [])`;
/// `Explicit(layers)` becomes `(true, layers)` — including when `layers` is
/// empty, so an intentionally-emptied explicit selection is not silently
/// promoted back to "Alle" (see [`layer_filter_from_selection`] for the
/// matching reverse conversion and the round-trip tests below).
pub fn layer_filter_to_ui_state(filter: &LayerSelection) -> (bool, Vec<LayerRef>) {
    match filter {
        LayerSelection::All => (false, Vec::new()),
        LayerSelection::Explicit(layers) => (true, layers.clone()),
    }
}

/// Builds a `LayerSelection` from the Layer-Filter-UI's edit buffer: when
/// `is_explicit_selection` is `false` this is always `LayerSelection::All`
/// (the `selected` buffer is ignored, matching the "(●) Alle" radio choice);
/// otherwise it is `LayerSelection::Explicit(selected.to_vec())`, even when
/// `selected` is empty (an explicit-but-empty filter means "no layers
/// contribute", which is a deliberately different, valid state from "all
/// layers contribute").
pub fn layer_filter_from_selection(is_explicit_selection: bool, selected: &[LayerRef]) -> LayerSelection {
    if is_explicit_selection {
        LayerSelection::Explicit(selected.to_vec())
    } else {
        LayerSelection::All
    }
}

/// Builds a `ComponentStyleOverride` from the Step 8 style-override editor's
/// raw text buffers (line type / hatch pattern as free text, colors as hex
/// text). Blank text fields become `None` on the resulting override, so an
/// editor left entirely blank round-trips to `ComponentStyleOverride::default()`
/// (used by callers to decide whether to remove the slot's entry instead of
/// inserting an all-`None` override).
pub fn component_style_override_from_editor_fields(
    line_type: &str,
    line_color: &str,
    hatch_pattern: &str,
    hatch_color: &str,
    fill_color: &str,
) -> ComponentStyleOverride {
    let line_type = line_type.trim();
    let hatch_pattern = hatch_pattern.trim();
    ComponentStyleOverride {
        line_type: if line_type.is_empty() { None } else { Some(line_type.to_string()) },
        line_color: parse_editor_hex_color(line_color),
        hatch_pattern: if hatch_pattern.is_empty() { None } else { Some(hatch_pattern.to_string()) },
        hatch_color: parse_editor_hex_color(hatch_color),
        hatch_scale: None,
        fill_color: parse_editor_hex_color(fill_color),
        hatch_angle: None,
        hatch_angle_relative: None,
        cad_layer: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_slot_falls_back_to_visible_default() {
        let rules = ComponentRuleSet::default();
        assert!(rules.is_visible(WallComponentSlot::AxisLine));
        assert!(rules.is_visible(WallComponentSlot::Layers2D));
        assert!(rules.style_for(WallComponentSlot::Layers2D).is_none());
    }

    #[test]
    fn explicit_visibility_override_is_respected() {
        let mut rules = ComponentRuleSet::default();
        rules
            .visibility
            .insert(WallComponentSlot::AxisLine.key().to_string(), false);
        assert!(!rules.is_visible(WallComponentSlot::AxisLine));
        // Unrelated slots remain visible.
        assert!(rules.is_visible(WallComponentSlot::Contour2D));
    }

    #[test]
    fn component_rule_set_roundtrip_serialization() {
        let mut rules = ComponentRuleSet::default();
        rules
            .visibility
            .insert(WallComponentSlot::Layers2D.key().to_string(), false);
        rules.style_override.insert(
            WallComponentSlot::Contour2D.key().to_string(),
            ComponentStyleOverride {
                line_color: Some(acadrust::types::Color::Rgb { r: 0, g: 0, b: 0 }),
                line_type: Some("Continuous".to_string()),
                ..Default::default()
            },
        );
        rules.layer_filter.insert(
            WallComponentSlot::Contour2D.key().to_string(),
            LayerSelection::Explicit(vec![LayerRef {
                material_id: "brick".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            }]),
        );

        let serialized = serde_json::to_string(&rules).unwrap();
        let deserialized: ComponentRuleSet = serde_json::from_str(&serialized).unwrap();
        assert_eq!(rules, deserialized);
    }

    #[test]
    fn new_unknown_slot_added_later_falls_back_to_default() {
        // Simulates a `DisplayConfig` persisted before a new slot variant
        // existed: the rule set simply has no entry for it, so lookups for
        // that (hypothetical future) slot must fall back to the default.
        let json = r#"{
            "visibility": { "AxisLine": false },
            "style_override": {},
            "layer_style_override": [],
            "layer_filter": {}
        }"#;
        let rules: ComponentRuleSet = serde_json::from_str(json).unwrap();
        assert!(!rules.is_visible(WallComponentSlot::AxisLine));
        // A slot never mentioned in the JSON still defaults to visible.
        assert!(rules.is_visible(WallComponentSlot::SurfaceStyle3D));
    }

    #[test]
    fn rule_set_without_optional_fields_deserializes_with_defaults() {
        let json = r#"{}"#;
        let rules: ComponentRuleSet = serde_json::from_str(json).unwrap();
        assert!(rules.visibility.is_empty());
        assert!(rules.layer_filter.is_empty());
        assert_eq!(rules.layer_filter_for(WallComponentSlot::Contour2D), &LayerSelection::All);
        assert_eq!(rules.layer_filter_for(WallComponentSlot::Solid3D), &LayerSelection::All);
    }

    #[test]
    fn layer_filter_for_is_independent_per_slot() {
        let mut rules = ComponentRuleSet::default();
        let explicit = LayerSelection::Explicit(vec![LayerRef {
            material_id: "brick".to_string(),
            role_tag: None,
            index: 0,
        layer_id: None,
        }]);
        rules
            .layer_filter
            .insert(WallComponentSlot::Contour2D.key().to_string(), explicit.clone());
        assert_eq!(rules.layer_filter_for(WallComponentSlot::Contour2D), &explicit);
        // Solid3D was never set, so it still defaults to All.
        assert_eq!(rules.layer_filter_for(WallComponentSlot::Solid3D), &LayerSelection::All);
    }

    // ---- Style-Substitutions-UI: upsert helper -----------------------------

    #[test]
    fn upsert_style_substitution_appends_new_source() {
        let existing = vec![("A".to_string(), "B".to_string())];
        let result = upsert_style_substitution(&existing, "C".to_string(), "D".to_string());
        assert_eq!(
            result,
            vec![("A".to_string(), "B".to_string()), ("C".to_string(), "D".to_string())]
        );
    }

    #[test]
    fn upsert_style_substitution_overwrites_existing_source_in_place() {
        let existing = vec![
            ("A".to_string(), "B".to_string()),
            ("C".to_string(), "D".to_string()),
        ];
        let result = upsert_style_substitution(&existing, "A".to_string(), "Z".to_string());
        // Overwritten in place, order of unrelated rows preserved.
        assert_eq!(
            result,
            vec![("A".to_string(), "Z".to_string()), ("C".to_string(), "D".to_string())]
        );
    }

    #[test]
    fn upsert_style_substitution_on_empty_buffer_creates_first_row() {
        let result = upsert_style_substitution(&[], "A".to_string(), "B".to_string());
        assert_eq!(result, vec![("A".to_string(), "B".to_string())]);
    }

    fn make_wall_style(
        id: &str,
        layers: Vec<(&str, f64)>,
    ) -> WallStyle {
        use crate::modules::aec::engine::style::Style;
        use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, LayerValue};
        use uuid::Uuid;
        WallStyle {
            style: Style {
                id: id.to_string(),
                name: id.to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: layers
                .into_iter()
                .map(|(mat, thickness)| Layer {
                    material_id: mat.to_string(),
                    thickness: LayerValue::Fixed(thickness),
                    function: LayerFunction::Structural,
                    axis_offset: LayerValue::Fixed(0.0),
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    layer_override: None,
                    hatch_override: None,
                    role_tag: None,
                    layer_id: Uuid::new_v4(),
                })
                .collect(),
            display_profiles: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn validate_style_substitution_accepts_matching_total_thickness() {
        let source = make_wall_style("architekt", vec![("brick", 0.24)]);
        let target = make_wall_style("statik", vec![("concrete", 0.24)]);
        assert!(validate_style_substitution(&source, &target).is_ok());
    }

    #[test]
    fn validate_style_substitution_rejects_mismatched_total_thickness() {
        let source = make_wall_style("architekt", vec![("brick", 0.24)]);
        let target = make_wall_style("statik", vec![("concrete", 0.30)]);
        let result = validate_style_substitution(&source, &target);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("inconsistent"));
    }

    // ---- Step 8: style-override editor helpers ----------------------------

    #[test]
    fn parse_editor_hex_color_parses_plain_and_hash_prefixed_hex() {
        assert_eq!(
            parse_editor_hex_color("FF0000"),
            Some(acadrust::types::Color::Rgb { r: 0xFF, g: 0x00, b: 0x00 })
        );
        assert_eq!(
            parse_editor_hex_color("#00ff00"),
            Some(acadrust::types::Color::Rgb { r: 0x00, g: 0xff, b: 0x00 })
        );
        assert_eq!(
            parse_editor_hex_color("  #123456  "),
            Some(acadrust::types::Color::Rgb { r: 0x12, g: 0x34, b: 0x56 })
        );
    }

    #[test]
    fn parse_editor_hex_color_blank_input_means_no_override() {
        assert_eq!(parse_editor_hex_color(""), None);
        assert_eq!(parse_editor_hex_color("   "), None);
        assert_eq!(parse_editor_hex_color("#"), None);
    }

    #[test]
    fn parse_editor_hex_color_invalid_hex_degrades_to_none() {
        assert_eq!(parse_editor_hex_color("not-a-color"), None);
        assert_eq!(parse_editor_hex_color("GGGGGG"), None);
    }

    #[test]
    fn parse_editor_hex_color_preserves_index_and_logical_tokens() {
        assert_eq!(
            parse_editor_hex_color("ACI1"),
            Some(acadrust::types::Color::Index(1))
        );
        assert_eq!(
            parse_editor_hex_color("ByLayer"),
            Some(acadrust::types::Color::ByLayer)
        );
        assert_eq!(
            parse_editor_hex_color("ByBlock"),
            Some(acadrust::types::Color::ByBlock)
        );
    }

    #[test]
    fn component_style_override_from_editor_fields_blank_matches_default() {
        let style = component_style_override_from_editor_fields("", "", "", "", "");
        assert_eq!(style, ComponentStyleOverride::default());
    }

    #[test]
    fn component_style_override_from_editor_fields_parses_all_fields() {
        let style = component_style_override_from_editor_fields(
            "Continuous",
            "#000000",
            "ANSI31",
            "FFFFFF",
            "808080",
        );
        assert_eq!(
            style,
            ComponentStyleOverride {
                line_type: Some("Continuous".to_string()),
                line_color: Some(acadrust::types::Color::Rgb { r: 0, g: 0, b: 0 }),
                hatch_pattern: Some("ANSI31".to_string()),
                hatch_color: Some(acadrust::types::Color::Rgb {
                    r: 0xFF,
                    g: 0xFF,
                    b: 0xFF
                }),
                hatch_scale: None,
                fill_color: Some(acadrust::types::Color::Rgb {
                    r: 0x80,
                    g: 0x80,
                    b: 0x80
                }),
                hatch_angle: None,
                hatch_angle_relative: None,
                cad_layer: None,
            }
        );
    }

    #[test]
    fn component_style_override_legacy_u32_deserializes_as_rgb() {
        let json = r#"{
            "line_type": null,
            "line_color": 16711680,
            "hatch_pattern": null,
            "hatch_color": 255,
            "fill_color": null
        }"#;
        let style: ComponentStyleOverride = serde_json::from_str(json).unwrap();
        assert_eq!(
            style.line_color,
            Some(acadrust::types::Color::Rgb { r: 255, g: 0, b: 0 })
        );
        assert_eq!(
            style.hatch_color,
            Some(acadrust::types::Color::Rgb { r: 0, g: 0, b: 255 })
        );
    }

    #[test]
    fn component_rule_set_roundtrip_uses_acad_color() {
        let mut rules = ComponentRuleSet::default();
        rules.style_override.insert(
            WallComponentSlot::Contour2D.key().to_string(),
            ComponentStyleOverride {
                line_color: Some(acadrust::types::Color::Index(1)),
                line_type: Some("Continuous".to_string()),
                ..Default::default()
            },
        );
        let json = serde_json::to_string(&rules).unwrap();
        let back: ComponentRuleSet = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.style_for(WallComponentSlot::Contour2D)
                .unwrap()
                .line_color,
            Some(acadrust::types::Color::Index(1))
        );
    }

    #[test]
    fn component_style_override_from_editor_fields_trims_whitespace_only_text_fields() {
        let style = component_style_override_from_editor_fields("  ", "", "   ", "", "");
        assert_eq!(style.line_type, None);
        assert_eq!(style.hatch_pattern, None);
    }

    // ---- Layer-Filter-UI helpers -------------------------------------------

    #[test]
    fn layer_filter_to_ui_state_all_is_false_and_empty() {
        assert_eq!(layer_filter_to_ui_state(&LayerSelection::All), (false, Vec::new()));
    }

    #[test]
    fn layer_filter_to_ui_state_explicit_is_true_and_layers() {
        let layers = vec![
            LayerRef { material_id: "brick".to_string(), role_tag: None, index: 0, layer_id: None },
            LayerRef { material_id: "plaster".to_string(), role_tag: None, index: 1, layer_id: None },
        ];
        assert_eq!(
            layer_filter_to_ui_state(&LayerSelection::Explicit(layers.clone())),
            (true, layers)
        );
    }

    #[test]
    fn layer_filter_from_selection_false_is_always_all() {
        let layers = vec![LayerRef { material_id: "brick".to_string(), role_tag: None, index: 0, layer_id: None }];
        assert_eq!(layer_filter_from_selection(false, &layers), LayerSelection::All);
        assert_eq!(layer_filter_from_selection(false, &[]), LayerSelection::All);
    }

    #[test]
    fn layer_filter_from_selection_true_is_explicit() {
        let layers = vec![LayerRef { material_id: "brick".to_string(), role_tag: None, index: 0, layer_id: None }];
        assert_eq!(
            layer_filter_from_selection(true, &layers),
            LayerSelection::Explicit(layers)
        );
    }

    #[test]
    fn layer_filter_round_trip_all() {
        let filter = LayerSelection::All;
        let (is_explicit, selected) = layer_filter_to_ui_state(&filter);
        assert_eq!(layer_filter_from_selection(is_explicit, &selected), filter);
    }

    #[test]
    fn layer_filter_round_trip_explicit_non_empty() {
        let filter = LayerSelection::Explicit(vec![
            LayerRef { material_id: "brick".to_string(), role_tag: None, index: 0, layer_id: None },
            LayerRef { material_id: "plaster".to_string(), role_tag: Some("Tragschale".to_string()), index: 2, layer_id: None },
        ]);
        let (is_explicit, selected) = layer_filter_to_ui_state(&filter);
        assert_eq!(layer_filter_from_selection(is_explicit, &selected), filter);
    }

    /// Edge case (documented behavior): an explicit selection that has been
    /// emptied out (e.g. the user unchecked every layer while "Auswahl" was
    /// still active) round-trips as `Explicit(vec![])`, not back to `All`.
    /// This preserves the user's explicit intent ("no layers contribute")
    /// instead of silently reinterpreting it as "all layers contribute".
    #[test]
    fn layer_filter_round_trip_explicit_empty_stays_explicit() {
        let filter = LayerSelection::Explicit(Vec::new());
        let (is_explicit, selected) = layer_filter_to_ui_state(&filter);
        assert_eq!(is_explicit, true);
        assert!(selected.is_empty());
        assert_eq!(layer_filter_from_selection(is_explicit, &selected), filter);
    }
}
