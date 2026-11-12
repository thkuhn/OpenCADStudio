//! 2D Floor Plan and Reflected Ceiling Plan (RCP) Derivation Engine.
//!
//! Slices through multi-layer walls, slabs, and openings at a horizontal cut plane (Z = z_cut),
//! extracting horizontal cross-sections (CutBoundary, CutFill with material hatches),
//! opening jambs, sill lines, swing arcs, and slab edges with full provenance.

use super::model_view::{DetailLevel, ModelView};
use super::provenance::{
    DerivedPrimitive, FeatureKey, Geometry2d, PrimitiveId, PrimitiveRole, ResolvedStyle,
    SourceReference, SourceSet,
};
use super::scene_collector::{
    CollectedOpening, CollectedSlab, CollectedWall, CollectedWallJoinData, SceneGeometryCollector,
};
use super::silhouette::{
    point_in_polygon, point_on_polygon_boundary, SilhouetteOptions, TRANSIENT_OCCLUDER_LAYER,
};
use super::style_resolver::{
    is_draw_suppressed, is_opening_slot_visible, is_slab_contour_visible, is_slab_layer_visible,
    is_slab_phase_visible, is_wall_layer_visible, is_wall_phase_visible, resolve_layer_cut_style,
    resolve_opening_representation_levels, resolve_opening_style, resolve_opening_view_style,
    resolve_overhead_lintel_style, resolve_slab_contour_style, ViewStyleContext,
};
use super::viewpoint::{ViewKind, Viewpoint};
use super::{
    DepthInterval, PaperMm, Point2, Point3, ValidationError,
};
use crate::modules::aec::engine::display_component::OpeningComponentSlot;
use crate::modules::aec::engine::library::StyleLibrary;
use crate::modules::aec::engine::opening_style::project_division_sized;
use crate::modules::aec::engine::openings::OpeningKind;
use crate::modules::aec::engine::opening_display::{bake_opening_plan_from_grid, OpeningBakeParams};
use crate::modules::aec::engine::plan_view::DisplayConfig;

const EPS: f64 = 1e-6;

#[derive(Clone, Debug)]
pub struct FloorPlanEngineOptions {
    pub structural_only: bool,
    pub include_openings: bool,
    pub show_door_swings: bool,
    pub show_window_sills: bool,
    pub show_overhead_edges: bool,
    pub emit_hidden_lines: bool,
    pub detail_level: DetailLevel,
    pub display_config: Option<DisplayConfig>,
    pub style_library: Option<StyleLibrary>,
}

impl Default for FloorPlanEngineOptions {
    fn default() -> Self {
        Self {
            structural_only: false,
            include_openings: true,
            show_door_swings: true,
            show_window_sills: true,
            show_overhead_edges: true,
            emit_hidden_lines: false,
            detail_level: DetailLevel::Fine,
            display_config: None,
            style_library: None,
        }
    }
}

/// Derives 2D floor plan or reflected ceiling plan primitives from a Viewpoint and ModelView.
pub fn derive_floorplan_view(
    viewpoint: &Viewpoint,
    model_view: &ModelView,
    scene: &SceneGeometryCollector,
    options: &FloorPlanEngineOptions,
) -> Result<Vec<DerivedPrimitive>, ValidationError> {
    viewpoint.validate()?;
    model_view.validate()?;

    let (cut_offset, _is_rcp) = match &viewpoint.kind {
        ViewKind::FloorPlan { range } => (range.below_cut.get(), false),
        ViewKind::ReflectedCeilingPlan { range } => (range.above_cut.get(), true),
        _ => {
            return Err(ValidationError(
                "viewpoint kind is not a FloorPlan or ReflectedCeilingPlan".to_string(),
            ))
        }
    };

    let z_min = viewpoint.z_range.z_min.get();
    let z_max = viewpoint.z_range.z_max.get();
    let z_cut = (z_min + cut_offset).clamp(z_min, z_max);

    // Deterministic, I/O-free defaults: the caller supplies the active Planart (`DisplayConfig`)
    // and the project/global style library; without them an in-memory seed library and the
    // neutral plan type (everything visible) apply.
    let default_style_lib;
    let style_library = if let Some(lib) = options.style_library.as_ref() {
        lib
    } else {
        default_style_lib = crate::modules::aec::engine::library::seed_default_library();
        &default_style_lib
    };

    let ctx = ViewStyleContext::new(
        model_view,
        options.display_config.as_ref(),
        Some(style_library),
        false,
    );

    let mut primitives = Vec::new();
    let mut primitive_counter: u32 = 0;

    // 1. Process Slabs (Background / Surface edges)
    for slab in &scene.slabs {
        if model_view.excluded_elements.contains(&slab.element_ref) {
            continue;
        }
        if !is_slab_phase_visible(&ctx, slab) || is_draw_suppressed(&ctx, &slab.element_ref, None) {
            continue;
        }
        let (slab_z_min, slab_z_max) = slab.z_range();
        if slab_z_max < z_min || slab_z_min > z_max {
            continue;
        }
        if !slab.layers.is_empty()
            && !slab
                .layers
                .iter()
                .enumerate()
                .any(|(index, _)| is_slab_layer_visible(&ctx, slab, index))
        {
            continue;
        }

        // Slab below cut plane is visible in floor plan
        if slab.polygon.len() >= 3
            && slab_z_min <= z_cut
            && is_slab_contour_visible(&ctx, slab, false)
        {
            let outer: Vec<Point2> = slab
                .polygon
                .iter()
                .map(|p| viewpoint.world_to_view(Point3::new(p.x_m, p.y_m, z_cut)).0)
                .collect();

            let source = SourceReference {
                element: slab.element_ref,
                component_id: None,
                feature: Some(FeatureKey::SlabTop),
                display_component_id: None,
                xref_source: slab.xref_source.clone(),
            };
            let sources = SourceSet::new(vec![source])?;

            let style = resolve_slab_contour_style(&ctx, slab, false);

            let slab_depth = (z_cut - slab_z_max).max(1e-3);
            let slab_thickness = (slab_z_max - slab_z_min).max(0.0);

            primitive_counter += 1;
            primitives.push(DerivedPrimitive {
                id: PrimitiveId::new(model_view.id, 0, primitive_counter),
                role: PrimitiveRole::ProjectedEdge,
                geometry: Geometry2d::Polyline {
                    vertices: outer,
                    closed: true,
                },
                sources,
                depth: DepthInterval::new(slab_depth, slab_depth + slab_thickness)?,
                style,
                edit_handles: Vec::new(),
            });
        }

        // Overhead Slabs (above cut plane, e.g. ceilings, drop beams, balconies)
        if options.show_overhead_edges
            && slab.polygon.len() >= 3
            && slab_z_min > z_cut
            && slab_z_min <= z_max
            && is_slab_contour_visible(&ctx, slab, true)
        {
            let outer: Vec<Point2> = slab
                .polygon
                .iter()
                .map(|p| viewpoint.world_to_view(Point3::new(p.x_m, p.y_m, z_cut)).0)
                .collect();

            let source = SourceReference {
                element: slab.element_ref,
                component_id: None,
                feature: Some(FeatureKey::SlabBottom),
                display_component_id: None,
                xref_source: slab.xref_source.clone(),
            };
            let sources = SourceSet::new(vec![source])?;

            let style = resolve_slab_contour_style(&ctx, slab, true);

            for idx in 0..outer.len() {
                let p0 = outer[idx];
                let p1 = outer[(idx + 1) % outer.len()];
                if (p1.x_m - p0.x_m).hypot(p1.y_m - p0.y_m) >= 1e-4 {
                    primitive_counter += 1;
                    primitives.push(DerivedPrimitive {
                        id: PrimitiveId::new(model_view.id, 0, primitive_counter),
                        role: PrimitiveRole::HiddenEdge,
                        geometry: Geometry2d::Line { start: p0, end: p1 },
                        sources: sources.clone(),
                        depth: DepthInterval::new(0.0, (slab_z_min - z_cut).max(0.0))?,
                        style: style.clone(),
                        edit_handles: Vec::new(),
                    });
                }
            }
        }
    }

    // 2. Process Walls (Horizontally sliced at z_cut with mitered layer junctions)
    let eligible_walls: Vec<(usize, &CollectedWall)> = scene
        .walls
        .iter()
        .enumerate()
        .filter(|(_, wall)| {
            !model_view.excluded_elements.contains(&wall.element_ref)
                && wall.top_z >= z_cut - EPS
                && wall.base_z <= z_cut + EPS
        })
        .collect();

    let wall_join_inputs: Vec<crate::modules::aec::engine::junction_solver::WallJoinInput> =
        eligible_walls
            .iter()
            .map(|(scene_index, wall)| {
                wall_join_input(
                    wall,
                    scene
                        .wall_join_data
                        .get(*scene_index)
                        .and_then(Option::as_ref),
                )
            })
            .collect();

    let solved_network = crate::modules::aec::engine::junction_solver::solve_network(&wall_join_inputs, 0.05);

    for (w_idx, (_, wall)) in eligible_walls.iter().enumerate() {
        let wall_footprints = solved_network.footprints.get(w_idx);

        slice_wall_horizontal(
            wall,
            wall_footprints,
            z_cut,
            z_max,
            viewpoint,
            model_view,
            options,
            &ctx,
            &scene.slabs,
            &mut primitives,
            &mut primitive_counter,
        )?;
    }

    let mut primitives = super::frame::resolve_view_frame(
        primitives,
        &SilhouetteOptions {
            emit_hidden_lines: options.emit_hidden_lines,
            max_depth_distance: None,
            hidden_layer: Some("AEC_HIDDEN".to_string()),
        },
    )?;

    primitives.retain(|p| p.style.layer_name.as_deref() != Some(TRANSIENT_OCCLUDER_LAYER));

    Ok(primitives)
}

fn wall_join_input(
    wall: &CollectedWall,
    join_data: Option<&CollectedWallJoinData>,
) -> crate::modules::aec::engine::junction_solver::WallJoinInput {
    let axis = wall
        .axis_points
        .iter()
        .map(|point| glam::DVec3::new(point.x_m, point.y_m, wall.base_z))
        .collect();
    let layers = wall
        .layers
        .iter()
        .map(|layer| crate::modules::aec::engine::miter::MiterLayer {
            thickness: layer.thickness,
            axis_offset: layer.axis_offset,
            material: layer.material.clone(),
            function: layer.function.clone(),
            layer_id: layer.layer_id,
        })
        .collect();

    crate::modules::aec::engine::junction_solver::WallJoinInput {
        axis,
        bulges: join_data.map(|data| data.bulges.clone()).unwrap_or_default(),
        layers,
        override_start: join_data.and_then(|data| data.override_start.clone()),
        override_end: join_data.and_then(|data| data.override_end.clone()),
        override_span: join_data.and_then(|data| data.override_span.clone()),
    }
}

pub(super) fn solve_wall_layer_footprints(
    scene: &SceneGeometryCollector,
) -> Vec<Vec<Option<Vec<(f64, f64)>>>> {
    let inputs: Vec<_> = scene
        .walls
        .iter()
        .enumerate()
        .map(|(index, wall)| {
            wall_join_input(
                wall,
                scene
                    .wall_join_data
                    .get(index)
                    .and_then(Option::as_ref),
            )
        })
        .collect();
    crate::modules::aec::engine::junction_solver::solve_network(&inputs, 0.05).footprints
}

/// Tests whether an overhead slab contains the opening head position along the wall axis,
/// allowing for half the wall thickness towards interior or exterior.
fn slab_covers_opening_center(
    slab: &CollectedSlab,
    op_center: Point2,
    normal: glam::DVec2,
    half_t: f64,
) -> bool {
    let (s_box_min, s_box_max) = slab.bounding_box_2d();
    let tol = half_t + 1e-3;
    if op_center.x_m < s_box_min.x_m - tol
        || op_center.x_m > s_box_max.x_m + tol
        || op_center.y_m < s_box_min.y_m - tol
        || op_center.y_m > s_box_max.y_m + tol
    {
        return false;
    }

    // 1. Direct point-in-polygon containment of the opening axis center
    let in_ring = point_in_polygon(op_center, &slab.polygon);
    let in_hole = slab
        .openings
        .iter()
        .any(|hole| point_in_polygon(op_center, hole));
    if in_ring && !in_hole {
        return true;
    }

    // 2. Off-axis points allowing for half wall thickness towards interior or exterior
    let pt_in = Point2::new(
        op_center.x_m - normal.x * (half_t * 0.99),
        op_center.y_m - normal.y * (half_t * 0.99),
    );
    let in_ring_in = point_in_polygon(pt_in, &slab.polygon);
    let in_hole_in = slab
        .openings
        .iter()
        .any(|hole| point_in_polygon(pt_in, hole));
    if in_ring_in && !in_hole_in {
        return true;
    }

    let pt_out = Point2::new(
        op_center.x_m + normal.x * (half_t * 0.99),
        op_center.y_m + normal.y * (half_t * 0.99),
    );
    let in_ring_out = point_in_polygon(pt_out, &slab.polygon);
    let in_hole_out = slab
        .openings
        .iter()
        .any(|hole| point_in_polygon(pt_out, hole));
    if in_ring_out && !in_hole_out {
        return true;
    }

    // 3. Proximity to slab outer polygon boundary within wall thickness tolerance
    if point_on_polygon_boundary(op_center, &slab.polygon, tol) {
        let in_deep_hole = slab
            .openings
            .iter()
            .any(|hole| point_in_polygon(op_center, hole) && !point_on_polygon_boundary(op_center, hole, tol));
        if !in_deep_hole {
            return true;
        }
    }

    false
}

fn slice_wall_horizontal(
    wall: &CollectedWall,
    wall_footprints: Option<&Vec<Option<Vec<(f64, f64)>>>>,
    z_cut: f64,
    z_max: f64,
    viewpoint: &Viewpoint,
    model_view: &ModelView,
    options: &FloorPlanEngineOptions,
    ctx: &ViewStyleContext,
    slabs: &[CollectedSlab],
    primitives: &mut Vec<DerivedPrimitive>,
    primitive_counter: &mut u32,
) -> Result<(), ValidationError> {
    if !is_wall_phase_visible(ctx, wall) {
        return Ok(());
    }
    if wall.axis_points.len() < 2 {
        return Ok(());
    }

    let p_start = wall.axis_points[0];
    let p_end = wall.axis_points[1];
    let axis_p0 = glam::DVec2::new(p_start.x_m, p_start.y_m);
    let axis_p1 = glam::DVec2::new(p_end.x_m, p_end.y_m);
    let axis_vec = axis_p1 - axis_p0;
    let axis_len = axis_vec.length();
    if axis_len < EPS {
        return Ok(());
    }
    let axis_dir = axis_vec / axis_len;
    let normal = glam::DVec2::new(-axis_dir.y, axis_dir.x);

    // Filter openings that cut the horizontal plane at z_cut
    let mut cut_openings: Vec<&CollectedOpening> = Vec::new();

    for opening in &wall.openings {
        let op_sill = wall.base_z + opening.sill_height;
        let op_head = op_sill + opening.height;
        if op_sill <= z_cut && op_head >= z_cut {
            cut_openings.push(opening);
        }
    }

    // Sort cut openings along wall axis
    cut_openings.sort_by(|a, b| {
        a.distance_along_axis
            .partial_cmp(&b.distance_along_axis)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut current_offset = -wall.total_thickness * 0.5;

    for (layer_idx, layer) in wall.layers.iter().enumerate() {
        if !is_wall_layer_visible(ctx, wall, layer_idx) {
            current_offset += layer.thickness;
            continue;
        }

        let (mut boundary_style_opt, mut fill_style_opt) = resolve_layer_cut_style(ctx, wall, layer_idx);
        if options.structural_only && !layer.is_structural {
            boundary_style_opt = None;
            fill_style_opt = None;
        }

        let y_min_offset = current_offset;
        let y_max_offset = current_offset + layer.thickness;
        current_offset += layer.thickness;

        // Partition wall along axis by cut openings
        let mut axis_segments: Vec<(f64, f64)> = Vec::new();
        let mut cur_s = 0.0;

        for op in &cut_openings {
            let op_start = (op.distance_along_axis - op.width * 0.5).max(0.0);
            let op_end = (op.distance_along_axis + op.width * 0.5).min(axis_len);

            if op_start > cur_s + EPS {
                axis_segments.push((cur_s, op_start));
            }
            cur_s = cur_s.max(op_end);
        }
        if cur_s + EPS < axis_len {
            axis_segments.push((cur_s, axis_len));
        }

        let footprint_opt = wall_footprints
            .and_then(|fps| fps.get(layer_idx))
            .and_then(|f| f.as_ref());

        let wall_axis_2d: Vec<(f64, f64)> = wall.axis_points.iter().map(|p| (p.x_m, p.y_m)).collect();
        let opening_objs: Vec<crate::modules::aec::engine::openings::Opening> = cut_openings
            .iter()
            .map(|op| {
                crate::modules::aec::engine::openings::Opening {
                    handle: acadrust::Handle::new(0),
                    host_wall: acadrust::Handle::new(0),
                    distance_along_axis: op.distance_along_axis,
                    width: op.width,
                    height: op.height,
                    sill_height: op.sill_height,
                    kind: op.kind,
                    style_id: op.style_id.clone(),
                    division_override: None,
                    hinge: op.hinge,
                    shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
                    spring_height: 0.0,
                    reference_side: crate::modules::aec::engine::openings::OpeningReferenceSide::Center,
                    cross_axis_offset: 0.0,
                    depth: None,
                    niche_side: crate::modules::aec::engine::openings::NicheSide::Exterior,
                    swing_side: op.swing_side,
                    sill_plane_id: None,
                    head_plane_id: None,
                    sill_plane_name: None,
                    head_plane_name: None,
                    sill_offset: 0.0,
                    head_offset: 0.0,
                    sill_origin: [0.0; 3],
                    sill_normal: [0.0, 0.0, 1.0],
                    head_origin: [0.0; 3],
                    head_normal: [0.0, 0.0, 1.0],
                    formulas: crate::modules::aec::engine::formulas::AecFormulas::default(),
                }
            })
            .collect();

        let layer_rings: Vec<Vec<Point2>> = if let Some(mitered_poly) = footprint_opt {
            if !opening_objs.is_empty() {
                let split_rings = crate::modules::aec::engine::miter::split_mitered_footprint_by_openings(
                    mitered_poly,
                    &wall_axis_2d,
                    &opening_objs,
                );
                split_rings
                    .into_iter()
                    .map(|ring| {
                        ring.into_iter()
                            .map(|(x, y)| viewpoint.world_to_view(Point3::new(x, y, z_cut)).0)
                            .collect()
                    })
                    .collect()
            } else {
                vec![
                    mitered_poly
                        .iter()
                        .map(|&(x, y)| viewpoint.world_to_view(Point3::new(x, y, z_cut)).0)
                        .collect()
                ]
            }
        } else {
            axis_segments
                .iter()
                .map(|&(s0, s1)| {
                    let p0 = axis_p0 + axis_dir * s0 + normal * y_min_offset;
                    let p1 = axis_p0 + axis_dir * s1 + normal * y_min_offset;
                    let p2 = axis_p0 + axis_dir * s1 + normal * y_max_offset;
                    let p3 = axis_p0 + axis_dir * s0 + normal * y_max_offset;
                    vec![
                        viewpoint.world_to_view(Point3::new(p0.x, p0.y, z_cut)).0,
                        viewpoint.world_to_view(Point3::new(p1.x, p1.y, z_cut)).0,
                        viewpoint.world_to_view(Point3::new(p2.x, p2.y, z_cut)).0,
                        viewpoint.world_to_view(Point3::new(p3.x, p3.y, z_cut)).0,
                    ]
                })
                .collect()
        };

        for ring in layer_rings {
            if ring.len() < 3 {
                continue;
            }

            let source = SourceReference {
                element: wall.element_ref,
                component_id: Some(layer.component_id),
                feature: Some(FeatureKey::LayerFace),
                display_component_id: None,
                xref_source: wall.xref_source.clone(),
            };
            let sources = SourceSet::new(vec![source])?;

            // Boundary
            if let Some(boundary_style) = boundary_style_opt.as_ref() {
                *primitive_counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                    role: PrimitiveRole::CutBoundary,
                    geometry: Geometry2d::Polyline {
                        vertices: ring.clone(),
                        closed: true,
                    },
                    sources: sources.clone(),
                    depth: DepthInterval::cut_plane(),
                    style: boundary_style.clone(),
                    edit_handles: Vec::new(),
                });
            }

            // Hatch fill
            if let Some(fill_style) = fill_style_opt.as_ref() {
                *primitive_counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                    role: PrimitiveRole::CutFill,
                    geometry: Geometry2d::Region {
                        outer: ring,
                        holes: Vec::new(),
                    },
                    sources,
                    depth: DepthInterval::cut_plane(),
                    style: fill_style.clone(),
                    edit_handles: Vec::new(),
                });
            } else {
                // When hatches are disabled, emit a transient CutFill occluder so background lines
                // are still properly occluded by the cut wall. This is cleaned up after silhouette processing.
                *primitive_counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                    role: PrimitiveRole::CutFill,
                    geometry: Geometry2d::Region {
                        outer: ring,
                        holes: Vec::new(),
                    },
                    sources,
                    depth: DepthInterval::cut_plane(),
                    style: ResolvedStyle {
                        line_weight_mm: PaperMm::new(0.18).unwrap(),
                        line_color: None,
                        line_type: None,
                        fill_color: None,
                        hatch_pattern: None,
                        hatch_scale: None,
                        hatch_angle_rad: None,
                        layer_name: Some(TRANSIENT_OCCLUDER_LAYER.to_string()),
                    },
                    edit_handles: Vec::new(),
                });
            }
        }
    }

    let host_wall_draw_suppressed = is_draw_suppressed(ctx, &wall.element_ref, None);

    // Transient Parapet Occluder for Windows and Breakthroughs with a sill (below sill)
    // Emitted outside `options.include_openings` because the wall body is partitioned by cut openings
    // regardless of whether opening symbols (jambs, sills, swings) are rendered.
    // Preserves transparency for doors while preventing below-cut slab edges from showing solid
    // through window/breakthrough parapet gaps.
    if !host_wall_draw_suppressed {
        for op in &cut_openings {
            if !matches!(op.kind, OpeningKind::Door) && op.sill_height > 1e-4 {
                let op_start_s = (op.distance_along_axis - op.width * 0.5).max(0.0);
                let op_end_s = (op.distance_along_axis + op.width * 0.5).min(axis_len);

                let half_t = wall.total_thickness * 0.5;
                let j_left_in = axis_p0 + axis_dir * op_start_s - normal * half_t;
                let j_left_out = axis_p0 + axis_dir * op_start_s + normal * half_t;
                let j_right_in = axis_p0 + axis_dir * op_end_s - normal * half_t;
                let j_right_out = axis_p0 + axis_dir * op_end_s + normal * half_t;

                let uv_j_left_in = viewpoint.world_to_view(Point3::new(j_left_in.x, j_left_in.y, z_cut)).0;
                let uv_j_left_out = viewpoint.world_to_view(Point3::new(j_left_out.x, j_left_out.y, z_cut)).0;
                let uv_j_right_in = viewpoint.world_to_view(Point3::new(j_right_in.x, j_right_in.y, z_cut)).0;
                let uv_j_right_out = viewpoint.world_to_view(Point3::new(j_right_out.x, j_right_out.y, z_cut)).0;

                let op_source = SourceReference {
                    element: op.element_ref,
                    component_id: None,
                    feature: Some(FeatureKey::OpeningSill),
                    display_component_id: None,
                    xref_source: wall.xref_source.clone(),
                };
                let op_sources = SourceSet::new(vec![op_source])?;

                let op_sill = wall.base_z + op.sill_height;
                let sill_depth = (z_cut - op_sill).max(1e-3);
                let parapet_ring = vec![uv_j_left_in, uv_j_right_in, uv_j_right_out, uv_j_left_out];
                *primitive_counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                    role: PrimitiveRole::SurfaceFill,
                    geometry: Geometry2d::Region {
                        outer: parapet_ring,
                        holes: Vec::new(),
                    },
                    sources: op_sources,
                    depth: DepthInterval::new(sill_depth, sill_depth)?,
                    style: ResolvedStyle {
                        line_weight_mm: PaperMm::new(0.18)?,
                        line_color: None,
                        line_type: None,
                        fill_color: None,
                        hatch_pattern: None,
                        hatch_scale: None,
                        hatch_angle_rad: None,
                        layer_name: Some(TRANSIENT_OCCLUDER_LAYER.to_string()),
                    },
                    edit_handles: Vec::new(),
                });
            }
        }
    }

    // 3. Emit Opening details (Jambs, Sill, Swing)
    if options.include_openings && !host_wall_draw_suppressed {
        for op in &cut_openings {
            let op_start_s = (op.distance_along_axis - op.width * 0.5).max(0.0);
            let op_end_s = (op.distance_along_axis + op.width * 0.5).min(axis_len);

            let half_t = wall.total_thickness * 0.5;
            let j_left_in = axis_p0 + axis_dir * op_start_s - normal * half_t;
            let j_left_out = axis_p0 + axis_dir * op_start_s + normal * half_t;
            let j_right_in = axis_p0 + axis_dir * op_end_s - normal * half_t;
            let j_right_out = axis_p0 + axis_dir * op_end_s + normal * half_t;

            let uv_j_left_in = viewpoint.world_to_view(Point3::new(j_left_in.x, j_left_in.y, z_cut)).0;
            let uv_j_left_out = viewpoint.world_to_view(Point3::new(j_left_out.x, j_left_out.y, z_cut)).0;
            let uv_j_right_in = viewpoint.world_to_view(Point3::new(j_right_in.x, j_right_in.y, z_cut)).0;
            let uv_j_right_out = viewpoint.world_to_view(Point3::new(j_right_out.x, j_right_out.y, z_cut)).0;

            let op_source = SourceReference {
                element: op.element_ref,
                component_id: None,
                feature: Some(FeatureKey::OpeningSill),
                display_component_id: None,
                xref_source: wall.xref_source.clone(),
            };

            let op_sources = SourceSet::new(vec![op_source.clone()])?;

            let style = resolve_opening_style(ctx, op);
            let construction = op.division_override.as_ref().unwrap_or(&style.grid);
            let grid = project_division_sized(construction, op.width, op.height);
            let mut levels = resolve_opening_representation_levels(ctx, op, &grid);
            if !options.show_door_swings {
                levels.swing = crate::modules::aec::engine::opening_style::SwingGlyph::Off;
            }
            let half_t = wall.total_thickness * 0.5;
            let bake_params = OpeningBakeParams {
                width: op.width,
                height: op.height,
                sill_height: op.sill_height,
                thickness: wall.total_thickness,
                frame_thickness: style.parts.fixed_frame.face_width,
                cross_axis_offset: op.cross_axis_offset,
                wall_y_min: -half_t,
                wall_y_max: half_t,
                hinge: op.hinge,
                swing_side: op.swing_side,
                shape: op.shape,
                spring_height: op.spring_height,
                opening_angle_deg: style.opening_angle_deg,
                kind: op.kind,
            };
            let opening_origin = axis_p0 + axis_dir * op.distance_along_axis
                + normal * op.cross_axis_offset;
            for baked in bake_opening_plan_from_grid(
                bake_params,
                &style.parts,
                ctx.style_library,
                construction,
                levels.plan_frame,
                levels.swing,
                levels.divisions,
            ) {
                if !is_opening_slot_visible(ctx, op, baked.slot) || baked.points.len() < 2 {
                    continue;
                }
                if baked.slot == OpeningComponentSlot::Swing2D
                    && levels.swing == crate::modules::aec::engine::opening_style::SwingGlyph::Arc
                {
                    continue;
                }
                let projected: Vec<Point2> = baked
                    .points
                    .iter()
                    .map(|&(x, y)| {
                        viewpoint
                            .world_to_view(Point3::new(
                                opening_origin.x + axis_dir.x * x + normal.x * y,
                                opening_origin.y + axis_dir.y * x + normal.y * y,
                                z_cut,
                            ))
                            .0
                    })
                    .collect();
                let geometry = if baked.closed {
                    Geometry2d::Region {
                        outer: projected,
                        holes: vec![],
                    }
                } else if projected.len() == 2 {
                    Geometry2d::Line {
                        start: projected[0],
                        end: projected[1],
                    }
                } else {
                    Geometry2d::Polyline {
                        vertices: projected,
                        closed: false,
                    }
                };
                let mut source = op_source.clone();
                source.feature = Some(match baked.slot {
                    OpeningComponentSlot::Swing2D => FeatureKey::OpeningLeftJamb,
                    OpeningComponentSlot::Frame2D => FeatureKey::OpeningLeftJamb,
                    _ => FeatureKey::OpeningSill,
                });
                *primitive_counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                    role: PrimitiveRole::Symbol,
                    geometry,
                    sources: SourceSet::new(vec![source])?,
                    depth: DepthInterval::cut_plane(),
                    style: resolve_opening_view_style(ctx, op, baked.slot),
                    edit_handles: Vec::new(),
                });
            }
            if levels.swing == crate::modules::aec::engine::opening_style::SwingGlyph::Arc
                && is_opening_slot_visible(ctx, op, OpeningComponentSlot::Swing2D)
            {
                if let Ok(cells) = grid.layout(op.width, op.height, op.kind) {
                    for cell in cells {
                        if cell.cell.kind
                            != crate::modules::aec::engine::opening_style::OpeningCellKind::DoorLeaf
                        {
                            continue;
                        }
                        let hinge = cell.cell.hinge.unwrap_or(op.hinge);
                        let center_x = match hinge.bearing() {
                            crate::modules::aec::engine::construction_grid::TrackEnd::Start => cell.left,
                            crate::modules::aec::engine::construction_grid::TrackEnd::End => cell.right,
                        };
                        let radius = cell.right - cell.left;
                        let angle = style.opening_angle_deg.abs().to_radians();
                        let swing_sign = match op.swing_side {
                            crate::modules::aec::engine::openings::SwingSide::Exterior => 1.0,
                            crate::modules::aec::engine::openings::SwingSide::Interior => -1.0,
                        };
                        let local_arc_point = |theta: f64| {
                            let x = match hinge.bearing() {
                                crate::modules::aec::engine::construction_grid::TrackEnd::Start => {
                                    center_x + radius * theta.cos()
                                }
                                crate::modules::aec::engine::construction_grid::TrackEnd::End => {
                                    center_x - radius * theta.cos()
                                }
                            };
                            (x, swing_sign * radius * theta.sin())
                        };
                        let project_local = |x: f64, y: f64| {
                            viewpoint
                                .world_to_view(Point3::new(
                                    opening_origin.x + axis_dir.x * x + normal.x * y,
                                    opening_origin.y + axis_dir.y * x + normal.y * y,
                                    z_cut,
                                ))
                                .0
                        };
                        let center = project_local(center_x, 0.0);
                        let (start_x, start_y) = local_arc_point(0.0);
                        let (mid_x, mid_y) = local_arc_point(angle * 0.5);
                        let (end_x, end_y) = local_arc_point(angle);
                        let start = project_local(start_x, start_y);
                        let mid = project_local(mid_x, mid_y);
                        let end = project_local(end_x, end_y);
                        let start_dx = start.x_m - center.x_m;
                        let start_dy = start.y_m - center.y_m;
                        let mid_dx = mid.x_m - center.x_m;
                        let mid_dy = mid.y_m - center.y_m;
                        let end_angle = (end.y_m - center.y_m).atan2(end.x_m - center.x_m);
                        let start_angle = start_dy.atan2(start_dx);
                        let mut sweep = end_angle - start_angle;
                        while sweep > std::f64::consts::PI {
                            sweep -= std::f64::consts::TAU;
                        }
                        while sweep < -std::f64::consts::PI {
                            sweep += std::f64::consts::TAU;
                        }
                        let direction = start_dx * mid_dy - start_dy * mid_dx;
                        if direction >= 0.0 && sweep < 0.0 {
                            sweep += std::f64::consts::TAU;
                        } else if direction < 0.0 && sweep > 0.0 {
                            sweep -= std::f64::consts::TAU;
                        }
                        let mut source = op_source.clone();
                        source.feature = Some(match hinge.bearing() {
                            crate::modules::aec::engine::construction_grid::TrackEnd::Start => {
                                FeatureKey::OpeningLeftJamb
                            }
                            crate::modules::aec::engine::construction_grid::TrackEnd::End => {
                                FeatureKey::OpeningRightJamb
                            }
                        });
                        *primitive_counter += 1;
                        primitives.push(DerivedPrimitive {
                            id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                            role: PrimitiveRole::Symbol,
                            geometry: Geometry2d::CircularArc {
                                center,
                                radius: super::PositiveMetres::new(radius)?,
                                start_rad: start_angle,
                                sweep_rad: sweep,
                            },
                            sources: SourceSet::new(vec![source])?,
                            depth: DepthInterval::cut_plane(),
                            style: resolve_opening_view_style(ctx, op, OpeningComponentSlot::Swing2D),
                            edit_handles: Vec::new(),
                        });
                    }
                }
            }

            // Window Sill Lines (Sill2D)
            if options.show_window_sills && matches!(op.kind, OpeningKind::Window) && is_opening_slot_visible(ctx, op, OpeningComponentSlot::Sill2D) {
                let sill_style = resolve_opening_view_style(ctx, op, OpeningComponentSlot::Sill2D);
                let op_sill = wall.base_z + op.sill_height;
                let sill_depth = (z_cut - op_sill).max(1e-3);
                *primitive_counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                    role: PrimitiveRole::ProjectedEdge,
                    geometry: Geometry2d::Line {
                        start: uv_j_left_out,
                        end: uv_j_right_out,
                    },
                    sources: op_sources.clone(),
                    depth: DepthInterval::new(sill_depth, sill_depth + 0.1)?,
                    style: sill_style.clone(),
                    edit_handles: Vec::new(),
                });

                *primitive_counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                    role: PrimitiveRole::ProjectedEdge,
                    geometry: Geometry2d::Line {
                        start: uv_j_left_in,
                        end: uv_j_right_in,
                    },
                    sources: op_sources.clone(),
                    depth: DepthInterval::new(sill_depth, sill_depth + 0.1)?,
                    style: sill_style,
                    edit_handles: Vec::new(),
                });
            }


            // Overhead Opening Head (lintel/Sturz) if not full-height / headless
            if options.show_overhead_edges {
                let op_dist = op.distance_along_axis;
                let op_center = axis_p0 + axis_dir * op_dist;
                let op_pt = Point2::new(op_center.x, op_center.y);

                let opening_slab_underside = slabs
                    .iter()
                    .filter(|s| {
                        let (s_min, _) = s.z_range();
                        s_min > z_cut && s_min <= z_max
                    })
                    .filter(|s| slab_covers_opening_center(s, op_pt, normal, half_t))
                    .map(|s| s.z_range().0)
                    .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

                let effective_wall_top = match opening_slab_underside {
                    Some(slab_bottom) => wall.top_z.min(slab_bottom),
                    None => wall.top_z,
                };

                let op_sill = wall.base_z + op.sill_height;
                let op_head = op_sill + op.height;
                let is_window = matches!(op.kind, OpeningKind::Window);
                let has_visible_sill = is_window && options.show_window_sills;
                if op_head < effective_wall_top - 1e-4 && op_head <= z_max + 1e-4 && !has_visible_sill {
                    let mut head_source = op_source.clone();
                    head_source.feature = Some(FeatureKey::OpeningHead);
                    let head_sources = SourceSet::new(vec![head_source])?;

                    let overhead_style = resolve_overhead_lintel_style(ctx, op);

                    *primitive_counter += 1;
                    primitives.push(DerivedPrimitive {
                        id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                        role: PrimitiveRole::HiddenEdge,
                        geometry: Geometry2d::Line {
                            start: uv_j_left_out,
                            end: uv_j_right_out,
                        },
                        sources: head_sources.clone(),
                        depth: DepthInterval::new(0.0, (op_head - z_cut).max(0.0))?,
                        style: overhead_style.clone(),
                        edit_handles: Vec::new(),
                    });

                    *primitive_counter += 1;
                    primitives.push(DerivedPrimitive {
                        id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                        role: PrimitiveRole::HiddenEdge,
                        geometry: Geometry2d::Line {
                            start: uv_j_left_in,
                            end: uv_j_right_in,
                        },
                        sources: head_sources,
                        depth: DepthInterval::new(0.0, (op_head - z_cut).max(0.0))?,
                        style: overhead_style,
                        edit_handles: Vec::new(),
                    });
                }
            }
        }

        // Overhead openings strictly above the cut plane
        if options.show_overhead_edges && options.include_openings && !host_wall_draw_suppressed {
            let overhead_openings: Vec<&CollectedOpening> = wall
                .openings
                .iter()
                .filter(|op| {
                    let op_sill = wall.base_z + op.sill_height;
                    op_sill > z_cut + EPS && op_sill < wall.top_z + EPS && op_sill <= z_max
                })
                .collect();

            for op in overhead_openings {
                let op_dist = op.distance_along_axis;
                let half_w = op.width * 0.5;
                let s_left = (op_dist - half_w).max(0.0);
                let s_right = (op_dist + half_w).min(axis_len);

                let half_thick = wall.total_thickness * 0.5;
                let j_left_in = axis_p0 + axis_dir * s_left - normal * half_thick;
                let j_left_out = axis_p0 + axis_dir * s_left + normal * half_thick;
                let j_right_in = axis_p0 + axis_dir * s_right - normal * half_thick;
                let j_right_out = axis_p0 + axis_dir * s_right + normal * half_thick;

                let uv_j_left_in = viewpoint.world_to_view(Point3::new(j_left_in.x, j_left_in.y, z_cut)).0;
                let uv_j_left_out = viewpoint.world_to_view(Point3::new(j_left_out.x, j_left_out.y, z_cut)).0;
                let uv_j_right_in = viewpoint.world_to_view(Point3::new(j_right_in.x, j_right_in.y, z_cut)).0;
                let uv_j_right_out = viewpoint.world_to_view(Point3::new(j_right_out.x, j_right_out.y, z_cut)).0;

                let op_source = SourceReference {
                    element: op.element_ref,
                    component_id: None,
                    feature: Some(FeatureKey::OpeningHead),
                    display_component_id: None,
                    xref_source: op.xref_source.clone(),
                };
                let head_sources = SourceSet::new(vec![op_source])?;

                let overhead_style = resolve_overhead_lintel_style(ctx, op);

                let op_sill = wall.base_z + op.sill_height;
                let depth = DepthInterval::new(0.0, (op_sill - z_cut).max(0.0))?;

                // Emit 2 jamb outline edges crossing the wall thickness
                for (start, end) in [
                    (uv_j_left_out, uv_j_left_in),
                    (uv_j_right_out, uv_j_right_in),
                ] {
                    *primitive_counter += 1;
                    primitives.push(DerivedPrimitive {
                        id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                        role: PrimitiveRole::HiddenEdge,
                        geometry: Geometry2d::Line { start, end },
                        sources: head_sources.clone(),
                        depth,
                        style: overhead_style.clone(),
                        edit_handles: Vec::new(),
                    });
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests;
