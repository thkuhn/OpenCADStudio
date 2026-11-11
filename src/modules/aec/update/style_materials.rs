//! AEC update handlers: Style manager: materials and copy-to-project/global.

use iced::Task;

use crate::app::{AecModalKind, Message, OpenCADStudio};
use crate::modules::aec::message::AecMessage;

impl OpenCADStudio {
    pub(crate) fn update_aec_style_materials(&mut self, msg: AecMessage) -> Task<Message> {
        match msg {
            AecMessage::AecStyleManagerMaterialNew => {
                self.aec.styles.selected_material = None;
                self.aec.styles.selected_wall_style = None;
                self.aec.styles.material_editing_id = None;
                self.aec.styles.material_name.clear();
                self.aec.styles.material_hatch.clear();
                self.aec.styles.material_color = "#FFFFFF".to_string();
                self.aec.styles.material_line_type = "Continuous".to_string();
                self.aec.styles.material_category.clear();
                self.aec.styles.material_hatch_color = 0xFFFFFF;
                self.aec.styles.material_hatch_scale = "1.0".to_string();
                self.aec.styles.material_render_ref.clear();
                self.aec.styles.material_hatch_angle = "0.0".to_string();
                self.aec.styles.material_hatch_angle_relative = true;
                self.aec.styles.material_hatch_color_picker_open = false;
                self.aec.styles.material_form_open = true;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialNameChanged(value) => {
                self.aec.styles.material_name = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchChanged(value) => {
                self.aec.styles.material_hatch = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchPickerToggle => {
                self.aec.styles.material_hatch_picker_open =
                    !self.aec.styles.material_hatch_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchSelected(name) => {
                self.aec.styles.material_hatch = name;
                self.aec.styles.material_hatch_picker_open = false;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialColorChanged(value) => {
                self.aec.styles.material_color = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialLineTypeChanged(value) => {
                self.ensure_aec_catalog_linetype(&value);
                self.aec.styles.material_line_type = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialColorPickerToggle => {
                self.aec.styles.material_color_picker_open =
                    !self.aec.styles.material_color_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialColorPicked(color) => {
                self.aec.styles.material_color_picker_open = false;
                let [r, g, b, _] = match color {
                    acadrust::types::Color::Rgb { r, g, b } => [r, g, b, 255u8],
                    acadrust::types::Color::Index(i) => {
                        let (r, g, b) = acadrust::types::aci_table::aci_to_rgb(i)
                            .unwrap_or((255, 255, 255));
                        [r, g, b, 255]
                    }
                    _ => [255, 255, 255, 255],
                };
                self.aec.styles.material_color = format!("#{r:02X}{g:02X}{b:02X}");
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialCategoryChanged(value) => {
                self.aec.styles.material_category = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchColorChanged(value) => {
                self.aec.styles.material_hatch_color = value;
                self.aec.styles.material_hatch_color_picker_open = false;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchColorPickerToggle => {
                self.aec.styles.material_hatch_color_picker_open =
                    !self.aec.styles.material_hatch_color_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchScaleChanged(value) => {
                self.aec.styles.material_hatch_scale = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialRenderRefChanged(value) => {
                self.aec.styles.material_render_ref = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchAngleChanged(value) => {
                self.aec.styles.material_hatch_angle = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchAngleRelativeToggle => {
                self.aec.styles.material_hatch_angle_relative =
                    !self.aec.styles.material_hatch_angle_relative;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialDuplicate => {
                let source_id = self
                    .aec.styles.material_editing_id
                    .clone()
                    .or_else(|| self.aec.styles.selected_material.clone());
                let Some(source_id) = source_id else {
                    return Task::none();
                };
                let Some(material) = self
                    .aec.aec_style_library
                    .as_ref()
                    .and_then(|lib| lib.materials.iter().find(|m| m.id == source_id))
                    .cloned()
                else {
                    return Task::none();
                };
                let existing_names: Vec<String> = self
                    .aec.aec_style_library
                    .as_ref()
                    .map(|lib| lib.materials.iter().map(|m| m.name.clone()).collect())
                    .unwrap_or_default();
                let base_name = format!("{} (Kopie)", material.name);
                let mut name = base_name.clone();
                let mut counter = 2;
                while existing_names.iter().any(|n| n == &name) {
                    name = format!("{base_name} {counter}");
                    counter += 1;
                }
                self.aec.styles.selected_material = None;
                self.aec.styles.selected_wall_style = None;
                self.aec.styles.material_editing_id = None;
                self.aec.styles.material_name = name;
                self.aec.styles.material_hatch = material.hatch_pattern;
                self.aec.styles.material_color =
                    format!("#{:06X}", material.line_color);
                self.aec.styles.material_line_type = material.line_type;
                self.aec.styles.material_category =
                    material.group.unwrap_or_default();
                                self.aec.styles.material_hatch_color = material
                    .hatch_color
                    .and_then(|c| c.rgb())
                    .map(|(r, g, b)| ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
                    .unwrap_or(material.line_color);
                self.aec.styles.material_hatch_scale =
                    format!("{}", material.hatch_scale);
                self.aec.styles.material_render_ref =
                    material.render_material_ref.unwrap_or_default();
                self.aec.styles.material_hatch_angle =
                    format!("{}", material.hatch_angle);
                self.aec.styles.material_hatch_angle_relative =
                    material.hatch_angle_relative;
                self.aec.styles.material_color_picker_open = false;
                self.aec.styles.material_hatch_color_picker_open = false;
                self.aec.styles.material_hatch_picker_open = false;
                self.aec.styles.material_form_open = true;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialSave => {
                self.aec_style_manager_material_save_internal();
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialSaveAndApply => {
                if let Some((mat_id, _lib)) = self.aec_style_manager_material_save_internal() {
                    let tab_index = self.active_tab;
                    let mut affected_walls = Vec::new();
                    for entity in self.tabs[tab_index].scene.document.entities() {
                        if let Some(wall) =
                            crate::modules::aec::engine::xdata::wall_from_entity(entity)
                        {
                            if wall.layers.iter().any(|l| l.material == mat_id) {
                                affected_walls.push(entity.common().handle);
                            }
                        }
                    }
                    let mut updated_count = 0;
                    for handle in affected_walls {
                        if self
                            .regenerate_wall_respecting_active_display_config(tab_index, handle)
                            .is_ok()
                        {
                            updated_count += 1;
                        }
                    }
                    if updated_count > 0 {
                        self.tabs[tab_index].scene.bump_geometry();
                        self.command_line.push_info(
                            crate::tf!(
                                "AEC Style Manager: updated {updated_count} wall(s) using this material."
                            )
                            .as_ref(),
                        );
                    }
                }
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialDelete => {
                if let Some(id) = self.aec.styles.selected_material.clone() {
                    let lib_snapshot = if let Some(lib) = self.aec.aec_style_library.as_mut() {
                        lib.remove_material(&id);
                        Some(lib.clone())
                    } else {
                        None
                    };
                    if let Some(lib_snapshot) = lib_snapshot {
                        match self.aec_save_style_library_preferring_project(&lib_snapshot) {
                            Ok(()) => self.command_line.push_info(
                                crate::t!("AEC Style Manager: material deleted.").as_ref(),
                            ),
                            Err(e) => self.command_line.push_error(
                                crate::tf!("AEC Style Manager: failed to save library: {e}")
                                    .as_ref(),
                            ),
                        }
                    }
                }
                self.aec.styles.selected_material = None;
                self.aec.styles.material_editing_id = None;
                self.aec.styles.material_form_open = false;
                Task::none()
            }
            AecMessage::AecStyleManagerCopyMaterialToProject => self.aec_handle_copy_material(true),
            AecMessage::AecStyleManagerCopyMaterialToGlobal => self.aec_handle_copy_material(false),
            AecMessage::AecStyleManagerCopyWallStyleToProject => self.aec_handle_copy_wall_style(true),
            AecMessage::AecStyleManagerCopyWallStyleToGlobal => self.aec_handle_copy_wall_style(false),
            AecMessage::AecStyleManagerCopyConflictConfirm(confirmed) => {
                self.aec.styles.copy_conflict_open = false;
                let return_modal = crate::app::ModalKind::Aec(AecModalKind::Styles);
                if confirmed {
                    if let Some(pending) = self.aec.styles.pending_copy.take() {
                        self.aec_execute_copy(pending);
                    }
                } else {
                    self.aec.styles.pending_copy = None;
                }
                self.active_modal = Some(return_modal);
                Task::none()
            }
            _ => Task::none(),
        }
    }
}
