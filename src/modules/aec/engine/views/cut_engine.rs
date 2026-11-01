//! 3D Cut and Section Derivation Engine for multi-layer walls, slabs, and openings.
//!
//! Evaluates straight and jogged section paths across the aggregated scene geometry,
//! generating associative 2D `DerivedPrimitive`s (cut boundaries, layer fills, material hatches)
//! with full provenance and feature keys.

use super::model_view::{DetailLevel, ModelView};
use super::provenance::{
    DerivedPrimitive, EditHandle, FeatureKey, Geometry2d, ModelEditAction, ModelParameter,
    PrimitiveId, PrimitiveRole, ResolvedStyle, SourceReference, SourceSet,
};
use super::scene_collector::{
    CollectedOpening, CollectedSlab, CollectedWall, SceneGeometryCollector,
};
use super::style_resolver::{
    is_draw_suppressed, is_opening_slot_visible, is_slab_phase_visible, is_slab_layer_visible,
    is_wall_layer_visible, is_wall_phase_visible, resolve_layer_cut_style,
    resolve_opening_view_style, resolve_slab_layer_cut_style, ViewStyleContext,
};
use super::viewpoint::{SectionSegment, ViewKind, Viewpoint};
use super::{
    PaperMm, Point2, ValidationError, Vector2,
};
use crate::modules::aec::engine::display_component::OpeningComponentSlot;
use crate::modules::aec::engine::library::StyleLibrary;
use crate::modules::aec::engine::openings::OpeningKind;
use crate::modules::aec::engine::plan_view::DisplayConfig;

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

    // Iterate over wall baseline segments
    for window in wall.axis_points.windows(2) {
        let w_p0 = window[0];
        let w_p1 = window[1];
        let w_vec = Vector2::new(w_p1.x_m - w_p0.x_m, w_p1.y_m - w_p0.y_m);
        let w_len = w_vec.length();
        if w_len < EPS {
            continue;
        }
        let w_dir = w_vec.normalized()?;
        let w_norm = Vector2::new(-w_dir.y, w_dir.x);

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
            let s_along_wall = (hit_pt.x_m - w_p0.x_m) * w_dir.x + (hit_pt.y_m - w_p0.y_m) * w_dir.y;

            // Check if cut passes through any opening in the wall
            let intersecting_opening = wall.openings.iter().find(|op| {
                let (op_s0, op_s1) = op.axis_span_local;
                s_along_wall >= op_s0 - EPS && s_along_wall <= op_s1 + EPS
            });

            let mut z_intervals = vec![(z_bottom, z_top)];

            if let Some(op) = intersecting_opening {
                let op_z_sill = wall.base_z + op.sill_height;
                let op_z_head = op_z_sill + op.height;

                // Split layer cut into sill remnant (below) and head remnant (above)
                let mut split = Vec::new();
                for (zb, zt) in z_intervals {
                    if op_z_sill > zb + EPS {
                        split.push((zb, zb.max(op_z_sill.min(zt))));
                    }
                    if op_z_head < zt - EPS {
                        split.push((zt.min(op_z_head.max(zb)), zt));
                    }
                }
                z_intervals = split;

                // The cut removes the wall, so the void itself has to be opaque.
                // Otherwise the section shows every opening and wall behind the glass.
                emit_opening_void_occluder(
                    op,
                    wall,
                    u_min,
                    u_max,
                    op_z_sill,
                    op_z_head,
                    model_view,
                    primitives,
                    counter,
                )?;

                if options.include_openings && (op.kind == OpeningKind::Window || op.kind == OpeningKind::Door) {
                    emit_opening_cut_details(
                        op,
                        wall,
                        u_min,
                        u_max,
                        accum_s,
                        model_view,
                        ctx,
                        primitives,
                        counter,
                    )?;
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

/// Emits opening cut components (frame profiles, sill/head lines) when a section passes through a window or door.
fn emit_opening_cut_details(
    op: &CollectedOpening,
    wall: &CollectedWall,
    u_min: f64,
    u_max: f64,
    _accum_s: f64,
    model_view: &ModelView,
    ctx: &ViewStyleContext,
    primitives: &mut Vec<DerivedPrimitive>,
    counter: &mut u32,
) -> Result<(), ValidationError> {
    let op_z_sill = wall.base_z + op.sill_height;
    let op_z_head = op_z_sill + op.height;

    let op_source = SourceReference {
        element: op.element_ref,
        xref_source: op.xref_source.clone(),
        component_id: None,
        feature: Some(FeatureKey::OpeningSill),
        display_component_id: None,
    };

    // Sill line with interactive sill grip handle
    if is_opening_slot_visible(ctx, op, OpeningComponentSlot::Sill2D) {
        let sill_style = resolve_opening_view_style(ctx, op, OpeningComponentSlot::Sill2D);
        *counter += 1;
        let sill_anchor = Point2::new((u_min + u_max) * 0.5, op_z_sill);
        primitives.push(DerivedPrimitive {
            id: PrimitiveId::new(model_view.id, 1, *counter),
            role: PrimitiveRole::CutBoundary,
            geometry: Geometry2d::Line {
                start: Point2::new(u_min, op_z_sill),
                end: Point2::new(u_max, op_z_sill),
            },
            sources: SourceSet::new(vec![op_source.clone()])?,
            depth: super::DepthInterval::cut_plane(),
            style: sill_style,
            edit_handles: vec![EditHandle {
                id: format!("opening_sill_{}", counter),
                position: sill_anchor,
                action: ModelEditAction::ChangeParameter(ModelParameter::OpeningSillHeight),
                direction_hint: Vector2::new(0.0, 1.0),
                target_feature: Some(FeatureKey::OpeningSill),
                source: op_source.clone(),
            }],
        });
    }

    // Head line with interactive head grip handle
    if is_opening_slot_visible(ctx, op, OpeningComponentSlot::Frame2D) {
        let head_style = resolve_opening_view_style(ctx, op, OpeningComponentSlot::Frame2D);
        let mut head_source = op_source;
        head_source.feature = Some(FeatureKey::OpeningHead);
        *counter += 1;
        let head_anchor = Point2::new((u_min + u_max) * 0.5, op_z_head);
        primitives.push(DerivedPrimitive {
            id: PrimitiveId::new(model_view.id, 1, *counter),
            role: PrimitiveRole::CutBoundary,
            geometry: Geometry2d::Line {
                start: Point2::new(u_min, op_z_head),
                end: Point2::new(u_max, op_z_head),
            },
            sources: SourceSet::new(vec![head_source.clone()])?,
            depth: super::DepthInterval::cut_plane(),
            style: head_style,
            edit_handles: vec![EditHandle {
                id: format!("opening_head_{}", counter),
                position: head_anchor,
                action: ModelEditAction::ChangeParameter(ModelParameter::OpeningHeadHeight),
                direction_hint: Vector2::new(0.0, 1.0),
                target_feature: Some(FeatureKey::OpeningHead),
                source: head_source,
            }],
        });
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
