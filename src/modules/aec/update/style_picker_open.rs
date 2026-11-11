//! AEC update handlers: Style picker: open, filter, select, cancel.

use iced::Task;

use crate::app::{AecModalKind, Message, OpenCADStudio};
use crate::modules::aec::message::AecMessage;

impl OpenCADStudio {
    pub(crate) fn update_aec_style_picker_open(&mut self, msg: AecMessage) -> Task<Message> {
        match msg {
            AecMessage::AecStylePickerOpen(target) => {
                self.aec_refresh_combined_style_library();
                self.aec.aec_style_picker_filter.clear();
                // Pre-select/highlight whatever is already assigned for this
                // target so the picker doesn't reopen with nothing
                // highlighted even though a value is already in use.
                self.aec.aec_style_picker_selection = match target {
                    crate::app::StylePickerTarget::WallStyleParent => {
                        self.aec.styles.wall_style_parent.clone()
                    }
                    crate::app::StylePickerTarget::LayerMaterial(index) => self
                        .aec.styles.wall_style_layers
                        .get(index)
                        .map(|l| l.material_id.clone()),
                    crate::app::StylePickerTarget::LayerOverride(index) => self
                        .aec.styles.wall_style_layers
                        .get(index)
                        .map(|l| l.layer_override.clone()),
                    crate::app::StylePickerTarget::WallPropertiesStyle
                    | crate::app::StylePickerTarget::ActiveCommand
                    | crate::app::StylePickerTarget::OpeningPropertiesStyle
                    | crate::app::StylePickerTarget::SlabPropertiesStyle => None,
                    crate::app::StylePickerTarget::OpeningStyleParent => {
                        self.aec.styles.opening_parent.clone()
                    }
                    crate::app::StylePickerTarget::OpeningComponentMaterial(role) => self
                        .aec
                        .styles
                        .opening_look_buffers
                        .get(&role)
                        .map(|buffer| buffer.material_id.clone()),
                    crate::app::StylePickerTarget::SlabStyleParent => {
                        self.aec.styles.slab_parent.clone()
                    }
                    crate::app::StylePickerTarget::SlabStructuralStyleParent => {
                        self.aec.styles.slab_structural_parent.clone()
                    }
                    crate::app::StylePickerTarget::SlabLayerMaterial(index) => self
                        .aec
                        .styles.slab_layers
                        .get(index)
                        .map(|l| l.material_id.clone()),
                    crate::app::StylePickerTarget::SlabStructuralLayerMaterial(index) => self
                        .aec
                        .styles.slab_structural_layers
                        .get(index)
                        .map(|l| l.material_id.clone()),
                    crate::app::StylePickerTarget::SlabLayerOverride(index) => self
                        .aec
                        .styles.slab_layers
                        .get(index)
                        .map(|l| l.layer_override.clone()),
                    crate::app::StylePickerTarget::SlabStructuralLayerOverride(index) => self
                        .aec
                        .styles.slab_structural_layers
                        .get(index)
                        .map(|l| l.layer_override.clone()),
                    crate::app::StylePickerTarget::FloorFinishLayerOverride(index) => self
                        .aec
                        .styles.floor_finish_layers
                        .get(index)
                        .map(|l| l.layer_override.clone()),
                    crate::app::StylePickerTarget::FloorFinishStyleParent => {
                        self.aec.styles.floor_finish_parent.clone()
                    }
                    crate::app::StylePickerTarget::FloorFinishLayerMaterial(index) => self
                        .aec
                        .styles.floor_finish_layers
                        .get(index)
                        .map(|l| l.material_id.clone()),
                };
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker { target }));
                Task::none()
            }
            AecMessage::AecStylePickerOpenForWallProperties(handles) => {
                self.aec_refresh_combined_style_library();
                self.aec.aec_style_picker_filter.clear();
                // Pre-select/highlight the style already assigned to the
                // (first) wall being edited, so the picker doesn't reopen
                // with nothing highlighted even though a style is in use.
                self.aec.aec_style_picker_selection = handles.first().and_then(|h| {
                    self.tabs[self.active_tab]
                        .scene
                        .document
                        .get_entity(*h)
                        .and_then(crate::modules::aec::engine::xdata::wall_from_entity)
                        .map(|v2| v2.style_id)
                });
                self.aec.aec_style_picker_wall_handles = handles;
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker {
                    target: crate::app::StylePickerTarget::WallPropertiesStyle,
                }));
                Task::none()
            }
            AecMessage::AecStylePickerOpenForOpeningProperties(handles) => {
                self.aec_refresh_combined_style_library();
                self.aec.aec_style_picker_filter.clear();
                self.aec.aec_style_picker_selection = handles.first().and_then(|h| {
                    let owner = crate::modules::aec::engine::opening_display::resolve_opening_package(
                        &self.tabs[self.active_tab].scene,
                        *h,
                    );
                    self.tabs[self.active_tab]
                        .scene
                        .document
                        .get_entity(owner)
                        .and_then(|e| {
                            crate::modules::aec::engine::opening_xdata::opening_from_entity(e, owner)
                        })
                        .and_then(|o| o.style_id)
                });
                self.aec.aec_style_picker_wall_handles = handles;
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker {
                    target: crate::app::StylePickerTarget::OpeningPropertiesStyle,
                }));
                Task::none()
            }
            AecMessage::AecStylePickerOpenForSlabProperties(handles) => {
                self.aec_refresh_combined_style_library();
                self.aec.aec_style_picker_filter.clear();
                self.aec.aec_style_picker_selection = handles.first().and_then(|h| {
                    let owner = crate::modules::aec::engine::slab_package::resolve_slab_package(
                        &self.tabs[self.active_tab].scene,
                        *h,
                    );
                    self.tabs[self.active_tab]
                        .scene
                        .document
                        .get_entity(owner)
                        .and_then(crate::modules::aec::engine::slab_xdata::slab_from_entity)
                        .map(|s| s.style_id)
                });
                self.aec.aec_style_picker_wall_handles = handles;
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker {
                    target: crate::app::StylePickerTarget::SlabPropertiesStyle,
                }));
                Task::none()
            }
            AecMessage::AecStylePickerOpenForActiveCommand => {
                self.aec_refresh_combined_style_library();
                self.aec.aec_style_picker_filter.clear();
                // Pre-select/highlight the style currently set on the
                // in-progress wall (if any) instead of always starting with
                // nothing highlighted.
                self.aec.aec_style_picker_selection = self.tabs[self.active_tab]
                    .active_cmd
                    .as_ref()
                    .and_then(|c| c.live_property_id("wall_style"));
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker {
                    target: crate::app::StylePickerTarget::ActiveCommand,
                }));
                Task::none()
            }
            AecMessage::AecStylePickerFilterChanged(v) => {
                self.aec.aec_style_picker_filter = v;
                Task::none()
            }
            AecMessage::AecStylePickerSelect(id) => {
                self.aec.aec_style_picker_selection = Some(id);
                Task::none()
            }
            AecMessage::AecStylePickerCancel => {
                // Targets opened from within the Wall Style Manager return
                // there on cancel, instead of closing the modal entirely.
                if let Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker { target })) = self.active_modal
                {
                    if matches!(
                        target,
                        crate::app::StylePickerTarget::WallStyleParent
                            | crate::app::StylePickerTarget::LayerMaterial(_)
                            | crate::app::StylePickerTarget::LayerOverride(_)
                    ) {
                        self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                        return Task::none();
                    }
                    if matches!(
                        target,
                        crate::app::StylePickerTarget::OpeningStyleParent
                    ) {
                        self.active_modal =
                            Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                        return Task::none();
                    }
                    if matches!(
                        target,
                        crate::app::StylePickerTarget::SlabStyleParent
                            | crate::app::StylePickerTarget::SlabLayerMaterial(_)
                            | crate::app::StylePickerTarget::SlabLayerOverride(_)
                    ) {
                        self.active_modal =
                            Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                        return Task::none();
                    }
                    if matches!(
                        target,
                        crate::app::StylePickerTarget::FloorFinishStyleParent
                            | crate::app::StylePickerTarget::FloorFinishLayerMaterial(_)
                            | crate::app::StylePickerTarget::FloorFinishLayerOverride(_)
                    ) {
                        self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                        return Task::none();
                    }
                    if matches!(
                        target,
                        crate::app::StylePickerTarget::SlabStructuralStyleParent
                            | crate::app::StylePickerTarget::SlabStructuralLayerMaterial(_)
                            | crate::app::StylePickerTarget::SlabStructuralLayerOverride(_)
                    ) {
                        self.active_modal =
                            Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                        return Task::none();
                    }
                }
                self.close_active_modal();
                Task::none()
            }
            _ => Task::none(),
        }
    }
}
