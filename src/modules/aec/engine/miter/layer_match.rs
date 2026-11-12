use super::*;

pub(super) fn point_axis_offset(p: (f64, f64), axis: &[(f64, f64)]) -> f64 {
    if axis.len() < 2 {
        return 0.0;
    }
    let mut best = f64::INFINITY;
    for s in 0..axis.len() - 1 {
        let d = point_seg_dist(p, axis[s], axis[s + 1]);
        if d < best {
            best = d;
        }
    }
    best
}

pub(super) fn lines_equivalent(
    a: ((f64, f64), (f64, f64)),
    b: ((f64, f64), (f64, f64)),
) -> bool {
    let da = point_seg_dist(a.0, b.0, b.1);
    let db = point_seg_dist(a.1, b.0, b.1);
    da < 1e-6 && db < 1e-6
}

pub(crate) fn is_structural_function(function: &str) -> bool {
    let f = function.trim();
    f.eq_ignore_ascii_case("structural")
        || f.eq_ignore_ascii_case("structure")
        || f.eq_ignore_ascii_case("tragwerk")
        || f.eq_ignore_ascii_case("tragschicht")
}

/// Pair Structural cores: among Structural layers, prefer material match
/// (`match_layer_indices` restricted to those); else the thickest Structural
/// on each wall. With no Structural, fall back to the thickest layer.
pub(crate) fn pair_structural_cores(layers_a: &[MiterLayer], layers_b: &[MiterLayer]) -> Option<(usize, usize)> {
    let struct_a: Vec<usize> = layers_a
        .iter()
        .enumerate()
        .filter(|(_, l)| is_structural_function(&l.function))
        .map(|(i, _)| i)
        .collect();
    let struct_b: Vec<usize> = layers_b
        .iter()
        .enumerate()
        .filter(|(_, l)| is_structural_function(&l.function))
        .map(|(i, _)| i)
        .collect();

    if !struct_a.is_empty() && !struct_b.is_empty() {
        let sub_a: Vec<MiterLayer> = struct_a.iter().map(|&i| layers_a[i].clone()).collect();
        let sub_b: Vec<MiterLayer> = struct_b.iter().map(|&i| layers_b[i].clone()).collect();
        let pairing = match_layer_indices(&sub_a, &sub_b);
        for (local_a, maybe_b) in pairing.into_iter().enumerate() {
            if let Some(local_b) = maybe_b {
                return Some((struct_a[local_a], struct_b[local_b]));
            }
        }
        let thickest = |idxs: &[usize], layers: &[MiterLayer]| {
            idxs.iter()
                .copied()
                .max_by(|i, j| {
                    layers[*i]
                        .thickness
                        .partial_cmp(&layers[*j].thickness)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
        };
        return Some((thickest(&struct_a, layers_a)?, thickest(&struct_b, layers_b)?));
    }

    if layers_a.is_empty() || layers_b.is_empty() {
        return None;
    }
    let thickest = |layers: &[MiterLayer]| {
        layers
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                a.thickness
                    .partial_cmp(&b.thickness)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
    };
    Some((thickest(layers_a)?, thickest(layers_b)?))
}

/// Pair each layer of wall A to at most one layer of wall B.
///
/// Matching priority:
/// 1. identical non-nil `layer_id`
/// 2. `material` equality (and `function` equality when both are non-empty)
/// 3. closest offset-from-axis (tie-break / no material match)
///
/// A layer is left unmatched (`None`) when no unique candidate remains —
/// those fall back to the single-vertex corner extension at the caller.
pub fn match_layer_indices(layers_a: &[MiterLayer], layers_b: &[MiterLayer]) -> Vec<Option<usize>> {
    let n_a = layers_a.len();
    let n_b = layers_b.len();
    if n_a == 0 {
        return Vec::new();
    }
    if n_b == 0 {
        return vec![None; n_a];
    }

    let centers_a = layer_center_offsets(layers_a);
    let centers_b = layer_center_offsets(layers_b);
    let mut used_b = vec![false; n_b];
    let mut out = vec![None; n_a];
    let mut done_a = vec![false; n_a];

    // Greedy global assignment: repeatedly take the best remaining (i, j)
    // pair by (identity class, offset distance). Ambiguous A layers (two B
    // candidates with the same class and equal distance) stay unmatched.
    loop {
        let mut best: Option<(u8, f64, usize, usize)> = None; // (class, dist, i, j)
        for i in 0..n_a {
            if done_a[i] {
                continue;
            }
            for j in 0..n_b {
                if used_b[j] {
                    continue;
                }
                let class = identity_class(&layers_a[i], &layers_b[j]);
                let dist = (centers_a[i] - centers_b[j]).abs();
                match best {
                    None => best = Some((class, dist, i, j)),
                    Some((bc, bd, _, _)) => {
                        if class < bc || (class == bc && dist < bd - 1e-12) {
                            best = Some((class, dist, i, j));
                        }
                    }
                }
            }
        }
        let Some((class, dist, i, j)) = best else {
            break;
        };

        let mut ambiguous = false;
        for j2 in 0..n_b {
            if j2 == j || used_b[j2] {
                continue;
            }
            let c2 = identity_class(&layers_a[i], &layers_b[j2]);
            if c2 != class {
                continue;
            }
            let d2 = (centers_a[i] - centers_b[j2]).abs();
            if (d2 - dist).abs() < 1e-9 {
                ambiguous = true;
                break;
            }
        }
        if class >= 3 {
            // Offset-only pairs are not identity. Remaining layers are
            // matched by stack index below so different materials still join
            // when they occupy the same slot.
            break;
        }
        done_a[i] = true;
        if ambiguous {
            // Leave out[i] = None (fallback to corner_override).
            continue;
        }
        out[i] = Some(j);
        used_b[j] = true;
    }

    for i in 0..n_a {
        if out[i].is_some() {
            continue;
        }
        if i < n_b && !used_b[i] {
            out[i] = Some(i);
            used_b[i] = true;
        }
    }

    out
}

/// When both walls use the same endpoint polarity at the joint (both start
/// vertices or both end vertices), they arrive from opposite circulation.
/// Matching against a mirrored copy of B then pairs finishes that share a
/// world-side of the L (inner plaster to the concave/convex bisector)
/// instead of cutting the innermost strip short of the corner.
pub(super) fn l_join_mirror_other_stack(end_a: usize, end_b: Option<usize>) -> bool {
    let Some(end_b) = end_b else {
        return false;
    };
    (end_a == 0) == (end_b == 0)
}

pub(super) fn mirror_layer_offsets_for_match(layers: &[MiterLayer]) -> Vec<MiterLayer> {
    layers
        .iter()
        .map(|layer| {
            let mut mirrored = layer.clone();
            mirrored.axis_offset = -(layer.axis_offset + layer.thickness);
            mirrored.layer_id = uuid::Uuid::nil();
            mirrored
        })
        .collect()
}

pub(crate) fn match_layer_indices_for_l_join(
    layers_a: &[MiterLayer],
    layers_b: &[MiterLayer],
    end_a: usize,
    end_b: Option<usize>,
) -> Vec<Option<usize>> {
    if l_join_mirror_other_stack(end_a, end_b) {
        match_layer_indices(layers_a, &mirror_layer_offsets_for_match(layers_b))
    } else {
        match_layer_indices(layers_a, layers_b)
    }
}

/// Identity match class: 0 = same layer_id, 1 = material+function,
/// 2 = material only, 3 = offset-only. Lower is better.
pub(super) fn identity_class(a: &MiterLayer, b: &MiterLayer) -> u8 {
    if !a.layer_id.is_nil() && a.layer_id == b.layer_id {
        return 0;
    }
    let mat = !a.material.is_empty() && a.material == b.material;
    if !mat {
        return 3;
    }
    let fun = !a.function.is_empty() && !b.function.is_empty() && a.function == b.function;
    if fun {
        1
    } else {
        2
    }
}

/// Centre offset of each layer from the reference axis:
/// `center = axis_offset + thickness/2` (direct, no stacking).
pub(super) fn layer_center_offsets(layers: &[MiterLayer]) -> Vec<f64> {
    layers
        .iter()
        .map(|l| l.axis_offset + l.thickness * 0.5)
        .collect()
}
