use iced::widget::{button, container, text};
use iced::{Element, Theme};
use crate::app::Message;
use crate::t;

/// Parses a `"#RRGGBB"` (or bare `RRGGBB`) hex string into a true colour,
/// falling back to white for anything that doesn't parse.
pub fn hex_to_acad_color(hex: &str) -> acadrust::types::Color {
    let digits = hex.trim().trim_start_matches('#');
    let value = u32::from_str_radix(digits, 16).unwrap_or(0xFFFFFF);
    acadrust::types::Color::Rgb {
        r: ((value >> 16) & 0xFF) as u8,
        g: ((value >> 8) & 0xFF) as u8,
        b: (value & 0xFF) as u8,
    }
}

pub fn muted(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.65)),
    }
}

pub fn list_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        if selected {
            button::primary(theme, status)
        } else {
            button::subtle(theme, status)
        }
    }
}

pub fn section_title<'a>(label: std::borrow::Cow<'a, str>) -> Element<'a, Message> {
    container(text(label).size(11).style(muted))
        .padding([4, 2])
        .into()
}

pub fn no_matches<'a>() -> Element<'a, Message> {
    container(text(t!("No matches")).size(11).style(muted))
        .padding([8, 12])
        .into()
}
