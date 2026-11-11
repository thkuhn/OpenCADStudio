use super::*;

pub(super) fn opening_look_line_style(
    style: &OpeningStyle,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
    role: OpeningLookRole,
    slot: OpeningComponentSlot,
) -> (Option<String>, Option<acadrust::types::Color>) {
    let look = style.looks.get(role);
    let material = style
        .material_id_for_look(role)
        .and_then(|id| library?.materials.iter().find(|material| material.id == id));
    let mut line_type = look
        .line_type
        .clone()
        .or_else(|| material.map(|material| material.line_type.clone()));
    let mut line_color = look.line_color.or_else(|| {
        material.map(|material| {
            let rgb = material.line_color;
            acadrust::types::Color::Rgb {
                r: ((rgb >> 16) & 0xff) as u8,
                g: ((rgb >> 8) & 0xff) as u8,
                b: (rgb & 0xff) as u8,
            }
        })
    });
    if let Some(override_style) = rules.and_then(|rules| rules.style_override.get(slot.key())) {
        if override_style.line_type.is_some() {
            line_type = override_style.line_type.clone();
        }
        if override_style.line_color.is_some() {
            line_color = override_style.line_color;
        }
    }
    (line_type, line_color)
}

pub const AEC_OPENING_GRID_LAYER: &str = "AEC_OPENING_GRID";

pub(super) fn ensure_opening_grid_layer(scene: &mut Scene, prefix: Option<&str>) -> String {
    let name = crate::modules::aec::engine::wall_package::prefix_layer(AEC_OPENING_GRID_LAYER, prefix);
    let existed = scene.document.layers.contains(&name);
    scene.ensure_layer(&name);
    if !existed {
        if let Some(layer) = scene.document.layers.get_mut(&name) {
            layer.color = acadrust::types::Color::Index(8);
            layer.flags.off = false;
            layer.is_plottable = true;
        }
    }
    name
}

pub(super) fn apply_opening_look_to_entity(
    entity: &mut EntityType,
    style: &OpeningStyle,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
    role: OpeningLookRole,
    slot: OpeningComponentSlot,
) {
    let (line_type, line_color) = opening_look_line_style(style, library, rules, role, slot);
    if let Some(line_type) = line_type {
        entity.common_mut().linetype = line_type;
    }
    if let Some(line_color) = line_color {
        entity.common_mut().color = line_color;
    }
}

pub(super) fn opening_native_material_handle(
    style: &OpeningStyle,
    library: Option<&StyleLibrary>,
    role: OpeningLookRole,
    scene: &Scene,
) -> Option<Handle> {
    let material_id = style.material_id_for_look(role)?;
    let material = library?
        .materials
        .iter()
        .find(|material| material.id == material_id)?;
    let names = [
        Some(material_id),
        Some(material.name.as_str()),
        material.render_material_ref.as_deref(),
    ];
    scene.document.objects.iter().find_map(|(handle, object)| {
        let acadrust::objects::ObjectType::Material(native) = object else {
            return None;
        };
        names
            .iter()
            .flatten()
            .any(|name| native.name.eq_ignore_ascii_case(name))
            .then_some(*handle)
    })
}

pub(super) fn emit_opening_solid_3d(
    scene: &mut Scene,
    body: cadkernel::brep::Body,
    opening_handle: Handle,
    opening_owner_handle: Handle,
    opening_layer: Option<&str>,
    origin: (f64, f64),
    tangent: (f64, f64),
    normal: (f64, f64),
    base_z: f64,
    offset_x: f64,
    offset_z: f64,
    slot: OpeningComponentSlot,
    look_role: OpeningLookRole,
    style: &OpeningStyle,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) {
    let Some(world_body) = crate::scene::model::solid_model::placed(
        &body,
        [tangent.0, tangent.1, 0.0],
        [normal.0, normal.1, 0.0],
        [0.0, 0.0, 1.0],
        [
            origin.0 + tangent.0 * offset_x,
            origin.1 + tangent.1 * offset_x,
            base_z + offset_z,
        ],
    ) else {
        return;
    };
    let solid = crate::modules::aec::engine::geometry::create_solid3d_from_body(&world_body);
    let mut entity = EntityType::Solid3D(solid);
    if !opening_owner_handle.is_null()
        && opening_owner_handle != scene.document.header.model_space_block_handle
    {
        entity.common_mut().owner_handle = opening_owner_handle;
    }
    if let Some(layer) = opening_layer {
        entity.as_entity_mut().set_layer(layer.to_string());
    }
    apply_opening_look_to_entity(&mut entity, style, library, rules, look_role, slot);
    if let Some(material_handle) = opening_native_material_handle(style, library, look_role, scene) {
        let common = entity.common_mut();
        common.material_flags = 3;
        common.material_handle = Some(material_handle);
    }
    let handle = scene.add_entity(entity);
    if let Some(display) = scene.prepare_solid_model_display(handle, &world_body) {
        scene.register_prepared_solid_model(handle, world_body, display);
    }
    write_opening_display_tag(scene, handle, opening_handle, slot);
    engine::owner_index::add_child(&mut scene.document, opening_handle, handle);
}

pub(super) fn emit_opening_accessory_3d(
    scene: &mut Scene,
    body: Option<cadkernel::brep::Body>,
    opening_handle: Handle,
    opening_owner_handle: Handle,
    opening_layer: Option<&str>,
    origin: (f64, f64),
    tangent: (f64, f64),
    normal: (f64, f64),
    base_z: f64,
    offset_x: f64,
    offset_z: f64,
    role: OpeningLookRole,
    style: &OpeningStyle,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) {
    if let Some(body) = body {
        emit_opening_solid_3d(
            scene,
            body,
            opening_handle,
            opening_owner_handle,
            opening_layer,
            origin,
            tangent,
            normal,
            base_z,
            offset_x,
            offset_z,
            OpeningComponentSlot::Solid3D,
            role,
            style,
            library,
            rules,
        );
    }
}

pub(crate) fn sill_x_span(half_width: f64, left: f64, right: f64) -> (f64, f64) {
    (-half_width - left.max(0.0), half_width + right.max(0.0))
}

pub(crate) fn wall_clamped_interval(wall_min: f64, wall_max: f64, start: f64, depth: f64) -> (f64, f64) {
    let lo = wall_min.min(wall_max);
    let hi = wall_min.max(wall_max);
    let start = start.clamp(lo, hi);
    let end = (start + depth.max(0.0)).clamp(lo, hi);
    (start.min(end), start.max(end))
}

pub(crate) fn accessory_bottom(head: f64, seat: f64) -> f64 {
    head + seat.max(0.0)
}

pub(crate) fn lintel_depth_interval(wall_min: f64, wall_max: f64, depth: f64) -> (f64, f64) {
    let lo = wall_min.min(wall_max);
    let hi = wall_min.max(wall_max);
    let depth = depth.max(0.0).min((hi - lo).max(0.0));
    let center = (lo + hi) * 0.5;
    (center - depth * 0.5, center + depth * 0.5)
}

pub(crate) fn exterior_sill_y_bounds(
    wall_y_min: f64,
    wall_y_max: f64,
    overhang: f64,
    frame_depth: f64,
) -> (f64, f64) {
    let wall_min = wall_y_min.min(wall_y_max);
    let wall_max = wall_y_min.max(wall_y_max);
    let frame_outer = (-frame_depth.max(0.0) * 0.5).clamp(wall_min, wall_max);
    (wall_min - overhang.max(0.0), frame_outer)
}

pub(crate) fn interior_board_y_bounds(
    wall_y_min: f64,
    wall_y_max: f64,
    overhang: f64,
    frame_depth: f64,
) -> (f64, f64) {
    let wall_min = wall_y_min.min(wall_y_max);
    let wall_max = wall_y_min.max(wall_y_max);
    let frame_inner = (frame_depth.max(0.0) * 0.5).clamp(wall_min, wall_max);
    (frame_inner, wall_max + overhang.max(0.0))
}

/// Depth interval (min, max) of a sill/board upstand: it stands on the sill at
/// the outer face of the fixed frame and runs `away` (-1 exterior, +1
/// interior) from `frame_edge`, at most as deep as the frame.
pub(crate) fn sill_upstand_interval(
    frame_edge: f64,
    away: f64,
    section_depth: f64,
    frame_depth: f64,
) -> (f64, f64) {
    let depth = section_depth.min(frame_depth).max(0.0);
    let far = frame_edge + away * depth;
    (far.min(frame_edge), far.max(frame_edge))
}

pub fn build_opening_glazing_3d(
    width: f64,
    height: f64,
    frame_thickness: f64,
    shape: OpeningShape,
    spring_height: f64,
) -> Option<cadkernel::brep::Body> {
    let hw = width * 0.5;
    let ft = frame_thickness.max(0.02).min(hw * 0.45);
    let (_outer, inner) = opening_elevation_loops(shape, width, height, spring_height, ft, false);
    let half_t = 0.01;
    build_faceted_prism(&inner, -half_t, half_t)
}

pub(super) fn local_to_world(
    lx: f64,
    ly: f64,
    origin: (f64, f64),
    tangent: (f64, f64),
    normal: (f64, f64),
) -> (f64, f64) {
    (
        origin.0 + tangent.0 * lx + normal.0 * ly,
        origin.1 + tangent.1 * lx + normal.1 * ly,
    )
}

pub(super) fn transform_paths(
    paths: &[BakedPath],
    origin: (f64, f64),
    tangent: (f64, f64),
    normal: (f64, f64),
) -> Vec<BakedPath> {
    paths
        .iter()
        .map(|p| {
            if p.slot.is_elevation() {
                BakedPath {
                    slot: p.slot,
                    look_role: p.look_role,
                    closed: p.closed,
                    filled: p.filled,
                    points: p
                        .points
                        .iter()
                        .map(|&(u, v)| (origin.0 + tangent.0 * u, origin.1 + tangent.1 * u + v))
                        .collect(),
                    points_3d: p.points_3d.clone(),
                }
            } else {
                BakedPath {
                    slot: p.slot,
                    look_role: p.look_role,
                    closed: p.closed,
                    filled: p.filled,
                    points: p
                        .points
                        .iter()
                        .map(|&(x, y)| local_to_world(x, y, origin, tangent, normal))
                        .collect(),
                    points_3d: p.points_3d.clone(),
                }
            }
        })
        .collect()
}

