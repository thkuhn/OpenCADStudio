use glam::DVec3;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinKind {
    L,
    T,
}

#[derive(Debug, PartialEq)]
pub enum JoinError {
    Parallel,
    NoIntersection,
    Degenerate,
}

impl fmt::Display for JoinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JoinError::Parallel => write!(f, "Parallel axes"),
            JoinError::NoIntersection => write!(f, "No intersection found"),
            JoinError::Degenerate => write!(f, "Degenerate axis"),
        }
    }
}

/// Joins two wall axes in an L- or T-configuration.
///
/// Returns `(new_a, new_b, kind, end_a, end_b)` where `end_a` / `end_b` are the
/// vertex indices on each axis that sit at the join (`Some(0)` or
/// `Some(last)`). For a T-junction the through-wall has `None` (no endpoint
/// participates). These indices are authoritative even when the axes already
/// meet at the intersection (so endpoints do not move) — callers that rebuild
/// mitered footprints must not rely on "which endpoint changed" alone.
pub fn join_wall_axes(
    axis_a: &[DVec3],
    axis_b: &[DVec3],
) -> Result<(Vec<DVec3>, Vec<DVec3>, JoinKind, Option<usize>, Option<usize>), JoinError> {
    if axis_a.len() < 2 || axis_b.len() < 2 {
        return Err(JoinError::Degenerate);
    }

    // We check intersections between the end segments of each wall and all segments of the other.
    // End segments of A: (axis_a[0], axis_a[1]) and (axis_a[n-2], axis_a[n-1])
    let ends_a = [
        (0, 1, axis_a[0], axis_a[1]),
        (axis_a.len() - 1, axis_a.len() - 2, axis_a[axis_a.len() - 1], axis_a[axis_a.len() - 2]),
    ];
    let ends_b = [
        (0, 1, axis_b[0], axis_b[1]),
        (axis_b.len() - 1, axis_b.len() - 2, axis_b[axis_b.len() - 1], axis_b[axis_b.len() - 2]),
    ];

    let mut best_l: Option<(DVec3, usize, usize)> = None;
    let mut best_l_dist = f64::INFINITY;
    let mut best_t: Option<(DVec3, usize, usize, bool)> = None; // (point, a_end_idx, b_seg_idx, a_is_stem)
    let mut best_t_dist = f64::INFINITY;

    let tol = 1e-6;

    // Check for L-junctions (both meeting at ends)
    for (idx_a, _, p1, p2) in &ends_a {
        for (idx_b, _, p3, p4) in &ends_b {
            if let Some(isect) = intersect_lines_2d(*p1, *p2, *p3, *p4) {
                let dist = p1.distance(isect) + p3.distance(isect);
                if dist < best_l_dist {
                    best_l_dist = dist;
                    best_l = Some((isect, *idx_a, *idx_b));
                }
            }
        }
    }

    // Check for T-junctions (A's end meeting B's *interior*, or vice-versa).
    // Intersection exactly at a through-wall endpoint is an L, not a T — so
    // require the hit to be strictly interior to the through segment. Without
    // that guard, already-joined L corners (both ends already at the isect)
    // get mis-classified as T and only one wall receives a miter rebuild.
    // Case 1: A is the "stem" (ends at B), B is the "through" wall
    for (idx_a, _, p1, p2) in &ends_a {
        for i in 0..axis_b.len() - 1 {
            let p3 = axis_b[i];
            let p4 = axis_b[i + 1];
            if let Some(isect) = intersect_lines_2d(*p1, *p2, p3, p4) {
                if is_on_segment_interior_2d(isect, p3, p4, tol) {
                    let dist = p1.distance(isect);
                    if dist < best_t_dist {
                        best_t_dist = dist;
                        best_t = Some((isect, *idx_a, i, true));
                    }
                }
            }
        }
    }
    // Case 2: B is the "stem", A is the "through" wall
    for (idx_b, _, p3, p4) in &ends_b {
        for i in 0..axis_a.len() - 1 {
            let p1 = axis_a[i];
            let p2 = axis_a[i + 1];
            if let Some(isect) = intersect_lines_2d(*p3, *p4, p1, p2) {
                if is_on_segment_interior_2d(isect, p1, p2, tol) {
                    let dist = p3.distance(isect);
                    if dist < best_t_dist {
                        best_t_dist = dist;
                        best_t = Some((isect, *idx_b, i, false));
                    }
                }
            }
        }
    }

    // Preference: T-junction if it's "very close" to one wall's end but definitely inside the other's segment.
    // L-junction if it's "close" to both ends.
    //
    // Actually, let's use a simple distance-based choice.
    // If we have an L-junction candidate and it's close to the actual endpoints, prefer it.

    if let Some((isect, idx_a, idx_b)) = best_l {
        // Only consider it an L-junction if the intersection is relatively close to the ends
        // compared to a T-junction.
        let mut use_l = true;
        if let Some((t_isect, _, _, _)) = best_t {
            if t_isect.distance(axis_a[idx_a]) + t_isect.distance(axis_b[idx_b])
                > isect.distance(axis_a[idx_a]) + isect.distance(axis_b[idx_b])
            {
                use_l = true;
            } else {
                use_l = false;
            }
        }

        if use_l {
            let mut new_a = axis_a.to_vec();
            new_a[idx_a] = isect;
            let mut new_b = axis_b.to_vec();
            new_b[idx_b] = isect;
            return Ok((new_a, new_b, JoinKind::L, Some(idx_a), Some(idx_b)));
        }
    }

    if let Some((isect, idx_stem, _idx_through_seg, a_is_stem)) = best_t {
        if a_is_stem {
            let mut new_a = axis_a.to_vec();
            new_a[idx_stem] = isect;
            return Ok((new_a, axis_b.to_vec(), JoinKind::T, Some(idx_stem), None));
        } else {
            let mut new_b = axis_b.to_vec();
            new_b[idx_stem] = isect;
            return Ok((axis_a.to_vec(), new_b, JoinKind::T, None, Some(idx_stem)));
        }
    }

    Err(JoinError::NoIntersection)
}

fn intersect_lines_2d(p1: DVec3, p2: DVec3, p3: DVec3, p4: DVec3) -> Option<DVec3> {
    let x1 = p1.x; let y1 = p1.y;
    let x2 = p2.x; let y2 = p2.y;
    let x3 = p3.x; let y3 = p3.y;
    let x4 = p4.x; let y4 = p4.y;

    let denom = (y4 - y3) * (x2 - x1) - (x4 - x3) * (y2 - y1);
    if denom.abs() < 1e-9 {
        return None;
    }

    let ua = ((x4 - x3) * (y1 - y3) - (y4 - y3) * (x1 - x3)) / denom;
    Some(DVec3::new(
        x1 + ua * (x2 - x1),
        y1 + ua * (y2 - y1),
        p1.z
    ))
}

fn is_on_segment_2d(p: DVec3, a: DVec3, b: DVec3, tol: f64) -> bool {
    let dist_ap = a.distance(p);
    let dist_pb = p.distance(b);
    let dist_ab = a.distance(b);
    (dist_ap + dist_pb - dist_ab).abs() < tol
}

/// Like [`is_on_segment_2d`], but rejects hits that land on either endpoint
/// (within `tol`). Used for T-junction detection so end-to-end meetings stay L.
fn is_on_segment_interior_2d(p: DVec3, a: DVec3, b: DVec3, tol: f64) -> bool {
    if !is_on_segment_2d(p, a, b, tol) {
        return false;
    }
    p.distance(a) > tol && p.distance(b) > tol
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_l_join() {
        let axis_a = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0)];
        let axis_b = vec![DVec3::new(11.0, 1.0, 0.0), DVec3::new(11.0, 10.0, 0.0)];

        let (new_a, new_b, kind, end_a, end_b) = join_wall_axes(&axis_a, &axis_b).unwrap();
        assert_eq!(kind, JoinKind::L);
        assert_eq!(new_a[1], DVec3::new(11.0, 0.0, 0.0));
        assert_eq!(new_b[0], DVec3::new(11.0, 0.0, 0.0));
        assert_eq!(end_a, Some(1));
        assert_eq!(end_b, Some(0));
    }

    #[test]
    fn test_t_join() {
        let axis_a = vec![DVec3::new(5.0, 1.0, 0.0), DVec3::new(5.0, 10.0, 0.0)];
        let axis_b = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0)];

        let (new_a, new_b, kind, end_a, end_b) = join_wall_axes(&axis_a, &axis_b).unwrap();
        assert_eq!(kind, JoinKind::T);
        assert_eq!(new_a[0], DVec3::new(5.0, 0.0, 0.0));
        assert_eq!(new_b, axis_b);
        assert_eq!(end_a, Some(0));
        assert_eq!(end_b, None);
    }

    #[test]
    fn test_l_join_already_coincident_still_reports_ends() {
        // Axes already meet at the L corner — endpoints do not move, but the
        // join still has well-defined end indices for miter rebuild.
        let axis_a = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(5.0, 0.0, 0.0)];
        let axis_b = vec![DVec3::new(5.0, 0.0, 0.0), DVec3::new(5.0, 5.0, 0.0)];

        let (new_a, new_b, kind, end_a, end_b) = join_wall_axes(&axis_a, &axis_b).unwrap();
        assert_eq!(kind, JoinKind::L);
        assert_eq!(new_a, axis_a);
        assert_eq!(new_b, axis_b);
        assert_eq!(end_a, Some(1));
        assert_eq!(end_b, Some(0));
    }

    #[test]
    fn test_parallel_error() {
        let axis_a = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0)];
        let axis_b = vec![DVec3::new(0.0, 1.0, 0.0), DVec3::new(10.0, 1.0, 0.0)];

        let res = join_wall_axes(&axis_a, &axis_b);
        assert!(res.is_err());
    }
}
