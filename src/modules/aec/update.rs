//! AEC iced update hook — handler bodies for [`AecMessage`].
//!
//! Core `update_inner` delegates `Message::Aec(msg)` here so AEC work does
//! not grow new match arms in `src/app/update/mod.rs`.

use iced::Task;

use crate::app::{AecModalKind, AecPendingCopy, AecProjectExplorerDeleteTarget, Message, OpenCADStudio};
use crate::modules::aec::engine::join_ops;
use crate::modules::aec::engine::xdata;
use crate::modules::aec::message::AecMessage;

/// Stable core hook: dispatch one AEC UI/update message.
pub(crate) fn update(app: &mut OpenCADStudio, msg: AecMessage) -> Task<Message> {
    app.update_aec(msg)
}

impl OpenCADStudio {
    pub(crate) fn update_aec(&mut self, msg: AecMessage) -> Task<Message> {
        match msg {
            AecMessage::AecMaterialManagerOpen => {
                self.ribbon.close_dropdown();
                self.aec_refresh_combined_style_library();
                self.aec.aec_style_manager_filter.clear();
                self.aec.aec_style_manager_selected_material = None;
                self.aec.aec_style_manager_selected_wall_style = None;
                self.aec.aec_style_manager_material_editing_id = None;
                self.aec.aec_style_manager_material_form_open = false;
                self.aec.aec_style_manager_wall_style_editing_id = None;
                self.aec.aec_style_manager_wall_style_form_open = false;
                self.aec.aec_style_manager_wall_style_layers.clear();
                self.refresh_aec_material_linetype_combo();
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::MaterialManager));
                Task::none()
            }
            AecMessage::AecWallStyleManagerOpen => {
                self.ribbon.close_dropdown();
                self.aec_refresh_combined_style_library();
                self.aec.aec_style_manager_filter.clear();
                self.aec.aec_style_manager_selected_material = None;
                self.aec.aec_style_manager_selected_wall_style = None;
                self.aec.aec_style_manager_material_editing_id = None;
                self.aec.aec_style_manager_material_form_open = false;
                self.aec.aec_style_manager_wall_style_editing_id = None;
                self.aec.aec_style_manager_wall_style_form_open = false;
                self.aec.aec_style_manager_wall_style_layers.clear();
                self.aec.aec_style_manager_profile_selected = None;
                self.aec.aec_style_manager_profile_contour_explicit = false;
                self.aec.aec_style_manager_profile_contour_selection.clear();
                self.aec.aec_style_manager_profile_solid_explicit = false;
                self.aec.aec_style_manager_profile_solid_selection.clear();
                self.aec.aec_style_manager_profile_hatch_angle.clear();
                self.aec.aec_style_manager_profile_hatch_relative = false;
                // Load (or refresh) the DisplayConfig library so the
                // "Darstellungs-Profile" table has data even if the Plan
                // Manager was never opened this session.
                self.aec.aec_plan_library = Some(
                    crate::modules::aec::engine::project::resolve_display_config_library(
                        self.aec.aec_project_explorer_file.as_ref(),
                    ),
                );
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::WallStyleManager));
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerOpen => self.aec_opening_style_manager_open(),
            AecMessage::AecOpeningStyleManagerFilter(value) => {
                self.aec.aec_opening_style_manager_filter = value;
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerSelect(id) => {
                self.aec_opening_style_manager_select(id)
            }
            AecMessage::AecOpeningStyleManagerNew => self.aec_opening_style_manager_new(),
            AecMessage::AecOpeningStyleManagerNameChanged(value) => {
                self.aec.aec_opening_style_manager_name = value;
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerParentChanged(value) => {
                self.aec.aec_opening_style_manager_parent = value;
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerKindChanged(value) => {
                self.aec_opening_style_manager_kind_changed(value)
            }
            AecMessage::AecOpeningStyleManagerShapeChanged(value) => {
                self.aec_opening_style_manager_shape_changed(value)
            }
            AecMessage::AecOpeningStyleManagerWidthChanged(value) => {
                self.aec_opening_style_manager_width_changed(value)
            }
            AecMessage::AecOpeningStyleManagerHeightChanged(value) => {
                self.aec_opening_style_manager_height_changed(value)
            }
            AecMessage::AecOpeningStyleManagerSillChanged(value) => {
                self.aec.aec_opening_style_manager_sill = value;
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerFrameChanged(value) => {
                self.aec.aec_opening_style_manager_frame = value;
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerAngleChanged(value) => {
                self.aec.aec_opening_style_manager_angle = value;
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerSpringChanged(value) => {
                self.aec.aec_opening_style_manager_spring = value;
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerHingeChanged(value) => {
                self.aec.aec_opening_style_manager_hinge =
                    crate::modules::aec::engine::opening_style::HingeSide::from_str(&value);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerSetPreviewMode(mode) => {
                self.aec.aec_opening_style_manager_preview_mode = mode;
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerSlotSourceChanged(index, value) => {
                self.aec_opening_style_manager_slot_source_changed(index, value)
            }
            AecMessage::AecOpeningStyleManagerSlotGeneratorChanged(index, value) => {
                self.aec_opening_style_manager_slot_generator_changed(index, value)
            }
            AecMessage::AecOpeningStyleManagerSlotBlockNameChanged(index, value) => {
                self.aec_opening_style_manager_slot_block_name_changed(index, value)
            }
            AecMessage::AecOpeningStyleManagerSlotBlockPlacementChanged(index, value) => {
                self.aec_opening_style_manager_slot_block_placement_changed(index, value)
            }
            AecMessage::AecOpeningStyleManagerProfileSelect(value) => {
                self.aec_opening_style_manager_profile_select(value)
            }
            AecMessage::AecOpeningStyleManagerSlotVisible(index, visible) => {
                self.aec_opening_style_manager_slot_visible(index, visible)
            }
            AecMessage::AecOpeningStyleManagerSketchSlotSelect(index) => {
                self.aec_opening_style_manager_sketch_slot_select(index)
            }
            AecMessage::AecOpeningStyleManagerSketchClick(x, y) => {
                self.aec_opening_style_manager_sketch_click(x, y)
            }
            AecMessage::AecOpeningStyleManagerSketchClosePath => {
                self.aec_opening_style_manager_sketch_close_path()
            }
            AecMessage::AecOpeningStyleManagerSketchFinishPath => {
                self.aec_opening_style_manager_sketch_finish_path()
            }
            AecMessage::AecOpeningStyleManagerSketchUndo => {
                self.aec_opening_style_manager_sketch_undo()
            }
            AecMessage::AecOpeningStyleManagerSketchDeletePath => {
                self.aec_opening_style_manager_sketch_delete_path()
            }
            AecMessage::AecOpeningStyleManagerSketchClear => {
                self.aec_opening_style_manager_sketch_clear()
            }
            AecMessage::AecOpeningStyleManagerSketchAddFrame => {
                self.aec_opening_style_manager_sketch_add_frame()
            }
            AecMessage::AecOpeningStyleManagerSketchArc => {
                self.aec_opening_style_manager_sketch_arc()
            }
            AecMessage::AecOpeningStyleManagerSave => self.aec_opening_style_manager_save(),
            AecMessage::AecOpeningStyleManagerSaveAndApply => {
                if let Some((id, _lib)) = self.aec_opening_style_manager_save_internal() {
                    let tab_index = self.active_tab;
                    let mut affected_walls = Vec::new();
                    for entity in self.tabs[tab_index].scene.document.entities() {
                        let h = entity.common().handle;
                        if let Some(opening) =
                            crate::modules::aec::engine::opening_xdata::opening_from_entity(entity, h)
                        {
                            if opening.style_id.as_deref() == Some(id.as_str()) {
                                let wall_handle = crate::modules::aec::engine::wall_package::resolve_wall_package(
                                    &self.tabs[tab_index].scene,
                                    opening.host_wall,
                                );
                                if !wall_handle.is_null() && !affected_walls.contains(&wall_handle) {
                                    affected_walls.push(wall_handle);
                                }
                            }
                        }
                    }
                    let mut updated_count = 0;
                    for wall_handle in affected_walls {
                        if self
                            .regenerate_wall_respecting_active_display_config(tab_index, wall_handle)
                            .is_ok()
                        {
                            updated_count += 1;
                        }
                    }
                    if updated_count > 0 {
                        self.tabs[tab_index].scene.bump_geometry();
                        self.command_line.push_info(
                            crate::tf!(
                                "AEC Style Manager: updated {updated_count} wall(s) / opening(s) using this style."
                            )
                            .as_ref(),
                        );
                    }
                }
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerDelete => self.aec_opening_style_manager_delete(),
            AecMessage::AecStyleManagerCopyOpeningStyleToProject => {
                self.aec_handle_copy_opening_style(true)
            }
            AecMessage::AecStyleManagerCopyOpeningStyleToGlobal => {
                self.aec_handle_copy_opening_style(false)
            }
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
                        .set_title("Open Project")
                        .add_filter("OpenCADStudio Project", &["ocsproj", "OCSPROJ"])
                        .add_filter("All Files", &["*"])
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
                        .set_title("Save Project As")
                        .set_file_name("project.ocsproj")
                        .add_filter("OpenCADStudio Project", &["ocsproj", "OCSPROJ"])
                        .add_filter("All Files", &["*"])
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
                        .set_title("Select Storey Drawing")
                        .add_filter("CAD Files", &["dwg", "dxf", "DWG", "DXF"])
                        .add_filter("All Files", &["*"])
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
                        .set_title("Select Storey Drawing")
                        .add_filter("CAD Files", &["dwg", "dxf", "DWG", "DXF"])
                        .add_filter("All Files", &["*"])
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
            AecMessage::AecStoreySettingsOpen(bid, sid) => {
                self.aec.aec_storey_settings_target = Some((bid, sid));
                self.aec.aec_storey_settings_new_plane_name.clear();
                self.sync_storey_from_active_drawing(bid, sid);
                if let Some(s) = self.aec.aec_project_explorer_file.as_ref().and_then(|p| {
                    p.buildings
                        .iter()
                        .find(|b| b.id == bid)
                        .and_then(|b| b.storeys.iter().find(|st| st.id == sid))
                }) {
                    self.aec.aec_storey_settings_elevation =
                        format!("{:.3}", s.derived_elevation());
                    self.aec.aec_storey_settings_height = format!("{:.3}", s.derived_height());
                    self.aec.aec_storey_settings_new_plane_z = "0.000".to_string();
                    self.aec.aec_storey_settings_plane_z = s
                        .control_planes
                        .iter()
                        .filter(|p| p.id != s.floor_plane_id)
                        .map(|p| {
                            let rel = s.plane_z_relative_to_floor(p.id).unwrap_or(0.0);
                            (p.id, format!("{:.3}", rel))
                        })
                        .collect();
                    let mut facet_z_map = std::collections::HashMap::new();
                    for p in &s.control_planes {
                        for (f_idx, _) in p.facets.iter().enumerate() {
                            let rel = s.facet_z_relative_to_floor(p.id, f_idx).unwrap_or(0.0);
                            facet_z_map.insert((p.id, f_idx), format!("{:.3}", rel));
                        }
                    }
                    self.aec.aec_storey_settings_facet_z = facet_z_map;
                }
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::StoreySettings));
                Task::none()
            }
            AecMessage::AecStoreySettingsClose => {
                self.aec_reset_control_plane_highlights();
                self.aec.aec_storey_settings_target = None;
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::ProjectExplorer));
                Task::none()
            }
            AecMessage::AecStoreySettingsSave(bid, sid) => {
                self.commit_storey_settings_buffers(bid, sid);
                self.aec_project_explorer_persist_if_pathed();
                self.command_line.push_info(crate::t!("AEC Storey Settings: settings saved.").as_ref());
                Task::none()
            }
            AecMessage::AecStoreySettingsSaveAndApply(bid, sid) => {
                self.commit_storey_settings_buffers(bid, sid);
                self.aec_project_explorer_persist_if_pathed();
                self.apply_storey_z_to_active_scene(bid, sid);
                self.tabs[self.active_tab].scene.bump_geometry();
                self.command_line.push_info(
                    crate::t!("AEC Storey Settings: settings applied to active drawing.").as_ref(),
                );
                Task::none()
            }
            AecMessage::AecStoreySettingsNameChanged(bid, sid, name) => {
                self.with_storey_mut(bid, sid, |s| s.name = name);
                self.aec_project_explorer_persist_if_pathed();
                Task::none()
            }
            AecMessage::AecStoreySettingsDrawingChanged(bid, sid, path) => {
                self.with_storey_mut(bid, sid, |s| s.drawing_path = path);
                self.aec_project_explorer_persist_if_pathed();
                Task::none()
            }
            AecMessage::AecStoreySettingsSetFloor(bid, sid, pid) => {
                self.with_storey_mut(bid, sid, |s| {
                    s.floor_plane_id = pid;
                    s.sync_derived_elevation_height();
                });
                if let Some(s) = self.aec.aec_project_explorer_file.as_ref().and_then(|p| {
                    p.buildings
                        .iter()
                        .find(|b| b.id == bid)
                        .and_then(|b| b.storeys.iter().find(|st| st.id == sid))
                }) {
                    self.aec.aec_storey_settings_elevation =
                        format!("{:.3}", s.derived_elevation());
                    self.aec.aec_storey_settings_height = format!("{:.3}", s.derived_height());
                    self.aec.aec_storey_settings_plane_z = s
                        .control_planes
                        .iter()
                        .filter(|p| p.id != s.floor_plane_id)
                        .map(|p| {
                            let rel = s.plane_z_relative_to_floor(p.id).unwrap_or(0.0);
                            (p.id, format!("{:.3}", rel))
                        })
                        .collect();
                    let mut facet_z_map = std::collections::HashMap::new();
                    for p in &s.control_planes {
                        for (f_idx, _) in p.facets.iter().enumerate() {
                            let rel = s.facet_z_relative_to_floor(p.id, f_idx).unwrap_or(0.0);
                            facet_z_map.insert((p.id, f_idx), format!("{:.3}", rel));
                        }
                    }
                    self.aec.aec_storey_settings_facet_z = facet_z_map;
                }
                self.aec_project_explorer_persist_if_pathed();
                Task::none()
            }
            AecMessage::AecStoreySettingsSetCeiling(bid, sid, pid) => {
                self.with_storey_mut(bid, sid, |s| {
                    s.ceiling_plane_id = pid;
                    s.sync_derived_elevation_height();
                });
                self.aec_project_explorer_persist_if_pathed();
                Task::none()
            }
            AecMessage::AecStoreySettingsElevation(bid, sid, text) => {
                self.aec.aec_storey_settings_elevation = text.clone();
                if let Ok(v) = text.trim().parse::<f64>() {
                    self.with_storey_mut(bid, sid, |s| s.set_elevation(v));
                }
                Task::none()
            }
            AecMessage::AecStoreySettingsHeight(bid, sid, text) => {
                self.aec.aec_storey_settings_height = text.clone();
                if let Ok(v) = text.trim().parse::<f64>() {
                    if v > 0.0 {
                        self.with_storey_mut(bid, sid, |s| s.set_height(v));
                    }
                }
                Task::none()
            }
            AecMessage::AecProjectExplorerFfl0NnChanged(text) => {
                self.aec.aec_project_explorer_ffl0_nn = text.clone();
                if let Some(project) = self.aec.aec_project_explorer_file.as_mut() {
                    let t = text.trim();
                    if t.is_empty() {
                        project.ffl0_nn_m = None;
                        self.aec_project_explorer_persist_if_pathed();
                    } else if let Ok(v) = t.parse::<f64>() {
                        project.ffl0_nn_m = Some(v);
                        self.aec_project_explorer_persist_if_pathed();
                    }
                }
                Task::none()
            }
            AecMessage::AecStoreySettingsNewPlaneNameChanged(name) => {
                self.aec.aec_storey_settings_new_plane_name = name;
                Task::none()
            }
            AecMessage::AecStoreySettingsNewPlaneZChanged(z) => {
                self.aec.aec_storey_settings_new_plane_z = z;
                Task::none()
            }
            AecMessage::AecStoreySettingsAddPlane(bid, sid) => {
                let name = self.aec.aec_storey_settings_new_plane_name.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                let rel = self
                    .aec
                    .aec_storey_settings_new_plane_z
                    .trim()
                    .parse::<f64>()
                    .ok();
                let mut added_id = None;
                self.with_storey_mut(bid, sid, |s| {
                    let rel = rel.unwrap_or(0.0);
                    let z = s.derived_elevation() + rel;
                    let plane =
                        crate::modules::aec::engine::control_plane::ControlPlane::horizontal(
                            name, z,
                        );
                    added_id = Some(plane.id);
                    s.add_control_plane(plane);
                });
                if let Some(id) = added_id {
                    let z_txt = if self.aec.aec_storey_settings_new_plane_z.trim().is_empty() {
                        "0.000".to_string()
                    } else {
                        self.aec.aec_storey_settings_new_plane_z.clone()
                    };
                    self.aec.aec_storey_settings_plane_z.insert(id, z_txt);
                }
                self.aec.aec_storey_settings_new_plane_name.clear();
                self.aec_project_explorer_persist_if_pathed();
                Task::none()
            }
            AecMessage::AecStoreySettingsDeletePlane(bid, sid, pid) => {
                self.with_storey_mut(bid, sid, |s| {
                    if pid == s.floor_plane_id || pid == s.ceiling_plane_id {
                        return;
                    }
                    s.control_planes.retain(|p| p.id != pid);
                });
                self.aec_project_explorer_persist_if_pathed();
                Task::none()
            }
            AecMessage::AecStoreySettingsPlaneName(bid, sid, pid, name) => {
                self.with_storey_mut(bid, sid, |s| {
                    if let Some(p) = s.plane_mut(pid) {
                        p.name = name;
                    }
                });
                self.aec_project_explorer_persist_if_pathed();
                Task::none()
            }
            AecMessage::AecStoreySettingsPlaneVisible(bid, sid, pid, visible) => {
                self.with_storey_mut(bid, sid, |s| {
                    if let Some(p) = s.plane_mut(pid) {
                        p.visible = visible;
                    }
                });
                self.aec_project_explorer_persist_if_pathed();
                Task::none()
            }
            AecMessage::AecStoreySettingsPlaneZ(bid, sid, pid, text) => {
                self.aec
                    .aec_storey_settings_plane_z
                    .insert(pid, text.clone());
                if let Ok(v) = text.trim().parse::<f64>() {
                    self.with_storey_mut(bid, sid, |s| {
                        s.set_plane_z_relative_to_floor(pid, v);
                    });
                    if let Some(s) = self.aec.aec_project_explorer_file.as_ref().and_then(|p| {
                        p.buildings
                            .iter()
                            .find(|b| b.id == bid)
                            .and_then(|b| b.storeys.iter().find(|st| st.id == sid))
                    }) {
                        self.aec.aec_storey_settings_elevation =
                            format!("{:.3}", s.derived_elevation());
                        self.aec.aec_storey_settings_height = format!("{:.3}", s.derived_height());
                    }
                    self.aec_project_explorer_persist_if_pathed();
                }
                Task::none()
            }
            AecMessage::AecStoreySettingsPlaneOrigin(bid, sid, pid, axis, text) => {
                if let Ok(v) = text.trim().parse::<f64>() {
                    self.with_storey_mut(bid, sid, |s| {
                        if let Some(p) = s.plane_mut(pid) {
                            if (axis as usize) < 3 {
                                p.origin[axis as usize] = v;
                            }
                        }
                        s.sync_derived_elevation_height();
                    });
                    self.aec_project_explorer_persist_if_pathed();
                }
                Task::none()
            }
            AecMessage::AecStoreySettingsPlaneNormal(bid, sid, pid, axis, text) => {
                if let Ok(v) = text.trim().parse::<f64>() {
                    self.with_storey_mut(bid, sid, |s| {
                        if let Some(p) = s.plane_mut(pid) {
                            if (axis as usize) < 3 {
                                p.normal[axis as usize] = v;
                            }
                        }
                        s.sync_derived_elevation_height();
                    });
                    self.aec_project_explorer_persist_if_pathed();
                }
                Task::none()
            }
            AecMessage::AecStoreySettingsPickPolygons(bid, sid, pid) => {
                self.commit_storey_settings_buffers(bid, sid);
                self.aec_project_explorer_persist_if_pathed();
                self.active_modal = None;
                let cmd = crate::modules::aec::project::plane_assign::PlaneAssignCommand::with_target(bid, sid, pid);
                self.command_line.push_info(&cmd.prompt());
                let tab = self.active_tab;
                self.tabs[tab].active_cmd = Some(Box::new(cmd));
                Task::none()
            }
            AecMessage::AecStoreySettingsShowPlaneInDrawing(bid, sid, pid) => {
                self.commit_storey_settings_buffers(bid, sid);
                self.aec_project_explorer_persist_if_pathed();
                self.aec_reset_control_plane_highlights();
                let tab = self.active_tab;
                let mut plane_name = String::new();
                if let Some(storey) = self.with_storey(bid, sid, |s| s.clone()) {
                    if let Some(p) = storey.plane(pid) {
                        plane_name = p.name.clone();
                    }
                    let touched = crate::modules::aec::project::preview::highlight_control_plane(
                        &mut self.tabs[tab].scene,
                        &storey,
                        pid,
                    );
                    let changes: Vec<_> = touched
                        .into_iter()
                        .map(|h| (h, crate::scene::ChangeKind::Modified))
                        .collect();
                    if !changes.is_empty() {
                        self.tabs[tab].scene.bump_entities(&changes);
                    }
                }
                self.command_line.push_info(&crate::tr!(
                    "aec",
                    "plane-highlighted-in-drawing",
                    name = plane_name.as_str()
                ));
                self.aec.aec_control_plane_highlight_epoch = self.aec.aec_control_plane_highlight_epoch.wrapping_add(1);
                let epoch = self.aec.aec_control_plane_highlight_epoch;
                Task::perform(
                    async move {
                        std::thread::sleep(std::time::Duration::from_millis(2000));
                        epoch
                    },
                    |e| Message::Aec(AecMessage::AecResetControlPlaneHighlights(e)),
                )
            }
            AecMessage::AecStoreySettingsFacetName(bid, sid, pid, f_idx, name) => {
                self.with_storey_mut(bid, sid, |s| {
                    if let Some(p) = s.plane_mut(pid) {
                        if let Some(f) = p.facets.get_mut(f_idx) {
                            f.name = name;
                        }
                    }
                });
                self.aec_project_explorer_persist_if_pathed();
                Task::none()
            }
            AecMessage::AecStoreySettingsFacetZ(bid, sid, pid, f_idx, text) => {
                self.aec.aec_storey_settings_facet_z.insert((pid, f_idx), text.clone());
                if let Ok(v) = text.trim().parse::<f64>() {
                    self.with_storey_mut(bid, sid, |s| {
                        s.set_facet_z_relative_to_floor(pid, f_idx, v);
                    });
                    self.aec_project_explorer_persist_if_pathed();
                }
                Task::none()
            }
            AecMessage::AecStoreySettingsDeleteFacet(bid, sid, pid, f_idx) => {
                let tab = self.active_tab;
                let mut removed_handle = None;
                self.with_storey_mut(bid, sid, |s| {
                    if let Some(f) = s.delete_facet(pid, f_idx) {
                        removed_handle = f.preview_handle;
                    }
                });
                if let Some(h) = removed_handle {
                    self.tabs[tab].scene.erase_entities(&[acadrust::Handle::new(h)]);
                }
                self.aec.aec_storey_settings_facet_z.remove(&(pid, f_idx));
                self.aec_project_explorer_persist_if_pathed();
                Task::none()
            }
            AecMessage::AecStoreySettingsShowFacetInDrawing(bid, sid, pid, f_idx) => {
                self.commit_storey_settings_buffers(bid, sid);
                self.aec_project_explorer_persist_if_pathed();
                self.aec_reset_control_plane_highlights();
                let tab = self.active_tab;
                let mut facet_name = String::new();
                if let Some(storey) = self.with_storey(bid, sid, |s| s.clone()) {
                    if let Some(p) = storey.plane(pid) {
                        if let Some(f) = p.facets.get(f_idx) {
                            facet_name = f.name.clone();
                        }
                    }
                    let touched = crate::modules::aec::project::preview::highlight_control_plane_facet(
                        &mut self.tabs[tab].scene,
                        &storey,
                        pid,
                        f_idx,
                    );
                    let changes: Vec<_> = touched
                        .into_iter()
                        .map(|h| (h, crate::scene::ChangeKind::Modified))
                        .collect();
                    if !changes.is_empty() {
                        self.tabs[tab].scene.bump_entities(&changes);
                    }
                }
                self.command_line.push_info(&crate::tr!(
                    "aec",
                    "plane-highlighted-in-drawing",
                    name = facet_name.as_str()
                ));
                self.aec.aec_control_plane_highlight_epoch = self.aec.aec_control_plane_highlight_epoch.wrapping_add(1);
                let epoch = self.aec.aec_control_plane_highlight_epoch;
                Task::perform(
                    async move {
                        std::thread::sleep(std::time::Duration::from_millis(2000));
                        epoch
                    },
                    |e| Message::Aec(AecMessage::AecResetControlPlaneHighlights(e)),
                )
            }
            AecMessage::AecResetControlPlaneHighlights(epoch) => {
                if epoch == self.aec.aec_control_plane_highlight_epoch {
                    self.aec_reset_control_plane_highlights();
                }
                Task::none()
            }
            AecMessage::AecPlanManagerOpen => {
                self.ribbon.close_dropdown();
                self.aec.aec_plan_library = Some(
                    crate::modules::aec::engine::project::resolve_display_config_library(
                        self.aec.aec_project_explorer_file.as_ref(),
                    ),
                );
                // Layer-Filter-UI: the multi-select checklist needs the
                // full set of wall styles/layers currently in effect, the
                // same "project overrides global" resolution the
                // Material/WallStyle managers already use.
                self.aec.aec_style_library = Some(
                    crate::modules::aec::engine::project::resolve_style_library(
                        self.aec.aec_project_explorer_file.as_ref(),
                    ),
                );
                self.aec.aec_plan_manager_filter.clear();
                self.aec.aec_plan_manager_selected = None;
                self.aec.aec_plan_manager_editing_name = None;
                self.aec.aec_plan_manager_form_open = false;
                self.refresh_aec_material_linetype_combo();
                self.aec.aec_plan_manager_wall_styles = self
                    .aec.aec_style_library
                    .as_ref()
                    .map(|lib| {
                        lib.wall_styles
                            .iter()
                            .map(|ws| {
                                let layers = ws
                                    .layers
                                    .iter()
                                    .enumerate()
                                    .map(|(i, l)| {
                                        let mat_name = lib
                                            .materials
                                            .iter()
                                            .find(|m| m.id == l.material_id)
                                            .map(|m| m.name.as_str())
                                            .unwrap_or(l.material_id.as_str());
                                        let label = format!("{} — {}", i + 1, mat_name);
                                        (l.layer_id, label)
                                    })
                                    .collect();
                                (ws.style.id.clone(), ws.style.name.clone(), layers)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::PlanManager));
                Task::none()
            }
            AecMessage::AecPlanManagerClose => {
                self.active_modal = None;
                Task::none()
            }
            AecMessage::AecPlanManagerFilter(value) => {
                self.aec.aec_plan_manager_filter = value;
                Task::none()
            }
            AecMessage::AecPlanManagerSelect(name) => {
                if let Some(cfg) = self
                    .aec.aec_plan_library
                    .as_ref()
                    .and_then(|lib| lib.find(&name))
                    .cloned()
                {
                    self.aec.aec_plan_manager_editing_name = Some(cfg.name.clone());
                    self.aec.aec_plan_manager_name = cfg.name.clone();
                    self.aec.aec_plan_manager_discipline = cfg.discipline.clone();
                    self.aec.aec_plan_manager_scale =
                        cfg.scale.map(|s| s.to_string()).unwrap_or_default();
                    self.aec.aec_plan_manager_planning_stage = cfg.planning_stage;
                    self.aec.aec_plan_manager_view_type = cfg.view_type.clone();
                    self.aec_plan_manager_load_phase_filter_buffers(cfg.phase_filter.as_ref());
                    self.aec_plan_manager_load_display_buffers(&cfg);
                    self.aec.aec_plan_manager_form_open = true;
                }
                self.aec.aec_plan_manager_selected = Some(name);
                Task::none()
            }
            AecMessage::AecPlanManagerNew => {
                self.aec.aec_plan_manager_selected = None;
                self.aec.aec_plan_manager_editing_name = None;
                self.aec.aec_plan_manager_name.clear();
                self.aec.aec_plan_manager_discipline.clear();
                self.aec.aec_plan_manager_scale.clear();
                self.aec.aec_plan_manager_planning_stage =
                    crate::modules::aec::engine::plan_view::PlanningStage::Design;
                self.aec.aec_plan_manager_view_type =
                    crate::modules::aec::engine::plan_view::ViewType::FloorPlan;
                self.aec_plan_manager_load_phase_filter_buffers(None);
                self.aec_plan_manager_reset_display_buffers();
                self.aec.aec_plan_manager_form_open = true;
                Task::none()
            }
            AecMessage::AecPlanManagerDuplicate => {
                let source_name = self
                    .aec.aec_plan_manager_editing_name
                    .clone()
                    .or_else(|| self.aec.aec_plan_manager_selected.clone());
                let Some(source_name) = source_name else {
                    return Task::none();
                };
                let Some(cfg) = self
                    .aec.aec_plan_library
                    .as_ref()
                    .and_then(|lib| lib.find(&source_name))
                    .cloned()
                else {
                    return Task::none();
                };
                let existing_names: Vec<String> = self
                    .aec.aec_plan_library
                    .as_ref()
                    .map(|lib| lib.configs.iter().map(|c| c.name.clone()).collect())
                    .unwrap_or_default();
                let base_name = format!("{} (Kopie)", cfg.name);
                let mut name = base_name.clone();
                let mut counter = 2;
                while existing_names.iter().any(|n| n == &name) {
                    name = format!("{base_name} {counter}");
                    counter += 1;
                }
                self.aec.aec_plan_manager_selected = None;
                self.aec.aec_plan_manager_editing_name = None;
                self.aec.aec_plan_manager_name = name;
                self.aec.aec_plan_manager_discipline = cfg.discipline.clone();
                self.aec.aec_plan_manager_scale = cfg.scale.map(|s| s.to_string()).unwrap_or_default();
                self.aec_plan_manager_load_phase_filter_buffers(cfg.phase_filter.as_ref());
                self.aec.aec_plan_manager_planning_stage = cfg.planning_stage;
                self.aec.aec_plan_manager_view_type = cfg.view_type.clone();
                self.aec_plan_manager_load_display_buffers(&cfg);
                self.aec.aec_plan_manager_editing_id = None;
                self.aec.aec_plan_manager_form_open = true;
                Task::none()
            }
            AecMessage::AecPlanManagerDelete => {
                if let Some(name) = self.aec.aec_plan_manager_selected.clone() {
                    let lib_snapshot = if let Some(lib) = self.aec.aec_plan_library.as_mut() {
                        lib.remove(&name);
                        Some(lib.clone())
                    } else {
                        None
                    };
                    if let Some(lib_snapshot) = lib_snapshot {
                        match self.aec_save_display_config_library_preferring_project(&lib_snapshot) {
                            Ok(()) => self.command_line.push_info(
                                crate::t!("AEC DisplayConfig Manager: config deleted.").as_ref(),
                            ),
                            Err(e) => self.command_line.push_error(
                                crate::tf!("AEC DisplayConfig Manager: failed to save library: {e}")
                                    .as_ref(),
                            ),
                        }
                    }
                }
                self.aec.aec_plan_manager_selected = None;
                self.aec.aec_plan_manager_editing_name = None;
                self.aec.aec_plan_manager_form_open = false;
                Task::none()
            }
            AecMessage::AecPlanManagerNameChanged(value) => {
                self.aec.aec_plan_manager_name = value;
                Task::none()
            }
            AecMessage::AecPlanManagerDisciplineChanged(value) => {
                self.aec.aec_plan_manager_discipline = value;
                Task::none()
            }
            AecMessage::AecPlanManagerScaleChanged(value) => {
                self.aec.aec_plan_manager_scale = value;
                Task::none()
            }
            AecMessage::AecPlanManagerPlanningStageChanged(stage) => {
                self.aec.aec_plan_manager_planning_stage = stage;
                Task::none()
            }
            AecMessage::AecPlanManagerViewTypeChanged(view_type) => {
                self.aec.aec_plan_manager_view_type = view_type;
                Task::none()
            }
            AecMessage::AecPlanManagerPhaseVisibleToggle(phase, visible) => {
                match phase {
                    crate::modules::aec::engine::plan_view::PlanPhase::Existing => {
                        self.aec.aec_plan_manager_phase_filter_visible_existing = visible;
                    }
                    crate::modules::aec::engine::plan_view::PlanPhase::Demolition => {
                        self.aec.aec_plan_manager_phase_filter_visible_demolition = visible;
                    }
                    crate::modules::aec::engine::plan_view::PlanPhase::New => {
                        self.aec.aec_plan_manager_phase_filter_visible_new = visible;
                    }
                }
                Task::none()
            }
            AecMessage::AecPlanManagerDemolitionStyleLineTypeChanged(value) => {
                self.aec.aec_plan_manager_demolition_style_line_type = value;
                Task::none()
            }
            AecMessage::AecPlanManagerDemolitionStyleLineColorChanged(value) => {
                self.aec.aec_plan_manager_demolition_style_line_color = value;
                Task::none()
            }
            AecMessage::AecPlanManagerDemolitionStyleHatchPatternChanged(value) => {
                self.aec.aec_plan_manager_demolition_style_hatch_pattern = value;
                self.aec.aec_plan_manager_demolition_style_hatch_picker_open = false;
                Task::none()
            }
            AecMessage::AecPlanManagerDemolitionStyleHatchPickerToggle => {
                self.aec.aec_plan_manager_demolition_style_hatch_picker_open =
                    !self.aec.aec_plan_manager_demolition_style_hatch_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerDemolitionStyleHatchColorChanged(value) => {
                self.aec.aec_plan_manager_demolition_style_hatch_color = value;
                Task::none()
            }
            AecMessage::AecPlanManagerDemolitionStyleFillColorChanged(value) => {
                self.aec.aec_plan_manager_demolition_style_fill_color = value;
                Task::none()
            }
            AecMessage::AecPlanManagerDemolitionStyleLineColorPickerToggle => {
                self.aec.aec_plan_manager_demolition_style_line_color_picker_open =
                    !self.aec.aec_plan_manager_demolition_style_line_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerDemolitionStyleHatchColorPickerToggle => {
                self.aec.aec_plan_manager_demolition_style_hatch_color_picker_open =
                    !self.aec.aec_plan_manager_demolition_style_hatch_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerDemolitionStyleFillColorPickerToggle => {
                self.aec.aec_plan_manager_demolition_style_fill_color_picker_open =
                    !self.aec.aec_plan_manager_demolition_style_fill_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerExistingStyleLineTypeChanged(value) => {
                self.aec.aec_plan_manager_existing_style_line_type = value;
                Task::none()
            }
            AecMessage::AecPlanManagerExistingStyleLineColorChanged(value) => {
                self.aec.aec_plan_manager_existing_style_line_color = value;
                Task::none()
            }
            AecMessage::AecPlanManagerExistingStyleHatchPatternChanged(value) => {
                self.aec.aec_plan_manager_existing_style_hatch_pattern = value;
                self.aec.aec_plan_manager_existing_style_hatch_picker_open = false;
                Task::none()
            }
            AecMessage::AecPlanManagerExistingStyleHatchPickerToggle => {
                self.aec.aec_plan_manager_existing_style_hatch_picker_open =
                    !self.aec.aec_plan_manager_existing_style_hatch_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerExistingStyleHatchColorChanged(value) => {
                self.aec.aec_plan_manager_existing_style_hatch_color = value;
                Task::none()
            }
            AecMessage::AecPlanManagerExistingStyleFillColorChanged(value) => {
                self.aec.aec_plan_manager_existing_style_fill_color = value;
                Task::none()
            }
            AecMessage::AecPlanManagerExistingStyleLineColorPickerToggle => {
                self.aec.aec_plan_manager_existing_style_line_color_picker_open =
                    !self.aec.aec_plan_manager_existing_style_line_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerExistingStyleHatchColorPickerToggle => {
                self.aec.aec_plan_manager_existing_style_hatch_color_picker_open =
                    !self.aec.aec_plan_manager_existing_style_hatch_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerExistingStyleFillColorPickerToggle => {
                self.aec.aec_plan_manager_existing_style_fill_color_picker_open =
                    !self.aec.aec_plan_manager_existing_style_fill_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerRepresentationChanged(mode) => {
                self.aec.aec_plan_manager_default_representation = mode;
                Task::none()
            }
            AecMessage::AecPlanManagerComponentVisibleToggle(kind, visible) => {
                if visible {
                    self.aec.aec_plan_manager_component_visibility.remove(&kind);
                } else {
                    self.aec.aec_plan_manager_component_visibility.insert(kind, false);
                }
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayStyleSelect(style_id) => {
                self.aec_plan_manager_write_overlay_buffers();
                self.aec_plan_manager_write_overlay_contour_hatch();
                self.aec.aec_plan_manager_overlay_style_id = Some(style_id);
                self.aec.aec_plan_manager_overlay_layer_id = self
                    .aec.aec_plan_manager_overlay_style_id
                    .as_ref()
                    .and_then(|sid| {
                        self.aec.aec_plan_manager_style_overlays
                            .get(sid)
                            .and_then(|o| o.layer_props.keys().next().copied())
                    });
                self.aec_plan_manager_load_overlay_layer_buffers();
                self.aec_plan_manager_load_overlay_contour_hatch_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayLayerSelect(layer_id) => {
                self.aec_plan_manager_write_overlay_buffers();
                self.aec.aec_plan_manager_overlay_layer_id = Some(layer_id);
                self.aec_plan_manager_load_overlay_layer_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayAddStyle(style_id) => {
                self.aec_plan_manager_write_overlay_buffers();
                self.aec_plan_manager_write_overlay_contour_hatch();
                self.aec.aec_plan_manager_style_overlays
                    .entry(style_id.clone())
                    .or_default();
                self.aec.aec_plan_manager_overlay_style_id = Some(style_id);
                self.aec.aec_plan_manager_overlay_layer_id = None;
                self.aec_plan_manager_clear_overlay_field_buffers();
                self.aec_plan_manager_load_overlay_contour_hatch_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayRemoveStyle => {
                if let Some(style_id) = self.aec.aec_plan_manager_overlay_style_id.take() {
                    self.aec.aec_plan_manager_style_overlays.remove(&style_id);
                }
                self.aec.aec_plan_manager_overlay_layer_id = None;
                self.aec.aec_plan_manager_overlay_style_id =
                    self.aec.aec_plan_manager_style_overlays.keys().next().cloned();
                self.aec_plan_manager_load_overlay_layer_buffers();
                self.aec_plan_manager_load_overlay_contour_hatch_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayLineTypeChanged(value) => {
                self.aec.aec_plan_manager_overlay_line_type = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayLineColorChanged(value) => {
                self.aec.aec_plan_manager_overlay_line_color = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayHatchPatternChanged(value) => {
                self.aec.aec_plan_manager_overlay_hatch_pattern = value;
                self.aec.aec_plan_manager_overlay_hatch_picker_open = false;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayHatchColorChanged(value) => {
                self.aec.aec_plan_manager_overlay_hatch_color = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayHatchScaleChanged(value) => {
                self.aec.aec_plan_manager_overlay_hatch_scale = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayHatchAngleChanged(value) => {
                self.aec.aec_plan_manager_overlay_hatch_angle = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayHatchAngleRelativeChanged(value) => {
                self.aec.aec_plan_manager_overlay_hatch_angle_relative = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayFillColorChanged(value) => {
                self.aec.aec_plan_manager_overlay_fill_color = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayLineColorPickerToggle => {
                self.aec.aec_plan_manager_overlay_line_color_picker_open =
                    !self.aec.aec_plan_manager_overlay_line_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayHatchPickerToggle => {
                self.aec.aec_plan_manager_overlay_hatch_picker_open =
                    !self.aec.aec_plan_manager_overlay_hatch_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayHatchColorPickerToggle => {
                self.aec.aec_plan_manager_overlay_hatch_color_picker_open =
                    !self.aec.aec_plan_manager_overlay_hatch_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayFillColorPickerToggle => {
                self.aec.aec_plan_manager_overlay_fill_color_picker_open =
                    !self.aec.aec_plan_manager_overlay_fill_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerContourHatchPatternChanged(value) => {
                self.aec.aec_plan_manager_contour_hatch_pattern = value;
                self.aec.aec_plan_manager_contour_hatch_picker_open = false;
                self.aec_plan_manager_write_overlay_contour_hatch();
                Task::none()
            }
            AecMessage::AecPlanManagerContourHatchColorChanged(value) => {
                self.aec.aec_plan_manager_contour_hatch_color = value;
                self.aec_plan_manager_write_overlay_contour_hatch();
                Task::none()
            }
            AecMessage::AecPlanManagerContourHatchScaleChanged(value) => {
                self.aec.aec_plan_manager_contour_hatch_scale = value;
                self.aec_plan_manager_write_overlay_contour_hatch();
                Task::none()
            }
            AecMessage::AecPlanManagerContourHatchAngleChanged(value) => {
                self.aec.aec_plan_manager_contour_hatch_angle = value;
                self.aec_plan_manager_write_overlay_contour_hatch();
                Task::none()
            }
            AecMessage::AecPlanManagerContourHatchAngleRelativeChanged(value) => {
                self.aec.aec_plan_manager_contour_hatch_angle_relative = value;
                self.aec_plan_manager_write_overlay_contour_hatch();
                Task::none()
            }
            AecMessage::AecPlanManagerContourHatchPickerToggle => {
                self.aec.aec_plan_manager_contour_hatch_picker_open =
                    !self.aec.aec_plan_manager_contour_hatch_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerContourHatchColorPickerToggle => {
                self.aec.aec_plan_manager_contour_hatch_color_picker_open =
                    !self.aec.aec_plan_manager_contour_hatch_color_picker_open;
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayLayerVis2d(visible) => {
                if let (Some(style_id), Some(layer_id)) = (
                    self.aec.aec_plan_manager_overlay_style_id.clone(),
                    self.aec.aec_plan_manager_overlay_layer_id,
                ) {
                    let overlay = self
                        .aec.aec_plan_manager_style_overlays
                        .entry(style_id)
                        .or_default();
                    let mut vis = overlay
                        .layer_visibility
                        .get(&layer_id)
                        .copied()
                        .unwrap_or_default();
                    vis.visible_2d = visible;
                    if vis.visible_2d && vis.visible_3d {
                        overlay.layer_visibility.remove(&layer_id);
                    } else {
                        overlay.layer_visibility.insert(layer_id, vis);
                    }
                }
                Task::none()
            }
            AecMessage::AecPlanManagerOverlayLayerVis3d(visible) => {
                if let (Some(style_id), Some(layer_id)) = (
                    self.aec.aec_plan_manager_overlay_style_id.clone(),
                    self.aec.aec_plan_manager_overlay_layer_id,
                ) {
                    let overlay = self
                        .aec.aec_plan_manager_style_overlays
                        .entry(style_id)
                        .or_default();
                    let mut vis = overlay
                        .layer_visibility
                        .get(&layer_id)
                        .copied()
                        .unwrap_or_default();
                    vis.visible_3d = visible;
                    if vis.visible_2d && vis.visible_3d {
                        overlay.layer_visibility.remove(&layer_id);
                    } else {
                        overlay.layer_visibility.insert(layer_id, vis);
                    }
                }
                Task::none()
            }
            AecMessage::AecPlanManagerSave => {
                self.aec_plan_manager_save_internal();
                Task::none()
            }
            AecMessage::AecPlanManagerApply => {
                if let Some(name) = self.aec_plan_manager_save_internal() {
                    let i = self.active_tab;
                    if !self.tabs[i].is_start
                        && self.tabs[i].active_display_config.as_deref() == Some(name.as_str())
                    {
                        self.apply_active_display_config_to_tab(i);
                    }
                }
                Task::none()
            }
            AecMessage::AecActiveDisplayConfigSelected(name) => {
                let i = self.active_tab;
                if self.tabs[i].is_start {
                    return Task::none();
                }
                self.tabs[i].active_display_config = name.clone();
                self.apply_active_display_config_to_tab(i);
                Task::none()
            }
            AecMessage::AecRepresentationOverrideSelected(mode) => {
                let i = self.active_tab;
                if self.tabs[i].is_start {
                    return Task::none();
                }
                self.tabs[i].representation_override = mode;
                self.apply_active_display_config_to_tab(i);
                Task::none()
            }
            AecMessage::AecStyleManagerFilter(value) => {
                self.aec.aec_style_manager_filter = value;
                Task::none()
            }
            AecMessage::AecStyleManagerSelectMaterial(id) => {
                let material = crate::modules::aec::engine::library::combined_material_entries(
                    self.aec.aec_project_explorer_file.as_ref(),
                )
                .into_iter()
                .find(|e| e.material.id == id)
                .map(|e| e.material)
                .or_else(|| {
                    self.aec.aec_style_library
                        .as_ref()
                        .and_then(|lib| lib.materials.iter().find(|m| m.id == id).cloned())
                });
                if let Some(material) = material {
                    self.aec.aec_style_manager_material_editing_id = Some(material.id.clone());
                    self.aec.aec_style_manager_material_name = material.name.clone();
                    self.aec.aec_style_manager_material_hatch = material.hatch_pattern.clone();
                    self.aec.aec_style_manager_material_color =
                        format!("#{:06X}", material.line_color);
                    self.aec.aec_style_manager_material_line_type = material.line_type.clone();
                    self.aec.aec_style_manager_material_category =
                        material.category.clone().unwrap_or_default();
                                        self.aec.aec_style_manager_material_hatch_color = material
                        .hatch_color
                        .and_then(|c| c.rgb())
                        .map(|(r, g, b)| ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
                        .unwrap_or(material.line_color);
                    self.aec.aec_style_manager_material_hatch_scale =
                        format!("{}", material.hatch_scale);
                    self.aec.aec_style_manager_material_render_ref =
                        material.render_material_ref.clone().unwrap_or_default();
                    self.aec.aec_style_manager_material_hatch_angle =
                        format!("{}", material.hatch_angle);
                    self.aec.aec_style_manager_material_hatch_angle_relative =
                        material.hatch_angle_relative;
                    self.aec.aec_style_manager_material_hatch_color_picker_open = false;
                    self.aec.aec_style_manager_material_form_open = true;
                }
                self.aec.aec_style_manager_selected_material = Some(id);
                self.aec.aec_style_manager_selected_wall_style = None;
                self.refresh_aec_material_linetype_combo();
                Task::none()
            }
            AecMessage::AecStyleManagerSelectWallStyle(id) => {
                let wall_style = crate::modules::aec::engine::library::combined_wall_style_entries(
                    self.aec.aec_project_explorer_file.as_ref(),
                )
                .into_iter()
                .find(|e| e.wall_style.style.id == id)
                .map(|e| e.wall_style)
                .or_else(|| {
                    self.aec.aec_style_library.as_ref().and_then(|lib| {
                        lib.wall_styles
                            .iter()
                            .find(|w| w.style.id == id)
                            .cloned()
                    })
                });
                if let Some(wall_style) = wall_style {
                    self.aec.aec_style_manager_wall_style_editing_id = Some(wall_style.style.id.clone());
                    self.aec.aec_style_manager_wall_style_name = wall_style.style.name.clone();
                    self.aec.aec_style_manager_wall_style_parent =
                        wall_style.style.parent_style_id.clone();
                    self.aec.aec_style_manager_wall_style_layers = wall_style
                        .layers
                        .iter()
                        .map(|l| crate::app::AecLayerBuffer {
                            material_id: l.material_id.clone(),
                            // Display in centimeters; fixed numbers and
                            // formula strings alike.
                            thickness: l.thickness.to_cm_display_string(),
                            function: match &l.function {
                                crate::modules::aec::engine::wall_style::LayerFunction::Structural => {
                                    "Structural".to_string()
                                }
                                crate::modules::aec::engine::wall_style::LayerFunction::Insulation => {
                                    "Insulation".to_string()
                                }
                                crate::modules::aec::engine::wall_style::LayerFunction::Finish => {
                                    "Finish".to_string()
                                }
                                crate::modules::aec::engine::wall_style::LayerFunction::Other(s) => s.clone(),
                            },
                            axis_offset: l.axis_offset.to_cm_display_string(),
                            bottom_offset: crate::modules::aec::engine::wall_style::LayerValue::format_cm(
                                l.bottom_offset * 100.0,
                            ),
                            top_offset: crate::modules::aec::engine::wall_style::LayerValue::format_cm(
                                l.top_offset * 100.0,
                            ),
                            layer_override: l.layer_override.clone().unwrap_or_default(),
                            hatch_override: l.hatch_override.clone().unwrap_or_default(),
                            role_tag: l.role_tag.clone().unwrap_or_default(),
                            layer_id: Some(l.layer_id),
                        })
                        .collect();
                    self.aec.aec_style_manager_wall_style_form_open = true;
                }
                self.aec.aec_style_manager_selected_wall_style = Some(id);
                self.aec.aec_style_manager_selected_material = None;
                self.aec.aec_style_manager_material_form_open = false;
                self.aec.aec_style_manager_material_editing_id = None;
                self.aec.aec_style_manager_profile_selected = None;
                self.aec.aec_style_manager_profile_contour_explicit = false;
                self.aec.aec_style_manager_profile_contour_selection.clear();
                self.aec.aec_style_manager_profile_solid_explicit = false;
                self.aec.aec_style_manager_profile_solid_selection.clear();
                self.aec.aec_style_manager_profile_hatch_angle.clear();
                self.aec.aec_style_manager_profile_hatch_relative = false;
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleNew => {
                self.aec.aec_style_manager_selected_material = None;
                self.aec.aec_style_manager_selected_wall_style = None;
                self.aec.aec_style_manager_wall_style_editing_id = None;
                self.aec.aec_style_manager_wall_style_name.clear();
                self.aec.aec_style_manager_wall_style_parent = None;
                self.aec.aec_style_manager_wall_style_layers.clear();
                self.aec.aec_style_manager_wall_style_form_open = true;
                self.aec.aec_style_manager_material_form_open = false;
                self.aec.aec_style_manager_material_editing_id = None;
                self.aec.aec_style_manager_profile_selected = None;
                self.aec.aec_style_manager_profile_contour_explicit = false;
                self.aec.aec_style_manager_profile_contour_selection.clear();
                self.aec.aec_style_manager_profile_solid_explicit = false;
                self.aec.aec_style_manager_profile_solid_selection.clear();
                self.aec.aec_style_manager_profile_hatch_angle.clear();
                self.aec.aec_style_manager_profile_hatch_relative = false;
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleNameChanged(value) => {
                self.aec.aec_style_manager_wall_style_name = value;
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleParentChanged(value) => {
                self.aec.aec_style_manager_wall_style_parent = value;
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerAdd => {
                let material_id = self
                    .aec.aec_style_library
                    .as_ref()
                    .and_then(|lib| lib.materials.first())
                    .map(|m| m.id.clone())
                    .unwrap_or_default();
                self.aec.aec_style_manager_wall_style_layers
                    .push(crate::app::AecLayerBuffer {
                        material_id,
                        thickness: "10".to_string(),
                        function: "Structural".to_string(),
                        axis_offset: "0".to_string(),
                        bottom_offset: "0".to_string(),
                        top_offset: "0".to_string(),
                        layer_override: String::new(),
                        hatch_override: String::new(),
                        role_tag: String::new(),
                        layer_id: Some(uuid::Uuid::new_v4()),
                    });
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerRemove(index) => {
                if index < self.aec.aec_style_manager_wall_style_layers.len() {
                    self.aec.aec_style_manager_wall_style_layers.remove(index);
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerMaterialChanged(index, material_id) => {
                if let Some(layer) = self.aec.aec_style_manager_wall_style_layers.get_mut(index) {
                    layer.material_id = material_id;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerThicknessChanged(index, thickness) => {
                if let Some(layer) = self.aec.aec_style_manager_wall_style_layers.get_mut(index) {
                    layer.thickness = thickness;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerFunctionChanged(index, function) => {
                if let Some(layer) = self.aec.aec_style_manager_wall_style_layers.get_mut(index) {
                    layer.function = function;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerAxisOffsetChanged(index, offset) => {
                if let Some(layer) = self.aec.aec_style_manager_wall_style_layers.get_mut(index) {
                    layer.axis_offset = offset;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerBottomOffsetChanged(index, offset) => {
                if let Some(layer) = self.aec.aec_style_manager_wall_style_layers.get_mut(index) {
                    layer.bottom_offset = offset;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerTopOffsetChanged(index, offset) => {
                if let Some(layer) = self.aec.aec_style_manager_wall_style_layers.get_mut(index) {
                    layer.top_offset = offset;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerOverrideChanged(index, layer_name) => {
                if let Some(layer) = self.aec.aec_style_manager_wall_style_layers.get_mut(index) {
                    layer.layer_override = layer_name;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerHatchOverrideChanged(index, hatch) => {
                if let Some(layer) = self.aec.aec_style_manager_wall_style_layers.get_mut(index) {
                    layer.hatch_override = hatch;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerRoleTagChanged(index, role_tag) => {
                if let Some(layer) = self.aec.aec_style_manager_wall_style_layers.get_mut(index) {
                    layer.role_tag = role_tag;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerMoveUp(index) => {
                if index > 0 && index < self.aec.aec_style_manager_wall_style_layers.len() {
                    self.aec.aec_style_manager_wall_style_layers.swap(index - 1, index);
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerMoveDown(index) => {
                if index + 1 < self.aec.aec_style_manager_wall_style_layers.len() {
                    self.aec.aec_style_manager_wall_style_layers.swap(index, index + 1);
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerDragStart(index) => {
                // Click-based "pick up / drop here" reordering: arms the
                // dragged row so a later drag-over on another row swaps it
                // into place. Clicking the already-armed row's handle again
                // cancels the drag (matches the toggle feel of `PaneMoveStart`).
                if self.aec.aec_style_manager_wall_style_drag_index == Some(index) {
                    self.aec.aec_style_manager_wall_style_drag_index = None;
                } else if index < self.aec.aec_style_manager_wall_style_layers.len() {
                    self.aec.aec_style_manager_wall_style_drag_index = Some(index);
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerDragOver(target) => {
                if let Some(from) = self.aec.aec_style_manager_wall_style_drag_index.take() {
                    let len = self.aec.aec_style_manager_wall_style_layers.len();
                    if from != target && from < len && target < len {
                        let layer = self.aec.aec_style_manager_wall_style_layers.remove(from);
                        self.aec.aec_style_manager_wall_style_layers.insert(target, layer);
                    }
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerDragEnd => {
                self.aec.aec_style_manager_wall_style_drag_index = None;
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleSortToggle => {
                self.aec.aec_style_manager_wall_style_sort =
                    self.aec.aec_style_manager_wall_style_sort.toggled();
                Task::none()
            }
            AecMessage::AecStylePickerOpen(target) => {
                self.aec_refresh_combined_style_library();
                self.aec.aec_style_picker_filter.clear();
                // Pre-select/highlight whatever is already assigned for this
                // target so the picker doesn't reopen with nothing
                // highlighted even though a value is already in use.
                self.aec.aec_style_picker_selection = match target {
                    crate::app::StylePickerTarget::WallStyleParent => {
                        self.aec.aec_style_manager_wall_style_parent.clone()
                    }
                    crate::app::StylePickerTarget::LayerMaterial(index) => self
                        .aec.aec_style_manager_wall_style_layers
                        .get(index)
                        .map(|l| l.material_id.clone()),
                    crate::app::StylePickerTarget::LayerOverride(index) => self
                        .aec.aec_style_manager_wall_style_layers
                        .get(index)
                        .map(|l| l.layer_override.clone()),
                    crate::app::StylePickerTarget::WallPropertiesStyle
                    | crate::app::StylePickerTarget::ActiveCommand
                    | crate::app::StylePickerTarget::OpeningPropertiesStyle => None,
                    crate::app::StylePickerTarget::OpeningStyleParent => {
                        self.aec.aec_opening_style_manager_parent.clone()
                    }
                };
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker { target }));
                Task::none()
            }
            AecMessage::AecStylePickerOpenForWallProperties(handles) => {
                self.aec_refresh_combined_style_library();
                self.aec.aec_style_picker_filter.clear();
                // Pre-select/highlight the style already assigned to the
                // (first) wall being edited, so the picker doesn't reopen
                // with nothing highlighted even though a style is in use.
                self.aec.aec_style_picker_selection = handles.first().and_then(|h| {
                    self.tabs[self.active_tab]
                        .scene
                        .document
                        .get_entity(*h)
                        .and_then(crate::modules::aec::engine::xdata::wall_from_entity)
                        .map(|v2| v2.style_id)
                });
                self.aec.aec_style_picker_wall_handles = handles;
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker {
                    target: crate::app::StylePickerTarget::WallPropertiesStyle,
                }));
                Task::none()
            }
            AecMessage::AecStylePickerOpenForOpeningProperties(handles) => {
                self.aec_refresh_combined_style_library();
                self.aec.aec_style_picker_filter.clear();
                self.aec.aec_style_picker_selection = handles.first().and_then(|h| {
                    let owner = crate::modules::aec::engine::opening_display::resolve_opening_package(
                        &self.tabs[self.active_tab].scene,
                        *h,
                    );
                    self.tabs[self.active_tab]
                        .scene
                        .document
                        .get_entity(owner)
                        .and_then(|e| {
                            crate::modules::aec::engine::opening_xdata::opening_from_entity(e, owner)
                        })
                        .and_then(|o| o.style_id)
                });
                self.aec.aec_style_picker_wall_handles = handles;
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker {
                    target: crate::app::StylePickerTarget::OpeningPropertiesStyle,
                }));
                Task::none()
            }
            AecMessage::AecStylePickerOpenForActiveCommand => {
                self.aec_refresh_combined_style_library();
                self.aec.aec_style_picker_filter.clear();
                // Pre-select/highlight the style currently set on the
                // in-progress wall (if any) instead of always starting with
                // nothing highlighted.
                self.aec.aec_style_picker_selection = self.tabs[self.active_tab]
                    .active_cmd
                    .as_ref()
                    .and_then(|c| c.live_property_id("wall_style"));
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker {
                    target: crate::app::StylePickerTarget::ActiveCommand,
                }));
                Task::none()
            }
            AecMessage::AecStylePickerFilterChanged(v) => {
                self.aec.aec_style_picker_filter = v;
                Task::none()
            }
            AecMessage::AecStylePickerSelect(id) => {
                self.aec.aec_style_picker_selection = Some(id);
                Task::none()
            }
            AecMessage::AecStylePickerCancel => {
                // Targets opened from within the Wall Style Manager return
                // there on cancel, instead of closing the modal entirely.
                if let Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker { target })) = self.active_modal
                {
                    if matches!(
                        target,
                        crate::app::StylePickerTarget::WallStyleParent
                            | crate::app::StylePickerTarget::LayerMaterial(_)
                            | crate::app::StylePickerTarget::LayerOverride(_)
                    ) {
                        self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::WallStyleManager));
                        return Task::none();
                    }
                    if matches!(
                        target,
                        crate::app::StylePickerTarget::OpeningStyleParent
                    ) {
                        self.active_modal =
                            Some(crate::app::ModalKind::Aec(AecModalKind::OpeningStyleManager));
                        return Task::none();
                    }
                }
                self.close_active_modal();
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSelect(name) => {
                use crate::modules::aec::engine::display_component::layer_filter_to_ui_state;
                use crate::modules::aec::engine::display_component::WallComponentSlot;
                let rules = self
                    .aec.aec_style_manager_wall_style_editing_id
                    .as_ref()
                    .and_then(|id| {
                        self.aec.aec_style_library
                            .as_ref()
                            .and_then(|lib| lib.wall_styles.iter().find(|w| w.style.id == *id))
                    })
                    .and_then(|ws| ws.display_profiles.get(&name))
                    .cloned();
                match rules {
                    Some(rules) => {
                        let (c_explicit, c_sel) =
                            layer_filter_to_ui_state(rules.layer_filter_for(WallComponentSlot::Contour2D));
                        let (s_explicit, s_sel) =
                            layer_filter_to_ui_state(rules.layer_filter_for(WallComponentSlot::Solid3D));
                        self.aec.aec_style_manager_profile_contour_explicit = c_explicit;
                        self.aec.aec_style_manager_profile_contour_selection = c_sel;
                        self.aec.aec_style_manager_profile_solid_explicit = s_explicit;
                        self.aec.aec_style_manager_profile_solid_selection = s_sel;
                        let hatch = rules.style_override.get(WallComponentSlot::ContourHatch2D.key());
                        self.aec.aec_style_manager_profile_hatch_angle = hatch
                            .and_then(|h| h.hatch_angle)
                            .map(|a| format!("{a}"))
                            .unwrap_or_default();
                        self.aec.aec_style_manager_profile_hatch_relative = hatch
                            .and_then(|h| h.hatch_angle_relative)
                            .unwrap_or(false);
                        let mut visibility = std::collections::HashMap::new();
                        for slot in [
                            WallComponentSlot::AxisLine,
                            WallComponentSlot::Contour2D,
                            WallComponentSlot::ContourHatch2D,
                            WallComponentSlot::Layers2D,
                            WallComponentSlot::LayerHatch2D,
                            WallComponentSlot::Solid3D,
                            WallComponentSlot::SurfaceStyle3D,
                            WallComponentSlot::SectionRepresentation,
                            WallComponentSlot::ElevationRepresentation,
                        ] {
                            visibility.insert(slot, rules.is_visible(slot));
                        }
                        self.aec.aec_style_manager_profile_slot_visibility = visibility;
                        let mut overrides = std::collections::HashMap::new();
                        for slot in [
                            WallComponentSlot::AxisLine,
                            WallComponentSlot::Contour2D,
                            WallComponentSlot::ContourHatch2D,
                            WallComponentSlot::Layers2D,
                            WallComponentSlot::LayerHatch2D,
                            WallComponentSlot::Solid3D,
                            WallComponentSlot::SurfaceStyle3D,
                            WallComponentSlot::SectionRepresentation,
                            WallComponentSlot::ElevationRepresentation,
                        ] {
                            if let Some(style) = rules.style_for(slot).cloned() {
                                let has_visual = style.line_type.is_some()
                                    || style.line_color.is_some()
                                    || style.hatch_pattern.is_some()
                                    || style.hatch_color.is_some()
                                    || style.fill_color.is_some();
                                // ContourHatch2D may only store the shared hatch-angle
                                // fields; keep those out of the per-slot override map so
                                // the badge stays "Standard" unless a real style exists.
                                if has_visual {
                                    overrides.insert(slot, style);
                                }
                            }
                        }
                        self.aec.aec_style_manager_profile_slot_overrides = overrides;
                        self.aec.aec_style_manager_profile_editing_slot = None;
                        self.clear_aec_profile_slot_style_editor_buffers();
                    }
                    None => {
                        self.aec.aec_style_manager_profile_contour_explicit = false;
                        self.aec.aec_style_manager_profile_contour_selection = Vec::new();
                        self.aec.aec_style_manager_profile_solid_explicit = false;
                        self.aec.aec_style_manager_profile_solid_selection = Vec::new();
                        self.aec.aec_style_manager_profile_hatch_angle = String::new();
                        self.aec.aec_style_manager_profile_hatch_relative = false;
                        self.aec.aec_style_manager_profile_slot_visibility =
                            std::collections::HashMap::new();
                        self.aec.aec_style_manager_profile_slot_overrides =
                            std::collections::HashMap::new();
                        self.aec.aec_style_manager_profile_editing_slot = None;
                        self.clear_aec_profile_slot_style_editor_buffers();
                    }
                }
                self.aec.aec_style_manager_profile_selected = Some(name);
                Task::none()
            }
            AecMessage::AecStyleManagerProfileContourModeToggle(is_explicit) => {
                self.aec.aec_style_manager_profile_contour_explicit = is_explicit;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSolidModeToggle(is_explicit) => {
                self.aec.aec_style_manager_profile_solid_explicit = is_explicit;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileContourLayerToggle(layer) => {
                if let Some(pos) = self
                    .aec.aec_style_manager_profile_contour_selection
                    .iter()
                    .position(|l| *l == layer)
                {
                    self.aec.aec_style_manager_profile_contour_selection.remove(pos);
                } else {
                    self.aec.aec_style_manager_profile_contour_selection.push(layer);
                }
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSolidLayerToggle(layer) => {
                if let Some(pos) = self
                    .aec.aec_style_manager_profile_solid_selection
                    .iter()
                    .position(|l| *l == layer)
                {
                    self.aec.aec_style_manager_profile_solid_selection.remove(pos);
                } else {
                    self.aec.aec_style_manager_profile_solid_selection.push(layer);
                }
                Task::none()
            }
            AecMessage::AecStyleManagerProfileHatchRelativeToggle(value) => {
                self.aec.aec_style_manager_profile_hatch_relative = value;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileHatchAngleChanged(value) => {
                self.aec.aec_style_manager_profile_hatch_angle = value;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotVisibilityToggle(slot, visible) => {
                self.aec.aec_style_manager_profile_slot_visibility
                    .insert(slot, visible);
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleOpen(slot) => {
                use crate::modules::aec::ui::aec_ui_util::acad_color_to_editor_string;
                let style = self
                    .aec.aec_style_manager_profile_slot_overrides
                    .get(&slot)
                    .cloned()
                    .unwrap_or_default();
                self.aec.aec_style_manager_profile_editing_slot = Some(slot);
                self.aec.aec_style_manager_profile_slot_style_line_type =
                    style.line_type.clone().unwrap_or_default();
                self.aec.aec_style_manager_profile_slot_style_line_color = style
                    .line_color
                    .map(acad_color_to_editor_string)
                    .unwrap_or_default();
                self.aec.aec_style_manager_profile_slot_style_hatch_pattern =
                    style.hatch_pattern.clone().unwrap_or_default();
                self.aec.aec_style_manager_profile_slot_style_hatch_color = style
                    .hatch_color
                    .map(acad_color_to_editor_string)
                    .unwrap_or_default();
                self.aec.aec_style_manager_profile_slot_style_fill_color = style
                    .fill_color
                    .map(acad_color_to_editor_string)
                    .unwrap_or_default();
                self.aec.aec_style_manager_profile_slot_style_line_color_picker_open = false;
                self.aec.aec_style_manager_profile_slot_style_hatch_color_picker_open = false;
                self.aec.aec_style_manager_profile_slot_style_fill_color_picker_open = false;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleLineTypeChanged(v) => {
                self.aec.aec_style_manager_profile_slot_style_line_type = v;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleLineColorChanged(v) => {
                self.aec.aec_style_manager_profile_slot_style_line_color = v;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleLineColorPickerToggle => {
                self.aec.aec_style_manager_profile_slot_style_line_color_picker_open =
                    !self.aec.aec_style_manager_profile_slot_style_line_color_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleHatchPatternChanged(v) => {
                self.aec.aec_style_manager_profile_slot_style_hatch_pattern = v;
                self.aec.aec_style_manager_profile_slot_style_hatch_picker_open = false;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleHatchPickerToggle => {
                self.aec.aec_style_manager_profile_slot_style_hatch_picker_open =
                    !self.aec.aec_style_manager_profile_slot_style_hatch_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleHatchColorChanged(v) => {
                self.aec.aec_style_manager_profile_slot_style_hatch_color = v;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleHatchColorPickerToggle => {
                self.aec.aec_style_manager_profile_slot_style_hatch_color_picker_open =
                    !self.aec.aec_style_manager_profile_slot_style_hatch_color_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleFillColorChanged(v) => {
                self.aec.aec_style_manager_profile_slot_style_fill_color = v;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleFillColorPickerToggle => {
                self.aec.aec_style_manager_profile_slot_style_fill_color_picker_open =
                    !self.aec.aec_style_manager_profile_slot_style_fill_color_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleApply => {
                use crate::modules::aec::engine::display_component::component_style_override_from_editor_fields;
                let Some(slot) = self.aec.aec_style_manager_profile_editing_slot else {
                    return Task::none();
                };
                let style = component_style_override_from_editor_fields(
                    &self.aec.aec_style_manager_profile_slot_style_line_type,
                    &self.aec.aec_style_manager_profile_slot_style_line_color,
                    &self.aec.aec_style_manager_profile_slot_style_hatch_pattern,
                    &self.aec.aec_style_manager_profile_slot_style_hatch_color,
                    &self.aec.aec_style_manager_profile_slot_style_fill_color,
                );
                if style == Default::default() {
                    self.aec.aec_style_manager_profile_slot_overrides.remove(&slot);
                } else {
                    self.aec.aec_style_manager_profile_slot_overrides
                        .insert(slot, style);
                }
                self.aec.aec_style_manager_profile_editing_slot = None;
                self.clear_aec_profile_slot_style_editor_buffers();
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleClear => {
                if let Some(slot) = self.aec.aec_style_manager_profile_editing_slot.take() {
                    self.aec.aec_style_manager_profile_slot_overrides.remove(&slot);
                }
                self.clear_aec_profile_slot_style_editor_buffers();
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleClose => {
                self.aec.aec_style_manager_profile_editing_slot = None;
                self.clear_aec_profile_slot_style_editor_buffers();
                Task::none()
            }
            AecMessage::AecWallStyleManagerDisplayProfilesOpen => {
                // Keep the wall style manager geometry so closing the child
                // modal can restore it (Plot → Plotstyle pattern).
                self.aec.aec_wall_style_manager_parent_geometry =
                    Some((self.modal_offset, self.modal_resize));
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::WallStyleDisplayProfiles));
                self.reset_modal_geometry();
                Task::none()
            }
            AecMessage::AecWallStyleManagerDisplayProfilesClose => {
                self.close_active_modal();
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSave => {
                use crate::modules::aec::engine::display_component::{
                    layer_filter_from_selection, ComponentStyleOverride, WallComponentSlot,
                };
                let Some(config_name) = self.aec.aec_style_manager_profile_selected.clone() else {
                    return Task::none();
                };
                let Some(id) = self.aec.aec_style_manager_wall_style_editing_id.clone() else {
                    return Task::none();
                };
                let Some(mut wall_style) = self.aec.aec_style_library.as_ref().and_then(|lib| {
                    lib.wall_styles.iter().find(|w| w.style.id == id).cloned()
                }) else {
                    return Task::none();
                };

                let mut rules = wall_style
                    .display_profiles
                    .get(&config_name)
                    .cloned()
                    .unwrap_or_default();
                rules.layer_filter.insert(
                    WallComponentSlot::Contour2D.key().to_string(),
                    layer_filter_from_selection(
                        self.aec.aec_style_manager_profile_contour_explicit,
                        &self.aec.aec_style_manager_profile_contour_selection,
                    ),
                );
                rules.layer_filter.insert(
                    WallComponentSlot::Solid3D.key().to_string(),
                    layer_filter_from_selection(
                        self.aec.aec_style_manager_profile_solid_explicit,
                        &self.aec.aec_style_manager_profile_solid_selection,
                    ),
                );
                let hatch_angle = self
                    .aec.aec_style_manager_profile_hatch_angle
                    .trim()
                    .parse::<f64>()
                    .ok();
                if hatch_angle.is_some() {
                    rules.style_override.insert(
                        WallComponentSlot::ContourHatch2D.key().to_string(),
                        ComponentStyleOverride {
                            hatch_angle,
                            hatch_angle_relative: Some(self.aec.aec_style_manager_profile_hatch_relative),
                            ..Default::default()
                        },
                    );
                } else {
                    rules.style_override.remove(WallComponentSlot::ContourHatch2D.key());
                }
                for (slot, visible) in &self.aec.aec_style_manager_profile_slot_visibility {
                    if *visible {
                        rules.visibility.remove(slot.key());
                    } else {
                        rules.visibility.insert(slot.key().to_string(), false);
                    }
                }
                // Preserve ContourHatch2D hatch-angle entry written above, then
                // merge/replace per-slot visual overrides from the pending map.
                let hatch_angle_entry = rules
                    .style_override
                    .get(WallComponentSlot::ContourHatch2D.key())
                    .cloned();
                // Drop previous visual overrides for known slots, then rewrite.
                for slot in [
                    WallComponentSlot::AxisLine,
                    WallComponentSlot::Contour2D,
                    WallComponentSlot::ContourHatch2D,
                    WallComponentSlot::Layers2D,
                    WallComponentSlot::LayerHatch2D,
                    WallComponentSlot::Solid3D,
                    WallComponentSlot::SurfaceStyle3D,
                    WallComponentSlot::SectionRepresentation,
                    WallComponentSlot::ElevationRepresentation,
                ] {
                    rules.style_override.remove(slot.key());
                }
                if let Some(entry) = hatch_angle_entry {
                    // Re-insert hatch-angle-only base; visual fields may be merged below.
                    if entry.hatch_angle.is_some() || entry.hatch_angle_relative.is_some() {
                        rules.style_override.insert(
                            WallComponentSlot::ContourHatch2D.key().to_string(),
                            ComponentStyleOverride {
                                hatch_angle: entry.hatch_angle,
                                hatch_angle_relative: entry.hatch_angle_relative,
                                ..Default::default()
                            },
                        );
                    }
                }
                for (slot, style) in &self.aec.aec_style_manager_profile_slot_overrides {
                    if *slot == WallComponentSlot::ContourHatch2D {
                        let mut merged = rules
                            .style_override
                            .remove(slot.key())
                            .unwrap_or_default();
                        if style.line_type.is_some() {
                            merged.line_type = style.line_type.clone();
                        }
                        if style.line_color.is_some() {
                            merged.line_color = style.line_color;
                        }
                        if style.hatch_pattern.is_some() {
                            merged.hatch_pattern = style.hatch_pattern.clone();
                        }
                        if style.hatch_color.is_some() {
                            merged.hatch_color = style.hatch_color;
                        }
                        if style.fill_color.is_some() {
                            merged.fill_color = style.fill_color;
                        }
                        rules
                            .style_override
                            .insert(slot.key().to_string(), merged);
                    } else {
                        rules
                            .style_override
                            .insert(slot.key().to_string(), style.clone());
                    }
                }
                wall_style.display_profiles.insert(config_name.clone(), rules);

                let source = crate::modules::aec::engine::library::wall_style_library_source_with_session(
                    self.aec.aec_project_explorer_file.as_ref(),
                    self.aec.aec_session_style_library.as_ref(),
                    &id,
                );
                let cow_from_standard =
                    source == Some(crate::modules::aec::engine::library::LibrarySource::Standard);

                if self.aec.aec_project_explorer_file.is_some() {
                    match self.aec_upsert_wall_style_into_project(wall_style) {
                        Ok(()) => {
                            if cow_from_standard {
                                self.command_line.push_info(
                                    crate::t!(
                                        "AEC Style Manager: Standard wall style was copied into the project and saved."
                                    )
                                    .as_ref(),
                                );
                            } else {
                                self.command_line.push_info(
                                    crate::t!("AEC Style Manager: display profile saved.").as_ref(),
                                );
                            }
                        }
                        Err(e) => {
                            self.command_line.push_error(
                                crate::tf!("AEC Style Manager: failed to save library: {e}").as_ref(),
                            );
                        }
                    }
                } else {
                    self.aec_upsert_wall_style_into_session(wall_style);
                }
                Task::none()
            }
            AecMessage::AecStyleManagerProfileRemove => {
                let Some(config_name) = self.aec.aec_style_manager_profile_selected.clone() else {
                    return Task::none();
                };
                let Some(id) = self.aec.aec_style_manager_wall_style_editing_id.clone() else {
                    return Task::none();
                };
                let Some(mut wall_style) = self.aec.aec_style_library.as_ref().and_then(|lib| {
                    lib.wall_styles.iter().find(|w| w.style.id == id).cloned()
                }) else {
                    return Task::none();
                };
                wall_style.display_profiles.remove(&config_name);

                self.aec.aec_style_manager_profile_contour_explicit = false;
                self.aec.aec_style_manager_profile_contour_selection = Vec::new();
                self.aec.aec_style_manager_profile_solid_explicit = false;
                self.aec.aec_style_manager_profile_solid_selection = Vec::new();
                self.aec.aec_style_manager_profile_hatch_angle = String::new();
                self.aec.aec_style_manager_profile_hatch_relative = false;
                self.aec.aec_style_manager_profile_slot_visibility = std::collections::HashMap::new();
                self.aec.aec_style_manager_profile_slot_overrides = std::collections::HashMap::new();
                self.aec.aec_style_manager_profile_editing_slot = None;
                self.clear_aec_profile_slot_style_editor_buffers();

                if self.aec.aec_project_explorer_file.is_some() {
                    if let Err(e) = self.aec_upsert_wall_style_into_project(wall_style) {
                        self.command_line.push_error(
                            crate::tf!("AEC Style Manager: failed to save library: {e}").as_ref(),
                        );
                    } else {
                        self.command_line
                            .push_info(crate::t!("AEC Style Manager: display profile removed.").as_ref());
                    }
                } else {
                    self.aec_upsert_wall_style_into_session(wall_style);
                }
                Task::none()
            }
            AecMessage::AecStylePickerConfirm => {
                if let (Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker { target })), Some(selection)) =
                    (self.active_modal, self.aec.aec_style_picker_selection.clone())
                {
                    match target {
                        crate::app::StylePickerTarget::WallStyleParent => {
                            let id = if selection.is_empty() {
                                None
                            } else {
                                Some(selection)
                            };
                            self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::WallStyleManager));
                            return self.update(Message::Aec(AecMessage::AecStyleManagerWallStyleParentChanged(id)));
                        }
                        crate::app::StylePickerTarget::LayerMaterial(index) => {
                            self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::WallStyleManager));
                            return self.update(Message::Aec(AecMessage::AecStyleManagerWallStyleLayerMaterialChanged(
                                index, selection,
                            )));
                        }
                        crate::app::StylePickerTarget::LayerOverride(index) => {
                            self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::WallStyleManager));
                            return self.update(Message::Aec(AecMessage::AecStyleManagerWallStyleLayerOverrideChanged(
                                index, selection,
                            )));
                        }
                        crate::app::StylePickerTarget::WallPropertiesStyle => {
                            self.active_modal = None;
                            let Some(lib) = self.aec.aec_style_library.clone() else {
                                return Task::none();
                            };

                            let i = self.active_tab;
                            self.push_undo_snapshot(i, "CHPROP");

                            let handles = self.aec.aec_style_picker_wall_handles.clone();
                            for handle in handles {
                                let wall_owner = crate::modules::aec::engine::wall_package::resolve_wall_package(
                                    &self.tabs[i].scene,
                                    handle,
                                );
                                let bb = self.tabs[i]
                                    .scene
                                    .document
                                    .get_entity(wall_owner)
                                    .and_then(crate::modules::aec::engine::xdata::wall_from_entity)
                                    .and_then(|w| {
                                        let t = w.total_thickness();
                                        if t > 0.0 {
                                            Some(t)
                                        } else {
                                            None
                                        }
                                    });
                                let wall_layers = crate::modules::aec::engine::xdata::resolve_wall_style_layers_ids(
                                    &lib,
                                    &selection,
                                    bb,
                                )
                                .unwrap_or_default();

                                if crate::modules::aec::engine::xdata::write_wall_style(
                                    &mut self.tabs[i].scene,
                                    wall_owner,
                                    &selection,
                                    wall_layers,
                                ) {
                                    let _ = self.regenerate_wall_respecting_active_display_config(i, wall_owner);
                                    self.tabs[i].dirty = true;
                                }
                            }
                            self.refresh_properties();
                        }
                        crate::app::StylePickerTarget::OpeningStyleParent => {
                            let id = if selection.is_empty() {
                                None
                            } else {
                                Some(selection)
                            };
                            self.active_modal =
                                Some(crate::app::ModalKind::Aec(AecModalKind::OpeningStyleManager));
                            return self.update(Message::Aec(
                                AecMessage::AecOpeningStyleManagerParentChanged(id),
                            ));
                        }
                        crate::app::StylePickerTarget::OpeningPropertiesStyle => {
                            self.active_modal = None;
                            return self.aec_apply_picked_opening_style(selection);
                        }
                        crate::app::StylePickerTarget::ActiveCommand => {
                            let i = self.active_tab;
                            if !selection.is_empty() {
                                self.aec.aec_last_wall_style_id = Some(selection.clone());
                            }
                            let result = self.tabs[i].active_cmd.as_mut().map(|c| {
                                c.apply_live_property(
                                    "wall_style",
                                    crate::command::LiveFieldValue::Picker(selection.clone()),
                                )
                            });
                            self.active_modal = None;
                            if let Some(r) = result {
                                let task = self.apply_cmd_result(r);
                                self.refresh_active_cmd_preview(i);
                                self.refresh_properties();
                                return task;
                            }
                            self.refresh_properties();
                            return Task::none();
                        }
                    }
                }
                self.active_modal = None;
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleSave => {
                self.aec_style_manager_wall_style_save_internal();
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleSaveAndApply => {
                if let Some((id, lib)) = self.aec_style_manager_wall_style_save_internal() {
                    let tab_index = self.active_tab;
                    let wall_style_map: std::collections::HashMap<
                        String,
                        crate::modules::aec::engine::wall_style::WallStyle,
                    > = lib
                        .wall_styles
                        .iter()
                        .map(|ws| (ws.style.id.clone(), ws.clone()))
                        .collect();

                    let style_map: std::collections::HashMap<
                        String,
                        crate::modules::aec::engine::style::Style,
                    > = wall_style_map
                        .iter()
                        .map(|(k, v)| (k.clone(), v.style.clone()))
                        .collect();

                    let mut affected_handles = Vec::new();
                    for entity in self.tabs[tab_index].scene.document.entities() {
                        if let Some(wall) =
                            crate::modules::aec::engine::xdata::wall_from_entity(entity)
                        {
                            if wall.style_id == id {
                                affected_handles.push(entity.common().handle);
                            } else if let Ok(chain) =
                                crate::modules::aec::engine::style::resolve_chain(
                                    &style_map,
                                    &wall.style_id,
                                )
                            {
                                if chain.contains(&id) {
                                    affected_handles.push(entity.common().handle);
                                }
                            }
                        }
                    }

                    let mut updated_count = 0;
                    for handle in affected_handles {
                        if let Some(entity) = self.tabs[tab_index].scene.document.get_entity(handle) {
                            if let Some(wall) =
                                crate::modules::aec::engine::xdata::wall_from_entity(entity)
                            {
                                // Prefer the wall's current total thickness as BB so
                                // formula layers scale with the placed wall width;
                                // fall back to style fixed-sum when empty.
                                let bb = {
                                    let t = wall.total_thickness();
                                    if t > 0.0 {
                                        Some(t)
                                    } else {
                                        None
                                    }
                                };
                                if let Some(wall_layers) =
                                    crate::modules::aec::engine::xdata::resolve_wall_style_layers_ids(
                                        &lib,
                                        &wall.style_id,
                                        bb,
                                    )
                                {
                                    crate::modules::aec::engine::xdata::write_wall_layers(
                                        &mut self.tabs[tab_index].scene,
                                        handle,
                                        wall_layers,
                                    );
                                }
                            }
                        }

                        if self
                            .regenerate_wall_respecting_active_display_config(tab_index, handle)
                            .is_ok()
                        {
                            updated_count += 1;
                        }
                    }

                    if updated_count > 0 {
                        self.tabs[tab_index].scene.bump_geometry();
                        self.command_line.push_info(
                            crate::tf!(
                                "AEC Style Manager: updated {updated_count} wall(s) using this style."
                            )
                            .as_ref(),
                        );
                    }
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleDelete => {
                if let Some(id) = self.aec.aec_style_manager_selected_wall_style.clone() {
                    let lib_snapshot = if let Some(lib) = self.aec.aec_style_library.as_mut() {
                        lib.remove_wall_style(&id);
                        Some(lib.clone())
                    } else {
                        None
                    };
                    if let Some(lib_snapshot) = lib_snapshot {
                        match self.aec_save_style_library_preferring_project(&lib_snapshot) {
                            Ok(()) => self.command_line.push_info(
                                crate::t!("AEC Style Manager: wall style deleted.").as_ref(),
                            ),
                            Err(e) => self.command_line.push_error(
                                crate::tf!("AEC Style Manager: failed to save library: {e}")
                                    .as_ref(),
                            ),
                        }
                    }
                }
                self.aec.aec_style_manager_selected_wall_style = None;
                self.aec.aec_style_manager_wall_style_editing_id = None;
                self.aec.aec_style_manager_wall_style_form_open = false;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialNew => {
                self.aec.aec_style_manager_selected_material = None;
                self.aec.aec_style_manager_selected_wall_style = None;
                self.aec.aec_style_manager_material_editing_id = None;
                self.aec.aec_style_manager_material_name.clear();
                self.aec.aec_style_manager_material_hatch.clear();
                self.aec.aec_style_manager_material_color = "#FFFFFF".to_string();
                self.aec.aec_style_manager_material_line_type = "Continuous".to_string();
                self.aec.aec_style_manager_material_category.clear();
                self.aec.aec_style_manager_material_hatch_color = 0xFFFFFF;
                self.aec.aec_style_manager_material_hatch_scale = "1.0".to_string();
                self.aec.aec_style_manager_material_render_ref.clear();
                self.aec.aec_style_manager_material_hatch_angle = "0.0".to_string();
                self.aec.aec_style_manager_material_hatch_angle_relative = true;
                self.aec.aec_style_manager_material_hatch_color_picker_open = false;
                self.aec.aec_style_manager_material_form_open = true;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialNameChanged(value) => {
                self.aec.aec_style_manager_material_name = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchChanged(value) => {
                self.aec.aec_style_manager_material_hatch = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchPickerToggle => {
                self.aec.aec_style_manager_material_hatch_picker_open =
                    !self.aec.aec_style_manager_material_hatch_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchSelected(name) => {
                self.aec.aec_style_manager_material_hatch = name;
                self.aec.aec_style_manager_material_hatch_picker_open = false;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialColorChanged(value) => {
                self.aec.aec_style_manager_material_color = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialLineTypeChanged(value) => {
                self.aec.aec_style_manager_material_line_type = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialColorPickerToggle => {
                self.aec.aec_style_manager_material_color_picker_open =
                    !self.aec.aec_style_manager_material_color_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialColorPicked(color) => {
                self.aec.aec_style_manager_material_color_picker_open = false;
                let [r, g, b, _] = match color {
                    acadrust::types::Color::Rgb { r, g, b } => [r, g, b, 255u8],
                    acadrust::types::Color::Index(i) => {
                        let (r, g, b) = acadrust::types::aci_table::aci_to_rgb(i)
                            .unwrap_or((255, 255, 255));
                        [r, g, b, 255]
                    }
                    _ => [255, 255, 255, 255],
                };
                self.aec.aec_style_manager_material_color = format!("#{r:02X}{g:02X}{b:02X}");
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialCategoryChanged(value) => {
                self.aec.aec_style_manager_material_category = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchColorChanged(value) => {
                self.aec.aec_style_manager_material_hatch_color = value;
                self.aec.aec_style_manager_material_hatch_color_picker_open = false;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchColorPickerToggle => {
                self.aec.aec_style_manager_material_hatch_color_picker_open =
                    !self.aec.aec_style_manager_material_hatch_color_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchScaleChanged(value) => {
                self.aec.aec_style_manager_material_hatch_scale = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialRenderRefChanged(value) => {
                self.aec.aec_style_manager_material_render_ref = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchAngleChanged(value) => {
                self.aec.aec_style_manager_material_hatch_angle = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchAngleRelativeToggle => {
                self.aec.aec_style_manager_material_hatch_angle_relative =
                    !self.aec.aec_style_manager_material_hatch_angle_relative;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialDuplicate => {
                let source_id = self
                    .aec.aec_style_manager_material_editing_id
                    .clone()
                    .or_else(|| self.aec.aec_style_manager_selected_material.clone());
                let Some(source_id) = source_id else {
                    return Task::none();
                };
                let Some(material) = self
                    .aec.aec_style_library
                    .as_ref()
                    .and_then(|lib| lib.materials.iter().find(|m| m.id == source_id))
                    .cloned()
                else {
                    return Task::none();
                };
                let existing_names: Vec<String> = self
                    .aec.aec_style_library
                    .as_ref()
                    .map(|lib| lib.materials.iter().map(|m| m.name.clone()).collect())
                    .unwrap_or_default();
                let base_name = format!("{} (Kopie)", material.name);
                let mut name = base_name.clone();
                let mut counter = 2;
                while existing_names.iter().any(|n| n == &name) {
                    name = format!("{base_name} {counter}");
                    counter += 1;
                }
                self.aec.aec_style_manager_selected_material = None;
                self.aec.aec_style_manager_selected_wall_style = None;
                self.aec.aec_style_manager_material_editing_id = None;
                self.aec.aec_style_manager_material_name = name;
                self.aec.aec_style_manager_material_hatch = material.hatch_pattern;
                self.aec.aec_style_manager_material_color =
                    format!("#{:06X}", material.line_color);
                self.aec.aec_style_manager_material_line_type = material.line_type;
                self.aec.aec_style_manager_material_category =
                    material.category.unwrap_or_default();
                                self.aec.aec_style_manager_material_hatch_color = material
                    .hatch_color
                    .and_then(|c| c.rgb())
                    .map(|(r, g, b)| ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
                    .unwrap_or(material.line_color);
                self.aec.aec_style_manager_material_hatch_scale =
                    format!("{}", material.hatch_scale);
                self.aec.aec_style_manager_material_render_ref =
                    material.render_material_ref.unwrap_or_default();
                self.aec.aec_style_manager_material_hatch_angle =
                    format!("{}", material.hatch_angle);
                self.aec.aec_style_manager_material_hatch_angle_relative =
                    material.hatch_angle_relative;
                self.aec.aec_style_manager_material_color_picker_open = false;
                self.aec.aec_style_manager_material_hatch_color_picker_open = false;
                self.aec.aec_style_manager_material_hatch_picker_open = false;
                self.aec.aec_style_manager_material_form_open = true;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialSave => {
                self.aec_style_manager_material_save_internal();
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialSaveAndApply => {
                if let Some((mat_id, _lib)) = self.aec_style_manager_material_save_internal() {
                    let tab_index = self.active_tab;
                    let mut affected_walls = Vec::new();
                    for entity in self.tabs[tab_index].scene.document.entities() {
                        if let Some(wall) =
                            crate::modules::aec::engine::xdata::wall_from_entity(entity)
                        {
                            if wall.layers.iter().any(|l| l.material == mat_id) {
                                affected_walls.push(entity.common().handle);
                            }
                        }
                    }
                    let mut updated_count = 0;
                    for handle in affected_walls {
                        if self
                            .regenerate_wall_respecting_active_display_config(tab_index, handle)
                            .is_ok()
                        {
                            updated_count += 1;
                        }
                    }
                    if updated_count > 0 {
                        self.tabs[tab_index].scene.bump_geometry();
                        self.command_line.push_info(
                            crate::tf!(
                                "AEC Style Manager: updated {updated_count} wall(s) using this material."
                            )
                            .as_ref(),
                        );
                    }
                }
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialDelete => {
                if let Some(id) = self.aec.aec_style_manager_selected_material.clone() {
                    let lib_snapshot = if let Some(lib) = self.aec.aec_style_library.as_mut() {
                        lib.remove_material(&id);
                        Some(lib.clone())
                    } else {
                        None
                    };
                    if let Some(lib_snapshot) = lib_snapshot {
                        match self.aec_save_style_library_preferring_project(&lib_snapshot) {
                            Ok(()) => self.command_line.push_info(
                                crate::t!("AEC Style Manager: material deleted.").as_ref(),
                            ),
                            Err(e) => self.command_line.push_error(
                                crate::tf!("AEC Style Manager: failed to save library: {e}")
                                    .as_ref(),
                            ),
                        }
                    }
                }
                self.aec.aec_style_manager_selected_material = None;
                self.aec.aec_style_manager_material_editing_id = None;
                self.aec.aec_style_manager_material_form_open = false;
                Task::none()
            }
            AecMessage::AecStyleManagerCopyMaterialToProject => self.aec_handle_copy_material(true),
            AecMessage::AecStyleManagerCopyMaterialToGlobal => self.aec_handle_copy_material(false),
            AecMessage::AecStyleManagerCopyWallStyleToProject => self.aec_handle_copy_wall_style(true),
            AecMessage::AecStyleManagerCopyWallStyleToGlobal => self.aec_handle_copy_wall_style(false),
            AecMessage::AecStyleManagerCopyConflictConfirm(confirmed) => {
                self.aec.aec_style_manager_copy_conflict_open = false;
                let return_modal = match &self.aec.aec_style_manager_pending_copy {
                    Some(AecPendingCopy::Material { .. }) => {
                        crate::app::ModalKind::Aec(AecModalKind::MaterialManager)
                    }
                    Some(AecPendingCopy::WallStyle { .. }) => {
                        crate::app::ModalKind::Aec(AecModalKind::WallStyleManager)
                    }
                    Some(AecPendingCopy::OpeningStyle { .. }) => {
                        crate::app::ModalKind::Aec(AecModalKind::OpeningStyleManager)
                    }
                    None => crate::app::ModalKind::Aec(AecModalKind::MaterialManager),
                };
                if confirmed {
                    if let Some(pending) = self.aec.aec_style_manager_pending_copy.take() {
                        self.aec_execute_copy(pending);
                    }
                } else {
                    self.aec.aec_style_manager_pending_copy = None;
                }
                self.active_modal = Some(return_modal);
                Task::none()
            }
            // ── Layer Translator (#624) ──────────────────────────────────
            AecMessage::WallJunctionSubmenuToggle => {
                let i = self.active_tab;
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.junction_menu_submenu = !sel.junction_menu_submenu;
                Task::none()
            }

            AecMessage::WallJunctionOverrideSetStyle(style) => {
                let i = self.active_tab;
                let junction = self.tabs[i].scene.selection.borrow().junction_menu;
                if let Some((axis_handle, end_index)) = junction {
                                        let mut override_data =
                        xdata::read_junction_override(&self.tabs[i].scene, axis_handle, end_index)
                            .unwrap_or_default();
                    override_data.default_style = Some(style);
                    let style_library =
                        crate::modules::aec::engine::project::resolve_style_library(
                            self.aec.aec_project_explorer_file.as_ref(),
                        );
                    let (display_rules, style_substitutions) =
                        self.resolve_active_display_config_wall_rules(i, Some(axis_handle));
                    let touched = join_ops::apply_junction_override_and_rebuild(
                        &mut self.tabs[i].scene,
                        axis_handle,
                        end_index,
                        Some(&override_data),
                        Some(&style_library),
                        display_rules.as_ref(),
                        style_substitutions.as_ref(),
                    );
                    self.reapply_active_display_config_to_wall_packages(i, &touched);
                }
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.context_menu = None;
                sel.junction_menu = None;
                drop(sel);
                self.refresh_properties();
                Task::none()
            }

            AecMessage::WallJunctionOverrideReset => {
                let i = self.active_tab;
                let junction = self.tabs[i].scene.selection.borrow().junction_menu;
                if let Some((axis_handle, end_index)) = junction {
                                        let style_library =
                        crate::modules::aec::engine::project::resolve_style_library(
                            self.aec.aec_project_explorer_file.as_ref(),
                        );
                    let (display_rules, style_substitutions) =
                        self.resolve_active_display_config_wall_rules(i, Some(axis_handle));
                    let touched = join_ops::apply_junction_override_and_rebuild(
                        &mut self.tabs[i].scene,
                        axis_handle,
                        end_index,
                        None,
                        Some(&style_library),
                        display_rules.as_ref(),
                        style_substitutions.as_ref(),
                    );
                    self.reapply_active_display_config_to_wall_packages(i, &touched);
                }
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.context_menu = None;
                sel.junction_menu = None;
                drop(sel);
                self.refresh_properties();
                Task::none()
            }

            AecMessage::AecJunctionLayerPairPickStart(axis_handle, end_index) => {
                let i = self.active_tab;
                self.aec.aec_layer_pair_draw = Some(crate::app::AecLayerPairDrawPick {
                    axis: axis_handle,
                    end_index,
                    layer_a: None,
                    layer_b: None,
                    layer_b_outer: false,
                    hover: None,
                    awaiting_style: false,
                });
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.context_menu = None;
                sel.junction_menu = Some((axis_handle, end_index));
                sel.junction_menu_only = false;
                drop(sel);
                self.sync_junction_editor_layer_highlight();
                self.command_line.push_info(
                    crate::t!("Erste Schicht am Knoten klicken (Esc = Abbrechen).").as_ref(),
                );
                Task::none()
            }

            AecMessage::AecJunctionLayerGapPickStart(axis_handle, end_index) => {
                let i = self.active_tab;
                self.aec.aec_layer_pair_draw = None;
                self.aec.aec_layer_gap_draw = Some(crate::app::AecLayerGapDrawPick {
                    axis: axis_handle,
                    end_index,
                    layer: None,
                    from: None,
                    to: None,
                    hover: None,
                });
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.context_menu = None;
                sel.junction_menu = Some((axis_handle, end_index));
                sel.junction_menu_only = false;
                drop(sel);
                self.sync_junction_editor_layer_highlight();
                self.command_line.push_info(
                    crate::t!(
                        "Schicht der durchlaufenden Wand klicken, die unterbrochen werden soll (Esc = Abbrechen)."
                    )
                    .as_ref(),
                );
                Task::none()
            }

            AecMessage::AecJunctionLayerPairPickCancel => {
                self.cancel_layer_pair_draw_pick();
                Task::none()
            }

            AecMessage::AecJunctionLayerPairSetStyle(style) => {
                self.apply_layer_pair_draw_style(style);
                Task::none()
            }

            AecMessage::AecJunctionEditorOpen(axis_handle, end_index) => {
                let i = self.active_tab;
                                let ov = xdata::read_junction_override(&self.tabs[i].scene, axis_handle, end_index)
                    .unwrap_or_default();
                self.aec.aec_junction_editor_target = Some((axis_handle, end_index));
                self.aec.aec_junction_editor_default_style = ov.default_style;
                self.aec.aec_junction_editor_pairs = ov.layer_pairs;
                self.aec.aec_junction_editor_gaps =
                    join_ops::read_through_layer_gaps(&self.tabs[i].scene, axis_handle, end_index);
                self.aec.aec_junction_editor_gap_layer = None;
                self.aec.aec_junction_editor_gap_from_wall = None;
                self.aec.aec_junction_editor_gap_from = None;
                self.aec.aec_junction_editor_gap_to_wall = None;
                self.aec.aec_junction_editor_gap_to = None;
                self.aec.aec_junction_editor_pair_layer_a = None;
                self.aec.aec_junction_editor_pair_wall_b = None;
                self.aec.aec_junction_editor_pair_layer_b = None;
                self.aec.aec_junction_editor_pair_style =
                    crate::modules::aec::engine::join::JoinOverrideStyle::Miter;
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.context_menu = None;
                sel.junction_menu = None;
                drop(sel);
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::JunctionEditor));
                self.sync_junction_editor_layer_highlight();
                Task::none()
            }

            AecMessage::AecJunctionEditorClose => {
                self.aec.aec_junction_editor_target = None;
                self.active_modal = None;
                self.reset_modal_geometry();
                self.sync_junction_editor_layer_highlight();
                Task::none()
            }

            AecMessage::AecJunctionEditorSetDefaultStyle(style) => {
                self.aec.aec_junction_editor_default_style = Some(style);
                Task::none()
            }

            AecMessage::AecJunctionEditorResetDefaultStyle => {
                self.aec.aec_junction_editor_default_style = None;
                Task::none()
            }

            AecMessage::AecJunctionEditorPairLayerAChanged(index, material_id) => {
                self.aec.aec_junction_editor_pair_layer_a = Some((index, material_id));
                self.sync_junction_editor_layer_highlight();
                Task::none()
            }

            AecMessage::AecJunctionEditorPairWallBChanged(wall_b) => {
                self.aec.aec_junction_editor_pair_wall_b = wall_b;
                self.aec.aec_junction_editor_pair_layer_b = None;
                self.sync_junction_editor_layer_highlight();
                Task::none()
            }

            AecMessage::AecJunctionEditorPairLayerBChanged(index, material_id) => {
                self.aec.aec_junction_editor_pair_layer_b = Some((index, material_id));
                self.sync_junction_editor_layer_highlight();
                Task::none()
            }

            AecMessage::AecJunctionEditorPairStyleChanged(style) => {
                self.aec.aec_junction_editor_pair_style = style;
                Task::none()
            }

            AecMessage::AecJunctionEditorAddPair => {
                use crate::modules::aec::engine::junction_pick::selected_junction_layer_ref;
                use crate::modules::aec::engine::join::LayerRef;
                if let Some((layer_a_index, layer_a_id)) = self.aec.aec_junction_editor_pair_layer_a.clone() {
                    let i = self.active_tab;
                    let participants = self.aec.aec_junction_editor_target.map(|(h, e)| {
                        crate::modules::aec::engine::join_ops::walls_at_junction(
                            &self.tabs[i].scene,
                            h,
                            e,
                        )
                    });
                    let resolve_ref = |wall: Option<acadrust::Handle>, index: usize, material_id: String| {
                        let layers = participants.as_ref().and_then(|parts| {
                            if let Some(h) = wall {
                                parts.iter().find(|p| p.axis_handle == h).map(|p| &p.layers)
                            } else {
                                self.aec.aec_junction_editor_target.and_then(|(h, e)| {
                                    parts
                                        .iter()
                                        .find(|p| p.axis_handle == h && p.end_index == e)
                                        .or_else(|| parts.iter().find(|p| p.axis_handle == h))
                                        .map(|p| &p.layers)
                                })
                            }
                        });
                        layers
                            .and_then(|ls| selected_junction_layer_ref(ls, index, &material_id))
                            .unwrap_or(LayerRef {
                                material_id,
                                role_tag: None,
                                index,
                                layer_id: None,
                            })
                    };
                    let layer_b = self.aec.aec_junction_editor_pair_layer_b.clone().map(|(index, id)| {
                        resolve_ref(self.aec.aec_junction_editor_pair_wall_b, index, id)
                    });
                    crate::modules::aec::engine::join::upsert_layer_pair(
                        &mut self.aec.aec_junction_editor_pairs,
                        crate::modules::aec::engine::join::LayerPairOverride {
                            layer_a: resolve_ref(None, layer_a_index, layer_a_id),
                            layer_b,
                            style: self.aec.aec_junction_editor_pair_style.clone(),
                        },
                    );
                    self.aec.aec_junction_editor_pair_layer_a = None;
                    self.aec.aec_junction_editor_pair_wall_b = None;
                    self.aec.aec_junction_editor_pair_layer_b = None;
                    self.sync_junction_editor_layer_highlight();
                }
                Task::none()
            }

            AecMessage::AecJunctionEditorSetPairStyle(idx, style) => {
                if let Some(pair) = self.aec.aec_junction_editor_pairs.get_mut(idx) {
                    pair.style = style;
                }
                Task::none()
            }

            AecMessage::AecJunctionEditorRemovePair(idx) => {
                if idx < self.aec.aec_junction_editor_pairs.len() {
                    self.aec.aec_junction_editor_pairs.remove(idx);
                }
                Task::none()
            }

            AecMessage::AecJunctionEditorGapLayerChanged(index, material_id) => {
                self.aec.aec_junction_editor_gap_layer = Some((index, material_id));
                self.sync_junction_editor_layer_highlight();
                Task::none()
            }
            AecMessage::AecJunctionEditorGapFromWallChanged(handle) => {
                self.aec.aec_junction_editor_gap_from_wall = handle;
                self.aec.aec_junction_editor_gap_from = None;
                Task::none()
            }
            AecMessage::AecJunctionEditorGapFromChanged(index, material_id) => {
                self.aec.aec_junction_editor_gap_from = Some((index, material_id));
                Task::none()
            }
            AecMessage::AecJunctionEditorGapToWallChanged(handle) => {
                self.aec.aec_junction_editor_gap_to_wall = handle;
                self.aec.aec_junction_editor_gap_to = None;
                Task::none()
            }
            AecMessage::AecJunctionEditorGapToChanged(index, material_id) => {
                self.aec.aec_junction_editor_gap_to = Some((index, material_id));
                Task::none()
            }
            AecMessage::AecJunctionEditorAddGap => {
                if let (Some((li, lid)), Some((fi, fid)), Some((ti, tid))) = (
                    self.aec.aec_junction_editor_gap_layer.clone(),
                    self.aec.aec_junction_editor_gap_from.clone(),
                    self.aec.aec_junction_editor_gap_to.clone(),
                ) {
                    use crate::modules::aec::engine::junction_pick::selected_junction_layer_ref;
                    use crate::modules::aec::engine::join::LayerRef;
                    let i = self.active_tab;
                    let participants = self.aec.aec_junction_editor_target.map(|(h, e)| {
                        crate::modules::aec::engine::join_ops::walls_at_junction(
                            &self.tabs[i].scene,
                            h,
                            e,
                        )
                    });
                    let resolve_ref = |wall: Option<acadrust::Handle>, index: usize, material_id: String| {
                        let layers = participants.as_ref().and_then(|parts| {
                            if let Some(h) = wall {
                                parts.iter().find(|p| p.axis_handle == h).map(|p| &p.layers)
                            } else {
                                parts
                                    .iter()
                                    .find(|p| p.is_through)
                                    .or_else(|| {
                                        self.aec.aec_junction_editor_target.and_then(|(h, e)| {
                                            parts
                                                .iter()
                                                .find(|p| p.axis_handle == h && p.end_index == e)
                                                .or_else(|| {
                                                    parts.iter().find(|p| p.axis_handle == h)
                                                })
                                        })
                                    })
                                    .map(|p| &p.layers)
                            }
                        });
                        layers
                            .and_then(|ls| selected_junction_layer_ref(ls, index, &material_id))
                            .unwrap_or(LayerRef {
                                material_id,
                                role_tag: None,
                                index,
                                layer_id: None,
                            })
                    };
                    crate::modules::aec::engine::join::upsert_layer_gap(
                        &mut self.aec.aec_junction_editor_gaps,
                        crate::modules::aec::engine::join::LayerGapOverride {
                            layer: resolve_ref(None, li, lid),
                            from: resolve_ref(self.aec.aec_junction_editor_gap_from_wall, fi, fid),
                            to: resolve_ref(self.aec.aec_junction_editor_gap_to_wall, ti, tid),
                        },
                    );
                    self.aec.aec_junction_editor_gap_layer = None;
                    self.aec.aec_junction_editor_gap_from = None;
                    self.aec.aec_junction_editor_gap_to = None;
                    self.sync_junction_editor_layer_highlight();
                }
                Task::none()
            }
            AecMessage::AecJunctionEditorRemoveGap(idx) => {
                if idx < self.aec.aec_junction_editor_gaps.len() {
                    self.aec.aec_junction_editor_gaps.remove(idx);
                }
                Task::none()
            }

            AecMessage::AecJunctionEditorSave => {
                if let Some((axis_handle, end_index)) = self.aec.aec_junction_editor_target {
                    let i = self.active_tab;
                                        let override_data = crate::modules::aec::engine::join::JunctionOverride {
                        default_style: self.aec.aec_junction_editor_default_style.clone(),
                        layer_pairs: self.aec.aec_junction_editor_pairs.clone(),
                        layer_gaps: self.aec.aec_junction_editor_gaps.clone(),
                    };
                    let style_library =
                        crate::modules::aec::engine::project::resolve_style_library(
                            self.aec.aec_project_explorer_file.as_ref(),
                        );
                    let (display_rules, style_substitutions) =
                        self.resolve_active_display_config_wall_rules(i, Some(axis_handle));
                    let ov = if override_data.is_empty() {
                        None
                    } else {
                        Some(override_data)
                    };
                    let touched = join_ops::apply_junction_override_and_rebuild(
                        &mut self.tabs[i].scene,
                        axis_handle,
                        end_index,
                        ov.as_ref(),
                        Some(&style_library),
                        display_rules.as_ref(),
                        style_substitutions.as_ref(),
                    );
                    self.reapply_active_display_config_to_wall_packages(i, &touched);
                    self.refresh_properties();
                }
                self.aec.aec_junction_editor_target = None;
                self.active_modal = None;
                self.reset_modal_geometry();
                self.sync_junction_editor_layer_highlight();
                Task::none()
            }

            AecMessage::AecJunctionEditorFullReset => {
                if let Some((axis_handle, end_index)) = self.aec.aec_junction_editor_target {
                    let i = self.active_tab;
                                        let style_library =
                        crate::modules::aec::engine::project::resolve_style_library(
                            self.aec.aec_project_explorer_file.as_ref(),
                        );
                    let (display_rules, style_substitutions) =
                        self.resolve_active_display_config_wall_rules(i, Some(axis_handle));
                    let touched = join_ops::apply_junction_override_and_rebuild(
                        &mut self.tabs[i].scene,
                        axis_handle,
                        end_index,
                        None,
                        Some(&style_library),
                        display_rules.as_ref(),
                        style_substitutions.as_ref(),
                    );
                    self.reapply_active_display_config_to_wall_packages(i, &touched);
                    self.refresh_properties();
                }
                self.aec.aec_junction_editor_target = None;
                self.active_modal = None;
                self.reset_modal_geometry();
                self.sync_junction_editor_layer_highlight();
                Task::none()
            }
        }
    }
}
