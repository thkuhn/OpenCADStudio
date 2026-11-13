use super::*;
use iced::widget::{column, row};

pub(super) fn divisions_tab<'a>(form: &OpeningStyleFormState<'a>) -> Element<'a, Message> {
    let style = form_preview_style(form);
    let width = crate::modules::aec::styles::opening_style_manager::parse_dim(form.width)
        .unwrap_or(style.default_width);
    let height = crate::modules::aec::styles::opening_style_manager::parse_dim(form.height)
        .unwrap_or(style.default_height);
    let selected_index = form.selected_cell.unwrap_or(0);
    let root = form.authored_grid.cloned().unwrap_or_else(|| style.grid.clone());
    let (level, level_width, level_height) =
        crate::modules::aec::engine::construction_grid::grid_at_path(
            &root,
            width,
            height,
            form.grid_path,
        )
        .unwrap_or((&root, width, height));
    let level = level.clone();
    let resolved = crate::modules::aec::engine::construction_grid::resolve_grid(
        &level,
        level_width,
        level_height,
    );
    let selected_resolved = resolved.cells.get(selected_index);
    let selected_kind = selected_resolved
        .map(|cell| fill_to_kind(&cell.cell.fill))
        .unwrap_or(OpeningCellKind::Glass);
    let selected_hinge = selected_resolved.and_then(|cell| cell.cell.operation);
    let kind_options: Vec<String> = OpeningCellKind::catalogue()
        .iter()
        .map(|kind| kind.as_str().to_string())
        .collect();
    let kind_selector = pick_list(
        Some(selected_kind.as_str().to_string()),
        kind_options,
        |value: &String| value.clone(),
    )
    .on_select(move |value| {
        Message::Aec(AecMessage::AecOpeningStyleManagerCellKindChanged(
            selected_index,
            value,
        ))
    })
    .text_size(11)
    .width(130);
    let edge_properties: Element<'a, Message> = if let Some((vertical, index)) = form.selected_grid_edge {
        use crate::modules::aec::engine::construction_grid::{
            format_track_measure, TrackEnd, TrackMeeting,
        };
        let axis = if vertical { "vertical" } else { "horizontal" };
        let tracks = if vertical { &level.columns } else { &level.rows };
        let resolved_tracks = if vertical { &resolved.columns } else { &resolved.rows };
        if let (Some(track), Some(resolved_track)) =
            (tracks.get(index), resolved_tracks.get(index))
        {
            let edge_label = tr!("aec", "opening-grid-edge");
            let selected_meeting = match track.meeting {
                TrackMeeting::Mullion => "mullion",
                TrackMeeting::Stulp {
                    bearer: TrackEnd::Start,
                } => "stulp-left",
                TrackMeeting::Stulp {
                    bearer: TrackEnd::End,
                } => "stulp-right",
                TrackMeeting::Flush => "flush",
            };
            let meeting_options = vec![
                ("mullion".to_string(), tr!("aec", "opening-grid-mullion")),
                ("stulp-left".to_string(), tr!("aec", "opening-grid-stulp-left")),
                ("stulp-right".to_string(), tr!("aec", "opening-grid-stulp-right")),
                ("flush".to_string(), tr!("aec", "opening-grid-no-bar")),
            ];
            let meeting_value = meeting_options
                .iter()
                .find(|(value, _)| value == selected_meeting)
                .cloned();
            let measure = form
                .grid_measure
                .map(str::to_string)
                .unwrap_or_else(|| format_track_measure(track.measure));
            column![
                text(edge_label).size(10).style(muted),
                row![
                    text(tr!("aec", "opening-grid-edge-kind")).size(10).width(78),
                    pick_list(meeting_value, meeting_options, |item: &(String, String)| item.1.clone())
                        .on_select(move |(value, _)| {
                            Message::Aec(AecMessage::AecOpeningStyleManagerGridMeeting(
                                axis.to_string(), index, value,
                            ))
                        })
                        .text_size(11)
                        .width(150),
                ]
                .spacing(4)
                .align_y(iced::Center),
                row![
                    text(tr!("aec", "opening-grid-measure")).size(10).width(78),
                    text_input("", &measure)
                        .on_input(move |value| {
                            Message::Aec(AecMessage::AecOpeningStyleManagerGridMeasure(
                                axis.to_string(), index, value,
                            ))
                        })
                        .size(11)
                        .padding([3, 5])
                        .width(100),
                    text(tr!("aec", "opening-grid-measure-example")).size(9),
                    text(format!(
                        "{} m",
                        crate::modules::aec::styles::opening_style_manager::format_dim(
                            resolved_track.end - resolved_track.start
                        )
                    ))
                    .size(10),
                ]
                .spacing(4)
                .align_y(iced::Center),
                button(text("\u{00d7}").size(12))
                    .padding([2, 8])
                    .on_press(Message::Aec(
                        AecMessage::AecOpeningStyleManagerDivisionRemove(axis.to_string(), index),
                    )),
            ]
            .spacing(5)
            .into()
        } else {
            text(tr!("aec", "opening-grid-select-edge")).size(10).style(muted).into()
        }
    } else if let Some(cell) = selected_resolved {
        let hinge_options = vec![
            ("".to_string(), tr!("aec", "opening-cell-style-default")),
            ("Left".to_string(), t!("Left").to_string()),
            ("Right".to_string(), t!("Right").to_string()),
        ];
        let hinge_value = hinge_options
            .iter()
            .find(|(value, _)| value == selected_hinge.map_or("", LeafOperation::as_str))
            .cloned();
        let hinge_selector: Element<'a, Message> = if selected_kind.permits_hinge() {
            pick_list(hinge_value, hinge_options, |item: &(String, String)| item.1.clone())
                .on_select(move |(value, _)| {
                    Message::Aec(AecMessage::AecOpeningStyleManagerCellHingeChanged(
                        selected_index,
                        value,
                    ))
                })
                .text_size(11)
                .width(130)
                .into()
        } else {
            text(tr!("aec", "opening-cell-hinge-not-applicable"))
                .size(11)
                .style(muted)
                .into()
        };
        let swing_options = vec![
            ("".to_string(), tr!("aec", "opening-cell-style-default")),
            (
                SwingSide::Exterior.as_str().to_string(),
                tr!("aec", "opening-swing-exterior"),
            ),
            (
                SwingSide::Interior.as_str().to_string(),
                tr!("aec", "opening-swing-interior"),
            ),
        ];
        let swing_value = swing_options
            .iter()
            .find(|(value, _)| {
                value == cell.cell.swing.map_or("", SwingSide::as_str)
            })
            .cloned();
        let swing_selector = pick_list(swing_value, swing_options, |item: &(String, String)| item.1.clone())
            .on_select(move |(value, _)| {
                Message::Aec(AecMessage::AecOpeningStyleManagerCellSwingChanged(
                    selected_index,
                    value,
                ))
            })
            .text_size(11)
            .width(130);
        let angle_default = tr!("aec", "opening-cell-style-default");
        let angle_value = cell.cell.angle_deg.map(format_dim).unwrap_or_default();
        column![
            text(tr!("aec", "opening-selected-cell")).size(10).style(muted),
            row![text(opening_label("opening-cell-kind")).size(10).width(55), kind_selector]
                .spacing(4)
                .align_y(iced::Center),
            row![text(opening_label("opening-cell-hinge")).size(10).width(55), hinge_selector]
                .spacing(4)
                .align_y(iced::Center),
            row![text(tr!("aec", "opening-cell-swing")).size(10).width(55), swing_selector]
                .spacing(4)
                .align_y(iced::Center),
            row![
                text(tr!("aec", "opening-cell-angle")).size(10).width(55),
                text_input(&angle_default, &angle_value)
                .on_input(move |value| {
                    Message::Aec(AecMessage::AecOpeningStyleManagerCellAngleChanged(
                        selected_index,
                        value,
                    ))
                })
                .size(11)
                .padding([3, 5])
                .width(130),
            ]
            .spacing(4)
            .align_y(iced::Center),
            {
                let span = |label: &'static str, action: &'static str| {
                    button(text(label).size(12)).padding([2, 8]).on_press(Message::Aec(
                        AecMessage::AecOpeningStyleManagerCellSpan(action.to_string()),
                    ))
                };
                row![
                    span("\u{25c0}", "join-left"),
                    span("\u{25b6}", "join-right"),
                    span("\u{25b2}", "join-up"),
                    span("\u{25bc}", "join-down"),
                    span("\u{229f}", "split"),
                    span("\u{229e}", "nest"),
                ]
                .spacing(4)
            },
        ]
        .spacing(5)
        .width(FillLen)
        .into()
    } else {
        text(tr!("aec", "opening-grid-select-cell")).size(10).style(muted).into()
    };
    let mut grid_heading = row![text(opening_label("opening-grid")).size(11).style(muted), Space::new()];
    let path_label = form
        .grid_path
        .iter()
        .map(|(column, row)| {
            let column = char::from(b'A' + (*column as u8).min(25));
            format!("{column}{}", row + 1)
        })
        .collect::<Vec<_>>()
        .join(" > ");
    if !path_label.is_empty() {
        grid_heading = grid_heading.push(text(path_label).size(10).style(muted));
    }
    if !form.grid_path.is_empty() {
        grid_heading = grid_heading.push(
            button(text(tr!("aec", "opening-grid-up")).size(10))
                .padding([3, 8])
                .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerGridBack)),
        );
    }
    let style_hinge = pick_list(
        Some(form.hinge.as_str().to_string()),
        vec!["Left".to_string(), "Right".to_string()],
        |value: &String| value.clone(),
    )
    .on_select(|value| {
        Message::Aec(AecMessage::AecOpeningStyleManagerHingeChanged(value))
    })
    .text_size(10)
    .width(100);
    let style_swing = pick_list(
        Some(form.swing_side.as_str().to_string()),
        vec![
            SwingSide::Exterior.as_str().to_string(),
            SwingSide::Interior.as_str().to_string(),
        ],
        |value: &String| match value.as_str() {
            "Interior" => tr!("aec", "opening-swing-interior"),
            _ => tr!("aec", "opening-swing-exterior"),
        },
    )
    .on_select(|value| Message::Aec(AecMessage::AecOpeningStyleManagerSwingChanged(value)))
    .text_size(10)
    .width(120);
    let leaf_defaults = column![
        text(t!("Default")).size(10).style(muted),
        row![text(opening_label("opening-cell-hinge")).size(10).width(90), style_hinge]
            .spacing(5)
            .align_y(iced::Center),
        row![text(tr!("aec", "opening-cell-swing")).size(10).width(90), style_swing]
            .spacing(5)
            .align_y(iced::Center),
    ]
    .spacing(5);
    let raster_content = column![
        grid_heading,
        row![
            container(
                crate::modules::aec::ui::style_manager::forms::opening_division_sketch::view(
                    &level,
                    level_width,
                    level_height,
                    style.kind,
                    form.selected_cell,
                    form.selected_grid_edge,
                    form.hinge,
                ),
            )
            .width(iced::Length::FillPortion(3))
            .height(FillLen),
            column![
                leaf_defaults,
                edge_properties,
                text(tr!("aec", "opening-division-sketch-hint")).size(10).style(muted),
            ]
            .spacing(8)
            .width(iced::Length::FillPortion(2)),
        ]
        .spacing(12)
        .height(FillLen),
    ]
    .spacing(6)
    .height(FillLen);
    container(raster_content)
        .padding(8)
        .width(FillLen)
        .height(248)
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
        })
        .into()
}

pub(super) fn fill_to_kind(
    fill: &crate::modules::aec::engine::construction_grid::CellFill,
) -> OpeningCellKind {
    use crate::modules::aec::engine::construction_grid::CellFill;
    match fill {
        CellFill::Glass | CellFill::Nested(_) => OpeningCellKind::Glass,
        CellFill::Opaque => OpeningCellKind::Opaque,
        CellFill::Door => OpeningCellKind::DoorLeaf,
        CellFill::Fixed => OpeningCellKind::Fixed,
    }
}

pub(super) fn meter_list_editor<'a>(
    label: &'static str,
    axis: &'static str,
    field_prefix: &'static str,
    values: &'a [String],
) -> Element<'a, Message> {
    let mut list = column![text(opening_label(label)).size(10).style(muted)].spacing(2);
    for (index, value) in values.iter().enumerate() {
        let field = format!("{field_prefix}:{index}");
        let remove_axis = axis.to_string();
        list = list.push(
            row![
                text_input("", value)
                    .on_input(move |value| {
                        Message::Aec(AecMessage::AecOpeningStyleManagerDimChanged(
                            field.clone(),
                            value,
                        ))
                    })
                    .size(11)
                    .padding([2, 5]),
                text("m").size(10),
                button(text("×").size(11))
                    .padding([2, 5])
                    .on_press(Message::Aec(
                        AecMessage::AecOpeningStyleManagerDivisionRemove(
                            remove_axis.clone(),
                            index,
                        ),
                    )),
            ]
            .spacing(4)
            .align_y(iced::Center),
        );
    }
    list.push(
        button(text("+").size(11))
            .padding([3, 7])
            .on_press(Message::Aec(AecMessage::AecOpeningStyleManagerDivisionAdd(
                axis.to_string(),
            ))),
    )
    .into()
}

pub(super) fn symbol_tab<'a>(form: &OpeningStyleFormState<'a>) -> Element<'a, Message> {
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

