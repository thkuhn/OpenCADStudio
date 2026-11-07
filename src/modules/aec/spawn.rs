//! AEC command spawn / dispatch hooks used by core.

use std::path::Path;

use acadrust::Handle;
use iced::Task;
use uuid::Uuid;

use crate::app::{Message, OpenCADStudio};
use crate::command::CadCommand;
use crate::modules::aec::engine::join_ops::aec_walljoin_do;
use crate::modules::aec::engine::storey_xdata::{linked_storey, write_storey_link};
use crate::modules::aec::engine::xdata::ensure_wall_app_id;
use crate::modules::aec::engine::wall_package::{is_wall_pick_target, resolve_wall_package};
use crate::modules::aec::engine::wall_regen::toggle_controlplanes_layer;
use crate::modules::aec::ifc::export::aec_ifc_export;
use crate::modules::aec::walls::refresh::aec_wall_refresh;
use crate::modules::aec::walls::extend::aec_wallextend_do;
use crate::modules::aec::walls::window::aec_wallopening_do;
use crate::modules::aec::walls::reverse::aec_wallreverse_do;
use crate::modules::aec::walls::wall::WallCommand;
use crate::modules::aec::slabs::slab::{aec_slab_autodetect_do, aec_slab_convert_do, SlabCommand};
use crate::modules::aec::slabs::slab_opening::SlabOpeningCommand;
use crate::modules::aec::rooms::room::RoomCommand;
use crate::modules::aec::engine::project::{ProjectFile, StoreyRef};
use crate::modules::aec::message::AecMessage;
use crate::modules::aec::ui::style_manager::StylesNode;
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
        "AEC_OPENING" => Some(Box::new(WallOpeningCommand::new_opening())),
        "AEC_SLAB" => Some(Box::new(SlabCommand::new())),
        "AEC_SLABOPENING" => Some(Box::new(SlabOpeningCommand::new())),
        "AEC_ROOM" | "AEC_ROOM_PICK" => Some(Box::new(RoomCommand::new().with_mode(crate::modules::aec::rooms::room::RoomDrawMode::PickPoint))),
        "AEC_ROOM_RECT" => Some(Box::new(RoomCommand::new().with_mode(crate::modules::aec::rooms::room::RoomDrawMode::Rectangle))),
        "AEC_ROOM_POLY" => Some(Box::new(RoomCommand::new().with_mode(crate::modules::aec::rooms::room::RoomDrawMode::Polygon))),
        "AEC_ROOM_OBJECT" => Some(Box::new(RoomCommand::new().with_mode(crate::modules::aec::rooms::room::RoomDrawMode::SelectPolyline))),
        "AEC_ROOMSCHEDULE" => Some(Box::new(crate::modules::aec::rooms::schedule::RoomScheduleCommand::new())),
        "AEC_PLANE_3POINT" => Some(Box::new(crate::modules::aec::project::plane_3point::Plane3PointCommand::new())),
        "AEC_PLANE_FACET" => Some(Box::new(crate::modules::aec::project::plane_facet::PlaneFacetCommand::new())),
        "AEC_PLANE_ASSIGN" => Some(Box::new(crate::modules::aec::project::plane_assign::PlaneAssignCommand::new())),
        "AEC_SECTION" => Some(Box::new(crate::modules::aec::views::SectionCommand::new())),
        "AEC_ELEVATION" => Some(Box::new(crate::modules::aec::views::ElevationCommand::new())),
        "AEC_FLOORPLAN" => Some(Box::new(crate::modules::aec::views::FloorPlanCommand::new_floorplan())),
        "AEC_RCP" => Some(Box::new(crate::modules::aec::views::FloorPlanCommand::new_rcp())),
        "AEC_VIEWUPDATE" => Some(Box::new(crate::modules::aec::views::ViewUpdateCommand::new())),
        "AEC_PLACE_VIEW" => Some(Box::new(crate::modules::aec::views::PlaceViewCommand::new())),
        "AEC_TITLEBLOCK" => Some(Box::new(crate::modules::aec::views::TitleBlockCommand::new())),
        _ => None,
    }
}

fn view_display_context(
    app: &OpenCADStudio,
    tab: usize,
) -> (
    crate::modules::aec::engine::library::DisplayConfigLibrary,
    crate::modules::aec::engine::library::StyleLibrary,
    Option<String>,
) {
    let project = app.aec.aec_project_explorer_file.as_ref();
    (
        crate::modules::aec::engine::project::resolve_display_config_library(project),
        crate::modules::aec::engine::project::resolve_style_library(project),
        app.tabs[tab].active_display_config.clone(),
    )
}

fn active_plan_stage_value(app: &OpenCADStudio, tab: usize) -> Option<String> {
    let library = crate::modules::aec::engine::project::resolve_display_config_library(
        app.aec.aec_project_explorer_file.as_ref(),
    );
    crate::modules::aec::views::title_block::active_stage_value(
        &library,
        app.tabs.get(tab)?.active_display_config.as_deref(),
    )
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
        "AEC_STYLES" => Some(Task::done(Message::Aec(AecMessage::AecStylesOpen(
            Default::default(),
        )))),
        "AEC_MATERIALMANAGER" => Some(Task::done(Message::Aec(AecMessage::AecStylesOpen(
            StylesNode::Materials,
        )))),
        "AEC_PROJECTEXPLORER" => Some(Task::done(Message::Aec(AecMessage::AecProjectExplorerOpen))),
        "AEC_STOREY" => Some(dispatch_storey(app, tab, cmd)),
        "AEC_STYLEMANAGER" => Some(Task::done(Message::Aec(AecMessage::AecStylesOpen(
            StylesNode::Wall,
        )))),
        "AEC_OPENINGSTYLEMANAGER" => Some(Task::done(Message::Aec(AecMessage::AecStylesOpen(
            StylesNode::Opening,
        )))),
        "AEC_SLABSTYLEMANAGER" => Some(Task::done(Message::Aec(AecMessage::AecStylesOpen(
            StylesNode::Slab,
        )))),
        "AEC_SLABSTRUCTURALSTYLEMANAGER" => Some(Task::done(Message::Aec(
            AecMessage::AecStylesOpen(StylesNode::SlabStructural),
        ))),
        "AEC_FLOORFINISHSTYLEMANAGER" => Some(Task::done(Message::Aec(
            AecMessage::AecStylesOpen(StylesNode::FloorFinish),
        ))),
        "AEC_PLANMANAGER" => Some(Task::done(Message::Aec(AecMessage::AecPlanManagerOpen))),
        "AEC_CONTROLPLANES" => Some(dispatch_control_planes(app, tab)),
        "AEC_WALL" => Some(dispatch_wall(app, tab, cmd)),
        "AEC_WALLJOIN" => Some(dispatch_walljoin(app, tab, cmd)),
        "AEC_WALLEXTEND" => Some(install_spawned(app, tab, cmd, "AEC_WALLEXTEND")),
        "AEC_WALLREVERSE" => Some(dispatch_wallreverse(app, tab, cmd)),
        "AEC_WINDOW" => Some(install_spawned(app, tab, cmd, "AEC_WINDOW")),
        "AEC_DOOR" => Some(install_spawned(app, tab, cmd, "AEC_DOOR")),
        "AEC_OPENING" => Some(install_spawned(app, tab, cmd, "AEC_OPENING")),
        "AEC_SLAB" => Some(dispatch_slab(app, tab, cmd)),
        "AEC_SLABOPENING" => Some(install_spawned(app, tab, cmd, "AEC_SLABOPENING")),
        "AEC_PLANE_3POINT" => Some(install_spawned(app, tab, cmd, "AEC_PLANE_3POINT")),
        "AEC_PLANE_FACET" => Some(install_spawned(app, tab, cmd, "AEC_PLANE_FACET")),
        "AEC_PLANE_ASSIGN" => Some(install_spawned(app, tab, cmd, "AEC_PLANE_ASSIGN")),
        "AEC_SECTION" => Some(dispatch_section(app, tab, cmd)),
        "AEC_ELEVATION" => Some(dispatch_elevation(app, tab, cmd)),
        "AEC_FLOORPLAN" => Some(install_spawned(app, tab, cmd, "AEC_FLOORPLAN")),
        "AEC_RCP" => Some(install_spawned(app, tab, cmd, "AEC_RCP")),
        "AEC_VIEWUPDATE" | "AEC_VIEWUPDATE_DO" => Some(dispatch_viewupdate(app, tab, cmd)),
        cmd if cmd.starts_with("AEC_VIEWUPDATE_DO") => Some(dispatch_viewupdate(app, tab, cmd)),
        "AEC_PLACE_VIEW" => Some(install_spawned(app, tab, cmd, "AEC_PLACE_VIEW")),
        "AEC_TITLEBLOCK" => Some(install_spawned(app, tab, cmd, "AEC_TITLEBLOCK")),
        cmd if cmd.starts_with("AEC_TITLEBLOCK_DO ") => {
            let args = cmd["AEC_TITLEBLOCK_DO ".len()..].to_string();
            let mut values = args.split('|');
            if let (Some(x), Some(y)) = (values.next(), values.next()) {
                if let (Ok(x), Ok(y)) = (x.parse::<f64>(), y.parse::<f64>()) {
                    let active_stage = active_plan_stage_value(app, tab);
                    let handle = crate::modules::aec::views::title_block::aec_title_block_at_point(
                        &mut app.tabs[tab].scene,
                        acadrust::types::Vector3::new(x, y, 0.0),
                        active_stage.as_deref(),
                    );
                    app.command_line.push_info(&crate::tr!(
                        "aec",
                        "title-block-created",
                        handle = handle.to_string()
                    ));
                    app.tabs[tab].dirty = true;
                }
            }
            Some(app.finish_dispatch(cmd))
        }
        "AEC_PLANE_SYNC" => {
            let path = app.tabs[tab].drawing_path().map(Path::to_path_buf);
            let base_dir = app
                .aec
                .aec_project_explorer_path
                .as_deref()
                .and_then(|p| p.parent());
            let storey_ids = app
                .aec
                .aec_project_explorer_file
                .as_ref()
                .and_then(|p| resolve_control_plane_storey_ids(p, path.as_deref(), base_dir));
            if let Some((bid, sid)) = storey_ids {
                app.sync_storey_from_active_drawing(bid, sid);
                app.command_line.push_info(
                    crate::t!("AEC: Kontrollebenen aus der Zeichnung synchronisiert.").as_ref(),
                );
            } else {
                app.command_line.push_info(
                    crate::t!("AEC: Keine Geschossbindung für die aktuelle Zeichnung gefunden.").as_ref(),
                );
            }
            Some(app.finish_dispatch(cmd))
        }
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
        "AEC_ROOM" | "AEC_ROOM_PICK" => Some(dispatch_room(app, tab, cmd, crate::modules::aec::rooms::room::RoomDrawMode::PickPoint)),
        "AEC_ROOM_RECT" => Some(dispatch_room(app, tab, cmd, crate::modules::aec::rooms::room::RoomDrawMode::Rectangle)),
        "AEC_ROOM_POLY" => Some(dispatch_room(app, tab, cmd, crate::modules::aec::rooms::room::RoomDrawMode::Polygon)),
        "AEC_ROOM_OBJECT" => Some(dispatch_room(app, tab, cmd, crate::modules::aec::rooms::room::RoomDrawMode::SelectPolyline)),
        "AEC_ROOMSCHEDULE" => Some(install_spawned(app, tab, cmd, "AEC_ROOMSCHEDULE")),
        cmd if cmd.starts_with("AEC_ROOMSCHEDULE_DO ") => {
            let args = cmd["AEC_ROOMSCHEDULE_DO ".len()..].to_string();
            let mut it = args.split('|');
            let (x, y) = if let (Some(x_str), Some(y_str)) = (it.next(), it.next()) {
                (x_str.parse::<f64>().unwrap_or(0.0), y_str.parse::<f64>().unwrap_or(0.0))
            } else {
                (0.0, 0.0)
            };
            let at = acadrust::types::Vector3::new(x, y, 0.0);
            let active_stage = active_plan_stage_value(app, tab);
            if let Some(handle) = crate::modules::aec::rooms::schedule::aec_room_schedule_at_point_with_stage(
                &mut app.tabs[tab].scene,
                at,
                active_stage.as_deref(),
            ) {
                let count = crate::modules::aec::rooms::schedule::collect_all_rooms(&app.tabs[tab].scene).len();
                app.command_line.push_info(&crate::tr!(
                    "aec",
                    "schedule-created",
                    count = count,
                    handle = handle.to_string()
                ));
                app.tabs[tab].dirty = true;
            } else {
                app.command_line.push_info(&crate::tr!("aec", "no-rooms"));
            }
            Some(app.finish_dispatch(cmd))
        }
        "AEC_IFCEXPORT" => {
            if let Some(storey) = project_storey_for_tab(app, tab) {
                let _ = sync_project_storey_link(app, tab, &storey);
            }
            aec_ifc_export(&mut app.tabs[tab].scene, &mut app.command_line);
            Some(app.finish_dispatch(cmd))
        }
        cmd if cmd.starts_with("AEC_ROOM_POLYGON_DO ") => {
            let args = cmd["AEC_ROOM_POLYGON_DO ".len()..].to_string();
            let mut cmd_obj = RoomCommand::new().with_display(app.resolve_active_display_config_room_display(tab));
            let pts: Vec<(f64, f64)> = args
                .split(';')
                .filter_map(|pair| {
                    let mut it = pair.split(',');
                    let x = it.next()?.parse::<f64>().ok()?;
                    let y = it.next()?.parse::<f64>().ok()?;
                    Some((x, y))
                })
                .collect();
            if pts.len() >= 3 {
                match cmd_obj.commit_polygon(&mut app.tabs[tab].scene, &pts) {
                    Ok(handle) => {
                        app.command_line.push_info(crate::tf!("AEC: Raum erzeugt ({handle}).").as_ref());
                        app.tabs[tab].dirty = true;
                    }
                    Err(err) => {
                        app.command_line.push_error(&format!("AEC_ROOM: {err}"));
                    }
                }
            }
            Some(app.finish_dispatch(cmd))
        }
        cmd if cmd.starts_with("AEC_ROOM_PICK_DO ") => {
            let args = cmd["AEC_ROOM_PICK_DO ".len()..].to_string();
            let mut cmd_obj = RoomCommand::new().with_display(app.resolve_active_display_config_room_display(tab));
            let mut it = args.split('|');
            if let (Some(x_str), Some(y_str)) = (it.next(), it.next()) {
                if let (Ok(x), Ok(y)) = (x_str.parse::<f64>(), y_str.parse::<f64>()) {
                    match cmd_obj.auto_detect_at_point(&mut app.tabs[tab].scene, (x, y)) {
                        Ok(handle) => {
                            app.command_line.push_info(
                                crate::tf!(
                                    "AEC: Raum an ({:.2}, {:.2}) erzeugt ({handle}).",
                                    x, y
                                )
                                .as_ref(),
                            );
                            app.tabs[tab].dirty = true;
                        }
                        Err(err) => {
                            app.command_line.push_error(&format!("AEC_ROOM: {err}"));
                        }
                    }
                }
            }
            Some(app.finish_dispatch(cmd))
        }
        cmd if cmd.starts_with("AEC_ROOM_CONVERT_DO ") => {
            let args = cmd["AEC_ROOM_CONVERT_DO ".len()..].to_string();
            if let Ok(raw_handle) = args.trim().parse::<u64>() {
                let handle = Handle::new(raw_handle);
                let mut cmd_obj = RoomCommand::new().with_display(app.resolve_active_display_config_room_display(tab));
                match cmd_obj.convert_selected_polyline(&mut app.tabs[tab].scene, handle) {
                    Ok(h) => {
                        app.command_line.push_info(crate::tf!("AEC: Polylinie in Raum konvertiert ({h}).").as_ref());
                        app.tabs[tab].dirty = true;
                    }
                    Err(err) => {
                        app.command_line.push_error(&format!("AEC_ROOM: {err}"));
                    }
                }
            }
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
        cmd if cmd.starts_with("AEC_SLAB_CONVERT_DO ") => {
            let args = cmd["AEC_SLAB_CONVERT_DO ".len()..].to_string();
            let style_library = crate::modules::aec::engine::project::resolve_style_library(
                app.aec.aec_project_explorer_file.as_ref(),
            );
            aec_slab_convert_do(
                &mut app.tabs[tab].scene,
                &mut app.command_line,
                &args,
                Some(&style_library),
            );
            app.tabs[tab].dirty = true;
            Some(app.finish_dispatch(cmd))
        }
        cmd if cmd.starts_with("AEC_SLAB_AUTODETECT_DO ") => {
            let args = cmd["AEC_SLAB_AUTODETECT_DO ".len()..].to_string();
            let style_library = crate::modules::aec::engine::project::resolve_style_library(
                app.aec.aec_project_explorer_file.as_ref(),
            );
            aec_slab_autodetect_do(
                &mut app.tabs[tab].scene,
                &mut app.command_line,
                &args,
                Some(&style_library),
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
        cmd if cmd.starts_with("AEC_PLANE_3POINT_DO ") => {
            let args = cmd["AEC_PLANE_3POINT_DO ".len()..].to_string();
            let path = app.tabs[tab].drawing_path().map(Path::to_path_buf);
            let base_dir = app
                .aec
                .aec_project_explorer_path
                .as_deref()
                .and_then(|p| p.parent());
            let storey_ids = app
                .aec
                .aec_project_explorer_file
                .as_ref()
                .and_then(|p| resolve_control_plane_storey_ids(p, path.as_deref(), base_dir));
            let mut storey_opt = None;
            if let Some((bid, sid)) = storey_ids {
                if let Some(project) = app.aec.aec_project_explorer_file.as_mut() {
                    storey_opt = project
                        .buildings
                        .iter_mut()
                        .find(|b| b.id == bid)
                        .and_then(|b| b.storeys.iter_mut().find(|s| s.id == sid));
                }
            }
            crate::modules::aec::project::plane_3point::aec_plane_3point_do(
                &mut app.tabs[tab].scene,
                &mut app.command_line,
                storey_opt,
                &args,
            );
            app.aec_project_explorer_persist_if_pathed();
            app.tabs[tab].dirty = true;
            Some(app.finish_dispatch(cmd))
        }
        cmd if cmd.starts_with("AEC_SECTION_DO ") => {
            let args = cmd["AEC_SECTION_DO ".len()..].to_string();
            let (display_config_library, style_library, active_name) = view_display_context(app, tab);
            crate::modules::aec::views::aec_section_do(
                &mut app.tabs[tab].scene,
                &mut app.command_line,
                &args,
                &display_config_library,
                &style_library,
                active_name.as_deref(),
            );
            app.tabs[tab].dirty = true;
            Some(app.finish_dispatch(cmd))
        }
        cmd if cmd.starts_with("AEC_ELEVATION_DO ") => {
            let args = cmd["AEC_ELEVATION_DO ".len()..].to_string();
            let (display_config_library, style_library, active_name) = view_display_context(app, tab);
            crate::modules::aec::views::aec_elevation_do(
                &mut app.tabs[tab].scene,
                &mut app.command_line,
                &args,
                &display_config_library,
                &style_library,
                active_name.as_deref(),
            );
            app.tabs[tab].dirty = true;
            Some(app.finish_dispatch(cmd))
        }
        cmd if cmd.starts_with("AEC_FLOORPLAN_DO ") => {
            let args = cmd["AEC_FLOORPLAN_DO ".len()..].to_string();
            let (display_config_library, style_library, active_name) = view_display_context(app, tab);
            crate::modules::aec::views::aec_floorplan_do(
                &mut app.tabs[tab].scene,
                &mut app.command_line,
                &args,
                &display_config_library,
                &style_library,
                active_name.as_deref(),
            );
            app.tabs[tab].dirty = true;
            Some(app.finish_dispatch(cmd))
        }
        cmd if cmd == "AEC_PLANE_FACET_DO" || cmd.starts_with("AEC_PLANE_FACET_DO ") => {
            let (_outcome, task) = dispatch_plane_facet_do_helper(app, tab, cmd);
            Some(task)
        }
        cmd if cmd == "AEC_PLANE_ASSIGN_DO" || cmd.starts_with("AEC_PLANE_ASSIGN_DO ") => {
            let (_outcome, task) = dispatch_plane_assign_do_helper(app, tab, cmd);
            Some(task)
        }
        _ => None,
    }
}

fn dispatch_control_planes(app: &mut OpenCADStudio, tab: usize) -> Task<Message> {
    let visible = toggle_controlplanes_layer(&mut app.tabs[tab].scene);
    if visible {
        let path = app.tabs[tab].drawing_path().map(Path::to_path_buf);
        let base_dir = app
            .aec
            .aec_project_explorer_path
            .as_deref()
            .and_then(|p| p.parent());
        let storey_ids = app
            .aec
            .aec_project_explorer_file
            .as_ref()
            .and_then(|p| resolve_control_plane_storey_ids(p, path.as_deref(), base_dir));
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
    let base_dir = app
        .aec
        .aec_project_explorer_path
        .as_deref()
        .and_then(|p| p.parent());
    if let Some(storey) = app
        .aec
        .aec_project_explorer_file
        .as_ref()
        .and_then(|p| {
            storey_matching_tab(p, app.tabs[tab].drawing_path(), base_dir)
                .or_else(|| {
                    app.aec
                        .aec_project_explorer_selected_storey
                        .and_then(|(bid, sid)| {
                            p.buildings
                                .iter()
                                .find(|b| b.id == bid)
                                .and_then(|b| b.storeys.iter().find(|s| s.id == sid))
                        })
                })
                .or_else(|| p.buildings.first().and_then(|b| b.storeys.first()))
        })
    {
        new_cmd = new_cmd.with_storey_planes(storey);
    }
    app.command_line.push_info(&new_cmd.prompt());
    app.tabs[tab].active_cmd = Some(Box::new(new_cmd));
    app.sync_wall_axis_layer_for_session(tab);
    app.refresh_properties();
    app.finish_dispatch(cmd)
}

fn dispatch_slab(app: &mut OpenCADStudio, tab: usize, cmd: &str) -> Task<Message> {
    let style_library = crate::modules::aec::engine::project::resolve_style_library(
        app.aec.aec_project_explorer_file.as_ref(),
    );
    let new_cmd = SlabCommand::new().with_library(style_library);
    app.command_line.push_info(&new_cmd.prompt());
    app.tabs[tab].active_cmd = Some(Box::new(new_cmd));
    app.refresh_properties();
    app.finish_dispatch(cmd)
}

fn dispatch_section(app: &mut OpenCADStudio, tab: usize, cmd: &str) -> Task<Message> {
    let new_cmd = crate::modules::aec::views::SectionCommand::new_with_project(
        app.aec.aec_project_explorer_file.as_ref(),
    );
    app.command_line.push_info(&new_cmd.prompt());
    app.tabs[tab].active_cmd = Some(Box::new(new_cmd));
    app.refresh_properties();
    app.finish_dispatch(cmd)
}

fn dispatch_elevation(app: &mut OpenCADStudio, tab: usize, cmd: &str) -> Task<Message> {
    let new_cmd = crate::modules::aec::views::ElevationCommand::new_with_project(
        app.aec.aec_project_explorer_file.as_ref(),
    );
    app.command_line.push_info(&new_cmd.prompt());
    app.tabs[tab].active_cmd = Some(Box::new(new_cmd));
    app.refresh_properties();
    app.finish_dispatch(cmd)
}

fn dispatch_room(
    app: &mut OpenCADStudio,
    tab: usize,
    cmd: &str,
    mode: crate::modules::aec::rooms::room::RoomDrawMode,
) -> Task<Message> {
    let style_library = crate::modules::aec::engine::project::resolve_style_library(
        app.aec.aec_project_explorer_file.as_ref(),
    );
    let new_cmd = RoomCommand::new_with_library(Some(style_library))
        .with_display(app.resolve_active_display_config_room_display(tab))
        .with_mode(mode);
    app.command_line.push_info(&new_cmd.prompt());
    app.tabs[tab].active_cmd = Some(Box::new(new_cmd));
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

fn dispatch_viewupdate(app: &mut OpenCADStudio, tab: usize, cmd: &str) -> Task<Message> {
    let mut cmd_obj = crate::modules::aec::views::ViewUpdateCommand::new();
    cmd_obj.force_all = true;
    let (display_config_library, style_library, active_name) = view_display_context(app, tab);
    let count = cmd_obj.update_all(
        &mut app.tabs[tab].scene,
        &display_config_library,
        &style_library,
        active_name.as_deref(),
    );
    app.command_line.push_info(&format!(
        "AEC: {} Ansicht(en) / Schnitte erfolgreich aktualisiert.",
        count
    ));
    app.tabs[tab].dirty = true;
    app.tabs[tab].scene.bump_geometry();
    app.post_ref_op(tab);
    app.finish_dispatch(cmd)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlaneTargetDispatchOutcome {
    ReopenStoreySettings(Uuid, Uuid),
    Finished,
    Error(String),
}

pub(crate) fn dispatch_plane_facet_do_helper(
    app: &mut OpenCADStudio,
    tab: usize,
    cmd: &str,
) -> (PlaneTargetDispatchOutcome, Task<Message>) {
    let rest = if cmd.starts_with("AEC_PLANE_FACET_DO ") {
        cmd["AEC_PLANE_FACET_DO ".len()..].trim()
    } else {
        ""
    };
    let mut parts = rest.split_whitespace();
    let first_token = parts.clone().next().unwrap_or("");
    let mut target_plane_id = None;
    let mut target_bid_sid = None;
    let coord_args: String;

    if first_token.contains('|') {
        match crate::modules::aec::project::plane_facet::parse_plane_target(first_token) {
            Ok(Some((bid, sid, pid))) => {
                target_bid_sid = Some((bid, sid));
                target_plane_id = Some(pid);
                let _ = parts.next();
                coord_args = parts.collect::<Vec<_>>().join(" ");
            }
            Ok(None) => {
                let _ = parts.next();
                coord_args = parts.collect::<Vec<_>>().join(" ");
            }
            Err(e) => {
                let msg = format!("AEC: {e}");
                app.command_line.push_error(&msg);
                return (PlaneTargetDispatchOutcome::Error(msg), app.finish_dispatch(cmd));
            }
        }
    } else {
        coord_args = rest.to_string();
    }

    let path = app.tabs[tab].drawing_path().map(Path::to_path_buf);
    let base_dir = app
        .aec
        .aec_project_explorer_path
        .as_deref()
        .and_then(|p| p.parent());
    let storey_ids = target_bid_sid.or_else(|| {
        app.aec
            .aec_project_explorer_file
            .as_ref()
            .and_then(|p| resolve_control_plane_storey_ids(p, path.as_deref(), base_dir))
    });

    let Some((bid, sid)) = storey_ids else {
        let msg = crate::t!("AEC: Kein Geschoss im Projekt gefunden.").into_owned();
        app.command_line.push_error(&msg);
        return (PlaneTargetDispatchOutcome::Error(msg), app.finish_dispatch(cmd));
    };

    let Some(project) = app.aec.aec_project_explorer_file.as_mut() else {
        let msg = crate::t!("AEC: Kein AEC-Projekt geladen.").into_owned();
        app.command_line.push_error(&msg);
        return (PlaneTargetDispatchOutcome::Error(msg), app.finish_dispatch(cmd));
    };

    let Some(bldg) = project.buildings.iter_mut().find(|b| b.id == bid) else {
        let msg = crate::t!("AEC: Zielgebäude im Projekt nicht gefunden.").into_owned();
        app.command_line.push_error(&msg);
        return (PlaneTargetDispatchOutcome::Error(msg), app.finish_dispatch(cmd));
    };

    let Some(storey) = bldg.storeys.iter_mut().find(|s| s.id == sid) else {
        let msg = crate::t!("AEC: Zielgeschoss im Projekt nicht gefunden.").into_owned();
        app.command_line.push_error(&msg);
        return (PlaneTargetDispatchOutcome::Error(msg), app.finish_dispatch(cmd));
    };

    if !coord_args.trim().is_empty() {
        let matches_drawing = path
            .as_deref()
            .map(|p| drawing_path_matches_storey(p, &storey.drawing_path, base_dir))
            .unwrap_or(false);

        let preview_scene = if matches_drawing {
            Some(&mut app.tabs[tab].scene)
        } else {
            None
        };

        let facet_opt = crate::modules::aec::project::plane_facet::aec_plane_facet_do(
            preview_scene,
            &mut app.command_line,
            Some(storey),
            target_plane_id,
            &coord_args,
        );

        if facet_opt.is_some() {
            app.aec_project_explorer_persist_if_pathed();
            if matches_drawing {
                app.tabs[tab].dirty = true;
            }
        }
    }

    if let Some((bid, sid)) = target_bid_sid {
        return (
            PlaneTargetDispatchOutcome::ReopenStoreySettings(bid, sid),
            Task::done(crate::app::Message::Aec(
                crate::modules::aec::message::AecMessage::AecStoreySettingsOpen(bid, sid),
            )),
        );
    }
    (PlaneTargetDispatchOutcome::Finished, app.finish_dispatch(cmd))
}

pub(crate) fn dispatch_plane_assign_do_helper(
    app: &mut OpenCADStudio,
    tab: usize,
    cmd: &str,
) -> (PlaneTargetDispatchOutcome, Task<Message>) {
    let rest = if cmd.starts_with("AEC_PLANE_ASSIGN_DO ") {
        cmd["AEC_PLANE_ASSIGN_DO ".len()..].trim()
    } else {
        ""
    };
    let mut parts = rest.split_whitespace();
    let target_str = parts.clone().next().unwrap_or("_|_|_");
    let mut target_plane_id = None;
    let mut target_bid_sid = None;
    if target_str.contains('|') {
        match crate::modules::aec::project::plane_facet::parse_plane_target(target_str) {
            Ok(Some((bid, sid, pid))) => {
                target_bid_sid = Some((bid, sid));
                target_plane_id = Some(pid);
                let _ = parts.next();
            }
            Ok(None) => {
                let _ = parts.next();
            }
            Err(e) => {
                let msg = format!("AEC: {e}");
                app.command_line.push_error(&msg);
                return (PlaneTargetDispatchOutcome::Error(msg), app.finish_dispatch(cmd));
            }
        }
    }
    let handles: Vec<Handle> = parts
        .filter_map(|s| s.parse::<u64>().ok())
        .map(Handle::new)
        .collect();

    let path = app.tabs[tab].drawing_path().map(Path::to_path_buf);
    let base_dir = app
        .aec
        .aec_project_explorer_path
        .as_deref()
        .and_then(|p| p.parent());
    let storey_ids = target_bid_sid.or_else(|| {
        app.aec
            .aec_project_explorer_file
            .as_ref()
            .and_then(|p| resolve_control_plane_storey_ids(p, path.as_deref(), base_dir))
    });

    let Some((bid, sid)) = storey_ids else {
        let msg = crate::t!("AEC: Kein Geschoss im Projekt gefunden.").into_owned();
        app.command_line.push_error(&msg);
        return (PlaneTargetDispatchOutcome::Error(msg), app.finish_dispatch(cmd));
    };

    let Some(project) = app.aec.aec_project_explorer_file.as_mut() else {
        let msg = crate::t!("AEC: Kein AEC-Projekt geladen.").into_owned();
        app.command_line.push_error(&msg);
        return (PlaneTargetDispatchOutcome::Error(msg), app.finish_dispatch(cmd));
    };

    let Some(bldg) = project.buildings.iter_mut().find(|b| b.id == bid) else {
        let msg = crate::t!("AEC: Zielgebäude im Projekt nicht gefunden.").into_owned();
        app.command_line.push_error(&msg);
        return (PlaneTargetDispatchOutcome::Error(msg), app.finish_dispatch(cmd));
    };

    let Some(storey) = bldg.storeys.iter_mut().find(|s| s.id == sid) else {
        let msg = crate::t!("AEC: Zielgeschoss im Projekt nicht gefunden.").into_owned();
        app.command_line.push_error(&msg);
        return (PlaneTargetDispatchOutcome::Error(msg), app.finish_dispatch(cmd));
    };

    if !handles.is_empty() {
        let matches_drawing = path
            .as_deref()
            .map(|p| drawing_path_matches_storey(p, &storey.drawing_path, base_dir))
            .unwrap_or(false);

        let count = crate::modules::aec::project::plane_assign::aec_plane_assign_do(
            &mut app.tabs[tab].scene,
            &mut app.command_line,
            Some(storey),
            target_plane_id,
            &handles,
            matches_drawing,
        );

        if count > 0 {
            app.aec_project_explorer_persist_if_pathed();
            if matches_drawing {
                app.tabs[tab].dirty = true;
            }
        }
    }

    if let Some((bid, sid)) = target_bid_sid {
        return (
            PlaneTargetDispatchOutcome::ReopenStoreySettings(bid, sid),
            Task::done(crate::app::Message::Aec(
                crate::modules::aec::message::AecMessage::AecStoreySettingsOpen(bid, sid),
            )),
        );
    }
    (PlaneTargetDispatchOutcome::Finished, app.finish_dispatch(cmd))
}

fn drawing_path_matches_storey(
    current: &Path,
    drawing_path: &str,
    base_dir: Option<&Path>,
) -> bool {
    crate::modules::aec::engine::project::storey_matches_drawing_path(
        drawing_path,
        base_dir,
        current,
    )
}

fn storey_matching_tab<'a>(
    project: &'a ProjectFile,
    current_path: Option<&Path>,
    base_dir: Option<&Path>,
) -> Option<&'a StoreyRef> {
    let current = current_path?;
    project
        .find_storey_by_drawing_path_with_base(base_dir, current)
        .map(|(_, s)| s)
}

fn project_storey_for_tab(app: &OpenCADStudio, tab: usize) -> Option<StoreyRef> {
    let drawing_path = app.tabs.get(tab)?.drawing_path()?;
    let base_dir = app
        .aec
        .aec_project_explorer_path
        .as_deref()
        .and_then(Path::parent);
    app.aec
        .aec_project_explorer_file
        .as_ref()
        .and_then(|project| storey_matching_tab(project, Some(drawing_path), base_dir))
        .cloned()
}

fn sync_project_storey_link(
    app: &mut OpenCADStudio,
    tab: usize,
    storey: &StoreyRef,
) -> Option<Handle> {
    let cached_storey = crate::modules::aec::engine::Storey::new(
        storey.name.clone(),
        storey.derived_elevation(),
        storey.derived_height(),
    );
    let link_is_current = linked_storey(&app.tabs.get(tab)?.scene.document)
        .is_some_and(|(_, cached, id)| id == storey.id && cached == cached_storey);
    let handle = write_storey_link(&mut app.tabs.get_mut(tab)?.scene, storey);
    if !link_is_current {
        app.tabs.get_mut(tab)?.dirty = true;
    }
    Some(handle)
}

fn dispatch_storey(app: &mut OpenCADStudio, tab: usize, cmd: &str) -> Task<Message> {
    if let Some(storey) = project_storey_for_tab(app, tab) {
        if let Some(handle) = sync_project_storey_link(app, tab, &storey) {
            app.command_line.push_info(
                &crate::tr!(
                    "aec",
                    "storey-link-synced",
                    name = storey.name.as_str(),
                    handle = handle.to_string()
                ),
            );
        } else {
            app.command_line
                .push_info(&crate::tr!("aec", "storey-defined-in-project"));
        }
    } else {
        app.command_line
            .push_info(&crate::tr!("aec", "storey-defined-in-project"));
    }
    app.finish_dispatch(cmd)
}

fn resolve_control_plane_storey_ids(
    project: &ProjectFile,
    current_path: Option<&Path>,
    base_dir: Option<&Path>,
) -> Option<(uuid::Uuid, uuid::Uuid)> {
    if let Some(storey) = storey_matching_tab(project, current_path, base_dir) {
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
    use std::path::PathBuf;
    use acadrust::types::Vector3;
    use acadrust::entities::EntityType;

    struct TempProjectRoot {
        path: PathBuf,
    }

    impl TempProjectRoot {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "opencadstudio_aec_spawn_{name}_{}",
                Uuid::new_v4()
            ));
            std::fs::create_dir_all(&path).expect("create temporary AEC project directory");
            Self { path }
        }

        fn project_path(&self) -> PathBuf {
            self.path.join("project.ocsproj")
        }
    }

    impl Drop for TempProjectRoot {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.path).expect("remove temporary AEC project directory");
        }
    }

    fn load_persisted_project(path: &std::path::Path) -> ProjectFile {
        ProjectFile::load(path).expect("project should have been saved")
    }

    fn persisted_storey(project: &ProjectFile, bid: Uuid, sid: Uuid) -> &StoreyRef {
        project
            .buildings
            .iter()
            .find(|building| building.id == bid)
            .and_then(|building| building.storeys.iter().find(|storey| storey.id == sid))
            .expect("target storey should be persisted")
    }

    #[test]
    fn spawn_command_known_interactive_verbs() {
        for name in [
            "AEC_WALL",
            "AEC_WALLJOIN",
            "AEC_WALLEXTEND",
            "AEC_WALLREVERSE",
            "AEC_WINDOW",
            "AEC_DOOR",
            "AEC_OPENING",
            "AEC_SLAB",
            "AEC_SLABOPENING",
            "AEC_ROOM",
            "AEC_ROOMSCHEDULE",
        ] {
            let cmd = spawn_command(name).unwrap_or_else(|| panic!("missing spawn for {name}"));
            assert_eq!(cmd.name(), name);
        }
        for sub_name in [
            "AEC_ROOM_PICK",
            "AEC_ROOM_RECT",
            "AEC_ROOM_POLY",
            "AEC_ROOM_OBJECT",
        ] {
            let cmd = spawn_command(sub_name).unwrap_or_else(|| panic!("missing spawn for {sub_name}"));
            assert_eq!(cmd.name(), "AEC_ROOM");
        }
        assert!(spawn_command("AEC_WALL_REFRESH").is_none());
        assert!(spawn_command("LINE").is_none());
    }

    #[test]
    fn aec_storey_links_the_matching_project_storey() {
        let temp_project = TempProjectRoot::new("storey_link");
        let drawing_path = temp_project.path.join("EG.dwg");
        std::fs::write(&drawing_path, b"drawing").expect("write drawing");

        let mut app = OpenCADStudio::new_for_test();
        let tab = app.active_tab;
        app.tabs[tab].set_drawing_path_for_test(drawing_path);
        app.aec.aec_project_explorer_path = Some(temp_project.project_path());

        let mut project = ProjectFile::default();
        let mut building = Building::new("House");
        let storey = StoreyRef::new_with_height("EG", 4.25, 2.85, "EG.dwg");
        let storey_id = storey.id;
        building.storeys.push(storey);
        project.buildings.push(building);
        app.aec.aec_project_explorer_file = Some(project);

        assert!(try_dispatch(&mut app, "AEC_STOREY", tab).is_some());

        let linked = linked_storey(&app.tabs[tab].scene.document).unwrap();
        assert_eq!(linked.2, storey_id);
        assert_eq!(linked.1.name, "EG");
        assert!(app.tabs[tab].dirty);
        assert_eq!(app.tabs[tab].scene.document.entities().count(), 1);
    }

    #[test]
    fn aec_storey_without_project_binding_does_not_create_a_level() {
        let mut app = OpenCADStudio::new_for_test();
        let tab = app.active_tab;

        assert!(try_dispatch(&mut app, "AEC_STOREY", tab).is_some());

        assert!(linked_storey(&app.tabs[tab].scene.document).is_none());
        assert_eq!(app.tabs[tab].scene.document.entities().count(), 0);
    }

    #[test]
    fn project_storey_sync_refreshes_cache_for_ifc_export() {
        let temp_project = TempProjectRoot::new("ifc_storey_sync");
        let drawing_path = temp_project.path.join("EG.dwg");
        std::fs::write(&drawing_path, b"drawing").expect("write drawing");

        let mut app = OpenCADStudio::new_for_test();
        let tab = app.active_tab;
        app.tabs[tab].set_drawing_path_for_test(drawing_path);
        app.aec.aec_project_explorer_path = Some(temp_project.project_path());

        let mut live_storey = StoreyRef::new_with_height("Current", 0.0, 3.0, "EG.dwg");
        let storey_id = live_storey.id;
        let mut stale_storey = live_storey.clone();
        stale_storey.name = "Stale cache".to_string();
        stale_storey.set_elevation(-2.0);
        stale_storey.set_height(2.0);
        let stale_handle = write_storey_link(&mut app.tabs[tab].scene, &stale_storey);

        live_storey.set_elevation(4.0);
        live_storey.set_height(3.5);
        let mut building = Building::new("House");
        building.storeys.push(live_storey);
        let mut project = ProjectFile::default();
        project.buildings.push(building);
        app.aec.aec_project_explorer_file = Some(project);

        let matched_storey = project_storey_for_tab(&app, tab).expect("matching project storey");
        assert_eq!(
            sync_project_storey_link(&mut app, tab, &matched_storey),
            Some(stale_handle)
        );

        let (storey_id_on_drawing, linked, linked_project_id) =
            linked_storey(&app.tabs[tab].scene.document).unwrap();
        assert_eq!(storey_id_on_drawing, 0);
        assert_eq!(linked_project_id, storey_id);
        assert_eq!(linked.name, "Current");
        assert!((linked.elevation - 4.0).abs() < 1e-9);
        assert!((linked.height - 3.5).abs() < 1e-9);
        assert!(app.tabs[tab].dirty);
    }

    #[test]
    fn test_drawing_path_matches_storey_base_dir_and_collision() {
        let cur = Path::new("/proj/HausA/EG.dwg");
        let base = Path::new("/proj");

        // Matching relative path under base directory
        assert!(drawing_path_matches_storey(cur, "HausA/EG.dwg", Some(base)));

        // Foreign relative path with same filename under different folder must NOT match
        assert!(!drawing_path_matches_storey(cur, "HausB/EG.dwg", Some(base)));

        // Foreign relative path with different filename
        assert!(!drawing_path_matches_storey(cur, "HausA/OG.dwg", Some(base)));

        // Empty path
        assert!(!drawing_path_matches_storey(cur, "   ", Some(base)));

        // Base-dir pointing to HausA folder matching bare EG.dwg
        assert!(drawing_path_matches_storey(cur, "EG.dwg", Some(Path::new("/proj/HausA"))));

        // Base-dir pointing to HausB folder does NOT match /proj/HausA/EG.dwg
        assert!(!drawing_path_matches_storey(cur, "EG.dwg", Some(Path::new("/proj/HausB"))));
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
            resolve_control_plane_storey_ids(&project, Some(Path::new("/tmp/eg.dwg")), None),
            Some(expected)
        );
        assert_eq!(
            resolve_control_plane_storey_ids(&project, Some(Path::new("/tmp/missing.dwg")), None),
            Some(first)
        );
        assert_eq!(
            resolve_control_plane_storey_ids(&project, None, None),
            Some(first)
        );
    }

    #[test]
    fn spawn_aec_plane_facet_do_targeted_happy_path_matching_tab() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let temp_project = TempProjectRoot::new("facet_matching_tab");
        let project_path = temp_project.project_path();
        app.tabs[tab].set_drawing_path_for_test(temp_project.path.join("storeys/eg.dwg"));
        app.aec.aec_project_explorer_path = Some(project_path.clone());

        let mut project = ProjectFile::default();
        let mut b = Building::new("House");
        let storey = StoreyRef::new("EG", 0.0, "storeys/eg.dwg");
        let bid = b.id;
        let sid = storey.id;
        let pid = storey.floor_plane_id;
        b.storeys.push(storey);
        project.buildings.push(b);
        app.aec.aec_project_explorer_file = Some(project);

        let initial_entities_count = app.tabs[tab].scene.document.entities().count();

        let cmd = format!("AEC_PLANE_FACET_DO {bid}|{sid}|{pid} 0.0 0.0 0.0 4.0 0.0 0.0 4.0 4.0 0.5");
        let (outcome, _task) = dispatch_plane_facet_do_helper(&mut app, tab, &cmd);

        assert_eq!(
            outcome,
            PlaneTargetDispatchOutcome::ReopenStoreySettings(bid, sid),
            "outcome must be reopening storey dialog"
        );
        assert!(app.tabs[tab].dirty, "matching tab must be marked dirty");

        let storey_after = app
            .aec
            .aec_project_explorer_file
            .as_ref()
            .unwrap()
            .buildings[0]
            .storeys[0]
            .clone();
        let floor = storey_after.plane(pid).unwrap();
        assert_eq!(floor.facets.len(), 2, "facet should be added to existing target plane");

        let new_facet = &floor.facets[1];
        assert!(
            new_facet.preview_handle.is_some(),
            "preview handle should be assigned when tab matches"
        );
        assert!(
            app.tabs[tab].scene.document.entities().count() > initial_entities_count,
            "preview entity should be generated in scene"
        );
        let saved = load_persisted_project(&project_path);
        assert_eq!(
            persisted_storey(&saved, bid, sid).plane(pid).unwrap().facets.len(),
            2,
            "saved project should contain the added facet"
        );
    }

    #[test]
    fn spawn_aec_plane_facet_do_empty_targeted_reopens_dialog_without_changes() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let mut project = ProjectFile::default();
        let mut b = Building::new("House");
        let storey = StoreyRef::new("EG", 0.0, "eg.dwg");
        let bid = b.id;
        let sid = storey.id;
        let pid = storey.floor_plane_id;
        b.storeys.push(storey);
        project.buildings.push(b);
        app.aec.aec_project_explorer_file = Some(project);

        let cmd = format!("AEC_PLANE_FACET_DO {bid}|{sid}|{pid}");
        let (outcome, _task) = dispatch_plane_facet_do_helper(&mut app, tab, &cmd);

        assert_eq!(
            outcome,
            PlaneTargetDispatchOutcome::ReopenStoreySettings(bid, sid),
            "empty targeted dispatch should reopen storey dialog"
        );
        assert!(!app.tabs[tab].dirty, "cancel should not dirty tab");

        let storey_after = app
            .aec
            .aec_project_explorer_file
            .as_ref()
            .unwrap()
            .buildings[0]
            .storeys[0]
            .clone();
        let floor = storey_after.plane(pid).unwrap();
        assert_eq!(floor.facets.len(), 1, "no facets should be added on cancel");
    }

    #[test]
    fn spawn_aec_plane_facet_do_malformed_target_reports_error() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let mut project = ProjectFile::default();
        let mut b = Building::new("House");
        let storey = StoreyRef::new("EG", 0.0, "eg.dwg");
        b.storeys.push(storey);
        project.buildings.push(b);
        app.aec.aec_project_explorer_file = Some(project);

        let cmd = "AEC_PLANE_FACET_DO invalid|target 0.0 0.0 0.0 4.0 0.0 0.0 4.0 4.0 0.5";
        let (outcome, _task) = dispatch_plane_facet_do_helper(&mut app, tab, cmd);

        assert!(
            matches!(outcome, PlaneTargetDispatchOutcome::Error(_)),
            "malformed target must produce error outcome"
        );
        assert!(app.command_line.last_error.is_some(), "command line should record error");
        assert!(!app.tabs[tab].dirty, "error should not dirty tab");

        let storey_after = app
            .aec
            .aec_project_explorer_file
            .as_ref()
            .unwrap()
            .buildings[0]
            .storeys[0]
            .clone();
        assert_eq!(storey_after.control_planes.len(), 2, "no plane should be created");
    }

    #[test]
    fn spawn_aec_plane_facet_do_missing_storey_reports_error() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let mut project = ProjectFile::default();
        let b = Building::new("House");
        let bid = b.id;
        let random_sid = Uuid::new_v4();
        let random_pid = Uuid::new_v4();
        project.buildings.push(b);
        app.aec.aec_project_explorer_file = Some(project);

        let cmd = format!("AEC_PLANE_FACET_DO {bid}|{random_sid}|{random_pid} 0.0 0.0 0.0 4.0 0.0 0.0 4.0 4.0 0.5");
        let (outcome, _task) = dispatch_plane_facet_do_helper(&mut app, tab, &cmd);

        assert!(
            matches!(outcome, PlaneTargetDispatchOutcome::Error(_)),
            "missing storey must yield error outcome"
        );
        assert!(
            app.command_line
                .last_error
                .as_deref()
                .unwrap_or("")
                .contains("Zielgeschoss"),
            "error message should indicate missing storey"
        );
        assert!(!app.tabs[tab].dirty, "missing storey error should not dirty tab");
    }

    #[test]
    fn spawn_aec_plane_facet_do_missing_target_plane_reports_error_and_reopens() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let mut project = ProjectFile::default();
        let mut b = Building::new("House");
        let storey = StoreyRef::new("EG", 0.0, "eg.dwg");
        let bid = b.id;
        let sid = storey.id;
        let random_pid = Uuid::new_v4();
        b.storeys.push(storey);
        project.buildings.push(b);
        app.aec.aec_project_explorer_file = Some(project);

        let cmd = format!("AEC_PLANE_FACET_DO {bid}|{sid}|{random_pid} 0.0 0.0 0.0 4.0 0.0 0.0 4.0 4.0 0.5");
        let (outcome, _task) = dispatch_plane_facet_do_helper(&mut app, tab, &cmd);

        assert_eq!(
            outcome,
            PlaneTargetDispatchOutcome::ReopenStoreySettings(bid, sid),
            "missing target plane on valid storey should still reopen dialog"
        );
        assert!(
            app.command_line
                .last_error
                .as_deref()
                .unwrap_or("")
                .contains("Zielebene"),
            "error message should indicate missing plane"
        );
        assert!(!app.tabs[tab].dirty, "missing plane must not dirty tab");

        let storey_after = app
            .aec
            .aec_project_explorer_file
            .as_ref()
            .unwrap()
            .buildings[0]
            .storeys[0]
            .clone();
        assert_eq!(storey_after.control_planes.len(), 2, "no plane or facet should be created on error");
    }

    #[test]
    fn spawn_aec_plane_facet_do_foreign_tab_same_basename_does_not_modify_foreign_scene() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let temp_project = TempProjectRoot::new("facet_foreign_tab");
        let project_path = temp_project.project_path();

        // Active tab is HausA/EG.dwg
        app.tabs[tab].set_drawing_path_for_test(temp_project.path.join("HausA/EG.dwg"));
        app.aec.aec_project_explorer_path = Some(project_path.clone());

        let mut project = ProjectFile::default();
        let mut b_a = Building::new("HausA");
        let storey_a = StoreyRef::new("EG", 0.0, "HausA/EG.dwg");
        b_a.storeys.push(storey_a);

        let mut b_b = Building::new("HausB");
        let storey_b = StoreyRef::new("EG", 0.0, "HausB/EG.dwg");
        let bid_b = b_b.id;
        let sid_b = storey_b.id;
        let pid_b = storey_b.floor_plane_id;
        b_b.storeys.push(storey_b);

        project.buildings.push(b_a);
        project.buildings.push(b_b);
        app.aec.aec_project_explorer_file = Some(project);

        let initial_entities_count = app.tabs[tab].scene.document.entities().count();

        let cmd = format!("AEC_PLANE_FACET_DO {bid_b}|{sid_b}|{pid_b} 0.0 0.0 0.0 4.0 0.0 0.0 4.0 4.0 0.5");
        let (outcome, _task) = dispatch_plane_facet_do_helper(&mut app, tab, &cmd);

        assert_eq!(
            outcome,
            PlaneTargetDispatchOutcome::ReopenStoreySettings(bid_b, sid_b),
            "dialog should reopen for target storey"
        );
        assert!(
            !app.tabs[tab].dirty,
            "foreign tab must NOT be marked dirty even with identical filename"
        );
        assert_eq!(
            app.tabs[tab].scene.document.entities().count(),
            initial_entities_count,
            "foreign tab scene must not have preview entities added"
        );

        let storey_b_after = app
            .aec
            .aec_project_explorer_file
            .as_ref()
            .unwrap()
            .buildings[1]
            .storeys[0]
            .clone();
        let floor_b = storey_b_after.plane(pid_b).unwrap();
        assert_eq!(floor_b.facets.len(), 2, "storey in project should still be updated");
        assert_eq!(
            floor_b.facets[1].preview_handle,
            None,
            "new facet preview_handle must be None because tab was foreign"
        );
        let saved = load_persisted_project(&project_path);
        assert_eq!(
            persisted_storey(&saved, bid_b, sid_b)
                .plane(pid_b)
                .unwrap()
                .facets
                .len(),
            2,
            "saved project should contain the added facet for the foreign target"
        );
    }

    #[test]
    fn spawn_aec_plane_assign_do_matching_and_foreign_tab() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let temp_project = TempProjectRoot::new("plane_assign_tabs");
        let project_path = temp_project.project_path();

        // Create a 3D face entity in scene to assign
        let face = acadrust::entities::Face3D::new(
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(4.0, 0.0, 1.0),
            Vector3::new(4.0, 4.0, 1.5),
            Vector3::new(0.0, 4.0, 1.5),
        );
        let poly_handle = app.tabs[tab].scene.add_entity(EntityType::Face3D(face));

        app.tabs[tab].set_drawing_path_for_test(temp_project.path.join("HausA/EG.dwg"));
        app.aec.aec_project_explorer_path = Some(project_path.clone());

        let mut project = ProjectFile::default();
        let mut b_a = Building::new("HausA");
        let storey_a = StoreyRef::new("EG", 0.0, "HausA/EG.dwg");
        let bid_a = b_a.id;
        let sid_a = storey_a.id;
        let pid_a = storey_a.floor_plane_id;
        b_a.storeys.push(storey_a);

        let mut b_b = Building::new("HausB");
        let storey_b = StoreyRef::new("EG", 0.0, "HausB/EG.dwg");
        let bid_b = b_b.id;
        let sid_b = storey_b.id;
        let pid_b = storey_b.floor_plane_id;
        b_b.storeys.push(storey_b);

        project.buildings.push(b_a);
        project.buildings.push(b_b);
        app.aec.aec_project_explorer_file = Some(project);

        // Case 1: Matching tab assignment
        let cmd_matching = format!("AEC_PLANE_ASSIGN_DO {bid_a}|{sid_a}|{pid_a} {}", poly_handle.value());
        let (outcome, _task) = dispatch_plane_assign_do_helper(&mut app, tab, &cmd_matching);
        assert_eq!(outcome, PlaneTargetDispatchOutcome::ReopenStoreySettings(bid_a, sid_a));
        assert!(app.tabs[tab].dirty, "matching tab must be dirty on assign");
        let saved_matching = load_persisted_project(&project_path);
        assert_eq!(
            persisted_storey(&saved_matching, bid_a, sid_a)
                .plane(pid_a)
                .unwrap()
                .facets[0]
                .name,
            "Facet_1_Face3D",
            "saved project should contain the assigned polygon"
        );

        // Case 2: Foreign tab assignment (same filename EG.dwg in different folder HausB)
        app.tabs[tab].dirty = false;
        let initial_entities = app.tabs[tab].scene.document.entities().count();
        let cmd_foreign = format!("AEC_PLANE_ASSIGN_DO {bid_b}|{sid_b}|{pid_b} {}", poly_handle.value());
        let (outcome_f, _task) = dispatch_plane_assign_do_helper(&mut app, tab, &cmd_foreign);
        assert_eq!(outcome_f, PlaneTargetDispatchOutcome::ReopenStoreySettings(bid_b, sid_b));
        assert!(!app.tabs[tab].dirty, "foreign tab must NOT be dirty on assign");
        assert_eq!(
            app.tabs[tab].scene.document.entities().count(),
            initial_entities,
            "foreign tab scene must not have previews generated"
        );
        let saved_foreign = load_persisted_project(&project_path);
        assert_eq!(
            persisted_storey(&saved_foreign, bid_b, sid_b)
                .plane(pid_b)
                .unwrap()
                .facets[0]
                .name,
            "Facet_1_Face3D",
            "saved project should contain the foreign-tab assignment"
        );

        // Case 3: Missing pid on valid storey reports error and reopens
        let random_pid = Uuid::new_v4();
        let cmd_missing_pid = format!("AEC_PLANE_ASSIGN_DO {bid_a}|{sid_a}|{random_pid} {}", poly_handle.value());
        let (outcome_m, _task) = dispatch_plane_assign_do_helper(&mut app, tab, &cmd_missing_pid);
        assert_eq!(outcome_m, PlaneTargetDispatchOutcome::ReopenStoreySettings(bid_a, sid_a));
        assert!(
            app.command_line.last_error.as_deref().unwrap_or("").contains("Zielebene"),
            "error should indicate missing plane"
        );

        // Case 4: Cancel / empty handles reopens dialog without dirtying
        app.tabs[tab].dirty = false;
        let cmd_cancel = format!("AEC_PLANE_ASSIGN_DO {bid_a}|{sid_a}|{pid_a}");
        let (outcome_c, _task) = dispatch_plane_assign_do_helper(&mut app, tab, &cmd_cancel);
        assert_eq!(outcome_c, PlaneTargetDispatchOutcome::ReopenStoreySettings(bid_a, sid_a));
        assert!(!app.tabs[tab].dirty);
    }
}
