//! Centralized view primitive style and material resolver cascade for 2D views.
//!
//! Evaluates the style cascade:
//! `ModelView.graphic_overrides` > `DisplayConfig` (`ComponentRuleSet` / slot overrides / phase
//! filter) > Bauteilstil (`WallStyle` / `SlabStyle` / `OpeningStyle`) > `Material` > fallback role.
//!
//! The `DisplayConfig` part is **not** re-implemented here: the effective rule sets are built once
//! per style through the canonical `library::build_effective_rule_set` /
//! `build_effective_slab_rule_set` (style default profile, plan `style_overrides`, detail switches,
//! layer filters, opening profiles) and layer/slot styles are resolved through
//! `display_apply::resolve_layer_style_override` / `layer_ref_matches`, exactly like the plan
//! regeneration in `wall_regen`. Only the `ModelView` tier and the material fallback live on top.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use acadrust::types::Color;

use crate::modules::aec::engine::display_apply::{
    apply_phase_filter, layer_ref_matches, resolve_layer_style_override,
};
use crate::modules::aec::engine::display_component::{
    ComponentRuleSet, ComponentStyleOverride, LayerSelection, OpeningComponentSlot,
    RepresentationMode, SlabComponentSlot, WallComponentSlot,
};
use crate::modules::aec::engine::join::LayerRef;
use crate::modules::aec::engine::library::{
    build_effective_rule_set, build_effective_slab_rule_set, effective_representation,
    resolve_effective_rule_set, DisplayConfigLibrary, StyleLibrary,
};
use crate::modules::aec::engine::material::Material;
use crate::modules::aec::engine::opening_style::{
    apply_plan_visibility, default_style_for_kind, OpeningLookRole, OpeningStyle,
};
use crate::modules::aec::engine::plan_view::{
    DisplayConfig, LayerDetail, PlanningStage, ViewType,
};
use crate::modules::aec::engine::slab_style::SlabStyle;
use crate::modules::aec::engine::views::model_view::{
    GraphicOverride, GraphicPatch, ModelView, OverrideSelector, StyleOverride,
};
use crate::modules::aec::engine::views::provenance::{ResolvedStyle, Rgba8};
use crate::modules::aec::engine::views::scene_collector::{
    CollectedOpening, CollectedSlab, CollectedWall,
};
use crate::modules::aec::engine::views::{ComponentId, ElementRef};
use crate::modules::aec::engine::wall_style::WallStyle;

use super::PaperMm;

/// Last-resort hatch patterns, used only when neither the override chain, the layer nor the
/// material provides a pattern (mirrors `wall_regen` / `slab_regen`).
const FALLBACK_WALL_HATCH: &str = "ANSI31";
const FALLBACK_SLAB_HATCH: &str = "AR-CONC";

/// Resolves the plan art used by a derived view without coupling its graphics to drawing scale.
pub fn resolve_view_display_config(
    library: &DisplayConfigLibrary,
    view_type: ViewType,
    stored_profile: Option<uuid::Uuid>,
    active_name: Option<&str>,
) -> DisplayConfig {
    if let Some(config) = stored_profile.and_then(|id| library.configs.iter().find(|cfg| cfg.id == id)) {
        return config.clone();
    }

    let active_config = active_name
        .and_then(|name| library.configs.iter().find(|cfg| cfg.name == name));
    if let Some(config) = active_config.filter(|cfg| cfg.view_type == view_type) {
        return config.clone();
    }

    if let Some(active_config) = active_config {
        if let Some(config) = library.configs.iter().find(|cfg| {
            cfg.view_type == view_type && cfg.planning_stage == active_config.planning_stage
        }) {
            return config.clone();
        }
        return active_config.clone();
    }

    DisplayConfig::new(
        "Neutral".to_string(),
        "AEC".to_string(),
        PlanningStage::Design,
        view_type,
    )
}

/// Converts an `acadrust::types::Color` into `Rgba8`.
pub fn color_to_rgba8(c: &Color) -> Rgba8 {
    color_to_rgba8_opt(c).unwrap_or(Rgba8::BLACK)
}

/// Converts an `acadrust::types::Color` into `Option<Rgba8>`, returning `None` for ByLayer and ByBlock.
pub fn color_to_rgba8_opt(c: &Color) -> Option<Rgba8> {
    match c {
        Color::ByLayer | Color::ByBlock => None,
        Color::Rgb { r, g, b } => Some(Rgba8::new(*r, *g, *b, 255)),
        Color::Index(idx) => Some(match idx {
            1 => Rgba8::new(255, 0, 0, 255),
            2 => Rgba8::new(255, 255, 0, 255),
            3 => Rgba8::new(0, 255, 0, 255),
            4 => Rgba8::new(0, 255, 255, 255),
            5 => Rgba8::new(0, 0, 255, 255),
            6 => Rgba8::new(255, 0, 255, 255),
            7 => Rgba8::WHITE,
            8 => Rgba8::new(128, 128, 128, 255),
            9 => Rgba8::new(192, 192, 192, 255),
            _ => Rgba8::BLACK,
        }),
        _ => None,
    }
}

/// Converts a 0xRRGGBB integer to `Rgba8`.
pub fn u32_to_rgba8(val: u32) -> Rgba8 {
    let r = ((val >> 16) & 0xFF) as u8;
    let g = ((val >> 8) & 0xFF) as u8;
    let b = (val & 0xFF) as u8;
    Rgba8::new(r, g, b, 255)
}

/// Context bundling the active `ModelView`, `DisplayConfig` (Planart), `StyleLibrary` and the
/// explicit kind of the derived view (section vs. plan).
///
/// Effective `ComponentRuleSet`s are built lazily per style id and cached for the lifetime of the
/// context, so a derivation builds each rule set at most once.
#[derive(Clone, Debug)]
pub struct ViewStyleContext<'a> {
    pub display_config: Option<&'a DisplayConfig>,
    pub style_library: Option<&'a StyleLibrary>,
    pub model_view: &'a ModelView,
    /// `true` for vertical sections (plan-relative hatch angles do not apply), `false` for floor
    /// plans / reflected ceiling plans.
    pub is_section: bool,
    neutral_config: DisplayConfig,
    rules_cache: RefCell<HashMap<String, Rc<ComponentRuleSet>>>,
}

impl<'a> ViewStyleContext<'a> {
    pub fn new(
        model_view: &'a ModelView,
        display_config: Option<&'a DisplayConfig>,
        style_library: Option<&'a StyleLibrary>,
        is_section: bool,
    ) -> Self {
        Self {
            display_config,
            style_library,
            model_view,
            is_section,
            neutral_config: DisplayConfig::new(
                String::new(),
                String::new(),
                PlanningStage::Design,
                ViewType::FloorPlan,
            ),
            rules_cache: RefCell::new(HashMap::new()),
        }
    }

    /// The active plan type, or a neutral (all visible, all detail) config when none is set.
    fn config(&self) -> &DisplayConfig {
        self.display_config.unwrap_or(&self.neutral_config)
    }

    fn material(&self, id_or_name: &str) -> Option<&'a Material> {
        self.style_library
            .and_then(|lib| lib.materials.iter().find(|m| m.id == id_or_name || m.name == id_or_name))
    }

    fn wall_style(&self, style_id: &str) -> Option<&'a WallStyle> {
        self.style_library
            .and_then(|lib| lib.wall_styles.iter().find(|ws| ws.style.id == style_id))
    }

    fn slab_style(&self, style_id: &str) -> Option<&'a SlabStyle> {
        self.style_library.and_then(|lib| lib.find_slab_style(style_id))
    }

    fn cached(&self, key: String, build: impl FnOnce() -> ComponentRuleSet) -> Rc<ComponentRuleSet> {
        if let Some(rules) = self.rules_cache.borrow().get(&key) {
            return rules.clone();
        }
        let rules = Rc::new(build());
        self.rules_cache.borrow_mut().insert(key, rules.clone());
        rules
    }

    /// Effective wall rule set (style default profile + plan overrides + detail switches).
    fn wall_rules(&self, style_id: &str) -> Rc<ComponentRuleSet> {
        self.cached(format!("w:{style_id}"), || {
            build_effective_rule_set(self.config(), self.wall_style(style_id), None)
        })
    }

    /// Effective slab rule set (style default profile + plan overrides + detail switches).
    fn slab_rules(&self, style_id: &str) -> Rc<ComponentRuleSet> {
        self.cached(format!("s:{style_id}"), || {
            build_effective_slab_rule_set(self.config(), self.slab_style(style_id), None)
        })
    }

    /// Effective opening rule set: plan visibility and representation mode are resolved before
    /// the opening style's inherited, hide-only visibility (as in opening display regeneration).
    /// Detail switches then force-hide their slots.
    fn opening_rules(&self, opening: &CollectedOpening) -> Rc<ComponentRuleSet> {
        let style_id = opening.style_id.clone().unwrap_or_else(|| {
            crate::modules::aec::engine::opening_style::seed_style_id_for_kind(opening.kind)
                .to_string()
        });
        self.cached(format!("o:{style_id}"), || {
            let mut rules = resolve_effective_rule_set(None, self.config(), Some(&style_id));
            for (id, overlay) in &self.config().style_overrides {
                if let Some(profile) = &overlay.opening {
                    rules.opening_profiles.insert(id.clone(), profile.clone());
                }
            }
            let mode = effective_representation(self.config(), None);
            for slot in OpeningComponentSlot::all() {
                let key = slot.key().to_string();
                let allowed_by_mode = match mode {
                    RepresentationMode::All => true,
                    RepresentationMode::TwoD => slot.is_2d_plan() || slot.is_elevation(),
                    RepresentationMode::ThreeD => slot.is_3d(),
                };
                if !allowed_by_mode {
                    rules.visibility.insert(key, false);
                }
            }
            if let Some(lib) = self.style_library {
                if lib.find_opening_style(&style_id).is_some() {
                    let map: HashMap<String, OpeningStyle> = lib
                        .opening_styles
                        .iter()
                        .map(|s| (s.style.id.clone(), s.clone()))
                        .collect();
                    let plan = rules.opening_profiles.clone();
                    apply_plan_visibility(&map, &style_id, Some(&plan), &mut rules.visibility);
                }
            }
            if !self.config().detail.annotation {
                rules.visibility.insert(OpeningComponentSlot::OpeningLabel2D.key().to_string(), false);
            }
            rules
        })
    }
}

// ── ModelView tier ────────────────────────────────────────────────────────────

fn selector_matches(
    selector: &OverrideSelector,
    element: &ElementRef,
    component: Option<&ComponentId>,
) -> bool {
    match selector {
        OverrideSelector::Element(e) => e == element,
        OverrideSelector::Component {
            element: e,
            component_id,
        } => e == element && component == Some(component_id),
        OverrideSelector::Feature(src) => {
            src.element == *element
                && match (src.component_id.as_ref(), component) {
                    (Some(a), Some(b)) => a == b,
                    (Some(_), None) => false,
                    (None, _) => true,
                }
        }
        OverrideSelector::DisplayComponent { element: e, .. } => e == element,
    }
}

/// Enabled overrides that apply to `element` / `component`, lowest priority first (so applying
/// them in order lets the highest priority win).
fn matching_overrides<'c>(
    ctx: &'c ViewStyleContext,
    element: &ElementRef,
    component: Option<&ComponentId>,
) -> Vec<&'c GraphicOverride> {
    let mut rules: Vec<&GraphicOverride> = ctx
        .model_view
        .graphic_overrides
        .iter()
        .filter(|r| r.enabled && selector_matches(&r.selector, element, component))
        .collect();
    rules.sort_by_key(|r| r.priority);
    rules
}

/// Resolves whether drawing is suppressed for `element` / `component` by `ModelView` overrides.
/// Overrides are sorted by priority ascending, and the last override with `draw = Some(_)` wins.
pub fn is_draw_suppressed(
    ctx: &ViewStyleContext,
    element: &ElementRef,
    component: Option<&ComponentId>,
) -> bool {
    let mut effective_draw = None;
    for r in matching_overrides(ctx, element, component) {
        if let Some(d) = r.patch.draw {
            effective_draw = Some(d);
        }
    }
    effective_draw == Some(false)
}

/// Stroke (contour / line) fields of a resolved style.
struct Stroke {
    weight: PaperMm,
    color: Option<Rgba8>,
    line_type: Option<String>,
    layer: Option<String>,
}

/// Hatch fields of a resolved style.
struct Hatch {
    weight: PaperMm,
    color: Option<Rgba8>,
    pattern: String,
    scale: f64,
    angle_deg: f64,
    angle_relative: bool,
    layer: Option<String>,
}

fn apply_patch_to_stroke(stroke: &mut Stroke, patch: &GraphicPatch) {
    if let StyleOverride::Set(lw) = patch.line_weight_mm {
        stroke.weight = lw;
    }
    match &patch.line_color {
        StyleOverride::Set(c) => stroke.color = Some(*c),
        StyleOverride::Clear => stroke.color = None,
        StyleOverride::Inherit => {}
    }
    match &patch.line_type {
        StyleOverride::Set(lt) => stroke.line_type = Some(lt.clone()),
        StyleOverride::Clear => stroke.line_type = None,
        StyleOverride::Inherit => {}
    }
    if let StyleOverride::Set(lay) = &patch.layer_name {
        stroke.layer = Some(lay.clone());
    }
}

fn apply_patch_to_hatch(hatch: &mut Hatch, patch: &GraphicPatch) {
    if let StyleOverride::Set(lw) = patch.line_weight_mm {
        hatch.weight = lw;
    }
    if let StyleOverride::Set(fc) = patch.fill_color {
        hatch.color = Some(fc);
    }
    if let StyleOverride::Set(pat) = &patch.hatch_pattern {
        hatch.pattern = pat.clone();
    }
    if let StyleOverride::Set(sc) = patch.hatch_scale {
        hatch.scale = sc;
    }
    if let StyleOverride::Set(rad) = patch.hatch_angle_rad {
        hatch.angle_deg = rad.to_degrees();
        hatch.angle_relative = false;
    }
    if let StyleOverride::Set(lay) = &patch.layer_name {
        hatch.layer = Some(lay.clone());
    }
}

fn stroke_style(stroke: Stroke) -> ResolvedStyle {
    ResolvedStyle {
        line_weight_mm: stroke.weight,
        line_color: stroke.color,
        line_type: stroke.line_type,
        fill_color: None,
        hatch_pattern: None,
        hatch_scale: None,
        hatch_angle_rad: None,
        layer_name: stroke.layer,
    }
}

fn hatch_style(hatch: Hatch, angle_rad: f64) -> ResolvedStyle {
    ResolvedStyle {
        line_weight_mm: hatch.weight,
        line_color: hatch.color,
        line_type: None,
        fill_color: hatch.color,
        hatch_pattern: Some(hatch.pattern),
        hatch_scale: Some(hatch.scale.max(1e-4)),
        hatch_angle_rad: Some(angle_rad),
        layer_name: hatch.layer,
    }
}

fn non_empty(s: &Option<String>) -> Option<String> {
    s.as_ref().filter(|s| !s.is_empty()).cloned()
}

/// Material line type as an explicit CAD line type (`None` = continuous).
fn material_line_type(mat: Option<&Material>) -> Option<String> {
    mat.map(|m| m.line_type.clone())
        .filter(|lt| !lt.is_empty() && !lt.eq_ignore_ascii_case("Continuous"))
}

/// Contour / hatch defaults of a [`ComponentStyleOverride`] resolved against the material.
fn base_stroke(
    ov: &ComponentStyleOverride,
    mat: Option<&Material>,
    default_layer: &str,
) -> Stroke {
    let color = ov
        .line_color
        .as_ref()
        .and_then(color_to_rgba8_opt)
        .or_else(|| mat.and_then(|m| if m.line_color != 0 { Some(u32_to_rgba8(m.line_color)) } else { None }));
    Stroke {
        weight: PaperMm::new(0.35).unwrap(),
        color,
        line_type: non_empty(&ov.line_type).or_else(|| material_line_type(mat)),
        layer: non_empty(&ov.cad_layer).or_else(|| Some(default_layer.to_string())),
    }
}

fn base_hatch(
    ov: &ComponentStyleOverride,
    layer_hatch_override: Option<&str>,
    mat: Option<&Material>,
    fallback_pattern: &str,
    default_layer: &str,
) -> Hatch {
    let pattern = non_empty(&ov.hatch_pattern)
        .or_else(|| layer_hatch_override.filter(|p| !p.is_empty()).map(str::to_string))
        .or_else(|| mat.map(|m| m.hatch_pattern.clone()).filter(|p| !p.is_empty()))
        .unwrap_or_else(|| fallback_pattern.to_string());
    let is_solid = pattern.eq_ignore_ascii_case("SOLID");
    let color = ov
        .hatch_color
        .as_ref()
        .and_then(color_to_rgba8_opt)
        .or_else(|| {
            if is_solid {
                ov.fill_color.as_ref().and_then(color_to_rgba8_opt)
            } else {
                None
            }
        })
        .or_else(|| mat.and_then(|m| m.hatch_color.as_ref().and_then(color_to_rgba8_opt)))
        .or_else(|| {
            if is_solid {
                mat.and_then(|m| if m.line_color != 0 { Some(u32_to_rgba8(m.line_color)) } else { None })
            } else {
                None
            }
        });
    Hatch {
        weight: PaperMm::new(0.18).unwrap(),
        color,
        pattern,
        scale: ov
            .hatch_scale
            .or_else(|| mat.map(|m| m.hatch_scale))
            .unwrap_or(1.0),
        angle_deg: ov
            .hatch_angle
            .or_else(|| mat.map(|m| m.hatch_angle))
            .unwrap_or(0.0),
        angle_relative: ov
            .hatch_angle_relative
            .or_else(|| mat.map(|m| m.hatch_angle_relative))
            .unwrap_or(true),
        layer: non_empty(&ov.cad_layer).or_else(|| Some(default_layer.to_string())),
    }
}

/// Phase extras (Abbruch/Bestand) win over the resolved slot/layer styles; stroke fields apply to
/// the contour, hatch fields only when the phase style explicitly configures them.
fn overlay_phase_extra(
    contour: &mut ComponentStyleOverride,
    hatch: &mut ComponentStyleOverride,
    extra: Option<&ComponentStyleOverride>,
) {
    let Some(extra) = extra else {
        return;
    };
    contour.overlay_from(extra);
    let hatch_part = ComponentStyleOverride {
        hatch_pattern: extra.hatch_pattern.clone(),
        hatch_color: extra.hatch_color,
        hatch_scale: extra.hatch_scale,
        hatch_angle: extra.hatch_angle,
        hatch_angle_relative: extra.hatch_angle_relative,
        ..Default::default()
    };
    hatch.overlay_from(&hatch_part);
}

fn wall_layer_ref(wall: &CollectedWall, layer_idx: usize) -> Option<LayerRef> {
    wall.layers.get(layer_idx).map(|l| LayerRef {
        material_id: l.material.clone(),
        role_tag: None,
        index: layer_idx,
        layer_id: Some(l.layer_id),
    })
}

fn slab_layer_ref(slab: &CollectedSlab, layer_idx: usize) -> Option<LayerRef> {
    slab.layers.get(layer_idx).map(|l| LayerRef {
        material_id: l.material.clone(),
        role_tag: None,
        index: layer_idx,
        layer_id: Some(l.layer_id),
    })
}

fn filter_includes(filter: &LayerSelection, layer_ref: &LayerRef) -> bool {
    match filter {
        LayerSelection::Explicit(refs) => layer_ref_matches(layer_ref, refs),
        LayerSelection::All => true,
    }
}

/// Which parts of a wall layer are drawn, evaluated exactly like `wall_regen` does.
struct WallLayerDraw {
    contour: bool,
    hatch: bool,
    is_envelope: bool,
}

fn wall_layer_draw(ctx: &ViewStyleContext, wall: &CollectedWall, layer_idx: usize) -> Option<WallLayerDraw> {
    let layer = wall.layers.get(layer_idx)?;
    let layer_ref = wall_layer_ref(wall, layer_idx)?;
    let rules = ctx.wall_rules(&wall.style_id);

    let layers2d_visible = rules.is_visible(WallComponentSlot::Layers2D);
    let contour2d_visible = rules.is_visible(WallComponentSlot::Contour2D);
    let layer_hatch_visible = rules.is_visible(WallComponentSlot::LayerHatch2D);
    let contour_hatch_visible = rules.is_visible(WallComponentSlot::ContourHatch2D);

    let contour_filter = rules.layer_filter_for(WallComponentSlot::Contour2D);
    let layers2d_filter = rules.layer_filter_for(WallComponentSlot::Layers2D);
    let hatch_filter = rules.layer_filter_for(WallComponentSlot::LayerHatch2D);
    let included_contour = filter_includes(contour_filter, &layer_ref);
    let included_layers2d = filter_includes(layers2d_filter, &layer_ref);
    let included_hatch = filter_includes(hatch_filter, &layer_ref);

    let is_envelope = layer_idx == 0 || layer_idx + 1 == wall.layers.len();

    let mut contour = if matches!(contour_filter, LayerSelection::Explicit(_)) {
        contour2d_visible && included_contour
    } else if matches!(layers2d_filter, LayerSelection::Explicit(_)) {
        layers2d_visible && included_layers2d
    } else {
        (layers2d_visible && included_layers2d)
            || (contour2d_visible && is_envelope && included_contour)
    };
    let mut hatch = (layer_hatch_visible && included_hatch) || (contour_hatch_visible && is_envelope);

    // `StructuralOnly` is strict for the views: the envelope-hatch shortcut above must not bring a
    // non-structural outer layer back, and without a resolvable wall style no explicit filter
    // exists at all, so the collected per-layer structural flag decides. A wall without any
    // structural layer is left untouched (same as the canonical filter, which stays unset then).
    if ctx.config().detail.layers == LayerDetail::StructuralOnly
        && wall.layers.iter().any(|l| l.is_structural)
        && !layer.is_structural
    {
        contour = false;
        hatch = false;
    }

    Some(WallLayerDraw {
        contour,
        hatch,
        is_envelope,
    })
}

// ── Visibility ────────────────────────────────────────────────────────────────

/// Determines whether a wall's lifecycle phase is visible under the active `DisplayConfig.phase_filter`.
pub fn is_wall_phase_visible(ctx: &ViewStyleContext, wall: &CollectedWall) -> bool {
    apply_phase_filter(wall.phase, ctx.config().phase_filter.as_ref()).visible
}

/// Determines whether a slab's lifecycle phase is visible under the active `DisplayConfig.phase_filter`.
pub fn is_slab_phase_visible(ctx: &ViewStyleContext, slab: &CollectedSlab) -> bool {
    apply_phase_filter(slab.phase, ctx.config().phase_filter.as_ref()).visible
}

/// Checks whether a wall layer physically exists in the view: its lifecycle phase is visible
/// and it has not been explicitly hidden via a `ModelView` override `draw == false`.
pub fn is_wall_layer_active(ctx: &ViewStyleContext, wall: &CollectedWall, layer_idx: usize) -> bool {
    if !is_wall_phase_visible(ctx, wall) {
        return false;
    }
    let Some(layer) = wall.layers.get(layer_idx) else {
        return false;
    };
    if is_draw_suppressed(ctx, &wall.element_ref, Some(&layer.component_id)) {
        return false;
    }
    true
}

/// Checks whether a slab layer physically exists in the view: its lifecycle phase is visible
/// and it has not been explicitly hidden via a `ModelView` override `draw == false`.
pub fn is_slab_layer_active(ctx: &ViewStyleContext, slab: &CollectedSlab, layer_idx: usize) -> bool {
    if !is_slab_phase_visible(ctx, slab) {
        return false;
    }
    let Some(layer) = slab.layers.get(layer_idx) else {
        return false;
    };
    if is_draw_suppressed(ctx, &slab.element_ref, Some(&layer.component_id)) {
        return false;
    }
    true
}

/// Checks whether a wall layer is visually rendered at all (has a cut boundary or a hatch fill):
/// phase filter, `ModelView` `draw == false`, and the canonical effective rule set
/// (`LayerDetail::{StructuralOnly, Off}`, slot visibility, per-slot layer filters, per-style layer visibility).
pub fn is_wall_layer_visible(ctx: &ViewStyleContext, wall: &CollectedWall, layer_idx: usize) -> bool {
    let (boundary, fill) = resolve_layer_cut_style(ctx, wall, layer_idx);
    boundary.is_some() || fill.is_some()
}

/// Checks whether a slab layer is visually rendered at all (has a cut boundary or a hatch fill):
/// phase filter, `ModelView` `draw == false` and the slab slot visibility / layer filter (`StructuralOnly`).
pub fn is_slab_layer_visible(ctx: &ViewStyleContext, slab: &CollectedSlab, layer_idx: usize) -> bool {
    let (boundary, fill) = resolve_slab_layer_cut_style(ctx, slab, layer_idx);
    boundary.is_some() || fill.is_some()
}

/// Checks whether the slab contour (floor edge, or overhead ceiling edge) is rendered.
pub fn is_slab_contour_visible(ctx: &ViewStyleContext, slab: &CollectedSlab, is_overhead: bool) -> bool {
    if !is_slab_phase_visible(ctx, slab) || is_draw_suppressed(ctx, &slab.element_ref, None) {
        return false;
    }
    let rules = ctx.slab_rules(&slab.style_id);
    let contour = rules.is_slab_visible(SlabComponentSlot::Contour2D);
    let ceiling = rules.is_slab_visible(SlabComponentSlot::CeilingOutline2D);
    if is_overhead {
        contour || ceiling
    } else if ctx.config().detail.ceiling_view {
        ceiling
    } else {
        contour
    }
}

/// Checks whether an opening slot should be rendered: model-view suppression,
/// representation mode and the opening style's plan visibility. Representation
/// levels are resolved separately and do not hide an unknown slot.
pub fn is_opening_slot_visible(
    ctx: &ViewStyleContext,
    opening: &CollectedOpening,
    slot: OpeningComponentSlot,
) -> bool {
    if is_draw_suppressed(ctx, &opening.element_ref, None) {
        return false;
    }
    ctx.opening_rules(opening).is_opening_visible(slot)
}

pub fn resolve_opening_style(ctx: &ViewStyleContext, opening: &CollectedOpening) -> OpeningStyle {
    ctx.style_library
        .and_then(|library| {
            let id = opening.style_id.as_deref().unwrap_or_else(|| {
                crate::modules::aec::engine::opening_style::seed_style_id_for_kind(opening.kind)
            });
            library.find_opening_style(id)
        })
        .cloned()
        .unwrap_or_else(|| default_style_for_kind(opening.kind))
}

pub fn resolve_opening_representation_levels(
    ctx: &ViewStyleContext,
    opening: &CollectedOpening,
    _grid: &crate::modules::aec::engine::opening_style::DivisionGrid,
) -> crate::modules::aec::engine::opening_style::OpeningRepresentationLevels {
    let style = resolve_opening_style(ctx, opening);
    let plan = ctx.config().opening_levels;
    style.suggested_levels.resolve(plan)
}

pub fn resolve_opening_section_roles(
    ctx: &ViewStyleContext,
    opening: &CollectedOpening,
) -> crate::modules::aec::engine::opening_style::SectionRoleSwitches {
    let style = resolve_opening_style(ctx, opening);
    ctx.config()
        .opening_levels
        .section
        .resolve(style.suggested_section)
}

// ── Wall / slab layer styles ─────────────────────────────────────────────────

/// Resolves the cascading styles for a wall layer in cut view:
/// returns `(Option<boundary_style>, Option<hatch_fill_style>)`. The boundary style is `None` when
/// the effective rule set does not draw a contour for this layer (e.g. hidden or filtered
/// `Layers2D` / `Contour2D`). The hatch style is `None` when the effective rule set does not draw
/// a hatch for this layer (`detail.hatches == false`, hidden or filtered `LayerHatch2D` / `ContourHatch2D`).
pub fn resolve_layer_cut_style(
    ctx: &ViewStyleContext,
    wall: &CollectedWall,
    layer_idx: usize,
) -> (Option<ResolvedStyle>, Option<ResolvedStyle>) {
    if !is_wall_layer_active(ctx, wall, layer_idx) {
        return (None, None);
    }
    let (Some(layer), Some(layer_ref), Some(draw)) = (
        wall.layers.get(layer_idx),
        wall_layer_ref(wall, layer_idx),
        wall_layer_draw(ctx, wall, layer_idx),
    ) else {
        return (None, None);
    };

    let rules = ctx.wall_rules(&wall.style_id);
    let mat = ctx.material(&layer.material);
    let (default_stroke_layer, default_hatch_layer) = if ctx.is_section {
        ("AEC_SECTION_CUT", "AEC_SECTION_HATCH")
    } else {
        ("AEC_WALL_CUT", "AEC_WALL_HATCH")
    };

    // Canonical slot/layer cascade (same slot priority as `wall_regen`).
    let contour_slots = if draw.is_envelope {
        [WallComponentSlot::Contour2D, WallComponentSlot::Layers2D]
    } else {
        [WallComponentSlot::Layers2D, WallComponentSlot::Contour2D]
    };
    let hatch_slots = if draw.is_envelope {
        [WallComponentSlot::ContourHatch2D, WallComponentSlot::LayerHatch2D]
    } else {
        [WallComponentSlot::LayerHatch2D, WallComponentSlot::ContourHatch2D]
    };
    let mut contour_ov = resolve_layer_style_override(Some(&rules), &contour_slots, &layer_ref);
    let mut hatch_ov = resolve_layer_style_override(Some(&rules), &hatch_slots, &layer_ref);
    if draw.is_envelope {
        if let Some(slot_style) = rules.style_for(WallComponentSlot::ContourHatch2D) {
            hatch_ov.overlay_from(slot_style);
        }
    }

    // Lifecycle phase (Abbruch/Bestand): always evaluated, `None` filter yields built-in extras.
    let phase = apply_phase_filter(wall.phase, ctx.config().phase_filter.as_ref());
    overlay_phase_extra(&mut contour_ov, &mut hatch_ov, phase.extra_style.as_ref());

    let mut stroke = base_stroke(&contour_ov, mat, default_stroke_layer);
    let mut hatch = base_hatch(
        &hatch_ov,
        layer.hatch_override.as_deref(),
        mat,
        FALLBACK_WALL_HATCH,
        default_hatch_layer,
    );

    // ModelView graphic overrides: highest tier.
    for rule in matching_overrides(ctx, &wall.element_ref, Some(&layer.component_id)) {
        apply_patch_to_stroke(&mut stroke, &rule.patch);
        apply_patch_to_hatch(&mut hatch, &rule.patch);
    }

    if hatch.scale <= 0.0 {
        hatch.scale = 0.01;
    }
    let wall_angle = if wall.axis_points.len() >= 2 {
        let dx = wall.axis_points[1].x_m - wall.axis_points[0].x_m;
        let dy = wall.axis_points[1].y_m - wall.axis_points[0].y_m;
        dy.atan2(dx)
    } else {
        0.0
    };
    let angle_rad = if !ctx.is_section && hatch.angle_relative {
        wall_angle + hatch.angle_deg.to_radians()
    } else {
        hatch.angle_deg.to_radians()
    };

    let boundary = if draw.contour {
        Some(stroke_style(stroke))
    } else {
        None
    };
    let fill = if draw.hatch {
        Some(hatch_style(hatch, angle_rad))
    } else {
        None
    };
    (boundary, fill)
}

/// Resolves the cascading styles for a slab layer in cut view:
/// returns `(Option<boundary_style>, Option<hatch_fill_style>)`.
pub fn resolve_slab_layer_cut_style(
    ctx: &ViewStyleContext,
    slab: &CollectedSlab,
    layer_idx: usize,
) -> (Option<ResolvedStyle>, Option<ResolvedStyle>) {
    if !is_slab_layer_active(ctx, slab, layer_idx) {
        return (None, None);
    }
    let (Some(layer), Some(layer_ref)) = (slab.layers.get(layer_idx), slab_layer_ref(slab, layer_idx)) else {
        return (None, None);
    };

    let rules = ctx.slab_rules(&slab.style_id);
    let mat = ctx.material(&layer.material);

    // Slab slot style first, per-layer override fills/wins on top (as for walls).
    let lso = rules
        .layer_style_override
        .iter()
        .find(|lso| layer_ref_matches(&layer_ref, std::slice::from_ref(&lso.layer)))
        .map(|lso| lso.style.clone());
    let layer_ov = |slot: SlabComponentSlot| {
        let mut ov = rules.slab_style_for(slot).cloned().unwrap_or_default();
        if let Some(l) = &lso {
            ov.overlay_from(l);
        }
        ov
    };
    let contour_slot = if ctx.config().detail.ceiling_view {
        SlabComponentSlot::CeilingOutline2D
    } else {
        SlabComponentSlot::Contour2D
    };
    let mut contour_ov = layer_ov(contour_slot);
    let mut hatch_ov = layer_ov(SlabComponentSlot::LayerHatch2D);

    let phase = apply_phase_filter(slab.phase, ctx.config().phase_filter.as_ref());
    overlay_phase_extra(&mut contour_ov, &mut hatch_ov, phase.extra_style.as_ref());

    let mut stroke = base_stroke(&contour_ov, mat, "AEC_SECTION_CUT");
    let mut hatch = base_hatch(
        &hatch_ov,
        layer.hatch_override.as_deref(),
        mat,
        FALLBACK_SLAB_HATCH,
        "AEC_SECTION_HATCH",
    );

    for rule in matching_overrides(ctx, &slab.element_ref, Some(&layer.component_id)) {
        apply_patch_to_stroke(&mut stroke, &rule.patch);
        apply_patch_to_hatch(&mut hatch, &rule.patch);
    }

    if hatch.scale <= 0.0 {
        hatch.scale = 0.01;
    }
    // Slab hatches are drawn in plan/section space: the angle is always absolute.
    let angle_rad = hatch.angle_deg.to_radians();

    let mut contour_drawn = rules.is_slab_visible(contour_slot)
        && filter_includes(rules.slab_layer_filter_for(contour_slot), &layer_ref);
    let mut hatch_drawn = rules.is_slab_visible(SlabComponentSlot::LayerHatch2D)
        && filter_includes(rules.slab_layer_filter_for(SlabComponentSlot::LayerHatch2D), &layer_ref);

    if ctx.config().detail.layers == LayerDetail::StructuralOnly
        && slab.layers.iter().any(|l| l.is_structural)
        && !layer.is_structural
    {
        contour_drawn = false;
        hatch_drawn = false;
    }

    let boundary = if contour_drawn {
        Some(stroke_style(stroke))
    } else {
        None
    };
    let fill = if hatch_drawn {
        Some(hatch_style(hatch, angle_rad))
    } else {
        None
    };
    (boundary, fill)
}

/// Resolves the contour style for slabs in floor plans (either projected floor edges or overhead
/// ceiling edges): slab slot style (`Contour2D` / `CeilingOutline2D`), phase extras, `ModelView`.
pub fn resolve_slab_contour_style(
    ctx: &ViewStyleContext,
    slab: &CollectedSlab,
    is_overhead: bool,
) -> ResolvedStyle {
    let rules = ctx.slab_rules(&slab.style_id);
    let slot = if ctx.config().detail.ceiling_view {
        SlabComponentSlot::CeilingOutline2D
    } else {
        SlabComponentSlot::Contour2D
    };
    let mut contour_ov = rules.slab_style_for(slot).cloned().unwrap_or_default();
    let mut unused_hatch = ComponentStyleOverride::default();
    let phase = apply_phase_filter(slab.phase, ctx.config().phase_filter.as_ref());
    overlay_phase_extra(&mut contour_ov, &mut unused_hatch, phase.extra_style.as_ref());

    let mut stroke = Stroke {
        weight: PaperMm::new(0.25).unwrap(),
        color: contour_ov
            .line_color
            .as_ref()
            .map(color_to_rgba8)
            .or(Some(Rgba8::BLACK)),
        line_type: if is_overhead {
            // Overhead edges stay dashed unless the phase / ModelView explicitly says otherwise.
            Some("DASHED".to_string())
        } else {
            non_empty(&contour_ov.line_type)
        },
        layer: Some(
            if is_overhead {
                "AEC_OVERHEAD".to_string()
            } else {
                non_empty(&contour_ov.cad_layer).unwrap_or_else(|| "AEC_SLAB_CONTOUR".to_string())
            },
        ),
    };
    if is_overhead && phase.extra_style.as_ref().and_then(|e| e.line_type.clone()).is_some() {
        stroke.line_type = non_empty(&contour_ov.line_type);
    }

    for rule in matching_overrides(ctx, &slab.element_ref, None) {
        apply_patch_to_stroke(&mut stroke, &rule.patch);
    }
    stroke_style(stroke)
}

/// Resolves the contour style for walls in elevation view.
pub fn resolve_wall_elevation_contour_style(
    ctx: &ViewStyleContext,
    wall: &CollectedWall,
) -> ResolvedStyle {
    let rules = ctx.wall_rules(&wall.style_id);
    let mut contour_ov = rules
        .style_for(WallComponentSlot::Contour2D)
        .cloned()
        .unwrap_or_default();
    let mut unused_hatch = ComponentStyleOverride::default();
    let phase = apply_phase_filter(wall.phase, ctx.config().phase_filter.as_ref());
    overlay_phase_extra(&mut contour_ov, &mut unused_hatch, phase.extra_style.as_ref());

    let mut stroke = Stroke {
        weight: PaperMm::new(0.35).unwrap(),
        color: contour_ov
            .line_color
            .as_ref()
            .and_then(color_to_rgba8_opt),
        line_type: non_empty(&contour_ov.line_type),
        layer: Some(
            non_empty(&contour_ov.cad_layer).unwrap_or_else(|| "AEC_WALL_CONTOUR".to_string()),
        ),
    };

    for rule in matching_overrides(ctx, &wall.element_ref, None) {
        apply_patch_to_stroke(&mut stroke, &rule.patch);
    }
    stroke_style(stroke)
}

// ── Openings ─────────────────────────────────────────────────────────────────

/// Resolves overhead lintel edge style for cut openings in floor plans.
pub fn resolve_overhead_lintel_style(
    ctx: &ViewStyleContext,
    opening: &CollectedOpening,
) -> ResolvedStyle {
    let mut stroke = Stroke {
        weight: PaperMm::new(0.25).unwrap(),
        color: Some(Rgba8::BLACK),
        line_type: Some("DASHED".to_string()),
        layer: Some("AEC_OVERHEAD".to_string()),
    };
    for rule in matching_overrides(ctx, &opening.element_ref, None) {
        apply_patch_to_stroke(&mut stroke, &rule.patch);
    }
    stroke_style(stroke)
}

/// Resolves the style of an opening component slot (e.g. Frame2D, Leaf2D, Swing2D, Sill2D):
/// slot defaults < plan slot style (`ComponentRuleSet.style_override`) < `ModelView` overrides.
pub fn resolve_opening_view_style(
    ctx: &ViewStyleContext,
    opening: &CollectedOpening,
    slot: OpeningComponentSlot,
) -> ResolvedStyle {
    let (default_weight, default_lt, default_layer) = match slot {
        OpeningComponentSlot::Frame2D | OpeningComponentSlot::Leaf2D => (0.25, None, "AEC_OPENING"),
        OpeningComponentSlot::Swing2D => (0.18, None, "AEC_OPENING"),
        OpeningComponentSlot::Sill2D | OpeningComponentSlot::Threshold2D => (0.18, None, "AEC_OPENING"),
        OpeningComponentSlot::Glazing2D => (0.18, None, "AEC_OPENING"),
        OpeningComponentSlot::BreakthroughSymbol2D => (0.25, None, "AEC_OPENING"),
        OpeningComponentSlot::ElevationContour2D => (0.35, None, "AEC_OPENING"),
        OpeningComponentSlot::ElevationSwing2D => (0.18, Some("DASHED".to_string()), "AEC_OPENING"),
        OpeningComponentSlot::HostCut2D => (0.35, None, "AEC_WALL_CUT"),
        _ => (0.25, None, "AEC_OPENING"),
    };

    let rules = ctx.opening_rules(opening);
    let ov_opt = rules.style_override.get(slot.key());
    let style = resolve_opening_style(ctx, opening);
    let role = OpeningLookRole::for_slot(slot);
    let look = role.map(|role| style.looks.get(role));
    let material = role
        .and_then(|role| style.material_id_for_look(role))
        .and_then(|id| ctx.material(id));

    let look_line_type = look
        .and_then(|look| non_empty(&look.line_type))
        .or_else(|| {
            material.and_then(|material| {
                (!material.line_type.is_empty()).then(|| material.line_type.clone())
            })
        })
        .or(default_lt);
    let look_line_color = match look.and_then(|look| look.line_color.as_ref()) {
        Some(color) => color_to_rgba8_opt(color),
        None => material.map(|material| u32_to_rgba8(material.line_color)),
    };
    let line_type = ov_opt
        .and_then(|ov| non_empty(&ov.line_type))
        .or(look_line_type);
    let line_color = match ov_opt.and_then(|ov| ov.line_color.as_ref()) {
        Some(color) => color_to_rgba8_opt(color),
        None => look_line_color,
    };

    let mut stroke = Stroke {
        weight: PaperMm::new(default_weight).unwrap(),
        color: line_color,
        line_type,
        layer: ov_opt.and_then(|ov| non_empty(&ov.cad_layer)).or_else(|| Some(default_layer.to_string())),
    };

    for rule in matching_overrides(ctx, &opening.element_ref, None) {
        apply_patch_to_stroke(&mut stroke, &rule.patch);
    }
    stroke_style(stroke)
}

/// Resolves an opaque or patterned fill style for an opening slot (e.g. Glazing2D or Leaf2D in elevation views).
pub fn resolve_opening_view_fill_style(
    ctx: &ViewStyleContext,
    opening: &CollectedOpening,
    slot: OpeningComponentSlot,
) -> Option<ResolvedStyle> {
    let rules = ctx.opening_rules(opening);
    let ov_opt = rules.style_override.get(slot.key());

    let fill_color = ov_opt.and_then(|ov| ov.fill_color.as_ref()).and_then(color_to_rgba8_opt);
    let hatch_color = ov_opt.and_then(|ov| ov.hatch_color.as_ref()).and_then(color_to_rgba8_opt);
    let hatch_pat = ov_opt.and_then(|ov| non_empty(&ov.hatch_pattern));

    let cad_layer = ov_opt
        .and_then(|ov| non_empty(&ov.cad_layer))
        .unwrap_or_else(|| "AEC_OPENING_SURFACE".to_string());

    Some(ResolvedStyle {
        line_weight_mm: PaperMm::new(0.18).unwrap(),
        line_color: None,
        line_type: None,
        fill_color: fill_color.or(hatch_color),
        hatch_pattern: hatch_pat,
        hatch_scale: ov_opt.and_then(|ov| ov.hatch_scale),
        hatch_angle_rad: ov_opt.and_then(|ov| ov.hatch_angle).map(|a| a.to_radians()),
        layer_name: Some(cad_layer),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use acadrust::types::Color;
    use crate::modules::aec::engine::display_component::{
        ComponentRuleSet, ComponentStyleOverride, LayerStyleOverride, OpeningComponentSlot,
        StyleDisplayOverlay,
    };
    use crate::modules::aec::engine::join::LayerRef;
    use crate::modules::aec::engine::library::StyleLibrary;
    use crate::modules::aec::engine::library::DisplayConfigLibrary;
    use crate::modules::aec::engine::material::Material;
    use crate::modules::aec::engine::openings::{OpeningKind, OpeningReferenceSide};
    use crate::modules::aec::engine::opening_shape::OpeningShape;
    use crate::modules::aec::engine::opening_style::{HingeSide, OpeningDisplayProfile, OpeningStyle};
    use crate::modules::aec::engine::plan_view::{
        DisplayConfig, LayerDetail, PhaseFilter, PlanPhase, PlanningStage, ViewType,
    };
    use crate::modules::aec::engine::style::Style;
    use crate::modules::aec::engine::views::model_view::{
        DetailPolicy, GraphicOverride, GraphicPatch, ModelView, OverrideSelector,
        StyleOverride, UpdatePolicy, ViewRevisions,
    };
    use crate::modules::aec::engine::views::{
        ComponentId, ElementId, ElementRef, ModelId, ModelViewId, OverrideId, PaperMm, Point2,
        Point3, Rgba8, ScaleDenominator, ViewProfileId, ViewpointId,
    };
    use crate::modules::aec::engine::display_component::{SlabComponentSlot, WallComponentSlot, LayerSelection};
    use crate::modules::aec::engine::slab_style::SlabStyle;
    use crate::modules::aec::engine::views::scene_collector::{
        CollectedOpening, CollectedSlab, CollectedSlabLayer, CollectedWall, CollectedWallLayer,
    };
    use crate::modules::aec::engine::wall::WallJustification;
    use crate::modules::aec::engine::wall_style::WallStyle;

    fn make_test_mv() -> ModelView {
        ModelView {
            id: ModelViewId::new(),
            name: "Test View".to_string(),
            viewpoint_id: ViewpointId::new(),
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(100.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        }
    }

    #[test]
    fn view_plan_resolution_prefers_stored_then_matching_stage_then_active() {
        let active = DisplayConfig::new(
            "Active Plan".to_string(),
            "Architecture".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        let section_execution = DisplayConfig::new(
            "Section Execution".to_string(),
            "Architecture".to_string(),
            PlanningStage::Execution,
            ViewType::Section,
        );
        let section_design = DisplayConfig::new(
            "Section Design".to_string(),
            "Architecture".to_string(),
            PlanningStage::Design,
            ViewType::Section,
        );
        let library = DisplayConfigLibrary {
            configs: vec![active.clone(), section_execution.clone(), section_design.clone()],
            ..Default::default()
        };

        let selected = resolve_view_display_config(
            &library,
            ViewType::Section,
            None,
            Some("Active Plan"),
        );
        assert_eq!(selected.id, section_design.id);

        let stored = resolve_view_display_config(
            &library,
            ViewType::Section,
            Some(section_execution.id),
            Some("Active Plan"),
        );
        assert_eq!(stored.name, "Section Execution");

        let fallback = resolve_view_display_config(
            &library,
            ViewType::Elevation,
            None,
            Some("Active Plan"),
        );
        assert_eq!(fallback.id, active.id);
    }

    #[test]
    fn view_plan_resolution_uses_neutral_config_when_no_active_plan_exists() {
        let selected = resolve_view_display_config(
            &DisplayConfigLibrary::empty(),
            ViewType::Section,
            None,
            None,
        );
        assert_eq!(selected.name, "Neutral");
        assert_eq!(selected.view_type, ViewType::Section);
        assert_eq!(selected.scale, None);
    }

    fn make_test_wall(mat_name: &str) -> CollectedWall {
        CollectedWall {
            handle: acadrust::Handle::from(10u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "WallStyle_Test".to_string(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(5.0, 0.0)],
            layers: vec![
                CollectedWallLayer {
                    material: mat_name.to_string(),
                    thickness: 0.20,
                    function: "Structural".to_string(),
                    axis_offset: 0.0,
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    hatch_override: None,
                    layer_id: uuid::Uuid::new_v4(),
                    component_id: ComponentId::new(),
                    is_structural: true,
                },
                CollectedWallLayer {
                    material: "Plaster".to_string(),
                    thickness: 0.02,
                    function: "Finish".to_string(),
                    axis_offset: 0.20,
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    hatch_override: None,
                    layer_id: uuid::Uuid::new_v4(),
                    component_id: ComponentId::new(),
                    is_structural: false,
                },
            ],
            total_thickness: 0.22,
            justification: WallJustification::Center,
            openings: Vec::new(),
            phase: PlanPhase::New,
        }
    }

    pub(crate) fn wall_style_for(wall: &CollectedWall) -> WallStyle {
        use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, LayerValue};
        WallStyle {
            style: Style {
                id: wall.style_id.clone(),
                name: wall.style_id.clone(),
                parent: None,
                category: "Wall".to_string(),
                origin: Default::default(),
            },
            layers: wall
                .layers
                .iter()
                .map(|l| Layer {
                    material_id: l.material.clone(),
                    thickness: LayerValue::Fixed(l.thickness),
                    function: if l.is_structural {
                        LayerFunction::Structural
                    } else {
                        LayerFunction::Finish
                    },
                    axis_offset: LayerValue::Fixed(l.axis_offset),
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    layer_override: None,
                    hatch_override: None,
                    role_tag: None,
                    layer_id: l.layer_id,
                })
                .collect(),
            default_profile: None,
        }
    }

    /// Plaster | Core (structural) | Plaster: layer 1 is the only non-envelope layer.
    fn make_three_layer_wall(core_material: &str) -> CollectedWall {
        let mut wall = make_test_wall(core_material);
        let core = wall.layers[0].clone();
        let mut outer = wall.layers[1].clone();
        outer.layer_id = uuid::Uuid::new_v4();
        outer.component_id = ComponentId::new();
        let mut inner = wall.layers[1].clone();
        inner.layer_id = uuid::Uuid::new_v4();
        inner.component_id = ComponentId::new();
        wall.layers = vec![outer, core, inner];
        wall
    }

    fn make_test_slab(mat_name: &str) -> CollectedSlab {
        CollectedSlab {
            handle: acadrust::Handle::from(30u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "SlabStyle_Test".to_string(),
            base_z: 0.0,
            total_thickness: 0.22,
            polygon: vec![
                Point2::new(0.0, 0.0),
                Point2::new(4.0, 0.0),
                Point2::new(4.0, 4.0),
                Point2::new(0.0, 4.0),
            ],
            layers: vec![
                CollectedSlabLayer {
                    material: mat_name.to_string(),
                    thickness: 0.20,
                    vertical_offset: 0.0,
                    function: "Structural".to_string(),
                    hatch_override: None,
                    layer_id: uuid::Uuid::new_v4(),
                    component_id: ComponentId::new(),
                    is_structural: true,
                },
                CollectedSlabLayer {
                    material: "Screed".to_string(),
                    thickness: 0.02,
                    vertical_offset: 0.0,
                    function: "Finish".to_string(),
                    hatch_override: None,
                    layer_id: uuid::Uuid::new_v4(),
                    component_id: ComponentId::new(),
                    is_structural: false,
                },
            ],
            justification: crate::modules::aec::engine::slab::SlabJustification::Top,
            openings: Vec::new(),
            phase: PlanPhase::New,
        }
    }

    fn material(id: &str, pattern: &str, scale: f64, angle: f64, relative: bool) -> Material {
        let mut mat = Material::new(
            id.to_string(),
            id.to_string(),
            pattern.to_string(),
            0x202020,
            "Continuous".to_string(),
        );
        mat.hatch_scale = scale;
        mat.hatch_angle = angle;
        mat.hatch_angle_relative = relative;
        mat
    }

    fn plan_dc() -> DisplayConfig {
        DisplayConfig::new(
            "Plan".to_string(),
            "Architektur".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        )
    }

    fn test_opening(style_id: Option<&str>) -> CollectedOpening {
        CollectedOpening {
            handle: acadrust::Handle::from(20u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            host_wall_handle: acadrust::Handle::from(10u64),
            kind: OpeningKind::Door,
            style_id: style_id.map(str::to_string),
            division_override: None,
            sill_height: 0.0,
            spring_height: 0.0,
            shape: OpeningShape::Rectangle,
            reference_side: OpeningReferenceSide::Start,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(2.0, 0.0, 1.05),
            height: 2.1,
            width: 1.0,
            distance_along_axis: 2.0,
            axis_span_local: (1.5, 2.5),
            hinge: HingeSide::Left,
            swing_side: Default::default(),
        }
    }

    #[test]
    fn test_resolve_material_hatch_and_color_cascade() {
        let mv = make_test_mv();
        let mut lib = StyleLibrary::default();
        let mut mat = Material::new(
            "Concrete".to_string(),
            "Stahlbeton".to_string(),
            "AR-CONC".to_string(),
            0xFF0000,
            "Continuous".to_string(),
        );
        mat.hatch_scale = 0.5;
        mat.hatch_angle = 45.0;
        mat.hatch_color = Some(Color::Rgb { r: 100, g: 150, b: 200 });
        lib.materials.push(mat);

        let wall = make_test_wall("Concrete");
        let ctx = ViewStyleContext::new(&mv, None, Some(&lib), false);

        let (boundary, fill) = resolve_layer_cut_style(&ctx, &wall, 0);
        let boundary = boundary.expect("boundary style");

        // Boundary line should inherit material line color
        assert_eq!(boundary.line_color, Some(Rgba8::new(255, 0, 0, 255)));

        // Fill should inherit material hatch properties
        let fill = fill.expect("fill style should be present");
        assert_eq!(fill.hatch_pattern.as_deref(), Some("AR-CONC"));
        assert!((fill.hatch_scale.unwrap() - 0.5).abs() < 1e-4);
        assert_eq!(fill.fill_color, Some(Rgba8::new(100, 150, 200, 255)));
        // Wall runs along +X, so the wall-relative material angle equals the material angle.
        assert!((fill.hatch_angle_rad.unwrap() - 45f64.to_radians()).abs() < 1e-9);
        assert_eq!(fill.layer_name.as_deref(), Some("AEC_WALL_HATCH"));
    }

    #[test]
    fn test_relative_hatch_angle_uses_wall_axis_in_plan_and_material_angle_in_section() {
        let mv = make_test_mv();
        let mut lib = StyleLibrary::default();
        lib.materials.push(material("Rotated", "ANSI31", 0.75, 30.0, true));
        let mut wall = make_test_wall("Rotated");
        wall.axis_points = vec![Point2::new(0.0, 0.0), Point2::new(0.0, 5.0)];

        let plan_ctx = ViewStyleContext::new(&mv, None, Some(&lib), false);
        let (_, plan_fill) = resolve_layer_cut_style(&plan_ctx, &wall, 0);
        assert!((plan_fill.unwrap().hatch_angle_rad.unwrap() - 120f64.to_radians()).abs() < 1e-9);

        let section_ctx = ViewStyleContext::new(&mv, None, Some(&lib), true);
        let (_, section_fill) = resolve_layer_cut_style(&section_ctx, &wall, 0);
        assert!((section_fill.unwrap().hatch_angle_rad.unwrap() - 30f64.to_radians()).abs() < 1e-9);
    }

    #[test]
    fn test_resolve_wall_style_overrides_material() {
        let mv = make_test_mv();
        let mut lib = StyleLibrary::default();
        let mat = Material::new(
            "Masonry".to_string(),
            "Mauerwerk".to_string(),
            "ANSI31".to_string(),
            0x000000,
            "Continuous".to_string(),
        );
        lib.materials.push(mat);

        let wall = make_test_wall("Masonry");
        let layer_id = wall.layers[0].layer_id;

        let mut ws = WallStyle {
            style: Style {
                id: "WallStyle_Test".to_string(),
                name: "WallStyle_Test".to_string(),
                parent: None,
                category: "Wall".to_string(),
                origin: Default::default(),
            },
            layers: vec![],
            default_profile: None,
        };
        let mut prof = ComponentRuleSet::default();
        prof.layer_style_override.push(LayerStyleOverride {
            layer: LayerRef {
                material_id: "Masonry".to_string(),
                role_tag: None,
                index: 0,
                layer_id: Some(layer_id),
            },
            style: ComponentStyleOverride {
                hatch_pattern: Some("BRICK".to_string()),
                hatch_scale: Some(2.5),
                line_color: Some(Color::Rgb { r: 0, g: 128, b: 0 }),
                ..Default::default()
            },
        });
        ws.default_profile = Some(prof);
        lib.wall_styles.push(ws);

        let ctx = ViewStyleContext::new(&mv, None, Some(&lib), false);
        let (boundary, fill) = resolve_layer_cut_style(&ctx, &wall, 0);
        let boundary = boundary.expect("boundary style");

        assert_eq!(boundary.line_color, Some(Rgba8::new(0, 128, 0, 255)));
        let fill = fill.expect("fill style");
        assert_eq!(fill.hatch_pattern.as_deref(), Some("BRICK"));
        assert!((fill.hatch_scale.unwrap() - 2.5).abs() < 1e-4);
    }

    #[test]
    fn test_resolve_display_config_overrides_wall_style() {
        let mut mv_50 = make_test_mv();
        mv_50.scale = ScaleDenominator::new(50.0).unwrap();
        let mut mv_100 = mv_50.clone();
        mv_100.scale = ScaleDenominator::new(100.0).unwrap();
        let mut wall = make_test_wall("Masonry");
        wall.axis_points = vec![Point2::new(0.0, 0.0), Point2::new(0.0, 5.0)];
        let layer_id = wall.layers[0].layer_id;

        let mut dc = DisplayConfig::new(
            "Ausfuehrung".to_string(),
            "Architektur".to_string(),
            PlanningStage::Execution,
            ViewType::FloorPlan,
        );
        let lprops = ComponentStyleOverride {
            hatch_pattern: Some("EARTH".to_string()),
            hatch_scale: Some(0.25),
            hatch_angle: Some(75.0),
            hatch_angle_relative: Some(false),
            line_color: Some(Color::Rgb { r: 255, g: 128, b: 0 }),
            ..Default::default()
        };

        let mut overlay = StyleDisplayOverlay::default();
        overlay.layer_props.insert(layer_id, lprops);
        dc.style_overrides.insert("WallStyle_Test".to_string(), overlay);

        // The canonical rule-set builder folds the plan overlay into the *style's* rule set, so the
        // wall style has to be part of the library.
        let mut lib = StyleLibrary::default();
        lib.wall_styles.push(wall_style_for(&wall));

        let ctx_50 = ViewStyleContext::new(&mv_50, Some(&dc), Some(&lib), false);
        let (boundary, fill_50) = resolve_layer_cut_style(&ctx_50, &wall, 0);
        let boundary = boundary.expect("boundary style");

        assert_eq!(boundary.line_color, Some(Rgba8::new(255, 128, 0, 255)));
        let fill_50 = fill_50.expect("fill style");
        assert_eq!(fill_50.hatch_pattern.as_deref(), Some("EARTH"));
        assert!((fill_50.hatch_scale.unwrap() - 0.25).abs() < 1e-4);
        assert!((fill_50.hatch_angle_rad.unwrap() - 75f64.to_radians()).abs() < 1e-9);

        let ctx_100 = ViewStyleContext::new(&mv_100, Some(&dc), Some(&lib), false);
        let (_, fill_100) = resolve_layer_cut_style(&ctx_100, &wall, 0);
        let fill_100 = fill_100.expect("fill style at 1:100");
        assert!((fill_100.hatch_scale.unwrap() - fill_50.hatch_scale.unwrap()).abs() < 1e-9);
        assert!((fill_100.hatch_angle_rad.unwrap() - fill_50.hatch_angle_rad.unwrap()).abs() < 1e-9);
    }

    #[test]
    fn test_model_view_graphic_override_takes_highest_precedence() {
        let mut mv = make_test_mv();
        let wall = make_test_wall("Concrete");

        let rule = GraphicOverride {
            id: OverrideId::new(),
            priority: 10,
            enabled: true,
            selector: OverrideSelector::Element(wall.element_ref),
            patch: GraphicPatch {
                draw: None,
                line_weight_mm: StyleOverride::Set(PaperMm::new(0.70).unwrap()),
                line_color: StyleOverride::Set(Rgba8::new(0, 0, 255, 255)),
                line_type: StyleOverride::Set("DASHDOT".to_string()),
                fill_color: StyleOverride::Set(Rgba8::new(200, 200, 255, 255)),
                hatch_pattern: StyleOverride::Set("SOLID".to_string()),
                hatch_scale: StyleOverride::Set(3.0),
                hatch_angle_rad: StyleOverride::Inherit,
                layer_name: StyleOverride::Set("AEC_OVERRIDE".to_string()),
            },
        };
        mv.graphic_overrides.push(rule);

        let ctx = ViewStyleContext::new(&mv, None, None, false);
        let (boundary, fill) = resolve_layer_cut_style(&ctx, &wall, 0);
        let boundary = boundary.expect("boundary style");

        assert_eq!(boundary.line_weight_mm.get(), 0.70);
        assert_eq!(boundary.line_color, Some(Rgba8::new(0, 0, 255, 255)));
        assert_eq!(boundary.line_type.as_deref(), Some("DASHDOT"));
        assert_eq!(boundary.layer_name.as_deref(), Some("AEC_OVERRIDE"));

        let fill = fill.expect("fill");
        assert_eq!(fill.hatch_pattern.as_deref(), Some("SOLID"));
        assert_eq!(fill.fill_color, Some(Rgba8::new(200, 200, 255, 255)));
        assert_eq!(fill.layer_name.as_deref(), Some("AEC_OVERRIDE"));
    }

    #[test]
    fn test_display_config_hatches_disabled() {
        let mv = make_test_mv();
        let wall = make_test_wall("Concrete");

        let mut dc = DisplayConfig::new(
            "Draft".to_string(),
            "Architektur".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        dc.detail.hatches = false;

        let ctx = ViewStyleContext::new(&mv, Some(&dc), None, false);
        let (_, fill) = resolve_layer_cut_style(&ctx, &wall, 0);
        assert!(fill.is_none(), "When dc.detail.hatches is false, no fill style must be emitted");
    }

    #[test]
    fn test_layer_detail_structural_only_filtering() {
        let mv = make_test_mv();
        let wall = make_test_wall("Concrete");

        let mut dc = DisplayConfig::new(
            "Structural".to_string(),
            "Statik".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        dc.detail.layers = LayerDetail::StructuralOnly;

        let ctx = ViewStyleContext::new(&mv, Some(&dc), None, false);
        // Layer 0 is structural
        assert!(is_wall_layer_visible(&ctx, &wall, 0));
        // Layer 1 is non-structural plaster finish
        assert!(!is_wall_layer_visible(&ctx, &wall, 1));
    }

    #[test]
    fn test_opening_symbols_disabled() {
        let mv = make_test_mv();
        let op = CollectedOpening {
            handle: acadrust::Handle::from(20u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            host_wall_handle: acadrust::Handle::from(10u64),
            kind: OpeningKind::Door,
            style_id: None,
            division_override: None,
            sill_height: 0.0,
            spring_height: 0.0,
            shape: OpeningShape::Rectangle,
            reference_side: OpeningReferenceSide::Start,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(2.0, 0.0, 1.05),
            height: 2.1,
            width: 1.0,
            distance_along_axis: 2.0,
            axis_span_local: (1.5, 2.5),
            hinge: HingeSide::Left,
            swing_side: Default::default(),
        };

        let mut dc = DisplayConfig::new(
            "SitePlan".to_string(),
            "Architektur".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        dc.opening_levels.swing = Some(crate::modules::aec::engine::opening_style::SwingGlyph::Off);

        let ctx = ViewStyleContext::new(&mv, Some(&dc), None, false);
        let levels = resolve_opening_representation_levels(
            &ctx,
            &op,
            &crate::modules::aec::engine::opening_style::DivisionGrid::default(),
        );
        assert_eq!(levels.swing, crate::modules::aec::engine::opening_style::SwingGlyph::Off);
        assert_eq!(
            levels.plan_frame,
            crate::modules::aec::engine::opening_style::OpeningRepresentationLevels::suggested_for(
                OpeningKind::Door
            )
            .plan_frame
        );
        assert!(is_opening_slot_visible(&ctx, &op, OpeningComponentSlot::Frame2D));
    }

    #[test]
    fn test_phase_filter_demolition_and_existing() {
        let mv = make_test_mv();
        let mut wall_demo = make_test_wall("Concrete");
        wall_demo.phase = PlanPhase::Demolition;

        let mut wall_exist = make_test_wall("Concrete");
        wall_exist.phase = PlanPhase::Existing;

        let mut dc = DisplayConfig::new(
            "PhasePlan".to_string(),
            "Architektur".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        let mut pf = PhaseFilter::default();
        pf.visible_phases = vec![PlanPhase::New, PlanPhase::Demolition, PlanPhase::Existing];
        pf.demolition_style = Some(ComponentStyleOverride {
            line_type: Some("DASHED".to_string()),
            ..Default::default()
        });
        pf.existing_style = Some(ComponentStyleOverride {
            line_color: Some(Color::Rgb { r: 128, g: 128, b: 128 }),
            ..Default::default()
        });
        dc.phase_filter = Some(pf);

        let ctx = ViewStyleContext::new(&mv, Some(&dc), None, false);

        // Demolition should receive dashed line type
        let (demo_bound, _) = resolve_layer_cut_style(&ctx, &wall_demo, 0);
        let demo_bound = demo_bound.expect("demo boundary");
        assert_eq!(demo_bound.line_type.as_deref(), Some("DASHED"));

        // Existing should receive grey line color
        let (exist_bound, _) = resolve_layer_cut_style(&ctx, &wall_exist, 0);
        let exist_bound = exist_bound.expect("exist boundary");
        assert_eq!(exist_bound.line_color, Some(Rgba8::new(128, 128, 128, 255)));
    }

    #[test]
    fn test_graphic_override_suppress_draw() {
        let mut mv = make_test_mv();
        let wall = make_test_wall("Concrete");

        let rule = GraphicOverride {
            id: OverrideId::new(),
            priority: 1,
            enabled: true,
            selector: OverrideSelector::Element(wall.element_ref),
            patch: GraphicPatch {
                draw: Some(false),
                ..Default::default()
            },
        };
        mv.graphic_overrides.push(rule);

        let ctx = ViewStyleContext::new(&mv, None, None, false);
        assert!(!is_wall_layer_visible(&ctx, &wall, 0));
        assert!(!is_wall_layer_visible(&ctx, &wall, 1));
    }

    #[test]
    fn test_material_hatch_angle_is_wall_relative_in_plan_and_absolute_in_section() {
        let mv = make_test_mv();
        let mut lib = StyleLibrary::default();
        lib.materials.push(material("Brick", "BRICK", 2.0, 45.0, true));
        let mut wall = make_test_wall("Brick");
        // Wall runs along +Y.
        wall.axis_points = vec![Point2::new(0.0, 0.0), Point2::new(0.0, 5.0)];

        let plan = ViewStyleContext::new(&mv, None, Some(&lib), false);
        let fill = resolve_layer_cut_style(&plan, &wall, 0).1.expect("plan fill");
        assert_eq!(fill.hatch_pattern.as_deref(), Some("BRICK"));
        assert!((fill.hatch_scale.unwrap() - 2.0).abs() < 1e-9);
        assert!(
            (fill.hatch_angle_rad.unwrap() - (90f64.to_radians() + 45f64.to_radians())).abs() < 1e-9,
            "plan hatch angle is added to the wall run direction"
        );

        let section = ViewStyleContext::new(&mv, None, Some(&lib), true);
        let (boundary, fill) = resolve_layer_cut_style(&section, &wall, 0);
        let boundary = boundary.expect("section boundary");
        let fill = fill.expect("section fill");
        assert!(
            (fill.hatch_angle_rad.unwrap() - 45f64.to_radians()).abs() < 1e-9,
            "section hatch angle is never wall-relative"
        );
        assert_eq!(boundary.layer_name.as_deref(), Some("AEC_SECTION_CUT"));
        assert_eq!(fill.layer_name.as_deref(), Some("AEC_SECTION_HATCH"));

        // Absolute material angle ignores the wall direction in plans as well.
        lib.materials.clear();
        lib.materials.push(material("Brick", "BRICK", 2.0, 45.0, false));
        let plan = ViewStyleContext::new(&mv, None, Some(&lib), false);
        let fill = resolve_layer_cut_style(&plan, &wall, 0).1.expect("plan fill");
        assert!((fill.hatch_angle_rad.unwrap() - 45f64.to_radians()).abs() < 1e-9);
    }

    #[test]
    fn test_material_patterns_ar_conc_brick_earth() {
        let mv = make_test_mv();
        let mut lib = StyleLibrary::default();
        lib.materials.push(material("Beton", "AR-CONC", 0.5, 0.0, true));
        lib.materials.push(material("Ziegel", "BRICK", 1.5, 0.0, true));
        lib.materials.push(material("Erde", "EARTH", 0.25, 0.0, true));
        let ctx = ViewStyleContext::new(&mv, None, Some(&lib), true);
        for (mat, pat, scale) in [("Beton", "AR-CONC", 0.5), ("Ziegel", "BRICK", 1.5), ("Erde", "EARTH", 0.25)] {
            let wall = make_test_wall(mat);
            let fill = resolve_layer_cut_style(&ctx, &wall, 0).1.expect("fill");
            assert_eq!(fill.hatch_pattern.as_deref(), Some(pat));
            assert!((fill.hatch_scale.unwrap() - scale).abs() < 1e-9);
        }
        // Slabs resolve the same material properties.
        let slab = make_test_slab("Beton");
        let fill = resolve_slab_layer_cut_style(&ctx, &slab, 0).1.expect("slab fill");
        assert_eq!(fill.hatch_pattern.as_deref(), Some("AR-CONC"));
        assert!((fill.hatch_scale.unwrap() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_slot_style_override_and_layer_filter_from_effective_rule_set() {
        let mv = make_test_mv();
        let wall = make_three_layer_wall("Concrete");
        let mut lib = StyleLibrary::default();
        lib.materials.push(material("Concrete", "AR-CONC", 1.0, 0.0, true));

        let mut ws = wall_style_for(&wall);
        let mut profile = ComponentRuleSet::default();
        profile.style_override.insert(
            WallComponentSlot::LayerHatch2D.key().to_string(),
            ComponentStyleOverride {
                hatch_pattern: Some("EARTH".to_string()),
                hatch_scale: Some(3.0),
                cad_layer: Some("MY_HATCH".to_string()),
                ..Default::default()
            },
        );
        profile.style_override.insert(
            WallComponentSlot::Layers2D.key().to_string(),
            ComponentStyleOverride {
                line_type: Some("DASHDOT".to_string()),
                ..Default::default()
            },
        );
        ws.default_profile = Some(profile);
        lib.wall_styles.push(ws.clone());

        let ctx = ViewStyleContext::new(&mv, None, Some(&lib), false);
        let (boundary, fill) = resolve_layer_cut_style(&ctx, &wall, 1);
        let boundary = boundary.expect("boundary");
        assert_eq!(boundary.line_type.as_deref(), Some("DASHDOT"), "slot style override must reach the contour");
        let fill = fill.expect("fill");
        assert_eq!(fill.hatch_pattern.as_deref(), Some("EARTH"));
        assert!((fill.hatch_scale.unwrap() - 3.0).abs() < 1e-9);
        assert_eq!(fill.layer_name.as_deref(), Some("MY_HATCH"));

        // Layer filter for LayerHatch2D: only the first layer; the middle (non-envelope) layer has no hatch.
        let mut profile = ws.default_profile.clone().unwrap();
        profile.layer_filter.insert(
            WallComponentSlot::LayerHatch2D.key().to_string(),
            LayerSelection::Explicit(vec![LayerRef {
                material_id: wall.layers[0].material.clone(),
                role_tag: None,
                index: 0,
                layer_id: Some(wall.layers[0].layer_id),
            }]),
        );
        ws.default_profile = Some(profile);
        lib.wall_styles.clear();
        lib.wall_styles.push(ws);
        let ctx = ViewStyleContext::new(&mv, None, Some(&lib), false);
        assert!(resolve_layer_cut_style(&ctx, &wall, 0).1.is_some());
        assert!(resolve_layer_cut_style(&ctx, &wall, 1).1.is_none());
    }

    #[test]
    fn test_display_config_rules_layer_style_override_wins_over_style_default() {
        let mv = make_test_mv();
        let wall = make_test_wall("Concrete");
        let mut lib = StyleLibrary::default();
        lib.materials.push(material("Concrete", "AR-CONC", 1.0, 0.0, true));
        let mut ws = wall_style_for(&wall);
        let mut default_profile = ComponentRuleSet::default();
        default_profile.layer_style_override.push(LayerStyleOverride {
            layer: LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 0,
                layer_id: Some(wall.layers[0].layer_id),
            },
            style: ComponentStyleOverride {
                hatch_pattern: Some("BRICK".to_string()),
                ..Default::default()
            },
        });
        ws.default_profile = Some(default_profile);
        lib.wall_styles.push(ws);

        let mut dc = plan_dc();
        let mut plan_rules = ComponentRuleSet::default();
        plan_rules.layer_style_override.push(LayerStyleOverride {
            layer: LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 0,
                layer_id: Some(wall.layers[0].layer_id),
            },
            style: ComponentStyleOverride {
                hatch_pattern: Some("EARTH".to_string()),
                ..Default::default()
            },
        });
        let mut overlay = StyleDisplayOverlay::default();
        overlay.rules = Some(plan_rules);
        dc.style_overrides.insert(wall.style_id.clone(), overlay);

        let ctx = ViewStyleContext::new(&mv, Some(&dc), Some(&lib), false);
        let fill = resolve_layer_cut_style(&ctx, &wall, 0).1.expect("fill");
        assert_eq!(fill.hatch_pattern.as_deref(), Some("EARTH"));
    }

    #[test]
    fn test_layer_detail_off_keeps_only_envelope_layers() {
        let mv = make_test_mv();
        let wall = make_three_layer_wall("Concrete");
        let mut dc = plan_dc();
        dc.detail.layers = LayerDetail::Off;
        let ctx = ViewStyleContext::new(&mv, Some(&dc), None, false);
        assert!(is_wall_layer_visible(&ctx, &wall, 0));
        assert!(!is_wall_layer_visible(&ctx, &wall, 1), "build-up layers vanish with LayerDetail::Off");
        assert!(is_wall_layer_visible(&ctx, &wall, 2));
    }

    #[test]
    fn test_structural_only_uses_canonical_filter_with_wall_style() {
        let mv = make_test_mv();
        let wall = make_three_layer_wall("Concrete");
        let mut lib = StyleLibrary::default();
        lib.wall_styles.push(wall_style_for(&wall));
        let mut dc = plan_dc();
        dc.detail.layers = LayerDetail::StructuralOnly;
        let ctx = ViewStyleContext::new(&mv, Some(&dc), Some(&lib), false);
        assert!(!is_wall_layer_visible(&ctx, &wall, 0), "outer finish is not structural");
        assert!(is_wall_layer_visible(&ctx, &wall, 1), "structural core stays");
        assert!(!is_wall_layer_visible(&ctx, &wall, 2));
    }

    #[test]
    fn test_style_overlay_layer_visibility_hides_layer() {
        use crate::modules::aec::engine::display_component::LayerVis;
        let mv = make_test_mv();
        let wall = make_three_layer_wall("Concrete");
        let mut lib = StyleLibrary::default();
        lib.wall_styles.push(wall_style_for(&wall));
        let mut dc = plan_dc();
        let mut overlay = StyleDisplayOverlay::default();
        overlay.layer_visibility.insert(
            wall.layers[1].layer_id,
            LayerVis {
                visible_2d: false,
                visible_3d: true,
            },
        );
        dc.style_overrides.insert(wall.style_id.clone(), overlay);
        let ctx = ViewStyleContext::new(&mv, Some(&dc), Some(&lib), false);
        assert!(is_wall_layer_visible(&ctx, &wall, 0));
        assert!(!is_wall_layer_visible(&ctx, &wall, 1));
        assert!(is_wall_layer_visible(&ctx, &wall, 2));
    }

    #[test]
    fn test_phase_defaults_apply_without_display_config() {
        let mv = make_test_mv();
        let mut demo = make_test_wall("Concrete");
        demo.phase = PlanPhase::Demolition;
        let mut existing = make_test_wall("Concrete");
        existing.phase = PlanPhase::Existing;
        let ctx = ViewStyleContext::new(&mv, None, None, false);
        assert_eq!(resolve_layer_cut_style(&ctx, &demo, 0).0.unwrap().line_type.as_deref(), Some("DASHED"));
        assert_eq!(
            resolve_layer_cut_style(&ctx, &existing, 0).0.unwrap().line_color,
            Some(Rgba8::new(136, 136, 136, 255))
        );
    }

    #[test]
    fn test_phase_filter_hides_phase_and_configures_hatch() {
        let mv = make_test_mv();
        let mut demo = make_test_wall("Concrete");
        demo.phase = PlanPhase::Demolition;
        let mut dc = plan_dc();
        dc.phase_filter = Some(PhaseFilter {
            visible_phases: vec![PlanPhase::New, PlanPhase::Demolition],
            demolition_style: Some(ComponentStyleOverride {
                line_type: Some("DASHED".to_string()),
                hatch_pattern: Some("ANSI37".to_string()),
                ..Default::default()
            }),
            existing_style: None,
        });
        let ctx = ViewStyleContext::new(&mv, Some(&dc), None, false);
        let (boundary, fill) = resolve_layer_cut_style(&ctx, &demo, 0);
        let boundary = boundary.expect("demo boundary");
        assert_eq!(boundary.line_type.as_deref(), Some("DASHED"));
        assert_eq!(fill.unwrap().hatch_pattern.as_deref(), Some("ANSI37"));

        let mut existing = make_test_wall("Concrete");
        existing.phase = PlanPhase::Existing;
        assert!(!is_wall_phase_visible(&ctx, &existing));
        assert!(!is_wall_layer_visible(&ctx, &existing, 0));
        assert!(is_wall_layer_visible(&ctx, &demo, 0));
    }

    #[test]
    fn test_slab_hatches_disabled_and_structural_only() {
        let mv = make_test_mv();
        let mut lib = StyleLibrary::default();
        lib.materials.push(material("Beton", "AR-CONC", 0.5, 0.0, true));
        let slab = make_test_slab("Beton");

        let mut dc = plan_dc();
        dc.detail.hatches = false;
        let ctx = ViewStyleContext::new(&mv, Some(&dc), Some(&lib), true);
        let (boundary, fill) = resolve_slab_layer_cut_style(&ctx, &slab, 0);
        let boundary = boundary.expect("slab boundary");
        assert!(fill.is_none(), "slab LayerHatch2D follows detail.hatches");
        assert_eq!(boundary.layer_name.as_deref(), Some("AEC_SECTION_CUT"));

        let mut dc = plan_dc();
        dc.detail.layers = LayerDetail::StructuralOnly;
        let ctx = ViewStyleContext::new(&mv, Some(&dc), Some(&lib), true);
        assert!(is_slab_layer_visible(&ctx, &slab, 0));
        assert!(!is_slab_layer_visible(&ctx, &slab, 1));
    }

    #[test]
    fn test_slab_style_overlay_layer_visibility_hides_layer() {
        use crate::modules::aec::engine::display_component::LayerVis;

        let mv = make_test_mv();
        let slab = make_test_slab("Beton");
        let mut lib = StyleLibrary::default();
        lib.materials.push(material("Beton", "AR-CONC", 0.5, 0.0, true));
        let mut slab_style = SlabStyle::new(slab.style_id.clone(), slab.style_id.clone());
        slab_style.layers = slab
            .layers
            .iter()
            .map(|layer| {
                let mut style_layer = crate::modules::aec::engine::slab_style::SlabStyleLayer::new(
                    layer.material.clone(),
                    crate::modules::aec::engine::wall_style::LayerValue::Fixed(layer.thickness),
                    if layer.is_structural {
                        crate::modules::aec::engine::wall_style::LayerFunction::Structural
                    } else {
                        crate::modules::aec::engine::wall_style::LayerFunction::Finish
                    },
                );
                style_layer.layer_id = layer.layer_id;
                style_layer
            })
            .collect();
        lib.slab_styles.push(slab_style);
        let mut dc = plan_dc();
        let mut overlay = crate::modules::aec::engine::display_component::StyleDisplayOverlay::default();
        overlay.layer_visibility.insert(
            slab.layers[0].layer_id,
            LayerVis {
                visible_2d: false,
                visible_3d: true,
            },
        );
        dc.style_overrides.insert(slab.style_id.clone(), overlay);

        let ctx = ViewStyleContext::new(&mv, Some(&dc), Some(&lib), true);

        assert!(!is_slab_layer_visible(&ctx, &slab, 0));
        assert!(is_slab_layer_visible(&ctx, &slab, 1));
    }

    #[test]
    fn test_slab_contour_visibility_and_slot_style() {
        let mv = make_test_mv();
        let slab = make_test_slab("Beton");
        let mut lib = StyleLibrary::default();
        let mut ss = SlabStyle::new("SlabStyle_Test", "SlabStyle_Test");
        let mut profile = ComponentRuleSet::default();
        profile.style_override.insert(
            SlabComponentSlot::Contour2D.key().to_string(),
            ComponentStyleOverride {
                line_type: Some("CENTER".to_string()),
                line_color: Some(Color::Rgb { r: 10, g: 20, b: 30 }),
                cad_layer: Some("MY_SLAB".to_string()),
                ..Default::default()
            },
        );
        ss.default_profile = Some(profile);
        lib.slab_styles.push(ss);

        let ctx = ViewStyleContext::new(&mv, None, Some(&lib), false);
        let floor = resolve_slab_contour_style(&ctx, &slab, false);
        assert_eq!(floor.line_type.as_deref(), Some("CENTER"));
        assert_eq!(floor.line_color, Some(Rgba8::new(10, 20, 30, 255)));
        assert_eq!(floor.layer_name.as_deref(), Some("MY_SLAB"));
        let overhead = resolve_slab_contour_style(&ctx, &slab, true);
        assert_eq!(overhead.line_type.as_deref(), Some("DASHED"));
        assert_eq!(overhead.layer_name.as_deref(), Some("AEC_OVERHEAD"));
        assert!(is_slab_contour_visible(&ctx, &slab, false));
        assert!(is_slab_contour_visible(&ctx, &slab, true));

        // Reflected ceiling plan config: the projected outline is the ceiling outline slot.
        let mut dc = plan_dc();
        dc.detail.ceiling_view = true;
        let ctx = ViewStyleContext::new(&mv, Some(&dc), Some(&lib), false);
        assert!(is_slab_contour_visible(&ctx, &slab, false));
        assert!(is_slab_contour_visible(&ctx, &slab, true));

        // Plan override hides the slab contour slots of this slab style entirely.
        let mut dc = plan_dc();
        let mut hide = ComponentRuleSet::default();
        hide.visibility.insert(SlabComponentSlot::Contour2D.key().to_string(), false);
        hide.visibility.insert(SlabComponentSlot::CeilingOutline2D.key().to_string(), false);
        let mut overlay = StyleDisplayOverlay::default();
        overlay.rules = Some(hide);
        dc.style_overrides.insert(slab.style_id.clone(), overlay);
        let ctx = ViewStyleContext::new(&mv, Some(&dc), Some(&lib), false);
        assert!(!is_slab_contour_visible(&ctx, &slab, false));
        assert!(!is_slab_contour_visible(&ctx, &slab, true));
    }

    #[test]
    fn test_opening_visibility_and_slot_style_from_plan() {
        let mv = make_test_mv();
        let op = test_opening(Some("Door_Test"));

        let mut dc = plan_dc();
        dc.opening_levels.section.exterior_sill = Some(false);
        let mut overlay = StyleDisplayOverlay::default();
        let mut rules = ComponentRuleSet::default();
        rules.style_override.insert(
            OpeningComponentSlot::Swing2D.key().to_string(),
            ComponentStyleOverride {
                line_type: Some("DASHED".to_string()),
                cad_layer: Some("MY_SWING".to_string()),
                ..Default::default()
            },
        );
        overlay.rules = Some(rules);
        dc.style_overrides.insert("Door_Test".to_string(), overlay);

        let ctx = ViewStyleContext::new(&mv, Some(&dc), None, false);
        assert!(!resolve_opening_section_roles(&ctx, &op).exterior_sill);
        assert!(is_opening_slot_visible(&ctx, &op, OpeningComponentSlot::Frame2D));
        let swing = resolve_opening_view_style(&ctx, &op, OpeningComponentSlot::Swing2D);
        assert_eq!(swing.line_type.as_deref(), Some("DASHED"));
        assert_eq!(swing.layer_name.as_deref(), Some("MY_SWING"));
        // Another opening style is not affected by the plan override of "Door_Test".
        let other = test_opening(Some("Window_Other"));
        let swing = resolve_opening_view_style(&ctx, &other, OpeningComponentSlot::Swing2D);
        assert_eq!(swing.layer_name.as_deref(), Some("AEC_OPENING"));
    }

    #[test]
    fn opening_look_is_shared_by_plan_and_elevation_before_plan_override() {
        let mv = make_test_mv();
        let mut style = OpeningStyle::standard_window();
        style.style.id = "Window_Looks".to_string();
        style.looks.fixed_frame.line_type = Some("STYLE_FRAME".to_string());
        style.looks.fixed_frame.line_color = Some(Color::Rgb {
            r: 220,
            g: 30,
            b: 40,
        });
        let mut lib = StyleLibrary::default();
        lib.opening_styles.push(style);

        let mut dc = plan_dc();
        let mut overlay = StyleDisplayOverlay::default();
        let mut rules = ComponentRuleSet::default();
        rules.style_override.insert(
            OpeningComponentSlot::Frame2D.key().to_string(),
            ComponentStyleOverride {
                line_type: Some("PLAN_FRAME".to_string()),
                cad_layer: Some("PLAN_FRAME_LAYER".to_string()),
                ..Default::default()
            },
        );
        overlay.rules = Some(rules);
        dc.style_overrides.insert("Window_Looks".to_string(), overlay);

        let opening = test_opening(Some("Window_Looks"));
        let ctx = ViewStyleContext::new(&mv, Some(&dc), Some(&lib), false);
        let plan_frame = resolve_opening_view_style(&ctx, &opening, OpeningComponentSlot::Frame2D);
        assert_eq!(plan_frame.line_type.as_deref(), Some("PLAN_FRAME"));
        assert_eq!(plan_frame.layer_name.as_deref(), Some("PLAN_FRAME_LAYER"));
        assert_eq!(plan_frame.line_color, Some(Rgba8::new(220, 30, 40, 255)));

        let elevation_frame = resolve_opening_view_style(
            &ctx,
            &opening,
            OpeningComponentSlot::ElevationContour2D,
        );
        assert_eq!(elevation_frame.line_type.as_deref(), Some("STYLE_FRAME"));
        assert_eq!(elevation_frame.line_color, Some(Rgba8::new(220, 30, 40, 255)));
    }

    #[test]
    fn opening_look_inherits_material_line_and_unknown_material_keeps_defaults() {
        let mv = make_test_mv();
        let mut style = OpeningStyle::standard_window();
        style.style.id = "Window_MaterialLook".to_string();
        style.looks.fixed_frame.material_id = Some("Oak".to_string());
        let mut lib = StyleLibrary::default();
        let mut mat = material("Oak", "WOOD", 1.0, 0.0, true);
        mat.line_color = 0x12_34_56;
        mat.line_type = "DASHDOT".to_string();
        lib.materials.push(mat);
        lib.opening_styles.push(style);

        let opening = test_opening(Some("Window_MaterialLook"));
        let ctx = ViewStyleContext::new(&mv, None, Some(&lib), false);
        let inherited = resolve_opening_view_style(&ctx, &opening, OpeningComponentSlot::Frame2D);
        assert_eq!(inherited.line_type.as_deref(), Some("DASHDOT"));
        assert_eq!(inherited.line_color, Some(Rgba8::new(0x12, 0x34, 0x56, 255)));
        drop(ctx);

        lib.opening_styles[0].looks.fixed_frame.material_id = Some("missing".to_string());
        let ctx = ViewStyleContext::new(&mv, None, Some(&lib), false);
        let fallback = resolve_opening_view_style(&ctx, &opening, OpeningComponentSlot::Frame2D);
        assert_eq!(fallback.line_type, None);
        assert_eq!(fallback.line_color, None);
        assert_eq!(fallback.layer_name.as_deref(), Some("AEC_OPENING"));
    }

    #[test]
    fn absent_plan_level_keeps_style_suggestion() {
        let opening = test_opening(None);
        let grid = crate::modules::aec::engine::opening_style::DivisionGrid::default();
        let config = plan_dc();
        let view = make_test_mv();
        let ctx = ViewStyleContext::new(&view, Some(&config), None, false);
        let resolved = resolve_opening_representation_levels(&ctx, &opening, &grid);
        let suggested = crate::modules::aec::engine::opening_style::OpeningRepresentationLevels::suggested_for(
            opening.kind,
        );
        assert_eq!(resolved, suggested);
        assert_ne!(resolved.swing, crate::modules::aec::engine::opening_style::SwingGlyph::Off);
    }

    #[test]
    fn opening_representation_levels_do_not_follow_model_view_scale() {
        let opening = test_opening(None);
        let grid = crate::modules::aec::engine::opening_style::DivisionGrid::default();
        let config = plan_dc();
        let mut fine_view = make_test_mv();
        fine_view.scale = crate::modules::aec::engine::views::ScaleDenominator::new(
            50.0,
        )
        .unwrap();
        let mut coarse_view = fine_view.clone();
        coarse_view.scale = crate::modules::aec::engine::views::ScaleDenominator::new(
            200.0,
        )
        .unwrap();
        let fine_context = ViewStyleContext::new(&fine_view, Some(&config), None, false);
        let coarse_context = ViewStyleContext::new(&coarse_view, Some(&config), None, false);
        assert_eq!(
            resolve_opening_representation_levels(&fine_context, &opening, &grid),
            resolve_opening_representation_levels(&coarse_context, &opening, &grid)
        );
    }

    #[test]
    fn test_model_view_override_matches_component_and_suppresses_single_layer() {
        let mut mv = make_test_mv();
        let wall = make_three_layer_wall("Concrete");
        mv.graphic_overrides.push(GraphicOverride {
            id: OverrideId::new(),
            priority: 1,
            enabled: true,
            selector: OverrideSelector::Component {
                element: wall.element_ref,
                component_id: wall.layers[1].component_id,
            },
            patch: GraphicPatch {
                draw: Some(false),
                ..Default::default()
            },
        });
        let ctx = ViewStyleContext::new(&mv, None, None, false);
        assert!(is_wall_layer_visible(&ctx, &wall, 0));
        assert!(!is_wall_layer_visible(&ctx, &wall, 1));
        assert!(is_wall_layer_visible(&ctx, &wall, 2));
    }

    #[test]
    fn test_context_is_io_free_and_neutral_without_config() {
        let mv = make_test_mv();
        let wall = make_test_wall("Concrete");
        let ctx = ViewStyleContext::new(&mv, None, None, false);
        // Without config/library everything is visible with the documented last-resort hatch.
        assert!(is_wall_layer_visible(&ctx, &wall, 0));
        assert!(is_wall_layer_visible(&ctx, &wall, 1));
        let fill = resolve_layer_cut_style(&ctx, &wall, 0).1.expect("fill");
        assert_eq!(fill.hatch_pattern.as_deref(), Some("ANSI31"));
    }

    #[test]
    fn test_contour_hidden_while_hatch_visible_yields_no_boundary_style() {
        let mv = make_test_mv();
        let wall = make_test_wall("Concrete");
        let mut lib = StyleLibrary::default();
        lib.materials.push(material("Concrete", "AR-CONC", 1.0, 0.0, true));

        let mut ws = wall_style_for(&wall);
        let mut profile = ComponentRuleSet::default();
        profile.visibility.insert(WallComponentSlot::Contour2D.key().to_string(), false);
        profile.visibility.insert(WallComponentSlot::Layers2D.key().to_string(), false);
        profile.visibility.insert(WallComponentSlot::LayerHatch2D.key().to_string(), true);
        ws.default_profile = Some(profile);
        lib.wall_styles.push(ws);

        let ctx = ViewStyleContext::new(&mv, None, Some(&lib), false);
        let (boundary, fill) = resolve_layer_cut_style(&ctx, &wall, 0);
        assert!(boundary.is_none(), "Boundary style must be None when contour slots are hidden");
        assert!(fill.is_some(), "Fill style must be present when LayerHatch2D is visible");
        assert!(is_wall_layer_visible(&ctx, &wall, 0), "Layer is still visible because hatch is drawn");
    }

    #[test]
    fn test_plan_override_cannot_undo_opening_symbols_detail() {
        let mv = make_test_mv();
        let op = test_opening(Some("Door_Test"));

        let mut dc = plan_dc();
        dc.opening_levels.swing = Some(crate::modules::aec::engine::opening_style::SwingGlyph::Off);
        let mut overlay = StyleDisplayOverlay::default();
        let mut rules = ComponentRuleSet::default();
        rules.visibility.insert(OpeningComponentSlot::Swing2D.key().to_string(), true);
        overlay.rules = Some(rules);
        dc.style_overrides.insert("Door_Test".to_string(), overlay);

        let ctx = ViewStyleContext::new(&mv, Some(&dc), None, false);
        assert_eq!(
            resolve_opening_representation_levels(
                &ctx,
                &op,
                &crate::modules::aec::engine::opening_style::DivisionGrid::default(),
            )
            .swing,
            crate::modules::aec::engine::opening_style::SwingGlyph::Off
        );
    }

    #[test]
    fn test_opening_visibility_wins_over_plan_override() {
        let mv = make_test_mv();
        let op = test_opening(Some("Door_Test"));

        let mut dc = plan_dc();
        dc.opening_levels.section.exterior_sill = Some(false);
        let mut overlay = StyleDisplayOverlay::default();
        let mut rules = ComponentRuleSet::default();
        rules.visibility.insert(OpeningComponentSlot::Sill2D.key().to_string(), true);
        overlay.rules = Some(rules);
        dc.style_overrides.insert("Door_Test".to_string(), overlay);

        let ctx = ViewStyleContext::new(&mv, Some(&dc), None, false);
        assert!(!resolve_opening_section_roles(&ctx, &op).exterior_sill);
    }


    #[test]
    fn test_plan_opening_profile_hide_survives_plan_visibility_true() {
        let mv = make_test_mv();
        let mut op = test_opening(Some("Window_Test"));
        op.kind = OpeningKind::Window;
        let mut style = OpeningStyle::standard_window();
        style.style.id = "Window_Test".to_string();
        let mut lib = StyleLibrary::default();
        lib.opening_styles.push(style);
        let mut dc = plan_dc();
        let mut profile = OpeningDisplayProfile::default();
        profile.visibility.insert(OpeningComponentSlot::Sill2D.key().to_string(), false);
        dc.style_overrides.insert(
            "Window_Test".to_string(),
            StyleDisplayOverlay { opening: Some(profile), ..Default::default() },
        );

        let ctx = ViewStyleContext::new(&mv, Some(&dc), Some(&lib), false);
        assert!(!is_opening_slot_visible(&ctx, &op, OpeningComponentSlot::Sill2D));
        assert!(is_opening_slot_visible(&ctx, &op, OpeningComponentSlot::Frame2D));
    }

    #[test]
    fn test_graphic_override_priority_aware_draw() {
        let mut mv = make_test_mv();
        let wall = make_three_layer_wall("Concrete");

        // Element-level rule: draw = false, priority = 1
        mv.graphic_overrides.push(GraphicOverride {
            id: OverrideId::new(),
            priority: 1,
            enabled: true,
            selector: OverrideSelector::Element(wall.element_ref),
            patch: GraphicPatch {
                draw: Some(false),
                ..Default::default()
            },
        });

        // Component-level rule for layer 1: draw = true, priority = 10
        mv.graphic_overrides.push(GraphicOverride {
            id: OverrideId::new(),
            priority: 10,
            enabled: true,
            selector: OverrideSelector::Component {
                element: wall.element_ref,
                component_id: wall.layers[1].component_id,
            },
            patch: GraphicPatch {
                draw: Some(true),
                ..Default::default()
            },
        });

        let ctx = ViewStyleContext::new(&mv, None, None, false);
        // Layer 0 has only the element-level rule (draw = false)
        assert!(!is_wall_layer_active(&ctx, &wall, 0));
        // Layer 1 has the higher-priority component-level rule (draw = true) which overrides element-level
        assert!(is_wall_layer_active(&ctx, &wall, 1));
        // Layer 2 has only the element-level rule (draw = false)
        assert!(!is_wall_layer_active(&ctx, &wall, 2));
    }
}
