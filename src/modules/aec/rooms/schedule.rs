//! `AEC_ROOMSCHEDULE` command for generating DIN 277 / WoFlV room schedules and area takeoffs as CAD tables.

use acadrust::entities::Table;
use acadrust::types::Vector3;
use acadrust::EntityType;

use crate::modules::aec::engine::room::Room;
use crate::modules::aec::engine::room_xdata::room_from_entity;
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

/// `AEC_ROOMSCHEDULE` — scans all Room entities and builds a complete DIN 277 / WoFlV room schedule table.
pub fn aec_room_schedule(scene: &mut Scene, command_line: &mut CommandLine) {
    let mut rooms: Vec<Room> = Vec::new();
    for entity in scene.document.entities() {
        if let Some(room) = room_from_entity(entity) {
            rooms.push(room);
        }
    }

    if rooms.is_empty() {
        command_line.push_info(&crate::tr!("aec", "no-rooms"));
        return;
    }

    // Sort rooms by Storey ID, then Room Number, then Name
    rooms.sort_by(|a, b| {
        (a.storey_id, &a.number, &a.name).cmp(&(b.storey_id, &b.number, &b.name))
    });

    let mut total_gross_area = 0.0;
    let mut total_calc_area = 0.0;
    let mut total_volume = 0.0;

    for r in &rooms {
        total_gross_area += r.area;
        total_calc_area += r.calculated_area();
        total_volume += r.effective_volume();
    }

    // Table structure:
    // Header row (0)
    // Room rows (1..=rooms.len())
    // Summary / Total row (rooms.len() + 1)
    let col_count = 11;
    let row_count = rooms.len() + 2;
    let mut table = Table::new(Vector3::ZERO, row_count, col_count);

    // Headers
    table.set_cell_text(0, 0, "Nr.");
    table.set_cell_text(0, 1, "Raumbezeichnung");
    table.set_cell_text(0, 2, "Kategorie (DIN 277)");
    table.set_cell_text(0, 3, "Rohfläche [m²]");
    table.set_cell_text(0, 4, "Faktor");
    table.set_cell_text(0, 5, "Fläche [m²]");
    table.set_cell_text(0, 6, "Umfang [m]");
    table.set_cell_text(0, 7, "Lichte Höhe [m]");
    table.set_cell_text(0, 8, "Volumen [m³]");
    table.set_cell_text(0, 9, "Bodenbelag");
    table.set_cell_text(0, 10, "Geschoss");

    for (i, r) in rooms.iter().enumerate() {
        let row = i + 1;
        table.set_cell_text(row, 0, &r.number);
        table.set_cell_text(row, 1, &r.name);
        table.set_cell_text(row, 2, r.function.din277_code());
        table.set_cell_text(row, 3, &format!("{:.2}", r.area));
        table.set_cell_text(row, 4, &format!("{:.2}", r.factor));
        table.set_cell_text(row, 5, &format!("{:.2}", r.calculated_area()));
        table.set_cell_text(row, 6, &format!("{:.2}", r.perimeter));
        table.set_cell_text(row, 7, &format!("{:.2}", r.clear_height));
        table.set_cell_text(row, 8, &format!("{:.2}", r.effective_volume()));
        table.set_cell_text(row, 9, &r.floor_finish_summary());
        table.set_cell_text(row, 10, &r.storey_id.to_string());
    }

    // Total / Summary row
    let sum_row = rooms.len() + 1;
    table.set_cell_text(sum_row, 0, "GESAMT");
    table.set_cell_text(sum_row, 1, "");
    table.set_cell_text(sum_row, 2, "");
    table.set_cell_text(sum_row, 3, &format!("{:.2}", total_gross_area));
    table.set_cell_text(sum_row, 4, "-");
    table.set_cell_text(sum_row, 5, &format!("{:.2}", total_calc_area));
    table.set_cell_text(sum_row, 6, "-");
    table.set_cell_text(sum_row, 7, "-");
    table.set_cell_text(sum_row, 8, &format!("{:.2}", total_volume));
    table.set_cell_text(sum_row, 9, "-");
    table.set_cell_text(sum_row, 10, "-");

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::room::RoomFunction;
    use crate::modules::aec::rooms::room::RoomCommand;

    #[test]
    fn schedule_creates_din277_table() {
        let mut scene = Scene::default();
        let mut cl = CommandLine::new();

        // Create 2 rooms
        let mut cmd1 = RoomCommand::new()
            .with_name("Wohnzimmer")
            .with_number("EG-01")
            .with_function(RoomFunction::Living);
        cmd1.commit_polygon(&mut scene, &[(0.0, 0.0), (5.0, 0.0), (5.0, 4.0), (0.0, 4.0)])
            .expect("room 1");

        let mut cmd2 = RoomCommand::new()
            .with_name("Terrasse")
            .with_number("EG-02")
            .with_function(RoomFunction::Balcony);
        cmd2.commit_polygon(&mut scene, &[(5.0, 0.0), (9.0, 0.0), (9.0, 3.0), (5.0, 3.0)])
            .expect("room 2");

        aec_room_schedule(&mut scene, &mut cl);

        let table_ent = scene
            .document
            .entities()
            .find(|e| matches!(e, EntityType::Table(_)))
            .expect("must contain a table");
        let EntityType::Table(table) = table_ent else {
            panic!("not a table");
        };

        assert_eq!(table.rows.len(), 4); // header + 2 rooms + summary
        assert_eq!(table.columns.len(), 11);
        assert_eq!(table.cell(0, 0).map(|c| c.text_value()), Some("Nr."));
        assert_eq!(table.cell(1, 0).map(|c| c.text_value()), Some("EG-01"));
        assert_eq!(table.cell(1, 1).map(|c| c.text_value()), Some("Wohnzimmer"));
        assert_eq!(table.cell(1, 2).map(|c| c.text_value()), Some("NUF 1"));
        assert_eq!(table.cell(2, 0).map(|c| c.text_value()), Some("EG-02"));
        assert_eq!(table.cell(2, 1).map(|c| c.text_value()), Some("Terrasse"));
        assert_eq!(table.cell(2, 2).map(|c| c.text_value()), Some("NUF 1 (50%)"));
        assert_eq!(table.cell(3, 0).map(|c| c.text_value()), Some("GESAMT"));
        // Total gross area: 20.0 + 12.0 = 32.00
        assert_eq!(table.cell(3, 3).map(|c| c.text_value()), Some("32.00"));
        // Total calc area: 20.0 + 6.0 = 26.00
        assert_eq!(table.cell(3, 5).map(|c| c.text_value()), Some("26.00"));
    }
}
