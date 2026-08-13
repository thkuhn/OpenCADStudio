//! AEC commands — wall/room/storey creation, room schedule, IFC export.
//!
//! `AEC_WALL` is an interactive multi-point drawing command (analogous to
//! `PLINE`); the rest remain non-interactive scaffold commands (matching the
//! former plugin's pragmatic behaviour) that operate directly on `&mut Scene`
//! / the document and report feedback via the command line.

use std::sync::Mutex;

use acadrust::entities::{LwPolyline, LwVertex, Table};
use acadrust::tables::AppId;
use acadrust::types::{Vector2, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, WorkingPlane};
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

use super::engine::{
    self, find_closed_loop, Room, Storey, StyleLibrary, Wall,
};
use super::engine::library::load_or_seed;
use super::engine::material::Material;
use super::engine::style::Style;
use super::engine::wall_style::{effective_layers, Layer, LayerFunction, WallStyle};
use std::collections::HashMap;

/// APPID used for all AEC XDATA records (must stay stable for round-trip).
pub const AEC_APPID: &str = "OPENCAD_AEC";

/// In-memory storey store for the scaffold (persistence via document XDATA
/// is a follow-up; matches the former plugin's pragmatism).
static STOREYS: Mutex<Vec<Storey>> = Mutex::new(Vec::new());

/// Register `OPENCAD_AEC` in the APPID table if missing so XDATA survives
/// DWG/DXF round-trip.
fn ensure_app_id(doc: &mut CadDocument) {
    if !doc.app_ids.contains(AEC_APPID) {
        let mut app = AppId::new(AEC_APPID);
        app.handle = doc.allocate_handle();
        let _ = doc.app_ids.add(app);
    }
}

/// Attach (or replace) an `OPENCAD_AEC` XDATA record on `handle`.
fn write_aec_record(doc: &mut CadDocument, handle: Handle, record: ExtendedDataRecord) -> bool {
    ensure_app_id(doc);
    let app_handle = doc.app_ids.get(AEC_APPID).map(|a| a.handle.value());
    let Some(entity) = doc.get_entity_mut(handle) else {
        return false;
    };
    let xd = &mut entity.common_mut().extended_data;
    let kept: Vec<_> = xd
        .records()
        .iter()
        .filter(|r| r.application_name != AEC_APPID)
        .cloned()
        .collect();
    xd.clear();
    for r in kept {
        xd.add_record(r);
    }
    xd.add_record(record);
    if let Some(ah) = app_handle {
        xd.raw_dwg_eed.retain(|(a, _)| *a != ah);
    }
    true
}

/// Read the `OPENCAD_AEC` record on `entity`, if any.
fn read_aec_record(entity: &EntityType) -> Option<&ExtendedDataRecord> {
    entity.common().extended_data.get_record(AEC_APPID)
}

/// Collect baseline segments of every `WALL`-tagged `LwPolyline`.
fn collect_wall_segments(doc: &CadDocument) -> Vec<((f64, f64), (f64, f64))> {
    let mut segments = Vec::new();
    for entity in doc.entities() {
        let EntityType::LwPolyline(pl) = entity else {
            continue;
        };
        let is_wall = matches!(
            read_aec_record(entity).and_then(|r| r.values.first()),
            Some(XDataValue::String(kind)) if kind == "WALL" || kind == "WALL_V2"
        );
        if !is_wall {
            continue;
        }
        for pair in pl.vertices.windows(2) {
            let a = (pair[0].location.x, pair[0].location.y);
            let b = (pair[1].location.x, pair[1].location.y);
            segments.push((a, b));
        }
        if pl.is_closed {
            if let (Some(first), Some(last)) = (pl.vertices.first(), pl.vertices.last()) {
                segments.push((
                    (last.location.x, last.location.y),
                    (first.location.x, first.location.y),
                ));
            }
        }
    }
    segments
}

/// Build the `WALL` XDATA record for `wall` (shared by the interactive draw
/// command and the properties-panel edit path).
fn wall_record(wall: &Wall) -> ExtendedDataRecord {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("WALL".to_string()));
    record.add_value(XDataValue::Distance(wall.thickness));
    record.add_value(XDataValue::Distance(wall.height));
    record.add_value(XDataValue::String(
        wall.material_ref.clone().unwrap_or_default(),
    ));
    record.add_value(XDataValue::Integer32(wall.storey_id as i32));
    record
}

/// In-memory representation of a `WALL_V2` record.
#[derive(Debug, Clone, PartialEq)]
pub struct WallV2 {
    pub style_id: String,
    pub height: f64,
    pub storey_id: u32,
    pub layers: Vec<(String, f64, String)>,
}

impl WallV2 {
    pub fn total_thickness(&self) -> f64 {
        self.layers.iter().map(|(_, t, _)| *t).sum()
    }
}

/// Build a `WALL_V2` XDATA record's values.
pub fn wall_v2_record(
    style_id: &str,
    height: f64,
    storey_id: u32,
    layers: &[(String, f64, String)],
) -> Vec<XDataValue> {
    let mut values = Vec::new();
    values.push(XDataValue::String("WALL_V2".to_string()));
    values.push(XDataValue::String(style_id.to_string()));
    values.push(XDataValue::Distance(height));
    values.push(XDataValue::Integer32(storey_id as i32));
    values.push(XDataValue::Integer32(layers.len() as i32));
    for (mat, thick, func) in layers {
        values.push(XDataValue::String(mat.clone()));
        values.push(XDataValue::Distance(*thick));
        values.push(XDataValue::String(func.clone()));
    }
    values
}

/// Parse a `WALL_V2` XDATA record back into a [`WallV2`].
pub fn wall_v2_from_entity(entity: &EntityType) -> Option<WallV2> {
    let record = read_aec_record(entity)?;
    let v = &record.values;
    if v.len() < 5 {
        return None;
    }
    let XDataValue::String(kind) = &v[0] else {
        return None;
    };
    if kind != "WALL_V2" {
        return None;
    }

    let style_id = if let XDataValue::String(s) = &v[1] {
        s.clone()
    } else {
        return None;
    };
    let height = if let XDataValue::Distance(d) = v[2] {
        d
    } else {
        return None;
    };
    let storey_id = if let XDataValue::Integer32(i) = v[3] {
        i as u32
    } else {
        return None;
    };
    let layer_count = if let XDataValue::Integer32(i) = v[4] {
        i as usize
    } else {
        return None;
    };

    if v.len() < 5 + layer_count * 3 {
        return None;
    }

    let mut layers = Vec::with_capacity(layer_count);
    for i in 0..layer_count {
        let base = 5 + i * 3;
        let mat = if let XDataValue::String(s) = &v[base] {
            s.clone()
        } else {
            return None;
        };
        let thick = if let XDataValue::Distance(d) = v[base + 1] {
            d
        } else {
            return None;
        };
        let func = if let XDataValue::String(s) = &v[base + 2] {
            s.clone()
        } else {
            return None;
        };
        layers.push((mat, thick, func));
    }

    Some(WallV2 {
        style_id,
        height,
        storey_id,
        layers,
    })
}

/// Unified helper to get total thickness, height, and storey_id for any wall entity
/// (supports both `WALL` and `WALL_V2`).
pub fn wall_thickness_and_height(entity: &EntityType) -> Option<(f64, f64, u32)> {
    let record = read_aec_record(entity)?;
    match record.values.first() {
        Some(XDataValue::String(kind)) if kind == "WALL" => {
            let wall = wall_from_entity(entity)?;
            Some((wall.thickness, wall.height, wall.storey_id))
        }
        Some(XDataValue::String(kind)) if kind == "WALL_V2" => {
            let wall = wall_v2_from_entity(entity)?;
            Some((wall.total_thickness(), wall.height, wall.storey_id))
        }
        _ => None,
    }
}

/// Parameters for a 3D extrusion of a wall layer.
///
/// Contains the 2D footprint (a closed polygon loop) and the height
/// to extrude it by.
#[derive(Debug, Clone, PartialEq)]
pub struct WallLayerExtrusion {
    pub footprint: Vec<(f64, f64)>,
    pub height: f64,
}

/// Extracts a wall's centerline points from its [`LwPolyline`] geometry and
/// computes parallel boundary lines for each layer.
///
/// Returns N+1 boundary lines for N layers.
pub fn wall_layer_contour_polylines(
    wall_entity: &EntityType,
    layers: &[(String, f64, String)],
) -> Vec<Vec<(f64, f64)>> {
    let EntityType::LwPolyline(pl) = wall_entity else {
        return Vec::new();
    };
    let centerline: Vec<(f64, f64)> = pl
        .vertices
        .iter()
        .map(|v| (v.location.x, v.location.y))
        .collect();
    let thicknesses: Vec<f64> = layers.iter().map(|(_, t, _)| *t).collect();
    engine::contour::layer_contours(&centerline, &thicknesses)
}

/// Produces the parameters needed to create an extruded solid for each wall layer.
///
/// This implementation uses the "layer footprint" approach: it builds a closed
/// 2D polygon per layer by combining consecutive boundary offsets and returns
/// it along with the wall height.
///
/// Scoping Decision: This function returns plain data ([`WallLayerExtrusion`]).
/// A future step can wire this to the host's `Solid3D` entity creation calls
/// (e.g., using `sweep_model::extruded`).
pub fn wall_layer_extrusions(
    wall_entity: &EntityType,
    layers: &[(String, f64, String)],
    height: f64,
) -> Vec<WallLayerExtrusion> {
    let boundaries = wall_layer_contour_polylines(wall_entity, layers);
    if boundaries.is_empty() || boundaries.len() < 2 {
        return Vec::new();
    }

    let mut extrusions = Vec::with_capacity(layers.len());
    for i in 0..layers.len() {
        let b1 = &boundaries[i];
        let b2 = &boundaries[i + 1];

        // Create a closed loop: forward along b1, then backward along b2.
        let mut footprint = Vec::with_capacity(b1.len() + b2.len());
        footprint.extend(b1.iter().cloned());
        footprint.extend(b2.iter().rev().cloned());

        extrusions.push(WallLayerExtrusion { footprint, height });
    }
    extrusions
}

/// Parse a `WALL` XDATA record back into a [`Wall`] (inverse of
/// [`wall_record`]). Returns `None` if `entity` isn't `WALL`-tagged or the
/// record doesn't have the expected shape.
pub fn wall_from_entity(entity: &EntityType) -> Option<Wall> {
    let record = read_aec_record(entity)?;
    match record.values.as_slice() {
        [XDataValue::String(kind), XDataValue::Distance(thickness), XDataValue::Distance(height), XDataValue::String(material), XDataValue::Integer32(storey_id)]
            if kind == "WALL" =>
        {
            Some(Wall {
                thickness: *thickness,
                height: *height,
                material_ref: if material.is_empty() {
                    None
                } else {
                    Some(material.clone())
                },
                storey_id: *storey_id as u32,
            })
        }
        [XDataValue::String(kind), ..] if kind == "WALL_V2" => {
            let v2 = wall_v2_from_entity(entity)?;
            Some(Wall {
                thickness: v2.total_thickness(),
                height: v2.height,
                material_ref: v2.layers.first().map(|(m, _, _)| m.clone()),
                storey_id: v2.storey_id,
            })
        }
        _ => None,
    }
}

/// Write `wall` back into `handle`'s `WALL` XDATA record, replacing the
/// previous one (used by the properties-panel edit path). Reuses
/// [`wall_record`] so the field layout stays in one place.
pub fn write_wall_properties(doc: &mut CadDocument, handle: Handle, wall: &Wall) -> bool {
    write_aec_record(doc, handle, wall_record(wall))
}

/// Register the `OPENCAD_AEC` APPID up front so an interactive `AEC_WALL`
/// command can embed XDATA directly on entities it builds (it has no
/// `&mut CadDocument` while collecting points).
pub fn ensure_wall_app_id(doc: &mut CadDocument) {
    ensure_app_id(doc);
}

/// Default wall height (metres) offered by the command-line prompt after
/// the point chain is finished.
const DEFAULT_WALL_HEIGHT: f64 = 2.8;
/// Default wall thickness (metres) offered by the command-line prompt after
/// the height has been entered.
const DEFAULT_WALL_THICKNESS: f64 = 0.2;

/// Drawing phase of an in-progress `AEC_WALL` command.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WallPhase {
    /// Collecting click points, like `PLINE`.
    Drawing,
    /// Point chain finished; waiting for a style selection.
    AskStyle,
    /// Point chain finished; waiting for a height value on the command line.
    AskHeight,
    /// Height entered; waiting for a thickness value on the command line.
    AskThickness,
}

/// `AEC_WALL` — interactive multi-point wall polyline drawing, analogous to
/// `PLINE`. Once the point chain is finished (Enter/Escape), the command
/// prompts for height and thickness on the command line (defaults 2.8 / 0.2)
/// before writing the final `WALL` XDATA record and finalizing the entity.
pub struct WallCommand {
    vertices: Vec<DVec3>,
    live_handle: Option<Handle>,
    plane: WorkingPlane,
    wall: Wall,
    phase: WallPhase,
    library: Option<StyleLibrary>,
    style_id: Option<String>,
    resolved_layers: Option<Vec<(String, f64, String)>>,
}

impl WallCommand {
    pub fn new() -> Self {
        // Always load a usable library: `load_or_seed` transparently creates
        // a small default library (materials + wall styles) on first use so
        // the style-selection prompt has something to offer without
        // requiring the user to define materials/styles first.
        Self::new_with_library(Some(engine::library::load_or_seed()))
    }

    pub fn new_with_library(library: Option<StyleLibrary>) -> Self {
        Self {
            vertices: Vec::new(),
            live_handle: None,
            plane: WorkingPlane::default(),
            wall: Wall::new(DEFAULT_WALL_THICKNESS, DEFAULT_WALL_HEIGHT, 0),
            phase: WallPhase::Drawing,
            library,
            style_id: None,
            resolved_layers: None,
        }
    }

    /// Parse a command-line value, falling back to `default` for an empty
    /// input; rejects non-positive/invalid input by keeping the default.
    fn parse_dimension(text: &str, default: f64) -> f64 {
        let t = text.trim();
        if t.is_empty() {
            return default;
        }
        match t.parse::<f64>() {
            Ok(v) if v > 0.0 => v,
            _ => default,
        }
    }

    /// Begin prompting for the wall's height/thickness once the point chain
    /// is done; returns the result that keeps the command active for the
    /// command-line follow-up.
    fn start_dimension_prompt(&mut self) -> CmdResult {
        if self.live_handle.is_none() {
            return CmdResult::Cancel;
        }
        if let Some(lib) = &self.library {
            if !lib.wall_styles.is_empty() {
                self.phase = WallPhase::AskStyle;
                return CmdResult::NeedPoint;
            }
        }
        self.phase = WallPhase::AskHeight;
        CmdResult::NeedPoint
    }

    fn build_entity(&self) -> Option<EntityType> {
        if self.vertices.len() < 2 {
            return None;
        }
        let mut pl = LwPolyline::new();
        for pt in &self.vertices {
            let local = self.plane.to_local(*pt);
            pl.add_vertex(LwVertex::new(Vector2::new(local.x, local.y)));
        }
        let mut entity = self.plane.place_entity(EntityType::LwPolyline(pl));

        let record = if let (Some(style_id), Some(layers)) = (&self.style_id, &self.resolved_layers) {
            let mut rec = ExtendedDataRecord::new(AEC_APPID);
            rec.values = wall_v2_record(style_id, self.wall.height, self.wall.storey_id, layers);
            rec
        } else {
            wall_record(&self.wall)
        };

        entity
            .common_mut()
            .extended_data
            .add_record(record);
        Some(entity)
    }

    fn sync_live(&self, finish: bool) -> CmdResult {
        match (self.build_entity(), self.live_handle) {
            (Some(entity), Some(handle)) => CmdResult::UpdateLiveEntity {
                handle,
                entity,
                finish,
            },
            (Some(entity), None) => CmdResult::CommitLiveEntity(entity),
            (None, _) => CmdResult::Cancel,
        }
    }

    fn undo_last_vertex(&mut self) -> CmdResult {
        if self.vertices.is_empty() {
            return CmdResult::NeedPoint;
        }
        self.vertices.pop();
        match self.vertices.len() {
            0 => CmdResult::NeedPoint,
            1 => match self.live_handle.take() {
                Some(h) => CmdResult::RemoveLiveEntity(h),
                None => CmdResult::NeedPoint,
            },
            _ => self.sync_live(false),
        }
    }
}

impl CadCommand for WallCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "AEC_WALL"
    }

    fn prompt(&self) -> String {
        match self.phase {
            WallPhase::Drawing if self.vertices.is_empty() => {
                "AEC_WALL  Specify start point:".to_string()
            }
            WallPhase::Drawing => {
                format!("AEC_WALL  Next pt  [{}pts]:", self.vertices.len())
            }
            WallPhase::AskStyle => {
                let styles = self.library.as_ref().map(|l| &l.wall_styles).unwrap();
                let names: Vec<_> = styles.iter().map(|s| s.style.name.as_str()).collect();
                let default = names.first().copied().unwrap_or("");
                format!("AEC_WALL  Select wall style [{}] <{}>:", names.join("/"), default)
            }
            WallPhase::AskHeight => {
                format!("AEC_WALL  Specify wall height <{DEFAULT_WALL_HEIGHT}>:")
            }
            WallPhase::AskThickness => {
                format!("AEC_WALL  Specify wall thickness <{DEFAULT_WALL_THICKNESS}>:")
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.phase {
            WallPhase::Drawing if self.vertices.is_empty() => Vec::new(),
            WallPhase::Drawing => vec![CmdOption::new("Undo", "U"), CmdOption::enter("Done")],
            WallPhase::AskStyle => {
                if let Some(lib) = &self.library {
                    lib.wall_styles
                        .iter()
                        .map(|s| CmdOption::new(&s.style.name, &s.style.name))
                        .collect()
                } else {
                    Vec::new()
                }
            }
            WallPhase::AskHeight | WallPhase::AskThickness => Vec::new(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.phase != WallPhase::Drawing {
            // Height/thickness prompt is active; ignore stray clicks.
            return CmdResult::NeedPoint;
        }
        self.vertices.push(pt);
        if self.vertices.len() >= 2 {
            self.sync_live(false)
        } else {
            CmdResult::NeedPoint
        }
    }

    fn set_live_handle(&mut self, handle: Handle) {
        self.live_handle = Some(handle);
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.phase {
            WallPhase::Drawing => self.start_dimension_prompt(),
            WallPhase::AskStyle | WallPhase::AskHeight | WallPhase::AskThickness => {
                self.on_text_input("").unwrap_or(CmdResult::Cancel)
            }
        }
    }

    fn enter_accepts_default_start(&self) -> bool {
        self.phase == WallPhase::Drawing && self.vertices.is_empty()
    }

    fn on_escape(&mut self) -> CmdResult {
        self.on_enter()
    }

    fn on_space_change(&mut self) -> CmdResult {
        self.on_enter()
    }

    fn wants_text_input(&self) -> bool {
        !self.vertices.is_empty() || self.phase != WallPhase::Drawing
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.phase == WallPhase::Drawing && !self.vertices.is_empty()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match self.phase {
            WallPhase::Drawing => match text.trim().to_uppercase().as_str() {
                "U" | "UNDO" => Some(self.undo_last_vertex()),
                _ => None,
            },
            WallPhase::AskStyle => {
                let lib = self.library.as_ref()?;
                let selected = if text.trim().is_empty() {
                    lib.wall_styles.first()
                } else {
                    lib.wall_styles.iter().find(|s| {
                        s.style.name.eq_ignore_ascii_case(text.trim())
                            || s.style.id.eq_ignore_ascii_case(text.trim())
                    })
                };

                if let Some(style) = selected {
                    self.style_id = Some(style.style.id.clone());

                    // Resolve layers
                    let mut style_map = HashMap::new();
                    for s in &lib.wall_styles {
                        style_map.insert(s.style.id.clone(), s.clone());
                    }

                    if let Ok(layers) = effective_layers(&style_map, &style.style.id) {
                        let mut resolved = Vec::new();
                        for layer in layers {
                            let mat_name = lib
                                .materials
                                .iter()
                                .find(|m| m.id == layer.material_id)
                                .map(|m| m.name.clone())
                                .unwrap_or_else(|| layer.material_id.clone());

                            let func_str = match &layer.function {
                                LayerFunction::Structural => "Structural".to_string(),
                                LayerFunction::Insulation => "Insulation".to_string(),
                                LayerFunction::Finish => "Finish".to_string(),
                                LayerFunction::Other(s) => s.clone(),
                            };
                            resolved.push((mat_name, layer.thickness, func_str));
                        }
                        self.resolved_layers = Some(resolved);
                    }

                    // Style doesn't have height, so go to AskHeight
                    self.phase = WallPhase::AskHeight;
                    Some(CmdResult::NeedPoint)
                } else {
                    // Invalid style name, stay here
                    Some(CmdResult::NeedPoint)
                }
            }
            WallPhase::AskHeight => {
                self.wall.height = Self::parse_dimension(text, DEFAULT_WALL_HEIGHT);
                if self.style_id.is_some() {
                    // We have a style, so we skip AskThickness
                    Some(self.sync_live(true))
                } else {
                    self.phase = WallPhase::AskThickness;
                    Some(CmdResult::NeedPoint)
                }
            }
            WallPhase::AskThickness => {
                self.wall.thickness = Self::parse_dimension(text, DEFAULT_WALL_THICKNESS);
                Some(self.sync_live(true))
            }
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if self.phase == WallPhase::Drawing && !self.vertices.is_empty() {
            Some(self.undo_last_vertex())
        } else {
            None
        }
    }
}

/// Converts a [`LayerFunction`] to the plain string used in XDATA/dispatch.
fn layer_function_to_str(f: &LayerFunction) -> String {
    match f {
        LayerFunction::Structural => "Structural".to_string(),
        LayerFunction::Insulation => "Insulation".to_string(),
        LayerFunction::Finish => "Finish".to_string(),
        LayerFunction::Other(s) => s.clone(),
    }
}

/// Parses a plain string (as entered on the command line) into a
/// [`LayerFunction`], defaulting to `Structural` for empty input and falling
/// back to `Other(..)` for anything unrecognized.
fn parse_layer_function(s: &str) -> LayerFunction {
    match s.trim() {
        "" | "Structural" | "structural" => LayerFunction::Structural,
        "Insulation" | "insulation" => LayerFunction::Insulation,
        "Finish" | "finish" => LayerFunction::Finish,
        other => LayerFunction::Other(other.to_string()),
    }
}

/// Turns a human-entered name into a stable, filesystem/XDATA-safe id
/// (lowercase, non-alphanumeric runs collapsed to `_`).
fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut last_was_sep = true; // suppress a leading separator
    for c in name.trim().chars() {
        if c.is_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            slug.push('_');
            last_was_sep = true;
        }
    }
    while slug.ends_with('_') {
        slug.pop();
    }
    if slug.is_empty() {
        "item".to_string()
    } else {
        slug
    }
}

/// Step of an in-progress `AEC_MATERIAL` command.
enum MaterialStep {
    Name,
    Hatch { name: String },
    Color { name: String, hatch: String },
    LineType { name: String, hatch: String, color: u32 },
}

/// `AEC_MATERIAL` — create (or update) a material in the AEC style library,
/// prompting step by step for name, hatch pattern, line color and line type.
pub struct MaterialCommand {
    step: MaterialStep,
}

impl MaterialCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            step: MaterialStep::Name,
        }
    }
}

impl CadCommand for MaterialCommand {
    fn name(&self) -> &'static str {
        "AEC_MATERIAL"
    }

    fn prompt(&self) -> String {
        match &self.step {
            MaterialStep::Name => "AEC_MATERIAL  Enter material name:".to_string(),
            MaterialStep::Hatch { .. } => {
                "AEC_MATERIAL  Enter hatch pattern <ANSI31>:".to_string()
            }
            MaterialStep::Color { .. } => {
                "AEC_MATERIAL  Enter line color as hex RRGGBB <000000>:".to_string()
            }
            MaterialStep::LineType { .. } => {
                "AEC_MATERIAL  Enter line type <Continuous>:".to_string()
            }
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        match &self.step {
            MaterialStep::Name => {
                if t.is_empty() {
                    // A material needs a name; keep prompting.
                    return Some(CmdResult::NeedPoint);
                }
                self.step = MaterialStep::Hatch {
                    name: t.to_string(),
                };
                Some(CmdResult::NeedPoint)
            }
            MaterialStep::Hatch { name } => {
                let hatch = if t.is_empty() {
                    "ANSI31".to_string()
                } else {
                    t.to_string()
                };
                self.step = MaterialStep::Color {
                    name: name.clone(),
                    hatch,
                };
                Some(CmdResult::NeedPoint)
            }
            MaterialStep::Color { name, hatch } => {
                let color = u32::from_str_radix(t.trim_start_matches('#'), 16).unwrap_or(0);
                self.step = MaterialStep::LineType {
                    name: name.clone(),
                    hatch: hatch.clone(),
                    color,
                };
                Some(CmdResult::NeedPoint)
            }
            MaterialStep::LineType { name, hatch, color } => {
                let line_type = if t.is_empty() {
                    "Continuous".to_string()
                } else {
                    t.to_string()
                };
                Some(CmdResult::Dispatch(format!(
                    "AEC_MATERIAL_ADD {name}|{hatch}|{color:06X}|{line_type}"
                )))
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").unwrap_or(CmdResult::Cancel)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// `AEC_MATERIAL_ADD name|hatch|color_hex|line_type` — the non-interactive
/// handler `MaterialCommand` dispatches to once all fields are collected;
/// upserts the material (by name → stable id) into the style library and
/// persists it.
pub fn aec_material_add(command_line: &mut CommandLine, args: &str) {
    let parts: Vec<&str> = args.split('|').collect();
    let [name, hatch, color_hex, line_type] = parts.as_slice() else {
        command_line.push_error("AEC_MATERIAL_ADD: malformed arguments.");
        return;
    };
    let color = u32::from_str_radix(color_hex, 16).unwrap_or(0);
    let id = format!("mat_{}", slugify(name));

    let mut lib = load_or_seed();
    lib.upsert_material(Material::new(
        id,
        name.to_string(),
        hatch.to_string(),
        color,
        line_type.to_string(),
    ));
    match engine::library::save_to_default_path(&lib) {
        Ok(()) => command_line.push_info(&format!(
            "AEC: Material '{name}' saved (hatch {hatch}, color #{color:06X}, linetype {line_type})."
        )),
        Err(e) => command_line.push_error(&format!("AEC_MATERIAL: failed to save library: {e}")),
    }
}

/// Step of an in-progress `AEC_STYLE` command.
enum StyleStep {
    Name,
    Parent {
        name: String,
    },
    /// Collecting layers; `layers` accumulates `(material_name, thickness, function)`.
    LayerMaterial {
        name: String,
        parent: Option<String>,
        layers: Vec<(String, f64, LayerFunction)>,
    },
    LayerThickness {
        name: String,
        parent: Option<String>,
        layers: Vec<(String, f64, LayerFunction)>,
        material: String,
    },
    LayerFunctionStep {
        name: String,
        parent: Option<String>,
        layers: Vec<(String, f64, LayerFunction)>,
        material: String,
        thickness: f64,
    },
}

/// `AEC_STYLE` — create (or update) a wall style in the AEC style library:
/// name, optional parent style (for single-parent inheritance), then a loop
/// collecting material/thickness/function per layer (blank material name
/// ends the loop).
pub struct StyleCommand {
    step: StyleStep,
}

impl StyleCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            step: StyleStep::Name,
        }
    }

    fn finish(name: &str, parent: &Option<String>, layers: &[(String, f64, LayerFunction)]) -> CmdResult {
        let parent_part = parent.clone().unwrap_or_default();
        let layers_part = layers
            .iter()
            .map(|(mat, thick, func)| format!("{mat}:{thick}:{}", layer_function_to_str(func)))
            .collect::<Vec<_>>()
            .join(";");
        CmdResult::Dispatch(format!("AEC_STYLE_ADD {name}|{parent_part}|{layers_part}"))
    }
}

impl CadCommand for StyleCommand {
    fn name(&self) -> &'static str {
        "AEC_STYLE"
    }

    fn prompt(&self) -> String {
        match &self.step {
            StyleStep::Name => "AEC_STYLE  Enter wall style name:".to_string(),
            StyleStep::Parent { .. } => {
                "AEC_STYLE  Enter parent style name (blank = none):".to_string()
            }
            StyleStep::LayerMaterial { layers, .. } => format!(
                "AEC_STYLE  Add layer {} — material name (blank = finish style):",
                layers.len() + 1
            ),
            StyleStep::LayerThickness { material, .. } => {
                format!("AEC_STYLE  Layer '{material}' — thickness <0.2>:")
            }
            StyleStep::LayerFunctionStep { material, .. } => format!(
                "AEC_STYLE  Layer '{material}' — function [Structural/Insulation/Finish] <Structural>:"
            ),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        match &self.step {
            StyleStep::Name => {
                if t.is_empty() {
                    return Some(CmdResult::NeedPoint);
                }
                self.step = StyleStep::Parent {
                    name: t.to_string(),
                };
                Some(CmdResult::NeedPoint)
            }
            StyleStep::Parent { name } => {
                let parent = if t.is_empty() {
                    None
                } else {
                    Some(t.to_string())
                };
                self.step = StyleStep::LayerMaterial {
                    name: name.clone(),
                    parent,
                    layers: Vec::new(),
                };
                Some(CmdResult::NeedPoint)
            }
            StyleStep::LayerMaterial { name, parent, layers } => {
                if t.is_empty() {
                    // No (more) layers: finish, possibly inheriting layers from
                    // the parent style if none were entered here.
                    return Some(Self::finish(name, parent, layers));
                }
                self.step = StyleStep::LayerThickness {
                    name: name.clone(),
                    parent: parent.clone(),
                    layers: layers.clone(),
                    material: t.to_string(),
                };
                Some(CmdResult::NeedPoint)
            }
            StyleStep::LayerThickness {
                name,
                parent,
                layers,
                material,
            } => {
                let thickness = WallCommand::parse_dimension(t, 0.2);
                self.step = StyleStep::LayerFunctionStep {
                    name: name.clone(),
                    parent: parent.clone(),
                    layers: layers.clone(),
                    material: material.clone(),
                    thickness,
                };
                Some(CmdResult::NeedPoint)
            }
            StyleStep::LayerFunctionStep {
                name,
                parent,
                layers,
                material,
                thickness,
            } => {
                let function = parse_layer_function(t);
                let mut layers = layers.clone();
                layers.push((material.clone(), *thickness, function));
                self.step = StyleStep::LayerMaterial {
                    name: name.clone(),
                    parent: parent.clone(),
                    layers,
                };
                Some(CmdResult::NeedPoint)
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").unwrap_or(CmdResult::Cancel)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// `AEC_STYLE_ADD name|parent|mat1:thick1:func1;mat2:thick2:func2...` — the
/// non-interactive handler `StyleCommand` dispatches to once all fields are
/// collected; upserts the wall style (by name → stable id) into the style
/// library and persists it. An empty layer list inherits layers from the
/// parent style at resolution time (see `effective_layers`).
pub fn aec_style_add(command_line: &mut CommandLine, args: &str) {
    let mut parts = args.splitn(3, '|');
    let (Some(name), Some(parent_raw), Some(layers_raw)) =
        (parts.next(), parts.next(), parts.next())
    else {
        command_line.push_error("AEC_STYLE_ADD: malformed arguments.");
        return;
    };

    let mut lib = load_or_seed();

    let parent_style_id = if parent_raw.is_empty() {
        None
    } else {
        match lib
            .wall_styles
            .iter()
            .find(|s| s.style.name.eq_ignore_ascii_case(parent_raw))
        {
            Some(p) => Some(p.style.id.clone()),
            None => {
                command_line.push_error(&format!(
                    "AEC_STYLE: unknown parent style '{parent_raw}', creating without a parent."
                ));
                None
            }
        }
    };

    let mut layers = Vec::new();
    if !layers_raw.is_empty() {
        for entry in layers_raw.split(';') {
            let fields: Vec<&str> = entry.splitn(3, ':').collect();
            let [mat_name, thick_str, func_str] = fields.as_slice() else {
                continue;
            };
            let material_id = lib
                .materials
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case(mat_name))
                .map(|m| m.id.clone())
                .unwrap_or_else(|| format!("mat_{}", slugify(mat_name)));
            let thickness: f64 = thick_str.parse().unwrap_or(0.2);
            layers.push(Layer {
                material_id,
                thickness,
                function: parse_layer_function(func_str),
            });
        }
    }

    let id = format!("style_{}", slugify(name));
    lib.upsert_wall_style(WallStyle {
        style: Style {
            id,
            name: name.to_string(),
            object_kind: "Wall".to_string(),
            parent_style_id,
        },
        layers,
    });

    match engine::library::save_to_default_path(&lib) {
        Ok(()) => command_line.push_info(&format!(
            "AEC: Wall style '{name}' saved with {} layer(s).",
            lib.wall_styles
                .iter()
                .find(|s| s.style.name == name)
                .map(|s| s.layers.len())
                .unwrap_or(0)
        )),
        Err(e) => command_line.push_error(&format!("AEC_STYLE: failed to save library: {e}")),
    }
}

/// `AEC_ROOM` — detect a closed wall loop (or demo rectangle) + ROOM XDATA.
pub fn aec_room(scene: &mut Scene, command_line: &mut CommandLine) {
    let wall_segments = collect_wall_segments(&scene.document);
    let (pts, detected) = match find_closed_loop(&wall_segments, 1e-3) {
        Some(loop_pts) => (loop_pts, true),
        None => (
            vec![(0.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 3.0)],
            false,
        ),
    };

    let room = Room::from_polygon("Office 101", &pts, 2.8, 0);

    let mut pl = LwPolyline::new();
    pl.is_closed = true;
    for (x, y) in &pts {
        pl.add_vertex(LwVertex::new(Vector2::new(*x, *y)));
    }

    let handle = scene.add_entity(EntityType::LwPolyline(pl));

    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("ROOM".to_string()));
    record.add_value(XDataValue::String(room.name.clone()));
    record.add_value(XDataValue::Real(room.area));
    record.add_value(XDataValue::Real(room.perimeter));
    record.add_value(XDataValue::Real(room.volume));
    record.add_value(XDataValue::Integer32(room.storey_id as i32));

    write_aec_record(&mut scene.document, handle, record);
    scene.bump_geometry();
    if detected {
        command_line.push_info(&format!(
            "AEC: Detected closed wall loop, created room '{}' ({} vertices) at {handle}",
            room.name,
            pts.len()
        ));
    } else {
        command_line.push_info(&format!(
            "AEC: No closed wall loop found, created demo room '{}' at {handle}",
            room.name
        ));
    }
}

/// `AEC_STOREY` — append an in-memory storey and report it.
pub fn aec_storey(_scene: &mut Scene, command_line: &mut CommandLine) {
    let mut storeys = STOREYS.lock().unwrap();
    let next_id = storeys.len() as u32;
    let new_storey = Storey::new(
        format!("Level {}", next_id + 1),
        (next_id as f64) * 3.0,
        3.0,
    );
    storeys.push(new_storey.clone());

    command_line.push_info(&format!(
        "AEC: Added storey '{}' at elevation {}",
        new_storey.name, new_storey.elevation
    ));
}

/// `AEC_ROOMSCHEDULE` — scan ROOM XDATA and build a real TABLE entity.
pub fn aec_room_schedule(scene: &mut Scene, command_line: &mut CommandLine) {
    let mut rooms = Vec::new();
    for entity in scene.document.entities() {
        let Some(record) = read_aec_record(entity) else {
            continue;
        };
        if let Some(XDataValue::String(kind)) = record.values.first() {
            if kind == "ROOM" && record.values.len() >= 6 {
                // 0: "ROOM", 1: name, 2: area, 3: perim, 4: vol, 5: storey_id
                let name = if let XDataValue::String(s) = &record.values[1] {
                    s.clone()
                } else {
                    "Unknown".to_string()
                };
                let area = if let XDataValue::Real(r) = record.values[2] {
                    r
                } else {
                    0.0
                };
                let storey_id = if let XDataValue::Integer32(i) = record.values[5] {
                    i
                } else {
                    0
                };
                rooms.push((name, area, storey_id));
            }
        }
    }

    if rooms.is_empty() {
        command_line.push_info("AEC: No rooms found in document.");
        return;
    }

    // Real TABLE entity: header row + one row per room.
    let row_count = rooms.len() + 1;
    let mut table = Table::new(Vector3::ZERO, row_count, 3);
    table.set_cell_text(0, 0, "Name");
    table.set_cell_text(0, 1, "Area");
    table.set_cell_text(0, 2, "Storey ID");
    for (row, (name, area, storey)) in rooms.iter().enumerate() {
        table.set_cell_text(row + 1, 0, name);
        table.set_cell_text(row + 1, 1, &format!("{area:.2}"));
        table.set_cell_text(row + 1, 2, &storey.to_string());
    }

    let handle = scene.add_entity(EntityType::Table(table));
    scene.bump_geometry();
    command_line.push_info(&format!(
        "AEC: Room schedule table with {} room(s) created at {handle}",
        rooms.len()
    ));
}

/// `AEC_IFCEXPORT` — collect walls/rooms/storeys and emit IFC4 SPF (in-memory).
pub fn aec_ifc_export(scene: &mut Scene, command_line: &mut CommandLine) {
    let mut ifc_scene = engine::Scene::default();

    // Add in-memory storeys
    {
        let storeys = STOREYS.lock().unwrap();
        for (i, s) in storeys.iter().enumerate() {
            ifc_scene.storeys.push((i as u32, s.clone()));
        }
    }

    // Collect walls and rooms from document XDATA
    for entity in scene.document.entities() {
        let Some(record) = read_aec_record(entity) else {
            continue;
        };
        match record.values.first() {
            Some(XDataValue::String(kind)) if kind == "WALL" || kind == "WALL_V2" => {
                if let Some((thickness, height, storey_id)) = wall_thickness_and_height(entity) {
                    let material_ref = if kind == "WALL" {
                        if let Some(XDataValue::String(s)) = record.values.get(3) {
                            if s.is_empty() { None } else { Some(s.clone()) }
                        } else { None }
                    } else {
                        // For V2, just take the first layer's material as representative for IFC export for now
                        if let Some(XDataValue::String(s)) = record.values.get(5) {
                            if s.is_empty() { None } else { Some(s.clone()) }
                        } else { None }
                    };
                    ifc_scene.walls.push(Wall {
                        thickness,
                        height,
                        material_ref,
                        storey_id,
                    });
                }
            }
            Some(XDataValue::String(kind)) if kind == "ROOM" => {
                // 0: "ROOM", 1: name, 2: area, 3: perim, 4: vol, 5: storey_id
                if record.values.len() >= 6 {
                    let name = if let XDataValue::String(s) = &record.values[1] {
                        s.clone()
                    } else {
                        "Unknown".to_string()
                    };
                    let area = if let XDataValue::Real(r) = record.values[2] {
                        r
                    } else {
                        0.0
                    };
                    let perimeter = if let XDataValue::Real(r) = record.values[3] {
                        r
                    } else {
                        0.0
                    };
                    let volume = if let XDataValue::Real(r) = record.values[4] {
                        r
                    } else {
                        0.0
                    };
                    let storey_id = if let XDataValue::Integer32(i) = record.values[5] {
                        i as u32
                    } else {
                        0
                    };
                    ifc_scene.rooms.push(Room {
                        name,
                        area,
                        perimeter,
                        volume,
                        storey_id,
                    });
                }
            }
            _ => {}
        }
    }

    let ifc_data = engine::ifc::write_spf(&ifc_scene);
    command_line.push_info(&format!(
        "AEC: Exported IFC4 SPF ({} bytes)",
        ifc_data.len()
    ));
    command_line.push_info("(Note: Real file-save dialog is a future step)");
}

#[cfg(test)]
mod wall_command_tests {
    use super::*;
    use acadrust::Handle;
    use glam::DVec3;

    fn wall_xdata(entity: &EntityType) -> Option<&ExtendedDataRecord> {
        entity.common().extended_data.get_record(AEC_APPID)
    }

    #[test]
    fn first_point_only_waits_for_the_next_one() {
        let mut cmd = WallCommand::new_with_library(None);
        assert!(matches!(
            cmd.on_point(DVec3::new(0.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));
    }

    #[test]
    fn second_point_commits_a_two_vertex_wall_polyline_with_xdata() {
        let mut cmd = WallCommand::new_with_library(None);
        assert!(matches!(
            cmd.on_point(DVec3::new(0.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));

        match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitLiveEntity(EntityType::LwPolyline(pl)) => {
                assert_eq!(pl.vertices.len(), 2);
            }
            _ => panic!("second point should commit a live wall polyline"),
        }
    }

    #[test]
    fn later_points_update_the_same_live_polyline_as_a_wall_chain() {
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        let committed = cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        let entity = match committed {
            CmdResult::CommitLiveEntity(e) => e,
            _ => panic!("expected CommitLiveEntity"),
        };
        assert!(
            wall_xdata(&entity).is_some(),
            "committed wall segment should carry OPENCAD_AEC/WALL xdata"
        );

        let handle = Handle::new(7);
        cmd.set_live_handle(handle);
        match cmd.on_point(DVec3::new(5.0, 3.0, 0.0)) {
            CmdResult::UpdateLiveEntity {
                handle: updated,
                entity: EntityType::LwPolyline(pl),
                finish,
            } => {
                assert_eq!(updated, handle);
                assert_eq!(pl.vertices.len(), 3);
                assert!(!finish);
            }
            _ => panic!("a third point should extend the same live wall chain"),
        }
    }

    #[test]
    fn undo_drops_the_last_vertex_and_removes_the_live_entity_below_two_points() {
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        cmd.set_live_handle(Handle::new(3));

        match cmd.on_text_input("U") {
            Some(CmdResult::RemoveLiveEntity(h)) => assert_eq!(h, Handle::new(3)),
            _ => panic!("undoing back to a single vertex should remove the live entity"),
        }
    }

    #[test]
    fn enter_after_the_point_chain_starts_the_height_prompt_instead_of_finalizing() {
        let handle = Handle::new(11);

        let mut enter_cmd = WallCommand::new_with_library(None);
        enter_cmd.set_live_handle(handle);
        assert!(matches!(enter_cmd.on_enter(), CmdResult::NeedPoint));
        assert!(enter_cmd.prompt().contains("height"));

        let mut escape_cmd = WallCommand::new_with_library(None);
        escape_cmd.set_live_handle(handle);
        assert!(matches!(escape_cmd.on_escape(), CmdResult::NeedPoint));
        assert!(escape_cmd.prompt().contains("height"));
    }

    #[test]
    fn height_then_thickness_prompt_writes_entered_values_and_finalizes() {
        let handle = Handle::new(11);
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        cmd.set_live_handle(handle);

        // Finish the point chain -> height prompt.
        assert!(matches!(cmd.on_enter(), CmdResult::NeedPoint));

        // Height entered -> thickness prompt.
        match cmd.on_text_input("3.5") {
            Some(CmdResult::NeedPoint) => {}
            _ => panic!("expected height entry to move to the thickness prompt"),
        }
        assert!(cmd.prompt().contains("thickness"));

        // Thickness entered -> final wall XDATA + finalize.
        match cmd.on_text_input("0.3") {
            Some(CmdResult::UpdateLiveEntity {
                handle: updated,
                entity: EntityType::LwPolyline(pl),
                finish,
            }) => {
                assert_eq!(updated, handle);
                assert!(finish);
                let record = pl
                    .common
                    .extended_data
                    .get_record(AEC_APPID)
                    .expect("finalized wall should carry WALL xdata");
                assert!(matches!(record.values[1], XDataValue::Distance(t) if (t - 0.3).abs() < 1e-9));
                assert!(matches!(record.values[2], XDataValue::Distance(h) if (h - 3.5).abs() < 1e-9));
            }
            _ => panic!("expected thickness entry to finalize the live wall"),
        }
    }

    #[test]
    fn empty_height_and_thickness_prompts_fall_back_to_defaults() {
        let handle = Handle::new(4);
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        cmd.set_live_handle(handle);
        cmd.on_enter();

        // Bare Enter on the height prompt (empty text) keeps the default.
        cmd.on_enter();
        match cmd.on_enter() {
            CmdResult::UpdateLiveEntity {
                entity: EntityType::LwPolyline(pl),
                finish,
                ..
            } => {
                assert!(finish);
                let record = pl.common.extended_data.get_record(AEC_APPID).unwrap();
                assert!(
                    matches!(record.values[1], XDataValue::Distance(t) if (t - DEFAULT_WALL_THICKNESS).abs() < 1e-9)
                );
                assert!(
                    matches!(record.values[2], XDataValue::Distance(h) if (h - DEFAULT_WALL_HEIGHT).abs() < 1e-9)
                );
            }
            _ => panic!("expected default height/thickness to finalize the live wall"),
        }
    }

    /// Draw four wall segments through `WallCommand` exactly as the
    /// interactive host would (point chain, then height/thickness prompt),
    /// then commit each finalized entity into a real `Scene`. Regression
    /// check for `AEC_ROOM`'s closed-loop detection against interactively
    /// drawn walls (previously only exercised against demo geometry).
    #[test]
    fn aec_room_detects_a_closed_loop_from_interactively_drawn_walls() {
        use crate::ui::command_line::CommandLine;

        let mut scene = Scene::new();
        let corners = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(4.0, 0.0, 0.0),
            DVec3::new(4.0, 3.0, 0.0),
            DVec3::new(0.0, 3.0, 0.0),
            DVec3::new(0.0, 0.0, 0.0),
        ];

        for pair in corners.windows(2) {
            let mut cmd = WallCommand::new_with_library(None);
            let committed = match cmd.on_point(pair[0]) {
                CmdResult::NeedPoint => cmd.on_point(pair[1]),
                other => other,
            };
            let entity = match committed {
                CmdResult::CommitLiveEntity(e) => e,
                _ => panic!("two points should commit a live wall segment"),
            };
            let handle = scene.add_entity(entity);
            cmd.set_live_handle(handle);

            // Finish the point chain, then accept default height/thickness
            // (Drawing -> AskHeight -> AskThickness -> finalize).
            cmd.on_enter();
            cmd.on_enter();
            let finalized = cmd.on_enter();
            match finalized {
                CmdResult::UpdateLiveEntity {
                    handle: h, entity, ..
                } => {
                    if let Some(slot) = scene.document.get_entity_mut(h) {
                        *slot = entity;
                    }
                }
                _ => panic!("height/thickness prompt should finalize the wall segment"),
            }
        }

        let mut command_line = CommandLine::default();
        aec_room(&mut scene, &mut command_line);

        let room_record = scene
            .document
            .entities()
            .filter_map(read_aec_record)
            .find(|r| matches!(r.values.first(), Some(XDataValue::String(k)) if k == "ROOM"))
            .expect("aec_room should have written a ROOM xdata record");
        let area = match room_record.values.get(2) {
            Some(XDataValue::Real(a)) => *a,
            _ => panic!("ROOM record should carry an area value"),
        };
        assert!(
            (area - 12.0).abs() < 1e-6,
            "expected the detected 4x3 wall loop to yield area 12.0, got {area}"
        );
    }

    /// `wall_from_entity` is the inverse of `wall_record` — the properties
    /// panel reads a `Wall` this way to populate the height/thickness/material
    /// fields for a WALL-tagged entity.
    #[test]
    fn wall_from_entity_reads_back_a_finalized_wall_record() {
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        cmd.set_live_handle(Handle::new(9));
        cmd.on_enter();
        cmd.on_text_input("3.5");
        let entity = match cmd.on_text_input("0.3") {
            Some(CmdResult::UpdateLiveEntity { entity, .. }) => entity,
            _ => panic!("expected thickness entry to finalize the live wall"),
        };

        let wall = wall_from_entity(&entity).expect("finalized entity should read back as a Wall");
        assert!((wall.thickness - 0.3).abs() < 1e-9);
        assert!((wall.height - 3.5).abs() < 1e-9);
        assert!(wall.material_ref.is_none());
    }

    /// A plain (non-WALL-tagged) entity must not be misread as a wall — this
    /// is what keeps the properties-panel Wall section from appearing on
    /// regular polylines.
    #[test]
    fn wall_from_entity_returns_none_for_a_plain_polyline() {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(1.0, 0.0)));
        let entity = EntityType::LwPolyline(pl);
        assert!(wall_from_entity(&entity).is_none());
    }

    /// `write_wall_properties` is the properties-panel writeback path: it
    /// must reuse `wall_record`'s exact layout so `wall_from_entity` and
    /// `collect_wall_segments`/`aec_room` keep working after an edit.
    #[test]
    fn write_wall_properties_updates_the_wall_xdata_in_place() {
        let mut scene = Scene::new();
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitLiveEntity(e) => e,
            _ => panic!("two points should commit a live wall segment"),
        };
        let handle = scene.add_entity(entity);

        let mut wall = wall_from_entity(scene.document.get_entity(handle).unwrap())
            .expect("committed segment should already carry WALL xdata");
        wall.height = 3.2;
        wall.thickness = 0.25;
        wall.material_ref = Some("Concrete".to_string());
        assert!(write_wall_properties(&mut scene.document, handle, &wall));

        let updated = wall_from_entity(scene.document.get_entity(handle).unwrap())
            .expect("entity should still read back as a wall after the edit");
        assert!((updated.height - 3.2).abs() < 1e-9);
        assert!((updated.thickness - 0.25).abs() < 1e-9);
        assert_eq!(updated.material_ref.as_deref(), Some("Concrete"));

        // The AEC_ROOM segment collector still sees this wall after the edit.
        let segments = collect_wall_segments(&scene.document);
        assert_eq!(segments.len(), 1);
    }

    #[test]
    fn wall_v2_round_trip() {
        let layers = vec![
            ("Finish".to_string(), 0.02, "Finish".to_string()),
            ("Brick".to_string(), 0.10, "Structural".to_string()),
            ("Finish".to_string(), 0.02, "Finish".to_string()),
        ];
        let values = wall_v2_record("style1", 3.0, 1, &layers);
        let pl = LwPolyline::new();
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = values;
        entity.common_mut().extended_data.add_record(record);

        let wall = wall_v2_from_entity(&entity).expect("Should parse WALL_V2");
        assert_eq!(wall.style_id, "style1");
        assert_eq!(wall.height, 3.0);
        assert_eq!(wall.storey_id, 1);
        assert_eq!(wall.layers.len(), 3);
        assert_eq!(wall.layers[1].0, "Brick");
        assert_eq!(wall.layers[1].1, 0.10);
        assert_eq!(wall.layers[1].2, "Structural");
        assert_eq!(wall.total_thickness(), 0.14);
    }

    #[test]
    fn wall_v2_from_entity_returns_none_for_legacy_wall() {
        let w = Wall {
            thickness: 0.2,
            height: 2.8,
            material_ref: Some("Concrete".to_string()),
            storey_id: 1,
        };
        let pl = LwPolyline::new();
        let mut entity = EntityType::LwPolyline(pl);
        entity.common_mut().extended_data.add_record(wall_record(&w));

        assert!(wall_v2_from_entity(&entity).is_none());
    }

    #[test]
    fn wall_thickness_and_height_supports_both_versions() {
        let pl_v1 = LwPolyline::new();
        let mut e_v1 = EntityType::LwPolyline(pl_v1);
        let w1 = Wall {
            thickness: 0.2,
            height: 2.8,
            material_ref: None,
            storey_id: 0,
        };
        e_v1.common_mut().extended_data.add_record(wall_record(&w1));

        let res1 = wall_thickness_and_height(&e_v1).expect("Should read V1");
        assert_eq!(res1, (0.2, 2.8, 0));

        let pl_v2 = LwPolyline::new();
        let mut e_v2 = EntityType::LwPolyline(pl_v2);
        let layers = vec![("Mat".to_string(), 0.15, "Func".to_string())];
        let mut rec2 = ExtendedDataRecord::new(AEC_APPID);
        rec2.values = wall_v2_record("style2", 3.2, 2, &layers);
        e_v2.common_mut().extended_data.add_record(rec2);

        let res2 = wall_thickness_and_height(&e_v2).expect("Should read V2");
        assert_eq!(res2, (0.15, 3.2, 2));
    }

    #[test]
    fn aec_room_detects_a_closed_loop_from_mixed_wall_versions() {
        use crate::ui::command_line::CommandLine;

        let mut scene = Scene::new();
        let corners = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(4.0, 0.0, 0.0),
            DVec3::new(4.0, 3.0, 0.0),
            DVec3::new(0.0, 3.0, 0.0),
            DVec3::new(0.0, 0.0, 0.0),
        ];

        for (i, pair) in corners.windows(2).enumerate() {
            let mut pl = LwPolyline::new();
            pl.add_vertex(LwVertex::new(Vector2::new(pair[0].x, pair[0].y)));
            pl.add_vertex(LwVertex::new(Vector2::new(pair[1].x, pair[1].y)));
            let mut entity = EntityType::LwPolyline(pl);

            let record = if i % 2 == 0 {
                // Version 1
                let w = Wall {
                    thickness: 0.2,
                    height: 2.8,
                    material_ref: None,
                    storey_id: 0,
                };
                wall_record(&w)
            } else {
                // Version 2
                let layers = vec![("Brick".to_string(), 0.2, "Structural".to_string())];
                let mut r = ExtendedDataRecord::new(AEC_APPID);
                r.values = wall_v2_record("style1", 2.8, 0, &layers);
                r
            };
            entity.common_mut().extended_data.add_record(record);
            scene.add_entity(entity);
        }

        let mut command_line = CommandLine::default();
        aec_room(&mut scene, &mut command_line);

        let room_record = scene
            .document
            .entities()
            .filter_map(read_aec_record)
            .find(|r| matches!(r.values.first(), Some(XDataValue::String(k)) if k == "ROOM"))
            .expect("aec_room should have written a ROOM xdata record");
        let area = match room_record.values.get(2) {
            Some(XDataValue::Real(a)) => *a,
            _ => panic!("ROOM record should carry an area value"),
        };
        assert!((area - 12.0).abs() < 1e-6);
    }

    #[test]
    fn wall_command_with_library_uses_ask_style_and_finalizes_v2() {
        use crate::modules::aec::engine::material::Material;
        use crate::modules::aec::engine::style::Style;
        use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, WallStyle};

        let material = Material::new(
            "brick_id".to_string(),
            "Brick Material".to_string(),
            "ANSI31".to_string(),
            0xFF0000,
            "Continuous".to_string(),
        );
        let style = WallStyle {
            style: Style {
                id: "style1".to_string(),
                name: "Standard Wall".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: vec![Layer {
                material_id: "brick_id".to_string(),
                thickness: 0.25,
                function: LayerFunction::Structural,
            }],
        };
        let lib = StyleLibrary {
            materials: vec![material],
            wall_styles: vec![style],
        };

        let mut cmd = WallCommand::new_with_library(Some(lib));
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        let handle = Handle::new(100);
        cmd.set_live_handle(handle);

        // Finish point chain -> AskStyle
        assert!(matches!(cmd.on_enter(), CmdResult::NeedPoint));
        assert!(cmd.prompt().contains("Select wall style"));

        // Select style -> AskHeight
        match cmd.on_text_input("Standard Wall") {
            Some(CmdResult::NeedPoint) => {}
            _ => panic!("Expected style selection to move to AskHeight"),
        }
        assert!(cmd.prompt().contains("height"));

        // Enter height -> Finalize V2
        match cmd.on_text_input("3.0") {
            Some(CmdResult::UpdateLiveEntity {
                entity: EntityType::LwPolyline(pl),
                finish,
                ..
            }) => {
                assert!(finish);
                let record = pl.common.extended_data.get_record(AEC_APPID).unwrap();
                assert_eq!(record.values[0], XDataValue::String("WALL_V2".to_string()));
                assert_eq!(record.values[1], XDataValue::String("style1".to_string()));
                assert!(
                    matches!(record.values[2], XDataValue::Distance(h) if (h - 3.0).abs() < 1e-9)
                );
                // Material name "Brick Material" should be used, not "brick_id"
                assert_eq!(
                    record.values[5],
                    XDataValue::String("Brick Material".to_string())
                );
                assert!(
                    matches!(record.values[6], XDataValue::Distance(t) if (t - 0.25).abs() < 1e-9)
                );
                assert_eq!(
                    record.values[7],
                    XDataValue::String("Structural".to_string())
                );
            }
            _ => panic!("Expected height entry to finalize wall with V2 record"),
        }
    }

    #[test]
    fn wall_command_with_no_library_falls_back_to_v1_record() {
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        let handle = Handle::new(101);
        cmd.set_live_handle(handle);

        // Finish point chain -> AskHeight (skipping AskStyle)
        assert!(matches!(cmd.on_enter(), CmdResult::NeedPoint));
        assert!(cmd.prompt().contains("height"));

        // Enter height -> AskThickness
        cmd.on_text_input("2.8");
        assert!(cmd.prompt().contains("thickness"));

        // Enter thickness -> Finalize V1
        match cmd.on_text_input("0.2") {
            Some(CmdResult::UpdateLiveEntity {
                entity: EntityType::LwPolyline(pl),
                finish,
                ..
            }) => {
                assert!(finish);
                let record = pl.common.extended_data.get_record(AEC_APPID).unwrap();
                assert_eq!(record.values[0], XDataValue::String("WALL".to_string()));
            }
            _ => panic!("Expected thickness entry to finalize wall with V1 record"),
        }
    }

    #[test]
    fn material_command_collects_fields_and_dispatches_add_command() {
        let mut cmd = MaterialCommand::new();
        assert!(cmd.prompt().contains("name"));

        assert!(matches!(
            cmd.on_text_input("Sichtbeton"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(cmd.prompt().contains("hatch"));

        assert!(matches!(
            cmd.on_text_input(""), // blank -> default hatch
            Some(CmdResult::NeedPoint)
        ));
        assert!(cmd.prompt().contains("color"));

        assert!(matches!(
            cmd.on_text_input("A0A0A0"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(cmd.prompt().contains("line type"));

        match cmd.on_text_input("") {
            Some(CmdResult::Dispatch(dispatch)) => {
                assert_eq!(
                    dispatch,
                    "AEC_MATERIAL_ADD Sichtbeton|ANSI31|A0A0A0|Continuous"
                );
            }
            _ => panic!("expected the final field to dispatch AEC_MATERIAL_ADD"),
        }
    }

    #[test]
    fn material_command_requires_a_non_empty_name() {
        let mut cmd = MaterialCommand::new();
        assert!(matches!(cmd.on_text_input(""), Some(CmdResult::NeedPoint)));
        // Still on the name step.
        assert!(cmd.prompt().contains("name") && !cmd.prompt().contains("hatch"));
    }

    #[test]
    fn style_command_collects_two_layers_and_dispatches_add_command() {
        let mut cmd = StyleCommand::new();
        assert!(cmd.prompt().contains("name"));

        assert!(matches!(
            cmd.on_text_input("Testwand"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(cmd.prompt().contains("parent"));

        assert!(matches!(
            cmd.on_text_input(""), // no parent
            Some(CmdResult::NeedPoint)
        ));
        assert!(cmd.prompt().contains("material name"));

        // Layer 1
        assert!(matches!(
            cmd.on_text_input("Putz"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            cmd.on_text_input("0.015"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            cmd.on_text_input("Finish"),
            Some(CmdResult::NeedPoint)
        ));

        // Layer 2
        assert!(matches!(
            cmd.on_text_input("Mauerwerk"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            cmd.on_text_input("0.24"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            cmd.on_text_input("Structural"),
            Some(CmdResult::NeedPoint)
        ));

        // Blank material name ends the layer loop and dispatches.
        match cmd.on_text_input("") {
            Some(CmdResult::Dispatch(dispatch)) => {
                assert_eq!(
                    dispatch,
                    "AEC_STYLE_ADD Testwand||Putz:0.015:Finish;Mauerwerk:0.24:Structural"
                );
            }
            _ => panic!("expected the final blank layer entry to dispatch AEC_STYLE_ADD"),
        }
    }

    #[test]
    fn style_command_with_no_layers_dispatches_empty_layer_list_for_inheritance() {
        let mut cmd = StyleCommand::new();
        cmd.on_text_input("Kind Wand");
        cmd.on_text_input("Standard Wall"); // parent

        match cmd.on_text_input("") {
            Some(CmdResult::Dispatch(dispatch)) => {
                assert_eq!(dispatch, "AEC_STYLE_ADD Kind Wand|Standard Wall|");
            }
            _ => panic!("expected an empty layer list to still dispatch AEC_STYLE_ADD"),
        }
    }

    #[test]
    fn layer_function_round_trips_through_its_plain_string_form() {
        for f in [
            LayerFunction::Structural,
            LayerFunction::Insulation,
            LayerFunction::Finish,
        ] {
            let s = layer_function_to_str(&f);
            assert_eq!(parse_layer_function(&s), f);
        }
        assert_eq!(parse_layer_function(""), LayerFunction::Structural);
        assert_eq!(
            parse_layer_function("Custom"),
            LayerFunction::Other("Custom".to_string())
        );
    }

    #[test]
    fn slugify_normalizes_names_into_stable_ids() {
        assert_eq!(slugify("Wand Stahlbeton 20cm"), "wand_stahlbeton_20cm");
        assert_eq!(slugify("  spaced  out  "), "spaced_out");
        assert_eq!(slugify(""), "item");
    }
}
