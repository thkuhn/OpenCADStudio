//! AEC Material definitions.

use serde::{Deserialize, Serialize};

/// Unique identifier for a material.
pub type MaterialId = String;

/// A material definition for AEC objects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Material {
    /// Unique identifier for the material.
    pub id: MaterialId,
    /// Human-readable name of the material.
    pub name: String,
    /// Pattern name for cross-section hatching.
    pub hatch_pattern: String,
    /// Color of lines in 2D representation (0xRRGGBB).
    pub line_color: u32,
    /// Type of lines in 2D representation (e.g., "Continuous").
    pub line_type: String,
    /// Optional reference to a 3D render material.
    pub render_material_ref: Option<String>,
}

impl Material {
    /// Creates a new material with default render material reference.
    pub fn new(
        id: MaterialId,
        name: String,
        hatch_pattern: String,
        line_color: u32,
        line_type: String,
    ) -> Self {
        Self {
            id,
            name,
            hatch_pattern,
            line_color,
            line_type,
            render_material_ref: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_material_construction() {
        let mat = Material::new(
            "brick".to_string(),
            "Red Brick".to_string(),
            "ANSI31".to_string(),
            0xFF0000,
            "Continuous".to_string(),
        );

        assert_eq!(mat.id, "brick");
        assert_eq!(mat.name, "Red Brick");
        assert_eq!(mat.hatch_pattern, "ANSI31");
        assert_eq!(mat.line_color, 0xFF0000);
        assert_eq!(mat.line_type, "Continuous");
        assert_eq!(mat.render_material_ref, None);
    }
}
