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
}
