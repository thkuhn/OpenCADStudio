//! Geometry and building component aggregator across native entities and external references (XREFs).
//!
//! Gathers walls, slabs, and openings with their layer snapshots and parametric properties,
//! transforming all coordinates by their host XREF's affine matrix $T_{xref}$ into the master scene coordinate system.

use acadrust::{CadDocument, EntityType, Handle};
use uuid::Uuid;

use crate::modules::aec::engine::opening_shape::OpeningShape;
use crate::modules::aec::engine::opening_style::HingeSide;
use crate::modules::aec::engine::opening_xdata::opening_from_entity;
use crate::modules::aec::engine::openings::{OpeningKind, OpeningReferenceSide};
use crate::modules::aec::engine::owner_index::children_of;
use crate::modules::aec::engine::plan_view::PlanPhase;
use crate::modules::aec::engine::slab::SlabJustification;
use crate::modules::aec::engine::slab_xdata::slab_from_entity;
use crate::modules::aec::engine::wall::WallJustification;
use crate::modules::aec::engine::xdata::wall_from_entity;

use super::{
    ComponentId, ElementId, ElementRef, ModelId, Point2, Point3, Vector2,
    XrefSourceRef,
};

/// An opening collected and transformed into the master scene space.
#[derive(Clone, Debug, PartialEq)]
pub struct CollectedOpening {
    pub handle: Handle,
    pub element_ref: ElementRef,
    pub xref_source: Option<XrefSourceRef>,
    pub host_wall_handle: Handle,
    pub kind: OpeningKind,
    pub style_id: Option<String>,
    pub distance_along_axis: f64,
    pub width: f64,
    pub height: f64,
    pub sill_height: f64,
    pub spring_height: f64,
    pub shape: OpeningShape,
    pub hinge: HingeSide,
    pub reference_side: OpeningReferenceSide,
    pub cross_axis_offset: f64,
    pub depth: Option<f64>,
    pub world_center: Point3,
    pub axis_span_local: (f64, f64),
}

/// One wall layer snapshot in global coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct CollectedWallLayer {
    pub material: String,
    pub thickness: f64,
    pub function: String,
    pub axis_offset: f64,
    pub bottom_offset: f64,
    pub top_offset: f64,
    pub hatch_override: Option<String>,
    pub layer_id: Uuid,
    pub component_id: ComponentId,
    pub is_structural: bool,
}

/// A multi-layer wall collected and transformed into the master scene space.
#[derive(Clone, Debug, PartialEq)]
pub struct CollectedWall {
    pub handle: Handle,
    pub element_ref: ElementRef,
    pub xref_source: Option<XrefSourceRef>,
    pub style_id: String,
    pub height: f64,
    pub base_z: f64,
    pub top_z: f64,
    pub axis_points: Vec<Point2>,
    pub layers: Vec<CollectedWallLayer>,
    pub total_thickness: f64,
    pub justification: WallJustification,
    pub openings: Vec<CollectedOpening>,
    pub phase: PlanPhase,
}

impl CollectedWall {
    pub fn length(&self) -> f64 {
        if self.axis_points.len() < 2 {
            return 0.0;
        }
        self.axis_points
            .windows(2)
            .map(|w| w[0].distance_to(w[1]))
            .sum()
    }

    pub fn z_range(&self) -> (f64, f64) {
        (self.base_z, self.top_z)
    }

    pub fn bounding_box_2d(&self) -> (Point2, Point2) {
        if self.axis_points.is_empty() {
            return (Point2::new(0.0, 0.0), Point2::new(0.0, 0.0));
        }
        let half_t = self.total_thickness * 0.5 + 0.1;
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for p in &self.axis_points {
            min_x = min_x.min(p.x_m - half_t);
            min_y = min_y.min(p.y_m - half_t);
            max_x = max_x.max(p.x_m + half_t);
            max_y = max_y.max(p.y_m + half_t);
        }
        (Point2::new(min_x, min_y), Point2::new(max_x, max_y))
    }

    /// Returns the signed offset and thickness of each layer relative to the baseline axis.
    pub fn layer_offsets(&self) -> Vec<(f64, f64)> {
        let just_offset = self.justification.offset(self.total_thickness);
        let mut current_offset = -self.total_thickness * 0.5 + just_offset;
        let mut offsets = Vec::with_capacity(self.layers.len());
        for layer in &self.layers {
            offsets.push((current_offset, layer.thickness));
            current_offset += layer.thickness;
        }
        offsets
    }
}

/// One slab layer snapshot in global coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct CollectedSlabLayer {
    pub material: String,
    pub thickness: f64,
    pub function: String,
    pub hatch_override: Option<String>,
    pub layer_id: Uuid,
    pub component_id: ComponentId,
    pub is_structural: bool,
}

/// A multi-layer slab collected and transformed into the master scene space.
#[derive(Clone, Debug, PartialEq)]
pub struct CollectedSlab {
    pub handle: Handle,
    pub element_ref: ElementRef,
    pub xref_source: Option<XrefSourceRef>,
    pub style_id: String,
    pub base_z: f64,
    pub total_thickness: f64,
    pub polygon: Vec<Point2>,
    pub layers: Vec<CollectedSlabLayer>,
    pub justification: SlabJustification,
    pub openings: Vec<Vec<Point2>>,
    pub phase: PlanPhase,
}

impl CollectedSlab {
    pub fn z_range(&self) -> (f64, f64) {
        (self.base_z, self.base_z + self.total_thickness)
    }

    pub fn bounding_box_2d(&self) -> (Point2, Point2) {
        if self.polygon.is_empty() {
            return (Point2::new(0.0, 0.0), Point2::new(0.0, 0.0));
        }
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for p in &self.polygon {
            min_x = min_x.min(p.x_m);
            min_y = min_y.min(p.y_m);
            max_x = max_x.max(p.x_m);
            max_y = max_y.max(p.y_m);
        }
        (Point2::new(min_x, min_y), Point2::new(max_x, max_y))
    }
}

/// Aggregator collecting all 3D AEC elements from model space and referenced XREF storeys.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneGeometryCollector {
    pub walls: Vec<CollectedWall>,
    pub slabs: Vec<CollectedSlab>,
}

impl SceneGeometryCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Aggregates all AEC elements from `doc` across native entities and all XREF block inserts.
    pub fn collect_from_document(doc: &CadDocument, model_id: ModelId) -> Self {
        let mut collector = Self::new();

        // 1. Native entities in model space (xref_source = None)
        collector.collect_scope(doc, model_id, None, None);

        // 2. Traversal of XREF block inserts
        for entity in doc.entities() {
            let EntityType::Insert(ins) = entity else {
                continue;
            };
            let block_name = &ins.block_name;
            let Some(block_record) = doc.block_records.get(block_name) else {
                continue;
            };

            // Derive affine 4x4 matrix from insertion point, scale, and rotation (in radians)
            let insert_point = [ins.insert_point.x, ins.insert_point.y, ins.insert_point.z];
            let scale = [ins.x_scale(), ins.y_scale(), ins.z_scale()];
            let rotation_rad = ins.rotation;

            let source_path = if block_record.flags.is_xref || block_record.flags.is_xref_overlay {
                if block_record.xref_path.is_empty() {
                    None
                } else {
                    Some(block_record.xref_path.clone())
                }
            } else {
                None
            };

            let xref_source = XrefSourceRef::from_insert_transform(
                block_name.clone(),
                source_path,
                insert_point,
                scale,
                rotation_rad,
            );

            // Collect all entities belonging to this block record
            collector.collect_scope(doc, model_id, Some(block_record.handle), Some(xref_source));
        }

        collector
    }

    fn collect_scope(
        &mut self,
        doc: &CadDocument,
        model_id: ModelId,
        owner_filter: Option<Handle>,
        xref_source: Option<XrefSourceRef>,
    ) {
        // Collect raw walls
        for entity in doc.entities() {
            if let Some(owner) = owner_filter {
                if entity.common().owner_handle != owner {
                    continue;
                }
            } else {
                // In native space, skip block-owned entities
                if entity.common().owner_handle != doc.header.model_space_block_handle
                    && entity.common().owner_handle != Handle::new(0)
                {
                    continue;
                }
            }

            let handle = entity.common().handle;
            let elem_ref = ElementRef::new(model_id, ElementId::new());

            if let Some(wall) = wall_from_entity(entity) {
                if let EntityType::LwPolyline(pl) = entity {
                    let mut axis_points = Vec::new();
                    for v in &pl.vertices {
                        let p = Point2::new(v.location.x, v.location.y);
                        let transformed_p = if let Some(xref) = &xref_source {
                            xref.transform_point2(p)
                        } else {
                            p
                        };
                        axis_points.push(transformed_p);
                    }

                    if axis_points.len() >= 2 {
                        let mut base_z = wall.base_origin[2];
                        let mut top_z = base_z + wall.height;
                        if let Some(xref) = &xref_source {
                            let (_, _, z_t) = xref.transform_coords(0.0, 0.0, base_z);
                            base_z = z_t;
                            top_z = base_z + wall.height;
                        }

                        let total_thickness: f64 = wall.layers.iter().map(|l| l.thickness).sum();

                        let collected_layers: Vec<CollectedWallLayer> = wall
                            .layers
                            .iter()
                            .map(|l| {
                                let is_structural = l.function.to_lowercase().contains("struct");
                                CollectedWallLayer {
                                    material: l.material.clone(),
                                    thickness: l.thickness,
                                    function: l.function.clone(),
                                    axis_offset: l.axis_offset,
                                    bottom_offset: l.bottom_offset,
                                    top_offset: l.top_offset,
                                    hatch_override: l.hatch_override.clone(),
                                    layer_id: l.layer_id,
                                    component_id: ComponentId::new(),
                                    is_structural,
                                }
                            })
                            .collect();

                        // Collect openings for this wall
                        let mut collected_openings = Vec::new();
                        for child in children_of(doc, handle) {
                            let Some(op_entity) = doc.get_entity(child) else {
                                continue;
                            };
                            let Some(op) = opening_from_entity(op_entity, child) else {
                                continue;
                            };

                            let op_elem_ref = ElementRef::new(model_id, ElementId::new());
                            let (s_lo, s_hi) = op.axis_span();

                            // Compute world center of opening
                            let p_center_local = if axis_points.len() >= 2 {
                                let seg_vec = Vector2::new(
                                    axis_points[1].x_m - axis_points[0].x_m,
                                    axis_points[1].y_m - axis_points[0].y_m,
                                );
                                let dir = seg_vec.normalized().unwrap_or(Vector2::new(1.0, 0.0));
                                let s_mid = (s_lo + s_hi) * 0.5;
                                Point3::new(
                                    axis_points[0].x_m + dir.x * s_mid,
                                    axis_points[0].y_m + dir.y * s_mid,
                                    base_z + op.sill_height + op.height * 0.5,
                                )
                            } else {
                                Point3::new(0.0, 0.0, base_z)
                            };

                            collected_openings.push(CollectedOpening {
                                handle: op.handle,
                                element_ref: op_elem_ref,
                                xref_source: xref_source.clone(),
                                host_wall_handle: handle,
                                kind: op.kind,
                                style_id: op.style_id.clone(),
                                distance_along_axis: op.distance_along_axis,
                                width: op.width,
                                height: op.height,
                                sill_height: op.sill_height,
                                spring_height: op.spring_height,
                                shape: op.shape,
                                hinge: op.hinge,
                                reference_side: op.reference_side,
                                cross_axis_offset: op.cross_axis_offset,
                                depth: op.depth,
                                world_center: p_center_local,
                                axis_span_local: (s_lo, s_hi),
                            });
                        }

                        self.walls.push(CollectedWall {
                            handle,
                            element_ref: elem_ref,
                            xref_source: xref_source.clone(),
                            style_id: wall.style_id.clone(),
                            height: wall.height,
                            base_z,
                            top_z,
                            axis_points,
                            layers: collected_layers,
                            total_thickness,
                            justification: wall.justification,
                            openings: collected_openings,
                            phase: wall.phase,
                        });
                    }
                }
            } else if let Some(slab) = slab_from_entity(entity) {
                if let EntityType::LwPolyline(pl) = entity {
                    let mut polygon = Vec::new();
                    for v in &pl.vertices {
                        let p = Point2::new(v.location.x, v.location.y);
                        let transformed_p = if let Some(xref) = &xref_source {
                            xref.transform_point2(p)
                        } else {
                            p
                        };
                        polygon.push(transformed_p);
                    }

                    if polygon.len() >= 3 {
                        let mut base_z = slab.base_origin[2];
                        if let Some(xref) = &xref_source {
                            let (_, _, z_t) = xref.transform_coords(0.0, 0.0, base_z);
                            base_z = z_t;
                        }

                        let total_thickness = slab.total_thickness();
                        let collected_layers: Vec<CollectedSlabLayer> = slab
                            .layers
                            .iter()
                            .map(|l| {
                                let is_structural = matches!(l.function, crate::modules::aec::engine::wall_style::LayerFunction::Structural);
                                CollectedSlabLayer {
                                    material: l.material.clone(),
                                    thickness: l.thickness,
                                    function: format!("{:?}", l.function),
                                    hatch_override: l.hatch_override.clone(),
                                    layer_id: l.layer_id,
                                    component_id: ComponentId::new(),
                                    is_structural,
                                }
                            })
                            .collect();

                        // Collect slab openings
                        let mut collected_slab_openings = Vec::new();
                        for op_handle in &slab.opening_handles {
                            if let Some(op_entity) = doc.get_entity(*op_handle) {
                                if let EntityType::LwPolyline(op_pl) = op_entity {
                                    let mut op_poly = Vec::new();
                                    for v in &op_pl.vertices {
                                        let p = Point2::new(v.location.x, v.location.y);
                                        let trans_p = if let Some(xref) = &xref_source {
                                            xref.transform_point2(p)
                                        } else {
                                            p
                                        };
                                        op_poly.push(trans_p);
                                    }
                                    if op_poly.len() >= 3 {
                                        collected_slab_openings.push(op_poly);
                                    }
                                }
                            }
                        }

                        self.slabs.push(CollectedSlab {
                            handle,
                            element_ref: elem_ref,
                            xref_source: xref_source.clone(),
                            style_id: slab.style_id.clone(),
                            base_z,
                            total_thickness,
                            polygon,
                            layers: collected_layers,
                            justification: slab.justification,
                            openings: collected_slab_openings,
                            phase: slab.phase,
                        });
                    }
                }
            }
        }
    }

    /// Filters collected geometry by vertical Z range.
    pub fn filter_by_z_range(&self, z_min: f64, z_max: f64) -> Self {
        let walls = self
            .walls
            .iter()
            .filter(|w| w.top_z >= z_min && w.base_z <= z_max)
            .cloned()
            .collect();
        let slabs = self
            .slabs
            .iter()
            .filter(|s| (s.base_z + s.total_thickness) >= z_min && s.base_z <= z_max)
            .cloned()
            .collect();
        Self { walls, slabs }
    }
}
