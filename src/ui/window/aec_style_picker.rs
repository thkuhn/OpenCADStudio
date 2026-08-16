use iced::widget::{
    button, column, container, row, scrollable, text, text_input, Space,
};
use iced::{Alignment, Border, Element, Fill, Theme};

use crate::app::Message;
use crate::modules::aec::engine::library::StyleLibrary;
use crate::t;

fn list_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        if selected {
            button::primary(theme, status)
        } else {
            button::subtle(theme, status)
        }
    }
}

pub fn view_window<'a>(
    library: Option<&'a StyleLibrary>,
    target: crate::app::StylePickerTarget,
    filter: &'a str,
    selection: Option<&'a str>,
    all_layer_names: Vec<String>,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let Some(library) = library else {
        return container(text(t!("No style library loaded.")).size(12))
            .padding(16)
            .width(Fill)
            .into();
    };

    let title = match target {
        crate::app::StylePickerTarget::WallStyleParent => t!("Select Parent Style"),
        crate::app::StylePickerTarget::LayerMaterial(_) => t!("Select Material"),
        crate::app::StylePickerTarget::LayerOverride(_) => t!("Select Layer Override"),
        crate::app::StylePickerTarget::WallPropertiesStyle => t!("Select Wall Style"),
        crate::app::StylePickerTarget::ActiveCommand => t!("Select Wall Style"),
    };

    let query = filter.trim().to_lowercase();

    let content: Element<'_, Message> = match target {
        crate::app::StylePickerTarget::WallStyleParent
        | crate::app::StylePickerTarget::WallPropertiesStyle
        | crate::app::StylePickerTarget::ActiveCommand => {
            let tree = library.wall_style_tree();
            
            // If filtering, identify which nodes must remain visible (matches + their parents).
            let mut visible_ids = std::collections::HashSet::new();
            if !query.is_empty() {
                let styles_map: std::collections::HashMap<_, _> = library
                    .wall_styles
                    .iter()
                    .map(|ws| (ws.style.id.clone(), ws.style.clone()))
                    .collect();

                for node in &tree {
                    if node.style.style.name.to_lowercase().contains(&query) {
                        if let Ok(chain) = crate::modules::aec::engine::style::resolve_chain(&styles_map, &node.style.style.id) {
                            for id in chain {
                                visible_ids.insert(id);
                            }
                        }
                    }
                }
            }

            let rows: Vec<Element<'_, Message>> = tree
                .into_iter()
                .filter(|node| {
                    query.is_empty() || visible_ids.contains(&node.style.style.id)
                })
                .map(|node| {
                    let is_selected = selection == Some(node.style.style.id.as_str());
                    button(
                        row![
                            Space::new().width(node.depth as f32 * 20.0),
                            text(node.style.style.name.as_str()).size(12),
                        ]
                        .align_y(Alignment::Center),
                    )
                    .on_press(Message::AecStylePickerSelect(node.style.style.id.clone()))
                    .style(list_style(is_selected))
                    .padding([4, 8])
                    .width(Fill)
                    .into()
                })
                .collect();
            scrollable(column(rows).spacing(2)).into()
        }
        crate::app::StylePickerTarget::LayerMaterial(_) => {
            let rows: Vec<Element<'_, Message>> = library.materials
                .iter()
                .filter(|m| query.is_empty() || m.name.to_lowercase().contains(&query))
                .map(|m| {
                    let is_selected = selection == Some(m.id.as_str());
                    button(text(m.name.as_str()).size(12))
                        .on_press(Message::AecStylePickerSelect(m.id.clone()))
                        .style(list_style(is_selected))
                        .padding([4, 8])
                        .width(Fill)
                        .into()
                })
                .collect();
            scrollable(column(rows).spacing(2)).into()
        }
        crate::app::StylePickerTarget::LayerOverride(_) => {
            let default_label = t!("(Default)").into_owned();
            let rows: Vec<Element<'_, Message>> = std::iter::once(default_label.clone())
                .chain(all_layer_names.into_iter())
                .filter(|name| query.is_empty() || name.to_lowercase().contains(&query))
                .map(|name| {
                    let value = if name == default_label { String::new() } else { name.clone() };
                    let is_selected = selection == Some(value.as_str())
                        || (selection.is_none() && name == default_label);
                    button(text(name).size(12))
                        .on_press(Message::AecStylePickerSelect(value))
                        .style(list_style(is_selected))
                        .padding([4, 8])
                        .width(Fill)
                        .into()
                })
                .collect();
            scrollable(column(rows).spacing(2)).into()
        }
    };

    column![
        row![
            text(title).size(14),
            Space::new().width(Fill),
            button(text("✕").size(12))
                .style(button::subtle)
                .on_press(Message::AecStylePickerCancel),
        ]
        .align_y(Alignment::Center),
        text_input(t!("Search...").as_ref(), filter)
            .on_input(Message::AecStylePickerFilterChanged)
            .size(12)
            .padding([4, 8]),
        {
            let mut main_content = row![container(content)
                .height(Fill)
                .width(Fill)
                .style(|theme: &Theme| container::Style {
                    border: Border {
                        color: theme.palette().background.neutral.color,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                })]
            .spacing(10);

            if matches!(
                target,
                crate::app::StylePickerTarget::WallStyleParent
                    | crate::app::StylePickerTarget::WallPropertiesStyle
                    | crate::app::StylePickerTarget::ActiveCommand
            ) {
                if let Some(selected_id) = selection {
                    let styles_map: std::collections::HashMap<_, _> = library
                        .wall_styles
                        .iter()
                        .map(|ws| (ws.style.id.clone(), ws.clone()))
                        .collect();

                    if let Ok(effective) = crate::modules::aec::engine::wall_style::effective_layers(
                        &styles_map,
                        &selected_id.to_string(),
                    ) {
                        if !effective.is_empty() {
                            let preview_rows: Vec<Element<'_, Message>> = effective
                                .into_iter()
                                .map(|l| {
                                    let mat_name = library
                                        .materials
                                        .iter()
                                        .find(|m| m.id == l.material_id)
                                        .map(|m| m.name.clone())
                                        .unwrap_or_else(|| l.material_id.clone());
                                    row![
                                        text(mat_name).size(10).width(120),
                                        text(format!("{:.2}", l.thickness)).size(10).width(40),
                                        text(format!("{:?}", l.function)).size(10).width(70),
                                    ]
                                    .spacing(4)
                                    .into()
                                })
                                .collect();

                            let preview_pane = column![
                                text(t!("Effective Buildup (Preview)")).size(11).style(
                                    |theme: &Theme| iced::widget::text::Style {
                                        color: Some(
                                            theme.palette().background.base.text.scale_alpha(0.65)
                                        ),
                                    }
                                ),
                                row![
                                    text(t!("Material")).size(9).style(
                                        |theme: &Theme| iced::widget::text::Style {
                                            color: Some(
                                                theme.palette().background.base.text.scale_alpha(
                                                    0.65
                                                )
                                            ),
                                        }
                                    ).width(120),
                                    text(t!("Thick")).size(9).style(
                                        |theme: &Theme| iced::widget::text::Style {
                                            color: Some(
                                                theme.palette().background.base.text.scale_alpha(
                                                    0.65
                                                )
                                            ),
                                        }
                                    ).width(40),
                                    text(t!("Function")).size(9).style(
                                        |theme: &Theme| iced::widget::text::Style {
                                            color: Some(
                                                theme.palette().background.base.text.scale_alpha(
                                                    0.65
                                                )
                                            ),
                                        }
                                    ).width(70),
                                ]
                                .spacing(4),
                                scrollable(column(preview_rows).spacing(2)).height(Fill),
                            ]
                            .spacing(5)
                            .width(250);

                            main_content = main_content.push(preview_pane);
                        }
                    }
                }
            }
            main_content
        },
        row![
            Space::new().width(Fill),
            button(text(t!("Cancel")).size(12))
                .style(button::subtle)
                .padding([6, 12])
                .on_press(Message::AecStylePickerCancel),
            button(text(t!("Select")).size(12))
                .style(button::primary)
                .padding([6, 12])
                .on_press(Message::AecStylePickerConfirm),
        ]
        .spacing(10)
    ]
    .spacing(10)
    .padding(15)
    .width(sizing.width)
    .height(sizing.height)
    .into()
}
