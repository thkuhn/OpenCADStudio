---
sessionId: session-260918-135144-gn0o
---

# Requirements

### Overview & Goals
Beim Kontextmenü-Befehl **Schichtverbindung in Zeichnung...** (und analog Schichtlücke) sollen Maus-Hover und Klick **einzelne Wandschichten** treffen und orange hervorheben — nicht die ganze Wand als Entity.

### Scope
**In scope**
- Hover/Klick während `AecLayerPairDrawPick` / `AecLayerGapDrawPick`
- Unterdrücken der normalen Entity-Rollover-Selektion
- Bestehende Pick-Logik `pick_junction_wall_layer` nutzen

**Out of scope**
- Junction-Solver / Geometrie der Verbindungen
- Junction-Editor-Panel (dort existiert bereits Highlight über `sync_junction_editor_layer_highlight`)

### Functional Requirements
- Nach Start aus dem Wandverbindungs-Kontextmenü: Cursor über einer Schicht markiert nur diese Schicht (Hatch `WireModel::LAYER_PICK`).
- Klick setzt Schicht A, zweiter Klick Schicht B (bzw. Gap-Schritte), wie in `click_layer_pair_draw` / `click_layer_gap_draw` vorgesehen.
- ESC bricht weiter über `cancel_layer_pair_draw_pick` ab.
- Keine Box-Selektion / kein Wand-Hover während des Picks.

# Technical Design

### Current Implementation
Der Flow ist bereits in AEC angelegt, aber **nicht an den Viewport angebunden**:

- Start: `AecMessage::AecJunctionLayerPairPickStart` in `src/modules/aec/update.rs` setzt `AecState.aec_layer_pair_draw`.
- Hover/Klick: `OpenCADStudio::update_layer_pair_draw_hover` / `click_layer_pair_draw` (und Gap-Pendants) in `src/app/update/mod.rs` rufen `commands::pick_junction_wall_layer` auf und schreiben `pick.hover` / `layer_a` / `layer_b`.
- Highlight: `sync_junction_editor_layer_highlight` baut Hatches via `wall_layer_highlight_hatch`.

**Ursache:** `update_layer_pair_draw_hover` und `click_layer_pair_draw` werden **nirgends** aufgerufen. Idle-Mausbewegung in `src/app/update/viewport.rs` setzt weiter `scene.set_hover_highlight` auf die **ganze Wand**.

### Key Decisions
- Thin Core-Hook im Viewport (erlaubt laut AEC≠Core): wenn `aec_layer_pair_draw` oder `aec_layer_gap_draw` gesetzt ist, Hover/Click dorthin umleiten; keine neue AEC-Logik in `CadApp`.
- Pick-Geometrie bleibt in `src/modules/aec/commands.rs` (`pick_junction_wall_layer` — kleinster Contour-Loop gewinnt).

### Proposed Changes
1. **`viewport.rs` Pointer-Move** (Idle-Zweig um ~1886): wenn Layer-Pick aktiv:
   - `set_hover_highlight(None)` und kein `hover_dwell`
   - `cursor_model_point` → `update_layer_pair_draw_hover` / `update_layer_gap_draw_hover`
2. **`on_viewport_left_press`**: vor normaler Selektion, wenn Pick aktiv (und nicht `awaiting_style`): `click_layer_pair_draw` / `click_layer_gap_draw` und `return`.
3. Optional: bestehende Tests zu `pick_junction_wall_layer` / `wall_layer_contour_loop_xy` in `commands.rs` belassen; bei Fehlpick Contour vs. Darstellung prüfen.

### File Structure
- Modify: `src/app/update/viewport.rs` (Hooks)
- Unverändert nutzen: `src/app/update/mod.rs`, `src/modules/aec/commands.rs`, `src/modules/aec/update.rs`

# Delivery Steps

### ✓ Step 1: Viewport-Hooks für Schicht-Hover und -Klick
Layer-Pair- und Layer-Gap-Pick reagieren auf Mausbewegung und Linksklick; die ganze Wand wird nicht mehr als Hover-Entity markiert.

- In `src/app/update/viewport.rs` beim Pointer-Move: wenn `aec.aec_layer_pair_draw` oder `aec.aec_layer_gap_draw` gesetzt ist, Entity-Rollover (`set_hover_highlight` / `hover_dwell`) unterdrücken und Weltpunkt an `update_layer_pair_draw_hover` bzw. `update_layer_gap_draw_hover` übergeben.
- In `on_viewport_left_press` vor Box-/Entity-Selektion dieselben Modi abfangen und `click_layer_pair_draw` / `click_layer_gap_draw` aufrufen (Style-Menü `awaiting_style` weiter wie bisher behandeln).
- Keine AEC-Pick-Logik neu im Viewport implementieren — nur die bereits vorhandenen Methoden in `src/app/update/mod.rs` verdrahten.

### ✓ Step 2: Pick- und Highlight-Pfad gegen Schichtkontur prüfen
Hover trifft die kleinste Schichtkontur am Knoten; die orange Vorschau füllt nur diese Schicht, nicht den gesamten Wandkörper.

- `pick_junction_wall_layer` und `wall_layer_highlight_hatch` in `src/modules/aec/commands.rs` gegen Teilnehmer-Wände am Junction-Knoten abgleichen (Handles, Layer-Index, Loop-Fläche).
- Nur bei nachgewiesenem Mismatch Contour/Highlight anpassen; sonst unverändert lassen.
- Bestehende `#[cfg(test)]`-Tests zu `wall_layer_contour_loop_xy` in `commands.rs` als Regression behalten.