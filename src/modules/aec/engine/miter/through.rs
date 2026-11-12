use super::*;

/// Straight-extend stem layer boundaries to a single target face line
/// (T-junction butt join). Both end-cap vertices lie on `target_line`.
pub(super) fn t_extend_one_layer(
    a_b1: &[(f64, f64)],
    a_b2: &[(f64, f64)],
    end_a: usize,
    target_line: ((f64, f64), (f64, f64)),
) -> Option<Vec<(f64, f64)>> {
    let (new_a1, new_a2) = t_extend_end_points(a_b1, a_b2, end_a, target_line)?;
    Some(rebuild_footprint(a_b1, a_b2, end_a, new_a1, new_a2))
}

pub(super) fn t_extend_end_points(
    a_b1: &[(f64, f64)],
    a_b2: &[(f64, f64)],
    end_a: usize,
    target_line: ((f64, f64), (f64, f64)),
) -> Option<((f64, f64), (f64, f64))> {
    if a_b1.len() < 2 || a_b2.len() < 2 || a_b1.len() != a_b2.len() {
        return None;
    }
    if end_a >= a_b1.len() {
        return None;
    }
    let prev_a = if end_a == 0 { 1 } else { end_a - 1 };
    let a1_line = extended_line(a_b1[prev_a], a_b1[end_a]);
    let a2_line = extended_line(a_b2[prev_a], a_b2[end_a]);
    let new_a1 = intersect_lines_2d(a1_line.0, a1_line.1, target_line.0, target_line.1)?;
    let new_a2 = intersect_lines_2d(a2_line.0, a2_line.1, target_line.0, target_line.1)?;
    Some((new_a1, new_a2))
}

/// Pick the face of through-wall layer B that the stem approaches first
/// (the near face). Both stem boundary lines butt against this single line.
pub(crate) fn near_face_line(
    a_b1: &[(f64, f64)],
    a_b2: &[(f64, f64)],
    end_a: usize,
    b_b1: &[(f64, f64)],
    b_b2: &[(f64, f64)],
    end_b: Option<usize>,
    axis_a: &[(f64, f64)],
    axis_b: &[(f64, f64)],
) -> Option<((f64, f64), (f64, f64))> {
    layer_face_line(a_b1, a_b2, end_a, b_b1, b_b2, end_b, axis_a, axis_b, false)
}

pub(super) fn layer_face_line(
    _a_b1: &[(f64, f64)],
    _a_b2: &[(f64, f64)],
    end_a: usize,
    b_b1: &[(f64, f64)],
    b_b2: &[(f64, f64)],
    end_b: Option<usize>,
    axis_a: &[(f64, f64)],
    axis_b: &[(f64, f64)],
    far: bool,
) -> Option<((f64, f64), (f64, f64))> {
    let (b1_line, b2_line) = other_boundary_lines(b_b1, b_b2, end_b, axis_a, axis_b, end_a)?;
    let join_pt = *axis_a.get(end_a)?;
    let prev_a = if end_a == 0 { 1 } else { end_a - 1 };
    let stem_interior = *axis_a.get(prev_a)?;
    // Direction from the joint toward the stem body — the near face is the
    // boundary whose closest point to the joint lies further along this dir.
    let to_stem = (
        stem_interior.0 - join_pt.0,
        stem_interior.1 - join_pt.1,
    );
    let s1 = closest_point_on_line(join_pt, b1_line);
    let s2 = closest_point_on_line(join_pt, b2_line);
    let d1 = (s1.0 - join_pt.0) * to_stem.0 + (s1.1 - join_pt.1) * to_stem.1;
    let d2 = (s2.0 - join_pt.0) * to_stem.0 + (s2.1 - join_pt.1) * to_stem.1;
    let near_is_b1 = d1 >= d2;
    if far ^ near_is_b1 {
        Some(b1_line)
    } else {
        Some(b2_line)
    }
}

/// Outer face of the entire through-wall stack on the stem's approach side.
/// Used when a stem layer has no material match in the through wall.
pub(crate) fn through_outer_near_face_line(
    axis_a: &[(f64, f64)],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    contours_b: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
) -> Option<((f64, f64), (f64, f64))> {
    through_outer_face_line(axis_a, end_a, axis_b, layers_b, contours_b, false)
}

pub(super) fn through_outer_face_line(
    axis_a: &[(f64, f64)],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    contours_b: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    far: bool,
) -> Option<((f64, f64), (f64, f64))> {
    if contours_b.is_empty() || axis_a.len() < 2 || end_a >= axis_a.len() {
        return None;
    }
    let mut min_i = 0usize;
    let mut max_i = 0usize;
    let mut min_off = f64::INFINITY;
    let mut max_off = f64::NEG_INFINITY;
    if layers_b.len() == contours_b.len() && !layers_b.is_empty() {
        for (i, l) in layers_b.iter().enumerate() {
            let start = l.axis_offset;
            let end = l.axis_offset + l.thickness;
            if start < min_off {
                min_off = start;
                min_i = i;
            }
            if end > max_off {
                max_off = end;
                max_i = i;
            }
        }
    } else {
        min_i = 0;
        max_i = contours_b.len() - 1;
    }
    let (ref first_b1, _) = contours_b[min_i];
    let (_, ref last_b2) = contours_b[max_i];
    layer_face_line(
        &[],
        &[],
        end_a,
        first_b1,
        last_b2,
        None,
        axis_a,
        axis_b,
        far,
    )
}

pub(super) fn contours_xy(
    axis: &[(f64, f64)],
    bulges: &[f64],
    geom: &[(f64, f64)],
) -> Vec<(Vec<(f64, f64)>, Vec<(f64, f64)>)> {
    if bulges.iter().any(|b| b.abs() > 1e-12) {
        layer_contours_with_bulges(axis, bulges, geom)
            .into_iter()
            .map(|(a, b)| (a.points, b.points))
            .collect()
    } else {
        layer_contours(axis, geom)
    }
}

/// Through-wall footprints at a T-junction.
///
/// Approach-side through layers overlapped by the stem **core** receive a
/// C-notch; the through core (and far-side finish) stay rectangular (`None`).
pub fn through_wall_cutout_footprints(
    axis_through: &[(f64, f64)],
    layers_through: &[MiterLayer],
    axis_stem: &[(f64, f64)],
    layers_stem: &[MiterLayer],
    stem_end: usize,
) -> Vec<Option<Vec<(f64, f64)>>> {
    through_wall_cutout_footprints_with_bulges(
        axis_through,
        layers_through,
        axis_stem,
        layers_stem,
        stem_end,
        &[],
        &[],
    )
}

pub fn through_wall_cutout_footprints_with_bulges(
    axis_through: &[(f64, f64)],
    layers_through: &[MiterLayer],
    axis_stem: &[(f64, f64)],
    layers_stem: &[MiterLayer],
    stem_end: usize,
    bulges_through: &[f64],
    bulges_stem: &[f64],
) -> Vec<Option<Vec<(f64, f64)>>> {
    through_wall_cutout_footprints_with_gaps(
        axis_through,
        layers_through,
        axis_stem,
        layers_stem,
        stem_end,
        bulges_through,
        bulges_stem,
        &[],
    )
}

/// Like [`through_wall_cutout_footprints_with_bulges`], then applies manual
/// [`LayerGapOverride`]s (user-defined interruptions between two stem layers).
pub fn through_wall_cutout_footprints_with_gaps(
    axis_through: &[(f64, f64)],
    layers_through: &[MiterLayer],
    axis_stem: &[(f64, f64)],
    layers_stem: &[MiterLayer],
    stem_end: usize,
    bulges_through: &[f64],
    bulges_stem: &[f64],
    gaps: &[LayerGapOverride],
) -> Vec<Option<Vec<(f64, f64)>>> {
    if axis_through.len() < 2 || axis_stem.len() < 2 || layers_through.is_empty() {
        return vec![None; layers_through.len()];
    }
    if stem_end >= axis_stem.len() {
        return vec![None; layers_through.len()];
    }
    let geom_t: Vec<(f64, f64)> = layers_through.iter().map(MiterLayer::as_geom).collect();
    let geom_s: Vec<(f64, f64)> = layers_stem.iter().map(MiterLayer::as_geom).collect();
    let contours_t = contours_xy(axis_through, bulges_through, &geom_t);
    let contours_s = contours_xy(axis_stem, bulges_stem, &geom_s);
    let mut out = vec![None; layers_through.len()];
    if let Some((core_s, core_t)) = pair_structural_cores(layers_stem, layers_through) {
        if core_t < contours_t.len() {
            let (ref core_t1, ref core_t2) = contours_t[core_t];
            let toward = through_offset_toward_stem(axis_stem, stem_end, axis_through);
            let punchers =
                punchers_outside_in(layers_stem, layers_through, Some((core_s, core_t)), toward);
            for ti in 0..layers_through.len() {
                if ti == core_t || ti >= contours_t.len() {
                    continue;
                }
                if is_structural_function(&layers_through[ti].function) {
                    continue;
                }
                let (ref t1, ref t2) = contours_t[ti];
                if !through_layer_on_approach_side(t1, t2, core_t1, core_t2, axis_stem, stem_end)
                {
                    continue;
                }
                let overlapping: Vec<usize> = punchers
                    .get(ti)
                    .map(|v| {
                        v.iter()
                            .copied()
                            .filter(|&si| si < contours_s.len())
                            .collect()
                    })
                    .unwrap_or_default();
                if overlapping.is_empty() {
                    continue;
                }
                if let Some(fp) = notch_through_layer_from_stem_layers(
                    t1,
                    t2,
                    &layers_through[ti],
                    &overlapping,
                    layers_stem,
                    &contours_s,
                    stem_end,
                    axis_stem,
                ) {
                    out[ti] = Some(fp);
                }
            }
        }
    }
    apply_manual_layer_gaps(
        &mut out,
        gaps,
        layers_through,
        layers_stem,
        &contours_t,
        &contours_s,
        stem_end,
        axis_stem,
    );
    out
}

pub(super) fn miter_layer_index(layers: &[MiterLayer], r: &LayerRef) -> Option<usize> {
    if let Some(id) = r.layer_id {
        if !id.is_nil() {
            if let Some(i) = layers.iter().position(|l| l.layer_id == id) {
                return Some(i);
            }
        }
    }
    layers.iter().enumerate().find_map(|(i, l)| {
        (l.material.eq_ignore_ascii_case(&r.material_id) && i == r.index).then_some(i)
    }).or_else(|| {
        layers
            .iter()
            .position(|l| l.material.eq_ignore_ascii_case(&r.material_id))
    })
}

pub(super) fn apply_manual_layer_gaps(
    out: &mut [Option<Vec<(f64, f64)>>],
    gaps: &[LayerGapOverride],
    layers_through: &[MiterLayer],
    layers_stem: &[MiterLayer],
    contours_t: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    contours_s: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    stem_end: usize,
    axis_stem: &[(f64, f64)],
) {
    for gap in gaps {
        let Some(ti) = miter_layer_index(layers_through, &gap.layer) else {
            continue;
        };
        let Some(si0) = miter_layer_index(layers_stem, &gap.from) else {
            continue;
        };
        let Some(si1) = miter_layer_index(layers_stem, &gap.to) else {
            continue;
        };
        if ti >= contours_t.len() {
            continue;
        }
        let lo = si0.min(si1);
        let hi = si0.max(si1);
        let overlapping: Vec<usize> = (lo..=hi).filter(|&si| si < contours_s.len()).collect();
        if overlapping.is_empty() {
            continue;
        }
        let (ref t1, ref t2) = contours_t[ti];
        if let Some(fp) = notch_through_layer_from_stem_layers(
            t1,
            t2,
            &layers_through[ti],
            &overlapping,
            layers_stem,
            contours_s,
            stem_end,
            axis_stem,
        ) {
            out[ti] = Some(fp);
        }
    }
}

/// Leftmost and rightmost stem boundary polylines (full stem envelope).
#[allow(dead_code)]
pub(super) fn stem_outer_boundaries(
    contours_s: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
) -> Option<(&[ (f64, f64) ], &[ (f64, f64) ])> {
    let first = contours_s.first()?;
    let last = contours_s.last()?;
    Some((first.0.as_slice(), last.1.as_slice()))
}

pub(super) fn closest_layer_index(a: &MiterLayer, layers_b: &[MiterLayer]) -> Option<usize> {
    if layers_b.is_empty() {
        return None;
    }
    let ca = a.axis_offset + a.thickness * 0.5;
    let mut best = 0usize;
    let mut best_d = f64::INFINITY;
    for (i, b) in layers_b.iter().enumerate() {
        let cb = b.axis_offset + b.thickness * 0.5;
        let d = (ca - cb).abs();
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    Some(best)
}

pub(super) fn contour_centroid(a: &[(f64, f64)], b: &[(f64, f64)]) -> (f64, f64) {
    let mut sx = 0.0;
    let mut sy = 0.0;
    let mut n = 0.0;
    for p in a.iter().chain(b.iter()) {
        sx += p.0;
        sy += p.1;
        n += 1.0;
    }
    if n < 1.0 {
        (0.0, 0.0)
    } else {
        (sx / n, sy / n)
    }
}

pub(super) fn stem_to_body(axis_stem: &[(f64, f64)], stem_end: usize) -> Option<(f64, f64)> {
    if stem_end >= axis_stem.len() || axis_stem.len() < 2 {
        return None;
    }
    let prev = if stem_end == 0 { 1 } else { stem_end - 1 };
    let join = axis_stem[stem_end];
    let interior = axis_stem[prev];
    Some((interior.0 - join.0, interior.1 - join.1))
}

pub(super) fn through_layer_on_approach_side(
    t1: &[(f64, f64)],
    t2: &[(f64, f64)],
    core1: &[(f64, f64)],
    core2: &[(f64, f64)],
    axis_stem: &[(f64, f64)],
    stem_end: usize,
) -> bool {
    let Some(to_stem) = stem_to_body(axis_stem, stem_end) else {
        return false;
    };
    let layer = contour_centroid(t1, t2);
    let core = contour_centroid(core1, core2);
    (layer.0 - core.0) * to_stem.0 + (layer.1 - core.1) * to_stem.1 > 1e-9
}

pub(super) fn line_param(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-24 {
        0.0
    } else {
        ((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2
    }
}

/// Pair stem vs through from the approach outer face inward:
/// same material → miter at this through layer (consumed, does not punch deeper);
/// different material → continue inward if a same-material through layer (or the
/// core) still exists, otherwise butt here.
pub(super) fn punchers_outside_in(
    layers_stem: &[MiterLayer],
    layers_through: &[MiterLayer],
    core_pair: Option<(usize, usize)>,
    toward_stem: f64,
) -> Vec<Vec<usize>> {
    let n_t = layers_through.len();
    let mut out = vec![Vec::new(); n_t];
    let core_t = core_pair.map(|(_, t)| t);
    let core_s = core_pair.map(|(s, _)| s);
    let mut order: Vec<usize> = (0..n_t).collect();
    order.sort_by(|&a, &b| {
        let da = layer_outerness(&layers_through[a], toward_stem);
        let db = layer_outerness(&layers_through[b], toward_stem);
        db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut consumed = vec![false; layers_stem.len()];
    for ti in order {
        if Some(ti) == core_t || is_structural_function(&layers_through[ti].function) {
            continue;
        }
        let thru = &layers_through[ti];
        let mut punch = Vec::new();
        for si in 0..layers_stem.len() {
            if consumed[si] {
                continue;
            }
            let stem = &layers_stem[si];
            let is_stem_structural = core_s == Some(si) || is_structural_function(&stem.function);
            let same_mat = !stem.material.is_empty()
                && stem.material.eq_ignore_ascii_case(&thru.material);
            if same_mat && !is_stem_structural {
                punch.push(si);
                consumed[si] = true;
                continue;
            }
            let deeper_same = layers_through.iter().enumerate().any(|(j, l)| {
                if j == ti || Some(j) == core_t || is_structural_function(&l.function) {
                    return false;
                }
                if layer_outerness(l, toward_stem) >= layer_outerness(thru, toward_stem) - 1e-12 {
                    return false;
                }
                !stem.material.is_empty() && stem.material.eq_ignore_ascii_case(&l.material)
            });
            if is_stem_structural || deeper_same {
                punch.push(si);
            } else {
                consumed[si] = true;
            }
        }
        out[ti] = punch;
    }
    out
}

pub(super) fn layer_outerness(layer: &MiterLayer, toward_stem: f64) -> f64 {
    if toward_stem >= 0.0 {
        layer.axis_offset + layer.thickness
    } else {
        -layer.axis_offset
    }
}

pub(super) fn notch_through_layer_from_stem_layers(
    t_b1: &[(f64, f64)],
    t_b2: &[(f64, f64)],
    thru: &MiterLayer,
    overlapping: &[usize],
    layers_stem: &[MiterLayer],
    contours_s: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    stem_end: usize,
    axis_stem: &[(f64, f64)],
) -> Option<Vec<(f64, f64)>> {
    if overlapping.is_empty() {
        return None;
    }
    let Some(to_stem) = stem_to_body(axis_stem, stem_end) else {
        return None;
    };
    let join = *axis_stem.get(stem_end)?;
    let t1_line = (*t_b1.first()?, *t_b1.last()?);
    let t2_line = (*t_b2.first()?, *t_b2.last()?);
    let s1 = closest_point_on_line(join, t1_line);
    let s2 = closest_point_on_line(join, t2_line);
    let d1 = (s1.0 - join.0) * to_stem.0 + (s1.1 - join.1) * to_stem.1;
    let d2 = (s2.0 - join.0) * to_stem.0 + (s2.1 - join.1) * to_stem.1;
    let (near, far) = if d1 >= d2 {
        (t_b1, t_b2)
    } else {
        (t_b2, t_b1)
    };
    let near_line = (*near.first()?, *near.last()?);
    let far_line = (*far.first()?, *far.last()?);

    let mut hits: Vec<(f64, usize, bool)> = Vec::new();
    for &si in overlapping {
        let (ref b1, ref b2) = contours_s[si];
        if stem_end >= b1.len() || stem_end >= b2.len() {
            continue;
        }
        let prev = if stem_end == 0 { 1 } else { stem_end - 1 };
        if prev >= b1.len() {
            continue;
        }
        for (is_b2, b) in [(false, b1), (true, b2)] {
            let line = extended_line(b[prev], b[stem_end]);
            if let Some(p) = intersect_lines_2d(near_line.0, near_line.1, line.0, line.1) {
                hits.push((line_param(p, near_line.0, near_line.1), si, is_b2));
            }
        }
    }
    if hits.len() < 2 {
        let (ref s1c, ref s2c) = contours_s[overlapping[0]];
        return notch_through_layer(t_b1, t_b2, s1c, s2c, stem_end, axis_stem);
    }
    hits.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let left = hits[0];
    let right = hits[hits.len() - 1];
    let (n0, f0) = through_cut_corners(
        thru,
        layers_stem,
        contours_s,
        left.1,
        left.2,
        near_line,
        far_line,
        stem_end,
        axis_stem,
    )?;
    let (n1, f1) = through_cut_corners(
        thru,
        layers_stem,
        contours_s,
        right.1,
        right.2,
        near_line,
        far_line,
        stem_end,
        axis_stem,
    )?;
    let mut n0 = n0;
    let mut n1 = n1;
    let mut f0 = f0;
    let mut f1 = f1;
    if line_param(n1, near_line.0, near_line.1) < line_param(n0, near_line.0, near_line.1) {
        std::mem::swap(&mut n0, &mut n1);
        std::mem::swap(&mut f0, &mut f1);
    }
    let mut left_ring = vec![near_line.0, n0, f0, far_line.0];
    let mut right_ring = vec![n1, near_line.1, far_line.1, f1];
    ensure_ccw(&mut left_ring);
    ensure_ccw(&mut right_ring);
    if left_ring.len() < 3 || right_ring.len() < 3 {
        return None;
    }
    let mut fp = left_ring;
    fp.push((f64::NAN, f64::NAN));
    fp.extend(right_ring);
    Some(fp)
}

pub(super) fn through_cut_corners(
    thru: &MiterLayer,
    layers_stem: &[MiterLayer],
    contours_s: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    stem_i: usize,
    use_b2: bool,
    near_line: ((f64, f64), (f64, f64)),
    far_line: ((f64, f64), (f64, f64)),
    stem_end: usize,
    axis_stem: &[(f64, f64)],
) -> Option<((f64, f64), (f64, f64))> {
    let stem = layers_stem.get(stem_i)?;
    let (ref b1, ref b2) = contours_s.get(stem_i)?;
    if stem_end >= b1.len() || stem_end >= b2.len() {
        return None;
    }
    let prev = if stem_end == 0 { 1 } else { stem_end - 1 };
    if prev >= b1.len() {
        return None;
    }
    let l1 = extended_line(b1[prev], b1[stem_end]);
    let l2 = extended_line(b2[prev], b2[stem_end]);
    let miter = !stem.material.is_empty()
        && stem.material.eq_ignore_ascii_case(&thru.material)
        && !is_structural_function(&stem.function);
    if miter {
        let d1 = point_axis_offset(b1[stem_end], axis_stem);
        let d2 = point_axis_offset(b2[stem_end], axis_stem);
        let (inner, outer) = if d1 <= d2 { (l1, l2) } else { (l2, l1) };
        let n = intersect_lines_2d(near_line.0, near_line.1, outer.0, outer.1)?;
        let f = intersect_lines_2d(far_line.0, far_line.1, inner.0, inner.1)?;
        Some((n, f))
    } else {
        let line = if use_b2 { l2 } else { l1 };
        let n = intersect_lines_2d(near_line.0, near_line.1, line.0, line.1)?;
        let f = intersect_lines_2d(far_line.0, far_line.1, line.0, line.1)?;
        Some((n, f))
    }
}

pub(super) fn notch_through_layer(
    t_b1: &[(f64, f64)],
    t_b2: &[(f64, f64)],
    s_b1: &[(f64, f64)],
    s_b2: &[(f64, f64)],
    stem_end: usize,
    axis_stem: &[(f64, f64)],
) -> Option<Vec<(f64, f64)>> {
    if t_b1.len() < 2 || t_b2.len() < 2 || s_b1.len() < 2 || s_b2.len() < 2 {
        return None;
    }
    if stem_end >= s_b1.len() || stem_end >= s_b2.len() {
        return None;
    }
    let prev = if stem_end == 0 { 1 } else { stem_end - 1 };
    if prev >= s_b1.len() || prev >= axis_stem.len() {
        return None;
    }
    let s1_line = extended_line(s_b1[prev], s_b1[stem_end]);
    let s2_line = extended_line(s_b2[prev], s_b2[stem_end]);
    let t1_line = (*t_b1.first()?, *t_b1.last()?);
    let t2_line = (*t_b2.first()?, *t_b2.last()?);
    let Some(to_stem) = stem_to_body(axis_stem, stem_end) else {
        return None;
    };
    let join = axis_stem[stem_end];
    let s1 = closest_point_on_line(join, t1_line);
    let s2 = closest_point_on_line(join, t2_line);
    let d1 = (s1.0 - join.0) * to_stem.0 + (s1.1 - join.1) * to_stem.1;
    let d2 = (s2.0 - join.0) * to_stem.0 + (s2.1 - join.1) * to_stem.1;
    let (near, far) = if d1 >= d2 {
        (t_b1, t_b2)
    } else {
        (t_b2, t_b1)
    };
    let near_line = (*near.first()?, *near.last()?);
    let far_line = (*far.first()?, *far.last()?);
    let mut n0 = intersect_lines_2d(near_line.0, near_line.1, s1_line.0, s1_line.1)?;
    let mut n1 = intersect_lines_2d(near_line.0, near_line.1, s2_line.0, s2_line.1)?;
    let mut f0 = intersect_lines_2d(far_line.0, far_line.1, s1_line.0, s1_line.1)?;
    let mut f1 = intersect_lines_2d(far_line.0, far_line.1, s2_line.0, s2_line.1)?;
    if line_param(n1, near_line.0, near_line.1) < line_param(n0, near_line.0, near_line.1) {
        std::mem::swap(&mut n0, &mut n1);
        std::mem::swap(&mut f0, &mut f1);
    }
    // Full-thickness cut through this shell: two disjoint remainders.
    // A single C-ring that travels the far edge twice self-overlaps and
    // tessellators fill the notch (2D hatch and 3D extrusion).
    let mut left = vec![near_line.0, n0, f0, far_line.0];
    let mut right = vec![n1, near_line.1, far_line.1, f1];
    ensure_ccw(&mut left);
    ensure_ccw(&mut right);
    if left.len() < 3 || right.len() < 3 {
        return None;
    }
    let mut fp = left;
    fp.push((f64::NAN, f64::NAN));
    fp.extend(right);
    Some(fp)
}
