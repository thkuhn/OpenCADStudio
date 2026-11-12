pub(super) fn closest_point_on_line(
    p: (f64, f64),
    line: ((f64, f64), (f64, f64)),
) -> (f64, f64) {
    let (a, b) = line;
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-24 {
        return a;
    }
    let t = ((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2;
    (a.0 + t * dx, a.1 + t * dy)
}

/// Boundary line pair on the other wall at the join.
///
/// For an L / T-stem (`end_b = Some(idx)`) this is the end segment of each
/// boundary. For a T through-wall (`end_b = None`) it is the boundary segment
/// whose parameter range contains the join point projected from A's end.
pub(super) fn other_boundary_lines(
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
pub(super) fn extended_line(from: (f64, f64), to: (f64, f64)) -> ((f64, f64), (f64, f64)) {
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

pub(super) fn intersect_lines_2d(
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

pub(super) fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    (dx * dx + dy * dy).sqrt()
}

pub(super) fn point_seg_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
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

/// Merge two single-end mitered results for the *same* layer/base footprint
/// into one polygon that reflects both ends' joins at once.
///
/// Each of `end_a_result` / `end_b_result` is the output of a miter helper
/// that only ever modifies the vertices near *its own* end, leaving every
/// other vertex byte-for-byte identical to `base` (see
/// [`mitered_layer_footprints`] / [`mitered_layer_footprints_with_override`]
/// docs). That invariant makes the merge purely positional: for each vertex
/// index, prefer whichever of the two results actually differs from `base`
/// at that index (ties/both-differ favor `end_a_result`, both-same keeps
/// `base`). `None` inputs are treated as "no change from base".
///
/// Returns `None` (falls back to `base`) when a candidate's vertex count
/// doesn't match `base`'s — this can only happen if the two calls were not
/// actually built from the same axis/layer topology, which callers should
/// avoid.
pub fn merge_end_footprints(
    base: &[(f64, f64)],
    end_a_result: Option<&Vec<(f64, f64)>>,
    end_b_result: Option<&Vec<(f64, f64)>>,
) -> Option<Vec<(f64, f64)>> {
    let a = end_a_result.filter(|v| v.len() == base.len());
    let b = end_b_result.filter(|v| v.len() == base.len());
    match (a, b) {
        (None, None) => None,
        (Some(a), None) => Some(a.clone()),
        (None, Some(b)) => Some(b.clone()),
        (Some(a), Some(b)) => {
            let eps = 1e-9;
            let close = |p: (f64, f64), q: (f64, f64)| {
                (p.0 - q.0).abs() < eps && (p.1 - q.1).abs() < eps
            };
            let merged: Vec<(f64, f64)> = base
                .iter()
                .enumerate()
                .map(|(idx, &bp)| {
                    let av = a[idx];
                    let bv = b[idx];
                    if !close(av, bp) {
                        av
                    } else if !close(bv, bp) {
                        bv
                    } else {
                        bp
                    }
                })
                .collect();
            Some(merged)
        }
    }
}
