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
    let loops = find_all_closed_loops(segments, epsilon);
    loops.into_iter().next()
}

/// Simplifies a closed polygon by removing redundant intermediate collinear vertices.
fn simplify_collinear(poly: &[Point], _epsilon: f64) -> Vec<Point> {
    let n = poly.len();
    if n < 3 {
        return poly.to_vec();
    }
    let mut result = Vec::new();
    for i in 0..n {
        let prev = if i == 0 { poly[n - 1] } else { poly[i - 1] };
        let curr = poly[i];
        let next = poly[(i + 1) % n];

        let v0 = (curr.0 - prev.0, curr.1 - prev.1);
        let v1 = (next.0 - curr.0, next.1 - curr.1);
        let len0 = (v0.0 * v0.0 + v0.1 * v0.1).sqrt();
        let len1 = (v1.0 * v1.0 + v1.1 * v1.1).sqrt();

        if len0 < 1e-6 || len1 < 1e-6 {
            continue;
        }

        let cross = (v0.0 * v1.1 - v0.1 * v1.0).abs();
        let dot = v0.0 * v1.0 + v0.1 * v1.1;

        // If collinear and heading in the same direction, skip the intermediate vertex
        if cross <= 1e-3 * len0 * len1 && dot > 0.0 {
            continue;
        }
        result.push(curr);
    }
    if result.len() < 3 {
        poly.to_vec()
    } else {
        result
    }
}

/// Finds all simple closed loops (minimal planar faces) formed by the given wall segments
/// using planar face traversal (angular sorting / left-hand rule).
pub fn find_all_closed_loops(segments: &[Segment], epsilon: f64) -> Vec<Vec<Point>> {
    let sub = subdivide_segments(segments, epsilon);
    if sub.len() < 3 {
        return Vec::new();
    }

    let mut key_to_id: HashMap<(i64, i64), usize> = HashMap::new();
    let mut points: Vec<Point> = Vec::new();
    let mut adj_set: Vec<std::collections::HashSet<usize>> = Vec::new();

    for &(a, b) in &sub {
        let mut adj_dummy = Vec::new();
        let ida = node_id(&mut key_to_id, &mut points, &mut adj_dummy, a, epsilon);
        let idb = node_id(&mut key_to_id, &mut points, &mut adj_dummy, b, epsilon);
        if ida == idb {
            continue;
        }
        while adj_set.len() < points.len() {
            adj_set.push(std::collections::HashSet::new());
        }
        adj_set[ida].insert(idb);
        adj_set[idb].insert(ida);
    }

    let n = points.len();
    if n < 3 {
        return Vec::new();
    }

    // Iterative pruning of dead-end vertices (degree <= 1)
    let mut active = vec![true; n];
    let mut queue: Vec<usize> = (0..n).filter(|&i| adj_set[i].len() <= 1).collect();
    while let Some(u) = queue.pop() {
        if !active[u] {
            continue;
        }
        active[u] = false;
        let neighbors: Vec<usize> = adj_set[u].iter().copied().collect();
        for v in neighbors {
            if active[v] {
                adj_set[v].remove(&u);
                if adj_set[v].len() <= 1 {
                    queue.push(v);
                }
            }
        }
        adj_set[u].clear();
    }

    // Build radially sorted outgoing half-edges for each active vertex
    let mut adj: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    for u in 0..n {
        if !active[u] {
            continue;
        }
        let (ux, uy) = points[u];
        let mut edges = Vec::new();
        for &v in &adj_set[u] {
            if active[v] {
                let (vx, vy) = points[v];
                let angle = (vy - uy).atan2(vx - ux);
                edges.push((v, angle));
            }
        }
        edges.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        adj[u] = edges;
    }

    // Traverse all planar minimal faces via left-hand rule (half-edge traversal)
    let mut visited_half_edges = std::collections::HashSet::new();
    let mut faces: Vec<Vec<Point>> = Vec::new();

    for u in 0..n {
        if !active[u] {
            continue;
        }
        for &(v, _) in &adj[u] {
            if visited_half_edges.contains(&(u, v)) {
                continue;
            }

            let mut cycle = Vec::new();
            let mut step_edges = Vec::new();
            let mut curr_u = u;
            let mut curr_v = v;
            let mut trapped = false;

            for _ in 0..(n * 2 + 10) {
                cycle.push(curr_u);
                step_edges.push((curr_u, curr_v));

                let edges_v = &adj[curr_v];
                if edges_v.is_empty() {
                    trapped = true;
                    break;
                }

                let pos = edges_v.iter().position(|&(w, _)| w == curr_u);
                let Some(idx) = pos else {
                    trapped = true;
                    break;
                };

                // Turn most CCW from incoming direction (preceding edge in CCW radial order)
                let next_idx = (idx + edges_v.len() - 1) % edges_v.len();
                let (next_w, _) = edges_v[next_idx];

                if next_w == v && curr_v == u {
                    break;
                }
                if next_w == curr_u {
                    trapped = true;
                    break;
                }

                curr_u = curr_v;
                curr_v = next_w;

                if (curr_u, curr_v) == (u, v) {
                    break;
                }
            }

            for edge in step_edges {
                visited_half_edges.insert(edge);
            }

            if !trapped && cycle.len() >= 3 {
                let poly: Vec<Point> = cycle.iter().map(|&id| points[id]).collect();
                let simplified = simplify_collinear(&poly, epsilon);
                if simplified.len() >= 3 {
                    let s_area = crate::modules::aec::engine::geometry::signed_area(&simplified);
                    // Strictly positive signed area corresponds to an interior face (CCW winding)
                    if s_area > 1e-4 {
                        faces.push(simplified);
                    }
                }
            }
        }
    }

    // Deduplicate faces
    let mut result: Vec<Vec<Point>> = Vec::new();
    let mut seen_keys = std::collections::HashSet::new();

    for poly in faces {
        let keys: Vec<(i64, i64)> = poly.iter().map(|&p| snap_key(p, epsilon)).collect();
        let min_pos = keys.iter().enumerate().min_by_key(|&(_, k)| k).map(|(i, _)| i).unwrap_or(0);
        let mut canon = Vec::with_capacity(poly.len());
        for i in 0..poly.len() {
            canon.push(keys[(min_pos + i) % poly.len()]);
        }
        if seen_keys.insert(canon) {
            result.push(poly);
        }
    }

    result
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

    #[test]
    fn finds_rooms_in_structural_boundary_mesh_with_t_junction() {
        // Simulating the structural segments from 4 outer walls and 1 vertical partition wall:
        // Outer room dimensions: 10x6 m, partition at x=5 m dividing into two 5x6 m rooms (axis-based).
        // Wall thickness 0.24m (structural offsets -0.12, +0.12).
        // Inner dimensions of Room 1: width = (5.0 - 0.12 - 0.12) = 4.76, height = (6.0 - 0.24) = 5.76. Area = 27.4176 m².
        // Inner dimensions of Room 2: width = (5.0 - 0.12 - 0.12) = 4.76, height = (6.0 - 0.24) = 5.76. Area = 27.4176 m².
        let mut segments: Vec<Segment> = Vec::new();

        // Helper to add structural faces + caps with extension
        let add_wall = |segs: &mut Vec<Segment>, a: Point, b: Point| {
            let dx = b.0 - a.0;
            let dy = b.1 - a.1;
            let len = (dx * dx + dy * dy).sqrt();
            let nx = -dy / len;
            let ny = dx / len;
            let ux = dx / len;
            let uy = dy / len;
            let ext = 0.48;
            let max_off = 0.12;
            let min_off = -0.12;

            let l0 = (a.0 + nx * max_off - ux * ext, a.1 + ny * max_off - uy * ext);
            let l1 = (b.0 + nx * max_off + ux * ext, b.1 + ny * max_off + uy * ext);
            segs.push((l0, l1));

            let r0 = (a.0 + nx * min_off - ux * ext, a.1 + ny * min_off - uy * ext);
            let r1 = (b.0 + nx * min_off + ux * ext, b.1 + ny * min_off + uy * ext);
            segs.push((r0, r1));

            let cap_a = ((a.0 + nx * min_off, a.1 + ny * min_off), (a.0 + nx * max_off, a.1 + ny * max_off));
            let cap_b = ((b.0 + nx * min_off, b.1 + ny * min_off), (b.0 + nx * max_off, b.1 + ny * max_off));
            segs.push(cap_a);
            segs.push(cap_b);
        };

        // 4 outer walls + 1 partition wall
        add_wall(&mut segments, (0.0, 0.0), (10.0, 0.0));
        add_wall(&mut segments, (10.0, 0.0), (10.0, 6.0));
        add_wall(&mut segments, (10.0, 6.0), (0.0, 6.0));
        add_wall(&mut segments, (0.0, 6.0), (0.0, 0.0));
        add_wall(&mut segments, (5.0, 0.0), (5.0, 6.0));

        let loop1 = find_closed_loop_at_point(&segments, (2.5, 3.0), 1e-3)
            .expect("room 1 must be detected at (2.5, 3.0)");
        let a1 = crate::modules::aec::engine::geometry::area(&loop1);
        assert!((a1 - 27.4176).abs() < 1e-2, "room 1 area expected ~27.42, got {a1}");

        let loop2 = find_closed_loop_at_point(&segments, (7.5, 3.0), 1e-3)
            .expect("room 2 must be detected at (7.5, 3.0)");
        let a2 = crate::modules::aec::engine::geometry::area(&loop2);
        assert!((a2 - 27.4176).abs() < 1e-2, "room 2 area expected ~27.42, got {a2}");
    }
}
