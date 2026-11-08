//! Reusable cross-sections for AEC components.

use crate::modules::aec::engine::style::Style;
use crate::modules::aec::engine::style_common::{
    CATEGORY_SECTION_PROFILE, StyleCommon, StyleOrigin,
};
use serde::{Deserialize, Serialize};

const CIRCLE_SEGMENTS: usize = 48;
const POINT_EPSILON: f64 = 1e-12;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SectionShape {
    Rectangle { face: f64, depth: f64 },
    Circle { diameter: f64 },
    Triangle { base: f64, height: f64 },
    Custom { points: Vec<(f64, f64)> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectionProfile {
    pub style: Style,
    pub shape: SectionShape,
}

impl SectionProfile {
    pub fn new(id: impl Into<String>, name: impl Into<String>, shape: SectionShape) -> Self {
        Self {
            style: StyleCommon::new(
                id,
                name,
                None,
                CATEGORY_SECTION_PROFILE,
                StyleOrigin::Default,
            ),
            shape,
        }
    }

    /// Returns whether this shape has a finite, non-degenerate cross-section.
    pub fn is_valid(&self) -> bool {
        match &self.shape {
            SectionShape::Rectangle { face, depth } => {
                positive_finite(*face) && positive_finite(*depth)
            }
            SectionShape::Circle { diameter } => positive_finite(*diameter),
            SectionShape::Triangle { base, height } => {
                positive_finite(*base) && positive_finite(*height)
            }
            SectionShape::Custom { points } => custom_points(points).is_some(),
        }
    }

    /// Closes a valid custom contour before it is stored in the library.
    pub fn close_custom_contour(&mut self) -> bool {
        if !self.is_valid() {
            return false;
        }
        if let SectionShape::Custom { points } = &mut self.shape {
            let Some((mut normalized, min_x, min_y, _, _)) = custom_points(points) else {
                return false;
            };
            for (x, y) in &mut normalized {
                *x -= min_x;
                *y -= min_y;
            }
            normalized.push(normalized[0]);
            *points = normalized;
        }
        true
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedSection {
    pub outline: Vec<(f64, f64)>,
    pub face: f64,
    pub depth: f64,
}

/// Resolves a profile outline and its axis-aligned bounding dimensions.
/// Coordinates use x for the view width and y for the wall depth, with the
/// lower-left of the bounds at the origin.
pub fn resolve_section(
    profile: Option<&SectionProfile>,
    fallback_face: f64,
    fallback_depth: f64,
) -> ResolvedSection {
    if let Some(profile) = profile.filter(|profile| profile.is_valid()) {
        return resolve_shape(&profile.shape).expect("validated section shape must resolve");
    }

    let face = non_negative_finite(fallback_face);
    let depth = non_negative_finite(fallback_depth);
    ResolvedSection {
        outline: rectangle_outline(face, depth),
        face,
        depth,
    }
}

fn resolve_shape(shape: &SectionShape) -> Option<ResolvedSection> {
    match shape {
        SectionShape::Rectangle { face, depth } => Some(ResolvedSection {
            outline: rectangle_outline(*face, *depth),
            face: *face,
            depth: *depth,
        }),
        SectionShape::Circle { diameter } => {
            let center = diameter / 2.0;
            let radius = center;
            let mut outline = Vec::with_capacity(CIRCLE_SEGMENTS + 1);
            for i in 0..CIRCLE_SEGMENTS {
                let angle = std::f64::consts::TAU * i as f64 / CIRCLE_SEGMENTS as f64;
                let mut x = center + radius * angle.cos();
                let mut y = center + radius * angle.sin();
                if x.abs() < POINT_EPSILON {
                    x = 0.0;
                } else if (x - diameter).abs() < POINT_EPSILON {
                    x = *diameter;
                }
                if y.abs() < POINT_EPSILON {
                    y = 0.0;
                } else if (y - diameter).abs() < POINT_EPSILON {
                    y = *diameter;
                }
                outline.push((x, y));
            }
            outline.push(outline[0]);
            Some(ResolvedSection {
                outline,
                face: *diameter,
                depth: *diameter,
            })
        }
        SectionShape::Triangle { base, height } => Some(ResolvedSection {
            outline: vec![(0.0, 0.0), (*base, 0.0), (*base / 2.0, *height), (0.0, 0.0)],
            face: *base,
            depth: *height,
        }),
        SectionShape::Custom { points } => {
            let (mut points, min_x, min_y, max_x, max_y) = custom_points(points)?;
            for (x, y) in &mut points {
                *x -= min_x;
                *y -= min_y;
            }
            points.push(points[0]);
            Some(ResolvedSection {
                outline: points,
                face: max_x - min_x,
                depth: max_y - min_y,
            })
        }
    }
}

fn rectangle_outline(face: f64, depth: f64) -> Vec<(f64, f64)> {
    vec![
        (0.0, 0.0),
        (face, 0.0),
        (face, depth),
        (0.0, depth),
        (0.0, 0.0),
    ]
}

fn custom_points(points: &[(f64, f64)]) -> Option<(Vec<(f64, f64)>, f64, f64, f64, f64)> {
    if points.iter().any(|(x, y)| !x.is_finite() || !y.is_finite()) {
        return None;
    }
    let mut points = points.to_vec();
    if points.len() > 1 && points_close(points[0], *points.last()?) {
        points.pop();
    }
    if points.len() < 3 {
        return None;
    }

    let min_x = points.iter().map(|(x, _)| *x).fold(f64::INFINITY, f64::min);
    let min_y = points.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
    let max_x = points
        .iter()
        .map(|(x, _)| *x)
        .fold(f64::NEG_INFINITY, f64::max);
    let max_y = points
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::NEG_INFINITY, f64::max);
    let face = max_x - min_x;
    let depth = max_y - min_y;
    if !face.is_finite() || !depth.is_finite() || face <= 0.0 || depth <= 0.0 {
        return None;
    }
    Some((points, min_x, min_y, max_x, max_y))
}

fn points_close(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() <= POINT_EPSILON && (a.1 - b.1).abs() <= POINT_EPSILON
}

fn positive_finite(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

fn non_negative_finite(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_resolves_to_closed_outline_and_matching_bounds() {
        let profile = SectionProfile::new(
            "rect",
            "Rectangle",
            SectionShape::Rectangle {
                face: 0.07,
                depth: 0.08,
            },
        );
        let resolved = resolve_section(Some(&profile), 1.0, 1.0);
        assert_eq!(resolved.face, 0.07);
        assert_eq!(resolved.depth, 0.08);
        assert_eq!(resolved.outline.len(), 5);
        assert_eq!(resolved.outline.first(), resolved.outline.last());
    }

    #[test]
    fn circle_has_a_closed_non_rectangular_outline() {
        let profile =
            SectionProfile::new("circle", "Circle", SectionShape::Circle { diameter: 0.04 });
        let resolved = resolve_section(Some(&profile), 1.0, 1.0);
        assert_eq!(resolved.face, 0.04);
        assert_eq!(resolved.depth, 0.04);
        assert_eq!(resolved.outline.len(), CIRCLE_SEGMENTS + 1);
        assert_eq!(resolved.outline.first(), resolved.outline.last());
        assert_ne!(resolved.outline[1].1, 0.0);
    }

    #[test]
    fn triangle_places_apex_at_half_the_base() {
        let profile = SectionProfile::new(
            "triangle",
            "Triangle",
            SectionShape::Triangle {
                base: 0.06,
                height: 0.05,
            },
        );
        let resolved = resolve_section(Some(&profile), 1.0, 1.0);
        assert_eq!(resolved.outline[2], (0.03, 0.05));
        assert_eq!((resolved.face, resolved.depth), (0.06, 0.05));
        assert_eq!(resolved.outline.first(), resolved.outline.last());
    }

    #[test]
    fn custom_shape_is_normalized_and_closed() {
        let mut profile = SectionProfile::new(
            "custom",
            "Custom",
            SectionShape::Custom {
                points: vec![(-1.0, 2.0), (1.0, 2.0), (0.0, 3.0)],
            },
        );
        assert!(profile.close_custom_contour());
        let SectionShape::Custom { points } = &profile.shape else {
            unreachable!();
        };
        assert_eq!(points.first(), points.last());
        assert_eq!(points[0], (0.0, 0.0));
        let resolved = resolve_section(Some(&profile), 1.0, 1.0);
        assert_eq!(resolved.outline[0], (0.0, 0.0));
        assert_eq!((resolved.face, resolved.depth), (2.0, 1.0));
        assert_eq!(resolved.outline.first(), resolved.outline.last());
    }

    #[test]
    fn self_intersecting_custom_contour_is_valid() {
        let profile = SectionProfile::new(
            "crossing",
            "Crossing contour",
            SectionShape::Custom {
                points: vec![(0.0, 0.0), (1.0, 1.0), (0.0, 1.0), (1.0, 0.0)],
            },
        );
        assert!(profile.is_valid());
        let resolved = resolve_section(Some(&profile), 0.0, 0.0);
        assert_eq!((resolved.face, resolved.depth), (1.0, 1.0));
    }

    #[test]
    fn invalid_custom_contour_is_rejected_and_falls_back() {
        let profile = SectionProfile::new(
            "invalid",
            "Invalid",
            SectionShape::Custom {
                points: vec![(0.0, 0.0), (1.0, 0.0)],
            },
        );
        assert!(!profile.is_valid());
        let resolved = resolve_section(Some(&profile), 0.07, 0.08);
        assert_eq!((resolved.face, resolved.depth), (0.07, 0.08));
        assert_eq!(resolved.outline.len(), 5);
    }

    #[test]
    fn fallback_dimensions_are_never_negative_or_non_finite() {
        let resolved = resolve_section(None, -0.1, f64::NAN);
        assert_eq!((resolved.face, resolved.depth), (0.0, 0.0));
        assert!(resolved.outline.iter().all(|(x, y)| *x >= 0.0 && *y >= 0.0));
    }
}
