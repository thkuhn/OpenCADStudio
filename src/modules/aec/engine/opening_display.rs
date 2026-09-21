//! Plan-view display children for wall openings (`OPENING_REP`).
//!
//! Parametric generators bake world geometry in the opening's local box
//! (X along the host axis, Y along wall thickness). Sketch slots use the
//! two-rectangle bake in [`super::opening_sketch`]; empty sketches emit no
//! children (they do not fall back to a generator).

use std::collections::HashMap;

use acadrust::entities::{LwPolyline, LwVertex};
use acadrust::types::Vector2;
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::modules::aec::engine::display_component::{ComponentRuleSet, OpeningComponentSlot};
use crate::modules::aec::engine::opening_sketch::bake_sketch;
use crate::modules::aec::engine::opening_style::{
    apply_plan_visibility, default_slots_for_kind, effective_slots_for_plan, OpeningGenerator,
    OpeningStyle, SlotGeometry, HingeSide, DEFAULT_FRAME_THICKNESS, DEFAULT_OPENING_ANGLE_DEG,
};
use crate::modules::aec::engine::opening_xdata::{
    opening_from_entity, openings_for_host_wall, write_opening_instance,
};
use crate::modules::aec::engine::openings::{
    point_and_tangent_at_distance, Opening, OpeningKind,
};
use crate::modules::aec::engine::wall_package::resolve_wall_package;
use crate::modules::aec::engine::xdata::{
    get_wall_vertices, wall_from_entity, write_aec_record, AEC_APPID,
};
use crate::modules::aec::engine::{self, StyleLibrary};
use crate::scene::model::hatch_model::{HatchModel, HatchPattern};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;

/// XDATA kind tag on opening display children.
pub const OPENING_REP_TAG: &str = "OPENING_REP";

/// Number of chords used to approximate a swing arc.
const SWING_CHORD_COUNT: usize = 16;

/// One baked 2D primitive in **local** opening coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct BakedPath {
    pub slot: OpeningComponentSlot,
    pub points: Vec<(f64, f64)>,
    pub closed: bool,
    /// When true the closed path is also emitted as a solid hatch.
    pub filled: bool,
}

/// Parameters that drive generator bake (instance box + absolute profile).
#[derive(Debug, Clone, Copy)]
pub struct OpeningBakeParams {
    pub width: f64,
    pub thickness: f64,
    pub frame_thickness: f64,
    pub hinge: HingeSide,
    pub opening_angle_deg: f64,
    pub kind: OpeningKind,
}

impl OpeningBakeParams {
    pub fn from_opening(opening: &Opening, thickness: f64, style: Option<&OpeningStyle>) -> Self {
        let (frame_thickness, angle) = if let Some(style) = style {
            (style.frame_thickness, style.opening_angle_deg)
        } else if opening.kind == OpeningKind::Breakthrough {
            (0.0, 0.0)
        } else {
            (DEFAULT_FRAME_THICKNESS, DEFAULT_OPENING_ANGLE_DEG)
        };
        Self {
            width: opening.width,
            thickness,
            frame_thickness,
            hinge: opening.hinge,
            opening_angle_deg: angle,
            kind: opening.kind,
        }
    }
}

/// Resolved slot map + profile for an instance (style, else kind defaults).
pub fn resolved_slots(
    opening: &Opening,
    library: Option<&StyleLibrary>,
) -> (HashMap<OpeningComponentSlot, SlotGeometry>, Option<OpeningStyle>) {
    resolved_slots_for_plan(opening, library, None)
}

/// Like [`resolved_slots`], overlaying the plan-type display profile when set.
pub fn resolved_slots_for_plan(
    opening: &Opening,
    library: Option<&StyleLibrary>,
    plan_name: Option<&str>,
) -> (HashMap<OpeningComponentSlot, SlotGeometry>, Option<OpeningStyle>) {
    if let Some(id) = opening.style_id.as_deref() {
        if let Some(lib) = library {
            if let Some(style) = lib.find_opening_style(id) {
                let map: HashMap<_, _> = lib
                    .opening_styles
                    .iter()
                    .map(|s| (s.style.id.clone(), s.clone()))
                    .collect();
                let slots = effective_slots_for_plan(&map, &id.to_string(), plan_name)
                    .unwrap_or_else(|_| style.slots.clone());
                return (slots, Some(style.clone()));
            }
        }
    }
    (default_slots_for_kind(opening.kind), None)
}

fn rules_with_plan_visibility(
    opening: &Opening,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> Option<ComponentRuleSet> {
    let mut merged = rules.cloned().unwrap_or_default();
    let plan = merged.plan_name.clone();
    if let (Some(lib), Some(id)) = (library, opening.style_id.as_ref()) {
        let map: HashMap<_, _> = lib
            .opening_styles
            .iter()
            .map(|s| (s.style.id.clone(), s.clone()))
            .collect();
        apply_plan_visibility(&map, id, plan.as_deref(), &mut merged.visibility);
    }
    if rules.is_none() && merged.visibility.is_empty() {
        None
    } else {
        Some(merged)
    }
}

/// Bake generator and sketch primitives in the local opening box.
///
/// Origin is the insertion (axis point). X is along the wall, Y across
/// thickness. Outer box is `width × thickness`, inner inset is
/// `frame_thickness` in drawing units (not a fraction of width).
pub fn bake_opening_generators(
    slots: &HashMap<OpeningComponentSlot, SlotGeometry>,
    params: OpeningBakeParams,
    rules: Option<&ComponentRuleSet>,
) -> Vec<BakedPath> {
    let mut out = Vec::new();
    let mut keys: Vec<_> = slots.keys().copied().collect();
    keys.sort_by_key(|s| s.key());
    for slot in keys {
        if !rules.map_or(true, |r| r.is_opening_visible(slot)) {
            continue;
        }
        match slots.get(&slot) {
            Some(SlotGeometry::Generator(gen)) => {
                out.extend(bake_generator(slot, *gen, params));
            }
            Some(SlotGeometry::Sketch(sketch)) => {
                out.extend(bake_sketch_slot(slot, sketch, params));
            }
            None => {}
        }
    }
    out
}

fn bake_sketch_slot(
    slot: OpeningComponentSlot,
    sketch: &crate::modules::aec::engine::opening_sketch::OpeningSketch,
    p: OpeningBakeParams,
) -> Vec<BakedPath> {
    bake_sketch(sketch, p.width, p.thickness, p.frame_thickness)
        .into_iter()
        .map(|baked| path(slot, baked.points, baked.closed, false))
        .collect()
}

fn bake_generator(
    slot: OpeningComponentSlot,
    gen: OpeningGenerator,
    p: OpeningBakeParams,
) -> Vec<BakedPath> {
    if p.width <= 1e-12 || p.thickness <= 1e-12 {
        return Vec::new();
    }
    let hw = p.width * 0.5;
    let ht = p.thickness * 0.5;
    let ft = p.frame_thickness.max(0.0);
    match gen {
        OpeningGenerator::None => Vec::new(),
        OpeningGenerator::FrameRect => bake_frame_rect(slot, hw, ht, ft),
        OpeningGenerator::LeafLine => bake_leaf_line(slot, p, hw, ht, ft),
        OpeningGenerator::SwingArc => bake_swing_arc(slot, p, hw),
        OpeningGenerator::SillLines => bake_sill_lines(slot, hw, ht),
        OpeningGenerator::Cross => vec![
            path(slot, vec![(-hw, -ht), (hw, ht)], false, false),
            path(slot, vec![(-hw, ht), (hw, -ht)], false, false),
        ],
        OpeningGenerator::DiagonalFill => vec![
            path(slot, vec![(-hw, -ht), (hw, ht)], false, false),
            path(
                slot,
                vec![(-hw, -ht), (hw, -ht), (hw, ht), (-hw, ht)],
                true,
                true,
            ),
        ],
    }
}

fn path(
    slot: OpeningComponentSlot,
    points: Vec<(f64, f64)>,
    closed: bool,
    filled: bool,
) -> BakedPath {
    BakedPath {
        slot,
        points,
        closed,
        filled,
    }
}

fn bake_frame_rect(
    slot: OpeningComponentSlot,
    hw: f64,
    ht: f64,
    ft: f64,
) -> Vec<BakedPath> {
    let mut out = vec![path(
        slot,
        vec![(-hw, -ht), (hw, -ht), (hw, ht), (-hw, ht)],
        true,
        false,
    )];
    let inner_w = hw - ft;
    let inner_t = ht - ft;
    if inner_w > 1e-9 && inner_t > 1e-9 {
        out.push(path(
            slot,
            vec![
                (-inner_w, -inner_t),
                (inner_w, -inner_t),
                (inner_w, inner_t),
                (-inner_w, inner_t),
            ],
            true,
            false,
        ));
    }
    out
}

fn bake_leaf_line(
    slot: OpeningComponentSlot,
    p: OpeningBakeParams,
    hw: f64,
    _ht: f64,
    ft: f64,
) -> Vec<BakedPath> {
    match p.kind {
        OpeningKind::Door => {
            let (start, end) = leaf_segment(p.hinge, hw, p.opening_angle_deg);
            vec![path(slot, vec![start, end], false, false)]
        }
        _ => {
            let inset = ft.min(hw * 0.45);
            vec![path(
                slot,
                vec![(-hw + inset, 0.0), (hw - inset, 0.0)],
                false,
                false,
            )]
        }
    }
}

fn leaf_segment(hinge: HingeSide, hw: f64, angle_deg: f64) -> ((f64, f64), (f64, f64)) {
    let width = hw * 2.0;
    let ang = angle_deg.to_radians();
    match hinge {
        HingeSide::Left => {
            let start = (-hw, 0.0);
            let end = (-hw + width * ang.cos(), width * ang.sin());
            (start, end)
        }
        HingeSide::Right => {
            let start = (hw, 0.0);
            let end = (hw - width * ang.cos(), width * ang.sin());
            (start, end)
        }
    }
}

fn bake_swing_arc(slot: OpeningComponentSlot, p: OpeningBakeParams, hw: f64) -> Vec<BakedPath> {
    let width = hw * 2.0;
    if width <= 1e-12 {
        return Vec::new();
    }
    let ang = p.opening_angle_deg.to_radians().abs().max(1e-6);
    let n = SWING_CHORD_COUNT.max(4);
    let pts = match p.hinge {
        HingeSide::Left => {
            let cx = -hw;
            (0..=n)
                .map(|i| {
                    let t = ang * (i as f64) / (n as f64);
                    (cx + width * t.cos(), width * t.sin())
                })
                .collect()
        }
        HingeSide::Right => {
            let cx = hw;
            (0..=n)
                .map(|i| {
                    let t = ang * (i as f64) / (n as f64);
                    (cx - width * t.cos(), width * t.sin())
                })
                .collect()
        }
    };
    vec![path(slot, pts, false, false)]
}

fn bake_sill_lines(slot: OpeningComponentSlot, hw: f64, ht: f64) -> Vec<BakedPath> {
    let tick = (0.04_f64).min(ht.max(0.02));
    vec![
        path(slot, vec![(-hw, -ht), (hw, -ht)], false, false),
        path(
            slot,
            vec![(-hw, -ht - tick), (hw, -ht - tick)],
            false,
            false,
        ),
    ]
}

fn local_to_world(
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

fn transform_paths(
    paths: &[BakedPath],
    origin: (f64, f64),
    tangent: (f64, f64),
    normal: (f64, f64),
) -> Vec<BakedPath> {
    paths
        .iter()
        .map(|p| BakedPath {
            slot: p.slot,
            closed: p.closed,
            filled: p.filled,
            points: p
                .points
                .iter()
                .map(|&(x, y)| local_to_world(x, y, origin, tangent, normal))
                .collect(),
        })
        .collect()
}

/// World-space generator bake for an opening on `axis`.
pub fn bake_opening_world(
    axis: &[(f64, f64)],
    thickness: f64,
    opening: &Opening,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> Vec<BakedPath> {
    let Some(((cx, cy), (tx, ty))) = point_and_tangent_at_distance(axis, opening.distance_along_axis)
    else {
        return Vec::new();
    };
    let normal = (-ty, tx);
    let plan = rules.and_then(|r| r.plan_name.as_deref());
    let (slots, style) = resolved_slots_for_plan(opening, library, plan);
    let params = OpeningBakeParams::from_opening(opening, thickness, style.as_ref());
    let merged = rules_with_plan_visibility(opening, library, rules);
    let local = bake_opening_generators(&slots, params, merged.as_ref());
    transform_paths(&local, (cx, cy), (tx, ty), normal)
}

/// Rubber-band wires for live placement preview.
pub fn preview_opening_wires(
    axis: &[(f64, f64)],
    thickness: f64,
    opening: &Opening,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> Vec<WireModel> {
    let paths = bake_opening_world(axis, thickness, opening, library, rules);
    let mut wires = Vec::new();
    if let Some(cut) = engine::openings::opening_footprint_2d(axis, thickness, opening) {
        let mut pts: Vec<[f32; 3]> = cut
            .iter()
            .map(|&(x, y)| [x as f32, y as f32, 0.0])
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
            .map(|&(x, y)| [x as f32, y as f32, 0.0])
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

fn solid_hatch_from_ring(ring: &[(f64, f64)]) -> Option<HatchModel> {
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

/// Grips along the host axis: center (0), start jamb (1), end jamb (2).
pub fn opening_axis_grips(
    axis: &[(f64, f64)],
    opening: &Opening,
) -> Vec<crate::scene::model::object::GripDef> {
    let Some(((cx, cy), (tx, ty))) = point_and_tangent_at_distance(axis, opening.distance_along_axis)
    else {
        return Vec::new();
    };
    let hw = opening.width * 0.5;
    let center = DVec3::new(cx, cy, 0.0);
    let start = DVec3::new(cx - tx * hw, cy - ty * hw, 0.0);
    let end = DVec3::new(cx + tx * hw, cy + ty * hw, 0.0);
    vec![
        crate::entities::common::square_grip(0, center),
        crate::entities::common::rectangle_grip(1, start, [tx as f32, ty as f32]),
        crate::entities::common::rectangle_grip(2, end, [tx as f32, ty as f32]),
    ]
}

/// Apply a center/width grip. `grip_id` 0 moves the opening along the axis;
/// 1/2 stretch a jamb (width + recentre).
pub fn apply_opening_axis_grip(
    axis: &[(f64, f64)],
    opening: &mut Opening,
    grip_id: usize,
    world: DVec3,
) {
    let Some(s) = engine::openings::distance_along_axis_from_point(axis, (world.x, world.y)) else {
        return;
    };
    match grip_id {
        0 => opening.distance_along_axis = s.max(0.0),
        1 | 2 => {
            let center = opening.distance_along_axis;
            let hw = opening.width * 0.5;
            let (start, end) = if grip_id == 1 {
                (s, center + hw)
            } else {
                (center - hw, s)
            };
            let lo = start.min(end);
            let hi = start.max(end);
            let width = (hi - lo).max(1e-6);
            opening.width = width;
            opening.distance_along_axis = (lo + hi) * 0.5;
            let (w, h) = opening.shape.lock_size(opening.width, opening.height, true);
            opening.width = w;
            opening.height = h;
        }
        _ => {}
    }
}

fn host_thickness(scene: &Scene, wall_handle: Handle) -> f64 {
    scene
        .document
        .get_entity(wall_handle)
        .and_then(wall_from_entity)
        .map(|w| w.total_thickness())
        .unwrap_or(0.0)
}

/// Move the opening POINT onto the current axis location.
pub fn sync_opening_point_to_axis(scene: &mut Scene, opening: &Opening, axis: &[(f64, f64)]) {
    let Some(((x, y), _)) = point_and_tangent_at_distance(axis, opening.distance_along_axis) else {
        return;
    };
    if let Some(EntityType::Point(pt)) = scene.document.get_entity_mut(opening.handle) {
        pt.location.x = x;
        pt.location.y = y;
    }
    scene.bump_entities(&[(opening.handle, crate::scene::ChangeKind::Modified)]);
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
    let axis: Vec<(f64, f64)> = get_wall_vertices(scene, wall_handle)
        .iter()
        .map(|v| (v.x, v.y))
        .collect();
    if axis.len() >= 2 {
        sync_opening_point_to_axis(scene, &opening, &axis);
    }
    let stale = collect_opening_display_children(scene, opening_handle);
    if !stale.is_empty() {
        scene.erase_entities(&stale);
        for h in &stale {
            engine::owner_index::remove_child(&mut scene.document, opening_handle, *h);
        }
    }
    let thickness = host_thickness(scene, wall_handle);
    let paths = bake_opening_world(&axis, thickness, &opening, library, rules);
    for baked in paths {
        if baked.points.len() < 2 {
            continue;
        }
        if baked.filled {
            if let Some(model) = solid_hatch_from_ring(&baked.points) {
                let hatch = scene.add_hatch(model, None, None);
                write_opening_display_tag(scene, hatch, opening_handle, baked.slot);
                engine::owner_index::add_child(&mut scene.document, opening_handle, hatch);
            }
            continue;
        }
        let mut pl = LwPolyline::new();
        for &(x, y) in &baked.points {
            pl.add_vertex(LwVertex::new(Vector2::new(x, y)));
        }
        pl.is_closed = baked.closed;
        let handle = scene.add_entity(EntityType::LwPolyline(pl));
        write_opening_display_tag(scene, handle, opening_handle, baked.slot);
        engine::owner_index::add_child(&mut scene.document, opening_handle, handle);
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

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::LwVertex;
    use acadrust::types::Vector2;
    use crate::modules::aec::engine::library::seed_default_library;
    use crate::modules::aec::engine::opening_sketch::OpeningSketch;
    use crate::modules::aec::engine::opening_style::{
        OpeningStyle, SlotGeometry, DEFAULT_FRAME_THICKNESS, SEED_WINDOW_STYLE_ID,
    };
    use crate::modules::aec::engine::opening_xdata::place_wall_opening;
    use crate::modules::aec::engine::openings::{
        OpeningKind, DEFAULT_WINDOW_WIDTH,
    };
    use crate::modules::aec::engine::wall::{Wall, WallJustification, WallLayer};
    use crate::modules::aec::engine::wall_regen::regenerate_wall_representation;
    use crate::modules::aec::engine::xdata::wall_record_for_wall;
    use crate::scene::Scene;

    fn params(width: f64, kind: OpeningKind, hinge: HingeSide) -> OpeningBakeParams {
        OpeningBakeParams {
            width,
            thickness: 0.3,
            frame_thickness: DEFAULT_FRAME_THICKNESS,
            hinge,
            opening_angle_deg: 90.0,
            kind,
        }
    }

    fn inner_frame_half_width(paths: &[BakedPath]) -> Option<f64> {
        let frames: Vec<_> = paths
            .iter()
            .filter(|p| p.slot == OpeningComponentSlot::Frame2D && p.closed)
            .collect();
        let inner = frames.iter().min_by(|a, b| {
            let wa = a.points.iter().map(|p| p.0).fold(f64::NAN, f64::max)
                - a.points.iter().map(|p| p.0).fold(f64::NAN, f64::min);
            let wb = b.points.iter().map(|p| p.0).fold(f64::NAN, f64::max)
                - b.points.iter().map(|p| p.0).fold(f64::NAN, f64::min);
            wa.partial_cmp(&wb).unwrap()
        })?;
        let max_x = inner.points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        Some(max_x)
    }

    #[test]
    fn window_frame_thickness_does_not_scale_with_width() {
        let slots = default_slots_for_kind(OpeningKind::Window);
        let a = bake_opening_generators(&slots, params(1.2, OpeningKind::Window, HingeSide::Left), None);
        let b = bake_opening_generators(&slots, params(1.8, OpeningKind::Window, HingeSide::Left), None);
        let ia = inner_frame_half_width(&a).unwrap();
        let ib = inner_frame_half_width(&b).unwrap();
        assert!((ia - (1.2 * 0.5 - DEFAULT_FRAME_THICKNESS)).abs() < 1e-9);
        assert!((ib - (1.8 * 0.5 - DEFAULT_FRAME_THICKNESS)).abs() < 1e-9);
        let inset_a = 1.2 * 0.5 - ia;
        let inset_b = 1.8 * 0.5 - ib;
        assert!((inset_a - DEFAULT_FRAME_THICKNESS).abs() < 1e-9);
        assert!((inset_b - DEFAULT_FRAME_THICKNESS).abs() < 1e-9);
    }

    #[test]
    fn door_swing_mirrors_with_hinge() {
        let slots = default_slots_for_kind(OpeningKind::Door);
        let left = bake_opening_generators(
            &slots,
            params(0.9, OpeningKind::Door, HingeSide::Left),
            None,
        );
        let right = bake_opening_generators(
            &slots,
            params(0.9, OpeningKind::Door, HingeSide::Right),
            None,
        );
        let leaf_l = left
            .iter()
            .find(|p| p.slot == OpeningComponentSlot::Leaf2D)
            .unwrap();
        let leaf_r = right
            .iter()
            .find(|p| p.slot == OpeningComponentSlot::Leaf2D)
            .unwrap();
        assert!(leaf_l.points[0].0 < 0.0);
        assert!(leaf_r.points[0].0 > 0.0);
        assert!((leaf_l.points[1].0 + leaf_r.points[1].0).abs() < 1e-9);
        assert!(left.iter().any(|p| p.slot == OpeningComponentSlot::Swing2D));
        assert!(right.iter().any(|p| p.slot == OpeningComponentSlot::Swing2D));
    }

    #[test]
    fn breakthrough_mark2d_has_no_swing_or_frame() {
        let slots = default_slots_for_kind(OpeningKind::Breakthrough);
        let baked = bake_opening_generators(
            &slots,
            params(1.0, OpeningKind::Breakthrough, HingeSide::Left),
            None,
        );
        assert!(baked.iter().all(|p| p.slot == OpeningComponentSlot::Mark2D));
        assert!(baked
            .iter()
            .any(|p| p.slot == OpeningComponentSlot::Mark2D && p.points.len() == 2));
        assert!(!baked
            .iter()
            .any(|p| p.slot == OpeningComponentSlot::Swing2D
                || p.slot == OpeningComponentSlot::Frame2D));
    }

    #[test]
    fn display_config_can_hide_swing() {
        let slots = default_slots_for_kind(OpeningKind::Door);
        let mut rules = ComponentRuleSet::default();
        rules
            .visibility
            .insert(OpeningComponentSlot::Swing2D.key().to_string(), false);
        let baked = bake_opening_generators(
            &slots,
            params(0.9, OpeningKind::Door, HingeSide::Left),
            Some(&rules),
        );
        assert!(!baked
            .iter()
            .any(|p| p.slot == OpeningComponentSlot::Swing2D));
        assert!(baked
            .iter()
            .any(|p| p.slot == OpeningComponentSlot::Frame2D));
    }

    fn add_test_wall(scene: &mut Scene) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let mut wall = Wall::new("s", 3.0, 0);
        wall.justification = WallJustification::Center;
        wall.layers = vec![WallLayer {
            material: "Concrete".into(),
            thickness: 0.3,
            function: "Structural".into(),
            axis_offset: -0.15,
            ..WallLayer::default()
        }];
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record_for_wall(&wall);
        entity.common_mut().extended_data.add_record(record);
        let handle = scene.add_entity(entity);
        regenerate_wall_representation(scene, handle, None).expect("regen");
        handle
    }

    fn child_slots(scene: &Scene, opening: Handle) -> Vec<OpeningComponentSlot> {
        let mut slots: Vec<_> = collect_opening_display_children(scene, opening)
            .into_iter()
            .filter_map(|h| {
                scene
                    .document
                    .get_entity(h)
                    .and_then(opening_rep_slot_from_entity)
            })
            .collect();
        slots.sort_by_key(|s| s.key());
        slots
    }

    #[test]
    fn place_window_emits_opening_rep_children_and_resolves_pick() {
        let mut scene = Scene::new();
        let wall = add_test_wall(&mut scene);
        let lib = seed_default_library();
        let (opening, _) = place_wall_opening(
            &mut scene,
            wall,
            DVec3::new(1.5, 0.0, 0.0),
            OpeningKind::Window,
            Some(&lib),
            None,
            None,
        )
        .expect("place");
        let slots = child_slots(&scene, opening);
        assert!(slots.contains(&OpeningComponentSlot::Frame2D));
        assert!(slots.contains(&OpeningComponentSlot::Leaf2D));
        assert!(slots.contains(&OpeningComponentSlot::Sill2D));
        assert!(!slots.contains(&OpeningComponentSlot::Swing2D));
        let child = collect_opening_display_children(&scene, opening)[0];
        assert_eq!(resolve_opening_package(&scene, child), opening);
        assert_eq!(
            engine::wall_package::resolve_wall_package(&scene, child),
            child
        );
        let parsed = opening_from_entity(scene.document.get_entity(opening).unwrap(), opening)
            .unwrap();
        assert_eq!(parsed.style_id.as_deref(), Some(SEED_WINDOW_STYLE_ID));
        assert!((parsed.width - OpeningStyle::standard_window().default_width).abs() < 1e-12);
    }

    #[test]
    fn place_breakthrough_has_mark_not_frame() {
        let mut scene = Scene::new();
        let wall = add_test_wall(&mut scene);
        let lib = seed_default_library();
        let (opening, _) = place_wall_opening(
            &mut scene,
            wall,
            DVec3::new(2.0, 0.0, 0.0),
            OpeningKind::Breakthrough,
            Some(&lib),
            None,
            None,
        )
        .expect("place");
        let slots = child_slots(&scene, opening);
        assert!(slots.contains(&OpeningComponentSlot::Mark2D));
        assert!(!slots.contains(&OpeningComponentSlot::Frame2D));
        assert!(!slots.contains(&OpeningComponentSlot::Swing2D));
    }

    #[test]
    fn legacy_opening_without_style_id_uses_kind_generators() {
        let mut scene = Scene::new();
        let wall = add_test_wall(&mut scene);
        let (opening, _) = place_wall_opening(
            &mut scene,
            wall,
            DVec3::new(1.5, 0.0, 0.0),
            OpeningKind::Window,
            None,
            None,
            None,
        )
        .expect("place");
        let parsed = opening_from_entity(scene.document.get_entity(opening).unwrap(), opening)
            .unwrap();
        assert_eq!(parsed.style_id, None);
        assert!((parsed.width - DEFAULT_WINDOW_WIDTH).abs() < 1e-12);
        let slots = child_slots(&scene, opening);
        assert!(slots.contains(&OpeningComponentSlot::Frame2D));
        assert!(slots.contains(&OpeningComponentSlot::Sill2D));
    }

    #[test]
    fn axis_width_grip_keeps_frame_inset() {
        let axis = vec![(0.0, 0.0), (5.0, 0.0)];
        let mut opening = Opening::window(Handle::new(1), Handle::new(2), 2.0);
        opening.width = 1.2;
        apply_opening_axis_grip(&axis, &mut opening, 2, DVec3::new(3.2, 0.0, 0.0));
        assert!((opening.width - 1.8).abs() < 1e-9);
        let slots = default_slots_for_kind(OpeningKind::Window);
        let baked = bake_opening_generators(
            &slots,
            params(opening.width, OpeningKind::Window, HingeSide::Left),
            None,
        );
        let inner = inner_frame_half_width(&baked).unwrap();
        assert!((opening.width * 0.5 - inner - DEFAULT_FRAME_THICKNESS).abs() < 1e-9);
    }

    #[test]
    fn sketch_slot_emits_children_empty_does_not_fall_back() {
        let mut slots = default_slots_for_kind(OpeningKind::Window);
        slots.insert(
            OpeningComponentSlot::Frame2D,
            SlotGeometry::Sketch(OpeningSketch::default()),
        );
        let empty = bake_opening_generators(
            &slots,
            params(1.2, OpeningKind::Window, HingeSide::Left),
            None,
        );
        assert!(empty
            .iter()
            .all(|p| p.slot != OpeningComponentSlot::Frame2D));
        assert!(empty
            .iter()
            .any(|p| p.slot == OpeningComponentSlot::Leaf2D));

        slots.insert(
            OpeningComponentSlot::Frame2D,
            SlotGeometry::Sketch(OpeningSketch::frame_ring(1.0, 0.3, DEFAULT_FRAME_THICKNESS)),
        );
        let mut p = params(1.2, OpeningKind::Window, HingeSide::Left);
        p.thickness = 0.24;
        let filled = bake_opening_generators(&slots, p, None);
        let frames: Vec<_> = filled
            .iter()
            .filter(|b| b.slot == OpeningComponentSlot::Frame2D)
            .collect();
        assert_eq!(frames.len(), 2);
        let inner = inner_frame_half_width(&filled).unwrap();
        assert!((1.2 * 0.5 - inner - DEFAULT_FRAME_THICKNESS).abs() < 1e-9);
    }
}
