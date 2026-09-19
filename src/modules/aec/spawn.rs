//! AEC command spawn / dispatch hooks used by core.

use std::path::Path;

use acadrust::Handle;
use iced::Task;

use crate::app::{Message, OpenCADStudio};
use crate::command::CadCommand;
use crate::modules::aec::engine::join_ops::aec_walljoin_do;
use crate::modules::aec::engine::xdata::ensure_wall_app_id;
use crate::modules::aec::engine::wall_package::{is_wall_pick_target, resolve_wall_package};
use crate::modules::aec::engine::wall_regen::toggle_controlplanes_layer;
use crate::modules::aec::ifc::export::aec_ifc_export;
use crate::modules::aec::rooms::room::aec_room;
use crate::modules::aec::rooms::schedule::aec_room_schedule;
use crate::modules::aec::walls::refresh::aec_wall_refresh;
use crate::modules::aec::walls::extend::aec_wallextend_do;
use crate::modules::aec::walls::window::aec_wallopening_do;
use crate::modules::aec::walls::reverse::aec_wallreverse_do;
use crate::modules::aec::walls::wall::WallCommand;
use crate::modules::aec::engine::project::{ProjectFile, StoreyRef};
use crate::modules::aec::message::AecMessage;
use crate::modules::aec::walls::reverse::WallReverseCommand;
use crate::modules::aec::walls::{WallExtendCommand, WallJoinCommand, WallOpeningCommand};

/// Construct an interactive AEC `CadCommand` by verb name.
///
/// Context-free factory: library, session defaults and storey planes are
/// applied by [`try_dispatch`] (or the caller) before the command is installed.
pub fn spawn_command(name: &str) -> Option<Box<dyn CadCommand>> {
    match name {
        "AEC_WALL" => Some(Box::new(WallCommand::new())),
        "AEC_WALLJOIN" => Some(Box::new(WallJoinCommand::new())),
        "AEC_WALLEXTEND" => Some(Box::new(WallExtendCommand::new())),
        "AEC_WALLREVERSE" => Some(Box::new(WallReverseCommand::new())),
        "AEC_WINDOW" => Some(Box::new(WallOpeningCommand::new_window())),
        "AEC_DOOR" => Some(Box::new(WallOpeningCommand::new_door())),
        _ => None,
    }
}

/// One-shot / interactive AEC dispatch. `None` means core should keep looking.
pub(crate) fn try_dispatch(
    app: &mut OpenCADStudio,
    cmd: &str,
    tab: usize,
) -> Option<Task<Message>> {
    if !cmd.starts_with("AEC_") {
        return None;
    }
    match cmd {
        "AEC_MATERIALMANAGER" => Some(Task::done(Message::Aec(AecMessage::AecMaterialManagerOpen))),
        "AEC_PROJECTEXPLORER" => Some(Task::done(Message::Aec(AecMessage::AecProjectExplorerOpen))),
        "AEC_STYLEMANAGER" => Some(Task::done(Message::Aec(
            AecMessage::AecWallStyleManagerOpen,
        ))),
        "AEC_PLANMANAGER" => Some(Task::done(Message::Aec(AecMessage::AecPlanManagerOpen))),
        "AEC_CONTROLPLANES" => Some(dispatch_control_planes(app, tab)),
        "AEC_WALL" => Some(dispatch_wall(app, tab, cmd)),
        "AEC_WALLJOIN" => Some(dispatch_walljoin(app, tab, cmd)),
        "AEC_WALLEXTEND" => Some(install_spawned(app, tab, cmd, "AEC_WALLEXTEND")),
        "AEC_WALLREVERSE" => Some(dispatch_wallreverse(app, tab, cmd)),
        "AEC_WINDOW" => Some(install_spawned(app, tab, cmd, "AEC_WINDOW")),
        "AEC_DOOR" => Some(install_spawned(app, tab, cmd, "AEC_DOOR")),
        "AEC_WALL_REFRESH" => {
            let style_library = crate::modules::aec::engine::project::resolve_style_library(
                app.aec.aec_project_explorer_file.as_ref(),
            );
            aec_wall_refresh(
                &mut app.tabs[tab].scene,
                &mut app.command_line,
                Some(&style_library),
            );
            Some(app.finish_dispatch(cmd))
        }
        "AEC_ROOM" => {
            aec_room(&mut app.tabs[tab].scene, &mut app.command_line);
            app.tabs[tab].dirty = true;
            Some(app.finish_dispatch(cmd))
        }
        "AEC_ROOMSCHEDULE" => {
            aec_room_schedule(&mut app.tabs[tab].scene, &mut app.command_line);
            app.tabs[tab].dirty = true;
            Some(app.finish_dispatch(cmd))
        }
        "AEC_IFCEXPORT" => {
            aec_ifc_export(&mut app.tabs[tab].scene, &mut app.command_line);
            Some(app.finish_dispatch(cmd))
        }
        cmd if cmd.starts_with("AEC_WALLOPENING_DO ") => {
            let args = cmd["AEC_WALLOPENING_DO ".len()..].to_string();
            let style_library = crate::modules::aec::engine::project::resolve_style_library(
                app.aec.aec_project_explorer_file.as_ref(),
            );
            let (display_rules, style_substitutions) =
                app.resolve_active_display_config_wall_rules(tab, None);
            aec_wallopening_do(
                &mut app.tabs[tab].scene,
                &mut app.command_line,
                &args,
                Some(&style_library),
                display_rules.as_ref(),
                style_substitutions.as_ref(),
            );
            app.tabs[tab].dirty = true;
            Some(app.finish_dispatch(cmd))
        }
        cmd if cmd.starts_with("AEC_WALLJOIN_DO ") => {
            let args = cmd["AEC_WALLJOIN_DO ".len()..].to_string();
            dispatch_walljoin_do(app, tab, cmd, &args)
        }
        cmd if cmd.starts_with("AEC_WALLEXTEND_DO ") => {
            let args = cmd["AEC_WALLEXTEND_DO ".len()..].to_string();
            dispatch_wallextend_do(app, tab, cmd, &args)
        }
        cmd if cmd.starts_with("AEC_WALLREVERSE_DO ") => {
            let args = cmd["AEC_WALLREVERSE_DO ".len()..].to_string();
            dispatch_wallreverse_do(app, tab, cmd, &args)
        }
        _ => None,
    }
}

fn dispatch_control_planes(app: &mut OpenCADStudio, tab: usize) -> Task<Message> {
    let visible = toggle_controlplanes_layer(&mut app.tabs[tab].scene);
    if visible {
        let path = app.tabs[tab].drawing_path().map(Path::to_path_buf);
        let storey_ids = app
            .aec
            .aec_project_explorer_file
            .as_ref()
            .and_then(|p| resolve_control_plane_storey_ids(p, path.as_deref()));
        if let Some((bid, sid)) = storey_ids {
            if let Some(project) = app.aec.aec_project_explorer_file.as_mut() {
                if let Some(storey) = project
                    .buildings
                    .iter_mut()
                    .find(|b| b.id == bid)
                    .and_then(|b| b.storeys.iter_mut().find(|s| s.id == sid))
                {
                    crate::modules::aec::project::drawing_sync::sync_storey_planes_from_drawing(
                        &app.tabs[tab].scene,
                        storey,
                    );
                    crate::modules::aec::project::preview::regenerate_control_plane_previews(
                        &mut app.tabs[tab].scene,
                        storey,
                    );
                }
            }
            app.aec_project_explorer_persist_if_pathed();
        }
        app.command_line
            .push_info(crate::t!("AEC_CONTROLPLANES: layer on, previews updated.").as_ref());
    } else {
        app.command_line
            .push_info(crate::t!("AEC_CONTROLPLANES: layer off.").as_ref());
    }
    app.tabs[tab].dirty = true;
    Task::none()
}

fn dispatch_wall(app: &mut OpenCADStudio, tab: usize, cmd: &str) -> Task<Message> {
    if !app.aec_require_project(Message::Command("AEC_WALL".to_string())) {
        // Returning `None` from try_dispatch would tell `dispatch_families`
        // that the verb was unmatched and can fall into autocomplete.
        return Task::none();
    }
    ensure_wall_app_id(&mut app.tabs[tab].scene.document);
    let style_library = crate::modules::aec::engine::project::resolve_style_library(
        app.aec.aec_project_explorer_file.as_ref(),
    );
    let mut new_cmd = WallCommand::new()
        .with_library(style_library)
        .with_session_defaults(
            app.aec.aec_last_wall_style_id.as_deref(),
            app.aec.aec_last_wall_height,
        );
    if let Some(storey) = app
        .aec
        .aec_project_explorer_file
        .as_ref()
        .and_then(|p| storey_matching_tab(p, app.tabs[tab].drawing_path()))
    {
        new_cmd = new_cmd.with_storey_planes(storey);
    }
    app.command_line.push_info(&new_cmd.prompt());
    app.tabs[tab].active_cmd = Some(Box::new(new_cmd));
    app.sync_wall_axis_layer_for_session(tab);
    app.refresh_properties();
    app.finish_dispatch(cmd)
}

fn dispatch_walljoin(app: &mut OpenCADStudio, tab: usize, cmd: &str) -> Task<Message> {
    // Context menu on a multi-wall selection never delivers a second pick.
    let wall_handles = selected_wall_handles(app, tab);
    if wall_handles.len() >= 2 {
        let style_library = crate::modules::aec::engine::project::resolve_style_library(
            app.aec.aec_project_explorer_file.as_ref(),
        );
        let (display_rules, style_substitutions) =
            app.resolve_active_display_config_wall_rules(tab, wall_handles.first().copied());
        aec_walljoin_do(
            &mut app.tabs[tab].scene,
            &mut app.command_line,
            &format!("{}|{}", wall_handles[0].value(), wall_handles[1].value()),
            Some(&style_library),
            display_rules.as_ref(),
            style_substitutions.as_ref(),
        );
        app.reapply_active_display_config_to_wall_packages(tab, &wall_handles);
        app.tabs[tab].dirty = true;
        return app.finish_dispatch(cmd);
    }
    install_spawned(app, tab, cmd, "AEC_WALLJOIN")
}

fn dispatch_wallreverse(app: &mut OpenCADStudio, tab: usize, cmd: &str) -> Task<Message> {
    let wall_handles = selected_wall_handles(app, tab);
    if !wall_handles.is_empty() {
        let style_library = crate::modules::aec::engine::project::resolve_style_library(
            app.aec.aec_project_explorer_file.as_ref(),
        );
        let (display_rules, style_substitutions) =
            app.resolve_active_display_config_wall_rules(tab, wall_handles.first().copied());
        for h in &wall_handles {
            aec_wallreverse_do(
                &mut app.tabs[tab].scene,
                &mut app.command_line,
                &h.value().to_string(),
                Some(&style_library),
                display_rules.as_ref(),
                style_substitutions.as_ref(),
            );
        }
        app.reapply_active_display_config_to_wall_packages(tab, &wall_handles);
        app.tabs[tab].dirty = true;
        return app.finish_dispatch(cmd);
    }
    install_spawned(app, tab, cmd, "AEC_WALLREVERSE")
}

fn dispatch_walljoin_do(
    app: &mut OpenCADStudio,
    tab: usize,
    cmd: &str,
    args: &str,
) -> Option<Task<Message>> {
    let style_library = crate::modules::aec::engine::project::resolve_style_library(
        app.aec.aec_project_explorer_file.as_ref(),
    );
    let join_handles: Vec<Handle> = args
        .split('|')
        .filter_map(|p| p.parse::<u64>().ok().map(Handle::new))
        .collect();
    let first = join_handles
        .first()
        .copied()
        .map(|h| resolve_wall_package(&app.tabs[tab].scene, h));
    let (display_rules, style_substitutions) =
        app.resolve_active_display_config_wall_rules(tab, first);
    aec_walljoin_do(
        &mut app.tabs[tab].scene,
        &mut app.command_line,
        args,
        Some(&style_library),
        display_rules.as_ref(),
        style_substitutions.as_ref(),
    );
    app.reapply_active_display_config_to_wall_packages(tab, &join_handles);
    app.tabs[tab].dirty = true;
    Some(app.finish_dispatch(cmd))
}

fn dispatch_wallextend_do(
    app: &mut OpenCADStudio,
    tab: usize,
    cmd: &str,
    args: &str,
) -> Option<Task<Message>> {
    let style_library = crate::modules::aec::engine::project::resolve_style_library(
        app.aec.aec_project_explorer_file.as_ref(),
    );
    let first = args
        .split('|')
        .next()
        .and_then(|p| p.parse::<u64>().ok())
        .map(Handle::new)
        .map(|h| resolve_wall_package(&app.tabs[tab].scene, h));
    let (display_rules, style_substitutions) =
        app.resolve_active_display_config_wall_rules(tab, first);
    aec_wallextend_do(
        &mut app.tabs[tab].scene,
        &mut app.command_line,
        args,
        Some(&style_library),
        display_rules.as_ref(),
        style_substitutions.as_ref(),
    );
    if let Some(h) = first {
        app.reapply_active_display_config_to_wall_packages(tab, &[h]);
    }
    app.tabs[tab].dirty = true;
    Some(app.finish_dispatch(cmd))
}

fn dispatch_wallreverse_do(
    app: &mut OpenCADStudio,
    tab: usize,
    cmd: &str,
    args: &str,
) -> Option<Task<Message>> {
    let style_library = crate::modules::aec::engine::project::resolve_style_library(
        app.aec.aec_project_explorer_file.as_ref(),
    );
    let first = args
        .parse::<u64>()
        .ok()
        .map(Handle::new)
        .or_else(|| {
            args.split('|')
                .next()
                .and_then(|p| p.parse::<u64>().ok())
                .map(Handle::new)
        })
        .map(|h| resolve_wall_package(&app.tabs[tab].scene, h));
    let (display_rules, style_substitutions) =
        app.resolve_active_display_config_wall_rules(tab, first);
    aec_wallreverse_do(
        &mut app.tabs[tab].scene,
        &mut app.command_line,
        args,
        Some(&style_library),
        display_rules.as_ref(),
        style_substitutions.as_ref(),
    );
    if let Some(h) = first {
        app.reapply_active_display_config_to_wall_packages(tab, &[h]);
    }
    app.tabs[tab].dirty = true;
    Some(app.finish_dispatch(cmd))
}

fn install_spawned(
    app: &mut OpenCADStudio,
    tab: usize,
    cmd: &str,
    spawn_name: &str,
) -> Task<Message> {
    let spawned = spawn_command(spawn_name)
        .unwrap_or_else(|| panic!("{spawn_name} is registered in aec::spawn_command"));
    app.command_line.push_info(&spawned.prompt());
    app.tabs[tab].active_cmd = Some(spawned);
    if matches!(spawn_name, "AEC_WALLJOIN" | "AEC_WALLEXTEND" | "AEC_WALL") {
        app.sync_wall_axis_layer_for_session(tab);
    }
    app.finish_dispatch(cmd)
}

fn selected_wall_handles(app: &OpenCADStudio, tab: usize) -> Vec<Handle> {
    let selected_handles: Vec<Handle> = app.tabs[tab]
        .scene
        .selected_entities()
        .into_iter()
        .map(|(h, _)| h)
        .collect();
    let mut wall_handles = Vec::new();
    for h in selected_handles {
        let resolved = resolve_wall_package(&app.tabs[tab].scene, h);
        if is_wall_pick_target(&app.tabs[tab].scene, resolved) && !wall_handles.contains(&resolved)
        {
            wall_handles.push(resolved);
        }
    }
    wall_handles
}

fn drawing_path_matches_storey(current: &Path, drawing_path: &str) -> bool {
    if drawing_path.trim().is_empty() {
        return false;
    }
    current.ends_with(drawing_path)
        || current.file_name().and_then(|n| n.to_str())
            == Path::new(drawing_path).file_name().and_then(|n| n.to_str())
}

fn storey_matching_tab<'a>(
    project: &'a ProjectFile,
    current_path: Option<&Path>,
) -> Option<&'a StoreyRef> {
    let current = current_path?;
    project.buildings.iter().find_map(|b| {
        b.storeys
            .iter()
            .find(|s| drawing_path_matches_storey(current, &s.drawing_path))
    })
}

fn resolve_control_plane_storey_ids(
    project: &ProjectFile,
    current_path: Option<&Path>,
) -> Option<(uuid::Uuid, uuid::Uuid)> {
    if let Some(storey) = storey_matching_tab(project, current_path) {
        let bid = project
            .buildings
            .iter()
            .find(|b| b.storeys.iter().any(|s| s.id == storey.id))?
            .id;
        return Some((bid, storey.id));
    }
    project
        .buildings
        .first()
        .and_then(|b| b.storeys.first().map(|s| (b.id, s.id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::project::Building;

    #[test]
    fn spawn_command_known_interactive_verbs() {
        for name in [
            "AEC_WALL",
            "AEC_WALLJOIN",
            "AEC_WALLEXTEND",
            "AEC_WALLREVERSE",
            "AEC_WINDOW",
            "AEC_DOOR",
        ] {
            let cmd = spawn_command(name).unwrap_or_else(|| panic!("missing spawn for {name}"));
            assert_eq!(cmd.name(), name);
        }
        assert!(spawn_command("AEC_ROOM").is_none());
        assert!(spawn_command("LINE").is_none());
    }

    #[test]
    fn drawing_path_matches_by_suffix_or_file_name() {
        let cur = Path::new("/proj/storeys/eg.dwg");
        assert!(drawing_path_matches_storey(cur, "storeys/eg.dwg"));
        assert!(drawing_path_matches_storey(cur, "eg.dwg"));
        assert!(!drawing_path_matches_storey(cur, "og.dwg"));
        assert!(!drawing_path_matches_storey(cur, "   "));
    }

    #[test]
    fn control_planes_prefer_path_then_first_storey() {
        let mut project = ProjectFile::default();
        let mut a = Building::new("A");
        a.storeys.push(StoreyRef::new("EG", 0.0, "other.dwg"));
        let mut b = Building::new("B");
        b.storeys.push(StoreyRef::new("OG", 3.0, "eg.dwg"));
        let expected = (b.id, b.storeys[0].id);
        let first = (a.id, a.storeys[0].id);
        project.buildings.push(a);
        project.buildings.push(b);

        assert_eq!(
            resolve_control_plane_storey_ids(&project, Some(Path::new("/tmp/eg.dwg"))),
            Some(expected)
        );
        assert_eq!(
            resolve_control_plane_storey_ids(&project, Some(Path::new("/tmp/missing.dwg"))),
            Some(first)
        );
        assert_eq!(
            resolve_control_plane_storey_ids(&project, None),
            Some(first)
        );
    }
}
