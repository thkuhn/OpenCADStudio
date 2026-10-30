//! Geometry and building component aggregator across native entities and external references (XREFs).
//!
//! Gathers walls, slabs, and openings with their layer snapshots and parametric properties,
//! transforming all coordinates by their host XREF's affine matrix $T_{xref}$ into the master scene coordinate system.

use acadrust::{CadDocument, EntityType, Handle};
use uuid::Uuid;

use crate::modules::aec::engine::opening_shape::OpeningShape;
use crate::modules::aec::engine::opening_style::HingeSide;
use crate::modules::aec::engine::opening_xdata::opening_from_entity;
use crate::modules::aec::engine::join::JunctionOverride;
use crate::modules::aec::engine::openings::{OpeningKind, OpeningReferenceSide};
use crate::modules::aec::engine::owner_index::children_of;
use crate::modules::aec::engine::plan_view::PlanPhase;
use crate::modules::aec::engine::slab::SlabJustification;
use crate::modules::aec::engine::slab_xdata::slab_from_entity;
use crate::modules::aec::engine::wall::WallJustification;
use crate::modules::aec::engine::xdata::{
    read_junction_override_from_entity, wall_from_entity, THROUGH_SPAN_OVERRIDE_END,
};

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
    pub swing_side: crate::modules::aec::engine::openings::SwingSide,
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

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CollectedWallJoinData {
    pub bulges: Vec<f64>,
    pub override_start: Option<JunctionOverride>,
    pub override_end: Option<JunctionOverride>,
    pub override_span: Option<JunctionOverride>,
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
    pub vertical_offset: f64,
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
    pub fn layer_z_ranges(&self) -> Vec<(f64, f64)> {
        let mut current_top = self.base_z + self.total_thickness;
        self.layers
            .iter()
            .map(|layer| {
                let top = current_top + layer.vertical_offset;
                let bottom = top - layer.thickness;
                current_top -= layer.thickness;
                (top, bottom)
            })
            .collect()
    }

    pub fn z_range(&self) -> (f64, f64) {
        let ranges = self.layer_z_ranges();
        if ranges.is_empty() {
            return (self.base_z, self.base_z + self.total_thickness);
        }
        let min_z = ranges
            .iter()
            .map(|(_, bottom)| *bottom)
            .fold(f64::INFINITY, f64::min);
        let max_z = ranges
            .iter()
            .map(|(top, _)| *top)
            .fold(f64::NEG_INFINITY, f64::max);
        (min_z, max_z)
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
    pub(crate) wall_join_data: Vec<Option<CollectedWallJoinData>>,
}

pub fn handle_to_element_id(handle: Handle) -> ElementId {
    let mut bytes = [0u8; 16];
    bytes[8..16].copy_from_slice(&handle.value().to_be_bytes());
    ElementId::from_uuid(uuid::Uuid::from_bytes(bytes))
}

pub fn element_id_to_handle(element_id: ElementId) -> Option<Handle> {
    let uuid = element_id.as_uuid();
    let bytes = uuid.as_bytes();
    if bytes[0..8] == [0u8; 8] {
        let val = u64::from_be_bytes(bytes[8..16].try_into().ok()?);
        if val != 0 {
            Some(Handle::new(val))
        } else {
            None
        }
    } else {
        None
    }
}

impl SceneGeometryCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Computes the 2D bounding box across all walls and slabs in the scene.
    pub fn bounding_box_2d(&self) -> Option<(Point2, Point2)> {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut has_geom = false;

        for wall in &self.walls {
            let (w_min, w_max) = wall.bounding_box_2d();
            if w_min.x_m <= w_max.x_m && w_min.y_m <= w_max.y_m {
                min_x = min_x.min(w_min.x_m);
                min_y = min_y.min(w_min.y_m);
                max_x = max_x.max(w_max.x_m);
                max_y = max_y.max(w_max.y_m);
                has_geom = true;
            }
        }

        for slab in &self.slabs {
            let (s_min, s_max) = slab.bounding_box_2d();
            if s_min.x_m <= s_max.x_m && s_min.y_m <= s_max.y_m {
                min_x = min_x.min(s_min.x_m);
                min_y = min_y.min(s_min.y_m);
                max_x = max_x.max(s_max.x_m);
                max_y = max_y.max(s_max.y_m);
                has_geom = true;
            }
        }

        if has_geom {
            Some((Point2::new(min_x, min_y), Point2::new(max_x, max_y)))
        } else {
            None
        }
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
            let elem_ref = ElementRef::new(model_id, handle_to_element_id(handle));

            if let Some(wall) = wall_from_entity(entity) {
                if let EntityType::LwPolyline(pl) = entity {
                    let last_vertex = pl.vertices.len().saturating_sub(1);
                    let xy_determinant = xref_source.as_ref().map_or(1.0, |xref| {
                        xref.transform[0] * xref.transform[5]
                            - xref.transform[1] * xref.transform[4]
                    });
                    let bulge_sign = if xy_determinant < 0.0 { -1.0 } else { 1.0 };
                    let join_data = CollectedWallJoinData {
                        bulges: pl.vertices.iter().map(|v| v.bulge * bulge_sign).collect(),
                        override_start: read_junction_override_from_entity(entity, 0),
                        override_end: read_junction_override_from_entity(entity, last_vertex),
                        override_span: read_junction_override_from_entity(
                            entity,
                            THROUGH_SPAN_OVERRIDE_END,
                        ),
                    };
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
                        let (v0, v1) = (&pl.vertices[0].location, &pl.vertices[pl.vertices.len() - 1].location);
                        let mid_local_x = (v0.x + v1.x) * 0.5;
                        let mid_local_y = (v0.y + v1.y) * 0.5;

                        let raw_base = if wall.base_plane_id.is_some() || !wall.base_facets.is_empty() {
                            wall.base_z_at_xy(mid_local_x, mid_local_y)
                        } else if wall.base_origin[2].abs() > 1e-6 {
                            wall.base_origin[2]
                        } else if pl.elevation.abs() > 1e-6 {
                            pl.elevation + wall.base_offset
                        } else {
                            wall.base_offset
                        };

                        let raw_top = if wall.top_plane_id.is_some() || !wall.top_facets.is_empty() {
                            wall.top_z_at_xy(mid_local_x, mid_local_y)
                        } else {
                            raw_base + wall.height + wall.top_offset
                        };

                        let (base_z, top_z) = if let Some(xref) = &xref_source {
                            let (_, _, z_b) = xref.transform_coords(0.0, 0.0, raw_base);
                            let (_, _, z_t) = xref.transform_coords(0.0, 0.0, raw_top);
                            (z_b, z_t)
                        } else {
                            (raw_base, raw_top)
                        };

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

                            let op_elem_ref = ElementRef::new(model_id, handle_to_element_id(child));
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
                                swing_side: op.swing_side,
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
                        self.wall_join_data.push(Some(join_data));
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
                        let raw_base = if slab.base_origin[2].abs() > 1e-6 {
                            slab.base_origin[2]
                        } else if pl.elevation.abs() > 1e-6 {
                            pl.elevation + slab.base_offset
                        } else {
                            slab.base_offset
                        };
                        let base_z = if let Some(xref) = &xref_source {
                            let (_, _, z_t) = xref.transform_coords(0.0, 0.0, raw_base);
                            z_t
                        } else {
                            raw_base
                        };

                        let total_thickness = slab.total_thickness();
                        let collected_layers: Vec<CollectedSlabLayer> = slab
                            .layers
                            .iter()
                            .map(|l| {
                                let is_structural = matches!(l.function, crate::modules::aec::engine::wall_style::LayerFunction::Structural);
                                CollectedSlabLayer {
                                    material: l.material.clone(),
                                    thickness: l.thickness,
                                    vertical_offset: l.vertical_offset,
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
        let mut walls = Vec::new();
        let mut wall_join_data = Vec::new();
        for (index, wall) in self.walls.iter().enumerate() {
            if wall.top_z >= z_min && wall.base_z <= z_max {
                walls.push(wall.clone());
                wall_join_data.push(self.wall_join_data.get(index).cloned().unwrap_or(None));
            }
        }
        let slabs = self
            .slabs
            .iter()
            .filter(|slab| {
                let (slab_min, slab_max) = slab.z_range();
                slab_max >= z_min && slab_min <= z_max
            })
            .cloned()
            .collect();
        Self {
            walls,
            slabs,
            wall_join_data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::xdata::ExtendedDataRecord;
    use crate::modules::aec::engine::control_plane::ControlPlaneFacet;
    use crate::modules::aec::engine::slab::{Slab, SlabLayer};
    use crate::modules::aec::engine::slab_xdata::slab_record_for_slab;
    use crate::modules::aec::engine::wall::Wall;
    use crate::modules::aec::engine::wall_style::LayerFunction;
    use crate::modules::aec::engine::xdata::{wall_record_for_wall, AEC_APPID};
    use crate::scene::Scene;

    #[test]
    fn test_wall_elevation_and_plane_offsets_collection() {
        let mut scene = Scene::new();
        let model_id = ModelId::new();

        // 1. Wall with polyline elevation = 2.80 and base_origin = 0.0
        let mut pl1 = LwPolyline::new();
        pl1.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl1.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        pl1.elevation = 2.80;
        let mut wall1 = Wall::new("Standard", 2.80, 0);
        wall1.base_origin = [0.0, 0.0, 0.0];
        let mut ent1 = EntityType::LwPolyline(pl1);
        let mut rec1 = ExtendedDataRecord::new(AEC_APPID);
        rec1.values = wall_record_for_wall(&wall1);
        ent1.common_mut().extended_data.add_record(rec1);
        scene.add_entity(ent1);

        // 2. Wall with polyline elevation = 2.80, base_offset = 0.10, top_offset = -0.05
        let mut pl2 = LwPolyline::new();
        pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 5.0)));
        pl2.add_vertex(LwVertex::new(Vector2::new(10.0, 5.0)));
        pl2.elevation = 2.80;
        let mut wall2 = Wall::new("Standard", 2.80, 0);
        wall2.base_origin = [0.0, 0.0, 0.0];
        wall2.base_offset = 0.10;
        wall2.top_offset = -0.05;
        let mut ent2 = EntityType::LwPolyline(pl2);
        let mut rec2 = ExtendedDataRecord::new(AEC_APPID);
        rec2.values = wall_record_for_wall(&wall2);
        ent2.common_mut().extended_data.add_record(rec2);
        scene.add_entity(ent2);

        // 3. Wall with sloped top plane (top plane facets)
        let mut pl3 = LwPolyline::new();
        pl3.add_vertex(LwVertex::new(Vector2::new(0.0, 10.0)));
        pl3.add_vertex(LwVertex::new(Vector2::new(10.0, 10.0)));
        pl3.elevation = 0.0;
        let mut wall3 = Wall::new("RoofWall", 4.0, 0);
        wall3.base_origin = [0.0, 10.0, 0.0];
        wall3.top_plane_id = Some(uuid::Uuid::new_v4());
        // Sloped roof facet: at x = 0: z = 5.0, at x = 10: z = 3.0 -> mid at x = 5 is z = 4.0
        let roof_facet = ControlPlaneFacet::new(
            "Roof",
            vec![
                [0.0, 5.0, 5.0],
                [10.0, 5.0, 3.0],
                [10.0, 15.0, 3.0],
                [0.0, 15.0, 5.0],
            ],
        );
        wall3.top_facets = vec![roof_facet];
        let mut ent3 = EntityType::LwPolyline(pl3);
        let mut rec3 = ExtendedDataRecord::new(AEC_APPID);
        rec3.values = wall_record_for_wall(&wall3);
        ent3.common_mut().extended_data.add_record(rec3);
        scene.add_entity(ent3);

        let mut slab_poly = LwPolyline::new();
        slab_poly.add_vertex(LwVertex::new(Vector2::new(0.0, 20.0)));
        slab_poly.add_vertex(LwVertex::new(Vector2::new(10.0, 20.0)));
        slab_poly.add_vertex(LwVertex::new(Vector2::new(10.0, 30.0)));
        slab_poly.add_vertex(LwVertex::new(Vector2::new(0.0, 30.0)));
        slab_poly.is_closed = true;
        let mut slab = Slab::new("Floor", 0);
        slab.layers = vec![
            SlabLayer::new("Finish", 0.02, LayerFunction::Finish),
            SlabLayer::new("Concrete", 0.28, LayerFunction::Structural),
        ];
        slab.layers[0].vertical_offset = 0.005;
        let mut slab_entity = EntityType::LwPolyline(slab_poly);
        let mut slab_record = ExtendedDataRecord::new(AEC_APPID);
        slab_record.values = slab_record_for_slab(&slab);
        slab_entity.common_mut().extended_data.add_record(slab_record);
        scene.add_entity(slab_entity);

        let collector = SceneGeometryCollector::collect_from_document(&scene.document, model_id);
        assert_eq!(collector.walls.len(), 3);
        assert_eq!(collector.slabs.len(), 1);
        assert!((collector.slabs[0].layers[0].vertical_offset - 0.005).abs() < 1e-9);

        // Verify Wall 1: base = 2.80, top = 5.60
        let w1 = &collector.walls[0];
        assert!((w1.base_z - 2.80).abs() < 1e-6, "w1 base_z expected 2.80, got {}", w1.base_z);
        assert!((w1.top_z - 5.60).abs() < 1e-6, "w1 top_z expected 5.60, got {}", w1.top_z);

        // Verify Wall 2: base = 2.90, top = 5.65
        let w2 = &collector.walls[1];
        assert!((w2.base_z - 2.90).abs() < 1e-6, "w2 base_z expected 2.90, got {}", w2.base_z);
        assert!((w2.top_z - 5.65).abs() < 1e-6, "w2 top_z expected 5.65, got {}", w2.top_z);

        // Verify Wall 3: base = 0.0, top = 4.0 at midpoint (x=5.0)
        let w3 = &collector.walls[2];
        assert!((w3.base_z - 0.0).abs() < 1e-6, "w3 base_z expected 0.0, got {}", w3.base_z);
        assert!((w3.top_z - 4.0).abs() < 1e-6, "w3 top_z expected 4.0, got {}", w3.top_z);
    }

    #[test]
    fn test_collect_wall_join_data_from_mirrored_xref() {
        let mut scene = Scene::new();
        let model_id = ModelId::new();
        let owner = Handle::new(900);

        let mut pl = LwPolyline::new();
        let mut v0 = LwVertex::new(Vector2::new(0.0, 0.0));
        v0.bulge = 0.25;
        pl.add_vertex(v0);
        let mut v1 = LwVertex::new(Vector2::new(4.0, 0.0));
        v1.bulge = -0.5;
        pl.add_vertex(v1);
        pl.add_vertex(LwVertex::new(Vector2::new(8.0, 0.0)));

        let wall = Wall::new("Standard", 2.8, 0);
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record_for_wall(&wall);
        entity.common_mut().extended_data.add_record(record);
        let handle = scene.add_entity(entity);
        scene
            .document
            .get_entity_mut(handle)
            .unwrap()
            .common_mut()
            .owner_handle = owner;

        use crate::modules::aec::engine::join::{JoinOverrideStyle, JunctionOverride};
        use crate::modules::aec::engine::xdata::write_junction_override;
        let start_override = JunctionOverride {
            default_style: Some(JoinOverrideStyle::FarFace),
            ..Default::default()
        };
        let end_override = JunctionOverride {
            default_style: Some(JoinOverrideStyle::NearFace),
            ..Default::default()
        };
        let span_override = JunctionOverride {
            default_style: Some(JoinOverrideStyle::NoExtend),
            ..Default::default()
        };
        assert!(write_junction_override(&mut scene, handle, 0, &start_override));
        assert!(write_junction_override(&mut scene, handle, 2, &end_override));
        assert!(write_junction_override(
            &mut scene,
            handle,
            THROUGH_SPAN_OVERRIDE_END,
            &span_override,
        ));

        let xref = XrefSourceRef::from_insert_transform(
            "MirroredStorey",
            Some("storey.dwg".to_string()),
            [20.0, 3.0, 2.0],
            [-2.0, 2.0, 3.0],
            0.0,
        );
        let mut collector = SceneGeometryCollector::new();
        collector.collect_scope(&scene.document, model_id, Some(owner), Some(xref));

        assert_eq!(collector.walls.len(), 1);
        assert_eq!(
            collector.walls[0].axis_points,
            vec![Point2::new(20.0, 3.0), Point2::new(12.0, 3.0), Point2::new(4.0, 3.0)]
        );
        assert!((collector.walls[0].base_z - 2.0).abs() < 1e-9);

        let join_data = collector.wall_join_data[0].as_ref().unwrap();
        assert_eq!(join_data.bulges, vec![-0.25, 0.5, 0.0]);
        assert_eq!(join_data.override_start, Some(start_override));
        assert_eq!(join_data.override_end, Some(end_override));
        assert_eq!(join_data.override_span, Some(span_override));
    }

    #[test]
    fn test_xref_insert_shifts_slab_base_without_reversing_layers() {
        let mut scene = Scene::new();
        let model_id = ModelId::new();
        let mut block = acadrust::BlockRecord::new("OG");
        let owner = scene.document.allocate_handle();
        block.handle = owner;
        block.flags.is_xref = true;
        block.xref_path = "OG.dwg".to_string();
        let _ = scene.document.block_records.add(block);

        let mut polygon = LwPolyline::new();
        polygon.add_vertex(LwVertex::new(Vector2::new(1.0, 2.0)));
        polygon.add_vertex(LwVertex::new(Vector2::new(6.0, 2.0)));
        polygon.add_vertex(LwVertex::new(Vector2::new(6.0, 5.0)));
        polygon.add_vertex(LwVertex::new(Vector2::new(1.0, 5.0)));
        polygon.is_closed = true;
        let mut slab = Slab::new("Floor", 1);
        slab.layers = vec![
            SlabLayer::new("Finish", 0.02, LayerFunction::Finish),
            SlabLayer::new("Insulation", 0.08, LayerFunction::Insulation),
            SlabLayer::new("Concrete", 0.20, LayerFunction::Structural),
        ];
        slab.layers[0].vertical_offset = -0.01;
        slab.layers[2].vertical_offset = -0.02;
        let mut entity = EntityType::LwPolyline(polygon);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = slab_record_for_slab(&slab);
        entity.common_mut().extended_data.add_record(record);
        let handle = scene.add_entity(entity);
        scene
            .document
            .get_entity_mut(handle)
            .unwrap()
            .common_mut()
            .owner_handle = owner;

        let insert = acadrust::entities::Insert::new(
            "OG",
            acadrust::types::Vector3::new(10.0, 4.0, 2.80),
        );
        scene.add_entity(EntityType::Insert(insert));

        let collector = SceneGeometryCollector::collect_from_document(&scene.document, model_id);
        assert_eq!(collector.slabs.len(), 1);
        let collected = &collector.slabs[0];
        assert!((collected.base_z - 2.80).abs() < 1e-9);
        assert!((collected.layers[0].vertical_offset + 0.01).abs() < 1e-9);
        assert!((collected.layers[2].vertical_offset + 0.02).abs() < 1e-9);
        assert_eq!(collected.polygon[0], Point2::new(11.0, 6.0));

        let ranges = collected.layer_z_ranges();
        assert!((ranges[0].0 - 3.09).abs() < 1e-9 && (ranges[0].1 - 3.07).abs() < 1e-9);
        assert!((ranges[1].0 - 3.08).abs() < 1e-9 && (ranges[1].1 - 3.00).abs() < 1e-9);
        assert!(
            (ranges[2].0 - 2.98).abs() < 1e-9 && (ranges[2].1 - 2.78).abs() < 1e-9,
            "XREF height must not reverse the stack or clamp the core to base_z"
        );
        assert!(ranges[0].1 > ranges[2].0);
    }
}
