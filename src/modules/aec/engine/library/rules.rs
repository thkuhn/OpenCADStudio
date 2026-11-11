use super::*;

/// Marker used by the style editors for "the style's own default profile"
/// (as opposed to the override of a named plan kind).
pub const STYLE_DEFAULT_PROFILE_KEY: &str = "*";

/// Merges `over` on top of `base` (entries of `over` win).
pub fn merge_rule_sets(base: &mut ComponentRuleSet, over: &ComponentRuleSet) {
    for (k, v) in &over.visibility {
        base.visibility.insert(k.clone(), *v);
    }
    for (k, v) in &over.style_override {
        base.style_override.insert(k.clone(), v.clone());
    }
    for (k, v) in &over.layer_filter {
        base.layer_filter.insert(k.clone(), v.clone());
    }
    for lso in &over.layer_style_override {
        match base
            .layer_style_override
            .iter_mut()
            .find(|e| e.layer.layer_id == lso.layer.layer_id)
        {
            Some(existing) => *existing = lso.clone(),
            None => base.layer_style_override.push(lso.clone()),
        }
    }
    for (k, v) in &over.opening_profiles {
        base.opening_profiles.insert(k.clone(), v.clone());
    }
}

/// Style display rules independent of any plan name: the style's own default
/// profile, overridden by the active plan kind's `style_overrides[style_id]`
/// (plan override > style default).
pub fn resolve_effective_rule_set(
    default_profile: Option<&ComponentRuleSet>,
    config: &DisplayConfig,
    style_id: Option<&str>,
) -> ComponentRuleSet {
    let mut rules = default_profile.cloned().unwrap_or_default();
    if let Some(over) = style_id
        .and_then(|id| config.style_overrides.get(id))
        .and_then(|o| o.rules.as_ref())
    {
        merge_rule_sets(&mut rules, over);
    }
    rules
}

/// Copies the plan's opening-style overrides into the rule set so opening
/// regen can resolve them without access to the `DisplayConfig`.
pub(super) fn attach_opening_profiles(rules: &mut ComponentRuleSet, config: &DisplayConfig) {
    for (id, overlay) in &config.style_overrides {
        if let Some(profile) = &overlay.opening {
            rules.opening_profiles.insert(id.clone(), profile.clone());
        }
    }
}

/// Session override wins over the plan-type default.
pub fn effective_representation(
    config: &DisplayConfig,
    session: Option<RepresentationMode>,
) -> RepresentationMode {
    session.unwrap_or(config.detail.representation)
}

/// Plan-type global visibility + representation + sparse style overlay,
/// starting from the wall style's default profile. Precedence (high to low):
/// session representation > plan `style_overrides` > style default profile.
pub fn build_effective_rule_set(
    config: &DisplayConfig,
    style: Option<&WallStyle>,
    session: Option<RepresentationMode>,
) -> ComponentRuleSet {
    let mut rules = resolve_effective_rule_set(
        style.and_then(|ws| ws.default_profile.as_ref()),
        config,
        style.map(|ws| ws.style.id.as_str()),
    );
    let mode = effective_representation(config, session);
    for kind in WallComponentKind::all() {
        let key = kind.to_slot().key().to_string();
        if session.is_some() {
            // Status-bar 2D/3D/All replaces plan type, plan override and
            // style default visibility for that representation.
            rules.visibility.insert(key, mode.allows(*kind));
        } else {
            let vis = component_is_visible(config, *kind, mode);
            rules
                .visibility
                .entry(key)
                .and_modify(|v| *v = *v && vis)
                .or_insert(vis);
        }
    }

    for slot in crate::modules::aec::engine::display_component::OpeningComponentSlot::all() {
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
    apply_wall_detail(&mut rules, &config.detail, style);
    if let Some(style) = style {
        if let Some(overlay) = config.style_overrides.get(&style.style.id) {
            merge_style_overlay_into_rules(&mut rules, overlay);
            apply_overlay_layer_visibility(&mut rules, overlay, style);
        }
    }
    attach_opening_profiles(&mut rules, config);
    apply_opening_level_visibility(&mut rules, &config.opening_levels);
    rules
}

/// Explicit plan levels hide the matching scene slots. Absent levels stay
/// visible so the style suggestion is not treated as hidden.
pub(super) fn apply_opening_level_visibility(
    rules: &mut ComponentRuleSet,
    levels: &crate::modules::aec::engine::opening_style::OpeningLevelOverride,
) {
    use crate::modules::aec::engine::display_component::OpeningComponentSlot;
    use crate::modules::aec::engine::opening_style::{
        DivisionLevel, ElevationFrameLevel, PlanFrameLevel, SwingGlyph,
    };
    rules.opening_division_level = levels.divisions;
    let mut hide = |slot: OpeningComponentSlot| {
        rules.visibility.insert(slot.key().to_string(), false);
    };
    if levels.swing == Some(SwingGlyph::Off) {
        hide(OpeningComponentSlot::Swing2D);
        hide(OpeningComponentSlot::ElevationSwing2D);
        hide(OpeningComponentSlot::BreakthroughSymbol2D);
    }
    if levels.plan_frame == Some(PlanFrameLevel::Off) {
        hide(OpeningComponentSlot::Frame2D);
        hide(OpeningComponentSlot::Leaf2D);
    }
    if levels.elevation_frame == Some(ElevationFrameLevel::Off) {
        hide(OpeningComponentSlot::ElevationContour2D);
    }
    if levels.divisions == Some(DivisionLevel::Off) {
        hide(OpeningComponentSlot::ElevationMuntins2D);
    }
    if levels.section.glazing == Some(false) {
        hide(OpeningComponentSlot::Glazing2D);
        hide(OpeningComponentSlot::Glazing3D);
    }
}

/// Plan-type + representation visibility for slab display slots, starting from
/// the slab style's default profile; precedence as for walls (session
/// representation > plan `style_overrides` > style default).
pub fn build_effective_slab_rule_set(
    config: &DisplayConfig,
    style: Option<&SlabStyle>,
    session: Option<RepresentationMode>,
) -> ComponentRuleSet {
    use crate::modules::aec::engine::display_component::{LayerSelection, SlabComponentSlot};
    use crate::modules::aec::engine::join::LayerRef;
    use crate::modules::aec::engine::wall_style::LayerFunction;

    let mut rules = resolve_effective_rule_set(
        style.and_then(|ss| ss.default_profile.as_ref()),
        config,
        style.map(|ss| ss.style.id.as_str()),
    );
    let mode = effective_representation(config, session);

    let detail = config.detail;
    let is_rcp = detail.ceiling_view;

    for slot in SlabComponentSlot::all() {
        let key = slot.key().to_string();
        let allowed_by_mode = match mode {
            RepresentationMode::All => true,
            RepresentationMode::TwoD => slot.is_2d(),
            RepresentationMode::ThreeD => slot.is_3d(),
        };

        let default_for_slot = match slot {
            SlabComponentSlot::Contour2D => !is_rcp,
            SlabComponentSlot::CeilingOutline2D => is_rcp,
            SlabComponentSlot::LayerHatch2D => {
                !is_rcp && detail.hatches && detail.layers != LayerDetail::Off
            }
            SlabComponentSlot::OpeningContour2D => true,
            SlabComponentSlot::OpeningSymbol2D => !is_rcp,
            SlabComponentSlot::Solid3D => true,
            SlabComponentSlot::SurfaceStyle3D => true,
        };

        if session.is_some() {
            rules.visibility.insert(key, allowed_by_mode && default_for_slot);
        } else if !allowed_by_mode {
            rules.visibility.insert(key, false);
        } else {
            rules
                .visibility
                .entry(key)
                .and_modify(|v| *v = *v && default_for_slot)
                .or_insert(default_for_slot);
        }
    }

    let is_structural_only = detail.layers == LayerDetail::StructuralOnly;

    if is_structural_only {
        if let Some(style) = style {
            let structural_refs: Vec<LayerRef> = style
                .layers
                .iter()
                .enumerate()
                .filter(|(_, l)| l.function == LayerFunction::Structural)
                .map(|(i, l)| LayerRef {
                    material_id: l.material_id.clone(),
                    role_tag: None,
                    index: i,
                    layer_id: Some(l.layer_id),
                })
                .collect();
            if !structural_refs.is_empty() {
                rules.layer_filter.insert(
                    SlabComponentSlot::Solid3D.key().to_string(),
                    LayerSelection::Explicit(structural_refs.clone()),
                );
                rules.layer_filter.insert(
                    SlabComponentSlot::LayerHatch2D.key().to_string(),
                    LayerSelection::Explicit(structural_refs),
                );
            }
        }
    }

    if let (Some(style), Some(overlay)) = (style, style.and_then(|style| config.style_overrides.get(&style.style.id))) {
        merge_style_overlay_into_rules(&mut rules, overlay);
        apply_overlay_slab_layer_visibility(&mut rules, overlay, style);
    }

    attach_opening_profiles(&mut rules, config);
    rules
}

/// Applies the plan kind's [`DetailProfile`] switches to a wall rule set.
/// Switches only ever *hide* (a `true` switch leaves visibility to the style
/// default and the plan's `style_overrides`, which are merged afterwards).
pub(super) fn apply_wall_detail(rules: &mut ComponentRuleSet, detail: &DetailProfile, style: Option<&WallStyle>) {
    use crate::modules::aec::engine::display_component::OpeningComponentSlot;
    let mut hide = |key: &str| {
        rules.visibility.insert(key.to_string(), false);
    };
    if !detail.hatches {
        hide(WallComponentSlot::ContourHatch2D.key());
        hide(WallComponentSlot::LayerHatch2D.key());
    }
    if detail.layers == LayerDetail::Off {
        hide(WallComponentSlot::Layers2D.key());
        hide(WallComponentSlot::LayerHatch2D.key());
    }
    if !detail.annotation {
        hide(OpeningComponentSlot::OpeningLabel2D.key());
    }
    if detail.layers == LayerDetail::StructuralOnly {
        if let Some(style) = style {
            let refs: Vec<LayerRef> = style
                .layers
                .iter()
                .enumerate()
                .filter(|(_, l)| l.function == LayerFunction::Structural)
                .map(|(i, l)| LayerRef {
                    material_id: l.material_id.clone(),
                    role_tag: l.role_tag.clone(),
                    index: i,
                    layer_id: Some(l.layer_id),
                })
                .collect();
            if !refs.is_empty() {
                for slot in [
                    WallComponentSlot::Layers2D,
                    WallComponentSlot::LayerHatch2D,
                    WallComponentSlot::Solid3D,
                ] {
                    rules
                        .layer_filter
                        .insert(slot.key().to_string(), LayerSelection::Explicit(refs.clone()));
                }
            }
        }
    }
}

pub(super) fn apply_overlay_layer_visibility(
    rules: &mut ComponentRuleSet,
    overlay: &StyleDisplayOverlay,
    style: &WallStyle,
) {
    if overlay.layer_visibility.is_empty() || style.layers.is_empty() {
        return;
    }
    let refs_2d: Vec<LayerRef> = style
        .layers
        .iter()
        .enumerate()
        .filter_map(|(i, layer)| {
            let vis = overlay
                .layer_visibility
                .get(&layer.layer_id)
                .copied()
                .unwrap_or_default();
            if vis.visible_2d {
                Some(LayerRef {
                    material_id: layer.material_id.clone(),
                    role_tag: layer.role_tag.clone(),
                    index: i,
                    layer_id: Some(layer.layer_id),
                })
            } else {
                None
            }
        })
        .collect();
    if refs_2d.len() != style.layers.len() {
        rules.layer_filter.insert(
            WallComponentSlot::Layers2D.key().to_string(),
            LayerSelection::Explicit(refs_2d.clone()),
        );
        rules.layer_filter.insert(
            WallComponentSlot::LayerHatch2D.key().to_string(),
            LayerSelection::Explicit(refs_2d),
        );
    }
    let refs_3d: Vec<LayerRef> = style
        .layers
        .iter()
        .enumerate()
        .filter_map(|(i, layer)| {
            let vis = overlay
                .layer_visibility
                .get(&layer.layer_id)
                .copied()
                .unwrap_or_default();
            if vis.visible_3d {
                Some(LayerRef {
                    material_id: layer.material_id.clone(),
                    role_tag: layer.role_tag.clone(),
                    index: i,
                    layer_id: Some(layer.layer_id),
                })
            } else {
                None
            }
        })
        .collect();
    if refs_3d.len() != style.layers.len() {
        rules.layer_filter.insert(
            WallComponentSlot::Solid3D.key().to_string(),
            LayerSelection::Explicit(refs_3d),
        );
    }
}

pub(super) fn apply_overlay_slab_layer_visibility(
    rules: &mut ComponentRuleSet,
    overlay: &StyleDisplayOverlay,
    style: &SlabStyle,
) {
    if overlay.layer_visibility.is_empty() || style.layers.is_empty() {
        return;
    }
    let make_layer_ref = |index: usize, layer: &SlabStyleLayer| LayerRef {
        material_id: layer.material_id.clone(),
        role_tag: None,
        index,
        layer_id: Some(layer.layer_id),
    };

    for slot in [
        SlabComponentSlot::Contour2D,
        SlabComponentSlot::CeilingOutline2D,
        SlabComponentSlot::LayerHatch2D,
    ] {
        let existing = rules.slab_layer_filter_for(slot).clone();
        let visible: Vec<LayerRef> = style
            .layers
            .iter()
            .enumerate()
            .filter_map(|(index, layer)| {
                let visibility = overlay
                    .layer_visibility
                    .get(&layer.layer_id)
                    .copied()
                    .unwrap_or_default();
                let layer_ref = make_layer_ref(index, layer);
                let included_by_existing = match &existing {
                    LayerSelection::All => true,
                    LayerSelection::Explicit(refs) => {
                        crate::modules::aec::engine::display_apply::layer_ref_matches(&layer_ref, refs)
                    }
                };
                (visibility.visible_2d && included_by_existing).then_some(layer_ref)
            })
            .collect();
        if visible.len() != style.layers.len() || matches!(existing, LayerSelection::Explicit(_)) {
            rules
                .layer_filter
                .insert(slot.key().to_string(), LayerSelection::Explicit(visible));
        }
    }

    for slot in [SlabComponentSlot::Solid3D, SlabComponentSlot::SurfaceStyle3D] {
        let existing = rules.slab_layer_filter_for(slot).clone();
        let visible: Vec<LayerRef> = style
            .layers
            .iter()
            .enumerate()
            .filter_map(|(index, layer)| {
                let visibility = overlay
                    .layer_visibility
                    .get(&layer.layer_id)
                    .copied()
                    .unwrap_or_default();
                let layer_ref = make_layer_ref(index, layer);
                let included_by_existing = match &existing {
                    LayerSelection::All => true,
                    LayerSelection::Explicit(refs) => {
                        crate::modules::aec::engine::display_apply::layer_ref_matches(&layer_ref, refs)
                    }
                };
                (visibility.visible_3d && included_by_existing).then_some(layer_ref)
            })
            .collect();
        if visible.len() != style.layers.len() || matches!(existing, LayerSelection::Explicit(_)) {
            rules
                .layer_filter
                .insert(slot.key().to_string(), LayerSelection::Explicit(visible));
        }
    }
}

/// Global plan-type visibility ∩ representation mode.
pub fn component_is_visible(
    config: &DisplayConfig,
    kind: WallComponentKind,
    mode: RepresentationMode,
) -> bool {
    if !mode.allows(kind) {
        return false;
    }
    config.component_visibility.get(&kind).copied().unwrap_or(true)
}

/// Field-wise look: overlay.layer_props → layer-direct → material → default.
pub fn resolve_layer_property_override(
    config: &DisplayConfig,
    style_id: &str,
    layer_id: Uuid,
    layer_direct: Option<&ComponentStyleOverride>,
    material: Option<&Material>,
) -> ComponentStyleOverride {
    let overlay = config
        .style_overrides
        .get(style_id)
        .and_then(|o| o.layer_props.get(&layer_id));
    let mut out = ComponentStyleOverride::default();
    let pick = |overlay: Option<&str>, direct: Option<&str>, mat: Option<&str>| {
        overlay
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| direct.filter(|s| !s.is_empty()).map(|s| s.to_string()))
            .or_else(|| mat.filter(|s| !s.is_empty()).map(|s| s.to_string()))
    };
    out.line_type = pick(
        overlay.and_then(|o| o.line_type.as_deref()),
        layer_direct.and_then(|d| d.line_type.as_deref()),
        material.map(|m| m.line_type.as_str()),
    );
    out.line_color = overlay
        .and_then(|o| o.line_color)
        .or_else(|| layer_direct.and_then(|d| d.line_color));
    out.hatch_pattern = pick(
        overlay.and_then(|o| o.hatch_pattern.as_deref()),
        layer_direct.and_then(|d| d.hatch_pattern.as_deref()),
        material.map(|m| m.hatch_pattern.as_str()),
    );
    out.hatch_color = overlay
        .and_then(|o| o.hatch_color)
        .or_else(|| layer_direct.and_then(|d| d.hatch_color))
        .or_else(|| material.and_then(|m| m.hatch_color));
    out.hatch_scale = overlay
        .and_then(|o| o.hatch_scale)
        .or_else(|| layer_direct.and_then(|d| d.hatch_scale))
        .or_else(|| material.map(|m| m.hatch_scale));
    if let Some(s) = out.hatch_scale {
        if s <= 0.0 {
            out.hatch_scale = Some(0.01);
        }
    }
    out.fill_color = overlay
        .and_then(|o| o.fill_color)
        .or_else(|| layer_direct.and_then(|d| d.fill_color));
    out.hatch_angle = overlay
        .and_then(|o| o.hatch_angle)
        .or_else(|| layer_direct.and_then(|d| d.hatch_angle))
        .or_else(|| material.map(|m| m.hatch_angle));
    out.hatch_angle_relative = overlay
        .and_then(|o| o.hatch_angle_relative)
        .or_else(|| layer_direct.and_then(|d| d.hatch_angle_relative))
        .or_else(|| material.map(|m| m.hatch_angle_relative));
    out.cad_layer = pick(
        overlay.and_then(|o| o.cad_layer.as_deref()),
        layer_direct.and_then(|d| d.cad_layer.as_deref()),
        None,
    );
    out
}

/// Phase extras apply only to 2D overall contour lines, never hatch/layers.
pub fn phase_style_for_slot(
    extra: Option<&ComponentStyleOverride>,
    slot: WallComponentSlot,
) -> Option<&ComponentStyleOverride> {
    if slot == WallComponentSlot::Contour2D {
        extra
    } else {
        None
    }
}

/// Fold a plan-type style overlay into a `ComponentRuleSet` used by regen.
pub fn merge_style_overlay_into_rules(rules: &mut ComponentRuleSet, overlay: &StyleDisplayOverlay) {
    for (layer_id, props) in &overlay.layer_props {
        if let Some(existing) = rules
            .layer_style_override
            .iter_mut()
            .find(|lso| lso.layer.layer_id == Some(*layer_id))
        {
            existing.style = merge_override_fields(props, &existing.style);
        } else {
            rules.layer_style_override.push(LayerStyleOverride {
                layer: LayerRef {
                    material_id: String::new(),
                    role_tag: None,
                    index: 0,
                    layer_id: Some(*layer_id),
                },
                style: props.clone(),
            });
        }
    }
    if let Some(ids) = &overlay.contour_layers {
        rules.layer_filter.insert(
            WallComponentSlot::Contour2D.key().to_string(),
            LayerSelection::Explicit(
                ids.iter()
                    .map(|id| LayerRef {
                        material_id: String::new(),
                        role_tag: None,
                        index: 0,
                        layer_id: Some(*id),
                    })
                    .collect(),
            ),
        );
    }
    if let Some(ch) = &overlay.contour_hatch {
        merge_contour_hatch_into_rules(rules, ch);
    }
}

pub(super) fn merge_contour_hatch_into_rules(rules: &mut ComponentRuleSet, hatch: &ComponentStyleOverride) {
    let slot = WallComponentSlot::ContourHatch2D.key().to_string();
    rules
        .style_override
        .entry(slot)
        .or_default()
        .overlay_from(hatch);
}

pub(super) fn merge_override_fields(
    overlay: &ComponentStyleOverride,
    base: &ComponentStyleOverride,
) -> ComponentStyleOverride {
    ComponentStyleOverride {
        line_type: overlay.line_type.clone().or_else(|| base.line_type.clone()),
        line_color: overlay.line_color.or(base.line_color),
        hatch_pattern: overlay
            .hatch_pattern
            .clone()
            .or_else(|| base.hatch_pattern.clone()),
        hatch_color: overlay.hatch_color.or(base.hatch_color),
        hatch_scale: overlay.hatch_scale.or(base.hatch_scale),
        fill_color: overlay.fill_color.or(base.fill_color),
        hatch_angle: overlay.hatch_angle.or(base.hatch_angle),
        hatch_angle_relative: overlay.hatch_angle_relative.or(base.hatch_angle_relative),
        cad_layer: overlay.cad_layer.clone().or_else(|| base.cad_layer.clone()),
    }
}

/// Checks whether copying `wall_style` into `target` would collide.
pub fn wall_style_copy_conflict(target: &StyleLibrary, wall_style: &WallStyle) -> CopyConflict {
    if let Some(existing) = target
        .wall_styles
        .iter()
        .find(|s| s.style.id == wall_style.style.id)
    {
        if existing == wall_style {
            CopyConflict::IdenticalAlreadyPresent
        } else {
            CopyConflict::DifferentContentCollision
        }
    } else {
        CopyConflict::None
    }
}

/// Checks whether copying `opening_style` into `target` would collide.
pub fn opening_style_copy_conflict(
    target: &StyleLibrary,
    opening_style: &OpeningStyle,
) -> CopyConflict {
    if let Some(existing) = target
        .opening_styles
        .iter()
        .find(|s| s.style.id == opening_style.style.id)
    {
        if existing == opening_style {
            CopyConflict::IdenticalAlreadyPresent
        } else {
            CopyConflict::DifferentContentCollision
        }
    } else {
        CopyConflict::None
    }
}

/// Checks whether copying `slab_style` into `target` would collide.
pub fn slab_style_copy_conflict(
    target: &StyleLibrary,
    slab_style: &SlabStyle,
) -> CopyConflict {
    if let Some(existing) = target
        .slab_styles
        .iter()
        .find(|s| s.style.id == slab_style.style.id)
    {
        if existing == slab_style {
            CopyConflict::IdenticalAlreadyPresent
        } else {
            CopyConflict::DifferentContentCollision
        }
    } else {
        CopyConflict::None
    }
}

/// Checks whether copying `floor_finish_style` into `target` would collide.
pub fn floor_finish_style_copy_conflict(
    target: &StyleLibrary,
    floor_finish_style: &FloorFinishStyle,
) -> CopyConflict {
    if let Some(existing) = target
        .floor_finish_styles
        .iter()
        .find(|s| s.style.id == floor_finish_style.style.id)
    {
        if existing == floor_finish_style {
            CopyConflict::IdenticalAlreadyPresent
        } else {
            CopyConflict::DifferentContentCollision
        }
    } else {
        CopyConflict::None
    }
}

/// Checks whether copying `slab_structural_style` into `target` would collide.
pub fn slab_structural_style_copy_conflict(
    target: &StyleLibrary,
    slab_structural_style: &SlabStructuralStyle,
) -> CopyConflict {
    if let Some(existing) = target
        .slab_structural_styles
        .iter()
        .find(|s| s.style.id == slab_structural_style.style.id)
    {
        if existing == slab_structural_style {
            CopyConflict::IdenticalAlreadyPresent
        } else {
            CopyConflict::DifferentContentCollision
        }
    } else {
        CopyConflict::None
    }
}

