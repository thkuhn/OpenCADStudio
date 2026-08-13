//! AEC Style Library for persisting materials and wall styles.

use crate::modules::aec::engine::material::Material;
use crate::modules::aec::engine::wall_style::WallStyle;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A collection of AEC materials and wall styles.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StyleLibrary {
    /// List of available materials.
    pub materials: Vec<Material>,
    /// List of available wall styles.
    pub wall_styles: Vec<WallStyle>,
}

/// Serializes the library to a string.
///
/// Chosen format: JSON (via `serde_json`) because the `toml` crate is not
/// present in Cargo.toml.
pub fn to_toml(lib: &StyleLibrary) -> Result<String, String> {
    serde_json::to_string_pretty(lib).map_err(|e| e.to_string())
}

/// Deserializes the library from a string.
///
/// Chosen format: JSON (via `serde_json`) because the `toml` crate is not
/// present in Cargo.toml.
pub fn from_toml(s: &str) -> Result<StyleLibrary, String> {
    serde_json::from_str(s).map_err(|e| e.to_string())
}

/// Returns the default path for the AEC styles library file.
///
/// Reuses the application's standard configuration directory.
pub fn default_library_path() -> PathBuf {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(p) = crate::config::config_dir() {
            return p.join("aec_styles.toml");
        }
    }

    // Fallback if config_dir fails or on web
    let mut p = PathBuf::new();
    #[cfg(all(not(target_arch = "wasm32"), target_os = "linux"))]
    {
        if let Ok(home) = std::env::var("HOME") {
            p.push(home);
            p.push(".config");
        }
    }
    p.push("OpenCADStudio");
    p.push("aec_styles.toml");
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::material::Material;
    use crate::modules::aec::engine::style::Style;
    use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, WallStyle};

    #[test]
    fn test_library_roundtrip() {
        let material = Material::new(
            "mat1".to_string(),
            "Material 1".to_string(),
            "HATCH1".to_string(),
            0xFF0000,
            "Continuous".to_string(),
        );

        let wall_style = WallStyle {
            style: Style {
                id: "style1".to_string(),
                name: "Style 1".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: vec![
                Layer {
                    material_id: "mat1".to_string(),
                    thickness: 10.0,
                    function: LayerFunction::Structural,
                },
                Layer {
                    material_id: "mat1".to_string(),
                    thickness: 5.0,
                    function: LayerFunction::Finish,
                },
            ],
        };

        let lib = StyleLibrary {
            materials: vec![material],
            wall_styles: vec![wall_style],
        };

        let serialized = to_toml(&lib).expect("Serialization failed");
        let deserialized = from_toml(&serialized).expect("Deserialization failed");

        assert_eq!(lib, deserialized);
    }
}
