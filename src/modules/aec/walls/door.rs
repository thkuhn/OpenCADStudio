//! `AEC_DOOR` ribbon tool (shares [`super::window::WallOpeningCommand`]).

use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_DOOR",
        label: "Door",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/aec/wall_create.svg")),
        event: ModuleEvent::Command("AEC_DOOR".to_string()),
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["AEC_DOOR"] });
