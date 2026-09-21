//! Opening elevation shapes (plan bounding box vs. true elevation profile).
//!
//! Plan-view host cuts always use the instance bounding width. The shape
//! lives in the elevation plane `(s, z)`: `s` along the wall axis from the
//! left jamb (`0..width`), `z` up from the sill (`0..height`).

use serde::{Deserialize, Serialize};

/// Number of chords used to approximate a full circle.
pub const CIRCLE_CHORD_COUNT: usize = 32;
/// Number of chords used to approximate an arch (circular segment).
pub const ARCH_CHORD_COUNT: usize = 16;

/// Variants of a triangular opening.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TriangleVariant {
    /// Isosceles triangle with apex at the head.
    #[default]
    IsoscelesUp,
    /// Isosceles triangle with apex at the sill.
    IsoscelesDown,
    /// Equilateral triangle (height derived from width).
    Equilateral,
    /// Right triangle with the right angle on the left jamb.
    RightLeft,
    /// Right triangle with the right angle on the right jamb.
    RightRight,
}

impl TriangleVariant {
    pub fn as_str(self) -> &'static str {
        match self {
            TriangleVariant::IsoscelesUp => "IsoscelesUp",
            TriangleVariant::IsoscelesDown => "IsoscelesDown",
            TriangleVariant::Equilateral => "Equilateral",
            TriangleVariant::RightLeft => "RightLeft",
            TriangleVariant::RightRight => "RightRight",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "IsoscelesUp" | "isosceles_up" => Some(TriangleVariant::IsoscelesUp),
            "IsoscelesDown" | "isosceles_down" => Some(TriangleVariant::IsoscelesDown),
            "Equilateral" | "equilateral" => Some(TriangleVariant::Equilateral),
            "RightLeft" | "right_left" => Some(TriangleVariant::RightLeft),
            "RightRight" | "right_right" => Some(TriangleVariant::RightRight),
            _ => None,
        }
    }
}

/// Elevation outline of an opening, independent of [`super::openings::OpeningKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum OpeningShape {
    #[default]
    Rectangle,
    /// `width == height == diameter`.
    Circle,
    Triangle(TriangleVariant),
    /// Rectangular legs up to `spring_height`, then a circular segment to `height`.
    Arch,
}

impl OpeningShape {
    pub fn catalogue() -> &'static [OpeningShape] {
        &[
            OpeningShape::Rectangle,
            OpeningShape::Circle,
            OpeningShape::Triangle(TriangleVariant::IsoscelesUp),
            OpeningShape::Triangle(TriangleVariant::IsoscelesDown),
            OpeningShape::Triangle(TriangleVariant::Equilateral),
            OpeningShape::Triangle(TriangleVariant::RightLeft),
            OpeningShape::Triangle(TriangleVariant::RightRight),
            OpeningShape::Arch,
        ]
    }

    pub fn as_str(self) -> &'static str {
        match self {
            OpeningShape::Rectangle => "Rectangle",
            OpeningShape::Circle => "Circle",
            OpeningShape::Triangle(v) => match v {
                TriangleVariant::IsoscelesUp => "Triangle:IsoscelesUp",
                TriangleVariant::IsoscelesDown => "Triangle:IsoscelesDown",
                TriangleVariant::Equilateral => "Triangle:Equilateral",
                TriangleVariant::RightLeft => "Triangle:RightLeft",
                TriangleVariant::RightRight => "Triangle:RightRight",
            },
            OpeningShape::Arch => "Arch",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "Circle" | "circle" => OpeningShape::Circle,
            "Arch" | "arch" => OpeningShape::Arch,
            other if other.starts_with("Triangle:") => {
                let variant = other.strip_prefix("Triangle:").unwrap_or("");
                OpeningShape::Triangle(
                    TriangleVariant::from_str(variant).unwrap_or(TriangleVariant::IsoscelesUp),
                )
            }
            "Triangle" | "triangle" => OpeningShape::Triangle(TriangleVariant::IsoscelesUp),
            _ => OpeningShape::Rectangle,
        }
    }

    /// Height of an equilateral triangle with the given base width.
    pub fn equilateral_height(width: f64) -> f64 {
        width * 3.0_f64.sqrt() / 2.0
    }

    /// Default arch spring (Kämpfer) measured from the sill: `height - width/2`
    /// (semicircle on rectangular legs), clamped to `[0, height)`.
    pub fn default_spring_height(width: f64, height: f64) -> f64 {
        clamp_spring(height - width * 0.5, height)
    }

    /// Couple width/height for shapes that lock them together.
    ///
    /// * Circle: both become the same diameter (`prefer_width` picks the source).
    /// * Equilateral triangle: `height = width * √3/2` (width is authoritative).
    pub fn lock_size(self, width: f64, height: f64, prefer_width: bool) -> (f64, f64) {
        match self {
            OpeningShape::Circle => {
                let d = if prefer_width { width } else { height };
                let d = if d > 1e-12 {
                    d
                } else if prefer_width {
                    height
                } else {
                    width
                };
                (d, d)
            }
            OpeningShape::Triangle(TriangleVariant::Equilateral) => {
                (width, Self::equilateral_height(width))
            }
            _ => (width, height),
        }
    }

    /// Closed elevation polygon in `(s, z)`, CCW, first vertex not repeated.
    pub fn elevation_polygon(
        self,
        width: f64,
        height: f64,
        spring_height: f64,
    ) -> Vec<(f64, f64)> {
        let (width, height) = self.lock_size(width, height, true);
        if width <= 1e-12 || height <= 1e-12 {
            return Vec::new();
        }
        match self {
            OpeningShape::Rectangle => vec![
                (0.0, 0.0),
                (width, 0.0),
                (width, height),
                (0.0, height),
            ],
            OpeningShape::Circle => tessellate_circle(width * 0.5, height * 0.5, width * 0.5),
            OpeningShape::Triangle(variant) => triangle_polygon(variant, width, height),
            OpeningShape::Arch => arch_polygon(width, height, spring_height),
        }
    }
}

fn triangle_polygon(variant: TriangleVariant, width: f64, height: f64) -> Vec<(f64, f64)> {
    match variant {
        TriangleVariant::IsoscelesUp | TriangleVariant::Equilateral => {
            vec![(0.0, 0.0), (width, 0.0), (width * 0.5, height)]
        }
        TriangleVariant::IsoscelesDown => {
            vec![(0.0, height), (width * 0.5, 0.0), (width, height)]
        }
        TriangleVariant::RightLeft => vec![(0.0, 0.0), (width, 0.0), (0.0, height)],
        TriangleVariant::RightRight => vec![(0.0, 0.0), (width, 0.0), (width, height)],
    }
}

fn tessellate_circle(cx: f64, cy: f64, radius: f64) -> Vec<(f64, f64)> {
    if radius <= 1e-12 {
        return Vec::new();
    }
    let n = CIRCLE_CHORD_COUNT.max(8);
    (0..n)
        .map(|i| {
            let t = (i as f64) * std::f64::consts::TAU / (n as f64);
            (cx + radius * t.cos(), cy + radius * t.sin())
        })
        .collect()
}

fn arch_polygon(width: f64, height: f64, spring_height: f64) -> Vec<(f64, f64)> {
    let spring = clamp_spring(spring_height, height);
    let rise = height - spring;
    let mut pts = vec![(0.0, 0.0), (width, 0.0), (width, spring)];
    if rise <= 1e-12 {
        pts.push((0.0, spring));
        return pts;
    }
    let chord = width;
    let radius = chord * chord / (8.0 * rise) + rise * 0.5;
    let cx = width * 0.5;
    let cy = height - radius;
    let left = (0.0, spring);
    let right = (width, spring);
    let a0 = (right.1 - cy).atan2(right.0 - cx);
    let a1 = (left.1 - cy).atan2(left.0 - cx);
    let mut a1 = a1;
    while a1 <= a0 {
        a1 += std::f64::consts::TAU;
    }
    let n = ARCH_CHORD_COUNT.max(4);
    for i in 1..n {
        let t = a0 + (a1 - a0) * (i as f64) / (n as f64);
        pts.push((cx + radius * t.cos(), cy + radius * t.sin()));
    }
    pts.push(left);
    pts
}

/// Clamp Kämpfer so `0 ≤ spring < height`.
pub fn clamp_spring(spring: f64, height: f64) -> f64 {
    if height <= 1e-12 {
        return 0.0;
    }
    spring.clamp(0.0, (height - 1e-9).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn shoelace(poly: &[(f64, f64)]) -> f64 {
        let n = poly.len();
        let mut acc = 0.0;
        for i in 0..n {
            let (x0, y0) = poly[i];
            let (x1, y1) = poly[(i + 1) % n];
            acc += x0 * y1 - x1 * y0;
        }
        acc * 0.5
    }

    #[test]
    fn circle_locks_width_and_height_to_diameter() {
        let (w, h) = OpeningShape::Circle.lock_size(1.2, 0.8, true);
        assert!(approx(w, 1.2) && approx(h, 1.2));
        let (w, h) = OpeningShape::Circle.lock_size(1.2, 0.8, false);
        assert!(approx(w, 0.8) && approx(h, 0.8));
    }

    #[test]
    fn equilateral_height_is_sqrt3_over_2() {
        let (w, h) = OpeningShape::Triangle(TriangleVariant::Equilateral).lock_size(2.0, 99.0, true);
        assert!(approx(w, 2.0));
        assert!(approx(h, OpeningShape::equilateral_height(2.0)));
        assert!(approx(h, 3.0_f64.sqrt()));
    }

    #[test]
    fn default_spring_is_height_minus_half_width() {
        let spring = OpeningShape::default_spring_height(1.0, 2.0);
        assert!(approx(spring, 1.5));
    }

    #[test]
    fn rectangle_polygon_is_unit_box() {
        let p = OpeningShape::Rectangle.elevation_polygon(1.2, 1.0, 0.0);
        assert_eq!(p, vec![(0.0, 0.0), (1.2, 0.0), (1.2, 1.0), (0.0, 1.0)]);
        assert!(shoelace(&p) > 0.0);
    }

    #[test]
    fn circle_polygon_stays_inside_bounding_box() {
        let p = OpeningShape::Circle.elevation_polygon(1.0, 2.0, 0.0);
        assert_eq!(p.len(), CIRCLE_CHORD_COUNT);
        for &(s, z) in &p {
            assert!(s >= -1e-9 && s <= 1.0 + 1e-9);
            assert!(z >= -1e-9 && z <= 1.0 + 1e-9);
        }
        assert!(shoelace(&p) > 0.0);
    }

    #[test]
    fn triangle_variants_have_expected_vertices() {
        let up = OpeningShape::Triangle(TriangleVariant::IsoscelesUp).elevation_polygon(2.0, 1.0, 0.0);
        assert_eq!(up, vec![(0.0, 0.0), (2.0, 0.0), (1.0, 1.0)]);
        let down =
            OpeningShape::Triangle(TriangleVariant::IsoscelesDown).elevation_polygon(2.0, 1.0, 0.0);
        assert_eq!(down, vec![(0.0, 1.0), (1.0, 0.0), (2.0, 1.0)]);
        let left = OpeningShape::Triangle(TriangleVariant::RightLeft).elevation_polygon(2.0, 1.0, 0.0);
        assert_eq!(left, vec![(0.0, 0.0), (2.0, 0.0), (0.0, 1.0)]);
        let right =
            OpeningShape::Triangle(TriangleVariant::RightRight).elevation_polygon(2.0, 1.0, 0.0);
        assert_eq!(right, vec![(0.0, 0.0), (2.0, 0.0), (2.0, 1.0)]);
        for poly in [&up, &down, &left, &right] {
            assert!(shoelace(poly) > 0.0);
        }
    }

    #[test]
    fn arch_default_spring_is_semicircle_on_legs() {
        let width = 1.0;
        let height = 2.0;
        let spring = OpeningShape::default_spring_height(width, height);
        let p = OpeningShape::Arch.elevation_polygon(width, height, spring);
        assert!(p.len() >= 5);
        assert_eq!(p[0], (0.0, 0.0));
        assert_eq!(p[1], (width, 0.0));
        assert_eq!(p[2], (width, spring));
        let apex_z = p.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        assert!((apex_z - height).abs() < 1e-6);
        assert!(shoelace(&p) > 0.0);
    }

    #[test]
    fn arch_changed_spring_changes_rise() {
        let low = OpeningShape::Arch.elevation_polygon(1.0, 2.0, 0.5);
        let high = OpeningShape::Arch.elevation_polygon(1.0, 2.0, 1.5);
        let rise_low = 2.0 - 0.5;
        let rise_high = 2.0 - 1.5;
        assert!((low.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max) - 2.0).abs() < 1e-6);
        assert!((high.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max) - 2.0).abs() < 1e-6);
        assert!(rise_low > rise_high);
        assert_eq!(low[2], (1.0, 0.5));
        assert_eq!(high[2], (1.0, 1.5));
    }

    #[test]
    fn from_str_defaults_unknown_to_rectangle() {
        assert_eq!(OpeningShape::from_str(""), OpeningShape::Rectangle);
        assert_eq!(OpeningShape::from_str("Circle"), OpeningShape::Circle);
        assert_eq!(
            OpeningShape::from_str("Triangle:RightLeft"),
            OpeningShape::Triangle(TriangleVariant::RightLeft)
        );
        assert_eq!(OpeningShape::from_str("nope"), OpeningShape::Rectangle);
    }

    #[test]
    fn roundtrip_as_str() {
        for shape in [
            OpeningShape::Rectangle,
            OpeningShape::Circle,
            OpeningShape::Arch,
            OpeningShape::Triangle(TriangleVariant::IsoscelesUp),
            OpeningShape::Triangle(TriangleVariant::IsoscelesDown),
            OpeningShape::Triangle(TriangleVariant::Equilateral),
            OpeningShape::Triangle(TriangleVariant::RightLeft),
            OpeningShape::Triangle(TriangleVariant::RightRight),
        ] {
            assert_eq!(OpeningShape::from_str(shape.as_str()), shape);
        }
    }
}
