//! Status-bar 2D / 3D / Alle representation filter.

use iced::widget::{button, row, text};
use iced::{Element, Fill};
use crate::t;

use crate::app::Message;
use crate::modules::aec::engine::display_component::RepresentationMode;
use crate::ui::statusbar::status_menu::Entry;

pub fn menu_entries(current: Option<RepresentationMode>) -> Vec<Entry<'static>> {
    let mut entries = vec![Entry::close(mode_row(
        t!("Planart").into_owned(),
        current.is_none(),
        Message::AecRepresentationOverrideSelected(None),
    ))];
    for (label, mode) in [
        (t!("2D").into_owned(), RepresentationMode::TwoD),
        (t!("3D").into_owned(), RepresentationMode::ThreeD),
        (t!("Alle").into_owned(), RepresentationMode::All),
    ] {
        let active = current == Some(mode);
        entries.push(Entry::close(mode_row(
            label,
            active,
            Message::AecRepresentationOverrideSelected(Some(mode)),
        )));
    }
    entries
}

fn mode_row(label: String, active: bool, msg: Message) -> Element<'static, Message> {
    let check = crate::ui::icons::themed_check_cell(active);
    let content = row![check, text(label).size(11)]
        .spacing(6)
        .align_y(iced::Center);
    button(content)
        .on_press(msg)
        .style(button::subtle)
        .width(Fill)
        .padding([4, 10])
        .into()
}
