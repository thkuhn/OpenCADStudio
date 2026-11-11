use super::*;

/// World-space generator bake for an opening on `axis`.
pub fn bake_opening_world(
    axis: &[(f64, f64)],
    thickness: f64,
    opening: &Opening,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> Vec<BakedPath> {
    bake_opening_world_with_doc(axis, thickness, opening, library, rules, None)
}

pub fn bake_opening_world_with_doc(
    axis: &[(f64, f64)],
    thickness: f64,
    opening: &Opening,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
    doc: Option<&acadrust::CadDocument>,
) -> Vec<BakedPath> {
    let center_s = opening.center_along_axis();
    let Some(((cx, cy), (tx, ty))) = point_and_tangent_at_distance(axis, center_s)
    else {
        return Vec::new();
    };
    let normal = (-ty, tx);
    let origin = (
        cx + normal.0 * opening.cross_axis_offset,
        cy + normal.1 * opening.cross_axis_offset,
    );
    let (wall_y_min, wall_y_max) = doc
        .and_then(|d| d.get_entity(opening.host_wall))
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
    let (slots, style) = resolved_slots_for_plan(opening, library, plan);
    let style = style.unwrap_or_else(|| default_style_for_kind(opening.kind));
    let grid = opening.division_override.as_ref().unwrap_or(&style.grid);
    let projected = project_division_sized(grid, opening.width, opening.height);
    let params = OpeningBakeParams::from_opening(opening, thickness, Some(&style))
        .with_wall_bounds(wall_y_min, wall_y_max);
    let merged = rules_with_plan_visibility(opening, library, rules);
    let local = bake_opening_representation(
        &slots,
        params,
        &style.parts,
        library,
        grid,
        representation_levels(
            &slots,
            &projected,
            merged
                .as_ref()
                .and_then(|rules| rules.opening_division_level),
        ),
        merged.as_ref(),
        doc,
    );
    transform_paths(&local, origin, (tx, ty), normal)
}

/// Rubber-band wires for live placement preview.
pub fn preview_opening_wires(
    axis: &[(f64, f64)],
    thickness: f64,
    opening: &Opening,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
    base_z: f64,
) -> Vec<WireModel> {
    let paths = bake_opening_world(axis, thickness, opening, library, rules);
    let mut wires = Vec::new();
    let z = base_z as f32;
    if let Some(cut) = engine::openings::opening_footprint_2d(axis, thickness, opening) {
        let mut pts: Vec<[f32; 3]> = cut
            .iter()
            .map(|&(x, y)| [x as f32, y as f32, z])
            .collect();
        if let Some(first) = pts.first().copied() {
            pts.push(first);
        }
        wires.push(WireModel::solid(
            "opening_cut".into(),
            pts,
            WireModel::CYAN,
            false,
        ));
    }
    for (i, path) in paths.iter().enumerate() {
        if path.points.len() < 2 {
            continue;
        }
        let mut pts: Vec<[f32; 3]> = path
            .points
            .iter()
            .map(|&(x, y)| [x as f32, y as f32, z])
            .collect();
        if path.closed {
            if let Some(first) = pts.first().copied() {
                pts.push(first);
            }
        }
        wires.push(WireModel::solid(
            format!("opening_slot_{i}"),
            pts,
            WireModel::CYAN,
            false,
        ));
    }
    wires
}

/// Solid-fill preview hatches (e.g. `DiagonalFill`).
pub fn preview_opening_hatches(
    axis: &[(f64, f64)],
    thickness: f64,
    opening: &Opening,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> Vec<HatchModel> {
    bake_opening_world(axis, thickness, opening, library, rules)
        .into_iter()
        .filter(|p| p.filled && p.closed && p.points.len() >= 3)
        .filter_map(|p| solid_hatch_from_ring(&p.points))
        .collect()
}

pub(super) fn solid_hatch_from_ring(ring: &[(f64, f64)]) -> Option<HatchModel> {
    if ring.len() < 3 {
        return None;
    }
    let origin = [ring[0].0, ring[0].1];
    let boundary: Vec<[f32; 2]> = ring
        .iter()
        .map(|(x, y)| [(*x - origin[0]) as f32, (*y - origin[1]) as f32])
        .collect();
    Some(HatchModel {
        pattern_origin: None,
        render_instance: None,
        world_origin: origin,
        boundary: std::sync::Arc::new(boundary),
        boundary_wcs: None,
        fill_plane: None,
        fill_plane_boundary: None,
        boundary_exterior: None,
        boundary_sources: None,
        boundary_paths: None,
        style: acadrust::entities::HatchStyleType::Normal,
        pattern: HatchPattern::Solid,
        name: "AEC_OPENING_FILL".into(),
        color: [0.55, 0.55, 0.55, 0.35],
        aci: 0,
        line_weight_px: 1.0,
        angle_offset: 0.0,
        scale: 1.0,
        draw_depth: 0.0,
    })
}

pub(crate) fn write_opening_display_tag(
    scene: &mut Scene,
    handle: Handle,
    owner: Handle,
    slot: OpeningComponentSlot,
) {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String(OPENING_REP_TAG.to_string()));
    record.add_value(XDataValue::Handle(owner));
    record.add_value(XDataValue::String(slot.key().to_string()));
    write_aec_record(&mut scene.document, handle, record);
}

pub fn opening_rep_owner_from_entity(entity: &EntityType) -> Option<Handle> {
    for record in entity.common().extended_data.records() {
        if record.application_name != AEC_APPID {
            continue;
        }
        match record.values.first() {
            Some(XDataValue::String(s)) if s == OPENING_REP_TAG => {
                return record.values.get(1).and_then(engine::xdata::aec_value_as_handle);
            }
            _ => {}
        }
    }
    None
}

pub fn opening_rep_slot_from_entity(entity: &EntityType) -> Option<OpeningComponentSlot> {
    for record in entity.common().extended_data.records() {
        if record.application_name != AEC_APPID {
            continue;
        }
        match record.values.first() {
            Some(XDataValue::String(s)) if s == OPENING_REP_TAG => {
                let key = match record.values.get(2) {
                    Some(XDataValue::String(k)) => k.as_str(),
                    _ => return None,
                };
                return OpeningComponentSlot::from_key(key);
            }
            _ => {}
        }
    }
    None
}

pub fn collect_opening_display_children(scene: &Scene, owner: Handle) -> Vec<Handle> {
    let mut out = Vec::new();
    for h in engine::owner_index::children_of(&scene.document, owner) {
        let Some(entity) = scene.document.get_entity(h) else {
            continue;
        };
        if opening_from_entity(entity, h).is_some() {
            continue;
        }
        if opening_rep_owner_from_entity(entity) == Some(owner) && !out.contains(&h) {
            out.push(h);
        }
    }
    for entity in scene.document.entities() {
        let handle = entity.common().handle;
        if handle == owner {
            continue;
        }
        if opening_rep_owner_from_entity(entity) == Some(owner) && !out.contains(&handle) {
            out.push(handle);
        }
    }
    out
}

/// Resolve an `OPENING_REP` child (or the opening POINT) to the opening owner.
pub fn resolve_opening_package(scene: &Scene, clicked: Handle) -> Handle {
    let Some(entity) = scene.document.get_entity(clicked) else {
        return clicked;
    };
    if opening_from_entity(entity, clicked).is_some() {
        return clicked;
    }
    if let Some(owner) = opening_rep_owner_from_entity(entity) {
        if scene.document.get_entity(owner).is_some() {
            return owner;
        }
    }
    for entity in scene.document.entities() {
        let owner = entity.common().handle;
        if opening_from_entity(entity, owner).is_none() {
            continue;
        }
        if engine::owner_index::children_of(&scene.document, owner)
            .iter()
            .any(|h| *h == clicked)
        {
            return owner;
        }
    }
    clicked
}

pub fn opening_owner_if_any(scene: &Scene, handle: Handle) -> Option<Handle> {
    let owner = resolve_opening_package(scene, handle);
    scene
        .document
        .get_entity(owner)
        .and_then(|e| opening_from_entity(e, owner))
        .map(|_| owner)
}

pub fn opening_package_handles(scene: &Scene, opening_handle: Handle) -> Vec<Handle> {
    let mut handles = vec![opening_handle];
    for child in collect_opening_display_children(scene, opening_handle) {
        if !handles.contains(&child) {
            handles.push(child);
        }
    }
    handles
}

/// Grips along the host axis: center (0), start jamb (1), end jamb (2), hinge flip handle (3), swing flip handle (4).
pub fn opening_axis_grips(
    axis: &[(f64, f64)],
    opening: &Opening,
    base_z: f64,
) -> Vec<crate::scene::model::object::GripDef> {
    let center_s = opening.center_along_axis();
    let Some(((cx, cy), (tx, ty))) = point_and_tangent_at_distance(axis, center_s)
    else {
        return Vec::new();
    };
    let normal = (-ty, tx);
    let offset_cx = cx + normal.0 * opening.cross_axis_offset;
    let offset_cy = cy + normal.1 * opening.cross_axis_offset;

    let hw = opening.width * 0.5;
    let center = DVec3::new(offset_cx, offset_cy, base_z);
    let start = DVec3::new(offset_cx - tx * hw, offset_cy - ty * hw, base_z);
    let end = DVec3::new(offset_cx + tx * hw, offset_cy + ty * hw, base_z);

    let swing_sign = match opening.swing_side {
        SwingSide::Exterior => 1.0,
        SwingSide::Interior => -1.0,
    };
    let hinge_sign = match opening.hinge.bearing() {
        crate::modules::aec::engine::construction_grid::TrackEnd::Start => -1.0,
        crate::modules::aec::engine::construction_grid::TrackEnd::End => 1.0,
    };

    let hinge_handle = DVec3::new(
        offset_cx + tx * (hinge_sign * hw * 0.4) + normal.0 * (swing_sign * 0.25),
        offset_cy + ty * (hinge_sign * hw * 0.4) + normal.1 * (swing_sign * 0.25),
        base_z,
    );
    let swing_handle = DVec3::new(
        offset_cx - normal.0 * (swing_sign * 0.25),
        offset_cy - normal.1 * (swing_sign * 0.25),
        base_z,
    );

    vec![
        crate::entities::common::square_grip(0, center),
        crate::entities::common::rectangle_grip(1, start, [tx as f32, ty as f32]),
        crate::entities::common::rectangle_grip(2, end, [tx as f32, ty as f32]),
        crate::entities::common::square_grip(3, hinge_handle),
        crate::entities::common::square_grip(4, swing_handle),
    ]
}

/// Apply a center/width/flip grip. `grip_id` 0 moves the opening along the axis;
/// 1/2 stretch a jamb (width + recentre); 3 flips hinge (left/right); 4 flips swing side (interior/exterior).
pub fn apply_opening_axis_grip(
    axis: &[(f64, f64)],
    opening: &mut Opening,
    grip_id: usize,
    world: DVec3,
) {
    if grip_id == 3 {
        opening.flip_hinge();
        return;
    }
    if grip_id == 4 {
        opening.flip_swing();
        return;
    }
    let Some(s) = engine::openings::distance_along_axis_from_point(axis, (world.x, world.y)) else {
        return;
    };
    match grip_id {
        0 => match opening.reference_side {
            engine::openings::OpeningReferenceSide::Center => {
                opening.distance_along_axis = s.max(0.0);
            }
            engine::openings::OpeningReferenceSide::Start => {
                opening.distance_along_axis = (s - opening.width * 0.5).max(0.0);
            }
            engine::openings::OpeningReferenceSide::End => {
                opening.distance_along_axis = (s + opening.width * 0.5).max(0.0);
            }
        },
        1 => {
            let (_start, end) = opening.axis_span();
            let new_start = s;
            let lo = new_start.min(end);
            let hi = new_start.max(end);
            let width = (hi - lo).max(1e-4);
            opening.width = width;
            let center = (lo + hi) * 0.5;
            match opening.reference_side {
                engine::openings::OpeningReferenceSide::Center => {
                    opening.distance_along_axis = center;
                }
                engine::openings::OpeningReferenceSide::Start => {
                    opening.distance_along_axis = lo;
                }
                engine::openings::OpeningReferenceSide::End => {
                    opening.distance_along_axis = hi;
                }
            }
            let (w, h) = opening.shape.lock_size(opening.width, opening.height, true);
            opening.width = w;
            opening.height = h;
        }
        2 => {
            let (start, _end) = opening.axis_span();
            let new_end = s;
            let lo = start.min(new_end);
            let hi = start.max(new_end);
            let width = (hi - lo).max(1e-4);
            opening.width = width;
            let center = (lo + hi) * 0.5;
            match opening.reference_side {
                engine::openings::OpeningReferenceSide::Center => {
                    opening.distance_along_axis = center;
                }
                engine::openings::OpeningReferenceSide::Start => {
                    opening.distance_along_axis = lo;
                }
                engine::openings::OpeningReferenceSide::End => {
                    opening.distance_along_axis = hi;
                }
            }
            let (w, h) = opening.shape.lock_size(opening.width, opening.height, true);
            opening.width = w;
            opening.height = h;
        }
        _ => {}
    }
}

pub fn host_thickness(scene: &Scene, wall_handle: Handle) -> f64 {
    scene
        .document
        .get_entity(wall_handle)
        .and_then(wall_from_entity)
        .map(|w| w.total_thickness())
        .unwrap_or(0.0)
}

pub fn host_base_z(scene: &Scene, wall_handle: Handle) -> f64 {
    scene
        .document
        .get_entity(wall_handle)
        .and_then(wall_from_entity)
        .map(|w| w.base_origin[2])
        .unwrap_or(0.0)
}

/// Move the opening POINT onto the current axis location.
pub fn sync_opening_point_to_axis(
    scene: &mut Scene,
    opening: &Opening,
    axis: &[(f64, f64)],
    base_z: f64,
) {
    let Some(((x, y), (tx, ty))) = point_and_tangent_at_distance(axis, opening.distance_along_axis) else {
        return;
    };
    let normal = (-ty, tx);
    let px = x + normal.0 * opening.cross_axis_offset;
    let py = y + normal.1 * opening.cross_axis_offset;
    if let Some(EntityType::Point(pt)) = scene.document.get_entity_mut(opening.handle) {
        pt.location.x = px;
        pt.location.y = py;
        pt.location.z = base_z;
    }
    scene.bump_entities(&[(opening.handle, crate::scene::ChangeKind::Modified)]);
}

