//! AEC update handlers: Storey settings dialog: planes, facets, control-plane highlights.

use iced::Task;

use crate::app::{AecModalKind, Message, OpenCADStudio};
use crate::modules::aec::message::AecMessage;

impl OpenCADStudio {
    pub(crate) fn update_aec_storey_settings(&mut self, msg: AecMessage) -> Task<Message> {
        match msg {
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
                let tab = self.active_tab;
                let mut deleted = false;
                self.with_storey_mut(bid, sid, |s| {
                    deleted = crate::modules::aec::project::wall_planes::delete_control_plane_from_storey(s, pid);
                });
                if deleted {
                    crate::modules::aec::project::preview::erase_plane_previews_if_tagged(
                        &mut self.tabs[tab].scene,
                        pid,
                    );
                    self.aec.aec_storey_settings_plane_z.remove(&pid);
                    self.aec.aec_storey_settings_facet_z.retain(|(p, _), _| *p != pid);
                }
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
            AecMessage::AecStoreySettingsPickFacet(bid, sid, pid) => {
                self.commit_storey_settings_buffers(bid, sid);
                self.aec_project_explorer_persist_if_pathed();
                self.active_modal = None;
                let cmd = crate::modules::aec::project::plane_facet::PlaneFacetCommand::with_target(bid, sid, pid);
                self.command_line.push_info(&cmd.prompt());
                let tab = self.active_tab;
                self.tabs[tab].active_cmd = Some(Box::new(cmd));
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
                    crate::modules::aec::project::preview::erase_facet_preview_if_tagged(
                        &mut self.tabs[tab].scene,
                        pid,
                        h,
                    );
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
            _ => Task::none(),
        }
    }
}
