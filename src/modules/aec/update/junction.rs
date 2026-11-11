//! AEC update handlers: Wall junction overrides, layer-pair picking and junction editor.

use iced::Task;

use crate::app::{AecModalKind, Message, OpenCADStudio};
use crate::modules::aec::engine::join_ops;
use crate::modules::aec::engine::xdata;
use crate::modules::aec::message::AecMessage;

impl OpenCADStudio {
    pub(crate) fn update_aec_junction(&mut self, msg: AecMessage) -> Task<Message> {
        match msg {
            AecMessage::WallJunctionSubmenuToggle => {
                let i = self.active_tab;
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.junction_menu_submenu = !sel.junction_menu_submenu;
                Task::none()
            }

            AecMessage::WallJunctionOverrideSetStyle(style) => {
                let i = self.active_tab;
                let junction = self.tabs[i].scene.selection.borrow().junction_menu;
                if let Some((axis_handle, end_index)) = junction {
                                        let mut override_data =
                        xdata::read_junction_override(&self.tabs[i].scene, axis_handle, end_index)
                            .unwrap_or_default();
                    override_data.default_style = Some(style);
                    let style_library =
                        crate::modules::aec::engine::project::resolve_style_library(
                            self.aec.aec_project_explorer_file.as_ref(),
                        );
                    let (display_rules, style_substitutions) =
                        self.resolve_active_display_config_wall_rules(i, Some(axis_handle));
                    let touched = join_ops::apply_junction_override_and_rebuild(
                        &mut self.tabs[i].scene,
                        axis_handle,
                        end_index,
                        Some(&override_data),
                        Some(&style_library),
                        display_rules.as_ref(),
                        style_substitutions.as_ref(),
                    );
                    self.reapply_active_display_config_to_wall_packages(i, &touched);
                }
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.menu.close();
                sel.junction_menu = None;
                drop(sel);
                self.refresh_properties();
                Task::none()
            }

            AecMessage::WallJunctionOverrideReset => {
                let i = self.active_tab;
                let junction = self.tabs[i].scene.selection.borrow().junction_menu;
                if let Some((axis_handle, end_index)) = junction {
                                        let style_library =
                        crate::modules::aec::engine::project::resolve_style_library(
                            self.aec.aec_project_explorer_file.as_ref(),
                        );
                    let (display_rules, style_substitutions) =
                        self.resolve_active_display_config_wall_rules(i, Some(axis_handle));
                    let touched = join_ops::apply_junction_override_and_rebuild(
                        &mut self.tabs[i].scene,
                        axis_handle,
                        end_index,
                        None,
                        Some(&style_library),
                        display_rules.as_ref(),
                        style_substitutions.as_ref(),
                    );
                    self.reapply_active_display_config_to_wall_packages(i, &touched);
                }
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.menu.close();
                sel.junction_menu = None;
                drop(sel);
                self.refresh_properties();
                Task::none()
            }

            AecMessage::AecJunctionLayerPairPickStart(axis_handle, end_index) => {
                let i = self.active_tab;
                self.aec.aec_layer_pair_draw = Some(crate::app::AecLayerPairDrawPick {
                    axis: axis_handle,
                    end_index,
                    layer_a: None,
                    layer_b: None,
                    layer_b_outer: false,
                    hover: None,
                    awaiting_style: false,
                });
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.menu.close();
                sel.junction_menu = Some((axis_handle, end_index));
                sel.junction_menu_only = false;
                drop(sel);
                self.sync_junction_editor_layer_highlight();
                self.command_line.push_info(
                    crate::t!("Erste Schicht am Knoten klicken (Esc = Abbrechen).").as_ref(),
                );
                Task::none()
            }

            AecMessage::AecJunctionLayerGapPickStart(axis_handle, end_index) => {
                let i = self.active_tab;
                self.aec.aec_layer_pair_draw = None;
                self.aec.aec_layer_gap_draw = Some(crate::app::AecLayerGapDrawPick {
                    axis: axis_handle,
                    end_index,
                    layer: None,
                    from: None,
                    to: None,
                    hover: None,
                });
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.menu.close();
                sel.junction_menu = Some((axis_handle, end_index));
                sel.junction_menu_only = false;
                drop(sel);
                self.sync_junction_editor_layer_highlight();
                self.command_line.push_info(
                    crate::t!(
                        "Schicht der durchlaufenden Wand klicken, die unterbrochen werden soll (Esc = Abbrechen)."
                    )
                    .as_ref(),
                );
                Task::none()
            }

            AecMessage::AecJunctionLayerPairPickCancel => {
                self.cancel_layer_pair_draw_pick();
                Task::none()
            }

            AecMessage::AecJunctionLayerPairSetStyle(style) => {
                self.apply_layer_pair_draw_style(style);
                Task::none()
            }

            AecMessage::AecJunctionEditorOpen(axis_handle, end_index) => {
                let i = self.active_tab;
                                let ov = xdata::read_junction_override(&self.tabs[i].scene, axis_handle, end_index)
                    .unwrap_or_default();
                self.aec.aec_junction_editor_target = Some((axis_handle, end_index));
                self.aec.aec_junction_editor_default_style = ov.default_style;
                self.aec.aec_junction_editor_pairs = ov.layer_pairs;
                self.aec.aec_junction_editor_gaps =
                    join_ops::read_through_layer_gaps(&self.tabs[i].scene, axis_handle, end_index);
                self.aec.aec_junction_editor_gap_layer = None;
                self.aec.aec_junction_editor_gap_from_wall = None;
                self.aec.aec_junction_editor_gap_from = None;
                self.aec.aec_junction_editor_gap_to_wall = None;
                self.aec.aec_junction_editor_gap_to = None;
                self.aec.aec_junction_editor_pair_layer_a = None;
                self.aec.aec_junction_editor_pair_wall_b = None;
                self.aec.aec_junction_editor_pair_layer_b = None;
                self.aec.aec_junction_editor_pair_style =
                    crate::modules::aec::engine::join::JoinOverrideStyle::Miter;
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.menu.close();
                sel.junction_menu = None;
                drop(sel);
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::JunctionEditor));
                self.sync_junction_editor_layer_highlight();
                Task::none()
            }

            AecMessage::AecJunctionEditorClose => {
                self.aec.aec_junction_editor_target = None;
                self.active_modal = None;
                self.reset_modal_geometry();
                self.sync_junction_editor_layer_highlight();
                Task::none()
            }

            AecMessage::AecJunctionEditorSetDefaultStyle(style) => {
                self.aec.aec_junction_editor_default_style = Some(style);
                Task::none()
            }

            AecMessage::AecJunctionEditorResetDefaultStyle => {
                self.aec.aec_junction_editor_default_style = None;
                Task::none()
            }

            AecMessage::AecJunctionEditorPairLayerAChanged(index, material_id) => {
                self.aec.aec_junction_editor_pair_layer_a = Some((index, material_id));
                self.sync_junction_editor_layer_highlight();
                Task::none()
            }

            AecMessage::AecJunctionEditorPairWallBChanged(wall_b) => {
                self.aec.aec_junction_editor_pair_wall_b = wall_b;
                self.aec.aec_junction_editor_pair_layer_b = None;
                self.sync_junction_editor_layer_highlight();
                Task::none()
            }

            AecMessage::AecJunctionEditorPairLayerBChanged(index, material_id) => {
                self.aec.aec_junction_editor_pair_layer_b = Some((index, material_id));
                self.sync_junction_editor_layer_highlight();
                Task::none()
            }

            AecMessage::AecJunctionEditorPairStyleChanged(style) => {
                self.aec.aec_junction_editor_pair_style = style;
                Task::none()
            }

            AecMessage::AecJunctionEditorAddPair => {
                use crate::modules::aec::engine::junction_pick::selected_junction_layer_ref;
                use crate::modules::aec::engine::join::LayerRef;
                if let Some((layer_a_index, layer_a_id)) = self.aec.aec_junction_editor_pair_layer_a.clone() {
                    let i = self.active_tab;
                    let participants = self.aec.aec_junction_editor_target.map(|(h, e)| {
                        crate::modules::aec::engine::join_ops::walls_at_junction(
                            &self.tabs[i].scene,
                            h,
                            e,
                        )
                    });
                    let resolve_ref = |wall: Option<acadrust::Handle>, index: usize, material_id: String| {
                        let layers = participants.as_ref().and_then(|parts| {
                            if let Some(h) = wall {
                                parts.iter().find(|p| p.axis_handle == h).map(|p| &p.layers)
                            } else {
                                self.aec.aec_junction_editor_target.and_then(|(h, e)| {
                                    parts
                                        .iter()
                                        .find(|p| p.axis_handle == h && p.end_index == e)
                                        .or_else(|| parts.iter().find(|p| p.axis_handle == h))
                                        .map(|p| &p.layers)
                                })
                            }
                        });
                        layers
                            .and_then(|ls| selected_junction_layer_ref(ls, index, &material_id))
                            .unwrap_or(LayerRef {
                                material_id,
                                role_tag: None,
                                index,
                                layer_id: None,
                            })
                    };
                    let layer_b = self.aec.aec_junction_editor_pair_layer_b.clone().map(|(index, id)| {
                        resolve_ref(self.aec.aec_junction_editor_pair_wall_b, index, id)
                    });
                    crate::modules::aec::engine::join::upsert_layer_pair(
                        &mut self.aec.aec_junction_editor_pairs,
                        crate::modules::aec::engine::join::LayerPairOverride {
                            layer_a: resolve_ref(None, layer_a_index, layer_a_id),
                            layer_b,
                            style: self.aec.aec_junction_editor_pair_style.clone(),
                        },
                    );
                    self.aec.aec_junction_editor_pair_layer_a = None;
                    self.aec.aec_junction_editor_pair_wall_b = None;
                    self.aec.aec_junction_editor_pair_layer_b = None;
                    self.sync_junction_editor_layer_highlight();
                }
                Task::none()
            }

            AecMessage::AecJunctionEditorSetPairStyle(idx, style) => {
                if let Some(pair) = self.aec.aec_junction_editor_pairs.get_mut(idx) {
                    pair.style = style;
                }
                Task::none()
            }

            AecMessage::AecJunctionEditorRemovePair(idx) => {
                if idx < self.aec.aec_junction_editor_pairs.len() {
                    self.aec.aec_junction_editor_pairs.remove(idx);
                }
                Task::none()
            }

            AecMessage::AecJunctionEditorGapLayerChanged(index, material_id) => {
                self.aec.aec_junction_editor_gap_layer = Some((index, material_id));
                self.sync_junction_editor_layer_highlight();
                Task::none()
            }
            AecMessage::AecJunctionEditorGapFromWallChanged(handle) => {
                self.aec.aec_junction_editor_gap_from_wall = handle;
                self.aec.aec_junction_editor_gap_from = None;
                Task::none()
            }
            AecMessage::AecJunctionEditorGapFromChanged(index, material_id) => {
                self.aec.aec_junction_editor_gap_from = Some((index, material_id));
                Task::none()
            }
            AecMessage::AecJunctionEditorGapToWallChanged(handle) => {
                self.aec.aec_junction_editor_gap_to_wall = handle;
                self.aec.aec_junction_editor_gap_to = None;
                Task::none()
            }
            AecMessage::AecJunctionEditorGapToChanged(index, material_id) => {
                self.aec.aec_junction_editor_gap_to = Some((index, material_id));
                Task::none()
            }
            AecMessage::AecJunctionEditorAddGap => {
                if let (Some((li, lid)), Some((fi, fid)), Some((ti, tid))) = (
                    self.aec.aec_junction_editor_gap_layer.clone(),
                    self.aec.aec_junction_editor_gap_from.clone(),
                    self.aec.aec_junction_editor_gap_to.clone(),
                ) {
                    use crate::modules::aec::engine::junction_pick::selected_junction_layer_ref;
                    use crate::modules::aec::engine::join::LayerRef;
                    let i = self.active_tab;
                    let participants = self.aec.aec_junction_editor_target.map(|(h, e)| {
                        crate::modules::aec::engine::join_ops::walls_at_junction(
                            &self.tabs[i].scene,
                            h,
                            e,
                        )
                    });
                    let resolve_ref = |wall: Option<acadrust::Handle>, index: usize, material_id: String| {
                        let layers = participants.as_ref().and_then(|parts| {
                            if let Some(h) = wall {
                                parts.iter().find(|p| p.axis_handle == h).map(|p| &p.layers)
                            } else {
                                parts
                                    .iter()
                                    .find(|p| p.is_through)
                                    .or_else(|| {
                                        self.aec.aec_junction_editor_target.and_then(|(h, e)| {
                                            parts
                                                .iter()
                                                .find(|p| p.axis_handle == h && p.end_index == e)
                                                .or_else(|| {
                                                    parts.iter().find(|p| p.axis_handle == h)
                                                })
                                        })
                                    })
                                    .map(|p| &p.layers)
                            }
                        });
                        layers
                            .and_then(|ls| selected_junction_layer_ref(ls, index, &material_id))
                            .unwrap_or(LayerRef {
                                material_id,
                                role_tag: None,
                                index,
                                layer_id: None,
                            })
                    };
                    crate::modules::aec::engine::join::upsert_layer_gap(
                        &mut self.aec.aec_junction_editor_gaps,
                        crate::modules::aec::engine::join::LayerGapOverride {
                            layer: resolve_ref(None, li, lid),
                            from: resolve_ref(self.aec.aec_junction_editor_gap_from_wall, fi, fid),
                            to: resolve_ref(self.aec.aec_junction_editor_gap_to_wall, ti, tid),
                        },
                    );
                    self.aec.aec_junction_editor_gap_layer = None;
                    self.aec.aec_junction_editor_gap_from = None;
                    self.aec.aec_junction_editor_gap_to = None;
                    self.sync_junction_editor_layer_highlight();
                }
                Task::none()
            }
            AecMessage::AecJunctionEditorRemoveGap(idx) => {
                if idx < self.aec.aec_junction_editor_gaps.len() {
                    self.aec.aec_junction_editor_gaps.remove(idx);
                }
                Task::none()
            }

            AecMessage::AecJunctionEditorSave => {
                if let Some((axis_handle, end_index)) = self.aec.aec_junction_editor_target {
                    let i = self.active_tab;
                                        let override_data = crate::modules::aec::engine::join::JunctionOverride {
                        default_style: self.aec.aec_junction_editor_default_style.clone(),
                        layer_pairs: self.aec.aec_junction_editor_pairs.clone(),
                        layer_gaps: self.aec.aec_junction_editor_gaps.clone(),
                    };
                    let style_library =
                        crate::modules::aec::engine::project::resolve_style_library(
                            self.aec.aec_project_explorer_file.as_ref(),
                        );
                    let (display_rules, style_substitutions) =
                        self.resolve_active_display_config_wall_rules(i, Some(axis_handle));
                    let ov = if override_data.is_empty() {
                        None
                    } else {
                        Some(override_data)
                    };
                    let touched = join_ops::apply_junction_override_and_rebuild(
                        &mut self.tabs[i].scene,
                        axis_handle,
                        end_index,
                        ov.as_ref(),
                        Some(&style_library),
                        display_rules.as_ref(),
                        style_substitutions.as_ref(),
                    );
                    self.reapply_active_display_config_to_wall_packages(i, &touched);
                    self.refresh_properties();
                }
                self.aec.aec_junction_editor_target = None;
                self.active_modal = None;
                self.reset_modal_geometry();
                self.sync_junction_editor_layer_highlight();
                Task::none()
            }

            AecMessage::AecJunctionEditorFullReset => {
                if let Some((axis_handle, end_index)) = self.aec.aec_junction_editor_target {
                    let i = self.active_tab;
                                        let style_library =
                        crate::modules::aec::engine::project::resolve_style_library(
                            self.aec.aec_project_explorer_file.as_ref(),
                        );
                    let (display_rules, style_substitutions) =
                        self.resolve_active_display_config_wall_rules(i, Some(axis_handle));
                    let touched = join_ops::apply_junction_override_and_rebuild(
                        &mut self.tabs[i].scene,
                        axis_handle,
                        end_index,
                        None,
                        Some(&style_library),
                        display_rules.as_ref(),
                        style_substitutions.as_ref(),
                    );
                    self.reapply_active_display_config_to_wall_packages(i, &touched);
                    self.refresh_properties();
                }
                self.aec.aec_junction_editor_target = None;
                self.active_modal = None;
                self.reset_modal_geometry();
                self.sync_junction_editor_layer_highlight();
                Task::none()
            }

            _ => Task::none(),
        }
    }
}
