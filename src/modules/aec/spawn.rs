//! AEC command spawn / dispatch hooks used by core.

use iced::Task;

use crate::app::{Message, OpenCADStudio};
use crate::command::CadCommand;
use crate::modules::aec::walls::{WallCommand, WallExtendCommand, WallJoinCommand};

/// Construct an interactive AEC `CadCommand` by verb name.
pub fn spawn_command(name: &str) -> Option<Box<dyn CadCommand>> {
    match name {
        "AEC_WALL" => Some(Box::new(WallCommand::new())),
        "AEC_WALLJOIN" => Some(Box::new(WallJoinCommand::new())),
        "AEC_WALLEXTEND" => Some(Box::new(WallExtendCommand::new())),
        _ => None,
    }
}

/// One-shot / interactive AEC dispatch. `None` means core should keep looking.
/// Step 1 keeps AEC arms in `dispatch_draw`; this hook is the stable core entry.
pub(crate) fn try_dispatch(
    app: &mut OpenCADStudio,
    cmd: &str,
    tab: usize,
) -> Option<Task<Message>> {
    if cmd != "AEC_CONTROLPLANES" {
        return None;
    }
    use crate::modules::aec::commands as aec;
    let i = tab;
    let visible = aec::toggle_controlplanes_layer(&mut app.tabs[i].scene);
    if visible {
        if let Some(storey) = app.aec.aec_project_explorer_file.as_mut().and_then(|p| {
            p.buildings
                .first_mut()
                .and_then(|b| b.storeys.first_mut())
        }) {
            crate::modules::aec::project::drawing_sync::sync_storey_planes_from_drawing(
                &app.tabs[i].scene,
                storey,
            );
            crate::modules::aec::project::preview::regenerate_control_plane_previews(
                &mut app.tabs[i].scene,
                storey,
            );
        }
        app.command_line
            .push_info(crate::t!("AEC_CONTROLPLANES: layer on, previews updated.").as_ref());
    } else {
        app.command_line
            .push_info(crate::t!("AEC_CONTROLPLANES: layer off.").as_ref());
    }
    Some(Task::none())
}
