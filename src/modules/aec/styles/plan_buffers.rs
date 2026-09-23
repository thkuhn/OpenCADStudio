//! Plan Manager editor buffers (phase filter, overlay, hatch, display).
//!
//! Bodies live here (still `impl OpenCADStudio`) so Core `update/mod.rs`
//! does not grow AEC plan-manager buffer logic.

use crate::app::OpenCADStudio;

impl OpenCADStudio {
    /// Two-stage phase-filter editor (Step 5): loads `filter` (or the
    /// "unfiltered"/blank defaults if `None`) into the edit buffers backing
    /// the DisplayConfig form's phase-filter section.
    pub(crate) fn aec_plan_manager_load_phase_filter_buffers(
        &mut self,
        filter: Option<&crate::modules::aec::engine::plan_view::PhaseFilter>,
    ) {
        use crate::modules::aec::engine::plan_view::PlanPhase;
        let visible = |phase: PlanPhase| match filter {
            Some(f) => f.visible_phases.contains(&phase),
            None => true,
        };
        self.aec.aec_plan_manager_phase_filter_visible_existing = visible(PlanPhase::Existing);
        self.aec.aec_plan_manager_phase_filter_visible_demolition = visible(PlanPhase::Demolition);
        self.aec.aec_plan_manager_phase_filter_visible_new = visible(PlanPhase::New);

        let demolition = filter.and_then(|f| f.demolition_style.clone()).unwrap_or_default();
        self.aec.aec_plan_manager_demolition_style_line_type =
            demolition.line_type.clone().unwrap_or_default();
        use crate::modules::aec::ui::aec_ui_util::acad_color_to_editor_string;
        self.aec.aec_plan_manager_demolition_style_line_color = demolition
            .line_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();
        self.aec.aec_plan_manager_demolition_style_hatch_pattern =
            demolition.hatch_pattern.clone().unwrap_or_default();
        self.aec.aec_plan_manager_demolition_style_hatch_color = demolition
            .hatch_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();
        self.aec.aec_plan_manager_demolition_style_fill_color = demolition
            .fill_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();

        let existing = filter.and_then(|f| f.existing_style.clone()).unwrap_or_default();
        self.aec.aec_plan_manager_existing_style_line_type =
            existing.line_type.clone().unwrap_or_default();
        self.aec.aec_plan_manager_existing_style_line_color = existing
            .line_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();
        self.aec.aec_plan_manager_existing_style_hatch_pattern =
            existing.hatch_pattern.clone().unwrap_or_default();
        self.aec.aec_plan_manager_existing_style_hatch_color = existing
            .hatch_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();
        self.aec.aec_plan_manager_existing_style_fill_color = existing
            .fill_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();
    }

    /// Two-stage phase-filter editor (Step 5): builds a [`PhaseFilter`]
    /// from the current edit buffers. Returns `None` when every phase is
    /// visible and neither style overlay is set — the "unfiltered"/legacy
    /// default — so a config left untouched keeps `phase_filter == None`.
    pub(crate) fn aec_plan_manager_build_phase_filter(
        &self,
    ) -> Option<crate::modules::aec::engine::plan_view::PhaseFilter> {
        use crate::modules::aec::engine::display_component::component_style_override_from_editor_fields;
        use crate::modules::aec::engine::plan_view::PlanPhase;

        let mut visible_phases = Vec::new();
        if self.aec.aec_plan_manager_phase_filter_visible_existing {
            visible_phases.push(PlanPhase::Existing);
        }
        if self.aec.aec_plan_manager_phase_filter_visible_demolition {
            visible_phases.push(PlanPhase::Demolition);
        }
        if self.aec.aec_plan_manager_phase_filter_visible_new {
            visible_phases.push(PlanPhase::New);
        }

        let demolition_style = component_style_override_from_editor_fields(
            &self.aec.aec_plan_manager_demolition_style_line_type,
            &self.aec.aec_plan_manager_demolition_style_line_color,
            &self.aec.aec_plan_manager_demolition_style_hatch_pattern,
            &self.aec.aec_plan_manager_demolition_style_hatch_color,
            &self.aec.aec_plan_manager_demolition_style_fill_color,
        );
        let demolition_style = (demolition_style != Default::default()).then_some(demolition_style);

        let existing_style = component_style_override_from_editor_fields(
            &self.aec.aec_plan_manager_existing_style_line_type,
            &self.aec.aec_plan_manager_existing_style_line_color,
            &self.aec.aec_plan_manager_existing_style_hatch_pattern,
            &self.aec.aec_plan_manager_existing_style_hatch_color,
            &self.aec.aec_plan_manager_existing_style_fill_color,
        );
        let existing_style = (existing_style != Default::default()).then_some(existing_style);

        let all_visible = visible_phases.len() == 3;
        if all_visible && demolition_style.is_none() && existing_style.is_none() {
            return None;
        }

        Some(crate::modules::aec::engine::plan_view::PhaseFilter {
            visible_phases,
            demolition_style,
            existing_style,
        })
    }

    pub(crate) fn aec_plan_manager_clear_overlay_field_buffers(&mut self) {
        self.aec.aec_plan_manager_overlay_line_type.clear();
        self.aec.aec_plan_manager_overlay_line_color.clear();
        self.aec.aec_plan_manager_overlay_hatch_pattern.clear();
        self.aec.aec_plan_manager_overlay_hatch_color.clear();
        self.aec.aec_plan_manager_overlay_hatch_scale.clear();
        self.aec.aec_plan_manager_overlay_hatch_angle.clear();
        self.aec.aec_plan_manager_overlay_hatch_angle_relative = None;
        self.aec.aec_plan_manager_overlay_fill_color.clear();
    }

    pub(crate) fn aec_plan_manager_override_from_hatch_buffers(
        pattern: &str,
        color: &str,
        scale: &str,
        angle: &str,
        relative: Option<bool>,
    ) -> crate::modules::aec::engine::display_component::ComponentStyleOverride {
        use crate::modules::aec::ui::aec_ui_util::editor_string_to_acad_color;
        let nonempty = |s: &str| {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        };
        crate::modules::aec::engine::display_component::ComponentStyleOverride {
            hatch_pattern: nonempty(pattern),
            hatch_color: editor_string_to_acad_color(color),
            hatch_scale: scale.trim().parse::<f64>().ok().filter(|s| *s > 0.0),
            hatch_angle: angle.trim().parse::<f64>().ok(),
            hatch_angle_relative: relative,
            ..Default::default()
        }
    }

    pub(crate) fn aec_plan_manager_load_contour_hatch_buffers(
        &mut self,
        hatch: Option<&crate::modules::aec::engine::display_component::ComponentStyleOverride>,
    ) {
        use crate::modules::aec::ui::aec_ui_util::acad_color_to_editor_string;
        let hatch = hatch.cloned().unwrap_or_default();
        self.aec.aec_plan_manager_contour_hatch_pattern =
            hatch.hatch_pattern.clone().unwrap_or_default();
        self.aec.aec_plan_manager_contour_hatch_color = hatch
            .hatch_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();
        self.aec.aec_plan_manager_contour_hatch_scale = hatch
            .hatch_scale
            .map(|s| s.to_string())
            .unwrap_or_default();
        self.aec.aec_plan_manager_contour_hatch_angle = hatch
            .hatch_angle
            .map(|a| a.to_string())
            .unwrap_or_default();
        self.aec.aec_plan_manager_contour_hatch_angle_relative = hatch.hatch_angle_relative;
    }

    pub(crate) fn aec_plan_manager_reset_display_buffers(&mut self) {
        self.aec.aec_plan_manager_editing_id = None;
        self.aec.aec_plan_manager_default_representation =
            crate::modules::aec::engine::display_component::RepresentationMode::All;
        self.aec.aec_plan_manager_component_visibility.clear();
        self.aec.aec_plan_manager_style_overlays.clear();
        self.aec.aec_plan_manager_overlay_style_id = None;
        self.aec.aec_plan_manager_overlay_layer_id = None;
        self.aec_plan_manager_clear_overlay_field_buffers();
        self.aec_plan_manager_load_contour_hatch_buffers(None);
    }

    pub(crate) fn aec_plan_manager_load_display_buffers(
        &mut self,
        cfg: &crate::modules::aec::engine::plan_view::DisplayConfig,
    ) {
        self.aec.aec_plan_manager_editing_id = Some(cfg.id);
        self.aec.aec_plan_manager_default_representation = cfg.default_representation;
        self.aec.aec_plan_manager_component_visibility = cfg.component_visibility.clone();
        self.aec.aec_plan_manager_style_overlays = cfg.style_overlays.clone();
        self.aec.aec_plan_manager_overlay_style_id = cfg.style_overlays.keys().next().cloned();
        self.aec.aec_plan_manager_overlay_layer_id = self
            .aec.aec_plan_manager_overlay_style_id
            .as_ref()
            .and_then(|sid| {
                self.aec.aec_plan_manager_style_overlays
                    .get(sid)
                    .and_then(|o| o.layer_props.keys().next().copied())
            });
        self.aec_plan_manager_load_overlay_layer_buffers();
        self.aec_plan_manager_load_overlay_contour_hatch_buffers();
    }

    pub(crate) fn aec_plan_manager_load_overlay_layer_buffers(&mut self) {
        use crate::modules::aec::ui::aec_ui_util::acad_color_to_editor_string;
        let props = self
            .aec.aec_plan_manager_overlay_style_id
            .as_ref()
            .and_then(|sid| self.aec.aec_plan_manager_style_overlays.get(sid))
            .and_then(|o| {
                self.aec.aec_plan_manager_overlay_layer_id
                    .and_then(|lid| o.layer_props.get(&lid))
            })
            .cloned()
            .unwrap_or_default();
        self.aec.aec_plan_manager_overlay_line_type = props.line_type.clone().unwrap_or_default();
        self.aec.aec_plan_manager_overlay_line_color = props
            .line_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();
        self.aec.aec_plan_manager_overlay_hatch_pattern =
            props.hatch_pattern.clone().unwrap_or_default();
        self.aec.aec_plan_manager_overlay_hatch_color = props
            .hatch_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();
        self.aec.aec_plan_manager_overlay_hatch_scale = props
            .hatch_scale
            .map(|s| s.to_string())
            .unwrap_or_default();
        self.aec.aec_plan_manager_overlay_hatch_angle = props
            .hatch_angle
            .map(|a| a.to_string())
            .unwrap_or_default();
        self.aec.aec_plan_manager_overlay_hatch_angle_relative = props.hatch_angle_relative;
        self.aec.aec_plan_manager_overlay_fill_color = props
            .fill_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();
    }

    pub(crate) fn aec_plan_manager_write_overlay_buffers(&mut self) {
        use crate::modules::aec::engine::display_component::ComponentStyleOverride;
        use crate::modules::aec::ui::aec_ui_util::editor_string_to_acad_color;
        let Some(style_id) = self.aec.aec_plan_manager_overlay_style_id.clone() else {
            return;
        };
        let Some(layer_id) = self.aec.aec_plan_manager_overlay_layer_id else {
            return;
        };
        let nonempty = |s: &str| {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        };
        let props = ComponentStyleOverride {
            line_type: nonempty(&self.aec.aec_plan_manager_overlay_line_type),
            line_color: editor_string_to_acad_color(&self.aec.aec_plan_manager_overlay_line_color),
            hatch_pattern: nonempty(&self.aec.aec_plan_manager_overlay_hatch_pattern),
            hatch_color: editor_string_to_acad_color(&self.aec.aec_plan_manager_overlay_hatch_color),
            hatch_scale: self
                .aec.aec_plan_manager_overlay_hatch_scale
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|s| *s > 0.0),
            fill_color: editor_string_to_acad_color(&self.aec.aec_plan_manager_overlay_fill_color),
            hatch_angle: self
                .aec.aec_plan_manager_overlay_hatch_angle
                .trim()
                .parse::<f64>()
                .ok(),
            hatch_angle_relative: self.aec.aec_plan_manager_overlay_hatch_angle_relative,
            cad_layer: None,
        };
        let overlay = self
            .aec.aec_plan_manager_style_overlays
            .entry(style_id)
            .or_default();
        if props == ComponentStyleOverride::default() {
            overlay.layer_props.remove(&layer_id);
        } else {
            overlay.layer_props.insert(layer_id, props);
        }
    }

    pub(crate) fn aec_plan_manager_load_overlay_contour_hatch_buffers(&mut self) {
        let hatch = self
            .aec.aec_plan_manager_overlay_style_id
            .as_ref()
            .and_then(|sid| self.aec.aec_plan_manager_style_overlays.get(sid))
            .and_then(|o| o.contour_hatch.clone());
        self.aec_plan_manager_load_contour_hatch_buffers(hatch.as_ref());
    }

    pub(crate) fn aec_plan_manager_write_overlay_contour_hatch(&mut self) {
        let Some(style_id) = self.aec.aec_plan_manager_overlay_style_id.clone() else {
            return;
        };
        let contour = Self::aec_plan_manager_override_from_hatch_buffers(
            &self.aec.aec_plan_manager_contour_hatch_pattern,
            &self.aec.aec_plan_manager_contour_hatch_color,
            &self.aec.aec_plan_manager_contour_hatch_scale,
            &self.aec.aec_plan_manager_contour_hatch_angle,
            self.aec.aec_plan_manager_contour_hatch_angle_relative,
        );
        let overlay = self
            .aec.aec_plan_manager_style_overlays
            .entry(style_id)
            .or_default();
        overlay.contour_hatch = if contour
            == crate::modules::aec::engine::display_component::ComponentStyleOverride::default()
        {
            None
        } else {
            Some(contour)
        };
    }

    pub(crate) fn aec_plan_manager_save_internal(&mut self) -> Option<String> {
        let name = self.aec.aec_plan_manager_name.trim().to_string();
        if name.is_empty() {
            self.command_line.push_error(
                crate::t!("AEC DisplayConfig Manager: name cannot be empty.").as_ref(),
            );
            return None;
        }
        let discipline = self.aec.aec_plan_manager_discipline.trim().to_string();
        let scale = self.aec.aec_plan_manager_scale.trim().parse::<f64>().ok();

        let mut config = crate::modules::aec::engine::plan_view::DisplayConfig::new(
            name.clone(),
            discipline,
            self.aec.aec_plan_manager_planning_stage,
            self.aec.aec_plan_manager_view_type.clone(),
        );
        config.scale = scale;
        if let Some(id) = self.aec.aec_plan_manager_editing_id {
            config.id = id;
        }
        self.aec_plan_manager_write_overlay_buffers();
        self.aec_plan_manager_write_overlay_contour_hatch();
        config.default_representation = self.aec.aec_plan_manager_default_representation;
        config.component_visibility = self.aec.aec_plan_manager_component_visibility.clone();
        config.style_overlays = self.aec.aec_plan_manager_style_overlays.clone();
        config.contour_hatch = None;

        config.phase_filter = self.aec_plan_manager_build_phase_filter();

        if let Some(old_name) = self.aec.aec_plan_manager_editing_name.clone() {
            if old_name != name {
                if let Some(lib) = self.aec.aec_plan_library.as_mut() {
                    lib.remove(&old_name);
                }
            }
        }

        let lib = self
            .aec.aec_plan_library
            .get_or_insert_with(crate::modules::aec::engine::library::DisplayConfigLibrary::empty);
        lib.upsert(config.clone());
        let lib_snapshot = lib.clone();
        match self.aec_save_display_config_library_preferring_project(&lib_snapshot) {
            Ok(()) => self.command_line.push_info(
                crate::t!("AEC DisplayConfig Manager: config saved.").as_ref(),
            ),
            Err(e) => self.command_line.push_error(
                crate::tf!("AEC DisplayConfig Manager: failed to save library: {e}").as_ref(),
            ),
        }

        self.aec.aec_plan_manager_editing_name = Some(name.clone());
        self.aec.aec_plan_manager_selected = Some(name.clone());
        self.aec.aec_plan_manager_editing_id = Some(config.id);
        Some(name)
    }
}
