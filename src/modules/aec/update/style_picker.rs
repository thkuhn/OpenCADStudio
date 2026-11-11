//! AEC update handlers: Style picker and style assignment flows.

use iced::Task;

use crate::app::{AecModalKind, Message, OpenCADStudio};
use crate::modules::aec::message::AecMessage;

impl OpenCADStudio {
    pub(crate) fn update_aec_style_picker(&mut self, msg: AecMessage) -> Task<Message> {
        match msg {
            AecMessage::AecStyleManagerFilter(value) => {
                self.aec.styles.filter = value;
                Task::none()
            }
            AecMessage::AecStyleManagerSelectMaterial(id) => {
                let material = crate::modules::aec::engine::library::combined_material_entries(
                    self.aec.aec_project_explorer_file.as_ref(),
                )
                .into_iter()
                .find(|e| e.material.id == id)
                .map(|e| e.material)
                .or_else(|| {
                    self.aec.aec_style_library
                        .as_ref()
                        .and_then(|lib| lib.materials.iter().find(|m| m.id == id).cloned())
                });
                if let Some(material) = material {
                    self.aec.styles.material_editing_id = Some(material.id.clone());
                    self.aec.styles.material_name = material.name.clone();
                    self.aec.styles.material_hatch = material.hatch_pattern.clone();
                    self.aec.styles.material_color =
                        format!("#{:06X}", material.line_color);
                    self.aec.styles.material_line_type = material.line_type.clone();
                    self.aec.styles.material_category =
                        material.group.clone().unwrap_or_default();
                                        self.aec.styles.material_hatch_color = material
                        .hatch_color
                        .and_then(|c| c.rgb())
                        .map(|(r, g, b)| ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
                        .unwrap_or(material.line_color);
                    self.aec.styles.material_hatch_scale =
                        format!("{}", material.hatch_scale);
                    self.aec.styles.material_render_ref =
                        material.render_material_ref.clone().unwrap_or_default();
                    self.aec.styles.material_hatch_angle =
                        format!("{}", material.hatch_angle);
                    self.aec.styles.material_hatch_angle_relative =
                        material.hatch_angle_relative;
                    self.aec.styles.material_hatch_color_picker_open = false;
                    self.aec.styles.material_form_open = true;
                }
                self.aec.styles.selected_material = Some(id);
                self.aec.styles.selected_wall_style = None;
                self.refresh_aec_material_linetype_combo();
                Task::none()
            }
            AecMessage::AecStyleManagerSelectWallStyle(id) => {
                self.refresh_aec_material_linetype_combo();
                let wall_style = crate::modules::aec::engine::library::combined_wall_style_entries(
                    self.aec.aec_project_explorer_file.as_ref(),
                )
                .into_iter()
                .find(|e| e.wall_style.style.id == id)
                .map(|e| e.wall_style)
                .or_else(|| {
                    self.aec.aec_style_library.as_ref().and_then(|lib| {
                        lib.wall_styles
                            .iter()
                            .find(|w| w.style.id == id)
                            .cloned()
                    })
                });
                if let Some(wall_style) = wall_style {
                    self.aec.styles.wall_style_editing_id = Some(wall_style.style.id.clone());
                    self.aec.styles.wall_duplicate_default_profile = None;
                    self.aec.styles.wall_style_name = wall_style.style.name.clone();
                    self.aec.styles.wall_style_parent =
                        wall_style.style.parent.clone();
                    self.aec.styles.wall_style_layers = wall_style
                        .layers
                        .iter()
                        .map(|l| crate::app::AecLayerBuffer {
                            material_id: l.material_id.clone(),
                            // Display in centimeters; fixed numbers and
                            // formula strings alike.
                            thickness: l.thickness.to_cm_display_string(),
                            function: match &l.function {
                                crate::modules::aec::engine::wall_style::LayerFunction::Structural => {
                                    "Structural".to_string()
                                }
                                crate::modules::aec::engine::wall_style::LayerFunction::Insulation => {
                                    "Insulation".to_string()
                                }
                                crate::modules::aec::engine::wall_style::LayerFunction::Finish => {
                                    "Finish".to_string()
                                }
                                crate::modules::aec::engine::wall_style::LayerFunction::Other(s) => s.clone(),
                            },
                            axis_offset: l.axis_offset.to_cm_display_string(),
                            bottom_offset: crate::modules::aec::engine::wall_style::LayerValue::format_cm(
                                l.bottom_offset * 100.0,
                            ),
                            top_offset: crate::modules::aec::engine::wall_style::LayerValue::format_cm(
                                l.top_offset * 100.0,
                            ),
                            layer_override: l.layer_override.clone().unwrap_or_default(),
                            hatch_override: l.hatch_override.clone().unwrap_or_default(),
                            role_tag: l.role_tag.clone().unwrap_or_default(),
                            layer_id: Some(l.layer_id),
                        })
                        .collect();
                    self.aec.styles.wall_style_form_open = true;
                }
                self.aec.styles.selected_wall_style = Some(id);
                self.aec.styles.selected_material = None;
                self.aec.styles.material_form_open = false;
                self.aec.styles.material_editing_id = None;
                self.aec.styles.profile_selected = None;
                self.aec.styles.profile_contour_explicit = false;
                self.aec.styles.profile_contour_selection.clear();
                self.aec.styles.profile_solid_explicit = false;
                self.aec.styles.profile_solid_selection.clear();
                self.aec.styles.profile_hatch_angle.clear();
                self.aec.styles.profile_hatch_relative = false;
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleNew => {
                self.aec.styles.selected_material = None;
                self.aec.styles.selected_wall_style = None;
                self.aec.styles.wall_style_editing_id = None;
                self.aec.styles.wall_duplicate_default_profile = None;
                self.aec.styles.wall_style_name.clear();
                self.aec.styles.wall_style_parent = None;
                self.aec.styles.wall_style_layers.clear();
                self.aec.styles.wall_style_form_open = true;
                self.aec.styles.material_form_open = false;
                self.aec.styles.material_editing_id = None;
                self.aec.styles.profile_selected = None;
                self.aec.styles.profile_contour_explicit = false;
                self.aec.styles.profile_contour_selection.clear();
                self.aec.styles.profile_solid_explicit = false;
                self.aec.styles.profile_solid_selection.clear();
                self.aec.styles.profile_hatch_angle.clear();
                self.aec.styles.profile_hatch_relative = false;
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleNameChanged(value) => {
                self.aec.styles.wall_style_name = value;
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleParentChanged(value) => {
                self.aec.styles.wall_style_parent = value;
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerAdd => {
                let material_id = self
                    .aec.aec_style_library
                    .as_ref()
                    .and_then(|lib| lib.materials.first())
                    .map(|m| m.id.clone())
                    .unwrap_or_default();
                self.aec.styles.wall_style_layers
                    .push(crate::app::AecLayerBuffer {
                        material_id,
                        thickness: "10".to_string(),
                        function: "Structural".to_string(),
                        axis_offset: "0".to_string(),
                        bottom_offset: "0".to_string(),
                        top_offset: "0".to_string(),
                        layer_override: String::new(),
                        hatch_override: String::new(),
                        role_tag: String::new(),
                        layer_id: Some(uuid::Uuid::new_v4()),
                    });
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerRemove(index) => {
                if index < self.aec.styles.wall_style_layers.len() {
                    self.aec.styles.wall_style_layers.remove(index);
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerMaterialChanged(index, material_id) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.material_id = material_id;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerThicknessChanged(index, thickness) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.thickness = thickness;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerFunctionChanged(index, function) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.function = function;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerAxisOffsetChanged(index, offset) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.axis_offset = offset;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerBottomOffsetChanged(index, offset) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.bottom_offset = offset;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerTopOffsetChanged(index, offset) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.top_offset = offset;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerOverrideChanged(index, layer_name) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.layer_override = layer_name;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerHatchOverrideChanged(index, hatch) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.hatch_override = hatch;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerHatchPickerToggle(index) => {
                let open = &mut self.aec.styles.wall_layer_hatch_picker;
                *open = if *open == Some(index) { None } else { Some(index) };
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerHatchSelected(index, hatch) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.hatch_override = hatch;
                }
                self.aec.styles.wall_layer_hatch_picker = None;
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerRoleTagChanged(index, role_tag) => {
                if let Some(layer) = self.aec.styles.wall_style_layers.get_mut(index) {
                    layer.role_tag = role_tag;
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerMoveUp(index) => {
                if index > 0 && index < self.aec.styles.wall_style_layers.len() {
                    self.aec.styles.wall_style_layers.swap(index - 1, index);
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerMoveDown(index) => {
                if index + 1 < self.aec.styles.wall_style_layers.len() {
                    self.aec.styles.wall_style_layers.swap(index, index + 1);
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerDragStart(index) => {
                // Click-based "pick up / drop here" reordering: arms the
                // dragged row so a later drag-over on another row swaps it
                // into place. Clicking the already-armed row's handle again
                // cancels the drag (matches the toggle feel of `PaneMoveStart`).
                if self.aec.styles.wall_style_drag_index == Some(index) {
                    self.aec.styles.wall_style_drag_index = None;
                } else if index < self.aec.styles.wall_style_layers.len() {
                    self.aec.styles.wall_style_drag_index = Some(index);
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerDragOver(target) => {
                if let Some(from) = self.aec.styles.wall_style_drag_index.take() {
                    let len = self.aec.styles.wall_style_layers.len();
                    if from != target && from < len && target < len {
                        let layer = self.aec.styles.wall_style_layers.remove(from);
                        self.aec.styles.wall_style_layers.insert(target, layer);
                    }
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleLayerDragEnd => {
                self.aec.styles.wall_style_drag_index = None;
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleSortToggle => {
                self.aec.styles.wall_style_sort =
                    self.aec.styles.wall_style_sort.toggled();
                Task::none()
            }
            AecMessage::AecStylePickerOpen(target) => {
                self.aec_refresh_combined_style_library();
                self.aec.aec_style_picker_filter.clear();
                // Pre-select/highlight whatever is already assigned for this
                // target so the picker doesn't reopen with nothing
                // highlighted even though a value is already in use.
                self.aec.aec_style_picker_selection = match target {
                    crate::app::StylePickerTarget::WallStyleParent => {
                        self.aec.styles.wall_style_parent.clone()
                    }
                    crate::app::StylePickerTarget::LayerMaterial(index) => self
                        .aec.styles.wall_style_layers
                        .get(index)
                        .map(|l| l.material_id.clone()),
                    crate::app::StylePickerTarget::LayerOverride(index) => self
                        .aec.styles.wall_style_layers
                        .get(index)
                        .map(|l| l.layer_override.clone()),
                    crate::app::StylePickerTarget::WallPropertiesStyle
                    | crate::app::StylePickerTarget::ActiveCommand
                    | crate::app::StylePickerTarget::OpeningPropertiesStyle
                    | crate::app::StylePickerTarget::SlabPropertiesStyle => None,
                    crate::app::StylePickerTarget::OpeningStyleParent => {
                        self.aec.styles.opening_parent.clone()
                    }
                    crate::app::StylePickerTarget::OpeningComponentMaterial(role) => self
                        .aec
                        .styles
                        .opening_look_buffers
                        .get(&role)
                        .map(|buffer| buffer.material_id.clone()),
                    crate::app::StylePickerTarget::SlabStyleParent => {
                        self.aec.styles.slab_parent.clone()
                    }
                    crate::app::StylePickerTarget::SlabStructuralStyleParent => {
                        self.aec.styles.slab_structural_parent.clone()
                    }
                    crate::app::StylePickerTarget::SlabLayerMaterial(index) => self
                        .aec
                        .styles.slab_layers
                        .get(index)
                        .map(|l| l.material_id.clone()),
                    crate::app::StylePickerTarget::SlabStructuralLayerMaterial(index) => self
                        .aec
                        .styles.slab_structural_layers
                        .get(index)
                        .map(|l| l.material_id.clone()),
                    crate::app::StylePickerTarget::SlabLayerOverride(index) => self
                        .aec
                        .styles.slab_layers
                        .get(index)
                        .map(|l| l.layer_override.clone()),
                    crate::app::StylePickerTarget::SlabStructuralLayerOverride(index) => self
                        .aec
                        .styles.slab_structural_layers
                        .get(index)
                        .map(|l| l.layer_override.clone()),
                    crate::app::StylePickerTarget::FloorFinishLayerOverride(index) => self
                        .aec
                        .styles.floor_finish_layers
                        .get(index)
                        .map(|l| l.layer_override.clone()),
                    crate::app::StylePickerTarget::FloorFinishStyleParent => {
                        self.aec.styles.floor_finish_parent.clone()
                    }
                    crate::app::StylePickerTarget::FloorFinishLayerMaterial(index) => self
                        .aec
                        .styles.floor_finish_layers
                        .get(index)
                        .map(|l| l.material_id.clone()),
                };
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker { target }));
                Task::none()
            }
            AecMessage::AecStylePickerOpenForWallProperties(handles) => {
                self.aec_refresh_combined_style_library();
                self.aec.aec_style_picker_filter.clear();
                // Pre-select/highlight the style already assigned to the
                // (first) wall being edited, so the picker doesn't reopen
                // with nothing highlighted even though a style is in use.
                self.aec.aec_style_picker_selection = handles.first().and_then(|h| {
                    self.tabs[self.active_tab]
                        .scene
                        .document
                        .get_entity(*h)
                        .and_then(crate::modules::aec::engine::xdata::wall_from_entity)
                        .map(|v2| v2.style_id)
                });
                self.aec.aec_style_picker_wall_handles = handles;
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker {
                    target: crate::app::StylePickerTarget::WallPropertiesStyle,
                }));
                Task::none()
            }
            AecMessage::AecStylePickerOpenForOpeningProperties(handles) => {
                self.aec_refresh_combined_style_library();
                self.aec.aec_style_picker_filter.clear();
                self.aec.aec_style_picker_selection = handles.first().and_then(|h| {
                    let owner = crate::modules::aec::engine::opening_display::resolve_opening_package(
                        &self.tabs[self.active_tab].scene,
                        *h,
                    );
                    self.tabs[self.active_tab]
                        .scene
                        .document
                        .get_entity(owner)
                        .and_then(|e| {
                            crate::modules::aec::engine::opening_xdata::opening_from_entity(e, owner)
                        })
                        .and_then(|o| o.style_id)
                });
                self.aec.aec_style_picker_wall_handles = handles;
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker {
                    target: crate::app::StylePickerTarget::OpeningPropertiesStyle,
                }));
                Task::none()
            }
            AecMessage::AecStylePickerOpenForSlabProperties(handles) => {
                self.aec_refresh_combined_style_library();
                self.aec.aec_style_picker_filter.clear();
                self.aec.aec_style_picker_selection = handles.first().and_then(|h| {
                    let owner = crate::modules::aec::engine::slab_package::resolve_slab_package(
                        &self.tabs[self.active_tab].scene,
                        *h,
                    );
                    self.tabs[self.active_tab]
                        .scene
                        .document
                        .get_entity(owner)
                        .and_then(crate::modules::aec::engine::slab_xdata::slab_from_entity)
                        .map(|s| s.style_id)
                });
                self.aec.aec_style_picker_wall_handles = handles;
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker {
                    target: crate::app::StylePickerTarget::SlabPropertiesStyle,
                }));
                Task::none()
            }
            AecMessage::AecStylePickerOpenForActiveCommand => {
                self.aec_refresh_combined_style_library();
                self.aec.aec_style_picker_filter.clear();
                // Pre-select/highlight the style currently set on the
                // in-progress wall (if any) instead of always starting with
                // nothing highlighted.
                self.aec.aec_style_picker_selection = self.tabs[self.active_tab]
                    .active_cmd
                    .as_ref()
                    .and_then(|c| c.live_property_id("wall_style"));
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker {
                    target: crate::app::StylePickerTarget::ActiveCommand,
                }));
                Task::none()
            }
            AecMessage::AecStylePickerFilterChanged(v) => {
                self.aec.aec_style_picker_filter = v;
                Task::none()
            }
            AecMessage::AecStylePickerSelect(id) => {
                self.aec.aec_style_picker_selection = Some(id);
                Task::none()
            }
            AecMessage::AecStylePickerCancel => {
                // Targets opened from within the Wall Style Manager return
                // there on cancel, instead of closing the modal entirely.
                if let Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker { target })) = self.active_modal
                {
                    if matches!(
                        target,
                        crate::app::StylePickerTarget::WallStyleParent
                            | crate::app::StylePickerTarget::LayerMaterial(_)
                            | crate::app::StylePickerTarget::LayerOverride(_)
                    ) {
                        self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                        return Task::none();
                    }
                    if matches!(
                        target,
                        crate::app::StylePickerTarget::OpeningStyleParent
                    ) {
                        self.active_modal =
                            Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                        return Task::none();
                    }
                    if matches!(
                        target,
                        crate::app::StylePickerTarget::SlabStyleParent
                            | crate::app::StylePickerTarget::SlabLayerMaterial(_)
                            | crate::app::StylePickerTarget::SlabLayerOverride(_)
                    ) {
                        self.active_modal =
                            Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                        return Task::none();
                    }
                    if matches!(
                        target,
                        crate::app::StylePickerTarget::FloorFinishStyleParent
                            | crate::app::StylePickerTarget::FloorFinishLayerMaterial(_)
                            | crate::app::StylePickerTarget::FloorFinishLayerOverride(_)
                    ) {
                        self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                        return Task::none();
                    }
                    if matches!(
                        target,
                        crate::app::StylePickerTarget::SlabStructuralStyleParent
                            | crate::app::StylePickerTarget::SlabStructuralLayerMaterial(_)
                            | crate::app::StylePickerTarget::SlabStructuralLayerOverride(_)
                    ) {
                        self.active_modal =
                            Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                        return Task::none();
                    }
                }
                self.close_active_modal();
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSelect(name) => self.aec_wall_profile_select(name),
            AecMessage::AecStyleManagerProfileContourModeToggle(is_explicit) => {
                self.aec.styles.profile_contour_explicit = is_explicit;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSolidModeToggle(is_explicit) => {
                self.aec.styles.profile_solid_explicit = is_explicit;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileContourLayerToggle(layer) => {
                if let Some(pos) = self
                    .aec.styles.profile_contour_selection
                    .iter()
                    .position(|l| *l == layer)
                {
                    self.aec.styles.profile_contour_selection.remove(pos);
                } else {
                    self.aec.styles.profile_contour_selection.push(layer);
                }
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSolidLayerToggle(layer) => {
                if let Some(pos) = self
                    .aec.styles.profile_solid_selection
                    .iter()
                    .position(|l| *l == layer)
                {
                    self.aec.styles.profile_solid_selection.remove(pos);
                } else {
                    self.aec.styles.profile_solid_selection.push(layer);
                }
                Task::none()
            }
            AecMessage::AecStyleManagerProfileHatchRelativeToggle(value) => {
                self.aec.styles.profile_hatch_relative = value;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileHatchAngleChanged(value) => {
                self.aec.styles.profile_hatch_angle = value;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotVisibilityToggle(slot, visible) => {
                self.aec.styles.profile_slot_visibility
                    .insert(slot, visible);
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleOpen(slot) => {
                use crate::modules::aec::ui::aec_ui_util::acad_color_to_editor_string;
                let style = self
                    .aec.styles.profile_slot_overrides
                    .get(&slot)
                    .cloned()
                    .unwrap_or_default();
                self.aec.styles.profile_editing_slot = Some(slot);
                self.aec.styles.profile_slot_style_line_type =
                    style.line_type.clone().unwrap_or_default();
                self.aec.styles.profile_slot_style_line_color = style
                    .line_color
                    .map(acad_color_to_editor_string)
                    .unwrap_or_default();
                self.aec.styles.profile_slot_style_hatch_pattern =
                    style.hatch_pattern.clone().unwrap_or_default();
                self.aec.styles.profile_slot_style_hatch_color = style
                    .hatch_color
                    .map(acad_color_to_editor_string)
                    .unwrap_or_default();
                self.aec.styles.profile_slot_style_fill_color = style
                    .fill_color
                    .map(acad_color_to_editor_string)
                    .unwrap_or_default();
                self.aec.styles.profile_slot_style_line_color_picker_open = false;
                self.aec.styles.profile_slot_style_hatch_color_picker_open = false;
                self.aec.styles.profile_slot_style_fill_color_picker_open = false;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleLineTypeChanged(v) => {
                self.ensure_aec_catalog_linetype(&v);
                self.aec.styles.profile_slot_style_line_type = v;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleLineColorChanged(v) => {
                self.aec.styles.profile_slot_style_line_color = v;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleLineColorPickerToggle => {
                self.aec.styles.profile_slot_style_line_color_picker_open =
                    !self.aec.styles.profile_slot_style_line_color_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleHatchPatternChanged(v) => {
                self.aec.styles.profile_slot_style_hatch_pattern = v;
                self.aec.styles.profile_slot_style_hatch_picker_open = false;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleHatchPickerToggle => {
                self.aec.styles.profile_slot_style_hatch_picker_open =
                    !self.aec.styles.profile_slot_style_hatch_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleHatchColorChanged(v) => {
                self.aec.styles.profile_slot_style_hatch_color = v;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleHatchColorPickerToggle => {
                self.aec.styles.profile_slot_style_hatch_color_picker_open =
                    !self.aec.styles.profile_slot_style_hatch_color_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleFillColorChanged(v) => {
                self.aec.styles.profile_slot_style_fill_color = v;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleFillColorPickerToggle => {
                self.aec.styles.profile_slot_style_fill_color_picker_open =
                    !self.aec.styles.profile_slot_style_fill_color_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleApply => {
                self.aec_wall_profile_slot_style_apply()
            }
            AecMessage::AecStyleManagerProfileSlotStyleClear => {
                if let Some(slot) = self.aec.styles.profile_editing_slot.take() {
                    self.aec.styles.profile_slot_overrides.remove(&slot);
                }
                self.clear_aec_profile_slot_style_editor_buffers();
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSlotStyleClose => {
                self.aec.styles.profile_editing_slot = None;
                self.clear_aec_profile_slot_style_editor_buffers();
                Task::none()
            }
            AecMessage::AecWallStyleManagerDisplayProfilesOpen => {
                // Keep the wall style manager geometry so closing the child
                // modal can restore it (Plot → Plotstyle pattern).
                self.aec.styles.wall_parent_geometry =
                    Some((self.modal_offset, self.modal_resize));
                self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::WallStyleDisplayProfiles));
                self.reset_modal_geometry();
                Task::none()
            }
            AecMessage::AecWallStyleManagerDisplayProfilesClose => {
                self.close_active_modal();
                Task::none()
            }
            AecMessage::AecStyleManagerProfileSave => {
                self.aec_wall_profile_save()
            }
            AecMessage::AecStyleManagerProfileRemove => {
                self.aec_wall_profile_remove()
            }
            AecMessage::AecStyleManagerProfileRemoveNamed(name) => {
                let _ = self.aec_wall_profile_select(name);
                self.aec_wall_profile_remove()
            }
            AecMessage::AecStylePickerConfirm => {
                if let (Some(crate::app::ModalKind::Aec(AecModalKind::StylePicker { target })), Some(selection)) =
                    (self.active_modal, self.aec.aec_style_picker_selection.clone())
                {
                    match target {
                        crate::app::StylePickerTarget::WallStyleParent => {
                            let id = if selection.is_empty() {
                                None
                            } else {
                                Some(selection)
                            };
                            self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(AecMessage::AecStyleManagerWallStyleParentChanged(id)));
                        }
                        crate::app::StylePickerTarget::LayerMaterial(index) => {
                            self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(AecMessage::AecStyleManagerWallStyleLayerMaterialChanged(
                                index, selection,
                            )));
                        }
                        crate::app::StylePickerTarget::LayerOverride(index) => {
                            self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(AecMessage::AecStyleManagerWallStyleLayerOverrideChanged(
                                index, selection,
                            )));
                        }
                        crate::app::StylePickerTarget::WallPropertiesStyle => {
                            self.active_modal = None;
                            let Some(lib) = self.aec.aec_style_library.clone() else {
                                return Task::none();
                            };

                            let i = self.active_tab;
                            self.push_undo_snapshot(i, "CHPROP");

                            let handles = self.aec.aec_style_picker_wall_handles.clone();
                            for handle in handles {
                                let wall_owner = crate::modules::aec::engine::wall_package::resolve_wall_package(
                                    &self.tabs[i].scene,
                                    handle,
                                );
                                let bb = self.tabs[i]
                                    .scene
                                    .document
                                    .get_entity(wall_owner)
                                    .and_then(crate::modules::aec::engine::xdata::wall_from_entity)
                                    .and_then(|w| {
                                        let t = w.total_thickness();
                                        if t > 0.0 {
                                            Some(t)
                                        } else {
                                            None
                                        }
                                    });
                                let wall_layers = crate::modules::aec::engine::xdata::resolve_wall_style_layers_ids(
                                    &lib,
                                    &selection,
                                    bb,
                                )
                                .unwrap_or_default();

                                if crate::modules::aec::engine::xdata::write_wall_style(
                                    &mut self.tabs[i].scene,
                                    wall_owner,
                                    &selection,
                                    wall_layers,
                                ) {
                                    let _ = self.regenerate_wall_respecting_active_display_config(i, wall_owner);
                                    self.tabs[i].dirty = true;
                                }
                            }
                            self.refresh_properties();
                        }
                        crate::app::StylePickerTarget::OpeningStyleParent => {
                            let id = if selection.is_empty() {
                                None
                            } else {
                                Some(selection)
                            };
                            self.active_modal =
                                Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecOpeningStyleManagerParentChanged(id),
                            ));
                        }
                        crate::app::StylePickerTarget::OpeningComponentMaterial(role) => {
                            self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            self.aec_opening_style_manager_look_material_changed(role, selection);
                            return Task::none();
                        }
                        crate::app::StylePickerTarget::OpeningPropertiesStyle => {
                            self.active_modal = None;
                            return self.aec_apply_picked_opening_style(selection);
                        }
                        crate::app::StylePickerTarget::SlabStyleParent => {
                            let id = if selection.is_empty() {
                                None
                            } else {
                                Some(selection)
                            };
                            self.active_modal =
                                Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecSlabStyleManagerParentChanged(id),
                            ));
                        }
                        crate::app::StylePickerTarget::FloorFinishStyleParent => {
                            let id = if selection.is_empty() {
                                None
                            } else {
                                Some(selection)
                            };
                            self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecFloorFinishStyleManagerParentChanged(id),
                            ));
                        }
                        crate::app::StylePickerTarget::FloorFinishLayerMaterial(index) => {
                            self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecFloorFinishStyleManagerLayerMaterialChanged(
                                    index, selection,
                                ),
                            ));
                        }
                        crate::app::StylePickerTarget::SlabStructuralStyleParent => {
                            let id = if selection.is_empty() {
                                None
                            } else {
                                Some(selection)
                            };
                            self.active_modal =
                                Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecSlabStructuralStyleManagerParentChanged(id),
                            ));
                        }
                        crate::app::StylePickerTarget::SlabLayerMaterial(index) => {
                            self.active_modal =
                                Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecSlabStyleManagerLayerMaterialChanged(
                                    index, selection,
                                ),
                            ));
                        }
                        crate::app::StylePickerTarget::SlabStructuralLayerMaterial(index) => {
                            self.active_modal =
                                Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecSlabStructuralStyleManagerLayerMaterialChanged(
                                    index, selection,
                                ),
                            ));
                        }
                        crate::app::StylePickerTarget::SlabStructuralLayerOverride(index) => {
                            self.active_modal =
                                Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecSlabStructuralStyleManagerLayerOverrideChanged(
                                    index, selection,
                                ),
                            ));
                        }
                        crate::app::StylePickerTarget::FloorFinishLayerOverride(index) => {
                            self.active_modal =
                                Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecFloorFinishStyleManagerLayerOverrideChanged(
                                    index, selection,
                                ),
                            ));
                        }
                        crate::app::StylePickerTarget::SlabLayerOverride(index) => {
                            self.active_modal =
                                Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
                            return self.update(Message::Aec(
                                AecMessage::AecSlabStyleManagerLayerOverrideChanged(
                                    index, selection,
                                ),
                            ));
                        }
                        crate::app::StylePickerTarget::SlabPropertiesStyle => {
                            self.active_modal = None;
                            let Some(lib) = self.aec.aec_style_library.clone() else {
                                return Task::none();
                            };
                            let i = self.active_tab;
                            self.push_undo_snapshot(i, "CHPROP");
                            let handles = self.aec.aec_style_picker_wall_handles.clone();
                            if !selection.is_empty() {
                                self.aec.aec_last_slab_style_id = Some(selection.clone());
                            }
                            for handle in handles {
                                let slab_owner =
                                    crate::modules::aec::engine::slab_package::resolve_slab_package(
                                        &self.tabs[i].scene,
                                        handle,
                                    );
                                let thickness = self.tabs[i]
                                    .scene
                                    .document
                                    .get_entity(slab_owner)
                                    .and_then(crate::modules::aec::engine::slab_xdata::slab_from_entity)
                                    .and_then(|s| {
                                        let t = s.total_thickness();
                                        if t > 0.0 {
                                            Some(t)
                                        } else {
                                            None
                                        }
                                    });
                                let slab_layers = crate::modules::aec::engine::slab_xdata::resolve_slab_style_layers_ids(
                                    &lib,
                                    &selection,
                                    thickness,
                                )
                                .unwrap_or_default();
                                if crate::modules::aec::engine::slab_xdata::write_slab_style(
                                    &mut self.tabs[i].scene,
                                    slab_owner,
                                    &selection,
                                    slab_layers,
                                ) {
                                    let _ = self
                                        .regenerate_slab_respecting_active_display_config(i, slab_owner);
                                    self.tabs[i].dirty = true;
                                }
                            }
                            self.refresh_properties();
                        }
                        crate::app::StylePickerTarget::ActiveCommand => {
                            let i = self.active_tab;
                            if !selection.is_empty() {
                                self.aec.aec_last_wall_style_id = Some(selection.clone());
                            }
                            let result = self.tabs[i].active_cmd.as_mut().map(|c| {
                                c.apply_live_property(
                                    "wall_style",
                                    crate::command::LiveFieldValue::Picker(selection.clone()),
                                )
                            });
                            self.active_modal = None;
                            if let Some(r) = result {
                                let task = self.apply_cmd_result(r);
                                self.refresh_active_cmd_preview(i);
                                self.refresh_properties();
                                return task;
                            }
                            self.refresh_properties();
                            return Task::none();
                        }
                    }
                }
                self.active_modal = None;
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleSave => {
                self.aec_style_manager_wall_style_save_internal();
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleSaveAndApply => {
                if let Some((id, lib)) = self.aec_style_manager_wall_style_save_internal() {
                    let tab_index = self.active_tab;
                    let wall_style_map: std::collections::HashMap<
                        String,
                        crate::modules::aec::engine::wall_style::WallStyle,
                    > = lib
                        .wall_styles
                        .iter()
                        .map(|ws| (ws.style.id.clone(), ws.clone()))
                        .collect();

                    let style_map: std::collections::HashMap<
                        String,
                        crate::modules::aec::engine::style::Style,
                    > = wall_style_map
                        .iter()
                        .map(|(k, v)| (k.clone(), v.style.clone()))
                        .collect();

                    let mut affected_handles = Vec::new();
                    for entity in self.tabs[tab_index].scene.document.entities() {
                        if let Some(wall) =
                            crate::modules::aec::engine::xdata::wall_from_entity(entity)
                        {
                            if wall.style_id == id {
                                affected_handles.push(entity.common().handle);
                            } else if let Ok(chain) =
                                crate::modules::aec::engine::style::resolve_chain(
                                    &style_map,
                                    &wall.style_id,
                                )
                            {
                                if chain.contains(&id) {
                                    affected_handles.push(entity.common().handle);
                                }
                            }
                        }
                    }

                    let mut updated_count = 0;
                    for handle in affected_handles {
                        if let Some(entity) = self.tabs[tab_index].scene.document.get_entity(handle) {
                            if let Some(wall) =
                                crate::modules::aec::engine::xdata::wall_from_entity(entity)
                            {
                                // Prefer the wall's current total thickness as BB so
                                // formula layers scale with the placed wall width;
                                // fall back to style fixed-sum when empty.
                                let bb = {
                                    let t = wall.total_thickness();
                                    if t > 0.0 {
                                        Some(t)
                                    } else {
                                        None
                                    }
                                };
                                if let Some(wall_layers) =
                                    crate::modules::aec::engine::xdata::resolve_wall_style_layers_ids(
                                        &lib,
                                        &wall.style_id,
                                        bb,
                                    )
                                {
                                    crate::modules::aec::engine::xdata::write_wall_layers(
                                        &mut self.tabs[tab_index].scene,
                                        handle,
                                        wall_layers,
                                    );
                                }
                            }
                        }

                        if self
                            .regenerate_wall_respecting_active_display_config(tab_index, handle)
                            .is_ok()
                        {
                            updated_count += 1;
                        }
                    }

                    if updated_count > 0 {
                        self.tabs[tab_index].scene.bump_geometry();
                        self.command_line.push_info(
                            crate::tf!(
                                "AEC Style Manager: updated {updated_count} wall(s) using this style."
                            )
                            .as_ref(),
                        );
                    }
                }
                Task::none()
            }
            AecMessage::AecStyleManagerWallStyleDelete => {
                if let Some(id) = self.aec.styles.selected_wall_style.clone() {
                    let lib_snapshot = if let Some(lib) = self.aec.aec_style_library.as_mut() {
                        lib.remove_wall_style(&id);
                        Some(lib.clone())
                    } else {
                        None
                    };
                    if let Some(lib_snapshot) = lib_snapshot {
                        match self.aec_save_style_library_preferring_project(&lib_snapshot) {
                            Ok(()) => self.command_line.push_info(
                                crate::t!("AEC Style Manager: wall style deleted.").as_ref(),
                            ),
                            Err(e) => self.command_line.push_error(
                                crate::tf!("AEC Style Manager: failed to save library: {e}")
                                    .as_ref(),
                            ),
                        }
                    }
                }
                self.aec.styles.selected_wall_style = None;
                self.aec.styles.wall_style_editing_id = None;
                self.aec.styles.wall_duplicate_default_profile = None;
                self.aec.styles.wall_style_form_open = false;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialNew => {
                self.aec.styles.selected_material = None;
                self.aec.styles.selected_wall_style = None;
                self.aec.styles.material_editing_id = None;
                self.aec.styles.material_name.clear();
                self.aec.styles.material_hatch.clear();
                self.aec.styles.material_color = "#FFFFFF".to_string();
                self.aec.styles.material_line_type = "Continuous".to_string();
                self.aec.styles.material_category.clear();
                self.aec.styles.material_hatch_color = 0xFFFFFF;
                self.aec.styles.material_hatch_scale = "1.0".to_string();
                self.aec.styles.material_render_ref.clear();
                self.aec.styles.material_hatch_angle = "0.0".to_string();
                self.aec.styles.material_hatch_angle_relative = true;
                self.aec.styles.material_hatch_color_picker_open = false;
                self.aec.styles.material_form_open = true;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialNameChanged(value) => {
                self.aec.styles.material_name = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchChanged(value) => {
                self.aec.styles.material_hatch = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchPickerToggle => {
                self.aec.styles.material_hatch_picker_open =
                    !self.aec.styles.material_hatch_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchSelected(name) => {
                self.aec.styles.material_hatch = name;
                self.aec.styles.material_hatch_picker_open = false;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialColorChanged(value) => {
                self.aec.styles.material_color = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialLineTypeChanged(value) => {
                self.ensure_aec_catalog_linetype(&value);
                self.aec.styles.material_line_type = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialColorPickerToggle => {
                self.aec.styles.material_color_picker_open =
                    !self.aec.styles.material_color_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialColorPicked(color) => {
                self.aec.styles.material_color_picker_open = false;
                let [r, g, b, _] = match color {
                    acadrust::types::Color::Rgb { r, g, b } => [r, g, b, 255u8],
                    acadrust::types::Color::Index(i) => {
                        let (r, g, b) = acadrust::types::aci_table::aci_to_rgb(i)
                            .unwrap_or((255, 255, 255));
                        [r, g, b, 255]
                    }
                    _ => [255, 255, 255, 255],
                };
                self.aec.styles.material_color = format!("#{r:02X}{g:02X}{b:02X}");
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialCategoryChanged(value) => {
                self.aec.styles.material_category = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchColorChanged(value) => {
                self.aec.styles.material_hatch_color = value;
                self.aec.styles.material_hatch_color_picker_open = false;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchColorPickerToggle => {
                self.aec.styles.material_hatch_color_picker_open =
                    !self.aec.styles.material_hatch_color_picker_open;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchScaleChanged(value) => {
                self.aec.styles.material_hatch_scale = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialRenderRefChanged(value) => {
                self.aec.styles.material_render_ref = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchAngleChanged(value) => {
                self.aec.styles.material_hatch_angle = value;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialHatchAngleRelativeToggle => {
                self.aec.styles.material_hatch_angle_relative =
                    !self.aec.styles.material_hatch_angle_relative;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialDuplicate => {
                let source_id = self
                    .aec.styles.material_editing_id
                    .clone()
                    .or_else(|| self.aec.styles.selected_material.clone());
                let Some(source_id) = source_id else {
                    return Task::none();
                };
                let Some(material) = self
                    .aec.aec_style_library
                    .as_ref()
                    .and_then(|lib| lib.materials.iter().find(|m| m.id == source_id))
                    .cloned()
                else {
                    return Task::none();
                };
                let existing_names: Vec<String> = self
                    .aec.aec_style_library
                    .as_ref()
                    .map(|lib| lib.materials.iter().map(|m| m.name.clone()).collect())
                    .unwrap_or_default();
                let base_name = format!("{} (Kopie)", material.name);
                let mut name = base_name.clone();
                let mut counter = 2;
                while existing_names.iter().any(|n| n == &name) {
                    name = format!("{base_name} {counter}");
                    counter += 1;
                }
                self.aec.styles.selected_material = None;
                self.aec.styles.selected_wall_style = None;
                self.aec.styles.material_editing_id = None;
                self.aec.styles.material_name = name;
                self.aec.styles.material_hatch = material.hatch_pattern;
                self.aec.styles.material_color =
                    format!("#{:06X}", material.line_color);
                self.aec.styles.material_line_type = material.line_type;
                self.aec.styles.material_category =
                    material.group.unwrap_or_default();
                                self.aec.styles.material_hatch_color = material
                    .hatch_color
                    .and_then(|c| c.rgb())
                    .map(|(r, g, b)| ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
                    .unwrap_or(material.line_color);
                self.aec.styles.material_hatch_scale =
                    format!("{}", material.hatch_scale);
                self.aec.styles.material_render_ref =
                    material.render_material_ref.unwrap_or_default();
                self.aec.styles.material_hatch_angle =
                    format!("{}", material.hatch_angle);
                self.aec.styles.material_hatch_angle_relative =
                    material.hatch_angle_relative;
                self.aec.styles.material_color_picker_open = false;
                self.aec.styles.material_hatch_color_picker_open = false;
                self.aec.styles.material_hatch_picker_open = false;
                self.aec.styles.material_form_open = true;
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialSave => {
                self.aec_style_manager_material_save_internal();
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialSaveAndApply => {
                if let Some((mat_id, _lib)) = self.aec_style_manager_material_save_internal() {
                    let tab_index = self.active_tab;
                    let mut affected_walls = Vec::new();
                    for entity in self.tabs[tab_index].scene.document.entities() {
                        if let Some(wall) =
                            crate::modules::aec::engine::xdata::wall_from_entity(entity)
                        {
                            if wall.layers.iter().any(|l| l.material == mat_id) {
                                affected_walls.push(entity.common().handle);
                            }
                        }
                    }
                    let mut updated_count = 0;
                    for handle in affected_walls {
                        if self
                            .regenerate_wall_respecting_active_display_config(tab_index, handle)
                            .is_ok()
                        {
                            updated_count += 1;
                        }
                    }
                    if updated_count > 0 {
                        self.tabs[tab_index].scene.bump_geometry();
                        self.command_line.push_info(
                            crate::tf!(
                                "AEC Style Manager: updated {updated_count} wall(s) using this material."
                            )
                            .as_ref(),
                        );
                    }
                }
                Task::none()
            }
            AecMessage::AecStyleManagerMaterialDelete => {
                if let Some(id) = self.aec.styles.selected_material.clone() {
                    let lib_snapshot = if let Some(lib) = self.aec.aec_style_library.as_mut() {
                        lib.remove_material(&id);
                        Some(lib.clone())
                    } else {
                        None
                    };
                    if let Some(lib_snapshot) = lib_snapshot {
                        match self.aec_save_style_library_preferring_project(&lib_snapshot) {
                            Ok(()) => self.command_line.push_info(
                                crate::t!("AEC Style Manager: material deleted.").as_ref(),
                            ),
                            Err(e) => self.command_line.push_error(
                                crate::tf!("AEC Style Manager: failed to save library: {e}")
                                    .as_ref(),
                            ),
                        }
                    }
                }
                self.aec.styles.selected_material = None;
                self.aec.styles.material_editing_id = None;
                self.aec.styles.material_form_open = false;
                Task::none()
            }
            AecMessage::AecStyleManagerCopyMaterialToProject => self.aec_handle_copy_material(true),
            AecMessage::AecStyleManagerCopyMaterialToGlobal => self.aec_handle_copy_material(false),
            AecMessage::AecStyleManagerCopyWallStyleToProject => self.aec_handle_copy_wall_style(true),
            AecMessage::AecStyleManagerCopyWallStyleToGlobal => self.aec_handle_copy_wall_style(false),
            AecMessage::AecStyleManagerCopyConflictConfirm(confirmed) => {
                self.aec.styles.copy_conflict_open = false;
                let return_modal = crate::app::ModalKind::Aec(AecModalKind::Styles);
                if confirmed {
                    if let Some(pending) = self.aec.styles.pending_copy.take() {
                        self.aec_execute_copy(pending);
                    }
                } else {
                    self.aec.styles.pending_copy = None;
                }
                self.active_modal = Some(return_modal);
                Task::none()
            }
            // ── Layer Translator (#624) ──────────────────────────────────
            _ => Task::none(),
        }
    }
}
