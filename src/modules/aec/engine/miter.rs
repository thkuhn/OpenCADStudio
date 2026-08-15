//! Per-layer miter geometry for L/T wall joins.
//!
//! Given two joined wall axes and their material-layer stacks, this module
//! computes a diagonal miter line at the shared corner and clips each wall's
//! matching layer footprint against it. Layers are matched by index from the
//! reference axis outward; unmatched layers (different counts) return `None`
//! so the caller can fall back to the single-vertex `corner_override` path.

use super::contour::layer_contours;
use super::join::JoinKind;

/// Context describing the *other* wall at a join, used when regenerating one
/// wall's visible representation with per-layer miters.
#[derive(Debug, Clone)]
pub struct JoinMiterContext {
    /// Vertex index on *this* wall's axis that sits at the join (0 or last).
    pub self_end: usize,
    /// Other wall's axis polyline as 2D points (already trimmed/extended by
    /// [`super::join::join_wall_axes`]).
    pub other_axis: Vec<(f64, f64)>,
    /// Other wall's layers as `(thickness, gap_before)` pairs, same order as
    /// the wall's material stack (index 0 = first from the reference side).
    pub other_layers: Vec<(f64, f64)>,
    /// Joined end on the other wall (`Some(0|last)` for L / T-stem). `None`
    /// when the other wall is the through-wall of a T-junction (no endpoint
    /// moved); the helper then locates the segment under the join point.
    pub other_end: Option<usize>,
    pub kind: JoinKind,
}

/// For each layer of wall A, return either a miter-clipped closed footprint
/// polygon, or `None` when that layer index can't be matched against wall B
/// (caller should fall back to `corner_override` for that layer).
///
/// `end_a` is the joined vertex index on A. `end_b` is `Some` for an L-join
/// or the stem of a T-join; `None` means B is the through-wall of a T.
pub fn mitered_layer_footprints(
    axis_a: &[(f64, f64)],
    layers_a: &[(f64, f64)],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[(f64, f64)],
    end_b: Option<usize>,
    _kind: JoinKind,
) -> Vec<Option<Vec<(f64, f64)>>> {
    if axis_a.len() < 2 || axis_b.len() < 2 || layers_a.is_empty() {
        return vec![None; layers_a.len()];
    }
    if end_a >= axis_a.len() {
        return vec![None; layers_a.len()];
    }

    let contours_a = layer_contours(axis_a, layers_a);
    let contours_b = layer_contours(axis_b, layers_b);
    if contours_a.is_empty() || contours_b.is_empty() {
        return vec![None; layers_a.len()];
    }

    let matched = layers_a.len().min(layers_b.len());
    let mut out = Vec::with_capacity(layers_a.len());

    for i in 0..layers_a.len() {
        if i >= matched {
            out.push(None);
            continue;
        }
        let (ref a_b1, ref a_b2) = contours_a[i];
        let (ref b_b1, ref b_b2) = contours_b[i];
        match miter_one_layer(a_b1, a_b2, end_a, b_b1, b_b2, end_b, axis_a, axis_b) {
            Some(fp) if fp.len() >= 3 => out.push(Some(fp)),
            _ => out.push(None),
        }
    }
    out
}

/// Build a closed footprint for one layer of wall A, with the joined end
/// replaced by the diagonal miter against wall B's matching layer.
fn miter_one_layer(
    a_b1: &[(f64, f64)],
    a_b2: &[(f64, f64)],
    end_a: usize,
    b_b1: &[(f64, f64)],
    b_b2: &[(f64, f64)],
    end_b: Option<usize>,
    axis_a: &[(f64, f64)],
    axis_b: &[(f64, f64)],
) -> Option<Vec<(f64, f64)>> {
    if a_b1.len() < 2 || a_b2.len() < 2 || a_b1.len() != a_b2.len() {
        return None;
    }
    if end_a >= a_b1.len() {
        return None;
    }
    if b_b1.len() < 2 || b_b2.len() < 2 || b_b1.len() != b_b2.len() {
        return None;
    }

    // Neighbour index on A: one step toward the wall interior from the join.
    let prev_a = if end_a == 0 { 1 } else { end_a - 1 };

    // Boundary lines on A at the joined end (interior point → end point,
    // extended past the end so intersections beyond the corner are found).
    let a1_line = extended_line(a_b1[prev_a], a_b1[end_a]);
    let a2_line = extended_line(a_b2[prev_a], a_b2[end_a]);

    // Boundary lines on B at the join.
    let (b1_line, b2_line) = other_boundary_lines(b_b1, b_b2, end_b, axis_a, axis_b, end_a)?;

    // Four candidate intersections of A's long edges with B's long edges.
    let i_a1_b1 = intersect_lines_2d(a1_line.0, a1_line.1, b1_line.0, b1_line.1)?;
    let i_a1_b2 = intersect_lines_2d(a1_line.0, a1_line.1, b2_line.0, b2_line.1)?;
    let i_a2_b1 = intersect_lines_2d(a2_line.0, a2_line.1, b1_line.0, b1_line.1)?;
    let i_a2_b2 = intersect_lines_2d(a2_line.0, a2_line.1, b2_line.0, b2_line.1)?;

    // Choose the pairing (a1↔b1 & a2↔b2) vs (a1↔b2 & a2↔b1) whose miter
    // segment is shorter — that is the diagonal that actually closes the
    // corner rather than the long exterior-to-exterior chord.
    let pair_direct = (i_a1_b1, i_a2_b2);
    let pair_cross = (i_a1_b2, i_a2_b1);
    let len_direct = dist(pair_direct.0, pair_direct.1);
    let len_cross = dist(pair_cross.0, pair_cross.1);
    let (new_a1, new_a2) = if len_direct <= len_cross {
        pair_direct
    } else {
        pair_cross
    };

    // Rebuild the closed footprint: forward along b1 with the joined end
    // replaced by new_a1, then back along b2 with the joined end replaced by
    // new_a2. For a 2-vertex wall this is a quad with a diagonal end-cap.
    let n = a_b1.len();
    let mut footprint = Vec::with_capacity(n * 2);
    for i in 0..n {
        if i == end_a {
            footprint.push(new_a1);
        } else {
            footprint.push(a_b1[i]);
        }
    }
    for i in (0..n).rev() {
        if i == end_a {
            footprint.push(new_a2);
        } else {
            footprint.push(a_b2[i]);
        }
    }
    Some(footprint)
}

/// Boundary line pair on the other wall at the join.
///
/// For an L / T-stem (`end_b = Some(idx)`) this is the end segment of each
/// boundary. For a T through-wall (`end_b = None`) it is the boundary segment
/// whose parameter range contains the join point projected from A's end.
fn other_boundary_lines(
    b_b1: &[(f64, f64)],
    b_b2: &[(f64, f64)],
    end_b: Option<usize>,
    axis_a: &[(f64, f64)],
    axis_b: &[(f64, f64)],
    end_a: usize,
) -> Option<(((f64, f64), (f64, f64)), ((f64, f64), (f64, f64)))> {
    let n = b_b1.len();
    if n < 2 {
        return None;
    }

    if let Some(eb) = end_b {
        if eb >= n {
            return None;
        }
        let prev_b = if eb == 0 { 1 } else { eb - 1 };
        return Some((
            extended_line(b_b1[prev_b], b_b1[eb]),
            extended_line(b_b2[prev_b], b_b2[eb]),
        ));
    }

    // T through-wall: find the segment of axis_b closest to A's join point.
    let join_pt = axis_a.get(end_a).copied()?;
    let mut best_seg = 0usize;
    let mut best_dist = f64::INFINITY;
    for s in 0..axis_b.len().saturating_sub(1) {
        let d = point_seg_dist(join_pt, axis_b[s], axis_b[s + 1]);
        if d < best_dist {
            best_dist = d;
            best_seg = s;
        }
    }
    let i0 = best_seg;
    let i1 = best_seg + 1;
    if i1 >= n {
        return None;
    }
    Some((
        extended_line(b_b1[i0], b_b1[i1]),
        extended_line(b_b2[i0], b_b2[i1]),
    ))
}

/// Extend the segment `from → to` well past `to` so line/line intersection
/// still finds corners that lie outside the original segment.
fn extended_line(from: (f64, f64), to: (f64, f64)) -> ((f64, f64), (f64, f64)) {
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-12 {
        return (from, to);
    }
    // Extend by a large multiple of the segment length past `to`.
    let scale = 100.0;
    let ext = (to.0 + dx / len * len * scale, to.1 + dy / len * len * scale);
    // Also extend slightly backward past `from` so near-end intersections work.
    let back = (
        from.0 - dx / len * len * scale,
        from.1 - dy / len * len * scale,
    );
    (back, ext)
}

fn intersect_lines_2d(
    p1: (f64, f64),
    p2: (f64, f64),
    p3: (f64, f64),
    p4: (f64, f64),
) -> Option<(f64, f64)> {
    let (x1, y1) = p1;
    let (x2, y2) = p2;
    let (x3, y3) = p3;
    let (x4, y4) = p4;
    let denom = (y4 - y3) * (x2 - x1) - (x4 - x3) * (y2 - y1);
    if denom.abs() < 1e-12 {
        return None;
    }
    let ua = ((x4 - x3) * (y1 - y3) - (y4 - y3) * (x1 - x3)) / denom;
    Some((x1 + ua * (x2 - x1), y1 + ua * (y2 - y1)))
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    (dx * dx + dy * dy).sqrt()
}

fn point_seg_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (px, py) = p;
    let (x1, y1) = a;
    let (x2, y2) = b;
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-24 {
        return dist(p, a);
    }
    let t = ((px - x1) * dx + (py - y1) * dy) / len2;
    let t = t.clamp(0.0, 1.0);
    dist(p, (x1 + t * dx, y1 + t * dy))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::join::JoinKind;

    fn close(a: (f64, f64), b: (f64, f64), tol: f64) -> bool {
        dist(a, b) < tol
    }

    #[test]
    fn l_corner_miter_shares_diagonal_endpoints() {
        // Wall A along +X ending at (10,0); wall B along +Y starting at (10,0).
        // Single layer, thickness 0.2 each → half = 0.1.
        let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
        let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
        let layers = vec![(0.2, 0.0)];

        let fps_a = mitered_layer_footprints(
            &axis_a,
            &layers,
            1, // end at (10,0)
            &axis_b,
            &layers,
            Some(0),
            JoinKind::L,
        );
        let fps_b = mitered_layer_footprints(
            &axis_b,
            &layers,
            0,
            &axis_a,
            &layers,
            Some(1),
            JoinKind::L,
        );

        let fp_a = fps_a[0].as_ref().expect("A layer 0 should miter");
        let fp_b = fps_b[0].as_ref().expect("B layer 0 should miter");

        // Expected outer/inner miter corners for equal 0.2 walls at 90°:
        // A boundaries y=±0.1, B boundaries x=10±0.1.
        // Pairing the short diagonal: (10.1, -0.1) and (9.9, 0.1).
        let c1 = (10.1, -0.1);
        let c2 = (9.9, 0.1);
        assert!(
            fp_a.iter().any(|p| close(*p, c1, 1e-6)) && fp_a.iter().any(|p| close(*p, c2, 1e-6)),
            "A footprint should contain both miter corners, got {fp_a:?}"
        );
        assert!(
            fp_b.iter().any(|p| close(*p, c1, 1e-6)) && fp_b.iter().any(|p| close(*p, c2, 1e-6)),
            "B footprint should share the same miter corners, got {fp_b:?}"
        );
    }

    #[test]
    fn t_corner_stem_miters_against_through_wall() {
        // Stem A: (5,1)->(5,10) will join onto through wall B at (5,0).
        // After join the stem end is at (5,0); through wall unchanged.
        let axis_a = vec![(5.0, 0.0), (5.0, 10.0)];
        let axis_b = vec![(0.0, 0.0), (10.0, 0.0)];
        let layers = vec![(0.2, 0.0)];

        let fps_a = mitered_layer_footprints(
            &axis_a,
            &layers,
            0, // stem end at (5,0)
            &axis_b,
            &layers,
            None, // B is through-wall
            JoinKind::T,
        );
        let fp_a = fps_a[0].as_ref().expect("stem layer should miter");

        // Through wall boundaries at y=±0.1; stem boundaries at x=5±0.1.
        // Short miter diagonal: (5.1, -0.1) and (4.9, 0.1) — or the
        // through-wall face cut at y=±0.1 depending on pairing. Either way
        // the stem's end vertices must lie on the through wall's layer band
        // (|y| ≈ 0.1) rather than stopping at the axis (y=0).
        let max_abs_y_at_end = fp_a
            .iter()
            .filter(|(x, _)| (*x - 5.0).abs() < 0.15)
            .map(|(_, y)| y.abs())
            .fold(0.0_f64, f64::max);
        assert!(
            max_abs_y_at_end > 0.05,
            "stem end should reach the through wall's layer face, got max |y|={max_abs_y_at_end}, fp={fp_a:?}"
        );
    }

    #[test]
    fn mismatched_layer_count_returns_none_for_unmatched() {
        let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
        let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
        // A has two layers, B has one — only index 0 can match.
        let layers_a = vec![(0.2, 0.0), (0.05, 0.0)];
        let layers_b = vec![(0.3, 0.0)];

        let fps = mitered_layer_footprints(
            &axis_a,
            &layers_a,
            1,
            &axis_b,
            &layers_b,
            Some(0),
            JoinKind::L,
        );
        assert_eq!(fps.len(), 2);
        assert!(fps[0].is_some(), "matched layer 0 should produce a miter");
        assert!(
            fps[1].is_none(),
            "unmatched layer 1 must fall back (None) to corner_override"
        );
    }
}
