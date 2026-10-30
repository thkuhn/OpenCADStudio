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
use crate::modules::aec::engine::opening_style::HingeSide;
use crate::modules::aec::engine::openings::{OpeningKind, SwingSide};
use crate::modules::aec::engine::plan_view::DisplayConfig;
use super::style_resolver::{
    is_draw_suppressed, is_opening_slot_visible, is_wall_phase_visible,
    is_slab_layer_active, resolve_slab_layer_cut_style,
    resolve_opening_view_fill_style, resolve_opening_view_style,
    resolve_wall_elevation_contour_style, ViewStyleContext,
};

const EPS: f64 = 1e-6;
const OPENING_SURFACE_DEPTH_OFFSET: f64 = 1e-3;

/// Projects a facade point: horizontal station from the view axes, vertical
/// station from world Z on the same datum as a section cut.
fn project_facade_point(viewpoint: &Viewpoint, point: Point3) -> (Point2, f64) {
    let (uv, depth) = viewpoint.world_to_view(point);
    (
        Point2::new(uv.x_m, viewpoint.paper_elevation(point.z_m)),
        depth,
    )
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
            &ctx,
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

    // 3. Silhouette & Occlusion processing
    super::silhouette::process_silhouette_and_depth(
        &mut primitives,
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
    if !is_wall_phase_visible(ctx, wall) || is_draw_suppressed(ctx, &wall.element_ref, None) {
        return Ok(());
    }
    if wall.axis_points.len() < 2 {
        return Ok(());
    }

    let style = resolve_wall_elevation_contour_style(ctx, wall);

    for window in wall.axis_points.windows(2) {
        let p0 = window[0];
        let p1 = window[1];

        // 3D coordinates of wall baseline endpoints at base Z and top Z
        let pt0_base = Point3::new(p0.x_m, p0.y_m, wall.base_z);
        let pt1_base = Point3::new(p1.x_m, p1.y_m, wall.base_z);
        let pt0_top = Point3::new(p0.x_m, p0.y_m, wall.top_z);
        let pt1_top = Point3::new(p1.x_m, p1.y_m, wall.top_z);

        let (uv0_base, d0_base) = project_facade_point(viewpoint, pt0_base);
        let (uv1_base, d1_base) = project_facade_point(viewpoint, pt1_base);
        let (uv0_top, _) = project_facade_point(viewpoint, pt0_top);
        let (uv1_top, _) = project_facade_point(viewpoint, pt1_top);

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
            style: style.clone(),
            edit_handles: vec![],
        });

        // Wall Opaque Surface Mask (used for 3D occlusion clipping)
        *counter += 1;
        let surface_poly = vec![uv0_base, uv1_base, uv1_top, uv0_top];
        primitives.push(DerivedPrimitive {
            id: PrimitiveId::new(model_view.id, 1, *counter),
            role: PrimitiveRole::SurfaceFill,
            geometry: Geometry2d::Region {
                outer: surface_poly,
                holes: vec![],
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
                layer_name: Some(super::silhouette::TRANSIENT_OCCLUDER_LAYER.to_string()),
            },
            edit_handles: vec![],
        });

        // Opening surfaces participate in occlusion independently of their visible symbols.
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
                ctx,
                options.include_opening_symbols,
                primitives,
                counter,
            )?;
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
    ctx: &ViewStyleContext,
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

    let (uv_bl, d_bl) = project_facade_point(viewpoint, pt_bl);
    let (uv_br, d_br) = project_facade_point(viewpoint, pt_br);
    let (uv_tl, _) = project_facade_point(viewpoint, pt_tl);
    let (uv_tr, _) = project_facade_point(viewpoint, pt_tr);

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

    // 1. ElevationContour2D: Outer frame contour
    if is_opening_slot_visible(ctx, op, OpeningComponentSlot::ElevationContour2D) {
        let frame_style = resolve_opening_view_style(ctx, op, OpeningComponentSlot::ElevationContour2D);
        *counter += 1;
        let sill_anchor = Point2::new((uv_bl.x_m + uv_br.x_m) * 0.5, (uv_bl.y_m + uv_br.y_m) * 0.5);
        let head_anchor = Point2::new((uv_tl.x_m + uv_tr.x_m) * 0.5, (uv_tl.y_m + uv_tr.y_m) * 0.5);

        primitives.push(DerivedPrimitive {
            id: PrimitiveId::new(model_view.id, 1, *counter),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Region {
                outer: frame_poly.clone(),
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

        let (hinge_top, hinge_bot, strike_top, strike_bot) = match op.hinge {
            HingeSide::Left => (uv_tl, uv_bl, uv_tr, uv_br),
            HingeSide::Right => (uv_tr, uv_br, uv_tl, uv_bl),
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

    let active_layers: Vec<_> = slab
        .layers
        .iter()
        .enumerate()
        .filter(|(index, _)| is_slab_layer_active(ctx, slab, *index))
        .collect();
    if active_layers.is_empty() {
        return Ok(());
    }
    let layer_ranges = slab.layer_z_ranges();
    let contour_style =
        crate::modules::aec::engine::views::style_resolver::resolve_slab_contour_style(ctx, slab, false);

    for i in 0..n {
        let p0 = slab.polygon[i];
        let p1 = slab.polygon[(i + 1) % n];

        for (layer_idx, layer) in &active_layers {
            let (z_top, z_bottom) = layer_ranges[*layer_idx];
            let (uv0_top, d0_top) = project_facade_point(viewpoint, Point3::new(p0.x_m, p0.y_m, z_top));
            let (uv1_top, d1_top) = project_facade_point(viewpoint, Point3::new(p1.x_m, p1.y_m, z_top));
            let (uv0_bottom, d0_bottom) =
                project_facade_point(viewpoint, Point3::new(p0.x_m, p0.y_m, z_bottom));
            let (uv1_bottom, d1_bottom) =
                project_facade_point(viewpoint, Point3::new(p1.x_m, p1.y_m, z_bottom));
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
                && is_slab_layer_active(ctx, slab, *layer_idx - 1);
            let next_layer_active = *layer_idx + 1 < slab.layers.len()
                && is_slab_layer_active(ctx, slab, *layer_idx + 1);
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
