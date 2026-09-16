//! `AEC_ROOMSCHEDULE` one-shot command.

use acadrust::entities::Table;
use acadrust::types::Vector3;
use acadrust::xdata::XDataValue;
use acadrust::EntityType;

use crate::modules::aec::commands::read_aec_record;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_ROOMSCHEDULE",
        label: "Schedule",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/table.svg")),
        event: ModuleEvent::Command("AEC_ROOMSCHEDULE".to_string()),
    }
}

/// `AEC_ROOMSCHEDULE` — scan ROOM XDATA and build a real TABLE entity.
pub fn aec_room_schedule(scene: &mut Scene, command_line: &mut CommandLine) {
    let mut rooms = Vec::new();
    for entity in scene.document.entities() {
        let Some(record) = read_aec_record(entity) else {
            continue;
        };
        if let Some(XDataValue::String(kind)) = record.values.first() {
            if kind == "ROOM" && record.values.len() >= 6 {
                // 0: "ROOM", 1: name, 2: area, 3: perim, 4: vol, 5: storey_id
                let name = if let XDataValue::String(s) = &record.values[1] {
                    s.clone()
                } else {
                    "Unknown".to_string()
                };
                let area = if let XDataValue::Real(r) = record.values[2] {
                    r
                } else {
                    0.0
                };
                let storey_id = if let XDataValue::Integer32(i) = record.values[5] {
                    i
                } else {
                    0
                };
                rooms.push((name, area, storey_id));
            }
        }
    }

    if rooms.is_empty() {
        command_line.push_info(&crate::tr!("aec", "no-rooms"));
        return;
    }

    // Real TABLE entity: header row + one row per room.
    let row_count = rooms.len() + 1;
    let mut table = Table::new(Vector3::ZERO, row_count, 3);
    let hdr_name = crate::tr!("aec", "schedule-name");
    let hdr_area = crate::tr!("aec", "schedule-area");
    let hdr_storey = crate::tr!("aec", "schedule-storey-id");
    table.set_cell_text(0, 0, &hdr_name);
    table.set_cell_text(0, 1, &hdr_area);
    table.set_cell_text(0, 2, &hdr_storey);
    for (row, (name, area, storey)) in rooms.iter().enumerate() {
        table.set_cell_text(row + 1, 0, name);
        table.set_cell_text(row + 1, 1, &format!("{area:.2}"));
        table.set_cell_text(row + 1, 2, &storey.to_string());
    }

    let handle = scene.add_entity(EntityType::Table(table));
    scene.bump_geometry();
    command_line.push_info(&crate::tr!(
        "aec",
        "schedule-created",
        count = rooms.len(),
        handle = handle.to_string()
    ));
}


inventory::submit!(crate::command::CommandRegistration { names: &["AEC_ROOMSCHEDULE"] });
