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
        if grip_id > 4 {
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

    /// Interactive grip handler for DIN 1356 Section and Elevation markers.
    /// Updates polyline vertices and marker XDATA in-place and renders preview wires
    /// during mouse movement, deferring view regeneration to commit time.
    pub(crate) fn apply_aec_section_marker_grip(
        &mut self,
        tab: usize,
        handle: acadrust::Handle,
        grip_id: usize,
        apply: &crate::scene::model::object::GripApply,
        orig_ent: Option<&acadrust::EntityType>,
    ) -> bool {
        let scene = &mut self.tabs[tab].scene;
        let marker_handle = crate::modules::aec::properties::resolve_section_marker_package(
            scene,
            handle,
        )
        .unwrap_or(handle);

        let Some(ent) = scene.document.get_entity(marker_handle) else {
            return false;
        };
        let is_marker = crate::modules::aec::views::din1356::read_section_marker(&scene.document, ent).is_some();
        if !is_marker {
            return false;
        }

        let orig_marker = orig_ent.and_then(|e| {
            crate::modules::aec::views::din1356::read_section_marker(&scene.document, e)
        });

        if let Some(res) = crate::modules::aec::views::din1356::apply_section_marker_grip_pure(
            &mut scene.document,
            marker_handle,
            grip_id,
            apply,
            orig_marker.as_ref(),
        ) {
            let mut wire_models = Vec::new();
            for (p1, p2) in res.preview_wires {
                let pts = vec![
                    [p1.x_m, p1.y_m, res.marker.z_min],
                    [p2.x_m, p2.y_m, res.marker.z_min],
                ];
                wire_models.push(crate::scene::model::wire_model::WireModel::solid_f64(
                    "marker_grip_preview".into(),
                    pts,
                    crate::scene::model::wire_model::WireModel::CYAN,
                    false,
                ));
            }
            scene.set_preview_wires(wire_models);
            self.tabs[tab].dirty = true;
        }
        true
    }

    /// Interactive grip handler for derived section and elevation primitives.
    /// Computes semantic mutation in real-time and renders preview wires
    /// during mouse movement, storing the pending mutation on AecState to commit on mouse release.
    pub(crate) fn apply_aec_derived_view_grip(
        &mut self,
        tab: usize,
        handle: acadrust::Handle,
        grip_id: usize,
        apply: &crate::scene::model::object::GripApply,
        orig_ent: Option<&acadrust::EntityType>,
    ) -> bool {
        if grip_id < crate::modules::aec::properties::DERIVED_VIEW_GRIP_BASE {
            return false;
        }

        let scene = &self.tabs[tab].scene;
        let Some(carrier) = crate::modules::aec::engine::views::carrier::find_view_carrier_owning_handle(&scene.document, handle) else {
            return false;
        };

        let Some((viewpoint, _model_view)) = crate::modules::aec::engine::views::carrier::read_view_carrier_definitions(&scene.document, carrier.handle) else {
            return false;
        };

        let Some(entity) = scene.document.get_entity(handle) else {
            return false;
        };
        let effective_ent = orig_ent.unwrap_or(entity);
        let Some(meta) = crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(effective_ent) else {
            return false;
        };
        let Some(feat) = meta.feature else {
            return false;
        };

        // Determine view delta in local carrier coordinate system
        let original_center = match effective_ent {
            acadrust::EntityType::Line(l) => {
                glam::DVec3::new(
                    (l.start.x + l.end.x) * 0.5,
                    (l.start.y + l.end.y) * 0.5,
                    (l.start.z + l.end.z) * 0.5,
                )
            }
            acadrust::EntityType::LwPolyline(pl) => {
                if pl.vertices.is_empty() {
                    return false;
                }
                let mut sx = 0.0;
                let mut sy = 0.0;
                for v in &pl.vertices {
                    sx += v.location.x;
                    sy += v.location.y;
                }
                let n = pl.vertices.len() as f64;
                glam::DVec3::new(sx / n, sy / n, pl.elevation)
            }
            _ => return false,
        };

        let new_world_pos = match apply {
            crate::scene::model::object::GripApply::Absolute(pt) => *pt,
            crate::scene::model::object::GripApply::Translate(delta) => original_center + *delta,
        };

        let world_delta = new_world_pos - original_center;

        // Build preview wire points before ending immutable borrow
        let mut wire_pts = Vec::new();
        match effective_ent {
            acadrust::EntityType::Line(l) => {
                wire_pts.push([l.start.x + world_delta.x, l.start.y + world_delta.y, l.start.z]);
                wire_pts.push([l.end.x + world_delta.x, l.end.y + world_delta.y, l.end.z]);
            }
            acadrust::EntityType::LwPolyline(pl) => {
                for v in &pl.vertices {
                    wire_pts.push([v.location.x + world_delta.x, v.location.y + world_delta.y, pl.elevation]);
                }
            }
            _ => {}
        }

        let cos_r = carrier.rotation_rad.cos();
        let sin_r = carrier.rotation_rad.sin();
        let scale = if carrier.scale.abs() > 1e-6 { carrier.scale } else { 1.0 };

        let unrot_x = cos_r * world_delta.x + sin_r * world_delta.y;
        let unrot_y = -sin_r * world_delta.x + cos_r * world_delta.y;
        let view_delta = crate::modules::aec::engine::views::Vector2::new(unrot_x / scale, unrot_y / scale);

        let elem_ref = match meta.element_ref {
            Some(r) => r,
            None => return false,
        };

        let collector = crate::modules::aec::engine::views::scene_collector::SceneGeometryCollector::collect_from_document(
            &scene.document,
            elem_ref.model_id,
        );
        let (mut action, direction_hint) = match feat {
            crate::modules::aec::engine::views::provenance::FeatureKey::WallTop => {
                (crate::modules::aec::engine::views::provenance::ModelEditAction::SetWallHeight, crate::modules::aec::engine::views::Vector2::new(0.0, 1.0))
            }
            crate::modules::aec::engine::views::provenance::FeatureKey::WallBase => {
                (crate::modules::aec::engine::views::provenance::ModelEditAction::ChangeParameter(crate::modules::aec::engine::views::provenance::ModelParameter::WallBaseOffset), crate::modules::aec::engine::views::Vector2::new(0.0, -1.0))
            }
            crate::modules::aec::engine::views::provenance::FeatureKey::OpeningSill => {
                (crate::modules::aec::engine::views::provenance::ModelEditAction::SetOpeningSillHeight, crate::modules::aec::engine::views::Vector2::new(0.0, 1.0))
            }
            crate::modules::aec::engine::views::provenance::FeatureKey::OpeningHead => {
                (crate::modules::aec::engine::views::provenance::ModelEditAction::ChangeParameter(crate::modules::aec::engine::views::provenance::ModelParameter::OpeningHeadHeight), crate::modules::aec::engine::views::Vector2::new(0.0, 1.0))
            }
            crate::modules::aec::engine::views::provenance::FeatureKey::OpeningLeftJamb
            | crate::modules::aec::engine::views::provenance::FeatureKey::OpeningRightJamb => {
                (crate::modules::aec::engine::views::provenance::ModelEditAction::MoveAlongHost {
                    host: elem_ref,
                    axis_world: crate::modules::aec::engine::views::Vector3::new(1.0, 0.0, 0.0),
                }, crate::modules::aec::engine::views::Vector2::new(1.0, 0.0))
            }
            crate::modules::aec::engine::views::provenance::FeatureKey::SlabTop => {
                (crate::modules::aec::engine::views::provenance::ModelEditAction::SetSlabElevation, crate::modules::aec::engine::views::Vector2::new(0.0, 1.0))
            }
            crate::modules::aec::engine::views::provenance::FeatureKey::SlabBottom => {
                (crate::modules::aec::engine::views::provenance::ModelEditAction::SetSlabThickness, crate::modules::aec::engine::views::Vector2::new(0.0, -1.0))
            }
            _ => return false,
        };

        if grip_id == crate::modules::aec::properties::DERIVED_VIEW_RESIZE_GRIP_BASE {
            action = crate::modules::aec::engine::views::provenance::ModelEditAction::ChangeParameter(
                crate::modules::aec::engine::views::provenance::ModelParameter::OpeningWidth,
            );
        }

        let src = crate::modules::aec::engine::views::provenance::SourceReference {
            element: elem_ref,
            xref_source: meta.xref_name.map(|n| crate::modules::aec::engine::views::provenance::XrefSourceRef::identity(n)),
            component_id: None,
            feature: Some(feat.clone()),
            display_component_id: None,
        };

        let edit_handle = crate::modules::aec::engine::views::provenance::EditHandle {
            id: format!("grip_{}", meta.index),
            position: crate::modules::aec::engine::views::Point2::new(0.0, 0.0),
            action,
            direction_hint,
            target_feature: Some(feat),
            source: src,
        };

        let mutation = match crate::modules::aec::engine::views::resolve_handle_drag(
            &edit_handle,
            &viewpoint,
            crate::modules::aec::engine::views::Point2::new(view_delta.x, view_delta.y),
            &collector,
        ) {
            Ok(m) => m,
            Err(_) => return false,
        };

        if !wire_pts.is_empty() {
            let wire = crate::scene::model::wire_model::WireModel::solid_f64(
                "derived_view_grip_preview".into(),
                wire_pts,
                crate::scene::model::wire_model::WireModel::CYAN,
                false,
            );
            self.tabs[tab].scene.set_preview_wires(vec![wire]);
            self.tabs[tab].dirty = true;
        }

        self.aec.aec_pending_derived_view_grip = Some((carrier.handle, mutation));
        true
    }

    /// Interactive grip handler for DerivedViewCarrier entities.
    /// Handles insertion relocation (grip 60_000) and cut-height adjustments (grip 60_001)
    /// with real-time preview wires, deferring regeneration to commit time.
    pub(crate) fn apply_aec_carrier_grip(
        &mut self,
        tab: usize,
        handle: acadrust::Handle,
        grip_id: usize,
        apply: &crate::scene::model::object::GripApply,
    ) -> bool {
        if grip_id != crate::modules::aec::properties::CARRIER_GRIP_INSERTION
            && grip_id != crate::modules::aec::properties::CARRIER_GRIP_CUT_HEIGHT
        {
            return false;
        }

        let scene = &self.tabs[tab].scene;
        let Some(entity) = scene.document.get_entity(handle) else {
            return false;
        };
        let Some(carrier) = crate::modules::aec::engine::views::carrier::read_view_carrier(&scene.document, entity) else {
            return false;
        };

        match grip_id {
            crate::modules::aec::properties::CARRIER_GRIP_INSERTION => {
                let orig = glam::DVec3::new(carrier.insertion_point.x_m, carrier.insertion_point.y_m, 0.0);
                let new_pos = match apply {
                    crate::scene::model::object::GripApply::Absolute(pt) => *pt,
                    crate::scene::model::object::GripApply::Translate(delta) => orig + *delta,
                };
                let new_ins = crate::modules::aec::engine::views::Point2::new(new_pos.x, new_pos.y);
                self.aec.aec_pending_carrier_insertion = Some((carrier.handle, new_ins));

                let wire = crate::scene::model::wire_model::WireModel::solid_f64(
                    "carrier_insertion_grip_preview".into(),
                    vec![
                        [orig.x, orig.y, 0.0],
                        [new_pos.x, new_pos.y, 0.0],
                    ],
                    crate::scene::model::wire_model::WireModel::CYAN,
                    false,
                );
                self.tabs[tab].scene.set_preview_wires(vec![wire]);
                self.tabs[tab].dirty = true;
                true
            }
            crate::modules::aec::properties::CARRIER_GRIP_CUT_HEIGHT => {
                let Some((vp, _)) = crate::modules::aec::engine::views::carrier::read_view_carrier_definitions(&scene.document, carrier.handle) else {
                    return false;
                };
                let current_cut_h = match &vp.kind {
                    crate::modules::aec::engine::views::viewpoint::ViewKind::FloorPlan { range } => {
                        (vp.z_range.z_min.get() + range.below_cut.get()).clamp(vp.z_range.z_min.get(), vp.z_range.z_max.get())
                    }
                    crate::modules::aec::engine::views::viewpoint::ViewKind::ReflectedCeilingPlan { range } => {
                        (vp.z_range.z_max.get() - range.above_cut.get()).clamp(vp.z_range.z_min.get(), vp.z_range.z_max.get())
                    }
                    _ => return false,
                };

                let orig_grip = glam::DVec3::new(carrier.insertion_point.x_m, carrier.insertion_point.y_m + current_cut_h, 0.0);
                let new_grip_pos = match apply {
                    crate::scene::model::object::GripApply::Absolute(pt) => *pt,
                    crate::scene::model::object::GripApply::Translate(delta) => orig_grip + *delta,
                };

                let new_cut_h = new_grip_pos.y - carrier.insertion_point.y_m;
                let z_min = vp.z_range.z_min.get();
                let z_max = vp.z_range.z_max.get();
                let clamped_h = new_cut_h.clamp(z_min, z_max);
                self.aec.aec_pending_carrier_cut_height = Some((carrier.handle, clamped_h));

                let x0 = carrier.insertion_point.x_m - 2.0;
                let x1 = carrier.insertion_point.x_m + 12.0;
                let y = carrier.insertion_point.y_m + clamped_h;
                let wire = crate::scene::model::wire_model::WireModel::solid_f64(
                    "carrier_cut_height_preview".into(),
                    vec![
                        [x0, y, 0.0],
                        [x1, y, 0.0],
                    ],
                    crate::scene::model::wire_model::WireModel::CYAN,
                    false,
                );
                self.tabs[tab].scene.set_preview_wires(vec![wire]);
                self.tabs[tab].dirty = true;
                true
            }
            _ => false,
        }
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
