//! `AEC_PROJECTEXPLORER` ribbon tool.

use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_PROJECTEXPLORER",
        label: "Project Explorer",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/layers/panel.svg")),
        event: ModuleEvent::Command("AEC_PROJECTEXPLORER".to_string()),
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["AEC_PROJECTEXPLORER"] });
