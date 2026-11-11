//! AEC update handlers: Style manager: wall style save and delete.

use iced::Task;

use crate::app::{Message, OpenCADStudio};
use crate::modules::aec::message::AecMessage;

impl OpenCADStudio {
    pub(crate) fn update_aec_style_wall_save(&mut self, msg: AecMessage) -> Task<Message> {
        match msg {
            AecMessage::AecStyleManagerWallStyleSave => {
                self.aec_style_manager_wall_style_save_internal();
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleSaveAndApply => {
                if let Some((id, lib)) = self.aec_style_manager_wall_style_save_internal() {
                    let tab_index = self.active_tab;
                    let wall_style_map: std::collections::HashMap<
                        String,
                        crate::modules::aec::engine::wall_style::WallStyle,
                    > = lib
                        .wall_styles
                        .iter()
                        .map(|ws| (ws.style.id.clone(), ws.clone()))
                        .collect();

                    let style_map: std::collections::HashMap<
                        String,
                        crate::modules::aec::engine::style::Style,
                    > = wall_style_map
                        .iter()
                        .map(|(k, v)| (k.clone(), v.style.clone()))
                        .collect();

                    let mut affected_handles = Vec::new();
                    for entity in self.tabs[tab_index].scene.document.entities() {
                        if let Some(wall) =
                            crate::modules::aec::engine::xdata::wall_from_entity(entity)
                        {
                            if wall.style_id == id {
                                affected_handles.push(entity.common().handle);
                            } else if let Ok(chain) =
                                crate::modules::aec::engine::style::resolve_chain(
                                    &style_map,
                                    &wall.style_id,
                                )
                            {
                                if chain.contains(&id) {
                                    affected_handles.push(entity.common().handle);
                                }
                            }
                        }
                    }

                    let mut updated_count = 0;
                    for handle in affected_handles {
                        if let Some(entity) = self.tabs[tab_index].scene.document.get_entity(handle) {
                            if let Some(wall) =
                                crate::modules::aec::engine::xdata::wall_from_entity(entity)
                            {
                                // Prefer the wall's current total thickness as BB so
                                // formula layers scale with the placed wall width;
                                // fall back to style fixed-sum when empty.
                                let bb = {
                                    let t = wall.total_thickness();
                                    if t > 0.0 {
                                        Some(t)
                                    } else {
                                        None
                                    }
                                };
                                if let Some(wall_layers) =
                                    crate::modules::aec::engine::xdata::resolve_wall_style_layers_ids(
                                        &lib,
                                        &wall.style_id,
                                        bb,
                                    )
                                {
                                    crate::modules::aec::engine::xdata::write_wall_layers(
                                        &mut self.tabs[tab_index].scene,
                                        handle,
                                        wall_layers,
                                    );
                                }
                            }
                        }

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
                                "AEC Style Manager: updated {updated_count} wall(s) using this style."
                            )
                            .as_ref(),
                        );
                    }
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleDelete => {
                if let Some(id) = self.aec.styles.selected_wall_style.clone() {
                    let lib_snapshot = if let Some(lib) = self.aec.aec_style_library.as_mut() {
                        lib.remove_wall_style(&id);
                        Some(lib.clone())
                    } else {
                        None
                    };
                    if let Some(lib_snapshot) = lib_snapshot {
                        match self.aec_save_style_library_preferring_project(&lib_snapshot) {
                            Ok(()) => self.command_line.push_info(
                                crate::t!("AEC Style Manager: wall style deleted.").as_ref(),
                            ),
                            Err(e) => self.command_line.push_error(
                                crate::tf!("AEC Style Manager: failed to save library: {e}")
                                    .as_ref(),
                            ),
                        }
                    }
                }
                self.aec.styles.selected_wall_style = None;
                self.aec.styles.wall_style_editing_id = None;
                self.aec.styles.wall_duplicate_default_profile = None;
                self.aec.styles.wall_style_form_open = false;
                Task::none()
            }
            _ => Task::none(),
        }
    }
}
