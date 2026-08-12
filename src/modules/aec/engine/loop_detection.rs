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

/// Finds the first closed loop formed by the given wall segments, if any.
///
/// Returns the loop as an ordered list of points (implicitly closed — the
/// first point is not repeated at the end), suitable for
/// [`crate::modules::aec::engine::room::Room::from_polygon`]. Returns `None`
/// when the segments do not contain any closed loop (e.g. a single open wall
/// run).
pub fn find_closed_loop(segments: &[Segment], epsilon: f64) -> Option<Vec<Point>> {
    if segments.len() < 3 {
        // A closed polygon needs at least 3 edges.
        return None;
    }

    let mut key_to_id: HashMap<(i64, i64), usize> = HashMap::new();
    let mut points: Vec<Point> = Vec::new();
    let mut adj: Vec<Vec<usize>> = Vec::new();

    for &(a, b) in segments {
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
}
