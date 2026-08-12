//! `Wall` domain model.
//!
//! Mirrors the `WALL` XDATA record (APPID `OPENCAD_AEC`): thickness, height,
//! an optional material reference and the owning storey id.

/// A parametric wall: a baseline polyline (owned by the host entity, not by
/// this struct) plus the metadata needed to extrude/render it and to write
/// the `WALL` XDATA record.
#[derive(Debug, Clone, PartialEq)]
pub struct Wall {
    /// Wall thickness, in drawing units.
    pub thickness: f64,
    /// Wall height, in drawing units.
    pub height: f64,
    /// Optional material name/id; `None` mirrors an empty string in the
    /// XDATA record.
    pub material_ref: Option<String>,
    /// Index of the owning `Storey` (see `storey.rs`).
    pub storey_id: u32,
}

impl Wall {
    /// Creates a new wall with no material assigned.
    pub fn new(thickness: f64, height: f64, storey_id: u32) -> Self {
        Self {
            thickness,
            height,
            material_ref: None,
            storey_id,
        }
    }

    /// Volume of the wall given the length of its (2D) baseline centerline.
    /// `volume = length * thickness * height`.
    pub fn volume(&self, baseline_length: f64) -> f64 {
        baseline_length * self.thickness * self.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_is_length_times_thickness_times_height() {
        let wall = Wall::new(0.2, 2.8, 0);
        assert!((wall.volume(5.0) - 2.8).abs() < 1e-9);
    }

    #[test]
    fn new_wall_has_no_material() {
        let wall = Wall::new(0.2, 2.8, 1);
        assert_eq!(wall.material_ref, None);
        assert_eq!(wall.storey_id, 1);
    }
}
