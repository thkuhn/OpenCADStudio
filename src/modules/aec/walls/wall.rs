//! `AEC_WALL` interactive wall drawing command.

use acadrust::entities::{LwPolyline, LwVertex};
use acadrust::types::Vector2;
use acadrust::xdata::ExtendedDataRecord;
use acadrust::{EntityType, Handle};
use uuid::Uuid;
use glam::{DVec2, DVec3};

use crate::command::{CadCommand, CmdOption, CmdResult, WorkingPlane};
use crate::modules::aec::commands::{
    auto_join_committed_wall_segment, erase_wall_live_preview_companions, regenerate_wall_representation,
    resolve_wall_style_layers, tessellate_bulge_segment, wall_from_entity, wall_record_for_wall,
    AEC_APPID,
};
use crate::modules::aec::engine::{self, StyleLibrary, Wall, WallJustification, WallLayer};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_WALL",
        label: "Wall",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/aec/wall_create.svg")),
        event: ModuleEvent::Command("AEC_WALL".to_string()),
    }
}

/// Default wall height (metres) offered by the command-line prompt after
/// the point chain is finished.
pub(crate) const DEFAULT_WALL_HEIGHT: f64 = 2.8;
/// Default wall thickness (metres) offered by the command-line prompt after
/// the height has been entered.
pub(crate) const DEFAULT_WALL_THICKNESS: f64 = 0.2;

// The command used to walk through `WallPhase::AskStyle` / `AskHeight` /
// `AskThickness` command-line follow-up prompts once the point chain was
// finished. Style/height/justification are now always live-editable in the
// Properties panel while drawing (see `live_properties`/`apply_live_property`
// below), so the point chain finishes immediately (`start_dimension_prompt`)
// with whatever is currently set — no separate phase state machine needed.

/// `AEC_WALL` — interactive wall drawing. Each completed 2-point segment is
/// written immediately as its own `LwPolyline` + `WALL` XDATA; the command
/// stays active with the last end as the next start. Style/height/justification
/// stay live-editable in the Properties panel for the whole chain.
pub struct WallCommand {
    pub(crate) vertices: Vec<DVec3>,
    live_handle: Option<Handle>,
    live_contour_handle: Option<Handle>,
    plane: WorkingPlane,
    /// Parametric wall metadata written on each segment commit.
    pub(crate) wall: Wall,
    /// Fallback single-layer thickness when no style is selected.
    pub(crate) thickness: f64,
    library: Option<StyleLibrary>,
    pub(crate) style_id: Option<String>,
    pub(crate) resolved_layers: Option<Vec<WallLayer>>,
    pub(crate) justification: WallJustification,
    ctrl_was_down: bool,
    /// Set once the wall height was explicitly edited via the live
    /// Properties-panel field while drawing.
    height_live_set: bool,
    /// Set when the user tried to finish the wall (Enter/Escape) while a
    /// style selection was mandatory but not made yet; `prompt()` shows a
    /// hint until a style is picked.
    no_style_warning: bool,
    /// Per-segment LWPOLYLINE-style bulge (`bulges[i]` is the bulge of the
    /// segment from `vertices[i]` to `vertices[i + 1]`); parallel to
    /// `vertices`, trailing entry unused. `0.0` = straight segment.
    bulges: Vec<f64>,
    /// When set, the next placed point closes an arc segment (tangent-
    /// continuous with the previous segment) instead of a straight line;
    /// toggled on/off with the `A`/`L` command-line keywords, like `PLINE`.
    pub(crate) arc_mode: bool,
    /// Handle of the last persisted segment in this chain (for auto-join).
    last_committed: Option<Handle>,
    /// Start of the last committed axis, used as tangent reference for arc mode.
    last_axis_from: Option<DVec3>,
    /// Exit tangent of the last committed segment (plane-local), same as PLINE.
    last_tangent: Option<DVec2>,
    /// Number of segments written this command (Enter after ≥1 ends the chain).
    pub(crate) segments_committed: usize,
    /// Untagged live-preview contour handles to erase after the axis commit.
    pending_preview_companions: Vec<Handle>,
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
        let mut cmd = Self {
            vertices: Vec::new(),
            live_handle: None,
            live_contour_handle: None,
            plane: WorkingPlane::default(),
            wall: Wall::new(String::new(), DEFAULT_WALL_HEIGHT, 0),
            thickness: DEFAULT_WALL_THICKNESS,
            library,
            style_id: None,
            resolved_layers: None,
            justification: WallJustification::Center,
            ctrl_was_down: false,
            height_live_set: false,
            no_style_warning: false,
            bulges: Vec::new(),
            arc_mode: false,
            last_committed: None,
            last_axis_from: None,
            last_tangent: None,
            segments_committed: 0,
            pending_preview_companions: Vec::new(),
        };
        cmd.apply_first_available_style();
        cmd
    }

    /// Pick the first library wall style so segments get a real representation
    /// even if the user never opens the style picker.
    fn apply_first_available_style(&mut self) {
        if self.style_id.is_some() {
            return;
        }
        let Some(lib) = &self.library else {
            return;
        };
        let Some(style) = lib.wall_styles.first() else {
            return;
        };
        let id = style.style.id.clone();
        if let Some(resolved) = resolve_wall_style_layers(lib, &id, None) {
            self.style_id = Some(id);
            self.resolved_layers = Some(resolved);
        }
    }

    /// Like [`Self::new`], but pre-fills the style/height with the given
    /// session defaults (typically the values used by the last wall
    /// finished this session) instead of the hardcoded fallback defaults.
    pub fn new_with_defaults(last_style_id: Option<&str>, last_height: Option<f64>) -> Self {
        Self::new().with_session_defaults(last_style_id, last_height)
    }

    pub fn with_storey_planes(
        mut self,
        storey: &crate::modules::aec::engine::project::StoreyRef,
    ) -> Self {
        self.wall.bind_storey_planes(storey, 0.0, 0.0);
        self
    }

    /// Apply last-used style/height against the command's current library.
    /// Call this *after* [`Self::with_library`] so the id is resolved in the
    /// project library, not the seed library used by [`Self::new`].
    pub fn with_session_defaults(
        mut self,
        last_style_id: Option<&str>,
        last_height: Option<f64>,
    ) -> Self {
        if let Some(h) = last_height {
            self.wall.height = h;
            self.height_live_set = true;
        }
        if let Some(id) = last_style_id {
            if let Some(lib) = &self.library {
                if let Some(style) = lib.wall_styles.iter().find(|s| s.style.id == id) {
                    if let Some(resolved) =
                        resolve_wall_style_layers(lib, &style.style.id, None)
                    {
                        self.style_id = Some(style.style.id.clone());
                        self.resolved_layers = Some(resolved);
                    }
                }
            }
        }
        self
    }

    /// Replace the style library (e.g. project-resolved) and re-apply the
    /// current style id, or the first available style when the previous id
    /// is missing from the new library.
    pub fn with_library(mut self, library: StyleLibrary) -> Self {
        let previous = self.style_id.clone();
        self.library = Some(library);
        self.style_id = None;
        self.resolved_layers = None;
        if let Some(id) = previous {
            if let Some(lib) = &self.library {
                if let Some(style) = lib.wall_styles.iter().find(|s| s.style.id == id) {
                    if let Some(resolved) =
                        resolve_wall_style_layers(lib, &style.style.id, None)
                    {
                        self.style_id = Some(style.style.id.clone());
                        self.resolved_layers = Some(resolved);
                    }
                }
            }
        }
        if self.style_id.is_none() {
            self.apply_first_available_style();
        }
        self
    }

    /// Parse a command-line value, falling back to `default` for an empty
    /// input; rejects non-positive/invalid input by keeping the default.
    pub(crate) fn parse_dimension(text: &str, default: f64) -> f64 {
        let t = text.trim();
        if t.is_empty() {
            return default;
        }
        match t.parse::<f64>() {
            Ok(v) if v > 0.0 => v,
            _ => default,
        }
    }

    /// True once a style library with at least one wall style is loaded, in
    /// which case a wall style selection is mandatory before finishing (there
    /// is something to choose, so silently falling back to a styleless V1
    /// wall would be surprising). No library / an empty library means there
    /// is nothing to pick, so the plain height/thickness V1 wall stays valid.
    #[cfg(test)]
    pub(crate) fn requires_style_selection(&self) -> bool {
        self.library
            .as_ref()
            .is_some_and(|lib| !lib.wall_styles.is_empty())
    }

    /// Begin prompting for the wall's height/thickness once the point chain
    /// is done; returns the result that keeps the command active for the
    /// command-line follow-up.
    fn start_dimension_prompt(&mut self) -> CmdResult {
        // Enter/Escape are global "finalize" keys in this app and fire even
        // while the user is typing in the live Properties-panel height field
        // (see `sync_live_if_previewable`) before a second point has been
        // placed. There's nothing to finalize yet in that case — keep the
        // command running instead of cancelling the whole wall.
        if self.vertices.len() < 2 {
            // A lone start point produces nothing. After at least one
            // committed segment, Enter/Escape ends the chain.
            if self.segments_committed > 0 {
                return CmdResult::Cancel;
            }
            return CmdResult::NeedPoint;
        }
        self.commit_current_segment()
    }

    fn commit_current_segment(&mut self) -> CmdResult {
        if self.vertices.len() < 2 {
            return CmdResult::NeedPoint;
        }
        let start = self.vertices[0];
        let end = *self.vertices.last().unwrap();
        if start.distance(end) < 1e-9 {
            self.vertices.pop();
            self.bulges.pop();
            return CmdResult::NeedPoint;
        }
        let Some(entity) = self.build_entity() else {
            return CmdResult::NeedPoint;
        };
        self.last_axis_from = Some(start);
        let start_l = self.plane.to_local(start);
        let end_l = self.plane.to_local(end);
        let bulge = self.bulges.first().copied().unwrap_or(0.0);
        self.last_tangent = crate::modules::draw::draw::polyline::seg_exit_tangent(
            start_l,
            end_l,
            bulge,
        )
        .map(|t| t.as_dvec2());
        self.vertices = vec![end];
        self.bulges = vec![0.0];
        self.segments_committed += 1;
        self.pending_preview_companions = [self.live_handle, self.live_contour_handle]
            .into_iter()
            .flatten()
            .collect();
        self.live_handle = None;
        self.live_contour_handle = None;
        CmdResult::CommitEntity(entity)
    }

    fn build_entity(&self) -> Option<EntityType> {
        if self.vertices.len() < 2 {
            return None;
        }

        let total_thickness = if let Some(layers) = &self.resolved_layers {
            layers.iter().map(|l| l.thickness).sum()
        } else {
            self.thickness
        };

        let offset = self.justification.offset(total_thickness);

        let points: Vec<(f64, f64)> = self.vertices
            .iter()
            .map(|pt| {
                let local = self.plane.to_local(*pt);
                (local.x, local.y)
            })
            .collect();

        let final_points = if offset.abs() > 1e-9 {
            let directions = engine::get_offset_directions(&points);
            points
                .iter()
                .zip(directions.iter())
                .map(|(&(x, y), &(dx, dy))| (x + dx * offset, y + dy * offset))
                .collect()
        } else {
            points
        };

        let mut pl = LwPolyline::new();
        for (i, (x, y)) in final_points.into_iter().enumerate() {
            let mut v = LwVertex::new(Vector2::new(x, y));
            // Bulge is preserved as-is on the offset axis: an exact offset
            // curve of an arc has a different (but nearby) radius, and this
            // approximation keeps the shape visually correct for the common
            // Center-justified case (offset 0.0, no change needed) while
            // staying serviceable for the off-center case.
            v.bulge = self.bulges.get(i).copied().unwrap_or(0.0);
            pl.add_vertex(v);
        }
        let mut entity = self.plane.place_entity(EntityType::LwPolyline(pl));

        let style_id = self.style_id.clone().unwrap_or_default();
        let layers: Vec<WallLayer> = if let Some(layers) = &self.resolved_layers {
            layers.clone()
        } else {
            vec![WallLayer {
                material: String::new(),
                thickness: self.thickness,
                function: "Structural".to_string(),
                axis_offset: -self.thickness * 0.5,
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
            }]
        };
        let mut wall = self.wall.clone();
        wall.style_id = style_id;
        wall.layers = layers;
        wall.justification = self.justification;
        if wall.base_plane_id.is_some() {
            if let Some(h) = wall.height_from_snapshot() {
                wall.height = h;
            }
        }
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record_for_wall(&wall);

        entity.common_mut().extended_data.add_record(record);
        Some(entity)
    }

    fn build_contour_entity(&self) -> Option<EntityType> {
        if self.vertices.len() < 2 {
            return None;
        }
        // Follow the same thickness fallback as `build_entity`: once a style
        // is picked, use its resolved layers; before that (or if there are no
        // layers), fall back to a single default-thickness layer so the outline
        // preview always follows the cursor.
        let layer_data: Vec<(f64, f64)> = match self.resolved_layers.as_ref() {
            Some(layers) if !layers.is_empty() => layers
                .iter()
                .map(|l| (l.thickness, l.axis_offset))
                .collect(),
            _ => vec![(self.thickness, -self.thickness * 0.5)],
        };
        let total_thickness = if layer_data.is_empty() {
            0.0
        } else {
            let min_s = layer_data
                .iter()
                .map(|(_, off)| *off)
                .fold(f64::INFINITY, f64::min);
            let max_e = layer_data
                .iter()
                .map(|(th, off)| *off + *th)
                .fold(f64::NEG_INFINITY, f64::max);
            (max_e - min_s).max(0.0)
        };
        let centerline_offset = self.justification.offset(total_thickness);

        let points: Vec<(f64, f64)> = self.vertices
            .iter()
            .map(|pt| {
                let local = self.plane.to_local(*pt);
                (local.x, local.y)
            })
            .collect();

        // Shared WallRepresentation path — outer_contour_2d is produced by the
        // existing contour helpers, so the live draw outline is unchanged.
        // Bulge-aware so curved axes segments keep their arc shape in the
        // preview/final outline (`wall_layer_footprints_with_bulges` consumes
        // the same axis bulges on regeneration).
        let repr = engine::representation::build_wall_representation_with_bulges(
            &points,
            &self.bulges,
            &layer_data,
            centerline_offset,
        );

        let mut pl = LwPolyline::new();
        pl.is_closed = true;
        for (i, (x, y)) in repr.outer_contour_2d.into_iter().enumerate() {
            let mut v = LwVertex::new(Vector2::new(x, y));
            v.bulge = repr.outer_contour_bulges.get(i).copied().unwrap_or(0.0);
            pl.add_vertex(v);
        }

        let entity = self.plane.place_entity(EntityType::LwPolyline(pl));
        // Contour is a visual helper; no XDATA needed (axis carries the truth).
        Some(entity)
    }

    fn sync_live(&self, finish: bool) -> CmdResult {
        let axis = self.build_entity();
        let contour = self.build_contour_entity();

        match (axis, self.live_handle) {
            (Some(a), Some(h_axis)) => {
                if let (Some(c), None) = (&contour, self.live_contour_handle) {
                    // We just gained a contour (e.g. style assigned mid-draw);
                    // replace the single axis with both axis + contour.
                    CmdResult::ReplaceEntity(h_axis, vec![a, c.clone()])
                } else if let (None, Some(h_contour)) = (&contour, self.live_contour_handle) {
                    // Style removed mid-draw? Rare, but handle it by replacing
                    // both with just the axis.
                    CmdResult::ReplaceManyContinue(vec![(h_axis, vec![a]), (h_contour, vec![])])
                } else {
                    let mut updates = vec![(h_axis, a)];
                    if let (Some(c), Some(h_contour)) = (contour, self.live_contour_handle) {
                        updates.push((h_contour, c));
                    }
                    CmdResult::UpdateLiveEntities { updates, finish }
                }
            }
            (Some(a), None) => {
                let mut entities = vec![a];
                if let Some(c) = contour {
                    entities.push(c);
                }
                CmdResult::CommitLiveEntities(entities)
            }
            (None, _) => CmdResult::Cancel,
        }
    }

    /// Like [`Self::sync_live`], but used for edits coming from the live
    /// Properties-panel fields (style/height), which can legitimately fire
    /// before there is anything to preview yet (e.g. only the start point has
    /// been placed). In that case there is no live entity to update/cancel —
    /// just keep the command running and wait for the next point.
    fn sync_live_if_previewable(&self, finish: bool) -> CmdResult {
        if self.vertices.len() < 2 {
            return CmdResult::NeedPoint;
        }
        self.sync_live(finish)
    }

    fn undo_last_vertex(&mut self) -> CmdResult {
        if self.vertices.is_empty() {
            return CmdResult::NeedPoint;
        }
        self.vertices.pop();
        self.bulges.pop();
        CmdResult::NeedPoint
    }

    /// Tangent-continuation bulge (matches `PLINE`'s arc-continue default):
    /// the arc from `prev` to `next` is tangent at `prev` to the direction of
    /// the previous segment (`prev_prev` -> `prev`). Without a previous
    /// segment to derive a tangent from (first placed segment while in arc
    /// mode), there is nothing to be tangent to, so this degrades gracefully
    /// to a straight line (`0.0`) rather than guessing a radius.
    #[allow(dead_code)]
    fn compute_tangent_bulge(
        prev_prev: Option<(f64, f64)>,
        prev: (f64, f64),
        next: (f64, f64),
    ) -> f64 {
        let Some(prev_prev) = prev_prev else {
            return 0.0;
        };
        Self::tangent_bulge_from_dir(prev_prev, prev, next)
    }

    #[allow(dead_code)]
    fn tangent_bulge_from_dir(
        prev_prev: (f64, f64),
        prev: (f64, f64),
        next: (f64, f64),
    ) -> f64 {
        let dir = (prev.0 - prev_prev.0, prev.1 - prev_prev.1);
        let dir_len = (dir.0 * dir.0 + dir.1 * dir.1).sqrt();
        let chord = (next.0 - prev.0, next.1 - prev.1);
        let chord_len = (chord.0 * chord.0 + chord.1 * chord.1).sqrt();
        if dir_len < 1e-12 || chord_len < 1e-12 {
            return 0.0;
        }
        // Signed angle from tangent direction to chord (cross/dot atan2).
        let cross = dir.0 * chord.1 - dir.1 * chord.0;
        let dot = dir.0 * chord.0 + dir.1 * chord.1;
        let alpha = cross.atan2(dot);
        // Tangent-chord angle equals half the arc's central angle.
        let theta = 2.0 * alpha;
        (theta / 4.0).tan()
    }

    /// Bulge of chord `start`→`end` for the circular arc that also passes
    /// through `through`. Collinear points yield `0.0`.
    #[allow(dead_code)]
    fn bulge_through_three(start: (f64, f64), through: (f64, f64), end: (f64, f64)) -> f64 {
        let (ax, ay) = start;
        let (bx, by) = through;
        let (cx, cy) = end;
        let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
        if d.abs() < 1e-18 {
            return 0.0;
        }
        let a2 = ax * ax + ay * ay;
        let b2 = bx * bx + by * by;
        let c2 = cx * cx + cy * cy;
        let ux = (a2 * (by - cy) + b2 * (cy - ay) + c2 * (ay - by)) / d;
        let uy = (a2 * (cx - bx) + b2 * (ax - cx) + c2 * (bx - ax)) / d;
        let ra = (ax - ux, ay - uy);
        let rb = (bx - ux, by - uy);
        let rc = (cx - ux, cy - uy);
        let cross_ac = ra.0 * rc.1 - ra.1 * rc.0;
        let dot_ac = ra.0 * rc.0 + ra.1 * rc.1;
        let mut theta = cross_ac.atan2(dot_ac);
        let cross_ab = ra.0 * rb.1 - ra.1 * rb.0;
        let dot_ab = ra.0 * rb.0 + ra.1 * rb.1;
        let theta_ab = cross_ab.atan2(dot_ab);
        let on_arc = if theta >= 0.0 {
            theta_ab >= -1e-9 && theta_ab <= theta + 1e-9
        } else {
            theta_ab <= 1e-9 && theta_ab >= theta - 1e-9
        };
        if !on_arc {
            if theta >= 0.0 {
                theta -= 2.0 * std::f64::consts::PI;
            } else {
                theta += 2.0 * std::f64::consts::PI;
            }
        }
        (theta / 4.0).tan()
    }

    fn pending_arc_bulge(&self, start: DVec3, end: DVec3) -> f64 {
        let start_l = self.plane.to_local(start);
        let end_l = self.plane.to_local(end);
        let a = DVec2::new(start_l.x, start_l.y);
        let b = DVec2::new(end_l.x, end_l.y);
        // Default tangent is *not* axis-aligned so a typical first horizontal
        // segment still gets a visible arc (PLINE's +X default would be 0).
        let tangent = self
            .last_tangent
            .unwrap_or_else(|| DVec2::new(1.0, 1.0).normalize_or_zero());
        crate::modules::draw::draw::polyline::compute_bulge(a, tangent, b)
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
        let mode = if self.arc_mode {
            crate::tr!("aec", "wall-mode-arc")
        } else {
            String::new()
        };
        if self.vertices.is_empty() {
            crate::tr!(
                "aec",
                "wall-prompt-start",
                justification = self.justification.as_str()
            )
        } else if self.no_style_warning {
            crate::tr!("aec", "wall-prompt-need-style")
        } else {
            crate::tr!(
                "aec",
                "wall-prompt-next",
                mode = mode.as_str(),
                justification = self.justification.as_str(),
                count = self.vertices.len()
            )
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.vertices.is_empty() {
            return Vec::new();
        }
        let arc_toggle = if self.arc_mode {
            CmdOption::new("Line", "L")
        } else {
            CmdOption::new("Arc", "A")
        };
        vec![
            CmdOption::new("Undo", "U"),
            arc_toggle,
            CmdOption::enter("Done"),
        ]
    }

    fn set_ctrl(&mut self, ctrl: bool) {
        if ctrl && !self.ctrl_was_down {
            self.justification = self.justification.next();
            // In a real CLI we would use CmdResult::Log, but set_ctrl doesn't
            // return it. Toggling the prompt is the next best thing for
            // live feedback.
        }
        self.ctrl_was_down = ctrl;
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        if self.vertices.is_empty() {
            return vec![];
        }

        // Axis rubber band: pending segment from the last placed point to
        // the cursor (the committed vertices already render as the live
        // axis polyline, same convention as `PlineCommand::on_mouse_move`).
        // In arc mode, tessellate the tangent-continuation arc instead of a
        // straight line so the curve is visible before the point is placed.
        let last_world = *self.vertices.last().unwrap();
        let axis_wire = if self.arc_mode {
            let last_local = self.plane.to_local(last_world);
            let cursor_local = self.plane.to_local(pt);
            let bulge = self.pending_arc_bulge(last_world, pt);
            let arc_pts = tessellate_bulge_segment(
                (last_local.x, last_local.y),
                (cursor_local.x, cursor_local.y),
                bulge,
                24,
            );
            let world_pts: Vec<[f32; 3]> = arc_pts
                .iter()
                .map(|&(x, y)| self.plane.to_world(DVec3::new(x, y, 0.0)).as_vec3().to_array())
                .collect();
            WireModel::solid("rubber_band_axis".into(), world_pts, WireModel::CYAN, false)
        } else {
            WireModel::solid(
                "rubber_band_axis".into(),
                vec![
                    last_world.as_vec3().to_array(),
                    pt.as_vec3().to_array(),
                ],
                WireModel::CYAN,
                false,
            )
        };
        let mut wires = vec![axis_wire];

        // Outline rubber band: the wall's outer contour via the shared
        // WallRepresentation builder, computed on the committed vertices plus
        // the not-yet-placed cursor point. Same thickness/justification fallback
        // as `build_contour_entity` so the outline tracks the cursor from the
        // first point onward, including before a style is chosen.
        let mut temp_vertices = self.vertices.clone();
        temp_vertices.push(pt);
        let mut temp_bulges = self.bulges.clone();
        temp_bulges.resize(self.vertices.len(), 0.0);
        if self.arc_mode && self.vertices.len() >= 1 {
            let bulge = self.pending_arc_bulge(*self.vertices.last().unwrap(), pt);
            if let Some(last) = temp_bulges.last_mut() {
                *last = bulge;
            }
        }
        temp_bulges.push(0.0);
        if temp_vertices.len() >= 2 {
            let layer_data: Vec<(f64, f64)> = match self.resolved_layers.as_ref() {
                Some(layers) if !layers.is_empty() => layers
                    .iter()
                    .map(|l| (l.thickness, l.axis_offset))
                    .collect(),
                _ => vec![(self.thickness, -self.thickness * 0.5)],
            };
            let total_thickness = if layer_data.is_empty() {
            0.0
        } else {
            let min_s = layer_data
                .iter()
                .map(|(_, off)| *off)
                .fold(f64::INFINITY, f64::min);
            let max_e = layer_data
                .iter()
                .map(|(th, off)| *off + *th)
                .fold(f64::NEG_INFINITY, f64::max);
            (max_e - min_s).max(0.0)
        };
            let centerline_offset = self.justification.offset(total_thickness);
            let points: Vec<(f64, f64)> = temp_vertices
                .iter()
                .map(|p| {
                    let local = self.plane.to_local(*p);
                    (local.x, local.y)
                })
                .collect();
            // Full outer contour (not drag_ghost) so justification stays correct
            // and the rubber-band matches the committed contour entity.
            let repr = engine::representation::build_wall_representation_with_bulges(
                &points,
                &temp_bulges,
                &layer_data,
                centerline_offset,
            );
            if !repr.outer_contour_2d.is_empty() {
                let mut world_pts: Vec<[f32; 3]> = repr
                    .outer_contour_2d
                    .iter()
                    .map(|&(x, y)| self.plane.to_world(DVec3::new(x, y, 0.0)).as_vec3().to_array())
                    .collect();
                if let Some(first) = world_pts.first().copied() {
                    world_pts.push(first);
                }
                wires.push(WireModel::solid(
                    "rubber_band_contour".into(),
                    world_pts,
                    WireModel::CYAN,
                    false,
                ));
            }
        }

        wires
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let Some(&last) = self.vertices.last() {
            if last.distance(pt) < 1e-9 {
                return CmdResult::NeedPoint;
            }
            let bulge = if self.arc_mode {
                self.pending_arc_bulge(last, pt)
            } else {
                0.0
            };
            if let Some(slot) = self.bulges.last_mut() {
                *slot = bulge;
            }
        }
        self.vertices.push(pt);
        self.bulges.push(0.0);
        if self.vertices.len() >= 2 {
            self.commit_current_segment()
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_entities_committed(&mut self, scene: &mut Scene, handles: &[Handle]) {
        let Some(&wall_handle) = handles.first() else {
            return;
        };
        let is_wall = scene
            .document
            .get_entity(wall_handle)
            .is_some_and(|e| wall_from_entity(e).is_some());
        if !is_wall {
            return;
        }
        if !self.pending_preview_companions.is_empty() {
            erase_wall_live_preview_companions(
                scene,
                wall_handle,
                &self.pending_preview_companions,
            );
            self.pending_preview_companions.clear();
        }
        auto_join_committed_wall_segment(
            scene,
            wall_handle,
            self.last_committed,
            self.library.as_ref(),
        );
        let _ = regenerate_wall_representation(scene, wall_handle, self.library.as_ref());
        self.last_committed = Some(wall_handle);
    }

    fn set_live_handles(&mut self, handles: Vec<Handle>) {
        self.live_handle = handles.first().copied();
        self.live_contour_handle = handles.get(1).copied();
    }

    fn on_entity_replaced(&mut self, old: Handle, new_handles: &[Handle]) {
        if Some(old) == self.live_handle {
            self.live_handle = new_handles.first().copied();
            if new_handles.len() > 1 {
                self.live_contour_handle = Some(new_handles[1]);
            }
        } else if Some(old) == self.live_contour_handle {
            if new_handles.is_empty() {
                self.live_contour_handle = None;
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        self.start_dimension_prompt()
    }

    fn enter_accepts_default_start(&self) -> bool {
        self.vertices.is_empty()
    }

    fn on_escape(&mut self) -> CmdResult {
        self.on_enter()
    }

    fn on_space_change(&mut self) -> CmdResult {
        self.on_enter()
    }

    fn wants_text_input(&self) -> bool {
        !self.vertices.is_empty()
    }

    fn point_step_accepts_keywords(&self) -> bool {
        !self.vertices.is_empty()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match text.trim().to_uppercase().as_str() {
            "U" | "UNDO" => Some(self.undo_last_vertex()),
            "A" | "ARC" => {
                self.arc_mode = true;
                Some(CmdResult::NeedPoint)
            }
            "L" | "LINE" => {
                self.arc_mode = false;
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if !self.vertices.is_empty() {
            Some(self.undo_last_vertex())
        } else {
            None
        }
    }

    fn live_properties(&self) -> Option<crate::command::LiveCommandProperties> {
        use crate::command::{LiveCommandField, LiveCommandProperties, LiveFieldValue};

        let style_name = match &self.style_id {
            Some(id) => self
                .library
                .as_ref()
                .and_then(|lib| lib.wall_styles.iter().find(|ws| &ws.style.id == id))
                .map(|ws| ws.style.name.clone())
                .unwrap_or_else(|| id.clone()),
            None => String::new(),
        };

        Some(LiveCommandProperties {
            title: crate::t!("Wall").into_owned(),
            fields: vec![
                LiveCommandField {
                    label: crate::t!("Style").into_owned(),
                    field_id: "wall_style",
                    value: LiveFieldValue::Picker(style_name),
                },
                LiveCommandField {
                    label: crate::t!("Height").into_owned(),
                    field_id: "wall_height",
                    value: LiveFieldValue::Number(self.wall.height),
                },
                LiveCommandField {
                    label: crate::t!("Justification").into_owned(),
                    field_id: "wall_justification",
                    value: LiveFieldValue::Choice {
                        selected: self.justification.as_str().to_string(),
                        options: vec![
                            "Interior".to_string(),
                            "Center".to_string(),
                            "Exterior".to_string(),
                        ],
                    },
                },
            ],
        })
    }

    fn live_property_id(&self, field_id: &str) -> Option<String> {
        match field_id {
            "wall_style" => self.style_id.clone(),
            _ => None,
        }
    }

    fn apply_live_property(
        &mut self,
        field_id: &str,
        value: crate::command::LiveFieldValue,
    ) -> CmdResult {
        use crate::command::LiveFieldValue;

        match (field_id, value) {
            ("wall_style", LiveFieldValue::Picker(style_id)) => {
                let Some(lib) = &self.library else {
                    return CmdResult::NeedPoint;
                };
                let Some(style) = lib
                    .wall_styles
                    .iter()
                    .find(|s| s.style.id == style_id)
                else {
                    return CmdResult::NeedPoint;
                };

                self.style_id = Some(style.style.id.clone());
                self.no_style_warning = false;
                self.resolved_layers =
                    resolve_wall_style_layers(lib, &style.style.id, None);

                if self.vertices.len() >= 2 {
                    self.commit_current_segment()
                } else {
                    CmdResult::NeedPoint
                }
            }
            ("wall_height", LiveFieldValue::Number(h)) => {
                self.wall.height = h;
                self.height_live_set = true;
                self.sync_live_if_previewable(false)
            }
            ("wall_justification", LiveFieldValue::Choice { selected, .. })
            | ("wall_justification", LiveFieldValue::Text(selected)) => {
                self.justification = WallJustification::from_str(&selected);
                self.sync_live_if_previewable(false)
            }
            _ => CmdResult::NeedPoint,
        }
    }
}


inventory::submit!(crate::command::CommandRegistration { names: &["AEC_WALL"] });
