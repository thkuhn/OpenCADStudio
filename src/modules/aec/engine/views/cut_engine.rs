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
use super::viewpoint::{SectionSegment, ViewKind, Viewpoint};
use super::{
    PaperMm, Point2, ValidationError, Vector2,
};
use crate::modules::aec::engine::openings::OpeningKind;

const EPS: f64 = 1e-6;

/// Configuration and styles resolved for the cut generation.
#[derive(Clone, Debug)]
pub struct CutEngineOptions {
    pub structural_only: bool,
    pub include_openings: bool,
    pub detail_level: DetailLevel,
}

impl Default for CutEngineOptions {
    fn default() -> Self {
        Self {
            structural_only: false,
            include_openings: true,
            detail_level: DetailLevel::Fine,
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
                &mut primitives,
                &mut primitive_counter,
            )?;
        }

        // Advance unfolded path parameter
        accum_s += seg_len;
    }

    Ok(primitives)
}

/// Cuts a multi-layer wall against a single section segment.
fn cut_wall_with_segment(
    wall: &CollectedWall,
    segment: &SectionSegment,
    accum_s: f64,
    model_view: &ModelView,
    options: &CutEngineOptions,
    primitives: &mut Vec<DerivedPrimitive>,
    counter: &mut u32,
) -> Result<(), ValidationError> {
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
            if options.structural_only && !layer.is_structural {
                continue;
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

                // Add opening cut frame & glazing details
                if options.include_openings && (op.kind == OpeningKind::Window || op.kind == OpeningKind::Door) {
                    emit_opening_cut_details(
                        op,
                        wall,
                        u_min,
                        u_max,
                        accum_s,
                        model_view,
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
                *counter += 1;
                let fill_style = ResolvedStyle {
                    line_weight_mm: PaperMm::new(0.18)?,
                    line_color: Some(Rgba8::new(128, 128, 128, 255)),
                    line_type: None,
                    fill_color: None,
                    hatch_pattern: Some(layer.hatch_override.clone().unwrap_or_else(|| "ANSI31".into())),
                    hatch_scale: Some(1.0),
                    hatch_angle_rad: Some(0.785398),
                    layer_name: None,
                };

                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 1, *counter),
                    role: PrimitiveRole::CutFill,
                    geometry: Geometry2d::Region {
                        outer: poly_points,
                        holes: vec![],
                    },
                    sources: SourceSet::new(vec![source_ref.clone()])?,
                    depth: super::DepthInterval::cut_plane(),
                    style: fill_style,
                    edit_handles: vec![],
                });

                // 2. Cut boundary lines
                let boundary_style = ResolvedStyle::solid_stroke(PaperMm::new(0.35)?);

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
            }
        }
    }

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

    let opening_line_style = ResolvedStyle {
        line_weight_mm: PaperMm::new(0.25)?,
        line_color: Some(Rgba8::new(0, 0, 180, 255)),
        line_type: None,
        fill_color: None,
        hatch_pattern: None,
        hatch_scale: None,
        hatch_angle_rad: None,
        layer_name: None,
    };

    // Sill line with interactive sill grip handle
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
        style: opening_line_style.clone(),
        edit_handles: vec![EditHandle {
            id: format!("opening_sill_{}", counter),
            position: sill_anchor,
            action: ModelEditAction::ChangeParameter(ModelParameter::OpeningSillHeight),
            direction_hint: Vector2::new(0.0, 1.0),
            target_feature: Some(FeatureKey::OpeningSill),
            source: op_source.clone(),
        }],
    });

    // Head line with interactive head grip handle
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
        style: opening_line_style,
        edit_handles: vec![EditHandle {
            id: format!("opening_head_{}", counter),
            position: head_anchor,
            action: ModelEditAction::ChangeParameter(ModelParameter::OpeningHeadHeight),
            direction_hint: Vector2::new(0.0, 1.0),
            target_feature: Some(FeatureKey::OpeningHead),
            source: head_source,
        }],
    });

    Ok(())
}

/// Cuts a multi-layer slab against a single section segment.
fn cut_slab_with_segment(
    slab: &CollectedSlab,
    segment: &SectionSegment,
    accum_s: f64,
    model_view: &ModelView,
    options: &CutEngineOptions,
    primitives: &mut Vec<DerivedPrimitive>,
    counter: &mut u32,
) -> Result<(), ValidationError> {
    if slab.polygon.len() < 3 {
        return Ok(());
    }

    let intersections = intersect_segment_with_polygon(segment, &slab.polygon);
    if intersections.len() < 2 {
        return Ok(());
    }

    let mut current_z = slab.base_z;

    for layer in &slab.layers {
        if options.structural_only && !layer.is_structural {
            current_z += layer.thickness;
            continue;
        }

        let z_bottom = current_z;
        let z_top = current_z + layer.thickness;
        current_z += layer.thickness;

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
                    line_color: Some(Rgba8::new(100, 100, 100, 255)),
                    line_type: None,
                    fill_color: None,
                    hatch_pattern: Some(layer.hatch_override.clone().unwrap_or_else(|| "AR-CONC".into())),
                    hatch_scale: Some(1.0),
                    hatch_angle_rad: Some(0.785398),
                    layer_name: None,
                },
                edit_handles: vec![],
            });

            // Slab top & bottom boundary lines
            let boundary_style = ResolvedStyle::solid_stroke(PaperMm::new(0.35)?);

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

            let mut bot_source = source_ref;
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
                style: boundary_style,
                edit_handles: vec![],
            });
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
