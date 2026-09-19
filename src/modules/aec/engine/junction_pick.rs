//! Junction layer pick, highlight, and override validation.

#![allow(unused_imports)]
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

use acadrust::entities::{LwPolyline, LwVertex, Point};
use acadrust::tables::AppId;
use acadrust::types::{Vector2, Vector3};
use acadrust::{CadDocument, EntityType, Handle};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use glam::DVec3;

use crate::scene::model::hatch_model::{HatchModel, HatchPattern};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

use super::{
    self as engine, Storey, StyleLibrary, Wall, WallJustification, WallLayer,
    join::{self, JoinError, JoinKind},
    junction_solver::{self, WallJoinInput},
    library::load_or_seed,
    plan_view::{PhaseFilter, PlanPhase},
    wall_style::{
        base_width_from_layers, effective_layers_for_wall_bb, migrate_gap_before_to_axis_offset,
        LayerFunction, ResolvedLayer, WallStyle,
    },
};

#[allow(unused_imports)]
use super::display_apply::*;
use super::xdata::*;
use super::wall_package::*;
use super::wall_regen::*;
use super::join_ops::*;
use super::storey_xdata::*;
use super::opening_xdata::*;

/// Sentinel grip ids for the screen-offset dropdown at a wall junction
/// endpoint (`end_index` 0 → start, 1 → last vertex). Kept below
/// `VIS_GRIP_ID` (`usize::MAX`) and the spline mode grip (`usize::MAX - 1`).
pub const WALL_JUNCTION_DROPDOWN_GRIP_START: usize = usize::MAX - 3;

/// Dropdown grip id for a wall junction at `end_index` (0 = start, 1 = end).
pub fn wall_junction_dropdown_grip_id(end_index: usize) -> usize {
    WALL_JUNCTION_DROPDOWN_GRIP_START + end_index.min(1)
}

/// Inverse of [`wall_junction_dropdown_grip_id`].
pub fn wall_junction_end_from_dropdown_grip(grip_id: usize) -> Option<usize> {
    if grip_id == WALL_JUNCTION_DROPDOWN_GRIP_START {
        Some(0)
    } else if grip_id == WALL_JUNCTION_DROPDOWN_GRIP_START + 1 {
        Some(1)
    } else {
        None
    }
}

/// True when this grip is a wall-end / junction dropdown with a stored join override.
pub(crate) fn grip_has_join_override(scene: &Scene, handle: Handle, grip_id: usize) -> bool {
    let axis = resolve_wall_package(scene, handle);
    let vertices = get_wall_vertices(scene, axis);
    if vertices.len() < 2 {
        return false;
    }
    let end_index = wall_junction_end_from_dropdown_grip(grip_id).or_else(|| {
        if grip_id == 0 {
            Some(0usize)
        } else if grip_id == vertices.len() - 1 {
            Some(1usize)
        } else {
            None
        }
    });
    end_index.is_some_and(|end_index| read_junction_override(scene, axis, end_index).is_some())
}

/// Closed 2D loop of layer `index` (outer→inner) from axis + `(thickness, axis_offset)`.
pub fn wall_layer_contour_loop_xy(
    centerline: &[(f64, f64)],
    layers: &[(f64, f64)],
    index: usize,
) -> Option<Vec<(f64, f64)>> {
    let (left, right) = engine::contour::layer_contours(centerline, layers).into_iter().nth(index)?;
    if left.len() < 2 || right.len() < 2 {
        return None;
    }
    let mut loop_xy = left;
    loop_xy.extend(right.into_iter().rev());
    if let Some(first) = loop_xy.first().copied() {
        loop_xy.push(first);
    }
    Some(loop_xy)
}

/// Preview outline of one wall layer (magenta/orange hover colour).
pub fn wall_layer_highlight_wire(
    scene: &Scene,
    wall_handle: Handle,
    layer_index: usize,
    color: [f32; 4],
) -> Option<WireModel> {
    let handle = resolve_wall_package(scene, wall_handle);
    let verts = get_wall_vertices(scene, handle);
    if verts.len() < 2 {
        return None;
    }
    let wall = wall_from_entity(scene.document.get_entity(handle)?)?;
    let specs: Vec<(f64, f64)> = wall
        .layers
        .iter()
        .map(|l| (l.thickness, l.axis_offset))
        .collect();
    let centerline: Vec<(f64, f64)> = verts.iter().map(|v| (v.x, v.y)).collect();
    let loop_xy = wall_layer_contour_loop_xy(&centerline, &specs, layer_index)?;
    let z = verts[0].z;
    let points: Vec<[f64; 3]> = loop_xy.into_iter().map(|(x, y)| [x, y, z]).collect();
    let mut wire = WireModel::solid_f64("aec_layer_highlight".into(), points, color, false);
    wire.line_weight_px = 3.0;
    Some(wire)
}

/// Transparent orange (or given RGB) fill of one wall-layer strip.
pub fn wall_layer_highlight_hatch(
    scene: &Scene,
    wall_handle: Handle,
    layer_index: usize,
    color: [f32; 4],
) -> Option<HatchModel> {
    let handle = resolve_wall_package(scene, wall_handle);
    let verts = get_wall_vertices(scene, handle);
    if verts.len() < 2 {
        return None;
    }
    let wall = wall_from_entity(scene.document.get_entity(handle)?)?;
    let specs: Vec<(f64, f64)> = wall
        .layers
        .iter()
        .map(|l| (l.thickness, l.axis_offset))
        .collect();
    let centerline: Vec<(f64, f64)> = verts.iter().map(|v| (v.x, v.y)).collect();
    let mut loop_xy = wall_layer_contour_loop_xy(&centerline, &specs, layer_index)?;
    if loop_xy.len() >= 2 && loop_xy.first() == loop_xy.last() {
        loop_xy.pop();
    }
    if loop_xy.len() < 3 {
        return None;
    }
    let origin = [loop_xy[0].0, loop_xy[0].1];
    let boundary: Vec<[f32; 2]> = loop_xy
        .iter()
        .map(|(x, y)| [(*x - origin[0]) as f32, (*y - origin[1]) as f32])
        .collect();
    let mut fill = color;
    fill[3] = if color[3] < 0.99 { color[3] } else { 0.32 };
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
        name: "AEC_LAYER_PREVIEW".into(),
        color: fill,
        aci: 0,
        line_weight_px: 1.0,
        angle_offset: 0.0,
        scale: 1.0,
        draw_depth: 0.0,
    })
}

/// Even-odd test for a closed XY loop (duplicate closing vertex allowed).
pub fn point_in_closed_loop_xy(x: f64, y: f64, poly: &[(f64, f64)]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > y) != (yj > y) {
            let denom = yj - yi;
            if denom.abs() > 1e-18 {
                let x_int = (xj - xi) * (y - yi) / denom + xi;
                if x < x_int {
                    inside = !inside;
                }
            }
        }
        j = i;
    }
    inside
}

pub(crate) fn polygon_area_xy(poly: &[(f64, f64)]) -> f64 {
    if poly.len() < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    for i in 0..poly.len() - 1 {
        a += poly[i].0 * poly[i + 1].1 - poly[i + 1].0 * poly[i].1;
    }
    a.abs() * 0.5
}

/// Layer strip under `(x, y)` at the junction, preferring the smallest
/// containing contour so inner layers win over the outer stack.
pub fn pick_junction_wall_layer(
    scene: &Scene,
    axis_handle: Handle,
    end_index: usize,
    x: f64,
    y: f64,
) -> Option<(Handle, usize, String)> {
    let participants = walls_at_junction(scene, axis_handle, end_index);
    let mut best: Option<(Handle, usize, String)> = None;
    let mut best_area = f64::INFINITY;
    for part in &participants {
        let handle = resolve_wall_package(scene, part.axis_handle);
        let verts = get_wall_vertices(scene, handle);
        if verts.len() < 2 {
            continue;
        }
        let Some(wall) = scene
            .document
            .get_entity(handle)
            .and_then(wall_from_entity)
        else {
            continue;
        };
        let specs: Vec<(f64, f64)> = wall
            .layers
            .iter()
            .map(|l| (l.thickness, l.axis_offset))
            .collect();
        let centerline: Vec<(f64, f64)> = verts.iter().map(|v| (v.x, v.y)).collect();
        for (idx, layer) in part.layers.iter().enumerate() {
            let Some(loop_xy) = wall_layer_contour_loop_xy(&centerline, &specs, idx) else {
                continue;
            };
            if !point_in_closed_loop_xy(x, y, &loop_xy) {
                continue;
            }
            let area = polygon_area_xy(&loop_xy);
            if area < best_area {
                best_area = area;
                best = Some((part.axis_handle, idx, layer.material_id.clone()));
            }
        }
    }
    best
}

/// World XY of the junction node used for the in-drawing layer pick.
pub fn junction_node_xy(
    scene: &Scene,
    axis_handle: Handle,
    end_index: usize,
) -> Option<(f64, f64)> {
    let verts = get_wall_vertices(scene, resolve_wall_package(scene, axis_handle));
    if verts.len() < 2 {
        return None;
    }
    let p = if end_index == 0 {
        verts[0]
    } else {
        verts[verts.len() - 1]
    };
    Some((p.x, p.y))
}

/// Click radius around the node for “outer face / no layer” (2× stack width).
pub fn junction_outer_pick_radius(
    scene: &Scene,
    axis_handle: Handle,
    end_index: usize,
) -> f64 {
    let participants = walls_at_junction(scene, axis_handle, end_index);
    let mut width = 0.05_f64;
    for part in &participants {
        let handle = resolve_wall_package(scene, part.axis_handle);
        if let Some(wall) = scene.document.get_entity(handle).and_then(wall_from_entity) {
            let stack: f64 = wall.layers.iter().map(|l| l.thickness.max(0.0)).sum();
            width = width.max(stack);
        }
    }
    (width * 2.0).max(0.1)
}

/// Resolve the Junction Editor's chosen source/target layer to the actual
/// [`join::LayerRef`] in `layers`.
///
/// The editor buttons pass `(enumerate index, material_id)`. Identity is the
/// stack entry at that index when its material still matches — that copies
/// `layer_id` / `role_tag` from the live layer. Matching "first layer with
/// this material" is intentionally not used: two plaster layers would then
/// always bind the outer one.
pub fn selected_junction_layer_ref(
    layers: &[join::LayerRef],
    index: usize,
    material_id: &str,
) -> Option<join::LayerRef> {
    if let Some(layer) = layers.get(index) {
        if layer.material_id == material_id {
            return Some(layer.clone());
        }
    }
    layers
        .iter()
        .find(|l| l.index == index && l.material_id == material_id)
        .cloned()
}

/// Validate a stored [`join::JunctionOverride`] against the current layer
/// sets of the wall(s) at the junction (`self_layers` and, when known,
/// `other_layers`): drop any [`join::LayerPairOverride`] whose `layer_a` or
/// `layer_b` (when `Some`) no longer matches an existing layer on either
/// side. Returns the cleaned override (`None` when nothing meaningful is
/// left — no `default_style` and no remaining valid `layer_pairs`) plus how
/// many pairs were dropped.
pub(crate) fn validate_junction_override(
    override_data: &join::JunctionOverride,
    self_layers: &[join::LayerRef],
    other_layers: &[join::LayerRef],
) -> (Option<join::JunctionOverride>, usize) {
    // `layer_a` identifies a layer on the wall that *owns* this override
    // (the resolver — see `resolve_layer_override_style` — only ever matches
    // it against that wall's own layer set); `layer_b`, when present, may
    // name a layer on either side of the junction.
    let matches_any = |r: &join::LayerRef| {
        layer_ref_matches(r, self_layers) || layer_ref_matches(r, other_layers)
    };
    let mut removed = 0usize;
    let kept_pairs: Vec<join::LayerPairOverride> = override_data
        .layer_pairs
        .iter()
        .filter(|p| {
            let ok = layer_ref_matches(&p.layer_a, self_layers)
                && match &p.layer_b {
                    Some(b) => matches_any(b),
                    None => true,
                };
            if !ok {
                removed += 1;
            }
            ok
        })
        .cloned()
        .collect();
    let kept_gaps: Vec<join::LayerGapOverride> = override_data
        .layer_gaps
        .iter()
        .filter(|g| {
            let ok = matches_any(&g.layer) && matches_any(&g.from) && matches_any(&g.to);
            if !ok {
                removed += 1;
            }
            ok
        })
        .cloned()
        .collect();
    if removed == 0 {
        return (Some(override_data.clone()), 0);
    }
    if override_data.default_style.is_none() && kept_pairs.is_empty() && kept_gaps.is_empty() {
        (None, removed)
    } else {
        (
            Some(join::JunctionOverride {
                default_style: override_data.default_style.clone(),
                layer_pairs: kept_pairs,
                layer_gaps: kept_gaps,
            }),
            removed,
        )
    }
}

/// Validate the [`join::JunctionOverride`] stored on `axis_handle`/`end_index`
/// against `self_layers`/`other_layers`, persisting the cleaned-up result (or
/// erasing the XDATA entirely) and queuing a user-visible notice when
/// anything was invalidated. Returns the override to actually use for this
/// regeneration (already cleaned).
pub(crate) fn validate_and_persist_junction_override(
    scene: &mut Scene,
    axis_handle: Handle,
    end_index: usize,
    override_data: join::JunctionOverride,
    self_layers: &[join::LayerRef],
    other_layers: &[join::LayerRef],
) -> Option<join::JunctionOverride> {
    let (cleaned, removed) =
        validate_junction_override(&override_data, self_layers, other_layers);
    if removed == 0 {
        return cleaned;
    }
    match &cleaned {
        Some(ov) => {
            write_junction_override(scene, axis_handle, end_index, ov);
        }
        None => {
            remove_junction_override(scene, axis_handle, end_index);
        }
    }
    queue_override_warning(crate::tr!(
        "aec",
        "join-override-cleaned",
        removed = removed,
        axis = axis_handle.to_string()
    ));
    cleaned
}
