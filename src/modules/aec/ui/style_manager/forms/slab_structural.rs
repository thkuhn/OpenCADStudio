//! AEC Structural Slab Style Manager — browse and edit structural slab styles held in the AEC
//! style library (`AEC_SLABSTRUCTURALSTYLEMANAGER`).

use iced::mouse;
use iced::widget::canvas::{Frame, Geometry, Path, Program};
use iced::widget::{
    button, canvas, column, container, pick_list, row, scrollable, text, text_input, Space,
};
use iced::{Alignment, Border, Color, Element, Fill, Length, Point, Rectangle, Theme};

use crate::app::{AecMessage, Message, StylePickerTarget};
use crate::modules::aec::engine::library::{LibrarySource, StyleLibrary};
use crate::modules::aec::engine::project::ProjectFile;
use crate::modules::aec::engine::slab_style::SlabStyleLayer;
use crate::modules::aec::state::{AecSlabLayerBuffer, AecSlabPreviewMode};
use crate::modules::aec::styles::slab_style_manager::{
    preview_paths_for_mode, SlabPreviewPath,
};
use crate::modules::aec::engine::style_common::StyleOrigin;
use crate::modules::aec::ui::aec_ui_util::muted;
use crate::modules::aec::ui::style_manager::actions::{self, ActionMessages};
use crate::modules::aec::ui::style_manager::list::{self, filter_entries, ListEntry, ListSpec};
use crate::modules::aec::ui::style_manager::StyleParts;
use crate::t;

const LAYER_COL_REORDER_W: f32 = 74.0;
const LAYER_COL_MATERIAL_W: f32 = 180.0;
const LAYER_COL_THICKNESS_W: f32 = 60.0;
const LAYER_COL_FUNCTION_W: f32 = 120.0;
const LAYER_COL_OFFSET_W: f32 = 50.0;
const LAYER_COL_OVERRIDE_W: f32 = 130.0;
const LAYER_COL_HATCH_W: f32 = 180.0;

pub struct SlabStructuralStyleFormState<'a> {
    pub open: bool,
    pub is_new: bool,
    pub name: &'a str,
    pub parent_id: Option<&'a str>,
    pub parent_name: Option<String>,
    pub layers: &'a [AecSlabLayerBuffer],
    pub drag_index: Option<usize>,
    pub all_structural_styles: Vec<(&'a str, &'a str)>,
    pub all_materials: Vec<(&'a str, &'a str)>,
    pub all_layer_names: Vec<String>,
    pub effective_layers: Vec<SlabStyleLayer>,
    pub preview_mode: AecSlabPreviewMode,
}

pub fn parts<'a>(
    library: &'a StyleLibrary,
    project: Option<&'a ProjectFile>,
    session_library: Option<&'a StyleLibrary>,
    filter: &'a str,
    selected_id: Option<&'a str>,
    form: Option<SlabStructuralStyleFormState<'a>>,
) -> StyleParts<'a> {
    let entries = crate::modules::aec::engine::library::combined_slab_structural_style_entries_with_session(
        project,
        session_library,
    );
    let source_by_id: std::collections::HashMap<String, LibrarySource> = entries
        .iter()
        .map(|e| (e.structural_style.style.id.clone(), e.source))
        .collect();
    let _ = library;

    let all: Vec<ListEntry> = entries
        .iter()
        .map(|e| ListEntry {
            id: e.structural_style.style.id.clone(),
            label: e.structural_style.style.name.clone(),
            detail: None,
            origin: StyleOrigin::from(e.source),
            group: None,
            swatch: None,
        })
        .collect();
    let rows = filter_entries(all, filter);

    let list = list::view(
        ListSpec {
            title: t!("Rohbaustile").into_owned(),
            search_hint: t!("Search styles...").into_owned(),
            new_label: t!("+ Neuer Rohbaustil").into_owned(),
            filter,
            selected: selected_id,
            on_filter: |s| Message::Aec(AecMessage::AecSlabStructuralStyleManagerFilter(s)),
            on_select: |id| Message::Aec(AecMessage::AecSlabStructuralStyleManagerSelect(id)),
            on_new: Message::Aec(AecMessage::AecSlabStructuralStyleManagerNew),
        },
        &rows,
    );

    let open = form.as_ref().is_some_and(|f| f.open);
    let is_new = form.as_ref().map(|f| f.is_new).unwrap_or(true);
    let selected_source = selected_id.and_then(|id| source_by_id.get(id).copied());
    let has_standard = selected_id.is_some_and(|id| {
        crate::modules::aec::engine::library::load_or_seed()
            .slab_structural_styles
            .iter()
            .any(|s| s.style.id == id)
    });
    let form_el: Element<'a, Message> = match form {
        Some(f) if f.open => view_form(f),
        _ => empty_form_view(),
    };
    let actions = open.then(|| {
        actions::view(
            ActionMessages {
                save_apply: Message::Aec(AecMessage::AecSlabStructuralStyleManagerSaveAndApply),
                save: Message::Aec(AecMessage::AecSlabStructuralStyleManagerSave),
                duplicate: Message::Aec(AecMessage::AecSlabStructuralStyleManagerDuplicate),
                delete: Message::Aec(AecMessage::AecSlabStructuralStyleManagerDelete),
                to_standard: Message::Aec(AecMessage::AecStyleManagerCopySlabStructuralStyleToGlobal),
                to_project: Message::Aec(AecMessage::AecStyleManagerCopySlabStructuralStyleToProject),
            },
            is_new,
            selected_source.map(StyleOrigin::from),
            has_standard,
        )
    });
    StyleParts { list, form: form_el, actions }
}

fn empty_form_view<'a>() -> Element<'a, Message> {
    container(text(t!("No style selected")).size(11).style(muted))
        .width(Fill)
        .height(Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
}

fn view_form<'a>(form: SlabStructuralStyleFormState<'a>) -> Element<'a, Message> {

    let name_input = text_input(t!("Style name").as_ref(), form.name)
        .size(11)
        .padding([4, 8])
        .on_input(|s| Message::Aec(AecMessage::AecSlabStructuralStyleManagerNameChanged(s)));

    let parent_label = form
        .parent_name
        .clone()
        .or_else(|| form.parent_id.map(str::to_string))
        .unwrap_or_else(|| t!("(None)").into_owned());

    let parent_btn = button(text(format!("{}: {}", t!("Basisstil"), parent_label)).size(11))
        .padding([4, 8])
        .style(button::secondary)
        .on_press(Message::Aec(AecMessage::AecStylePickerOpen(
            StylePickerTarget::SlabStructuralStyleParent,
        )));


    let header_row = row![
        text(t!("Name:")).size(11).style(muted),
        name_input,
        Space::new().width(8),
        parent_btn,
    ]
    .align_y(Alignment::Center)
    .spacing(4);

    let is_cs = form.preview_mode == AecSlabPreviewMode::CrossSection;
    let is_rcp = form.preview_mode == AecSlabPreviewMode::ReflectedCeilingPlan;
    let is_3d = form.preview_mode == AecSlabPreviewMode::Model3D;

    let preview_header = row![
        text(t!("Vorschau:")).size(10).style(muted),
        Space::new(),
        button(text(t!("Querschnitt (2D)")).size(10))
            .style(if is_cs {
                button::primary
            } else {
                button::secondary
            })
            .padding([2, 8])
            .on_press(Message::Aec(AecMessage::AecSlabStructuralStyleManagerSetPreviewMode(
                AecSlabPreviewMode::CrossSection,
            ))),
        button(text(t!("Deckenspiegel (2D)")).size(10))
            .style(if is_rcp {
                button::primary
            } else {
                button::secondary
            })
            .padding([2, 8])
            .on_press(Message::Aec(AecMessage::AecSlabStructuralStyleManagerSetPreviewMode(
                AecSlabPreviewMode::ReflectedCeilingPlan,
            ))),
        button(text(t!("Isometrie (3D)")).size(10))
            .style(if is_3d {
                button::primary
            } else {
                button::secondary
            })
            .padding([2, 8])
            .on_press(Message::Aec(AecMessage::AecSlabStructuralStyleManagerSetPreviewMode(
                AecSlabPreviewMode::Model3D,
            ))),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let preview_paths = preview_paths_for_mode(&form.effective_layers, form.preview_mode);
    let preview_box = container(
        canvas(SlabStructuralPreviewCanvas {
            paths: preview_paths,
            mode: form.preview_mode,
        })
        .width(Fill)
        .height(180),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(
            theme
                .palette()
                .background
                .neutral
                .color
                .scale_alpha(0.12)
                .into(),
        ),
        border: Border {
            color: theme.palette().background.neutral.color.scale_alpha(0.3),
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    });

    let table_header = row![
        Space::new().width(LAYER_COL_REORDER_W),
        text(t!("Material")).size(10).style(muted).width(LAYER_COL_MATERIAL_W),
        text(t!("Thick (cm)")).size(10).style(muted).width(LAYER_COL_THICKNESS_W),
        text(t!("Function")).size(10).style(muted).width(LAYER_COL_FUNCTION_W),
        text(t!("Offset [cm]")).size(10).style(muted).width(LAYER_COL_OFFSET_W),
        text(t!("Lyr. Over.")).size(10).style(muted).width(LAYER_COL_OVERRIDE_W),
        text(t!("Hatch")).size(10).style(muted).width(LAYER_COL_HATCH_W),
        Space::new().width(24),
    ]
    .spacing(8);

    let mut layers_col = column![].spacing(4);
    let functions = vec![
        "Structural".to_string(),
        "Insulation".to_string(),
        "Finish".to_string(),
        "Other".to_string(),
    ];

    let mut total_th_cm = 0.0;
    for (i, layer_buf) in form.layers.iter().enumerate() {
        if let Ok(val) = layer_buf.thickness.parse::<f64>() {
            total_th_cm += val;
        }

        let drag_up_btn = button(text("▲").size(10))
            .padding([2, 5])
            .on_press(Message::Aec(AecMessage::AecSlabStructuralStyleManagerLayerMoveUp(i)));
        let drag_down_btn = button(text("▼").size(10))
            .padding([2, 5])
            .on_press(Message::Aec(AecMessage::AecSlabStructuralStyleManagerLayerMoveDown(i)));

        let reorder_cell = row![
            drag_up_btn,
            drag_down_btn,
        ]
        .spacing(2)
        .align_y(Alignment::Center);

        let mat_btn = button(text(layer_buf.material_id.as_str()).size(11))
            .width(Fill)
            .padding([3, 6])
            .style(button::secondary)
            .on_press(Message::Aec(AecMessage::AecStylePickerOpen(
                StylePickerTarget::SlabStructuralLayerMaterial(i),
            )));

        let th_input = text_input("cm", &layer_buf.thickness)
            .size(11)
            .padding([3, 6])
            .on_input(move |s| {
                Message::Aec(AecMessage::AecSlabStructuralStyleManagerLayerThicknessChanged(i, s))
            });

        let fn_pick = pick_list(
            Some(layer_buf.function.clone()),
            functions.clone(),
            |func: &String| func.clone(),
        )
        .on_select(move |sel| {
            Message::Aec(AecMessage::AecSlabStructuralStyleManagerLayerFunctionChanged(i, sel))
        })
        .text_size(11)
        .padding([3, 6]);

        let offset_input = text_input("cm", &layer_buf.vertical_offset)
            .size(11)
            .padding([3, 6])
            .on_input(move |s| {
                Message::Aec(AecMessage::AecSlabStructuralStyleManagerLayerVerticalOffsetChanged(i, s))
            });

        let layer_override_input = text_input(t!("Standard").as_ref(), &layer_buf.layer_override)
            .size(11)
            .padding([3, 6])
            .on_input(move |s| {
                Message::Aec(AecMessage::AecSlabStructuralStyleManagerLayerOverrideChanged(i, s))
            });

        let hatch_override_input = text_input(t!("Standard").as_ref(), &layer_buf.hatch_override)
            .size(11)
            .padding([3, 6])
            .on_input(move |s| {
                Message::Aec(AecMessage::AecSlabStructuralStyleManagerLayerHatchOverrideChanged(i, s))
            });

        let del_btn = button(text("✕").size(10))
            .padding([2, 6])
            .style(button::danger)
            .on_press(Message::Aec(AecMessage::AecSlabStructuralStyleManagerLayerRemove(i)));

        let row_item = row![
            container(reorder_cell).width(LAYER_COL_REORDER_W),
            container(mat_btn).width(LAYER_COL_MATERIAL_W),
            container(th_input).width(LAYER_COL_THICKNESS_W),
            container(fn_pick).width(LAYER_COL_FUNCTION_W),
            container(offset_input).width(LAYER_COL_OFFSET_W),
            container(layer_override_input).width(LAYER_COL_OVERRIDE_W),
            container(hatch_override_input).width(LAYER_COL_HATCH_W),
            del_btn,
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        layers_col = layers_col.push(row_item);
    }

    let add_layer_btn = button(text(t!("Add Layer")).size(10))
        .padding([2, 8])
        .on_press(Message::Aec(AecMessage::AecSlabStructuralStyleManagerLayerAdd));

    let summary_label = text(crate::tf!(
        "Gesamtdicke: {:.1} cm ({} Schichten)",
        total_th_cm,
        form.layers.len()
    ))
    .size(10)
    .style(muted);

        let layers_section = column![
        row![
            text(t!("Schichtaufbau (von oben nach unten):")).size(10).style(muted),
            Space::new().width(Fill),
            add_layer_btn,
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        table_header,
        container(scrollable(layers_col).height(Length::Fixed(180.0)))
            .style(|theme: &Theme| container::Style {
                border: Border {
                    color: theme.palette().background.neutral.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            })
            .padding(iced::Padding { top: 4.0, right: 14.0, bottom: 4.0, left: 4.0 }),
        summary_label,
    ]
    .spacing(6);

    column![
        header_row,
        Space::new().height(4),
        preview_header,
        preview_box,
        Space::new().height(4),
        layers_section,
    ]
    .spacing(6)
    .height(Fill)
    .into()
}

struct SlabStructuralPreviewCanvas {
    paths: Vec<SlabPreviewPath>,
    #[allow(dead_code)]
    mode: AecSlabPreviewMode,
}

impl<Message> Program<Message> for SlabStructuralPreviewCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for p in &self.paths {
            for &(x, y) in &p.points {
                if x.is_finite() && y.is_finite() {
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }
        if !min_x.is_finite() || !max_x.is_finite() || (max_x - min_x) < 1e-4 {
            min_x = -1.5;
            max_x = 1.5;
        }
        if !min_y.is_finite() || !max_y.is_finite() || (max_y - min_y) < 1e-4 {
            min_y = -0.5;
            max_y = 0.5;
        }

        let pad = 28.0f32;
        let avail_w = (bounds.width - pad * 2.0).max(10.0);
        let avail_h = (bounds.height - pad * 2.0).max(10.0);
        let span_x = ((max_x - min_x) as f32).max(0.1);
        let span_y = ((max_y - min_y) as f32).max(0.1);
        let scale = (avail_w / span_x).min(avail_h / span_y);
        let cx = ((min_x + max_x) * 0.5) as f32;
        let cy = ((min_y + max_y) * 0.5) as f32;
        let map_pt = |x: f64, y: f64| -> Point {
            Point::new(
                bounds.width * 0.5 + (x as f32 - cx) * scale,
                bounds.height * 0.5 - (y as f32 - cy) * scale,
            )
        };

        for path in &self.paths {
            if path.points.is_empty() {
                continue;
            }
            let first = map_pt(path.points[0].0, path.points[0].1);
            let built = Path::new(|p| {
                p.move_to(first);
                for pt in &path.points[1..] {
                    p.line_to(map_pt(pt.0, pt.1));
                }
                if path.closed {
                    p.close();
                }
            });

            if let Some(fill) = path.fill_color {
                frame.fill(
                    &built,
                    Color::from_rgba(fill[0], fill[1], fill[2], fill[3]),
                );
            }

            let stroke_col = Color::from_rgba(
                path.color[0],
                path.color[1],
                path.color[2],
                path.color[3],
            );
            frame.stroke(
                &built,
                iced::widget::canvas::Stroke::default()
                    .with_color(stroke_col)
                    .with_width(path.line_width),
            );
        }

        vec![frame.into_geometry()]
    }
}
