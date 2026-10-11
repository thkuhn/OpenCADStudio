//! Shared scaffold for all AEC style kinds (`AEC_STYLES`).
//!
//! [`StyleCommon`] (`id`, `name`, `parent`, `category`, `origin`) is embedded
//! as the persisted `style` field of every style type (wall, opening, slab,
//! structural slab, floor finish; room stamp later); `style::Style` is an alias
//! of it. [`StyleKindOps`] gives the style manager a uniform handle.

use crate::modules::aec::engine::library::LibrarySource;
use crate::modules::aec::engine::material::Material;
use crate::modules::aec::engine::opening_style::OpeningStyle;
use crate::modules::aec::engine::slab_style::{FloorFinishStyle, SlabStructuralStyle, SlabStyle};
use crate::modules::aec::engine::wall_style::WallStyle;
use serde::{Deserialize, Serialize};

/// Where a style comes from. Precedence: `Default` < `Project` < `Session`.
/// Runtime-only (derived from the library a style was found in, never stored
/// inside the style itself).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
pub enum StyleOrigin {
    #[default]
    Default,
    Project,
    Session,
}

impl From<LibrarySource> for StyleOrigin {
    fn from(src: LibrarySource) -> Self {
        match src {
            LibrarySource::Standard => StyleOrigin::Default,
            LibrarySource::Project => StyleOrigin::Project,
            LibrarySource::Session => StyleOrigin::Session,
        }
    }
}

/// Category tag of the tree group a style kind belongs to.
pub const CATEGORY_MATERIAL: &str = "Material";
pub const CATEGORY_WALL: &str = "Wall";
pub const CATEGORY_OPENING: &str = "Opening";
pub const CATEGORY_SLAB: &str = "Slab";
pub const CATEGORY_STRUCTURAL_SLAB: &str = "SlabStructural";
pub const CATEGORY_FLOOR_FINISH: &str = "FloorFinish";

/// Uniform scaffold shared by all style kinds; embedded as the persisted
/// `style` field (flattened) of every style type. `origin` is derived from the
/// library a style lives in and is therefore never stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleCommon {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub parent: Option<String>,
    pub category: String,
    #[serde(default, skip_serializing)]
    pub origin: StyleOrigin,
}

/// Content equality: `origin` is provenance (derived from the library a style
/// lives in), not content, so it is ignored — otherwise identical styles from
/// different libraries would report a false copy conflict.
impl PartialEq for StyleCommon {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.name == other.name
            && self.parent == other.parent
            && self.category == other.category
    }
}

impl Eq for StyleCommon {}

impl StyleCommon {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        parent: Option<String>,
        category: impl Into<String>,
        origin: StyleOrigin,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            parent,
            category: category.into(),
            origin,
        }
    }

    pub fn with_origin(mut self, origin: StyleOrigin) -> Self {
        self.origin = origin;
        self
    }
}

/// Implemented by every style kind; gives the style manager a uniform handle.
pub trait StyleKindOps {
    /// Category tag of this kind (`CATEGORY_*`).
    fn kind_category() -> &'static str;
    /// Fluent key of the kind label (tree node title).
    fn kind_label_key() -> &'static str;
    /// Persisted common scaffold of this style.
    fn common(&self) -> &StyleCommon;
    fn common_mut(&mut self) -> &mut StyleCommon;
}

macro_rules! impl_style_kind_ops {
    ($ty:ty, $cat:expr, $key:expr, |$s:ident| $style:expr) => {
        impl StyleKindOps for $ty {
            fn kind_category() -> &'static str {
                $cat
            }
            fn kind_label_key() -> &'static str {
                $key
            }
            fn common(&self) -> &StyleCommon {
                let $s = self;
                $style
            }
            fn common_mut(&mut self) -> &mut StyleCommon {
                let $s = self;
                &mut $s.style
            }
        }
    };
}

impl_style_kind_ops!(WallStyle, CATEGORY_WALL, "aec-styles-kind-wall", |s| &s.style);
impl_style_kind_ops!(OpeningStyle, CATEGORY_OPENING, "aec-styles-kind-opening", |s| &s.style);
impl_style_kind_ops!(SlabStyle, CATEGORY_SLAB, "aec-styles-kind-slab", |s| &s.style);
impl_style_kind_ops!(
    SlabStructuralStyle,
    CATEGORY_STRUCTURAL_SLAB,
    "aec-styles-kind-structural",
    |s| &s.style
);
impl_style_kind_ops!(
    FloorFinishStyle,
    CATEGORY_FLOOR_FINISH,
    "aec-styles-kind-finish",
    |s| &s.style
);

impl_style_kind_ops!(Material, CATEGORY_MATERIAL, "aec-styles-kind-material", |s| &s.style);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_precedence_and_library_source() {
        assert!(StyleOrigin::Default < StyleOrigin::Project);
        assert!(StyleOrigin::Project < StyleOrigin::Session);
        assert_eq!(StyleOrigin::from(LibrarySource::Standard), StyleOrigin::Default);
        assert_eq!(StyleOrigin::from(LibrarySource::Session), StyleOrigin::Session);
    }

    #[test]
    fn common_roundtrip_and_wall_view() {
        let mut ws = SlabStyle::new("w1", "Wand 1");
        ws.common_mut().parent = Some("base".into());
        let c = ws.common().clone().with_origin(StyleOrigin::Project);
        assert_eq!(c.id, "w1");
        assert_eq!(c.parent.as_deref(), Some("base"));
        assert_eq!(c.category, CATEGORY_SLAB);
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<StyleCommon>(&json).unwrap(), c.clone().with_origin(StyleOrigin::Default));
        // Origin is runtime-only: never written, defaults when read.
        assert!(!json.contains("origin"));
        assert_eq!(serde_json::from_str::<StyleCommon>(&json).unwrap().origin, StyleOrigin::Default);
    }

    #[test]
    fn scaffold_is_persisted_flat_without_origin() {
        let ws = SlabStyle::new("w1", "Wand 1");
        let v: serde_json::Value = serde_json::to_value(&ws).unwrap();
        // The common scaffold is flattened into the stored record; origin is
        // runtime-only and must not leak into persisted data.
        assert!(v.get("origin").is_none());
        assert_eq!(v["id"], "w1");
        assert_eq!(v["category"], "Slab");
    }

    #[test]
    fn material_embeds_common_scaffold() {
        let mut m = Material::new("m1".into(), "Beton".into(), "ANSI31".into(), 0x808080, "Continuous".into());
        m.group = Some("Beton".into());
        assert_eq!(m.common().category, CATEGORY_MATERIAL);
        assert_eq!(m.id, "m1"); // via Deref
        let v: serde_json::Value = serde_json::to_value(&m).unwrap();
        assert_eq!(v["id"], "m1");
        assert_eq!(v["category"], "Material");
        assert_eq!(v["group"], "Beton");
        assert!(v.get("origin").is_none());
        let back: Material = serde_json::from_value(v).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn constructors_use_kind_category_constants() {
        use crate::modules::aec::engine::slab_style::{FloorFinishStyle, SlabStructuralStyle};
        assert_eq!(WallStyle::kind_category(), CATEGORY_WALL);
        let wall = WallStyle {
            style: StyleCommon::new("w", "W", None, CATEGORY_WALL, StyleOrigin::Default),
            layers: vec![],
            default_profile: None,
        };
        assert_eq!(wall.common().category, WallStyle::kind_category());
        assert_eq!(OpeningStyle::standard_window().common().category, OpeningStyle::kind_category());
        assert_eq!(OpeningStyle::standard_door().common().category, CATEGORY_OPENING);
        assert_eq!(SlabStyle::new("s", "S").common().category, SlabStyle::kind_category());
        assert_eq!(
            SlabStructuralStyle::new("s", "S").common().category,
            SlabStructuralStyle::kind_category()
        );
        assert_eq!(FloorFinishStyle::new("s", "S").common().category, FloorFinishStyle::kind_category());
    }

    #[test]
    fn origin_is_ignored_by_equality() {
        let a = StyleCommon::new("a", "A", None, CATEGORY_WALL, StyleOrigin::Default);
        let b = a.clone().with_origin(StyleOrigin::Project);
        assert_eq!(a, b);
        let mut ws = SlabStyle::new("s", "S");
        let plain = ws.clone();
        ws.common_mut().origin = StyleOrigin::Session;
        assert_eq!(ws, plain);
    }
}
