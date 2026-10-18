//! 2D Floor Plan and Reflected Ceiling Plan (RCP) Derivation Engine.
//!
//! Slices through multi-layer walls, slabs, and openings at a horizontal cut plane (Z = z_cut),
//! extracting horizontal cross-sections (CutBoundary, CutFill with material hatches),
//! opening jambs, sill lines, swing arcs, and slab edges with full provenance.

use super::model_view::{DetailLevel, ModelView};
use super::provenance::{
    DerivedPrimitive, FeatureKey, Geometry2d, PrimitiveId, PrimitiveRole, ResolvedStyle, Rgba8,
    SourceReference, SourceSet,
};
use super::scene_collector::{CollectedOpening, CollectedWall, SceneGeometryCollector};
use super::viewpoint::{ViewKind, Viewpoint};
use super::{
    DepthInterval, PaperMm, Point2, Point3, ValidationError,
};
use crate::modules::aec::engine::openings::OpeningKind;
use crate::modules::aec::engine::opening_style::HingeSide;

const EPS: f64 = 1e-6;

#[derive(Clone, Debug)]
pub struct FloorPlanEngineOptions {
    pub structural_only: bool,
    pub include_openings: bool,
    pub show_door_swings: bool,
    pub show_window_sills: bool,
    pub detail_level: DetailLevel,
}

impl Default for FloorPlanEngineOptions {
    fn default() -> Self {
        Self {
            structural_only: false,
            include_openings: true,
            show_door_swings: true,
            show_window_sills: true,
            detail_level: DetailLevel::Fine,
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

    let mut primitives = Vec::new();
    let mut primitive_counter: u32 = 0;

    // 1. Process Slabs (Background / Surface edges)
    for slab in &scene.slabs {
        if model_view.excluded_elements.contains(&slab.element_ref) {
            continue;
        }
        let (slab_z_min, slab_z_max) = slab.z_range();
        if slab_z_max < z_min || slab_z_min > z_max {
            continue;
        }

        // Slab below cut plane is visible in floor plan
        if slab.polygon.len() >= 3 && slab_z_min <= z_cut {
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

            let style = ResolvedStyle {
                line_weight_mm: PaperMm::new(0.25)?,
                line_color: Some(Rgba8::new(128, 128, 128, 255)),
                line_type: None,
                fill_color: None,
                hatch_pattern: None,
                hatch_scale: None,
                hatch_angle_rad: None,
                layer_name: Some("AEC_SLAB_CONTOUR".to_string()),
            };

            primitive_counter += 1;
            primitives.push(DerivedPrimitive {
                id: PrimitiveId::new(model_view.id, 0, primitive_counter),
                role: PrimitiveRole::ProjectedEdge,
                geometry: Geometry2d::Polyline {
                    vertices: outer,
                    closed: true,
                },
                sources,
                depth: DepthInterval::new(0.0, (z_cut - slab_z_max).max(0.0))?,
                style,
                edit_handles: Vec::new(),
            });
        }
    }

    // 2. Process Walls (Horizontally sliced at z_cut)
    for wall in &scene.walls {
        if model_view.excluded_elements.contains(&wall.element_ref) {
            continue;
        }
        if wall.top_z < z_cut - EPS || wall.base_z > z_cut + EPS {
            continue;
        }

        slice_wall_horizontal(
            wall,
            z_cut,
            viewpoint,
            model_view,
            options,
            &mut primitives,
            &mut primitive_counter,
        )?;
    }

    Ok(primitives)
}

fn slice_wall_horizontal(
    wall: &CollectedWall,
    z_cut: f64,
    viewpoint: &Viewpoint,
    model_view: &ModelView,
    options: &FloorPlanEngineOptions,
    primitives: &mut Vec<DerivedPrimitive>,
    primitive_counter: &mut u32,
) -> Result<(), ValidationError> {
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

    for layer in &wall.layers {
        if options.structural_only && !layer.is_structural {
            current_offset += layer.thickness;
            continue;
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

        // Emit solid wall pieces
        for (s0, s1) in axis_segments {
            if s1 - s0 < EPS {
                continue;
            }

            let p0 = axis_p0 + axis_dir * s0 + normal * y_min_offset;
            let p1 = axis_p0 + axis_dir * s1 + normal * y_min_offset;
            let p2 = axis_p0 + axis_dir * s1 + normal * y_max_offset;
            let p3 = axis_p0 + axis_dir * s0 + normal * y_max_offset;

            let uv0 = viewpoint.world_to_view(Point3::new(p0.x, p0.y, z_cut)).0;
            let uv1 = viewpoint.world_to_view(Point3::new(p1.x, p1.y, z_cut)).0;
            let uv2 = viewpoint.world_to_view(Point3::new(p2.x, p2.y, z_cut)).0;
            let uv3 = viewpoint.world_to_view(Point3::new(p3.x, p3.y, z_cut)).0;

            let ring = vec![uv0, uv1, uv2, uv3];

            let source = SourceReference {
                element: wall.element_ref,
                component_id: Some(layer.component_id),
                feature: Some(FeatureKey::LayerFace),
                display_component_id: None,
                xref_source: wall.xref_source.clone(),
            };
            let sources = SourceSet::new(vec![source])?;

            // Boundary
            *primitive_counter += 1;
            let boundary_style = ResolvedStyle {
                line_weight_mm: PaperMm::new(if layer.is_structural { 0.35 } else { 0.25 })?,
                line_color: Some(Rgba8::new(0, 0, 0, 255)),
                line_type: None,
                fill_color: None,
                hatch_pattern: None,
                hatch_scale: None,
                hatch_angle_rad: None,
                layer_name: Some("AEC_WALL_CONTOUR".to_string()),
            };

            primitives.push(DerivedPrimitive {
                id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                role: PrimitiveRole::CutBoundary,
                geometry: Geometry2d::Polyline {
                    vertices: ring.clone(),
                    closed: true,
                },
                sources: sources.clone(),
                depth: DepthInterval::cut_plane(),
                style: boundary_style,
                edit_handles: Vec::new(),
            });

            // Hatch fill
            *primitive_counter += 1;
            let hatch_style = ResolvedStyle {
                line_weight_mm: PaperMm::new(0.18)?,
                line_color: Some(Rgba8::new(100, 100, 100, 255)),
                line_type: None,
                fill_color: None,
                hatch_pattern: layer.hatch_override.clone().or_else(|| Some("ANSI31".to_string())),
                hatch_scale: Some(1.0),
                hatch_angle_rad: Some(0.7853981633974483), // 45 deg
                layer_name: Some("AEC_WALL_HATCH".to_string()),
            };

            primitives.push(DerivedPrimitive {
                id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                role: PrimitiveRole::CutFill,
                geometry: Geometry2d::Region {
                    outer: ring,
                    holes: Vec::new(),
                },
                sources,
                depth: DepthInterval::cut_plane(),
                style: hatch_style,
                edit_handles: Vec::new(),
            });
        }
    }

    // 3. Emit Opening details (Jambs, Sill, Swing)
    if options.include_openings {
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

            let line_style = ResolvedStyle {
                line_weight_mm: PaperMm::new(0.25)?,
                line_color: Some(Rgba8::new(0, 0, 200, 255)),
                line_type: None,
                fill_color: None,
                hatch_pattern: None,
                hatch_scale: None,
                hatch_angle_rad: None,
                layer_name: Some("AEC_OPENING".to_string()),
            };

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
                style: line_style.clone(),
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
                style: line_style.clone(),
                edit_handles: Vec::new(),
            });

            let op_sources = SourceSet::new(vec![op_source.clone()])?;

            // Window Sill Lines
            if options.show_window_sills && matches!(op.kind, OpeningKind::Window) {
                *primitive_counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                    role: PrimitiveRole::ProjectedEdge,
                    geometry: Geometry2d::Line {
                        start: uv_j_left_out,
                        end: uv_j_right_out,
                    },
                    sources: op_sources.clone(),
                    depth: DepthInterval::new(0.0, 0.5)?,
                    style: line_style.clone(),
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
                    depth: DepthInterval::new(0.0, 0.5)?,
                    style: line_style.clone(),
                    edit_handles: Vec::new(),
                });
            }

            // Door Swing Arc
            if options.show_door_swings && matches!(op.kind, OpeningKind::Door) {
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
                    style: line_style.clone(),
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
                        style: line_style,
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
    use crate::modules::aec::engine::views::model_view::{DetailPolicy, ViewRevisions};
    use crate::modules::aec::engine::views::scene_collector::{CollectedSlab, CollectedWall, CollectedWallLayer};
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
    fn test_localized_floorplan_projection_with_origin_offset() {
        use crate::modules::aec::engine::openings::{OpeningKind, OpeningReferenceSide};
        use crate::modules::aec::engine::opening_shape::OpeningShape;
        use crate::modules::aec::engine::opening_style::HingeSide;

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
}
