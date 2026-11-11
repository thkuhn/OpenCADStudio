//! AEC plan-kind status menu: plan kinds plus the session 2D / 3D / Alle
//! override in one menu (single status pill).

use iced::widget::{button, row, text};
use iced::{Element, Fill};
use crate::t;

use crate::app::{AecMessage, Message};
use crate::modules::aec::engine::display_component::RepresentationMode;
use crate::ui::statusbar::status_menu::Entry;

/// - `current_name`: name of the active `DisplayConfig` for the tab, if any.
/// - `plan_names`: all `DisplayConfig` names in `App::aec_plan_library`, in
///   library order.
/// - `representation_override`: session 2D/3D/Alle override, `None` = plan default.
pub fn menu_entries(
    current_name: Option<&str>,
    plan_names: Vec<String>,
    representation_override: Option<RepresentationMode>,
) -> Vec<Entry<'static>> {
    let mut entries: Vec<Entry<'static>> = vec![
        Entry::close(heading_row(t!("Planarten").into_owned())),
        Entry::close(plan_row(
            t!("Kein Plan").into_owned(),
            current_name.is_none(),
            Message::Aec(AecMessage::AecActiveDisplayConfigSelected(None)),
        )),
    ];

    entries.extend(plan_names.into_iter().map(|name| {
        let active = current_name == Some(name.as_str());
        let msg = Message::Aec(AecMessage::AecActiveDisplayConfigSelected(Some(name.clone())));
        Entry::close(plan_row(name, active, msg))
    }));

    entries.push(Entry::close(heading_row(t!("Darstellung").into_owned())));
    entries.push(Entry::close(plan_row(
        t!("Planart-Standard").into_owned(),
        representation_override.is_none(),
        Message::Aec(AecMessage::AecRepresentationOverrideSelected(None)),
    )));
    for (label, mode) in [
        ("2D", RepresentationMode::TwoD),
        ("3D", RepresentationMode::ThreeD),
        ("Alle", RepresentationMode::All),
    ] {
        entries.push(Entry::close(plan_row(
            label.to_string(),
            representation_override == Some(mode),
            Message::Aec(AecMessage::AecRepresentationOverrideSelected(Some(mode))),
        )));
    }
    entries.push(Entry::close(manage_row()));
    entries
}

/// Project pill menu: heading, active project (read-only) and a manage entry
/// that opens the Project Explorer.
pub fn project_menu_entries(active_project: Option<&str>) -> Vec<Entry<'static>> {
    let current: Element<'static, Message> = iced::widget::container(
        text(
            active_project
                .map(str::to_owned)
                .unwrap_or_else(|| t!("Kein Projekt").into_owned()),
        )
        .size(11),
    )
    .padding([4, 10])
    .into();
    let manage: Element<'static, Message> = button(text(t!("Verwalten...")).size(11))
        .on_press(Message::Aec(AecMessage::AecProjectExplorerOpen))
        .style(button::primary)
        .width(Fill)
        .padding([5, 10])
        .into();
    vec![
        Entry::close(heading_row(t!("Projekt").into_owned())),
        Entry::close(current),
        Entry::close(manage),
    ]
}

/// Left-aligned, bold section heading with a subtle background band.
fn heading_row(label: String) -> Element<'static, Message> {
    iced::widget::container(
        text(label)
            .size(11)
            .font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..iced::Font::DEFAULT
            })
            .align_x(iced::alignment::Horizontal::Left)
            .width(Fill),
    )
    .width(Fill)
    .padding([4, 10])
    .style(|theme: &iced::Theme| {
        let palette = theme.palette();
        iced::widget::container::Style {
            background: Some(palette.background.neutral.color.into()),
            text_color: Some(palette.background.neutral.text),
            ..Default::default()
        }
    })
    .into()
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
        .on_press(styles_open_message())
        .style(button::primary)
        .width(Fill)
        .padding([5, 10])
        .into()
}

/// Open `AEC_STYLES` at the *Planarten* node.
pub fn styles_open_message() -> Message {
    Message::Aec(AecMessage::AecStylesOpen(
        crate::modules::aec::ui::style_manager::StylesNode::PlanKinds,
    ))
}
