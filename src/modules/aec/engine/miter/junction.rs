use super::*;

/// One wall's geometry at a multi-wall junction (axis already snapped).
#[derive(Debug, Clone)]
pub struct JunctionWallGeom {
    pub axis: Vec<(f64, f64)>,
    pub layers: Vec<MiterLayer>,
    /// `Some(end_idx)` when this wall ends at the junction; `None` for a
    /// through-wall (T stem target) that only participates as a miter partner.
    pub end: Option<usize>,
}

/// Resolve per-layer mitered footprints for every wall at a multi-wall junction.
///
/// Endpoint walls are ordered by outgoing angle around the junction. Each is
/// mitered against its two angular neighbors (reusing [`match_layer_indices`]
/// pairwise). Through-walls only act as miter targets and receive an all-`None`
/// result (rectangular fallback at the caller). When a layer cannot be matched
/// on either side the entry stays `None` (single-vertex `corner_override`).
///
/// `walls` must be aligned with `junction.participants` (same length/order) —
/// typically built after [`super::super::join::apply_junction_to_axes`].
pub fn mitered_junction_layer_footprints(
    junction: &Junction,
    walls: &[JunctionWallGeom],
) -> Vec<Vec<Option<Vec<(f64, f64)>>>> {
    let n = walls.len();
    let mut out: Vec<Vec<Option<Vec<(f64, f64)>>>> = walls
        .iter()
        .map(|w| vec![None; w.layers.len()])
        .collect();
    if n == 0 || junction.participants.len() != n {
        return out;
    }

    // Fast path: fewer than 2 endpoint walls → nothing to miter together.
    let endpoint_indices: Vec<usize> = walls
        .iter()
        .enumerate()
        .filter_map(|(i, w)| w.end.map(|_| i))
        .collect();
    if endpoint_indices.is_empty() {
        return out;
    }

    // Two-wall (or one endpoint + through) → reuse pairwise helper.
    if n == 2 || (endpoint_indices.len() == 1 && n == 2) {
        for &ei in &endpoint_indices {
            let other = 1 - ei;
            let self_w = &walls[ei];
            let other_w = &walls[other];
            let Some(end_a) = self_w.end else { continue };
            let kind = if other_w.end.is_some() {
                JoinKind::L
            } else {
                JoinKind::T
            };
            out[ei] = mitered_layer_footprints(
                &self_w.axis,
                &self_w.layers,
                end_a,
                &other_w.axis,
                &other_w.layers,
                other_w.end,
                kind,
            );
        }
        return out;
    }

    // Build axis refs for ray ordering (DVec3-less 2D → temporary DVec3 via join helpers).
    // junction_rays expects &[&[DVec3]]; we work purely in 2D here and sort by angle ourselves.
    let jp = (junction.point.x, junction.point.y);
    let rays = ordered_junction_rays_2d(jp, walls);
    if rays.len() < 2 {
        return out;
    }

    // Pre-compute contours per wall.
    let contours: Vec<Vec<(Vec<(f64, f64)>, Vec<(f64, f64)>)>> = walls
        .iter()
        .map(|w| {
            let geom: Vec<(f64, f64)> = w.layers.iter().map(MiterLayer::as_geom).collect();
            layer_contours(&w.axis, &geom)
        })
        .collect();

    // For each ray that belongs to an endpoint wall, miter against angular neighbors.
    // Multiple rays can reference the same through-wall participant; endpoint walls
    // have exactly one ray.
    for (ri, ray) in rays.iter().enumerate() {
        let wi = ray.wall_index;
        let Some(end_a) = walls[wi].end else {
            // Through-wall ray: not rebuilding its own footprint.
            continue;
        };
        let n_rays = rays.len();
        let prev = &rays[(ri + n_rays - 1) % n_rays];
        let next = &rays[(ri + 1) % n_rays];
        // Skip self-adjacent (shouldn't happen with ≥2 distinct rays).
        if prev.wall_index == wi && next.wall_index == wi {
            continue;
        }

        // b1 = negative-offset side = CW / right of leave dir → prev in CCW order.
        // b2 = positive-offset side = CCW / left of leave dir → next in CCW order.
        let right_w = prev.wall_index;
        let left_w = next.wall_index;
        let right_collinear = is_collinear_opposite_endpoint(&walls[wi], &walls[right_w]);
        let left_collinear = is_collinear_opposite_endpoint(&walls[wi], &walls[left_w]);

        let pair_right = match_layer_indices(&walls[wi].layers, &walls[right_w].layers);
        let pair_left = match_layer_indices(&walls[wi].layers, &walls[left_w].layers);

        for li in 0..walls[wi].layers.len() {
            if li >= contours[wi].len() {
                out[wi][li] = None;
                continue;
            }
            let (ref a_b1, ref a_b2) = contours[wi][li];

            // Resolve each side independently; if only one neighbor matches,
            // miter both sides against that neighbor (same as pairwise).
            let right_j = pair_right.get(li).copied().flatten();
            let left_j = pair_left.get(li).copied().flatten();

            let fp = match (right_j, left_j) {
                (Some(rj), Some(_lj)) if right_w == left_w => {
                    // Both neighbors are the same wall (classic T against through).
                    if rj >= contours[right_w].len() {
                        None
                    } else {
                        let (ref b_b1, ref b_b2) = contours[right_w][rj];
                        miter_one_layer(
                            a_b1,
                            a_b2,
                            end_a,
                            b_b1,
                            b_b2,
                            walls[right_w].end,
                            &walls[wi].axis,
                            &walls[right_w].axis,
                        )
                    }
                }
                (Some(rj), Some(lj)) => {
                    if rj >= contours[right_w].len() || lj >= contours[left_w].len() {
                        None
                    } else {
                        let (ref r_b1, ref r_b2) = contours[right_w][rj];
                        let (ref l_b1, ref l_b2) = contours[left_w][lj];
                        let ends_r = miter_end_points(
                            a_b1,
                            a_b2,
                            end_a,
                            r_b1,
                            r_b2,
                            walls[right_w].end,
                            &walls[wi].axis,
                            &walls[right_w].axis,
                        );
                        let ends_l = miter_end_points(
                            a_b1,
                            a_b2,
                            end_a,
                            l_b1,
                            l_b2,
                            walls[left_w].end,
                            &walls[wi].axis,
                            &walls[left_w].axis,
                        );
                        match (ends_r, ends_l) {
                            // b1 from right neighbor, b2 from left neighbor.
                            (Some((new_a1, _)), Some((_, new_a2))) => {
                                Some(rebuild_footprint(a_b1, a_b2, end_a, new_a1, new_a2))
                            }
                            (Some((new_a1, new_a2)), None) if !left_collinear => {
                                Some(rebuild_footprint(a_b1, a_b2, end_a, new_a1, new_a2))
                            }
                            (None, Some((new_a1, new_a2))) if !right_collinear => {
                                Some(rebuild_footprint(a_b1, a_b2, end_a, new_a1, new_a2))
                            }
                            // Collinear opposite neighbor (180°): keep that
                            // side square so two same-axis walls do not both
                            // L-miter into the same square.
                            (Some((new_a1, _)), None) if left_collinear => {
                                Some(rebuild_footprint(a_b1, a_b2, end_a, new_a1, a_b2[end_a]))
                            }
                            (None, Some((_, new_a2))) if right_collinear => {
                                Some(rebuild_footprint(a_b1, a_b2, end_a, a_b1[end_a], new_a2))
                            }
                            _ => None,
                        }
                    }
                }
                (Some(rj), None) => {
                    if rj >= contours[right_w].len() {
                        None
                    } else {
                        let (ref b_b1, ref b_b2) = contours[right_w][rj];
                        miter_one_layer(
                            a_b1,
                            a_b2,
                            end_a,
                            b_b1,
                            b_b2,
                            walls[right_w].end,
                            &walls[wi].axis,
                            &walls[right_w].axis,
                        )
                    }
                }
                (None, Some(lj)) => {
                    if lj >= contours[left_w].len() {
                        None
                    } else {
                        let (ref b_b1, ref b_b2) = contours[left_w][lj];
                        miter_one_layer(
                            a_b1,
                            a_b2,
                            end_a,
                            b_b1,
                            b_b2,
                            walls[left_w].end,
                            &walls[wi].axis,
                            &walls[left_w].axis,
                        )
                    }
                }
                (None, None) => {
                    // No material match on either side. If one of the angular
                    // neighbors is a through-wall, extend to its outer face
                    // (mirrors the pairwise `t_junction_layer_footprints` path).
                    let through_w = if walls[right_w].end.is_none() {
                        Some(right_w)
                    } else if walls[left_w].end.is_none() {
                        Some(left_w)
                    } else {
                        None
                    };
                    through_w.and_then(|tw| {
                        let outer = through_outer_near_face_line(
                            &walls[wi].axis,
                            end_a,
                            &walls[tw].axis,
                            &walls[tw].layers,
                            &contours[tw],
                        )?;
                        t_extend_one_layer(a_b1, a_b2, end_a, outer)
                    })
                }
            };

            out[wi][li] = fp.filter(|p| p.len() >= 3);
        }
    }

    out
}

/// Override-aware variant of [`mitered_junction_layer_footprints`].
///
/// `layer_refs` and `overrides` must be aligned with `walls` /
/// `junction.participants` (one entry per participant). `layer_refs[wi]`
/// carries the material_id/role_tag identity for `walls[wi].layers`;
/// `overrides[wi]` is the [`JunctionOverride`] stored for that wall's end at
/// this junction (`None` when no override exists).
///
/// Endpoint walls with an override recompute overridden layers against
/// their angular right-hand neighbor (mirroring the automatic path's
/// pairwise fallback); layers without a matching override — and walls
/// without any override at all — stay byte-for-byte identical to
/// [`mitered_junction_layer_footprints`].
pub fn mitered_junction_layer_footprints_with_overrides(
    junction: &Junction,
    walls: &[JunctionWallGeom],
    layer_refs: &[Vec<LayerRef>],
    overrides: &[Option<JunctionOverride>],
) -> Vec<Vec<Option<Vec<(f64, f64)>>>> {
    let mut out = mitered_junction_layer_footprints(junction, walls);
    let n = walls.len();
    if n == 0 || junction.participants.len() != n {
        return out;
    }

    let jp = (junction.point.x, junction.point.y);
    let rays = ordered_junction_rays_2d(jp, walls);
    if rays.len() < 2 {
        return out;
    }

    let contours: Vec<Vec<(Vec<(f64, f64)>, Vec<(f64, f64)>)>> = walls
        .iter()
        .map(|w| {
            let geom: Vec<(f64, f64)> = w.layers.iter().map(MiterLayer::as_geom).collect();
            layer_contours(&w.axis, &geom)
        })
        .collect();

    for (ri, ray) in rays.iter().enumerate() {
        let wi = ray.wall_index;
        let Some(end_a) = walls[wi].end else {
            continue;
        };
        let Some(refs) = layer_refs.get(wi) else {
            continue;
        };
        let n_rays = rays.len();
        let prev = &rays[(ri + n_rays - 1) % n_rays];
        let ov_self = overrides.get(wi).and_then(|o| o.as_ref());
        let ov_partner = overrides.get(prev.wall_index).and_then(|o| o.as_ref());
        let ov_merged = crate::modules::aec::engine::join::merge_partner_junction_override(
            ov_self, ov_partner,
        );
        let Some(ov) = ov_merged.as_ref() else {
            continue;
        };
        let other_w = prev.wall_index;
        if other_w == wi {
            continue;
        }
        let other_axis = &walls[other_w].axis;
        let other_contours = &contours[other_w];
        let pairing = if walls[other_w].end.is_some() {
            Some(match_layer_indices_for_l_join(
                &walls[wi].layers,
                &walls[other_w].layers,
                end_a,
                walls[other_w].end,
            ))
        } else {
            None
        };
        let outer_target = through_outer_near_face_line(
            &walls[wi].axis,
            end_a,
            other_axis,
            &walls[other_w].layers,
            other_contours,
        );
        let refs_b = layer_refs.get(other_w).map(|v| v.as_slice());

        for li in 0..walls[wi].layers.len() {
            let Some(layer_ref) = refs.get(li) else {
                continue;
            };
            if let Some(fp) = apply_layer_override_footprint(
                li,
                layer_ref,
                ov,
                &walls[wi].axis,
                &walls[wi].layers,
                end_a,
                other_axis,
                &walls[other_w].layers,
                refs_b,
                walls[other_w].end,
                &contours[wi],
                other_contours,
                pairing.as_deref(),
                outer_target,
                out[wi][li].clone(),
            ) {
                if fp.len() >= 3 {
                    out[wi][li] = Some(fp);
                }
            }
        }
    }

    out
}

/// Build [`JunctionWallGeom`] list aligned with `junction.participants` from
/// already-snapped axis polylines (`axes[wall_index]`) and layer stacks.
pub fn junction_wall_geoms(
    junction: &Junction,
    axes: &[Vec<(f64, f64)>],
    layers: &[Vec<MiterLayer>],
) -> Vec<JunctionWallGeom> {
    junction
        .participants
        .iter()
        .map(|p| {
            let axis = axes
                .get(p.wall_index)
                .cloned()
                .unwrap_or_default();
            let ly = layers
                .get(p.wall_index)
                .cloned()
                .unwrap_or_default();
            let end = match p.role {
                JunctionRole::Endpoint(e) => Some(e),
                JunctionRole::Through(_) => None,
            };
            JunctionWallGeom {
                axis,
                layers: ly,
                end,
            }
        })
        .collect()
}

#[derive(Clone, Copy)]
pub(super) struct Ray2 {
    wall_index: usize,
    angle: f64,
}

/// CCW-sorted outgoing rays at a junction (through-walls contribute two).
pub(super) fn ordered_junction_rays_2d(jp: (f64, f64), walls: &[JunctionWallGeom]) -> Vec<Ray2> {
    let mut rays = Vec::new();
    for (wi, w) in walls.iter().enumerate() {
        if w.axis.len() < 2 {
            continue;
        }
        if let Some(end) = w.end {
            if end >= w.axis.len() {
                continue;
            }
            let interior = if end == 0 { 1 } else { end - 1 };
            let dx = w.axis[interior].0 - jp.0;
            let dy = w.axis[interior].1 - jp.1;
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-12 {
                continue;
            }
            rays.push(Ray2 {
                wall_index: wi,
                angle: dy.atan2(dx),
            });
        } else {
            // Through-wall: find segment closest to junction and emit two rays.
            let mut best_seg = 0usize;
            let mut best_d = f64::INFINITY;
            for s in 0..w.axis.len() - 1 {
                let d = point_seg_dist(jp, w.axis[s], w.axis[s + 1]);
                if d < best_d {
                    best_d = d;
                    best_seg = s;
                }
            }
            for target in [w.axis[best_seg], w.axis[best_seg + 1]] {
                let dx = target.0 - jp.0;
                let dy = target.1 - jp.1;
                let len = (dx * dx + dy * dy).sqrt();
                if len < 1e-12 {
                    continue;
                }
                rays.push(Ray2 {
                    wall_index: wi,
                    angle: dy.atan2(dx),
                });
            }
        }
    }
    rays.sort_by(|a, b| {
        a.angle
            .partial_cmp(&b.angle)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // Drop consecutive duplicate angles from the same wall.
    rays.dedup_by(|a, b| {
        a.wall_index == b.wall_index && (a.angle - b.angle).abs() < 1e-12
    });
    rays
}

/// Compute the two replaced end-cap points (new_a1 on b1, new_a2 on b2) for
/// one layer of wall A against wall B — shared by pairwise and N-way paths.
///
/// When `end_b` is `None` (B is a T through-wall), both end-caps butt against
/// the near face of B's layer (straight extension). Otherwise a diagonal
/// L-miter is formed.
pub(super) fn miter_end_points(
    a_b1: &[(f64, f64)],
    a_b2: &[(f64, f64)],
    end_a: usize,
    b_b1: &[(f64, f64)],
    b_b2: &[(f64, f64)],
    end_b: Option<usize>,
    axis_a: &[(f64, f64)],
    axis_b: &[(f64, f64)],
) -> Option<((f64, f64), (f64, f64))> {
    if a_b1.len() < 2 || a_b2.len() < 2 || a_b1.len() != a_b2.len() {
        return None;
    }
    if end_a >= a_b1.len() {
        return None;
    }
    if b_b1.len() < 2 || b_b2.len() < 2 || b_b1.len() != b_b2.len() {
        return None;
    }

    // T through-wall partner: straight butt, not diagonal miter.
    if end_b.is_none() {
        let target = near_face_line(a_b1, a_b2, end_a, b_b1, b_b2, None, axis_a, axis_b)?;
        return t_extend_end_points(a_b1, a_b2, end_a, target);
    }

    let prev_a = if end_a == 0 { 1 } else { end_a - 1 };
    let a1_line = extended_line(a_b1[prev_a], a_b1[end_a]);
    let a2_line = extended_line(a_b2[prev_a], a_b2[end_a]);

    // Degenerate angle check: if the walls meet at a very shallow or very
    // sharp angle, the miter intersection point moves to infinity.
    // `end_b` is guaranteed `Some` here (the T through-wall case returned
    // early above), so `lb` is always the L/T-stem leave direction.
    let la = wall_leave_dir(axis_a, end_a);
    let lb = wall_leave_dir(axis_b, end_b.expect("end_b is Some past the T-join early return"));
    let det = la.0 * lb.1 - la.1 * lb.0;
    if det.abs() < MIN_MITER_SINE {
        return None;
    }

    let (b1_line, b2_line) = other_boundary_lines(b_b1, b_b2, end_b, axis_a, axis_b, end_a)?;

    let i_a1_b1 = intersect_lines_2d(a1_line.0, a1_line.1, b1_line.0, b1_line.1)?;
    let i_a1_b2 = intersect_lines_2d(a1_line.0, a1_line.1, b2_line.0, b2_line.1)?;
    let i_a2_b1 = intersect_lines_2d(a2_line.0, a2_line.1, b1_line.0, b1_line.1)?;
    let i_a2_b2 = intersect_lines_2d(a2_line.0, a2_line.1, b2_line.0, b2_line.1)?;

    let pair_direct = (i_a1_b1, i_a2_b2);
    let pair_cross = (i_a1_b2, i_a2_b1);

    // The correct pairing (`direct`: a1↔b1, a2↔b2, vs `cross`: a1↔b2,
    // a2↔b1) is a fixed geometric fact about which offset boundary
    // continues into which at the corner — it does NOT depend on layer
    // thickness/gap, so it must never be chosen by comparing the resulting
    // segment lengths (the historical approach): that heuristic only
    // happens to agree with the correct choice near 90°, where both
    // candidates are (near-)equidistant, but silently flips to the wrong
    // (beveled) pairing for acute angles where the wrong candidate becomes
    // shorter. See the `acute_*_deg_l_corner_*` regression tests below.
    //
    // `a1`/`b1` are each defined as the "right-hand" (negative) offset
    // relative to their own axis's stored point order (index 0 → last),
    // `a2`/`b2` the "left-hand" one (see `layer_contours`/`normal`). Whether
    // `direct` or `cross` is the true continuation flips both with the
    // corner's turn handedness (`la`/`lb`, the directions leaving the joint)
    // and with each axis's own stored point order relative to which end is
    // jointed (`fwd_a`/`fwd_b`, the axis's natural index-0→last direction).
    // Both `cross(la, lb)` and `cross(fwd_a, fwd_b)` individually flip sign
    // when swapping which wall is "self" vs "other" (or when an axis's
    // point order is reversed) — but their *product* does not, matching the
    // fact that "does a1 connect to b1" is a swap-independent physical fact.
    let fwd_a = {
        let (dx, dy) = (
            axis_a[axis_a.len() - 1].0 - axis_a[0].0,
            axis_a[axis_a.len() - 1].1 - axis_a[0].1,
        );
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-12 { (0.0, 0.0) } else { (dx / len, dy / len) }
    };
    let fwd_b = {
        let (dx, dy) = (
            axis_b[axis_b.len() - 1].0 - axis_b[0].0,
            axis_b[axis_b.len() - 1].1 - axis_b[0].1,
        );
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-12 { (0.0, 0.0) } else { (dx / len, dy / len) }
    };
    let cross_fwd = fwd_a.0 * fwd_b.1 - fwd_a.1 * fwd_b.0;
    let invariant = cross_fwd * det;
    let (new_a1, new_a2) = if invariant < 0.0 {
        pair_direct
    } else {
        pair_cross
    };
    Some((new_a1, new_a2))
}

/// Unit direction leaving a wall axis junction.
pub(super) fn is_collinear_opposite_endpoint(a: &JunctionWallGeom, b: &JunctionWallGeom) -> bool {
    let (Some(ea), Some(eb)) = (a.end, b.end) else {
        return false;
    };
    let da = wall_leave_dir(&a.axis, ea);
    let db = wall_leave_dir(&b.axis, eb);
    da.0 * db.0 + da.1 * db.1 < -0.95
}

pub(super) fn wall_leave_dir(axis: &[(f64, f64)], end: usize) -> (f64, f64) {
    if axis.len() < 2 || end >= axis.len() {
        return (0.0, 0.0);
    }
    let prev = if end == 0 { 1 } else { end - 1 };
    let (dx, dy) = (axis[prev].0 - axis[end].0, axis[prev].1 - axis[end].1);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-12 {
        (0.0, 0.0)
    } else {
        (dx / len, dy / len)
    }
}

pub(super) fn rebuild_footprint(
    a_b1: &[(f64, f64)],
    a_b2: &[(f64, f64)],
    end_a: usize,
    new_a1: (f64, f64),
    new_a2: (f64, f64),
) -> Vec<(f64, f64)> {
    let n = a_b1.len();
    let mut footprint = Vec::with_capacity(n * 2);
    for i in 0..n {
        if i == end_a {
            footprint.push(new_a1);
        } else {
            footprint.push(a_b1[i]);
        }
    }
    for i in (0..n).rev() {
        if i == end_a {
            footprint.push(new_a2);
        } else {
            footprint.push(a_b2[i]);
        }
    }
    ensure_ccw(&mut footprint);
    footprint
}

pub(super) fn ensure_ccw(ring: &mut Vec<(f64, f64)>) {
    if super::super::geometry::signed_area(ring) < 0.0 {
        ring.reverse();
    }
}

/// Split a footprint that may contain NaN separators into simple rings.
pub fn split_footprint_rings(fp: &[(f64, f64)]) -> Vec<Vec<(f64, f64)>> {
    let mut rings = Vec::new();
    let mut cur = Vec::new();
    for &p in fp {
        if !p.0.is_finite() || !p.1.is_finite() {
            if cur.len() >= 3 {
                rings.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        } else {
            cur.push(p);
        }
    }
    if cur.len() >= 3 {
        rings.push(cur);
    }
    if rings.is_empty() && fp.len() >= 3 && fp.iter().all(|p| p.0.is_finite() && p.1.is_finite()) {
        rings.push(fp.to_vec());
    }
    rings
}

/// Build a closed footprint for one layer of wall A, with the joined end
/// replaced by the diagonal miter against wall B's matching layer.
///
/// When `end_b` is `None` (T through-wall partner), uses straight butt
/// extension to the near face of B's layer instead of a diagonal miter.
pub(super) fn miter_one_layer(
    a_b1: &[(f64, f64)],
    a_b2: &[(f64, f64)],
    end_a: usize,
    b_b1: &[(f64, f64)],
    b_b2: &[(f64, f64)],
    end_b: Option<usize>,
    axis_a: &[(f64, f64)],
    axis_b: &[(f64, f64)],
) -> Option<Vec<(f64, f64)>> {
    if end_b.is_none() {
        let target = near_face_line(a_b1, a_b2, end_a, b_b1, b_b2, None, axis_a, axis_b)?;
        return t_extend_one_layer(a_b1, a_b2, end_a, target);
    }
    let (new_a1, new_a2) =
        miter_end_points(a_b1, a_b2, end_a, b_b1, b_b2, end_b, axis_a, axis_b)?;
    let mut fp = rebuild_footprint(a_b1, a_b2, end_a, new_a1, new_a2);
    if end_a < axis_a.len() && end_a < a_b1.len() {
        let prev = if end_a == 0 { 1 } else { end_a - 1 };
        let keep = if prev < a_b1.len() {
            a_b1[prev]
        } else {
            axis_a[if end_a == 0 { 1 } else { end_a - 1 }]
        };
        let clipped = clip_closed_to_halfplane(&fp, new_a1, new_a2, keep);
        if clipped.len() >= 3 {
            fp = clipped;
        }
    }
    Some(fp)
}
