//! `AEC_CONTROLPLANES` ribbon tool.

use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_CONTROLPLANES",
        label: "Control planes",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/layers/panel.svg")),
        event: ModuleEvent::Command("AEC_CONTROLPLANES".to_string()),
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["AEC_CONTROLPLANES"] });
