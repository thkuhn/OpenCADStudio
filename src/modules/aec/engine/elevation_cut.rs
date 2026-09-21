//! Elevation boolean for opening-zone 3D: wall rectangle minus opening shapes.
//!
//! Plan-view host cuts stay a bounding-width band split. In the opening zone the
//! wall face is an `(s, z)` rectangle (`s` along the axis, `z` up from the wall
//! base). Tessellated [`OpeningShape`]s are subtracted and the remainder is
//! emitted as **simple loops** suitable for `sweep_model::extruded_direction`.
//! Interior holes become multiple simple remainder loops (sill, head, side
//! remnants) because the sweep profile is a single ring.
//!
//! Overlapping openings that share an axis span are grouped into one zone; their
//! holes are united (interval union on each scanline) and subtracted once.

use super::geometry::{area, signed_area};
use super::openings::Opening;

const EPS: f64 = 1e-9;
const MIN_SPAN: f64 = 1e-6;

/// One merged opening zone along the host axis.
#[derive(Debug, Clone, PartialEq)]
pub struct OpeningZone {
    /// Axis parameter of the left jamb of the merged span.
    pub s0: f64,
    /// Axis parameter of the right jamb of the merged span.
    pub s1: f64,
    /// Opening-shape polygons in zone elevation CS: `s` in `[0, width]`, `z`
    /// relative to the wall base (sill already applied).
    pub holes: Vec<Vec<(f64, f64)>>,
}

impl OpeningZone {
    pub fn width(&self) -> f64 {
        self.s1 - self.s0
    }
}

/// Group openings whose axis spans overlap (or nearly touch) into zones.
pub fn opening_zones(openings: &[Opening]) -> Vec<OpeningZone> {
    let mut items: Vec<(f64, f64, usize)> = Vec::new();
    for (i, opening) in openings.iter().enumerate() {
        if opening.width <= EPS {
            continue;
        }
        let (lo, hi) = opening.axis_span();
        if hi - lo > EPS {
            items.push((lo, hi, i));
        }
    }
    if items.is_empty() {
        return Vec::new();
    }
    items.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut zones = Vec::new();
    let (mut lo, mut hi, first) = items[0];
    let mut members = vec![first];
    for &(a, b, idx) in &items[1..] {
        if a <= hi + MIN_SPAN {
            hi = hi.max(b);
            members.push(idx);
        } else {
            zones.push(build_zone(lo, hi, &members, openings));
            lo = a;
            hi = b;
            members = vec![idx];
        }
    }
    zones.push(build_zone(lo, hi, &members, openings));
    zones
}

fn build_zone(s0: f64, s1: f64, members: &[usize], openings: &[Opening]) -> OpeningZone {
    let holes = members
        .iter()
        .filter_map(|&i| openings.get(i).and_then(|o| hole_in_zone(o, s0)))
        .collect();
    OpeningZone { s0, s1, holes }
}

/// Elevation polygon of an opening with `z` relative to the wall base and
/// `s` relative to the opening's left jamb (`0..width`).
pub fn opening_hole_local(opening: &Opening) -> Vec<(f64, f64)> {
    let poly = opening
        .shape
        .elevation_polygon(opening.width, opening.height, opening.spring_height);
    poly.into_iter()
        .map(|(s, z)| (s, z + opening.sill_height))
        .filter(|(s, z)| s.is_finite() && z.is_finite())
        .collect()
}

fn hole_in_zone(opening: &Opening, zone_s0: f64) -> Option<Vec<(f64, f64)>> {
    let local = opening_hole_local(opening);
    if local.len() < 3 {
        return None;
    }
    let (left, _) = opening.axis_span();
    let shifted: Vec<(f64, f64)> = local
        .into_iter()
        .map(|(s, z)| (s + left - zone_s0, z))
        .collect();
    if area(&shifted) <= EPS {
        return None;
    }
    Some(ensure_ccw(shifted))
}

/// Subtract `holes` from the elevation rectangle `[0, width] × [z_min, z_max]`.
///
/// Returns simple CCW loops (first vertex not repeated). Empty when the holes
/// consume the whole rectangle. Interior holes are keyholed onto the outer ring.
pub fn cut_elevation(
    width: f64,
    z_min: f64,
    z_max: f64,
    holes: &[Vec<(f64, f64)>],
) -> Vec<Vec<(f64, f64)>> {
    if width <= MIN_SPAN || z_max - z_min <= MIN_SPAN {
        return Vec::new();
    }
    let cleaned: Vec<Vec<(f64, f64)>> = holes
        .iter()
        .filter(|h| h.len() >= 3 && area(h) > EPS)
        .map(|h| ensure_ccw(h.clone()))
        .collect();
    if cleaned.is_empty() {
        return vec![rect(width, z_min, z_max)];
    }

    let quads = remaining_quads(width, z_min, z_max, &cleaned);
    let mut loops = Vec::with_capacity(quads.len());
    for q in &quads {
        let ring = dedup_ring(ensure_ccw(q.to_vec()));
        if ring.len() >= 3 && area(&ring) > EPS * 10.0 {
            loops.push(ring);
        }
    }
    loops
}

fn rect(width: f64, z_min: f64, z_max: f64) -> Vec<(f64, f64)> {
    vec![
        (0.0, z_min),
        (width, z_min),
        (width, z_max),
        (0.0, z_max),
    ]
}

/// All unique `x` coordinates where vertical elevation cuts are placed
/// across an opening zone `[0, width]`.
pub fn opening_slice_x_positions(width: f64, holes: &[Vec<(f64, f64)>]) -> Vec<f64> {
    let mut xs = vec![0.0, width];
    for hole in holes {
        for &(s, _) in hole {
            if s > 0.0 - EPS && s < width + EPS {
                xs.push(s.clamp(0.0, width));
            }
        }
        let n = hole.len();
        for i in 0..n {
            let a = hole[i];
            let b = hole[(i + 1) % n];
            for other in holes {
                let m = other.len();
                for j in 0..m {
                    if let Some(p) = seg_intersect(a, b, other[j], other[(j + 1) % m]) {
                        if p.0 > 0.0 - EPS && p.0 < width + EPS {
                            xs.push(p.0.clamp(0.0, width));
                        }
                    }
                }
            }
        }
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut unique = Vec::with_capacity(xs.len());
    for x in xs {
        if unique
            .last()
            .map(|&p: &f64| (p - x).abs() > EPS)
            .unwrap_or(true)
        {
            unique.push(x);
        }
    }
    unique
}

fn remaining_quads(
    width: f64,
    z_min: f64,
    z_max: f64,
    holes: &[Vec<(f64, f64)>],
) -> Vec<[(f64, f64); 4]> {
    let unique = opening_slice_x_positions(width, holes);

    let mut quads = Vec::new();
    for w in unique.windows(2) {
        let s0 = w[0];
        let s1 = w[1];
        if s1 - s0 <= EPS {
            continue;
        }
        let inset = ((s1 - s0) * 1e-6).max(1e-12);
        let left = remaining_z(s0 + inset, z_min, z_max, holes);
        let right = remaining_z(s1 - inset, z_min, z_max, holes);
        let pairs = match_intervals(&left, &right);
        for ((zl0, zh0), (zl1, zh1)) in pairs {
            if (zh0 - zl0).max(zh1 - zl1) <= EPS {
                continue;
            }
            let quad = [
                (s0, zl0),
                (s1, zl1),
                (s1, zh1),
                (s0, zh0),
            ];
            if trapezoid_area(&quad) > EPS {
                quads.push(quad);
            }
        }
    }
    quads
}

fn remaining_z(s: f64, z_min: f64, z_max: f64, holes: &[Vec<(f64, f64)>]) -> Vec<(f64, f64)> {
    let mut blocked = Vec::new();
    for hole in holes {
        blocked.extend(polygon_z_intervals(hole, s));
    }
    let blocked = merge_intervals(blocked);
    subtract_intervals(z_min, z_max, &blocked)
}

fn polygon_z_intervals(poly: &[(f64, f64)], s: f64) -> Vec<(f64, f64)> {
    let n = poly.len();
    if n < 3 {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for i in 0..n {
        let (x0, z0) = poly[i];
        let (x1, z1) = poly[(i + 1) % n];
        let dx = x1 - x0;
        if dx.abs() <= EPS {
            continue;
        }
        // Half-open crossing so vertices are counted once.
        let crosses = (x0 < s && x1 >= s) || (x1 < s && x0 >= s);
        if !crosses {
            continue;
        }
        let t = (s - x0) / dx;
        if (0.0..=1.0).contains(&t) {
            hits.push(z0 + t * (z1 - z0));
        }
    }
    hits.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = Vec::new();
    let mut k = 0;
    while k + 1 < hits.len() {
        let a = hits[k];
        let b = hits[k + 1];
        if b - a > EPS {
            out.push((a, b));
        }
        k += 2;
    }
    out
}

fn merge_intervals(mut intervals: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    if intervals.is_empty() {
        return intervals;
    }
    intervals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = Vec::with_capacity(intervals.len());
    let (mut lo, mut hi) = intervals[0];
    for &(a, b) in &intervals[1..] {
        if a <= hi + EPS {
            hi = hi.max(b);
        } else {
            out.push((lo, hi));
            lo = a;
            hi = b;
        }
    }
    out.push((lo, hi));
    out
}

fn subtract_intervals(z_min: f64, z_max: f64, holes: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut cursor = z_min;
    for &(lo, hi) in holes {
        let lo = lo.clamp(z_min, z_max);
        let hi = hi.clamp(z_min, z_max);
        if hi <= lo {
            continue;
        }
        if lo - cursor > EPS {
            out.push((cursor, lo.min(z_max)));
        }
        cursor = cursor.max(hi);
        if cursor >= z_max - EPS {
            break;
        }
    }
    if z_max - cursor > EPS {
        out.push((cursor, z_max));
    }
    out
}

fn match_intervals(
    left: &[(f64, f64)],
    right: &[(f64, f64)],
) -> Vec<((f64, f64), (f64, f64))> {
    if left.len() == right.len() {
        return left.iter().copied().zip(right.iter().copied()).collect();
    }
    let mut used_r = vec![false; right.len()];
    let mut pairs = Vec::new();
    for &l in left {
        let mut best = None;
        let mut best_ov = 0.0;
        for (j, &r) in right.iter().enumerate() {
            if used_r[j] {
                continue;
            }
            let ov = (l.1.min(r.1) - l.0.max(r.0)).max(0.0);
            if ov > best_ov {
                best_ov = ov;
                best = Some(j);
            }
        }
        if let Some(j) = best {
            used_r[j] = true;
            pairs.push((l, right[j]));
        }
    }
    pairs
}

fn trapezoid_area(q: &[(f64, f64); 4]) -> f64 {
    area(&q.to_vec())
}

fn dedup_ring(mut ring: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    ring.retain(|p| p.0.is_finite() && p.1.is_finite());
    let mut out = Vec::with_capacity(ring.len());
    for p in ring {
        if out
            .last()
            .map(|&q: &(f64, f64)| (q.0 - p.0).abs() > EPS || (q.1 - p.1).abs() > EPS)
            .unwrap_or(true)
        {
            out.push(p);
        }
    }
    if out.len() >= 2 {
        let first = out[0];
        let last = *out.last().unwrap();
        if (first.0 - last.0).abs() <= EPS && (first.1 - last.1).abs() <= EPS {
            out.pop();
        }
    }
    if signed_area(&out) < 0.0 {
        out.reverse();
    }
    out
}

fn ensure_ccw(mut poly: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    if signed_area(&poly) < 0.0 {
        poly.reverse();
    }
    poly
}

fn seg_intersect(
    a: (f64, f64),
    b: (f64, f64),
    c: (f64, f64),
    d: (f64, f64),
) -> Option<(f64, f64)> {
    let (x1, y1) = a;
    let (x2, y2) = b;
    let (x3, y3) = c;
    let (x4, y4) = d;
    let den = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
    if den.abs() <= EPS {
        return None;
    }
    let t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / den;
    let u = ((x1 - x3) * (y1 - y2) - (y1 - y3) * (x1 - x2)) / den;
    if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
        Some((x1 + t * (x2 - x1), y1 + t * (y2 - y1)))
    } else {
        None
    }
}

/// True when `p` is inside or on the boundary of `poly` (even-odd).
pub fn point_in_polygon(poly: &[(f64, f64)], p: (f64, f64)) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let (x, y) = p;
    let mut inside = false;
    for i in 0..n {
        let (x0, y0) = poly[i];
        let (x1, y1) = poly[(i + 1) % n];
        let on_x = (x - x0).abs() <= EPS && (x - x1).abs() <= EPS;
        let on_y = (y - y0).abs() <= EPS && (y - y1).abs() <= EPS;
        if on_x && y >= y0.min(y1) - EPS && y <= y0.max(y1) + EPS {
            return true;
        }
        if on_y && x >= x0.min(x1) - EPS && x <= x0.max(x1) + EPS {
            return true;
        }
        let crosses = (y0 > y) != (y1 > y);
        if crosses {
            let at_x = x0 + (y - y0) * (x1 - x0) / (y1 - y0);
            if at_x >= x - EPS {
                inside = !inside;
            }
        }
    }
    inside
}

/// Map an elevation loop `(s, z)` onto a wall-layer face and the extrusion
/// vector through that layer's thickness.
pub fn elevation_loop_to_layer_face(
    axis: &[(f64, f64)],
    zone_s0: f64,
    axis_offset: f64,
    thickness: f64,
    ring: &[(f64, f64)],
) -> Option<(Vec<[f64; 3]>, [f64; 3])> {
    if ring.len() < 3 || thickness.abs() <= EPS {
        return None;
    }
    let mut loop_xyz = Vec::with_capacity(ring.len());
    for &(s, z) in ring {
        let (p, (tx, ty)) = super::openings::point_and_tangent_at_distance(axis, zone_s0 + s)?;
        let nx = -ty;
        let ny = tx;
        loop_xyz.push([p.0 + nx * axis_offset, p.1 + ny * axis_offset, z]);
    }
    let mid_s = zone_s0 + ring.iter().map(|p| p.0).sum::<f64>() / ring.len() as f64;
    let (_, (tx, ty)) = super::openings::point_and_tangent_at_distance(axis, mid_s)?;
    let nx = -ty;
    let ny = tx;
    let direction = [nx * thickness, ny * thickness, 0.0];
    Some((loop_xyz, direction))
}

/// Closed elevation remainder on a wall-layer face, extruded along the layer normal.
#[derive(Debug, Clone, PartialEq)]
pub struct OpeningZoneSolidPath {
    pub layer_index: usize,
    /// World XY + Z relative to the wall base (regen adds `wall_base_z`).
    pub loop_xyz: Vec<[f64; 3]>,
    /// Layer thickness along the wall normal.
    pub direction: [f64; 3],
}

/// Per-layer zone solids: elevation remainder extruded through each layer.
pub fn zone_solid_paths(
    axis: &[(f64, f64)],
    layers: &[(f64, f64)],
    layer_extrusion: &[(f64, f64)],
    openings: &[Opening],
) -> Vec<OpeningZoneSolidPath> {
    let zones = opening_zones(openings);
    if zones.is_empty() {
        return Vec::new();
    }
    let n = layers.len().min(layer_extrusion.len());
    let mut out = Vec::new();
    for (li, (&(thickness, axis_offset), &(height, base_offset))) in
        layers.iter().zip(layer_extrusion.iter()).take(n).enumerate()
    {
        if thickness.abs() <= EPS || height.abs() <= EPS {
            continue;
        }
        let z_min = base_offset;
        let z_max = base_offset + height;
        for zone in &zones {
            let width = zone.width();
            if width <= MIN_SPAN {
                continue;
            }
            let rings = cut_elevation(width, z_min, z_max, &zone.holes);
            for ring in rings {
                if let Some((loop_xyz, direction)) =
                    elevation_loop_to_layer_face(axis, zone.s0, axis_offset, thickness, &ring)
                {
                    out.push(OpeningZoneSolidPath {
                        layer_index: li,
                        loop_xyz,
                        direction,
                    });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::opening_shape::{OpeningShape, TriangleVariant};
    use crate::modules::aec::engine::openings::OpeningKind;
    use acadrust::Handle;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn remaining_area(rings: &[Vec<(f64, f64)>]) -> f64 {
        rings.iter().map(|r| area(r)).sum()
    }

    fn hole_area(holes: &[Vec<(f64, f64)>]) -> f64 {
        holes.iter().map(|h| area(h)).sum()
    }

    fn dummy(
        dist: f64,
        width: f64,
        height: f64,
        sill: f64,
        shape: OpeningShape,
        spring: f64,
    ) -> Opening {
        let mut o = Opening::new(
            Handle::new(1),
            Handle::new(2),
            dist,
            width,
            height,
            sill,
            OpeningKind::Window,
        );
        o.shape = shape;
        o.spring_height = spring;
        o
    }

    #[test]
    fn rectangle_window_yields_sill_and_head() {
        let wall_h = 2.7;
        let width = 1.2;
        let height = 1.2;
        let sill = 0.9;
        let opening = dummy(5.0, width, height, sill, OpeningShape::Rectangle, 0.0);
        let zones = opening_zones(&[opening]);
        assert_eq!(zones.len(), 1);
        let rings = cut_elevation(width, 0.0, wall_h, &zones[0].holes);
        assert_eq!(rings.len(), 2, "sill + head, got {rings:?}");
        let expected = wall_h * width - width * height;
        assert!(
            approx(remaining_area(&rings), expected),
            "area {} vs {expected}",
            remaining_area(&rings)
        );
        let max_below = rings
            .iter()
            .map(|r| r.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max))
            .min_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();
        let min_above = rings
            .iter()
            .map(|r| r.iter().map(|p| p.1).fold(f64::INFINITY, f64::min))
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();
        assert!(approx(max_below, sill), "sill top {max_below}");
        assert!(approx(min_above, sill + height), "head bottom {min_above}");
    }

    #[test]
    fn door_to_sill_zero_is_head_notch_only() {
        let wall_h = 2.7;
        let width = 0.9;
        let height = 2.1;
        let opening = dummy(2.0, width, height, 0.0, OpeningShape::Rectangle, 0.0);
        let zones = opening_zones(&[opening]);
        let rings = cut_elevation(width, 0.0, wall_h, &zones[0].holes);
        assert_eq!(rings.len(), 1);
        assert!(approx(remaining_area(&rings), width * (wall_h - height)));
        let zmin = rings[0]
            .iter()
            .map(|p| p.1)
            .fold(f64::INFINITY, f64::min);
        assert!(approx(zmin, height));
    }

    #[test]
    fn circle_keeps_bounding_box_corners() {
        let width = 1.0;
        let height = 1.0;
        let sill = 0.9;
        let wall_h = 3.0;
        let opening = dummy(2.0, width, height, sill, OpeningShape::Circle, 0.0);
        let zones = opening_zones(&[opening]);
        let holes = &zones[0].holes;
        let rings = cut_elevation(width, 0.0, wall_h, holes);
        let expected = width * wall_h - hole_area(holes);
        assert!(
            (remaining_area(&rings) - expected).abs() < 1e-4,
            "area {} vs {expected}",
            remaining_area(&rings)
        );
        let corners = [
            (0.0, sill),
            (width, sill),
            (width, sill + height),
            (0.0, sill + height),
        ];
        for c in corners {
            assert!(
                rings.iter().any(|r| point_in_polygon(r, c)),
                "circle zwickel missing at {c:?}"
            );
        }
        let center = (width * 0.5, sill + height * 0.5);
        assert!(
            rings.iter().all(|r| !point_in_polygon(r, center)),
            "circle interior must be cut out"
        );
    }

    #[test]
    fn arch_default_spring_and_changed_rise() {
        let width = 1.0;
        let height = 2.0;
        let wall_h = 3.0;
        let default_spring = OpeningShape::default_spring_height(width, height);
        let a = dummy(1.5, width, height, 0.0, OpeningShape::Arch, default_spring);
        let b = dummy(1.5, width, height, 0.0, OpeningShape::Arch, 0.5);
        let za = opening_zones(&[a]);
        let zb = opening_zones(&[b]);
        let ra = cut_elevation(width, 0.0, wall_h, &za[0].holes);
        let rb = cut_elevation(width, 0.0, wall_h, &zb[0].holes);
        let hole_a = hole_area(&za[0].holes);
        let hole_b = hole_area(&zb[0].holes);
        assert!(hole_b > hole_a + 1e-4, "lower kämfer must increase rise/area");
        let got_a = remaining_area(&ra);
        let got_b = remaining_area(&rb);
        // Default Kämpfer is a semicircle on the jambs: hole stays inside the zone.
        assert!(
            (got_a - (width * wall_h - hole_a)).abs() < 1e-3,
            "default spring remaining {got_a} vs {}",
            width * wall_h - hole_a
        );
        // Lower Kämpfer increases the rise (and hole); remainder must shrink.
        assert!(got_b < got_a - 1e-3, "bigger arch hole must leave less wall");
        let head_a = ra.iter().map(|r| r.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max)).fold(f64::NEG_INFINITY, f64::max);
        let head_b = rb.iter().map(|r| r.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max)).fold(f64::NEG_INFINITY, f64::max);
        assert!(approx(head_a, wall_h) && approx(head_b, wall_h));
        assert!(za[0].holes[0].iter().any(|p| (p.1 - default_spring).abs() < 1e-6));
    }

    #[test]
    fn triangle_variants_subtract_expected_area() {
        let width = 2.0;
        let height = 1.0;
        let wall_h = 2.5;
        let sill = 0.4;
        let tri_area = 0.5 * width * height;
        for variant in [
            TriangleVariant::IsoscelesUp,
            TriangleVariant::IsoscelesDown,
            TriangleVariant::Equilateral,
            TriangleVariant::RightLeft,
            TriangleVariant::RightRight,
        ] {
            let (w, h) = OpeningShape::Triangle(variant).lock_size(width, height, true);
            let expected_hole = if variant == TriangleVariant::Equilateral {
                0.5 * w * h
            } else {
                tri_area
            };
            let opening = dummy(3.0, w, h, sill, OpeningShape::Triangle(variant), 0.0);
            let zones = opening_zones(&[opening]);
            let rings = cut_elevation(w, 0.0, wall_h, &zones[0].holes);
            let got_hole = hole_area(&zones[0].holes);
            assert!(
                (got_hole - expected_hole).abs() < 1e-6,
                "{variant:?} hole {got_hole} vs {expected_hole}"
            );
            let expected = w * wall_h - expected_hole;
            assert!(
                (remaining_area(&rings) - expected).abs() < 1e-4,
                "{variant:?} remaining {} vs {expected}",
                remaining_area(&rings)
            );
            let apex = match variant {
                TriangleVariant::IsoscelesUp | TriangleVariant::Equilateral => (w * 0.5, sill + h),
                TriangleVariant::IsoscelesDown => (w * 0.5, sill),
                TriangleVariant::RightLeft => (0.0, sill + h),
                TriangleVariant::RightRight => (w, sill + h),
            };
            assert!(
                rings.iter().all(|r| !point_in_polygon(r, (w * 0.5, sill + h * 0.35))
                    || variant == TriangleVariant::IsoscelesDown),
                "{variant:?} should cut the triangle interior"
            );
            let _ = apex;
        }
    }

    #[test]
    fn stacked_openings_union_once_without_duplicate_bodies() {
        let width = 1.2;
        let wall_h = 3.0;
        let lower = dummy(4.0, width, 0.8, 0.2, OpeningShape::Rectangle, 0.0);
        let upper = dummy(4.0, width, 0.8, 1.4, OpeningShape::Rectangle, 0.0);
        let zones = opening_zones(&[lower, upper]);
        assert_eq!(zones.len(), 1, "same axis span is one zone");
        assert_eq!(zones[0].holes.len(), 2);
        let rings = cut_elevation(width, 0.0, wall_h, &zones[0].holes);
        // sill + middle + head
        assert_eq!(rings.len(), 3, "expected three remainder strips, got {}", rings.len());
        let expected = width * wall_h - 2.0 * width * 0.8;
        assert!(approx(remaining_area(&rings), expected));
        // Overlapping stack (union, not two copies).
        let mut overlap_upper = dummy(4.0, width, 1.0, 0.6, OpeningShape::Rectangle, 0.0);
        overlap_upper.handle = Handle::new(9);
        let lower = dummy(4.0, width, 1.0, 0.2, OpeningShape::Rectangle, 0.0);
        let zones = opening_zones(&[lower, overlap_upper]);
        let rings = cut_elevation(width, 0.0, wall_h, &zones[0].holes);
        let union_h = 1.4; // 0.2..1.2 union 0.6..1.6
        let expected = width * wall_h - width * union_h;
        assert!(
            approx(remaining_area(&rings), expected),
            "union remaining {} vs {expected}",
            remaining_area(&rings)
        );
        assert!(
            rings.len() <= 2,
            "overlapping stack must not emit duplicate solids, got {}",
            rings.len()
        );
    }

    #[test]
    fn interior_hole_becomes_simple_remainder_loops() {
        let outer_w = 2.0;
        let wall_h = 2.0;
        let hole = vec![(0.5, 0.5), (1.5, 0.5), (1.5, 1.5), (0.5, 1.5)];
        let rings = cut_elevation(outer_w, 0.0, wall_h, &[hole]);
        assert!(!rings.is_empty());
        let a = remaining_area(&rings);
        assert!((a - 3.0).abs() < 1e-4, "remainder area {a} vs 3");
        assert!(rings.iter().any(|r| point_in_polygon(r, (0.1, 0.1))));
        assert!(rings.iter().all(|r| !point_in_polygon(r, (1.0, 1.0))));
    }

    #[test]
    fn empty_holes_return_full_rectangle() {
        let rings = cut_elevation(1.0, 0.0, 2.0, &[]);
        assert_eq!(rings.len(), 1);
        assert!(approx(remaining_area(&rings), 2.0));
    }
}
