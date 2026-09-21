//! Opening Style Manager modal: list, form, slot table, 2D generator/sketch preview.

use iced::mouse;
use iced::widget::canvas::{Frame, Path, Program, Stroke};
use iced::widget::{button, canvas, column, container, pick_list, row, scrollable, text, text_input, Space};
use iced::{Border, Color, Element, Fill as FillLen, Point, Rectangle, Theme};

use crate::app::Message;
use crate::modules::aec::engine::library::{LibrarySource, StyleLibrary};
use crate::modules::aec::engine::opening_display::BakedPath;
use crate::modules::aec::engine::opening_shape::OpeningShape;
use crate::modules::aec::engine::opening_sketch::{tessellate_path, OpeningSketchPath, TwoRectBake};
use crate::modules::aec::engine::opening_style::{OpeningGenerator, OpeningStyle, HingeSide};
use crate::modules::aec::engine::openings::OpeningKind;
use crate::modules::aec::engine::project::ProjectFile;
use crate::modules::aec::message::AecMessage;
use crate::modules::aec::state::{AecOpeningSlotBuffer, AecOpeningSlotSource, StylePickerTarget};
use crate::modules::aec::styles::opening_style_manager::{
    preview_baked_paths, slots_from_buffers, PREVIEW_WALL_THICKNESS,
};
use crate::modules::aec::ui::aec_ui_util::{list_style, muted, no_matches, section_title};
use crate::t;
use crate::tr;

pub struct OpeningStyleFormState<'a> {
    pub open: bool,
    pub is_new: bool,
    pub name: &'a str,
    pub parent_id: Option<&'a str>,
    pub parent_name: Option<&'a str>,
    pub kind: OpeningKind,
    pub shape: OpeningShape,
    pub width: &'a str,
    pub height: &'a str,
    pub sill: &'a str,
    pub frame: &'a str,
    pub angle: &'a str,
    pub spring: &'a str,
    pub hinge: HingeSide,
    pub slots: &'a [AecOpeningSlotBuffer],
    pub profile_selected: Option<&'a str>,
    pub plan_names: Vec<String>,
    pub sketch_slot: Option<usize>,
    pub sketch_draft: &'a [(f64, f64)],
    pub sketch_draft_bulges: &'a [f64],
}

pub fn view_window<'a>(
    library: &'a StyleLibrary,
    project: Option<&'a ProjectFile>,
    session: Option<&'a StyleLibrary>,
    selected_id: Option<&str>,
    filter: &str,
    form: OpeningStyleFormState<'a>,
) -> Element<'a, Message> {
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
    for m in &library.materials {
        merged.upsert_material(m.clone());
    }

    let filter_lower = filter.to_lowercase();
    let owned_rows: Vec<(OpeningStyle, usize, LibrarySource)> = merged
        .opening_style_tree()
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

    let style_rows: Vec<Element<'_, Message>> = owned_rows
        .into_iter()
        .map(|(style, depth, source)| {
            let selected = selected_id == Some(style.style.id.as_str());
            tree_row(
                style.style.id,
                style.style.name,
                style.kind,
                depth,
                source,
                selected,
            )
        })
        .collect();

    let mut master_list = column![section_title(t!("Opening Styles"))].spacing(2);
    if style_rows.is_empty() {
        master_list = master_list.push(no_matches());
    } else {
        for row_el in style_rows {
            master_list = master_list.push(row_el);
        }
    }

    let sidebar = column![
        row![
            text_input(t!("Search opening styles…").as_ref(), filter)
                .on_input(|v| Message::Aec(AecMessage::AecOpeningStyleManagerFilter(v)))
                .size(11)
                .padding([4, 6]),
            button(text("+").size(11))
                .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerNew))
                .padding([4, 8]),
        ]
        .spacing(4),
        scrollable(master_list),
    ]
    .spacing(8)
    .width(270);

    let selected_source = selected_id.and_then(|id| source_by_id.get(id).copied());
    let selected_has_standard_counterpart = selected_id
        .map(|id| standard_ids.contains(id))
        .unwrap_or(false);

    let detail = if form.open {
        form_view(
            library,
            form,
            selected_source,
            selected_has_standard_counterpart,
        )
    } else {
        container(text(t!("Select an opening style to edit or create a new one.")).style(muted))
            .width(FillLen)
            .height(FillLen)
            .center_x(FillLen)
            .center_y(FillLen)
            .into()
    };

    row![sidebar, detail]
        .spacing(16)
        .padding(12)
        .into()
}

fn tree_row<'a>(
    id: String,
    name: String,
    kind: OpeningKind,
    depth: usize,
    source: LibrarySource,
    selected: bool,
) -> Element<'a, Message> {
    let indent = "    ".repeat(depth);
    let badge = match source {
        LibrarySource::Standard => t!("Std"),
        LibrarySource::Project => t!("Prj"),
        LibrarySource::Session => t!("Ses"),
    };
    let label = format!("{indent}{name}  [{}]  {badge}", kind.as_str());
    button(text(label).size(11))
        .style(list_style(selected))
        .padding([4, 6])
        .width(FillLen)
        .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSelect(id)))
        .into()
}

fn form_view<'a>(
    library: &'a StyleLibrary,
    form: OpeningStyleFormState<'a>,
    selected_source: Option<LibrarySource>,
    has_standard_counterpart: bool,
) -> Element<'a, Message> {
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
            text(tr!("aec", "opening-frame-thickness")).size(10).style(muted).width(100),
            text_input("", form.frame)
                .on_input(|v| Message::Aec(AecMessage::AecOpeningStyleManagerFrameChanged(v)))
                .size(11)
                .padding([4, 6])
                .width(120),
            text(tr!("aec", "opening-angle")).size(10).style(muted).width(60),
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
            cursor = os.style.parent_style_id.as_deref();
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

    let default_plan = tr!("aec", "opening-plan-default").to_string();
    let mut plan_options = vec![default_plan.clone()];
    plan_options.extend(form.plan_names.iter().cloned());
    let selected_plan = form
        .profile_selected
        .map(str::to_string)
        .unwrap_or_else(|| default_plan.clone());
    let default_plan_select = default_plan.clone();
    detail_col = detail_col
        .push(Space::new().height(6))
        .push(
            row![
                text(tr!("aec", "opening-plan-type")).size(10).style(muted).width(100),
                pick_list(
                    Some(selected_plan),
                    plan_options,
                    |v: &String| v.clone(),
                )
                .on_select(move |v| {
                    let value = if v == default_plan_select {
                        String::new()
                    } else {
                        v
                    };
                    Message::Aec(AecMessage::AecOpeningStyleManagerProfileSelect(value))
                })
                .text_size(11)
                .width(220),
            ]
            .spacing(8)
            .align_y(iced::Center),
        )
        .push(text(tr!("aec", "opening-slots")).size(10).style(muted))
        .push(slot_header(form.profile_selected.is_some()))
        .push(
            container(scrollable(column(slot_rows(
                form.slots,
                form.sketch_slot,
                form.profile_selected.is_some(),
            )).spacing(4)).height(150))
                .style(|theme: &Theme| container::Style {
                    border: Border {
                        color: theme.palette().background.neutral.color,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                })
                .padding(4),
        )
        .push(sketch_toolbar(&form));

    let preview_style = form_preview_style(&form);
    let preview_width = crate::modules::aec::styles::opening_style_manager::parse_dim(form.width)
        .unwrap_or(preview_style.default_width);
    let frame = crate::modules::aec::styles::opening_style_manager::parse_dim(form.frame)
        .unwrap_or(0.06);
    let paths = preview_baked_paths(&preview_style, preview_width);
    let sketch_edit = form
        .sketch_slot
        .and_then(|i| form.slots.get(i))
        .is_some_and(|b| b.source == AecOpeningSlotSource::Sketch);
    let draft_inst = draft_in_instance(&form, preview_width, frame);
    detail_col = detail_col
        .push(Space::new().height(8))
        .push(text(tr!("aec", "opening-preview")).size(10).style(muted))
        .push(
            container(
                canvas(OpeningPreviewCanvas {
                    paths,
                    width: preview_width.max(1e-6),
                    thickness: PREVIEW_WALL_THICKNESS,
                    frame: frame.max(0.0),
                    edit: sketch_edit,
                    draft: draft_inst,
                })
                    .width(FillLen)
                    .height(200),
            )
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
            }),
        );

    let mut actions = row![
        Space::new(),
        button(text(t!("Save")).size(11))
            .style(button::subtle)
            .padding([5, 12])
            .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSave)),
    ]
    .spacing(8);

    if !form.is_new {
        actions = actions.push(
            button(text(t!("Delete")).size(11))
                .style(button::danger)
                .padding([5, 12])
                .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerDelete)),
        );
        if selected_source == Some(LibrarySource::Project) && !has_standard_counterpart {
            actions = actions.push(
                button(text(t!("→ Standard")).size(11))
                    .padding([5, 12])
                    .on_press(Message::Aec(AecMessage::AecStyleManagerCopyOpeningStyleToGlobal)),
            );
        }
        if selected_source == Some(LibrarySource::Standard) {
            actions = actions.push(
                button(text(t!("→ Projekt")).size(11))
                    .padding([5, 12])
                    .on_press(Message::Aec(AecMessage::AecStyleManagerCopyOpeningStyleToProject)),
            );
        }
    }

    column![detail_col, Space::new(), actions]
        .spacing(10)
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
    crate::modules::aec::styles::opening_style_manager::draft_opening_style(
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
        {
            let mut slots = slots_from_buffers(form.slots);
            if form.profile_selected.is_some() {
                for buf in form.slots {
                    if !buf.visible {
                        slots.remove(&buf.slot);
                    }
                }
            }
            slots
        },
    )
    .unwrap_or_else(|_| OpeningStyle::standard_window())
}

fn slot_header<'a>(show_visible: bool) -> Element<'a, Message> {
    let mut r = row![
        text(t!("Slot")).size(10).style(muted).width(90),
        text(tr!("aec", "opening-slot-source")).size(10).style(muted).width(110),
        text(tr!("aec", "opening-slot-generator")).size(10).style(muted).width(140),
    ]
    .spacing(8);
    if show_visible {
        r = r.push(text(tr!("aec", "opening-slot-visible")).size(10).style(muted).width(70));
    }
    r.into()
}

fn slot_rows(
    slots: &[AecOpeningSlotBuffer],
    selected: Option<usize>,
    show_visible: bool,
) -> Vec<Element<'_, Message>> {
    let sources = vec!["Generator".to_string(), "Sketch".to_string()];
    let generators: Vec<String> = OpeningGenerator::catalogue()
        .iter()
        .map(|g| g.as_str().to_string())
        .collect();
    slots
        .iter()
        .enumerate()
        .map(|(index, buf)| {
            let source = match buf.source {
                AecOpeningSlotSource::Generator => "Generator",
                AecOpeningSlotSource::Sketch => "Sketch",
            };
            let mut gen = pick_list(
                Some(buf.generator.as_str().to_string()),
                generators.clone(),
                |v: &String| v.clone(),
            )
            .text_size(11)
            .width(140);
            if buf.source == AecOpeningSlotSource::Generator {
                gen = gen.on_select(move |v| {
                    Message::Aec(AecMessage::AecOpeningStyleManagerSlotGeneratorChanged(
                        index, v,
                    ))
                });
            }
            let selected_row = selected == Some(index);
            let mut r = row![
                button(text(buf.slot.key()).size(11))
                    .style(list_style(selected_row))
                    .padding([2, 4])
                    .width(90)
                    .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSketchSlotSelect(
                        index,
                    ))),
                pick_list(
                    Some(source.to_string()),
                    sources.clone(),
                    |v: &String| v.clone(),
                )
                .on_select(move |v| {
                    Message::Aec(AecMessage::AecOpeningStyleManagerSlotSourceChanged(index, v))
                })
                .text_size(11)
                .width(110),
                gen,
            ]
            .spacing(8);
            if show_visible {
                let checked = buf.visible;
                r = r.push(
                    iced::widget::checkbox(checked)
                        .on_toggle(move |v| {
                            Message::Aec(AecMessage::AecOpeningStyleManagerSlotVisible(index, v))
                        })
                        .size(13),
                );
            }
            r.into()
        })
        .collect()
}

fn sketch_toolbar<'a>(form: &OpeningStyleFormState<'_>) -> Element<'a, Message> {
    let editing = form
        .sketch_slot
        .and_then(|i| form.slots.get(i))
        .is_some_and(|b| b.source == AecOpeningSlotSource::Sketch);
    if !editing {
        return text(tr!("aec", "opening-sketch-hint"))
            .size(10)
            .style(muted)
            .into();
    }
    column![
        text(tr!("aec", "opening-sketch-click")).size(10).style(muted),
        row![
            button(text(tr!("aec", "opening-sketch-frame")).size(10))
                .padding([3, 6])
                .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSketchAddFrame)),
            button(text(tr!("aec", "opening-sketch-close")).size(10))
                .padding([3, 6])
                .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSketchClosePath)),
            button(text(tr!("aec", "opening-sketch-finish")).size(10))
                .padding([3, 6])
                .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSketchFinishPath)),
            button(text(tr!("aec", "opening-sketch-arc")).size(10))
                .padding([3, 6])
                .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSketchArc)),
            button(text(tr!("aec", "opening-sketch-undo")).size(10))
                .padding([3, 6])
                .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSketchUndo)),
            button(text(tr!("aec", "opening-sketch-delete-path")).size(10))
                .padding([3, 6])
                .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSketchDeletePath)),
            button(text(tr!("aec", "opening-sketch-clear")).size(10))
                .padding([3, 6])
                .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerSketchClear)),
        ]
        .spacing(4),
    ]
    .spacing(4)
    .into()
}

fn draft_in_instance(
    form: &OpeningStyleFormState<'_>,
    inst_width: f64,
    frame: f64,
) -> Vec<(f64, f64)> {
    let Some(index) = form.sketch_slot else {
        return Vec::new();
    };
    let Some(buf) = form.slots.get(index) else {
        return Vec::new();
    };
    if buf.source != AecOpeningSlotSource::Sketch || form.sketch_draft.is_empty() {
        return Vec::new();
    }
    let bake = TwoRectBake::from_sketch(
        &buf.sketch,
        inst_width.max(1e-6),
        PREVIEW_WALL_THICKNESS,
        frame.max(0.0),
    );
    let path = OpeningSketchPath {
        points: form.sketch_draft.to_vec(),
        bulges: form.sketch_draft_bulges.to_vec(),
        closed: false,
    };
    tessellate_path(&path)
        .into_iter()
        .map(|p| bake.map_point(p))
        .collect()
}

struct PreviewCamera {
    scale: f32,
    ox: f32,
    oy: f32,
}

impl PreviewCamera {
    fn fit(bounds: Rectangle, width: f64, thickness: f64) -> Self {
        let pad = 14.0f32;
        let world_w = width.max(1e-6) as f32 * 1.15;
        let world_h = thickness.max(1e-6) as f32 * 1.8;
        let avail_w = (bounds.width - 2.0 * pad).max(1.0);
        let avail_h = (bounds.height - 2.0 * pad).max(1.0);
        let scale = (avail_w / world_w).min(avail_h / world_h);
        Self {
            scale,
            ox: bounds.width * 0.5,
            oy: bounds.height * 0.5,
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

struct OpeningPreviewCanvas {
    paths: Vec<BakedPath>,
    width: f64,
    thickness: f64,
    frame: f64,
    edit: bool,
    draft: Vec<(f64, f64)>,
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
        if !self.edit {
            return None;
        }
        let canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event else {
            return None;
        };
        let pos = cursor.position_in(bounds)?;
        let cam = PreviewCamera::fit(bounds, self.width, self.thickness);
        let (x, y) = cam.to_local(pos);
        Some(
            canvas::Action::publish(Message::Aec(AecMessage::AecOpeningStyleManagerSketchClick(
                x, y,
            )))
            .and_capture(),
        )
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if self.edit {
            mouse::Interaction::Crosshair
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
        let cam = PreviewCamera::fit(bounds, self.width, self.thickness);
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

        let stroke_color = theme.palette().background.base.text;
        let fill_color = stroke_color.scale_alpha(0.18);
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
            if path.filled && path.closed {
                frame.fill(&built, fill_color);
            }
            frame.stroke(
                &built,
                Stroke::default()
                    .with_width(1.2)
                    .with_color(stroke_color),
            );
        }

        if !self.draft.is_empty() {
            let draft_color = Color::from_rgb(0.85, 0.4, 0.08);
            if self.draft.len() == 1 {
                let c = cam.to_screen(self.draft[0].0, self.draft[0].1);
                frame.fill(&Path::circle(c, 3.0), draft_color);
            } else {
                let first = cam.to_screen(self.draft[0].0, self.draft[0].1);
                let pts = self.draft.clone();
                let built = Path::new(|p| {
                    p.move_to(first);
                    for &(x, y) in &pts[1..] {
                        p.line_to(cam.to_screen(x, y));
                    }
                });
                frame.stroke(
                    &built,
                    Stroke::default().with_width(1.6).with_color(draft_color),
                );
            }
        }
        vec![frame.into_geometry()]
    }
}
