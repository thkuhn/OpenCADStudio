//! Storage of wall display profiles edited in the wall style manager.
//!
//! A profile lives either on the style itself (`WallStyle::default_profile`,
//! selected with [`STYLE_DEFAULT_PROFILE_KEY`]) or on a plan kind
//! (`DisplayConfig::style_overrides[style_id].rules`). Neither depends on a
//! plan name at runtime; the plan name is only the editor's way to pick the
//! plan kind.

use crate::app::{Message, OpenCADStudio};
use iced::Task;
use crate::modules::aec::engine::display_component::ComponentRuleSet;
use crate::modules::aec::engine::library::STYLE_DEFAULT_PROFILE_KEY;

impl OpenCADStudio {
    pub(crate) fn aec_wall_profile_slot_style_apply(&mut self) -> Task<Message> {
        use crate::modules::aec::engine::display_component::component_style_override_from_editor_fields;
        let Some(slot) = self.aec.styles.profile_editing_slot else {
            return Task::none();
        };
        let style = component_style_override_from_editor_fields(
            &self.aec.styles.profile_slot_style_line_type,
            &self.aec.styles.profile_slot_style_line_color,
            &self.aec.styles.profile_slot_style_hatch_pattern,
            &self.aec.styles.profile_slot_style_hatch_color,
            &self.aec.styles.profile_slot_style_fill_color,
        );
        if style == Default::default() {
            self.aec.styles.profile_slot_overrides.remove(&slot);
        } else {
            self.aec.styles.profile_slot_overrides.insert(slot, style);
        }
        self.aec.styles.profile_editing_slot = None;
        self.clear_aec_profile_slot_style_editor_buffers();
        Task::none()
    }

    /// Loads the profile `name` (style default or plan) into the editor
    /// buffers. For a plan, visibility shows plan-over-default.
    pub(crate) fn aec_wall_profile_select(&mut self, name: String) -> Task<Message> {
        use crate::modules::aec::engine::display_component::{layer_filter_to_ui_state, WallComponentSlot};
        const SLOTS: [WallComponentSlot; 9] = [
            WallComponentSlot::AxisLine,
            WallComponentSlot::Contour2D,
            WallComponentSlot::ContourHatch2D,
            WallComponentSlot::Layers2D,
            WallComponentSlot::LayerHatch2D,
            WallComponentSlot::Solid3D,
            WallComponentSlot::SurfaceStyle3D,
            WallComponentSlot::SectionRepresentation,
            WallComponentSlot::ElevationRepresentation,
        ];
        let id = self.aec.styles.wall_style_editing_id.clone();
        let rules = id.as_ref().and_then(|id| self.aec_wall_profile_lookup(id, &name));
        let default_rules = match (&id, name != STYLE_DEFAULT_PROFILE_KEY) {
            (Some(id), true) => self.aec_wall_profile_lookup(id, STYLE_DEFAULT_PROFILE_KEY),
            _ => None,
        };
        let mut visibility = std::collections::HashMap::new();
        for slot in SLOTS {
            let plan_value = rules.as_ref().and_then(|r| r.visibility.get(slot.key()).copied());
            let default_value = default_rules
                .as_ref()
                .and_then(|r| r.visibility.get(slot.key()).copied());
            visibility.insert(slot, plan_value.or(default_value).unwrap_or(true));
        }
        self.aec.styles.profile_slot_visibility = visibility;
        match rules {
            Some(rules) => {
                let (c_explicit, c_sel) =
                    layer_filter_to_ui_state(rules.layer_filter_for(WallComponentSlot::Contour2D));
                let (s_explicit, s_sel) =
                    layer_filter_to_ui_state(rules.layer_filter_for(WallComponentSlot::Solid3D));
                self.aec.styles.profile_contour_explicit = c_explicit;
                self.aec.styles.profile_contour_selection = c_sel;
                self.aec.styles.profile_solid_explicit = s_explicit;
                self.aec.styles.profile_solid_selection = s_sel;
                let hatch = rules.style_override.get(WallComponentSlot::ContourHatch2D.key());
                self.aec.styles.profile_hatch_angle = hatch
                    .and_then(|h| h.hatch_angle)
                    .map(|a| format!("{a}"))
                    .unwrap_or_default();
                self.aec.styles.profile_hatch_relative =
                    hatch.and_then(|h| h.hatch_angle_relative).unwrap_or(false);
                let mut overrides = std::collections::HashMap::new();
                for slot in SLOTS {
                    if let Some(style) = rules.style_for(slot).cloned() {
                        // ContourHatch2D may only store the shared hatch-angle
                        // fields; keep those out of the per-slot override map.
                        let has_visual = style.line_type.is_some()
                            || style.line_color.is_some()
                            || style.hatch_pattern.is_some()
                            || style.hatch_color.is_some()
                            || style.fill_color.is_some();
                        if has_visual {
                            overrides.insert(slot, style);
                        }
                    }
                }
                self.aec.styles.profile_slot_overrides = overrides;
            }
            None => {
                self.aec.styles.profile_contour_explicit = false;
                self.aec.styles.profile_contour_selection = Vec::new();
                self.aec.styles.profile_solid_explicit = false;
                self.aec.styles.profile_solid_selection = Vec::new();
                self.aec.styles.profile_hatch_angle = String::new();
                self.aec.styles.profile_hatch_relative = false;
                self.aec.styles.profile_slot_overrides = std::collections::HashMap::new();
            }
        }
        self.aec.styles.profile_editing_slot = None;
        self.clear_aec_profile_slot_style_editor_buffers();
        self.aec.styles.profile_selected = Some(name);
        Task::none()
    }

    /// Profile stored under `key` (style default or plan name) for `style_id`.
    pub(crate) fn aec_wall_profile_lookup(&self, style_id: &str, key: &str) -> Option<ComponentRuleSet> {
        if key == STYLE_DEFAULT_PROFILE_KEY {
            return self
                .aec
                .aec_style_library
                .as_ref()
                .and_then(|lib| lib.wall_styles.iter().find(|w| w.style.id == style_id))
                .and_then(|ws| ws.default_profile.clone());
        }
        self.aec
            .aec_plan_library
            .as_ref()
            .and_then(|lib| lib.find(key))
            .and_then(|cfg| cfg.style_overrides.get(style_id))
            .and_then(|o| o.rules.clone())
    }

    /// Editor keys that currently carry a profile for `style_id`.
    pub(crate) fn aec_wall_profile_keys(&self, style_id: &str) -> Vec<String> {
        let mut keys: Vec<String> = Vec::new();
        if self.aec_wall_profile_lookup(style_id, STYLE_DEFAULT_PROFILE_KEY).is_some() {
            keys.push(STYLE_DEFAULT_PROFILE_KEY.to_string());
        }
        if let Some(lib) = self.aec.aec_plan_library.as_ref() {
            for cfg in &lib.configs {
                if cfg.style_overrides.get(style_id).is_some_and(|o| o.rules.is_some()) {
                    keys.push(cfg.name.clone());
                }
            }
        }
        keys
    }

    /// Writes (or clears) the plan kind `plan_name`'s rule override for `style_id`.
    pub(crate) fn aec_wall_profile_store_plan_override(
        &mut self,
        style_id: &str,
        plan_name: &str,
        rules: Option<ComponentRuleSet>,
    ) {
        let Some(lib) = self.aec.aec_plan_library.as_mut() else {
            return;
        };
        let Some(cfg) = lib.configs.iter_mut().find(|c| c.name == plan_name) else {
            return;
        };
        cfg.style_overrides
            .entry(style_id.to_string())
            .or_default()
            .rules = rules;
        let snapshot = lib.clone();
        if let Err(e) = self.aec_save_display_config_library_preferring_project(&snapshot) {
            self.command_line.push_error(
                crate::tf!("AEC Style Manager: failed to save library: {e}").as_ref(),
            );
        }
    }

    /// Saves the wall display profile edited in the profile window.
    pub(crate) fn aec_wall_profile_save(&mut self) -> Task<Message> {
        use crate::modules::aec::engine::display_component::{
            layer_filter_from_selection, ComponentStyleOverride, WallComponentSlot,
        };
        let Some(config_name) = self.aec.styles.profile_selected.clone() else {
            return Task::none();
        };
        let Some(id) = self.aec.styles.wall_style_editing_id.clone() else {
            return Task::none();
        };
        let Some(mut wall_style) = self.aec.aec_style_library.as_ref().and_then(|lib| {
            lib.wall_styles.iter().find(|w| w.style.id == id).cloned()
        }) else {
            return Task::none();
        };

        let mut rules = self
            .aec_wall_profile_lookup(&id, &config_name)
            .unwrap_or_default();
        rules.layer_filter.insert(
            WallComponentSlot::Contour2D.key().to_string(),
            layer_filter_from_selection(
                self.aec.styles.profile_contour_explicit,
                &self.aec.styles.profile_contour_selection,
            ),
        );
        rules.layer_filter.insert(
            WallComponentSlot::Solid3D.key().to_string(),
            layer_filter_from_selection(
                self.aec.styles.profile_solid_explicit,
                &self.aec.styles.profile_solid_selection,
            ),
        );
        let hatch_angle = self
            .aec.styles.profile_hatch_angle
            .trim()
            .parse::<f64>()
            .ok();
        if hatch_angle.is_some() {
            rules.style_override.insert(
                WallComponentSlot::ContourHatch2D.key().to_string(),
                ComponentStyleOverride {
                    hatch_angle,
                    hatch_angle_relative: Some(self.aec.styles.profile_hatch_relative),
                    ..Default::default()
                },
            );
        } else {
            rules.style_override.remove(WallComponentSlot::ContourHatch2D.key());
        }
        let is_plan = config_name != STYLE_DEFAULT_PROFILE_KEY;
        let default_rules = if is_plan {
            self.aec_wall_profile_lookup(&id, STYLE_DEFAULT_PROFILE_KEY)
        } else {
            None
        };
        for (slot, visible) in &self.aec.styles.profile_slot_visibility {
            if *visible {
                // A plan must say `true` explicitly to show a slot the style
                // default hides (merge only overwrites present keys).
                let default_hides = default_rules
                    .as_ref()
                    .is_some_and(|d| d.visibility.get(slot.key()) == Some(&false));
                if default_hides {
                    rules.visibility.insert(slot.key().to_string(), true);
                } else {
                    rules.visibility.remove(slot.key());
                }
            } else {
                rules.visibility.insert(slot.key().to_string(), false);
            }
        }
        // Preserve ContourHatch2D hatch-angle entry written above, then
        // merge/replace per-slot visual overrides from the pending map.
        let hatch_angle_entry = rules
            .style_override
            .get(WallComponentSlot::ContourHatch2D.key())
            .cloned();
        // Drop previous visual overrides for known slots, then rewrite.
        for slot in [
            WallComponentSlot::AxisLine,
            WallComponentSlot::Contour2D,
            WallComponentSlot::ContourHatch2D,
            WallComponentSlot::Layers2D,
            WallComponentSlot::LayerHatch2D,
            WallComponentSlot::Solid3D,
            WallComponentSlot::SurfaceStyle3D,
            WallComponentSlot::SectionRepresentation,
            WallComponentSlot::ElevationRepresentation,
        ] {
            rules.style_override.remove(slot.key());
        }
        if let Some(entry) = hatch_angle_entry {
            // Re-insert hatch-angle-only base; visual fields may be merged below.
            if entry.hatch_angle.is_some() || entry.hatch_angle_relative.is_some() {
                rules.style_override.insert(
                    WallComponentSlot::ContourHatch2D.key().to_string(),
                    ComponentStyleOverride {
                        hatch_angle: entry.hatch_angle,
                        hatch_angle_relative: entry.hatch_angle_relative,
                        ..Default::default()
                    },
                );
            }
        }
        for (slot, style) in &self.aec.styles.profile_slot_overrides {
            if *slot == WallComponentSlot::ContourHatch2D {
                let mut merged = rules
                    .style_override
                    .remove(slot.key())
                    .unwrap_or_default();
                if style.line_type.is_some() {
                    merged.line_type = style.line_type.clone();
                }
                if style.line_color.is_some() {
                    merged.line_color = style.line_color;
                }
                if style.hatch_pattern.is_some() {
                    merged.hatch_pattern = style.hatch_pattern.clone();
                }
                if style.hatch_color.is_some() {
                    merged.hatch_color = style.hatch_color;
                }
                if style.fill_color.is_some() {
                    merged.fill_color = style.fill_color;
                }
                rules
                    .style_override
                    .insert(slot.key().to_string(), merged);
            } else {
                rules
                    .style_override
                    .insert(slot.key().to_string(), style.clone());
            }
        }
        if config_name != crate::modules::aec::engine::library::STYLE_DEFAULT_PROFILE_KEY {
            // Plan-kind override: stored on the plan, not on the style.
            self.aec_wall_profile_store_plan_override(&id, &config_name, Some(rules));
            self.command_line
                .push_info(crate::t!("AEC Style Manager: display profile saved.").as_ref());
            return Task::none();
        }
        wall_style.default_profile = Some(rules);

        let source = crate::modules::aec::engine::library::wall_style_library_source_with_session(
            self.aec.aec_project_explorer_file.as_ref(),
            self.aec.aec_session_style_library.as_ref(),
            &id,
        );
        let cow_from_standard =
            source == Some(crate::modules::aec::engine::library::LibrarySource::Standard);

        if self.aec.aec_project_explorer_file.is_some() {
            match self.aec_upsert_wall_style_into_project(wall_style) {
                Ok(()) => {
                    if cow_from_standard {
                        self.command_line.push_info(
                            crate::t!(
                                "AEC Style Manager: Standard wall style was copied into the project and saved."
                            )
                            .as_ref(),
                        );
                    } else {
                        self.command_line.push_info(
                            crate::t!("AEC Style Manager: display profile saved.").as_ref(),
                        );
                    }
                }
                Err(e) => {
                    self.command_line.push_error(
                        crate::tf!("AEC Style Manager: failed to save library: {e}").as_ref(),
                    );
                }
            }
        } else {
            self.aec_upsert_wall_style_into_session(wall_style);
        }
        Task::none()
    }

    /// Removes the selected wall display profile (style default or plan override).
    pub(crate) fn aec_wall_profile_remove(&mut self) -> Task<Message> {
        let Some(config_name) = self.aec.styles.profile_selected.clone() else {
            return Task::none();
        };
        let Some(id) = self.aec.styles.wall_style_editing_id.clone() else {
            return Task::none();
        };
        let Some(mut wall_style) = self.aec.aec_style_library.as_ref().and_then(|lib| {
            lib.wall_styles.iter().find(|w| w.style.id == id).cloned()
        }) else {
            return Task::none();
        };
        if config_name == crate::modules::aec::engine::library::STYLE_DEFAULT_PROFILE_KEY {
            wall_style.default_profile = None;
        } else {
            self.aec_wall_profile_store_plan_override(&id, &config_name, None);
        }

        self.aec.styles.profile_contour_explicit = false;
        self.aec.styles.profile_contour_selection = Vec::new();
        self.aec.styles.profile_solid_explicit = false;
        self.aec.styles.profile_solid_selection = Vec::new();
        self.aec.styles.profile_hatch_angle = String::new();
        self.aec.styles.profile_hatch_relative = false;
        self.aec.styles.profile_slot_visibility = std::collections::HashMap::new();
        self.aec.styles.profile_slot_overrides = std::collections::HashMap::new();
        self.aec.styles.profile_editing_slot = None;
        self.clear_aec_profile_slot_style_editor_buffers();

        if self.aec.aec_project_explorer_file.is_some() {
            if let Err(e) = self.aec_upsert_wall_style_into_project(wall_style) {
                self.command_line.push_error(
                    crate::tf!("AEC Style Manager: failed to save library: {e}").as_ref(),
                );
            } else {
                self.command_line
                    .push_info(crate::t!("AEC Style Manager: display profile removed.").as_ref());
            }
        } else {
            self.aec_upsert_wall_style_into_session(wall_style);
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AecMessage;
    use crate::modules::aec::engine::display_component::WallComponentSlot;
    use crate::modules::aec::engine::library::DisplayConfigLibrary;
    use crate::modules::aec::engine::plan_view::{DisplayConfig, PlanningStage, ViewType};
    use crate::modules::aec::engine::wall_style::WallStyle;
    use crate::modules::aec::engine::style_common::{StyleCommon, StyleOrigin, CATEGORY_WALL};

    #[test]
    fn save_and_remove_default_and_plan_profiles() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let id = "t_wall_profile";
        app.aec_upsert_wall_style_into_session(WallStyle {
            style: StyleCommon::new(id, "Wall", None, CATEGORY_WALL, StyleOrigin::Session),
            layers: Vec::new(),
            default_profile: None,
        });
        let mut plans = DisplayConfigLibrary::empty();
        plans.configs.push(DisplayConfig::new("Wall plan".into(), "A".into(), PlanningStage::Execution, ViewType::FloorPlan));
        app.aec.aec_plan_library = Some(plans);
        app.aec.styles.wall_style_editing_id = Some(id.into());
        app.aec.styles.profile_selected = Some(STYLE_DEFAULT_PROFILE_KEY.into());
        app.aec.styles.profile_slot_visibility.insert(WallComponentSlot::Contour2D, false);
        let _ = app.update_aec(AecMessage::AecStyleManagerProfileSave);
        assert_eq!(app.aec.aec_style_library.as_ref().unwrap().wall_styles.iter().find(|s| s.style.id == id).unwrap()
            .default_profile.as_ref().unwrap().visibility[WallComponentSlot::Contour2D.key()], false);

        app.aec.styles.profile_selected = Some("Wall plan".into());
        app.aec.styles.profile_slot_visibility.insert(WallComponentSlot::Solid3D, false);
        let _ = app.update_aec(AecMessage::AecStyleManagerProfileSave);
        assert_eq!(app.aec.aec_plan_library.as_ref().unwrap().find("Wall plan").unwrap()
            .style_overrides[id].rules.as_ref().unwrap().visibility[WallComponentSlot::Solid3D.key()], false);
        let _ = app.update_aec(AecMessage::AecStyleManagerProfileRemove);
        assert!(app.aec.aec_plan_library.as_ref().unwrap().find("Wall plan").unwrap()
            .style_overrides[id].rules.is_none());
        assert!(app.aec.aec_style_library.as_ref().unwrap().wall_styles.iter().find(|s| s.style.id == id).unwrap().default_profile.is_some());
        app.aec.styles.profile_selected = Some(STYLE_DEFAULT_PROFILE_KEY.into());
        let _ = app.update_aec(AecMessage::AecStyleManagerProfileRemove);
        assert!(app.aec.aec_style_library.as_ref().unwrap().wall_styles.iter().find(|s| s.style.id == id).unwrap().default_profile.is_none());
    }

    #[test]
    fn plan_profile_can_show_slot_hidden_by_style_default() {
        use crate::modules::aec::engine::library::build_effective_rule_set;
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let id = "t_wall_vis";
        let slot = WallComponentSlot::Solid3D;
        let mut default = ComponentRuleSet::default();
        default.visibility.insert(slot.key().to_string(), false);
        app.aec_upsert_wall_style_into_session(WallStyle {
            style: StyleCommon::new(id, "Wall", None, CATEGORY_WALL, StyleOrigin::Session),
            layers: Vec::new(),
            default_profile: Some(default),
        });
        let mut plans = DisplayConfigLibrary::empty();
        plans.configs.push(DisplayConfig::new("Wall plan".into(), "A".into(), PlanningStage::Execution, ViewType::FloorPlan));
        app.aec.aec_plan_library = Some(plans);
        app.aec.styles.wall_style_editing_id = Some(id.into());

        // Plan without override shows the merged (hidden) default.
        let _ = app.update_aec(AecMessage::AecStyleManagerProfileSelect("Wall plan".into()));
        assert_eq!(app.aec.styles.profile_slot_visibility[&slot], false);
        // Show it in the plan only.
        app.aec.styles.profile_slot_visibility.insert(slot, true);
        let _ = app.update_aec(AecMessage::AecStyleManagerProfileSave);

        let lib = app.aec.aec_style_library.as_ref().unwrap();
        let style = lib.wall_styles.iter().find(|s| s.style.id == id).unwrap();
        assert_eq!(style.default_profile.as_ref().unwrap().visibility[slot.key()], false);
        let cfg = app.aec.aec_plan_library.as_ref().unwrap().find("Wall plan").unwrap();
        assert_eq!(cfg.style_overrides[id].rules.as_ref().unwrap().visibility[slot.key()], true);
        let eff = build_effective_rule_set(cfg, Some(style), None);
        assert!(eff.is_visible(slot));
        // Reload shows the explicit override.
        let _ = app.update_aec(AecMessage::AecStyleManagerProfileSelect(STYLE_DEFAULT_PROFILE_KEY.into()));
        assert_eq!(app.aec.styles.profile_slot_visibility[&slot], false);
        let _ = app.update_aec(AecMessage::AecStyleManagerProfileSelect("Wall plan".into()));
        assert_eq!(app.aec.styles.profile_slot_visibility[&slot], true);
    }

    #[test]
    fn wall_duplicate_keeps_default_profile_but_not_plan_overrides() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let id = "t_wall_dup";
        let slot = WallComponentSlot::Solid3D;
        let mut default = ComponentRuleSet::default();
        default.visibility.insert(slot.key().to_string(), false);
        app.aec_upsert_wall_style_into_session(WallStyle {
            style: StyleCommon::new(id, "Wall", None, CATEGORY_WALL, StyleOrigin::Session),
            layers: Vec::new(),
            default_profile: Some(default.clone()),
        });
        let mut plans = DisplayConfigLibrary::empty();
        plans.configs.push(DisplayConfig::new("P".into(), "A".into(), PlanningStage::Execution, ViewType::FloorPlan));
        app.aec.aec_plan_library = Some(plans);
        app.aec.styles.wall_style_editing_id = Some(id.into());
        app.aec.styles.wall_style_form_open = true;
        app.aec.styles.wall_style_name = "Wall".into();
        let _ = app.update_aec(AecMessage::AecStyleManagerProfileSelect("P".into()));
        app.aec.styles.profile_slot_visibility.insert(WallComponentSlot::Contour2D, false);
        let _ = app.update_aec(AecMessage::AecStyleManagerProfileSave);

        let _ = app.update_aec(AecMessage::AecStyleManagerWallStyleDuplicate);
        // Duplicating the still-unsaved copy keeps the carried default.
        let _ = app.update_aec(AecMessage::AecStyleManagerWallStyleDuplicate);
        let _ = app.update_aec(AecMessage::AecStyleManagerWallStyleSave);
        let lib = app.aec.aec_style_library.as_ref().unwrap();
        let copy = lib.wall_styles.iter().find(|s| s.style.name == "Wall (Kopie) (Kopie)").unwrap();
        assert_eq!(copy.default_profile.as_ref(), Some(&default));
        let copy_id = copy.style.id.clone();
        let cfg = app.aec.aec_plan_library.as_ref().unwrap().find("P").unwrap();
        assert!(cfg.style_overrides.contains_key(id));
        assert!(!cfg.style_overrides.contains_key(&copy_id));
        assert!(app.aec.styles.wall_duplicate_default_profile.is_none());
    }

    #[test]
    fn failed_save_keeps_carried_default_for_retry() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let id = "t_wall_dup_fail";
        let mut default = ComponentRuleSet::default();
        default.visibility.insert(WallComponentSlot::Solid3D.key().to_string(), false);
        app.aec_upsert_wall_style_into_session(WallStyle {
            style: StyleCommon::new(id, "Wall", None, CATEGORY_WALL, StyleOrigin::Session),
            layers: Vec::new(),
            default_profile: Some(default.clone()),
        });
        app.aec.styles.wall_style_editing_id = Some(id.into());
        app.aec.styles.wall_style_form_open = true;
        app.aec.styles.wall_style_name = "Wall".into();
        let _ = app.update_aec(AecMessage::AecStyleManagerWallStyleDuplicate);
        assert_eq!(app.aec.styles.wall_duplicate_default_profile.as_ref(), Some(&default));

        // Unwritable project path: the upsert fails.
        app.aec.aec_project_explorer_file = Some(Default::default());
        app.aec.aec_project_explorer_path =
            Some(std::path::PathBuf::from("/proc/ocs_nonexistent/p.ocsproj"));
        let _ = app.update_aec(AecMessage::AecStyleManagerWallStyleSave);
        assert!(app.aec.styles.wall_style_editing_id.is_none());
        assert_eq!(app.aec.styles.wall_duplicate_default_profile.as_ref(), Some(&default));

        // Retry succeeds (no path -> in-memory only) and keeps the default.
        app.aec.aec_project_explorer_path = None;
        let _ = app.update_aec(AecMessage::AecStyleManagerWallStyleSave);
        assert!(app.aec.styles.wall_duplicate_default_profile.is_none());
        let lib = app.aec.aec_style_library.as_ref().unwrap();
        let copy = lib.wall_styles.iter().find(|s| s.style.name == "Wall (Kopie)").unwrap();
        assert_eq!(copy.default_profile.as_ref(), Some(&default));
    }
}
