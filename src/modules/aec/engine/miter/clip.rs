use super::*;

pub(crate) fn clip_closed_to_halfplane(
    poly: &[(f64, f64)],
    a: (f64, f64),
    b: (f64, f64),
    keep: (f64, f64),
) -> Vec<(f64, f64)> {
    let side = |p: (f64, f64)| (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
    let keep_s = side(keep);
    if keep_s.abs() < 1e-14 {
        return poly.to_vec();
    }
    let inside = |p: (f64, f64)| side(p) * keep_s >= -1e-12;
    let n = poly.len();
    if n < 3 {
        return poly.to_vec();
    }
    let mut out = Vec::with_capacity(n + 2);
    for i in 0..n {
        let cur = poly[i];
        let nxt = poly[(i + 1) % n];
        let cin = inside(cur);
        let nin = inside(nxt);
        if cin {
            out.push(cur);
        }
        if cin != nin {
            if let Some(hit) = intersect_lines_2d(cur, nxt, a, b) {
                if out.last().is_none_or(|p| dist(*p, hit) > 1e-12) {
                    out.push(hit);
                }
            }
        }
    }
    if let (Some(&first), Some(&last)) = (out.first(), out.last()) {
        if dist(first, last) < 1e-12 {
            out.pop();
        }
    }
    out
}

/// Intersect `subject` with convex CCW `clip` (Sutherland–Hodgman).
pub(super) fn clip_subject_to_convex_clip(
    subject: &[(f64, f64)],
    clip: &[(f64, f64)],
) -> Vec<(f64, f64)> {
    let mut clip_poly = clip.to_vec();
    ensure_ccw(&mut clip_poly);
    if subject.len() < 3 || clip_poly.len() < 3 {
        return subject.to_vec();
    }
    let mut output = subject.to_vec();
    let n = clip_poly.len();
    for i in 0..n {
        let a = clip_poly[i];
        let b = clip_poly[(i + 1) % n];
        let input = std::mem::take(&mut output);
        if input.is_empty() {
            break;
        }
        let inside = |p: (f64, f64)| {
            (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0) >= -1e-12
        };
        for j in 0..input.len() {
            let cur = input[j];
            let prev = input[(j + input.len() - 1) % input.len()];
            let cin = inside(cur);
            let pin = inside(prev);
            if cin {
                if !pin {
                    if let Some(hit) = intersect_lines_2d(prev, cur, a, b) {
                        if output.last().is_none_or(|p| dist(*p, hit) > 1e-12) {
                            output.push(hit);
                        }
                    }
                }
                if output.last().is_none_or(|p| dist(*p, cur) > 1e-12) {
                    output.push(cur);
                }
            } else if pin {
                if let Some(hit) = intersect_lines_2d(prev, cur, a, b) {
                    if output.last().is_none_or(|p| dist(*p, hit) > 1e-12) {
                        output.push(hit);
                    }
                }
            }
        }
    }
    output
}

/// Keep a T-junction / manual gap cutout without discarding an L-miter
/// already computed for the same layer (square cutout caps at the wall
/// ends would otherwise replace the diagonal).
///
/// Each cutout ring is bounded by **every** neighbouring notch edge, not
/// just the closest one: a middle ring of a 3-ring (two crossings) has two
/// notches and must be clipped on both sides or it runs through the second
/// through-wall core. Outer square caps are *not* used as clip edges, so an
/// L-miter beyond the axis end survives on the end rings.
pub fn merge_through_cutout_preserving_miter(
    mitered: Option<&[(f64, f64)]>,
    cutout: &[(f64, f64)],
) -> Vec<(f64, f64)> {
    let Some(m) = mitered else {
        return cutout.to_vec();
    };
    let m_rings = split_footprint_rings(m);
    if m_rings.is_empty() {
        return cutout.to_vec();
    }
    let rings = split_footprint_rings(cutout);
    if rings.is_empty() {
        return m.to_vec();
    }
    let mut out = Vec::new();
    for (ri, ring) in rings.iter().enumerate() {
        if ring.len() < 3 {
            continue;
        }
        let n = ring.len() as f64;
        let keep = {
            let s = ring.iter().fold((0.0, 0.0), |acc, p| (acc.0 + p.0, acc.1 + p.1));
            (s.0 / n, s.1 / n)
        };
        // One notch edge per neighbouring ring so a middle span is bounded
        // on both sides. Duplicate edges (same neighbour pair) are skipped.
        let mut notches: Vec<((f64, f64), (f64, f64))> = Vec::new();
        for (rj, other) in rings.iter().enumerate() {
            if rj == ri {
                continue;
            }
            let mut best: Option<((f64, f64), (f64, f64), f64)> = None;
            for i in 0..ring.len() {
                let a = ring[i];
                let b = ring[(i + 1) % ring.len()];
                let mid = ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
                let d = other
                    .iter()
                    .map(|p| dist(*p, mid))
                    .fold(f64::INFINITY, f64::min);
                if best.is_none_or(|(_, _, bd)| d < bd) {
                    best = Some((a, b, d));
                }
            }
            if let Some((a, b, _)) = best {
                let dup = notches.iter().any(|&(ea, eb)| {
                    (dist(ea, a) < 1e-12 && dist(eb, b) < 1e-12)
                        || (dist(ea, b) < 1e-12 && dist(eb, a) < 1e-12)
                });
                if !dup {
                    notches.push((a, b));
                }
            }
        }
        for m_ring in &m_rings {
            if m_ring.len() < 3 {
                continue;
            }
            let clipped = if notches.is_empty() {
                clip_subject_to_convex_clip(m_ring, ring)
            } else {
                let mut cur = m_ring.clone();
                for &(ea, eb) in &notches {
                    if cur.len() < 3 {
                        break;
                    }
                    cur = clip_closed_to_halfplane(&cur, ea, eb, keep);
                }
                cur
            };
            if clipped.len() >= 3 {
                if !out.is_empty() {
                    out.push((f64::NAN, f64::NAN));
                }
                out.extend(clipped);
            }
        }
    }
    if out.is_empty() {
        cutout.to_vec()
    } else {
        out
    }
}

/// Split a simple ring along the finite chord `a`–`b` (extended by a few
/// micrometres so chord ends that sit exactly on the ring boundary still
/// cross it). Only boundary crossings *inside* the chord extent count, so
/// unrelated parts of a polyline footprint that happen to lie on the chord's
/// supporting line are left alone. Returns the resulting simple rings (the
/// input ring when the chord does not cut it).
pub(crate) fn split_ring_along_chord(
    ring: &[(f64, f64)],
    a: (f64, f64),
    b: (f64, f64),
) -> Vec<Vec<(f64, f64)>> {
    split_ring_along_chord_depth(ring, a, b, 0)
}

pub(super) fn split_ring_along_chord_depth(
    ring: &[(f64, f64)],
    a: (f64, f64),
    b: (f64, f64),
    depth: usize,
) -> Vec<Vec<(f64, f64)>> {
    let n = ring.len();
    let len = dist(a, b);
    if n < 3 || depth > 16 || len < 1e-9 {
        return vec![ring.to_vec()];
    }
    let u = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    let nrm = (-u.1, u.0);
    let side = |p: (f64, f64)| (p.0 - a.0) * nrm.0 + (p.1 - a.1) * nrm.1;
    let along = |p: (f64, f64)| (p.0 - a.0) * u.0 + (p.1 - a.1) * u.1;
    let (t_lo, t_hi) = (-1e-5, len + 1e-5);
    let eps = 1e-7;
    let sg: Vec<i8> = ring
        .iter()
        .map(|&p| {
            let s = side(p);
            if s > eps {
                1
            } else if s < -eps {
                -1
            } else {
                0
            }
        })
        .collect();

    let mut aug: Vec<(f64, f64)> = Vec::with_capacity(n + 4);
    let mut marks: Vec<(usize, f64)> = Vec::new();
    for i in 0..n {
        let j = (i + 1) % n;
        aug.push(ring[i]);
        if sg[i] == 0 {
            let prev = sg[(i + n - 1) % n];
            let next = sg[j];
            if prev != 0 && next != 0 && prev * next < 0 {
                let t = along(ring[i]);
                if (t_lo..=t_hi).contains(&t) {
                    marks.push((aug.len() - 1, t));
                }
            }
        } else if sg[j] != 0 && sg[i] * sg[j] < 0 {
            let s0 = side(ring[i]);
            let s1 = side(ring[j]);
            let f = s0 / (s0 - s1);
            let q = (
                ring[i].0 + (ring[j].0 - ring[i].0) * f,
                ring[i].1 + (ring[j].1 - ring[i].1) * f,
            );
            let t = along(q);
            if (t_lo..=t_hi).contains(&t) {
                aug.push(q);
                marks.push((aug.len() - 1, t));
            }
        }
    }
    if marks.len() < 2 || marks.len() % 2 != 0 {
        return vec![ring.to_vec()];
    }
    marks.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap_or(std::cmp::Ordering::Equal));
    let (i0, i1) = (marks[0].0.min(marks[1].0), marks[0].0.max(marks[1].0));
    if i0 == i1 {
        return vec![ring.to_vec()];
    }
    let poly1: Vec<(f64, f64)> = aug[i0..=i1].to_vec();
    let poly2: Vec<(f64, f64)> = aug[i1..].iter().chain(aug[..=i0].iter()).copied().collect();
    let mut out = Vec::new();
    for poly in [poly1, poly2] {
        if poly.len() < 3 || super::super::geometry::area(&poly) < 1e-9 {
            continue;
        }
        out.extend(split_ring_along_chord_depth(&poly, a, b, depth + 1));
    }
    if out.is_empty() {
        vec![ring.to_vec()]
    } else {
        out
    }
}

/// Merge the two butt-jointed stem rings of an X crossing (as produced by the
/// junction solver) into the crossing wall's already-built layer footprint.
///
/// Unlike [`merge_through_cutout_preserving_miter`] this never uses infinite
/// half-planes: the footprint is **cut along the two finite butt chords** of the
/// stems and only the piece(s) lying wholly inside the through-wall band
/// between the chords are removed. A polyline crossing wall (U / zig-zag)
/// that crosses the same or another wall again therefore keeps every leg:
/// legs only lose the band slice at this crossing. Falls back to the legacy
/// merge when the cutout is not a clean pair of butting stems.
pub fn merge_crossing_stems_local(
    mitered: Option<&[(f64, f64)]>,
    cutout: &[(f64, f64)],
) -> Vec<(f64, f64)> {
    let Some(m) = mitered else {
        return cutout.to_vec();
    };
    let m_rings = split_footprint_rings(m);
    let rings = split_footprint_rings(cutout);
    if m_rings.is_empty() {
        return cutout.to_vec();
    }
    if rings.len() != 2 {
        return merge_through_cutout_preserving_miter(mitered, cutout);
    }
    // Butt edge: the ring edge nearest the other ring; the ring body lies on
    // the side its neighbouring vertices are on.
    let butt = |ring: &[(f64, f64)], other: &[(f64, f64)]| -> Option<((f64, f64), (f64, f64), f64)> {
        let k = ring.len();
        let mut best: Option<(usize, f64)> = None;
        for i in 0..k {
            let (p, q) = (ring[i], ring[(i + 1) % k]);
            let mid = ((p.0 + q.0) * 0.5, (p.1 + q.1) * 0.5);
            let d = other.iter().map(|o| dist(*o, mid)).fold(f64::INFINITY, f64::min);
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        let (i, _) = best?;
        let (p, q) = (ring[i], ring[(i + 1) % k]);
        let l = dist(p, q);
        if l < 1e-9 {
            return None;
        }
        let nrm = (-(q.1 - p.1) / l, (q.0 - p.0) / l);
        let sd = |v: (f64, f64)| (v.0 - p.0) * nrm.0 + (v.1 - p.1) * nrm.1;
        let body = sd(ring[(i + k - 1) % k]) + sd(ring[(i + 2) % k]);
        if body.abs() < 1e-9 {
            return None;
        }
        Some((p, q, body.signum()))
    };
    let (Some((a0, b0, s0)), Some((a1, b1, s1))) =
        (butt(&rings[0], &rings[1]), butt(&rings[1], &rings[0]))
    else {
        return merge_through_cutout_preserving_miter(mitered, cutout);
    };
    let sd_of = |a: (f64, f64), b: (f64, f64)| {
        let l = dist(a, b).max(1e-12);
        let nrm = (-(b.1 - a.1) / l, (b.0 - a.0) / l);
        move |v: (f64, f64)| (v.0 - a.0) * nrm.0 + (v.1 - a.1) * nrm.1
    };
    let sd0 = sd_of(a0, b0);
    let sd1 = sd_of(a1, b1);
    let mid0 = ((a0.0 + b0.0) * 0.5, (a0.1 + b0.1) * 0.5);
    let mid1 = ((a1.0 + b1.0) * 0.5, (a1.1 + b1.1) * 0.5);
    // A real gap (through-wall band) exists only when each butt edge lies
    // outside the other stem's half-plane; overlapping stems (explicit
    // FarFace / NoExtend overrides) keep the legacy behaviour.
    if sd1(mid0) * s1 >= -1e-6 || sd0(mid1) * s0 >= -1e-6 {
        return merge_through_cutout_preserving_miter(mitered, cutout);
    }
    let tol = 1e-5;
    let mut out: Vec<(f64, f64)> = Vec::new();
    for ring in &m_rings {
        let mut pieces = Vec::new();
        for piece in split_ring_along_chord(ring, a0, b0) {
            pieces.extend(split_ring_along_chord(&piece, a1, b1));
        }
        // A prior crossing may have already separated a distant leg into
        // its own ring. Never discard such a ring merely because it lies in
        // the infinite *extension* of the new notch band: only the finite
        // chords at this crossing are allowed to remove material.
        let untouched = pieces.len() == 1 && pieces[0] == *ring;
        for piece in pieces {
            if piece.len() < 3 {
                continue;
            }
            let in_band = !untouched && piece
                .iter()
                .all(|&v| sd0(v) * s0 <= tol && sd1(v) * s1 <= tol);
            if in_band {
                continue;
            }
            if !out.is_empty() {
                out.push((f64::NAN, f64::NAN));
            }
            out.extend(piece);
        }
    }
    if out.is_empty() {
        cutout.to_vec()
    } else {
        out
    }
}

/// Split a layer footprint (which may have mitered, extended, or butt-joined
/// ends) by wall openings.
///
/// For each remaining free span along the axis:
/// - If the span starts at the wall start (s0 ≈ 0), the start of the footprint
///   (including any join/miter at end 0) is preserved untouched.
///   Otherwise, it is sliced cleanly by the opening reveal perpendicular to the axis at s0.
/// - If the span ends at the wall end (s1 ≈ L), the end of the footprint
///   (including any join/miter at end L) is preserved untouched.
///   Otherwise, it is sliced cleanly by the opening reveal perpendicular to the axis at s1.
///
/// Multiple rings (e.g. from T-through cutouts) separated by NaN are supported.
pub fn split_mitered_footprint_by_openings(
    footprint: &[(f64, f64)],
    axis: &[(f64, f64)],
    openings: &[Opening],
) -> Vec<Vec<(f64, f64)>> {
    if openings.is_empty() || axis.len() < 2 || footprint.len() < 3 {
        let rings = split_footprint_rings(footprint);
        return if rings.is_empty() {
            vec![footprint.to_vec()]
        } else {
            rings
        };
    }
    let length = axis_length(axis);
    if length <= 1e-9 {
        return vec![footprint.to_vec()];
    }
    let through_openings: Vec<Opening> = openings
        .iter()
        .filter(|o| !(o.kind == super::super::openings::OpeningKind::Breakthrough && o.depth.is_some()))
        .cloned()
        .collect();
    let spans = remaining_axis_spans(length, &through_openings);
    let rings = split_footprint_rings(footprint);
    let mut pieces = Vec::new();

    for (s0, s1) in spans {
        if s1 - s0 <= 1e-6 {
            continue;
        }
        for ring in &rings {
            if ring.len() < 3 {
                continue;
            }
            let mut clipped = ring.clone();

            // 1. Cut at start of span (s0) if s0 is strictly after the wall start
            if s0 > 1e-4 {
                if let Some((p0, t0)) = point_and_tangent_at_distance(axis, s0) {
                    let n0 = (-t0.1, t0.0);
                    let a = p0;
                    let b = (p0.0 + n0.0, p0.1 + n0.1);
                    let keep = (p0.0 + t0.0, p0.1 + t0.1);
                    clipped = clip_closed_to_halfplane(&clipped, a, b, keep);
                }
            }

            // 2. Cut at end of span (s1) if s1 is strictly before the wall end
            if s1 < length - 1e-4 {
                if let Some((p1, t1)) = point_and_tangent_at_distance(axis, s1) {
                    let n1 = (-t1.1, t1.0);
                    let a = p1;
                    let b = (p1.0 + n1.0, p1.1 + n1.1);
                    let keep = (p1.0 - t1.0, p1.1 - t1.1);
                    clipped = clip_closed_to_halfplane(&clipped, a, b, keep);
                }
            }

            if clipped.len() >= 3 && super::super::geometry::area(&clipped) > 1e-6 {
                ensure_ccw(&mut clipped);
                pieces.push(clipped);
            }
        }
    }

    if pieces.is_empty() {
        rings
    } else {
        pieces
    }
}
