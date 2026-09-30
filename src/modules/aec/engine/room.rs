//! `Room` domain model.
//!
//! Mirrors the `ROOM` XDATA record (APPID `OPENCAD_AEC`): name, area,
//! perimeter, volume and the owning storey id. `area`/`perimeter`/`volume`
//! are derived from a closed floor polygon via the shoelace-formula helpers
//! in `geometry.rs`.

use serde::{Deserialize, Serialize};

use super::geometry;

/// Individual finish layer within a room floor build-up (e.g. Impact Sound Insulation, Screed, Parquet, Tiles).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoomFinish {
    /// Material name or material library ID.
    pub material: String,
    /// Thickness in meters (drawing units).
    pub thickness: f64,
    /// Vertical offset above base level (OKRD) in meters.
    pub vertical_offset: f64,
    /// Optional material hatch pattern override (e.g. "AR-CONC", "ANSI31", "SQUARE").
    pub hatch_pattern: Option<String>,
}

impl RoomFinish {
    pub fn new(material: impl Into<String>, thickness: f64) -> Self {
        Self {
            material: material.into(),
            thickness,
            vertical_offset: 0.0,
            hatch_pattern: None,
        }
    }

    pub fn with_offset(mut self, offset: f64) -> Self {
        self.vertical_offset = offset;
        self
    }

    pub fn with_hatch(mut self, pattern: impl Into<String>) -> Self {
        self.hatch_pattern = Some(pattern.into());
        self
    }
}

/// Room-specific floor finish override (Fußbodenaufbau) defined for a closed room perimeter.
///
/// Replaces or specifies the floor build-up from Oberkante Rohdecke (OKRD) up to
/// Oberkante Fertigfußboden (OKFF) on top of the structural slab core.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FloorFinishOverride {
    /// Room identifier or name.
    pub room_name: String,
    /// 2D boundary polygon of the room perimeter (inner wall face).
    pub boundary: Vec<(f64, f64)>,
    /// Floor finish layer stack (ordered from bottom to top).
    pub finishes: Vec<RoomFinish>,
    /// Base level elevation (Oberkante Rohdecke / OKRD).
    pub base_z: f64,
}

impl FloorFinishOverride {
    pub fn new(
        room_name: impl Into<String>,
        boundary: Vec<(f64, f64)>,
        base_z: f64,
    ) -> Self {
        Self {
            room_name: room_name.into(),
            boundary,
            finishes: Vec::new(),
            base_z,
        }
    }

    pub fn with_finishes(mut self, finishes: Vec<RoomFinish>) -> Self {
        self.finishes = finishes;
        self
    }

    /// Total thickness of the floor build-up in meters.
    pub fn total_thickness(&self) -> f64 {
        self.finishes.iter().map(|f| f.thickness).sum()
    }

    /// Resulting finished floor level (Oberkante Fertigfußboden / OKFF).
    pub fn top_z(&self) -> f64 {
        self.base_z + self.total_thickness()
    }

    /// Evaluates finished floor level (OKFF) at a given (x, y) point on the slab.
    pub fn okff_at_xy(&self, _x: f64, _y: f64) -> f64 {
        self.top_z()
    }

    /// Base level (Oberkante Rohdecke / OKRD) for the structural slab core and walls standing on OKRD.
    pub fn okrd_at_xy(&self, _x: f64, _y: f64) -> f64 {
        self.base_z
    }
}

/// A room, computed from a closed loop of wall polylines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Room {
    /// Room name/number.
    pub name: String,
    /// Floor area, drawing units squared.
    pub area: f64,
    /// Boundary perimeter, drawing units.
    pub perimeter: f64,
    /// `area * storey height`, drawing units cubed.
    pub volume: f64,
    /// Index of the owning `Storey`.
    pub storey_id: u32,
    /// Optional room-specific floor finish layer stack.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_finish: Option<Vec<RoomFinish>>,
}

impl Room {
    /// Builds a `Room` from a closed floor polygon (implicitly closed, do
    /// not repeat the first point) and the storey height used for the
    /// volume computation.
    pub fn from_polygon(
        name: impl Into<String>,
        points: &[(f64, f64)],
        storey_height: f64,
        storey_id: u32,
    ) -> Self {
        Self {
            name: name.into(),
            area: geometry::area(points),
            perimeter: geometry::perimeter(points),
            volume: geometry::volume(points, storey_height),
            storey_id,
            floor_finish: None,
        }
    }

    /// Sets custom room-specific floor finishes.
    pub fn with_floor_finish(mut self, finishes: Vec<RoomFinish>) -> Self {
        self.floor_finish = Some(finishes);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_rectangle_polygon_computes_area_perimeter_volume() {
        let pts = [(0.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 3.0)];
        let room = Room::from_polygon("Office 101", &pts, 2.5, 0);

        assert_eq!(room.name, "Office 101");
        assert_eq!(room.area, 12.0);
        assert_eq!(room.perimeter, 14.0);
        assert_eq!(room.volume, 30.0);
        assert_eq!(room.storey_id, 0);
    }

    #[test]
    fn from_l_shape_polygon_computes_area_perimeter_volume() {
        let pts = [
            (0.0, 0.0),
            (4.0, 0.0),
            (4.0, 2.0),
            (2.0, 2.0),
            (2.0, 4.0),
            (0.0, 4.0),
        ];
        let room = Room::from_polygon("Lobby", &pts, 3.0, 2);

        assert_eq!(room.area, 12.0);
        assert_eq!(room.perimeter, 16.0);
        assert_eq!(room.volume, 36.0);
        assert_eq!(room.storey_id, 2);
    }

    #[test]
    fn test_floor_finish_override_and_levels() {
        let boundary = vec![(0.0, 0.0), (5.0, 0.0), (5.0, 4.0), (0.0, 4.0)];
        let okrd = 2.80; // Oberkante Rohdecke

        let finish = FloorFinishOverride::new("Bathroom", boundary, okrd).with_finishes(vec![
            RoomFinish::new("Impact Sound Insulation", 0.04),
            RoomFinish::new("Screed", 0.06),
            RoomFinish::new("Tiles", 0.02).with_hatch("SQUARE"),
        ]);

        assert_eq!(finish.room_name, "Bathroom");
        assert!((finish.total_thickness() - 0.12).abs() < 1e-6);
        assert!((finish.okrd_at_xy(1.0, 1.0) - 2.80).abs() < 1e-6);
        assert!((finish.okff_at_xy(1.0, 1.0) - 2.92).abs() < 1e-6);
        assert_eq!(finish.finishes[2].hatch_pattern.as_deref(), Some("SQUARE"));
    }
}
