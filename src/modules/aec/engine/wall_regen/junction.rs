use super::*;

/// Margin for the local junction scan around one wall. Junction detection
/// itself only uses `JUNCTION_TOLERANCE` / `END_MID_TOLERANCE`; the snap
/// radius is a generous, still local, safety margin.
pub(super) const LOCAL_JUNCTION_SCAN_MARGIN: f64 = WALL_JOIN_SNAP_RADIUS;

/// Local candidate set for the span-junction scans of `wall_handle`: the wall
/// itself plus every wall whose (bulge-aware) segment boxes come within
/// [`LOCAL_JUNCTION_SCAN_MARGIN`] of it. Handles keep the global document
/// order of [`all_wall_axis_handles`] so index-based tie-breaks (e.g. X
/// crossing priority in [`solve_x_pair_footprints`]) resolve exactly as a
/// whole-document scan would. `None` when no other wall is nearby — the common
/// case for an unjoined wall, which then skips detection entirely.
pub(crate) struct LocalJunctionScan {
    pub(crate) handles: Vec<Handle>,
    pub(crate) self_i: usize,
    pub(crate) axes: Vec<Vec<DVec3>>,
    pub(crate) bulges: Vec<Vec<f64>>,
}

pub(crate) fn local_junction_scan(
    scene: &Scene,
    wall_handle: Handle,
    self_axis_2d: &[(f64, f64)],
) -> Option<LocalJunctionScan> {
    let self_axis: Vec<DVec3> = self_axis_2d
        .iter()
        .map(|(x, y)| DVec3::new(*x, *y, 0.0))
        .collect();
    let self_bulges = get_wall_bulges(scene, wall_handle);
    // The regen axis may differ from the stored one (corner override /
    // trimmed preview); cover both so no stored-axis junction is missed.
    let mut self_boxes = segment_aabbs_with_bulges(&self_axis, &self_bulges);
    self_boxes.extend(segment_aabbs_with_bulges(
        &get_wall_vertices(scene, wall_handle),
        &self_bulges,
    ));
    let self_aabb = union_aabbs(&self_boxes);

    let mut candidates = Vec::new();
    for h in all_wall_axis_handles(scene) {
        let axis = get_wall_vertices(scene, h);
        let b = get_wall_bulges(scene, h);
        if h != wall_handle {
            if axis.len() < 2 {
                continue;
            }
            let boxes = segment_aabbs_with_bulges(&axis, &b);
            if !segment_boxes_overlap(self_aabb, &self_boxes, &boxes, LOCAL_JUNCTION_SCAN_MARGIN) {
                continue;
            }
        }
        candidates.push((h, axis, b));
    }
    // Document entity iteration can change as derived wall children are
    // rebuilt. Sort by stable axis handle so equal-priority X crossings keep
    // the same through wall across representation-only regenerations.
    candidates.sort_by_key(|(h, _, _)| h.value());
    let self_i = candidates.iter().position(|(h, _, _)| *h == wall_handle)?;
    if candidates.len() < 2 {
        return None;
    }
    let mut handles = Vec::with_capacity(candidates.len());
    let mut axes = Vec::with_capacity(candidates.len());
    let mut bulges = Vec::with_capacity(candidates.len());
    for (handle, axis, wall_bulges) in candidates {
        handles.push(handle);
        axes.push(axis);
        bulges.push(wall_bulges);
    }
    Some(LocalJunctionScan {
        handles,
        self_i,
        axes,
        bulges,
    })
}

pub(crate) fn find_other_end_junction_footprints(
    scene: &mut Scene,
    wall_handle: Handle,
    self_axis_2d: &[(f64, f64)],
    handled_end: usize,
) -> Option<Vec<Option<Vec<(f64, f64)>>>> {
    if self_axis_2d.len() < 2 {
        return None;
    }
    // `handled_end` is the axis vertex we want footprints for (the caller
    // already skipped the end covered by this regen's primary join).
    let other_end = handled_end;
    if other_end >= self_axis_2d.len() {
        return None;
    }
    let pt = self_axis_2d[other_end];
    let tol = join::JUNCTION_TOLERANCE.max(1e-4);

    // Participants: this wall plus every currently-linked peer whose axis is
    // actually coincident (endpoint or through-span) with this wall's other
    // end. `handles[0]` is always `wall_handle`.
    let mut handles = vec![wall_handle];
    let candidates = engine::owner_index::peers_of(&scene.document, wall_handle);
    for peer in candidates {
        if peer != wall_handle && !handles.contains(&peer) {
            handles.push(peer);
        }
    }
    let nearby = nearby_wall_axis_handles(scene, wall_handle, self_axis_2d, WALL_JOIN_SNAP_RADIUS);
    for peer in nearby {
        if peer == wall_handle || handles.contains(&peer) {
            continue;
        }
        let peer_axis = get_wall_vertices(scene, peer);
        if peer_axis.len() < 2 {
            continue;
        }
        let end_hit = (peer_axis[0].x - pt.0).hypot(peer_axis[0].y - pt.1) <= tol
            || (peer_axis.last().unwrap().x - pt.0).hypot(peer_axis.last().unwrap().y - pt.1) <= tol;
        let through_hit = (0..peer_axis.len() - 1).any(|i| {
            let pt3 = DVec3::new(pt.0, pt.1, peer_axis[i].z);
            point_to_segment_dist_2d(pt3, peer_axis[i], peer_axis[i + 1]) <= WALL_JOIN_SNAP_RADIUS
                && (peer_axis[i].x - pt.0).hypot(peer_axis[i].y - pt.1) > join::END_MID_TOLERANCE
                && (peer_axis[i + 1].x - pt.0).hypot(peer_axis[i + 1].y - pt.1) > join::END_MID_TOLERANCE
        });
        if end_hit || through_hit {
            handles.push(peer);
        }
    }
    if handles.len() < 2 {
        return None;
    }

    let inputs: Vec<super::super::junction_solver::WallJoinInput> = handles
        .iter()
        .map(|h| super::super::join_ops::wall_join_input_from_scene(scene, *h))
        .collect();
    if inputs.iter().any(|w| w.axis.len() < 2) {
        return None;
    }
    let solved = super::super::junction_solver::solve(&inputs, tol);
    let sj = solved
        .into_iter()
        .find(|s| (s.junction.point.x - pt.0).hypot(s.junction.point.y - pt.1) <= tol.max(1e-3))?;

    // Confirm wall 0 actually participates at other_end
    let pi = sj
        .junction
        .participants
        .iter()
        .position(|p| p.wall_index == 0)?;
    if !matches!(sj.junction.participants[pi].role, join::JunctionRole::Endpoint(e) if e == other_end) {
        return None;
    }

    sj.footprints.get(0).cloned()
}

/// T-junction cutouts when this wall is the through (head) wall: the join
/// sits on a span, not an axis end, so [`find_other_end_junction_footprints`]
/// never sees it. Without this, plan-type / 2D-3D regen drops pockets.
#[cfg(test)]
pub(crate) fn find_through_span_cutout_footprints(
    scene: &mut Scene,
    wall_handle: Handle,
    self_axis_2d: &[(f64, f64)],
) -> Option<Vec<Option<Vec<(f64, f64)>>>> {
    find_through_span_cutout_footprints_seeded(scene, wall_handle, self_axis_2d, None)
}

pub(crate) fn find_through_span_cutout_footprints_seeded(
    scene: &mut Scene,
    wall_handle: Handle,
    self_axis_2d: &[(f64, f64)],
    seed: Option<&[Option<Vec<(f64, f64)>>]>,
) -> Option<Vec<Option<Vec<(f64, f64)>>>> {
    if self_axis_2d.len() < 2 {
        return None;
    }
    let LocalJunctionScan {
        handles,
        self_i,
        axes,
        bulges,
    } = local_junction_scan(scene, wall_handle, self_axis_2d)?;
    if axes.iter().any(|a| a.len() < 2) {
        return None;
    }
    let axis_refs: Vec<&[DVec3]> = axes.iter().map(|a| a.as_slice()).collect();
    let bulge_refs: Vec<&[f64]> = bulges.iter().map(|b| b.as_slice()).collect();
    let tol = join::JUNCTION_TOLERANCE.max(1e-4);
    let junctions = join::detect_junctions_with_bulges(&axis_refs, &bulge_refs, tol);
    let through_layers = wall_layer_data(scene, wall_handle);
    let through_bulges = get_wall_bulges(scene, wall_handle);
    let span_gaps = read_junction_override(scene, wall_handle, THROUGH_SPAN_OVERRIDE_END)
        .map(|ov| ov.layer_gaps)
        .unwrap_or_default();
    let end0_gaps = read_junction_override(scene, wall_handle, 0)
        .map(|ov| ov.layer_gaps)
        .unwrap_or_default();
    let end1_gaps = read_junction_override(
        scene,
        wall_handle,
        self_axis_2d.len().saturating_sub(1),
    )
    .map(|ov| ov.layer_gaps)
    .unwrap_or_default();

    let inputs: Vec<super::super::junction_solver::WallJoinInput> = handles
        .iter()
        .map(|h| super::super::join_ops::wall_join_input_from_scene(scene, *h))
        .collect();

    let mut merged: Vec<Option<Vec<(f64, f64)>>> = match seed {
        Some(s) => {
            let mut v = s.to_vec();
            v.resize(through_layers.len(), None);
            v
        }
        None => vec![None; through_layers.len()],
    };
    let mut any = false;
    for junc in &junctions {
        let is_through = junc.participants.iter().any(|p| {
            p.wall_index == self_i && matches!(p.role, join::JunctionRole::Through(_))
        });
        if !is_through {
            continue;
        }

        // Case 1: 2-Through X crossing
        if junc.participants.len() == 2
            && junc
                .participants
                .iter()
                .all(|p| matches!(p.role, join::JunctionRole::Through(_)))
        {
            let other_p = junc.participants.iter().find(|p| p.wall_index != self_i).unwrap();
            let other_i = other_p.wall_index;
            if let Some((sj_fps, self_is_priority)) =
                solve_x_pair_footprints(&inputs, self_i, other_i, junc.point, tol)
            {
                if self_is_priority {
                    if axes[self_i].len() > 2 {
                        // The pair solver's full-axis footprint cannot be
                        // combined with a second crossing via half-plane
                        // clipping. Use only the intersected segment's two
                        // notch chords, retaining every other leg and any
                        // already resolved endpoint miter in `merged`.
                        let cuts = super::super::junction_solver::x_priority_local_cutouts(
                            &inputs, junc, self_i, other_i,
                        );
                        let offsets: Vec<_> = through_layers.iter().map(|l| (l.thickness, l.axis_offset)).collect();
                        let raw = engine::contour::layer_contours_with_bulges(
                            self_axis_2d, &through_bulges, &offsets,
                        );
                        for cut in &cuts {
                            for (i, fp) in cut.iter().enumerate() {
                                let Some(cut_fp) = fp else { continue };
                                let Some((b1, b2)) = raw.get(i) else { continue };
                                let base = engine::contour::closed_layer_footprint(b1, b2).points;
                                any = true;
                                merged[i] = Some(engine::miter::merge_crossing_stems_local(
                                    merged[i].as_deref().or(Some(base.as_slice())), cut_fp,
                                ));
                            }
                        }
                        continue;
                    }
                    for (i, fp) in sj_fps.iter().enumerate() {
                        if let Some(cut_fp) = fp {
                            any = true;
                            if i >= merged.len() {
                                merged.resize(i + 1, None);
                            }
                            merged[i] = Some(match merged[i].as_deref() {
                                Some(prev) => engine::miter::merge_through_cutout_preserving_miter(
                                    Some(prev),
                                    cut_fp,
                                ),
                                None => cut_fp.clone(),
                            });
                        }
                    }
                }
            }
            continue;
        }

        // Case 2: T or N-Way with Endpoint stem(s)
        for stem in junc
            .participants
            .iter()
            .filter(|p| matches!(p.role, join::JunctionRole::Endpoint(_)))
        {
            let join::JunctionRole::Endpoint(stem_end) = stem.role else {
                continue;
            };
            let stem_handle = handles[stem.wall_index];
            let stem_axis: Vec<(f64, f64)> = axes[stem.wall_index]
                .iter()
                .map(|p| (p.x, p.y))
                .collect();
            let stem_layers = wall_layer_data(scene, stem_handle);
            let mut gaps = span_gaps.clone();
            gaps.extend(end0_gaps.iter().cloned());
            gaps.extend(end1_gaps.iter().cloned());
            if let Some(ov) = read_junction_override(scene, stem_handle, stem_end) {
                gaps.extend(ov.layer_gaps);
            }
            let cut = engine::miter::through_wall_cutout_footprints_with_gaps(
                self_axis_2d,
                &through_layers,
                &stem_axis,
                &stem_layers,
                stem_end,
                &through_bulges,
                &get_wall_bulges(scene, stem_handle),
                &gaps,
            );
            for (i, fp) in cut.into_iter().enumerate() {
                if let Some(cut_fp) = fp {
                    any = true;
                    if i >= merged.len() {
                        merged.resize(i + 1, None);
                    }
                    merged[i] = Some(match merged[i].as_deref() {
                        Some(prev) => {
                            engine::miter::merge_through_cutout_preserving_miter(Some(prev), &cut_fp)
                        }
                        None => cut_fp,
                    });
                }
            }
        }
    }
    any.then_some(merged)
}

/// Solve the 2-wall X crossing at `point` between `self_i` and `other_i` and
/// return `(footprints of self, self_is_priority)`. The pair is always solved
/// in ascending wall-index order so equal-priority ties resolve the same way
/// for both walls. `None` when no X is found there or the crossing is
/// unsupported (either wall has a curved segment).
pub(super) fn solve_x_pair_footprints(
    inputs: &[super::super::junction_solver::WallJoinInput],
    self_i: usize,
    other_i: usize,
    point: DVec3,
    tol: f64,
) -> Option<(Vec<Option<Vec<(f64, f64)>>>, bool)> {
    let (lo, hi) = if self_i <= other_i {
        (self_i, other_i)
    } else {
        (other_i, self_i)
    };
    let pair = [inputs[lo].clone(), inputs[hi].clone()];
    let solved = super::super::junction_solver::solve(&pair, tol);
    let sj = solved
        .iter()
        .find(|s| s.junction.point.distance(point) < tol)?;
    let (pri_local, _) = super::super::junction_solver::x_priority_pair(&pair, &sj.junction)?;
    let self_local = if self_i == lo { 0 } else { 1 };
    Some((sj.footprints.get(self_local)?.clone(), pri_local == self_local))
}

pub(crate) fn find_crossing_span_footprints_seeded(
    scene: &mut Scene,
    wall_handle: Handle,
    self_axis_2d: &[(f64, f64)],
    seed: Option<&[Option<Vec<(f64, f64)>>]>,
) -> Option<Vec<Option<Vec<(f64, f64)>>>> {
    if self_axis_2d.len() < 2 {
        return None;
    }
    let LocalJunctionScan {
        handles,
        self_i,
        axes,
        bulges,
    } = local_junction_scan(scene, wall_handle, self_axis_2d)?;
    if axes.iter().any(|a| a.len() < 2) {
        return None;
    }
    let axis_refs: Vec<&[DVec3]> = axes.iter().map(|a| a.as_slice()).collect();
    let bulge_refs: Vec<&[f64]> = bulges.iter().map(|b| b.as_slice()).collect();
    let tol = join::JUNCTION_TOLERANCE.max(1e-4);
    let junctions = join::detect_junctions_with_bulges(&axis_refs, &bulge_refs, tol);
    let inputs: Vec<super::super::junction_solver::WallJoinInput> = handles
        .iter()
        .map(|h| super::super::join_ops::wall_join_input_from_scene(scene, *h))
        .collect();

    let mut merged: Vec<Option<Vec<(f64, f64)>>> = match seed {
        Some(s) => {
            let mut v = s.to_vec();
            v.resize(inputs[self_i].layers.len(), None);
            v
        }
        None => vec![None; inputs[self_i].layers.len()],
    };
    let mut any = false;

    for junc in &junctions {
        let is_through = junc.participants.iter().any(|p| {
            p.wall_index == self_i && matches!(p.role, join::JunctionRole::Through(_))
        });
        if !is_through {
            continue;
        }
        if junc.participants.len() == 2
            && junc
                .participants
                .iter()
                .all(|p| matches!(p.role, join::JunctionRole::Through(_)))
        {
            let other_p = junc.participants.iter().find(|p| p.wall_index != self_i).unwrap();
            let other_i = other_p.wall_index;
            if let Some((sj_fps, self_is_priority)) =
                solve_x_pair_footprints(&inputs, self_i, other_i, junc.point, tol)
            {
                if !self_is_priority {
                    for (i, fp) in sj_fps.iter().enumerate() {
                        if let Some(c_fp) = fp {
                            any = true;
                            if i >= merged.len() {
                                merged.resize(i + 1, None);
                            }
                            // Finite-chord merge: a polyline crossing wall
                            // keeps every leg that merely passes the notch of
                            // another crossing.
                            merged[i] = Some(engine::miter::merge_crossing_stems_local(
                                merged[i].as_deref(),
                                c_fp,
                            ));
                        }
                    }
                }
            }
        }
    }

    any.then_some(merged)
}

pub(super) fn fill_missing_opening_grids(
    openings: &mut [engine::openings::Opening],
    library: &StyleLibrary,
) {
    for opening in openings {
        if opening.division_override.is_some() {
            continue;
        }
        let fallback = engine::opening_style::default_style_for_kind(opening.kind);
        let style_id = opening
            .style_id
            .as_deref()
            .unwrap_or(&fallback.style.id);
        let style = library
            .find_opening_style(style_id)
            .or_else(|| library.find_opening_style(&fallback.style.id));
        opening.division_override = Some(
            style
                .map(|style| style.grid.clone())
                .unwrap_or(fallback.grid),
        );
    }
}
