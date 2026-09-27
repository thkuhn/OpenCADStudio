//! `AEC_SLABOPENING` interactive command for slab cutouts and recesses.
//!
//! Supports:
//! - Host slab selection / point containment hit-testing
//! - Rectangular openings (2 corner points)
//! - Polygonal openings (arbitrary polygon vertex chains)
//! - Circular openings (center + radius point)
//! - Opening types (Stairwell, Shaft, Duct, Chimney, Skylight, Custom)
//! - Depth modes (Through-hole vs Recessed niche)
//! - Associative cutout subtraction from host slab 3D solid and DIN 1356 symbol generation.

use acadrust::entities::{LwPolyline, LwVertex};
use acadrust::types::Vector2;
use acadrust::{EntityType, Handle};
use glam::DVec3;
use std::f64::consts::PI;

use crate::command::{CadCommand, CmdOption, CmdResult, LiveCommandField, LiveCommandProperties, LiveFieldValue, WorkingPlane};
use crate::modules::aec::engine::geometry::signed_area;
use crate::modules::aec::engine::slab_opening::{SlabOpening, SlabOpeningDepth, SlabOpeningKind};
use crate::modules::aec::engine::slab_package::{
    resolve_slab_package, AEC_SLAB_OPENING_LAYER,
};
use crate::modules::aec::engine::slab_regen::regenerate_slab_opening_representation;
use crate::modules::aec::engine::slab_xdata::{
    add_slab_opening_handle, slab_from_entity, slab_opening_from_entity, slab_opening_record,
    write_slab_opening_record,
};
use crate::modules::aec::engine::{self, StyleLibrary};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_SLABOPENING",
        label: "Slab Opening",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/array_rect.svg")),
        event: ModuleEvent::Command("AEC_SLABOPENING".to_string()),
    }
}

/// Shape modes for drawing a slab opening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlabOpeningShapeMode {
    Rectangle,
    Polygon,
    Circle,
}

/// Interactive `AEC_SLABOPENING` command.
pub struct SlabOpeningCommand {
    pub(crate) host_slab: Option<Handle>,
    pub(crate) kind: SlabOpeningKind,
    pub(crate) depth: SlabOpeningDepth,
    pub(crate) shape: SlabOpeningShapeMode,
    pub(crate) vertices: Vec<DVec3>,
    pub(crate) rect_corner: Option<DVec3>,
    pub(crate) circle_center: Option<DVec3>,
    plane: WorkingPlane,
    library: Option<StyleLibrary>,
    pub(crate) last_committed: Option<Handle>,
}

impl SlabOpeningCommand {
    pub fn new() -> Self {
        Self::new_with_library(Some(engine::library::load_or_seed()))
    }

    pub fn new_with_library(library: Option<StyleLibrary>) -> Self {
        Self {
            host_slab: None,
            kind: SlabOpeningKind::Stairwell,
            depth: SlabOpeningDepth::ThroughHole,
            shape: SlabOpeningShapeMode::Rectangle,
            vertices: Vec::new(),
            rect_corner: None,
            circle_center: None,
            plane: WorkingPlane::default(),
            library,
            last_committed: None,
        }
    }

    pub fn with_host_slab(mut self, host_slab: Handle) -> Self {
        self.host_slab = Some(host_slab);
        self
    }

    pub fn with_kind(mut self, kind: SlabOpeningKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn with_depth(mut self, depth: SlabOpeningDepth) -> Self {
        self.depth = depth;
        self
    }

    pub fn with_shape(mut self, shape: SlabOpeningShapeMode) -> Self {
        self.shape = shape;
        self.vertices.clear();
        self.rect_corner = None;
        self.circle_center = None;
        self
    }

    /// Checks if a 2D point lies inside the polygon formed by slab vertices.
    fn point_in_polygon(pt: (f64, f64), poly: &[(f64, f64)]) -> bool {
        if poly.len() < 3 {
            return false;
        }
        let mut inside = false;
        let mut j = poly.len() - 1;
        for i in 0..poly.len() {
            let (xi, yi) = poly[i];
            let (xj, yj) = poly[j];
            let intersect = ((yi > pt.1) != (yj > pt.1))
                && (pt.0 < (xj - xi) * (pt.1 - yi) / (yj - yi) + xi);
            if intersect {
                inside = !inside;
            }
            j = i;
        }
        inside
    }

    /// Finds a slab entity at `world_pt`.
    pub fn find_slab_at_point(&self, scene: &Scene, world_pt: DVec3) -> Option<Handle> {
        let local_pt = self.plane.to_local(world_pt);
        let pt2d = (local_pt.x, local_pt.y);

        for entity in scene.document.entities() {
            let handle = entity.common().handle;
            if let Some(_slab) = slab_from_entity(entity) {
                if let EntityType::LwPolyline(pl) = entity {
                    let poly: Vec<(f64, f64)> = pl
                        .vertices
                        .iter()
                        .map(|v| (v.location.x, v.location.y))
                        .collect();
                    if Self::point_in_polygon(pt2d, &poly) {
                        return Some(handle);
                    }
                }
            }
        }
        // Fallback: return the first slab carrier in the scene if available
        for entity in scene.document.entities() {
            if slab_from_entity(entity).is_some() {
                return Some(entity.common().handle);
            }
        }
        None
    }

    /// Commits an opening boundary into the host slab.
    pub fn commit_opening(
        &mut self,
        scene: &mut Scene,
        boundary: &[(f64, f64)],
    ) -> Result<Handle, String> {
        let Some(host_h) = self.host_slab else {
            return Err("No host slab selected for opening.".to_string());
        };
        let Some(host_entity) = scene.document.get_entity(host_h) else {
            return Err(format!("Host slab {host_h} does not exist."));
        };
        if slab_from_entity(host_entity).is_none() {
            return Err(format!("Entity {host_h} is not a valid slab."));
        }

        if boundary.len() < 3 {
            return Err("An opening requires at least 3 boundary points.".to_string());
        }
        let poly_area = signed_area(boundary).abs();
        if poly_area < 1e-4 {
            return Err("Degenerate opening boundary (area is zero).".to_string());
        }

        // Create opening carrier entity
        let mut pl = LwPolyline::new();
        pl.is_closed = true;
        for (x, y) in boundary {
            pl.add_vertex(LwVertex::new(Vector2::new(*x, *y)));
        }
        let mut entity = self.plane.place_entity(EntityType::LwPolyline(pl));
        entity.common_mut().layer = AEC_SLAB_OPENING_LAYER.to_string();

        let opening = SlabOpening {
            host_slab: host_h,
            kind: self.kind,
            depth: self.depth,
            boundary: boundary.to_vec(),
            derived_handles: Vec::new(),
        };

        let handle = scene.add_entity(entity);
        write_slab_opening_record(&mut scene.document, handle, &opening);
        add_slab_opening_handle(&mut scene.document, host_h, handle);

        // Regenerate opening 2D DIN 1356 symbols and host slab cutout
        let _ = regenerate_slab_opening_representation(scene, handle, self.library.as_ref(), None);

        self.last_committed = Some(handle);
        self.vertices.clear();
        self.rect_corner = None;
        self.circle_center = None;
        scene.bump_geometry();

        Ok(handle)
    }
}

impl Default for SlabOpeningCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl CadCommand for SlabOpeningCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "AEC_SLABOPENING"
    }

    fn prompt(&self) -> String {
        if self.host_slab.is_none() {
            return crate::tr!("aec", "slabopening-select-host");
        }
        match self.shape {
            SlabOpeningShapeMode::Rectangle => {
                if self.rect_corner.is_none() {
                    crate::tr!("aec", "slabopening-rect-corner1", kind = self.kind.as_str())
                } else {
                    crate::tr!("aec", "slabopening-rect-corner2")
                }
            }
            SlabOpeningShapeMode::Polygon => {
                if self.vertices.is_empty() {
                    crate::tr!("aec", "slabopening-poly-start")
                } else if self.vertices.len() < 3 {
                    crate::tr!("aec", "slabopening-poly-next", count = self.vertices.len())
                } else {
                    crate::tr!("aec", "slabopening-poly-close", count = self.vertices.len())
                }
            }
            SlabOpeningShapeMode::Circle => {
                if self.circle_center.is_none() {
                    crate::tr!("aec", "slabopening-circle-center")
                } else {
                    crate::tr!("aec", "slabopening-circle-radius")
                }
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.host_slab.is_none() {
            return vec![CmdOption::new("Select Slab", "S")];
        }
        match self.shape {
            SlabOpeningShapeMode::Rectangle => vec![
                CmdOption::new("Polygon", "P"),
                CmdOption::new("Circle", "C"),
                CmdOption::new("Kind", "K"),
                CmdOption::new("Depth", "D"),
            ],
            SlabOpeningShapeMode::Polygon => {
                if self.vertices.len() >= 3 {
                    vec![
                        CmdOption::new("Close", "C"),
                        CmdOption::new("Undo", "U"),
                        CmdOption::enter("Done"),
                    ]
                } else if !self.vertices.is_empty() {
                    vec![CmdOption::new("Undo", "U")]
                } else {
                    vec![
                        CmdOption::new("Rectangle", "R"),
                        CmdOption::new("Circle", "C"),
                        CmdOption::new("Kind", "K"),
                        CmdOption::new("Depth", "D"),
                    ]
                }
            }
            SlabOpeningShapeMode::Circle => vec![
                CmdOption::new("Rectangle", "R"),
                CmdOption::new("Polygon", "P"),
                CmdOption::new("Kind", "K"),
                CmdOption::new("Depth", "D"),
            ],
        }
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        let mut wires = Vec::new();
        if self.host_slab.is_none() {
            return wires;
        }

        match self.shape {
            SlabOpeningShapeMode::Rectangle => {
                if let Some(c1) = self.rect_corner {
                    let c1_l = self.plane.to_local(c1);
                    let pt_l = self.plane.to_local(pt);
                    let min_x = c1_l.x.min(pt_l.x);
                    let max_x = c1_l.x.max(pt_l.x);
                    let min_y = c1_l.y.min(pt_l.y);
                    let max_y = c1_l.y.max(pt_l.y);

                    let p1 = self.plane.to_world(DVec3::new(min_x, min_y, 0.0)).as_vec3().to_array();
                    let p2 = self.plane.to_world(DVec3::new(max_x, min_y, 0.0)).as_vec3().to_array();
                    let p3 = self.plane.to_world(DVec3::new(max_x, max_y, 0.0)).as_vec3().to_array();
                    let p4 = self.plane.to_world(DVec3::new(min_x, max_y, 0.0)).as_vec3().to_array();

                    wires.push(WireModel::solid(
                        "opening_rect_preview".into(),
                        vec![p1, p2, p3, p4, p1],
                        WireModel::HOVER,
                        false,
                    ));

                    // DIN 1356 diagonal cross symbol preview
                    wires.push(WireModel::solid(
                        "opening_cross_d1".into(),
                        vec![p1, p3],
                        WireModel::HOVER,
                        false,
                    ));
                    wires.push(WireModel::solid(
                        "opening_cross_d2".into(),
                        vec![p2, p4],
                        WireModel::HOVER,
                        false,
                    ));
                }
            }
            SlabOpeningShapeMode::Polygon => {
                if !self.vertices.is_empty() {
                    let mut pts: Vec<[f32; 3]> = self
                        .vertices
                        .iter()
                        .map(|v| v.as_vec3().to_array())
                        .collect();
                    pts.push(pt.as_vec3().to_array());

                    wires.push(WireModel::solid(
                        "opening_poly_preview".into(),
                        pts,
                        WireModel::HOVER,
                        false,
                    ));
                }
            }
            SlabOpeningShapeMode::Circle => {
                if let Some(center) = self.circle_center {
                    let center_l = self.plane.to_local(center);
                    let pt_l = self.plane.to_local(pt);
                    let radius = ((pt_l.x - center_l.x).powi(2) + (pt_l.y - center_l.y).powi(2)).sqrt();

                    if radius > 1e-4 {
                        let segs = 24;
                        let mut circle_pts = Vec::with_capacity(segs + 1);
                        for i in 0..=segs {
                            let theta = (i as f64) * 2.0 * PI / (segs as f64);
                            let x = center_l.x + radius * theta.cos();
                            let y = center_l.y + radius * theta.sin();
                            circle_pts.push(self.plane.to_world(DVec3::new(x, y, 0.0)).as_vec3().to_array());
                        }
                        wires.push(WireModel::solid(
                            "opening_circle_preview".into(),
                            circle_pts,
                            WireModel::HOVER,
                            false,
                        ));
                    }
                }
            }
        }

        wires
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.host_slab.is_none() {
            return CmdResult::NeedPoint;
        }

        match self.shape {
            SlabOpeningShapeMode::Rectangle => {
                if let Some(c1) = self.rect_corner {
                    let c1_l = self.plane.to_local(c1);
                    let pt_l = self.plane.to_local(pt);
                    if (c1_l.x - pt_l.x).abs() < 1e-4 || (c1_l.y - pt_l.y).abs() < 1e-4 {
                        return CmdResult::NeedPoint;
                    }
                    let min_x = c1_l.x.min(pt_l.x);
                    let max_x = c1_l.x.max(pt_l.x);
                    let min_y = c1_l.y.min(pt_l.y);
                    let max_y = c1_l.y.max(pt_l.y);

                    let boundary = vec![
                        (min_x, min_y),
                        (max_x, min_y),
                        (max_x, max_y),
                        (min_x, max_y),
                    ];
                    self.rect_corner = None;
                    self.finish_boundary(&boundary)
                } else {
                    self.rect_corner = Some(pt);
                    CmdResult::NeedPoint
                }
            }
            SlabOpeningShapeMode::Polygon => {
                if self.vertices.len() >= 3 {
                    let start = self.vertices[0];
                    if pt.distance(start) < 0.25 {
                        return self.on_enter();
                    }
                }
                self.vertices.push(pt);
                CmdResult::NeedPoint
            }
            SlabOpeningShapeMode::Circle => {
                if let Some(center) = self.circle_center {
                    let center_l = self.plane.to_local(center);
                    let pt_l = self.plane.to_local(pt);
                    let radius = ((pt_l.x - center_l.x).powi(2) + (pt_l.y - center_l.y).powi(2)).sqrt();
                    if radius < 1e-4 {
                        return CmdResult::NeedPoint;
                    }

                    let segs = 24;
                    let mut boundary = Vec::with_capacity(segs);
                    for i in 0..segs {
                        let theta = (i as f64) * 2.0 * PI / (segs as f64);
                        let x = center_l.x + radius * theta.cos();
                        let y = center_l.y + radius * theta.sin();
                        boundary.push((x, y));
                    }
                    self.circle_center = None;
                    self.finish_boundary(&boundary)
                } else {
                    self.circle_center = Some(pt);
                    CmdResult::NeedPoint
                }
            }
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match text.trim().to_uppercase().as_str() {
            "R" | "RECT" | "RECTANGLE" => {
                self.shape = SlabOpeningShapeMode::Rectangle;
                self.vertices.clear();
                self.rect_corner = None;
                self.circle_center = None;
                Some(CmdResult::NeedPoint)
            }
            "P" | "POLY" | "POLYGON" => {
                self.shape = SlabOpeningShapeMode::Polygon;
                self.vertices.clear();
                self.rect_corner = None;
                self.circle_center = None;
                Some(CmdResult::NeedPoint)
            }
            "C" | "CIRC" | "CIRCLE" => {
                if self.shape == SlabOpeningShapeMode::Polygon && self.vertices.len() >= 3 {
                    Some(self.on_enter())
                } else {
                    self.shape = SlabOpeningShapeMode::Circle;
                    self.vertices.clear();
                    self.rect_corner = None;
                    self.circle_center = None;
                    Some(CmdResult::NeedPoint)
                }
            }
            "K" | "KIND" => {
                self.kind = match self.kind {
                    SlabOpeningKind::Stairwell => SlabOpeningKind::Shaft,
                    SlabOpeningKind::Shaft => SlabOpeningKind::Duct,
                    SlabOpeningKind::Duct => SlabOpeningKind::Chimney,
                    SlabOpeningKind::Chimney => SlabOpeningKind::Skylight,
                    SlabOpeningKind::Skylight => SlabOpeningKind::Custom,
                    SlabOpeningKind::Custom => SlabOpeningKind::Stairwell,
                };
                Some(CmdResult::NeedPoint)
            }
            "D" | "DEPTH" => {
                self.depth = match self.depth {
                    SlabOpeningDepth::ThroughHole => SlabOpeningDepth::Recess(0.05),
                    SlabOpeningDepth::Recess(_) => SlabOpeningDepth::ThroughHole,
                };
                Some(CmdResult::NeedPoint)
            }
            "U" | "UNDO" => {
                if self.rect_corner.is_some() {
                    self.rect_corner = None;
                    Some(CmdResult::NeedPoint)
                } else if self.circle_center.is_some() {
                    self.circle_center = None;
                    Some(CmdResult::NeedPoint)
                } else if !self.vertices.is_empty() {
                    self.vertices.pop();
                    Some(CmdResult::NeedPoint)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.shape == SlabOpeningShapeMode::Polygon && self.vertices.len() >= 3 {
            let boundary: Vec<(f64, f64)> = self
                .vertices
                .iter()
                .map(|p| {
                    let local = self.plane.to_local(*p);
                    (local.x, local.y)
                })
                .collect();
            self.vertices.clear();
            self.finish_boundary(&boundary)
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        if self.rect_corner.is_some() || self.circle_center.is_some() || !self.vertices.is_empty() {
            self.rect_corner = None;
            self.circle_center = None;
            self.vertices.clear();
            CmdResult::NeedPoint
        } else {
            CmdResult::Cancel
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if self.rect_corner.is_some() {
            self.rect_corner = None;
            Some(CmdResult::NeedPoint)
        } else if self.circle_center.is_some() {
            self.circle_center = None;
            Some(CmdResult::NeedPoint)
        } else if !self.vertices.is_empty() {
            self.vertices.pop();
            Some(CmdResult::NeedPoint)
        } else {
            None
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        true
    }

    fn needs_entity_pick(&self) -> bool {
        self.host_slab.is_none()
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        self.host_slab.is_none()
    }

    fn entity_pick_hover_highlights_handle(&self, scene: &Scene, handle: Handle) -> bool {
        let owner = resolve_slab_package(scene, handle);
        scene.document.get_entity(owner).and_then(slab_from_entity).is_some()
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        // Package resolution is deliberately deferred to the hover hook; this
        // callback has no Scene access under CadCommand.
        self.host_slab = Some(handle);
        CmdResult::NeedPoint
    }

    fn on_entities_committed(&mut self, scene: &mut Scene, handles: &[Handle]) {
        for &h in handles {
            let Some(host) = self.host_slab.map(|picked| resolve_slab_package(scene, picked)) else {
                continue;
            };
            if scene.document.get_entity(host).and_then(slab_from_entity).is_none() {
                continue;
            }
            if let Some(mut opening) = scene.document.get_entity(h).and_then(slab_opening_from_entity) {
                opening.host_slab = host;
                write_slab_opening_record(&mut scene.document, h, &opening);
                add_slab_opening_handle(&mut scene.document, host, h);
            }
            let _ = regenerate_slab_opening_representation(scene, h, self.library.as_ref(), None);
        }
    }

    fn live_properties(&self) -> Option<LiveCommandProperties> {
        Some(LiveCommandProperties {
            title: crate::t!("Slab Opening").into_owned(),
            fields: vec![
                LiveCommandField {
                    label: crate::t!("Kind").into_owned(),
                    field_id: "opening_kind",
                    value: LiveFieldValue::Choice {
                        selected: self.kind.as_str().to_string(),
                        options: vec![
                            "Stairwell".to_string(),
                            "Shaft".to_string(),
                            "Duct".to_string(),
                            "Chimney".to_string(),
                            "Skylight".to_string(),
                            "Custom".to_string(),
                        ],
                    },
                },
                LiveCommandField {
                    label: crate::t!("Depth Mode").into_owned(),
                    field_id: "opening_depth_mode",
                    value: LiveFieldValue::Choice {
                        selected: self.depth.as_str().to_string(),
                        options: vec![
                            "ThroughHole".to_string(),
                            "Recess".to_string(),
                        ],
                    },
                },
                LiveCommandField {
                    label: crate::t!("Recess Depth").into_owned(),
                    field_id: "opening_recess_depth",
                    value: LiveFieldValue::Number(self.depth.depth_value().unwrap_or(0.0)),
                },
                LiveCommandField {
                    label: crate::t!("Shape").into_owned(),
                    field_id: "opening_shape",
                    value: LiveFieldValue::Choice {
                        selected: match self.shape {
                            SlabOpeningShapeMode::Rectangle => "Rectangle".to_string(),
                            SlabOpeningShapeMode::Polygon => "Polygon".to_string(),
                            SlabOpeningShapeMode::Circle => "Circle".to_string(),
                        },
                        options: vec![
                            "Rectangle".to_string(),
                            "Polygon".to_string(),
                            "Circle".to_string(),
                        ],
                    },
                },
            ],
        })
    }

    fn apply_live_property(&mut self, field_id: &str, value: LiveFieldValue) -> CmdResult {
        let changed = match (field_id, value) {
            ("opening_kind", LiveFieldValue::Choice { selected, .. }) => {
                self.kind = SlabOpeningKind::from_str(&selected);
                true
            }
            ("opening_depth_mode", LiveFieldValue::Choice { selected, .. }) => {
                if selected == "Recess" {
                    let d = self.depth.depth_value().unwrap_or(0.05);
                    self.depth = SlabOpeningDepth::Recess(d);
                } else {
                    self.depth = SlabOpeningDepth::ThroughHole;
                }
                true
            }
            ("opening_recess_depth", LiveFieldValue::Number(d)) => {
                if matches!(self.depth, SlabOpeningDepth::Recess(_)) {
                    self.depth = SlabOpeningDepth::Recess(d.max(0.001));
                    true
                } else {
                    false
                }
            }
            ("opening_shape", LiveFieldValue::Choice { selected, .. }) => {
                self.shape = match selected.as_str() {
                    "Polygon" => SlabOpeningShapeMode::Polygon,
                    "Circle" => SlabOpeningShapeMode::Circle,
                    _ => SlabOpeningShapeMode::Rectangle,
                };
                self.vertices.clear();
                self.rect_corner = None;
                self.circle_center = None;
                true
            }
            _ => false,
        };
        let _ = changed;
        CmdResult::NeedPoint
    }
}

impl SlabOpeningCommand {
    fn finish_boundary(&mut self, boundary: &[(f64, f64)]) -> CmdResult {
        let Some(host_h) = self.host_slab else {
            return CmdResult::NeedPoint;
        };

        let mut pl = LwPolyline::new();
        pl.is_closed = true;
        for (x, y) in boundary {
            pl.add_vertex(LwVertex::new(Vector2::new(*x, *y)));
        }
        let mut entity = self.plane.place_entity(EntityType::LwPolyline(pl));
        entity.common_mut().layer = AEC_SLAB_OPENING_LAYER.to_string();

        let opening = SlabOpening {
            host_slab: host_h,
            kind: self.kind,
            depth: self.depth,
            boundary: boundary.to_vec(),
            derived_handles: Vec::new(),
        };

        let record = slab_opening_record(&opening);
        entity.common_mut().extended_data.add_record(record);

        self.vertices.clear();
        self.rect_corner = None;
        self.circle_center = None;

        CmdResult::CommitEntity(entity)
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["AEC_SLABOPENING"] });

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::slab::Slab;
    use crate::modules::aec::engine::slab_xdata::{slab_from_entity, slab_opening_from_entity, write_slab_record};

    #[test]
    fn test_slab_opening_commit_and_cutout() {
        let mut scene = Scene::new();
        let lib = engine::library::seed_default_library();

        // 1. Create a host slab
        let mut pl = LwPolyline::new();
        pl.is_closed = true;
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(10.0, 10.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 10.0)));
        let slab_h = scene.add_entity(EntityType::LwPolyline(pl));

        let slab = Slab::new("style_slab_concrete_20", 0);
        write_slab_record(&mut scene.document, slab_h, &slab);

        // 2. Commit an opening inside the slab
        let mut cmd = SlabOpeningCommand::new_with_library(Some(lib)).with_host_slab(slab_h);
        let opening_boundary = vec![(2.0, 2.0), (5.0, 2.0), (5.0, 6.0), (2.0, 6.0)];
        let op_h = cmd.commit_opening(&mut scene, &opening_boundary).expect("commit opening");

        // 3. Verify opening XDATA and linkage
        let op_entity = scene.document.get_entity(op_h).expect("get opening entity");
        let op = slab_opening_from_entity(op_entity).expect("parse opening");
        assert_eq!(op.host_slab, slab_h);
        assert_eq!(op.kind, SlabOpeningKind::Stairwell);
        assert_eq!(op.boundary.len(), 4);

        let updated_slab_entity = scene.document.get_entity(slab_h).expect("get slab entity");
        let updated_slab = slab_from_entity(updated_slab_entity).expect("parse slab");
        assert!(updated_slab.opening_handles.contains(&op_h));
    }
}
