//! AEC update handlers: Style manager: display profiles and slot styles.

use iced::Task;

use crate::app::{AecModalKind, Message, OpenCADStudio};
use crate::modules::aec::message::AecMessage;

impl OpenCADStudio {
    pub(crate) fn update_aec_style_display_profiles(&mut self, msg: AecMessage) -> Task<Message> {
        match msg {
            AecMessage::AecStyleManagerProfileSelect(name) => self.aec_wall_profile_select(name),
            AecMessage::AecStyleManagerProfileContourModeToggle(is_explicit) => {
                self.aec.styles.profile_contour_explicit = is_explicit;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSolidModeToggle(is_explicit) => {
                self.aec.styles.profile_solid_explicit = is_explicit;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileContourLayerToggle(layer) => {
                if let Some(pos) = self
                    .aec.styles.profile_contour_selection
                    .iter()
                    .position(|l| *l == layer)
                {
                    self.aec.styles.profile_contour_selection.remove(pos);
                } else {
                    self.aec.styles.profile_contour_selection.push(layer);
                }
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSolidLayerToggle(layer) => {
                if let Some(pos) = self
                    .aec.styles.profile_solid_selection
                    .iter()
                    .position(|l| *l == layer)
                {
                    self.aec.styles.profile_solid_selection.remove(pos);
                } else {
                    self.aec.styles.profile_solid_selection.push(layer);
                }
                Task::none()
            }
            AecMessage::AecStyleManagerProfileHatchRelativeToggle(value) => {
                self.aec.styles.profile_hatch_relative = value;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileHatchAngleChanged(value) => {
                self.aec.styles.profile_hatch_angle = value;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotVisibilityToggle(slot, visible) => {
                self.aec.styles.profile_slot_visibility
                    .insert(slot, visible);
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleOpen(slot) => {
                use crate::modules::aec::ui::aec_ui_util::acad_color_to_editor_string;
                let style = self
                    .aec.styles.profile_slot_overrides
                    .get(&slot)
                    .cloned()
                    .unwrap_or_default();
                self.aec.styles.profile_editing_slot = Some(slot);
                self.aec.styles.profile_slot_style_line_type =
                    style.line_type.clone().unwrap_or_default();
                self.aec.styles.profile_slot_style_line_color = style
                    .line_color
                    .map(acad_color_to_editor_string)
                    .unwrap_or_default();
                self.aec.styles.profile_slot_style_hatch_pattern =
                    style.hatch_pattern.clone().unwrap_or_default();
                self.aec.styles.profile_slot_style_hatch_color = style
                    .hatch_color
                    .map(acad_color_to_editor_string)
                    .unwrap_or_default();
                self.aec.styles.profile_slot_style_fill_color = style
                    .fill_color
                    .map(acad_color_to_editor_string)
                    .unwrap_or_default();
                self.aec.styles.profile_slot_style_line_color_picker_open = false;
                self.aec.styles.profile_slot_style_hatch_color_picker_open = false;
                self.aec.styles.profile_slot_style_fill_color_picker_open = false;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleLineTypeChanged(v) => {
                self.ensure_aec_catalog_linetype(&v);
                self.aec.styles.profile_slot_style_line_type = v;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleLineColorChanged(v) => {
                self.aec.styles.profile_slot_style_line_color = v;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleLineColorPickerToggle => {
                self.aec.styles.profile_slot_style_line_color_picker_open =
                    !self.aec.styles.profile_slot_style_line_color_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleHatchPatternChanged(v) => {
                self.aec.styles.profile_slot_style_hatch_pattern = v;
                self.aec.styles.profile_slot_style_hatch_picker_open = false;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleHatchPickerToggle => {
                self.aec.styles.profile_slot_style_hatch_picker_open =
                    !self.aec.styles.profile_slot_style_hatch_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleHatchColorChanged(v) => {
                self.aec.styles.profile_slot_style_hatch_color = v;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleHatchColorPickerToggle => {
                self.aec.styles.profile_slot_style_hatch_color_picker_open =
                    !self.aec.styles.profile_slot_style_hatch_color_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleFillColorChanged(v) => {
                self.aec.styles.profile_slot_style_fill_color = v;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleFillColorPickerToggle => {
                self.aec.styles.profile_slot_style_fill_color_picker_open =
                    !self.aec.styles.profile_slot_style_fill_color_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleApply => {
                self.aec_wall_profile_slot_style_apply()
            }
            AecMessage::AecStyleManagerProfileSlotStyleClear => {
                if let Some(slot) = self.aec.styles.profile_editing_slot.take() {
                    self.aec.styles.profile_slot_overrides.remove(&slot);
                }
                self.clear_aec_profile_slot_style_editor_buffers();
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleClose => {
                self.aec.styles.profile_editing_slot = None;
                self.clear_aec_profile_slot_style_editor_buffers();
                Task::none()
            }
            AecMessage::AecWallStyleManagerDisplayProfilesOpen => {
                // Keep the wall style manager geometry so closing the child
                // modal can restore it (Plot → Plotstyle pattern).
                self.aec.styles.wall_parent_geometry =
                    Some((self.modal_offset, self.modal_resize));
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::WallStyleDisplayProfiles));
                self.reset_modal_geometry();
                Task::none()
            }
            AecMessage::AecWallStyleManagerDisplayProfilesClose => {
                self.close_active_modal();
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSave => {
                self.aec_wall_profile_save()
            }
            AecMessage::AecStyleManagerProfileRemove => {
                self.aec_wall_profile_remove()
            }
            AecMessage::AecStyleManagerProfileRemoveNamed(name) => {
                let _ = self.aec_wall_profile_select(name);
                self.aec_wall_profile_remove()
            }
            _ => Task::none(),
        }
    }
}
