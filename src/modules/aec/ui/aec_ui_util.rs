use iced::widget::{button, canvas, column, combo_box, container, row, scrollable, text, Space};
use iced::{Element, Theme};
use crate::app::Message;
use crate::t;
use crate::tr;

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
#[allow(dead_code)]
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

/// Encode an `AcadColor` for AEC style-editor text buffers so `Index` /
/// `ByLayer` / `ByBlock` round-trip losslessly. Only `Rgb` falls back to a
/// bare `"RRGGBB"` hex string (the historical buffer format).
pub fn acad_color_to_editor_string(color: acadrust::types::Color) -> String {
    match color {
        acadrust::types::Color::ByLayer => "ByLayer".to_string(),
        acadrust::types::Color::ByBlock => "ByBlock".to_string(),
        acadrust::types::Color::None => "None".to_string(),
        acadrust::types::Color::Index(i) => format!("ACI{i}"),
        acadrust::types::Color::Rgb { r, g, b } => format!("{r:02X}{g:02X}{b:02X}"),
    }
}

/// Parse an AEC style-editor colour buffer back into an `AcadColor`.
/// Accepts the lossless tokens produced by [`acad_color_to_editor_string`],
/// bare/`#`-prefixed hex (historical RGB buffers), and plain ACI integers.
/// Blank input yields `None` ("no override").
pub fn editor_string_to_acad_color(text: &str) -> Option<acadrust::types::Color> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.eq_ignore_ascii_case("ByLayer") {
        return Some(acadrust::types::Color::ByLayer);
    }
    if trimmed.eq_ignore_ascii_case("ByBlock") {
        return Some(acadrust::types::Color::ByBlock);
    }
    if trimmed.eq_ignore_ascii_case("None") {
        return Some(acadrust::types::Color::None);
    }
    let aci_body = trimmed
        .strip_prefix("ACI")
        .or_else(|| trimmed.strip_prefix("aci"))
        .or_else(|| trimmed.strip_prefix("I:"))
        .or_else(|| trimmed.strip_prefix("i:"));
    if let Some(body) = aci_body {
        if let Ok(n) = body.trim().parse::<i16>() {
            return Some(acadrust::types::Color::from_index(n));
        }
    }
    let digits = trimmed.trim_start_matches('#');
    if digits.len() == 6 {
        if let Ok(value) = u32::from_str_radix(digits, 16) {
            return Some(acadrust::types::Color::Rgb {
                r: ((value >> 16) & 0xFF) as u8,
                g: ((value >> 8) & 0xFF) as u8,
                b: (value & 0xFF) as u8,
            });
        }
    }
    if let Ok(n) = trimmed.parse::<i16>() {
        return Some(acadrust::types::Color::from_index(n));
    }
    None
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

/// Edit-buffer fields for a single `ComponentStyleOverride` form
/// (plan-manager phase styles and wall-style per-slot style overrides).
#[derive(Clone, Copy)]
pub struct StyleEditorFormState<'a> {
    pub line_type: &'a str,
    /// All available line types in the document (name + ASCII art), used
    /// by the same `combo_box`+preview widget as the Layer Manager.
    pub linetype_items: &'a [crate::ui::properties::LinetypeItem],
    /// `combo_box` state built from `linetype_items`.
    pub linetype_combo: &'a iced::widget::combo_box::State<crate::ui::properties::LinetypeItem>,
    /// Colour editor text (see [`acad_color_to_editor_string`]).
    pub line_color: &'a str,
    pub line_color_picker_open: bool,
    pub hatch_pattern: &'a str,
    pub hatch_picker_open: bool,
    pub hatch_color: &'a str,
    pub hatch_color_picker_open: bool,
    pub fill_color: &'a str,
    pub fill_color_picker_open: bool,
}

/// Shared inline `ComponentStyleOverride` editor form. Line-type uses the
/// shared Layer-Manager-style `combo_box`+preview widget; every colour field
/// uses the shared `color_selector` swatch+name widget and the lossless
/// [`acad_color_to_editor_string`] / [`editor_string_to_acad_color`] helpers.
pub fn style_editor_form<'a>(
    title: String,
    editor: StyleEditorFormState<'a>,
    on_line_type: impl Fn(String) -> Message + 'static,
    on_line_color: impl Fn(String) -> Message + 'static,
    on_line_color_toggle: Message,
    line_color_more_target: crate::app::ColorPickTarget,
    on_hatch_pattern: impl Fn(String) -> Message + 'a,
    on_hatch_picker_toggle: Message,
    on_hatch_color: impl Fn(String) -> Message + 'static,
    on_hatch_color_toggle: Message,
    hatch_color_more_target: crate::app::ColorPickTarget,
    on_fill_color: impl Fn(String) -> Message + 'static,
    on_fill_color_toggle: Message,
    fill_color_more_target: crate::app::ColorPickTarget,
) -> Element<'a, Message> {
    let line_acad_color =
        editor_string_to_acad_color(editor.line_color).unwrap_or(acadrust::types::Color::ByLayer);
    let hatch_acad_color =
        editor_string_to_acad_color(editor.hatch_color).unwrap_or(acadrust::types::Color::ByLayer);
    let fill_acad_color =
        editor_string_to_acad_color(editor.fill_color).unwrap_or(acadrust::types::Color::ByLayer);
    container(
        column![
            text(title).size(11),
            row![
                text(t!("Linientyp")).size(10).style(muted).width(90),
                linetype_field(
                    editor.line_type,
                    editor.linetype_items,
                    editor.linetype_combo,
                    on_line_type,
                ),
            ]
            .spacing(8),
            row![
                text(t!("Linienfarbe")).size(10).style(muted).width(90),
                container(crate::ui::color_select::color_selector(
                    line_acad_color,
                    editor.line_color_picker_open,
                    crate::ui::color_select::ColorExtras::default(),
                    move |c| on_line_color(acad_color_to_editor_string(c)),
                    on_line_color_toggle,
                    Message::OpenColorWindow(line_color_more_target, line_acad_color),
                ))
                .width(180),
            ]
            .spacing(8),
            row![
                text(t!("Schraffurmuster")).size(10).style(muted).width(90),
                hatch_pattern_field(
                    editor.hatch_pattern,
                    editor.hatch_picker_open,
                    Some(tr!("aec", "inherit")),
                    on_hatch_picker_toggle,
                    on_hatch_pattern,
                ),
            ]
            .spacing(8),
            row![
                text(t!("Schraffurfarbe")).size(10).style(muted).width(90),
                container(crate::ui::color_select::color_selector(
                    hatch_acad_color,
                    editor.hatch_color_picker_open,
                    crate::ui::color_select::ColorExtras::default(),
                    move |c| on_hatch_color(acad_color_to_editor_string(c)),
                    on_hatch_color_toggle,
                    Message::OpenColorWindow(hatch_color_more_target, hatch_acad_color),
                ))
                .width(180),
            ]
            .spacing(8),
            row![
                text(t!("Füllfarbe")).size(10).style(muted).width(90),
                container(crate::ui::color_select::color_selector(
                    fill_acad_color,
                    editor.fill_color_picker_open,
                    crate::ui::color_select::ColorExtras::default(),
                    move |c| on_fill_color(acad_color_to_editor_string(c)),
                    on_fill_color_toggle,
                    Message::OpenColorWindow(fill_color_more_target, fill_acad_color),
                ))
                .width(180),
            ]
            .spacing(8),
        ]
        .spacing(6),
    )
    .padding(8)
    .style(container::bordered_box)
    .into()
}

/// Hatch-pattern picker with optional inherit placeholder when `current` is empty.
pub fn hatch_pattern_field<'a>(
    current: &'a str,
    open: bool,
    inherit_label: Option<String>,
    on_toggle: Message,
    on_select: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let inherit = inherit_label.clone();
    let label = if current.is_empty() {
        inherit.clone().unwrap_or_else(|| "SOLID".to_string())
    } else {
        current.to_string()
    };
    let head = button(
        row![
            text(label).size(11),
            Space::new(),
            if open {
                text("▲").size(9)
            } else {
                text("▼").size(9)
            },
        ]
        .align_y(iced::Center),
    )
    .on_press(on_toggle)
    .style(button::subtle)
    .padding([4, 6])
    .width(180);

    if !open {
        return head.into();
    }

    let mut grid = column![].spacing(4);
    if inherit.is_some() {
        grid = grid.push(
            button(text(inherit.unwrap_or_else(|| tr!("aec", "inherit"))).size(10))
                .on_press(on_select(String::new()))
                .style(if current.is_empty() {
                    button::primary
                } else {
                    button::subtle
                })
                .padding([3, 6]),
        );
    }
    let patterns = crate::ui::properties::filtered_hatch_patterns("");
    for pair in patterns.chunks(2) {
        let mut cards = row![].spacing(4);
        for entry in pair {
            let selected = current.eq_ignore_ascii_case(&entry.name);
            let name = entry.name.clone();
            let preview = canvas(crate::ui::properties::HatchPatternPreview {
                pattern: entry.gpu.clone(),
            })
            .width(70)
            .height(36);
            let card = button(
                column![
                    preview,
                    text(crate::ui::text_util::elide(&entry.name, 12)).size(9),
                ]
                .spacing(2)
                .align_x(iced::Center),
            )
            .on_press(on_select(name))
            .style(if selected {
                button::primary
            } else {
                button::subtle
            })
            .padding(3)
            .width(84);
            cards = cards.push(card);
        }
        grid = grid.push(cards);
    }

    column![head, scrollable(grid).height(180)].spacing(4).into()
}
