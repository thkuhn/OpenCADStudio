//! `AEC_STOREY` one-shot command.

use crate::modules::aec::commands::{ensure_storey_entity, STOREYS};
use crate::modules::aec::engine::Storey;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_STOREY",
        label: "Storey",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/layers/panel.svg")),
        event: ModuleEvent::Command("AEC_STOREY".to_string()),
    }
}

/// `AEC_STOREY` — append a storey (document entity + in-memory scaffold list).
pub fn aec_storey(scene: &mut Scene, command_line: &mut CommandLine) {
    let mut storeys = STOREYS.lock().unwrap();
    let next_id = storeys.len() as u32;
    let new_storey = Storey::new(
        format!("Level {}", next_id + 1),
        (next_id as f64) * 3.0,
        3.0,
    );
    storeys.push(new_storey.clone());
    drop(storeys);

    let handle = ensure_storey_entity(scene, next_id, Some(&new_storey));
    scene.bump_geometry();

    command_line.push_info(&crate::tr!(
        "aec",
        "storey-added",
        name = new_storey.name.as_str(),
        elevation = format!("{}", new_storey.elevation),
        handle = handle.to_string()
    ));
}


inventory::submit!(crate::command::CommandRegistration { names: &["AEC_STOREY"] });
