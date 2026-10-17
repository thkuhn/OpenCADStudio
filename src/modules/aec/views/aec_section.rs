//! `AEC_SECTION` interactive building section command.
//!
//! Allows architects to define straight and jogged section lines in floor plans,
//! visualizes the dynamic viewing direction and far-depth preview frustum polygon,
//! auto-initializes the vertical section range from project storey manager heights,
//! inserts DIN 1356 section marker lines and arrows, and generates an associative
//! 2D derived section view carrier in model space.

use glam::{DVec2, DVec3};

use acadrust::Handle;

use crate::command::{
    CadCommand, CmdOption, CmdResult, LiveCommandField, LiveCommandProperties, LiveFieldValue,
    WorkingPlane,
};
use crate::modules::aec::engine::project::ProjectFile;
use crate::modules::aec::engine::views::carrier::{
    create_view_carrier, sync_carrier_primitives, write_view_carrier_definitions,
};
use crate::modules::aec::engine::views::cut_engine::{derive_section_view, CutEngineOptions};
use crate::modules::aec::engine::views::*;
use crate::modules::aec::views::din1356::Din1356SectionSymbol;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_SECTION",
        label: "Section",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/aec/wall_create.svg")),
        event: ModuleEvent::Command("AEC_SECTION".to_string()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionPhase {
    StartPoint,
    NextPoint,
    ViewingDirection,
    PlacementPoint,
}

pub struct SectionCommand {
    pub phase: SectionPhase,
    pub points: Vec<DVec3>,
    pub is_jog_step: Vec<bool>,
    pub label: String,
    pub far_depth: Option<f64>,
    pub z_min: f64,
    pub z_max: f64,
    pub flip_direction: bool,
    pub view_dir: Option<DVec2>,
    pub placement_point: Option<DVec3>,
    pub is_jog_mode: bool,
    plane: WorkingPlane,
    last_committed: Option<Handle>,
}

impl SectionCommand {
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
            phase: SectionPhase::StartPoint,
            points: Vec::new(),
            is_jog_step: Vec::new(),
            label: "A - A".to_string(),
            far_depth: Some(5.0),
            z_min,
            z_max,
            flip_direction: false,
            view_dir: None,
            placement_point: None,
            is_jog_mode: false,
            plane: WorkingPlane::default(),
            last_committed: None,
        }
    }

    /// Builds the `SectionPath` from recorded points and jog flags.
    pub fn build_section_path(&self) -> Option<SectionPath> {
        if self.points.len() < 2 {
            return None;
        }

        if self.points.len() == 2 && self.is_jog_step.is_empty() {
            let p1 = Point2::new(self.points[0].x, self.points[0].y);
            let p2 = Point2::new(self.points[1].x, self.points[1].y);
            return Some(SectionPath::straight(p1, p2));
        }

        let mut segments = Vec::new();
        for i in 0..self.points.len() - 1 {
            let p1 = Point2::new(self.points[i].x, self.points[i].y);
            let p2 = Point2::new(self.points[i + 1].x, self.points[i + 1].y);
            let is_jog = self.is_jog_step.get(i).copied().unwrap_or(false);
            segments.push(SectionSegment::new(p1, p2, is_jog));
        }

        Some(SectionPath::jogged(segments))
    }

    /// Generates preview wire geometry showing the section path, viewing arrows, and depth frustum polygon.
    pub fn generate_preview_wires(&self, cursor: Option<DVec3>) -> Vec<(Point2, Point2)> {
        let mut wires = Vec::new();

        // 1. Current section line segments
        for i in 0..self.points.len().saturating_sub(1) {
            let p1 = Point2::new(self.points[i].x, self.points[i].y);
            let p2 = Point2::new(self.points[i + 1].x, self.points[i + 1].y);
            wires.push((p1, p2));
        }

        // 2. Rubberband to cursor in NextPoint phase
        if self.phase == SectionPhase::NextPoint {
            if let (Some(last), Some(cur)) = (self.points.last(), cursor) {
                wires.push((Point2::new(last.x, last.y), Point2::new(cur.x, cur.y)));
            }
        }

        // 3. DIN 1356 symbols & Depth frustum polygon in ViewingDirection or PlacementPoint phase
        if let Some(path) = self.build_section_path() {
            // Determine dynamic depth and flip in ViewingDirection phase
            let (depth, flip) = if self.phase == SectionPhase::ViewingDirection {
                if let (Some(cur), Some(first), Some(second)) =
                    (cursor, self.points.first(), self.points.get(1))
                {
                    let dir = (Vector2::new(second.x - first.x, second.y - first.y))
                        .normalized()
                        .unwrap_or(Vector2::new(1.0, 0.0));
                    let n0 = Vector2::new(-dir.y, dir.x);
                    let w = Vector2::new(cur.x - first.x, cur.y - first.y);
                    let d_signed = w.dot(n0);
                    (d_signed.abs().max(0.5), d_signed < 0.0)
                } else {
                    (self.far_depth.unwrap_or(5.0), self.flip_direction)
                }
            } else {
                (self.far_depth.unwrap_or(5.0), self.flip_direction)
            };

            let sym = Din1356SectionSymbol::new(&self.label, path.clone()).with_flip(flip);
            wires.extend(sym.generate_preview_wires());

            // Build frustum polygon
            match &path {
                SectionPath::Straight { start, end } => {
                    let dir = (Vector2::new(end.x_m - start.x_m, end.y_m - start.y_m))
                        .normalized()
                        .unwrap_or(Vector2::new(1.0, 0.0));
                    let norm = if !flip {
                        Vector2::new(-dir.y, dir.x)
                    } else {
                        Vector2::new(dir.y, -dir.x)
                    };
                    let p3 = Point2::new(end.x_m + norm.x * depth, end.y_m + norm.y * depth);
                    let p4 = Point2::new(start.x_m + norm.x * depth, start.y_m + norm.y * depth);
                    wires.push((*end, p3));
                    wires.push((p3, p4));
                    wires.push((p4, *start));
                }
                SectionPath::Jogged { segments } => {
                    for seg in segments {
                        if let Ok(dir) = seg.direction() {
                            let norm = if !flip {
                                Vector2::new(-dir.y, dir.x)
                            } else {
                                Vector2::new(dir.y, -dir.x)
                            };
                            let p3 = Point2::new(
                                seg.end.x_m + norm.x * depth,
                                seg.end.y_m + norm.y * depth,
                            );
                            let p4 = Point2::new(
                                seg.start.x_m + norm.x * depth,
                                seg.start.y_m + norm.y * depth,
                            );
                            wires.push((seg.end, p3));
                            wires.push((p3, p4));
                            wires.push((p4, seg.start));
                        }
                    }
                }
            }
        }

        // 4. Viewport placement preview box & crosshair in PlacementPoint phase
        if self.phase == SectionPhase::PlacementPoint {
            if let Some(cur) = cursor {
                let cp = Point2::new(cur.x, cur.y);
                let sz_x = 10.0;
                let sz_y = (self.z_max - self.z_min).max(3.0);
                wires.push((Point2::new(cp.x_m - 2.0, cp.y_m), Point2::new(cp.x_m + 2.0, cp.y_m)));
                wires.push((Point2::new(cp.x_m, cp.y_m - 2.0), Point2::new(cp.x_m, cp.y_m + 2.0)));
                wires.push((cp, Point2::new(cp.x_m + sz_x, cp.y_m)));
                wires.push((Point2::new(cp.x_m + sz_x, cp.y_m), Point2::new(cp.x_m + sz_x, cp.y_m + sz_y)));
                wires.push((Point2::new(cp.x_m + sz_x, cp.y_m + sz_y), Point2::new(cp.x_m, cp.y_m + sz_y)));
                wires.push((Point2::new(cp.x_m, cp.y_m + sz_y), cp));
            }
        }

        wires
    }

    /// Commits the section cut and inserts both DIN 1356 annotations and the 2D View Carrier.
    pub fn commit_section(&mut self, scene: &mut Scene, insertion_pt: Point2) -> Option<Handle> {
        let path = self.build_section_path()?;

        // 1. Insert DIN 1356 floor plan section line and arrows
        let sym = Din1356SectionSymbol::new(&self.label, path.clone()).with_flip(self.flip_direction);
        let din_entities = sym.generate_cad_entities("AEC_SECTION_MARK");
        for ent in din_entities {
            let _ = scene.document.add_entity(ent);
        }

        // 2. Build Viewpoint
        let first_seg = path.segments().first()?.clone();
        let cut_dir = first_seg.direction().ok()?;
        let axes = ViewAxes::section_with_side(cut_dir, self.flip_direction).ok()?;

        let z_min = Metres::new(self.z_min).ok()?;
        let z_max = Metres::new(self.z_max).ok()?;
        let z_range = VerticalRange::new(z_min, z_max).ok()?;

        let viewpoint = Viewpoint {
            id: ViewpointId::new(),
            name: self.label.clone(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(
                first_seg.start.x_m,
                first_seg.start.y_m,
                self.z_min,
            )),
            axes,
            kind: ViewKind::Section {
                path,
                far_depth: self.far_depth.and_then(|d| PositiveMetres::new(d).ok()),
            },
            z_range,
            mapping: Default::default(),
            crop: None,
        };

        // 3. Build ModelView
        let model_view = ModelView {
            id: ModelViewId::new(),
            name: format!("Section {}", self.label),
            viewpoint_id: viewpoint.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).ok()?, // 1:50
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: UpdatePolicy::Automatic,
            revisions: Default::default(),
        };

        // 4. Collect scene & derive 2D primitives
        let collector = SceneGeometryCollector::collect_from_document(&scene.document, ModelId::new());
        let mut primitives = derive_section_view(
            &viewpoint,
            &model_view,
            &collector,
            &CutEngineOptions::default(),
        )
        .unwrap_or_default();

        let _ = process_silhouette_and_depth(&mut primitives, &SilhouetteOptions::default());

        // 5. Create and sync DerivedViewCarrier in model space
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
        write_view_carrier_definitions(&mut scene.document, carrier.handle, &viewpoint, &model_view);
        scene.bump_geometry();

        self.last_committed = Some(carrier.handle);
        Some(carrier.handle)
    }
}

impl CadCommand for SectionCommand {
    fn name(&self) -> &'static str {
        "AEC_SECTION"
    }

    fn prompt(&self) -> String {
        match self.phase {
            SectionPhase::StartPoint => "Specify first point of section line:".into(),
            SectionPhase::NextPoint => {
                if self.is_jog_mode {
                    "Specify next jog point or [Done/Undo]:".into()
                } else {
                    "Specify next point of section line or [Jog/Done]:".into()
                }
            }
            SectionPhase::ViewingDirection => {
                "Specify viewing direction / depth or [Enter for default]:".into()
            }
            SectionPhase::PlacementPoint => {
                "Specify insertion point for section drawing in model space:".into()
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.phase {
            SectionPhase::StartPoint => vec![],
            SectionPhase::NextPoint => vec![
                CmdOption::new("Jog", "J"),
                CmdOption::new("Done", "D"),
                CmdOption::new("Undo", "U"),
            ],
            SectionPhase::ViewingDirection => vec![CmdOption::new("Default", "D")],
            SectionPhase::PlacementPoint => vec![],
        }
    }

    fn live_properties(&self) -> Option<LiveCommandProperties> {
        Some(LiveCommandProperties {
            title: "Section Properties".into(),
            fields: vec![
                LiveCommandField {
                    field_id: "label",
                    label: "Label".into(),
                    value: LiveFieldValue::Text(self.label.clone()),
                },
                LiveCommandField {
                    field_id: "far_depth",
                    label: "Far Depth (m)".into(),
                    value: LiveFieldValue::Number(self.far_depth.unwrap_or(5.0)),
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
            ("label", LiveFieldValue::Text(s)) => {
                self.label = s;
                CmdResult::NeedPoint
            }
            ("far_depth", LiveFieldValue::Number(v)) => {
                self.far_depth = Some(v.max(0.1));
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
            SectionPhase::StartPoint => {
                self.points.push(pt);
                self.phase = SectionPhase::NextPoint;
                CmdResult::NeedPoint
            }
            SectionPhase::NextPoint => {
                let is_jog = self.is_jog_mode;
                self.points.push(pt);
                let is_transition = if self.points.len() <= 2 {
                    false
                } else {
                    is_jog
                };
                self.is_jog_step.push(is_transition);
                self.is_jog_mode = false;
                if self.points.len() == 2 && !is_jog {
                    self.phase = SectionPhase::ViewingDirection;
                }
                CmdResult::NeedPoint
            }
            SectionPhase::ViewingDirection => {
                if let (Some(first), Some(second)) = (self.points.first(), self.points.get(1)) {
                    let dir = (Vector2::new(second.x - first.x, second.y - first.y))
                        .normalized()
                        .unwrap_or(Vector2::new(1.0, 0.0));
                    let n0 = Vector2::new(-dir.y, dir.x);
                    let w = Vector2::new(pt.x - first.x, pt.y - first.y);
                    let d_signed = w.dot(n0);
                    self.flip_direction = d_signed < 0.0;
                    self.far_depth = Some(d_signed.abs().max(0.5));
                }
                self.phase = SectionPhase::PlacementPoint;
                CmdResult::NeedPoint
            }
            SectionPhase::PlacementPoint => {
                self.placement_point = Some(pt);
                let depth = self.far_depth.unwrap_or(5.0);
                let flip_flag = if self.flip_direction { 1 } else { 0 };
                let pts_str = self
                    .points
                    .iter()
                    .map(|p| format!("{},{}", p.x, p.y))
                    .collect::<Vec<_>>()
                    .join(";");
                let jog_str = self
                    .is_jog_step
                    .iter()
                    .map(|b| if *b { "1" } else { "0" })
                    .collect::<Vec<_>>()
                    .join(",");
                CmdResult::Dispatch(format!(
                    "AEC_SECTION_DO {}|{}|{}|{}|{}|{},{}|{}|{}",
                    self.label,
                    self.z_min,
                    self.z_max,
                    depth,
                    flip_flag,
                    pt.x,
                    pt.y,
                    pts_str,
                    jog_str
                ))
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.phase {
            SectionPhase::NextPoint if self.points.len() >= 2 => {
                self.phase = SectionPhase::ViewingDirection;
                CmdResult::NeedPoint
            }
            SectionPhase::ViewingDirection => {
                self.phase = SectionPhase::PlacementPoint;
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match text.trim().to_uppercase().as_str() {
            "J" | "JOG" => {
                self.is_jog_mode = true;
                Some(CmdResult::NeedPoint)
            }
            "D" | "DONE" => {
                if self.phase == SectionPhase::NextPoint && self.points.len() >= 2 {
                    self.phase = SectionPhase::ViewingDirection;
                } else if self.phase == SectionPhase::ViewingDirection {
                    self.phase = SectionPhase::PlacementPoint;
                }
                Some(CmdResult::NeedPoint)
            }
            "U" | "UNDO" => {
                if self.points.len() > 1 {
                    self.points.pop();
                    self.is_jog_step.pop();
                } else {
                    self.points.clear();
                    self.phase = SectionPhase::StartPoint;
                }
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_preview_wires(&mut self, cursor: DVec3) -> Vec<WireModel> {
        let wires = self.generate_preview_wires(Some(cursor));
        let mut models = Vec::new();
        for (p1, p2) in wires {
            let pts = vec![
                [p1.x_m, p1.y_m, self.z_min],
                [p2.x_m, p2.y_m, self.z_min],
            ];
            models.push(WireModel::solid_f64("section_preview".into(), pts, WireModel::CYAN, false));
        }
        models
    }
}

/// Dispatched non-interactive section creation handler.
pub fn aec_section_do(scene: &mut Scene, command_line: &mut CommandLine, args: &str) -> Option<Handle> {
    // Format: label|z_min|z_max|far_depth|flip_dir|ins_x,ins_y|pt0_x,pt0_y;pt1_x,pt1_y...|jog0,jog1...
    let mut parts = args.split('|');
    let label = parts.next()?.trim().to_string();
    let z_min = parts.next()?.parse::<f64>().ok()?;
    let z_max = parts.next()?.parse::<f64>().ok()?;
    let far_depth = parts.next()?.parse::<f64>().ok()?;
    let flip_dir = parts.next()?.parse::<u8>().unwrap_or(0) != 0;

    let ins_str = parts.next()?;
    let mut ins_parts = ins_str.split(',');
    let ins_x = ins_parts.next()?.parse::<f64>().ok()?;
    let ins_y = ins_parts.next()?.parse::<f64>().ok()?;

    let pts_str = parts.next()?;
    let points: Vec<DVec3> = pts_str
        .split(';')
        .filter_map(|pair| {
            let mut it = pair.split(',');
            let x = it.next()?.parse::<f64>().ok()?;
            let y = it.next()?.parse::<f64>().ok()?;
            Some(DVec3::new(x, y, 0.0))
        })
        .collect();
    if points.len() < 2 {
        return None;
    }

    let is_jog_step: Vec<bool> = parts
        .next()
        .map(|jogs| {
            jogs.split(',')
                .map(|s| s.trim() == "1")
                .collect()
        })
        .unwrap_or_default();

    let mut cmd = SectionCommand::new_with_heights(z_min, z_max);
    cmd.label = label.clone();
    cmd.far_depth = Some(far_depth);
    cmd.flip_direction = flip_dir;
    cmd.points = points;
    cmd.is_jog_step = is_jog_step;

    let handle = cmd.commit_section(scene, Point2::new(ins_x, ins_y))?;
    command_line.push_info(&format!("AEC: Schnitt '{}' an ({:.2}, {:.2}) erzeugt (Träger {}).", label, ins_x, ins_y, handle));
    Some(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_section_command_straight_flow() {
        let mut cmd = SectionCommand::new();
        let mut scene = Scene::new();

        assert_eq!(cmd.phase, SectionPhase::StartPoint);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        assert_eq!(cmd.phase, SectionPhase::NextPoint);

        cmd.on_point(DVec3::new(10.0, 0.0, 0.0));
        assert_eq!(cmd.phase, SectionPhase::ViewingDirection);

        cmd.on_point(DVec3::new(10.0, 5.0, 0.0));
        assert_eq!(cmd.phase, SectionPhase::PlacementPoint);

        let handle = cmd.commit_section(&mut scene, Point2::new(20.0, 0.0));
        assert!(handle.is_some());
    }

    #[test]
    fn test_section_command_jogged_flow() {
        let mut cmd = SectionCommand::new();

        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_text_input("J");
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        cmd.on_text_input("J");
        cmd.on_point(DVec3::new(5.0, 2.0, 0.0));
        cmd.on_point(DVec3::new(10.0, 2.0, 0.0));
        cmd.on_text_input("D");

        let path = cmd.build_section_path().unwrap();
        assert_eq!(path.cut_length(), 10.0);
    }
}
