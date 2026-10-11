//! `AEC_SLABSTRUCTURALSTYLEMANAGER` ribbon tool and manager dispatch.
//!
//! Form draft / layer-buffer helpers live here so `update.rs` stays thin and
//! the 2D/3D preview can bake geometry without the CAD viewport.

use iced::Task;

use crate::app::{AecModalKind, Message, OpenCADStudio};
use crate::modules::aec::engine::library::{
    combined_slab_structural_style_entries_with_session, load_or_seed,
};
use crate::modules::aec::engine::slab_style::{
    LayerFunction, SlabStructuralStyle,
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
        id: "AEC_SLABSTRUCTURALSTYLEMANAGER",
        label: "Slab Shell",
        icon: IconKind::Svg(include_bytes!(
            "../../../../assets/icons/aec/style_floor_finish.svg"
        )),
        event: ModuleEvent::Command("AEC_SLABSTRUCTURALSTYLEMANAGER".to_string()),
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["AEC_SLABSTRUCTURALSTYLEMANAGER"],
});

impl OpenCADStudio {
    pub(crate) fn aec_slab_structural_style_manager_open(&mut self) -> Task<Message> {
        self.aec_refresh_combined_style_library();
        let lib = self.aec.aec_style_library.clone().unwrap_or_else(load_or_seed);

        let entries = combined_slab_structural_style_entries_with_session(
            self.aec.aec_project_explorer_file.as_ref(),
            self.aec.aec_session_style_library.as_ref(),
        );

        let select_id = self
            .aec
            .aec_last_slab_structural_style_id
            .clone()
            .or_else(|| entries.first().map(|e| e.structural_style.style.id.clone()))
            .or_else(|| lib.slab_structural_styles.first().map(|s| s.style.id.clone()));

        self.aec.styles.slab_structural_filter.clear();
        self.aec.styles.slab_structural_editing_id = None;
        self.aec.styles.slab_structural_form_open = false;

        if let Some(id) = select_id {
            let _ = self.aec_slab_structural_style_manager_select(id);
        } else {
            let _ = self.aec_slab_structural_style_manager_new();
        }

        self.active_modal = Some(crate::app::ModalKind::Aec(
            AecModalKind::Styles,
        ));
        Task::none()
    }

    pub(crate) fn aec_slab_structural_style_manager_select(&mut self, id: String) -> Task<Message> {
        let entries = combined_slab_structural_style_entries_with_session(
            self.aec.aec_project_explorer_file.as_ref(),
            self.aec.aec_session_style_library.as_ref(),
        );
        let style = entries
            .iter()
            .find(|e| e.structural_style.style.id == id)
            .map(|e| e.structural_style.clone())
            .or_else(|| {
                self.aec
                    .aec_style_library
                    .as_ref()
                    .and_then(|lib| lib.find_slab_structural_style(&id).cloned())
            });

        if let Some(style) = style {
            self.aec.styles.slab_structural_selected = Some(style.style.id.clone());
            self.aec.styles.slab_structural_editing_id = Some(style.style.id.clone());
            self.aec.aec_last_slab_structural_style_id = Some(style.style.id.clone());
            self.aec.styles.slab_structural_name = style.style.name.clone();
            self.aec.styles.slab_structural_parent = style.style.parent.clone();
            self.aec.styles.slab_structural_layers =
                style.layers.iter().map(layer_to_buffer).collect();
            self.aec.styles.slab_structural_form_open = true;
            self.aec.styles.slab_structural_drag_index = None;
        }

        Task::none()
    }

    pub(crate) fn aec_slab_structural_style_manager_new(&mut self) -> Task<Message> {
        let first_mat = self
            .aec
            .aec_style_library
            .as_ref()
            .and_then(|l| l.materials.first().map(|m| m.id.clone()))
            .unwrap_or_else(|| "Stahlbeton C25/30".to_string());

        self.aec.styles.slab_structural_selected = None;
        self.aec.styles.slab_structural_editing_id = None;
        self.aec.styles.slab_structural_name = crate::t!("New Structural Slab Style").to_string();
        self.aec.styles.slab_structural_parent = None;
        self.aec.styles.slab_structural_layers = vec![AecSlabLayerBuffer {
            material_id: first_mat,
            thickness: "20.0".to_string(),
            function: layer_function_to_str(&LayerFunction::Structural),
            vertical_offset: "0.0".to_string(),
            layer_override: String::new(),
            hatch_override: String::new(),
            role_tag: String::new(),
            layer_id: Some(uuid::Uuid::new_v4()),
        }];
        self.aec.styles.slab_structural_form_open = true;
        self.aec.styles.slab_structural_drag_index = None;

        Task::none()
    }

    pub(crate) fn aec_slab_structural_style_manager_duplicate(&mut self) -> Task<Message> {
        let current_layers = self.aec.styles.slab_structural_layers.clone();
        let current_name = self.aec.styles.slab_structural_name.clone();
        let current_parent = self.aec.styles.slab_structural_parent.clone();

        self.aec.styles.slab_structural_selected = None;
        self.aec.styles.slab_structural_editing_id = None;
        self.aec.styles.slab_structural_name = format!("{current_name} (Kopie)");
        self.aec.styles.slab_structural_parent = current_parent;
        self.aec.styles.slab_structural_layers = current_layers
            .into_iter()
            .map(|mut l| {
                l.layer_id = Some(uuid::Uuid::new_v4());
                l
            })
            .collect();
        self.aec.styles.slab_structural_form_open = true;
        self.aec.styles.slab_structural_drag_index = None;

        Task::none()
    }

    pub(crate) fn aec_slab_structural_style_manager_layer_add(&mut self) {
        let first_mat = self
            .aec
            .aec_style_library
            .as_ref()
            .and_then(|l| l.materials.first().map(|m| m.id.clone()))
            .unwrap_or_else(|| "Stahlbeton C25/30".to_string());

        self.aec
            .styles.slab_structural_layers
            .push(AecSlabLayerBuffer {
                material_id: first_mat,
                thickness: "20.0".to_string(),
                function: layer_function_to_str(&LayerFunction::Structural),
                vertical_offset: "0.0".to_string(),
                layer_override: String::new(),
                hatch_override: String::new(),
                role_tag: String::new(),
                layer_id: Some(uuid::Uuid::new_v4()),
            });
    }

    pub(crate) fn aec_slab_structural_style_manager_layer_remove(&mut self, index: usize) {
        if index < self.aec.styles.slab_structural_layers.len() {
            self.aec.styles.slab_structural_layers.remove(index);
        }
    }

    pub(crate) fn aec_slab_structural_style_manager_layer_move_up(&mut self, index: usize) {
        if index > 0 && index < self.aec.styles.slab_structural_layers.len() {
            self.aec
                .styles.slab_structural_layers
                .swap(index, index - 1);
        }
    }

    pub(crate) fn aec_slab_structural_style_manager_layer_move_down(&mut self, index: usize) {
        if index + 1 < self.aec.styles.slab_structural_layers.len() {
            self.aec
                .styles.slab_structural_layers
                .swap(index, index + 1);
        }
    }

    pub(crate) fn aec_slab_structural_style_manager_save_internal(
        &mut self,
    ) -> Result<SlabStructuralStyle, String> {
        let name = self.aec.styles.slab_structural_name.trim().to_string();
        if name.is_empty() {
            return Err(crate::t!("Style name cannot be empty").to_string());
        }

        let id = if let Some(edit_id) = &self.aec.styles.slab_structural_editing_id {
            edit_id.clone()
        } else {
            let base_slug = name
                .to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '_' })
                .collect::<String>();
            format!("struct_slab_{base_slug}")
        };

        let mut layers = Vec::new();
        for b in &self.aec.styles.slab_structural_layers {
            layers.push(buffer_to_layer(b)?);
        }

        let structural_style = SlabStructuralStyle {
            style: Style {
                id: id.clone(),
                name,
                category: "SlabStructural".to_string(),
                origin: Default::default(),
                parent: self.aec.styles.slab_structural_parent.clone(),
            },
            layers,
            default_profile: None,
        };

        if self.aec.aec_project_explorer_file.is_some() {
            self.aec_upsert_slab_structural_style_into_project(structural_style.clone())?;
        } else {
            self.aec_upsert_slab_structural_style_into_session(structural_style.clone());
        }

        self.aec.styles.slab_structural_selected = Some(id.clone());
        self.aec.styles.slab_structural_editing_id = Some(id.clone());
        self.aec.aec_last_slab_structural_style_id = Some(id);

        Ok(structural_style)
    }

    pub(crate) fn aec_slab_structural_style_manager_save(&mut self) -> Task<Message> {
        match self.aec_slab_structural_style_manager_save_internal() {
            Ok(_) => {
                self.command_line
                    .push_info(crate::t!("Structural slab style saved successfully").as_ref());
            }
            Err(e) => {
                self.command_line.push_error(&e);
            }
        }
        Task::none()
    }

    pub(crate) fn aec_slab_structural_style_manager_save_and_apply(&mut self) -> Task<Message> {
        match self.aec_slab_structural_style_manager_save_internal() {
            Ok(saved_style) => {
                let tab = self.active_tab;
                let slab_handles: Vec<acadrust::Handle> = self.tabs.get(tab).map_or_else(
                    Vec::new,
                    |tab_data| {
                        tab_data
                            .scene
                            .document
                            .entities()
                            .filter_map(|e| {
                                let handle = e.common().handle;
                                let slab =
                                    crate::modules::aec::engine::slab_xdata::slab_from_entity(e)?;
                                if slab.structural_style_id.as_deref() == Some(saved_style.style.id.as_str()) {
                                    Some(handle)
                                } else {
                                    None
                                }
                            })
                            .collect()
                    },
                );

                let mut count = 0;
                for handle in slab_handles {
                    if self.regenerate_slab_respecting_active_display_config(tab, handle) {
                        count += 1;
                    }
                }

                if count > 0 {
                    if let Some(tab_data) = self.tabs.get_mut(tab) {
                        tab_data.dirty = true;
                    }
                    self.command_line.push_info(
                        crate::tf!(
                            "AEC Structural Slab Style Manager: Saved and updated {count} slab(s)."
                        )
                        .as_ref(),
                    );
                } else {
                    self.command_line.push_info(
                        crate::tf!(
                            "AEC Structural Slab Style Manager: Saved style '{}'.",
                            saved_style.style.name
                        )
                        .as_ref(),
                    );
                }
            }
            Err(e) => {
                self.command_line.push_error(
                    crate::tf!("AEC Structural Slab Style Manager: Failed to save: {e}").as_ref(),
                );
            }
        }
        Task::none()
    }

    pub(crate) fn aec_slab_structural_style_manager_delete(&mut self) -> Task<Message> {
        let Some(selected) = self.aec.styles.slab_structural_selected.clone() else {
            return Task::none();
        };

        let has_project = self.aec.aec_project_explorer_file.is_some();
        let mut removed = false;

        if let Some(session_lib) = self.tabs[self.active_tab]
            .aec_session_style_library_mut()
            .as_mut()
        {
            if session_lib.remove_slab_structural_style(&selected) {
                removed = true;
            }
        }

        if has_project {
            if let Some(project) = self.aec.aec_project_explorer_file.as_mut() {
                if project
                    .material_wall_style_library
                    .remove_slab_structural_style(&selected)
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
                    "AEC Structural Slab Style Manager: Deleted style '{selected}'."
                )
                .as_ref(),
            );
            self.aec.styles.slab_structural_selected = None;
            self.aec.styles.slab_structural_editing_id = None;
            self.aec.styles.slab_structural_form_open = false;
        } else {
            self.command_line.push_error(
                crate::tf!(
                    "AEC Structural Slab Style Manager: Cannot delete standard or non-existent style '{selected}'."
                )
                .as_ref(),
            );
        }

        Task::none()
    }
}
