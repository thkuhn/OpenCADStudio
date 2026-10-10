//! Project explorer persistence and storey mutation helpers.
//!
//! Bodies live here (still `impl OpenCADStudio`) so Core `update/mod.rs`
//! does not grow AEC project I/O logic.

use crate::app::{AecModalKind, Message, OpenCADStudio};

impl OpenCADStudio {
    /// Returns `true` when an AEC project is active. Otherwise opens the
    /// project-required modal and returns `false` so callers abort the
    /// requested entry point. In automation/headless sessions, seeds a blank
    /// in-memory project instead of opening an undismissable modal.
    ///
    /// `resume` is the message that would have started the requested tool
    /// (e.g. `Message::Command("AEC_WALL".into())`). It is remembered and
    /// replayed automatically once the user picks or creates a project from
    /// the modal, so the tool the user actually asked for starts right away
    /// instead of leaving them stuck after only the Project Explorer opens.
    pub(crate) fn aec_require_project(&mut self, resume: Message) -> bool {
        if self.aec.aec_project_explorer_file.is_some() {
            return true;
        }
        if self.is_automation_session() {
            self.aec.aec_project_explorer_file = Some(
                crate::modules::aec::engine::project::ProjectFile::default(),
            );
            self.aec.aec_plan_library = Some(
                crate::modules::aec::engine::project::resolve_display_config_library(
                    self.aec.aec_project_explorer_file.as_ref(),
                ),
            );
            return true;
        }
        self.aec.aec_project_required_resume = Some(resume);
        self.ribbon.close_dropdown();
        self.reset_modal_geometry();
        self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::ProjectRequired));
        false
    }

    /// Write the in-memory project to `aec_project_explorer_path` when both are set.
    pub(crate) fn aec_project_explorer_persist(&mut self) {
        let Some(path) = self.aec.aec_project_explorer_path.clone() else {
            return;
        };
        let Some(project) = self.aec.aec_project_explorer_file.as_ref() else {
            return;
        };
        match project.save(&path) {
            Ok(()) => {
                self.command_line.push_output(
                    crate::tf!("AEC Project Explorer: saved \"{}\"", path.display()).as_ref(),
                );
            }
            Err(e) => {
                self.command_line.push_error(
                    crate::tf!("AEC Project Explorer: save failed: {e}").as_ref(),
                );
            }
        }
    }

    /// Persist only when a path is already known (silent no-op otherwise).
    pub(crate) fn aec_project_explorer_persist_if_pathed(&mut self) {
        if self.aec.aec_project_explorer_path.is_some() {
            self.aec_project_explorer_persist();
        }
    }

    pub(crate) fn with_storey<R>(
        &self,
        bid: uuid::Uuid,
        sid: uuid::Uuid,
        f: impl FnOnce(&crate::modules::aec::engine::project::StoreyRef) -> R,
    ) -> Option<R> {
        let project = self.aec.aec_project_explorer_file.as_ref()?;
        let building = project
            .building_index(bid)
            .and_then(|bi| project.buildings.get(bi))?;
        let si = building.storey_index(sid)?;
        let storey = building.storeys.get(si)?;
        Some(f(storey))
    }

    pub(crate) fn with_storey_mut<R>(
        &mut self,
        bid: uuid::Uuid,
        sid: uuid::Uuid,
        f: impl FnOnce(&mut crate::modules::aec::engine::project::StoreyRef) -> R,
    ) -> Option<R> {
        let project = self.aec.aec_project_explorer_file.as_mut()?;
        let building = project
            .building_index(bid)
            .and_then(|bi| project.buildings.get_mut(bi))?;
        let si = building.storey_index(sid)?;
        let storey = building.storeys.get_mut(si)?;
        Some(f(storey))
    }

    /// Apply any not-yet-saved building/storey edit buffers (from the inline
    /// "Speichern" rows) to the in-memory project, so the top-level "Save"
    /// button captures everything the user typed, even if they never
    /// pressed the per-row "Speichern" button.
    pub(crate) fn aec_project_explorer_apply_pending_edits(&mut self) {
        if let Some(bid) = self.aec.aec_project_explorer_selected_building {
            let name = self.aec.aec_project_explorer_edit_building_name.clone();
            if let Some(project) = self.aec.aec_project_explorer_file.as_mut() {
                if let Some(building) = project
                    .building_index(bid)
                    .and_then(|bi| project.buildings.get_mut(bi))
                {
                    building.name = name;
                }
            }
        }
        if let Some((bid, sid)) = self.aec.aec_project_explorer_selected_storey {
            let name = self.aec.aec_project_explorer_edit_storey_name.clone();
            let drawing = self.aec.aec_project_explorer_edit_storey_drawing.clone();
            let elevation = self
                .aec.aec_project_explorer_edit_elevation
                .trim()
                .parse::<f64>()
                .ok();
            if let Some(project) = self.aec.aec_project_explorer_file.as_mut() {
                if let Some(storey) = project
                    .building_index(bid)
                    .and_then(|bi| project.buildings.get_mut(bi))
                    .and_then(|b| {
                        let si = b.storey_index(sid)?;
                        b.storeys.get_mut(si)
                    })
                {
                    storey.name = name;
                    storey.drawing_path = drawing;
                    if let Some(value) = elevation {
                        storey.elevation = value;
                    }
                }
            }
        }
    }

    /// Discovers and opens an AEC project for the drawing loaded in `tab_index` if one is found.
    ///
    /// If the drawing is part of a project, the project file, display configurations,
    /// and style libraries are loaded into memory, and the active storey is selected.
    pub(crate) fn aec_auto_open_project_for_tab(&mut self, tab_index: usize) {
        if tab_index >= self.tabs.len() || self.tabs[tab_index].is_start {
            return;
        }
        let Some(drawing_path) = self.tabs[tab_index].drawing_path() else {
            return;
        };

        let discovered =
            crate::modules::aec::engine::project::discover_project_for_drawing(drawing_path);
        let Some(project_path) = discovered else {
            return;
        };

        // If the project is already loaded with the same path, sync storey and return.
        if self.aec.aec_project_explorer_path.as_deref() == Some(project_path.as_path())
            && self.aec.aec_project_explorer_file.is_some()
        {
            self.aec_sync_active_storey_for_tab(tab_index);
            return;
        }

        match crate::modules::aec::engine::project::ProjectFile::load(&project_path) {
            Ok(project) => {
                self.aec.aec_project_explorer_ffl0_nn = project
                    .ffl0_nn_m
                    .map(|v| format!("{v}"))
                    .unwrap_or_default();
                self.aec.aec_project_explorer_file = Some(project);
                self.aec.aec_project_explorer_path = Some(project_path.clone());
                self.aec.aec_plan_library = Some(
                    crate::modules::aec::engine::project::resolve_display_config_library(
                        self.aec.aec_project_explorer_file.as_ref(),
                    ),
                );
                self.aec_sync_active_storey_for_tab(tab_index);
                self.aec_refresh_combined_style_library();

                let project_name = project_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| project_path.display().to_string());
                self.command_line.push_info(
                    crate::tr!(
                        "aec",
                        "project-auto-opened",
                        project = project_name.as_str()
                    )
                    .as_ref(),
                );
            }
            Err(e) => {
                self.command_line.push_error(
                    crate::tf!("AEC Project Explorer: failed to auto-open project: {e}").as_ref(),
                );
            }
        }
    }

    /// Synchronizes the selected building and storey in Project Explorer with the drawing
    /// open in `tab_index`.
    pub(crate) fn aec_sync_active_storey_for_tab(&mut self, tab_index: usize) {
        if tab_index >= self.tabs.len() || self.tabs[tab_index].is_start {
            return;
        }
        let Some(drawing_path) = self.tabs[tab_index].drawing_path() else {
            return;
        };
        let Some(project) = self.aec.aec_project_explorer_file.as_ref() else {
            return;
        };
        let base_dir = self
            .aec
            .aec_project_explorer_path
            .as_deref()
            .and_then(|p| p.parent());
        if let Some((building, storey)) =
            project.find_storey_by_drawing_path_with_base(base_dir, drawing_path)
        {
            let bid = building.id;
            let sid = storey.id;
            self.aec.aec_project_explorer_selected_building = Some(bid);
            self.aec.aec_project_explorer_selected_storey = Some((bid, sid));
            self.aec.aec_project_explorer_edit_building_name = building.name.clone();
            self.aec.aec_project_explorer_edit_storey_name = storey.name.clone();
            self.aec.aec_project_explorer_edit_elevation = format!("{:.3}", storey.elevation);
            self.aec.aec_project_explorer_edit_storey_drawing = storey.drawing_path.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::project::{Building, ProjectFile, StoreyRef};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_test_dir(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("opencadstudio_persist_test_{name}_{nanos}"));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn test_auto_open_project_when_tab_path_matches_storey() {
        let dir = temp_test_dir("auto_open_match");
        let storeys_dir = dir.join("storeys");
        fs::create_dir_all(&storeys_dir).expect("create subfolder");

        let proj_path = dir.join("haus.ocsproj");
        let eg_path = storeys_dir.join("EG.dwg");

        let mut project = ProjectFile::default();
        let mut building = Building::new("Haus");
        building
            .storeys
            .push(StoreyRef::new("EG", 0.0, "storeys/EG.dwg"));
        building
            .storeys
            .push(StoreyRef::new("OG", 2.8, "storeys/OG.dwg"));
        project.buildings.push(building);
        project.save(&proj_path).expect("save project");
        fs::write(&eg_path, b"dummy dwg").expect("write drawing");

        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].set_drawing_path_for_test(eg_path);

        assert!(app.aec.aec_project_explorer_file.is_none());
        assert!(app.aec.aec_project_explorer_path.is_none());

        app.aec_auto_open_project_for_tab(0);

        assert!(app.aec.aec_project_explorer_file.is_some());
        assert_eq!(
            app.aec
                .aec_project_explorer_path
                .as_ref()
                .and_then(|p| fs::canonicalize(p).ok()),
            fs::canonicalize(&proj_path).ok()
        );
        assert!(app.aec.aec_plan_library.is_some());
        assert!(app.aec.aec_project_explorer_selected_storey.is_some());
        assert_eq!(app.aec.aec_project_explorer_edit_storey_name, "EG");
        assert_eq!(app.aec.aec_project_explorer_edit_building_name, "Haus");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_auto_open_project_standalone_drawing_keeps_none() {
        let dir = temp_test_dir("auto_open_standalone");
        let standalone_dir = dir.join("isolated");
        fs::create_dir_all(&standalone_dir).expect("create isolated folder");
        let drawing_path = standalone_dir.join("standalone.dwg");
        fs::write(&drawing_path, b"dummy dwg").expect("write drawing");

        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].set_drawing_path_for_test(drawing_path);

        app.aec_auto_open_project_for_tab(0);

        assert!(app.aec.aec_project_explorer_file.is_none());
        assert!(app.aec.aec_project_explorer_path.is_none());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_sync_active_storey_on_tab_switch() {
        let dir = temp_test_dir("sync_storey_tab_switch");
        let storeys_dir = dir.join("storeys");
        fs::create_dir_all(&storeys_dir).expect("create subfolder");

        let proj_path = dir.join("projekt.ocsproj");
        let eg_path = storeys_dir.join("EG.dwg");
        let og_path = storeys_dir.join("OG.dwg");

        let mut project = ProjectFile::default();
        let mut building = Building::new("Hauptgebaeude");
        building
            .storeys
            .push(StoreyRef::new("EG", 0.0, "storeys/EG.dwg"));
        building
            .storeys
            .push(StoreyRef::new("OG", 3.2, "storeys/OG.dwg"));
        project.buildings.push(building);
        project.save(&proj_path).expect("save project");
        fs::write(&eg_path, b"dummy eg").expect("write eg");
        fs::write(&og_path, b"dummy og").expect("write og");

        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].set_drawing_path_for_test(eg_path);

        // Add second tab for OG
        app.push_test_document();
        app.tabs[1].set_drawing_path_for_test(og_path);

        // Auto-open on tab 0 (EG)
        app.aec_auto_open_project_for_tab(0);
        assert_eq!(app.aec.aec_project_explorer_edit_storey_name, "EG");
        assert_eq!(app.aec.aec_project_explorer_edit_elevation, "0.000");

        // Switch to tab 1 (OG)
        app.aec_sync_active_storey_for_tab(1);
        assert_eq!(app.aec.aec_project_explorer_edit_storey_name, "OG");
        assert_eq!(app.aec.aec_project_explorer_edit_elevation, "3.200");

        // Switch back to tab 0 (EG)
        app.aec_sync_active_storey_for_tab(0);
        assert_eq!(app.aec.aec_project_explorer_edit_storey_name, "EG");
        assert_eq!(app.aec.aec_project_explorer_edit_elevation, "0.000");

        let _ = fs::remove_dir_all(&dir);
    }
}
