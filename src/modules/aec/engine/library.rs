//! AEC Style Library for persisting materials and wall styles.

use crate::modules::aec::engine::material::Material;
use crate::modules::aec::engine::style::Style;
use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, WallStyle};
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

/// A node in a hierarchical wall-style tree.
pub struct TreeNode<'a> {
    /// The wall style at this node.
    pub style: &'a WallStyle,
    /// Indentation depth (0 for roots).
    pub depth: usize,
}

impl StyleLibrary {
    /// An empty library (no materials, no wall styles).
    pub fn empty() -> Self {
        Self {
            materials: Vec::new(),
            wall_styles: Vec::new(),
        }
    }

    /// Inserts or replaces (by `id`) a material.
    pub fn upsert_material(&mut self, material: Material) {
        if let Some(existing) = self.materials.iter_mut().find(|m| m.id == material.id) {
            *existing = material;
        } else {
            self.materials.push(material);
        }
    }

    /// Inserts or replaces (by `style.id`) a wall style.
    pub fn upsert_wall_style(&mut self, wall_style: WallStyle) {
        if let Some(existing) = self
            .wall_styles
            .iter_mut()
            .find(|s| s.style.id == wall_style.style.id)
        {
            *existing = wall_style;
        } else {
            self.wall_styles.push(wall_style);
        }
    }

    /// Removes a material by `id`. Returns `true` if a material was removed.
    pub fn remove_material(&mut self, id: &str) -> bool {
        let before = self.materials.len();
        self.materials.retain(|m| m.id != id);
        self.materials.len() != before
    }

    /// Removes a wall style by `id`. Returns `true` if a wall style was removed.
    pub fn remove_wall_style(&mut self, id: &str) -> bool {
        let before = self.wall_styles.len();
        self.wall_styles.retain(|s| s.style.id != id);
        self.wall_styles.len() != before
    }

    /// Returns all wall styles in a hierarchical tree order: roots first,
    /// each followed immediately by its descendants, siblings sorted by name.
    pub fn wall_style_tree(&self) -> Vec<TreeNode<'_>> {
        let mut children: std::collections::HashMap<&str, Vec<&WallStyle>> =
            std::collections::HashMap::new();
        let mut roots: Vec<&WallStyle> = Vec::new();
        for ws in &self.wall_styles {
            match &ws.style.parent_style_id {
                Some(pid) if self.wall_styles.iter().any(|other| &other.style.id == pid) => {
                    children.entry(pid.as_str()).or_default().push(ws);
                }
                _ => roots.push(ws),
            }
        }
        roots.sort_by(|a, b| a.style.name.to_lowercase().cmp(&b.style.name.to_lowercase()));
        for siblings in children.values_mut() {
            siblings.sort_by(|a, b| a.style.name.to_lowercase().cmp(&b.style.name.to_lowercase()));
        }

        let mut ordered = Vec::with_capacity(self.wall_styles.len());
        let mut stack: Vec<(&WallStyle, usize)> = roots
            .into_iter()
            .rev()
            .map(|ws| (ws, 0))
            .collect();
        while let Some((ws, depth)) = stack.pop() {
            ordered.push(TreeNode { style: ws, depth });
            if let Some(kids) = children.get(ws.style.id.as_str()) {
                for kid in kids.iter().rev() {
                    stack.push((kid, depth + 1));
                }
            }
        }
        ordered
    }
}

/// Builds a small, ready-to-use default library so `AEC_WALL`'s style
/// selection has something to offer out of the box, without requiring the
/// user to define materials/styles first via `AEC_MATERIAL`/`AEC_STYLE`.
pub fn seed_default_library() -> StyleLibrary {
    let masonry = Material::new(
        "mat_masonry".to_string(),
        "Mauerwerk".to_string(),
        "ANSI31".to_string(),
        0x8B7355,
        "Continuous".to_string(),
    );
    let concrete = Material::new(
        "mat_concrete".to_string(),
        "Stahlbeton".to_string(),
        "AR-CONC".to_string(),
        0x888888,
        "Continuous".to_string(),
    );
    let insulation = Material::new(
        "mat_insulation".to_string(),
        "Daemmung".to_string(),
        "ANSI37".to_string(),
        0xFFDD88,
        "Continuous".to_string(),
    );
    let plaster = Material::new(
        "mat_plaster".to_string(),
        "Putz".to_string(),
        "SOLID".to_string(),
        0xFFFFFF,
        "Continuous".to_string(),
    );

    let masonry_wall = WallStyle {
        style: Style {
            id: "style_masonry".to_string(),
            name: "Wand Mauerwerk 24cm".to_string(),
            object_kind: "Wall".to_string(),
            parent_style_id: None,
        },
        layers: vec![Layer {
            material_id: masonry.id.clone(),
            thickness: 0.24,
            function: LayerFunction::Structural,
            gap_before: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
        }],
    };

    let concrete_wall = WallStyle {
        style: Style {
            id: "style_concrete_20".to_string(),
            name: "Wand Stahlbeton 20cm".to_string(),
            object_kind: "Wall".to_string(),
            parent_style_id: None,
        },
        layers: vec![Layer {
            material_id: concrete.id.clone(),
            thickness: 0.20,
            function: LayerFunction::Structural,
            gap_before: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
        }],
    };

    let insulated_wall = WallStyle {
        style: Style {
            id: "style_insulated_ext".to_string(),
            name: "Aussenwand gedaemmt".to_string(),
            object_kind: "Wall".to_string(),
            parent_style_id: None,
        },
        layers: vec![
            Layer {
                material_id: plaster.id.clone(),
                thickness: 0.015,
                function: LayerFunction::Finish,
                gap_before: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
            },
            Layer {
                material_id: masonry.id.clone(),
                thickness: 0.175,
                function: LayerFunction::Structural,
                gap_before: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
            },
            Layer {
                material_id: insulation.id.clone(),
                thickness: 0.14,
                function: LayerFunction::Insulation,
                gap_before: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
            },
            Layer {
                material_id: plaster.id.clone(),
                thickness: 0.015,
                function: LayerFunction::Finish,
                gap_before: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
            },
        ],
    };

    StyleLibrary {
        materials: vec![masonry, concrete, insulation, plaster],
        wall_styles: vec![masonry_wall, concrete_wall, insulated_wall],
    }
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

/// Persists `lib` to [`default_library_path`], creating parent directories
/// as needed. On `wasm32` there is no local filesystem to persist to, so
/// this is a no-op there.
pub fn save_to_default_path(lib: &StyleLibrary) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = default_library_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = to_toml(lib)?;
        return std::fs::write(&path, content).map_err(|e| e.to_string());
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = lib;
        Ok(())
    }
}

/// Loads the library from [`default_library_path`], or — if it does not
/// exist yet — creates it from [`seed_default_library`] and persists it, so
/// `AEC_WALL`'s style selection always has something to offer without
/// requiring the user to define materials/styles first. On `wasm32` there
/// is no local filesystem, so this always returns an in-memory seed.
pub fn load_or_seed() -> StyleLibrary {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = default_library_path();
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(lib) = from_toml(&content) {
                    return lib;
                }
            }
        }
    }
    let seed = seed_default_library();
    let _ = save_to_default_path(&seed);
    seed
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
                    gap_before: 0.0,
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    layer_override: None,
                },
                Layer {
                    material_id: "mat1".to_string(),
                    thickness: 5.0,
                    function: LayerFunction::Finish,
                    gap_before: 0.0,
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    layer_override: None,
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

    #[test]
    fn test_remove_material() {
        let material = Material::new(
            "mat1".to_string(),
            "Material 1".to_string(),
            "HATCH1".to_string(),
            0xFF0000,
            "Continuous".to_string(),
        );
        let mut lib = StyleLibrary {
            materials: vec![material],
            wall_styles: Vec::new(),
        };

        assert!(lib.remove_material("mat1"));
        assert!(lib.materials.is_empty());
        assert!(!lib.remove_material("mat1"));
    }

    #[test]
    fn test_remove_wall_style() {
        let wall_style = WallStyle {
            style: Style {
                id: "style1".to_string(),
                name: "Style 1".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: Vec::new(),
        };
        let mut lib = StyleLibrary {
            materials: Vec::new(),
            wall_styles: vec![wall_style],
        };

        assert!(lib.remove_wall_style("style1"));
        assert!(lib.wall_styles.is_empty());
        assert!(!lib.remove_wall_style("style1"));
    }

    #[test]
    fn test_wall_style_tree() {
        let mut lib = StyleLibrary::empty();
        let s1 = WallStyle {
            style: Style {
                id: "s1".to_string(),
                name: "Style A".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: vec![],
        };
        let s2 = WallStyle {
            style: Style {
                id: "s2".to_string(),
                name: "Style B".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: Some("s1".to_string()),
            },
            layers: vec![],
        };
        let s3 = WallStyle {
            style: Style {
                id: "s3".to_string(),
                name: "Style C".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: Some("s1".to_string()),
            },
            layers: vec![],
        };
        let s4 = WallStyle {
            style: Style {
                id: "s4".to_string(),
                name: "Style D".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: Some("orphan".to_string()),
            },
            layers: vec![],
        };

        lib.wall_styles = vec![s1, s2, s3, s4];
        let tree = lib.wall_style_tree();

        assert_eq!(tree.len(), 4);

        // s1 (root)
        assert_eq!(tree[0].style.style.id, "s1");
        assert_eq!(tree[0].depth, 0);

        // s1 children sorted by name: s2 (Style B) then s3 (Style C)
        assert_eq!(tree[1].style.style.id, "s2");
        assert_eq!(tree[1].depth, 1);
        assert_eq!(tree[2].style.style.id, "s3");
        assert_eq!(tree[2].depth, 1);

        // s4 (orphaned parent is root)
        assert_eq!(tree[3].style.style.id, "s4");
        assert_eq!(tree[3].depth, 0);
    }
}
