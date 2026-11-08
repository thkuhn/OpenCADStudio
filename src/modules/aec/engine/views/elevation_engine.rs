//! 2D Elevation and Façade Derivation Engine.
//!
//! Projects 3D walls, slabs, and openings onto the view plane with full slot-based
//! opening components (ElevationContour2D, ElevationSwing2D, ElevationSill2D, ElevationMuntins2D).

use super::model_view::{DetailLevel, ModelView};
use super::provenance::{
    DerivedPrimitive, EditHandle, FeatureKey, Geometry2d, ModelEditAction, ModelParameter,
    PrimitiveId, PrimitiveRole, ResolvedStyle, SourceReference, SourceSet,
};
use super::scene_collector::{CollectedOpening, CollectedSlab, CollectedWall, SceneGeometryCollector};
use super::viewpoint::{ViewKind, Viewpoint};
use super::{
    DepthInterval, PaperMm, Point2, Point3, ValidationError, Vector2,
};
use crate::modules::aec::engine::display_component::OpeningComponentSlot;
use crate::modules::aec::engine::library::StyleLibrary;
use crate::modules::aec::engine::opening_style::project_division_sized;
use crate::modules::aec::engine::opening_display::{
    bake_opening_elevation_from_grid, OpeningBakeParams,
};
use crate::modules::aec::engine::openings::{OpeningKind, SwingSide};
use crate::modules::aec::engine::plan_view::DisplayConfig;
use super::style_resolver::{
    is_draw_suppressed, is_opening_slot_visible, is_wall_layer_visible, is_wall_phase_visible,
    is_slab_layer_visible, resolve_slab_layer_cut_style,
    resolve_opening_representation_levels, resolve_opening_style,
    resolve_opening_view_fill_style, resolve_opening_view_style,
    resolve_wall_elevation_contour_style, ViewStyleContext,
};

const EPS: f64 = 1e-6;
const OPENING_SURFACE_DEPTH_OFFSET: f64 = 1e-3;

fn unit_left_normal(dx: f64, dy: f64) -> Option<(f64, f64)> {
    let len = dx.hypot(dy);
    if len < EPS {
        return None;
    }
    Some((-dy / len, dx / len))
}

/// Offset, along the left normal, of the layer face turned toward the camera.
fn facing_layer_offset(offset: f64, thickness: f64, n_dot: f64) -> f64 {
    if n_dot < 0.0 {
        offset + thickness
    } else {
        offset
    }
}

/// Depth of the face turned toward the camera. Axis depth would let a nearer
/// wall's thickness fall behind an opening that is only slightly farther away.
fn visible_wall_face_depth(
    axis_depth: f64,
    dx: f64,
    dy: f64,
    wall: &CollectedWall,
    visible_layer_indices: &[usize],
    forward: Vector2,
) -> Option<f64> {
    let Some((nx, ny)) = unit_left_normal(dx, dy) else {
        return Some(axis_depth);
    };
    let n_dot = nx * forward.x + ny * forward.y;
    let offsets = wall.layer_offsets();
    visible_layer_indices
        .iter()
        .filter_map(|index| {
            let (offset, thickness) = *offsets.get(*index)?;
            let layer_front = facing_layer_offset(offset, thickness, n_dot);
            Some(axis_depth + layer_front * n_dot)
        })
        .reduce(f64::min)
}

fn offset_point(point: Point2, nx: f64, ny: f64, offset: f64) -> Point2 {
    Point2::new(point.x_m + nx * offset, point.y_m + ny * offset)
}

fn cross(origin: Point2, a: Point2, b: Point2) -> f64 {
    (a.x_m - origin.x_m) * (b.y_m - origin.y_m) - (a.y_m - origin.y_m) * (b.x_m - origin.x_m)
}

fn convex_hull(points: &[Point2]) -> Vec<Point2> {
    let mut pts: Vec<Point2> = points
        .iter()
        .copied()
        .filter(|point| point.x_m.is_finite() && point.y_m.is_finite())
        .collect();
    pts.sort_by(|a, b| {
        a.x_m
            .partial_cmp(&b.x_m)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.y_m.partial_cmp(&b.y_m).unwrap_or(std::cmp::Ordering::Equal))
    });
    pts.dedup_by(|a, b| (a.x_m - b.x_m).abs() < EPS && (a.y_m - b.y_m).abs() < EPS);
    if pts.len() <= 1 {
        return pts;
    }
    let mut lower = Vec::new();
    for point in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], *point) <= EPS {
            lower.pop();
        }
        lower.push(*point);
    }
    let mut upper = Vec::new();
    for point in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], *point) <= EPS {
            upper.pop();
        }
        upper.push(*point);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

fn project_facade_point_on_segment(
    viewpoint: &Viewpoint,
    anchor: Point2,
    point: Point3,
) -> (Point2, f64) {
    let Some((base_station, segment_start, direction)) = section_segment_at(viewpoint, anchor) else {
        return project_facade_point(viewpoint, point);
    };
    let forward = facade_forward_at(viewpoint, anchor);
    let dx = point.x_m - segment_start.x_m;
    let dy = point.y_m - segment_start.y_m;
    let station = base_station + dx * direction.x + dy * direction.y;
    (
        viewpoint.mapping.apply(Point2::new(station, point.z_m)),
        dx * forward.x + dy * forward.y,
    )
}

fn layer_band_hull(
    viewpoint: &Viewpoint,
    p0: Point2,
    p1: Point2,
    nx: f64,
    ny: f64,
    offset: f64,
    thickness: f64,
    z_bottom: f64,
    z_top: f64,
) -> Vec<Point2> {
    let corners = [
        offset_point(p0, nx, ny, offset),
        offset_point(p1, nx, ny, offset),
        offset_point(p1, nx, ny, offset + thickness),
        offset_point(p0, nx, ny, offset + thickness),
    ];
    let anchor = Point2::new((p0.x_m + p1.x_m) * 0.5, (p0.y_m + p1.y_m) * 0.5);
    let mut paper = Vec::with_capacity(8);
    for corner in corners {
        for z in [z_bottom, z_top] {
            let (uv, _) = project_facade_point_on_segment(
                viewpoint,
                anchor,
                Point3::new(corner.x_m, corner.y_m, z),
            );
            paper.push(uv);
        }
    }
    convex_hull(&paper)
}

/// Lateral offset of the nearest visible layer face. Openings sit on that face, not on the axis.
fn visible_face_offset(
    dx: f64,
    dy: f64,
    wall: &CollectedWall,
    visible_layer_indices: &[usize],
    forward: Vector2,
) -> Option<f64> {
    let (nx, ny) = unit_left_normal(dx, dy)?;
    let n_dot = nx * forward.x + ny * forward.y;
    let offsets = wall.layer_offsets();
    visible_layer_indices
        .iter()
        .filter_map(|index| {
            let (offset, thickness) = *offsets.get(*index)?;
            let face = facing_layer_offset(offset, thickness, n_dot);
            Some((face * n_dot, face))
        })
        .min_by(|left, right| left.0.partial_cmp(&right.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(_, face)| face)
}

/// Projects a facade point: horizontal station from the view axes, vertical
/// station from world Z on the same datum as a section cut.
fn project_facade_point(viewpoint: &Viewpoint, point: Point3) -> (Point2, f64) {
    let (uv, depth) = viewpoint.world_to_view(point);
    (
        Point2::new(uv.x_m, viewpoint.paper_elevation(point.z_m)),
        depth,
    )
}

fn section_segment_at(viewpoint: &Viewpoint, point: Point2) -> Option<(f64, Point2, Vector2)> {
    let ViewKind::Section { path, .. } = &viewpoint.kind else {
        return None;
    };
    let mut accumulated_station = 0.0;
    let mut closest = None;
    for segment in path.segments() {
        let length = segment.length();
        if length < EPS {
            continue;
        }
        let Ok(direction) = segment.direction() else {
            accumulated_station += length;
            continue;
        };
        let along = ((point.x_m - segment.start.x_m) * direction.x
            + (point.y_m - segment.start.y_m) * direction.y)
            .clamp(0.0, length);
        let closest_point = Point2::new(
            segment.start.x_m + direction.x * along,
            segment.start.y_m + direction.y * along,
        );
        let distance = point.distance_to(closest_point);
        if closest.map_or(true, |(best_distance, _, _, _)| distance < best_distance) {
            closest = Some((distance, accumulated_station, segment.start, direction));
        }
        accumulated_station += length;
    }
    closest.map(|(_, station, start, direction)| (station, start, direction))
}

fn facade_forward_at(viewpoint: &Viewpoint, point: Point2) -> Vector2 {
    let Some((_, _, direction)) = section_segment_at(viewpoint, point) else {
        return Vector2::new(viewpoint.axes.forward.x, viewpoint.axes.forward.y);
    };
    let left_normal = Vector2::new(-direction.y, direction.x);
    let side = if left_normal.x * viewpoint.axes.forward.x
        + left_normal.y * viewpoint.axes.forward.y
        >= 0.0
    {
        1.0
    } else {
        -1.0
    };
    Vector2::new(left_normal.x * side, left_normal.y * side)
}

fn project_facade_segment(
    viewpoint: &Viewpoint,
    start: Point3,
    end: Point3,
) -> ((Point2, f64), (Point2, f64)) {
    let Some((base_station, segment_start, direction)) = section_segment_at(
        viewpoint,
        Point2::new((start.x_m + end.x_m) * 0.5, (start.y_m + end.y_m) * 0.5),
    ) else {
        return (
            project_facade_point(viewpoint, start),
            project_facade_point(viewpoint, end),
        );
    };

    let forward = facade_forward_at(
        viewpoint,
        Point2::new((start.x_m + end.x_m) * 0.5, (start.y_m + end.y_m) * 0.5),
    );
    let project = |point: Point3| {
        let dx = point.x_m - segment_start.x_m;
        let dy = point.y_m - segment_start.y_m;
        let station = base_station + dx * direction.x + dy * direction.y;
        let paper = viewpoint.mapping.apply(Point2::new(station, point.z_m));
        (paper, dx * forward.x + dy * forward.y)
    };
    (project(start), project(end))
}

#[derive(Clone, Debug)]
pub struct ElevationEngineOptions {
    pub show_hidden_edges: bool,
    pub include_opening_symbols: bool,
    pub detail_level: DetailLevel,
    pub display_config: Option<DisplayConfig>,
    pub style_library: Option<StyleLibrary>,
}

impl Default for ElevationEngineOptions {
    fn default() -> Self {
        Self {
            show_hidden_edges: false,
            include_opening_symbols: true,
            detail_level: DetailLevel::Fine,
            display_config: None,
            style_library: None,
        }
    }
}

/// Derives 2D elevation primitives from a Viewpoint and ModelView across the scene collector.
pub fn derive_elevation_view(
    viewpoint: &Viewpoint,
    model_view: &ModelView,
    scene: &SceneGeometryCollector,
    options: &ElevationEngineOptions,
) -> Result<Vec<DerivedPrimitive>, ValidationError> {
    viewpoint.validate()?;
    model_view.validate()?;

    let (near_depth, far_depth) = match &viewpoint.kind {
        ViewKind::Elevation { depth } => (
            depth.near.get(),
            depth.far.map(|d| d.get()).unwrap_or(f64::INFINITY),
        ),
        ViewKind::Section { far_depth, .. } => (
            0.0,
            far_depth.map(|d| d.get()).unwrap_or(f64::INFINITY),
        ),
        _ => (0.0, f64::INFINITY),
    };

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
        true,
    );

    let mut primitives = Vec::new();
    let mut primitive_counter: u32 = 0;

    let z_min = viewpoint.z_range.z_min.get();
    let z_max = viewpoint.z_range.z_max.get();

    // 1. Process Walls
    let wall_footprints = super::floorplan_engine::solve_wall_layer_footprints(scene);
    for (wall_index, wall) in scene.walls.iter().enumerate() {
        if model_view.excluded_elements.contains(&wall.element_ref) {
            continue;
        }
        if wall.top_z < z_min || wall.base_z > z_max {
            continue;
        }

        project_wall_elevation_with_footprints(
            wall,
            viewpoint,
            near_depth,
            far_depth,
            model_view,
            options,
            &ctx,
            wall_footprints.get(wall_index).map(Vec::as_slice),
            &mut primitives,
            &mut primitive_counter,
        )?;
    }

    // 2. Process Slabs
    for slab in &scene.slabs {
        if model_view.excluded_elements.contains(&slab.element_ref) {
            continue;
        }
        let (slab_z_min, slab_z_max) = slab.z_range();
        if slab_z_max < z_min || slab_z_min > z_max {
            continue;
        }

        project_slab_elevation(
            slab,
            viewpoint,
            near_depth,
            far_depth,
            model_view,
            options,
            &ctx,
            &mut primitives,
            &mut primitive_counter,
        )?;
    }

    // 3. Resolve all contributors in the shared elevation paper frame.
    let mut primitives = super::frame::resolve_view_frame(
        primitives,
        &super::silhouette::SilhouetteOptions {
            emit_hidden_lines: options.show_hidden_edges,
            max_depth_distance: Some(far_depth),
            hidden_layer: Some("AEC_HIDDEN".to_string()),
        },
    )?;

    primitives.retain(|p| {
        p.style.layer_name.as_deref() != Some(super::silhouette::TRANSIENT_OCCLUDER_LAYER)
    });

    Ok(primitives)
}

pub fn project_wall_elevation(
    wall: &CollectedWall,
    viewpoint: &Viewpoint,
    near_depth: f64,
    far_depth: f64,
    model_view: &ModelView,
    options: &ElevationEngineOptions,
    ctx: &ViewStyleContext,
    primitives: &mut Vec<DerivedPrimitive>,
    counter: &mut u32,
) -> Result<(), ValidationError> {
    project_wall_elevation_with_footprints(
        wall,
        viewpoint,
        near_depth,
        far_depth,
        model_view,
        options,
        ctx,
        None,
        primitives,
        counter,
    )
}

pub(super) fn project_wall_elevation_with_footprints(
    wall: &CollectedWall,
    viewpoint: &Viewpoint,
    near_depth: f64,
    far_depth: f64,
    model_view: &ModelView,
    options: &ElevationEngineOptions,
    ctx: &ViewStyleContext,
    wall_footprints: Option<&[Option<Vec<(f64, f64)>>]>,
    primitives: &mut Vec<DerivedPrimitive>,
    counter: &mut u32,
) -> Result<(), ValidationError> {
    if !is_wall_phase_visible(ctx, wall) || is_draw_suppressed(ctx, &wall.element_ref, None) {
        return Ok(());
    }
    if wall.axis_points.len() < 2 {
        return Ok(());
    }

    let visible_layer_indices: Vec<usize> = wall
        .layers
        .iter()
        .enumerate()
        .filter_map(|(index, _)| is_wall_layer_visible(ctx, wall, index).then_some(index))
        .collect();
    if visible_layer_indices.is_empty() {
        return Ok(());
    }

    let style = resolve_wall_elevation_contour_style(ctx, wall);
    let joined_layer_edges: Vec<Option<Vec<(Point2, Point2)>>> = wall
        .layers
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let footprint = wall_footprints
                .and_then(|footprints| footprints.get(index))
                .and_then(Option::as_deref)?;
            let edges = visible_footprint_edges(viewpoint, footprint);
            (!edges.is_empty()).then_some(edges)
        })
        .collect();

    let mut station = 0.0;
    for window in wall.axis_points.windows(2) {
        let p0 = window[0];
        let p1 = window[1];
        let seg_len = p0.distance_to(p1);
        let dx = p1.x_m - p0.x_m;
        let dy = p1.y_m - p0.y_m;
        let Some((nx, ny)) = unit_left_normal(dx, dy) else {
            station += seg_len;
            continue;
        };

        let ((_, d0_base), (_, d1_base)) = project_facade_segment(
            viewpoint,
            Point3::new(p0.x_m, p0.y_m, wall.base_z),
            Point3::new(p1.x_m, p1.y_m, wall.base_z),
        );
        let axis_depth = (d0_base + d1_base) * 0.5;
        if axis_depth < near_depth - EPS || axis_depth > far_depth + EPS {
            station += seg_len;
            continue;
        }
        let segment_midpoint = Point2::new((p0.x_m + p1.x_m) * 0.5, (p0.y_m + p1.y_m) * 0.5);
        let forward = facade_forward_at(viewpoint, segment_midpoint);
        let Some(avg_depth) = visible_wall_face_depth(
            axis_depth,
            dx,
            dy,
            wall,
            &visible_layer_indices,
            forward,
        ) else {
            station += seg_len;
            continue;
        };
        let face_offset = visible_face_offset(dx, dy, wall, &visible_layer_indices, forward)
            .unwrap_or(0.0);
        let offsets = wall.layer_offsets();
        let mut top_handle_emitted = false;

        for &index in &visible_layer_indices {
            let Some(layer) = wall.layers.get(index) else {
                continue;
            };
            if joined_layer_edges.get(index).is_some_and(Option::is_some) {
                continue;
            }
            let Some(&(offset, thickness)) = offsets.get(index) else {
                continue;
            };
            let z_bottom = wall.base_z + layer.bottom_offset;
            let z_top = wall.top_z + layer.top_offset;
            let hull = layer_band_hull(
                viewpoint,
                p0,
                p1,
                nx,
                ny,
                offset,
                thickness,
                z_bottom,
                z_top,
            );
            if hull.len() < 2 {
                continue;
            }
            let layer_depth = visible_wall_face_depth(
                axis_depth,
                dx,
                dy,
                wall,
                &[index],
                forward,
            )
            .unwrap_or(avg_depth);
            let mut layer_source = SourceReference {
                element: wall.element_ref,
                xref_source: wall.xref_source.clone(),
                component_id: Some(layer.component_id),
                feature: Some(FeatureKey::LayerFace),
                display_component_id: None,
            };
            let top_y = viewpoint.paper_elevation(z_top);
            let base_y = viewpoint.paper_elevation(z_bottom);
            let mut ring = hull.clone();
            ring.push(hull[0]);
            for edge in ring.windows(2) {
                let start = edge[0];
                let end = edge[1];
                if start.distance_to(end) < EPS {
                    continue;
                }
                let is_top = (start.y_m - top_y).abs() < 1e-4 && (end.y_m - top_y).abs() < 1e-4;
                let is_base = (start.y_m - base_y).abs() < 1e-4 && (end.y_m - base_y).abs() < 1e-4;
                if is_top {
                    layer_source.feature = Some(FeatureKey::WallTop);
                } else if is_base {
                    layer_source.feature = Some(FeatureKey::WallBase);
                } else {
                    layer_source.feature = Some(FeatureKey::LayerFace);
                }
                let handles = if is_top && !top_handle_emitted {
                    top_handle_emitted = true;
                    vec![EditHandle {
                        id: format!("wall_top_elev_{}", *counter + 1),
                        position: Point2::new(
                            (start.x_m + end.x_m) * 0.5,
                            (start.y_m + end.y_m) * 0.5,
                        ),
                        action: ModelEditAction::ChangeParameter(ModelParameter::WallTopOffset),
                        direction_hint: Vector2::new(0.0, 1.0),
                        target_feature: Some(FeatureKey::WallTop),
                        source: layer_source.clone(),
                    }]
                } else {
                    Vec::new()
                };
                *counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 1, *counter),
                    role: PrimitiveRole::ProjectedEdge,
                    geometry: Geometry2d::Line { start, end },
                    sources: SourceSet::new(vec![layer_source.clone()])?,
                    depth: DepthInterval::new(layer_depth, layer_depth)?,
                    style: style.clone(),
                    edit_handles: handles,
                });
            }
            if hull.len() >= 3 {
                *counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 1, *counter),
                    role: PrimitiveRole::SurfaceFill,
                    geometry: Geometry2d::Region {
                        outer: hull,
                        holes: vec![],
                    },
                    sources: SourceSet::new(vec![layer_source])?,
                    depth: DepthInterval::new(layer_depth, layer_depth)?,
                    style: ResolvedStyle {
                        line_weight_mm: PaperMm::new(0.18)?,
                        line_color: None,
                        line_type: None,
                        fill_color: None,
                        hatch_pattern: None,
                        hatch_scale: None,
                        hatch_angle_rad: None,
                        layer_name: Some(super::silhouette::TRANSIENT_OCCLUDER_LAYER.to_string()),
                    },
                    edit_handles: vec![],
                });
            }
        }

        let p0_face = offset_point(p0, nx, ny, face_offset);
        let p1_face = offset_point(p1, nx, ny, face_offset);
        // Opening surfaces participate in occlusion independently of their visible symbols.
        for op in &wall.openings {
            let (s_lo, s_hi) = op.axis_span_local;
            if s_hi < station - EPS || s_lo > station + seg_len + EPS {
                continue;
            }
            let mut local_op = op.clone();
            local_op.axis_span_local = ((s_lo - station).max(0.0), (s_hi - station).min(seg_len));
            project_opening_elevation(
                &local_op,
                wall,
                p0_face,
                p1_face,
                viewpoint,
                near_depth,
                far_depth,
                model_view,
                ctx,
                &visible_layer_indices,
                options.include_opening_symbols,
                primitives,
                counter,
            )?;
        }
        station += seg_len;
    }

    for &index in &visible_layer_indices {
        let Some(edges) = joined_layer_edges.get(index).and_then(Option::as_ref) else {
            continue;
        };
        let Some(layer) = wall.layers.get(index) else {
            continue;
        };
        let z_bottom = wall.base_z + layer.bottom_offset;
        let z_top = wall.top_z + layer.top_offset;
        let mut top_handle_emitted = false;
        for &(edge_start, edge_end) in edges {
            let anchor = Point2::new(
                (edge_start.x_m + edge_end.x_m) * 0.5,
                (edge_start.y_m + edge_end.y_m) * 0.5,
            );
            let world_corners = [
                Point3::new(edge_start.x_m, edge_start.y_m, z_bottom),
                Point3::new(edge_end.x_m, edge_end.y_m, z_bottom),
                Point3::new(edge_end.x_m, edge_end.y_m, z_top),
                Point3::new(edge_start.x_m, edge_start.y_m, z_top),
            ];
            let mut depths = Vec::with_capacity(world_corners.len());
            let mut paper_corners = Vec::with_capacity(world_corners.len());
            for corner in world_corners {
                let (paper, depth) = project_facade_point_on_segment(viewpoint, anchor, corner);
                paper_corners.push(paper);
                depths.push(depth);
            }
            let depth = depths.iter().sum::<f64>() / depths.len() as f64;
            if depth < near_depth - EPS || depth > far_depth + EPS {
                continue;
            }
            let hull = convex_hull(&paper_corners);
            if hull.len() < 2 {
                continue;
            }
            let mut layer_source = SourceReference {
                element: wall.element_ref,
                xref_source: wall.xref_source.clone(),
                component_id: Some(layer.component_id),
                feature: Some(FeatureKey::LayerFace),
                display_component_id: None,
            };
            let top_y = viewpoint.paper_elevation(z_top);
            let base_y = viewpoint.paper_elevation(z_bottom);
            let mut ring = hull.clone();
            ring.push(hull[0]);
            for edge in ring.windows(2) {
                let start = edge[0];
                let end = edge[1];
                if start.distance_to(end) < EPS {
                    continue;
                }
                let is_top = (start.y_m - top_y).abs() < 1e-4
                    && (end.y_m - top_y).abs() < 1e-4;
                let is_base = (start.y_m - base_y).abs() < 1e-4
                    && (end.y_m - base_y).abs() < 1e-4;
                layer_source.feature = if is_top {
                    Some(FeatureKey::WallTop)
                } else if is_base {
                    Some(FeatureKey::WallBase)
                } else {
                    Some(FeatureKey::LayerFace)
                };
                let handles = if is_top && !top_handle_emitted {
                    top_handle_emitted = true;
                    vec![EditHandle {
                        id: format!("wall_top_elev_{}", *counter + 1),
                        position: Point2::new(
                            (start.x_m + end.x_m) * 0.5,
                            (start.y_m + end.y_m) * 0.5,
                        ),
                        action: ModelEditAction::ChangeParameter(ModelParameter::WallTopOffset),
                        direction_hint: Vector2::new(0.0, 1.0),
                        target_feature: Some(FeatureKey::WallTop),
                        source: layer_source.clone(),
                    }]
                } else {
                    Vec::new()
                };
                *counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 1, *counter),
                    role: PrimitiveRole::ProjectedEdge,
                    geometry: Geometry2d::Line { start, end },
                    sources: SourceSet::new(vec![layer_source.clone()])?,
                    depth: DepthInterval::new(depth, depth)?,
                    style: style.clone(),
                    edit_handles: handles,
                });
            }
            if hull.len() >= 3 {
                *counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 1, *counter),
                    role: PrimitiveRole::SurfaceFill,
                    geometry: Geometry2d::Region {
                        outer: hull,
                        holes: vec![],
                    },
                    sources: SourceSet::new(vec![layer_source])?,
                    depth: DepthInterval::new(depth, depth)?,
                    style: ResolvedStyle {
                        line_weight_mm: PaperMm::new(0.18)?,
                        line_color: None,
                        line_type: None,
                        fill_color: None,
                        hatch_pattern: None,
                        hatch_scale: None,
                        hatch_angle_rad: None,
                        layer_name: Some(super::silhouette::TRANSIENT_OCCLUDER_LAYER.to_string()),
                    },
                    edit_handles: vec![],
                });
            }
        }
    }

    Ok(())
}

fn visible_footprint_edges(
    viewpoint: &Viewpoint,
    footprint: &[(f64, f64)],
) -> Vec<(Point2, Point2)> {
    if footprint.len() < 3 {
        return Vec::new();
    }
    let signed_area = footprint
        .iter()
        .zip(footprint.iter().cycle().skip(1))
        .take(footprint.len())
        .map(|(a, b)| a.0 * b.1 - b.0 * a.1)
        .sum::<f64>();
    if signed_area.abs() < EPS {
        return Vec::new();
    }
    let ccw = signed_area > 0.0;
    let mut edges = Vec::new();
    for index in 0..footprint.len() {
        let a = footprint[index];
        let b = footprint[(index + 1) % footprint.len()];
        let edge_x = b.0 - a.0;
        let edge_y = b.1 - a.1;
        let length = edge_x.hypot(edge_y);
        if length < EPS {
            continue;
        }
        let (normal_x, normal_y) = if ccw {
            (edge_y / length, -edge_x / length)
        } else {
            (-edge_y / length, edge_x / length)
        };
        let start = Point2::new(a.0, a.1);
        let end = Point2::new(b.0, b.1);
        let midpoint = Point2::new((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
        let forward = facade_forward_at(viewpoint, midpoint);
        if normal_x * forward.x + normal_y * forward.y < -EPS {
            edges.push((start, end));
        }
    }
    edges
}

fn project_opening_elevation(
    op: &CollectedOpening,
    wall: &CollectedWall,
    w_p0: Point2,
    w_p1: Point2,
    viewpoint: &Viewpoint,
    near_depth: f64,
    far_depth: f64,
    model_view: &ModelView,
    ctx: &ViewStyleContext,
    visible_layer_indices: &[usize],
    include_symbols: bool,
    primitives: &mut Vec<DerivedPrimitive>,
    counter: &mut u32,
) -> Result<(), ValidationError> {
    if is_draw_suppressed(ctx, &op.element_ref, None) {
        return Ok(());
    }

    let seg_vec = Vector2::new(w_p1.x_m - w_p0.x_m, w_p1.y_m - w_p0.y_m);
    let seg_len = seg_vec.length();
    if seg_len < EPS {
        return Ok(());
    }
    let dir = seg_vec.normalized()?;

    let (s_lo, s_hi) = op.axis_span_local;
    let z_sill = wall.base_z + op.sill_height;
    let z_head = z_sill + op.height;

    // 4 corners of opening in 3D world space
    let pt_bl = Point3::new(w_p0.x_m + dir.x * s_lo, w_p0.y_m + dir.y * s_lo, z_sill);
    let pt_br = Point3::new(w_p0.x_m + dir.x * s_hi, w_p0.y_m + dir.y * s_hi, z_sill);
    let pt_tl = Point3::new(w_p0.x_m + dir.x * s_lo, w_p0.y_m + dir.y * s_lo, z_head);
    let pt_tr = Point3::new(w_p0.x_m + dir.x * s_hi, w_p0.y_m + dir.y * s_hi, z_head);

    let ((uv_bl, d_bl), (uv_br, d_br)) = project_facade_segment(viewpoint, pt_bl, pt_br);
    let ((uv_tl, _), (uv_tr, _)) = project_facade_segment(viewpoint, pt_tl, pt_tr);

    let axis_depth = (d_bl + d_br) * 0.5;
    if axis_depth < near_depth - EPS || axis_depth > far_depth + EPS {
        return Ok(());
    }
    let midpoint = Point2::new(
        (pt_bl.x_m + pt_br.x_m) * 0.5,
        (pt_bl.y_m + pt_br.y_m) * 0.5,
    );
    let Some(avg_depth) = visible_wall_face_depth(
        axis_depth,
        dir.x,
        dir.y,
        wall,
        visible_layer_indices,
        facade_forward_at(viewpoint, midpoint),
    ) else {
        return Ok(());
    };

    let op_source = SourceReference {
        element: op.element_ref,
        xref_source: op.xref_source.clone(),
        component_id: None,
        feature: Some(FeatureKey::OpeningSill),
        display_component_id: None,
    };

    let frame_poly = vec![uv_bl, uv_br, uv_tr, uv_tl];

    // 0. Opaque opening surface filler: prevents background geometry from shining through opening
    let fill_slot = if op.kind == OpeningKind::Door {
        OpeningComponentSlot::Leaf2D
    } else {
        OpeningComponentSlot::Glazing2D
    };
    let fill_style = resolve_opening_view_fill_style(ctx, op, fill_slot).unwrap_or_else(|| {
        ResolvedStyle {
            line_weight_mm: PaperMm::new(0.18).unwrap(),
            line_color: None,
            line_type: None,
            fill_color: None,
            hatch_pattern: None,
            hatch_scale: None,
            hatch_angle_rad: None,
            layer_name: Some(super::silhouette::TRANSIENT_OCCLUDER_LAYER.to_string()),
        }
    });

    *counter += 1;
    primitives.push(DerivedPrimitive {
        id: PrimitiveId::new(model_view.id, 1, *counter),
        role: PrimitiveRole::SurfaceFill,
        geometry: Geometry2d::Region {
            outer: frame_poly.clone(),
            holes: vec![],
        },
        sources: SourceSet::new(vec![op_source.clone()])?,
        depth: DepthInterval::new(
            avg_depth - OPENING_SURFACE_DEPTH_OFFSET,
            avg_depth - OPENING_SURFACE_DEPTH_OFFSET,
        )?,
        style: fill_style,
        edit_handles: vec![],
    });

    if !include_symbols {
        return Ok(());
    }

    let style = resolve_opening_style(ctx, op);
    let construction = op.division_override.as_ref().unwrap_or(&style.grid);
    let grid = project_division_sized(construction, op.width, op.height);
    let levels = resolve_opening_representation_levels(ctx, op, &grid);
    let wall_half_depth = wall.total_thickness * 0.5;
    let bake_params = OpeningBakeParams {
        width: op.width,
        height: op.height,
        sill_height: op.sill_height,
        thickness: wall.total_thickness,
        frame_thickness: style.parts.fixed_frame.face_width,
        cross_axis_offset: op.cross_axis_offset,
        wall_y_min: -wall_half_depth,
        wall_y_max: wall_half_depth,
        hinge: op.hinge,
        swing_side: op.swing_side,
        shape: op.shape,
        spring_height: op.spring_height,
        opening_angle_deg: style.opening_angle_deg,
        kind: op.kind,
    };
    for baked in bake_opening_elevation_from_grid(
        bake_params,
        &style.parts,
        ctx.style_library,
        construction,
        levels.elevation_frame,
        levels.divisions,
    ) {
        if !is_opening_slot_visible(ctx, op, baked.slot) || baked.points.len() < 2 {
            continue;
        }
        let projected: Vec<Point2> = baked
            .points
            .iter()
            .map(|&(x, z)| {
                let t = ((x + op.width * 0.5) / op.width).clamp(0.0, 1.0);
                let base = Point2::new(
                    uv_bl.x_m + (uv_br.x_m - uv_bl.x_m) * t,
                    uv_bl.y_m + (uv_br.y_m - uv_bl.y_m) * t,
                );
                let vertical_left = Point2::new(uv_tl.x_m - uv_bl.x_m, uv_tl.y_m - uv_bl.y_m);
                let vertical_right = Point2::new(uv_tr.x_m - uv_br.x_m, uv_tr.y_m - uv_br.y_m);
                Point2::new(
                    base.x_m + (vertical_left.x_m + vertical_right.x_m) * 0.5 * (z / op.height),
                    base.y_m + (vertical_left.y_m + vertical_right.y_m) * 0.5 * (z / op.height),
                )
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
        let symbol_style = resolve_opening_view_style(ctx, op, baked.slot);
        *counter += 1;
        primitives.push(DerivedPrimitive {
            id: PrimitiveId::new(model_view.id, 1, *counter),
            role: PrimitiveRole::ProjectedEdge,
            geometry,
            sources: SourceSet::new(vec![op_source.clone()])?,
            depth: DepthInterval::new(avg_depth, avg_depth)?,
            style: symbol_style,
            edit_handles: vec![],
        });
    }

    // 2. ElevationSwing2D: DIN Opening triangle symbol
    if (op.kind == OpeningKind::Window || op.kind == OpeningKind::Door)
        && is_opening_slot_visible(ctx, op, OpeningComponentSlot::ElevationSwing2D)
    {
        let w_normal = Vector2::new(-dir.y, dir.x);
        let view_dir = viewpoint.axes.forward;
        let dot_normal = view_dir.x * w_normal.x + view_dir.y * w_normal.y;

        let observer_on_swing_side = if dot_normal <= 0.0 {
            op.swing_side == SwingSide::Exterior
        } else {
            op.swing_side == SwingSide::Interior
        };

        let (hinge_top, hinge_bot, strike_top, strike_bot) = match op.hinge.bearing() {
            crate::modules::aec::engine::construction_grid::TrackEnd::Start => (uv_tl, uv_bl, uv_tr, uv_br),
            crate::modules::aec::engine::construction_grid::TrackEnd::End => (uv_tr, uv_br, uv_tl, uv_bl),
        };

        // The tip marks the hinge jamb. A tip on the strike reads as a mirrored leaf.
        let apex = Point2::new(
            (hinge_top.x_m + hinge_bot.x_m) * 0.5,
            (hinge_top.y_m + hinge_bot.y_m) * 0.5,
        );
        let swing_pts = vec![strike_top, apex, strike_bot];

        let mut swing_style = resolve_opening_view_style(ctx, op, OpeningComponentSlot::ElevationSwing2D);
        if observer_on_swing_side && swing_style.line_type.as_deref() == Some("DASHED") {
            swing_style.line_type = None;
        }

        *counter += 1;
        primitives.push(DerivedPrimitive {
            id: PrimitiveId::new(model_view.id, 1, *counter),
            role: PrimitiveRole::Symbol,
            geometry: Geometry2d::Polyline {
                vertices: swing_pts,
                closed: false,
            },
            sources: SourceSet::new(vec![op_source.clone()])?,
            depth: DepthInterval::new(avg_depth, avg_depth)?,
            style: swing_style,
            edit_handles: vec![],
        });
    }

    // 3. ElevationSill2D: Window sill projection line
    if op.kind == OpeningKind::Window && is_opening_slot_visible(ctx, op, OpeningComponentSlot::ElevationSill2D) {
        let ear = 0.05;
        let sill_p0 = Point2::new(uv_bl.x_m.min(uv_br.x_m) - ear, uv_bl.y_m.min(uv_br.y_m) - 0.04);
        let sill_p1 = Point2::new(uv_bl.x_m.max(uv_br.x_m) + ear, uv_bl.y_m.min(uv_br.y_m) - 0.04);

        let sill_style = resolve_opening_view_style(ctx, op, OpeningComponentSlot::ElevationSill2D);

        *counter += 1;
        primitives.push(DerivedPrimitive {
            id: PrimitiveId::new(model_view.id, 1, *counter),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Line {
                start: sill_p0,
                end: sill_p1,
            },
            sources: SourceSet::new(vec![op_source])?,
            depth: DepthInterval::new(avg_depth, avg_depth)?,
            style: sill_style,
            edit_handles: vec![],
        });
    }

    Ok(())
}

pub fn project_slab_elevation(
    slab: &CollectedSlab,
    viewpoint: &Viewpoint,
    near_depth: f64,
    far_depth: f64,
    model_view: &ModelView,
    _options: &ElevationEngineOptions,
    ctx: &ViewStyleContext,
    primitives: &mut Vec<DerivedPrimitive>,
    counter: &mut u32,
) -> Result<(), ValidationError> {
    if !crate::modules::aec::engine::views::style_resolver::is_slab_phase_visible(ctx, slab)
        || is_draw_suppressed(ctx, &slab.element_ref, None)
    {
        return Ok(());
    }
    let n = slab.polygon.len();
    if n < 3 {
        return Ok(());
    }

    let visible_layers: Vec<_> = slab
        .layers
        .iter()
        .enumerate()
        .filter(|(index, _)| is_slab_layer_visible(ctx, slab, *index))
        .collect();
    if visible_layers.is_empty() {
        return Ok(());
    }
    let layer_ranges = slab.layer_z_ranges();
    let contour_style =
        crate::modules::aec::engine::views::style_resolver::resolve_slab_contour_style(ctx, slab, false);

    for i in 0..n {
        let p0 = slab.polygon[i];
        let p1 = slab.polygon[(i + 1) % n];

        for (layer_idx, layer) in &visible_layers {
            let (z_top, z_bottom) = layer_ranges[*layer_idx];
            let ((uv0_top, d0_top), (uv1_top, d1_top)) = project_facade_segment(
                viewpoint,
                Point3::new(p0.x_m, p0.y_m, z_top),
                Point3::new(p1.x_m, p1.y_m, z_top),
            );
            let ((uv0_bottom, d0_bottom), (uv1_bottom, d1_bottom)) = project_facade_segment(
                viewpoint,
                Point3::new(p0.x_m, p0.y_m, z_bottom),
                Point3::new(p1.x_m, p1.y_m, z_bottom),
            );
            let avg_depth = (d0_top + d1_top + d0_bottom + d1_bottom) * 0.25;
            if avg_depth < near_depth - EPS || avg_depth > far_depth + EPS {
                continue;
            }

            let (boundary_style, fill_style) = resolve_slab_layer_cut_style(ctx, slab, *layer_idx);
            let layer_source = SourceReference {
                element: slab.element_ref,
                xref_source: slab.xref_source.clone(),
                component_id: Some(layer.component_id),
                feature: Some(FeatureKey::LayerFace),
                display_component_id: None,
            };

            let previous_layer_active = *layer_idx > 0
                && is_slab_layer_visible(ctx, slab, *layer_idx - 1);
            let next_layer_active = *layer_idx + 1 < slab.layers.len()
                && is_slab_layer_visible(ctx, slab, *layer_idx + 1);
            if !previous_layer_active {
                *counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 1, *counter),
                    role: PrimitiveRole::ProjectedEdge,
                    geometry: Geometry2d::Line {
                        start: uv0_top,
                        end: uv1_top,
                    },
                    sources: SourceSet::new(vec![layer_source.clone()])?,
                    depth: DepthInterval::new(avg_depth, avg_depth)?,
                    style: boundary_style.clone().unwrap_or_else(|| contour_style.clone()),
                    edit_handles: vec![],
                });
            }
            let bottom_boundary_style = if next_layer_active {
                boundary_style.clone()
            } else {
                boundary_style.clone().or_else(|| Some(contour_style.clone()))
            };
            if let Some(bottom_boundary_style) = bottom_boundary_style {
                *counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 1, *counter),
                    role: PrimitiveRole::ProjectedEdge,
                    geometry: Geometry2d::Line {
                        start: uv0_bottom,
                        end: uv1_bottom,
                    },
                    sources: SourceSet::new(vec![layer_source.clone()])?,
                    depth: DepthInterval::new(avg_depth, avg_depth)?,
                    style: bottom_boundary_style,
                    edit_handles: vec![],
                });
            }

            if (uv1_bottom.x_m - uv0_bottom.x_m).hypot(uv1_bottom.y_m - uv0_bottom.y_m) > EPS {
                *counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 1, *counter),
                    role: PrimitiveRole::SurfaceFill,
                    geometry: Geometry2d::Region {
                        outer: vec![uv0_bottom, uv1_bottom, uv1_top, uv0_top],
                        holes: vec![],
                    },
                    sources: SourceSet::new(vec![layer_source])?,
                    depth: DepthInterval::new(avg_depth, avg_depth)?,
                    style: fill_style.unwrap_or(ResolvedStyle {
                        line_weight_mm: PaperMm::new(0.18)?,
                        line_color: None,
                        line_type: None,
                        fill_color: None,
                        hatch_pattern: None,
                        hatch_scale: None,
                        hatch_angle_rad: None,
                        layer_name: Some(super::silhouette::TRANSIENT_OCCLUDER_LAYER.to_string()),
                    }),
                    edit_handles: vec![],
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::plan_view::PlanPhase;
    use crate::modules::aec::engine::views::provenance::ElementRef;
    use crate::modules::aec::engine::views::scene_collector::CollectedWallLayer;
    use crate::modules::aec::engine::views::{ComponentId, ElementId, ModelId};
    use crate::modules::aec::engine::wall::WallJustification;

    #[test]
    fn facade_mask_depth_comes_from_nearest_visible_layer_not_hidden_plaster() {
        let wall = CollectedWall {
            handle: acadrust::Handle::new(1),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "ThreeLayerWall".to_string(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)],
            layers: vec![
                CollectedWallLayer {
                    material: "Plaster".to_string(),
                    thickness: 0.02,
                    function: "Finish".to_string(),
                    axis_offset: -0.10,
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    hatch_override: None,
                    layer_id: uuid::Uuid::new_v4(),
                    component_id: ComponentId::new(),
                    is_structural: false,
                },
                CollectedWallLayer {
                    material: "Concrete".to_string(),
                    thickness: 0.20,
                    function: "Structural".to_string(),
                    axis_offset: 0.0,
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    hatch_override: None,
                    layer_id: uuid::Uuid::new_v4(),
                    component_id: ComponentId::new(),
                    is_structural: true,
                },
                CollectedWallLayer {
                    material: "Plaster".to_string(),
                    thickness: 0.02,
                    function: "Finish".to_string(),
                    axis_offset: 0.10,
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    hatch_override: None,
                    layer_id: uuid::Uuid::new_v4(),
                    component_id: ComponentId::new(),
                    is_structural: false,
                },
            ],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: vec![],
            phase: PlanPhase::New,
        };

        let core_depth = visible_wall_face_depth(5.0, 1.0, 0.0, &wall, &[1], Vector2::new(0.0, 1.0));

        assert_eq!(core_depth, Some(5.0));
        assert_eq!(
            visible_wall_face_depth(5.0, 1.0, 0.0, &wall, &[0], Vector2::new(0.0, 1.0)),
            Some(4.9)
        );
        assert_eq!(
            visible_wall_face_depth(5.0, 1.0, 0.0, &wall, &[], Vector2::new(0.0, 1.0)),
            None
        );
    }

    #[test]
    fn elevation_outline_follows_visible_layer_faces_not_the_axis() {
        use crate::modules::aec::engine::views::{
            DepthRange, DetailPolicy, Geometry2d, Metres, ModelView, ModelViewId, NonNegativeMetres,
            Point3, PositiveMetres, PrimitiveRole, ProjectionMapping, Revision, ScaleDenominator,
            UpdatePolicy, VerticalRange, ViewAxes, ViewKind, ViewOrigin, ViewProfileId, ViewRevisions,
            Viewpoint, ViewpointId,
        };
        use std::collections::BTreeSet;

        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(CollectedWall {
            handle: acadrust::Handle::new(9),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "Return".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(0.0, 6.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.10,
                function: "Structural".into(),
                axis_offset: -0.20,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: uuid::Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.10,
            justification: WallJustification::Exterior,
            openings: vec![],
            phase: PlanPhase::New,
        });
        let viewpoint = Viewpoint {
            id: ViewpointId::new(),
            name: "South".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(0.0, -2.0, 0.0)),
            axes: ViewAxes::front_elevation(),
            kind: ViewKind::Elevation {
                depth: DepthRange {
                    near: NonNegativeMetres::ZERO,
                    far: Some(PositiveMetres::new(12.0).unwrap()),
                },
            },
            z_range: VerticalRange::new(Metres::new(-1.0).unwrap(), Metres::new(5.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };
        let model_view = ModelView {
            id: ModelViewId::new(),
            name: "South".into(),
            viewpoint_id: viewpoint.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(100.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let primitives = derive_elevation_view(
            &viewpoint,
            &model_view,
            &collector,
            &ElevationEngineOptions::default(),
        )
        .unwrap();
        let mut xs = Vec::new();
        for primitive in &primitives {
            if primitive.role != PrimitiveRole::ProjectedEdge {
                continue;
            }
            if let Geometry2d::Line { start, end } = &primitive.geometry {
                xs.push(start.x_m);
                xs.push(end.x_m);
            }
        }
        let min_x = xs.iter().copied().fold(f64::INFINITY, f64::min);
        let max_x = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(min_x.is_finite(), "elevation edge");
        assert!((min_x - 0.10).abs() < 1e-4, "near layer face {min_x}, not the axis");
        assert!((max_x - 0.20).abs() < 1e-4, "far layer face {max_x}, not the axis");
    }

    #[test]
    fn elevation_uses_the_joined_visible_layer_footprint() {
        use crate::modules::aec::engine::join::{
            JunctionOverride, JoinOverrideStyle, LayerPairOverride, LayerRef,
        };
        use crate::modules::aec::engine::junction_solver::{solve_network, WallJoinInput};
        use crate::modules::aec::engine::miter::MiterLayer;
        use crate::modules::aec::engine::plan_view::{LayerDetail, PlanningStage, ViewType};
        use crate::modules::aec::engine::views::{
            DepthRange, DetailPolicy, Geometry2d, Metres, ModelView, ModelViewId,
            NonNegativeMetres, Point3, PositiveMetres, ProjectionMapping, Revision,
            ScaleDenominator, UpdatePolicy, VerticalRange, ViewAxes, ViewKind, ViewOrigin,
            ViewProfileId, ViewRevisions, Viewpoint, ViewpointId,
        };
        use std::collections::BTreeSet;

        let target_element = ElementRef::new(ModelId::new(), ElementId::new());
        let target_component = ComponentId::new();
        let target_finish_id = uuid::Uuid::new_v4();
        let target_core_id = uuid::Uuid::new_v4();
        let target_finish = CollectedWallLayer {
            material: "plaster".into(),
            thickness: 0.02,
            function: "Finish".into(),
            axis_offset: -0.22,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: None,
            layer_id: target_finish_id,
            component_id: ComponentId::new(),
            is_structural: false,
        };
        let target_core = CollectedWallLayer {
            material: "core".into(),
            thickness: 0.20,
            function: "Structural".into(),
            axis_offset: -0.20,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: None,
            layer_id: target_core_id,
            component_id: target_component,
            is_structural: true,
        };
        let target_wall = CollectedWall {
            handle: acadrust::Handle::new(21),
            element_ref: target_element,
            xref_source: None,
            style_id: "Wall".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
            layers: vec![target_finish, target_core],
            total_thickness: 0.22,
            justification: WallJustification::Center,
            openings: vec![],
            phase: PlanPhase::New,
        };
        let partner_wall = CollectedWall {
            handle: acadrust::Handle::new(22),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "Wall".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(10.0, 0.0), Point2::new(10.0, 10.0)],
            layers: vec![
                CollectedWallLayer {
                    material: "plaster".into(),
                    thickness: 0.02,
                    function: "Finish".into(),
                    axis_offset: -0.22,
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    hatch_override: None,
                    layer_id: uuid::Uuid::new_v4(),
                    component_id: ComponentId::new(),
                    is_structural: false,
                },
                CollectedWallLayer {
                    material: "core".into(),
                    thickness: 0.20,
                    function: "Structural".into(),
                    axis_offset: -0.20,
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    hatch_override: None,
                    layer_id: uuid::Uuid::new_v4(),
                    component_id: ComponentId::new(),
                    is_structural: true,
                },
            ],
            total_thickness: 0.22,
            justification: WallJustification::Center,
            openings: vec![],
            phase: PlanPhase::New,
        };

        let miter_override = JunctionOverride {
            layer_pairs: vec![LayerPairOverride {
                layer_a: LayerRef {
                    material_id: "core".into(),
                    role_tag: None,
                    index: 1,
                    layer_id: Some(target_core_id),
                },
                layer_b: Some(LayerRef {
                    material_id: "core".into(),
                    role_tag: None,
                    index: 1,
                    layer_id: Some(partner_wall.layers[1].layer_id),
                }),
                style: JoinOverrideStyle::Miter,
            }],
            ..JunctionOverride::default()
        };
        let join_input = |wall: &CollectedWall, override_end| WallJoinInput {
            axis: wall
                .axis_points
                .iter()
                .map(|point| glam::DVec3::new(point.x_m, point.y_m, wall.base_z))
                .collect(),
            bulges: vec![],
            layers: wall
                .layers
                .iter()
                .map(|layer| MiterLayer {
                    thickness: layer.thickness,
                    axis_offset: layer.axis_offset,
                    material: layer.material.clone(),
                    function: layer.function.clone(),
                    layer_id: layer.layer_id,
                })
                .collect(),
            override_start: None,
            override_end,
            override_span: None,
        };
        let solved = solve_network(
            &[
                join_input(&target_wall, Some(miter_override.clone())),
                join_input(&partner_wall, None),
            ],
            0.05,
        );
        let footprint = solved.footprints[0][1]
            .as_ref()
            .expect("joined wall footprint");
        let front_y = footprint
            .iter()
            .map(|(_, y)| *y)
            .fold(f64::INFINITY, f64::min);
        let expected_max_x = footprint
            .iter()
            .filter(|(_, y)| (*y - front_y).abs() < 1e-6)
            .map(|(x, _)| *x)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (expected_max_x - 10.0).abs() > 1e-4,
            "join must change the visible edge extent: {footprint:?}"
        );

        let mut collector = SceneGeometryCollector::new();
        collector.walls = vec![target_wall, partner_wall];
        collector.wall_join_data = vec![
            Some(crate::modules::aec::engine::views::scene_collector::CollectedWallJoinData {
                override_end: Some(miter_override),
                ..Default::default()
            }),
            Some(Default::default()),
        ];
        let viewpoint = Viewpoint {
            id: ViewpointId::new(),
            name: "South".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(0.0, -2.0, 0.0)),
            axes: ViewAxes::front_elevation(),
            kind: ViewKind::Elevation {
                depth: DepthRange {
                    near: NonNegativeMetres::ZERO,
                    far: Some(PositiveMetres::new(12.0).unwrap()),
                },
            },
            z_range: VerticalRange::new(Metres::new(-1.0).unwrap(), Metres::new(5.0).unwrap())
                .unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };
        let model_view = ModelView {
            id: ModelViewId::new(),
            name: "South".into(),
            viewpoint_id: viewpoint.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(100.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };
        let mut display_config = DisplayConfig::new(
            "Structural elevation".into(),
            "Structural elevation".into(),
            PlanningStage::Design,
            ViewType::Elevation,
        );
        display_config.detail.layers = LayerDetail::StructuralOnly;
        let primitives = derive_elevation_view(
            &viewpoint,
            &model_view,
            &collector,
            &ElevationEngineOptions {
                display_config: Some(display_config),
                ..ElevationEngineOptions::default()
            },
        )
        .unwrap();
        let max_x = primitives
            .iter()
            .filter(|primitive| primitive.role == PrimitiveRole::ProjectedEdge)
            .filter(|primitive| {
                primitive
                    .sources
                    .as_slice()
                    .iter()
                    .any(|source| source.element == target_element)
            })
            .filter_map(|primitive| match &primitive.geometry {
                Geometry2d::Line { start, end } => Some(start.x_m.max(end.x_m)),
                _ => None,
            })
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(max_x.is_finite(), "visible wall outline");
        assert!((max_x - expected_max_x).abs() < 1e-4, "joined edge {max_x} != {expected_max_x}");
    }
}
