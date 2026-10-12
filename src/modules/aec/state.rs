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
    OpeningStyle {
        opening_style: crate::modules::aec::engine::opening_style::OpeningStyle,
        to_project: bool,
    },
    SlabStyle {
        slab_style: crate::modules::aec::engine::slab_style::SlabStyle,
        to_project: bool,
    },
    SlabStructuralStyle {
        slab_structural_style: crate::modules::aec::engine::slab_style::SlabStructuralStyle,
        to_project: bool,
    },
    FloorFinishStyle {
        floor_finish_style: crate::modules::aec::engine::slab_style::FloorFinishStyle,
        to_project: bool,
    },
}

/// Preview mode in the Wall Style Manager canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AecWallPreviewMode {
    /// 2D layer buildup (cross-section).
    #[default]
    CrossSection2D,
    Model3D,
}

/// Preview mode in the Slab Style Manager canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AecSlabPreviewMode {
    #[default]
    CrossSection,
    ReflectedCeilingPlan,
    Model3D,
}

/// Edit-buffer row for a slab style layer.
#[derive(Clone, Debug, PartialEq)]
pub struct AecSlabLayerBuffer {
    pub material_id: String,
    pub thickness: String,
    pub function: String,
    pub vertical_offset: String,
    pub layer_override: String,
    pub hatch_override: String,
    pub role_tag: String,
    pub layer_id: Option<uuid::Uuid>,
}

/// Geometry source for one opening-style slot in the manager form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AecOpeningSlotSource {
    Generator,
    Block,
    Sketch,
}

/// Preview mode in the Opening Style Manager canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AecOpeningPreviewMode {
    #[default]
    Plan2D,
    Elevation2D,
    Model3D,
}

/// Edit-buffer row for an opening-style display slot.
#[derive(Clone, Debug)]
pub struct AecOpeningSlotBuffer {
    pub slot: crate::modules::aec::engine::display_component::OpeningComponentSlot,
    pub source: AecOpeningSlotSource,
    pub generator: crate::modules::aec::engine::opening_style::OpeningGenerator,
    pub block_name: String,
    pub block_placement: crate::modules::aec::engine::opening_style::BlockPlacementMode,
    pub sketch: crate::modules::aec::engine::opening_style::OpeningSketch,
    pub visible: bool,
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
    /// Selecting a parent style for an opening style being edited.
    OpeningStyleParent,
    /// Selecting a new style for one or more opening entities in the
    /// properties panel. Handles live in `aec_style_picker_wall_handles`.
    OpeningPropertiesStyle,
    /// Selecting a parent style for a slab style being edited.
    SlabStyleParent,
    /// Selecting a parent style for a structural slab style being edited.
    SlabStructuralStyleParent,
    /// Selecting a material for a specific slab layer index.
    SlabLayerMaterial(usize),
    /// Selecting a material for a specific structural slab layer index.
    SlabStructuralLayerMaterial(usize),
    /// Selecting a layer override for a specific structural slab layer index.
    SlabStructuralLayerOverride(usize),
    /// Selecting a layer override for a specific slab layer index.
    SlabLayerOverride(usize),
    /// Selecting a parent style for a floor finish style being edited.
    FloorFinishStyleParent,
    /// Selecting a material for a specific floor finish layer index.
    FloorFinishLayerMaterial(usize),
    /// Selecting a layer override for a specific floor finish layer index.
    FloorFinishLayerOverride(usize),
    /// Selecting a new style for one or more slab entities in the properties panel.
    SlabPropertiesStyle,
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
    /// All `AEC_STYLES` window state (selection, filters, form buffers).
    pub styles: crate::modules::aec::ui::style_manager::AecStylesUiState,
    /// Filter text for the AEC Style Picker modal.
    pub aec_style_picker_filter: String,
    /// Currently highlighted ID in the AEC Style Picker (material ID, style ID, or layer name).
    pub aec_style_picker_selection: Option<String>,
    /// Target wall entity handle(s) for `StylePickerTarget::WallPropertiesStyle`
    /// (one per selected wall; more than one when editing a multi-selection).
    pub aec_style_picker_wall_handles: Vec<acadrust::Handle>,
    /// Cache of last applied AEC property target (handle, field, val) to dedup
    /// batch-invocations on package child entities in the properties panel.
    pub aec_last_applied_property: Option<(acadrust::Handle, String, String)>,
    pub aec_last_slab_style_id: Option<String>,
    pub aec_last_slab_structural_style_id: Option<String>,
    pub aec_last_floor_finish_style_id: Option<String>,

    // ── AEC DisplayConfig Manager (Step 5) ─────────────────────────────────
    /// Loaded (or seeded) on `AEC_PLANMANAGER`; holds the `DisplayConfig`
    /// entries the manager browses/edits.
    pub aec_plan_library: Option<crate::modules::aec::engine::library::DisplayConfigLibrary>,

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
    pub aec_storey_settings_facet_z: std::collections::HashMap<(uuid::Uuid, usize), String>,
    pub aec_storey_settings_elevation: String,
    pub aec_storey_settings_height: String,
    pub aec_control_plane_highlight_epoch: u64,
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
            styles: Default::default(),
            aec_style_picker_filter: String::new(),
            aec_style_picker_selection: None,
            aec_style_picker_wall_handles: Vec::new(),
            aec_last_applied_property: None,
            aec_last_slab_style_id: None,
            aec_last_slab_structural_style_id: None,
            aec_last_floor_finish_style_id: None,
            aec_plan_library: None,
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
            aec_storey_settings_facet_z: std::collections::HashMap::new(),
            aec_storey_settings_elevation: String::new(),
            aec_storey_settings_height: String::new(),
            aec_control_plane_highlight_epoch: 0,
        }
    }
}
