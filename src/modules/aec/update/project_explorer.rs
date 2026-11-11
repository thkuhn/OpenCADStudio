//! AEC update handlers: Project explorer: load/save, buildings, storeys.

use iced::Task;

use crate::app::{AecModalKind, AecProjectExplorerDeleteTarget, Message, OpenCADStudio};
use crate::modules::aec::message::AecMessage;

impl OpenCADStudio {
    pub(crate) fn update_aec_project_explorer(&mut self, msg: AecMessage) -> Task<Message> {
        match msg {
            AecMessage::AecProjectExplorerOpen => {
                self.ribbon.close_dropdown();
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::ProjectExplorer));
                Task::none()
            }
            AecMessage::AecProjectExplorerNew => {
                self.aec.aec_project_explorer_file =
                    Some(crate::modules::aec::engine::project::ProjectFile::default());
                self.aec.aec_project_explorer_path = None;
                self.aec.aec_project_explorer_selected_building = None;
                self.aec.aec_project_explorer_selected_storey = None;
                self.aec.aec_project_explorer_new_building_name.clear();
                self.aec.aec_project_explorer_new_storey_name.clear();
                self.aec.aec_project_explorer_new_storey_elevation = "0.0".to_string();
                self.aec.aec_project_explorer_new_storey_drawing.clear();
                self.aec.aec_project_explorer_ffl0_nn.clear();
                // Refresh the plan-type library so the status-bar picker
                // reflects this (empty) project's library immediately,
                // instead of still showing a previously loaded drawing's
                // global/library-file entries until the Plan Manager is
                // opened once.
                self.aec.aec_plan_library = Some(
                    crate::modules::aec::engine::project::resolve_display_config_library(
                        self.aec.aec_project_explorer_file.as_ref(),
                    ),
                );
                // Coming from the project-required gate: replay the tool the
                // user originally asked for (e.g. AEC_WALL) instead of just
                // opening the explorer and leaving them stuck; fall back to
                // opening the explorer if there is nothing to resume (i.e.
                // this was invoked directly via AEC_PROJECTEXPLORER).
                if self.active_modal == Some(crate::app::ModalKind::Aec(AecModalKind::ProjectRequired)) {
                    self.reset_modal_geometry();
                    self.active_modal = None;
                    if let Some(resume) = self.aec.aec_project_required_resume.take() {
                        return Task::done(resume);
                    }
                    self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::ProjectExplorer));
                }
                Task::none()
            }
            AecMessage::AecProjectExplorerLoad => Task::perform(
                async {
                    crate::sys::file_dialog()
                        .set_title(crate::t!("Open Project").as_ref())
                        .add_filter(crate::t!("OpenCADStudio Project").as_ref(), &["ocsproj", "OCSPROJ"])
                        .add_filter(crate::t!("All Files").as_ref(), &["*"])
                        .pick_file()
                        .await
                        .map(|h| crate::sys::handle_path(&h))
                },
                |path| Message::Aec(AecMessage::AecProjectExplorerLoadResult(path)),
            ),
            AecMessage::AecProjectExplorerLoadResult(None) => Task::none(),
            AecMessage::AecProjectExplorerLoadResult(Some(path)) => {
                match crate::modules::aec::engine::project::ProjectFile::load(&path) {
                    Ok(project) => {
                        self.aec.aec_project_explorer_ffl0_nn = project
                            .ffl0_nn_m
                            .map(|v| format!("{v}"))
                            .unwrap_or_default();
                        self.aec.aec_project_explorer_file = Some(project);
                        self.aec.aec_project_explorer_path = Some(path);
                        self.aec.aec_project_explorer_selected_building = None;
                        self.aec.aec_project_explorer_selected_storey = None;
                        // Same reasoning as `AecProjectExplorerNew`: make the
                        // freshly loaded project's own plan-type library
                        // visible right away instead of only after the Plan
                        // Manager is opened once.
                        self.aec.aec_plan_library = Some(
                            crate::modules::aec::engine::project::resolve_display_config_library(
                                self.aec.aec_project_explorer_file.as_ref(),
                            ),
                        );
                        // Same reasoning as `AecProjectExplorerNew`: replay
                        // the originally requested tool instead of just
                        // opening the explorer and leaving the user stuck.
                        if self.active_modal == Some(crate::app::ModalKind::Aec(AecModalKind::ProjectRequired)) {
                            self.reset_modal_geometry();
                            self.active_modal = None;
                            if let Some(resume) = self.aec.aec_project_required_resume.take() {
                                return Task::done(resume);
                            }
                            self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::ProjectExplorer));
                        }
                    }
                    Err(e) => {
                        self.command_line.push_error(
                            crate::tf!("AEC Project Explorer: failed to load: {e}").as_ref(),
                        );
                    }
                }
                Task::none()
            }
            AecMessage::AecProjectExplorerSave => {
                self.aec_project_explorer_apply_pending_edits();
                if self.aec.aec_project_explorer_path.is_some() {
                    self.aec_project_explorer_persist();
                    Task::none()
                } else {
                    self.update(Message::Aec(AecMessage::AecProjectExplorerSaveAs))
                }
            }
            AecMessage::AecProjectExplorerSaveAs => Task::perform(
                async {
                    crate::sys::file_dialog()
                        .set_title(crate::t!("Save Project As").as_ref())
                        .set_file_name("project.ocsproj")
                        .add_filter(crate::t!("OpenCADStudio Project").as_ref(), &["ocsproj", "OCSPROJ"])
                        .add_filter(crate::t!("All Files").as_ref(), &["*"])
                        .save_file()
                        .await
                        .map(|h| crate::sys::handle_path(&h))
                },
                |path| Message::Aec(AecMessage::AecProjectExplorerSaveAsResult(path)),
            ),
            AecMessage::AecProjectExplorerSaveAsResult(None) => Task::none(),
            AecMessage::AecProjectExplorerSaveAsResult(Some(path)) => {
                self.aec_project_explorer_apply_pending_edits();
                self.aec.aec_project_explorer_path = Some(path);
                self.aec_project_explorer_persist();
                Task::none()
            }
            AecMessage::AecProjectExplorerSelectBuilding(bid) => {
                self.aec.aec_project_explorer_selected_building = Some(bid);
                self.aec.aec_project_explorer_selected_storey = None;
                self.aec.aec_project_explorer_edit_building_name = self
                    .aec.aec_project_explorer_file
                    .as_ref()
                    .and_then(|p| p.buildings.iter().find(|b| b.id == bid))
                    .map(|b| b.name.clone())
                    .unwrap_or_default();
                Task::none()
            }
            AecMessage::AecProjectExplorerSelectStorey(bid, sid) => {
                self.aec.aec_project_explorer_selected_building = Some(bid);
                self.aec.aec_project_explorer_selected_storey = Some((bid, sid));
                let storey = self
                    .aec.aec_project_explorer_file
                    .as_ref()
                    .and_then(|p| p.buildings.iter().find(|b| b.id == bid))
                    .and_then(|b| b.storeys.iter().find(|s| s.id == sid));
                self.aec.aec_project_explorer_edit_storey_name =
                    storey.map(|s| s.name.clone()).unwrap_or_default();
                self.aec.aec_project_explorer_edit_elevation = storey
                    .map(|s| format!("{:.3}", s.elevation))
                    .unwrap_or_default();
                self.aec.aec_project_explorer_edit_storey_drawing =
                    storey.map(|s| s.drawing_path.clone()).unwrap_or_default();
                Task::none()
            }
            AecMessage::AecProjectExplorerOpenStorey(bid, sid) => {
                let Some(project) = self.aec.aec_project_explorer_file.as_ref() else {
                    return Task::none();
                };
                let Some(building) = project.buildings.iter().find(|b| b.id == bid) else {
                    return Task::none();
                };
                let Some(storey) = building.storeys.iter().find(|s| s.id == sid) else {
                    return Task::none();
                };
                let drawing = std::path::PathBuf::from(&storey.drawing_path);
                let resolved = if drawing.is_absolute() {
                    drawing
                } else if let Some(base) = self
                    .aec.aec_project_explorer_path
                    .as_ref()
                    .and_then(|p| p.parent())
                {
                    base.join(drawing)
                } else {
                    drawing
                };
                // Keep the explorer open; open the drawing in a new tab.
                Task::done(Message::OpenExternal(resolved))
            }
            AecMessage::AecProjectExplorerAddBuilding => {
                let name = self.aec.aec_project_explorer_new_building_name.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                let project = self
                    .aec.aec_project_explorer_file
                    .get_or_insert_with(crate::modules::aec::engine::project::ProjectFile::default);
                let building = crate::modules::aec::engine::project::Building::new(name);
                let bid = building.id;
                project.buildings.push(building);
                self.aec.aec_project_explorer_selected_building = Some(bid);
                self.aec.aec_project_explorer_selected_storey = None;
                self.aec.aec_project_explorer_edit_building_name.clear();
                self.aec.aec_project_explorer_new_building_name.clear();
                self.aec_project_explorer_persist_if_pathed();
                Task::none()
            }
            AecMessage::AecProjectExplorerEditBuildingName(_bid, name) => {
                self.aec.aec_project_explorer_edit_building_name = name;
                Task::none()
            }
            AecMessage::AecProjectExplorerSaveBuildingEdits(bid) => {
                let name = self.aec.aec_project_explorer_edit_building_name.clone();
                if let Some(project) = self.aec.aec_project_explorer_file.as_mut() {
                    if let Some(building) = project.buildings.iter_mut().find(|b| b.id == bid) {
                        building.name = name;
                    }
                }
                self.aec_project_explorer_persist_if_pathed();
                Task::none()
            }
            AecMessage::AecProjectExplorerEditStoreyName(_bid, _sid, name) => {
                self.aec.aec_project_explorer_edit_storey_name = name;
                Task::none()
            }
            AecMessage::AecProjectExplorerEditStoreyElevation(_bid, _sid, text) => {
                self.aec.aec_project_explorer_edit_elevation = text;
                Task::none()
            }
            AecMessage::AecProjectExplorerEditStoreyDrawing(_bid, _sid, text) => {
                self.aec.aec_project_explorer_edit_storey_drawing = text;
                Task::none()
            }
            AecMessage::AecProjectExplorerSaveStoreyEdits(bid, sid) => {
                let name = self.aec.aec_project_explorer_edit_storey_name.clone();
                let drawing = self.aec.aec_project_explorer_edit_storey_drawing.clone();
                let elevation = self.aec.aec_project_explorer_edit_elevation.trim().parse::<f64>().ok();
                if let Some(project) = self.aec.aec_project_explorer_file.as_mut() {
                    if let Some(storey) = project
                        .buildings
                        .iter_mut()
                        .find(|b| b.id == bid)
                        .and_then(|b| b.storeys.iter_mut().find(|s| s.id == sid))
                    {
                        storey.name = name;
                        storey.drawing_path = drawing;
                        if let Some(value) = elevation {
                            storey.set_elevation(value);
                        }
                    }
                }
                self.aec_project_explorer_persist_if_pathed();
                Task::none()
            }
            AecMessage::AecProjectExplorerRequestDeleteBuilding(bid) => {
                self.aec.aec_project_explorer_pending_delete =
                    Some(AecProjectExplorerDeleteTarget::Building(bid));
                Task::none()
            }
            AecMessage::AecProjectExplorerRequestDeleteStorey(bid, sid) => {
                self.aec.aec_project_explorer_pending_delete =
                    Some(AecProjectExplorerDeleteTarget::Storey(bid, sid));
                Task::none()
            }
            AecMessage::AecProjectExplorerCancelDelete => {
                self.aec.aec_project_explorer_pending_delete = None;
                Task::none()
            }
            AecMessage::AecProjectExplorerConfirmDelete => {
                match self.aec.aec_project_explorer_pending_delete.take() {
                    Some(AecProjectExplorerDeleteTarget::Building(bid)) => {
                        if let Some(project) = self.aec.aec_project_explorer_file.as_mut() {
                            if let Some(bi) = project.building_index(bid) {
                                project.buildings.remove(bi);
                            }
                        }
                        self.aec.aec_project_explorer_selected_building = None;
                        self.aec.aec_project_explorer_selected_storey = None;
                        self.aec_project_explorer_persist_if_pathed();
                    }
                    Some(AecProjectExplorerDeleteTarget::Storey(bid, sid)) => {
                        if let Some(project) = self.aec.aec_project_explorer_file.as_mut() {
                            if let Some(building) =
                                project.building_index(bid).and_then(|bi| project.buildings.get_mut(bi))
                            {
                                if let Some(si) = building.storey_index(sid) {
                                    building.storeys.remove(si);
                                }
                            }
                        }
                        self.aec.aec_project_explorer_selected_storey = None;
                        self.aec_project_explorer_persist_if_pathed();
                    }
                    None => {}
                }
                Task::none()
            }
            AecMessage::AecProjectExplorerAddStorey => {
                let Some(bid) = self.aec.aec_project_explorer_selected_building else {
                    self.command_line.push_info(
                        crate::t!("AEC Project Explorer: select a building first.").as_ref(),
                    );
                    return Task::none();
                };
                let name = self.aec.aec_project_explorer_new_storey_name.trim().to_string();
                let drawing = self.aec.aec_project_explorer_new_storey_drawing.trim().to_string();
                if name.is_empty() {
                    self.command_line.push_info(
                        crate::t!("AEC Project Explorer: a storey name is required.").as_ref(),
                    );
                    return Task::none();
                }
                let elevation = self
                    .aec.aec_project_explorer_new_storey_elevation
                    .trim()
                    .parse::<f64>()
                    .unwrap_or(0.0);
                let Some(project) = self.aec.aec_project_explorer_file.as_mut() else {
                    return Task::none();
                };
                let Some(building) =
                    project.building_index(bid).and_then(|bi| project.buildings.get_mut(bi))
                else {
                    return Task::none();
                };
                let storey = crate::modules::aec::engine::project::StoreyRef::new(
                    name, elevation, drawing,
                );
                let sid = storey.id;
                building.storeys.push(storey);
                self.aec.aec_project_explorer_selected_storey = Some((bid, sid));
                self.aec.aec_project_explorer_new_storey_name.clear();
                self.aec.aec_project_explorer_new_storey_drawing.clear();
                self.aec_project_explorer_persist_if_pathed();
                Task::none()
            }
            AecMessage::AecProjectExplorerNewBuildingNameChanged(v) => {
                self.aec.aec_project_explorer_new_building_name = v;
                Task::none()
            }
            AecMessage::AecProjectExplorerNewStoreyNameChanged(v) => {
                self.aec.aec_project_explorer_new_storey_name = v;
                Task::none()
            }
            AecMessage::AecProjectExplorerNewStoreyElevationChanged(v) => {
                self.aec.aec_project_explorer_new_storey_elevation = v;
                Task::none()
            }
            AecMessage::AecProjectExplorerNewStoreyDrawingChanged(v) => {
                self.aec.aec_project_explorer_new_storey_drawing = v;
                Task::none()
            }
            AecMessage::AecProjectExplorerPickStoreyDrawing => Task::perform(
                async {
                    crate::sys::file_dialog()
                        .set_title(crate::t!("Select Storey Drawing").as_ref())
                        .add_filter(crate::t!("CAD Files").as_ref(), &["dwg", "dxf", "DWG", "DXF"])
                        .add_filter(crate::t!("All Files").as_ref(), &["*"])
                        .pick_file()
                        .await
                        .map(|h| crate::sys::handle_path(&h))
                },
                |path| Message::Aec(AecMessage::AecProjectExplorerPickStoreyDrawingResult(path)),
            ),
            AecMessage::AecProjectExplorerPickStoreyDrawingResult(None) => Task::none(),
            AecMessage::AecProjectExplorerPickStoreyDrawingResult(Some(path)) => {
                // Prefer a path relative to the project file when possible.
                let display = if let Some(base) = self
                    .aec.aec_project_explorer_path
                    .as_ref()
                    .and_then(|p| p.parent())
                {
                    path.strip_prefix(base)
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|_| path.clone())
                        .to_string_lossy()
                        .into_owned()
                } else {
                    path.to_string_lossy().into_owned()
                };
                self.aec.aec_project_explorer_new_storey_drawing = display;
                Task::none()
            }
            // `bid`/`sid` aren't needed in the result — only one storey can be
            // selected/edited at a time, so the pick always targets the
            // currently selected storey's edit buffer.
            AecMessage::AecProjectExplorerPickEditStoreyDrawing(_bid, _sid) => Task::perform(
                async {
                    crate::sys::file_dialog()
                        .set_title(crate::t!("Select Storey Drawing").as_ref())
                        .add_filter(crate::t!("CAD Files").as_ref(), &["dwg", "dxf", "DWG", "DXF"])
                        .add_filter(crate::t!("All Files").as_ref(), &["*"])
                        .pick_file()
                        .await
                        .map(|h| crate::sys::handle_path(&h))
                },
                |path| Message::Aec(AecMessage::AecProjectExplorerPickEditStoreyDrawingResult(path)),
            ),
            AecMessage::AecProjectExplorerPickEditStoreyDrawingResult(None) => Task::none(),
            AecMessage::AecProjectExplorerPickEditStoreyDrawingResult(Some(path)) => {
                // Prefer a path relative to the project file when possible.
                let display = if let Some(base) = self
                    .aec.aec_project_explorer_path
                    .as_ref()
                    .and_then(|p| p.parent())
                {
                    path.strip_prefix(base)
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|_| path.clone())
                        .to_string_lossy()
                        .into_owned()
                } else {
                    path.to_string_lossy().into_owned()
                };
                self.aec.aec_project_explorer_edit_storey_drawing = display.clone();
                if let Some((bid, sid)) = self.aec.aec_storey_settings_target {
                    self.with_storey_mut(bid, sid, |s| s.drawing_path = display);
                    self.aec_project_explorer_persist_if_pathed();
                }
                Task::none()
            }
            AecMessage::AecProjectExplorerMigrateLibraries => {
                if let Some(project) = self.aec.aec_project_explorer_file.as_mut() {
                    crate::modules::aec::engine::project::migrate_file_library_to_project(project);
                    self.aec_project_explorer_persist_if_pathed();
                    self.command_line.push_info(
                        crate::t!("AEC Project Explorer: libraries migrated into project.").as_ref(),
                    );
                } else {
                    self.command_line.push_error(
                        crate::t!("AEC Project Explorer: no project loaded to migrate into.")
                            .as_ref(),
                    );
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }
}
