//! "Duplizieren" for wall and opening styles in `AEC_STYLES`: turns the open
//! form into a new, unsaved entry (same pattern as slab / floor-finish).

use iced::Task;

use crate::app::{Message, OpenCADStudio};
use crate::modules::aec::engine::library::STYLE_DEFAULT_PROFILE_KEY;

impl OpenCADStudio {
    pub(crate) fn aec_wall_style_manager_duplicate(&mut self) -> Task<Message> {
        let a = &mut self.aec;
        if !a.styles.wall_style_form_open {
            return Task::none();
        }
        // Carry the style default (not plan overrides) into the copy.
        let carried = a.styles.wall_style_editing_id.clone();
        let default = carried
            .as_deref()
            .and_then(|id| self.aec_wall_profile_lookup(id, STYLE_DEFAULT_PROFILE_KEY));
        let a = &mut self.aec;
        a.styles.wall_duplicate_default_profile =
            default.or(a.styles.wall_duplicate_default_profile.take());
        a.styles.selected_wall_style = None;
        a.styles.wall_style_editing_id = None;
        a.styles.wall_style_name = format!("{} (Kopie)", a.styles.wall_style_name);
        for l in &mut a.styles.wall_style_layers {
            l.layer_id = Some(uuid::Uuid::new_v4());
        }
        a.styles.wall_style_drag_index = None;
        a.styles.profile_selected = None;
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_duplicate(&mut self) -> Task<Message> {
        if !self.aec.styles.opening_form_open {
            return Task::none();
        }
        super::opening_style_manager::flush_buffers(self);
        // Keep the style default (incl. its profile); drop plan overrides.
        super::opening_style_manager::detach_plan_profiles(self, true);
        let a = &mut self.aec;
        a.styles.opening_selected = None;
        a.styles.opening_editing_id = None;
        a.styles.opening_name = format!("{} (Kopie)", a.styles.opening_name);
        Task::none()
    }
}
