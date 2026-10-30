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
    point_in_polygon, point_on_polygon_boundary, process_silhouette_and_depth,
    SilhouetteOptions, TRANSIENT_OCCLUDER_LAYER,
};
use super::style_resolver::{
    is_draw_suppressed, is_opening_slot_visible, is_slab_contour_visible, is_slab_phase_visible,
    is_wall_layer_active, is_wall_phase_visible, resolve_layer_cut_style,
    resolve_opening_view_style, resolve_overhead_lintel_style, resolve_slab_contour_style,
    ViewStyleContext,
};
use super::viewpoint::{ViewKind, Viewpoint};
use super::{
    DepthInterval, PaperMm, Point2, Point3, ValidationError,
};
use crate::modules::aec::engine::display_component::OpeningComponentSlot;
use crate::modules::aec::engine::library::StyleLibrary;
use crate::modules::aec::engine::openings::OpeningKind;
use crate::modules::aec::engine::opening_style::HingeSide;
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

    process_silhouette_and_depth(
        &mut primitives,
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
        if !is_wall_layer_active(ctx, wall, layer_idx) {
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

            // Left & Right Jamb (Frame2D)
            if is_opening_slot_visible(ctx, op, OpeningComponentSlot::Frame2D) {
                let frame_style = resolve_opening_view_style(ctx, op, OpeningComponentSlot::Frame2D);

                // Left Jamb
                let mut left_source = op_source.clone();
                left_source.feature = Some(FeatureKey::OpeningLeftJamb);
                let left_sources = SourceSet::new(vec![left_source])?;

                *primitive_counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                    role: PrimitiveRole::CutBoundary,
                    geometry: Geometry2d::Line {
                        start: uv_j_left_in,
                        end: uv_j_left_out,
                    },
                    sources: left_sources,
                    depth: DepthInterval::cut_plane(),
                    style: frame_style.clone(),
                    edit_handles: Vec::new(),
                });

                // Right Jamb
                let mut right_source = op_source.clone();
                right_source.feature = Some(FeatureKey::OpeningRightJamb);
                let right_sources = SourceSet::new(vec![right_source])?;

                *primitive_counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                    role: PrimitiveRole::CutBoundary,
                    geometry: Geometry2d::Line {
                        start: uv_j_right_in,
                        end: uv_j_right_out,
                    },
                    sources: right_sources,
                    depth: DepthInterval::cut_plane(),
                    style: frame_style,
                    edit_handles: Vec::new(),
                });
            }

            let op_sources = SourceSet::new(vec![op_source.clone()])?;

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

            // Door Swing Arc (Swing2D)
            if options.show_door_swings && matches!(op.kind, OpeningKind::Door) && is_opening_slot_visible(ctx, op, OpeningComponentSlot::Swing2D) {
                let swing_style = resolve_opening_view_style(ctx, op, OpeningComponentSlot::Swing2D);
                let hinge_pt = match op.hinge {
                    HingeSide::Left => j_left_in,
                    HingeSide::Right => j_right_in,
                };

                let leaf_end = hinge_pt - normal * op.width;
                let uv_hinge = viewpoint.world_to_view(Point3::new(hinge_pt.x, hinge_pt.y, z_cut)).0;
                let uv_leaf_end = viewpoint.world_to_view(Point3::new(leaf_end.x, leaf_end.y, z_cut)).0;

                // Leaf Line
                *primitive_counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                    role: PrimitiveRole::Symbol,
                    geometry: Geometry2d::Line {
                        start: uv_hinge,
                        end: uv_leaf_end,
                    },
                    sources: op_sources.clone(),
                    depth: DepthInterval::cut_plane(),
                    style: swing_style.clone(),
                    edit_handles: Vec::new(),
                });

                // Swing Arc
                let r = op.width;
                if r > 0.1 {
                    let leaf_dx = uv_leaf_end.x_m - uv_hinge.x_m;
                    let leaf_dy = uv_leaf_end.y_m - uv_hinge.y_m;
                    let start_ang = leaf_dy.atan2(leaf_dx);
                    let sweep_rad = match op.hinge {
                        HingeSide::Left => 1.5707963267948966,
                        HingeSide::Right => -1.5707963267948966,
                    };
                    *primitive_counter += 1;
                    primitives.push(DerivedPrimitive {
                        id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                        role: PrimitiveRole::Symbol,
                        geometry: Geometry2d::CircularArc {
                            center: uv_hinge,
                            radius: super::PositiveMetres::new(r)?,
                            start_rad: start_ang,
                            sweep_rad,
                        },
                        sources: op_sources,
                        depth: DepthInterval::cut_plane(),
                        style: swing_style,
                        edit_handles: Vec::new(),
                    });
                }
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
mod tests {
    use super::*;
    use crate::modules::aec::engine::opening_shape::OpeningShape;
    use crate::modules::aec::engine::opening_style::HingeSide;
    use crate::modules::aec::engine::openings::{OpeningKind, OpeningReferenceSide};
    use crate::modules::aec::engine::views::model_view::{DetailPolicy, ViewRevisions};
    use crate::modules::aec::engine::views::scene_collector::{
        CollectedOpening, CollectedSlab, CollectedSlabLayer, CollectedWall,
        CollectedWallJoinData, CollectedWallLayer,
    };
    use crate::modules::aec::engine::views::viewpoint::{PlanRange, VerticalRange, ViewAxes, ViewOrigin};
    use crate::modules::aec::engine::views::{
        ComponentId, ElementId, ElementRef, Metres, ModelId, ModelViewId, NonNegativeMetres, Point3,
        Revision, ScaleDenominator, ViewProfileId, ViewpointId,
    };
    use crate::modules::aec::engine::wall::WallJustification;
    use crate::modules::aec::engine::plan_view::PlanPhase;

    #[test]
    fn test_floorplan_derivation_with_wall_and_slab() {
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let viewpoint = Viewpoint {
            id: vp_id,
            name: "Ground Floor".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.8).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(2.8).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };

        let model_view = ModelView {
            id: mv_id,
            name: "1:50 Floor Plan".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let wall = CollectedWall {
            handle: acadrust::Handle::from(100u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "WallStyle_Exterior".into(),
            height: 2.8,
            base_z: 0.0,
            top_z: 2.8,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Masonry".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: Some("ANSI31".into()),
                layer_id: uuid::Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: Vec::new(),
            phase: PlanPhase::New,
        };

        let slab = CollectedSlab {
            handle: acadrust::Handle::from(200u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "Slab_20".into(),
            base_z: 0.0,
            total_thickness: 0.20,
            polygon: vec![
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(10.0, 8.0),
                Point2::new(0.0, 8.0),
            ],
            layers: Vec::new(),
            justification: crate::modules::aec::engine::slab::SlabJustification::Top,
            openings: Vec::new(),
            phase: PlanPhase::New,
        };

        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);
        collector.slabs.push(slab);

        let primitives = derive_floorplan_view(
            &viewpoint,
            &model_view,
            &collector,
            &FloorPlanEngineOptions::default(),
        )
        .expect("floor plan derived successfully");

        assert!(!primitives.is_empty(), "primitives should not be empty");
        let has_cut_fill = primitives.iter().any(|p| p.role == PrimitiveRole::CutFill);
        let has_cut_boundary = primitives.iter().any(|p| p.role == PrimitiveRole::CutBoundary);
        assert!(has_cut_fill, "should contain cut fill with hatch");
        assert!(has_cut_boundary, "should contain cut boundary");
    }

    #[test]
    fn test_floorplan_hatchless_suppresses_cut_fill_and_retains_masks() {
        use crate::modules::aec::engine::plan_view::{PlanningStage, ViewType};
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let viewpoint = Viewpoint {
            id: vp_id,
            name: "Ground Floor Hatchless".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 1.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.8).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };

        let model_view = ModelView {
            id: mv_id,
            name: "1:50 Hatchless Floor Plan".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let wall = CollectedWall {
            handle: acadrust::Handle::from(100u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "Wall_24".into(),
            height: 2.8,
            base_z: 0.0,
            top_z: 2.8,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Masonry".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: Some("ANSI31".into()),
                layer_id: uuid::Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: Vec::new(),
            phase: PlanPhase::New,
        };

        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);

        let mut dc = DisplayConfig::new(
            "Draft".to_string(),
            "Architektur".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        dc.detail.hatches = false;

        let primitives = derive_floorplan_view(
            &viewpoint,
            &model_view,
            &collector,
            &FloorPlanEngineOptions {
                display_config: Some(dc),
                ..FloorPlanEngineOptions::default()
            },
        )
        .expect("floor plan derived successfully");

        assert!(!primitives.is_empty(), "primitives should not be empty");
        let has_cut_fill = primitives.iter().any(|p| p.role == PrimitiveRole::CutFill);
        let has_cut_boundary = primitives.iter().any(|p| p.role == PrimitiveRole::CutBoundary);
        assert!(!has_cut_fill, "should NOT contain cut fill when hatches are disabled");
        assert!(has_cut_boundary, "should still contain cut boundary");
        assert!(!primitives.iter().any(|p| p.style.layer_name.as_deref() == Some(TRANSIENT_OCCLUDER_LAYER)), "transient occluders must be pruned");
    }

    fn plan_fixture() -> (Viewpoint, ModelView) {
        let vp_id = ViewpointId::new();
        let viewpoint = Viewpoint {
            id: vp_id,
            name: "Style Plan".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 1.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.8).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };
        let model_view = ModelView {
            id: ModelViewId::new(),
            name: "Style Plan 1:50".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };
        (viewpoint, model_view)
    }

    fn style_layer(material: &str, thickness: f64, axis_offset: f64, structural: bool) -> CollectedWallLayer {
        CollectedWallLayer {
            material: material.into(),
            thickness,
            function: if structural { "Structural" } else { "Finish" }.into(),
            axis_offset,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
            component_id: ComponentId::new(),
            is_structural: structural,
        }
    }

    fn style_wall(layers: Vec<CollectedWallLayer>, axis: [(f64, f64); 2]) -> CollectedWall {
        let total = layers.iter().map(|l| l.thickness).sum();
        CollectedWall {
            handle: acadrust::Handle::from(100u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "Wall_Style".into(),
            height: 2.8,
            base_z: 0.0,
            top_z: 2.8,
            axis_points: vec![Point2::new(axis[0].0, axis[0].1), Point2::new(axis[1].0, axis[1].1)],
            layers,
            total_thickness: total,
            justification: WallJustification::Center,
            openings: Vec::new(),
            phase: PlanPhase::New,
        }
    }

    #[test]
    fn test_wall_join_input_preserves_bulges_overrides_and_l_footprint() {
        use crate::modules::aec::engine::join::{JoinOverrideStyle, JunctionOverride};
        use crate::modules::aec::engine::junction_solver::{self, SolvedJoinKind};
        use crate::modules::aec::engine::join::JUNCTION_TOLERANCE;

        let wall_a = style_wall(
            vec![style_layer("core", 0.2, -0.1, true)],
            [(0.0, 0.0), (10.0, 0.0)],
        );
        let wall_b = style_wall(
            vec![style_layer("core", 0.2, -0.1, true)],
            [(10.0, 0.0), (10.0, 10.0)],
        );
        let override_end = JunctionOverride {
            default_style: Some(JoinOverrideStyle::FarFace),
            ..Default::default()
        };
        let override_span = JunctionOverride {
            default_style: Some(JoinOverrideStyle::NoExtend),
            ..Default::default()
        };
        let join_data = CollectedWallJoinData {
            bulges: vec![0.0, 0.0],
            override_start: None,
            override_end: Some(override_end.clone()),
            override_span: Some(override_span.clone()),
        };
        let input_a = wall_join_input(&wall_a, Some(&join_data));
        let input_b = wall_join_input(&wall_b, None);
        assert_eq!(input_a.bulges, vec![0.0, 0.0]);
        assert_eq!(input_a.override_end, Some(override_end));
        assert_eq!(input_a.override_span, Some(override_span));

        let automatic = junction_solver::solve_network(
            &[wall_join_input(&wall_a, None), input_b.clone()],
            JUNCTION_TOLERANCE,
        );
        let overridden = junction_solver::solve_network(
            &[input_a, input_b],
            JUNCTION_TOLERANCE,
        );
        assert_eq!(overridden.junctions[0].kind, SolvedJoinKind::L);
        assert_ne!(
            automatic.footprints[0][0], overridden.footprints[0][0],
            "the collected endpoint override should reach the L-junction footprint"
        );
    }

    fn concrete_material() -> crate::modules::aec::engine::material::Material {
        let mut mat = crate::modules::aec::engine::material::Material::new(
            "Concrete".to_string(),
            "Stahlbeton".to_string(),
            "AR-CONC".to_string(),
            0x303030,
            "Continuous".to_string(),
        );
        mat.hatch_scale = 0.5;
        mat.hatch_angle = 45.0;
        mat.hatch_angle_relative = true;
        mat
    }

    #[test]
    fn test_floorplan_material_pattern_scale_angle_reach_cut_fill() {
        let (viewpoint, model_view) = plan_fixture();
        // Wall along +Y: the plan hatch angle is wall-relative (90° + 45°).
        let wall = style_wall(vec![style_layer("Concrete", 0.24, 0.0, true)], [(0.0, 0.0), (0.0, 10.0)]);
        let mut lib = StyleLibrary::default();
        lib.materials.push(concrete_material());
        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);

        let primitives = derive_floorplan_view(
            &viewpoint,
            &model_view,
            &collector,
            &FloorPlanEngineOptions {
                style_library: Some(lib),
                ..FloorPlanEngineOptions::default()
            },
        )
        .unwrap();
        let fill = primitives
            .iter()
            .find(|p| p.role == PrimitiveRole::CutFill)
            .expect("cut fill");
        assert_eq!(fill.style.hatch_pattern.as_deref(), Some("AR-CONC"));
        assert!((fill.style.hatch_scale.unwrap() - 0.5).abs() < 1e-9);
        assert!((fill.style.hatch_angle_rad.unwrap() - 135f64.to_radians()).abs() < 1e-9);
        assert_eq!(fill.style.layer_name.as_deref(), Some("AEC_WALL_HATCH"));
    }

    #[test]
    fn test_floorplan_structural_only_keeps_only_structural_layer() {
        use crate::modules::aec::engine::plan_view::{LayerDetail, PlanningStage, ViewType};
        let (viewpoint, model_view) = plan_fixture();
        let wall = style_wall(
            vec![
                style_layer("Plaster", 0.02, 0.0, false),
                style_layer("Concrete", 0.20, 0.02, true),
                style_layer("Plaster", 0.02, 0.22, false),
            ],
            [(0.0, 0.0), (10.0, 0.0)],
        );
        let core_component = wall.layers[1].component_id;
        let mut lib = StyleLibrary::default();
        lib.materials.push(concrete_material());
        lib.wall_styles
            .push(crate::modules::aec::engine::views::style_resolver::tests::wall_style_for(&wall));
        let mut dc = DisplayConfig::new(
            "Statik".to_string(),
            "Statik".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        dc.detail.layers = LayerDetail::StructuralOnly;
        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);

        let primitives = derive_floorplan_view(
            &viewpoint,
            &model_view,
            &collector,
            &FloorPlanEngineOptions {
                display_config: Some(dc),
                style_library: Some(lib),
                ..FloorPlanEngineOptions::default()
            },
        )
        .unwrap();
        let fills: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::CutFill).collect();
        assert!(!fills.is_empty());
        for f in &fills {
            assert!(
                f.sources.as_slice().iter().all(|s| s.component_id == Some(core_component)),
                "StructuralOnly must only emit the structural core layer"
            );
        }
        for b in primitives.iter().filter(|p| p.role == PrimitiveRole::CutBoundary) {
            assert!(b.sources.as_slice().iter().all(|s| s.component_id == Some(core_component)));
        }
    }

    #[test]
    fn test_floorplan_demolition_phase_is_dashed_without_display_config() {
        let (viewpoint, model_view) = plan_fixture();
        let mut wall = style_wall(vec![style_layer("Concrete", 0.24, 0.0, true)], [(0.0, 0.0), (10.0, 0.0)]);
        wall.phase = PlanPhase::Demolition;
        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);

        let primitives = derive_floorplan_view(
            &viewpoint,
            &model_view,
            &collector,
            &FloorPlanEngineOptions::default(),
        )
        .unwrap();
        let boundaries: Vec<_> = primitives
            .iter()
            .filter(|p| p.role == PrimitiveRole::CutBoundary)
            .collect();
        assert!(!boundaries.is_empty());
        assert!(boundaries.iter().all(|b| b.style.line_type.as_deref() == Some("DASHED")));
    }

    #[test]
    fn test_floorplan_derivation_is_deterministic_without_options() {
        // No filesystem libraries are consulted: two derivations without options are identical.
        let (viewpoint, model_view) = plan_fixture();
        let wall = style_wall(vec![style_layer("Concrete", 0.24, 0.0, true)], [(0.0, 0.0), (10.0, 0.0)]);
        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);
        let a = derive_floorplan_view(&viewpoint, &model_view, &collector, &FloorPlanEngineOptions::default()).unwrap();
        let b = derive_floorplan_view(&viewpoint, &model_view, &collector, &FloorPlanEngineOptions::default()).unwrap();
        let styles = |v: &Vec<DerivedPrimitive>| v.iter().map(|p| (p.role, p.style.clone())).collect::<Vec<_>>();
        assert_eq!(styles(&a), styles(&b));
    }

    #[test]
    fn test_floorplan_contour_hidden_while_hatch_visible_emits_no_cut_boundary() {
        use crate::modules::aec::engine::display_component::{ComponentRuleSet, WallComponentSlot};
        let (viewpoint, model_view) = plan_fixture();
        let wall = style_wall(vec![style_layer("Concrete", 0.24, 0.0, true)], [(0.0, 0.0), (10.0, 0.0)]);
        let mut lib = StyleLibrary::default();
        lib.materials.push(concrete_material());

        let mut ws = crate::modules::aec::engine::views::style_resolver::tests::wall_style_for(&wall);
        let mut profile = ComponentRuleSet::default();
        profile.visibility.insert(WallComponentSlot::Contour2D.key().to_string(), false);
        profile.visibility.insert(WallComponentSlot::Layers2D.key().to_string(), false);
        profile.visibility.insert(WallComponentSlot::LayerHatch2D.key().to_string(), true);
        ws.default_profile = Some(profile);
        lib.wall_styles.push(ws);

        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);

        let primitives = derive_floorplan_view(
            &viewpoint,
            &model_view,
            &collector,
            &FloorPlanEngineOptions {
                style_library: Some(lib),
                ..FloorPlanEngineOptions::default()
            },
        )
        .unwrap();

        let boundaries: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::CutBoundary).collect();
        assert!(boundaries.is_empty(), "When contour slots are hidden, no CutBoundary primitives must be emitted");
        let fills: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::CutFill).collect();
        assert!(!fills.is_empty(), "CutFill must still be emitted when LayerHatch2D is visible");
    }

    #[test]
    fn test_floorplan_layer_detail_off_occludes_background_across_whole_wall_thickness() {
        use crate::modules::aec::engine::plan_view::{LayerDetail, PlanningStage, ViewType};
        let (viewpoint, model_view) = plan_fixture();

        // 3-layer cut wall: plaster 0.02 | core 0.20 | plaster 0.02. Total thickness = 0.24m.
        // Wall runs along X from x=0.0 to x=4.0, centered at y=0.0 (so y extends from -0.12 to +0.12).
        let wall = style_wall(
            vec![
                style_layer("Plaster", 0.02, 0.0, false),
                style_layer("Concrete", 0.20, 0.02, true),
                style_layer("Plaster", 0.02, 0.22, false),
            ],
            [(0.0, 0.0), (4.0, 0.0)],
        );
        let core_comp = wall.layers[1].component_id;
        let mut lib = StyleLibrary::default();
        lib.materials.push(concrete_material());
        lib.wall_styles
            .push(crate::modules::aec::engine::views::style_resolver::tests::wall_style_for(&wall));

        // Background slab below cut plane (z from -0.2 to 0.0, cut is at z=1.0).
        // It spans x from 1.0 to 3.0, y from -1.0 to 1.0.
        // Its edge runs across the cut wall at y ∈ [-1.0, 1.0]!
        let slab = CollectedSlab {
            handle: acadrust::Handle::from(200u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "Slab_Test".into(),
            base_z: -0.2,
            total_thickness: 0.2,
            polygon: vec![
                Point2::new(1.0, -1.0),
                Point2::new(3.0, -1.0),
                Point2::new(3.0, 1.0),
                Point2::new(1.0, 1.0),
            ],
            layers: vec![CollectedSlabLayer {
                material: "Concrete".into(),
                thickness: 0.2,
                vertical_offset: 0.0,
                function: "Structural".into(),
                hatch_override: None,
                layer_id: uuid::Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            justification: crate::modules::aec::engine::slab::SlabJustification::Top,
            openings: Vec::new(),
            phase: PlanPhase::New,
        };

        let mut dc = DisplayConfig::new(
            "DetailOff".to_string(),
            "Architektur".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        dc.detail.layers = LayerDetail::Off;

        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);
        collector.slabs.push(slab);

        let primitives = derive_floorplan_view(
            &viewpoint,
            &model_view,
            &collector,
            &FloorPlanEngineOptions {
                display_config: Some(dc),
                style_library: Some(lib),
                ..FloorPlanEngineOptions::default()
            },
        )
        .unwrap();

        // 1. Verify that no CutBoundary primitives were emitted for the middle layer
        for p in primitives.iter().filter(|p| p.role == PrimitiveRole::CutBoundary) {
            assert!(
                !p.sources.as_slice().iter().any(|s| s.component_id == Some(core_comp)),
                "Core build-up layer must have no CutBoundary when LayerDetail::Off"
            );
        }

        // 2. Verify that background slab projected edges were clipped across the whole wall thickness
        // (y ∈ [-0.12, 0.12] along x ∈ [1.0, 3.0])
        let projected: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::ProjectedEdge).collect();
        assert!(!projected.is_empty(), "Background slab must produce projected edges");

        for prim in &projected {
            match &prim.geometry {
                Geometry2d::Line { start, end } => {
                    let min_x = start.x_m.min(end.x_m);
                    let max_x = start.x_m.max(end.x_m);
                    let mid_y = (start.y_m + end.y_m) * 0.5;
                    if max_x > 0.05 && min_x < 3.95 {
                        assert!(
                            mid_y < -0.11 || mid_y > 0.11,
                            "Background edge must be occluded across full wall thickness, but found mid_y = {}",
                            mid_y
                        );
                    }
                }
                Geometry2d::Polyline { vertices, .. } => {
                    for win in vertices.windows(2) {
                        let min_x = win[0].x_m.min(win[1].x_m);
                        let max_x = win[0].x_m.max(win[1].x_m);
                        let mid_y = (win[0].y_m + win[1].y_m) * 0.5;
                        if max_x > 0.05 && min_x < 3.95 {
                            assert!(
                                mid_y < -0.11 || mid_y > 0.11,
                                "Background edge must be occluded across full wall thickness, but found mid_y = {}",
                                mid_y
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }

    #[test]
    fn test_localized_floorplan_projection_with_origin_offset() {
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        // Building is located at (10.0, 20.0) in world coordinates.
        // Viewpoint origin is set to (10.0, 20.0, 1.0).
        let viewpoint = Viewpoint {
            id: vp_id,
            name: "Local Ground Floor".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(10.0, 20.0, 1.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.8).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };

        let model_view = ModelView {
            id: mv_id,
            name: "1:50 Local Floor Plan".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let wall = CollectedWall {
            handle: acadrust::Handle::from(101u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "WallStyle_Exterior".into(),
            height: 2.8,
            base_z: 0.0,
            top_z: 2.8,
            // Axis in world coordinates: from (10.0, 20.0) to (20.0, 20.0)
            axis_points: vec![Point2::new(10.0, 20.0), Point2::new(20.0, 20.0)],
            layers: vec![CollectedWallLayer {
                material: "Masonry".into(),
                thickness: 0.30,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: Some("ANSI31".into()),
                layer_id: uuid::Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.30,
            justification: WallJustification::Center,
            openings: vec![
                CollectedOpening {
                    handle: acadrust::Handle::from(102u64),
                    element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
                    xref_source: None,
                    host_wall_handle: acadrust::Handle::from(101u64),
                    kind: OpeningKind::Door,
                    style_id: None,
                    distance_along_axis: 3.0, // center at world x = 13.0, relative x = 3.0
                    width: 1.0,
                    height: 2.1,
                    sill_height: 0.0,
                    spring_height: 0.0,
                    shape: OpeningShape::Rectangle,
                    hinge: HingeSide::Left,
                    swing_side: Default::default(),
                    reference_side: OpeningReferenceSide::Start,
                    cross_axis_offset: 0.0,
                    depth: None,
                    world_center: Point3::new(13.0, 20.0, 1.05),
                    axis_span_local: (2.5, 3.5),
                },
            ],
            phase: PlanPhase::New,
        };

        let slab = CollectedSlab {
            handle: acadrust::Handle::from(201u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "Slab_20".into(),
            base_z: 0.0,
            total_thickness: 0.20,
            polygon: vec![
                Point2::new(10.0, 20.0),
                Point2::new(20.0, 20.0),
                Point2::new(20.0, 28.0),
                Point2::new(10.0, 28.0),
            ],
            layers: Vec::new(),
            justification: crate::modules::aec::engine::slab::SlabJustification::Top,
            openings: Vec::new(),
            phase: PlanPhase::New,
        };

        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);
        collector.slabs.push(slab);

        // Verify bounding_box_2d on collector: wall includes thickness margin
        let bbox = collector.bounding_box_2d().expect("bbox calculated");
        assert!(bbox.0.x_m >= 9.7 && bbox.0.x_m <= 10.0, "bbox.0.x_m was {}", bbox.0.x_m);
        assert!(bbox.0.y_m >= 19.7 && bbox.0.y_m <= 20.0, "bbox.0.y_m was {}", bbox.0.y_m);

        let primitives = derive_floorplan_view(
            &viewpoint,
            &model_view,
            &collector,
            &FloorPlanEngineOptions::default(),
        )
        .expect("floor plan derived successfully");

        // Verify that primitives are transformed relative to origin (10.0, 20.0):
        // Lowest X must be near 0.0, NOT 10.0!
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;

        for p in &primitives {
            match &p.geometry {
                Geometry2d::Line { start, end } => {
                    min_x = min_x.min(start.x_m).min(end.x_m);
                    max_x = max_x.max(start.x_m).max(end.x_m);
                }
                Geometry2d::Polyline { vertices, .. } => {
                    for v in vertices {
                        min_x = min_x.min(v.x_m);
                        max_x = max_x.max(v.x_m);
                    }
                }
                Geometry2d::Region { outer, .. } => {
                    for v in outer {
                        min_x = min_x.min(v.x_m);
                        max_x = max_x.max(v.x_m);
                    }
                }
                Geometry2d::CircularArc { center, .. } => {
                    min_x = min_x.min(center.x_m);
                    max_x = max_x.max(center.x_m);
                }
            }
        }

        assert!(min_x >= -0.2 && min_x <= 0.2, "Minimum relative coordinate must be ~0.0 (got {})", min_x);
        assert!(max_x >= 9.8 && max_x <= 10.2, "Maximum relative coordinate must be ~10.0 (got {})", max_x);

        // Door swing arc center must be relative to origin:
        let door_swing = primitives.iter().find(|p| matches!(p.geometry, Geometry2d::CircularArc { .. })).expect("door swing arc");
        if let Geometry2d::CircularArc { center, radius, .. } = door_swing.geometry {
            assert!((radius.get() - 1.0).abs() < 1e-4);
            // Left jamb was at s = 2.5m along axis from (10, 20), so relative X is 2.5m
            assert!((center.x_m - 2.5).abs() < 1e-3, "Door hinge X must be ~2.5m (got {})", center.x_m);
        }
    }

    #[test]
    fn test_floorplan_opening_lintel_overhead_edges() {
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let viewpoint = Viewpoint {
            id: vp_id,
            name: "Ground Floor".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.8).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(2.8).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };

        let model_view = ModelView {
            id: mv_id,
            name: "1:50 Floor Plan".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let wall = CollectedWall {
            handle: acadrust::Handle::from(100u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "WallStyle".into(),
            height: 2.8,
            base_z: 0.0,
            top_z: 2.8,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(6.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Masonry".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: uuid::Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: vec![CollectedOpening {
                handle: acadrust::Handle::from(101u64),
                element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
                xref_source: None,
                host_wall_handle: acadrust::Handle::from(100u64),
                kind: OpeningKind::Door,
                style_id: None,
                distance_along_axis: 3.0,
                width: 1.0,
                height: 2.1, // lintel head = 2.1 < 2.8
                sill_height: 0.0,
                spring_height: 0.0,
                shape: OpeningShape::Rectangle,
                hinge: HingeSide::Left,
                swing_side: Default::default(),
                reference_side: OpeningReferenceSide::Start,
                cross_axis_offset: 0.0,
                depth: None,
                world_center: Point3::new(3.0, 0.0, 1.05),
                axis_span_local: (2.5, 3.5),
            }],
            phase: PlanPhase::New,
        };

        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);

        let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &FloorPlanEngineOptions::default()).unwrap();
        let lintel_lines: Vec<_> = prims
            .iter()
            .filter(|p| {
                p.role == PrimitiveRole::HiddenEdge
                    && p.style.layer_name.as_deref() == Some("AEC_OVERHEAD")
                    && p.sources.as_slice().iter().any(|s| s.feature == Some(FeatureKey::OpeningHead))
            })
            .collect();

        assert_eq!(lintel_lines.len(), 2, "Expected 2 lintel lines (inner and outer) on AEC_OVERHEAD");
        for l in lintel_lines {
            assert_eq!(l.style.line_type.as_deref(), Some("DASHED"));
        }
    }

    #[test]
    fn test_floorplan_full_height_opening_emits_no_lintel() {
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let viewpoint = Viewpoint {
            id: vp_id,
            name: "Ground Floor".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.8).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(2.8).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };

        let model_view = ModelView {
            id: mv_id,
            name: "1:50 Floor Plan".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        // Full-height opening (height = 2.8, equal to wall top_z)
        let wall = CollectedWall {
            handle: acadrust::Handle::from(100u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "WallStyle".into(),
            height: 2.8,
            base_z: 0.0,
            top_z: 2.8,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(6.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Masonry".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: uuid::Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: vec![CollectedOpening {
                handle: acadrust::Handle::from(101u64),
                element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
                xref_source: None,
                host_wall_handle: acadrust::Handle::from(100u64),
                kind: OpeningKind::Door,
                style_id: None,
                distance_along_axis: 3.0,
                width: 1.0,
                height: 2.8, // Full-height: op_head == 2.8 == wall.top_z
                sill_height: 0.0,
                spring_height: 0.0,
                shape: OpeningShape::Rectangle,
                hinge: HingeSide::Left,
                swing_side: Default::default(),
                reference_side: OpeningReferenceSide::Start,
                cross_axis_offset: 0.0,
                depth: None,
                world_center: Point3::new(3.0, 0.0, 1.4),
                axis_span_local: (2.5, 3.5),
            }],
            phase: PlanPhase::New,
        };

        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);

        let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &FloorPlanEngineOptions::default()).unwrap();
        let lintel_lines: Vec<_> = prims
            .iter()
            .filter(|p| {
                p.role == PrimitiveRole::HiddenEdge
                    && p.sources.as_slice().iter().any(|s| s.feature == Some(FeatureKey::OpeningHead))
            })
            .collect();

        assert_eq!(lintel_lines.len(), 0, "Full-height opening must emit zero lintel lines");
    }

    #[test]
    fn test_floorplan_show_overhead_edges_false_emits_none() {
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let viewpoint = Viewpoint {
            id: vp_id,
            name: "Ground Floor".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.8).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(2.8).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };

        let model_view = ModelView {
            id: mv_id,
            name: "1:50 Floor Plan".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let wall = CollectedWall {
            handle: acadrust::Handle::from(100u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "WallStyle".into(),
            height: 2.8,
            base_z: 0.0,
            top_z: 2.8,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(6.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Masonry".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: uuid::Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: vec![CollectedOpening {
                handle: acadrust::Handle::from(101u64),
                element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
                xref_source: None,
                host_wall_handle: acadrust::Handle::from(100u64),
                kind: OpeningKind::Door,
                style_id: None,
                distance_along_axis: 3.0,
                width: 1.0,
                height: 2.1,
                sill_height: 0.0,
                spring_height: 0.0,
                shape: OpeningShape::Rectangle,
                hinge: HingeSide::Left,
                swing_side: Default::default(),
                reference_side: OpeningReferenceSide::Start,
                cross_axis_offset: 0.0,
                depth: None,
                world_center: Point3::new(3.0, 0.0, 1.05),
                axis_span_local: (2.5, 3.5),
            }],
            phase: PlanPhase::New,
        };

        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);

        let options = FloorPlanEngineOptions {
            show_overhead_edges: false,
            ..Default::default()
        };
        let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &options).unwrap();
        let overhead_lines: Vec<_> = prims
            .iter()
            .filter(|p| p.style.layer_name.as_deref() == Some("AEC_OVERHEAD"))
            .collect();

        assert_eq!(overhead_lines.len(), 0, "show_overhead_edges=false must emit zero overhead lines");
    }

    #[test]
    fn test_floorplan_overhead_opening_and_slab_above_cut() {
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let viewpoint = Viewpoint {
            id: vp_id,
            name: "Ground Floor".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.8).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };

        let model_view = ModelView {
            id: mv_id,
            name: "1:50 Floor Plan".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let op_elem = ElementRef::new(ModelId::new(), ElementId::new());
        // High window fully above cut plane (sill = 1.5 > z_cut = 1.0)
        let wall = CollectedWall {
            handle: acadrust::Handle::from(100u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "WallStyle".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(6.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Masonry".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: uuid::Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: vec![CollectedOpening {
                handle: acadrust::Handle::from(101u64),
                element_ref: op_elem,
                xref_source: None,
                host_wall_handle: acadrust::Handle::from(100u64),
                kind: OpeningKind::Window,
                style_id: None,
                distance_along_axis: 3.0,
                width: 1.0,
                height: 0.8,
                sill_height: 1.5, // > z_cut (1.0)
                spring_height: 0.0,
                shape: OpeningShape::Rectangle,
                hinge: HingeSide::Left,
                swing_side: Default::default(),
                reference_side: OpeningReferenceSide::Start,
                cross_axis_offset: 0.0,
                depth: None,
                world_center: Point3::new(3.0, 0.0, 1.9),
                axis_span_local: (2.5, 3.5),
            }],
            phase: PlanPhase::New,
        };

        // Overhead drop ceiling / slab strictly above cut plane (z = 2.4 > z_cut = 1.0)
        let slab_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let slab = CollectedSlab {
            handle: acadrust::Handle::from(200u64),
            element_ref: slab_elem,
            xref_source: None,
            style_id: "Ceiling".into(),
            base_z: 2.4,
            total_thickness: 0.20,
            polygon: vec![
                Point2::new(1.0, 1.0),
                Point2::new(5.0, 1.0),
                Point2::new(5.0, 3.0),
                Point2::new(1.0, 3.0),
            ],
            layers: Vec::new(),
            justification: crate::modules::aec::engine::slab::SlabJustification::Bottom,
            openings: Vec::new(),
            phase: PlanPhase::New,
        };

        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);
        collector.slabs.push(slab);

        let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &FloorPlanEngineOptions::default()).unwrap();

        // 1. Verify overhead high window jamb lines
        let op_overhead: Vec<_> = prims
            .iter()
            .filter(|p| {
                p.role == PrimitiveRole::HiddenEdge
                    && p.style.layer_name.as_deref() == Some("AEC_OVERHEAD")
                    && p.sources.as_slice().iter().any(|s| s.element == op_elem)
            })
            .collect();
        assert_eq!(op_overhead.len(), 2, "Expected 2 overhead opening jamb lines crossing the wall");
        for p in op_overhead {
            assert_eq!(p.style.line_type.as_deref(), Some("DASHED"));
        }

        // 2. Verify overhead slab boundary
        let slab_overhead: Vec<_> = prims
            .iter()
            .filter(|p| {
                p.role == PrimitiveRole::HiddenEdge
                    && p.style.layer_name.as_deref() == Some("AEC_OVERHEAD")
                    && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
            })
            .collect();
        assert!(!slab_overhead.is_empty(), "Expected overhead slab boundary to be emitted");
        for s in slab_overhead {
            assert_eq!(s.style.line_type.as_deref(), Some("DASHED"));
        }
    }

    #[test]
    fn test_floorplan_slab_under_cut_wall_occlusion_and_hidden_lines() {
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let viewpoint = Viewpoint {
            id: vp_id,
            name: "Ground Floor".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.8).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };

        let model_view = ModelView {
            id: mv_id,
            name: "1:50 Floor Plan".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        // Cut wall along y = 0 from x = 0 to 4, thickness 0.24 (y in [-0.12, 0.12])
        let wall = CollectedWall {
            handle: acadrust::Handle::from(100u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "WallStyle".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: uuid::Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: Vec::new(),
            phase: PlanPhase::New,
        };

        // Slab below cut plane (base_z = -0.2, top_z = 0.0, thickness = 0.2)
        // Polygon: x in [1, 3], y in [-1, 1].
        // The slab crosses directly through the wall (y in [-0.12, 0.12]).
        // Edges at x = 1 and x = 3 cross from y = -1 to y = 1 across the wall!
        let slab_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let slab = CollectedSlab {
            handle: acadrust::Handle::from(200u64),
            element_ref: slab_elem,
            xref_source: None,
            style_id: "FloorSlab".into(),
            base_z: 0.0,
            total_thickness: 0.20,
            polygon: vec![
                Point2::new(1.0, -1.0),
                Point2::new(3.0, -1.0),
                Point2::new(3.0, 1.0),
                Point2::new(1.0, 1.0),
            ],
            layers: Vec::new(),
            justification: crate::modules::aec::engine::slab::SlabJustification::Top,
            openings: Vec::new(),
            phase: PlanPhase::New,
        };

        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);
        collector.slabs.push(slab);

        // Case A: emit_hidden_lines = false
        let opts_off = FloorPlanEngineOptions {
            emit_hidden_lines: false,
            ..Default::default()
        };
        let prims_off = derive_floorplan_view(&viewpoint, &model_view, &collector, &opts_off).unwrap();
        let hidden_off: Vec<_> = prims_off.iter().filter(|p| p.role == PrimitiveRole::HiddenEdge).collect();
        assert_eq!(hidden_off.len(), 0, "No hidden lines when emit_hidden_lines is false");

        // The slab contour segments across the cut wall should be clipped out of visible projected edges!
        let slab_vis_off: Vec<_> = prims_off
            .iter()
            .filter(|p| p.role == PrimitiveRole::ProjectedEdge && p.sources.as_slice().iter().any(|s| s.element == slab_elem))
            .collect();
        // Since the 2 vertical edges [1, -1]..[1, 1] and [3, -1]..[3, 1] cross through the wall [-0.12, 0.12],
        // they are split into 2 visible segments each (below wall and above wall), plus 2 horizontal edges = 6 visible segments.
        assert_eq!(slab_vis_off.len(), 6, "Expected slab contour to be clipped by cut wall into 6 visible pieces");

        // Case B: emit_hidden_lines = true
        let opts_on = FloorPlanEngineOptions {
            emit_hidden_lines: true,
            ..Default::default()
        };
        let prims_on = derive_floorplan_view(&viewpoint, &model_view, &collector, &opts_on).unwrap();
        let hidden_on: Vec<_> = prims_on
            .iter()
            .filter(|p| {
                p.role == PrimitiveRole::HiddenEdge
                    && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
            })
            .collect();
        assert_eq!(hidden_on.len(), 2, "Expected 2 occluded slab contour segments under the cut wall to be emitted dashed");
        for h in hidden_on {
            assert_eq!(h.style.line_type.as_deref(), Some("DASHED"));
            assert_eq!(h.style.layer_name.as_deref(), Some("AEC_HIDDEN"));
        }
    }

    #[test]
    fn test_floorplan_overhead_slab_coinciding_with_cut_wall_suppressed() {
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let viewpoint = Viewpoint {
            id: vp_id,
            name: "Ground Floor".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.8).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };

        let model_view = ModelView {
            id: mv_id,
            name: "1:50 Floor Plan".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        // Cut wall along y = 0 from x = 0 to 4, thickness 0.24 (y in [-0.12, 0.12])
        let wall = CollectedWall {
            handle: acadrust::Handle::from(100u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "WallStyle".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: uuid::Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: Vec::new(),
            phase: PlanPhase::New,
        };

        // Overhead ceiling slab at z = 2.5 (above cut plane z = 1.0)
        // Polygon: [0.0, 0.12] -> [4.0, 0.12] -> [4.0, 2.0] -> [0.0, 2.0]
        // Edge 1: [0, 0.12] -> [4, 0.12] coincides exactly with the exterior face of the cut wall!
        // It must be suppressed so dashed overhead lines do NOT draw over the solid wall cut boundary!
        let slab_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let slab = CollectedSlab {
            handle: acadrust::Handle::from(200u64),
            element_ref: slab_elem,
            xref_source: None,
            style_id: "Ceiling".into(),
            base_z: 2.5,
            total_thickness: 0.20,
            polygon: vec![
                Point2::new(0.0, 0.12),
                Point2::new(4.0, 0.12),
                Point2::new(4.0, 2.0),
                Point2::new(0.0, 2.0),
            ],
            layers: Vec::new(),
            justification: crate::modules::aec::engine::slab::SlabJustification::Bottom,
            openings: Vec::new(),
            phase: PlanPhase::New,
        };

        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);
        collector.slabs.push(slab);

        let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &FloorPlanEngineOptions::default()).unwrap();

        let overhead_slab: Vec<_> = prims
            .iter()
            .filter(|p| {
                p.role == PrimitiveRole::HiddenEdge
                    && p.style.layer_name.as_deref() == Some("AEC_OVERHEAD")
                    && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
            })
            .collect();

        // Edge [0, 0.12] -> [4, 0.12] is suppressed!
        // Edges [4, 0.12]->[4, 2], [4, 2]->[0, 2], [0, 2]->[0, 0.12] are kept (3 edges).
        assert_eq!(overhead_slab.len(), 3, "Coinciding overhead slab edge must be suppressed, remaining 3 edges kept");
        for s in overhead_slab {
            if let Geometry2d::Line { start, end } = s.geometry {
                // Ensure none of the kept lines are the edge along y = 0.12 from 0 to 4
                let is_coinciding = (start.y_m - 0.12).abs() < 1e-4 && (end.y_m - 0.12).abs() < 1e-4;
                assert!(!is_coinciding, "Coinciding edge along y = 0.12 must not be present in overhead lines");
            }
        }
    }

    #[test]
    fn test_cut_opening_lintel_suppressed_when_above_z_max() {
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        // Viewpoint with z_range = 0.0 .. 2.0 (z_max = 2.0)
        let viewpoint_low = Viewpoint {
            id: vp_id,
            name: "Ground Floor".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.0).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(2.0).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };

        // Viewpoint with z_range = 0.0 .. 3.0 (z_max = 3.0)
        let viewpoint_high = Viewpoint {
            id: vp_id,
            name: "Ground Floor".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(2.0).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };

        let model_view = ModelView {
            id: mv_id,
            name: "1:50 Floor Plan".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        // Door with sill = 0.0, height = 2.1 => op_head = 2.1
        let op_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let wall = CollectedWall {
            handle: acadrust::Handle::from(100u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "WallStyle".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: uuid::Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: vec![CollectedOpening {
                handle: acadrust::Handle::from(101u64),
                element_ref: op_elem,
                xref_source: None,
                host_wall_handle: acadrust::Handle::from(100u64),
                kind: OpeningKind::Door,
                style_id: None,
                distance_along_axis: 2.0,
                width: 1.0,
                height: 2.1,
                sill_height: 0.0,
                spring_height: 0.0,
                shape: OpeningShape::Rectangle,
                hinge: HingeSide::Left,
                swing_side: Default::default(),
                reference_side: OpeningReferenceSide::Start,
                cross_axis_offset: 0.0,
                depth: None,
                world_center: Point3::new(2.0, 0.0, 1.05),
                axis_span_local: (1.5, 2.5),
            }],
            phase: PlanPhase::New,
        };

        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);

        // Low view range: z_max = 2.0 < op_head = 2.1 => lintel must NOT be emitted
        let prims_low = derive_floorplan_view(&viewpoint_low, &model_view, &collector, &FloorPlanEngineOptions::default()).unwrap();
        let lintel_low: Vec<_> = prims_low
            .iter()
            .filter(|p| {
                p.role == PrimitiveRole::HiddenEdge
                    && p.sources.as_slice().iter().any(|s| s.feature == Some(FeatureKey::OpeningHead))
            })
            .collect();
        assert_eq!(lintel_low.len(), 0, "Door head above z_max must not emit lintel lines");

        // High view range: z_max = 3.0 > op_head = 2.1 => lintel lines MUST be emitted
        let prims_high = derive_floorplan_view(&viewpoint_high, &model_view, &collector, &FloorPlanEngineOptions::default()).unwrap();
        let lintel_high: Vec<_> = prims_high
            .iter()
            .filter(|p| {
                p.role == PrimitiveRole::HiddenEdge
                    && p.sources.as_slice().iter().any(|s| s.feature == Some(FeatureKey::OpeningHead))
            })
            .collect();
        assert_eq!(lintel_high.len(), 2, "Door head within z_max must emit 2 lintel lines");
    }

    #[test]
    fn test_floorplan_window_parapet_occludes_slab_and_preserves_door_transparency() {
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let viewpoint = Viewpoint {
            id: vp_id,
            name: "Ground Floor".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.8).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };

        let model_view = ModelView {
            id: mv_id,
            name: "1:50 Floor Plan".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        // Wall from x=0 to x=10 along y=0, thickness 0.24 (y in [-0.12, 0.12]), base_z=0.0, top_z=3.0
        let win_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let door_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let wall = CollectedWall {
            handle: acadrust::Handle::from(100u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "WallStyle".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: uuid::Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: vec![
                // Window at x=2.5, width=1.0 (span 2.0..3.0), sill=0.8, height=1.3
                CollectedOpening {
                    handle: acadrust::Handle::from(101u64),
                    element_ref: win_elem,
                    xref_source: None,
                    host_wall_handle: acadrust::Handle::from(100u64),
                    kind: OpeningKind::Window,
                    style_id: None,
                    distance_along_axis: 2.5,
                    width: 1.0,
                    height: 1.3,
                    sill_height: 0.8,
                    spring_height: 0.0,
                    shape: OpeningShape::Rectangle,
                    hinge: HingeSide::Left,
                    swing_side: Default::default(),
                    reference_side: OpeningReferenceSide::Start,
                    cross_axis_offset: 0.0,
                    depth: None,
                    world_center: Point3::new(2.5, 0.0, 1.45),
                    axis_span_local: (2.0, 3.0),
                },
                // Door at x=7.5, width=1.0 (span 7.0..8.0), sill=0.0, height=2.1
                CollectedOpening {
                    handle: acadrust::Handle::from(102u64),
                    element_ref: door_elem,
                    xref_source: None,
                    host_wall_handle: acadrust::Handle::from(100u64),
                    kind: OpeningKind::Door,
                    style_id: None,
                    distance_along_axis: 7.5,
                    width: 1.0,
                    height: 2.1,
                    sill_height: 0.0,
                    spring_height: 0.0,
                    shape: OpeningShape::Rectangle,
                    hinge: HingeSide::Left,
                    swing_side: Default::default(),
                    reference_side: OpeningReferenceSide::Start,
                    cross_axis_offset: 0.0,
                    depth: None,
                    world_center: Point3::new(7.5, 0.0, 1.05),
                    axis_span_local: (7.0, 8.0),
                },
            ],
            phase: PlanPhase::New,
        };

        // Below-cut slab from x=1.0 to x=9.0, running from y=-1.0 to y=0.0 (top edge lies along y=0.0 directly on wall axis)
        let slab_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let slab = CollectedSlab {
            handle: acadrust::Handle::from(200u64),
            element_ref: slab_elem,
            xref_source: None,
            style_id: "FloorSlab".into(),
            base_z: -0.20,
            total_thickness: 0.20, // top_z = 0.0
            polygon: vec![
                Point2::new(1.0, -1.0),
                Point2::new(9.0, -1.0),
                Point2::new(9.0, 0.0),
                Point2::new(1.0, 0.0),
            ],
            layers: Vec::new(),
            justification: crate::modules::aec::engine::slab::SlabJustification::Top,
            openings: Vec::new(),
            phase: PlanPhase::New,
        };

        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);
        collector.slabs.push(slab);

        // Case A: emit_hidden_lines = false
        // The slab edge along y=0.0 must be occluded by wall (1..2, 3..7, 8..9) AND by window parapet (2..3).
        // Only the piece inside the door opening (7.0..8.0) must remain visible!
        let opts_off = FloorPlanEngineOptions {
            emit_hidden_lines: false,
            ..Default::default()
        };
        let prims_off = derive_floorplan_view(&viewpoint, &model_view, &collector, &opts_off).unwrap();
        let visible_slab_edges: Vec<_> = prims_off
            .iter()
            .filter(|p| {
                p.role == PrimitiveRole::ProjectedEdge
                    && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
            })
            .collect();

        // Check if there is any visible slab edge along y=0.0 in the window gap (x in [2.0, 3.0])
        let win_slab_edge = visible_slab_edges.iter().find(|p| match &p.geometry {
            Geometry2d::Line { start, end } => {
                let mid_x = (start.x_m + end.x_m) * 0.5;
                let mid_y = (start.y_m + end.y_m) * 0.5;
                mid_x >= 2.0 && mid_x <= 3.0 && mid_y.abs() < 1e-4
            }
            _ => false,
        });
        assert!(win_slab_edge.is_none(), "Slab edge inside window parapet gap must be occluded");

        // Check that the slab edge inside the door gap (x in [7.0, 8.0]) is visible!
        let door_slab_edge = visible_slab_edges.iter().find(|p| match &p.geometry {
            Geometry2d::Line { start, end } => {
                let min_x = start.x_m.min(end.x_m);
                let max_x = start.x_m.max(end.x_m);
                let mid_y = (start.y_m + end.y_m) * 0.5;
                (min_x - 7.0).abs() < 1e-4 && (max_x - 8.0).abs() < 1e-4 && mid_y.abs() < 1e-4
            }
            _ => false,
        });
        assert!(door_slab_edge.is_some(), "Slab edge inside door opening must remain transparent and visible");

        // Case B: emit_hidden_lines = true
        // The slab edge under the wall and window parapet (x in [1.0, 7.0]) becomes a dashed line on AEC_HIDDEN!
        let opts_on = FloorPlanEngineOptions {
            emit_hidden_lines: true,
            ..Default::default()
        };
        let prims_on = derive_floorplan_view(&viewpoint, &model_view, &collector, &opts_on).unwrap();
        let hidden_slab_edges: Vec<_> = prims_on
            .iter()
            .filter(|p| {
                p.role == PrimitiveRole::HiddenEdge
                    && p.style.layer_name.as_deref() == Some("AEC_HIDDEN")
                    && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
            })
            .collect();

        // The occluded portion across [1.0, 7.0] is emitted as hidden dashed line
        let hidden_across_win = hidden_slab_edges.iter().find(|p| match &p.geometry {
            Geometry2d::Line { start, end } => {
                let min_x = start.x_m.min(end.x_m);
                let max_x = start.x_m.max(end.x_m);
                let mid_y = (start.y_m + end.y_m) * 0.5;
                min_x <= 2.0 + 1e-4 && max_x >= 3.0 - 1e-4 && mid_y.abs() < 1e-4
            }
            _ => false,
        });
        assert!(hidden_across_win.is_some(), "Slab edge across wall and window parapet must be emitted dashed");

        // Inside the door opening (x in [7.0, 8.0]), it stays visible solid
        let visible_door_edge_on = prims_on.iter().find(|p| {
            p.role == PrimitiveRole::ProjectedEdge
                && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
                && match &p.geometry {
                    Geometry2d::Line { start, end } => {
                        let min_x = start.x_m.min(end.x_m);
                        let max_x = start.x_m.max(end.x_m);
                        let mid_y = (start.y_m + end.y_m) * 0.5;
                        (min_x - 7.0).abs() < 1e-4 && (max_x - 8.0).abs() < 1e-4 && mid_y.abs() < 1e-4
                    }
                    _ => false,
                }
        });
        assert!(visible_door_edge_on.is_some(), "Slab edge in door opening must stay visible solid when hidden lines on");

        // Case C: include_openings = false (with emit_hidden_lines = false)
        // Even when opening symbols are not rendered, parapet geometry occludes below-cut slab edge!
        let opts_no_openings = FloorPlanEngineOptions {
            include_openings: false,
            emit_hidden_lines: false,
            ..Default::default()
        };
        let prims_no_openings = derive_floorplan_view(&viewpoint, &model_view, &collector, &opts_no_openings).unwrap();
        let visible_slab_edges_no_op: Vec<_> = prims_no_openings
            .iter()
            .filter(|p| {
                p.role == PrimitiveRole::ProjectedEdge
                    && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
            })
            .collect();

        // Parapet still occludes slab in window gap (x in [2.0, 3.0])
        let win_slab_edge_no_op = visible_slab_edges_no_op.iter().find(|p| match &p.geometry {
            Geometry2d::Line { start, end } => {
                let mid_x = (start.x_m + end.x_m) * 0.5;
                let mid_y = (start.y_m + end.y_m) * 0.5;
                mid_x >= 2.0 && mid_x <= 3.0 && mid_y.abs() < 1e-4
            }
            _ => false,
        });
        assert!(
            win_slab_edge_no_op.is_none(),
            "Slab edge inside window gap must be occluded even when include_openings is false"
        );

        // Door gap (x in [7.0, 8.0]) remains transparent!
        let door_slab_edge_no_op = visible_slab_edges_no_op.iter().find(|p| match &p.geometry {
            Geometry2d::Line { start, end } => {
                let min_x = start.x_m.min(end.x_m);
                let max_x = start.x_m.max(end.x_m);
                let mid_y = (start.y_m + end.y_m) * 0.5;
                (min_x - 7.0).abs() < 1e-4 && (max_x - 8.0).abs() < 1e-4 && mid_y.abs() < 1e-4
            }
            _ => false,
        });
        assert!(
            door_slab_edge_no_op.is_some(),
            "Slab edge inside door gap must remain visible when include_openings is false"
        );
    }

    #[test]
    fn test_floorplan_hidden_wall_does_not_emit_openings_or_occlude() {
        use crate::modules::aec::engine::views::model_view::{
            GraphicOverride, GraphicPatch, OverrideSelector,
        };
        use crate::modules::aec::engine::views::OverrideId;

        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let viewpoint = Viewpoint {
            id: vp_id,
            name: "Ground Floor".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.8).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };

        // Wall from x=0 to x=10 along y=0, thickness 0.24 (y in [-0.12, 0.12]), base_z=0.0, top_z=3.0
        let win_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let door_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let wall_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let wall = CollectedWall {
            handle: acadrust::Handle::from(100u64),
            element_ref: wall_elem,
            xref_source: None,
            style_id: "WallStyle".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: uuid::Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: vec![
                CollectedOpening {
                    handle: acadrust::Handle::from(101u64),
                    element_ref: win_elem,
                    xref_source: None,
                    host_wall_handle: acadrust::Handle::from(100u64),
                    kind: OpeningKind::Window,
                    style_id: None,
                    distance_along_axis: 2.5,
                    width: 1.0,
                    height: 1.3,
                    sill_height: 0.8,
                    spring_height: 0.0,
                    shape: OpeningShape::Rectangle,
                    hinge: HingeSide::Left,
                    swing_side: Default::default(),
                    reference_side: OpeningReferenceSide::Start,
                    cross_axis_offset: 0.0,
                    depth: None,
                    world_center: Point3::new(2.5, 0.0, 1.45),
                    axis_span_local: (2.0, 3.0),
                },
                CollectedOpening {
                    handle: acadrust::Handle::from(102u64),
                    element_ref: door_elem,
                    xref_source: None,
                    host_wall_handle: acadrust::Handle::from(100u64),
                    kind: OpeningKind::Door,
                    style_id: None,
                    distance_along_axis: 7.5,
                    width: 1.0,
                    height: 2.1,
                    sill_height: 0.0,
                    spring_height: 0.0,
                    shape: OpeningShape::Rectangle,
                    hinge: HingeSide::Left,
                    swing_side: Default::default(),
                    reference_side: OpeningReferenceSide::Start,
                    cross_axis_offset: 0.0,
                    depth: None,
                    world_center: Point3::new(7.5, 0.0, 1.05),
                    axis_span_local: (7.0, 8.0),
                },
            ],
            phase: PlanPhase::New,
        };

        // Below-cut slab from x=1.0 to x=9.0, running from y=-1.0 to y=0.0 (top edge lies along y=0.0 directly on wall axis)
        let slab_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let slab = CollectedSlab {
            handle: acadrust::Handle::from(200u64),
            element_ref: slab_elem,
            xref_source: None,
            style_id: "SlabStyle".into(),
            polygon: vec![
                Point2::new(1.0, -1.0),
                Point2::new(9.0, -1.0),
                Point2::new(9.0, 0.0),
                Point2::new(1.0, 0.0),
            ],
            openings: vec![],
            base_z: -0.2,
            total_thickness: 0.20,
            layers: Vec::new(),
            justification: crate::modules::aec::engine::slab::SlabJustification::Top,
            phase: PlanPhase::New,
        };

        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);
        collector.slabs.push(slab);

        // Hide the host wall via ModelView GraphicOverride (draw = false)
        let model_view = ModelView {
            id: mv_id,
            name: "1:50 Floor Plan".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: vec![GraphicOverride {
                id: OverrideId::new(),
                priority: 1,
                enabled: true,
                selector: OverrideSelector::Element(wall_elem),
                patch: GraphicPatch {
                    draw: Some(false),
                    ..Default::default()
                },
            }],
            annotation_ids: Vec::new(),
            update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &FloorPlanEngineOptions::default()).unwrap();

        // 1. No primitives from the hidden wall or its openings must be emitted
        let wall_and_opening_prims: Vec<_> = prims
            .iter()
            .filter(|p| {
                p.sources.as_slice().iter().any(|s| {
                    s.element == wall_elem || s.element == win_elem || s.element == door_elem
                })
            })
            .collect();
        assert!(
            wall_and_opening_prims.is_empty(),
            "Hidden wall and its openings must emit zero primitives, but emitted {}",
            wall_and_opening_prims.len()
        );

        // 2. The slab edge along y=0.0 across x ∈ [1.0, 9.0] must NOT be occluded by the hidden wall or its parapet!
        let slab_edges: Vec<_> = prims
            .iter()
            .filter(|p| {
                p.role == PrimitiveRole::ProjectedEdge
                    && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
            })
            .collect();

        let top_edge_full = slab_edges.iter().find(|p| match &p.geometry {
            Geometry2d::Line { start, end } => {
                let min_x = start.x_m.min(end.x_m);
                let max_x = start.x_m.max(end.x_m);
                let mid_y = (start.y_m + end.y_m) * 0.5;
                (min_x - 1.0).abs() < 1e-3 && (max_x - 9.0).abs() < 1e-3 && mid_y.abs() < 1e-3
            }
            Geometry2d::Polyline { vertices, closed } => {
                let n = vertices.len();
                let edge_count = if *closed { n } else { n.saturating_sub(1) };
                (0..edge_count).any(|i| {
                    let start = vertices[i];
                    let end = vertices[(i + 1) % n];
                    let min_x = start.x_m.min(end.x_m);
                    let max_x = start.x_m.max(end.x_m);
                    let mid_y = (start.y_m + end.y_m) * 0.5;
                    (min_x - 1.0).abs() < 1e-3 && (max_x - 9.0).abs() < 1e-3 && mid_y.abs() < 1e-3
                })
            }
            _ => false,
        });
        assert!(
            top_edge_full.is_some(),
            "Slab edge along y=0.0 must span the full interval [1.0, 9.0] unoccluded"
        );

        let slab_hidden_edges: Vec<_> = prims
            .iter()
            .filter(|p| {
                p.role == PrimitiveRole::HiddenEdge
                    && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
            })
            .collect();
        assert!(
            slab_hidden_edges.is_empty(),
            "Hidden wall must not cause slab edges to become hidden lines, but found {}",
            slab_hidden_edges.len()
        );
    }

    #[test]
    fn test_floorplan_door_reaching_overhead_slab_underside_emits_no_lintel() {
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let viewpoint = Viewpoint {
            id: vp_id,
            name: "Ground Floor".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.8).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };

        let model_view = ModelView {
            id: mv_id,
            name: "1:50 Floor Plan".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        // Wall from x=0 to x=5 along y=0, base_z=0.0, top_z=3.0
        // Overhead ceiling slab at base_z=2.5, thickness=0.2 (underside at 2.5) covering x in [0, 5], y in [-1, 1]
        let door_full = ElementRef::new(ModelId::new(), ElementId::new());
        let door_std = ElementRef::new(ModelId::new(), ElementId::new());
        let wall = CollectedWall {
            handle: acadrust::Handle::from(100u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "WallStyle".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(5.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: uuid::Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: vec![
                // Door 1: height=2.5 (reaches slab underside at 2.5 => full-height / headless)
                CollectedOpening {
                    handle: acadrust::Handle::from(101u64),
                    element_ref: door_full,
                    xref_source: None,
                    host_wall_handle: acadrust::Handle::from(100u64),
                    kind: OpeningKind::Door,
                    style_id: None,
                    distance_along_axis: 1.5,
                    width: 1.0,
                    height: 2.5,
                    sill_height: 0.0,
                    spring_height: 0.0,
                    shape: OpeningShape::Rectangle,
                    hinge: HingeSide::Left,
                    swing_side: Default::default(),
                    reference_side: OpeningReferenceSide::Start,
                    cross_axis_offset: 0.0,
                    depth: None,
                    world_center: Point3::new(1.5, 0.0, 1.25),
                    axis_span_local: (1.0, 2.0),
                },
                // Door 2: height=2.1 (below slab underside 2.5 => has lintel)
                CollectedOpening {
                    handle: acadrust::Handle::from(102u64),
                    element_ref: door_std,
                    xref_source: None,
                    host_wall_handle: acadrust::Handle::from(100u64),
                    kind: OpeningKind::Door,
                    style_id: None,
                    distance_along_axis: 3.5,
                    width: 1.0,
                    height: 2.1,
                    sill_height: 0.0,
                    spring_height: 0.0,
                    shape: OpeningShape::Rectangle,
                    hinge: HingeSide::Left,
                    swing_side: Default::default(),
                    reference_side: OpeningReferenceSide::Start,
                    cross_axis_offset: 0.0,
                    depth: None,
                    world_center: Point3::new(3.5, 0.0, 1.05),
                    axis_span_local: (3.0, 4.0),
                },
            ],
            phase: PlanPhase::New,
        };

        let slab = CollectedSlab {
            handle: acadrust::Handle::from(200u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "CeilingSlab".into(),
            base_z: 2.5,
            total_thickness: 0.20,
            polygon: vec![
                Point2::new(0.0, -1.0),
                Point2::new(5.0, -1.0),
                Point2::new(5.0, 1.0),
                Point2::new(0.0, 1.0),
            ],
            layers: Vec::new(),
            justification: crate::modules::aec::engine::slab::SlabJustification::Top,
            openings: Vec::new(),
            phase: PlanPhase::New,
        };

        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);
        collector.slabs.push(slab);

        let opts = FloorPlanEngineOptions {
            show_overhead_edges: true,
            ..Default::default()
        };
        let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &opts).unwrap();

        // Full-height door (reaches slab underside 2.5) must NOT have OpeningHead lintel lines
        let full_lintels: Vec<_> = prims
            .iter()
            .filter(|p| {
                p.role == PrimitiveRole::HiddenEdge
                    && p.sources.as_slice().iter().any(|s| s.element == door_full && s.feature == Some(FeatureKey::OpeningHead))
            })
            .collect();
        assert_eq!(full_lintels.len(), 0, "Door reaching ceiling slab underside must not emit lintel lines");

        // Standard door (height 2.1 < 2.5) MUST have OpeningHead lintel lines
        let std_lintels: Vec<_> = prims
            .iter()
            .filter(|p| {
                p.role == PrimitiveRole::HiddenEdge
                    && p.sources.as_slice().iter().any(|s| s.element == door_std && s.feature == Some(FeatureKey::OpeningHead))
            })
            .collect();
        assert_eq!(std_lintels.len(), 2, "Standard door below ceiling slab underside must emit lintel lines");
    }

    #[test]
    fn test_floorplan_window_sill_at_cut_plane_emits_sill_lines() {
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let viewpoint = Viewpoint {
            id: vp_id,
            name: "Ground Floor".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.8).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(), // z_cut = 1.0
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };

        let model_view = ModelView {
            id: mv_id,
            name: "1:50 Floor Plan".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let win_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let wall = CollectedWall {
            handle: acadrust::Handle::from(100u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "WallStyle".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(5.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: uuid::Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: vec![
                // Window with sill_height = 1.0 (exactly at cut plane z_cut = 1.0)
                CollectedOpening {
                    handle: acadrust::Handle::from(101u64),
                    element_ref: win_elem,
                    xref_source: None,
                    host_wall_handle: acadrust::Handle::from(100u64),
                    kind: OpeningKind::Window,
                    style_id: None,
                    distance_along_axis: 2.5,
                    width: 1.2,
                    height: 1.2,
                    sill_height: 1.0,
                    spring_height: 0.0,
                    shape: OpeningShape::Rectangle,
                    hinge: HingeSide::Left,
                    swing_side: Default::default(),
                    reference_side: OpeningReferenceSide::Start,
                    cross_axis_offset: 0.0,
                    depth: None,
                    world_center: Point3::new(2.5, 0.0, 1.6),
                    axis_span_local: (1.9, 3.1),
                },
            ],
            phase: PlanPhase::New,
        };

        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);

        let opts = FloorPlanEngineOptions {
            show_window_sills: true,
            include_openings: true,
            ..Default::default()
        };
        let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &opts).unwrap();

        let sill_lines: Vec<_> = prims
            .iter()
            .filter(|p| {
                p.role == PrimitiveRole::ProjectedEdge
                    && p.style.layer_name.as_deref() == Some("AEC_OPENING")
                    && p.sources.as_slice().iter().any(|s| s.element == win_elem)
            })
            .collect();

        assert_eq!(
            sill_lines.len(),
            2,
            "Window whose sill equals cut elevation must retain its 2 sill lines"
        );
    }

    #[test]
    fn test_floorplan_nearby_slab_not_covering_door_does_not_suppress_lintel() {
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let viewpoint = Viewpoint {
            id: vp_id,
            name: "Ground Floor".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.8).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(), // z_cut = 1.0
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };

        let model_view = ModelView {
            id: mv_id,
            name: "1:50 Floor Plan".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let door_elem = ElementRef::new(ModelId::new(), ElementId::new());
        // Long stairwell wall from x=0 to x=6
        let wall = CollectedWall {
            handle: acadrust::Handle::from(100u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "WallStyle".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(6.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: uuid::Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: vec![
                // Door to hallway at x=4.5 (span 4.0..5.0), height=2.1, sill=0.0
                CollectedOpening {
                    handle: acadrust::Handle::from(101u64),
                    element_ref: door_elem,
                    xref_source: None,
                    host_wall_handle: acadrust::Handle::from(100u64),
                    kind: OpeningKind::Door,
                    style_id: None,
                    distance_along_axis: 4.5,
                    width: 1.0,
                    height: 2.1,
                    sill_height: 0.0,
                    spring_height: 0.0,
                    shape: OpeningShape::Rectangle,
                    hinge: HingeSide::Left,
                    swing_side: Default::default(),
                    reference_side: OpeningReferenceSide::Start,
                    cross_axis_offset: 0.0,
                    depth: None,
                    world_center: Point3::new(4.5, 0.0, 1.05),
                    axis_span_local: (4.0, 5.0),
                },
            ],
            phase: PlanPhase::New,
        };

        // Lower stair landing (Zwischenpodest) at base_z=1.3, polygon at x in [0, 2], y in [0.12, 2.0]
        // Its AABB overlaps the wall's AABB [0, 6] x [-0.12, 0.12], but it does NOT cover the door at x=4.5!
        let stair_landing = CollectedSlab {
            handle: acadrust::Handle::from(200u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "LandingSlab".into(),
            base_z: 1.3,
            total_thickness: 0.20,
            polygon: vec![
                Point2::new(0.0, 0.12),
                Point2::new(2.0, 0.12),
                Point2::new(2.0, 2.0),
                Point2::new(0.0, 2.0),
            ],
            layers: Vec::new(),
            justification: crate::modules::aec::engine::slab::SlabJustification::Top,
            openings: Vec::new(),
            phase: PlanPhase::New,
        };

        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);
        collector.slabs.push(stair_landing);

        let opts = FloorPlanEngineOptions {
            show_overhead_edges: true,
            ..Default::default()
        };
        let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &opts).unwrap();

        let lintels: Vec<_> = prims
            .iter()
            .filter(|p| {
                p.role == PrimitiveRole::HiddenEdge
                    && p.sources.as_slice().iter().any(|s| s.element == door_elem && s.feature == Some(FeatureKey::OpeningHead))
            })
            .collect();

        assert_eq!(
            lintels.len(),
            2,
            "Lower nearby slab that does not cover the door must NOT suppress the door's lintel"
        );
    }

    #[test]
    fn test_floorplan_breakthrough_with_sill_occludes_slab() {
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let viewpoint = Viewpoint {
            id: vp_id,
            name: "Ground Floor".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.8).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };

        let model_view = ModelView {
            id: mv_id,
            name: "1:50 Floor Plan".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let bt_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let wall = CollectedWall {
            handle: acadrust::Handle::from(100u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "WallStyle".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(5.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: uuid::Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: vec![
                CollectedOpening {
                    handle: acadrust::Handle::from(101u64),
                    element_ref: bt_elem,
                    xref_source: None,
                    host_wall_handle: acadrust::Handle::from(100u64),
                    kind: OpeningKind::Breakthrough,
                    style_id: None,
                    distance_along_axis: 2.5,
                    width: 1.0,
                    height: 1.0,
                    sill_height: 0.5,
                    spring_height: 0.0,
                    shape: OpeningShape::Rectangle,
                    hinge: HingeSide::Left,
                    swing_side: Default::default(),
                    reference_side: OpeningReferenceSide::Start,
                    cross_axis_offset: 0.0,
                    depth: None,
                    world_center: Point3::new(2.5, 0.0, 1.0),
                    axis_span_local: (2.0, 3.0),
                },
            ],
            phase: PlanPhase::New,
        };

        let slab_elem = ElementRef::new(ModelId::new(), ElementId::new());
        let slab = CollectedSlab {
            handle: acadrust::Handle::from(200u64),
            element_ref: slab_elem,
            xref_source: None,
            style_id: "FloorSlab".into(),
            base_z: -0.20,
            total_thickness: 0.20,
            polygon: vec![
                Point2::new(1.0, -1.0),
                Point2::new(4.0, -1.0),
                Point2::new(4.0, 0.0),
                Point2::new(1.0, 0.0),
            ],
            layers: Vec::new(),
            justification: crate::modules::aec::engine::slab::SlabJustification::Top,
            openings: Vec::new(),
            phase: PlanPhase::New,
        };

        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);
        collector.slabs.push(slab);

        let opts = FloorPlanEngineOptions {
            emit_hidden_lines: false,
            ..Default::default()
        };
        let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &opts).unwrap();

        let visible_slab_in_bt = prims.iter().find(|p| {
            p.role == PrimitiveRole::ProjectedEdge
                && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
                && match &p.geometry {
                    Geometry2d::Line { start, end } => {
                        let mid_x = (start.x_m + end.x_m) * 0.5;
                        let mid_y = (start.y_m + end.y_m) * 0.5;
                        mid_x >= 2.0 && mid_x <= 3.0 && mid_y.abs() < 1e-4
                    }
                    _ => false,
                }
        });

        assert!(
            visible_slab_in_bt.is_none(),
            "Breakthrough with sill must occlude below-cut slab edge"
        );
    }
}
