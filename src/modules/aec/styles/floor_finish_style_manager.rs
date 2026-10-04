//! `AEC_FLOORFINISHSTYLEMANAGER` ribbon tool and manager dispatch.
//!
//! Form draft / layer-buffer helpers live here so `update.rs` stays thin and
//! the 2D/3D preview can bake geometry without the CAD viewport.

use iced::Task;

use crate::app::{AecModalKind, Message, OpenCADStudio};
use crate::modules::aec::engine::library::{
    combined_floor_finish_style_entries_with_session, load_or_seed,
};
use crate::modules::aec::engine::slab_style::{
    FloorFinishStyle, LayerFunction,
};
use crate::modules::aec::engine::slab_xdata::layer_function_to_str;
use crate::modules::aec::engine::style::Style;
use crate::modules::aec::state::AecSlabLayerBuffer;
use crate::modules::aec::styles::slab_style_manager::{
    buffer_to_layer, layer_to_buffer,
};
use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_FLOORFINISHSTYLEMANAGER",
        label: "Floor Finish Style Manager",
        icon: IconKind::Svg(include_bytes!(
            "../../../../assets/icons/aec/material_manager.svg"
        )),
        event: ModuleEvent::Command("AEC_FLOORFINISHSTYLEMANAGER".to_string()),
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["AEC_FLOORFINISHSTYLEMANAGER"],
});

impl OpenCADStudio {
    pub(crate) fn aec_floor_finish_style_manager_open(&mut self) -> Task<Message> {
        self.aec_refresh_combined_style_library();
        let lib = self.aec.aec_style_library.clone().unwrap_or_else(load_or_seed);

        let entries = combined_floor_finish_style_entries_with_session(
            self.aec.aec_project_explorer_file.as_ref(),
            self.aec.aec_session_style_library.as_ref(),
        );

        let select_id = self
            .aec
            .aec_last_floor_finish_style_id
            .clone()
            .or_else(|| entries.first().map(|e| e.finish_style.style.id.clone()))
            .or_else(|| lib.floor_finish_styles.first().map(|s| s.style.id.clone()));

        self.aec.aec_floor_finish_style_manager_filter.clear();
        self.aec.aec_floor_finish_style_manager_editing_id = None;
        self.aec.aec_floor_finish_style_manager_form_open = false;

        if let Some(id) = select_id {
            let _ = self.aec_floor_finish_style_manager_select(id);
        } else {
            let _ = self.aec_floor_finish_style_manager_new();
        }

        self.active_modal = Some(crate::app::ModalKind::Aec(
            AecModalKind::FloorFinishStyleManager,
        ));
        Task::none()
    }

    pub(crate) fn aec_floor_finish_style_manager_select(&mut self, id: String) -> Task<Message> {
        let entries = combined_floor_finish_style_entries_with_session(
            self.aec.aec_project_explorer_file.as_ref(),
            self.aec.aec_session_style_library.as_ref(),
        );
        let style = entries
            .iter()
            .find(|e| e.finish_style.style.id == id)
            .map(|e| e.finish_style.clone())
            .or_else(|| {
                self.aec
                    .aec_style_library
                    .as_ref()
                    .and_then(|lib| lib.find_floor_finish_style(&id).cloned())
            });

        if let Some(style) = style {
            self.aec.aec_floor_finish_style_manager_selected = Some(style.style.id.clone());
            self.aec.aec_floor_finish_style_manager_editing_id = Some(style.style.id.clone());
            self.aec.aec_last_floor_finish_style_id = Some(style.style.id.clone());
            self.aec.aec_floor_finish_style_manager_name = style.style.name.clone();
            self.aec.aec_floor_finish_style_manager_parent = style.style.parent_style_id.clone();
            self.aec.aec_floor_finish_style_manager_layers =
                style.layers.iter().map(layer_to_buffer).collect();
            self.aec.aec_floor_finish_style_manager_form_open = true;
            self.aec.aec_floor_finish_style_manager_drag_index = None;
        }

        Task::none()
    }

    pub(crate) fn aec_floor_finish_style_manager_new(&mut self) -> Task<Message> {
        let first_mat = self
            .aec
            .aec_style_library
            .as_ref()
            .and_then(|l| l.materials.first().map(|m| m.id.clone()))
            .unwrap_or_else(|| "Parkett Eiche".to_string());

        self.aec.aec_floor_finish_style_manager_selected = None;
        self.aec.aec_floor_finish_style_manager_editing_id = None;
        self.aec.aec_floor_finish_style_manager_name =
            crate::t!("New Floor Finish Style").to_string();
        self.aec.aec_floor_finish_style_manager_parent = None;
        self.aec.aec_floor_finish_style_manager_layers = vec![
            AecSlabLayerBuffer {
                material_id: first_mat,
                thickness: "1.5".to_string(),
                function: layer_function_to_str(&LayerFunction::Finish),
                vertical_offset: "0.0".to_string(),
                layer_override: String::new(),
                hatch_override: "ANSI31".to_string(),
                role_tag: String::new(),
                layer_id: Some(uuid::Uuid::new_v4()),
            },
            AecSlabLayerBuffer {
                material_id: "Zementestrich CT-C25-F4".to_string(),
                thickness: "4.5".to_string(),
                function: layer_function_to_str(&LayerFunction::Other("Screed".to_string())),
                vertical_offset: "0.0".to_string(),
                layer_override: String::new(),
                hatch_override: "AR-CONC".to_string(),
                role_tag: String::new(),
                layer_id: Some(uuid::Uuid::new_v4()),
            },
            AecSlabLayerBuffer {
                material_id: "Trittschalldämmung EPS-T".to_string(),
                thickness: "2.0".to_string(),
                function: layer_function_to_str(&LayerFunction::Insulation),
                vertical_offset: "0.0".to_string(),
                layer_override: String::new(),
                hatch_override: String::new(),
                role_tag: String::new(),
                layer_id: Some(uuid::Uuid::new_v4()),
            },
        ];
        self.aec.aec_floor_finish_style_manager_form_open = true;
        self.aec.aec_floor_finish_style_manager_drag_index = None;

        Task::none()
    }

    pub(crate) fn aec_floor_finish_style_manager_duplicate(&mut self) -> Task<Message> {
        let current_layers = self.aec.aec_floor_finish_style_manager_layers.clone();
        let current_name = self.aec.aec_floor_finish_style_manager_name.clone();
        let current_parent = self.aec.aec_floor_finish_style_manager_parent.clone();

        self.aec.aec_floor_finish_style_manager_selected = None;
        self.aec.aec_floor_finish_style_manager_editing_id = None;
        self.aec.aec_floor_finish_style_manager_name = format!("{current_name} (Kopie)");
        self.aec.aec_floor_finish_style_manager_parent = current_parent;
        self.aec.aec_floor_finish_style_manager_layers = current_layers
            .into_iter()
            .map(|mut l| {
                l.layer_id = Some(uuid::Uuid::new_v4());
                l
            })
            .collect();
        self.aec.aec_floor_finish_style_manager_form_open = true;
        self.aec.aec_floor_finish_style_manager_drag_index = None;

        Task::none()
    }

    pub(crate) fn aec_floor_finish_style_manager_layer_add(&mut self) {
        let first_mat = self
            .aec
            .aec_style_library
            .as_ref()
            .and_then(|l| l.materials.first().map(|m| m.id.clone()))
            .unwrap_or_else(|| "Fliesen Feinsteinzeug".to_string());

        self.aec
            .aec_floor_finish_style_manager_layers
            .push(AecSlabLayerBuffer {
                material_id: first_mat,
                thickness: "1.5".to_string(),
                function: layer_function_to_str(&LayerFunction::Finish),
                vertical_offset: "0.0".to_string(),
                layer_override: String::new(),
                hatch_override: String::new(),
                role_tag: String::new(),
                layer_id: Some(uuid::Uuid::new_v4()),
            });
    }

    pub(crate) fn aec_floor_finish_style_manager_layer_remove(&mut self, index: usize) {
        if index < self.aec.aec_floor_finish_style_manager_layers.len() {
            self.aec.aec_floor_finish_style_manager_layers.remove(index);
        }
    }

    pub(crate) fn aec_floor_finish_style_manager_layer_move_up(&mut self, index: usize) {
        if index > 0 && index < self.aec.aec_floor_finish_style_manager_layers.len() {
            self.aec
                .aec_floor_finish_style_manager_layers
                .swap(index, index - 1);
        }
    }

    pub(crate) fn aec_floor_finish_style_manager_layer_move_down(&mut self, index: usize) {
        if index + 1 < self.aec.aec_floor_finish_style_manager_layers.len() {
            self.aec
                .aec_floor_finish_style_manager_layers
                .swap(index, index + 1);
        }
    }

    pub(crate) fn aec_floor_finish_style_manager_save_internal(
        &mut self,
    ) -> Result<FloorFinishStyle, String> {
        let name = self.aec.aec_floor_finish_style_manager_name.trim().to_string();
        if name.is_empty() {
            return Err("Floor finish style name cannot be empty".to_string());
        }

        let mut layers = Vec::new();
        for (i, buf) in self.aec.aec_floor_finish_style_manager_layers.iter().enumerate() {
            let layer = buffer_to_layer(buf).map_err(|e| format!("Layer {}: {}", i + 1, e))?;
            layers.push(layer);
        }

        let id = self
            .aec
            .aec_floor_finish_style_manager_editing_id
            .clone()
            .unwrap_or_else(|| {
                name.to_lowercase()
                    .replace(' ', "_")
                    .replace('/', "_")
                    .replace('\\', "_")
            });

        let finish_style = FloorFinishStyle {
            style: Style {
                id: id.clone(),
                name: name.clone(),
                object_kind: "FloorFinish".to_string(),
                parent_style_id: self.aec.aec_floor_finish_style_manager_parent.clone(),
            },
            layers,
            display_profiles: std::collections::HashMap::new(),
        };

        // Self-parent validation
        if finish_style.style.parent_style_id.as_deref() == Some(id.as_str()) {
            return Err("Style cannot be its own parent".to_string());
        }

        // Validate cycle in hierarchy
        if let Some(parent_id) = &finish_style.style.parent_style_id {
            if let Some(lib) = &self.aec.aec_style_library {
                let mut visited = vec![id.clone()];
                let mut curr = Some(parent_id.clone());
                while let Some(c) = curr {
                    if visited.contains(&c) {
                        return Err(format!("Inheritance cycle detected involving '{c}'"));
                    }
                    visited.push(c.clone());
                    curr = lib.find_floor_finish_style(&c).and_then(|s| s.style.parent_style_id.clone());
                }
            }
        }

        // Upsert into project or session
        let has_project = self.aec.aec_project_explorer_file.is_some();
        if has_project {
            self.aec_upsert_floor_finish_style_into_project(finish_style.clone())?;
        } else {
            self.aec_upsert_floor_finish_style_into_session(finish_style.clone());
        }

        self.aec.aec_floor_finish_style_manager_selected = Some(id.clone());
        self.aec.aec_floor_finish_style_manager_editing_id = Some(id.clone());
        self.aec.aec_last_floor_finish_style_id = Some(id);

        Ok(finish_style)
    }

    pub(crate) fn aec_floor_finish_style_manager_save(&mut self) -> Task<Message> {
        match self.aec_floor_finish_style_manager_save_internal() {
            Ok(style) => {
                self.command_line.push_info(
                    crate::tf!(
                        "AEC Floor Finish Style Manager: Saved style '{}'.",
                        style.style.name
                    )
                    .as_ref(),
                );
            }
            Err(e) => {
                self.command_line.push_error(
                    crate::tf!("AEC Floor Finish Style Manager: Failed to save: {e}").as_ref(),
                );
            }
        }
        Task::none()
    }

    pub(crate) fn aec_floor_finish_style_manager_save_and_apply(&mut self) -> Task<Message> {
        match self.aec_floor_finish_style_manager_save_internal() {
            Ok(style) => {
                // Regenerate rooms using this style
                let tab = self.active_tab;
                let room_handles: Vec<acadrust::Handle> = self.tabs.get(tab).map_or_else(
                    Vec::new,
                    |tab_data| {
                        tab_data
                            .scene
                            .document
                            .entities()
                            .filter_map(|e| {
                                let handle = e.common().handle;
                                let room =
                                    crate::modules::aec::engine::room_xdata::room_from_entity(e)?;
                                if room.finish_style_id.as_deref() == Some(style.style.id.as_str()) {
                                    Some(handle)
                                } else {
                                    None
                                }
                            })
                            .collect()
                    },
                );

                let mut count = 0;
                let finishes = style.to_room_finishes();
                for handle in room_handles {
                    if let Some(tab_data) = self.tabs.get_mut(tab) {
                        let doc = &mut tab_data.scene.document;
                        if let Some(entity) = doc.get_entity_mut(handle) {
                            if let Some(mut room) =
                                crate::modules::aec::engine::room_xdata::room_from_entity(entity)
                            {
                                room.floor_finish = Some(finishes.clone());
                                crate::modules::aec::engine::room_xdata::write_room_record(
                                    doc, handle, &room,
                                );
                            }
                        }
                        crate::modules::aec::engine::room_regen::regenerate_room_representation(
                            &mut tab_data.scene, handle, None,
                        );
                        count += 1;
                    }
                }

                if count > 0 {
                    if let Some(tab_data) = self.tabs.get_mut(tab) {
                        tab_data.dirty = true;
                        crate::modules::aec::rooms::schedule::update_all_room_schedules(
                            &mut tab_data.scene,
                        );
                    }
                    self.command_line.push_info(
                        crate::tf!(
                            "AEC Floor Finish Style Manager: Saved and updated {count} room(s)."
                        )
                        .as_ref(),
                    );
                } else {
                    self.command_line.push_info(
                        crate::tf!(
                            "AEC Floor Finish Style Manager: Saved style '{}'.",
                            style.style.name
                        )
                        .as_ref(),
                    );
                }
            }
            Err(e) => {
                self.command_line.push_error(
                    crate::tf!("AEC Floor Finish Style Manager: Failed to save: {e}").as_ref(),
                );
            }
        }
        Task::none()
    }

    pub(crate) fn aec_floor_finish_style_manager_delete(&mut self) -> Task<Message> {
        let Some(selected) = self.aec.aec_floor_finish_style_manager_selected.clone() else {
            return Task::none();
        };

        let has_project = self.aec.aec_project_explorer_file.is_some();
        let mut removed = false;

        if let Some(session_lib) = self.tabs[self.active_tab]
            .aec_session_style_library_mut()
            .as_mut()
        {
            if session_lib.remove_floor_finish_style(&selected) {
                removed = true;
            }
        }

        if has_project {
            if let Some(project) = self.aec.aec_project_explorer_file.as_mut() {
                if project
                    .material_wall_style_library
                    .remove_floor_finish_style(&selected)
                {
                    removed = true;
                    if let Some(path) = self.aec.aec_project_explorer_path.clone() {
                        let lib = project.material_wall_style_library.clone();
                        let _ = crate::modules::aec::engine::project::save_style_library_to_project(
                            project, &path, lib,
                        );
                    }
                }
            }
        }

        if removed {
            self.aec_refresh_combined_style_library();
            self.command_line.push_info(
                crate::tf!(
                    "AEC Floor Finish Style Manager: Deleted style '{selected}'."
                )
                .as_ref(),
            );
            self.aec.aec_floor_finish_style_manager_selected = None;
            self.aec.aec_floor_finish_style_manager_editing_id = None;
            self.aec.aec_floor_finish_style_manager_form_open = false;
        } else {
            self.command_line.push_error(
                crate::tf!(
                    "AEC Floor Finish Style Manager: Cannot delete standard or non-existent style '{selected}'."
                )
                .as_ref(),
            );
        }

        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::slab_style::SlabStyleLayer;

    #[test]
    fn test_floor_finish_style_creation_and_defaults() {
        let tool_def = tool();
        assert_eq!(tool_def.id, "AEC_FLOORFINISHSTYLEMANAGER");

        let style = FloorFinishStyle::new("finish_tiles_60", "Fliesen 60mm").with_layers(vec![
            SlabStyleLayer::new(
                "mat_tiles",
                crate::modules::aec::engine::slab_style::LayerValue::Fixed(0.015),
                LayerFunction::Finish,
            )
            .with_hatch(Some("SQUARE".to_string())),
            SlabStyleLayer::new(
                "mat_screed",
                crate::modules::aec::engine::slab_style::LayerValue::Fixed(0.045),
                LayerFunction::Other("Screed".to_string()),
            ),
        ]);

        assert_eq!(style.style.name, "Fliesen 60mm");
        assert_eq!(style.layers.len(), 2);
        assert!((style.nominal_thickness() - 0.06).abs() < 1e-6);

        let finishes = style.to_room_finishes();
        assert_eq!(finishes.len(), 2);
        assert_eq!(finishes[0].material, "mat_tiles");
        assert_eq!(finishes[0].hatch_pattern.as_deref(), Some("SQUARE"));
        assert!((finishes[0].thickness - 0.015).abs() < 1e-6);
        assert!((finishes[1].vertical_offset - 0.015).abs() < 1e-6);
    }
}
