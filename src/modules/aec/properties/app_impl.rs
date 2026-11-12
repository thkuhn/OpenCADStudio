use super::*;

impl crate::app::OpenCADStudio {
    /// Apply an AEC properties field. Returns `true` when `field` is AEC-owned
    /// so Core must skip the generic geom path.
    pub(crate) fn aec_apply_property_field(
        &mut self,
        tab: usize,
        handle: acadrust::Handle,
        field: &str,
        val: &str,
    ) -> bool {
        let is_aec = field.starts_with("opening_")
            || field.starts_with("slab_")
            || field.starts_with("slabopening_")
            || field.starts_with("room_")
            || field.starts_with("marker_")
            || field.starts_with("carrier_")
            || field == "view_cut_height"
            || matches!(
                field,
                "wall_justification"
                    | "wall_base_plane"
                    | "wall_top_plane"
                    | "wall_phase"
                    | "control_plane_name"
                    | "wall_height"
                    | "wall_thickness"
                    | "wall_material"
                    | "wall_base_offset"
                    | "wall_top_offset"
                    | "wall_base_z"
                    | "wall_hatch_angle"
            );
        if !is_aec {
            return false;
        }
        if self.tabs[tab].scene.is_layer_locked(handle) {
            return true;
        }
        if field.starts_with("marker_") {
            let marker_owner = resolve_section_marker_package(&self.tabs[tab].scene, handle).unwrap_or(handle);
            if self.aec.aec_last_applied_property
                == Some((marker_owner, field.to_string(), val.to_string()))
            {
                return true;
            }
            self.aec.aec_last_applied_property =
                Some((marker_owner, field.to_string(), val.to_string()));

            let display_config_library = crate::modules::aec::engine::project::resolve_display_config_library(
                self.aec.aec_project_explorer_file.as_ref(),
            );
            let style_library = crate::modules::aec::engine::project::resolve_style_library(
                self.aec.aec_project_explorer_file.as_ref(),
            );
            let (active_name, _) = self.aec_tab_display_state(tab);
            let ok = apply_marker_property_with_libraries(
                &mut self.tabs[tab].scene.document,
                marker_owner,
                field,
                val,
                &display_config_library,
                &style_library,
                active_name.as_deref(),
            );
            if ok {
                self.tabs[tab].dirty = true;
                self.tabs[tab].scene.bump_geometry();
            }
            return true;
        }
        if field.starts_with("carrier_") || field == "view_cut_height" {
            let carrier_handle = crate::modules::aec::engine::views::carrier::resolve_carrier_handle(
                &self.tabs[tab].scene.document,
                handle,
            );
            // `carrier_action` is a one-shot command; skip dedup so "Update now"
            // can be triggered repeatedly on the same carrier.
            let skip_dedup = field == "carrier_action";
            if !skip_dedup
                && self.aec.aec_last_applied_property
                    == Some((carrier_handle, field.to_string(), val.to_string()))
            {
                return true;
            }
            if skip_dedup {
                self.aec.aec_last_applied_property = None;
            } else {
                self.aec.aec_last_applied_property =
                    Some((carrier_handle, field.to_string(), val.to_string()));
            }

            let display_config_library = crate::modules::aec::engine::project::resolve_display_config_library(
                self.aec.aec_project_explorer_file.as_ref(),
            );
            let style_library = crate::modules::aec::engine::project::resolve_style_library(
                self.aec.aec_project_explorer_file.as_ref(),
            );
            let (active_name, _) = self.aec_tab_display_state(tab);
            let ok = apply_carrier_property_with_libraries(
                &mut self.tabs[tab].scene.document,
                carrier_handle,
                field,
                val,
                &display_config_library,
                &style_library,
                active_name.as_deref(),
            );
            if ok {
                self.tabs[tab].dirty = true;
                self.tabs[tab].scene.bump_geometry();
            }
            return true;
        }
        if field.starts_with("room_") {
            let room_owner = self.tabs[tab]
                .scene
                .document
                .get_entity(handle)
                .and_then(room_package::resolve_room_package)
                .unwrap_or(handle);
            if self.aec.aec_last_applied_property
                == Some((room_owner, field.to_string(), val.to_string()))
            {
                return true;
            }
            self.aec.aec_last_applied_property =
                Some((room_owner, field.to_string(), val.to_string()));

            let Some(entity) = self.tabs[tab].scene.document.get_entity(room_owner) else {
                return true;
            };
            let Some(mut room) = room_xdata::room_from_entity(entity) else {
                return true;
            };

            match field {
                "room_name" => room.name = val.to_string(),
                "room_number" => room.number = val.to_string(),
                "room_function" => {
                    room.function = RoomFunction::from_str(val);
                    room.factor = room.function.default_factor();
                }
                "room_factor" => {
                    match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                        Ok(v) => {
                            room.factor = v;
                            if crate::modules::aec::engine::formulas::is_formula_expression(val) {
                                room.formulas.set("room_factor", val.to_string());
                            } else {
                                room.formulas.remove("room_factor");
                            }
                        }
                        Err(e) => {
                            self.command_line.push_error(&format!("AEC: {e}"));
                            return true;
                        }
                    }
                }
                "room_clear_height" => {
                    match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                        Ok(v) => {
                            room.clear_height = v;
                            if crate::modules::aec::engine::formulas::is_formula_expression(val) {
                                room.formulas.set("room_clear_height", val.to_string());
                            } else {
                                room.formulas.remove("room_clear_height");
                            }
                        }
                        Err(e) => {
                            self.command_line.push_error(&format!("AEC: {e}"));
                            return true;
                        }
                    }
                }
                "room_floor_finish_style" => {
                    let trimmed = val.trim();
                    if trimmed.is_empty() || trimmed == "-" || trimmed == "- (Standard)" {
                        room.finish_style_id = None;
                        room.floor_finish = None;
                    } else {
                        let style_library = crate::modules::aec::engine::project::resolve_style_library(
                            self.aec.aec_project_explorer_file.as_ref(),
                        );
                        let fallback_lib = library::seed_floor_finish_styles();
                        let found = style_library
                            .find_floor_finish_style_by_name_or_id(trimmed)
                            .cloned()
                            .or_else(|| {
                                fallback_lib
                                    .into_iter()
                                    .find(|s| s.style.name == trimmed || s.style.id == trimmed)
                            });
                        if let Some(style) = found {
                            room.finish_style_id = Some(style.style.id.clone());
                            room.floor_finish = Some(style.to_room_finishes());
                        } else {
                            room.finish_style_id = Some(trimmed.to_string());
                        }
                    }
                }
                "room_floor_finish" => {
                    let trimmed = val.trim();
                    if trimmed.is_empty() || trimmed == "-" {
                        room.floor_finish = None;
                    } else {
                        let lower = trimmed.to_lowercase();
                        let hatch = if lower.contains("fliese") || lower.contains("tile") {
                            Some("SQUARE".to_string())
                        } else if lower.contains("parkett") || lower.contains("wood") {
                            Some("ANSI31".to_string())
                        } else {
                            None
                        };
                        let mut finish = RoomFinish::new(trimmed, 0.05);
                        if let Some(h) = hatch {
                            finish = finish.with_hatch(h);
                        }
                        room.floor_finish = Some(vec![finish]);
                    }
                }
                "room_ceiling_finish" => {
                    let trimmed = val.trim();
                    if trimmed.is_empty() || trimmed == "-" {
                        room.ceiling_finish = None;
                    } else {
                        let lower = trimmed.to_lowercase();
                        let hatch = if lower.contains("raster") || lower.contains("grid") {
                            Some("NET".to_string())
                        } else if lower.contains("fliese") || lower.contains("tile") || lower.contains("kassette") {
                            Some("SQUARE".to_string())
                        } else if lower.contains("gips") || lower.contains("wood") || lower.contains("holz") {
                            Some("ANSI31".to_string())
                        } else {
                            None
                        };
                        let mut finish = RoomFinish::new(trimmed, 0.02);
                        if let Some(h) = hatch {
                            finish = finish.with_hatch(h);
                        }
                        room.ceiling_finish = Some(vec![finish]);
                    }
                }
                "room_stamp_x" => {
                    match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                        Ok(v) => {
                            let (_, sy) = room.stamp_pos.unwrap_or_else(|| {
                                let pts = room_boundary_points(entity);
                                if pts.len() >= 3 {
                                    crate::modules::aec::engine::geometry::centroid(&pts)
                                } else {
                                    (0.0, 0.0)
                                }
                            });
                            room.stamp_pos = Some((v, sy));
                        }
                        Err(e) => {
                            self.command_line.push_error(&format!("AEC: {e}"));
                            return true;
                        }
                    }
                }
                "room_stamp_y" => {
                    match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                        Ok(v) => {
                            let (sx, _) = room.stamp_pos.unwrap_or_else(|| {
                                let pts = room_boundary_points(entity);
                                if pts.len() >= 3 {
                                    crate::modules::aec::engine::geometry::centroid(&pts)
                                } else {
                                    (0.0, 0.0)
                                }
                            });
                            room.stamp_pos = Some((sx, v));
                        }
                        Err(e) => {
                            self.command_line.push_error(&format!("AEC: {e}"));
                            return true;
                        }
                    }
                }
                _ => return true,
            }

            room_xdata::write_room_record(&mut self.tabs[tab].scene.document, room_owner, &room);
            let _ = self.regenerate_room_respecting_active_display_config(tab, room_owner);
            let _ = crate::modules::aec::rooms::schedule::update_all_room_schedules(
                &mut self.tabs[tab].scene,
            );
            self.tabs[tab].dirty = true;
            self.tabs[tab].scene.bump_geometry();
            return true;
        }
        if field.starts_with("slabopening_") {
            let owner = slab_package::slab_opening_owner_if_any(&self.tabs[tab].scene, handle)
                .unwrap_or(handle);
            if self.aec.aec_last_applied_property
                == Some((owner, field.to_string(), val.to_string()))
            {
                return true;
            }
            self.aec.aec_last_applied_property =
                Some((owner, field.to_string(), val.to_string()));

            let Some(entity) = self.tabs[tab].scene.document.get_entity(owner) else {
                return true;
            };
            let Some(mut opening) = slab_xdata::slab_opening_from_entity(entity) else {
                return true;
            };
            let host = opening.host_slab;
            match field {
                "slabopening_kind" => {
                    opening.kind = SlabOpeningKind::from_str(val);
                    let _ = slab_xdata::write_slab_opening_model(
                        &mut self.tabs[tab].scene,
                        owner,
                        &opening,
                    );
                }
                "slabopening_depth_mode" => {
                    let depth_val = opening.depth.depth_value().unwrap_or(0.10);
                    opening.depth = SlabOpeningDepth::from_mode_and_depth(val, depth_val);
                    let _ = slab_xdata::write_slab_opening_model(
                        &mut self.tabs[tab].scene,
                        owner,
                        &opening,
                    );
                }
                "slabopening_recess_depth" => {
                    match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                        Ok(v) => {
                            if v >= 0.0 {
                                opening.depth = SlabOpeningDepth::Recess(v);
                                let _ = slab_xdata::write_slab_opening_model(
                                    &mut self.tabs[tab].scene,
                                    owner,
                                    &opening,
                                );
                            }
                        }
                        Err(e) => {
                            self.command_line.push_error(&format!("AEC: {e}"));
                            return true;
                        }
                    }
                }
                _ => return true,
            }
            let _ = self.regenerate_slab_respecting_active_display_config(tab, host);
            let style_library = crate::modules::aec::engine::project::resolve_style_library(
                self.aec.aec_project_explorer_file.as_ref(),
            );
            let rules = self.resolve_active_display_config_slab_rules(tab, Some(host));
            let _ = crate::modules::aec::engine::slab_regen::regenerate_slab_opening_representation(
                &mut self.tabs[tab].scene,
                owner,
                Some(&style_library),
                rules.as_ref(),
            );
            return true;
        }
        if field.starts_with("slab_") {
            let slab_owner = slab_package::resolve_slab_package(&self.tabs[tab].scene, handle);
            if self.aec.aec_last_applied_property
                == Some((slab_owner, field.to_string(), val.to_string()))
            {
                return true;
            }
            self.aec.aec_last_applied_property =
                Some((slab_owner, field.to_string(), val.to_string()));

            match field {
                "slab_justification" => {
                    let justification = SlabJustification::from_str(val);
                    if slab_xdata::write_slab_justification(
                        &mut self.tabs[tab].scene,
                        slab_owner,
                        justification,
                    ) {
                        let _ = self.regenerate_slab_respecting_active_display_config(tab, slab_owner);
                    }
                }
                "slab_phase" => {
                    let new_phase =
                        crate::modules::aec::engine::plan_view::PlanPhase::from_str(val);
                    if slab_xdata::write_slab_phase(
                        &mut self.tabs[tab].scene,
                        slab_owner,
                        new_phase,
                    ) {
                        let _ = self.regenerate_slab_respecting_active_display_config(tab, slab_owner);
                    }
                }
                "slab_base_offset" | "slab_top_offset" | "slab_offset" | "top_offset" | "base_offset" | "offset" => {
                    match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                        Ok(v) => {
                            let is_base = field == "slab_base_offset" || field == "base_offset";
                            let (base, top) = if is_base {
                                (Some(v), None)
                            } else {
                                (None, Some(v))
                            };
                            let canonical_field = if is_base {
                                "slab_base_offset"
                            } else {
                                "slab_top_offset"
                            };
                            let alias_group = if is_base {
                                SLAB_BASE_OFFSET_KEYS
                            } else {
                                SLAB_TOP_OFFSET_KEYS
                            };
                            if slab_xdata::write_slab_plane_offsets(
                                &mut self.tabs[tab].scene,
                                slab_owner,
                                base,
                                top,
                            ) {
                                if let Some(entity) = self.tabs[tab].scene.document.get_entity(slab_owner) {
                                    if let Some(mut slab) = slab_xdata::slab_from_entity(entity) {
                                        slab.formulas.remove_all(alias_group);
                                        if crate::modules::aec::engine::formulas::is_formula_expression(val) {
                                            slab.formulas.set(canonical_field, val.to_string());
                                        }
                                        let _ = slab_xdata::write_slab_record(&mut self.tabs[tab].scene.document, slab_owner, &slab);
                                    }
                                }
                                let _ = self
                                    .regenerate_slab_respecting_active_display_config(tab, slab_owner);
                            }
                        }
                        Err(e) => {
                            self.command_line.push_error(&format!("AEC: {e}"));
                        }
                    }
                }
                "slab_base_plane" | "slab_top_plane" | "slab_plane" => {
                    let style_library =
                        crate::modules::aec::engine::project::resolve_style_library(
                            self.aec.aec_project_explorer_file.as_ref(),
                        );
                    let rules =
                        self.resolve_active_display_config_slab_rules(tab, Some(slab_owner));
                    crate::modules::aec::project::slab_planes::apply_slab_plane_choice(
                        &mut self.tabs[tab].scene,
                        self.aec.aec_project_explorer_file.as_ref(),
                        slab_owner,
                        field == "slab_base_plane",
                        val,
                        Some(&style_library),
                        rules.as_ref(),
                    );
                }
                "slab_structural_style" => {
                    let trimmed = val.trim();
                    let style_library = crate::modules::aec::engine::project::resolve_style_library(
                        self.aec.aec_project_explorer_file.as_ref(),
                    );
                    if let Some(entity) = self.tabs[tab].scene.document.get_entity(slab_owner) {
                        if let Some(mut slab) = slab_xdata::slab_from_entity(entity) {
                            if trimmed.is_empty() || trimmed == "-" || trimmed == "- (Standard)" {
                                slab.structural_style_id = None;
                            } else {
                                let fallback = library::seed_slab_structural_styles();
                                let found = style_library
                                    .find_slab_structural_style_by_name_or_id(trimmed)
                                    .or_else(|| {
                                        fallback
                                            .iter()
                                            .find(|s| s.style.name == trimmed || s.style.id == trimmed)
                                    });
                                slab.structural_style_id = found
                                    .map(|s| s.style.id.clone())
                                    .or_else(|| Some(trimmed.to_string()));
                            }
                            let _ = slab_xdata::write_slab_model(&mut self.tabs[tab].scene, slab_owner, &slab);
                            let _ = self.regenerate_slab_respecting_active_display_config(tab, slab_owner);
                        }
                    }
                }
                "slab_finish_style" => {
                    let trimmed = val.trim();
                    let style_library = crate::modules::aec::engine::project::resolve_style_library(
                        self.aec.aec_project_explorer_file.as_ref(),
                    );
                    if let Some(entity) = self.tabs[tab].scene.document.get_entity(slab_owner) {
                        if let Some(mut slab) = slab_xdata::slab_from_entity(entity) {
                            if trimmed.is_empty() || trimmed == "-" || trimmed == "- (Standard)" {
                                slab.finish_style_id = None;
                            } else {
                                let fallback = library::seed_floor_finish_styles();
                                let found = style_library
                                    .find_floor_finish_style_by_name_or_id(trimmed)
                                    .or_else(|| {
                                        fallback
                                            .iter()
                                            .find(|s| s.style.name == trimmed || s.style.id == trimmed)
                                    });
                                slab.finish_style_id = found
                                    .map(|s| s.style.id.clone())
                                    .or_else(|| Some(trimmed.to_string()));
                            }
                            let _ = slab_xdata::write_slab_model(&mut self.tabs[tab].scene, slab_owner, &slab);
                            let _ = self.regenerate_slab_respecting_active_display_config(tab, slab_owner);
                        }
                    }
                }
                // Style is applied via the style picker modal.
                "slab_style" => {}
                _ => {}
            }
            return true;
        }
        if field.starts_with("opening_") {
            let owner = opening_display::opening_owner_if_any(&self.tabs[tab].scene, handle)
                .unwrap_or(handle);
            if self.aec.aec_last_applied_property
                == Some((owner, field.to_string(), val.to_string()))
            {
                return true;
            }
            self.aec.aec_last_applied_property =
                Some((owner, field.to_string(), val.to_string()));

            let style_library = crate::modules::aec::engine::project::resolve_style_library(
                self.aec.aec_project_explorer_file.as_ref(),
            );
            let Some(entity) = self.tabs[tab].scene.document.get_entity(owner).cloned() else {
                return true;
            };
            let Some(mut opening) = opening_xdata::opening_from_entity(&entity, owner) else {
                return true;
            };
            let (rules, _) =
                self.resolve_active_display_config_wall_rules(tab, Some(opening.host_wall));
            match field {
                "opening_sill_plane" | "opening_head_plane" => {
                    crate::modules::aec::project::opening_planes::apply_opening_plane_choice(
                        &mut self.tabs[tab].scene,
                        self.aec.aec_project_explorer_file.as_ref(),
                        owner,
                        field == "opening_sill_plane",
                        val,
                        Some(&style_library),
                        rules.as_ref(),
                    );
                    return true;
                }
                "opening_sill_offset" | "opening_head_offset" => {
                    match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                        Ok(v) => {
                            let (sill, head) = if field == "opening_sill_offset" {
                                (Some(v), None)
                            } else {
                                (None, Some(v))
                            };
                            crate::modules::aec::project::opening_planes::apply_opening_plane_offsets(
                                &mut self.tabs[tab].scene,
                                self.aec.aec_project_explorer_file.as_ref(),
                                owner,
                                sill,
                                head,
                                Some(&style_library),
                                rules.as_ref(),
                            );
                            if let Some(entity) = self.tabs[tab].scene.document.get_entity(owner) {
                                if let Some(mut op) = opening_xdata::opening_from_entity(entity, owner) {
                                    if crate::modules::aec::engine::formulas::is_formula_expression(val) {
                                        op.formulas.set(field, val.to_string());
                                    } else {
                                        op.formulas.remove(field);
                                    }
                                    let _ = opening_display::commit_opening_instance(
                                        &mut self.tabs[tab].scene,
                                        &op,
                                        Some(&style_library),
                                        rules.as_ref(),
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            self.command_line.push_error(&format!("AEC: {e}"));
                        }
                    }
                    return true;
                }
                _ => {}
            }
            let evaluated_val = match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                Ok(v) => v.to_string(),
                Err(_) => val.to_string(),
            };
            if apply_opening_property(&mut opening, field, &evaluated_val, Some(&style_library)) {
                if crate::modules::aec::engine::formulas::is_formula_expression(val) {
                    opening.formulas.set(field, val.to_string());
                } else {
                    opening.formulas.remove(field);
                }
                let _ = opening_display::commit_opening_instance(
                    &mut self.tabs[tab].scene,
                    &opening,
                    Some(&style_library),
                    rules.as_ref(),
                );
            }
            return true;
        }
        let wall_owner = crate::modules::aec::engine::wall_package::resolve_wall_package(
            &self.tabs[tab].scene,
            handle,
        );
        if self.aec.aec_last_applied_property
            == Some((wall_owner, field.to_string(), val.to_string()))
        {
            return true;
        }
        self.aec.aec_last_applied_property =
            Some((wall_owner, field.to_string(), val.to_string()));

        match field {
            "wall_justification" => {
                let new_justification =
                    crate::modules::aec::engine::WallJustification::from_str(val);
                let style_library = crate::modules::aec::engine::project::resolve_style_library(
                    self.aec.aec_project_explorer_file.as_ref(),
                );
                crate::modules::aec::engine::wall_regen::change_wall_justification(
                    &mut self.tabs[tab].scene,
                    wall_owner,
                    new_justification,
                    Some(&style_library),
                );
            }
            "wall_base_plane" | "wall_top_plane" => {
                let style_library = crate::modules::aec::engine::project::resolve_style_library(
                    self.aec.aec_project_explorer_file.as_ref(),
                );
                crate::modules::aec::project::wall_planes::apply_wall_plane_choice(
                    &mut self.tabs[tab].scene,
                    self.aec.aec_project_explorer_file.as_ref(),
                    wall_owner,
                    field == "wall_base_plane",
                    val,
                    Some(&style_library),
                );
            }
            "wall_phase" => {
                let new_phase = crate::modules::aec::engine::plan_view::PlanPhase::from_str(val);
                crate::modules::aec::engine::xdata::write_wall_phase(
                    &mut self.tabs[tab].scene,
                    wall_owner,
                    new_phase,
                );
                let _ = self.regenerate_wall_respecting_active_display_config(tab, wall_owner);
            }
            "control_plane_name" => {
                apply_control_plane_name(
                    &mut self.tabs[tab].scene,
                    self.aec.aec_project_explorer_file.as_mut(),
                    wall_owner,
                    val,
                );
                self.aec_project_explorer_persist_if_pathed();
            }
            "wall_height" => {
                match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                    Ok(v) => {
                        if v > 0.0 {
                            if let Some(entity) = self.tabs[tab].scene.document.get_entity(wall_owner) {
                                if let Some(mut wall) = crate::modules::aec::engine::xdata::wall_from_entity(entity) {
                                    if crate::modules::aec::engine::formulas::is_formula_expression(val) {
                                        wall.formulas.set("wall_height", val.to_string());
                                    } else {
                                        wall.formulas.remove("wall_height");
                                    }
                                    wall.height = v;
                                    let mut record = acadrust::xdata::ExtendedDataRecord::new(crate::modules::aec::engine::xdata::AEC_APPID);
                                    record.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
                                    crate::modules::aec::engine::xdata::write_aec_record(&mut self.tabs[tab].scene.document, wall_owner, record);
                                } else {
                                    crate::modules::aec::engine::xdata::write_wall_height(
                                        &mut self.tabs[tab].scene,
                                        wall_owner,
                                        v,
                                    );
                                }
                            }
                            let _ =
                                self.regenerate_wall_respecting_active_display_config(tab, wall_owner);
                        }
                    }
                    Err(e) => {
                        self.command_line.push_error(&format!("AEC: {e}"));
                    }
                }
            }
            "wall_base_z" => {
                match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                    Ok(v) => {
                        crate::modules::aec::project::wall_planes::apply_wall_base_z(
                            &mut self.tabs[tab].scene,
                            wall_owner,
                            v,
                        );
                        if let Some(entity) = self.tabs[tab].scene.document.get_entity(wall_owner) {
                            if let Some(mut wall) = crate::modules::aec::engine::xdata::wall_from_entity(entity) {
                                if crate::modules::aec::engine::formulas::is_formula_expression(val) {
                                    wall.formulas.set("wall_base_z", val.to_string());
                                } else {
                                    wall.formulas.remove("wall_base_z");
                                }
                                let mut record = acadrust::xdata::ExtendedDataRecord::new(crate::modules::aec::engine::xdata::AEC_APPID);
                                record.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
                                crate::modules::aec::engine::xdata::write_aec_record(&mut self.tabs[tab].scene.document, wall_owner, record);
                            }
                        }
                    }
                    Err(e) => {
                        self.command_line.push_error(&format!("AEC: {e}"));
                    }
                }
            }
            "wall_base_offset" | "wall_top_offset" => {
                match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                    Ok(v) => {
                        let (base, top) = if field == "wall_base_offset" {
                            (Some(v), None)
                        } else {
                            (None, Some(v))
                        };
                        crate::modules::aec::engine::xdata::write_wall_plane_offsets(
                            &mut self.tabs[tab].scene,
                            wall_owner,
                            base,
                            top,
                        );
                        if let Some(entity) = self.tabs[tab].scene.document.get_entity(wall_owner) {
                            if let Some(mut wall) = crate::modules::aec::engine::xdata::wall_from_entity(entity) {
                                if crate::modules::aec::engine::formulas::is_formula_expression(val) {
                                    wall.formulas.set(field, val.to_string());
                                } else {
                                    wall.formulas.remove(field);
                                }
                                let mut record = acadrust::xdata::ExtendedDataRecord::new(crate::modules::aec::engine::xdata::AEC_APPID);
                                record.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
                                crate::modules::aec::engine::xdata::write_aec_record(&mut self.tabs[tab].scene.document, wall_owner, record);
                            }
                        }
                        let _ =
                            self.regenerate_wall_respecting_active_display_config(tab, wall_owner);
                    }
                    Err(e) => {
                        self.command_line.push_error(&format!("AEC: {e}"));
                    }
                }
            }
            "wall_hatch_angle" => {
                match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                    Ok(v) => {
                        let existing_override = self.tabs[tab]
                            .scene
                            .document
                            .get_entity(wall_owner)
                            .and_then(crate::modules::aec::engine::xdata::wall_from_entity)
                            .and_then(|wall| wall.hatch_override);
                        if let Some(mut ov) = existing_override {
                            ov.hatch_angle = Some(v);
                            crate::modules::aec::engine::xdata::write_wall_hatch_override(
                                &mut self.tabs[tab].scene,
                                wall_owner,
                                Some(ov),
                            );
                        }
                    }
                    Err(e) => {
                        self.command_line.push_error(&format!("AEC: {e}"));
                    }
                }
            }
            "wall_thickness" | "wall_material" => {}
            _ => {}
        }
        true
    }

    /// Re-evaluates all stored formulas across all AEC entities (Walls, Slabs, Openings, Rooms)
    /// in the specified tab after named drawing parameters have changed.
    pub(crate) fn aec_recompute_all_formulas(&mut self, tab: usize) {
        if tab >= self.tabs.len() {
            return;
        }
        let style_library = crate::modules::aec::engine::project::resolve_style_library(
            self.aec.aec_project_explorer_file.as_ref(),
        );

        let entities: Vec<Handle> = self.tabs[tab]
            .scene
            .document
            .entities()
            .map(|e| e.common().handle)
            .collect();
        let mut modified_walls = Vec::new();
        let mut modified_slabs = Vec::new();
        let mut modified_rooms = Vec::new();

        for handle in entities {
            let Some(entity) = self.tabs[tab].scene.document.get_entity(handle).cloned() else {
                continue;
            };

            // 1. Walls
            if let Some(mut wall) = xdata::wall_from_entity(&entity) {
                if !wall.formulas.is_empty() {
                    let mut changed = false;
                    for (field, expr) in &wall.formulas.fields {
                        if let Ok(val) = eval_aec_property_expression(&self.tabs[tab].scene, expr) {
                            match field.as_str() {
                                "wall_height" | "height" => {
                                    if (wall.height - val).abs() > 1e-9 && val > 0.0 {
                                        wall.height = val;
                                        changed = true;
                                    }
                                }
                                "wall_base_offset" | "base_offset" => {
                                    if (wall.base_offset - val).abs() > 1e-9 {
                                        wall.base_offset = val;
                                        changed = true;
                                    }
                                }
                                "wall_top_offset" | "top_offset" => {
                                    if (wall.top_offset - val).abs() > 1e-9 {
                                        wall.top_offset = val;
                                        changed = true;
                                    }
                                }
                                "wall_base_z" => {
                                    if (wall.base_origin[2] - val).abs() > 1e-9 {
                                        wall.base_origin[2] = val;
                                        changed = true;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    if changed {
                        let mut record = acadrust::xdata::ExtendedDataRecord::new(xdata::AEC_APPID);
                        record.values = xdata::wall_record_for_wall(&wall);
                        xdata::write_aec_record(&mut self.tabs[tab].scene.document, handle, record);
                        modified_walls.push(handle);
                    }
                }
            }

            // 2. Slabs
            if let Some(mut slab) = slab_xdata::slab_from_entity(&entity) {
                if !slab.formulas.is_empty() {
                    let mut changed = false;
                    if let Some((_, expr)) = slab.formulas.get_any(SLAB_BASE_OFFSET_KEYS) {
                        if let Ok(val) = eval_aec_property_expression(&self.tabs[tab].scene, expr) {
                            if (slab.base_offset - val).abs() > 1e-9 {
                                slab.base_offset = val;
                                changed = true;
                            }
                        }
                    }
                    if let Some((_, expr)) = slab.formulas.get_any(SLAB_TOP_OFFSET_KEYS) {
                        if let Ok(val) = eval_aec_property_expression(&self.tabs[tab].scene, expr) {
                            if (slab.top_offset - val).abs() > 1e-9 {
                                slab.top_offset = val;
                                changed = true;
                            }
                        }
                    }
                    if changed {
                        slab_xdata::write_slab_record(&mut self.tabs[tab].scene.document, handle, &slab);
                        modified_slabs.push(handle);
                    }
                }
            }

            // 3. Openings
            if let Some(mut opening) = opening_xdata::opening_from_entity(&entity, handle) {
                if !opening.formulas.is_empty() {
                    let mut changed = false;
                    for (field, expr) in &opening.formulas.fields {
                        if let Ok(val) = eval_aec_property_expression(&self.tabs[tab].scene, expr) {
                            match field.as_str() {
                                "opening_width" | "width" => {
                                    if (opening.width - val).abs() > 1e-9 && val > 0.0 {
                                        opening.width = val;
                                        changed = true;
                                    }
                                }
                                "opening_height" | "height" => {
                                    if (opening.height - val).abs() > 1e-9 && val > 0.0 {
                                        opening.height = val;
                                        changed = true;
                                    }
                                }
                                "opening_sill" | "opening_sill_height" | "sill_height" => {
                                    if (opening.sill_height - val).abs() > 1e-9 && val >= 0.0 {
                                        opening.sill_height = val;
                                        changed = true;
                                    }
                                }
                                "opening_sill_offset" | "sill_offset" => {
                                    if (opening.sill_offset - val).abs() > 1e-9 {
                                        opening.sill_offset = val;
                                        changed = true;
                                    }
                                }
                                "opening_head_offset" | "head_offset" => {
                                    if (opening.head_offset - val).abs() > 1e-9 {
                                        opening.head_offset = val;
                                        changed = true;
                                    }
                                }
                                "opening_spring" | "spring_height" => {
                                    if (opening.spring_height - val).abs() > 1e-9 {
                                        opening.spring_height = val;
                                        changed = true;
                                    }
                                }
                                "opening_cross_offset" | "cross_axis_offset" => {
                                    if (opening.cross_axis_offset - val).abs() > 1e-9 {
                                        opening.cross_axis_offset = val;
                                        changed = true;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    if changed {
                        let (rules, _) = self.resolve_active_display_config_wall_rules(tab, Some(opening.host_wall));
                        let _ = opening_display::commit_opening_instance(
                            &mut self.tabs[tab].scene,
                            &opening,
                            Some(&style_library),
                            rules.as_ref(),
                        );
                    }
                }
            }

            // 4. Rooms
            if let Some(mut room) = room_xdata::room_from_entity(&entity) {
                if !room.formulas.is_empty() {
                    let mut changed = false;
                    for (field, expr) in &room.formulas.fields {
                        if let Ok(val) = eval_aec_property_expression(&self.tabs[tab].scene, expr) {
                            match field.as_str() {
                                "room_factor" | "factor" => {
                                    if (room.factor - val).abs() > 1e-9 {
                                        room.factor = val;
                                        changed = true;
                                    }
                                }
                                "room_clear_height" | "clear_height" => {
                                    if (room.clear_height - val).abs() > 1e-9 {
                                        room.clear_height = val;
                                        changed = true;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    if changed {
                        room_xdata::write_room_record(&mut self.tabs[tab].scene.document, handle, &room);
                        modified_rooms.push(handle);
                    }
                }
            }
        }

        for wall_handle in modified_walls {
            let _ = self.regenerate_wall_respecting_active_display_config(tab, wall_handle);
        }
        for slab_handle in modified_slabs {
            let _ = self.regenerate_slab_respecting_active_display_config(tab, slab_handle);
        }
        for room_handle in modified_rooms {
            let _ = self.regenerate_room_respecting_active_display_config(tab, room_handle);
        }
    }
}
