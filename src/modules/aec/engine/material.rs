//! AEC Material definitions.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Unique identifier for a material.
pub type MaterialId = String;

fn default_hatch_scale() -> f64 {
    1.0
}

fn default_hatch_angle_relative() -> bool {
    true
}

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
    /// Optional grouping category (e.g. "Holz", "Metall").
    #[serde(default)]
    pub category: Option<String>,
    /// Optional hatch fill color (`AcadColor`). Falls back to `line_color`
    /// when `None`. Legacy bare `u32` 0xRRGGBB values still load as
    /// `AcadColor::Rgb`.
    #[serde(default)]
    #[serde(
        serialize_with = "serialize_option_acad_color",
        deserialize_with = "deserialize_option_acad_color"
    )]
    pub hatch_color: Option<acadrust::types::Color>,
    /// Hatch pattern scale factor. Defaults to `1.0`.
    #[serde(default = "default_hatch_scale")]
    pub hatch_scale: f64,
    /// Additional hatch rotation in degrees, applied on top of the base
    /// direction (see `hatch_angle_relative`). Defaults to `0.0`.
    #[serde(default)]
    pub hatch_angle: f64,
    /// When `true` (default), `hatch_angle` is added to the wall's own
    /// direction so the hatch pattern follows the wall run. When `false`,
    /// `hatch_angle` is used as an absolute, world/global angle instead.
    #[serde(default = "default_hatch_angle_relative")]
    pub hatch_angle_relative: bool,
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
            category: None,
            hatch_color: None,
            hatch_scale: 1.0,
            hatch_angle: 0.0,
            hatch_angle_relative: true,
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
        assert_eq!(mat.category, None);
        assert_eq!(mat.hatch_color, None);
        assert_eq!(mat.hatch_scale, 1.0);
        assert_eq!(mat.hatch_angle, 0.0);
        assert!(mat.hatch_angle_relative);
    }

    #[test]
    fn material_without_new_attrs_deserializes_with_defaults() {
        // Simulates an older saved library that predates `category`,
        // `hatch_color` and `hatch_scale`: all three must default rather than
        // failing to deserialize.
        let material_json = r#"{
            "id": "brick",
            "name": "Red Brick",
            "hatch_pattern": "ANSI31",
            "line_color": 16711680,
            "line_type": "Continuous",
            "render_material_ref": null
        }"#;
        let mat: Material = serde_json::from_str(material_json).unwrap();
        assert_eq!(mat.category, None);
        assert_eq!(mat.hatch_color, None);
        assert_eq!(mat.hatch_scale, 1.0);
        assert_eq!(mat.hatch_angle, 0.0);
        assert!(mat.hatch_angle_relative);
    }

    #[test]
    fn material_new_attrs_roundtrip() {
        let mut mat = Material::new(
            "wood".to_string(),
            "Oak".to_string(),
            "ANSI31".to_string(),
            0xC4A35A,
            "Continuous".to_string(),
        );
        mat.category = Some("Holz".to_string());
        mat.hatch_color = Some(acadrust::types::Color::Rgb {
            r: 0xA6,
            g: 0x7C,
            b: 0x52,
        });
        mat.hatch_scale = 0.5;
        mat.hatch_angle = 45.0;
        mat.hatch_angle_relative = false;
        mat.render_material_ref = Some("oak_pbr".to_string());

        let serialized = serde_json::to_string(&mat).unwrap();
        let deserialized: Material = serde_json::from_str(&serialized).unwrap();
        assert_eq!(mat, deserialized);
        assert_eq!(deserialized.category.as_deref(), Some("Holz"));
        assert_eq!(
            deserialized.hatch_color,
            Some(acadrust::types::Color::Rgb {
                r: 0xA6,
                g: 0x7C,
                b: 0x52
            })
        );
        assert!((deserialized.hatch_scale - 0.5).abs() < 1e-12);
        assert!((deserialized.hatch_angle - 45.0).abs() < 1e-12);
        assert!(!deserialized.hatch_angle_relative);
        assert_eq!(deserialized.render_material_ref.as_deref(), Some("oak_pbr"));
    }

    #[test]
    fn material_legacy_hatch_color_u32_deserializes_as_rgb() {
        // 0xA67C52 == 10_910_802
        let material_json = r#"{
            "id": "brick",
            "name": "Red Brick",
            "hatch_pattern": "ANSI31",
            "line_color": 16711680,
            "line_type": "Continuous",
            "render_material_ref": null,
            "hatch_color": 10910802
        }"#;
        let mat: Material = serde_json::from_str(material_json).unwrap();
        assert_eq!(
            mat.hatch_color,
            Some(acadrust::types::Color::Rgb {
                r: 0xA6,
                g: 0x7C,
                b: 0x52
            })
        );
    }

    #[test]
    fn material_hatch_angle_without_new_attrs_deserializes_with_defaults() {
        // Simulates a library saved before `hatch_angle`/`hatch_angle_relative`
        // existed but already had the other new fields.
        let material_json = r#"{
            "id": "brick",
            "name": "Red Brick",
            "hatch_pattern": "ANSI31",
            "line_color": 16711680,
            "line_type": "Continuous",
            "render_material_ref": null,
            "category": null,
            "hatch_color": null,
            "hatch_scale": 1.0
        }"#;
        let mat: Material = serde_json::from_str(material_json).unwrap();
        assert_eq!(mat.hatch_angle, 0.0);
        assert!(mat.hatch_angle_relative);
    }
}
