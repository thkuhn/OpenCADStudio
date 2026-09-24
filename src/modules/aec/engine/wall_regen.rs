//! Wall representation regeneration, axis layer, and stretch.

#![allow(unused_imports)]
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

use acadrust::entities::{LwPolyline, LwVertex, Point, Polyline3D};
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
use super::junction_pick::*;
use super::xdata::*;
use super::wall_package::*;
use super::join_ops::*;
use super::storey_xdata::*;
use super::opening_xdata::*;

/// Layer name used for the (invisible) wall axis / centerline reference
/// geometry. `AEC_ROOM` / loop-detection and the `WALL` XDATA carrier keep
/// living on this layer once the visible contour/hatch/solid representation
/// is regenerated.
pub const AEC_WALL_AXIS_LAYER: &str = "AEC_WALL_AXIS";

pub const AEC_CONTROLPLANES_LAYER: &str = "AEC_CONTROLPLANES";

/// Helper to get total thickness, height, and storey_id for a wall entity.
pub fn wall_thickness_and_height(entity: &EntityType) -> Option<(f64, f64, u32)> {
    let wall = wall_from_entity(entity)?;
    Some((wall.total_thickness(), wall.height, wall.storey_id))
}

/// Parameters for a 3D extrusion of a wall layer.
///
/// Contains the 2D footprint (a closed polygon loop) and the height
/// to extrude it by.
#[derive(Debug, Clone, PartialEq)]
pub struct WallLayerExtrusion {
    pub footprint: Vec<(f64, f64)>,
    pub height: f64,
    pub base_offset: f64,
}

/// Extract open-axis centerline points and per-vertex LWPOLYLINE bulges from a
/// wall axis entity. Returns empty vectors when `wall_entity` is not an
/// `LwPolyline`.
pub(crate) fn wall_axis_points_and_bulges(wall_entity: &EntityType) -> (Vec<(f64, f64)>, Vec<f64>) {
    let EntityType::LwPolyline(pl) = wall_entity else {
        return (Vec::new(), Vec::new());
    };
    let centerline: Vec<(f64, f64)> = pl
        .vertices
        .iter()
        .map(|v| (v.location.x, v.location.y))
        .collect();
    let bulges: Vec<f64> = pl.vertices.iter().map(|v| v.bulge).collect();
    (centerline, bulges)
}

/// Sample a bulge segment (straight or arc) into `segments + 1` points for
/// wireframe preview rendering. Falls back to the two endpoints for a
/// straight (`bulge ≈ 0`) or degenerate segment.
pub(crate) fn tessellate_bulge_segment(
    start: (f64, f64),
    end: (f64, f64),
    bulge: f64,
    segments: usize,
) -> Vec<(f64, f64)> {
    let Some(arc) = engine::arc::bulge_to_arc(start, end, bulge) else {
        return vec![start, end];
    };
    let included = arc.included_angle();
    let n = segments.max(1);
    (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let angle = if arc.ccw {
                arc.start_angle + included * t
            } else {
                arc.start_angle - included * t
            };
            (
                arc.center.0 + arc.radius * angle.cos(),
                arc.center.1 + arc.radius * angle.sin(),
            )
        })
        .collect()
}

/// Extracts a wall's centerline points from its [`LwPolyline`] geometry and
/// computes parallel boundary lines for each layer.
///
/// Returns one `(inner, outer)` boundary pair per layer. Prefer
/// [`wall_layer_footprints`] / [`engine::representation::build_wall_representation`]
/// when closed per-layer polygons are enough — this pair form is kept for
/// callers that still need the raw offset polylines (tests, miter diagnostics).
///
/// Axis bulges (arc segments) are honoured via
/// [`engine::contour::layer_contours_with_bulges`].
pub fn wall_layer_contour_polylines(
    wall_entity: &EntityType,
    layers: &[WallLayer],
) -> Vec<(Vec<(f64, f64)>, Vec<(f64, f64)>)> {
    let (centerline, bulges) = wall_axis_points_and_bulges(wall_entity);
    if centerline.len() < 2 {
        return Vec::new();
    }
    let layer_data: Vec<(f64, f64)> = layers.iter().map(|l| (l.thickness, l.axis_offset)).collect();
    engine::contour::layer_contours_with_bulges(&centerline, &bulges, &layer_data)
        .into_iter()
        .map(|(a, b)| (a.points, b.points))
        .collect()
}

/// Closed per-layer footprints for a wall axis entity, derived via the shared
/// [`engine::representation::WallRepresentation`] builder.
///
/// Axis is assumed already justification-shifted (centerline_offset = 0),
/// matching how walls are persisted after draw. Axis bulges are forwarded so
/// curved walls produce arc-aware offset footprints.
pub fn wall_layer_footprints(
    wall_entity: &EntityType,
    layers: &[WallLayer],
) -> Vec<Vec<(f64, f64)>> {
    wall_layer_footprints_with_bulges(wall_entity, layers)
        .into_iter()
        .map(|(pts, _)| pts)
        .collect()
}

/// Like [`wall_layer_footprints`], but also returns per-vertex bulges for each
/// closed footprint (LWPOLYLINE convention) so derived contour entities can
/// keep arc segments exact.
pub fn wall_layer_footprints_with_bulges(
    wall_entity: &EntityType,
    layers: &[WallLayer],
) -> Vec<(Vec<(f64, f64)>, Vec<f64>)> {
    let (centerline, bulges) = wall_axis_points_and_bulges(wall_entity);
    if centerline.len() < 2 {
        return Vec::new();
    }
    let layer_data: Vec<(f64, f64)> = layers.iter().map(|l| (l.thickness, l.axis_offset)).collect();
    let repr = engine::representation::build_wall_representation_with_bulges(
        &centerline,
        &bulges,
        &layer_data,
        0.0,
    );
    repr.layer_contours_2d
        .into_iter()
        .zip(repr.layer_contour_bulges.into_iter())
        .collect()
}

/// Produces the parameters needed to create an extruded solid for each wall layer.
///
/// This implementation uses the "layer footprint" approach: it builds a closed
/// 2D polygon per layer via [`engine::representation::build_wall_representation`]
/// and returns it along with the wall height.
///
/// Scoping Decision: This function returns plain data ([`WallLayerExtrusion`]).
/// A future step can wire this to the host's `Solid3D` entity creation calls
/// (e.g., using `sweep_model::extruded`).
pub fn wall_layer_extrusions(
    wall_entity: &EntityType,
    layers: &[WallLayer],
    height: f64,
) -> Vec<WallLayerExtrusion> {
    let footprints = wall_layer_footprints(wall_entity, layers);
    if footprints.is_empty() {
        return Vec::new();
    }

    let mut extrusions = Vec::with_capacity(layers.len());
    for (i, footprint) in footprints.into_iter().enumerate() {
        let layer = &layers[i];
        let effective_height = (height - layer.bottom_offset - layer.top_offset).max(0.0);
        let base_offset = layer.bottom_offset;

        extrusions.push(WallLayerExtrusion {
            footprint,
            height: effective_height,
            base_offset,
        });
    }
    extrusions
}

/// Register the `AEC_WALL_AXIS` layer (invisible / non-printable) if it
/// isn't already in the document's layer table.
pub fn ensure_wall_axis_layer(scene: &mut Scene) {
    set_wall_axis_layer_visible(scene, false);
}

/// Session override: show or hide every wall axis via `AEC_WALL_AXIS`.
/// The layer stays non-plottable. OSNAP still injects axis wires when off.
pub fn ensure_controlplanes_layer(scene: &mut Scene) {
    scene.ensure_layer(AEC_CONTROLPLANES_LAYER);
    if let Some(layer) = scene.document.layers.get_mut(AEC_CONTROLPLANES_LAYER) {
        layer.is_plottable = false;
    }
}

pub fn set_controlplanes_layer_visible(scene: &mut Scene, visible: bool) {
    ensure_controlplanes_layer(scene);
    let changed = if let Some(layer) = scene.document.layers.get_mut(AEC_CONTROLPLANES_LAYER) {
        layer.is_plottable = false;
        let was_off = layer.flags.off;
        layer.flags.off = !visible;
        was_off != !visible
    } else {
        false
    };
    if changed {
        scene.invalidate_layer_dependencies(&[AEC_CONTROLPLANES_LAYER.to_string()]);
    }
}

pub fn toggle_controlplanes_layer(scene: &mut Scene) -> bool {
    ensure_controlplanes_layer(scene);
    let visible = scene
        .document
        .layers
        .get(AEC_CONTROLPLANES_LAYER)
        .map(|l| l.flags.off)
        .unwrap_or(true);
    set_controlplanes_layer_visible(scene, visible);
    visible
}

/// Rebuild Face3D previews for a storey's control planes on `AEC_CONTROLPLANES`.
pub fn regenerate_control_plane_previews(
    scene: &mut Scene,
    storey: &mut crate::modules::aec::engine::project::StoreyRef,
) {
    crate::modules::aec::project::preview::regenerate_control_plane_previews(scene, storey);
}

pub fn set_wall_axis_layer_visible(scene: &mut Scene, visible: bool) {
    scene.ensure_layer(AEC_WALL_AXIS_LAYER);
    let changed = if let Some(layer) = scene.document.layers.get_mut(AEC_WALL_AXIS_LAYER) {
        layer.is_plottable = false;
        let was_off = layer.flags.off;
        layer.flags.off = !visible;
        was_off != !visible
    } else {
        false
    };
    if changed {
        scene.invalidate_layer_dependencies(&[AEC_WALL_AXIS_LAYER.to_string()]);
    }
}

/// Idle visibility of the axis layer from a resolved rule set. Absent rules
/// keep the historical default (layer off).
pub fn axis_visible_from_rules(
    rules: Option<&engine::display_component::ComponentRuleSet>,
) -> bool {
    use engine::display_component::WallComponentSlot;
    rules.is_some_and(|r| r.is_visible(WallComponentSlot::AxisLine))
}

/// Erase draw-time companion entities that are superseded once
/// [`regenerate_wall_representation`] builds proper `WALL_REP` children.
///
/// `WallCommand` commits an untagged outer-contour `LwPolyline` alongside the
/// axis for live preview. That polyline is **not** a `WALL_REP` child and is
/// never refreshed on axis edits — leaving it in the document produces the
/// "orphan 2D polyline that doesn't follow wall changes" symptom. Call this
/// with every non-axis live handle when the wall drawing command finishes,
/// immediately before regeneration.
pub fn erase_wall_live_preview_companions(
    scene: &mut Scene,
    axis_handle: Handle,
    companions: &[Handle],
) {
    let mut to_erase = Vec::new();
    for &h in companions {
        if h == axis_handle {
            continue;
        }
        let Some(entity) = scene.document.get_entity(h) else {
            continue;
        };
        // Drop the rubber-band contour *and* a superseded live axis: each
        // segment currently `CommitEntity`s a *new* WALL polyline while the
        // draw-time live axis stays in the document as a visible 2D line.
        if !matches!(entity, EntityType::LwPolyline(_)) {
            continue;
        }
        if is_wall_display_child_entity(entity) {
            continue;
        }
        to_erase.push(h);
    }
    if !to_erase.is_empty() {
        scene.erase_entities(&to_erase);
    }
}

/// Number of straight segments used to approximate one arc edge when
/// tessellating a closed footprint ring for the hatch fill boundary (see
/// [`tessellate_ring_with_bulges`]). [`HatchModel::boundary`] is a plain
/// point list with no bulge support, unlike the `LwPolyline` contour entity,
/// so a curved wall layer's hatch would otherwise fill only the straight
/// chord between an arc segment's endpoints instead of following the curve.
pub(crate) const WALL_HATCH_ARC_SEGMENTS: usize = 24;

/// Expand a closed footprint ring (LWPOLYLINE-style vertex + per-edge bulge)
/// into a dense polyline so arc segments are approximated by short straight
/// chords instead of a single long one.
///
/// Returns `ring` unchanged (cloned) when every bulge is zero (or absent),
/// so straight-only walls keep the exact same hatch boundary as before this
/// tessellation was added — no behavior change, no performance cost.
pub(crate) fn retarget_closed_footprint_bulges(
    original: &[(f64, f64)],
    original_bulges: &[f64],
    edited: &[(f64, f64)],
) -> Vec<f64> {
    let n = edited.len();
    if n < 2 {
        return vec![0.0; n];
    }
    if original.len() != n {
        return vec![0.0; n];
    }
    let mut out = vec![0.0; n];
    for i in 0..n {
        let bulge = original_bulges.get(i).copied().unwrap_or(0.0);
        if bulge.abs() <= 1e-12 {
            continue;
        }
        let j = (i + 1) % n;
        out[i] = engine::arc::retarget_bulge(
            original[i],
            original[j],
            bulge,
            edited[i],
            edited[j],
        );
    }
    out
}

pub(crate) fn tessellate_ring_with_bulges(ring: &[(f64, f64)], bulges: &[f64]) -> Vec<(f64, f64)> {
    if ring.len() < 3 || !bulges.iter().any(|b| b.abs() > 1e-12) {
        return ring.to_vec();
    }
    let n = ring.len();
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        let start = ring[i];
        let end = ring[(i + 1) % n];
        let bulge = bulges.get(i).copied().unwrap_or(0.0);
        if bulge.abs() <= 1e-12 {
            out.push(start);
            continue;
        }
        // `tessellate_bulge_segment` includes both endpoints; drop the last
        // sample (shared with the next edge's start) to avoid duplicates.
        let mut samples = tessellate_bulge_segment(start, end, bulge, WALL_HATCH_ARC_SEGMENTS);
        samples.pop();
        out.extend(samples);
    }
    out
}

/// Pack a single closed 2D ring into a [`HatchModel`]'s relative-boundary
/// representation (anchor at the ring's first vertex, f32 offsets from it),
/// mirroring the shape `pack_rings` builds in `draw/hatch.rs` for a single
/// loop with no holes.
pub(crate) fn pack_wall_ring(ring: &[(f64, f64)]) -> (Vec<[f32; 2]>, [f64; 2], Vec<[f64; 2]>) {
    let origin = ring.first().copied().unwrap_or((0.0, 0.0));
    let origin = [origin.0, origin.1];
    let mut rel: Vec<[f32; 2]> = ring
        .iter()
        .map(|&(x, y)| [(x - origin[0]) as f32, (y - origin[1]) as f32])
        .collect();
    let mut wcs: Vec<[f64; 2]> = ring.iter().map(|&(x, y)| [x, y]).collect();
    // Close the loop so hatch tessellation matches the closed 2D contour.
    if rel.len() >= 3 {
        let first = rel[0];
        let last = *rel.last().unwrap();
        if (first[0] - last[0]).abs() > 1e-6 || (first[1] - last[1]).abs() > 1e-6 {
            rel.push(first);
            wcs.push(wcs[0]);
        }
    }
    (rel, origin, wcs)
}

/// Convert an `AcadColor` into a normalized RGBA color with the alpha the
/// AEC hatch representation uses. Logical colours without a resolvable RGB
/// fall back to light grey.
pub(crate) fn wall_hatch_color(color: acadrust::types::Color) -> [f32; 4] {
    let (r, g, b) = color.rgb().unwrap_or((153, 153, 153));
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 0.85]
}

/// Convert a material `line_color` 0xRRGGBB into an `AcadColor::Rgb`.
pub(crate) fn material_line_color_as_acad(rgb: u32) -> acadrust::types::Color {
    acadrust::types::Color::Rgb {
        r: ((rgb >> 16) & 0xFF) as u8,
        g: ((rgb >> 8) & 0xFF) as u8,
        b: (rgb & 0xFF) as u8,
    }
}

/// Error returned by [`regenerate_wall_representation`]; never a panic —
/// malformed axis/XDATA just leaves the wall without a rebuilt
/// representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallRegenError {
    /// `wall_handle` doesn't resolve to an entity carrying `WALL`
    /// XDATA.
    NotAWall,
    /// The wall has no material layers to build a representation from.
    NoLayers,
    /// The axis geometry didn't yield usable layer contours (e.g. fewer
    /// than two vertices).
    NoContours,
}

/// (Re)build the visible 2D contour + hatch and 3D solid representation for
/// the wall at `wall_handle`, from its axis polyline + `WALL`
/// XDATA.
///
/// The axis polyline is moved onto the invisible `AEC_WALL_AXIS` layer (kept
/// as reference geometry for `AEC_ROOM` / loop detection and as the XDATA
/// carrier). Every entity handle previously recorded in `derived_handles` is
/// erased first, so calling this repeatedly on the same wall never
/// accumulates duplicates. Derived handles are always persisted on the
/// wall's `WALL` XDATA record so subsequent regenerations can erase them.
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation(
    scene: &mut Scene,
    wall_handle: Handle,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    regenerate_wall_representation_with_corner(scene, wall_handle, None, None, library_override)
}

/// Like [`regenerate_wall_representation`], but honors per-slot visibility
/// from `rules` (see [`engine::display_component::ComponentRuleSet`]):
/// a `WALL_REP` child whose corresponding [`engine::display_component::WallComponentSlot`]
/// is hidden simply isn't created, instead of a global LOD switch. `None`
/// (or a default rule set) reproduces today's behavior exactly (every slot
/// defaults to visible).
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_rules(
    scene: &mut Scene,
    wall_handle: Handle,
    rules: Option<&engine::display_component::ComponentRuleSet>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    regenerate_wall_representation_with_corner_and_rules(
        scene,
        wall_handle,
        None,
        None,
        rules,
        library_override,
    )
}

/// Like [`regenerate_wall_representation_with_rules`], but additionally
/// honors a `DisplayConfig`'s `style_substitutions` map (source wall style
/// id -> target wall style id, see [`engine::plan_view::DisplayConfig`]):
/// when the wall's own style id has an entry here, the *style* (material/
/// hatch/color) of each layer is taken from the corresponding layer (by
/// index) of the target wall style, while axis, thickness and layer count
/// are always derived from the wall's own (unchanged) layers. `None`
/// reproduces today's behavior exactly (no substitution applied).
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_rules_and_substitutions(
    scene: &mut Scene,
    wall_handle: Handle,
    rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    regenerate_wall_representation_with_corner_rules_and_substitutions(
        scene,
        wall_handle,
        None,
        None,
        rules,
        style_substitutions,
        library_override,
    )
}

/// Rebuild a wall after its axis vertices changed (grip / stretch) and
/// re-resolve nearby L/T/N junctions. Returns every axis + derived handle
/// that the scene tessellation must refresh.
pub fn refresh_wall_after_axis_edit(
    scene: &mut Scene,
    wall_handle: Handle,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Vec<Handle> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let mut touched = try_auto_join_nearby_walls(
        scene,
        wall_handle,
        library_override,
        display_rules,
        style_substitutions,
    );
    if !touched.iter().any(|h| *h == wall_handle) {
        match regenerate_wall_representation_with_rules_and_substitutions(
            scene,
            wall_handle,
            display_rules,
            style_substitutions,
            library_override,
        ) {
            Ok(t) => touched.extend(t),
            Err(_) => touched.push(wall_handle),
        }
    }
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    // Resident wire cache can keep the previous 2D outline if only
    // Added/Removed deltas are replayed after a delete+recreate. Force a
    // full rebuild so the contour on screen matches the new axis.
    scene.bump_geometry();
    touched
}

/// Move a wall's **axis** vertices (not its visible contour/hatch/solid
/// children) by `delta` wherever `in_win` reports the vertex as selected,
/// then regenerate + re-join via [`refresh_wall_after_axis_edit`].
///
/// The wall axis lives on the invisible `AEC_WALL_AXIS` layer, so commands
/// like STRETCH that hit-test only visible geometry never see it — they only
/// ever get a handle to the derived contour. Moving that derived contour's
/// own vertices instead of the axis is reverted by the very next
/// regeneration (which rebuilds the contour from the unchanged axis), so any
/// wall-aware caller must resolve to the axis and move *it* first. This
/// function is that shared operation; used by STRETCH in
/// `command_driver.rs`.
///
/// Returns `None` if `owner` isn't a wall, has no axis vertices, or none of
/// them fall inside the window (no-op). Otherwise returns every axis +
/// derived handle touched by the regeneration, exactly like
/// [`refresh_wall_after_axis_edit`].
pub fn stretch_wall_axis_in_window(
    scene: &mut Scene,
    owner: Handle,
    in_win: impl Fn(f64, f64) -> bool,
    delta: DVec3,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Option<Vec<Handle>> {
    let axis_vertices = get_wall_vertices(scene, owner);
    if axis_vertices.is_empty() {
        return None;
    }
    let mut moved = false;
    let new_vertices: Vec<DVec3> = axis_vertices
        .iter()
        .map(|v| {
            if in_win(v.x, v.y) {
                moved = true;
                DVec3::new(v.x + delta.x, v.y + delta.y, v.z + delta.z)
            } else {
                *v
            }
        })
        .collect();
    if !moved {
        return None;
    }
    update_wall_vertices(scene, owner, &new_vertices);
    Some(refresh_wall_after_axis_edit(
        scene,
        owner,
        library_override,
        display_rules,
        style_substitutions,
    ))
}

/// Like [`regenerate_wall_representation`], but lets a caller supply a
/// corner-extension hint: `(vertex_index, extended_position)` moves one axis
/// vertex further out — past a joined corner and into the other wall's
/// footprint — for contour/hatch/solid generation only. The persisted axis
/// polyline (and therefore `AEC_ROOM` loop detection, which depends on the
/// exact trimmed corner) is left untouched; only the *visible representation*
/// uses the extended point.
///
/// When `join_miter` is supplied, matched layers (by material/function then
/// offset-from-axis) are rebuilt with a true diagonal miter against the other
/// wall's corresponding layer; unmatched layers fall back to the single-vertex
/// `corner_override` extension.
///
/// Used by [`join_two_walls_in_document`] so an L/T join's two walls share a
/// clean mitered corner instead of leaving a seam where their
/// independently-capped rectangles merely touch.
///
/// On success returns the axis handle plus every newly created derived
/// (contour/hatch/solid) handle, so callers (e.g. grip-release) can bump 2D
/// and 3D representations together.
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_corner(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    join_miter: Option<&engine::miter::JoinMiterContext>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    regenerate_wall_representation_with_corner_and_rules(
        scene,
        wall_handle,
        corner_override,
        join_miter,
        None,
        library_override,
    )
}

/// Like [`regenerate_wall_representation_with_corner`], but also honors
/// per-slot visibility from `rules` (see [`regenerate_wall_representation_with_rules`]).
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_corner_and_rules(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    join_miter: Option<&engine::miter::JoinMiterContext>,
    rules: Option<&engine::display_component::ComponentRuleSet>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    regenerate_wall_representation_with_corner_rules_and_substitutions(
        scene,
        wall_handle,
        corner_override,
        join_miter,
        rules,
        None,
        library_override,
    )
}

/// Like [`regenerate_wall_representation_with_corner_and_rules`], but also
/// honors `style_substitutions` (see
/// [`regenerate_wall_representation_with_rules_and_substitutions`]).
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_corner_rules_and_substitutions(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    join_miter: Option<&engine::miter::JoinMiterContext>,
    rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    regenerate_wall_representation_inner(
        scene,
        wall_handle,
        corner_override,
        join_miter,
        None,
        rules,
        style_substitutions,
        library_override,
    )
}

/// Like [`regenerate_wall_representation_with_corner`], but takes precomputed
/// per-layer miter footprints (from N-way junction resolution). `None` entries
/// fall back to `corner_override` / base contours exactly as unmatched layers do.
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_precomputed_miters(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    mitered_footprints: &[Option<Vec<(f64, f64)>>],
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    regenerate_wall_representation_with_precomputed_miters_and_rules(
        scene,
        wall_handle,
        corner_override,
        mitered_footprints,
        None,
        library_override,
    )
}

/// Like [`regenerate_wall_representation_with_precomputed_miters`], but also
/// honors per-slot visibility from `rules` (see
/// [`regenerate_wall_representation_with_rules`]).
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_precomputed_miters_and_rules(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    mitered_footprints: &[Option<Vec<(f64, f64)>>],
    rules: Option<&engine::display_component::ComponentRuleSet>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    regenerate_wall_representation_with_precomputed_miters_rules_and_substitutions(
        scene,
        wall_handle,
        corner_override,
        mitered_footprints,
        rules,
        None,
        library_override,
    )
}

/// Like [`regenerate_wall_representation_with_precomputed_miters_and_rules`],
/// but also honors `style_substitutions` (see
/// [`regenerate_wall_representation_with_rules_and_substitutions`]).
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_precomputed_miters_rules_and_substitutions(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    mitered_footprints: &[Option<Vec<(f64, f64)>>],
    rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    regenerate_wall_representation_inner(
        scene,
        wall_handle,
        corner_override,
        None,
        Some(mitered_footprints),
        rules,
        style_substitutions,
        library_override,
    )
}

fn find_end_peer_layers(
    scene: &Scene,
    wall_handle: Handle,
    self_axis_2d: &[(f64, f64)],
    handled_end: usize,
) -> Option<(Vec<engine::miter::MiterLayer>, Option<usize>)> {
    if self_axis_2d.len() < 2 || handled_end >= self_axis_2d.len() {
        return None;
    }
    let pt = self_axis_2d[handled_end];
    let tol = join::JUNCTION_TOLERANCE.max(1e-4);

    let candidates = engine::owner_index::peers_of(&scene.document, wall_handle);
    if candidates.is_empty() {
        return None;
    }
    for peer in candidates {
        if peer == wall_handle {
            continue;
        }
        let peer_axis = get_wall_vertices(scene, peer);
        if peer_axis.len() < 2 {
            continue;
        }
        if (peer_axis[0].x - pt.0).hypot(peer_axis[0].y - pt.1) <= tol {
            let layers = wall_layer_data(scene, peer);
            return Some((layers, Some(0)));
        } else if (peer_axis.last().unwrap().x - pt.0).hypot(peer_axis.last().unwrap().y - pt.1) <= tol {
            let layers = wall_layer_data(scene, peer);
            return Some((layers, Some(peer_axis.len() - 1)));
        }
    }
    None
}

fn layer_has_same_material_peer(
    layer: &WallLayer,
    layer_idx: usize,
    self_end: usize,
    all_layers: &[WallLayer],
    peer_data: Option<&(Vec<engine::miter::MiterLayer>, Option<usize>)>,
) -> bool {
    let Some((peer_layers, peer_end)) = peer_data else {
        return false;
    };
    let miter_self_layers: Vec<engine::miter::MiterLayer> = all_layers
        .iter()
        .map(|l| engine::miter::MiterLayer::with_id(l.thickness, l.axis_offset, l.material.clone(), l.function.clone(), l.layer_id))
        .collect();
    let pairing = engine::miter::match_layer_indices_for_l_join(&miter_self_layers, peer_layers, self_end, *peer_end);
    if let Some(Some(other_idx)) = pairing.get(layer_idx) {
        if let Some(other_l) = peer_layers.get(*other_idx) {
            return !layer.material.is_empty() && layer.material.eq_ignore_ascii_case(&other_l.material);
        }
    }
    false
}

fn filter_opening_surface_edges(
    set: &mut crate::scene::model::mesh_model::MeshLodSet,
    wires: &mut Vec<acadrust::entities::Wire>,
    openings: &[engine::openings::Opening],
    centerline: &[(f64, f64)],
    solid_height: f64,
    solid_base: f64,
) {
    if openings.is_empty() || centerline.len() < 2 {
        return;
    }

    struct CutPlane {
        origin: (f64, f64),
        tangent: (f64, f64),
        jamb_intervals: Vec<(f64, f64)>,
    }

    let mut cut_planes: Vec<CutPlane> = Vec::new();
    let zones = engine::elevation_cut::opening_zones(openings);
    for zone in &zones {
        let width = zone.width();
        let xs = engine::elevation_cut::opening_slice_x_positions(width, &zone.holes);
        for x in xs {
            let s_axis = zone.s0 + x;
            if let Some((origin, tangent)) =
                engine::openings::point_and_tangent_at_distance(centerline, s_axis)
            {
                let mut jambs = Vec::new();
                for hole in &zone.holes {
                    let n = hole.len();
                    for i in 0..n {
                        let p_a = hole[i];
                        let p_b = hole[(i + 1) % n];
                        if (p_a.0 - x).abs() <= 1e-3 && (p_b.0 - x).abs() <= 1e-3 {
                            let dz = (p_b.1 - p_a.1).abs();
                            if dz > 1e-3 {
                                let z_low = p_a.1.min(p_b.1) + solid_base;
                                let z_high = p_a.1.max(p_b.1) + solid_base;
                                jambs.push((z_low, z_high));
                            }
                        }
                    }
                }

                if let Some(existing) = cut_planes.iter_mut().find(|cp| {
                    (cp.origin.0 - origin.0).hypot(cp.origin.1 - origin.1) <= 1e-3
                }) {
                    existing.jamb_intervals.extend(jambs);
                } else {
                    cut_planes.push(CutPlane {
                        origin,
                        tangent,
                        jamb_intervals: jambs,
                    });
                }
            }
        }
    }

    let is_on_plane = |p: [f64; 3], cp: &CutPlane| -> bool {
        let dot = (p[0] - cp.origin.0) * cp.tangent.0 + (p[1] - cp.origin.1) * cp.tangent.1;
        dot.abs() <= 2e-3
    };

    let process_segment = |p0: [f64; 3], p1: [f64; 3]| -> Vec<([f64; 3], [f64; 3])> {
        let matched_cp = cut_planes.iter().find(|cp| is_on_plane(p0, cp) && is_on_plane(p1, cp));
        let Some(cp) = matched_cp else {
            return vec![(p0, p1)];
        };

        let dxy = (p1[0] - p0[0]).hypot(p1[1] - p0[1]);
        let dz = (p1[2] - p0[2]).abs();

        // Transverse horizontal edge at wall crown
        if (p0[2] - (solid_base + solid_height)).abs() <= 2e-3
            && (p1[2] - (solid_base + solid_height)).abs() <= 2e-3
        {
            return Vec::new();
        }
        // Transverse horizontal edge at wall base
        if (p0[2] - solid_base).abs() <= 2e-3 && (p1[2] - solid_base).abs() <= 2e-3 {
            return Vec::new();
        }

        // Vertical edge on wall face
        if dxy <= 1e-3 && dz > 1e-3 {
            let z_low = p0[2].min(p1[2]);
            let z_high = p0[2].max(p1[2]);
            let mut kept = Vec::new();
            for &(jamb_low, jamb_high) in &cp.jamb_intervals {
                let overlap_low = z_low.max(jamb_low);
                let overlap_high = z_high.min(jamb_high);
                if overlap_high - overlap_low > 1e-4 {
                    let v_a = [p0[0], p0[1], overlap_low];
                    let v_b = [p0[0], p0[1], overlap_high];
                    kept.push((v_a, v_b));
                }
            }
            return kept;
        }

        vec![(p0, p1)]
    };

    let old_verts = std::mem::take(&mut set.edge_verts);
    let mut new_verts = Vec::with_capacity(old_verts.len());
    let mut new_verts_low = Vec::with_capacity(old_verts.len());

    for chunk in old_verts.chunks_exact(2) {
        let p0 = [chunk[0][0] as f64, chunk[0][1] as f64, chunk[0][2] as f64];
        let p1 = [chunk[1][0] as f64, chunk[1][1] as f64, chunk[1][2] as f64];
        let kept = process_segment(p0, p1);
        for (v0, v1) in kept {
            let h0 = [v0[0] as f32, v0[1] as f32, v0[2] as f32];
            let l0 = [(v0[0] - h0[0] as f64) as f32, (v0[1] - h0[1] as f64) as f32, (v0[2] - h0[2] as f64) as f32];
            let h1 = [v1[0] as f32, v1[1] as f32, v1[2] as f32];
            let l1 = [(v1[0] - h1[0] as f64) as f32, (v1[1] - h1[1] as f64) as f32, (v1[2] - h1[2] as f64) as f32];
            new_verts.push(h0);
            new_verts_low.push(l0);
            new_verts.push(h1);
            new_verts_low.push(l1);
        }
    }
    set.edge_verts = new_verts;
    set.edge_verts_low = new_verts_low;

    let old_wires = std::mem::take(wires);
    let mut new_wires = Vec::new();
    for wire in old_wires {
        if wire.points.len() < 2 {
            continue;
        }
        for window in wire.points.windows(2) {
            let p0 = [window[0].x, window[0].y, window[0].z];
            let p1 = [window[1].x, window[1].y, window[1].z];
            let kept = process_segment(p0, p1);
            for (v0, v1) in kept {
                let w = acadrust::entities::Wire::from_points(vec![
                    acadrust::types::Vector3::new(v0[0], v0[1], v0[2]),
                    acadrust::types::Vector3::new(v1[0], v1[1], v1[2]),
                ]);
                new_wires.push(w);
            }
        }
    }
    *wires = new_wires;
}

fn filter_miter_deck_edges(
    set: &mut crate::scene::model::mesh_model::MeshLodSet,
    wires: &mut Vec<acadrust::entities::Wire>,
    miter_segments_2d: &[((f64, f64), (f64, f64))],
    z_base: f64,
    z_top: f64,
) {
    if miter_segments_2d.is_empty() {
        return;
    }

    let is_deck_miter_segment = |p0: [f64; 3], p1: [f64; 3]| -> bool {
        let is_at_deck = (p0[2] - z_top).abs() <= 2e-3 && (p1[2] - z_top).abs() <= 2e-3;
        let is_at_base = (p0[2] - z_base).abs() <= 2e-3 && (p1[2] - z_base).abs() <= 2e-3;
        if !is_at_deck && !is_at_base {
            return false;
        }

        for &(v_a, v_b) in miter_segments_2d {
            let d0a = (p0[0] - v_a.0).hypot(p0[1] - v_a.1);
            let d1b = (p1[0] - v_b.0).hypot(p1[1] - v_b.1);
            let d0b = (p0[0] - v_b.0).hypot(p0[1] - v_b.1);
            let d1a = (p1[0] - v_a.0).hypot(p1[1] - v_a.1);
            if (d0a <= 5e-3 && d1b <= 5e-3) || (d0b <= 5e-3 && d1a <= 5e-3) {
                return true;
            }
        }
        false
    };

    let old_verts = std::mem::take(&mut set.edge_verts);
    let old_low = std::mem::take(&mut set.edge_verts_low);
    let mut new_verts = Vec::with_capacity(old_verts.len());
    let mut new_verts_low = Vec::with_capacity(old_low.len());

    for (i, chunk) in old_verts.chunks_exact(2).enumerate() {
        let p0 = [chunk[0][0] as f64, chunk[0][1] as f64, chunk[0][2] as f64];
        let p1 = [chunk[1][0] as f64, chunk[1][1] as f64, chunk[1][2] as f64];
        if !is_deck_miter_segment(p0, p1) {
            new_verts.push(chunk[0]);
            new_verts.push(chunk[1]);
            if let Some(low_chunk) = old_low.get(i * 2..i * 2 + 2) {
                new_verts_low.push(low_chunk[0]);
                new_verts_low.push(low_chunk[1]);
            }
        }
    }
    set.edge_verts = new_verts;
    set.edge_verts_low = new_verts_low;

    let old_wires = std::mem::take(wires);
    let mut new_wires = Vec::new();
    for wire in old_wires {
        if wire.points.len() < 2 {
            continue;
        }
        let mut filtered_segments = Vec::new();
        for win in wire.points.windows(2) {
            let p0 = [win[0].x, win[0].y, win[0].z];
            let p1 = [win[1].x, win[1].y, win[1].z];
            if !is_deck_miter_segment(p0, p1) {
                filtered_segments.push((win[0], win[1]));
            }
        }
        for (w0, w1) in filtered_segments {
            new_wires.push(acadrust::entities::Wire::from_points(vec![w0, w1]));
        }
    }
    *wires = new_wires;
}

fn find_cap_edge_index(
    footprint: &[(f64, f64)],
    target_pt: (f64, f64),
    axis_tangent: (f64, f64),
    max_dist: f64,
) -> Option<usize> {
    let n = footprint.len();
    if n < 3 {
        return None;
    }
    let mut best_idx = None;
    let mut best_dist = f64::MAX;

    for i in 0..n {
        let p0 = footprint[i];
        let p1 = footprint[(i + 1) % n];
        let mid = ((p0.0 + p1.0) * 0.5, (p0.1 + p1.1) * 0.5);
        let dist = ((mid.0 - target_pt.0).powi(2) + (mid.1 - target_pt.1).powi(2)).sqrt();
        if dist > max_dist {
            continue;
        }
        let edge = (p1.0 - p0.0, p1.1 - p0.1);
        let edge_len = (edge.0.powi(2) + edge.1.powi(2)).sqrt();
        if edge_len < 1e-6 {
            continue;
        }
        let edge_dir = (edge.0 / edge_len, edge.1 / edge_len);
        let dot = (edge_dir.0 * axis_tangent.0 + edge_dir.1 * axis_tangent.1).abs();
        if dot <= 0.85 && dist < best_dist {
            best_dist = dist;
            best_idx = Some(i);
        }
    }
    best_idx
}

/// Locate a wall's already-established join(s) — pairwise (L/T) or N-way —
/// at the axis end that is *not* `handled_end`, using the peer-link index
/// maintained by `engine::owner_index`, cross-referenced against which of
/// this wall's own axis ends is actually coincident with a peer's endpoint
/// or lies on a peer's span. Reuses the exact same junction-detection and
/// per-layer-miter machinery as `join_junction_in_document`
/// (`join::detect_junctions` + `engine::miter::junction_wall_geoms` +
/// `mitered_junction_layer_footprints_with_overrides`), but purely
/// read-only: it never mutates axis vertices or persists overrides for
/// participants other than confirming this wall's own existing
/// `JunctionOverride`.
///
/// Returns `None` when there is no peer at the other end (the common case:
/// a wall with only one join, or a freshly drawn unjoined end), which keeps
/// `regenerate_wall_representation_inner` byte-for-byte unchanged for that
/// case. Used so a NEW join event at one end doesn't silently drop an
/// already-established join at the other end during regeneration.
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
    let mut candidates = engine::owner_index::peers_of(&scene.document, wall_handle);
    for h in all_wall_axis_handles(scene) {
        if h != wall_handle && !candidates.contains(&h) {
            candidates.push(h);
        }
    }
    for peer in candidates {
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

    let axes: Vec<Vec<DVec3>> = handles.iter().map(|h| get_wall_vertices(scene, *h)).collect();
    if axes.iter().any(|a| a.len() < 2) {
        return None;
    }
    let axis_refs: Vec<&[DVec3]> = axes.iter().map(|a| a.as_slice()).collect();
    let junctions = join::detect_junctions(&axis_refs, tol);
    let junc = junctions.into_iter().find(|j| (j.point.x - pt.0).hypot(j.point.y - pt.1) <= tol.max(1e-3))?;

    // Confirm this wall (`handles[0]`) actually participates as an endpoint
    // at `other_end` in the detected junction (guards against picking up an
    // unrelated junction that happens to share the same point).
    let self_wall_index = 0usize;
    let pi = junc
        .participants
        .iter()
        .position(|p| p.wall_index == self_wall_index)?;
    if !matches!(junc.participants[pi].role, join::JunctionRole::Endpoint(e) if e == other_end) {
        return None;
    }

    let axes_2d: Vec<Vec<(f64, f64)>> = axes
        .iter()
        .map(|a| a.iter().map(|p| (p.x, p.y)).collect())
        .collect();
    let layers: Vec<Vec<engine::miter::MiterLayer>> =
        handles.iter().map(|h| wall_layer_data(scene, *h)).collect();
    let geoms = engine::miter::junction_wall_geoms(&junc, &axes_2d, &layers);
    let layer_refs: Vec<Vec<join::LayerRef>> = junc
        .participants
        .iter()
        .map(|p| {
            layers
                .get(p.wall_index)
                .map(|ls| {
                    layer_refs_from_materials(ls.iter().map(|l| (l.material.as_str(), l.layer_id)))
                })
                .unwrap_or_default()
        })
        .collect();
    let junction_overrides: Vec<Option<join::JunctionOverride>> = junc
        .participants
        .iter()
        .enumerate()
        .map(|(oi, p)| match p.role {
            join::JunctionRole::Endpoint(end_idx) => {
                read_junction_override(scene, handles[p.wall_index], end_idx).and_then(|ov| {
                    let self_refs = layer_refs.get(oi).cloned().unwrap_or_default();
                    let other_refs: Vec<join::LayerRef> = layer_refs
                        .iter()
                        .enumerate()
                        .filter(|(oi2, _)| *oi2 != oi)
                        .flat_map(|(_, ls)| ls.iter().cloned())
                        .collect();
                    validate_and_persist_junction_override(
                        scene,
                        handles[p.wall_index],
                        end_idx,
                        ov,
                        &self_refs,
                        &other_refs,
                    )
                })
            }
            join::JunctionRole::Through(_) => None,
        })
        .collect();
    let all_fps = engine::miter::mitered_junction_layer_footprints_with_overrides(
        &junc,
        &geoms,
        &layer_refs,
        &junction_overrides,
    );
    all_fps.get(pi).cloned()
}

/// T-junction cutouts when this wall is the through (head) wall: the join
/// sits on a span, not an axis end, so [`find_other_end_junction_footprints`]
/// never sees it. Without this, plan-type / 2D-3D regen drops pockets.
pub(crate) fn find_through_span_cutout_footprints(
    scene: &mut Scene,
    wall_handle: Handle,
    self_axis_2d: &[(f64, f64)],
) -> Option<Vec<Option<Vec<(f64, f64)>>>> {
    if self_axis_2d.len() < 2 {
        return None;
    }
    let handles = all_wall_axis_handles(scene);
    if handles.len() < 2 {
        return None;
    }
    let self_i = handles.iter().position(|h| *h == wall_handle)?;
    let axes: Vec<Vec<DVec3>> = handles.iter().map(|h| get_wall_vertices(scene, *h)).collect();
    if axes.iter().any(|a| a.len() < 2) {
        return None;
    }
    let axis_refs: Vec<&[DVec3]> = axes.iter().map(|a| a.as_slice()).collect();
    let tol = join::JUNCTION_TOLERANCE.max(1e-4);
    let junctions = join::detect_junctions(&axis_refs, tol);
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

    let mut merged: Vec<Option<Vec<(f64, f64)>>> = vec![None; through_layers.len()];
    let mut any = false;
    for junc in &junctions {
        let is_through = junc.participants.iter().any(|p| {
            p.wall_index == self_i && matches!(p.role, join::JunctionRole::Through(_))
        });
        if !is_through {
            continue;
        }
        let Some(stem) = junc
            .participants
            .iter()
            .find(|p| matches!(p.role, join::JunctionRole::Endpoint(_)))
        else {
            continue;
        };
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
    any.then_some(merged)
}

pub(crate) fn regenerate_wall_representation_inner(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    join_miter: Option<&engine::miter::JoinMiterContext>,
    precomputed_miters: Option<&[Option<Vec<(f64, f64)>>]>,
    rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
    library_override: Option<&StyleLibrary>,
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
    let (layers, height, wall_base_z, _old_derived, wall_style_id, wall_hatch_override) = (
        wall.layers,
        wall.height,
        wall.base_origin[2],
        wall.derived_handles,
        wall.style_id,
        wall.hatch_override,
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
    if let Some(e) = scene.document.get_entity_mut(wall_handle) {
        e.as_entity_mut().set_layer(AEC_WALL_AXIS_LAYER.to_string());
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

    // Openings hosted by this wall — split 2D contours/hatches into
    // disconnected pieces. 3D rest-wall solids use those pieces; the opening
    // zone is an elevation-cut extrusion through each layer.
    let host_cut_visible = rules.map_or(true, |r| {
        r.is_opening_visible(engine::display_component::OpeningComponentSlot::HostCut2D)
    });
    let wall_openings = if host_cut_visible {
        openings_for_host_wall(scene, wall_handle)
    } else {
        Vec::new()
    };
    let (centerline, bulges) = wall_axis_points_and_bulges(&axis_entity);
    let layer_data: Vec<(f64, f64)> =
        layers.iter().map(|l| (l.thickness, l.axis_offset)).collect();
    let layer_extrusion: Vec<(f64, f64)> = layers
        .iter()
        .map(|l| {
            (
                (height - l.bottom_offset - l.top_offset).max(0.0),
                l.bottom_offset,
            )
        })
        .collect();
    let display = engine::representation::build_wall_display_set(
        &centerline,
        &bulges,
        &layer_data,
        0.0,
        &wall_openings,
        &layer_extrusion,
    );

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

    if self_axis_2d.len() >= 2 {
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
        if let Some(cut) =
            find_through_span_cutout_footprints(scene, wall_handle, &self_axis_2d)
        {
            for (i, fp) in cut.into_iter().enumerate() {
                if let Some(cut_fp) = fp {
                    if i < mitered_footprints.len() {
                        let merged = engine::miter::merge_through_cutout_preserving_miter(
                            mitered_footprints[i].as_deref(),
                            &cut_fp,
                        );
                        mitered_footprints[i] = Some(merged);
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
    // Prefer a caller-supplied (usually project-resolved) library so hatch
    // scale/pattern/color and material lookups match the active project
    // instead of always reading the global on-disk defaults.
    let owned_library;
    let library: &StyleLibrary = match library_override {
        Some(lib) => lib,
        None => {
            owned_library = load_or_seed();
            &owned_library
        }
    };

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

        let pieces_2d: Vec<(Vec<(f64, f64)>, Vec<f64>)> = if !wall_openings.is_empty() {
            let split_rings = engine::miter::split_mitered_footprint_by_openings(
                &uncut_footprint.0,
                &centerline,
                &wall_openings,
            );
            split_rings
                .into_iter()
                .map(|p| {
                    let b = if uncut_footprint.1.is_empty()
                        || uncut_footprint.1.iter().all(|&x| x.abs() < 1e-12)
                    {
                        vec![0.0; p.len()]
                    } else {
                        retarget_closed_footprint_bulges(
                            &base_footprints[i],
                            base_footprint_bulges.get(i).map(|b| b.as_slice()).unwrap_or(&[]),
                            &p,
                        )
                    };
                    (p, b)
                })
                .collect()
        } else {
            let rings = engine::miter::split_footprint_rings(&uncut_footprint.0);
            if rings.len() > 1 {
                rings
                    .into_iter()
                    .map(|r| {
                        let n = r.len();
                        (r, vec![0.0; n])
                    })
                    .collect()
            } else if !uncut_footprint.0.is_empty() {
                vec![uncut_footprint.clone()]
            } else {
                Vec::new()
            }
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
                        reuse_or_add_wall_contour(scene, &mut reusable_contours, pl);
                    if let Some(layer_name) = layer.layer_override.as_deref().filter(|s| !s.is_empty()) {
                        scene.ensure_layer(layer_name);
                        if let Some(e) = scene.document.get_entity_mut(contour_handle) {
                            e.as_entity_mut().set_layer(layer_name.to_string());
                        }
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
                if let Some(layer_name) = layer.layer_override.as_deref().filter(|s| !s.is_empty()) {
                    scene.ensure_layer(layer_name);
                    if let Some(e) = scene.document.get_entity_mut(hatch_handle) {
                        e.as_entity_mut().set_layer(layer_name.to_string());
                    }
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
        let solid_rings: Vec<(Vec<(f64, f64)>, Vec<f64>)> = pieces_2d.clone();
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

                if let Some(body) =
                    crate::scene::model::sweep_model::extruded(entity_to_use, solid_height)
                {
                    let s3d = acadrust::entities::Solid3D::new();
                    let solid_handle = scene.add_entity(EntityType::Solid3D(s3d));
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
                                solid_base,
                                solid_base + solid_height,
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
            if !wall_openings.is_empty() {
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
                        let s3d = acadrust::entities::Solid3D::new();
                        let solid_handle = scene.add_entity(EntityType::Solid3D(s3d));
                        if let Some(mut display_geom) = scene.prepare_solid_model_display(solid_handle, &body) {
                            filter_opening_surface_edges(
                                &mut display_geom.0,
                                &mut display_geom.1,
                                &wall_openings,
                                &centerline,
                                solid_height,
                                solid_base,
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

/// Changes an existing `WALL` wall's justification (Interior/Center/
/// Exterior), shifting its axis polyline sideways by the delta between the
/// old and new justification offsets (same `WallJustification::offset` math
/// used by [`WallCommand::build_entity`]), then regenerates its
/// contour/hatch/solid representation. No-op (returns `false`) if
/// `wall_handle` doesn't carry a `WALL` record.
pub fn change_wall_justification(
    scene: &mut Scene,
    wall_handle: Handle,
    new_justification: WallJustification,
    library_override: Option<&StyleLibrary>,
) -> bool {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(v2) = wall_from_entity(entity) else {
        return false;
    };

    let total_thickness = v2.total_thickness();
    let old_offset = v2.justification.offset(total_thickness);
    let new_offset = new_justification.offset(total_thickness);
    let delta = new_offset - old_offset;

    if delta.abs() > 1e-9 {
        let vertices = get_wall_vertices(scene, wall_handle);
        if vertices.len() >= 2 {
            let points: Vec<(f64, f64)> = vertices.iter().map(|v| (v.x, v.y)).collect();
            let directions = engine::get_offset_directions(&points);
            let shifted: Vec<DVec3> = points
                .iter()
                .zip(directions.iter())
                .map(|(&(x, y), &(dx, dy))| DVec3::new(x + dx * delta, y + dy * delta, 0.0))
                .collect();
            update_wall_vertices(scene, wall_handle, &shifted);
        }
    }

    let mut record = ExtendedDataRecord::new(AEC_APPID);
    let mut wall = v2;
    wall.justification = new_justification;
    record.values = wall_record_for_wall(&wall);
    if !write_aec_record(&mut scene.document, wall_handle, record) {
        return false;
    }

    let _ = regenerate_wall_representation(scene, wall_handle, library_override);
    true
}
