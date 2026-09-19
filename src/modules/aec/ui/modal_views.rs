//! AEC modal view packing (formerly Core `app/view/modal.rs` AEC arms).

use iced::widget::{button, column, container, row, text};
use iced::{Background, Element, Theme};
use std::borrow::Cow;

use crate::app::{Message, OpenCADStudio};
use crate::modules::aec::AecMessage;
use crate::t;

use super::modal_kind::AecModalKind;

impl OpenCADStudio {
    pub(crate) fn aec_view_modal<'a>(
        &'a self,
        kind: AecModalKind,
        sizing: crate::ui::modal::ModalSizing,
    ) -> Element<'a, Message> {
        match kind {
            AecModalKind::DropWarning => self.aec_drop_warning_view(sizing),
            AecModalKind::MaterialManager => self.aec_material_manager_view(),
            AecModalKind::WallStyleManager => self.aec_wall_style_manager_view(),
            AecModalKind::WallStyleDisplayProfiles => self.aec_wall_style_display_profiles_view(),
            AecModalKind::JunctionEditor => self.aec_junction_editor_view(),
            AecModalKind::ProjectExplorer => self.aec_project_explorer_view(),
            AecModalKind::StoreySettings => self.aec_storey_settings_view(),
            AecModalKind::PlanManager => self.aec_plan_manager_view(),
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

fn aec_material_manager_view(&self) -> Element<'_, Message> {
    let Some(library) = self.aec.aec_style_library.as_ref() else {
        return iced::widget::text(t!("No style library loaded.")).into();
    };
    crate::modules::aec::ui::aec_material_manager::view_window(
        library,
        self.aec.aec_project_explorer_file.as_ref(),
        self.aec.aec_session_style_library.as_ref(),
        self.aec.aec_style_manager_selected_material.as_deref(),
        &self.aec.aec_style_manager_filter,
        crate::modules::aec::ui::aec_material_manager::MaterialFormState {
            open: self.aec.aec_style_manager_material_form_open,
            is_new: self.aec.aec_style_manager_material_editing_id.is_none(),
            editing_id: self.aec.aec_style_manager_material_editing_id.as_deref(),
            name: &self.aec.aec_style_manager_material_name,
            hatch: &self.aec.aec_style_manager_material_hatch,
            color: &self.aec.aec_style_manager_material_color,
            line_type: &self.aec.aec_style_manager_material_line_type,
            category: &self.aec.aec_style_manager_material_category,
            hatch_color: self.aec.aec_style_manager_material_hatch_color,
            hatch_scale: &self.aec.aec_style_manager_material_hatch_scale,
            render_material_ref: &self.aec.aec_style_manager_material_render_ref,
            hatch_angle: &self.aec.aec_style_manager_material_hatch_angle,
            hatch_angle_relative: self.aec.aec_style_manager_material_hatch_angle_relative,
            hatch_picker_open: self.aec.aec_style_manager_material_hatch_picker_open,
            color_picker_open: self.aec.aec_style_manager_material_color_picker_open,
            hatch_color_picker_open: self.aec.aec_style_manager_material_hatch_color_picker_open,
            linetype_items: &self.aec.aec_style_manager_material_linetype_items,
            linetype_combo: &self.aec.aec_style_manager_material_linetype_combo,
        },
    )
}

fn aec_wall_style_manager_view(&self) -> Element<'_, Message> {
    let Some(library) = self.aec.aec_style_library.as_ref() else {
        return iced::widget::text(t!("No style library loaded.")).into();
    };
    let form = self.aec_wall_style_form_state(library);
    crate::modules::aec::ui::aec_wall_style_manager::view_window(
        library,
        self.aec.aec_project_explorer_file.as_ref(),
        self.aec.aec_session_style_library.as_ref(),
        self.aec.aec_style_manager_selected_wall_style.as_deref(),
        &self.aec.aec_style_manager_filter,
        form,
    )
}

fn aec_wall_style_display_profiles_view(&self) -> Element<'_, Message> {
    let Some(library) = self.aec.aec_style_library.as_ref() else {
        return iced::widget::text(t!("No style library loaded.")).into();
    };
    let form = self.aec_wall_style_form_state(library);
    match form.display_profiles {
        Some(profiles) => {
            crate::modules::aec::ui::aec_wall_style_manager::view_display_profiles_window(
                profiles,
                form.layers,
            )
        }
        None => iced::widget::text(t!("No display profiles")).into(),
    }
}

fn aec_wall_style_form_state<'a>(
    &'a self,
    library: &'a crate::modules::aec::engine::library::StyleLibrary,
) -> crate::modules::aec::ui::aec_wall_style_manager::WallStyleFormState<'a> {
    use crate::modules::aec::ui::aec_ui_util::StyleEditorFormState;
    use crate::modules::aec::ui::aec_wall_style_manager::{
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
    let mut parent = self.aec.aec_style_manager_wall_style_parent.as_deref();
    while let Some(id) = parent {
        let Some(ws) = library.wall_styles.iter().find(|s| s.style.id == id) else {
            break;
        };
        inheritance_chain.push(ws.style.name.clone());
        parent = ws.style.parent_style_id.as_deref();
    }
    inheritance_chain.reverse();

    let display_config_names: Vec<String> = self
        .aec.aec_plan_library
        .as_ref()
        .map(|lib| lib.configs.iter().map(|c| c.name.clone()).collect())
        .unwrap_or_default();
    let existing_overrides = self
        .aec.aec_style_manager_wall_style_editing_id
        .as_deref()
        .and_then(|id| library.wall_styles.iter().find(|ws| ws.style.id == id))
        .map(|ws| ws.display_profiles.keys().cloned().collect())
        .unwrap_or_default();

    let slot_style_editor = StyleEditorFormState {
        line_type: &self.aec.aec_style_manager_profile_slot_style_line_type,
        linetype_items: &self.aec.aec_style_manager_material_linetype_items,
        linetype_combo: &self.aec.aec_style_manager_material_linetype_combo,
        line_color: &self.aec.aec_style_manager_profile_slot_style_line_color,
        line_color_picker_open: self.aec.aec_style_manager_profile_slot_style_line_color_picker_open,
        hatch_pattern: &self.aec.aec_style_manager_profile_slot_style_hatch_pattern,
        hatch_picker_open: self.aec.aec_style_manager_profile_slot_style_hatch_picker_open,
        hatch_color: &self.aec.aec_style_manager_profile_slot_style_hatch_color,
        hatch_color_picker_open: self
            .aec.aec_style_manager_profile_slot_style_hatch_color_picker_open,
        fill_color: &self.aec.aec_style_manager_profile_slot_style_fill_color,
        fill_color_picker_open: self.aec.aec_style_manager_profile_slot_style_fill_color_picker_open,
    };

    let display_profiles = if self.aec.aec_style_manager_wall_style_editing_id.is_some() {
        Some(DisplayProfileFormState {
            display_config_names,
            existing_overrides,
            selected: self.aec.aec_style_manager_profile_selected.as_deref(),
            contour_explicit: self.aec.aec_style_manager_profile_contour_explicit,
            contour_selected: &self.aec.aec_style_manager_profile_contour_selection,
            solid_explicit: self.aec.aec_style_manager_profile_solid_explicit,
            solid_selected: &self.aec.aec_style_manager_profile_solid_selection,
            hatch_angle: &self.aec.aec_style_manager_profile_hatch_angle,
            hatch_relative: self.aec.aec_style_manager_profile_hatch_relative,
            slot_visibility: self.aec.aec_style_manager_profile_slot_visibility.clone(),
            editing_slot: self.aec.aec_style_manager_profile_editing_slot,
            slot_overrides: &self.aec.aec_style_manager_profile_slot_overrides,
            slot_style_editor,
        })
    } else {
        None
    };

    WallStyleFormState {
        open: self.aec.aec_style_manager_wall_style_form_open,
        is_new: self.aec.aec_style_manager_wall_style_editing_id.is_none(),
        name: &self.aec.aec_style_manager_wall_style_name,
        parent_id: self.aec.aec_style_manager_wall_style_parent.as_deref(),
        layers: &self.aec.aec_style_manager_wall_style_layers,
        drag_index: self.aec.aec_style_manager_wall_style_drag_index,
        all_wall_styles,
        all_materials,
        all_layer_names,
        effective_layers: Vec::new(),
        inheritance_chain,
        display_profiles,
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
            elevation: &self.aec.aec_storey_settings_elevation,
            height: &self.aec.aec_storey_settings_height,
        },
    )
}

fn aec_plan_manager_view(&self) -> Element<'_, Message> {
    use crate::modules::aec::ui::aec_plan_manager::PlanConfigFormState;
    use crate::modules::aec::ui::aec_ui_util::StyleEditorFormState;
    let Some(library) = self.aec.aec_plan_library.as_ref() else {
        return iced::widget::text(t!("No plan library loaded.")).into();
    };
    let demolition_style = StyleEditorFormState {
        line_type: &self.aec.aec_plan_manager_demolition_style_line_type,
        linetype_items: &self.aec.aec_style_manager_material_linetype_items,
        linetype_combo: &self.aec.aec_style_manager_material_linetype_combo,
        line_color: &self.aec.aec_plan_manager_demolition_style_line_color,
        line_color_picker_open: self.aec.aec_plan_manager_demolition_style_line_color_picker_open,
        hatch_pattern: &self.aec.aec_plan_manager_demolition_style_hatch_pattern,
        hatch_picker_open: self.aec.aec_plan_manager_demolition_style_hatch_picker_open,
        hatch_color: &self.aec.aec_plan_manager_demolition_style_hatch_color,
        hatch_color_picker_open: self.aec.aec_plan_manager_demolition_style_hatch_color_picker_open,
        fill_color: &self.aec.aec_plan_manager_demolition_style_fill_color,
        fill_color_picker_open: self.aec.aec_plan_manager_demolition_style_fill_color_picker_open,
    };
    let existing_style = StyleEditorFormState {
        line_type: &self.aec.aec_plan_manager_existing_style_line_type,
        linetype_items: &self.aec.aec_style_manager_material_linetype_items,
        linetype_combo: &self.aec.aec_style_manager_material_linetype_combo,
        line_color: &self.aec.aec_plan_manager_existing_style_line_color,
        line_color_picker_open: self.aec.aec_plan_manager_existing_style_line_color_picker_open,
        hatch_pattern: &self.aec.aec_plan_manager_existing_style_hatch_pattern,
        hatch_picker_open: self.aec.aec_plan_manager_existing_style_hatch_picker_open,
        hatch_color: &self.aec.aec_plan_manager_existing_style_hatch_color,
        hatch_color_picker_open: self.aec.aec_plan_manager_existing_style_hatch_color_picker_open,
        fill_color: &self.aec.aec_plan_manager_existing_style_fill_color,
        fill_color_picker_open: self.aec.aec_plan_manager_existing_style_fill_color_picker_open,
    };
    crate::modules::aec::ui::aec_plan_manager::view_window(
        library,
        self.aec.aec_plan_manager_selected.as_deref(),
        &self.aec.aec_plan_manager_filter,
        PlanConfigFormState {
            open: self.aec.aec_plan_manager_form_open,
            is_new: self.aec.aec_plan_manager_editing_name.is_none(),
            editing_name: self.aec.aec_plan_manager_editing_name.as_deref(),
            name: &self.aec.aec_plan_manager_name,
            discipline: &self.aec.aec_plan_manager_discipline,
            scale: &self.aec.aec_plan_manager_scale,
            planning_stage: self.aec.aec_plan_manager_planning_stage,
            view_type: self.aec.aec_plan_manager_view_type,
            phase_filter_visible_existing: self.aec.aec_plan_manager_phase_filter_visible_existing,
            phase_filter_visible_demolition: self.aec.aec_plan_manager_phase_filter_visible_demolition,
            phase_filter_visible_new: self.aec.aec_plan_manager_phase_filter_visible_new,
            demolition_style,
            existing_style,
            default_representation: self.aec.aec_plan_manager_default_representation,
            component_visibility: &self.aec.aec_plan_manager_component_visibility,
            wall_styles: &self.aec.aec_plan_manager_wall_styles,
            style_overlays: &self.aec.aec_plan_manager_style_overlays,
            overlay_style_id: self.aec.aec_plan_manager_overlay_style_id.as_deref(),
            overlay_layer_id: self.aec.aec_plan_manager_overlay_layer_id,
            overlay_line_type: &self.aec.aec_plan_manager_overlay_line_type,
            overlay_line_color: &self.aec.aec_plan_manager_overlay_line_color,
            overlay_hatch_pattern: &self.aec.aec_plan_manager_overlay_hatch_pattern,
            overlay_hatch_color: &self.aec.aec_plan_manager_overlay_hatch_color,
            overlay_hatch_scale: &self.aec.aec_plan_manager_overlay_hatch_scale,
            overlay_hatch_angle: &self.aec.aec_plan_manager_overlay_hatch_angle,
            overlay_hatch_angle_relative: self.aec.aec_plan_manager_overlay_hatch_angle_relative,
            overlay_fill_color: &self.aec.aec_plan_manager_overlay_fill_color,
            overlay_linetype_items: &self.aec.aec_style_manager_material_linetype_items,
            overlay_linetype_combo: &self.aec.aec_style_manager_material_linetype_combo,
            overlay_line_color_picker_open: self.aec.aec_plan_manager_overlay_line_color_picker_open,
            overlay_hatch_picker_open: self.aec.aec_plan_manager_overlay_hatch_picker_open,
            overlay_hatch_color_picker_open: self.aec.aec_plan_manager_overlay_hatch_color_picker_open,
            overlay_fill_color_picker_open: self.aec.aec_plan_manager_overlay_fill_color_picker_open,
            contour_hatch_pattern: &self.aec.aec_plan_manager_contour_hatch_pattern,
            contour_hatch_color: &self.aec.aec_plan_manager_contour_hatch_color,
            contour_hatch_scale: &self.aec.aec_plan_manager_contour_hatch_scale,
            contour_hatch_angle: &self.aec.aec_plan_manager_contour_hatch_angle,
            contour_hatch_angle_relative: self.aec.aec_plan_manager_contour_hatch_angle_relative,
            contour_hatch_picker_open: self.aec.aec_plan_manager_contour_hatch_picker_open,
            contour_hatch_color_picker_open: self.aec.aec_plan_manager_contour_hatch_color_picker_open,
        },
    )
}
}

fn dialog_button<'a>(
    label: impl Into<Cow<'a, str>>,
    message: Message,
    style: fn(&Theme, button::Status) -> button::Style,
) -> Element<'a, Message> {
    button(text(label.into()).size(13))
        .on_press(message)
        .style(style)
        .padding([6, 18])
        .into()
}

fn dialog_body_style(theme: &Theme) -> container::Style {
    let palette = theme.palette();
    container::Style {
        background: Some(Background::Color(palette.background.base.color)),
        text_color: Some(palette.background.base.text),
        ..Default::default()
    }
}

/// Warning shown before a lossy Save-As: the drawing carries unsupported
/// (AEC / application) objects that survive only as verbatim source-version
/// bytes, so saving to a different version or to DXF would drop them. Offers to
/// save in the source version (keep them) or proceed (drop them).
fn aec_drop_dialog_window(
    count: usize,
    target: &str,
    src_version: &str,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'static, Message> {
    let body_text = t!(
        "This drawing contains %{count} AEC/Civil objects that \"%{target}\" cannot store, so they will not be saved.\n\nTo keep them, save in the source version (%{src_version}).",
        count = count,
        target = target,
        src_version = src_version,
    );

    container(
        column![
            text(body_text).size(13),
            iced::widget::Space::new().height(20),
            row![
                dialog_button(
                    t!("Save in source version"),
                    Message::AecDropSameVersion,
                    button::primary
                ),
                iced::widget::Space::new().width(8),
                dialog_button(t!("Save anyway"), Message::AecDropProceed, button::warning),
                iced::widget::Space::new().width(8),
                dialog_button(t!("Back"), Message::AecDropBack, button::secondary),
            ],
        ]
        .spacing(0),
    )
    .style(dialog_body_style)
    .center_x(sizing.width)
    .center_y(sizing.height)
    .padding([24, 28])
    .into()
}

/// Confirm overwriting an entry that already exists (with different
/// content) in the target library during an AEC Style Manager copy (Step
/// 9). "Overwrite" proceeds with the copy; "Cancel" leaves both libraries
/// untouched.
fn aec_style_copy_conflict_window(
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'static, Message> {
    let body_text = t!(
        "An entry with this name/id already exists in the target library with different content.\n\nOverwrite it?"
    );

    container(
        column![
            text(body_text).size(13),
            iced::widget::Space::new().height(20),
            row![
                dialog_button(
                    t!("Overwrite"),
                    Message::Aec(AecMessage::AecStyleManagerCopyConflictConfirm(true)),
                    button::danger
                ),
                iced::widget::Space::new().width(8),
                dialog_button(
                    t!("Cancel"),
                    Message::Aec(AecMessage::AecStyleManagerCopyConflictConfirm(false)),
                    button::secondary
                ),
            ],
        ]
        .spacing(0),
    )
    .style(dialog_body_style)
    .center_x(sizing.width)
    .center_y(sizing.height)
    .padding([24, 28])
    .into()
}

/// Blocking prompt when Architecture tools are used without an active
/// project. "Open Project" / "New Project" route into the Project Explorer
/// flow; closing the modal leaves the entry point unopened.
fn aec_project_required_window(
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'static, Message> {
    let body_text = t!(
        "Architecture tools require an active project.\n\nOpen an existing project or create a new one to continue."
    );

    container(
        column![
            text(body_text).size(13),
            iced::widget::Space::new().height(20),
            row![
                dialog_button(
                    t!("Open Project"),
                    Message::Aec(AecMessage::AecProjectExplorerLoad),
                    button::primary
                ),
                iced::widget::Space::new().width(8),
                dialog_button(
                    t!("New Project"),
                    Message::Aec(AecMessage::AecProjectExplorerNew),
                    button::secondary
                ),
            ],
        ]
        .spacing(0),
    )
    .style(dialog_body_style)
    .center_x(sizing.width)
    .center_y(sizing.height)
    .padding([24, 28])
    .into()
}
