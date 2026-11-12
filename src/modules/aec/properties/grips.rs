use super::*;

pub fn append_section_marker_grips(
    scene: &crate::scene::Scene,
    handle: Handle,
    entity_grips: &mut Vec<crate::scene::model::object::GripDef>,
) {
    let marker_handle = resolve_section_marker_package(scene, handle).unwrap_or(handle);
    let Some(entity) = scene.document.get_entity(marker_handle) else {
        return;
    };
    let Some(marker) = crate::modules::aec::views::din1356::read_section_marker(&scene.document, entity) else {
        return;
    };

    let segments = marker.path.segments();
    if segments.is_empty() {
        return;
    }

    entity_grips.clear();

    let z = match entity {
        EntityType::LwPolyline(pl) => pl.elevation,
        _ => 0.0,
    };

    let first = &segments[0];
    let last = &segments[segments.len() - 1];

    // 0: Start grip
    entity_grips.push(crate::entities::common::square_grip(
        SECTION_MARKER_START_GRIP,
        glam::DVec3::new(first.start.x_m, first.start.y_m, z),
    ));

    // 1: End grip
    entity_grips.push(crate::entities::common::square_grip(
        SECTION_MARKER_END_GRIP,
        glam::DVec3::new(last.end.x_m, last.end.y_m, z),
    ));

    // 100+: Intermediate jog transition grips
    for i in 0..segments.len().saturating_sub(1) {
        let jog_pt = glam::DVec3::new(segments[i].end.x_m, segments[i].end.y_m, z);
        entity_grips.push(crate::entities::common::square_grip(
            SECTION_MARKER_JOG_BASE + i,
            jog_pt,
        ));
    }

    // Midpoint of the cut path
    let mid_x = (first.start.x_m + last.end.x_m) * 0.5;
    let mid_y = (first.start.y_m + last.end.y_m) * 0.5;
    let mid = glam::DVec3::new(mid_x, mid_y, z);

    let dir = first.direction().unwrap_or(crate::modules::aec::engine::views::Vector2::new(1.0, 0.0));
    let normal = if !marker.flip_direction {
        crate::modules::aec::engine::views::Vector2::new(-dir.y, dir.x)
    } else {
        crate::modules::aec::engine::views::Vector2::new(dir.y, -dir.x)
    };

    // 2: Depth grip along normal
    if let Some(depth) = marker.far_depth {
        let depth_pos = mid + glam::DVec3::new(normal.x * depth, normal.y * depth, 0.0);
        let mut depth_grip = crate::entities::common::rectangle_grip(
            SECTION_MARKER_DEPTH_GRIP,
            depth_pos,
            [normal.x as f32, normal.y as f32],
        );
        depth_grip.axis = Some(glam::DVec3::new(normal.x, normal.y, 0.0));
        entity_grips.push(depth_grip);
    }

    // 3: Flip grip pointing along normal
    let flip_offset = 1.0;
    let flip_pos = mid + glam::DVec3::new(normal.x * flip_offset, normal.y * flip_offset, 0.0);
    let flip_grip = crate::entities::common::oriented_triangle_grip(
        SECTION_MARKER_FLIP_GRIP,
        flip_pos,
        glam::DVec3::new(normal.x, normal.y, 0.0),
    );
    entity_grips.push(flip_grip);
}

pub const DERIVED_VIEW_GRIP_BASE: usize = 50_000;

/// Appends interactive semantic model grips for derived section primitives.
pub fn append_derived_view_grips(
    scene: &crate::scene::Scene,
    handle: Handle,
    entity_grips: &mut Vec<crate::scene::model::object::GripDef>,
) {
    let Some(entity) = scene.document.get_entity(handle) else {
        return;
    };
    let Some(meta) = crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(entity) else {
        return;
    };
    let Some(feat) = meta.feature else {
        return;
    };
    let is_horizontal_grip = matches!(
        feat,
        crate::modules::aec::engine::views::provenance::FeatureKey::OpeningLeftJamb
            | crate::modules::aec::engine::views::provenance::FeatureKey::OpeningRightJamb
    );

    match feat {
        crate::modules::aec::engine::views::provenance::FeatureKey::WallTop
        | crate::modules::aec::engine::views::provenance::FeatureKey::WallBase
        | crate::modules::aec::engine::views::provenance::FeatureKey::OpeningSill
        | crate::modules::aec::engine::views::provenance::FeatureKey::OpeningHead
        | crate::modules::aec::engine::views::provenance::FeatureKey::OpeningLeftJamb
        | crate::modules::aec::engine::views::provenance::FeatureKey::OpeningRightJamb
        | crate::modules::aec::engine::views::provenance::FeatureKey::SlabTop
        | crate::modules::aec::engine::views::provenance::FeatureKey::SlabBottom => {}
        _ => return,
    }

    let pos = match entity {
        EntityType::Line(l) => {
            glam::DVec3::new(
                (l.start.x + l.end.x) * 0.5,
                (l.start.y + l.end.y) * 0.5,
                (l.start.z + l.end.z) * 0.5,
            )
        }
        EntityType::LwPolyline(pl) => {
            if pl.vertices.is_empty() {
                return;
            }
            let mut sx = 0.0;
            let mut sy = 0.0;
            for v in &pl.vertices {
                sx += v.location.x;
                sy += v.location.y;
            }
            let n = pl.vertices.len() as f64;
            glam::DVec3::new(sx / n, sy / n, pl.elevation)
        }
        _ => return,
    };

    let carrier = meta
        .carrier_handle
        .and_then(|ch| scene.document.get_entity(ch))
        .and_then(|e| crate::modules::aec::engine::views::carrier::read_view_carrier(&scene.document, e))
        .or_else(|| crate::modules::aec::engine::views::carrier::find_view_carrier_owning_handle(&scene.document, handle));

    let rot = carrier.as_ref().map(|c| c.rotation_rad).unwrap_or(0.0);
    let cos_r = rot.cos();
    let sin_r = rot.sin();

    let is_vertical_view = carrier
        .as_ref()
        .and_then(|c| crate::modules::aec::engine::views::carrier::read_view_carrier_definitions(&scene.document, c.handle))
        .map(|(vp, _)| matches!(vp.kind, crate::modules::aec::engine::views::viewpoint::ViewKind::Section { .. } | crate::modules::aec::engine::views::viewpoint::ViewKind::Elevation { .. }))
        .unwrap_or(true);

    let axis_dir = if is_horizontal_grip {
        if is_vertical_view {
            let rx = cos_r;
            let ry = sin_r;
            glam::DVec3::new(rx, ry, 0.0)
        } else {
            // In a horizontal view (FloorPlan / RCP), opening jambs move along the 2D host wall axis.
            let world_wall_dir = (|| {
                let elem_ref = meta.element_ref?;
                let uuid = elem_ref.element_id.as_uuid();
                let bytes = uuid.as_bytes();
                let handle_val = u64::from_be_bytes(bytes[8..16].try_into().ok()?);
                let op_handle = Handle::new(handle_val);
                let op_ent = scene.document.get_entity(op_handle)?;
                let opening = crate::modules::aec::engine::opening_xdata::opening_from_entity(op_ent, op_handle)?;
                let wall = crate::modules::aec::engine::wall_package::resolve_wall_package(scene, opening.host_wall);
                let pts = crate::modules::aec::engine::xdata::get_wall_vertices(scene, wall);
                if pts.len() >= 2 {
                    let dx = pts[1].x - pts[0].x;
                    let dy = pts[1].y - pts[0].y;
                    let len = dx.hypot(dy);
                    if len > 1e-6 {
                        return Some((dx / len, dy / len));
                    }
                }
                None
            })();

            if let Some((dir_x, dir_y)) = world_wall_dir {
                let rx = cos_r * dir_x - sin_r * dir_y;
                let ry = sin_r * dir_x + cos_r * dir_y;
                let v = glam::DVec3::new(rx, ry, 0.0);
                if v.length_squared() > 1e-6 {
                    v.normalize()
                } else {
                    glam::DVec3::new(1.0, 0.0, 0.0)
                }
            } else if let EntityType::Line(l) = entity {
                // Fallback: the jamb line segment in CAD space is perpendicular to the wall axis.
                let jx = l.end.x - l.start.x;
                let jy = l.end.y - l.start.y;
                let j_len = jx.hypot(jy);
                if j_len > 1e-6 {
                    glam::DVec3::new(-jy / j_len, jx / j_len, 0.0)
                } else {
                    glam::DVec3::new(cos_r, sin_r, 0.0)
                }
            } else {
                glam::DVec3::new(cos_r, sin_r, 0.0)
            }
        }
    } else {
        let rx = -sin_r;
        let ry = cos_r;
        glam::DVec3::new(rx, ry, 0.0)
    };

    entity_grips.clear();
    let mut grip = crate::entities::common::square_grip(DERIVED_VIEW_GRIP_BASE, pos);
    grip.axis = Some(axis_dir);
    entity_grips.push(grip);

    match feat {
        crate::modules::aec::engine::views::provenance::FeatureKey::OpeningLeftJamb
        | crate::modules::aec::engine::views::provenance::FeatureKey::OpeningRightJamb
        | crate::modules::aec::engine::views::provenance::FeatureKey::SlabBottom => {
            let dropdown = crate::entities::common::dropdown_grip(DERIVED_VIEW_DROPDOWN_GRIP_BASE, pos);
            entity_grips.push(dropdown);
        }
        _ => {}
    }
}

pub const DERIVED_VIEW_DROPDOWN_GRIP_BASE: usize = 50_100;
pub const DERIVED_VIEW_RESIZE_GRIP_BASE: usize = 50_200;
pub const OPENING_DROPDOWN_GRIP_ID: usize = 50_300;
pub const WALL_BASE_DROPDOWN_GRIP_ID: usize = 50_400;
pub const SLAB_BOTTOM_DROPDOWN_GRIP_ID: usize = 50_500;

pub fn opening_handle_from_derived_primitive(doc: &acadrust::CadDocument, derived_handle: Handle) -> Option<Handle> {
    let ent = doc.get_entity(derived_handle)?;
    let meta = crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(ent)?;
    let elem_ref = meta.element_ref?;
    let uuid = elem_ref.element_id.as_uuid();
    let bytes = uuid.as_bytes();
    let handle_val = u64::from_be_bytes(bytes[8..16].try_into().ok()?);
    Some(Handle::new(handle_val))
}

pub fn carrier_handle_from_derived_primitive(doc: &acadrust::CadDocument, derived_handle: Handle) -> Option<Handle> {
    let ent = doc.get_entity(derived_handle)?;
    let meta = crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(ent)?;
    meta.carrier_handle.or_else(|| crate::modules::aec::engine::views::carrier::find_view_carrier_owning_handle(doc, derived_handle).map(|c| c.handle))
}

pub const CARRIER_GRIP_INSERTION: usize = 60_000;
pub const CARRIER_GRIP_CUT_HEIGHT: usize = 60_001;

/// Appends interactive grips for a `DerivedViewCarrier` entity (insertion relocation and cut height).
pub fn append_view_carrier_grips(
    scene: &crate::scene::Scene,
    handle: Handle,
    entity_grips: &mut Vec<crate::scene::model::object::GripDef>,
) {
    let handle = crate::modules::aec::engine::views::carrier::resolve_carrier_handle(&scene.document, handle);
    let Some(entity) = scene.document.get_entity(handle) else {
        return;
    };
    let Some(carrier) = crate::modules::aec::engine::views::carrier::read_view_carrier(&scene.document, entity) else {
        return;
    };

    // Grip 0: Insertion point (to move the drawing)
    let ins = glam::DVec3::new(carrier.insertion_point.x_m, carrier.insertion_point.y_m, 0.0);
    entity_grips.push(crate::entities::common::square_grip(CARRIER_GRIP_INSERTION, ins));

    // Grip 1: Cut height adjustment for FloorPlan / RCP
    let defs = crate::modules::aec::engine::views::carrier::read_view_carrier_definitions(&scene.document, carrier.handle);
    if let Some((vp, _)) = defs {
        match &vp.kind {
            crate::modules::aec::engine::views::viewpoint::ViewKind::FloorPlan { range } => {
                let cut_h = (vp.z_range.z_min.get() + range.below_cut.get())
                    .clamp(vp.z_range.z_min.get(), vp.z_range.z_max.get());
                let cut_pos = glam::DVec3::new(carrier.insertion_point.x_m, carrier.insertion_point.y_m + cut_h, 0.0);
                let mut grip = crate::entities::common::round_grip(CARRIER_GRIP_CUT_HEIGHT, cut_pos);
                grip.axis = Some(glam::DVec3::new(0.0, 1.0, 0.0));
                entity_grips.push(grip);
            }
            crate::modules::aec::engine::views::viewpoint::ViewKind::ReflectedCeilingPlan { range } => {
                let cut_h = (vp.z_range.z_max.get() - range.above_cut.get())
                    .clamp(vp.z_range.z_min.get(), vp.z_range.z_max.get());
                let cut_pos = glam::DVec3::new(carrier.insertion_point.x_m, carrier.insertion_point.y_m + cut_h, 0.0);
                let mut grip = crate::entities::common::round_grip(CARRIER_GRIP_CUT_HEIGHT, cut_pos);
                grip.axis = Some(glam::DVec3::new(0.0, 1.0, 0.0));
                entity_grips.push(grip);
            }
            _ => {}
        }
    }
}

pub fn append_opening_axis_grips(
    scene: &crate::scene::Scene,
    handle: Handle,
    entity_grips: &mut Vec<crate::scene::model::object::GripDef>,
) {
    let Some(owner) = opening_display::opening_owner_if_any(scene, handle) else {
        return;
    };
    let Some(entity) = scene.document.get_entity(owner) else {
        return;
    };
    let Some(opening) = opening_xdata::opening_from_entity(entity, owner) else {
        return;
    };
    let wall = wall_package::resolve_wall_package(scene, opening.host_wall);
    let base_z = opening_display::host_base_z(scene, wall);
    let axis: Vec<(f64, f64)> = xdata::get_wall_vertices(scene, wall)
        .iter()
        .map(|v| (v.x, v.y))
        .collect();
    let grips = opening_display::opening_axis_grips(&axis, &opening, base_z);
    if grips.is_empty() {
        return;
    }
    entity_grips.clear();
    entity_grips.extend(grips);
    let center = if let Some(((cx, cy), _)) = crate::modules::aec::engine::openings::point_and_tangent_at_distance(&axis, opening.center_along_axis()) {
        glam::DVec3::new(cx, cy, base_z)
    } else {
        glam::DVec3::ZERO
    };
    entity_grips.push(crate::entities::common::dropdown_grip(OPENING_DROPDOWN_GRIP_ID, center));
}

pub const ROOM_STAMP_GRIP_ID: usize = usize::MAX - 10;

pub fn append_room_grips(
    scene: &crate::scene::Scene,
    handle: Handle,
    _entity: &EntityType,
    entity_grips: &mut Vec<crate::scene::model::object::GripDef>,
) {
    let owner = room_package::resolve_room_package_handle(scene, handle);
    let Some(carrier) = scene.document.get_entity(owner) else {
        return;
    };
    let Some(room) = room_xdata::room_from_entity(carrier) else {
        return;
    };

    let (sx, sy) = room.stamp_pos.unwrap_or_else(|| {
        let pts = room_regen::room_boundary_points(carrier);
        if pts.len() >= 3 {
            crate::modules::aec::engine::geometry::centroid(&pts)
        } else {
            (0.0, 0.0)
        }
    });

    let z = match carrier {
        EntityType::LwPolyline(pl) => pl.elevation,
        _ => 0.0,
    };

    entity_grips.push(crate::entities::common::round_grip(
        ROOM_STAMP_GRIP_ID,
        glam::DVec3::new(sx, sy, z),
    ));
}

/// Appends all AEC grips (walls, slabs, openings, rooms, derived views).
pub fn append_aec_grips(
    scene: &crate::scene::Scene,
    handle: Handle,
    entity: &EntityType,
    entity_grips: &mut Vec<crate::scene::model::object::GripDef>,
) {
    append_wall_junction_grips(scene, handle, entity, entity_grips);
    append_slab_base_grips(scene, handle, entity, entity_grips);
    append_opening_axis_grips(scene, handle, entity_grips);
    append_room_grips(scene, handle, entity, entity_grips);
    append_section_marker_grips(scene, handle, entity_grips);
    append_derived_view_grips(scene, handle, entity_grips);
    append_view_carrier_grips(scene, handle, entity_grips);
}

pub fn append_wall_junction_grips(
    scene: &crate::scene::Scene,
    handle: Handle,
    entity: &EntityType,
    entity_grips: &mut Vec<crate::scene::model::object::GripDef>,
) {
    if xdata::wall_from_entity(entity).is_none() {
        return;
    }
    let verts = xdata::get_wall_vertices(scene, handle);
    if verts.len() < 2 {
        return;
    }
    for (end_index, world) in [(0usize, verts[0]), (1usize, verts[verts.len() - 1])] {
        let participants = join_ops::walls_at_junction(scene, handle, end_index);
        if participants.len() > 1 {
            entity_grips.push(crate::entities::common::dropdown_grip(
                junction_pick::wall_junction_dropdown_grip_id(end_index),
                world,
            ));
        }
    }
}

pub fn append_slab_base_grips(
    _scene: &crate::scene::Scene,
    _handle: Handle,
    entity: &EntityType,
    entity_grips: &mut Vec<crate::scene::model::object::GripDef>,
) {
    if slab_xdata::slab_from_entity(entity).is_none() {
        return;
    }
    let pos = match entity {
        EntityType::LwPolyline(pl) if !pl.vertices.is_empty() => {
            glam::DVec3::new(pl.vertices[0].location.x, pl.vertices[0].location.y, pl.elevation)
        }
        _ => return,
    };
    entity_grips.push(crate::entities::common::dropdown_grip(
        SLAB_BOTTOM_DROPDOWN_GRIP_ID,
        pos,
    ));
}
