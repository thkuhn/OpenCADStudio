//! AEC iced messages and the single core update hook.

use super::state::StylePickerTarget;

#[derive(Debug, Clone)]
pub enum AecMessage {
    // ── AEC Style Manager (`AEC_STYLEMANAGER`) ───────────────────────────
    /// Load (or seed) the AEC style library into app state and open the
    /// manager modal shell.
    AecMaterialManagerOpen,
    AecWallStyleManagerOpen,
    /// Filter text changed in the AEC Style Manager's master lists.
    AecStyleManagerFilter(String),
    /// A material row was selected in the AEC Style Manager.
    AecStyleManagerSelectMaterial(String),
    /// A wall style row was selected in the AEC Style Manager.
    AecStyleManagerSelectWallStyle(String),
    /// "New" pressed in the material panel — opens a blank material edit form.
    AecStyleManagerMaterialNew,
    /// Name field changed in the material edit form.
    AecStyleManagerMaterialNameChanged(String),
    /// Hatch-pattern field changed in the material edit form.
    AecStyleManagerMaterialHatchChanged(String),
    /// Opens/closes the material edit form's visual hatch-pattern picker.
    AecStyleManagerMaterialHatchPickerToggle,
    /// A hatch pattern was chosen in the material edit form's visual picker.
    AecStyleManagerMaterialHatchSelected(String),
    /// Line-color hex field changed in the material edit form.
    AecStyleManagerMaterialColorChanged(String),
    /// Line-type field changed in the material edit form.
    AecStyleManagerMaterialLineTypeChanged(String),
    /// Opens/closes the material edit form's colour-picker popup.
    AecStyleManagerMaterialColorPickerToggle,
    /// A colour was chosen in the material edit form's colour picker.
    AecStyleManagerMaterialColorPicked(acadrust::types::Color),
    /// Category field changed in the material edit form.
    AecStyleManagerMaterialCategoryChanged(String),
    /// Hatch colour changed in the material edit form.
    AecStyleManagerMaterialHatchColorChanged(u32),
    /// Opens/closes the material edit form's hatch-colour-picker popup.
    AecStyleManagerMaterialHatchColorPickerToggle,
    /// Hatch-scale field changed in the material edit form.
    AecStyleManagerMaterialHatchScaleChanged(String),
    /// Render-material-ref field changed in the material edit form.
    AecStyleManagerMaterialRenderRefChanged(String),
    /// Hatch-angle field changed in the material edit form.
    AecStyleManagerMaterialHatchAngleChanged(String),
    /// Toggles whether the hatch angle is relative to the wall direction or
    /// an absolute/global angle.
    AecStyleManagerMaterialHatchAngleRelativeToggle,
    /// "Duplicate" pressed — clones the selected material into a new unsaved form.
    AecStyleManagerMaterialDuplicate,
    /// "Save" pressed in the material edit form — upserts and persists.
    AecStyleManagerMaterialSave,
    /// "Delete" pressed for the currently selected material.
    AecStyleManagerMaterialDelete,
    /// Copy the currently selected material from global library to project library.
    AecStyleManagerCopyMaterialToProject,
    /// Copy the currently selected material from project library to global library.
    AecStyleManagerCopyMaterialToGlobal,

    AecStyleManagerWallStyleNew,
    AecStyleManagerWallStyleNameChanged(String),
    AecStyleManagerWallStyleParentChanged(Option<String>),
    AecStyleManagerWallStyleLayerAdd,
    AecStyleManagerWallStyleLayerRemove(usize),
    AecStyleManagerWallStyleLayerMaterialChanged(usize, String),
    AecStyleManagerWallStyleLayerThicknessChanged(usize, String),
    AecStyleManagerWallStyleLayerFunctionChanged(usize, String),
    /// Horizontal gap-before field changed for the layer at `index`.
    AecStyleManagerWallStyleLayerAxisOffsetChanged(usize, String),
    /// Bottom vertical offset field changed for the layer at `index`.
    AecStyleManagerWallStyleLayerBottomOffsetChanged(usize, String),
    /// Top vertical offset field changed for the layer at `index`.
    AecStyleManagerWallStyleLayerTopOffsetChanged(usize, String),
    /// Optional drawing-layer override changed for the layer at `index`
    /// (empty string = use default behavior).
    AecStyleManagerWallStyleLayerOverrideChanged(usize, String),
    /// Optional hatch pattern override changed for the layer at `index`
    /// (empty string = use the material's own hatch pattern).
    AecStyleManagerWallStyleLayerHatchOverrideChanged(usize, String),
    /// Optional free-text role tag changed for the layer at `index`.
    AecStyleManagerWallStyleLayerRoleTagChanged(usize, String),
    /// Moves the layer at `index` one position up (towards the outside).
    AecStyleManagerWallStyleLayerMoveUp(usize),
    /// Moves the layer at `index` one position down (towards the inside).
    AecStyleManagerWallStyleLayerMoveDown(usize),
    /// Drag handle pressed on the layer row at `index` — arms a pending
    /// reorder ("pick up"); a click on a different row's handle drops it
    /// there. This is a click-based approximation of drag-and-drop, since
    /// `iced`'s `mouse_area` in this version has no hover/enter callbacks
    /// suitable for continuous drag tracking.
    AecStyleManagerWallStyleLayerDragStart(usize),
    /// Drop target clicked while a layer is armed for reordering — moves
    /// the armed layer to this index and disarms.
    AecStyleManagerWallStyleLayerDragOver(usize),
    /// Cancels an armed drag-reorder without moving anything.
    AecStyleManagerWallStyleLayerDragEnd,
    AecStyleManagerWallStyleSave,
    AecStyleManagerWallStyleSaveAndApply,
    AecStyleManagerWallStyleDelete,
    /// Copy the currently selected wall style from global library to project library.
    AecStyleManagerCopyWallStyleToProject,
    /// Copy the currently selected wall style from project library to global library.
    AecStyleManagerCopyWallStyleToGlobal,
    /// User's yes/no answer to an overwrite confirmation dialog during AEC Style copy.
    AecStyleManagerCopyConflictConfirm(bool),
    /// Toggles the "Wall Styles" master-list ordering between Name/Hierarchy.
    AecStyleManagerWallStyleSortToggle,
    /// Open the AEC Style Picker for a specific target.
    AecStylePickerOpen(StylePickerTarget),
    /// Open the AEC Style Picker for the `WallPropertiesStyle` target,
    /// carrying the (one or more) wall entity handles to write the picked
    /// style back to.
    AecStylePickerOpenForWallProperties(Vec<acadrust::Handle>),
    /// Open the AEC Style Picker for the currently active interactive command.
    AecStylePickerOpenForActiveCommand,
    /// Live search filter change in the AEC Style Picker.
    AecStylePickerFilterChanged(String),
    /// Selection/highlight change in the AEC Style Picker.
    AecStylePickerSelect(String),
    /// Confirm selection in the AEC Style Picker.
    AecStylePickerConfirm,
    /// Cancel the AEC Style Picker without applying a selection. For
    /// targets opened from within the Wall Style Manager (parent style,
    /// layer material, layer override) this returns to the manager instead
    /// of closing the modal entirely.
    AecStylePickerCancel,

    // ── AEC Wall Style Manager: Darstellungs-Profile (Step 4) ────────────
    /// A `DisplayConfig` row was selected in the "Darstellungs-Profile"
    /// table of the currently edited wall style — loads its
    /// `ComponentRuleSet` (if any) into the profile edit buffers.
    AecStyleManagerProfileSelect(String),
    /// "Alle Schichten" / "Auswahl" toggle for the `Contour2D` slot.
    AecStyleManagerProfileContourModeToggle(bool),
    /// "Alle Schichten" / "Auswahl" toggle for the `Solid3D` slot.
    AecStyleManagerProfileSolidModeToggle(bool),
    /// A layer checkbox was toggled in the `Contour2D` explicit selection.
    AecStyleManagerProfileContourLayerToggle(crate::modules::aec::engine::join::LayerRef),
    /// A layer checkbox was toggled in the `Solid3D` explicit selection.
    AecStyleManagerProfileSolidLayerToggle(crate::modules::aec::engine::join::LayerRef),
    /// "Relativ zur Wand" checkbox for the profile's hatch-angle override.
    AecStyleManagerProfileHatchRelativeToggle(bool),
    /// Hatch-angle text field for the profile's hatch-angle override.
    AecStyleManagerProfileHatchAngleChanged(String),
    /// Visibility toggle for a specific `WallComponentSlot` (Step 4).
    AecStyleManagerProfileSlotVisibilityToggle(
        crate::modules::aec::engine::display_component::WallComponentSlot,
        bool,
    ),
    /// Open the per-slot style override editor for `slot`.
    AecStyleManagerProfileSlotStyleOpen(
        crate::modules::aec::engine::display_component::WallComponentSlot,
    ),
    AecStyleManagerProfileSlotStyleLineTypeChanged(String),
    AecStyleManagerProfileSlotStyleLineColorChanged(String),
    AecStyleManagerProfileSlotStyleLineColorPickerToggle,
    AecStyleManagerProfileSlotStyleHatchPatternChanged(String),
    AecStyleManagerProfileSlotStyleHatchPickerToggle,
    AecStyleManagerProfileSlotStyleHatchColorChanged(String),
    AecStyleManagerProfileSlotStyleHatchColorPickerToggle,
    AecStyleManagerProfileSlotStyleFillColorChanged(String),
    AecStyleManagerProfileSlotStyleFillColorPickerToggle,
    /// Apply the slot-style editor buffers into the pending override map.
    AecStyleManagerProfileSlotStyleApply,
    /// Clear any pending override for the currently edited slot.
    AecStyleManagerProfileSlotStyleClear,
    /// Close the slot-style editor without changing the pending map.
    AecStyleManagerProfileSlotStyleClose,
    /// Open the plan-type display-profiles child modal from the wall style manager.
    AecWallStyleManagerDisplayProfilesOpen,
    /// Close the plan-type display-profiles child modal and restore the wall
    /// style manager (Plotstyle → Plot pattern).
    AecWallStyleManagerDisplayProfilesClose,
    /// Saves the currently edited profile's `ComponentRuleSet` into the
    /// wall style's `display_profiles[selected_config_name]`.
    AecStyleManagerProfileSave,
    /// Removes the currently selected `DisplayConfig`'s override entirely,
    /// reverting that plan type back to the style's default representation.
    AecStyleManagerProfileRemove,

    // ── AEC Project Explorer (`AEC_PROJECTEXPLORER`) ──────────────────────
    /// Open the Project Explorer modal.
    AecProjectExplorerOpen,
    /// Start a blank in-memory project (does not write until Save).
    AecProjectExplorerNew,
    /// Pick and load a `.ocsproj` file.
    AecProjectExplorerLoad,
    /// Result of the project-file open dialog (`None` = cancelled).
    AecProjectExplorerLoadResult(Option<std::path::PathBuf>),
    /// Save the current project to its known path (or Save As if none).
    AecProjectExplorerSave,
    /// Pick a path and save the current project.
    AecProjectExplorerSaveAs,
    /// Result of the project-file save dialog (`None` = cancelled).
    AecProjectExplorerSaveAsResult(Option<std::path::PathBuf>),
    /// Building row selected in the explorer tree, by its stable id.
    AecProjectExplorerSelectBuilding(uuid::Uuid),
    /// Storey row selected in the explorer tree, as `(building_id, storey_id)`.
    AecProjectExplorerSelectStorey(uuid::Uuid, uuid::Uuid),
    /// "Open" pressed on a storey row — open its drawing in a new tab.
    AecProjectExplorerOpenStorey(uuid::Uuid, uuid::Uuid),
    /// Append a building using the name buffer.
    AecProjectExplorerAddBuilding,
    /// Live-edit the name buffer for the building with this id (not yet applied).
    AecProjectExplorerEditBuildingName(uuid::Uuid, String),
    /// Apply the building-name edit buffer to the project and persist it.
    AecProjectExplorerSaveBuildingEdits(uuid::Uuid),
    /// Live-edit the name buffer for the storey at `(building_id, storey_id)` (not yet applied).
    AecProjectExplorerEditStoreyName(uuid::Uuid, uuid::Uuid, String),
    /// Live-edit the elevation text buffer of the storey at `(building_id, storey_id)` (not yet applied).
    AecProjectExplorerEditStoreyElevation(uuid::Uuid, uuid::Uuid, String),
    /// Live-edit the drawing path buffer of the storey at `(building_id, storey_id)` (not yet applied).
    AecProjectExplorerEditStoreyDrawing(uuid::Uuid, uuid::Uuid, String),
    /// Apply the storey name/elevation/drawing edit buffers to the project and persist them.
    AecProjectExplorerSaveStoreyEdits(uuid::Uuid, uuid::Uuid),
    /// Ask for confirmation before deleting the building with this id (and
    /// all its storeys) — sets the pending-delete state shown inline.
    AecProjectExplorerRequestDeleteBuilding(uuid::Uuid),
    /// Ask for confirmation before deleting the storey at
    /// `(building_id, storey_id)`.
    AecProjectExplorerRequestDeleteStorey(uuid::Uuid, uuid::Uuid),
    /// User confirmed the pending delete — actually remove it.
    AecProjectExplorerConfirmDelete,
    /// User cancelled the pending delete — clear it without changes.
    AecProjectExplorerCancelDelete,
    /// Append a storey to the selected building using the form buffers.
    AecProjectExplorerAddStorey,
    AecProjectExplorerNewBuildingNameChanged(String),
    AecProjectExplorerNewStoreyNameChanged(String),
    AecProjectExplorerNewStoreyElevationChanged(String),
    AecProjectExplorerNewStoreyDrawingChanged(String),
    /// Pick a drawing path for the new-storey form.
    AecProjectExplorerPickStoreyDrawing,
    AecProjectExplorerPickStoreyDrawingResult(Option<std::path::PathBuf>),
    /// Pick a drawing path while editing the storey at `(building_id, storey_id)`
    /// — writes the result into the edit buffer, not directly into the project.
    AecProjectExplorerPickEditStoreyDrawing(uuid::Uuid, uuid::Uuid),
    AecProjectExplorerPickEditStoreyDrawingResult(Option<std::path::PathBuf>),
    /// "Bibliotheken migrieren" — copies the current global material/wall-
    /// style and `DisplayConfig` libraries into the loaded project (Step 6:
    /// "Projektweite Bibliotheks-Persistenz"), then persists the project.
    AecProjectExplorerFfl0NnChanged(String),
    AecProjectExplorerMigrateLibraries,
    AecStoreySettingsOpen(uuid::Uuid, uuid::Uuid),
    AecStoreySettingsClose,
    AecStoreySettingsNameChanged(uuid::Uuid, uuid::Uuid, String),
    AecStoreySettingsDrawingChanged(uuid::Uuid, uuid::Uuid, String),
    AecStoreySettingsSetFloor(uuid::Uuid, uuid::Uuid, uuid::Uuid),
    AecStoreySettingsSetCeiling(uuid::Uuid, uuid::Uuid, uuid::Uuid),
    AecStoreySettingsElevation(uuid::Uuid, uuid::Uuid, String),
    AecStoreySettingsHeight(uuid::Uuid, uuid::Uuid, String),
    AecStoreySettingsNewPlaneNameChanged(String),
    AecStoreySettingsNewPlaneZChanged(String),
    AecStoreySettingsAddPlane(uuid::Uuid, uuid::Uuid),
    AecStoreySettingsDeletePlane(uuid::Uuid, uuid::Uuid, uuid::Uuid),
    AecStoreySettingsPlaneName(uuid::Uuid, uuid::Uuid, uuid::Uuid, String),
    AecStoreySettingsPlaneVisible(uuid::Uuid, uuid::Uuid, uuid::Uuid, bool),
    AecStoreySettingsPlaneZ(uuid::Uuid, uuid::Uuid, uuid::Uuid, String),
    AecStoreySettingsPlaneOrigin(uuid::Uuid, uuid::Uuid, uuid::Uuid, u8, String),
    AecStoreySettingsPlaneNormal(uuid::Uuid, uuid::Uuid, uuid::Uuid, u8, String),

    // ── AEC DisplayConfig Manager (`AEC_PLANMANAGER`, Step 5) ─────────────
    /// Open the DisplayConfig Manager modal.
    AecPlanManagerOpen,
    /// Close the DisplayConfig Manager modal.
    AecPlanManagerClose,
    /// Filter text changed in the DisplayConfig master list.
    AecPlanManagerFilter(String),
    /// A config row was selected in the DisplayConfig Manager.
    AecPlanManagerSelect(String),
    /// "New" pressed — opens a blank DisplayConfig edit form.
    AecPlanManagerNew,
    /// "Duplizieren" pressed on the currently edited/selected config.
    AecPlanManagerDuplicate,
    /// "Löschen" pressed on the currently selected config.
    AecPlanManagerDelete,
    /// Name field changed in the DisplayConfig edit form.
    AecPlanManagerNameChanged(String),
    /// Discipline field changed in the DisplayConfig edit form.
    AecPlanManagerDisciplineChanged(String),
    /// Scale (informative) field changed in the DisplayConfig edit form.
    AecPlanManagerScaleChanged(String),
    /// Planning stage field changed in the DisplayConfig edit form.
    AecPlanManagerPlanningStageChanged(crate::modules::aec::engine::plan_view::PlanningStage),
    /// View-type field changed in the DisplayConfig edit form.
    AecPlanManagerViewTypeChanged(crate::modules::aec::engine::plan_view::ViewType),
    /// Two-stage phase-filter editor, Stage 1: a phase's visibility
    /// checkbox was toggled in the DisplayConfig edit form.
    AecPlanManagerPhaseVisibleToggle(crate::modules::aec::engine::plan_view::PlanPhase, bool),
    /// Stage 2: `demolition_style` line-type field changed.
    AecPlanManagerDemolitionStyleLineTypeChanged(String),
    /// Stage 2: `demolition_style` line-color field (hex `"RRGGBB"`) changed.
    AecPlanManagerDemolitionStyleLineColorChanged(String),
    /// Stage 2: `demolition_style` hatch-pattern field changed.
    AecPlanManagerDemolitionStyleHatchPatternChanged(String),
    AecPlanManagerDemolitionStyleHatchPickerToggle,
    /// Stage 2: `demolition_style` hatch-color field (hex `"RRGGBB"`) changed.
    AecPlanManagerDemolitionStyleHatchColorChanged(String),
    /// Stage 2: `demolition_style` fill-color field (hex `"RRGGBB"`) changed.
    AecPlanManagerDemolitionStyleFillColorChanged(String),
    /// Stage 2: `existing_style` line-type field changed.
    AecPlanManagerExistingStyleLineTypeChanged(String),
    /// Stage 2: `existing_style` line-color field (hex `"RRGGBB"`) changed.
    AecPlanManagerExistingStyleLineColorChanged(String),
    /// Stage 2: `existing_style` hatch-pattern field changed.
    AecPlanManagerExistingStyleHatchPatternChanged(String),
    AecPlanManagerExistingStyleHatchPickerToggle,
    /// Stage 2: `existing_style` hatch-color field (hex `"RRGGBB"`) changed.
    AecPlanManagerExistingStyleHatchColorChanged(String),
    /// Stage 2: `existing_style` fill-color field (hex `"RRGGBB"`) changed.
    AecPlanManagerExistingStyleFillColorChanged(String),
    /// Stage 2: toggles the `demolition_style` line-color picker dropdown.
    AecPlanManagerDemolitionStyleLineColorPickerToggle,
    /// Stage 2: toggles the `demolition_style` hatch-color picker dropdown.
    AecPlanManagerDemolitionStyleHatchColorPickerToggle,
    /// Stage 2: toggles the `demolition_style` fill-color picker dropdown.
    AecPlanManagerDemolitionStyleFillColorPickerToggle,
    /// Stage 2: toggles the `existing_style` line-color picker dropdown.
    AecPlanManagerExistingStyleLineColorPickerToggle,
    /// Stage 2: toggles the `existing_style` hatch-color picker dropdown.
    AecPlanManagerExistingStyleHatchColorPickerToggle,
    /// Stage 2: toggles the `existing_style` fill-color picker dropdown.
    AecPlanManagerExistingStyleFillColorPickerToggle,
    AecPlanManagerRepresentationChanged(
        crate::modules::aec::engine::display_component::RepresentationMode,
    ),
    AecPlanManagerComponentVisibleToggle(
        crate::modules::aec::engine::display_component::WallComponentKind,
        bool,
    ),
    AecPlanManagerOverlayStyleSelect(String),
    AecPlanManagerOverlayLayerSelect(uuid::Uuid),
    AecPlanManagerOverlayAddStyle(String),
    AecPlanManagerOverlayRemoveStyle,
    AecPlanManagerOverlayLineTypeChanged(String),
    AecPlanManagerOverlayLineColorChanged(String),
    AecPlanManagerOverlayHatchPatternChanged(String),
    AecPlanManagerOverlayHatchColorChanged(String),
    AecPlanManagerOverlayHatchScaleChanged(String),
    AecPlanManagerOverlayHatchAngleChanged(String),
    AecPlanManagerOverlayHatchAngleRelativeChanged(Option<bool>),
    AecPlanManagerOverlayFillColorChanged(String),
    AecPlanManagerOverlayLineColorPickerToggle,
    AecPlanManagerOverlayHatchPickerToggle,
    AecPlanManagerOverlayHatchColorPickerToggle,
    AecPlanManagerOverlayFillColorPickerToggle,
    AecPlanManagerContourHatchPatternChanged(String),
    AecPlanManagerContourHatchColorChanged(String),
    AecPlanManagerContourHatchScaleChanged(String),
    AecPlanManagerContourHatchAngleChanged(String),
    AecPlanManagerContourHatchAngleRelativeChanged(Option<bool>),
    AecPlanManagerContourHatchPickerToggle,
    AecPlanManagerContourHatchColorPickerToggle,
    AecPlanManagerOverlayLayerVis2d(bool),
    AecPlanManagerOverlayLayerVis3d(bool),
    /// Apply pressed — persists the edit buffer to the library and,
    /// if the config being edited is the active tab's active DisplayConfig,
    /// re-applies it to the active scene's walls.
    AecPlanManagerApply,
    /// The active-DisplayConfig dropdown selected a config by name for the
    /// active document tab; immediately regenerates the tab's walls.
    AecActiveDisplayConfigSelected(Option<String>),
    /// Status-bar 2D/3D/All. `None` inherits the plan-type default.
    AecRepresentationOverrideSelected(
        Option<crate::modules::aec::engine::display_component::RepresentationMode>,
    ),
    // ── Wall Junction context menu ──────────────────────────────────────
    /// Toggle the Wall Junction sub-items (Miter/Butt/Außenkante/Automatisch/
    /// Detailansicht...) in the viewport context menu.
    WallJunctionSubmenuToggle,
    /// Set the node-level default join style for the wall junction the
    /// viewport context menu is currently anchored on.
    WallJunctionOverrideSetStyle(crate::modules::aec::engine::join::JoinOverrideStyle),
    /// Remove the junction override for the wall junction the viewport
    /// context menu is currently anchored on, restoring automatic resolution.
    WallJunctionOverrideReset,
    // ── AEC Junction Editor Panel (Step 5) ──────────────────────────────
    /// Open the Junction Editor Panel for the given `(axis_handle,
    /// end_index)`, loading all participating walls/layers and the current
    /// `JunctionOverride` (if any) into the edit buffers.
    AecJunctionEditorOpen(acadrust::Handle, usize),
    /// Close the panel without saving pending edits.
    AecJunctionEditorClose,
    /// Set the pending node-level default style in the panel's edit buffer.
    AecJunctionEditorSetDefaultStyle(crate::modules::aec::engine::join::JoinOverrideStyle),
    /// Reset the pending node-level default style back to "Automatisch".
    AecJunctionEditorResetDefaultStyle,
    /// The "add pair" form's `layer_a` choice changed: `(layer index,
    /// material id)` — the index is required to disambiguate layers that
    /// reuse the same material.
    AecJunctionEditorPairLayerAChanged(usize, String),
    /// The "add pair" form's other-wall choice changed (`None` = Außenkante).
    AecJunctionEditorPairWallBChanged(Option<acadrust::Handle>),
    /// The "add pair" form's `layer_b` choice changed: `(layer index,
    /// material id)` (see `AecJunctionEditorPairLayerAChanged`).
    AecJunctionEditorPairLayerBChanged(usize, String),
    /// The "add pair" form's style choice changed.
    AecJunctionEditorPairStyleChanged(crate::modules::aec::engine::join::JoinOverrideStyle),
    /// Commit the "add pair" form as a new `LayerPairOverride` in the edit
    /// buffer (not yet persisted — Save writes the whole `JunctionOverride`).
    AecJunctionEditorAddPair,
    /// Change the join style of an existing layer-pair override in the edit
    /// buffer without removing the pair.
    AecJunctionEditorSetPairStyle(usize, crate::modules::aec::engine::join::JoinOverrideStyle),
    /// Remove the layer-pair override at this index from the edit buffer.
    AecJunctionEditorRemovePair(usize),
    AecJunctionEditorGapLayerChanged(usize, String),
    AecJunctionEditorGapFromWallChanged(Option<acadrust::Handle>),
    AecJunctionEditorGapFromChanged(usize, String),
    AecJunctionEditorGapToWallChanged(Option<acadrust::Handle>),
    AecJunctionEditorGapToChanged(usize, String),
    AecJunctionEditorAddGap,
    AecJunctionEditorRemoveGap(usize),
    /// Persist the edit buffer as the complete `JunctionOverride` via
    /// `write_junction_override`, then refresh the wall and close the panel.
    AecJunctionEditorSave,
    /// Start in-drawing layer-pair definition at `(axis_handle, end_index)`.
    AecJunctionLayerPairPickStart(acadrust::Handle, usize),
    /// Cancel in-drawing layer-pair definition.
    AecJunctionLayerPairPickCancel,
    /// Apply the join style to the pair just picked in the drawing.
    AecJunctionLayerPairSetStyle(crate::modules::aec::engine::join::JoinOverrideStyle),
    AecJunctionLayerGapPickStart(acadrust::Handle, usize),
    /// Remove the entire override for the open junction (same effect as
    /// Step 4's context-menu reset) and close the panel.
    AecJunctionEditorFullReset,
}


