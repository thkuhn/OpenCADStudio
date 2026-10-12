//! Shared "plan type overrides" section used by every style form that can
//! carry per-plan-type exceptions (wall, opening, slab).
//!
//! A dropdown offers only plan types without an exception yet; below it the
//! existing exceptions are listed with Edit / Remove buttons.

use iced::widget::{button, column, container, pick_list, row, text, Space};
use iced::{Background, Border, Element, Fill, Theme};

use crate::app::Message;
use crate::t;

pub struct PlanOverrideProps<'a> {
    /// Every plan type name of the library.
    pub all_names: &'a [String],
    /// Plan type names that already carry an exception for this style.
    pub existing: &'a [String],
    /// Currently edited plan type (`None` = the style default).
    pub selected: Option<&'a str>,
    /// Value passed to `on_select` for the style-default row.
    pub default_key: &'a str,
}

/// Plan types without an exception yet (the dropdown content).
pub fn available_plan_names(
    all_names: &[String],
    existing: &[String],
    default_key: &str,
) -> Vec<String> {
    all_names
        .iter()
        .filter(|n| n.as_str() != default_key && !existing.contains(n))
        .cloned()
        .collect()
}

pub fn plan_override_section<'a>(
    props: PlanOverrideProps<'_>,
    on_add: fn(String) -> Message,
    on_select: fn(String) -> Message,
    on_remove: fn(String) -> Message,
) -> Element<'a, Message> {
    // A plan type selected for editing counts as an override while it is
    // still pending (not stored yet), so it can be removed again.
    let mut rows: Vec<String> = props.existing.to_vec();
    if let Some(sel) = props.selected {
        if sel != props.default_key && !rows.iter().any(|r| r == sel) {
            rows.push(sel.to_string());
        }
    }
    let available = available_plan_names(props.all_names, &rows, props.default_key);
    let is_default_selected = match props.selected {
        None => true,
        Some(s) => s == props.default_key,
    };
    let choices: Vec<OverrideChoice> = available
        .into_iter()
        .map(|n| OverrideChoice { label: n.clone(), id: n })
        .collect();
    let entries: Vec<(String, String)> = rows
        .iter()
        .filter(|n| n.as_str() != props.default_key)
        .map(|n| (n.clone(), n.clone()))
        .collect();
    let default_row = Some((
        props.default_key.to_string(),
        t!("Style default").to_string(),
        is_default_selected,
    ));
    let selected = props.selected.map(str::to_string);
    override_list(
        t!("Plan type overrides").to_string(),
        choices,
        default_row,
        entries,
        selected,
        on_add,
        on_select,
        on_remove,
    )
}

/// One selectable entry of the add-dropdown (`id` is passed to the messages,
/// `label` is displayed).
#[derive(Clone, PartialEq, Eq)]
pub struct OverrideChoice {
    pub id: String,
    pub label: String,
}

impl std::fmt::Display for OverrideChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// Generic override section: add-dropdown (`choices`), optional default row
/// `(id, label, selected)` and the list of `entries` `(id, label)` with
/// Edit / Remove buttons. Used by the style forms (per plan type) and by the
/// plan type editor (per wall style).
#[allow(clippy::too_many_arguments)]
pub fn override_list<'a>(
    title: String,
    choices: Vec<OverrideChoice>,
    default_row: Option<(String, String, bool)>,
    entries: Vec<(String, String)>,
    selected: Option<String>,
    on_add: fn(String) -> Message,
    on_select: fn(String) -> Message,
    on_remove: fn(String) -> Message,
) -> Element<'a, Message> {
    let mut section = column![text(title).size(12)].spacing(4);
    section = section.push(
        pick_list(None::<OverrideChoice>, choices, |v: &OverrideChoice| v.label.clone())
            .placeholder(t!("Add exception…").to_string())
            .on_select(move |c: OverrideChoice| on_add(c.id))
            .text_size(11)
            .width(260),
    );

    let mut list = column![].spacing(2);
    if let Some((id, label, sel)) = default_row {
        list = list.push(override_row(label, sel, on_select(id), None));
    }
    for (id, label) in entries {
        let is_selected = selected.as_deref() == Some(id.as_str());
        list = list.push(override_row(
            label,
            is_selected,
            on_select(id.clone()),
            Some(on_remove(id)),
        ));
    }
    section = section.push(container(list).padding(4));
    section.into()
}

fn override_row<'a>(
    label: String,
    selected: bool,
    edit: Message,
    remove: Option<Message>,
) -> Element<'a, Message> {
    let mut r = row![
        text(label).size(11),
        Space::new().width(Fill),
        button(text(t!("Edit")).size(10))
            .style(if selected { button::primary } else { button::secondary })
            .padding([2, 8])
            .on_press(edit),
    ]
    .spacing(6)
    .align_y(iced::Center);
    if let Some(remove) = remove {
        r = r.push(
            button(text(t!("Remove")).size(10))
                .style(button::danger)
                .padding([2, 8])
                .on_press(remove),
        );
    }
    container(r)
        .padding([3, 6])
        .width(Fill)
        .style(move |theme: &Theme| container::Style {
            background: selected.then(|| {
                Background::Color(theme.palette().background.strong.color.scale_alpha(0.35))
            }),
            border: Border { radius: 3.0.into(), ..Default::default() },
            ..Default::default()
        })
        .into()
}


#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn available_excludes_existing_and_default_key() {
        let all = s(&["__default__", "A", "B", "C"]);
        let existing = s(&["B"]);
        assert_eq!(available_plan_names(&all, &existing, "__default__"), s(&["A", "C"]));
    }

    #[test]
    fn available_empty_when_all_have_exceptions() {
        let all = s(&["A", "B"]);
        assert!(available_plan_names(&all, &all, "").is_empty());
    }
}
