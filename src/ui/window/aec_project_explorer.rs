//! AEC Project Explorer — browse a `.ocsproj` Building → Storey tree and
//! open each storey's drawing (`AEC_PROJECTEXPLORER`).

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Element, Fill};

use crate::app::Message;
use crate::modules::aec::engine::project::{Building, ProjectFile, StoreyRef};
use crate::t;
use super::aec_ui_util::*;

/// Transient form buffers owned by `App` and borrowed here for rendering.
pub struct ProjectExplorerState<'a> {
    /// Absolute (or last-known) path of the loaded/saved `.ocsproj`, if any.
    pub path: Option<&'a std::path::Path>,
    /// Currently selected building index (sidebar highlight).
    pub selected_building: Option<usize>,
    /// Currently selected storey as `(building_idx, storey_idx)`.
    pub selected_storey: Option<(usize, usize)>,
    /// "Add building" name buffer.
    pub new_building_name: &'a str,
    /// "Add storey" form buffers.
    pub new_storey_name: &'a str,
    pub new_storey_elevation: &'a str,
    pub new_storey_drawing: &'a str,
}

pub fn view_window<'a>(
    project: Option<&'a ProjectFile>,
    state: ProjectExplorerState<'a>,
) -> Element<'a, Message> {
    let path_label = match state.path {
        Some(p) => p.display().to_string(),
        None => t!("(unsaved project)").into_owned(),
    };

    let toolbar = row![
        button(text(t!("New Project")).size(11))
            .padding([5, 10])
            .on_press(Message::AecProjectExplorerNew),
        button(text(t!("Open…")).size(11))
            .padding([5, 10])
            .on_press(Message::AecProjectExplorerLoad),
        button(text(t!("Save")).size(11))
            .style(button::primary)
            .padding([5, 10])
            .on_press(Message::AecProjectExplorerSave),
        button(text(t!("Save As…")).size(11))
            .padding([5, 10])
            .on_press(Message::AecProjectExplorerSaveAs),
        Space::new().width(Fill),
        text(path_label).size(10).style(muted),
    ]
    .spacing(6)
    .align_y(iced::Center);

    let body = match project {
        Some(project) => project_tree(project, &state),
        None => container(
            text(t!("Open or create a project to browse buildings and storeys.")).style(muted),
        )
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill)
        .into(),
    };

    column![toolbar, body]
        .spacing(10)
        .padding(10)
        .into()
}

fn project_tree<'a>(
    project: &'a ProjectFile,
    state: &ProjectExplorerState<'a>,
) -> Element<'a, Message> {
    let mut tree = column![section_title(t!("Buildings"))].spacing(2);

    if project.buildings.is_empty() {
        tree = tree.push(no_matches());
    } else {
        for (bi, building) in project.buildings.iter().enumerate() {
            tree = tree.push(building_row(bi, building, state));
            for (si, storey) in building.storeys.iter().enumerate() {
                tree = tree.push(storey_row(bi, si, storey, state));
            }
        }
    }

    let sidebar = column![
        scrollable(tree).height(Fill),
        add_building_form(state.new_building_name),
        add_storey_form(state),
    ]
    .spacing(10)
    .width(Fill);

    sidebar.into()
}

fn building_row<'a>(
    bi: usize,
    building: &'a Building,
    state: &ProjectExplorerState<'a>,
) -> Element<'a, Message> {
    // Highlight when this building is the explicit selection (not a storey row).
    let selected = state.selected_building == Some(bi) && state.selected_storey.is_none();

    button(
        row![
            text("▸").size(11).style(muted),
            text(building.name.as_str()).size(12),
            Space::new().width(Fill),
            text(format!("{} storey(s)", building.storeys.len()))
                .size(10)
                .style(muted),
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .on_press(Message::AecProjectExplorerSelectBuilding(bi))
    .style(list_style(selected))
    .padding([6, 9])
    .width(Fill)
    .into()
}

fn storey_row<'a>(
    bi: usize,
    si: usize,
    storey: &'a StoreyRef,
    state: &ProjectExplorerState<'a>,
) -> Element<'a, Message> {
    let selected = state.selected_storey == Some((bi, si));
    let elev = format!("{:.3}", storey.elevation);

    // Row + separate Open button (no nested buttons — iced dislikes that).
    row![
        button(
            row![
                Space::new().width(14),
                column![
                    text(storey.name.as_str()).size(12),
                    text(format!("elev {elev}  ·  {}", storey.drawing_path))
                        .size(10)
                        .style(muted),
                ]
                .spacing(2)
                .width(Fill),
            ]
            .spacing(8)
            .align_y(iced::Center),
        )
        .on_press(Message::AecProjectExplorerSelectStorey(bi, si))
        .style(list_style(selected))
        .padding([6, 9])
        .width(Fill),
        button(text(t!("Open")).size(10))
            .style(button::primary)
            .padding([4, 8])
            .on_press(Message::AecProjectExplorerOpenStorey(bi, si)),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

fn add_building_form<'a>(name: &'a str) -> Element<'a, Message> {
    column![
        section_title(t!("Add Building")),
        row![
            text_input(t!("Building name").as_ref(), name)
                .on_input(Message::AecProjectExplorerNewBuildingNameChanged)
                .size(11)
                .padding([4, 6]),
            button(text(t!("Add")).size(11))
                .padding([4, 10])
                .on_press(Message::AecProjectExplorerAddBuilding),
        ]
        .spacing(6),
    ]
    .spacing(4)
    .into()
}

fn add_storey_form<'a>(state: &ProjectExplorerState<'a>) -> Element<'a, Message> {
    let building_hint = match state.selected_building {
        Some(i) => t!("Adding to building #{n}", n = i + 1).into_owned(),
        None => t!("Select a building first").into_owned(),
    };

    column![
        section_title(t!("Add Storey")),
        text(building_hint).size(10).style(muted),
        row![
            text(t!("Name")).size(10).style(muted).width(70),
            text_input("", state.new_storey_name)
                .on_input(Message::AecProjectExplorerNewStoreyNameChanged)
                .size(11)
                .padding([4, 6]),
        ]
        .spacing(8)
        .align_y(iced::Center),
        row![
            text(t!("Elevation")).size(10).style(muted).width(70),
            text_input("0.0", state.new_storey_elevation)
                .on_input(Message::AecProjectExplorerNewStoreyElevationChanged)
                .size(11)
                .padding([4, 6]),
        ]
        .spacing(8)
        .align_y(iced::Center),
        row![
            text(t!("Drawing")).size(10).style(muted).width(70),
            text_input(t!("path/to/storey.dwg").as_ref(), state.new_storey_drawing)
                .on_input(Message::AecProjectExplorerNewStoreyDrawingChanged)
                .size(11)
                .padding([4, 6]),
            button(text("…").size(11))
                .padding([4, 8])
                .on_press(Message::AecProjectExplorerPickStoreyDrawing),
        ]
        .spacing(6)
        .align_y(iced::Center),
        button(text(t!("Add Storey")).size(11))
            .style(button::primary)
            .padding([5, 12])
            .on_press(Message::AecProjectExplorerAddStorey),
    ]
    .spacing(5)
    .into()
}
