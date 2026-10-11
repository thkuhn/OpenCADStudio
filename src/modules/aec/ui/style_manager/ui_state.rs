//! Unified UI state of the `AEC_STYLES` window: selected tree node, filters,
//! selections and the per-kind form buffers.

use crate::modules::aec::state::*;

#[derive(Debug)]
pub struct AecStylesUiState {
    /// Filter text applied to both the material and wall-style master lists.
    pub filter: String,
    /// Selected tree node of `AEC_STYLES`.
    pub node: crate::modules::aec::ui::style_manager::StylesNode,
    /// Currently selected material id (if any), by `Material::id`.
    pub selected_material: Option<String>,
    /// Currently selected wall style id (if any), by `Style::id`.
    pub selected_wall_style: Option<String>,
    /// Id of the material currently being edited, if the edit buffer holds
    /// an existing material (`None` while composing a new/unsaved one).
    pub material_editing_id: Option<String>,
    /// Whether the material edit form is visible (set by selecting a
    /// material or pressing "New"; cleared on Save/Delete/deselect).
    pub material_form_open: bool,
    /// Edit-buffer fields for the material form (name/hatch/color/line type).
    pub material_name: String,
    pub material_hatch: String,
    pub material_color: String,
    pub material_line_type: String,
    pub material_category: String,
    pub material_hatch_color: u32,
    pub material_hatch_scale: String,
    pub material_render_ref: String,
    pub material_hatch_angle: String,
    /// `true` when `hatch_angle` is relative to the wall's own direction,
    /// `false` when it is an absolute/global angle.
    pub material_hatch_angle_relative: bool,
    /// Whether the material form's colour-picker popup is expanded.
    pub material_color_picker_open: bool,
    /// Whether the material form's hatch-colour-picker popup is expanded.
    pub material_hatch_color_picker_open: bool,
    /// Whether the material form's visual hatch-pattern picker is expanded.
    pub material_hatch_picker_open: bool,
    /// Line types available for the material form's line-type combo box
    /// (name + ASCII-art preview), refreshed when the manager is opened.
    pub material_linetype_items: Vec<crate::ui::properties::LinetypeItem>,
    /// combo_box state built from `material_linetype_items`.
    pub material_linetype_combo:
        iced::widget::combo_box::State<crate::ui::properties::LinetypeItem>,
    /// Id of the wall style currently being edited, if the edit buffer holds
    /// an existing wall style (`None` while composing a new/unsaved one).
    pub wall_style_editing_id: Option<String>,
    /// Style default profile carried into an unsaved Duplicate.
    pub wall_duplicate_default_profile: Option<crate::modules::aec::engine::display_component::ComponentRuleSet>,
    /// Whether the wall style edit form is visible.
    pub wall_style_form_open: bool,
    /// Edit-buffer fields for the wall style form (name/parent/layers).
    pub wall_style_name: String,
    pub wall_style_parent: Option<String>,
    pub wall_style_layers: Vec<AecLayerBuffer>,
    /// Index of the layer row currently "picked up" for a click-based
    /// drag-and-drop reorder (armed by its drag handle, dropped by clicking
    /// another row's drag handle); `None` when no drag is in progress.
    pub wall_style_drag_index: Option<usize>,
    /// How the "Wall Styles" master list is ordered: alphabetically by name,
    /// or hierarchically (parents before children, siblings grouped).
    pub wall_style_sort: AecWallStyleSort,
    /// Whether a copy-conflict confirmation dialog is currently open in the
    /// AEC Style Manager.
    pub copy_conflict_open: bool,
    /// The material or wall style currently awaiting an overwrite confirmation.
    pub pending_copy: Option<AecPendingCopy>,
    // ── AEC Wall Style Manager: Darstellungs-Profile (Step 4) ──────────────
    /// Name of the `DisplayConfig` currently selected in the "Darstellungs-
    /// Profile" table of the wall style form (`None` = none selected).
    pub profile_selected: Option<String>,
    /// Layer-Filter-UI: `false` = "Alle" (`LayerSelection::All`), `true` =
    /// "Auswahl", for the currently edited profile's `Contour2D` slot.
    pub profile_contour_explicit: bool,
    /// Layer-Filter-UI: edit-buffer of explicitly selected layers for
    /// `Contour2D`, only relevant while the above is `true`.
    pub profile_contour_selection:
        Vec<crate::modules::aec::engine::join::LayerRef>,
    /// Same as `..._contour_explicit`, but for the `Solid3D` slot.
    pub profile_solid_explicit: bool,
    /// Same as `..._contour_selection`, but for the `Solid3D` slot.
    pub profile_solid_selection:
        Vec<crate::modules::aec::engine::join::LayerRef>,
    /// Hatch-angle override text field ("Relativ zur Wand" + Winkel) for
    /// the currently edited profile (empty = no override).
    pub profile_hatch_angle: String,
    /// Whether `profile_hatch_angle` is relative to the
    /// wall's own run direction (mirrors `ComponentStyleOverride::hatch_angle_relative`).
    pub profile_hatch_relative: bool,
    /// Visibility checkboxes for each `WallComponentSlot` in the currently
    /// edited profile (Step 4). Absent keys mean visible (default).
    pub profile_slot_visibility:
        std::collections::HashMap<crate::modules::aec::engine::display_component::WallComponentSlot, bool>,
    /// Slot currently open in the per-slot style override editor.
    pub profile_editing_slot:
        Option<crate::modules::aec::engine::display_component::WallComponentSlot>,
    /// Pending per-slot style overrides for the selected display profile.
    pub profile_slot_overrides: std::collections::HashMap<
        crate::modules::aec::engine::display_component::WallComponentSlot,
        crate::modules::aec::engine::display_component::ComponentStyleOverride,
    >,
    pub profile_slot_style_line_type: String,
    pub profile_slot_style_line_color: String,
    pub profile_slot_style_hatch_pattern: String,
    pub profile_slot_style_hatch_color: String,
    pub profile_slot_style_fill_color: String,
    pub profile_slot_style_line_color_picker_open: bool,
    pub profile_slot_style_hatch_color_picker_open: bool,
    pub profile_slot_style_fill_color_picker_open: bool,
    pub profile_slot_style_hatch_picker_open: bool,
    /// Geometry of the wall-style manager while the display-profiles child
    /// modal is open (Plot → Plotstyle pattern).
    pub wall_parent_geometry: Option<(iced::Vector, iced::Vector)>,
    // ── AEC Opening Style Manager ─────────────────────────────────────────
    pub opening_filter: String,
    pub opening_selected: Option<String>,
    pub opening_editing_id: Option<String>,
    pub opening_form_open: bool,
    pub opening_preview_mode: AecOpeningPreviewMode,
    pub opening_name: String,
    pub opening_parent: Option<String>,
    pub opening_kind: crate::modules::aec::engine::openings::OpeningKind,
    pub opening_width: String,
    pub opening_height: String,
    pub opening_sill: String,
    pub opening_frame: String,
    pub opening_angle: String,
    pub opening_spring: String,
    pub opening_hinge: crate::modules::aec::engine::opening_style::HingeSide,
    pub opening_shape: crate::modules::aec::engine::opening_shape::OpeningShape,
    pub opening_slots: Vec<AecOpeningSlotBuffer>,
    /// Active plan-type name while editing slot overrides (`None` = default).
    pub opening_profile_selected: Option<String>,
    /// Default slot map (not plan-specific) while the form is open.
    pub opening_default_slots:
        std::collections::HashMap<crate::modules::aec::engine::display_component::OpeningComponentSlot, crate::modules::aec::engine::opening_style::SlotGeometry>,
    /// Plan-kind slot overrides of the edited style while the form is open,
    /// keyed by plan name (UI buffer only; persisted into the plan's
    /// `DisplayConfig::style_overrides`, never into the style).
    pub opening_display_profiles:
        std::collections::HashMap<String, crate::modules::aec::engine::opening_style::OpeningDisplayProfile>,
    /// Name-independent default display profile of the edited style.
    pub opening_default_profile:
        Option<crate::modules::aec::engine::opening_style::OpeningDisplayProfile>,
    /// Slot-table row currently targeted by the manager sketch canvas.
    pub opening_sketch_slot: Option<usize>,
    /// In-progress polyline in the selected slot's reference box.
    pub opening_sketch_draft: Vec<(f64, f64)>,
    pub opening_sketch_draft_bulges: Vec<f64>,
    // ── AEC Slab Style Manager (`AEC_SLABSTYLEMANAGER`) ───────────────────
    pub slab_filter: String,
    pub slab_selected: Option<String>,
    pub slab_editing_id: Option<String>,
    pub slab_form_open: bool,
    pub slab_preview_mode: AecSlabPreviewMode,
    pub slab_name: String,
    pub slab_parent: Option<String>,
    pub slab_structural_style: Option<String>,
    pub slab_finish_style: Option<String>,
    pub slab_layers: Vec<AecSlabLayerBuffer>,
    pub slab_drag_index: Option<usize>,
    pub slab_profile_selected: Option<String>,
    pub slab_default_profile: Option<crate::modules::aec::engine::display_component::ComponentRuleSet>,
    pub slab_plan_profile_edits: std::collections::HashMap<String, crate::modules::aec::engine::display_component::ComponentRuleSet>,
    pub slab_profile_slot_visibility:
        std::collections::HashMap<crate::modules::aec::engine::display_component::SlabComponentSlot, bool>,
    // ── AEC Slab Structural Style Manager (`AEC_SLABSTRUCTURALSTYLEMANAGER`) ──
    pub slab_structural_filter: String,
    pub slab_structural_selected: Option<String>,
    pub slab_structural_editing_id: Option<String>,
    pub slab_structural_form_open: bool,
    pub slab_structural_preview_mode: AecSlabPreviewMode,
    pub slab_structural_name: String,
    pub slab_structural_parent: Option<String>,
    pub slab_structural_layers: Vec<AecSlabLayerBuffer>,
    pub slab_structural_drag_index: Option<usize>,
    // ── AEC Floor Finish Style Manager (`AEC_FLOORFINISHSTYLEMANAGER`) ──
    pub floor_finish_filter: String,
    pub floor_finish_selected: Option<String>,
    pub floor_finish_editing_id: Option<String>,
    pub floor_finish_form_open: bool,
    pub floor_finish_preview_mode: AecSlabPreviewMode,
    pub floor_finish_name: String,
    pub floor_finish_parent: Option<String>,
    pub floor_finish_layers: Vec<AecSlabLayerBuffer>,
    pub floor_finish_drag_index: Option<usize>,
    /// Filter text applied to the DisplayConfig master list.
    pub plan_filter: String,
    /// Currently selected config name (if any), by `DisplayConfig::name`.
    pub plan_selected: Option<String>,
    /// Name of the config currently being edited, if the edit buffer holds
    /// an existing config (`None` while composing a new/unsaved one).
    pub plan_editing_name: Option<String>,
    /// Whether the DisplayConfig edit form is visible.
    pub plan_form_open: bool,
    /// Edit-buffer fields for the DisplayConfig form.
    pub plan_name: String,
    pub plan_discipline: String,
    pub plan_scale: String,
    pub plan_planning_stage: crate::modules::aec::engine::plan_view::PlanningStage,
    pub plan_view_type: crate::modules::aec::engine::plan_view::ViewType,
    /// Two-stage phase-filter editor (Step 5), Stage 1: which
    /// [`crate::modules::aec::engine::plan_view::PlanPhase`]s are visible
    /// for the currently edited config, one bool per phase.
    pub plan_phase_filter_visible_existing: bool,
    pub plan_phase_filter_visible_demolition: bool,
    pub plan_phase_filter_visible_new: bool,
    /// Stage 2: edit-buffer for `PhaseFilter::demolition_style` (hex colors
    /// as `"RRGGBB"` text, mirrors the old Step 8 style-editor fields).
    pub plan_demolition_style_line_type: String,
    pub plan_demolition_style_line_color: String,
    pub plan_demolition_style_hatch_pattern: String,
    pub plan_demolition_style_hatch_color: String,
    pub plan_demolition_style_fill_color: String,
    /// Stage 2: whether each `demolition_style` colour-picker dropdown is open
    /// (mirrors the Layer Manager's `color_selector` widget pattern).
    pub plan_demolition_style_line_color_picker_open: bool,
    pub plan_demolition_style_hatch_color_picker_open: bool,
    pub plan_demolition_style_fill_color_picker_open: bool,
    pub plan_demolition_style_hatch_picker_open: bool,
    /// Stage 2: edit-buffer for `PhaseFilter::existing_style`.
    pub plan_existing_style_line_type: String,
    pub plan_existing_style_line_color: String,
    pub plan_existing_style_hatch_pattern: String,
    pub plan_existing_style_hatch_color: String,
    pub plan_existing_style_fill_color: String,
    /// Stage 2: whether each `existing_style` colour-picker dropdown is open.
    pub plan_existing_style_line_color_picker_open: bool,
    pub plan_existing_style_hatch_color_picker_open: bool,
    pub plan_existing_style_fill_color_picker_open: bool,
    pub plan_existing_style_hatch_picker_open: bool,
    /// Stable `DisplayConfig.id` while editing an existing plan type.
    pub plan_editing_id: Option<uuid::Uuid>,
    /// Detail switches of the plan kind being edited.
    pub plan_detail: crate::modules::aec::engine::plan_view::DetailProfile,
    /// Standard preset id of the plan kind being edited.
    pub plan_preset: Option<String>,
    pub plan_default_representation:
        crate::modules::aec::engine::display_component::RepresentationMode,
    pub plan_component_visibility: std::collections::HashMap<
        crate::modules::aec::engine::display_component::WallComponentKind,
        bool,
    >,
    pub plan_style_overlays: std::collections::HashMap<
        String,
        crate::modules::aec::engine::display_component::StyleDisplayOverlay,
    >,
    pub plan_overlay_style_id: Option<String>,
    pub plan_overlay_layer_id: Option<uuid::Uuid>,
    pub plan_overlay_line_type: String,
    pub plan_overlay_line_color: String,
    pub plan_overlay_hatch_pattern: String,
    pub plan_overlay_hatch_color: String,
    pub plan_overlay_hatch_scale: String,
    pub plan_overlay_hatch_angle: String,
    /// `None` inherits; `Some(true)` hatch relative to wall run.
    pub plan_overlay_hatch_angle_relative: Option<bool>,
    pub plan_overlay_fill_color: String,
    pub plan_overlay_line_color_picker_open: bool,
    pub plan_overlay_hatch_picker_open: bool,
    pub plan_overlay_hatch_color_picker_open: bool,
    pub plan_overlay_fill_color_picker_open: bool,
    pub plan_contour_hatch_pattern: String,
    pub plan_contour_hatch_color: String,
    pub plan_contour_hatch_scale: String,
    pub plan_contour_hatch_angle: String,
    pub plan_contour_hatch_angle_relative: Option<bool>,
    pub plan_contour_hatch_picker_open: bool,
    pub plan_contour_hatch_color_picker_open: bool,
    pub plan_wall_styles: Vec<(String, String, Vec<(uuid::Uuid, String)>)>,
}

impl Default for AecStylesUiState {
    fn default() -> Self {
        Self {
            filter: String::new(),
            node: Default::default(),
            selected_material: None,
            selected_wall_style: None,
            material_editing_id: None,
            material_form_open: false,
            material_name: String::new(),
            material_hatch: String::new(),
            material_color: String::new(),
            material_line_type: String::new(),
            material_category: String::new(),
            material_hatch_color: 0xFFFFFF,
            material_hatch_scale: "1.0".to_string(),
            material_render_ref: String::new(),
            material_hatch_angle: "0.0".to_string(),
            material_hatch_angle_relative: true,
            material_color_picker_open: false,
            material_hatch_color_picker_open: false,
            material_hatch_picker_open: false,
            material_linetype_items: Vec::new(),
            material_linetype_combo: iced::widget::combo_box::State::new(Vec::new()),
            wall_style_editing_id: None,
            wall_duplicate_default_profile: None,
            wall_style_form_open: false,
            wall_style_name: String::new(),
            wall_style_parent: None,
            wall_style_layers: Vec::new(),
            wall_style_drag_index: None,
            wall_style_sort: AecWallStyleSort::default(),
            copy_conflict_open: false,
            pending_copy: None,
            profile_selected: None,
            profile_contour_explicit: false,
            profile_contour_selection: Vec::new(),
            profile_solid_explicit: false,
            profile_solid_selection: Vec::new(),
            profile_hatch_angle: String::new(),
            profile_hatch_relative: false,
            profile_slot_visibility: std::collections::HashMap::new(),
            profile_editing_slot: None,
            profile_slot_overrides: std::collections::HashMap::new(),
            profile_slot_style_line_type: String::new(),
            profile_slot_style_line_color: String::new(),
            profile_slot_style_hatch_pattern: String::new(),
            profile_slot_style_hatch_color: String::new(),
            profile_slot_style_fill_color: String::new(),
            profile_slot_style_line_color_picker_open: false,
            profile_slot_style_hatch_color_picker_open: false,
            profile_slot_style_fill_color_picker_open: false,
            profile_slot_style_hatch_picker_open: false,
            wall_parent_geometry: None,
            opening_filter: String::new(),
            opening_selected: None,
            opening_editing_id: None,
            opening_form_open: false,
            opening_preview_mode: AecOpeningPreviewMode::Plan2D,
            opening_name: String::new(),
            opening_parent: None,
            opening_kind: crate::modules::aec::engine::openings::OpeningKind::Window,
            opening_width: "1.2".to_string(),
            opening_height: "1.2".to_string(),
            opening_sill: "0.9".to_string(),
            opening_frame: "0.06".to_string(),
            opening_angle: "90".to_string(),
            opening_spring: String::new(),
            opening_hinge: crate::modules::aec::engine::opening_style::HingeSide::Left,
            opening_shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
            opening_slots: Vec::new(),
            opening_profile_selected: None,
            opening_default_slots: std::collections::HashMap::new(),
            opening_display_profiles: Default::default(),
            opening_default_profile: None,
            opening_sketch_slot: None,
            opening_sketch_draft: Vec::new(),
            opening_sketch_draft_bulges: Vec::new(),
            slab_filter: String::new(),
            slab_selected: None,
            slab_editing_id: None,
            slab_form_open: false,
            slab_preview_mode: AecSlabPreviewMode::CrossSection,
            slab_name: String::new(),
            slab_parent: None,
            slab_structural_style: None,
            slab_finish_style: None,
            slab_layers: Vec::new(),
            slab_drag_index: None,
            slab_profile_selected: None,
            slab_default_profile: None,
            slab_plan_profile_edits: std::collections::HashMap::new(),
            slab_profile_slot_visibility: std::collections::HashMap::new(),
            slab_structural_filter: String::new(),
            slab_structural_selected: None,
            slab_structural_editing_id: None,
            slab_structural_form_open: false,
            slab_structural_preview_mode: AecSlabPreviewMode::CrossSection,
            slab_structural_name: String::new(),
            slab_structural_parent: None,
            slab_structural_layers: Vec::new(),
            slab_structural_drag_index: None,
            floor_finish_filter: String::new(),
            floor_finish_selected: None,
            floor_finish_editing_id: None,
            floor_finish_form_open: false,
            floor_finish_preview_mode: AecSlabPreviewMode::CrossSection,
            floor_finish_name: String::new(),
            floor_finish_parent: None,
            floor_finish_layers: Vec::new(),
            floor_finish_drag_index: None,
            plan_filter: String::new(),
            plan_selected: None,
            plan_editing_name: None,
            plan_form_open: false,
            plan_name: String::new(),
            plan_discipline: String::new(),
            plan_scale: String::new(),
            plan_planning_stage: crate::modules::aec::engine::plan_view::PlanningStage::Design,
            plan_view_type: crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
            plan_phase_filter_visible_existing: true,
            plan_phase_filter_visible_demolition: true,
            plan_phase_filter_visible_new: true,
            plan_demolition_style_line_type: String::new(),
            plan_demolition_style_line_color: String::new(),
            plan_demolition_style_hatch_pattern: String::new(),
            plan_demolition_style_hatch_color: String::new(),
            plan_demolition_style_fill_color: String::new(),
            plan_demolition_style_line_color_picker_open: false,
            plan_demolition_style_hatch_color_picker_open: false,
            plan_demolition_style_fill_color_picker_open: false,
            plan_demolition_style_hatch_picker_open: false,
            plan_existing_style_line_type: String::new(),
            plan_existing_style_line_color: String::new(),
            plan_existing_style_hatch_pattern: String::new(),
            plan_existing_style_hatch_color: String::new(),
            plan_existing_style_fill_color: String::new(),
            plan_existing_style_line_color_picker_open: false,
            plan_existing_style_hatch_color_picker_open: false,
            plan_existing_style_fill_color_picker_open: false,
            plan_existing_style_hatch_picker_open: false,
            plan_editing_id: None,
            plan_detail: Default::default(),
            plan_preset: None,
            plan_default_representation: crate::modules::aec::engine::display_component::RepresentationMode::All,
            plan_component_visibility: std::collections::HashMap::new(),
            plan_style_overlays: std::collections::HashMap::new(),
            plan_overlay_style_id: None,
            plan_overlay_layer_id: None,
            plan_overlay_line_type: String::new(),
            plan_overlay_line_color: String::new(),
            plan_overlay_hatch_pattern: String::new(),
            plan_overlay_hatch_color: String::new(),
            plan_overlay_hatch_scale: String::new(),
            plan_overlay_hatch_angle: String::new(),
            plan_overlay_hatch_angle_relative: None,
            plan_overlay_fill_color: String::new(),
            plan_overlay_line_color_picker_open: false,
            plan_overlay_hatch_picker_open: false,
            plan_overlay_hatch_color_picker_open: false,
            plan_overlay_fill_color_picker_open: false,
            plan_contour_hatch_pattern: String::new(),
            plan_contour_hatch_color: String::new(),
            plan_contour_hatch_scale: String::new(),
            plan_contour_hatch_angle: String::new(),
            plan_contour_hatch_angle_relative: None,
            plan_contour_hatch_picker_open: false,
            plan_contour_hatch_color_picker_open: false,
            plan_wall_styles: Vec::new(),
        }
    }
}
