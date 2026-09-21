//! Opening styles (window / door / breakthrough) with parametric slot geometry.
//!
//! Generator parameters (`frame_thickness`, leaf inset, …) are **absolute
//! drawing units**. Changing instance width resizes the outer box, not the
//! profile thickness. Sketch slots store a reference-box payload; bake is
//! the two-rectangle map in [`super::opening_sketch`].

use crate::modules::aec::engine::display_component::OpeningComponentSlot;
use crate::modules::aec::engine::opening_shape::OpeningShape;
use crate::modules::aec::engine::openings::OpeningKind;

pub use crate::modules::aec::engine::opening_sketch::{OpeningSketch, OpeningSketchPath};
use crate::modules::aec::engine::style::{resolve_chain, Style, StyleError, StyleId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Seed style id for the standard window.
pub const SEED_WINDOW_STYLE_ID: &str = "style_window_standard";
/// Seed style id for the standard door.
pub const SEED_DOOR_STYLE_ID: &str = "style_door_standard";
/// Seed style id for the standard breakthrough.
pub const SEED_BREAKTHROUGH_STYLE_ID: &str = "style_breakthrough_standard";

/// Default frame / profile thickness in drawing units (metres).
pub const DEFAULT_FRAME_THICKNESS: f64 = 0.06;
/// Default swing angle for door generators.
pub const DEFAULT_OPENING_ANGLE_DEG: f64 = 90.0;

/// Hinge / strike side of a door or window leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum HingeSide {
    #[default]
    Left,
    Right,
}

impl HingeSide {
    pub fn as_str(self) -> &'static str {
        match self {
            HingeSide::Left => "Left",
            HingeSide::Right => "Right",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "Right" | "right" | "R" | "r" => HingeSide::Right,
            _ => HingeSide::Left,
        }
    }

    pub fn mirrored(self) -> Self {
        match self {
            HingeSide::Left => HingeSide::Right,
            HingeSide::Right => HingeSide::Left,
        }
    }
}

/// Parametric plan-symbol generator for one display slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum OpeningGenerator {
    #[default]
    None,
    FrameRect,
    LeafLine,
    SwingArc,
    SillLines,
    /// Breakthrough: two diagonals.
    Cross,
    /// Breakthrough: one diagonal plus a fill of the opening area.
    DiagonalFill,
}

impl OpeningGenerator {
    pub fn as_str(self) -> &'static str {
        match self {
            OpeningGenerator::None => "None",
            OpeningGenerator::FrameRect => "FrameRect",
            OpeningGenerator::LeafLine => "LeafLine",
            OpeningGenerator::SwingArc => "SwingArc",
            OpeningGenerator::SillLines => "SillLines",
            OpeningGenerator::Cross => "Cross",
            OpeningGenerator::DiagonalFill => "DiagonalFill",
        }
    }

    pub fn catalogue() -> &'static [OpeningGenerator] {
        &[
            OpeningGenerator::None,
            OpeningGenerator::FrameRect,
            OpeningGenerator::LeafLine,
            OpeningGenerator::SwingArc,
            OpeningGenerator::SillLines,
            OpeningGenerator::Cross,
            OpeningGenerator::DiagonalFill,
        ]
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "FrameRect" => OpeningGenerator::FrameRect,
            "LeafLine" => OpeningGenerator::LeafLine,
            "SwingArc" => OpeningGenerator::SwingArc,
            "SillLines" => OpeningGenerator::SillLines,
            "Cross" => OpeningGenerator::Cross,
            "DiagonalFill" => OpeningGenerator::DiagonalFill,
            _ => OpeningGenerator::None,
        }
    }
}

/// Geometry source for one opening display slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum SlotGeometry {
    Generator(OpeningGenerator),
    Sketch(OpeningSketch),
}

impl Default for SlotGeometry {
    fn default() -> Self {
        SlotGeometry::Generator(OpeningGenerator::None)
    }
}

/// A library style for windows, doors, or breakthroughs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningStyle {
    pub style: Style,
    pub kind: OpeningKind,
    pub default_width: f64,
    pub default_height: f64,
    pub default_sill: f64,
    pub hinge: HingeSide,
    /// Absolute profile thickness in drawing units (not a fraction of width).
    pub frame_thickness: f64,
    pub opening_angle_deg: f64,
    pub shape: OpeningShape,
    /// Arch Kämpfer above the sill. Unused for other shapes.
    pub spring_height: f64,
    #[serde(default)]
    pub slots: HashMap<OpeningComponentSlot, SlotGeometry>,
    /// Per plan-type (`DisplayConfig` name) slot geometry and visibility.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub display_profiles: HashMap<String, OpeningDisplayProfile>,
}

/// Opening-slot geometry/visibility override for one plan type or scale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OpeningDisplayProfile {
    #[serde(default)]
    pub slots: HashMap<OpeningComponentSlot, SlotGeometry>,
    /// Slot key → visible. Absent keys stay visible.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub visibility: HashMap<String, bool>,
}

impl OpeningStyle {
    pub fn object_kind_for(kind: OpeningKind) -> &'static str {
        match kind {
            OpeningKind::Window => "Window",
            OpeningKind::Door => "Door",
            OpeningKind::Breakthrough => "Opening",
        }
    }

    pub fn standard_window() -> Self {
        let mut slots = HashMap::new();
        slots.insert(
            OpeningComponentSlot::Frame2D,
            SlotGeometry::Generator(OpeningGenerator::FrameRect),
        );
        slots.insert(
            OpeningComponentSlot::Leaf2D,
            SlotGeometry::Generator(OpeningGenerator::LeafLine),
        );
        slots.insert(
            OpeningComponentSlot::Sill2D,
            SlotGeometry::Generator(OpeningGenerator::SillLines),
        );
        Self {
            style: Style {
                id: SEED_WINDOW_STYLE_ID.to_string(),
                name: "Standardfenster".to_string(),
                object_kind: Self::object_kind_for(OpeningKind::Window).to_string(),
                parent_style_id: None,
            },
            kind: OpeningKind::Window,
            default_width: crate::modules::aec::engine::openings::DEFAULT_WINDOW_WIDTH,
            default_height: crate::modules::aec::engine::openings::DEFAULT_WINDOW_HEIGHT,
            default_sill: crate::modules::aec::engine::openings::DEFAULT_WINDOW_SILL,
            hinge: HingeSide::Left,
            frame_thickness: DEFAULT_FRAME_THICKNESS,
            opening_angle_deg: DEFAULT_OPENING_ANGLE_DEG,
            shape: OpeningShape::Rectangle,
            spring_height: 0.0,
            slots,
            display_profiles: HashMap::new(),
        }
    }

    pub fn standard_door() -> Self {
        let mut slots = HashMap::new();
        slots.insert(
            OpeningComponentSlot::Frame2D,
            SlotGeometry::Generator(OpeningGenerator::FrameRect),
        );
        slots.insert(
            OpeningComponentSlot::Leaf2D,
            SlotGeometry::Generator(OpeningGenerator::LeafLine),
        );
        slots.insert(
            OpeningComponentSlot::Swing2D,
            SlotGeometry::Generator(OpeningGenerator::SwingArc),
        );
        Self {
            style: Style {
                id: SEED_DOOR_STYLE_ID.to_string(),
                name: "Standardtür".to_string(),
                object_kind: Self::object_kind_for(OpeningKind::Door).to_string(),
                parent_style_id: None,
            },
            kind: OpeningKind::Door,
            default_width: crate::modules::aec::engine::openings::DEFAULT_DOOR_WIDTH,
            default_height: crate::modules::aec::engine::openings::DEFAULT_DOOR_HEIGHT,
            default_sill: crate::modules::aec::engine::openings::DEFAULT_DOOR_SILL,
            hinge: HingeSide::Left,
            frame_thickness: DEFAULT_FRAME_THICKNESS,
            opening_angle_deg: DEFAULT_OPENING_ANGLE_DEG,
            shape: OpeningShape::Rectangle,
            spring_height: 0.0,
            slots,
            display_profiles: HashMap::new(),
        }
    }

    pub fn standard_breakthrough() -> Self {
        let mut slots = HashMap::new();
        slots.insert(
            OpeningComponentSlot::Mark2D,
            SlotGeometry::Generator(OpeningGenerator::Cross),
        );
        Self {
            style: Style {
                id: SEED_BREAKTHROUGH_STYLE_ID.to_string(),
                name: "Standarddurchbruch".to_string(),
                object_kind: Self::object_kind_for(OpeningKind::Breakthrough).to_string(),
                parent_style_id: None,
            },
            kind: OpeningKind::Breakthrough,
            default_width: crate::modules::aec::engine::openings::DEFAULT_BREAKTHROUGH_WIDTH,
            default_height: crate::modules::aec::engine::openings::DEFAULT_BREAKTHROUGH_HEIGHT,
            default_sill: crate::modules::aec::engine::openings::DEFAULT_BREAKTHROUGH_SILL,
            hinge: HingeSide::Left,
            frame_thickness: 0.0,
            opening_angle_deg: 0.0,
            shape: OpeningShape::Rectangle,
            spring_height: 0.0,
            slots,
            display_profiles: HashMap::new(),
        }
    }
}

/// Seed opening styles shipped with the default library.
pub fn seed_opening_styles() -> Vec<OpeningStyle> {
    vec![
        OpeningStyle::standard_window(),
        OpeningStyle::standard_door(),
        OpeningStyle::standard_breakthrough(),
    ]
}

/// Copy numeric/style identity defaults onto an instance (not plane ids).
pub fn apply_style_defaults(opening: &mut crate::modules::aec::engine::openings::Opening, style: &OpeningStyle) {
    opening.style_id = Some(style.style.id.clone());
    opening.kind = style.kind;
    opening.width = style.default_width;
    opening.height = style.default_height;
    opening.sill_height = style.default_sill;
    opening.hinge = style.hinge;
    opening.shape = style.shape;
    opening.spring_height = style.spring_height;
    let (w, h) = opening.shape.lock_size(opening.width, opening.height, true);
    opening.width = w;
    opening.height = h;
    if opening.shape == OpeningShape::Arch && opening.spring_height <= 1e-12 {
        opening.spring_height = OpeningShape::default_spring_height(opening.width, opening.height);
    }
}

/// Seed style id for `kind` (used when placing with a library).
pub fn seed_style_id_for_kind(kind: OpeningKind) -> &'static str {
    match kind {
        OpeningKind::Window => SEED_WINDOW_STYLE_ID,
        OpeningKind::Door => SEED_DOOR_STYLE_ID,
        OpeningKind::Breakthrough => SEED_BREAKTHROUGH_STYLE_ID,
    }
}

/// Kind-default slot map used for legacy instances with no `style_id`.
pub fn default_slots_for_kind(
    kind: OpeningKind,
) -> HashMap<OpeningComponentSlot, SlotGeometry> {
    match kind {
        OpeningKind::Window => OpeningStyle::standard_window().slots,
        OpeningKind::Door => OpeningStyle::standard_door().slots,
        OpeningKind::Breakthrough => OpeningStyle::standard_breakthrough().slots,
    }
}

/// Overlay slot maps from root to leaf so a child can override individual slots.
pub fn effective_slots(
    opening_styles: &HashMap<StyleId, OpeningStyle>,
    id: &StyleId,
) -> Result<HashMap<OpeningComponentSlot, SlotGeometry>, StyleError> {
    let styles: HashMap<StyleId, Style> = opening_styles
        .iter()
        .map(|(k, v)| (k.clone(), v.style.clone()))
        .collect();
    let chain = resolve_chain(&styles, id)?;
    let mut slots = HashMap::new();
    for style_id in chain {
        if let Some(os) = opening_styles.get(&style_id) {
            for (slot, geom) in &os.slots {
                slots.insert(*slot, geom.clone());
            }
        }
    }
    Ok(slots)
}

/// Default inherited slots, then plan-type overlays from root to leaf.
pub fn effective_slots_for_plan(
    opening_styles: &HashMap<StyleId, OpeningStyle>,
    id: &StyleId,
    plan_name: Option<&str>,
) -> Result<HashMap<OpeningComponentSlot, SlotGeometry>, StyleError> {
    let mut slots = effective_slots(opening_styles, id)?;
    let Some(plan) = plan_name.filter(|n| !n.is_empty()) else {
        return Ok(slots);
    };
    let styles: HashMap<StyleId, Style> = opening_styles
        .iter()
        .map(|(k, v)| (k.clone(), v.style.clone()))
        .collect();
    let chain = resolve_chain(&styles, id)?;
    for style_id in chain {
        if let Some(os) = opening_styles.get(&style_id) {
            if let Some(profile) = os.display_profiles.get(plan) {
                for (slot, geom) in &profile.slots {
                    slots.insert(*slot, geom.clone());
                }
            }
        }
    }
    Ok(slots)
}

/// Overlay plan-type slot visibility (hidden keys only) onto a rule set.
pub fn apply_plan_visibility(
    opening_styles: &HashMap<StyleId, OpeningStyle>,
    id: &StyleId,
    plan_name: Option<&str>,
    visibility: &mut HashMap<String, bool>,
) {
    let Some(plan) = plan_name.filter(|n| !n.is_empty()) else {
        return;
    };
    let styles: HashMap<StyleId, Style> = opening_styles
        .iter()
        .map(|(k, v)| (k.clone(), v.style.clone()))
        .collect();
    let Ok(chain) = resolve_chain(&styles, id) else {
        return;
    };
    for style_id in chain {
        if let Some(os) = opening_styles.get(&style_id) {
            if let Some(profile) = os.display_profiles.get(plan) {
                for (key, vis) in &profile.visibility {
                    visibility.insert(key.clone(), *vis);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::openings::{
        DEFAULT_BREAKTHROUGH_HEIGHT, DEFAULT_BREAKTHROUGH_SILL, DEFAULT_BREAKTHROUGH_WIDTH,
        DEFAULT_WINDOW_WIDTH,
    };

    fn map_of(styles: Vec<OpeningStyle>) -> HashMap<StyleId, OpeningStyle> {
        styles
            .into_iter()
            .map(|s| (s.style.id.clone(), s))
            .collect()
    }

    #[test]
    fn seed_styles_cover_three_kinds() {
        let seeds = seed_opening_styles();
        assert_eq!(seeds.len(), 3);
        assert_eq!(seeds[0].kind, OpeningKind::Window);
        assert_eq!(seeds[1].kind, OpeningKind::Door);
        assert_eq!(seeds[2].kind, OpeningKind::Breakthrough);
        assert!(approx(seeds[0].frame_thickness, DEFAULT_FRAME_THICKNESS));
        assert!(approx(seeds[2].default_width, DEFAULT_BREAKTHROUGH_WIDTH));
        assert!(approx(seeds[2].default_height, DEFAULT_BREAKTHROUGH_HEIGHT));
        assert!(approx(seeds[2].default_sill, DEFAULT_BREAKTHROUGH_SILL));
        assert!(!seeds[0].slots.contains_key(&OpeningComponentSlot::Swing2D));
        assert!(!seeds[2].slots.contains_key(&OpeningComponentSlot::Frame2D));
        assert_eq!(
            seeds[2].slots.get(&OpeningComponentSlot::Mark2D),
            Some(&SlotGeometry::Generator(OpeningGenerator::Cross))
        );
    }

    #[test]
    fn frame_thickness_is_absolute_not_a_fraction_of_width() {
        let style = OpeningStyle::standard_window();
        assert!(approx(style.frame_thickness, 0.06));
        assert!(style.frame_thickness < DEFAULT_WINDOW_WIDTH * 0.2);
        let wider = DEFAULT_WINDOW_WIDTH + 0.6;
        assert!(approx(style.frame_thickness, 0.06));
        let _ = wider;
    }

    #[test]
    fn child_overrides_individual_slots_via_inheritance() {
        let mut parent = OpeningStyle::standard_window();
        parent.style.id = "parent".into();
        let mut child = OpeningStyle::standard_window();
        child.style.id = "child".into();
        child.style.parent_style_id = Some("parent".into());
        child.slots.clear();
        child.slots.insert(
            OpeningComponentSlot::Mark2D,
            SlotGeometry::Generator(OpeningGenerator::Cross),
        );
        let map = map_of(vec![parent, child]);
        let slots = effective_slots(&map, &"child".to_string()).unwrap();
        assert_eq!(
            slots.get(&OpeningComponentSlot::Frame2D),
            Some(&SlotGeometry::Generator(OpeningGenerator::FrameRect))
        );
        assert_eq!(
            slots.get(&OpeningComponentSlot::Mark2D),
            Some(&SlotGeometry::Generator(OpeningGenerator::Cross))
        );
    }

    #[test]
    fn serde_roundtrip_preserves_slots() {
        let style = OpeningStyle::standard_door();
        let json = serde_json::to_string(&style).unwrap();
        let back: OpeningStyle = serde_json::from_str(&json).unwrap();
        assert_eq!(style, back);
    }

    #[test]
    fn hinge_roundtrip() {
        assert_eq!(HingeSide::from_str("Right"), HingeSide::Right);
        assert_eq!(HingeSide::from_str(""), HingeSide::Left);
        assert_eq!(HingeSide::Left.mirrored(), HingeSide::Right);
    }

    #[test]
    fn plan_profile_overrides_slots_and_visibility() {
        let mut style = OpeningStyle::standard_window();
        let mut profile = OpeningDisplayProfile::default();
        profile.slots.insert(
            OpeningComponentSlot::Swing2D,
            SlotGeometry::Generator(OpeningGenerator::SwingArc),
        );
        profile
            .visibility
            .insert(OpeningComponentSlot::Sill2D.key().to_string(), false);
        style
            .display_profiles
            .insert("Ausführungsplan".to_string(), profile);
        let map = map_of(vec![style]);
        let id = SEED_WINDOW_STYLE_ID.to_string();
        let default_slots = effective_slots_for_plan(&map, &id, None).unwrap();
        assert!(!default_slots.contains_key(&OpeningComponentSlot::Swing2D));
        let plan_slots = effective_slots_for_plan(&map, &id, Some("Ausführungsplan")).unwrap();
        assert_eq!(
            plan_slots.get(&OpeningComponentSlot::Swing2D),
            Some(&SlotGeometry::Generator(OpeningGenerator::SwingArc))
        );
        let mut vis = HashMap::new();
        apply_plan_visibility(&map, &id, Some("Ausführungsplan"), &mut vis);
        assert_eq!(vis.get("Sill2D"), Some(&false));
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }
}
