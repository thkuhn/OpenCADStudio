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
    pub base_facets: Vec<crate::modules::aec::engine::control_plane::ControlPlaneFacet>,
    pub top_facets: Vec<crate::modules::aec::engine::control_plane::ControlPlaneFacet>,
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
            base_facets: Vec::new(),
            top_facets: Vec::new(),
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
        let base = self.base_plane_id.and_then(&lookup);
        if let Some(base) = base.as_ref() {
            self.base_normal = base.unit_normal();
            self.base_plane_name = Some(base.name.clone());
            self.base_facets = base.facets.clone();
            let offset_base = base.offset(self.base_offset);
            if let Some(pt) = intersect_vertical_at_xy(x, y, &offset_base) {
                self.base_origin = pt;
            } else {
                self.base_origin = offset_base.origin;
            }
        } else {
            self.base_facets = Vec::new();
        }

        let top = self.top_plane_id.and_then(&lookup);
        if let Some(top) = top.as_ref() {
            self.top_normal = top.unit_normal();
            self.top_plane_name = Some(top.name.clone());
            self.top_facets = top.facets.clone();
            let offset_top = top.offset(self.top_offset);
            if let Some(pt) = intersect_vertical_at_xy(x, y, &offset_top) {
                self.top_origin = pt;
            } else {
                self.top_origin = offset_top.origin;
            }
        } else {
            self.top_facets = Vec::new();
        }

        match (base.as_ref(), top.as_ref()) {
            (Some(b), Some(t)) => {
                if let Some(h) = resolve_wall_height(x, y, b, t, self.base_offset, self.top_offset) {
                    if h.abs() > 1e-9 {
                        self.height = h.abs();
                    }
                }
            }
            (None, Some(_)) => {
                let h = self.top_origin[2] - self.base_origin[2];
                if h.abs() > 1e-9 {
                    self.height = h.abs();
                }
            }
            (Some(_), None) => {
                self.top_origin = [
                    self.base_origin[0],
                    self.base_origin[1],
                    self.base_origin[2] + self.height,
                ];
                self.top_normal = self.base_normal;
            }
            (None, None) => {}
        }
    }

    /// Evaluates base Z-height at world (x, y).
    pub fn base_z_at_xy(&self, x: f64, y: f64) -> f64 {
        if !self.base_facets.is_empty() {
            let dummy = crate::modules::aec::engine::control_plane::ControlPlane {
                id: uuid::Uuid::nil(),
                name: String::new(),
                origin: self.base_origin,
                normal: self.base_normal,
                facets: self.base_facets.clone(),
                face_handle: None,
                preview_handle: None,
                visible: true,
            };
            if let Some(z) = dummy.z_offset_at_xy(x, y, self.base_offset) {
                return z;
            } else {
                return self.base_origin[2] + self.base_offset;
            }
        }
        let n = self.base_normal;
        if n[2].abs() >= 1e-6 {
            let d = n[0] * (x - self.base_origin[0]) + n[1] * (y - self.base_origin[1]);
            self.base_origin[2] - d / n[2]
        } else {
            self.base_origin[2]
        }
    }

    /// Evaluates top Z-height at world (x, y).
    pub fn top_z_at_xy(&self, x: f64, y: f64) -> f64 {
        if !self.top_facets.is_empty() {
            let dummy = crate::modules::aec::engine::control_plane::ControlPlane {
                id: uuid::Uuid::nil(),
                name: String::new(),
                origin: self.top_origin,
                normal: self.top_normal,
                facets: self.top_facets.clone(),
                face_handle: None,
                preview_handle: None,
                visible: true,
            };
            if let Some(z) = dummy.z_offset_at_xy(x, y, self.top_offset) {
                return z;
            } else {
                return self.base_z_at_xy(x, y) + self.height + self.top_offset;
            }
        }
        if self.top_plane_id.is_none() && (self.top_origin[2] - self.base_origin[2]).abs() <= 1e-6 {
            return self.base_z_at_xy(x, y) + self.height;
        }
        let n = self.top_normal;
        if n[2].abs() >= 1e-6 {
            let d = n[0] * (x - self.top_origin[0]) + n[1] * (y - self.top_origin[1]);
            self.top_origin[2] - d / n[2]
        } else {
            self.top_origin[2]
        }
    }

    /// Evaluates top Z-height for a specific layer including its top_offset at world (x, y).
    pub fn layer_top_z_at_xy(&self, layer: &WallLayer, x: f64, y: f64) -> f64 {
        self.top_z_at_xy(x, y) + layer.top_offset
    }

    /// Evaluates base Z-height for a specific layer including its bottom_offset at world (x, y).
    pub fn layer_base_z_at_xy(&self, layer: &WallLayer, x: f64, y: f64) -> f64 {
        self.base_z_at_xy(x, y) + layer.bottom_offset
    }

    /// Evaluates wall height at world (x, y), ensuring non-negative result.
    pub fn height_at_xy(&self, x: f64, y: f64) -> f64 {
        (self.top_z_at_xy(x, y) - self.base_z_at_xy(x, y)).max(0.0)
    }

    /// Returns true if either the base or top plane is sloped or has multiple/sloped polygonal facets.
    pub fn is_sloped(&self) -> bool {
        (self.base_normal[2].abs() - 1.0).abs() > 1e-5
            || (self.top_normal[2].abs() - 1.0).abs() > 1e-5
            || self.base_facets.iter().any(|f| f.slope_degrees() > 1e-4)
            || self.top_facets.iter().any(|f| f.slope_degrees() > 1e-4)
            || self.base_facets.len() > 1
            || self.top_facets.len() > 1
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
        let tn = self.top_normal;
        self.top_origin[0] += tn[0] * dt;
        self.top_origin[1] += tn[1] * dt;
        self.top_origin[2] += tn[2] * dt;
        let h = self.height - db + dt;
        if h.abs() > 1e-9 {
            self.height = h.abs();
        }
    }

    /// Height from baked underside/top world points. Offsets are already
    /// folded into [`Self::base_origin`] / [`Self::top_origin`] by rebake
    /// and must not be applied again.
    pub fn height_from_snapshot(&self) -> Option<f64> {
        let base = crate::modules::aec::engine::control_plane::ControlPlane {
            id: uuid::Uuid::nil(),
            name: String::new(),
            origin: self.base_origin,
            normal: self.base_normal,
            facets: self.base_facets.clone(),
            face_handle: None,
            preview_handle: None,
            visible: true,
        };
        let top = crate::modules::aec::engine::control_plane::ControlPlane {
            id: uuid::Uuid::nil(),
            name: String::new(),
            origin: self.top_origin,
            normal: self.top_normal,
            facets: self.top_facets.clone(),
            face_handle: None,
            preview_handle: None,
            visible: true,
        };
        crate::modules::aec::engine::control_plane::resolve_wall_height(
            self.base_origin[0],
            self.base_origin[1],
            &base,
            &top,
            0.0,
            0.0,
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
    fn rebake_bakes_offset_top_and_snapshot_matches_height() {
        use crate::modules::aec::engine::project::StoreyRef;
        let storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        let mut wall = Wall::new("s", 2.5, 0);
        wall.bind_storey_planes(&storey, 1.0, 2.0);
        wall.base_offset = 0.1;
        wall.top_offset = -0.05;
        wall.rebake_planes(&storey, 1.0, 2.0);
        assert!((wall.base_origin[2] - 0.1).abs() < 1e-9);
        assert!((wall.top_origin[2] - 2.95).abs() < 1e-9);
        assert!((wall.height - 2.85).abs() < 1e-9);
        let snap = wall.height_from_snapshot().expect("snapshot");
        assert!(
            (snap - wall.height).abs() < 1e-12,
            "snapshot must not re-apply offsets, got {snap} vs {}",
            wall.height
        );
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

    #[test]
    fn sloped_wall_height_and_layer_evaluation() {
        let mut wall = Wall::new("s", 3.0, 0);
        wall.base_origin = [0.0, 0.0, 0.0];
        wall.base_normal = [0.0, 0.0, 1.0]; // flat base
        // top plane slopes at 30 deg along X: normal = [-sin(30), 0, cos(30)] = [-0.5, 0, 0.8660254]
        let p = crate::modules::aec::engine::control_plane::ControlPlane::from_slope(
            "Roof",
            [0.0, 0.0, 2.5],
            30.0,
            0.0,
        );
        wall.top_origin = p.origin;
        wall.top_normal = p.unit_normal();

        assert!(wall.is_sloped());
        let z_base = wall.base_z_at_xy(10.0, 0.0);
        assert!((z_base - 0.0).abs() < 1e-9);

        let z_top_0 = wall.top_z_at_xy(0.0, 0.0);
        let z_top_10 = wall.top_z_at_xy(10.0, 0.0);
        assert!((z_top_0 - 2.5).abs() < 1e-9);
        let expected_top_10 = 2.5 + 10.0 * 30.0_f64.to_radians().tan();
        assert!((z_top_10 - expected_top_10).abs() < 1e-6);

        let h_0 = wall.height_at_xy(0.0, 0.0);
        let h_10 = wall.height_at_xy(10.0, 0.0);
        assert!((h_0 - 2.5).abs() < 1e-9);
        assert!((h_10 - expected_top_10).abs() < 1e-6);

        let layer = WallLayer {
            material: "Insulation".into(),
            thickness: 0.1,
            function: "Insulation".into(),
            axis_offset: 0.0,
            bottom_offset: -0.1,
            top_offset: 0.05,
            layer_override: None,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
        };
        assert!((wall.layer_top_z_at_xy(&layer, 0.0, 0.0) - 2.55).abs() < 1e-9);
        assert!((wall.layer_base_z_at_xy(&layer, 0.0, 0.0) - (-0.1)).abs() < 1e-9);
    }

    #[test]
    fn multi_facet_wall_top_evaluation_and_step_heights() {
        use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};
        let f1 = ControlPlaneFacet::new(
            "Roof1",
            vec![
                [0.0, -5.0, 3.2],
                [5.0, -5.0, 3.2],
                [5.0, 5.0, 3.2],
                [0.0, 5.0, 3.2],
            ],
        );
        let f2 = ControlPlaneFacet::new(
            "Roof2",
            vec![
                [5.0, -5.0, 2.4],
                [10.0, -5.0, 2.4],
                [10.0, 5.0, 2.4],
                [5.0, 5.0, 2.4],
            ],
        );
        let cp = ControlPlane::from_facets("SteppedCeiling", vec![f1, f2]);
        let mut wall = Wall::new("s", 3.0, 0);
        wall.base_origin = [0.0, 0.0, 0.0];
        wall.base_normal = [0.0, 0.0, 1.0];
        wall.top_origin = cp.origin;
        wall.top_normal = cp.normal;
        wall.top_facets = cp.facets.clone();

        assert_eq!(wall.top_z_at_xy(2.0, 0.0), 3.2);
        assert_eq!(wall.top_z_at_xy(8.0, 0.0), 2.4);
        assert_eq!(wall.height_at_xy(2.0, 0.0), 3.2);
        assert_eq!(wall.height_at_xy(8.0, 0.0), 2.4);

        // At boundary x=5.0, lowest height applies:
        assert_eq!(wall.top_z_at_xy(5.0, 0.0), 2.4);
    }

    #[test]
    fn partial_facet_wall_falls_back_to_wall_height_outside_facets() {
        use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};
        let f1 = ControlPlaneFacet::new(
            "PartialRoof",
            vec![
                [0.0, -5.0, 4.0],
                [5.0, -5.0, 4.0],
                [5.0, 5.0, 4.0],
                [0.0, 5.0, 4.0],
            ],
        );
        let cp = ControlPlane::from_facets("PartialCeiling", vec![f1]);
        let mut wall = Wall::new("s", 2.8, 0);
        wall.base_origin = [0.0, 0.0, 0.0];
        wall.base_normal = [0.0, 0.0, 1.0];
        wall.top_origin = cp.origin;
        wall.top_normal = cp.normal;
        wall.top_facets = cp.facets.clone();

        // Under facet (x=2.0) -> facet height 4.0m
        assert_eq!(wall.top_z_at_xy(2.0, 0.0), 4.0);
        assert_eq!(wall.height_at_xy(2.0, 0.0), 4.0);

        // Outside facet (x=8.0) -> standard wall height 2.8m (0.0 + 2.8)
        assert_eq!(wall.top_z_at_xy(8.0, 0.0), 2.8);
        assert_eq!(wall.height_at_xy(8.0, 0.0), 2.8);
    }

    #[test]
    fn wall_rebake_only_adopts_assigned_plane_facets() {
        use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};
        use crate::modules::aec::engine::project::StoreyRef;

        let mut storey = StoreyRef::new_with_height("EG", 0.0, 2.8, "eg.dwg");
        let ceiling_id = storey.ceiling_plane_id;

        // Extra roof plane with sloped facet in the same storey
        let f1 = ControlPlaneFacet::new(
            "Roof1",
            vec![
                [0.0, -5.0, 5.0],
                [5.0, -5.0, 6.0],
                [5.0, 5.0, 6.0],
                [0.0, 5.0, 5.0],
            ],
        );
        let roof_cp = ControlPlane::from_facets("ShedRoof", vec![f1]);
        let roof_id = roof_cp.id;
        storey.control_planes.push(roof_cp);

        // Wall bound to standard ceiling plane (which has its own standard facet, not the extra roof plane's facet)
        let mut wall1 = Wall::new("s", 2.8, 0);
        wall1.base_plane_id = Some(storey.floor_plane_id);
        wall1.top_plane_id = Some(ceiling_id);
        wall1.rebake_planes(&storey, 0.0, 0.0);

        // Wall 1 must NOT have any facets adopted from the unassigned roof plane
        assert_eq!(wall1.top_facets.len(), 1);
        assert_eq!(wall1.top_facets[0].name, "EG_OKGH");
        assert!(!wall1.is_sloped());
        assert_eq!(wall1.top_z_at_xy(2.0, 0.0), 2.8);

        // Wall bound to roof plane (which has sloped facets)
        let mut wall2 = Wall::new("s", 2.8, 0);
        wall2.base_plane_id = Some(storey.floor_plane_id);
        wall2.top_plane_id = Some(roof_id);
        wall2.rebake_planes(&storey, 0.0, 0.0);

        // Wall 2 MUST have the roof facets
        assert_eq!(wall2.top_facets.len(), 1);
        assert_eq!(wall2.top_facets[0].name, "Roof1");
        assert!(wall2.is_sloped());
        assert!((wall2.top_z_at_xy(2.0, 0.0) - 5.4).abs() < 1e-9);
    }
}
