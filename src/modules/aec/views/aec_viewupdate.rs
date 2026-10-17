//! `AEC_VIEWUPDATE` interactive command for refreshing outdated view carriers.
//!
//! Scans the document for derived view carriers and regenerates their 2D primitives
//! from current 3D BIM model geometry.

use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult};
use crate::modules::aec::engine::views::carrier::all_view_carriers;
use crate::modules::aec::engine::views::update_ops::regenerate_carrier;
use crate::modules::aec::engine::views::*;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_VIEWUPDATE",
        label: "Update Views",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/aec/wall_refresh.svg")),
        event: ModuleEvent::Command("AEC_VIEWUPDATE".to_string()),
    }
}

pub struct ViewUpdateCommand {
    pub updated_count: usize,
    pub force_all: bool,
}

impl ViewUpdateCommand {
    pub fn new() -> Self {
        Self {
            updated_count: 0,
            force_all: false,
        }
    }

    pub fn update_all(&mut self, scene: &mut Scene) -> usize {
        let carriers = all_view_carriers(&scene.document);
        let mut count = 0;
        let collector = SceneGeometryCollector::collect_from_document(&scene.document, ModelId::new());

        for carrier in carriers {
            if self.force_all || carrier.is_outdated {
                let viewpoint = Viewpoint {
                    id: carrier.viewpoint_id,
                    name: "View".to_string(),
                    revision: Revision(1),
                    origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
                    axes: ViewAxes::front_elevation(),
                    kind: ViewKind::Section {
                        path: SectionPath::straight(
                            Point2::new(-50.0, 0.0),
                            Point2::new(50.0, 0.0),
                        ),
                        far_depth: None,
                    },
                    z_range: VerticalRange::new(
                        Metres::new(0.0).unwrap(),
                        Metres::new(3.0).unwrap(),
                    )
                    .unwrap(),
                    mapping: Default::default(),
                    crop: None,
                };

                let model_view = ModelView {
                    id: carrier.model_view_id,
                    name: "View".to_string(),
                    viewpoint_id: viewpoint.id,
                    profile_id: ViewProfileId::new(),
                    scale: ScaleDenominator::new(50.0).unwrap(),
                    detail: DetailPolicy::FromProfileAndScale,
                    excluded_elements: Default::default(),
                    graphic_overrides: Vec::new(),
                    annotation_ids: Vec::new(),
                    update_policy: UpdatePolicy::Automatic,
                    revisions: Default::default(),
                };

                let _ = regenerate_carrier(
                    &mut scene.document,
                    carrier.handle,
                    &[viewpoint],
                    &[model_view],
                    &collector,
                );
                count += 1;
            }
        }

        self.updated_count = count;
        count
    }
}

impl CadCommand for ViewUpdateCommand {
    fn name(&self) -> &'static str {
        "AEC_VIEWUPDATE"
    }

    fn prompt(&self) -> String {
        "Updating outdated views... Press Enter to confirm or [All/Force]:".into()
    }

    fn options(&self) -> Vec<CmdOption> {
        vec![CmdOption::new("All", "A"), CmdOption::new("Force", "F")]
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match text.trim().to_uppercase().as_str() {
            "A" | "ALL" => {
                self.force_all = false;
                Some(CmdResult::NeedPoint)
            }
            "F" | "FORCE" => {
                self.force_all = true;
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_preview_wires(&mut self, _cursor: DVec3) -> Vec<WireModel> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_update_command() {
        let mut cmd = ViewUpdateCommand::new();
        let mut scene = Scene::new();
        let count = cmd.update_all(&mut scene);
        assert_eq!(count, 0); // No carriers in empty scene
    }
}
