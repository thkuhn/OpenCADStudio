//! `AEC_SLAB` interactive command for multi-layer floor/ceiling/roof slabs.
//!
//! Supports:
//! - Interactive polygon drawing (point-by-point, close with "C", snap or Enter)
//! - 2-point rectangle placement
//! - Closed polyline conversion ("Select" mode)
//! - Auto-detection from closed wall perimeters via `find_closed_loop`
//! - Live style, justification (OKFF, OKRD, UKD), and storey elevation binding.

use acadrust::entities::{LwPolyline, LwVertex};
use acadrust::types::Vector2;
use acadrust::xdata::ExtendedDataRecord;
use acadrust::{EntityType, Handle};
use glam::DVec3;
use uuid::Uuid;

use crate::command::{CadCommand, CmdOption, CmdResult, LiveCommandField, LiveCommandProperties, LiveFieldValue, WorkingPlane};
use crate::modules::aec::engine::geometry::signed_area;
use crate::modules::aec::engine::loop_detection::find_closed_loop;
use crate::modules::aec::engine::plan_view::PlanPhase;
use crate::modules::aec::engine::project::StoreyRef;
use crate::modules::aec::engine::slab::{Slab, SlabJustification, SlabLayer};
use crate::modules::aec::engine::slab_package::AEC_SLAB_CARRIER_LAYER;
use crate::modules::aec::engine::slab_regen::regenerate_slab_representation;
use crate::modules::aec::engine::slab_xdata::{resolve_slab_style_layers, write_slab_record};
use crate::modules::aec::engine::wall_style::LayerFunction;
use crate::modules::aec::engine::xdata::{collect_wall_segments, AEC_APPID};
use crate::modules::aec::engine::{self, StyleLibrary};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

/// Standard fallback thickness when no style is selected (0.20m = 20cm).
pub const DEFAULT_SLAB_THICKNESS: f64 = 0.20;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_SLAB",
        label: "Slab",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/boundary.svg")),
        event: ModuleEvent::Command("AEC_SLAB".to_string()),
    }
}

/// Drawing sub-modes for `AEC_SLAB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlabDrawMode {
    /// Click polygon vertices one by one.
    Polygon,
    /// Pick 2 opposite corners of a rectangle.
    Rectangle,
    /// Pick an existing closed polyline to convert to a slab.
    SelectPolyline,
    /// Auto-detect closed wall perimeter.
    AutoDetect,
}

/// `AEC_SLAB` interactive command.
pub struct SlabCommand {
    pub(crate) vertices: Vec<DVec3>,
    plane: WorkingPlane,
    pub(crate) mode: SlabDrawMode,
    pub(crate) rect_corner: Option<DVec3>,
    library: Option<StyleLibrary>,
    pub(crate) style_id: Option<String>,
    pub(crate) resolved_layers: Option<Vec<SlabLayer>>,
    pub(crate) justification: SlabJustification,
    pub(crate) storey_id: u32,
    pub(crate) base_plane_id: Option<Uuid>,
    pub(crate) top_plane_id: Option<Uuid>,
    pub(crate) base_plane_name: Option<String>,
    pub(crate) top_plane_name: Option<String>,
    pub(crate) base_offset: f64,
    pub(crate) top_offset: f64,
    pub(crate) phase: PlanPhase,
    pub(crate) last_committed: Option<Handle>,
    ctrl_was_down: bool,
    no_style_warning: bool,
}

impl SlabCommand {
    pub fn new() -> Self {
        Self::new_with_library(Some(engine::library::load_or_seed()))
    }

    pub fn new_with_library(library: Option<StyleLibrary>) -> Self {
        let mut cmd = Self {
            vertices: Vec::new(),
            plane: WorkingPlane::default(),
            mode: SlabDrawMode::Polygon,
            rect_corner: None,
            library,
            style_id: None,
            resolved_layers: None,
            justification: SlabJustification::Top,
            storey_id: 0,
            base_plane_id: None,
            top_plane_id: None,
            base_plane_name: None,
            top_plane_name: None,
            base_offset: 0.0,
            top_offset: 0.0,
            phase: PlanPhase::New,
            last_committed: None,
            ctrl_was_down: false,
            no_style_warning: false,
        };
        cmd.apply_first_available_style();
        cmd
    }

    pub fn with_library(mut self, library: StyleLibrary) -> Self {
        self.library = Some(library);
        self.apply_first_available_style();
        self
    }

    pub fn with_session_defaults(mut self, style_id: Option<&str>, storey_id: Option<u32>) -> Self {
        if let Some(sid) = style_id {
            if let Some(lib) = &self.library {
                if let Some(style) = lib.slab_styles.iter().find(|s| s.style.id == sid) {
                    self.style_id = Some(style.style.id.clone());
                    self.resolved_layers = resolve_slab_style_layers(lib, &style.style.id, None);
                }
            }
        }
        if let Some(storey) = storey_id {
            self.storey_id = storey;
        }
        self
    }

    pub fn with_storey_planes(mut self, storey: &StoreyRef) -> Self {
        // Storey references use UUIDs while the persisted slab carrier keeps
        // the existing numeric compatibility ID.  The elevation is baked into
        // the slab snapshot when the carrier is built.
        self.base_offset = storey.elevation;
        self
    }

    pub fn with_mode(mut self, mode: SlabDrawMode) -> Self {
        self.mode = mode;
        self.vertices.clear();
        self.rect_corner = None;
        self
    }

    fn apply_first_available_style(&mut self) {
        if self.style_id.is_some() {
            return;
        }
        let Some(lib) = &self.library else {
            return;
        };
        if let Some(style) = lib.slab_styles.first() {
            let id = style.style.id.clone();
            self.resolved_layers = resolve_slab_style_layers(lib, &id, None);
            self.style_id = Some(id);
        }
    }

    /// Helper to resolve default layers if none are set.
    fn fallback_layers(&self) -> Vec<SlabLayer> {
        if let Some(layers) = &self.resolved_layers {
            if !layers.is_empty() {
                return layers.clone();
            }
        }
        vec![SlabLayer::new(
            "Concrete",
            DEFAULT_SLAB_THICKNESS,
            LayerFunction::Structural,
        )]
    }

    /// Commits a closed polygon as a new Slab entity into the scene.
    pub fn commit_polygon(
        &mut self,
        scene: &mut Scene,
        points: &[(f64, f64)],
    ) -> Result<Handle, String> {
        if points.len() < 3 {
            return Err("A slab polygon requires at least 3 points.".to_string());
        }
        let poly_area = signed_area(points).abs();
        if poly_area < 1e-4 {
            return Err("Degenerate slab polygon (area is zero or self-intersecting).".to_string());
        }

        // Build carrier LwPolyline
        let mut pl = LwPolyline::new();
        pl.is_closed = true;
        for (x, y) in points {
            pl.add_vertex(LwVertex::new(Vector2::new(*x, *y)));
        }
        let mut entity = self.plane.place_entity(EntityType::LwPolyline(pl));
        entity.common_mut().layer = AEC_SLAB_CARRIER_LAYER.to_string();

        let style_id = self.style_id.clone().unwrap_or_else(|| "style_slab_concrete_20".to_string());
        let layers = self.fallback_layers();

        let mut slab = Slab::new(style_id, self.storey_id);
        slab.layers = layers;
        slab.justification = self.justification;
        slab.phase = self.phase;
        slab.base_plane_id = self.base_plane_id;
        slab.top_plane_id = self.top_plane_id;
        slab.base_plane_name = self.base_plane_name.clone();
        slab.top_plane_name = self.top_plane_name.clone();
        slab.base_offset = self.base_offset;
        slab.top_offset = self.top_offset;

        let handle = scene.add_entity(entity);
        write_slab_record(&mut scene.document, handle, &slab);

        // Regenerate 2D contours, 2D sectional hatches, and 3D solids
        let _ = regenerate_slab_representation(scene, handle, self.library.as_ref(), None);

        self.last_committed = Some(handle);
        self.vertices.clear();
        self.rect_corner = None;
        scene.bump_geometry();

        Ok(handle)
    }

    /// Automatically detects a closed loop from existing wall segments and creates a Slab.
    pub fn auto_detect_from_wall_loop(&mut self, scene: &mut Scene) -> Result<Handle, String> {
        let wall_segments = collect_wall_segments(&scene.document);
        if wall_segments.len() < 3 {
            return Err("At least 3 connected wall segments are required for auto-detection.".to_string());
        }
        let Some(loop_pts) = find_closed_loop(&wall_segments, 1e-3) else {
            return Err("No closed wall loop detected.".to_string());
        };
        self.commit_polygon(scene, &loop_pts)
    }

    /// Converts an existing closed polyline entity into a Slab.
    pub fn convert_selected_polyline(
        &mut self,
        scene: &mut Scene,
        handle: Handle,
    ) -> Result<Handle, String> {
        let Some(entity) = scene.document.get_entity_mut(handle) else {
            return Err(format!("Entity {handle} not found."));
        };
        let EntityType::LwPolyline(pl) = entity else {
            return Err("Selected entity is not a 2D polyline (LwPolyline).".to_string());
        };
        if pl.vertices.len() < 3 {
            return Err("Polyline must have at least 3 vertices.".to_string());
        }
        pl.is_closed = true;
        pl.common.layer = AEC_SLAB_CARRIER_LAYER.to_string();

        let style_id = self.style_id.clone().unwrap_or_else(|| "style_slab_concrete_20".to_string());
        let layers = self.fallback_layers();

        let mut slab = Slab::new(style_id, self.storey_id);
        slab.layers = layers;
        slab.justification = self.justification;
        slab.phase = self.phase;
        slab.base_plane_id = self.base_plane_id;
        slab.top_plane_id = self.top_plane_id;
        slab.base_plane_name = self.base_plane_name.clone();
        slab.top_plane_name = self.top_plane_name.clone();
        slab.base_offset = self.base_offset;
        slab.top_offset = self.top_offset;

        write_slab_record(&mut scene.document, handle, &slab);
        let _ = regenerate_slab_representation(scene, handle, self.library.as_ref(), None);

        self.last_committed = Some(handle);
        scene.bump_geometry();

        Ok(handle)
    }
}

impl Default for SlabCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl CadCommand for SlabCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "AEC_SLAB"
    }

    fn prompt(&self) -> String {
        match self.mode {
            SlabDrawMode::Polygon => {
                if self.vertices.is_empty() {
                    crate::tr!("aec", "slab-prompt-start", justification = self.justification.display_short())
                } else if self.vertices.len() < 3 {
                    crate::tr!("aec", "slab-prompt-next", count = self.vertices.len())
                } else {
                    crate::tr!("aec", "slab-prompt-close", count = self.vertices.len())
                }
            }
            SlabDrawMode::Rectangle => {
                if self.rect_corner.is_none() {
                    crate::tr!("aec", "slab-rect-corner1")
                } else {
                    crate::tr!("aec", "slab-rect-corner2")
                }
            }
            SlabDrawMode::SelectPolyline => {
                crate::tr!("aec", "slab-select-polyline")
            }
            SlabDrawMode::AutoDetect => {
                crate::tr!("aec", "slab-autodetect-prompt")
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.mode {
            SlabDrawMode::Polygon => {
                if self.vertices.is_empty() {
                    vec![
                        CmdOption::new("Rectangle", "R"),
                        CmdOption::new("Select", "S"),
                        CmdOption::new("Auto", "A"),
                        CmdOption::new("Justification", "J"),
                    ]
                } else if self.vertices.len() < 3 {
                    vec![
                        CmdOption::new("Undo", "U"),
                        CmdOption::new("Justification", "J"),
                    ]
                } else {
                    vec![
                        CmdOption::new("Close", "C"),
                        CmdOption::new("Undo", "U"),
                        CmdOption::enter("Done"),
                    ]
                }
            }
            SlabDrawMode::Rectangle => vec![
                CmdOption::new("Polygon", "P"),
                CmdOption::new("Select", "S"),
                CmdOption::new("Auto", "A"),
            ],
            SlabDrawMode::SelectPolyline => vec![
                CmdOption::new("Polygon", "P"),
                CmdOption::new("Rectangle", "R"),
                CmdOption::new("Auto", "A"),
            ],
            SlabDrawMode::AutoDetect => vec![
                CmdOption::new("Polygon", "P"),
                CmdOption::new("Rectangle", "R"),
            ],
        }
    }

    fn set_ctrl(&mut self, ctrl: bool) {
        if ctrl && !self.ctrl_was_down {
            self.justification = self.justification.next();
        }
        self.ctrl_was_down = ctrl;
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        let mut wires = Vec::new();
        match self.mode {
            SlabDrawMode::Polygon => {
                if self.vertices.is_empty() {
                    return wires;
                }
                let mut pts: Vec<[f32; 3]> = self
                    .vertices
                    .iter()
                    .map(|v| v.as_vec3().to_array())
                    .collect();
                pts.push(pt.as_vec3().to_array());

                wires.push(WireModel::solid(
                    "slab_polygon_edges".into(),
                    pts,
                    WireModel::CYAN,
                    false,
                ));

                if self.vertices.len() >= 2 {
                    // Dashed closing reference line back to start
                    wires.push(WireModel::solid(
                        "slab_closing_wire".into(),
                        vec![pt.as_vec3().to_array(), self.vertices[0].as_vec3().to_array()],
                        WireModel::HOVER,
                        false,
                    ));
                }
            }
            SlabDrawMode::Rectangle => {
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
                        "slab_rect_preview".into(),
                        vec![p1, p2, p3, p4, p1],
                        WireModel::CYAN,
                        false,
                    ));
                }
            }
            _ => {}
        }
        wires
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.mode {
            SlabDrawMode::Polygon => {
                if self.vertices.len() >= 3 {
                    // Check if clicked close to start point to close polygon
                    let start = self.vertices[0];
                    if pt.distance(start) < 0.25 {
                        return self.on_enter();
                    }
                }
                if let Some(&last) = self.vertices.last() {
                    if last.distance(pt) < 1e-6 {
                        return CmdResult::NeedPoint;
                    }
                }
                self.vertices.push(pt);
                CmdResult::NeedPoint
            }
            SlabDrawMode::Rectangle => {
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

                    let pts = vec![
                        (min_x, min_y),
                        (max_x, min_y),
                        (max_x, max_y),
                        (min_x, max_y),
                    ];
                    self.vertices = pts
                        .iter()
                        .map(|&(x, y)| self.plane.to_world(DVec3::new(x, y, 0.0)))
                        .collect();
                    self.rect_corner = None;
                    self.finish_polygon()
                } else {
                    self.rect_corner = Some(pt);
                    CmdResult::NeedPoint
                }
            }
            SlabDrawMode::SelectPolyline => CmdResult::NeedPoint,
            SlabDrawMode::AutoDetect => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match text.trim().to_uppercase().as_str() {
            "R" | "RECT" | "RECTANGLE" => {
                self.mode = SlabDrawMode::Rectangle;
                self.vertices.clear();
                self.rect_corner = None;
                Some(CmdResult::NeedPoint)
            }
            "P" | "POLY" | "POLYGON" => {
                self.mode = SlabDrawMode::Polygon;
                self.vertices.clear();
                self.rect_corner = None;
                Some(CmdResult::NeedPoint)
            }
            "S" | "SEL" | "SELECT" => {
                self.mode = SlabDrawMode::SelectPolyline;
                self.vertices.clear();
                self.rect_corner = None;
                Some(CmdResult::NeedPoint)
            }
            "A" | "AUTO" | "WALL" => {
                self.mode = SlabDrawMode::AutoDetect;
                self.vertices.clear();
                self.rect_corner = None;
                Some(CmdResult::Dispatch(format!(
                    "AEC_SLAB_AUTODETECT_DO {}|{}|{}",
                    self.style_id.clone().unwrap_or_default(),
                    self.justification.as_str(),
                    self.storey_id,
                )))
            }
            "C" | "CLOSE" => {
                if self.vertices.len() >= 3 {
                    Some(self.finish_polygon())
                } else {
                    Some(CmdResult::NeedPoint)
                }
            }
            "U" | "UNDO" => {
                if self.mode == SlabDrawMode::Rectangle && self.rect_corner.is_some() {
                    self.rect_corner = None;
                    Some(CmdResult::NeedPoint)
                } else if !self.vertices.is_empty() {
                    self.vertices.pop();
                    Some(CmdResult::NeedPoint)
                } else {
                    None
                }
            }
            "J" | "JUSTIFICATION" | "OKFF" | "OKRD" | "UKD" => {
                self.justification = self.justification.next();
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.vertices.len() >= 3 {
            self.finish_polygon()
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        if self.rect_corner.is_some() {
            self.rect_corner = None;
            CmdResult::NeedPoint
        } else if !self.vertices.is_empty() {
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
        self.mode == SlabDrawMode::SelectPolyline
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        CmdResult::Dispatch(format!(
            "AEC_SLAB_CONVERT_DO {}|{}|{}|{}",
            handle.value(),
            self.style_id.clone().unwrap_or_default(),
            self.justification.as_str(),
            self.storey_id,
        ))
    }

    fn on_entities_committed(&mut self, scene: &mut Scene, handles: &[Handle]) {
        for &h in handles {
            let _ = regenerate_slab_representation(scene, h, self.library.as_ref(), None);
        }
    }

    fn live_properties(&self) -> Option<LiveCommandProperties> {
        let style_name = match &self.style_id {
            Some(id) => self
                .library
                .as_ref()
                .and_then(|lib| lib.slab_styles.iter().find(|ss| &ss.style.id == id))
                .map(|ss| ss.style.name.clone())
                .unwrap_or_else(|| id.clone()),
            None => String::new(),
        };

        Some(LiveCommandProperties {
            title: crate::t!("Slab").into_owned(),
            fields: vec![
                LiveCommandField {
                    label: crate::t!("Style").into_owned(),
                    field_id: "slab_style",
                    value: LiveFieldValue::Picker(style_name),
                },
                LiveCommandField {
                    label: crate::t!("Justification").into_owned(),
                    field_id: "slab_justification",
                    value: LiveFieldValue::Choice {
                        selected: self.justification.as_str().to_string(),
                        options: vec![
                            "Top".to_string(),
                            "StructuralTop".to_string(),
                            "Bottom".to_string(),
                        ],
                    },
                },
                LiveCommandField {
                    label: crate::t!("Storey").into_owned(),
                    field_id: "slab_storey",
                    value: LiveFieldValue::Number(self.storey_id as f64),
                },
                LiveCommandField {
                    label: crate::t!("Base Offset").into_owned(),
                    field_id: "slab_base_offset",
                    value: LiveFieldValue::Number(self.base_offset),
                },
            ],
        })
    }

    fn live_property_id(&self, field_id: &str) -> Option<String> {
        match field_id {
            "slab_style" => self.style_id.clone(),
            _ => None,
        }
    }

    fn apply_live_property(&mut self, field_id: &str, value: LiveFieldValue) -> CmdResult {
        let changed = match (field_id, value) {
            ("slab_style", LiveFieldValue::Picker(id)) => {
                if let Some(lib) = &self.library {
                    if let Some(style) = lib.slab_styles.iter().find(|s| s.style.id == id || s.style.name == id) {
                        self.style_id = Some(style.style.id.clone());
                        self.resolved_layers = resolve_slab_style_layers(lib, &style.style.id, None);
                        return CmdResult::NeedPoint;
                    }
                }
                self.style_id = Some(id);
                true
            }
            ("slab_justification", LiveFieldValue::Choice { selected, .. }) => {
                self.justification = SlabJustification::from_str(&selected);
                true
            }
            ("slab_storey", LiveFieldValue::Number(n)) => {
                self.storey_id = n.max(0.0) as u32;
                true
            }
            ("slab_base_offset", LiveFieldValue::Number(n)) => {
                self.base_offset = n;
                true
            }
            _ => false,
        };
        let _ = changed;
        CmdResult::NeedPoint
    }
}

impl SlabCommand {
    fn finish_polygon(&mut self) -> CmdResult {
        if self.vertices.len() < 3 {
            return CmdResult::NeedPoint;
        }
        let points: Vec<(f64, f64)> = self
            .vertices
            .iter()
            .map(|p| {
                let local = self.plane.to_local(*p);
                (local.x, local.y)
            })
            .collect();

        // Build temporary polyline entity for standard CAD engine commit
        let mut pl = LwPolyline::new();
        pl.is_closed = true;
        for (x, y) in &points {
            pl.add_vertex(LwVertex::new(Vector2::new(*x, *y)));
        }
        let mut entity = self.plane.place_entity(EntityType::LwPolyline(pl));
        entity.common_mut().layer = AEC_SLAB_CARRIER_LAYER.to_string();

        let style_id = self.style_id.clone().unwrap_or_else(|| "style_slab_concrete_20".to_string());
        let layers = self.fallback_layers();

        let mut slab = Slab::new(style_id, self.storey_id);
        slab.layers = layers;
        slab.justification = self.justification;
        slab.phase = self.phase;
        slab.base_plane_id = self.base_plane_id;
        slab.top_plane_id = self.top_plane_id;
        slab.base_plane_name = self.base_plane_name.clone();
        slab.top_plane_name = self.top_plane_name.clone();
        slab.base_offset = self.base_offset;
        slab.top_offset = self.top_offset;

        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = crate::modules::aec::engine::slab_xdata::slab_record_for_slab(&slab);
        entity.common_mut().extended_data.add_record(record);

        self.vertices.clear();
        self.rect_corner = None;

        CmdResult::CommitEntity(entity)
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["AEC_SLAB"] });

/// Converts a selected closed lightweight polyline into a slab carrier.
/// This is deliberately a one-shot handler because `CadCommand::on_entity_pick`
/// cannot borrow the scene mutably.
pub fn aec_slab_convert_do(
    scene: &mut Scene,
    command_line: &mut CommandLine,
    args: &str,
    library: Option<&StyleLibrary>,
) {
    let parts: Vec<_> = args.split('|').collect();
    if parts.len() != 4 {
        command_line.push_error("AEC_SLAB: malformed polyline conversion arguments.");
        return;
    }
    let Ok(raw_handle) = parts[0].parse::<u64>() else {
        command_line.push_error("AEC_SLAB: invalid polyline handle.");
        return;
    };
    let handle = Handle::new(raw_handle);
    let valid = scene.document.get_entity(handle).is_some_and(|entity| {
        matches!(entity, EntityType::LwPolyline(poly) if poly.is_closed && poly.vertices.len() >= 3)
    });
    if !valid {
        command_line.push_error("AEC_SLAB: select a closed lightweight polyline.");
        return;
    }
    if let Some(EntityType::LwPolyline(poly)) = scene.document.get_entity_mut(handle) {
        poly.common.layer = AEC_SLAB_CARRIER_LAYER.to_string();
    }
    let slab = slab_from_dispatch_args(&parts[1..], library);
    write_slab_record(&mut scene.document, handle, &slab);
    let _ = regenerate_slab_representation(scene, handle, library, None);
    scene.bump_geometry();
    command_line.push_info("AEC_SLAB: closed polyline converted to slab.");
}

/// Detects the first valid closed wall loop and creates its slab carrier.
pub fn aec_slab_autodetect_do(
    scene: &mut Scene,
    command_line: &mut CommandLine,
    args: &str,
    library: Option<&StyleLibrary>,
) {
    let parts: Vec<_> = args.split('|').collect();
    if parts.len() != 3 {
        command_line.push_error("AEC_SLAB: malformed auto-detect arguments.");
        return;
    }
    let segments = collect_wall_segments(&scene.document);
    let Some(boundary) = find_closed_loop(&segments, 1e-3) else {
        command_line.push_error("AEC_SLAB: no closed wall loop found.");
        return;
    };
    if boundary.len() < 3 || signed_area(&boundary).abs() < 1e-4 {
        command_line.push_error("AEC_SLAB: detected wall loop is degenerate.");
        return;
    }
    let mut polyline = LwPolyline::new();
    polyline.is_closed = true;
    for (x, y) in boundary {
        polyline.add_vertex(LwVertex::new(Vector2::new(x, y)));
    }
    let mut entity = EntityType::LwPolyline(polyline);
    entity.common_mut().layer = AEC_SLAB_CARRIER_LAYER.to_string();
    let handle = scene.add_entity(entity);
    let slab = slab_from_dispatch_args(&parts, library);
    write_slab_record(&mut scene.document, handle, &slab);
    let _ = regenerate_slab_representation(scene, handle, library, None);
    scene.bump_geometry();
    command_line.push_info("AEC_SLAB: slab created from closed wall loop.");
}

fn slab_from_dispatch_args(parts: &[&str], library: Option<&StyleLibrary>) -> Slab {
    let style_id = parts.first().copied().filter(|id| !id.is_empty()).unwrap_or("style_slab_concrete_20");
    let justification = parts
        .get(1)
        .map(|value| SlabJustification::from_str(value))
        .unwrap_or(SlabJustification::Top);
    let storey_id = parts.get(2).and_then(|value| value.parse().ok()).unwrap_or(0);
    let layers = library
        .and_then(|lib| resolve_slab_style_layers(lib, style_id, None))
        .filter(|layers| !layers.is_empty())
        .unwrap_or_else(|| vec![SlabLayer::new("Concrete", DEFAULT_SLAB_THICKNESS, LayerFunction::Structural)]);
    let mut slab = Slab::new(style_id, storey_id);
    slab.layers = layers;
    slab.justification = justification;
    slab
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::slab_package::resolve_slab_package;
    use crate::modules::aec::engine::slab_xdata::slab_from_entity;

    #[test]
    fn test_slab_command_polygon_commit() {
        let mut scene = Scene::new();
        let lib = engine::library::seed_default_library();
        let mut cmd = SlabCommand::new_with_library(Some(lib));

        let points = vec![(0.0, 0.0), (5.0, 0.0), (5.0, 4.0), (0.0, 4.0)];
        let handle = cmd.commit_polygon(&mut scene, &points).expect("commit slab");

        let entity = scene.document.get_entity(handle).expect("get entity");
        let slab = slab_from_entity(entity).expect("get slab xdata");

        assert_eq!(slab.layers.len(), 1);
        assert!(!slab.derived_handles.is_empty(), "Derived handles should be populated by regen");
        assert_eq!(resolve_slab_package(&scene, slab.derived_handles[0]), handle);
    }

    #[test]
    fn test_slab_command_rectangle_flow() {
        let mut cmd = SlabCommand::new().with_mode(SlabDrawMode::Rectangle);

        let p1 = DVec3::new(1.0, 2.0, 0.0);
        let p2 = DVec3::new(6.0, 7.0, 0.0);

        let res1 = cmd.on_point(p1);
        assert!(matches!(res1, CmdResult::NeedPoint));
        assert!(cmd.rect_corner.is_some());

        let res2 = cmd.on_point(p2);
        assert!(matches!(res2, CmdResult::CommitEntity(_)));
    }

    #[test]
    fn test_slab_command_convert_polyline() {
        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(4.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(4.0, 3.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 3.0)));
        let handle = scene.add_entity(EntityType::LwPolyline(pl));

        let mut cmd = SlabCommand::new();
        let converted_h = cmd.convert_selected_polyline(&mut scene, handle).expect("convert polyline");
        assert_eq!(converted_h, handle);

        let entity = scene.document.get_entity(handle).expect("get entity");
        let slab = slab_from_entity(entity).expect("parse slab xdata");
        assert_eq!(slab.justification, SlabJustification::Top);
    }
}
