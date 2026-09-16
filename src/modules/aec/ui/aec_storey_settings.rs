//! Storey settings modal: name, drawing path, floor/ceiling roles, control planes.

use std::collections::HashMap;

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Element, Fill};

use crate::app::{AecMessage, Message};
use crate::modules::aec::engine::control_plane::ControlPlane;
use crate::modules::aec::engine::project::StoreyRef;
use crate::t;
use super::aec_ui_util::*;

pub struct StoreySettingsState<'a> {
    pub building_id: uuid::Uuid,
    pub storey: &'a StoreyRef,
    pub new_plane_name: &'a str,
    pub new_plane_z: &'a str,
    pub plane_z: &'a HashMap<uuid::Uuid, String>,
    pub elevation: &'a str,
    pub height: &'a str,
}

pub fn view_window<'a>(state: StoreySettingsState<'a>) -> Element<'a, Message> {
    let bid = state.building_id;
    let sid = state.storey.id;
    let storey = state.storey;

    let header = row![
        text(t!("Storey settings")).size(14),
        Space::new().width(Fill),
        button(text(t!("Close")).size(11))
            .padding([4, 10])
            .on_press(Message::Aec(AecMessage::AecStoreySettingsClose)),
    ]
    .spacing(8)
    .align_y(iced::Center);

    let name_row = row![
        text(t!("Name")).size(11).style(muted).width(80),
        text_input("", storey.name.as_str())
            .on_input(move |v| Message::Aec(AecMessage::AecStoreySettingsNameChanged(bid, sid, v)))
            .size(11)
            .padding([4, 6]),
    ]
    .spacing(8)
    .align_y(iced::Center);

    let drawing_row = row![
        text(t!("Drawing")).size(11).style(muted).width(80),
        text_input("", storey.drawing_path.as_str())
            .on_input(move |v| Message::Aec(AecMessage::AecStoreySettingsDrawingChanged(bid, sid, v)))
            .size(11)
            .padding([4, 6]),
        button(text("…").size(11))
            .padding([4, 8])
            .on_press(Message::Aec(AecMessage::AecProjectExplorerPickEditStoreyDrawing(bid, sid))),
    ]
    .spacing(8)
    .align_y(iced::Center);

    let derived = row![
        text(t!("aec.storey-location")).size(11).style(muted).width(80),
        text_input("", state.elevation)
            .on_input(move |v| Message::Aec(AecMessage::AecStoreySettingsElevation(bid, sid, v)))
            .size(11)
            .padding([4, 6])
            .width(90),
        text(t!("aec.storey-height")).size(11).style(muted).width(80),
        text_input("", state.height)
            .on_input(move |v| Message::Aec(AecMessage::AecStoreySettingsHeight(bid, sid, v)))
            .size(11)
            .padding([4, 6])
            .width(90),
    ]
    .spacing(8)
    .align_y(iced::Center);

    let mut floor_opts =
        row![text(t!("aec.main-control-plane")).size(11).style(muted).width(160)].spacing(6);
    let mut ceil_opts = row![text(t!("Ceiling")).size(11).style(muted).width(80)].spacing(6);
    for p in &storey.control_planes {
        let pid = p.id;
        let floor_sel = storey.floor_plane_id == pid;
        let ceil_sel = storey.ceiling_plane_id == pid;
        floor_opts = floor_opts.push(
            button(text(p.name.as_str()).size(10))
                .style(if floor_sel {
                    button::primary
                } else {
                    button::secondary
                })
                .padding([3, 6])
                .on_press(Message::Aec(AecMessage::AecStoreySettingsSetFloor(bid, sid, pid))),
        );
        ceil_opts = ceil_opts.push(
            button(text(p.name.as_str()).size(10))
                .style(if ceil_sel {
                    button::primary
                } else {
                    button::secondary
                })
                .padding([3, 6])
                .on_press(Message::Aec(AecMessage::AecStoreySettingsSetCeiling(bid, sid, pid))),
        );
    }

    let mut planes = column![
        section_title(t!("Control planes")),
        text(t!("aec.plane-relative-hint")).size(10).style(muted),
    ]
    .spacing(6);
    for p in &storey.control_planes {
        planes = planes.push(plane_row(bid, sid, storey, p, state.plane_z, state.elevation));
    }
    planes = planes.push(
        row![
            text_input(t!("Name").as_ref(), state.new_plane_name)
                .on_input(|v| Message::Aec(AecMessage::AecStoreySettingsNewPlaneNameChanged(v)))
                .size(11)
                .padding([4, 6]),
            text(t!("aec.plane-z-relative")).size(10).style(muted),
            text_input(t!("aec.plane-z-relative-placeholder").as_ref(), state.new_plane_z)
                .on_input(|v| Message::Aec(AecMessage::AecStoreySettingsNewPlaneZChanged(v)))
                .size(11)
                .padding([4, 6])
                .width(90),
            button(text(t!("Add")).size(11))
                .style(button::primary)
                .padding([4, 10])
                .on_press(Message::Aec(AecMessage::AecStoreySettingsAddPlane(bid, sid))),
        ]
        .spacing(6)
        .align_y(iced::Center),
    );

    column![
        header,
        name_row,
        drawing_row,
        derived,
        floor_opts,
        ceil_opts,
        scrollable(planes).height(Fill),
    ]
    .spacing(8)
    .padding(10)
    .into()
}

fn plane_row<'a>(
    bid: uuid::Uuid,
    sid: uuid::Uuid,
    storey: &'a StoreyRef,
    plane: &'a ControlPlane,
    plane_z: &'a HashMap<uuid::Uuid, String>,
    elevation: &'a str,
) -> Element<'a, Message> {
    let pid = plane.id;
    let is_floor = pid == storey.floor_plane_id;
    let is_role = is_floor || pid == storey.ceiling_plane_id;
    let z_buf = plane_z
        .get(&pid)
        .map(|s| s.as_str())
        .unwrap_or("");
    let mut del = button(text(t!("Delete")).size(10))
        .style(button::danger)
        .padding([3, 8]);
    if !is_role {
        del = del.on_press(Message::Aec(AecMessage::AecStoreySettingsDeletePlane(bid, sid, pid)));
    }
    let z_field: Element<'a, Message> = if is_floor {
        row![
            text(t!("aec.main-control-plane")).size(10).style(muted),
            text(t!("aec.plane-follows-storey-location")).size(10).style(muted),
            text(elevation).size(11),
        ]
        .spacing(6)
        .align_y(iced::Center)
        .into()
    } else {
        row![
            text(t!("aec.plane-z-relative")).size(10).style(muted),
            text_input(t!("aec.plane-z-relative-placeholder").as_ref(), z_buf)
                .on_input(move |v| Message::Aec(AecMessage::AecStoreySettingsPlaneZ(bid, sid, pid, v)))
                .size(11)
                .padding([3, 6])
                .width(90),
        ]
        .spacing(6)
        .align_y(iced::Center)
        .into()
    };
    container(
        column![
            row![
                text_input("", plane.name.as_str())
                    .on_input(move |v| Message::Aec(AecMessage::AecStoreySettingsPlaneName(bid, sid, pid, v)))
                    .size(11)
                    .padding([3, 6])
                    .width(Fill),
                z_field,
                iced::widget::checkbox(plane.visible)
                    .label(t!("Visible").into_owned())
                    .on_toggle(move |v| Message::Aec(AecMessage::AecStoreySettingsPlaneVisible(bid, sid, pid, v)))
                    .size(13)
                    .text_size(11),
                del,
            ]
            .spacing(6)
            .align_y(iced::Center),
        ]
        .spacing(4),
    )
    .padding(6)
    .into()
}


