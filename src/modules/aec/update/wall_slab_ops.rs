//! AEC update handlers: Wall and slab prompts, moves, elevation and style setters.

use iced::Task;

use crate::app::{Message, OpenCADStudio};
use crate::modules::aec::message::AecMessage;

impl OpenCADStudio {
    pub(crate) fn update_aec_wall_slab_ops(&mut self, msg: AecMessage) -> Task<Message> {
        match msg {
            AecMessage::AecOpeningGripMove(derived_handle, _) => {
                let tab = self.active_tab;
                if let Some(ent) = self.tabs[tab].scene.document.get_entity(derived_handle) {
                    if let Some(op) = crate::modules::aec::engine::opening_xdata::opening_from_entity(ent, derived_handle) {
                        let wall = crate::modules::aec::engine::wall_package::resolve_wall_package(&self.tabs[tab].scene, op.host_wall);
                        let base_z = crate::modules::aec::engine::opening_display::host_base_z(&self.tabs[tab].scene, wall);
                        let axis: Vec<(f64, f64)> = crate::modules::aec::engine::xdata::get_wall_vertices(&self.tabs[tab].scene, wall)
                            .iter()
                            .map(|v| (v.x, v.y))
                            .collect();
                        let world = if let Some(((cx, cy), _)) = crate::modules::aec::engine::openings::point_and_tangent_at_distance(&axis, op.center_along_axis()) {
                            glam::DVec3::new(cx, cy, base_z)
                        } else {
                            glam::DVec3::ZERO
                        };
                        self.engage_grip_edit(tab, derived_handle, 0, world);
                    } else {
                        let world = match ent {
                            acadrust::EntityType::Line(l) => glam::DVec3::new(
                                (l.start.x + l.end.x) * 0.5,
                                (l.start.y + l.end.y) * 0.5,
                                (l.start.z + l.end.z) * 0.5,
                            ),
                            acadrust::EntityType::LwPolyline(pl) if !pl.vertices.is_empty() => {
                                glam::DVec3::new(pl.vertices[0].location.x, pl.vertices[0].location.y, pl.elevation)
                            }
                            _ => glam::DVec3::ZERO,
                        };
                        self.engage_grip_edit(tab, derived_handle, crate::modules::aec::properties::DERIVED_VIEW_GRIP_BASE, world);
                    }
                }
                Task::none()
            }
            AecMessage::AecOpeningGripResize(derived_handle, _) => {
                let tab = self.active_tab;
                if let Some(ent) = self.tabs[tab].scene.document.get_entity(derived_handle) {
                    if let Some(op) = crate::modules::aec::engine::opening_xdata::opening_from_entity(ent, derived_handle) {
                        let wall = crate::modules::aec::engine::wall_package::resolve_wall_package(&self.tabs[tab].scene, op.host_wall);
                        let base_z = crate::modules::aec::engine::opening_display::host_base_z(&self.tabs[tab].scene, wall);
                        let axis: Vec<(f64, f64)> = crate::modules::aec::engine::xdata::get_wall_vertices(&self.tabs[tab].scene, wall)
                            .iter()
                            .map(|v| (v.x, v.y))
                            .collect();
                        let (start, _end) = op.axis_span();
                        let world = if let Some(((cx, cy), _)) = crate::modules::aec::engine::openings::point_and_tangent_at_distance(&axis, start) {
                            glam::DVec3::new(cx, cy, base_z)
                        } else {
                            glam::DVec3::ZERO
                        };
                        self.engage_grip_edit(tab, derived_handle, 1, world);
                    } else {
                        let world = match ent {
                            acadrust::EntityType::Line(l) => glam::DVec3::new(
                                (l.start.x + l.end.x) * 0.5,
                                (l.start.y + l.end.y) * 0.5,
                                (l.start.z + l.end.z) * 0.5,
                            ),
                            acadrust::EntityType::LwPolyline(pl) if !pl.vertices.is_empty() => {
                                glam::DVec3::new(pl.vertices[0].location.x, pl.vertices[0].location.y, pl.elevation)
                            }
                            _ => glam::DVec3::ZERO,
                        };
                        self.engage_grip_edit(tab, derived_handle, crate::modules::aec::properties::DERIVED_VIEW_RESIZE_GRIP_BASE, world);
                    }
                }
                Task::none()
            }
            AecMessage::AecOpeningGripFlipHinge(derived_handle) => {
                let tab = self.active_tab;
                let op_h = crate::modules::aec::properties::opening_handle_from_derived_primitive(&self.tabs[tab].scene.document, derived_handle)
                    .or_else(|| {
                        let ent = self.tabs[tab].scene.document.get_entity(derived_handle)?;
                        crate::modules::aec::engine::opening_xdata::opening_from_entity(ent, derived_handle).map(|_| derived_handle)
                    });
                if let Some(op_h) = op_h {
                    if let Some(ent) = self.tabs[tab].scene.document.get_entity(op_h) {
                        if let Some(mut opening) = crate::modules::aec::engine::opening_xdata::opening_from_entity(ent, op_h) {
                            opening.flip_hinge();
                            let rec = crate::modules::aec::engine::opening_xdata::opening_record(&opening);
                            crate::modules::aec::engine::xdata::write_aec_record(&mut self.tabs[tab].scene.document, op_h, rec);
                            let host_wall = opening.host_wall;
                            let _ = self.regenerate_wall_respecting_active_display_config(tab, host_wall);
                            if let Some(carrier_h) = crate::modules::aec::properties::carrier_handle_from_derived_primitive(&self.tabs[tab].scene.document, derived_handle) {
                                if let Some((vp, mv)) = crate::modules::aec::engine::views::carrier::read_view_carrier_definitions(&self.tabs[tab].scene.document, carrier_h) {
                                    self.regenerate_derived_view_carrier(tab, carrier_h, &vp, &mv);
                                }
                            }
                            self.tabs[tab].scene.bump_geometry();
                        }
                    }
                }
                Task::none()
            }
            AecMessage::AecOpeningGripFlipSwing(derived_handle) => {
                let tab = self.active_tab;
                let op_h = crate::modules::aec::properties::opening_handle_from_derived_primitive(&self.tabs[tab].scene.document, derived_handle)
                    .or_else(|| {
                        let ent = self.tabs[tab].scene.document.get_entity(derived_handle)?;
                        crate::modules::aec::engine::opening_xdata::opening_from_entity(ent, derived_handle).map(|_| derived_handle)
                    });
                if let Some(op_h) = op_h {
                    if let Some(ent) = self.tabs[tab].scene.document.get_entity(op_h) {
                        if let Some(mut opening) = crate::modules::aec::engine::opening_xdata::opening_from_entity(ent, op_h) {
                            opening.flip_swing();
                            let rec = crate::modules::aec::engine::opening_xdata::opening_record(&opening);
                            crate::modules::aec::engine::xdata::write_aec_record(&mut self.tabs[tab].scene.document, op_h, rec);
                            let host_wall = opening.host_wall;
                            let _ = self.regenerate_wall_respecting_active_display_config(tab, host_wall);
                            if let Some(carrier_h) = crate::modules::aec::properties::carrier_handle_from_derived_primitive(&self.tabs[tab].scene.document, derived_handle) {
                                if let Some((vp, mv)) = crate::modules::aec::engine::views::carrier::read_view_carrier_definitions(&self.tabs[tab].scene.document, carrier_h) {
                                    self.regenerate_derived_view_carrier(tab, carrier_h, &vp, &mv);
                                }
                            }
                            self.tabs[tab].scene.bump_geometry();
                        }
                    }
                }
                Task::none()
            }
            AecMessage::AecOpeningGripSetStyle(derived_handle, style_id) => {
                let tab = self.active_tab;
                let op_h = crate::modules::aec::properties::opening_handle_from_derived_primitive(&self.tabs[tab].scene.document, derived_handle)
                    .or_else(|| {
                        let ent = self.tabs[tab].scene.document.get_entity(derived_handle)?;
                        crate::modules::aec::engine::opening_xdata::opening_from_entity(ent, derived_handle).map(|_| derived_handle)
                    });
                if let Some(op_h) = op_h {
                    if let Some(ent) = self.tabs[tab].scene.document.get_entity(op_h) {
                        if let Some(mut opening) = crate::modules::aec::engine::opening_xdata::opening_from_entity(ent, op_h) {
                            opening.style_id = if style_id.is_empty() { None } else { Some(style_id) };
                            let rec = crate::modules::aec::engine::opening_xdata::opening_record(&opening);
                            crate::modules::aec::engine::xdata::write_aec_record(&mut self.tabs[tab].scene.document, op_h, rec);
                            let host_wall = opening.host_wall;
                            let _ = self.regenerate_wall_respecting_active_display_config(tab, host_wall);
                            if let Some(carrier_h) = crate::modules::aec::properties::carrier_handle_from_derived_primitive(&self.tabs[tab].scene.document, derived_handle) {
                                if let Some((vp, mv)) = crate::modules::aec::engine::views::carrier::read_view_carrier_definitions(&self.tabs[tab].scene.document, carrier_h) {
                                    self.regenerate_derived_view_carrier(tab, carrier_h, &vp, &mv);
                                }
                            }
                            self.tabs[tab].scene.bump_geometry();
                        }
                    }
                }
                Task::none()
            }
            AecMessage::AecOpeningGripSetSill(derived_handle, sill_val) => {
                let tab = self.active_tab;
                let op_h = crate::modules::aec::properties::opening_handle_from_derived_primitive(&self.tabs[tab].scene.document, derived_handle)
                    .or_else(|| {
                        let ent = self.tabs[tab].scene.document.get_entity(derived_handle)?;
                        crate::modules::aec::engine::opening_xdata::opening_from_entity(ent, derived_handle).map(|_| derived_handle)
                    });
                if let Some(op_h) = op_h {
                    if let Some(ent) = self.tabs[tab].scene.document.get_entity(op_h) {
                        if let Some(mut opening) = crate::modules::aec::engine::opening_xdata::opening_from_entity(ent, op_h) {
                            opening.sill_height = sill_val;
                            let rec = crate::modules::aec::engine::opening_xdata::opening_record(&opening);
                            crate::modules::aec::engine::xdata::write_aec_record(&mut self.tabs[tab].scene.document, op_h, rec);
                            let host_wall = opening.host_wall;
                            let _ = self.regenerate_wall_respecting_active_display_config(tab, host_wall);
                            if let Some(carrier_h) = crate::modules::aec::properties::carrier_handle_from_derived_primitive(&self.tabs[tab].scene.document, derived_handle) {
                                if let Some((vp, mv)) = crate::modules::aec::engine::views::carrier::read_view_carrier_definitions(&self.tabs[tab].scene.document, carrier_h) {
                                    self.regenerate_derived_view_carrier(tab, carrier_h, &vp, &mv);
                                }
                            }
                            self.tabs[tab].scene.bump_geometry();
                        }
                    }
                }
                Task::none()
            }
            AecMessage::AecWallBaseOffsetPrompt(derived_handle) => {
                let tab = self.active_tab;
                let wall_h = (|| {
                    let ent = self.tabs[tab].scene.document.get_entity(derived_handle)?;
                    if crate::modules::aec::engine::xdata::wall_from_entity(ent).is_some() {
                        return Some(derived_handle);
                    }
                    let meta = crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(ent)?;
                    let elem_ref = meta.element_ref?;
                    let uuid = elem_ref.element_id.as_uuid();
                    let bytes = uuid.as_bytes();
                    let handle_val = u64::from_be_bytes(bytes[8..16].try_into().ok()?);
                    Some(acadrust::Handle::new(handle_val))
                })();
                if let Some(wall_h) = wall_h {
                    let verts = crate::modules::aec::engine::xdata::get_wall_vertices(&self.tabs[tab].scene, wall_h);
                    if !verts.is_empty() {
                        let world = (verts[0] + verts[verts.len() - 1]) * 0.5;
                        self.engage_grip_edit(tab, wall_h, crate::modules::aec::properties::WALL_BASE_DROPDOWN_GRIP_ID, world);
                    }
                } else if let Some(ent) = self.tabs[tab].scene.document.get_entity(derived_handle) {
                    let world = match ent {
                        acadrust::EntityType::Line(l) => glam::DVec3::new(
                            (l.start.x + l.end.x) * 0.5,
                            (l.start.y + l.end.y) * 0.5,
                            (l.start.z + l.end.z) * 0.5,
                        ),
                        acadrust::EntityType::LwPolyline(pl) if !pl.vertices.is_empty() => {
                            glam::DVec3::new(pl.vertices[0].location.x, pl.vertices[0].location.y, pl.elevation)
                        }
                        _ => glam::DVec3::ZERO,
                    };
                    self.engage_grip_edit(tab, derived_handle, crate::modules::aec::properties::DERIVED_VIEW_GRIP_BASE, world);
                }
                Task::none()
            }
            AecMessage::AecWallTopMove(derived_handle) => {
                let tab = self.active_tab;
                let wall_h = (|| {
                    let ent = self.tabs[tab].scene.document.get_entity(derived_handle)?;
                    if crate::modules::aec::engine::xdata::wall_from_entity(ent).is_some() {
                        return Some(derived_handle);
                    }
                    let meta = crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(ent)?;
                    let elem_ref = meta.element_ref?;
                    let uuid = elem_ref.element_id.as_uuid();
                    let bytes = uuid.as_bytes();
                    let handle_val = u64::from_be_bytes(bytes[8..16].try_into().ok()?);
                    Some(acadrust::Handle::new(handle_val))
                })();
                if let Some(wall_h) = wall_h {
                    let verts = crate::modules::aec::engine::xdata::get_wall_vertices(&self.tabs[tab].scene, wall_h);
                    if !verts.is_empty() {
                        let world = (verts[0] + verts[verts.len() - 1]) * 0.5;
                        self.engage_grip_edit(tab, wall_h, 0, world);
                    }
                } else if let Some(ent) = self.tabs[tab].scene.document.get_entity(derived_handle) {
                    let world = match ent {
                        acadrust::EntityType::Line(l) => glam::DVec3::new(
                            (l.start.x + l.end.x) * 0.5,
                            (l.start.y + l.end.y) * 0.5,
                            (l.start.z + l.end.z) * 0.5,
                        ),
                        acadrust::EntityType::LwPolyline(pl) if !pl.vertices.is_empty() => {
                            glam::DVec3::new(pl.vertices[0].location.x, pl.vertices[0].location.y, pl.elevation)
                        }
                        _ => glam::DVec3::ZERO,
                    };
                    self.engage_grip_edit(tab, derived_handle, crate::modules::aec::properties::DERIVED_VIEW_GRIP_BASE, world);
                }
                Task::none()
            }
            AecMessage::AecWallSetElevation(derived_handle, elevation) => {
                let tab = self.active_tab;
                let wall_h = (|| {
                    let ent = self.tabs[tab].scene.document.get_entity(derived_handle)?;
                    if crate::modules::aec::engine::xdata::wall_from_entity(ent).is_some() {
                        return Some(derived_handle);
                    }
                    let meta = crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(ent)?;
                    let elem_ref = meta.element_ref?;
                    let uuid = elem_ref.element_id.as_uuid();
                    let bytes = uuid.as_bytes();
                    let handle_val = u64::from_be_bytes(bytes[8..16].try_into().ok()?);
                    Some(acadrust::Handle::new(handle_val))
                })();
                if let Some(wall_h) = wall_h {
                    if let Some(ent) = self.tabs[tab].scene.document.get_entity(wall_h) {
                        if let Some(mut wall) = crate::modules::aec::engine::xdata::wall_from_entity(ent) {
                            let old_z = wall.base_origin[2];
                            let delta_z = elevation - old_z;
                            wall.base_origin[2] = elevation;
                            wall.base_offset += delta_z;
                            wall.height -= delta_z;
                            let mut rec = acadrust::xdata::ExtendedDataRecord::new(crate::modules::aec::engine::xdata::AEC_APPID);
                            rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
                            crate::modules::aec::engine::xdata::write_aec_record(&mut self.tabs[tab].scene.document, wall_h, rec);

                            let mut affected: Vec<acadrust::Handle> = crate::modules::aec::engine::owner_index::children_of(&self.tabs[tab].scene.document, wall_h);
                            for e in self.tabs[tab].scene.document.entities() {
                                let h = e.common().handle;
                                if let Some(op) = crate::modules::aec::engine::opening_xdata::opening_from_entity(e, h) {
                                    if op.host_wall == wall_h && !affected.contains(&h) {
                                        affected.push(h);
                                    }
                                }
                            }
                            for op_h in affected {
                                if let Some(oent) = self.tabs[tab].scene.document.get_entity(op_h) {
                                    if let Some(mut opening) = crate::modules::aec::engine::opening_xdata::opening_from_entity(oent, op_h) {
                                        opening.sill_height -= delta_z;
                                        let orec = crate::modules::aec::engine::opening_xdata::opening_record(&opening);
                                        crate::modules::aec::engine::xdata::write_aec_record(&mut self.tabs[tab].scene.document, op_h, orec);
                                    }
                                }
                            }

                            let _ = self.regenerate_wall_respecting_active_display_config(tab, wall_h);
                            if let Some(carrier_h) = crate::modules::aec::properties::carrier_handle_from_derived_primitive(&self.tabs[tab].scene.document, derived_handle) {
                                if let Some((vp, mv)) = crate::modules::aec::engine::views::carrier::read_view_carrier_definitions(&self.tabs[tab].scene.document, carrier_h) {
                                    self.regenerate_derived_view_carrier(tab, carrier_h, &vp, &mv);
                                }
                            }
                            self.tabs[tab].scene.bump_geometry();
                        }
                    }
                }
                Task::none()
            }
            AecMessage::AecWallSetStyle(derived_handle, style_id) => {
                let tab = self.active_tab;
                let wall_h = (|| {
                    let ent = self.tabs[tab].scene.document.get_entity(derived_handle)?;
                    if crate::modules::aec::engine::xdata::wall_from_entity(ent).is_some() {
                        return Some(derived_handle);
                    }
                    let meta = crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(ent)?;
                    let elem_ref = meta.element_ref?;
                    let uuid = elem_ref.element_id.as_uuid();
                    let bytes = uuid.as_bytes();
                    let handle_val = u64::from_be_bytes(bytes[8..16].try_into().ok()?);
                    Some(acadrust::Handle::new(handle_val))
                })();
                if let Some(wall_h) = wall_h {
                    if let Some(ent) = self.tabs[tab].scene.document.get_entity(wall_h) {
                        if let Some(mut wall) = crate::modules::aec::engine::xdata::wall_from_entity(ent) {
                            wall.style_id = style_id;
                            let mut rec = acadrust::xdata::ExtendedDataRecord::new(crate::modules::aec::engine::xdata::AEC_APPID);
                            rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
                            crate::modules::aec::engine::xdata::write_aec_record(&mut self.tabs[tab].scene.document, wall_h, rec);
                            let _ = self.regenerate_wall_respecting_active_display_config(tab, wall_h);
                            if let Some(carrier_h) = crate::modules::aec::properties::carrier_handle_from_derived_primitive(&self.tabs[tab].scene.document, derived_handle) {
                                if let Some((vp, mv)) = crate::modules::aec::engine::views::carrier::read_view_carrier_definitions(&self.tabs[tab].scene.document, carrier_h) {
                                    self.regenerate_derived_view_carrier(tab, carrier_h, &vp, &mv);
                                }
                            }
                            self.tabs[tab].scene.bump_geometry();
                        }
                    }
                }
                Task::none()
            }
            AecMessage::AecSlabThicknessPrompt(derived_handle) => {
                let tab = self.active_tab;
                let slab_h = (|| {
                    let ent = self.tabs[tab].scene.document.get_entity(derived_handle)?;
                    if crate::modules::aec::engine::slab_xdata::slab_from_entity(ent).is_some() {
                        return Some(derived_handle);
                    }
                    let meta = crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(ent)?;
                    let elem_ref = meta.element_ref?;
                    let uuid = elem_ref.element_id.as_uuid();
                    let bytes = uuid.as_bytes();
                    let handle_val = u64::from_be_bytes(bytes[8..16].try_into().ok()?);
                    Some(acadrust::Handle::new(handle_val))
                })();
                if let Some(slab_h) = slab_h {
                    if let Some(ent) = self.tabs[tab].scene.document.get_entity(slab_h) {
                        let pos = match ent {
                            acadrust::EntityType::LwPolyline(pl) if !pl.vertices.is_empty() => {
                                glam::DVec3::new(pl.vertices[0].location.x, pl.vertices[0].location.y, pl.elevation)
                            }
                            _ => glam::DVec3::ZERO,
                        };
                        self.engage_grip_edit(tab, slab_h, crate::modules::aec::properties::SLAB_BOTTOM_DROPDOWN_GRIP_ID, pos);
                    }
                } else if let Some(ent) = self.tabs[tab].scene.document.get_entity(derived_handle) {
                    let world = match ent {
                        acadrust::EntityType::Line(l) => glam::DVec3::new(
                            (l.start.x + l.end.x) * 0.5,
                            (l.start.y + l.end.y) * 0.5,
                            (l.start.z + l.end.z) * 0.5,
                        ),
                        acadrust::EntityType::LwPolyline(pl) if !pl.vertices.is_empty() => {
                            glam::DVec3::new(pl.vertices[0].location.x, pl.vertices[0].location.y, pl.elevation)
                        }
                        _ => glam::DVec3::ZERO,
                    };
                    self.engage_grip_edit(tab, derived_handle, crate::modules::aec::properties::DERIVED_VIEW_GRIP_BASE, world);
                }
                Task::none()
            }
            AecMessage::AecSlabTopMove(derived_handle) => {
                let tab = self.active_tab;
                let slab_h = (|| {
                    let ent = self.tabs[tab].scene.document.get_entity(derived_handle)?;
                    if crate::modules::aec::engine::slab_xdata::slab_from_entity(ent).is_some() {
                        return Some(derived_handle);
                    }
                    let meta = crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(ent)?;
                    let elem_ref = meta.element_ref?;
                    let uuid = elem_ref.element_id.as_uuid();
                    let bytes = uuid.as_bytes();
                    let handle_val = u64::from_be_bytes(bytes[8..16].try_into().ok()?);
                    Some(acadrust::Handle::new(handle_val))
                })();
                if let Some(slab_h) = slab_h {
                    if let Some(ent) = self.tabs[tab].scene.document.get_entity(slab_h) {
                        let pos = match ent {
                            acadrust::EntityType::LwPolyline(pl) if !pl.vertices.is_empty() => {
                                glam::DVec3::new(pl.vertices[0].location.x, pl.vertices[0].location.y, pl.elevation)
                            }
                            _ => glam::DVec3::ZERO,
                        };
                        self.engage_grip_edit(tab, slab_h, 0, pos);
                    }
                } else if let Some(ent) = self.tabs[tab].scene.document.get_entity(derived_handle) {
                    let world = match ent {
                        acadrust::EntityType::Line(l) => glam::DVec3::new(
                            (l.start.x + l.end.x) * 0.5,
                            (l.start.y + l.end.y) * 0.5,
                            (l.start.z + l.end.z) * 0.5,
                        ),
                        acadrust::EntityType::LwPolyline(pl) if !pl.vertices.is_empty() => {
                            glam::DVec3::new(pl.vertices[0].location.x, pl.vertices[0].location.y, pl.elevation)
                        }
                        _ => glam::DVec3::ZERO,
                    };
                    self.engage_grip_edit(tab, derived_handle, crate::modules::aec::properties::DERIVED_VIEW_GRIP_BASE, world);
                }
                Task::none()
            }
            AecMessage::AecSlabSetElevation(derived_handle, elevation) => {
                let tab = self.active_tab;
                let slab_h = (|| {
                    let ent = self.tabs[tab].scene.document.get_entity(derived_handle)?;
                    if crate::modules::aec::engine::slab_xdata::slab_from_entity(ent).is_some() {
                        return Some(derived_handle);
                    }
                    let meta = crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(ent)?;
                    let elem_ref = meta.element_ref?;
                    let uuid = elem_ref.element_id.as_uuid();
                    let bytes = uuid.as_bytes();
                    let handle_val = u64::from_be_bytes(bytes[8..16].try_into().ok()?);
                    Some(acadrust::Handle::new(handle_val))
                })();
                if let Some(slab_h) = slab_h {
                    if let Some(ent) = self.tabs[tab].scene.document.get_entity(slab_h) {
                        if let Some(mut slab) = crate::modules::aec::engine::slab_xdata::slab_from_entity(ent) {
                            let delta_z = elevation - slab.base_origin[2];
                            slab.base_origin[2] = elevation;
                            slab.top_origin[2] += delta_z;
                            crate::modules::aec::engine::slab_xdata::write_slab_record(&mut self.tabs[tab].scene.document, slab_h, &slab);
                            let _ = self.regenerate_slab_respecting_active_display_config(tab, slab_h);
                            if let Some(carrier_h) = crate::modules::aec::properties::carrier_handle_from_derived_primitive(&self.tabs[tab].scene.document, derived_handle) {
                                if let Some((vp, mv)) = crate::modules::aec::engine::views::carrier::read_view_carrier_definitions(&self.tabs[tab].scene.document, carrier_h) {
                                    self.regenerate_derived_view_carrier(tab, carrier_h, &vp, &mv);
                                }
                            }
                            self.tabs[tab].scene.bump_geometry();
                        }
                    }
                }
                Task::none()
            }
            AecMessage::AecSlabSetStyle(derived_handle, style_id) => {
                let tab = self.active_tab;
                let slab_h = (|| {
                    let ent = self.tabs[tab].scene.document.get_entity(derived_handle)?;
                    if crate::modules::aec::engine::slab_xdata::slab_from_entity(ent).is_some() {
                        return Some(derived_handle);
                    }
                    let meta = crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(ent)?;
                    let elem_ref = meta.element_ref?;
                    let uuid = elem_ref.element_id.as_uuid();
                    let bytes = uuid.as_bytes();
                    let handle_val = u64::from_be_bytes(bytes[8..16].try_into().ok()?);
                    Some(acadrust::Handle::new(handle_val))
                })();
                if let Some(slab_h) = slab_h {
                    if let Some(ent) = self.tabs[tab].scene.document.get_entity(slab_h) {
                        if let Some(mut slab) = crate::modules::aec::engine::slab_xdata::slab_from_entity(ent) {
                            slab.style_id = style_id;
                            crate::modules::aec::engine::slab_xdata::write_slab_record(&mut self.tabs[tab].scene.document, slab_h, &slab);
                            let _ = self.regenerate_slab_respecting_active_display_config(tab, slab_h);
                            if let Some(carrier_h) = crate::modules::aec::properties::carrier_handle_from_derived_primitive(&self.tabs[tab].scene.document, derived_handle) {
                                if let Some((vp, mv)) = crate::modules::aec::engine::views::carrier::read_view_carrier_definitions(&self.tabs[tab].scene.document, carrier_h) {
                                    self.regenerate_derived_view_carrier(tab, carrier_h, &vp, &mv);
                                }
                            }
                            self.tabs[tab].scene.bump_geometry();
                        }
                    }
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }
}
