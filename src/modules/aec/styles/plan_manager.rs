//! `AEC_PLANMANAGER` ribbon tool.

use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_PLANMANAGER",
        label: "Plan Types",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/aec/style_plan_kinds.svg")),
        event: ModuleEvent::Command("AEC_PLANMANAGER".to_string()),
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["AEC_PLANMANAGER"] });
