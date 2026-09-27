//! AEC Slab Style Manager — browse and edit slab styles held in the AEC
//! style library (`AEC_SLABSTYLEMANAGER`).

use iced::mouse;
use iced::widget::canvas::{Frame, Geometry, LineDash, Path, Program, Stroke, Style as CanvasStyle, Text};
use iced::widget::{
    button, canvas, column, container, pick_list, row, scrollable, text, text_input, Space,
};
use iced::{Alignment, Background, Border, Color, Element, Fill, Length, Point, Rectangle, Theme};

use crate::app::{AecMessage, Message, StylePickerTarget};
use crate::modules::aec::engine::display_component::SlabComponentSlot;
use crate::modules::aec::engine::library::{LibrarySource, StyleLibrary};
use crate::modules::aec::engine::project::ProjectFile;
use crate::modules::aec::engine::slab_style::SlabStyleLayer;
use crate::modules::aec::state::{AecSlabLayerBuffer, AecSlabPreviewMode};
use crate::modules::aec::styles::slab_style_manager::{
    preview_paths_for_mode, SlabPreviewPath, EDITABLE_SLOTS,
};
use crate::modules::aec::ui::aec_ui_util::{muted, section_title};
use crate::t;

const LAYER_COL_REORDER_W: f32 = 60.0;
const LAYER_COL_MATERIAL_W: f32 = 170.0;
const LAYER_COL_THICKNESS_W: f32 = 80.0;
const LAYER_COL_FUNCTION_W: f32 = 120.0;
const LAYER_COL_OFFSET_W: f32 = 70.0;
const LAYER_COL_OVERRIDE_W: f32 = 110.0;
const LAYER_COL_HATCH_W: f32 = 90.0;

pub struct SlabStyleFormState<'a> {
    pub open: bool,
    pub is_new: bool,
    pub name: &'a str,
    pub parent_id: Option<&'a str>,
    pub parent_name: Option<String>,
    pub layers: &'a [AecSlabLayerBuffer],
    pub drag_index: Option<usize>,
    pub all_slab_styles: Vec<(&'a str, &'a str)>,
    pub all_materials: Vec<(&'a str, &'a str)>,
    pub all_layer_names: Vec<String>,
    pub effective_layers: Vec<SlabStyleLayer>,
    pub inheritance_chain: Vec<String>,
    pub preview_mode: AecSlabPreviewMode,
    pub display_config_names: Vec<String>,
    pub profile_selected: Option<&'a str>,
    pub slot_visibility: std::collections::HashMap<SlabComponentSlot, bool>,
}

pub fn view_window<'a>(
    library: &'a StyleLibrary,
    project: Option<&'a ProjectFile>,
    session_library: Option<&'a StyleLibrary>,
    filter: &'a str,
    selected_id: Option<&'a str>,
    form: Option<SlabStyleFormState<'a>>,
) -> Element<'a, Message> {
    let entries = crate::modules::aec::engine::library::combined_slab_style_entries_with_session(
        project,
        session_library,
    );
    let source_by_id: std::collections::HashMap<String, LibrarySource> = entries
        .iter()
        .map(|e| (e.slab_style.style.id.clone(), e.source))
        .collect();

    let mut merged = StyleLibrary::empty();
    for e in &entries {
        merged.upsert_slab_style(e.slab_style.clone());
    }
    for m in &library.materials {
        merged.upsert_material(m.clone());
    }

    let filter_lower = filter.to_lowercase();
    let owned_rows: Vec<(crate::modules::aec::engine::slab_style::SlabStyle, usize, LibrarySource)> =
        merged
            .slab_style_tree()
            .into_iter()
            .filter(|node| {
                filter.is_empty()
                    || node.style.style.name.to_lowercase().contains(&filter_lower)
                    || node.style.style.id.to_lowercase().contains(&filter_lower)
            })
            .map(|node| {
                let source = source_by_id
                    .get(&node.style.style.id)
                    .copied()
                    .unwrap_or(LibrarySource::Standard);
                (node.style.clone(), node.depth, source)
            })
            .collect();

    let search_box = text_input(t!("Search styles...").as_ref(), filter)
        .size(11)
        .padding([4, 8])
        .on_input(|s| Message::Aec(AecMessage::AecSlabStyleManagerFilter(s)));

    let mut list_col = column![search_box].spacing(4);
    let mut tree_scroll = column![].spacing(2);
    for (slab_style, depth, source) in owned_rows {
        let is_selected = selected_id == Some(slab_style.style.id.as_str());
        let badge = source_badge_element(Some(source));
        let id_for_click = slab_style.style.id.clone();
        let indent = depth as f32 * 12.0;
        let name_label = text(slab_style.style.name.clone()).size(11);
        let row_content = row![
            Space::new().width(indent),
            badge,
            Space::new().width(4),
            name_label,
            Space::new(),
        ]
        .align_y(Alignment::Center)
        .spacing(2);

        let item_btn = button(row_content)
            .width(Fill)
            .padding([3, 6])
            .style(if is_selected {
                button::primary
            } else {
                button::secondary
            })
            .on_press(Message::Aec(AecMessage::AecSlabStyleManagerSelect(
                id_for_click,
            )));
        tree_scroll = tree_scroll.push(item_btn);
    }

    let tree_scroll_view = scrollable(tree_scroll).height(Fill);
    let new_btn = button(text(t!("+ Neuer Deckenstil")).size(11))
        .width(Fill)
        .padding([4, 8])
        .style(button::primary)
        .on_press(Message::Aec(AecMessage::AecSlabStyleManagerNew));
    list_col = list_col.push(tree_scroll_view).push(new_btn);

    let left_panel = container(list_col)
        .width(260)
        .height(Fill)
        .padding(8)
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
                color: theme.palette().background.neutral.color.scale_alpha(0.3),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        });

    let right_panel: Element<'a, Message> = if let Some(form_state) = form {
        form_view(form_state, selected_id, project, session_library)
    } else {
        container(
            text(t!("Select or create a slab style to edit."))
                .size(12)
                .style(muted),
        )
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill)
        .into()
    };

    column![
        section_title(t!("AEC Slab Style Manager")),
        row![left_panel, Space::new().width(8), right_panel]
            .width(Fill)
            .height(Fill),
    ]
    .spacing(8)
    .padding(8)
    .width(Fill)
    .height(Fill)
    .into()
}

fn source_badge_element<'a>(source: Option<LibrarySource>) -> Element<'a, Message> {
    let (label, col) = match source {
        Some(LibrarySource::Standard) => ("Std", Color::from_rgb(0.35, 0.55, 0.85)),
        Some(LibrarySource::Project) => ("Proj", Color::from_rgb(0.30, 0.70, 0.40)),
        Some(LibrarySource::Session) => ("Ses", Color::from_rgb(0.80, 0.55, 0.20)),
        None => ("?", Color::from_rgb(0.5, 0.5, 0.5)),
    };
    container(text(label).size(9).style(move |_t: &Theme| text::Style {
        color: Some(Color::WHITE),
    }))
    .padding([1, 5])
    .style(move |_theme: &Theme| container::Style {
        background: Some(Background::Color(col)),
        border: Border {
            color: col,
            width: 1.0,
            radius: 2.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn form_view<'a>(
    form: SlabStyleFormState<'a>,
    selected_id: Option<&'a str>,
    project: Option<&'a ProjectFile>,
    session_library: Option<&'a StyleLibrary>,
) -> Element<'a, Message> {
    let selected_source = selected_id.and_then(|id| {
        crate::modules::aec::engine::library::slab_style_library_source_with_session(
            project,
            session_library,
            id,
        )
    });
    let has_standard_counterpart = selected_id.is_some_and(|id| {
        crate::modules::aec::engine::library::load_or_seed()
            .slab_styles
            .iter()
            .any(|s| s.style.id == id)
    });

    let name_input = text_input(t!("Style name").as_ref(), form.name)
        .size(11)
        .padding([4, 8])
        .on_input(|s| Message::Aec(AecMessage::AecSlabStyleManagerNameChanged(s)));

    let parent_label = form
        .parent_name
        .clone()
        .or_else(|| form.parent_id.map(str::to_string))
        .unwrap_or_else(|| t!("(None)").into_owned());

    let parent_btn = button(text(format!("{}: {}", t!("Basisstil"), parent_label)).size(11))
        .padding([4, 8])
        .style(button::secondary)
        .on_press(Message::Aec(AecMessage::AecStylePickerOpen(
            StylePickerTarget::SlabStyleParent,
        )));

    let duplicate_btn = button(text(t!("Duplizieren")).size(11))
        .padding([4, 8])
        .style(button::secondary)
        .on_press(Message::Aec(AecMessage::AecSlabStyleManagerDuplicate));

    let header_row = row![
        text(t!("Name:")).size(11).style(muted),
        name_input,
        Space::new().width(8),
        parent_btn,
        Space::new().width(4),
        duplicate_btn,
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
            .on_press(Message::Aec(AecMessage::AecSlabStyleManagerSetPreviewMode(
                AecSlabPreviewMode::CrossSection,
            ))),
        button(text(t!("Deckenspiegel (2D)")).size(10))
            .style(if is_rcp {
                button::primary
            } else {
                button::secondary
            })
            .padding([2, 8])
            .on_press(Message::Aec(AecMessage::AecSlabStyleManagerSetPreviewMode(
                AecSlabPreviewMode::ReflectedCeilingPlan,
            ))),
        button(text(t!("Isometrie (3D)")).size(10))
            .style(if is_3d {
                button::primary
            } else {
                button::secondary
            })
            .padding([2, 8])
            .on_press(Message::Aec(AecMessage::AecSlabStyleManagerSetPreviewMode(
                AecSlabPreviewMode::Model3D,
            ))),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let preview_paths = preview_paths_for_mode(&form.effective_layers, form.preview_mode);
    let preview_box = container(
        canvas(SlabPreviewCanvas {
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
        container(text(t!("Pos")).size(10).style(muted)).width(LAYER_COL_REORDER_W),
        container(text(t!("Material")).size(10).style(muted)).width(LAYER_COL_MATERIAL_W),
        container(text(t!("Dicke [cm]")).size(10).style(muted)).width(LAYER_COL_THICKNESS_W),
        container(text(t!("Funktion")).size(10).style(muted)).width(LAYER_COL_FUNCTION_W),
        container(text(t!("Offset [cm]")).size(10).style(muted)).width(LAYER_COL_OFFSET_W),
        container(text(t!("Layer-Override")).size(10).style(muted)).width(LAYER_COL_OVERRIDE_W),
        container(text(t!("Schraffur-Override")).size(10).style(muted)).width(LAYER_COL_HATCH_W),
        Space::new(),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let mut layers_col = column![table_header].spacing(4);
    let functions = vec![
        "Structural".to_string(),
        "Insulation".to_string(),
        "Finish".to_string(),
        "Other".to_string(),
    ];

    let mut total_th_cm = 0.0;
    for (i, layer_buf) in form.layers.iter().enumerate() {
        if let Ok(v) = layer_buf.thickness.trim().parse::<f64>() {
            total_th_cm += v;
        }
        let is_first = i == 0;
        let is_last = i + 1 == form.layers.len();

        let up_btn = button(text("▲").size(9))
            .padding([2, 4])
            .style(button::secondary)
            .on_press_maybe((!is_first).then_some(Message::Aec(
                AecMessage::AecSlabStyleManagerLayerMoveUp(i),
            )));
        let down_btn = button(text("▼").size(9))
            .padding([2, 4])
            .style(button::secondary)
            .on_press_maybe((!is_last).then_some(Message::Aec(
                AecMessage::AecSlabStyleManagerLayerMoveDown(i),
            )));
        let reorder_cell = row![up_btn, down_btn]
            .spacing(2)
            .align_y(Alignment::Center);

        let mat_btn = button(text(layer_buf.material_id.clone()).size(10))
            .width(Fill)
            .padding([3, 6])
            .style(button::secondary)
            .on_press(Message::Aec(AecMessage::AecStylePickerOpen(
                StylePickerTarget::SlabLayerMaterial(i),
            )));

        let th_input = text_input("cm", &layer_buf.thickness)
            .size(10)
            .padding([3, 6])
            .on_input(move |s| {
                Message::Aec(AecMessage::AecSlabStyleManagerLayerThicknessChanged(i, s))
            });

        let fn_pick = pick_list(Some(layer_buf.function.clone()), functions.clone(), |func: &String| {
            func.clone()
        })
        .on_select(move |sel| {
            Message::Aec(AecMessage::AecSlabStyleManagerLayerFunctionChanged(i, sel))
        })
        .text_size(10)
        .padding([3, 6]);

        let offset_input = text_input("cm", &layer_buf.vertical_offset)
            .size(10)
            .padding([3, 6])
            .on_input(move |s| {
                Message::Aec(AecMessage::AecSlabStyleManagerLayerVerticalOffsetChanged(i, s))
            });

        let layer_override_input = text_input(t!("Standard").as_ref(), &layer_buf.layer_override)
            .size(10)
            .padding([3, 6])
            .on_input(move |s| {
                Message::Aec(AecMessage::AecSlabStyleManagerLayerOverrideChanged(i, s))
            });

        let hatch_override_input = text_input(t!("Standard").as_ref(), &layer_buf.hatch_override)
            .size(10)
            .padding([3, 6])
            .on_input(move |s| {
                Message::Aec(AecMessage::AecSlabStyleManagerLayerHatchOverrideChanged(i, s))
            });

        let del_btn = button(text("✕").size(10))
            .padding([2, 6])
            .style(button::danger)
            .on_press(Message::Aec(AecMessage::AecSlabStyleManagerLayerRemove(i)));

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
        .spacing(4)
        .align_y(Alignment::Center);
        layers_col = layers_col.push(row_item);
    }

    let add_layer_btn = button(text(t!("+ Schicht hinzufügen")).size(10))
        .padding([4, 8])
        .style(button::primary)
        .on_press(Message::Aec(AecMessage::AecSlabStyleManagerLayerAdd));

    let summary_label = text(format!(
        "Gesamtdicke: {:.1} cm ({} Schichten)",
        total_th_cm,
        form.layers.len()
    ))
    .size(10)
    .style(muted);

    let layers_footer = row![add_layer_btn, Space::new(), summary_label].align_y(Alignment::Center);
    let layers_section = column![
        text(t!("Schichtaufbau (von oben nach unten):"))
            .size(11)
            .style(muted),
        scrollable(layers_col).height(Length::Fixed(140.0)),
        layers_footer,
    ]
    .spacing(6);

    let slots_title = text(t!("Komponenten-Sichtbarkeit:")).size(10).style(muted);
    let mut slots_row = row![slots_title].spacing(8).align_y(Alignment::Center);
    for slot in EDITABLE_SLOTS {
        let is_vis = form.slot_visibility.get(slot).copied().unwrap_or(true);
        let slot_name = match slot {
            SlabComponentSlot::Contour2D => "Kontur 2D",
            SlabComponentSlot::CeilingOutline2D => "Deckenspiegel 2D",
            SlabComponentSlot::LayerHatch2D => "Schraffur 2D",
            SlabComponentSlot::OpeningContour2D => "Öffnungskontur",
            SlabComponentSlot::OpeningSymbol2D => "DIN 1356 Symbol",
            SlabComponentSlot::Solid3D => "Körper 3D",
            SlabComponentSlot::SurfaceStyle3D => "Oberfläche 3D",
        };
        let slot_copy = *slot;
        let btn = button(text(slot_name).size(9))
            .style(if is_vis {
                button::primary
            } else {
                button::secondary
            })
            .padding([2, 6])
            .on_press(Message::Aec(
                AecMessage::AecSlabStyleManagerProfileSlotVisibilityToggle(slot_copy, !is_vis),
            ));
        slots_row = slots_row.push(btn);
    }

    let mut actions = row![
        Space::new(),
        button(text(t!("Übernehmen")).size(11))
            .style(button::primary)
            .padding([5, 12])
            .on_press(Message::Aec(AecMessage::AecSlabStyleManagerSaveAndApply)),
        button(text(t!("Speichern")).size(11))
            .style(button::secondary)
            .padding([5, 12])
            .on_press(Message::Aec(AecMessage::AecSlabStyleManagerSave)),
    ]
    .spacing(8);

    if !form.is_new {
        actions = actions.push(
            button(text(t!("Delete")).size(11))
                .style(button::danger)
                .padding([5, 12])
                .on_press(Message::Aec(AecMessage::AecSlabStyleManagerDelete)),
        );
        if selected_source == Some(LibrarySource::Project) && !has_standard_counterpart {
            actions = actions.push(
                button(text(t!("→ Standard")).size(11)).padding([5, 12]).on_press(Message::Aec(
                    AecMessage::AecStyleManagerCopySlabStyleToGlobal,
                )),
            );
        }
        if selected_source == Some(LibrarySource::Standard) {
            actions = actions.push(
                button(text(t!("→ Projekt")).size(11)).padding([5, 12]).on_press(Message::Aec(
                    AecMessage::AecStyleManagerCopySlabStyleToProject,
                )),
            );
        }
    }

    column![
        header_row,
        Space::new().height(4),
        preview_header,
        preview_box,
        Space::new().height(4),
        layers_section,
        Space::new().height(4),
        slots_row,
        Space::new(),
        actions,
    ]
    .spacing(6)
    .height(Fill)
    .into()
}

struct SlabPreviewCanvas {
    paths: Vec<SlabPreviewPath>,
    #[allow(dead_code)]
    mode: AecSlabPreviewMode,
}

impl<Message> Program<Message> for SlabPreviewCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        theme: &Theme,
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
            let stroke = Stroke {
                width: path.line_width,
                style: CanvasStyle::Solid(stroke_col),
                line_dash: if path.dashed {
                    LineDash {
                        segments: &[4.0, 3.0],
                        offset: 0,
                    }
                } else {
                    LineDash::default()
                },
                ..Default::default()
            };
            frame.stroke(&built, stroke);

            if let Some((lbl, (lx, ly))) = &path.label {
                let pt = map_pt(*lx, *ly);
                frame.fill_text(Text {
                    content: lbl.clone(),
                    position: pt,
                    color: theme.palette().background.base.text,
                    size: iced::Pixels(10.0),
                    align_x: iced::advanced::text::Alignment::Center,
                    align_y: iced::alignment::Vertical::Center,
                    ..Default::default()
                });
            }
        }

        vec![frame.into_geometry()]
    }
}
