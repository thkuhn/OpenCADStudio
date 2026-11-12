use super::*;

/// Resolve which [`JoinOverrideStyle`] applies to a layer identified by
/// `layer_ref`, given `override_data`. A [`LayerPairOverride`]
/// whose `layer_a` matches `layer_ref` takes precedence over `default_style`.
/// Returns the style and the optional explicit target layer (`layer_b`).
pub(super) fn resolve_layer_override<'a>(
    layer_ref: &LayerRef,
    override_data: &'a JunctionOverride,
) -> Option<(&'a JoinOverrideStyle, Option<&'a LayerRef>)> {
    for pair in &override_data.layer_pairs {
        if layer_ref_matches_one(&pair.layer_a, layer_ref) {
            return Some((&pair.style, pair.layer_b.as_ref()));
        }
    }
    override_data.default_style.as_ref().map(|s| (s, None))
}

/// For an L-miter, a same-index `layer_b` (Innenputz↔Innenputz) can sit on
/// opposite world faces of the corner. Prefer the geometric pairing then.
pub(super) fn l_miter_target_index(
    style: JoinOverrideStyle,
    end_b: Option<usize>,
    layer_a_index: usize,
    layer_b: Option<&LayerRef>,
    pairing_fb: Option<usize>,
    explicit: Option<usize>,
) -> Option<usize> {
    if style == JoinOverrideStyle::Miter && end_b.is_some() {
        if let (Some(paired), Some(picked)) = (pairing_fb, explicit) {
            if paired != picked && layer_b.is_some_and(|r| r.index == layer_a_index) {
                return Some(paired);
            }
        }
    }
    explicit.or(pairing_fb)
}

pub(super) fn override_target_layer_index(
    layer_b: Option<&LayerRef>,
    layers_b: &[MiterLayer],
    layer_refs_b: Option<&[LayerRef]>,
    pairing_fallback: Option<usize>,
) -> Option<usize> {
    let Some(target) = layer_b else {
        return pairing_fallback;
    };
    if let Some(refs) = layer_refs_b {
        if let Some(i) = refs.iter().position(|r| layer_ref_matches_one(r, target)) {
            return Some(i);
        }
    }
    if let Some(id) = target.layer_id {
        if !id.is_nil() {
            if let Some(i) = layers_b.iter().position(|l| l.layer_id == id) {
                return Some(i);
            }
        }
    }
    if target.index < layers_b.len() {
        let l = &layers_b[target.index];
        if target.material_id.is_empty()
            || l.material.eq_ignore_ascii_case(&target.material_id)
        {
            return Some(target.index);
        }
    }
    let matches: Vec<usize> = layers_b
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            !target.material_id.is_empty()
                && l.material.eq_ignore_ascii_case(&target.material_id)
        })
        .map(|(i, _)| i)
        .collect();
    if matches.len() == 1 {
        Some(matches[0])
    } else {
        pairing_fallback
    }
}

/// Compare two [`LayerRef`]s the same way `display_apply::layer_ref_matches`
/// does: prioritize the stable `layer_id` when both sides have one set,
/// otherwise fall back to the `material_id`/`role_tag`/`index` triple. This
/// keeps legacy/hand-written overrides (with `layer_id: None`) matching a
/// live layer that now carries a real ID.
pub(super) fn layer_ref_matches_one(a: &LayerRef, b: &LayerRef) -> bool {
    match (a.layer_id, b.layer_id) {
        (Some(x), Some(y)) => x == y,
        _ => a.material_id == b.material_id && a.role_tag == b.role_tag && a.index == b.index,
    }
}

/// Compute the footprint for a single layer under a [`JunctionOverride`].
pub(super) fn apply_layer_override_footprint(
    i: usize,
    layer_ref: &LayerRef,
    override_data: &JunctionOverride,
    axis_a: &[(f64, f64)],
    layers_a: &[MiterLayer],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    layer_refs_b: Option<&[LayerRef]>,
    end_b: Option<usize>,
    contours_a: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    contours_b: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    pairing_for_l: Option<&[Option<usize>]>,
    outer_target: Option<((f64, f64), (f64, f64))>,
    default_fp: Option<Vec<(f64, f64)>>,
) -> Option<Vec<(f64, f64)>> {
    let (style, layer_b) = resolve_layer_override(layer_ref, override_data)?;
    if i >= contours_a.len() {
        return None;
    }
    let (ref a_b1, ref a_b2) = contours_a[i];

    let b_idx = if end_b.is_some() {
        let pairing_fb = pairing_for_l
            .and_then(|p| p.get(i).copied().flatten())
            .or_else(|| closest_layer_index(&layers_a[i], layers_b));
        let explicit = override_target_layer_index(layer_b, layers_b, layer_refs_b, pairing_fb);
        l_miter_target_index(*style, end_b, i, layer_b, pairing_fb, explicit)
    } else if let Some(target) = layer_b {
        override_target_layer_index(Some(target), layers_b, layer_refs_b, None)
    } else {
        let core_pair = pair_structural_cores(layers_a, layers_b);
        let is_structural = is_structural_function(&layers_a[i].function)
            || core_pair.is_some_and(|(core_a, _)| i == core_a);
        if is_structural {
            core_pair.map(|(_, core_b)| core_b)
        } else {
            approach_finish_match(i, layers_a, layers_b, core_pair, axis_a, end_a, axis_b)
        }
    };

    let b_contour = b_idx.and_then(|j| contours_b.get(j));

    let new_fp = match style {
        JoinOverrideStyle::NoExtend => {
            if end_a < a_b1.len() && end_a < a_b2.len() {
                Some(rebuild_footprint(a_b1, a_b2, end_a, a_b1[end_a], a_b2[end_a]))
            } else {
                None
            }
        }
        JoinOverrideStyle::Miter => {
            if let (Some(eb), Some((b_b1, b_b2))) = (end_b, b_contour) {
                miter_one_layer(a_b1, a_b2, end_a, b_b1, b_b2, Some(eb), axis_a, axis_b)
            } else {
                default_fp
            }
        }
        JoinOverrideStyle::Butt
        | JoinOverrideStyle::OuterFace
        | JoinOverrideStyle::NearFace
        | JoinOverrideStyle::FarFace => override_target_line(
            *style,
            a_b1,
            a_b2,
            end_a,
            b_contour,
            end_b,
            axis_a,
            axis_b,
            layers_b,
            contours_b,
            outer_target,
        )
        .and_then(|t| t_extend_one_layer(a_b1, a_b2, end_a, t)),
    };

    new_fp
}

/// Target face line a [`JoinOverrideStyle::Butt`] / `OuterFace` / `NearFace` /
/// `FarFace` override snaps layer A's joined end to. `None` for the other
/// styles or when no face can be resolved.
#[allow(clippy::too_many_arguments)]
pub(super) fn override_target_line(
    style: JoinOverrideStyle,
    a_b1: &[(f64, f64)],
    a_b2: &[(f64, f64)],
    end_a: usize,
    b_contour: Option<&(Vec<(f64, f64)>, Vec<(f64, f64)>)>,
    end_b: Option<usize>,
    axis_a: &[(f64, f64)],
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    contours_b: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    outer_target: Option<((f64, f64), (f64, f64))>,
) -> Option<((f64, f64), (f64, f64))> {
    match style {
        JoinOverrideStyle::Butt => {
            if let Some((b_b1, b_b2)) = b_contour {
                near_face_line(a_b1, a_b2, end_a, b_b1, b_b2, end_b, axis_a, axis_b)
                    .or(outer_target)
            } else {
                outer_target
            }
        }
        JoinOverrideStyle::OuterFace => outer_target,
        JoinOverrideStyle::NearFace => {
            if let Some((b_b1, b_b2)) = b_contour {
                layer_face_line(a_b1, a_b2, end_a, b_b1, b_b2, end_b, axis_a, axis_b, false)
                    .or(outer_target)
            } else {
                outer_target
            }
        }
        JoinOverrideStyle::FarFace => {
            if let Some((b_b1, b_b2)) = b_contour {
                layer_face_line(a_b1, a_b2, end_a, b_b1, b_b2, end_b, axis_a, axis_b, true).or_else(
                    || {
                        if end_b.is_some() {
                            through_outer_face_line(axis_a, end_a, axis_b, layers_b, contours_b, true)
                        } else {
                            outer_target
                        }
                    },
                )
            } else if end_b.is_some() {
                through_outer_face_line(axis_a, end_a, axis_b, layers_b, contours_b, true)
            } else {
                outer_target
            }
        }
        JoinOverrideStyle::NoExtend | JoinOverrideStyle::Miter => None,
    }
}

/// What a manual override means for one stem layer of a T / X stem.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum StemOverrideTarget {
    /// No override (or `Miter`): the automatic near-face butt applies.
    Auto,
    /// `NoExtend`: the layer ends on the axis end, nothing is extended.
    NoExtend,
    /// Override snaps the layer end to this face line.
    Line(((f64, f64), (f64, f64))),
    /// Override style whose target face could not be resolved.
    Unresolved,
}

/// Effective target of the manual override on stem layer `i` when the stem
/// butts against a through wall (`end_b = None`). Mirrors the target
/// selection of [`apply_layer_override_footprint`] so X crossings can
/// validate / clip against the face the override really asks for.
#[allow(clippy::too_many_arguments)]
pub(crate) fn t_stem_override_target(
    i: usize,
    layer_ref: &LayerRef,
    override_data: &JunctionOverride,
    axis_a: &[(f64, f64)],
    layers_a: &[MiterLayer],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    contours_a: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    contours_b: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)],
    outer_target: Option<((f64, f64), (f64, f64))>,
) -> StemOverrideTarget {
    let Some((style, layer_b)) = resolve_layer_override(layer_ref, override_data) else {
        return StemOverrideTarget::Auto;
    };
    if i >= contours_a.len() || i >= layers_a.len() {
        return StemOverrideTarget::Unresolved;
    }
    match style {
        JoinOverrideStyle::Miter => return StemOverrideTarget::Auto,
        JoinOverrideStyle::NoExtend => return StemOverrideTarget::NoExtend,
        _ => {}
    }
    let (ref a_b1, ref a_b2) = contours_a[i];
    let b_idx = if let Some(target) = layer_b {
        override_target_layer_index(Some(target), layers_b, None, None)
    } else {
        let core_pair = pair_structural_cores(layers_a, layers_b);
        let is_structural = is_structural_function(&layers_a[i].function)
            || core_pair.is_some_and(|(core_a, _)| i == core_a);
        if is_structural {
            core_pair.map(|(_, core_b)| core_b)
        } else {
            approach_finish_match(i, layers_a, layers_b, core_pair, axis_a, end_a, axis_b)
        }
    };
    let b_contour = b_idx.and_then(|j| contours_b.get(j));
    match override_target_line(
        *style,
        a_b1,
        a_b2,
        end_a,
        b_contour,
        None,
        axis_a,
        axis_b,
        layers_b,
        contours_b,
        outer_target,
    ) {
        Some(line) => StemOverrideTarget::Line(line),
        None => StemOverrideTarget::Unresolved,
    }
}

/// Override-aware variant of [`mitered_layer_footprints`].
///
/// `layer_refs_a` must be aligned 1:1 with `layers_a` and carries the
/// material_id/role_tag identity used to match `override_data`'s
/// `layer_pairs`. When `override_data` is `None` (or a layer has no
/// matching override and no `default_style` applies), the result for that
/// layer is byte-for-byte identical to [`mitered_layer_footprints`].
///
/// Per-layer precedence: matching `LayerPairOverride` > `default_style` >
/// automatic computation. See [`JoinOverrideStyle`] for how each style
/// translates into geometry.
pub fn mitered_layer_footprints_with_override(
    axis_a: &[(f64, f64)],
    layers_a: &[MiterLayer],
    layer_refs_a: &[LayerRef],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    end_b: Option<usize>,
    kind: JoinKind,
    override_data: Option<&JunctionOverride>,
) -> Vec<Option<Vec<(f64, f64)>>> {
    mitered_layer_footprints_with_override_and_bulges(
        axis_a,
        layers_a,
        layer_refs_a,
        end_a,
        axis_b,
        layers_b,
        end_b,
        kind,
        override_data,
        &[],
        &[],
    )
}

/// Override-aware miter using bulge-offset layer contours.
pub fn mitered_layer_footprints_with_override_and_bulges(
    axis_a: &[(f64, f64)],
    layers_a: &[MiterLayer],
    layer_refs_a: &[LayerRef],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    end_b: Option<usize>,
    kind: JoinKind,
    override_data: Option<&JunctionOverride>,
    bulges_a: &[f64],
    bulges_b: &[f64],
) -> Vec<Option<Vec<(f64, f64)>>> {
    let mut out = mitered_layer_footprints_with_bulges(
        axis_a, layers_a, end_a, axis_b, layers_b, end_b, kind, bulges_a, bulges_b,
    );
    let Some(ov) = override_data else {
        return out;
    };
    if axis_a.len() < 2 || axis_b.len() < 2 || layers_a.is_empty() || end_a >= axis_a.len() {
        return out;
    }

    let geom_a: Vec<(f64, f64)> = layers_a.iter().map(MiterLayer::as_geom).collect();
    let geom_b: Vec<(f64, f64)> = layers_b.iter().map(MiterLayer::as_geom).collect();
    let contours_a = contours_xy(axis_a, bulges_a, &geom_a);
    let contours_b = contours_xy(axis_b, bulges_b, &geom_b);
    if contours_a.is_empty() || contours_b.is_empty() {
        return out;
    }
    let pairing = if end_b.is_some() {
        Some(match_layer_indices_for_l_join(layers_a, layers_b, end_a, end_b))
    } else {
        None
    };
    let outer_target =
        through_outer_near_face_line(axis_a, end_a, axis_b, layers_b, &contours_b);

    for i in 0..layers_a.len() {
        let Some(layer_ref) = layer_refs_a.get(i) else {
            continue;
        };
        if let Some(fp) = apply_layer_override_footprint(
            i,
            layer_ref,
            ov,
            axis_a,
            layers_a,
            end_a,
            axis_b,
            layers_b,
            None,
            end_b,
            &contours_a,
            &contours_b,
            pairing.as_deref(),
            outer_target,
            out[i].clone(),
        ) {
            if fp.len() >= 3 {
                out[i] = Some(fp);
            }
        }
    }
    out
}
