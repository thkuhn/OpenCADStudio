use super::*;

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
        let effective_height = (height + layer.top_offset - layer.bottom_offset).max(0.0);
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

