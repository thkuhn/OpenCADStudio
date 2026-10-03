//! `Room` domain model.
//!
//! Mirrors the `ROOM` XDATA record (APPID `OPENCAD_AEC`): name, area,
//! perimeter, volume and the owning storey id. `area`/`perimeter`/`volume`
//! are derived from a closed floor polygon via the shoelace-formula helpers
//! in `geometry.rs`.

use serde::{Deserialize, Serialize};

use super::geometry;
use super::plan_view::PlanPhase;

/// DIN 277 / WoFlV room usage category and function classification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RoomFunction {
    /// DIN 277 NUF 1: Wohnen und Aufenthalt
    Living,
    /// DIN 277 NUF 2: Büroarbeit
    Office,
    /// DIN 277 NUF 7: Sanitärräume
    Sanitary,
    /// DIN 277 NUF 3: Kochen / Speiseräume
    Kitchen,
    /// DIN 277 VF: Verkehrsfläche / Flur / Treppenraum
    Corridor,
    /// DIN 277 NUF 4: Lagern / Abstellen
    Storage,
    /// DIN 277 TF: Technische Funktionsfläche
    Technical,
    /// DIN 277 NUF 1 / WoFlV (50% Anrechnung): Balkon / Loggia / Terrasse
    Balcony,
    /// User-defined function
    Custom(String),
}

impl Default for RoomFunction {
    fn default() -> Self {
        Self::Living
    }
}

impl RoomFunction {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Living => "Living",
            Self::Office => "Office",
            Self::Sanitary => "Sanitary",
            Self::Kitchen => "Kitchen",
            Self::Corridor => "Corridor",
            Self::Storage => "Storage",
            Self::Technical => "Technical",
            Self::Balcony => "Balcony",
            Self::Custom(s) => s.as_str(),
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "living" | "wohnen" | "aufenthalt" | "schlafen" | "kind" | "zimmer" => Self::Living,
            "office" | "büro" | "arbeitszimmer" | "arbeiten" => Self::Office,
            "sanitary" | "bad" | "wc" | "sanitär" | "dusche" => Self::Sanitary,
            "kitchen" | "küche" | "kochen" | "speisekammer" => Self::Kitchen,
            "corridor" | "flur" | "diele" | "gang" | "verkehr" => Self::Corridor,
            "storage" | "abstell" | "lager" | "keller" | "abstellraum" => Self::Storage,
            "technical" | "technik" | "hwr" | "har" | "heizung" => Self::Technical,
            "balcony" | "balkon" | "terrasse" | "loggia" | "dachgarten" => Self::Balcony,
            other if !other.is_empty() => Self::Custom(s.trim().to_string()),
            _ => Self::Living,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Living => "Wohnen / Aufenthalt",
            Self::Office => "Büroarbeit",
            Self::Sanitary => "Sanitärraum",
            Self::Kitchen => "Küche",
            Self::Corridor => "Flur / Verkehrsfläche",
            Self::Storage => "Lagern / Abstellen",
            Self::Technical => "Technikfläche",
            Self::Balcony => "Balkon / Terrasse",
            Self::Custom(_) => "Sonstige Nutzung",
        }
    }

    pub fn din277_code(&self) -> &'static str {
        match self {
            Self::Living => "NUF 1",
            Self::Office => "NUF 2",
            Self::Sanitary => "NUF 7",
            Self::Kitchen => "NUF 3",
            Self::Corridor => "VF",
            Self::Storage => "NUF 4",
            Self::Technical => "TF",
            Self::Balcony => "NUF 1 (50%)",
            Self::Custom(_) => "NUF",
        }
    }

    pub fn default_factor(&self) -> f64 {
        match self {
            Self::Balcony => 0.50,
            _ => 1.0,
        }
    }
}

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
    /// Room name (e.g. "Wohnen & Essen", "Küche", "Bad", "Büro 101").
    pub name: String,
    /// Room number / identifier (e.g. "01", "EG-01", "1.02").
    #[serde(default)]
    pub number: String,
    /// Room usage category / DIN 277 function.
    #[serde(default)]
    pub function: RoomFunction,
    /// Measured gross floor area, drawing units squared (m²).
    pub area: f64,
    /// Calculation factor (e.g. 1.0 = 100%, 0.5 = 50% for balcony/terrace).
    #[serde(default = "default_room_factor")]
    pub factor: f64,
    /// Boundary perimeter, drawing units (m).
    pub perimeter: f64,
    /// Clear room height (lichte Raumhöhe), drawing units (m).
    #[serde(default = "default_room_height")]
    pub clear_height: f64,
    /// Net room volume (`calculated_area * clear_height`), drawing units cubed (m³).
    pub volume: f64,
    /// Index of the owning `Storey`.
    pub storey_id: u32,
    /// Base level elevation (Oberkante Rohdecke / OKRD).
    #[serde(default)]
    pub base_z: f64,
    /// Construction phase (Existing, Demolition, New).
    #[serde(default)]
    pub phase: PlanPhase,
    /// Interactive 2D placement coordinate for the room stamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stamp_pos: Option<(f64, f64)>,
    /// Optional room-specific floor finish layer stack.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_finish: Option<Vec<RoomFinish>>,
}

fn default_room_factor() -> f64 {
    1.0
}

fn default_room_height() -> f64 {
    2.50
}

impl Room {
    /// Builds a `Room` from a closed floor polygon (implicitly closed, do
    /// not repeat the first point) and the clear storey height used for the
    /// volume computation.
    pub fn from_polygon(
        name: impl Into<String>,
        points: &[(f64, f64)],
        storey_height: f64,
        storey_id: u32,
    ) -> Self {
        let area = geometry::area(points);
        let perimeter = geometry::perimeter(points);
        let height = if storey_height > 0.0 { storey_height } else { 2.50 };
        let volume = area * height;
        Self {
            name: name.into(),
            number: String::new(),
            function: RoomFunction::Living,
            area,
            factor: 1.0,
            perimeter,
            clear_height: height,
            volume,
            storey_id,
            base_z: 0.0,
            phase: PlanPhase::New,
            stamp_pos: if points.is_empty() { None } else { Some(geometry::centroid(points)) },
            floor_finish: None,
        }
    }

    /// Calculated net room area (DIN 277 / WoFlV) taking the factor into account.
    pub fn calculated_area(&self) -> f64 {
        self.area * self.factor
    }

    /// Evaluates effective net room volume.
    pub fn effective_volume(&self) -> f64 {
        self.calculated_area() * self.clear_height
    }

    pub fn with_number(mut self, number: impl Into<String>) -> Self {
        self.number = number.into();
        self
    }

    pub fn with_function(mut self, function: RoomFunction) -> Self {
        self.factor = function.default_factor();
        self.function = function;
        self.volume = self.effective_volume();
        self
    }

    pub fn with_factor(mut self, factor: f64) -> Self {
        self.factor = factor;
        self.volume = self.effective_volume();
        self
    }

    pub fn with_clear_height(mut self, height: f64) -> Self {
        self.clear_height = height;
        self.volume = self.effective_volume();
        self
    }

    pub fn with_base_z(mut self, base_z: f64) -> Self {
        self.base_z = base_z;
        self
    }

    pub fn with_phase(mut self, phase: PlanPhase) -> Self {
        self.phase = phase;
        self
    }

    pub fn with_stamp_pos(mut self, pos: (f64, f64)) -> Self {
        self.stamp_pos = Some(pos);
        self
    }

    /// Sets custom room-specific floor finishes.
    pub fn with_floor_finish(mut self, finishes: Vec<RoomFinish>) -> Self {
        self.floor_finish = Some(finishes);
        self
    }

    /// Total thickness of floor finishes in meters.
    pub fn floor_finish_thickness(&self) -> f64 {
        self.floor_finish
            .as_ref()
            .map(|f| f.iter().map(|l| l.thickness).sum())
            .unwrap_or(0.0)
    }

    /// Concise summary string of floor finishes (e.g. "Parkett", "Fliesen").
    pub fn floor_finish_summary(&self) -> String {
        match &self.floor_finish {
            Some(finishes) if !finishes.is_empty() => {
                let mats: Vec<&str> = finishes.iter().map(|f| f.material.as_str()).collect();
                mats.join(", ")
            }
            _ => "-".to_string(),
        }
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
