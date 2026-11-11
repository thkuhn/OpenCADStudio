use super::*;

/// Shared inputs of the emit phases of `regenerate_opening_display`.
struct RegenCtx<'a> {
    opening: &'a Opening,
    opening_handle: Handle,
    wall_base_z: f64,
    origin: (f64, f64),
    tangent: (f64, f64),
    normal: (f64, f64),
    owner: Handle,
    xref_layer_prefix: Option<String>,
    default_opening_layer: Option<String>,
    style: &'a OpeningStyle,
    library: Option<&'a StyleLibrary>,
    merged_rules: Option<ComponentRuleSet>,
    slots: HashMap<OpeningComponentSlot, SlotGeometry>,
    levels: OpeningRepresentationLevels,
    params: OpeningBakeParams,
    grid: &'a ConstructionGrid,
    wall_y_min: f64,
    wall_y_max: f64,
    retained_label_scale: Option<Handle>,
    axis: &'a [(f64, f64)],
}

impl RegenCtx<'_> {
    fn layer_for(&self, scene: &mut Scene, role: Option<OpeningLookRole>) -> Option<String> {
        if role == Some(OpeningLookRole::Division) {
            Some(ensure_opening_grid_layer(scene, self.xref_layer_prefix.as_deref()))
        } else {
            self.default_opening_layer.clone()
        }
    }
}


/// Recreate `OPENING_REP` children for one opening.
pub fn regenerate_opening_display(
    scene: &mut Scene,
    opening_handle: Handle,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) {
    let Some(entity) = scene.document.get_entity(opening_handle).cloned() else {
        return;
    };
    let Some(opening) = opening_from_entity(&entity, opening_handle) else {
        return;
    };
    let wall_handle = resolve_wall_package(scene, opening.host_wall);
    let wall_base_z = host_base_z(scene, wall_handle);
    let axis: Vec<(f64, f64)> = get_wall_vertices(scene, wall_handle)
        .iter()
        .map(|v| (v.x, v.y))
        .collect();
    if axis.len() >= 2 {
        sync_opening_point_to_axis(scene, &opening, &axis, wall_base_z);
    }
    let stale = collect_opening_display_children(scene, opening_handle);
    let retained_label_scale = stale.iter().find_map(|handle| {
        let entity = scene.document.get_entity(*handle)?;
        if !matches!(entity, EntityType::MText(_))
            || opening_rep_slot_from_entity(entity) != Some(OpeningComponentSlot::OpeningLabel2D)
        {
            return None;
        }
        engine::annotative_label::native_annotation_scale(&scene.document, *handle)
    });
    if !stale.is_empty() {
        scene.erase_entities(&stale);
        for h in &stale {
            engine::owner_index::remove_child(&mut scene.document, opening_handle, *h);
        }
    }
    let thickness = host_thickness(scene, wall_handle);
    let (wall_y_min, wall_y_max) = scene
        .document
        .get_entity(wall_handle)
        .and_then(wall_from_entity)
        .filter(|w| !w.layers.is_empty())
        .map(|w| {
            let mut y_min = f64::INFINITY;
            let mut y_max = f64::NEG_INFINITY;
            for l in &w.layers {
                y_min = y_min.min(l.axis_offset).min(l.axis_offset + l.thickness);
                y_max = y_max.max(l.axis_offset).max(l.axis_offset + l.thickness);
            }
            (y_min, y_max)
        })
        .unwrap_or((-thickness * 0.5, thickness * 0.5));
    let plan = rules.map(|r| &r.opening_profiles);
    let (slots, style) = resolved_slots_for_plan(&opening, library, plan);
    let style = style.unwrap_or_else(|| default_style_for_kind(opening.kind));
    let grid = opening.division_override.as_ref().unwrap_or(&style.grid);
    let projected = project_division_sized(grid, opening.width, opening.height);
    let params = OpeningBakeParams::from_opening(&opening, thickness, Some(&style))
        .with_wall_bounds(wall_y_min, wall_y_max);
    let merged_rules = rules_with_plan_visibility(&opening, library, rules);
    let levels = representation_levels(
        &slots,
        &projected,
        merged_rules
            .as_ref()
            .and_then(|rules| rules.opening_division_level),
    );
    let local = bake_opening_representation(
        &slots,
        params,
        &style.parts,
        library,
        &grid,
        levels,
        merged_rules.as_ref(),
        Some(&scene.document),
    );

    let center_s = opening.center_along_axis();
    let Some(((cx, cy), (tx, ty))) = point_and_tangent_at_distance(&axis, center_s) else {
        return;
    };
    let normal = (-ty, tx);
    let origin = (
        cx + normal.0 * opening.cross_axis_offset,
        cy + normal.1 * opening.cross_axis_offset,
    );

    let (opening_owner_handle, xref_layer_prefix) =
        crate::modules::aec::engine::wall_package::wall_owner_and_layer_prefix(scene, opening_handle);
    let default_opening_layer = match xref_layer_prefix.as_deref() {
        Some(p) => {
            let l = format!("{p}0");
            scene.ensure_layer(&l);
            Some(l)
        }
        None => None,
    };
    let ctx = RegenCtx {
        opening: &opening,
        opening_handle,
        wall_base_z,
        origin,
        tangent: (tx, ty),
        normal,
        owner: opening_owner_handle,
        xref_layer_prefix,
        default_opening_layer,
        style: &style,
        library,
        merged_rules,
        slots,
        levels,
        params,
        grid,
        wall_y_min,
        wall_y_max,
        retained_label_scale,
        axis: &axis,
    };
    emit_plan_and_elevation(scene, &ctx, local);
    emit_label_2d(scene, &ctx);
    emit_3d_bodies(scene, &ctx);
}


fn emit_plan_and_elevation(scene: &mut Scene, ctx: &RegenCtx, local: Vec<BakedPath>) {
    let library = ctx.library;
    let merged_rules = &ctx.merged_rules;
    let normal = ctx.normal;
    let opening = ctx.opening;
    let opening_handle = ctx.opening_handle;
    let origin = ctx.origin;
    let style = ctx.style;
    let wall_base_z = ctx.wall_base_z;
    let opening_owner_handle = ctx.owner;
    let (tx, ty) = ctx.tangent;
    let layer_for = |scene: &mut Scene, role: Option<OpeningLookRole>| ctx.layer_for(scene, role);
    for baked in local {
        if baked.points.len() < 2 {
            continue;
        }
        if baked.slot.is_elevation() {
            let pts: Vec<acadrust::types::Vector3> = baked
                .points
                .iter()
                .map(|&(u, v)| {
                    acadrust::types::Vector3::new(
                        origin.0 + tx * u,
                        origin.1 + ty * u,
                        wall_base_z + opening.sill_height + v,
                    )
                })
                .collect();
            let mut pl = acadrust::entities::Polyline3D::from_points(pts);
            pl.flags.closed = baked.closed;
            let mut entity = EntityType::Polyline3D(pl);
            if !opening_owner_handle.is_null() && opening_owner_handle != scene.document.header.model_space_block_handle {
                entity.common_mut().owner_handle = opening_owner_handle;
            }
            if let Some(layer) = layer_for(scene, baked.resolved_look_role()) {
                entity.as_entity_mut().set_layer(layer);
            }
            if let Some(role) = baked.resolved_look_role() {
                apply_opening_look_to_entity(
                    &mut entity,
                    &style,
                    library,
                    merged_rules.as_ref(),
                    role,
                    baked.slot,
                );
            }
            let handle = scene.add_entity(entity);
            write_opening_display_tag(scene, handle, opening_handle, baked.slot);
            engine::owner_index::add_child(&mut scene.document, opening_handle, handle);
            continue;
        }

        let world_pts: Vec<(f64, f64)> = baked
            .points
            .iter()
            .map(|&(x, y)| local_to_world(x, y, origin, (tx, ty), normal))
            .collect();

        if baked.filled {
            if let Some(model) = solid_hatch_from_ring(&world_pts) {
                let hatch = scene.add_hatch(model, None, None);
                if !opening_owner_handle.is_null() && opening_owner_handle != scene.document.header.model_space_block_handle {
                    if let Some(e) = scene.document.get_entity_mut(hatch) {
                        e.common_mut().owner_handle = opening_owner_handle;
                    }
                }
                if let Some(layer) = layer_for(scene, baked.resolved_look_role()) {
                    if let Some(e) = scene.document.get_entity_mut(hatch) {
                        e.as_entity_mut().set_layer(layer);
                    }
                }
                if let Some(EntityType::Hatch(h)) = scene.document.get_entity_mut(hatch) {
                    h.elevation = wall_base_z;
                }
                write_opening_display_tag(scene, hatch, opening_handle, baked.slot);
                engine::owner_index::add_child(&mut scene.document, opening_handle, hatch);
            }
            continue;
        }
        let mut pl = LwPolyline::new();
        for &(x, y) in &world_pts {
            pl.add_vertex(LwVertex::new(Vector2::new(x, y)));
        }
        pl.is_closed = baked.closed;
        pl.elevation = wall_base_z;
        let mut entity = EntityType::LwPolyline(pl);
        if !opening_owner_handle.is_null() && opening_owner_handle != scene.document.header.model_space_block_handle {
            entity.common_mut().owner_handle = opening_owner_handle;
        }
        if let Some(layer) = layer_for(scene, baked.resolved_look_role()) {
            entity.as_entity_mut().set_layer(layer);
        }
        if let Some(role) = baked.resolved_look_role() {
            apply_opening_look_to_entity(
                &mut entity,
                &style,
                library,
                merged_rules.as_ref(),
                role,
                baked.slot,
            );
        }
        let handle = scene.add_entity(entity);
        write_opening_display_tag(scene, handle, opening_handle, baked.slot);
        engine::owner_index::add_child(&mut scene.document, opening_handle, handle);
    }
}

fn emit_label_2d(scene: &mut Scene, ctx: &RegenCtx) {
    let axis = ctx.axis;
    let default_opening_layer = &ctx.default_opening_layer;
    let merged_rules = &ctx.merged_rules;
    let opening = ctx.opening;
    let opening_handle = ctx.opening_handle;
    let retained_label_scale = ctx.retained_label_scale;
    let slots = &ctx.slots;
    let wall_base_z = ctx.wall_base_z;
    let opening_owner_handle = ctx.owner;
    if merged_rules
        .as_ref()
        .map_or(true, |r| r.is_opening_visible(OpeningComponentSlot::OpeningLabel2D))
        && slots.contains_key(&OpeningComponentSlot::OpeningLabel2D)
    {
        if let Some(((cx, cy), (tx, ty))) =
            point_and_tangent_at_distance(&axis, opening.center_along_axis())
        {
            let normal = (-ty, tx);
            let lx = cx + normal.0 * opening.cross_axis_offset;
            let ly = cy + normal.1 * opening.cross_axis_offset;
            let label_text = format_din1356_label(&opening);
            let mut mtext = acadrust::entities::MText::new();
            mtext.value = label_text;
            mtext.insertion_point = acadrust::types::Vector3::new(lx, ly, wall_base_z);
            mtext.height = 0.15;
            let mut rot = ty.atan2(tx);
            if rot > std::f64::consts::FRAC_PI_2 {
                rot -= std::f64::consts::PI;
            } else if rot < -std::f64::consts::FRAC_PI_2 {
                rot += std::f64::consts::PI;
            }
            mtext.rotation = rot;
            mtext.attachment_point = acadrust::entities::mtext::AttachmentPoint::MiddleCenter;
            let mut entity = EntityType::MText(Box::new(mtext));
            if !opening_owner_handle.is_null() && opening_owner_handle != scene.document.header.model_space_block_handle {
                entity.common_mut().owner_handle = opening_owner_handle;
            }
            if let Some(ref l) = default_opening_layer {
                entity.as_entity_mut().set_layer(l.clone());
            }
            let handle = scene.add_entity(entity);
            let _ = engine::annotative_label::bind_annotative_mtext(
                &mut scene.document,
                handle,
                retained_label_scale,
            );
            write_opening_display_tag(
                scene,
                handle,
                opening_handle,
                OpeningComponentSlot::OpeningLabel2D,
            );
            engine::owner_index::add_child(&mut scene.document, opening_handle, handle);
        }
    }
}

fn emit_3d_bodies(scene: &mut Scene, ctx: &RegenCtx) {
    let default_opening_layer = &ctx.default_opening_layer;
    let grid = ctx.grid;
    let levels = &ctx.levels;
    let library = ctx.library;
    let merged_rules = &ctx.merged_rules;
    let normal = ctx.normal;
    let opening = ctx.opening;
    let opening_handle = ctx.opening_handle;
    let origin = ctx.origin;
    let params = ctx.params;
    let slots = &ctx.slots;
    let style = ctx.style;
    let wall_base_z = ctx.wall_base_z;
    let wall_y_max = ctx.wall_y_max;
    let wall_y_min = ctx.wall_y_min;
    let opening_owner_handle = ctx.owner;
    let (tx, ty) = ctx.tangent;
    let layer_for = |scene: &mut Scene, role: Option<OpeningLookRole>| ctx.layer_for(scene, role);
    let frame_3d_vis = merged_rules
        .as_ref()
        .map_or(true, |r| r.is_opening_visible(OpeningComponentSlot::Frame3D))
        && (slots.contains_key(&OpeningComponentSlot::Frame3D)
            || slots.contains_key(&OpeningComponentSlot::Solid3D));
    let fixed_frame_section = resolve_component_section(
        library,
        style.parts.fixed_frame.profile_id.as_deref(),
        style.parts.fixed_frame.face_width,
        style.parts.fixed_frame.wall_depth,
    );
    let leaf_frame_section = resolve_component_section(
        library,
        style.parts.leaf_frame.profile_id.as_deref(),
        style.parts.leaf_frame.face_width,
        style.parts.leaf_frame.depth,
    );
    let division_profile = grid
        .muntin_profile_id
        .as_deref()
        .and_then(|id| library?.find_section_profile(id))
        .filter(|profile| profile.is_valid());
    let division_section = resolve_component_section(
        library,
        grid.muntin_profile_id.as_deref(),
        style.parts.fixed_frame.face_width,
        style.parts.fixed_frame.wall_depth,
    );
    let exterior_sill_section = resolve_component_section(
        library,
        style.parts.exterior_sill.profile_id.as_deref(),
        style.parts.exterior_sill.thickness,
        style.parts.exterior_sill.thickness,
    );
    let interior_board_section = resolve_component_section(
        library,
        style.parts.interior_board.profile_id.as_deref(),
        style.parts.interior_board.thickness,
        style.parts.interior_board.thickness,
    );
    let threshold_section = resolve_component_section(
        library,
        style.parts.threshold.profile_id.as_deref(),
        style.parts.threshold.height,
        style.parts.threshold.width,
    );
    let shutter_section = resolve_component_section(
        library,
        style.parts.shutter_box.profile_id.as_deref(),
        style.parts.shutter_box.height,
        style.parts.shutter_box.depth,
    );
    let lintel_section = resolve_component_section(
        library,
        style.parts.lintel.profile_id.as_deref(),
        style.parts.lintel.height,
        wall_y_max - wall_y_min,
    );
    let division_post_width = division_profile.map_or(fixed_frame_section.face, |_| division_section.face);
    let division_depth = division_profile.map_or(fixed_frame_section.depth, |_| division_section.depth);

    if frame_3d_vis && opening.kind != OpeningKind::Breakthrough {
        if opening_component_should_render_with_library(
            &style.parts,
            opening.kind,
            OpeningLookRole::FixedFrame,
            library,
        ) {
            if let Some(body) = build_opening_frame_3d_with_threshold(
                opening.width,
                opening.height,
                fixed_frame_section.face,
                fixed_frame_section.depth,
                params.shape,
                params.spring_height,
                !omits_threshold(&resolve_grid(grid, opening.width, opening.height), opening.width)
                    && opening.kind != OpeningKind::Door,
            ) {
                emit_opening_solid_3d(
                    scene,
                    body,
                    opening_handle,
                    opening_owner_handle,
                    default_opening_layer.as_deref(),
                    origin,
                    (tx, ty),
                    normal,
                    wall_base_z + opening.sill_height,
                    0.0,
                    0.0,
                    OpeningComponentSlot::Frame3D,
                    OpeningLookRole::FixedFrame,
                    &style,
                    library,
                    merged_rules.as_ref(),
                );
            }
        }
        if levels.divisions != DivisionLevel::Off
            && opening_component_should_render_with_library(
                &style.parts,
                opening.kind,
                OpeningLookRole::Division,
                library,
            )
        {
            for (body, center_x, center_z) in build_opening_grid_bars_3d(
                grid,
                params,
                division_post_width,
                division_depth,
                library,
                &leaf_frame_section,
            ) {
                let grid_layer = layer_for(scene, Some(OpeningLookRole::Division));
                emit_opening_solid_3d(
                    scene,
                    body,
                    opening_handle,
                    opening_owner_handle,
                    grid_layer.as_deref(),
                    origin,
                    (tx, ty),
                    normal,
                    wall_base_z + opening.sill_height,
                    center_x,
                    center_z,
                    OpeningComponentSlot::Frame3D,
                    OpeningLookRole::Division,
                    &style,
                    library,
                    merged_rules.as_ref(),
                );
            }
        }
    }

    let leaf_3d_vis = merged_rules
        .as_ref()
        .map_or(true, |r| r.is_opening_visible(OpeningComponentSlot::Leaf3D))
        && (slots.contains_key(&OpeningComponentSlot::Leaf3D)
            || slots.contains_key(&OpeningComponentSlot::Solid3D));
    let leaf_component_visible = leaf_3d_vis
        && opening_component_should_render_with_library(
            &style.parts,
            opening.kind,
            OpeningLookRole::Leaf,
            library,
        );
    let glazing_3d_vis = merged_rules
        .as_ref()
        .map_or(true, |r| r.is_opening_visible(OpeningComponentSlot::Glazing3D))
        && (slots.contains_key(&OpeningComponentSlot::Glazing3D)
            || slots.contains_key(&OpeningComponentSlot::Solid3D));
    let leaf_glazing_visible = glazing_3d_vis
        && opening_component_should_render_with_library(
            &style.parts,
            opening.kind,
            OpeningLookRole::Glazing,
            library,
        );

    if leaf_component_visible {
        {
            let divisions_visible = levels.divisions != DivisionLevel::Off
                && opening_component_should_render_with_library(
                    &style.parts,
                    opening.kind,
                    OpeningLookRole::Division,
                    library,
                );
            let overlap = (style.parts.leaf_frame.rebate_overlap.max(0.0)
                + style.parts.leaf_frame.lateral_overlap.max(0.0))
                .max(0.0);
            let cells = leaf_cells_with_bar_offsets(
                grid,
                opening.width,
                opening.height,
                fixed_frame_section.face,
                divisions_visible.then_some(division_post_width),
                leaf_frame_section.face,
                overlap,
                frame_3d_vis
                    && opening_component_should_render_with_library(
                        &style.parts,
                        opening.kind,
                        OpeningLookRole::FixedFrame,
                        library,
                    ),
                opening.kind == OpeningKind::Door,
            );
            for cell in cells {
                if !matches!(
                    cell.cell.cell.fill,
                    crate::modules::aec::engine::construction_grid::CellFill::Door
                        | crate::modules::aec::engine::construction_grid::CellFill::Glass
                ) {
                    continue;
                }
                let top = cell.top.min(arch_height_at_x(params, cell.left)).min(arch_height_at_x(params, cell.right));
                let height = top - cell.bottom;
                let width = cell.right - cell.left;
                if width <= 0.0 || height <= 0.0 {
                    continue;
                }
                let is_door = matches!(
                    cell.cell.cell.fill,
                    crate::modules::aec::engine::construction_grid::CellFill::Door
                );
                let center_x = (cell.left + cell.right) * 0.5;
                let operation = cell.cell.cell.operation.unwrap_or(opening.hinge);
                let pivot_x = match operation.bearing() {
                    TrackEnd::Start => cell.pivot_start - center_x,
                    TrackEnd::End => cell.pivot_end - center_x,
                };
                let swing = cell.cell.cell.swing.unwrap_or(params.swing_side);
                let angle = cell
                    .cell
                    .cell
                    .angle_deg
                    .unwrap_or(style.opening_angle_deg);
                if let Some((body, glass)) = build_opening_leaf_components_3d(
                    width,
                    height,
                    leaf_frame_section.face,
                    leaf_frame_section.depth,
                    style.parts.leaf_frame.depth_offset,
                    pivot_x,
                    angle,
                    operation,
                    swing,
                    is_door,
                    OpeningShape::Rectangle,
                    0.0,
                    (!is_door && leaf_glazing_visible)
                        .then_some(style.parts.glazing.thickness),
                ) {
                    emit_opening_solid_3d(
                        scene,
                        body,
                        opening_handle,
                        opening_owner_handle,
                        default_opening_layer.as_deref(),
                        origin,
                        (tx, ty),
                        normal,
                        wall_base_z + opening.sill_height,
                        center_x,
                        cell.bottom,
                        OpeningComponentSlot::Leaf3D,
                        OpeningLookRole::Leaf,
                        &style,
                        library,
                        merged_rules.as_ref(),
                    );
                    if let Some(glass) = glass {
                        emit_opening_solid_3d(
                            scene,
                            glass,
                            opening_handle,
                            opening_owner_handle,
                            default_opening_layer.as_deref(),
                            origin,
                            (tx, ty),
                            normal,
                            wall_base_z + opening.sill_height,
                            center_x,
                            cell.bottom,
                            OpeningComponentSlot::Glazing3D,
                            OpeningLookRole::Glazing,
                            &style,
                            library,
                            merged_rules.as_ref(),
                        );
                    }
                }
            }
        }
    }

    if glazing_3d_vis
        && opening_component_should_render_with_library(
            &style.parts,
            opening.kind,
            OpeningLookRole::Glazing,
            library,
        )
    {
        {
            for cell in flatten_resolved_cells(grid, opening.width, opening.height) {
                let is_leaf_glass = matches!(
                    cell.cell.fill,
                    crate::modules::aec::engine::construction_grid::CellFill::Glass
                );
                if (!is_leaf_glass
                    && !matches!(
                        cell.cell.fill,
                        crate::modules::aec::engine::construction_grid::CellFill::Fixed
                    ))
                    || (is_leaf_glass && leaf_component_visible)
                {
                    continue;
                }
                let inset = style.parts.glazing.inset.max(0.0);
                let leaf_here = is_leaf_glass && leaf_component_visible;
                let bar = if leaf_here {
                    leaf_frame_section.face
                } else {
                    fixed_frame_section.face
                };
                let overlap = if leaf_here {
                    (style.parts.leaf_frame.rebate_overlap + style.parts.leaf_frame.lateral_overlap)
                        .max(0.0)
                } else {
                    0.0
                };
                let surround = (bar - overlap).max(0.0);
                let left = glass_edge_into_bar(cell.left, 1.0, surround, inset);
                let right = glass_edge_into_bar(cell.right, -1.0, surround, inset);
                let bottom_bar = if matches!(
                    cell.cell.fill,
                    crate::modules::aec::engine::construction_grid::CellFill::Door
                ) {
                    0.0
                } else {
                    surround
                };
                let bottom = glass_edge_into_bar(cell.bottom, 1.0, bottom_bar, inset);
                let top = glass_edge_into_bar(
                    cell.top
                        .min(arch_height_at_x(params, left))
                        .min(arch_height_at_x(params, right)),
                    -1.0,
                    surround,
                    inset,
                );
                let width = right - left;
                let height = top - bottom;
                if width <= 0.0 || height <= 0.0 {
                    continue;
                }
                if let Some(body) = build_opening_box_3d(
                    width,
                    height,
                    style.parts.glazing.thickness,
                ) {
                    emit_opening_solid_3d(
                        scene,
                        body,
                        opening_handle,
                        opening_owner_handle,
                        default_opening_layer.as_deref(),
                        origin,
                        (tx, ty),
                        normal,
                        wall_base_z + opening.sill_height,
                        (left + right) * 0.5,
                        (bottom + top) * 0.5,
                        OpeningComponentSlot::Glazing3D,
                        OpeningLookRole::Glazing,
                        &style,
                        library,
                        merged_rules.as_ref(),
                    );
                }
            }
        }
    }

    let accessories_3d_vis = merged_rules
        .as_ref()
        .map_or(true, |r| r.is_opening_visible(OpeningComponentSlot::Solid3D));
    if accessories_3d_vis {
        let sill = &style.parts.exterior_sill;
        if opening_component_should_render_with_library(
            &style.parts,
            opening.kind,
            OpeningLookRole::ExteriorSill,
            library,
        )
            && wall_y_max > wall_y_min
        {
            let overhang = sill.overhang.max(0.0);
            let upstand = sill.upstand.max(0.0);
            let left = sill.side_overhang_left.max(0.0);
            let right = sill.side_overhang_right.max(0.0);
            let width = opening.width + left + right;
            let x_offset = (right - left) * 0.5;
            let frame_outer = (-fixed_frame_section.depth * 0.5)
                .clamp(wall_y_min.min(wall_y_max), wall_y_min.max(wall_y_max));
            let has_profile = sill
                .profile_id
                .as_deref()
                .and_then(|id| library?.find_section_profile(id))
                .is_some_and(|profile| profile.is_valid());
            if has_profile {
                let profile_outer = frame_outer - exterior_sill_section.depth;
                let requested_outer = wall_y_min - overhang;
                let y_min = requested_outer.min(profile_outer);
                emit_opening_accessory_3d(
                    scene,
                    build_opening_profile_prism_3d(
                        width,
                        &exterior_sill_section,
                        profile_outer,
                        1.0,
                    ),
                    opening_handle,
                    opening_owner_handle,
                    default_opening_layer.as_deref(),
                    origin,
                    (tx, ty),
                    normal,
                    wall_base_z + opening.sill_height,
                    x_offset,
                    0.0,
                    OpeningLookRole::ExteriorSill,
                    &style,
                    library,
                    merged_rules.as_ref(),
                );
                emit_opening_accessory_3d(
                    scene,
                    build_opening_box_3d_with_bounds(
                        width,
                        exterior_sill_section.face,
                        y_min,
                        profile_outer,
                    ),
                    opening_handle,
                    opening_owner_handle,
                    default_opening_layer.as_deref(),
                    origin,
                    (tx, ty),
                    normal,
                    wall_base_z + opening.sill_height,
                    x_offset,
                    exterior_sill_section.face * 0.5,
                    OpeningLookRole::ExteriorSill,
                    &style,
                    library,
                    merged_rules.as_ref(),
                );
            } else {
                let (y_min, y_max) = exterior_sill_y_bounds(
                    wall_y_min,
                    wall_y_max,
                    overhang,
                    fixed_frame_section.depth,
                );
                emit_opening_accessory_3d(
                    scene,
                    build_opening_box_3d_with_bounds(
                        width,
                        exterior_sill_section.face,
                        y_min,
                        y_max,
                    ),
                    opening_handle,
                    opening_owner_handle,
                    default_opening_layer.as_deref(),
                    origin,
                    (tx, ty),
                    normal,
                    wall_base_z + opening.sill_height,
                    x_offset,
                    exterior_sill_section.face * 0.5,
                    OpeningLookRole::ExteriorSill,
                    &style,
                    library,
                    merged_rules.as_ref(),
                );
                if upstand > 0.0 {
                    let (up_min, up_max) = sill_upstand_interval(
                        frame_outer,
                        -1.0,
                        exterior_sill_section.depth,
                        fixed_frame_section.depth,
                    );
                    emit_opening_accessory_3d(
                        scene,
                        build_opening_box_3d_with_bounds(width, upstand, up_min, up_max),
                        opening_handle,
                        opening_owner_handle,
                        default_opening_layer.as_deref(),
                        origin,
                        (tx, ty),
                        normal,
                        wall_base_z + opening.sill_height,
                        x_offset,
                        exterior_sill_section.face + upstand * 0.5,
                        OpeningLookRole::ExteriorSill,
                        &style,
                        library,
                        merged_rules.as_ref(),
                    );
                }
            }
        }

        let board = &style.parts.interior_board;
        if opening_component_should_render_with_library(
            &style.parts,
            opening.kind,
            OpeningLookRole::InteriorBoard,
            library,
        )
            && wall_y_max > wall_y_min
        {
            let overhang = board.overhang.max(0.0);
            let upstand = board.upstand.max(0.0);
            let left = board.side_overhang_left.max(0.0);
            let right = board.side_overhang_right.max(0.0);
            let width = opening.width + left + right;
            let x_offset = (right - left) * 0.5;
            let frame_inner = (fixed_frame_section.depth * 0.5)
                .clamp(wall_y_min.min(wall_y_max), wall_y_min.max(wall_y_max));
            let has_profile = board
                .profile_id
                .as_deref()
                .and_then(|id| library?.find_section_profile(id))
                .is_some_and(|profile| profile.is_valid());
            if has_profile {
                let profile_inner = frame_inner + interior_board_section.depth;
                let requested_inner = wall_y_max + overhang;
                let y_max = requested_inner.max(profile_inner);
                emit_opening_accessory_3d(
                    scene,
                    build_opening_profile_prism_3d(
                        width,
                        &interior_board_section,
                        frame_inner,
                        1.0,
                    ),
                    opening_handle,
                    opening_owner_handle,
                    default_opening_layer.as_deref(),
                    origin,
                    (tx, ty),
                    normal,
                    wall_base_z + opening.sill_height,
                    x_offset,
                    0.0,
                    OpeningLookRole::InteriorBoard,
                    &style,
                    library,
                    merged_rules.as_ref(),
                );
                emit_opening_accessory_3d(
                    scene,
                    build_opening_box_3d_with_bounds(
                        width,
                        interior_board_section.face,
                        profile_inner,
                        y_max,
                    ),
                    opening_handle,
                    opening_owner_handle,
                    default_opening_layer.as_deref(),
                    origin,
                    (tx, ty),
                    normal,
                    wall_base_z + opening.sill_height,
                    x_offset,
                    interior_board_section.face * 0.5,
                    OpeningLookRole::InteriorBoard,
                    &style,
                    library,
                    merged_rules.as_ref(),
                );
            } else {
                let (y_min, y_max) = interior_board_y_bounds(
                    wall_y_min,
                    wall_y_max,
                    overhang,
                    fixed_frame_section.depth,
                );
                emit_opening_accessory_3d(
                    scene,
                    build_opening_box_3d_with_bounds(
                        width,
                        interior_board_section.face,
                        y_min,
                        y_max,
                    ),
                    opening_handle,
                    opening_owner_handle,
                    default_opening_layer.as_deref(),
                    origin,
                    (tx, ty),
                    normal,
                    wall_base_z + opening.sill_height,
                    x_offset,
                    interior_board_section.face * 0.5,
                    OpeningLookRole::InteriorBoard,
                    &style,
                    library,
                    merged_rules.as_ref(),
                );
                if upstand > 0.0 {
                    let (up_min, up_max) = sill_upstand_interval(
                        frame_inner,
                        1.0,
                        interior_board_section.depth,
                        fixed_frame_section.depth,
                    );
                    emit_opening_accessory_3d(
                        scene,
                        build_opening_box_3d_with_bounds(width, upstand, up_min, up_max),
                        opening_handle,
                        opening_owner_handle,
                        default_opening_layer.as_deref(),
                        origin,
                        (tx, ty),
                        normal,
                        wall_base_z + opening.sill_height,
                        x_offset,
                        interior_board_section.face + upstand * 0.5,
                        OpeningLookRole::InteriorBoard,
                        &style,
                        library,
                        merged_rules.as_ref(),
                    );
                }
            }
        }

        if opening_component_should_render_with_library(
            &style.parts,
            opening.kind,
            OpeningLookRole::Threshold,
            library,
        ) {
            emit_opening_accessory_3d(
                scene,
                build_opening_box_3d_with_bounds(
                    opening.width,
                    threshold_section.face,
                    -threshold_section.depth * 0.5,
                    threshold_section.depth * 0.5,
                ),
                opening_handle,
                opening_owner_handle,
                default_opening_layer.as_deref(),
                origin,
                (tx, ty),
                normal,
                wall_base_z + opening.sill_height,
                0.0,
                threshold_section.face * 0.5,
                OpeningLookRole::Threshold,
                &style,
                library,
                merged_rules.as_ref(),
            );
        }

        let shutter = &style.parts.shutter_box;
        if opening_component_should_render_with_library(
            &style.parts,
            opening.kind,
            OpeningLookRole::ShutterBox,
            library,
        ) {
            let (y_min, y_max) = wall_clamped_interval(
                wall_y_min,
                wall_y_max,
                wall_y_min + shutter.installation_depth.max(0.0),
                shutter_section.depth,
            );
            emit_opening_accessory_3d(
                scene,
                build_opening_box_3d_with_bounds(
                    opening.width,
                    shutter_section.face,
                    y_min,
                    y_max,
                ),
                opening_handle,
                opening_owner_handle,
                default_opening_layer.as_deref(),
                origin,
                (tx, ty),
                normal,
                wall_base_z + opening.sill_height,
                0.0,
                accessory_bottom(opening.height, shutter.seat_above_frame) + shutter_section.face * 0.5,
                OpeningLookRole::ShutterBox,
                &style,
                library,
                merged_rules.as_ref(),
            );
        }

        let lintel = &style.parts.lintel;
        if opening_component_should_render_with_library(
            &style.parts,
            opening.kind,
            OpeningLookRole::Lintel,
            library,
        ) {
            let bearing_left = lintel.bearing_left.max(0.0);
            let bearing_right = lintel.bearing_right.max(0.0);
            let (y_min, y_max) = lintel_depth_interval(wall_y_min, wall_y_max, lintel_section.depth);
            emit_opening_accessory_3d(
                scene,
                build_opening_box_3d_with_bounds(
                    opening.width + bearing_left + bearing_right,
                    lintel_section.face,
                    y_min,
                    y_max,
                ),
                opening_handle,
                opening_owner_handle,
                default_opening_layer.as_deref(),
                origin,
                (tx, ty),
                normal,
                wall_base_z + opening.sill_height,
                (bearing_right - bearing_left) * 0.5,
                opening.height + lintel_section.face * 0.5,
                OpeningLookRole::Lintel,
                &style,
                library,
                merged_rules.as_ref(),
            );
        }
    }
}

/// Regen every opening hosted by `wall_handle`.
pub fn regenerate_openings_for_wall(
    scene: &mut Scene,
    wall_handle: Handle,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let openings = openings_for_host_wall(scene, wall_handle);
    for opening in openings {
        regenerate_opening_display(scene, opening.handle, library, rules);
    }
}

/// Write instance XDATA, snap the POINT, and regen host cut + symbols.
pub fn commit_opening_instance(
    scene: &mut Scene,
    opening: &Opening,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> Vec<Handle> {
    write_opening_instance(scene, opening);
    let wall_handle = resolve_wall_package(scene, opening.host_wall);
    let mut touched = match engine::wall_regen::regenerate_wall_representation_with_rules_and_substitutions(
        scene,
        wall_handle,
        rules,
        None,
        library,
    ) {
        Ok(t) => t,
        Err(_) => {
            regenerate_opening_display(scene, opening.handle, library, rules);
            vec![wall_handle, opening.handle]
        }
    };
    touched.push(opening.handle);
    touched.extend(collect_opening_display_children(scene, opening.handle));
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    touched
}

/// Project a dragged opening POINT back onto the host axis and rebake.
pub fn sync_opening_from_point_location(
    scene: &mut Scene,
    opening_handle: Handle,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> bool {
    let Some(entity) = scene.document.get_entity(opening_handle).cloned() else {
        return false;
    };
    let Some(mut opening) = opening_from_entity(&entity, opening_handle) else {
        return false;
    };
    let EntityType::Point(pt) = &entity else {
        return false;
    };
    let wall_handle = resolve_wall_package(scene, opening.host_wall);
    let axis: Vec<(f64, f64)> = get_wall_vertices(scene, wall_handle)
        .iter()
        .map(|v| (v.x, v.y))
        .collect();
    let Some(s) = engine::openings::distance_along_axis_from_point(&axis, (pt.location.x, pt.location.y))
    else {
        return false;
    };
    opening.distance_along_axis = s.max(0.0);
    let _ = commit_opening_instance(scene, &opening, library, rules);
    true
}


