//! Wall-axis layer visibility for the current editing session.
//!
//! Bodies live here (still `impl OpenCADStudio`) so Core `update/mod.rs`
//! does not grow AEC session logic.

use crate::app::OpenCADStudio;

impl OpenCADStudio {
    /// Apply one interactive opening grip without letting the native POINT
    /// grip handler move the host marker independently of its XDATA.
    pub(crate) fn apply_aec_opening_grip(
        &mut self,
        tab: usize,
        handle: acadrust::Handle,
        grip_id: usize,
        apply: &crate::scene::model::object::GripApply,
    ) -> bool {
        if grip_id > 3 {
            return false;
        }
        let scene = &self.tabs[tab].scene;
        let Some(owner) =
            crate::modules::aec::engine::opening_display::opening_owner_if_any(scene, handle)
        else {
            return false;
        };
        let Some(entity) = scene.document.get_entity(owner) else {
            return false;
        };
        let Some(mut opening) =
            crate::modules::aec::engine::opening_xdata::opening_from_entity(entity, owner)
        else {
            return false;
        };
        let wall = crate::modules::aec::engine::wall_package::resolve_wall_package(
            scene,
            opening.host_wall,
        );
        let axis: Vec<(f64, f64)> =
            crate::modules::aec::engine::xdata::get_wall_vertices(scene, wall)
                .iter()
                .map(|v| (v.x, v.y))
                .collect();
        if axis.len() < 2 {
            return false;
        }

        let world = match apply {
            crate::scene::model::object::GripApply::Absolute(point) => *point,
            crate::scene::model::object::GripApply::Translate(delta) => {
                let Some(acadrust::EntityType::Point(point)) = scene.document.get_entity(owner)
                else {
                    return false;
                };
                glam::DVec3::new(point.location.x, point.location.y, point.location.z) + *delta
            }
        };
        crate::modules::aec::engine::opening_display::apply_opening_axis_grip(
            &axis,
            &mut opening,
            grip_id,
            world,
        );

        crate::modules::aec::engine::opening_xdata::write_opening_instance(
            &mut self.tabs[tab].scene,
            &opening,
        );
        let base_z = crate::modules::aec::engine::opening_display::host_base_z(
            &self.tabs[tab].scene,
            wall,
        );
        crate::modules::aec::engine::opening_display::sync_opening_point_to_axis(
            &mut self.tabs[tab].scene,
            &opening,
            &axis,
            base_z,
        );

        let style_library = crate::modules::aec::engine::project::resolve_style_library(
            self.aec.aec_project_explorer_file.as_ref(),
        );
        let (rules, _) =
            self.resolve_active_display_config_wall_rules(tab, Some(opening.host_wall));
        let thickness = crate::modules::aec::engine::opening_display::host_thickness(
            &self.tabs[tab].scene,
            wall,
        );
        let preview_wires = crate::modules::aec::engine::opening_display::preview_opening_wires(
            &axis,
            thickness,
            &opening,
            Some(&style_library),
            rules.as_ref(),
            base_z,
        );
        self.tabs[tab].scene.set_preview_wires(preview_wires);

        self.tabs[tab].dirty = true;
        true
    }

    pub(crate) fn remember_last_wall_defaults(
        &mut self,
        tab_index: usize,
        handle: acadrust::Handle,
    ) {
        let scene = &self.tabs[tab_index].scene;
        let axis = crate::modules::aec::engine::wall_package::resolve_wall_package(scene, handle);
        let Some(entity) = scene.document.get_entity(axis) else {
            return;
        };
        let Some(wall) = crate::modules::aec::engine::xdata::wall_from_entity(entity) else {
            return;
        };
        if !wall.style_id.is_empty() {
            self.aec.aec_last_wall_style_id = Some(wall.style_id.clone());
        }
        self.aec.aec_last_wall_height = Some(wall.height);
    }

    /// Live-commit hook: walls erase preview companions, regen, display-config,
    /// and last-defaults. Non-walls are a no-op.
    pub(crate) fn aec_on_live_entity_finished(
        &mut self,
        tab: usize,
        handle: acadrust::Handle,
        companions: &[acadrust::Handle],
    ) {
        let is_wall = self.tabs[tab]
            .scene
            .document
            .get_entity(handle)
            .is_some_and(|e| {
                crate::modules::aec::engine::wall_regen::wall_thickness_and_height(e).is_some()
            });
        if !is_wall {
            let owner = crate::modules::aec::engine::opening_display::opening_owner_if_any(
                &self.tabs[tab].scene,
                handle,
            );
            if let Some(owner) = owner {
                let style_library = crate::modules::aec::engine::project::resolve_style_library(
                    self.aec.aec_project_explorer_file.as_ref(),
                );
                let (rules, _) =
                    self.resolve_active_display_config_wall_rules(tab, Some(owner));
                crate::modules::aec::engine::opening_display::sync_opening_from_point_location(
                    &mut self.tabs[tab].scene,
                    owner,
                    Some(&style_library),
                    rules.as_ref(),
                );
            }
            return;
        }
        crate::modules::aec::engine::wall_regen::erase_wall_live_preview_companions(
            &mut self.tabs[tab].scene,
            handle,
            companions,
        );
        let _ = self.regenerate_wall_respecting_active_display_config(tab, handle);
        self.reapply_active_display_config_to_wall_packages(tab, &[handle]);
        self.remember_last_wall_defaults(tab, handle);
    }

    /// During `AEC_WALL` the axis layer stays on regardless of plan type.
    /// After the command ends, restore idle visibility from the active
    /// DisplayConfig (`WallComponentKind::Axis` / AxisLine slot).
    fn handle_is_wall_package(&self, tab_index: usize, handle: acadrust::Handle) -> bool {
        let scene = &self.tabs[tab_index].scene;
        let axis = crate::modules::aec::engine::wall_package::resolve_wall_package(scene, handle);
        scene
            .document
            .get_entity(axis)
            .and_then(crate::modules::aec::engine::xdata::wall_from_entity)
            .is_some()
    }

    fn tab_is_editing_wall_axis(&self, tab_index: usize) -> bool {
        let Some(tab) = self.tabs.get(tab_index) else {
            return false;
        };
        if tab.active_cmd.as_ref().is_some_and(|cmd| {
            matches!(
                cmd.name(),
                "AEC_WALL" | "AEC_WALLJOIN" | "AEC_WALLEXTEND"
            )
        }) {
            return true;
        }
        let transform_edit = tab.active_cmd.as_ref().is_some_and(|cmd| {
            matches!(cmd.name(), "MOVE" | "STRETCH" | "ROTATE" | "SCALE")
        });
        if transform_edit
            && tab
                .scene
                .selected
                .iter()
                .any(|&h| self.handle_is_wall_package(tab_index, h))
        {
            return true;
        }
        if let Some(grip) = tab.active_grip() {
            if grip
                .targets
                .iter()
                .any(|t| self.handle_is_wall_package(tab_index, t.handle))
            {
                return true;
            }
        }
        false
    }

    pub(crate) fn sync_wall_axis_layer_for_session(&mut self, tab_index: usize) {
        let keep_visible = self.tab_is_editing_wall_axis(tab_index);
        if keep_visible {
            crate::modules::aec::engine::wall_regen::set_wall_axis_layer_visible(
                &mut self.tabs[tab_index].scene,
                true,
            );
            self.tabs[tab_index].dirty = true;
            return;
        }
        let axis_kind = crate::modules::aec::engine::display_component::WallComponentKind::Axis;
        let axis_visible = self.tabs.get(tab_index).and_then(|tab| {
            let name = tab.active_display_config.as_deref()?;
            if let Some(config) = self.aec.aec_plan_library.as_ref().and_then(|lib| lib.find(name))
            {
                return Some(
                    config
                        .component_visibility
                        .get(&axis_kind)
                        .copied()
                        .unwrap_or(true),
                );
            }
            crate::modules::aec::engine::project::resolve_display_config_library(
                self.aec.aec_project_explorer_file.as_ref(),
            )
            .find(name)
            .map(|config| {
                config
                    .component_visibility
                    .get(&axis_kind)
                    .copied()
                    .unwrap_or(true)
            })
        })
        .unwrap_or(false);
        crate::modules::aec::engine::wall_regen::set_wall_axis_layer_visible(
            &mut self.tabs[tab_index].scene,
            axis_visible,
        );
    }
}
