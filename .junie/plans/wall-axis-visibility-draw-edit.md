---
sessionId: session-260909-030136-14n6
---

# Requirements

### Overview & Goals
Beim Zeichnen und Ändern von Wänden (`AEC_WALL`, Grips/Join) soll die **Wandachse sichtbar** sein, unabhängig von der Planart. Nach Abschluss gilt wieder die aktive Display-Konfiguration (`WallComponentKind::Axis` / Layer `AEC_WALL_AXIS`).

Die Achse bleibt **immer fangbar** (OSNAP), auch wenn sie unsichtbar ist.

### Scope
**In Scope**
- Temporäre Sichtbarkeit der Achse während Live-Zeichnen, Segment-Commit und Bearbeitung (MOVE/Grips/Join).
- Nach Command-Ende: Achse wieder gemäß aktiver Planart (oft unsichtbar).
- Snapping bleibt auf der Achse, **auch wenn der Layer/Slot aus ist** (`wall_axis_snap_wires`).
- Kein Snap auf abgeleitete Kontur/Hatch/Solid-Kanten.

**Out of Scope**
- Neue Display-Slots, Plan-Manager-UI, IFC, Öffnungen.

### Functional Requirements
- Während `AEC_WALL` aktiv: Achse der live- und bereits committed Segmente sichtbar (Preview + committed Axis-Polyline).
- Während Grip/Join/Geometrie-Änderung an einer Wand: Achse dieser Wand (und Join-Nachbarn) sichtbar.
- Idle: Layer `AEC_WALL_AXIS` bzw. Slot `Axis` folgt `DisplayConfig` / Style-Profile; kein dauerhaftes Einschalten des Layers.
- Unsichtbare Achse bleibt OSNAP-Kandidat: `wall_axis_snap_wires` tesselliert Axis-Entities trotz `layer.flags.off` und filtert derived Wires raus. Die Axis-Polyline darf nicht gelöscht werden, nur unsichtbar sein.

# Technical Design

### Current Implementation
- Achse ist `LwPolyline` auf `AEC_WALL_AXIS` (`src/modules/aec/commands.rs`), Layer standardmäßig `off` / nicht plotbar (`ensure_wall_axis_layer`).
- OSNAP: Viewport (`src/app/update/viewport.rs`) ruft `wall_axis_snap_wires` auf dem Wire-Set auf — injiziert Axis-Wires trotz unsichtbarem Layer, droppt derived Konturen. Test: `wall_axis_snap_wires_excludes_derived_and_includes_axis`.
- Sichtbarkeit Slot `WallComponentKind::Axis` / `WallComponentSlot::AxisLine` über `build_effective_rule_set` und Regeneration nach Commit (`regenerate_wall_respecting_active_display_config` + `reapply_active_display_config_to_wall_packages` in `src/app/update/mod.rs` / `command_driver.rs`).
- Neu gezeichnete Wände folgen bereits der Planart (letzter Fix).

### Key Decisions
- **Session-Override, nicht Layer dauerhaft an:** während Wand-Command/Edit die Achse per Overlay/Force-visible zeichnen oder Layer nur für die Session einschalten und danach `reapply_active_display_config`.
- **Kein zweites Achs-Entity:** bestehende Axis-Polyline nutzen.
- **Snap unabhängig von Sichtbarkeit:** Visibility-Override darf `wall_axis_snap_wires` nicht umgehen oder die Axis-Entity entfernen. Idle mit verstecktem Axis-Slot muss weiterhin Axis-Wires liefern.

### Proposed Changes
1. **Draw (`WallCommand`):** Live-Preview zeigt Achse immer. Nach Segment-Commit: Achse sichtbar halten, solange Command aktiv; Display-Config erst beim Command-Ende voll anwenden (Packages regenerieren, Axis-Slot respektieren).
2. **Edit:** Beim Start von Grip/MOVE/Join an Wall-Package Achse der betroffenen Handles sichtbar; beim Verlassen reapply Display-Config.
3. **Idle:** Planart — typisch Achse unsichtbar; Snap weiter über `wall_axis_snap_wires` (Viewport-Pfade beibehalten).
4. **Regression:** Hidden Axis-Slot + `wall_axis_snap_wires` enthält Axis, nicht derived; Draw/Edit-Session darf das nicht brechen.

### File Structure
- `src/modules/aec/commands.rs` — Preview + Axis-Layer während Command
- `src/app/command_driver.rs` — Commit vs. Command-Ende Regeneration
- `src/app/update/mod.rs` — reapply / Tests

# Delivery Steps

### ✓ Step 1: Achse während AEC_WALL sichtbar halten
Während des Wandzeichnens bleibt die Achse sichtbar, die Planart gilt erst nach Command-Ende.

- Live-Preview der Achse in `WallCommand` immer zeichnen.
- Nach Segment-Commit Display-Config auf Kontur/Hatch anwenden, Axis-Slot während aktivem Command nicht verstecken.
- Betroffene Stellen: `commands.rs`, `command_driver.rs` (Commit-Pfad).

### ✓ Step 2: Achse bei Wand-Änderung und Idle-Reset
Beim Editieren betroffener Wände ist die Achse sichtbar; danach gilt wieder die Planart.

- Grip/Join/MOVE auf Wall-Packages: Axis der betroffenen Handles temporär sichtbar.
- Command-Ende / Idle: `reapply_active_display_config_to_wall_packages` inkl. Axis-Slot.
- Tests: Draw mit verstecktem Axis-Slot → Achse während Command sichtbar, danach unsichtbar; Edit analog.
- Idle/hidden: `wall_axis_snap_wires` enthält Axis-Wires, keine derived Konturen; Viewport-Pfade unverändert.