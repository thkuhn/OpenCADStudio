//! Closed-loop detection for wall segments.
//!
//! Pure graph algorithm: given a set of undirected line segments (the baselines
//! of `WALL` XDATA-tagged polylines already drawn in the document), find a
//! closed loop (cycle) among them and return it as an ordered polygon.
//!
//! Endpoints are considered identical when they fall within `epsilon` of
//! each other. This is a scaffold-level algorithm: it returns the *first*
//! closed loop found by depth-first search, not every possible loop, and
//! treats an edge back to the immediate DFS parent as a non-cycle (so a
//! single wall segment alone never counts as a loop).

use std::collections::HashMap;

/// A 2D point.
pub type Point = (f64, f64);
/// An undirected line segment between two points.
pub type Segment = (Point, Point);

/// Snaps a point to a grid of size `epsilon` so nearly-coincident wall
/// endpoints are treated as the same graph node.
fn snap_key(p: Point, epsilon: f64) -> (i64, i64) {
    let e = if epsilon > 0.0 { epsilon } else { 1e-6 };
    ((p.0 / e).round() as i64, (p.1 / e).round() as i64)
}

/// Returns the node id for `p`, creating a new node (and `adj` slot) the
/// first time a point is seen within `epsilon` of any prior point.
fn node_id(
    key_to_id: &mut HashMap<(i64, i64), usize>,
    points: &mut Vec<Point>,
    adj: &mut Vec<Vec<usize>>,
    p: Point,
    epsilon: f64,
) -> usize {
    let key = snap_key(p, epsilon);
    if let Some(&id) = key_to_id.get(&key) {
        id
    } else {
        let id = points.len();
        points.push(p);
        adj.push(Vec::new());
        key_to_id.insert(key, id);
        id
    }
}

/// Subdivides a set of line segments at all mutual intersection points and T-junction endpoints,
/// returning the refined set of non-overlapping subsegments.
pub fn subdivide_segments(segments: &[Segment], epsilon: f64) -> Vec<Segment> {
    let eps = if epsilon > 0.0 { epsilon } else { 1e-4 };
    let eps_sq = eps * eps;

    // Filter out zero-length segments
    let valid_segments: Vec<Segment> = segments
        .iter()
        .copied()
        .filter(|&(a, b)| {
            let dx = b.0 - a.0;
            let dy = b.1 - a.1;
            dx * dx + dy * dy > eps_sq
        })
        .collect();

    if valid_segments.is_empty() {
        return Vec::new();
    }

    // For each segment, store a list of scalar parameters `t` in [0.0, 1.0].
    let mut split_params: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; valid_segments.len()];

    let n = valid_segments.len();
    for i in 0..n {
        let (a, b) = valid_segments[i];
        let v = (b.0 - a.0, b.1 - a.1);
        let len_sq_i = v.0 * v.0 + v.1 * v.1;
        let len_i = len_sq_i.sqrt();

        for j in (i + 1)..n {
            let (c, d) = valid_segments[j];
            let w = (d.0 - c.0, d.1 - c.1);
            let len_sq_j = w.0 * w.0 + w.1 * w.1;
            let len_j = len_sq_j.sqrt();

            let det = v.0 * w.1 - v.1 * w.0;
            let u = (c.0 - a.0, c.1 - a.1);

            if det.abs() > 1e-9 {
                // Lines are not parallel: compute intersection
                let t = (u.0 * w.1 - u.1 * w.0) / det;
                let s = (u.0 * v.1 - u.1 * v.0) / det;

                let t_margin = eps / len_i;
                let s_margin = eps / len_j;

                if t >= -t_margin && t <= 1.0 + t_margin && s >= -s_margin && s <= 1.0 + s_margin {
                    let t_clamped = t.clamp(0.0, 1.0);
                    let s_clamped = s.clamp(0.0, 1.0);
                    split_params[i].push(t_clamped);
                    split_params[j].push(s_clamped);
                }
            } else {
                // Lines are parallel/collinear: check if endpoints lie on each other's segment
                for &pt in &[c, d] {
                    let d_vec = (pt.0 - a.0, pt.1 - a.1);
                    let t = (d_vec.0 * v.0 + d_vec.1 * v.1) / len_sq_i;
                    let t_margin = eps / len_i;
                    if t >= -t_margin && t <= 1.0 + t_margin {
                        let t_clamped = t.clamp(0.0, 1.0);
                        let proj = (a.0 + t_clamped * v.0, a.1 + t_clamped * v.1);
                        let dist_sq = (pt.0 - proj.0).powi(2) + (pt.1 - proj.1).powi(2);
                        if dist_sq <= eps_sq {
                            split_params[i].push(t_clamped);
                        }
                    }
                }
                for &pt in &[a, b] {
                    let d_vec = (pt.0 - c.0, pt.1 - c.1);
                    let s = (d_vec.0 * w.0 + d_vec.1 * w.1) / len_sq_j;
                    let s_margin = eps / len_j;
                    if s >= -s_margin && s <= 1.0 + s_margin {
                        let s_clamped = s.clamp(0.0, 1.0);
                        let proj = (c.0 + s_clamped * w.0, c.1 + s_clamped * w.1);
                        let dist_sq = (pt.0 - proj.0).powi(2) + (pt.1 - proj.1).powi(2);
                        if dist_sq <= eps_sq {
                            split_params[j].push(s_clamped);
                        }
                    }
                }
            }
        }
    }

    // Also check all endpoints against all segments for T-junction proximity
    for (i, &(a, b)) in valid_segments.iter().enumerate() {
        let v = (b.0 - a.0, b.1 - a.1);
        let len_sq = v.0 * v.0 + v.1 * v.1;
        let len = len_sq.sqrt();
        let t_margin = eps / len;

        for (j, &(c, d)) in valid_segments.iter().enumerate() {
            if i == j {
                continue;
            }
            for &pt in &[c, d] {
                let d_vec = (pt.0 - a.0, pt.1 - a.1);
                let t = (d_vec.0 * v.0 + d_vec.1 * v.1) / len_sq;
                if t >= -t_margin && t <= 1.0 + t_margin {
                    let t_clamped = t.clamp(0.0, 1.0);
                    let proj = (a.0 + t_clamped * v.0, a.1 + t_clamped * v.1);
                    let dist_sq = (pt.0 - proj.0).powi(2) + (pt.1 - proj.1).powi(2);
                    if dist_sq <= eps_sq {
                        split_params[i].push(t_clamped);
                    }
                }
            }
        }
    }

    let mut result = Vec::new();
    let mut seen_subsegments = std::collections::HashSet::new();

    for (i, &(a, b)) in valid_segments.iter().enumerate() {
        let v = (b.0 - a.0, b.1 - a.1);
        let len = (v.0 * v.0 + v.1 * v.1).sqrt();

        let mut params = split_params[i].clone();
        params.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));

        // Deduplicate close t-values
        let mut unique_params = Vec::new();
        for t in params {
            if unique_params.is_empty() {
                unique_params.push(t);
            } else {
                let last = *unique_params.last().unwrap();
                if (t - last) * len > eps * 0.5 {
                    unique_params.push(t);
                }
            }
        }

        for window in unique_params.windows(2) {
            let t0 = window[0];
            let t1 = window[1];
            if (t1 - t0) * len <= eps * 0.5 {
                continue;
            }
            let p0 = (a.0 + t0 * v.0, a.1 + t0 * v.1);
            let p1 = (a.0 + t1 * v.0, a.1 + t1 * v.1);

            let k0 = snap_key(p0, eps);
            let k1 = snap_key(p1, eps);
            if k0 == k1 {
                continue;
            }
            let seg_key = if k0 < k1 { (k0, k1) } else { (k1, k0) };
            if seen_subsegments.insert(seg_key) {
                result.push((p0, p1));
            }
        }
    }

    result
}

/// Finds the first closed loop formed by the given wall segments, if any.
///
/// Returns the loop as an ordered list of points (implicitly closed — the
/// first point is not repeated at the end), suitable for
/// [`crate::modules::aec::engine::room::Room::from_polygon`]. Returns `None`
/// when the segments do not contain any closed loop (e.g. a single open wall
/// run).
pub fn find_closed_loop(segments: &[Segment], epsilon: f64) -> Option<Vec<Point>> {
    let sub = subdivide_segments(segments, epsilon);
    if sub.len() < 3 {
        // A closed polygon needs at least 3 edges.
        return None;
    }

    let mut key_to_id: HashMap<(i64, i64), usize> = HashMap::new();
    let mut points: Vec<Point> = Vec::new();
    let mut adj: Vec<Vec<usize>> = Vec::new();

    for &(a, b) in &sub {
        let ida = node_id(&mut key_to_id, &mut points, &mut adj, a, epsilon);
        let idb = node_id(&mut key_to_id, &mut points, &mut adj, b, epsilon);
        if ida == idb {
            continue; // zero-length segment, not a useful edge
        }
        adj[ida].push(idb);
        adj[idb].push(ida);
    }

    let n = points.len();
    let mut visited = vec![false; n];

    for start in 0..n {
        if visited[start] {
            continue;
        }
        let mut on_stack = vec![false; n];
        let mut stack: Vec<usize> = Vec::new();
        if let Some(cycle) = dfs(start, usize::MAX, &adj, &mut visited, &mut on_stack, &mut stack) {
            return Some(cycle.into_iter().map(|id| points[id]).collect());
        }
    }

    None
}

/// Finds all simple closed loops formed by the given wall segments.
pub fn find_all_closed_loops(segments: &[Segment], epsilon: f64) -> Vec<Vec<Point>> {
    let sub = subdivide_segments(segments, epsilon);
    if sub.len() < 3 {
        return Vec::new();
    }

    let mut key_to_id: HashMap<(i64, i64), usize> = HashMap::new();
    let mut points: Vec<Point> = Vec::new();
    let mut adj: Vec<Vec<usize>> = Vec::new();

    for &(a, b) in &sub {
        let ida = node_id(&mut key_to_id, &mut points, &mut adj, a, epsilon);
        let idb = node_id(&mut key_to_id, &mut points, &mut adj, b, epsilon);
        if ida == idb {
            continue;
        }
        if !adj[ida].contains(&idb) {
            adj[ida].push(idb);
        }
        if !adj[idb].contains(&ida) {
            adj[idb].push(ida);
        }
    }

    let n = points.len();
    let mut all_cycles: Vec<Vec<usize>> = Vec::new();

    for start in 0..n {
        let mut path = vec![start];
        let mut visited = vec![false; n];
        visited[start] = true;
        find_cycles_from(start, start, usize::MAX, &adj, &mut visited, &mut path, &mut all_cycles, 64);
    }

    let mut result: Vec<Vec<Point>> = Vec::new();
    let mut seen_keys = std::collections::HashSet::new();

    for cycle in all_cycles {
        if cycle.len() < 3 {
            continue;
        }
        let poly: Vec<Point> = cycle.iter().map(|&id| points[id]).collect();
        let a = crate::modules::aec::engine::geometry::area(&poly);
        if a < 1e-4 {
            continue;
        }
        // Canonical sorted representation of vertex IDs for deduplication
        let mut min_pos = 0;
        for i in 1..cycle.len() {
            if cycle[i] < cycle[min_pos] {
                min_pos = i;
            }
        }
        let fwd: Vec<usize> = (0..cycle.len()).map(|i| cycle[(min_pos + i) % cycle.len()]).collect();
        let mut rev: Vec<usize> = Vec::with_capacity(cycle.len());
        rev.push(cycle[min_pos]);
        for i in 1..cycle.len() {
            rev.push(cycle[(min_pos + cycle.len() - i) % cycle.len()]);
        }
        let canon = if fwd < rev { fwd } else { rev };
        if seen_keys.insert(canon) {
            result.push(poly);
        }
    }

    result
}

fn find_cycles_from(
    start: usize,
    curr: usize,
    parent: usize,
    adj: &[Vec<usize>],
    visited: &mut [bool],
    path: &mut Vec<usize>,
    cycles: &mut Vec<Vec<usize>>,
    max_depth: usize,
) {
    if path.len() > max_depth {
        return;
    }
    for &next in &adj[curr] {
        if next == parent {
            continue;
        }
        if next == start && path.len() >= 3 {
            cycles.push(path.clone());
        } else if next > start && !visited[next] {
            visited[next] = true;
            path.push(next);
            find_cycles_from(start, next, curr, adj, visited, path, cycles, max_depth);
            path.pop();
            visited[next] = false;
        }
    }
}

/// Finds the smallest closed wall loop that encloses the given pick point `pt`.
pub fn find_closed_loop_at_point(segments: &[Segment], pt: Point, epsilon: f64) -> Option<Vec<Point>> {
    let loops = find_all_closed_loops(segments, epsilon);
    let mut candidates: Vec<Vec<Point>> = loops
        .into_iter()
        .filter(|poly| crate::modules::aec::engine::geometry::point_in_polygon(pt, poly))
        .collect();

    if candidates.is_empty() {
        return None;
    }

    candidates.sort_by(|a, b| {
        let area_a = crate::modules::aec::engine::geometry::area(a);
        let area_b = crate::modules::aec::engine::geometry::area(b);
        area_a.partial_cmp(&area_b).unwrap_or(std::cmp::Ordering::Equal)
    });

    Some(candidates.remove(0))
}

/// Depth-first search for a cycle, returning it as a list of node ids in
/// loop order (first id not repeated at the end) once found.
fn dfs(
    u: usize,
    parent: usize,
    adj: &[Vec<usize>],
    visited: &mut [bool],
    on_stack: &mut [bool],
    stack: &mut Vec<usize>,
) -> Option<Vec<usize>> {
    visited[u] = true;
    on_stack[u] = true;
    stack.push(u);

    // Skip at most one edge back to the immediate parent, so a simple
    // "there and back" pair of nodes never counts as a cycle.
    let mut skipped_parent = false;

    for &v in &adj[u] {
        if v == parent && !skipped_parent {
            skipped_parent = true;
            continue;
        }
        if on_stack[v] {
            let start = stack.iter().position(|&x| x == v).expect("v is on_stack");
            return Some(stack[start..].to_vec());
        }
        if !visited[v] {
            if let Some(cycle) = dfs(v, u, adj, visited, on_stack, stack) {
                return Some(cycle);
            }
        }
    }

    stack.pop();
    on_stack[u] = false;
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_closed_rectangle_loop_from_four_wall_segments() {
        let segments: Vec<Segment> = vec![
            ((0.0, 0.0), (4.0, 0.0)),
            ((4.0, 0.0), (4.0, 3.0)),
            ((4.0, 3.0), (0.0, 3.0)),
            ((0.0, 3.0), (0.0, 0.0)),
        ];

        let loop_points = find_closed_loop(&segments, 1e-3).expect("rectangle loop expected");
        assert_eq!(loop_points.len(), 4);

        // The area of the returned polygon must match the rectangle,
        // independent of the exact starting point/winding order.
        let area = crate::modules::aec::engine::geometry::area(&loop_points);
        assert!((area - 12.0).abs() < 1e-6, "unexpected area: {area}");
    }

    #[test]
    fn finds_closed_loop_with_slightly_misaligned_endpoints_within_epsilon() {
        let segments: Vec<Segment> = vec![
            ((0.0, 0.0), (4.0, 0.0002)),
            ((4.0, 0.0), (4.0002, 3.0)),
            ((4.0, 3.0), (0.0002, 3.0)),
            ((0.0, 3.0), (0.0, 0.0002)),
        ];

        let loop_points = find_closed_loop(&segments, 1e-2).expect("loop within epsilon expected");
        assert_eq!(loop_points.len(), 4);
    }

    #[test]
    fn returns_none_for_open_wall_run() {
        let segments: Vec<Segment> = vec![
            ((0.0, 0.0), (4.0, 0.0)),
            ((4.0, 0.0), (4.0, 3.0)),
            ((4.0, 3.0), (0.0, 3.0)),
            // Missing closing segment back to (0.0, 0.0).
        ];

        assert!(find_closed_loop(&segments, 1e-3).is_none());
    }

    #[test]
    fn returns_none_for_fewer_than_three_segments() {
        let segments: Vec<Segment> = vec![
            ((0.0, 0.0), (4.0, 0.0)),
            ((4.0, 0.0), (4.0, 3.0)),
        ];

        assert!(find_closed_loop(&segments, 1e-3).is_none());
    }

    #[test]
    fn finds_loop_among_extra_disconnected_wall_segments() {
        // A closed triangle plus an unrelated, disconnected wall segment.
        let segments: Vec<Segment> = vec![
            ((10.0, 10.0), (20.0, 10.0)), // unrelated open wall
            ((0.0, 0.0), (4.0, 0.0)),
            ((4.0, 0.0), (2.0, 3.0)),
            ((2.0, 3.0), (0.0, 0.0)),
        ];

        let loop_points = find_closed_loop(&segments, 1e-3).expect("triangle loop expected");
        assert_eq!(loop_points.len(), 3);
        let area = crate::modules::aec::engine::geometry::area(&loop_points);
        assert!((area - 6.0).abs() < 1e-6, "unexpected area: {area}");
    }

    #[test]
    fn finds_adjacent_rooms_and_point_in_room() {
        // Two adjacent rooms sharing a wall:
        // Room 1: (0,0)-(4,0)-(4,3)-(0,3)
        // Room 2: (4,0)-(8,0)-(8,3)-(4,3)
        let segments: Vec<Segment> = vec![
            ((0.0, 0.0), (4.0, 0.0)),
            ((4.0, 0.0), (4.0, 3.0)),
            ((4.0, 3.0), (0.0, 3.0)),
            ((0.0, 3.0), (0.0, 0.0)),
            ((4.0, 0.0), (8.0, 0.0)),
            ((8.0, 0.0), (8.0, 3.0)),
            ((8.0, 3.0), (4.0, 3.0)),
        ];

        let loops = find_all_closed_loops(&segments, 1e-3);
        assert!(loops.len() >= 2, "must find at least 2 loops, got {}", loops.len());

        let loop1 = find_closed_loop_at_point(&segments, (2.0, 1.5), 1e-3).expect("room 1 loop");
        let a1 = crate::modules::aec::engine::geometry::area(&loop1);
        assert!((a1 - 12.0).abs() < 1e-6, "room 1 area must be 12.0, got {a1}");

        let loop2 = find_closed_loop_at_point(&segments, (6.0, 1.5), 1e-3).expect("room 2 loop");
        let a2 = crate::modules::aec::engine::geometry::area(&loop2);
        assert!((a2 - 12.0).abs() < 1e-6, "room 2 area must be 12.0, got {a2}");

        assert!(find_closed_loop_at_point(&segments, (10.0, 1.5), 1e-3).is_none());
    }

    #[test]
    fn finds_rooms_with_t_junction_partition_walls() {
        // Outer rectangle: (0,0) -> (10,0) -> (10,6) -> (0,6) -> (0,0)
        // Partition 1 (vertical, T-junction at y=0 and y=6): (4,0) -> (4,6)
        // Partition 2 (horizontal, T-junction at x=4 and x=10): (4,3) -> (10,3)
        let segments: Vec<Segment> = vec![
            ((0.0, 0.0), (10.0, 0.0)),
            ((10.0, 0.0), (10.0, 6.0)),
            ((10.0, 6.0), (0.0, 6.0)),
            ((0.0, 6.0), (0.0, 0.0)),
            ((4.0, 0.0), (4.0, 6.0)),
            ((4.0, 3.0), (10.0, 3.0)),
        ];

        let loop_left = find_closed_loop_at_point(&segments, (2.0, 3.0), 1e-3)
            .expect("left room must be detected at (2, 3)");
        let a_left = crate::modules::aec::engine::geometry::area(&loop_left);
        assert!((a_left - 24.0).abs() < 1e-6, "left room area expected 24.0, got {a_left}");

        let loop_bottom_right = find_closed_loop_at_point(&segments, (7.0, 1.5), 1e-3)
            .expect("bottom right room must be detected at (7, 1.5)");
        let a_br = crate::modules::aec::engine::geometry::area(&loop_bottom_right);
        assert!((a_br - 18.0).abs() < 1e-6, "bottom right room area expected 18.0, got {a_br}");

        let loop_top_right = find_closed_loop_at_point(&segments, (7.0, 4.5), 1e-3)
            .expect("top right room must be detected at (7, 4.5)");
        let a_tr = crate::modules::aec::engine::geometry::area(&loop_top_right);
        assert!((a_tr - 18.0).abs() < 1e-6, "top right room area expected 18.0, got {a_tr}");
    }
}
