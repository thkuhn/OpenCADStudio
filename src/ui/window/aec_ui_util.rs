use iced::widget::{button, combo_box, container, text};
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

/// Encode a true/indexed colour as a bare `"RRGGBB"` hex string (no `#`
/// prefix), the inverse of [`hex_to_acad_color`]. Indexed colours are
/// resolved to their RGB value via the ACI table first.
pub fn acad_color_to_hex(color: acadrust::types::Color) -> String {
    let (r, g, b) = match color {
        acadrust::types::Color::Rgb { r, g, b } => (r, g, b),
        acadrust::types::Color::Index(i) => {
            acadrust::types::aci_table::aci_to_rgb(i).unwrap_or((255, 255, 255))
        }
        _ => (255, 255, 255),
    };
    format!("{r:02X}{g:02X}{b:02X}")
}

/// Shared "linetype preview" combo box: shows the line-type name plus its
/// ASCII-art pattern (mirrors the Layer Manager's line-type combo, see
/// `LinetypeItem`). `line_type` is the currently selected name (empty =
/// "ByLayer"); `on_select` is invoked with the newly-picked name.
pub fn linetype_field<'a>(
    line_type: &'a str,
    linetype_items: &'a [crate::ui::properties::LinetypeItem],
    linetype_combo: &'a combo_box::State<crate::ui::properties::LinetypeItem>,
    on_select: impl Fn(String) -> Message + 'static,
) -> Element<'a, Message> {
    let display = if line_type.is_empty() { "ByLayer" } else { line_type };
    let selected = linetype_items
        .iter()
        .find(|item| item.name.eq_ignore_ascii_case(display))
        .cloned();
    combo_box(
        linetype_combo,
        "ByLayer",
        selected.as_ref(),
        move |item: crate::ui::properties::LinetypeItem| on_select(item.name),
    )
    .size(11)
    .padding([4, 6])
    .width(180)
    .into()
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
