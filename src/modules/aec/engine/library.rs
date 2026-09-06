//! AEC Style Library for persisting materials and wall styles.

use crate::modules::aec::engine::display_component::{
    ComponentRuleSet, ComponentStyleOverride, LayerSelection, LayerStyleOverride,
    RepresentationMode, StyleDisplayOverlay, WallComponentKind, WallComponentSlot,
};
use crate::modules::aec::engine::join::LayerRef;
use crate::modules::aec::engine::material::Material;
use crate::modules::aec::engine::plan_view::{DisplayConfig, ScaleDisplayConfigMapping};
use crate::modules::aec::engine::style::Style;
use crate::modules::aec::engine::wall_style::{LayerValue, Layer, LayerFunction, WallStyle};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::path::PathBuf;

/// A collection of AEC materials and wall styles.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
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

/// Conflict status when copying an entry from one library to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CopyConflict {
    /// No entry with the same ID exists in the target library.
    None,
    /// An entry with the same ID already exists and its content is identical.
    IdenticalAlreadyPresent,
    /// An entry with the same ID already exists but its content differs.
    DifferentContentCollision,
}

/// Where a style-library entry originates in the combined Standard+Project view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibrarySource {
    /// Machine-wide global / standard library (`aec_styles.toml`).
    Standard,
    /// Embedded library of the active `.ocsproj`.
    Project,
}

/// A material shown in the combined Standard+Project list, with provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct CombinedMaterialEntry {
    pub material: Material,
    pub source: LibrarySource,
}

/// A wall style shown in the combined Standard+Project list, with provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct CombinedWallStyleEntry {
    pub wall_style: WallStyle,
    pub source: LibrarySource,
}

/// Builds a combined material list from the Standard library and (when present)
/// the project's embedded library. Project entries win on id collision.
pub fn combined_material_entries(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> Vec<CombinedMaterialEntry> {
    let standard = load_or_seed();
    let mut entries = Vec::new();
    let mut seen = std::collections::HashSet::new();

    if let Some(project) = project {
        for material in &project.material_wall_style_library.materials {
            seen.insert(material.id.clone());
            entries.push(CombinedMaterialEntry {
                material: material.clone(),
                source: LibrarySource::Project,
            });
        }
    }

    for material in standard.materials {
        if seen.insert(material.id.clone()) {
            entries.push(CombinedMaterialEntry {
                material,
                source: LibrarySource::Standard,
            });
        }
    }

    entries
}

/// Builds a combined wall-style list from the Standard library and (when
/// present) the project's embedded library. Project entries win on id collision.
pub fn combined_wall_style_entries(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> Vec<CombinedWallStyleEntry> {
    let standard = load_or_seed();
    let mut entries = Vec::new();
    let mut seen = std::collections::HashSet::new();

    if let Some(project) = project {
        for wall_style in &project.material_wall_style_library.wall_styles {
            seen.insert(wall_style.style.id.clone());
            entries.push(CombinedWallStyleEntry {
                wall_style: wall_style.clone(),
                source: LibrarySource::Project,
            });
        }
    }

    for wall_style in standard.wall_styles {
        if seen.insert(wall_style.style.id.clone()) {
            entries.push(CombinedWallStyleEntry {
                wall_style,
                source: LibrarySource::Standard,
            });
        }
    }

    entries
}

/// Merged Standard+Project style library for UI lookups (project wins on id).
pub fn combined_style_library(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> StyleLibrary {
    let mut lib = StyleLibrary::empty();
    for entry in combined_material_entries(project) {
        lib.upsert_material(entry.material);
    }
    for entry in combined_wall_style_entries(project) {
        lib.upsert_wall_style(entry.wall_style);
    }
    lib
}

/// Source of a material id in the combined view, if present.
pub fn material_library_source(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    id: &str,
) -> Option<LibrarySource> {
    combined_material_entries(project)
        .into_iter()
        .find(|e| e.material.id == id)
        .map(|e| e.source)
}

/// Source of a wall-style id in the combined view, if present.
pub fn wall_style_library_source(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    id: &str,
) -> Option<LibrarySource> {
    combined_wall_style_entries(project)
        .into_iter()
        .find(|e| e.wall_style.style.id == id)
        .map(|e| e.source)
}

/// Checks whether copying `material` into `target` would collide.
pub fn material_copy_conflict(target: &StyleLibrary, material: &Material) -> CopyConflict {
    if let Some(existing) = target.materials.iter().find(|m| m.id == material.id) {
        if existing == material {
            CopyConflict::IdenticalAlreadyPresent
        } else {
            CopyConflict::DifferentContentCollision
        }
    } else {
        CopyConflict::None
    }
}

/// Resolves the [`ComponentRuleSet`] `style` wants applied for the
/// `DisplayConfig` named `display_config_name` (Step 2's style-centered
/// override model, Key Decision 1/3): looks up
/// `style.display_profiles.get(display_config_name)`. Returns `None` when
/// `style` has no profile for that Planart, in which case callers should
/// fall back to the default rule set (every slot visible, standard style,
/// `All` layers for `Contour2D`/`Solid3D` — see `ComponentRuleSet::default()`
/// and `ComponentRuleSet::layer_filter_for`), exactly as an absent
/// `DisplayConfig::component_rules` entry used to behave before this step.
pub fn resolve_effective_rule_set<'a>(
    style: &'a WallStyle,
    display_config_name: &str,
) -> Option<&'a ComponentRuleSet> {
    style.display_profiles.get(display_config_name)
}

/// Session override wins over the Planart default.
pub fn effective_representation(
    config: &DisplayConfig,
    session: Option<RepresentationMode>,
) -> RepresentationMode {
    session.unwrap_or(config.default_representation)
}

/// Planart global visibility + representation + sparse style overlay,
/// starting from a wall style's leftover `display_profiles` (legacy) if any.
pub fn build_effective_rule_set(
    config: &DisplayConfig,
    style: Option<&WallStyle>,
    session: Option<RepresentationMode>,
) -> ComponentRuleSet {
    let mut rules = style
        .and_then(|ws| resolve_effective_rule_set(ws, &config.name))
        .cloned()
        .unwrap_or_default();
    let mode = effective_representation(config, session);
    for kind in WallComponentKind::all() {
        let key = kind.to_slot().key().to_string();
        if session.is_some() {
            // Status-bar 2D/3D/Alle replaces Planart (and leftover
            // display_profiles) visibility for that representation.
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
    if let Some(style) = style {
        if let Some(overlay) = config.style_overlays.get(&style.style.id) {
            merge_style_overlay_into_rules(&mut rules, overlay);
        }
    }
    rules
}

/// Global Planart visibility ∩ representation mode.
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
        .style_overlays
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

/// Copy-on-Write into the standard library must not take Planart overlays.
pub fn wall_style_without_display_profiles(mut style: WallStyle) -> WallStyle {
    style.display_profiles.clear();
    style
}

/// Fold a Planart style overlay into a `ComponentRuleSet` used by regen.
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

fn merge_contour_hatch_into_rules(rules: &mut ComponentRuleSet, hatch: &ComponentStyleOverride) {
    let slot = WallComponentSlot::ContourHatch2D.key().to_string();
    rules
        .style_override
        .entry(slot)
        .or_default()
        .overlay_from(hatch);
}

fn merge_override_fields(
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

fn overlay_from_legacy_rules(rules: &ComponentRuleSet) -> StyleDisplayOverlay {
    let mut overlay = StyleDisplayOverlay::default();
    for lso in &rules.layer_style_override {
        if let Some(id) = lso.layer.layer_id {
            overlay.layer_props.insert(id, lso.style.clone());
        }
    }
    if let Some(LayerSelection::Explicit(refs)) = rules.layer_filter.get(WallComponentSlot::Contour2D.key())
    {
        let ids: Vec<Uuid> = refs.iter().filter_map(|r| r.layer_id).collect();
        if !ids.is_empty() {
            overlay.contour_layers = Some(ids);
        }
    }
    overlay
}

fn visibility_from_legacy_rules(rules: &ComponentRuleSet) -> std::collections::HashMap<WallComponentKind, bool> {
    let mut map = std::collections::HashMap::new();
    for slot in [
        WallComponentSlot::AxisLine,
        WallComponentSlot::Layers2D,
        WallComponentSlot::LayerHatch2D,
        WallComponentSlot::Contour2D,
        WallComponentSlot::ContourHatch2D,
        WallComponentSlot::Solid3D,
        WallComponentSlot::SurfaceStyle3D,
    ] {
        if let Some(kind) = WallComponentKind::from_slot(slot) {
            if let Some(&vis) = rules.visibility.get(slot.key()) {
                map.insert(kind, vis);
            }
        }
    }
    map
}

/// Move name-keyed `WallStyle.display_profiles` onto Planart UUID overlays.
pub fn migrate_display_profiles_into_planarts(
    configs: &mut [DisplayConfig],
    styles: &mut [WallStyle],
) {
    for style in styles.iter_mut() {
        if style.display_profiles.is_empty() {
            continue;
        }
        let profiles = std::mem::take(&mut style.display_profiles);
        for (name, rules) in profiles {
            let Some(cfg) = configs.iter_mut().find(|c| c.name == name) else {
                continue;
            };
            if cfg.component_visibility.is_empty() {
                cfg.component_visibility = visibility_from_legacy_rules(&rules);
            }
            let overlay = overlay_from_legacy_rules(&rules);
            if overlay.layer_props.is_empty()
                && overlay.contour_layers.is_none()
                && overlay.layer_visibility.is_empty()
            {
                continue;
            }
            cfg.style_overlays
                .insert(style.style.id.clone(), overlay);
        }
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

    /// Returns every wall style / layer index that references `material_id`.
    pub fn materials_using(&self, material_id: &str) -> Vec<(&WallStyle, usize)> {
        let mut result = Vec::new();
        for ws in &self.wall_styles {
            for (idx, layer) in ws.layers.iter().enumerate() {
                if layer.material_id == material_id {
                    result.push((ws, idx));
                }
            }
        }
        result
    }
}

/// Builds a small, ready-to-use default library so `AEC_WALL`'s style
/// selection has something to offer out of the box, without requiring the
/// user to define materials/styles first via `AEC_MATERIAL`/`AEC_STYLE`.
pub fn seed_default_library() -> StyleLibrary {
    let masonry = Material {
        category: Some("Mauerwerk".to_string()),
        ..Material::new(
            "mat_masonry".to_string(),
            "Mauerwerk".to_string(),
            "ANSI31".to_string(),
            0x8B7355,
            "Continuous".to_string(),
        )
    };
    let concrete = Material {
        category: Some("Beton".to_string()),
        ..Material::new(
            "mat_concrete".to_string(),
            "Stahlbeton".to_string(),
            "AR-CONC".to_string(),
            0x888888,
            "Continuous".to_string(),
        )
    };
    let insulation = Material {
        category: Some("Daemmung".to_string()),
        hatch_scale: 0.75,
        ..Material::new(
            "mat_insulation".to_string(),
            "Daemmung".to_string(),
            "ANSI37".to_string(),
            0xFFDD88,
            "Continuous".to_string(),
        )
    };
    let plaster = Material {
        category: Some("Putz".to_string()),
        ..Material::new(
            "mat_plaster".to_string(),
            "Putz".to_string(),
            "SOLID".to_string(),
            0xFFFFFF,
            "Continuous".to_string(),
        )
    };
    let wood = Material {
        category: Some("Holz".to_string()),
        hatch_color: Some(acadrust::types::Color::Rgb { r: 166, g: 124, b: 82 }),
        hatch_scale: 0.5,
        ..Material::new(
            "mat_wood".to_string(),
            "Holz".to_string(),
            "ANSI31".to_string(),
            0xC4A35A,
            "Continuous".to_string(),
        )
    };
    let drywall = Material {
        category: Some("Trockenbau".to_string()),
        hatch_color: Some(acadrust::types::Color::Rgb { r: 232, g: 232, b: 224 }),
        hatch_scale: 1.0,
        ..Material::new(
            "mat_drywall".to_string(),
            "Gipskarton".to_string(),
            "ANSI31".to_string(),
            0xF5F5F0,
            "Continuous".to_string(),
        )
    };
    let steel = Material {
        category: Some("Metall".to_string()),
        hatch_color: Some(acadrust::types::Color::Rgb { r: 74, g: 85, b: 104 }),
        hatch_scale: 1.5,
        ..Material::new(
            "mat_steel".to_string(),
            "Stahl".to_string(),
            "ANSI32".to_string(),
            0x708090,
            "Continuous".to_string(),
        )
    };
    let glass = Material {
        category: Some("Verglasung".to_string()),
        hatch_color: Some(acadrust::types::Color::Rgb { r: 168, g: 212, b: 232 }),
        hatch_scale: 2.0,
        ..Material::new(
            "mat_glass".to_string(),
            "Glas".to_string(),
            "ANSI33".to_string(),
            0xC8E6F0,
            "Continuous".to_string(),
        )
    };

    let masonry_wall = WallStyle {
        style: Style {
            id: "style_masonry".to_string(),
            name: "Wand Mauerwerk 24cm".to_string(),
            object_kind: "Wall".to_string(),
            parent_style_id: None,
        },
        layers: vec![Layer {
            material_id: masonry.id.clone(),
            thickness: LayerValue::Fixed(0.24),
            function: LayerFunction::Structural,
            axis_offset: LayerValue::Fixed(-0.12),
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            role_tag: Some("Tragschale".to_string()),
            layer_id: Uuid::new_v4(),
        }],
    display_profiles: std::collections::HashMap::new(),
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
            thickness: LayerValue::Fixed(0.20),
            function: LayerFunction::Structural,
            axis_offset: LayerValue::Fixed(-0.10),
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: Some("AR-CONC".to_string()),
            role_tag: Some("Tragschale".to_string()),
            layer_id: Uuid::new_v4(),
        }],
    display_profiles: std::collections::HashMap::new(),
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
                thickness: LayerValue::Fixed(0.015),
                function: LayerFunction::Finish,
                axis_offset: LayerValue::Fixed(-0.1725),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: Some("Innenputz".to_string()),
                layer_id: Uuid::new_v4(),
            },
            Layer {
                material_id: masonry.id.clone(),
                thickness: LayerValue::Fixed(0.175),
                function: LayerFunction::Structural,
                axis_offset: LayerValue::Fixed(-0.1575),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: Some("Tragschale".to_string()),
                layer_id: Uuid::new_v4(),
            },
            Layer {
                material_id: insulation.id.clone(),
                thickness: LayerValue::Fixed(0.14),
                function: LayerFunction::Insulation,
                axis_offset: LayerValue::Fixed(0.0175),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: Some("ANSI37".to_string()),
                role_tag: Some("Daemmschicht".to_string()),
                layer_id: Uuid::new_v4(),
            },
            Layer {
                material_id: plaster.id.clone(),
                thickness: LayerValue::Fixed(0.015),
                function: LayerFunction::Finish,
                axis_offset: LayerValue::Fixed(0.1575),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: Some("A-WALL-FINISH".to_string()),
                hatch_override: None,
                role_tag: Some("Aussenputz".to_string()),
                layer_id: Uuid::new_v4(),
            },
        ],
    display_profiles: std::collections::HashMap::new(),
    };

    // Derived style demonstrating style-manager inheritance: shares the
    // insulated exterior wall's layer buildup (empty `layers`, resolved via
    // `effective_layers`) but with a distinct name/id for a different
    // context (e.g. a garden-facing facade using the same buildup).
    let insulated_wall_variant = WallStyle {
        style: Style {
            id: "style_insulated_ext_garden".to_string(),
            name: "Aussenwand gedaemmt (Gartenseite)".to_string(),
            object_kind: "Wall".to_string(),
            parent_style_id: Some(insulated_wall.style.id.clone()),
        },
        layers: vec![],
    display_profiles: std::collections::HashMap::new(),
    };

    StyleLibrary {
        materials: vec![
            masonry, concrete, insulation, plaster, wood, drywall, steel, glass,
        ],
        wall_styles: vec![
            masonry_wall,
            concrete_wall,
            insulated_wall,
            insulated_wall_variant,
        ],
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
                    // Legacy files written before `Layer::layer_id` existed
                    // get a freshly-migrated set of stable IDs assigned
                    // in-memory (see `layers_from_serde`). Persist them back
                    // immediately so subsequent loads return the *same*
                    // IDs instead of re-randomizing them on every read.
                    let _ = save_to_default_path(&lib);
                    return lib;
                }
            }
        }
    }
    let seed = seed_default_library();
    let _ = save_to_default_path(&seed);
    seed
}

/// A collection of [`DisplayConfig`]s ("Plans"), persisted analogously to
/// [`StyleLibrary`] — see Step 5 of
/// `.junie/plans/aec-plan-view-display-variants.md`. Deliberately a
/// separate library/file (not folded into `StyleLibrary`) since
/// `DisplayConfig`s are a distinct kind of library entry with their own
/// lifecycle, and keeping them apart avoids growing every wall-style/
/// material save into a `DisplayConfig` roundtrip too.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DisplayConfigLibrary {
    /// List of available display configurations ("Plans").
    #[serde(default)]
    pub configs: Vec<DisplayConfig>,
    /// Legacy scale→DisplayConfig mapping table. Kept for deserialization of
    /// older project files; unused by the UI/runtime (feature removed).
    #[serde(default)]
    pub scale_display_config_mappings: Vec<ScaleDisplayConfigMapping>,
}

impl DisplayConfigLibrary {
    /// An empty library (no display configs, no scale mappings).
    pub fn empty() -> Self {
        Self {
            configs: Vec::new(),
            scale_display_config_mappings: Vec::new(),
        }
    }

    /// Inserts or replaces (by `name`) a display config.
    pub fn upsert(&mut self, config: DisplayConfig) {
        if let Some(existing) = self.configs.iter_mut().find(|c| c.name == config.name) {
            *existing = config;
        } else {
            self.configs.push(config);
        }
    }

    /// Removes a display config by `name`. Returns `true` if one was removed.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.configs.len();
        self.configs.retain(|c| c.name != name);
        self.configs.len() != before
    }

    /// Looks up a display config by `name`.
    pub fn find(&self, name: &str) -> Option<&DisplayConfig> {
        self.configs.iter().find(|c| c.name == name)
    }
}

/// Serializes the display-config library to a string (JSON, see [`to_toml`]
/// for the rationale of the misleadingly-named-but-consistent function).
pub fn display_config_library_to_toml(lib: &DisplayConfigLibrary) -> Result<String, String> {
    serde_json::to_string_pretty(lib).map_err(|e| e.to_string())
}

/// Deserializes the display-config library from a string.
pub fn display_config_library_from_toml(s: &str) -> Result<DisplayConfigLibrary, String> {
    serde_json::from_str(s).map_err(|e| e.to_string())
}

/// Returns the default path for the AEC display-config ("Plan") library
/// file. Reuses the application's standard configuration directory.
pub fn default_display_config_library_path() -> PathBuf {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(p) = crate::config::config_dir() {
            return p.join("aec_display_configs.toml");
        }
    }

    let mut p = PathBuf::new();
    #[cfg(all(not(target_arch = "wasm32"), target_os = "linux"))]
    {
        if let Ok(home) = std::env::var("HOME") {
            p.push(home);
            p.push(".config");
        }
    }
    p.push("OpenCADStudio");
    p.push("aec_display_configs.toml");
    p
}

/// Persists `lib` to [`default_display_config_library_path`], creating
/// parent directories as needed. On `wasm32` this is a no-op.
pub fn save_display_config_library_to_default_path(lib: &DisplayConfigLibrary) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = default_display_config_library_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = display_config_library_to_toml(lib)?;
        return std::fs::write(&path, content).map_err(|e| e.to_string());
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = lib;
        Ok(())
    }
}

/// Loads the display-config library from
/// [`default_display_config_library_path`], or — if it does not exist yet —
/// returns an empty library (no seed data: unlike materials/wall styles,
/// `DisplayConfig`s have no sensible non-empty default without a project's
/// own wall styles to reference). On `wasm32` this always returns an empty
/// in-memory library.
pub fn load_or_seed_display_config_library() -> DisplayConfigLibrary {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = default_display_config_library_path();
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(lib) = display_config_library_from_toml(&content) {
                    return lib;
                }
            }
        }
    }
    DisplayConfigLibrary::empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::material::Material;
    use crate::modules::aec::engine::style::Style;
    use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, LayerValue, WallStyle};

    #[test]
    fn resolve_effective_rule_set_returns_none_without_a_matching_profile() {
        let style = WallStyle {
            style: Style {
                id: "style1".to_string(),
                name: "Style 1".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: vec![],
            display_profiles: std::collections::HashMap::new(),
        };
        assert!(resolve_effective_rule_set(&style, "Architekt 1:50").is_none());
    }

    #[test]
    fn resolve_effective_rule_set_returns_the_profile_for_the_matching_display_config_name() {
        use crate::modules::aec::engine::display_component::ComponentRuleSet;

        let mut rules_50 = ComponentRuleSet::default();
        rules_50.visibility.insert("AxisLine".to_string(), false);
        let rules_200 = ComponentRuleSet::default();

        let mut display_profiles = std::collections::HashMap::new();
        display_profiles.insert("Ausf\u{fc}hrungsplan 1:50".to_string(), rules_50.clone());
        display_profiles.insert("\u{dc}bersichtsplan 1:200".to_string(), rules_200.clone());

        let style = WallStyle {
            style: Style {
                id: "style1".to_string(),
                name: "Style 1".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: vec![],
            display_profiles,
        };

        assert_eq!(
            resolve_effective_rule_set(&style, "Ausf\u{fc}hrungsplan 1:50"),
            Some(&rules_50)
        );
        assert_eq!(
            resolve_effective_rule_set(&style, "\u{dc}bersichtsplan 1:200"),
            Some(&rules_200)
        );
        // A Planart this style has no profile for still falls back to `None`
        // (default rule set), instead of erroring or picking an arbitrary
        // profile.
        assert!(resolve_effective_rule_set(&style, "Schalplan 1:50").is_none());
    }

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
                    thickness: LayerValue::Fixed(10.0),
                    function: LayerFunction::Structural,
                    axis_offset: LayerValue::Fixed(-7.5),
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    layer_override: None,
                    hatch_override: None,
                    role_tag: Some("Tragschale".to_string()),
                    layer_id: Uuid::new_v4(),
                },
                Layer {
                    material_id: "mat1".to_string(),
                    thickness: LayerValue::Fixed(5.0),
                    function: LayerFunction::Finish,
                    axis_offset: LayerValue::Fixed(2.5),
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    layer_override: None,
                    hatch_override: Some("ANSI31".to_string()),
                    role_tag: None,
                    layer_id: Uuid::new_v4(),
                },
            ],
        display_profiles: std::collections::HashMap::new(),
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
    fn legacy_gap_before_library_migrates_to_axis_offset() {
        let json = r#"{
            "materials": [],
            "wall_styles": [{
                "id": "legacy",
                "name": "Legacy",
                "object_kind": "Wall",
                "parent_style_id": null,
                "layers": [
                    {
                        "material_id": "a",
                        "thickness": 0.1,
                        "function": "Structural",
                        "gap_before": 0.0
                    },
                    {
                        "material_id": "b",
                        "thickness": 0.1,
                        "function": "Finish",
                        "gap_before": 0.05
                    }
                ]
            }]
        }"#;
        let lib = from_toml(json).expect("legacy library must load");
        let style = &lib.wall_styles[0];
        assert!((style.layers[0].axis_offset.as_fixed_or(0.0) - (-0.125)).abs() < 1e-12);
        assert!((style.layers[1].axis_offset.as_fixed_or(0.0) - 0.025).abs() < 1e-12);
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
        display_profiles: std::collections::HashMap::new(),
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
        display_profiles: std::collections::HashMap::new(),
        };
        let s2 = WallStyle {
            style: Style {
                id: "s2".to_string(),
                name: "Style B".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: Some("s1".to_string()),
            },
            layers: vec![],
        display_profiles: std::collections::HashMap::new(),
        };
        let s3 = WallStyle {
            style: Style {
                id: "s3".to_string(),
                name: "Style C".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: Some("s1".to_string()),
            },
            layers: vec![],
        display_profiles: std::collections::HashMap::new(),
        };
        let s4 = WallStyle {
            style: Style {
                id: "s4".to_string(),
                name: "Style D".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: Some("orphan".to_string()),
            },
            layers: vec![],
        display_profiles: std::collections::HashMap::new(),
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

    #[test]
    fn seed_default_library_demonstrates_new_layer_attributes() {
        let lib = seed_default_library();

        // At least one layer per demo style uses `role_tag`, and at least
        // one layer overrides its hatch pattern / drawing layer.
        let any_role_tag = lib
            .wall_styles
            .iter()
            .flat_map(|ws| ws.layers.iter())
            .any(|l| l.role_tag.is_some());
        let any_hatch_override = lib
            .wall_styles
            .iter()
            .flat_map(|ws| ws.layers.iter())
            .any(|l| l.hatch_override.is_some());
        let any_layer_override = lib
            .wall_styles
            .iter()
            .flat_map(|ws| ws.layers.iter())
            .any(|l| l.layer_override.is_some());
        assert!(any_role_tag, "expected at least one seeded layer with a role_tag");
        assert!(
            any_hatch_override,
            "expected at least one seeded layer with a hatch_override"
        );
        assert!(
            any_layer_override,
            "expected at least one seeded layer with a layer_override"
        );
    }

    #[test]
    fn seed_default_library_includes_an_inherited_wall_style() {
        let lib = seed_default_library();

        let inherited = lib
            .wall_styles
            .iter()
            .find(|ws| ws.style.parent_style_id.is_some())
            .expect("expected at least one demo style with a parent");
        assert!(
            inherited.layers.is_empty(),
            "the inherited demo style should rely on effective_layers, not duplicate the parent's buildup"
        );

        let styles: std::collections::HashMap<_, _> = lib
            .wall_styles
            .iter()
            .map(|ws| (ws.style.id.clone(), ws.clone()))
            .collect();
        let effective = crate::modules::aec::engine::wall_style::effective_layers(
            &styles,
            &inherited.style.id,
        )
        .unwrap();
        assert!(!effective.is_empty(), "inherited style must resolve to a non-empty buildup");
    }

    #[test]
    fn library_without_new_layer_fields_loads_with_defaults() {
        // Simulates a library file saved before `hatch_override`/`role_tag`
        // existed: they must default to `None` rather than failing to load.
        let json = r#"{
            "materials": [],
            "wall_styles": [
                {
                    "id": "style1",
                    "name": "Style 1",
                    "object_kind": "Wall",
                    "parent_style_id": null,
                    "layers": [
                        {
                            "material_id": "mat1",
                            "thickness": 0.2,
                            "function": "Structural"
                        }
                    ]
                }
            ]
        }"#;
        let lib = from_toml(json).expect("old-format library must still deserialize");
        assert_eq!(lib.wall_styles.len(), 1);
        let layer = &lib.wall_styles[0].layers[0];
        assert_eq!(layer.hatch_override, None);
        assert_eq!(layer.role_tag, None);
    }

    #[test]
    fn seed_default_library_includes_new_example_materials() {
        let lib = seed_default_library();

        let wood = lib
            .materials
            .iter()
            .find(|m| m.id == "mat_wood")
            .expect("Holz material");
        assert_eq!(wood.name, "Holz");
        assert_eq!(wood.category.as_deref(), Some("Holz"));
        assert_eq!(wood.hatch_color, Some(acadrust::types::Color::Rgb { r: 0xA6, g: 0x7C, b: 0x52 }));
        assert!((wood.hatch_scale - 0.5).abs() < 1e-12);

        let drywall = lib
            .materials
            .iter()
            .find(|m| m.id == "mat_drywall")
            .expect("Gipskarton material");
        assert_eq!(drywall.category.as_deref(), Some("Trockenbau"));

        let steel = lib
            .materials
            .iter()
            .find(|m| m.id == "mat_steel")
            .expect("Stahl material");
        assert_eq!(steel.category.as_deref(), Some("Metall"));
        assert!((steel.hatch_scale - 1.5).abs() < 1e-12);

        let glass = lib
            .materials
            .iter()
            .find(|m| m.id == "mat_glass")
            .expect("Glas material");
        assert_eq!(glass.category.as_deref(), Some("Verglasung"));
        assert_eq!(glass.hatch_color, Some(acadrust::types::Color::Rgb { r: 0xA8, g: 0xD4, b: 0xE8 }));
    }

    #[test]
    fn materials_using_reports_wall_styles_and_layer_indices() {
        let lib = seed_default_library();
        let masonry_uses = lib.materials_using("mat_masonry");
        assert!(
            !masonry_uses.is_empty(),
            "mat_masonry should be referenced by at least one wall style layer"
        );
        assert!(masonry_uses.iter().any(|(ws, _)| ws.style.id == "style_masonry"));

        let unused = lib.materials_using("mat_wood");
        assert!(
            unused.is_empty(),
            "mat_wood is seeded but not used by any default wall style"
        );
    }

    #[test]
    fn display_config_library_roundtrip() {
        use crate::modules::aec::engine::plan_view::{PlanningStage, ViewType};

        let mut lib = DisplayConfigLibrary::empty();
        let mut cfg = DisplayConfig::new(
            "Architekt 1:50".to_string(),
            "Architektur".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        cfg.scale = Some(50.0);
        lib.upsert(cfg);

        let serialized = display_config_library_to_toml(&lib).expect("serialization failed");
        let deserialized =
            display_config_library_from_toml(&serialized).expect("deserialization failed");
        assert_eq!(lib, deserialized);
    }

    #[test]
    fn display_config_library_upsert_replaces_by_name() {
        use crate::modules::aec::engine::plan_view::{PlanningStage, ViewType};

        let mut lib = DisplayConfigLibrary::empty();
        lib.upsert(DisplayConfig::new(
            "Statik 1:50".to_string(),
            "Statik".to_string(),
            PlanningStage::Design,
            ViewType::Section,
        ));
        assert_eq!(lib.configs.len(), 1);

        let mut updated = DisplayConfig::new(
            "Statik 1:50".to_string(),
            "Statik (überarbeitet)".to_string(),
            PlanningStage::Design,
            ViewType::Section,
        );
        updated.scale = Some(100.0);
        lib.upsert(updated);

        assert_eq!(lib.configs.len(), 1, "same name must replace, not duplicate");
        let found = lib.find("Statik 1:50").expect("config must still be found by name");
        assert_eq!(found.discipline, "Statik (überarbeitet)");
        assert_eq!(found.scale, Some(100.0));
    }

    #[test]
    fn display_config_library_remove_by_name() {
        use crate::modules::aec::engine::plan_view::{PlanningStage, ViewType};

        let mut lib = DisplayConfigLibrary::empty();
        lib.upsert(DisplayConfig::new(
            "Präsentation 1:200".to_string(),
            "Präsentation".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        ));

        assert!(lib.remove("Präsentation 1:200"));
        assert!(lib.configs.is_empty());
        assert!(!lib.remove("Präsentation 1:200"));
    }

    #[test]
    fn display_config_library_without_configs_field_deserializes_empty() {
        let json = r#"{}"#;
        let lib = display_config_library_from_toml(json)
            .expect("missing configs field must default to empty");
        assert!(lib.configs.is_empty());
        assert!(lib.scale_display_config_mappings.is_empty());
    }

    #[test]
    fn display_config_library_scale_mappings_roundtrip() {
        use crate::modules::aec::engine::plan_view::{
            PlanningStage, ScaleDisplayConfigMapping, ViewType,
        };

        let mut lib = DisplayConfigLibrary::empty();
        lib.upsert(DisplayConfig::new(
            "Architekt 1:50".to_string(),
            "Architektur".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        ));
        lib.scale_display_config_mappings.push(ScaleDisplayConfigMapping {
            scale_name: "1:50".to_string(),
            display_config_name: "Architekt 1:50".to_string(),
        });

        let serialized = display_config_library_to_toml(&lib).expect("serialization failed");
        let deserialized =
            display_config_library_from_toml(&serialized).expect("deserialization failed");
        assert_eq!(lib, deserialized);
    }

    #[test]
    fn display_config_library_without_scale_mappings_field_deserializes_empty() {
        // Simulates a library file saved before Step 7 introduced
        // `scale_display_config_mappings`: must default to empty, not fail.
        let json = r#"{
            "configs": [
                {
                    "name": "Architekt 1:50",
                    "discipline": "Architektur",
                    "planning_stage": "Design",
                    "view_type": "FloorPlan"
                }
            ]
        }"#;
        let lib = display_config_library_from_toml(json)
            .expect("old-format library without scale mappings must still deserialize");
        assert_eq!(lib.configs.len(), 1);
        assert!(lib.scale_display_config_mappings.is_empty());
    }

    #[test]
    fn combined_material_entries_project_overrides_standard_on_id() {
        use crate::modules::aec::engine::project::ProjectFile;

        let standard_id = "shared_mat".to_string();
        let mut project = ProjectFile::default();
        project.material_wall_style_library.upsert_material(Material::new(
            standard_id.clone(),
            "Project Material".to_string(),
            "SOLID".to_string(),
            0x00FF00,
            "Continuous".to_string(),
        ));
        project.material_wall_style_library.upsert_material(Material::new(
            "proj_only".to_string(),
            "Project Only".to_string(),
            "SOLID".to_string(),
            0x0000FF,
            "Continuous".to_string(),
        ));

        let entries = combined_material_entries(Some(&project));
        let shared = entries
            .iter()
            .find(|e| e.material.id == standard_id)
            .expect("shared id present");
        assert_eq!(shared.source, LibrarySource::Project);
        assert_eq!(shared.material.name, "Project Material");
        assert!(entries
            .iter()
            .any(|e| e.material.id == "proj_only" && e.source == LibrarySource::Project));
        // Standard-only entries (from load_or_seed) still appear with Standard source.
        assert!(entries.iter().any(|e| e.source == LibrarySource::Standard));
    }

    #[test]
    fn test_material_copy_conflict() {
        let mat_a = Material::new(
            "mat1".to_string(),
            "Material 1".to_string(),
            "HATCH1".to_string(),
            0xFF0000,
            "Continuous".to_string(),
        );
        let mut mat_b = mat_a.clone();
        mat_b.name = "Material 1 Updated".to_string();

        let mut target = StyleLibrary::empty();
        assert_eq!(material_copy_conflict(&target, &mat_a), CopyConflict::None);

        target.upsert_material(mat_a.clone());
        assert_eq!(
            material_copy_conflict(&target, &mat_a),
            CopyConflict::IdenticalAlreadyPresent
        );
        assert_eq!(
            material_copy_conflict(&target, &mat_b),
            CopyConflict::DifferentContentCollision
        );
    }

    #[test]
    fn test_wall_style_copy_conflict() {
        let style_a = WallStyle {
            style: Style {
                id: "style1".to_string(),
                name: "Style 1".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: vec![],
        display_profiles: std::collections::HashMap::new(),
        };
        let mut style_b = style_a.clone();
        style_b.style.name = "Style 1 Updated".to_string();

        let mut target = StyleLibrary::empty();
        assert_eq!(wall_style_copy_conflict(&target, &style_a), CopyConflict::None);

        target.upsert_wall_style(style_a.clone());
        assert_eq!(
            wall_style_copy_conflict(&target, &style_a),
            CopyConflict::IdenticalAlreadyPresent
        );
        assert_eq!(
            wall_style_copy_conflict(&target, &style_b),
            CopyConflict::DifferentContentCollision
        );
    }

    #[test]
    fn build_effective_rule_set_session_3d_hides_2d_slots() {
        let mut cfg = DisplayConfig::new(
            "Entwurf".into(),
            "Architektur".into(),
            crate::modules::aec::engine::plan_view::PlanningStage::Design,
            crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
        );
        cfg.default_representation = RepresentationMode::All;
        let rules = build_effective_rule_set(&cfg, None, Some(RepresentationMode::ThreeD));
        assert!(!rules.is_visible(WallComponentSlot::Layers2D));
        assert!(!rules.is_visible(WallComponentSlot::Contour2D));
        assert!(rules.is_visible(WallComponentSlot::Solid3D));
    }

    #[test]
    fn viewport_3d_hides_2d_components_including_hatch() {
        let mut cfg = DisplayConfig::new(
            "Entwurf".into(),
            "Architektur".into(),
            crate::modules::aec::engine::plan_view::PlanningStage::Design,
            crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
        );
        cfg.default_representation = RepresentationMode::TwoD;
        let mode = effective_representation(&cfg, Some(RepresentationMode::ThreeD));
        assert!(!component_is_visible(&cfg, WallComponentKind::Layers2D, mode));
        assert!(!component_is_visible(&cfg, WallComponentKind::LayerHatch2D, mode));
        assert!(component_is_visible(&cfg, WallComponentKind::Layers3D, mode));
    }

    #[test]
    fn session_all_shows_3d_even_if_planart_hides_layers3d() {
        let mut cfg = DisplayConfig::new(
            "Entwurf".into(),
            "Architektur".into(),
            crate::modules::aec::engine::plan_view::PlanningStage::Design,
            crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
        );
        cfg.default_representation = RepresentationMode::TwoD;
        cfg.component_visibility
            .insert(WallComponentKind::Layers3D, false);
        cfg.component_visibility
            .insert(WallComponentKind::SurfaceStyle3D, false);
        let rules = build_effective_rule_set(&cfg, None, Some(RepresentationMode::All));
        assert!(rules.is_visible(WallComponentSlot::Solid3D));
        assert!(rules.is_visible(WallComponentSlot::SurfaceStyle3D));
        assert!(rules.is_visible(WallComponentSlot::Layers2D));
        let rules_3d = build_effective_rule_set(&cfg, None, Some(RepresentationMode::ThreeD));
        assert!(rules_3d.is_visible(WallComponentSlot::Solid3D));
        assert!(!rules_3d.is_visible(WallComponentSlot::Layers2D));
    }

    #[test]
    fn session_3d_overrides_hidden_solid_in_display_profiles() {
        use crate::modules::aec::engine::display_component::ComponentRuleSet;
        use crate::modules::aec::engine::wall_style::WallStyle;
        let mut cfg = DisplayConfig::new(
            "Entwurf".into(),
            "Architektur".into(),
            crate::modules::aec::engine::plan_view::PlanningStage::Design,
            crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
        );
        cfg.default_representation = RepresentationMode::TwoD;
        let mut style = WallStyle {
            style: Style {
                id: "s".to_string(),
                name: "S".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: vec![],
            display_profiles: std::collections::HashMap::new(),
        };
        let mut profile = ComponentRuleSet::default();
        profile
            .visibility
            .insert(WallComponentSlot::Solid3D.key().to_string(), false);
        style
            .display_profiles
            .insert("Entwurf".into(), profile);
        let rules = build_effective_rule_set(&cfg, Some(&style), Some(RepresentationMode::ThreeD));
        assert!(rules.is_visible(WallComponentSlot::Solid3D));
        assert!(!rules.is_visible(WallComponentSlot::Layers2D));
        let rules_2d = build_effective_rule_set(&cfg, Some(&style), Some(RepresentationMode::TwoD));
        assert!(!rules_2d.is_visible(WallComponentSlot::Solid3D));
        assert!(rules_2d.is_visible(WallComponentSlot::Layers2D));
    }

    #[test]
    fn planart_can_show_layers2d_without_layer_hatch() {
        let mut cfg = DisplayConfig::new(
            "Statik".into(),
            "Statik".into(),
            crate::modules::aec::engine::plan_view::PlanningStage::Design,
            crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
        );
        cfg.component_visibility
            .insert(WallComponentKind::Layers2D, true);
        cfg.component_visibility
            .insert(WallComponentKind::LayerHatch2D, false);
        let mode = RepresentationMode::All;
        assert!(component_is_visible(&cfg, WallComponentKind::Layers2D, mode));
        assert!(!component_is_visible(
            &cfg,
            WallComponentKind::LayerHatch2D,
            mode
        ));
    }

    #[test]
    fn field_override_color_keeps_material_hatch() {
        let layer_id = Uuid::new_v4();
        let mut cfg = DisplayConfig::new(
            "A".into(),
            "Arch".into(),
            crate::modules::aec::engine::plan_view::PlanningStage::Design,
            crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
        );
        let mut overlay = StyleDisplayOverlay::default();
        overlay.layer_props.insert(
            layer_id,
            ComponentStyleOverride {
                line_color: Some(acadrust::types::Color::Rgb { r: 255, g: 0, b: 0 }),
                ..Default::default()
            },
        );
        cfg.style_overlays.insert("style1".into(), overlay);
        let mat = Material::new(
            "m".into(),
            "M".into(),
            "ANSI31".into(),
            0x111111,
            "Continuous".into(),
        );
        let resolved = resolve_layer_property_override(&cfg, "style1", layer_id, None, Some(&mat));
        assert_eq!(
            resolved.line_color,
            Some(acadrust::types::Color::Rgb { r: 255, g: 0, b: 0 })
        );
        assert_eq!(resolved.hatch_pattern.as_deref(), Some("ANSI31"));
    }

    #[test]
    fn field_override_hatch_scale_keeps_material_pattern() {
        let layer_id = Uuid::new_v4();
        let mut cfg = DisplayConfig::new(
            "A".into(),
            "Arch".into(),
            crate::modules::aec::engine::plan_view::PlanningStage::Design,
            crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
        );
        let mut overlay = StyleDisplayOverlay::default();
        overlay.layer_props.insert(
            layer_id,
            ComponentStyleOverride {
                hatch_scale: Some(2.5),
                ..Default::default()
            },
        );
        cfg.style_overlays.insert("style1".into(), overlay);
        let mut mat = Material::new(
            "m".into(),
            "M".into(),
            "ANSI31".into(),
            0x111111,
            "Continuous".into(),
        );
        mat.hatch_scale = 1.0;
        let resolved = resolve_layer_property_override(&cfg, "style1", layer_id, None, Some(&mat));
        assert_eq!(resolved.hatch_scale, Some(2.5));
        assert_eq!(resolved.hatch_pattern.as_deref(), Some("ANSI31"));
    }

    #[test]
    fn contour_hatch_on_planart_merges_into_contour_hatch2d_slot() {
        let mut cfg = DisplayConfig::new(
            "A".into(),
            "Arch".into(),
            crate::modules::aec::engine::plan_view::PlanningStage::Design,
            crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
        );
        // Global Planart hatch must not apply; only per-style overlay.
        cfg.contour_hatch = Some(ComponentStyleOverride {
            hatch_pattern: Some("SOLID".into()),
            hatch_scale: Some(9.0),
            ..Default::default()
        });
        let mut overlay = StyleDisplayOverlay::default();
        overlay.contour_hatch = Some(ComponentStyleOverride {
            hatch_pattern: Some("ANSI31".into()),
            hatch_scale: Some(2.0),
            ..Default::default()
        });
        cfg.style_overlays.insert("s".into(), overlay);
        let style = WallStyle {
            style: Style {
                id: "s".to_string(),
                name: "S".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: vec![],
            display_profiles: std::collections::HashMap::new(),
        };
        let rules = build_effective_rule_set(&cfg, Some(&style), None);
        let ov = rules
            .style_for(WallComponentSlot::ContourHatch2D)
            .expect("contour hatch slot");
        assert_eq!(ov.hatch_pattern.as_deref(), Some("ANSI31"));
        assert_eq!(ov.hatch_scale, Some(2.0));
        let without = build_effective_rule_set(&cfg, None, None);
        assert!(without.style_for(WallComponentSlot::ContourHatch2D).is_none());
    }

    #[test]
    fn merge_overlay_hatch_scale_into_rules() {
        let layer_id = Uuid::new_v4();
        let mut overlay = StyleDisplayOverlay::default();
        overlay.layer_props.insert(
            layer_id,
            ComponentStyleOverride {
                hatch_scale: Some(3.0),
                ..Default::default()
            },
        );
        let mut rules = ComponentRuleSet::default();
        merge_style_overlay_into_rules(&mut rules, &overlay);
        assert_eq!(
            rules
                .layer_style_override
                .iter()
                .find(|l| l.layer.layer_id == Some(layer_id))
                .and_then(|l| l.style.hatch_scale),
            Some(3.0)
        );
    }

    #[test]
    fn phase_style_only_on_contour2d() {
        let ov = ComponentStyleOverride {
            line_type: Some("Dashed".into()),
            ..Default::default()
        };
        assert!(phase_style_for_slot(Some(&ov), WallComponentSlot::Contour2D).is_some());
        assert!(phase_style_for_slot(Some(&ov), WallComponentSlot::ContourHatch2D).is_none());
        assert!(phase_style_for_slot(Some(&ov), WallComponentSlot::Layers2D).is_none());
    }

    #[test]
    fn copy_to_standard_strips_display_profiles() {
        let mut style = WallStyle {
            style: Style {
                id: "style1".into(),
                name: "S".into(),
                object_kind: "Wall".into(),
                parent_style_id: None,
            },
            layers: vec![],
            display_profiles: std::collections::HashMap::new(),
        };
        style
            .display_profiles
            .insert("Architekt".into(), ComponentRuleSet::default());
        let stripped = wall_style_without_display_profiles(style);
        assert!(stripped.display_profiles.is_empty());
    }

    #[test]
    fn migrate_name_keyed_profiles_onto_planart_overlays() {
        let layer_id = Uuid::new_v4();
        let cfg = DisplayConfig::new(
            "Architekt 1:50".into(),
            "Arch".into(),
            crate::modules::aec::engine::plan_view::PlanningStage::Design,
            crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
        );
        let id_before = cfg.id;
        let mut rules = ComponentRuleSet::default();
        rules.visibility.insert("LayerHatch2D".into(), false);
        rules.layer_style_override.push(
            crate::modules::aec::engine::display_component::LayerStyleOverride {
                layer: crate::modules::aec::engine::join::LayerRef {
                    material_id: "m".into(),
                    role_tag: None,
                    index: 0,
                    layer_id: Some(layer_id),
                },
                style: ComponentStyleOverride {
                    line_color: Some(acadrust::types::Color::Rgb { r: 1, g: 2, b: 3 }),
                    ..Default::default()
                },
            },
        );
        let mut style = WallStyle {
            style: Style {
                id: "style1".into(),
                name: "S".into(),
                object_kind: "Wall".into(),
                parent_style_id: None,
            },
            layers: vec![],
            display_profiles: std::collections::HashMap::new(),
        };
        style
            .display_profiles
            .insert("Architekt 1:50".into(), rules);
        let mut configs = vec![cfg];
        let mut styles = vec![style];
        migrate_display_profiles_into_planarts(&mut configs, &mut styles);
        assert_eq!(configs[0].id, id_before);
        assert!(styles[0].display_profiles.is_empty());
        assert_eq!(
            configs[0]
                .component_visibility
                .get(&WallComponentKind::LayerHatch2D)
                .copied(),
            Some(false)
        );
        assert_eq!(
            configs[0]
                .style_overlays
                .get("style1")
                .and_then(|o| o.layer_props.get(&layer_id))
                .and_then(|p| p.line_color),
            Some(acadrust::types::Color::Rgb { r: 1, g: 2, b: 3 })
        );
        configs[0].name = "Renamed".into();
        assert!(configs[0].style_overlays.contains_key("style1"));
    }

    #[test]
    fn overlay_unknown_layer_id_is_ignored_by_resolver() {
        let mut cfg = DisplayConfig::new(
            "A".into(),
            "A".into(),
            crate::modules::aec::engine::plan_view::PlanningStage::Design,
            crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
        );
        let mut overlay = StyleDisplayOverlay::default();
        overlay.layer_props.insert(
            Uuid::new_v4(),
            ComponentStyleOverride {
                line_type: Some("Hidden".into()),
                ..Default::default()
            },
        );
        cfg.style_overlays.insert("style1".into(), overlay);
        let other = Uuid::new_v4();
        let resolved = resolve_layer_property_override(&cfg, "style1", other, None, None);
        assert!(resolved.line_type.is_none());
    }
}
