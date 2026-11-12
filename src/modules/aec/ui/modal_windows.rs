//! Small confirmation dialog windows for AEC modals.

use iced::widget::{button, column, container, row, text};
use iced::{Background, Element, Theme};
use std::borrow::Cow;

use crate::app::Message;
use crate::modules::aec::AecMessage;
use crate::t;
pub(super) fn dialog_button<'a>(
    label: impl Into<Cow<'a, str>>,
    message: Message,
    style: fn(&Theme, button::Status) -> button::Style,
) -> Element<'a, Message> {
    button(text(label.into()).size(13))
        .on_press(message)
        .style(style)
        .padding([6, 18])
        .into()
}

pub(super) fn dialog_body_style(theme: &Theme) -> container::Style {
    let palette = theme.palette();
    container::Style {
        background: Some(Background::Color(palette.background.base.color)),
        text_color: Some(palette.background.base.text),
        ..Default::default()
    }
}

/// Warning shown before a lossy Save-As: the drawing carries unsupported
/// (AEC / application) objects that survive only as verbatim source-version
/// bytes, so saving to a different version or to DXF would drop them. Offers to
/// save in the source version (keep them) or proceed (drop them).
pub(super) fn aec_drop_dialog_window(
    count: usize,
    target: &str,
    src_version: &str,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'static, Message> {
    let body_text = t!(
        "This drawing contains %{count} AEC/Civil objects that \"%{target}\" cannot store, so they will not be saved.\n\nTo keep them, save in the source version (%{src_version}).",
        count = count,
        target = target,
        src_version = src_version,
    );

    container(
        column![
            text(body_text).size(13),
            iced::widget::Space::new().height(20),
            row![
                dialog_button(
                    t!("Save in source version"),
                    Message::AecDropSameVersion,
                    button::primary
                ),
                iced::widget::Space::new().width(8),
                dialog_button(t!("Save anyway"), Message::AecDropProceed, button::warning),
                iced::widget::Space::new().width(8),
                dialog_button(t!("Back"), Message::AecDropBack, button::secondary),
            ],
        ]
        .spacing(0),
    )
    .style(dialog_body_style)
    .center_x(sizing.width)
    .center_y(sizing.height)
    .padding([24, 28])
    .into()
}

/// Confirm overwriting an entry that already exists (with different
/// content) in the target library during an AEC Style Manager copy (Step
/// 9). "Overwrite" proceeds with the copy; "Cancel" leaves both libraries
/// untouched.
pub(super) fn aec_style_copy_conflict_window(
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'static, Message> {
    let body_text = t!(
        "An entry with this name/id already exists in the target library with different content.\n\nOverwrite it?"
    );

    container(
        column![
            text(body_text).size(13),
            iced::widget::Space::new().height(20),
            row![
                dialog_button(
                    t!("Overwrite"),
                    Message::Aec(AecMessage::AecStyleManagerCopyConflictConfirm(true)),
                    button::danger
                ),
                iced::widget::Space::new().width(8),
                dialog_button(
                    t!("Cancel"),
                    Message::Aec(AecMessage::AecStyleManagerCopyConflictConfirm(false)),
                    button::secondary
                ),
            ],
        ]
        .spacing(0),
    )
    .style(dialog_body_style)
    .center_x(sizing.width)
    .center_y(sizing.height)
    .padding([24, 28])
    .into()
}

/// Blocking prompt when Architecture tools are used without an active
/// project. "Open Project" / "New Project" route into the Project Explorer
/// flow; closing the modal leaves the entry point unopened.
pub(super) fn aec_project_required_window(
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'static, Message> {
    let body_text = t!(
        "Architecture tools require an active project.\n\nOpen an existing project or create a new one to continue."
    );

    container(
        column![
            text(body_text).size(13),
            iced::widget::Space::new().height(20),
            row![
                dialog_button(
                    t!("Open Project"),
                    Message::Aec(AecMessage::AecProjectExplorerLoad),
                    button::primary
                ),
                iced::widget::Space::new().width(8),
                dialog_button(
                    t!("New Project"),
                    Message::Aec(AecMessage::AecProjectExplorerNew),
                    button::secondary
                ),
            ],
        ]
        .spacing(0),
    )
    .style(dialog_body_style)
    .center_x(sizing.width)
    .center_y(sizing.height)
    .padding([24, 28])
    .into()
}
