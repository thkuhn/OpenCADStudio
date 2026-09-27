//! Associative slab openings (through-holes and recesses).
//!
//! A [`SlabOpening`] references its host [`Slab`], carries a 2D boundary polygon,
//! a functional opening kind (e.g. Stairwell, Shaft, Duct, Skylight), and a depth
//! specification (ThroughHole or Recess). It provides DIN 1356 2D diagonal cross
//! symbol geometry for floor plans.

use acadrust::Handle;
use serde::{Deserialize, Serialize};

use super::geometry::{area, perimeter};

/// Functional classification of a slab opening.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SlabOpeningKind {
    /// Stairwell cutout / stair eye (Treppenöffnung / Treppenauge).
    #[default]
    Stairwell,
    /// Vertical technical service shaft (Installationsschacht / Versorgungsschacht).
    Shaft,
    /// Ventilation or duct breakthrough (Lüftungskanal / Leitungsdurchbruch).
    Duct,
    /// Chimney cutout (Kamin / Schornstein).
    Chimney,
    /// Roof skylight or horizontal daylight opening (Oberlicht / Deckenaussparung).
    Skylight,
    /// Custom opening geometry.
    Custom,
}

impl SlabOpeningKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SlabOpeningKind::Stairwell => "Stairwell",
            SlabOpeningKind::Shaft => "Shaft",
            SlabOpeningKind::Duct => "Duct",
            SlabOpeningKind::Chimney => "Chimney",
            SlabOpeningKind::Skylight => "Skylight",
            SlabOpeningKind::Custom => "Custom",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "Stairwell" | "stairwell" | "STAIRWELL" | "Treppe" | "treppe" => SlabOpeningKind::Stairwell,
            "Shaft" | "shaft" | "SHAFT" | "Schacht" | "schacht" => SlabOpeningKind::Shaft,
            "Duct" | "duct" | "DUCT" | "Kanal" | "kanal" => SlabOpeningKind::Duct,
            "Chimney" | "chimney" | "CHIMNEY" | "Kamin" | "kamin" => SlabOpeningKind::Chimney,
            "Skylight" | "skylight" | "SKYLIGHT" | "Oberlicht" | "oberlicht" => SlabOpeningKind::Skylight,
            _ => SlabOpeningKind::Custom,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            SlabOpeningKind::Stairwell => "Treppenöffnung",
            SlabOpeningKind::Shaft => "Installationsschacht",
            SlabOpeningKind::Duct => "Lüftungsdurchbruch",
            SlabOpeningKind::Chimney => "Kamindurchbruch",
            SlabOpeningKind::Skylight => "Oberlicht",
            SlabOpeningKind::Custom => "Benutzerdefiniert",
        }
    }

    pub fn all() -> &'static [SlabOpeningKind] {
        &[
            SlabOpeningKind::Stairwell,
            SlabOpeningKind::Shaft,
            SlabOpeningKind::Duct,
            SlabOpeningKind::Chimney,
            SlabOpeningKind::Skylight,
            SlabOpeningKind::Custom,
        ]
    }
}

/// Depth specification for a slab opening.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SlabOpeningDepth {
    /// Full cut through all slab layers.
    ThroughHole,
    /// Partial-depth depression / recess in meters (measured from slab top surface downward).
    Recess(f64),
}

impl Default for SlabOpeningDepth {
    fn default() -> Self {
        SlabOpeningDepth::ThroughHole
    }
}

impl SlabOpeningDepth {
    pub fn as_str(self) -> &'static str {
        match self {
            SlabOpeningDepth::ThroughHole => "ThroughHole",
            SlabOpeningDepth::Recess(_) => "Recess",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            SlabOpeningDepth::ThroughHole => "Through hole",
            SlabOpeningDepth::Recess(_) => "Recess",
        }
    }

    pub fn depth_value(self) -> Option<f64> {
        match self {
            SlabOpeningDepth::ThroughHole => None,
            SlabOpeningDepth::Recess(d) => Some(d),
        }
    }

    pub fn is_through_hole(self) -> bool {
        matches!(self, SlabOpeningDepth::ThroughHole)
    }

    pub fn from_mode_and_depth(mode: &str, depth: f64) -> Self {
        match mode {
            "Recess" | "recess" | "RECESS" | "Aussparung" | "Niche" | "niche" => {
                SlabOpeningDepth::Recess(depth.max(0.0))
            }
            _ => SlabOpeningDepth::ThroughHole,
        }
    }

    pub fn all_modes() -> &'static [&'static str] {
        &["ThroughHole", "Recess"]
    }
}

/// Parametric slab opening domain entity.
#[derive(Debug, Clone, PartialEq)]
pub struct SlabOpening {
    /// Entity handle of the host [`Slab`].
    pub host_slab: Handle,
    /// Functional opening kind.
    pub kind: SlabOpeningKind,
    /// Depth mode and value.
    pub depth: SlabOpeningDepth,
    /// 2D boundary polygon (implicitly closed, first point not repeated).
    pub boundary: Vec<(f64, f64)>,
    /// Handles of 2D/3D derived child entities.
    pub derived_handles: Vec<Handle>,
}

impl SlabOpening {
    pub fn new(host_slab: Handle, kind: SlabOpeningKind, boundary: Vec<(f64, f64)>) -> Self {
        Self {
            host_slab,
            kind,
            depth: SlabOpeningDepth::ThroughHole,
            boundary,
            derived_handles: Vec::new(),
        }
    }

    pub fn new_through_hole(
        host_slab: Handle,
        kind: SlabOpeningKind,
        boundary: Vec<(f64, f64)>,
    ) -> Self {
        Self {
            host_slab,
            kind,
            depth: SlabOpeningDepth::ThroughHole,
            boundary,
            derived_handles: Vec::new(),
        }
    }

    pub fn new_recess(
        host_slab: Handle,
        kind: SlabOpeningKind,
        depth: f64,
        boundary: Vec<(f64, f64)>,
    ) -> Self {
        Self {
            host_slab,
            kind,
            depth: SlabOpeningDepth::Recess(depth),
            boundary,
            derived_handles: Vec::new(),
        }
    }

    /// Creates a rectangular slab opening centered at `center = (cx, cy)`.
    pub fn new_rectangle(
        host_slab: Handle,
        kind: SlabOpeningKind,
        depth: SlabOpeningDepth,
        center: (f64, f64),
        width: f64,
        height: f64,
        angle_rad: f64,
    ) -> Self {
        let hw = width * 0.5;
        let hh = height * 0.5;
        let cos_a = angle_rad.cos();
        let sin_a = angle_rad.sin();

        let corners = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)];
        let boundary: Vec<(f64, f64)> = corners
            .iter()
            .map(|&(dx, dy)| {
                (
                    center.0 + dx * cos_a - dy * sin_a,
                    center.1 + dx * sin_a + dy * cos_a,
                )
            })
            .collect();

        Self {
            host_slab,
            kind,
            depth,
            boundary,
            derived_handles: Vec::new(),
        }
    }

    /// Creates an approximate circular opening polygon centered at `center`.
    pub fn new_circle(
        host_slab: Handle,
        kind: SlabOpeningKind,
        depth: SlabOpeningDepth,
        center: (f64, f64),
        radius: f64,
        segments: usize,
    ) -> Self {
        let n = segments.max(8);
        let mut boundary = Vec::with_capacity(n);
        let step = std::f64::consts::TAU / (n as f64);
        for i in 0..n {
            let theta = (i as f64) * step;
            boundary.push((center.0 + radius * theta.cos(), center.1 + radius * theta.sin()));
        }
        Self {
            host_slab,
            kind,
            depth,
            boundary,
            derived_handles: Vec::new(),
        }
    }

    /// 2D area of this opening cutout in square meters.
    pub fn area(&self) -> f64 {
        area(&self.boundary)
    }

    /// 2D perimeter of this opening boundary in meters.
    pub fn perimeter(&self) -> f64 {
        perimeter(&self.boundary)
    }

    /// Axis-aligned 2D bounding box `(min_x, min_y, max_x, max_y)`.
    pub fn bounding_box(&self) -> (f64, f64, f64, f64) {
        if self.boundary.is_empty() {
            return (0.0, 0.0, 0.0, 0.0);
        }
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for &(x, y) in &self.boundary {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        (min_x, min_y, max_x, max_y)
    }

    /// Arithmetic centroid `(cx, cy)` of the boundary vertices.
    pub fn centroid(&self) -> (f64, f64) {
        if self.boundary.is_empty() {
            return (0.0, 0.0);
        }
        let n = self.boundary.len() as f64;
        let sum_x: f64 = self.boundary.iter().map(|p| p.0).sum();
        let sum_y: f64 = self.boundary.iter().map(|p| p.1).sum();
        (sum_x / n, sum_y / n)
    }

    /// Generates DIN 1356 standard opening symbol lines (diagonal cross "X")
    /// for 2D floor plans.
    ///
    /// For a 4-vertex quad (rectangle/parallelogram): returns diagonals `(p0 -> p2)` and `(p1 -> p3)`.
    /// For general polygons: returns diagonal lines spanning the bounding box.
    pub fn din_1356_cross_lines(&self) -> Vec<((f64, f64), (f64, f64))> {
        if self.boundary.len() == 4 {
            vec![
                (self.boundary[0], self.boundary[2]),
                (self.boundary[1], self.boundary[3]),
            ]
        } else if self.boundary.len() >= 3 {
            let (min_x, min_y, max_x, max_y) = self.bounding_box();
            vec![
                ((min_x, min_y), (max_x, max_y)),
                ((min_x, max_y), (max_x, min_y)),
            ]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slab_opening_creation_and_bounds() {
        let opening = SlabOpening::new_rectangle(
            Handle::from(42u64),
            SlabOpeningKind::Stairwell,
            SlabOpeningDepth::ThroughHole,
            (5.0, 5.0),
            2.0,
            3.0,
            0.0,
        );

        assert_eq!(opening.host_slab, Handle::from(42u64));
        assert_eq!(opening.kind, SlabOpeningKind::Stairwell);
        assert!(opening.depth.is_through_hole());
        assert!((opening.area() - 6.0).abs() < 1e-6);
        assert!((opening.perimeter() - 10.0).abs() < 1e-6);

        let bbox = opening.bounding_box();
        assert!((bbox.0 - 4.0).abs() < 1e-6);
        assert!((bbox.1 - 3.5).abs() < 1e-6);
        assert!((bbox.2 - 6.0).abs() < 1e-6);
        assert!((bbox.3 - 6.5).abs() < 1e-6);

        let cross = opening.din_1356_cross_lines();
        assert_eq!(cross.len(), 2);
    }

    #[test]
    fn test_slab_opening_recess() {
        let opening = SlabOpening::new_recess(
            Handle::from(100u64),
            SlabOpeningKind::Duct,
            0.08,
            vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
        );

        assert_eq!(opening.depth.depth_value(), Some(0.08));
        assert!(!opening.depth.is_through_hole());
    }

    #[test]
    fn test_slab_opening_circle() {
        let circle = SlabOpening::new_circle(
            Handle::from(10u64),
            SlabOpeningKind::Shaft,
            SlabOpeningDepth::ThroughHole,
            (0.0, 0.0),
            1.0,
            32,
        );

        assert_eq!(circle.boundary.len(), 32);
        // Area of unit circle ~ pi ~ 3.14159...
        assert!((circle.area() - std::f64::consts::PI).abs() < 0.05);
    }
}
