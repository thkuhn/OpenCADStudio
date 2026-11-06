//! Opening Style Manager modal: list, form, slot table and symbol preview.

use iced::mouse;
use iced::widget::canvas::{Frame, Path, Program, Stroke};
use iced::widget::{button, canvas, column, container, pick_list, row, scrollable, text, text_input, Space};
use iced::{Border, Color as IcedColor, Element, Fill as FillLen, Point, Rectangle, Theme};
use std::collections::HashMap;

use crate::app::Message;
use crate::modules::aec::engine::library::{LibrarySource, StyleLibrary};
use crate::modules::aec::engine::opening_display::BakedPath;
use crate::modules::aec::engine::opening_shape::OpeningShape;
use crate::modules::aec::engine::opening_style::{
    FamilyPlacement, OpeningCellKind, OpeningLookRole, OpeningStyle, HingeSide,
};
use crate::modules::aec::engine::openings::OpeningKind;
use crate::modules::aec::engine::project::ProjectFile;
use crate::modules::aec::message::AecMessage;
use crate::modules::aec::state::{
    AecOpeningCellBuffer, AecOpeningEditorTab, AecOpeningLookBuffer, AecOpeningPreviewMode,
    StylePickerTarget,
};
use crate::modules::aec::styles::opening_style_manager::{
    preview_baked_paths_for_mode, PREVIEW_WALL_THICKNESS,
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
    pub editor_tab: AecOpeningEditorTab,
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
    pub glazing_thickness: &'a str,
    pub glazing_inset: &'a str,
    pub ext_sill_thickness: &'a str,
    pub ext_sill_overhang: &'a str,
    pub ext_sill_upstand: &'a str,
    pub int_board_thickness: &'a str,
    pub int_board_overhang: &'a str,
    pub int_board_upstand: &'a str,
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
    pub plan_block: &'a str,
    pub plan_placement: FamilyPlacement,
    pub section_block: &'a str,
    pub section_placement: FamilyPlacement,
    pub elevation_block: &'a str,
    pub elevation_placement: FamilyPlacement,
    pub angle: &'a str,
    pub spring: &'a str,
    pub hinge: HingeSide,
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
    let hinges: Vec<String> = vec![
        HingeSide::Left.as_str().to_string(),
        HingeSide::Right.as_str().to_string(),
    ];
    let height_locked = matches!(
        form.shape,
        OpeningShape::Circle | OpeningShape::Triangle(crate::modules::aec::engine::opening_shape::TriangleVariant::Equilateral)
    );

    let mut detail_col = column![
        row![
            text(t!("Name")).size(10).style(muted).width(100),
            text_input("", form.name)
                .on_input(|v| Message::Aec(AecMessage::AecOpeningStyleManagerNameChanged(v)))
                .size(11)
                .padding([4, 6]),
        ]
        .spacing(8),
        row![
            text(t!("Parent")).size(10).style(muted).width(100),
            button(text(parent_label).size(11))
                .padding([4, 8])
                .on_press(Message::Aec(AecMessage::AecStylePickerOpen(
                    StylePickerTarget::OpeningStyleParent,
                ))),
        ]
        .spacing(8),
        row![
            text(t!("Kind")).size(10).style(muted).width(100),
            pick_list(
                Some(form.kind.as_str().to_string()),
                kinds,
                |v: &String| v.clone(),
            )
            .on_select(|v| Message::Aec(AecMessage::AecOpeningStyleManagerKindChanged(v)))
            .text_size(11)
            .width(180),
        ]
        .spacing(8),
        row![
            text(tr!("aec", "opening-shape")).size(10).style(muted).width(100),
            pick_list(
                Some(form.shape.as_str().to_string()),
                shapes,
                |v: &String| v.clone(),
            )
            .on_select(|v| Message::Aec(AecMessage::AecOpeningStyleManagerShapeChanged(v)))
            .text_size(11)
            .width(220),
        ]
        .spacing(8),
        row![
            text(t!("Width")).size(10).style(muted).width(100),
            text_input("", form.width)
                .on_input(|v| Message::Aec(AecMessage::AecOpeningStyleManagerWidthChanged(v)))
                .size(11)
                .padding([4, 6])
                .width(120),
            text(t!("Height")).size(10).style(muted).width(60),
            {
                let mut h = text_input("", form.height).size(11).padding([4, 6]).width(120);
                if !height_locked {
                    h = h.on_input(|v| Message::Aec(AecMessage::AecOpeningStyleManagerHeightChanged(v)));
                }
                h
            },
        ]
        .spacing(8),
        row![
            text(tr!("aec", "opening-sill")).size(10).style(muted).width(100),
            text_input("", form.sill)
                .on_input(|v| Message::Aec(AecMessage::AecOpeningStyleManagerSillChanged(v)))
                .size(11)
                .padding([4, 6])
                .width(120),
            text(tr!("aec", "opening-hinge")).size(10).style(muted).width(60),
            pick_list(
                Some(form.hinge.as_str().to_string()),
                hinges,
                |v: &String| v.clone(),
            )
            .on_select(|v| Message::Aec(AecMessage::AecOpeningStyleManagerHingeChanged(v)))
            .text_size(11)
            .width(120),
        ]
        .spacing(8),
        row![
            text(tr!("aec", "opening-angle")).size(10).style(muted).width(100),
            text_input("", form.angle)
                .on_input(|v| Message::Aec(AecMessage::AecOpeningStyleManagerAngleChanged(v)))
                .size(11)
                .padding([4, 6])
                .width(120),
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

    let editor_tabs = row![
        editor_tab_button(form.editor_tab, AecOpeningEditorTab::Dimensions, "opening-tab-dimensions"),
        editor_tab_button(form.editor_tab, AecOpeningEditorTab::Graphics, "opening-tab-graphics"),
        editor_tab_button(form.editor_tab, AecOpeningEditorTab::Materials, "opening-tab-materials"),
        editor_tab_button(form.editor_tab, AecOpeningEditorTab::Divisions, "opening-tab-divisions"),
        editor_tab_button(form.editor_tab, AecOpeningEditorTab::Symbol, "opening-tab-symbol"),
    ]
    .spacing(4);
    let tab_body = opening_editor_tab_content(&form, library);
    detail_col = detail_col
        .push(Space::new().height(6))
        .push(editor_tabs)
        .push(
            scrollable(tab_body)
                .id(iced::advanced::widget::Id::new(OPENING_EDITOR_SCROLL_ID))
                .height(340),
        );

    let preview_style = form_preview_style(&form);
    let preview_width = crate::modules::aec::styles::opening_style_manager::parse_dim(form.width)
        .unwrap_or(preview_style.default_width);
    let preview_height = crate::modules::aec::styles::opening_style_manager::parse_dim(form.height)
        .unwrap_or(preview_style.default_height);
    let frame = crate::modules::aec::styles::opening_style_manager::parse_dim(form.frame)
        .unwrap_or(0.06);
    let paths = preview_baked_paths_for_mode(&preview_style, preview_width, form.preview_mode);
    let look_styles = preview_look_styles(&preview_style, library);

    let is_plan = form.preview_mode == AecOpeningPreviewMode::Plan2D;
    let is_elev = form.preview_mode == AecOpeningPreviewMode::Elevation2D;
    let is_section = form.preview_mode == AecOpeningPreviewMode::Section;
    let is_3d = form.preview_mode == AecOpeningPreviewMode::Model3D;

    let preview_header = row![
        text(tr!("aec", "opening-preview")).size(10).style(muted),
        Space::new(),
        button(text(tr!("aec", "opening-preview-plan2d")).size(10))
            .style(if is_plan { button::primary } else { button::secondary })
            .padding([2, 8])
            .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSetPreviewMode(
                AecOpeningPreviewMode::Plan2D,
            ))),
        button(text(tr!("aec", "opening-preview-elevation2d")).size(10))
            .style(if is_elev { button::primary } else { button::secondary })
            .padding([2, 8])
            .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSetPreviewMode(
                AecOpeningPreviewMode::Elevation2D,
            ))),
        button(text(tr!("aec", "opening-preview-section")).size(10))
            .style(if is_section { button::primary } else { button::secondary })
            .padding([2, 8])
            .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSetPreviewMode(
                AecOpeningPreviewMode::Section,
            ))),
        button(text(tr!("aec", "opening-preview-model3d")).size(10))
            .style(if is_3d { button::primary } else { button::secondary })
            .padding([2, 8])
            .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSetPreviewMode(
                AecOpeningPreviewMode::Model3D,
            ))),
    ]
    .spacing(4)
    .align_y(iced::Center);

    let preview_canvas = container(
        canvas(OpeningPreviewCanvas {
            paths,
            mode: form.preview_mode,
            width: preview_width.max(1e-6),
            height: preview_height.max(1e-6),
            thickness: PREVIEW_WALL_THICKNESS,
            frame: frame.max(0.0),
            look_styles,
            selected_role: form.selected_look_role,
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
    let preview_col = column![preview_header, preview_canvas]
        .spacing(8)
        .width(360)
        .height(FillLen);

    row![
        container(column![detail_col].spacing(10).height(FillLen))
            .width(FillLen)
            .height(FillLen),
        preview_col,
    ]
    .spacing(12)
    .height(FillLen)
    .into()
}

fn form_preview_style(form: &OpeningStyleFormState<'_>) -> OpeningStyle {
    let width = crate::modules::aec::styles::opening_style_manager::parse_dim(form.width)
        .unwrap_or(1.2);
    let height = crate::modules::aec::styles::opening_style_manager::parse_dim(form.height)
        .unwrap_or(1.2);
    let sill = crate::modules::aec::styles::opening_style_manager::parse_dim(form.sill).unwrap_or(0.0);
    let frame =
        crate::modules::aec::styles::opening_style_manager::parse_dim(form.frame).unwrap_or(0.06);
    let angle =
        crate::modules::aec::styles::opening_style_manager::parse_dim(form.angle).unwrap_or(90.0);
    let spring =
        crate::modules::aec::styles::opening_style_manager::parse_dim(form.spring).unwrap_or(0.0);
    let mut style = crate::modules::aec::styles::opening_style_manager::draft_opening_style(
        Some("preview".to_string()),
        if form.name.is_empty() { "preview" } else { form.name },
        form.parent_id.map(str::to_string),
        form.kind,
        width.max(1e-6),
        height.max(1e-6),
        sill,
        form.hinge,
        frame,
        angle,
        form.shape,
        spring,
    )
    .unwrap_or_else(|_| OpeningStyle::standard_window());
    let dim = |value: &str, fallback: f64| {
        crate::modules::aec::styles::opening_style_manager::parse_dim(value).unwrap_or(fallback)
    };
    style.parts.fixed_frame.face_width = frame.max(0.0);
    style.parts.fixed_frame.wall_depth = dim(form.frame_depth, style.parts.fixed_frame.wall_depth);
    style.parts.fixed_frame.axis_offset = dim(form.frame_offset, 0.0);
    style.parts.leaf_frame.face_width =
        dim(form.leaf_face, style.parts.leaf_frame.face_width);
    style.parts.leaf_frame.depth = dim(form.leaf_depth, style.parts.leaf_frame.depth);
    style.parts.leaf_frame.rebate_overlap = dim(form.leaf_rebate, style.parts.leaf_frame.rebate_overlap);
    style.parts.glazing.thickness = dim(form.glazing_thickness, style.parts.glazing.thickness);
    style.parts.glazing.inset = dim(form.glazing_inset, style.parts.glazing.inset);
    style.parts.exterior_sill.thickness = dim(form.ext_sill_thickness, 0.0);
    style.parts.exterior_sill.overhang = dim(form.ext_sill_overhang, 0.0);
    style.parts.exterior_sill.upstand = dim(form.ext_sill_upstand, 0.0);
    style.parts.interior_board.thickness = dim(form.int_board_thickness, 0.0);
    style.parts.interior_board.overhang = dim(form.int_board_overhang, 0.0);
    style.parts.interior_board.upstand = dim(form.int_board_upstand, 0.0);
    style.parts.threshold.height = dim(form.threshold_height, 0.0);
    style.parts.threshold.width = dim(form.threshold_width, 0.0);
    style.parts.shutter_box.height = dim(form.shutter_height, 0.0);
    style.parts.shutter_box.depth = dim(form.shutter_depth, 0.0);
    style.parts.shutter_box.seat_above_frame = dim(form.shutter_seat, 0.0);
    style.parts.shutter_box.installation_depth = dim(form.shutter_install, 0.0);
    style.parts.lintel.height = dim(form.lintel_height, 0.0);
    style.parts.lintel.bearing_left = dim(form.lintel_left, 0.0);
    style.parts.lintel.bearing_right = dim(form.lintel_right, 0.0);
    style.looks = crate::modules::aec::styles::opening_style_manager::opening_looks_from_buffers(
        form.look_buffers,
    );
    style
}

#[derive(Clone, Copy, Default)]
struct PreviewLookColors {
    line: Option<IcedColor>,
    hatch: Option<IcedColor>,
}

fn preview_color_from_acad(color: &acadrust::types::Color) -> Option<IcedColor> {
    let rgba = crate::modules::aec::engine::views::style_resolver::color_to_rgba8_opt(color)?;
    Some(IcedColor::from_rgba8(
        rgba.r,
        rgba.g,
        rgba.b,
        rgba.a as f32 / 255.0,
    ))
}

fn preview_color_from_rgb(rgb: u32) -> IcedColor {
    IcedColor::from_rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

fn preview_look_colors(
    style: &OpeningStyle,
    library: &StyleLibrary,
) -> HashMap<OpeningLookRole, PreviewLookColors> {
    OpeningLookRole::all()
        .iter()
        .copied()
        .map(|role| {
            let look = style.looks.get(role);
            let material = style
                .material_id_for_look(role)
                .and_then(|material_id| {
                    library.materials.iter().find(|material| {
                        material.id == material_id || material.name == material_id
                    })
                });
            let line = match look.line_color.as_ref() {
                Some(color) => preview_color_from_acad(color),
                None => material.map(|material| preview_color_from_rgb(material.line_color)),
            };
            let hatch = material
                .and_then(|material| material.hatch_color.as_ref())
                .and_then(preview_color_from_acad);
            (role, PreviewLookColors { line, hatch })
        })
        .collect()
}

fn preview_look_styles(
    style: &OpeningStyle,
    library: &StyleLibrary,
) -> HashMap<OpeningLookRole, PreviewLookColors> {
    preview_look_colors(style, library)
}

fn editor_tab_button<'a>(
    selected: AecOpeningEditorTab,
    tab: AecOpeningEditorTab,
    label_key: &'static str,
) -> Element<'a, Message> {
    button(text(opening_label(label_key)).size(11))
        .style(if selected == tab {
            button::primary
        } else {
            button::secondary
        })
        .padding([4, 10])
        .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSetEditorTab(tab)))
        .into()
}

fn opening_editor_tab_content<'a>(
    form: &OpeningStyleFormState<'a>,
    library: &StyleLibrary,
) -> Element<'a, Message> {
    match form.editor_tab {
        AecOpeningEditorTab::Dimensions => dimensions_tab(form),
        AecOpeningEditorTab::Graphics => graphics_tab(form),
        AecOpeningEditorTab::Materials => materials_tab(form, library),
        AecOpeningEditorTab::Divisions => divisions_tab(form),
        AecOpeningEditorTab::Symbol => symbol_tab(form),
    }
}

fn dimensions_tab<'a>(form: &OpeningStyleFormState<'a>) -> Element<'a, Message> {
    column![
        text(opening_label("opening-parts")).size(11).style(muted),
        dim_row("opening-frame-face", "frame", form.frame),
        dim_row("opening-frame-depth", "frame_depth", form.frame_depth),
        dim_row("opening-frame-offset", "frame_offset", form.frame_offset),
        dim_row("opening-leaf-face", "leaf_face", form.leaf_face),
        dim_row("opening-leaf-depth", "leaf_depth", form.leaf_depth),
        dim_row("opening-leaf-rebate", "leaf_rebate", form.leaf_rebate),
        dim_row("opening-glazing-thickness", "glazing_thickness", form.glazing_thickness),
        dim_row("opening-glazing-inset", "glazing_inset", form.glazing_inset),
        dim_row("opening-ext-sill-thickness", "ext_sill_thickness", form.ext_sill_thickness),
        dim_row("opening-ext-sill-overhang", "ext_sill_overhang", form.ext_sill_overhang),
        dim_row("opening-ext-sill-upstand", "ext_sill_upstand", form.ext_sill_upstand),
        dim_row("opening-int-board-thickness", "int_board_thickness", form.int_board_thickness),
        dim_row("opening-int-board-overhang", "int_board_overhang", form.int_board_overhang),
        dim_row("opening-int-board-upstand", "int_board_upstand", form.int_board_upstand),
        dim_row("opening-threshold-height", "threshold_height", form.threshold_height),
        dim_row("opening-threshold-width", "threshold_width", form.threshold_width),
        dim_row("opening-shutter-height", "shutter_height", form.shutter_height),
        dim_row("opening-shutter-depth", "shutter_depth", form.shutter_depth),
        dim_row("opening-shutter-seat", "shutter_seat", form.shutter_seat),
        dim_row("opening-shutter-install", "shutter_install", form.shutter_install),
        dim_row("opening-lintel-height", "lintel_height", form.lintel_height),
        dim_row("opening-lintel-left", "lintel_left", form.lintel_left),
        dim_row("opening-lintel-right", "lintel_right", form.lintel_right),
    ]
    .spacing(4)
    .into()
}

fn graphics_tab<'a>(form: &OpeningStyleFormState<'a>) -> Element<'a, Message> {
    let mut col = column![text(opening_label("opening-graphics")).size(11).style(muted)]
        .spacing(4);
    for role in OpeningLookRole::all() {
        if let Some(buffer) = form.look_buffers.get(role) {
            col = col.push(graphics_row(form, *role, buffer));
        }
    }
    col.into()
}

fn graphics_row<'a>(
    form: &OpeningStyleFormState<'a>,
    role: OpeningLookRole,
    buffer: &'a AecOpeningLookBuffer,
) -> Element<'a, Message> {
    let line_color = crate::modules::aec::ui::aec_ui_util::editor_string_to_acad_color(
        &buffer.line_color,
    )
    .unwrap_or(acadrust::types::Color::ByLayer);
    let color_field = container(crate::ui::color_select::color_selector(
        line_color,
        form.look_color_picker_role == Some(role),
        crate::ui::color_select::ColorExtras::default(),
        move |color| {
            Message::Aec(AecMessage::AecOpeningStyleManagerLookLineColorChanged(
                role,
                crate::modules::aec::ui::aec_ui_util::acad_color_to_editor_string(color),
            ))
        },
        Message::Aec(AecMessage::AecOpeningStyleManagerLookColorPickerToggle(role)),
        Message::OpenColorWindow(
            crate::app::ColorPickTarget::Aec(
                crate::modules::aec::ui::color_pick::AecColorPickTarget::OpeningComponent(role),
            ),
            line_color,
        ),
    ))
    .width(160);

    let fields = row![
        text(opening_look_role_label(role)).size(10).style(muted).width(130),
        crate::modules::aec::ui::aec_ui_util::linetype_field(
            &buffer.line_type,
            form.linetype_items,
            form.linetype_combo,
            move |value| Message::Aec(AecMessage::AecOpeningStyleManagerLookLineTypeChanged(role, value)),
        ),
        color_field,
    ]
    .spacing(6)
    .align_y(iced::Center);
    if form.selected_look_role == Some(role) {
        container(fields)
            .padding(4)
            .style(|theme: &Theme| container::Style {
                background: Some(
                    theme
                        .palette()
                        .primary
                        .base
                        .color
                        .scale_alpha(0.16)
                        .into(),
                ),
                border: Border {
                    color: theme.palette().primary.base.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            })
            .into()
    } else {
        fields.into()
    }
}

fn materials_tab<'a>(
    form: &OpeningStyleFormState<'a>,
    library: &StyleLibrary,
) -> Element<'a, Message> {
    let mut col = column![text(opening_label("opening-materials")).size(11).style(muted)]
        .spacing(4);
    for role in OpeningLookRole::all() {
        if let Some(buffer) = form.look_buffers.get(role) {
            let material_name = library
                .materials
                .iter()
                .find(|material| material.id == buffer.material_id)
                .map(|material| material.name.as_str())
                .unwrap_or_else(|| {
                    if buffer.material_id.is_empty() {
                        ""
                    } else {
                        buffer.material_id.as_str()
                    }
                });
            let mut fields = row![
                text(opening_look_role_label(*role)).size(10).style(muted).width(130),
                button(text(if material_name.is_empty() {
                    opening_label("opening-material-inherit")
                } else {
                    material_name.to_string()
                }).size(11))
                    .padding([3, 8])
                    .on_press(Message::Aec(AecMessage::AecStylePickerOpen(
                        StylePickerTarget::OpeningComponentMaterial(*role),
                    ))),
            ]
            .spacing(6)
            .align_y(iced::Center);
            if !buffer.material_id.is_empty() {
                fields = fields.push(
                    button(text("×").size(11))
                        .style(button::subtle)
                        .padding([2, 6])
                        .on_press(Message::Aec(
                            AecMessage::AecOpeningStyleManagerLookMaterialClear(*role),
                        )),
                );
            }
            if form.selected_look_role == Some(*role) {
                col = col.push(
                    container(fields)
                        .padding(4)
                        .style(|theme: &Theme| container::Style {
                            background: Some(
                                theme
                                    .palette()
                                    .primary
                                    .base
                                    .color
                                    .scale_alpha(0.16)
                                    .into(),
                            ),
                            border: Border {
                                color: theme.palette().primary.base.color,
                                width: 1.0,
                                radius: 3.0.into(),
                            },
                            ..Default::default()
                        }),
                );
            } else {
                col = col.push(fields);
            }
        }
    }
    col.into()
}

fn divisions_tab<'a>(form: &OpeningStyleFormState<'a>) -> Element<'a, Message> {
    let mut col = column![
        text(opening_label("opening-grid")).size(11).style(muted),
        dim_row("opening-vertical", "vertical", form.vertical),
        dim_row("opening-horizontal", "horizontal", form.horizontal),
        dim_row("opening-vertical-muntins", "vertical_muntins", form.vertical_muntins),
        dim_row("opening-horizontal-muntins", "horizontal_muntins", form.horizontal_muntins),
    ]
    .spacing(4);
    for (index, cell) in form.cells.iter().enumerate() {
        col = col.push(cell_row(index, cell));
    }
    col = col.push(
        button(text(opening_label("opening-cell-add")).size(11))
            .style(button::secondary)
            .padding([2, 8])
            .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerCellAdd)),
    );
    col.into()
}

fn symbol_tab<'a>(form: &OpeningStyleFormState<'a>) -> Element<'a, Message> {
    column![
        text(opening_label("opening-family-blocks")).size(11).style(muted),
        family_row("plan", "opening-family-plan", form.plan_block, form.plan_placement),
        family_row(
            "section",
            "opening-family-section",
            form.section_block,
            form.section_placement,
        ),
        family_row(
            "elevation",
            "opening-family-elevation",
            form.elevation_block,
            form.elevation_placement,
        ),
    ]
    .spacing(4)
    .into()
}

fn opening_label(key: &str) -> String {
    match key {
        "opening-parts" => tr!("aec", "opening-parts"),
        "opening-tab-dimensions" => tr!("aec", "opening-tab-dimensions"),
        "opening-tab-graphics" => tr!("aec", "opening-tab-graphics"),
        "opening-tab-materials" => tr!("aec", "opening-tab-materials"),
        "opening-tab-divisions" => tr!("aec", "opening-tab-divisions"),
        "opening-tab-symbol" => tr!("aec", "opening-tab-symbol"),
        "opening-graphics" => tr!("aec", "opening-graphics"),
        "opening-materials" => tr!("aec", "opening-materials"),
        "opening-material-inherit" => tr!("aec", "opening-material-inherit"),
        "opening-frame-face" => tr!("aec", "opening-frame-face"),
        "opening-frame-depth" => tr!("aec", "opening-frame-depth"),
        "opening-frame-offset" => tr!("aec", "opening-frame-offset"),
        "opening-leaf-face" => tr!("aec", "opening-leaf-face"),
        "opening-leaf-depth" => tr!("aec", "opening-leaf-depth"),
        "opening-leaf-rebate" => tr!("aec", "opening-leaf-rebate"),
        "opening-glazing-thickness" => tr!("aec", "opening-glazing-thickness"),
        "opening-glazing-inset" => tr!("aec", "opening-glazing-inset"),
        "opening-ext-sill-thickness" => tr!("aec", "opening-ext-sill-thickness"),
        "opening-ext-sill-overhang" => tr!("aec", "opening-ext-sill-overhang"),
        "opening-ext-sill-upstand" => tr!("aec", "opening-ext-sill-upstand"),
        "opening-int-board-thickness" => tr!("aec", "opening-int-board-thickness"),
        "opening-int-board-overhang" => tr!("aec", "opening-int-board-overhang"),
        "opening-int-board-upstand" => tr!("aec", "opening-int-board-upstand"),
        "opening-threshold-height" => tr!("aec", "opening-threshold-height"),
        "opening-threshold-width" => tr!("aec", "opening-threshold-width"),
        "opening-shutter-height" => tr!("aec", "opening-shutter-height"),
        "opening-shutter-depth" => tr!("aec", "opening-shutter-depth"),
        "opening-shutter-seat" => tr!("aec", "opening-shutter-seat"),
        "opening-shutter-install" => tr!("aec", "opening-shutter-install"),
        "opening-lintel-height" => tr!("aec", "opening-lintel-height"),
        "opening-lintel-left" => tr!("aec", "opening-lintel-left"),
        "opening-lintel-right" => tr!("aec", "opening-lintel-right"),
        "opening-grid" => tr!("aec", "opening-grid"),
        "opening-vertical" => tr!("aec", "opening-vertical"),
        "opening-horizontal" => tr!("aec", "opening-horizontal"),
        "opening-vertical-muntins" => tr!("aec", "opening-vertical-muntins"),
        "opening-horizontal-muntins" => tr!("aec", "opening-horizontal-muntins"),
        "opening-cell" => tr!("aec", "opening-cell"),
        "opening-cell-add" => tr!("aec", "opening-cell-add"),
        "opening-cell-remove" => tr!("aec", "opening-cell-remove"),
        "opening-family-blocks" => tr!("aec", "opening-family-blocks"),
        "opening-family-plan" => tr!("aec", "opening-family-plan"),
        "opening-family-section" => tr!("aec", "opening-family-section"),
        "opening-family-elevation" => tr!("aec", "opening-family-elevation"),
        "opening-block-name" => tr!("aec", "opening-block-name"),
        _ => key.to_string(),
    }
}

fn opening_look_role_label(role: OpeningLookRole) -> String {
    match role {
        OpeningLookRole::FixedFrame => tr!("aec", "opening-role-fixed-frame"),
        OpeningLookRole::Leaf => tr!("aec", "opening-role-leaf"),
        OpeningLookRole::Glazing => tr!("aec", "opening-role-glazing"),
        OpeningLookRole::Swing => tr!("aec", "opening-role-swing"),
        OpeningLookRole::Division => tr!("aec", "opening-role-division"),
        OpeningLookRole::ExteriorSill => tr!("aec", "opening-role-exterior-sill"),
        OpeningLookRole::InteriorBoard => tr!("aec", "opening-role-interior-board"),
        OpeningLookRole::Threshold => tr!("aec", "opening-role-threshold"),
        OpeningLookRole::ShutterBox => tr!("aec", "opening-role-shutter-box"),
        OpeningLookRole::Lintel => tr!("aec", "opening-role-lintel"),
    }
}

fn dim_row<'a>(label_key: &'static str, field: &'static str, value: &'a str) -> Element<'a, Message> {
    row![
        text(opening_label(label_key)).size(10).style(muted).width(140),
        text_input("", value)
            .on_input(move |v| Message::Aec(AecMessage::AecOpeningStyleManagerDimChanged(field.into(), v)))
            .size(11)
            .padding([2, 6]),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

fn cell_row<'a>(index: usize, cell: &'a AecOpeningCellBuffer) -> Element<'a, Message> {
    let kinds: Vec<String> = OpeningCellKind::catalogue().iter().map(|k| k.as_str().to_string()).collect();
    let hinges = vec!["-".to_string(), "Left".to_string(), "Right".to_string()];
    let hinge = cell.hinge.map(|h| h.as_str().to_string()).unwrap_or_else(|| "-".into());
    row![
        text(opening_label("opening-cell")).size(10).style(muted).width(50),
        pick_list(Some(cell.kind.as_str().to_string()), kinds, |v: &String| v.clone())
            .on_select(move |v| Message::Aec(AecMessage::AecOpeningStyleManagerCellKindChanged(index, v)))
            .text_size(11)
            .width(100),
        pick_list(Some(hinge), hinges, |v: &String| v.clone())
            .on_select(move |v| Message::Aec(AecMessage::AecOpeningStyleManagerCellHingeChanged(index, v)))
            .text_size(11)
            .width(80),
        text_input("0", &cell.sill)
            .on_input(move |v| Message::Aec(AecMessage::AecOpeningStyleManagerCellSillChanged(index, v)))
            .size(11)
            .padding([2, 6])
            .width(70),
        button(text(opening_label("opening-cell-remove")).size(10))
            .style(button::secondary)
            .padding([2, 6])
            .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerCellRemove(index))),
    ]
    .spacing(4)
    .align_y(iced::Center)
    .into()
}

fn family_row<'a>(
    family: &'static str,
    label_key: &'static str,
    name: &'a str,
    placement: FamilyPlacement,
) -> Element<'a, Message> {
    let placements = vec!["Evaluated".to_string(), "JambPair".to_string()];
    row![
        text(opening_label(label_key)).size(10).style(muted).width(90),
        text_input(opening_label("opening-block-name").as_str(), name)
            .on_input(move |v| {
                Message::Aec(AecMessage::AecOpeningStyleManagerFamilyBlockChanged(family.into(), v))
            })
            .size(11)
            .padding([2, 6]),
        pick_list(Some(placement.as_str().to_string()), placements, |v: &String| v.clone())
            .on_select(move |v| {
                Message::Aec(AecMessage::AecOpeningStyleManagerFamilyPlacementChanged(family.into(), v))
            })
            .text_size(11)
            .width(110),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

struct PreviewCamera {
    scale: f32,
    ox: f32,
    oy: f32,
}

impl PreviewCamera {
    fn fit(
        bounds: Rectangle,
        paths: &[BakedPath],
        mode: AecOpeningPreviewMode,
        width: f64,
        height: f64,
        thickness: f64,
    ) -> Self {
        let pad = 14.0f32;
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for path in paths {
            for &(x, y) in &path.points {
                if x.is_finite() && y.is_finite() {
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }

        let hw = width.max(1e-6) * 0.5;
        match mode {
            AecOpeningPreviewMode::Plan2D => {
                let ht = thickness.max(1e-6) * 0.5;
                min_x = min_x.min(-hw);
                max_x = max_x.max(hw);
                min_y = min_y.min(-ht);
                max_y = max_y.max(ht);
            }
            AecOpeningPreviewMode::Elevation2D => {
                min_x = min_x.min(-hw);
                max_x = max_x.max(hw);
                min_y = min_y.min(0.0);
                max_y = max_y.max(height.max(1e-6));
            }
            AecOpeningPreviewMode::Section | AecOpeningPreviewMode::Model3D => {}
        }

        if !min_x.is_finite() || !max_x.is_finite() || (max_x - min_x) < 1e-4 {
            min_x = -hw;
            max_x = hw;
        }
        if !min_y.is_finite() || !max_y.is_finite() || (max_y - min_y) < 1e-4 {
            let (ref_y_min, ref_y_max) = match mode {
                AecOpeningPreviewMode::Plan2D => {
                    let ht = thickness.max(1e-6) * 0.5;
                    (-ht, ht)
                }
                AecOpeningPreviewMode::Elevation2D
                | AecOpeningPreviewMode::Section
                | AecOpeningPreviewMode::Model3D => {
                    (0.0, height.max(1e-6))
                }
            };
            min_y = ref_y_min;
            max_y = ref_y_max;
        }

        let span_x = (max_x - min_x).max(1e-4);
        let span_y = (max_y - min_y).max(1e-4);
        let center_x = (min_x + max_x) * 0.5;
        let center_y = (min_y + max_y) * 0.5;

        let world_w = (span_x * 1.15) as f32;
        let world_h = (span_y * 1.15) as f32;
        let avail_w = (bounds.width - 2.0 * pad).max(1.0);
        let avail_h = (bounds.height - 2.0 * pad).max(1.0);
        let scale = (avail_w / world_w).min(avail_h / world_h);

        Self {
            scale,
            ox: bounds.width * 0.5 - (center_x as f32 * scale),
            oy: bounds.height * 0.5 + (center_y as f32 * scale),
        }
    }

    fn to_screen(&self, x: f64, y: f64) -> Point {
        Point::new(
            self.ox + x as f32 * self.scale,
            self.oy - y as f32 * self.scale,
        )
    }

    fn to_local(&self, p: Point) -> (f64, f64) {
        (
            (p.x - self.ox) as f64 / self.scale as f64,
            (self.oy - p.y) as f64 / self.scale as f64,
        )
    }
}

/// Slot of the preview path under the model-space point `(x, y)`: an edge
/// within `tol` wins (nearest first), otherwise the smallest closed path
/// containing the point.
fn hit_path<'a>(
    paths: &'a [BakedPath],
    x: f64,
    y: f64,
    tol: f64,
) -> Option<&'a BakedPath> {
    fn seg_dist(px: f64, py: f64, a: (f64, f64), b: (f64, f64)) -> f64 {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len2 = dx * dx + dy * dy;
        let t = if len2 < 1e-18 { 0.0 } else { (((px - a.0) * dx + (py - a.1) * dy) / len2).clamp(0.0, 1.0) };
        ((px - (a.0 + t * dx)).powi(2) + (py - (a.1 + t * dy)).powi(2)).sqrt()
    }
    let mut best_edge: Option<(f64, &'a BakedPath)> = None;
    let mut best_area: Option<(f64, &'a BakedPath)> = None;
    for path in paths {
        let pts = &path.points;
        if pts.len() < 2 {
            continue;
        }
        let n = if path.closed { pts.len() } else { pts.len() - 1 };
        for i in 0..n {
            let d = seg_dist(x, y, pts[i], pts[(i + 1) % pts.len()]);
            if d <= tol && best_edge.map_or(true, |(bd, _)| d < bd) {
                best_edge = Some((d, path));
            }
        }
        if path.closed && pts.len() >= 3 {
            let mut inside = false;
            let mut area = 0.0;
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                area += a.0 * b.1 - b.0 * a.1;
                if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
                    inside = !inside;
                }
            }
            let area = (area * 0.5).abs();
            if inside && best_area.map_or(true, |(ba, _)| area < ba) {
                best_area = Some((area, path));
            }
        }
    }
    best_edge.map(|(_, path)| path).or(best_area.map(|(_, path)| path))
}

pub(crate) fn hit_slot(
    paths: &[BakedPath],
    x: f64,
    y: f64,
    tol: f64,
) -> Option<crate::modules::aec::engine::display_component::OpeningComponentSlot> {
    hit_path(paths, x, y, tol).map(|path| path.slot)
}

fn hit_role(
    paths: &[BakedPath],
    x: f64,
    y: f64,
    tol: f64,
) -> Option<OpeningLookRole> {
    hit_path(paths, x, y, tol).and_then(BakedPath::resolved_look_role)
}

struct OpeningPreviewCanvas {
    paths: Vec<BakedPath>,
    mode: AecOpeningPreviewMode,
    width: f64,
    height: f64,
    thickness: f64,
    frame: f64,
    look_styles: HashMap<OpeningLookRole, PreviewLookColors>,
    selected_role: Option<OpeningLookRole>,
}

impl Program<Message> for OpeningPreviewCanvas {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event else {
            return None;
        };
        let pos = cursor.position_in(bounds)?;
        let cam = PreviewCamera::fit(
            bounds,
            &self.paths,
            self.mode,
            self.width,
            self.height,
            self.thickness,
        );
        let (x, y) = cam.to_local(pos);
        Some(
            canvas::Action::publish(Message::Aec(
                AecMessage::AecOpeningStyleManagerPreviewSlotSelect(hit_role(
                    &self.paths,
                    x,
                    y,
                    8.0 / cam.scale.max(1e-6) as f64,
                )),
            ))
            .and_capture(),
        )
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let Some(pos) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        let cam = PreviewCamera::fit(
            bounds,
            &self.paths,
            self.mode,
            self.width,
            self.height,
            self.thickness,
        );
        let (x, y) = cam.to_local(pos);
        if hit_path(&self.paths, x, y, 8.0 / cam.scale.max(1e-6) as f64).is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let cam = PreviewCamera::fit(
            bounds,
            &self.paths,
            self.mode,
            self.width,
            self.height,
            self.thickness,
        );

        if self.mode == AecOpeningPreviewMode::Plan2D {
            let hw = self.width * 0.5;
            let ht = self.thickness * 0.5;
            let ft = self.frame.max(0.0);
            let guide = theme.palette().background.neutral.color;
            let outer = Path::new(|p| {
                p.move_to(cam.to_screen(-hw, -ht));
                p.line_to(cam.to_screen(hw, -ht));
                p.line_to(cam.to_screen(hw, ht));
                p.line_to(cam.to_screen(-hw, ht));
                p.close();
            });
            frame.stroke(
                &outer,
                Stroke::default().with_width(1.0).with_color(guide),
            );
            let iw = (hw - ft).max(0.0);
            let it = (ht - ft).max(0.0);
            if iw > 1e-9 && it > 1e-9 {
                let inner = Path::new(|p| {
                    p.move_to(cam.to_screen(-iw, -it));
                    p.line_to(cam.to_screen(iw, -it));
                    p.line_to(cam.to_screen(iw, it));
                    p.line_to(cam.to_screen(-iw, it));
                    p.close();
                });
                frame.stroke(
                    &inner,
                    Stroke::default()
                        .with_width(1.0)
                        .with_color(guide.scale_alpha(0.6)),
                );
            }
        }

        let fallback_stroke_color = theme.palette().background.base.text;
        let fallback_fill_color = fallback_stroke_color.scale_alpha(0.18);
        for path in &self.paths {
            if path.points.len() < 2 {
                continue;
            }
            let first = cam.to_screen(path.points[0].0, path.points[0].1);
            let closed = path.closed;
            let pts = path.points.clone();
            let built = Path::new(|p| {
                p.move_to(first);
                for &(x, y) in &pts[1..] {
                    p.line_to(cam.to_screen(x, y));
                }
                if closed {
                    p.close();
                }
            });
            let role_colors = path
                .resolved_look_role()
                .and_then(|role| self.look_styles.get(&role));
            let selected = self.selected_role.is_some()
                && path.resolved_look_role() == self.selected_role;
            let stroke_color = if selected {
                theme.palette().primary.base.color
            } else {
                role_colors
                    .and_then(|colors| colors.line)
                    .unwrap_or(fallback_stroke_color)
            };
            if path.filled && path.closed {
                let fill_color = if self.mode == AecOpeningPreviewMode::Section {
                    role_colors
                        .and_then(|colors| colors.hatch)
                        .or_else(|| role_colors.and_then(|colors| colors.line))
                        .unwrap_or(stroke_color)
                        .scale_alpha(0.18)
                } else {
                    fallback_fill_color
                };
                frame.fill(&built, fill_color);
            }
            frame.stroke(
                &built,
                Stroke::default()
                    .with_width(if selected { 3.0 } else { 1.2 })
                    .with_color(stroke_color),
            );
        }

        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod hit_tests {
    use super::*;
    use crate::modules::aec::engine::display_component::OpeningComponentSlot;

    fn rect(slot: OpeningComponentSlot, x0: f64, y0: f64, x1: f64, y1: f64) -> BakedPath {
        BakedPath {
            slot,
            look_role: None,
            points: vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)],
            closed: true,
            filled: false,
        }
    }

    #[test]
    fn inner_rect_wins_inside_and_edge_wins_near_border() {
        let outer = OpeningComponentSlot::HostCut2D;
        let inner = OpeningComponentSlot::Frame2D;
        let paths = vec![rect(outer, 0.0, 0.0, 10.0, 10.0), rect(inner, 4.0, 4.0, 6.0, 6.0)];
        assert_eq!(hit_slot(&paths, 5.0, 5.0, 0.1), Some(inner));
        assert_eq!(hit_slot(&paths, 1.0, 1.0, 0.1), Some(outer));
        assert_eq!(hit_slot(&paths, 0.05, 5.0, 0.1), Some(outer));
        assert_eq!(hit_slot(&paths, 20.0, 20.0, 0.1), None);
    }

    #[test]
    fn preview_hit_resolves_role_override_and_clears_unmapped_or_missed_paths() {
        let role_path = BakedPath {
            slot: OpeningComponentSlot::Frame2D,
            look_role: Some(OpeningLookRole::ShutterBox),
            points: vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
            closed: true,
            filled: true,
        };
        assert_eq!(
            hit_role(&[role_path], 5.0, 5.0, 0.1),
            Some(OpeningLookRole::ShutterBox)
        );

        let unassigned = rect(OpeningComponentSlot::HostCut2D, 0.0, 0.0, 10.0, 10.0);
        assert_eq!(hit_role(&[unassigned], 5.0, 5.0, 0.1), None);
        assert_eq!(hit_role(&[], 5.0, 5.0, 0.1), None);
    }

    #[test]
    fn preview_role_colors_resolve_line_and_material_hatch_independently() {
        use crate::modules::aec::engine::material::Material;

        let mut style = OpeningStyle::standard_window();
        style.looks.fixed_frame.line_color = Some(acadrust::types::Color::Rgb {
            r: 210,
            g: 20,
            b: 30,
        });
        style.looks.fixed_frame.material_id = Some("frame_material".into());
        let mut library = StyleLibrary::empty();
        let mut material = Material::new(
            "frame_material".into(),
            "Frame material".into(),
            "ANSI31".into(),
            0x0080c0,
            "Continuous".into(),
        );
        material.hatch_color = Some(acadrust::types::Color::Rgb {
            r: 10,
            g: 40,
            b: 90,
        });
        library.materials.push(material);

        let colors = preview_look_styles(&style, &library)[&OpeningLookRole::FixedFrame];
        assert_eq!(colors.line, Some(IcedColor::from_rgb8(210, 20, 30)));
        assert_eq!(colors.hatch, Some(IcedColor::from_rgb8(10, 40, 90)));
    }
}
