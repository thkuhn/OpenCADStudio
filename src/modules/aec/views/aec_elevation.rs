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
use crate::modules::aec::engine::views::carrier::{
    create_view_carrier, sync_carrier_primitives, write_view_carrier_definitions,
};
use crate::modules::aec::engine::views::elevation_engine::{
    derive_elevation_view, ElevationEngineOptions,
};
use crate::modules::aec::engine::views::*;
use crate::modules::aec::views::din1356::{create_section_marker, Din1356SectionMarker};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

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
    pub flip_direction: bool,
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
            flip_direction: false,
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
                let (depth, flip) = if self.phase == ElevationPhase::Depth {
                    if let Some(cur) = cursor {
                        let n0 = Vector2::new(-dir.y, dir.x);
                        let w = Vector2::new(cur.x - start.x_m, cur.y - start.y_m);
                        let d_signed = w.dot(n0);
                        (d_signed.abs().max(0.5), d_signed < 0.0)
                    } else {
                        (self.depth.unwrap_or(15.0), self.flip_direction)
                    }
                } else {
                    (self.depth.unwrap_or(15.0), self.flip_direction)
                };

                let norm = if !flip {
                    Vector2::new(-dir.y, dir.x)
                } else {
                    Vector2::new(dir.y, -dir.x)
                };

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

        // Viewport placement preview box & crosshair in PlacementPoint phase
        if self.phase == ElevationPhase::PlacementPoint {
            if let Some(cur) = cursor {
                let cp = Point2::new(cur.x, cur.y);
                let sz_x = 10.0;
                let y0 = cp.y_m + self.z_min;
                let y1 = cp.y_m + self.z_max;
                wires.push((Point2::new(cp.x_m - 2.0, cp.y_m), Point2::new(cp.x_m + 2.0, cp.y_m)));
                wires.push((Point2::new(cp.x_m, cp.y_m - 2.0), Point2::new(cp.x_m, cp.y_m + 2.0)));
                wires.push((Point2::new(cp.x_m, y0), Point2::new(cp.x_m + sz_x, y0)));
                wires.push((Point2::new(cp.x_m + sz_x, y0), Point2::new(cp.x_m + sz_x, y1)));
                wires.push((Point2::new(cp.x_m + sz_x, y1), Point2::new(cp.x_m, y1)));
                wires.push((Point2::new(cp.x_m, y1), Point2::new(cp.x_m, y0)));
            }
        }

        wires
    }

    /// Commits the elevation and inserts the 2D View Carrier.
    pub fn commit_elevation(
        &mut self,
        scene: &mut Scene,
        insertion_pt: Point2,
        display_config_library: &crate::modules::aec::engine::library::DisplayConfigLibrary,
        style_library: &crate::modules::aec::engine::library::StyleLibrary,
        active_name: Option<&str>,
    ) -> Option<Handle> {
        let p1 = self.start_pt?;
        let p2 = self.end_pt?;

        let baseline_dir = (Vector2::new(p2.x - p1.x, p2.y - p1.y)).normalized().ok()?;
        let axes = ViewAxes::elevation_with_side(baseline_dir, self.flip_direction).ok()?;

        let z_min = Metres::new(self.z_min).ok()?;
        let z_max = Metres::new(self.z_max).ok()?;
        let z_range = VerticalRange::new(z_min, z_max).ok()?;

        let width = ((p2.x - p1.x).powi(2) + (p2.y - p1.y).powi(2)).sqrt();

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
                Point2::new(0.0, self.z_min),
                Point2::new(width.max(1.0), self.z_max),
            )),
        };

        let mut model_view = ModelView {
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

        let display_config = crate::modules::aec::engine::views::style_resolver::resolve_view_display_config(
            display_config_library,
            crate::modules::aec::engine::plan_view::ViewType::Elevation,
            Some(model_view.profile_id.as_uuid()),
            active_name,
        );
        model_view.profile_id = ViewProfileId::from_uuid(display_config.id);

        let collector = SceneGeometryCollector::collect_from_document(&scene.document, ModelId::new());
        let primitives = derive_elevation_view(
            &viewpoint,
            &model_view,
            &collector,
            &ElevationEngineOptions {
                display_config: Some(display_config),
                style_library: Some(style_library.clone()),
                ..ElevationEngineOptions::default()
            },
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
        let _ = crate::modules::aec::engine::views::sheet_annotation::append_overall_dimensions(
            &mut scene.document,
            &mut carrier,
            &primitives,
        );
        write_view_carrier_definitions(&mut scene.document, carrier.handle, &viewpoint, &model_view);

        let marker = Din1356SectionMarker::new_elevation(
            &self.name,
            Point2::new(p1.x, p1.y),
            Point2::new(p2.x, p2.y),
            viewpoint.id,
            Some(carrier.handle),
        )
        .with_depth(self.depth)
        .with_z_range(self.z_min, self.z_max)
        .with_flip(self.flip_direction);
        let marker_h = create_section_marker(&mut scene.document, &marker);
        crate::modules::aec::engine::owner_index::link_peers(&mut scene.document, marker_h, carrier.handle);
        carrier.marker_handle = Some(marker_h);
        crate::modules::aec::engine::views::carrier::write_view_carrier(&mut scene.document, &carrier);

        scene.bump_geometry();

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
                if let (Some(start), Some(end)) = (self.start_pt, self.end_pt) {
                    let dir = (Vector2::new(end.x - start.x, end.y - start.y))
                        .normalized()
                        .unwrap_or(Vector2::new(1.0, 0.0));
                    let n0 = Vector2::new(-dir.y, dir.x);
                    let w = Vector2::new(pt.x - start.x, pt.y - start.y);
                    let d_signed = w.dot(n0);
                    self.flip_direction = d_signed < 0.0;
                    self.depth = Some(d_signed.abs().max(0.5));
                }
                self.phase = ElevationPhase::PlacementPoint;
                CmdResult::NeedPoint
            }
            ElevationPhase::PlacementPoint => {
                self.placement_point = Some(pt);
                let start = self.start_pt.unwrap_or(DVec3::ZERO);
                let end = self.end_pt.unwrap_or(DVec3::new(10.0, 0.0, 0.0));
                let depth = self.depth.unwrap_or(15.0);
                let flip_flag = if self.flip_direction { 1 } else { 0 };
                CmdResult::Dispatch(format!(
                    "AEC_ELEVATION_DO {}|{}|{}|{}|{}|{},{}|{},{}|{},{}",
                    self.name,
                    self.z_min,
                    self.z_max,
                    depth,
                    flip_flag,
                    pt.x,
                    pt.y,
                    start.x,
                    start.y,
                    end.x,
                    end.y
                ))
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

/// Dispatched non-interactive elevation creation handler.
pub fn aec_elevation_do(
    scene: &mut Scene,
    command_line: &mut CommandLine,
    args: &str,
    display_config_library: &crate::modules::aec::engine::library::DisplayConfigLibrary,
    style_library: &crate::modules::aec::engine::library::StyleLibrary,
    active_name: Option<&str>,
) -> Option<Handle> {
    // Format: name|z_min|z_max|depth|flip_dir|ins_x,ins_y|start_x,start_y|end_x,end_y
    let mut parts = args.split('|');
    let name = parts.next()?.trim().to_string();
    let z_min = parts.next()?.parse::<f64>().ok()?;
    let z_max = parts.next()?.parse::<f64>().ok()?;
    let depth = parts.next()?.parse::<f64>().ok()?;
    let flip_dir = parts.next()?.parse::<u8>().unwrap_or(0) != 0;

    let ins_str = parts.next()?;
    let mut ins_parts = ins_str.split(',');
    let ins_x = ins_parts.next()?.parse::<f64>().ok()?;
    let ins_y = ins_parts.next()?.parse::<f64>().ok()?;

    let start_str = parts.next()?;
    let mut start_parts = start_str.split(',');
    let start_x = start_parts.next()?.parse::<f64>().ok()?;
    let start_y = start_parts.next()?.parse::<f64>().ok()?;

    let end_str = parts.next()?;
    let mut end_parts = end_str.split(',');
    let end_x = end_parts.next()?.parse::<f64>().ok()?;
    let end_y = end_parts.next()?.parse::<f64>().ok()?;

    let mut cmd = ElevationCommand::new_with_heights(z_min, z_max);
    cmd.name = name.clone();
    cmd.depth = Some(depth);
    cmd.flip_direction = flip_dir;
    cmd.start_pt = Some(DVec3::new(start_x, start_y, 0.0));
    cmd.end_pt = Some(DVec3::new(end_x, end_y, 0.0));

    let handle = cmd.commit_elevation(
        scene,
        Point2::new(ins_x, ins_y),
        display_config_library,
        style_library,
        active_name,
    )?;
    command_line.push_info(&format!("AEC: Ansicht '{}' an ({:.2}, {:.2}) erzeugt (Träger {}).", name, ins_x, ins_y, handle));
    Some(handle)
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

        let display_config_library = crate::modules::aec::engine::library::DisplayConfigLibrary::seed_default_configs();
        let style_library = crate::modules::aec::engine::library::StyleLibrary::default();
        let handle = cmd.commit_elevation(
            &mut scene,
            Point2::new(0.0, 20.0),
            &display_config_library,
            &style_library,
            Some("Ansicht Fassade"),
        );
        assert!(handle.is_some());
        let carrier_handle = handle.unwrap();

        let (marker_handle, marker) =
            crate::modules::aec::views::din1356::find_section_marker_by_carrier(
                &scene.document,
                carrier_handle,
            )
            .expect("marker must be found by carrier handle");
        assert_ne!(marker_handle, Handle::new(0));
        assert!(marker.is_elevation);
        assert_eq!(marker.carrier_handle, Some(carrier_handle));
        assert_eq!(marker.label, "Elevation");

        let carrier = crate::modules::aec::engine::views::carrier::read_view_carrier(
            &scene.document,
            scene.document.get_entity(carrier_handle).unwrap(),
        )
        .expect("carrier exists");
        assert_eq!(marker.viewpoint_id, carrier.viewpoint_id);
    }
}
