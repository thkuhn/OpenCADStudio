//! AEC Material Manager — browse and edit materials held in the AEC
//! style library (`AEC_MATERIALMANAGER`).

use iced::widget::{
    column, combo_box, container, row, text, text_input,
};
use iced::{Element, Fill};

use crate::app::{AecColorPickTarget, AecMessage, Message};
use crate::modules::aec::engine::library::{
    combined_material_entries_with_session, LibrarySource, StyleLibrary,
};
use crate::modules::aec::engine::project::ProjectFile;
use crate::modules::aec::engine::style_common::StyleOrigin;
use crate::t;
use crate::tr;
use crate::modules::aec::ui::aec_ui_util::*;
use crate::modules::aec::ui::style_manager::actions::{self, ActionMessages};
use crate::modules::aec::ui::style_manager::list::{self, filter_entries, ListEntry, ListSpec};
use crate::modules::aec::ui::style_manager::StyleParts;

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

pub fn parts<'a>(
    library: &'a StyleLibrary,
    project: Option<&'a ProjectFile>,
    session: Option<&'a StyleLibrary>,
    selected_id: Option<&'a str>,
    filter: &'a str,
    material_form: MaterialFormState<'a>,
) -> StyleParts<'a> {
    let entries = combined_material_entries_with_session(project, session);
    let standard_ids: std::collections::HashSet<String> =
        crate::modules::aec::engine::library::load_or_seed()
            .materials
            .into_iter()
            .map(|m| m.style.id)
            .collect();

    let all: Vec<(ListEntry, LibrarySource)> = entries
        .iter()
        .map(|e| {
            (
                ListEntry {
                    id: e.material.id.clone(),
                    label: e.material.name.clone(),
                    detail: Some(e.material.hatch_pattern.clone()),
                    origin: StyleOrigin::from(e.source),
                    group: e.material.group.clone(),
                    swatch: Some(e.material.line_color),
                },
                e.source,
            )
        })
        .collect();
    let rows = filter_entries(all.iter().map(|(e, _)| e.clone()).collect(), filter);
    let selected_source = selected_id.and_then(|id| all.iter().find(|(e, _)| e.id == id).map(|(_, s)| *s));
    let has_standard = selected_id.map(|id| standard_ids.contains(id)).unwrap_or(false);

    let list = list::view(
        ListSpec {
            title: t!("Materials").into_owned(),
            search_hint: t!("Search materials…").into_owned(),
            new_label: t!("New Material").into_owned(),
            filter,
            selected: selected_id,
            on_filter: |v| Message::Aec(AecMessage::AecStyleManagerFilter(v)),
            on_select: |id| Message::Aec(AecMessage::AecStyleManagerSelectMaterial(id)),
            on_new: Message::Aec(AecMessage::AecStyleManagerMaterialNew),
        },
        &rows,
    );

    let open = material_form.open;
    let is_new = material_form.is_new;
    let form = if open {
        material_form_view(library, material_form)
    } else {
        container(text(t!("Select a material to edit or create a new one.")).style(muted))
            .width(Fill)
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill)
            .into()
    };
    let actions = open.then(|| {
        actions::view(
            ActionMessages {
                save_apply: Message::Aec(AecMessage::AecStyleManagerMaterialSaveAndApply),
                save: Message::Aec(AecMessage::AecStyleManagerMaterialSave),
                duplicate: Message::Aec(AecMessage::AecStyleManagerMaterialDuplicate),
                delete: Message::Aec(AecMessage::AecStyleManagerMaterialDelete),
                to_standard: Message::Aec(AecMessage::AecStyleManagerCopyMaterialToGlobal),
                to_project: Message::Aec(AecMessage::AecStyleManagerCopyMaterialToProject),
            },
            is_new,
            selected_source.map(StyleOrigin::from),
            has_standard,
        )
    });
    StyleParts { list, form, actions }
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
) -> Element<'a, Message> {
    let form_title = if material_form.is_new {
        t!("New Material")
    } else {
        t!("Material")
    };

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
            crate::modules::aec::ui::aec_ui_util::hatch_pattern_field(
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
                    crate::app::ColorPickTarget::Aec(AecColorPickTarget::Material),
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
                    crate::app::ColorPickTarget::Aec(AecColorPickTarget::MaterialHatch),
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
            crate::modules::aec::ui::aec_ui_util::linetype_field(
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

