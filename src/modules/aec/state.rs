//! AEC UI/session state owned by the AEC module (CadApp holds `aec: AecState`).

/// Edit-buffer row for a wall-style layer in the AEC Style Manager.
#[derive(Clone, Debug)]
pub struct AecLayerBuffer {
    /// Selected material id.
    pub material_id: String,
    /// Thickness string: plain number or arithmetic formula (e.g. `BB * 0.5`).
    /// Parsed to [`LayerValue`] on save.
    pub thickness: String,
    /// Function enum as a display string (Structural, Insulation, Finish, Other).
    pub function: String,
    /// Axis offset string (fixed number or formula; may be negative).
    pub axis_offset: String,
    /// Bottom vertical offset string (parsed to f64 on save).
    pub bottom_offset: String,
    /// Top vertical offset string (parsed to f64 on save).
    pub top_offset: String,
    /// Optional drawing-layer override (empty string = use default behavior).
    pub layer_override: String,
    /// Optional hatch pattern override (empty string = use material's own hatch).
    pub hatch_override: String,
    /// Optional free-text role tag shown in the manager (empty = none).
    pub role_tag: String,
    /// Stable identity for the layer (kept as-is for existing layers).
    pub layer_id: Option<uuid::Uuid>,
}

/// How the AEC Style Manager orders the "Wall Styles" master list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AecWallStyleSort {
    /// Alphabetically by display name.
    #[default]
    Name,
    /// Parents listed before their children, siblings grouped together.
    Hierarchy,
}

/// A material or wall style currently awaiting an overwrite confirmation
/// during a copy operation between project and global libraries.
#[derive(Clone, Debug)]
pub enum AecPendingCopy {
    Material {
        material: crate::modules::aec::engine::material::Material,
        to_project: bool,
    },
    WallStyle {
        wall_style: crate::modules::aec::engine::wall_style::WallStyle,
        to_project: bool,
    },
}

impl AecWallStyleSort {
    /// Toggles between the two supported orderings.
    pub fn toggled(self) -> Self {
        match self {
            AecWallStyleSort::Name => AecWallStyleSort::Hierarchy,
            AecWallStyleSort::Hierarchy => AecWallStyleSort::Name,
        }
    }

    /// Short label shown on the toggle button.
    pub fn label(self) -> &'static str {
        match self {
            AecWallStyleSort::Name => "Name",
            AecWallStyleSort::Hierarchy => "Hierarchy",
        }
    }
}

/// Target context for the AEC Style Picker modal, identifying which field
/// the selected style/material/layer should be written back to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StylePickerTarget {
    /// Selecting a parent style for a wall style being edited.
    WallStyleParent,
    /// Selecting a material for a specific layer index.
    LayerMaterial(usize),
    /// Selecting a layer override for a specific layer index.
    LayerOverride(usize),
    /// Selecting a new style for one or more existing wall entities in the
    /// properties panel. The actual target handle(s) are kept separately in
    /// `App::aec_style_picker_wall_handles` (more than one handle vector
    /// element when editing a multi-selection) since `ModalKind`/this enum
    /// must stay `Copy`.
    WallPropertiesStyle,
    /// Selecting a new style for the currently active interactive command
    /// (e.g. while drawing a wall).
    ActiveCommand,
}

/// A pending delete in the AEC Project Explorer, awaiting user confirmation
/// before the building/storey is actually removed from the project tree.
/// Identifies targets by their stable `id`, not their list position.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AecProjectExplorerDeleteTarget {
    Building(uuid::Uuid),
    /// `(building_id, storey_id)`.
    Storey(uuid::Uuid, uuid::Uuid),
}

/// In-drawing layer-pair definition at a wall junction.
#[derive(Clone, Debug)]
pub struct AecLayerPairDrawPick {
    pub axis: acadrust::Handle,
    pub end_index: usize,
    pub layer_a: Option<(acadrust::Handle, usize, String)>,
    pub layer_b: Option<(acadrust::Handle, usize, String)>,
    pub layer_b_outer: bool,
    pub hover: Option<(acadrust::Handle, usize)>,
    pub awaiting_style: bool,
}

/// In-drawing layer interruption: pick the layer to cut, then two bounding layers.
#[derive(Clone, Debug)]
pub struct AecLayerGapDrawPick {
    pub axis: acadrust::Handle,
    pub end_index: usize,
    pub layer: Option<(acadrust::Handle, usize, String)>,
    pub from: Option<(acadrust::Handle, usize, String)>,
    pub to: Option<(acadrust::Handle, usize, String)>,
    pub hover: Option<(acadrust::Handle, usize)>,
}


/// All AEC-specific CadApp fields, grouped so core `OpenCADStudio` stays merge-stable.
#[derive(Debug)]
pub struct AecState {
    /// Set once the user acknowledges the AEC-drop warning, so re-entering the
    /// save path proceeds instead of re-showing the warning.
    pub aec_drop_acknowledged: bool,
    /// Number of unsupported objects shown in the AEC-drop warning modal.
    pub aec_drop_count: usize,
    // ── AEC Style Manager ─────────────────────────────────────────────────
    /// Loaded (or seeded) on `AEC_STYLEMANAGER`; holds the materials + wall
    /// styles the manager shell will browse/edit in a later step.
    pub aec_style_library: Option<crate::modules::aec::engine::library::StyleLibrary>,
    /// Session overlay reconstructed from the active drawing (not written to
    /// `aec_styles.toml` or `.ocsproj` unless the user saves explicitly).
    pub aec_session_style_library: Option<crate::modules::aec::engine::library::StyleLibrary>,
    /// Style id of the last wall finished via `AEC_WALL` this session, used
    /// to pre-fill the live Properties-panel style field on the next call.
    pub aec_last_wall_style_id: Option<String>,
    /// Height of the last wall finished via `AEC_WALL` this session, used to
    /// pre-fill the live Properties-panel height field on the next call.
    pub aec_last_wall_height: Option<f64>,
    /// Filter text applied to both the material and wall-style master lists.
    pub aec_style_manager_filter: String,
    /// Currently selected material id (if any), by `Material::id`.
    pub aec_style_manager_selected_material: Option<String>,
    /// Currently selected wall style id (if any), by `Style::id`.
    pub aec_style_manager_selected_wall_style: Option<String>,
    /// Filter text for the AEC Style Picker modal.
    pub aec_style_picker_filter: String,
    /// Currently highlighted ID in the AEC Style Picker (material ID, style ID, or layer name).
    pub aec_style_picker_selection: Option<String>,
    /// Target wall entity handle(s) for `StylePickerTarget::WallPropertiesStyle`
    /// (one per selected wall; more than one when editing a multi-selection).
    pub aec_style_picker_wall_handles: Vec<acadrust::Handle>,
    /// Id of the material currently being edited, if the edit buffer holds
    /// an existing material (`None` while composing a new/unsaved one).
    pub aec_style_manager_material_editing_id: Option<String>,
    /// Whether the material edit form is visible (set by selecting a
    /// material or pressing "New"; cleared on Save/Delete/deselect).
    pub aec_style_manager_material_form_open: bool,
    /// Edit-buffer fields for the material form (name/hatch/color/line type).
    pub aec_style_manager_material_name: String,
    pub aec_style_manager_material_hatch: String,
    pub aec_style_manager_material_color: String,
    pub aec_style_manager_material_line_type: String,
    pub aec_style_manager_material_category: String,
    pub aec_style_manager_material_hatch_color: u32,
    pub aec_style_manager_material_hatch_scale: String,
    pub aec_style_manager_material_render_ref: String,
    pub aec_style_manager_material_hatch_angle: String,
    /// `true` when `hatch_angle` is relative to the wall's own direction,
    /// `false` when it is an absolute/global angle.
    pub aec_style_manager_material_hatch_angle_relative: bool,
    /// Whether the material form's colour-picker popup is expanded.
    pub aec_style_manager_material_color_picker_open: bool,
    /// Whether the material form's hatch-colour-picker popup is expanded.
    pub aec_style_manager_material_hatch_color_picker_open: bool,
    /// Whether the material form's visual hatch-pattern picker is expanded.
    pub aec_style_manager_material_hatch_picker_open: bool,
    /// Line types available for the material form's line-type combo box
    /// (name + ASCII-art preview), refreshed when the manager is opened.
    pub aec_style_manager_material_linetype_items: Vec<crate::ui::properties::LinetypeItem>,
    /// combo_box state built from `aec_style_manager_material_linetype_items`.
    pub aec_style_manager_material_linetype_combo:
        iced::widget::combo_box::State<crate::ui::properties::LinetypeItem>,

    /// Id of the wall style currently being edited, if the edit buffer holds
    /// an existing wall style (`None` while composing a new/unsaved one).
    pub aec_style_manager_wall_style_editing_id: Option<String>,
    /// Whether the wall style edit form is visible.
    pub aec_style_manager_wall_style_form_open: bool,
    /// Edit-buffer fields for the wall style form (name/parent/layers).
    pub aec_style_manager_wall_style_name: String,
    pub aec_style_manager_wall_style_parent: Option<String>,
    pub aec_style_manager_wall_style_layers: Vec<AecLayerBuffer>,
    /// Index of the layer row currently "picked up" for a click-based
    /// drag-and-drop reorder (armed by its drag handle, dropped by clicking
    /// another row's drag handle); `None` when no drag is in progress.
    pub aec_style_manager_wall_style_drag_index: Option<usize>,
    /// How the "Wall Styles" master list is ordered: alphabetically by name,
    /// or hierarchically (parents before children, siblings grouped).
    pub aec_style_manager_wall_style_sort: AecWallStyleSort,
    /// Whether a copy-conflict confirmation dialog is currently open in the
    /// AEC Style Manager.
    pub aec_style_manager_copy_conflict_open: bool,
    /// The material or wall style currently awaiting an overwrite confirmation.
    pub aec_style_manager_pending_copy: Option<AecPendingCopy>,

    // ── AEC Wall Style Manager: Darstellungs-Profile (Step 4) ──────────────
    /// Name of the `DisplayConfig` currently selected in the "Darstellungs-
    /// Profile" table of the wall style form (`None` = none selected).
    pub aec_style_manager_profile_selected: Option<String>,
    /// Layer-Filter-UI: `false` = "Alle" (`LayerSelection::All`), `true` =
    /// "Auswahl", for the currently edited profile's `Contour2D` slot.
    pub aec_style_manager_profile_contour_explicit: bool,
    /// Layer-Filter-UI: edit-buffer of explicitly selected layers for
    /// `Contour2D`, only relevant while the above is `true`.
    pub aec_style_manager_profile_contour_selection:
        Vec<crate::modules::aec::engine::join::LayerRef>,
    /// Same as `..._contour_explicit`, but for the `Solid3D` slot.
    pub aec_style_manager_profile_solid_explicit: bool,
    /// Same as `..._contour_selection`, but for the `Solid3D` slot.
    pub aec_style_manager_profile_solid_selection:
        Vec<crate::modules::aec::engine::join::LayerRef>,
    /// Hatch-angle override text field ("Relativ zur Wand" + Winkel) for
    /// the currently edited profile (empty = no override).
    pub aec_style_manager_profile_hatch_angle: String,
    /// Whether `aec_style_manager_profile_hatch_angle` is relative to the
    /// wall's own run direction (mirrors `ComponentStyleOverride::hatch_angle_relative`).
    pub aec_style_manager_profile_hatch_relative: bool,
    /// Visibility checkboxes for each `WallComponentSlot` in the currently
    /// edited profile (Step 4). Absent keys mean visible (default).
    pub aec_style_manager_profile_slot_visibility:
        std::collections::HashMap<crate::modules::aec::engine::display_component::WallComponentSlot, bool>,
    /// Slot currently open in the per-slot style override editor.
    pub aec_style_manager_profile_editing_slot:
        Option<crate::modules::aec::engine::display_component::WallComponentSlot>,
    /// Pending per-slot style overrides for the selected display profile.
    pub aec_style_manager_profile_slot_overrides: std::collections::HashMap<
        crate::modules::aec::engine::display_component::WallComponentSlot,
        crate::modules::aec::engine::display_component::ComponentStyleOverride,
    >,
    pub aec_style_manager_profile_slot_style_line_type: String,
    pub aec_style_manager_profile_slot_style_line_color: String,
    pub aec_style_manager_profile_slot_style_hatch_pattern: String,
    pub aec_style_manager_profile_slot_style_hatch_color: String,
    pub aec_style_manager_profile_slot_style_fill_color: String,
    pub aec_style_manager_profile_slot_style_line_color_picker_open: bool,
    pub aec_style_manager_profile_slot_style_hatch_color_picker_open: bool,
    pub aec_style_manager_profile_slot_style_fill_color_picker_open: bool,
    pub aec_style_manager_profile_slot_style_hatch_picker_open: bool,
    /// Geometry of the wall-style manager while the display-profiles child
    /// modal is open (Plot → Plotstyle pattern).
    pub aec_wall_style_manager_parent_geometry: Option<(iced::Vector, iced::Vector)>,

    // ── AEC DisplayConfig Manager (Step 5) ─────────────────────────────────
    /// Loaded (or seeded) on `AEC_PLANMANAGER`; holds the `DisplayConfig`
    /// entries the manager browses/edits.
    pub aec_plan_library: Option<crate::modules::aec::engine::library::DisplayConfigLibrary>,
    /// Filter text applied to the DisplayConfig master list.
    pub aec_plan_manager_filter: String,
    /// Currently selected config name (if any), by `DisplayConfig::name`.
    pub aec_plan_manager_selected: Option<String>,
    /// Name of the config currently being edited, if the edit buffer holds
    /// an existing config (`None` while composing a new/unsaved one).
    pub aec_plan_manager_editing_name: Option<String>,
    /// Whether the DisplayConfig edit form is visible.
    pub aec_plan_manager_form_open: bool,
    /// Edit-buffer fields for the DisplayConfig form.
    pub aec_plan_manager_name: String,
    pub aec_plan_manager_discipline: String,
    pub aec_plan_manager_scale: String,
    pub aec_plan_manager_planning_stage: crate::modules::aec::engine::plan_view::PlanningStage,
    pub aec_plan_manager_view_type: crate::modules::aec::engine::plan_view::ViewType,
    /// Two-stage phase-filter editor (Step 5), Stage 1: which
    /// [`crate::modules::aec::engine::plan_view::PlanPhase`]s are visible
    /// for the currently edited config, one bool per phase.
    pub aec_plan_manager_phase_filter_visible_existing: bool,
    pub aec_plan_manager_phase_filter_visible_demolition: bool,
    pub aec_plan_manager_phase_filter_visible_new: bool,
    /// Stage 2: edit-buffer for `PhaseFilter::demolition_style` (hex colors
    /// as `"RRGGBB"` text, mirrors the old Step 8 style-editor fields).
    pub aec_plan_manager_demolition_style_line_type: String,
    pub aec_plan_manager_demolition_style_line_color: String,
    pub aec_plan_manager_demolition_style_hatch_pattern: String,
    pub aec_plan_manager_demolition_style_hatch_color: String,
    pub aec_plan_manager_demolition_style_fill_color: String,
    /// Stage 2: whether each `demolition_style` colour-picker dropdown is open
    /// (mirrors the Layer Manager's `color_selector` widget pattern).
    pub aec_plan_manager_demolition_style_line_color_picker_open: bool,
    pub aec_plan_manager_demolition_style_hatch_color_picker_open: bool,
    pub aec_plan_manager_demolition_style_fill_color_picker_open: bool,
    pub aec_plan_manager_demolition_style_hatch_picker_open: bool,
    /// Stage 2: edit-buffer for `PhaseFilter::existing_style`.
    pub aec_plan_manager_existing_style_line_type: String,
    pub aec_plan_manager_existing_style_line_color: String,
    pub aec_plan_manager_existing_style_hatch_pattern: String,
    pub aec_plan_manager_existing_style_hatch_color: String,
    pub aec_plan_manager_existing_style_fill_color: String,
    /// Stage 2: whether each `existing_style` colour-picker dropdown is open.
    pub aec_plan_manager_existing_style_line_color_picker_open: bool,
    pub aec_plan_manager_existing_style_hatch_color_picker_open: bool,
    pub aec_plan_manager_existing_style_fill_color_picker_open: bool,
    pub aec_plan_manager_existing_style_hatch_picker_open: bool,
    /// Stable `DisplayConfig.id` while editing an existing plan type.
    pub aec_plan_manager_editing_id: Option<uuid::Uuid>,
    pub aec_plan_manager_default_representation:
        crate::modules::aec::engine::display_component::RepresentationMode,
    pub aec_plan_manager_component_visibility: std::collections::HashMap<
        crate::modules::aec::engine::display_component::WallComponentKind,
        bool,
    >,
    pub aec_plan_manager_style_overlays: std::collections::HashMap<
        String,
        crate::modules::aec::engine::display_component::StyleDisplayOverlay,
    >,
    pub aec_plan_manager_overlay_style_id: Option<String>,
    pub aec_plan_manager_overlay_layer_id: Option<uuid::Uuid>,
    pub aec_plan_manager_overlay_line_type: String,
    pub aec_plan_manager_overlay_line_color: String,
    pub aec_plan_manager_overlay_hatch_pattern: String,
    pub aec_plan_manager_overlay_hatch_color: String,
    pub aec_plan_manager_overlay_hatch_scale: String,
    pub aec_plan_manager_overlay_hatch_angle: String,
    /// `None` inherits; `Some(true)` hatch relative to wall run.
    pub aec_plan_manager_overlay_hatch_angle_relative: Option<bool>,
    pub aec_plan_manager_overlay_fill_color: String,
    pub aec_plan_manager_overlay_line_color_picker_open: bool,
    pub aec_plan_manager_overlay_hatch_picker_open: bool,
    pub aec_plan_manager_overlay_hatch_color_picker_open: bool,
    pub aec_plan_manager_overlay_fill_color_picker_open: bool,
    pub aec_plan_manager_contour_hatch_pattern: String,
    pub aec_plan_manager_contour_hatch_color: String,
    pub aec_plan_manager_contour_hatch_scale: String,
    pub aec_plan_manager_contour_hatch_angle: String,
    pub aec_plan_manager_contour_hatch_angle_relative: Option<bool>,
    pub aec_plan_manager_contour_hatch_picker_open: bool,
    pub aec_plan_manager_contour_hatch_color_picker_open: bool,
    pub aec_plan_manager_wall_styles: Vec<(String, String, Vec<(uuid::Uuid, String)>)>,

    // ── AEC Junction Editor Panel (Step 5) ──────────────────────────────────
    /// `(axis_handle, end_index)` of the junction currently open in the
    /// editor panel — the same identity Step 4's context menu uses.
    pub aec_junction_editor_target: Option<(acadrust::Handle, usize)>,
    /// Edit buffer: pending node-level default style for the open junction.
    pub aec_junction_editor_default_style: Option<crate::modules::aec::engine::join::JoinOverrideStyle>,
    /// Edit buffer: pending layer-pair overrides for the open junction.
    pub aec_junction_editor_pairs: Vec<crate::modules::aec::engine::join::LayerPairOverride>,
    /// "Add pair" form: `(layer index, material id)` of `layer_a` (always on
    /// the target wall). The index disambiguates layers that reuse the same
    /// material (e.g. two plaster layers) — matching on material id alone
    /// could never target one specific occurrence.
    pub aec_junction_editor_pair_layer_a: Option<(usize, String)>,
    /// "Add pair" form: the other wall's axis handle chosen for `layer_b`
    /// (`None` = "Außenkante / keine").
    pub aec_junction_editor_pair_wall_b: Option<acadrust::Handle>,
    /// "Add pair" form: `(layer index, material id)` of `layer_b` on the
    /// chosen other wall (see `aec_junction_editor_pair_layer_a`).
    pub aec_junction_editor_pair_layer_b: Option<(usize, String)>,
    /// "Add pair" form: style to apply to the pair being composed.
    pub aec_junction_editor_pair_style: crate::modules::aec::engine::join::JoinOverrideStyle,
    /// Edit buffer: pending layer-gap (interruption) overrides.
    pub aec_junction_editor_gaps: Vec<crate::modules::aec::engine::join::LayerGapOverride>,
    pub aec_junction_editor_gap_layer: Option<(usize, String)>,
    pub aec_junction_editor_gap_from_wall: Option<acadrust::Handle>,
    pub aec_junction_editor_gap_from: Option<(usize, String)>,
    pub aec_junction_editor_gap_to_wall: Option<acadrust::Handle>,
    pub aec_junction_editor_gap_to: Option<(usize, String)>,
    /// In-drawing layer-pair pick at a junction (click layers, then style).
    pub aec_layer_pair_draw: Option<AecLayerPairDrawPick>,
    /// In-drawing layer-gap pick: interrupted layer, then from/to bounds.
    pub aec_layer_gap_draw: Option<AecLayerGapDrawPick>,

    // ── AEC Project Explorer ──────────────────────────────────────────────
    /// Loaded `.ocsproj` contents (if any).
    pub aec_project_explorer_file: Option<crate::modules::aec::engine::project::ProjectFile>,
    /// Path of the loaded/saved `.ocsproj` (used for relative drawing paths).
    pub aec_project_explorer_path: Option<std::path::PathBuf>,
    /// Message to replay after the project-required modal creates or loads a project.
    pub aec_project_required_resume: Option<crate::app::Message>,
    /// Selected building, identified by its stable `id` (not its position —
    /// two buildings can share a name, so only the id disambiguates them).
    pub aec_project_explorer_selected_building: Option<uuid::Uuid>,
    /// Selected storey as `(building_id, storey_id)`.
    pub aec_project_explorer_selected_storey: Option<(uuid::Uuid, uuid::Uuid)>,
    /// "Add Building" name buffer.
    pub aec_project_explorer_new_building_name: String,
    /// "Add Storey" form buffers.
    pub aec_project_explorer_new_storey_name: String,
    pub aec_project_explorer_new_storey_elevation: String,
    pub aec_project_explorer_new_storey_drawing: String,
    /// Live edit buffer for the name of the currently selected building —
    /// only applied to the project when the user presses "Speichern".
    pub aec_project_explorer_edit_building_name: String,
    /// Live edit buffer for the name of the currently selected storey — only
    /// applied to the project when the user presses "Speichern".
    pub aec_project_explorer_edit_storey_name: String,
    /// Live text buffer for the elevation field of the currently selected
    /// storey (kept as text so intermediate typing like "3." isn't rejected).
    /// Only applied to the project when the user presses "Speichern".
    pub aec_project_explorer_edit_elevation: String,
    /// Live edit buffer for the drawing path of the currently selected storey
    /// — only applied to the project when the user presses "Speichern".
    pub aec_project_explorer_edit_storey_drawing: String,
    /// A pending delete awaiting user confirmation (building or storey), so a
    /// misclick on "Delete" cannot silently drop project structure/files.
    pub aec_project_explorer_ffl0_nn: String,
    pub aec_project_explorer_pending_delete: Option<AecProjectExplorerDeleteTarget>,
    pub aec_storey_settings_target: Option<(uuid::Uuid, uuid::Uuid)>,
    pub aec_storey_settings_new_plane_name: String,
    pub aec_storey_settings_new_plane_z: String,
    pub aec_storey_settings_plane_z: std::collections::HashMap<uuid::Uuid, String>,
    pub aec_storey_settings_elevation: String,
    pub aec_storey_settings_height: String,
}

impl Default for AecState {
    fn default() -> Self {
        Self {
            aec_drop_acknowledged: false,
            aec_drop_count: 0,
            aec_style_library: None,
            aec_session_style_library: None,
            aec_last_wall_style_id: None,
            aec_last_wall_height: None,
            aec_style_manager_filter: String::new(),
            aec_style_manager_selected_material: None,
            aec_style_manager_selected_wall_style: None,
            aec_style_picker_filter: String::new(),
            aec_style_picker_selection: None,
            aec_style_picker_wall_handles: Vec::new(),
            aec_style_manager_material_editing_id: None,
            aec_style_manager_material_form_open: false,
            aec_style_manager_material_name: String::new(),
            aec_style_manager_material_hatch: String::new(),
            aec_style_manager_material_color: String::new(),
            aec_style_manager_material_line_type: String::new(),
            aec_style_manager_material_category: String::new(),
            aec_style_manager_material_hatch_color: 0xFFFFFF,
            aec_style_manager_material_hatch_scale: "1.0".to_string(),
            aec_style_manager_material_render_ref: String::new(),
            aec_style_manager_material_hatch_angle: "0.0".to_string(),
            aec_style_manager_material_hatch_angle_relative: true,
            aec_style_manager_material_color_picker_open: false,
            aec_style_manager_material_hatch_color_picker_open: false,
            aec_style_manager_material_hatch_picker_open: false,
            aec_style_manager_material_linetype_items: Vec::new(),
            aec_style_manager_material_linetype_combo: iced::widget::combo_box::State::new(Vec::new()),
            aec_style_manager_wall_style_editing_id: None,
            aec_style_manager_wall_style_form_open: false,
            aec_style_manager_wall_style_name: String::new(),
            aec_style_manager_wall_style_parent: None,
            aec_style_manager_wall_style_layers: Vec::new(),
            aec_style_manager_wall_style_drag_index: None,
            aec_style_manager_wall_style_sort: AecWallStyleSort::default(),
            aec_style_manager_copy_conflict_open: false,
            aec_style_manager_pending_copy: None,
            aec_style_manager_profile_selected: None,
            aec_style_manager_profile_contour_explicit: false,
            aec_style_manager_profile_contour_selection: Vec::new(),
            aec_style_manager_profile_solid_explicit: false,
            aec_style_manager_profile_solid_selection: Vec::new(),
            aec_style_manager_profile_hatch_angle: String::new(),
            aec_style_manager_profile_hatch_relative: false,
            aec_style_manager_profile_slot_visibility: std::collections::HashMap::new(),
            aec_style_manager_profile_editing_slot: None,
            aec_style_manager_profile_slot_overrides: std::collections::HashMap::new(),
            aec_style_manager_profile_slot_style_line_type: String::new(),
            aec_style_manager_profile_slot_style_line_color: String::new(),
            aec_style_manager_profile_slot_style_hatch_pattern: String::new(),
            aec_style_manager_profile_slot_style_hatch_color: String::new(),
            aec_style_manager_profile_slot_style_fill_color: String::new(),
            aec_style_manager_profile_slot_style_line_color_picker_open: false,
            aec_style_manager_profile_slot_style_hatch_color_picker_open: false,
            aec_style_manager_profile_slot_style_fill_color_picker_open: false,
            aec_style_manager_profile_slot_style_hatch_picker_open: false,
            aec_wall_style_manager_parent_geometry: None,
            aec_plan_library: None,
            aec_plan_manager_filter: String::new(),
            aec_plan_manager_selected: None,
            aec_plan_manager_editing_name: None,
            aec_plan_manager_form_open: false,
            aec_plan_manager_name: String::new(),
            aec_plan_manager_discipline: String::new(),
            aec_plan_manager_scale: String::new(),
            aec_plan_manager_planning_stage: crate::modules::aec::engine::plan_view::PlanningStage::Design,
            aec_plan_manager_view_type: crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
            aec_plan_manager_phase_filter_visible_existing: true,
            aec_plan_manager_phase_filter_visible_demolition: true,
            aec_plan_manager_phase_filter_visible_new: true,
            aec_plan_manager_demolition_style_line_type: String::new(),
            aec_plan_manager_demolition_style_line_color: String::new(),
            aec_plan_manager_demolition_style_hatch_pattern: String::new(),
            aec_plan_manager_demolition_style_hatch_color: String::new(),
            aec_plan_manager_demolition_style_fill_color: String::new(),
            aec_plan_manager_demolition_style_line_color_picker_open: false,
            aec_plan_manager_demolition_style_hatch_color_picker_open: false,
            aec_plan_manager_demolition_style_fill_color_picker_open: false,
            aec_plan_manager_demolition_style_hatch_picker_open: false,
            aec_plan_manager_existing_style_line_type: String::new(),
            aec_plan_manager_existing_style_line_color: String::new(),
            aec_plan_manager_existing_style_hatch_pattern: String::new(),
            aec_plan_manager_existing_style_hatch_color: String::new(),
            aec_plan_manager_existing_style_fill_color: String::new(),
            aec_plan_manager_existing_style_line_color_picker_open: false,
            aec_plan_manager_existing_style_hatch_color_picker_open: false,
            aec_plan_manager_existing_style_fill_color_picker_open: false,
            aec_plan_manager_existing_style_hatch_picker_open: false,
            aec_plan_manager_editing_id: None,
            aec_plan_manager_default_representation: crate::modules::aec::engine::display_component::RepresentationMode::All,
            aec_plan_manager_component_visibility: std::collections::HashMap::new(),
            aec_plan_manager_style_overlays: std::collections::HashMap::new(),
            aec_plan_manager_overlay_style_id: None,
            aec_plan_manager_overlay_layer_id: None,
            aec_plan_manager_overlay_line_type: String::new(),
            aec_plan_manager_overlay_line_color: String::new(),
            aec_plan_manager_overlay_hatch_pattern: String::new(),
            aec_plan_manager_overlay_hatch_color: String::new(),
            aec_plan_manager_overlay_hatch_scale: String::new(),
            aec_plan_manager_overlay_hatch_angle: String::new(),
            aec_plan_manager_overlay_hatch_angle_relative: None,
            aec_plan_manager_overlay_fill_color: String::new(),
            aec_plan_manager_overlay_line_color_picker_open: false,
            aec_plan_manager_overlay_hatch_picker_open: false,
            aec_plan_manager_overlay_hatch_color_picker_open: false,
            aec_plan_manager_overlay_fill_color_picker_open: false,
            aec_plan_manager_contour_hatch_pattern: String::new(),
            aec_plan_manager_contour_hatch_color: String::new(),
            aec_plan_manager_contour_hatch_scale: String::new(),
            aec_plan_manager_contour_hatch_angle: String::new(),
            aec_plan_manager_contour_hatch_angle_relative: None,
            aec_plan_manager_contour_hatch_picker_open: false,
            aec_plan_manager_contour_hatch_color_picker_open: false,
            aec_plan_manager_wall_styles: Vec::new(),
            aec_junction_editor_target: None,
            aec_junction_editor_default_style: None,
            aec_junction_editor_pairs: Vec::new(),
            aec_junction_editor_pair_layer_a: None,
            aec_junction_editor_pair_wall_b: None,
            aec_junction_editor_pair_layer_b: None,
            aec_junction_editor_pair_style: crate::modules::aec::engine::join::JoinOverrideStyle::Miter,
            aec_junction_editor_gaps: Vec::new(),
            aec_junction_editor_gap_layer: None,
            aec_junction_editor_gap_from_wall: None,
            aec_junction_editor_gap_from: None,
            aec_junction_editor_gap_to_wall: None,
            aec_junction_editor_gap_to: None,
            aec_layer_pair_draw: None,
            aec_layer_gap_draw: None,
            aec_project_explorer_file: None,
            aec_project_explorer_path: None,
            aec_project_required_resume: None,
            aec_project_explorer_selected_building: None,
            aec_project_explorer_selected_storey: None,
            aec_project_explorer_new_building_name: String::new(),
            aec_project_explorer_new_storey_name: String::new(),
            aec_project_explorer_new_storey_elevation: String::from("0.0"),
            aec_project_explorer_new_storey_drawing: String::new(),
            aec_project_explorer_edit_building_name: String::new(),
            aec_project_explorer_edit_storey_name: String::new(),
            aec_project_explorer_edit_elevation: String::new(),
            aec_project_explorer_edit_storey_drawing: String::new(),
            aec_project_explorer_ffl0_nn: String::new(),
            aec_project_explorer_pending_delete: None,
            aec_storey_settings_target: None,
            aec_storey_settings_new_plane_name: String::new(),
            aec_storey_settings_new_plane_z: String::new(),
            aec_storey_settings_plane_z: std::collections::HashMap::new(),
            aec_storey_settings_elevation: String::new(),
            aec_storey_settings_height: String::new(),

        }
    }
}
