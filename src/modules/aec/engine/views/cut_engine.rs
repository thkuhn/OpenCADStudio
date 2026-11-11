//! 3D Cut and Section Derivation Engine for multi-layer walls, slabs, and openings.
//!
//! Evaluates straight and jogged section paths across the aggregated scene geometry,
//! generating associative 2D `DerivedPrimitive`s (cut boundaries, layer fills, material hatches)
//! with full provenance and feature keys.

use super::model_view::{DetailLevel, ModelView};
use super::provenance::{
    DerivedPrimitive, EditHandle, FeatureKey, Geometry2d, ModelEditAction, ModelParameter,
    PrimitiveId, PrimitiveRole, ResolvedStyle, Rgba8, SourceReference, SourceSet,
};
use super::scene_collector::{
    CollectedOpening, CollectedSlab, CollectedWall, SceneGeometryCollector,
};
use super::style_resolver::{
    is_draw_suppressed, is_slab_phase_visible, is_slab_layer_visible,
    is_wall_layer_visible, is_wall_phase_visible, resolve_layer_cut_style,
    color_to_rgba8_opt, resolve_opening_style, u32_to_rgba8,
    resolve_slab_layer_cut_style, ViewStyleContext,
};
use super::viewpoint::{SectionSegment, ViewKind, Viewpoint};
use super::{
    PaperMm, Point2, ValidationError, Vector2,
};
use crate::modules::aec::engine::library::StyleLibrary;
use crate::modules::aec::engine::opening_style::{project_division_sized, OpeningLookRole};
use crate::modules::aec::engine::openings::OpeningKind;
use crate::modules::aec::engine::plan_view::DisplayConfig;
use crate::modules::aec::engine::section_profile::{resolve_section, ResolvedSection};

const EPS: f64 = 1e-6;

/// Configuration and styles resolved for the cut generation.
#[derive(Clone, Debug)]
pub struct CutEngineOptions {
    pub structural_only: bool,
    pub include_openings: bool,
    pub detail_level: DetailLevel,
    pub emit_hidden_lines: bool,
    pub display_config: Option<DisplayConfig>,
    pub style_library: Option<StyleLibrary>,
}

impl Default for CutEngineOptions {
    fn default() -> Self {
        Self {
            structural_only: false,
            include_openings: true,
            detail_level: DetailLevel::Fine,
            emit_hidden_lines: false,
            display_config: None,
            style_library: None,
        }
    }
}

/// Derives 2D cut primitives from a Viewpoint and ModelView across the scene collector.
pub fn derive_section_view(
    viewpoint: &Viewpoint,
    model_view: &ModelView,
    scene: &SceneGeometryCollector,
    options: &CutEngineOptions,
) -> Result<Vec<DerivedPrimitive>, ValidationError> {
    viewpoint.validate()?;
    model_view.validate()?;

    let section_path = match &viewpoint.kind {
        ViewKind::Section { path, .. } => path,
        _ => {
            return Err(ValidationError(
                "viewpoint kind is not a section".to_string(),
            ))
        }
    };

    let segments = section_path.segments();
    let mut primitives = Vec::new();
    let mut primitive_counter: u32 = 0;

    let z_min = viewpoint.z_range.z_min.get();
    let z_max = viewpoint.z_range.z_max.get();

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
        true,
    );

    // Cumulative horizontal distance along the unfolded section path
    let mut accum_s = 0.0;

    for segment in &segments {
        let seg_len = segment.length();
        if seg_len < EPS {
            continue;
        }

        // 1. Cut walls intersecting this segment
        for wall in &scene.walls {
            if model_view.excluded_elements.contains(&wall.element_ref) {
                continue;
            }
            if wall.top_z < z_min || wall.base_z > z_max {
                continue;
            }

            cut_wall_with_segment(
                wall,
                segment,
                accum_s,
                model_view,
                options,
                &ctx,
                &mut primitives,
                &mut primitive_counter,
            )?;
        }

        // 2. Cut slabs intersecting this segment
        for slab in &scene.slabs {
            if model_view.excluded_elements.contains(&slab.element_ref) {
                continue;
            }
            let (slab_z_min, slab_z_max) = slab.z_range();
            if slab_z_max < z_min || slab_z_min > z_max {
                continue;
            }

            cut_slab_with_segment(
                slab,
                segment,
                accum_s,
                model_view,
                options,
                &ctx,
                &mut primitives,
                &mut primitive_counter,
            )?;
        }

        // Advance unfolded path parameter
        accum_s += seg_len;
    }

    // 3. Project visible background elements (walls, slabs, openings) behind the section line
    let forward_depth_max = match &viewpoint.kind {
        ViewKind::Section { far_depth, .. } => far_depth.map(|d| d.get()).unwrap_or(100.0),
        _ => 100.0,
    };

    let elev_opts = crate::modules::aec::engine::views::elevation_engine::ElevationEngineOptions {
        show_hidden_edges: options.emit_hidden_lines,
        include_opening_symbols: options.include_openings,
        detail_level: options.detail_level,
        display_config: options.display_config.clone(),
        style_library: options.style_library.clone(),
    };

    let wall_footprints = super::floorplan_engine::solve_wall_layer_footprints(scene);
    for (wall_index, wall) in scene.walls.iter().enumerate() {
        if model_view.excluded_elements.contains(&wall.element_ref) {
            continue;
        }
        if !is_wall_phase_visible(&ctx, wall) || is_draw_suppressed(&ctx, &wall.element_ref, None) {
            continue;
        }
        if wall.top_z < z_min || wall.base_z > z_max {
            continue;
        }
        let _ = crate::modules::aec::engine::views::elevation_engine::project_wall_elevation_with_footprints(
            wall,
            viewpoint,
            0.05,
            forward_depth_max,
            model_view,
            &elev_opts,
            &ctx,
            wall_footprints.get(wall_index).map(Vec::as_slice),
            &mut primitives,
            &mut primitive_counter,
        );
    }

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
        let _ = crate::modules::aec::engine::views::elevation_engine::project_slab_elevation(
            slab,
            viewpoint,
            0.05,
            forward_depth_max,
            model_view,
            &elev_opts,
            &ctx,
            &mut primitives,
            &mut primitive_counter,
        );
    }

    // 4. Deduplicate coincident cut boundary lines
    deduplicate_cut_boundaries(&mut primitives);

    // 5. Resolve all contributors in the shared section paper frame.
    let mut primitives = super::frame::resolve_view_frame(
        primitives,
        &super::silhouette::SilhouetteOptions {
            emit_hidden_lines: options.emit_hidden_lines,
            max_depth_distance: Some(forward_depth_max),
            hidden_layer: Some("AEC_SECTION_HIDDEN".to_string()),
        },
    )?;

    primitives.retain(|p| p.style.layer_name.as_deref() != Some(super::silhouette::TRANSIENT_OCCLUDER_LAYER));

    Ok(primitives)
}

/// Deduplicates identical cut boundary line segments arising from adjacent wall layers or touching elements.
fn deduplicate_cut_boundaries(primitives: &mut Vec<DerivedPrimitive>) {
    let mut i = 0;
    while i < primitives.len() {
        if primitives[i].role != PrimitiveRole::CutBoundary {
            i += 1;
            continue;
        }

        let (a0, a1) = match &primitives[i].geometry {
            Geometry2d::Line { start, end } => (*start, *end),
            _ => {
                i += 1;
                continue;
            }
        };

        let mut j = i + 1;
        while j < primitives.len() {
            if primitives[j].role != PrimitiveRole::CutBoundary {
                j += 1;
                continue;
            }

            let (b0, b1) = match &primitives[j].geometry {
                Geometry2d::Line { start, end } => (*start, *end),
                _ => {
                    j += 1;
                    continue;
                }
            };

            let same_dir = (a0.x_m - b0.x_m).hypot(a0.y_m - b0.y_m) < 1e-4
                && (a1.x_m - b1.x_m).hypot(a1.y_m - b1.y_m) < 1e-4;
            let opp_dir = (a0.x_m - b1.x_m).hypot(a0.y_m - b1.y_m) < 1e-4
                && (a1.x_m - b0.x_m).hypot(a1.y_m - b0.y_m) < 1e-4;

            if same_dir || opp_dir {
                let extra_sources = primitives[j].sources.as_slice().to_vec();
                let mut merged = primitives[i].sources.as_slice().to_vec();
                merged.extend(extra_sources);
                if let Ok(new_sources) = SourceSet::new(merged) {
                    primitives[i].sources = new_sources;
                }
                let extra_handles = primitives[j].edit_handles.clone();
                for eh in extra_handles {
                    if !primitives[i].edit_handles.contains(&eh) {
                        primitives[i].edit_handles.push(eh);
                    }
                }
                primitives.swap_remove(j);
            } else {
                j += 1;
            }
        }

        i += 1;
    }
}

/// Cuts a multi-layer wall against a single section segment.
fn cut_wall_with_segment(
    wall: &CollectedWall,
    segment: &SectionSegment,
    accum_s: f64,
    model_view: &ModelView,
    options: &CutEngineOptions,
    ctx: &ViewStyleContext,
    primitives: &mut Vec<DerivedPrimitive>,
    counter: &mut u32,
) -> Result<(), ValidationError> {
    if !is_wall_phase_visible(ctx, wall) {
        return Ok(());
    }
    if wall.axis_points.len() < 2 {
        return Ok(());
    }

    let seg_len = segment.length();
    if seg_len < EPS {
        return Ok(());
    }

    let layer_offsets = wall.layer_offsets();
    let wall_offset_min = layer_offsets
        .iter()
        .map(|(offset, _)| *offset)
        .fold(f64::INFINITY, f64::min);
    let wall_offset_max = layer_offsets
        .iter()
        .map(|(offset, thickness)| offset + thickness)
        .fold(f64::NEG_INFINITY, f64::max);

    // Iterate over wall baseline segments
    for (axis_segment_index, window) in wall.axis_points.windows(2).enumerate() {
        let w_p0 = window[0];
        let w_p1 = window[1];
        let w_vec = Vector2::new(w_p1.x_m - w_p0.x_m, w_p1.y_m - w_p0.y_m);
        let w_len = w_vec.length();
        if w_len < EPS {
            continue;
        }
        let w_dir = w_vec.normalized()?;
        let w_norm = Vector2::new(-w_dir.y, w_dir.x);
        let mut opening_details_emitted = false;

        // Process each wall layer
        for (i, layer) in wall.layers.iter().enumerate() {
            if !is_wall_layer_visible(ctx, wall, i) {
                continue;
            }

            let (mut boundary_style_opt, mut fill_style_opt) = resolve_layer_cut_style(ctx, wall, i);
            if options.structural_only && !layer.is_structural {
                boundary_style_opt = None;
                fill_style_opt = None;
            }

            let (layer_offset, layer_thick) = layer_offsets[i];
            let off_inner = layer_offset;
            let off_outer = layer_offset + layer_thick;

            // Compute 4 corners of layer band in world coordinates
            let c0 = Point2::new(
                w_p0.x_m + w_norm.x * off_inner,
                w_p0.y_m + w_norm.y * off_inner,
            );
            let c1 = Point2::new(
                w_p1.x_m + w_norm.x * off_inner,
                w_p1.y_m + w_norm.y * off_inner,
            );
            let c2 = Point2::new(
                w_p1.x_m + w_norm.x * off_outer,
                w_p1.y_m + w_norm.y * off_outer,
            );
            let c3 = Point2::new(
                w_p0.x_m + w_norm.x * off_outer,
                w_p0.y_m + w_norm.y * off_outer,
            );
            let layer_quad = [c0, c1, c2, c3];

            // Intersect section segment with layer quad
            let intersections = intersect_segment_with_polygon(segment, &layer_quad);
            if intersections.len() < 2 {
                continue;
            }

            let s_start = accum_s + intersections[0].0;
            let s_end = accum_s + intersections[1].0;
            let (u_min, u_max) = if s_start <= s_end {
                (s_start, s_end)
            } else {
                (s_end, s_start)
            };

            let z_bottom = wall.base_z + layer.bottom_offset;
            let z_top = wall.top_z + layer.top_offset;

            // Compute distance along wall baseline from the cut intersection point
            let hit_t = (intersections[0].0 + intersections[1].0) * 0.5;
            let hit_pt = Point2::new(
                segment.start.x_m + (segment.end.x_m - segment.start.x_m) * (hit_t / seg_len),
                segment.start.y_m + (segment.end.y_m - segment.start.y_m) * (hit_t / seg_len),
            );
            let segment_s = (hit_pt.x_m - w_p0.x_m) * w_dir.x
                + (hit_pt.y_m - w_p0.y_m) * w_dir.y;
            let wall_s_prefix: f64 = wall.axis_points[..axis_segment_index]
                .windows(2)
                .map(|pair| {
                    (pair[1].x_m - pair[0].x_m).hypot(pair[1].y_m - pair[0].y_m)
                })
                .sum();
            let s_along_wall = wall_s_prefix + segment_s;

            // Check if cut passes through any opening in the wall
            let intersecting_opening = wall.openings.iter().find(|op| {
                let (op_s0, op_s1) = op.axis_span_local;
                s_along_wall >= op_s0 - EPS && s_along_wall <= op_s1 + EPS
            });

            let mut z_intervals = vec![(z_bottom, z_top)];

            if let Some(op) = intersecting_opening {
                let style = resolve_opening_style(ctx, op);
                let grid = project_division_sized(
                    op.division_override.as_ref().unwrap_or(&style.grid),
                    op.width,
                    op.height,
                );
                let (op_s0, _) = op.axis_span_local;
                let local_x = s_along_wall - op_s0;
                let section_dir = Vector2::new(
                    (segment.end.x_m - segment.start.x_m) / seg_len,
                    (segment.end.y_m - segment.start.y_m) / seg_len,
                );
                let wall_axis_point = Point2::new(
                    w_p0.x_m + w_dir.x * segment_s,
                    w_p0.y_m + w_dir.y * segment_s,
                );
                let project_wall_offset = |offset: f64| {
                    let point = Point2::new(
                        wall_axis_point.x_m + w_norm.x * offset,
                        wall_axis_point.y_m + w_norm.y * offset,
                    );
                    accum_s
                        + (point.x_m - segment.start.x_m) * section_dir.x
                        + (point.y_m - segment.start.y_m) * section_dir.y
                };
                let exterior_u = project_wall_offset(wall_offset_max);
                let interior_u = project_wall_offset(wall_offset_min);
                let wall_u_min = exterior_u.min(interior_u);
                let wall_u_max = exterior_u.max(interior_u);
                let opening_center_u = project_wall_offset(op.cross_axis_offset);
                let cell_intervals: Vec<(f64, f64)> =
                    super::super::elevation_cut::opening_cell_vertical_intervals_at_x(
                        op.width,
                        op.height,
                        op.sill_height,
                        op.kind,
                        op.shape,
                        op.spring_height,
                        &grid,
                        local_x,
                    )
                    .into_iter()
                    .map(|(bottom, top)| (wall.base_z + bottom, wall.base_z + top))
                    .collect();
                z_intervals = subtract_vertical_intervals(&z_intervals, &cell_intervals);

                for (cell_sill, cell_head) in &cell_intervals {
                    emit_opening_void_occluder(
                        op,
                        wall,
                        u_min,
                        u_max,
                        *cell_sill,
                        *cell_head,
                        model_view,
                        primitives,
                        counter,
                    )?;
                }

                let has_door_cell = grid
                    .layout(op.width, op.height, op.kind)
                    .is_ok_and(|cells| {
                        cells.iter().any(|cell| {
                            cell.cell.kind
                                == crate::modules::aec::engine::opening_style::OpeningCellKind::DoorLeaf
                                && local_x >= cell.left + op.width * 0.5 - EPS
                                && local_x <= cell.right + op.width * 0.5 + EPS
                        })
                    });
                if options.include_openings
                    && (op.kind == OpeningKind::Window || op.kind == OpeningKind::Door)
                    && !cell_intervals.is_empty()
                    && !opening_details_emitted
                {
                    emit_opening_cut_details(
                        op,
                        wall,
                        &cell_intervals,
                        &style.parts,
                        has_door_cell,
                        opening_center_u,
                        exterior_u,
                        interior_u,
                        wall_u_min,
                        wall_u_max,
                        model_view,
                        ctx,
                        primitives,
                        counter,
                    )?;
                    opening_details_emitted = true;
                }
            }

            // Emit CutFill and CutBoundary for each remaining vertical interval
            for (zb, zt) in z_intervals {
                if zt <= zb + EPS {
                    continue;
                }

                let poly_points = vec![
                    Point2::new(u_min, zb),
                    Point2::new(u_max, zb),
                    Point2::new(u_max, zt),
                    Point2::new(u_min, zt),
                ];

                let source_ref = SourceReference {
                    element: wall.element_ref,
                    xref_source: wall.xref_source.clone(),
                    component_id: Some(layer.component_id),
                    feature: Some(FeatureKey::LayerFace),
                    display_component_id: None,
                };

                // 1. Cut fill (Hatch / Solid)
                if let Some(fill_style) = fill_style_opt.as_ref() {
                    *counter += 1;
                    primitives.push(DerivedPrimitive {
                        id: PrimitiveId::new(model_view.id, 1, *counter),
                        role: PrimitiveRole::CutFill,
                        geometry: Geometry2d::Region {
                            outer: poly_points,
                            holes: vec![],
                        },
                        sources: SourceSet::new(vec![source_ref.clone()])?,
                        depth: super::DepthInterval::cut_plane(),
                        style: fill_style.clone(),
                        edit_handles: vec![],
                    });
                } else {
                    // When hatches are disabled, emit a transient CutFill occluder so background lines
                    // are still properly occluded by the cut wall. This is cleaned up after silhouette processing.
                    *counter += 1;
                    primitives.push(DerivedPrimitive {
                        id: PrimitiveId::new(model_view.id, 1, *counter),
                        role: PrimitiveRole::CutFill,
                        geometry: Geometry2d::Region {
                            outer: poly_points,
                            holes: vec![],
                        },
                        sources: SourceSet::new(vec![source_ref.clone()])?,
                        depth: super::DepthInterval::cut_plane(),
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

                // 2. Cut boundary lines
                if let Some(boundary_style) = boundary_style_opt.as_ref() {
                    // Wall top edge
                    let mut top_source = source_ref.clone();
                    top_source.feature = Some(FeatureKey::WallTop);
                    *counter += 1;
                    let top_anchor = Point2::new((u_min + u_max) * 0.5, zt);
                    primitives.push(DerivedPrimitive {
                        id: PrimitiveId::new(model_view.id, 1, *counter),
                        role: PrimitiveRole::CutBoundary,
                        geometry: Geometry2d::Line {
                            start: Point2::new(u_min, zt),
                            end: Point2::new(u_max, zt),
                        },
                        sources: SourceSet::new(vec![top_source.clone()])?,
                        depth: super::DepthInterval::cut_plane(),
                        style: boundary_style.clone(),
                        edit_handles: vec![EditHandle {
                            id: format!("wall_top_{}", counter),
                            position: top_anchor,
                            action: ModelEditAction::ChangeParameter(ModelParameter::WallTopOffset),
                            direction_hint: Vector2::new(0.0, 1.0),
                            target_feature: Some(FeatureKey::WallTop),
                            source: top_source,
                        }],
                    });

                    // Wall bottom edge
                    let mut bot_source = source_ref.clone();
                    bot_source.feature = Some(FeatureKey::WallBase);
                    *counter += 1;
                    primitives.push(DerivedPrimitive {
                        id: PrimitiveId::new(model_view.id, 1, *counter),
                        role: PrimitiveRole::CutBoundary,
                        geometry: Geometry2d::Line {
                            start: Point2::new(u_min, zb),
                            end: Point2::new(u_max, zb),
                        },
                        sources: SourceSet::new(vec![bot_source])?,
                        depth: super::DepthInterval::cut_plane(),
                        style: boundary_style.clone(),
                        edit_handles: vec![],
                    });

                    // Wall left boundary edge (jamb)
                    let mut left_source = source_ref.clone();
                    left_source.feature = Some(FeatureKey::LayerFace);
                    *counter += 1;
                    primitives.push(DerivedPrimitive {
                        id: PrimitiveId::new(model_view.id, 1, *counter),
                        role: PrimitiveRole::CutBoundary,
                        geometry: Geometry2d::Line {
                            start: Point2::new(u_min, zb),
                            end: Point2::new(u_min, zt),
                        },
                        sources: SourceSet::new(vec![left_source])?,
                        depth: super::DepthInterval::cut_plane(),
                        style: boundary_style.clone(),
                        edit_handles: vec![],
                    });

                    // Wall right boundary edge (jamb)
                    let mut right_source = source_ref.clone();
                    right_source.feature = Some(FeatureKey::LayerFace);
                    *counter += 1;
                    primitives.push(DerivedPrimitive {
                        id: PrimitiveId::new(model_view.id, 1, *counter),
                        role: PrimitiveRole::CutBoundary,
                        geometry: Geometry2d::Line {
                            start: Point2::new(u_max, zb),
                            end: Point2::new(u_max, zt),
                        },
                        sources: SourceSet::new(vec![right_source])?,
                        depth: super::DepthInterval::cut_plane(),
                        style: boundary_style.clone(),
                        edit_handles: vec![],
                    });
                }
            }
        }
    }

    Ok(())
}

/// Opaque sheet in the cut void. The wall fill stops at the opening, so without
/// this sheet every background edge and opening symbol shows through the glass.
fn emit_opening_void_occluder(
    op: &CollectedOpening,
    wall: &CollectedWall,
    u_min: f64,
    u_max: f64,
    z_sill: f64,
    z_head: f64,
    model_view: &ModelView,
    primitives: &mut Vec<DerivedPrimitive>,
    counter: &mut u32,
) -> Result<(), ValidationError> {
    if u_max - u_min < EPS || z_head - z_sill < EPS {
        return Ok(());
    }
    *counter += 1;
    primitives.push(DerivedPrimitive {
        id: PrimitiveId::new(model_view.id, 1, *counter),
        role: PrimitiveRole::SurfaceFill,
        geometry: Geometry2d::Region {
            outer: vec![
                Point2::new(u_min, z_sill),
                Point2::new(u_max, z_sill),
                Point2::new(u_max, z_head),
                Point2::new(u_min, z_head),
            ],
            holes: vec![],
        },
        sources: SourceSet::new(vec![SourceReference {
            element: op.element_ref,
            xref_source: wall.xref_source.clone(),
            component_id: None,
            feature: Some(FeatureKey::OpeningSill),
            display_component_id: None,
        }])?,
        // Just behind the cut plane, in front of every background projection.
        depth: super::DepthInterval::new(1.0e-4, 1.0e-4)?,
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
    Ok(())
}

fn subtract_vertical_intervals(
    source: &[(f64, f64)],
    cuts: &[(f64, f64)],
) -> Vec<(f64, f64)> {
    let mut cuts = cuts.to_vec();
    cuts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut result = Vec::new();
    for &(bottom, top) in source {
        let mut remaining = vec![(bottom, top)];
        for &(cut_bottom, cut_top) in &cuts {
            let mut next = Vec::new();
            for (low, high) in remaining {
                if cut_top <= low + EPS || cut_bottom >= high - EPS {
                    next.push((low, high));
                    continue;
                }
                if cut_bottom > low + EPS {
                    next.push((low, cut_bottom.min(high)));
                }
                if cut_top < high - EPS {
                    next.push((cut_top.max(low), high));
                }
            }
            remaining = next;
        }
        result.extend(
            remaining
                .into_iter()
                .filter(|(low, high)| *high > *low + EPS),
        );
    }
    result
}

fn opening_section_style() -> ResolvedStyle {
    ResolvedStyle {
        line_weight_mm: PaperMm::new(0.18).unwrap(),
        line_color: None,
        line_type: None,
        fill_color: None,
        hatch_pattern: None,
        hatch_scale: None,
        hatch_angle_rad: None,
        layer_name: Some("AEC_OPENING_SECTION".to_string()),
    }
}

fn resolve_opening_component_cut_style(
    ctx: &ViewStyleContext,
    op: &CollectedOpening,
    role: OpeningLookRole,
) -> ResolvedStyle {
    let mut resolved = opening_section_style();
    let opening_style = resolve_opening_style(ctx, op);
    let look = opening_style.looks.get(role);
    let material = opening_style
        .material_id_for_look(role)
        .and_then(|material_id| {
            ctx.style_library.and_then(|library| {
                library.materials.iter().find(|material| {
                    material.id == material_id || material.name == material_id
                })
            })
        });

    resolved.line_color = match look.line_color.as_ref() {
        Some(color) => color_to_rgba8_opt(color),
        None => material.map(|material| u32_to_rgba8(material.line_color)),
    };
    resolved.line_type = look
        .line_type
        .as_deref()
        .filter(|line_type| !line_type.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            material.and_then(|material| {
                (!material.line_type.trim().is_empty()).then(|| material.line_type.clone())
            })
        });

    if let Some(material) = material {
        resolved.hatch_pattern = (!material.hatch_pattern.trim().is_empty())
            .then(|| material.hatch_pattern.clone());
        resolved.hatch_scale = Some(material.hatch_scale);
        resolved.hatch_angle_rad = Some(material.hatch_angle.to_radians());
        resolved.fill_color = material
            .hatch_color
            .as_ref()
            .and_then(color_to_rgba8_opt)
            .or_else(|| Some(u32_to_rgba8(material.line_color)));
    }
    resolved
}

fn emit_opening_section_region(
    op: &CollectedOpening,
    wall: &CollectedWall,
    model_view: &ModelView,
    primitives: &mut Vec<DerivedPrimitive>,
    counter: &mut u32,
    bounds: (f64, f64, f64, f64),
    role: PrimitiveRole,
    feature: FeatureKey,
    style: ResolvedStyle,
) -> Result<(), ValidationError> {
    let (u_min, u_max, z_min, z_max) = bounds;
    if u_max - u_min <= EPS || z_max - z_min <= EPS {
        return Ok(());
    }
    emit_opening_section_outline(
        op,
        wall,
        model_view,
        primitives,
        counter,
        vec![
            Point2::new(u_min, z_min),
            Point2::new(u_max, z_min),
            Point2::new(u_max, z_max),
            Point2::new(u_min, z_max),
        ],
        role,
        feature,
        style,
    )
}

fn emit_opening_section_profile(
    op: &CollectedOpening,
    wall: &CollectedWall,
    model_view: &ModelView,
    primitives: &mut Vec<DerivedPrimitive>,
    counter: &mut u32,
    section: &ResolvedSection,
    origin_u: f64,
    origin_z: f64,
    inward: f64,
    clip: (f64, f64, f64, f64),
    role: PrimitiveRole,
    feature: FeatureKey,
    style: ResolvedStyle,
) -> Result<(), ValidationError> {
    let mut outline: Vec<Point2> = section
        .outline
        .iter()
        .map(|(face, depth)| Point2::new(origin_u + inward * depth, origin_z + face))
        .collect();
    if outline.len() > 1 && outline.first() == outline.last() {
        outline.pop();
    }
    let outline = clip_polygon_to_bounds(outline, clip);
    emit_opening_section_outline(
        op,
        wall,
        model_view,
        primitives,
        counter,
        outline,
        role,
        feature,
        style,
    )
}

fn resolve_opening_section(
    library: Option<&StyleLibrary>,
    profile_id: Option<&str>,
    fallback_face: f64,
    fallback_depth: f64,
) -> ResolvedSection {
    let profile = profile_id.and_then(|id| library?.find_section_profile(id));
    resolve_section(profile, fallback_face, fallback_depth)
}

fn emit_opening_section_outline(
    op: &CollectedOpening,
    wall: &CollectedWall,
    model_view: &ModelView,
    primitives: &mut Vec<DerivedPrimitive>,
    counter: &mut u32,
    outline: Vec<Point2>,
    role: PrimitiveRole,
    feature: FeatureKey,
    style: ResolvedStyle,
) -> Result<(), ValidationError> {
    if outline.len() < 3 {
        return Ok(());
    }
    *counter += 1;
    primitives.push(DerivedPrimitive {
        id: PrimitiveId::new(model_view.id, 1, *counter),
        role,
        geometry: Geometry2d::Region {
            outer: outline,
            holes: vec![],
        },
        sources: SourceSet::new(vec![SourceReference {
            element: op.element_ref,
            xref_source: wall.xref_source.clone(),
            component_id: None,
            feature: Some(feature),
            display_component_id: None,
        }])?,
        depth: super::DepthInterval::cut_plane(),
        style,
        edit_handles: vec![],
    });
    Ok(())
}

fn clip_polygon_to_bounds(
    mut points: Vec<Point2>,
    (u_min, u_max, z_min, z_max): (f64, f64, f64, f64),
) -> Vec<Point2> {
    for (axis, bound, keep_greater) in [
        (0, u_min, true),
        (0, u_max, false),
        (1, z_min, true),
        (1, z_max, false),
    ] {
        if points.is_empty() {
            break;
        }
        let value = |point: Point2| if axis == 0 { point.x_m } else { point.y_m };
        let inside = |point: Point2| {
            if keep_greater {
                value(point) >= bound - EPS
            } else {
                value(point) <= bound + EPS
            }
        };
        let mut clipped = Vec::new();
        let mut previous = *points.last().unwrap();
        for current in points.iter().copied() {
            let previous_inside = inside(previous);
            let current_inside = inside(current);
            if previous_inside != current_inside {
                let delta = value(current) - value(previous);
                if delta.abs() > EPS {
                    let t = (bound - value(previous)) / delta;
                    clipped.push(Point2::new(
                        previous.x_m + (current.x_m - previous.x_m) * t,
                        previous.y_m + (current.y_m - previous.y_m) * t,
                    ));
                }
            }
            if current_inside {
                clipped.push(current);
            }
            previous = current;
        }
        points = clipped;
    }
    points
}

/// Emits section components from their absolute style dimensions.
fn emit_opening_cut_details(
    op: &CollectedOpening,
    wall: &CollectedWall,
    cell_intervals: &[(f64, f64)],
    parts: &crate::modules::aec::engine::opening_style::OpeningParts,
    has_door_cell: bool,
    opening_center_u: f64,
    exterior_u: f64,
    interior_u: f64,
    layer_u_min: f64,
    layer_u_max: f64,
    model_view: &ModelView,
    ctx: &ViewStyleContext,
    primitives: &mut Vec<DerivedPrimitive>,
    counter: &mut u32,
) -> Result<(), ValidationError> {
    let roles = super::style_resolver::resolve_opening_section_roles(ctx, op);
    let frame_style = resolve_opening_component_cut_style(ctx, op, OpeningLookRole::FixedFrame);
    let exterior_sill_style =
        resolve_opening_component_cut_style(ctx, op, OpeningLookRole::ExteriorSill);
    let interior_board_style =
        resolve_opening_component_cut_style(ctx, op, OpeningLookRole::InteriorBoard);
    let threshold_style = resolve_opening_component_cut_style(ctx, op, OpeningLookRole::Threshold);
    let shutter_box_style =
        resolve_opening_component_cut_style(ctx, op, OpeningLookRole::ShutterBox);
    let lintel_style = resolve_opening_component_cut_style(ctx, op, OpeningLookRole::Lintel);
    let wall_span = (layer_u_max - layer_u_min).max(0.0);
    let frame_center = opening_center_u + parts.fixed_frame.axis_offset;
    let frame_section = resolve_opening_section(
        ctx.style_library,
        parts.fixed_frame.profile_id.as_deref(),
        parts.fixed_frame.face_width,
        parts.fixed_frame.wall_depth,
    );
    let exterior_sill_section = resolve_opening_section(
        ctx.style_library,
        parts.exterior_sill.profile_id.as_deref(),
        parts.exterior_sill.thickness,
        parts.exterior_sill.thickness,
    );
    let interior_board_section = resolve_opening_section(
        ctx.style_library,
        parts.interior_board.profile_id.as_deref(),
        parts.interior_board.thickness,
        parts.interior_board.thickness,
    );
    let threshold_section = resolve_opening_section(
        ctx.style_library,
        parts.threshold.profile_id.as_deref(),
        parts.threshold.height,
        parts.threshold.width,
    );
    let shutter_section = resolve_opening_section(
        ctx.style_library,
        parts.shutter_box.profile_id.as_deref(),
        parts.shutter_box.height,
        parts.shutter_box.depth,
    );
    let lintel_section = resolve_opening_section(
        ctx.style_library,
        parts.lintel.profile_id.as_deref(),
        parts.lintel.height,
        wall_span,
    );
    let frame_depth = frame_section.depth.min(wall_span);
    let frame_face = frame_section.face.max(0.0);
    let sill_z = wall.base_z + op.sill_height;
    let head_z = sill_z + op.height;
    let inward = if interior_u >= exterior_u { 1.0 } else { -1.0 };

    for &(cell_sill, cell_head) in cell_intervals {
        let rail_height = frame_face.min((cell_head - cell_sill) * 0.5);
        let sill_region_start = primitives.len();
        if roles.frame && parts.fixed_frame.enabled && frame_face > EPS && frame_section.depth > EPS {
        emit_opening_section_profile(
            op,
            wall,
            model_view,
            primitives,
            counter,
            &frame_section,
            frame_center - inward * frame_section.depth * 0.5,
            cell_sill,
            inward,
            (layer_u_min, layer_u_max, cell_sill, cell_sill + rail_height),
            PrimitiveRole::CutBoundary,
            FeatureKey::OpeningSill,
            frame_style.clone(),
        )?;
        if cell_sill <= cell_intervals
            .iter()
            .map(|(sill, _)| *sill)
            .fold(f64::INFINITY, f64::min)
            + EPS
            && primitives.len() > sill_region_start
        {
            if let Some(primitive) = primitives.last_mut() {
                let source = SourceReference {
                    element: op.element_ref,
                    xref_source: wall.xref_source.clone(),
                    component_id: None,
                    feature: Some(FeatureKey::OpeningSill),
                    display_component_id: None,
                };
                primitive.edit_handles.push(EditHandle {
                    id: format!("opening_sill_{}", counter),
                    position: Point2::new(opening_center_u, cell_sill),
                    action: ModelEditAction::ChangeParameter(ModelParameter::OpeningSillHeight),
                    direction_hint: Vector2::new(0.0, 1.0),
                    target_feature: Some(FeatureKey::OpeningSill),
                    source,
                });
            }
        }
        emit_opening_section_profile(
            op,
            wall,
            model_view,
            primitives,
            counter,
            &frame_section,
            frame_center - frame_section.depth * 0.5,
            cell_head - frame_section.face,
            inward,
            (layer_u_min, layer_u_max, cell_head - rail_height, cell_head),
            PrimitiveRole::CutBoundary,
            FeatureKey::OpeningHead,
            frame_style.clone(),
        )?;
        let head_region_emitted = primitives.len() > sill_region_start + 1;
        if cell_head >= cell_intervals
            .iter()
            .map(|(_, head)| *head)
            .fold(f64::NEG_INFINITY, f64::max)
            - EPS
            && head_region_emitted
        {
            if let Some(primitive) = primitives.last_mut() {
                let source = SourceReference {
                    element: op.element_ref,
                    xref_source: wall.xref_source.clone(),
                    component_id: None,
                    feature: Some(FeatureKey::OpeningHead),
                    display_component_id: None,
                };
                primitive.edit_handles.push(EditHandle {
                    id: format!("opening_head_{}", counter),
                    position: Point2::new(opening_center_u, cell_head),
                    action: ModelEditAction::ChangeParameter(ModelParameter::OpeningHeadHeight),
                    direction_hint: Vector2::new(0.0, 1.0),
                    target_feature: Some(FeatureKey::OpeningHead),
                    source,
                });
            }
        }
        }

    if roles.exterior_sill
            && parts.exterior_sill.enabled
            && exterior_sill_section.face > EPS
            && exterior_sill_section.depth > EPS
            && !(op.kind == OpeningKind::Door && cell_sill <= sill_z + EPS)
        {
            let exterior_sill = &parts.exterior_sill;
            let frame_outer = frame_center - inward * frame_depth * 0.5;
            let requested_outer = exterior_u - inward * exterior_sill.overhang.max(0.0);
            let has_profile = exterior_sill
                .profile_id
                .as_deref()
                .and_then(|id| ctx.style_library?.find_section_profile(id))
                .is_some_and(|profile| profile.is_valid());
            if has_profile {
                let profile_outer = frame_outer - inward * exterior_sill_section.depth;
                emit_opening_section_profile(
                    op,
                    wall,
                    model_view,
                    primitives,
                    counter,
                    &exterior_sill_section,
                    profile_outer,
                    cell_sill,
                    inward,
                    (
                        requested_outer.min(profile_outer).min(frame_outer),
                        requested_outer.max(profile_outer).max(frame_outer),
                        cell_sill,
                        cell_sill + exterior_sill_section.face,
                    ),
                    PrimitiveRole::CutBoundary,
                    FeatureKey::OpeningSill,
                    exterior_sill_style.clone(),
                )?;
                if (profile_outer - requested_outer) * inward > EPS {
                    emit_opening_section_region(
                        op,
                        wall,
                        model_view,
                        primitives,
                        counter,
                        (
                            requested_outer.min(profile_outer),
                            requested_outer.max(profile_outer),
                            cell_sill,
                            cell_sill + exterior_sill_section.face,
                        ),
                        PrimitiveRole::CutBoundary,
                        FeatureKey::OpeningSill,
                        exterior_sill_style.clone(),
                    )?;
                }
            } else {
                emit_opening_section_region(
                    op,
                    wall,
                    model_view,
                    primitives,
                    counter,
                    (
                        requested_outer.min(frame_outer),
                        requested_outer.max(frame_outer),
                        cell_sill,
                        cell_sill + exterior_sill_section.face,
                    ),
                    PrimitiveRole::CutBoundary,
                    FeatureKey::OpeningSill,
                    exterior_sill_style.clone(),
                )?;
                if exterior_sill.upstand > 0.0 {
                    let (upstand_lo, upstand_hi) = crate::modules::aec::engine::opening_display::sill_upstand_interval(
                        frame_outer,
                        -inward,
                        exterior_sill_section.depth,
                        frame_depth,
                    );
                    emit_opening_section_region(
                        op,
                        wall,
                        model_view,
                        primitives,
                        counter,
                        (
                            upstand_lo,
                            upstand_hi,
                            cell_sill + exterior_sill_section.face,
                            cell_sill + exterior_sill_section.face + exterior_sill.upstand,
                        ),
                        PrimitiveRole::CutBoundary,
                        FeatureKey::OpeningSill,
                        exterior_sill_style.clone(),
                    )?;
                }
            }
        }

        if roles.interior_board
            && parts.interior_board.enabled
            && interior_board_section.face > EPS
            && interior_board_section.depth > EPS
            && !(op.kind == OpeningKind::Door && cell_sill <= sill_z + EPS)
        {
            let interior_board = &parts.interior_board;
            let frame_inner = frame_center + inward * frame_depth * 0.5;
            let requested_inner = interior_u + inward * interior_board.overhang.max(0.0);
            let has_profile = interior_board
                .profile_id
                .as_deref()
                .and_then(|id| ctx.style_library?.find_section_profile(id))
                .is_some_and(|profile| profile.is_valid());
            if has_profile {
                let profile_end = frame_inner + inward * interior_board_section.depth;
                emit_opening_section_profile(
                    op,
                    wall,
                    model_view,
                    primitives,
                    counter,
                    &interior_board_section,
                    frame_inner,
                    cell_sill,
                    inward,
                    (
                        frame_inner.min(profile_end).min(requested_inner),
                        frame_inner.max(profile_end).max(requested_inner),
                        cell_sill,
                        cell_sill + interior_board_section.face,
                    ),
                    PrimitiveRole::CutBoundary,
                    FeatureKey::OpeningSill,
                    interior_board_style.clone(),
                )?;
                if (requested_inner - profile_end) * inward > EPS {
                    emit_opening_section_region(
                        op,
                        wall,
                        model_view,
                        primitives,
                        counter,
                        (
                            profile_end.min(requested_inner),
                            profile_end.max(requested_inner),
                            cell_sill,
                            cell_sill + interior_board_section.face,
                        ),
                        PrimitiveRole::CutBoundary,
                        FeatureKey::OpeningSill,
                        interior_board_style.clone(),
                    )?;
                }
            } else {
                emit_opening_section_region(
                    op,
                    wall,
                    model_view,
                    primitives,
                    counter,
                    (
                        frame_inner.min(requested_inner),
                        frame_inner.max(requested_inner),
                        cell_sill,
                        cell_sill + interior_board_section.face,
                    ),
                    PrimitiveRole::CutBoundary,
                    FeatureKey::OpeningSill,
                    interior_board_style.clone(),
                )?;
                if interior_board.upstand > 0.0 {
                    let (upstand_lo, upstand_hi) = crate::modules::aec::engine::opening_display::sill_upstand_interval(
                        frame_inner,
                        inward,
                        interior_board_section.depth,
                        frame_depth,
                    );
                    emit_opening_section_region(
                        op,
                        wall,
                        model_view,
                        primitives,
                        counter,
                        (
                            upstand_lo,
                            upstand_hi,
                            cell_sill + interior_board_section.face,
                            cell_sill + interior_board_section.face + interior_board.upstand,
                        ),
                        PrimitiveRole::CutBoundary,
                        FeatureKey::OpeningSill,
                        interior_board_style.clone(),
                    )?;
                }
            }
        }
    }

    if roles.threshold
        && parts.threshold.enabled
        && has_door_cell
        && threshold_section.face > EPS
        && threshold_section.depth > EPS
    {
        let origin_u = opening_center_u - inward * threshold_section.depth * 0.5;
        emit_opening_section_profile(
            op,
            wall,
            model_view,
            primitives,
            counter,
            &threshold_section,
            origin_u,
            sill_z,
            inward,
            (layer_u_min, layer_u_max, sill_z, sill_z + threshold_section.face),
            PrimitiveRole::CutBoundary,
            FeatureKey::OpeningSill,
            threshold_style.clone(),
        )?;
    }

    let shutter = &parts.shutter_box;
    if roles.shutter_box
        && shutter.enabled
        && shutter_section.face > EPS
        && shutter_section.depth > EPS
    {
        let u0 = exterior_u + inward * shutter.installation_depth.max(0.0);
        let z0 = crate::modules::aec::engine::opening_display::accessory_bottom(
            head_z,
            shutter.seat_above_frame,
        );
        emit_opening_section_profile(
            op,
            wall,
            model_view,
            primitives,
            counter,
            &shutter_section,
            u0,
            z0,
            inward,
            (layer_u_min, layer_u_max, z0, z0 + shutter_section.face),
            PrimitiveRole::CutBoundary,
            FeatureKey::OpeningHead,
            shutter_box_style.clone(),
        )?;
    }

    if roles.lintel && parts.lintel.enabled && lintel_section.face > EPS && lintel_section.depth > EPS {
        let origin_u = (layer_u_min + layer_u_max) * 0.5 - inward * lintel_section.depth * 0.5;
        emit_opening_section_profile(
            op,
            wall,
            model_view,
            primitives,
            counter,
            &lintel_section,
            origin_u,
            head_z,
            inward,
            (layer_u_min, layer_u_max, head_z, head_z + lintel_section.face),
            PrimitiveRole::CutFill,
            FeatureKey::OpeningHead,
            lintel_style,
        )?;
    }
    Ok(())
}

/// Cuts a multi-layer slab against a single section segment.
fn cut_slab_with_segment(
    slab: &CollectedSlab,
    segment: &SectionSegment,
    accum_s: f64,
    model_view: &ModelView,
    options: &CutEngineOptions,
    ctx: &ViewStyleContext,
    primitives: &mut Vec<DerivedPrimitive>,
    counter: &mut u32,
) -> Result<(), ValidationError> {
    if !is_slab_phase_visible(ctx, slab) {
        return Ok(());
    }
    if slab.polygon.len() < 3 {
        return Ok(());
    }

    let intersections = intersect_segment_with_polygon(segment, &slab.polygon);
    if intersections.len() < 2 {
        return Ok(());
    }

    for (layer_idx, (layer, (z_top, z_bottom))) in
        slab.layers.iter().zip(slab.layer_z_ranges()).enumerate()
    {
        if !is_slab_layer_visible(ctx, slab, layer_idx) {
            continue;
        }

        let (mut boundary_style_opt, mut fill_style_opt) = resolve_slab_layer_cut_style(ctx, slab, layer_idx);
        if options.structural_only && !layer.is_structural {
            boundary_style_opt = None;
            fill_style_opt = None;
        }

        // For each entry/exit pair through the slab polygon
        for chunk in intersections.chunks_exact(2) {
            let u1 = accum_s + chunk[0].0;
            let u2 = accum_s + chunk[1].0;
            let (u_min, u_max) = if u1 <= u2 { (u1, u2) } else { (u2, u1) };

            let poly_points = vec![
                Point2::new(u_min, z_bottom),
                Point2::new(u_max, z_bottom),
                Point2::new(u_max, z_top),
                Point2::new(u_min, z_top),
            ];

            let source_ref = SourceReference {
                element: slab.element_ref,
                xref_source: slab.xref_source.clone(),
                component_id: Some(layer.component_id),
                feature: Some(FeatureKey::LayerFace),
                display_component_id: None,
            };

            // Slab fill
            if let Some(fill_style) = fill_style_opt.as_ref() {
                *counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 1, *counter),
                    role: PrimitiveRole::CutFill,
                    geometry: Geometry2d::Region {
                        outer: poly_points,
                        holes: vec![],
                    },
                    sources: SourceSet::new(vec![source_ref.clone()])?,
                    depth: super::DepthInterval::cut_plane(),
                    style: fill_style.clone(),
                    edit_handles: vec![],
                });
            } else {
                // When hatches are disabled, emit a transient CutFill occluder so background lines
                // are still properly occluded by the cut slab. This is cleaned up after silhouette processing.
                *counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 1, *counter),
                    role: PrimitiveRole::CutFill,
                    geometry: Geometry2d::Region {
                        outer: poly_points,
                        holes: vec![],
                    },
                    sources: SourceSet::new(vec![source_ref.clone()])?,
                    depth: super::DepthInterval::cut_plane(),
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

            // Slab top & bottom boundary lines
            if let Some(boundary_style) = boundary_style_opt.as_ref() {
                let mut top_source = source_ref.clone();
                top_source.feature = Some(FeatureKey::SlabTop);
                *counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 1, *counter),
                    role: PrimitiveRole::CutBoundary,
                    geometry: Geometry2d::Line {
                        start: Point2::new(u_min, z_top),
                        end: Point2::new(u_max, z_top),
                    },
                    sources: SourceSet::new(vec![top_source])?,
                    depth: super::DepthInterval::cut_plane(),
                    style: boundary_style.clone(),
                    edit_handles: vec![],
                });

                let mut bot_source = source_ref.clone();
                bot_source.feature = Some(FeatureKey::SlabBottom);
                *counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 1, *counter),
                    role: PrimitiveRole::CutBoundary,
                    geometry: Geometry2d::Line {
                        start: Point2::new(u_min, z_bottom),
                        end: Point2::new(u_max, z_bottom),
                    },
                    sources: SourceSet::new(vec![bot_source])?,
                    depth: super::DepthInterval::cut_plane(),
                    style: boundary_style.clone(),
                    edit_handles: vec![],
                });

                // Slab left boundary edge
                let mut left_source = source_ref.clone();
                left_source.feature = Some(FeatureKey::LayerFace);
                *counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 1, *counter),
                    role: PrimitiveRole::CutBoundary,
                    geometry: Geometry2d::Line {
                        start: Point2::new(u_min, z_bottom),
                        end: Point2::new(u_min, z_top),
                    },
                    sources: SourceSet::new(vec![left_source])?,
                    depth: super::DepthInterval::cut_plane(),
                    style: boundary_style.clone(),
                    edit_handles: vec![],
                });

                // Slab right boundary edge
                let mut right_source = source_ref;
                right_source.feature = Some(FeatureKey::LayerFace);
                *counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 1, *counter),
                    role: PrimitiveRole::CutBoundary,
                    geometry: Geometry2d::Line {
                        start: Point2::new(u_max, z_bottom),
                        end: Point2::new(u_max, z_top),
                    },
                    sources: SourceSet::new(vec![right_source])?,
                    depth: super::DepthInterval::cut_plane(),
                    style: boundary_style.clone(),
                    edit_handles: vec![],
                });
            }
        }
    }

    Ok(())
}

/// Computes intersections of a section segment with a 2D closed polygon.
/// Returns sorted `(t_on_segment, t_on_polygon_edge)` parameter pairs.
fn intersect_segment_with_polygon(
    segment: &SectionSegment,
    polygon: &[Point2],
) -> Vec<(f64, f64)> {
    let mut hits = Vec::new();
    let n = polygon.len();
    if n < 3 {
        return hits;
    }

    let p1 = segment.start;
    let p2 = segment.end;
    let seg_len = segment.length();
    if seg_len < EPS {
        return hits;
    }

    for i in 0..n {
        let p3 = polygon[i];
        let p4 = polygon[(i + 1) % n];

        if let Some((t_seg, t_edge)) = line_segment_intersection(p1, p2, p3, p4) {
            hits.push((t_seg * seg_len, t_edge));
        }
    }

    hits.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    hits.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-4);
    hits
}

/// 2D line segment intersection parameter computation.
/// Returns `Some((t1, t2))` where `t1, t2 ∈ [0, 1]` if segments intersect.
fn line_segment_intersection(
    p1: Point2,
    p2: Point2,
    p3: Point2,
    p4: Point2,
) -> Option<(f64, f64)> {
    let d1x = p2.x_m - p1.x_m;
    let d1y = p2.y_m - p1.y_m;
    let d2x = p4.x_m - p3.x_m;
    let d2y = p4.y_m - p3.y_m;

    let denom = d1x * d2y - d1y * d2x;
    if denom.abs() < 1e-10 {
        return None; // Parallel or collinear
    }

    let dx = p3.x_m - p1.x_m;
    let dy = p3.y_m - p1.y_m;

    let t1 = (dx * d2y - dy * d2x) / denom;
    let t2 = (dx * d1y - dy * d1x) / denom;

    if (-1e-5..=1.0 + 1e-5).contains(&t1) && (-1e-5..=1.0 + 1e-5).contains(&t2) {
        Some((t1.clamp(0.0, 1.0), t2.clamp(0.0, 1.0)))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::plan_view::PlanPhase;
    use crate::modules::aec::engine::views::{
        CollectedWallLayer, ComponentId, DetailPolicy, ElementId, ElementRef, Geometry2d, Metres,
        ModelId, ModelView, ModelViewId, Point2, Point3, PrimitiveRole, ProjectionMapping, Revision,
        ScaleDenominator, SectionPath, UpdatePolicy, Vector2, VerticalRange, ViewAxes, ViewKind,
        ViewOrigin, ViewProfileId, ViewRevisions, Viewpoint, ViewpointId,
    };
    use crate::modules::aec::engine::wall::WallJustification;
    use std::collections::BTreeSet;

    fn layer(material: &str, thickness: f64, axis_offset: f64, structural: bool) -> CollectedWallLayer {
        CollectedWallLayer {
            material: material.into(),
            thickness,
            function: if structural { "Structural".into() } else { "Finish".into() },
            axis_offset,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
            component_id: ComponentId::new(),
            is_structural: structural,
        }
    }

    #[test]
    fn section_cut_uses_heterogeneous_cell_heights() {
        use crate::modules::aec::engine::opening_style::{
            DivisionGrid, LeafOperation, OpeningCell, OpeningCellKind, OpeningStyle,
            construction_from_division,
        };
        use crate::modules::aec::engine::openings::{
            OpeningKind, OpeningReferenceSide, SwingSide,
        };
        use crate::modules::aec::engine::views::scene_collector::{
            CollectedOpening, CollectedWall,
        };
        use crate::modules::aec::engine::views::SectionSegment;

        let mut style = OpeningStyle::standard_window();
        style.style.id = "door_window_combo".into();
        style.grid = construction_from_division(
            &DivisionGrid {
                vertical: vec![0.5, 0.5],
                cells: vec![
                    OpeningCell {
                        kind: OpeningCellKind::DoorLeaf,
                        hinge: Some(LeafOperation::left()),
                        sill: 0.0,
                    },
                    OpeningCell {
                        kind: OpeningCellKind::Glass,
                        hinge: None,
                        sill: 1.0,
                    },
                ],
                ..DivisionGrid::default()
            },
            1.8,
            2.2,
        );
        let mut library = StyleLibrary::default();
        library.opening_styles.push(style.clone());

        let opening_element = ElementRef::new(ModelId::new(), ElementId::new());
        let opening = CollectedOpening {
            handle: acadrust::Handle::new(20),
            element_ref: opening_element,
            xref_source: None,
            host_wall_handle: acadrust::Handle::new(10),
            kind: OpeningKind::Window,
            style_id: Some(style.style.id.clone()),
            division_override: None,
            distance_along_axis: 5.0,
            width: 1.8,
            height: 2.2,
            sill_height: 0.0,
            spring_height: 0.0,
            shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
            hinge: LeafOperation::left(),
            swing_side: SwingSide::Exterior,
            reference_side: OpeningReferenceSide::Center,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(5.0, 0.0, 1.1),
            axis_span_local: (4.1, 5.9),
        };
        let wall = CollectedWall {
            handle: acadrust::Handle::new(10),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "section_wall".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
            layers: vec![layer("Core", 0.30, -0.15, true)],
            total_thickness: 0.30,
            justification: WallJustification::Center,
            openings: vec![opening],
            phase: PlanPhase::New,
        };
        let model_view = ModelView {
            id: ModelViewId::new(),
            name: "Door-window cut".into(),
            viewpoint_id: ViewpointId::new(),
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };
        let ctx = ViewStyleContext::new(&model_view, None, Some(&library), true);
        let cut_at = |x| {
            let segment = SectionSegment::new(Point2::new(x, -1.0), Point2::new(x, 1.0), false);
            let mut primitives = Vec::new();
            cut_wall_with_segment(
                &wall,
                &segment,
                0.0,
                &model_view,
                &CutEngineOptions::default(),
                &ctx,
                &mut primitives,
                &mut 0,
            )
            .unwrap();
            primitives
                .iter()
                .filter(|primitive| primitive.role == PrimitiveRole::CutFill)
                .filter_map(|primitive| match &primitive.geometry {
                    Geometry2d::Region { outer, .. } => Some((
                        outer.iter().map(|point| point.y_m).fold(f64::INFINITY, f64::min),
                        outer.iter().map(|point| point.y_m).fold(f64::NEG_INFINITY, f64::max),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let door_section = cut_at(4.5);
        let window_section = cut_at(5.5);
        assert!(door_section.iter().all(|(bottom, _)| *bottom >= 2.2 - EPS));
        assert!(window_section.iter().any(|(bottom, top)| {
            bottom.abs() < EPS && (*top - 1.0).abs() < EPS
        }));
        assert!(window_section.iter().any(|(bottom, _)| *bottom >= 2.2 - EPS));
    }

    #[test]
    fn section_layers_keep_axis_offset_instead_of_restacking_from_the_axis() {
        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(CollectedWall {
            handle: acadrust::Handle::new(7),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "Asymmetric".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(5.0, -4.0), Point2::new(5.0, 4.0)],
            layers: vec![
                layer("Plaster", 0.02, -0.24, false),
                layer("Concrete", 0.20, -0.08, true),
                layer("Plaster", 0.02, 0.16, false),
            ],
            total_thickness: 0.42,
            justification: WallJustification::Exterior,
            openings: vec![],
            phase: PlanPhase::New,
        });
        let viewpoint = Viewpoint {
            id: ViewpointId::new(),
            name: "Schnitt".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::section(Vector2::new(1.0, 0.0)).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
                far_depth: None,
            },
            z_range: VerticalRange::new(Metres::new(-1.0).unwrap(), Metres::new(5.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };
        let model_view = ModelView {
            id: ModelViewId::new(),
            name: "Schnitt".into(),
            viewpoint_id: viewpoint.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let primitives = derive_section_view(
            &viewpoint,
            &model_view,
            &collector,
            &CutEngineOptions::default(),
        )
        .unwrap();
        let mut spans: Vec<(f64, f64)> = primitives
            .iter()
            .filter(|primitive| primitive.role == PrimitiveRole::CutFill)
            .filter_map(|primitive| match &primitive.geometry {
                Geometry2d::Region { outer, .. } => {
                    let min_x = outer.iter().map(|point| point.x_m).fold(f64::INFINITY, f64::min);
                    let max_x = outer.iter().map(|point| point.x_m).fold(f64::NEG_INFINITY, f64::max);
                    Some((min_x, max_x))
                }
                _ => None,
            })
            .collect();
        spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        assert_eq!(spans.len(), 3, "each visible layer keeps its own station");
        assert!((spans[0].0 - 4.82).abs() < 1e-4 && (spans[0].1 - 4.84).abs() < 1e-4, "{spans:?}");
        assert!((spans[1].0 - 4.88).abs() < 1e-4 && (spans[1].1 - 5.08).abs() < 1e-4, "{spans:?}");
        assert!((spans[2].0 - 5.22).abs() < 1e-4 && (spans[2].1 - 5.24).abs() < 1e-4, "{spans:?}");
    }

    #[test]
    fn opening_section_parts_keep_absolute_sizes_and_lintel_material() {
        use crate::modules::aec::engine::library::StyleLibrary;
        use crate::modules::aec::engine::material::Material;
        use crate::modules::aec::engine::opening_style::{
            LeafOperation, LintelDimensions, OpeningStyle, ShutterBoxDimensions, SillDimensions,
        };
        use crate::modules::aec::engine::openings::{
            OpeningKind, OpeningReferenceSide, SwingSide,
        };
        use crate::modules::aec::engine::views::scene_collector::{
            CollectedOpening, CollectedWall,
        };

        let mut style = OpeningStyle::standard_window();
        style.style.id = "section_window".into();
        style.parts.fixed_frame.face_width = 0.07;
        style.parts.fixed_frame.wall_depth = 0.12;
        style.parts.exterior_sill = SillDimensions {
            enabled: true,
            profile_id: None,
            thickness: 0.10,
            overhang: 0.20,
            upstand: 0.03,
            side_overhang_left: 0.0,
            side_overhang_right: 0.0,
        };
        style.parts.interior_board = SillDimensions {
            enabled: true,
            profile_id: None,
            thickness: 0.08,
            overhang: 0.15,
            upstand: 0.0,
            side_overhang_left: 0.0,
            side_overhang_right: 0.0,
        };
        style.parts.shutter_box = ShutterBoxDimensions {
            enabled: true,
            profile_id: None,
            height: 0.20,
            depth: 0.25,
            seat_above_frame: 0.0,
            installation_depth: 0.02,
        };
        style.parts.lintel = LintelDimensions {
            enabled: true,
            profile_id: None,
            height: 0.20,
            depth: 0.0,
            bearing_left: 0.15,
            bearing_right: 0.15,
            material_id: Some("lintel_material".into()),
        };
        style.looks.exterior_sill.material_id = Some("sill_material".into());
        style.looks.shutter_box.material_id = Some("missing_material".into());
        let mut library = StyleLibrary::default();
        let mut material = Material::new(
            "lintel_material".into(),
            "Lintel hatch".into(),
            "ANSI32".into(),
            0x804020,
            "Continuous".into(),
        );
        material.hatch_scale = 1.7;
        library.materials.push(material);
        let mut sill_material = Material::new(
            "sill_material".into(),
            "Sill hatch".into(),
            "ANSI33".into(),
            0x0060c0,
            "Continuous".into(),
        );
        sill_material.hatch_color = Some(acadrust::types::Color::Rgb {
            r: 12,
            g: 34,
            b: 56,
        });
        sill_material.hatch_scale = 0.75;
        library.materials.push(sill_material);
        library.opening_styles.push(style.clone());

        let opening_element = ElementRef::new(ModelId::new(), ElementId::new());
        let opening = CollectedOpening {
            handle: acadrust::Handle::new(20),
            element_ref: opening_element,
            xref_source: None,
            host_wall_handle: acadrust::Handle::new(10),
            kind: OpeningKind::Window,
            style_id: Some(style.style.id.clone()),
            division_override: None,
            distance_along_axis: 5.0,
            width: 1.2,
            height: 1.4,
            sill_height: 0.8,
            spring_height: 0.0,
            shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
            hinge: LeafOperation::left(),
            swing_side: SwingSide::Exterior,
            reference_side: OpeningReferenceSide::Center,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(5.0, 0.0, 1.5),
            axis_span_local: (4.4, 5.6),
        };
        let wall = CollectedWall {
            handle: acadrust::Handle::new(10),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            style_id: "section_wall".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
            layers: vec![layer("Core", 0.30, -0.15, true)],
            total_thickness: 0.30,
            justification: WallJustification::Center,
            openings: vec![opening.clone()],
            phase: PlanPhase::New,
        };
        let model_view = ModelView {
            id: ModelViewId::new(),
            name: "Section parts".into(),
            viewpoint_id: ViewpointId::new(),
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };
        let ctx = ViewStyleContext::new(&model_view, None, Some(&library), true);
        let emit = |width| {
            let mut opening = opening.clone();
            opening.width = width;
            let mut primitives = Vec::new();
            let mut counter = 0;
            emit_opening_cut_details(
                &opening,
                &wall,
                &[(0.8, 2.2)],
                &style.parts,
                false,
                1.0,
                1.15,
                0.85,
                0.85,
                1.15,
                &model_view,
                &ctx,
                &mut primitives,
                &mut counter,
            )
            .unwrap();
            primitives
        };
        let standard = emit(1.2);
        let wider = emit(2.4);
        assert_eq!(standard, wider);

        let bounds = |primitive: &DerivedPrimitive| {
            let Geometry2d::Region { outer, .. } = &primitive.geometry else {
                return None;
            };
            Some((
                outer.iter().map(|point| point.x_m).fold(f64::INFINITY, f64::min),
                outer.iter().map(|point| point.x_m).fold(f64::NEG_INFINITY, f64::max),
                outer.iter().map(|point| point.y_m).fold(f64::INFINITY, f64::min),
                outer.iter().map(|point| point.y_m).fold(f64::NEG_INFINITY, f64::max),
            ))
        };
        assert!(standard.iter().any(|primitive| {
            bounds(primitive).is_some_and(|(u0, u1, z0, z1)| {
                (u1 - u0 - 0.12).abs() < 1e-9 && (z1 - z0 - 0.07).abs() < 1e-9
            })
        }));
        assert!(standard.iter().any(|primitive| {
            bounds(primitive).is_some_and(|(u0, u1, z0, z1)| {
                (u1 - u0 - 0.25).abs() < 1e-9 && (z1 - z0 - 0.20).abs() < 1e-9
            })
        }));
        let lintel = standard
            .iter()
            .find(|primitive| primitive.role == PrimitiveRole::CutFill)
            .unwrap();
        assert_eq!(lintel.style.hatch_pattern.as_deref(), Some("ANSI32"));
        assert_eq!(lintel.style.hatch_scale, Some(1.7));
        assert_eq!(lintel.style.fill_color, Some(Rgba8::new(0x80, 0x40, 0x20, 255)));

        let sill = standard
            .iter()
            .find(|primitive| primitive.style.hatch_pattern.as_deref() == Some("ANSI33"))
            .unwrap();
        assert_eq!(sill.style.hatch_scale, Some(0.75));
        assert_eq!(sill.style.fill_color, Some(Rgba8::new(12, 34, 56, 255)));

        let unknown_material_style = resolve_opening_component_cut_style(
            &ctx,
            &opening,
            OpeningLookRole::ShutterBox,
        );
        assert_eq!(unknown_material_style.hatch_pattern, None);
        assert_eq!(unknown_material_style.line_color, None);

        let mut triangle_library = library.clone();
        let mut triangle_style = style.clone();
        triangle_style.parts.fixed_frame.profile_id = Some("section_triangle".into());
        triangle_style.parts.fixed_frame.face_width = 0.0;
        triangle_style.parts.fixed_frame.wall_depth = 0.0;
        triangle_library.opening_styles[0] = triangle_style.clone();
        assert!(triangle_library.upsert_section_profile(
            crate::modules::aec::engine::section_profile::SectionProfile::new(
                "section_triangle",
                "Section triangle",
                crate::modules::aec::engine::section_profile::SectionShape::Triangle {
                    base: 0.07,
                    height: 0.05,
                },
            )
        ));
        let triangle_ctx = ViewStyleContext::new(&model_view, None, Some(&triangle_library), true);
        let mut triangle_primitives = Vec::new();
        let mut triangle_counter = 0;
        emit_opening_cut_details(
            &opening,
            &wall,
            &[(0.8, 2.2)],
            &triangle_style.parts,
            false,
            1.0,
            1.15,
            0.85,
            0.85,
            1.15,
            &model_view,
            &triangle_ctx,
            &mut triangle_primitives,
            &mut triangle_counter,
        )
        .unwrap();
        let triangle_frame = triangle_primitives
            .iter()
            .find(|primitive| {
                primitive.sources.as_slice().iter().any(|source| {
                    source.feature == Some(FeatureKey::OpeningSill)
                }) && matches!(&primitive.geometry, Geometry2d::Region { outer, .. } if outer.len() == 3)
            })
            .expect("triangle profile should be a triangular cut region");
        let triangle_bounds = bounds(triangle_frame).expect("triangle is a section region");
        assert!((triangle_bounds.1 - triangle_bounds.0 - 0.05).abs() < 1e-9);
        assert!((triangle_bounds.3 - triangle_bounds.2 - 0.07).abs() < 1e-9);

        let mut sill_profile_style = style.clone();
        sill_profile_style.parts.exterior_sill.profile_id = Some("sill_section_triangle".into());
        let mut sill_profile_library = library.clone();
        assert!(sill_profile_library.upsert_section_profile(
            crate::modules::aec::engine::section_profile::SectionProfile::new(
                "sill_section_triangle",
                "Sill section triangle",
                crate::modules::aec::engine::section_profile::SectionShape::Triangle {
                    base: 0.07,
                    height: 0.05,
                },
            )
        ));
        let sill_profile_ctx =
            ViewStyleContext::new(&model_view, None, Some(&sill_profile_library), true);
        let mut sill_profile_primitives = Vec::new();
        let mut sill_profile_counter = 0;
        emit_opening_cut_details(
            &opening,
            &wall,
            &[(0.8, 2.2)],
            &sill_profile_style.parts,
            false,
            1.0,
            1.15,
            0.85,
            0.85,
            1.15,
            &model_view,
            &sill_profile_ctx,
            &mut sill_profile_primitives,
            &mut sill_profile_counter,
        )
        .unwrap();
        let sill_profile = sill_profile_primitives
            .iter()
            .find(|primitive| {
                primitive.sources.as_slice().iter().any(|source| {
                    source.feature == Some(FeatureKey::OpeningSill)
                }) && matches!(&primitive.geometry, Geometry2d::Region { outer, .. } if outer.len() == 3)
            })
            .expect("profiled sill should keep its triangle contour");
        let sill_profile_bounds = bounds(sill_profile).expect("profiled sill is a section region");
        assert!((sill_profile_bounds.1 - sill_profile_bounds.0 - 0.05).abs() < 1e-9);
        assert!((sill_profile_bounds.3 - sill_profile_bounds.2 - 0.07).abs() < 1e-9);
        assert!(
            (sill_profile_bounds.0 - 1.06).abs() < 1e-9
                || (sill_profile_bounds.1 - 1.06).abs() < 1e-9,
            "profiled sill bounds should include the fixed-frame face: {sill_profile_bounds:?}"
        );
        assert!((sill_profile_bounds.2 - opening.sill_height).abs() < 1e-9);
    }

    #[test]
    fn section_and_elevation_place_slab_layers_on_the_live_top() {
        use crate::modules::aec::engine::slab::{Slab, SlabJustification, SlabLayer};
        use crate::modules::aec::engine::slab_xdata::slab_record_for_slab;
        use crate::modules::aec::engine::wall_style::LayerFunction;
        use crate::modules::aec::engine::xdata::AEC_APPID;
        use crate::scene::Scene;
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2 as AcadVector2;
        use acadrust::xdata::ExtendedDataRecord;

        let mut slab = Slab::new("Geschossdecke", 1);
        slab.justification = SlabJustification::Top;
        slab.layers = vec![
            SlabLayer::new("Estrich", 0.06, LayerFunction::Finish),
            SlabLayer::new("Stahlbeton", 0.20, LayerFunction::Structural),
            SlabLayer::new("Putz", 0.015, LayerFunction::Finish),
        ];
        slab.base_origin = [0.0, 0.0, 2.80];
        slab.top_origin = [0.0, 0.0, 2.80];
        let mut polygon = LwPolyline::new();
        for point in [(0.0, 0.0), (6.0, 0.0), (6.0, 4.0), (0.0, 4.0)] {
            polygon.add_vertex(LwVertex::new(AcadVector2::new(point.0, point.1)));
        }
        polygon.is_closed = true;
        polygon.elevation = 2.80;
        let mut entity = acadrust::EntityType::LwPolyline(polygon);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = slab_record_for_slab(&slab);
        entity.common_mut().extended_data.add_record(record);
        let mut scene = Scene::new();
        scene.add_entity(entity);

        let collector = SceneGeometryCollector::collect_from_document(&scene.document, ModelId::new());
        let expected = slab.layer_z_ranges(2.80);
        assert!(
            (collector.slabs[0].layer_z_ranges()[0].0 - 2.80).abs() < 1e-9,
            "OKFF top stays on the reference, it is not lifted by the thickness"
        );

        let viewpoint = Viewpoint {
            id: ViewpointId::new(),
            name: "Schnitt".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, -1.0)),
            axes: ViewAxes::section(Vector2::new(1.0, 0.0)).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(-1.0, 2.0), Point2::new(8.0, 2.0)),
                far_depth: None,
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(4.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };
        let model_view = ModelView {
            id: ModelViewId::new(),
            name: "Schnitt".into(),
            viewpoint_id: viewpoint.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };
        let section = derive_section_view(
            &viewpoint,
            &model_view,
            &collector,
            &CutEngineOptions::default(),
        )
        .unwrap();
        let mut section_bands: Vec<(f64, f64)> = section
            .iter()
            .filter(|primitive| primitive.role == PrimitiveRole::CutFill)
            .filter_map(|primitive| match &primitive.geometry {
                Geometry2d::Region { outer, .. } => {
                    let min_y = outer.iter().map(|point| point.y_m).fold(f64::INFINITY, f64::min);
                    let max_y = outer.iter().map(|point| point.y_m).fold(f64::NEG_INFINITY, f64::max);
                    Some((max_y, min_y))
                }
                _ => None,
            })
            .collect();
        section_bands.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        assert_eq!(section_bands.len(), expected.len());
        for (actual, live) in section_bands.iter().zip(expected.iter()) {
            assert!((actual.0 - live.0).abs() < 1e-6 && (actual.1 - live.1).abs() < 1e-6, "{actual:?} {live:?}");
        }

        let mut elevation = Vec::new();
        let mut counter = 0;
        crate::modules::aec::engine::views::elevation_engine::project_slab_elevation(
            &collector.slabs[0],
            &viewpoint,
            0.0,
            f64::INFINITY,
            &model_view,
            &crate::modules::aec::engine::views::elevation_engine::ElevationEngineOptions::default(),
            &crate::modules::aec::engine::views::style_resolver::ViewStyleContext::new(
                &model_view,
                None,
                None,
                true,
            ),
            &mut elevation,
            &mut counter,
        )
        .unwrap();
        let mut elevation_bands: Vec<(f64, f64)> = elevation
            .iter()
            .filter(|primitive| primitive.role == PrimitiveRole::SurfaceFill)
            .filter_map(|primitive| match &primitive.geometry {
                Geometry2d::Region { outer, .. } => {
                    let min_y = outer.iter().map(|point| point.y_m).fold(f64::INFINITY, f64::min);
                    let max_y = outer.iter().map(|point| point.y_m).fold(f64::NEG_INFINITY, f64::max);
                    Some((max_y, min_y))
                }
                _ => None,
            })
            .collect();
        elevation_bands.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        elevation_bands.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6);
        assert_eq!(elevation_bands.len(), expected.len(), "{elevation_bands:?}");
        for (actual, live) in elevation_bands.iter().zip(expected.iter()) {
            assert!((actual.0 - live.0).abs() < 1e-6 && (actual.1 - live.1).abs() < 1e-6, "{actual:?} {live:?}");
        }
    }
}
