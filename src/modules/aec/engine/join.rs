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

/// Default clustering tolerance for multi-wall junction detection.
pub const JUNCTION_TOLERANCE: f64 = 1e-6;

/// How a wall participates in a multi-wall junction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JunctionRole {
    /// Wall ends at the junction; `0` or last vertex index.
    Endpoint(usize),
    /// Junction lies on the interior of segment `seg` → `seg + 1`.
    Through(usize),
}

/// One wall's participation in a [`Junction`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JunctionParticipant {
    /// Index into the `walls` slice passed to [`detect_junctions`].
    pub wall_index: usize,
    pub role: JunctionRole,
}

/// A shared meeting point of 2+ wall ends / through-hits.
#[derive(Debug, Clone, PartialEq)]
pub struct Junction {
    pub point: DVec3,
    pub participants: Vec<JunctionParticipant>,
}

impl Junction {
    /// Number of walls that end at this junction (excludes through-walls).
    pub fn endpoint_count(&self) -> usize {
        self.participants
            .iter()
            .filter(|p| matches!(p.role, JunctionRole::Endpoint(_)))
            .count()
    }

    /// True when 3 or more walls meet (N-way), counting through-walls.
    pub fn is_multi_wall(&self) -> bool {
        self.participants.len() >= 3
    }
}

/// Group wall-axis endpoints (and through-segment hits) into junctions.
///
/// 1. Cluster endpoints whose XY distance is ≤ `tol`.
/// 2. For each cluster, take the mean position as the junction point.
/// 3. Any wall not already present as an endpoint, but whose *interior*
///    segment contains the junction point, is added as [`JunctionRole::Through`].
///
/// Only clusters with at least two participants (endpoint or through) are
/// returned. Degenerate axes (`len < 2`) are skipped.
pub fn detect_junctions(walls: &[&[DVec3]], tol: f64) -> Vec<Junction> {
    // Collect (wall_index, end_vertex_index, point).
    let mut endpoints: Vec<(usize, usize, DVec3)> = Vec::new();
    for (wi, axis) in walls.iter().enumerate() {
        if axis.len() < 2 {
            continue;
        }
        endpoints.push((wi, 0, axis[0]));
        let last = axis.len() - 1;
        endpoints.push((wi, last, axis[last]));
    }
    if endpoints.is_empty() {
        return Vec::new();
    }

    // Union-find over endpoints.
    let n = endpoints.len();
    let mut parent: Vec<usize> = (0..n).collect();
    let find = |parent: &mut [usize], mut i: usize| -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    };
    for i in 0..n {
        for j in (i + 1)..n {
            let pi = endpoints[i].2;
            let pj = endpoints[j].2;
            let d = {
                let dx = pi.x - pj.x;
                let dy = pi.y - pj.y;
                (dx * dx + dy * dy).sqrt()
            };
            if d <= tol {
                let ri = find(&mut parent, i);
                let rj = find(&mut parent, j);
                if ri != rj {
                    parent[rj] = ri;
                }
            }
        }
    }

    // Group endpoint indices by root.
    let mut clusters: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        let r = find(&mut parent, i);
        clusters[r].push(i);
    }

    let mut junctions = Vec::new();
    for members in clusters.into_iter().filter(|m| !m.is_empty()) {
        // Mean junction point (XY); Z from first member.
        let mut sx = 0.0;
        let mut sy = 0.0;
        let z = endpoints[members[0]].2.z;
        for &i in &members {
            sx += endpoints[i].2.x;
            sy += endpoints[i].2.y;
        }
        let inv = 1.0 / members.len() as f64;
        let point = DVec3::new(sx * inv, sy * inv, z);

        // Endpoint participants (one role per wall; if both ends coincide —
        // degenerate zero-length — keep the first seen).
        let mut participants: Vec<JunctionParticipant> = Vec::new();
        let mut seen_walls = vec![false; walls.len()];
        for &i in &members {
            let (wi, end_idx, _) = endpoints[i];
            if seen_walls[wi] {
                continue;
            }
            seen_walls[wi] = true;
            participants.push(JunctionParticipant {
                wall_index: wi,
                role: JunctionRole::Endpoint(end_idx),
            });
        }

        // Through-wall participants: junction on an interior segment.
        for (wi, axis) in walls.iter().enumerate() {
            if seen_walls[wi] || axis.len() < 2 {
                continue;
            }
            if let Some(seg) = find_through_segment(point, axis, tol) {
                seen_walls[wi] = true;
                participants.push(JunctionParticipant {
                    wall_index: wi,
                    role: JunctionRole::Through(seg),
                });
            }
        }

        if participants.len() >= 2 {
            // Stable order by wall_index for deterministic tests/callers.
            participants.sort_by_key(|p| p.wall_index);
            junctions.push(Junction {
                point,
                participants,
            });
        }
    }

    // Also detect pure T-style junctions where a single endpoint sits on
    // another wall's interior (no second endpoint in the cluster). The
    // clustering above only emits when ≥2 endpoints share a point OR when
    // through-hits bring the count to ≥2 — a lone endpoint on a through wall
    // is exactly that second case and is already handled. Done.

    junctions
}

/// Snap every endpoint participant of `junction` to `junction.point`.
/// Returns one updated axis polyline per input wall (through-walls unchanged).
pub fn apply_junction_to_axes(walls: &[&[DVec3]], junction: &Junction) -> Vec<Vec<DVec3>> {
    let mut out: Vec<Vec<DVec3>> = walls.iter().map(|a| a.to_vec()).collect();
    for p in &junction.participants {
        if let JunctionRole::Endpoint(end_idx) = p.role {
            if let Some(axis) = out.get_mut(p.wall_index) {
                if end_idx < axis.len() {
                    axis[end_idx] = junction.point;
                }
            }
        }
    }
    out
}

/// Locate the segment index on `axis` whose interior contains `point` (XY).
fn find_through_segment(point: DVec3, axis: &[DVec3], tol: f64) -> Option<usize> {
    for i in 0..axis.len().saturating_sub(1) {
        if is_on_segment_interior_2d(point, axis[i], axis[i + 1], tol) {
            return Some(i);
        }
    }
    None
}

/// Outgoing unit direction from a junction participant, used to order walls
/// around the junction by angle. Through-walls yield two opposite directions
/// via [`junction_rays`].
pub fn participant_leave_dir(axis: &[DVec3], role: JunctionRole, junction: DVec3) -> Option<(f64, f64)> {
    match role {
        JunctionRole::Endpoint(end) => {
            if axis.len() < 2 || end >= axis.len() {
                return None;
            }
            let interior = if end == 0 { 1 } else { end - 1 };
            let dx = axis[interior].x - junction.x;
            let dy = axis[interior].y - junction.y;
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-12 {
                None
            } else {
                Some((dx / len, dy / len))
            }
        }
        JunctionRole::Through(seg) => {
            // Primary leave dir toward axis[seg+1]; callers that need both
            // rays should use `junction_rays`.
            if seg + 1 >= axis.len() {
                return None;
            }
            let dx = axis[seg + 1].x - junction.x;
            let dy = axis[seg + 1].y - junction.y;
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-12 {
                None
            } else {
                Some((dx / len, dy / len))
            }
        }
    }
}

/// One outgoing ray at a junction (endpoint walls contribute one; through
/// walls contribute two opposite rays).
#[derive(Debug, Clone, Copy)]
pub struct JunctionRay {
    pub participant_index: usize,
    pub angle: f64,
    /// For through-walls: `true` when this ray points toward `axis[seg+1]`.
    pub through_forward: bool,
}

/// Build CCW-sorted outgoing rays for every participant at `junction`.
pub fn junction_rays(walls: &[&[DVec3]], junction: &Junction) -> Vec<JunctionRay> {
    let mut rays = Vec::new();
    let jp = junction.point;
    for (pi, part) in junction.participants.iter().enumerate() {
        let Some(axis) = walls.get(part.wall_index).copied() else {
            continue;
        };
        match part.role {
            JunctionRole::Endpoint(end) => {
                if let Some((dx, dy)) = participant_leave_dir(axis, JunctionRole::Endpoint(end), jp) {
                    rays.push(JunctionRay {
                        participant_index: pi,
                        angle: dy.atan2(dx),
                        through_forward: true,
                    });
                }
            }
            JunctionRole::Through(seg) => {
                if axis.len() < 2 || seg + 1 >= axis.len() {
                    continue;
                }
                // Two opposite rays along the through segment.
                for (toward_fwd, target) in [(true, axis[seg + 1]), (false, axis[seg])] {
                    let dx = target.x - jp.x;
                    let dy = target.y - jp.y;
                    let len = (dx * dx + dy * dy).sqrt();
                    if len < 1e-12 {
                        continue;
                    }
                    rays.push(JunctionRay {
                        participant_index: pi,
                        angle: dy.atan2(dx),
                        through_forward: toward_fwd,
                    });
                }
            }
        }
    }
    rays.sort_by(|a, b| {
        a.angle
            .partial_cmp(&b.angle)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    rays
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

    #[test]
    fn detect_junctions_x_crossing_four_endpoints() {
        // Four walls meeting at the origin (X-crossing).
        let e = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0)];
        let n = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 10.0, 0.0)];
        let w = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(-10.0, 0.0, 0.0)];
        let s = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, -10.0, 0.0)];
        let walls: Vec<&[DVec3]> = vec![&e, &n, &w, &s];

        let junctions = detect_junctions(&walls, JUNCTION_TOLERANCE);
        assert_eq!(junctions.len(), 1, "expected one shared junction, got {junctions:?}");
        let j = &junctions[0];
        assert!(j.point.distance(DVec3::ZERO) < 1e-9);
        assert_eq!(j.participants.len(), 4);
        assert!(j.is_multi_wall());
        assert_eq!(j.endpoint_count(), 4);
        for p in &j.participants {
            assert!(matches!(p.role, JunctionRole::Endpoint(0)));
        }
    }

    #[test]
    fn detect_junctions_t_with_third_endpoint() {
        // Through wall along X; stem from +Y; third wall from NE ending at
        // the same T point (5, 0).
        let through = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0)];
        let stem = vec![DVec3::new(5.0, 0.0, 0.0), DVec3::new(5.0, 10.0, 0.0)];
        let third = vec![DVec3::new(5.0, 0.0, 0.0), DVec3::new(10.0, 5.0, 0.0)];
        let walls: Vec<&[DVec3]> = vec![&through, &stem, &third];

        let junctions = detect_junctions(&walls, JUNCTION_TOLERANCE);
        let multi: Vec<_> = junctions.iter().filter(|j| j.is_multi_wall()).collect();
        assert_eq!(multi.len(), 1, "expected one 3-wall junction, got {junctions:?}");
        let j = multi[0];
        assert!(j.point.distance(DVec3::new(5.0, 0.0, 0.0)) < 1e-9);
        assert_eq!(j.participants.len(), 3);

        let roles: Vec<_> = j.participants.iter().map(|p| (p.wall_index, p.role)).collect();
        assert!(
            roles.iter().any(|(wi, r)| *wi == 0 && matches!(r, JunctionRole::Through(_))),
            "through wall should be Through, got {roles:?}"
        );
        assert!(
            roles
                .iter()
                .filter(|(wi, r)| (*wi == 1 || *wi == 2) && matches!(r, JunctionRole::Endpoint(_)))
                .count()
                == 2,
            "stem and third should be endpoints, got {roles:?}"
        );
    }

    #[test]
    fn detect_junctions_simple_l_is_two_participants() {
        let a = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(5.0, 0.0, 0.0)];
        let b = vec![DVec3::new(5.0, 0.0, 0.0), DVec3::new(5.0, 5.0, 0.0)];
        let walls: Vec<&[DVec3]> = vec![&a, &b];
        let junctions = detect_junctions(&walls, JUNCTION_TOLERANCE);
        assert_eq!(junctions.len(), 1);
        assert!(!junctions[0].is_multi_wall());
        assert_eq!(junctions[0].endpoint_count(), 2);
    }

    #[test]
    fn apply_junction_snaps_endpoints() {
        let a = vec![DVec3::new(0.01, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0)];
        let b = vec![DVec3::new(-0.01, 0.0, 0.0), DVec3::new(0.0, 10.0, 0.0)];
        // Use a looser tol so the slightly-offset ends cluster.
        let walls: Vec<&[DVec3]> = vec![&a, &b];
        let junctions = detect_junctions(&walls, 0.05);
        assert_eq!(junctions.len(), 1);
        let updated = apply_junction_to_axes(&walls, &junctions[0]);
        assert!(updated[0][0].distance(junctions[0].point) < 1e-12);
        assert!(updated[1][0].distance(junctions[0].point) < 1e-12);
    }
}
