//! 2D layer-contour generation for walls.
//!
//! This module provides functions to compute parallel offset polylines for
//! multi-layer walls. Straight axes use a per-vertex angle-bisector miter
//! (see [`super::geometry::get_offset_directions`]). Axes that carry LWPOLYLINE
//! bulges use exact circular-arc offsets from [`super::arc`].
//!
//! For an N-layer wall, this implementation returns N boundary pairs
//! (parallel offsets of the centerline).

use super::arc::{offset_polyline_with_bulges, reverse_bulges};
use super::geometry::get_offset_directions;

/// Open polyline with optional per-vertex bulges (LWPOLYLINE convention:
/// `bulges[i]` is the bulge from vertex `i` to `i+1`; missing = 0).
#[derive(Debug, Clone, PartialEq)]
pub struct OffsetPolyline {
    pub points: Vec<(f64, f64)>,
    pub bulges: Vec<f64>,
}

impl OffsetPolyline {
    pub fn straight(points: Vec<(f64, f64)>) -> Self {
        let n = points.len();
        Self {
            points,
            bulges: vec![0.0; n],
        }
    }
}

fn has_nonzero_bulge(bulges: &[f64]) -> bool {
    bulges.iter().any(|b| b.abs() > 1e-12)
}

fn normalize_bulges(point_count: usize, bulges: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; point_count];
    let n = point_count.min(bulges.len());
    out[..n].copy_from_slice(&bulges[..n]);
    out
}

/// Offset an open centerline by a signed distance (left-positive).
///
/// When `bulges` is empty or all zeros, uses the historical mitered vertex
/// offset so results stay bit-identical to pre-arc behaviour.
pub fn offset_centerline(
    centerline: &[(f64, f64)],
    bulges: &[f64],
    distance: f64,
) -> OffsetPolyline {
    let n = centerline.len();
    if n < 2 {
        return OffsetPolyline::straight(centerline.to_vec());
    }
    let bulges = normalize_bulges(n, bulges);
    if !has_nonzero_bulge(&bulges) {
        let directions = get_offset_directions(centerline);
        let points = centerline
            .iter()
            .zip(directions.iter())
            .map(|(&(x, y), &(dx, dy))| (x + dx * distance, y + dy * distance))
            .collect();
        return OffsetPolyline::straight(points);
    }
    let (points, out_bulges) = offset_polyline_with_bulges(centerline, &bulges, distance);
    OffsetPolyline {
        points,
        bulges: out_bulges,
    }
}

/// Computes parallel offset polylines (boundary pairs) for an open polyline
/// centerline and a list of layer `(thickness, axis_offset)` pairs.
///
/// For a list of N layers, it returns N boundary pairs. Each layer is placed
/// directly: start = `axis_offset`, end = `axis_offset + thickness` (no
/// cumulative stacking or automatic centering).
///
/// Corner handling uses a simple angle-bisector miter join. This may produce
/// minor imperfections at very sharp/acute corners, but matches the plan's
/// stated simplification for architectural walls.
///
/// Straight-only convenience wrapper — equivalent to
/// [`layer_contours_with_bulges`] with all-zero bulges.
pub fn layer_contours(
    centerline: &[(f64, f64)],
    layers: &[(f64, f64)],
) -> Vec<(Vec<(f64, f64)>, Vec<(f64, f64)>)> {
    layer_contours_with_bulges(centerline, &[], layers)
        .into_iter()
        .map(|(a, b)| (a.points, b.points))
        .collect()
}

/// Bulge-aware variant of [`layer_contours`].
///
/// `bulges` follows the LWPOLYLINE per-vertex convention (bulge from vertex
/// `i` to `i+1`). Empty / all-zero bulges take the historical straight path
/// so existing tests stay bit-identical.
///
/// Each boundary is returned as an [`OffsetPolyline`] so callers that rebuild
/// `LwPolyline` entities can preserve arc bulges on the offset edges.
///
/// `layers` entries are `(thickness, axis_offset)`.
pub fn layer_contours_with_bulges(
    centerline: &[(f64, f64)],
    bulges: &[f64],
    layers: &[(f64, f64)],
) -> Vec<(OffsetPolyline, OffsetPolyline)> {
    if centerline.len() < 2 {
        return Vec::new();
    }

    let mut results = Vec::with_capacity(layers.len());
    for &(thickness, axis_offset) in layers {
        let start_offset = axis_offset;
        let end_offset = axis_offset + thickness;

        let b1 = offset_centerline(centerline, bulges, start_offset);
        let b2 = offset_centerline(centerline, bulges, end_offset);
        results.push((b1, b2));
    }

    results
}

/// Computes the combined outer boundary polygon (closed) for an open polyline
/// centerline, total thickness, and an offset from the centerline.
///
/// The boundary is a single closed loop formed by the two outermost parallel
/// offset lines, capped at the ends.
///
/// Straight-only convenience wrapper — equivalent to
/// [`outer_contour_with_bulges`] with all-zero bulges (points only).
pub fn outer_contour(
    centerline: &[(f64, f64)],
    total_thickness: f64,
    centerline_offset: f64,
) -> Vec<(f64, f64)> {
    outer_contour_with_bulges(centerline, &[], total_thickness, centerline_offset).points
}

/// Bulge-aware outer boundary as a closed [`OffsetPolyline`].
///
/// Point order matches [`outer_contour`]: forward along the lower offset, then
/// back along the upper. End caps are straight (`bulge = 0`); arc bulges on
/// each side are preserved (reversed/negated on the return leg).
pub fn outer_contour_with_bulges(
    centerline: &[(f64, f64)],
    bulges: &[f64],
    total_thickness: f64,
    centerline_offset: f64,
) -> OffsetPolyline {
    if centerline.len() < 2 {
        return OffsetPolyline::straight(Vec::new());
    }

    let half_thickness = total_thickness * 0.5;
    let b1_offset = centerline_offset - half_thickness;
    let b2_offset = centerline_offset + half_thickness;

    let b1 = offset_centerline(centerline, bulges, b1_offset);
    let mut b2 = offset_centerline(centerline, bulges, b2_offset);

    let b2_bulges_rev = reverse_bulges(&b2.bulges, b2.points.len());
    b2.points.reverse();

    let mut points = b1.points;
    let mut out_bulges = b1.bulges;
    // Ensure bulge vec length matches points before the join.
    out_bulges.resize(points.len(), 0.0);
    // End-cap segment (last of b1 → first of reversed b2) is straight.
    if !out_bulges.is_empty() {
        let last = out_bulges.len() - 1;
        out_bulges[last] = 0.0;
    }

    let join_index = points.len();
    points.extend(b2.points);
    // b2 reversed bulges; the final vertex closes back to b1[0] with bulge 0.
    let mut rev = b2_bulges_rev;
    rev.resize(points.len() - join_index, 0.0);
    if !rev.is_empty() {
        let last = rev.len() - 1;
        rev[last] = 0.0;
    }
    out_bulges.extend(rev);
    out_bulges.resize(points.len(), 0.0);

    OffsetPolyline {
        points,
        bulges: out_bulges,
    }
}

/// Build a closed per-layer footprint (forward along inner boundary, back along
/// outer) including bulges suitable for an `LwPolyline`.
pub fn closed_layer_footprint(b1: &OffsetPolyline, b2: &OffsetPolyline) -> OffsetPolyline {
    let mut points = b1.points.clone();
    let mut bulges = b1.bulges.clone();
    bulges.resize(points.len(), 0.0);
    if !bulges.is_empty() {
        let last = bulges.len() - 1;
        bulges[last] = 0.0; // end cap
    }

    let mut b2_pts = b2.points.clone();
    let b2_rev_bulges = reverse_bulges(&b2.bulges, b2_pts.len());
    b2_pts.reverse();

    let join = points.len();
    points.extend(b2_pts);
    let mut rev = b2_rev_bulges;
    rev.resize(points.len() - join, 0.0);
    if !rev.is_empty() {
        let last = rev.len() - 1;
        rev[last] = 0.0; // start cap back to b1[0]
    }
    bulges.extend(rev);
    bulges.resize(points.len(), 0.0);

    OffsetPolyline { points, bulges }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::arc::{bulge_to_arc, arc_point_at};
    use std::f64::consts::PI;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-8
    }

    fn pts_close(a: &[(f64, f64)], b: &[(f64, f64)]) -> bool {
        a.len() == b.len()
            && a.iter()
                .zip(b.iter())
                .all(|(&(x0, y0), &(x1, y1))| approx(x0, x1) && approx(y0, y1))
    }

    #[test]
    fn straight_wall_two_layers() {
        // Horizontal wall along X axis from 0 to 10.
        // Layers: 0.1 and 0.2 with explicit axis offsets (centered stack).
        // Boundaries at Y = -0.15, -0.05, 0.15.
        let centerline = vec![(0.0, 0.0), (10.0, 0.0)];
        let layers = vec![(0.1, -0.15), (0.2, -0.05)];
        let contours = layer_contours(&centerline, &layers);

        assert_eq!(contours.len(), 2);
        
        // Layer 0: Y = -0.15 to -0.05
        assert!((contours[0].0[0].1 - (-0.15)).abs() < 1e-9);
        assert!((contours[0].1[0].1 - (-0.05)).abs() < 1e-9);

        // Layer 1: Y = -0.05 to 0.15
        assert!((contours[1].0[0].1 - (-0.05)).abs() < 1e-9);
        assert!((contours[1].1[0].1 - 0.15).abs() < 1e-9);
    }

    #[test]
    fn three_point_bend_centerline() {
        // L-bend: (0,0) -> (10,0) -> (10,10)
        // One layer of 0.2 centered on the axis.
        // Boundaries at -0.1 and 0.1.
        let centerline = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        let layers = vec![(0.2, -0.1)];
        let contours = layer_contours(&centerline, &layers);

        assert_eq!(contours.len(), 1);

        // Check first boundary (-0.1 offset)
        // Vertex 0: (0, 0) + (0, 1)*(-0.1) = (0, -0.1)
        assert!((contours[0].0[0].0 - 0.0).abs() < 1e-9);
        assert!((contours[0].0[0].1 - (-0.1)).abs() < 1e-9);

        // Vertex 1 (bend): (10, 0). 
        // Point = (10, 0) + (0.1, -0.1) = (10.1, -0.1)
        assert!((contours[0].0[1].0 - 10.1).abs() < 1e-9);
        assert!((contours[0].0[1].1 - (-0.1)).abs() < 1e-9);

        // Check second boundary (0.1 offset)
        // Vertex 1 bend: (10, 0) + (-0.1, 0.1) = (9.9, 0.1)
        assert!((contours[0].1[1].0 - 9.9).abs() < 1e-9);
        assert!((contours[0].1[1].1 - 0.1).abs() < 1e-9);
    }

    #[test]
    fn layer_with_asymmetric_axis_offset() {
        // Horizontal wall along X axis from 0 to 10.
        // Equivalent to the former gap layout (0.1@gap0 + 0.1@gap0.05 centered):
        // Layer 0: Y = -0.125 to -0.025
        // Air gap: Y = -0.025 to 0.025
        // Layer 1: Y = 0.025 to 0.125
        let centerline = vec![(0.0, 0.0), (10.0, 0.0)];
        let layers = vec![(0.1, -0.125), (0.1, 0.025)];
        let contours = layer_contours(&centerline, &layers);

        assert_eq!(contours.len(), 2);
        
        // Layer 0
        assert!((contours[0].0[0].1 - (-0.125)).abs() < 1e-9);
        assert!((contours[0].1[0].1 - (-0.025)).abs() < 1e-9);

        // Layer 1
        assert!((contours[1].0[0].1 - 0.025).abs() < 1e-9);
        assert!((contours[1].1[0].1 - 0.125).abs() < 1e-9);
    }

    #[test]
    fn negative_axis_offset_places_layer_on_negative_side() {
        let centerline = vec![(0.0, 0.0), (10.0, 0.0)];
        // Single layer entirely on the negative side of the axis.
        let layers = vec![(0.2, -0.3)];
        let contours = layer_contours(&centerline, &layers);
        assert_eq!(contours.len(), 1);
        assert!((contours[0].0[0].1 - (-0.3)).abs() < 1e-9);
        assert!((contours[0].1[0].1 - (-0.1)).abs() < 1e-9);
    }

    #[test]
    fn outer_contour_straight() {
        // Horizontal wall (0,0) -> (10,0), thickness 0.2, offset 0 (Center)
        let centerline = vec![(0.0, 0.0), (10.0, 0.0)];
        let poly = outer_contour(&centerline, 0.2, 0.0);

        // Expected 4 points: (0, -0.1), (10, -0.1), (10, 0.1), (0, 0.1)
        assert_eq!(poly.len(), 4);
        assert!((poly[0].1 - (-0.1)).abs() < 1e-9);
        assert!((poly[1].1 - (-0.1)).abs() < 1e-9);
        assert!((poly[2].1 - 0.1).abs() < 1e-9);
        assert!((poly[3].1 - 0.1).abs() < 1e-9);
    }

    #[test]
    fn outer_contour_l_shape_justified() {
        // L-bend: (0,0) -> (10,0) -> (10,10)
        // Thickness 0.2, Interior justification (-0.1 offset)
        let centerline = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        let poly = outer_contour(&centerline, 0.2, -0.1);

        // Axis is shifted by -0.1. Boundaries at -0.1 - 0.1 = -0.2 and -0.1 + 0.1 = 0.
        // Outer loop should have 6 points (3 per side).
        assert_eq!(poly.len(), 6);

        // Check b1 (offset -0.2)
        // Vertex 0: (0, 0) + (0, 1)*(-0.2) = (0, -0.2)
        assert!((poly[0].0 - 0.0).abs() < 1e-9);
        assert!((poly[0].1 - (-0.2)).abs() < 1e-9);

        // Check b2 (offset 0.0)
        // Vertex 0: (0, 0) + (0, 1)*(0) = (0, 0)
        assert!((poly[5].0 - 0.0).abs() < 1e-9);
        assert!((poly[5].1 - 0.0).abs() < 1e-9);
    }

    #[test]
    fn zero_bulge_path_bit_identical_to_direct_offset() {
        // Explicit axis offsets (equivalent to a former centered stack).
        let centerline = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        let layers = vec![(0.1, -0.175), (0.2, -0.025)];
        let direct = {
            let dirs = get_offset_directions(&centerline);
            let mut out = Vec::new();
            for &(t, axis) in &layers {
                let s = axis;
                let e = axis + t;
                let b1: Vec<_> = centerline
                    .iter()
                    .zip(dirs.iter())
                    .map(|(&(x, y), &(dx, dy))| (x + dx * s, y + dy * s))
                    .collect();
                let b2: Vec<_> = centerline
                    .iter()
                    .zip(dirs.iter())
                    .map(|(&(x, y), &(dx, dy))| (x + dx * e, y + dy * e))
                    .collect();
                out.push((b1, b2));
            }
            out
        };
        let via_api = layer_contours(&centerline, &layers);
        assert_eq!(via_api.len(), direct.len());
        for (a, b) in via_api.iter().zip(direct.iter()) {
            assert!(pts_close(&a.0, &b.0));
            assert!(pts_close(&a.1, &b.1));
        }
        // Explicit zero bulges must match too.
        let with_zeros = layer_contours_with_bulges(&centerline, &[0.0, 0.0, 0.0], &layers);
        for (a, b) in with_zeros.iter().zip(direct.iter()) {
            assert!(pts_close(&a.0.points, &b.0));
            assert!(pts_close(&a.1.points, &b.1));
        }
    }

    #[test]
    fn curved_single_layer_parallel_offset_arc() {
        // Axis: semicircle diameter (-1,0)→(1,0), bulge=1, radius 1 about origin.
        // Single layer thickness 0.2 centered → boundaries at offset ±0.1.
        // CCW arc: left(+)=inward. start_offset=-0.1 → radius 1.1;
        // end_offset=+0.1 → radius 0.9.
        let centerline = vec![(-1.0, 0.0), (1.0, 0.0)];
        let bulges = vec![1.0];
        let layers = vec![(0.2, -0.1)];
        let contours = layer_contours_with_bulges(&centerline, &bulges, &layers);
        assert_eq!(contours.len(), 1);

        let (inner, outer) = &contours[0];
        // "inner" here is the more-negative offset boundary (start_offset).
        let arc_lo = bulge_to_arc(inner.points[0], inner.points[1], inner.bulges[0]).unwrap();
        let arc_hi = bulge_to_arc(outer.points[0], outer.points[1], outer.bulges[0]).unwrap();

        assert!(approx(arc_lo.center.0, 0.0) && approx(arc_lo.center.1, 0.0));
        assert!(approx(arc_hi.center.0, 0.0) && approx(arc_hi.center.1, 0.0));
        assert!(approx(arc_lo.radius, 1.1));
        assert!(approx(arc_hi.radius, 0.9));
        // Bulge preserved (same included angle).
        assert!(approx(inner.bulges[0], 1.0));
        assert!(approx(outer.bulges[0], 1.0));
        // Mid-arc points sit on the offset circles (CCW semi → negative Y).
        assert!(approx(arc_point_at(&arc_lo, 0.5).1, -1.1));
        assert!(approx(arc_point_at(&arc_hi, 0.5).1, -0.9));
    }

    #[test]
    fn curved_to_straight_continuous_offset() {
        // Straight (0,0)→(1,0) then quarter-circle (1,0)→(2,1) about (1,1)?
        // Simpler G1 join: straight along +X into a CCW quarter that starts
        // with the same tangent.
        // Straight: (0,0) → (1,0). Arc: (1,0) → (1,1) about center (1,0)? 
        // That arc would have vertical tangent at start — not G1.
        //
        // G1 case: straight (0,0)→(1,0); arc quarter from (1,0) to (2,1)
        // about (1,1): start angle = -π/2, end = 0, CCW.
        // At (1,0) tangent of arc is +X (CCW from down), matches straight.
        let centerline = vec![(0.0, 0.0), (1.0, 0.0), (2.0, 1.0)];
        let bulge = (PI / 8.0).tan(); // quarter circle
        let bulges = vec![0.0, bulge];
        // Verify the arc geometry first.
        let arc = bulge_to_arc(centerline[1], centerline[2], bulge).unwrap();
        assert!(approx(arc.center.0, 1.0) && approx(arc.center.1, 1.0));
        assert!(approx(arc.radius, 1.0));

        let offset = 0.25;
        let off = offset_centerline(&centerline, &bulges, offset);
        assert_eq!(off.points.len(), 3);

        // At the transition vertex, straight-offset end and arc-offset start
        // must coincide (G1 continuity of the offset).
        let straight_end = (1.0, offset); // left of +X
        // Arc CCW about (1,1): left = inward, radius 1 - 0.25 = 0.75.
        // Start angle -π/2 → point (1, 1 - 0.75) = (1, 0.25).
        let arc_start = (1.0, 0.25);
        assert!(approx(straight_end.0, arc_start.0) && approx(straight_end.1, arc_start.1));
        assert!(approx(off.points[1].0, 1.0));
        assert!(approx(off.points[1].1, 0.25));

        // End of offset arc: angle 0 → (1 + 0.75, 1) = (1.75, 1).
        assert!(approx(off.points[2].0, 1.75));
        assert!(approx(off.points[2].1, 1.0));
        // Start of offset straight.
        assert!(approx(off.points[0].0, 0.0));
        assert!(approx(off.points[0].1, 0.25));
    }

    #[test]
    fn curved_multi_layer_increasing_radius() {
        // Same semicircle axis; two layers thickness 0.1 each, centered.
        // Offsets: -0.1, 0.0, +0.1 relative to axis.
        // Layer0 boundaries at -0.1 and 0.0 → radii 1.1 and 1.0
        // Layer1 boundaries at  0.0 and +0.1 → radii 1.0 and 0.9
        // (CCW: +offset decreases radius).
        let centerline = vec![(-1.0, 0.0), (1.0, 0.0)];
        let bulges = vec![1.0];
        let layers = vec![(0.1, -0.1), (0.1, 0.0)];
        let contours = layer_contours_with_bulges(&centerline, &bulges, &layers);
        assert_eq!(contours.len(), 2);

        let r = |poly: &OffsetPolyline| {
            bulge_to_arc(poly.points[0], poly.points[1], poly.bulges[0])
                .unwrap()
                .radius
        };

        let r0_lo = r(&contours[0].0);
        let r0_hi = r(&contours[0].1);
        let r1_lo = r(&contours[1].0);
        let r1_hi = r(&contours[1].1);

        assert!(approx(r0_lo, 1.1));
        assert!(approx(r0_hi, 1.0));
        assert!(approx(r1_lo, 1.0));
        assert!(approx(r1_hi, 0.9));
        // Each layer's outer (more positive offset / smaller radius for CCW)
        // sits inside the previous; radii decrease as we walk the stack in
        // the +offset direction.
        assert!(r0_lo > r0_hi && r1_lo > r1_hi);
        assert!(approx(r0_hi, r1_lo));
    }
}
