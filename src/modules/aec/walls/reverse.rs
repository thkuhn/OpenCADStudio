//! `AEC_WALLREVERSE` command.

use acadrust::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::modules::aec::commands::{
    is_wall_pick_target, resolve_wall_package, reverse_wall_in_document,
};
use crate::modules::aec::engine::{self, StyleLibrary};
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;
use std::collections::HashMap;

/// `AEC_WALLREVERSE` — interactive front-end: pick one wall entity (derived
/// contour/hatch/solid resolves to its axis via [`resolve_wall_package`]),
/// then reverse its axis direction and layer-stack side assignment.
pub struct WallReverseCommand {
    done: bool,
}

impl WallReverseCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self { done: false }
    }
}

impl CadCommand for WallReverseCommand {
    fn name(&self) -> &'static str {
        "AEC_WALLREVERSE"
    }

    fn prompt(&self) -> String {
        crate::tr!("aec", "wallreverse-select")
    }

    fn needs_entity_pick(&self) -> bool {
        !self.done
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        !self.done
    }

    fn entity_pick_hover_highlights_handle(&self, scene: &Scene, handle: Handle) -> bool {
        is_wall_pick_target(scene, handle)
    }

    /// Walls are rendered as filled contours, so a click anywhere inside the
    /// wall's body (not just precisely on its outline) must resolve to the
    /// wall entity; otherwise clicking a wall almost always misses.
    fn entity_pick_includes_fills(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.done = true;
        CmdResult::Dispatch(format!("AEC_WALLREVERSE_DO {}", handle.value()))
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// `AEC_WALLREVERSE_DO handle` — non-interactive handler dispatched by
/// [`WallReverseCommand`] once a wall is picked.
pub fn aec_wallreverse_do(
    scene: &mut Scene,
    command_line: &mut CommandLine,
    args: &str,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) {
    let Ok(val) = args.trim().parse::<u64>() else {
        command_line.push_error(&crate::tr!("aec", "wallreverse-malformed"));
        return;
    };
    let handle = resolve_wall_package(scene, Handle::new(val));
    if !is_wall_pick_target(scene, handle) {
        command_line.push_error(&crate::tr!("aec", "wallreverse-need-wall"));
        return;
    }
    match reverse_wall_in_document(
        scene,
        handle,
        library_override,
        display_rules,
        style_substitutions,
    ) {
        Ok(touched) => {
            let changes: Vec<_> = touched
                .into_iter()
                .map(|h| (h, crate::scene::ChangeKind::Modified))
                .collect();
            if !changes.is_empty() {
                scene.bump_entities(&changes);
            }
            command_line.push_info(&crate::tr!("aec", "wallreverse-ok"));
        }
        Err(e) => {
            command_line.push_error(&format!("AEC_WALLREVERSE: {e:?}"));
        }
    }
}


inventory::submit!(crate::command::CommandRegistration { names: &["AEC_WALLREVERSE"] });
