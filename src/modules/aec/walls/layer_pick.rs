//! Junction layer-pair / layer-gap pick in the viewport.
//!
//! Bodies live here (still `impl OpenCADStudio`) so Core `update/mod.rs`
//! does not grow AEC pick logic.

use crate::app::{AecModalKind, OpenCADStudio};
use crate::modules::aec::engine::join_ops;
use crate::modules::aec::engine::junction_pick;
use crate::modules::aec::engine::xdata;

impl OpenCADStudio {
    /// Fill the Junction Editor's currently chosen wall layer(s) in the
    /// viewport (transparent orange). Cleared when nothing is selected or the panel closes.
    pub(crate) fn sync_junction_editor_layer_highlight(&mut self) {
        let i = self.active_tab;
        if self.tabs[i].active_cmd.is_some() {
            return;
        }
        let orange = crate::scene::model::wire_model::WireModel::LAYER_PICK;
        let mut hatches = Vec::new();
        let mut push = |wall, idx| {
            if let Some(h) = crate::modules::aec::engine::junction_pick::wall_layer_highlight_hatch(
                &self.tabs[i].scene,
                wall,
                idx,
                orange,
            ) {
                hatches.push(h);
            }
        };
        if self.active_modal == Some(crate::app::ModalKind::Aec(AecModalKind::JunctionEditor)) {
            if let Some((axis, _)) = self.aec.aec_junction_editor_target {
                if let Some((idx, _)) = self.aec.aec_junction_editor_pair_layer_a {
                    push(axis, idx);
                }
                if let Some((idx, _)) = self.aec.aec_junction_editor_gap_layer.as_ref() {
                    let gap_wall = self
                        .aec.aec_junction_editor_target
                        .and_then(|(h, e)| {
                            crate::modules::aec::engine::join_ops::through_wall_at_junction(
                                &self.tabs[i].scene,
                                h,
                                e,
                            )
                        })
                        .map(|p| p.axis_handle)
                        .unwrap_or(axis);
                    push(gap_wall, *idx);
                }
                if let (Some(wall_b), Some((idx, _))) = (
                    self.aec.aec_junction_editor_pair_wall_b,
                    self.aec.aec_junction_editor_pair_layer_b.as_ref(),
                ) {
                    push(wall_b, *idx);
                }
            }
        }
        if let Some(pick) = self.aec.aec_layer_pair_draw.as_ref() {
            if let Some((wall, idx, _)) = pick.layer_a.as_ref() {
                push(*wall, *idx);
            }
            let cyan = pick
                .layer_b
                .as_ref()
                .map(|(wall, idx, _)| (*wall, *idx))
                .or(pick.hover);
            if let Some((wall, idx)) = cyan {
                if pick.layer_a.as_ref().map(|(w, i, _)| (*w, *i)) != Some((wall, idx)) {
                    push(wall, idx);
                }
            }
        }
        if let Some(pick) = self.aec.aec_layer_gap_draw.as_ref() {
            if let Some((wall, idx, _)) = pick.layer.as_ref() {
                push(*wall, *idx);
            }
            if let Some((wall, idx, _)) = pick.from.as_ref() {
                push(*wall, *idx);
            }
            if let Some((wall, idx)) = pick
                .to
                .as_ref()
                .map(|(w, i, _)| (*w, *i))
                .or(pick.hover)
            {
                let skip = pick.from.as_ref().map(|(w, i, _)| (*w, *i)) == Some((wall, idx))
                    || pick.layer.as_ref().map(|(w, i, _)| (*w, *i)) == Some((wall, idx));
                if !skip {
                    push(wall, idx);
                }
            }
        }
        self.tabs[i].scene.set_preview_wires(Vec::new());
        self.tabs[i].scene.set_command_preview_hatches(hatches);
    }

    pub(crate) fn cancel_layer_pair_draw_pick(&mut self) {
        if self.aec.aec_layer_pair_draw.take().is_some() {
            let i = self.active_tab;
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            sel.context_menu = None;
            sel.junction_menu_only = false;
            drop(sel);
            self.sync_junction_editor_layer_highlight();
            self.command_line.push_info(crate::t!("*Cancel*").as_ref());
        }
        if self.aec.aec_layer_gap_draw.take().is_some() {
            let i = self.active_tab;
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            sel.context_menu = None;
            sel.junction_menu_only = false;
            drop(sel);
            self.sync_junction_editor_layer_highlight();
            self.command_line.push_info(crate::t!("*Cancel*").as_ref());
        }
    }

    pub(crate) fn update_layer_pair_draw_hover(&mut self, world: glam::DVec3) {
        let Some(pick) = self.aec.aec_layer_pair_draw.as_ref() else {
            return;
        };
        if pick.awaiting_style {
            return;
        }
        let i = self.active_tab;
        let axis = pick.axis;
        let end = pick.end_index;
        let hit = crate::modules::aec::engine::junction_pick::pick_junction_wall_layer(
            &self.tabs[i].scene,
            axis,
            end,
            world.x,
            world.y,
        );
        if let Some(pick) = self.aec.aec_layer_pair_draw.as_mut() {
            pick.hover = hit.map(|(h, idx, _)| (h, idx));
        }
        self.sync_junction_editor_layer_highlight();
    }

    pub(crate) fn click_layer_pair_draw(
        &mut self,
        world: glam::DVec3,
        menu_pos: iced::Point,
    ) {
        let Some(pick) = self.aec.aec_layer_pair_draw.as_ref() else {
            return;
        };
        if pick.awaiting_style {
            return;
        }
        let i = self.active_tab;
        let axis = pick.axis;
        let end = pick.end_index;
        let hit = crate::modules::aec::engine::junction_pick::pick_junction_wall_layer(
            &self.tabs[i].scene,
            axis,
            end,
            world.x,
            world.y,
        );
        let near_node = crate::modules::aec::engine::junction_pick::junction_node_xy(
            &self.tabs[i].scene,
            axis,
            end,
        )
        .is_some_and(|(jx, jy)| {
            let r = crate::modules::aec::engine::junction_pick::junction_outer_pick_radius(
                &self.tabs[i].scene,
                axis,
                end,
            );
            let dx = world.x - jx;
            let dy = world.y - jy;
            dx * dx + dy * dy <= r * r
        });
        let picking_a = self
            .aec.aec_layer_pair_draw
            .as_ref()
            .is_some_and(|p| p.layer_a.is_none());
        if picking_a {
            let Some(hit) = hit else {
                return;
            };
            if let Some(pick) = self.aec.aec_layer_pair_draw.as_mut() {
                pick.layer_a = Some(hit);
                pick.hover = None;
            }
            self.sync_junction_editor_layer_highlight();
            self.command_line.push_info(
                crate::t!(
                    "Zweite Schicht am Knoten klicken (Eingabe = Außenkante, Esc = Abbrechen)."
                )
                .as_ref(),
            );
            return;
        }
        if let Some(hit) = hit {
            if let Some(pick) = self.aec.aec_layer_pair_draw.as_mut() {
                pick.layer_b = Some(hit);
                pick.layer_b_outer = false;
                pick.awaiting_style = true;
                pick.hover = None;
            }
        } else if near_node {
            if let Some(pick) = self.aec.aec_layer_pair_draw.as_mut() {
                pick.layer_b = None;
                pick.layer_b_outer = true;
                pick.awaiting_style = true;
                pick.hover = None;
            }
        } else {
            return;
        }
        let mut sel = self.tabs[i].scene.selection.borrow_mut();
        sel.context_menu = Some(menu_pos);
        sel.junction_menu_only = false;
        drop(sel);
        self.sync_junction_editor_layer_highlight();
        self.command_line
            .push_info(crate::t!("Verbindungsart wählen.").as_ref());
    }

    pub(crate) fn update_layer_gap_draw_hover(&mut self, world: glam::DVec3) {
        let Some(pick) = self.aec.aec_layer_gap_draw.as_ref() else {
            return;
        };
        if pick.to.is_some() {
            return;
        }
        let i = self.active_tab;
        let axis = pick.axis;
        let end = pick.end_index;
        let hit = crate::modules::aec::engine::junction_pick::pick_junction_wall_layer(
            &self.tabs[i].scene,
            axis,
            end,
            world.x,
            world.y,
        );
        if let Some(pick) = self.aec.aec_layer_gap_draw.as_mut() {
            pick.hover = hit.map(|(h, idx, _)| (h, idx));
        }
        self.sync_junction_editor_layer_highlight();
    }

    pub(crate) fn click_layer_gap_draw(&mut self, world: glam::DVec3) {
        let Some(pick) = self.aec.aec_layer_gap_draw.as_ref() else {
            return;
        };
        if pick.to.is_some() {
            return;
        }
        let i = self.active_tab;
        let axis = pick.axis;
        let end = pick.end_index;
        let Some(hit) = crate::modules::aec::engine::junction_pick::pick_junction_wall_layer(
            &self.tabs[i].scene,
            axis,
            end,
            world.x,
            world.y,
        ) else {
            return;
        };
        if pick.layer.is_none() {
            let through = crate::modules::aec::engine::join_ops::through_wall_at_junction(
                &self.tabs[i].scene,
                axis,
                end,
            );
            if through.as_ref().map(|p| p.axis_handle) != Some(hit.0) {
                self.command_line.push_info(
                    crate::t!(
                        "Bitte eine Schicht der durchlaufenden Wand wählen (Esc = Abbrechen)."
                    )
                    .as_ref(),
                );
                return;
            }
            if let Some(pick) = self.aec.aec_layer_gap_draw.as_mut() {
                pick.layer = Some(hit);
                pick.hover = None;
            }
            self.sync_junction_editor_layer_highlight();
            self.command_line.push_info(
                crate::t!("Erste angrenzende Schicht der Stammwand klicken (Esc = Abbrechen).")
                    .as_ref(),
            );
            return;
        }
        if pick.from.is_none() {
            if let Some(pick) = self.aec.aec_layer_gap_draw.as_mut() {
                pick.from = Some(hit);
                pick.hover = None;
            }
            self.sync_junction_editor_layer_highlight();
            self.command_line.push_info(
                crate::t!("Zweite angrenzende Schicht klicken (Esc = Abbrechen).").as_ref(),
            );
            return;
        }
        if let Some(pick) = self.aec.aec_layer_gap_draw.as_mut() {
            pick.to = Some(hit);
            pick.hover = None;
        }
        self.commit_layer_gap_draw();
    }

    fn commit_layer_gap_draw(&mut self) {
        let Some(pick) = self.aec.aec_layer_gap_draw.clone() else {
            return;
        };
        let (Some((wall, idx, mat)), Some((w0, i0, m0)), Some((w1, i1, m1))) =
            (pick.layer, pick.from, pick.to)
        else {
            return;
        };
        let i = self.active_tab;
                use crate::modules::aec::engine::join::{LayerGapOverride, LayerRef};
        let participants = join_ops::walls_at_junction(&self.tabs[i].scene, pick.axis, pick.end_index);
        let resolve = |wall: acadrust::Handle, index: usize, material_id: String| {
            participants
                .iter()
                .find(|p| p.axis_handle == wall)
                .and_then(|p| junction_pick::selected_junction_layer_ref(&p.layers, index, &material_id))
                .unwrap_or(LayerRef {
                    material_id,
                    role_tag: None,
                    index,
                    layer_id: None,
                })
        };
        let mut override_data =
            xdata::read_junction_override(&self.tabs[i].scene, pick.axis, pick.end_index)
                .unwrap_or_default();
        let existing_gaps =
            join_ops::read_through_layer_gaps(&self.tabs[i].scene, pick.axis, pick.end_index);
        if !existing_gaps.is_empty() {
            override_data.layer_gaps = existing_gaps;
        }
        crate::modules::aec::engine::join::upsert_layer_gap(
            &mut override_data.layer_gaps,
            LayerGapOverride {
                layer: resolve(wall, idx, mat),
                from: resolve(w0, i0, m0),
                to: resolve(w1, i1, m1),
            },
        );
        let style_library = crate::modules::aec::engine::project::resolve_style_library(
            self.aec.aec_project_explorer_file.as_ref(),
        );
        let (display_rules, style_substitutions) =
            self.resolve_active_display_config_wall_rules(i, Some(pick.axis));
        let touched = join_ops::apply_junction_override_and_rebuild(
            &mut self.tabs[i].scene,
            pick.axis,
            pick.end_index,
            Some(&override_data),
            Some(&style_library),
            display_rules.as_ref(),
            style_substitutions.as_ref(),
        );
        self.reapply_active_display_config_to_wall_packages(i, &touched);
        self.aec.aec_layer_gap_draw = None;
        self.sync_junction_editor_layer_highlight();
        self.refresh_properties();
        self.command_line
            .push_info(crate::t!("Schichtunterbrechung gesetzt.").as_ref());
    }

    pub(crate) fn apply_layer_pair_draw_style(
        &mut self,
        style: crate::modules::aec::engine::join::JoinOverrideStyle,
    ) {
        let Some(pick) = self.aec.aec_layer_pair_draw.clone() else {
            return;
        };
        let Some((wall_a, idx_a, mat_a)) = pick.layer_a else {
            return;
        };
        let i = self.active_tab;
                use crate::modules::aec::engine::join::{LayerPairOverride, LayerRef};
        let participants = join_ops::walls_at_junction(&self.tabs[i].scene, pick.axis, pick.end_index);
        let resolve = |wall: acadrust::Handle, index: usize, material_id: String| {
            participants
                .iter()
                .find(|p| p.axis_handle == wall)
                .and_then(|p| junction_pick::selected_junction_layer_ref(&p.layers, index, &material_id))
                .unwrap_or(LayerRef {
                    material_id,
                    role_tag: None,
                    index,
                    layer_id: None,
                })
        };
        let layer_a = resolve(wall_a, idx_a, mat_a);
        let layer_b = if pick.layer_b_outer {
            None
        } else {
            pick.layer_b
                .map(|(wall, idx, mat)| resolve(wall, idx, mat))
        };
        let mut override_data =
            xdata::read_junction_override(&self.tabs[i].scene, pick.axis, pick.end_index)
                .unwrap_or_default();
        crate::modules::aec::engine::join::upsert_layer_pair(
            &mut override_data.layer_pairs,
            LayerPairOverride {
                layer_a,
                layer_b,
                style,
            },
        );
        let style_library = crate::modules::aec::engine::project::resolve_style_library(
            self.aec.aec_project_explorer_file.as_ref(),
        );
        let (display_rules, style_substitutions) =
            self.resolve_active_display_config_wall_rules(i, Some(pick.axis));
        let touched = join_ops::apply_junction_override_and_rebuild(
            &mut self.tabs[i].scene,
            pick.axis,
            pick.end_index,
            Some(&override_data),
            Some(&style_library),
            display_rules.as_ref(),
            style_substitutions.as_ref(),
        );
        self.reapply_active_display_config_to_wall_packages(i, &touched);
        self.aec.aec_layer_pair_draw = None;
        let mut sel = self.tabs[i].scene.selection.borrow_mut();
        sel.context_menu = None;
        sel.junction_menu = None;
        drop(sel);
        self.sync_junction_editor_layer_highlight();
        self.refresh_properties();
        self.command_line
            .push_info(crate::t!("Schichtverbindung gesetzt.").as_ref());
    }
}
