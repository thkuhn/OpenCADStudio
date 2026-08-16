//! AEC Project Explorer — browse a `.ocsproj` Building → Storey tree and
//! open each storey's drawing (`AEC_PROJECTEXPLORER`).

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Element, Fill};

use crate::app::{AecProjectExplorerDeleteTarget, Message};
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
    /// Live edit buffer for the name of the currently selected building.
    pub edit_building_name: &'a str,
    /// Live edit buffer for the name of the currently selected storey.
    pub edit_storey_name: &'a str,
    /// Live text buffer for the elevation field of the currently selected storey.
    pub edit_elevation: &'a str,
    /// Live edit buffer for the drawing path of the currently selected storey.
    pub edit_storey_drawing: &'a str,
    /// A delete awaiting confirmation, rendered as an inline Yes/No prompt.
    pub pending_delete: Option<AecProjectExplorerDeleteTarget>,
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

    let mut content = column![toolbar].spacing(10);
    if let Some(bar) = delete_confirm_bar(project, state.pending_delete) {
        content = content.push(bar);
    }
    content = content.push(body);

    content.padding(10).into()
}

/// Inline "are you sure?" prompt shown instead of deleting immediately —
/// removing a building/storey can affect the whole project and its files.
fn delete_confirm_bar<'a>(
    project: Option<&'a ProjectFile>,
    pending: Option<AecProjectExplorerDeleteTarget>,
) -> Option<Element<'a, Message>> {
    let project = project?;
    let pending = pending?;

    let message = match pending {
        AecProjectExplorerDeleteTarget::Building(bi) => {
            let name = project
                .buildings
                .get(bi)
                .map(|b| b.name.as_str())
                .unwrap_or("?");
            t!(
                "Delete building \"%{name}\" and all its storeys? This cannot be undone.",
                name = name
            )
            .into_owned()
        }
        AecProjectExplorerDeleteTarget::Storey(bi, si) => {
            let name = project
                .buildings
                .get(bi)
                .and_then(|b| b.storeys.get(si))
                .map(|s| s.name.as_str())
                .unwrap_or("?");
            t!(
                "Delete storey \"%{name}\"? This cannot be undone.",
                name = name
            )
            .into_owned()
        }
    };

    Some(
        container(
            row![
                text(message).size(11),
                Space::new().width(Fill),
                button(text(t!("Cancel")).size(11))
                    .padding([4, 10])
                    .on_press(Message::AecProjectExplorerCancelDelete),
                button(text(t!("Delete")).size(11))
                    .style(button::danger)
                    .padding([4, 10])
                    .on_press(Message::AecProjectExplorerConfirmDelete),
            ]
            .spacing(8)
            .align_y(iced::Center),
        )
        .padding(8)
        .width(Fill)
        .into(),
    )
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
        add_storey_form(project, state),
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

    let label_row = button(
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
    .width(Fill);

    if !selected {
        return label_row.into();
    }

    // Selected: show an inline edit field below the row (avoids nesting a
    // text_input inside a button, which iced doesn't support well). Changes
    // are only applied/persisted when "Speichern" is pressed.
    column![
        label_row,
        row![
            Space::new().width(20),
            text(t!("Name")).size(10).style(muted),
            text_input("", state.edit_building_name)
                .on_input(move |v| Message::AecProjectExplorerEditBuildingName(bi, v))
                .size(11)
                .padding([3, 6])
                .width(Fill),
            button(text(t!("Save")).size(10))
                .style(button::primary)
                .padding([3, 8])
                .on_press(Message::AecProjectExplorerSaveBuildingEdits(bi)),
            button(text(t!("Delete")).size(10))
                .style(button::danger)
                .padding([3, 8])
                .on_press(Message::AecProjectExplorerRequestDeleteBuilding(bi)),
        ]
        .spacing(8)
        .align_y(iced::Center),
    ]
    .spacing(3)
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
    let label_row = row![
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
    .align_y(iced::Center);

    if !selected {
        return label_row.into();
    }

    // Selected: show inline fields to edit name, elevation and drawing path.
    // Nothing is applied/persisted until "Speichern" is pressed.
    column![
        label_row,
        row![
            Space::new().width(28),
            text(t!("Name")).size(10).style(muted).width(60),
            text_input("", state.edit_storey_name)
                .on_input(move |v| Message::AecProjectExplorerEditStoreyName(bi, si, v))
                .size(11)
                .padding([3, 6])
                .width(Fill),
            button(text(t!("Delete")).size(10))
                .style(button::danger)
                .padding([3, 8])
                .on_press(Message::AecProjectExplorerRequestDeleteStorey(bi, si)),
        ]
        .spacing(8)
        .align_y(iced::Center),
        row![
            Space::new().width(28),
            text(t!("Elevation")).size(10).style(muted).width(60),
            text_input("0.0", state.edit_elevation)
                .on_input(move |v| Message::AecProjectExplorerEditStoreyElevation(bi, si, v))
                .size(11)
                .padding([3, 6])
                .width(Fill),
        ]
        .spacing(8)
        .align_y(iced::Center),
        row![
            Space::new().width(28),
            text(t!("Drawing")).size(10).style(muted).width(60),
            text_input("", state.edit_storey_drawing)
                .on_input(move |v| Message::AecProjectExplorerEditStoreyDrawing(bi, si, v))
                .size(11)
                .padding([3, 6])
                .width(Fill),
        ]
        .spacing(8)
        .align_y(iced::Center),
        row![
            Space::new().width(28),
            Space::new().width(60),
            button(text(t!("Save")).size(10))
                .style(button::primary)
                .padding([3, 10])
                .on_press(Message::AecProjectExplorerSaveStoreyEdits(bi, si)),
        ]
        .spacing(8)
        .align_y(iced::Center),
    ]
    .spacing(3)
    .into()
}

fn add_building_form<'a>(name: &'a str) -> Element<'a, Message> {
    column![
        Space::new().height(6),
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

fn add_storey_form<'a>(project: &'a ProjectFile, state: &ProjectExplorerState<'a>) -> Element<'a, Message> {
    let building_hint = match state.selected_building.and_then(|i| project.buildings.get(i)) {
        Some(building) => {
            t!("Adding to building \"%{name}\"", name = building.name.as_str()).into_owned()
        }
        None => t!("Select a building first").into_owned(),
    };

    column![
        Space::new().height(6),
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
