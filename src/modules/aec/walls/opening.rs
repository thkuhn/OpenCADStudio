//! `AEC_OPENING` ribbon tool (breakthrough; shares [`super::window::WallOpeningCommand`]).

use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_OPENING",
        label: "Opening",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/aec/wall_create.svg")),
        event: ModuleEvent::Command("AEC_OPENING".to_string()),
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["AEC_OPENING"]
});
