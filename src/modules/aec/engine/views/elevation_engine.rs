//! 2D Elevation and Façade Derivation Engine.
//!
//! Projects 3D walls, slabs, and openings onto the view plane with full slot-based
//! opening components (ElevationContour2D, ElevationSwing2D, ElevationSill2D, ElevationMuntins2D).

use super::model_view::{DetailLevel, ModelView};
use super::provenance::{
    DerivedPrimitive, EditHandle, FeatureKey, Geometry2d, ModelEditAction, ModelParameter,
    PrimitiveId, PrimitiveRole, ResolvedStyle, Rgba8, SourceReference, SourceSet,
};
use super::scene_collector::{CollectedOpening, CollectedSlab, CollectedWall, SceneGeometryCollector};
use super::viewpoint::{ViewKind, Viewpoint};
use super::{
    DepthInterval, PaperMm, Point2, Point3, ValidationError, Vector2,
};
use crate::modules::aec::engine::opening_style::HingeSide;
use crate::modules::aec::engine::openings::OpeningKind;

const EPS: f64 = 1e-6;

#[derive(Clone, Debug)]
pub struct ElevationEngineOptions {
    pub show_hidden_edges: bool,
    pub include_opening_symbols: bool,
    pub detail_level: DetailLevel,
}

impl Default for ElevationEngineOptions {
    fn default() -> Self {
        Self {
            show_hidden_edges: false,
            include_opening_symbols: true,
            detail_level: DetailLevel::Fine,
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

    let mut primitives = Vec::new();
    let mut primitive_counter: u32 = 0;

    let z_min = viewpoint.z_range.z_min.get();
    let z_max = viewpoint.z_range.z_max.get();

    // 1. Process Walls
    for wall in &scene.walls {
        if model_view.excluded_elements.contains(&wall.element_ref) {
            continue;
        }
        if wall.top_z < z_min || wall.base_z > z_max {
            continue;
        }

        project_wall_elevation(
            wall,
            viewpoint,
            near_depth,
            far_depth,
            model_view,
            options,
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
            &mut primitives,
            &mut primitive_counter,
        )?;
    }

    // 3. Silhouette & Occlusion processing
    super::silhouette::process_silhouette_and_depth(
        &mut primitives,
        &super::silhouette::SilhouetteOptions {
            emit_hidden_lines: options.show_hidden_edges,
            max_depth_distance: Some(far_depth),
        },
    )?;

    Ok(primitives)
}

pub fn project_wall_elevation(
    wall: &CollectedWall,
    viewpoint: &Viewpoint,
    near_depth: f64,
    far_depth: f64,
    model_view: &ModelView,
    options: &ElevationEngineOptions,
    primitives: &mut Vec<DerivedPrimitive>,
    counter: &mut u32,
) -> Result<(), ValidationError> {
    if wall.axis_points.len() < 2 {
        return Ok(());
    }

    for window in wall.axis_points.windows(2) {
        let p0 = window[0];
        let p1 = window[1];

        // 3D coordinates of wall baseline endpoints at base Z and top Z
        let pt0_base = Point3::new(p0.x_m, p0.y_m, wall.base_z);
        let pt1_base = Point3::new(p1.x_m, p1.y_m, wall.base_z);
        let pt0_top = Point3::new(p0.x_m, p0.y_m, wall.top_z);
        let pt1_top = Point3::new(p1.x_m, p1.y_m, wall.top_z);

        let (uv0_base, d0_base) = viewpoint.world_to_view(pt0_base);
        let (uv1_base, d1_base) = viewpoint.world_to_view(pt1_base);
        let (uv0_top, _) = viewpoint.world_to_view(pt0_top);
        let (uv1_top, _) = viewpoint.world_to_view(pt1_top);

        let avg_depth = (d0_base + d1_base) * 0.5;
        if avg_depth < near_depth - EPS || avg_depth > far_depth + EPS {
            continue;
        }

        let wall_source = SourceReference {
            element: wall.element_ref,
            xref_source: wall.xref_source.clone(),
            component_id: None,
            feature: Some(FeatureKey::WallTop),
            display_component_id: None,
        };

        let style = ResolvedStyle::solid_stroke(PaperMm::new(0.25)?);

        // Wall Top projected edge
        *counter += 1;
        let top_anchor = Point2::new((uv0_top.x_m + uv1_top.x_m) * 0.5, (uv0_top.y_m + uv1_top.y_m) * 0.5);
        primitives.push(DerivedPrimitive {
            id: PrimitiveId::new(model_view.id, 1, *counter),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Line {
                start: uv0_top,
                end: uv1_top,
            },
            sources: SourceSet::new(vec![wall_source.clone()])?,
            depth: DepthInterval::new(avg_depth, avg_depth)?,
            style: style.clone(),
            edit_handles: vec![EditHandle {
                id: format!("wall_top_elev_{}", counter),
                position: top_anchor,
                action: ModelEditAction::ChangeParameter(ModelParameter::WallTopOffset),
                direction_hint: Vector2::new(0.0, 1.0),
                target_feature: Some(FeatureKey::WallTop),
                source: wall_source.clone(),
            }],
        });

        // Wall Base projected edge
        let mut base_source = wall_source.clone();
        base_source.feature = Some(FeatureKey::WallBase);
        *counter += 1;
        primitives.push(DerivedPrimitive {
            id: PrimitiveId::new(model_view.id, 1, *counter),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Line {
                start: uv0_base,
                end: uv1_base,
            },
            sources: SourceSet::new(vec![base_source])?,
            depth: DepthInterval::new(avg_depth, avg_depth)?,
            style: style.clone(),
            edit_handles: vec![],
        });

        // Left and Right Wall Contour jambs
        *counter += 1;
        primitives.push(DerivedPrimitive {
            id: PrimitiveId::new(model_view.id, 1, *counter),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Line {
                start: uv0_base,
                end: uv0_top,
            },
            sources: SourceSet::new(vec![wall_source.clone()])?,
            depth: DepthInterval::new(avg_depth, avg_depth)?,
            style: style.clone(),
            edit_handles: vec![],
        });

        *counter += 1;
        primitives.push(DerivedPrimitive {
            id: PrimitiveId::new(model_view.id, 1, *counter),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Line {
                start: uv1_base,
                end: uv1_top,
            },
            sources: SourceSet::new(vec![wall_source.clone()])?,
            depth: DepthInterval::new(avg_depth, avg_depth)?,
            style,
            edit_handles: vec![],
        });

        // Wall Opaque Surface Mask (used for 3D occlusion clipping)
        *counter += 1;
        let surface_poly = vec![uv0_base, uv1_base, uv1_top, uv0_top];
        let seg_len = (p1.x_m - p0.x_m).hypot(p1.y_m - p0.y_m);
        let mut opening_holes = Vec::new();
        if options.include_opening_symbols && seg_len > EPS {
            let dir = Vector2::new((p1.x_m - p0.x_m) / seg_len, (p1.y_m - p0.y_m) / seg_len);
            for op in &wall.openings {
                let (s_lo, s_hi) = op.axis_span_local;
                let z_sill = wall.base_z + op.sill_height;
                let z_head = z_sill + op.height;
                let pt_bl = Point3::new(p0.x_m + dir.x * s_lo, p0.y_m + dir.y * s_lo, z_sill);
                let pt_br = Point3::new(p0.x_m + dir.x * s_hi, p0.y_m + dir.y * s_hi, z_sill);
                let pt_tl = Point3::new(p0.x_m + dir.x * s_lo, p0.y_m + dir.y * s_lo, z_head);
                let pt_tr = Point3::new(p0.x_m + dir.x * s_hi, p0.y_m + dir.y * s_hi, z_head);
                let (uv_bl, _) = viewpoint.world_to_view(pt_bl);
                let (uv_br, _) = viewpoint.world_to_view(pt_br);
                let (uv_tr, _) = viewpoint.world_to_view(pt_tr);
                let (uv_tl, _) = viewpoint.world_to_view(pt_tl);
                opening_holes.push(vec![uv_bl, uv_br, uv_tr, uv_tl]);
            }
        }
        primitives.push(DerivedPrimitive {
            id: PrimitiveId::new(model_view.id, 1, *counter),
            role: PrimitiveRole::SurfaceFill,
            geometry: Geometry2d::Region {
                outer: surface_poly,
                holes: opening_holes,
            },
            sources: SourceSet::new(vec![wall_source])?,
            depth: DepthInterval::new(avg_depth, avg_depth)?,
            style: ResolvedStyle {
                line_weight_mm: PaperMm::new(0.18)?,
                line_color: None,
                line_type: None,
                fill_color: None,
                hatch_pattern: None,
                hatch_scale: None,
                hatch_angle_rad: None,
                layer_name: Some("AEC_SECTION_SURFACE".to_string()),
            },
            edit_handles: vec![],
        });

        // Openings on this wall
        if options.include_opening_symbols {
            for op in &wall.openings {
                project_opening_elevation(
                    op,
                    wall,
                    p0,
                    p1,
                    viewpoint,
                    near_depth,
                    far_depth,
                    model_view,
                    primitives,
                    counter,
                )?;
            }
        }
    }

    Ok(())
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
    primitives: &mut Vec<DerivedPrimitive>,
    counter: &mut u32,
) -> Result<(), ValidationError> {
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

    let (uv_bl, d_bl) = viewpoint.world_to_view(pt_bl);
    let (uv_br, d_br) = viewpoint.world_to_view(pt_br);
    let (uv_tl, _) = viewpoint.world_to_view(pt_tl);
    let (uv_tr, _) = viewpoint.world_to_view(pt_tr);

    let avg_depth = (d_bl + d_br) * 0.5;
    if avg_depth < near_depth - EPS || avg_depth > far_depth + EPS {
        return Ok(());
    }

    let op_source = SourceReference {
        element: op.element_ref,
        xref_source: op.xref_source.clone(),
        component_id: None,
        feature: Some(FeatureKey::OpeningSill),
        display_component_id: None,
    };

    let frame_style = ResolvedStyle {
        line_weight_mm: PaperMm::new(0.35)?,
        line_color: Some(Rgba8::new(0, 90, 180, 255)),
        line_type: None,
        fill_color: None,
        hatch_pattern: None,
        hatch_scale: None,
        hatch_angle_rad: None,
        layer_name: None,
    };

    // 1. ElevationContour2D: Outer frame contour
    *counter += 1;
    let frame_poly = vec![uv_bl, uv_br, uv_tr, uv_tl];
    let sill_anchor = Point2::new((uv_bl.x_m + uv_br.x_m) * 0.5, (uv_bl.y_m + uv_br.y_m) * 0.5);
    let head_anchor = Point2::new((uv_tl.x_m + uv_tr.x_m) * 0.5, (uv_tl.y_m + uv_tr.y_m) * 0.5);

    primitives.push(DerivedPrimitive {
        id: PrimitiveId::new(model_view.id, 1, *counter),
        role: PrimitiveRole::ProjectedEdge,
        geometry: Geometry2d::Region {
            outer: frame_poly,
            holes: vec![],
        },
        sources: SourceSet::new(vec![op_source.clone()])?,
        depth: DepthInterval::new(avg_depth, avg_depth)?,
        style: frame_style,
        edit_handles: vec![
            EditHandle {
                id: format!("op_sill_{}", counter),
                position: sill_anchor,
                action: ModelEditAction::ChangeParameter(ModelParameter::OpeningSillHeight),
                direction_hint: Vector2::new(0.0, 1.0),
                target_feature: Some(FeatureKey::OpeningSill),
                source: op_source.clone(),
            },
            EditHandle {
                id: format!("op_head_{}", counter),
                position: head_anchor,
                action: ModelEditAction::ChangeParameter(ModelParameter::OpeningHeadHeight),
                direction_hint: Vector2::new(0.0, 1.0),
                target_feature: Some(FeatureKey::OpeningHead),
                source: op_source.clone(),
            },
        ],
    });

    // 2. ElevationSwing2D: DIN Opening triangle symbol
    if op.kind == OpeningKind::Window || op.kind == OpeningKind::Door {
        let apex = match op.hinge {
            HingeSide::Left => Point2::new(
                uv_br.x_m,
                (uv_br.y_m + uv_tr.y_m) * 0.5,
            ),
            HingeSide::Right => Point2::new(
                uv_bl.x_m,
                (uv_bl.y_m + uv_tl.y_m) * 0.5,
            ),
        };

        let swing_pts = match op.hinge {
            HingeSide::Left => vec![uv_tl, apex, uv_bl],
            HingeSide::Right => vec![uv_tr, apex, uv_br],
        };

        let swing_style = ResolvedStyle {
            line_weight_mm: PaperMm::new(0.18)?,
            line_color: Some(Rgba8::new(100, 100, 100, 255)),
            line_type: Some("DASHED".into()),
            fill_color: None,
            hatch_pattern: None,
            hatch_scale: None,
            hatch_angle_rad: None,
            layer_name: None,
        };

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
    if op.kind == OpeningKind::Window {
        let ear = 0.05;
        let sill_p0 = Point2::new(uv_bl.x_m - ear, uv_bl.y_m - 0.04);
        let sill_p1 = Point2::new(uv_br.x_m + ear, uv_br.y_m - 0.04);

        let sill_style = ResolvedStyle {
            line_weight_mm: PaperMm::new(0.25)?,
            line_color: Some(Rgba8::new(0, 0, 0, 255)),
            line_type: None,
            fill_color: None,
            hatch_pattern: None,
            hatch_scale: None,
            hatch_angle_rad: None,
            layer_name: None,
        };

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
    primitives: &mut Vec<DerivedPrimitive>,
    counter: &mut u32,
) -> Result<(), ValidationError> {
    let n = slab.polygon.len();
    if n < 3 {
        return Ok(());
    }

    let z_bot = slab.base_z;
    let z_top = slab.base_z + slab.total_thickness;

    for i in 0..n {
        let p0 = slab.polygon[i];
        let p1 = slab.polygon[(i + 1) % n];

        let pt0_bot = Point3::new(p0.x_m, p0.y_m, z_bot);
        let pt1_bot = Point3::new(p1.x_m, p1.y_m, z_bot);
        let pt0_top = Point3::new(p0.x_m, p0.y_m, z_top);
        let pt1_top = Point3::new(p1.x_m, p1.y_m, z_top);

        let (uv0_bot, d0) = viewpoint.world_to_view(pt0_bot);
        let (uv1_bot, d1) = viewpoint.world_to_view(pt1_bot);
        let (uv0_top, _) = viewpoint.world_to_view(pt0_top);
        let (uv1_top, _) = viewpoint.world_to_view(pt1_top);

        let avg_depth = (d0 + d1) * 0.5;
        if avg_depth < near_depth - EPS || avg_depth > far_depth + EPS {
            continue;
        }

        let slab_source = SourceReference {
            element: slab.element_ref,
            xref_source: slab.xref_source.clone(),
            component_id: None,
            feature: Some(FeatureKey::SlabTop),
            display_component_id: None,
        };

        let style = ResolvedStyle::solid_stroke(PaperMm::new(0.25)?);

        *counter += 1;
        primitives.push(DerivedPrimitive {
            id: PrimitiveId::new(model_view.id, 1, *counter),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Line {
                start: uv0_top,
                end: uv1_top,
            },
            sources: SourceSet::new(vec![slab_source.clone()])?,
            depth: DepthInterval::new(avg_depth, avg_depth)?,
            style: style.clone(),
            edit_handles: vec![],
        });

        let mut bot_source = slab_source.clone();
        bot_source.feature = Some(FeatureKey::SlabBottom);
        *counter += 1;
        primitives.push(DerivedPrimitive {
            id: PrimitiveId::new(model_view.id, 1, *counter),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Line {
                start: uv0_bot,
                end: uv1_bot,
            },
            sources: SourceSet::new(vec![bot_source])?,
            depth: DepthInterval::new(avg_depth, avg_depth)?,
            style,
            edit_handles: vec![],
        });

        // Slab vertical face opaque surface mask
        if (uv1_bot.x_m - uv0_bot.x_m).abs() > EPS || (uv1_bot.y_m - uv0_bot.y_m).abs() > EPS {
            *counter += 1;
            let slab_poly = vec![uv0_bot, uv1_bot, uv1_top, uv0_top];
            primitives.push(DerivedPrimitive {
                id: PrimitiveId::new(model_view.id, 1, *counter),
                role: PrimitiveRole::SurfaceFill,
                geometry: Geometry2d::Region {
                    outer: slab_poly,
                    holes: vec![],
                },
                sources: SourceSet::new(vec![slab_source])?,
                depth: DepthInterval::new(avg_depth, avg_depth)?,
                style: ResolvedStyle {
                    line_weight_mm: PaperMm::new(0.18)?,
                    line_color: None,
                    line_type: None,
                    fill_color: None,
                    hatch_pattern: None,
                    hatch_scale: None,
                    hatch_angle_rad: None,
                    layer_name: Some("AEC_SECTION_SURFACE".to_string()),
                },
                edit_handles: vec![],
            });
        }
    }

    Ok(())
}
