//! Circular-arc helpers for wall axes that use LWPOLYLINE bulge segments.
//!
//! Bulge convention matches DXF / `acadrust::entities::LwVertex`:
//! `bulge = tan(included_angle / 4)`. Positive bulge = counter-clockwise arc
//! from start → end; negative = clockwise; `0.0` = straight segment.
//!
//! Offset convention matches [`super::geometry::normal`]: positive distance is
//! to the **left** of the directed segment / arc travel direction.

use std::f64::consts::PI;

const EPS: f64 = 1e-12;
const ANG_EPS: f64 = 1e-10;

/// One open-axis segment: straight when `bulge == 0`, otherwise a circular arc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisSegment {
    pub start: (f64, f64),
    pub end: (f64, f64),
    /// LWPOLYLINE bulge from `start` to `end` (`0.0` = straight).
    pub bulge: f64,
}

impl AxisSegment {
    pub fn straight(start: (f64, f64), end: (f64, f64)) -> Self {
        Self {
            start,
            end,
            bulge: 0.0,
        }
    }

    pub fn with_bulge(start: (f64, f64), end: (f64, f64), bulge: f64) -> Self {
        Self { start, end, bulge }
    }

    pub fn is_arc(&self) -> bool {
        self.bulge.abs() > EPS
    }
}

/// Build axis segments from a vertex list and per-vertex bulges.
///
/// `bulges[i]` is the bulge from `points[i]` to `points[i + 1]`. Missing
/// entries (shorter slice) are treated as `0.0`. The last bulge of an open
/// polyline is unused, matching LWPOLYLINE.
pub fn segments_from_points_bulges(points: &[(f64, f64)], bulges: &[f64]) -> Vec<AxisSegment> {
    if points.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(points.len() - 1);
    for i in 0..points.len() - 1 {
        let bulge = bulges.get(i).copied().unwrap_or(0.0);
        out.push(AxisSegment {
            start: points[i],
            end: points[i + 1],
            bulge,
        });
    }
    out
}

/// Circular arc in the plane (always stored with `radius > 0`).
///
/// Angles are absolute (atan2 space). Travel from `start_angle` to `end_angle`
/// is counter-clockwise when `ccw` is true, otherwise clockwise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CircularArc {
    pub center: (f64, f64),
    pub radius: f64,
    pub start_angle: f64,
    pub end_angle: f64,
    pub ccw: bool,
}

impl CircularArc {
    pub fn start_point(&self) -> (f64, f64) {
        point_on_circle(self.center, self.radius, self.start_angle)
    }

    pub fn end_point(&self) -> (f64, f64) {
        point_on_circle(self.center, self.radius, self.end_angle)
    }

    /// Included central angle in radians, always in `(0, 2π]`.
    pub fn included_angle(&self) -> f64 {
        delta_angle(self.start_angle, self.end_angle, self.ccw)
    }
}

fn point_on_circle(center: (f64, f64), radius: f64, angle: f64) -> (f64, f64) {
    (
        center.0 + radius * angle.cos(),
        center.1 + radius * angle.sin(),
    )
}

fn normalize_angle(a: f64) -> f64 {
    let mut x = a % (2.0 * PI);
    if x < 0.0 {
        x += 2.0 * PI;
    }
    x
}

/// Signed turn from `from` to `to` in the given orientation, in `(0, 2π]`.
fn delta_angle(from: f64, to: f64, ccw: bool) -> f64 {
    let mut d = if ccw {
        normalize_angle(to - from)
    } else {
        normalize_angle(from - to)
    };
    if d < ANG_EPS {
        d = 2.0 * PI;
    }
    d
}

fn chord_length(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    (dx * dx + dy * dy).sqrt()
}

fn left_unit(dx: f64, dy: f64) -> Option<(f64, f64)> {
    let len = (dx * dx + dy * dy).sqrt();
    if len < EPS {
        None
    } else {
        Some((-dy / len, dx / len))
    }
}

/// Convert a bulge segment into a [`CircularArc`].
///
/// Returns `None` when the segment is straight (`|bulge| ≈ 0`) or degenerate
/// (coincident endpoints).
pub fn bulge_to_arc(start: (f64, f64), end: (f64, f64), bulge: f64) -> Option<CircularArc> {
    if bulge.abs() <= EPS {
        return None;
    }
    let chord = chord_length(start, end);
    if chord < EPS {
        return None;
    }

    // radius = chord * (1 + b²) / (4 |b|); mid→center = chord * (1 - b²) / (4 b)
    // along the left normal of start→end.
    let b = bulge;
    let radius = (chord * (1.0 + b * b) / (4.0 * b)).abs();
    if radius < EPS {
        return None;
    }

    let mid = ((start.0 + end.0) * 0.5, (start.1 + end.1) * 0.5);
    let (nx, ny) = left_unit(end.0 - start.0, end.1 - start.1)?;
    let d = chord * (1.0 - b * b) / (4.0 * b);
    let center = (mid.0 + nx * d, mid.1 + ny * d);

    let start_angle = (start.1 - center.1).atan2(start.0 - center.0);
    let end_angle = (end.1 - center.1).atan2(end.0 - center.0);
    let ccw = b > 0.0;

    Some(CircularArc {
        center,
        radius,
        start_angle,
        end_angle,
        ccw,
    })
}

/// Convert a circular arc back to start/end points and LWPOLYLINE bulge.
pub fn arc_to_bulge(arc: &CircularArc) -> ((f64, f64), (f64, f64), f64) {
    let start = arc.start_point();
    let end = arc.end_point();
    let included = arc.included_angle();
    // bulge = tan(included/4); sign from orientation.
    let mut bulge = (included * 0.25).tan();
    if !arc.ccw {
        bulge = -bulge;
    }
    (start, end, bulge)
}

/// Parallel offset of a circular arc by a signed distance (left-positive).
///
/// For a CCW arc the left side points toward the center, so a positive offset
/// decreases radius; for a CW arc the left side points outward and a positive
/// offset increases radius. If the offset would cross the center the arc
/// orientation flips and the radius is taken absolute.
///
/// Returns `None` when the result collapses to a point (`|new_radius| ≈ 0`).
pub fn offset_arc(arc: &CircularArc, distance: f64) -> Option<CircularArc> {
    // Left-of-travel radius change: −sign for CCW, +sign for CW.
    let side = if arc.ccw { -1.0 } else { 1.0 };
    let mut new_radius = arc.radius + side * distance;
    let mut ccw = arc.ccw;
    let mut start_angle = arc.start_angle;
    let mut end_angle = arc.end_angle;

    if new_radius.abs() < EPS {
        return None;
    }
    if new_radius < 0.0 {
        // Crossed the center: flip orientation and swap ends so the image of
        // the original start/end mapping stays continuous with left-offset.
        new_radius = -new_radius;
        ccw = !ccw;
        std::mem::swap(&mut start_angle, &mut end_angle);
    }

    Some(CircularArc {
        center: arc.center,
        radius: new_radius,
        start_angle,
        end_angle,
        ccw,
    })
}

/// Offset a bulge segment by `distance` (left-positive).
///
/// Returns the offset start, end, and bulge. Straight segments are shifted
/// along their left unit normal. Arc segments use [`offset_arc`].
pub fn offset_bulge_segment(
    start: (f64, f64),
    end: (f64, f64),
    bulge: f64,
    distance: f64,
) -> Option<((f64, f64), (f64, f64), f64)> {
    if bulge.abs() <= EPS {
        let (nx, ny) = left_unit(end.0 - start.0, end.1 - start.1)?;
        let s = (start.0 + nx * distance, start.1 + ny * distance);
        let e = (end.0 + nx * distance, end.1 + ny * distance);
        return Some((s, e, 0.0));
    }
    let arc = bulge_to_arc(start, end, bulge)?;
    let off = offset_arc(&arc, distance)?;
    let (s, e, b) = arc_to_bulge(&off);
    Some((s, e, b))
}

/// Point on the arc at parameter `t ∈ [0, 1]` along travel direction.
pub fn arc_point_at(arc: &CircularArc, t: f64) -> (f64, f64) {
    let included = arc.included_angle();
    let angle = if arc.ccw {
        arc.start_angle + t * included
    } else {
        arc.start_angle - t * included
    };
    point_on_circle(arc.center, arc.radius, angle)
}

/// Unit left normal at a point along an arc (left of travel direction).
pub fn arc_left_normal(arc: &CircularArc, at_start: bool) -> (f64, f64) {
    let angle = if at_start {
        arc.start_angle
    } else {
        arc.end_angle
    };
    let radial = (angle.cos(), angle.sin());
    // CCW travel: left = −radial (inward). CW travel: left = +radial (outward).
    if arc.ccw {
        (-radial.0, -radial.1)
    } else {
        radial
    }
}


fn dist2(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    dx * dx + dy * dy
}

/// True when `angle` lies on the arc from `start` to `end` in the given sense
/// (inclusive, with wrap). Full-circle arcs (`included ≈ 2π`) accept all angles.
fn angle_on_arc(angle: f64, start: f64, end: f64, ccw: bool, tol: f64) -> bool {
    let included = delta_angle(start, end, ccw);
    if (included - 2.0 * PI).abs() < tol {
        return true;
    }
    let from_start = if ccw {
        normalize_angle(angle - start)
    } else {
        normalize_angle(start - angle)
    };
    from_start <= included + tol
}

/// Intersection of the infinite line through `p1→p2` with the circle of `arc`,
/// filtered to points that lie on the arc sweep (inclusive).
///
/// Returns 0, 1, or 2 points.
pub fn arc_line_intersection(
    arc: &CircularArc,
    p1: (f64, f64),
    p2: (f64, f64),
) -> Vec<(f64, f64)> {
    let dx = p2.0 - p1.0;
    let dy = p2.1 - p1.1;
    let len2 = dx * dx + dy * dy;
    if len2 < EPS * EPS {
        return Vec::new();
    }

    // Quadratic in t for |p1 + t (p2-p1) - c|² = r²
    let fx = p1.0 - arc.center.0;
    let fy = p1.1 - arc.center.1;
    let a = len2;
    let b = 2.0 * (fx * dx + fy * dy);
    let c = fx * fx + fy * fy - arc.radius * arc.radius;
    let disc = b * b - 4.0 * a * c;
    if disc < -EPS {
        return Vec::new();
    }
    let disc = disc.max(0.0);
    let sqrt_d = disc.sqrt();
    let inv = 0.5 / a;
    let t0 = (-b - sqrt_d) * inv;
    let t1 = (-b + sqrt_d) * inv;

    let mut out = Vec::with_capacity(2);
    for t in [t0, t1] {
        let pt = (p1.0 + t * dx, p1.1 + t * dy);
        let ang = (pt.1 - arc.center.1).atan2(pt.0 - arc.center.0);
        if angle_on_arc(ang, arc.start_angle, arc.end_angle, arc.ccw, ANG_EPS) {
            // Dedup near-tangent double roots.
            if out
                .iter()
                .all(|&q| dist2(q, pt) > 1e-18)
            {
                out.push(pt);
            }
        }
    }
    out
}

/// Intersection of two circular arcs (points on both sweeps).
///
/// Returns 0, 1, or 2 points.
pub fn arc_arc_intersection(a: &CircularArc, b: &CircularArc) -> Vec<(f64, f64)> {
    let dx = b.center.0 - a.center.0;
    let dy = b.center.1 - a.center.1;
    let d = (dx * dx + dy * dy).sqrt();
    if d < EPS {
        // Concentric: either none or infinitely many — treat as no discrete hit.
        return Vec::new();
    }
    if d > a.radius + b.radius + EPS || d < (a.radius - b.radius).abs() - EPS {
        return Vec::new();
    }

    // Distance from a.center to the line of intersection points.
    let x = (a.radius * a.radius - b.radius * b.radius + d * d) / (2.0 * d);
    let h2 = a.radius * a.radius - x * x;
    if h2 < -EPS {
        return Vec::new();
    }
    let h = h2.max(0.0).sqrt();
    let ux = dx / d;
    let uy = dy / d;
    let px = a.center.0 + x * ux;
    let py = a.center.1 + x * uy;

    let mut candidates = Vec::with_capacity(2);
    if h < EPS {
        candidates.push((px, py));
    } else {
        candidates.push((px + h * (-uy), py + h * ux));
        candidates.push((px - h * (-uy), py - h * ux));
    }

    candidates
        .into_iter()
        .filter(|&pt| {
            let ang_a = (pt.1 - a.center.1).atan2(pt.0 - a.center.0);
            let ang_b = (pt.1 - b.center.1).atan2(pt.0 - b.center.0);
            angle_on_arc(ang_a, a.start_angle, a.end_angle, a.ccw, ANG_EPS)
                && angle_on_arc(ang_b, b.start_angle, b.end_angle, b.ccw, ANG_EPS)
        })
        .collect()
}

/// Intersection of two infinite lines `a1→a2` and `b1→b2` (2D).
pub fn line_line_intersection(
    a1: (f64, f64),
    a2: (f64, f64),
    b1: (f64, f64),
    b2: (f64, f64),
) -> Option<(f64, f64)> {
    let d1x = a2.0 - a1.0;
    let d1y = a2.1 - a1.1;
    let d2x = b2.0 - b1.0;
    let d2y = b2.1 - b1.1;
    let cross = d1x * d2y - d1y * d2x;
    if cross.abs() < EPS {
        return None;
    }
    let t = ((b1.0 - a1.0) * d2y - (b1.1 - a1.1) * d2x) / cross;
    Some((a1.0 + t * d1x, a1.1 + t * d1y))
}

/// Pick the intersection candidate closest to `hint`.
fn closest_to(hint: (f64, f64), pts: &[(f64, f64)]) -> Option<(f64, f64)> {
    pts.iter()
        .copied()
        .min_by(|p, q| {
            dist2(*p, hint)
                .partial_cmp(&dist2(*q, hint))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// Offset an open polyline that may contain bulge arcs.
///
/// Returns `(offset_points, offset_bulges)` with the same vertex count as
/// `points`. `bulges[i]` is the bulge of segment `i → i+1` (missing = 0).
/// Interior joints are resolved by intersecting consecutive offset segments
/// (line-line / arc-line / arc-arc). Endpoints use the segment-local left
/// offset of the terminal vertex.
///
/// When every bulge is zero this produces the same geometry as the miter
/// path in [`super::geometry::get_offset_directions`] (line-line miter).
pub fn offset_polyline_with_bulges(
    points: &[(f64, f64)],
    bulges: &[f64],
    distance: f64,
) -> (Vec<(f64, f64)>, Vec<f64>) {
    let n = points.len();
    if n < 2 {
        return (points.to_vec(), vec![0.0; n]);
    }

    let segs = segments_from_points_bulges(points, bulges);
    let mut off_segs: Vec<Option<((f64, f64), (f64, f64), f64)>> = Vec::with_capacity(segs.len());
    for s in &segs {
        off_segs.push(offset_bulge_segment(s.start, s.end, s.bulge, distance));
    }

    let mut out_pts = vec![(0.0, 0.0); n];
    let mut out_bulges = vec![0.0; n];

    // Endpoints from first/last offset segments.
    if let Some((s, e, b)) = off_segs[0] {
        out_pts[0] = s;
        out_bulges[0] = b;
        if n == 2 {
            out_pts[1] = e;
            return (out_pts, out_bulges);
        }
    } else {
        // Degenerate first segment — fall back to raw point.
        out_pts[0] = points[0];
    }

    if let Some((_, e, _)) = off_segs[n - 2] {
        out_pts[n - 1] = e;
    } else {
        out_pts[n - 1] = points[n - 1];
    }

    // Interior vertices: intersect offset of seg i-1 with offset of seg i.
    for i in 1..n - 1 {
        let hint = {
            // Simple average of adjacent endpoint offsets as a selector hint.
            let mut hx = points[i].0;
            let mut hy = points[i].1;
            if let Some((_, e, _)) = off_segs[i - 1] {
                hx = e.0;
                hy = e.1;
            }
            if let Some((s, _, _)) = off_segs[i] {
                hx = 0.5 * (hx + s.0);
                hy = 0.5 * (hy + s.1);
            }
            (hx, hy)
        };

        let prev = &segs[i - 1];
        let next = &segs[i];
        let prev_off = off_segs[i - 1];
        let next_off = off_segs[i];

        let joined = match (prev_off, next_off) {
            (Some((ps, pe, pb)), Some((ns, ne, nb))) => {
                let hit = intersect_offset_segments(ps, pe, pb, ns, ne, nb, hint);
                // Propagate bulges from the offset segments.
                out_bulges[i - 1] = pb;
                out_bulges[i] = nb;
                hit.or(Some(hint))
            }
            (Some((_, pe, pb)), None) => {
                out_bulges[i - 1] = pb;
                Some(pe)
            }
            (None, Some((ns, _, nb))) => {
                out_bulges[i] = nb;
                Some(ns)
            }
            (None, None) => Some(points[i]),
        };

        out_pts[i] = joined.unwrap_or(hint);

        // Keep prev-segment bulge consistent even when intersection failed.
        if let Some((_, _, pb)) = prev_off {
            out_bulges[i - 1] = pb;
        }
        let _ = (prev, next); // retained for readability / future joint heuristics
    }

    // Last segment bulge already set when n==2; for longer polys set from last off seg.
    if n > 2 {
        if let Some((_, _, b)) = off_segs[n - 2] {
            out_bulges[n - 2] = b;
        }
    }

    (out_pts, out_bulges)
}

fn intersect_offset_segments(
    a1: (f64, f64),
    a2: (f64, f64),
    a_bulge: f64,
    b1: (f64, f64),
    b2: (f64, f64),
    b_bulge: f64,
    hint: (f64, f64),
) -> Option<(f64, f64)> {
    let a_is_arc = a_bulge.abs() > EPS;
    let b_is_arc = b_bulge.abs() > EPS;

    match (a_is_arc, b_is_arc) {
        (false, false) => line_line_intersection(a1, a2, b1, b2),
        (true, false) => {
            let arc = bulge_to_arc(a1, a2, a_bulge)?;
            // Prefer arc ∩ infinite line; if empty (numerical), fall back to
            // unconstrained circle∩line by temporarily using a full arc.
            let mut hits = arc_line_intersection(&arc, b1, b2);
            if hits.is_empty() {
                let full = CircularArc {
                    ccw: true,
                    end_angle: arc.start_angle + 2.0 * PI,
                    ..arc
                };
                hits = arc_line_intersection(&full, b1, b2);
            }
            // G1 joints often sit exactly at a shared offset endpoint — prefer
            // those candidates to avoid quadratic round-off.
            hits.push(a1);
            hits.push(a2);
            hits.push(b1);
            hits.push(b2);
            closest_to(hint, &hits)
        }
        (false, true) => {
            let arc = bulge_to_arc(b1, b2, b_bulge)?;
            let mut hits = arc_line_intersection(&arc, a1, a2);
            if hits.is_empty() {
                let full = CircularArc {
                    ccw: true,
                    end_angle: arc.start_angle + 2.0 * PI,
                    ..arc
                };
                hits = arc_line_intersection(&full, a1, a2);
            }
            hits.push(a1);
            hits.push(a2);
            hits.push(b1);
            hits.push(b2);
            closest_to(hint, &hits)
        }
        (true, true) => {
            let arc_a = bulge_to_arc(a1, a2, a_bulge)?;
            let arc_b = bulge_to_arc(b1, b2, b_bulge)?;
            let mut hits = arc_arc_intersection(&arc_a, &arc_b);
            if hits.is_empty() {
                // Fall back to full circles when the joint sits just outside
                // a tight angular tolerance.
                let full_a = CircularArc {
                    ccw: true,
                    end_angle: arc_a.start_angle + 2.0 * PI,
                    ..arc_a
                };
                let full_b = CircularArc {
                    ccw: true,
                    end_angle: arc_b.start_angle + 2.0 * PI,
                    ..arc_b
                };
                hits = arc_arc_intersection(&full_a, &full_b);
            }
            closest_to(hint, &hits)
        }
    }
}

/// Reverse an open polyline's bulge list for the reversed point order.
///
/// Segment `points[i] → points[i+1]` with bulge `b` becomes
/// `points[i+1] → points[i]` with bulge `-b` after reversal.
pub fn reverse_bulges(bulges: &[f64], point_count: usize) -> Vec<f64> {
    if point_count == 0 {
        return Vec::new();
    }
    let mut out = vec![0.0; point_count];
    // Original segment i (bulge i) becomes reversed segment (n-2-i).
    let seg_count = point_count.saturating_sub(1);
    for i in 0..seg_count {
        let b = bulges.get(i).copied().unwrap_or(0.0);
        let rev_i = seg_count - 1 - i;
        out[rev_i] = -b;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn approx_pt(a: (f64, f64), b: (f64, f64)) -> bool {
        approx(a.0, b.0) && approx(a.1, b.1)
    }

    #[test]
    fn bulge_semicircle_unit() {
        // Diameter from (-1,0) to (1,0), upper semicircle: bulge = tan(π/4) = 1.
        let start = (-1.0, 0.0);
        let end = (1.0, 0.0);
        let bulge = 1.0;
        let arc = bulge_to_arc(start, end, bulge).expect("arc");
        assert!(approx(arc.center.0, 0.0));
        assert!(approx(arc.center.1, 0.0));
        assert!(approx(arc.radius, 1.0));
        assert!(arc.ccw);
        // CCW from (-1,0) to (1,0) sweeps the lower semicircle.
        assert!(approx_pt(arc_point_at(&arc, 0.5), (0.0, -1.0)));

        let (s, e, b) = arc_to_bulge(&arc);
        assert!(approx_pt(s, start));
        assert!(approx_pt(e, end));
        assert!(approx(b, 1.0));
    }

    #[test]
    fn bulge_quarter_circle() {
        // (1,0) → (0,1), CCW quarter about origin: included = π/2, bulge = tan(π/8).
        let start = (1.0, 0.0);
        let end = (0.0, 1.0);
        let bulge = (PI / 8.0).tan();
        let arc = bulge_to_arc(start, end, bulge).expect("arc");
        assert!(approx(arc.center.0, 0.0));
        assert!(approx(arc.center.1, 0.0));
        assert!(approx(arc.radius, 1.0));
        assert!(approx(arc.included_angle(), PI / 2.0));
    }

    #[test]
    fn offset_arc_ccw_positive_decreases_radius() {
        let arc = bulge_to_arc((-1.0, 0.0), (1.0, 0.0), 1.0).unwrap();
        let off = offset_arc(&arc, 0.25).expect("offset");
        // CCW: left is inward → radius 0.75, same center.
        assert!(approx(off.center.0, 0.0) && approx(off.center.1, 0.0));
        assert!(approx(off.radius, 0.75));
        assert!(off.ccw);
        let (s, e, b) = arc_to_bulge(&off);
        assert!(approx(b, 1.0)); // included angle unchanged
        assert!(approx_pt(s, (-0.75, 0.0)));
        assert!(approx_pt(e, (0.75, 0.0)));
    }

    #[test]
    fn offset_arc_ccw_negative_increases_radius() {
        let arc = bulge_to_arc((-1.0, 0.0), (1.0, 0.0), 1.0).unwrap();
        let off = offset_arc(&arc, -0.5).expect("offset");
        assert!(approx(off.radius, 1.5));
        assert!(approx_pt(off.start_point(), (-1.5, 0.0)));
        assert!(approx_pt(off.end_point(), (1.5, 0.0)));
    }

    #[test]
    fn offset_bulge_segment_straight() {
        let (s, e, b) = offset_bulge_segment((0.0, 0.0), (10.0, 0.0), 0.0, 0.5).unwrap();
        assert!(approx_pt(s, (0.0, 0.5)));
        assert!(approx_pt(e, (10.0, 0.5)));
        assert!(approx(b, 0.0));
    }

    #[test]
    fn arc_line_intersection_diameter() {
        let arc = bulge_to_arc((-1.0, 0.0), (1.0, 0.0), 1.0).unwrap();
        // Vertical line x=0 should hit the bottom of the CCW semicircle.
        let hits = arc_line_intersection(&arc, (0.0, -2.0), (0.0, 2.0));
        assert_eq!(hits.len(), 1);
        assert!(approx_pt(hits[0], (0.0, -1.0)));
    }

    #[test]
    fn arc_line_intersection_misses_outside_sweep() {
        let arc = bulge_to_arc((-1.0, 0.0), (1.0, 0.0), 1.0).unwrap(); // upper semi
        // Vertical line x=0 already tested; horizontal through center hits ends.
        let hits = arc_line_intersection(&arc, (-2.0, 0.0), (2.0, 0.0));
        assert_eq!(hits.len(), 2);
        // Upper half-circle point (0,+1) must NOT appear on this CCW semi.
        assert!(hits.iter().all(|p| p.1 <= 1e-9));
    }

    #[test]
    fn arc_arc_intersection_two_unit_circles() {
        // Unit circle arc right half about (0,0) and left-ish about (1,0).
        let a = CircularArc {
            center: (0.0, 0.0),
            radius: 1.0,
            start_angle: -PI / 2.0,
            end_angle: PI / 2.0,
            ccw: true,
        };
        let b = CircularArc {
            center: (1.0, 0.0),
            radius: 1.0,
            start_angle: PI / 2.0,
            end_angle: 3.0 * PI / 2.0,
            ccw: true,
        };
        let hits = arc_arc_intersection(&a, &b);
        assert_eq!(hits.len(), 2);
        // Known intersections at (0.5, ±√3/2).
        let y = (3.0_f64).sqrt() * 0.5;
        assert!(hits.iter().any(|p| approx_pt(*p, (0.5, y))));
        assert!(hits.iter().any(|p| approx_pt(*p, (0.5, -y))));
    }

    #[test]
    fn arc_arc_intersection_no_hit() {
        let a = bulge_to_arc((-1.0, 0.0), (1.0, 0.0), 1.0).unwrap();
        let b = bulge_to_arc((10.0, 0.0), (12.0, 0.0), 1.0).unwrap();
        assert!(arc_arc_intersection(&a, &b).is_empty());
    }

    #[test]
    fn offset_polyline_straight_matches_parallel() {
        let pts = [(0.0, 0.0), (10.0, 0.0)];
        let (off, bulges) = offset_polyline_with_bulges(&pts, &[0.0, 0.0], 0.5);
        assert!(approx_pt(off[0], (0.0, 0.5)));
        assert!(approx_pt(off[1], (10.0, 0.5)));
        assert!(approx(bulges[0], 0.0));
    }

    #[test]
    fn offset_polyline_single_arc() {
        // Semicircle diameter on x-axis, offset inward by 0.25.
        let pts = [(-1.0, 0.0), (1.0, 0.0)];
        let (off, bulges) = offset_polyline_with_bulges(&pts, &[1.0], 0.25);
        assert!(approx_pt(off[0], (-0.75, 0.0)));
        assert!(approx_pt(off[1], (0.75, 0.0)));
        assert!(approx(bulges[0], 1.0));
        let arc = bulge_to_arc(off[0], off[1], bulges[0]).unwrap();
        assert!(approx(arc.radius, 0.75));
        assert!(approx(arc.center.0, 0.0) && approx(arc.center.1, 0.0));
    }

    #[test]
    fn reverse_bulges_negates_and_reorders() {
        let rev = reverse_bulges(&[0.5, -0.25, 0.0], 3);
        assert!(approx(rev[0], 0.25));
        assert!(approx(rev[1], -0.5));
        assert!(approx(rev[2], 0.0));
    }
}
