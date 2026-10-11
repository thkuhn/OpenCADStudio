//! Shared action bar of `AEC_STYLES` (save / duplicate / delete / copy).

use iced::widget::{button, row, text, Space};
use iced::Element;

use crate::app::Message;
use crate::modules::aec::engine::style_common::StyleOrigin;
use crate::t;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    SaveApply,
    Save,
    Duplicate,
    Delete,
    ToStandard,
    ToProject,
}

/// Which actions are offered. Saved entries get duplicate / delete; copy to
/// Standard only for pure project entries (no Standard counterpart), copy to
/// Projekt only for Standard entries.
pub fn visible_actions(is_new: bool, origin: Option<StyleOrigin>, has_standard_counterpart: bool) -> Vec<ActionKind> {
    let mut v = vec![ActionKind::SaveApply, ActionKind::Save];
    if is_new {
        return v;
    }
    v.push(ActionKind::Duplicate);
    v.push(ActionKind::Delete);
    if origin == Some(StyleOrigin::Project) && !has_standard_counterpart {
        v.push(ActionKind::ToStandard);
    }
    if origin == Some(StyleOrigin::Default) {
        v.push(ActionKind::ToProject);
    }
    v
}

/// Messages of one style kind for the action bar.
pub struct ActionMessages {
    pub save_apply: Message,
    pub save: Message,
    pub duplicate: Message,
    pub delete: Message,
    pub to_standard: Message,
    pub to_project: Message,
}

pub fn view<'a>(
    msgs: ActionMessages,
    is_new: bool,
    origin: Option<StyleOrigin>,
    has_standard_counterpart: bool,
) -> Element<'a, Message> {
    let mut bar = row![Space::new()].spacing(8);
    for kind in visible_actions(is_new, origin, has_standard_counterpart) {
        let (label, msg, style): (_, &Message, fn(&iced::Theme, button::Status) -> button::Style) = match kind {
            ActionKind::SaveApply => (t!("Übernehmen"), &msgs.save_apply, button::primary),
            ActionKind::Save => (t!("Speichern"), &msgs.save, button::secondary),
            ActionKind::Duplicate => (t!("Duplizieren"), &msgs.duplicate, button::secondary),
            ActionKind::Delete => (t!("Delete"), &msgs.delete, button::danger),
            ActionKind::ToStandard => (t!("→ Standard"), &msgs.to_standard, button::secondary),
            ActionKind::ToProject => (t!("→ Projekt"), &msgs.to_project, button::secondary),
        };
        bar = bar.push(button(text(label).size(11)).style(style).padding([5, 12]).on_press(msg.clone()));
    }
    bar.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ActionKind::*;

    #[test]
    fn new_entry_only_saves() {
        assert_eq!(visible_actions(true, None, false), vec![SaveApply, Save]);
    }

    #[test]
    fn copy_actions_follow_origin() {
        let p = visible_actions(false, Some(StyleOrigin::Project), false);
        assert!(p.contains(&ToStandard) && !p.contains(&ToProject));
        let p = visible_actions(false, Some(StyleOrigin::Project), true);
        assert!(!p.contains(&ToStandard));
        let d = visible_actions(false, Some(StyleOrigin::Default), true);
        assert!(d.contains(&ToProject) && !d.contains(&ToStandard));
        let s = visible_actions(false, Some(StyleOrigin::Session), false);
        assert!(s.contains(&Delete) && !s.contains(&ToProject) && !s.contains(&ToStandard));
    }
}
