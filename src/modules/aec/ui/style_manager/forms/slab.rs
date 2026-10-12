//! AEC Slab Style Manager — browse and edit slab styles held in the AEC
//! style library (`AEC_SLABSTYLEMANAGER`).

use iced::mouse;
use iced::widget::canvas::{Frame, Geometry, LineDash, Path, Program, Stroke, Style as CanvasStyle, Text};
use iced::widget::{
    button, canvas, column, container, pick_list, row, text, text_input, Space,
};
use iced::{Alignment, Border, Color, Element, Fill, Length, Point, Rectangle, Theme};

use crate::app::{AecMessage, Message, StylePickerTarget};
use crate::modules::aec::engine::display_component::SlabComponentSlot;
use crate::modules::aec::engine::library::{LibrarySource, StyleLibrary};
use crate::modules::aec::engine::project::ProjectFile;
use crate::modules::aec::engine::slab_style::SlabStyleLayer;
use crate::modules::aec::state::AecSlabPreviewMode;
use crate::modules::aec::styles::slab_style_manager::{
    preview_paths_for_mode, SlabPreviewPath, EDITABLE_SLOTS,
};
use crate::modules::aec::engine::style_common::StyleOrigin;
use crate::modules::aec::ui::aec_ui_util::muted;
use crate::modules::aec::ui::style_manager::actions::{self, ActionMessages};
use crate::modules::aec::ui::style_manager::list::{self, filter_entries, ListEntry, ListSpec};
use crate::modules::aec::ui::style_manager::StyleParts;
use crate::t;


pub struct SlabStyleFormState<'a> {
    pub open: bool,
    pub is_new: bool,
    pub name: &'a str,
    pub parent_id: Option<&'a str>,
    pub parent_name: Option<String>,
    pub structural_style_id: Option<&'a str>,
    pub finish_style_id: Option<&'a str>,
    pub referenced_structural: Option<&'a crate::modules::aec::engine::slab_style::SlabStructuralStyle>,
    pub referenced_finish: Option<&'a crate::modules::aec::engine::slab_style::FloorFinishStyle>,
    pub all_slab_styles: Vec<(&'a str, &'a str)>,
    pub all_structural_styles: Vec<(&'a str, &'a str)>,
    pub all_finish_styles: Vec<(&'a str, &'a str)>,
    pub all_materials: Vec<(&'a str, &'a str)>,
    pub effective_layers: Vec<SlabStyleLayer>,
    pub inheritance_chain: Vec<String>,
    pub preview_mode: AecSlabPreviewMode,
    pub display_config_names: Vec<String>,
    pub existing_overrides: Vec<String>,
    /// Effective layer selected by a preview click.
    pub selected_layer: Option<usize>,
    pub profile_selected: Option<&'a str>,
    pub slot_visibility: std::collections::HashMap<SlabComponentSlot, bool>,
}

pub fn parts<'a>(
    library: &'a StyleLibrary,
    project: Option<&'a ProjectFile>,
    session_library: Option<&'a StyleLibrary>,
    filter: &'a str,
    selected_id: Option<&'a str>,
    form: Option<SlabStyleFormState<'a>>,
) -> StyleParts<'a> {
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
    let _ = library;

    let all: Vec<ListEntry> = merged
        .slab_style_tree()
        .into_iter()
        .map(|node| ListEntry {
            id: node.style.style.id.clone(),
            label: format!("{}{}", "  ".repeat(node.depth), node.style.style.name),
            detail: None,
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
            title: t!("Deckenstile").into_owned(),
            search_hint: t!("Search styles...").into_owned(),
            new_label: t!("+ Neuer Deckenstil").into_owned(),
            filter,
            selected: selected_id,
            on_filter: |s| Message::Aec(AecMessage::AecSlabStyleManagerFilter(s)),
            on_select: |id| Message::Aec(AecMessage::AecSlabStyleManagerSelect(id)),
            on_new: Message::Aec(AecMessage::AecSlabStyleManagerNew),
        },
        &rows,
    );

    let open = form.is_some();
    let is_new = form.as_ref().map(|f| f.is_new).unwrap_or(true);
    let selected_source = selected_id.and_then(|id| source_by_id.get(id).copied());
    let has_standard = selected_id.is_some_and(|id| {
        crate::modules::aec::engine::library::load_or_seed()
            .slab_styles
            .iter()
            .any(|s| s.style.id == id)
    });
    let form_el: Element<'a, Message> = match form {
        Some(f) => form_view(f),
        None => container(
            text(t!("Select or create a slab style to edit."))
                .size(12)
                .style(muted),
        )
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill)
        .into(),
    };
    let actions = open.then(|| {
        actions::view(
            ActionMessages {
                save_apply: Message::Aec(AecMessage::AecSlabStyleManagerSaveAndApply),
                save: Message::Aec(AecMessage::AecSlabStyleManagerSave),
                duplicate: Message::Aec(AecMessage::AecSlabStyleManagerDuplicate),
                delete: Message::Aec(AecMessage::AecSlabStyleManagerDelete),
                to_standard: Message::Aec(AecMessage::AecStyleManagerCopySlabStyleToGlobal),
                to_project: Message::Aec(AecMessage::AecStyleManagerCopySlabStyleToProject),
            },
            is_new,
            selected_source.map(StyleOrigin::from),
            has_standard,
        )
    });
    StyleParts { list, form: form_el, actions }
}

fn form_view<'a>(form: SlabStyleFormState<'a>) -> Element<'a, Message> {
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

    let header_row = row![
        text(t!("Name:")).size(11).style(muted),
        name_input,
        Space::new().width(8),
        parent_btn,
    ]
    .align_y(Alignment::Center)
    .spacing(4);

    // Modular sub-styles row (Structural Slab Style + Default Floor Finish Style)
    let mut struct_options = vec![(String::new(), t!("(Kein Rohbaustil)").into_owned())];
    for (id, name) in &form.all_structural_styles {
        struct_options.push((id.to_string(), format!("{name} ({id})")));
    }
    let sel_struct = form.structural_style_id.unwrap_or("").to_string();
    let struct_selected = struct_options.iter().find(|(id, _)| id == &sel_struct).cloned();

    let struct_pick = pick_list(struct_selected, struct_options, |opt| opt.1.clone())
        .on_select(|(id, _)| {
            let val = if id.is_empty() { None } else { Some(id) };
            Message::Aec(AecMessage::AecSlabStyleManagerStructuralStyleChanged(val))
        })
        .text_size(10)
        .padding([3, 6]);

    let mut finish_options = vec![(String::new(), t!("(Kein Ausbau / Rohdecke)").into_owned())];
    for (id, name) in &form.all_finish_styles {
        finish_options.push((id.to_string(), format!("{name} ({id})")));
    }
    let sel_finish = form.finish_style_id.unwrap_or("").to_string();
    let finish_selected = finish_options.iter().find(|(id, _)| id == &sel_finish).cloned();

    let finish_pick = pick_list(finish_selected, finish_options, |opt| opt.1.clone())
        .on_select(|(id, _)| {
            let val = if id.is_empty() { None } else { Some(id) };
            Message::Aec(AecMessage::AecSlabStyleManagerFinishStyleChanged(val))
        })
        .text_size(10)
        .padding([3, 6]);

    let modular_row = row![
        text(t!("Rohbaustil (Tragwerk):")).size(10).style(muted),
        struct_pick,
        Space::new().width(8),
        text(t!("Ausbaustil (Fußbodenaufbau):")).size(10).style(muted),
        finish_pick,
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
            layers: form.effective_layers.clone(),
            selected: form.selected_layer,
            on_select: |i| Message::Aec(AecMessage::AecSlabStyleManagerPreviewLayerSelect(i)),
        })
        .width(Fill)
        .height(140),
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

    // Breakdown cards for Finish style & Structural style
    let mut finish_layers_col = column![].spacing(2);
    let mut finish_th_cm = 0.0;
    if let Some(fs) = &form.referenced_finish {
        let title = text(crate::tf!(
            "▲ Fußbodenaufbau: {} ({}) — {:.1} cm",
            fs.style.name,
            fs.style.id,
            fs.nominal_thickness() * 100.0
        ))
        .size(10)
        .style(muted);
        finish_layers_col = finish_layers_col.push(title);

        for (i, layer) in fs.layers.iter().enumerate() {
            let th = match layer.thickness {
                crate::modules::aec::engine::slab_style::LayerValue::Fixed(v) => v * 100.0,
                _ => 0.0,
            };
            finish_th_cm += th;
            let mat_name = form
                .all_materials
                .iter()
                .find(|(id, _)| *id == layer.material_id.as_str())
                .map(|(_, n)| *n)
                .unwrap_or(&layer.material_id);
            let hatch_str = layer.hatch_override.as_deref().unwrap_or("ANSI31");
            let layer_row = row![
                text(format!("{}.", i + 1)).size(9).width(Length::Fixed(20.0)),
                text(mat_name).size(10).width(Length::Fixed(180.0)),
                text(format!("{th:.1} cm")).size(10).width(Length::Fixed(80.0)),
                text(format!("{:?}", layer.function)).size(9).style(muted).width(Length::Fixed(100.0)),
                text(format!("Schraffur: {hatch_str}")).size(9).style(muted),
            ]
            .align_y(Alignment::Center)
            .spacing(4);
            finish_layers_col =
                finish_layers_col.push(highlight_row(layer_row, form.selected_layer == Some(i)));
        }
    } else {
        finish_layers_col = finish_layers_col.push(
            text(t!("(Kein Ausbaustil zugewiesen / reine Rohdecke)"))
                .size(10)
                .style(muted),
        );
    }

    let finish_card = container(finish_layers_col)
        .width(Fill)
        .padding(6)
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
                radius: 3.0.into(),
                width: 1.0,
                color: theme.palette().background.neutral.color.scale_alpha(0.2),
            },
            ..Default::default()
        });

    let mut struct_layers_col = column![].spacing(2);
    let mut struct_th_cm = 0.0;
    if let Some(ss) = &form.referenced_structural {
        let title = text(format!(
            "▼ Rohdecke: {} ({}) — {:.1} cm",
            ss.style.name,
            ss.style.id,
            ss.nominal_thickness() * 100.0
        ))
        .size(10)
        .style(muted);
        struct_layers_col = struct_layers_col.push(title);

        for (i, layer) in ss.layers.iter().enumerate() {
            let th = match layer.thickness {
                crate::modules::aec::engine::slab_style::LayerValue::Fixed(v) => v * 100.0,
                _ => 0.0,
            };
            struct_th_cm += th;
            let mat_name = form
                .all_materials
                .iter()
                .find(|(id, _)| *id == layer.material_id.as_str())
                .map(|(_, n)| *n)
                .unwrap_or(&layer.material_id);
            let hatch_str = layer.hatch_override.as_deref().unwrap_or("AR-CONC");
            let layer_row = row![
                text(format!("{}.", i + 1)).size(9).width(Length::Fixed(20.0)),
                text(mat_name).size(10).width(Length::Fixed(180.0)),
                text(format!("{th:.1} cm")).size(10).width(Length::Fixed(80.0)),
                text(format!("{:?}", layer.function)).size(9).style(muted).width(Length::Fixed(100.0)),
                text(format!("Schraffur: {hatch_str}")).size(9).style(muted),
            ]
            .align_y(Alignment::Center)
            .spacing(4);
            let effective_index =
                i + form.referenced_finish.as_ref().map_or(0, |f| f.layers.len());
            struct_layers_col = struct_layers_col
                .push(highlight_row(layer_row, form.selected_layer == Some(effective_index)));
        }
    } else {
        struct_layers_col = struct_layers_col.push(
            text(t!("(Kein Rohbaustil zugewiesen)"))
                .size(10)
                .style(muted),
        );
    }

    let struct_card = container(struct_layers_col)
        .width(Fill)
        .padding(6)
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
                radius: 3.0.into(),
                width: 1.0,
                color: theme.palette().background.neutral.color.scale_alpha(0.2),
            },
            ..Default::default()
        });

    let total_th_cm = finish_th_cm + struct_th_cm;
    let summary_label = text(crate::tf!(
        "Gesamtdicke der Decke: {:.1} cm (Ausbau: {:.1} cm + Rohbau: {:.1} cm) | {} Schichten",
        total_th_cm,
        finish_th_cm,
        struct_th_cm,
        form.effective_layers.len()
    ))
    .size(10)
    .style(muted);

    let layers_section = column![
        text(t!("Zusammensetzung & Schichtenübersicht (aus Referenzen):"))
            .size(11)
            .style(muted),
        finish_card,
        Space::new().height(2),
        struct_card,
        Space::new().height(2),
        summary_label,
    ]
    .spacing(4);

    let slots_title = text(t!("Komponenten-Sichtbarkeit:")).size(10).style(muted);
    let profile_picker = crate::modules::aec::ui::style_manager::forms::plan_overrides::plan_override_section(
        crate::modules::aec::ui::style_manager::forms::plan_overrides::PlanOverrideProps {
            all_names: &form.display_config_names,
            existing: &form.existing_overrides,
            selected: form.profile_selected,
            default_key: "",
        },
        |n| Message::Aec(AecMessage::AecSlabStyleManagerProfileSelect(n)),
        |n| Message::Aec(AecMessage::AecSlabStyleManagerProfileSelect(n)),
        |n| Message::Aec(AecMessage::AecSlabStyleManagerProfileRemove(n)),
    );
    let mut slots_row = row![slots_title].spacing(8).align_y(Alignment::Center);
    for slot in EDITABLE_SLOTS {
        let is_vis = form.slot_visibility.get(slot).copied().unwrap_or(true);
        let slot_name = match slot {
            SlabComponentSlot::Contour2D => crate::t!("Kontur 2D"),
            SlabComponentSlot::CeilingOutline2D => crate::t!("Deckenspiegel 2D"),
            SlabComponentSlot::LayerHatch2D => crate::t!("Schraffur 2D"),
            SlabComponentSlot::OpeningContour2D => crate::t!("Öffnungskontur"),
            SlabComponentSlot::OpeningSymbol2D => crate::t!("DIN 1356 Symbol"),
            SlabComponentSlot::Solid3D => crate::t!("Körper 3D"),
            SlabComponentSlot::SurfaceStyle3D => crate::t!("Oberfläche 3D"),
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

    column![
        header_row,
        modular_row,
        Space::new().height(4),
        preview_header,
        preview_box,
        Space::new().height(4),
        layers_section,
        Space::new().height(4),
        profile_picker,
        slots_row,
    ]
    .spacing(6)
    .height(Fill)
    .into()
}

pub(crate) fn highlight_row<'a>(
    row: impl Into<Element<'a, Message>>,
    selected: bool,
) -> Element<'a, Message> {
    container(row)
        .width(Fill)
        .padding([1, 3])
        .style(move |theme: &Theme| container::Style {
            background: selected.then(|| {
                theme.palette().primary.base.color.scale_alpha(0.18).into()
            }),
            border: Border {
                radius: 3.0.into(),
                width: if selected { 1.0 } else { 0.0 },
                color: theme.palette().primary.base.color,
            },
            ..Default::default()
        })
        .into()
}

pub(crate) struct SlabPreviewCanvas {
    pub(crate) paths: Vec<SlabPreviewPath>,
    #[allow(dead_code)]
    pub(crate) mode: AecSlabPreviewMode,
    /// Effective layers, for cross-section hit testing.
    pub(crate) layers: Vec<SlabStyleLayer>,
    pub(crate) selected: Option<usize>,
    /// Message built when a cross-section layer is clicked.
    pub(crate) on_select: fn(usize) -> Message,
}

/// Scale and model-space centre that fit `paths` into a canvas of `size`.
fn fit_paths(paths: &[SlabPreviewPath], size: iced::Size) -> (f32, f32, f32) {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for p in paths {
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
    let avail_w = (size.width - pad * 2.0).max(10.0);
    let avail_h = (size.height - pad * 2.0).max(10.0);
    let span_x = ((max_x - min_x) as f32).max(0.1);
    let span_y = ((max_y - min_y) as f32).max(0.1);
    let scale = (avail_w / span_x).min(avail_h / span_y);
    (scale, ((min_x + max_x) * 0.5) as f32, ((min_y + max_y) * 0.5) as f32)
}

/// Layer whose box in the cross-section preview contains the model point
/// `(x, y)` (boxes span `|x| <= 1.2`, stacked downwards from y = 0).
pub(crate) fn cross_section_hit(layers: &[SlabStyleLayer], x: f64, y: f64) -> Option<usize> {
    if x.abs() > 1.2 {
        return None;
    }
    let mut top = 0.0;
    for (i, layer) in layers.iter().enumerate() {
        let th = layer.thickness.as_fixed_or(0.10).max(0.005);
        if y <= top && y >= top - th {
            return Some(i);
        }
        top -= th;
    }
    None
}

impl Program<Message> for SlabPreviewCanvas {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if self.mode != AecSlabPreviewMode::CrossSection {
            return None;
        }
        let canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event else {
            return None;
        };
        let pos = cursor.position_in(bounds)?;
        let (scale, cx, cy) = fit_paths(&self.paths, bounds.size());
        let x = ((pos.x - bounds.width * 0.5) / scale + cx) as f64;
        let y = ((bounds.height * 0.5 - pos.y) / scale + cy) as f64;
        let index = cross_section_hit(&self.layers, x, y)?;
        Some(
            canvas::Action::publish((self.on_select)(index)).and_capture(),
        )
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        let (scale, cx, cy) = fit_paths(&self.paths, bounds.size());
        let map_pt = |x: f64, y: f64| -> Point {
            Point::new(
                bounds.width * 0.5 + (x as f32 - cx) * scale,
                bounds.height * 0.5 - (y as f32 - cy) * scale,
            )
        };

        if self.mode == AecSlabPreviewMode::CrossSection {
            if let Some(i) = self.selected {
                let top: f64 = self
                    .layers
                    .iter()
                    .take(i)
                    .map(|l| l.thickness.as_fixed_or(0.10).max(0.005))
                    .sum();
                if let Some(layer) = self.layers.get(i) {
                    let th = layer.thickness.as_fixed_or(0.10).max(0.005);
                    let a = map_pt(-1.2, -top);
                    let b = map_pt(1.2, -top - th);
                    frame.stroke(
                        &Path::rectangle(a, iced::Size::new(b.x - a.x, b.y - a.y)),
                        Stroke::default().with_width(3.0).with_color(Color::from_rgb(0.1, 0.6, 0.9)),
                    );
                }
            }
        }

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

#[cfg(test)]
mod hit_tests {
    use super::*;
    use crate::modules::aec::engine::slab_style::{LayerValue, SlabStyleLayer};

    fn layer(th_m: f64) -> SlabStyleLayer {
        SlabStyleLayer::new(
            "mat",
            LayerValue::Fixed(th_m),
            crate::modules::aec::engine::wall_style::LayerFunction::Structural,
        )
    }

    #[test]
    fn cross_section_hit_picks_stacked_layer() {
        let layers = vec![layer(0.1), layer(0.2)];
        assert_eq!(cross_section_hit(&layers, 0.0, -0.05), Some(0));
        assert_eq!(cross_section_hit(&layers, 0.5, -0.2), Some(1));
        assert_eq!(cross_section_hit(&layers, 0.0, -0.5), None);
        assert_eq!(cross_section_hit(&layers, 5.0, -0.05), None);
        assert_eq!(cross_section_hit(&layers, 0.0, 0.1), None);
    }
}
