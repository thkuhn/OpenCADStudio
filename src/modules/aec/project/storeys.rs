//! `AEC_STOREY` one-shot command.

use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_STOREY",
        label: "Storey",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/layers/panel.svg")),
        event: ModuleEvent::Command("AEC_STOREY".to_string()),
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["AEC_STOREY"] });
