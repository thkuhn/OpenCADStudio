//! Style-manager part builders for the AEC styles modal.

use crate::app::OpenCADStudio;

impl OpenCADStudio {
pub(super) fn aec_material_parts(&self) -> Option<crate::modules::aec::ui::style_manager::StyleParts<'_>> {
    let library = self.aec.aec_style_library.as_ref()?;
    Some(crate::modules::aec::ui::style_manager::forms::material::parts(
        library,
        self.aec.aec_project_explorer_file.as_ref(),
        self.aec.aec_session_style_library.as_ref(),
        self.aec.styles.selected_material.as_deref(),
        &self.aec.styles.filter,
        crate::modules::aec::ui::style_manager::forms::material::MaterialFormState {
            open: self.aec.styles.material_form_open,
            is_new: self.aec.styles.material_editing_id.is_none(),
            editing_id: self.aec.styles.material_editing_id.as_deref(),
            name: &self.aec.styles.material_name,
            hatch: &self.aec.styles.material_hatch,
            color: &self.aec.styles.material_color,
            line_type: &self.aec.styles.material_line_type,
            category: &self.aec.styles.material_category,
            hatch_color: self.aec.styles.material_hatch_color,
            hatch_scale: &self.aec.styles.material_hatch_scale,
            render_material_ref: &self.aec.styles.material_render_ref,
            hatch_angle: &self.aec.styles.material_hatch_angle,
            hatch_angle_relative: self.aec.styles.material_hatch_angle_relative,
            hatch_picker_open: self.aec.styles.material_hatch_picker_open,
            color_picker_open: self.aec.styles.material_color_picker_open,
            hatch_color_picker_open: self.aec.styles.material_hatch_color_picker_open,
            linetype_items: &self.aec.styles.material_linetype_items,
            linetype_combo: &self.aec.styles.material_linetype_combo,
        },
    ))
}

pub(super) fn aec_opening_parts(&self) -> Option<crate::modules::aec::ui::style_manager::StyleParts<'_>> {
    let library = self.aec.aec_style_library.as_ref()?;
    let parent_name = self.aec.styles.opening_parent.as_deref().and_then(|id| {
        library
            .opening_styles
            .iter()
            .find(|s| s.style.id == id)
            .map(|s| s.style.name.as_str())
    });
    Some(crate::modules::aec::ui::style_manager::forms::opening::parts(
        library,
        self.aec.aec_project_explorer_file.as_ref(),
        self.aec.aec_session_style_library.as_ref(),
        self.aec.styles.opening_selected.as_deref(),
        &self.aec.styles.opening_filter,
        crate::modules::aec::ui::style_manager::forms::opening::OpeningStyleFormState {
            open: self.aec.styles.opening_form_open,
            is_new: self.aec.styles.opening_editing_id.is_none(),
            preview_mode: self.aec.styles.opening_preview_mode,
            editor_tab: self.aec.styles.opening_editor_tab,
            component_enabled: &self.aec.styles.opening_component_enabled,
            profile_ids: &self.aec.styles.opening_profile_ids,
            look_buffers: &self.aec.styles.opening_look_buffers,
            look_color_picker_role: self.aec.styles.opening_look_color_picker_role,
            selected_look_role: self.aec.styles.opening_selected_look_role,
            linetype_items: &self.aec.styles.material_linetype_items,
            linetype_combo: &self.aec.styles.material_linetype_combo,
            name: &self.aec.styles.opening_name,
            parent_id: self.aec.styles.opening_parent.as_deref(),
            parent_name,
            kind: self.aec.styles.opening_kind,
            shape: self.aec.styles.opening_shape,
            width: &self.aec.styles.opening_width,
            height: &self.aec.styles.opening_height,
            sill: &self.aec.styles.opening_sill,
            frame: &self.aec.styles.opening_frame,
            frame_depth: &self.aec.styles.opening_frame_depth,
            frame_offset: &self.aec.styles.opening_frame_offset,
            leaf_face: &self.aec.styles.opening_leaf_face,
            leaf_depth: &self.aec.styles.opening_leaf_depth,
            leaf_rebate: &self.aec.styles.opening_leaf_rebate,
            leaf_lateral: &self.aec.styles.opening_leaf_lateral,
            glazing_thickness: &self.aec.styles.opening_glazing_thickness,
            glazing_inset: &self.aec.styles.opening_glazing_inset,
            ext_sill_thickness: &self.aec.styles.opening_ext_sill_thickness,
            ext_sill_overhang: &self.aec.styles.opening_ext_sill_overhang,
            ext_sill_upstand: &self.aec.styles.opening_ext_sill_upstand,
            ext_sill_side_left: &self.aec.styles.opening_ext_sill_side_left,
            ext_sill_side_right: &self.aec.styles.opening_ext_sill_side_right,
            int_board_thickness: &self.aec.styles.opening_int_board_thickness,
            int_board_overhang: &self.aec.styles.opening_int_board_overhang,
            int_board_upstand: &self.aec.styles.opening_int_board_upstand,
            int_board_side_left: &self.aec.styles.opening_int_board_side_left,
            int_board_side_right: &self.aec.styles.opening_int_board_side_right,
            threshold_height: &self.aec.styles.opening_threshold_height,
            threshold_width: &self.aec.styles.opening_threshold_width,
            shutter_height: &self.aec.styles.opening_shutter_height,
            shutter_depth: &self.aec.styles.opening_shutter_depth,
            shutter_seat: &self.aec.styles.opening_shutter_seat,
            shutter_install: &self.aec.styles.opening_shutter_install,
            lintel_height: &self.aec.styles.opening_lintel_height,
            lintel_left: &self.aec.styles.opening_lintel_left,
            lintel_right: &self.aec.styles.opening_lintel_right,
            vertical: &self.aec.styles.opening_vertical,
            horizontal: &self.aec.styles.opening_horizontal,
            vertical_muntins: &self.aec.styles.opening_vertical_muntins,
            horizontal_muntins: &self.aec.styles.opening_horizontal_muntins,
            cells: &self.aec.styles.opening_cells,
            selected_cell: self.aec.styles.opening_selected_cell,
            vertical_positions: &self.aec.styles.opening_vertical_positions,
            horizontal_positions: &self.aec.styles.opening_horizontal_positions,
            vertical_muntin_positions: &self.aec.styles.opening_vertical_muntin_positions,
            horizontal_muntin_positions: &self.aec.styles.opening_horizontal_muntin_positions,
            plan_block: &self.aec.styles.opening_plan_block,
            plan_placement: self.aec.styles.opening_plan_placement,
            section_block: &self.aec.styles.opening_section_block,
            section_placement: self.aec.styles.opening_section_placement,
            elevation_block: &self.aec.styles.opening_elevation_block,
            elevation_placement: self.aec.styles.opening_elevation_placement,
            angle: &self.aec.styles.opening_angle,
            spring: &self.aec.styles.opening_spring,
            hinge: self.aec.styles.opening_hinge,
            swing_side: self.aec.styles.opening_swing_side,
            authored_grid: self.aec.styles.opening_authored_grid.as_ref(),
            grid_path: &self.aec.styles.opening_grid_path,
            selected_grid_edge: self.aec.styles.opening_selected_grid_edge,
            grid_measure: self.aec.styles.opening_grid_measure.as_deref(),
            preview_isometric: self.aec.styles.opening_preview_isometric,
            preview_zoom: self.aec.styles.opening_preview_zoom,
            preview_pan_x: self.aec.styles.opening_preview_pan_x,
            preview_pan_y: self.aec.styles.opening_preview_pan_y,
            preview_model_cache: &self.aec.styles.opening_preview_model_cache,
        },
    ))
}

pub(super) fn aec_slab_parts(&self) -> Option<crate::modules::aec::ui::style_manager::StyleParts<'_>> {
    let library = self.aec.aec_style_library.as_ref()?;
    let parent_name = self
        .aec
        .styles.slab_parent
        .as_deref()
        .and_then(|id| {
            library
                .slab_styles
                .iter()
                .find(|s| s.style.id == id)
                .map(|s| s.style.name.clone())
        });
    let all_slab_styles: Vec<(&str, &str)> = library
        .slab_styles
        .iter()
        .map(|s| (s.style.id.as_str(), s.style.name.as_str()))
        .collect();
    let all_structural_styles: Vec<(&str, &str)> = library
        .slab_structural_styles
        .iter()
        .map(|s| (s.style.id.as_str(), s.style.name.as_str()))
        .collect();
    let all_finish_styles: Vec<(&str, &str)> = library
        .floor_finish_styles
        .iter()
        .map(|f| (f.style.id.as_str(), f.style.name.as_str()))
        .collect();
    let all_materials: Vec<(&str, &str)> = library
        .materials
        .iter()
        .map(|m| (m.id.as_str(), m.name.as_str()))
        .collect();

    let referenced_structural = self
        .aec
        .styles.slab_structural_style
        .as_deref()
        .and_then(|id| library.find_slab_structural_style(id));
    let referenced_finish = self
        .aec
        .styles.slab_finish_style
        .as_deref()
        .and_then(|id| library.find_floor_finish_style(id));

    let mut dummy_style = crate::modules::aec::engine::slab_style::SlabStyle::new("dummy", "dummy");
    dummy_style.structural_style_id = self.aec.styles.slab_structural_style.clone();
    dummy_style.default_finish_style_id = self.aec.styles.slab_finish_style.clone();

    let s_map: std::collections::HashMap<String, _> = library
        .slab_structural_styles
        .iter()
        .map(|s| (s.style.id.clone(), s.clone()))
        .collect();
    let f_map: std::collections::HashMap<String, _> = library
        .floor_finish_styles
        .iter()
        .map(|f| (f.style.id.clone(), f.clone()))
        .collect();

    let effective_layers = crate::modules::aec::engine::slab_style::resolve_slab_style_layers(
        &dummy_style,
        Some(&s_map),
        Some(&f_map),
    );

    let form = if self.aec.styles.slab_form_open {
        Some(crate::modules::aec::ui::style_manager::forms::slab::SlabStyleFormState {
            open: true,
            is_new: self.aec.styles.slab_editing_id.is_none(),
            name: &self.aec.styles.slab_name,
            parent_id: self.aec.styles.slab_parent.as_deref(),
            parent_name,
            structural_style_id: self.aec.styles.slab_structural_style.as_deref(),
            finish_style_id: self.aec.styles.slab_finish_style.as_deref(),
            referenced_structural,
            referenced_finish,
            all_slab_styles,
            all_structural_styles,
            all_finish_styles,
            all_materials,
            effective_layers,
            inheritance_chain: Vec::new(),
            preview_mode: self.aec.styles.slab_preview_mode,
            display_config_names: self
                .aec
                .aec_plan_library
                .as_ref()
                .map(|lib| lib.configs.iter().map(|c| c.name.clone()).collect())
                .unwrap_or_default(),
            existing_overrides: self.aec_slab_profile_keys(),
            selected_layer: self.aec.styles.slab_selected_layer,
            profile_selected: self.aec.styles.slab_profile_selected.as_deref(),
            slot_visibility: self.aec.styles.slab_profile_slot_visibility.clone(),
        })
    } else {
        None
    };

    Some(crate::modules::aec::ui::style_manager::forms::slab::parts(
        library,
        self.aec.aec_project_explorer_file.as_ref(),
        self.aec.aec_session_style_library.as_ref(),
        &self.aec.styles.slab_filter,
        self.aec.styles.slab_selected.as_deref(),
        form,
    ))
}

pub(super) fn aec_slab_structural_parts(&self) -> Option<crate::modules::aec::ui::style_manager::StyleParts<'_>> {
    let library = self.aec.aec_style_library.as_ref()?;

    let parent_name = self
        .aec
        .styles.slab_structural_parent
        .as_ref()
        .and_then(|id| {
            library
                .slab_structural_styles
                .iter()
                .find(|s| s.style.id == *id)
                .map(|s| s.style.name.clone())
        });
    let all_structural_styles: Vec<(&str, &str)> = library
        .slab_structural_styles
        .iter()
        .map(|s| (s.style.id.as_str(), s.style.name.as_str()))
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

    let mut effective_layers = Vec::new();
    for buf in &self.aec.styles.slab_structural_layers {
        if let Ok(layer) = crate::modules::aec::styles::slab_style_manager::buffer_to_layer(buf) {
            effective_layers.push(layer);
        }
    }

    let form = if self.aec.styles.slab_structural_form_open {
        Some(
            crate::modules::aec::ui::style_manager::forms::slab_structural::SlabStructuralStyleFormState {
                open: true,
                is_new: self.aec.styles.slab_structural_editing_id.is_none(),
                name: &self.aec.styles.slab_structural_name,
                parent_id: self.aec.styles.slab_structural_parent.as_deref(),
                parent_name,
                layers: &self.aec.styles.slab_structural_layers,
                drag_index: self.aec.styles.slab_structural_drag_index,
                all_structural_styles,
                all_materials,
                all_layer_names,
                effective_layers,
                preview_mode: self.aec.styles.slab_structural_preview_mode,
                selected_layer: self.aec.styles.slab_structural_selected_layer,
            },
        )
    } else {
        None
    };

    Some(crate::modules::aec::ui::style_manager::forms::slab_structural::parts(
        library,
        self.aec.aec_project_explorer_file.as_ref(),
        self.aec.aec_session_style_library.as_ref(),
        &self.aec.styles.slab_structural_filter,
        self.aec.styles.slab_structural_selected.as_deref(),
        form,
    ))
}

pub(super) fn aec_floor_finish_parts(&self) -> Option<crate::modules::aec::ui::style_manager::StyleParts<'_>> {
    let library = self.aec.aec_style_library.as_ref()?;

    let parent_name = self
        .aec
        .styles.floor_finish_parent
        .as_ref()
        .and_then(|id| {
            library
                .floor_finish_styles
                .iter()
                .find(|s| s.style.id == *id)
                .map(|s| s.style.name.clone())
        });
    let all_floor_finish_styles: Vec<(&str, &str)> = library
        .floor_finish_styles
        .iter()
        .map(|s| (s.style.id.as_str(), s.style.name.as_str()))
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

    let mut effective_layers = Vec::new();
    for buf in &self.aec.styles.floor_finish_layers {
        if let Ok(layer) = crate::modules::aec::styles::slab_style_manager::buffer_to_layer(buf) {
            effective_layers.push(layer);
        }
    }
    if effective_layers.is_empty() {
        if let Some(id) = self.aec.styles.floor_finish_editing_id.as_deref() {
            if let Some(style) = library.find_floor_finish_style(id) {
                effective_layers = style.layers.clone();
            }
        }
    }

    let form = if self.aec.styles.floor_finish_form_open {
        Some(crate::modules::aec::ui::style_manager::forms::floor_finish::FloorFinishStyleFormState {
            open: true,
            is_new: self.aec.styles.floor_finish_editing_id.is_none(),
            name: &self.aec.styles.floor_finish_name,
            parent_id: self.aec.styles.floor_finish_parent.as_deref(),
            parent_name,
            layers: &self.aec.styles.floor_finish_layers,
            drag_index: self.aec.styles.floor_finish_drag_index,
            all_floor_finish_styles,
            all_materials,
            all_layer_names,
            effective_layers,
            preview_mode: self.aec.styles.floor_finish_preview_mode,
            selected_layer: self.aec.styles.floor_finish_selected_layer,
        })
    } else {
        None
    };

    Some(crate::modules::aec::ui::style_manager::forms::floor_finish::parts(
        library,
        self.aec.aec_project_explorer_file.as_ref(),
        self.aec.aec_session_style_library.as_ref(),
        &self.aec.styles.floor_finish_filter,
        self.aec.styles.floor_finish_selected.as_deref(),
        form,
    ))
}

pub(super) fn aec_wall_parts(&self) -> Option<crate::modules::aec::ui::style_manager::StyleParts<'_>> {
    let library = self.aec.aec_style_library.as_ref()?;
    let form = self.aec_wall_style_form_state(library);
    Some(crate::modules::aec::ui::style_manager::forms::wall::parts(
        library,
        self.aec.aec_project_explorer_file.as_ref(),
        self.aec.aec_session_style_library.as_ref(),
        self.aec.styles.selected_wall_style.as_deref(),
        &self.aec.styles.filter,
        form,
    ))
}

pub(super) fn aec_plan_parts(&self) -> Option<crate::modules::aec::ui::style_manager::StyleParts<'_>> {
    use crate::modules::aec::ui::aec_plan_manager::PlanConfigFormState;
    use crate::modules::aec::ui::aec_ui_util::StyleEditorFormState;
    let Some(library) = self.aec.aec_plan_library.as_ref() else {
        return None;
    };
    let demolition_style = StyleEditorFormState {
        line_type: &self.aec.styles.plan_demolition_style_line_type,
        linetype_items: &self.aec.styles.material_linetype_items,
        linetype_combo: &self.aec.styles.material_linetype_combo,
        line_color: &self.aec.styles.plan_demolition_style_line_color,
        line_color_picker_open: self.aec.styles.plan_demolition_style_line_color_picker_open,
        hatch_pattern: &self.aec.styles.plan_demolition_style_hatch_pattern,
        hatch_picker_open: self.aec.styles.plan_demolition_style_hatch_picker_open,
        hatch_color: &self.aec.styles.plan_demolition_style_hatch_color,
        hatch_color_picker_open: self.aec.styles.plan_demolition_style_hatch_color_picker_open,
        fill_color: &self.aec.styles.plan_demolition_style_fill_color,
        fill_color_picker_open: self.aec.styles.plan_demolition_style_fill_color_picker_open,
    };
    let existing_style = StyleEditorFormState {
        line_type: &self.aec.styles.plan_existing_style_line_type,
        linetype_items: &self.aec.styles.material_linetype_items,
        linetype_combo: &self.aec.styles.material_linetype_combo,
        line_color: &self.aec.styles.plan_existing_style_line_color,
        line_color_picker_open: self.aec.styles.plan_existing_style_line_color_picker_open,
        hatch_pattern: &self.aec.styles.plan_existing_style_hatch_pattern,
        hatch_picker_open: self.aec.styles.plan_existing_style_hatch_picker_open,
        hatch_color: &self.aec.styles.plan_existing_style_hatch_color,
        hatch_color_picker_open: self.aec.styles.plan_existing_style_hatch_color_picker_open,
        fill_color: &self.aec.styles.plan_existing_style_fill_color,
        fill_color_picker_open: self.aec.styles.plan_existing_style_fill_color_picker_open,
    };
    let from_project = self
        .aec
        .aec_project_explorer_file
        .as_ref()
        .is_some_and(|p| !p.display_config_library.configs.is_empty());
    Some(crate::modules::aec::ui::aec_plan_manager::parts(
        library,
        from_project,
        self.aec.styles.plan_selected.as_deref(),
        &self.aec.styles.plan_filter,
        PlanConfigFormState {
            open: self.aec.styles.plan_form_open,
            is_new: self.aec.styles.plan_editing_name.is_none(),
            editing_name: self.aec.styles.plan_editing_name.as_deref(),
            name: &self.aec.styles.plan_name,
            discipline: &self.aec.styles.plan_discipline,
            scale: &self.aec.styles.plan_scale,
            planning_stage: self.aec.styles.plan_planning_stage,
            view_type: self.aec.styles.plan_view_type,
            opening_levels: self.aec.styles.plan_opening_levels,
            phase_filter_visible_existing: self.aec.styles.plan_phase_filter_visible_existing,
            phase_filter_visible_demolition: self.aec.styles.plan_phase_filter_visible_demolition,
            phase_filter_visible_new: self.aec.styles.plan_phase_filter_visible_new,
            demolition_style,
            existing_style,
            detail: self.aec.styles.plan_detail,
            default_representation: self.aec.styles.plan_default_representation,
            component_visibility: &self.aec.styles.plan_component_visibility,
            wall_styles: &self.aec.styles.plan_wall_styles,
            style_overrides: &self.aec.styles.plan_style_overlays,
            overlay_style_id: self.aec.styles.plan_overlay_style_id.as_deref(),
            overlay_layer_id: self.aec.styles.plan_overlay_layer_id,
            overlay_line_type: &self.aec.styles.plan_overlay_line_type,
            overlay_line_color: &self.aec.styles.plan_overlay_line_color,
            overlay_hatch_pattern: &self.aec.styles.plan_overlay_hatch_pattern,
            overlay_hatch_color: &self.aec.styles.plan_overlay_hatch_color,
            overlay_hatch_scale: &self.aec.styles.plan_overlay_hatch_scale,
            overlay_hatch_angle: &self.aec.styles.plan_overlay_hatch_angle,
            overlay_hatch_angle_relative: self.aec.styles.plan_overlay_hatch_angle_relative,
            overlay_fill_color: &self.aec.styles.plan_overlay_fill_color,
            overlay_linetype_items: &self.aec.styles.material_linetype_items,
            overlay_linetype_combo: &self.aec.styles.material_linetype_combo,
            overlay_line_color_picker_open: self.aec.styles.plan_overlay_line_color_picker_open,
            overlay_hatch_picker_open: self.aec.styles.plan_overlay_hatch_picker_open,
            overlay_hatch_color_picker_open: self.aec.styles.plan_overlay_hatch_color_picker_open,
            overlay_fill_color_picker_open: self.aec.styles.plan_overlay_fill_color_picker_open,
            contour_hatch_pattern: &self.aec.styles.plan_contour_hatch_pattern,
            contour_hatch_color: &self.aec.styles.plan_contour_hatch_color,
            contour_hatch_scale: &self.aec.styles.plan_contour_hatch_scale,
            contour_hatch_angle: &self.aec.styles.plan_contour_hatch_angle,
            contour_hatch_angle_relative: self.aec.styles.plan_contour_hatch_angle_relative,
            contour_hatch_picker_open: self.aec.styles.plan_contour_hatch_picker_open,
            contour_hatch_color_picker_open: self.aec.styles.plan_contour_hatch_color_picker_open,
        },
    ))
}
}
