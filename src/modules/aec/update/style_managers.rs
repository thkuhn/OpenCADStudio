//! AEC update handlers: Style/material/section-profile managers (wall, opening, slab, floor finish).

use iced::Task;

use crate::app::{AecModalKind, Message, OpenCADStudio};
use crate::modules::aec::message::AecMessage;

impl OpenCADStudio {
    pub(crate) fn update_aec_style_managers(&mut self, msg: AecMessage) -> Task<Message> {
        match msg {
            AecMessage::AecStylesOpen(node) => self.aec_styles_open(node),
            AecMessage::AecStylesSelectNode(node) => {
                // Re-clicking the active node must not re-seed the form
                // (would discard unsaved edits).
                let in_styles = matches!(
                    self.active_modal,
                    Some(crate::app::ModalKind::Aec(AecModalKind::Styles))
                );
                if in_styles && self.aec.styles.node == node {
                    Task::none()
                } else {
                    self.aec_styles_open(node)
                }
            }
            AecMessage::AecMaterialManagerOpen => {
                self.ribbon.close_dropdown();
                self.aec_refresh_combined_style_library();
                self.aec.styles.filter.clear();
                self.aec.styles.selected_material = None;
                self.aec.styles.selected_wall_style = None;
                self.aec.styles.material_editing_id = None;
                self.aec.styles.material_form_open = false;
                self.aec.styles.wall_style_editing_id = None;
                self.aec.styles.wall_duplicate_default_profile = None;
                self.aec.styles.wall_style_form_open = false;
                self.aec.styles.wall_style_layers.clear();
                self.refresh_aec_material_linetype_combo();
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                Task::none()
            }
            AecMessage::AecSectionProfileManagerOpen => self.aec_section_profile_manager_open(),
            AecMessage::AecSectionProfileFilter(value) => {
                self.aec.styles.section_profile_filter = value;
                Task::none()
            }
            AecMessage::AecSectionProfileSelect(id) => self.aec_section_profile_select(id),
            AecMessage::AecSectionProfileNew => self.aec_section_profile_new(),
            AecMessage::AecSectionProfileNameChanged(value) => {
                self.aec_section_profile_name_changed(value)
            }
            AecMessage::AecSectionProfileShapeChanged(value) => {
                self.aec_section_profile_shape_changed(value)
            }
            AecMessage::AecSectionProfileDimensionChanged(field, value) => {
                self.aec_section_profile_dimension_changed(&field, value);
                Task::none()
            }
            AecMessage::AecSectionProfilePointAdd => {
                self.aec_section_profile_point_add();
                Task::none()
            }
            AecMessage::AecSectionProfilePointRemove(index) => {
                self.aec_section_profile_point_remove(index);
                Task::none()
            }
            AecMessage::AecSectionProfilePointXChanged(index, value) => {
                self.aec_section_profile_point_changed(index, true, value);
                Task::none()
            }
            AecMessage::AecSectionProfilePointYChanged(index, value) => {
                self.aec_section_profile_point_changed(index, false, value);
                Task::none()
            }
            AecMessage::AecSectionProfileSave => self.aec_section_profile_save(),
            AecMessage::AecSectionProfileDuplicate => self.aec_section_profile_duplicate(),
            AecMessage::AecSectionProfileDelete => self.aec_section_profile_delete(),
            AecMessage::AecOpeningStyleProfileChanged(part, profile_id) => {
                if let Some(profile_id) = profile_id.filter(|id| !id.trim().is_empty()) {
                    self.aec.styles.opening_profile_ids.insert(part, profile_id);
                } else {
                    self.aec.styles.opening_profile_ids.remove(&part);
                }
                Task::none()
            }
            AecMessage::AecWallStyleManagerOpen => {
                self.ribbon.close_dropdown();
                self.aec_refresh_combined_style_library();
                self.aec.styles.filter.clear();
                self.aec.styles.selected_material = None;
                self.aec.styles.selected_wall_style = None;
                self.aec.styles.material_editing_id = None;
                self.aec.styles.material_form_open = false;
                self.aec.styles.wall_style_editing_id = None;
                self.aec.styles.wall_duplicate_default_profile = None;
                self.aec.styles.wall_style_form_open = false;
                self.aec.styles.wall_style_layers.clear();
                self.aec.styles.profile_selected = None;
                self.aec.styles.profile_contour_explicit = false;
                self.aec.styles.profile_contour_selection.clear();
                self.aec.styles.profile_solid_explicit = false;
                self.aec.styles.profile_solid_selection.clear();
                self.aec.styles.profile_hatch_angle.clear();
                self.aec.styles.profile_hatch_relative = false;
                // Load (or refresh) the DisplayConfig library so the
                // "Darstellungs-Profile" table has data even if the Plan
                // Manager was never opened this session.
                self.aec.aec_plan_library = Some(
                    crate::modules::aec::engine::project::resolve_display_config_library(
                        self.aec.aec_project_explorer_file.as_ref(),
                    ),
                );
                self.refresh_aec_material_linetype_combo();
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerOpen => self.aec_opening_style_manager_open(),
            AecMessage::AecOpeningStyleManagerFilter(value) => {
                self.aec.styles.opening_filter = value;
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerSelect(id) => {
                self.aec_opening_style_manager_select(id)
            }
            AecMessage::AecOpeningStyleManagerNew => self.aec_opening_style_manager_new(),
            AecMessage::AecOpeningStyleManagerNameChanged(value) => {
                self.aec.styles.opening_name = value;
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerParentChanged(value) => {
                self.aec.styles.opening_parent = value;
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerKindChanged(value) => {
                self.aec_opening_style_manager_kind_changed(value)
            }
            AecMessage::AecOpeningStyleManagerShapeChanged(value) => {
                self.aec_opening_style_manager_shape_changed(value)
            }
            AecMessage::AecOpeningStyleManagerWidthChanged(value) => {
                self.aec_opening_style_manager_width_changed(value)
            }
            AecMessage::AecOpeningStyleManagerHeightChanged(value) => {
                self.aec_opening_style_manager_height_changed(value)
            }
            AecMessage::AecOpeningStyleManagerSillChanged(value) => {
                self.aec.styles.opening_sill = value;
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerFrameChanged(value) => {
                self.aec.styles.opening_frame = value;
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerAngleChanged(value) => {
                self.aec.styles.opening_angle = value;
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerSpringChanged(value) => {
                self.aec.styles.opening_spring = value;
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerHingeChanged(value) => {
                self.aec.styles.opening_hinge =
                    crate::modules::aec::engine::opening_style::LeafOperation::from_str(&value);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerSwingChanged(value) => {
                self.aec.styles.opening_swing_side =
                    crate::modules::aec::engine::openings::SwingSide::from_str(&value);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerSetPreviewMode(mode) => {
                self.aec.styles.opening_preview_mode = mode;
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerSetIsometric(view) => {
                self.aec.styles.opening_preview_isometric = view;
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerPreviewView(zoom, pan_x, pan_y) => {
                self.aec.styles.opening_preview_zoom = zoom;
                self.aec.styles.opening_preview_pan_x = pan_x;
                self.aec.styles.opening_preview_pan_y = pan_y;
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerSetEditorTab(tab) => {
                self.aec_opening_style_manager_set_editor_tab(tab);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerComponentEnabledChanged(group, enabled) => {
                self.aec_opening_style_manager_component_enabled_changed(group, enabled);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerSlotSourceChanged(index, value) => {
                self.aec_opening_style_manager_slot_source_changed(index, value)
            }
            AecMessage::AecOpeningStyleManagerSlotGeneratorChanged(index, value) => {
                self.aec_opening_style_manager_slot_generator_changed(index, value)
            }
            AecMessage::AecOpeningStyleManagerSlotBlockNameChanged(index, value) => {
                self.aec_opening_style_manager_slot_block_name_changed(index, value)
            }
            AecMessage::AecOpeningStyleManagerSlotBlockPlacementChanged(index, value) => {
                self.aec_opening_style_manager_slot_block_placement_changed(index, value)
            }
            AecMessage::AecOpeningStyleManagerProfileSelect(value) => {
                self.aec_opening_style_manager_profile_select(value)
            }
            AecMessage::AecOpeningStyleManagerPreviewSlotSelect(slot) => {
                self.aec_opening_style_manager_preview_slot_select(slot)
            }
            AecMessage::AecOpeningStyleManagerProfileRemove(name) => {
                self.aec_opening_style_manager_profile_remove(name)
            }
            AecMessage::AecOpeningStyleManagerSlotVisible(index, visible) => {
                self.aec_opening_style_manager_slot_visible(index, visible)
            }
            AecMessage::AecOpeningStyleManagerDimChanged(key, value) => {
                self.aec_opening_style_manager_dim_changed(&key, value);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerCellKindChanged(index, value) => {
                self.aec_opening_style_manager_cell_kind(index, &value);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerCellHingeChanged(index, value) => {
                self.aec_opening_style_manager_cell_hinge(index, &value);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerCellSwingChanged(index, value) => {
                self.aec_opening_style_manager_cell_swing(index, &value);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerCellAngleChanged(index, value) => {
                self.aec_opening_style_manager_cell_angle(index, &value);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerCellSelect(index) => {
                self.aec_opening_style_manager_cell_select(index);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerCellSillChanged(index, value) => {
                self.aec_opening_style_manager_cell_sill(index, value);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerCellAdd => {
                self.aec_opening_style_manager_cell_add();
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerCellRemove(index) => {
                self.aec_opening_style_manager_cell_remove(index);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerDivisionAdd(axis) => {
                self.aec_opening_style_manager_division_add(&axis);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerDivisionRemove(axis, index) => {
                self.aec_opening_style_manager_division_remove(&axis, index);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerDivisionPreset(name) => {
                self.aec_opening_style_manager_division_preset(&name);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerCellSpan(action) => {
                self.aec_opening_style_manager_cell_span(&action);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerGridInsert(axis, position) => {
                self.aec_opening_style_manager_grid_insert(&axis, &position);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerGridDivider(axis, index, position) => {
                self.aec_opening_style_manager_grid_divider(&axis, index, &position);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerGridEdgeSelect(axis, index) => {
                self.aec_opening_style_manager_grid_edge_select(&axis, index);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerGridMeasure(axis, index, measure) => {
                self.aec_opening_style_manager_grid_measure(&axis, index, &measure);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerGridMeeting(axis, index, meeting) => {
                self.aec_opening_style_manager_grid_meeting(&axis, index, &meeting);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerGridEnter(index) => {
                self.aec_opening_style_manager_grid_enter(index);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerGridBack => {
                self.aec_opening_style_manager_grid_back();
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerFamilyBlockChanged(family, name) => {
                self.aec_opening_style_manager_family_block(&family, name);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerFamilyPlacementChanged(family, placement) => {
                self.aec_opening_style_manager_family_placement(&family, &placement);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerLookLineTypeChanged(role, value) => {
                self.aec_opening_style_manager_look_line_type_changed(role, value);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerLookLineColorChanged(role, value) => {
                self.aec_opening_style_manager_look_line_color_changed(role, value);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerLookColorPickerToggle(role) => {
                self.aec_opening_style_manager_look_color_picker_toggle(role);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerLookMaterialClear(role) => {
                self.aec_opening_style_manager_look_material_clear(role);
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerSave => self.aec_opening_style_manager_save(),
            AecMessage::AecOpeningStyleManagerSaveAndApply => {
                if let Some((id, _lib)) = self.aec_opening_style_manager_save_internal() {
                    let tab_index = self.active_tab;
                    let mut affected_walls = Vec::new();
                    for entity in self.tabs[tab_index].scene.document.entities() {
                        let h = entity.common().handle;
                        if let Some(opening) =
                            crate::modules::aec::engine::opening_xdata::opening_from_entity(entity, h)
                        {
                            if opening.style_id.as_deref() == Some(id.as_str()) {
                                let wall_handle = crate::modules::aec::engine::wall_package::resolve_wall_package(
                                    &self.tabs[tab_index].scene,
                                    opening.host_wall,
                                );
                                if !wall_handle.is_null() && !affected_walls.contains(&wall_handle) {
                                    affected_walls.push(wall_handle);
                                }
                            }
                        }
                    }
                    let mut updated_count = 0;
                    for wall_handle in affected_walls {
                        if self
                            .regenerate_wall_respecting_active_display_config(tab_index, wall_handle)
                            .is_ok()
                        {
                            updated_count += 1;
                        }
                    }
                    if updated_count > 0 {
                        self.tabs[tab_index].scene.bump_geometry();
                        self.command_line.push_info(
                            crate::tf!(
                                "AEC Style Manager: updated {updated_count} wall(s) / opening(s) using this style."
                            )
                            .as_ref(),
                        );
                    }
                }
                Task::none()
            }
            AecMessage::AecOpeningStyleManagerDelete => self.aec_opening_style_manager_delete(),
            AecMessage::AecOpeningStyleManagerDuplicate => self.aec_opening_style_manager_duplicate(),
            AecMessage::AecStyleManagerWallStyleDuplicate => self.aec_wall_style_manager_duplicate(),
            AecMessage::AecStyleManagerCopyOpeningStyleToProject => {
                self.aec_handle_copy_opening_style(true)
            }
            AecMessage::AecStyleManagerCopyOpeningStyleToGlobal => {
                self.aec_handle_copy_opening_style(false)
            }
            AecMessage::AecSlabStyleManagerOpen => self.aec_slab_style_manager_open(),
            AecMessage::AecSlabStyleManagerFilter(value) => {
                self.aec.styles.slab_filter = value;
                Task::none()
            }
            AecMessage::AecSlabStyleManagerSelect(id) => self.aec_slab_style_manager_select(id),
            AecMessage::AecSlabStyleManagerNew => self.aec_slab_style_manager_new(),
            AecMessage::AecSlabStyleManagerDuplicate => self.aec_slab_style_manager_duplicate(),
            AecMessage::AecSlabStyleManagerNameChanged(value) => {
                self.aec.styles.slab_name = value;
                Task::none()
            }
            AecMessage::AecSlabStyleManagerParentChanged(value) => {
                self.aec.styles.slab_parent = value;
                Task::none()
            }
            AecMessage::AecSlabStyleManagerStructuralStyleChanged(value) => {
                self.aec_slab_style_manager_structural_style_changed(value);
                Task::none()
            }
            AecMessage::AecSlabStyleManagerFinishStyleChanged(value) => {
                self.aec_slab_style_manager_finish_style_changed(value);
                Task::none()
            }
            AecMessage::AecSlabStyleManagerLoadModularLayers => {
                self.aec_slab_style_manager_load_modular_layers();
                Task::none()
            }
            AecMessage::AecSlabStyleManagerLayerAdd => {
                self.aec_slab_style_manager_layer_add();
                Task::none()
            }
            AecMessage::AecSlabStyleManagerLayerRemove(index) => {
                self.aec_slab_style_manager_layer_remove(index);
                Task::none()
            }
            AecMessage::AecSlabStyleManagerLayerMoveUp(index) => {
                self.aec_slab_style_manager_layer_move_up(index);
                Task::none()
            }
            AecMessage::AecSlabStyleManagerLayerMoveDown(index) => {
                self.aec_slab_style_manager_layer_move_down(index);
                Task::none()
            }
            AecMessage::AecSlabStyleManagerLayerDragStart(index) => {
                self.aec.styles.slab_drag_index = Some(index);
                Task::none()
            }
            AecMessage::AecSlabStyleManagerLayerDragOver(index) => {
                if let Some(from) = self.aec.styles.slab_drag_index {
                    if from != index && index < self.aec.styles.slab_layers.len() {
                        let item = self.aec.styles.slab_layers.remove(from);
                        self.aec.styles.slab_layers.insert(index, item);
                        self.aec.styles.slab_drag_index = Some(index);
                    }
                }
                Task::none()
            }
            AecMessage::AecSlabStyleManagerLayerDragEnd => {
                self.aec.styles.slab_drag_index = None;
                Task::none()
            }
            AecMessage::AecSlabStyleManagerLayerMaterialChanged(index, value) => {
                if let Some(layer) = self.aec.styles.slab_layers.get_mut(index) {
                    layer.material_id = value;
                }
                Task::none()
            }
            AecMessage::AecSlabStyleManagerLayerThicknessChanged(index, value) => {
                if let Some(layer) = self.aec.styles.slab_layers.get_mut(index) {
                    layer.thickness = value;
                }
                Task::none()
            }
            AecMessage::AecSlabStyleManagerLayerFunctionChanged(index, value) => {
                if let Some(layer) = self.aec.styles.slab_layers.get_mut(index) {
                    layer.function = value;
                }
                Task::none()
            }
            AecMessage::AecSlabStyleManagerLayerVerticalOffsetChanged(index, value) => {
                if let Some(layer) = self.aec.styles.slab_layers.get_mut(index) {
                    layer.vertical_offset = value;
                }
                Task::none()
            }
            AecMessage::AecSlabStyleManagerLayerOverrideChanged(index, value) => {
                if let Some(layer) = self.aec.styles.slab_layers.get_mut(index) {
                    layer.layer_override = value;
                }
                Task::none()
            }
            AecMessage::AecSlabStyleManagerLayerHatchOverrideChanged(index, value) => {
                if let Some(layer) = self.aec.styles.slab_layers.get_mut(index) {
                    layer.hatch_override = value;
                }
                Task::none()
            }
            AecMessage::AecSlabStyleManagerLayerRoleTagChanged(index, value) => {
                if let Some(layer) = self.aec.styles.slab_layers.get_mut(index) {
                    layer.role_tag = value;
                }
                Task::none()
            }
            AecMessage::AecWallStyleManagerSetPreviewMode(mode) => {
                self.aec.styles.wall_preview_mode = mode;
                Task::none()
            }
            AecMessage::AecWallStyleManagerPreviewLayerSelect(index) => {
                self.aec.styles.wall_selected_layer = Some(index);
                iced::widget::operation::focus(iced::widget::Id::from(
                    crate::modules::aec::ui::style_manager::forms::wall::layer_thickness_input_id(index),
                ))
            }
            AecMessage::AecSlabStyleManagerPreviewLayerSelect(index) => {
                self.aec.styles.slab_selected_layer = Some(index);
                Task::none()
            }
            AecMessage::AecSlabStructuralStyleManagerPreviewLayerSelect(index) => {
                self.aec.styles.slab_structural_selected_layer = Some(index);
                Task::none()
            }
            AecMessage::AecFloorFinishStyleManagerPreviewLayerSelect(index) => {
                self.aec.styles.floor_finish_selected_layer = Some(index);
                Task::none()
            }
            AecMessage::AecSlabStyleManagerSetPreviewMode(mode) => {
                self.aec.styles.slab_preview_mode = mode;
                Task::none()
            }
            AecMessage::AecSlabStyleManagerProfileSelect(name) => {
                self.aec_slab_style_manager_profile_select(name);
                Task::none()
            }
            AecMessage::AecSlabStyleManagerProfileRemove(name) => {
                self.aec_slab_style_manager_profile_remove(name);
                Task::none()
            }
            AecMessage::AecSlabStyleManagerProfileSlotVisibilityToggle(slot, visible) => {
                self.aec_slab_style_manager_profile_slot_visibility(slot, visible);
                Task::none()
            }
            AecMessage::AecSlabStyleManagerSave => self.aec_slab_style_manager_save(),
            AecMessage::AecSlabStyleManagerSaveAndApply => {
                self.aec_slab_style_manager_save_and_apply()
            }
            AecMessage::AecSlabStyleManagerDelete => self.aec_slab_style_manager_delete(),
            AecMessage::AecStyleManagerCopySlabStyleToProject => {
                self.aec_handle_copy_slab_style(true)
            }
            AecMessage::AecStyleManagerCopySlabStyleToGlobal => {
                self.aec_handle_copy_slab_style(false)
            }
            AecMessage::AecSlabStructuralStyleManagerOpen => {
                self.aec_slab_structural_style_manager_open()
            }
            AecMessage::AecSlabStructuralStyleManagerFilter(value) => {
                self.aec.styles.slab_structural_filter = value;
                Task::none()
            }
            AecMessage::AecSlabStructuralStyleManagerSelect(id) => {
                self.aec_slab_structural_style_manager_select(id)
            }
            AecMessage::AecSlabStructuralStyleManagerNew => {
                self.aec_slab_structural_style_manager_new()
            }
            AecMessage::AecSlabStructuralStyleManagerDuplicate => {
                self.aec_slab_structural_style_manager_duplicate()
            }
            AecMessage::AecSlabStructuralStyleManagerNameChanged(value) => {
                self.aec.styles.slab_structural_name = value;
                Task::none()
            }
            AecMessage::AecSlabStructuralStyleManagerParentChanged(value) => {
                self.aec.styles.slab_structural_parent = value;
                Task::none()
            }
            AecMessage::AecSlabStructuralStyleManagerLayerAdd => {
                self.aec_slab_structural_style_manager_layer_add();
                Task::none()
            }
            AecMessage::AecSlabStructuralStyleManagerLayerRemove(index) => {
                self.aec_slab_structural_style_manager_layer_remove(index);
                Task::none()
            }
            AecMessage::AecSlabStructuralStyleManagerLayerMoveUp(index) => {
                self.aec_slab_structural_style_manager_layer_move_up(index);
                Task::none()
            }
            AecMessage::AecSlabStructuralStyleManagerLayerMoveDown(index) => {
                self.aec_slab_structural_style_manager_layer_move_down(index);
                Task::none()
            }
            AecMessage::AecSlabStructuralStyleManagerLayerDragStart(index) => {
                self.aec.styles.slab_structural_drag_index = Some(index);
                Task::none()
            }
            AecMessage::AecSlabStructuralStyleManagerLayerDragOver(index) => {
                if let Some(from) = self.aec.styles.slab_structural_drag_index {
                    if from != index
                        && index < self.aec.styles.slab_structural_layers.len()
                    {
                        let item = self
                            .aec
                            .styles.slab_structural_layers
                            .remove(from);
                        self.aec
                            .styles.slab_structural_layers
                            .insert(index, item);
                        self.aec.styles.slab_structural_drag_index = Some(index);
                    }
                }
                Task::none()
            }
            AecMessage::AecSlabStructuralStyleManagerLayerDragEnd => {
                self.aec.styles.slab_structural_drag_index = None;
                Task::none()
            }
            AecMessage::AecSlabStructuralStyleManagerLayerMaterialChanged(index, value) => {
                if let Some(layer) = self
                    .aec
                    .styles.slab_structural_layers
                    .get_mut(index)
                {
                    layer.material_id = value;
                }
                Task::none()
            }
            AecMessage::AecSlabStructuralStyleManagerLayerThicknessChanged(index, value) => {
                if let Some(layer) = self
                    .aec
                    .styles.slab_structural_layers
                    .get_mut(index)
                {
                    layer.thickness = value;
                }
                Task::none()
            }
            AecMessage::AecSlabStructuralStyleManagerLayerFunctionChanged(index, value) => {
                if let Some(layer) = self
                    .aec
                    .styles.slab_structural_layers
                    .get_mut(index)
                {
                    layer.function = value;
                }
                Task::none()
            }
            AecMessage::AecSlabStructuralStyleManagerLayerVerticalOffsetChanged(index, value) => {
                if let Some(layer) = self
                    .aec
                    .styles.slab_structural_layers
                    .get_mut(index)
                {
                    layer.vertical_offset = value;
                }
                Task::none()
            }
            AecMessage::AecSlabStructuralStyleManagerLayerOverrideChanged(index, value) => {
                if let Some(layer) = self
                    .aec
                    .styles.slab_structural_layers
                    .get_mut(index)
                {
                    layer.layer_override = value;
                }
                Task::none()
            }
            AecMessage::AecSlabStructuralStyleManagerLayerHatchOverrideChanged(index, value) => {
                if let Some(layer) = self
                    .aec
                    .styles.slab_structural_layers
                    .get_mut(index)
                {
                    layer.hatch_override = value;
                }
                Task::none()
            }
            AecMessage::AecSlabStructuralStyleManagerLayerRoleTagChanged(index, value) => {
                if let Some(layer) = self
                    .aec
                    .styles.slab_structural_layers
                    .get_mut(index)
                {
                    layer.role_tag = value;
                }
                Task::none()
            }
            AecMessage::AecSlabStructuralStyleManagerSetPreviewMode(mode) => {
                self.aec.styles.slab_structural_preview_mode = mode;
                Task::none()
            }
            AecMessage::AecSlabStructuralStyleManagerSave => {
                self.aec_slab_structural_style_manager_save()
            }
            AecMessage::AecSlabStructuralStyleManagerSaveAndApply => {
                self.aec_slab_structural_style_manager_save_and_apply()
            }
            AecMessage::AecSlabStructuralStyleManagerDelete => {
                self.aec_slab_structural_style_manager_delete()
            }
            AecMessage::AecStyleManagerCopySlabStructuralStyleToProject => {
                self.aec_handle_copy_slab_structural_style(true)
            }
            AecMessage::AecStyleManagerCopySlabStructuralStyleToGlobal => {
                self.aec_handle_copy_slab_structural_style(false)
            }
            AecMessage::AecFloorFinishStyleManagerOpen => {
                self.aec_floor_finish_style_manager_open()
            }
            AecMessage::AecFloorFinishStyleManagerFilter(value) => {
                self.aec.styles.floor_finish_filter = value;
                Task::none()
            }
            AecMessage::AecFloorFinishStyleManagerSelect(id) => {
                self.aec_floor_finish_style_manager_select(id)
            }
            AecMessage::AecFloorFinishStyleManagerNew => {
                self.aec_floor_finish_style_manager_new()
            }
            AecMessage::AecFloorFinishStyleManagerDuplicate => {
                self.aec_floor_finish_style_manager_duplicate()
            }
            AecMessage::AecFloorFinishStyleManagerNameChanged(value) => {
                self.aec.styles.floor_finish_name = value;
                Task::none()
            }
            AecMessage::AecFloorFinishStyleManagerParentChanged(value) => {
                self.aec.styles.floor_finish_parent = value;
                Task::none()
            }
            AecMessage::AecFloorFinishStyleManagerLayerAdd => {
                self.aec_floor_finish_style_manager_layer_add();
                Task::none()
            }
            AecMessage::AecFloorFinishStyleManagerLayerRemove(index) => {
                self.aec_floor_finish_style_manager_layer_remove(index);
                Task::none()
            }
            AecMessage::AecFloorFinishStyleManagerLayerMoveUp(index) => {
                self.aec_floor_finish_style_manager_layer_move_up(index);
                Task::none()
            }
            AecMessage::AecFloorFinishStyleManagerLayerMoveDown(index) => {
                self.aec_floor_finish_style_manager_layer_move_down(index);
                Task::none()
            }
            AecMessage::AecFloorFinishStyleManagerLayerDragStart(index) => {
                self.aec.styles.floor_finish_drag_index = Some(index);
                Task::none()
            }
            AecMessage::AecFloorFinishStyleManagerLayerDragOver(index) => {
                if let Some(from) = self.aec.styles.floor_finish_drag_index {
                    if from != index
                        && index < self.aec.styles.floor_finish_layers.len()
                    {
                        let item = self
                            .aec
                            .styles.floor_finish_layers
                            .remove(from);
                        self.aec
                            .styles.floor_finish_layers
                            .insert(index, item);
                        self.aec.styles.floor_finish_drag_index = Some(index);
                    }
                }
                Task::none()
            }
            AecMessage::AecFloorFinishStyleManagerLayerDragEnd => {
                self.aec.styles.floor_finish_drag_index = None;
                Task::none()
            }
            AecMessage::AecFloorFinishStyleManagerLayerMaterialChanged(index, value) => {
                if let Some(layer) = self
                    .aec
                    .styles.floor_finish_layers
                    .get_mut(index)
                {
                    layer.material_id = value;
                }
                Task::none()
            }
            AecMessage::AecFloorFinishStyleManagerLayerThicknessChanged(index, value) => {
                if let Some(layer) = self
                    .aec
                    .styles.floor_finish_layers
                    .get_mut(index)
                {
                    layer.thickness = value;
                }
                Task::none()
            }
            AecMessage::AecFloorFinishStyleManagerLayerFunctionChanged(index, value) => {
                if let Some(layer) = self
                    .aec
                    .styles.floor_finish_layers
                    .get_mut(index)
                {
                    layer.function = value;
                }
                Task::none()
            }
            AecMessage::AecFloorFinishStyleManagerLayerVerticalOffsetChanged(index, value) => {
                if let Some(layer) = self
                    .aec
                    .styles.floor_finish_layers
                    .get_mut(index)
                {
                    layer.vertical_offset = value;
                }
                Task::none()
            }
            AecMessage::AecFloorFinishStyleManagerLayerOverrideChanged(index, value) => {
                if let Some(layer) = self
                    .aec
                    .styles.floor_finish_layers
                    .get_mut(index)
                {
                    layer.layer_override = value;
                }
                Task::none()
            }
            AecMessage::AecFloorFinishStyleManagerLayerHatchOverrideChanged(index, value) => {
                if let Some(layer) = self
                    .aec
                    .styles.floor_finish_layers
                    .get_mut(index)
                {
                    layer.hatch_override = value;
                }
                Task::none()
            }
            AecMessage::AecFloorFinishStyleManagerLayerRoleTagChanged(index, value) => {
                if let Some(layer) = self
                    .aec
                    .styles.floor_finish_layers
                    .get_mut(index)
                {
                    layer.role_tag = value;
                }
                Task::none()
            }
            AecMessage::AecFloorFinishStyleManagerSetPreviewMode(mode) => {
                self.aec.styles.floor_finish_preview_mode = mode;
                Task::none()
            }
            AecMessage::AecFloorFinishStyleManagerSave => {
                self.aec_floor_finish_style_manager_save()
            }
            AecMessage::AecFloorFinishStyleManagerSaveAndApply => {
                self.aec_floor_finish_style_manager_save_and_apply()
            }
            AecMessage::AecFloorFinishStyleManagerDelete => {
                self.aec_floor_finish_style_manager_delete()
            }
            AecMessage::AecStyleManagerCopyFloorFinishStyleToProject => {
                self.aec_handle_copy_floor_finish_style(true)
            }
            AecMessage::AecStyleManagerCopyFloorFinishStyleToGlobal => {
                self.aec_handle_copy_floor_finish_style(false)
            }
            _ => Task::none(),
        }
    }
}
