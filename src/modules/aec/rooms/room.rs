//! `AEC_ROOM` one-shot command.

use acadrust::entities::{LwPolyline, LwVertex};
use acadrust::types::Vector2;
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::EntityType;

use crate::modules::aec::commands::{collect_wall_segments, write_aec_record, AEC_APPID};
use crate::modules::aec::engine::{find_closed_loop, Room};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_ROOM",
        label: "Room",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/array_rect.svg")),
        event: ModuleEvent::Command("AEC_ROOM".to_string()),
    }
}

/// `AEC_ROOM` — detect a closed wall loop (or demo rectangle) + ROOM XDATA.
pub fn aec_room(scene: &mut Scene, command_line: &mut CommandLine) {
    let wall_segments = collect_wall_segments(&scene.document);
    let (pts, detected) = match find_closed_loop(&wall_segments, 1e-3) {
        Some(loop_pts) => (loop_pts, true),
        None => (
            vec![(0.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 3.0)],
            false,
        ),
    };

    let room = Room::from_polygon("Office 101", &pts, 2.8, 0);

    let mut pl = LwPolyline::new();
    pl.is_closed = true;
    for (x, y) in &pts {
        pl.add_vertex(LwVertex::new(Vector2::new(*x, *y)));
    }

    let handle = scene.add_entity(EntityType::LwPolyline(pl));

    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("ROOM".to_string()));
    record.add_value(XDataValue::String(room.name.clone()));
    record.add_value(XDataValue::Real(room.area));
    record.add_value(XDataValue::Real(room.perimeter));
    record.add_value(XDataValue::Real(room.volume));
    record.add_value(XDataValue::Integer32(room.storey_id as i32));

    write_aec_record(&mut scene.document, handle, record);
    scene.bump_geometry();
    if detected {
        command_line.push_info(&crate::tr!(
            "aec",
            "room-detected",
            name = room.name.as_str(),
            count = pts.len(),
            handle = handle.to_string()
        ));
    } else {
        command_line.push_info(&crate::tr!(
            "aec",
            "room-demo",
            name = room.name.as_str(),
            handle = handle.to_string()
        ));
    }
}


inventory::submit!(crate::command::CommandRegistration { names: &["AEC_ROOM"] });
