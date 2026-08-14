---
sessionId: session-260812-011701-iuuy
---

# Requirements

### Overview & Goals
Two issues reported after testing the AEC Style Manager (branch `feature/aec-core-module`):

1. **Bug**: Editing a wall style and clicking "Save" reports "Updated N wall(s)" in the command line, but the drawing itself does not visually change.
2. **UX restructuring request**: Split the combined "AEC Style Manager" dialog into two separate manager dialogs — one for Materials, one for Wall Styles. The Wall Style manager should show the wall styles as a hierarchical list, the layer table should use the full available dialog width without needing to scroll, and the "Save" button should sit bottom-right of the modal.

### Scope
**In Scope:**
- Root-cause fix so that saving a wall style actually regenerates the *current* rendered geometry of every wall using that style (or an inherited descendant).
- New user-facing flow: an explicit action to save a style **and** apply it to the current drawing, decoupled from a plain "save the library only" action, so users understand what will happen to their drawing before triggering a (potentially large) regeneration pass.
- Split `src/ui/window/aec_style_manager.rs` into two separate modal dialogs: **AEC Material Manager** (`AEC_MATERIALMANAGER`) and **AEC Wall Style Manager** (`AEC_STYLEMANAGER`, repurposed to open only the wall-style dialog).
- Wall Style Manager: hierarchical (indented, parent-before-children) list of wall styles as the master list; the per-layer table uses the full dialog width and does not require inner scrolling for a reasonably small number of layers; "Save"/"Delete" buttons anchored bottom-right of the dialog.
- Material Manager keeps existing material list + edit form (name, hatch pattern picker, color swatch, line type), just extracted into its own dialog.
- Existing `AEC_STYLE`/`AEC_MATERIAL` command-line commands, the `AEC Style Picker` modal, and `AEC_WALL`'s style-selection prompt keep working unchanged (they read the same `StyleLibrary`, unaffected by the dialog split).

**Out of Scope:**
- Changing the underlying `WALL_V2` XDATA schema.
- Persistent/undoable regeneration history, batch progress UI for very large drawings.
- Any change to `AEC_WALL`, `AEC_ROOM`, `AEC_STOREY`, `AEC_ROOMSCHEDULE`, `AEC_IFCEXPORT`.

### User Stories
- As a planner, after editing a wall style's layers and clicking Save, I want the walls in my current drawing that use this style to visually update immediately, so I can verify the change without reopening the file.
- As a user, I want it to be clear whether "Save" only updates the style library or also touches my drawing, so I'm not surprised by unexpected geometry regeneration.
- As a user, I want Materials and Wall Styles managed in separate, focused dialogs, so each one is simpler and less cluttered.
- As a planner, I want the wall-style layer table to be fully visible without scrolling and to use the dialog's full width, so I can see all layer columns (material, thickness, gap, offsets, function, layer override) at once.

### Functional Requirements
- Root cause: `regenerate_wall_representation` (in `src/modules/aec/commands.rs`) rebuilds a wall's contour/hatch/solid entities from the **layer snapshot stored in the wall's own `WALL_V2` XDATA** (`wall_v2_from_entity(entity).layers`), not from the *current* `StyleLibrary`'s `effective_layers(style_id)`. When `AecStyleManagerWallStyleSave` calls `regenerate_wall_representation` after editing the style, the function faithfully rebuilds the *same old* stored layers, so nothing visually changes even though the command line correctly reports "N wall(s) updated".
- Fix: before calling `regenerate_wall_representation` for an affected wall, the save handler must first re-resolve `effective_layers(&style_map, &wall.style_id)` from the just-saved library and rewrite the wall's `WALL_V2` layer snapshot (mirroring the same snapshot-writing logic already used when a wall is first drawn with a chosen style), then call `regenerate_wall_representation`.
- Explicit two-action UX in the Wall Style Manager: "Save" persists the style to the library only (current behavior minus the drawing update); a new distinct action "Save & Update Drawing" (or equivalent single primary action per below UX decision) persists **and** re-resolves + regenerates affected walls in the active drawing.
- `AEC_MATERIALMANAGER` new command opens a standalone Material Manager modal; `AEC_STYLEMANAGER` opens a standalone Wall Style Manager modal (no material list/tab in it anymore).
- Wall Style Manager's master list always renders as a hierarchical, indented tree (no Name/Hierarchy toggle needed anymore, since flat sorting is no longer relevant without materials sharing the list).
- Layer table columns fit within the dialog's available width without per-row horizontal scrolling; the whole layer list scrolls vertically only if it exceeds the available vertical space.

### Non-Functional Requirements
- No change to XDATA persistence format (`WALL_V2`) or the `StyleLibrary` file format — only how/when things are (re)written.
- Existing tests (`cargo test --lib aec`, `polyline`, `properties`) must remain green; new tests must cover the regeneration-uses-current-style-layers fix.

# Technical Design

### Current Implementation
- `src/ui/window/aec_style_manager.rs`: single 960×520 modal (`ModalKind::AecStyleManager`, opened by `AEC_STYLEMANAGER`) with a combined two-pane master list (Materials section + Wall Styles section, filterable, with a Name/Hierarchy sort toggle for wall styles via `ordered_wall_styles`) and a detail pane that renders either the Material form or the Wall Style form depending on selection.
- `src/app/update/mod.rs::Message::AecStyleManagerWallStyleSave` (~line 2241-2409): builds the `WallStyle` from the form buffers, upserts it into `self.aec_style_library`, calls `save_to_default_path`, then — on success — scans all entities in the *active tab's* document via `wall_v2_from_entity`, resolves each wall's style-inheritance chain via `resolve_chain`, and for every wall whose chain contains the saved style id, calls `crate::modules::aec::commands::regenerate_wall_representation(&mut tab.scene, handle)`, reporting the updated count.
- **Root cause of the bug**: `regenerate_wall_representation` (`src/modules/aec/commands.rs` ~line 594-651) reads `wall_v2_from_entity(entity)` to get `(layers, height, old_derived, is_v2)` — i.e. it uses the **layer snapshot already stored on the wall's own XDATA**, which was written once when the wall was drawn/assigned a style and never touched by the style-manager save flow. So calling `regenerate_wall_representation` after a style edit rebuilds the exact same (now stale) layers — the drawing is literally regenerated identically, hence no visible change, even though `updated_count` correctly counts how many walls were processed.
- `WallStyle`/`effective_layers` (`src/modules/aec/engine/wall_style.rs`) already implements the parent-chain layer resolution used when a wall is first drawn (`WallPhase::AskStyle` in `commands.rs`) — this exact function needs to be reused at save-time to refresh each affected wall's stored snapshot.
- `set_wall_v2_derived_handles`/writing `WALL_V2` records already exists in `commands.rs` (used by `regenerate_wall_representation` itself and by the wall-drawing finalize path) — the same record-writing helper can be reused to overwrite a wall's `layers` field before calling `regenerate_wall_representation`.
- `StyleLibrary::wall_style_tree()` (added in `src/modules/aec/engine/library.rs`) already produces the indented hierarchical order used by the AEC Style Picker modal — this is the exact function to reuse for the Wall Style Manager's master list once materials are split out (removing the need for `ordered_wall_styles`'s Name/Hierarchy toggle, which existed only because materials and wall styles shared one flat/mixed list).
- Two modals already coexist as a template for splitting: `ModalKind::AecStyleManager` (960×520) and `ModalKind::AecStylePicker { target }` (500×500, `src/ui/window/aec_style_picker.rs`) — the split reuses this exact `ModalKind` + `sized_flow` pattern.

### Key Decisions
- **Fix at the save-handler level, not inside `regenerate_wall_representation` itself**: `regenerate_wall_representation` is also called from non-style-edit paths (wall drawing finalize, move/rotate/grip-edit, properties-panel edits) where the wall's *own* stored snapshot is intentionally authoritative (e.g. after a properties-panel edit that doesn't touch the style library). Changing its core reading logic to always re-resolve from the library would risk breaking those paths (e.g. a wall whose style was deleted from the library but whose old snapshot should still render). Instead, the **style-manager save handler** is responsible for refreshing each affected wall's own snapshot before calling `regenerate_wall_representation`, keeping `regenerate_wall_representation`'s existing single-responsibility contract ("rebuild from what's stored on this wall") intact.
- **Explicit "Save" vs "Save & Update Drawing"**: rather than silently regenerating potentially many walls on every save (as today, ineffectively), the Wall Style Manager gets two clearly distinct primary actions so the user consciously decides when to touch the drawing. This directly addresses the user's own suggestion ("Speichern und aktuelle Zeichnung aktualisieren").
- **Two separate `ModalKind` variants instead of tabs inside one dialog**: splitting into `AecMaterialManager` and `AecWallStyleManager` (renaming the existing `AecStyleManager` kind) mirrors the existing pattern of one dialog per concern (e.g. `LayerStateManager`, `LayoutManager`) rather than introducing an in-dialog tab-switcher, which keeps each dialog focused and simpler, per the user's explicit request.
- **Wall Style Manager always shows the hierarchical tree** (via `wall_style_tree()`), dropping the Name/Hierarchy toggle: once materials are no longer sharing the list, there's no longer a strong case for a flat alternative — hierarchy is the more informative default the user specifically asked for.
- **Layer table full-width, no horizontal scroll**: achieved purely by widening the Wall Style Manager modal further and/or reducing/reflowing the existing fixed per-column widths (`LAYER_COL_*_W` constants) to fit within the new dialog width — no new virtualization/layout technique needed, since the column set and widths are already centrally defined as constants.

### Proposed Changes
1. **Fix wall regeneration to reflect the current style** (`src/modules/aec/commands.rs` + `src/app/update/mod.rs`): add a small helper (e.g. `fn write_wall_v2_layers(scene: &mut Scene, wall_handle: Handle, layers: &[WallLayer])`, reusing the existing XDATA-record-write path from `wall_v2_from_entity`'s counterpart writer) that overwrites just the `layers` portion of a wall's `WALL_V2` record. In the wall-style save flow, for each affected handle: resolve `effective_layers(&style_map, &wall.style_id)` from the just-saved library, convert to `WallLayer`s, call the new helper, **then** call `regenerate_wall_representation`.
2. **"Save" vs "Save & Update Drawing"**: split `Message::AecStyleManagerWallStyleSave` into `AecStyleManagerWallStyleSave` (library-only, no drawing touch) and a new `AecStyleManagerWallStyleSaveAndApply` (library save + step 1's refresh-and-regenerate pass); both share the validation/upsert logic via a common internal function, only the final drawing-update block differs.
3. **Split the modal**: rename `ModalKind::AecStyleManager` to `ModalKind::AecMaterialManager`; add `ModalKind::AecWallStyleManager`. Split `src/ui/window/aec_style_manager.rs` into `aec_material_manager.rs` (materials list + form only) and `aec_wall_style_manager.rs` (wall styles tree + layer-table form only, using `wall_style_tree()` for ordering). Update `AEC_STYLEMANAGER` (now opens `AecWallStyleManager`) and add `AEC_MATERIALMANAGER` command + Ribbon button (Styles group) opening `AecMaterialManager`.
4. **Layout tweaks for the Wall Style Manager**: widen the modal (e.g. to accommodate all `LAYER_COL_*_W` columns without scroll, recompute/verify total column width vs. dialog width) and move the Save/Save & Update Drawing/Delete button row to the bottom-right via a `row![Space::new().width(Fill), ...buttons]` pattern (already used elsewhere in the app, e.g. AEC Style Picker's Cancel/Select row).

### Data Models / Contracts
```rust
// commands.rs — new small helper reused by the save-and-apply flow
fn write_wall_v2_layers(scene: &mut Scene, wall_handle: Handle, layers: &[WallLayer]) -> bool { /* rewrite WALL_V2 record's layer fields only, keep style_id/height/derived_handles */ }

// app/mod.rs — new Message variant alongside the existing one
AecStyleManagerWallStyleSaveAndApply,

// app/mod.rs — ModalKind rename + split
AecMaterialManager,      // was AecStyleManager
AecWallStyleManager,     // new
```

### Components
- `src/modules/aec/commands.rs` (changed): new `write_wall_v2_layers` helper.
- `src/app/update/mod.rs` (changed): `AecStyleManagerWallStyleSave` split into save-only and save-and-apply handlers, both sharing upsert/validation logic; save-and-apply refreshes each affected wall's stored layers before regenerating.
- `src/app/mod.rs` (changed): `ModalKind` split/rename, new `Message::AecStyleManagerWallStyleSaveAndApply`.
- `src/ui/window/aec_material_manager.rs` (new, extracted from `aec_style_manager.rs`): materials-only dialog.
- `src/ui/window/aec_wall_style_manager.rs` (new, extracted from `aec_style_manager.rs`, using `wall_style_tree()`): wall-styles-only dialog with full-width layer table and bottom-right Save/Save & Update Drawing/Delete buttons.
- `src/app/view/modal.rs` (changed): routes the two new `ModalKind`s to the two new view modules with appropriately sized `sized_flow` calls.
- `src/app/commands/draw.rs` (changed): `AEC_STYLEMANAGER` now opens `AecWallStyleManager`; new `AEC_MATERIALMANAGER` opens `AecMaterialManager`.
- `src/modules/aec/mod.rs` (changed): new Ribbon button for `AEC_MATERIALMANAGER` in the "Styles" group.

### Risks
- **Regeneration cost on large drawings**: "Save & Update Drawing" walks every entity in the active document and regenerates matching walls — acceptable for the current AEC feature's expected scale (single-building models), consistent with the existing (buggy) implementation's already-identical cost.
- **Splitting the file**: `aec_style_manager.rs` currently holds shared helpers (`muted`, `list_style`, `hex_to_acad_color`, `LAYER_COL_*` constants) used by both material and wall-style sections — these need to be moved to a small shared module (or kept in one of the two files and imported by the other) to avoid duplication.
- **Migration of existing users' muscle memory**: `AEC_STYLEMANAGER` changing what it opens (wall styles instead of the combined dialog) is a deliberate breaking change to the command's behavior, mitigated by adding the clearly-named new `AEC_MATERIALMANAGER` command alongside it.

# Delivery Steps

###   Step 1: Fix wall-style-save so drawing regeneration reflects the current (edited) style
Saving a wall style and updating the drawing actually changes the rendered geometry of affected walls, instead of silently rebuilding stale stored layers.
- Add `write_wall_v2_layers(scene, wall_handle, layers)` helper in `src/modules/aec/commands.rs` that overwrites only the layer-snapshot portion of a wall's `WALL_V2` XDATA record, keeping `style_id`/`height`/`derived_handles` intact.
- In `src/app/update/mod.rs`'s wall-style-save drawing-update block, for each affected wall handle: resolve `effective_layers(&style_map, &wall.style_id)` from the just-saved library, convert to `WallLayer`s, call `write_wall_v2_layers`, then call `regenerate_wall_representation`.
- Add a unit test that edits a wall style's layers (e.g. changes a layer's thickness/material), triggers the save-and-refresh flow on a wall using that style, and asserts the wall's new derived geometry (contour/solid dimensions) reflects the updated layer, not the original one.

###   Step 2: Add an explicit "Save & Update Drawing" action separate from plain "Save"
Users can choose between saving the wall style library only, or saving and immediately applying the change to the current drawing.
- Add `Message::AecStyleManagerWallStyleSaveAndApply` in `src/app/mod.rs`.
- Refactor `Message::AecStyleManagerWallStyleSave`'s handler in `src/app/update/mod.rs` into a shared internal validation/upsert/library-save function, with `AecStyleManagerWallStyleSave` doing library-save only (no drawing touch) and the new `AecStyleManagerWallStyleSaveAndApply` doing library-save plus the fixed regeneration pass from the previous step.
- Wire both actions to buttons in the wall-style form (temporary location in the existing combined dialog, ahead of the split in the next steps).

###   Step 3: Split the AEC Style Manager into a standalone Material Manager dialog
Materials are managed in their own focused modal, separate from wall styles.
- New file `src/ui/window/aec_material_manager.rs` containing the materials list + material edit form (name, hatch-pattern picker, color swatch, line-type dropdown), extracted from `src/ui/window/aec_style_manager.rs`; shared helpers (`muted`, `list_style`, `hex_to_acad_color`) moved to a small shared location importable by both new files.
- Rename `ModalKind::AecStyleManager` to `ModalKind::AecMaterialManager` in `src/app/mod.rs`; update `src/app/view/modal.rs` routing accordingly.
- New `AEC_MATERIALMANAGER` command in `src/app/commands/draw.rs` opening `ModalKind::AecMaterialManager`; new Ribbon button in the "Styles" group (`src/modules/aec/mod.rs`).

###   Step 4: Build the standalone Wall Style Manager dialog with hierarchical list and full-width layer table
Wall styles get their own dialog showing a hierarchical tree and a layer table that fits the dialog width without horizontal scrolling.
- New file `src/ui/window/aec_wall_style_manager.rs` containing the wall-style master list (rendered via `StyleLibrary::wall_style_tree()` for indentation, dropping the now-unneeded Name/Hierarchy toggle) and the wall-style edit form (name, parent picker via AEC Style Picker, layer table).
- New `ModalKind::AecWallStyleManager`; update `AEC_STYLEMANAGER` in `src/app/commands/draw.rs` to open this new modal kind instead of the old combined one.
- Widen the dialog and/or adjust `LAYER_COL_*_W` constants so all layer-table columns (material, thickness, gap, bottom/top offset, function, layer override, reorder/delete actions) are visible without horizontal scrolling for a typical layer count.
- Verify the Save/Save & Update Drawing/Delete button row from the previous step renders anchored to the bottom-right of this new dialog.

###   Step 5: Remove the old combined dialog and finalize wiring/tests
The old combined AEC Style Manager is fully replaced by the two focused dialogs, and existing tests continue to pass.
- Delete `src/ui/window/aec_style_manager.rs` once its content has been fully migrated into the two new files from the previous steps.
- Verify `src/app/view/modal.rs` no longer references the old `ModalKind::AecStyleManager` variant anywhere; confirm `AEC_STYLE`/`AEC_MATERIAL` command-line commands and the AEC Style Picker modal still function unchanged against the same underlying `StyleLibrary`.
- Run `cargo test --lib aec`, `cargo test --lib polyline`, `cargo test --lib properties` and fix any regressions; add/adjust tests for the new `write_wall_v2_layers` helper and the save-vs-save-and-apply split.