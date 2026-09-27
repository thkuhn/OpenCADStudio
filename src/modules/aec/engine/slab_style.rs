//! AEC Slab Style definitions.
//!
//! A [`SlabStyle`] defines the parametric multi-layer recipe for a slab,
//! supporting fixed and formula-based thicknesses, display profiles across
//! plan types, and style inheritance.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::display_component::ComponentRuleSet;
use super::material::MaterialId;
use super::slab::SlabLayer;
use super::style::{resolve_chain, Style, StyleError, StyleId};
pub use super::wall_style::{LayerFunction, LayerValue};

/// A layer definition within a [`SlabStyle`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlabStyleLayer {
    /// Identifier of the material assigned to this layer.
    pub material_id: MaterialId,
    /// Layer thickness (either a fixed metric distance or an arithmetic formula).
    pub thickness: LayerValue,
    /// Functional category of the layer (Structural, Insulation, Finish, Other).
    pub function: LayerFunction,
    /// Vertical offset (optional adjustment).
    #[serde(default = "default_zero_layer_value")]
    pub vertical_offset: LayerValue,
    /// Optional CAD layer override.
    #[serde(default)]
    pub layer_override: Option<String>,
    /// Optional hatch pattern override.
    #[serde(default)]
    pub hatch_override: Option<String>,
    /// Optional role tag.
    #[serde(default)]
    pub role_tag: Option<String>,
    /// Stable layer UUID.
    #[serde(default = "Uuid::new_v4")]
    pub layer_id: Uuid,
}

fn default_zero_layer_value() -> LayerValue {
    LayerValue::Fixed(0.0)
}

impl SlabStyleLayer {
    pub fn new(material_id: impl Into<MaterialId>, thickness: LayerValue, function: LayerFunction) -> Self {
        Self {
            material_id: material_id.into(),
            thickness,
            function,
            vertical_offset: LayerValue::Fixed(0.0),
            layer_override: None,
            hatch_override: None,
            role_tag: None,
            layer_id: Uuid::new_v4(),
        }
    }

    pub fn is_structural(&self) -> bool {
        self.function == LayerFunction::Structural
    }
}

/// A resolved layer evaluated with concrete numeric values.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedSlabLayer {
    pub material_id: MaterialId,
    pub thickness: f64,
    pub function: LayerFunction,
    pub vertical_offset: f64,
    pub layer_override: Option<String>,
    pub hatch_override: Option<String>,
    pub role_tag: Option<String>,
    pub layer_id: Uuid,
    pub formula_error: Option<String>,
}

impl ResolvedSlabLayer {
    pub fn to_slab_layer(&self, material_name: &str) -> SlabLayer {
        SlabLayer {
            material: material_name.to_string(),
            thickness: self.thickness,
            function: self.function.clone(),
            vertical_offset: self.vertical_offset,
            layer_override: self.layer_override.clone(),
            hatch_override: self.hatch_override.clone(),
            role_tag: self.role_tag.clone(),
            layer_id: self.layer_id,
        }
    }
}

/// Parametric style schema for multi-layer slabs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlabStyle {
    #[serde(flatten)]
    pub style: Style,
    #[serde(default)]
    pub layers: Vec<SlabStyleLayer>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub display_profiles: HashMap<String, ComponentRuleSet>,
}

impl SlabStyle {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            style: Style {
                id: id.into(),
                name: name.into(),
                object_kind: "Slab".to_string(),
                parent_style_id: None,
            },
            layers: Vec::new(),
            display_profiles: HashMap::new(),
        }
    }

    pub fn with_layers(mut self, layers: Vec<SlabStyleLayer>) -> Self {
        self.layers = layers;
        self
    }

    /// Total nominal thickness of this style's fixed layers in meters.
    pub fn nominal_thickness(&self) -> f64 {
        base_thickness_from_layers(&self.layers)
    }
}

/// Sums the fixed thickness of all layers (formulas count as 0.0).
pub fn base_thickness_from_layers(layers: &[SlabStyleLayer]) -> f64 {
    layers
        .iter()
        .map(|l| l.thickness.as_fixed_or(0.0))
        .sum()
}

/// Standard variables dictionary for slab formula evaluation.
pub fn slab_vars(nominal_thickness: f64) -> HashMap<String, f64> {
    let mut vars = HashMap::new();
    vars.insert("D".to_string(), nominal_thickness);
    vars.insert("THICKNESS".to_string(), nominal_thickness);
    vars.insert("T".to_string(), nominal_thickness);
    vars
}

/// Resolves effective layer stack through the style inheritance chain.
pub fn effective_layers(
    styles: &HashMap<String, SlabStyle>,
    start_id: &str,
) -> Result<Vec<SlabStyleLayer>, StyleError> {
    let generic_styles: HashMap<StyleId, Style> = styles
        .iter()
        .map(|(id, ss)| (id.clone(), ss.style.clone()))
        .collect();
    let chain = resolve_chain(&generic_styles, &start_id.to_string())?;

    for style_id in &chain {
        if let Some(ss) = styles.get(style_id) {
            if !ss.layers.is_empty() {
                return Ok(ss.layers.clone());
            }
        }
    }
    Ok(Vec::new())
}

/// Resolves effective layers evaluating any arithmetic formulas against `vars`.
pub fn effective_layers_for_slab_vars(
    styles: &HashMap<String, SlabStyle>,
    start_id: &str,
    vars: &HashMap<String, f64>,
) -> Result<Vec<ResolvedSlabLayer>, StyleError> {
    let unresolved = effective_layers(styles, start_id)?;
    let mut resolved = Vec::with_capacity(unresolved.len());

    for l in unresolved {
        let (thickness, formula_error) = match l.thickness.resolve(vars) {
            Ok(v) => (v, None),
            Err(e) => (0.0, Some(e)),
        };
        let vertical_offset = l.vertical_offset.resolve(vars).unwrap_or(0.0);
        resolved.push(ResolvedSlabLayer {
            material_id: l.material_id,
            thickness,
            function: l.function,
            vertical_offset,
            layer_override: l.layer_override,
            hatch_override: l.hatch_override,
            role_tag: l.role_tag,
            layer_id: l.layer_id,
            formula_error,
        });
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slab_style_creation_and_thickness() {
        let style = SlabStyle::new("slab_conc_20", "Stahlbetondecke 20cm").with_layers(vec![
            SlabStyleLayer::new("mat_concrete", LayerValue::Fixed(0.20), LayerFunction::Structural),
        ]);

        assert_eq!(style.style.id, "slab_conc_20");
        assert_eq!(style.style.name, "Stahlbetondecke 20cm");
        assert_eq!(style.style.object_kind, "Slab");
        assert!((style.nominal_thickness() - 0.20).abs() < 1e-6);
    }

    #[test]
    fn test_slab_style_inheritance() {
        let mut styles = HashMap::new();

        let parent = SlabStyle::new("parent_slab", "Parent Base Slab").with_layers(vec![
            SlabStyleLayer::new("mat_concrete", LayerValue::Fixed(0.24), LayerFunction::Structural),
            SlabStyleLayer::new("mat_plaster", LayerValue::Fixed(0.015), LayerFunction::Finish),
        ]);
        styles.insert(parent.style.id.clone(), parent);

        let mut child = SlabStyle::new("child_slab", "Child Slab");
        child.style.parent_style_id = Some("parent_slab".to_string());
        styles.insert(child.style.id.clone(), child);

        let layers = effective_layers(&styles, "child_slab").expect("resolve");
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].material_id, "mat_concrete");
        assert_eq!(layers[1].material_id, "mat_plaster");
    }

    #[test]
    fn test_slab_style_formula_resolution() {
        let mut styles = HashMap::new();
        let style = SlabStyle::new("var_slab", "Variable Slab").with_layers(vec![
            SlabStyleLayer::new("mat_screed", LayerValue::Formula("D * 0.2".to_string()), LayerFunction::Other("Screed".to_string())),
            SlabStyleLayer::new("mat_concrete", LayerValue::Formula("D * 0.8".to_string()), LayerFunction::Structural),
        ]);
        styles.insert(style.style.id.clone(), style);

        let vars = slab_vars(0.30);
        let resolved = effective_layers_for_slab_vars(&styles, "var_slab", &vars).expect("resolve");
        assert_eq!(resolved.len(), 2);
        assert!((resolved[0].thickness - 0.06).abs() < 1e-6);
        assert!((resolved[1].thickness - 0.24).abs() < 1e-6);
    }
}
