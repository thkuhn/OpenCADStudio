//! `AEC_ROOM` interactive command for room boundary creation, room stamp placement, and area calculation.
//!
//! Supports:
//! - Auto-detect / Pick point inside closed walls (`RoomDrawMode::PickPoint`)
//! - Multi-point polygon drawing (`RoomDrawMode::Polygon`)
//! - 2-point bounding rectangle (`RoomDrawMode::Rectangle`)
//! - Conversion of existing closed lightweight polylines (`RoomDrawMode::SelectPolyline`)
//! - Live Properties Panel editing for Room Name, Number, DIN 277 Function, Height, Factor, Floor Finish

#![allow(unused_imports)]
use glam::DVec3;
use uuid::Uuid;

use acadrust::entities::{LwPolyline, LwVertex, MText};
use acadrust::types::{Vector2, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{EntityType, Handle};

use crate::command::{
    CadCommand, CmdOption, CmdResult, LiveCommandField, LiveCommandProperties, LiveFieldValue,
    WorkingPlane,
};
use crate::modules::aec::engine::geometry::{area, centroid, perimeter, signed_area};
use crate::modules::aec::engine::loop_detection::{find_all_closed_loops, find_closed_loop_at_point};
use crate::modules::aec::engine::plan_view::PlanPhase;
use crate::modules::aec::engine::room::{Room, RoomFinish, RoomFunction};
use crate::modules::aec::engine::room_package::*;
use crate::modules::aec::engine::room_regen::regenerate_room_representation;
use crate::modules::aec::engine::room_xdata::{room_from_entity, write_room_record};
use crate::modules::aec::engine::xdata::{
    collect_wall_segments, collect_wall_structural_segments, write_aec_record, AEC_APPID,
};
use crate::modules::aec::engine::StyleLibrary;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

pub const ICON_AUTO: IconKind = IconKind::Svg(include_bytes!("../../../../assets/icons/boundary.svg"));
pub const ICON_RECT: IconKind = IconKind::Svg(include_bytes!("../../../../assets/icons/array_rect.svg"));
pub const ICON_POLY: IconKind = IconKind::Svg(include_bytes!("../../../../assets/icons/polyline.svg"));
pub const ICON_OBJECT: IconKind = IconKind::Svg(include_bytes!("../../../../assets/icons/area.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_ROOM",
        label: "Room",
        icon: ICON_AUTO,
        event: ModuleEvent::Command("AEC_ROOM".to_string()),
    }
}

pub fn tool_pick() -> ToolDef {
    ToolDef {
        id: "AEC_ROOM_PICK",
        label: "Room (Auto)",
        icon: ICON_AUTO,
        event: ModuleEvent::Command("AEC_ROOM_PICK".to_string()),
    }
}

pub fn tool_rect() -> ToolDef {
    ToolDef {
        id: "AEC_ROOM_RECT",
        label: "Room (Rectangle)",
        icon: ICON_RECT,
        event: ModuleEvent::Command("AEC_ROOM_RECT".to_string()),
    }
}

pub fn tool_poly() -> ToolDef {
    ToolDef {
        id: "AEC_ROOM_POLY",
        label: "Room (Polygon)",
        icon: ICON_POLY,
        event: ModuleEvent::Command("AEC_ROOM_POLY".to_string()),
    }
}

pub fn tool_object() -> ToolDef {
    ToolDef {
        id: "AEC_ROOM_OBJECT",
        label: "Room (Object)",
        icon: ICON_OBJECT,
        event: ModuleEvent::Command("AEC_ROOM_OBJECT".to_string()),
    }
}

/// Drawing sub-modes for `AEC_ROOM`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomDrawMode {
    /// Click a point inside closed walls to detect room boundaries automatically.
    PickPoint,
    /// Click polygon vertices one by one.
    Polygon,
    /// Pick 2 opposite corners of a rectangle.
    Rectangle,
    /// Pick an existing closed polyline to convert to a room.
    SelectPolyline,
}

/// `AEC_ROOM` interactive command.
pub struct RoomCommand {
    pub(crate) vertices: Vec<DVec3>,
    plane: WorkingPlane,
    pub(crate) mode: RoomDrawMode,
    pub(crate) rect_corner: Option<DVec3>,
    library: Option<StyleLibrary>,
    pub(crate) name: String,
    pub(crate) number: String,
    pub(crate) function: RoomFunction,
    pub(crate) clear_height: f64,
    pub(crate) factor: f64,
    pub(crate) floor_finish_name: String,
    pub(crate) storey_id: u32,
    pub(crate) base_z: f64,
    pub(crate) phase: PlanPhase,
    pub(crate) stamp_pos: Option<(f64, f64)>,
    pub(crate) last_committed: Option<Handle>,
}

impl RoomCommand {
    pub fn new() -> Self {
        Self::new_with_library(Some(crate::modules::aec::engine::library::load_or_seed()))
    }

    pub fn new_with_library(library: Option<StyleLibrary>) -> Self {
        Self {
            vertices: Vec::new(),
            plane: WorkingPlane::default(),
            mode: RoomDrawMode::PickPoint,
            rect_corner: None,
            library,
            name: "Wohnen".to_string(),
            number: "01".to_string(),
            function: RoomFunction::Living,
            clear_height: 2.50,
            factor: 1.0,
            floor_finish_name: "Parkett".to_string(),
            storey_id: 0,
            base_z: 0.0,
            phase: PlanPhase::New,
            stamp_pos: None,
            last_committed: None,
        }
    }

    pub fn with_mode(mut self, mode: RoomDrawMode) -> Self {
        self.mode = mode;
        self.vertices.clear();
        self.rect_corner = None;
        self
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn with_number(mut self, number: impl Into<String>) -> Self {
        self.number = number.into();
        self
    }

    pub fn with_function(mut self, func: RoomFunction) -> Self {
        self.factor = func.default_factor();
        self.function = func;
        self
    }

    pub fn with_storey_id(mut self, storey_id: u32) -> Self {
        self.storey_id = storey_id;
        self
    }

    pub fn with_base_z(mut self, base_z: f64) -> Self {
        self.base_z = base_z;
        self
    }

    /// Sets floor finish string and parses standard finishes.
    pub fn set_floor_finish_str(&mut self, s: &str) {
        self.floor_finish_name = s.to_string();
    }

    /// Builds a `Vec<RoomFinish>` from the current floor finish configuration.
    pub fn current_finishes(&self) -> Option<Vec<RoomFinish>> {
        let trimmed = self.floor_finish_name.trim();
        if trimmed.is_empty() || trimmed == "-" {
            return None;
        }
        let lower = trimmed.to_lowercase();
        let hatch = if lower.contains("fliese") || lower.contains("tile") {
            Some("SQUARE".to_string())
        } else if lower.contains("parkett") || lower.contains("wood") {
            Some("ANSI31".to_string())
        } else {
            None
        };
        let mut finish = RoomFinish::new(trimmed, 0.05);
        if let Some(h) = hatch {
            finish = finish.with_hatch(h);
        }
        Some(vec![finish])
    }

    /// Commits the given 2D boundary polygon to the document as a Room.
    pub fn commit_polygon(
        &mut self,
        scene: &mut Scene,
        points: &[(f64, f64)],
    ) -> Result<Handle, String> {
        if points.len() < 3 {
            return Err("A closed room requires at least 3 vertices.".to_string());
        }
        if area(points) < 1e-4 {
            return Err("Room polygon area is too small or degenerate.".to_string());
        }

        let mut pl = LwPolyline::new();
        pl.is_closed = true;
        pl.elevation = self.base_z;
        for (x, y) in points {
            pl.add_vertex(LwVertex::new(Vector2::new(*x, *y)));
        }

        let mut entity = EntityType::LwPolyline(pl);
        entity.common_mut().layer = AEC_ROOM_CARRIER_LAYER.to_string();

        let handle = scene.add_entity(entity);

        let mut room = Room::from_polygon(&self.name, points, self.clear_height, self.storey_id)
            .with_number(&self.number)
            .with_function(self.function.clone())
            .with_factor(self.factor)
            .with_clear_height(self.clear_height)
            .with_base_z(self.base_z)
            .with_phase(self.phase);

        if let Some(pos) = self.stamp_pos {
            room = room.with_stamp_pos(pos);
        } else {
            room = room.with_stamp_pos(centroid(points));
        }

        if let Some(finishes) = self.current_finishes() {
            room = room.with_floor_finish(finishes);
        }

        write_room_record(&mut scene.document, handle, &room);
        let _ = regenerate_room_representation(scene, handle, self.library.as_ref());

        self.last_committed = Some(handle);
        scene.bump_geometry();

        Ok(handle)
    }

    /// Converts an existing closed polyline entity into a Room.
    pub fn convert_selected_polyline(
        &mut self,
        scene: &mut Scene,
        handle: Handle,
    ) -> Result<Handle, String> {
        let Some(entity) = scene.document.get_entity_mut(handle) else {
            return Err(format!("Entity {handle} not found."));
        };
        let EntityType::LwPolyline(pl) = entity else {
            return Err("Selected entity is not a 2D polyline (LwPolyline).".to_string());
        };
        if pl.vertices.len() < 3 {
            return Err("Polyline must have at least 3 vertices.".to_string());
        }
        pl.is_closed = true;
        pl.common.layer = AEC_ROOM_CARRIER_LAYER.to_string();

        let points: Vec<(f64, f64)> = pl
            .vertices
            .iter()
            .map(|v| (v.location.x, v.location.y))
            .collect();

        let mut room = Room::from_polygon(&self.name, &points, self.clear_height, self.storey_id)
            .with_number(&self.number)
            .with_function(self.function.clone())
            .with_factor(self.factor)
            .with_clear_height(self.clear_height)
            .with_base_z(self.base_z)
            .with_phase(self.phase);

        if let Some(pos) = self.stamp_pos {
            room = room.with_stamp_pos(pos);
        } else {
            room = room.with_stamp_pos(centroid(&points));
        }

        if let Some(finishes) = self.current_finishes() {
            room = room.with_floor_finish(finishes);
        }

        write_room_record(&mut scene.document, handle, &room);
        let _ = regenerate_room_representation(scene, handle, self.library.as_ref());

        self.last_committed = Some(handle);
        scene.bump_geometry();

        Ok(handle)
    }

    /// Auto-detects the closed wall loop around `pt` and creates the room.
    pub fn auto_detect_at_point(&mut self, scene: &mut Scene, pt: (f64, f64)) -> Result<Handle, String> {
        let structural_segments = collect_wall_structural_segments(&scene.document);
        let loop_pts = find_closed_loop_at_point(&structural_segments, pt, 1e-3).or_else(|| {
            let baseline_segments = collect_wall_segments(&scene.document);
            find_closed_loop_at_point(&baseline_segments, pt, 1e-3)
        });
        let Some(loop_pts) = loop_pts else {
            return Err("No enclosing closed wall loop found around clicked point.".to_string());
        };
        self.stamp_pos = Some(pt);
        self.commit_polygon(scene, &loop_pts)
    }
}

impl Default for RoomCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl CadCommand for RoomCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "AEC_ROOM"
    }

    fn prompt(&self) -> String {
        match self.mode {
            RoomDrawMode::PickPoint => crate::tr!("aec", "room-prompt-pick-point"),
            RoomDrawMode::Polygon => {
                if self.vertices.is_empty() {
                    crate::tr!("aec", "room-prompt-start")
                } else if self.vertices.len() < 3 {
                    crate::tr!("aec", "room-prompt-next", count = self.vertices.len())
                } else {
                    crate::tr!("aec", "room-prompt-close", count = self.vertices.len())
                }
            }
            RoomDrawMode::Rectangle => {
                if self.rect_corner.is_none() {
                    crate::tr!("aec", "room-prompt-rect-first")
                } else {
                    crate::tr!("aec", "room-prompt-rect-second")
                }
            }
            RoomDrawMode::SelectPolyline => crate::tr!("aec", "room-prompt-select-obj"),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.mode {
            RoomDrawMode::PickPoint => vec![
                CmdOption::new("Polygon", "P"),
                CmdOption::new("Rectangle", "R"),
                CmdOption::new("Select", "S"),
            ],
            RoomDrawMode::Polygon => {
                if self.vertices.is_empty() {
                    vec![
                        CmdOption::new("PickPoint", "K"),
                        CmdOption::new("Rectangle", "R"),
                        CmdOption::new("Select", "S"),
                    ]
                } else if self.vertices.len() < 3 {
                    vec![CmdOption::new("Undo", "U")]
                } else {
                    vec![
                        CmdOption::new("Close", "C"),
                        CmdOption::new("Undo", "U"),
                        CmdOption::enter("Done"),
                    ]
                }
            }
            RoomDrawMode::Rectangle => vec![
                CmdOption::new("PickPoint", "K"),
                CmdOption::new("Polygon", "P"),
                CmdOption::new("Select", "S"),
            ],
            RoomDrawMode::SelectPolyline => vec![
                CmdOption::new("PickPoint", "K"),
                CmdOption::new("Polygon", "P"),
                CmdOption::new("Rectangle", "R"),
            ],
        }
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        let mut wires = Vec::new();
        match self.mode {
            RoomDrawMode::Polygon => {
                if self.vertices.is_empty() {
                    return wires;
                }
                let mut pts: Vec<[f32; 3]> = self
                    .vertices
                    .iter()
                    .map(|v| v.as_vec3().to_array())
                    .collect();
                pts.push(pt.as_vec3().to_array());

                wires.push(WireModel::solid(
                    "room_polygon_edges".into(),
                    pts,
                    WireModel::CYAN,
                    false,
                ));

                if self.vertices.len() >= 2 {
                    wires.push(WireModel::solid(
                        "room_closing_wire".into(),
                        vec![pt.as_vec3().to_array(), self.vertices[0].as_vec3().to_array()],
                        WireModel::HOVER,
                        false,
                    ));
                }
            }
            RoomDrawMode::Rectangle => {
                if let Some(c1) = self.rect_corner {
                    let c1_l = self.plane.to_local(c1);
                    let pt_l = self.plane.to_local(pt);
                    let min_x = c1_l.x.min(pt_l.x);
                    let max_x = c1_l.x.max(pt_l.x);
                    let min_y = c1_l.y.min(pt_l.y);
                    let max_y = c1_l.y.max(pt_l.y);

                    let p1 = self.plane.to_world(DVec3::new(min_x, min_y, 0.0)).as_vec3().to_array();
                    let p2 = self.plane.to_world(DVec3::new(max_x, min_y, 0.0)).as_vec3().to_array();
                    let p3 = self.plane.to_world(DVec3::new(max_x, max_y, 0.0)).as_vec3().to_array();
                    let p4 = self.plane.to_world(DVec3::new(min_x, max_y, 0.0)).as_vec3().to_array();

                    wires.push(WireModel::solid(
                        "room_rect_preview".into(),
                        vec![p1, p2, p3, p4, p1],
                        WireModel::CYAN,
                        false,
                    ));
                }
            }
            _ => {}
        }
        wires
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.mode {
            RoomDrawMode::PickPoint => {
                let p2d = (pt.x, pt.y);
                CmdResult::Dispatch(format!("AEC_ROOM_PICK_DO {:.6}|{:.6}", p2d.0, p2d.1))
            }
            RoomDrawMode::Polygon => {
                self.vertices.push(pt);
                CmdResult::NeedPoint
            }
            RoomDrawMode::Rectangle => {
                if let Some(c1) = self.rect_corner {
                    let c1_l = self.plane.to_local(c1);
                    let pt_l = self.plane.to_local(pt);
                    let min_x = c1_l.x.min(pt_l.x);
                    let max_x = c1_l.x.max(pt_l.x);
                    let min_y = c1_l.y.min(pt_l.y);
                    let max_y = c1_l.y.max(pt_l.y);

                    let p1 = self.plane.to_world(DVec3::new(min_x, min_y, 0.0));
                    let p2 = self.plane.to_world(DVec3::new(max_x, min_y, 0.0));
                    let p3 = self.plane.to_world(DVec3::new(max_x, max_y, 0.0));
                    let p4 = self.plane.to_world(DVec3::new(min_x, max_y, 0.0));

                    self.vertices = vec![p1, p2, p3, p4];
                    self.rect_corner = None;

                    let pts_str = self
                        .vertices
                        .iter()
                        .map(|v| format!("{:.6},{:.6}", v.x, v.y))
                        .collect::<Vec<_>>()
                        .join(";");

                    CmdResult::Dispatch(format!("AEC_ROOM_POLYGON_DO {}", pts_str))
                } else {
                    self.rect_corner = Some(pt);
                    CmdResult::NeedPoint
                }
            }
            RoomDrawMode::SelectPolyline => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        let upper = t.to_uppercase();

        if upper == "K" || upper == "PICK" || upper == "PICKPOINT" {
            self.mode = RoomDrawMode::PickPoint;
            self.vertices.clear();
            self.rect_corner = None;
            return Some(CmdResult::NeedPoint);
        }
        if upper == "P" || upper == "POLYGON" {
            self.mode = RoomDrawMode::Polygon;
            self.vertices.clear();
            self.rect_corner = None;
            return Some(CmdResult::NeedPoint);
        }
        if upper == "R" || upper == "RECTANGLE" || upper == "RECT" {
            self.mode = RoomDrawMode::Rectangle;
            self.vertices.clear();
            self.rect_corner = None;
            return Some(CmdResult::NeedPoint);
        }
        if upper == "S" || upper == "SELECT" {
            self.mode = RoomDrawMode::SelectPolyline;
            self.vertices.clear();
            self.rect_corner = None;
            return Some(CmdResult::NeedPoint);
        }
        if (upper == "C" || upper == "CLOSE" || t.is_empty()) && self.mode == RoomDrawMode::Polygon {
            if self.vertices.len() >= 3 {
                let pts_str = self
                    .vertices
                    .iter()
                    .map(|v| format!("{:.6},{:.6}", v.x, v.y))
                    .collect::<Vec<_>>()
                    .join(";");
                return Some(CmdResult::Dispatch(format!("AEC_ROOM_POLYGON_DO {}", pts_str)));
            }
        }
        if (upper == "U" || upper == "UNDO") && !self.vertices.is_empty() {
            self.vertices.pop();
            return Some(CmdResult::NeedPoint);
        }

        Some(CmdResult::NeedPoint)
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.mode == RoomDrawMode::Polygon && self.vertices.len() >= 3 {
            let pts_str = self
                .vertices
                .iter()
                .map(|v| format!("{:.6},{:.6}", v.x, v.y))
                .collect::<Vec<_>>()
                .join(";");
            CmdResult::Dispatch(format!("AEC_ROOM_POLYGON_DO {}", pts_str))
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        if self.rect_corner.is_some() {
            self.rect_corner = None;
            CmdResult::NeedPoint
        } else if !self.vertices.is_empty() {
            self.vertices.clear();
            CmdResult::NeedPoint
        } else {
            CmdResult::Cancel
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if self.rect_corner.is_some() {
            self.rect_corner = None;
            Some(CmdResult::NeedPoint)
        } else if !self.vertices.is_empty() {
            self.vertices.pop();
            Some(CmdResult::NeedPoint)
        } else {
            None
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        true
    }

    fn needs_entity_pick(&self) -> bool {
        self.mode == RoomDrawMode::SelectPolyline
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        CmdResult::Dispatch(format!("AEC_ROOM_CONVERT_DO {}", handle.value()))
    }

    fn live_properties(&self) -> Option<LiveCommandProperties> {
        Some(LiveCommandProperties {
            title: crate::t!("Room").into_owned(),
            fields: vec![
                LiveCommandField {
                    field_id: "room_name",
                    label: crate::tr!("aec", "room-name"),
                    value: LiveFieldValue::Text(self.name.clone()),
                },
                LiveCommandField {
                    field_id: "room_number",
                    label: crate::tr!("aec", "room-number"),
                    value: LiveFieldValue::Text(self.number.clone()),
                },
                LiveCommandField {
                    field_id: "room_function",
                    label: crate::tr!("aec", "room-function"),
                    value: LiveFieldValue::Picker(self.function.display_name().to_string()),
                },
                LiveCommandField {
                    field_id: "room_clear_height",
                    label: crate::tr!("aec", "room-height"),
                    value: LiveFieldValue::Number(self.clear_height),
                },
                LiveCommandField {
                    field_id: "room_factor",
                    label: crate::tr!("aec", "room-factor"),
                    value: LiveFieldValue::Number(self.factor),
                },
                LiveCommandField {
                    field_id: "room_floor_finish",
                    label: crate::tr!("aec", "room-floor-finish"),
                    value: LiveFieldValue::Text(self.floor_finish_name.clone()),
                },
            ],
        })
    }

    fn apply_live_property(&mut self, field_id: &str, value: LiveFieldValue) -> CmdResult {
        match (field_id, value) {
            ("room_name", LiveFieldValue::Text(s)) => {
                self.name = s;
                CmdResult::NeedPoint
            }
            ("room_number", LiveFieldValue::Text(s)) => {
                self.number = s;
                CmdResult::NeedPoint
            }
            ("room_function", LiveFieldValue::Picker(s)) => {
                self.function = RoomFunction::from_str(&s);
                self.factor = self.function.default_factor();
                CmdResult::NeedPoint
            }
            ("room_clear_height", LiveFieldValue::Number(n)) => {
                self.clear_height = n;
                CmdResult::NeedPoint
            }
            ("room_factor", LiveFieldValue::Number(n)) => {
                self.factor = n;
                CmdResult::NeedPoint
            }
            ("room_floor_finish", LiveFieldValue::Text(s)) => {
                self.set_floor_finish_str(&s);
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }
}

/// Legacy one-shot `AEC_ROOM` function (auto-detect or demo room).
pub fn aec_room(scene: &mut Scene, command_line: &mut CommandLine) {
    let mut cmd = RoomCommand::new();
    let wall_segments = collect_wall_segments(&scene.document);
    let (pts, detected) = match crate::modules::aec::engine::find_closed_loop(&wall_segments, 1e-3) {
        Some(loop_pts) => (loop_pts, true),
        None => (
            vec![(0.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 3.0)],
            false,
        ),
    };

    match cmd.commit_polygon(scene, &pts) {
        Ok(handle) => {
            if detected {
                command_line.push_info(&crate::tr!(
                    "aec",
                    "room-detected",
                    name = cmd.name.as_str(),
                    count = pts.len(),
                    handle = handle.to_string()
                ));
            } else {
                command_line.push_info(&crate::tr!(
                    "aec",
                    "room-demo",
                    name = cmd.name.as_str(),
                    handle = handle.to_string()
                ));
            }
        }
        Err(err) => command_line.push_error(&format!("AEC_ROOM: {err}")),
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &[
        "AEC_ROOM",
        "AEC_ROOM_PICK",
        "AEC_ROOM_RECT",
        "AEC_ROOM_POLY",
        "AEC_ROOM_OBJECT",
        "AEC_ROOM_POLYGON_DO",
        "AEC_ROOM_PICK_DO",
        "AEC_ROOM_CONVERT_DO",
    ]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_command_creates_room_with_stamp() {
        let mut scene = Scene::default();
        let mut cmd = RoomCommand::new()
            .with_name("Wohnen & Essen")
            .with_number("EG-01")
            .with_function(RoomFunction::Living);

        let pts = vec![(0.0, 0.0), (6.0, 0.0), (6.0, 4.0), (0.0, 4.0)];
        let handle = cmd.commit_polygon(&mut scene, &pts).expect("commit room");

        let entity = scene.document.get_entity(handle).expect("carrier entity");
        let room = room_from_entity(entity).expect("room xdata");
        assert_eq!(room.name, "Wohnen & Essen");
        assert_eq!(room.number, "EG-01");
        assert_eq!(room.function, RoomFunction::Living);
        assert!((room.area - 24.0).abs() < 1e-6);
        assert!((room.perimeter - 20.0).abs() < 1e-6);
        assert!((room.volume - 60.0).abs() < 1e-6); // 24.0 * 2.50 = 60.0

        // Check stamp display child
        let children = collect_room_display_children(&scene, handle);
        assert!(!children.is_empty(), "must have stamp child entity");
        let stamp_ent = scene.document.get_entity(children[0]).expect("stamp child");
        let EntityType::MText(mtext) = stamp_ent else {
            panic!("stamp child must be MText");
        };
        assert!(mtext.value.contains("EG-01 Wohnen & Essen"));
        assert!(mtext.value.contains("F: 24.00 m²"));
        assert!(mtext.value.contains("RH: 2.50 m"));
    }

    #[test]
    fn room_with_factor_shows_living_area() {
        let mut scene = Scene::default();
        let mut cmd = RoomCommand::new()
            .with_name("Balkon")
            .with_number("OG-05")
            .with_function(RoomFunction::Balcony);

        let pts = vec![(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (0.0, 2.0)];
        let handle = cmd.commit_polygon(&mut scene, &pts).expect("commit balcony");

        let entity = scene.document.get_entity(handle).expect("carrier entity");
        let room = room_from_entity(entity).expect("room xdata");
        assert_eq!(room.factor, 0.5);
        assert!((room.area - 8.0).abs() < 1e-6);
        assert!((room.calculated_area() - 4.0).abs() < 1e-6);

        let children = collect_room_display_children(&scene, handle);
        let stamp_ent = scene.document.get_entity(children[0]).expect("stamp child");
        let EntityType::MText(mtext) = stamp_ent else {
            panic!("stamp child must be MText");
        };
        assert!(mtext.value.contains("WF: 4.00 m² / 50%"));
    }

    #[test]
    fn room_vertex_edit_dynamically_recalculates_area_and_regen_updates_stamp_and_hatch() {
        let mut scene = Scene::default();
        let mut cmd = RoomCommand::new()
            .with_name("Zimmer")
            .with_number("EG-02")
            .with_function(RoomFunction::Living);

        // Initial 4x4 room (16 m²)
        let pts = vec![(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)];
        let handle = cmd.commit_polygon(&mut scene, &pts).expect("commit room");

        let initial_children = collect_room_display_children(&scene, handle);
        assert!(!initial_children.is_empty());

        // Simulate vertex move: extend from 4x4 to 5x4 (20 m²)
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity_mut(handle) {
            pl.vertices[1].location.x = 5.0;
            pl.vertices[2].location.x = 5.0;
        }

        // 1. Check that room_from_entity immediately calculates the updated area (20 m²)
        let ent = scene.document.get_entity(handle).unwrap();
        let dynamic_room = room_from_entity(ent).expect("room");
        assert!((dynamic_room.area - 20.0).abs() < 1e-4);
        assert!((dynamic_room.perimeter - 18.0).abs() < 1e-4);

        // 2. Regenerate room representation (as done after grip edits)
        regenerate_room_representation(&mut scene, handle, None);

        // Old children must have been replaced
        let new_children = collect_room_display_children(&scene, handle);
        for old_c in &initial_children {
            assert!(
                !new_children.contains(old_c),
                "old children must be erased on regen"
            );
            assert!(
                scene.document.get_entity(*old_c).is_none(),
                "old child must be removed from document"
            );
        }

        // New stamp must reflect the updated 20.00 m²
        let stamp_h = new_children
            .iter()
            .copied()
            .find(|h| matches!(scene.document.get_entity(*h), Some(EntityType::MText(_))))
            .expect("stamp entity");
        let EntityType::MText(mtext) = scene.document.get_entity(stamp_h).unwrap() else {
            panic!("expected mtext");
        };
        assert!(mtext.value.contains("F: 20.00 m²"));
        assert!(mtext.value.contains("U: 18.00 m"));
    }

    #[test]
    fn test_room_tools_and_modes() {
        assert_eq!(tool().id, "AEC_ROOM");
        assert_eq!(tool_pick().id, "AEC_ROOM_PICK");
        assert_eq!(tool_rect().id, "AEC_ROOM_RECT");
        assert_eq!(tool_poly().id, "AEC_ROOM_POLY");
        assert_eq!(tool_object().id, "AEC_ROOM_OBJECT");

        let cmd_rect = RoomCommand::new().with_mode(RoomDrawMode::Rectangle);
        assert_eq!(cmd_rect.mode, RoomDrawMode::Rectangle);
        let cmd_poly = RoomCommand::new().with_mode(RoomDrawMode::Polygon);
        assert_eq!(cmd_poly.mode, RoomDrawMode::Polygon);
    }

    #[test]
    fn test_auto_detect_room_structural_inner_boundary() {
        let mut scene = Scene::default();
        let wall_segments = vec![
            ((0.0, 0.0), (10.0, 0.0)),
            ((10.0, 0.0), (10.0, 6.0)),
            ((10.0, 6.0), (0.0, 6.0)),
            ((0.0, 6.0), (0.0, 0.0)),
        ];
        for (a, b) in wall_segments {
            let mut pl = LwPolyline::new();
            pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
            pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
            let wall_h = scene.add_entity(EntityType::LwPolyline(pl));
            let wall = crate::modules::aec::engine::wall::Wall::new("style_wall_24", 2.5, 0);
            let mut record = ExtendedDataRecord::new(AEC_APPID);
            record.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
            write_aec_record(&mut scene.document, wall_h, record);
        }

        let mut cmd = RoomCommand::new()
            .with_name("Wohnzimmer")
            .with_number("EG-01");

        let handle = cmd.auto_detect_at_point(&mut scene, (5.0, 3.0)).expect("auto detect room");
        let ent = scene.document.get_entity(handle).unwrap();
        let room = room_from_entity(ent).expect("room");
        assert!(
            (room.area - 56.2176).abs() < 1e-2,
            "Expected ~56.22 m² inner structural area, got {}",
            room.area
        );
    }
}
