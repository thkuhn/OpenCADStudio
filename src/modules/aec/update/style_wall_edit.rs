//! AEC update handlers: Style manager: wall style editing and layer edits.

use iced::Task;

use crate::app::{Message, OpenCADStudio};
use crate::modules::aec::message::AecMessage;

impl OpenCADStudio {
    pub(crate) fn update_aec_style_wall_edit(&mut self, msg: AecMessage) -> Task<Message> {
        match msg {
            AecMessage::AecStyleManagerFilter(value) => {
                self.aec.styles.filter = value;
                Task::none()
            }
            AecMessage::AecStyleManagerSelectMaterial(id) => {
                let material = crate::modules::aec::engine::library::combined_material_entries(
                    self.aec.aec_project_explorer_file.as_ref(),
                )
                .into_iter()
                .find(|e| e.material.id == id)
                .map(|e| e.material)
                .or_else(|| {
                    self.aec.aec_style_library
                        .as_ref()
                        .and_then(|lib| lib.materials.iter().find(|m| m.id == id).cloned())
                });
                if let Some(material) = material {
                    self.aec.styles.material_editing_id = Some(material.id.clone());
                    self.aec.styles.material_name = material.name.clone();
                    self.aec.styles.material_hatch = material.hatch_pattern.clone();
                    self.aec.styles.material_color =
                        format!("#{:06X}", material.line_color);
                    self.aec.styles.material_line_type = material.line_type.clone();
                    self.aec.styles.material_category =
                        material.group.clone().unwrap_or_default();
                                        self.aec.styles.material_hatch_color = material
                        .hatch_color
                        .and_then(|c| c.rgb())
                        .map(|(r, g, b)| ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
                        .unwrap_or(material.line_color);
                    self.aec.styles.material_hatch_scale =
                        format!("{}", material.hatch_scale);
                    self.aec.styles.material_render_ref =
                        material.render_material_ref.clone().unwrap_or_default();
                    self.aec.styles.material_hatch_angle =
                        format!("{}", material.hatch_angle);
                    self.aec.styles.material_hatch_angle_relative =
                        material.hatch_angle_relative;
                    self.aec.styles.material_hatch_color_picker_open = false;
                    self.aec.styles.material_form_open = true;
                }
                self.aec.styles.selected_material = Some(id);
                self.aec.styles.selected_wall_style = None;
                self.refresh_aec_material_linetype_combo();
                Task::none()
            }
            AecMessage::AecStyleManagerSelectWallStyle(id) => {
                self.refresh_aec_material_linetype_combo();
                let wall_style = crate::modules::aec::engine::library::combined_wall_style_entries(
                    self.aec.aec_project_explorer_file.as_ref(),
                )
                .into_iter()
                .find(|e| e.wall_style.style.id == id)
                .map(|e| e.wall_style)
                .or_else(|| {
                    self.aec.aec_style_library.as_ref().and_then(|lib| {
                        lib.wall_styles
                            .iter()
                            .find(|w| w.style.id == id)
                            .cloned()
                    })
                });
                if let Some(wall_style) = wall_style {
                    self.aec.styles.wall_style_editing_id = Some(wall_style.style.id.clone());
                    self.aec.styles.wall_duplicate_default_profile = None;
                    self.aec.styles.wall_style_name = wall_style.style.name.clone();
                    self.aec.styles.wall_style_parent =
                        wall_style.style.parent.clone();
                    self.aec.styles.wall_style_layers = wall_style
                        .layers
                        .iter()
                        .map(|l| crate::app::AecLayerBuffer {
                            material_id: l.material_id.clone(),
                            // Display in centimeters; fixed numbers and
                            // formula strings alike.
                            thickness: l.thickness.to_cm_display_string(),
                            function: match &l.function {
                                crate::modules::aec::engine::wall_style::LayerFunction::Structural => {
                                    "Structural".to_string()
                                }
                                crate::modules::aec::engine::wall_style::LayerFunction::Insulation => {
                                    "Insulation".to_string()
                                }
                                crate::modules::aec::engine::wall_style::LayerFunction::Finish => {
                                    "Finish".to_string()
                                }
                                crate::modules::aec::engine::wall_style::LayerFunction::Other(s) => s.clone(),
                            },
                            axis_offset: l.axis_offset.to_cm_display_string(),
                            bottom_offset: crate::modules::aec::engine::wall_style::LayerValue::format_cm(
                                l.bottom_offset * 100.0,
                            ),
                            top_offset: crate::modules::aec::engine::wall_style::LayerValue::format_cm(
                                l.top_offset * 100.0,
                            ),
                            layer_override: l.layer_override.clone().unwrap_or_default(),
                            hatch_override: l.hatch_override.clone().unwrap_or_default(),
                            role_tag: l.role_tag.clone().unwrap_or_default(),
                            layer_id: Some(l.layer_id),
                        })
                        .collect();
                    self.aec.styles.wall_style_form_open = true;
                }
                self.aec.styles.selected_wall_style = Some(id);
                self.aec.styles.selected_material = None;
                self.aec.styles.material_form_open = false;
                self.aec.styles.material_editing_id = None;
                self.aec.styles.profile_selected = None;
                self.aec.styles.profile_contour_explicit = false;
                self.aec.styles.profile_contour_selection.clear();
                self.aec.styles.profile_solid_explicit = false;
                self.aec.styles.profile_solid_selection.clear();
                self.aec.styles.profile_hatch_angle.clear();
                self.aec.styles.profile_hatch_relative = false;
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleNew => {
                self.aec.styles.selected_material = None;
                self.aec.styles.selected_wall_style = None;
                self.aec.styles.wall_style_editing_id = None;
                self.aec.styles.wall_duplicate_default_profile = None;
                self.aec.styles.wall_style_name.clear();
                self.aec.styles.wall_style_parent = None;
                self.aec.styles.wall_style_layers.clear();
                self.aec.styles.wall_style_form_open = true;
                self.aec.styles.material_form_open = false;
                self.aec.styles.material_editing_id = None;
                self.aec.styles.profile_selected = None;
                self.aec.styles.profile_contour_explicit = false;
                self.aec.styles.profile_contour_selection.clear();
                self.aec.styles.profile_solid_explicit = false;
                self.aec.styles.profile_solid_selection.clear();
                self.aec.styles.profile_hatch_angle.clear();
                self.aec.styles.profile_hatch_relative = false;
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleNameChanged(value) => {
                self.aec.styles.wall_style_name = value;
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleParentChanged(value) => {
                self.aec.styles.wall_style_parent = value;
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerAdd => {
                let material_id = self
                    .aec.aec_style_library
                    .as_ref()
                    .and_then(|lib| lib.materials.first())
                    .map(|m| m.id.clone())
                    .unwrap_or_default();
                self.aec.styles.wall_style_layers
                    .push(crate::app::AecLayerBuffer {
                        material_id,
                        thickness: "10".to_string(),
                        function: "Structural".to_string(),
                        axis_offset: "0".to_string(),
                        bottom_offset: "0".to_string(),
                        top_offset: "0".to_string(),
                        layer_override: String::new(),
                        hatch_override: String::new(),
                        role_tag: String::new(),
                        layer_id: Some(uuid::Uuid::new_v4()),
                    });
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerRemove(index) => {
                if index < self.aec.styles.wall_style_layers.len() {
                    self.aec.styles.wall_style_layers.remove(index);
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerMaterialChanged(index, material_id) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.material_id = material_id;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerThicknessChanged(index, thickness) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.thickness = thickness;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerFunctionChanged(index, function) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.function = function;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerAxisOffsetChanged(index, offset) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.axis_offset = offset;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerBottomOffsetChanged(index, offset) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.bottom_offset = offset;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerTopOffsetChanged(index, offset) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.top_offset = offset;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerOverrideChanged(index, layer_name) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.layer_override = layer_name;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerHatchOverrideChanged(index, hatch) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.hatch_override = hatch;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerHatchPickerToggle(index) => {
                let open = &mut self.aec.styles.wall_layer_hatch_picker;
                *open = if *open == Some(index) { None } else { Some(index) };
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerHatchSelected(index, hatch) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.hatch_override = hatch;
                }
                self.aec.styles.wall_layer_hatch_picker = None;
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerRoleTagChanged(index, role_tag) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.role_tag = role_tag;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerMoveUp(index) => {
                if index > 0 && index < self.aec.styles.wall_style_layers.len() {
                    self.aec.styles.wall_style_layers.swap(index - 1, index);
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerMoveDown(index) => {
                if index + 1 < self.aec.styles.wall_style_layers.len() {
                    self.aec.styles.wall_style_layers.swap(index, index + 1);
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerDragStart(index) => {
                // Click-based "pick up / drop here" reordering: arms the
                // dragged row so a later drag-over on another row swaps it
                // into place. Clicking the already-armed row's handle again
                // cancels the drag (matches the toggle feel of `PaneMoveStart`).
                if self.aec.styles.wall_style_drag_index == Some(index) {
                    self.aec.styles.wall_style_drag_index = None;
                } else if index < self.aec.styles.wall_style_layers.len() {
                    self.aec.styles.wall_style_drag_index = Some(index);
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerDragOver(target) => {
                if let Some(from) = self.aec.styles.wall_style_drag_index.take() {
                    let len = self.aec.styles.wall_style_layers.len();
                    if from != target && from < len && target < len {
                        let layer = self.aec.styles.wall_style_layers.remove(from);
                        self.aec.styles.wall_style_layers.insert(target, layer);
                    }
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerDragEnd => {
                self.aec.styles.wall_style_drag_index = None;
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleSortToggle => {
                self.aec.styles.wall_style_sort =
                    self.aec.styles.wall_style_sort.toggled();
                Task::none()
            }
            _ => Task::none(),
        }
    }
}
