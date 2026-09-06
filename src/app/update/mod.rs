use super::{AecPendingCopy, AecProjectExplorerDeleteTarget, ArrowKey, Message, OpenCADStudio};
use crate::scene::VIEWCUBE_DRAW_PX;
use crate::ui::PropertiesPanel;
use iced::time::Instant;
use iced::Task;

/// Keystroke-derived messages that an open modal dialog must swallow so the
/// keyboard can't reach the main window (command line, F-key toggles, edit
/// shortcuts) while a dialog is up. `CommandEscape` is handled separately (it
/// closes the modal); a modal's own text fields emit their own messages, which
/// are not in this set. See [`OpenCADStudio::update`] and #126.
fn is_modal_blocked_key_msg(msg: &Message) -> bool {
    matches!(
        msg,
        Message::CommandInput(_)
            | Message::CommandAppendChar(_)
            | Message::CommandSpace
            | Message::CommandFinalize
            | Message::CommandBackspace
            | Message::CommandHistoryPrev
            | Message::CommandHistoryNext
            | Message::ArrowKeyPressed { .. }
            | Message::CommandLineArrowProbe { .. }
            | Message::CommandLineArrowResolved { .. }
            | Message::DynTabNext
            | Message::MTextCaretMove(_)
            | Message::DeleteSelected
            | Message::ToggleSnapEnabled
            | Message::ToggleGrid
            | Message::ToggleOrtho
            | Message::ToggleGridSnap
            | Message::TogglePolar
            | Message::ToggleOTrack
            | Message::ToggleDynInput
            | Message::ShortcutPressed(_)
            | Message::TabNew
            | Message::OpenFile
            | Message::SaveFile
            | Message::SaveAs
            | Message::Undo
            | Message::Redo
            | Message::FindReplaceOpen
    )
}

fn perf_message_label(msg: &Message) -> &'static str {
    match msg {
        Message::ViewportLeftPress | Message::PanePress(_) => "pointer-down",
        Message::ViewportLeftRelease | Message::PaneRelease(_) => "pointer-up",
        Message::ViewportMove(_) | Message::PaneMove(_, _) => "pointer-move",
        Message::CommandFinalize => "command-finalize",
        Message::CommandEscape => "command-escape",
        Message::Undo | Message::UndoMany(_) => "undo",
        Message::Redo | Message::RedoMany(_) => "redo",
        Message::DeleteSelected => "delete",
        Message::HoverDwellTick => "hover-dwell",
        Message::InteractionIndexReady { .. } => "interaction-index-ready",
        _ => "other",
    }
}

const VIEWCUBE_HIT_SIZE: f32 = VIEWCUBE_DRAW_PX;

fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

fn reorder_insertion_index(from: usize, to: usize, after: bool, len: usize) -> Option<usize> {
    if from >= len || to >= len || from == to {
        return None;
    }
    let mut insertion = to + usize::from(after);
    if from < insertion {
        insertion -= 1;
    }
    (insertion != from).then_some(insertion)
}

mod command;
mod dialog;
mod dynamic;
mod file;
mod style;
mod util;
mod viewport;

impl OpenCADStudio {
    pub(in crate::app) fn reset_modal_geometry(&mut self) {
        self.modal_offset = iced::Vector::ZERO;
        self.modal_resize = iced::Vector::ZERO;
        self.modal_content_size = None;
        self.modal_drag_last = None;
        self.modal_dragging = false;
        self.modal_resizing = false;
    }

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
    pub(in crate::app) fn aec_require_project(&mut self, resume: Message) -> bool {
        if self.aec_project_explorer_file.is_some() {
            return true;
        }
        if self.automation_session {
            self.aec_project_explorer_file = Some(
                crate::modules::aec::engine::project::ProjectFile::default(),
            );
            self.aec_plan_library = Some(
                crate::modules::aec::engine::project::resolve_display_config_library(
                    self.aec_project_explorer_file.as_ref(),
                ),
            );
            return true;
        }
        self.aec_project_required_resume = Some(resume);
        self.ribbon.close_dropdown();
        self.reset_modal_geometry();
        self.active_modal = Some(super::ModalKind::AecProjectRequired);
        false
    }

    /// Write the in-memory project to `aec_project_explorer_path` when both are set.
    fn aec_project_explorer_persist(&mut self) {
        let Some(path) = self.aec_project_explorer_path.clone() else {
            return;
        };
        let Some(project) = self.aec_project_explorer_file.as_ref() else {
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
    fn aec_project_explorer_persist_if_pathed(&mut self) {
        if self.aec_project_explorer_path.is_some() {
            self.aec_project_explorer_persist();
        }
    }

    /// Persists `lib` as the effective material/wall-style library: into
    /// the loaded project (fanning out to every drawing/storey referencing
    /// it) when a project is loaded *and* pathed, otherwise falling back to
    /// the machine-wide global library file so standalone drawings (no
    /// project) keep working exactly as before.
    fn aec_save_style_library_preferring_project(
        &mut self,
        lib: &crate::modules::aec::engine::library::StyleLibrary,
    ) -> Result<(), String> {
        if let (Some(project), Some(path)) = (
            self.aec_project_explorer_file.as_mut(),
            self.aec_project_explorer_path.clone(),
        ) {
            crate::modules::aec::engine::project::save_style_library_to_project(
                project,
                &path,
                lib.clone(),
            )
            .map_err(|e| e.to_string())
        } else {
            crate::modules::aec::engine::library::save_to_default_path(lib)
        }
    }

    /// Upserts a single material into the active project's library (never the
    /// Standard library). Used for normal project edits and copy-on-write
    /// saves of Standard entries. Refreshes the combined in-memory view.
    fn aec_upsert_material_into_project(
        &mut self,
        material: crate::modules::aec::engine::material::Material,
    ) -> Result<(), String> {
        let path = self.aec_project_explorer_path.clone();
        let Some(project) = self.aec_project_explorer_file.as_mut() else {
            return Err("no project loaded".to_string());
        };
        project
            .material_wall_style_library
            .upsert_material(material);
        let lib = project.material_wall_style_library.clone();
        if let Some(path) = path {
            crate::modules::aec::engine::project::save_style_library_to_project(
                project, &path, lib,
            )
            .map_err(|e| e.to_string())?;
        }
        self.aec_style_library = Some(
            crate::modules::aec::engine::library::combined_style_library(
                self.aec_project_explorer_file.as_ref(),
            ),
        );
        Ok(())
    }

    /// Upserts a single wall style into the active project's library (never
    /// the Standard library). See [`Self::aec_upsert_material_into_project`].
    fn aec_upsert_wall_style_into_project(
        &mut self,
        wall_style: crate::modules::aec::engine::wall_style::WallStyle,
    ) -> Result<(), String> {
        let path = self.aec_project_explorer_path.clone();
        let Some(project) = self.aec_project_explorer_file.as_mut() else {
            return Err("no project loaded".to_string());
        };
        project
            .material_wall_style_library
            .upsert_wall_style(wall_style);
        let lib = project.material_wall_style_library.clone();
        if let Some(path) = path {
            crate::modules::aec::engine::project::save_style_library_to_project(
                project, &path, lib,
            )
            .map_err(|e| e.to_string())?;
        }
        self.aec_style_library = Some(
            crate::modules::aec::engine::library::combined_style_library(
                self.aec_project_explorer_file.as_ref(),
            ),
        );
        Ok(())
    }

    /// Persists `lib` as the effective `DisplayConfig` library, analogous
    /// to [`Self::aec_save_style_library_preferring_project`].
    fn aec_save_display_config_library_preferring_project(
        &mut self,
        lib: &crate::modules::aec::engine::library::DisplayConfigLibrary,
    ) -> Result<(), String> {
        if let (Some(project), Some(path)) = (
            self.aec_project_explorer_file.as_mut(),
            self.aec_project_explorer_path.clone(),
        ) {
            crate::modules::aec::engine::project::save_display_config_library_to_project(
                project,
                &path,
                lib.clone(),
            )
            .map_err(|e| e.to_string())
        } else {
            crate::modules::aec::engine::library::save_display_config_library_to_default_path(lib)
        }
    }

    /// Two-stage Phasenfilter-Editor (Step 5): loads `filter` (or the
    /// "unfiltered"/blank defaults if `None`) into the edit buffers backing
    /// the DisplayConfig form's phase-filter section.
    fn aec_plan_manager_load_phase_filter_buffers(
        &mut self,
        filter: Option<&crate::modules::aec::engine::plan_view::PhaseFilter>,
    ) {
        use crate::modules::aec::engine::plan_view::PlanPhase;
        let visible = |phase: PlanPhase| match filter {
            Some(f) => f.visible_phases.contains(&phase),
            None => true,
        };
        self.aec_plan_manager_phase_filter_visible_existing = visible(PlanPhase::Existing);
        self.aec_plan_manager_phase_filter_visible_demolition = visible(PlanPhase::Demolition);
        self.aec_plan_manager_phase_filter_visible_new = visible(PlanPhase::New);

        let demolition = filter.and_then(|f| f.demolition_style.clone()).unwrap_or_default();
        self.aec_plan_manager_demolition_style_line_type =
            demolition.line_type.clone().unwrap_or_default();
        use crate::ui::window::aec_ui_util::acad_color_to_editor_string;
        self.aec_plan_manager_demolition_style_line_color = demolition
            .line_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();
        self.aec_plan_manager_demolition_style_hatch_pattern =
            demolition.hatch_pattern.clone().unwrap_or_default();
        self.aec_plan_manager_demolition_style_hatch_color = demolition
            .hatch_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();
        self.aec_plan_manager_demolition_style_fill_color = demolition
            .fill_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();

        let existing = filter.and_then(|f| f.existing_style.clone()).unwrap_or_default();
        self.aec_plan_manager_existing_style_line_type =
            existing.line_type.clone().unwrap_or_default();
        self.aec_plan_manager_existing_style_line_color = existing
            .line_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();
        self.aec_plan_manager_existing_style_hatch_pattern =
            existing.hatch_pattern.clone().unwrap_or_default();
        self.aec_plan_manager_existing_style_hatch_color = existing
            .hatch_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();
        self.aec_plan_manager_existing_style_fill_color = existing
            .fill_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();
    }

    /// Two-stage Phasenfilter-Editor (Step 5): builds a [`PhaseFilter`]
    /// from the current edit buffers. Returns `None` when every phase is
    /// visible and neither style overlay is set — the "unfiltered"/legacy
    /// default — so a config left untouched keeps `phase_filter == None`.
    fn aec_plan_manager_build_phase_filter(
        &self,
    ) -> Option<crate::modules::aec::engine::plan_view::PhaseFilter> {
        use crate::modules::aec::engine::display_component::component_style_override_from_editor_fields;
        use crate::modules::aec::engine::plan_view::PlanPhase;

        let mut visible_phases = Vec::new();
        if self.aec_plan_manager_phase_filter_visible_existing {
            visible_phases.push(PlanPhase::Existing);
        }
        if self.aec_plan_manager_phase_filter_visible_demolition {
            visible_phases.push(PlanPhase::Demolition);
        }
        if self.aec_plan_manager_phase_filter_visible_new {
            visible_phases.push(PlanPhase::New);
        }

        let demolition_style = component_style_override_from_editor_fields(
            &self.aec_plan_manager_demolition_style_line_type,
            &self.aec_plan_manager_demolition_style_line_color,
            &self.aec_plan_manager_demolition_style_hatch_pattern,
            &self.aec_plan_manager_demolition_style_hatch_color,
            &self.aec_plan_manager_demolition_style_fill_color,
        );
        let demolition_style = (demolition_style != Default::default()).then_some(demolition_style);

        let existing_style = component_style_override_from_editor_fields(
            &self.aec_plan_manager_existing_style_line_type,
            &self.aec_plan_manager_existing_style_line_color,
            &self.aec_plan_manager_existing_style_hatch_pattern,
            &self.aec_plan_manager_existing_style_hatch_color,
            &self.aec_plan_manager_existing_style_fill_color,
        );
        let existing_style = (existing_style != Default::default()).then_some(existing_style);

        let all_visible = visible_phases.len() == 3;
        if all_visible && demolition_style.is_none() && existing_style.is_none() {
            return None;
        }

        Some(crate::modules::aec::engine::plan_view::PhaseFilter {
            visible_phases,
            demolition_style,
            existing_style,
        })
    }

    fn aec_plan_manager_clear_overlay_field_buffers(&mut self) {
        self.aec_plan_manager_overlay_line_type.clear();
        self.aec_plan_manager_overlay_line_color.clear();
        self.aec_plan_manager_overlay_hatch_pattern.clear();
        self.aec_plan_manager_overlay_hatch_color.clear();
        self.aec_plan_manager_overlay_hatch_scale.clear();
        self.aec_plan_manager_overlay_hatch_angle.clear();
        self.aec_plan_manager_overlay_hatch_angle_relative = None;
        self.aec_plan_manager_overlay_fill_color.clear();
    }

    fn aec_plan_manager_override_from_hatch_buffers(
        pattern: &str,
        color: &str,
        scale: &str,
        angle: &str,
        relative: Option<bool>,
    ) -> crate::modules::aec::engine::display_component::ComponentStyleOverride {
        use crate::ui::window::aec_ui_util::editor_string_to_acad_color;
        let nonempty = |s: &str| {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        };
        crate::modules::aec::engine::display_component::ComponentStyleOverride {
            hatch_pattern: nonempty(pattern),
            hatch_color: editor_string_to_acad_color(color),
            hatch_scale: scale.trim().parse::<f64>().ok().filter(|s| *s > 0.0),
            hatch_angle: angle.trim().parse::<f64>().ok(),
            hatch_angle_relative: relative,
            ..Default::default()
        }
    }

    fn aec_plan_manager_load_contour_hatch_buffers(
        &mut self,
        hatch: Option<&crate::modules::aec::engine::display_component::ComponentStyleOverride>,
    ) {
        use crate::ui::window::aec_ui_util::acad_color_to_editor_string;
        let hatch = hatch.cloned().unwrap_or_default();
        self.aec_plan_manager_contour_hatch_pattern =
            hatch.hatch_pattern.clone().unwrap_or_default();
        self.aec_plan_manager_contour_hatch_color = hatch
            .hatch_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();
        self.aec_plan_manager_contour_hatch_scale = hatch
            .hatch_scale
            .map(|s| s.to_string())
            .unwrap_or_default();
        self.aec_plan_manager_contour_hatch_angle = hatch
            .hatch_angle
            .map(|a| a.to_string())
            .unwrap_or_default();
        self.aec_plan_manager_contour_hatch_angle_relative = hatch.hatch_angle_relative;
    }

    fn aec_plan_manager_reset_display_buffers(&mut self) {
        self.aec_plan_manager_editing_id = None;
        self.aec_plan_manager_default_representation =
            crate::modules::aec::engine::display_component::RepresentationMode::All;
        self.aec_plan_manager_component_visibility.clear();
        self.aec_plan_manager_style_overlays.clear();
        self.aec_plan_manager_overlay_style_id = None;
        self.aec_plan_manager_overlay_layer_id = None;
        self.aec_plan_manager_clear_overlay_field_buffers();
        self.aec_plan_manager_load_contour_hatch_buffers(None);
    }

    fn aec_plan_manager_load_display_buffers(
        &mut self,
        cfg: &crate::modules::aec::engine::plan_view::DisplayConfig,
    ) {
        self.aec_plan_manager_editing_id = Some(cfg.id);
        self.aec_plan_manager_default_representation = cfg.default_representation;
        self.aec_plan_manager_component_visibility = cfg.component_visibility.clone();
        self.aec_plan_manager_style_overlays = cfg.style_overlays.clone();
        self.aec_plan_manager_overlay_style_id = cfg.style_overlays.keys().next().cloned();
        self.aec_plan_manager_overlay_layer_id = self
            .aec_plan_manager_overlay_style_id
            .as_ref()
            .and_then(|sid| {
                self.aec_plan_manager_style_overlays
                    .get(sid)
                    .and_then(|o| o.layer_props.keys().next().copied())
            });
        self.aec_plan_manager_load_overlay_layer_buffers();
        self.aec_plan_manager_load_overlay_contour_hatch_buffers();
    }

    fn aec_plan_manager_load_overlay_layer_buffers(&mut self) {
        use crate::ui::window::aec_ui_util::acad_color_to_editor_string;
        let props = self
            .aec_plan_manager_overlay_style_id
            .as_ref()
            .and_then(|sid| self.aec_plan_manager_style_overlays.get(sid))
            .and_then(|o| {
                self.aec_plan_manager_overlay_layer_id
                    .and_then(|lid| o.layer_props.get(&lid))
            })
            .cloned()
            .unwrap_or_default();
        self.aec_plan_manager_overlay_line_type = props.line_type.clone().unwrap_or_default();
        self.aec_plan_manager_overlay_line_color = props
            .line_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();
        self.aec_plan_manager_overlay_hatch_pattern =
            props.hatch_pattern.clone().unwrap_or_default();
        self.aec_plan_manager_overlay_hatch_color = props
            .hatch_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();
        self.aec_plan_manager_overlay_hatch_scale = props
            .hatch_scale
            .map(|s| s.to_string())
            .unwrap_or_default();
        self.aec_plan_manager_overlay_hatch_angle = props
            .hatch_angle
            .map(|a| a.to_string())
            .unwrap_or_default();
        self.aec_plan_manager_overlay_hatch_angle_relative = props.hatch_angle_relative;
        self.aec_plan_manager_overlay_fill_color = props
            .fill_color
            .map(acad_color_to_editor_string)
            .unwrap_or_default();
    }

    fn aec_plan_manager_write_overlay_buffers(&mut self) {
        use crate::modules::aec::engine::display_component::ComponentStyleOverride;
        use crate::ui::window::aec_ui_util::editor_string_to_acad_color;
        let Some(style_id) = self.aec_plan_manager_overlay_style_id.clone() else {
            return;
        };
        let Some(layer_id) = self.aec_plan_manager_overlay_layer_id else {
            return;
        };
        let nonempty = |s: &str| {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        };
        let props = ComponentStyleOverride {
            line_type: nonempty(&self.aec_plan_manager_overlay_line_type),
            line_color: editor_string_to_acad_color(&self.aec_plan_manager_overlay_line_color),
            hatch_pattern: nonempty(&self.aec_plan_manager_overlay_hatch_pattern),
            hatch_color: editor_string_to_acad_color(&self.aec_plan_manager_overlay_hatch_color),
            hatch_scale: self
                .aec_plan_manager_overlay_hatch_scale
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|s| *s > 0.0),
            fill_color: editor_string_to_acad_color(&self.aec_plan_manager_overlay_fill_color),
            hatch_angle: self
                .aec_plan_manager_overlay_hatch_angle
                .trim()
                .parse::<f64>()
                .ok(),
            hatch_angle_relative: self.aec_plan_manager_overlay_hatch_angle_relative,
            cad_layer: None,
        };
        let overlay = self
            .aec_plan_manager_style_overlays
            .entry(style_id)
            .or_default();
        if props == ComponentStyleOverride::default() {
            overlay.layer_props.remove(&layer_id);
        } else {
            overlay.layer_props.insert(layer_id, props);
        }
    }

    fn aec_plan_manager_load_overlay_contour_hatch_buffers(&mut self) {
        let hatch = self
            .aec_plan_manager_overlay_style_id
            .as_ref()
            .and_then(|sid| self.aec_plan_manager_style_overlays.get(sid))
            .and_then(|o| o.contour_hatch.clone());
        self.aec_plan_manager_load_contour_hatch_buffers(hatch.as_ref());
    }

    fn aec_plan_manager_write_overlay_contour_hatch(&mut self) {
        let Some(style_id) = self.aec_plan_manager_overlay_style_id.clone() else {
            return;
        };
        let contour = Self::aec_plan_manager_override_from_hatch_buffers(
            &self.aec_plan_manager_contour_hatch_pattern,
            &self.aec_plan_manager_contour_hatch_color,
            &self.aec_plan_manager_contour_hatch_scale,
            &self.aec_plan_manager_contour_hatch_angle,
            self.aec_plan_manager_contour_hatch_angle_relative,
        );
        let overlay = self
            .aec_plan_manager_style_overlays
            .entry(style_id)
            .or_default();
        overlay.contour_hatch = if contour
            == crate::modules::aec::engine::display_component::ComponentStyleOverride::default()
        {
            None
        } else {
            Some(contour)
        };
    }

    /// Copies the currently-selected material between the project and
    /// global libraries (`to_project == true` copies global→project,
    /// `false` copies project→global). Shows an overwrite confirmation
    /// first if the target already holds a different entry with the same
    /// id (Step 9).
    fn aec_handle_copy_material(&mut self, to_project: bool) -> Task<Message> {
        if to_project && self.aec_project_explorer_file.is_none() {
            return Task::none();
        }
        let Some(id) = self.aec_style_manager_selected_material.clone() else {
            return Task::none();
        };
        let global_lib = crate::modules::aec::engine::library::load_or_seed();
        let project_lib = crate::modules::aec::engine::project::resolve_style_library(
            self.aec_project_explorer_file.as_ref(),
        );
        let (source_lib, target_lib) = if to_project {
            (&global_lib, &project_lib)
        } else {
            (&project_lib, &global_lib)
        };
        let Some(material) = source_lib.materials.iter().find(|m| m.id == id).cloned() else {
            return Task::none();
        };
        let conflict =
            crate::modules::aec::engine::library::material_copy_conflict(target_lib, &material);
        match conflict {
            crate::modules::aec::engine::library::CopyConflict::DifferentContentCollision => {
                self.aec_style_manager_pending_copy = Some(AecPendingCopy::Material {
                    material,
                    to_project,
                });
                self.aec_style_manager_copy_conflict_open = true;
                self.active_modal = Some(crate::app::ModalKind::AecStyleCopyConflict);
            }
            _ => {
                self.aec_execute_copy(AecPendingCopy::Material {
                    material,
                    to_project,
                });
            }
        }
        Task::none()
    }

    /// Copies the currently-selected wall style between the project and
    /// global libraries; see [`Self::aec_handle_copy_material`] for the
    /// direction convention and overwrite-confirmation behavior.
    fn aec_handle_copy_wall_style(&mut self, to_project: bool) -> Task<Message> {
        if to_project && self.aec_project_explorer_file.is_none() {
            return Task::none();
        }
        let Some(id) = self.aec_style_manager_selected_wall_style.clone() else {
            return Task::none();
        };
        let global_lib = crate::modules::aec::engine::library::load_or_seed();
        let project_lib = crate::modules::aec::engine::project::resolve_style_library(
            self.aec_project_explorer_file.as_ref(),
        );
        let (source_lib, target_lib) = if to_project {
            (&global_lib, &project_lib)
        } else {
            (&project_lib, &global_lib)
        };
        let Some(wall_style) = source_lib
            .wall_styles
            .iter()
            .find(|w| w.style.id == id)
            .cloned()
        else {
            return Task::none();
        };
        let conflict = crate::modules::aec::engine::library::wall_style_copy_conflict(
            target_lib,
            &wall_style,
        );
        match conflict {
            crate::modules::aec::engine::library::CopyConflict::DifferentContentCollision => {
                self.aec_style_manager_pending_copy = Some(AecPendingCopy::WallStyle {
                    wall_style,
                    to_project,
                });
                self.aec_style_manager_copy_conflict_open = true;
                self.active_modal = Some(crate::app::ModalKind::AecStyleCopyConflict);
            }
            _ => {
                self.aec_execute_copy(AecPendingCopy::WallStyle {
                    wall_style,
                    to_project,
                });
            }
        }
        Task::none()
    }

    /// Performs a confirmed (or conflict-free) copy: upserts the entry
    /// into the target library, persists it (project or global, following
    /// [`Self::aec_save_style_library_preferring_project`]'s target
    /// resolution), and refreshes the in-memory `aec_style_library` so the
    /// manager UI reflects the change immediately.
    fn aec_execute_copy(&mut self, pending: AecPendingCopy) {
        let global_lib_before = crate::modules::aec::engine::library::load_or_seed();
        let project_lib_before = crate::modules::aec::engine::project::resolve_style_library(
            self.aec_project_explorer_file.as_ref(),
        );
        let (mut target_lib, to_project, label) = match &pending {
            AecPendingCopy::Material { to_project, .. } => {
                let lib = if *to_project {
                    project_lib_before
                } else {
                    global_lib_before
                };
                (lib, *to_project, crate::t!("material"))
            }
            AecPendingCopy::WallStyle { to_project, .. } => {
                let lib = if *to_project {
                    project_lib_before
                } else {
                    global_lib_before
                };
                (lib, *to_project, crate::t!("wall style"))
            }
        };
        match &pending {
            AecPendingCopy::Material { material, .. } => {
                target_lib.upsert_material(material.clone());
            }
            AecPendingCopy::WallStyle {
                wall_style,
                to_project,
                ..
            } => {
                let mut ws = wall_style.clone();
                if !*to_project {
                    ws = crate::modules::aec::engine::library::wall_style_without_display_profiles(
                        ws,
                    );
                }
                target_lib.upsert_wall_style(ws);
            }
        }
        let save_result = if to_project {
            if let (Some(project), Some(path)) = (
                self.aec_project_explorer_file.as_mut(),
                self.aec_project_explorer_path.clone(),
            ) {
                crate::modules::aec::engine::project::save_style_library_to_project(
                    project,
                    &path,
                    target_lib.clone(),
                )
                .map_err(|e| e.to_string())
            } else {
                Err("no project loaded".to_string())
            }
        } else {
            crate::modules::aec::engine::library::save_to_default_path(&target_lib)
        };
        match save_result {
            Ok(()) => {
                self.command_line.push_info(
                    crate::tf!("AEC Style Manager: {label} copied.").as_ref(),
                );
            }
            Err(e) => {
                self.command_line.push_error(
                    crate::tf!("AEC Style Manager: failed to copy {label}: {e}").as_ref(),
                );
            }
        }
        // Refresh the in-memory manager library so the UI reflects the copy
        // immediately without requiring a manager reopen.
        self.aec_style_library = Some(
            crate::modules::aec::engine::library::combined_style_library(
                self.aec_project_explorer_file.as_ref(),
            ),
        );
    }

    /// Apply any not-yet-saved building/storey edit buffers (from the inline
    /// "Speichern" rows) to the in-memory project, so the top-level "Save"
    /// button captures everything the user typed, even if they never
    /// pressed the per-row "Speichern" button.
    fn aec_project_explorer_apply_pending_edits(&mut self) {
        if let Some(bid) = self.aec_project_explorer_selected_building {
            let name = self.aec_project_explorer_edit_building_name.clone();
            if let Some(project) = self.aec_project_explorer_file.as_mut() {
                if let Some(building) = project
                    .building_index(bid)
                    .and_then(|bi| project.buildings.get_mut(bi))
                {
                    building.name = name;
                }
            }
        }
        if let Some((bid, sid)) = self.aec_project_explorer_selected_storey {
            let name = self.aec_project_explorer_edit_storey_name.clone();
            let drawing = self.aec_project_explorer_edit_storey_drawing.clone();
            let elevation = self
                .aec_project_explorer_edit_elevation
                .trim()
                .parse::<f64>()
                .ok();
            if let Some(project) = self.aec_project_explorer_file.as_mut() {
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

    /// Resolves `tabs[tab_index]`'s active `DisplayConfig` (`active_display_config`
    /// + `aec_plan_library`), if any, into the wall `ComponentRuleSet` and
    /// `style_substitutions` map that must be threaded through wall-mutating
    /// regenerations (join/extend/reverse/opening as well as style
    /// assignment) so they honor the currently active Planart. Returns owned
    /// data (rather than borrowing `self.tabs`/`self.aec_plan_library`) so
    /// callers can resolve this once and still freely borrow `self.tabs[i]`
    /// mutably afterwards.
    pub(crate) fn resolve_active_display_config_wall_rules(
        &self,
        tab_index: usize,
        wall_handle: Option<acadrust::Handle>,
    ) -> (
        Option<crate::modules::aec::engine::display_component::ComponentRuleSet>,
        Option<std::collections::HashMap<
            crate::modules::aec::engine::plan_view::WallStyleRef,
            crate::modules::aec::engine::plan_view::WallStyleRef,
        >>,
    ) {
        // `DisplayConfig::component_rules`/`style_substitutions` were
        // removed in Step 2 (moved to `WallStyle::display_profiles`, keyed
        // per wall style rather than per `DisplayConfig`). Since overrides
        // now live on the *wall style* rather than on the `DisplayConfig`,
        // resolving them requires knowing which style this particular wall
        // uses; `style_substitutions` has no successor concept (removed
        // without migration), so it's always `None` from here on.
        let session = self.tabs[tab_index].representation_override;
        let config_name = self.tabs[tab_index].active_display_config.clone();
        let Some(wall_handle) = wall_handle else {
            return (None, None);
        };
        let Some(entity) = self.tabs[tab_index].scene.document.get_entity(wall_handle) else {
            return (None, None);
        };
        let Some(wall) = crate::modules::aec::commands::wall_from_entity(entity) else {
            return (None, None);
        };
        let style_library = crate::modules::aec::engine::project::resolve_style_library(
            self.aec_project_explorer_file.as_ref(),
        );
        let style = style_library
            .wall_styles
            .iter()
            .find(|ws| ws.style.id == wall.style_id);
        let config = config_name.as_deref().and_then(|name| {
            self.aec_plan_library
                .as_ref()
                .and_then(|lib| lib.find(name))
                .cloned()
        });
        let config = config.unwrap_or_else(|| {
            crate::modules::aec::engine::plan_view::DisplayConfig::new(
                String::new(),
                String::new(),
                crate::modules::aec::engine::plan_view::PlanningStage::Design,
                crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
            )
        });
        let mut rules = crate::modules::aec::engine::library::build_effective_rule_set(
            &config,
            style,
            session,
        );
        let filter_result = crate::modules::aec::commands::apply_phase_filter(
            wall.phase,
            config.phase_filter.as_ref(),
        );
        if !filter_result.visible {
            for slot in [
                crate::modules::aec::engine::display_component::WallComponentSlot::Contour2D,
                crate::modules::aec::engine::display_component::WallComponentSlot::Layers2D,
                crate::modules::aec::engine::display_component::WallComponentSlot::ContourHatch2D,
                crate::modules::aec::engine::display_component::WallComponentSlot::LayerHatch2D,
                crate::modules::aec::engine::display_component::WallComponentSlot::Solid3D,
            ] {
                rules.visibility.insert(slot.key().to_string(), false);
            }
        } else if let Some(ov) = filter_result.extra_style {
            let slot = crate::modules::aec::engine::display_component::WallComponentSlot::Contour2D
                .key()
                .to_string();
            rules
                .style_override
                .entry(slot)
                .or_default()
                .overlay_from(&ov);
        }
        (Some(rules), None)
    }

    fn apply_active_display_config_to_tab(&mut self, tab_index: usize) {
        if self.tabs[tab_index].is_start {
            return;
        }
        let session = self.tabs[tab_index].representation_override;
        let name = self.tabs[tab_index].active_display_config.clone();
        let style_library = crate::modules::aec::engine::project::resolve_style_library(
            self.aec_project_explorer_file.as_ref(),
        );
        let config = name.as_deref().and_then(|n| {
            self.aec_plan_library
                .get_or_insert_with(|| {
                    crate::modules::aec::engine::project::resolve_display_config_library(
                        self.aec_project_explorer_file.as_ref(),
                    )
                })
                .find(n)
                .cloned()
        });
        let Some(config) = config.or_else(|| {
            session.map(|_| {
                crate::modules::aec::engine::plan_view::DisplayConfig::new(
                    String::new(),
                    String::new(),
                    crate::modules::aec::engine::plan_view::PlanningStage::Design,
                    crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
                )
            })
        }) else {
            return;
        };
        crate::modules::aec::commands::apply_display_config_to_scene_with_representation(
            &mut self.tabs[tab_index].scene,
            &config,
            Some(&style_library),
            session,
        );
    }

    /// Regenerates a single wall's representation the same way
    /// [`crate::modules::aec::commands::regenerate_wall_representation`]
    /// does, but additionally honors `tabs[tab_index]`'s active
    /// `DisplayConfig` (`active_display_config` + `aec_plan_library`), if
    /// any — mirroring what [`crate::modules::aec::commands::apply_display_config_to_scene`]
    /// does when a `DisplayConfig` is (re-)applied to the whole document.
    /// Without this, per-wall regenerations triggered from the AEC Style
    /// Manager (assigning/saving a wall style) would silently ignore the
    /// currently active plan and fall back to the default, fully-visible
    /// representation.
    fn regenerate_wall_respecting_active_display_config(
        &mut self,
        tab_index: usize,
        wall_handle: acadrust::Handle,
    ) -> Result<Vec<acadrust::Handle>, crate::modules::aec::commands::WallRegenError> {
        let style_library = crate::modules::aec::engine::project::resolve_style_library(
            self.aec_project_explorer_file.as_ref(),
        );
        let (rules, substitutions) =
            self.resolve_active_display_config_wall_rules(tab_index, Some(wall_handle));
        crate::modules::aec::commands::regenerate_wall_representation_with_rules_and_substitutions(
            &mut self.tabs[tab_index].scene,
            wall_handle,
            rules.as_ref(),
            substitutions.as_ref(),
            Some(&style_library),
        )
    }

    /// After a join (or a new wall segment that auto-joins), rebuild each
    /// participating wall with *its* Planart/style profile. Shared join
    /// regenerations often pass `None` or the first wall's rules, which
    /// would otherwise drop the active display configuration.
    pub(in crate::app) fn reapply_active_display_config_to_wall_packages(
        &mut self,
        tab_index: usize,
        seeds: &[acadrust::Handle],
    ) {
        use crate::modules::aec::commands as aec_cmds;
        let mut walls: Vec<acadrust::Handle> = Vec::new();
        for &seed in seeds {
            let axis = aec_cmds::resolve_wall_package(&self.tabs[tab_index].scene, seed);
            if axis.is_null() {
                continue;
            }
            if self.tabs[tab_index]
                .scene
                .document
                .get_entity(axis)
                .and_then(aec_cmds::wall_from_entity)
                .is_none()
            {
                continue;
            }
            if !walls.contains(&axis) {
                walls.push(axis);
            }
            for peer in crate::modules::aec::engine::owner_index::peers_of(
                &self.tabs[tab_index].scene.document,
                axis,
            ) {
                if !walls.contains(&peer) {
                    walls.push(peer);
                }
            }
        }
        if walls.is_empty() {
            return;
        }
        let mut touched = Vec::new();
        for wall in walls {
            if let Ok(handles) =
                self.regenerate_wall_respecting_active_display_config(tab_index, wall)
            {
                touched.extend(handles);
            }
        }
        touched.sort_by_key(|h| h.value());
        touched.dedup();
        let changes: Vec<_> = touched
            .into_iter()
            .filter(|h| self.tabs[tab_index].scene.document.get_entity(*h).is_some())
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect();
        if !changes.is_empty() {
            self.tabs[tab_index].scene.bump_entities(&changes);
        }
    }

    fn aec_style_manager_wall_style_save_internal(
        &mut self,
    ) -> Option<(String, crate::modules::aec::engine::library::StyleLibrary)> {
        use crate::modules::aec::engine::wall_style::LayerValue;
        // Wall style layer fields are shown/edited in the Style Manager in
        // centimeters, while the underlying data model (and all geometry
        // formulas, e.g. `BB`) stay in meters; `LayerValue::parse_cm_str`
        // converts fixed numeric input back to meters, formula strings are
        // left untouched since they already operate on meter-based vars.
        fn cm_to_m(s: &str) -> f64 {
            s.trim().parse::<f64>().unwrap_or(0.0) / 100.0
        }
        let name = self.aec_style_manager_wall_style_name.trim().to_string();
        if name.is_empty() {
            self.command_line.push_error(
                crate::t!("AEC Style Manager: wall style name cannot be empty.").as_ref(),
            );
            return None;
        }

        // A brand-new style gets a fresh, globally unique id (name +
        // random suffix) instead of a purely name-derived one, so two
        // styles created independently (e.g. in different projects) that
        // happen to share a name never collide on the same id later. Ids
        // of styles already being edited are left untouched.
        let id = self
            .aec_style_manager_wall_style_editing_id
            .clone()
            .unwrap_or_else(|| crate::modules::aec::commands::unique_id("style", &name));

        // Cycle detection before saving
        if let Some(parent_id) = &self.aec_style_manager_wall_style_parent {
            if parent_id == &id {
                self.command_line.push_error(
                    crate::t!("AEC Style Manager: a wall style cannot be its own parent.")
                        .as_ref(),
                );
                return None;
            }

            if let Some(lib) = &self.aec_style_library {
                // Create a temporary style map for resolve_chain
                let mut styles = std::collections::HashMap::new();
                for ws in &lib.wall_styles {
                    if ws.style.id != id {
                        styles.insert(ws.style.id.clone(), ws.style.clone());
                    }
                }
                // Insert the proposed state
                styles.insert(
                    id.clone(),
                    crate::modules::aec::engine::style::Style {
                        id: id.clone(),
                        name: name.clone(),
                        object_kind: "Wall".to_string(),
                        parent_style_id: Some(parent_id.clone()),
                    },
                );

                if let Err(crate::modules::aec::engine::style::StyleError::CycleDetected) =
                    crate::modules::aec::engine::style::resolve_chain(&styles, &id)
                {
                    self.command_line.push_error(
                        crate::t!("AEC Style Manager: cycle detected in wall style inheritance.")
                            .as_ref(),
                    );
                    return None;
                }
            }
        }

        let mut layers = Vec::new();
        let mut formula_warnings = Vec::new();
        for (idx, lb) in self.aec_style_manager_wall_style_layers.iter().enumerate() {
            let thickness = LayerValue::parse_cm_str(&lb.thickness);
            if let crate::modules::aec::engine::wall_style::LayerValue::Formula(ref formula) =
                thickness
            {
                // Validate with a dummy BB so the user gets feedback without
                // blocking save (runtime still falls back safely).
                let vars = crate::modules::aec::engine::wall_style::wall_vars(1.0);
                if let Err(e) =
                    crate::modules::aec::engine::expr::eval_formula(formula, &vars)
                {
                    formula_warnings.push(format!("layer {}: {e}", idx + 1));
                }
            }
            let function = match lb.function.as_str() {
                "Structural" => crate::modules::aec::engine::wall_style::LayerFunction::Structural,
                "Insulation" => crate::modules::aec::engine::wall_style::LayerFunction::Insulation,
                "Finish" => crate::modules::aec::engine::wall_style::LayerFunction::Finish,
                other => {
                    crate::modules::aec::engine::wall_style::LayerFunction::Other(other.to_string())
                }
            };
            layers.push(crate::modules::aec::engine::wall_style::Layer {
                material_id: lb.material_id.clone(),
                thickness,
                function,
                axis_offset: LayerValue::parse_cm_str(&lb.axis_offset),
                bottom_offset: cm_to_m(&lb.bottom_offset),
                top_offset: cm_to_m(&lb.top_offset),
                layer_override: if lb.layer_override.trim().is_empty() {
                    None
                } else {
                    Some(lb.layer_override.trim().to_string())
                },
                hatch_override: if lb.hatch_override.trim().is_empty() {
                    None
                } else {
                    Some(lb.hatch_override.trim().to_string())
                },
                role_tag: if lb.role_tag.trim().is_empty() {
                    None
                } else {
                    Some(lb.role_tag.trim().to_string())
                },
                layer_id: lb.layer_id.unwrap_or_else(uuid::Uuid::new_v4),
            });
        }
        for w in formula_warnings {
            self.command_line.push_error(
                crate::tf!("AEC Style Manager: invalid thickness formula ({w})").as_ref(),
            );
        }

        // Preserve any `display_profiles` already saved on this style (Step
        // 4's editor writes those separately) — this save path only edits
        // layers/parenting, so it must not silently wipe existing overrides.
        let existing_display_profiles = self
            .aec_style_library
            .as_ref()
            .and_then(|lib| lib.wall_styles.iter().find(|ws| ws.style.id == id))
            .map(|ws| ws.display_profiles.clone())
            .unwrap_or_default();

        let wall_style = crate::modules::aec::engine::wall_style::WallStyle {
            style: crate::modules::aec::engine::style::Style {
                id: id.clone(),
                name,
                object_kind: "Wall".to_string(),
                parent_style_id: self.aec_style_manager_wall_style_parent.clone(),
            },
            layers,
            display_profiles: existing_display_profiles,
        };

        // Copy-on-write: editing a Standard wall style lands in the project.
        let source = crate::modules::aec::engine::library::wall_style_library_source(
            self.aec_project_explorer_file.as_ref(),
            &id,
        );
        let cow_from_standard = source
            == Some(crate::modules::aec::engine::library::LibrarySource::Standard);

        if self.aec_project_explorer_file.is_some() {
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
                            crate::t!("AEC Style Manager: wall style saved.").as_ref(),
                        );
                    }
                    self.aec_style_manager_selected_wall_style = Some(id.clone());
                    self.aec_style_manager_wall_style_editing_id = Some(id.clone());
                    let lib_snapshot = self
                        .aec_style_library
                        .clone()
                        .unwrap_or_else(crate::modules::aec::engine::library::StyleLibrary::empty);
                    Some((id, lib_snapshot))
                }
                Err(e) => {
                    self.command_line.push_error(
                        crate::tf!("AEC Style Manager: failed to save library: {e}").as_ref(),
                    );
                    None
                }
            }
        } else {
            let lib = self
                .aec_style_library
                .get_or_insert_with(crate::modules::aec::engine::library::StyleLibrary::empty);
            lib.upsert_wall_style(wall_style);
            let lib_snapshot = lib.clone();

            match self.aec_save_style_library_preferring_project(&lib_snapshot) {
                Ok(()) => {
                    self.command_line
                        .push_info(crate::t!("AEC Style Manager: wall style saved.").as_ref());
                    self.aec_style_manager_selected_wall_style = Some(id.clone());
                    self.aec_style_manager_wall_style_editing_id = Some(id.clone());
                    Some((id, lib_snapshot))
                }
                Err(e) => {
                    self.command_line.push_error(
                        crate::tf!("AEC Style Manager: failed to save library: {e}").as_ref(),
                    );
                    None
                }
            }
        }
    }

    fn sync_open_command_history(&mut self) {
        if !self.command_line.history_open {
            return;
        }
        let latest = self.command_line.history_plain_text();
        if self.history_content.text() == latest {
            return;
        }
        self.history_content = iced::widget::text_editor::Content::with_text(&latest);
        // The outer scrollable is anchored to the newest lines. Leaving the
        // editor cursor at its initial position also keeps horizontal scroll at
        // zero, so the first glyph of each line cannot be clipped.
    }

    fn clear_aec_profile_slot_style_editor_buffers(&mut self) {
        self.aec_style_manager_profile_slot_style_line_type.clear();
        self.aec_style_manager_profile_slot_style_line_color.clear();
        self.aec_style_manager_profile_slot_style_hatch_pattern.clear();
        self.aec_style_manager_profile_slot_style_hatch_color.clear();
        self.aec_style_manager_profile_slot_style_fill_color.clear();
        self.aec_style_manager_profile_slot_style_line_color_picker_open = false;
        self.aec_style_manager_profile_slot_style_hatch_color_picker_open = false;
        self.aec_style_manager_profile_slot_style_fill_color_picker_open = false;
    }

    /// Close the active in-canvas modal (Plan B), mirroring what closing the
    /// old OS window did: a style editor discards its staged (un-applied)
    /// changes, and the ribbon tool that launched the dialog is de-highlighted.
    fn close_active_modal(&mut self) {
        use super::ModalKind::*;
        // Plot Style opened from PLOT behaves as a child modal.
        // Closing it restores the parent Plot dialog instead of returning
        // to the drawing.
        if self.active_modal == Some(Plotstyle) {
            if let Some((plot_offset, plot_resize)) =
                self.plotstyle_parent_plot_geometry.take()
            {
                self.active_modal = Some(Plot);

                self.reset_modal_geometry();
                self.modal_offset = plot_offset;
                self.modal_resize = plot_resize;

                return;
            }
        }
        // Display-profiles opened from the wall style manager behaves as a child modal.
        if self.active_modal == Some(AecWallStyleDisplayProfiles) {
            if let Some((parent_offset, parent_resize)) =
                self.aec_wall_style_manager_parent_geometry.take()
            {
                self.active_modal = Some(AecWallStyleManager);
                self.reset_modal_geometry();
                self.modal_offset = parent_offset;
                self.modal_resize = parent_resize;
                return;
            }
        }
        if self.active_modal == Some(Plot) && self.print_all_options {
            if let Some(previous) = self.print_all_options_prev.take() {
                self.plot_dialog = previous;
            }
            if let Some(previous) = self.print_all_plot_style_prev.take() {
                self.active_plot_style = previous;
            }
            if let Some(previous) = self.print_all_plot_window_prev.take() {
                self.plot_window = previous;
            }
            if let Some(previous) = self.print_all_plot_setup_prev.take() {
                self.plot_setup_template = previous;
            }
            self.print_all_options = false;
            self.active_modal = Some(PrintAll);
            self.reset_modal_geometry();
            return;
        }
        if matches!(
            self.active_modal,
            Some(TextStyle | DimStyle | TableStyle | MLeaderStyle | MlStyle)
        ) {
            self.style_stage_discard();
        }
        if self.active_modal == Some(ScaleManager) {
            self.scale_stage_discard();
        }
        if self.active_modal == Some(DraftingSettings) {
            self.snap_popup_open = false;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.active_modal == Some(FileInUse) {
            self.pending_save_failure = None;
            self.pending_close = None;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.active_modal == Some(ExternalChange) {
            self.pending_external_change = None;
            self.pending_close = None;
        }
        match self.active_modal {
            // Dismissing these via ✕ is the cancel/decline path.
            Some(Unsaved) => self.pending_close = None,
            Some(AssocPrompt) => self.mark_assoc_prompted(),
            // Cancel: leave the layer (and its objects) untouched.
            Some(LayerDeleteWarning) => self.layer_delete_pending = None,
            // Cancel: drop the working copy without touching the block.
            Some(AttributeEditor) => {
                self.attr_editor_handle = None;
                self.attr_editor_block.clear();
                self.attr_editor_rows.clear();
                self.attr_editor_selected = 0;
                self.attr_editor_tab = crate::ui::window::attribute_editor::AttrTab::Attribute;
            }
            Some(GeometricTolerance) => self.geometric_tolerance = None,
            // Closing (✕) discards edits made since the last Apply — matching the
            // style editors. Committing happens only through the Apply button.
            Some(Aliases) => self.alias_editor_rows.clear(),
            Some(Shortcuts) => self.shortcut_editor_rows.clear(),
            Some(LayerStateEditor) => {
                self.layer_state_edit_draft = None;
                self.layer_state_edit_filter.clear();
                self.layer_state_edit_color_open = None;
            }
            Some(Recovery) => self.recovery_report = None,
            // Dismissing without picking/creating a project: forget the tool
            // that was about to run so it isn't replayed unexpectedly later.
            Some(AecProjectRequired) => self.aec_project_required_resume = None,
            _ => {}
        }
        // The tool that opened this dialog is done with it now. Keep the
        // highlight only while an interactive command still runs (it owns
        // it). Replaces the old per-modal deactivate_tool_if list, which
        // missed every newly added dialog (CUI, Plugin Manager, Point
        // Style, Attribute Editor…). (#355)
        if self.tabs[self.active_tab].active_cmd.is_none() {
            self.ribbon.deactivate_tool();
        }
        self.active_modal = None;
        // Recentre / reset the size of the next dialog and drop any drag.
        self.reset_modal_geometry();
    }

    /// Fire the focus-sweep once when a property field is active, so a
    /// click-away that moved focus off the field (viewport, toolbar, command
    /// line) clears the active-row highlight. Idle (`Task::none()`) unless a
    /// field is active, and the sweep itself keeps the marker whenever the
    /// active field still holds focus — re-firing it on unrelated messages is
    /// cheap and safe.
    fn sync_active_field_if_any(&self) -> Task<Message> {
        if self.tabs[self.active_tab].properties.active_field.is_some() {
            crate::ui::properties::sync_active_field_task()
        } else {
            Task::none()
        }
    }

    /// Emit `SelectionChangedV4` to V4 plugins when the active tab's selection
    /// set actually changed since the last broadcast.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn notify_plugins_selection_changed(&mut self) {
        if self.active_tab >= self.tabs.len() {
            return;
        }
        let i = self.active_tab;
        let tab_id = self.tabs[i].id;
        let fingerprint = self.tabs[i].scene.selection_fingerprint();
        let key = (tab_id, fingerprint);
        if self.last_plugin_selection == Some(key) {
            return;
        }
        self.last_plugin_selection = Some(key);
        let handles = self.tabs[i].scene.selected_handles_in_order();
        crate::plugin::v4_support::publish_selection_changed_v4(tab_id, handles);
    }

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        let perf_started = crate::perf::enabled().then(Instant::now);
        let perf_label = perf_message_label(&msg);
        // A modal dialog must capture the keyboard the same way it already
        // captures the mouse. Otherwise keystrokes from the global key
        // subscription leak past the modal into the command line and fire as
        // commands once the dialog closes. While a modal is open, Escape
        // closes it and every other keystroke-derived message is swallowed;
        // the modal's own text fields keep working because they emit their own
        // (non-blocked) messages. (#126)
        if self.active_modal.is_some() {
            if matches!(msg, Message::CommandEscape)
                || matches!(&msg, Message::ShortcutPressed(key) if key.rsplit('+').next() == Some("ESCAPE"))
            {
                return self.update(Message::CloseModal);
            }
            if is_modal_blocked_key_msg(&msg) {
                return Task::none();
            }
        }
        let task = self.update_inner(msg);
        self.sync_open_command_history();
        // Close the document-level first-touch transaction started by
        // push_undo_snapshot at this message boundary.
        self.finish_all_pending_history();
        // After every message, mirror the active command step's prompt so
        // its history line stays pinned (non-fading) until the step changes.
        let prompt = self.tabs[self.active_tab]
            .active_cmd
            .as_ref()
            .map(|c| c.prompt());
        self.command_line.set_step_prompt(prompt);
        // Mirror the step's clickable options so they render as buttons (#304).
        let opts = self.tabs[self.active_tab]
            .active_cmd
            .as_ref()
            .map(|c| c.options())
            .unwrap_or_default();
        self.command_line.set_step_options(opts);
        // Persist UI preferences whenever a toggle changes them (issue #68).
        self.persist_settings_if_changed();
        // The block panel watches the drawing's block list and rebuilds its
        // thumbnails whenever the names change (BLOCK define, file open, …).
        self.refresh_block_palette_if_stale();
        // Let V4 plugins observe selection changes that happened while handling
        // this message (picking, window select, QSELECT, SELECTALL, grip edits,
        // and plugin request draining).
        #[cfg(not(target_arch = "wasm32"))]
        self.notify_plugins_selection_changed();
        // OTRACK acquires tracking points only while a command or grip drag is
        // running; drop them once neither is active so the temporary tracking
        // points / vectors disappear when the command ends (issue #64).
        let i = self.active_tab;
        if self.tabs[i].active_cmd.is_none()
            && self.tabs[i].active_grip.is_none()
            && !self.snapper.tracking_points.is_empty()
        {
            self.snapper.clear_tracking();
            self.otrack_active = None;
            self.otrack_kind = None;
        }
        if let Some(started) = perf_started {
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            if elapsed_ms >= 5.0 {
                crate::perf_record!(
                    "[perf] update {:>7.1}ms message={perf_label}",
                    elapsed_ms,
                );
            }
        }
        #[cfg(target_arch = "wasm32")]
        crate::sys::set_unsaved_changes_warning(self.tabs.iter().any(|tab| tab.dirty));
        if self.tabs[i].active_cmd.is_none() {
            self.block_palette.placing = None;
        }
        task
    }

    /// Drop the OTRACK acquired points and the live alignment vector once a
    /// point has been committed to the active command. Temporary tracking
    /// points are reset on every input so they don't pile up across a
    /// multi-point command and overwhelm the next pick (issue #85).
    pub(in crate::app) fn reset_tracking_after_point(&mut self) {
        self.snapper.clear_tracking();
        self.otrack_active = None;
        self.otrack_kind = None;
    }

    fn update_inner(&mut self, msg: Message) -> Task<Message> {
        match msg {
            // Drain plugin-to-host requests that arrived outside of a host call.
            // This runs on a periodic timer so long-lived plugin sessions such
            // as the Python REPL can mutate the document without requiring a
            // user-generated message. Desktop only.
            #[cfg(not(target_arch = "wasm32"))]
            Message::DrainPluginRequests => {
                for tab in 0..self.tabs.len() {
                    let mut host = crate::app::plugin_host::HostSession::new(self, tab);
                    crate::plugin::external::with_manager(|mgr| {
                        mgr.drain_requests(&mut host, &mut |_| {});
                    });
                }
                crate::plugin::external::with_manager(|mgr| {
                    for line in mgr.drain_io() {
                        if line.text.starts_with("[runner]")
                            || line.text.starts_with("[plugin] ")
                            || line.text.starts_with("[python-repl]")
                        {
                            continue;
                        }
                        let prefixed = format!("[{} {}] {}", line.plugin_id, line.source, line.text);
                        match line.source {
                            ocs_plugin_api::process::IoStream::Stderr => {
                                self.command_line.push_error(&prefixed);
                            }
                            _ => {
                                self.command_line.push_output(&prefixed);
                            }
                        }
                    }
                });
                Task::none()
            }

            // Web: fetch every script queued by startup language selection or
            // drawing text discovery. Each script has one shared store entry.
            Message::PollWebFonts => {
                let pending = crate::scene::text::web_font::take_pending();
                if pending.is_empty() {
                    return Task::none();
                }
                Task::batch(pending.into_iter().map(|script| {
                    Task::perform(crate::scene::text::web_font::fetch(script), move |res| {
                        Message::WebFontLoaded(script, res)
                    })
                }))
            }

            // Web: a per-script font arrived. The same bytes feed drawing text,
            // the UI renderer, and the navigation-cube label atlas.
            Message::WebFontLoaded(script, res) => {
                match res {
                    Ok(bytes) => {
                        crate::scene::text::web_font::insert(script, Some(bytes));
                        crate::scene::text::ttf_glyph::clear_fallback_cache();
                        for tab in self.tabs.iter_mut() {
                            tab.scene.invalidate_text_geometry_dependencies();
                        }
                        return Task::done(Message::ApplyWebFont(script));
                    }
                    Err(e) => {
                        crate::scene::text::web_font::insert(script, None);
                        self.command_line
                            .push_error(crate::tf!("Font load failed ({script:?}): {e}").as_ref());
                    }
                }
                Task::none()
            }

            Message::ApplyWebFont(script) => {
                let Some(bytes) = crate::scene::text::web_font::loaded(script) else {
                    return Task::none();
                };
                iced::font::load((*bytes).clone()).map(move |result| {
                    Message::WebUiFontLoaded(
                        script,
                        result.map_err(|error| format!("{error:?}")),
                    )
                })
            }

            Message::WebUiFontLoaded(script, result) => {
                if let Err(error) = result {
                    self.command_line.push_error(
                        crate::tf!("Font load failed ({script:?}): {error}").as_ref(),
                    );
                    return Task::none();
                }
                if script == crate::scene::text::web_font::primary_script() {
                    let family = iced::font::Family::name(script.family());
                    return iced::font::set_defaults(iced::Font::with_family(family), 16.0);
                }
                Task::none()
            }

            Message::Tick(t) => self.on_tick(t),

            Message::OpenFile => self.on_open_file(),

            Message::OpenPathPicked(None) => Task::none(),

            Message::OpenUrl(url) => crate::sys::open_url(&url, self.main_window),

            Message::StartSectionSelect(section) => {
                self.start_section = section;
                self.save_config();
                Task::none()
            }

            Message::ScrollLayoutTabs(dx) => iced::widget::operation::scroll_by(
                iced::advanced::widget::Id::new(crate::ui::statusbar::LAYOUT_TABS_SCROLL_ID),
                iced::widget::scrollable::AbsoluteOffset { x: dx, y: 0.0 },
            ),

            Message::OpenRecent(path) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // Recents are read from disk every save → the path may be
                    // stale. Skip silently if the file no longer exists; the
                    // entry stays in the list so the user can clean it up.
                    return match std::fs::metadata(&path) {
                        Ok(m) => self.update(Message::OpenPathPicked(Some((path, m.len())))),
                        Err(_) => {
                            self.command_line.push_error(crate::tf!(
                                "Recent file no longer exists: {}",
                                path.display()
                            ).as_ref());
                            Task::none()
                        }
                    };
                }

                #[cfg(target_arch = "wasm32")]
                {
                    if let Some(idx) = self.tab_showing(&path) {
                        return self.update(Message::TabSwitch(idx));
                    }
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.to_string_lossy().into_owned());
                    let state = std::sync::Arc::new(crate::io::OpenProgressState::new(
                        crate::app::OPEN_PHASE_READING,
                    ));
                    let open_id = self.next_open_id();
                    self.opening = Some(crate::app::OpenProgress {
                        id: open_id,
                        name,
                        source_path: Some(path.clone()),
                        size_bytes: 0,
                        state: state.clone(),
                        started: Instant::now(),
                        recovery_error: None,
                        recovery_read_stats: None,
                        recovery_bytes: None,
                    });
                    Task::perform(
                        crate::io::open_recent_web(path, state),
                        move |outcome| Message::WebFileOpened(open_id, outcome),
                    )
                }
            }

            Message::OpenExternal(path) => {
                // A second launch forwarded this drawing. Route it through
                // `OpenRecent` so the redirect and a cold start share one path:
                // it stats the file and reports a missing one visibly, instead
                // of a boot that appears to do nothing.
                //
                // Raising is best-effort and cannot be made reliable from here.
                // `gain_focus` reaches winit's `focus_window`, which on Wayland
                // has an empty body — it is `request_user_attention` that walks
                // the xdg-activation path, and it mints its token without a seat
                // serial, which a compositor may refuse to honour. So expect an
                // attention mark rather than a raise on Wayland; X11 does raise.
                // A real raise needs the activation token from the launching
                // process, and neither iced 0.14 nor winit 0.30 can apply one to
                // an existing window.
                let raise = match self.main_window {
                    Some(id) => Task::batch([
                        iced::window::gain_focus(id),
                        iced::window::request_user_attention(
                            id,
                            Some(iced::window::UserAttention::Critical),
                        ),
                    ]),
                    None => Task::none(),
                };
                if self.opening.is_some()
                    || self.active_modal == Some(super::ModalKind::Recovery)
                {
                    self.pending_opens.push_back(path);
                    raise
                } else if let Some(idx) = self.tab_showing(&path) {
                    Task::batch([raise, self.update(Message::TabSwitch(idx))])
                } else {
                    Task::batch([raise, self.update(Message::OpenRecent(path))])
                }
            }

            Message::WebFieldPaste => {
                // The MText / inline-TEXT editors have their own web paste
                // paths — don't double-feed them.
                if self.mtext_editor.is_some() || self.text_inline.is_some() {
                    return self.on_paste_shortcut();
                }
                #[cfg(target_arch = "wasm32")]
                return Task::perform(
                    crate::sys::read_clipboard_text(),
                    Message::WebFieldPasteText,
                );
                #[cfg(not(target_arch = "wasm32"))]
                Task::none()
            }

            Message::WebFieldPasteText(text) => {
                #[cfg(target_arch = "wasm32")]
                if let Some(t) = &text {
                    crate::sys::synthesize_typing(t);
                }
                let _ = text;
                Task::none()
            }

            Message::WebFieldCopy => {
                // Walk the widget tree for the focused text input's visible
                // text. iced calls `text_input` then `focusable` back-to-back
                // on the same widget, so remembering the last text seen pairs
                // it with the focus check. (An empty field reports its
                // placeholder — that's iced's "visible text" contract.)
                use iced::advanced::widget::operation::{
                    Focusable, Outcome, TextInput,
                };
                use iced::advanced::widget::{Id, Operation};
                #[derive(Default)]
                struct FocusedText {
                    last_text: Option<String>,
                    found: Option<String>,
                }
                impl Operation<Option<String>> for FocusedText {
                    fn text_input(
                        &mut self,
                        _id: Option<&Id>,
                        _bounds: iced::Rectangle,
                        state: &mut dyn TextInput,
                    ) {
                        self.last_text = Some(state.text().to_owned());
                    }
                    fn focusable(
                        &mut self,
                        _id: Option<&Id>,
                        _bounds: iced::Rectangle,
                        state: &mut dyn Focusable,
                    ) {
                        let text = self.last_text.take();
                        if state.is_focused() && self.found.is_none() {
                            self.found = text;
                        }
                    }
                    fn traverse(
                        &mut self,
                        operate: &mut dyn FnMut(&mut dyn Operation<Option<String>>),
                    ) {
                        operate(self);
                    }
                    fn finish(&self) -> Outcome<Option<String>> {
                        Outcome::Some(self.found.clone())
                    }
                }
                iced::advanced::widget::operate(FocusedText::default())
                    .map(Message::WebFieldCopyText)
            }

            Message::WebFieldCopyText(text) => {
                #[cfg(target_arch = "wasm32")]
                if let Some(t) = &text {
                    if !t.is_empty() {
                        crate::sys::write_clipboard_text(t);
                    }
                }
                let _ = text;
                Task::none()
            }

            Message::SnapOverridePick(t) => {
                self.snap_override_popup = None;
                self.snapper.set_override(t);
                let label = crate::snap::ALL_SNAP_MODES
                    .iter()
                    .find(|(m, _, _)| *m == t)
                    .map(|(_, _, l)| *l)
                    .unwrap_or("Snap");
                self.command_line
                    .push_info(crate::tf!("Snap override: {label} (next pick only).").as_ref());
                Task::none()
            }

            Message::SnapOverrideClose => {
                self.snap_override_popup = None;
                Task::none()
            }

            Message::FileDropped(path) => {
                // Desktop drag & drop (#344): accept the formats the Open
                // dialog accepts — a drop has no picker filter, so anything
                // else reports instead of failing silently in the parser.
                let ext = path
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if !matches!(ext.as_str(), "dwg" | "dxf" | "bak" | "sv$") {
                    self.command_line.push_error(crate::tf!(
                        "Unsupported file type: {}",
                        path.display()
                    ).as_ref());
                    return Task::none();
                }
                // A load or recovery report owns the open slot; queue another
                // drop until that state is acknowledged.
                if self.opening.is_some()
                    || self.active_modal == Some(super::ModalKind::Recovery)
                {
                    self.pending_opens.push_back(path);
                    Task::none()
                } else if let Some(idx) = self.tab_showing(&path) {
                    self.update(Message::TabSwitch(idx))
                } else {
                    self.update(Message::OpenRecent(path))
                }
            }

            Message::RecentRemove(path) => {
                self.remove_recent(&path);
                Task::none()
            }

            Message::SetRecentLimit(limit) => {
                self.set_recent_limit(limit);
                // Resync the input box to the clamped, applied value.
                self.recent_limit_input = self.recent_limit.to_string();
                Task::none()
            }

            Message::RecentLimitInput(s) => {
                // Keep only digits while typing; applied on Enter (SetRecentLimit).
                self.recent_limit_input = s.chars().filter(|c| c.is_ascii_digit()).collect();
                Task::none()
            }

            Message::OpenPathPicked(Some((path, size_bytes))) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unknown".into());
                let progress = std::sync::Arc::new(crate::io::OpenProgressState::new(
                    super::OPEN_PHASE_READING,
                ));
                let open_id = self.next_open_id();
                self.opening = Some(super::OpenProgress {
                    id: open_id,
                    name: name.clone(),
                    source_path: Some(path.clone()),
                    size_bytes,
                    state: progress.clone(),
                    started: Instant::now(),
                    recovery_error: None,
                    recovery_read_stats: None,
                    #[cfg(target_arch = "wasm32")]
                    recovery_bytes: None,
                    #[cfg(not(target_arch = "wasm32"))]
                    fingerprint:
                        crate::io::edit_lock::FileFingerprint::capture(&path).ok(),
                });
                let size_label = format_size(size_bytes);
                self.command_line
                    .push_info(crate::tf!("Opening \"{name}\" ({size_label})…").as_ref());
                let model_bg = self.default_bg_color.unwrap_or([
                    33.0 / 255.0,
                    40.0 / 255.0,
                    48.0 / 255.0,
                    1.0,
                ]);
                Task::perform(
                    crate::io::open_path_with_phase(path, progress, model_bg),
                    move |result| Message::FileOpened(open_id, result),
                )
            }

            Message::OpenCancel => {
                if let Some(p) = self.opening.take() {
                    self.command_line
                        .push_info(crate::tf!("Open cancelled: \"{}\"", p.name).as_ref());
                }
                self.drain_pending_open()
            }

            #[cfg(target_arch = "wasm32")]
            Message::WebFileOpened(open_id, mut outcome) => {
                if self.opening.as_ref().map(|opening| opening.id) != Some(open_id) {
                    return Task::none();
                }
                if let Some(opening) = self.opening.as_mut() {
                    opening.name = outcome.name.clone();
                    opening.source_path = Some(std::path::PathBuf::from(&outcome.name));
                    if outcome.size_bytes > 0 || opening.size_bytes == 0 {
                        opening.size_bytes = outcome.size_bytes;
                    }
                    opening.recovery_bytes = outcome.recovery_bytes.take();
                }
                if let Some(bytes) = outcome.cache_bytes.take() {
                    let name = outcome.name.clone();
                    return Task::perform(
                        async move {
                            let result =
                                crate::io::web_recent::store_open(&name, bytes, open_id).await;
                            (outcome, result)
                        },
                        move |(outcome, result)| {
                            Message::WebFileCached(open_id, outcome, result)
                        },
                    );
                }
                let recent_task = if outcome.record_recent && outcome.result.is_ok() {
                    self.push_recent(std::path::PathBuf::from(&outcome.name))
                } else {
                    Task::none()
                };
                let opened_task = self.update(Message::FileOpened(open_id, outcome.result));
                Task::batch([recent_task, opened_task])
            }

            #[cfg(target_arch = "wasm32")]
            Message::WebFileCached(open_id, outcome, cache_result) => {
                if self.opening.as_ref().map(|opening| opening.id) != Some(open_id) {
                    return Task::none();
                }
                let recent_task = match cache_result {
                    Ok(()) => self.push_recent(std::path::PathBuf::from(&outcome.name)),
                    Err(error) => {
                        self.command_line.push_error(crate::tf!(
                            "Opened drawing, but recent copy could not be stored: {error}"
                        ).as_ref());
                        Task::none()
                    }
                };
                let opened_task = self.update(Message::FileOpened(open_id, outcome.result));
                Task::batch([recent_task, opened_task])
            }

            Message::FileOpened(open_id, Ok((name, path, doc, caches))) => {
                if self.opening.as_ref().map(|opening| opening.id) != Some(open_id) {
                    return Task::none();
                }
                self.on_file_opened(name, path, doc, caches)
            }

            Message::FileOpened(open_id, Err(e)) => {
                if self.opening.as_ref().map(|opening| opening.id) != Some(open_id) {
                    return Task::none();
                }
                if e.recovery_available {
                    if let Some(opening) = self.opening.as_mut() {
                        opening.recovery_error = Some(e.message);
                        opening.recovery_read_stats = e.read_stats;
                        self.active_modal = Some(super::ModalKind::RecoveryPrompt);
                        return Task::none();
                    }
                }
                // If the user cancelled, the overlay was already cleared and
                // we suppress the noise.
                let opening = self.opening.take();
                if let Some(opening) = opening.filter(|_| e.message != "Cancelled") {
                    self.command_line.push_error(crate::tf!("Open failed: {e}").as_ref());
                    let total_ms = opening.started.elapsed().as_millis() as u32;
                    let failure_phase = crate::io::open_phase_name(
                        opening
                            .state
                            .phase
                            .load(std::sync::atomic::Ordering::Acquire),
                    )
                    .to_string();
                    let mut report = crate::io::recovery::RecoveryReport::failed(
                        opening.source_path,
                        opening.name,
                        opening.size_bytes,
                        e.source_sha256,
                        e.read_stats,
                        failure_phase,
                        e.message,
                        total_ms,
                    );
                    report.persist();
                    self.recovery_report = Some(report);
                    self.active_modal = Some(super::ModalKind::Recovery);
                    return Task::none();
                }
                // A drawing that fails to parse must not strand the ones queued
                // behind it.
                self.drain_pending_open()
            }

            #[cfg(target_arch = "wasm32")]
            Message::WebRecentStored(result) => match result {
                Ok(path) => self.push_recent(path),
                Err(error) => {
                    self.command_line.push_error(crate::tf!(
                        "Saved download, but recent copy could not be stored: {error}"
                    ).as_ref());
                    Task::none()
                }
            },

            Message::ImagePick => {
                Task::perform(crate::io::pick_image_file(), Message::ImagePickResult)
            }

            Message::ImagePickResult(Ok((path, pw, ph))) => {
                use crate::command::CadCommand;
                use crate::modules::draw::draw::raster_image::ImageCommand;
                let path_str = path.to_string_lossy().into_owned();
                let short = std::path::Path::new(&path_str)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&path_str)
                    .to_string();
                self.command_line
                    .push_output(crate::tf!("IMAGE  \"{short}\": {pw}×{ph} px").as_ref());
                let cmd = ImageCommand::new(path_str, pw, ph);
                let i = self.active_tab;
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
                Task::none()
            }

            Message::ImagePickResult(Err(e)) => {
                if e != "Cancelled" {
                    self.command_line.push_error(crate::tf!("IMAGE: {e}").as_ref());
                }
                Task::none()
            }

            Message::XAttachPick => Task::perform(
                async {
                    let handle = crate::sys::file_dialog()
                        .set_title("Select External Reference File")
                        .add_filter("CAD Files", &["dwg", "dxf", "bak", "DWG", "DXF", "BAK"])
                        .add_filter("DWG Files", &["dwg", "DWG"])
                        .add_filter("DXF Files", &["dxf", "DXF"])
                        .add_filter("Backup Files", &["bak", "BAK"])
                        .pick_file()
                        .await;
                    match handle {
                        Some(h) => Ok(crate::sys::handle_path(&h)),
                        None => Err("Cancelled".to_string()),
                    }
                },
                Message::XAttachPickResult,
            ),

            Message::XAttachPickResult(Ok(path)) => {
                use crate::command::CadCommand;
                use crate::modules::insert::xattach::XAttachCommand;
                let path_str = path.to_string_lossy().into_owned();
                let cmd = XAttachCommand::with_path(path_str);
                let i = self.active_tab;
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
                Task::none()
            }

            Message::XAttachPickResult(Err(e)) => {
                if e != "Cancelled" {
                    self.command_line.push_error(crate::tf!("XATTACH: {e}").as_ref());
                }
                Task::none()
            }

            Message::WblockSave(block_name) => {
                let name = block_name.clone();
                Task::perform(
                    async move {
                        let path = crate::sys::file_dialog()
                            .set_title("Save Block As")
                            .set_file_name("block.dwg")
                            .add_filter("DWG Files", &["dwg"])
                            .save_file()
                            .await
                            .map(|h| crate::sys::handle_path(&h));
                        (name, path)
                    },
                    |(name, path)| Message::WblockSaveResult(name, path),
                )
            }

            Message::WblockSaveResult(block_name, Some(path)) => {
                self.on_wblock_save_result_some(block_name, path)
            }

            Message::WblockSaveResult(_, None) => Task::none(),

            Message::WblockWriteFinished(block_name, path, result) => {
                match result {
                    Ok(()) => self.command_line.push_output(crate::tf!(
                        "WBLOCK  Saved \"{block_name}\" → \"{}\"",
                        path.display()
                    ).as_ref()),
                    Err(error) => self
                        .command_line
                        .push_error(crate::tf!("WBLOCK save failed: {error}").as_ref()),
                }
                Task::none()
            }

            Message::DataExtractionSave(csv) => {
                let csv_clone = csv.clone();
                Task::perform(
                    async move {
                        let path = crate::sys::file_dialog()
                            .set_title("Save Data Extraction")
                            .set_file_name("extraction.csv")
                            .add_filter("CSV", &["csv"])
                            .add_filter("All Files", &["*"])
                            .save_file()
                            .await
                            .map(|h| crate::sys::handle_path(&h));
                        (csv_clone, path)
                    },
                    |(csv, path)| Message::DataExtractionSaveResult(csv, path),
                )
            }

            Message::DataExtractionSaveResult(csv, Some(path)) => {
                match std::fs::write(&path, csv.as_bytes()) {
                    Ok(()) => {
                        let rows = csv.lines().count().saturating_sub(1);
                        let fname = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.to_string_lossy().into_owned());
                        self.command_line
                            .push_output(crate::tf!("DATAEXTRACTION  {rows} rows → \"{fname}\"").as_ref());
                    }
                    Err(e) => self
                        .command_line
                        .push_error(crate::tf!("DATAEXTRACTION: write failed: {e}").as_ref()),
                }
                Task::none()
            }

            Message::DataExtractionSaveResult(_, None) => Task::none(),

            Message::StlExport => {
                let i = self.active_tab;
                if self.tabs[i].scene.meshes.is_empty() {
                    self.command_line
                        .push_error(crate::t!("STLOUT: no 3D mesh data in this drawing.").as_ref());
                    return Task::none();
                }
                Task::perform(
                    async {
                        crate::sys::file_dialog()
                            .set_title("Export STL")
                            .set_file_name("export.stl")
                            .add_filter("STL Files", &["stl"])
                            .add_filter("All Files", &["*"])
                            .save_file()
                            .await
                            .map(|h| crate::sys::handle_path(&h))
                    },
                    Message::StlExportPath,
                )
            }

            Message::StlExportPath(Some(path)) => self.on_stl_export_path_some(path),

            Message::StlExportPath(None) => Task::none(),

            Message::StlExportFinished(path, result) => {
                match result {
                    Ok(()) => self
                        .command_line
                        .push_output(crate::tf!("STLOUT: exported to \"{}\"", path.display()).as_ref()),
                    Err(error) => self.command_line.push_error(crate::tf!("STLOUT: {error}").as_ref()),
                }
                Task::none()
            }

            // ── STEP AP203 export ─────────────────────────────────────────
            Message::StepExport => {
                let i = self.active_tab;
                if self.tabs[i].scene.meshes.is_empty() {
                    self.command_line
                        .push_error(crate::t!("STEPOUT: no 3D mesh data in this drawing.").as_ref());
                    return Task::none();
                }
                Task::perform(
                    async {
                        crate::sys::file_dialog()
                            .set_title("Export STEP AP203")
                            .set_file_name("export.step")
                            .add_filter("STEP Files", &["step", "stp"])
                            .add_filter("All Files", &["*"])
                            .save_file()
                            .await
                            .map(|h| crate::sys::handle_path(&h))
                    },
                    Message::StepExportPath,
                )
            }

            Message::StepExportPath(Some(path)) => self.on_step_export_path_some(path),

            Message::StepExportPath(None) => Task::none(),

            Message::StepExportFinished(path, result) => {
                match result {
                    Ok(()) => self
                        .command_line
                        .push_output(crate::tf!("STEPOUT: exported to \"{}\"", path.display()).as_ref()),
                    Err(error) => self.command_line.push_error(crate::tf!("STEPOUT: {error}").as_ref()),
                }
                Task::none()
            }

            // ── OBJ import ────────────────────────────────────────────────
            Message::ObjImport => Task::perform(
                async {
                    crate::sys::file_dialog()
                        .set_title("Import OBJ Mesh")
                        .add_filter("Wavefront OBJ", &["obj", "OBJ"])
                        .add_filter("All Files", &["*"])
                        .pick_file()
                        .await
                        .map(|h| crate::sys::handle_path(&h))
                },
                Message::ObjImportPath,
            ),

            Message::ObjImportPath(Some(path)) => self.on_obj_import_path_some(path),

            Message::ObjImportPath(None) => Task::none(),

            Message::ObjImportFinished(tab_id, path, result) => {
                match result {
                    Err(error) => self.command_line.push_error(crate::tf!("IMPORTOBJ: {error}").as_ref()),
                    Ok(mut mesh) => {
                        let Some(i) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
                            self.command_line
                                .push_info(crate::t!("IMPORTOBJ: target drawing was closed.").as_ref());
                            return Task::none();
                        };
                        let file_stem = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "obj_mesh".into());
                        mesh.name = file_stem.clone();
                        self.push_undo_snapshot(i, "IMPORTOBJ");
                        let entity = crate::modules::insert::solid3d_cmds::empty_solid3d();
                        let handle = self.tabs[i].scene.add_entity(entity);
                        if !handle.is_null() {
                            self.tabs[i]
                                .scene
                                .meshes
                                .insert(handle, crate::scene::MeshLodSet::from_single(mesh));
                            self.tabs[i].dirty = true;
                            self.command_line.push_output(crate::tf!(
                                "IMPORTOBJ: imported \"{file_stem}\" as mesh."
                            ).as_ref());
                        }
                    }
                }
                Task::none()
            }

            Message::SaveFile => self.on_save_file(),

            Message::SaveAs => {
                if self.read_only {
                    self.command_line
                        .push_error(crate::t!("Read-only session (--read-only): saving is disabled.").as_ref());
                    return Task::none();
                }
                let i = self.active_tab;
                self.save_dialog_for_unsaved = false;
                self.open_save_dialog_window(i)
            }

            Message::SaveDialogFormatChanged(fmt) => {
                let (ext, _) = crate::io::parse_save_format(&fmt);
                let stem = std::path::Path::new(&self.save_dialog_filename)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "drawing".to_string());
                self.save_dialog_filename = format!("{stem}.{ext}");
                self.save_dialog_format = fmt;
                Task::none()
            }

            Message::SaveDialogFilenameChanged(name) => {
                self.save_dialog_filename = name;
                Task::none()
            }

            Message::SaveDialogConfirm => self.on_save_dialog_confirm(),

            Message::SaveDialogCancel => self.close_save_dialog_window(),

            #[cfg(not(target_arch = "wasm32"))]
            Message::SaveDialogPathPicked(picked) => self.on_save_dialog_path_picked(picked),

            #[cfg(target_arch = "wasm32")]
            Message::SaveDialogPathPicked(_) => Task::none(),

            Message::ClearScene => {
                let i = self.active_tab;
                self.push_undo_snapshot(i, "CLEAR");
                self.tabs[i].scene.clear();
                crate::io::linetypes::populate_document(&mut self.tabs[i].scene.document);
                self.tabs[i].properties = PropertiesPanel::empty();
                let doc_layers = self.tabs[i].scene.document.layers.clone();
                let vp_info = self.tabs[i].scene.viewport_list();
                self.tabs[i]
                    .layers
                    .sync_with_viewports(&doc_layers, vp_info);
                self.command_line
                    .push_output(crate::t!("Scene cleared. Standard linetypes loaded.").as_ref());
                self.tabs[i].current_path = None;
                self.tabs[i].dirty = true;
                self.sync_ribbon_layers();
                Task::none()
            }

            Message::SetRenderMode(mode) => {
                self.render_mode_menu_open = false;
                self.render_mode_preview = None;
                self.on_set_render_mode(mode)
            }

            Message::ToggleRenderModeMenu(mode) => {
                self.render_mode_menu_open = !self.render_mode_menu_open;
                self.render_mode_preview = self.render_mode_menu_open.then_some(mode);
                Task::none()
            }

            Message::DismissRenderModeMenu => {
                self.render_mode_menu_open = false;
                self.render_mode_preview = None;
                Task::none()
            }

            Message::PreviewRenderMode(mode) => {
                if self.render_mode_menu_open {
                    self.render_mode_preview = Some(mode);
                }
                Task::none()
            }

            Message::SetProjection(ortho) => {
                use crate::scene::Projection;
                let proj = if ortho {
                    Projection::Orthographic
                } else {
                    Projection::Perspective
                };
                let i = self.active_tab;
                self.tabs[i]
                    .scene
                    .set_projection_preserving_frame(proj);
                self.ribbon.set_ortho(ortho);
                self.command_line.push_output(if ortho {
                    "Projection: Orthographic"
                } else {
                    "Projection: Perspective"
                });
                Task::none()
            }

            Message::RibbonSelectTab(idx) => {
                self.ribbon.select(idx);
                Task::none()
            }

            Message::SetRibbonCollapseMode(mode) => {
                self.ribbon.set_collapse_mode(mode);
                self.ribbon.close_dropdown();
                self.save_config();
                Task::none()
            }

            Message::RibbonToolClick { tool_id, event } => {
                let sweep = self.sync_active_field_if_any();
                Task::batch(vec![sweep, self.on_ribbon_tool_click(tool_id, event)])
            }
            Message::PluginFileDialogResult { command, path } => {
                if let Some(path) = path {
                    // Dispatch "<command> <path>" with original case intact —
                    // the command line would upper-case the whole string and
                    // mangle case-sensitive paths on Linux/macOS.
                    let line = format!("{} {}", command, path.to_string_lossy());
                    let i = self.active_tab;
                    if !crate::plugin::try_dispatch(self, i, &line) {
                        self.command_line
                            .push_error(crate::tf!("No plugin handled: {command}").as_ref());
                    }
                }
                Task::none()
            }

            // ── Document tabs ─────────────────────────────────────────────
            Message::TabNew => {
                // Preserve the outgoing drawing's Ortho / running OSNAP.
                self.stamp_header_sysvars(self.active_tab);
                self.tab_counter += 1;
                let new_tab = super::document::DocumentTab::new_drawing(self.tab_counter);
                self.tabs.push(new_tab);
                self.active_tab = self.tabs.len() - 1;
                let idx = self.active_tab;
                // A fresh drawing inherits the app's current Ortho / OSNAP so
                // creating one doesn't silently reset them (its header default is
                // 0 = no snaps); saving then persists them into the file.
                self.stamp_header_sysvars(idx);
                self.apply_bg_default(idx);
                self.sync_ribbon_layers();
                self.sync_ribbon_styles();
                // #21: reset ribbon Color / Linetype / Lineweight to the
                // fresh tab's defaults (ByLayer) instead of inheriting the
                // previous tab's last selection.
                self.sync_ribbon_from_selection();
                // A fresh drawing starts with grid/snap off (its tile defaults).
                self.adopt_view_display(self.active_tab);
                Task::none()
            }

            Message::TabSwitch(idx) => {
                if self.active_modal == Some(super::ModalKind::Recovery) {
                    return Task::none();
                }
                self.layout_list_open = false;
                self.layout_rename_state = None;
                if idx < self.tabs.len() {
                    if idx != self.active_tab {
                        // The attribute editor is tab-scoped; leaving its tab
                        // drops it (its handle is that document's, not this one's).
                        self.cancel_attr_editor();
                        // Persist the outgoing drawing's Ortho / running OSNAP
                        // before leaving it, so switching back restores them.
                        let prev = self.active_tab;
                        self.stamp_header_sysvars(prev);
                    }
                    self.active_tab = idx;
                    if self.tabs[idx].is_start {
                        self.ribbon.close_dropdown();
                    }
                    if self.tabs[idx].is_start
                        && matches!(
                            self.active_modal,
                            Some(
                                super::ModalKind::LayoutManager
                                    | super::ModalKind::LayerStateManager
                                    | super::ModalKind::LayerStateEditor
                            )
                        )
                    {
                        self.close_active_modal();
                    } else if self.active_modal == Some(super::ModalKind::LayerStateEditor) {
                        self.close_active_modal();
                    } else if self.active_modal == Some(super::ModalKind::LayerStateManager) {
                        let mut names: Vec<String> = self.tabs[idx]
                            .scene
                            .document
                            .layer_states()
                            .into_iter()
                            .map(|state| state.name)
                            .collect();
                        names.sort_by_key(|name| name.to_lowercase());
                        self.load_layer_state_editor(names.into_iter().next());
                    }
                    self.sync_ribbon_layers();
                    self.sync_ribbon_styles();
                    // #21: also re-seed ribbon Color / Linetype / Lineweight
                    // from the newly active tab so they reflect that doc's
                    // CECOLOR / CELTYPE / CELWEIGHT (or its current selection
                    // if there is one), not the prior tab's choice.
                    self.sync_ribbon_from_selection();
                    // Grid/snap follow the newly active drawing's viewport.
                    self.adopt_view_display(idx);
                    // Ortho / running OSNAP follow the newly active drawing.
                    self.adopt_header_sysvars(idx);
                    // Shared CJK ideographs follow the newly active drawing's
                    // language; re-tessellate if it differs from the last. (#141)
                    if crate::scene::text::web_font::set_cjk_lang_from_codepage(
                        &self.tabs[idx].scene.document.header.code_page,
                    ) {
                        crate::scene::text::ttf_glyph::clear_fallback_cache();
                        self.tabs[idx]
                            .scene
                            .invalidate_text_geometry_dependencies();
                    }
                }
                Task::none()
            }

            Message::DocTabHover(index) => {
                self.hovered_doc_tab = index.filter(|&idx| idx < self.tabs.len());
                Task::none()
            }

            Message::TabReorder { from, to, after } => {
                let Some(insertion) =
                    reorder_insertion_index(from, to, after, self.tabs.len())
                else {
                    return Task::none();
                };
                if self.tabs.get(from).is_some_and(|tab| tab.is_start)
                    || self.tabs.get(to).is_some_and(|tab| tab.is_start)
                {
                    return Task::none();
                }

                let active_id = self.tabs[self.active_tab].id;
                let moved = self.tabs.remove(from);
                self.tabs.insert(insertion, moved);
                if let Some(index) = self.tabs.iter().position(|tab| tab.id == active_id) {
                    self.active_tab = index;
                }
                self.hovered_doc_tab = None;
                Task::none()
            }

            Message::TabClose(idx) => {
                self.hovered_doc_tab = None;
                self.on_tab_close(idx)
            }

            Message::DocTabSaveAll => self.dispatch_command("SAVEALL"),

            Message::DocTabCloseAll => {
                let ids = self
                    .tabs
                    .iter()
                    .filter(|tab| !tab.is_start)
                    .map(|tab| tab.id)
                    .collect();
                self.begin_tab_close_queue(ids)
            }

            Message::DocTabCloseOthers(idx) => {
                let Some(keep_id) = self.tabs.get(idx).filter(|tab| !tab.is_start).map(|t| t.id)
                else {
                    return Task::none();
                };
                let switch = self.update(Message::TabSwitch(idx));
                let ids = self
                    .tabs
                    .iter()
                    .filter(|tab| !tab.is_start && tab.id != keep_id)
                    .map(|tab| tab.id)
                    .collect();
                Task::batch([switch, self.begin_tab_close_queue(ids)])
            }

            Message::DocTabCopyFullPath(idx) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let Some(path) = self.tabs.get(idx).and_then(|tab| tab.current_path.clone())
                    else {
                        self.command_line
                            .push_error(crate::t!("Save the drawing before copying its file path.").as_ref());
                        return Task::none();
                    };
                    let full_path = path.canonicalize().unwrap_or_else(|_| {
                        if path.is_absolute() {
                            path
                        } else {
                            std::env::current_dir()
                                .map(|dir| dir.join(&path))
                                .unwrap_or(path)
                        }
                    });
                    self.command_line
                        .push_output(crate::tf!("Copied path: {}", full_path.display()).as_ref());
                    return iced::clipboard::write(full_path.to_string_lossy().into_owned())
                        .discard();
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = idx;
                    self.command_line
                        .push_error(crate::t!("Full file paths are unavailable in the web application.").as_ref());
                    Task::none()
                }
            }

            Message::DocTabOpenFileLocation(idx) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let Some(path) = self.tabs.get(idx).and_then(|tab| tab.current_path.clone())
                    else {
                        self.command_line
                            .push_error(crate::t!("Save the drawing before opening its file location.").as_ref());
                        return Task::none();
                    };
                    match crate::sys::reveal_in_file_manager(&path) {
                        Ok(()) => self
                            .command_line
                            .push_output(crate::tf!("Opened file location: {}", path.display()).as_ref()),
                        Err(error) => self
                            .command_line
                            .push_error(crate::tf!("Could not open file location: {error}").as_ref()),
                    }
                    Task::none()
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = idx;
                    self.command_line
                        .push_error(crate::t!("File locations are unavailable in the web application.").as_ref());
                    Task::none()
                }
            }

            Message::CommandInput(s) => {
                // Space submits (acts like Enter) so a command advances
                // token-by-token, matching CAD convention. A leading `>` switches
                // to literal-space mode so an argument containing spaces (a text
                // string, a path, `UCS Z 90` as one line) can be typed; the `>`
                // is stripped on submit. (Unfocused Space repeats the last
                // command via CommandSpace.)
                // Command-line entry is shown uppercase.
                let s = s.to_uppercase();
                // A space submits (acts like Enter). The whole value is handed to
                // the submit path, which tokenises multi-token lines — so a typed
                // token, a pasted `LINE 0,0 10,10`, or API-fed text all run their
                // spaces as step separators. A leading `>` keeps spaces literal.
                let sweep = self.sync_active_field_if_any();
                if !self.command_line.literal_spaces && !s.starts_with('>') && s.contains(' ') {
                    self.command_line.input = s;
                    return Task::batch(vec![sweep, self.update(Message::CommandSubmit)]);
                }
                let live_input = s.clone();
                self.command_line.input = live_input.clone();
                self.command_line.autocomplete_cursor = None;
                self.command_line.cancel_history_navigation();
                // Live incremental search for INSERT/MINSERT: update picker on each keystroke
                // without requiring Enter. Performance-first: uses upper/lower caches
                // and partial sort (O(k)) so per-keystroke is <0.2ms even for 10k blocks.
                // Suggestion count is in direct relation to CLIPROMPTLINES (picker limit).
                {
                    let i = self.active_tab;
                    // Capture prompt/options while holding cmd borrow, then release before
                    // borrowing command_line to satisfy borrow checker.
                    let (should_update, opts, prompt) = if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                        if cmd.on_live_input(&live_input) {
                            (true, cmd.options(), cmd.prompt())
                        } else {
                            (false, Vec::new(), String::new())
                        }
                    } else {
                        (false, Vec::new(), String::new())
                    };
                    if should_update {
                        self.command_line.set_step_options(opts);
                        // Update pinned prompt text live without pushing new history entry
                        // (avoids flooding history with one entry per keystroke).
                        if let Some(last) = self.command_line.history.last_mut() {
                            if last.pinned {
                                last.text = prompt;
                            }
                        }
                    }
                }
                Task::batch(vec![sweep, Task::none()])
            }

            Message::CommandAppendChar(s) => self.on_command_append_char(s),

            Message::CommandBackspace => self.on_command_backspace(),

            Message::DynTabNext if self.grip_popup.is_some() => {
                if let Some(popup) = self.grip_popup.as_mut() {
                    if !popup.items.is_empty() {
                        popup.selected = (popup.selected + 1) % popup.items.len();
                    }
                }
                Task::none()
            }

            Message::DynTabNext => {
                let i = self.active_tab;
                let n = self.tabs[i].dyn_fields.len();
                if n > 0 {
                    self.tabs[i].dyn_active = (self.tabs[i].dyn_active + 1) % n;
                    // TAB locks the value just typed — reshape the rubber-band
                    // to the constrained point now (#356).
                    self.refresh_active_cmd_preview(i);
                }
                self.focus_cmd_input()
            }

            Message::SplitModelViewport(horizontal) => {
                let i = self.active_tab;
                self.tabs[i].scene.split_active_pane(horizontal);
                self.tabs[i].scene.camera_generation += 1;
                Task::none()
            }

            Message::CloseModelViewport => {
                let i = self.active_tab;
                self.tabs[i].scene.close_active_pane();
                self.tabs[i].scene.camera_generation += 1;
                self.sync_render_mode_to_active_tile(i);
                self.adopt_view_display(i);
                Task::none()
            }

            Message::CommandHistoryPrev => {
                if self.tabs[self.active_tab]
                    .properties
                    .hatch_pattern_picker_open
                {
                    return self.update(Message::PropHatchPatternNavigate(-2));
                }
                // Grip popup wins first — arrow keys walk its items.
                if let Some(popup) = self.grip_popup.as_mut() {
                    if !popup.items.is_empty() {
                        popup.selected = if popup.selected == 0 {
                            popup.items.len() - 1
                        } else {
                            popup.selected - 1
                        };
                    }
                    return Task::none();
                }
                let i = self.active_tab;
                if !self.command_line.history_navigation_active()
                    && self.tabs[i].active_cmd.is_none()
                    && self.command_line.autocomplete_prev()
                {
                    return Task::none();
                }
                self.command_line.history_prev();
                iced::widget::operation::move_cursor_to_end(iced::widget::Id::new(
                    crate::ui::command_line::CMD_INPUT_ID,
                ))
            }

            Message::CommandHistoryNext => {
                if self.tabs[self.active_tab]
                    .properties
                    .hatch_pattern_picker_open
                {
                    return self.update(Message::PropHatchPatternNavigate(2));
                }
                if let Some(popup) = self.grip_popup.as_mut() {
                    if !popup.items.is_empty() {
                        popup.selected = (popup.selected + 1) % popup.items.len();
                    }
                    return Task::none();
                }
                let i = self.active_tab;
                if !self.command_line.history_navigation_active()
                    && self.tabs[i].active_cmd.is_none()
                    && self.command_line.autocomplete_next()
                {
                    return Task::none();
                }
                self.command_line.history_next();
                iced::widget::operation::move_cursor_to_end(iced::widget::Id::new(
                    crate::ui::command_line::CMD_INPUT_ID,
                ))
            }

            Message::CommandLineArrowProbe {
                direction,
                extend_selection,
            } => {
                iced::widget::operation::is_focused(iced::widget::Id::new(
                    crate::ui::command_line::CMD_INPUT_ID,
                ))
                .map(move |focused| Message::CommandLineArrowResolved {
                    direction,
                    focused,
                    extend_selection,
                })
            }

            Message::CommandLineArrowResolved {
                direction,
                focused,
                extend_selection,
            } => {
                if !focused {
                    return Task::none();
                }
                // The editor inherits arrows captured by the focused command line.
                if self.mtext_editor.is_some() {
                    match direction {
                        ArrowKey::Up => self.mtext_caret_move_vertical(1, extend_selection),
                        ArrowKey::Down => self.mtext_caret_move_vertical(-1, extend_selection),
                        ArrowKey::Left | ArrowKey::Right => {}
                    }
                    return Task::none();
                }
                match direction {
                    ArrowKey::Up => self.update(Message::CommandHistoryPrev),
                    ArrowKey::Down => self.update(Message::CommandHistoryNext),
                    ArrowKey::Left | ArrowKey::Right => Task::none(),
                }
            }

            Message::ArrowKeyPressed {
                direction,
                shortcut,
                extend_selection,
            } => {
                if self.mtext_editor.is_some() {
                    match direction {
                        ArrowKey::Left => self.mtext_caret_move(-1, extend_selection),
                        ArrowKey::Right => self.mtext_caret_move(1, extend_selection),
                        ArrowKey::Up => {
                            self.mtext_caret_move_vertical(1, extend_selection)
                        }
                        ArrowKey::Down => {
                            self.mtext_caret_move_vertical(-1, extend_selection)
                        }
                    }
                    Task::none()
                } else {
                    match direction {
                        ArrowKey::Up => self.update(Message::CommandHistoryPrev),
                        ArrowKey::Down => self.update(Message::CommandHistoryNext),
                        ArrowKey::Left | ArrowKey::Right => self.run_shortcut(&shortcut),
                    }
                }
            }

            Message::CommandLiteralToggle => {
                self.command_line.literal_spaces = !self.command_line.literal_spaces;
                self.save_config();
                self.focus_cmd_input()
            }

            Message::CommandHistoryToggle => {
                self.command_line.toggle_history();
                if !self.command_line.history_open {
                    self.command_history_resizing = false;
                    self.command_history_drag_last = None;
                }
                self.sync_open_command_history();
                Task::none()
            }

            Message::CommandHistoryResizeGrab => {
                if self.command_line.history_open {
                    self.command_history_resizing = true;
                    self.command_history_drag_last = None;
                }
                Task::none()
            }

            Message::CommandHistoryResizeMove(point) => {
                if self.command_history_resizing {
                    if let Some(last) = self.command_history_drag_last {
                        let dy = point.y - last.y;
                        let max_height =
                            crate::ui::command_line::history_max_height(self.win_size.1);
                        self.command_line.history_height = (self.command_line.history_height - dy)
                            .clamp(
                                crate::ui::command_line::HISTORY_HEIGHT_MIN,
                                max_height,
                            );
                    }
                    self.command_history_drag_last = Some(point);
                }
                Task::none()
            }

            Message::CommandHistoryResizeRelease => {
                let changed = self.command_history_resizing;
                self.command_history_resizing = false;
                self.command_history_drag_last = None;
                if changed {
                    self.save_config();
                }
                Task::none()
            }

            Message::CommandHistoryHeightReset => {
                self.command_line.history_height =
                    crate::ui::command_line::HISTORY_HEIGHT_DEFAULT;
                self.save_config();
                Task::none()
            }

            Message::CommandHistoryCopy => {
                let text = self.command_line.history_plain_text();
                if text.is_empty() {
                    Task::none()
                } else {
                    iced::clipboard::write(text).discard()
                }
            }

            Message::CommandHistoryClear => {
                self.command_line.clear_history();
                self.history_content = iced::widget::text_editor::Content::new();
                Task::none()
            }

            Message::PerfCopy => {
                let text = crate::perf::snapshot_text();
                if text.is_empty() {
                    Task::none()
                } else {
                    iced::clipboard::write(text).discard()
                }
            }

            Message::PerfClear => {
                crate::perf::clear();
                Task::none()
            }

            Message::CommandHistoryEdit(action) => {
                // Read-only: drop edits, keep selection/cursor actions, and
                // route wheel input to the outer scrollbar. The text editor
                // remains one selectable buffer while the scrollbar stays
                // visible and draggable.
                if let iced::widget::text_editor::Action::Scroll { lines } = action {
                    iced::widget::operation::scroll_by(
                        iced::widget::Id::new(crate::ui::command_line::HISTORY_SCROLL_ID),
                        iced::widget::scrollable::AbsoluteOffset {
                            x: 0.0,
                            y: lines as f32 * 14.0,
                        },
                    )
                } else {
                    if !action.is_edit() {
                        self.history_content.perform(action);
                    }
                    Task::none()
                }
            }

            Message::CommandSuggestionPick(cmd) => {
                self.command_line.input.clear();
                self.command_line.autocomplete_cursor = None;
                self.command_line.close_history();
                self.dispatch_command(&cmd)
            }

            Message::CommandOptionPick(kw) => {
                // Clicking an option button feeds its keyword to the active
                // command through the same path as typed text; an empty keyword
                // finishes the step like Enter. (#304)
                self.command_line.input.clear();
                self.command_line.close_history();
                if kw.is_empty() {
                    return self.feed_command(crate::command::StepInput::Enter);
                }
                self.feed_active_cmd(&kw);
                Task::none()
            }

            Message::CommandSubmit => self.on_command_submit(),

            Message::CommandSpace => {
                // Space is a literal space inside the MText preview; otherwise
                // it finalises the active command like Enter.
                if self.mtext_editor.as_ref().is_some_and(|e| e.show_preview) {
                    self.mtext_type(" ");
                    return Task::none();
                }
                // A leading `>` (or the persistent `>` toggle) puts the command
                // line in "literal space" mode so the user can type arguments
                // that contain spaces; otherwise Space works like Enter. The
                // typed `>` is stripped on submit.
                if self.command_line.literal_spaces || self.command_line.input.starts_with('>') {
                    self.command_line.input.push(' ');
                    return Task::none();
                }
                return self.update(Message::CommandFinalize);
            }
            Message::CommandFinalize => self.on_command_finalize(),

            Message::CommandEscape => {
                let panel = &mut self.tabs[self.active_tab].properties;
                if panel.hatch_pattern_picker_open {
                    panel.hatch_pattern_picker_open = false;
                    panel.hatch_pattern_search.clear();
                    panel.hatch_pattern_focus = 0;
                    Task::none()
                } else {
                    self.on_command_escape()
                }
            }

            Message::Command(cmd) => {
                // Close viewport context menu if open.
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().context_menu = None;
                // Any command also dismisses the Isolate action menu.
                self.isolate_popup_open = false;
                // "Pick window" (PLOTWINDOW) from Page Setup needs the backdrop
                // gone so the viewport pick lands; every other command leaves an
                // open modal (and its staged edits) untouched.
                if cmd.trim().eq_ignore_ascii_case("PLOTWINDOW")
                    || cmd.trim().eq_ignore_ascii_case("PW")
                {
                    self.close_active_modal();
                }
                self.dispatch_command(&cmd)
            }

            Message::ToggleLayers => {
                if self.active_modal == Some(super::ModalKind::Layers) {
                    self.ribbon.deactivate_tool_if("LAYERS");
                    self.active_modal = None;
                    self.reset_modal_geometry();
                } else {
                    self.sync_ribbon_layers();
                    self.active_modal = Some(super::ModalKind::Layers);
                }
                Task::none()
            }

            Message::LayerStateManagerOpen => {
                let i = self.active_tab;
                self.ribbon.close_dropdown();
                if self.tabs[i].is_start {
                    self.command_line
                        .push_info(crate::t!("Open or create a drawing to manage layer states.").as_ref());
                    return Task::none();
                }
                let mut names: Vec<String> = self.tabs[i]
                    .scene
                    .document
                    .layer_states()
                    .into_iter()
                    .map(|state| state.name)
                    .collect();
                names.sort_by_key(|name| name.to_lowercase());
                self.load_layer_state_editor(names.into_iter().next());
                self.active_modal = Some(super::ModalKind::LayerStateManager);
                Task::none()
            }
            Message::AecMaterialManagerOpen => {
                if !self.aec_require_project(Message::AecMaterialManagerOpen) {
                    return Task::none();
                }
                self.ribbon.close_dropdown();
                self.aec_style_library = Some(
                    crate::modules::aec::engine::library::combined_style_library(
                        self.aec_project_explorer_file.as_ref(),
                    ),
                );
                self.aec_style_manager_filter.clear();
                self.aec_style_manager_selected_material = None;
                self.aec_style_manager_selected_wall_style = None;
                self.aec_style_manager_material_editing_id = None;
                self.aec_style_manager_material_form_open = false;
                self.aec_style_manager_wall_style_editing_id = None;
                self.aec_style_manager_wall_style_form_open = false;
                self.aec_style_manager_wall_style_layers.clear();
                self.refresh_aec_material_linetype_combo();
                self.active_modal = Some(super::ModalKind::AecMaterialManager);
                Task::none()
            }
            Message::AecWallStyleManagerOpen => {
                if !self.aec_require_project(Message::AecWallStyleManagerOpen) {
                    return Task::none();
                }
                self.ribbon.close_dropdown();
                self.aec_style_library = Some(
                    crate::modules::aec::engine::library::combined_style_library(
                        self.aec_project_explorer_file.as_ref(),
                    ),
                );
                self.aec_style_manager_filter.clear();
                self.aec_style_manager_selected_material = None;
                self.aec_style_manager_selected_wall_style = None;
                self.aec_style_manager_material_editing_id = None;
                self.aec_style_manager_material_form_open = false;
                self.aec_style_manager_wall_style_editing_id = None;
                self.aec_style_manager_wall_style_form_open = false;
                self.aec_style_manager_wall_style_layers.clear();
                self.aec_style_manager_profile_selected = None;
                self.aec_style_manager_profile_contour_explicit = false;
                self.aec_style_manager_profile_contour_selection.clear();
                self.aec_style_manager_profile_solid_explicit = false;
                self.aec_style_manager_profile_solid_selection.clear();
                self.aec_style_manager_profile_hatch_angle.clear();
                self.aec_style_manager_profile_hatch_relative = false;
                // Load (or refresh) the DisplayConfig library so the
                // "Darstellungs-Profile" table has data even if the Plan
                // Manager was never opened this session.
                self.aec_plan_library = Some(
                    crate::modules::aec::engine::project::resolve_display_config_library(
                        self.aec_project_explorer_file.as_ref(),
                    ),
                );
                self.active_modal = Some(super::ModalKind::AecWallStyleManager);
                Task::none()
            }
            Message::AecProjectExplorerOpen => {
                self.ribbon.close_dropdown();
                self.active_modal = Some(super::ModalKind::AecProjectExplorer);
                Task::none()
            }
            Message::AecProjectExplorerNew => {
                self.aec_project_explorer_file =
                    Some(crate::modules::aec::engine::project::ProjectFile::default());
                self.aec_project_explorer_path = None;
                self.aec_project_explorer_selected_building = None;
                self.aec_project_explorer_selected_storey = None;
                self.aec_project_explorer_new_building_name.clear();
                self.aec_project_explorer_new_storey_name.clear();
                self.aec_project_explorer_new_storey_elevation = "0.0".to_string();
                self.aec_project_explorer_new_storey_drawing.clear();
                // Refresh the "Planart" library so the status-bar picker
                // reflects this (empty) project's library immediately,
                // instead of still showing a previously loaded drawing's
                // global/library-file entries until the Plan Manager is
                // opened once.
                self.aec_plan_library = Some(
                    crate::modules::aec::engine::project::resolve_display_config_library(
                        self.aec_project_explorer_file.as_ref(),
                    ),
                );
                // Coming from the project-required gate: replay the tool the
                // user originally asked for (e.g. AEC_WALL) instead of just
                // opening the explorer and leaving them stuck; fall back to
                // opening the explorer if there is nothing to resume (i.e.
                // this was invoked directly via AEC_PROJECTEXPLORER).
                if self.active_modal == Some(super::ModalKind::AecProjectRequired) {
                    self.reset_modal_geometry();
                    self.active_modal = None;
                    if let Some(resume) = self.aec_project_required_resume.take() {
                        return Task::done(resume);
                    }
                    self.active_modal = Some(super::ModalKind::AecProjectExplorer);
                }
                Task::none()
            }
            Message::AecProjectExplorerLoad => Task::perform(
                async {
                    crate::sys::file_dialog()
                        .set_title("Open Project")
                        .add_filter("OpenCADStudio Project", &["ocsproj", "OCSPROJ"])
                        .add_filter("All Files", &["*"])
                        .pick_file()
                        .await
                        .map(|h| crate::sys::handle_path(&h))
                },
                Message::AecProjectExplorerLoadResult,
            ),
            Message::AecProjectExplorerLoadResult(None) => Task::none(),
            Message::AecProjectExplorerLoadResult(Some(path)) => {
                match crate::modules::aec::engine::project::ProjectFile::load(&path) {
                    Ok(project) => {
                        self.aec_project_explorer_file = Some(project);
                        self.aec_project_explorer_path = Some(path);
                        self.aec_project_explorer_selected_building = None;
                        self.aec_project_explorer_selected_storey = None;
                        // Same reasoning as `AecProjectExplorerNew`: make the
                        // freshly loaded project's own "Planart" library
                        // visible right away instead of only after the Plan
                        // Manager is opened once.
                        self.aec_plan_library = Some(
                            crate::modules::aec::engine::project::resolve_display_config_library(
                                self.aec_project_explorer_file.as_ref(),
                            ),
                        );
                        // Same reasoning as `AecProjectExplorerNew`: replay
                        // the originally requested tool instead of just
                        // opening the explorer and leaving the user stuck.
                        if self.active_modal == Some(super::ModalKind::AecProjectRequired) {
                            self.reset_modal_geometry();
                            self.active_modal = None;
                            if let Some(resume) = self.aec_project_required_resume.take() {
                                return Task::done(resume);
                            }
                            self.active_modal = Some(super::ModalKind::AecProjectExplorer);
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
            Message::AecProjectExplorerSave => {
                self.aec_project_explorer_apply_pending_edits();
                if self.aec_project_explorer_path.is_some() {
                    self.aec_project_explorer_persist();
                    Task::none()
                } else {
                    self.update(Message::AecProjectExplorerSaveAs)
                }
            }
            Message::AecProjectExplorerSaveAs => Task::perform(
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
                Message::AecProjectExplorerSaveAsResult,
            ),
            Message::AecProjectExplorerSaveAsResult(None) => Task::none(),
            Message::AecProjectExplorerSaveAsResult(Some(path)) => {
                self.aec_project_explorer_apply_pending_edits();
                self.aec_project_explorer_path = Some(path);
                self.aec_project_explorer_persist();
                Task::none()
            }
            Message::AecProjectExplorerSelectBuilding(bid) => {
                self.aec_project_explorer_selected_building = Some(bid);
                self.aec_project_explorer_selected_storey = None;
                self.aec_project_explorer_edit_building_name = self
                    .aec_project_explorer_file
                    .as_ref()
                    .and_then(|p| p.buildings.iter().find(|b| b.id == bid))
                    .map(|b| b.name.clone())
                    .unwrap_or_default();
                Task::none()
            }
            Message::AecProjectExplorerSelectStorey(bid, sid) => {
                self.aec_project_explorer_selected_building = Some(bid);
                self.aec_project_explorer_selected_storey = Some((bid, sid));
                let storey = self
                    .aec_project_explorer_file
                    .as_ref()
                    .and_then(|p| p.buildings.iter().find(|b| b.id == bid))
                    .and_then(|b| b.storeys.iter().find(|s| s.id == sid));
                self.aec_project_explorer_edit_storey_name =
                    storey.map(|s| s.name.clone()).unwrap_or_default();
                self.aec_project_explorer_edit_elevation = storey
                    .map(|s| format!("{:.3}", s.elevation))
                    .unwrap_or_default();
                self.aec_project_explorer_edit_storey_drawing =
                    storey.map(|s| s.drawing_path.clone()).unwrap_or_default();
                Task::none()
            }
            Message::AecProjectExplorerOpenStorey(bid, sid) => {
                let Some(project) = self.aec_project_explorer_file.as_ref() else {
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
                    .aec_project_explorer_path
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
            Message::AecProjectExplorerAddBuilding => {
                let name = self.aec_project_explorer_new_building_name.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                let project = self
                    .aec_project_explorer_file
                    .get_or_insert_with(crate::modules::aec::engine::project::ProjectFile::default);
                let building = crate::modules::aec::engine::project::Building::new(name);
                let bid = building.id;
                project.buildings.push(building);
                self.aec_project_explorer_selected_building = Some(bid);
                self.aec_project_explorer_selected_storey = None;
                self.aec_project_explorer_edit_building_name.clear();
                self.aec_project_explorer_new_building_name.clear();
                self.aec_project_explorer_persist_if_pathed();
                Task::none()
            }
            Message::AecProjectExplorerEditBuildingName(_bid, name) => {
                self.aec_project_explorer_edit_building_name = name;
                Task::none()
            }
            Message::AecProjectExplorerSaveBuildingEdits(bid) => {
                let name = self.aec_project_explorer_edit_building_name.clone();
                if let Some(project) = self.aec_project_explorer_file.as_mut() {
                    if let Some(building) = project.buildings.iter_mut().find(|b| b.id == bid) {
                        building.name = name;
                    }
                }
                self.aec_project_explorer_persist_if_pathed();
                Task::none()
            }
            Message::AecProjectExplorerEditStoreyName(_bid, _sid, name) => {
                self.aec_project_explorer_edit_storey_name = name;
                Task::none()
            }
            Message::AecProjectExplorerEditStoreyElevation(_bid, _sid, text) => {
                self.aec_project_explorer_edit_elevation = text;
                Task::none()
            }
            Message::AecProjectExplorerEditStoreyDrawing(_bid, _sid, text) => {
                self.aec_project_explorer_edit_storey_drawing = text;
                Task::none()
            }
            Message::AecProjectExplorerSaveStoreyEdits(bid, sid) => {
                let name = self.aec_project_explorer_edit_storey_name.clone();
                let drawing = self.aec_project_explorer_edit_storey_drawing.clone();
                let elevation = self.aec_project_explorer_edit_elevation.trim().parse::<f64>().ok();
                if let Some(project) = self.aec_project_explorer_file.as_mut() {
                    if let Some(storey) = project
                        .buildings
                        .iter_mut()
                        .find(|b| b.id == bid)
                        .and_then(|b| b.storeys.iter_mut().find(|s| s.id == sid))
                    {
                        storey.name = name;
                        storey.drawing_path = drawing;
                        if let Some(value) = elevation {
                            storey.elevation = value;
                        }
                    }
                }
                self.aec_project_explorer_persist_if_pathed();
                Task::none()
            }
            Message::AecProjectExplorerRequestDeleteBuilding(bid) => {
                self.aec_project_explorer_pending_delete =
                    Some(AecProjectExplorerDeleteTarget::Building(bid));
                Task::none()
            }
            Message::AecProjectExplorerRequestDeleteStorey(bid, sid) => {
                self.aec_project_explorer_pending_delete =
                    Some(AecProjectExplorerDeleteTarget::Storey(bid, sid));
                Task::none()
            }
            Message::AecProjectExplorerCancelDelete => {
                self.aec_project_explorer_pending_delete = None;
                Task::none()
            }
            Message::AecProjectExplorerConfirmDelete => {
                match self.aec_project_explorer_pending_delete.take() {
                    Some(AecProjectExplorerDeleteTarget::Building(bid)) => {
                        if let Some(project) = self.aec_project_explorer_file.as_mut() {
                            if let Some(bi) = project.building_index(bid) {
                                project.buildings.remove(bi);
                            }
                        }
                        self.aec_project_explorer_selected_building = None;
                        self.aec_project_explorer_selected_storey = None;
                        self.aec_project_explorer_persist_if_pathed();
                    }
                    Some(AecProjectExplorerDeleteTarget::Storey(bid, sid)) => {
                        if let Some(project) = self.aec_project_explorer_file.as_mut() {
                            if let Some(building) =
                                project.building_index(bid).and_then(|bi| project.buildings.get_mut(bi))
                            {
                                if let Some(si) = building.storey_index(sid) {
                                    building.storeys.remove(si);
                                }
                            }
                        }
                        self.aec_project_explorer_selected_storey = None;
                        self.aec_project_explorer_persist_if_pathed();
                    }
                    None => {}
                }
                Task::none()
            }
            Message::AecProjectExplorerAddStorey => {
                let Some(bid) = self.aec_project_explorer_selected_building else {
                    self.command_line.push_info(
                        crate::t!("AEC Project Explorer: select a building first.").as_ref(),
                    );
                    return Task::none();
                };
                let name = self.aec_project_explorer_new_storey_name.trim().to_string();
                let drawing = self.aec_project_explorer_new_storey_drawing.trim().to_string();
                if name.is_empty() {
                    self.command_line.push_info(
                        crate::t!("AEC Project Explorer: a storey name is required.").as_ref(),
                    );
                    return Task::none();
                }
                let elevation = self
                    .aec_project_explorer_new_storey_elevation
                    .trim()
                    .parse::<f64>()
                    .unwrap_or(0.0);
                let Some(project) = self.aec_project_explorer_file.as_mut() else {
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
                self.aec_project_explorer_selected_storey = Some((bid, sid));
                self.aec_project_explorer_new_storey_name.clear();
                self.aec_project_explorer_new_storey_drawing.clear();
                self.aec_project_explorer_persist_if_pathed();
                Task::none()
            }
            Message::AecProjectExplorerNewBuildingNameChanged(v) => {
                self.aec_project_explorer_new_building_name = v;
                Task::none()
            }
            Message::AecProjectExplorerNewStoreyNameChanged(v) => {
                self.aec_project_explorer_new_storey_name = v;
                Task::none()
            }
            Message::AecProjectExplorerNewStoreyElevationChanged(v) => {
                self.aec_project_explorer_new_storey_elevation = v;
                Task::none()
            }
            Message::AecProjectExplorerNewStoreyDrawingChanged(v) => {
                self.aec_project_explorer_new_storey_drawing = v;
                Task::none()
            }
            Message::AecProjectExplorerPickStoreyDrawing => Task::perform(
                async {
                    crate::sys::file_dialog()
                        .set_title("Select Storey Drawing")
                        .add_filter("CAD Files", &["dwg", "dxf", "DWG", "DXF"])
                        .add_filter("All Files", &["*"])
                        .pick_file()
                        .await
                        .map(|h| crate::sys::handle_path(&h))
                },
                Message::AecProjectExplorerPickStoreyDrawingResult,
            ),
            Message::AecProjectExplorerPickStoreyDrawingResult(None) => Task::none(),
            Message::AecProjectExplorerPickStoreyDrawingResult(Some(path)) => {
                // Prefer a path relative to the project file when possible.
                let display = if let Some(base) = self
                    .aec_project_explorer_path
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
                self.aec_project_explorer_new_storey_drawing = display;
                Task::none()
            }
            // `bid`/`sid` aren't needed in the result — only one storey can be
            // selected/edited at a time, so the pick always targets the
            // currently selected storey's edit buffer.
            Message::AecProjectExplorerPickEditStoreyDrawing(_bid, _sid) => Task::perform(
                async {
                    crate::sys::file_dialog()
                        .set_title("Select Storey Drawing")
                        .add_filter("CAD Files", &["dwg", "dxf", "DWG", "DXF"])
                        .add_filter("All Files", &["*"])
                        .pick_file()
                        .await
                        .map(|h| crate::sys::handle_path(&h))
                },
                Message::AecProjectExplorerPickEditStoreyDrawingResult,
            ),
            Message::AecProjectExplorerPickEditStoreyDrawingResult(None) => Task::none(),
            Message::AecProjectExplorerPickEditStoreyDrawingResult(Some(path)) => {
                // Prefer a path relative to the project file when possible.
                let display = if let Some(base) = self
                    .aec_project_explorer_path
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
                self.aec_project_explorer_edit_storey_drawing = display;
                Task::none()
            }
            Message::AecProjectExplorerMigrateLibraries => {
                if let Some(project) = self.aec_project_explorer_file.as_mut() {
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
            Message::AecPlanManagerOpen => {
                if !self.aec_require_project(Message::AecPlanManagerOpen) {
                    return Task::none();
                }
                self.ribbon.close_dropdown();
                self.aec_plan_library = Some(
                    crate::modules::aec::engine::project::resolve_display_config_library(
                        self.aec_project_explorer_file.as_ref(),
                    ),
                );
                // Layer-Filter-UI: the multi-select checklist needs the
                // full set of wall styles/layers currently in effect, the
                // same "project overrides global" resolution the
                // Material/WallStyle managers already use.
                self.aec_style_library = Some(
                    crate::modules::aec::engine::project::resolve_style_library(
                        self.aec_project_explorer_file.as_ref(),
                    ),
                );
                self.aec_plan_manager_filter.clear();
                self.aec_plan_manager_selected = None;
                self.aec_plan_manager_editing_name = None;
                self.aec_plan_manager_form_open = false;
                self.refresh_aec_material_linetype_combo();
                self.aec_plan_manager_wall_styles = self
                    .aec_style_library
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
                self.active_modal = Some(super::ModalKind::AecPlanManager);
                Task::none()
            }
            Message::AecPlanManagerClose => {
                self.active_modal = None;
                Task::none()
            }
            Message::AecPlanManagerFilter(value) => {
                self.aec_plan_manager_filter = value;
                Task::none()
            }
            Message::AecPlanManagerSelect(name) => {
                if let Some(cfg) = self
                    .aec_plan_library
                    .as_ref()
                    .and_then(|lib| lib.find(&name))
                    .cloned()
                {
                    self.aec_plan_manager_editing_name = Some(cfg.name.clone());
                    self.aec_plan_manager_name = cfg.name.clone();
                    self.aec_plan_manager_discipline = cfg.discipline.clone();
                    self.aec_plan_manager_scale =
                        cfg.scale.map(|s| s.to_string()).unwrap_or_default();
                    self.aec_plan_manager_planning_stage = cfg.planning_stage;
                    self.aec_plan_manager_view_type = cfg.view_type.clone();
                    self.aec_plan_manager_load_phase_filter_buffers(cfg.phase_filter.as_ref());
                    self.aec_plan_manager_load_display_buffers(&cfg);
                    self.aec_plan_manager_form_open = true;
                }
                self.aec_plan_manager_selected = Some(name);
                Task::none()
            }
            Message::AecPlanManagerNew => {
                self.aec_plan_manager_selected = None;
                self.aec_plan_manager_editing_name = None;
                self.aec_plan_manager_name.clear();
                self.aec_plan_manager_discipline.clear();
                self.aec_plan_manager_scale.clear();
                self.aec_plan_manager_planning_stage =
                    crate::modules::aec::engine::plan_view::PlanningStage::Design;
                self.aec_plan_manager_view_type =
                    crate::modules::aec::engine::plan_view::ViewType::FloorPlan;
                self.aec_plan_manager_load_phase_filter_buffers(None);
                self.aec_plan_manager_reset_display_buffers();
                self.aec_plan_manager_form_open = true;
                Task::none()
            }
            Message::AecPlanManagerDuplicate => {
                let source_name = self
                    .aec_plan_manager_editing_name
                    .clone()
                    .or_else(|| self.aec_plan_manager_selected.clone());
                let Some(source_name) = source_name else {
                    return Task::none();
                };
                let Some(cfg) = self
                    .aec_plan_library
                    .as_ref()
                    .and_then(|lib| lib.find(&source_name))
                    .cloned()
                else {
                    return Task::none();
                };
                let existing_names: Vec<String> = self
                    .aec_plan_library
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
                self.aec_plan_manager_selected = None;
                self.aec_plan_manager_editing_name = None;
                self.aec_plan_manager_name = name;
                self.aec_plan_manager_discipline = cfg.discipline.clone();
                self.aec_plan_manager_scale = cfg.scale.map(|s| s.to_string()).unwrap_or_default();
                self.aec_plan_manager_load_phase_filter_buffers(cfg.phase_filter.as_ref());
                self.aec_plan_manager_planning_stage = cfg.planning_stage;
                self.aec_plan_manager_view_type = cfg.view_type.clone();
                self.aec_plan_manager_load_display_buffers(&cfg);
                self.aec_plan_manager_editing_id = None;
                self.aec_plan_manager_form_open = true;
                Task::none()
            }
            Message::AecPlanManagerDelete => {
                if let Some(name) = self.aec_plan_manager_selected.clone() {
                    let lib_snapshot = if let Some(lib) = self.aec_plan_library.as_mut() {
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
                self.aec_plan_manager_selected = None;
                self.aec_plan_manager_editing_name = None;
                self.aec_plan_manager_form_open = false;
                Task::none()
            }
            Message::AecPlanManagerNameChanged(value) => {
                self.aec_plan_manager_name = value;
                Task::none()
            }
            Message::AecPlanManagerDisciplineChanged(value) => {
                self.aec_plan_manager_discipline = value;
                Task::none()
            }
            Message::AecPlanManagerScaleChanged(value) => {
                self.aec_plan_manager_scale = value;
                Task::none()
            }
            Message::AecPlanManagerPlanningStageChanged(stage) => {
                self.aec_plan_manager_planning_stage = stage;
                Task::none()
            }
            Message::AecPlanManagerViewTypeChanged(view_type) => {
                self.aec_plan_manager_view_type = view_type;
                Task::none()
            }
            Message::AecPlanManagerPhaseVisibleToggle(phase, visible) => {
                match phase {
                    crate::modules::aec::engine::plan_view::PlanPhase::Existing => {
                        self.aec_plan_manager_phase_filter_visible_existing = visible;
                    }
                    crate::modules::aec::engine::plan_view::PlanPhase::Demolition => {
                        self.aec_plan_manager_phase_filter_visible_demolition = visible;
                    }
                    crate::modules::aec::engine::plan_view::PlanPhase::New => {
                        self.aec_plan_manager_phase_filter_visible_new = visible;
                    }
                }
                Task::none()
            }
            Message::AecPlanManagerDemolitionStyleLineTypeChanged(value) => {
                self.aec_plan_manager_demolition_style_line_type = value;
                Task::none()
            }
            Message::AecPlanManagerDemolitionStyleLineColorChanged(value) => {
                self.aec_plan_manager_demolition_style_line_color = value;
                Task::none()
            }
            Message::AecPlanManagerDemolitionStyleHatchPatternChanged(value) => {
                self.aec_plan_manager_demolition_style_hatch_pattern = value;
                self.aec_plan_manager_demolition_style_hatch_picker_open = false;
                Task::none()
            }
            Message::AecPlanManagerDemolitionStyleHatchPickerToggle => {
                self.aec_plan_manager_demolition_style_hatch_picker_open =
                    !self.aec_plan_manager_demolition_style_hatch_picker_open;
                Task::none()
            }
            Message::AecPlanManagerDemolitionStyleHatchColorChanged(value) => {
                self.aec_plan_manager_demolition_style_hatch_color = value;
                Task::none()
            }
            Message::AecPlanManagerDemolitionStyleFillColorChanged(value) => {
                self.aec_plan_manager_demolition_style_fill_color = value;
                Task::none()
            }
            Message::AecPlanManagerDemolitionStyleLineColorPickerToggle => {
                self.aec_plan_manager_demolition_style_line_color_picker_open =
                    !self.aec_plan_manager_demolition_style_line_color_picker_open;
                Task::none()
            }
            Message::AecPlanManagerDemolitionStyleHatchColorPickerToggle => {
                self.aec_plan_manager_demolition_style_hatch_color_picker_open =
                    !self.aec_plan_manager_demolition_style_hatch_color_picker_open;
                Task::none()
            }
            Message::AecPlanManagerDemolitionStyleFillColorPickerToggle => {
                self.aec_plan_manager_demolition_style_fill_color_picker_open =
                    !self.aec_plan_manager_demolition_style_fill_color_picker_open;
                Task::none()
            }
            Message::AecPlanManagerExistingStyleLineTypeChanged(value) => {
                self.aec_plan_manager_existing_style_line_type = value;
                Task::none()
            }
            Message::AecPlanManagerExistingStyleLineColorChanged(value) => {
                self.aec_plan_manager_existing_style_line_color = value;
                Task::none()
            }
            Message::AecPlanManagerExistingStyleHatchPatternChanged(value) => {
                self.aec_plan_manager_existing_style_hatch_pattern = value;
                self.aec_plan_manager_existing_style_hatch_picker_open = false;
                Task::none()
            }
            Message::AecPlanManagerExistingStyleHatchPickerToggle => {
                self.aec_plan_manager_existing_style_hatch_picker_open =
                    !self.aec_plan_manager_existing_style_hatch_picker_open;
                Task::none()
            }
            Message::AecPlanManagerExistingStyleHatchColorChanged(value) => {
                self.aec_plan_manager_existing_style_hatch_color = value;
                Task::none()
            }
            Message::AecPlanManagerExistingStyleFillColorChanged(value) => {
                self.aec_plan_manager_existing_style_fill_color = value;
                Task::none()
            }
            Message::AecPlanManagerExistingStyleLineColorPickerToggle => {
                self.aec_plan_manager_existing_style_line_color_picker_open =
                    !self.aec_plan_manager_existing_style_line_color_picker_open;
                Task::none()
            }
            Message::AecPlanManagerExistingStyleHatchColorPickerToggle => {
                self.aec_plan_manager_existing_style_hatch_color_picker_open =
                    !self.aec_plan_manager_existing_style_hatch_color_picker_open;
                Task::none()
            }
            Message::AecPlanManagerExistingStyleFillColorPickerToggle => {
                self.aec_plan_manager_existing_style_fill_color_picker_open =
                    !self.aec_plan_manager_existing_style_fill_color_picker_open;
                Task::none()
            }
            Message::AecPlanManagerRepresentationChanged(mode) => {
                self.aec_plan_manager_default_representation = mode;
                Task::none()
            }
            Message::AecPlanManagerComponentVisibleToggle(kind, visible) => {
                if visible {
                    self.aec_plan_manager_component_visibility.remove(&kind);
                } else {
                    self.aec_plan_manager_component_visibility.insert(kind, false);
                }
                Task::none()
            }
            Message::AecPlanManagerOverlayStyleSelect(style_id) => {
                self.aec_plan_manager_write_overlay_buffers();
                self.aec_plan_manager_write_overlay_contour_hatch();
                self.aec_plan_manager_overlay_style_id = Some(style_id);
                self.aec_plan_manager_overlay_layer_id = self
                    .aec_plan_manager_overlay_style_id
                    .as_ref()
                    .and_then(|sid| {
                        self.aec_plan_manager_style_overlays
                            .get(sid)
                            .and_then(|o| o.layer_props.keys().next().copied())
                    });
                self.aec_plan_manager_load_overlay_layer_buffers();
                self.aec_plan_manager_load_overlay_contour_hatch_buffers();
                Task::none()
            }
            Message::AecPlanManagerOverlayLayerSelect(layer_id) => {
                self.aec_plan_manager_write_overlay_buffers();
                self.aec_plan_manager_overlay_layer_id = Some(layer_id);
                self.aec_plan_manager_load_overlay_layer_buffers();
                Task::none()
            }
            Message::AecPlanManagerOverlayAddStyle(style_id) => {
                self.aec_plan_manager_write_overlay_buffers();
                self.aec_plan_manager_write_overlay_contour_hatch();
                self.aec_plan_manager_style_overlays
                    .entry(style_id.clone())
                    .or_default();
                self.aec_plan_manager_overlay_style_id = Some(style_id);
                self.aec_plan_manager_overlay_layer_id = None;
                self.aec_plan_manager_clear_overlay_field_buffers();
                self.aec_plan_manager_load_overlay_contour_hatch_buffers();
                Task::none()
            }
            Message::AecPlanManagerOverlayRemoveStyle => {
                if let Some(style_id) = self.aec_plan_manager_overlay_style_id.take() {
                    self.aec_plan_manager_style_overlays.remove(&style_id);
                }
                self.aec_plan_manager_overlay_layer_id = None;
                self.aec_plan_manager_overlay_style_id =
                    self.aec_plan_manager_style_overlays.keys().next().cloned();
                self.aec_plan_manager_load_overlay_layer_buffers();
                self.aec_plan_manager_load_overlay_contour_hatch_buffers();
                Task::none()
            }
            Message::AecPlanManagerOverlayLineTypeChanged(value) => {
                self.aec_plan_manager_overlay_line_type = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            Message::AecPlanManagerOverlayLineColorChanged(value) => {
                self.aec_plan_manager_overlay_line_color = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            Message::AecPlanManagerOverlayHatchPatternChanged(value) => {
                self.aec_plan_manager_overlay_hatch_pattern = value;
                self.aec_plan_manager_overlay_hatch_picker_open = false;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            Message::AecPlanManagerOverlayHatchColorChanged(value) => {
                self.aec_plan_manager_overlay_hatch_color = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            Message::AecPlanManagerOverlayHatchScaleChanged(value) => {
                self.aec_plan_manager_overlay_hatch_scale = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            Message::AecPlanManagerOverlayHatchAngleChanged(value) => {
                self.aec_plan_manager_overlay_hatch_angle = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            Message::AecPlanManagerOverlayHatchAngleRelativeChanged(value) => {
                self.aec_plan_manager_overlay_hatch_angle_relative = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            Message::AecPlanManagerOverlayFillColorChanged(value) => {
                self.aec_plan_manager_overlay_fill_color = value;
                self.aec_plan_manager_write_overlay_buffers();
                Task::none()
            }
            Message::AecPlanManagerOverlayLineColorPickerToggle => {
                self.aec_plan_manager_overlay_line_color_picker_open =
                    !self.aec_plan_manager_overlay_line_color_picker_open;
                Task::none()
            }
            Message::AecPlanManagerOverlayHatchPickerToggle => {
                self.aec_plan_manager_overlay_hatch_picker_open =
                    !self.aec_plan_manager_overlay_hatch_picker_open;
                Task::none()
            }
            Message::AecPlanManagerOverlayHatchColorPickerToggle => {
                self.aec_plan_manager_overlay_hatch_color_picker_open =
                    !self.aec_plan_manager_overlay_hatch_color_picker_open;
                Task::none()
            }
            Message::AecPlanManagerOverlayFillColorPickerToggle => {
                self.aec_plan_manager_overlay_fill_color_picker_open =
                    !self.aec_plan_manager_overlay_fill_color_picker_open;
                Task::none()
            }
            Message::AecPlanManagerContourHatchPatternChanged(value) => {
                self.aec_plan_manager_contour_hatch_pattern = value;
                self.aec_plan_manager_contour_hatch_picker_open = false;
                self.aec_plan_manager_write_overlay_contour_hatch();
                Task::none()
            }
            Message::AecPlanManagerContourHatchColorChanged(value) => {
                self.aec_plan_manager_contour_hatch_color = value;
                self.aec_plan_manager_write_overlay_contour_hatch();
                Task::none()
            }
            Message::AecPlanManagerContourHatchScaleChanged(value) => {
                self.aec_plan_manager_contour_hatch_scale = value;
                self.aec_plan_manager_write_overlay_contour_hatch();
                Task::none()
            }
            Message::AecPlanManagerContourHatchAngleChanged(value) => {
                self.aec_plan_manager_contour_hatch_angle = value;
                self.aec_plan_manager_write_overlay_contour_hatch();
                Task::none()
            }
            Message::AecPlanManagerContourHatchAngleRelativeChanged(value) => {
                self.aec_plan_manager_contour_hatch_angle_relative = value;
                self.aec_plan_manager_write_overlay_contour_hatch();
                Task::none()
            }
            Message::AecPlanManagerContourHatchPickerToggle => {
                self.aec_plan_manager_contour_hatch_picker_open =
                    !self.aec_plan_manager_contour_hatch_picker_open;
                Task::none()
            }
            Message::AecPlanManagerContourHatchColorPickerToggle => {
                self.aec_plan_manager_contour_hatch_color_picker_open =
                    !self.aec_plan_manager_contour_hatch_color_picker_open;
                Task::none()
            }
            Message::AecPlanManagerOverlayLayerVis2d(visible) => {
                if let (Some(style_id), Some(layer_id)) = (
                    self.aec_plan_manager_overlay_style_id.clone(),
                    self.aec_plan_manager_overlay_layer_id,
                ) {
                    let overlay = self
                        .aec_plan_manager_style_overlays
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
            Message::AecPlanManagerOverlayLayerVis3d(visible) => {
                if let (Some(style_id), Some(layer_id)) = (
                    self.aec_plan_manager_overlay_style_id.clone(),
                    self.aec_plan_manager_overlay_layer_id,
                ) {
                    let overlay = self
                        .aec_plan_manager_style_overlays
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
            Message::AecPlanManagerApply => {
                let name = self.aec_plan_manager_name.trim().to_string();
                if name.is_empty() {
                    self.command_line.push_error(
                        crate::t!("AEC DisplayConfig Manager: name cannot be empty.").as_ref(),
                    );
                    return Task::none();
                }
                let discipline = self.aec_plan_manager_discipline.trim().to_string();
                let scale = self.aec_plan_manager_scale.trim().parse::<f64>().ok();

                let mut config = crate::modules::aec::engine::plan_view::DisplayConfig::new(
                    name.clone(),
                    discipline,
                    self.aec_plan_manager_planning_stage,
                    self.aec_plan_manager_view_type.clone(),
                );
                config.scale = scale;
                if let Some(id) = self.aec_plan_manager_editing_id {
                    config.id = id;
                }
                self.aec_plan_manager_write_overlay_buffers();
                self.aec_plan_manager_write_overlay_contour_hatch();
                config.default_representation = self.aec_plan_manager_default_representation;
                config.component_visibility = self.aec_plan_manager_component_visibility.clone();
                config.style_overlays = self.aec_plan_manager_style_overlays.clone();
                config.contour_hatch = None;

                // Two-stage Phasenfilter-Editor (Step 5): `phase_filter` is
                // now derived straight from the edit buffers.
                config.phase_filter = self.aec_plan_manager_build_phase_filter();

                // If renaming an existing entry, drop the old name first.
                if let Some(old_name) = self.aec_plan_manager_editing_name.clone() {
                    if old_name != name {
                        if let Some(lib) = self.aec_plan_library.as_mut() {
                            lib.remove(&old_name);
                        }
                    }
                }

                let lib = self
                    .aec_plan_library
                    .get_or_insert_with(crate::modules::aec::engine::library::DisplayConfigLibrary::empty);
                lib.upsert(config.clone());
                let lib_snapshot = lib.clone();
                match self.aec_save_display_config_library_preferring_project(&lib_snapshot) {
                    Ok(()) => self.command_line.push_info(
                        crate::t!("AEC DisplayConfig Manager: config saved.").as_ref(),
                    ),
                    Err(e) => self.command_line.push_error(
                        crate::tf!("AEC DisplayConfig Manager: failed to save library: {e}").as_ref(),
                    ),
                }

                // If this config is the active tab's active DisplayConfig,
                // re-apply it immediately so edits are reflected live.
                let i = self.active_tab;
                if !self.tabs[i].is_start
                    && self.tabs[i].active_display_config.as_deref() == Some(name.as_str())
                {
                    self.apply_active_display_config_to_tab(i);
                }

                self.aec_plan_manager_editing_name = Some(name.clone());
                self.aec_plan_manager_selected = Some(name);
                self.aec_plan_manager_editing_id = Some(config.id);
                Task::none()
            }
            Message::AecActiveDisplayConfigSelected(name) => {
                let i = self.active_tab;
                if self.tabs[i].is_start {
                    return Task::none();
                }
                self.tabs[i].active_display_config = name.clone();
                self.apply_active_display_config_to_tab(i);
                Task::none()
            }
            Message::AecRepresentationOverrideSelected(mode) => {
                let i = self.active_tab;
                if self.tabs[i].is_start {
                    return Task::none();
                }
                self.tabs[i].representation_override = mode;
                self.apply_active_display_config_to_tab(i);
                Task::none()
            }
            Message::SelectAndZoomTo(handle) => {
                let i = self.active_tab;
                self.tabs[i].scene.select_entity(handle, true);
                self.tabs[i].scene.remember_current_view();
                let _ = self.tabs[i].scene.zoom_to_entities(&[handle]);
                Task::none()
            }
            Message::AecStyleManagerFilter(value) => {
                self.aec_style_manager_filter = value;
                Task::none()
            }
            Message::AecStyleManagerSelectMaterial(id) => {
                let material = crate::modules::aec::engine::library::combined_material_entries(
                    self.aec_project_explorer_file.as_ref(),
                )
                .into_iter()
                .find(|e| e.material.id == id)
                .map(|e| e.material)
                .or_else(|| {
                    self.aec_style_library
                        .as_ref()
                        .and_then(|lib| lib.materials.iter().find(|m| m.id == id).cloned())
                });
                if let Some(material) = material {
                    self.aec_style_manager_material_editing_id = Some(material.id.clone());
                    self.aec_style_manager_material_name = material.name.clone();
                    self.aec_style_manager_material_hatch = material.hatch_pattern.clone();
                    self.aec_style_manager_material_color =
                        format!("#{:06X}", material.line_color);
                    self.aec_style_manager_material_line_type = material.line_type.clone();
                    self.aec_style_manager_material_category =
                        material.category.clone().unwrap_or_default();
                                        self.aec_style_manager_material_hatch_color = material
                        .hatch_color
                        .and_then(|c| c.rgb())
                        .map(|(r, g, b)| ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
                        .unwrap_or(material.line_color);
                    self.aec_style_manager_material_hatch_scale =
                        format!("{}", material.hatch_scale);
                    self.aec_style_manager_material_render_ref =
                        material.render_material_ref.clone().unwrap_or_default();
                    self.aec_style_manager_material_hatch_angle =
                        format!("{}", material.hatch_angle);
                    self.aec_style_manager_material_hatch_angle_relative =
                        material.hatch_angle_relative;
                    self.aec_style_manager_material_hatch_color_picker_open = false;
                    self.aec_style_manager_material_form_open = true;
                }
                self.aec_style_manager_selected_material = Some(id);
                self.aec_style_manager_selected_wall_style = None;
                self.refresh_aec_material_linetype_combo();
                Task::none()
            }
            Message::AecStyleManagerSelectWallStyle(id) => {
                let wall_style = crate::modules::aec::engine::library::combined_wall_style_entries(
                    self.aec_project_explorer_file.as_ref(),
                )
                .into_iter()
                .find(|e| e.wall_style.style.id == id)
                .map(|e| e.wall_style)
                .or_else(|| {
                    self.aec_style_library.as_ref().and_then(|lib| {
                        lib.wall_styles
                            .iter()
                            .find(|w| w.style.id == id)
                            .cloned()
                    })
                });
                if let Some(wall_style) = wall_style {
                    self.aec_style_manager_wall_style_editing_id = Some(wall_style.style.id.clone());
                    self.aec_style_manager_wall_style_name = wall_style.style.name.clone();
                    self.aec_style_manager_wall_style_parent =
                        wall_style.style.parent_style_id.clone();
                    self.aec_style_manager_wall_style_layers = wall_style
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
                    self.aec_style_manager_wall_style_form_open = true;
                }
                self.aec_style_manager_selected_wall_style = Some(id);
                self.aec_style_manager_selected_material = None;
                self.aec_style_manager_material_form_open = false;
                self.aec_style_manager_material_editing_id = None;
                self.aec_style_manager_profile_selected = None;
                self.aec_style_manager_profile_contour_explicit = false;
                self.aec_style_manager_profile_contour_selection.clear();
                self.aec_style_manager_profile_solid_explicit = false;
                self.aec_style_manager_profile_solid_selection.clear();
                self.aec_style_manager_profile_hatch_angle.clear();
                self.aec_style_manager_profile_hatch_relative = false;
                Task::none()
            }
            Message::AecStyleManagerWallStyleNew => {
                self.aec_style_manager_selected_material = None;
                self.aec_style_manager_selected_wall_style = None;
                self.aec_style_manager_wall_style_editing_id = None;
                self.aec_style_manager_wall_style_name.clear();
                self.aec_style_manager_wall_style_parent = None;
                self.aec_style_manager_wall_style_layers.clear();
                self.aec_style_manager_wall_style_form_open = true;
                self.aec_style_manager_material_form_open = false;
                self.aec_style_manager_material_editing_id = None;
                self.aec_style_manager_profile_selected = None;
                self.aec_style_manager_profile_contour_explicit = false;
                self.aec_style_manager_profile_contour_selection.clear();
                self.aec_style_manager_profile_solid_explicit = false;
                self.aec_style_manager_profile_solid_selection.clear();
                self.aec_style_manager_profile_hatch_angle.clear();
                self.aec_style_manager_profile_hatch_relative = false;
                Task::none()
            }
            Message::AecStyleManagerWallStyleNameChanged(value) => {
                self.aec_style_manager_wall_style_name = value;
                Task::none()
            }
            Message::AecStyleManagerWallStyleParentChanged(value) => {
                self.aec_style_manager_wall_style_parent = value;
                Task::none()
            }
            Message::AecStyleManagerWallStyleLayerAdd => {
                let material_id = self
                    .aec_style_library
                    .as_ref()
                    .and_then(|lib| lib.materials.first())
                    .map(|m| m.id.clone())
                    .unwrap_or_default();
                self.aec_style_manager_wall_style_layers
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
            Message::AecStyleManagerWallStyleLayerRemove(index) => {
                if index < self.aec_style_manager_wall_style_layers.len() {
                    self.aec_style_manager_wall_style_layers.remove(index);
                }
                Task::none()
            }
            Message::AecStyleManagerWallStyleLayerMaterialChanged(index, material_id) => {
                if let Some(layer) = self.aec_style_manager_wall_style_layers.get_mut(index) {
                    layer.material_id = material_id;
                }
                Task::none()
            }
            Message::AecStyleManagerWallStyleLayerThicknessChanged(index, thickness) => {
                if let Some(layer) = self.aec_style_manager_wall_style_layers.get_mut(index) {
                    layer.thickness = thickness;
                }
                Task::none()
            }
            Message::AecStyleManagerWallStyleLayerFunctionChanged(index, function) => {
                if let Some(layer) = self.aec_style_manager_wall_style_layers.get_mut(index) {
                    layer.function = function;
                }
                Task::none()
            }
            Message::AecStyleManagerWallStyleLayerAxisOffsetChanged(index, offset) => {
                if let Some(layer) = self.aec_style_manager_wall_style_layers.get_mut(index) {
                    layer.axis_offset = offset;
                }
                Task::none()
            }
            Message::AecStyleManagerWallStyleLayerBottomOffsetChanged(index, offset) => {
                if let Some(layer) = self.aec_style_manager_wall_style_layers.get_mut(index) {
                    layer.bottom_offset = offset;
                }
                Task::none()
            }
            Message::AecStyleManagerWallStyleLayerTopOffsetChanged(index, offset) => {
                if let Some(layer) = self.aec_style_manager_wall_style_layers.get_mut(index) {
                    layer.top_offset = offset;
                }
                Task::none()
            }
            Message::AecStyleManagerWallStyleLayerOverrideChanged(index, layer_name) => {
                if let Some(layer) = self.aec_style_manager_wall_style_layers.get_mut(index) {
                    layer.layer_override = layer_name;
                }
                Task::none()
            }
            Message::AecStyleManagerWallStyleLayerHatchOverrideChanged(index, hatch) => {
                if let Some(layer) = self.aec_style_manager_wall_style_layers.get_mut(index) {
                    layer.hatch_override = hatch;
                }
                Task::none()
            }
            Message::AecStyleManagerWallStyleLayerRoleTagChanged(index, role_tag) => {
                if let Some(layer) = self.aec_style_manager_wall_style_layers.get_mut(index) {
                    layer.role_tag = role_tag;
                }
                Task::none()
            }
            Message::AecStyleManagerWallStyleLayerMoveUp(index) => {
                if index > 0 && index < self.aec_style_manager_wall_style_layers.len() {
                    self.aec_style_manager_wall_style_layers.swap(index - 1, index);
                }
                Task::none()
            }
            Message::AecStyleManagerWallStyleLayerMoveDown(index) => {
                if index + 1 < self.aec_style_manager_wall_style_layers.len() {
                    self.aec_style_manager_wall_style_layers.swap(index, index + 1);
                }
                Task::none()
            }
            Message::AecStyleManagerWallStyleLayerDragStart(index) => {
                // Click-based "pick up / drop here" reordering: arms the
                // dragged row so a later drag-over on another row swaps it
                // into place. Clicking the already-armed row's handle again
                // cancels the drag (matches the toggle feel of `PaneMoveStart`).
                if self.aec_style_manager_wall_style_drag_index == Some(index) {
                    self.aec_style_manager_wall_style_drag_index = None;
                } else if index < self.aec_style_manager_wall_style_layers.len() {
                    self.aec_style_manager_wall_style_drag_index = Some(index);
                }
                Task::none()
            }
            Message::AecStyleManagerWallStyleLayerDragOver(target) => {
                if let Some(from) = self.aec_style_manager_wall_style_drag_index.take() {
                    let len = self.aec_style_manager_wall_style_layers.len();
                    if from != target && from < len && target < len {
                        let layer = self.aec_style_manager_wall_style_layers.remove(from);
                        self.aec_style_manager_wall_style_layers.insert(target, layer);
                    }
                }
                Task::none()
            }
            Message::AecStyleManagerWallStyleLayerDragEnd => {
                self.aec_style_manager_wall_style_drag_index = None;
                Task::none()
            }
            Message::AecStyleManagerWallStyleSortToggle => {
                self.aec_style_manager_wall_style_sort =
                    self.aec_style_manager_wall_style_sort.toggled();
                Task::none()
            }
            Message::AecStylePickerOpen(target) => {
                self.aec_style_library = Some(
                    crate::modules::aec::engine::library::combined_style_library(
                        self.aec_project_explorer_file.as_ref(),
                    ),
                );
                self.aec_style_picker_filter.clear();
                // Pre-select/highlight whatever is already assigned for this
                // target so the picker doesn't reopen with nothing
                // highlighted even though a value is already in use.
                self.aec_style_picker_selection = match target {
                    crate::app::StylePickerTarget::WallStyleParent => {
                        self.aec_style_manager_wall_style_parent.clone()
                    }
                    crate::app::StylePickerTarget::LayerMaterial(index) => self
                        .aec_style_manager_wall_style_layers
                        .get(index)
                        .map(|l| l.material_id.clone()),
                    crate::app::StylePickerTarget::LayerOverride(index) => self
                        .aec_style_manager_wall_style_layers
                        .get(index)
                        .map(|l| l.layer_override.clone()),
                    crate::app::StylePickerTarget::WallPropertiesStyle
                    | crate::app::StylePickerTarget::ActiveCommand => None,
                };
                self.active_modal = Some(crate::app::ModalKind::AecStylePicker { target });
                Task::none()
            }
            Message::AecStylePickerOpenForWallProperties(handles) => {
                self.aec_style_library = Some(
                    crate::modules::aec::engine::library::combined_style_library(
                        self.aec_project_explorer_file.as_ref(),
                    ),
                );
                self.aec_style_picker_filter.clear();
                // Pre-select/highlight the style already assigned to the
                // (first) wall being edited, so the picker doesn't reopen
                // with nothing highlighted even though a style is in use.
                self.aec_style_picker_selection = handles.first().and_then(|h| {
                    self.tabs[self.active_tab]
                        .scene
                        .document
                        .get_entity(*h)
                        .and_then(crate::modules::aec::commands::wall_from_entity)
                        .map(|v2| v2.style_id)
                });
                self.aec_style_picker_wall_handles = handles;
                self.active_modal = Some(crate::app::ModalKind::AecStylePicker {
                    target: crate::app::StylePickerTarget::WallPropertiesStyle,
                });
                Task::none()
            }
            Message::AecStylePickerOpenForActiveCommand => {
                self.aec_style_library = Some(
                    crate::modules::aec::engine::library::combined_style_library(
                        self.aec_project_explorer_file.as_ref(),
                    ),
                );
                self.aec_style_picker_filter.clear();
                // Pre-select/highlight the style currently set on the
                // in-progress wall (if any) instead of always starting with
                // nothing highlighted.
                self.aec_style_picker_selection = self.tabs[self.active_tab]
                    .active_cmd
                    .as_ref()
                    .and_then(|c| c.live_property_id("wall_style"));
                self.active_modal = Some(crate::app::ModalKind::AecStylePicker {
                    target: crate::app::StylePickerTarget::ActiveCommand,
                });
                Task::none()
            }
            Message::ActiveCommandLivePropertyChanged(field, value) => {
                let i = self.active_tab;
                let result = self.tabs[i]
                    .active_cmd
                    .as_mut()
                    .map(|c| c.apply_live_property(field, value));
                if let Some(r) = result {
                    let task = self.apply_cmd_result(r);
                    self.refresh_active_cmd_preview(i);
                    // Rebuild the Properties panel so the just-applied value
                    // (e.g. a new height, or the resolved style name) is
                    // reflected immediately instead of appearing to have no
                    // effect.
                    self.refresh_properties();
                    return task;
                }
                Task::none()
            }
            Message::ActiveCommandLiveTextInput(field, value) => {
                self.tabs[self.active_tab]
                    .properties
                    .edit_buf
                    .insert(crate::ui::properties::FieldKey::Geom(field), value);
                Task::none()
            }
            Message::ActiveCommandLiveTextCommit(field) => {
                let i = self.active_tab;
                let Some(raw) = self.tabs[i]
                    .properties
                    .edit_buf
                    .remove(&crate::ui::properties::FieldKey::Geom(field))
                else {
                    return Task::none();
                };
                let Ok(parsed) = raw.trim().parse::<f64>() else {
                    // Invalid text (e.g. left mid-edit) — drop the buffer so
                    // the field reverts to showing the command's real value.
                    return Task::none();
                };
                let result = self.tabs[i].active_cmd.as_mut().map(|c| {
                    c.apply_live_property(
                        field,
                        crate::command::LiveFieldValue::Number(parsed),
                    )
                });
                if let Some(r) = result {
                    let task = self.apply_cmd_result(r);
                    self.refresh_active_cmd_preview(i);
                    self.refresh_properties();
                    return task;
                }
                Task::none()
            }
            Message::AecStylePickerFilterChanged(v) => {
                self.aec_style_picker_filter = v;
                Task::none()
            }
            Message::AecStylePickerSelect(id) => {
                self.aec_style_picker_selection = Some(id);
                Task::none()
            }
            Message::AecStylePickerCancel => {
                // Targets opened from within the Wall Style Manager return
                // there on cancel, instead of closing the modal entirely.
                if let Some(crate::app::ModalKind::AecStylePicker { target }) = self.active_modal
                {
                    if matches!(
                        target,
                        crate::app::StylePickerTarget::WallStyleParent
                            | crate::app::StylePickerTarget::LayerMaterial(_)
                            | crate::app::StylePickerTarget::LayerOverride(_)
                    ) {
                        self.active_modal = Some(crate::app::ModalKind::AecWallStyleManager);
                        return Task::none();
                    }
                }
                self.close_active_modal();
                Task::none()
            }
            Message::AecStyleManagerProfileSelect(name) => {
                use crate::modules::aec::engine::display_component::layer_filter_to_ui_state;
                use crate::modules::aec::engine::display_component::WallComponentSlot;
                let rules = self
                    .aec_style_manager_wall_style_editing_id
                    .as_ref()
                    .and_then(|id| {
                        self.aec_style_library
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
                        self.aec_style_manager_profile_contour_explicit = c_explicit;
                        self.aec_style_manager_profile_contour_selection = c_sel;
                        self.aec_style_manager_profile_solid_explicit = s_explicit;
                        self.aec_style_manager_profile_solid_selection = s_sel;
                        let hatch = rules.style_override.get(WallComponentSlot::ContourHatch2D.key());
                        self.aec_style_manager_profile_hatch_angle = hatch
                            .and_then(|h| h.hatch_angle)
                            .map(|a| format!("{a}"))
                            .unwrap_or_default();
                        self.aec_style_manager_profile_hatch_relative = hatch
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
                        self.aec_style_manager_profile_slot_visibility = visibility;
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
                        self.aec_style_manager_profile_slot_overrides = overrides;
                        self.aec_style_manager_profile_editing_slot = None;
                        self.clear_aec_profile_slot_style_editor_buffers();
                    }
                    None => {
                        self.aec_style_manager_profile_contour_explicit = false;
                        self.aec_style_manager_profile_contour_selection = Vec::new();
                        self.aec_style_manager_profile_solid_explicit = false;
                        self.aec_style_manager_profile_solid_selection = Vec::new();
                        self.aec_style_manager_profile_hatch_angle = String::new();
                        self.aec_style_manager_profile_hatch_relative = false;
                        self.aec_style_manager_profile_slot_visibility =
                            std::collections::HashMap::new();
                        self.aec_style_manager_profile_slot_overrides =
                            std::collections::HashMap::new();
                        self.aec_style_manager_profile_editing_slot = None;
                        self.clear_aec_profile_slot_style_editor_buffers();
                    }
                }
                self.aec_style_manager_profile_selected = Some(name);
                Task::none()
            }
            Message::AecStyleManagerProfileContourModeToggle(is_explicit) => {
                self.aec_style_manager_profile_contour_explicit = is_explicit;
                Task::none()
            }
            Message::AecStyleManagerProfileSolidModeToggle(is_explicit) => {
                self.aec_style_manager_profile_solid_explicit = is_explicit;
                Task::none()
            }
            Message::AecStyleManagerProfileContourLayerToggle(layer) => {
                if let Some(pos) = self
                    .aec_style_manager_profile_contour_selection
                    .iter()
                    .position(|l| *l == layer)
                {
                    self.aec_style_manager_profile_contour_selection.remove(pos);
                } else {
                    self.aec_style_manager_profile_contour_selection.push(layer);
                }
                Task::none()
            }
            Message::AecStyleManagerProfileSolidLayerToggle(layer) => {
                if let Some(pos) = self
                    .aec_style_manager_profile_solid_selection
                    .iter()
                    .position(|l| *l == layer)
                {
                    self.aec_style_manager_profile_solid_selection.remove(pos);
                } else {
                    self.aec_style_manager_profile_solid_selection.push(layer);
                }
                Task::none()
            }
            Message::AecStyleManagerProfileHatchRelativeToggle(value) => {
                self.aec_style_manager_profile_hatch_relative = value;
                Task::none()
            }
            Message::AecStyleManagerProfileHatchAngleChanged(value) => {
                self.aec_style_manager_profile_hatch_angle = value;
                Task::none()
            }
            Message::AecStyleManagerProfileSlotVisibilityToggle(slot, visible) => {
                self.aec_style_manager_profile_slot_visibility
                    .insert(slot, visible);
                Task::none()
            }
            Message::AecStyleManagerProfileSlotStyleOpen(slot) => {
                use crate::ui::window::aec_ui_util::acad_color_to_editor_string;
                let style = self
                    .aec_style_manager_profile_slot_overrides
                    .get(&slot)
                    .cloned()
                    .unwrap_or_default();
                self.aec_style_manager_profile_editing_slot = Some(slot);
                self.aec_style_manager_profile_slot_style_line_type =
                    style.line_type.clone().unwrap_or_default();
                self.aec_style_manager_profile_slot_style_line_color = style
                    .line_color
                    .map(acad_color_to_editor_string)
                    .unwrap_or_default();
                self.aec_style_manager_profile_slot_style_hatch_pattern =
                    style.hatch_pattern.clone().unwrap_or_default();
                self.aec_style_manager_profile_slot_style_hatch_color = style
                    .hatch_color
                    .map(acad_color_to_editor_string)
                    .unwrap_or_default();
                self.aec_style_manager_profile_slot_style_fill_color = style
                    .fill_color
                    .map(acad_color_to_editor_string)
                    .unwrap_or_default();
                self.aec_style_manager_profile_slot_style_line_color_picker_open = false;
                self.aec_style_manager_profile_slot_style_hatch_color_picker_open = false;
                self.aec_style_manager_profile_slot_style_fill_color_picker_open = false;
                Task::none()
            }
            Message::AecStyleManagerProfileSlotStyleLineTypeChanged(v) => {
                self.aec_style_manager_profile_slot_style_line_type = v;
                Task::none()
            }
            Message::AecStyleManagerProfileSlotStyleLineColorChanged(v) => {
                self.aec_style_manager_profile_slot_style_line_color = v;
                Task::none()
            }
            Message::AecStyleManagerProfileSlotStyleLineColorPickerToggle => {
                self.aec_style_manager_profile_slot_style_line_color_picker_open =
                    !self.aec_style_manager_profile_slot_style_line_color_picker_open;
                Task::none()
            }
            Message::AecStyleManagerProfileSlotStyleHatchPatternChanged(v) => {
                self.aec_style_manager_profile_slot_style_hatch_pattern = v;
                self.aec_style_manager_profile_slot_style_hatch_picker_open = false;
                Task::none()
            }
            Message::AecStyleManagerProfileSlotStyleHatchPickerToggle => {
                self.aec_style_manager_profile_slot_style_hatch_picker_open =
                    !self.aec_style_manager_profile_slot_style_hatch_picker_open;
                Task::none()
            }
            Message::AecStyleManagerProfileSlotStyleHatchColorChanged(v) => {
                self.aec_style_manager_profile_slot_style_hatch_color = v;
                Task::none()
            }
            Message::AecStyleManagerProfileSlotStyleHatchColorPickerToggle => {
                self.aec_style_manager_profile_slot_style_hatch_color_picker_open =
                    !self.aec_style_manager_profile_slot_style_hatch_color_picker_open;
                Task::none()
            }
            Message::AecStyleManagerProfileSlotStyleFillColorChanged(v) => {
                self.aec_style_manager_profile_slot_style_fill_color = v;
                Task::none()
            }
            Message::AecStyleManagerProfileSlotStyleFillColorPickerToggle => {
                self.aec_style_manager_profile_slot_style_fill_color_picker_open =
                    !self.aec_style_manager_profile_slot_style_fill_color_picker_open;
                Task::none()
            }
            Message::AecStyleManagerProfileSlotStyleApply => {
                use crate::modules::aec::engine::display_component::component_style_override_from_editor_fields;
                let Some(slot) = self.aec_style_manager_profile_editing_slot else {
                    return Task::none();
                };
                let style = component_style_override_from_editor_fields(
                    &self.aec_style_manager_profile_slot_style_line_type,
                    &self.aec_style_manager_profile_slot_style_line_color,
                    &self.aec_style_manager_profile_slot_style_hatch_pattern,
                    &self.aec_style_manager_profile_slot_style_hatch_color,
                    &self.aec_style_manager_profile_slot_style_fill_color,
                );
                if style == Default::default() {
                    self.aec_style_manager_profile_slot_overrides.remove(&slot);
                } else {
                    self.aec_style_manager_profile_slot_overrides
                        .insert(slot, style);
                }
                self.aec_style_manager_profile_editing_slot = None;
                self.clear_aec_profile_slot_style_editor_buffers();
                Task::none()
            }
            Message::AecStyleManagerProfileSlotStyleClear => {
                if let Some(slot) = self.aec_style_manager_profile_editing_slot.take() {
                    self.aec_style_manager_profile_slot_overrides.remove(&slot);
                }
                self.clear_aec_profile_slot_style_editor_buffers();
                Task::none()
            }
            Message::AecStyleManagerProfileSlotStyleClose => {
                self.aec_style_manager_profile_editing_slot = None;
                self.clear_aec_profile_slot_style_editor_buffers();
                Task::none()
            }
            Message::AecWallStyleManagerDisplayProfilesOpen => {
                // Keep the wall style manager geometry so closing the child
                // modal can restore it (Plot → Plotstyle pattern).
                self.aec_wall_style_manager_parent_geometry =
                    Some((self.modal_offset, self.modal_resize));
                self.active_modal = Some(crate::app::ModalKind::AecWallStyleDisplayProfiles);
                self.reset_modal_geometry();
                Task::none()
            }
            Message::AecWallStyleManagerDisplayProfilesClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::AecStyleManagerProfileSave => {
                use crate::modules::aec::engine::display_component::{
                    layer_filter_from_selection, ComponentStyleOverride, WallComponentSlot,
                };
                let Some(config_name) = self.aec_style_manager_profile_selected.clone() else {
                    return Task::none();
                };
                let Some(id) = self.aec_style_manager_wall_style_editing_id.clone() else {
                    return Task::none();
                };
                let Some(mut wall_style) = self.aec_style_library.as_ref().and_then(|lib| {
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
                        self.aec_style_manager_profile_contour_explicit,
                        &self.aec_style_manager_profile_contour_selection,
                    ),
                );
                rules.layer_filter.insert(
                    WallComponentSlot::Solid3D.key().to_string(),
                    layer_filter_from_selection(
                        self.aec_style_manager_profile_solid_explicit,
                        &self.aec_style_manager_profile_solid_selection,
                    ),
                );
                let hatch_angle = self
                    .aec_style_manager_profile_hatch_angle
                    .trim()
                    .parse::<f64>()
                    .ok();
                if hatch_angle.is_some() {
                    rules.style_override.insert(
                        WallComponentSlot::ContourHatch2D.key().to_string(),
                        ComponentStyleOverride {
                            hatch_angle,
                            hatch_angle_relative: Some(self.aec_style_manager_profile_hatch_relative),
                            ..Default::default()
                        },
                    );
                } else {
                    rules.style_override.remove(WallComponentSlot::ContourHatch2D.key());
                }
                for (slot, visible) in &self.aec_style_manager_profile_slot_visibility {
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
                for (slot, style) in &self.aec_style_manager_profile_slot_overrides {
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

                let source = crate::modules::aec::engine::library::wall_style_library_source(
                    self.aec_project_explorer_file.as_ref(),
                    &id,
                );
                let cow_from_standard =
                    source == Some(crate::modules::aec::engine::library::LibrarySource::Standard);

                if self.aec_project_explorer_file.is_some() {
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
                    let lib = self
                        .aec_style_library
                        .get_or_insert_with(crate::modules::aec::engine::library::StyleLibrary::empty);
                    lib.upsert_wall_style(wall_style);
                    let lib_snapshot = lib.clone();
                    if let Err(e) = self.aec_save_style_library_preferring_project(&lib_snapshot) {
                        self.command_line.push_error(
                            crate::tf!("AEC Style Manager: failed to save library: {e}").as_ref(),
                        );
                    } else {
                        self.command_line
                            .push_info(crate::t!("AEC Style Manager: display profile saved.").as_ref());
                    }
                }
                Task::none()
            }
            Message::AecStyleManagerProfileRemove => {
                let Some(config_name) = self.aec_style_manager_profile_selected.clone() else {
                    return Task::none();
                };
                let Some(id) = self.aec_style_manager_wall_style_editing_id.clone() else {
                    return Task::none();
                };
                let Some(mut wall_style) = self.aec_style_library.as_ref().and_then(|lib| {
                    lib.wall_styles.iter().find(|w| w.style.id == id).cloned()
                }) else {
                    return Task::none();
                };
                wall_style.display_profiles.remove(&config_name);

                self.aec_style_manager_profile_contour_explicit = false;
                self.aec_style_manager_profile_contour_selection = Vec::new();
                self.aec_style_manager_profile_solid_explicit = false;
                self.aec_style_manager_profile_solid_selection = Vec::new();
                self.aec_style_manager_profile_hatch_angle = String::new();
                self.aec_style_manager_profile_hatch_relative = false;
                self.aec_style_manager_profile_slot_visibility = std::collections::HashMap::new();
                self.aec_style_manager_profile_slot_overrides = std::collections::HashMap::new();
                self.aec_style_manager_profile_editing_slot = None;
                self.clear_aec_profile_slot_style_editor_buffers();

                if self.aec_project_explorer_file.is_some() {
                    if let Err(e) = self.aec_upsert_wall_style_into_project(wall_style) {
                        self.command_line.push_error(
                            crate::tf!("AEC Style Manager: failed to save library: {e}").as_ref(),
                        );
                    } else {
                        self.command_line
                            .push_info(crate::t!("AEC Style Manager: display profile removed.").as_ref());
                    }
                } else {
                    let lib = self
                        .aec_style_library
                        .get_or_insert_with(crate::modules::aec::engine::library::StyleLibrary::empty);
                    lib.upsert_wall_style(wall_style);
                    let lib_snapshot = lib.clone();
                    if let Err(e) = self.aec_save_style_library_preferring_project(&lib_snapshot) {
                        self.command_line.push_error(
                            crate::tf!("AEC Style Manager: failed to save library: {e}").as_ref(),
                        );
                    } else {
                        self.command_line
                            .push_info(crate::t!("AEC Style Manager: display profile removed.").as_ref());
                    }
                }
                Task::none()
            }
            Message::AecStylePickerConfirm => {
                if let (Some(crate::app::ModalKind::AecStylePicker { target }), Some(selection)) =
                    (self.active_modal, self.aec_style_picker_selection.clone())
                {
                    match target {
                        crate::app::StylePickerTarget::WallStyleParent => {
                            let id = if selection.is_empty() {
                                None
                            } else {
                                Some(selection)
                            };
                            self.active_modal = Some(crate::app::ModalKind::AecWallStyleManager);
                            return self.update(Message::AecStyleManagerWallStyleParentChanged(id));
                        }
                        crate::app::StylePickerTarget::LayerMaterial(index) => {
                            self.active_modal = Some(crate::app::ModalKind::AecWallStyleManager);
                            return self.update(Message::AecStyleManagerWallStyleLayerMaterialChanged(
                                index, selection,
                            ));
                        }
                        crate::app::StylePickerTarget::LayerOverride(index) => {
                            self.active_modal = Some(crate::app::ModalKind::AecWallStyleManager);
                            return self.update(Message::AecStyleManagerWallStyleLayerOverrideChanged(
                                index, selection,
                            ));
                        }
                        crate::app::StylePickerTarget::WallPropertiesStyle => {
                            self.active_modal = None;
                            let Some(lib) = &self.aec_style_library else {
                                return Task::none();
                            };

                            let wall_layers = crate::modules::aec::commands::resolve_wall_style_layers_ids(
                                lib,
                                &selection,
                                None,
                            )
                            .unwrap_or_default();

                            let i = self.active_tab;
                            self.push_undo_snapshot(i, "CHPROP");

                            let handles = self.aec_style_picker_wall_handles.clone();
                            for handle in handles {
                                let handle = crate::modules::aec::commands::resolve_wall_package(
                                    &self.tabs[i].scene,
                                    handle,
                                );
                                let mut record = acadrust::xdata::ExtendedDataRecord::new(
                                    crate::modules::aec::commands::AEC_APPID,
                                );

                                if let Some(entity) = self.tabs[i].scene.document.get_entity(handle) {
                                    if let Some(mut wall) =
                                        crate::modules::aec::commands::wall_from_entity(entity)
                                    {
                                        wall.style_id = selection.clone();
                                        wall.layers = wall_layers.clone();

                                        record.values = crate::modules::aec::commands::wall_record(
                                            &wall.style_id,
                                            wall.height,
                                            wall.storey_id,
                                            &wall.layers,
                                            &wall.derived_handles,
                                            wall.justification,
                                            wall.phase,
                                            wall.hatch_override.as_ref(),
                                        );
                                    }
                                }

                                if !record.values.is_empty() {
                                    let app_handle = self.tabs[i]
                                        .scene
                                        .document
                                        .app_ids
                                        .get(crate::modules::aec::commands::AEC_APPID)
                                        .map(|a| a.handle.value());

                                    if let Some(entity) =
                                        self.tabs[i].scene.document.get_entity_mut(handle)
                                    {
                                        let xd = &mut entity.common_mut().extended_data;
                                        let kept: Vec<_> = xd
                                            .records()
                                            .iter()
                                            .filter(|r| {
                                                r.application_name
                                                    != crate::modules::aec::commands::AEC_APPID
                                            })
                                            .cloned()
                                            .collect();
                                        xd.clear();
                                        for r in kept {
                                            xd.add_record(r);
                                        }
                                        xd.add_record(record);
                                        if let Some(ah) = app_handle {
                                            xd.raw_dwg_eed.retain(|(a, _)| *a != ah);
                                        }

                                        let _ = self.regenerate_wall_respecting_active_display_config(i, handle);
                                        self.tabs[i].dirty = true;
                                    }
                                }
                            }
                            self.refresh_properties();
                        }
                        crate::app::StylePickerTarget::ActiveCommand => {
                            let i = self.active_tab;
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
            Message::AecStyleManagerWallStyleSave => {
                self.aec_style_manager_wall_style_save_internal();
                Task::none()
            }
            Message::AecStyleManagerWallStyleSaveAndApply => {
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
                            crate::modules::aec::commands::wall_from_entity(entity)
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
                                crate::modules::aec::commands::wall_from_entity(entity)
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
                                    crate::modules::aec::commands::resolve_wall_style_layers_ids(
                                        &lib,
                                        &wall.style_id,
                                        bb,
                                    )
                                {
                                    crate::modules::aec::commands::write_wall_layers(
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
            Message::AecStyleManagerWallStyleDelete => {
                if let Some(id) = self.aec_style_manager_selected_wall_style.clone() {
                    let lib_snapshot = if let Some(lib) = self.aec_style_library.as_mut() {
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
                self.aec_style_manager_selected_wall_style = None;
                self.aec_style_manager_wall_style_editing_id = None;
                self.aec_style_manager_wall_style_form_open = false;
                Task::none()
            }
            Message::AecStyleManagerMaterialNew => {
                self.aec_style_manager_selected_material = None;
                self.aec_style_manager_selected_wall_style = None;
                self.aec_style_manager_material_editing_id = None;
                self.aec_style_manager_material_name.clear();
                self.aec_style_manager_material_hatch.clear();
                self.aec_style_manager_material_color = "#FFFFFF".to_string();
                self.aec_style_manager_material_line_type = "Continuous".to_string();
                self.aec_style_manager_material_category.clear();
                self.aec_style_manager_material_hatch_color = 0xFFFFFF;
                self.aec_style_manager_material_hatch_scale = "1.0".to_string();
                self.aec_style_manager_material_render_ref.clear();
                self.aec_style_manager_material_hatch_angle = "0.0".to_string();
                self.aec_style_manager_material_hatch_angle_relative = true;
                self.aec_style_manager_material_hatch_color_picker_open = false;
                self.aec_style_manager_material_form_open = true;
                Task::none()
            }
            Message::AecStyleManagerMaterialNameChanged(value) => {
                self.aec_style_manager_material_name = value;
                Task::none()
            }
            Message::AecStyleManagerMaterialHatchChanged(value) => {
                self.aec_style_manager_material_hatch = value;
                Task::none()
            }
            Message::AecStyleManagerMaterialHatchPickerToggle => {
                self.aec_style_manager_material_hatch_picker_open =
                    !self.aec_style_manager_material_hatch_picker_open;
                Task::none()
            }
            Message::AecStyleManagerMaterialHatchSelected(name) => {
                self.aec_style_manager_material_hatch = name;
                self.aec_style_manager_material_hatch_picker_open = false;
                Task::none()
            }
            Message::AecStyleManagerMaterialColorChanged(value) => {
                self.aec_style_manager_material_color = value;
                Task::none()
            }
            Message::AecStyleManagerMaterialLineTypeChanged(value) => {
                self.aec_style_manager_material_line_type = value;
                Task::none()
            }
            Message::AecStyleManagerMaterialColorPickerToggle => {
                self.aec_style_manager_material_color_picker_open =
                    !self.aec_style_manager_material_color_picker_open;
                Task::none()
            }
            Message::AecStyleManagerMaterialColorPicked(color) => {
                self.aec_style_manager_material_color_picker_open = false;
                let [r, g, b, _] = match color {
                    acadrust::types::Color::Rgb { r, g, b } => [r, g, b, 255u8],
                    acadrust::types::Color::Index(i) => {
                        let (r, g, b) = acadrust::types::aci_table::aci_to_rgb(i)
                            .unwrap_or((255, 255, 255));
                        [r, g, b, 255]
                    }
                    _ => [255, 255, 255, 255],
                };
                self.aec_style_manager_material_color = format!("#{r:02X}{g:02X}{b:02X}");
                Task::none()
            }
            Message::AecStyleManagerMaterialCategoryChanged(value) => {
                self.aec_style_manager_material_category = value;
                Task::none()
            }
            Message::AecStyleManagerMaterialHatchColorChanged(value) => {
                self.aec_style_manager_material_hatch_color = value;
                self.aec_style_manager_material_hatch_color_picker_open = false;
                Task::none()
            }
            Message::AecStyleManagerMaterialHatchColorPickerToggle => {
                self.aec_style_manager_material_hatch_color_picker_open =
                    !self.aec_style_manager_material_hatch_color_picker_open;
                Task::none()
            }
            Message::AecStyleManagerMaterialHatchScaleChanged(value) => {
                self.aec_style_manager_material_hatch_scale = value;
                Task::none()
            }
            Message::AecStyleManagerMaterialRenderRefChanged(value) => {
                self.aec_style_manager_material_render_ref = value;
                Task::none()
            }
            Message::AecStyleManagerMaterialHatchAngleChanged(value) => {
                self.aec_style_manager_material_hatch_angle = value;
                Task::none()
            }
            Message::AecStyleManagerMaterialHatchAngleRelativeToggle => {
                self.aec_style_manager_material_hatch_angle_relative =
                    !self.aec_style_manager_material_hatch_angle_relative;
                Task::none()
            }
            Message::AecStyleManagerMaterialDuplicate => {
                let source_id = self
                    .aec_style_manager_material_editing_id
                    .clone()
                    .or_else(|| self.aec_style_manager_selected_material.clone());
                let Some(source_id) = source_id else {
                    return Task::none();
                };
                let Some(material) = self
                    .aec_style_library
                    .as_ref()
                    .and_then(|lib| lib.materials.iter().find(|m| m.id == source_id))
                    .cloned()
                else {
                    return Task::none();
                };
                let existing_names: Vec<String> = self
                    .aec_style_library
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
                self.aec_style_manager_selected_material = None;
                self.aec_style_manager_selected_wall_style = None;
                self.aec_style_manager_material_editing_id = None;
                self.aec_style_manager_material_name = name;
                self.aec_style_manager_material_hatch = material.hatch_pattern;
                self.aec_style_manager_material_color =
                    format!("#{:06X}", material.line_color);
                self.aec_style_manager_material_line_type = material.line_type;
                self.aec_style_manager_material_category =
                    material.category.unwrap_or_default();
                                self.aec_style_manager_material_hatch_color = material
                    .hatch_color
                    .and_then(|c| c.rgb())
                    .map(|(r, g, b)| ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
                    .unwrap_or(material.line_color);
                self.aec_style_manager_material_hatch_scale =
                    format!("{}", material.hatch_scale);
                self.aec_style_manager_material_render_ref =
                    material.render_material_ref.unwrap_or_default();
                self.aec_style_manager_material_hatch_angle =
                    format!("{}", material.hatch_angle);
                self.aec_style_manager_material_hatch_angle_relative =
                    material.hatch_angle_relative;
                self.aec_style_manager_material_color_picker_open = false;
                self.aec_style_manager_material_hatch_color_picker_open = false;
                self.aec_style_manager_material_hatch_picker_open = false;
                self.aec_style_manager_material_form_open = true;
                Task::none()
            }
            Message::AecStyleManagerMaterialSave => {
                let name = self.aec_style_manager_material_name.trim().to_string();
                if name.is_empty() {
                    self.command_line.push_error(
                        crate::t!("AEC Style Manager: material name cannot be empty.").as_ref(),
                    );
                    return Task::none();
                }
                let hatch = if self.aec_style_manager_material_hatch.trim().is_empty() {
                    "SOLID".to_string()
                } else {
                    self.aec_style_manager_material_hatch.trim().to_string()
                };
                let color_hex = self
                    .aec_style_manager_material_color
                    .trim()
                    .trim_start_matches('#');
                let color = u32::from_str_radix(color_hex, 16).unwrap_or(0);
                let line_type = if self.aec_style_manager_material_line_type.trim().is_empty() {
                    "Continuous".to_string()
                } else {
                    self.aec_style_manager_material_line_type.trim().to_string()
                };
                let category = {
                    let c = self.aec_style_manager_material_category.trim();
                    if c.is_empty() {
                        None
                    } else {
                        Some(c.to_string())
                    }
                };
                let mut hatch_scale = self
                    .aec_style_manager_material_hatch_scale
                    .trim()
                    .parse::<f64>()
                    .unwrap_or(1.0);
                if hatch_scale <= 0.0 {
                    hatch_scale = 0.01;
                }
                let render_material_ref = {
                    let r = self.aec_style_manager_material_render_ref.trim();
                    if r.is_empty() {
                        None
                    } else {
                        Some(r.to_string())
                    }
                };
                let hatch_angle = self
                    .aec_style_manager_material_hatch_angle
                    .trim()
                    .parse::<f64>()
                    .unwrap_or(0.0);
                let hatch_angle_relative = self.aec_style_manager_material_hatch_angle_relative;
                // Same reasoning as for wall styles above: only genuinely
                // new materials get a fresh, globally unique id.
                let id = self
                    .aec_style_manager_material_editing_id
                    .clone()
                    .unwrap_or_else(|| crate::modules::aec::commands::unique_id("mat", &name));

                let material = crate::modules::aec::engine::material::Material {
                    id: id.clone(),
                    name,
                    hatch_pattern: hatch,
                    line_color: color,
                    line_type,
                    render_material_ref,
                    category,
                    hatch_color: Some(acadrust::types::Color::Rgb {
                        r: ((self.aec_style_manager_material_hatch_color >> 16) & 0xFF) as u8,
                        g: ((self.aec_style_manager_material_hatch_color >> 8) & 0xFF) as u8,
                        b: (self.aec_style_manager_material_hatch_color & 0xFF) as u8,
                    }),
                    hatch_scale,
                    hatch_angle,
                    hatch_angle_relative,
                };

                // Copy-on-write: editing a Standard entry writes into the
                // project library and leaves the Standard library unchanged.
                let source = crate::modules::aec::engine::library::material_library_source(
                    self.aec_project_explorer_file.as_ref(),
                    &id,
                );
                let cow_from_standard = source
                    == Some(crate::modules::aec::engine::library::LibrarySource::Standard);

                if self.aec_project_explorer_file.is_some() {
                    match self.aec_upsert_material_into_project(material) {
                        Ok(()) => {
                            if cow_from_standard {
                                self.command_line.push_info(
                                    crate::t!(
                                        "AEC Style Manager: Standard material was copied into the project and saved."
                                    )
                                    .as_ref(),
                                );
                            } else {
                                self.command_line.push_info(
                                    crate::t!("AEC Style Manager: material saved.").as_ref(),
                                );
                            }
                        }
                        Err(e) => self.command_line.push_error(
                            crate::tf!("AEC Style Manager: failed to save library: {e}").as_ref(),
                        ),
                    }
                } else {
                    let lib = self.aec_style_library.get_or_insert_with(
                        crate::modules::aec::engine::library::StyleLibrary::empty,
                    );
                    lib.upsert_material(material);
                    let lib_snapshot = lib.clone();
                    match self.aec_save_style_library_preferring_project(&lib_snapshot) {
                        Ok(()) => self
                            .command_line
                            .push_info(crate::t!("AEC Style Manager: material saved.").as_ref()),
                        Err(e) => self.command_line.push_error(
                            crate::tf!("AEC Style Manager: failed to save library: {e}").as_ref(),
                        ),
                    }
                }
                self.aec_style_manager_selected_material = Some(id.clone());
                self.aec_style_manager_material_editing_id = Some(id);
                Task::none()
            }
            Message::AecStyleManagerMaterialDelete => {
                if let Some(id) = self.aec_style_manager_selected_material.clone() {
                    let lib_snapshot = if let Some(lib) = self.aec_style_library.as_mut() {
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
                self.aec_style_manager_selected_material = None;
                self.aec_style_manager_material_editing_id = None;
                self.aec_style_manager_material_form_open = false;
                Task::none()
            }
            Message::AecStyleManagerCopyMaterialToProject => self.aec_handle_copy_material(true),
            Message::AecStyleManagerCopyMaterialToGlobal => self.aec_handle_copy_material(false),
            Message::AecStyleManagerCopyWallStyleToProject => self.aec_handle_copy_wall_style(true),
            Message::AecStyleManagerCopyWallStyleToGlobal => self.aec_handle_copy_wall_style(false),
            Message::AecStyleManagerCopyConflictConfirm(confirmed) => {
                self.aec_style_manager_copy_conflict_open = false;
                let return_modal = match &self.aec_style_manager_pending_copy {
                    Some(AecPendingCopy::Material { .. }) => {
                        crate::app::ModalKind::AecMaterialManager
                    }
                    Some(AecPendingCopy::WallStyle { .. }) => {
                        crate::app::ModalKind::AecWallStyleManager
                    }
                    None => crate::app::ModalKind::AecMaterialManager,
                };
                if confirmed {
                    if let Some(pending) = self.aec_style_manager_pending_copy.take() {
                        self.aec_execute_copy(pending);
                    }
                } else {
                    self.aec_style_manager_pending_copy = None;
                }
                self.active_modal = Some(return_modal);
                Task::none()
            }
            // ── Layer Translator (#624) ──────────────────────────────────
            Message::LayerTranslatorLoad => Task::perform(
                crate::io::pick_layer_standard_path(),
                |path| match path {
                    Some(path) => Message::LayerTranslatorLoaded(path),
                    None => Message::Noop,
                },
            ),
            Message::LayerTranslatorLoaded(path) => {
                use crate::modules::draw::layers::laytrans;
                match laytrans::load_targets(&path) {
                    Ok(targets) => {
                        let state = self.layer_translator.get_or_insert_with(Default::default);
                        state.source_file = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        state.targets = targets;
                        // A target set that no longer contains a mapped name
                        // would translate onto nothing.
                        let names: Vec<String> =
                            state.targets.iter().map(|t| t.name.clone()).collect();
                        state
                            .mappings
                            .retain(|m| names.iter().any(|n| n.eq_ignore_ascii_case(&m.to)));
                        state.selected_to = None;
                    }
                    Err(why) => self
                        .command_line
                        .push_error(crate::tf!("LAYTRANS: {why}.").as_ref()),
                }
                Task::none()
            }
            Message::LayerTranslatorSelectFrom(name) => {
                if let Some(state) = self.layer_translator.as_mut() {
                    state.selected_from = Some(name);
                }
                Task::none()
            }
            Message::LayerTranslatorSelectTo(name) => {
                if let Some(state) = self.layer_translator.as_mut() {
                    state.selected_to = Some(name);
                }
                Task::none()
            }
            Message::LayerTranslatorMap => {
                use crate::modules::draw::layers::laytrans::Mapping;
                if let Some(state) = self.layer_translator.as_mut() {
                    if let (Some(from), Some(to)) =
                        (state.selected_from.take(), state.selected_to.clone())
                    {
                        state.mappings.retain(|m| !m.from.eq_ignore_ascii_case(&from));
                        state.mappings.push(Mapping { from, to });
                    }
                }
                Task::none()
            }
            Message::LayerTranslatorMapSame => {
                use crate::modules::draw::layers::laytrans;
                let i = self.active_tab;
                let current = self.tabs[i].active_layer.clone();
                let sources = laytrans::source_layers(&self.tabs[i].scene, &current);
                if let Some(state) = self.layer_translator.as_mut() {
                    for mapping in laytrans::map_same(&sources, &state.targets) {
                        if !state
                            .mappings
                            .iter()
                            .any(|m| m.from.eq_ignore_ascii_case(&mapping.from))
                        {
                            state.mappings.push(mapping);
                        }
                    }
                }
                Task::none()
            }
            Message::LayerTranslatorUnmap(from) => {
                if let Some(state) = self.layer_translator.as_mut() {
                    state.mappings.retain(|m| m.from != from);
                }
                Task::none()
            }
            Message::LayerTranslatorForceByLayer(value) => {
                if let Some(state) = self.layer_translator.as_mut() {
                    state.force_bylayer = value;
                }
                Task::none()
            }
            Message::LayerTranslatorWriteLog(value) => {
                if let Some(state) = self.layer_translator.as_mut() {
                    state.write_log = value;
                }
                Task::none()
            }
            Message::LayerTranslatorSaveMappings => Task::perform(
                crate::io::pick_layer_mapping_path(true),
                |path| match path {
                    Some(path) => Message::LayerTranslatorMappingsPath(path, true),
                    None => Message::Noop,
                },
            ),
            Message::LayerTranslatorLoadMappings => Task::perform(
                crate::io::pick_layer_mapping_path(false),
                |path| match path {
                    Some(path) => Message::LayerTranslatorMappingsPath(path, false),
                    None => Message::Noop,
                },
            ),
            Message::LayerTranslatorMappingsPath(path, save) => {
                self.layer_translator_mappings_file(&path, save);
                Task::none()
            }
            Message::LayerTranslatorTranslate => {
                use crate::modules::draw::layers::laytrans;
                let Some(state) = self.layer_translator.take() else {
                    return Task::none();
                };
                self.active_modal = None;
                let i = self.active_tab;
                let current = self.tabs[i].active_layer.clone();
                self.push_undo_snapshot(i, "LAYTRANS");
                let report = laytrans::translate(
                    &mut self.tabs[i].scene,
                    &state.mappings,
                    &state.targets,
                    &current,
                    laytrans::Options {
                        force_bylayer: state.force_bylayer,
                    },
                );
                let write_log = state.write_log;
                let task = self.finish_layer_translation(i, report);
                if write_log {
                    self.write_layer_translation_log(i);
                }
                task
            }
            Message::LayerStateManagerSelect(name) => {
                self.load_layer_state_editor(Some(name));
                Task::none()
            }
            Message::LayerStateManagerNew => {
                self.load_layer_state_editor(None);
                Task::none()
            }
            Message::LayerStateManagerFilter(value) => {
                self.layer_state_filter = value;
                Task::none()
            }
            Message::LayerStateManagerName(value) => {
                self.layer_state_name_buf = value;
                Task::none()
            }
            Message::LayerStateManagerDescription(value) => {
                self.layer_state_description_buf = value;
                Task::none()
            }
            Message::LayerStateManagerSave => {
                let i = self.active_tab;
                let name = self.layer_state_name_buf.trim().to_string();
                if name.is_empty() {
                    self.command_line
                        .push_error(crate::t!("Layer state name cannot be empty.").as_ref());
                    return Task::none();
                }
                let old_name = self.layer_state_selected.clone();
                let duplicate = self.tabs[i]
                    .scene
                    .document
                    .layer_states()
                    .into_iter()
                    .any(|state| {
                        state.name.eq_ignore_ascii_case(&name)
                            && old_name
                                .as_deref()
                                .is_none_or(|old| !state.name.eq_ignore_ascii_case(old))
                    });
                if duplicate {
                    self.command_line
                        .push_error(crate::tf!("Layer state \"{name}\" already exists.").as_ref());
                    return Task::none();
                }

                self.push_undo_snapshot(i, "LAYERSTATE SAVE");
                if let Some(old_name) = old_name.as_deref() {
                    if !old_name.eq_ignore_ascii_case(&name) {
                        self.tabs[i]
                            .scene
                            .document
                            .rename_layer_state(old_name, &name);
                    }
                }
                self.tabs[i].scene.document.capture_layer_state(
                    &name,
                    self.layer_state_description_buf.trim(),
                );
                self.tabs[i].dirty = true;
                self.layer_state_selected = Some(name.clone());
                self.layer_state_name_buf = name.clone();
                self.command_line
                    .push_output(crate::tf!("LAYERSTATE: saved \"{name}\" in the drawing.").as_ref());
                Task::none()
            }
            Message::LayerStateManagerRestore => {
                let i = self.active_tab;
                let Some(name) = self.layer_state_selected.clone() else {
                    return Task::none();
                };
                let layer_names: Vec<String> = self.tabs[i]
                    .scene
                    .document
                    .layers
                    .iter()
                    .map(|layer| layer.name.clone())
                    .collect();
                self.push_undo_snapshot(i, "LAYERSTATE RESTORE");
                let restored = self.tabs[i]
                    .scene
                    .document
                    .restore_layer_state(&name)
                    .unwrap_or(0);
                let active = self.tabs[i]
                    .scene
                    .document
                    .header
                    .current_layer_name
                    .clone();
                self.tabs[i].active_layer = active;
                self.tabs[i]
                    .scene
                    .invalidate_layer_dependencies(&layer_names);
                self.tabs[i].dirty = true;
                self.refresh_layer_panel();
                self.command_line.push_output(crate::tf!(
                    "LAYERSTATE: restored \"{name}\" ({restored} layer(s))."
                ).as_ref());
                Task::none()
            }
            Message::LayerStateManagerDelete => {
                let i = self.active_tab;
                let Some(name) = self.layer_state_selected.clone() else {
                    return Task::none();
                };
                self.push_undo_snapshot(i, "LAYERSTATE DELETE");
                if self.tabs[i].scene.document.delete_layer_state(&name) {
                    self.tabs[i].dirty = true;
                    let mut names: Vec<String> = self.tabs[i]
                        .scene
                        .document
                        .layer_states()
                        .into_iter()
                        .map(|state| state.name)
                        .collect();
                    names.sort_by_key(|name| name.to_lowercase());
                    self.load_layer_state_editor(names.into_iter().next());
                    self.command_line
                        .push_output(crate::tf!("LAYERSTATE: deleted \"{name}\".").as_ref());
                }
                Task::none()
            }
            Message::LayerStateManagerEdit => {
                let i = self.active_tab;
                let Some(name) = self.layer_state_selected.clone() else {
                    return Task::none();
                };
                let Some(state) = self.tabs[i].scene.document.layer_state(&name) else {
                    self.command_line
                        .push_error(crate::tf!("Layer state \"{name}\" was not found.").as_ref());
                    return Task::none();
                };
                self.layer_state_edit_draft = Some(state);
                self.layer_state_edit_filter.clear();
                self.layer_state_edit_color_open = None;
                self.active_modal = Some(super::ModalKind::LayerStateEditor);
                Task::none()
            }
            Message::LayerStateEditorMaskToggle(property) => {
                let flag = match property {
                    super::LayerStateProperty::On => acadrust::LayerStateMask::ON,
                    super::LayerStateProperty::Frozen => acadrust::LayerStateMask::FROZEN,
                    super::LayerStateProperty::Locked => acadrust::LayerStateMask::LOCKED,
                    super::LayerStateProperty::Plot => acadrust::LayerStateMask::PLOT,
                    super::LayerStateProperty::NewViewport => {
                        acadrust::LayerStateMask::NEW_VIEWPORT
                    }
                    super::LayerStateProperty::Color => acadrust::LayerStateMask::COLOR,
                    super::LayerStateProperty::LineType => acadrust::LayerStateMask::LINE_TYPE,
                    super::LayerStateProperty::LineWeight => {
                        acadrust::LayerStateMask::LINE_WEIGHT
                    }
                    super::LayerStateProperty::PlotStyle => acadrust::LayerStateMask::PLOT_STYLE,
                    super::LayerStateProperty::Transparency => {
                        acadrust::LayerStateMask::TRANSPARENCY
                    }
                };
                if let Some(state) = self.layer_state_edit_draft.as_mut() {
                    state.mask =
                        acadrust::LayerStateMask::from_bits(state.mask.bits() ^ flag.bits());
                }
                Task::none()
            }
            Message::LayerStateEditorLayerFlagToggle(index, flag) => {
                let Some(layer) = self
                    .layer_state_edit_draft
                    .as_mut()
                    .and_then(|state| state.layers.get_mut(index))
                else {
                    return Task::none();
                };
                match flag {
                    super::LayerStateLayerFlag::On => layer.off = !layer.off,
                    super::LayerStateLayerFlag::Frozen => layer.frozen = !layer.frozen,
                    super::LayerStateLayerFlag::Locked => layer.locked = !layer.locked,
                    super::LayerStateLayerFlag::Plot => layer.plottable = !layer.plottable,
                    super::LayerStateLayerFlag::NewViewport => {
                        layer.new_viewport_frozen = !layer.new_viewport_frozen
                    }
                }
                Task::none()
            }
            Message::LayerStateEditorLayerColorToggle(index) => {
                self.layer_state_edit_color_open = if self.layer_state_edit_color_open == Some(index)
                {
                    None
                } else {
                    Some(index)
                };
                Task::none()
            }
            Message::LayerStateEditorLayerColor(index, color) => {
                if let Some(layer) = self
                    .layer_state_edit_draft
                    .as_mut()
                    .and_then(|state| state.layers.get_mut(index))
                {
                    layer.color = color;
                }
                self.layer_state_edit_color_open = None;
                Task::none()
            }
            Message::LayerStateEditorLayerLinetype(index, value) => {
                if let Some(layer) = self
                    .layer_state_edit_draft
                    .as_mut()
                    .and_then(|state| state.layers.get_mut(index))
                {
                    layer.line_type = value;
                }
                Task::none()
            }
            Message::LayerStateEditorLayerLineweight(index, value) => {
                if let Some(layer) = self
                    .layer_state_edit_draft
                    .as_mut()
                    .and_then(|state| state.layers.get_mut(index))
                {
                    layer.line_weight = value;
                }
                Task::none()
            }
            Message::LayerStateEditorLayerPlotStyle(index, value) => {
                if let Some(layer) = self
                    .layer_state_edit_draft
                    .as_mut()
                    .and_then(|state| state.layers.get_mut(index))
                {
                    layer.plot_style = value;
                }
                Task::none()
            }
            Message::LayerStateEditorLayerTransparency(index, value) => {
                if let Some(layer) = self
                    .layer_state_edit_draft
                    .as_mut()
                    .and_then(|state| state.layers.get_mut(index))
                {
                    layer.transparency = value;
                }
                Task::none()
            }
            Message::LayerStateEditorName(value) => {
                if let Some(state) = self.layer_state_edit_draft.as_mut() {
                    state.name = value;
                }
                Task::none()
            }
            Message::LayerStateEditorDescription(value) => {
                if let Some(state) = self.layer_state_edit_draft.as_mut() {
                    state.description = value;
                }
                Task::none()
            }
            Message::LayerStateEditorCurrentLayer(value) => {
                if let Some(state) = self.layer_state_edit_draft.as_mut() {
                    state.current_layer = value;
                }
                Task::none()
            }
            Message::LayerStateEditorFilter(value) => {
                self.layer_state_edit_filter = value;
                Task::none()
            }
            Message::LayerStateEditorSave => {
                let i = self.active_tab;
                let Some(draft) = self.layer_state_edit_draft.as_ref() else {
                    return Task::none();
                };
                let name = draft.name.trim().to_string();
                if name.is_empty() {
                    self.command_line
                        .push_error(crate::t!("Layer state name cannot be empty.").as_ref());
                    return Task::none();
                }
                let old_name = self.layer_state_selected.clone();
                let duplicate = self.tabs[i]
                    .scene
                    .document
                    .layer_states()
                    .into_iter()
                    .any(|state| {
                        state.name.eq_ignore_ascii_case(&name)
                            && old_name
                                .as_deref()
                                .is_none_or(|old| !state.name.eq_ignore_ascii_case(old))
                    });
                if duplicate {
                    self.command_line
                        .push_error(crate::tf!("Layer state \"{name}\" already exists.").as_ref());
                    return Task::none();
                }
                let Some(state) = self.layer_state_edit_draft.take() else {
                    return Task::none();
                };
                let mut state = state;
                state.name.clone_from(&name);
                state.description = state.description.trim().to_string();
                let description = state.description.clone();
                self.push_undo_snapshot(i, "LAYERSTATE EDIT");
                if let Some(old_name) = old_name.as_deref() {
                    if !old_name.eq_ignore_ascii_case(&name) {
                        self.tabs[i]
                            .scene
                            .document
                            .rename_layer_state(old_name, &name);
                    }
                }
                self.tabs[i].scene.document.store_layer_state(state);
                self.tabs[i].dirty = true;
                self.layer_state_selected = Some(name.clone());
                self.layer_state_name_buf = name.clone();
                self.layer_state_description_buf = description;
                self.layer_state_edit_filter.clear();
                self.layer_state_edit_color_open = None;
                self.active_modal = Some(super::ModalKind::LayerStateManager);
                self.command_line
                    .push_output(crate::tf!("LAYERSTATE: updated \"{name}\".").as_ref());
                Task::none()
            }
            Message::LayerStateEditorCancel => {
                self.layer_state_edit_draft = None;
                self.layer_state_edit_filter.clear();
                self.layer_state_edit_color_open = None;
                self.active_modal = Some(super::ModalKind::LayerStateManager);
                Task::none()
            }

            Message::WindowCloseRequested(id) => {
                if self.main_window == Some(id) {
                    if self.tabs.iter().any(|t| t.dirty) {
                        self.pending_close = Some(super::PendingClose::Quit);
                        return self.open_unsaved_dialog_window();
                    }
                    return self.exit_app();
                }
                Task::none()
            }

            Message::OsWindowClosed(id) => {
                // Only the main window exists now; all dialogs are in-canvas
                // modals (Plan B). Closing it exits.
                if self.main_window == Some(id) {
                    return self.exit_app();
                }
                Task::none()
            }

            // ── Layer panel messages ───────────────────────────────────────
            Message::LayerToggleVisible(idx) => {
                let i = self.active_tab;
                // New state = toggle of the clicked row, applied to every target
                // (the whole selection when the clicked row is part of it) (#236).
                let on = self.tabs[i].layers.layers.get(idx).map(|l| !l.visible);
                let targets = self.layer_row_action_targets(i, idx);
                if let Some(on) = on {
                    if !targets.is_empty() {
                        let undo = self.begin_layer_undo(i, "LAYER OFF/ON", &targets);
                        for name in &targets {
                            if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                                dl.flags.off = !on;
                            }
                            if let Some(pl) =
                                self.tabs[i].layers.layers.iter_mut().find(|l| &l.name == name)
                            {
                                pl.visible = on;
                            }
                        }
                        self.tabs[i].scene.invalidate_layer_dependencies(&targets);
                        self.tabs[i].dirty = true;
                        self.commit_layer_undo(i, undo);
                        self.command_line.push_output(crate::tf!(
                            "{} layer(s) turned {}",
                            targets.len(),
                            if on { "on" } else { "off" }
                        ).as_ref());
                        self.sync_ribbon_layers();
                    }
                }
                Task::none()
            }

            Message::LayerSort(col) => {
                let i = self.active_tab;
                self.tabs[i].layers.sort_by(col);
                // Keep the ribbon dropdown's order (and its toggle indices) in
                // step with the re-sorted manager table.
                self.sync_ribbon_layers();
                Task::none()
            }

            Message::LayerToggleLock(idx) => {
                let i = self.active_tab;
                let locked = self.tabs[i].layers.layers.get(idx).map(|l| !l.locked);
                let targets = self.layer_row_action_targets(i, idx);
                if let Some(locked) = locked {
                    if !targets.is_empty() {
                        let undo = self.begin_layer_undo(i, "LAYER LOCK/UNLOCK", &targets);
                        for name in &targets {
                            if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                                dl.flags.locked = locked;
                            }
                            if let Some(pl) =
                                self.tabs[i].layers.layers.iter_mut().find(|l| &l.name == name)
                            {
                                pl.locked = locked;
                            }
                        }
                        // Lock state affects editability, not rendered geometry.
                        self.tabs[i].dirty = true;
                        self.commit_layer_undo(i, undo);
                        self.command_line.push_output(crate::tf!(
                            "{} layer(s) {}",
                            targets.len(),
                            if locked { "locked" } else { "unlocked" }
                        ).as_ref());
                        self.sync_ribbon_layers();
                        self.refresh_properties();
                    }
                }
                Task::none()
            }

            Message::LayerToggleFreeze(idx) => {
                let i = self.active_tab;
                let frozen = self.tabs[i].layers.layers.get(idx).map(|l| !l.frozen);
                let targets = self.layer_row_action_targets(i, idx);
                if let Some(frozen) = frozen {
                    if !targets.is_empty() {
                        let undo = self.begin_layer_undo(i, "LAYER FREEZE", &targets);
                        for name in &targets {
                            if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                                if frozen {
                                    dl.freeze();
                                } else {
                                    dl.thaw();
                                }
                            }
                            if let Some(pl) =
                                self.tabs[i].layers.layers.iter_mut().find(|l| &l.name == name)
                            {
                                pl.frozen = frozen;
                            }
                        }
                        self.tabs[i].scene.invalidate_layer_dependencies(&targets);
                        self.tabs[i].dirty = true;
                        self.commit_layer_undo(i, undo);
                        self.command_line.push_output(crate::tf!(
                            "{} layer(s) {}",
                            targets.len(),
                            if frozen { "frozen" } else { "thawed" }
                        ).as_ref());
                        self.sync_ribbon_layers();
                    }
                }
                Task::none()
            }
            Message::LayerTogglePlot(idx) => {
                let i = self.active_tab;

                let plottable = self.tabs[i]
                    .layers
                    .layers
                    .get(idx)
                    .map(|layer| !layer.plottable);

                let targets = self.layer_row_action_targets(i, idx);

                if let Some(plottable) = plottable {
                    if !targets.is_empty() {
                        let undo = self.begin_layer_undo(i, "LAYER PLOT/NOPLOT", &targets);

                        for name in &targets {
                            if let Some(layer) = self.tabs[i].scene.document.layers.get_mut(name) {
                                layer.is_plottable = plottable;
                            }

                            if let Some(layer) = self.tabs[i]
                                .layers
                                .layers
                                .iter_mut()
                                .find(|layer| &layer.name == name)
                            {
                                layer.plottable = plottable;
                            }
                        }

                        self.tabs[i].layers.refresh_sort();

                        self.tabs[i]
                            .scene
                            .invalidate_layer_dependencies(&targets);

                        self.tabs[i].dirty = true;
                        self.commit_layer_undo(i, undo);

                        self.command_line.push_output(
                            crate::tf!(
                                "{} layer(s) set to {}",
                                targets.len(),
                                if plottable { "Plot" } else { "No Plot" }
                            )
                            .as_ref(),
                        );
                    }
                }

                Task::none()
            },

            Message::LayerToggleVpFreeze(layer_idx, vp_col_idx) => {
                self.on_layer_toggle_vp_freeze(layer_idx, vp_col_idx)
            }

            Message::LayerNew => self.on_layer_new(),

            Message::LayerDelete => self.on_layer_delete(),

            Message::LayerDeleteConfirm => self.on_layer_delete_confirm(),

            Message::LayerSetCurrent => self.on_layer_set_current(),

            Message::LayerSelect(idx) => {
                let i = self.active_tab;
                if self.tabs[i].layers.editing.is_some() {
                    return Task::done(Message::LayerRenameCommit);
                }
                let (shift, ctrl) = (self.shift_down, self.ctrl_down);
                let panel = &mut self.tabs[i].layers;
                if ctrl {
                    // Ctrl/Cmd-click toggles this row in the selection.
                    if let Some(pos) = panel.selected_multi.iter().position(|&x| x == idx) {
                        panel.selected_multi.remove(pos);
                    } else {
                        panel.selected_multi.push(idx);
                    }
                } else if shift {
                    // Shift-click selects the range from the anchor to here.
                    let anchor = panel.selected.unwrap_or(idx);
                    let (lo, hi) = (anchor.min(idx), anchor.max(idx));
                    panel.selected_multi = (lo..=hi).collect();
                } else if !panel.selected_multi.contains(&idx) {
                    // Plain click collapses to this row — but NOT when it is
                    // already part of a multi-selection, so clicking a property
                    // combo (linetype / lineweight) on one of the selected rows
                    // keeps the selection and the edit stays bulk (#236).
                    panel.selected_multi = vec![idx];
                }
                panel.selected = Some(idx);
                Task::none()
            }

            Message::LayerRenameStart(idx) => {
                let i = self.active_tab;
                self.tabs[i].layers.selected = Some(idx);
                self.tabs[i].layers.selected_multi = vec![idx];
                if let Some(layer) = self.tabs[i].layers.layers.get(idx) {
                    self.tabs[i].layers.edit_buf = layer.name.clone();
                }
                self.tabs[i].layers.editing = Some(idx);
                Task::none()
            }

            Message::LayerRenameEdit(s) => {
                let i = self.active_tab;
                self.tabs[i].layers.edit_buf = s;
                Task::none()
            }

            Message::LayerRenameCommit => self.on_layer_rename_commit(),

            Message::LayerColorPickerToggle(idx) => {
                let i = self.active_tab;
                let panel = &mut self.tabs[i].layers;
                if panel.color_picker_row == Some(idx) {
                    panel.color_picker_row = None;
                    panel.color_full_palette = false;
                } else {
                    panel.color_picker_row = Some(idx);
                    panel.color_full_palette = false;
                    panel.selected = Some(idx);
                    // Opening the swatch on a row outside the current
                    // multi-selection narrows to just that row; on a selected
                    // row it keeps the multi-selection so the pick applies to all.
                    if !panel.selected_multi.contains(&idx) {
                        panel.selected_multi = vec![idx];
                    }
                }
                Task::none()
            }

            Message::LayerColorMorePalette => {
                let i = self.active_tab;
                self.tabs[i].layers.color_full_palette = !self.tabs[i].layers.color_full_palette;
                Task::none()
            }

            Message::LayerColorSet(color) => {
                let i = self.active_tab;
                // Apply to every selected layer (multi-select), not just one.
                let names = self.selected_layer_names(i);
                if !names.is_empty() {
                    let undo = self.begin_layer_undo(i, "LAYER COLOR", &names);
                    for name in &names {
                        if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                            dl.color = color;
                        }
                    }
                    for pl in self.tabs[i].layers.layers.iter_mut() {
                        if names.contains(&pl.name) {
                            pl.color = color;
                        }
                    }
                    self.tabs[i].dirty = true;
                    self.commit_layer_undo(i, undo);
                    // ByLayer color is baked into the cached wires at
                    // tessellation time, so bump the geometry epoch to
                    // invalidate the wire cache and repaint with the new color.
                    self.tabs[i].scene.invalidate_layer_dependencies(&names);
                    self.tabs[i].layers.color_picker_row = None;
                    self.tabs[i].layers.color_full_palette = false;
                    self.sync_ribbon_layers();
                }
                Task::none()
            }

            Message::LayerLinetypeSet(lt) => {
                let i = self.active_tab;
                let names = self.selected_layer_names(i);
                if !names.is_empty() {
                    let undo = self.begin_layer_undo(i, "LAYER LINETYPE", &names);
                    for name in &names {
                        if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                            dl.line_type = lt.clone();
                        }
                    }
                    for pl in self.tabs[i].layers.layers.iter_mut() {
                        if names.contains(&pl.name) {
                            pl.linetype = lt.clone();
                        }
                    }
                    self.tabs[i].dirty = true;
                    self.commit_layer_undo(i, undo);
                    // Linetype is baked into the cached wires; repaint.
                    self.tabs[i].scene.invalidate_layer_dependencies(&names);
                }
                Task::none()
            }

            Message::LayerLineweightSet(lw) => {
                let i = self.active_tab;
                let names = self.selected_layer_names(i);
                if !names.is_empty() {
                    let undo = self.begin_layer_undo(i, "LAYER LINEWEIGHT", &names);
                    for name in &names {
                        if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                            dl.line_weight = lw;
                        }
                    }
                    for pl in self.tabs[i].layers.layers.iter_mut() {
                        if names.contains(&pl.name) {
                            pl.lineweight = lw;
                        }
                    }
                    self.tabs[i].dirty = true;
                    self.commit_layer_undo(i, undo);
                    // Lineweight is baked into the cached wires; repaint.
                    self.tabs[i].scene.invalidate_layer_dependencies(&names);
                }
                Task::none()
            }

            Message::LayerTransparencyEdit(idx, s) => {
                let i = self.active_tab;
                let val = if let Ok(v) = s.parse::<i32>() {
                    Some(v.clamp(0, 90))
                } else if s.is_empty() {
                    Some(0)
                } else {
                    None
                };
                // Apply the edited transparency to every selected layer (#236).
                if let Some(v) = val {
                    let targets = self.layer_row_action_targets(i, idx);
                    for name in &targets {
                        if let Some(layer) =
                            self.tabs[i].scene.document.layers.get_mut(name)
                        {
                            layer.transparency =
                                acadrust::types::Transparency::from_percent(v as f64 / 100.0);
                        }
                        if let Some(pl) =
                            self.tabs[i].layers.layers.iter_mut().find(|l| &l.name == name)
                        {
                            pl.transparency = v;
                        }
                    }
                    if !targets.is_empty() {
                        self.tabs[i].scene.invalidate_layer_dependencies(&targets);
                        self.tabs[i].dirty = true;
                    }
                }
                Task::none()
            }

            // ── Cursor / viewport messages ─────────────────────────────────
            Message::CursorMoved(p, viewport) => self.on_cursor_moved(p, viewport),

            Message::ViewportMove(p) => self.on_viewport_move(p),

            Message::ViewportExit => self.on_viewport_exit(),

            // ── Per-pane Model viewport ───────────────────────────────────
            Message::PaneResized(ev) => self.on_pane_resized(ev),
            Message::PaneClicked(pane) => self.on_pane_clicked(pane),
            Message::PaneDragged(ev) => self.on_pane_dragged(ev),
            Message::PaneMove(idx, local) => {
                if self.color_pick_target.is_some() {
                    return Task::none();
                }
                let p = self.pane_canvas_point(idx, local);
                // While dragging a pane, just track the cursor (no focus swap or
                // snap) so the drop target reads cleanly.
                if self.pane_move_from.is_some() {
                    self.tabs[self.active_tab]
                        .scene
                        .selection
                        .borrow_mut()
                        .last_move_pos = Some(p);
                    return Task::none();
                }
                self.focus_model_pane(idx);
                self.on_viewport_move(p)
            }
            Message::PaneMoveStart => {
                let i = self.active_tab;
                self.pane_move_from = Some(self.tabs[i].scene.active_model_tile.get());
                Task::none()
            }
            Message::PanePress(idx) => {
                // A click-away into the drawing area re-syncs the active-row
                // highlight against real focus before the pick runs.
                let sweep = self.sync_active_field_if_any();
                // A fresh press ends any stale (un-dropped) pane move.
                self.pane_move_from = None;
                self.focus_model_pane(idx);
                Task::batch(vec![sweep, self.on_viewport_left_press()])
            }
            Message::PaneRelease(idx) => {
                // Finishing a pane-move drag: swap the source pane with the one
                // released over, instead of the normal release handling.
                if let Some(from) = self.pane_move_from.take() {
                    let i = self.active_tab;
                    self.tabs[i].scene.swap_model_panes(from, idx);
                    self.tabs[i].scene.camera_generation += 1;
                    return Task::none();
                }
                self.focus_model_pane(idx);
                self.on_viewport_left_release()
            }
            Message::PaneRightPress(idx) => {
                self.focus_model_pane(idx);
                self.update(Message::ViewportRightPress)
            }
            Message::PaneRightRelease(idx) => {
                self.focus_model_pane(idx);
                self.update(Message::ViewportRightRelease)
            }
            Message::PaneMiddlePress(idx) => {
                self.focus_model_pane(idx);
                self.update(Message::ViewportMiddlePress)
            }
            Message::PaneMiddleRelease(idx) => {
                self.focus_model_pane(idx);
                self.update(Message::ViewportMiddleRelease)
            }
            Message::PaneScroll(idx, d) => {
                self.focus_model_pane(idx);
                self.update(Message::ViewportScroll(d))
            }

            Message::ViewportLeftPress => {
                let sweep = self.sync_active_field_if_any();
                Task::batch(vec![sweep, self.on_viewport_left_press()])
            }

            Message::ViewportLeftRelease => self.on_viewport_left_release(),

            Message::ViewportRightPress => {
                let i = self.active_tab;
                self.ribbon.close_dropdown();
                // Shift+RMB: the one-shot snap override menu at the cursor —
                // pick a snap for just the next point, then it expires (#337).
                if self.shift_down {
                    let pos = self.tabs[i].scene.selection.borrow().last_move_pos;
                    if let Some(p) = pos {
                        self.snap_override_popup = Some(p);
                    }
                    return Task::none();
                }
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                let Some(p) = sel.last_move_pos else {
                    return Task::none();
                };
                sel.context_menu = None;
                sel.right_down = true;
                sel.right_press_pos = Some(p);
                sel.right_press_time = Some(iced::time::Instant::now());
                sel.right_last_pos = Some(p);
                sel.right_dragging = false;
                Task::none()
            }

            Message::ViewportRightRelease => {
                let i = self.active_tab;
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                let Some(click_pos) = sel.last_move_pos else {
                    return Task::none();
                };
                if !sel.right_down {
                    return Task::none();
                }
                let was_click = !sel.right_dragging;
                sel.right_down = false;
                sel.right_press_pos = None;
                sel.right_press_time = None;
                sel.right_last_pos = None;
                sel.right_dragging = false;
                sel.orbit_pivot = None;
                if !was_click {
                    return Task::none();
                }
                // A command name (or option keyword / value) typed into the
                // command line but not yet entered runs on right-click, exactly
                // as pressing Enter would. Route through CommandFinalize — the
                // canonical Enter action — so the same MText / grip-popup guards
                // apply and a non-empty line is forwarded to the submit path.
                // Without this the typed text would be swallowed by the context
                // menu (when idle) or the Enter cycle. Every other right-click
                // behaviour below is unchanged and only applies when the command
                // line is empty. Pending text always runs and resets the Enter
                // cycle so the next right-click acts as Enter again.
                if !self.command_line.input.trim().is_empty() {
                    sel.right_click_entered = false;
                    drop(sel);
                    return self.update(Message::CommandFinalize);
                }
                // A right-click (no orbit). While a command is active the first
                // right-click acts as Enter (commit / close); a second
                // consecutive right-click opens the context menu instead. When
                // idle it always opens the menu. (Right-drag, handled above,
                // always orbits.) Any other interaction — a left-click pick or a
                // new command — resets the cycle so the next right-click is Enter.
                if self.tabs[i].active_cmd.is_some() && !sel.right_click_entered {
                    sel.right_click_entered = true;
                    drop(sel);
                    return self.update(Message::CommandFinalize);
                }
                sel.right_click_entered = false;
                sel.context_menu = Some(click_pos);
                sel.draworder_submenu = false;
                sel.junction_menu_submenu = false;
                drop(sel);
                // If the cursor is hovering a wall axis endpoint grip when the
                // context menu opens, remember which junction (axis handle +
                // end_index) it's anchored on so the menu can offer the
                // per-junction join-override actions (#join-constraints step 4).
                let junction = self.grip_hover.as_ref().and_then(|h| {
                    let axis = crate::modules::aec::commands::resolve_wall_package(
                        &self.tabs[i].scene,
                        h.handle,
                    );
                    let vertices = crate::modules::aec::commands::get_wall_vertices(
                        &self.tabs[i].scene,
                        axis,
                    );
                    if vertices.len() < 2 {
                        return None;
                    }
                    if h.grip_id == 0 {
                        Some((axis, 0usize))
                    } else if h.grip_id == vertices.len() - 1 {
                        Some((axis, 1usize))
                    } else {
                        None
                    }
                });
                self.tabs[i]
                    .scene
                    .selection
                    .borrow_mut()
                    .junction_menu = junction;
                Task::none()
            }

            Message::ViewportMiddlePress => self.on_viewport_middle_press(),

            Message::ViewportMiddleRelease => {
                let i = self.active_tab;
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.middle_down = false;
                sel.middle_last_pos = None;
                // End of a Shift+MMB orbit — drop the captured pivot so the next
                // gesture recomputes it against the current selection. (#229)
                sel.orbit_pivot = None;
                drop(sel);
                self.arm_hover_after_navigation(i);
                Task::none()
            }

            Message::ViewportScroll(delta) => self.on_viewport_scroll(delta),

            Message::ViewportClick(viewport) => self.on_viewport_click(viewport),

            Message::WindowResized(w, h) => {
                self.vp_size = ((w - 440.0).max(200.0), h);
                self.win_size = (w, h);
                Task::none()
            }

            Message::ViewCubeSnap(region) => self.on_view_cube_snap(region),
            Message::ViewCubeSnapWorld(region) => self.on_view_cube_snap_world(region),

            Message::ViewCubeHome => {
                let i = self.active_tab;
                self.clear_navigation_hover(i);
                self.tabs[i].scene.remember_current_view();
                let r_ucs = self.tabs[i].scene.viewcube_ucs_mat();
                if self.tabs[i].scene.active_viewport.is_some() {
                    self.tabs[i]
                        .scene
                        .mutate_active_viewport_camera(|c| c.home_view(r_ucs));
                } else {
                    self.tabs[i].scene.camera.borrow_mut().home_view(r_ucs);
                }
                self.tabs[i].scene.camera_generation += 1;
                self.command_line.push_output(crate::t!("View: Home").as_ref());
                Task::none()
            }

            Message::ViewCubeRoll(cw) => {
                let i = self.active_tab;
                self.clear_navigation_hover(i);
                self.tabs[i].scene.remember_current_view();
                let ang = if cw {
                    std::f32::consts::FRAC_PI_2
                } else {
                    -std::f32::consts::FRAC_PI_2
                };
                if self.tabs[i].scene.active_viewport.is_some() {
                    self.tabs[i]
                        .scene
                        .mutate_active_viewport_camera(|c| c.roll_by(ang));
                } else {
                    self.tabs[i].scene.camera.borrow_mut().roll_by(ang);
                }
                self.tabs[i].scene.camera_generation += 1;
                Task::none()
            }

            Message::ViewCubeNudge(dir) => {
                use crate::scene::NudgeDir;
                let (horizontal, positive) = match dir {
                    NudgeDir::Up => (false, false),
                    NudgeDir::Down => (false, true),
                    NudgeDir::Left => (true, false),
                    NudgeDir::Right => (true, true),
                };
                let i = self.active_tab;
                self.clear_navigation_hover(i);
                self.tabs[i].scene.remember_current_view();
                if self.tabs[i].scene.active_viewport.is_some() {
                    self.tabs[i]
                        .scene
                        .mutate_active_viewport_camera(|c| c.nudge_90(horizontal, positive));
                } else {
                    self.tabs[i]
                        .scene
                        .camera
                        .borrow_mut()
                        .nudge_90(horizontal, positive);
                }
                self.tabs[i].scene.camera_generation += 1;
                Task::none()
            }

            Message::SetViewcubeUcs(name) => {
                let i = self.active_tab;
                let mut changed = false;
                if name.is_empty() || name == "WCS" {
                    self.tabs[i].active_ucs = None;
                    self.command_line.push_output(crate::t!("UCS: World").as_ref());
                    changed = true;
                } else if let Some(named) = self.tabs[i].scene.document.ucss.get(&name).cloned() {
                    self.tabs[i].active_ucs = Some(named);
                    self.command_line.push_output(crate::tf!("UCS: {}", name).as_ref());
                    changed = true;
                }
                if changed {
                    self.commit_active_ucs_change(i, "UCS");
                    self.tabs[i].scene.camera_generation += 1;
                }
                Task::none()
            }

            Message::GripDwellTick => {
                let i = self.active_tab;
                // Reuse the move-time logic — `p` is the last cursor
                // position the viewport saw, which is also what the
                // hover state was last set with.
                let p = self.tabs[i]
                    .scene
                    .selection
                    .borrow()
                    .last_move_pos
                    .unwrap_or(self.cursor_pos);
                self.update_grip_hover(i, p);
                Task::none()
            }

            Message::HoverDwellTick => self.on_hover_dwell_tick(),

            Message::InteractionIndexReady {
                tab_id,
                epoch,
                source,
                wires,
                index,
                build_ms,
            } => {
                if self.active_interaction_index == Some((tab_id, epoch, source)) {
                    self.active_interaction_index = None;
                }
                let installed = self
                    .tabs
                    .iter()
                    .position(|tab| tab.id == tab_id)
                    .is_some_and(|i| {
                        self.tabs[i].scene.install_prepared_interaction_index(
                            epoch, source, wires, index,
                        )
                    });
                if crate::perf::enabled() {
                    crate::perf_record!(
                        "[perf] interaction-index-bg {:>7.1}ms installed={installed}",
                        build_ms,
                    );
                }
                while let Some((
                    queued_tab,
                    queued_epoch,
                    queued_source,
                    queued_wires,
                    screen_height,
                )) = self.queued_interaction_indices.pop_front()
                {
                    let Some(i) = self.tabs.iter().position(|tab| tab.id == queued_tab) else {
                        continue;
                    };
                    let stale = self.tabs[i].scene.geometry_epoch != queued_epoch
                        || std::sync::Arc::as_ptr(&queued_wires) as usize != queued_source;
                    let (wires, screen_height) = if stale {
                        (
                            self.tabs[i].scene.hit_test_wires(),
                            self.tabs[i].scene.selection.borrow().vp_size.1,
                        )
                    } else {
                        (queued_wires, screen_height)
                    };
                    if let Some(task) = self.prepare_interaction_index_task(
                        i,
                        wires,
                        screen_height,
                    ) {
                        return task;
                    }
                }
                if self.active_interaction_index.is_none() && !self.tabs.is_empty() {
                    let i = self.active_tab.min(self.tabs.len() - 1);
                    let wires = self.tabs[i].scene.hit_test_wires();
                    let screen_height = self.tabs[i].scene.selection.borrow().vp_size.1;
                    self.prepare_interaction_index_task(i, wires, screen_height)
                        .unwrap_or_else(Task::none)
                } else {
                    Task::none()
                }
            }

            Message::VisibilityPick(idx) => {
                if let Some(popup) = self.visibility_popup.take() {
                    self.apply_visibility_state(popup.insert_handle, idx);
                }
                Task::none()
            }

            Message::GripMenuPick(idx) => self.on_grip_menu_pick(idx),

            // ── Snap / mode toggles ───────────────────────────────────────
            Message::ToggleSnapEnabled => {
                self.snapper.toggle_global();
                self.sync_vport_display(self.active_tab);
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::ToggleGridSnap => {
                self.snapper.toggle_grid_snap();
                self.sync_vport_display(self.active_tab);
                Task::none()
            }
            Message::ToggleIsometricDrafting => {
                self.isometric_drafting = !self.isometric_drafting;
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::SetIsoPlane(plane) => {
                self.isometric_drafting = true;
                self.iso_plane = plane;
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::CycleIsoPlane => {
                if self.isometric_drafting {
                    self.iso_plane = self.iso_plane.next();
                } else {
                    self.isometric_drafting = true;
                }
                self.command_line.push_output(crate::tf!(
                    "Isometric plane: {}.",
                    self.iso_plane.label()
                ).as_ref());
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::ResetDraftingRotation => {
                self.snap_angle_deg = 0.0;
                let i = self.active_tab;
                if self.tabs[i].active_ucs.is_some() {
                    self.tabs[i].active_ucs = None;
                    self.commit_active_ucs_change(i, "UCS");
                    self.tabs[i].scene.camera_generation += 1;
                }
                self.command_line
                    .push_output(crate::t!("Drafting rotation reset to World at 0°.").as_ref());
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::ToggleGrid => {
                self.show_grid ^= true;
                self.sync_vport_display(self.active_tab);
                Task::none()
            }
            Message::ToggleOrtho => {
                self.ortho_mode ^= true;
                if self.ortho_mode {
                    self.polar_mode = false;
                }
                // If the user manually toggles ortho during a command that
                // suppressed it (e.g. RECTANG), the toggle is permanent —
                // don't restore the pre-command state when the command ends.
                self.rect_suppressed_ortho = false;
                Task::none()
            }
            Message::ToggleLineweightDisplay => {
                let i = self.active_tab;
                if i < self.tabs.len() {
                    let h = &mut self.tabs[i].scene.document.header;
                    h.lineweight_display = !h.lineweight_display;
                    // No retessellate — the wire shader reads the flag from uniforms.
                    self.tabs[i].dirty = true;
                }
                Task::none()
            }
            Message::CycleCoordsMode => {
                // $COORDS 0 (static) → 1 (live absolute) → 2 (polar) → 0.
                let i = self.active_tab;
                if i < self.tabs.len() {
                    let mode = {
                        let h = &mut self.tabs[i].scene.document.header;
                        h.coords_mode = (h.coords_mode + 1).rem_euclid(3);
                        h.coords_mode
                    };
                    self.tabs[i].dirty = true;
                    let label = match mode {
                        0 => "static",
                        2 => "polar",
                        _ => "live",
                    };
                    self.command_line
                        .push_output(crate::tf!("COORDS = {mode} ({label})").as_ref());
                }
                Task::none()
            }
            Message::TogglePolar => {
                self.polar_mode ^= true;
                if self.polar_mode {
                    self.ortho_mode = false;
                }
                Task::none()
            }
            Message::ToggleDynInput => {
                self.dyn_input ^= true;
                Task::none()
            }
            Message::ToggleViewCube => {
                self.show_viewcube ^= true;
                self.ribbon.set_viewcube(self.show_viewcube);
                Task::none()
            }
            Message::ToggleProperties => {
                self.show_properties ^= true;
                self.ribbon.set_properties(self.show_properties);
                Task::none()
            }
            Message::ToggleFileTabs => {
                self.show_file_tabs ^= true;
                self.ribbon.set_file_tabs(self.show_file_tabs);
                Task::none()
            }
            Message::ToggleLayoutTabs => {
                self.show_layout_tabs ^= true;
                self.ribbon.set_layout_tabs(self.show_layout_tabs);
                Task::none()
            }
            Message::ToggleOTrack => {
                self.snapper.otrack_enabled ^= true;
                if !self.snapper.otrack_enabled {
                    self.snapper.clear_tracking();
                    self.otrack_active = None;
                    self.otrack_kind = None;
                }
                Task::none()
            }
            Message::SetPolarAngle(deg) => {
                self.polar_increment_deg = deg;
                self.polar_mode = true;
                self.ortho_mode = false;
                self.polar_popup_open = false;
                Task::none()
            }
            Message::TogglePolarPopup => {
                // MenuBar owns its open state. Reset only the transient field
                // whenever the caret starts a fresh interaction.
                self.polar_custom_input.clear();
                Task::none()
            }
            Message::ClosePolarPopup => {
                self.polar_popup_open = false;
                Task::none()
            }
            Message::PolarCustomInput(s) => {
                self.polar_custom_input = s;
                Task::none()
            }
            Message::SubmitPolarCustom => {
                // Accept any positive angle up to a full turn; ignore garbage.
                if let Ok(v) = self.polar_custom_input.trim().parse::<f32>() {
                    if v > 0.0 && v <= 360.0 {
                        self.polar_increment_deg = v;
                        self.polar_mode = true;
                        self.ortho_mode = false;
                    }
                }
                self.polar_custom_input.clear();
                self.polar_popup_open = false;
                Task::none()
            }
            Message::SetAnnotationScale(scale) => {
                self.scale_popup_open = false;
                let auto_scale = self.annotation_auto_scale;
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    let previous = tab.scene.displayed_annotation_scale_handle();
                    if let Some(handle) = tab.scene.set_annotation_scale_named(&scale) {
                        if auto_scale > 0 {
                            tab.scene.add_annotation_scale_to_objects(
                                handle,
                                previous,
                                auto_scale as u8,
                            );
                        }
                        tab.dirty = true;
                    }
                }
                Task::none()
            }
            Message::SetViewportScale(scale) => {
                self.scale_popup_open = false;
                let auto_scale = self.annotation_auto_scale;
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    let previous = tab.scene.displayed_annotation_scale_handle();
                    if let Some(handle) = tab.scene.set_viewport_scale_named(&scale) {
                        if auto_scale > 0 {
                            tab.scene.add_annotation_scale_to_objects(
                                handle,
                                previous,
                                auto_scale as u8,
                            );
                        }
                        tab.dirty = true;
                    }
                }
                Task::none()
            }
            Message::ToggleAnnotationVisibility => {
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    let value = !tab.scene.annotation_all_visible();
                    tab.scene.set_annotation_all_visible(value);
                    tab.dirty = true;
                }
                Task::none()
            }
            Message::ToggleAnnotationAutoAdd => {
                self.annotation_auto_scale = match self.annotation_auto_scale {
                    0 => 4,
                    value => -value,
                };
                Task::none()
            }
            Message::SyncViewportAnnotationScale => {
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    if tab.scene.sync_viewport_annotation_scale() {
                        tab.dirty = true;
                    }
                }
                Task::none()
            }
            Message::ToggleScalePopup => {
                self.scale_popup_open ^= true;
                Task::none()
            }
            Message::CloseScalePopup => {
                self.scale_popup_open = false;
                Task::none()
            }
            Message::ScaleManagerOpen => {
                let i = self.active_tab;
                self.scale_popup_open = false;
                // Snapshot so New / Copy / Delete / edits revert if closed
                // without Apply.
                self.scale_stage_begin();
                // Fallback scales are virtual (no real objects), so they can't
                // be edited or renamed. Materialise the standard set into real
                // staged objects so the manager behaves like a drawing with its
                // own list; the stage reverts them on close unless applied.
                if self.tabs[i].scene.ensure_real_scale_list() {
                    self.scale_stage_materialized();
                }
                self.scale_rename = None;
                let cur = self.tabs[i]
                    .scene
                    .document
                    .header
                    .current_annotation_scale
                    .clone();
                // Select the current scale, or the first one if it isn't listed.
                if self.tabs[i].scene.scale_paper_drawing(&cur).is_some() {
                    self.load_scale_editor(&cur);
                } else if let Some((first, _, _)) =
                    self.tabs[i].scene.scale_list().into_iter().next()
                {
                    self.load_scale_editor(&first);
                }
                self.active_modal = Some(crate::app::ModalKind::ScaleManager);
                Task::none()
            }
            Message::AnnoObjectScaleOpen => {
                // The dialog edits a single object's per-scale memberships.
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if handles.len() == 1 {
                    let ok = self.tabs[i]
                        .scene
                        .document
                        .get_entity(handles[0])
                        .is_some_and(crate::scene::annotative::supports_annotation_context);
                    if ok {
                        self.anno_object_scale_target = Some(handles[0]);
                        self.active_modal = Some(crate::app::ModalKind::AnnoObjectScale);
                    } else {
                        self.command_line
                            .push_info(crate::t!("The selected object does not support annotation scales.").as_ref());
                    }
                } else {
                    self.command_line
                        .push_info(crate::t!("Select one object first, then run OBJECTSCALE.").as_ref());
                }
                Task::none()
            }
            Message::AnnoObjectScaleToggle(name) => {
                let i = self.active_tab;
                if let Some(entity) = self.anno_object_scale_target {
                    if self.tabs[i].scene.is_layer_locked(entity) {
                        return Task::none();
                    }
                    if let Some(sh) = self.tabs[i].scene.scale_handle_ensuring(&name) {
                        self.push_undo_snapshot(i, "OBJECTSCALE");
                        let doc = &mut self.tabs[i].scene.document;
                        let is_member = crate::scene::annotative::object_scale_memberships(doc, entity)
                            .iter()
                            .any(|(_, h)| *h == sh);
                        if is_member {
                            crate::scene::annotative::remove_annotation_context_for_scale(
                                doc, entity, sh,
                            );
                        } else {
                            crate::scene::annotative::create_annotation_context(doc, entity, sh);
                        }
                        self.tabs[i].dirty = true;
                        self.invalidate_property_targets(i, &[entity]);
                        self.refresh_properties();
                    }
                }
                Task::none()
            }
            Message::ScaleManagerSelect(name) => {
                // Stage the current editor edits before switching so they aren't
                // lost, then load the newly-selected scale.
                self.scale_rename = None;
                self.scale_apply_current();
                self.load_scale_editor(&name);
                Task::none()
            }
            Message::ScaleManagerPaperBuf(s) => {
                self.scale_manager_paper_buf = s;
                Task::none()
            }
            Message::ScaleManagerDrawingBuf(s) => {
                self.scale_manager_drawing_buf = s;
                Task::none()
            }
            Message::ScaleManagerNew => {
                // Add a new scale to the list immediately (staged) and select it,
                // like the style managers' New. The user edits its name / ratio;
                // it's kept only if Apply is pressed before the window closes.
                self.scale_apply_current();
                let i = self.active_tab;
                let name = self.unique_scale_name("New Scale");
                if self.tabs[i].scene.add_scale(&name, 1.0, 1.0) {
                    self.load_scale_editor(&name);
                    self.scale_stage_mark();
                }
                Task::none()
            }
            Message::ScaleManagerCopy => {
                // Duplicate the selected scale under a unique name (staged).
                self.scale_apply_current();
                let i = self.active_tab;
                let sel = self.scale_manager_selected.clone();
                if !sel.is_empty() {
                    let (paper, drawing) =
                        self.tabs[i].scene.scale_paper_drawing(&sel).unwrap_or((1.0, 1.0));
                    let name = self.unique_scale_name(&sel);
                    if self.tabs[i].scene.add_scale(&name, paper, drawing) {
                        self.load_scale_editor(&name);
                        self.scale_stage_mark();
                    }
                }
                Task::none()
            }
            Message::ScaleRenameStart(name) => {
                // Stage current editor edits, then rename this row inline.
                self.scale_apply_current();
                self.scale_rename_buf = name.clone();
                self.scale_rename = Some(name);
                iced::widget::operation::focus(crate::ui::style::scale_manager::rename_input_id())
            }
            Message::ScaleRenameEdit(s) => {
                self.scale_rename_buf = s;
                Task::none()
            }
            Message::ScaleRenameCommit => {
                let i = self.active_tab;
                if let Some(old) = self.scale_rename.take() {
                    let new = self.scale_rename_buf.trim().to_string();
                    if !new.is_empty() && !new.eq_ignore_ascii_case(&old) {
                        let (paper, drawing) =
                            self.tabs[i].scene.scale_paper_drawing(&old).unwrap_or((1.0, 1.0));
                        // Only fall back to add_scale for a built-in fallback (no
                        // real object); never for a real scale whose rename was
                        // rejected (name collision) — that would duplicate it.
                        let ok = self.tabs[i].scene.edit_scale(&old, &new, paper, drawing)
                            || (self.tabs[i].scene.scale_paper_drawing(&old).is_none()
                                && self.tabs[i].scene.add_scale(&new, paper, drawing));
                        if ok {
                            if self.tabs[i]
                                .scene
                                .document
                                .header
                                .current_annotation_scale
                                .eq_ignore_ascii_case(&old)
                            {
                                self.tabs[i].scene.document.header.current_annotation_scale =
                                    new.clone();
                            }
                            if self.scale_manager_selected.eq_ignore_ascii_case(&old) {
                                self.load_scale_editor(&new);
                            }
                            self.scale_stage_mark();
                        }
                    }
                }
                Task::none()
            }
            Message::ScaleManagerApply => {
                // Fold the editor into the selected scale, then commit the staged
                // transaction (this edit plus any New / Copy / Delete since open)
                // as one undo entry.
                self.scale_apply_current();
                self.scale_stage_commit();
                Task::none()
            }
            Message::ScaleManagerDelete => {
                // Staged: reverted on close unless a later Apply commits it.
                let i = self.active_tab;
                let sel = self.scale_manager_selected.clone();
                let cur = self.tabs[i]
                    .scene
                    .document
                    .header
                    .current_annotation_scale
                    .clone();
                if !sel.is_empty() && !sel.eq_ignore_ascii_case(&cur) {
                    if self.tabs[i].scene.remove_scale(&sel) {
                        self.scale_manager_selected.clear();
                        self.scale_manager_paper_buf.clear();
                        self.scale_manager_drawing_buf.clear();
                        self.scale_stage_mark();
                    }
                }
                Task::none()
            }
            Message::ScaleManagerSetCurrent => {
                // Set Current takes effect immediately, exactly like the scale
                // pill — it isn't part of the staged list transaction, so it is
                // never rolled back when the manager closes.
                let i = self.active_tab;
                let sel = self.scale_manager_selected.clone();
                let previous = self.tabs[i].scene.displayed_annotation_scale_handle();
                if let Some(scale) = self.tabs[i].scene.set_annotation_scale_named(&sel) {
                    if self.annotation_auto_scale > 0 {
                        self.tabs[i].scene.add_annotation_scale_to_objects(
                            scale,
                            previous,
                            self.annotation_auto_scale as u8,
                        );
                    }
                    self.tabs[i].dirty = true;
                }
                Task::none()
            }
            Message::ToggleLayoutList => {
                if self.tabs[self.active_tab].is_start {
                    self.layout_list_open = false;
                    return Task::none();
                }
                self.layout_list_open ^= true;
                Task::none()
            }
            Message::CloseLayoutList => {
                self.layout_list_open = false;
                Task::none()
            }
            Message::ToggleStatusBarMenu => {
                self.statusbar_menu_open ^= true;
                Task::none()
            }
            Message::CloseStatusBarMenu => {
                self.statusbar_menu_open = false;
                Task::none()
            }
            Message::ToggleStatusPill(pill) => {
                // Keep the menu open so several pills can be toggled in a row.
                self.statusbar_config.toggle(pill);
                self.save_config();
                Task::none()
            }
            Message::ToggleCleanScreen => {
                self.clean_screen ^= true;
                Task::none()
            }
            Message::ToggleTransparencyDisplay => {
                let i = self.active_tab;
                if i < self.tabs.len() {
                    // No retessellate — the wire shader reads the flag from uniforms.
                    self.tabs[i].scene.transparency_display ^= true;
                }
                Task::none()
            }
            Message::ToggleQuickProperties => {
                self.quick_properties ^= true;
                if self.quick_properties {
                    self.quick_properties_anchor =
                        self.tabs[self.active_tab].last_cursor_screen;
                }
                self.save_config();
                Task::none()
            }
            Message::ToggleSelectionCycling => {
                self.selection_cycling ^= true;
                self.cycle_candidates = None;
                self.tabs[self.active_tab].scene.set_hover_highlight(None);
                Task::none()
            }
            Message::CycleSelect(handle) => {
                // Add the picked object to the current selection (accumulate).
                let quick_properties_anchor = self
                    .cycle_candidates
                    .as_ref()
                    .map(|(point, _)| *point);
                self.cycle_candidates = None;
                let i = self.active_tab;
                self.tabs[i].scene.set_hover_highlight(None);
                self.tabs[i].scene.select_entity(handle, false);
                self.tabs[i].scene.expand_selection_for_groups(&[handle]);
                self.refresh_properties();
                if let Some(point) = quick_properties_anchor {
                    self.quick_properties_anchor = point;
                }
                Task::none()
            }
            Message::CycleHover(handle) => {
                let i = self.active_tab;
                self.tabs[i].scene.set_hover_highlight(handle);
                Task::none()
            }
            Message::CycleHoverExit(handle) => {
                // Only clear if another row hasn't already taken the highlight;
                // enter/exit can fire out of order when moving between rows.
                let i = self.active_tab;
                if self.tabs[i].scene.hover_highlight == Some(handle) {
                    self.tabs[i].scene.set_hover_highlight(None);
                }
                Task::none()
            }
            Message::CycleCancel => {
                self.cycle_candidates = None;
                self.tabs[self.active_tab].scene.set_hover_highlight(None);
                Task::none()
            }
            Message::ToggleSelectionFilterPopup => {
                self.selection_filter_popup_open ^= true;
                Task::none()
            }
            Message::CloseSelectionFilterPopup => {
                self.selection_filter_popup_open = false;
                Task::none()
            }
            Message::ToggleSelectionFilterType(name) => {
                let f = &mut self.tabs[self.active_tab].scene.selection_filter;
                if !f.remove(&name) {
                    f.insert(name);
                }
                Task::none()
            }
            Message::SelectionFilterSelectAll => {
                self.tabs[self.active_tab].scene.selection_filter.clear();
                Task::none()
            }
            Message::SelectionFilterClearAll => {
                let i = self.active_tab;
                let types = self.tabs[i].scene.entity_type_names_in_layout();
                let f = &mut self.tabs[i].scene.selection_filter;
                for t in types {
                    f.insert(t.to_string());
                }
                Task::none()
            }
            Message::ToggleUnitsPopup => {
                self.units_popup_open ^= true;
                Task::none()
            }
            Message::CloseUnitsPopup => {
                self.units_popup_open = false;
                Task::none()
            }
            Message::OpenDrawingUnits => {
                self.units_popup_open = false;
                let header = &self.tabs[self.active_tab].scene.document.header;
                self.drawing_units = Some(crate::ui::window::drawing_units::State {
                    linear_format: header.linear_unit_format,
                    linear_precision: header.linear_unit_precision,
                    angular_format: header.angular_unit_format,
                    angular_precision: header.angular_unit_precision,
                    clockwise: header.angle_direction != 0,
                    base_angle: format!("{:.6}", header.angle_base.to_degrees())
                        .trim_end_matches('0')
                        .trim_end_matches('.')
                        .to_string(),
                    insertion_units: header.insertion_units,
                });
                self.active_modal = Some(crate::app::ModalKind::DrawingUnits);
                Task::none()
            }
            Message::DrawingUnitsField(field) => {
                use crate::ui::window::drawing_units::Field;
                let Some(state) = self.drawing_units.as_mut() else {
                    return Task::none();
                };
                match field {
                    Field::LinearFormat(v) => state.linear_format = v,
                    Field::LinearPrecision(v) => state.linear_precision = v,
                    Field::AngularFormat(v) => state.angular_format = v,
                    Field::AngularPrecision(v) => state.angular_precision = v,
                    Field::Clockwise(v) => state.clockwise = v,
                    // Kept as typed so a lone "-" or a trailing "." survives
                    // until the number it is becoming is finished.
                    Field::BaseAngle(v) => state.base_angle = v,
                    Field::InsertionUnits(v) => state.insertion_units = v,
                }
                Task::none()
            }
            Message::DrawingUnitsApply => {
                let Some(state) = self.drawing_units.take() else {
                    self.active_modal = None;
                    return Task::none();
                };
                self.active_modal = None;
                let i = self.active_tab;
                self.push_undo_snapshot(i, "UNITS");
                let header = &mut self.tabs[i].scene.document.header;
                header.linear_unit_format = state.linear_format;
                header.linear_unit_precision = state.linear_precision;
                header.angular_unit_format = state.angular_format;
                header.angular_unit_precision = state.angular_precision;
                header.angle_direction = i16::from(state.clockwise);
                // A base angle that will not parse is a half-finished edit, not
                // an instruction to move zero — leave the drawing's own value.
                if let Ok(degrees) = state.base_angle.trim().parse::<f64>() {
                    header.angle_base = degrees.to_radians();
                }
                header.insertion_units = state.insertion_units;
                self.tabs[i].dirty = true;
                self.tabs[i].scene.bump_geometry();
                self.refresh_properties();
                Task::none()
            }
            Message::ToleranceDialogField(field) => {
                if let Some(state) = self.geometric_tolerance.as_mut() {
                    state.apply_field(field);
                }
                Task::none()
            }
            Message::ToleranceDialogToggle(toggle) => {
                if let Some(state) = self.geometric_tolerance.as_mut() {
                    state.apply_toggle(toggle);
                }
                Task::none()
            }
            Message::ToleranceDialogApply => {
                self.apply_tolerance_dialog_edit();
                Task::none()
            }
            Message::ToleranceDialogOk => {
                let editing = self
                    .geometric_tolerance
                    .as_ref()
                    .and_then(|state| state.editing)
                    .is_some();
                if editing {
                    if self.apply_tolerance_dialog_edit() {
                        self.geometric_tolerance = None;
                        self.active_modal = None;
                        self.reset_modal_geometry();
                    }
                } else {
                    self.begin_tolerance_placement();
                    self.reset_modal_geometry();
                }
                Task::none()
            }
            Message::SetLinearFormat(code) => {
                self.units_popup_open = false;
                let i = self.active_tab;
                if self.tabs[i].scene.document.header.linear_unit_format == code {
                    return Task::none();
                }
                // Every displayed length is written through this, so the whole
                // drawing re-reads at once — no geometry moves, only the way it
                // is written down.
                self.push_undo_snapshot(i, "LUNITS");
                self.tabs[i].scene.document.header.linear_unit_format = code;
                self.tabs[i].dirty = true;
                self.tabs[i].scene.bump_geometry();
                self.refresh_properties();
                let label = crate::modules::draw::units::linear_format_label(code);
                self.command_line
                    .push_output(crate::tf!("Length format is now {label}.").as_ref());
                Task::none()
            }
            Message::SetDrawingUnits(code) => {
                self.units_popup_open = false;
                let i = self.active_tab;
                let current = self.tabs[i].scene.document.header.insertion_units;
                if current == code {
                    return Task::none();
                }
                // Relabelling only: the geometry keeps every number it had, and
                // now says they count something else. Say so, because picking a
                // unit and seeing the drawing sit still otherwise reads as the
                // menu having done nothing. (#668)
                self.push_undo_snapshot(i, "UNITS");
                self.tabs[i].scene.document.header.insertion_units = code;
                self.tabs[i].dirty = true;
                let label = crate::modules::draw::units::label(code);
                self.command_line.push_output(
                    crate::tf!(
                        "Drawing unit is now {label}. Geometry unchanged — use DWGUNITS to convert it."
                    )
                    .as_ref(),
                );
                Task::none()
            }
            Message::ToggleIsolatePopup => {
                self.isolate_popup_open ^= true;
                Task::none()
            }
            Message::CloseIsolatePopup => {
                self.isolate_popup_open = false;
                Task::none()
            }
            Message::ToggleSnap(t) => {
                self.snapper.toggle(t);
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::ToggleSnapPopup => {
                if self.active_modal == Some(super::ModalKind::DraftingSettings) {
                    self.close_active_modal();
                    self.snap_popup_open = false;
                } else {
                    self.active_modal = Some(super::ModalKind::DraftingSettings);
                    self.snap_popup_open = true;
                }
                Task::none()
            }
            Message::CloseSnapPopup => {
                self.snap_popup_open = false;
                if self.active_modal == Some(super::ModalKind::DraftingSettings) {
                    self.close_active_modal();
                }
                Task::none()
            }
            Message::SnapSelectAll => {
                self.snapper.enable_all();
                Task::none()
            }
            Message::SnapClearAll => {
                self.snapper.disable_all();
                Task::none()
            }

            // ── Ribbon dropdowns ──────────────────────────────────────────
            Message::ToggleRibbonDropdown(id) => {
                if self.tabs[self.active_tab].is_start {
                    self.ribbon.close_dropdown();
                } else {
                    self.ribbon.toggle_dropdown(&id);
                }
                Task::none()
            }
            Message::ToggleRibbonPanel(id) => {
                if self.tabs[self.active_tab].is_start {
                    self.ribbon.close_dropdown();
                } else {
                    self.ribbon.toggle_collapsed_panel(&id);
                }
                Task::none()
            }
            Message::CloseRibbonDropdown => {
                self.ribbon.close_dropdown();
                Task::none()
            }
            Message::DropdownSelectItem { dropdown_id, cmd } => {
                if self.tabs[self.active_tab].is_start {
                    self.ribbon.close_dropdown();
                    return Task::none();
                }
                self.ribbon.select_dropdown_item(dropdown_id, cmd);
                self.ribbon.activate_tool(cmd);
                self.dispatch_command(cmd)
            }

            Message::DeleteSelected => {
                // In the MText preview, Delete removes text at the caret.
                if self.mtext_editor.as_ref().is_some_and(|e| e.show_preview) {
                    self.mtext_delete();
                    return Task::none();
                }
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().context_menu = None;
                let mut handles: Vec<_> = self.tabs[i].scene.selected.iter().cloned().collect();
                if !handles.is_empty() {
                    crate::modules::aec::commands::expand_with_wall_derived_handles(
                        &self.tabs[i].scene,
                        &mut handles,
                    );
                    crate::modules::aec::commands::unregister_walls_from_storeys(
                        &mut self.tabs[i].scene,
                        &handles,
                    );
                    // Erase is delta-safe unless a target is in a group (group
                    // cleanup rewrites document.objects).
                    let delta_safe = self.delta_erase_safe(i, &handles);
                    let pending = self.begin_undo(i, "ERASE", handles.len(), delta_safe);
                    // Stash the erased entities so OOPS can restore them.
                    self.oops_cache = handles
                        .iter()
                        .filter_map(|h| self.tabs[i].scene.document.get_entity_arc(*h))
                        .collect();
                    self.tabs[i].scene.erase_entities(&handles);
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                    if let Some(pd) = pending {
                        self.commit_undo_delta(i, pd);
                    }
                }
                Task::none()
            }

            Message::SetModifiers { shift, ctrl } => {
                let ctrl_changed = self.ctrl_down != ctrl;
                self.shift_down = shift;
                self.ctrl_down = ctrl;
                // Releasing Shift drops the hard axis lock immediately (#312)
                // — without this a lock could linger until the next move.
                if !shift {
                    self.axis_lock_dir = None;
                }
                // A live command may key its preview off Ctrl (arc-direction
                // flip). Rebuild it at the current cursor so the flip shows
                // without waiting for the next mouse move.
                let i = self.active_tab;
                if ctrl_changed && self.tabs[i].active_cmd.is_some() {
                    let p = self.tabs[i].last_cursor_screen;
                    return Task::done(Message::ViewportMove(p));
                }
                Task::none()
            }

            // ── In-place MText editor ───────────────────────────────────
            Message::MTextEdit(action) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.content.perform(action);
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextFmt(kind) => {
                self.mtext_apply_fmt(kind);
                Task::none()
            }
            Message::MTextHeight(s) => {
                self.mtext_apply_span_number("height", s);
                Task::none()
            }
            Message::MTextRectWidth(width) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.rect_width = width.max(1e-6);
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextColorChanged(color) => {
                self.mtext_apply_color(color);
                Task::none()
            }
            Message::MTextColorPickerToggle => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.color_picker_open = !ed.color_picker_open;
                }
                Task::none()
            }
            Message::MTextStyle(s) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.style = s;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextFont(f) => {
                self.mtext_apply_font(&f);
                Task::none()
            }
            Message::MTextOblique(s) => {
                self.mtext_apply_span_number("oblique", s);
                Task::none()
            }
            Message::MTextWidth(s) => {
                self.mtext_apply_span_number("width", s);
                Task::none()
            }
            Message::MTextCharSpace(s) => {
                self.mtext_apply_span_number("tracking", s);
                Task::none()
            }
            Message::MTextUndo => {
                self.mtext_undo();
                Task::none()
            }
            Message::MTextRedo => {
                self.mtext_redo();
                Task::none()
            }
            Message::MTextStack => {
                self.mtext_stack_selection();
                Task::none()
            }
            Message::MTextClearFormatting => {
                self.mtext_clear_formatting();
                Task::none()
            }
            Message::MTextInsert(value) => {
                self.mtext_type(&value);
                Task::none()
            }
            Message::MTextAnnotative(value) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.annotative = value;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextColumnMode(mode) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    match mode.as_str() {
                        "Static" => {
                            ed.column_type = 1;
                            ed.column_auto_height = false;
                        }
                        "Dynamic auto" => {
                            ed.column_type = 2;
                            ed.column_auto_height = true;
                        }
                        "Dynamic manual" => {
                            ed.column_type = 2;
                            ed.column_auto_height = false;
                        }
                        _ => ed.column_type = 0,
                    }
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextColumnCount(value) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.column_count = value;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextColumnWidth(value) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.column_width = value;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextColumnGutter(value) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.column_gutter = value;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextColumnHeight(value) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.rect_height = value;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextColumnFlowReversed(value) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.column_flow_reversed = value;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextParagraphNumber(field, value) => {
                self.mtext_apply_paragraph_number(field, value);
                Task::none()
            }
            Message::MTextFindText(value) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.find_text = value;
                }
                Task::none()
            }
            Message::MTextReplaceText(value) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.replace_text = value;
                }
                Task::none()
            }
            Message::MTextFindNext => {
                self.mtext_find_next();
                Task::none()
            }
            Message::MTextReplaceNext => {
                self.mtext_replace_next();
                Task::none()
            }
            Message::MTextReplaceAll => {
                self.mtext_replace_all();
                Task::none()
            }
            Message::MTextJustify(ap) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.attachment = ap;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextAlign(a) => {
                self.mtext_apply_align(a);
                Task::none()
            }
            Message::MTextLineSpacing(f) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.line_spacing = f;
                }
                self.rebuild_mtext_preview();
                Task::none()
            }
            Message::MTextShowPreview(on) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.show_preview = on;
                }
                self.rebuild_mtext_preview();
                // Focus the text area when switching to Edit so the caret
                // shows and typing/clicking edits immediately.
                if on {
                    Task::none()
                } else {
                    iced::widget::operation::focus(iced::widget::Id::new(
                        super::view::MTEXT_TEXT_ID,
                    ))
                }
            }
            Message::MTextSelStart(off) => {
                // Count quick same-spot clicks: 1 = place caret, 2 = select the
                // word, 3 = select all.
                let now = Instant::now();
                let count = match self.mtext_click_time {
                    Some(t)
                        if now.duration_since(t).as_millis() < 400
                            && off.abs_diff(self.mtext_click_off) <= 1 =>
                    {
                        (self.mtext_click_count + 1).min(3)
                    }
                    _ => 1,
                };
                self.mtext_click_time = Some(now);
                self.mtext_click_off = off;
                self.mtext_click_count = count;
                match count {
                    2 => self.mtext_select_word(off),
                    3 => self.mtext_select_all(),
                    _ => {
                        if let Some(ed) = self.mtext_editor.as_mut() {
                            ed.sel_anchor = off;
                            ed.sel = Some((off, off));
                            ed.caret = off;
                            ed.caret_blink_on = true;
                        }
                    }
                }
                self.unfocus_widgets()
            }
            Message::MTextSelTo(off) => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    let a = ed.sel_anchor;
                    ed.sel = Some((a.min(off), a.max(off)));
                    ed.caret = off;
                    ed.caret_blink_on = true;
                }
                Task::none()
            }
            Message::MTextCaretMove(d) => {
                if self.tabs[self.active_tab]
                    .properties
                    .hatch_pattern_picker_open
                {
                    return self.update(Message::PropHatchPatternNavigate(d as i8));
                }
                self.mtext_caret_move(d, false);
                Task::none()
            }
            Message::MTextCaretBlink => {
                if let Some(ed) = self.mtext_editor.as_mut() {
                    ed.caret_blink_on = !ed.caret_blink_on;
                }
                Task::none()
            }
            Message::MTextOk => {
                let committed = self.mtext_commit();
                self.post_editor_closed(committed)
            }
            Message::MTextApply => {
                self.mtext_apply();
                Task::none()
            }
            Message::MTextCancel => {
                self.mtext_cancel();
                self.post_editor_closed(false)
            }

            Message::TextInlineInput(s) => {
                if let Some(ed) = self.text_inline.as_mut() {
                    ed.value = s;
                }
                Task::none()
            }

            // Ctrl+V. The MText editor and (on the web) the TEXT editor read the
            // system clipboard asynchronously — the only paste path that works
            // in the browser, where the synchronous clipboard the iced
            // text_input expects is empty. With no editor open, drawing objects
            // take priority and system text is used when that clipboard is empty.
            Message::PasteShortcut => self.on_paste_shortcut(),

            Message::SystemClipboardPaste(result) => {
                use super::SystemClipboardText as Text;

                match result {
                    Text::Text(text) => {
                        let flat = text.replace(['\r', '\n'], " ").to_uppercase();
                        self.command_line.input.push_str(&flat);
                        self.command_line.autocomplete_cursor = None;
                        self.command_line.cancel_history_navigation();
                        self.focus_cmd_input()
                    }
                    Text::EmptyOrUnsupported => {
                        self.command_line.push_error(
                            crate::tr!("clipboard", "no-supported-content").as_ref(),
                        );
                        Task::none()
                    }
                    Text::Unavailable => {
                        self.command_line
                            .push_error(crate::tr!("clipboard", "unavailable").as_ref());
                        Task::none()
                    }
                    Text::Occupied => {
                        self.command_line
                            .push_error(crate::tr!("clipboard", "occupied").as_ref());
                        Task::none()
                    }
                    Text::ConversionFailed => {
                        self.command_line
                            .push_error(crate::tr!("clipboard", "conversion-failed").as_ref());
                        Task::none()
                    }
                }
            }

            Message::SelectAllShortcut => {
                let i = self.active_tab;
                if self.mtext_editor.as_ref().is_some_and(|e| e.show_preview) {
                    // Ctrl+A in the MText editor selects all of its text.
                    self.mtext_select_all();
                    Task::none()
                } else if self.active_modal == Some(super::ModalKind::Layers) {
                    // Select every row in the Layer Manager (#236).
                    let n = self.tabs[i].layers.layers.len();
                    self.tabs[i].layers.selected_multi = (0..n).collect();
                    self.tabs[i].layers.selected = (n > 0).then_some(0);
                    Task::none()
                } else {
                    self.dispatch_command("SELECTALL")
                }
            }
            Message::FindReplaceOpen => self.open_find_replace(),
            Message::FindReplaceSearchChanged(value) => {
                self.find_replace_search_changed(value);
                Task::none()
            }
            Message::FindReplaceReplacementChanged(value) => {
                self.find_replace_replacement_changed(value);
                Task::none()
            }
            Message::FindReplaceNext => {
                self.find_replace_next();
                Task::none()
            }
            Message::FindReplaceOne => {
                self.find_replace_one();
                Task::none()
            }
            Message::FindReplaceAll => {
                self.find_replace_all();
                Task::none()
            }
            Message::MTextPasteClip(text) => {
                if let Some(text) = text.filter(|t| !t.is_empty()) {
                    // CR/LF arrive as line breaks; MText keeps "\n", drop "\r".
                    self.mtext_type(&text.replace('\r', ""));
                    self.rebuild_mtext_preview();
                }
                Task::none()
            }
            Message::TextInlinePasteClip(text) => {
                if let Some(text) = text.filter(|t| !t.is_empty()) {
                    // Single-line field: collapse newlines, append at the end.
                    let flat = text.replace(['\r', '\n'], " ");
                    if let Some(ed) = self.text_inline.as_mut() {
                        ed.value.push_str(&flat);
                    }
                }
                Task::none()
            }
            Message::TextInlineOk => {
                let committed = self.text_inline_commit();
                self.post_editor_closed(committed)
            }

            Message::DrawOrderSubmenuToggle => {
                let i = self.active_tab;
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.draworder_submenu = !sel.draworder_submenu;
                Task::none()
            }

            Message::WallJustificationSubmenuToggle => {
                let i = self.active_tab;
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.wall_justification_submenu = !sel.wall_justification_submenu;
                Task::none()
            }

            Message::WallJunctionSubmenuToggle => {
                let i = self.active_tab;
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.junction_menu_submenu = !sel.junction_menu_submenu;
                Task::none()
            }

            Message::WallJunctionOverrideSetStyle(style) => {
                let i = self.active_tab;
                let junction = self.tabs[i].scene.selection.borrow().junction_menu;
                if let Some((axis_handle, end_index)) = junction {
                    use crate::modules::aec::commands as aec_cmds;
                    let mut override_data =
                        aec_cmds::read_junction_override(&self.tabs[i].scene, axis_handle, end_index)
                            .unwrap_or_default();
                    override_data.default_style = Some(style);
                    aec_cmds::write_junction_override(
                        &mut self.tabs[i].scene,
                        axis_handle,
                        end_index,
                        &override_data,
                    );
                    let style_library =
                        crate::modules::aec::engine::project::resolve_style_library(
                            self.aec_project_explorer_file.as_ref(),
                        );
                    let (display_rules, style_substitutions) =
                        self.resolve_active_display_config_wall_rules(i, Some(axis_handle));
                    aec_cmds::refresh_wall_after_axis_edit(
                        &mut self.tabs[i].scene,
                        axis_handle,
                        Some(&style_library),
                        display_rules.as_ref(),
                        style_substitutions.as_ref(),
                    );
                    self.reapply_active_display_config_to_wall_packages(i, &[axis_handle]);
                }
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.context_menu = None;
                sel.junction_menu = None;
                drop(sel);
                self.refresh_properties();
                Task::none()
            }

            Message::WallJunctionOverrideReset => {
                let i = self.active_tab;
                let junction = self.tabs[i].scene.selection.borrow().junction_menu;
                if let Some((axis_handle, end_index)) = junction {
                    use crate::modules::aec::commands as aec_cmds;
                    aec_cmds::remove_junction_override(&mut self.tabs[i].scene, axis_handle, end_index);
                    let style_library =
                        crate::modules::aec::engine::project::resolve_style_library(
                            self.aec_project_explorer_file.as_ref(),
                        );
                    let (display_rules, style_substitutions) =
                        self.resolve_active_display_config_wall_rules(i, Some(axis_handle));
                    aec_cmds::refresh_wall_after_axis_edit(
                        &mut self.tabs[i].scene,
                        axis_handle,
                        Some(&style_library),
                        display_rules.as_ref(),
                        style_substitutions.as_ref(),
                    );
                    self.reapply_active_display_config_to_wall_packages(i, &[axis_handle]);
                }
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.context_menu = None;
                sel.junction_menu = None;
                drop(sel);
                self.refresh_properties();
                Task::none()
            }

            Message::AecJunctionEditorOpen(axis_handle, end_index) => {
                let i = self.active_tab;
                use crate::modules::aec::commands as aec_cmds;
                let ov = aec_cmds::read_junction_override(&self.tabs[i].scene, axis_handle, end_index)
                    .unwrap_or_default();
                self.aec_junction_editor_target = Some((axis_handle, end_index));
                self.aec_junction_editor_default_style = ov.default_style;
                self.aec_junction_editor_pairs = ov.layer_pairs;
                self.aec_junction_editor_pair_layer_a = None;
                self.aec_junction_editor_pair_wall_b = None;
                self.aec_junction_editor_pair_layer_b = None;
                self.aec_junction_editor_pair_style =
                    crate::modules::aec::engine::join::JoinOverrideStyle::Miter;
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.context_menu = None;
                sel.junction_menu = None;
                drop(sel);
                self.active_modal = Some(super::ModalKind::AecJunctionEditor);
                Task::none()
            }

            Message::AecJunctionEditorClose => {
                self.aec_junction_editor_target = None;
                self.active_modal = None;
                self.reset_modal_geometry();
                Task::none()
            }

            Message::AecJunctionEditorSetDefaultStyle(style) => {
                self.aec_junction_editor_default_style = Some(style);
                Task::none()
            }

            Message::AecJunctionEditorResetDefaultStyle => {
                self.aec_junction_editor_default_style = None;
                Task::none()
            }

            Message::AecJunctionEditorPairLayerAChanged(index, material_id) => {
                self.aec_junction_editor_pair_layer_a = Some((index, material_id));
                Task::none()
            }

            Message::AecJunctionEditorPairWallBChanged(wall_b) => {
                self.aec_junction_editor_pair_wall_b = wall_b;
                self.aec_junction_editor_pair_layer_b = None;
                Task::none()
            }

            Message::AecJunctionEditorPairLayerBChanged(index, material_id) => {
                self.aec_junction_editor_pair_layer_b = Some((index, material_id));
                Task::none()
            }

            Message::AecJunctionEditorPairStyleChanged(style) => {
                self.aec_junction_editor_pair_style = style;
                Task::none()
            }

            Message::AecJunctionEditorAddPair => {
                use crate::modules::aec::engine::join::LayerRef;
                if let Some((layer_a_index, layer_a_id)) = self.aec_junction_editor_pair_layer_a.clone() {
                    let layer_b =
                        self.aec_junction_editor_pair_layer_b.clone().map(|(index, id)| LayerRef {
                            material_id: id,
                            role_tag: None,
                            index,
                            // The junction editor selects layers by material/index only;
                            // it doesn't track stable layer identity.
                            layer_id: None,
                        });
                    self.aec_junction_editor_pairs.push(
                        crate::modules::aec::engine::join::LayerPairOverride {
                            layer_a: LayerRef {
                                material_id: layer_a_id,
                                role_tag: None,
                                index: layer_a_index,
                                layer_id: None,
                            },
                            layer_b,
                            style: self.aec_junction_editor_pair_style.clone(),
                        },
                    );
                    self.aec_junction_editor_pair_layer_a = None;
                    self.aec_junction_editor_pair_wall_b = None;
                    self.aec_junction_editor_pair_layer_b = None;
                }
                Task::none()
            }

            Message::AecJunctionEditorRemovePair(idx) => {
                if idx < self.aec_junction_editor_pairs.len() {
                    self.aec_junction_editor_pairs.remove(idx);
                }
                Task::none()
            }

            Message::AecJunctionEditorSave => {
                if let Some((axis_handle, end_index)) = self.aec_junction_editor_target {
                    let i = self.active_tab;
                    use crate::modules::aec::commands as aec_cmds;
                    let override_data = crate::modules::aec::engine::join::JunctionOverride {
                        default_style: self.aec_junction_editor_default_style.clone(),
                        layer_pairs: self.aec_junction_editor_pairs.clone(),
                    };
                    if override_data.default_style.is_none() && override_data.layer_pairs.is_empty() {
                        aec_cmds::remove_junction_override(&mut self.tabs[i].scene, axis_handle, end_index);
                    } else {
                        aec_cmds::write_junction_override(
                            &mut self.tabs[i].scene,
                            axis_handle,
                            end_index,
                            &override_data,
                        );
                    }
                    let style_library =
                        crate::modules::aec::engine::project::resolve_style_library(
                            self.aec_project_explorer_file.as_ref(),
                        );
                    let (display_rules, style_substitutions) =
                        self.resolve_active_display_config_wall_rules(i, Some(axis_handle));
                    aec_cmds::refresh_wall_after_axis_edit(
                        &mut self.tabs[i].scene,
                        axis_handle,
                        Some(&style_library),
                        display_rules.as_ref(),
                        style_substitutions.as_ref(),
                    );
                    self.reapply_active_display_config_to_wall_packages(i, &[axis_handle]);
                    self.refresh_properties();
                }
                self.aec_junction_editor_target = None;
                self.active_modal = None;
                self.reset_modal_geometry();
                Task::none()
            }

            Message::AecJunctionEditorFullReset => {
                if let Some((axis_handle, end_index)) = self.aec_junction_editor_target {
                    let i = self.active_tab;
                    use crate::modules::aec::commands as aec_cmds;
                    aec_cmds::remove_junction_override(&mut self.tabs[i].scene, axis_handle, end_index);
                    let style_library =
                        crate::modules::aec::engine::project::resolve_style_library(
                            self.aec_project_explorer_file.as_ref(),
                        );
                    let (display_rules, style_substitutions) =
                        self.resolve_active_display_config_wall_rules(i, Some(axis_handle));
                    aec_cmds::refresh_wall_after_axis_edit(
                        &mut self.tabs[i].scene,
                        axis_handle,
                        Some(&style_library),
                        display_rules.as_ref(),
                        style_substitutions.as_ref(),
                    );
                    self.reapply_active_display_config_to_wall_packages(i, &[axis_handle]);
                    self.refresh_properties();
                }
                self.aec_junction_editor_target = None;
                self.active_modal = None;
                self.reset_modal_geometry();
                Task::none()
            }

            Message::DrawOrderPickRef(above) => {
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().context_menu = None;
                let to_move: Vec<_> = self.tabs[i].scene.selected.iter().cloned().collect();
                if to_move.is_empty() {
                    self.command_line
                        .push_error(crate::t!("DRAWORDER: select entities first.").as_ref());
                } else {
                    use crate::command::CadCommand;
                    let cmd = super::commands::DrawOrderCommand::for_reference_pick(to_move, above);
                    self.command_line.push_info(&cmd.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(cmd));
                }
                Task::none()
            }

            Message::SelectSimilar => {
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().context_menu = None;
                let added = self.tabs[i].scene.select_similar();
                self.command_line
                    .push_output(crate::tf!("Select Similar: {} added.", added).as_ref());
                self.refresh_properties();
                Task::none()
            }

            Message::InvertSelection => {
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().context_menu = None;
                let count = self.tabs[i].scene.invert_selection();
                self.command_line
                    .push_output(crate::tf!("Invert Selection: {} object(s) selected.", count).as_ref());
                self.refresh_properties();
                Task::none()
            }

            Message::QSelectOpen => self.on_qselect_open(),

            Message::QSelectClose => {
                self.qselect = None;
                self.reset_modal_geometry();
                Task::none()
            }

            Message::QSelectSetScope(scope) => {
                let i = self.active_tab;
                let available_types = self.tabs[i].scene.qselect_entity_type_names(scope);
                let type_filter = self.qselect.as_ref().and_then(|state| {
                    state
                        .type_filter
                        .as_ref()
                        .filter(|selected| available_types.iter().any(|item| item == *selected))
                        .cloned()
                });
                let available_properties = self.tabs[i]
                    .scene
                    .qselect_properties(type_filter.as_deref(), scope);
                let candidate_count = self.tabs[i].scene.qselect_candidate_count(scope);
                if let Some(state) = self.qselect.as_mut() {
                    state.scope = scope;
                    state.available_types = available_types;
                    state.available_properties = available_properties;
                    state.candidate_count = candidate_count;
                    state.type_filter = type_filter;
                    state.property = state.property.as_ref().and_then(|selected| {
                        state
                            .available_properties
                            .iter()
                            .find(|available| available.field == selected.field)
                            .cloned()
                    });
                    state.value.clear();
                    state.error = None;
                    if matches!(scope, crate::app::QSelectScope::CurrentSelection) {
                        state.append = false;
                    }
                    if matches!(state.operator, crate::app::QSelectOp::Gt | crate::app::QSelectOp::Lt)
                        && !state.property.as_ref().is_some_and(|property| {
                            matches!(property.editor, crate::app::QSelectValueEditor::Number)
                        })
                    {
                        state.operator = crate::app::QSelectOp::Eq;
                    }
                }
                Task::none()
            }

            Message::QSelectSetType(t) => {
                let i = self.active_tab;
                let scope = self
                    .qselect
                    .as_ref()
                    .map_or(crate::app::QSelectScope::CurrentSpace, |state| state.scope);
                let properties = self.tabs[i]
                    .scene
                    .qselect_properties(t.as_deref(), scope);
                if let Some(state) = self.qselect.as_mut() {
                    let kept_property = state.property.as_ref().and_then(|selected| {
                        properties
                            .iter()
                            .find(|available| available.field == selected.field)
                            .cloned()
                    });
                    state.type_filter = t;
                    state.available_properties = properties;
                    state.property = kept_property;
                    state.value.clear();
                    state.error = None;
                    if matches!(state.operator, crate::app::QSelectOp::Gt | crate::app::QSelectOp::Lt)
                        && !state.property.as_ref().is_some_and(|property| {
                            matches!(property.editor, crate::app::QSelectValueEditor::Number)
                        })
                    {
                        state.operator = crate::app::QSelectOp::Eq;
                    }
                }
                Task::none()
            }

            Message::QSelectSetProperty(p) => {
                if let Some(state) = self.qselect.as_mut() {
                    state.property = p;
                    state.value.clear();
                    state.error = None;
                    if matches!(state.operator, crate::app::QSelectOp::Gt | crate::app::QSelectOp::Lt)
                        && !state.property.as_ref().is_some_and(|property| {
                            matches!(property.editor, crate::app::QSelectValueEditor::Number)
                        })
                    {
                        state.operator = crate::app::QSelectOp::Eq;
                    }
                }
                Task::none()
            }

            Message::QSelectSetOperator(op) => {
                if let Some(state) = self.qselect.as_mut() {
                    state.operator = op;
                    state.error = None;
                }
                Task::none()
            }

            Message::QSelectSetValue(v) => {
                if let Some(state) = self.qselect.as_mut() {
                    state.value = v;
                    state.error = None;
                }
                Task::none()
            }

            Message::QSelectSetMode(mode) => {
                if let Some(state) = self.qselect.as_mut() {
                    state.mode = mode;
                    state.error = None;
                }
                Task::none()
            }

            Message::QSelectSetAppend(b) => {
                if let Some(state) = self.qselect.as_mut() {
                    if matches!(state.scope, crate::app::QSelectScope::CurrentSpace) {
                        state.append = b;
                    }
                    state.error = None;
                }
                Task::none()
            }

            Message::QSelectApply => {
                let validation_error = self.qselect.as_ref().and_then(|state| {
                    let candidate_count = self.tabs[self.active_tab]
                        .scene
                        .qselect_candidate_count(state.scope);
                    if candidate_count == 0 {
                        Some(crate::t!("No objects are available in this scope.").into_owned())
                    } else if let Some(property) = state.property.as_ref() {
                        if matches!(state.operator, crate::app::QSelectOp::Any) {
                            None
                        } else if matches!(
                            state.operator,
                            crate::app::QSelectOp::Gt | crate::app::QSelectOp::Lt
                        ) && !matches!(
                            property.editor,
                            crate::app::QSelectValueEditor::Number
                        ) {
                            Some(
                                crate::t!("This operator requires a numeric property.")
                                    .into_owned(),
                            )
                        } else {
                            match &property.editor {
                                crate::app::QSelectValueEditor::Number
                                    if crate::entities::common::parse_f64(&state.value).is_none() =>
                                {
                                    Some(crate::t!("Enter a valid number.").into_owned())
                                }
                                crate::app::QSelectValueEditor::Choice(_)
                                    if state.value.is_empty() =>
                                {
                                    Some(crate::t!("Choose a value.").into_owned())
                                }
                                crate::app::QSelectValueEditor::Text
                                | crate::app::QSelectValueEditor::Number
                                | crate::app::QSelectValueEditor::Choice(_) => None,
                            }
                        }
                    } else {
                        None
                    }
                });
                if let Some(error) = validation_error {
                    if let Some(state) = self.qselect.as_mut() {
                        state.error = Some(error);
                    }
                    return Task::none();
                }
                let Some(state) = self.qselect.take() else {
                    return Task::none();
                };
                self.reset_modal_geometry();
                let i = self.active_tab;
                let matched = self.tabs[i].scene.qselect(
                    state.scope,
                    state.type_filter.as_deref(),
                    state.property.as_ref().map(|p| p.field.as_str()),
                    state.operator,
                    &state.value,
                    state.mode,
                    state.append
                        && matches!(state.scope, crate::app::QSelectScope::CurrentSpace),
                );
                self.command_line
                    .push_output(crate::tf!("QSELECT: {} object(s) selected.", matched).as_ref());
                self.refresh_properties();
                Task::none()
            }

            // ── Properties panel messages ─────────────────────────────────
            Message::PropSelectionGroupChanged(group) => {
                self.tabs[self.active_tab].properties.selected_group = Some(group);
                self.refresh_properties();
                Task::none()
            }

            Message::RibbonLayerChanged(layer) => self.on_ribbon_layer_changed(layer),

            Message::RibbonColorChanged(color) => self.on_ribbon_color_changed(color),
            Message::RibbonColorPaletteToggle => {
                self.ribbon.prop_color_palette_open ^= true;
                Task::none()
            }
            Message::RibbonLinetypeChanged(lt) => self.on_ribbon_linetype_changed(lt),
            Message::RibbonLineweightChanged(lw) => {
                let i = self.active_tab;
                self.ribbon.close_dropdown();
                let handles = self.property_target_handles(i);
                if handles.is_empty() {
                    if self.has_property_selection(i) {
                        return Task::none();
                    }
                    // Persist into the tab's header (CELWEIGHT). #21.
                    self.tabs[i].scene.document.header.current_line_weight = lw.value();
                    self.tabs[i].dirty = true;
                    self.ribbon.active_lineweight = lw;
                } else {
                    // Lineweight is baked into the cached wire geometry —
                    // re-tessellate so the change shows immediately (issue #231
                    // class).
                    self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                        if let Some(entity) =
                            app.tabs[i].scene.document.get_entity_mut(handle)
                        {
                            crate::scene::view::dispatch::apply_line_weight(entity, lw);
                        }
                    });
                    self.ribbon.active_lineweight = lw;
                }
                Task::none()
            }

            Message::RibbonStyleChanged { key, name } => self.on_ribbon_style_changed(key, name),

            Message::PropLayerChanged(layer) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if handles.is_empty() {
                    let task = self.on_ribbon_layer_changed(layer);
                    self.refresh_properties();
                    return task;
                }
                self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                    if let Some(entity) =
                        app.tabs[i].scene.document.get_entity_mut(handle)
                    {
                        crate::scene::view::dispatch::apply_common_prop(
                            entity, "layer", &layer,
                        );
                    }
                });
                Task::none()
            }

            Message::PropColorChanged(color) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if handles.is_empty() {
                    self.tabs[i].properties.color_palette_open = false;
                    let task = self.on_ribbon_color_changed(color);
                    self.refresh_properties();
                    return task;
                }
                self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                    if let Some(entity) =
                        app.tabs[i].scene.document.get_entity_mut(handle)
                    {
                        crate::scene::view::dispatch::apply_color(entity, color);
                    }
                });
                self.tabs[i].properties.color_picker_open = false;
                self.tabs[i].properties.color_palette_open = false;
                Task::none()
            }

            Message::PropLwChanged(lw) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if handles.is_empty() {
                    if self.has_property_selection(i) {
                        return Task::none();
                    }
                    self.tabs[i].scene.document.header.current_line_weight = lw.value();
                    self.tabs[i].dirty = true;
                    self.ribbon.active_lineweight = lw;
                    self.refresh_properties();
                    return Task::none();
                }
                self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                    if let Some(entity) =
                        app.tabs[i].scene.document.get_entity_mut(handle)
                    {
                        crate::scene::view::dispatch::apply_line_weight(entity, lw);
                    }
                });
                Task::none()
            }

            Message::PropLinetypeChanged(lt) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if handles.is_empty() {
                    let task = self.on_ribbon_linetype_changed(lt);
                    self.refresh_properties();
                    return task;
                }
                self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                    if let Some(entity) =
                        app.tabs[i].scene.document.get_entity_mut(handle)
                    {
                        crate::scene::view::dispatch::apply_common_prop(
                            entity, "linetype", &lt,
                        );
                    }
                });
                Task::none()
            }

            Message::PropHatchPatternChanged(name) => {
                let panel = &mut self.tabs[self.active_tab].properties;
                panel.hatch_pattern_picker_open = false;
                panel.hatch_pattern_search.clear();
                self.on_prop_hatch_pattern_changed(name)
            }

            Message::PropHatchPatternPickerToggle(current) => {
                let panel = &mut self.tabs[self.active_tab].properties;
                panel.hatch_pattern_picker_open = !panel.hatch_pattern_picker_open;
                if panel.hatch_pattern_picker_open {
                    panel.color_picker_open = false;
                    panel.color_palette_open = false;
                    panel.open_color_field = None;
                    panel.edit_choice_open = false;
                    panel.hatch_pattern_focus =
                        crate::ui::properties::filtered_hatch_patterns("")
                            .iter()
                            .position(|entry| entry.name.eq_ignore_ascii_case(&current))
                            .unwrap_or(0);
                    return iced::widget::operation::focus(iced::widget::Id::new(
                        "hatch-pattern-search",
                    ));
                } else {
                    panel.hatch_pattern_search.clear();
                    panel.hatch_pattern_focus = 0;
                }
                Task::none()
            }

            Message::PropHatchPatternSearchChanged(search) => {
                let panel = &mut self.tabs[self.active_tab].properties;
                panel.hatch_pattern_search = search;
                panel.hatch_pattern_focus = 0;
                Task::none()
            }

            Message::PropHatchPatternFocus(index) => {
                let panel = &mut self.tabs[self.active_tab].properties;
                let len =
                    crate::ui::properties::filtered_hatch_patterns(&panel.hatch_pattern_search)
                        .len();
                if index < len {
                    panel.hatch_pattern_focus = index;
                }
                Task::none()
            }

            Message::PropHatchPatternNavigate(delta) => {
                let panel = &mut self.tabs[self.active_tab].properties;
                let len =
                    crate::ui::properties::filtered_hatch_patterns(&panel.hatch_pattern_search)
                        .len();
                if len > 0 {
                    panel.hatch_pattern_focus =
                        (panel.hatch_pattern_focus as isize + delta as isize)
                            .rem_euclid(len as isize) as usize;
                }
                Task::none()
            }

            Message::PropHatchPatternConfirm => {
                let panel = &self.tabs[self.active_tab].properties;
                let name =
                    crate::ui::properties::filtered_hatch_patterns(&panel.hatch_pattern_search)
                        .get(panel.hatch_pattern_focus)
                        .map(|entry| entry.name.clone());
                if let Some(name) = name {
                    self.update(Message::PropHatchPatternChanged(name))
                } else {
                    Task::none()
                }
            }

            Message::PropBoolToggle(field) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if !handles.is_empty() {
                    self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                        match field {
                            // Per-object annotative toggle: MTEXT/MULTILEADER carry
                            // a native flag; single-line TEXT is annotative purely
                            // by the presence of a per-object context. A doc-aware
                            // toggle so turning it on synthesizes a real per-scale
                            // representation and turning it off removes it (not just
                            // the flag).
                            "is_annotative" | "enable_annotation_scale" | "annotative_ctx" => {
                                let doc = &app.tabs[i].scene.document;
                                let cur = match doc.get_entity(handle) {
                                    Some(acadrust::EntityType::MText(t)) => t.is_annotative,
                                    Some(acadrust::EntityType::MultiLeader(m)) => {
                                        m.enable_annotation_scale
                                    }
                                    // TEXT (and any other context-only type): its
                                    // annotative state is whether a context exists.
                                    Some(e) => crate::scene::annotative::is_annotative(doc, e),
                                    None => return,
                                };
                                crate::scene::annotative::set_entity_annotative(
                                    &mut app.tabs[i].scene.document,
                                    handle,
                                    !cur,
                                );
                                // Turning it on also gives the object a real
                                // per-scale representation at the current
                                // annotation scale (not just the native flag),
                                // so it interoperates as a genuine annotative
                                // object. Off is handled inside set_entity_*.
                                if !cur {
                                    if let Some(sh) =
                                        app.tabs[i].scene.creation_annotation_scale_handle()
                                    {
                                        crate::scene::annotative::create_annotation_context(
                                            &mut app.tabs[i].scene.document,
                                            handle,
                                            sh,
                                        );
                                    }
                                }
                            }
                            "invisible" => {
                                if let Some(entity) =
                                    app.tabs[i].scene.document.get_entity_mut(handle)
                                {
                                    crate::scene::view::dispatch::toggle_invisible(entity);
                                }
                            }
                            // Uniform-scale checkbox on a block reference
                            // (#427): checking collapses Y/Z onto X; unchecking
                            // only switches the panel to per-axis rows.
                            "ins_uniform" => {
                                let scales = match app.tabs[i].scene.document.get_entity(handle) {
                                    Some(acadrust::EntityType::Insert(ins)) => {
                                        Some((ins.x_scale(), ins.y_scale(), ins.z_scale()))
                                    }
                                    _ => None,
                                };
                                let Some((sx, sy, sz)) = scales else { return };
                                let eq = (sx - sy).abs() < 1e-12 && (sx - sz).abs() < 1e-12;
                                let checked =
                                    eq && !app.props_asym_scale.contains(&handle.value());
                                if checked {
                                    app.props_asym_scale.insert(handle.value());
                                } else {
                                    app.props_asym_scale.remove(&handle.value());
                                    if let Some(acadrust::EntityType::Insert(ins)) =
                                        app.tabs[i].scene.document.get_entity_mut(handle)
                                    {
                                        ins.set_y_scale(sx);
                                        ins.set_z_scale(sx);
                                    }
                                }
                            }
                            "tbl_title_suppressed" | "tbl_header_suppressed" => {
                                let next = {
                                    let document = &app.tabs[i].scene.document;
                                    let Some(acadrust::EntityType::Table(table)) =
                                        document.get_entity(handle)
                                    else {
                                        return;
                                    };
                                    let table_style = table.table_style_handle.and_then(|style_handle| {
                                        document.objects.get(&style_handle).and_then(|object| {
                                            match object {
                                                acadrust::objects::ObjectType::TableStyle(style) => {
                                                    Some(style)
                                                }
                                                _ => None,
                                            }
                                        })
                                    });
                                    let current = if field == "tbl_title_suppressed" {
                                        crate::entities::table::resolved_title_suppressed(
                                            table,
                                            table_style,
                                        )
                                    } else {
                                        crate::entities::table::resolved_header_suppressed(
                                            table,
                                            table_style,
                                        )
                                    };
                                    !current
                                };
                                if let Some(entity) =
                                    app.tabs[i].scene.document.get_entity_mut(handle)
                                {
                                    crate::scene::view::dispatch::apply_geom_prop(
                                        entity,
                                        field,
                                        if next { "true" } else { "false" },
                                    );
                                }
                            }
                            "wall_hatch_override_enabled" => {
                                // Toggling this checkbox on synthesizes a
                                // fresh override at angle 0/relative so the
                                // angle/relative rows appear editable;
                                // toggling it off clears the override
                                // entirely (falls back to the style-profile/
                                // material tiers of the hatch-angle chain).
                                let has_override = crate::modules::aec::commands::wall_from_entity(
                                    app.tabs[i].scene.document.get_entity(handle).unwrap(),
                                )
                                .map(|w| w.hatch_override.is_some())
                                .unwrap_or(false);
                                let next_override = if has_override {
                                    None
                                } else {
                                    Some(crate::modules::aec::engine::display_component::ComponentStyleOverride {
                                        hatch_angle: Some(0.0),
                                        hatch_angle_relative: Some(true),
                                        ..Default::default()
                                    })
                                };
                                crate::modules::aec::commands::write_wall_hatch_override(
                                    &mut app.tabs[i].scene,
                                    handle,
                                    next_override,
                                );
                            }
                            "wall_hatch_relative" => {
                                let Some(wall) = crate::modules::aec::commands::wall_from_entity(
                                    app.tabs[i].scene.document.get_entity(handle).unwrap(),
                                ) else {
                                    return;
                                };
                                let mut ov = wall.hatch_override.unwrap_or_default();
                                ov.hatch_angle_relative = Some(!ov.hatch_angle_relative.unwrap_or(true));
                                crate::modules::aec::commands::write_wall_hatch_override(
                                    &mut app.tabs[i].scene,
                                    handle,
                                    Some(ov),
                                );
                            }
                            _ => {
                                if let Some(entity) =
                                    app.tabs[i].scene.document.get_entity_mut(handle)
                                {
                                    crate::scene::view::dispatch::apply_geom_prop(
                                        entity, field, "toggle",
                                    );
                                }
                            }
                        }
                        if field.starts_with("tbl_") {
                            if let Some(acadrust::EntityType::Table(table)) =
                                app.tabs[i].scene.document.get_entity_mut(handle)
                            {
                                table.block_record_handle = None;
                            }
                        }
                    });
                }
                Task::none()
            }

            Message::PropVertexStep(delta) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                // Vertex navigation applies to a single selected polyline.
                let n = if handles.len() == 1 {
                    match self.tabs[i].scene.document.get_entity(handles[0]) {
                        Some(acadrust::EntityType::LwPolyline(p)) => p.vertices.len(),
                        Some(acadrust::EntityType::Polyline2D(p)) => p.vertices.len(),
                        Some(acadrust::EntityType::Table(table)) => {
                            table.row_count().saturating_mul(table.column_count())
                        }
                        _ => 0,
                    }
                } else {
                    0
                };
                if n > 0 {
                    let cur = self.tabs[i].properties.prop_vertex.min(n - 1) as i64;
                    // Wrap around so ◀ from the first vertex lands on the last.
                    let next = (cur + delta as i64).rem_euclid(n as i64) as usize;
                    self.tabs[i].properties.prop_vertex = next;
                    self.tabs[i].properties.prop_vertex_indicator_active = next != cur as usize;
                    crate::entities::table::set_prop_current_cell(next);
                    self.refresh_properties();
                }
                Task::none()
            }

            Message::PropGeomChoiceChanged { field, value } => {
                self.on_prop_geom_choice_changed(field, value)
            }

            Message::PropGeomInput { field, value } => {
                self.tabs[self.active_tab]
                    .properties
                    .edit_buf
                    .insert(crate::ui::properties::FieldKey::Geom(field), value);
                Task::none()
            }

            Message::PropGeomCommit(field) => self.on_prop_geom_commit(field),

            Message::PropGroupToggle(key) => {
                let groups = &mut self.tabs[self.active_tab].properties.expanded_groups;
                if !groups.remove(&key) {
                    groups.insert(key);
                }
                Task::none()
            }

            Message::PropEditChoiceToggle => {
                let panel = &mut self.tabs[self.active_tab].properties;
                panel.edit_choice_open = !panel.edit_choice_open;
                if panel.edit_choice_open {
                    panel.hatch_pattern_picker_open = false;
                    panel.hatch_pattern_search.clear();
                }
                Task::none()
            }

            Message::PropAttrInput { tag, value } => {
                self.tabs[self.active_tab]
                    .properties
                    .edit_buf
                    .insert(crate::ui::properties::attr_edit_key(&tag), value);
                Task::none()
            }

            Message::PropAttrCommit(tag) => self.on_prop_attr_commit(tag),

            Message::PropPointerPressed => {
                if !self.dock_panel_visible(crate::ui::dock::PanelId::Properties) {
                    return Task::none();
                }
                crate::ui::properties::sync_active_field_task()
            }

            Message::PropSyncActive(focused) => {
                let panel = &mut self.tabs[self.active_tab].properties;
                if let Some(id) = focused.as_ref() {
                    if let Some(key) = panel.prop_field_key_for_id(id) {
                        let changed = panel.active_field.as_ref() != Some(&key);
                        panel.active_field = Some(key);
                        // Only select the whole value when focus landed on a
                        // field that wasn't already the active one; re-focusing
                        // the same field (or sweeping after a click elsewhere
                        // left its focus in place) must keep caret placement.
                        if changed {
                            return iced::widget::operation::select_all(id.clone());
                        }
                        return Task::none();
                    }
                }
                if !crate::ui::properties::active_key_focused(
                    panel.active_field.as_ref(),
                    focused.as_ref(),
                ) {
                    panel.active_field = None;
                }
                Task::none()
            }

            Message::PropColorPickerToggle => {
                let i = self.active_tab;
                self.tabs[i].properties.color_picker_open =
                    !self.tabs[i].properties.color_picker_open;
                if self.tabs[i].properties.color_picker_open {
                    self.tabs[i].properties.color_palette_open = false;
                    self.tabs[i].properties.hatch_pattern_picker_open = false;
                    self.tabs[i].properties.hatch_pattern_search.clear();
                }
                Task::none()
            }

            Message::PropBgColorPickerToggle => {
                let i = self.active_tab;
                self.tabs[i].properties.bg_color_picker_open =
                    !self.tabs[i].properties.bg_color_picker_open;
                Task::none()
            }

            Message::PropBgColorChanged(color) => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                if !handles.is_empty() {
                    self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                        match app.tabs[i].scene.document.get_entity_mut(handle) {
                            Some(acadrust::EntityType::MText(m)) => {
                                m.background_color = color.clone();
                                // Picking a colour turns the background on in Fill
                                // mode (specific colour), preserving the frame bit.
                                m.background_fill_flags =
                                    (m.background_fill_flags & !0x02) | 0x01;
                            }
                            Some(acadrust::EntityType::Hatch(h)) => {
                                crate::entities::hatch::set_background_color(h, &color);
                            }
                            _ => {}
                        }
                    });
                    self.tabs[i].properties.bg_color_picker_open = false;
                }
                Task::none()
            }

            Message::PropColorFieldToggle(field) => {
                let i = self.active_tab;
                let p = &mut self.tabs[i].properties;
                p.open_color_field = if p.open_color_field.as_deref() == Some(field.as_str()) {
                    None
                } else {
                    Some(field)
                };
                Task::none()
            }

            Message::PropColorFieldChanged { field, color } => {
                let i = self.active_tab;
                let handles = self.property_target_handles(i);
                // Dim-line colour override (Leader / Dimension): write it as an
                // ACAD_DSTYLE code-176 override (an ACI index) so it round-trips
                // through DWG and DXF. RGB picks collapse to the nearest ACI, in
                // line with the rest of the dim-colour stack (index-only through
                // the file layer). Guarded to leaders / dimensions so a mixed
                // selection can't stamp the override onto other entities.
                if matches!(
                    field.as_str(),
                    "dim_line_color"
                        | "dim_ext_line_color"
                        | "dim_text_color"
                        | "dim_text_fill_color"
                ) {
                    let fill_mode = (field == "dim_text_fill_color").then(|| match color {
                        acadrust::types::Color::None => 0,
                        acadrust::types::Color::ByBlock => 1,
                        _ => 2,
                    });
                    let aci = color.approximate_index();
                    let code = match field.as_str() {
                        "dim_ext_line_color" => crate::entities::dim_override::DIMCLRE,
                        "dim_text_color" => crate::entities::dim_override::DIMCLRT,
                        "dim_text_fill_color" => crate::entities::dim_override::DIMTFILLCLR,
                        _ => crate::entities::dim_override::DIMCLRD,
                    };
                    let targets: Vec<acadrust::Handle> = handles
                        .iter()
                        .copied()
                        .filter(|&h| {
                            matches!(
                                self.tabs[i].scene.document.get_entity(h),
                                Some(acadrust::EntityType::Leader(_))
                                    | Some(acadrust::EntityType::Dimension(_))
                            )
                        })
                        .collect();
                    if !targets.is_empty() {
                        self.apply_property_op(i, "CHPROP", &targets, |app, handle| {
                            if field == "dim_text_fill_color" {
                                crate::entities::dim_override::set(
                                    &mut app.tabs[i].scene.document,
                                    handle,
                                    crate::entities::dim_override::DIMTFILL,
                                    Some(acadrust::xdata::XDataValue::Integer16(
                                        fill_mode.unwrap_or(2),
                                    )),
                                );
                                if fill_mode != Some(2) {
                                    return;
                                }
                            }
                            crate::entities::dim_override::set(
                                &mut app.tabs[i].scene.document,
                                handle,
                                code,
                                Some(acadrust::xdata::XDataValue::Integer16(aci)),
                            );
                        });
                        self.tabs[i].properties.open_color_field = None;
                    }
                    return Task::none();
                }
                if matches!(
                    field.as_str(),
                    "tbl_cell_content_color" | "tbl_cell_background_color"
                ) {
                    let cell_index = self.tabs[i].properties.prop_vertex;
                    if !handles.is_empty() {
                        self.apply_property_op(i, "TABLE CELL COLOR", &handles, |app, handle| {
                            let Some(acadrust::EntityType::Table(table)) =
                                app.tabs[i].scene.document.get_entity_mut(handle)
                            else {
                                return;
                            };
                            let columns = table.column_count();
                            if columns == 0 {
                                return;
                            }
                            if let Some(cell) = table.cell_mut(
                                cell_index / columns,
                                cell_index % columns,
                            ) {
                                use acadrust::entities::table::CellStateFlags;
                                if cell.state.intersects(
                                    CellStateFlags::FORMAT_LOCKED
                                        | CellStateFlags::FORMAT_READ_ONLY,
                                ) {
                                    return;
                                }
                                let style = cell.style.get_or_insert_with(Default::default);
                                if field == "tbl_cell_content_color" {
                                    style.content_color = color;
                                    style.property_flags.insert(
                                        acadrust::entities::table::CellStylePropertyFlags::CONTENT_COLOR,
                                    );
                                } else {
                                    style.background_color = color;
                                    style.fill_enabled = true;
                                    style.property_flags.insert(
                                        acadrust::entities::table::CellStylePropertyFlags::BACKGROUND_COLOR,
                                    );
                                }
                            }
                            table.block_record_handle = None;
                        });
                        self.tabs[i].properties.open_color_field = None;
                    }
                    return Task::none();
                }
                if !handles.is_empty() {
                    let idx = if field == "gradient_color_2" { 1 } else { 0 };
                    self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
                        if let Some(acadrust::EntityType::Hatch(h)) =
                            app.tabs[i].scene.document.get_entity_mut(handle)
                        {
                            while h.gradient_color.colors.len() <= idx {
                                let value = if h.gradient_color.colors.is_empty() {
                                    0.0
                                } else {
                                    1.0
                                };
                                h.gradient_color.colors.push(
                                    acadrust::entities::hatch::GradientColorEntry {
                                        value,
                                        color: acadrust::types::Color::Index(7),
                                    },
                                );
                            }
                            h.gradient_color.colors[idx].color = color.clone();
                        }
                    });
                    // Rebuild hatch seeds so the gradient fill picks up the new
                    // colour (synced_hatch_models only patches the main colour).
                    self.tabs[i].scene.populate_hatches_from_document();
                    self.tabs[i].properties.open_color_field = None;
                }
                Task::none()
            }

            Message::PropColorPickerClose => {
                let i = self.active_tab;
                self.tabs[i].properties.color_picker_open = false;
                self.tabs[i].properties.color_palette_open = false;
                self.tabs[i].properties.hatch_pattern_picker_open = false;
                self.tabs[i].properties.hatch_pattern_search.clear();
                Task::none()
            }

            Message::PropColorPaletteToggle => {
                self.tabs[self.active_tab].properties.color_palette_open =
                    !self.tabs[self.active_tab].properties.color_palette_open;
                Task::none()
            }

            Message::LayoutSwitch(name) => {
                self.layout_list_open = false;
                self.on_layout_switch(name)
            }

            Message::BlockEditSwitch(name) => {
                self.layout_list_open = false;
                self.on_block_edit_switch(name)
            }

            Message::LayoutReorder { from, to, after } => {
                let i = self.active_tab;
                if self.tabs[i].is_start {
                    return Task::none();
                }
                let mut paper: Vec<String> = self.tabs[i]
                    .scene
                    .layout_names()
                    .into_iter()
                    .skip(1)
                    .collect();
                let Some(from_index) = paper.iter().position(|name| name == &from) else {
                    return Task::none();
                };
                let Some(to_index) = paper.iter().position(|name| name == &to) else {
                    return Task::none();
                };
                let Some(insertion) =
                    reorder_insertion_index(from_index, to_index, after, paper.len())
                else {
                    return Task::none();
                };

                let moved = paper.remove(from_index);
                paper.insert(insertion, moved);
                self.push_undo_snapshot(i, "LAYOUT REORDER");
                self.tabs[i].scene.set_layout_tab_order(&paper);
                self.tabs[i].dirty = true;
                Task::none()
            }

            Message::LayoutCreate => self.on_layout_create(),

            Message::LayoutDelete(name) => {
                let i = self.active_tab;
                let deleting_current = self.tabs[i].scene.current_layout == name;
                let cancel_task = if deleting_current {
                    self.cancel_active_command_for_space_change()
                } else {
                    Task::none()
                };
                self.push_undo_snapshot(i, "LAYOUT DEL");
                let switch_task = if deleting_current {
                    self.on_layout_switch("Model".to_string())
                } else {
                    Task::none()
                };
                if self.tabs[i].scene.delete_layout(&name) {
                    self.layout_rename_state = None;
                    self.command_line
                        .push_output(crate::tf!("Layout \"{name}\" silindi").as_ref());
                    self.tabs[i].dirty = true;
                }
                Task::batch([cancel_task, switch_task])
            }

            Message::LayoutRenameStart(name) => {
                if name != "Model" {
                    self.layout_rename_state = Some((name.clone(), name));
                    // Focus the inline field so the user types into it
                    // directly instead of the command line (issue #86).
                    return iced::widget::operation::focus(iced::widget::Id::new(
                        crate::ui::statusbar::LAYOUT_RENAME_INPUT_ID,
                    ));
                }
                Task::none()
            }

            Message::LayoutRenameEdit(val) => {
                if let Some((orig, _)) = &self.layout_rename_state {
                    let orig = orig.clone();
                    self.layout_rename_state = Some((orig, val));
                }
                Task::none()
            }

            Message::LayoutRenameCommit => self.on_layout_rename_commit(),

            Message::LayoutRenameCancel => {
                self.layout_rename_state = None;
                Task::none()
            }

            // ── Layout Manager Panel ──────────────────────────────────────────
            Message::LayoutManagerOpen => {
                let i = self.active_tab;
                if self.tabs[i].is_start {
                    self.command_line
                        .push_info(crate::t!("Open or create a drawing to manage layouts.").as_ref());
                    return Task::none();
                }
                let current = self.tabs[i].scene.current_layout.clone();
                self.layout_manager_selected = current.clone();
                self.layout_manager_rename_buf = if current == "Model" {
                    String::new()
                } else {
                    current
                };
                self.active_modal = Some(super::ModalKind::LayoutManager);
                Task::none()
            }
            Message::LayoutManagerClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::LayoutManagerSelect(name) => {
                self.layout_manager_rename_buf = if name == "Model" {
                    String::new()
                } else {
                    name.clone()
                };
                self.layout_manager_selected = name;
                Task::none()
            }
            Message::LayoutManagerRenameBuf(s) => {
                self.layout_manager_rename_buf = s;
                Task::none()
            }
            Message::LayoutManagerRenameCommit => {
                let i = self.active_tab;
                let old_name = self.layout_manager_selected.clone();
                let new_name = self.layout_manager_rename_buf.trim().to_string();
                if old_name == "Model" {
                    self.command_line
                        .push_error(crate::t!("Cannot rename the Model layout.").as_ref());
                } else if new_name.is_empty() {
                    self.command_line.push_error(crate::t!("Layout name cannot be empty.").as_ref());
                } else if new_name == old_name {
                    // no-op
                } else {
                    self.push_undo_snapshot(i, "LAYOUT RENAME");
                    self.tabs[i].scene.rename_layout(&old_name, &new_name);
                    if self.tabs[i].scene.current_layout == old_name {
                        self.tabs[i].scene.set_current_layout(new_name.clone());
                    }
                    self.layout_manager_selected = new_name.clone();
                    self.tabs[i].dirty = true;
                    self.command_line
                        .push_output(crate::tf!("Layout renamed: '{old_name}' → '{new_name}'").as_ref());
                }
                Task::none()
            }
            Message::LayoutManagerNew => {
                let i = self.active_tab;
                if self.tabs[i].is_start {
                    self.command_line
                        .push_info(crate::t!("Open or create a drawing to add a layout.").as_ref());
                    return Task::none();
                }
                let existing = self.tabs[i].scene.layout_names();
                let n = (1usize..)
                    .find(|n| !existing.contains(&format!("Layout{n}")))
                    .unwrap_or(1);
                let name = format!("Layout{n}");
                self.push_undo_snapshot(i, "LAYOUT NEW");
                match self.tabs[i].scene.document.add_layout(&name) {
                    Ok(_) => {
                        self.tabs[i].dirty = true;
                        self.layout_manager_selected = name.clone();
                        self.layout_manager_rename_buf = name.clone();
                        self.command_line
                            .push_output(crate::tf!("Layout '{name}' created.").as_ref());
                    }
                    Err(e) => self.command_line.push_error(crate::tf!("LAYOUT: {e}").as_ref()),
                }
                Task::none()
            }
            Message::LayoutManagerDelete => {
                let i = self.active_tab;
                let name = self.layout_manager_selected.clone();
                if name == "Model" {
                    self.command_line
                        .push_error(crate::t!("Cannot delete the Model layout.").as_ref());
                    Task::none()
                } else {
                    let deleting_current = self.tabs[i].scene.current_layout == name;
                    let cancel_task = if deleting_current {
                        self.cancel_active_command_for_space_change()
                    } else {
                        Task::none()
                    };
                    self.push_undo_snapshot(i, "LAYOUT DELETE");
                    let switch_task = if deleting_current {
                        self.on_layout_switch("Model".to_string())
                    } else {
                        Task::none()
                    };
                    self.tabs[i].scene.delete_layout(&name);
                    self.tabs[i].dirty = true;
                    self.layout_manager_selected = "Model".to_string();
                    self.layout_manager_rename_buf = String::new();
                    self.command_line
                        .push_output(crate::tf!("Layout '{name}' deleted.").as_ref());
                    Task::batch([cancel_task, switch_task])
                }
            }
            Message::LayoutManagerMoveLeft => {
                let i = self.active_tab;
                let name = self.layout_manager_selected.clone();
                if name == "Model" {
                    return Task::none();
                }
                let names = self.tabs[i].scene.layout_names();
                // Find position among paper layouts only.
                let paper: Vec<&str> = names.iter().skip(1).map(|s| s.as_str()).collect();
                if let Some(pos) = paper.iter().position(|&n| n == name) {
                    if pos > 0 {
                        self.push_undo_snapshot(i, "LAYOUT REORDER");
                        self.tabs[i].scene.swap_layout_order(&name, paper[pos - 1]);
                        self.tabs[i].dirty = true;
                    }
                }
                Task::none()
            }
            Message::LayoutManagerMoveRight => {
                let i = self.active_tab;
                let name = self.layout_manager_selected.clone();
                if name == "Model" {
                    return Task::none();
                }
                let names = self.tabs[i].scene.layout_names();
                let paper: Vec<&str> = names.iter().skip(1).map(|s| s.as_str()).collect();
                if let Some(pos) = paper.iter().position(|&n| n == name) {
                    if pos + 1 < paper.len() {
                        self.push_undo_snapshot(i, "LAYOUT REORDER");
                        self.tabs[i].scene.swap_layout_order(&name, paper[pos + 1]);
                        self.tabs[i].dirty = true;
                    }
                }
                Task::none()
            }
            Message::LayoutManagerSetCurrent => {
                let name = self.layout_manager_selected.clone();
                let task = self.on_layout_switch(name.clone());
                if self.tabs[self.active_tab].scene.current_layout == name {
                    self.command_line
                        .push_output(crate::tf!("Switched to layout '{name}'.").as_ref());
                }
                task
            }

            Message::SetTheme(theme) => {
                self.ui_theme.name = theme.to_string();
                self.ui_theme.palette =
                    crate::app::config::UiThemePalette::from_iced(theme.seed());
                self.theme_color_inputs = self.ui_theme.palette.hex_values();
                self.active_theme = theme;
                self.persist_settings_if_changed();
                Task::none()
            }

            // ── Keyboard Shortcuts Panel ──────────────────────────────────────
            Message::ShortcutsPanelOpen => {
                let mut rows: Vec<(String, String)> = self
                    .shortcut_bindings
                    .iter()
                    .map(|(key, command)| (key.clone(), command.clone()))
                    .collect();
                rows.sort_by(|a, b| a.0.cmp(&b.0));
                self.shortcut_editor_rows = rows;
                self.active_modal = Some(super::ModalKind::Shortcuts);
                Task::none()
            }
            Message::ShortcutsPanelClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::ShortcutEditorInput { idx, field, value } => {
                use crate::ui::window::shortcuts::ShortcutField;
                if let Some(row) = self.shortcut_editor_rows.get_mut(idx) {
                    match field {
                        ShortcutField::Key => row.0 = value.to_uppercase(),
                        ShortcutField::Command => row.1 = value.to_uppercase(),
                    }
                }
                Task::none()
            }
            Message::ShortcutEditorAdd => {
                self.shortcut_editor_rows
                    .push((String::new(), String::new()));
                Task::none()
            }
            Message::ShortcutEditorRemove(idx) => {
                if idx < self.shortcut_editor_rows.len() {
                    self.shortcut_editor_rows.remove(idx);
                }
                Task::none()
            }
            Message::ShortcutEditorApply => {
                self.apply_shortcut_editor_rows();
                self.command_line.push_info(
                    crate::tf!("{} shortcut(s) applied.", self.shortcut_bindings.len()).as_ref(),
                );
                Task::none()
            }
            Message::ShortcutPressed(key) => self.run_shortcut(&key),

            // ── Command Alias Editor (ALIASEDIT) ──────────────────────────────
            Message::AliasEditorOpen => {
                // Seed the working buffer from the current table, sorted by alias
                // so the list is stable and diffable.
                let mut rows: Vec<(String, String)> = self
                    .command_aliases
                    .iter()
                    .map(|(a, c)| (a.clone(), c.clone()))
                    .collect();
                rows.sort_by(|a, b| a.0.cmp(&b.0));
                self.alias_editor_rows = rows;
                self.active_modal = Some(super::ModalKind::Aliases);
                Task::none()
            }
            Message::AliasEditorInput { idx, field, value } => {
                use crate::ui::window::alias_editor::AliasField;
                // Aliases and commands are stored uppercase; uppercasing as the
                // user types keeps display and the committed table consistent.
                let value = value.to_uppercase();
                if let Some(rowdata) = self.alias_editor_rows.get_mut(idx) {
                    match field {
                        AliasField::Alias => rowdata.0 = value,
                        AliasField::Command => rowdata.1 = value,
                    }
                }
                Task::none()
            }
            Message::AliasEditorAdd => {
                self.alias_editor_rows.push((String::new(), String::new()));
                Task::none()
            }
            Message::AliasEditorRemove(idx) => {
                if idx < self.alias_editor_rows.len() {
                    self.alias_editor_rows.remove(idx);
                }
                Task::none()
            }
            Message::AliasEditorApply => {
                self.apply_alias_editor_rows();
                self.command_line.push_info(crate::tf!(
                    "{} alias(es) applied.",
                    self.command_aliases.len()
                ).as_ref());
                Task::none()
            }

            // ── Options / About windows ───────────────────────────────────
            Message::OptionsOpen => {
                self.active_modal = Some(super::ModalKind::Options);
                Task::none()
            }

            Message::OptionsTabChanged(tab) => {
                self.options_tab = tab;
                Task::none()
            }

            Message::CursorSizeChanged(value) => {
                self.cursor_size = value.clamp(1, 100);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::PickBoxChanged(value) => {
                self.pick_box = value.clamp(0, 50);
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::CursorTypeChanged(value) => {
                self.cursor_type = value;
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::CrosshairColorChanged(value) => {
                self.crosshair_color_input = value.clone();
                if value.trim().is_empty() {
                    self.crosshair_color = None;
                    self.persist_settings_if_changed();
                } else if let Some(rgb) = crate::app::config::parse_hex(&value) {
                    self.crosshair_color = Some(rgb);
                    self.persist_settings_if_changed();
                }
                Task::none()
            }

            Message::DefaultSaveFormatChanged(format) => {
                self.default_save_format =
                    crate::io::canonical_save_format(&format).to_string();
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::OptionsThemeChanged(name) => {
                self.ui_theme.name = name;
                if let Some(theme) =
                    crate::app::config::builtin_theme(&self.ui_theme.name)
                {
                    self.ui_theme.palette =
                        crate::app::config::UiThemePalette::from_iced(theme.seed());
                    self.theme_color_inputs = self.ui_theme.palette.hex_values();
                    self.active_theme = theme;
                } else {
                    self.ui_theme.name = "Custom".to_string();
                    self.active_theme = self.ui_theme.to_iced();
                }
                self.persist_settings_if_changed();
                Task::none()
            }

            Message::OptionsThemeColorChanged(index, value) => {
                if index >= self.theme_color_inputs.len() {
                    return Task::none();
                }
                self.theme_color_inputs[index] = value.clone();
                if self.ui_theme.palette.set_hex(index, &value) {
                    self.ui_theme.name = "Custom".to_string();
                    self.active_theme = self.ui_theme.to_iced();
                    self.persist_settings_if_changed();
                }
                Task::none()
            }

            Message::FileAssocChanged(enabled) => {
                // The same two calls FILEASSOC makes, so the checkbox and the
                // command cannot leave the setting and the registration
                // disagreeing.
                self.file_assoc_enabled = enabled;
                self.persist_settings_if_changed();
                let outcome = if enabled {
                    crate::io::file_association::register_as_handler()
                } else {
                    crate::io::file_association::unregister_handler()
                };
                match outcome {
                    Ok(()) => {
                        let said = if enabled {
                            crate::t!("Registered as a .dwg/.dxf handler.")
                        } else {
                            crate::t!("No longer registered as a file handler.")
                        };
                        self.command_line.push_output(said.as_ref());
                    }
                    Err(why) => {
                        // The setting is what the user asked for; the
                        // registration is what the system allowed. Put the
                        // checkbox back rather than showing a state that is not
                        // true.
                        self.file_assoc_enabled = !enabled;
                        self.persist_settings_if_changed();
                        self.command_line
                            .push_error(crate::tf!("File association failed: {why}").as_ref());
                    }
                }
                Task::none()
            }
            Message::LanguageChanged(language) => {
                if self.language == language {
                    return Task::none();
                }
                match crate::i18n::set_language(language) {
                    Ok(()) => {
                        self.language = language;
                        self.persist_settings_if_changed();
                        #[cfg(target_arch = "wasm32")]
                        {
                            let script = crate::scene::text::web_font::preload_language(
                                &crate::i18n::active_language_tag(),
                            );
                            return Task::batch([
                                Task::done(Message::PollWebFonts),
                                Task::done(Message::ApplyWebFont(script)),
                            ]);
                        }
                    }
                    Err(error) => self
                        .command_line
                        .push_error(crate::tf!("Unable to change UI language: {error}").as_ref()),
                }
                Task::none()
            }

            Message::AboutOpen => {
                self.active_modal = Some(super::ModalKind::About);
                Task::none()
            }

            Message::CloseModal => {
                if self.active_modal == Some(super::ModalKind::RecoveryPrompt) {
                    return self.update(Message::RecoveryDecline);
                }
                let resume_open_queue = self.active_modal == Some(super::ModalKind::Recovery);
                self.close_active_modal();
                if resume_open_queue {
                    self.drain_pending_open()
                } else {
                    Task::none()
                }
            }
            Message::RecoveryClose => {
                self.close_active_modal();
                self.drain_pending_open()
            }
            Message::RecoveryAttempt => {
                let open_id = self.next_open_id();
                let Some(opening) = self.opening.as_mut() else {
                    self.close_active_modal();
                    return Task::none();
                };
                let model_bg = self.default_bg_color.unwrap_or([
                    33.0 / 255.0,
                    40.0 / 255.0,
                    48.0 / 255.0,
                    1.0,
                ]);
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(path) = opening.source_path.clone() {
                    let current_fingerprint =
                        crate::io::edit_lock::FileFingerprint::capture(&path).ok();
                    if current_fingerprint.as_ref() != opening.fingerprint.as_ref() {
                        let progress = std::sync::Arc::new(crate::io::OpenProgressState::new(
                            super::OPEN_PHASE_READING,
                        ));
                        opening.id = open_id;
                        opening.state = progress.clone();
                        opening.started = Instant::now();
                        opening.recovery_error = None;
                        opening.recovery_read_stats = None;
                        opening.fingerprint = current_fingerprint;
                        opening.size_bytes = std::fs::metadata(&path)
                            .map(|metadata| metadata.len())
                            .unwrap_or(0);
                        self.close_active_modal();
                        return Task::perform(
                            crate::io::open_path_with_phase(path, progress, model_bg),
                            move |result| Message::FileOpened(open_id, result),
                        );
                    }
                }
                let Some(initial_error) = opening.recovery_error.take() else {
                    self.close_active_modal();
                    return Task::none();
                };
                let initial_stats = opening.recovery_read_stats.take();
                let Some(path) = opening.source_path.clone() else {
                    self.close_active_modal();
                    return Task::none();
                };
                let progress = std::sync::Arc::new(crate::io::OpenProgressState::new(
                    super::OPEN_PHASE_READING,
                ));
                opening.id = open_id;
                opening.state = progress.clone();
                opening.started = Instant::now();
                #[cfg(target_arch = "wasm32")]
                let recovery_bytes = opening.recovery_bytes.take();
                self.close_active_modal();
                #[cfg(not(target_arch = "wasm32"))]
                {
                    Task::perform(
                        crate::io::recover_path_with_phase(
                            path,
                            progress,
                            model_bg,
                            initial_error,
                            initial_stats,
                        ),
                        move |result| Message::FileOpened(open_id, result),
                    )
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = model_bg;
                    let Some(bytes) = recovery_bytes else {
                        self.opening = None;
                        return self.drain_pending_open();
                    };
                    Task::perform(
                        crate::io::recover_web_bytes(
                            path.to_string_lossy().into_owned(),
                            bytes,
                            progress,
                            initial_error,
                            initial_stats,
                        ),
                        move |outcome| Message::WebFileOpened(open_id, outcome),
                    )
                }
            }
            Message::RecoveryDecline => {
                let declined = self.opening.take();
                self.close_active_modal();
                if let Some(opening) = declined {
                    self.command_line.push_info(crate::tf!(
                        "Recovery cancelled: \"{}\"",
                        opening.name
                    ).as_ref());
                }
                self.drain_pending_open()
            }
            Message::RecoverySaveAs => {
                if !self.pending_opens.is_empty() {
                    self.close_active_modal();
                    return self.drain_pending_open();
                }
                let Some(tab_id) = self
                    .recovery_report
                    .as_ref()
                    .and_then(|report| report.tab_id)
                else {
                    self.close_active_modal();
                    return Task::none();
                };
                let Some(i) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
                    self.close_active_modal();
                    return Task::none();
                };
                self.close_active_modal();
                self.active_tab = i;
                self.open_save_dialog_window(i)
            }
            Message::RecoveryShowLog => {
                let Some(report) = self.recovery_report.as_ref() else {
                    return Task::none();
                };
                #[cfg(not(target_arch = "wasm32"))]
                {
                    if let Some(path) = &report.log_path {
                        if let Err(error) = crate::sys::reveal_in_file_manager(path) {
                            self.command_line.push_error(crate::tf!(
                                "Could not show recovery log: {error}"
                            ).as_ref());
                        }
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let name = report.suggested_download_name();
                    let body = report.log_text();
                    crate::sys::download_bytes(&name, body.as_bytes());
                }
                Task::none()
            }
            Message::AttrEditorOpen(handle) => {
                self.open_attribute_editor(handle);
                Task::none()
            }
            Message::AttrEditorTab(t) => {
                self.attr_editor_tab = t;
                Task::none()
            }
            Message::AttrEditorSelect(idx) => {
                if idx < self.attr_editor_rows.len() {
                    self.attr_editor_selected = idx;
                }
                Task::none()
            }
            Message::AttrEditorInput { idx, value } => {
                if let Some(r) = self.attr_editor_rows.get_mut(idx) {
                    r.value = value;
                }
                Task::none()
            }
            Message::AttrEditorTextStyle(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.text_style = s;
                }
                Task::none()
            }
            Message::AttrEditorJustify(label) => {
                if let Some((h, v)) =
                    crate::ui::window::attribute_editor::justify_from_label(&label)
                {
                    if let Some(r) = self.attr_row_selected_mut() {
                        r.h_align = h;
                        r.v_align = v;
                    }
                }
                Task::none()
            }
            Message::AttrEditorHeight(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.height = s;
                }
                Task::none()
            }
            Message::AttrEditorRotation(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.rotation = s;
                }
                Task::none()
            }
            Message::AttrEditorWidth(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.width_factor = s;
                }
                Task::none()
            }
            Message::AttrEditorOblique(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.oblique = s;
                }
                Task::none()
            }
            Message::AttrEditorBackwards(b) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.backwards = b;
                }
                Task::none()
            }
            Message::AttrEditorUpsideDown(b) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.upside_down = b;
                }
                Task::none()
            }
            Message::AttrEditorLayer(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.layer = s;
                }
                Task::none()
            }
            Message::AttrEditorLinetype(s) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.linetype = if s == "ByLayer" { String::new() } else { s };
                }
                Task::none()
            }
            Message::AttrEditorColor(label) => {
                if let Some(c) = crate::ui::window::attribute_editor::color_from_label(&label) {
                    if let Some(r) = self.attr_row_selected_mut() {
                        r.color = c;
                    }
                }
                Task::none()
            }
            Message::AttrEditorLineweight(lw) => {
                if let Some(r) = self.attr_row_selected_mut() {
                    r.line_weight = lw;
                }
                Task::none()
            }
            Message::AttrEditorApply => self.on_attr_editor_apply(),
            Message::ModalGrab => {
                // Start a drag; the first ModalDragMove seeds the reference.
                self.modal_dragging = true;
                self.modal_drag_last = None;
                Task::none()
            }
            Message::ModalResizeGrab => {
                // Start a resize; the first ModalDragMove seeds the reference.
                self.modal_resizing = true;
                self.modal_drag_last = None;
                Task::none()
            }
            Message::ModalContentResized(size) => {
                if !size.width.is_finite()
                    || !size.height.is_finite()
                    || size.width <= 0.0
                    || size.height <= 0.0
                {
                    return Task::none();
                }
                let first_measurement = self.modal_content_size.replace(size).is_none();
                if first_measurement {
                    let initial_width = self.mtext_editor.as_ref().and_then(|editor| {
                        editor.editing.is_none().then(|| {
                            (size.width - 2.0 * super::view::overlay::MTEXT_PREVIEW_PAD)
                                .max(80.0)
                                / editor.preview_scale()
                        })
                    });
                    if let (Some(editor), Some(width)) =
                        (self.mtext_editor.as_mut(), initial_width)
                    {
                        editor.rect_width = f64::from(width.max(1e-6));
                        self.rebuild_mtext_preview();
                    }
                }
                Task::none()
            }
            Message::RibbonLayerFilterChanged(f) => {
                self.ribbon.layer_filter = f;
                Task::none()
            }
            Message::LayerManagerFilterChanged(f) => {
                let i = self.active_tab;
                self.tabs[i].layers.filter = f;
                Task::none()
            }
            Message::LayerNameColGrab => {
                // Start a Name-column divider drag; rides ModalDragMove.
                self.layer_col_dragging = true;
                self.modal_drag_last = None;
                Task::none()
            }
            Message::ModalDragMove(p) => {
                if let Some(last) = self.modal_drag_last {
                    let (dx, dy) = (p.x - last.x, p.y - last.y);
                    if self.layer_col_dragging {
                        self.layer_name_col_w = (self.layer_name_col_w + dx).clamp(60.0, 640.0);
                    } else if self.modal_resizing {
                        // The grip sits bottom-right, so dragging out grows the
                        // box. The delta is added to each dialog's natural size,
                        // so clamp it at zero — dragging in past the natural size
                        // does nothing (the natural size is the floor).
                        let nx = (self.modal_resize.x + dx).max(0.0);
                        let ny = (self.modal_resize.y + dy).max(0.0);
                        let (rx, ry) = (nx - self.modal_resize.x, ny - self.modal_resize.y);
                        self.modal_resize.x = nx;
                        self.modal_resize.y = ny;
                        // The box is centred, so shift the centre by half the
                        // growth to pin the top-left corner — the grip then
                        // tracks the cursor instead of drifting at half speed.
                        self.modal_offset.x += rx * 0.5;
                        self.modal_offset.y += ry * 0.5;
                    } else if self.modal_dragging {
                        self.modal_offset.x += dx;
                        self.modal_offset.y += dy;
                        // Clamp so the dialog stops at the window edge instead
                        // of being squeezed (the off-centre padding shrinks the
                        // dialog once it overlaps a border).
                        if let Some((cw, ch)) = self.modal_outer_size() {
                            let (ww, wh) = if self.mtext_editor.is_some() {
                                self.vp_size
                            } else {
                                self.win_size
                            };
                            let max_x = ((ww - cw) * 0.5).max(0.0);
                            let max_y = ((wh - ch) * 0.5).max(0.0);
                            self.modal_offset.x = self.modal_offset.x.clamp(-max_x, max_x);
                            self.modal_offset.y = self.modal_offset.y.clamp(-max_y, max_y);
                        }
                    }
                }
                if self.modal_dragging || self.modal_resizing || self.layer_col_dragging {
                    self.modal_drag_last = Some(p);
                }
                Task::none()
            }
            Message::ModalDragRelease => {
                self.modal_dragging = false;
                self.modal_resizing = false;
                self.layer_col_dragging = false;
                self.modal_drag_last = None;
                Task::none()
            }

            Message::AboutCopyInfo => {
                let info = format!(
                    "Open CAD Studio v{}\nOS: {}\nArch: {}",
                    env!("CARGO_PKG_VERSION"),
                    crate::ui::window::about::platform_name(),
                    crate::ui::window::about::architecture_name(),
                );
                #[cfg(target_arch = "wasm32")]
                {
                    crate::sys::write_clipboard_text(&info);
                    Task::none()
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    iced::clipboard::write(info).discard()
                }
            }

            // ── Plugin Manager window ─────────────────────────────────────
            Message::PluginManagerOpen => {
                #[cfg(target_arch = "wasm32")]
                {
                    self.active_modal = Some(super::ModalKind::PluginManager);
                    return Task::none();
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // Refresh the on-disk external-plugin list each time the manager
                    // opens so newly dropped-in packages show up.
                    self.external_plugins = crate::plugin::external::discover();
                    self.marketplace_status.clear();
                    self.active_modal = Some(super::ModalKind::PluginManager);
                    // Fetch the curated registry and release lists for linked repos.
                    self.plugin_registry_loading = true;
                    self.plugin_registry_error = None;
                    self.plugin_registry_error_details_open = false;
                    let mut tasks = vec![self.fetch_registry_task()];
                    let release_repos: rustc_hash::FxHashSet<String> = self
                        .plugin_repos
                        .iter()
                        .cloned()
                        .chain(
                            self.external_plugins
                                .iter()
                                .filter_map(|plugin| plugin.repository.clone()),
                        )
                        .collect();
                    tasks.extend(
                        release_repos
                            .into_iter()
                            .map(|r| self.fetch_releases_task(r)),
                    );
                    if self.selected_plugin_repo.is_none() {
                        self.selected_plugin_repo = self
                            .external_plugins
                            .iter()
                            .find_map(|plugin| plugin.repository.clone())
                            .or_else(|| self.plugin_registry.first().map(|entry| entry.repo.clone()))
                            .or_else(|| self.plugin_repos.first().cloned());
                    }
                    if let Some(repo) = self.selected_plugin_repo.clone() {
                        if !self.plugin_readmes.contains_key(&repo)
                            && self.plugin_readme_loading.insert(repo.clone())
                        {
                            tasks.push(self.fetch_plugin_readme_task(repo));
                        }
                    }
                    return Task::batch(tasks);
                }
            }
            Message::PluginManagerClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::SetPluginEnabled(id, enabled) => {
                if enabled {
                    self.disabled_plugins.remove(&id);
                } else {
                    self.disabled_plugins.insert(id);
                }
                self.rebuild_ribbon_modules();
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::PluginRepoInput(s) => {
                self.plugin_repo_input = s;
                Task::none()
            }
            Message::PluginSearchInput(s) => {
                self.plugin_search_input = s;
                Task::none()
            }
            Message::PluginRepoAdd => {
                let Some(repo) =
                    crate::plugin::external::normalize_repository(&self.plugin_repo_input)
                else {
                    self.marketplace_status =
                        "Enter a GitHub URL or repository in owner/repo format.".to_string();
                    return Task::none();
                };
                if self.plugin_repos.contains(&repo)
                    || self.plugin_registry.iter().any(|entry| entry.repo == repo)
                {
                    self.marketplace_status = format!("{repo} is already in the catalog.");
                    self.selected_plugin_repo = Some(repo.clone());
                    if !self.plugin_readmes.contains_key(&repo)
                        && self.plugin_readme_loading.insert(repo.clone())
                    {
                        return self.fetch_plugin_readme_task(repo);
                    }
                    return Task::none();
                }
                if self
                    .external_plugins
                    .iter()
                    .any(|plugin| plugin.repository.as_deref() == Some(repo.as_str()))
                {
                    self.marketplace_status = format!("{repo} is already installed.");
                    self.selected_plugin_repo = Some(repo.clone());
                    if !self.plugin_readmes.contains_key(&repo)
                        && self.plugin_readme_loading.insert(repo.clone())
                    {
                        return self.fetch_plugin_readme_task(repo);
                    }
                    return Task::none();
                }
                self.plugin_repos.push(repo.clone());
                self.plugin_repo_input.clear();
                self.persist_settings_if_changed();
                self.marketplace_status = format!("Fetching releases for {repo}…");
                self.selected_plugin_repo = Some(repo.clone());
                self.plugin_readmes.remove(&repo);
                self.plugin_readme_loading.insert(repo.clone());
                Task::batch(vec![
                    self.fetch_releases_task(repo.clone()),
                    self.fetch_plugin_readme_task(repo),
                ])
            }
            Message::PluginRepoRemove(repo) => {
                self.plugin_repos.retain(|r| r != &repo);
                self.repo_release_tags.remove(&repo);
                self.repo_selected_tag.remove(&repo);
                if self.selected_plugin_repo.as_deref() == Some(repo.as_str())
                    && !self.plugin_registry.iter().any(|entry| entry.repo == repo)
                {
                    self.selected_plugin_repo =
                        self.plugin_registry.first().map(|entry| entry.repo.clone());
                }
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::PluginRegistryFetched(Ok(entries)) => {
                self.plugin_registry_loading = false;
                self.plugin_registry_error = None;
                self.plugin_registry_error_details_open = false;
                // Fetch releases for every curated repo so the dropdowns fill in.
                #[cfg(not(target_arch = "wasm32"))]
                {
                    if self.selected_plugin_repo.is_none() {
                        self.selected_plugin_repo = self
                            .external_plugins
                            .iter()
                            .find_map(|plugin| {
                                plugin.repository.clone().or_else(|| {
                                    entries
                                        .iter()
                                        .find(|entry| entry.name.eq_ignore_ascii_case(&plugin.name))
                                        .map(|entry| entry.repo.clone())
                                })
                            })
                            .or_else(|| entries.first().map(|entry| entry.repo.clone()));
                    }
                    let mut tasks: Vec<_> = entries
                        .iter()
                        .map(|e| self.fetch_releases_task(e.repo.clone()))
                        .collect();
                    self.plugin_registry = entries;
                    if let Some(repo) = self.selected_plugin_repo.clone() {
                        if !self.plugin_readmes.contains_key(&repo)
                            && self.plugin_readme_loading.insert(repo.clone())
                        {
                            tasks.push(self.fetch_plugin_readme_task(repo));
                        }
                    }
                    return Task::batch(tasks);
                }
                #[cfg(target_arch = "wasm32")]
                {
                    self.plugin_registry = entries;
                    Task::none()
                }
            }
            Message::PluginRegistryFetched(Err(e)) => {
                self.plugin_registry_loading = false;
                self.plugin_registry_error = Some(e);
                self.plugin_registry_error_details_open = false;
                Task::none()
            }
            Message::PluginRegistryRetry => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.plugin_registry_loading = true;
                    self.plugin_registry_error = None;
                    self.plugin_registry_error_details_open = false;
                    return self.fetch_registry_task();
                }
                #[cfg(target_arch = "wasm32")]
                Task::none()
            }
            Message::PluginRegistryErrorDetailsToggle => {
                if self.plugin_registry_error.is_some() {
                    self.plugin_registry_error_details_open =
                        !self.plugin_registry_error_details_open;
                }
                Task::none()
            }
            Message::PluginRegistryCopyDiagnostics => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(error) = &self.plugin_registry_error {
                    return iced::clipboard::write(format!(
                        "Open CAD Studio v{}\nOS: {}\nArchitecture: {}\nRegistry: {}\nError: {}",
                        env!("CARGO_PKG_VERSION"),
                        std::env::consts::OS,
                        std::env::consts::ARCH,
                        crate::plugin::marketplace::REGISTRY_URL,
                        error,
                    ))
                    .discard();
                }
                Task::none()
            }
            Message::PatronsFetched(Ok(names)) => {
                // Merge the hand-maintained supporters and rank everyone by
                // amount (also sorts the web list, which arrives unsorted).
                self.patrons = crate::patreon::merge_manual(names);
                Task::none()
            }
            // No token / offline: still show any hand-maintained supporters
            // (Start page shows a "Support on Patreon" prompt when empty).
            Message::PatronsFetched(Err(_)) => {
                self.patrons = crate::patreon::merge_manual(Vec::new());
                Task::none()
            }
            Message::VideosFetched(Ok(videos)) => {
                self.videos_loading = false;
                self.set_videos(videos);
                Task::none()
            }
            // Offline / markup change: keep whatever the on-disk cache seeded.
            Message::VideosFetched(Err(_)) => {
                self.videos_loading = false;
                Task::none()
            }
            Message::DiscussionsFetched(Ok(discussions)) => {
                self.discussions_loading = false;
                self.discussions = discussions;
                Task::none()
            }
            // Offline: keep the native cache (web leaves the panel empty).
            Message::DiscussionsFetched(Err(_)) => {
                self.discussions_loading = false;
                Task::none()
            }
            Message::RecentThumbsLoaded(thumbs) => {
                for (path, handle) in thumbs {
                    self.recent_thumbs.insert(path, handle);
                }
                Task::none()
            }
            Message::PluginReleasesFetched(repo, Ok(releases)) => {
                if let Some(first) = releases.first() {
                    self.repo_selected_tag
                        .entry(repo.clone())
                        .or_insert_with(|| first.tag.clone());
                }
                if self.marketplace_status == format!("Fetching releases for {repo}…") {
                    self.marketplace_status =
                        format!(
                            "Repository added. {} installable release(s) found.",
                            releases.len()
                        );
                }
                self.repo_release_tags.insert(repo, releases);
                Task::none()
            }
            Message::PluginReleasesFetched(repo, Err(e)) => {
                self.marketplace_status = format!("{repo}: {e}");
                Task::none()
            }
            Message::PluginReleaseSelect(repo, tag) => {
                self.repo_selected_tag.insert(repo, tag);
                Task::none()
            }
            Message::PluginReadmeSelect(repo) => {
                self.selected_plugin_repo = Some(repo.clone());
                if self.plugin_readme_loading.contains(&repo) {
                    return Task::none();
                }
                if matches!(self.plugin_readmes.get(&repo), Some(Ok(_))) {
                    return Task::none();
                }
                // A second click on an error state acts as retry.
                self.plugin_readmes.remove(&repo);
                self.plugin_readme_loading.insert(repo.clone());
                self.fetch_plugin_readme_task(repo)
            }
            Message::PluginReadmeFetched(repo, result) => {
                self.plugin_readme_loading.remove(&repo);
                self.plugin_readmes.insert(
                    repo,
                    result.map(|source| iced::widget::markdown::Content::parse(&source)),
                );
                Task::none()
            }
            Message::PluginInstall(repo) => {
                let Some(tag) = self.repo_selected_tag.get(&repo).cloned() else {
                    return Task::none();
                };
                self.marketplace_status = format!("Installing {repo} {tag}…");
                self.install_task(repo, tag)
            }
            Message::PluginUpdate(repo, tag) => {
                self.marketplace_status = format!("Updating {repo} to {tag}…");
                self.install_task(repo, tag)
            }
            Message::PluginInstalled(Ok(id)) => {
                self.marketplace_status = format!("Installed '{id}'. Restart to load it.");
                self.plugin_load_errors.remove(&id);
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.external_plugins = crate::plugin::external::discover();
                }
                Task::none()
            }
            Message::PluginInstalled(Err(e)) => {
                self.marketplace_status = format!("Install failed: {e}");
                Task::none()
            }
            Message::PluginUninstall(id) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // Stop the plugin runner first so Windows releases the DLL
                    // and allows the package directory to be deleted.
                    if !crate::plugin::external::remove_plugin(&id) {
                        self.marketplace_status =
                            format!("Uninstall failed: plugin '{id}' did not stop in time");
                        return Task::none();
                    }
                    match crate::plugin::external::uninstall(&id) {
                        Ok(()) => {
                            self.marketplace_status =
                                format!("Uninstalled '{id}'.");
                            self.plugin_load_errors.remove(&id);
                            self.loaded_plugin_ids.remove(&id);
                            self.rebuild_ribbon_modules();
                            self.external_plugins = crate::plugin::external::discover();
                        }
                        Err(e) => {
                            self.marketplace_status = format!("Uninstall failed: {e}");
                        }
                    }
                }
                #[cfg(target_arch = "wasm32")]
                let _ = id;
                Task::none()
            }
            Message::PointStyleSetMode(mode) => {
                self.set_point_mode_bits(!0, mode);
                Task::none()
            }
            Message::PointStyleSizeRelative(relative) => {
                self.point_size_relative = relative;
                self.apply_point_size();
                Task::none()
            }
            Message::PointStyleSizeInput(s) => {
                self.point_size_buf = s;
                Task::none()
            }
            Message::PointStyleApplySize => {
                self.apply_point_size();
                Task::none()
            }
            Message::PointStyleOk => {
                self.apply_point_size();
                self.close_active_modal();
                Task::none()
            }

            Message::EnterViewport(handle) => {
                let i = self.active_tab;
                let context_changed = self.tabs[i].scene.active_viewport != Some(handle);
                let cancel_task = if context_changed {
                    self.cancel_active_command_for_space_change()
                } else {
                    Task::none()
                };
                let perf = crate::perf::enabled();
                let total = Instant::now();
                if context_changed {
                    self.tabs[i].scene.clear_preview_wire();
                }
                // Clear paper-space selection before entering model space.
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.active_viewport = Some(handle);
                // Fold a stale UTM saved view onto the effective (auto-fit)
                // centre so pan/zoom, paper↔model and the display all agree —
                // otherwise the camera auto-fits to the model while the cursor
                // math stays at the origin, jittering as pan toggles the two.
                let phase = Instant::now();
                self.tabs[i].scene.normalize_active_viewport_view();
                let normalize_ms = phase.elapsed().as_secs_f64() * 1000.0;
                // Grid/snap follow the entered viewport.
                let phase = Instant::now();
                self.adopt_view_display(i);
                let display_ms = phase.elapsed().as_secs_f64() * 1000.0;
                // Adopt the entered viewport's own per-viewport UCS.
                let phase = Instant::now();
                self.tabs[i].refresh_active_ucs();
                let ucs_ms = phase.elapsed().as_secs_f64() * 1000.0;
                let phase = Instant::now();
                self.refresh_properties();
                let properties_ms = phase.elapsed().as_secs_f64() * 1000.0;
                self.command_line.push_output(crate::t!("MSPACE").as_ref());
                if perf {
                    crate::perf_record!(
                        "[perf] viewport-enter total={:.2}ms normalize={:.2}ms display={:.2}ms ucs={:.2}ms properties={:.2}ms handle={}",
                        total.elapsed().as_secs_f64() * 1000.0,
                        normalize_ms,
                        display_ms,
                        ucs_ms,
                        properties_ms,
                        handle.value(),
                    );
                }
                if context_changed {
                    self.sync_dyn_fields();
                }
                cancel_task
            }

            Message::ExitViewport => {
                let i = self.active_tab;
                let context_changed = self.tabs[i].scene.active_viewport.is_some();
                let cancel_task = if context_changed {
                    self.cancel_active_command_for_space_change()
                } else {
                    Task::none()
                };
                if context_changed {
                    self.tabs[i].scene.clear_preview_wire();
                }
                // Clear model-space selection before returning to paper space.
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.active_viewport = None;
                // Grid/snap return to the paper sheet's own state.
                self.adopt_view_display(i);
                // Paper space has no UCS — drop the viewport's UCS.
                self.tabs[i].refresh_active_ucs();
                self.refresh_properties();
                self.command_line.push_output(crate::t!("PSPACE").as_ref());
                if context_changed {
                    self.sync_dyn_fields();
                }
                cancel_task
            }

            Message::MspaceCommand => {
                let i = self.active_tab;
                if self.tabs[i].scene.current_layout == "Model" {
                    self.command_line
                        .push_error(crate::t!("MS is only available in paper space layouts.").as_ref());
                    return Task::none();
                }
                if self.tabs[i].scene.active_viewport.is_some() {
                    // Already in MSPACE — nothing to do.
                    return Task::none();
                }
                match self.tabs[i].scene.first_user_viewport() {
                    Some(handle) => Task::done(Message::EnterViewport(handle)),
                    None => {
                        self.command_line
                            .push_error(crate::t!("No viewport found in this layout.").as_ref());
                        Task::none()
                    }
                }
            }

            Message::PspaceCommand => Task::done(Message::ExitViewport),

            Message::Undo => {
                // Mid-command Ctrl+Z: a drawing command steps itself back
                // (PLINE pops the last vertex) instead of the document undo
                // swallowing the whole in-progress object.
                let i = self.active_tab;
                let step = self.tabs[i]
                    .active_cmd
                    .as_mut()
                    .and_then(|c| c.on_undo_step());
                if let Some(r) = step {
                    return self.apply_cmd_result(r);
                }
                self.undo_active_tab();
                Task::none()
            }
            Message::Redo => {
                self.redo_active_tab();
                Task::none()
            }

            Message::UndoMany(steps) => {
                self.ribbon.close_dropdown();
                self.undo_steps(steps);
                Task::none()
            }

            Message::RedoMany(steps) => {
                self.ribbon.close_dropdown();
                self.redo_steps(steps);
                Task::none()
            }

            Message::Noop => Task::none(),
            Message::StatusMenuTooltipHidden(hidden) => {
                self.status_menu_tooltip_hidden = hidden;
                if hidden {
                    self.polar_custom_input.clear();
                }
                Task::none()
            }

            // ── Unsaved-changes dialog ────────────────────────────────────
            Message::UnsavedDialogCancel => {
                self.pending_close = None;
                self.pending_tab_closes.clear();
                self.close_unsaved_dialog_window()
            }

            Message::UnsavedDialogDiscard => self.on_unsaved_dialog_discard(),

            Message::UnsavedDialogSave => self.on_unsaved_dialog_save(),

            Message::AecDropSameVersion => self.on_aec_drop_same_version(),
            Message::AecDropProceed => self.on_aec_drop_proceed(),
            Message::AecDropBack => {
                self.active_modal = Some(crate::app::ModalKind::SaveDialog);
                Task::none()
            }

            Message::AutoSave => self.on_autosave(),

            Message::ThumbnailCaptureFrame => self.on_thumbnail_capture_frame(),

            Message::ThumbnailCaptureFinished => {
                self.thumbnail_capture_clean = false;
                Task::none()
            }

            #[cfg(not(target_arch = "wasm32"))]
            Message::SaveFinished(outcome) => self.on_save_finished(outcome),

            #[cfg(target_arch = "wasm32")]
            Message::WebSaveScreenshot {
                tab_id,
                filename,
                ext,
                version,
                bounds,
                screenshot,
            } => self.on_web_save_screenshot(
                tab_id,
                filename,
                ext,
                version,
                bounds,
                screenshot,
            ),

            #[cfg(not(target_arch = "wasm32"))]
            Message::SaveFileInUseRetry => self.on_save_file_in_use_retry(),

            #[cfg(not(target_arch = "wasm32"))]
            Message::SaveFileInUseSaveAs => self.on_save_file_in_use_save_as(),

            #[cfg(not(target_arch = "wasm32"))]
            Message::SaveFileInUseCancel => {
                self.close_active_modal();
                Task::none()
            }

            #[cfg(not(target_arch = "wasm32"))]
            Message::ExternalChangeReload => self.on_external_change_reload(),

            #[cfg(not(target_arch = "wasm32"))]
            Message::ExternalChangeSaveAs => self.on_external_change_save_as(),

            #[cfg(not(target_arch = "wasm32"))]
            Message::ExternalChangeOverwrite => self.on_external_change_overwrite(),

            #[cfg(not(target_arch = "wasm32"))]
            Message::ExternalChangeCancel => {
                self.close_active_modal();
                Task::none()
            }

            // ── Page Setup ────────────────────────────────────────────────
            Message::UpdateCheckResult(latest) => {
                let Some(info) = latest else {
                    return Task::none();
                };
                self.update_notice_version = Some(info.version);
                self.update_notice_body = Some(info.body);
                self.active_modal = Some(super::ModalKind::UpdateNotice);
                Task::none()
            }
            Message::UpdateNoticeClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::UpdateNoticeOpenRelease => {
                let open = crate::sys::open_url(
                    crate::io::update_check::RELEASES_PAGE,
                    self.main_window,
                );
                self.close_active_modal();
                open
            }
            Message::AssocPromptYes => {
                self.file_assoc_enabled = true;
                self.mark_assoc_prompted();
                self.active_modal = None;
                self.reset_modal_geometry();
                // set_default_app registers the handler first, then makes us the
                // default — boot no longer does this automatically.
                Task::perform(
                    crate::io::file_association::set_default_app(),
                    Message::AssocResult,
                )
            }
            Message::AssocPromptNo => {
                self.file_assoc_enabled = false;
                self.mark_assoc_prompted();
                self.active_modal = None;
                self.reset_modal_geometry();
                Task::none()
            }
            Message::AssocResult(result) => {
                match result {
                    Ok(msg) => self.command_line.push_info(&msg),
                    Err(err) => self
                        .command_line
                        .push_error(crate::tf!("Could not set default app: {err}").as_ref()),
                }
                Task::none()
            }
            Message::PlotDialogOpen => self.on_plot_dialog_open(),
            Message::PlotDlg(m) => self.on_plot_dlg(m),
            Message::BlockPalette(m) => self.on_block_palette(m),
            Message::Dock(m) => self.on_dock(m),
            Message::PrintAllOpen => self.on_print_all_open(),
            Message::PrintAllToggle(name) => {
                if let Some((_, selected)) = self
                    .print_all_layouts
                    .iter_mut()
                    .find(|(layout, _)| layout == &name)
                {
                    *selected = !*selected;
                }
                Task::none()
            }
            Message::PrintAllSelectAll => {
                for (_, selected) in &mut self.print_all_layouts {
                    *selected = true;
                }
                Task::none()
            }
            Message::PrintAllSelectNone => {
                for (_, selected) in &mut self.print_all_layouts {
                    *selected = false;
                }
                Task::none()
            }
            Message::PrintAllOptions => self.on_print_all_options(),
            Message::PrintAllPdf => {
                let i = self.active_tab;
                let stem = self.tabs[i]
                    .current_path
                    .as_deref()
                    .and_then(|path| path.file_stem())
                    .map(|name| format!("{}_layouts", name.to_string_lossy()))
                    .unwrap_or_else(|| "drawing_layouts".into());
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let Some(window_id) = self.main_window else {
                        return Task::done(Message::PrintAllPdfPath(None));
                    };
                    iced::window::run(window_id, move |parent| {
                        crate::io::pdf_export::pick_pdf_path_owned(stem, parent)
                    })
                    .map(Message::PrintAllPdfPath)
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = stem;
                    self.command_line.push_error(
                        crate::t!("PDF export is not available in the web version.").as_ref(),
                    );
                    Task::none()
                }
            }
            Message::PrintAllPdfPath(None) => Task::none(),
            Message::PrintAllPdfPath(Some(path)) => self.on_print_all_pdf_path_some(path),
            Message::PrintAllPrint => self.on_print_all_print(),
            Message::PrintAllFinished(result) => {
                match result {
                    Ok(message) => self.command_line.push_info(&message),
                    Err(error) => {
                        self.command_line.push_error(&error);
                        if self.active_modal.is_none() {
                            self.active_modal = Some(super::ModalKind::PrintAll);
                            self.reset_modal_geometry();
                        }
                    }
                }
                Task::none()
            }

            // ── Plot / Export ─────────────────────────────────────────────
            Message::PlotExport => {
                let i = self.active_tab;
                let stem = self.tabs[i]
                    .current_path
                    .as_deref()
                    .and_then(|p: &std::path::Path| p.file_stem())
                    .map(|s: &std::ffi::OsStr| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "drawing".into());
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let Some(window_id) = self.main_window else {
                        return Task::done(Message::PlotExportPath(None));
                    };
                    iced::window::run(window_id, move |parent| {
                        crate::io::pdf_export::pick_pdf_path_owned(stem, parent)
                    })
                    .map(Message::PlotExportPath)
                }
                #[cfg(target_arch = "wasm32")]
                {
                    Task::perform(
                        crate::io::pdf_export::pick_pdf_path_owned(stem),
                        Message::PlotExportPath,
                    )
                }
            }
            Message::PlotExportPath(None) => Task::none(),
            Message::PlotExportPath(Some(path)) => self.on_plot_export_path_some(path),

            Message::PlotFormat(f) => {
                self.plot_format = f;
                Task::none()
            }
            Message::PlotOrientation(o) => {
                self.plot_orientation = o;
                Task::none()
            }
            Message::PlotWindowExport => {
                let i = self.active_tab;
                let stem = self.tabs[i]
                    .current_path
                    .as_deref()
                    .and_then(|p: &std::path::Path| p.file_stem())
                    .map(|s: &std::ffi::OsStr| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "drawing".into());
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let Some(window_id) = self.main_window else {
                        return Task::done(Message::PlotWindowExportPath(None));
                    };
                    iced::window::run(window_id, move |parent| {
                        crate::io::pdf_export::pick_pdf_path_owned(stem, parent)
                    })
                    .map(Message::PlotWindowExportPath)
                }
                #[cfg(target_arch = "wasm32")]
                {
                    Task::perform(
                        crate::io::pdf_export::pick_pdf_path_owned(stem),
                        Message::PlotWindowExportPath,
                    )
                }
            }
            Message::PlotWindowExportPath(None) => Task::none(),
            Message::PlotWindowExportPath(Some(path)) => self.on_plot_window_export_path_some(path),

            Message::BackgroundIoFinished(result, reopen_plot) => {
                match result {
                    Ok(message) => self.command_line.push_info(&message),
                    Err(error) => self.command_line.push_error(&error),
                }
                if reopen_plot {
                    self.active_modal = Some(crate::app::ModalKind::Plot);
                }
                Task::none()
            }

            // ── Print to system printer ───────────────────────────────────────
            Message::PrintToPrinter => self.on_print_to_printer(),
            Message::PrintResult(Ok(printer)) => {
                self.command_line
                    .push_info(crate::tf!("Sent to printer: {printer}").as_ref());
                Task::none()
            }
            Message::PrintResult(Err(e)) => {
                self.command_line.push_error(crate::tf!("Print failed: {e}").as_ref());
                Task::none()
            }

            // ── Plot Style Table ──────────────────────────────────────────────
            Message::PlotStyleLoad => {
                Task::perform(crate::io::pick_plot_style(), Message::PlotStyleLoaded)
            }
            Message::PlotStyleLoaded(Some(table)) => {
                if table.is_stb {
                    self.command_line.push_error(
                        "Named plot style tables are not supported by the vector plotter.",
                    );
                    return Task::none();
                }
                self.plot_dialog.style_name = table.name.clone();
                self.plot_dialog.style_missing = false;
                self.command_line.push_output(crate::tf!(
                    "Plot style '{}' loaded ({} color entries).",
                    table.name,
                    table
                        .aci_entries
                        .iter()
                        .filter(|e| e.color.is_some())
                        .count()
                ).as_ref());
                self.active_plot_style = Some(table);
                self.plot_dialog.plot_styles = crate::io::plot_style::available_ctb_names();
                Task::none()
            }
            Message::PlotStyleLoaded(None) => Task::none(),
            Message::PlotStyleClear => {
                self.active_plot_style = None;
                self.plot_dialog.style_name.clear();
                self.plot_dialog.style_missing = false;
                self.command_line.push_output(crate::t!("Plot style table cleared.").as_ref());
                Task::none()
            }

            // ── Plot Style Panel ──────────────────────────────────────────────
            Message::PlotStylePanelOpen => {
                // The Plot dialog's selected table is authoritative. Make sure
                // the editor opens that table rather than a stale active table.
                let selected_style = self.plot_dialog.style_name.clone();

                if selected_style.is_empty() {
                    self.active_plot_style = None;
                } else {
                    let needs_load = self
                        .active_plot_style
                        .as_ref()
                        .is_none_or(|table| !table.name.eq_ignore_ascii_case(&selected_style));

                    if needs_load {
                        match crate::io::plot_style::PlotStyleTable::load_named(&selected_style) {
                            Ok(table) => {
                                self.active_plot_style = Some(table);
                                self.plot_dialog.style_missing = false;
                            }
                            Err(error) => {
                                self.plot_dialog.style_missing = true;
                                self.command_line.push_error(&error);
                                return Task::none();
                            }
                        }
                    }
                }
                // Initialise edit buffers for ACI 1.
                self.plotstyle_panel_aci = 1;
                let entry = self
                    .active_plot_style
                    .as_ref()
                    .and_then(|t| t.aci_entries.get(1));
                self.ps_color_buf = entry
                    .and_then(|e| {
                        e.color
                            .map(|[r, g, b]| format!("#{:02X}{:02X}{:02X}", r, g, b))
                    })
                    .unwrap_or_default();
                self.ps_lineweight_buf = entry
                    .map(|e| e.lineweight.to_string())
                    .unwrap_or("255".into());
                self.ps_screening_buf = entry
                    .map(|e| e.screening.to_string())
                    .unwrap_or("100".into());
                // When launched from PLOT, preserve the parent dialog so the Plot Style
                // editor can appear above it instead of replacing it.
                if self.active_modal == Some(super::ModalKind::Plot) {
                    self.plotstyle_parent_plot_geometry =
                        Some((self.modal_offset, self.modal_resize));

                    // The child editor starts with its own centred geometry.
                    self.reset_modal_geometry();
                } else {
                    // Direct command launch: Plotstyle is a normal standalone modal.
                    self.plotstyle_parent_plot_geometry = None;
                }

                self.active_modal = Some(super::ModalKind::Plotstyle);
                Task::none()
            }
            Message::PlotStylePanelClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::PlotStylePanelSelectAci(aci) => {
                self.plotstyle_panel_aci = aci;
                let entry = self
                    .active_plot_style
                    .as_ref()
                    .and_then(|t| t.aci_entries.get(aci as usize));
                self.ps_color_buf = entry
                    .and_then(|e| {
                        e.color
                            .map(|[r, g, b]| format!("#{:02X}{:02X}{:02X}", r, g, b))
                    })
                    .unwrap_or_default();
                self.ps_lineweight_buf = entry
                    .map(|e| e.lineweight.to_string())
                    .unwrap_or("255".into());
                self.ps_screening_buf = entry
                    .map(|e| e.screening.to_string())
                    .unwrap_or("100".into());
                Task::none()
            }
            Message::PlotStylePanelColorBuf(s) => {
                self.ps_color_buf = s;
                self.on_plot_style_panel_apply()
            }

            Message::PlotStylePanelLwBuf(s) => {
                self.ps_lineweight_buf = s;
                self.on_plot_style_panel_apply()
            }
            Message::PlotStylePanelLwSet(index) => {
                self.ps_lineweight_buf = index.to_string();
                self.on_plot_style_panel_apply()
            }

            Message::PlotStylePanelScreenBuf(s) => {
                self.ps_screening_buf = s;
                self.on_plot_style_panel_apply()
            }

            Message::PlotStylePanelApply => self.on_plot_style_panel_apply(),

        Message::PlotStylePanelSaveDirect => {
            if self.active_plot_style.is_none() {
                self.command_line.push_error(
                    crate::t!("No plot style table loaded. Load or create one first.").as_ref(),
                );
                return Task::none();
            }

            #[cfg(not(target_arch = "wasm32"))]
            {
                let table = self.active_plot_style.as_ref().expect("checked above");
                let table_name = table.name.clone();

                let result = crate::io::plot_style::ensure_plot_styles_dir().and_then(|dir| {
                    let path = dir.join(&table_name);
                    table.save(&path)?;
                    Ok(path)
                });

                match result {
                    Ok(path) => {
                        self.plot_dialog.style_name = table_name;
                        self.plot_dialog.style_missing = false;
                        self.plot_dialog.plot_styles =
                            crate::io::plot_style::available_ctb_names();
                        self.tabs[self.active_tab]
                            .scene
                            .invalidate_display_plot_style();

                        self.command_line.push_output(
                            crate::tf!(
                                "Plot style table saved to \"{}\".",
                                path.display()
                            )
                            .as_ref(),
                        );
                    }
                    Err(error) => {
                        self.command_line
                            .push_error(crate::tf!("Save error: {error}").as_ref());
                    }
                }

                Task::none()
            }

            #[cfg(target_arch = "wasm32")]
            {
                // Browsers cannot overwrite a local file directly, so fall back
                // to the existing Save As flow.
                self.on_plot_style_panel_save()
            }
        }

        Message::PlotStylePanelSave => self.on_plot_style_panel_save(),
        Message::PlotStylePanelSavePath(Some(path)) => {
                let path = if path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("ctb"))
                {
                    path
                } else {
                    path.with_extension("ctb")
                };
                if let Some(table) = &self.active_plot_style {
                    match table.save(&path) {
                        Ok(()) => {
                            let name = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                            if let Some(table) = self.active_plot_style.as_mut() {
                                table.name = name.clone();
                            }
                            self.plot_dialog.style_name = name;
                            self.plot_dialog.style_missing = false;
                            self.plot_dialog.plot_styles =
                                crate::io::plot_style::available_ctb_names();
                            self.tabs[self.active_tab]
                                .scene
                                .invalidate_display_plot_style();
                            self.command_line.push_output(crate::tf!(
                                "Plot style table saved to \"{}\".",
                                path.display()
                            ).as_ref());
                        }
                        Err(e) => self.command_line.push_error(crate::tf!("Save error: {e}").as_ref()),
                    }
                }
                Task::none()
            }
            Message::PlotStylePanelSavePath(None) => Task::none(),

            // ── TextStyle Font Browser ────────────────────────────────────────
            Message::TextStyleDialogOpen => self.on_text_style_dialog_open(),
            Message::TextStyleDialogClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::TextStyleDialogSelect(name) => {
                self.stage_textstyle_bufs();
                let i = self.active_tab;
                self.textstyle_selected = name;
                self.load_textstyle_bufs(i);
                Task::none()
            }
            Message::TextStyleDialogTab(tab) => {
                self.textstyle_tab = tab;
                Task::none()
            }
            Message::TextStyleDialogCompare(name) => {
                self.textstyle_compare = name;
                Task::none()
            }
            Message::TextStyleDialogSetCurrent => {
                // Staged: persists on Apply.
                let i = self.active_tab;
                let name = self.textstyle_selected.clone();
                if self.tabs[i]
                    .scene
                    .document
                    .text_styles
                    .get(&name)
                    .is_some_and(|style| !style.xref_dependent)
                {
                    self.tabs[i].scene.document.header.current_text_style_name = name.clone();
                    self.sync_ribbon_styles();
                    self.command_line
                        .push_output(crate::tf!("Current text style: {}", name).as_ref());
                }
                Task::none()
            }
            Message::TextStyleDialogNew => {
                self.style_new(crate::app::StyleKind::Text);
                Task::none()
            }
            Message::TextStyleDialogCopy => {
                self.style_copy(crate::app::StyleKind::Text);
                Task::none()
            }
            Message::TextStyleDialogDelete => {
                self.style_delete(crate::app::StyleKind::Text);
                Task::none()
            }
            // ── Shared inline rename (all style managers) ─────────────────
            Message::StyleRenameStart(kind, name) => {
                self.style_rename_start(kind, name);
                // Focus the freshly-shown rename field so the user can type
                // immediately after the double click.
                iced::widget::operation::focus(crate::ui::style::style_list::rename_input_id())
            }
            Message::StyleRenameEdit(s) => {
                self.style_rename_buf = s;
                Task::none()
            }
            Message::StyleRenameCommit(kind) => {
                self.style_rename_commit(kind);
                Task::none()
            }
            Message::StyleRenameCancel => {
                self.style_rename_cancel();
                Task::none()
            }
            Message::TextStyleEdit { field, value } => {
                match field {
                    "font" => self.textstyle_font = value,
                    "width" => self.textstyle_width = value,
                    "oblique" => self.textstyle_oblique = value,
                    "height" => self.textstyle_height = value,
                    "bigfont" => self.textstyle_bigfont = value,
                    "ttf" => self.textstyle_ttf = value,
                    _ => {}
                }
                Task::none()
            }
            Message::TextStyleToggle(field) => {
                // Staged: mutate live for preview, persist on Apply.
                let i = self.active_tab;
                let name = self.textstyle_selected.clone();
                if let Some(s) = self.tabs[i].scene.document.text_styles.get_mut(&name) {
                    if s.xref_dependent {
                        return Task::none();
                    }
                    match field {
                        "backward" => s.flags.backward = !s.flags.backward,
                        "upside_down" => s.flags.upside_down = !s.flags.upside_down,
                        "vertical" => s.is_vertical = !s.is_vertical,
                        "annotative" => s.annotative = !s.annotative,
                        _ => {}
                    }
                }
                Task::none()
            }
            Message::TextStyleApply => self.on_text_style_apply(),
            Message::TextStyleFontPick(font_file) => {
                // Staged: update the buffer + live style; persist on Apply.
                let i = self.active_tab;
                let name = self.textstyle_selected.clone();
                if self.tabs[i]
                    .scene
                    .document
                    .text_styles
                    .get(&name)
                    .is_some_and(|style| style.xref_dependent)
                {
                    return Task::none();
                }
                self.textstyle_font = font_file.clone();
                if let Some(s) = self.tabs[i].scene.document.text_styles.get_mut(&name) {
                    s.font_file = font_file;
                }
                Task::none()
            }

            // ── TableStyle Dialog ─────────────────────────────────────────────
            Message::TableStyleDialogOpen => {
                use acadrust::objects::ObjectType;
                let i = self.active_tab;
                self.tablestyle_selected = self.tabs[i]
                    .scene
                    .document
                    .objects
                    .values()
                    .find_map(|o| {
                        if let ObjectType::TableStyle(s) = o {
                            Some(s.name.clone())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| "Standard".to_string());
                self.load_tablestyle_bufs(i);
                self.active_modal = Some(super::ModalKind::TableStyle);
                self.style_stage_begin();
                Task::none()
            }
            Message::TableStyleDialogClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::TableStyleDialogSelect(name) => {
                for row in 0..3 {
                    let _ = self.on_table_style_cell_apply(row);
                }
                self.stage_tablestyle_bufs();
                self.tablestyle_selected = name;
                let i = self.active_tab;
                self.load_tablestyle_bufs(i);
                Task::none()
            }
            Message::TableStyleDialogTab(tab) => {
                self.tablestyle_tab = tab;
                Task::none()
            }
            Message::TableStyleDialogCompare(name) => {
                self.tablestyle_compare = name;
                Task::none()
            }

            Message::TableStyleEdit { field, value } => {
                match field {
                    "hmargin" => self.ts_hmargin = value,
                    "vmargin" => self.ts_vmargin = value,
                    "description" => self.ts_description = value,
                    _ => {}
                }
                Task::none()
            }

            Message::TableStyleApply => {
                for row in 0..3 {
                    let _ = self.on_table_style_cell_apply(row);
                }
                self.stage_tablestyle_bufs();
                self.style_stage_commit();
                Task::none()
            }

            Message::TableStyleSetFlow(value) => {
                use acadrust::objects::TableFlowDirection;
                let i = self.active_tab;
                if let Some(s) = self.tablestyle_mut(i) {
                    s.flow_direction = match value.as_str() {
                        "Up" => TableFlowDirection::Up,
                        _ => TableFlowDirection::Down,
                    };
                }
                Task::none()
            }

            Message::TableColorMore(row, field) => {
                self.ts_color_open = if self.ts_color_open == Some((row, field)) {
                    None
                } else {
                    Some((row, field))
                };
                Task::none()
            }
            Message::TableStyleCellEdit { row, field, value } => {
                self.ts_color_open = None;
                let r = row as usize;
                if r < 3 {
                    match field {
                        "textstyle" => self.ts_cell_textstyle[r] = value,
                        "height" => self.ts_cell_height[r] = value,
                        "textcolor" => self.ts_cell_textcolor[r] = value,
                        "fillcolor" => self.ts_cell_fillcolor[r] = value,
                        "datatype" => self.ts_cell_datatype[r] = value,
                        "unittype" => self.ts_cell_unittype[r] = value,
                        "format" => self.ts_cell_format[r] = value,
                        _ => {}
                    }
                }
                Task::none()
            }

            Message::TableStyleBorderEdit {
                cell,
                border,
                field,
                value,
            } => {
                let (c, b) = (cell as usize, border as usize);
                if c < 3 && b < 6 {
                    match field {
                        "lw" => self.ts_border_lw[c][b] = value,
                        "color" => self.ts_border_color[c][b] = value,
                        "spacing" => self.ts_border_spacing[c][b] = value,
                        _ => {}
                    }
                }
                Task::none()
            }

            Message::TableStyleBorderSetType {
                cell,
                border,
                value,
            } => {
                use acadrust::objects::TableBorderType;
                let i = self.active_tab;
                if let Some(s) = self.tablestyle_mut(i) {
                    if let Some(bd) =
                        Self::ts_cell_of(s, cell).and_then(|c| Self::ts_border_of(c, border))
                    {
                        bd.border_type = match value.as_str() {
                            "Double" => TableBorderType::Double,
                            _ => TableBorderType::Single,
                        };
                    }
                }
                Task::none()
            }

            Message::TableStyleBorderToggleInvisible { cell, border } => {
                let i = self.active_tab;
                if let Some(s) = self.tablestyle_mut(i) {
                    if let Some(bd) =
                        Self::ts_cell_of(s, cell).and_then(|c| Self::ts_border_of(c, border))
                    {
                        bd.is_invisible = !bd.is_invisible;
                    }
                }
                Task::none()
            }

            Message::TableStyleCellToggleFill(row) => {
                let i = self.active_tab;
                if let Some(s) = self.tablestyle_mut(i) {
                    if let Some(c) = Self::ts_cell_of(s, row) {
                        c.fill_enabled = !c.fill_enabled;
                    }
                }
                Task::none()
            }

            Message::TableStyleCellSetAlign { row, value } => {
                use acadrust::objects::CellAlignment;
                let i = self.active_tab;
                if let Some(s) = self.tablestyle_mut(i) {
                    if let Some(c) = Self::ts_cell_of(s, row) {
                        c.alignment = match value.as_str() {
                            "TopLeft" => CellAlignment::TopLeft,
                            "TopCenter" => CellAlignment::TopCenter,
                            "TopRight" => CellAlignment::TopRight,
                            "MiddleLeft" => CellAlignment::MiddleLeft,
                            "MiddleRight" => CellAlignment::MiddleRight,
                            "BottomLeft" => CellAlignment::BottomLeft,
                            "BottomCenter" => CellAlignment::BottomCenter,
                            "BottomRight" => CellAlignment::BottomRight,
                            _ => CellAlignment::MiddleCenter,
                        };
                    }
                }
                Task::none()
            }

            Message::TableStyleCellApply(row) => self.on_table_style_cell_apply(row),

            Message::TableStyleToggle(field) => {
                use acadrust::objects::ObjectType;
                let i = self.active_tab;
                let name = self.tablestyle_selected.clone();
                for obj in self.tabs[i].scene.document.objects.values_mut() {
                    if let ObjectType::TableStyle(s) = obj {
                        if s.name == name {
                            match field {
                                "title_sup" => s.title_suppressed = !s.title_suppressed,
                                "header_sup" => s.header_suppressed = !s.header_suppressed,
                                _ => {}
                            }
                        }
                    }
                }
                Task::none()
            }

            Message::TableStyleToggleAnnotative => {
                use acadrust::objects::ObjectType;
                let i = self.active_tab;
                let name = self.tablestyle_selected.clone();
                for obj in self.tabs[i].scene.document.objects.values_mut() {
                    if let ObjectType::TableStyle(s) = obj {
                        if s.name == name {
                            s.annotative = !s.annotative;
                        }
                    }
                }
                Task::none()
            }

            Message::TableStyleDialogNew => {
                self.style_new(crate::app::StyleKind::Table);
                Task::none()
            }
            Message::TableStyleDialogCopy => {
                self.style_copy(crate::app::StyleKind::Table);
                Task::none()
            }
            Message::TableStyleDialogDelete => {
                self.style_delete(crate::app::StyleKind::Table);
                Task::none()
            }
            Message::TableStyleDialogSetCurrent => {
                // Staged: persists on Apply. The header field is the round-trip
                // source of truth ($CTABLESTYLE); the ribbon mirrors it.
                let i = self.active_tab;
                let name = self.tablestyle_selected.clone();
                if self
                    .style_names(crate::app::StyleKind::Table)
                    .contains(&name)
                {
                    self.tabs[i].scene.document.header.current_table_style_name = name.clone();
                    self.ribbon.active_table_style = name.clone();
                    self.command_line
                        .push_output(crate::tf!("Current table style: {name}").as_ref());
                }
                Task::none()
            }

            // ── MLineStyle Dialog ─────────────────────────────────────────────
            Message::MlStyleDialogOpen => self.on_ml_style_dialog_open(),
            Message::MlStyleDialogClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::MlStyleDialogSelect(name) => {
                self.stage_mlstyle_bufs();
                self.mlstyle_selected = name;
                let i = self.active_tab;
                self.load_mlstyle_bufs(i);
                Task::none()
            }
            Message::MlStyleDialogTab(tab) => {
                self.mlstyle_tab = tab;
                Task::none()
            }
            Message::MlStyleDialogCompare(name) => {
                self.mlstyle_compare = name;
                Task::none()
            }
            Message::MlStyleDialogSetCurrent => {
                use acadrust::objects::ObjectType;
                let i = self.active_tab;
                let name = self.mlstyle_selected.clone();
                let exists = self.tabs[i]
                    .scene
                    .document
                    .objects
                    .values()
                    .any(|o| matches!(o, ObjectType::MLineStyle(s) if s.name == name));
                if exists {
                    // Staged: persists on Apply.
                    self.tabs[i].scene.document.header.multiline_style = name.clone();
                    self.command_line
                        .push_output(crate::tf!("Current multiline style: {}", name).as_ref());
                }
                Task::none()
            }
            Message::MlStyleApply => {
                self.stage_mlstyle_bufs();
                self.style_stage_commit();
                Task::none()
            }
            Message::MlStyleDialogNew => {
                self.style_new(crate::app::StyleKind::MLine);
                Task::none()
            }
            Message::MlStyleDialogCopy => {
                self.style_copy(crate::app::StyleKind::MLine);
                Task::none()
            }
            Message::MlStyleDialogDelete => {
                self.style_delete(crate::app::StyleKind::MLine);
                Task::none()
            }
            Message::MlStyleEdit { field, value } => {
                match field {
                    "description" => self.mln_description = value,
                    "start_angle" => self.mln_start_angle = value,
                    "end_angle" => self.mln_end_angle = value,
                    "fill_color" => self.mln_fill_color = value,
                    _ => {}
                }
                self.stage_mlstyle_bufs();
                Task::none()
            }
            Message::MlStyleToggle(field) => {
                let i = self.active_tab;
                if let Some(style) = self.mlstyle_mut(i) {
                    match field {
                        "fill" => style.flags.fill_on = !style.flags.fill_on,
                        "joints" => style.flags.display_joints = !style.flags.display_joints,
                        "start_square" => style.flags.start_square_cap = !style.flags.start_square_cap,
                        "start_inner" => style.flags.start_inner_arcs_cap = !style.flags.start_inner_arcs_cap,
                        "start_round" => style.flags.start_round_cap = !style.flags.start_round_cap,
                        "end_square" => style.flags.end_square_cap = !style.flags.end_square_cap,
                        "end_inner" => style.flags.end_inner_arcs_cap = !style.flags.end_inner_arcs_cap,
                        "end_round" => style.flags.end_round_cap = !style.flags.end_round_cap,
                        _ => {}
                    }
                }
                Task::none()
            }
            Message::MlStyleElementEdit { index, field, value } => {
                if let Some(element) = self.mln_elements.get_mut(index) {
                    match field {
                        "offset" => element[0] = value,
                        "color" => element[1] = value,
                        "linetype" => element[2] = value,
                        _ => {}
                    }
                }
                self.stage_mlstyle_bufs();
                Task::none()
            }
            Message::MlStyleElementAdd => {
                let i = self.active_tab;
                if let Some(style) = self.mlstyle_mut(i) {
                    style
                        .elements
                        .push(acadrust::objects::MLineStyleElement::default());
                }
                self.load_mlstyle_bufs(i);
                Task::none()
            }
            Message::MlStyleElementDelete(index) => {
                let i = self.active_tab;
                if let Some(style) = self.mlstyle_mut(i) {
                    if style.elements.len() > 1 && index < style.elements.len() {
                        style.elements.remove(index);
                    }
                }
                self.load_mlstyle_bufs(i);
                Task::none()
            }

            // ── MLeaderStyle Dialog ───────────────────────────────────────────
            Message::MLeaderStyleDialogOpen => self.on_mleader_style_dialog_open(),
            Message::MLeaderStyleDialogClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::MLeaderStyleDialogSelect(name) => {
                self.stage_mleaderstyle_bufs();
                self.mleaderstyle_selected = name;
                let i = self.active_tab;
                self.load_mleaderstyle_bufs(i);
                Task::none()
            }
            Message::MLeaderStyleDialogTab(tab) => {
                self.mleaderstyle_tab = tab;
                Task::none()
            }
            Message::MLeaderStyleDialogCompare(name) => {
                self.mleaderstyle_compare = name;
                Task::none()
            }
            Message::MLeaderStyleDialogSetCurrent => self.on_mleader_style_dialog_set_current(),
            Message::MLeaderStyleDialogNew => {
                self.style_new(crate::app::StyleKind::MLeader);
                Task::none()
            }
            Message::MLeaderStyleDialogCopy => {
                self.style_copy(crate::app::StyleKind::MLeader);
                Task::none()
            }
            Message::MLeaderStyleDialogDelete => {
                self.style_delete(crate::app::StyleKind::MLeader);
                Task::none()
            }
            Message::MLeaderStyleEdit { field, value } => self.on_mleader_style_edit(field, value),
            Message::MLeaderStyleToggle(field) => {
                let i = self.active_tab;
                if let Some(s) = self.mleaderstyle_mut(i) {
                    match field {
                        "enable_landing" => s.enable_landing = !s.enable_landing,
                        "enable_dogleg" => s.enable_dogleg = !s.enable_dogleg,
                        "text_frame" => s.text_frame = !s.text_frame,
                        "text_always_left" => s.text_always_left = !s.text_always_left,
                        "annotative" => s.is_annotative = !s.is_annotative,
                        "enable_block_scale" => s.enable_block_scale = !s.enable_block_scale,
                        "enable_block_rotation" => {
                            s.enable_block_rotation = !s.enable_block_rotation
                        }
                        _ => {}
                    }
                }
                Task::none()
            }
            Message::MLeaderColorMore(field) => {
                self.mls_color_open = if self.mls_color_open == Some(field) {
                    None
                } else {
                    Some(field)
                };
                Task::none()
            }
            Message::MLeaderStyleSetEnum { field, value } => {
                self.on_mleader_style_set_enum(field, value)
            }
            Message::MLeaderStyleLineWeightChanged(line_weight) => {
                let i = self.active_tab;
                if let Some(s) = self.mleaderstyle_mut(i) {
                    s.line_weight = line_weight;
                }
                Task::none()
            }
            Message::MLeaderStyleSetHandle { field, value } => {
                self.on_mleader_style_set_handle(field, value)
            }
            Message::MLeaderStyleApply => self.on_mleader_style_apply(),

            // ── DimStyle Dialog ───────────────────────────────────────────────
            Message::DimStyleDialogOpen => self.on_dim_style_dialog_open(),
            Message::DimStyleDialogClose => {
                self.close_active_modal();
                Task::none()
            }
            Message::DimStyleDialogApply => {
                let i = self.active_tab;
                self.apply_dimstyle_bufs(i);
                self.style_stage_commit();
                Task::none()
            }
            Message::DimStyleDialogSelect(name) => {
                let i = self.active_tab;
                // Stage the current edits before switching so they aren't lost.
                self.apply_dimstyle_bufs(i);
                self.dimstyle_selected = name;
                self.load_dimstyle_bufs(i);
                Task::none()
            }
            Message::DimStyleDialogTab(tab) => {
                self.dimstyle_tab = tab;
                Task::none()
            }
            Message::DimStyleDialogCompare(name) => {
                self.dimstyle_compare = name;
                Task::none()
            }
            Message::DimStyleDialogNew => {
                self.style_new(crate::app::StyleKind::Dim);
                Task::none()
            }
            Message::DimStyleDialogCopy => {
                self.style_copy(crate::app::StyleKind::Dim);
                Task::none()
            }
            Message::DimStyleDialogSetCurrent => {
                // Staged: persists on Apply.
                let i = self.active_tab;

                self.tabs[i].scene.document.header.current_dimstyle_name =
                    self.dimstyle_selected.clone(); 
                self.sync_ribbon_styles();
                self.command_line.push_output(crate::tf!(
                    "Current dim style set to '{}'.",
                    self.dimstyle_selected
                ).as_ref());
                Task::none()
            }
            Message::DimStyleDialogDelete => {
                self.style_delete(crate::app::StyleKind::Dim);
                Task::none()
            }
            Message::DsEdit(field, val) => {
                self.apply_ds_edit(field, val);
                self.ds_color_open = None;
                Task::none()
            }
            Message::DsToggle(field) => {
                let separate_arrows = field == crate::app::DsField::Dimsah;
                self.apply_ds_toggle(field);
                if separate_arrows && self.ds_dimsah {
                    let i = self.active_tab;
                    if let Some(style) = self.tabs[i]
                        .scene
                        .document
                        .dim_styles
                        .get_mut(&self.dimstyle_selected)
                    {
                        if style.dimblk1.is_null() {
                            style.dimblk1 = style.dimblk;
                        }
                        if style.dimblk2.is_null() {
                            style.dimblk2 = style.dimblk1;
                        }
                    }
                }
                Task::none()
            }
            Message::DsToleranceMode(mode) => {
                self.ds_dimlim = mode == "limits";
                self.ds_dimtol = matches!(mode.as_str(), "symmetrical" | "deviation");
                if mode == "symmetrical" {
                    self.ds_dimtm = self.ds_dimtp.clone();
                }
                let gap = self.ds_dimgap.trim().parse::<f64>().unwrap_or(0.625).abs();
                self.ds_dimgap = if mode == "basic" {
                    format!("-{}", gap.max(f64::EPSILON))
                } else {
                    format!("{}", gap)
                };
                Task::none()
            }
            Message::DsZeroBase(field, base) => {
                let current = match &field {
                    crate::app::DsField::Dimzin => &self.ds_dimzin,
                    crate::app::DsField::Dimaltz => &self.ds_dimaltz,
                    crate::app::DsField::Dimalttz => &self.ds_dimalttz,
                    crate::app::DsField::Dimtzin => &self.ds_dimtzin,
                    _ => return Task::none(),
                }
                .trim()
                .parse::<i16>()
                .unwrap_or(0);
                self.apply_ds_edit(field, ((current & !3) | (base & 3)).to_string());
                Task::none()
            }
            Message::DsZeroFlag(field, bit) => {
                let current = match &field {
                    crate::app::DsField::Dimzin => &self.ds_dimzin,
                    crate::app::DsField::Dimaltz => &self.ds_dimaltz,
                    crate::app::DsField::Dimalttz => &self.ds_dimalttz,
                    crate::app::DsField::Dimtzin => &self.ds_dimtzin,
                    _ => return Task::none(),
                }
                .trim()
                .parse::<i16>()
                .unwrap_or(0);
                self.apply_ds_edit(field, (current ^ bit).to_string());
                Task::none()
            }
            Message::DsCenterMarkMode(mode) => {
                let size = self.ds_dimcen.trim().parse::<f64>().unwrap_or(0.09).abs();
                self.ds_dimcen = match mode.as_str() {
                    "mark" => size.max(f64::EPSILON).to_string(),
                    "lines" => format!("-{}", size.max(f64::EPSILON)),
                    _ => "0".to_string(),
                };
                Task::none()
            }
            Message::DsColorMore(field) => {
                self.ds_color_open = if self.ds_color_open.as_ref() == Some(&field) {
                    None
                } else {
                    Some(field)
                };
                Task::none()
            }
            Message::OpenColorWindow(target, color) => {
                self.color_pick_target = Some((target, color));

                // Always open the shared CAD colour picker on the indexed ACI page.
                self.color_picker_tab = crate::app::ColorPickerTab::Index;
                self.modal_offset = iced::Vector::ZERO;
                self.ds_color_open = None;
                self.mls_color_open = None;
                self.ts_color_open = None;
                self.ribbon.close_dropdown();

                let i = self.active_tab;
                self.tabs[i].properties.color_picker_open = false;
                self.tabs[i].properties.open_color_field = None;
                self.tabs[i].layers.color_picker_row = None;

                Task::none()
            }

            Message::ColorPickerTabChanged(tab) => {
                self.color_picker_tab = tab;
                Task::none()
            }
            Message::ColorPickerColorChanged(color) => {
                if let Some((_, current)) = self.color_pick_target.as_mut() {
                    *current = color;
                }
                Task::none()
            }
            Message::CloseColorPicker => {
                self.color_pick_target = None;
                Task::none()
            }

            Message::ColorWindowPick(color) => {
                // Keep only real colours in the recent list. ByLayer / ByBlock / None
                // are logical CAD states rather than reusable colours.
                if matches!(
                    &color,
                    acadrust::types::Color::Index(_)
                        | acadrust::types::Color::Rgb { .. }
                ) {
                    // No duplicates: selecting an existing colour moves it to the front.
                    if let Some(pos) = self.recent_colors.iter().position(|c| c == &color) {
                        self.recent_colors.remove(pos);
                    }

                    self.recent_colors.insert(0, color.clone());
                    self.recent_colors.truncate(12);
                }

                self.on_color_window_pick(color)
            }
            Message::DsSetHandle { field, value } => self.on_ds_set_handle(field, value),
        }
    }

    /// Load a named scale into the scale-manager editor buffers (name + the
    /// paper / drawing units); blank ratios when the scale isn't found.
    fn load_scale_editor(&mut self, name: &str) {
        let i = self.active_tab;
        self.scale_manager_selected = name.to_string();
        match self.tabs[i].scene.scale_paper_drawing(name) {
            Some((p, d)) => {
                self.scale_manager_paper_buf = format!("{p}");
                self.scale_manager_drawing_buf = format!("{d}");
            }
            None => {
                self.scale_manager_paper_buf.clear();
                self.scale_manager_drawing_buf.clear();
            }
        }
    }

    /// Fold the editor's paper:drawing ratio into the selected scale, keeping
    /// its name (renaming is done inline in the list). Staged, no commit — so
    /// the ratio edit survives Apply *and* switching to another row. Editing a
    /// built-in fallback scale materialises it as a real one.
    fn scale_apply_current(&mut self) {
        let i = self.active_tab;
        let sel = self.scale_manager_selected.clone();
        if sel.is_empty() {
            return;
        }
        let paper = self.scale_manager_paper_buf.trim().parse::<f64>().ok();
        let drawing = self.scale_manager_drawing_buf.trim().parse::<f64>().ok();
        if let (Some(paper), Some(drawing)) = (paper, drawing) {
            if paper > 0.0 && drawing > 0.0 {
                // Skip when the editor still holds the stored ratio, so merely
                // navigating between scales doesn't dirty the drawing.
                if let Some((cp, cd)) = self.tabs[i].scene.scale_paper_drawing(&sel) {
                    if (cp - paper).abs() < 1e-9 && (cd - drawing).abs() < 1e-9 {
                        return;
                    }
                }
                let changed = self.tabs[i].scene.edit_scale(&sel, &sel, paper, drawing)
                    || (self.tabs[i].scene.scale_paper_drawing(&sel).is_none()
                        && self.tabs[i].scene.add_scale(&sel, paper, drawing));
                if changed {
                    self.scale_stage_mark();
                }
            }
        }
    }

    /// Write the table-style editor buffers (margins / description) into the
    /// selected style (staged, no commit), so edits survive switching as well
    /// as Apply.
    fn stage_tablestyle_bufs(&mut self) {
        use acadrust::objects::ObjectType;
        let i = self.active_tab;
        let name = self.tablestyle_selected.clone();
        let h: Option<f64> = self.ts_hmargin.trim().parse().ok();
        let v: Option<f64> = self.ts_vmargin.trim().parse().ok();
        let desc = self.ts_description.clone();
        for obj in self.tabs[i].scene.document.objects.values_mut() {
            if let ObjectType::TableStyle(s) = obj {
                if s.name == name {
                    if let Some(h) = h {
                        s.horizontal_margin = h;
                    }
                    if let Some(v) = v {
                        s.vertical_margin = v;
                    }
                    s.description = desc.clone();
                }
            }
        }
    }

    /// A scale name based on `base`, suffixed " (n)" until it's unique in the
    /// drawing's scale list (used by New / Copy).
    fn unique_scale_name(&self, base: &str) -> String {
        let existing: std::collections::HashSet<String> = self.tabs[self.active_tab]
            .scene
            .scale_list()
            .into_iter()
            .map(|(n, _, _)| n.to_ascii_lowercase())
            .collect();
        if !existing.contains(&base.to_ascii_lowercase()) {
            return base.to_string();
        }
        let mut n = 2;
        loop {
            let candidate = format!("{base} ({n})");
            if !existing.contains(&candidate.to_ascii_lowercase()) {
                return candidate;
            }
            n += 1;
        }
    }
}

/// End-to-end GUI-flow test: drives the real `App::update` message loop
/// (no window/renderer) to exercise the full "Planart" (DisplayConfig)
/// status-bar flow end to end — draw a wall via the headless command
/// automation used by the GUI's own command line, select a `DisplayConfig`
/// that hides every wall display slot (as the status-bar dropdown's
/// `Message::AecActiveDisplayConfigSelected` does), and confirm the wall's
/// rendered representation actually disappears/reappears, instead of only
/// unit-testing the underlying slot-visibility logic in isolation.
#[cfg(test)]
mod aec_display_config_gui_flow_test {
    use super::Message;
    use crate::app::OpenCADStudio;
    use crate::modules::aec::commands::{
        regenerate_wall_representation, wall_from_entity, wall_record, AEC_APPID,
    };
    use crate::modules::aec::engine::display_component::{
        ComponentRuleSet, LayerSelection, WallComponentSlot,
    };
    use crate::modules::aec::engine::library::DisplayConfigLibrary;
    use crate::modules::aec::engine::plan_view::{
        DisplayConfig, PlanPhase, PlanningStage, ViewType, WALL_ELEMENT_TYPE_ID,
    };
    use crate::modules::aec::engine::wall::{WallJustification, WallLayer};
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::xdata::ExtendedDataRecord;
    use acadrust::EntityType;

    fn drawing_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    fn wall_layer(material: &str, thickness: f64, function: &str) -> WallLayer {
        WallLayer {
            material: material.to_string(),
            thickness,
            function: function.to_string(),
            axis_offset: -thickness * 0.5,
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
        layer_id: uuid::Uuid::new_v4(),
        }
    }

    fn wall_layers(specs: &[(&str, f64, &str)]) -> Vec<WallLayer> {
        use crate::modules::aec::engine::wall_style::migrate_gap_before_to_axis_offset;
        let pairs: Vec<(f64, f64)> = specs.iter().map(|(_, t, _)| (*t, 0.0)).collect();
        let offsets = migrate_gap_before_to_axis_offset(&pairs);
        specs
            .iter()
            .zip(offsets)
            .map(|(&(mat, t, fun), off)| WallLayer {
                material: mat.to_string(),
                thickness: t,
                function: fun.to_string(),
                axis_offset: off,
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
            })
            .collect()
    }

    /// Adds a two-layer `WALL` axis polyline directly to `app`'s active tab
    /// scene (same shape as `commands.rs`'s own `add_multi_layer_wall` test
    /// helper), then regenerates it once to build its baseline rendering —
    /// this setup step stands in for a user finishing `AEC_WALL` and is not
    /// itself what this test exercises.
    fn add_and_regenerate_wall(app: &mut OpenCADStudio) -> acadrust::Handle {
        let i = app.active_tab;
        let scene = &mut app.tabs[i].scene;
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = wall_layers(&[
            ("Concrete", 0.2, "Structural"),
            ("Insulation", 0.05, "Insulation"),
        ]);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record(
            "style1",
            3.0,
            0,
            &layers,
            &[],
            WallJustification::Center,
            crate::modules::aec::engine::plan_view::PlanPhase::New,
            None,
        );
        entity.common_mut().extended_data.add_record(record);
        let wall_handle = scene.add_entity(entity);
        regenerate_wall_representation(scene, wall_handle, None)
            .expect("regenerating a fresh two-layer wall must succeed");
        wall_handle
    }

    /// Finds the single `WALL` entity's handle and its current count of
    /// rendered display children (contour/hatch/solid), mirroring how the
    /// status bar/plan manager would inspect the drawing after a plan
    /// switch.
    fn wall_handle_and_derived_count(app: &OpenCADStudio) -> (acadrust::Handle, usize) {
        let scene = &app.tabs[app.active_tab].scene;
        for entity in scene.document.entities() {
            if let Some(wall) = wall_from_entity(entity) {
                return (entity.common().handle, wall.derived_handles.len());
            }
        }
        panic!("expected a WALL entity to exist");
    }

    #[test]
    fn selecting_a_display_config_that_hides_all_wall_slots_removes_the_walls_rendering() {
        // 1) Set up a wall with its baseline (default) rendering.
        let mut app = drawing_app();
        let expected_handle = add_and_regenerate_wall(&mut app);

        let (wall_handle, derived_before) = wall_handle_and_derived_count(&app);
        assert_eq!(wall_handle, expected_handle);
        assert!(
            derived_before > 0,
            "a freshly drawn wall must have a rendered representation (contour/hatch/solid)"
        );

        // 2) Build a "Statik 1:50" DisplayConfig that hides every wall
        //    display slot, and make it available the same way the Plan
        //    Manager/status-bar dropdown would (via `App::aec_plan_library`).
        // Step 3: overrides now live on `WallStyle::display_profiles`,
        // keyed by `DisplayConfig::name`, so the wall's own style ("style1")
        // must carry this rule set for the resolver to find it.
        let mut rules = ComponentRuleSet::default();
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
            rules.visibility.insert(slot.key().to_string(), false);
        }
        let hide_config = DisplayConfig::new(
            "Statik 1:50".to_string(),
            "Statik".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        let _ = WALL_ELEMENT_TYPE_ID;
        // A second config with no overrides ("Architekt 1:50") stands in for
        // the default, everything-visible representation, so switching back
        // to it exercises the regeneration path (`Message::
        // AecActiveDisplayConfigSelected(Some(...))`) rather than the
        // `None`/"Kein Plan" case, which only clears the active config
        // without regenerating (see `AecActiveDisplayConfigSelected` handler).
        let show_config = DisplayConfig::new(
            "Architekt 1:50".to_string(),
            "Architektur".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        app.aec_plan_library = Some(DisplayConfigLibrary {
            configs: vec![hide_config, show_config],
            ..Default::default()
        });

        // Register "style1" (the wall's `style_id`, set up by
        // `add_and_regenerate_wall`) with a `display_profiles` entry for
        // "Statik 1:50", via a project-embedded style library so
        // `apply_display_config_to_scene`'s per-wall resolver finds it.
        let mut display_profiles = std::collections::HashMap::new();
        display_profiles.insert("Statik 1:50".to_string(), rules);
        let wall_style = crate::modules::aec::engine::wall_style::WallStyle {
            style: crate::modules::aec::engine::style::Style {
                id: "style1".to_string(),
                name: "style1".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: Vec::new(),
            display_profiles,
        };
        let mut project = crate::modules::aec::engine::project::ProjectFile {
            buildings: Vec::new(),
            material_wall_style_library: crate::modules::aec::engine::library::StyleLibrary::default(),
            display_config_library: Default::default(),
        };
        project.material_wall_style_library.wall_styles.push(wall_style);
        app.aec_project_explorer_file = Some(project);

        // 3) Drive the exact message the status-bar "Planart" dropdown/popup
        //    dispatches on selection.
        let _ = app.update(Message::AecActiveDisplayConfigSelected(Some(
            "Statik 1:50".to_string(),
        )));

        let (still_same_handle, derived_after_hide) = wall_handle_and_derived_count(&app);
        assert_eq!(still_same_handle, wall_handle, "the wall's own handle must not change");
        assert_eq!(
            derived_after_hide, 0,
            "hiding every wall display slot must leave the wall with no rendered children"
        );

        // 4) Switching to the "Architekt 1:50" plan (no overrides) must
        //    regenerate the full, default representation again.
        let _ = app.update(Message::AecActiveDisplayConfigSelected(Some(
            "Architekt 1:50".to_string(),
        )));
        let (_, derived_after_reset) = wall_handle_and_derived_count(&app);
        assert_eq!(
            derived_after_reset, derived_before,
            "switching to a plan without wall overrides must restore the original rendering"
        );
    }

    /// Step 4 (Wandstil-Manager "Darstellungs-Profile" editor): driving the
    /// profile-select/layer-toggle/save messages the way the UI would must
    /// persist an independent `layer_filter` for `Contour2D` vs `Solid3D`
    /// into `WallStyle::display_profiles`, via the same copy-on-write save
    /// path the layer/parent form already uses.
    #[test]
    fn wall_style_manager_profile_save_persists_independent_slot_layer_filters() {
        let mut app = drawing_app();

        // A project-embedded style with one wall style ("style1") and two
        // Planarten, mirroring how `AecWallStyleManagerOpen`/`AecPlanManagerOpen`
        // would have already populated these libraries.
        let wall_style = crate::modules::aec::engine::wall_style::WallStyle {
            style: crate::modules::aec::engine::style::Style {
                id: "style1".to_string(),
                name: "Style 1".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: Vec::new(),
            display_profiles: std::collections::HashMap::new(),
        };
        let mut project = crate::modules::aec::engine::project::ProjectFile {
            buildings: Vec::new(),
            material_wall_style_library: crate::modules::aec::engine::library::StyleLibrary::default(),
            display_config_library: Default::default(),
        };
        project.material_wall_style_library.wall_styles.push(wall_style);
        app.aec_project_explorer_file = Some(project);
        app.aec_style_library = Some(
            crate::modules::aec::engine::library::combined_style_library(
                app.aec_project_explorer_file.as_ref(),
            ),
        );
        app.aec_plan_library = Some(DisplayConfigLibrary {
            configs: vec![DisplayConfig::new(
                "Ausführungsplan 1:50".to_string(),
                "Architektur".to_string(),
                PlanningStage::Design,
                ViewType::FloorPlan,
            )],
            ..Default::default()
        });

        // Simulate the form having "style1" open for editing, with two
        // layers in its edit-buffer (mirrors `AecStyleManagerSelectWallStyle`).
        app.aec_style_manager_wall_style_editing_id = Some("style1".to_string());
        app.aec_style_manager_wall_style_layers = vec![
            crate::app::AecLayerBuffer {
                material_id: "brick".to_string(),
                thickness: "24".to_string(),
                function: "Structural".to_string(),
                axis_offset: "0".to_string(),
                bottom_offset: "0".to_string(),
                top_offset: "0".to_string(),
                layer_override: String::new(),
                hatch_override: String::new(),
                role_tag: String::new(),
                layer_id: Some(uuid::Uuid::new_v4()),
            },
            crate::app::AecLayerBuffer {
                material_id: "insulation".to_string(),
                thickness: "5".to_string(),
                function: "Insulation".to_string(),
                axis_offset: "0".to_string(),
                bottom_offset: "0".to_string(),
                top_offset: "0".to_string(),
                layer_override: String::new(),
                hatch_override: String::new(),
                role_tag: String::new(),
                layer_id: Some(uuid::Uuid::new_v4()),
            },
        ];

        // Select the profile, then set an explicit Contour2D selection
        // (layer #0 only) while leaving Solid3D on "All".
        let _ = app.update(Message::AecStyleManagerProfileSelect(
            "Ausführungsplan 1:50".to_string(),
        ));
        let _ = app.update(Message::AecStyleManagerProfileContourModeToggle(true));
        let _ = app.update(Message::AecStyleManagerProfileContourLayerToggle(
            crate::modules::aec::engine::join::LayerRef {
                material_id: "brick".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            },
        ));
        let _ = app.update(Message::AecStyleManagerProfileHatchAngleChanged("45".to_string()));
        let _ = app.update(Message::AecStyleManagerProfileHatchRelativeToggle(true));
        let _ = app.update(Message::AecStyleManagerProfileSave);

        let saved_style = app
            .aec_project_explorer_file
            .as_ref()
            .unwrap()
            .material_wall_style_library
            .wall_styles
            .iter()
            .find(|w| w.style.id == "style1")
            .expect("style1 must still exist after saving its profile");
        let rules = saved_style
            .display_profiles
            .get("Ausführungsplan 1:50")
            .expect("the selected Planart must now have a display_profiles entry");

        assert_eq!(
            rules.layer_filter_for(WallComponentSlot::Contour2D),
            &LayerSelection::Explicit(vec![crate::modules::aec::engine::join::LayerRef {
                material_id: "brick".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            }]),
            "Contour2D must be restricted to the explicitly toggled layer"
        );
        assert_eq!(
            rules.layer_filter_for(WallComponentSlot::Solid3D),
            &LayerSelection::All,
            "Solid3D must remain untouched (still `All`) since it was never toggled"
        );
        let hatch = rules
            .style_override
            .get(WallComponentSlot::ContourHatch2D.key())
            .expect("hatch-angle override must have been saved");
        assert_eq!(hatch.hatch_angle, Some(45.0));
        assert_eq!(hatch.hatch_angle_relative, Some(true));
    }

    /// Removing a profile (`AecStyleManagerProfileRemove`) must delete the
    /// `display_profiles` entry entirely, reverting that Planart back to
    /// the style's default (non-regression) representation.
    #[test]
    fn wall_style_manager_profile_remove_deletes_the_display_profiles_entry() {
        let mut app = drawing_app();

        let mut display_profiles = std::collections::HashMap::new();
        display_profiles.insert(
            "Ausführungsplan 1:50".to_string(),
            ComponentRuleSet::default(),
        );
        let wall_style = crate::modules::aec::engine::wall_style::WallStyle {
            style: crate::modules::aec::engine::style::Style {
                id: "style1".to_string(),
                name: "Style 1".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: Vec::new(),
            display_profiles,
        };
        let mut project = crate::modules::aec::engine::project::ProjectFile {
            buildings: Vec::new(),
            material_wall_style_library: crate::modules::aec::engine::library::StyleLibrary::default(),
            display_config_library: Default::default(),
        };
        project.material_wall_style_library.wall_styles.push(wall_style);
        app.aec_project_explorer_file = Some(project);
        app.aec_style_library = Some(
            crate::modules::aec::engine::library::combined_style_library(
                app.aec_project_explorer_file.as_ref(),
            ),
        );
        app.aec_style_manager_wall_style_editing_id = Some("style1".to_string());
        app.aec_style_manager_profile_selected = Some("Ausführungsplan 1:50".to_string());

        let _ = app.update(Message::AecStyleManagerProfileRemove);

        let saved_style = app
            .aec_project_explorer_file
            .as_ref()
            .unwrap()
            .material_wall_style_library
            .wall_styles
            .iter()
            .find(|w| w.style.id == "style1")
            .expect("style1 must still exist after removing its profile");
        assert!(
            !saved_style.display_profiles.contains_key("Ausführungsplan 1:50"),
            "the removed Planart must no longer have a display_profiles entry"
        );
    }

    /// Step 5: applying the DisplayConfig form with the "Abbruch" phase
    /// unchecked and a `demolition_style` override set must persist a
    /// `PhaseFilter` whose `visible_phases` excludes `Demolition` and whose
    /// `demolition_style` carries the entered line-color override.
    #[test]
    fn plan_manager_apply_persists_phase_filter_from_two_stage_editor_buffers() {
        let mut app = drawing_app();

        let _ = app.update(Message::AecPlanManagerOpen);
        let _ = app.update(Message::AecPlanManagerNew);
        let _ = app.update(Message::AecPlanManagerNameChanged("Abbruchplan 1:50".to_string()));
        let _ = app.update(Message::AecPlanManagerPhaseVisibleToggle(PlanPhase::Demolition, false));
        let _ = app.update(Message::AecPlanManagerDemolitionStyleLineColorChanged("FF0000".to_string()));
        let _ = app.update(Message::AecPlanManagerApply);

        let lib = app.aec_plan_library.as_ref().expect("library must be loaded");
        let cfg = lib.find("Abbruchplan 1:50").expect("the new config must have been saved");
        let filter = cfg.phase_filter.as_ref().expect("an active PhaseFilter must have been built");
        assert!(!filter.visible_phases.contains(&PlanPhase::Demolition));
        assert!(filter.visible_phases.contains(&PlanPhase::New));
        assert!(filter.visible_phases.contains(&PlanPhase::Existing));
        assert_eq!(
            filter.demolition_style.as_ref().and_then(|s| s.line_color),
            Some(acadrust::types::Color::Rgb { r: 0xFF, g: 0x00, b: 0x00 })
        );
        assert!(filter.existing_style.is_none());
    }

    /// Selecting an existing config must load its `PhaseFilter` back into
    /// the two-stage editor buffers (round-trip of the Apply test above).
    #[test]
    fn plan_manager_select_reloads_phase_filter_into_edit_buffers() {
        let mut app = drawing_app();

        let _ = app.update(Message::AecPlanManagerOpen);
        let _ = app.update(Message::AecPlanManagerNew);
        let _ = app.update(Message::AecPlanManagerNameChanged("Bestandsplan 1:50".to_string()));
        let _ = app.update(Message::AecPlanManagerPhaseVisibleToggle(PlanPhase::New, false));
        let _ = app.update(Message::AecPlanManagerExistingStyleLineColorChanged("00FF00".to_string()));
        let _ = app.update(Message::AecPlanManagerApply);

        // Reset the buffers by opening a blank form, then re-select.
        let _ = app.update(Message::AecPlanManagerNew);
        assert!(app.aec_plan_manager_phase_filter_visible_new);
        let _ = app.update(Message::AecPlanManagerSelect("Bestandsplan 1:50".to_string()));

        assert!(!app.aec_plan_manager_phase_filter_visible_new);
        assert!(app.aec_plan_manager_phase_filter_visible_demolition);
        assert!(app.aec_plan_manager_phase_filter_visible_existing);
        assert_eq!(app.aec_plan_manager_existing_style_line_color, "00FF00");
    }

    #[test]
    fn plan_manager_apply_persists_global_visibility_and_hatch_scale_overlay() {
        use crate::modules::aec::engine::display_component::{
            ComponentStyleOverride, RepresentationMode, StyleDisplayOverlay, WallComponentKind,
        };
        use uuid::Uuid;

        let mut app = drawing_app();
        let layer_id = Uuid::new_v4();
        let _ = app.update(Message::AecPlanManagerOpen);
        let _ = app.update(Message::AecPlanManagerNew);
        let _ = app.update(Message::AecPlanManagerNameChanged("Statik 1:50".to_string()));
        let _ = app.update(Message::AecPlanManagerRepresentationChanged(
            RepresentationMode::TwoD,
        ));
        let _ = app.update(Message::AecPlanManagerComponentVisibleToggle(
            WallComponentKind::LayerHatch2D,
            false,
        ));
        app.aec_plan_manager_style_overlays.insert(
            "style1".into(),
            StyleDisplayOverlay {
                layer_props: [(
                    layer_id,
                    ComponentStyleOverride {
                        hatch_scale: Some(2.5),
                        hatch_angle: Some(45.0),
                        hatch_angle_relative: Some(true),
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
                ..Default::default()
            },
        );
        let _ = app.update(Message::AecPlanManagerApply);

        let cfg = app
            .aec_plan_library
            .as_ref()
            .unwrap()
            .find("Statik 1:50")
            .unwrap();
        assert_eq!(cfg.default_representation, RepresentationMode::TwoD);
        assert_eq!(
            cfg.component_visibility.get(&WallComponentKind::LayerHatch2D),
            Some(&false)
        );
        let overlay_props = cfg
            .style_overlays
            .get("style1")
            .and_then(|o| o.layer_props.get(&layer_id))
            .cloned()
            .unwrap();
        assert_eq!(overlay_props.hatch_scale, Some(2.5));
        assert_eq!(overlay_props.hatch_angle, Some(45.0));
        assert_eq!(overlay_props.hatch_angle_relative, Some(true));
        let saved_id = cfg.id;
        let _ = app.update(Message::AecPlanManagerSelect("Statik 1:50".to_string()));
        assert_eq!(app.aec_plan_manager_editing_id, Some(saved_id));
        assert_eq!(
            app.aec_plan_manager_default_representation,
            RepresentationMode::TwoD
        );
        let _ = app.update(Message::AecPlanManagerOverlayStyleSelect("style1".into()));
        let _ = app.update(Message::AecPlanManagerOverlayLayerSelect(layer_id));
        assert_eq!(app.aec_plan_manager_overlay_hatch_scale, "2.5");
        assert_eq!(app.aec_plan_manager_overlay_hatch_angle, "45");
        assert_eq!(app.aec_plan_manager_overlay_hatch_angle_relative, Some(true));
    }

    #[test]
    fn plan_manager_apply_persists_contour_hatch() {
        let mut app = drawing_app();
        app.aec_plan_manager_name = "Plan".into();
        app.aec_plan_manager_discipline = "Architektur".into();
        app.aec_plan_manager_overlay_style_id = Some("style1".into());
        app.aec_plan_manager_contour_hatch_pattern = "ANSI31".into();
        app.aec_plan_manager_contour_hatch_scale = "1.5".into();
        let _ = app.update(Message::AecPlanManagerApply);
        let cfg = app
            .aec_plan_library
            .as_ref()
            .and_then(|lib| lib.find("Plan"))
            .expect("saved");
        assert!(cfg.contour_hatch.is_none());
        let hatch = cfg
            .style_overlays
            .get("style1")
            .and_then(|o| o.contour_hatch.as_ref())
            .expect("style overlay contour hatch");
        assert_eq!(hatch.hatch_pattern.as_deref(), Some("ANSI31"));
        assert_eq!(hatch.hatch_scale, Some(1.5));
    }

    /// Step 7: the Stage 2 colour fields now open via `color_selector`
    /// (Layer-Manager-style swatch widget) instead of a raw hex text field —
    /// toggling flips the picker's `open` state, and picking a colour from
    /// the shared standalone palette window (`OpenColorWindow` /
    /// `on_color_window_pick`) round-trips back into the same hex buffer
    /// the old text field used to write to.
    #[test]
    fn plan_manager_demolition_line_color_picker_toggles_and_applies_from_color_window() {
        let mut app = drawing_app();

        let _ = app.update(Message::AecPlanManagerOpen);
        let _ = app.update(Message::AecPlanManagerNew);

        assert!(!app.aec_plan_manager_demolition_style_line_color_picker_open);
        let _ = app.update(Message::AecPlanManagerDemolitionStyleLineColorPickerToggle);
        assert!(app.aec_plan_manager_demolition_style_line_color_picker_open);
        let _ = app.update(Message::AecPlanManagerDemolitionStyleLineColorPickerToggle);
        assert!(!app.aec_plan_manager_demolition_style_line_color_picker_open);

        // Simulate picking a colour via the shared "More…" palette window,
        // as `style_editor_form`'s `color_selector` wires it up.
        let _ = app.update(Message::OpenColorWindow(
            crate::app::ColorPickTarget::AecPlanDemolitionLineColor,
            acadrust::types::Color::Rgb { r: 0x12, g: 0x34, b: 0x56 },
        ));
        let _ = app.on_color_window_pick(acadrust::types::Color::Rgb {
            r: 0x12,
            g: 0x34,
            b: 0x56,
        });
        assert_eq!(app.aec_plan_manager_demolition_style_line_color, "123456");
    }
}

/// Step 6: end-to-end Properties-panel flow for the wall-instance hatch-angle
/// override — driven through the real `Message`/`update` loop rather than
/// calling `commands::write_wall_hatch_override` directly, so it also
/// exercises the "wall_hatch_override_enabled"/"wall_hatch_relative"
/// `PropBoolToggle` handlers and the "wall_hatch_angle" `PropGeomCommit` path.
#[cfg(test)]
mod wall_hatch_override_properties_test {
    use super::Message;
    use crate::app::OpenCADStudio;
    use crate::modules::aec::commands::{wall_from_entity, wall_record, AEC_APPID};
    use crate::modules::aec::engine::wall::WallJustification;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::xdata::ExtendedDataRecord;
    use acadrust::EntityType;

    fn drawing_app_with_wall() -> (OpenCADStudio, acadrust::Handle) {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let scene = &mut app.tabs[i].scene;
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record(
            "style1",
            3.0,
            0,
            &[],
            &[],
            WallJustification::Center,
            crate::modules::aec::engine::plan_view::PlanPhase::New,
            None,
        );
        entity.common_mut().extended_data.add_record(record);
        let handle = scene.add_entity(entity);
        scene.select_entity(handle, true);
        app.refresh_properties();
        (app, handle)
    }

    /// Checking "Hatch Angle Override" synthesizes a default override
    /// (angle 0°/relative); unchecking it again clears the override
    /// entirely, restoring the style/material fallback.
    #[test]
    fn bool_toggle_sets_then_clears_the_wall_hatch_override() {
        let (mut app, handle) = drawing_app_with_wall();

        let _ = app.update(Message::PropBoolToggle("wall_hatch_override_enabled"));
        let wall = wall_from_entity(app.tabs[app.active_tab].scene.document.get_entity(handle).unwrap())
            .unwrap();
        assert_eq!(wall.hatch_override.as_ref().and_then(|o| o.hatch_angle), Some(0.0));
        assert_eq!(
            wall.hatch_override.as_ref().and_then(|o| o.hatch_angle_relative),
            Some(true)
        );

        let _ = app.update(Message::PropBoolToggle("wall_hatch_override_enabled"));
        let wall = wall_from_entity(app.tabs[app.active_tab].scene.document.get_entity(handle).unwrap())
            .unwrap();
        assert_eq!(wall.hatch_override, None);
    }

    /// Toggling "Relative to Wall" flips `hatch_angle_relative` on an
    /// already-active override, leaving the angle value untouched.
    #[test]
    fn bool_toggle_flips_relative_flag_on_an_existing_override() {
        let (mut app, handle) = drawing_app_with_wall();
        let _ = app.update(Message::PropBoolToggle("wall_hatch_override_enabled"));

        let _ = app.update(Message::PropBoolToggle("wall_hatch_relative"));
        let wall = wall_from_entity(app.tabs[app.active_tab].scene.document.get_entity(handle).unwrap())
            .unwrap();
        assert_eq!(
            wall.hatch_override.as_ref().and_then(|o| o.hatch_angle_relative),
            Some(false)
        );
    }

    /// Editing the angle text field (input + commit) updates
    /// `hatch_override.hatch_angle` while the override is active; with no
    /// override present, the edit is a harmless no-op.
    #[test]
    fn geom_commit_updates_the_angle_only_while_override_is_active() {
        let (mut app, handle) = drawing_app_with_wall();

        // No override yet: editing the angle field must not create one.
        let _ = app.update(Message::PropGeomInput {
            field: "wall_hatch_angle",
            value: "30".to_string(),
        });
        let _ = app.update(Message::PropGeomCommit("wall_hatch_angle"));
        let wall = wall_from_entity(app.tabs[app.active_tab].scene.document.get_entity(handle).unwrap())
            .unwrap();
        assert_eq!(wall.hatch_override, None);

        // Enable the override, then edit its angle.
        let _ = app.update(Message::PropBoolToggle("wall_hatch_override_enabled"));
        let _ = app.update(Message::PropGeomInput {
            field: "wall_hatch_angle",
            value: "60".to_string(),
        });
        let _ = app.update(Message::PropGeomCommit("wall_hatch_angle"));
        let wall = wall_from_entity(app.tabs[app.active_tab].scene.document.get_entity(handle).unwrap())
            .unwrap();
        assert_eq!(wall.hatch_override.and_then(|o| o.hatch_angle), Some(60.0));
    }
}

#[cfg(test)]
mod prop_pointer_tests {
    use super::Message;
    use crate::app::OpenCADStudio;
    use crate::ui::dock::PanelId;

    fn drawing_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    #[test]
    fn hidden_properties_panel_skips_the_focus_sweep() {
        // A click while the panel is closed must not run the widget-tree sweep:
        // the returned task is a bare `Task::none` (units == 0).
        let mut app = drawing_app();
        assert!(app.dock_panel_visible(PanelId::Properties));
        app.show_properties = false;
        assert!(!app.dock_panel_visible(PanelId::Properties));

        let task = app.update(Message::PropPointerPressed);
        assert_eq!(task.units(), 0);
    }

    #[test]
    fn visible_properties_panel_runs_the_focus_sweep() {
        // A click while the panel is open must still fire the sweep (units > 0)
        // so the select-whole-value feature keeps working.
        let mut app = drawing_app();
        app.show_properties = true;
        assert!(app.dock_panel_visible(PanelId::Properties));

        let task = app.update(Message::PropPointerPressed);
        assert!(task.units() > 0);
    }

    #[test]
    fn wall_style_manager_profile_visibility_toggles_persist_and_reload() {
        use crate::modules::aec::engine::display_component::WallComponentSlot;
        use crate::modules::aec::engine::library::DisplayConfigLibrary;
        use crate::modules::aec::engine::plan_view::{DisplayConfig, PlanningStage, ViewType};
        let mut app = drawing_app();

        let wall_style = crate::modules::aec::engine::wall_style::WallStyle {
            style: crate::modules::aec::engine::style::Style {
                id: "style1".to_string(),
                name: "Style 1".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: Vec::new(),
            display_profiles: std::collections::HashMap::new(),
        };
        let mut project = crate::modules::aec::engine::project::ProjectFile {
            buildings: Vec::new(),
            material_wall_style_library: crate::modules::aec::engine::library::StyleLibrary::default(),
            display_config_library: Default::default(),
        };
        project
            .material_wall_style_library
            .wall_styles
            .push(wall_style);
        app.aec_project_explorer_file = Some(project);
        app.aec_style_library = Some(crate::modules::aec::engine::library::combined_style_library(
            app.aec_project_explorer_file.as_ref(),
        ));
        app.aec_plan_library = Some(DisplayConfigLibrary {
            configs: vec![DisplayConfig::new(
                "Plan 1".to_string(),
                "Architektur".to_string(),
                PlanningStage::Design,
                ViewType::FloorPlan,
            )],
            ..Default::default()
        });

        app.aec_style_manager_wall_style_editing_id = Some("style1".to_string());

        // 1) Select profile, toggle visibility off for AxisLine
        let _ = app.update(Message::AecStyleManagerProfileSelect("Plan 1".to_string()));

        let _ = app.update(Message::AecStyleManagerProfileSlotVisibilityToggle(
            WallComponentSlot::AxisLine,
            false,
        ));
        let _ = app.update(Message::AecStyleManagerProfileSave);

        // 2) Verify persistence in the library
        let saved_style = app
            .aec_project_explorer_file
            .as_ref()
            .unwrap()
            .material_wall_style_library
            .wall_styles
            .iter()
            .find(|w| w.style.id == "style1")
            .unwrap();
        let rules = saved_style.display_profiles.get("Plan 1").unwrap();
        assert!(!rules.is_visible(WallComponentSlot::AxisLine));
        assert!(rules.is_visible(WallComponentSlot::Contour2D)); // Unchanged default

        // 3) Verify reloading
        let _ = app.update(Message::AecStyleManagerProfileSelect("Plan 1".to_string()));
        assert!(!app
            .aec_style_manager_profile_slot_visibility
            .get(&WallComponentSlot::AxisLine)
            .copied()
            .unwrap_or(true));
        assert!(app
            .aec_style_manager_profile_slot_visibility
            .get(&WallComponentSlot::Contour2D)
            .copied()
            .unwrap_or(true));
    }
}
