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

    pub fn update_all(
        &mut self,
        scene: &mut Scene,
        display_config_library: &crate::modules::aec::engine::library::DisplayConfigLibrary,
        style_library: &crate::modules::aec::engine::library::StyleLibrary,
        active_name: Option<&str>,
    ) -> usize {
        let carriers = all_view_carriers(&scene.document);
        let mut count = 0;
        let collector = SceneGeometryCollector::collect_from_document(&scene.document, ModelId::new());

        for carrier in carriers {
            if self.force_all || carrier.is_outdated {
                if let Ok(c) = regenerate_carrier(
                    &mut scene.document,
                    carrier.handle,
                    &[],
                    &[],
                    &collector,
                    display_config_library,
                    style_library,
                    active_name,
                ) {
                    count += c;
                }
            }
        }

        scene.bump_geometry();
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
        CmdResult::Dispatch("AEC_VIEWUPDATE_DO".to_string())
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
        let display_config_library = crate::modules::aec::engine::library::DisplayConfigLibrary::seed_default_configs();
        let style_library = crate::modules::aec::engine::library::seed_default_library();
        let count = cmd.update_all(
            &mut scene,
            &display_config_library,
            &style_library,
            None,
        );
        assert_eq!(count, 0); // No carriers in empty scene
    }
}
