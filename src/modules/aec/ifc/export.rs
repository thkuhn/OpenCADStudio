//! `AEC_IFCEXPORT` one-shot command.

use crate::modules::aec::commands::{read_aec_record, wall_from_entity, STOREYS};
use crate::modules::aec::engine::{self, Room};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;
use acadrust::xdata::XDataValue;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_IFCEXPORT",
        label: "Export IFC",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/cui_export.svg")),
        event: ModuleEvent::Command("AEC_IFCEXPORT".to_string()),
    }
}

/// `AEC_IFCEXPORT` — collect walls/rooms/storeys and emit IFC4 SPF (in-memory).
pub fn aec_ifc_export(scene: &mut Scene, command_line: &mut CommandLine) {
    let mut ifc_scene = engine::Scene::default();

    // Add in-memory storeys
    {
        let storeys = STOREYS.lock().unwrap();
        for (i, s) in storeys.iter().enumerate() {
            ifc_scene.storeys.push((i as u32, s.clone()));
        }
    }

    // Collect walls and rooms from document XDATA
    for entity in scene.document.entities() {
        let Some(record) = read_aec_record(entity) else {
            continue;
        };
        match record.values.first() {
            Some(XDataValue::String(kind)) if kind == "WALL" => {
                if let Some(wall) = wall_from_entity(entity) {
                    ifc_scene.walls.push(wall);
                }
            }
            Some(XDataValue::String(kind)) if kind == "ROOM" => {
                // 0: "ROOM", 1: name, 2: area, 3: perim, 4: vol, 5: storey_id
                if record.values.len() >= 6 {
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
                    let perimeter = if let XDataValue::Real(r) = record.values[3] {
                        r
                    } else {
                        0.0
                    };
                    let volume = if let XDataValue::Real(r) = record.values[4] {
                        r
                    } else {
                        0.0
                    };
                    let storey_id = if let XDataValue::Integer32(i) = record.values[5] {
                        i as u32
                    } else {
                        0
                    };
                    ifc_scene.rooms.push(Room {
                        name,
                        area,
                        perimeter,
                        volume,
                        storey_id,
                    });
                }
            }
            _ => {}
        }
    }

    let ifc_data = engine::ifc::write_spf(&ifc_scene);
    command_line.push_info(&crate::tr!(
        "aec",
        "ifc-exported",
        bytes = ifc_data.len()
    ));
    command_line.push_info(&crate::tr!("aec", "ifc-note"));
}


inventory::submit!(crate::command::CommandRegistration { names: &["AEC_IFCEXPORT"] });
