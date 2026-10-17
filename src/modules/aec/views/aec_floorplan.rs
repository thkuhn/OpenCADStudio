//! `AEC_FLOORPLAN` and `AEC_RCP` interactive plan view commands.
//!
//! Generates associative 2D floor plans and reflected ceiling plans at specified
//! cut-plane heights or storey levels.

use glam::DVec3;

use acadrust::Handle;

use crate::command::{
    CadCommand, CmdOption, CmdResult, LiveCommandField, LiveCommandProperties, LiveFieldValue,
    WorkingPlane,
};
use crate::modules::aec::engine::views::carrier::{create_view_carrier, sync_carrier_primitives};
use crate::modules::aec::engine::views::cut_engine::{derive_section_view, CutEngineOptions};
use crate::modules::aec::engine::views::*;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

pub fn floorplan_tool() -> ToolDef {
    ToolDef {
        id: "AEC_FLOORPLAN",
        label: "Floor Plan",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/aec/wall_create.svg")),
        event: ModuleEvent::Command("AEC_FLOORPLAN".to_string()),
    }
}

pub fn rcp_tool() -> ToolDef {
    ToolDef {
        id: "AEC_RCP",
        label: "Ceiling Plan",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/boundary.svg")),
        event: ModuleEvent::Command("AEC_RCP".to_string()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanViewKind {
    FloorPlan,
    ReflectedCeilingPlan,
}

pub struct FloorPlanCommand {
    pub kind: PlanViewKind,
    pub name: String,
    pub cut_height: f64,
    pub z_min: f64,
    pub z_max: f64,
    pub placement_point: Option<DVec3>,
    plane: WorkingPlane,
    last_committed: Option<Handle>,
}

impl FloorPlanCommand {
    pub fn new_floorplan() -> Self {
        Self {
            kind: PlanViewKind::FloorPlan,
            name: "Floor Plan EG".to_string(),
            cut_height: 1.0,
            z_min: 0.0,
            z_max: 2.8,
            placement_point: None,
            plane: WorkingPlane::default(),
            last_committed: None,
        }
    }

    pub fn new_rcp() -> Self {
        Self {
            kind: PlanViewKind::ReflectedCeilingPlan,
            name: "Reflected Ceiling Plan EG".to_string(),
            cut_height: 2.4,
            z_min: 0.0,
            z_max: 2.8,
            placement_point: None,
            plane: WorkingPlane::default(),
            last_committed: None,
        }
    }

    pub fn commit_plan_view(&mut self, scene: &mut Scene, insertion_pt: Point2) -> Option<Handle> {
        let (axes, view_kind) = match self.kind {
            PlanViewKind::FloorPlan => (
                ViewAxes::floor_plan(),
                ViewKind::FloorPlan {
                    range: PlanRange {
                        above_cut: NonNegativeMetres::new(1.8).unwrap_or(NonNegativeMetres::ZERO),
                        below_cut: NonNegativeMetres::new(1.0).unwrap_or(NonNegativeMetres::ZERO),
                        beyond_below: NonNegativeMetres::ZERO,
                    },
                },
            ),
            PlanViewKind::ReflectedCeilingPlan => (
                ViewAxes::ceiling_plan(),
                ViewKind::ReflectedCeilingPlan {
                    range: CeilingRange {
                        above_cut: NonNegativeMetres::new(0.4).unwrap_or(NonNegativeMetres::ZERO),
                        beyond_above: NonNegativeMetres::ZERO,
                    },
                },
            ),
        };

        let z_min = Metres::new(self.z_min).ok()?;
        let z_max = Metres::new(self.z_max).ok()?;
        let z_range = VerticalRange::new(z_min, z_max).ok()?;

        let viewpoint = Viewpoint {
            id: ViewpointId::new(),
            name: self.name.clone(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, self.cut_height)),
            axes,
            kind: view_kind,
            z_range,
            mapping: Default::default(),
            crop: None,
        };

        let model_view = ModelView {
            id: ModelViewId::new(),
            name: self.name.clone(),
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

        // For horizontal plan views, we can derive horizontal section cuts at cut_height
        let collector = SceneGeometryCollector::collect_from_document(&scene.document, ModelId::new());
        let p1 = Point2::new(-50.0, 0.0);
        let p2 = Point2::new(50.0, 0.0);
        let dummy_section_vp = Viewpoint {
            id: viewpoint.id,
            name: viewpoint.name.clone(),
            revision: viewpoint.revision,
            origin: viewpoint.origin.clone(),
            axes: ViewAxes::section(Vector2::new(1.0, 0.0)).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(p1, p2),
                far_depth: None,
            },
            z_range,
            mapping: Default::default(),
            crop: None,
        };

        let primitives = derive_section_view(&dummy_section_vp, &model_view, &collector, &CutEngineOptions::default()).unwrap_or_default();

        let mut carrier = create_view_carrier(
            &mut scene.document,
            viewpoint.id,
            model_view.id,
            insertion_pt,
            1.0,
            0.0,
            UpdatePolicy::Automatic,
        ).ok()?;

        let _ = sync_carrier_primitives(&mut scene.document, &mut carrier, &primitives);

        self.last_committed = Some(carrier.handle);
        Some(carrier.handle)
    }
}

impl CadCommand for FloorPlanCommand {
    fn name(&self) -> &'static str {
        match self.kind {
            PlanViewKind::FloorPlan => "AEC_FLOORPLAN",
            PlanViewKind::ReflectedCeilingPlan => "AEC_RCP",
        }
    }

    fn prompt(&self) -> String {
        "Specify insertion point for plan drawing in model space:".into()
    }

    fn options(&self) -> Vec<CmdOption> {
        vec![]
    }

    fn live_properties(&self) -> Option<LiveCommandProperties> {
        Some(LiveCommandProperties {
            title: match self.kind {
                PlanViewKind::FloorPlan => "Floor Plan Properties".into(),
                PlanViewKind::ReflectedCeilingPlan => "Ceiling Plan Properties".into(),
            },
            fields: vec![
                LiveCommandField {
                    field_id: "name",
                    label: "Name".into(),
                    value: LiveFieldValue::Text(self.name.clone()),
                },
                LiveCommandField {
                    field_id: "cut_height",
                    label: "Cut Height (m)".into(),
                    value: LiveFieldValue::Number(self.cut_height),
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
            ("cut_height", LiveFieldValue::Number(v)) => {
                self.cut_height = v;
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
        self.placement_point = Some(pt);
        let kind_code = match self.kind {
            PlanViewKind::FloorPlan => 0,
            PlanViewKind::ReflectedCeilingPlan => 1,
        };
        CmdResult::Dispatch(format!(
            "AEC_FLOORPLAN_DO {}|{}|{}|{}|{}|{},{}",
            kind_code,
            self.name,
            self.cut_height,
            self.z_min,
            self.z_max,
            pt.x,
            pt.y
        ))
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_preview_wires(&mut self, cursor: DVec3) -> Vec<WireModel> {
        let size = 2.0;
        let pts1 = vec![
            [cursor.x - size, cursor.y, 0.0],
            [cursor.x + size, cursor.y, 0.0],
        ];
        let pts2 = vec![
            [cursor.x, cursor.y - size, 0.0],
            [cursor.x, cursor.y + size, 0.0],
        ];
        vec![
            WireModel::solid_f64("plan_cross_h".into(), pts1, WireModel::CYAN, false),
            WireModel::solid_f64("plan_cross_v".into(), pts2, WireModel::CYAN, false),
        ]
    }
}

/// Dispatched non-interactive floor plan / ceiling plan creation handler.
pub fn aec_floorplan_do(scene: &mut Scene, command_line: &mut CommandLine, args: &str) -> Option<Handle> {
    // Format: kind_code|name|cut_height|z_min|z_max|ins_x,ins_y
    let mut parts = args.split('|');
    let kind_code = parts.next()?.parse::<u8>().ok()?;
    let name = parts.next()?.trim().to_string();
    let cut_height = parts.next()?.parse::<f64>().ok()?;
    let z_min = parts.next()?.parse::<f64>().ok()?;
    let z_max = parts.next()?.parse::<f64>().ok()?;

    let ins_str = parts.next()?;
    let mut ins_parts = ins_str.split(',');
    let ins_x = ins_parts.next()?.parse::<f64>().ok()?;
    let ins_y = ins_parts.next()?.parse::<f64>().ok()?;

    let kind = match kind_code {
        1 => PlanViewKind::ReflectedCeilingPlan,
        _ => PlanViewKind::FloorPlan,
    };

    let mut cmd = match kind {
        PlanViewKind::FloorPlan => FloorPlanCommand::new_floorplan(),
        PlanViewKind::ReflectedCeilingPlan => FloorPlanCommand::new_rcp(),
    };
    cmd.name = name.clone();
    cmd.cut_height = cut_height;
    cmd.z_min = z_min;
    cmd.z_max = z_max;

    let handle = cmd.commit_plan_view(scene, Point2::new(ins_x, ins_y))?;
    let kind_label = match kind {
        PlanViewKind::FloorPlan => "Grundriss",
        PlanViewKind::ReflectedCeilingPlan => "Deckenspiegel",
    };
    command_line.push_info(&format!("AEC: {} '{}' an ({:.2}, {:.2}) erzeugt (Träger {}).", kind_label, name, ins_x, ins_y, handle));
    Some(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_floorplan_command_creation() {
        let mut cmd = FloorPlanCommand::new_floorplan();
        let mut scene = Scene::new();
        cmd.on_point(DVec3::new(10.0, 10.0, 0.0));
        let handle = cmd.commit_plan_view(&mut scene, Point2::new(10.0, 10.0));
        assert!(handle.is_some());
    }

    #[test]
    fn test_rcp_command_creation() {
        let mut cmd = FloorPlanCommand::new_rcp();
        let mut scene = Scene::new();
        cmd.on_point(DVec3::new(10.0, 10.0, 0.0));
        let handle = cmd.commit_plan_view(&mut scene, Point2::new(10.0, 10.0));
        assert!(handle.is_some());
    }
}
