//! Wall openings (windows / doors).
//!
//! An [`Opening`] is a first-class AEC entity that references a host wall and a
//! position along that wall's axis. Pure geometry helpers here compute the 2D
//! opening footprint and subtract it from wall-band polygons for plan-view
//! contours and hatches.
//!
//! ## Contour semantics after subtraction
//!
//! A full-thickness opening through a wall band is **not** an interior hole —
//! it reaches both long sides of the band and splits the wall into disconnected
//! pieces (before / after the opening along the axis). The hatch renderer
//! (`HatchModel`) supports multi-ring boundaries via NaN separators and even-odd
//! fill, which works for islands/holes, but a through-cut is topologically two
//! outer rings, not an outer+hole pair. This module therefore returns
//! **disconnected pieces** (0, 1, or more closed polygons).
//!
//! ## Limitations (documented, by design for Step 4)
//!
//! - Subtraction is a **targeted wall-band split** along the axis parameter, not
//!   a general polygon boolean. It is correct for the rectangle-band contours
//!   produced by [`super::contour::outer_contour`] / layer footprints on straight
//!   (and bulge-approximated) axes. Self-intersecting or highly non-band polygons
//!   are out of scope.
//! - `distance_along_axis` uses **straight-segment chain length** along the axis
//!   polyline. Arc-axis openings use the same polyline chord length (not true
//!   arc length) — an acceptable approximation for placement; document callers
//!   that need arc-length precision can upgrade later.
//! - 3D solid cutting (sill/head box boolean) is deferred; 2D plan cutting is
//!   the primary deliverable.

use acadrust::Handle;

use super::contour::outer_contour;
use super::geometry::area;

/// Window or door.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpeningKind {
    Window,
    Door,
}

impl OpeningKind {
    pub fn as_str(self) -> &'static str {
        match self {
            OpeningKind::Window => "Window",
            OpeningKind::Door => "Door",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "Door" | "door" | "DOOR" => OpeningKind::Door,
            _ => OpeningKind::Window,
        }
    }
}

/// A wall opening (window or door) hosted by a wall axis entity.
///
/// `distance_along_axis` is the distance from the wall axis **start** vertex to
/// the opening's center, measured along the axis polyline as the sum of
/// straight segment lengths (chord length on arc segments).
#[derive(Debug, Clone, PartialEq)]
pub struct Opening {
    pub handle: Handle,
    pub host_wall: Handle,
    pub distance_along_axis: f64,
    pub width: f64,
    pub height: f64,
    pub sill_height: f64,
    pub kind: OpeningKind,
}

/// Default window width (drawing units / metres).
pub const DEFAULT_WINDOW_WIDTH: f64 = 1.2;
/// Default window height.
pub const DEFAULT_WINDOW_HEIGHT: f64 = 1.2;
/// Default window sill height above wall base.
pub const DEFAULT_WINDOW_SILL: f64 = 0.9;
/// Default door width.
pub const DEFAULT_DOOR_WIDTH: f64 = 0.9;
/// Default door height.
pub const DEFAULT_DOOR_HEIGHT: f64 = 2.1;
/// Default door sill height (flush with floor).
pub const DEFAULT_DOOR_SILL: f64 = 0.0;

impl Opening {
    /// Construct a window with default dimensions.
    pub fn window(handle: Handle, host_wall: Handle, distance_along_axis: f64) -> Self {
        Self {
            handle,
            host_wall,
            distance_along_axis,
            width: DEFAULT_WINDOW_WIDTH,
            height: DEFAULT_WINDOW_HEIGHT,
            sill_height: DEFAULT_WINDOW_SILL,
            kind: OpeningKind::Window,
        }
    }

    /// Construct a door with default dimensions.
    pub fn door(handle: Handle, host_wall: Handle, distance_along_axis: f64) -> Self {
        Self {
            handle,
            host_wall,
            distance_along_axis,
            width: DEFAULT_DOOR_WIDTH,
            height: DEFAULT_DOOR_HEIGHT,
            sill_height: DEFAULT_DOOR_SILL,
            kind: OpeningKind::Door,
        }
    }

    /// Axis-parameter interval `[start, end]` occupied by this opening.
    pub fn axis_span(&self) -> (f64, f64) {
        let half = self.width * 0.5;
        (self.distance_along_axis - half, self.distance_along_axis + half)
    }
}

/// Total polyline length as sum of straight segment lengths.
pub fn axis_length(axis: &[(f64, f64)]) -> f64 {
    let mut total = 0.0;
    for w in axis.windows(2) {
        let dx = w[1].0 - w[0].0;
        let dy = w[1].1 - w[0].1;
        total += (dx * dx + dy * dy).sqrt();
    }
    total
}

/// Point and unit tangent on the axis at distance `s` from the start.
///
/// `s` is clamped to `[0, axis_length]`. Returns `None` if the axis has fewer
/// than two distinct points.
pub fn point_and_tangent_at_distance(axis: &[(f64, f64)], s: f64) -> Option<((f64, f64), (f64, f64))> {
    if axis.len() < 2 {
        return None;
    }
    let total = axis_length(axis);
    if total < 1e-12 {
        return None;
    }
    let s = s.clamp(0.0, total);
    let mut walked = 0.0;
    for w in axis.windows(2) {
        let dx = w[1].0 - w[0].0;
        let dy = w[1].1 - w[0].1;
        let seg = (dx * dx + dy * dy).sqrt();
        if seg < 1e-12 {
            continue;
        }
        if walked + seg >= s - 1e-12 {
            let t = ((s - walked) / seg).clamp(0.0, 1.0);
            let px = w[0].0 + dx * t;
            let py = w[0].1 + dy * t;
            let tx = dx / seg;
            let ty = dy / seg;
            return Some(((px, py), (tx, ty)));
        }
        walked += seg;
    }
    // Fallback: end point, last non-zero tangent.
    let last = *axis.last().unwrap();
    for w in axis.windows(2).rev() {
        let dx = w[1].0 - w[0].0;
        let dy = w[1].1 - w[0].1;
        let seg = (dx * dx + dy * dy).sqrt();
        if seg >= 1e-12 {
            return Some((last, (dx / seg, dy / seg)));
        }
    }
    Some((last, (1.0, 0.0)))
}

/// Distance along the axis from the start to the closest point on the axis
/// polyline to `point` (straight-segment projection).
pub fn distance_along_axis_from_point(axis: &[(f64, f64)], point: (f64, f64)) -> Option<f64> {
    if axis.len() < 2 {
        return None;
    }
    let (px, py) = point;
    let mut best_dist_sq = f64::INFINITY;
    let mut best_s = 0.0;
    let mut walked = 0.0;
    for w in axis.windows(2) {
        let (x0, y0) = w[0];
        let (x1, y1) = w[1];
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len_sq = dx * dx + dy * dy;
        let (s_local, cx, cy) = if len_sq < 1e-24 {
            (0.0, x0, y0)
        } else {
            let t = ((px - x0) * dx + (py - y0) * dy) / len_sq;
            let t = t.clamp(0.0, 1.0);
            (t * len_sq.sqrt(), x0 + dx * t, y0 + dy * t)
        };
        let ddx = px - cx;
        let ddy = py - cy;
        let d2 = ddx * ddx + ddy * ddy;
        if d2 < best_dist_sq {
            best_dist_sq = d2;
            best_s = walked + s_local;
        }
        walked += len_sq.sqrt();
    }
    Some(best_s)
}

/// 2D opening footprint rectangle in world XY.
///
/// Centered at `opening.distance_along_axis` along `axis`, spanning
/// `opening.width` along the local axis tangent and `total_thickness`
/// perpendicular to it (full wall thickness). Vertices are ordered CCW and
/// the first point is **not** repeated.
///
/// Returns `None` for degenerate axis / non-positive width or thickness.
pub fn opening_footprint_2d(
    axis: &[(f64, f64)],
    total_thickness: f64,
    opening: &Opening,
) -> Option<Vec<(f64, f64)>> {
    if opening.width <= 1e-12 || total_thickness <= 1e-12 {
        return None;
    }
    let ((cx, cy), (tx, ty)) =
        point_and_tangent_at_distance(axis, opening.distance_along_axis)?;
    // Left-hand unit normal (same convention as geometry::normal).
    let nx = -ty;
    let ny = tx;
    let half_w = opening.width * 0.5;
    let half_t = total_thickness * 0.5;
    // Four corners: center ± (half_w * tangent) ± (half_t * normal).
    let along = |s: f64, n: f64| (cx + tx * s + nx * n, cy + ty * s + ny * n);
    Some(vec![
        along(-half_w, -half_t),
        along(half_w, -half_t),
        along(half_w, half_t),
        along(-half_w, half_t),
    ])
}

/// Minimum meaningful axis-parameter span (in drawing units). Free spans (or
/// gaps between merged opening intervals) narrower than this are treated as
/// zero-width: real drawings work in metres/millimetres, so a "gap" of a
/// fraction of a micrometre is floating-point noise from chained
/// distance-along-axis sums, not a deliberate sliver pier between two
/// openings. Without this, near-coincident opening edges (two openings
/// placed edge-to-edge, or the same opening re-evaluated after a tiny axis
/// edit) could otherwise produce a degenerate near-zero-area contour piece
/// that survives the `area > 1e-12` filter downstream as a thin shard.
const MIN_AXIS_SPAN: f64 = 1e-6;

/// Merge overlapping (or near-touching, within [`MIN_AXIS_SPAN`]) `[lo, hi]`
/// intervals; input need not be sorted.
fn merge_intervals(mut intervals: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    if intervals.is_empty() {
        return intervals;
    }
    intervals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = Vec::with_capacity(intervals.len());
    let (mut lo, mut hi) = intervals[0];
    for &(a, b) in &intervals[1..] {
        if a <= hi + MIN_AXIS_SPAN {
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

/// Remaining axis spans after subtracting opening intervals from `[0, length]`.
///
/// Free spans (including the leading/trailing spans at the wall ends)
/// narrower than [`MIN_AXIS_SPAN`] are dropped rather than kept as
/// degenerate slivers — this keeps an opening placed almost flush with a
/// wall end (or two openings placed almost edge-to-edge) from producing a
/// near-zero-width extra piece alongside the real one.
fn remaining_axis_spans(length: f64, openings: &[Opening]) -> Vec<(f64, f64)> {
    if length <= 1e-12 {
        return Vec::new();
    }
    let mut intervals = Vec::new();
    for o in openings {
        if o.width <= 1e-12 {
            continue;
        }
        let (a, b) = o.axis_span();
        let lo = a.clamp(0.0, length);
        let hi = b.clamp(0.0, length);
        if hi - lo > 1e-12 {
            intervals.push((lo, hi));
        }
    }
    let blocked = merge_intervals(intervals);
    let mut free = Vec::new();
    let mut cursor = 0.0;
    for (lo, hi) in blocked {
        if lo - cursor > MIN_AXIS_SPAN {
            free.push((cursor, lo));
        }
        cursor = cursor.max(hi);
    }
    if length - cursor > MIN_AXIS_SPAN {
        free.push((cursor, length));
    }
    free
}

/// Sample the axis polyline restricted to parameter range `[s0, s1]`.
///
/// Returns at least two points (the endpoints), plus every original vertex
/// strictly inside the range so multi-segment walls keep their corners.
pub fn sub_axis(axis: &[(f64, f64)], s0: f64, s1: f64) -> Vec<(f64, f64)> {
    if axis.len() < 2 || s1 - s0 <= 1e-12 {
        return Vec::new();
    }
    let mut pts = Vec::new();
    if let Some((p, _)) = point_and_tangent_at_distance(axis, s0) {
        pts.push(p);
    } else {
        return Vec::new();
    }
    let mut walked = 0.0;
    for w in axis.windows(2) {
        let dx = w[1].0 - w[0].0;
        let dy = w[1].1 - w[0].1;
        let seg = (dx * dx + dy * dy).sqrt();
        let next = walked + seg;
        // Include the segment end vertex when it lies strictly inside (s0, s1).
        if next > s0 + 1e-12 && next < s1 - 1e-12 {
            let p = w[1];
            if pts
                .last()
                .map(|&(x, y)| (x - p.0).abs() > 1e-12 || (y - p.1).abs() > 1e-12)
                .unwrap_or(true)
            {
                pts.push(p);
            }
        }
        walked = next;
    }
    if let Some((p, _)) = point_and_tangent_at_distance(axis, s1) {
        if pts
            .last()
            .map(|&(x, y)| (x - p.0).abs() > 1e-12 || (y - p.1).abs() > 1e-12)
            .unwrap_or(true)
        {
            pts.push(p);
        }
    }
    if pts.len() < 2 {
        Vec::new()
    } else {
        pts
    }
}

/// Subtract opening footprints from a wall-band polygon by splitting the band
/// along the axis parameter around each opening span.
///
/// Implementation strategy (targeted, not a general boolean):
/// 1. Collect opening intervals on the axis and merge overlaps.
/// 2. For each remaining free span, rebuild a band polygon via
///    [`outer_contour`] on the sub-axis (same thickness / center justification).
///
/// This yields clean rectangular end-caps at opening edges for the common
/// straight single-layer wall case and stays well-defined near wall ends
/// (a free span that is too short simply produces no piece).
///
/// `polygon` is accepted for API completeness / future general-boolean work;
/// the current path rebuilds pieces from `axis` + `total_thickness` so the
/// result matches the engine's contour conventions bit-for-bit with a wall
/// whose axis was the free span. Layer footprints should call
/// [`subtract_openings_from_band_with_builder`] with their own builder instead.
///
/// Returns an empty vec if the openings consume the entire wall.
pub fn subtract_openings_from_band(
    _polygon: &[(f64, f64)],
    axis: &[(f64, f64)],
    total_thickness: f64,
    openings: &[Opening],
) -> Vec<Vec<(f64, f64)>> {
    if openings.is_empty() {
        if _polygon.len() >= 3 {
            return vec![_polygon.to_vec()];
        }
        return Vec::new();
    }
    let length = axis_length(axis);
    let spans = remaining_axis_spans(length, openings);
    let mut pieces = Vec::new();
    for (s0, s1) in spans {
        let sub = sub_axis(axis, s0, s1);
        if sub.len() < 2 {
            continue;
        }
        let piece = outer_contour(&sub, total_thickness, 0.0);
        if piece.len() >= 3 && area(&piece) > 1e-12 {
            pieces.push(piece);
        }
    }
    pieces
}

/// Like [`subtract_openings_from_band`], but each free sub-axis is turned into
/// a polygon by `build_piece(sub_axis) -> polygon` so layer footprints (which
/// are not simple outer bands of `total_thickness`) can reuse the same split.
pub fn subtract_openings_from_band_with_builder<F>(
    axis: &[(f64, f64)],
    openings: &[Opening],
    mut build_piece: F,
) -> Vec<Vec<(f64, f64)>>
where
    F: FnMut(&[(f64, f64)]) -> Vec<(f64, f64)>,
{
    if openings.is_empty() {
        let p = build_piece(axis);
        return if p.len() >= 3 { vec![p] } else { Vec::new() };
    }
    let length = axis_length(axis);
    let spans = remaining_axis_spans(length, openings);
    let mut pieces = Vec::new();
    for (s0, s1) in spans {
        let sub = sub_axis(axis, s0, s1);
        if sub.len() < 2 {
            continue;
        }
        let piece = build_piece(&sub);
        if piece.len() >= 3 && area(&piece) > 1e-12 {
            pieces.push(piece);
        }
    }
    pieces
}

/// Single-opening convenience wrapper around [`subtract_openings_from_band`].
pub fn subtract_rect_opening_from_band(
    polygon: &[(f64, f64)],
    axis: &[(f64, f64)],
    total_thickness: f64,
    opening: &Opening,
) -> Vec<Vec<(f64, f64)>> {
    subtract_openings_from_band(polygon, axis, total_thickness, std::slice::from_ref(opening))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::contour::outer_contour;
    use crate::modules::aec::engine::geometry::area;

    fn dummy_opening(dist: f64, width: f64) -> Opening {
        Opening {
            handle: Handle::new(1),
            host_wall: Handle::new(2),
            distance_along_axis: dist,
            width,
            height: 1.2,
            sill_height: 0.9,
            kind: OpeningKind::Window,
        }
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn opening_footprint_centered_on_straight_axis() {
        let axis = vec![(0.0, 0.0), (10.0, 0.0)];
        let o = dummy_opening(5.0, 1.2);
        let fp = opening_footprint_2d(&axis, 0.2, &o).expect("footprint");
        assert_eq!(fp.len(), 4);
        // x spans [4.4, 5.6], y spans [-0.1, 0.1]
        let xs: Vec<f64> = fp.iter().map(|p| p.0).collect();
        let ys: Vec<f64> = fp.iter().map(|p| p.1).collect();
        assert!(approx(*xs.iter().min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap(), 4.4));
        assert!(approx(*xs.iter().max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap(), 5.6));
        assert!(approx(*ys.iter().min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap(), -0.1));
        assert!(approx(*ys.iter().max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap(), 0.1));
        // Area = width * thickness
        assert!(approx(area(&fp), 1.2 * 0.2));
    }

    #[test]
    fn opening_cuts_straight_single_layer_outer_contour() {
        let axis = vec![(0.0, 0.0), (10.0, 0.0)];
        let thickness = 0.2;
        let outer = outer_contour(&axis, thickness, 0.0);
        let uncut_area = area(&outer);
        assert!(approx(uncut_area, 10.0 * 0.2));

        let o = dummy_opening(5.0, 1.2);
        let pieces = subtract_rect_opening_from_band(&outer, &axis, thickness, &o);
        assert_eq!(pieces.len(), 2, "full-thickness opening splits band into 2 pieces");

        let cut_area: f64 = pieces.iter().map(|p| area(p)).sum();
        let expected = uncut_area - 1.2 * thickness;
        assert!(
            (cut_area - expected).abs() < 1e-9,
            "cut area {cut_area} vs expected {expected}"
        );

        // Left piece ends at x=4.4, right starts at x=5.6
        let left = &pieces[0];
        let right = &pieces[1];
        let left_max_x = left
            .iter()
            .map(|p| p.0)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();
        let right_min_x = right
            .iter()
            .map(|p| p.0)
            .min_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();
        assert!(approx(left_max_x, 4.4));
        assert!(approx(right_min_x, 5.6));
    }

    #[test]
    fn opening_near_wall_end_produces_valid_geometry() {
        let axis = vec![(0.0, 0.0), (10.0, 0.0)];
        let thickness = 0.2;
        let outer = outer_contour(&axis, thickness, 0.0);
        // Opening near start: center at 0.7, width 1.0 → span [0.2, 1.2]
        let o = dummy_opening(0.7, 1.0);
        let pieces = subtract_rect_opening_from_band(&outer, &axis, thickness, &o);
        assert!(
            !pieces.is_empty(),
            "near-end opening must leave at least the far piece"
        );
        for p in &pieces {
            assert!(p.len() >= 3, "no degenerate polygons");
            assert!(area(p) > 1e-9, "no zero-area pieces");
            // Basic self-intersection smoke: shoelace area equals bbox-ish bound
            let xs: Vec<f64> = p.iter().map(|q| q.0).collect();
            let ys: Vec<f64> = p.iter().map(|q| q.1).collect();
            let min_x = xs.iter().cloned().fold(f64::INFINITY, f64::min);
            let max_x = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let min_y = ys.iter().cloned().fold(f64::INFINITY, f64::min);
            let max_y = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            assert!(area(p) <= (max_x - min_x) * (max_y - min_y) + 1e-9);
        }
        let cut_area: f64 = pieces.iter().map(|p| area(p)).sum();
        // Consumed span length = 1.0 (fully inside [0,10])
        assert!((cut_area - (10.0 - 1.0) * thickness).abs() < 1e-9);
    }

    #[test]
    fn distance_along_axis_invariant_when_extending_far_end() {
        // Opening position is an absolute offset from the wall *start*.
        // Extending the far end does not move earlier vertices, so the
        // opening's world position (and distance_along_axis) stay fixed.
        let axis_before = vec![(0.0, 0.0), (10.0, 0.0)];
        let o = dummy_opening(5.0, 1.2);
        let fp_before = opening_footprint_2d(&axis_before, 0.2, &o).unwrap();
        let center_before = (
            fp_before.iter().map(|p| p.0).sum::<f64>() / 4.0,
            fp_before.iter().map(|p| p.1).sum::<f64>() / 4.0,
        );

        let axis_after = vec![(0.0, 0.0), (15.0, 0.0)]; // extend far end
        let fp_after = opening_footprint_2d(&axis_after, 0.2, &o).unwrap();
        let center_after = (
            fp_after.iter().map(|p| p.0).sum::<f64>() / 4.0,
            fp_after.iter().map(|p| p.1).sum::<f64>() / 4.0,
        );
        assert!(approx(center_before.0, center_after.0));
        assert!(approx(center_before.1, center_after.1));
        assert!(approx(o.distance_along_axis, 5.0));

        // Join that only appends geometry past the opening likewise preserves
        // the absolute start-offset (no special opening rewrite needed).
        let axis_joined = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 5.0)];
        let fp_joined = opening_footprint_2d(&axis_joined, 0.2, &o).unwrap();
        let center_joined = (
            fp_joined.iter().map(|p| p.0).sum::<f64>() / 4.0,
            fp_joined.iter().map(|p| p.1).sum::<f64>() / 4.0,
        );
        assert!(approx(center_joined.0, 5.0));
        assert!(approx(center_joined.1, 0.0));
    }

    #[test]
    fn project_point_onto_axis_midpoint() {
        let axis = vec![(0.0, 0.0), (10.0, 0.0)];
        let d = distance_along_axis_from_point(&axis, (5.0, 0.3)).unwrap();
        assert!(approx(d, 5.0));
    }

    #[test]
    fn opening_wider_than_wall_consumes_entire_band() {
        let axis = vec![(0.0, 0.0), (2.0, 0.0)];
        let outer = outer_contour(&axis, 0.2, 0.0);
        let o = dummy_opening(1.0, 5.0); // wider than wall
        let pieces = subtract_rect_opening_from_band(&outer, &axis, 0.2, &o);
        assert!(pieces.is_empty());
    }

    #[test]
    fn multiple_openings_on_one_segment_produce_three_valid_pieces() {
        let axis = vec![(0.0, 0.0), (20.0, 0.0)];
        let thickness = 0.2;
        let outer = outer_contour(&axis, thickness, 0.0);
        let uncut_area = area(&outer);
        // Three windows spread along the wall, clearly separated.
        let openings = vec![
            dummy_opening(4.0, 1.0),
            dummy_opening(10.0, 1.2),
            dummy_opening(16.0, 0.9),
        ];
        let pieces = subtract_openings_from_band(&outer, &axis, thickness, &openings);
        assert_eq!(pieces.len(), 4, "three separated openings split the band into 4 pieces");
        for p in &pieces {
            assert!(p.len() >= 3);
            assert!(area(p) > 1e-9);
        }
        let cut_area: f64 = pieces.iter().map(|p| area(p)).sum();
        let consumed = (1.0 + 1.2 + 0.9) * thickness;
        assert!((cut_area - (uncut_area - consumed)).abs() < 1e-9);
    }

    #[test]
    fn two_openings_placed_edge_to_edge_do_not_produce_a_degenerate_sliver() {
        let axis = vec![(0.0, 0.0), (10.0, 0.0)];
        let thickness = 0.2;
        let outer = outer_contour(&axis, thickness, 0.0);
        // First opening spans [4.0, 5.0]; second starts exactly where the
        // first ends (touching, no gap) — no legitimate pier between them.
        let a = dummy_opening(4.5, 1.0); // [4.0, 5.0]
        let b = dummy_opening(5.5, 1.0); // [5.0, 6.0]
        let pieces = subtract_openings_from_band(&outer, &axis, thickness, &[a, b]);
        assert_eq!(
            pieces.len(),
            2,
            "touching openings must merge into one gap, leaving only the two outer pieces"
        );
        for p in &pieces {
            assert!(area(p) > 1e-6, "no degenerate sliver piece between touching openings");
        }
    }

    #[test]
    fn two_openings_extremely_close_merge_instead_of_leaving_a_micro_sliver() {
        let axis = vec![(0.0, 0.0), (10.0, 0.0)];
        let thickness = 0.2;
        let outer = outer_contour(&axis, thickness, 0.0);
        // Gap between the two openings is 1e-9 — floating-point noise, not a
        // deliberate pier.
        let a = dummy_opening(4.5, 1.0); // [4.0, 5.0]
        let b = dummy_opening(5.0 + 1e-9 + 0.5, 1.0); // starts at ~5.0 + 1e-9
        let pieces = subtract_openings_from_band(&outer, &axis, thickness, &[a, b]);
        assert_eq!(pieces.len(), 2, "near-coincident opening edges must merge, not leave a sliver");
        for p in &pieces {
            assert!(area(p) > 1e-6);
        }
    }

    #[test]
    fn opening_flush_with_wall_start_leaves_no_degenerate_leading_piece() {
        let axis = vec![(0.0, 0.0), (10.0, 0.0)];
        let thickness = 0.2;
        let outer = outer_contour(&axis, thickness, 0.0);
        // Opening starts essentially at the wall's very start (span ~[0, 1.0]).
        let o = dummy_opening(0.5 + 1e-9, 1.0);
        let pieces = subtract_openings_from_band(&outer, &axis, thickness, &[o]);
        assert_eq!(
            pieces.len(),
            1,
            "an opening flush with the wall start should leave only the trailing piece"
        );
        assert!(area(&pieces[0]) > 1e-6);
    }

    #[test]
    fn opening_over_gap_layer_splits_every_layer_including_the_gap_neighbors() {
        use crate::modules::aec::engine::contour::{closed_layer_footprint, layer_contours_with_bulges};

        let axis = vec![(0.0, 0.0), (10.0, 0.0)];
        // Three layers with explicit axis offsets (including a physical gap
        // between layers 0 and 1). The opening spans full thickness so every
        // layer must split.
        let layers = vec![(0.2, -0.19), (0.05, -0.06), (0.1, -0.01)];
        let total_thickness = {
            let min_s = layers.iter().map(|(_, o)| *o).fold(f64::INFINITY, f64::min);
            let max_e = layers.iter().map(|(th, o)| th + o).fold(f64::NEG_INFINITY, f64::max);
            max_e - min_s
        };
        let opening = dummy_opening(5.0, 1.2);

        for (li, _layer) in layers.iter().enumerate() {
            let pieces = subtract_openings_from_band_with_builder(&axis, &[opening.clone()], |sub| {
                let pairs = layer_contours_with_bulges(sub, &[], &layers);
                pairs
                    .get(li)
                    .map(|(b1, b2)| closed_layer_footprint(b1, b2).points)
                    .unwrap_or_default()
            });
            assert_eq!(
                pieces.len(),
                2,
                "layer {li} (total thickness stack {total_thickness}) must split into 2 pieces around the opening"
            );
            for p in &pieces {
                assert!(area(p) > 1e-9, "layer {li} produced a degenerate piece");
            }
        }
    }
}
