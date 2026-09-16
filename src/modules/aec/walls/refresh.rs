//! `AEC_WALL_REFRESH` one-shot command.

use acadrust::Handle;
use acadrust::xdata::XDataValue;

use crate::modules::aec::commands::{
    read_aec_record, regenerate_wall_representation, wall_from_entity,
};
use crate::modules::aec::engine::StyleLibrary;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_WALL_REFRESH",
        label: "Refresh Walls",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/aec/wall_refresh.svg")),
        event: ModuleEvent::Command("AEC_WALL_REFRESH".to_string()),
    }
}

/// `AEC_WALL_REFRESH` — migration path for walls created before the
/// contour/hatch/solid representation existed: rebuild it for every
/// `WALL` entity in the document that doesn't already carry a
/// `derived_handles` list (new walls skip a redundant rebuild).
pub fn aec_wall_refresh(scene: &mut Scene, command_line: &mut CommandLine, library_override: Option<&StyleLibrary>) {
    let candidates: Vec<Handle> = scene
        .document
        .entities()
        .filter_map(|entity| {
            let record = read_aec_record(entity)?;
            match record.values.first() {
                Some(XDataValue::String(kind)) if kind == "WALL" => {
                    // Skip axes that already have derived entities so a
                    // refresh doesn't double-build representations for walls
                    // that still hold a valid package.
                    let wall = wall_from_entity(entity)?;
                    if wall.derived_handles.is_empty() {
                        Some(entity.common().handle)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        })
        .collect();

    let mut refreshed = 0usize;
    for handle in candidates {
        if regenerate_wall_representation(scene, handle, library_override).is_ok() {
            refreshed += 1;
        }
    }
    scene.bump_geometry();
    command_line.push_info(&crate::tr!(
        "aec",
        "wall-refresh",
        count = refreshed
    ));
}


inventory::submit!(crate::command::CommandRegistration { names: &["AEC_WALL_REFRESH"] });
