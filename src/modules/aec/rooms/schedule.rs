//! `AEC_ROOMSCHEDULE` command for generating DIN 277 / WoFlV room schedules and area takeoffs as CAD tables.

use acadrust::entities::Table;
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::aec::engine::room::Room;
use crate::modules::aec::engine::room_package::{
    is_room_schedule, write_room_schedule_tag, AEC_ROOM_SCHEDULE_LAYER,
};
use crate::modules::aec::engine::room_xdata::room_from_entity;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../../assets/icons/table.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_ROOMSCHEDULE",
        label: "Schedule",
        icon: ICON,
        event: ModuleEvent::Command("AEC_ROOMSCHEDULE".to_string()),
    }
}

const SCHEDULE_COL_WIDTHS: [f64; 12] = [1.5, 4.0, 3.5, 2.5, 1.5, 2.5, 2.0, 2.0, 2.5, 3.5, 3.5, 1.8];
const SCHEDULE_ROW_HEIGHT: f64 = 0.6;

/// Collects and sorts all AEC rooms in the document by Storey ID, Number, and Name.
pub fn collect_all_rooms(scene: &Scene) -> Vec<Room> {
    let mut rooms: Vec<Room> = Vec::new();
    for entity in scene.document.entities() {
        if let Some(room) = room_from_entity(entity) {
            rooms.push(room);
        }
    }
    rooms.sort_by(|a, b| {
        (a.storey_id, &a.number, &a.name).cmp(&(b.storey_id, &b.number, &b.name))
    });
    rooms
}

/// Constructs a DIN 277 / WoFlV room schedule CAD table at the given coordinate.
pub fn build_room_schedule_table(rooms: &[Room], at: Vector3) -> Table {
    let col_count = 12;
    let row_count = rooms.len() + 2;
    let mut table = Table::new(at, row_count, col_count);

    // Apply column widths and row heights
    for (i, &w) in SCHEDULE_COL_WIDTHS.iter().enumerate() {
        if let Some(col) = table.columns.get_mut(i) {
            col.width = w;
        }
    }
    for row in table.rows.iter_mut() {
        row.height = SCHEDULE_ROW_HEIGHT;
    }

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
    table.set_cell_text(0, 10, "Deckenbelag");
    table.set_cell_text(0, 11, "Geschoss");

    let mut total_gross_area = 0.0;
    let mut total_calc_area = 0.0;
    let mut total_volume = 0.0;

    for (i, r) in rooms.iter().enumerate() {
        let row = i + 1;
        total_gross_area += r.area;
        total_calc_area += r.calculated_area();
        total_volume += r.effective_volume();

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
        table.set_cell_text(row, 10, &r.ceiling_finish_summary());
        table.set_cell_text(row, 11, &r.storey_id.to_string());
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
    table.set_cell_text(sum_row, 11, "-");

    table
}

/// Automatically updates all room schedule tables existing in the scene.
pub fn update_all_room_schedules(scene: &mut Scene) {
    let schedule_handles: Vec<Handle> = scene
        .document
        .entities()
        .filter(|e| {
            is_room_schedule(e)
                || (matches!(e, EntityType::Table(_))
                    && e.common().layer == AEC_ROOM_SCHEDULE_LAYER)
        })
        .map(|e| e.common().handle)
        .collect();

    if schedule_handles.is_empty() {
        return;
    }

    let rooms = collect_all_rooms(scene);

    for h in schedule_handles {
        let pt = match scene.document.get_entity(h) {
            Some(EntityType::Table(t)) => t.insertion_point,
            _ => Vector3::ZERO,
        };
        let mut new_table = build_room_schedule_table(&rooms, pt);
        if let Some(EntityType::Table(tbl)) = scene.document.get_entity_mut(h) {
            new_table.common = tbl.common.clone();
            *tbl = new_table;
        }
    }
    scene.bump_geometry();
}

/// Creates a new room schedule table at the specified coordinate.
pub fn aec_room_schedule_at_point(scene: &mut Scene, pos: Vector3) -> Option<Handle> {
    let rooms = collect_all_rooms(scene);
    if rooms.is_empty() {
        return None;
    }

    scene.ensure_layer(AEC_ROOM_SCHEDULE_LAYER);
    let table = build_room_schedule_table(&rooms, pos);
    let mut table_entity = EntityType::Table(table);
    table_entity.common_mut().layer = AEC_ROOM_SCHEDULE_LAYER.to_string();

    let handle = scene.add_entity(table_entity);
    write_room_schedule_tag(scene, handle);
    scene.bump_geometry();
    Some(handle)
}

/// `AEC_ROOMSCHEDULE` — scans all Room entities and builds or updates a complete DIN 277 / WoFlV room schedule table.
pub fn aec_room_schedule(scene: &mut Scene, command_line: &mut CommandLine) {
    let rooms = collect_all_rooms(scene);

    if rooms.is_empty() {
        command_line.push_info(&crate::tr!("aec", "no-rooms"));
        return;
    }

    if let Some(handle) = aec_room_schedule_at_point(scene, Vector3::ZERO) {
        command_line.push_info(&crate::tr!(
            "aec",
            "schedule-created",
            count = rooms.len(),
            handle = handle.to_string()
        ));
    }
}

/// Interactive command for inserting or updating a room schedule table with point picking and preview.
pub struct RoomScheduleCommand {
    plane: WorkingPlane,
    room_count: usize,
}

impl RoomScheduleCommand {
    pub fn new() -> Self {
        Self {
            plane: WorkingPlane::default(),
            room_count: 0,
        }
    }

    fn preview_grid(&self, point: DVec3, rows: usize) -> WireModel {
        let point = self.plane.to_local(point);
        let columns = SCHEDULE_COL_WIDTHS.len();
        let width: f64 = SCHEDULE_COL_WIDTHS.iter().sum();
        let height = rows as f64 * SCHEDULE_ROW_HEIGHT;
        let mut points = Vec::with_capacity((rows + columns + 2) * 2);

        let mut current_x = 0.0;
        for &w in &SCHEDULE_COL_WIDTHS {
            points.push(self.plane.to_world(point + DVec3::X * current_x).as_vec3().to_array());
            points.push(
                self.plane
                    .to_world(point + DVec3::new(current_x, -height, 0.0))
                    .as_vec3()
                    .to_array(),
            );
            current_x += w;
        }
        points.push(self.plane.to_world(point + DVec3::X * width).as_vec3().to_array());
        points.push(
            self.plane
                .to_world(point + DVec3::new(width, -height, 0.0))
                .as_vec3()
                .to_array(),
        );

        for row in 0..=rows {
            let y = -(row as f64 * SCHEDULE_ROW_HEIGHT);
            points.push(self.plane.to_world(point + DVec3::Y * y).as_vec3().to_array());
            points.push(
                self.plane
                    .to_world(point + DVec3::new(width, y, 0.0))
                    .as_vec3()
                    .to_array(),
            );
        }

        WireModel {
            bg_adapt: None,
            point_marker: None,
            taper_widths: Vec::new(),
            pattern_stations: Vec::new(),
            world_width: 0.0,
            depth_override: None,
            display_visible: true,
            plot_visible: true,
            fill_is_3d: false,
            fill_is_2d_solid: false,
            render_instance: None,
            pick_tris: Vec::new(),
            pick_tris_low: Vec::new(),
            dash_from_start: false,
            dash_align_end: None,
            text_verts: Vec::new(),
            name: "room_schedule_preview".into(),
            points,
            points_low: Vec::new(),
            color: WireModel::CYAN,
            selected: false,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px: 1.0,
            snap_pts: Vec::new(),
            tangent_geoms: Vec::new(),
            aci: 4,
            key_vertices: Vec::new(),
            aabb: WireModel::UNBOUNDED_AABB,
            plinegen: true,
            fill_tris: Vec::new(),
            fill_tris_low: Vec::new(),
        }
    }
}

impl CadCommand for RoomScheduleCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "AEC_ROOMSCHEDULE"
    }

    fn prompt(&self) -> String {
        crate::tr!("aec", "room-schedule-prompt-point")
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        let rows = self.room_count.max(1) + 2;
        vec![self.preview_grid(pt, rows)]
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        CmdResult::Dispatch(format!("AEC_ROOMSCHEDULE_DO {:.6}|{:.6}", pt.x, pt.y))
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        if let Some((x_str, y_str)) = t.split_once(',') {
            if let (Ok(x), Ok(y)) = (x_str.trim().parse::<f64>(), y_str.trim().parse::<f64>()) {
                return Some(CmdResult::Dispatch(format!(
                    "AEC_ROOMSCHEDULE_DO {:.6}|{:.6}",
                    x, y
                )));
            }
        }
        Some(CmdResult::NeedPoint)
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["AEC_ROOMSCHEDULE", "AEC_ROOMSCHEDULE_DO"]
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::room::RoomFunction;
    use crate::modules::aec::rooms::room::RoomCommand;

    #[test]
    fn schedule_creates_and_updates_din277_table() {
        let mut scene = Scene::default();

        // Create 2 rooms
        let mut cmd1 = RoomCommand::new()
            .with_name("Wohnzimmer")
            .with_number("EG-01")
            .with_function(RoomFunction::Living);
        let r1_h = cmd1
            .commit_polygon(&mut scene, &[(0.0, 0.0), (5.0, 0.0), (5.0, 4.0), (0.0, 4.0)])
            .expect("room 1");

        let mut cmd2 = RoomCommand::new()
            .with_name("Terrasse")
            .with_number("EG-02")
            .with_function(RoomFunction::Balcony);
        cmd2.commit_polygon(&mut scene, &[(5.0, 0.0), (9.0, 0.0), (9.0, 3.0), (5.0, 3.0)])
            .expect("room 2");

        // Insert schedule table at (10, 10, 0)
        let schedule_handle =
            aec_room_schedule_at_point(&mut scene, Vector3::new(10.0, 10.0, 0.0))
                .expect("schedule handle");

        let table_ent = scene
            .document
            .get_entity(schedule_handle)
            .expect("must contain schedule table");
        let EntityType::Table(table) = table_ent else {
            panic!("not a table");
        };

        assert_eq!(table.rows.len(), 4); // header + 2 rooms + summary
        assert_eq!(table.columns.len(), 12);
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

        // Now modify room 1 geometry (expand to 6x4 = 24m²) and regenerate
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity_mut(r1_h) {
            pl.vertices[1].location.x = 6.0;
            pl.vertices[2].location.x = 6.0;
        }
        crate::modules::aec::engine::room_regen::regenerate_room_representation(
            &mut scene, r1_h, None,
        );

        // Schedule table must now be automatically updated to 24m² + 12m² = 36m² gross, 24m² + 6m² = 30m² calc
        let table_ent_updated = scene
            .document
            .get_entity(schedule_handle)
            .expect("must contain schedule table");
        let EntityType::Table(table_updated) = table_ent_updated else {
            panic!("not a table");
        };

        assert_eq!(
            table_updated.cell(1, 3).map(|c| c.text_value()),
            Some("24.00")
        );
        assert_eq!(
            table_updated.cell(3, 3).map(|c| c.text_value()),
            Some("36.00")
        );
        assert_eq!(
            table_updated.cell(3, 5).map(|c| c.text_value()),
            Some("30.00")
        );
    }
}
