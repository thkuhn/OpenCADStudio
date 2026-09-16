---
sessionId: session-260916-073620-1igx
---

# Requirements

### Overview & Goals
**Stand (2026-09-16):** Inventur aller 35 Dateien unter `.junie/plans/`. Die Pläne sind Historie; der Code unter `src/modules/aec/` ist die Quelle der Wahrheit.

**Ergebnis:** Fast alles Geplante ist umgesetzt. Es gibt **kein** offenes Feature-Backlog in den Plänen außer wenigen bewusst zurückgestellten Punkten und einem unvollständigen Core-Split.

### Erledigt (Code + Plan-Steps ✓)

**Wand-Workflow**
- `aec-architecture-plugin.md` — interaktives `AEC_WALL`, Properties, Grips (Core-Modul, nicht Plugin)
- `aec-wall-live-properties-panel.md`
- `aec-wall-preview-join-context-menu.md` — Kontur-Vorschau, frühe Stilwarnung, `AEC_WALLJOIN`/`AEC_WALLEXTEND`, Auto-Join beim Zeichnen, Kontextmenü
- `aec-wall-dynamic-length-height-input.md` — trotz Dateiname: Reverse, Miter-Matching, Hover, Extend-Modi (nicht dyn_spec)
- `wall-single-segment.md` — eine Entity pro Segment, Auto-Join an der Kette
- `wall-axis-visibility-draw-edit.md` — Delivery ✓
- `wandstil-schicht-achsversatz.md`, Composite/Rendering/Style-Pläne (Phasen 1–4, Manager, Picker, Plan-View, Display-Fixes)

**Joins / T-Stoß**
- `t-join-core-miter.md` + `t-join-core-miter-recap.md` — Kern-Paarung, Putz-Pocket, Gehrung, `layer_gaps` an der Durchgangswand

**Projekt / Geschosse / Ebenen**
- `aec-control-planes.md` — ControlPlane, Storey-Modal, Preview-Layer, Wand Fuß/Kopf
- `storey-z-hatch-origin.md` — Elevation-Translation, OKGH, Bind/Unbind, Hatch WCS 0

**Bibliothek / i18n**
- `fix-wall-joins.md` — Session-Library aus DXF (`aec_session_style_library`, Combined-Listen)
- `document-style-library-copy-override-backlog*.md` — kombinierte Listen, Copy-on-Write, Projekt-Guard
- `replace-hardcoded-strings.md` — AEC-i18n Done (Commit genannt)

### Teilweise / in Arbeit

**`aec-core-separation.md` Step 3 (`*`)**
- Step 1–2: Fassade (`Message::Aec`, `AecState`, `spawn_command`) und Aufteilung `walls/`, `rooms/`, `styles/`, `project/`, `ifc/`, `ui/` — vorhanden.
- Step 3: UI/Properties leben bereits unter `src/modules/aec/{ui,properties.rs}`. **Noch offen laut Plan:** Core-Reste reduzieren (`document.rs` Session-Feld, Overlay-Kontextmenü, `commands.rs` als große Sammeldatei neben den Tool-Dateien). Kein neues Fachfeature.

### Bewusst offen / Nice-to-have (kein aktiver Plan)

- Dynamische In-Viewport-Längen-/Höhen-Eingabe (`dyn_spec` auf `WallCommand`) — Preview-Plan Step 6 `~`
- Manuelle Junction-Overrides je Schicht, N-Wege-Joins
- Fenster/Türen als echte Bauteile (Stub-Dateien `walls/window.rs`, `door.rs` existieren)
- Vollautomatische Graph-Erkennung aller Kreuzungen
- IFC-Import, Boolean-Durchbrüche, Multi-Dokument-Projekt
- Drag&Drop-Stilimport, App-weites i18n außerhalb AEC
- Schnitt-/Ansichts-Viewports aus Kontrollflächen
- Höhenkoten-Zeichnung, Pflicht-UKRD

### Out of Scope (unverändert oft wiederholt)

- Eigenes AEC-Crate / Feature-Flag
- `include!`
- AEC-Fachlogik in Core wachsen lassen

# Technical Design

### Current Implementation
Layout entspricht weitgehend dem Ziel aus `aec-core-separation.md`:

- `src/modules/aec/engine/` — Domain (miter, join, contour, control_plane, library, …)
- `src/modules/aec/walls|rooms|styles|project|ifc|ui/`
- `commands.rs` existiert **noch** parallel (nicht vollständig aufgelöst)
- Core-Hooks: `aec::update`, `spawn_command`, `properties::extend`, Session-Library am Tab

### Key Decisions (gültig)
1. AEC nur unter `src/modules/aec/**`; Core dünne Hooks.
2. Session-Library aus Zeichnung, keine Auto-`.ocsproj`.
3. T-Stoß: Structural über `function`; Putz-Pocket; Gehrung gleiches Material.
4. Eine Wand-Entity pro Segment; Altbestand-Mehrpunkt bleibt ladbar.

### File Structure
Neuer Recap: diese Datei. Ältere Recaps: `t-join-core-miter-recap.md`, Status-Blöcke in `storey-z-hatch-origin.md` und `replace-hardcoded-strings.md`.
