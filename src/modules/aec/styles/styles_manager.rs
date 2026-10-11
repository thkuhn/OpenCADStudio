//! `AEC_STYLES` ribbon tool (large button of the Styles group).

use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_STYLES",
        label: "Style Manager",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/aec/styles_manager.svg")),
        event: ModuleEvent::Command("AEC_STYLES".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ribbon_tool_opens_styles() {
        assert_eq!(tool().id, "AEC_STYLES");
    }
}
