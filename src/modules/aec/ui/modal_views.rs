//! AEC modal view packing (formerly Core `app/view/modal.rs` AEC arms).

use iced::Element;

use crate::app::{Message, OpenCADStudio};
use crate::t;

use super::modal_kind::AecModalKind;
use super::modal_windows::{aec_project_required_window, aec_style_copy_conflict_window, aec_drop_dialog_window};

impl OpenCADStudio {
    pub(crate) fn aec_view_modal<'a>(
        &'a self,
        kind: AecModalKind,
        sizing: crate::ui::modal::ModalSizing,
    ) -> Element<'a, Message> {
        match kind {
            AecModalKind::DropWarning => self.aec_drop_warning_view(sizing),
            AecModalKind::Styles => self.aec_styles_view(),
            AecModalKind::WallStyleDisplayProfiles => self.aec_wall_style_display_profiles_view(),
            AecModalKind::JunctionEditor => self.aec_junction_editor_view(),
            AecModalKind::ProjectExplorer => self.aec_project_explorer_view(),
            AecModalKind::StoreySettings => self.aec_storey_settings_view(),
            AecModalKind::StylePicker { target } => self.aec_style_picker_view(target, sizing),
            AecModalKind::StyleCopyConflict => aec_style_copy_conflict_window(sizing),
            AecModalKind::ProjectRequired => aec_project_required_window(sizing),
        }
    }

    fn aec_drop_warning_view(&self, sizing: crate::ui::modal::ModalSizing) -> Element<'_, Message> {
        let src_label = self
            .tabs
            .get(self.active_tab)
            .map(|t| {
                let is_dxf = t
                    .drawing_path()
                    .and_then(|path| path.extension())
                    .and_then(|extension| extension.to_str())
                    .map(|extension| extension.eq_ignore_ascii_case("dxf"))
                    .unwrap_or(false);
                let version = if is_dxf {
                    t.scene.document.version
                } else {
                    t.scene
                        .document
                        .dwg_source_version
                        .unwrap_or(t.scene.document.version)
                };
                crate::io::format_for_version(version, is_dxf)
            })
            .unwrap_or_else(|| "DWG".to_string());
        aec_drop_dialog_window(
            self.aec.aec_drop_count,
            self.save_dialog_format(),
            &src_label,
            sizing,
        )
    }

    fn aec_style_picker_view(
        &self,
        target: crate::modules::aec::StylePickerTarget,
        flow: crate::ui::modal::ModalSizing,
    ) -> Element<'_, Message> {
        let all_layer_names: Vec<String> = self.tabs[self.active_tab]
            .scene
            .document
            .layers
            .iter()
            .map(|l| l.name.clone())
            .collect();
        crate::modules::aec::ui::aec_style_picker::view_window(
            self.aec.aec_style_library.as_ref(),
            self.aec.aec_project_explorer_file.as_ref(),
            self.aec.aec_session_style_library.as_ref(),
            target,
            &self.aec.aec_style_picker_filter,
            self.aec.aec_style_picker_selection.as_deref(),
            all_layer_names,
            flow,
        )
    }

pub(in crate::modules::aec::ui) fn aec_styles_view(&self) -> Element<'_, Message> {
    use crate::modules::aec::ui::style_manager::{self, StylesNode};
    let node = self.aec.styles.node;
    let parts = match node {
        StylesNode::Materials => self.aec_material_parts(),
        StylesNode::Profiles => Some(self.aec_section_profile_parts()),
        StylesNode::FloorFinish => self.aec_floor_finish_parts(),
        StylesNode::Wall => self.aec_wall_parts(),
        StylesNode::Opening => self.aec_opening_parts(),
        StylesNode::Slab => self.aec_slab_parts(),
        StylesNode::SlabStructural => self.aec_slab_structural_parts(),
        StylesNode::PlanKinds => self.aec_plan_parts(),
    };
    style_manager::view(node, parts)
}

fn aec_wall_style_display_profiles_view(&self) -> Element<'_, Message> {
    let Some(library) = self.aec.aec_style_library.as_ref() else {
        return iced::widget::text(t!("No style library loaded.")).into();
    };
    let form = self.aec_wall_style_form_state(library);
    match form.display_profiles {
        Some(profiles) => {
            crate::modules::aec::ui::style_manager::forms::wall::view_display_profiles_window(
                profiles,
                form.layers,
            )
        }
        None => iced::widget::text(t!("No display profiles")).into(),
    }
}

pub(super) fn aec_wall_style_form_state<'a>(
    &'a self,
    library: &'a crate::modules::aec::engine::library::StyleLibrary,
) -> crate::modules::aec::ui::style_manager::forms::wall::WallStyleFormState<'a> {
    use crate::modules::aec::ui::aec_ui_util::StyleEditorFormState;
    use crate::modules::aec::ui::style_manager::forms::wall::{
        DisplayProfileFormState, WallStyleFormState,
    };

    let all_wall_styles: Vec<(&str, &str)> = library
        .wall_styles
        .iter()
        .map(|ws| (ws.style.id.as_str(), ws.style.name.as_str()))
        .collect();
    let all_materials: Vec<(&str, &str)> = library
        .materials
        .iter()
        .map(|m| (m.id.as_str(), m.name.as_str()))
        .collect();
    let all_layer_names: Vec<String> = self.tabs[self.active_tab]
        .scene
        .document
        .layers
        .iter()
        .map(|l| l.name.clone())
        .collect();
    let mut inheritance_chain = Vec::new();
    let mut parent = self.aec.styles.wall_style_parent.as_deref();
    while let Some(id) = parent {
        let Some(ws) = library.wall_styles.iter().find(|s| s.style.id == id) else {
            break;
        };
        inheritance_chain.push(ws.style.name.clone());
        parent = ws.style.parent.as_deref();
    }
    inheritance_chain.reverse();

    let mut display_config_names: Vec<String> = vec![
        crate::modules::aec::engine::library::STYLE_DEFAULT_PROFILE_KEY.to_string(),
    ];
    display_config_names.extend(
        self.aec
            .aec_plan_library
            .as_ref()
            .map(|lib| lib.configs.iter().map(|c| c.name.clone()).collect::<Vec<_>>())
            .unwrap_or_default(),
    );
    let existing_overrides = self
        .aec.styles.wall_style_editing_id
        .as_deref()
        .map(|id| self.aec_wall_profile_keys(id).into_iter().collect())
        .unwrap_or_default();

    let slot_style_editor = StyleEditorFormState {
        line_type: &self.aec.styles.profile_slot_style_line_type,
        linetype_items: &self.aec.styles.material_linetype_items,
        linetype_combo: &self.aec.styles.material_linetype_combo,
        line_color: &self.aec.styles.profile_slot_style_line_color,
        line_color_picker_open: self.aec.styles.profile_slot_style_line_color_picker_open,
        hatch_pattern: &self.aec.styles.profile_slot_style_hatch_pattern,
        hatch_picker_open: self.aec.styles.profile_slot_style_hatch_picker_open,
        hatch_color: &self.aec.styles.profile_slot_style_hatch_color,
        hatch_color_picker_open: self
            .aec.styles.profile_slot_style_hatch_color_picker_open,
        fill_color: &self.aec.styles.profile_slot_style_fill_color,
        fill_color_picker_open: self.aec.styles.profile_slot_style_fill_color_picker_open,
    };

    let display_profiles = if self.aec.styles.wall_style_editing_id.is_some() {
        Some(DisplayProfileFormState {
            display_config_names,
            existing_overrides,
            selected: self.aec.styles.profile_selected.as_deref(),
            contour_explicit: self.aec.styles.profile_contour_explicit,
            contour_selected: &self.aec.styles.profile_contour_selection,
            solid_explicit: self.aec.styles.profile_solid_explicit,
            solid_selected: &self.aec.styles.profile_solid_selection,
            hatch_angle: &self.aec.styles.profile_hatch_angle,
            hatch_relative: self.aec.styles.profile_hatch_relative,
            slot_visibility: self.aec.styles.profile_slot_visibility.clone(),
            editing_slot: self.aec.styles.profile_editing_slot,
            slot_overrides: &self.aec.styles.profile_slot_overrides,
            slot_style_editor,
        })
    } else {
        None
    };

    WallStyleFormState {
        open: self.aec.styles.wall_style_form_open,
        is_new: self.aec.styles.wall_style_editing_id.is_none(),
        name: &self.aec.styles.wall_style_name,
        parent_id: self.aec.styles.wall_style_parent.as_deref(),
        layers: &self.aec.styles.wall_style_layers,
        drag_index: self.aec.styles.wall_style_drag_index,
        all_wall_styles,
        all_materials,
        all_layer_names,
        effective_layers: Vec::new(),
        inheritance_chain,
        display_profiles,
        preview_layers: crate::modules::aec::ui::style_manager::forms::wall_preview::preview_layers(
            &self.aec.styles.wall_style_layers,
            &|id| {
                library
                    .materials
                    .iter()
                    .find(|m| m.id == id)
                    .map(|m| (m.line_color, m.hatch_pattern.clone()))
            },
        ),
        preview_mode: self.aec.styles.wall_preview_mode,
        selected_layer: self.aec.styles.wall_selected_layer,
        hatch_picker: self.aec.styles.wall_layer_hatch_picker,
    }
}

fn aec_junction_editor_view(&self) -> Element<'_, Message> {
    let Some(library) = self.aec.aec_style_library.as_ref() else {
        return iced::widget::text(t!("No style library loaded.")).into();
    };
    let Some((axis_handle, end_index)) = self.aec.aec_junction_editor_target else {
        return iced::widget::text(t!("No junction selected")).into();
    };
    let participants = crate::modules::aec::engine::join_ops::walls_at_junction(
        &self.tabs[self.active_tab].scene,
        axis_handle,
        end_index,
    );
    crate::modules::aec::ui::aec_junction_editor::view_window(
        crate::modules::aec::ui::aec_junction_editor::JunctionEditorState {
            axis_handle,
            end_index,
            participants,
            library,
            default_style: self.aec.aec_junction_editor_default_style.clone(),
            pairs: &self.aec.aec_junction_editor_pairs,
            pair_layer_a: self
                .aec.aec_junction_editor_pair_layer_a
                .as_ref()
                .map(|(i, s)| (*i, s.as_str())),
            pair_wall_b: self.aec.aec_junction_editor_pair_wall_b,
            pair_layer_b: self
                .aec.aec_junction_editor_pair_layer_b
                .as_ref()
                .map(|(i, s)| (*i, s.as_str())),
            pair_style: self.aec.aec_junction_editor_pair_style.clone(),
            gaps: &self.aec.aec_junction_editor_gaps,
            gap_layer: self
                .aec.aec_junction_editor_gap_layer
                .as_ref()
                .map(|(i, s)| (*i, s.as_str())),
            gap_from_wall: self.aec.aec_junction_editor_gap_from_wall,
            gap_from: self
                .aec.aec_junction_editor_gap_from
                .as_ref()
                .map(|(i, s)| (*i, s.as_str())),
            gap_to_wall: self.aec.aec_junction_editor_gap_to_wall,
            gap_to: self
                .aec.aec_junction_editor_gap_to
                .as_ref()
                .map(|(i, s)| (*i, s.as_str())),
        },
    )
}

fn aec_project_explorer_view(&self) -> Element<'_, Message> {
    crate::modules::aec::ui::aec_project_explorer::view_window(
        self.aec.aec_project_explorer_file.as_ref(),
        crate::modules::aec::ui::aec_project_explorer::ProjectExplorerState {
            path: self.aec.aec_project_explorer_path.as_deref(),
            selected_building: self.aec.aec_project_explorer_selected_building,
            selected_storey: self.aec.aec_project_explorer_selected_storey,
            new_building_name: &self.aec.aec_project_explorer_new_building_name,
            new_storey_name: &self.aec.aec_project_explorer_new_storey_name,
            new_storey_elevation: &self.aec.aec_project_explorer_new_storey_elevation,
            new_storey_drawing: &self.aec.aec_project_explorer_new_storey_drawing,
            edit_building_name: &self.aec.aec_project_explorer_edit_building_name,
            pending_delete: self.aec.aec_project_explorer_pending_delete,
            ffl0_nn: &self.aec.aec_project_explorer_ffl0_nn,
        },
    )
}

fn aec_storey_settings_view(&self) -> Element<'_, Message> {
    let Some((bid, sid)) = self.aec.aec_storey_settings_target else {
        return iced::widget::text(t!("No storey selected.")).into();
    };
    let Some(storey) = self.aec.aec_project_explorer_file.as_ref().and_then(|p| {
        p.buildings
            .iter()
            .find(|b| b.id == bid)
            .and_then(|b| b.storeys.iter().find(|s| s.id == sid))
    }) else {
        return iced::widget::text(t!("Storey not found.")).into();
    };
    crate::modules::aec::ui::aec_storey_settings::view_window(
        crate::modules::aec::ui::aec_storey_settings::StoreySettingsState {
            building_id: bid,
            storey,
            new_plane_name: &self.aec.aec_storey_settings_new_plane_name,
            new_plane_z: &self.aec.aec_storey_settings_new_plane_z,
            plane_z: &self.aec.aec_storey_settings_plane_z,
            facet_z: &self.aec.aec_storey_settings_facet_z,
            elevation: &self.aec.aec_storey_settings_elevation,
            height: &self.aec.aec_storey_settings_height,
        },
    )
}

}
