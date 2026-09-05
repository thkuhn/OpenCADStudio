//! AEC Wall Style definitions.

use crate::modules::aec::engine::expr::eval_formula;
use crate::modules::aec::engine::material::MaterialId;
use crate::modules::aec::engine::style::{resolve_chain, Style, StyleError, StyleId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// The functional role of a wall layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LayerFunction {
    /// Load-bearing structural component.
    Structural,
    /// Thermal or acoustic insulation.
    Insulation,
    /// Decorative or protective finish.
    Finish,
    /// Any other functional role.
    Other(String),
}

/// Thickness of a wall style layer: a fixed number or an arithmetic formula.
///
/// Serialized untagged so existing libraries that store a bare JSON number
/// keep loading as [`LayerValue::Fixed`]. A JSON string becomes
/// [`LayerValue::Formula`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LayerValue {
    /// Constant thickness in drawing units.
    Fixed(f64),
    /// Arithmetic expression over wall variables (e.g. `"BB * 0.5"`).
    Formula(String),
}

impl LayerValue {
    /// Returns the fixed value, or `default` when this is a formula.
    ///
    /// Intended for UI/summary paths that need a plain number without wall
    /// context (style editors, rough totals). Formulas contribute `default`
    /// (typically `0.0`) rather than attempting evaluation.
    pub fn as_fixed_or(&self, default: f64) -> f64 {
        match self {
            LayerValue::Fixed(v) => *v,
            LayerValue::Formula(_) => default,
        }
    }

    /// Parse a user-entered thickness string.
    ///
    /// A value that parses as a full `f64` becomes [`LayerValue::Fixed`];
    /// anything else (non-empty) becomes [`LayerValue::Formula`]. Empty
    /// input is treated as `Fixed(0.0)`.
    pub fn parse_str(s: &str) -> Self {
        let t = s.trim();
        if t.is_empty() {
            return LayerValue::Fixed(0.0);
        }
        if let Ok(v) = t.parse::<f64>() {
            LayerValue::Fixed(v)
        } else {
            LayerValue::Formula(t.to_string())
        }
    }

    /// Resolve this value against `vars`.
    ///
    /// Fixed values return immediately. Formulas go through
    /// [`eval_formula`]; on failure returns `Err` (caller chooses fallback).
    pub fn resolve(&self, vars: &HashMap<String, f64>) -> Result<f64, String> {
        match self {
            LayerValue::Fixed(v) => Ok(*v),
            LayerValue::Formula(s) => eval_formula(s, vars),
        }
    }

    /// Render this value for display in a UI that shows lengths in
    /// centimeters, while the value itself is stored in meters.
    ///
    /// Formula strings are passed through unchanged (formulas already
    /// operate on meter-based vars such as `BB`).
    pub fn to_cm_display_string(&self) -> String {
        match self {
            LayerValue::Fixed(v) => format!("{}", v * 100.0),
            LayerValue::Formula(s) => s.clone(),
        }
    }

    /// Parse a user-entered string that represents a value in centimeters,
    /// converting fixed numbers back to meters for storage. Formula strings
    /// are left untouched.
    pub fn parse_cm_str(s: &str) -> Self {
        match Self::parse_str(s) {
            LayerValue::Fixed(v) => LayerValue::Fixed(v / 100.0),
            formula => formula,
        }
    }
}

impl fmt::Display for LayerValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayerValue::Fixed(v) => write!(f, "{v}"),
            LayerValue::Formula(s) => write!(f, "{s}"),
        }
    }
}

impl From<f64> for LayerValue {
    fn from(v: f64) -> Self {
        LayerValue::Fixed(v)
    }
}

impl Default for LayerValue {
    fn default() -> Self {
        LayerValue::Fixed(0.0)
    }
}

/// A single layer within a wall buildup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    /// Identifier of the material for this layer.
    pub material_id: MaterialId,
    /// Thickness of the layer (fixed number or formula).
    pub thickness: LayerValue,
    /// Functional role of this layer.
    pub function: LayerFunction,
    /// Start of this layer relative to the wall axis (fixed or formula).
    ///
    /// `0.0` places the layer start on the axis; negative values go to one
    /// side, positive to the other. Unlike the former sequential `gap_before`
    /// stacking model, this is an absolute offset — layers are not centered
    /// or accumulated automatically.
    #[serde(default)]
    pub axis_offset: LayerValue,
    /// Optional vertical offset from the wall's base.
    #[serde(default)]
    pub bottom_offset: f64,
    /// Optional vertical offset from the wall's top.
    #[serde(default)]
    pub top_offset: f64,
    /// Optional override of the drawing layer this layer's contour/hatch
    /// entities are placed on. `None` falls back to the wall's own layer.
    #[serde(default)]
    pub layer_override: Option<String>,
    /// Optional hatch pattern override for this layer's 2D hatch, taking
    /// precedence over the material's own hatch pattern. `None` falls back
    /// to the material's `hatch_pattern`. Additive field: absent in older
    /// libraries, so it defaults to `None` on load.
    #[serde(default)]
    pub hatch_override: Option<String>,
    /// Optional free-text role tag shown in the Style Manager to clarify a
    /// layer's purpose beyond its [`LayerFunction`] (e.g. "Vormauerschale",
    /// "Luftschicht"). Purely informational; does not affect geometry.
    /// Additive field: absent in older libraries, so it defaults to `None`.
    #[serde(default)]
    pub role_tag: Option<String>,
}

/// A style defining the layered buildup of a wall.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WallStyle {
    /// Generic style properties and inheritance.
    #[serde(flatten)]
    pub style: Style,
    /// List of layers from exterior to interior.
    /// An empty list means "inherits parent's layers".
    ///
    /// Deserialized via [`deserialize_layers`] so legacy `gap_before` entries
    /// are migrated to absolute `axis_offset` values at the list level.
    #[serde(deserialize_with = "deserialize_layers")]
    pub layers: Vec<Layer>,
    /// Style-centered display overrides, keyed by `DisplayConfig::name`: one
    /// `ComponentRuleSet` per Planart/Maßstab this style has been tuned for.
    /// Absent entries mean "use the default rule set" (every slot visible,
    /// standard style, `All` layers for `Contour2D`/`Solid3D`) — see
    /// [`crate::modules::aec::engine::library::resolve_effective_rule_set`].
    #[serde(default)]
    pub display_profiles: HashMap<String, crate::modules::aec::engine::display_component::ComponentRuleSet>,
}

/// Intermediate layer shape used only during library/XDATA-adjacent serde.
/// Accepts either the new `axis_offset` field or the legacy `gap_before`.
#[derive(Debug, Clone, Deserialize)]
struct LayerSerde {
    material_id: MaterialId,
    thickness: LayerValue,
    function: LayerFunction,
    #[serde(default)]
    axis_offset: Option<LayerValue>,
    #[serde(default)]
    gap_before: Option<f64>,
    #[serde(default)]
    bottom_offset: f64,
    #[serde(default)]
    top_offset: f64,
    #[serde(default)]
    layer_override: Option<String>,
    #[serde(default)]
    hatch_override: Option<String>,
    #[serde(default)]
    role_tag: Option<String>,
}

fn deserialize_layers<'de, D>(deserializer: D) -> Result<Vec<Layer>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Vec<LayerSerde> = Vec::deserialize(deserializer)?;
    Ok(layers_from_serde(raw))
}

fn layers_from_serde(raw: Vec<LayerSerde>) -> Vec<Layer> {
    let any_explicit_offset = raw.iter().any(|l| l.axis_offset.is_some());
    if any_explicit_offset {
        return raw
            .into_iter()
            .map(|l| Layer {
                material_id: l.material_id,
                thickness: l.thickness,
                function: l.function,
                axis_offset: l.axis_offset.unwrap_or_default(),
                bottom_offset: l.bottom_offset,
                top_offset: l.top_offset,
                layer_override: l.layer_override,
                hatch_override: l.hatch_override,
                role_tag: l.role_tag,
            })
            .collect();
    }

    // Legacy path: no axis_offset present on any layer → migrate the whole
    // stack from gap_before (default 0.0) using the historical centered layout.
    let pairs: Vec<(f64, f64)> = raw
        .iter()
        .map(|l| (l.thickness.as_fixed_or(0.0), l.gap_before.unwrap_or(0.0)))
        .collect();
    let offsets = migrate_gap_before_to_axis_offset(&pairs);
    raw.into_iter()
        .zip(offsets)
        .map(|(l, offset)| Layer {
            material_id: l.material_id,
            thickness: l.thickness,
            function: l.function,
            axis_offset: LayerValue::Fixed(offset),
            bottom_offset: l.bottom_offset,
            top_offset: l.top_offset,
            layer_override: l.layer_override,
            hatch_override: l.hatch_override,
            role_tag: l.role_tag,
        })
        .collect()
}

/// A wall-style layer after formula resolution.
///
/// `thickness` is always a concrete `f64` suitable for geometry. When a
/// formula fails to evaluate, `thickness` is the safe fallback `0.0` and
/// `formula_error` carries the validation message for UI/diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedLayer {
    pub material_id: MaterialId,
    pub thickness: f64,
    pub function: LayerFunction,
    pub axis_offset: f64,
    pub bottom_offset: f64,
    pub top_offset: f64,
    pub layer_override: Option<String>,
    pub hatch_override: Option<String>,
    pub role_tag: Option<String>,
    /// Present when a `LayerValue::Formula` failed; the failed value is then `0.0`.
    pub formula_error: Option<String>,
}

/// Build the standard wall-variable map. Currently exposes:
/// - `"BB"`: wall base / total width in drawing units.
pub fn wall_vars(bb: f64) -> HashMap<String, f64> {
    let mut vars = HashMap::new();
    vars.insert("BB".to_string(), bb);
    vars
}

/// Approximate base width from style layers using only fixed thicknesses
/// (formulas contribute `0.0`). Used when no concrete wall width is known yet.
///
/// Only thicknesses are summed — `axis_offset` is a positioning value and does
/// not contribute to total width.
pub fn base_width_from_layers(layers: &[Layer]) -> f64 {
    layers
        .iter()
        .map(|l| l.thickness.as_fixed_or(0.0))
        .sum()
}

/// Resolve each layer's [`LayerValue`] fields against `vars`.
///
/// Invalid formulas fall back to `0.0` and set
/// [`ResolvedLayer::formula_error`] — never panics.
pub fn resolve_layer_values(layers: &[Layer], vars: &HashMap<String, f64>) -> Vec<ResolvedLayer> {
    layers
        .iter()
        .map(|layer| {
            let (thickness, t_err) = match layer.thickness.resolve(vars) {
                Ok(v) => (v, None),
                Err(e) => (0.0, Some(e)),
            };
            let (axis_offset, a_err) = match layer.axis_offset.resolve(vars) {
                Ok(v) => (v, None),
                Err(e) => (0.0, Some(e)),
            };
            let formula_error = match (t_err, a_err) {
                (None, None) => None,
                (Some(e), None) | (None, Some(e)) => Some(e),
                (Some(e1), Some(e2)) => Some(format!("{e1}; {e2}")),
            };
            ResolvedLayer {
                material_id: layer.material_id.clone(),
                thickness,
                function: layer.function.clone(),
                axis_offset,
                bottom_offset: layer.bottom_offset,
                top_offset: layer.top_offset,
                layer_override: layer.layer_override.clone(),
                hatch_override: layer.hatch_override.clone(),
                role_tag: layer.role_tag.clone(),
                formula_error,
            }
        })
        .collect()
}

/// Resolves the effective layers for a wall style by traversing the inheritance chain.
///
/// Returns the layers of the nearest ancestor (including self) that has a non-empty
/// `layers` list. If no ancestor has layers, returns an empty vector.

/// Convert a legacy sequential `(thickness, gap_before)` stack into absolute
/// `axis_offset` values that reproduce the old centered layout.
///
/// Old algorithm (also used historically by `layer_contours`):
/// `total = Σ(thickness + gap_before)`, start at `-total/2`, then for each
/// layer `start = cur + gap_before`, `end = start + thickness`, advance `cur`.
/// The returned value for each layer is that historical `start` offset.
pub fn migrate_gap_before_to_axis_offset(layers_in_old_order: &[(f64, f64)]) -> Vec<f64> {
    let total: f64 = layers_in_old_order
        .iter()
        .map(|(thickness, gap)| thickness + gap)
        .sum();
    let mut cur = -total * 0.5;
    let mut offsets = Vec::with_capacity(layers_in_old_order.len());
    for &(thickness, gap) in layers_in_old_order {
        let start = cur + gap;
        offsets.push(start);
        cur = start + thickness;
    }
    offsets
}

pub fn effective_layers(
    wall_styles: &HashMap<StyleId, WallStyle>,
    id: &StyleId,
) -> Result<Vec<Layer>, StyleError> {
    // We need to resolve the chain based on the underlying Style data.
    // Since resolve_chain takes a HashMap<StyleId, Style>, we need to provide that.
    // We can either map it or just implement the resolution logic here.
    // Mapping might be expensive for large libraries, but it's cleaner to reuse resolve_chain.

    // Actually, resolve_chain is generic enough that I could have made it take a trait,
    // but I can't change style.rs easily.
    // Let's create a temporary map for resolve_chain.
    let styles: HashMap<StyleId, Style> = wall_styles
        .iter()
        .map(|(k, v)| (k.clone(), v.style.clone()))
        .collect();

    let chain = resolve_chain(&styles, id)?;

    // resolve_chain returns root-to-child order: [root, ..., id]
    // We want the nearest ancestor (including self) that has layers,
    // so we should iterate backwards from the end of the chain.
    for style_id in chain.iter().rev() {
        if let Some(wall_style) = wall_styles.get(style_id) {
            if !wall_style.layers.is_empty() {
                return Ok(wall_style.layers.clone());
            }
        }
    }

    Ok(vec![])
}

/// Like [`effective_layers`], then resolves each layer's thickness against
/// `vars` (see [`resolve_layer_values`]).
///
/// Callers should populate `vars` with at least `"BB"` (wall base width),
/// typically via [`wall_vars`]. When the wall's overall width is not yet
/// known, use [`base_width_from_layers`] on the unresolved effective layers.
pub fn effective_layers_for_wall(
    wall_styles: &HashMap<StyleId, WallStyle>,
    id: &StyleId,
    vars: &HashMap<String, f64>,
) -> Result<Vec<ResolvedLayer>, StyleError> {
    let layers = effective_layers(wall_styles, id)?;
    Ok(resolve_layer_values(&layers, vars))
}

/// Convenience: resolve effective layers using `"BB" = bb`.
pub fn effective_layers_for_wall_bb(
    wall_styles: &HashMap<StyleId, WallStyle>,
    id: &StyleId,
    bb: f64,
) -> Result<Vec<ResolvedLayer>, StyleError> {
    effective_layers_for_wall(wall_styles, id, &wall_vars(bb))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_wall_style(id: &str, parent: Option<&str>, layers: Vec<Layer>) -> WallStyle {
        WallStyle {
            style: Style {
                id: id.to_string(),
                name: format!("Wall Style {id}"),
                object_kind: "Wall".to_string(),
                parent_style_id: parent.map(|s| s.to_string()),
            },
            layers,
            display_profiles: HashMap::new(),
        }
    }

    fn create_layer(mat_id: &str, thick: f64) -> Layer {
        Layer {
            material_id: mat_id.to_string(),
            thickness: LayerValue::Fixed(thick),
            function: LayerFunction::Structural,
            axis_offset: LayerValue::Fixed(0.0),
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            role_tag: None,
        }
    }

    fn create_formula_layer(mat_id: &str, formula: &str) -> Layer {
        Layer {
            material_id: mat_id.to_string(),
            thickness: LayerValue::Formula(formula.to_string()),
            function: LayerFunction::Structural,
            axis_offset: LayerValue::Fixed(0.0),
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            role_tag: None,
        }
    }

    #[test]
    fn test_inheritance() {
        let mut styles = HashMap::new();
        let parent_layers = vec![create_layer("brick", 100.0)];
        styles.insert(
            "parent".to_string(),
            create_wall_style("parent", None, parent_layers.clone()),
        );
        styles.insert(
            "child".to_string(),
            create_wall_style("child", Some("parent"), vec![]),
        );

        let layers = effective_layers(&styles, &"child".to_string()).unwrap();
        assert_eq!(layers, parent_layers);
    }

    #[test]
    fn test_override() {
        let mut styles = HashMap::new();
        let parent_layers = vec![create_layer("brick", 100.0)];
        let child_layers = vec![create_layer("concrete", 200.0)];
        styles.insert(
            "parent".to_string(),
            create_wall_style("parent", None, parent_layers),
        );
        styles.insert(
            "child".to_string(),
            create_wall_style("child", Some("parent"), child_layers.clone()),
        );

        let layers = effective_layers(&styles, &"child".to_string()).unwrap();
        assert_eq!(layers, child_layers);
    }

    #[test]
    fn test_empty_inheritance() {
        let mut styles = HashMap::new();
        styles.insert("A".to_string(), create_wall_style("A", None, vec![]));
        styles.insert("B".to_string(), create_wall_style("B", Some("A"), vec![]));

        let layers = effective_layers(&styles, &"B".to_string()).unwrap();
        assert!(layers.is_empty());
    }

    #[test]
    fn test_cycle_propagation() {
        let mut styles = HashMap::new();
        styles.insert(
            "A".to_string(),
            create_wall_style("A", Some("B"), vec![]),
        );
        styles.insert(
            "B".to_string(),
            create_wall_style("B", Some("A"), vec![]),
        );

        let result = effective_layers(&styles, &"A".to_string());
        assert_eq!(result, Err(StyleError::CycleDetected));
    }

    #[test]
    fn wall_style_display_profiles_default_to_empty_and_roundtrip() {
        use crate::modules::aec::engine::display_component::ComponentRuleSet;

        let style = create_wall_style("s", None, vec![create_layer("a", 0.24)]);
        assert!(style.display_profiles.is_empty());

        let mut with_profile = style.clone();
        let mut rules = ComponentRuleSet::default();
        rules.visibility.insert("AxisLine".to_string(), false);
        with_profile
            .display_profiles
            .insert("Statik 1:50".to_string(), rules);

        let serialized = serde_json::to_string(&with_profile).unwrap();
        let deserialized: WallStyle = serde_json::from_str(&serialized).unwrap();
        assert_eq!(with_profile, deserialized);
    }

    #[test]
    fn wall_style_without_display_profiles_field_deserializes_to_empty_map() {
        // Older/minimal records that predate this field must still parse.
        let json = r#"{
            "id": "legacy",
            "name": "Legacy",
            "object_kind": "Wall",
            "parent_style_id": null,
            "layers": []
        }"#;
        let style: WallStyle = serde_json::from_str(json).unwrap();
        assert!(style.display_profiles.is_empty());
    }

    #[test]
    fn layer_value_serde_fixed_number_roundtrip() {
        let json = "0.24";
        let v: LayerValue = serde_json::from_str(json).unwrap();
        assert_eq!(v, LayerValue::Fixed(0.24));
        // bare number still deserializes when embedded in a Layer
        let layer_json = r#"{
            "material_id": "m",
            "thickness": 0.175,
            "function": "Structural"
        }"#;
        let layer: Layer = serde_json::from_str(layer_json).unwrap();
        assert_eq!(layer.thickness, LayerValue::Fixed(0.175));
    }

    #[test]
    fn layer_value_serde_formula_string() {
        let json = r#""BB * 0.5""#;
        let v: LayerValue = serde_json::from_str(json).unwrap();
        assert_eq!(v, LayerValue::Formula("BB * 0.5".to_string()));
    }

    #[test]
    fn fixed_value_styles_unaffected_after_resolution() {
        let mut styles = HashMap::new();
        let layers = vec![
            create_layer("a", 0.1),
            create_layer("b", 0.2),
            create_layer("c", 0.05),
        ];
        styles.insert(
            "s".to_string(),
            create_wall_style("s", None, layers.clone()),
        );

        let bb = base_width_from_layers(&layers);
        assert!((bb - 0.35).abs() < 1e-12);

        let resolved = effective_layers_for_wall_bb(&styles, &"s".to_string(), bb).unwrap();
        assert_eq!(resolved.len(), 3);
        assert!((resolved[0].thickness - 0.1).abs() < 1e-12);
        assert!((resolved[1].thickness - 0.2).abs() < 1e-12);
        assert!((resolved[2].thickness - 0.05).abs() < 1e-12);
        assert!(resolved.iter().all(|r| r.formula_error.is_none()));

        // Same geometry inputs as formula pipeline: (thickness, axis_offset)
        let geometry: Vec<(f64, f64)> = resolved
            .iter()
            .map(|r| (r.thickness, r.axis_offset))
            .collect();
        let expected: Vec<(f64, f64)> = layers
            .iter()
            .map(|l| {
                (
                    l.thickness.as_fixed_or(0.0),
                    l.axis_offset.as_fixed_or(0.0),
                )
            })
            .collect();
        assert_eq!(geometry, expected);
    }

    #[test]
    fn bb_formula_resolves_for_different_base_widths() {
        let mut styles = HashMap::new();
        styles.insert(
            "s".to_string(),
            create_wall_style(
                "s",
                None,
                vec![
                    create_layer("fixed", 0.1),
                    create_formula_layer("half", "BB * 0.5"),
                ],
            ),
        );

        let r1 = effective_layers_for_wall_bb(&styles, &"s".to_string(), 0.4).unwrap();
        assert!((r1[0].thickness - 0.1).abs() < 1e-12);
        assert!((r1[1].thickness - 0.2).abs() < 1e-12);
        assert!(r1[1].formula_error.is_none());

        let r2 = effective_layers_for_wall_bb(&styles, &"s".to_string(), 0.8).unwrap();
        assert!((r2[0].thickness - 0.1).abs() < 1e-12);
        assert!((r2[1].thickness - 0.4).abs() < 1e-12);
        assert_ne!(r1[1].thickness, r2[1].thickness);
    }

    #[test]
    fn invalid_formula_falls_back_and_reports_error() {
        let mut styles = HashMap::new();
        styles.insert(
            "s".to_string(),
            create_wall_style(
                "s",
                None,
                vec![
                    create_layer("ok", 0.2),
                    create_formula_layer("bad_var", "UNKNOWN * 2"),
                    create_formula_layer("bad_syntax", "BB *"),
                    create_formula_layer("div0", "BB / 0"),
                ],
            ),
        );

        let resolved = effective_layers_for_wall_bb(&styles, &"s".to_string(), 0.4).unwrap();
        assert!((resolved[0].thickness - 0.2).abs() < 1e-12);
        assert!(resolved[0].formula_error.is_none());

        for bad in &resolved[1..] {
            assert_eq!(bad.thickness, 0.0, "invalid formula must fall back to 0.0");
            assert!(
                bad.formula_error.is_some(),
                "invalid formula must surface an error"
            );
        }
    }

    #[test]
    fn as_fixed_or_helper() {
        assert_eq!(LayerValue::Fixed(1.5).as_fixed_or(0.0), 1.5);
        assert_eq!(
            LayerValue::Formula("BB".into()).as_fixed_or(0.0),
            0.0
        );
    }

    #[test]
    fn layer_without_new_attrs_deserializes_with_defaults() {
        // Simulates an older saved library that predates `hatch_override`
        // and `role_tag`: both fields must default to `None` rather than
        // failing to deserialize.
        let layer_json = r#"{
            "material_id": "m",
            "thickness": 0.1,
            "function": "Structural",
            "bottom_offset": 0.0,
            "top_offset": 0.0,
            "layer_override": null
        }"#;
        let layer: Layer = serde_json::from_str(layer_json).unwrap();
        assert_eq!(layer.hatch_override, None);
        assert_eq!(layer.role_tag, None);
        assert_eq!(layer.axis_offset, LayerValue::Fixed(0.0));
    }

    #[test]
    fn negative_axis_offset_resolves_and_does_not_affect_base_width() {
        let mut layer = create_layer("brick", 0.24);
        layer.axis_offset = LayerValue::Fixed(-0.12);
        let layers = vec![layer];
        assert!((base_width_from_layers(&layers) - 0.24).abs() < 1e-12);

        let resolved = resolve_layer_values(&layers, &wall_vars(0.24));
        assert!((resolved[0].axis_offset - (-0.12)).abs() < 1e-12);
        assert!((resolved[0].thickness - 0.24).abs() < 1e-12);
        assert!(resolved[0].formula_error.is_none());
    }

    #[test]
    fn axis_offset_formula_resolves_and_invalid_falls_back() {
        let ok = Layer {
            material_id: "a".into(),
            thickness: LayerValue::Fixed(0.1),
            function: LayerFunction::Structural,
            axis_offset: LayerValue::Formula("BB * -0.5".into()),
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            role_tag: None,
        };
        let bad = Layer {
            material_id: "b".into(),
            thickness: LayerValue::Fixed(0.1),
            function: LayerFunction::Structural,
            axis_offset: LayerValue::Formula("UNKNOWN".into()),
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            role_tag: None,
        };
        let resolved = resolve_layer_values(&[ok, bad], &wall_vars(0.4));
        assert!((resolved[0].axis_offset - (-0.2)).abs() < 1e-12);
        assert!(resolved[0].formula_error.is_none());
        assert_eq!(resolved[1].axis_offset, 0.0);
        assert!(resolved[1].formula_error.is_some());
    }

    #[test]
    fn layer_new_attrs_roundtrip() {
        let mut layer = create_layer("brick", 0.24);
        layer.hatch_override = Some("ANSI32".to_string());
        layer.role_tag = Some("Vormauerschale".to_string());

        let serialized = serde_json::to_string(&layer).unwrap();
        let deserialized: Layer = serde_json::from_str(&serialized).unwrap();
        assert_eq!(layer, deserialized);
        assert_eq!(deserialized.hatch_override.as_deref(), Some("ANSI32"));
        assert_eq!(deserialized.role_tag.as_deref(), Some("Vormauerschale"));
    }

    #[test]
    fn effective_layers_for_wall_carries_new_attrs_through_resolution() {
        let mut styles = HashMap::new();
        let mut layer = create_layer("brick", 0.24);
        layer.hatch_override = Some("ANSI32".to_string());
        layer.role_tag = Some("Vormauerschale".to_string());
        styles.insert(
            "s".to_string(),
            create_wall_style("s", None, vec![layer]),
        );

        let resolved = effective_layers_for_wall_bb(&styles, &"s".to_string(), 0.24).unwrap();
        assert_eq!(resolved[0].hatch_override.as_deref(), Some("ANSI32"));
        assert_eq!(resolved[0].role_tag.as_deref(), Some("Vormauerschale"));
    }

    #[test]
    fn migrate_gap_before_matches_old_stacking() {
        // thickness/gap pairs from the historical contour layout.
        let old = vec![(0.1, 0.0), (0.1, 0.05)];
        let offsets = migrate_gap_before_to_axis_offset(&old);
        assert_eq!(offsets.len(), 2);
        assert!((offsets[0] - (-0.125)).abs() < 1e-12);
        assert!((offsets[1] - 0.025).abs() < 1e-12);
    }

    #[test]
    fn wall_style_deserializes_legacy_gap_before_to_axis_offset() {
        let json = r#"{
            "id": "s",
            "name": "S",
            "object_kind": "Wall",
            "parent_style_id": null,
            "layers": [
                {
                    "material_id": "a",
                    "thickness": 0.1,
                    "function": "Structural",
                    "gap_before": 0.0
                },
                {
                    "material_id": "b",
                    "thickness": 0.1,
                    "function": "Finish",
                    "gap_before": 0.05
                }
            ]
        }"#;
        let style: WallStyle = serde_json::from_str(json).unwrap();
        assert_eq!(style.layers.len(), 2);
        assert!((style.layers[0].axis_offset.as_fixed_or(0.0) - (-0.125)).abs() < 1e-12);
        assert!((style.layers[1].axis_offset.as_fixed_or(0.0) - 0.025).abs() < 1e-12);
    }
}
