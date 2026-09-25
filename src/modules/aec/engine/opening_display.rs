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
use crate::modules::aec::engine::opening_shape::OpeningShape;
use crate::modules::aec::engine::opening_sketch::bake_sketch;
use crate::modules::aec::engine::opening_style::{
    apply_plan_visibility, default_slots_for_kind, effective_slots_for_plan, BlockPlacementMode,
    OpeningGenerator, OpeningStyle, SlotGeometry, HingeSide, DEFAULT_FRAME_THICKNESS,
    DEFAULT_OPENING_ANGLE_DEG,
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
    pub height: f64,
    pub sill_height: f64,
    pub thickness: f64,
    pub frame_thickness: f64,
    pub cross_axis_offset: f64,
    pub wall_y_min: f64,
    pub wall_y_max: f64,
    pub hinge: HingeSide,
    pub shape: OpeningShape,
    pub spring_height: f64,
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
        let ht = thickness * 0.5;
        Self {
            width: opening.width,
            height: opening.height,
            sill_height: opening.sill_height,
            thickness,
            frame_thickness,
            cross_axis_offset: opening.cross_axis_offset,
            wall_y_min: -ht,
            wall_y_max: ht,
            hinge: opening.hinge,
            shape: opening.shape,
            spring_height: opening.spring_height,
            opening_angle_deg: angle,
            kind: opening.kind,
        }
    }

    pub fn with_wall_bounds(mut self, y_min: f64, y_max: f64) -> Self {
        self.wall_y_min = y_min;
        self.wall_y_max = y_max;
        self
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
    bake_opening_generators_with_doc(slots, params, rules, None)
}

pub fn bake_opening_generators_with_doc(
    slots: &HashMap<OpeningComponentSlot, SlotGeometry>,
    params: OpeningBakeParams,
    rules: Option<&ComponentRuleSet>,
    doc: Option<&acadrust::CadDocument>,
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
            Some(SlotGeometry::Block {
                block_name,
                placement,
            }) => {
                out.extend(bake_block_slot(slot, block_name, *placement, params, doc));
            }
            Some(SlotGeometry::Sketch(sketch)) => {
                out.extend(bake_sketch_slot(slot, sketch, params));
            }
            None => {}
        }
    }
    out
}

pub fn extract_block_paths(
    doc: &acadrust::CadDocument,
    block_name: &str,
) -> Vec<(Vec<(f64, f64)>, bool)> {
    let Some(rec) = doc.block_records.get(block_name) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for entity in doc.entities() {
        if entity.common().owner_handle != rec.handle {
            continue;
        }
        match entity {
            EntityType::Line(line) => {
                paths.push((
                    vec![(line.start.x, line.start.y), (line.end.x, line.end.y)],
                    false,
                ));
            }
            EntityType::LwPolyline(pl) => {
                if pl.vertices.len() >= 2 {
                    let pts: Vec<(f64, f64)> = pl
                        .vertices
                        .iter()
                        .map(|v| (v.location.x, v.location.y))
                        .collect();
                    paths.push((pts, pl.is_closed));
                }
            }
            EntityType::Arc(arc) => {
                let n = 16;
                let start_a = arc.start_angle.to_radians();
                let end_a = arc.end_angle.to_radians();
                let sweep = if end_a >= start_a {
                    end_a - start_a
                } else {
                    end_a + 2.0 * std::f64::consts::PI - start_a
                };
                let pts: Vec<(f64, f64)> = (0..=n)
                    .map(|i| {
                        let a = start_a + sweep * (i as f64) / (n as f64);
                        (
                            arc.center.x + arc.radius * a.cos(),
                            arc.center.y + arc.radius * a.sin(),
                        )
                    })
                    .collect();
                paths.push((pts, false));
            }
            EntityType::Circle(circ) => {
                let n = 32;
                let pts: Vec<(f64, f64)> = (0..=n)
                    .map(|i| {
                        let a = 2.0 * std::f64::consts::PI * (i as f64) / (n as f64);
                        (
                            circ.center.x + circ.radius * a.cos(),
                            circ.center.y + circ.radius * a.sin(),
                        )
                    })
                    .collect();
                paths.push((pts, true));
            }
            _ => {}
        }
    }
    paths
}

fn bake_block_slot(
    slot: OpeningComponentSlot,
    block_name: &str,
    placement: BlockPlacementMode,
    p: OpeningBakeParams,
    doc: Option<&acadrust::CadDocument>,
) -> Vec<BakedPath> {
    let Some(doc) = doc else {
        return Vec::new();
    };
    let raw_paths = extract_block_paths(doc, block_name);
    if raw_paths.is_empty() {
        return Vec::new();
    }
    let mut min_u = f64::INFINITY;
    let mut max_u = f64::NEG_INFINITY;
    for (pts, _) in &raw_paths {
        for &(u, _) in pts {
            min_u = min_u.min(u);
            max_u = max_u.max(u);
        }
    }
    if !min_u.is_finite() || !max_u.is_finite() {
        return Vec::new();
    }
    let u_span = (max_u - min_u).max(1e-9);
    let hw = p.width * 0.5;

    let mut out = Vec::new();
    match placement {
        BlockPlacementMode::JambPair => {
            for (pts, closed) in &raw_paths {
                let left_pts: Vec<(f64, f64)> = pts
                    .iter()
                    .map(|&(u, v)| (-hw + (u - min_u), v))
                    .collect();
                out.push(path(slot, left_pts, *closed, false));
            }
            for (pts, closed) in &raw_paths {
                let right_pts: Vec<(f64, f64)> = pts
                    .iter()
                    .map(|&(u, v)| (hw - (u - min_u), v))
                    .collect();
                out.push(path(slot, right_pts, *closed, false));
            }
        }
        BlockPlacementMode::StretchToFit => {
            for (pts, closed) in &raw_paths {
                let stretched: Vec<(f64, f64)> = pts
                    .iter()
                    .map(|&(u, v)| (-hw + ((u - min_u) / u_span) * (2.0 * hw), v))
                    .collect();
                out.push(path(slot, stretched, *closed, false));
            }
        }
        BlockPlacementMode::CenterAnchor => {
            let u_mid = (min_u + max_u) * 0.5;
            for (pts, closed) in &raw_paths {
                let centered: Vec<(f64, f64)> = pts
                    .iter()
                    .map(|&(u, v)| (u - u_mid, v))
                    .collect();
                out.push(path(slot, centered, *closed, false));
            }
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
        OpeningGenerator::FrameRect => bake_frame_rect(slot, p, hw, ht, ft),
        OpeningGenerator::DoorFrame => bake_door_frame(slot, hw, ht, ft),
        OpeningGenerator::LeafLine => bake_leaf_line(slot, p, hw, ht, ft),
        OpeningGenerator::SwingArc => bake_swing_arc(slot, p, hw),
        OpeningGenerator::SillLines => bake_sill_lines(slot, hw, ht, ft, p.cross_axis_offset, p.wall_y_min, p.wall_y_max),
        OpeningGenerator::Cross => {
            let y_ext = p.wall_y_min - p.cross_axis_offset;
            let y_int = p.wall_y_max - p.cross_axis_offset;
            vec![
                path(slot, vec![(-hw, y_ext), (hw, y_int)], false, false),
                path(slot, vec![(-hw, y_int), (hw, y_ext)], false, false),
            ]
        }
        OpeningGenerator::DiagonalFill => {
            let y_ext = p.wall_y_min - p.cross_axis_offset;
            let y_int = p.wall_y_max - p.cross_axis_offset;
            vec![
                path(slot, vec![(-hw, y_ext), (hw, y_int)], false, false),
                path(
                    slot,
                    vec![(-hw, y_ext), (hw, y_ext), (hw, y_int), (-hw, y_int)],
                    true,
                    true,
                ),
            ]
        }
        OpeningGenerator::GlazingLine => bake_glazing_line(slot, hw, ft),
        OpeningGenerator::ThresholdLine => bake_threshold_line(slot, hw, ht),
        OpeningGenerator::OpeningLabel => bake_opening_label_preview(slot, hw),
        OpeningGenerator::ElevationFrameRect => {
            bake_elevation_contour(slot, p, hw, p.height, ft)
        }
        OpeningGenerator::ElevationFrameArch => {
            bake_elevation_contour(slot, p, hw, p.height, ft)
        }
        OpeningGenerator::ElevationMuntinsSingle => {
            bake_elevation_muntins_single(slot, hw, p.height, ft)
        }
        OpeningGenerator::ElevationMuntinsDouble => {
            bake_elevation_muntins_double(slot, hw, p.height, ft)
        }
        OpeningGenerator::ElevationSwingTriangle => {
            bake_elevation_swing_triangle(slot, p, hw, p.height, ft)
        }
        OpeningGenerator::ElevationSillLine => {
            bake_elevation_sill_line(slot, hw, p.height)
        }
        _ => Vec::new(),
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
    p: OpeningBakeParams,
    hw: f64,
    ht: f64,
    ft: f64,
) -> Vec<BakedPath> {
    if p.kind == OpeningKind::Door {
        bake_door_frame(slot, hw, ht, ft)
    } else {
        bake_window_posts(slot, hw, ht, ft)
    }
}

fn bake_window_posts(
    slot: OpeningComponentSlot,
    hw: f64,
    ht: f64,
    ft: f64,
) -> Vec<BakedPath> {
    let frame_depth = (ft.max(0.07)).min(ht * 2.0);
    let half_depth = frame_depth * 0.5;
    let post_w = ft.min(hw * 0.45).max(1e-4);

    let left_post = vec![
        (-hw, -half_depth),
        (-hw + post_w, -half_depth),
        (-hw + post_w, half_depth),
        (-hw, half_depth),
    ];
    let right_post = vec![
        (hw - post_w, -half_depth),
        (hw, -half_depth),
        (hw, half_depth),
        (hw - post_w, half_depth),
    ];

    vec![
        path(slot, left_post, true, false),
        path(slot, right_post, true, false),
    ]
}

fn bake_door_frame(
    slot: OpeningComponentSlot,
    hw: f64,
    ht: f64,
    ft: f64,
) -> Vec<BakedPath> {
    let post_w = ft.min(hw * 0.45).max(1e-4);
    let rebate_w = post_w * 0.5;
    let left_jamb = vec![
        (-hw, -ht),
        (-hw + post_w, -ht),
        (-hw + post_w, 0.0),
        (-hw + rebate_w, 0.0),
        (-hw + rebate_w, ht),
        (-hw, ht),
    ];
    let right_jamb = vec![
        (hw, -ht),
        (hw - post_w, -ht),
        (hw - post_w, 0.0),
        (hw - rebate_w, 0.0),
        (hw - rebate_w, ht),
        (hw, ht),
    ];
    vec![
        path(slot, left_jamb, true, false),
        path(slot, right_jamb, true, false),
    ]
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

fn bake_sill_lines(
    slot: OpeningComponentSlot,
    hw: f64,
    ht: f64,
    ft: f64,
    cross_offset: f64,
    wall_y_min: f64,
    wall_y_max: f64,
) -> Vec<BakedPath> {
    let frame_depth = (ft.max(0.07)).min(ht * 2.0);
    let frame_half_d = frame_depth * 0.5;

    let mut out = Vec::new();
    let sill_overhang = 0.035;
    let wall_ext_y = wall_y_min - cross_offset;
    let wall_int_y = wall_y_max - cross_offset;
    let sill_y_outer = wall_ext_y - sill_overhang;
    let sill_ear = 0.02;

    // Exterior sill front nose
    out.push(path(
        slot,
        vec![
            (-hw - sill_ear, sill_y_outer),
            (hw + sill_ear, sill_y_outer),
            (hw + sill_ear, wall_ext_y),
            (-hw - sill_ear, wall_ext_y),
        ],
        true,
        false,
    ));
    // Reveal line along exterior wall face
    out.push(path(
        slot,
        vec![(-hw, wall_ext_y), (hw, wall_ext_y)],
        false,
        false,
    ));

    // Exterior reveal side lines (connecting frame to exterior wall)
    if -frame_half_d > wall_ext_y + 1e-4 {
        out.push(path(
            slot,
            vec![(-hw, -frame_half_d), (-hw, wall_ext_y)],
            false,
            false,
        ));
        out.push(path(
            slot,
            vec![(hw, -frame_half_d), (hw, wall_ext_y)],
            false,
            false,
        ));
    }

    let board_overhang = 0.025;
    let board_y_inner = wall_int_y + board_overhang;
    let board_ear = 0.02;

    // Interior board front nose
    out.push(path(
        slot,
        vec![
            (-hw - board_ear, board_y_inner),
            (hw + board_ear, board_y_inner),
            (hw + board_ear, wall_int_y),
            (-hw - board_ear, wall_int_y),
        ],
        true,
        false,
    ));
    // Reveal line along interior wall face
    out.push(path(
        slot,
        vec![(-hw, wall_int_y), (hw, wall_int_y)],
        false,
        false,
    ));

    // Interior reveal side lines (connecting frame to interior wall)
    if frame_half_d < wall_int_y - 1e-4 {
        out.push(path(
            slot,
            vec![(-hw, frame_half_d), (-hw, wall_int_y)],
            false,
            false,
        ));
        out.push(path(
            slot,
            vec![(hw, frame_half_d), (hw, wall_int_y)],
            false,
            false,
        ));
    }

    out
}

fn bake_glazing_line(slot: OpeningComponentSlot, hw: f64, ft: f64) -> Vec<BakedPath> {
    let post_w = ft.min(hw * 0.45).max(1e-4);
    let clear_hw = (hw - post_w).max(0.0);
    if clear_hw <= 1e-6 {
        return Vec::new();
    }
    vec![
        path(slot, vec![(-clear_hw, -0.012), (clear_hw, -0.012)], false, false),
        path(slot, vec![(-clear_hw, 0.012), (clear_hw, 0.012)], false, false),
    ]
}

fn bake_threshold_line(slot: OpeningComponentSlot, hw: f64, ht: f64) -> Vec<BakedPath> {
    let t_w = (0.05_f64).min(ht);
    vec![path(
        slot,
        vec![(-hw, -t_w), (hw, -t_w), (hw, t_w), (-hw, t_w)],
        true,
        false,
    )]
}

fn bake_opening_label_preview(slot: OpeningComponentSlot, hw: f64) -> Vec<BakedPath> {
    let stroke = hw.min(0.2);
    vec![path(slot, vec![(-stroke, 0.0), (stroke, 0.0)], false, false)]
}

fn bake_elevation_contour(
    slot: OpeningComponentSlot,
    p: OpeningBakeParams,
    hw: f64,
    h: f64,
    ft: f64,
) -> Vec<BakedPath> {
    match p.shape {
        OpeningShape::Arch => {
            let spring = p.spring_height.min(h).max(0.0);
            let mut outer = vec![(-hw, 0.0), (hw, 0.0), (hw, spring)];
            let n = 16;
            for i in 0..=n {
                let angle = (i as f64) / (n as f64) * std::f64::consts::PI;
                let x = hw * angle.cos();
                let z = spring + (h - spring) * angle.sin();
                outer.push((x, z));
            }
            outer.push((-hw, spring));
            let mut res = vec![path(slot, outer, true, false)];
            if ft > 1e-4 && hw > ft && h > 2.0 * ft {
                let inner_hw = hw - ft;
                let inner_h = h - ft;
                let mut inner = vec![(-inner_hw, ft), (inner_hw, ft), (inner_hw, spring)];
                for i in 0..=n {
                    let angle = (i as f64) / (n as f64) * std::f64::consts::PI;
                    let x = inner_hw * angle.cos();
                    let z = spring + (inner_h - spring) * angle.sin();
                    inner.push((x, z));
                }
                inner.push((-inner_hw, spring));
                res.push(path(slot, inner, true, false));
            }
            res
        }
        _ => {
            let outer = vec![(-hw, 0.0), (hw, 0.0), (hw, h), (-hw, h)];
            let mut res = vec![path(slot, outer, true, false)];
            if ft > 1e-4 && hw > ft && h > 2.0 * ft {
                let inner = vec![
                    (-hw + ft, ft),
                    (hw - ft, ft),
                    (hw - ft, h - ft),
                    (-hw + ft, h - ft),
                ];
                res.push(path(slot, inner, true, false));
            }
            res
        }
    }
}

fn bake_elevation_muntins_single(
    slot: OpeningComponentSlot,
    hw: f64,
    h: f64,
    ft: f64,
) -> Vec<BakedPath> {
    if hw <= ft || h <= 2.0 * ft {
        return Vec::new();
    }
    vec![path(
        slot,
        vec![
            (-hw + ft, ft),
            (hw - ft, ft),
            (hw - ft, h - ft),
            (-hw + ft, h - ft),
        ],
        true,
        false,
    )]
}

fn bake_elevation_muntins_double(
    slot: OpeningComponentSlot,
    hw: f64,
    h: f64,
    ft: f64,
) -> Vec<BakedPath> {
    if hw <= ft || h <= 2.0 * ft {
        return Vec::new();
    }
    let mullion_half = ft * 0.5;
    let left_leaf = vec![
        (-hw + ft, ft),
        (-mullion_half, ft),
        (-mullion_half, h - ft),
        (-hw + ft, h - ft),
    ];
    let right_leaf = vec![
        (mullion_half, ft),
        (hw - ft, ft),
        (hw - ft, h - ft),
        (mullion_half, h - ft),
    ];
    let mullion = vec![
        (-mullion_half, 0.0),
        (mullion_half, 0.0),
        (mullion_half, h),
        (-mullion_half, h),
    ];
    vec![
        path(slot, mullion, true, false),
        path(slot, left_leaf, true, false),
        path(slot, right_leaf, true, false),
    ]
}

fn bake_elevation_swing_triangle(
    slot: OpeningComponentSlot,
    p: OpeningBakeParams,
    hw: f64,
    h: f64,
    ft: f64,
) -> Vec<BakedPath> {
    let inset_w = (hw - ft).max(0.0);
    let inset_bot = ft;
    let inset_top = (h - ft).max(ft);
    let mid_h = (inset_bot + inset_top) * 0.5;

    match p.hinge {
        HingeSide::Left => {
            let pts = vec![
                (inset_w, inset_bot),
                (-inset_w, mid_h),
                (inset_w, inset_top),
            ];
            vec![path(slot, pts, false, false)]
        }
        HingeSide::Right => {
            let pts = vec![
                (-inset_w, inset_bot),
                (inset_w, mid_h),
                (-inset_w, inset_top),
            ];
            vec![path(slot, pts, false, false)]
        }
    }
}

fn bake_elevation_sill_line(slot: OpeningComponentSlot, hw: f64, _h: f64) -> Vec<BakedPath> {
    let ear = 0.035;
    let sill_drop = -0.05;
    vec![
        path(slot, vec![(-hw - ear, 0.0), (hw + ear, 0.0)], false, false),
        path(
            slot,
            vec![
                (-hw - ear, 0.0),
                (-hw - ear, sill_drop),
                (hw + ear, sill_drop),
                (hw + ear, 0.0),
            ],
            true,
            false,
        ),
    ]
}

fn format_din1356_num(v: f64) -> String {
    let s3 = format!("{v:.3}");
    if s3.ends_with('0') {
        format!("{v:.2}")
    } else {
        s3
    }
}

/// Formats the opening annotation according to DIN 1356:
/// - Doors: `W / H`
/// - Windows: `W / H` and `BRH ...`
/// - Breakthroughs: `W / H` and `UK ...`
pub fn format_din1356_label(opening: &Opening) -> String {
    let w = format_din1356_num(opening.width);
    let h = format_din1356_num(opening.height);
    let sill = format_din1356_num(opening.sill_height);
    match opening.kind {
        OpeningKind::Door => {
            format!("{w} / {h}")
        }
        OpeningKind::Window => {
            format!("{w} / {h}\\PBRH {sill}")
        }
        OpeningKind::Breakthrough => {
            if let Some(d) = opening.depth {
                let d_str = format_din1356_num(d);
                format!("{w} / {h} / {d_str}\\PUK {sill}")
            } else {
                format!("{w} / {h}\\PUK {sill}")
            }
        }
    }
}

pub fn opening_elevation_loops(
    shape: OpeningShape,
    width: f64,
    height: f64,
    spring_height: f64,
    frame_thickness: f64,
    is_door: bool,
) -> (Vec<(f64, f64)>, Vec<(f64, f64)>) {
    let hw = width * 0.5;
    let ft = frame_thickness.max(0.01).min(hw * 0.45);
    let bot_in = if is_door { 0.0 } else { ft };

    match shape {
        OpeningShape::Rectangle => {
            let outer = vec![
                (-hw, 0.0),
                (hw, 0.0),
                (hw, height),
                (-hw, height),
            ];
            let inner = vec![
                (-hw + ft, bot_in),
                (hw - ft, bot_in),
                (hw - ft, (height - ft).max(bot_in + 0.01)),
                (-hw + ft, (height - ft).max(bot_in + 0.01)),
            ];
            (outer, inner)
        }
        OpeningShape::Circle => {
            let (w, h) = shape.lock_size(width, height, true);
            let r_out = w * 0.5;
            let cy = h * 0.5;
            let r_in = (r_out - ft).max(0.01);
            let n = crate::modules::aec::engine::opening_shape::CIRCLE_CHORD_COUNT.max(16);
            let mut outer = Vec::with_capacity(n);
            let mut inner = Vec::with_capacity(n);
            for i in 0..n {
                let t = (i as f64) * std::f64::consts::TAU / (n as f64);
                outer.push((r_out * t.cos(), cy + r_out * t.sin()));
                inner.push((r_in * t.cos(), cy + r_in * t.sin()));
            }
            (outer, inner)
        }
        OpeningShape::Arch => {
            let spring = crate::modules::aec::engine::opening_shape::clamp_spring(spring_height, height);
            let rise = height - spring;
            let mut outer = vec![(-hw, 0.0), (hw, 0.0), (hw, spring)];
            let n = crate::modules::aec::engine::opening_shape::ARCH_CHORD_COUNT.max(8);
            if rise > 1e-6 {
                let chord = width;
                let radius = chord * chord / (8.0 * rise) + rise * 0.5;
                let cy = height - radius;
                let a0 = (spring - cy).atan2(hw);
                let mut a1 = (spring - cy).atan2(-hw);
                while a1 <= a0 {
                    a1 += std::f64::consts::TAU;
                }
                for i in 1..n {
                    let t = a0 + (a1 - a0) * (i as f64) / (n as f64);
                    outer.push((radius * t.cos(), cy + radius * t.sin()));
                }
            }
            outer.push((-hw, spring));

            let in_hw = (hw - ft).max(0.01);
            let in_h = (height - ft).max(bot_in + 0.02);
            let in_spring = crate::modules::aec::engine::opening_shape::clamp_spring(spring, in_h);
            let in_rise = in_h - in_spring;
            let mut inner = vec![(-in_hw, bot_in), (in_hw, bot_in), (in_hw, in_spring)];
            if in_rise > 1e-6 {
                let in_chord = 2.0 * in_hw;
                let in_radius = in_chord * in_chord / (8.0 * in_rise) + in_rise * 0.5;
                let in_cy = in_h - in_radius;
                let a0 = (in_spring - in_cy).atan2(in_hw);
                let mut a1 = (in_spring - in_cy).atan2(-in_hw);
                while a1 <= a0 {
                    a1 += std::f64::consts::TAU;
                }
                for i in 1..n {
                    let t = a0 + (a1 - a0) * (i as f64) / (n as f64);
                    inner.push((in_radius * t.cos(), in_cy + in_radius * t.sin()));
                }
            }
            inner.push((-in_hw, in_spring));

            (outer, inner)
        }
        OpeningShape::Triangle(variant) => {
            let outer_raw = crate::modules::aec::engine::opening_shape::triangle_polygon(variant, width, height);
            let outer: Vec<(f64, f64)> = outer_raw.into_iter().map(|(s, z)| (s - hw, z)).collect();
            let cx = outer.iter().map(|p| p.0).sum::<f64>() / 3.0;
            let cz = outer.iter().map(|p| p.1).sum::<f64>() / 3.0;
            let scale = ((hw - ft) / hw).max(0.1).min(0.95);
            let inner: Vec<(f64, f64)> = outer.iter().map(|&(x, z)| (cx + (x - cx) * scale, cz + (z - cz) * scale)).collect();
            (outer, inner)
        }
    }
}

pub fn build_faceted_hollow_prism(
    outer_loop: &[(f64, f64)],
    inner_loop: &[(f64, f64)],
    y_min: f64,
    y_max: f64,
) -> Option<cadkernel::brep::Body> {
    let n = outer_loop.len();
    if n < 3 || inner_loop.len() != n {
        return None;
    }

    let mut vertices: Vec<[f64; 3]> = Vec::with_capacity(4 * n);
    for &(x, z) in outer_loop {
        vertices.push([x, y_min, z]);
    }
    for &(x, z) in outer_loop {
        vertices.push([x, y_max, z]);
    }
    for &(x, z) in inner_loop {
        vertices.push([x, y_min, z]);
    }
    for &(x, z) in inner_loop {
        vertices.push([x, y_max, z]);
    }

    let mut faces: Vec<Vec<usize>> = Vec::with_capacity(4 * n);
    for i in 0..n {
        let next = (i + 1) % n;
        // Outer side face (y_min to y_max)
        faces.push(vec![i, next, next + n, i + n]);
        // Inner side face (normal points inwards)
        faces.push(vec![2 * n + i, 3 * n + i, 3 * n + next, 2 * n + next]);
        // Back face quad (y = y_min, normal -Y)
        faces.push(vec![i, 2 * n + i, 2 * n + next, next]);
        // Front face quad (y = y_max, normal +Y)
        faces.push(vec![n + i, n + next, 3 * n + next, 3 * n + i]);
    }

    cadkernel::brep::make::faceted_solid(&vertices, &faces)
}

pub fn build_faceted_prism(
    polygon: &[(f64, f64)],
    y_min: f64,
    y_max: f64,
) -> Option<cadkernel::brep::Body> {
    let n = polygon.len();
    if n < 3 {
        return None;
    }

    let mut vertices: Vec<[f64; 3]> = Vec::with_capacity(2 * n);
    for &(x, z) in polygon {
        vertices.push([x, y_min, z]);
    }
    for &(x, z) in polygon {
        vertices.push([x, y_max, z]);
    }

    let mut faces: Vec<Vec<usize>> = Vec::with_capacity(n + 2);
    for i in 0..n {
        let next = (i + 1) % n;
        faces.push(vec![i, next, next + n, i + n]);
    }
    let back_face: Vec<usize> = (0..n).rev().collect();
    faces.push(back_face);
    let front_face: Vec<usize> = (n..2 * n).collect();
    faces.push(front_face);

    cadkernel::brep::make::faceted_solid(&vertices, &faces)
}

pub fn build_opening_frame_3d(
    width: f64,
    height: f64,
    frame_thickness: f64,
    depth: f64,
    shape: OpeningShape,
    spring_height: f64,
) -> Option<cadkernel::brep::Body> {
    let hw = width * 0.5;
    let hd = depth * 0.5;
    let ft = frame_thickness.max(0.02).min(hw * 0.45);
    let (outer, inner) = opening_elevation_loops(shape, width, height, spring_height, ft, false);
    build_faceted_hollow_prism(&outer, &inner, -hd, hd)
}

pub fn build_opening_leaf_3d(
    width: f64,
    height: f64,
    frame_thickness: f64,
    opening_angle_deg: f64,
    hinge: HingeSide,
    is_door: bool,
    shape: OpeningShape,
    spring_height: f64,
) -> Option<cadkernel::brep::Body> {
    let hw = width * 0.5;
    let ft = frame_thickness.max(0.02).min(hw * 0.45);
    let (_outer, inner) = opening_elevation_loops(shape, width, height, spring_height, ft, is_door);
    let leaf_t = 0.04;
    let leaf_ht = leaf_t * 0.5;
    let cub = build_faceted_prism(&inner, -leaf_ht, leaf_ht)?;

    let hinge_x = match hinge {
        HingeSide::Left => -hw + ft,
        HingeSide::Right => hw - ft,
    };
    let ang = match hinge {
        HingeSide::Left => opening_angle_deg.to_radians(),
        HingeSide::Right => -opening_angle_deg.to_radians(),
    };
    if ang.abs() > 1e-6 {
        crate::scene::model::solid_model::turned(&cub, 2, ang, [hinge_x, 0.0, 0.0])
    } else {
        Some(cub)
    }
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
        .map(|p| {
            if p.slot.is_elevation() {
                BakedPath {
                    slot: p.slot,
                    closed: p.closed,
                    filled: p.filled,
                    points: p
                        .points
                        .iter()
                        .map(|&(u, v)| (origin.0 + tangent.0 * u, origin.1 + tangent.1 * u + v))
                        .collect(),
                }
            } else {
                BakedPath {
                    slot: p.slot,
                    closed: p.closed,
                    filled: p.filled,
                    points: p
                        .points
                        .iter()
                        .map(|&(x, y)| local_to_world(x, y, origin, tangent, normal))
                        .collect(),
                }
            }
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
    let plan = rules.and_then(|r| r.plan_name.as_deref());
    let (slots, style) = resolved_slots_for_plan(opening, library, plan);
    let params = OpeningBakeParams::from_opening(opening, thickness, style.as_ref())
        .with_wall_bounds(wall_y_min, wall_y_max);
    let merged = rules_with_plan_visibility(opening, library, rules);
    let local = bake_opening_generators_with_doc(&slots, params, merged.as_ref(), doc);
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

/// Grips along the host axis: center (0), start jamb (1), end jamb (2), flip handle (3).
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
    let flip_handle = DVec3::new(offset_cx + normal.0 * 0.25, offset_cy + normal.1 * 0.25, base_z);
    vec![
        crate::entities::common::square_grip(0, center),
        crate::entities::common::rectangle_grip(1, start, [tx as f32, ty as f32]),
        crate::entities::common::rectangle_grip(2, end, [tx as f32, ty as f32]),
        crate::entities::common::square_grip(3, flip_handle),
    ]
}

/// Apply a center/width/flip grip. `grip_id` 0 moves the opening along the axis;
/// 1/2 stretch a jamb (width + recentre); 3 flips reference side and swing.
pub fn apply_opening_axis_grip(
    axis: &[(f64, f64)],
    opening: &mut Opening,
    grip_id: usize,
    world: DVec3,
) {
    if grip_id == 3 {
        opening.flip();
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
    let plan = rules.and_then(|r| r.plan_name.as_deref());
    let (slots, style) = resolved_slots_for_plan(&opening, library, plan);
    let params = OpeningBakeParams::from_opening(&opening, thickness, style.as_ref())
        .with_wall_bounds(wall_y_min, wall_y_max);
    let merged_rules = rules_with_plan_visibility(&opening, library, rules);
    let local = bake_opening_generators_with_doc(
        &slots,
        params,
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
            let handle = scene.add_entity(EntityType::Polyline3D(pl));
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
        let handle = scene.add_entity(EntityType::LwPolyline(pl));
        write_opening_display_tag(scene, handle, opening_handle, baked.slot);
        engine::owner_index::add_child(&mut scene.document, opening_handle, handle);
    }

    let plan = rules.and_then(|r| r.plan_name.as_deref());
    let (slots, _) = resolved_slots_for_plan(&opening, library, plan);
    let merged_rules = rules_with_plan_visibility(&opening, library, rules);
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
            let handle = scene.add_entity(EntityType::MText(mtext));
            write_opening_display_tag(
                scene,
                handle,
                opening_handle,
                OpeningComponentSlot::OpeningLabel2D,
            );
            engine::owner_index::add_child(&mut scene.document, opening_handle, handle);
        }
    }

    let frame_depth = 0.08_f64.min(thickness * 0.8);
    let frame_3d_vis = merged_rules
        .as_ref()
        .map_or(true, |r| r.is_opening_visible(OpeningComponentSlot::Frame3D))
        && (slots.contains_key(&OpeningComponentSlot::Frame3D)
            || slots.contains_key(&OpeningComponentSlot::Solid3D));

    if frame_3d_vis && opening.kind != OpeningKind::Breakthrough {
        if let Some(body) = build_opening_frame_3d(
            opening.width,
            opening.height,
            params.frame_thickness,
            frame_depth,
            params.shape,
            params.spring_height,
        ) {
            let world_body = crate::scene::model::solid_model::placed(
                &body,
                [tx, ty, 0.0],
                [normal.0, normal.1, 0.0],
                [0.0, 0.0, 1.0],
                [origin.0, origin.1, wall_base_z + opening.sill_height],
            );
            if let Some(body) = world_body {
                let solid_handle = scene.add_entity(EntityType::Solid3D(acadrust::entities::Solid3D::new()));
                if let Some(geom) = scene.prepare_solid_model_display(solid_handle, &body) {
                    scene.register_prepared_solid_model(solid_handle, body, geom);
                }
                write_opening_display_tag(
                    scene,
                    solid_handle,
                    opening_handle,
                    OpeningComponentSlot::Frame3D,
                );
                engine::owner_index::add_child(&mut scene.document, opening_handle, solid_handle);
            }
        }
    }

    let leaf_3d_vis = merged_rules
        .as_ref()
        .map_or(true, |r| r.is_opening_visible(OpeningComponentSlot::Leaf3D))
        && (slots.contains_key(&OpeningComponentSlot::Leaf3D)
            || slots.contains_key(&OpeningComponentSlot::Solid3D));

    if leaf_3d_vis && opening.kind != OpeningKind::Breakthrough {
        if let Some(body) = build_opening_leaf_3d(
            opening.width,
            opening.height,
            params.frame_thickness,
            params.opening_angle_deg,
            opening.hinge,
            opening.kind == OpeningKind::Door,
            params.shape,
            params.spring_height,
        ) {
            let world_body = crate::scene::model::solid_model::placed(
                &body,
                [tx, ty, 0.0],
                [normal.0, normal.1, 0.0],
                [0.0, 0.0, 1.0],
                [origin.0, origin.1, wall_base_z + opening.sill_height],
            );
            if let Some(body) = world_body {
                let solid_handle = scene.add_entity(EntityType::Solid3D(acadrust::entities::Solid3D::new()));
                if let Some(geom) = scene.prepare_solid_model_display(solid_handle, &body) {
                    scene.register_prepared_solid_model(solid_handle, body, geom);
                }
                write_opening_display_tag(
                    scene,
                    solid_handle,
                    opening_handle,
                    OpeningComponentSlot::Leaf3D,
                );
                engine::owner_index::add_child(&mut scene.document, opening_handle, solid_handle);
            }
        }
    }

    let glazing_3d_vis = merged_rules
        .as_ref()
        .map_or(true, |r| r.is_opening_visible(OpeningComponentSlot::Glazing3D))
        && (slots.contains_key(&OpeningComponentSlot::Glazing3D)
            || slots.contains_key(&OpeningComponentSlot::Solid3D));

    if glazing_3d_vis && opening.kind == OpeningKind::Window {
        if let Some(body) = build_opening_glazing_3d(
            opening.width,
            opening.height,
            params.frame_thickness,
            params.shape,
            params.spring_height,
        ) {
            let world_body = crate::scene::model::solid_model::placed(
                &body,
                [tx, ty, 0.0],
                [normal.0, normal.1, 0.0],
                [0.0, 0.0, 1.0],
                [origin.0, origin.1, wall_base_z + opening.sill_height],
            );
            if let Some(body) = world_body {
                let solid_handle = scene.add_entity(EntityType::Solid3D(acadrust::entities::Solid3D::new()));
                if let Some(geom) = scene.prepare_solid_model_display(solid_handle, &body) {
                    scene.register_prepared_solid_model(solid_handle, body, geom);
                }
                write_opening_display_tag(
                    scene,
                    solid_handle,
                    opening_handle,
                    OpeningComponentSlot::Glazing3D,
                );
                engine::owner_index::add_child(&mut scene.document, opening_handle, solid_handle);
            }
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
            height: 1.2,
            sill_height: 0.9,
            thickness: 0.3,
            frame_thickness: DEFAULT_FRAME_THICKNESS,
            cross_axis_offset: 0.0,
            wall_y_min: -0.15,
            wall_y_max: 0.15,
            hinge,
            shape: OpeningShape::Rectangle,
            spring_height: 0.0,
            opening_angle_deg: 90.0,
            kind,
        }
    }

    fn inner_frame_half_width(paths: &[BakedPath]) -> Option<f64> {
        let frames: Vec<_> = paths
            .iter()
            .filter(|p| p.slot == OpeningComponentSlot::Frame2D && p.closed)
            .collect();
        if frames.is_empty() {
            return None;
        }
        if frames.len() == 1 {
            let max_x = frames[0]
                .points
                .iter()
                .map(|p| p.0)
                .fold(f64::NEG_INFINITY, f64::max);
            return Some(max_x);
        }
        let has_two_separated_posts = frames.len() == 2 && {
            let p0_center_x = frames[0].points.iter().map(|p| p.0).sum::<f64>()
                / (frames[0].points.len() as f64);
            let p1_center_x = frames[1].points.iter().map(|p| p.0).sum::<f64>()
                / (frames[1].points.len() as f64);
            (p0_center_x * p1_center_x) < 0.0
        };
        if has_two_separated_posts {
            let right = frames.iter().max_by(|a, b| {
                let ax = a.points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
                let bx = b.points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
                ax.partial_cmp(&bx).unwrap()
            })?;
            let min_x = right
                .points
                .iter()
                .map(|p| p.0)
                .fold(f64::INFINITY, f64::min);
            Some(min_x)
        } else {
            let inner = frames.iter().min_by(|a, b| {
                let wa = a.points.iter().map(|p| p.0).fold(f64::NAN, f64::max)
                    - a.points.iter().map(|p| p.0).fold(f64::NAN, f64::min);
                let wb = b.points.iter().map(|p| p.0).fold(f64::NAN, f64::max)
                    - b.points.iter().map(|p| p.0).fold(f64::NAN, f64::min);
                wa.partial_cmp(&wb).unwrap()
            })?;
            let max_x = inner
                .points
                .iter()
                .map(|p| p.0)
                .fold(f64::NEG_INFINITY, f64::max);
            Some(max_x)
        }
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
        assert!(baked.iter().all(|p| {
            p.slot == OpeningComponentSlot::Mark2D
                || p.slot == OpeningComponentSlot::BreakthroughSymbol2D
                || p.slot == OpeningComponentSlot::OpeningLabel2D
        }));
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
