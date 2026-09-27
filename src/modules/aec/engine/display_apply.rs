//! Phase filters, layer style overrides, and DisplayConfig apply.

#![allow(unused_imports)]
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

use acadrust::entities::{LwPolyline, LwVertex, Point};
use acadrust::tables::AppId;
use acadrust::types::{Vector2, Vector3};
use acadrust::{CadDocument, EntityType, Handle};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use glam::DVec3;

use crate::scene::model::hatch_model::{HatchModel, HatchPattern};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

use super::{
    self as engine, Storey, StyleLibrary, Wall, WallJustification, WallLayer,
    join::{self, JoinError, JoinKind},
    junction_solver::{self, WallJoinInput},
    library::load_or_seed,
    plan_view::{PhaseFilter, PlanPhase},
    wall_style::{
        base_width_from_layers, effective_layers_for_wall_bb, migrate_gap_before_to_axis_offset,
        LayerFunction, ResolvedLayer, WallStyle,
    },
};

#[allow(unused_imports)]
use super::junction_pick::*;
use super::xdata::*;
use super::wall_package::*;
use super::wall_regen::*;
use super::join_ops::*;
use super::storey_xdata::*;
use super::opening_xdata::*;

/// User-visible notices queued when a stored [`join::JunctionOverride`] is
/// found to reference a layer/material that no longer exists and is
/// automatically cleaned up during regeneration (see
/// `validate_junction_override` / `remove_junction_override`). Regeneration
/// helpers (`regenerate_wall_representation_inner`, `join_junction_in_document`)
/// don't have direct access to a [`CommandLine`], so they queue the message
/// here; command entry points that do have one (e.g. `aec_walljoin_do`)
/// drain it via [`take_pending_override_warnings`] and surface it the same
/// way other non-fatal warnings are reported (`command_line.push_info`).
pub(crate) static PENDING_OVERRIDE_WARNINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub(crate) fn queue_override_warning(msg: String) {
    if let Ok(mut q) = PENDING_OVERRIDE_WARNINGS.lock() {
        q.push(msg);
    }
}

/// Drain and return every queued override-invalidation notice since the last
/// call. Command entry points with a [`CommandLine`] should call this after
/// a regeneration/join and forward each message via `command_line.push_info`.
pub fn take_pending_override_warnings() -> Vec<String> {
    PENDING_OVERRIDE_WARNINGS
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

pub(crate) fn layer_ref_matches(r: &join::LayerRef, set: &[join::LayerRef]) -> bool {
    set.iter().any(|l| {
        match (l.layer_id, r.layer_id) {
            (Some(a), Some(b)) => a == b,
            _ => l.material_id == r.material_id && l.role_tag == r.role_tag && l.index == r.index,
        }
    })
}

/// Merge a `ComponentRuleSet`'s slot-level `style_override` (checked in
/// `slots` priority order, first match per field wins) with its
/// `layer_style_override` entry matching `layer_ref` (fills any field the
/// slot override(s) left unset) — precedence tiers (a)/(b) of the Step 3
/// style resolution chain in `.junie/plans/aec-plan-view-display-variants.md`.
/// Returns an all-`None` [`engine::display_component::ComponentStyleOverride`]
/// when `rules` is `None` or nothing applies, so callers can always fall
/// through unconditionally to the `style_substitutions`/`hatch_override`/
/// material tiers below.
pub(crate) fn resolve_layer_style_override(
    rules: Option<&engine::display_component::ComponentRuleSet>,
    slots: &[engine::display_component::WallComponentSlot],
    layer_ref: &join::LayerRef,
) -> engine::display_component::ComponentStyleOverride {
    let mut out = engine::display_component::ComponentStyleOverride::default();
    let Some(rules) = rules else {
        return out;
    };
    // Detailed per-layer override first (more specific than a whole-slot
    // override), then each slot in priority order fills any remaining gaps.
    if let Some(lso) = rules.layer_style_override.iter().find(|lso| {
        match (lso.layer.layer_id, layer_ref.layer_id) {
            (Some(a), Some(b)) => a == b,
            _ => {
                lso.layer.material_id == layer_ref.material_id
                    && lso.layer.role_tag == layer_ref.role_tag
                    && lso.layer.index == layer_ref.index
            }
        }
    }) {
        out.line_type = lso.style.line_type.clone();
        out.line_color = lso.style.line_color;
        out.hatch_pattern = lso.style.hatch_pattern.clone();
        out.hatch_color = lso.style.hatch_color;
        out.hatch_scale = lso.style.hatch_scale;
        out.fill_color = lso.style.fill_color;
        out.hatch_angle = lso.style.hatch_angle;
        out.hatch_angle_relative = lso.style.hatch_angle_relative;
        out.cad_layer = lso.style.cad_layer.clone();
    }
    for slot in slots {
        if let Some(s) = rules.style_for(*slot) {
            if out.line_type.is_none() {
                out.line_type = s.line_type.clone();
            }
            if out.line_color.is_none() {
                out.line_color = s.line_color;
            }
            if out.hatch_pattern.is_none() {
                out.hatch_pattern = s.hatch_pattern.clone();
            }
            if out.hatch_color.is_none() {
                out.hatch_color = s.hatch_color;
            }
            if out.hatch_scale.is_none() {
                out.hatch_scale = s.hatch_scale;
            }
            if out.fill_color.is_none() {
                out.fill_color = s.fill_color;
            }
            if out.hatch_angle.is_none() {
                out.hatch_angle = s.hatch_angle;
            }
            if out.hatch_angle_relative.is_none() {
                out.hatch_angle_relative = s.hatch_angle_relative;
            }
            if out.cad_layer.is_none() {
                out.cad_layer = s.cad_layer.clone();
            }
        }
    }
    out
}

/// Result of resolving a [`PhaseFilter`] against a single wall's
/// [`PlanPhase`]: whether the wall should be shown at all under this
/// `DisplayConfig`, plus an optional extra style overlay (dashed lines for
/// `Demolition`, greyed-out for `Existing`, ...) to merge on top of the
/// wall's normally resolved style.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PhaseFilterResult {
    /// `false` means the wall must be hidden entirely for this config.
    pub visible: bool,
    /// Extra style overlay for `Demolition`/`Existing` walls; `None` for
    /// `New` walls or when the filter defines no overlay for this phase.
    pub extra_style: Option<engine::display_component::ComponentStyleOverride>,
}

/// Resolves a [`DisplayConfig::phase_filter`] against a wall's `phase`.
///
/// `filter == None` keeps every phase visible and applies built-in extras
/// (dashed demolition contour, grey existing contour). When a filter is
/// present, `phase` must be listed in `visible_phases`; `Demolition` /
/// `Existing` pick up `demolition_style`/`existing_style` or the same
/// built-in extras when those fields are unset.
pub(crate) fn default_phase_extra_style(
    phase: PlanPhase,
) -> Option<engine::display_component::ComponentStyleOverride> {
    match phase {
        PlanPhase::Demolition => Some(engine::display_component::ComponentStyleOverride {
            line_type: Some("DASHED".to_string()),
            ..Default::default()
        }),
        PlanPhase::Existing => Some(engine::display_component::ComponentStyleOverride {
            line_color: Some(acadrust::types::Color::Rgb {
                r: 136,
                g: 136,
                b: 136,
            }),
            ..Default::default()
        }),
        PlanPhase::New => None,
    }
}

pub fn apply_phase_filter(phase: PlanPhase, filter: Option<&PhaseFilter>) -> PhaseFilterResult {
    let Some(filter) = filter else {
        return PhaseFilterResult {
            visible: true,
            extra_style: default_phase_extra_style(phase),
        };
    };
    let visible = filter.visible_phases.contains(&phase);
    let extra_style = match phase {
        PlanPhase::Demolition => filter
            .demolition_style
            .clone()
            .or_else(|| default_phase_extra_style(phase)),
        PlanPhase::Existing => filter
            .existing_style
            .clone()
            .or_else(|| default_phase_extra_style(phase)),
        PlanPhase::New => None,
    };
    PhaseFilterResult {
        visible,
        extra_style,
    }
}

pub(crate) fn merge_phase_extra_into_rules(
    rules: &mut engine::display_component::ComponentRuleSet,
    extra: engine::display_component::ComponentStyleOverride,
) {
    let slot = engine::display_component::WallComponentSlot::Contour2D
        .key()
        .to_string();
    rules
        .style_override
        .entry(slot)
        .or_default()
        .overlay_from(&extra);
}

pub(crate) fn hide_all_display_slots(rules: &mut engine::display_component::ComponentRuleSet) {
    for slot in [
        engine::display_component::WallComponentSlot::Contour2D,
        engine::display_component::WallComponentSlot::Layers2D,
        engine::display_component::WallComponentSlot::ContourHatch2D,
        engine::display_component::WallComponentSlot::LayerHatch2D,
        engine::display_component::WallComponentSlot::Solid3D,
    ] {
        rules.visibility.insert(slot.key().to_string(), false);
    }
}

/// Build the [`join::LayerRef`] list for a wall's layer stack (given its
/// materials in layer order), keeping the `index` field aligned with each
/// layer's position — required so layers that reuse the same material (e.g.
/// two plaster layers) stay individually addressable by
/// [`join::LayerPairOverride`] instead of colliding.
pub(crate) fn layer_refs_from_materials<'a>(
    layers: impl IntoIterator<Item = (&'a str, Uuid)>,
) -> Vec<join::LayerRef> {
    layers
        .into_iter()
        .enumerate()
        .map(|(i, (m, id))| join::LayerRef {
            material_id: m.to_string(),
            role_tag: None,
            index: i,
            layer_id: Some(id),
        })
        .collect()
}

/// Regenerates every wall in `scene` under `config`'s wall
/// [`engine::display_component::ComponentRuleSet`] (via
/// [`DisplayConfig::wall_rules`]) and `style_substitutions`. This is the
/// entry point the "active DisplayConfig" dropdown/manager (Step 5) uses
/// to apply a selected `DisplayConfig` to the whole document at once,
/// mirroring what [`regenerate_wall_representation_with_rules_and_substitutions`]
/// does for a single wall. Returns every handle touched (axis + derived),
/// same convention as [`refresh_wall_after_axis_edit`]. Walls whose
/// regeneration fails (e.g. no layers) are skipped silently, same as a
/// single-wall regeneration failure would be.
pub fn apply_display_config_to_scene(
    scene: &mut Scene,
    config: &engine::plan_view::DisplayConfig,
    library_override: Option<&StyleLibrary>,
) -> Vec<Handle> {
    apply_display_config_to_scene_with_representation(scene, config, library_override, None)
}

/// Like [`apply_display_config_to_scene`], but a session `RepresentationMode`
/// (status-bar 2D/3D/All) overrides the plan-type default.
pub fn apply_display_config_to_scene_with_representation(
    scene: &mut Scene,
    config: &engine::plan_view::DisplayConfig,
    library_override: Option<&StyleLibrary>,
    representation_override: Option<engine::display_component::RepresentationMode>,
) -> Vec<Handle> {
    // `DisplayConfig::wall_rules`/`style_substitutions` were removed in
    // Step 2 (overrides now live per-wall-style on
    // `WallStyle::display_profiles`, keyed by `DisplayConfig::name`); each
    // wall's own style is resolved against `config.name` via
    // `engine::library::resolve_effective_rule_set` below, instead of a
    // single document-wide rule set. `style_substitutions` has no
    // successor concept (removed without migration), so it's always
    // `None`. `config.phase_filter` (Step 1) hides walls whose `phase`
    // isn't in `visible_phases`; visible demolition/existing walls get an
    // additional style overlay merged on top of their resolved rules.
    let owned_lib = library_override.map_or_else(
        || engine::project::resolve_style_library(None),
        |l| l.clone(),
    );
    let library_override = Some(&owned_lib);
    let mut touched = Vec::new();
    for wall_handle in all_wall_axis_handles(scene) {
        let Some(entity) = scene.document.get_entity(wall_handle) else {
            continue;
        };
        let Some(wall) = wall_from_entity(entity) else {
            continue;
        };
        let filter_result = apply_phase_filter(wall.phase, config.phase_filter.as_ref());
        if !filter_result.visible {
            let mut hidden = engine::display_component::ComponentRuleSet::default();
            hide_all_display_slots(&mut hidden);
            if let Ok(handles) = regenerate_wall_representation_with_rules_and_substitutions(
                scene,
                wall_handle,
                Some(&hidden),
                None,
                library_override,
            ) {
                touched.extend(handles);
            }
            continue;
        }

        let style = owned_lib
            .wall_styles
            .iter()
            .find(|ws| ws.style.id == wall.style_id);
        let mut rules = engine::library::build_effective_rule_set(
            config,
            style,
            representation_override,
        );
        // Phase extras apply only to 2D overall contour lines (fieldwise).
        if let Some(ov) = filter_result.extra_style {
            merge_phase_extra_into_rules(&mut rules, ov);
        }
        let effective_rules = Some(rules);

        if let Ok(handles) = regenerate_wall_representation_with_rules_and_substitutions(
            scene,
            wall_handle,
            effective_rules.as_ref(),
            None,
            library_override,
        ) {
            touched.extend(handles);
        }
    }

    // Regenerates every slab under the active DisplayConfig / representation mode.
    for slab_handle in super::slab_package::all_slab_carrier_handles(scene) {
        let Some(entity) = scene.document.get_entity(slab_handle) else {
            continue;
        };
        let Some(slab) = super::slab_xdata::slab_from_entity(entity) else {
            continue;
        };
        let filter_result = apply_phase_filter(slab.phase, config.phase_filter.as_ref());
        if !filter_result.visible {
            let mut hidden = engine::display_component::ComponentRuleSet::default();
            hide_all_slab_display_slots(&mut hidden);
            if super::slab_regen::regenerate_slab_representation(
                scene,
                slab_handle,
                library_override,
                Some(&hidden),
            ) {
                touched.push(slab_handle);
                touched.extend(super::slab_package::slab_package_handles(scene, slab_handle));
            }
            continue;
        }

        let style = owned_lib
            .slab_styles
            .iter()
            .find(|ss| ss.style.id == slab.style_id);
        let mut rules = engine::library::build_effective_slab_rule_set(
            config,
            style,
            representation_override,
        );
        if let Some(ov) = filter_result.extra_style {
            merge_phase_extra_into_slab_rules(&mut rules, ov);
        }
        if super::slab_regen::regenerate_slab_representation(
            scene,
            slab_handle,
            library_override,
            Some(&rules),
        ) {
            touched.push(slab_handle);
            touched.extend(super::slab_package::slab_package_handles(scene, slab_handle));
        }
    }

    touched.sort_by_key(|h| h.value());
    touched.dedup();
    touched
}

pub(crate) fn hide_all_slab_display_slots(
    rules: &mut engine::display_component::ComponentRuleSet,
) {
    for slot in engine::display_component::SlabComponentSlot::all() {
        rules.visibility.insert(slot.key().to_string(), false);
    }
}

pub(crate) fn merge_phase_extra_into_slab_rules(
    rules: &mut engine::display_component::ComponentRuleSet,
    extra: engine::display_component::ComponentStyleOverride,
) {
    let slot = engine::display_component::SlabComponentSlot::Contour2D
        .key()
        .to_string();
    rules
        .style_override
        .entry(slot)
        .or_default()
        .overlay_from(&extra);
}

#[cfg(test)]
mod slab_display_tests {
    use super::*;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use crate::modules::aec::engine::display_component::{
        RepresentationMode, SlabComponentSlot,
    };
    use crate::modules::aec::engine::plan_view::{
        DisplayConfig, PlanningStage, ViewType,
    };
    use crate::modules::aec::engine::slab::{Slab, SlabLayer};
    use crate::modules::aec::engine::slab_package::all_slab_carrier_handles;
    use crate::modules::aec::engine::slab_xdata::{slab_from_entity, write_slab_record};
    use crate::modules::aec::engine::wall_style::LayerFunction;
    use crate::scene::Scene;

    fn add_test_slab(scene: &mut Scene) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 4.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 4.0)));
        pl.is_closed = true;
        let handle = scene.add_entity(EntityType::LwPolyline(pl));
        let mut slab = Slab::new("style_slab_concrete_20", 1);
        slab.layers = vec![SlabLayer::new(
            "Concrete",
            0.20,
            LayerFunction::Structural,
        )];
        assert!(write_slab_record(&mut scene.document, handle, &slab));
        handle
    }

    #[test]
    fn apply_display_config_touches_slab_carriers() {
        let mut scene = Scene::new();
        let handle = add_test_slab(&mut scene);
        assert_eq!(all_slab_carrier_handles(&scene), vec![handle]);

        let cfg = DisplayConfig::new(
            "Plan".into(),
            "Plan".into(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        let lib = load_or_seed();
        let touched = apply_display_config_to_scene_with_representation(
            &mut scene,
            &cfg,
            Some(&lib),
            Some(RepresentationMode::TwoD),
        );
        assert!(
            touched.contains(&handle),
            "display apply should touch the slab carrier"
        );
        let entity = scene.document.get_entity(handle).expect("slab entity");
        let slab = slab_from_entity(entity).expect("slab xdata");
        // 2D mode should still keep the slab record intact.
        assert_eq!(slab.style_id, "style_slab_concrete_20");
        assert!((slab.total_thickness() - 0.20).abs() < 1e-9);
        let _ = SlabComponentSlot::Contour2D;
    }

    #[test]
    fn slab_xdata_handles_are_typed_for_xref_remapping() {
        use acadrust::xdata::XDataValue;
        use crate::modules::aec::engine::slab_xdata::slab_record_for_slab;

        let mut slab = Slab::new("s", 1);
        slab.derived_handles = vec![Handle::from(11u64), Handle::from(12u64)];
        slab.opening_handles = vec![Handle::from(21u64)];
        let values = slab_record_for_slab(&slab);
        let handle_count = values
            .iter()
            .filter(|v| matches!(v, XDataValue::Handle(_)))
            .count();
        assert_eq!(
            handle_count, 3,
            "derived and opening handles must be typed XDataValue::Handle for generic XREF remap"
        );
    }
}
