//! Storey settings modal: name, drawing path, floor/ceiling roles, control planes.

use std::collections::HashMap;

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Element, Fill};

use crate::app::{AecMessage, Message};
use crate::modules::aec::engine::control_plane::ControlPlane;
use crate::modules::aec::engine::project::StoreyRef;
use crate::t;
use super::aec_ui_util::*;

static SHOW_ICON: &[u8] = include_bytes!("../../../../assets/icons/constrain/show.svg");

pub struct StoreySettingsState<'a> {
    pub building_id: uuid::Uuid,
    pub storey: &'a StoreyRef,
    pub new_plane_name: &'a str,
    pub new_plane_z: &'a str,
    pub plane_z: &'a HashMap<uuid::Uuid, String>,
    pub facet_z: &'a HashMap<(uuid::Uuid, usize), String>,
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
        planes = planes.push(plane_row(bid, sid, storey, p, state.plane_z, state.facet_z, state.elevation));
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

    let footer = row![
        Space::new().width(Fill),
        button(text(t!("Übernehmen")).size(11))
            .style(button::primary)
            .padding([5, 14])
            .on_press(Message::Aec(AecMessage::AecStoreySettingsSaveAndApply(bid, sid))),
        button(text(t!("Speichern")).size(11))
            .style(button::subtle)
            .padding([5, 14])
            .on_press(Message::Aec(AecMessage::AecStoreySettingsSave(bid, sid))),
        button(text(t!("Schließen")).size(11))
            .padding([5, 14])
            .on_press(Message::Aec(AecMessage::AecStoreySettingsClose)),
    ]
    .spacing(8)
    .align_y(iced::Center);

    column![
        header,
        name_row,
        drawing_row,
        derived,
        floor_opts,
        ceil_opts,
        scrollable(planes).height(Fill),
        footer,
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
    facet_z: &'a HashMap<(uuid::Uuid, usize), String>,
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
    } else if !plane.facets.is_empty() {
        let label = if plane.is_sloped() {
            format!("{} Polygone ({:.1}°)", plane.facets.len(), plane.slope_degrees())
        } else {
            format!("{} Polygone", plane.facets.len())
        };
        row![
            text(label).size(10).style(muted),
            text_input(t!("aec.plane-z-relative-placeholder").as_ref(), z_buf)
                .on_input(move |v| Message::Aec(AecMessage::AecStoreySettingsPlaneZ(bid, sid, pid, v)))
                .size(11)
                .padding([3, 6])
                .width(70),
        ]
        .spacing(4)
        .align_y(iced::Center)
        .into()
    } else if plane.is_sloped() {
        row![
            text(t!("aec.plane-z-relative")).size(10).style(muted),
            text_input(t!("aec.plane-z-relative-placeholder").as_ref(), z_buf)
                .on_input(move |v| Message::Aec(AecMessage::AecStoreySettingsPlaneZ(bid, sid, pid, v)))
                .size(11)
                .padding([3, 6])
                .width(70),
            text(format!("{:.1}°", plane.slope_degrees())).size(10).style(muted),
        ]
        .spacing(4)
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
    let assign_poly = button(text(t!("aec.assign-polygons-btn")).size(10))
        .style(button::secondary)
        .padding([3, 6])
        .on_press(Message::Aec(AecMessage::AecStoreySettingsPickPolygons(bid, sid, pid)));
    let show_btn = button(crate::ui::icons::semantic(SHOW_ICON, 13.0))
        .style(button::secondary)
        .padding([3, 5])
        .on_press(Message::Aec(AecMessage::AecStoreySettingsShowPlaneInDrawing(bid, sid, pid)));
    let show_in_drawing = iced::widget::tooltip(
        show_btn,
        container(text(t!("aec.show-in-drawing-btn")).size(10))
            .padding([3, 6])
            .style(container::bordered_box),
        iced::widget::tooltip::Position::Top,
    );

    let main_row = row![
        text_input("", plane.name.as_str())
            .on_input(move |v| Message::Aec(AecMessage::AecStoreySettingsPlaneName(bid, sid, pid, v)))
            .size(11)
            .padding([3, 6])
            .width(Fill),
        z_field,
        assign_poly,
        show_in_drawing,
        iced::widget::checkbox(plane.visible)
            .label(t!("Visible").into_owned())
            .on_toggle(move |v| Message::Aec(AecMessage::AecStoreySettingsPlaneVisible(bid, sid, pid, v)))
            .size(13)
            .text_size(11),
        del,
    ]
    .spacing(6)
    .align_y(iced::Center);

    let mut col = column![main_row].spacing(4);

    for (f_idx, facet) in plane.facets.iter().enumerate() {
        let f_z_buf = facet_z
            .get(&(pid, f_idx))
            .map(|s| s.as_str())
            .unwrap_or("");
        let facet_slope = if facet.slope_degrees() > 0.05 {
            format!("{:.1}°", facet.slope_degrees())
        } else {
            "0.0°".to_string()
        };
        let facet_pts_label = format!("{} Pkt.", facet.vertices.len());
        let facet_del = button(text(t!("Delete")).size(9))
            .style(button::danger)
            .padding([2, 6])
            .on_press(Message::Aec(AecMessage::AecStoreySettingsDeleteFacet(bid, sid, pid, f_idx)));
        let facet_show_btn = button(crate::ui::icons::semantic(SHOW_ICON, 11.0))
            .style(button::secondary)
            .padding([2, 4])
            .on_press(Message::Aec(AecMessage::AecStoreySettingsShowFacetInDrawing(bid, sid, pid, f_idx)));
        let facet_show = iced::widget::tooltip(
            facet_show_btn,
            container(text(t!("aec.show-in-drawing-btn")).size(10))
                .padding([2, 5])
                .style(container::bordered_box),
            iced::widget::tooltip::Position::Top,
        );

        let facet_name_input = text_input("", facet.name.as_str())
            .on_input(move |v| Message::Aec(AecMessage::AecStoreySettingsFacetName(bid, sid, pid, f_idx, v)))
            .size(10)
            .padding([2, 4])
            .width(130);

        let facet_z_input = text_input(t!("aec.plane-z-relative-placeholder").as_ref(), f_z_buf)
            .on_input(move |v| Message::Aec(AecMessage::AecStoreySettingsFacetZ(bid, sid, pid, f_idx, v)))
            .size(10)
            .padding([2, 4])
            .width(60);

        let f_row = row![
            Space::new().width(16),
            text("↳").size(11).style(muted),
            text(format!("#{}", f_idx + 1)).size(10).style(muted).width(20),
            facet_name_input,
            text(facet_pts_label).size(10).style(muted).width(45),
            text(t!("aec.plane-z-relative")).size(10).style(muted),
            facet_z_input,
            text(facet_slope).size(10).style(muted).width(35),
            facet_show,
            facet_del,
        ]
        .spacing(6)
        .align_y(iced::Center);

        col = col.push(f_row);
    }

    container(col).padding(6).into()
}


