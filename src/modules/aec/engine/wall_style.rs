//! AEC Wall Style definitions.

use crate::modules::aec::engine::material::MaterialId;
use crate::modules::aec::engine::style::{resolve_chain, Style, StyleError, StyleId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

/// A single layer within a wall buildup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    /// Identifier of the material for this layer.
    pub material_id: MaterialId,
    /// Thickness of the layer in drawing units.
    pub thickness: f64,
    /// Functional role of this layer.
    pub function: LayerFunction,
}

/// A style defining the layered buildup of a wall.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WallStyle {
    /// Generic style properties and inheritance.
    #[serde(flatten)]
    pub style: Style,
    /// List of layers from exterior to interior.
    /// An empty list means "inherits parent's layers".
    pub layers: Vec<Layer>,
}

/// Resolves the effective layers for a wall style by traversing the inheritance chain.
///
/// Returns the layers of the nearest ancestor (including self) that has a non-empty
/// `layers` list. If no ancestor has layers, returns an empty vector.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_wall_style(id: &str, parent: Option<&str>, layers: Vec<Layer>) -> WallStyle {
        WallStyle {
            style: Style {
                id: id.to_string(),
                name: format!("Wall Style {}", id),
                object_kind: "Wall".to_string(),
                parent_style_id: parent.map(|s| s.to_string()),
            },
            layers,
        }
    }

    fn create_layer(mat_id: &str, thick: f64) -> Layer {
        Layer {
            material_id: mat_id.to_string(),
            thickness: thick,
            function: LayerFunction::Structural,
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
        styles.insert(
            "A".to_string(),
            create_wall_style("A", None, vec![]),
        );
        styles.insert(
            "B".to_string(),
            create_wall_style("B", Some("A"), vec![]),
        );

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
}
