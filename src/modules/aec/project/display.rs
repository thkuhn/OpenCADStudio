//! Active DisplayConfig resolve/apply/regenerate for wall packages.
//!
//! Bodies live here (still `impl OpenCADStudio`) so Core `update/mod.rs`
//! does not grow AEC display-config logic.

use crate::app::OpenCADStudio;
use crate::modules::aec::engine::wall_package;
use crate::modules::aec::engine::xdata;

impl OpenCADStudio {
    /// Persists `lib` as the effective `DisplayConfig` library, analogous
    /// to [`Self::aec_save_style_library_preferring_project`].
    pub(crate) fn aec_save_display_config_library_preferring_project(
        &mut self,
        lib: &crate::modules::aec::engine::library::DisplayConfigLibrary,
    ) -> Result<(), String> {
        if let (Some(project), Some(path)) = (
            self.aec.aec_project_explorer_file.as_mut(),
            self.aec.aec_project_explorer_path.clone(),
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

    /// Resolves `tabs[tab_index]`'s active `DisplayConfig` (`active_display_config`
    /// + `aec_plan_library`), if any, into the wall `ComponentRuleSet` and
    /// `style_substitutions` map that must be threaded through wall-mutating
    /// regenerations (join/extend/reverse/opening as well as style
    /// assignment) so they honor the currently active plan type. Returns owned
    /// data (rather than borrowing `self.tabs`/`self.aec.aec_plan_library`) so
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
        let Some(wall) = crate::modules::aec::engine::xdata::wall_from_entity(entity) else {
            return (None, None);
        };
        let style_library = crate::modules::aec::engine::project::resolve_style_library(
            self.aec.aec_project_explorer_file.as_ref(),
        );
        let style = style_library
            .wall_styles
            .iter()
            .find(|ws| ws.style.id == wall.style_id);
        let config = config_name.as_deref().and_then(|name| {
            self.aec.aec_plan_library
                .as_ref()
                .and_then(|lib| lib.find(name))
                .cloned()
                .or_else(|| {
                    crate::modules::aec::engine::project::resolve_display_config_library(
                        self.aec.aec_project_explorer_file.as_ref(),
                    )
                    .find(name)
                    .cloned()
                })
        });
        // A named plan that cannot be resolved must not fall back to an empty
        // `DisplayConfig` (`RepresentationMode::All`): that would redraw a
        // newly created wall fully visible and ignore the active plan type.
        let config = match config {
            Some(config) => config,
            None if session.is_some() => {
                crate::modules::aec::engine::plan_view::DisplayConfig::new(
                    String::new(),
                    String::new(),
                    crate::modules::aec::engine::plan_view::PlanningStage::Design,
                    crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
                )
            }
            None if config_name.is_none() => {
                crate::modules::aec::engine::plan_view::DisplayConfig::new(
                    String::new(),
                    String::new(),
                    crate::modules::aec::engine::plan_view::PlanningStage::Design,
                    crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
                )
            }
            None => return (None, None),
        };
        let mut rules = crate::modules::aec::engine::library::build_effective_rule_set(
            &config,
            style,
            session,
        );
        let filter_result = crate::modules::aec::engine::display_apply::apply_phase_filter(
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

    pub(crate) fn apply_active_display_config_to_tab(&mut self, tab_index: usize) {
        if self.tabs[tab_index].is_start {
            return;
        }
        let session = self.tabs[tab_index].representation_override;
        let name = self.tabs[tab_index].active_display_config.clone();
        let style_library = crate::modules::aec::engine::project::resolve_style_library(
            self.aec.aec_project_explorer_file.as_ref(),
        );
        let config = name.as_deref().and_then(|n| {
            self.aec.aec_plan_library
                .get_or_insert_with(|| {
                    crate::modules::aec::engine::project::resolve_display_config_library(
                        self.aec.aec_project_explorer_file.as_ref(),
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
        crate::modules::aec::engine::display_apply::apply_display_config_to_scene_with_representation(
            &mut self.tabs[tab_index].scene,
            &config,
            Some(&style_library),
            session,
        );
    }

    /// Regenerates a single wall's representation the same way
    /// [`crate::modules::aec::engine::wall_regen::regenerate_wall_representation`]
    /// does, but additionally honors `tabs[tab_index]`'s active
    /// `DisplayConfig` (`active_display_config` + `aec_plan_library`), if
    /// any — mirroring what [`crate::modules::aec::engine::display_apply::apply_display_config_to_scene`]
    /// does when a `DisplayConfig` is (re-)applied to the whole document.
    /// Without this, per-wall regenerations triggered from the AEC Style
    /// Manager (assigning/saving a wall style) would silently ignore the
    /// currently active plan and fall back to the default, fully-visible
    /// representation.
    pub(crate) fn regenerate_wall_respecting_active_display_config(
        &mut self,
        tab_index: usize,
        wall_handle: acadrust::Handle,
    ) -> Result<Vec<acadrust::Handle>, crate::modules::aec::engine::wall_regen::WallRegenError> {
        let style_library = crate::modules::aec::engine::project::resolve_style_library(
            self.aec.aec_project_explorer_file.as_ref(),
        );
        let (rules, substitutions) =
            self.resolve_active_display_config_wall_rules(tab_index, Some(wall_handle));
        crate::modules::aec::engine::wall_regen::regenerate_wall_representation_with_rules_and_substitutions(
            &mut self.tabs[tab_index].scene,
            wall_handle,
            rules.as_ref(),
            substitutions.as_ref(),
            Some(&style_library),
        )
    }

    /// Removes a wall opening while honoring `tabs[tab_index]`'s active
    /// `DisplayConfig` for the host wall's regeneration.
    pub(crate) fn remove_wall_opening_respecting_active_display_config(
        &mut self,
        tab_index: usize,
        opening_handle: acadrust::Handle,
    ) -> Result<Vec<acadrust::Handle>, String> {
        let style_library = crate::modules::aec::engine::project::resolve_style_library(
            self.aec.aec_project_explorer_file.as_ref(),
        );
        let host_wall = self.tabs[tab_index]
            .scene
            .document
            .get_entity(opening_handle)
            .and_then(|entity| {
                crate::modules::aec::engine::opening_xdata::opening_from_entity(entity, opening_handle)
            })
            .map(|op| wall_package::resolve_wall_package(&self.tabs[tab_index].scene, op.host_wall));
        let (rules, substitutions) =
            self.resolve_active_display_config_wall_rules(tab_index, host_wall);
        crate::modules::aec::engine::opening_xdata::remove_wall_opening_with_rules_and_substitutions(
            &mut self.tabs[tab_index].scene,
            opening_handle,
            Some(&style_library),
            rules.as_ref(),
            substitutions.as_ref(),
        )
    }

    /// Interactively erase openings in `handles` whose host walls are not being
    /// deleted, unlinking them from host walls and regenerating those host walls
    /// according to the tab's active display configuration.
    pub(crate) fn aec_erase_openings_respecting_active_display_config(
        &mut self,
        tab_index: usize,
        handles: &[acadrust::Handle],
    ) {
        let mut openings_to_remove = Vec::new();
        let scene = &self.tabs[tab_index].scene;
        for &h in handles {
            let Some(opening_owner) =
                crate::modules::aec::engine::opening_display::opening_owner_if_any(scene, h)
            else {
                continue;
            };
            if openings_to_remove.contains(&opening_owner) {
                continue;
            }
            let Some(entity) = scene.document.get_entity(opening_owner) else {
                continue;
            };
            let Some(opening) =
                crate::modules::aec::engine::opening_xdata::opening_from_entity(entity, opening_owner)
            else {
                continue;
            };
            let host_wall =
                wall_package::resolve_wall_package(scene, opening.host_wall);
            // If the host wall is itself being deleted in `handles`, skip individual removal
            // as the entire wall and all its children will be erased together.
            if !handles.contains(&host_wall) {
                openings_to_remove.push(opening_owner);
            }
        }
        for opening_handle in openings_to_remove {
            let _ = self.remove_wall_opening_respecting_active_display_config(tab_index, opening_handle);
        }
    }

    /// When Face3D preview entities for control planes or facets are deleted in the drawing,
    /// remove the corresponding facet assignments from the control plane in the project manager.
    pub(crate) fn aec_erase_control_planes_or_facets(
        &mut self,
        tab_index: usize,
        handles: &[acadrust::Handle],
    ) {
        let scene = &self.tabs[tab_index].scene;
        let mut facets_to_remove: Vec<(uuid::Uuid, usize)> = Vec::new();
        for &h in handles {
            if let Some(entity) = scene.document.get_entity(h) {
                if let Some((plane_id, _name, facet_idx)) =
                    crate::modules::aec::project::preview::control_plane_facet_from_entity(entity)
                {
                    if let Some(idx) = facet_idx {
                        facets_to_remove.push((plane_id, idx));
                    }
                }
            }
        }
        if facets_to_remove.is_empty() {
            return;
        }
        facets_to_remove.sort_by(|a, b| b.1.cmp(&a.1));
        if let Some(project) = self.aec.aec_project_explorer_file.as_mut() {
            let mut modified = false;
            for (plane_id, idx) in facets_to_remove {
                for building in &mut project.buildings {
                    for storey in &mut building.storeys {
                        if let Some(plane) = storey.plane_mut(plane_id) {
                            if idx < plane.facets.len() {
                                plane.facets.remove(idx);
                                if let Some(first) = plane.facets.first() {
                                    plane.origin = first.origin();
                                    plane.normal = first.unit_normal();
                                }
                                modified = true;
                            }
                        }
                    }
                }
            }
            if modified {
                self.aec_project_explorer_persist_if_pathed();
            }
        }
    }

    /// Resets any temporary orange highlight on control plane or facet preview entities
    /// across all storeys in the active project back to their default colors (cyan / yellow).
    pub(crate) fn aec_reset_control_plane_highlights(&mut self) {
        if self.active_tab >= self.tabs.len() {
            return;
        }
        let tab = self.active_tab;
        let mut all_touched = Vec::new();
        if let Some(project) = self.aec.aec_project_explorer_file.as_ref() {
            for b in &project.buildings {
                for s in &b.storeys {
                    let touched = crate::modules::aec::project::preview::reset_control_plane_preview_colors(
                        &mut self.tabs[tab].scene,
                        s,
                    );
                    all_touched.extend(touched);
                }
            }
        }
        let changes: Vec<_> = all_touched
            .into_iter()
            .map(|h| (h, crate::scene::ChangeKind::Modified))
            .collect();
        if !changes.is_empty() {
            self.tabs[tab].scene.bump_entities(&changes);
        }
    }

    /// After a join (or a new wall segment that auto-joins), rebuild each
    /// participating wall with *its* plan-type/style profile. Shared join
    /// regenerations often pass `None` or the first wall's rules, which
    /// would otherwise drop the active display configuration.
    pub(crate) fn reapply_active_display_config_to_wall_packages(
        &mut self,
        tab_index: usize,
        seeds: &[acadrust::Handle],
    ) {
                let mut walls: Vec<acadrust::Handle> = Vec::new();
        for &seed in seeds {
            let axis = wall_package::resolve_wall_package(&self.tabs[tab_index].scene, seed);
            if axis.is_null() {
                continue;
            }
            if self.tabs[tab_index]
                .scene
                .document
                .get_entity(axis)
                .and_then(xdata::wall_from_entity)
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
}
