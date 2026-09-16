//! AEC Project Explorer — browse a `.ocsproj` Building → Storey tree and
//! open each storey's drawing (`AEC_PROJECTEXPLORER`).

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Element, Fill};

use crate::app::{AecMessage, AecProjectExplorerDeleteTarget, Message};
use crate::modules::aec::engine::project::{Building, ProjectFile, StoreyRef};
use crate::t;
use crate::tr;
use super::aec_ui_util::*;

/// Transient form buffers owned by `App` and borrowed here for rendering.
pub struct ProjectExplorerState<'a> {
    /// Absolute (or last-known) path of the loaded/saved `.ocsproj`, if any.
    pub path: Option<&'a std::path::Path>,
    /// Currently selected building, by its stable id (sidebar highlight).
    pub selected_building: Option<uuid::Uuid>,
    /// Currently selected storey as `(building_id, storey_id)`.
    pub selected_storey: Option<(uuid::Uuid, uuid::Uuid)>,
    /// "Add building" name buffer.
    pub new_building_name: &'a str,
    /// "Add storey" form buffers.
    pub new_storey_name: &'a str,
    pub new_storey_elevation: &'a str,
    pub new_storey_drawing: &'a str,
    /// Live edit buffer for the name of the currently selected building.
    pub edit_building_name: &'a str,
    /// A delete awaiting confirmation, rendered as an inline Yes/No prompt.
    pub pending_delete: Option<AecProjectExplorerDeleteTarget>,
    pub ffl0_nn: &'a str,
}

pub fn view_window<'a>(
    project: Option<&'a ProjectFile>,
    state: ProjectExplorerState<'a>,
) -> Element<'a, Message> {
    let path_label = match state.path {
        Some(p) => p.display().to_string(),
        None => t!("(unsaved project)").into_owned(),
    };

    let mut migrate_button = button(text(t!("Bibliotheken migrieren")).size(11)).padding([5, 10]);
    if project.is_some() {
        migrate_button = migrate_button.on_press(Message::Aec(AecMessage::AecProjectExplorerMigrateLibraries));
    }

    let toolbar = row![
        button(text(t!("New Project")).size(11))
            .padding([5, 10])
            .on_press(Message::Aec(AecMessage::AecProjectExplorerNew)),
        button(text(t!("Open…")).size(11))
            .padding([5, 10])
            .on_press(Message::Aec(AecMessage::AecProjectExplorerLoad)),
        button(text(t!("Save")).size(11))
            .style(button::primary)
            .padding([5, 10])
            .on_press(Message::Aec(AecMessage::AecProjectExplorerSave)),
        button(text(t!("Save As…")).size(11))
            .padding([5, 10])
            .on_press(Message::Aec(AecMessage::AecProjectExplorerSaveAs)),
        migrate_button,
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

    let nn_row = row![
        text(t!("aec.ffl0-nn")).size(11).style(muted),
        text_input(t!("aec.ffl0-nn-placeholder").as_ref(), state.ffl0_nn)
            .on_input(|v| Message::Aec(AecMessage::AecProjectExplorerFfl0NnChanged(v)))
            .size(11)
            .padding([4, 6])
            .width(120),
    ]
    .spacing(8)
    .align_y(iced::Center);

    let mut content = column![toolbar, nn_row].spacing(10);
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
        AecProjectExplorerDeleteTarget::Building(bid) => {
            let name = project
                .buildings
                .iter()
                .find(|b| b.id == bid)
                .map(|b| b.name.as_str())
                .unwrap_or("?");
            tr!("aec", "delete-building", name = name)
        }
        AecProjectExplorerDeleteTarget::Storey(bid, sid) => {
            let name = project
                .buildings
                .iter()
                .find(|b| b.id == bid)
                .and_then(|b| b.storeys.iter().find(|s| s.id == sid))
                .map(|s| s.name.as_str())
                .unwrap_or("?");
            tr!("aec", "delete-storey", name = name)
        }
    };

    Some(
        container(
            row![
                text(message).size(11),
                Space::new().width(Fill),
                button(text(t!("Cancel")).size(11))
                    .padding([4, 10])
                    .on_press(Message::Aec(AecMessage::AecProjectExplorerCancelDelete)),
                button(text(t!("Delete")).size(11))
                    .style(button::danger)
                    .padding([4, 10])
                    .on_press(Message::Aec(AecMessage::AecProjectExplorerConfirmDelete)),
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
        for building in project.buildings.iter() {
            tree = tree.push(building_row(building, state));
            for storey in building.storeys.iter() {
                tree = tree.push(storey_row(building.id, storey, state));
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
    building: &'a Building,
    state: &ProjectExplorerState<'a>,
) -> Element<'a, Message> {
    let bid = building.id;
    // Highlight when this building is the explicit selection (not a storey row).
    let selected = state.selected_building == Some(bid) && state.selected_storey.is_none();

    let label_row = button(
        row![
            text("▸").size(11).style(muted),
            text(building.name.as_str()).size(12),
            Space::new().width(Fill),
            text(tr!("aec", "storey-count", count = building.storeys.len()))
                .size(10)
                .style(muted),
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .on_press(Message::Aec(AecMessage::AecProjectExplorerSelectBuilding(bid)))
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
                .on_input(move |v| Message::Aec(AecMessage::AecProjectExplorerEditBuildingName(bid, v)))
                .size(11)
                .padding([3, 6])
                .width(Fill),
            button(text(t!("Save")).size(10))
                .style(button::primary)
                .padding([3, 8])
                .on_press(Message::Aec(AecMessage::AecProjectExplorerSaveBuildingEdits(bid))),
            button(text(t!("Delete")).size(10))
                .style(button::danger)
                .padding([3, 8])
                .on_press(Message::Aec(AecMessage::AecProjectExplorerRequestDeleteBuilding(bid))),
        ]
        .spacing(8)
        .align_y(iced::Center),
    ]
    .spacing(3)
    .into()
}

fn storey_row<'a>(
    bid: uuid::Uuid,
    storey: &'a StoreyRef,
    state: &ProjectExplorerState<'a>,
) -> Element<'a, Message> {
    let sid = storey.id;
    let selected = state.selected_storey == Some((bid, sid));
    let elev = format!("{:.3}", storey.derived_elevation());

    let has_drawing = !storey.drawing_path.trim().is_empty();
    let mut open_button = button(text(t!("Open")).size(10))
        .style(button::primary)
        .padding([4, 8]);
    if has_drawing {
        open_button = open_button.on_press(Message::Aec(AecMessage::AecProjectExplorerOpenStorey(bid, sid)));
    }
    let settings_button = button(text(t!("Settings…")).size(10))
        .padding([4, 8])
        .on_press(Message::Aec(AecMessage::AecStoreySettingsOpen(bid, sid)));
    row![
        button(
            row![
                Space::new().width(14),
                column![
                    text(storey.name.as_str()).size(12),
                    text(tr!(
                        "aec",
                        "storey-list-meta",
                        location = elev.as_str(),
                        height = format!("{:.3}", storey.derived_height()),
                        path = storey.drawing_path.as_str()
                    ))
                        .size(10)
                        .style(muted),
                ]
                .spacing(2)
                .width(Fill),
            ]
            .spacing(8)
            .align_y(iced::Center),
        )
        .on_press(Message::Aec(AecMessage::AecProjectExplorerSelectStorey(bid, sid)))
        .style(list_style(selected))
        .padding([6, 9])
        .width(Fill),
        open_button,
        settings_button,
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

fn add_building_form<'a>(name: &'a str) -> Element<'a, Message> {
    column![
        Space::new().height(6),
        section_title(t!("Add Building")),
        row![
            text_input(t!("Building name").as_ref(), name)
                .on_input(|v| Message::Aec(AecMessage::AecProjectExplorerNewBuildingNameChanged(v)))
                .size(11)
                .padding([4, 6]),
            button(text(t!("Add")).size(11))
                .padding([4, 10])
                .on_press(Message::Aec(AecMessage::AecProjectExplorerAddBuilding)),
        ]
        .spacing(6),
    ]
    .spacing(4)
    .into()
}

fn add_storey_form<'a>(project: &'a ProjectFile, state: &ProjectExplorerState<'a>) -> Element<'a, Message> {
    let building_hint = match state
        .selected_building
        .and_then(|bid| project.buildings.iter().find(|b| b.id == bid))
    {
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
                .on_input(|v| Message::Aec(AecMessage::AecProjectExplorerNewStoreyNameChanged(v)))
                .size(11)
                .padding([4, 6]),
        ]
        .spacing(8)
        .align_y(iced::Center),
        row![
            text(t!("aec.storey-location")).size(10).style(muted).width(70),
            text_input("0.0", state.new_storey_elevation)
                .on_input(|v| Message::Aec(AecMessage::AecProjectExplorerNewStoreyElevationChanged(v)))
                .size(11)
                .padding([4, 6]),
        ]
        .spacing(8)
        .align_y(iced::Center),
        row![
            text(t!("Drawing")).size(10).style(muted).width(70),
            text_input(t!("path/to/storey.dwg").as_ref(), state.new_storey_drawing)
                .on_input(|v| Message::Aec(AecMessage::AecProjectExplorerNewStoreyDrawingChanged(v)))
                .size(11)
                .padding([4, 6]),
            button(text("…").size(11))
                .padding([4, 8])
                .on_press(Message::Aec(AecMessage::AecProjectExplorerPickStoreyDrawing)),
        ]
        .spacing(6)
        .align_y(iced::Center),
        button(text(t!("Add Storey")).size(11))
            .style(button::primary)
            .padding([5, 12])
            .on_press(Message::Aec(AecMessage::AecProjectExplorerAddStorey)),
    ]
    .spacing(5)
    .into()
}
