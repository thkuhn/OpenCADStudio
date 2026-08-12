//! `Storey` domain model.
//!
//! Mirrors the `STOREY` XDATA record (APPID `OPENCAD_AEC`): name, reference
//! elevation and floor-to-floor height. The `storey_id` used to cross-reference
//! `Wall`/`Room` records is kept out of this struct's fields (it is the
//! record's own key/index) but callers commonly pair a `Storey` with its
//! `u32` id, e.g. `(u32, Storey)`.

/// A building storey (floor level).
#[derive(Debug, Clone, PartialEq)]
pub struct Storey {
    /// Storey name, e.g. `"Level 1"`.
    pub name: String,
    /// Reference elevation above the project origin, drawing units.
    pub elevation: f64,
    /// Floor-to-floor height, drawing units.
    pub height: f64,
}

impl Storey {
    /// Creates a new storey.
    pub fn new(name: impl Into<String>, elevation: f64, height: f64) -> Self {
        Self {
            name: name.into(),
            elevation,
            height,
        }
    }

    /// Elevation of the storey directly above this one, assuming stacking
    /// with no gaps: `elevation + height`.
    pub fn top_elevation(&self) -> f64 {
        self.elevation + self.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_elevation_adds_height_to_elevation() {
        let storey = Storey::new("Level 1", 0.0, 3.2);
        assert_eq!(storey.top_elevation(), 3.2);
    }

    #[test]
    fn stacked_storeys_have_contiguous_elevations() {
        let ground = Storey::new("Level 1", 0.0, 3.2);
        let first = Storey::new("Level 2", ground.top_elevation(), 2.8);
        assert_eq!(first.elevation, 3.2);
        assert_eq!(first.top_elevation(), 6.0);
    }
}
