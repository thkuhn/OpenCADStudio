//! Combined style library, tab session install, upserts, and copy-on-write.
//!
//! Bodies live here (still `impl OpenCADStudio`) so Core `update/mod.rs`
//! does not grow AEC style-session logic.

use iced::Task;

use crate::app::{AecModalKind, AecPendingCopy, Message, OpenCADStudio};

impl OpenCADStudio {
    /// Persists `lib` as the effective material/wall-style library: into
    /// the loaded project (fanning out to every drawing/storey referencing
    /// it) when a project is loaded *and* pathed, otherwise falling back to
    /// the machine-wide global library file so standalone drawings (no
    /// project) keep working exactly as before.
    pub(crate) fn aec_save_style_library_preferring_project(
        &mut self,
        lib: &crate::modules::aec::engine::library::StyleLibrary,
    ) -> Result<(), String> {
        if let (Some(project), Some(path)) = (
            self.aec.aec_project_explorer_file.as_mut(),
            self.aec.aec_project_explorer_path.clone(),
        ) {
            crate::modules::aec::engine::project::save_style_library_to_project(
                project,
                &path,
                lib.clone(),
            )
            .map_err(|e| e.to_string())
        } else {
            // Standalone drawing: keep edits in the session library only.
            Ok(())
        }
    }

    pub(crate) fn aec_refresh_combined_style_library(&mut self) {
        let session = self
            .tabs
            .get(self.active_tab)
            .and_then(|t| t.aec_session_style_library().cloned());
        self.aec.aec_session_style_library = session;
        self.aec.aec_style_library = Some(
            crate::modules::aec::engine::library::combined_style_library_with_session(
                self.aec.aec_project_explorer_file.as_ref(),
                self.aec.aec_session_style_library.as_ref(),
            ),
        );
    }

    pub(crate) fn aec_install_session_styles_for_tab(&mut self, tab_index: usize) {
        let session = crate::modules::aec::properties::session_styles_for_scene(
            &self.tabs[tab_index].scene,
            self.aec.aec_project_explorer_file.as_ref(),
        );
        self.tabs[tab_index].set_aec_session_style_library(session);
        if tab_index == self.active_tab {
            self.aec_refresh_combined_style_library();
        }
    }

    pub(crate) fn aec_upsert_material_into_session(
        &mut self,
        material: crate::modules::aec::engine::material::Material,
    ) {
        let lib = self.tabs[self.active_tab]
            .aec_session_style_library_mut()
            .get_or_insert_with(crate::modules::aec::engine::library::StyleLibrary::empty);
        lib.upsert_material(material);
        self.aec_refresh_combined_style_library();
        self.command_line.push_info(
            crate::t!(
                "AEC Style Manager: saved in this drawing session only (not written to a project or the standard library)."
            )
            .as_ref(),
        );
    }

    pub(crate) fn aec_upsert_wall_style_into_session(
        &mut self,
        wall_style: crate::modules::aec::engine::wall_style::WallStyle,
    ) {
        let lib = self.tabs[self.active_tab]
            .aec_session_style_library_mut()
            .get_or_insert_with(crate::modules::aec::engine::library::StyleLibrary::empty);
        lib.upsert_wall_style(wall_style);
        self.aec_refresh_combined_style_library();
        self.command_line.push_info(
            crate::t!(
                "AEC Style Manager: saved in this drawing session only (not written to a project or the standard library)."
            )
            .as_ref(),
        );
    }

    pub(crate) fn aec_upsert_opening_style_into_session(
        &mut self,
        opening_style: crate::modules::aec::engine::opening_style::OpeningStyle,
    ) {
        let lib = self.tabs[self.active_tab]
            .aec_session_style_library_mut()
            .get_or_insert_with(crate::modules::aec::engine::library::StyleLibrary::empty);
        lib.upsert_opening_style(opening_style);
        self.aec_refresh_combined_style_library();
        self.command_line.push_info(
            crate::t!(
                "AEC Style Manager: saved in this drawing session only (not written to a project or the standard library)."
            )
            .as_ref(),
        );
    }

    /// Upserts a single material into the active project's library (never the
    /// Standard library). Used for normal project edits and copy-on-write
    /// saves of Standard entries. Refreshes the combined in-memory view.
    pub(crate) fn aec_upsert_material_into_project(
        &mut self,
        material: crate::modules::aec::engine::material::Material,
    ) -> Result<(), String> {
        let path = self.aec.aec_project_explorer_path.clone();
        let Some(project) = self.aec.aec_project_explorer_file.as_mut() else {
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
        self.aec_refresh_combined_style_library();
        Ok(())
    }

    /// Upserts a single wall style into the active project's library (never
    /// the Standard library). See [`Self::aec_upsert_material_into_project`].
    pub(crate) fn aec_upsert_wall_style_into_project(
        &mut self,
        wall_style: crate::modules::aec::engine::wall_style::WallStyle,
    ) -> Result<(), String> {
        let path = self.aec.aec_project_explorer_path.clone();
        let Some(project) = self.aec.aec_project_explorer_file.as_mut() else {
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
        self.aec_refresh_combined_style_library();
        Ok(())
    }

    /// Upserts a single opening style into the active project's library.
    pub(crate) fn aec_upsert_opening_style_into_project(
        &mut self,
        opening_style: crate::modules::aec::engine::opening_style::OpeningStyle,
    ) -> Result<(), String> {
        let path = self.aec.aec_project_explorer_path.clone();
        let Some(project) = self.aec.aec_project_explorer_file.as_mut() else {
            return Err("no project loaded".to_string());
        };
        project
            .material_wall_style_library
            .upsert_opening_style(opening_style);
        let lib = project.material_wall_style_library.clone();
        if let Some(path) = path {
            crate::modules::aec::engine::project::save_style_library_to_project(
                project, &path, lib,
            )
            .map_err(|e| e.to_string())?;
        }
        self.aec_refresh_combined_style_library();
        Ok(())
    }

    /// Copies the currently-selected material between the project and
    /// global libraries (`to_project == true` copies global→project,
    /// `false` copies project→global). Shows an overwrite confirmation
    /// first if the target already holds a different entry with the same
    /// id (Step 9).
    pub(crate) fn aec_handle_copy_material(&mut self, to_project: bool) -> Task<Message> {
        if to_project && self.aec.aec_project_explorer_file.is_none() {
            return Task::none();
        }
        let Some(id) = self.aec.aec_style_manager_selected_material.clone() else {
            return Task::none();
        };
        let global_lib = crate::modules::aec::engine::library::load_or_seed();
        let project_lib = crate::modules::aec::engine::project::resolve_style_library(
            self.aec.aec_project_explorer_file.as_ref(),
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
                self.aec.aec_style_manager_pending_copy = Some(AecPendingCopy::Material {
                    material,
                    to_project,
                });
                self.aec.aec_style_manager_copy_conflict_open = true;
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::StyleCopyConflict));
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
    pub(crate) fn aec_handle_copy_wall_style(&mut self, to_project: bool) -> Task<Message> {
        if to_project && self.aec.aec_project_explorer_file.is_none() {
            return Task::none();
        }
        let Some(id) = self.aec.aec_style_manager_selected_wall_style.clone() else {
            return Task::none();
        };
        let global_lib = crate::modules::aec::engine::library::load_or_seed();
        let project_lib = crate::modules::aec::engine::project::resolve_style_library(
            self.aec.aec_project_explorer_file.as_ref(),
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
                self.aec.aec_style_manager_pending_copy = Some(AecPendingCopy::WallStyle {
                    wall_style,
                    to_project,
                });
                self.aec.aec_style_manager_copy_conflict_open = true;
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::StyleCopyConflict));
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
    pub(crate) fn aec_execute_copy(&mut self, pending: AecPendingCopy) {
        let global_lib_before = crate::modules::aec::engine::library::load_or_seed();
        let project_lib_before = crate::modules::aec::engine::project::resolve_style_library(
            self.aec.aec_project_explorer_file.as_ref(),
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
            AecPendingCopy::OpeningStyle { to_project, .. } => {
                let lib = if *to_project {
                    project_lib_before
                } else {
                    global_lib_before
                };
                (lib, *to_project, crate::t!("opening style"))
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
            AecPendingCopy::OpeningStyle { opening_style, .. } => {
                target_lib.upsert_opening_style(opening_style.clone());
            }
        }
        let save_result = if to_project {
            if let (Some(project), Some(path)) = (
                self.aec.aec_project_explorer_file.as_mut(),
                self.aec.aec_project_explorer_path.clone(),
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
        self.aec_refresh_combined_style_library();
    }

    pub(crate) fn aec_style_manager_material_save_internal(
        &mut self,
    ) -> Option<(String, crate::modules::aec::engine::library::StyleLibrary)> {
        let name = self.aec.aec_style_manager_material_name.trim().to_string();
        if name.is_empty() {
            self.command_line.push_error(
                crate::t!("AEC Style Manager: material name cannot be empty.").as_ref(),
            );
            return None;
        }
        let hatch = if self.aec.aec_style_manager_material_hatch.trim().is_empty() {
            "SOLID".to_string()
        } else {
            self.aec.aec_style_manager_material_hatch.trim().to_string()
        };
        let color_hex = self
            .aec.aec_style_manager_material_color
            .trim()
            .trim_start_matches('#');
        let color = u32::from_str_radix(color_hex, 16).unwrap_or(0);
        let line_type = if self.aec.aec_style_manager_material_line_type.trim().is_empty() {
            "Continuous".to_string()
        } else {
            self.aec.aec_style_manager_material_line_type.trim().to_string()
        };
        let category = {
            let c = self.aec.aec_style_manager_material_category.trim();
            if c.is_empty() {
                None
            } else {
                Some(c.to_string())
            }
        };
        let mut hatch_scale = self
            .aec.aec_style_manager_material_hatch_scale
            .trim()
            .parse::<f64>()
            .unwrap_or(1.0);
        if hatch_scale <= 0.0 {
            hatch_scale = 0.01;
        }
        let render_material_ref = {
            let r = self.aec.aec_style_manager_material_render_ref.trim();
            if r.is_empty() {
                None
            } else {
                Some(r.to_string())
            }
        };
        let hatch_angle = self
            .aec.aec_style_manager_material_hatch_angle
            .trim()
            .parse::<f64>()
            .unwrap_or(0.0);
        let hatch_angle_relative = self.aec.aec_style_manager_material_hatch_angle_relative;
        let id = self
            .aec.aec_style_manager_material_editing_id
            .clone()
            .unwrap_or_else(|| crate::modules::aec::engine::xdata::unique_id("mat", &name));

        let material = crate::modules::aec::engine::material::Material {
            id: id.clone(),
            name,
            hatch_pattern: hatch,
            line_color: color,
            line_type,
            render_material_ref,
            category,
            hatch_color: Some(acadrust::types::Color::Rgb {
                r: ((self.aec.aec_style_manager_material_hatch_color >> 16) & 0xFF) as u8,
                g: ((self.aec.aec_style_manager_material_hatch_color >> 8) & 0xFF) as u8,
                b: (self.aec.aec_style_manager_material_hatch_color & 0xFF) as u8,
            }),
            hatch_scale,
            hatch_angle,
            hatch_angle_relative,
        };

        let source = crate::modules::aec::engine::library::material_library_source_with_session(
            self.aec.aec_project_explorer_file.as_ref(),
            self.aec.aec_session_style_library.as_ref(),
            &id,
        );
        let cow_from_standard = source
            == Some(crate::modules::aec::engine::library::LibrarySource::Standard);

        if self.aec.aec_project_explorer_file.is_some() {
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
                    self.aec.aec_style_manager_selected_material = Some(id.clone());
                    self.aec.aec_style_manager_material_editing_id = Some(id.clone());
                    let lib_snapshot = self
                        .aec
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
            self.aec_upsert_material_into_session(material);
            self.aec.aec_style_manager_selected_material = Some(id.clone());
            self.aec.aec_style_manager_material_editing_id = Some(id.clone());
            let lib_snapshot = self
                .aec
                .aec_style_library
                .clone()
                .unwrap_or_else(crate::modules::aec::engine::library::StyleLibrary::empty);
            Some((id, lib_snapshot))
        }
    }

    pub(crate) fn aec_style_manager_wall_style_save_internal(
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
        let name = self.aec.aec_style_manager_wall_style_name.trim().to_string();
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
            .aec.aec_style_manager_wall_style_editing_id
            .clone()
            .unwrap_or_else(|| crate::modules::aec::engine::xdata::unique_id("style", &name));

        // Cycle detection before saving
        if let Some(parent_id) = &self.aec.aec_style_manager_wall_style_parent {
            if parent_id == &id {
                self.command_line.push_error(
                    crate::t!("AEC Style Manager: a wall style cannot be its own parent.")
                        .as_ref(),
                );
                return None;
            }

            if let Some(lib) = &self.aec.aec_style_library {
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
        for (idx, lb) in self.aec.aec_style_manager_wall_style_layers.iter().enumerate() {
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
            .aec.aec_style_library
            .as_ref()
            .and_then(|lib| lib.wall_styles.iter().find(|ws| ws.style.id == id))
            .map(|ws| ws.display_profiles.clone())
            .unwrap_or_default();

        let wall_style = crate::modules::aec::engine::wall_style::WallStyle {
            style: crate::modules::aec::engine::style::Style {
                id: id.clone(),
                name,
                object_kind: "Wall".to_string(),
                parent_style_id: self.aec.aec_style_manager_wall_style_parent.clone(),
            },
            layers,
            display_profiles: existing_display_profiles,
        };

        // Copy-on-write: editing a Standard wall style lands in the project.
        let source = crate::modules::aec::engine::library::wall_style_library_source_with_session(
            self.aec.aec_project_explorer_file.as_ref(),
            self.aec.aec_session_style_library.as_ref(),
            &id,
        );
        let cow_from_standard = source
            == Some(crate::modules::aec::engine::library::LibrarySource::Standard);

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
                            crate::t!("AEC Style Manager: wall style saved.").as_ref(),
                        );
                    }
                    self.aec.aec_style_manager_selected_wall_style = Some(id.clone());
                    self.aec.aec_style_manager_wall_style_editing_id = Some(id.clone());
                    let lib_snapshot = self
                        .aec.aec_style_library
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
            self.aec_upsert_wall_style_into_session(wall_style);
            self.aec.aec_style_manager_selected_wall_style = Some(id.clone());
            self.aec.aec_style_manager_wall_style_editing_id = Some(id.clone());
            let lib_snapshot = self
                .aec.aec_style_library
                .clone()
                .unwrap_or_else(crate::modules::aec::engine::library::StyleLibrary::empty);
            Some((id, lib_snapshot))
        }
    }

    pub(crate) fn clear_aec_profile_slot_style_editor_buffers(&mut self) {
        self.aec.aec_style_manager_profile_slot_style_line_type.clear();
        self.aec.aec_style_manager_profile_slot_style_line_color.clear();
        self.aec.aec_style_manager_profile_slot_style_hatch_pattern.clear();
        self.aec.aec_style_manager_profile_slot_style_hatch_color.clear();
        self.aec.aec_style_manager_profile_slot_style_fill_color.clear();
        self.aec.aec_style_manager_profile_slot_style_line_color_picker_open = false;
        self.aec.aec_style_manager_profile_slot_style_hatch_color_picker_open = false;
        self.aec.aec_style_manager_profile_slot_style_fill_color_picker_open = false;
    }
}
