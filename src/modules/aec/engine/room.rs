//! `Room` domain model.
//!
//! Mirrors the `ROOM` XDATA record (APPID `OPENCAD_AEC`): name, area,
//! perimeter, volume and the owning storey id. `area`/`perimeter`/`volume`
//! are derived from a closed floor polygon via the shoelace-formula helpers
//! in `geometry.rs`.

use super::geometry;

/// A room, computed from a closed loop of wall polylines.
#[derive(Debug, Clone, PartialEq)]
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
}
