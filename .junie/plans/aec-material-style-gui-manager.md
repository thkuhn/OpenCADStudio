---
sessionId: session-260812-011701-iuuy
---

# Requirements

### Overview & Goals
Bisher können AEC-Materialien und Wandstile nur über die Kommandozeilen-Kommandos `AEC_MATERIAL`/`AEC_STYLE` (schrittweise Text-Prompts) angelegt werden — es gibt keine Übersicht über bereits vorhandene Materialien/Stile, keine Bearbeitung/Löschung, und keine visuelle Darstellung der Vererbungskette. Diese Aufgabe ergänzt einen echten **GUI-Manager-Dialog** (`AEC_STYLEMANAGER`), analog zum bestehenden `LAYERSTATE`/Layer-State-Manager (`src/ui/window/layer_state_manager.rs`), der Materialien und Wandstile in einer Liste anzeigt, bearbeitbar macht und konsistent mit der bereits vorhandenen Bibliotheksdatei (`engine::library`) speichert.

### Scope
**In Scope:**
- Neuer Modal-Dialog "AEC Style Manager" mit Listen für Materialien und Wandstile (mit Filter, analog Layer-State-Manager).
- Anlegen/Bearbeiten/Löschen von Materialien (Name, Schraffurmuster, Linienfarbe, Linientyp).
- Anlegen/Bearbeiten/Löschen von Wandstilen inkl. Eltern-Stil-Auswahl (Vererbung) und geordneter Schicht-Liste (Material-Referenz, Dicke, Funktion).
- Live-Vorschau der aufgelösten effektiven Schichten (`effective_layers()`) unter Berücksichtigung der Vererbungskette.
- Speicherung über die bestehende `StyleLibrary`/`save_to_default_path`-Infrastruktur, sodass Kommandozeile (`AEC_MATERIAL`/`AEC_STYLE`) und GUI dieselbe Bibliotheksdatei konsistent nutzen.
- Sofortige Verfügbarkeit neuer/geänderter Stile in der `AEC_WALL`-Stilauswahl.

**Out of Scope:**
- Persistente Geschoss-Verwaltung, Fenster/Türen, 3D-Material-Rendering-Anbindung an den Host (weiterhin zurückgestellt).
- Mehrfachvererbung/Mixins (bleibt Einzel-Elternkette wie bisher entschieden).
- Drag&Drop-Reihenfolge der Schichten (einfache Auf/Ab- bzw. Hinzufügen/Entfernen-Buttons reichen für diese Iteration).

### User Stories
- Als Planer möchte ich alle vorhandenen Materialien und Wandstile in einer Liste sehen, damit ich nicht raten muss, was per Kommandozeile bereits angelegt wurde.
- Als Planer möchte ich einen bestehenden Wandstil bearbeiten (z. B. eine Schicht hinzufügen), ohne ihn komplett über die Kommandozeile neu eingeben zu müssen.
- Als Nutzer möchte ich beim Bearbeiten eines Wandstils sehen, welche Schichten er von seinem Eltern-Stil erbt, damit ich die Auswirkung meiner Änderung verstehe.
- Als Nutzer möchte ich ein nicht mehr benötigtes Material oder einen Stil löschen können.

### Functional Requirements
- `AEC_STYLEMANAGER`-Kommando öffnet den Dialog und lädt die aktuelle Bibliothek via `load_or_seed()`.
- Materialliste: Auswahl zeigt Detailformular; "Neu"/"Löschen"-Buttons; Speichern schreibt via `save_to_default_path`.
- Wandstilliste: Auswahl zeigt Detailformular mit Name, Eltern-Stil-Picklist (Zyklus-sicher), Schicht-Editor (Material-Picklist aus vorhandenen Materialien, Dicke, Funktion).
- Änderungen im Manager sind sofort danach für einen neu gestarteten `AEC_WALL`-Zeichenworkflow sichtbar (gleiche Bibliotheksdatei).

# Delivery Steps

###   Step 1: Add Message variants, app state, and command dispatch for the manager modal
The app can open an empty Material & Style Manager modal via a new AEC_STYLEMANAGER command and Ribbon button, with state fields wired but no content yet.
- Add `ModalKind::AecStyleManager` in `src/app/mod.rs`, plus state fields (`aec_library: StyleLibrary`, `aec_style_filter: String`, `aec_selected_material: Option<String>`, `aec_selected_wall_style: Option<String>`, edit-buffer fields for the active material/style form).
- Add `Message` variants: `AecStyleManagerOpen`, `AecStyleManagerClose`, `AecSelectMaterial(String)`, `AecSelectWallStyle(String)`, `AecNewMaterial`, `AecNewWallStyle`, `AecDeleteMaterial`, `AecDeleteWallStyle`, `AecMaterialFieldChanged(...)`, `AecWallStyleFieldChanged(...)`, `AecLayerAdd`, `AecLayerRemove(usize)`, `AecLayerFieldChanged(usize, ...)`, `AecStyleManagerSave`.
- Add `AEC_STYLEMANAGER` command dispatch in `src/app/commands/draw.rs` (mirroring the `LAYERSTATE`/`LMAN` pattern in `src/app/commands/layers.rs`) that loads `load_or_seed()` into `aec_library` and sets `active_modal`.
- Add a Ribbon button in the existing "Styles" group in `src/modules/aec/mod.rs` that fires `AEC_STYLEMANAGER` (alongside the existing `AEC_MATERIAL`/`AEC_STYLE` command-line tools).

###   Step 2: Build the two-pane manager view (material list + wall-style list with filter)
Opening the modal shows a real dialog with a filterable master list of materials and wall styles, styled consistently with `layer_state_manager.rs`.
- New file `src/ui/window/aec_style_manager.rs` with a `view_window(...)` function following the `layer_state_manager.rs` layout convention (filter text_input, scrollable selectable list using `list_style`/`button_style` helpers, divider, detail panel placeholder).
- Register the modal in `src/app/view/modal.rs` (match arm for `ModalKind::AecStyleManager` calling the new view function) and wire Escape-to-close in `src/app/update/mod.rs` alongside the existing `LayoutManager`/`LayerStateManager` escape handling.
- List rendering: materials and wall styles shown in two sections/tabs within the same window, each row showing name (+ parent style name for wall styles, resolved via the existing `style.rs` chain logic).

###   Step 3: Implement the material detail/edit form
Selecting or creating a material shows an editable form (name, hatch pattern, line color swatch/hex, line type) that writes back through the existing `Material` model.
- Detail panel in `aec_style_manager.rs` renders `text_input` fields for name/hatch/line type and a hex color field (reuse formatting conventions from existing property panels, e.g. `src/app/properties.rs` color handling).
- `Message::AecMaterialFieldChanged`/`AecNewMaterial`/`AecDeleteMaterial` handlers in `src/app/update/mod.rs` mutate `aec_library.materials` in place using `StyleLibrary::upsert_material`/removal (extend `StyleLibrary` with a `remove_material` method if not already present).
- `AecStyleManagerSave` persists the in-memory `aec_library` via `engine::library::save_to_default_path`, reusing the exact same library shape that `AEC_MATERIAL_ADD`/`AEC_STYLE_ADD` already write, so command-line and GUI paths stay consistent.

###   Step 4: Implement the wall-style detail/edit form with parent selection and layer list editor
Selecting or creating a wall style shows an editable form (name, parent-style dropdown, ordered layer list with add/remove/reorder) that writes back through the existing `WallStyle`/`Layer` model.
- Detail panel section for wall styles: name field, a parent-style picklist (list of other wall style names, excluding self and any style that would create a cycle — reuse `style.rs`'s cycle-detection logic to validate before allowing selection), and a scrollable list of layer rows (material picklist referencing `aec_library.materials`, thickness `text_input`, function picklist Structural/Insulation/Finish/Other).
- `Message::AecLayerAdd`/`AecLayerRemove`/`AecLayerFieldChanged` mutate the in-progress `WallStyle.layers` `Vec`; `AecNewWallStyle`/`AecDeleteWallStyle` mirror the material handlers using `StyleLibrary::upsert_wall_style`.
- Show a live-resolved preview of `effective_layers()` (inherited layers from the parent chain merged with this style's own layers) so the user can see the final result of inheritance while editing.

###   Step 5: Wire manager edits into AEC_WALL's style picker and add regression tests
Materials/styles created or edited in the GUI manager immediately appear in `AEC_WALL`'s command-line style-selection prompt, and the manager's core mutation logic is covered by tests.
- Ensure `WallCommand::new()` (via `load_or_seed()`) picks up styles saved by the manager by confirming `AecStyleManagerSave` writes to the same `default_library_path()` used by `load_or_seed`/`aec_material_add`/`aec_style_add`.
- Add unit tests in `src/modules/aec/engine/library.rs` for any new `StyleLibrary` methods (`remove_material`, `remove_wall_style` if added) covering upsert/remove/cycle-safe parent reassignment.
- Add a small integration-style test (or targeted manual verification path documented in the PR) confirming a material/style created via the manager's mutation functions round-trips through `to_toml`/`from_toml` and is visible to a freshly constructed `WallCommand`.