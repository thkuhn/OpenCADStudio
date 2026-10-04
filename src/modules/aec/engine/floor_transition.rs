//! Floor transition logic for room boundaries at wall openings (doors, thresholds, reveals, and niches).
//!
//! Provides geometric calculation of door reveal transition polygons, threshold joint lines,
//! and additional floor finish take-off areas.

use acadrust::Handle;

use super::geometry::area;
use super::opening_xdata::openings_for_host_wall;
use super::openings::OpeningKind;
use super::xdata::wall_from_entity;
use crate::scene::Scene;

/// Represents a floor transition area (e.g. half-reveal up to door leaf / threshold line).
#[derive(Debug, Clone, PartialEq)]
pub struct FloorTransitionZone {
    /// Opening entity handle.
    pub opening_handle: Handle,
    /// Host wall entity handle.
    pub wall_handle: Handle,
    /// Opening kind (Door, Breakthrough, Window recess).
    pub kind: OpeningKind,
    /// 4 polygon vertices forming the transition zone in world XY (CCW).
    pub polygon: Vec<(f64, f64)>,
    /// Threshold / transition separation line in world coordinates.
    pub threshold_line: ((f64, f64), (f64, f64)),
    /// Area of the transition polygon in m².
    pub area: f64,
}

/// Computes the distance from a point `p` to the line segment `ab`.
pub fn dist_point_to_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-12 {
        return (p.0 - a.0).hypot(p.1 - a.1);
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len_sq).clamp(0.0, 1.0);
    let proj = (a.0 + t * dx, a.1 + t * dy);
    (p.0 - proj.0).hypot(p.1 - proj.1)
}

/// Computes the minimum distance from a point `p` to any edge of the polygon `poly`.
pub fn dist_point_to_polygon_boundary(p: (f64, f64), poly: &[(f64, f64)]) -> f64 {
    if poly.len() < 2 {
        return f64::MAX;
    }
    let mut min_d = f64::MAX;
    for i in 0..poly.len() {
        let j = (i + 1) % poly.len();
        let d = dist_point_to_segment(p, poly[i], poly[j]);
        if d < min_d {
            min_d = d;
        }
    }
    min_d
}

/// Identifies all floor transition zones (e.g. door reveals and threshold zones)
/// for a room given its boundary polygon and the walls/openings in the scene.
pub fn find_floor_transitions_for_room(
    scene: &Scene,
    room_boundary: &[(f64, f64)],
) -> Vec<FloorTransitionZone> {
    if room_boundary.len() < 3 {
        return Vec::new();
    }

    let mut transitions = Vec::new();

    for entity in scene.document.entities() {
        let Some(wall) = wall_from_entity(entity) else {
            continue;
        };
        let wall_handle = entity.common().handle;

        // Retrieve wall axis endpoints
        let (p0, p1) = if let acadrust::EntityType::LwPolyline(pl) = entity {
            if pl.vertices.len() >= 2 {
                (
                    (pl.vertices[0].location.x, pl.vertices[0].location.y),
                    (
                        pl.vertices[pl.vertices.len() - 1].location.x,
                        pl.vertices[pl.vertices.len() - 1].location.y,
                    ),
                )
            } else {
                continue;
            }
        } else {
            continue;
        };

        let dx = p1.0 - p0.0;
        let dy = p1.1 - p0.1;
        let len = dx.hypot(dy);
        if len < 1e-4 {
            continue;
        }

        let dir = (dx / len, dy / len);
        let normal = (-dir.1, dir.0); // +90 deg
        let wall_thickness = if wall.total_thickness() > 1e-4 {
            wall.total_thickness()
        } else {
            0.24
        };

        // Check all openings hosted on this wall
        let openings = openings_for_host_wall(scene, wall_handle);
        for opening in openings {
            let is_candidate = opening.kind == OpeningKind::Door
                || opening.kind == OpeningKind::Breakthrough
                || opening.depth.is_some();
            if !is_candidate {
                continue;
            }

            let s_center = opening.distance_along_axis;
            let w = opening.width;
            let s_start = (s_center - w * 0.5).max(0.0);
            let s_end = (s_center + w * 0.5).min(len);
            if s_end <= s_start {
                continue;
            }

            let c0 = (p0.0 + dir.0 * s_start, p0.1 + dir.1 * s_start);
            let c1 = (p0.0 + dir.0 * s_end, p0.1 + dir.1 * s_end);

            let half_thick = wall_thickness * 0.5;
            let p0_neg = (c0.0 - normal.0 * half_thick, c0.1 - normal.1 * half_thick);
            let p1_neg = (c1.0 - normal.0 * half_thick, c1.1 - normal.1 * half_thick);
            let p0_pos = (c0.0 + normal.0 * half_thick, c0.1 + normal.1 * half_thick);
            let p1_pos = (c1.0 + normal.0 * half_thick, c1.1 + normal.1 * half_thick);

            let mid_neg = ((p0_neg.0 + p1_neg.0) * 0.5, (p0_neg.1 + p1_neg.1) * 0.5);
            let mid_pos = ((p0_pos.0 + p1_pos.0) * 0.5, (p0_pos.1 + p1_pos.1) * 0.5);

            let thresh_off = opening.cross_axis_offset;
            let t0 = (c0.0 + normal.0 * thresh_off, c0.1 + normal.1 * thresh_off);
            let t1 = (c1.0 + normal.0 * thresh_off, c1.1 + normal.1 * thresh_off);

            let d_neg = dist_point_to_polygon_boundary(mid_neg, room_boundary);
            let d_pos = dist_point_to_polygon_boundary(mid_pos, room_boundary);

            let tolerance = 0.25;

            if d_neg < tolerance && d_neg <= d_pos {
                let poly = vec![p0_neg, p1_neg, t1, t0];
                let a = area(&poly);
                transitions.push(FloorTransitionZone {
                    opening_handle: opening.handle,
                    wall_handle,
                    kind: opening.kind,
                    polygon: poly,
                    threshold_line: (t0, t1),
                    area: a,
                });
            } else if d_pos < tolerance && d_pos < d_neg {
                let poly = vec![t0, t1, p1_pos, p0_pos];
                let a = area(&poly);
                transitions.push(FloorTransitionZone {
                    opening_handle: opening.handle,
                    wall_handle,
                    kind: opening.kind,
                    polygon: poly,
                    threshold_line: (t0, t1),
                    area: a,
                });
            }
        }
    }

    transitions
}

/// Computes the total additional floor finish area from transition zones (m²).
pub fn total_transition_area(transitions: &[FloorTransitionZone]) -> f64 {
    transitions.iter().map(|t| t.area).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dist_point_to_segment() {
        let a = (0.0, 0.0);
        let b = (4.0, 0.0);
        assert!((dist_point_to_segment((2.0, 1.0), a, b) - 1.0).abs() < 1e-6);
        assert!((dist_point_to_segment((-1.0, 0.0), a, b) - 1.0).abs() < 1e-6);
        assert!((dist_point_to_segment((5.0, 0.0), a, b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_dist_point_to_polygon_boundary() {
        let poly = vec![(0.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 3.0)];
        assert!((dist_point_to_polygon_boundary((2.0, -0.5), &poly) - 0.5).abs() < 1e-6);
        assert!((dist_point_to_polygon_boundary((2.0, 0.0), &poly) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_floor_transition_area_calculation() {
        let transitions = vec![
            FloorTransitionZone {
                opening_handle: Handle::new(1),
                wall_handle: Handle::new(2),
                kind: OpeningKind::Door,
                polygon: vec![(0.0, 0.0), (1.0, 0.0), (1.0, 0.12), (0.0, 0.12)],
                threshold_line: ((0.0, 0.12), (1.0, 0.12)),
                area: 0.12,
            },
            FloorTransitionZone {
                opening_handle: Handle::new(3),
                wall_handle: Handle::new(4),
                kind: OpeningKind::Door,
                polygon: vec![(5.0, 0.0), (6.0, 0.0), (6.0, 0.12), (5.0, 0.12)],
                threshold_line: ((5.0, 0.12), (6.0, 0.12)),
                area: 0.12,
            },
        ];

        assert!((total_transition_area(&transitions) - 0.24).abs() < 1e-6);
    }
}
