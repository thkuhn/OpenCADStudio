//! AEC update handlers: Plan manager and display configuration.

use iced::Task;

use crate::app::{Message, OpenCADStudio};
use crate::modules::aec::message::AecMessage;

impl OpenCADStudio {
    pub(crate) fn update_aec_plan_display(&mut self, msg: AecMessage) -> Task<Message> {
        match msg {
            AecMessage::AecPlanManagerOpen => self.aec_plan_kinds_open(),
            AecMessage::AecPlanManagerDetailLayersChanged(layers) => {
                self.aec.styles.plan_detail.layers = layers;
                Task::none()
            }
            AecMessage::AecPlanManagerDetailToggle(switch, value) => {
                self.aec.styles.plan_detail.set_switch(switch, value);
                Task::none()
            }
            AecMessage::AecPlanManagerFilter(value) => {
                self.aec.styles.plan_filter = value;
                Task::none()
            }
            AecMessage::AecPlanManagerSelect(name) => self.aec_plan_kind_select(name),
            AecMessage::AecPlanManagerNew => self.aec_plan_kind_new(),
            AecMessage::AecPlanManagerDuplicate => self.aec_plan_kind_duplicate(),
            AecMessage::AecPlanManagerDelete => {
                if let Some(name) = self.aec.styles.plan_selected.clone() {
                    let lib_snapshot = if let Some(lib) = self.aec.aec_plan_library.as_mut() {
                        lib.remove(&name);
                        Some(lib.clone())
                    } else {
                        None
                    };
                    if let Some(lib_snapshot) = lib_snapshot {
                        match self.aec_save_display_config_library_preferring_project(&lib_snapshot) {
                            Ok(()) => self.command_line.push_info(
                                crate::t!("AEC DisplayConfig Manager: config deleted.").as_ref(),
                            ),
                            Err(e) => self.command_line.push_error(
                                crate::tf!("AEC DisplayConfig Manager: failed to save library: {e}")
                                    .as_ref(),
                            ),
                        }
                    }
                }
                self.aec.styles.plan_selected = None;
                self.aec.styles.plan_editing_name = None;
                self.aec.styles.plan_form_open = false;
                Task::none()
            }
            AecMessage::AecPlanManagerNameChanged(value) => {
                self.aec.styles.plan_name = value;
                Task::none()
            }
            AecMessage::AecPlanManagerDisciplineChanged(value) => {
                self.aec.styles.plan_discipline = value;
                Task::none()
            }
            AecMessage::AecPlanManagerScaleChanged(value) => {
                self.aec.styles.plan_scale = value;
                Task::none()
            }
            AecMessage::AecPlanManagerPlanningStageChanged(stage) => {
                self.aec.styles.plan_planning_stage = stage;
                Task::none()
            }
            AecMessage::AecPlanManagerViewTypeChanged(view_type) => {
                self.aec.styles.plan_view_type = view_type;
                Task::none()
            }
            AecMessage::AecPlanManagerOpeningLevelChanged(key, value) => {
                self.aec_plan_manager_opening_level_changed(&key, &value);
                Task::none()
            }
            AecMessage::AecPlanManagerPhaseVisibleToggle(phase, visible) => {
                match phase {
                    crate::modules::aec::engine::plan_view::PlanPhase::Existing => {
                        self.aec.styles.plan_phase_filter_visible_existing = visible;
                    }
                    crate::modules::aec::engine::plan_view::PlanPhase::Demolition => {
                        self.aec.styles.plan_phase_filter_visible_demolition = visible;
                    }
                    crate::modules::aec::engine::plan_view::PlanPhase::New => {
                        self.aec.styles.plan_phase_filter_visible_new = visible;
                    }
                }
                Task::none()
            }
            AecMessage::AecPlanManagerDemolitionStyleLineTypeChanged(value) => {
                self.ensure_aec_catalog_linetype(&value);
                self.aec.styles.plan_demolition_style_line_type = value;
                Task::none()
            }
            AecMessage::AecPlanManagerDemolitionStyleLineColorChanged(value) => {
                self.aec.styles.plan_demolition_style_line_color = value;
                Task::none()
            }
            AecMessage::AecPlanManagerDemolitionStyleHatchPatternChanged(value) => {
                self.aec.styles.plan_demolition_style_hatch_pattern = value;
                self.aec.styles.plan_demolition_style_hatch_picker_open = false;
                Task::none()
            }
            AecMessage::AecPlanManagerDemolitionStyleHatchPickerToggle => {
                self.aec.styles.plan_demolition_style_hatch_picker_open =
                    !self.aec.styles.plan_demolition_style_hatch_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerDemolitionStyleHatchColorChanged(value) => {
                self.aec.styles.plan_demolition_style_hatch_color = value;
                Task::none()
            }
            AecMessage::AecPlanManagerDemolitionStyleFillColorChanged(value) => {
                self.aec.styles.plan_demolition_style_fill_color = value;
                Task::none()
            }
            AecMessage::AecPlanManagerDemolitionStyleLineColorPickerToggle => {
                self.aec.styles.plan_demolition_style_line_color_picker_open =
                    !self.aec.styles.plan_demolition_style_line_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerDemolitionStyleHatchColorPickerToggle => {
                self.aec.styles.plan_demolition_style_hatch_color_picker_open =
                    !self.aec.styles.plan_demolition_style_hatch_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerDemolitionStyleFillColorPickerToggle => {
                self.aec.styles.plan_demolition_style_fill_color_picker_open =
                    !self.aec.styles.plan_demolition_style_fill_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerExistingStyleLineTypeChanged(value) => {
                self.ensure_aec_catalog_linetype(&value);
                self.aec.styles.plan_existing_style_line_type = value;
                Task::none()
            }
            AecMessage::AecPlanManagerExistingStyleLineColorChanged(value) => {
                self.aec.styles.plan_existing_style_line_color = value;
                Task::none()
            }
            AecMessage::AecPlanManagerExistingStyleHatchPatternChanged(value) => {
                self.aec.styles.plan_existing_style_hatch_pattern = value;
                self.aec.styles.plan_existing_style_hatch_picker_open = false;
                Task::none()
            }
            AecMessage::AecPlanManagerExistingStyleHatchPickerToggle => {
                self.aec.styles.plan_existing_style_hatch_picker_open =
                    !self.aec.styles.plan_existing_style_hatch_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerExistingStyleHatchColorChanged(value) => {
                self.aec.styles.plan_existing_style_hatch_color = value;
                Task::none()
            }
            AecMessage::AecPlanManagerExistingStyleFillColorChanged(value) => {
                self.aec.styles.plan_existing_style_fill_color = value;
                Task::none()
            }
            AecMessage::AecPlanManagerExistingStyleLineColorPickerToggle => {
                self.aec.styles.plan_existing_style_line_color_picker_open =
                    !self.aec.styles.plan_existing_style_line_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerExistingStyleHatchColorPickerToggle => {
                self.aec.styles.plan_existing_style_hatch_color_picker_open =
                    !self.aec.styles.plan_existing_style_hatch_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerExistingStyleFillColorPickerToggle => {
                self.aec.styles.plan_existing_style_fill_color_picker_open =
                    !self.aec.styles.plan_existing_style_fill_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerRepresentationChanged(mode) => {
                self.aec.styles.plan_default_representation = mode;
                Task::none()
            }
            AecMessage::AecPlanManagerComponentVisibleToggle(kind, visible) => {
                if visible {
                    self.aec.styles.plan_component_visibility.remove(&kind);
                } else {
                    self.aec.styles.plan_component_visibility.insert(kind, false);
                }
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayStyleSelect(style_id) => {
                self.aec_plan_manager_write_overlay_buffers();
                self.aec_plan_manager_write_overlay_contour_hatch();
                self.aec.styles.plan_overlay_style_id = Some(style_id);
                self.aec.styles.plan_overlay_layer_id = self
                    .aec.styles.plan_overlay_style_id
                    .as_ref()
                    .and_then(|sid| {
                        self.aec.styles.plan_style_overlays
                            .get(sid)
                            .and_then(|o| o.layer_props.keys().next().copied())
                    });
                self.aec_plan_manager_load_overlay_layer_buffers();
                self.aec_plan_manager_load_overlay_contour_hatch_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayLayerSelect(layer_id) => {
                self.aec_plan_manager_write_overlay_buffers();
                self.aec.styles.plan_overlay_layer_id = Some(layer_id);
                self.aec_plan_manager_load_overlay_layer_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayAddStyle(style_id) => {
                self.aec_plan_manager_write_overlay_buffers();
                self.aec_plan_manager_write_overlay_contour_hatch();
                self.aec.styles.plan_style_overlays
                    .entry(style_id.clone())
                    .or_default();
                self.aec.styles.plan_overlay_style_id = Some(style_id);
                self.aec.styles.plan_overlay_layer_id = None;
                self.aec_plan_manager_clear_overlay_field_buffers();
                self.aec_plan_manager_load_overlay_contour_hatch_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayRemoveStyle(style_id) => {
                self.aec.styles.plan_style_overlays.remove(&style_id);
                if self.aec.styles.plan_overlay_style_id.as_deref() == Some(style_id.as_str()) {
                    self.aec.styles.plan_overlay_style_id = None;
                }
                self.aec.styles.plan_overlay_layer_id = None;
                if self.aec.styles.plan_overlay_style_id.is_none() {
                    self.aec.styles.plan_overlay_style_id =
                        self.aec.styles.plan_style_overlays.keys().next().cloned();
                }
                self.aec_plan_manager_load_overlay_layer_buffers();
                self.aec_plan_manager_load_overlay_contour_hatch_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayLineTypeChanged(value) => {
                self.ensure_aec_catalog_linetype(&value);
                self.aec.styles.plan_overlay_line_type = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayLineColorChanged(value) => {
                self.aec.styles.plan_overlay_line_color = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayHatchPatternChanged(value) => {
                self.aec.styles.plan_overlay_hatch_pattern = value;
                self.aec.styles.plan_overlay_hatch_picker_open = false;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayHatchColorChanged(value) => {
                self.aec.styles.plan_overlay_hatch_color = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayHatchScaleChanged(value) => {
                self.aec.styles.plan_overlay_hatch_scale = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayHatchAngleChanged(value) => {
                self.aec.styles.plan_overlay_hatch_angle = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayHatchAngleRelativeChanged(value) => {
                self.aec.styles.plan_overlay_hatch_angle_relative = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayFillColorChanged(value) => {
                self.aec.styles.plan_overlay_fill_color = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayLineColorPickerToggle => {
                self.aec.styles.plan_overlay_line_color_picker_open =
                    !self.aec.styles.plan_overlay_line_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayHatchPickerToggle => {
                self.aec.styles.plan_overlay_hatch_picker_open =
                    !self.aec.styles.plan_overlay_hatch_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayHatchColorPickerToggle => {
                self.aec.styles.plan_overlay_hatch_color_picker_open =
                    !self.aec.styles.plan_overlay_hatch_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayFillColorPickerToggle => {
                self.aec.styles.plan_overlay_fill_color_picker_open =
                    !self.aec.styles.plan_overlay_fill_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerContourHatchPatternChanged(value) => {
                self.aec.styles.plan_contour_hatch_pattern = value;
                self.aec.styles.plan_contour_hatch_picker_open = false;
                self.aec_plan_manager_write_overlay_contour_hatch();
                Task::none()
            }
            AecMessage::AecPlanManagerContourHatchColorChanged(value) => {
                self.aec.styles.plan_contour_hatch_color = value;
                self.aec_plan_manager_write_overlay_contour_hatch();
                Task::none()
            }
            AecMessage::AecPlanManagerContourHatchScaleChanged(value) => {
                self.aec.styles.plan_contour_hatch_scale = value;
                self.aec_plan_manager_write_overlay_contour_hatch();
                Task::none()
            }
            AecMessage::AecPlanManagerContourHatchAngleChanged(value) => {
                self.aec.styles.plan_contour_hatch_angle = value;
                self.aec_plan_manager_write_overlay_contour_hatch();
                Task::none()
            }
            AecMessage::AecPlanManagerContourHatchAngleRelativeChanged(value) => {
                self.aec.styles.plan_contour_hatch_angle_relative = value;
                self.aec_plan_manager_write_overlay_contour_hatch();
                Task::none()
            }
            AecMessage::AecPlanManagerContourHatchPickerToggle => {
                self.aec.styles.plan_contour_hatch_picker_open =
                    !self.aec.styles.plan_contour_hatch_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerContourHatchColorPickerToggle => {
                self.aec.styles.plan_contour_hatch_color_picker_open =
                    !self.aec.styles.plan_contour_hatch_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayLayerVis2d(visible) => {
                if let (Some(style_id), Some(layer_id)) = (
                    self.aec.styles.plan_overlay_style_id.clone(),
                    self.aec.styles.plan_overlay_layer_id,
                ) {
                    let overlay = self
                        .aec.styles.plan_style_overlays
                        .entry(style_id)
                        .or_default();
                    let mut vis = overlay
                        .layer_visibility
                        .get(&layer_id)
                        .copied()
                        .unwrap_or_default();
                    vis.visible_2d = visible;
                    if vis.visible_2d && vis.visible_3d {
                        overlay.layer_visibility.remove(&layer_id);
                    } else {
                        overlay.layer_visibility.insert(layer_id, vis);
                    }
                }
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayLayerVis3d(visible) => {
                if let (Some(style_id), Some(layer_id)) = (
                    self.aec.styles.plan_overlay_style_id.clone(),
                    self.aec.styles.plan_overlay_layer_id,
                ) {
                    let overlay = self
                        .aec.styles.plan_style_overlays
                        .entry(style_id)
                        .or_default();
                    let mut vis = overlay
                        .layer_visibility
                        .get(&layer_id)
                        .copied()
                        .unwrap_or_default();
                    vis.visible_3d = visible;
                    if vis.visible_2d && vis.visible_3d {
                        overlay.layer_visibility.remove(&layer_id);
                    } else {
                        overlay.layer_visibility.insert(layer_id, vis);
                    }
                }
                Task::none()
            }
            AecMessage::AecPlanManagerSave => {
                self.aec_plan_manager_save_internal();
                Task::none()
            }
            AecMessage::AecPlanManagerApply => {
                if let Some(name) = self.aec_plan_manager_save_internal() {
                    let i = self.active_tab;
                    let (active_name, _) = self.aec_tab_display_state(i);
                    if !self.tabs[i].is_start
                        && active_name.as_deref() == Some(name.as_str())
                    {
                        self.apply_active_display_config_to_tab(i);
                    }
                }
                Task::none()
            }
            AecMessage::AecActiveDisplayConfigSelected(name) => {
                let i = self.active_tab;
                if self.tabs[i].is_start {
                    return Task::none();
                }
                self.aec_set_active_display_config(i, name.clone());
                self.apply_active_display_config_to_tab(i);
                Task::none()
            }
            AecMessage::AecRepresentationOverrideSelected(mode) => {
                let i = self.active_tab;
                if self.tabs[i].is_start {
                    return Task::none();
                }
                self.aec_set_representation_override(i, mode);
                self.apply_active_display_config_to_tab(i);
                Task::none()
            }
            _ => Task::none(),
        }
    }
}
