//! AEC commands — wall/room/storey creation, room schedule, IFC export.
//!
//! `AEC_WALL` is an interactive multi-point drawing command (analogous to
//! `PLINE`); the rest remain non-interactive scaffold commands (matching the
//! former plugin's pragmatic behaviour) that operate directly on `&mut Scene`
//! / the document and report feedback via the command line.

use std::sync::Mutex;

use acadrust::entities::{LwPolyline, LwVertex, Table};
use acadrust::tables::AppId;
use acadrust::types::{Vector2, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, WorkingPlane};
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

use super::engine::{self, find_closed_loop, Room, Storey, Wall};

/// APPID used for all AEC XDATA records (must stay stable for round-trip).
pub const AEC_APPID: &str = "OPENCAD_AEC";

/// In-memory storey store for the scaffold (persistence via document XDATA
/// is a follow-up; matches the former plugin's pragmatism).
static STOREYS: Mutex<Vec<Storey>> = Mutex::new(Vec::new());

/// Register `OPENCAD_AEC` in the APPID table if missing so XDATA survives
/// DWG/DXF round-trip.
fn ensure_app_id(doc: &mut CadDocument) {
    if !doc.app_ids.contains(AEC_APPID) {
        let mut app = AppId::new(AEC_APPID);
        app.handle = doc.allocate_handle();
        let _ = doc.app_ids.add(app);
    }
}

/// Attach (or replace) an `OPENCAD_AEC` XDATA record on `handle`.
fn write_aec_record(doc: &mut CadDocument, handle: Handle, record: ExtendedDataRecord) -> bool {
    ensure_app_id(doc);
    let app_handle = doc.app_ids.get(AEC_APPID).map(|a| a.handle.value());
    let Some(entity) = doc.get_entity_mut(handle) else {
        return false;
    };
    let xd = &mut entity.common_mut().extended_data;
    let kept: Vec<_> = xd
        .records()
        .iter()
        .filter(|r| r.application_name != AEC_APPID)
        .cloned()
        .collect();
    xd.clear();
    for r in kept {
        xd.add_record(r);
    }
    xd.add_record(record);
    if let Some(ah) = app_handle {
        xd.raw_dwg_eed.retain(|(a, _)| *a != ah);
    }
    true
}

/// Read the `OPENCAD_AEC` record on `entity`, if any.
fn read_aec_record(entity: &EntityType) -> Option<&ExtendedDataRecord> {
    entity.common().extended_data.get_record(AEC_APPID)
}

/// Collect baseline segments of every `WALL`-tagged `LwPolyline`.
fn collect_wall_segments(doc: &CadDocument) -> Vec<((f64, f64), (f64, f64))> {
    let mut segments = Vec::new();
    for entity in doc.entities() {
        let EntityType::LwPolyline(pl) = entity else {
            continue;
        };
        let is_wall = matches!(
            read_aec_record(entity).and_then(|r| r.values.first()),
            Some(XDataValue::String(kind)) if kind == "WALL"
        );
        if !is_wall {
            continue;
        }
        for pair in pl.vertices.windows(2) {
            let a = (pair[0].location.x, pair[0].location.y);
            let b = (pair[1].location.x, pair[1].location.y);
            segments.push((a, b));
        }
        if pl.is_closed {
            if let (Some(first), Some(last)) = (pl.vertices.first(), pl.vertices.last()) {
                segments.push((
                    (last.location.x, last.location.y),
                    (first.location.x, first.location.y),
                ));
            }
        }
    }
    segments
}

/// Build the `WALL` XDATA record for `wall` (shared by the interactive draw
/// command and the properties-panel edit path).
fn wall_record(wall: &Wall) -> ExtendedDataRecord {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("WALL".to_string()));
    record.add_value(XDataValue::Distance(wall.thickness));
    record.add_value(XDataValue::Distance(wall.height));
    record.add_value(XDataValue::String(
        wall.material_ref.clone().unwrap_or_default(),
    ));
    record.add_value(XDataValue::Integer32(wall.storey_id as i32));
    record
}

/// Parse a `WALL` XDATA record back into a [`Wall`] (inverse of
/// [`wall_record`]). Returns `None` if `entity` isn't `WALL`-tagged or the
/// record doesn't have the expected shape.
pub fn wall_from_entity(entity: &EntityType) -> Option<Wall> {
    let record = read_aec_record(entity)?;
    match record.values.as_slice() {
        [XDataValue::String(kind), XDataValue::Distance(thickness), XDataValue::Distance(height), XDataValue::String(material), XDataValue::Integer32(storey_id)]
            if kind == "WALL" =>
        {
            Some(Wall {
                thickness: *thickness,
                height: *height,
                material_ref: if material.is_empty() {
                    None
                } else {
                    Some(material.clone())
                },
                storey_id: *storey_id as u32,
            })
        }
        _ => None,
    }
}

/// Write `wall` back into `handle`'s `WALL` XDATA record, replacing the
/// previous one (used by the properties-panel edit path). Reuses
/// [`wall_record`] so the field layout stays in one place.
pub fn write_wall_properties(doc: &mut CadDocument, handle: Handle, wall: &Wall) -> bool {
    write_aec_record(doc, handle, wall_record(wall))
}

/// Register the `OPENCAD_AEC` APPID up front so an interactive `AEC_WALL`
/// command can embed XDATA directly on entities it builds (it has no
/// `&mut CadDocument` while collecting points).
pub fn ensure_wall_app_id(doc: &mut CadDocument) {
    ensure_app_id(doc);
}

/// Default wall height (metres) offered by the command-line prompt after
/// the point chain is finished.
const DEFAULT_WALL_HEIGHT: f64 = 2.8;
/// Default wall thickness (metres) offered by the command-line prompt after
/// the height has been entered.
const DEFAULT_WALL_THICKNESS: f64 = 0.2;

/// Drawing phase of an in-progress `AEC_WALL` command.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WallPhase {
    /// Collecting click points, like `PLINE`.
    Drawing,
    /// Point chain finished; waiting for a height value on the command line.
    AskHeight,
    /// Height entered; waiting for a thickness value on the command line.
    AskThickness,
}

/// `AEC_WALL` — interactive multi-point wall polyline drawing, analogous to
/// `PLINE`. Once the point chain is finished (Enter/Escape), the command
/// prompts for height and thickness on the command line (defaults 2.8 / 0.2)
/// before writing the final `WALL` XDATA record and finalizing the entity.
pub struct WallCommand {
    vertices: Vec<DVec3>,
    live_handle: Option<Handle>,
    plane: WorkingPlane,
    wall: Wall,
    phase: WallPhase,
}

impl WallCommand {
    pub fn new() -> Self {
        Self {
            vertices: Vec::new(),
            live_handle: None,
            plane: WorkingPlane::default(),
            wall: Wall::new(DEFAULT_WALL_THICKNESS, DEFAULT_WALL_HEIGHT, 0),
            phase: WallPhase::Drawing,
        }
    }

    /// Parse a command-line value, falling back to `default` for an empty
    /// input; rejects non-positive/invalid input by keeping the default.
    fn parse_dimension(text: &str, default: f64) -> f64 {
        let t = text.trim();
        if t.is_empty() {
            return default;
        }
        match t.parse::<f64>() {
            Ok(v) if v > 0.0 => v,
            _ => default,
        }
    }

    /// Begin prompting for the wall's height/thickness once the point chain
    /// is done; returns the result that keeps the command active for the
    /// command-line follow-up.
    fn start_dimension_prompt(&mut self) -> CmdResult {
        if self.live_handle.is_none() {
            return CmdResult::Cancel;
        }
        self.phase = WallPhase::AskHeight;
        CmdResult::NeedPoint
    }

    fn build_entity(&self) -> Option<EntityType> {
        if self.vertices.len() < 2 {
            return None;
        }
        let mut pl = LwPolyline::new();
        for pt in &self.vertices {
            let local = self.plane.to_local(*pt);
            pl.add_vertex(LwVertex::new(Vector2::new(local.x, local.y)));
        }
        let mut entity = self.plane.place_entity(EntityType::LwPolyline(pl));
        entity
            .common_mut()
            .extended_data
            .add_record(wall_record(&self.wall));
        Some(entity)
    }

    fn sync_live(&self, finish: bool) -> CmdResult {
        match (self.build_entity(), self.live_handle) {
            (Some(entity), Some(handle)) => CmdResult::UpdateLiveEntity {
                handle,
                entity,
                finish,
            },
            (Some(entity), None) => CmdResult::CommitLiveEntity(entity),
            (None, _) => CmdResult::Cancel,
        }
    }

    fn undo_last_vertex(&mut self) -> CmdResult {
        if self.vertices.is_empty() {
            return CmdResult::NeedPoint;
        }
        self.vertices.pop();
        match self.vertices.len() {
            0 => CmdResult::NeedPoint,
            1 => match self.live_handle.take() {
                Some(h) => CmdResult::RemoveLiveEntity(h),
                None => CmdResult::NeedPoint,
            },
            _ => self.sync_live(false),
        }
    }
}

impl CadCommand for WallCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "AEC_WALL"
    }

    fn prompt(&self) -> String {
        match self.phase {
            WallPhase::Drawing if self.vertices.is_empty() => {
                "AEC_WALL  Specify start point:".to_string()
            }
            WallPhase::Drawing => {
                format!("AEC_WALL  Next pt  [{}pts]:", self.vertices.len())
            }
            WallPhase::AskHeight => {
                format!("AEC_WALL  Specify wall height <{DEFAULT_WALL_HEIGHT}>:")
            }
            WallPhase::AskThickness => {
                format!("AEC_WALL  Specify wall thickness <{DEFAULT_WALL_THICKNESS}>:")
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.phase {
            WallPhase::Drawing if self.vertices.is_empty() => Vec::new(),
            WallPhase::Drawing => vec![CmdOption::new("Undo", "U"), CmdOption::enter("Done")],
            WallPhase::AskHeight | WallPhase::AskThickness => Vec::new(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.phase != WallPhase::Drawing {
            // Height/thickness prompt is active; ignore stray clicks.
            return CmdResult::NeedPoint;
        }
        self.vertices.push(pt);
        if self.vertices.len() >= 2 {
            self.sync_live(false)
        } else {
            CmdResult::NeedPoint
        }
    }

    fn set_live_handle(&mut self, handle: Handle) {
        self.live_handle = Some(handle);
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.phase {
            WallPhase::Drawing => self.start_dimension_prompt(),
            WallPhase::AskHeight | WallPhase::AskThickness => {
                self.on_text_input("").unwrap_or(CmdResult::Cancel)
            }
        }
    }

    fn enter_accepts_default_start(&self) -> bool {
        self.phase == WallPhase::Drawing && self.vertices.is_empty()
    }

    fn on_escape(&mut self) -> CmdResult {
        self.on_enter()
    }

    fn on_space_change(&mut self) -> CmdResult {
        self.on_enter()
    }

    fn wants_text_input(&self) -> bool {
        !self.vertices.is_empty() || self.phase != WallPhase::Drawing
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.phase == WallPhase::Drawing && !self.vertices.is_empty()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match self.phase {
            WallPhase::Drawing => match text.trim().to_uppercase().as_str() {
                "U" | "UNDO" => Some(self.undo_last_vertex()),
                _ => None,
            },
            WallPhase::AskHeight => {
                self.wall.height = Self::parse_dimension(text, DEFAULT_WALL_HEIGHT);
                self.phase = WallPhase::AskThickness;
                Some(CmdResult::NeedPoint)
            }
            WallPhase::AskThickness => {
                self.wall.thickness = Self::parse_dimension(text, DEFAULT_WALL_THICKNESS);
                Some(self.sync_live(true))
            }
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if self.phase == WallPhase::Drawing && !self.vertices.is_empty() {
            Some(self.undo_last_vertex())
        } else {
            None
        }
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
        command_line.push_info(&format!(
            "AEC: Detected closed wall loop, created room '{}' ({} vertices) at {handle}",
            room.name,
            pts.len()
        ));
    } else {
        command_line.push_info(&format!(
            "AEC: No closed wall loop found, created demo room '{}' at {handle}",
            room.name
        ));
    }
}

/// `AEC_STOREY` — append an in-memory storey and report it.
pub fn aec_storey(_scene: &mut Scene, command_line: &mut CommandLine) {
    let mut storeys = STOREYS.lock().unwrap();
    let next_id = storeys.len() as u32;
    let new_storey = Storey::new(
        format!("Level {}", next_id + 1),
        (next_id as f64) * 3.0,
        3.0,
    );
    storeys.push(new_storey.clone());

    command_line.push_info(&format!(
        "AEC: Added storey '{}' at elevation {}",
        new_storey.name, new_storey.elevation
    ));
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
        command_line.push_info("AEC: No rooms found in document.");
        return;
    }

    // Real TABLE entity: header row + one row per room.
    let row_count = rooms.len() + 1;
    let mut table = Table::new(Vector3::ZERO, row_count, 3);
    table.set_cell_text(0, 0, "Name");
    table.set_cell_text(0, 1, "Area");
    table.set_cell_text(0, 2, "Storey ID");
    for (row, (name, area, storey)) in rooms.iter().enumerate() {
        table.set_cell_text(row + 1, 0, name);
        table.set_cell_text(row + 1, 1, &format!("{area:.2}"));
        table.set_cell_text(row + 1, 2, &storey.to_string());
    }

    let handle = scene.add_entity(EntityType::Table(table));
    scene.bump_geometry();
    command_line.push_info(&format!(
        "AEC: Room schedule table with {} room(s) created at {handle}",
        rooms.len()
    ));
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
                // 0: "WALL", 1: thick, 2: height, 3: mat, 4: storey_id
                if record.values.len() >= 5 {
                    let thickness = if let XDataValue::Distance(d) = record.values[1] {
                        d
                    } else {
                        0.2
                    };
                    let height = if let XDataValue::Distance(d) = record.values[2] {
                        d
                    } else {
                        2.8
                    };
                    let material_ref = if let XDataValue::String(s) = &record.values[3] {
                        if s.is_empty() {
                            None
                        } else {
                            Some(s.clone())
                        }
                    } else {
                        None
                    };
                    let storey_id = if let XDataValue::Integer32(i) = record.values[4] {
                        i as u32
                    } else {
                        0
                    };
                    ifc_scene.walls.push(Wall {
                        thickness,
                        height,
                        material_ref,
                        storey_id,
                    });
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
    command_line.push_info(&format!(
        "AEC: Exported IFC4 SPF ({} bytes)",
        ifc_data.len()
    ));
    command_line.push_info("(Note: Real file-save dialog is a future step)");
}

#[cfg(test)]
mod wall_command_tests {
    use super::*;
    use acadrust::Handle;
    use glam::DVec3;

    fn wall_xdata(entity: &EntityType) -> Option<&ExtendedDataRecord> {
        entity.common().extended_data.get_record(AEC_APPID)
    }

    #[test]
    fn first_point_only_waits_for_the_next_one() {
        let mut cmd = WallCommand::new();
        assert!(matches!(
            cmd.on_point(DVec3::new(0.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));
    }

    #[test]
    fn second_point_commits_a_two_vertex_wall_polyline_with_xdata() {
        let mut cmd = WallCommand::new();
        assert!(matches!(
            cmd.on_point(DVec3::new(0.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));

        match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitLiveEntity(EntityType::LwPolyline(pl)) => {
                assert_eq!(pl.vertices.len(), 2);
            }
            _ => panic!("second point should commit a live wall polyline"),
        }
    }

    #[test]
    fn later_points_update_the_same_live_polyline_as_a_wall_chain() {
        let mut cmd = WallCommand::new();
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        let committed = cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        let entity = match committed {
            CmdResult::CommitLiveEntity(e) => e,
            _ => panic!("expected CommitLiveEntity"),
        };
        assert!(
            wall_xdata(&entity).is_some(),
            "committed wall segment should carry OPENCAD_AEC/WALL xdata"
        );

        let handle = Handle::new(7);
        cmd.set_live_handle(handle);
        match cmd.on_point(DVec3::new(5.0, 3.0, 0.0)) {
            CmdResult::UpdateLiveEntity {
                handle: updated,
                entity: EntityType::LwPolyline(pl),
                finish,
            } => {
                assert_eq!(updated, handle);
                assert_eq!(pl.vertices.len(), 3);
                assert!(!finish);
            }
            _ => panic!("a third point should extend the same live wall chain"),
        }
    }

    #[test]
    fn undo_drops_the_last_vertex_and_removes_the_live_entity_below_two_points() {
        let mut cmd = WallCommand::new();
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        cmd.set_live_handle(Handle::new(3));

        match cmd.on_text_input("U") {
            Some(CmdResult::RemoveLiveEntity(h)) => assert_eq!(h, Handle::new(3)),
            _ => panic!("undoing back to a single vertex should remove the live entity"),
        }
    }

    #[test]
    fn enter_after_the_point_chain_starts_the_height_prompt_instead_of_finalizing() {
        let handle = Handle::new(11);

        let mut enter_cmd = WallCommand::new();
        enter_cmd.set_live_handle(handle);
        assert!(matches!(enter_cmd.on_enter(), CmdResult::NeedPoint));
        assert!(enter_cmd.prompt().contains("height"));

        let mut escape_cmd = WallCommand::new();
        escape_cmd.set_live_handle(handle);
        assert!(matches!(escape_cmd.on_escape(), CmdResult::NeedPoint));
        assert!(escape_cmd.prompt().contains("height"));
    }

    #[test]
    fn height_then_thickness_prompt_writes_entered_values_and_finalizes() {
        let handle = Handle::new(11);
        let mut cmd = WallCommand::new();
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        cmd.set_live_handle(handle);

        // Finish the point chain -> height prompt.
        assert!(matches!(cmd.on_enter(), CmdResult::NeedPoint));

        // Height entered -> thickness prompt.
        match cmd.on_text_input("3.5") {
            Some(CmdResult::NeedPoint) => {}
            _ => panic!("expected height entry to move to the thickness prompt"),
        }
        assert!(cmd.prompt().contains("thickness"));

        // Thickness entered -> final wall XDATA + finalize.
        match cmd.on_text_input("0.3") {
            Some(CmdResult::UpdateLiveEntity {
                handle: updated,
                entity: EntityType::LwPolyline(pl),
                finish,
            }) => {
                assert_eq!(updated, handle);
                assert!(finish);
                let record = pl
                    .common
                    .extended_data
                    .get_record(AEC_APPID)
                    .expect("finalized wall should carry WALL xdata");
                assert!(matches!(record.values[1], XDataValue::Distance(t) if (t - 0.3).abs() < 1e-9));
                assert!(matches!(record.values[2], XDataValue::Distance(h) if (h - 3.5).abs() < 1e-9));
            }
            _ => panic!("expected thickness entry to finalize the live wall"),
        }
    }

    #[test]
    fn empty_height_and_thickness_prompts_fall_back_to_defaults() {
        let handle = Handle::new(4);
        let mut cmd = WallCommand::new();
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        cmd.set_live_handle(handle);
        cmd.on_enter();

        // Bare Enter on the height prompt (empty text) keeps the default.
        cmd.on_enter();
        match cmd.on_enter() {
            CmdResult::UpdateLiveEntity {
                entity: EntityType::LwPolyline(pl),
                finish,
                ..
            } => {
                assert!(finish);
                let record = pl.common.extended_data.get_record(AEC_APPID).unwrap();
                assert!(
                    matches!(record.values[1], XDataValue::Distance(t) if (t - DEFAULT_WALL_THICKNESS).abs() < 1e-9)
                );
                assert!(
                    matches!(record.values[2], XDataValue::Distance(h) if (h - DEFAULT_WALL_HEIGHT).abs() < 1e-9)
                );
            }
            _ => panic!("expected default height/thickness to finalize the live wall"),
        }
    }

    /// Draw four wall segments through `WallCommand` exactly as the
    /// interactive host would (point chain, then height/thickness prompt),
    /// then commit each finalized entity into a real `Scene`. Regression
    /// check for `AEC_ROOM`'s closed-loop detection against interactively
    /// drawn walls (previously only exercised against demo geometry).
    #[test]
    fn aec_room_detects_a_closed_loop_from_interactively_drawn_walls() {
        use crate::ui::command_line::CommandLine;

        let mut scene = Scene::new();
        let corners = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(4.0, 0.0, 0.0),
            DVec3::new(4.0, 3.0, 0.0),
            DVec3::new(0.0, 3.0, 0.0),
            DVec3::new(0.0, 0.0, 0.0),
        ];

        for pair in corners.windows(2) {
            let mut cmd = WallCommand::new();
            let committed = match cmd.on_point(pair[0]) {
                CmdResult::NeedPoint => cmd.on_point(pair[1]),
                other => other,
            };
            let entity = match committed {
                CmdResult::CommitLiveEntity(e) => e,
                _ => panic!("two points should commit a live wall segment"),
            };
            let handle = scene.add_entity(entity);
            cmd.set_live_handle(handle);

            // Finish the point chain, then accept default height/thickness
            // (Drawing -> AskHeight -> AskThickness -> finalize).
            cmd.on_enter();
            cmd.on_enter();
            let finalized = cmd.on_enter();
            match finalized {
                CmdResult::UpdateLiveEntity {
                    handle: h, entity, ..
                } => {
                    if let Some(slot) = scene.document.get_entity_mut(h) {
                        *slot = entity;
                    }
                }
                _ => panic!("height/thickness prompt should finalize the wall segment"),
            }
        }

        let mut command_line = CommandLine::default();
        aec_room(&mut scene, &mut command_line);

        let room_record = scene
            .document
            .entities()
            .filter_map(read_aec_record)
            .find(|r| matches!(r.values.first(), Some(XDataValue::String(k)) if k == "ROOM"))
            .expect("aec_room should have written a ROOM xdata record");
        let area = match room_record.values.get(2) {
            Some(XDataValue::Real(a)) => *a,
            _ => panic!("ROOM record should carry an area value"),
        };
        assert!(
            (area - 12.0).abs() < 1e-6,
            "expected the detected 4x3 wall loop to yield area 12.0, got {area}"
        );
    }

    /// `wall_from_entity` is the inverse of `wall_record` — the properties
    /// panel reads a `Wall` this way to populate the height/thickness/material
    /// fields for a WALL-tagged entity.
    #[test]
    fn wall_from_entity_reads_back_a_finalized_wall_record() {
        let mut cmd = WallCommand::new();
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        cmd.set_live_handle(Handle::new(9));
        cmd.on_enter();
        cmd.on_text_input("3.5");
        let entity = match cmd.on_text_input("0.3") {
            Some(CmdResult::UpdateLiveEntity { entity, .. }) => entity,
            _ => panic!("expected thickness entry to finalize the live wall"),
        };

        let wall = wall_from_entity(&entity).expect("finalized entity should read back as a Wall");
        assert!((wall.thickness - 0.3).abs() < 1e-9);
        assert!((wall.height - 3.5).abs() < 1e-9);
        assert!(wall.material_ref.is_none());
    }

    /// A plain (non-WALL-tagged) entity must not be misread as a wall — this
    /// is what keeps the properties-panel Wall section from appearing on
    /// regular polylines.
    #[test]
    fn wall_from_entity_returns_none_for_a_plain_polyline() {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(1.0, 0.0)));
        let entity = EntityType::LwPolyline(pl);
        assert!(wall_from_entity(&entity).is_none());
    }

    /// `write_wall_properties` is the properties-panel writeback path: it
    /// must reuse `wall_record`'s exact layout so `wall_from_entity` and
    /// `collect_wall_segments`/`aec_room` keep working after an edit.
    #[test]
    fn write_wall_properties_updates_the_wall_xdata_in_place() {
        let mut scene = Scene::new();
        let mut cmd = WallCommand::new();
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitLiveEntity(e) => e,
            _ => panic!("two points should commit a live wall segment"),
        };
        let handle = scene.add_entity(entity);

        let mut wall = wall_from_entity(scene.document.get_entity(handle).unwrap())
            .expect("committed segment should already carry WALL xdata");
        wall.height = 3.2;
        wall.thickness = 0.25;
        wall.material_ref = Some("Concrete".to_string());
        assert!(write_wall_properties(&mut scene.document, handle, &wall));

        let updated = wall_from_entity(scene.document.get_entity(handle).unwrap())
            .expect("entity should still read back as a wall after the edit");
        assert!((updated.height - 3.2).abs() < 1e-9);
        assert!((updated.thickness - 0.25).abs() < 1e-9);
        assert_eq!(updated.material_ref.as_deref(), Some("Concrete"));

        // The AEC_ROOM segment collector still sees this wall after the edit.
        let segments = collect_wall_segments(&scene.document);
        assert_eq!(segments.len(), 1);
    }
}
