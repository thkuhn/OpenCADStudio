//! `AEC_WINDOW` / shared wall opening command.

use acadrust::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::modules::aec::commands::{is_wall_pick_target, place_wall_opening};
use crate::modules::aec::engine::{self, StyleLibrary};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;
use std::collections::HashMap;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_WINDOW",
        label: "Window",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/aec/wall_create.svg")),
        event: ModuleEvent::Command("AEC_WINDOW".to_string()),
    }
}

/// `AEC_WINDOW` / `AEC_DOOR` — pick a wall, then a point along it to place
/// an opening with default dimensions.
pub struct WallOpeningCommand {
    kind: engine::openings::OpeningKind,
    wall: Option<Handle>,
}

impl WallOpeningCommand {
    #[allow(clippy::new_without_default)]
    pub fn new_window() -> Self {
        Self {
            kind: engine::openings::OpeningKind::Window,
            wall: None,
        }
    }

    #[allow(clippy::new_without_default)]
    pub fn new_door() -> Self {
        Self {
            kind: engine::openings::OpeningKind::Door,
            wall: None,
        }
    }
}

impl CadCommand for WallOpeningCommand {
    fn name(&self) -> &'static str {
        match self.kind {
            engine::openings::OpeningKind::Window => "AEC_WINDOW",
            engine::openings::OpeningKind::Door => "AEC_DOOR",
        }
    }

    fn prompt(&self) -> String {
        let tag = self.name();
        if self.wall.is_none() {
            crate::tr!("aec", "opening-select-wall", cmd = tag)
        } else {
            crate::tr!("aec", "opening-specify-point", cmd = tag)
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.wall.is_none()
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        self.wall.is_none()
    }

    fn entity_pick_hover_highlights_handle(&self, scene: &Scene, handle: Handle) -> bool {
        is_wall_pick_target(scene, handle)
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.wall = Some(handle);
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let Some(wall) = self.wall else {
            return CmdResult::NeedPoint;
        };
        let kind_flag = match self.kind {
            engine::openings::OpeningKind::Window => "W",
            engine::openings::OpeningKind::Door => "D",
        };
        CmdResult::Dispatch(format!(
            "AEC_WALLOPENING_DO {}|{}|{},{},{}",
            wall.value(),
            kind_flag,
            pt.x,
            pt.y,
            pt.z
        ))
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// `AEC_WALLOPENING_DO handle|W|x,y,z` or `...|D|x,y,z` — non-interactive
/// handler dispatched by [`WallOpeningCommand`].
pub fn aec_wallopening_do(
    scene: &mut Scene,
    command_line: &mut CommandLine,
    args: &str,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) {
    let parts: Vec<&str> = args.split('|').collect();
    if parts.len() != 3 {
        command_line.push_error(&crate::tr!("aec", "opening-malformed-args"));
        return;
    }
    let Ok(wall_val) = parts[0].parse::<u64>() else {
        command_line.push_error(&crate::tr!("aec", "opening-malformed-handle"));
        return;
    };
    let kind = match parts[1] {
        "D" | "d" | "Door" | "door" => engine::openings::OpeningKind::Door,
        _ => engine::openings::OpeningKind::Window,
    };
    let xyz: Vec<&str> = parts[2].split(',').collect();
    if xyz.len() < 2 {
        command_line.push_error(&crate::tr!("aec", "opening-malformed-point"));
        return;
    }
    let Ok(x) = xyz[0].parse::<f64>() else {
        command_line.push_error(&crate::tr!("aec", "opening-malformed-point"));
        return;
    };
    let Ok(y) = xyz[1].parse::<f64>() else {
        command_line.push_error(&crate::tr!("aec", "opening-malformed-point"));
        return;
    };
    let z = xyz.get(2).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);

    match place_wall_opening(
        scene,
        Handle::new(wall_val),
        DVec3::new(x, y, z),
        kind,
        library_override,
        display_rules,
        style_substitutions,
    ) {
        Ok((_opening, touched)) => {
            let changes: Vec<_> = touched
                .into_iter()
                .map(|h| (h, crate::scene::ChangeKind::Modified))
                .collect();
            if !changes.is_empty() {
                scene.bump_entities(&changes);
            }
            let label = kind.as_str();
            command_line.push_info(&crate::tr!("aec", "opening-placed", kind = label));
        }
        Err(e) => {
            command_line.push_error(&crate::tr!("aec", "opening-error", error = e.to_string()));
        }
    }
}


inventory::submit!(crate::command::CommandRegistration { names: &["AEC_WINDOW"] });
