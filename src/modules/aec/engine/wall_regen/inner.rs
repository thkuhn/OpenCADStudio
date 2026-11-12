use super::*;

pub(crate) fn regenerate_wall_representation_inner(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    join_miter: Option<&engine::miter::JoinMiterContext>,
    precomputed_miters: Option<&[Option<Vec<(f64, f64)>>]>,
    rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
    library_override: Option<&StyleLibrary>,
    skip_junction_rescan: bool,
) -> Result<Vec<Handle>, WallRegenError> {
    use engine::display_component::{LayerSelection, WallComponentSlot};
    // Geometry vs. hatch are independent: layer contours/hatches vs. overall
    // contour/hatch. Envelope (first/last layer) stands in for overall contour
    // until a dedicated union path exists. Axis/SurfaceStyle3D/Section/Elevation
    // remain no-ops here.
    let layers2d_visible = rules.map_or(true, |r| r.is_visible(WallComponentSlot::Layers2D));
    let contour2d_visible = rules.map_or(true, |r| r.is_visible(WallComponentSlot::Contour2D));
    let layer_hatch_visible = rules.map_or(true, |r| r.is_visible(WallComponentSlot::LayerHatch2D));
    let contour_hatch_visible =
        rules.map_or(true, |r| r.is_visible(WallComponentSlot::ContourHatch2D));
    let solid_visible = rules.map_or(true, |r| r.is_visible(WallComponentSlot::Solid3D));
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return Err(WallRegenError::NotAWall);
    };

    let Some(wall) = wall_from_entity(entity) else {
        return Err(WallRegenError::NotAWall);
    };
    let (wall_owner_handle, xref_layer_prefix) = wall_owner_and_layer_prefix(scene, wall_handle);
    let (layers, height, wall_base_z, _old_derived, wall_style_id, wall_hatch_override) = (
        wall.layers.clone(),
        wall.height,
        wall.base_origin[2],
        wall.derived_handles.clone(),
        wall.style_id.clone(),
        wall.hatch_override.clone(),
    );
    let wall_hatch_override = wall_hatch_override.as_ref();

    if layers.is_empty() {
        return Err(WallRegenError::NoLayers);
    }

    // Keep existing contour polylines so their tessellation handles stay
    // valid (delete+recreate left the old outline on screen). Hatches and
    // solids are still replaced; leftover contours are erased after reuse.
    let stale = collect_wall_display_children(scene, wall_handle);
    let mut reusable_contours: Vec<Handle> = Vec::new();
    let mut erase_now: Vec<Handle> = Vec::new();
    for h in stale {
        match scene.document.get_entity(h) {
            Some(EntityType::LwPolyline(_)) => reusable_contours.push(h),
            _ => erase_now.push(h),
        }
    }
    if !erase_now.is_empty() {
        scene.erase_entities(&erase_now);
    }

    // The axis is reference geometry on its own layer. Idle visibility follows
    // the AxisLine slot when rules are present; otherwise the layer stays off.
    set_wall_axis_layer_visible(scene, axis_visible_from_rules(rules));
    let axis_layer = prefix_layer(AEC_WALL_AXIS_LAYER, xref_layer_prefix.as_deref());
    scene.ensure_layer(&axis_layer);
    if let Some(e) = scene.document.get_entity_mut(wall_handle) {
        e.as_entity_mut().set_layer(axis_layer);
        if let EntityType::LwPolyline(pl) = e {
            pl.elevation = wall_base_z;
        }
    }
    scene.bump_entities(&[(wall_handle, crate::scene::ChangeKind::Modified)]);

    let axis_entity = scene
        .document
        .get_entity(wall_handle)
        .cloned()
        .ok_or(WallRegenError::NotAWall)?;

    // Base footprints from the true (persisted) axis via the shared
    // WallRepresentation builder. Corner extension is only applied as a
    // per-layer fallback when a join miter can't match layers.
    let base_footprints_with_bulges = wall_layer_footprints_with_bulges(&axis_entity, &layers);
    if base_footprints_with_bulges.is_empty() {
        if !reusable_contours.is_empty() {
            scene.erase_entities(&reusable_contours);
        }
        let _ = set_wall_derived_handles(scene, wall_handle, &[]);
        return Err(WallRegenError::NoContours);
    }
    let base_footprints: Vec<Vec<(f64, f64)>> = base_footprints_with_bulges
        .iter()
        .map(|(pts, _)| pts.clone())
        .collect();
    let base_footprint_bulges: Vec<Vec<f64>> = base_footprints_with_bulges
        .iter()
        .map(|(_, b)| b.clone())
        .collect();

    let owned_library;
    let library: &StyleLibrary = match library_override {
        Some(lib) => lib,
        None => {
            owned_library = load_or_seed();
            &owned_library
        }
    };

    // Load host openings independently of the 2D cut slot: rest-wall solids
    // and opening-zone solids always use them, while contours/hatches only
    // split when `HostCut2D` is visible.
    let host_cut_visible = rules.map_or(true, |r| {
        r.is_opening_visible(engine::display_component::OpeningComponentSlot::HostCut2D)
    });
    let mut wall_openings = openings_for_host_wall(scene, wall_handle);
    fill_missing_opening_grids(&mut wall_openings, library);
    let contour_openings = if host_cut_visible {
        wall_openings.as_slice()
    } else {
        &[]
    };
    let (centerline, bulges) = wall_axis_points_and_bulges(&axis_entity);
    let layer_data: Vec<(f64, f64)> =
        layers.iter().map(|l| (l.thickness, l.axis_offset)).collect();
    let layer_extrusion: Vec<(f64, f64)> = layers
        .iter()
        .map(|l| {
            (
                (height + l.top_offset - l.bottom_offset).max(0.0),
                l.bottom_offset,
            )
        })
        .collect();
    let display = if wall.is_sloped() {
        let z_bounds = |li: usize, s: f64| {
            if let Some((pt, _)) = engine::openings::point_and_tangent_at_distance(&centerline, s) {
                if let Some(l) = layers.get(li) {
                    let zb = wall.layer_base_z_at_xy(l, pt.0, pt.1) - wall_base_z;
                    let zt = wall.layer_top_z_at_xy(l, pt.0, pt.1) - wall_base_z;
                    (zb, zt)
                } else {
                    let zb = wall.base_z_at_xy(pt.0, pt.1) - wall_base_z;
                    let zt = wall.top_z_at_xy(pt.0, pt.1) - wall_base_z;
                    (zb, zt)
                }
            } else if let Some(&(h, b)) = layer_extrusion.get(li) {
                (b, b + h)
            } else {
                (0.0, height)
            }
        };
        engine::representation::build_wall_display_set_sloped(
            &centerline,
            &bulges,
            &layer_data,
            0.0,
            &wall_openings,
            &layer_extrusion,
            z_bounds,
        )
    } else {
        engine::representation::build_wall_display_set(
            &centerline,
            &bulges,
            &layer_data,
            0.0,
            &wall_openings,
            &layer_extrusion,
        )
    };

    // Fallback footprints: axis with a single vertex pushed past the join into
    // the other wall's footprint (legacy corner-overlap path).
    let mut extended_axis_entity = axis_entity.clone();
    let has_corner_override = if let (Some((idx, pos)), EntityType::LwPolyline(pl)) =
        (corner_override, &mut extended_axis_entity)
    {
        if let Some(v) = pl.vertices.get_mut(idx) {
            v.location = Vector2::new(pos.x, pos.y);
            true
        } else {
            false
        }
    } else {
        false
    };
    let extended_footprints = if has_corner_override {
        wall_layer_footprints(&extended_axis_entity, &layers)
    } else {
        Vec::new()
    };

    // Self axis as plain 2D points for the miter helper.
    let self_axis_2d: Vec<(f64, f64)> = match &axis_entity {
        EntityType::LwPolyline(pl) => pl
            .vertices
            .iter()
            .map(|v| (v.location.x, v.location.y))
            .collect(),
        _ => Vec::new(),
    };
    let self_layer_data: Vec<engine::miter::MiterLayer> = layers
        .iter()
        .map(|l| {
            engine::miter::MiterLayer::with_id(
                l.thickness,
                l.axis_offset,
                l.material.clone(),
                l.function.clone(),
                l.layer_id,
            )
        })
        .collect();

    // Shared helper: build per-layer mitered footprints for one end's join
    // context, honoring any persisted `JunctionOverride` for that end.
    let compute_end_footprints = |scene: &mut Scene, ctx: &engine::miter::JoinMiterContext| {
        if ctx.as_through {
            let stem_end = ctx.other_end.unwrap_or(ctx.self_end);
            let mut gaps = Vec::new();
            if let Some(ov) = read_junction_override(scene, wall_handle, THROUGH_SPAN_OVERRIDE_END)
            {
                gaps.extend(ov.layer_gaps);
            }
            return engine::miter::through_wall_cutout_footprints_with_gaps(
                &self_axis_2d,
                &self_layer_data,
                &ctx.other_axis,
                &ctx.other_layers,
                stem_end,
                &ctx.self_bulges,
                &ctx.other_bulges,
                &gaps,
            );
        }
        let self_layer_refs: Vec<join::LayerRef> =
            layer_refs_from_materials(layers.iter().map(|l| (l.material.as_str(), l.layer_id)));
        let junction_override = read_junction_override(scene, wall_handle, ctx.self_end)
            .and_then(|ov| {
                let other_layer_refs: Vec<join::LayerRef> = layer_refs_from_materials(
                    ctx.other_layers.iter().map(|l| (l.material.as_str(), l.layer_id)),
                );
                validate_and_persist_junction_override(
                    scene,
                    wall_handle,
                    ctx.self_end,
                    ov,
                    &self_layer_refs,
                    &other_layer_refs,
                )
            });
        engine::miter::mitered_layer_footprints_with_override_and_bulges(
            &self_axis_2d,
            &self_layer_data,
            &self_layer_refs,
            ctx.self_end,
            &ctx.other_axis,
            &ctx.other_layers,
            ctx.other_end,
            ctx.kind,
            junction_override.as_ref(),
            &ctx.self_bulges,
            &ctx.other_bulges,
        )
    };

    // Pre-compute per-layer mitered footprints when a join context is present,
    // or use caller-supplied N-way junction footprints.
    let mut mitered_footprints: Vec<Option<Vec<(f64, f64)>>> =
        if let Some(pre) = precomputed_miters {
            let mut v = pre.to_vec();
            v.resize(layers.len(), None);
            v
        } else if let Some(ctx) = join_miter {
            compute_end_footprints(scene, ctx)
        } else {
            vec![None; layers.len()]
        };

    // Whichever end this call's `join_miter` handled (if any) shouldn't be
    // re-derived below; every other axis end that currently has an
    // established peer join must also be reflected here, or that end's
    // rendering would revert to a plain unjoined cap whenever this wall is
    // regenerated for a *different* join event (see module-level bug notes
    // on `find_other_end_join_miter`).
    let mut peer_at_end0: Option<(Vec<engine::miter::MiterLayer>, Option<usize>)> = None;
    let mut peer_at_end1: Option<(Vec<engine::miter::MiterLayer>, Option<usize>)> = None;

    if let Some(ctx) = join_miter {
        if ctx.self_end == 0 {
            peer_at_end0 = Some((ctx.other_layers.clone(), ctx.other_end));
        } else {
            peer_at_end1 = Some((ctx.other_layers.clone(), ctx.other_end));
        }
    }

    if skip_junction_rescan && self_axis_2d.len() >= 2 {
        // Peer lookup is O(degree) via owner_index; keep it for hatch matching.
        if peer_at_end0.is_none() {
            peer_at_end0 = find_end_peer_layers(scene, wall_handle, &self_axis_2d, 0);
        }
        if peer_at_end1.is_none() {
            let last = self_axis_2d.len() - 1;
            peer_at_end1 = find_end_peer_layers(scene, wall_handle, &self_axis_2d, last);
        }
    }

    if self_axis_2d.len() >= 2 {
        if !skip_junction_rescan {
            let mut candidate_ends = vec![0usize, self_axis_2d.len() - 1];
            candidate_ends.dedup();
            if let Some(ctx) = join_miter {
                candidate_ends.retain(|&e| e != ctx.self_end);
            }
            for end_idx in candidate_ends {
                if let Some(other_footprints) =
                    find_other_end_junction_footprints(scene, wall_handle, &self_axis_2d, end_idx)
                {
                    if let Some(peer_data) = find_end_peer_layers(scene, wall_handle, &self_axis_2d, end_idx) {
                        if end_idx == 0 {
                            peer_at_end0 = Some(peer_data);
                        } else {
                            peer_at_end1 = Some(peer_data);
                        }
                    }
                    for (i, base_fp) in base_footprints.iter().enumerate() {
                        let merged = engine::miter::merge_end_footprints(
                            base_fp,
                            mitered_footprints.get(i).and_then(|o| o.as_ref()),
                            other_footprints.get(i).and_then(|o| o.as_ref()),
                        );
                        if i < mitered_footprints.len() {
                            mitered_footprints[i] = merged;
                        }
                    }
                }
            }
        }
        // Merge each 2-ring crossing/cutout into the already-mitered slot
        // sequentially. Combining an N-ring cutout with an L-miter in one
        // shot used to clip middle rings on only one notch. Network-complete
        // miters already include every span cutout from solve_network.
        if !skip_junction_rescan {
            if let Some(crossing_fps) = find_crossing_span_footprints_seeded(
                scene,
                wall_handle,
                &self_axis_2d,
                Some(mitered_footprints.as_slice()),
            ) {
                for (i, fp) in crossing_fps.into_iter().enumerate() {
                    if let Some(c_fp) = fp {
                        if i < mitered_footprints.len() {
                            mitered_footprints[i] = Some(c_fp);
                        }
                    }
                }
            }
            if let Some(cut) = find_through_span_cutout_footprints_seeded(
                scene,
                wall_handle,
                &self_axis_2d,
                Some(mitered_footprints.as_slice()),
            ) {
                for (i, fp) in cut.into_iter().enumerate() {
                    if let Some(cut_fp) = fp {
                        if i < mitered_footprints.len() {
                            mitered_footprints[i] = Some(cut_fp);
                        }
                    }
                }
            }
        }
    }

    // Extrusion height/base come from the (possibly extended) axis so solids
    // stay consistent with the 2D footprint chosen per layer below.
    let extrusion_axis = if has_corner_override {
        &extended_axis_entity
    } else {
        &axis_entity
    };
    let extrusions = wall_layer_extrusions(extrusion_axis, &layers, height);

    // `StyleSubstitution`: when the wall's own style has a target entry,
    // the target wall style's layers become the *style* source (material,
    // and therefore hatch/color) for the corresponding layer index, while
    // axis/thickness/layer count always stay derived from `layers` above —
    // only the style-lookup material id per layer changes. Layer count
    // mismatches (fewer target layers than source) simply leave the
    // remaining layers on their own original material (no substitution for
    // those indices), matching the plan's precedence chain (c falls back
    // to (d)/(e) for anything the substitution can't resolve).
    let substituted_layer_styles: Option<Vec<(String, Option<String>)>> =
        style_substitutions.and_then(|subs| {
            subs.get(&wall_style_id).and_then(|target_id| {
                library
                    .wall_styles
                    .iter()
                    .find(|s| &s.style.id == target_id)
                    .map(|s| {
                        s.layers
                            .iter()
                            .map(|l| (l.material_id.clone(), l.hatch_override.clone()))
                            .collect()
                    })
            })
        });

    // Overall wall run direction (radians), used as the base angle for
    // materials whose hatch angle is relative to the wall instead of a
    // fixed/global angle.
    let wall_angle_rad = {
        let first = centerline.first().copied();
        let last = centerline.last().copied();
        match (first, last) {
            (Some((x0, y0)), Some((x1, y1))) if (x1 - x0).abs() > 1e-9 || (y1 - y0).abs() > 1e-9 => {
                (y1 - y0).atan2(x1 - x0)
            }
            _ => 0.0,
        }
    };

    let mut new_derived: Vec<Handle> = Vec::new();
    for (i, layer) in layers.iter().enumerate() {
        let mat_name = &layer.material;
        // Prefer a true per-layer miter when the join helper could match this
        // layer index against the other wall; otherwise fall back to the
        // corner-extended footprint (or the plain base footprint) from
        // WallRepresentation.
        // Prefer a true per-layer miter / corner-extended footprint when
        // available (those paths are still straight-only). Otherwise use the
        // bulge-aware base footprint so curved axes keep exact offset arcs.
        // With openings and no miter/corner override, emit one 2D contour+hatch
        // per disconnected piece (through-cut splits the band).
        let uncut_footprint: (Vec<(f64, f64)>, Vec<f64>) =
            if let Some(Some(mitered)) = mitered_footprints.get(i) {
                let bg = retarget_closed_footprint_bulges(
                    &base_footprints[i],
                    base_footprint_bulges.get(i).map(|b| b.as_slice()).unwrap_or(&[]),
                    mitered,
                );
                (mitered.clone(), bg)
            } else if let Some(fp) = extended_footprints.get(i) {
                let bg = retarget_closed_footprint_bulges(
                    &base_footprints[i],
                    base_footprint_bulges.get(i).map(|b| b.as_slice()).unwrap_or(&[]),
                    fp,
                );
                (fp.clone(), bg)
            } else {
                (
                    base_footprints[i].clone(),
                    base_footprint_bulges
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| vec![0.0; base_footprints[i].len()]),
                )
            };

        let pieces_for_openings = |openings: &[engine::openings::Opening]| {
            if !openings.is_empty() {
                engine::miter::split_mitered_footprint_by_openings(
                    &uncut_footprint.0,
                    &centerline,
                    openings,
                )
                .into_iter()
                .map(|piece| {
                    let bulges = if uncut_footprint.1.is_empty()
                        || uncut_footprint.1.iter().all(|&bulge| bulge.abs() < 1e-12)
                    {
                        vec![0.0; piece.len()]
                    } else {
                        retarget_closed_footprint_bulges(
                            &base_footprints[i],
                            base_footprint_bulges.get(i).map(|b| b.as_slice()).unwrap_or(&[]),
                            &piece,
                        )
                    };
                    (piece, bulges)
                })
                .collect()
            } else {
                let rings = engine::miter::split_footprint_rings(&uncut_footprint.0);
                if rings.len() > 1 {
                    rings
                        .into_iter()
                        .map(|ring| {
                            let n = ring.len();
                            (ring, vec![0.0; n])
                        })
                        .collect()
                } else if !uncut_footprint.0.is_empty() {
                    vec![uncut_footprint.clone()]
                } else {
                    Vec::new()
                }
            }
        };
        let pieces_2d = pieces_for_openings(contour_openings);
        let pieces_3d = if host_cut_visible {
            pieces_2d.clone()
        } else {
            pieces_for_openings(&wall_openings)
        };

        // Stable per-layer identity, used to match `layer_filter` /
        // `layer_style_override` entries against this layer (mirrors
        // `layer_refs_from_materials`'s convention: `WallLayer` carries no
        // `role_tag`, so it's always `None` here).
        let layer_ref = join::LayerRef {
            material_id: mat_name.clone(),
            role_tag: None,
            index: i,
            layer_id: Some(layer.layer_id),
        };
        // `layer_filter_for(slot)` gates `Contour2D`/`Solid3D` independently
        // (Step 2/3): the 2D contour below consults the `Contour2D` slot,
        // while the 3D solid further down consults `Solid3D` separately —
        // so e.g. a 5-layer wall can show only the masonry layer as its 2D
        // contour while still extruding every layer's solid. `All` (or no
        // rules) keeps every layer, exactly like before this feature
        // existed.
        let layer_included_contour = match rules.map(|r| r.layer_filter_for(WallComponentSlot::Contour2D)) {
            Some(LayerSelection::Explicit(refs)) => layer_ref_matches(&layer_ref, refs),
            _ => true,
        };
        let layer_included_layers2d = match rules.map(|r| r.layer_filter_for(WallComponentSlot::Layers2D)) {
            Some(LayerSelection::Explicit(refs)) => layer_ref_matches(&layer_ref, refs),
            _ => true,
        };
        let layer_included_hatch = match rules.map(|r| r.layer_filter_for(WallComponentSlot::LayerHatch2D)) {
            Some(LayerSelection::Explicit(refs)) => layer_ref_matches(&layer_ref, refs),
            _ => true,
        };
        let layer_included_solid = match rules.map(|r| r.layer_filter_for(WallComponentSlot::Solid3D)) {
            Some(LayerSelection::Explicit(refs)) => layer_ref_matches(&layer_ref, refs),
            _ => true,
        };

        // `StyleSubstitution` (precedence tier c): swap the *style* source
        // (material id + its own hatch override) for this layer index from
        // the resolved target wall style, while `layer`'s own geometry
        // (thickness/gaps/offsets) is untouched. Falls back to this layer's
        // own material/hatch_override when the target has no layer at this
        // index (or no substitution applies at all).
        let (effective_mat_name, effective_hatch_override): (&str, Option<&str>) =
            match substituted_layer_styles.as_ref().and_then(|v| v.get(i)) {
                Some((sub_mat, sub_hatch)) => (sub_mat.as_str(), sub_hatch.as_deref()),
                None => (mat_name.as_str(), layer.hatch_override.as_deref()),
            };
        let material = library.materials.iter().find(|m| {
            m.id == effective_mat_name || m.name == effective_mat_name
        });

        // Envelope (overall 2D contour) prefers `Contour2D` so phase extras
        // (Abbruch/Bestand) win over per-layer `Layers2D` overrides.
        let is_envelope = i == 0 || i + 1 == layers.len();
        let contour_slots = if is_envelope {
            [WallComponentSlot::Contour2D, WallComponentSlot::Layers2D]
        } else {
            [WallComponentSlot::Layers2D, WallComponentSlot::Contour2D]
        };
        let contour_style_override =
            resolve_layer_style_override(rules, &contour_slots, &layer_ref);
        let hatch_slots = if is_envelope {
            [
                WallComponentSlot::ContourHatch2D,
                WallComponentSlot::LayerHatch2D,
            ]
        } else {
            [
                WallComponentSlot::LayerHatch2D,
                WallComponentSlot::ContourHatch2D,
            ]
        };
        let mut hatch_style_override =
            resolve_layer_style_override(rules, &hatch_slots, &layer_ref);
        if is_envelope {
            if let Some(slot_style) = rules.and_then(|r| {
                r.style_for(WallComponentSlot::ContourHatch2D)
            }) {
                hatch_style_override.overlay_from(slot_style);
            }
        }

        // Layer-level `hatch_override` (Step 4) takes precedence over the
        // material's own `hatch_pattern`; both fall back to "ANSI31" so a
        // layer/material without an explicit pattern still renders a hatch.
        let pattern_name = hatch_style_override
            .hatch_pattern
            .clone()
            .or_else(|| {
                effective_hatch_override
                    .filter(|p| !p.is_empty())
                    .map(|p| p.to_string())
            })
            .or_else(|| material.map(|m| m.hatch_pattern.clone()).filter(|p| !p.is_empty()))
            .unwrap_or_else(|| "ANSI31".to_string());
        let hatch_acad_color = hatch_style_override
            .hatch_color
            .or_else(|| material.and_then(|m| m.hatch_color))
            .or_else(|| material.map(|m| material_line_color_as_acad(m.line_color)));
        let color = hatch_acad_color
            .map(wall_hatch_color)
            .unwrap_or([0.6, 0.6, 0.6, 0.85]);
        let mut hatch_scale = hatch_style_override
            .hatch_scale
            .or_else(|| material.map(|m| m.hatch_scale))
            .unwrap_or(1.0);
        if hatch_scale <= 0.0 {
            hatch_scale = 0.01;
        }
        let hatch_scale = hatch_scale as f32;
        // Hatch direction: either the material's own hatch angle applied on
        // top of the wall's run direction ("relative"), or used verbatim as
        // a fixed/global angle. Both are stored in degrees on `Material` and
        // converted to the radians `HatchModel::angle_offset` expects.
        // Override precedence: `Wall.hatch_override` (per-instance) >
        // style-profile `hatch_style_override` > `Material` default.
        let hatch_angle_deg = wall_hatch_override
            .and_then(|ov| ov.hatch_angle)
            .or(hatch_style_override.hatch_angle)
            .or_else(|| material.map(|m| m.hatch_angle))
            .unwrap_or(0.0);
        let hatch_angle_relative = wall_hatch_override
            .and_then(|ov| ov.hatch_angle_relative)
            .or(hatch_style_override.hatch_angle_relative)
            .or_else(|| material.map(|m| m.hatch_angle_relative))
            .unwrap_or(true);
        let hatch_angle_offset = if hatch_angle_relative {
            wall_angle_rad + hatch_angle_deg.to_radians()
        } else {
            hatch_angle_deg.to_radians()
        } as f32;
        let line_color = contour_style_override
            .line_color
            .or_else(|| material.map(|m| material_line_color_as_acad(m.line_color)));
        let fill_color = contour_style_override.fill_color;
        let families = crate::scene::model::hatch_patterns::find(&pattern_name)
            .and_then(|e| {
                if let crate::scene::model::hatch_model::HatchPattern::Pattern(f) = &e.gpu {
                    Some(f.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        let has_explicit_contour_filter = matches!(
            rules.map(|r| r.layer_filter_for(WallComponentSlot::Contour2D)),
            Some(LayerSelection::Explicit(_))
        );
        let has_explicit_layers2d_filter = matches!(
            rules.map(|r| r.layer_filter_for(WallComponentSlot::Layers2D)),
            Some(LayerSelection::Explicit(_))
        );

        let same_mat_end0 = layer_has_same_material_peer(layer, i, 0, &layers, peer_at_end0.as_ref());
        let same_mat_end1 = layer_has_same_material_peer(
            layer,
            i,
            self_axis_2d.len().saturating_sub(1),
            &layers,
            peer_at_end1.as_ref(),
        );

        let (p_start, t_start) = if centerline.len() >= 2 {
            let p0 = centerline[0];
            let p1 = centerline[1];
            let dx = p1.0 - p0.0;
            let dy = p1.1 - p0.1;
            let len = (dx * dx + dy * dy).sqrt().max(1e-9);
            (p0, (dx / len, dy / len))
        } else {
            ((0.0, 0.0), (1.0, 0.0))
        };
        let (p_end, t_end) = if centerline.len() >= 2 {
            let p0 = centerline[centerline.len() - 2];
            let p1 = centerline[centerline.len() - 1];
            let dx = p1.0 - p0.0;
            let dy = p1.1 - p0.1;
            let len = (dx * dx + dy * dy).sqrt().max(1e-9);
            (p1, (dx / len, dy / len))
        } else {
            ((0.0, 0.0), (1.0, 0.0))
        };

        // 2D contour + hatch for each remaining piece after openings.
        for (footprint, footprint_bulges) in &pieces_2d {
            if footprint.len() < 3 {
                continue;
            }

            let draw_contour = if has_explicit_contour_filter {
                contour2d_visible && layer_included_contour
            } else if has_explicit_layers2d_filter {
                layers2d_visible && layer_included_layers2d
            } else {
                (layers2d_visible && layer_included_layers2d)
                    || (contour2d_visible && is_envelope && layer_included_contour)
            };
            let draw_hatch = (layer_hatch_visible && layer_included_hatch)
                || (contour_hatch_visible && is_envelope);

            if draw_contour {
                let n = footprint.len();
                let total_thick: f64 = layers.iter().map(|l| l.thickness).sum();
                let cap0 = if same_mat_end0 {
                    find_cap_edge_index(footprint, p_start, t_start, total_thick.max(0.2) * 2.5)
                } else {
                    None
                };
                let cap1 = if same_mat_end1 {
                    find_cap_edge_index(footprint, p_end, t_end, total_thick.max(0.2) * 2.5)
                } else {
                    None
                };

                let polylines_to_emit: Vec<LwPolyline> = match (cap0, cap1) {
                    (Some(c0), Some(c1)) if c0 != c1 && n >= 4 => {
                        let imin = c0.min(c1);
                        let imax = c0.max(c1);
                        let mut pl1 = LwPolyline::new();
                        for step in 0..=(imax - imin - 1) {
                            let idx = imin + 1 + step;
                            let bulge = if step < imax - imin - 1 {
                                footprint_bulges.get(idx).copied().unwrap_or(0.0)
                            } else {
                                0.0
                            };
                            let (x, y) = footprint[idx];
                            pl1.add_vertex(LwVertex::with_bulge(Vector2::new(x, y), bulge));
                        }
                        pl1.is_closed = false;

                        let mut pl2 = LwPolyline::new();
                        let count2 = n - (imax - imin);
                        for step in 0..count2 {
                            let idx = (imax + 1 + step) % n;
                            let bulge = if step < count2 - 1 {
                                footprint_bulges.get(idx).copied().unwrap_or(0.0)
                            } else {
                                0.0
                            };
                            let (x, y) = footprint[idx];
                            pl2.add_vertex(LwVertex::with_bulge(Vector2::new(x, y), bulge));
                        }
                        pl2.is_closed = false;
                        vec![pl1, pl2]
                    }
                    (Some(c), _) | (_, Some(c)) if n >= 3 => {
                        let mut pl = LwPolyline::new();
                        for step in 0..n {
                            let idx = (c + 1 + step) % n;
                            let bulge = if step < n - 1 {
                                footprint_bulges.get(idx).copied().unwrap_or(0.0)
                            } else {
                                0.0
                            };
                            let (x, y) = footprint[idx];
                            pl.add_vertex(LwVertex::with_bulge(Vector2::new(x, y), bulge));
                        }
                        pl.is_closed = false;
                        vec![pl]
                    }
                    _ => {
                        let mut pl = LwPolyline::new();
                        for (idx, &(x, y)) in footprint.iter().enumerate() {
                            let bulge = footprint_bulges.get(idx).copied().unwrap_or(0.0);
                            pl.add_vertex(LwVertex::with_bulge(Vector2::new(x, y), bulge));
                        }
                        pl.is_closed = true;
                        vec![pl]
                    }
                };

                for mut pl in polylines_to_emit {
                    pl.elevation = wall_base_z;
                    let contour_handle =
                        reuse_or_add_wall_contour(scene, &mut reusable_contours, pl, Some(wall_owner_handle));
                    let layer_name = layer
                        .layer_override
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("0");
                    let prefixed_layer = prefix_layer(layer_name, xref_layer_prefix.as_deref());
                    scene.ensure_layer(&prefixed_layer);
                    if let Some(e) = scene.document.get_entity_mut(contour_handle) {
                        e.as_entity_mut().set_layer(prefixed_layer);
                    }
                    if let Some(color) = line_color {
                        if let Some(e) = scene.document.get_entity_mut(contour_handle) {
                            e.as_entity_mut().set_color(color);
                        }
                    }
                    if let Some(lt) = contour_style_override.line_type.as_deref().filter(|s| !s.is_empty()) {
                        if let Some(e) = scene.document.get_entity_mut(contour_handle) {
                            e.common_mut().linetype = lt.to_string();
                        }
                    }
                    write_wall_display_tag(scene, contour_handle, wall_handle, WALL_REP_ROLE_CONTOUR);
                    new_derived.push(contour_handle);
                }
            }

            if draw_hatch {
                let tessellated = tessellate_ring_with_bulges(footprint, footprint_bulges);
                let (rel, origin, wcs) = pack_wall_ring(&tessellated);
                let phased = crate::modules::aec::project::hatch_origin::phase_families_wcs0(
                    &families,
                    origin,
                );
                let hatch_model = crate::scene::model::hatch_model::HatchModel {
                    pattern_origin: None,
                    render_instance: None,
                    boundary: std::sync::Arc::new(rel.clone()),
                    pattern: crate::scene::model::hatch_model::HatchPattern::Pattern(phased),
                    name: pattern_name.clone(),
                    color,
                    aci: 0,
                    line_weight_px: 1.0,
                    angle_offset: hatch_angle_offset,
                    scale: hatch_scale,
                    world_origin: origin,
                    boundary_wcs: Some(std::sync::Arc::new(wcs)),
                    fill_plane: Some(crate::scene::model::hatch_model::FillPlane {
                        origin: [origin[0], origin[1], wall_base_z],
                        x_axis: [1.0, 0.0, 0.0],
                        y_axis: [0.0, 1.0, 0.0],
                    }),
                    fill_plane_boundary: Some(std::sync::Arc::new(rel)),
                    boundary_exterior: None,
                    boundary_sources: None,
                    boundary_paths: None,
                    style: acadrust::entities::HatchStyleType::Normal,
                    draw_depth: 0.0,
                };
                let hatch_style = hatch_acad_color.map(|c| {
                    (c, acadrust::types::Transparency::from_percent(0.0))
                });
                let hatch_handle = scene.add_hatch(hatch_model, None, hatch_style);
                if !wall_owner_handle.is_null() && wall_owner_handle != scene.document.header.model_space_block_handle {
                    if let Some(e) = scene.document.get_entity_mut(hatch_handle) {
                        e.common_mut().owner_handle = wall_owner_handle;
                    }
                }
                let layer_name = layer
                    .layer_override
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("0");
                let prefixed_layer = prefix_layer(layer_name, xref_layer_prefix.as_deref());
                scene.ensure_layer(&prefixed_layer);
                if let Some(e) = scene.document.get_entity_mut(hatch_handle) {
                    e.as_entity_mut().set_layer(prefixed_layer);
                }
                if let Some(c) = hatch_acad_color {
                    if let Some(e) = scene.document.get_entity_mut(hatch_handle) {
                        e.as_entity_mut().set_color(c);
                    }
                }
                write_wall_display_tag(scene, hatch_handle, wall_handle, WALL_REP_ROLE_HATCH);
                new_derived.push(hatch_handle);
            }
        }

        // Rest-wall solids: Z-extrude remaining band pieces (or mitered /
        // corner-extended footprints). Opening-zone remainders are extruded
        // through the layer thickness separately below.
        let rest_for_layer: Vec<&engine::representation::WallLayerSolidPath> = display
            .solids
            .iter()
            .filter(|s| s.layer_index == i)
            .collect();
        let (solid_height, solid_base) = if let Some(ex) = extrusions.get(i) {
            (ex.height, ex.base_offset + wall_base_z)
        } else if let Some(solid) = rest_for_layer.first() {
            (solid.height, solid.base_offset + wall_base_z)
        } else {
            (height, wall_base_z)
        };
        let solid_rings: Vec<(Vec<(f64, f64)>, Vec<f64>)> = if !wall.top_facets.is_empty() || !wall.base_facets.is_empty() {
            let mut all_facets = wall.top_facets.clone();
            all_facets.extend(wall.base_facets.clone());
            let mut partitioned = Vec::new();
            for (fp, bulges) in &pieces_3d {
                partitioned.extend(partition_footprint_by_facets(fp, bulges, &all_facets));
            }
            partitioned
        } else {
            pieces_3d
        };
        if solid_visible && layer_included_solid && solid_height.abs() > 1e-9 {
            for (footprint, footprint_bulges) in &solid_rings {
                if footprint.len() < 3 {
                    continue;
                }
                let mut pl = LwPolyline::new();
                for (idx, &(x, y)) in footprint.iter().enumerate() {
                    let bulge = footprint_bulges.get(idx).copied().unwrap_or(0.0);
                    pl.add_vertex(LwVertex::with_bulge(Vector2::new(x, y), bulge));
                }
                pl.is_closed = true;
                let contour_entity = EntityType::LwPolyline(pl);

                let to_extrude = if solid_base.abs() > 1e-9 {
                    let mut clone = contour_entity.clone();
                    if let EntityType::LwPolyline(ref mut pl) = clone {
                        pl.elevation = solid_base;
                    }
                    Some(clone)
                } else {
                    None
                };
                let entity_to_use = to_extrude.as_ref().unwrap_or(&contour_entity);

                let body = if wall.is_sloped() {
                    build_sloped_layer_solid_3d(
                        footprint,
                        footprint_bulges,
                        &wall,
                        layers.get(i),
                        wall_base_z,
                        solid_height,
                    )
                } else {
                    crate::scene::model::sweep_model::extruded(entity_to_use, solid_height)
                };

                if let Some(body) = body {
                    let s3d = crate::modules::aec::engine::geometry::create_solid3d_from_body(&body);
                    let mut s3d_entity = EntityType::Solid3D(s3d);
                    if !wall_owner_handle.is_null() && wall_owner_handle != scene.document.header.model_space_block_handle {
                        s3d_entity.common_mut().owner_handle = wall_owner_handle;
                    }
                    let solid_handle = scene.add_entity(s3d_entity);
                    let layer_name = layer
                        .layer_override
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("0");
                    let prefixed_layer = prefix_layer(layer_name, xref_layer_prefix.as_deref());
                    scene.ensure_layer(&prefixed_layer);
                    if let Some(e) = scene.document.get_entity_mut(solid_handle) {
                        e.as_entity_mut().set_layer(prefixed_layer);
                    }
                    if let Some(mut display_geom) = scene.prepare_solid_model_display(solid_handle, &body) {
                        let mut miter_seams = Vec::new();
                        let total_thick: f64 = layers.iter().map(|l| l.thickness).sum();
                        if same_mat_end0 {
                            if let Some(c0) = find_cap_edge_index(footprint, p_start, t_start, total_thick.max(0.2) * 2.5) {
                                miter_seams.push((footprint[c0], footprint[(c0 + 1) % footprint.len()]));
                            }
                        }
                        if same_mat_end1 {
                            if let Some(c1) = find_cap_edge_index(footprint, p_end, t_end, total_thick.max(0.2) * 2.5) {
                                miter_seams.push((footprint[c1], footprint[(c1 + 1) % footprint.len()]));
                            }
                        }
                        if !miter_seams.is_empty() {
                            filter_miter_deck_edges(
                                &mut display_geom.0,
                                &mut display_geom.1,
                                &miter_seams,
                                &wall,
                                layers.get(i),
                                solid_base,
                                solid_base + solid_height,
                            );
                        }
                        if solid_rings.len() > 1 {
                            filter_facet_partition_edges(
                                &mut display_geom.0,
                                &mut display_geom.1,
                                footprint,
                                &solid_rings,
                                &wall,
                                layers.get(i),
                                solid_base,
                                solid_height,
                            );
                        }
                        if !wall_openings.is_empty() {
                            filter_opening_surface_edges(
                                &mut display_geom.0,
                                &mut display_geom.1,
                                &wall_openings,
                                &centerline,
                                solid_height,
                                solid_base,
                                wall_base_z,
                            );
                        }
                        scene.register_prepared_solid_model(solid_handle, body, display_geom);
                    }
                    if let Some(color) = fill_color {
                        if let Some(e) = scene.document.get_entity_mut(solid_handle) {
                            e.as_entity_mut().set_color(color);
                        }
                    }
                    write_wall_display_tag(scene, solid_handle, wall_handle, WALL_REP_ROLE_SOLID);
                    new_derived.push(solid_handle);
                }
            }
            // Opening zone: elevation remainder extruded through the layer
            // thickness.
            if solid_visible && layer_included_solid && !wall_openings.is_empty() {
                for zone in display.zone_solids.iter().filter(|z| z.layer_index == i) {
                    if zone.loop_xyz.len() < 3 {
                        continue;
                    }
                    let pts: Vec<Vector3> = zone
                        .loop_xyz
                        .iter()
                        .map(|p| Vector3::new(p[0], p[1], p[2] + wall_base_z))
                        .collect();
                    let mut pl = Polyline3D::from_points(pts);
                    pl.flags.closed = true;
                    let entity = EntityType::Polyline3D(pl);
                    if let Some(body) = crate::scene::model::sweep_model::extruded_direction(
                        &entity,
                        zone.direction,
                        0.0,
                    ) {
                        let s3d = crate::modules::aec::engine::geometry::create_solid3d_from_body(&body);
                        let mut s3d_entity = EntityType::Solid3D(s3d);
                        if !wall_owner_handle.is_null() && wall_owner_handle != scene.document.header.model_space_block_handle {
                            s3d_entity.common_mut().owner_handle = wall_owner_handle;
                        }
                        let solid_handle = scene.add_entity(s3d_entity);
                        let layer_name = layer
                            .layer_override
                            .as_deref()
                            .filter(|s| !s.is_empty())
                            .unwrap_or("0");
                        let prefixed_layer = prefix_layer(layer_name, xref_layer_prefix.as_deref());
                        scene.ensure_layer(&prefixed_layer);
                        if let Some(e) = scene.document.get_entity_mut(solid_handle) {
                            e.as_entity_mut().set_layer(prefixed_layer);
                        }
                        if let Some(mut display_geom) = scene.prepare_solid_model_display(solid_handle, &body) {
                            filter_opening_surface_edges(
                                &mut display_geom.0,
                                &mut display_geom.1,
                                &wall_openings,
                                &centerline,
                                solid_height,
                                solid_base,
                                wall_base_z,
                            );
                            scene.register_prepared_solid_model(solid_handle, body, display_geom);
                        }
                        if let Some(color) = fill_color {
                            if let Some(e) = scene.document.get_entity_mut(solid_handle) {
                                e.as_entity_mut().set_color(color);
                            }
                        }
                        write_wall_display_tag(
                            scene,
                            solid_handle,
                            wall_handle,
                            WALL_REP_ROLE_SOLID,
                        );
                        new_derived.push(solid_handle);
                    }
                }
            }
        }
    }

    if !reusable_contours.is_empty() {
        scene.erase_entities(&reusable_contours);
    }

    let _ = set_wall_derived_handles(scene, wall_handle, &new_derived);
    // Keep storey membership index in sync whenever a wall is (re)built.
    register_wall_in_storey(scene, wall_handle);
    engine::opening_display::regenerate_openings_for_wall(
        scene,
        wall_handle,
        library_override,
        rules,
    );
    let mut touched = Vec::with_capacity(1 + new_derived.len());
    touched.push(wall_handle);
    touched.extend(new_derived.iter().copied());
    for opening in openings_for_host_wall(scene, wall_handle) {
        touched.push(opening.handle);
        touched.extend(engine::opening_display::collect_opening_display_children(
            scene,
            opening.handle,
        ));
    }
    Ok(touched)
}
