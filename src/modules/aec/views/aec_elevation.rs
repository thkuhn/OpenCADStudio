//! `AEC_ELEVATION` interactive building elevation / facade command.
//!
//! Allows architects to define elevation baselines and viewing depths,
//! visualizes the view frustum polygon, computes multi-storey facade projections
//! with opening slot components (contours, swing triangles, sills), and generates
//! an associative 2D derived elevation view carrier.

use glam::DVec3;

use acadrust::Handle;

use crate::command::{
    CadCommand, CmdOption, CmdResult, LiveCommandField, LiveCommandProperties, LiveFieldValue,
    WorkingPlane,
};
use crate::modules::aec::engine::project::ProjectFile;
use crate::modules::aec::engine::views::carrier::{create_view_carrier, sync_carrier_primitives};
use crate::modules::aec::engine::views::elevation_engine::{
    derive_elevation_view, ElevationEngineOptions,
};
use crate::modules::aec::engine::views::*;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_ELEVATION",
        label: "Elevation",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/aec/wall_create.svg")),
        event: ModuleEvent::Command("AEC_ELEVATION".to_string()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElevationPhase {
    StartPoint,
    EndPoint,
    Depth,
    PlacementPoint,
}

pub struct ElevationCommand {
    pub phase: ElevationPhase,
    pub start_pt: Option<DVec3>,
    pub end_pt: Option<DVec3>,
    pub name: String,
    pub depth: Option<f64>,
    pub z_min: f64,
    pub z_max: f64,
    pub placement_point: Option<DVec3>,
    plane: WorkingPlane,
    last_committed: Option<Handle>,
}

impl ElevationCommand {
    pub fn new() -> Self {
        Self::new_with_heights(0.0, 3.0)
    }

    pub fn new_with_project(project: Option<&ProjectFile>) -> Self {
        let (z_min, z_max) = if let Some(proj) = project {
            let vr = proj.default_vertical_range();
            (vr.z_min.get(), vr.z_max.get())
        } else {
            (0.0, 3.0)
        };
        Self::new_with_heights(z_min, z_max)
    }

    pub fn new_with_heights(z_min: f64, z_max: f64) -> Self {
        Self {
            phase: ElevationPhase::StartPoint,
            start_pt: None,
            end_pt: None,
            name: "Elevation".to_string(),
            depth: Some(15.0),
            z_min,
            z_max,
            placement_point: None,
            plane: WorkingPlane::default(),
            last_committed: None,
        }
    }

    /// Generates wire preview segments showing baseline, direction arrow, and depth frustum polygon.
    pub fn generate_preview_wires(&self, cursor: Option<DVec3>) -> Vec<(Point2, Point2)> {
        let mut wires = Vec::new();

        if let (Some(p1), Some(p2)) = (self.start_pt, self.end_pt.or(cursor)) {
            let start = Point2::new(p1.x, p1.y);
            let end = Point2::new(p2.x, p2.y);
            wires.push((start, end));

            if let Ok(dir) = (Vector2::new(end.x_m - start.x_m, end.y_m - start.y_m)).normalized()
            {
                let norm = Vector2::new(-dir.y, dir.x);
                let depth = self.depth.unwrap_or(15.0);

                let p3 = Point2::new(end.x_m + norm.x * depth, end.y_m + norm.y * depth);
                let p4 = Point2::new(start.x_m + norm.x * depth, start.y_m + norm.y * depth);

                wires.push((end, p3));
                wires.push((p3, p4));
                wires.push((p4, start));

                // Direction pointer in the middle
                let mid = Point2::new((start.x_m + end.x_m) * 0.5, (start.y_m + end.y_m) * 0.5);
                let arrow_tip = Point2::new(mid.x_m + norm.x * 1.5, mid.y_m + norm.y * 1.5);
                wires.push((mid, arrow_tip));
            }
        }

        wires
    }

    /// Commits the elevation and inserts the 2D View Carrier.
    pub fn commit_elevation(&mut self, scene: &mut Scene, insertion_pt: Point2) -> Option<Handle> {
        let p1 = self.start_pt?;
        let p2 = self.end_pt?;

        let baseline_dir = (Vector2::new(p2.x - p1.x, p2.y - p1.y)).normalized().ok()?;
        let right = Vector3::new(baseline_dir.x, baseline_dir.y, 0.0);
        let up = Vector3::new(0.0, 0.0, 1.0);
        let forward = Vector3::new(-baseline_dir.y, baseline_dir.x, 0.0);
        let axes = ViewAxes::new(right, up, forward);

        let z_min = Metres::new(self.z_min).ok()?;
        let z_max = Metres::new(self.z_max).ok()?;
        let z_range = VerticalRange::new(z_min, z_max).ok()?;

        let width = ((p2.x - p1.x).powi(2) + (p2.y - p1.y).powi(2)).sqrt();
        let height = self.z_max - self.z_min;

        let viewpoint = Viewpoint {
            id: ViewpointId::new(),
            name: self.name.clone(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(p1.x, p1.y, self.z_min)),
            axes,
            kind: ViewKind::Elevation {
                depth: DepthRange {
                    near: NonNegativeMetres::ZERO,
                    far: self.depth.and_then(|d| PositiveMetres::new(d).ok()),
                },
            },
            z_range,
            mapping: Default::default(),
            crop: Some(Rect2::new(
                Point2::new(0.0, 0.0),
                Point2::new(width.max(1.0), height.max(1.0)),
            )),
        };

        let model_view = ModelView {
            id: ModelViewId::new(),
            name: format!("Elevation {}", self.name),
            viewpoint_id: viewpoint.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(100.0).ok()?, // 1:100
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: UpdatePolicy::Automatic,
            revisions: Default::default(),
        };

        let collector = SceneGeometryCollector::collect_from_document(&scene.document, ModelId::new());
        let primitives = derive_elevation_view(
            &viewpoint,
            &model_view,
            &collector,
            &ElevationEngineOptions::default(),
        )
        .unwrap_or_default();

        let mut carrier = create_view_carrier(
            &mut scene.document,
            viewpoint.id,
            model_view.id,
            insertion_pt,
            1.0,
            0.0,
            UpdatePolicy::Automatic,
        )
        .ok()?;

        let _ = sync_carrier_primitives(&mut scene.document, &mut carrier, &primitives);

        self.last_committed = Some(carrier.handle);
        Some(carrier.handle)
    }
}

impl CadCommand for ElevationCommand {
    fn name(&self) -> &'static str {
        "AEC_ELEVATION"
    }

    fn prompt(&self) -> String {
        match self.phase {
            ElevationPhase::StartPoint => "Specify start point of elevation baseline:".into(),
            ElevationPhase::EndPoint => "Specify end point of elevation baseline:".into(),
            ElevationPhase::Depth => {
                "Specify viewing depth / direction or [Enter for default]:".into()
            }
            ElevationPhase::PlacementPoint => {
                "Specify insertion point for elevation drawing in model space:".into()
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.phase {
            ElevationPhase::StartPoint | ElevationPhase::EndPoint => vec![],
            ElevationPhase::Depth => vec![CmdOption::new("Default", "D")],
            ElevationPhase::PlacementPoint => vec![],
        }
    }

    fn live_properties(&self) -> Option<LiveCommandProperties> {
        Some(LiveCommandProperties {
            title: "Elevation Properties".into(),
            fields: vec![
                LiveCommandField {
                    field_id: "name",
                    label: "Name".into(),
                    value: LiveFieldValue::Text(self.name.clone()),
                },
                LiveCommandField {
                    field_id: "depth",
                    label: "Depth (m)".into(),
                    value: LiveFieldValue::Number(self.depth.unwrap_or(15.0)),
                },
                LiveCommandField {
                    field_id: "z_min",
                    label: "Z Min (m)".into(),
                    value: LiveFieldValue::Number(self.z_min),
                },
                LiveCommandField {
                    field_id: "z_max",
                    label: "Z Max (m)".into(),
                    value: LiveFieldValue::Number(self.z_max),
                },
            ],
        })
    }

    fn apply_live_property(&mut self, field_id: &str, value: LiveFieldValue) -> CmdResult {
        match (field_id, value) {
            ("name", LiveFieldValue::Text(s)) => {
                self.name = s;
                CmdResult::NeedPoint
            }
            ("depth", LiveFieldValue::Number(v)) => {
                self.depth = Some(v.max(0.1));
                CmdResult::NeedPoint
            }
            ("z_min", LiveFieldValue::Number(v)) => {
                self.z_min = v;
                CmdResult::NeedPoint
            }
            ("z_max", LiveFieldValue::Number(v)) => {
                self.z_max = v;
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.phase {
            ElevationPhase::StartPoint => {
                self.start_pt = Some(pt);
                self.phase = ElevationPhase::EndPoint;
                CmdResult::NeedPoint
            }
            ElevationPhase::EndPoint => {
                self.end_pt = Some(pt);
                self.phase = ElevationPhase::Depth;
                CmdResult::NeedPoint
            }
            ElevationPhase::Depth => {
                if let Some(start) = self.start_pt {
                    let d = (pt - start).length().max(0.5);
                    self.depth = Some(d);
                }
                self.phase = ElevationPhase::PlacementPoint;
                CmdResult::NeedPoint
            }
            ElevationPhase::PlacementPoint => {
                self.placement_point = Some(pt);
                CmdResult::NeedPoint
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.phase == ElevationPhase::Depth {
            self.phase = ElevationPhase::PlacementPoint;
        }
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if text.trim().to_uppercase() == "D" && self.phase == ElevationPhase::Depth {
            self.phase = ElevationPhase::PlacementPoint;
            Some(CmdResult::NeedPoint)
        } else {
            None
        }
    }

    fn on_preview_wires(&mut self, cursor: DVec3) -> Vec<WireModel> {
        let preview = self.generate_preview_wires(Some(cursor));
        let mut models = Vec::new();
        for (p1, p2) in preview {
            let pts = vec![
                [p1.x_m, p1.y_m, self.z_min],
                [p2.x_m, p2.y_m, self.z_min],
            ];
            models.push(WireModel::solid_f64("elevation_preview".into(), pts, WireModel::CYAN, false));
        }
        models
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elevation_command_flow() {
        let mut cmd = ElevationCommand::new();
        let mut scene = Scene::new();

        assert_eq!(cmd.phase, ElevationPhase::StartPoint);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        assert_eq!(cmd.phase, ElevationPhase::EndPoint);

        cmd.on_point(DVec3::new(10.0, 0.0, 0.0));
        assert_eq!(cmd.phase, ElevationPhase::Depth);

        cmd.on_point(DVec3::new(5.0, 5.0, 0.0));
        assert_eq!(cmd.phase, ElevationPhase::PlacementPoint);

        let handle = cmd.commit_elevation(&mut scene, Point2::new(0.0, 20.0));
        assert!(handle.is_some());
    }
}
