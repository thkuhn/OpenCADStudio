//! AEC Wall Style Manager — browse and edit wall styles held in the AEC
//! style library (`AEC_STYLEMANAGER`).

use iced::widget::{
    button, column, container, mouse_area, pick_list, row, scrollable, text, text_input,
    Space,
};
use iced::{Border, Element, Fill, Theme};

use crate::app::Message;
use crate::modules::aec::engine::library::{StyleLibrary, TreeNode};
use crate::modules::aec::engine::wall_style::WallStyle;
use crate::t;
use super::aec_ui_util::*;

const LAYER_COL_REORDER_W: f32 = 22.0;
const LAYER_COL_MATERIAL_W: f32 = 180.0;
const LAYER_COL_THICKNESS_W: f32 = 60.0;
const LAYER_COL_GAP_W: f32 = 50.0;
const LAYER_COL_OFFSET_W: f32 = 50.0;
const LAYER_COL_FUNCTION_W: f32 = 120.0;
const LAYER_COL_OVERRIDE_W: f32 = 130.0;

pub struct WallStyleFormState<'a> {
    pub open: bool,
    pub is_new: bool,
    pub name: &'a str,
    pub parent_id: Option<&'a str>,
    pub layers: &'a [crate::app::AecLayerBuffer],
    pub drag_index: Option<usize>,
    pub all_wall_styles: Vec<(&'a str, &'a str)>,
    pub all_materials: Vec<(&'a str, &'a str)>,
    pub all_layer_names: Vec<String>,
    pub effective_layers: Vec<crate::modules::aec::engine::wall_style::Layer>,
}

pub fn view_window<'a>(
    library: &'a StyleLibrary,
    selected_id: Option<&str>,
    filter: &str,
    wall_style_form: WallStyleFormState<'a>,
) -> Element<'a, Message> {
    let tree = library.wall_style_tree();
    let filter_lower = filter.to_lowercase();

    let wall_style_rows: Vec<Element<'a, Message>> = tree
        .into_iter()
        .filter(|node| {
            filter.is_empty()
                || node.style.style.name.to_lowercase().contains(&filter_lower)
                || node.style.style.id.to_lowercase().contains(&filter_lower)
        })
        .map(|node| {
            wall_style_tree_row(node.style, node.depth, selected_id == Some(&node.style.style.id))
        })
        .collect();

    let mut master_list = column![section_title(t!("Wall Styles"))].spacing(2);
    master_list = if wall_style_rows.is_empty() {
        master_list.push(no_matches())
    } else {
        master_list.extend(wall_style_rows)
    };

    let sidebar = column![
        row![
            text_input(t!("Search styles…").as_ref(), filter)
                .on_input(Message::AecStyleManagerFilter)
                .size(11)
                .padding([4, 6]),
            button(text("+").size(11))
                .on_press(Message::AecStyleManagerWallStyleNew)
                .padding([4, 8]),
        ]
        .spacing(4),
        scrollable(master_list),
    ]
    .spacing(8)
    .width(250);

    let detail = if wall_style_form.open {
        wall_style_form_view(wall_style_form)
    } else {
        container(text(t!("Select a wall style to edit or create a new one.")).style(muted))
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

fn wall_style_tree_row<'a>(
    wall_style: &'a WallStyle,
    depth: usize,
    selected: bool,
) -> Element<'a, Message> {
    let subtitle = crate::tf!(
        "{count} layer(s)",
        count = wall_style.layers.len()
    );
    let mut row_content = row![].spacing(4).align_y(iced::Center);
    if depth > 0 {
        row_content = row_content.push(Space::new().width(12.0 * depth as f32));
        row_content = row_content.push(text("└").size(10).style(muted));
    }

    button(
        row_content.push(
            column![
                text(wall_style.style.name.as_str()).size(12),
                text(subtitle).size(10).style(muted),
            ]
            .spacing(2),
        ),
    )
    .on_press(Message::AecStyleManagerSelectWallStyle(
        wall_style.style.id.clone(),
    ))
    .style(list_style(selected))
    .padding([6, 9])
    .width(Fill)
    .into()
}

fn wall_style_form_view<'a>(wall_style_form: WallStyleFormState<'a>) -> Element<'a, Message> {
    let form_title = if wall_style_form.is_new {
        t!("New Wall Style")
    } else {
        t!("Wall Style")
    };

    let none_label = t!("(None)");
    let selected_parent = wall_style_form
        .parent_id
        .and_then(|pid| {
            wall_style_form
                .all_wall_styles
                .iter()
                .find(|(id, _)| id == &pid)
                .map(|(_, name)| name.to_string())
        })
        .unwrap_or_else(|| none_label.into_owned());

    let all_materials_ref = &wall_style_form.all_materials;
    let all_layer_names_ref = &wall_style_form.all_layer_names;
    let layer_count = wall_style_form.layers.len();
    let drag_index = wall_style_form.drag_index;
    let layer_rows: Vec<Element<'_, Message>> = wall_style_form
        .layers
        .iter()
        .enumerate()
        .map(|(i, lb)| {
            layer_row(i, layer_count, lb, all_materials_ref, all_layer_names_ref, drag_index)
        })
        .collect();

    let mut detail_col = column![
        text(form_title).size(13),
        row![
            text(t!("Name")).size(10).style(muted).width(100),
            text_input("", wall_style_form.name)
                .on_input(Message::AecStyleManagerWallStyleNameChanged)
                .size(11)
                .padding([4, 6]),
        ]
        .spacing(8),
        row![
            text(t!("Based on")).size(10).style(muted).width(100),
            button(
                row![
                    text(selected_parent).size(11),
                    Space::new(),
                    text("▾").size(9),
                ]
                .align_y(iced::Center),
            )
            .on_press(Message::AecStylePickerOpen(
                crate::app::StylePickerTarget::WallStyleParent
            ))
            .style(button::subtle)
            .padding([4, 6])
            .width(Fill),
        ]
        .spacing(8),
        row![
            text(t!("Layers")).size(10).style(muted),
            Space::new(),
            button(text(t!("Add Layer")).size(10))
                .on_press(Message::AecStyleManagerWallStyleLayerAdd)
                .padding([2, 8]),
        ]
        .spacing(8),
        layer_header_row(),
        container(scrollable(column(layer_rows).spacing(4)).height(180))
            .style(|theme: &Theme| container::Style {
                border: Border {
                    color: theme.palette().background.neutral.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            })
            .padding(4),
    ]
    .spacing(7);

    if !wall_style_form.effective_layers.is_empty() {
        detail_col = detail_col.push(Space::new().height(8)).push(
            text(t!("Effective Buildup (Preview)"))
                .size(10)
                .style(muted),
        );
        let preview_rows: Vec<Element<'_, Message>> = wall_style_form
            .effective_layers
            .iter()
            .map(|l| {
                let mat_name = wall_style_form
                    .all_materials
                    .iter()
                    .find(|(id, _)| *id == l.material_id)
                    .map(|(_, name)| name.to_string())
                    .unwrap_or_else(|| l.material_id.clone());
                row![
                    Space::new().width(LAYER_COL_REORDER_W),
                    text(mat_name).size(11).width(LAYER_COL_MATERIAL_W),
                    text(format!("{:.2}", l.thickness))
                        .size(11)
                        .width(LAYER_COL_THICKNESS_W),
                    text(format!("{:.2}", l.gap_before))
                        .size(11)
                        .width(LAYER_COL_GAP_W),
                    text(format!("{:.2}", l.bottom_offset))
                        .size(11)
                        .width(LAYER_COL_OFFSET_W),
                    text(format!("{:.2}", l.top_offset))
                        .size(11)
                        .width(LAYER_COL_OFFSET_W),
                    text(format!("{:?}", l.function))
                        .size(11)
                        .width(LAYER_COL_FUNCTION_W),
                ]
                .spacing(8)
                .into()
            })
            .collect();
        detail_col = detail_col.push(
            container(scrollable(column(preview_rows).spacing(4)).height(100))
                .padding(4)
                .style(|theme: &Theme| container::Style {
                    background: Some(theme.palette().background.neutral.color.scale_alpha(0.2).into()),
                    ..Default::default()
                }),
        );
    }

    let mut actions = row![
        Space::new(),
        button(text(t!("Save")).size(11))
            .style(button::subtle)
            .padding([5, 12])
            .on_press(Message::AecStyleManagerWallStyleSave),
        button(text(t!("Save & Update Drawing")).size(11))
            .style(button::primary)
            .padding([5, 12])
            .on_press(Message::AecStyleManagerWallStyleSaveAndApply),
    ]
    .spacing(8);

    if !wall_style_form.is_new {
        actions = actions.push(
            button(text(t!("Delete")).size(11))
                .style(button::danger)
                .padding([5, 12])
                .on_press(Message::AecStyleManagerWallStyleDelete),
        );
    }

    column![detail_col, Space::new(), actions]
        .spacing(10)
        .height(Fill)
        .into()
}

fn layer_header_row<'a>() -> Element<'a, Message> {
    row![
        Space::new().width(LAYER_COL_REORDER_W),
        text(t!("Material")).size(10).style(muted).width(LAYER_COL_MATERIAL_W),
        text(t!("Thick")).size(10).style(muted).width(LAYER_COL_THICKNESS_W),
        text(t!("Gap")).size(10).style(muted).width(LAYER_COL_GAP_W),
        text(t!("Bot.")).size(10).style(muted).width(LAYER_COL_OFFSET_W),
        text(t!("Top")).size(10).style(muted).width(LAYER_COL_OFFSET_W),
        text(t!("Function")).size(10).style(muted).width(LAYER_COL_FUNCTION_W),
        text(t!("Lyr. Over.")).size(10).style(muted).width(LAYER_COL_OVERRIDE_W),
        Space::new().width(24), // Delete button space
    ]
    .spacing(8)
    .into()
}

fn layer_row<'a>(
    index: usize,
    layer_count: usize,
    buffer: &'a crate::app::AecLayerBuffer,
    all_materials: &[(&'a str, &'a str)],
    _all_layer_names: &[String],
    drag_index: Option<usize>,
) -> Element<'a, Message> {
    let functions: Vec<String> = vec![
        "Structural".to_string(),
        "Insulation".to_string(),
        "Finish".to_string(),
        "Other".to_string(),
    ];

    let mut up_button = button(text("▲").size(10)).padding([2, 6]);
    if index > 0 {
        up_button = up_button.on_press(Message::AecStyleManagerWallStyleLayerMoveUp(index));
    }
    let mut down_button = button(text("▼").size(10)).padding([2, 6]);
    if index + 1 < layer_count {
        down_button = down_button.on_press(Message::AecStyleManagerWallStyleLayerMoveDown(index));
    }

    let armed = drag_index == Some(index);
    let handle = mouse_area(
        button(text("⠿").size(12))
            .style(if armed { button::primary } else { button::subtle })
            .padding([2, 6])
            .on_press(if armed {
                Message::AecStyleManagerWallStyleLayerDragEnd
            } else if drag_index.is_some() {
                Message::AecStyleManagerWallStyleLayerDragOver(index)
            } else {
                Message::AecStyleManagerWallStyleLayerDragStart(index)
            }),
    )
    .interaction(iced::mouse::Interaction::Grab);

    row![
        column![handle, up_button, down_button].spacing(2).width(LAYER_COL_REORDER_W),
        {
            let label = if buffer.material_id.is_empty() {
                t!("(None)").into_owned()
            } else {
                all_materials
                    .iter()
                    .find(|(id, _)| *id == buffer.material_id)
                    .map(|(_, name)| name.to_string())
                    .unwrap_or_else(|| buffer.material_id.clone())
            };
            button(
                row![
                    text(label).size(11),
                    Space::new(),
                    text("▾").size(9),
                ]
                .align_y(iced::Center),
            )
            .on_press(Message::AecStylePickerOpen(
                crate::app::StylePickerTarget::LayerMaterial(index),
            ))
            .style(button::subtle)
            .padding([4, 6])
            .width(LAYER_COL_MATERIAL_W)
        },
        text_input("", &buffer.thickness)
            .on_input(move |v| Message::AecStyleManagerWallStyleLayerThicknessChanged(index, v))
            .size(11)
            .width(LAYER_COL_THICKNESS_W),
        text_input("gap", &buffer.gap_before)
            .on_input(move |v| Message::AecStyleManagerWallStyleLayerGapChanged(index, v))
            .size(11)
            .width(LAYER_COL_GAP_W),
        text_input("bottom", &buffer.bottom_offset)
            .on_input(move |v| Message::AecStyleManagerWallStyleLayerBottomOffsetChanged(index, v))
            .size(11)
            .width(LAYER_COL_OFFSET_W),
        text_input("top", &buffer.top_offset)
            .on_input(move |v| Message::AecStyleManagerWallStyleLayerTopOffsetChanged(index, v))
            .size(11)
            .width(LAYER_COL_OFFSET_W),
        pick_list(Some(buffer.function.clone()), functions, |func: &String| {
            func.clone()
        })
        .on_select(move |func| Message::AecStyleManagerWallStyleLayerFunctionChanged(index, func))
        .text_size(11)
        .width(LAYER_COL_FUNCTION_W),
        {
            let label = if buffer.layer_override.is_empty() {
                t!("(Default)").into_owned()
            } else {
                buffer.layer_override.clone()
            };
            button(
                row![
                    text(label).size(11),
                    Space::new(),
                    text("▾").size(9),
                ]
                .align_y(iced::Center),
            )
            .on_press(Message::AecStylePickerOpen(
                crate::app::StylePickerTarget::LayerOverride(index),
            ))
            .style(button::subtle)
            .padding([4, 6])
            .width(LAYER_COL_OVERRIDE_W)
        },
        button(text("✕").size(10))
            .style(button::danger)
            .on_press(Message::AecStyleManagerWallStyleLayerRemove(index))
            .padding([4, 8]),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}
