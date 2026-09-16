//! AEC Material Manager — browse and edit materials held in the AEC
//! style library (`AEC_MATERIALMANAGER`).

use iced::widget::{
    button, column, combo_box, container, row, scrollable, text, text_input, Space,
};
use iced::{Background, Border, Color, Element, Fill, Theme};

use crate::app::{AecMessage, Message};
use crate::modules::aec::engine::library::{
    combined_material_entries_with_session, LibrarySource, StyleLibrary,
};
use crate::modules::aec::engine::material::Material;
use crate::modules::aec::engine::project::ProjectFile;
use crate::t;
use crate::tr;
use super::aec_ui_util::*;

/// Edit-buffer fields for the material form, owned by `App` and borrowed
/// here for rendering.
pub struct MaterialFormState<'a> {
    /// Whether the form should be shown at all (a material is selected, or
    /// "New" was pressed).
    pub open: bool,
    /// `true` while composing a not-yet-saved material (no id assigned yet).
    pub is_new: bool,
    /// Id of the material currently being edited, if any (saved materials only).
    pub editing_id: Option<&'a str>,
    /// Current name buffer.
    pub name: &'a str,
    /// Current hatch pattern name.
    pub hatch: &'a str,
    /// Current color hex (e.g. `"#RRGGBB"`).
    pub color: &'a str,
    /// Current line type name.
    pub line_type: &'a str,
    /// Current category buffer.
    pub category: &'a str,
    /// Current hatch colour (0xRRGGBB).
    pub hatch_color: u32,
    /// Current hatch scale buffer.
    pub hatch_scale: &'a str,
    /// Current render-material-ref buffer.
    pub render_material_ref: &'a str,
    /// Current hatch-angle buffer (degrees).
    pub hatch_angle: &'a str,
    /// `true` when `hatch_angle` is relative to the wall's own direction.
    pub hatch_angle_relative: bool,
    /// Whether the hatch pattern picker dropdown is open.
    pub hatch_picker_open: bool,
    /// Whether the color picker dropdown is open.
    pub color_picker_open: bool,
    /// Whether the hatch-color picker dropdown is open.
    pub hatch_color_picker_open: bool,
    /// List of all available line types in the document (name + ASCII art).
    pub linetype_items: &'a [crate::ui::properties::LinetypeItem],
    /// combo_box state built from `linetype_items`.
    pub linetype_combo: &'a combo_box::State<crate::ui::properties::LinetypeItem>,
}

pub fn view_window<'a>(
    library: &'a StyleLibrary,
    project: Option<&'a ProjectFile>,
    session: Option<&'a StyleLibrary>,
    selected_id: Option<&str>,
    filter: &str,
    material_form: MaterialFormState<'a>,
) -> Element<'a, Message> {
    let entries = combined_material_entries_with_session(project, session);
    let standard_ids: std::collections::HashSet<String> =
        crate::modules::aec::engine::library::load_or_seed()
            .materials
            .into_iter()
            .map(|m| m.id)
            .collect();

    let filter_lower = filter.to_lowercase();
    let filtered: Vec<_> = entries
        .into_iter()
        .filter(|e| {
            let m = &e.material;
            filter.is_empty()
                || m.name.to_lowercase().contains(&filter_lower)
                || m.id.to_lowercase().contains(&filter_lower)
                || m.category
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&filter_lower)
        })
        .collect();

    let selected_source = selected_id.and_then(|id| {
        filtered
            .iter()
            .find(|e| e.material.id == id)
            .map(|e| e.source)
    });
    let selected_has_standard_counterpart = selected_id
        .map(|id| standard_ids.contains(id))
        .unwrap_or(false);

    // Group by category: named categories sorted alphabetically, then
    // uncategorized materials under "Ohne Kategorie".
    let mut categories: Vec<String> = filtered
        .iter()
        .filter_map(|e| {
            e.material
                .category
                .as_ref()
                .map(|c| c.trim())
                .filter(|c| !c.is_empty())
                .map(|c| c.to_string())
        })
        .collect();
    categories.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    categories.dedup();

    let mut master_list = column![section_title(t!("Materials"))].spacing(2);

    if filtered.is_empty() {
        master_list = master_list.push(no_matches());
    } else {
        for cat in &categories {
            master_list = master_list.push(section_title(std::borrow::Cow::Owned(cat.clone())));
            for e in filtered.iter().filter(|e| {
                e.material
                    .category
                    .as_ref()
                    .map(|c| c.trim() == cat.as_str())
                    .unwrap_or(false)
            }) {
                master_list = master_list.push(material_row(
                    &e.material,
                    e.source,
                    selected_id == Some(e.material.id.as_str()),
                ));
            }
        }

        let uncategorized: Vec<_> = filtered
            .iter()
            .filter(|e| {
                e.material
                    .category
                    .as_ref()
                    .map(|c| c.trim().is_empty())
                    .unwrap_or(true)
            })
            .collect();
        if !uncategorized.is_empty() {
            master_list = master_list.push(section_title(t!("Ohne Kategorie")));
            for e in uncategorized {
                master_list = master_list.push(material_row(
                    &e.material,
                    e.source,
                    selected_id == Some(e.material.id.as_str()),
                ));
            }
        }
    }

    let sidebar = column![
        row![
            text_input(t!("Search materials…").as_ref(), filter)
                .on_input(|v| Message::Aec(AecMessage::AecStyleManagerFilter(v)))
                .size(11)
                .padding([4, 6]),
            button(text("+").size(11))
                .on_press(Message::Aec(AecMessage::AecStyleManagerMaterialNew))
                .padding([4, 8]),
        ]
        .spacing(4),
        scrollable(master_list),
    ]
    .spacing(8)
    .width(220);

    let detail = if material_form.open {
        material_form_view(
            library,
            material_form,
            selected_source,
            selected_has_standard_counterpart,
        )
    } else {
        container(text(t!("Select a material to edit or create a new one.")).style(muted))
            .width(Fill)
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill)
            .into()
    };

    row![sidebar, container(detail).padding(10).width(Fill)]
        .spacing(10)
        .padding(10)
        .into()
}

fn source_badge<'a>(source: LibrarySource) -> Element<'a, Message> {
    let label = match source {
        LibrarySource::Standard => t!("Standard"),
        LibrarySource::Project => t!("Projekt"),
        LibrarySource::Session => t!("Sitzung"),
    };
    container(text(label).size(9))
        .padding([1, 5])
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.strong.color.scale_alpha(0.55),
            )),
            text_color: Some(theme.palette().background.base.text.scale_alpha(0.85)),
            border: Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

fn material_row<'a>(
    material: &Material,
    source: LibrarySource,
    selected: bool,
) -> Element<'a, Message> {
    let rgb = material.line_color;
    let r = ((rgb >> 16) & 0xFF) as u8;
    let g = ((rgb >> 8) & 0xFF) as u8;
    let b = (rgb & 0xFF) as u8;
    let swatch = container(Space::new())
        .width(10)
        .height(10)
        .style(move |theme: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgb8(r, g, b))),
            border: Border {
                color: theme.palette().background.strong.color,
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        });

    button(
        row![
            swatch,
            column![
                row![
                    text(material.name.clone()).size(12),
                    Space::new().width(6),
                    source_badge(source),
                ]
                .spacing(4)
                .align_y(iced::Center),
                text(material.hatch_pattern.clone()).size(10).style(muted),
            ]
            .spacing(2),
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .on_press(Message::Aec(AecMessage::AecStyleManagerSelectMaterial(material.id.clone())))
    .style(list_style(selected))
    .padding([6, 9])
    .width(Fill)
    .into()
}

fn rgb_u32_to_acad(rgb: u32) -> acadrust::types::Color {
    acadrust::types::Color::Rgb {
        r: ((rgb >> 16) & 0xFF) as u8,
        g: ((rgb >> 8) & 0xFF) as u8,
        b: (rgb & 0xFF) as u8,
    }
}

fn material_form_view<'a>(
    library: &'a StyleLibrary,
    material_form: MaterialFormState<'a>,
    selected_source: Option<LibrarySource>,
    has_standard_counterpart: bool,
) -> Element<'a, Message> {
    let form_title = if material_form.is_new {
        t!("New Material")
    } else {
        t!("Material")
    };

    let mut actions = row![button(text(t!("Save")).size(11))
        .style(button::primary)
        .padding([5, 12])
        .on_press(Message::Aec(AecMessage::AecStyleManagerMaterialSave))]
    .spacing(8);

    if !material_form.is_new {
        actions = actions.push(
            button(text(t!("Duplizieren")).size(11))
                .padding([5, 12])
                .on_press(Message::Aec(AecMessage::AecStyleManagerMaterialDuplicate)),
        );
        actions = actions.push(
            button(text(t!("Delete")).size(11))
                .style(button::danger)
                .padding([5, 12])
                .on_press(Message::Aec(AecMessage::AecStyleManagerMaterialDelete)),
        );
        // → Standard: only pure project entries without a global counterpart.
        if selected_source == Some(LibrarySource::Project) && !has_standard_counterpart {
            actions = actions.push(
                button(text(t!("→ Standard")).size(11))
                    .padding([5, 12])
                    .on_press(Message::Aec(AecMessage::AecStyleManagerCopyMaterialToGlobal)),
            );
        }
        // → Projekt: only Standard entries (copy into the project library).
        if selected_source == Some(LibrarySource::Standard) {
            actions = actions.push(
                button(text(t!("→ Projekt")).size(11))
                    .padding([5, 12])
                    .on_press(Message::Aec(AecMessage::AecStyleManagerCopyMaterialToProject)),
            );
        }
    }

    let hatch_acad = rgb_u32_to_acad(material_form.hatch_color);

    let mut form = column![
        text(form_title).size(13),
        row![
            text(t!("Name")).size(10).style(muted).width(100),
            text_input("", material_form.name)
                .on_input(|v| Message::Aec(AecMessage::AecStyleManagerMaterialNameChanged(v)))
                .size(11)
                .padding([4, 6]),
        ]
        .spacing(8),
        row![
            text(t!("Category")).size(10).style(muted).width(100),
            text_input("", material_form.category)
                .on_input(|v| Message::Aec(AecMessage::AecStyleManagerMaterialCategoryChanged(v)))
                .size(11)
                .padding([4, 6]),
        ]
        .spacing(8),
        row![
            text(t!("Hatch pattern")).size(10).style(muted).width(100),
            super::aec_ui_util::hatch_pattern_field(
                material_form.hatch,
                material_form.hatch_picker_open,
                None,
                Message::Aec(AecMessage::AecStyleManagerMaterialHatchPickerToggle),
                |v| Message::Aec(AecMessage::AecStyleManagerMaterialHatchSelected(v)),
            ),
        ]
        .spacing(8),
        row![
            text(t!("Line color")).size(10).style(muted).width(100),
            container(crate::ui::color_select::color_selector(
                hex_to_acad_color(material_form.color),
                material_form.color_picker_open,
                crate::ui::color_select::ColorExtras {
                    by_layer: false,
                    by_block: false,
                    ..Default::default()
                },
                |v| Message::Aec(AecMessage::AecStyleManagerMaterialColorPicked(v)),
                Message::Aec(AecMessage::AecStyleManagerMaterialColorPickerToggle),
                Message::OpenColorWindow(
                    crate::app::ColorPickTarget::AecMaterial,
                    hex_to_acad_color(material_form.color),
                ),
            ))
            .width(180),
        ]
        .spacing(8),
        row![
            text(t!("Hatch color")).size(10).style(muted).width(100),
            container(crate::ui::color_select::color_selector(
                hatch_acad,
                material_form.hatch_color_picker_open,
                crate::ui::color_select::ColorExtras {
                    by_layer: false,
                    by_block: false,
                    ..Default::default()
                },
                |color| {
                    let value = match color {
                        acadrust::types::Color::Rgb { r, g, b } => {
                            ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
                        }
                        acadrust::types::Color::Index(i) => {
                            let (r, g, b) = acadrust::types::aci_table::aci_to_rgb(i)
                                .unwrap_or((255, 255, 255));
                            ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
                        }
                        _ => 0xFFFFFF,
                    };
                    Message::Aec(AecMessage::AecStyleManagerMaterialHatchColorChanged(value))
                },
                Message::Aec(AecMessage::AecStyleManagerMaterialHatchColorPickerToggle),
                Message::OpenColorWindow(
                    crate::app::ColorPickTarget::AecMaterialHatch,
                    hatch_acad,
                ),
            ))
            .width(180),
        ]
        .spacing(8),
        row![
            text(t!("Hatch scale")).size(10).style(muted).width(100),
            text_input("1.0", material_form.hatch_scale)
                .on_input(|v| Message::Aec(AecMessage::AecStyleManagerMaterialHatchScaleChanged(v)))
                .size(11)
                .padding([4, 6])
                .width(180),
        ]
        .spacing(8),
        row![
            text(t!("Hatch angle")).size(10).style(muted).width(100),
            text_input("0.0", material_form.hatch_angle)
                .on_input(|v| Message::Aec(AecMessage::AecStyleManagerMaterialHatchAngleChanged(v)))
                .size(11)
                .padding([4, 6])
                .width(90),
            iced::widget::checkbox(material_form.hatch_angle_relative)
                .on_toggle(|_| Message::Aec(AecMessage::AecStyleManagerMaterialHatchAngleRelativeToggle))
                .size(13),
            text(t!("Relativ zur Wand")).size(11),
        ]
        .spacing(8)
        .align_y(iced::Center),
        row![
            text(t!("Line type")).size(10).style(muted).width(100),
            super::aec_ui_util::linetype_field(
                material_form.line_type,
                material_form.linetype_items,
                material_form.linetype_combo,
                |v| Message::Aec(AecMessage::AecStyleManagerMaterialLineTypeChanged(v)),
            ),
        ]
        .spacing(8),
        row![
            text(t!("Render ref")).size(10).style(muted).width(100),
            text_input("", material_form.render_material_ref)
                .on_input(|v| Message::Aec(AecMessage::AecStyleManagerMaterialRenderRefChanged(v)))
                .size(11)
                .padding([4, 6]),
        ]
        .spacing(8),
        actions,
    ]
    .spacing(7);

    // Usage overview for saved materials only.
    if !material_form.is_new {
        if let Some(id) = material_form.editing_id {
            let usages = library.materials_using(id);
            form = form.push(text(t!("Verwendet in:")).size(11).style(muted));
            if usages.is_empty() {
                form = form.push(
                    text(t!("Wird derzeit nicht verwendet."))
                        .size(10)
                        .style(muted),
                );
            } else {
                for (ws, layer_idx) in usages {
                    form = form.push(
                        text(format!(
                            "{} {}",
                            ws.style.name,
                            tr!("aec", "layer-n", n = { (layer_idx + 1) as i32 })
                        ))
                        .size(10),
                    );
                }
            }
        }
    }

    form.into()
}

