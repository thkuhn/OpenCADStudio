//! Opening Style Manager modal: list, form, slot table and symbol preview.

use iced::mouse;
use iced::widget::canvas::{Frame, Path, Program, Stroke};
use iced::widget::{button, canvas, column, container, pick_list, row, scrollable, text, text_input, Space};
use iced::{Border, Color as IcedColor, Element, Fill as FillLen, Point, Rectangle, Theme};
use std::collections::HashMap;

use crate::app::Message;
use crate::modules::aec::engine::display_component::OpeningComponentSlot;
use crate::modules::aec::engine::library::{LibrarySource, StyleLibrary};
use crate::modules::aec::engine::opening_display::BakedPath;
use crate::modules::aec::engine::opening_shape::OpeningShape;
use crate::modules::aec::engine::opening_style::{
    FamilyPlacement, OpeningCellKind, OpeningEditorGroup, OpeningLookRole, OpeningProfilePart,
    construction_from_division, OpeningStyle, LeafOperation,
};
use crate::modules::aec::engine::openings::{OpeningKind, SwingSide};
use crate::modules::aec::engine::project::ProjectFile;
use crate::modules::aec::message::AecMessage;
use crate::modules::aec::state::{
    AecOpeningCellBuffer, AecOpeningIsometric, AecOpeningLookBuffer, AecOpeningPreviewMode,
    StylePickerTarget,
};
use crate::modules::aec::styles::opening_style_manager::{
    cached_preview_baked_paths_for_mode, format_dim, preview_project, PreviewCamera3d,
    PREVIEW_WALL_THICKNESS,
};
use crate::modules::aec::engine::style_common::StyleOrigin;
use crate::modules::aec::ui::aec_ui_util::muted;
use crate::modules::aec::ui::style_manager::actions::{self, ActionMessages};
use crate::modules::aec::ui::style_manager::list::{self, filter_entries, ListEntry, ListSpec};
use crate::modules::aec::ui::style_manager::StyleParts;
use crate::t;
use crate::tr;

pub(crate) const OPENING_EDITOR_SCROLL_ID: &str = "aec-opening-editor-tab-scroll";

pub struct OpeningStyleFormState<'a> {
    pub open: bool,
    pub is_new: bool,
    pub preview_mode: AecOpeningPreviewMode,
    pub editor_tab: OpeningEditorGroup,
    pub component_enabled: &'a HashMap<OpeningEditorGroup, bool>,
    pub profile_ids: &'a HashMap<OpeningProfilePart, String>,
    pub look_buffers: &'a HashMap<OpeningLookRole, AecOpeningLookBuffer>,
    pub look_color_picker_role: Option<OpeningLookRole>,
    pub selected_look_role: Option<OpeningLookRole>,
    pub linetype_items: &'a [crate::ui::properties::LinetypeItem],
    pub linetype_combo: &'a iced::widget::combo_box::State<crate::ui::properties::LinetypeItem>,
    pub name: &'a str,
    pub parent_id: Option<&'a str>,
    pub parent_name: Option<&'a str>,
    pub kind: OpeningKind,
    pub shape: OpeningShape,
    pub width: &'a str,
    pub height: &'a str,
    pub sill: &'a str,
    pub frame: &'a str,
    pub frame_depth: &'a str,
    pub frame_offset: &'a str,
    pub leaf_face: &'a str,
    pub leaf_depth: &'a str,
    pub leaf_rebate: &'a str,
    pub leaf_lateral: &'a str,
    pub glazing_thickness: &'a str,
    pub glazing_inset: &'a str,
    pub ext_sill_thickness: &'a str,
    pub ext_sill_overhang: &'a str,
    pub ext_sill_upstand: &'a str,
    pub ext_sill_side_left: &'a str,
    pub ext_sill_side_right: &'a str,
    pub int_board_thickness: &'a str,
    pub int_board_overhang: &'a str,
    pub int_board_upstand: &'a str,
    pub int_board_side_left: &'a str,
    pub int_board_side_right: &'a str,
    pub threshold_height: &'a str,
    pub threshold_width: &'a str,
    pub shutter_height: &'a str,
    pub shutter_depth: &'a str,
    pub shutter_seat: &'a str,
    pub shutter_install: &'a str,
    pub lintel_height: &'a str,
    pub lintel_left: &'a str,
    pub lintel_right: &'a str,
    pub vertical: &'a str,
    pub horizontal: &'a str,
    pub vertical_muntins: &'a str,
    pub horizontal_muntins: &'a str,
    pub cells: &'a [AecOpeningCellBuffer],
    pub selected_cell: Option<usize>,
    pub vertical_positions: &'a [String],
    pub horizontal_positions: &'a [String],
    pub vertical_muntin_positions: &'a [String],
    pub horizontal_muntin_positions: &'a [String],
    pub plan_block: &'a str,
    pub plan_placement: FamilyPlacement,
    pub section_block: &'a str,
    pub section_placement: FamilyPlacement,
    pub elevation_block: &'a str,
    pub elevation_placement: FamilyPlacement,
    pub angle: &'a str,
    pub spring: &'a str,
    pub hinge: LeafOperation,
    pub swing_side: SwingSide,
    pub authored_grid: Option<&'a crate::modules::aec::engine::construction_grid::ConstructionGrid>,
    pub grid_path: &'a [(u32, u32)],
    pub selected_grid_edge: Option<(bool, usize)>,
    pub grid_measure: Option<&'a str>,
    pub preview_isometric: AecOpeningIsometric,
    pub preview_zoom: f64,
    pub preview_pan_x: f64,
    pub preview_pan_y: f64,
    pub(crate) preview_model_cache: &'a std::cell::RefCell<
        crate::modules::aec::styles::opening_style_manager::OpeningPreviewModelCache,
    >,
}

pub fn parts<'a>(
    library: &'a StyleLibrary,
    project: Option<&'a ProjectFile>,
    session: Option<&'a StyleLibrary>,
    selected_id: Option<&'a str>,
    filter: &'a str,
    form: OpeningStyleFormState<'a>,
) -> StyleParts<'a> {
    let entries = crate::modules::aec::engine::library::combined_opening_style_entries_with_session(
        project, session,
    );
    let standard_ids: std::collections::HashSet<String> =
        crate::modules::aec::engine::library::load_or_seed()
            .opening_styles
            .into_iter()
            .map(|s| s.style.id)
            .collect();
    let source_by_id: std::collections::HashMap<String, LibrarySource> = entries
        .iter()
        .map(|e| (e.opening_style.style.id.clone(), e.source))
        .collect();

    let mut merged = StyleLibrary::empty();
    for e in &entries {
        merged.upsert_opening_style(e.opening_style.clone());
    }

    let all: Vec<ListEntry> = merged
        .opening_style_tree()
        .into_iter()
        .map(|node| ListEntry {
            id: node.style.style.id.clone(),
            label: format!("{}{}", "  ".repeat(node.depth), node.style.style.name),
            detail: Some(node.style.kind.as_str().to_string()),
            origin: StyleOrigin::from(
                source_by_id
                    .get(&node.style.style.id)
                    .copied()
                    .unwrap_or(LibrarySource::Standard),
            ),
            group: None,
            swatch: None,
        })
        .collect();
    let rows = filter_entries(all, filter);

    let list = list::view(
        ListSpec {
            title: t!("Opening Styles").into_owned(),
            search_hint: t!("Search opening styles…").into_owned(),
            new_label: t!("+ Neuer Öffnungsstil").into_owned(),
            filter,
            selected: selected_id,
            on_filter: |s| Message::Aec(AecMessage::AecOpeningStyleManagerFilter(s)),
            on_select: |id| Message::Aec(AecMessage::AecOpeningStyleManagerSelect(id)),
            on_new: Message::Aec(AecMessage::AecOpeningStyleManagerNew),
        },
        &rows,
    );

    let selected_source = selected_id.and_then(|id| source_by_id.get(id).copied());
    let has_standard = selected_id.is_some_and(|id| standard_ids.contains(id));
    let open = form.open;
    let is_new = form.is_new;

    let detail: Element<'a, Message> = if form.open {
        form_view(library, form)
    } else {
        container(text(t!("Select an opening style to edit or create a new one.")).style(muted))
            .width(FillLen)
            .height(FillLen)
            .center_x(FillLen)
            .center_y(FillLen)
            .into()
    };
    let actions = open.then(|| {
        actions::view(
            ActionMessages {
                save_apply: Message::Aec(AecMessage::AecOpeningStyleManagerSaveAndApply),
                save: Message::Aec(AecMessage::AecOpeningStyleManagerSave),
                duplicate: Message::Aec(AecMessage::AecOpeningStyleManagerDuplicate),
                delete: Message::Aec(AecMessage::AecOpeningStyleManagerDelete),
                to_standard: Message::Aec(AecMessage::AecStyleManagerCopyOpeningStyleToGlobal),
                to_project: Message::Aec(AecMessage::AecStyleManagerCopyOpeningStyleToProject),
            },
            is_new,
            selected_source.map(StyleOrigin::from),
            has_standard,
        )
    });
    StyleParts { list, form: detail, actions }
}

fn form_view<'a>(library: &'a StyleLibrary, form: OpeningStyleFormState<'a>) -> Element<'a, Message> {
    let parent_label = form.parent_name.unwrap_or("(None)");
    let kinds: Vec<String> = vec![
        OpeningKind::Window.as_str().to_string(),
        OpeningKind::Door.as_str().to_string(),
        OpeningKind::Breakthrough.as_str().to_string(),
    ];
    let shapes: Vec<String> = OpeningShape::catalogue()
        .iter()
        .map(|s| s.as_str().to_string())
        .collect();
    let height_locked = matches!(
        form.shape,
        OpeningShape::Circle | OpeningShape::Triangle(crate::modules::aec::engine::opening_shape::TriangleVariant::Equilateral)
    );

    let mut detail_col = column![
        row![
            text(t!("Name")).size(10).style(muted).width(48),
            text_input("", form.name)
                .on_input(|v| Message::Aec(AecMessage::AecOpeningStyleManagerNameChanged(v)))
                .size(11)
                .padding([4, 6])
                .width(iced::Length::FillPortion(3)),
            text(t!("Parent")).size(10).style(muted).width(55),
            button(text(parent_label).size(11))
                .padding([4, 8])
                .width(iced::Length::FillPortion(2))
                .on_press(Message::Aec(AecMessage::AecStylePickerOpen(
                    StylePickerTarget::OpeningStyleParent,
                ))),
        ]
        .spacing(8)
        .align_y(iced::Center),
        row![
            text(t!("Kind")).size(10).style(muted).width(48),
            pick_list(
                Some(form.kind.as_str().to_string()),
                kinds,
                |v: &String| v.clone(),
            )
            .on_select(|v| Message::Aec(AecMessage::AecOpeningStyleManagerKindChanged(v)))
            .text_size(11)
            .width(140),
            text(tr!("aec", "opening-shape")).size(10).style(muted).width(48),
            pick_list(
                Some(form.shape.as_str().to_string()),
                shapes,
                |v: &String| v.clone(),
            )
            .on_select(|v| Message::Aec(AecMessage::AecOpeningStyleManagerShapeChanged(v)))
            .text_size(11)
            .width(170),
        ]
        .spacing(8),
        row![
            text(t!("Width")).size(10).style(muted).width(50),
            text_input("", form.width)
                .on_input(|v| Message::Aec(AecMessage::AecOpeningStyleManagerWidthChanged(v)))
                .size(11)
                .padding([4, 6])
                .width(95),
            text(t!("Height")).size(10).style(muted).width(52),
            {
                let mut h = text_input("", form.height).size(11).padding([4, 6]).width(95);
                if !height_locked {
                    h = h.on_input(|v| Message::Aec(AecMessage::AecOpeningStyleManagerHeightChanged(v)));
                }
                h
            },
            text(tr!("aec", "opening-sill")).size(10).style(muted).width(55),
            text_input("", form.sill)
                .on_input(|v| Message::Aec(AecMessage::AecOpeningStyleManagerSillChanged(v)))
                .size(11)
                .padding([4, 6])
                .width(95),
            text(tr!("aec", "opening-angle")).size(10).style(muted).width(90),
            text_input("", form.angle)
                .on_input(|v| Message::Aec(AecMessage::AecOpeningStyleManagerAngleChanged(v)))
                .size(11)
                .padding([4, 6])
                .width(95),
        ]
        .spacing(8),
    ]
    .spacing(7);


    if form.shape == OpeningShape::Arch {
        detail_col = detail_col.push(
            row![
                text(tr!("aec", "opening-spring")).size(10).style(muted).width(100),
                text_input("", form.spring)
                    .on_input(|v| Message::Aec(AecMessage::AecOpeningStyleManagerSpringChanged(v)))
                    .size(11)
                    .padding([4, 6])
                    .width(120),
            ]
            .spacing(8),
        );
    }

    if !form.parent_id.is_none() {
        let mut names = Vec::new();
        let mut cursor = form.parent_id;
        while let Some(id) = cursor {
            let Some(os) = library
                .opening_styles
                .iter()
                .find(|s| s.style.id == id)
            else {
                break;
            };
            names.push(os.style.name.clone());
            cursor = os.style.parent.as_deref();
        }
        names.reverse();
        if !names.is_empty() {
            let mut chain_row = row![text(t!("Inheritance")).size(10).style(muted).width(100)]
                .spacing(4)
                .align_y(iced::Center);
            for (i, name) in names.into_iter().enumerate() {
                if i > 0 {
                    chain_row = chain_row.push(text("→").size(10).style(muted));
                }
                chain_row = chain_row.push(text(name).size(11));
            }
            chain_row = chain_row.push(text("→").size(10).style(muted));
            chain_row = chain_row.push(
                text(if form.name.is_empty() {
                    t!("(this style)").into_owned()
                } else {
                    form.name.to_string()
                })
                .size(11),
            );
            detail_col = detail_col.push(chain_row);
        }
    }

    let preview_style = form_preview_style(&form);
    let preview_width = crate::modules::aec::styles::opening_style_manager::parse_dim(form.width)
        .unwrap_or(preview_style.default_width);
    let preview_height = crate::modules::aec::styles::opening_style_manager::parse_dim(form.height)
        .unwrap_or(preview_style.default_height);
    let preview_sill = crate::modules::aec::styles::opening_style_manager::parse_dim(form.sill)
        .unwrap_or(preview_style.default_sill);
    let paths = cached_preview_baked_paths_for_mode(
        form.preview_model_cache,
        &preview_style,
        preview_width,
        form.preview_mode,
        Some(library),
    );
    let look_styles = preview_look_styles(&preview_style, library);

    let is_plan = form.preview_mode == AecOpeningPreviewMode::Plan2D;
    let is_elev = form.preview_mode == AecOpeningPreviewMode::Elevation2D;
    let is_section = form.preview_mode == AecOpeningPreviewMode::Section;
    let is_3d = form.preview_mode == AecOpeningPreviewMode::Model3D;

    let preview_header = row![
        text(tr!("aec", "opening-preview")).size(10).style(muted),
        Space::new(),
        button(text(tr!("aec", "opening-preview-plan2d-short")).size(10))
            .style(if is_plan { button::primary } else { button::secondary })
            .padding([2, 8])
            .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSetPreviewMode(
                AecOpeningPreviewMode::Plan2D,
            ))),
        button(text(tr!("aec", "opening-preview-elevation2d-short")).size(10))
            .style(if is_elev { button::primary } else { button::secondary })
            .padding([2, 8])
            .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSetPreviewMode(
                AecOpeningPreviewMode::Elevation2D,
            ))),
        button(text(tr!("aec", "opening-preview-section-short")).size(10))
            .style(if is_section { button::primary } else { button::secondary })
            .padding([2, 8])
            .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSetPreviewMode(
                AecOpeningPreviewMode::Section,
            ))),
        button(text(tr!("aec", "opening-preview-model3d-short")).size(10))
            .style(if is_3d { button::primary } else { button::secondary })
            .padding([2, 8])
            .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSetPreviewMode(
                AecOpeningPreviewMode::Model3D,
            ))),
    ]
    .spacing(4)
    .align_y(iced::Center);
    let isometric_buttons = row![
        button(text(tr!("aec", "opening-isometric-front-left")).size(9))
            .style(if form.preview_isometric == AecOpeningIsometric::FrontLeft { button::primary } else { button::secondary })
            .padding([2, 4])
            .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSetIsometric(AecOpeningIsometric::FrontLeft))),
        button(text(tr!("aec", "opening-isometric-back-left")).size(9))
            .style(if form.preview_isometric == AecOpeningIsometric::BackLeft { button::primary } else { button::secondary })
            .padding([2, 4])
            .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSetIsometric(AecOpeningIsometric::BackLeft))),
        button(text(tr!("aec", "opening-isometric-back-right")).size(9))
            .style(if form.preview_isometric == AecOpeningIsometric::BackRight { button::primary } else { button::secondary })
            .padding([2, 4])
            .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSetIsometric(AecOpeningIsometric::BackRight))),
        button(text(tr!("aec", "opening-isometric-front-right")).size(9))
            .style(if form.preview_isometric == AecOpeningIsometric::FrontRight { button::primary } else { button::secondary })
            .padding([2, 4])
            .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSetIsometric(AecOpeningIsometric::FrontRight))),
    ]
    .spacing(3);
    let preview_toolbar = if is_3d {
        column![preview_header, isometric_buttons].spacing(4)
    } else {
        column![preview_header]
    };

    let preview_canvas = container(
        canvas(OpeningPreviewCanvas {
            paths,
            mode: form.preview_mode,
            width: preview_width.max(1e-6),
            height: preview_height.max(1e-6),
            sill: preview_sill.max(0.0),
            thickness: PREVIEW_WALL_THICKNESS,
            look_styles,
            selected_role: form.selected_look_role,
            isometric: form.preview_isometric,
            zoom: form.preview_zoom,
            pan_x: form.preview_pan_x,
            pan_y: form.preview_pan_y,
        })
        .width(FillLen)
        .height(FillLen),
    )
    .width(FillLen)
    .height(FillLen)
    .style(|theme: &Theme| container::Style {
        background: Some(
            theme
                .palette()
                .background
                .neutral
                .color
                .scale_alpha(0.15)
                .into(),
        ),
        border: Border {
            color: theme.palette().background.neutral.color,
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    });
    let preview_col = column![preview_toolbar, preview_canvas]
        .spacing(8)
        .width(iced::Length::FillPortion(3))
        .height(FillLen);

    let properties = scrollable(opening_editor_tab_content(&form, library))
        .id(iced::advanced::widget::Id::new(OPENING_EDITOR_SCROLL_ID))
        .height(FillLen)
        .width(iced::Length::FillPortion(2));
    let editor = container(row![component_group_list(&form), properties, preview_col]
        .spacing(12)
        .height(FillLen))
        .padding(8)
        .width(FillLen)
        .height(FillLen)
        .style(|theme: &Theme| container::Style {
            background: Some(
                theme
                    .palette()
                    .background
                    .neutral
                    .color
                    .scale_alpha(0.08)
                    .into(),
            ),
            border: Border {
                color: theme.palette().background.neutral.color,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });
    let raster = divisions_tab(&form);
    let detail_panel = container(detail_col)
        .padding(8)
        .width(FillLen)
        .style(|theme: &Theme| container::Style {
            background: Some(
                theme
                    .palette()
                    .background
                    .neutral
                    .color
                    .scale_alpha(0.08)
                    .into(),
            ),
            border: Border {
                color: theme.palette().background.neutral.color,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });
    column![detail_panel, editor, raster]
        .spacing(10)
        .height(FillLen)
        .into()
}

mod editor;
pub(crate) use editor::*;
mod grid;
use grid::*;
mod labels;
use labels::*;
mod preview;
pub(crate) use preview::*;
#[cfg(test)]
mod tests;
