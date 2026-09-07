---
sessionId: session-260906-155215-1ltl
---

# Requirements

### Overview & Goals
Feinschliff der zuletzt umgesetzten **Wandstil-/Planart-Overrides** und der **Phasen** Neubau / Bestand / Abbruch, ohne das Modell umzudrehen.

Ziel: Darstellung folgt der Kette **Planart (Sichtbarkeit) → Stil-Overlay (Optik je Wandstil) → Phase (Filter + Zusatz nur auf 2D-Gesamtkontur) → Sitzungs-Umschalter 2D/3D/Alle**.

### Scope
**In Scope**
- Restliche UX-/Konsistenz-Punkte an Overlay-Schichten (Nummer + Materialname), Kontur-Schraffur nur im Stil-Overlay, Material-Picker im Wandstil-Manager.
- Statusleisten-Umschalter bleibt harter Override der Planart-Sichtbarkeit.
- Phasen: Wand hat `PlanPhase`; Planart filtert Sichtbarkeit; Zusatzstile Abbruch/Bestand gelten **nur** auf der 2D-Gesamtkontur (`Contour2D`), nicht schichtweise.
- Prüfen/glätten, dass Phasen-Filter und Regen mit dem Session-Override zusammenpassen.

**Out of Scope**
- Geschosse, Fenster/Türen, Boolean-Durchbrüche, IFC-Import.
- Globale Planart-Kontur-Schraffur wieder einführen.
- Stil-zentrierte `display_profiles` als produktives Modell (Legacy bleibt ungenutzt).

### Functional Requirements
- 2D/3D/Alle in der Statusleiste ändert die Wanddarstellung sofort, unabhängig von Planart-Default.
- Kontur-Hatch nur, wenn der aktive Wandstil in der Planart ein Overlay dafür hat.
- Overlay-Schichten: `n — Materialname` wie im Eigenschaftspanel.
- Wandstil-Manager: Material je Schicht als Auswahlfeld.
- Phasen-Checkboxen im Plan-Manager steuern Sichtbarkeit; Abbruch-/Bestand-Linienstil nur Envelope 2D.
- Wände tragen Phase (Default Neubau); Properties können sie setzen.

# Technical Design

### Current Implementation
Bereits vorhanden:
- `DisplayConfig` + `style_overlays` + `component_visibility` + `default_representation` in `src/modules/aec/engine/plan_view.rs`.
- `PlanPhase` / `PhaseFilter` (`visible_phases`, `demolition_style`, `existing_style`).
- `build_effective_rule_set` in `library.rs`: Session setzt Slot-Sichtbarkeit hart; globale `contour_hatch` wird nicht mehr gemerged.
- Plan-Manager: Overlay-Tabelle, Phasenfilter-Sektion (`aec_plan_manager.rs`).
- Regen: Envelope bevorzugt `Contour2D`, damit Phasen-Extras über Layer-Overrides gewinnen (`commands.rs`).

### Key Decisions
1. **Keine Modelländerung** der Override-Hierarchie — nur Korrektur und UX.
2. **Phase bleibt Element-Attribut**, Filter/Look bleibt an der Planart.
3. **Phasen-Look nur 2D-Kontur**, analog zur letzten Produktregel.

### Proposed Changes
- Audit Regen vs. `representation_override` und `phase_filter` (versteckte Phasen, Session 3D vs. 2D-Phasen-Look).
- Overlay-UI und Wandstil-Manager gegen die vier Korrekturen der letzten Session halten; Lücken schließen.
- Phasen-UI: Labels Neubau/Bestand/Abbruch, Zusatzstile klar als „nur Gesamtkontur 2D“.
- Properties: Phase editierbar, konsistent mit XDATA.

### File Structure
- `src/modules/aec/engine/library.rs`, `plan_view.rs`, `commands.rs`
- `src/ui/window/aec_plan_manager.rs`, `aec_wall_style_manager.rs`
- `src/app/properties.rs`, Statusleiste / `representation_popup.rs`

# Delivery Steps

### ✓ Step 1: Overlay- und Umschalter-Feinschliff absichern
Wandstil-Overlays und der 2D/3D/Alle-Umschalter verhalten sich wie in der letzten Korrekturrunde spezifiziert.

- Regen/Session-Override in `build_effective_rule_set` gegen UI-Pfad prüfen und Restlücken schließen.
- Kontur-Schraffur nur aus `style_overlays`, nicht aus `DisplayConfig.contour_hatch`.
- Overlay-Schichten nummeriert + Materialname; Material-Picker im Wandstil-Manager.
- Gezielte Lib-Tests für Session vs. Planart und Overlay-Hatch.

### ✓ Step 2: Neubau/Bestand/Abbruch durchziehen
Phasen filtern Wände in der Planart und ändern nur die 2D-Gesamtkontur für Abbruch/Bestand.

- Phase am Element (Default Neubau) in Properties und Persistenz konsistent halten.
- Plan-Manager: Sichtbarkeits-Checkboxen + Zusatzstile; Hinweis nur Envelope 2D.
- Regen: `phase_filter` vor Slot-Look; Demolition/Existing-Override nur `Contour2D`.
- Kein schichtweiser Phasen-Look; Tests für Filter und Envelope-Extras.