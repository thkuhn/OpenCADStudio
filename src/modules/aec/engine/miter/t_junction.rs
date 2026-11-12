use super::*;

/// T-junction footprints: the Structural/core stem layer butts the near
/// face of the paired through-wall Structural layer. Remaining stem layers
/// extend to the through-wall outer face on the approach side (no tunnel).
pub(super) fn t_junction_layer_footprints(
    axis_a: &[(f64, f64)],
    layers_a: &[MiterLayer],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    contours_a: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    contours_b: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
) -> Vec<Option<Vec<(f64, f64)>>> {
    let outer_target =
        through_outer_near_face_line(axis_a, end_a, axis_b, layers_b, contours_b);
    let core_pair = pair_structural_cores(layers_a, layers_b);
    let mut out = Vec::with_capacity(layers_a.len());

    for i in 0..layers_a.len() {
        if i >= contours_a.len() {
            out.push(None);
            continue;
        }
        let (ref a_b1, ref a_b2) = contours_a[i];
        let is_structural = is_structural_function(&layers_a[i].function)
            || core_pair.is_some_and(|(core_a, _)| i == core_a);

        let fp = if is_structural {
            if let Some((_, core_b)) = core_pair {
                if core_b < contours_b.len() {
                    let (ref b_b1, ref b_b2) = contours_b[core_b];
                    let target = near_face_line(a_b1, a_b2, end_a, b_b1, b_b2, None, axis_a, axis_b)
                        .or(outer_target);
                    target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
                } else {
                    outer_target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
                }
            } else {
                outer_target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
            }
        } else if let Some(j) = approach_finish_match(i, layers_a, layers_b, core_pair, axis_a, end_a, axis_b)
        {
            if j < contours_b.len() {
                let (ref b_b1, ref b_b2) = contours_b[j];
                t_miter_finish_against_through(
                    a_b1, a_b2, end_a, b_b1, b_b2, axis_a, axis_b,
                )
            } else {
                outer_target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
            }
        } else {
            outer_target.and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t))
        };

        match fp {
            Some(poly) if poly.len() >= 3 => out.push(Some(poly)),
            _ => out.push(None),
        }
    }
    out
}

pub(crate) fn approach_finish_match(
    stem_i: usize,
    layers_a: &[MiterLayer],
    layers_b: &[MiterLayer],
    core_pair: Option<(usize, usize)>,
    axis_a: &[(f64, f64)],
    end_a: usize,
    axis_b: &[(f64, f64)],
) -> Option<usize> {
    let stem = layers_a.get(stem_i)?;
    if is_structural_function(&stem.function) {
        return None;
    }
    if stem.material.is_empty() {
        return None;
    }
    let core_b = core_pair.map(|(_, b)| b);
    let toward_stem = through_offset_toward_stem(axis_a, end_a, axis_b);
    let core_center = core_b.map(|j| {
        let l = &layers_b[j];
        l.axis_offset + l.thickness * 0.5
    });
    let ref_center = core_center.unwrap_or_else(|| {
        if layers_b.is_empty() {
            0.0
        } else {
            let mut min_off = f64::INFINITY;
            let mut max_off = f64::NEG_INFINITY;
            for l in layers_b {
                min_off = min_off.min(l.axis_offset);
                max_off = max_off.max(l.axis_offset + l.thickness);
            }
            (min_off + max_off) * 0.5
        }
    });

    let mut best: Option<(usize, f64)> = None;
    for (j, thru) in layers_b.iter().enumerate() {
        if Some(j) == core_b || is_structural_function(&thru.function) {
            continue;
        }
        if !thru.material.eq_ignore_ascii_case(&stem.material) {
            continue;
        }
        let center = thru.axis_offset + thru.thickness * 0.5;
        if (center - ref_center) * toward_stem < 1e-9 {
            continue;
        }
        let d = center * toward_stem;
        match best {
            None => best = Some((j, d)),
            Some((_, bd)) if d > bd => best = Some((j, d)),
            _ => {}
        }
    }
    best.map(|(j, _)| j)
}

pub(super) fn through_offset_toward_stem(
    axis_a: &[(f64, f64)],
    end_a: usize,
    axis_b: &[(f64, f64)],
) -> f64 {
    if axis_a.len() < 2 || axis_b.len() < 2 || end_a >= axis_a.len() {
        return 1.0;
    }
    let join = axis_a[end_a];
    let prev = if end_a == 0 { 1 } else { end_a - 1 };
    let stem = (axis_a[prev].0 - join.0, axis_a[prev].1 - join.1);
    let mut best_seg = 0usize;
    let mut best_d = f64::INFINITY;
    for s in 0..axis_b.len().saturating_sub(1) {
        let d = point_seg_dist(join, axis_b[s], axis_b[s + 1]);
        if d < best_d {
            best_d = d;
            best_seg = s;
        }
    }
    let b0 = axis_b[best_seg];
    let b1 = axis_b[best_seg + 1];
    let bx = b1.0 - b0.0;
    let by = b1.1 - b0.1;
    let bl = (bx * bx + by * by).sqrt();
    if bl < 1e-12 {
        return 1.0;
    }
    let nx = -by / bl;
    let ny = bx / bl;
    let side = nx * stem.0 + ny * stem.1;
    if side >= 0.0 {
        1.0
    } else {
        -1.0
    }
}

pub(super) fn t_miter_finish_against_through(
    a_b1: &[(f64, f64)],
    a_b2: &[(f64, f64)],
    end_a: usize,
    b_b1: &[(f64, f64)],
    b_b2: &[(f64, f64)],
    axis_a: &[(f64, f64)],
    axis_b: &[(f64, f64)],
) -> Option<Vec<(f64, f64)>> {
    if a_b1.len() < 2 || a_b2.len() < 2 || end_a >= a_b1.len() {
        return None;
    }
    let (b1_line, b2_line) = other_boundary_lines(b_b1, b_b2, None, axis_a, axis_b, end_a)?;
    let outer_b = near_face_line(a_b1, a_b2, end_a, b_b1, b_b2, None, axis_a, axis_b)?;
    let inner_b = if lines_equivalent(outer_b, b1_line) {
        b2_line
    } else {
        b1_line
    };
    let prev_a = if end_a == 0 { 1 } else { end_a - 1 };
    let a1_line = extended_line(a_b1[prev_a], a_b1[end_a]);
    let a2_line = extended_line(a_b2[prev_a], a_b2[end_a]);
    // Inner stem edge is closer to the wall axis; outer is farther out.
    let d1 = point_axis_offset(a_b1[end_a], axis_a);
    let d2 = point_axis_offset(a_b2[end_a], axis_a);
    let (inner_a, outer_a) = if d1 <= d2 {
        (a1_line, a2_line)
    } else {
        (a2_line, a1_line)
    };
    let new_inner = intersect_lines_2d(inner_a.0, inner_a.1, inner_b.0, inner_b.1)?;
    let new_outer = intersect_lines_2d(outer_a.0, outer_a.1, outer_b.0, outer_b.1)?;
    let (new_a1, new_a2) = if d1 <= d2 {
        (new_inner, new_outer)
    } else {
        (new_outer, new_inner)
    };
    Some(rebuild_footprint(a_b1, a_b2, end_a, new_a1, new_a2))
}
