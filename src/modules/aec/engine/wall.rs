//! `Wall` domain model.
//!
//! Mirrors the `WALL` XDATA record (APPID `OPENCAD_AEC`): style id, height,
//! storey id, multi-layer snapshot, derived representation handles, and
//! justification.

use crate::modules::aec::engine::display_component::ComponentStyleOverride;
use crate::modules::aec::engine::plan_view::PlanPhase;
use acadrust::Handle;

/// A parametric multi-layer wall: a baseline polyline (owned by the host
/// entity, not by this struct) plus the metadata needed to extrude/render it
/// and to write the `WALL` XDATA record.
#[derive(Debug, Clone, PartialEq)]
pub struct Wall {
    pub style_id: String,
    pub height: f64,
    pub storey_id: u32,
    pub layers: Vec<WallLayer>,
    /// Handles of the contour/hatch/solid entities most recently derived
    /// from this wall's axis, so they can be cleanly replaced or removed.
    pub derived_handles: Vec<Handle>,
    /// Informational field: which justification was used when drawing.
    pub justification: WallJustification,
    /// Construction/planning phase (Neu/Abbruch/Bestand), used by a
    /// `DisplayConfig`'s `phase_filter` to hide/style this wall differently
    /// per plan. Defaults to `New` for walls drawn without an explicit
    /// phase selection.
    pub phase: PlanPhase,
    /// Per-wall-instance override of the layer hatch angle/relativity,
    /// taking precedence over any style-profile (`ComponentRuleSet`)
    /// override, which in turn takes precedence over the material's own
    /// `hatch_angle`/`hatch_angle_relative` (Step 3 hatch-angle chain).
    /// Only `hatch_angle`/`hatch_angle_relative` are meaningful here; other
    /// fields are unused for this purpose.
    pub hatch_override: Option<ComponentStyleOverride>,
    pub base_plane_id: Option<uuid::Uuid>,
    pub top_plane_id: Option<uuid::Uuid>,
    /// Last persisted plane names (XDATA); used when the project is not loaded.
    pub base_plane_name: Option<String>,
    pub top_plane_name: Option<String>,
    pub base_offset: f64,
    pub top_offset: f64,
    pub base_origin: [f64; 3],
    pub base_normal: [f64; 3],
    pub top_origin: [f64; 3],
    pub top_normal: [f64; 3],
}

/// One material layer in a wall's cross-section snapshot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WallLayer {
    pub material: String,
    pub thickness: f64,
    pub function: String,
    pub axis_offset: f64,
    pub bottom_offset: f64,
    pub top_offset: f64,
    pub layer_override: Option<String>,
    /// Optional hatch pattern override, taking precedence over the
    /// material's own hatch pattern when rendering this layer's 2D hatch.
    /// Additive field (mirrors [`crate::modules::aec::engine::wall_style::Layer::hatch_override`]).
    pub hatch_override: Option<String>,
    /// Stable identity copied from the style layer.
    pub layer_id: uuid::Uuid,
}

/// Axis justification relative to the wall's total thickness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallJustification {
    Interior,
    Center,
    Exterior,
}

impl WallJustification {
    pub fn next(self) -> Self {
        match self {
            WallJustification::Interior => WallJustification::Center,
            WallJustification::Center => WallJustification::Exterior,
            WallJustification::Exterior => WallJustification::Interior,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            WallJustification::Interior => "Interior",
            WallJustification::Center => "Center",
            WallJustification::Exterior => "Exterior",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "Interior" => WallJustification::Interior,
            "Exterior" => WallJustification::Exterior,
            _ => WallJustification::Center,
        }
    }

    pub fn offset(&self, total_thickness: f64) -> f64 {
        match self {
            WallJustification::Center => 0.0,
            WallJustification::Interior => total_thickness * -0.5,
            WallJustification::Exterior => total_thickness * 0.5,
        }
    }
}

impl Wall {
    /// Creates a wall with no layers and center justification.
    pub fn new(style_id: impl Into<String>, height: f64, storey_id: u32) -> Self {
        Self {
            style_id: style_id.into(),
            height,
            storey_id,
            layers: Vec::new(),
            derived_handles: Vec::new(),
            justification: WallJustification::Center,
            phase: PlanPhase::default(),
            hatch_override: None,
            base_plane_id: None,
            top_plane_id: None,
            base_plane_name: None,
            top_plane_name: None,
            base_offset: 0.0,
            top_offset: 0.0,
            base_origin: [0.0, 0.0, 0.0],
            base_normal: [0.0, 0.0, 1.0],
            top_origin: [0.0, 0.0, 0.0],
            top_normal: [0.0, 0.0, 1.0],
        }
    }

    /// Bind floor/ceiling of `storey` and bake height at `(x, y)`.
    pub fn bind_storey_planes(
        &mut self,
        storey: &crate::modules::aec::engine::project::StoreyRef,
        x: f64,
        y: f64,
    ) {
        self.base_plane_id = Some(storey.floor_plane_id);
        self.top_plane_id = Some(storey.ceiling_plane_id);
        self.base_plane_name = storey.plane(storey.floor_plane_id).map(|p| p.name.clone());
        self.top_plane_name = storey.plane(storey.ceiling_plane_id).map(|p| p.name.clone());
        self.rebake_planes(storey, x, y);
    }

    pub fn rebake_planes(
        &mut self,
        storey: &crate::modules::aec::engine::project::StoreyRef,
        x: f64,
        y: f64,
    ) {
        self.rebake_lookup(|id| storey.plane(id).cloned(), x, y);
    }

    pub fn rebake_from_project(
        &mut self,
        project: &crate::modules::aec::engine::project::ProjectFile,
        x: f64,
        y: f64,
    ) {
        self.rebake_lookup(|id| project.control_plane(id).cloned(), x, y);
    }

    fn rebake_lookup(
        &mut self,
        lookup: impl Fn(uuid::Uuid) -> Option<crate::modules::aec::engine::control_plane::ControlPlane>,
        x: f64,
        y: f64,
    ) {
        use crate::modules::aec::engine::control_plane::{
            intersect_vertical_at_xy, resolve_wall_height,
        };
        let Some(base) = self.base_plane_id.and_then(&lookup) else {
            return;
        };
        self.base_normal = base.unit_normal();
        let offset_base = base.offset(self.base_offset);
        if let Some(pt) = intersect_vertical_at_xy(x, y, &offset_base) {
            self.base_origin = pt;
        } else {
            self.base_origin = offset_base.origin;
        }
        let Some(top) = self.top_plane_id.and_then(&lookup) else {
            return;
        };
        self.top_origin = top.origin;
        self.top_normal = top.unit_normal();
        if let Some(h) = resolve_wall_height(x, y, &base, &top, self.base_offset, self.top_offset) {
            if h.abs() > 1e-9 {
                self.height = h.abs();
            }
        }
    }

    /// Shift the wall base along its normal by Δ(base offset). Height is
    /// adjusted so the top world-Z stays put; top offset only changes height.
    pub fn apply_plane_offsets(&mut self, base: Option<f64>, top: Option<f64>) {
        let old_base = self.base_offset;
        let old_top = self.top_offset;
        if let Some(v) = base {
            self.base_offset = v;
        }
        if let Some(v) = top {
            self.top_offset = v;
        }
        let db = self.base_offset - old_base;
        let dt = self.top_offset - old_top;
        let n = self.base_normal;
        self.base_origin[0] += n[0] * db;
        self.base_origin[1] += n[1] * db;
        self.base_origin[2] += n[2] * db;
        let h = self.height - db + dt;
        if h.abs() > 1e-9 {
            self.height = h.abs();
        }
    }

    /// Height from baked plane Z when live IDs are unavailable.
    pub fn height_from_snapshot(&self) -> Option<f64> {
        let base = crate::modules::aec::engine::control_plane::ControlPlane {
            id: uuid::Uuid::nil(),
            name: String::new(),
            origin: self.base_origin,
            normal: self.base_normal,
            face_handle: None,
            preview_handle: None,
            visible: true,
        };
        let top = crate::modules::aec::engine::control_plane::ControlPlane {
            id: uuid::Uuid::nil(),
            name: String::new(),
            origin: self.top_origin,
            normal: self.top_normal,
            face_handle: None,
            preview_handle: None,
            visible: true,
        };
        crate::modules::aec::engine::control_plane::resolve_wall_height(
            0.0,
            0.0,
            &base,
            &top,
            self.base_offset,
            self.top_offset,
        )
        .filter(|h| h.abs() > 1e-9)
        .map(|h| h.abs())
    }

    /// Total cross-section width spanning all layers (min start to max end).
    pub fn total_thickness(&self) -> f64 {
        if self.layers.is_empty() {
            return 0.0;
        }
        let mut min_start = f64::INFINITY;
        let mut max_end = f64::NEG_INFINITY;
        for l in &self.layers {
            min_start = min_start.min(l.axis_offset);
            max_end = max_end.max(l.axis_offset + l.thickness);
        }
        (max_end - min_start).max(0.0)
    }

    /// Volume of the wall given the length of its (2D) baseline centerline.
    /// `volume = length * total_thickness * height`.
    pub fn volume(&self, baseline_length: f64) -> f64 {
        baseline_length * self.total_thickness() * self.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_is_length_times_thickness_times_height() {
        let mut wall = Wall::new("s", 2.8, 0);
        wall.layers.push(WallLayer {
            material: "Concrete".into(),
            thickness: 0.2,
            function: "Structural".into(),
            axis_offset: -0.1,
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
        });
        assert!((wall.volume(5.0) - 2.8).abs() < 1e-9);
    }

    #[test]
    fn base_plane_sets_wall_base_z() {
        use crate::modules::aec::engine::project::StoreyRef;
        let storey = StoreyRef::new_with_height("EG", 3.0, 2.8, "eg.dwg");
        let mut wall = Wall::new("s", 2.5, 0);
        wall.base_origin[2] = 0.0;
        wall.base_plane_id = Some(storey.floor_plane_id);
        wall.rebake_planes(&storey, 1.0, 2.0);
        assert!((wall.base_origin[2] - 3.0).abs() < 1e-9);
        assert!((wall.height - 2.5).abs() < 1e-9);
        wall.base_offset = 0.1;
        wall.rebake_planes(&storey, 1.0, 2.0);
        assert!((wall.base_origin[2] - 3.1).abs() < 1e-9);
    }

    #[test]
    fn rebake_base_and_top_from_different_storeys() {
        use crate::modules::aec::engine::project::{Building, ProjectFile, StoreyRef};
        let eg = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        let og = StoreyRef::new_with_height("OG", 3.0, 3.0, "og.dwg");
        let eg_floor = eg.floor_plane_id;
        let og_top = og.ceiling_plane_id;
        let mut project = ProjectFile::default();
        let mut building = Building::new("B");
        building.storeys.push(eg);
        building.storeys.push(og);
        project.buildings.push(building);
        let mut wall = Wall::new("s", 2.5, 0);
        wall.base_origin[2] = 3.0;
        wall.base_plane_id = Some(eg_floor);
        wall.top_plane_id = Some(og_top);
        wall.rebake_from_project(&project, 0.0, 0.0);
        assert!((wall.base_origin[2] - 0.0).abs() < 1e-9);
        assert!((wall.height - 6.0).abs() < 1e-6);
    }

    #[test]
    fn apply_base_offset_moves_underside_not_top() {
        let mut wall = Wall::new("s", 3.0, 0);
        wall.base_origin[2] = 0.0;
        wall.apply_plane_offsets(Some(0.2), None);
        assert!((wall.base_origin[2] - 0.2).abs() < 1e-9);
        assert!((wall.height - 2.8).abs() < 1e-9);
        wall.apply_plane_offsets(None, Some(-0.1));
        assert!((wall.base_origin[2] - 0.2).abs() < 1e-9);
        assert!((wall.height - 2.7).abs() < 1e-9);
    }

    #[test]
    fn new_wall_has_no_layers() {
        let wall = Wall::new("s", 2.8, 1);
        assert!(wall.layers.is_empty());
        assert_eq!(wall.storey_id, 1);
        assert_eq!(wall.justification, WallJustification::Center);
    }

    #[test]
    fn total_thickness_spans_axis_offsets() {
        let mut wall = Wall::new("s", 3.0, 0);
        wall.layers = vec![
            WallLayer {
                material: "A".into(),
                thickness: 0.1,
                function: "Finish".into(),
                axis_offset: -0.15,
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                layer_id: uuid::Uuid::new_v4(),
            },
            WallLayer {
                material: "B".into(),
                thickness: 0.2,
                function: "Structural".into(),
                axis_offset: -0.05,
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                layer_id: uuid::Uuid::new_v4(),
            },
        ];
        // span from -0.15 to 0.15
        assert!((wall.total_thickness() - 0.30).abs() < 1e-9);
    }
}
