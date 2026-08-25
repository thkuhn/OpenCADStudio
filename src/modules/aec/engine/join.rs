use glam::DVec3;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A stable, material-/role-based reference to a wall layer, used by
/// [`JunctionOverride`] so manual join overrides survive layer reordering
/// (unlike a raw layer index, which shifts when layers are inserted/removed).
///
/// Identity is `material_id` plus an optional `role_tag` (see
/// [`crate::modules::aec::engine::wall_style::Layer`]) — the same fields the
/// miter matcher (`MiterLayer`) already uses to pair layers across a join.
///
/// `index` additionally records the layer's position within its owning
/// wall's layer stack at the time the reference was captured. `material_id`
/// (+ `role_tag`) alone cannot distinguish two layers that use the same
/// material without an explicit role tag (e.g. two plaster layers on either
/// side of a wall) — without `index`, such a pair would be indistinguishable
/// and an override created for one would silently apply to both. Defaults to
/// `0` for records persisted before this field existed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LayerRef {
    /// Identifier of the material for the referenced layer.
    pub material_id: String,
    /// Optional role tag (e.g. `"Tragschale"`), used to disambiguate layers
    /// that share the same material within a wall style.
    #[serde(default)]
    pub role_tag: Option<String>,
    /// Position of the layer within its owning wall's layer stack.
    #[serde(default)]
    pub index: usize,
}

/// Manual override for how two layers (or a layer and the outer face) are
/// joined at a specific junction end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum JoinOverrideStyle {
    Miter,
    Butt,
    OuterFace,
    NoExtend,
}

/// Override for a single pair of layers at a junction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerPairOverride {
    /// Material/function-based reference to the first layer, not a raw index.
    pub layer_a: LayerRef,
    /// Material/function-based reference to the second layer. `None` means
    /// the through-wall side / outer face rather than a specific layer.
    #[serde(default)]
    pub layer_b: Option<LayerRef>,
    pub style: JoinOverrideStyle,
}

/// Manual join-constraint overrides for one end of a wall axis (a
/// "junction"). Persisted as XDATA on the wall axis entity, keyed by which
/// end of the axis the junction sits at (see `write_junction_override` /
/// `read_junction_override` in `commands.rs`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct JunctionOverride {
    #[serde(default)]
    pub default_style: Option<JoinOverrideStyle>,
    #[serde(default)]
    pub layer_pairs: Vec<LayerPairOverride>,
}

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
    Ambiguous,
}

impl fmt::Display for JoinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JoinError::Parallel => write!(f, "Parallel axes"),
            JoinError::NoIntersection => write!(f, "No intersection found"),
            JoinError::Degenerate => write!(f, "Degenerate axis"),
            JoinError::Ambiguous => write!(f, "Ambiguous junction set"),
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

    // Explicit End-vs-Mid classification. Near-coincident hits at a through
    // wall's endpoint stay L (both End); a hit strictly interior to one axis
    // with the other wall ending there is T — and the through axis is never
    // shortened.
    let classify = |axis: &[DVec3], isect: DVec3| -> Option<JunctionRole> {
        classify_axis_at_point(axis, isect, END_MID_TOLERANCE)
    };

    if let Some((isect, idx_a, idx_b)) = best_l {
        let role_a = classify(axis_a, isect);
        let role_b = classify(axis_b, isect);
        let both_end = matches!(role_a, Some(JunctionRole::Endpoint(_)))
            && matches!(role_b, Some(JunctionRole::Endpoint(_)));
        // Extension L: intersection is beyond both finite axes (roles None)
        // or one role is End and the other is an extension (None, not Mid).
        let a_mid = matches!(role_a, Some(JunctionRole::Through(_)));
        let b_mid = matches!(role_b, Some(JunctionRole::Through(_)));
        let a_head = axis_overhangs_both_sides(axis_a, isect, END_MID_TOLERANCE);
        let b_head = axis_overhangs_both_sides(axis_b, isect, END_MID_TOLERANCE);
        // Kopfwand of a T continues past the stem on both sides — never L.
        let use_l = (both_end || (!a_mid && !b_mid)) && !(a_head ^ b_head);
        if use_l {
            let mut new_a = axis_a.to_vec();
            new_a[idx_a] = isect;
            let mut new_b = axis_b.to_vec();
            new_b[idx_b] = isect;
            return Ok((new_a, new_b, JoinKind::L, Some(idx_a), Some(idx_b)));
        }
        // Intersection classified as T via Kopfwand overhang: keep the
        // through axis full length and snap only the stem end.
        if a_head ^ b_head {
            if a_head {
                let mut new_b = axis_b.to_vec();
                new_b[idx_b] = isect;
                return Ok((axis_a.to_vec(), new_b, JoinKind::T, None, Some(idx_b)));
            } else {
                let mut new_a = axis_a.to_vec();
                new_a[idx_a] = isect;
                return Ok((new_a, axis_b.to_vec(), JoinKind::T, Some(idx_a), None));
            }
        }
    }

    if let Some((isect, idx_stem, _idx_through_seg, a_is_stem)) = best_t {
        let (stem_axis, through_axis) = if a_is_stem {
            (axis_a, axis_b)
        } else {
            (axis_b, axis_a)
        };
        let stem_role = classify(stem_axis, isect);
        let through_role = classify(through_axis, isect);
        let stem_is_end = matches!(stem_role, Some(JunctionRole::Endpoint(_)) | None);
        let through_is_mid = matches!(through_role, Some(JunctionRole::Through(_)));
        if stem_is_end && through_is_mid {
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
    }

    Err(JoinError::NoIntersection)
}

/// Always form an L-corner: snap both nearest end vertices to the
/// intersection of the end-segment lines. Shortens or lengthens both axes.
/// Never classifies as T, even if one wall currently overhangs the hit.
pub fn join_wall_axes_as_l(
    axis_a: &[DVec3],
    axis_b: &[DVec3],
) -> Result<(Vec<DVec3>, Vec<DVec3>, JoinKind, Option<usize>, Option<usize>), JoinError> {
    if axis_a.len() < 2 || axis_b.len() < 2 {
        return Err(JoinError::Degenerate);
    }
    let ends_a = [
        (0, axis_a[0], axis_a[1]),
        (axis_a.len() - 1, axis_a[axis_a.len() - 1], axis_a[axis_a.len() - 2]),
    ];
    let ends_b = [
        (0, axis_b[0], axis_b[1]),
        (axis_b.len() - 1, axis_b[axis_b.len() - 1], axis_b[axis_b.len() - 2]),
    ];
    let mut best: Option<(DVec3, usize, usize)> = None;
    let mut best_dist = f64::INFINITY;
    let mut best_idx_sum = 0usize;
    for (idx_a, p1, p2) in ends_a {
        for (idx_b, p3, p4) in ends_b {
            if let Some(isect) = intersect_lines_2d(p1, p2, p3, p4) {
                let dist = p1.distance(isect) + p3.distance(isect);
                let idx_sum = idx_a + idx_b;
                // On a tie (typical 2-point through wall, both ends equally
                // far from the hit) snap the later vertices so the original
                // start remains and the overhang past the corner is trimmed.
                if dist < best_dist - 1e-9 || ((dist - best_dist).abs() <= 1e-9 && idx_sum > best_idx_sum)
                {
                    best_dist = dist;
                    best_idx_sum = idx_sum;
                    best = Some((isect, idx_a, idx_b));
                }
            }
        }
    }
    let Some((isect, idx_a, idx_b)) = best else {
        return Err(JoinError::NoIntersection);
    };
    let mut new_a = axis_a.to_vec();
    new_a[idx_a] = isect;
    let mut new_b = axis_b.to_vec();
    new_b[idx_b] = isect;
    Ok((new_a, new_b, JoinKind::L, Some(idx_a), Some(idx_b)))
}

/// Move only `source`'s nearer end to the intersection with `target`'s axis
/// line(s). `target` is never shortened or lengthened.
pub fn extend_axis_to_other(
    source: &[DVec3],
    target: &[DVec3],
) -> Result<(Vec<DVec3>, usize, DVec3), JoinError> {
    if source.len() < 2 || target.len() < 2 {
        return Err(JoinError::Degenerate);
    }
    let ends = [
        (0, source[0], source[1]),
        (source.len() - 1, source[source.len() - 1], source[source.len() - 2]),
    ];
    let mut best: Option<(DVec3, usize)> = None;
    let mut best_dist = f64::INFINITY;
    for (idx, p1, p2) in ends {
        for i in 0..target.len() - 1 {
            if let Some(isect) = intersect_lines_2d(p1, p2, target[i], target[i + 1]) {
                let dist = p1.distance(isect);
                if dist < best_dist {
                    best_dist = dist;
                    best = Some((isect, idx));
                }
            }
        }
    }
    let Some((isect, idx)) = best else {
        return Err(JoinError::NoIntersection);
    };
    let mut new_source = source.to_vec();
    new_source[idx] = isect;
    Ok((new_source, idx, isect))
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

/// Distance from an axis endpoint below which a join hit is End, not Mid.
/// Prevents a T-stem that lands almost on a through-wall end from being
/// classified as T (which would leave a hairline gap instead of an L miter).
pub const END_MID_TOLERANCE: f64 = 1e-3;

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

        // A wall clustered by a nearby endpoint is still Through when the
        // head/crossbar continues past the junction on *both* sides (T),
        // even if that endpoint sits inside the clustering snap radius.
        for p in &mut participants {
            let axis = walls[p.wall_index];
            if !axis_overhangs_both_sides(axis, point, END_MID_TOLERANCE) {
                continue;
            }
            // Cluster `tol` is too loose for the interior-vs-end test
            // (`is_on_segment_interior` would reject a 0.2 overhang).
            if let Some(seg) = find_through_segment(point, axis, END_MID_TOLERANCE) {
                p.role = JunctionRole::Through(seg);
            } else if let Some(seg) = closest_segment_index(point, axis) {
                p.role = JunctionRole::Through(seg);
            }
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

fn closest_segment_index(point: DVec3, axis: &[DVec3]) -> Option<usize> {
    if axis.len() < 2 {
        return None;
    }
    let mut best = None;
    let mut best_d = f64::INFINITY;
    for i in 0..axis.len() - 1 {
        let a = axis[i];
        let b = axis[i + 1];
        let abx = b.x - a.x;
        let aby = b.y - a.y;
        let len2 = abx * abx + aby * aby;
        if len2 < 1e-24 {
            continue;
        }
        let t = ((point.x - a.x) * abx + (point.y - a.y) * aby) / len2;
        let t = t.clamp(0.0, 1.0);
        let proj = DVec3::new(a.x + abx * t, a.y + aby * t, a.z);
        let d = proj.distance(point);
        if d < best_d {
            best_d = d;
            best = Some(i);
        }
    }
    best
}

/// True when `point` lies on `axis` such that both endpoints remain at least
/// `min_overhang` away — the Kopfwand of a T continues past the stem.
fn axis_overhangs_both_sides(axis: &[DVec3], point: DVec3, min_overhang: f64) -> bool {
    if axis.len() < 2 {
        return false;
    }
    let start = axis[0];
    let end = *axis.last().unwrap();
    if start.distance(point) <= min_overhang || end.distance(point) <= min_overhang {
        return false;
    }
    // Closest point on any segment must be interior, and the leftovers to
    // both finite ends must exceed `min_overhang`.
    let mut best_d = f64::INFINITY;
    let mut best_on = false;
    for i in 0..axis.len() - 1 {
        let a = axis[i];
        let b = axis[i + 1];
        let abx = b.x - a.x;
        let aby = b.y - a.y;
        let len2 = abx * abx + aby * aby;
        if len2 < 1e-24 {
            continue;
        }
        let t = ((point.x - a.x) * abx + (point.y - a.y) * aby) / len2;
        let t_clamped = t.clamp(0.0, 1.0);
        let proj = DVec3::new(a.x + abx * t_clamped, a.y + aby * t_clamped, a.z);
        let d = proj.distance(point);
        if d < best_d {
            best_d = d;
            best_on = t > 0.0 && t < 1.0;
        }
    }
    best_on && best_d <= min_overhang.max(1e-6)
}

/// Classify `point` against `axis`: Endpoint if within `tol` of a vertex end,
/// Through if it lies strictly on a segment interior. `None` when the point
/// is off the finite axis (typical of an L-extension before the ends move).
fn classify_axis_at_point(axis: &[DVec3], point: DVec3, tol: f64) -> Option<JunctionRole> {
    if axis.len() < 2 {
        return None;
    }
    if point.distance(axis[0]) <= tol {
        return Some(JunctionRole::Endpoint(0));
    }
    let last = axis.len() - 1;
    if point.distance(axis[last]) <= tol {
        return Some(JunctionRole::Endpoint(last));
    }
    find_through_segment(point, axis, tol).map(JunctionRole::Through)
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
    fn test_t_join_does_not_shorten_through_axis() {
        let stem = vec![DVec3::new(4.0, 2.0, 0.0), DVec3::new(4.0, 8.0, 0.0)];
        let through = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0)];
        let (new_stem, new_through, kind, end_stem, end_through) =
            join_wall_axes(&stem, &through).unwrap();
        assert_eq!(kind, JoinKind::T);
        assert_eq!(end_stem, Some(0));
        assert_eq!(end_through, None);
        assert_eq!(new_through, through);
        assert_eq!(new_stem[0], DVec3::new(4.0, 0.0, 0.0));
        assert_eq!(new_stem[1], DVec3::new(4.0, 8.0, 0.0));
    }

    #[test]
    fn test_near_coincident_end_is_l_not_t() {
        // Stem hits 5e-4 from the through wall's start — within END_MID_TOLERANCE.
        let stem = vec![DVec3::new(0.0005, 1.0, 0.0), DVec3::new(0.0005, 5.0, 0.0)];
        let through = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0)];
        let (_a, _b, kind, end_a, end_b) = join_wall_axes(&stem, &through).unwrap();
        assert_eq!(kind, JoinKind::L);
        assert!(end_a.is_some() && end_b.is_some());
    }

    #[test]
    fn detect_junctions_two_wall_t_marks_through_not_endpoint() {
        let through = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0)];
        let stem = vec![DVec3::new(5.0, 0.0, 0.0), DVec3::new(5.0, 10.0, 0.0)];
        let walls: Vec<&[DVec3]> = vec![&through, &stem];
        let junctions = detect_junctions(&walls, JUNCTION_TOLERANCE);
        assert_eq!(junctions.len(), 1);
        assert!(!junctions[0].is_multi_wall());
        let roles: Vec<_> = junctions[0]
            .participants
            .iter()
            .map(|p| (p.wall_index, p.role))
            .collect();
        assert!(
            roles
                .iter()
                .any(|(wi, r)| *wi == 0 && matches!(r, JunctionRole::Through(_))),
            "through wall must stay Through, got {roles:?}"
        );
        assert!(
            roles
                .iter()
                .any(|(wi, r)| *wi == 1 && matches!(r, JunctionRole::Endpoint(_))),
            "stem must be Endpoint, got {roles:?}"
        );
        let snapped = apply_junction_to_axes(&walls, &junctions[0]);
        assert_eq!(snapped[0], through, "T must not shorten the through axis");
    }

    #[test]
    fn detect_junctions_near_end_overhang_stays_t() {
        // Kopfwand continues 0.2 past the stem — inside WALL_JOIN_SNAP_RADIUS
        // (0.3) but still a T because it overhangs both sides.
        let through = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(5.2, 0.0, 0.0)];
        let stem = vec![DVec3::new(5.0, 0.0, 0.0), DVec3::new(5.0, 4.0, 0.0)];
        let walls: Vec<&[DVec3]> = vec![&through, &stem];
        let junctions = detect_junctions(&walls, 0.3);
        assert_eq!(junctions.len(), 1);
        let roles: Vec<_> = junctions[0]
            .participants
            .iter()
            .map(|p| (p.wall_index, p.role))
            .collect();
        assert!(
            roles
                .iter()
                .any(|(wi, r)| *wi == 0 && matches!(r, JunctionRole::Through(_))),
            "head wall must stay Through, got {roles:?}"
        );
        let snapped = apply_junction_to_axes(&walls, &junctions[0]);
        assert_eq!(snapped[0], through);
    }

    #[test]
    fn join_wall_axes_overhang_is_t_not_l() {
        let through = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(5.2, 0.0, 0.0)];
        let stem = vec![DVec3::new(5.0, 1.0, 0.0), DVec3::new(5.0, 4.0, 0.0)];
        let (new_through, new_stem, kind, end_through, end_stem) =
            join_wall_axes(&through, &stem).unwrap();
        assert_eq!(kind, JoinKind::T);
        assert_eq!(end_through, None);
        assert_eq!(end_stem, Some(0));
        assert_eq!(new_through, through);
        assert_eq!(new_stem[0], DVec3::new(5.0, 0.0, 0.0));
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

    #[test]
    fn join_wall_axes_as_l_trims_overhang_to_corner() {
        let through = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0)];
        let stem = vec![DVec3::new(5.0, 1.0, 0.0), DVec3::new(5.0, 4.0, 0.0)];
        let (new_a, new_b, kind, end_a, end_b) = join_wall_axes_as_l(&through, &stem).unwrap();
        assert_eq!(kind, JoinKind::L);
        assert_eq!(end_a, Some(1));
        assert_eq!(end_b, Some(0));
        assert_eq!(new_a[1], DVec3::new(5.0, 0.0, 0.0));
        assert_eq!(new_b[0], DVec3::new(5.0, 0.0, 0.0));
        assert_eq!(new_a[0], DVec3::new(0.0, 0.0, 0.0));
    }

    #[test]
    fn junction_override_serde_roundtrip() {
        let ov = JunctionOverride {
            default_style: Some(JoinOverrideStyle::Miter),
            layer_pairs: vec![
                LayerPairOverride {
                    layer_a: LayerRef {
                        material_id: "masonry".to_string(),
                        role_tag: Some("Tragschale".to_string()),
                        index: 0,
                    },
                    layer_b: Some(LayerRef {
                        material_id: "insulation".to_string(),
                        role_tag: None,
                        index: 1,
                    }),
                    style: JoinOverrideStyle::Butt,
                },
                LayerPairOverride {
                    layer_a: LayerRef {
                        material_id: "plaster".to_string(),
                        role_tag: Some("Innenputz".to_string()),
                        index: 2,
                    },
                    layer_b: None,
                    style: JoinOverrideStyle::OuterFace,
                },
            ],
        };
        let json = serde_json::to_string(&ov).unwrap();
        let back: JunctionOverride = serde_json::from_str(&json).unwrap();
        assert_eq!(ov, back);
    }

    #[test]
    fn extend_axis_to_other_does_not_change_target() {
        let source = vec![DVec3::new(5.0, 2.0, 0.0), DVec3::new(5.0, 6.0, 0.0)];
        let target = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0)];
        let (new_source, idx, isect) = extend_axis_to_other(&source, &target).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(isect, DVec3::new(5.0, 0.0, 0.0));
        assert_eq!(new_source[0], isect);
        assert_eq!(new_source[1], source[1]);
    }
}
