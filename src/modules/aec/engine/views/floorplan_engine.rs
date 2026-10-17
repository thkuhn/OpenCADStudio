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
    DepthInterval, PaperMm, Point2, ValidationError,
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
            let outer = slab.polygon.clone();

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

            let ring = vec![
                Point2::new(p0.x, p0.y),
                Point2::new(p1.x, p1.y),
                Point2::new(p2.x, p2.y),
                Point2::new(p3.x, p3.y),
            ];

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

            let op_source = SourceReference {
                element: op.element_ref,
                component_id: None,
                feature: Some(FeatureKey::OpeningSill),
                display_component_id: None,
                xref_source: wall.xref_source.clone(),
            };
            let op_sources = SourceSet::new(vec![op_source])?;

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
            *primitive_counter += 1;
            primitives.push(DerivedPrimitive {
                id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                role: PrimitiveRole::CutBoundary,
                geometry: Geometry2d::Line {
                    start: Point2::new(j_left_in.x, j_left_in.y),
                    end: Point2::new(j_left_out.x, j_left_out.y),
                },
                sources: op_sources.clone(),
                depth: DepthInterval::cut_plane(),
                style: line_style.clone(),
                edit_handles: Vec::new(),
            });

            // Right Jamb
            *primitive_counter += 1;
            primitives.push(DerivedPrimitive {
                id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                role: PrimitiveRole::CutBoundary,
                geometry: Geometry2d::Line {
                    start: Point2::new(j_right_in.x, j_right_in.y),
                    end: Point2::new(j_right_out.x, j_right_out.y),
                },
                sources: op_sources.clone(),
                depth: DepthInterval::cut_plane(),
                style: line_style.clone(),
                edit_handles: Vec::new(),
            });

            // Window Sill Lines
            if options.show_window_sills && matches!(op.kind, OpeningKind::Window) {
                *primitive_counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                    role: PrimitiveRole::ProjectedEdge,
                    geometry: Geometry2d::Line {
                        start: Point2::new(j_left_out.x, j_left_out.y),
                        end: Point2::new(j_right_out.x, j_right_out.y),
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
                        start: Point2::new(j_left_in.x, j_left_in.y),
                        end: Point2::new(j_right_in.x, j_right_in.y),
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

                // Leaf Line
                *primitive_counter += 1;
                primitives.push(DerivedPrimitive {
                    id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                    role: PrimitiveRole::Symbol,
                    geometry: Geometry2d::Line {
                        start: Point2::new(hinge_pt.x, hinge_pt.y),
                        end: Point2::new(leaf_end.x, leaf_end.y),
                    },
                    sources: op_sources.clone(),
                    depth: DepthInterval::cut_plane(),
                    style: line_style.clone(),
                    edit_handles: Vec::new(),
                });

                // Swing Arc
                let r = op.width;
                if r > 0.1 {
                    let start_ang = (-normal.y).atan2(-normal.x);
                    *primitive_counter += 1;
                    primitives.push(DerivedPrimitive {
                        id: PrimitiveId::new(model_view.id, 0, *primitive_counter),
                        role: PrimitiveRole::Symbol,
                        geometry: Geometry2d::CircularArc {
                            center: Point2::new(hinge_pt.x, hinge_pt.y),
                            radius: super::PositiveMetres::new(r)?,
                            start_rad: start_ang,
                            sweep_rad: 1.5707963267948966,
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
}
