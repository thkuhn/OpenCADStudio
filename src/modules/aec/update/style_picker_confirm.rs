//! AEC update handlers: Style picker: confirm and apply selection.

use iced::Task;

use crate::app::{AecModalKind, Message, OpenCADStudio};
use crate::modules::aec::message::AecMessage;

impl OpenCADStudio {
    pub(crate) fn update_aec_style_picker_confirm(&mut self, msg: AecMessage) -> Task<Message> {
        match msg {
            AecMessage::AecStylePickerConfirm => {
                if let (Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker { target })), Some(selection)) =
                    (self.active_modal, self.aec.aec_style_picker_selection.clone())
                {
                    match target {
                        crate::app::StylePickerTarget::WallStyleParent => {
                            let id = if selection.is_empty() {
                                None
                            } else {
                                Some(selection)
                            };
                            self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(AecMessage::AecStyleManagerWallStyleParentChanged(id)));
                        }
                        crate::app::StylePickerTarget::LayerMaterial(index) => {
                            self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(AecMessage::AecStyleManagerWallStyleLayerMaterialChanged(
                                index, selection,
                            )));
                        }
                        crate::app::StylePickerTarget::LayerOverride(index) => {
                            self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(AecMessage::AecStyleManagerWallStyleLayerOverrideChanged(
                                index, selection,
                            )));
                        }
                        crate::app::StylePickerTarget::WallPropertiesStyle => {
                            self.active_modal = None;
                            let Some(lib) = self.aec.aec_style_library.clone() else {
                                return Task::none();
                            };

                            let i = self.active_tab;
                            self.push_undo_snapshot(i, "CHPROP");

                            let handles = self.aec.aec_style_picker_wall_handles.clone();
                            for handle in handles {
                                let wall_owner = crate::modules::aec::engine::wall_package::resolve_wall_package(
                                    &self.tabs[i].scene,
                                    handle,
                                );
                                let bb = self.tabs[i]
                                    .scene
                                    .document
                                    .get_entity(wall_owner)
                                    .and_then(crate::modules::aec::engine::xdata::wall_from_entity)
                                    .and_then(|w| {
                                        let t = w.total_thickness();
                                        if t > 0.0 {
                                            Some(t)
                                        } else {
                                            None
                                        }
                                    });
                                let wall_layers = crate::modules::aec::engine::xdata::resolve_wall_style_layers_ids(
                                    &lib,
                                    &selection,
                                    bb,
                                )
                                .unwrap_or_default();

                                if crate::modules::aec::engine::xdata::write_wall_style(
                                    &mut self.tabs[i].scene,
                                    wall_owner,
                                    &selection,
                                    wall_layers,
                                ) {
                                    let _ = self.regenerate_wall_respecting_active_display_config(i, wall_owner);
                                    self.tabs[i].dirty = true;
                                }
                            }
                            self.refresh_properties();
                        }
                        crate::app::StylePickerTarget::OpeningStyleParent => {
                            let id = if selection.is_empty() {
                                None
                            } else {
                                Some(selection)
                            };
                            self.active_modal =
                                Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecOpeningStyleManagerParentChanged(id),
                            ));
                        }
                        crate::app::StylePickerTarget::OpeningComponentMaterial(role) => {
                            self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            self.aec_opening_style_manager_look_material_changed(role, selection);
                            return Task::none();
                        }
                        crate::app::StylePickerTarget::OpeningPropertiesStyle => {
                            self.active_modal = None;
                            return self.aec_apply_picked_opening_style(selection);
                        }
                        crate::app::StylePickerTarget::SlabStyleParent => {
                            let id = if selection.is_empty() {
                                None
                            } else {
                                Some(selection)
                            };
                            self.active_modal =
                                Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecSlabStyleManagerParentChanged(id),
                            ));
                        }
                        crate::app::StylePickerTarget::FloorFinishStyleParent => {
                            let id = if selection.is_empty() {
                                None
                            } else {
                                Some(selection)
                            };
                            self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecFloorFinishStyleManagerParentChanged(id),
                            ));
                        }
                        crate::app::StylePickerTarget::FloorFinishLayerMaterial(index) => {
                            self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecFloorFinishStyleManagerLayerMaterialChanged(
                                    index, selection,
                                ),
                            ));
                        }
                        crate::app::StylePickerTarget::SlabStructuralStyleParent => {
                            let id = if selection.is_empty() {
                                None
                            } else {
                                Some(selection)
                            };
                            self.active_modal =
                                Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecSlabStructuralStyleManagerParentChanged(id),
                            ));
                        }
                        crate::app::StylePickerTarget::SlabLayerMaterial(index) => {
                            self.active_modal =
                                Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecSlabStyleManagerLayerMaterialChanged(
                                    index, selection,
                                ),
                            ));
                        }
                        crate::app::StylePickerTarget::SlabStructuralLayerMaterial(index) => {
                            self.active_modal =
                                Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecSlabStructuralStyleManagerLayerMaterialChanged(
                                    index, selection,
                                ),
                            ));
                        }
                        crate::app::StylePickerTarget::SlabStructuralLayerOverride(index) => {
                            self.active_modal =
                                Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecSlabStructuralStyleManagerLayerOverrideChanged(
                                    index, selection,
                                ),
                            ));
                        }
                        crate::app::StylePickerTarget::FloorFinishLayerOverride(index) => {
                            self.active_modal =
                                Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecFloorFinishStyleManagerLayerOverrideChanged(
                                    index, selection,
                                ),
                            ));
                        }
                        crate::app::StylePickerTarget::SlabLayerOverride(index) => {
                            self.active_modal =
                                Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecSlabStyleManagerLayerOverrideChanged(
                                    index, selection,
                                ),
                            ));
                        }
                        crate::app::StylePickerTarget::SlabPropertiesStyle => {
                            self.active_modal = None;
                            let Some(lib) = self.aec.aec_style_library.clone() else {
                                return Task::none();
                            };
                            let i = self.active_tab;
                            self.push_undo_snapshot(i, "CHPROP");
                            let handles = self.aec.aec_style_picker_wall_handles.clone();
                            if !selection.is_empty() {
                                self.aec.aec_last_slab_style_id = Some(selection.clone());
                            }
                            for handle in handles {
                                let slab_owner =
                                    crate::modules::aec::engine::slab_package::resolve_slab_package(
                                        &self.tabs[i].scene,
                                        handle,
                                    );
                                let thickness = self.tabs[i]
                                    .scene
                                    .document
                                    .get_entity(slab_owner)
                                    .and_then(crate::modules::aec::engine::slab_xdata::slab_from_entity)
                                    .and_then(|s| {
                                        let t = s.total_thickness();
                                        if t > 0.0 {
                                            Some(t)
                                        } else {
                                            None
                                        }
                                    });
                                let slab_layers = crate::modules::aec::engine::slab_xdata::resolve_slab_style_layers_ids(
                                    &lib,
                                    &selection,
                                    thickness,
                                )
                                .unwrap_or_default();
                                if crate::modules::aec::engine::slab_xdata::write_slab_style(
                                    &mut self.tabs[i].scene,
                                    slab_owner,
                                    &selection,
                                    slab_layers,
                                ) {
                                    let _ = self
                                        .regenerate_slab_respecting_active_display_config(i, slab_owner);
                                    self.tabs[i].dirty = true;
                                }
                            }
                            self.refresh_properties();
                        }
                        crate::app::StylePickerTarget::ActiveCommand => {
                            let i = self.active_tab;
                            if !selection.is_empty() {
                                self.aec.aec_last_wall_style_id = Some(selection.clone());
                            }
                            let result = self.tabs[i].active_cmd.as_mut().map(|c| {
                                c.apply_live_property(
                                    "wall_style",
                                    crate::command::LiveFieldValue::Picker(selection.clone()),
                                )
                            });
                            self.active_modal = None;
                            if let Some(r) = result {
                                let task = self.apply_cmd_result(r);
                                self.refresh_active_cmd_preview(i);
                                self.refresh_properties();
                                return task;
                            }
                            self.refresh_properties();
                            return Task::none();
                        }
                    }
                }
                self.active_modal = None;
                Task::none()
            }
            _ => Task::none(),
        }
    }
}
