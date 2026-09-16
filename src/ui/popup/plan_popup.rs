//! AEC DisplayConfig (plan type) status menu.

use iced::widget::{button, row, text};
use iced::{Element, Fill};
use crate::t;

use crate::app::{AecMessage, Message};
use crate::ui::statusbar::status_menu::Entry;

/// - `current_name`: name of the active `DisplayConfig` for the tab, if any.
/// - `plan_names`: all `DisplayConfig` names in `App::aec_plan_library`, in
///   library order.
pub fn menu_entries(current_name: Option<&str>, plan_names: Vec<String>) -> Vec<Entry<'static>> {
    let mut entries: Vec<Entry<'static>> = vec![Entry::close(plan_row(
        t!("Kein Plan").into_owned(),
        current_name.is_none(),
        Message::Aec(AecMessage::AecActiveDisplayConfigSelected(None)),
    ))];

    entries.extend(plan_names.into_iter().map(|name| {
        let active = current_name == Some(name.as_str());
        let msg = Message::Aec(AecMessage::AecActiveDisplayConfigSelected(Some(name.clone())));
        Entry::close(plan_row(name, active, msg))
    }));

    entries.push(Entry::close(manage_row()));
    entries
}

fn plan_row(label: String, active: bool, msg: Message) -> Element<'static, Message> {
    let check = crate::ui::icons::themed_check_cell(active);

    let lbl = text(label).size(11);

    let content = row![check, lbl].spacing(6).align_y(iced::Center);

    button(content)
        .on_press(msg)
        .style(button::subtle)
        .width(Fill)
        .padding([4, 10])
        .into()
}

fn manage_row() -> Element<'static, Message> {
    button(text(t!("Manage...")).size(11))
        .on_press(Message::Aec(AecMessage::AecPlanManagerOpen))
        .style(button::primary)
        .width(Fill)
        .padding([5, 10])
        .into()
}
