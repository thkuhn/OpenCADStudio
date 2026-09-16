---
sessionId: session-260912-191611-18vj
---

# Requirements

### Overview & Goals
**Stand (festgestellt):** der T-Stoß-Zyklus ist **umgesetzt**, kein offenes Feature in diesem Plan.

Geprüft gegen Code: automatischer T-Stoß (Kern-Paarung, Putz-Pocket, Gehrung) und **manuelle Schichtunterbrechungen**, die an der **Durchgangswand** persistiert werden.

### Scope
**Erledigt (Code vorhanden)**
- Automatischer T-Stoß ohne Overrides: Structural trifft Structural
- Putz-Pocket auf der Approach-Seite; Gehrung bei gleichem Material
- `LayerGapOverride` / `layer_gaps` an der Durchgangswand (`read_through_layer_gaps`, Span-Key — nicht am Stamm-Ende)

**Nicht in diesem Plan**
- Neue Feature-Arbeit an T-Stoß oder Gaps
- App-Build/Start ist Betrieb, kein Plan-Deliverable

**Out of Scope (unverändert)**
- Manuelle Junction-Overrides je Schicht, L-Ecken, N-Wege, Geschosse, Öffnungen

# Technical Design

### Current Implementation
Bereits vorhanden:
- `miter.rs`: `through_wall_cutout_footprints`, `_with_bulges`, `_with_gaps`
- `join.rs`: `layer_gaps: Vec<LayerGapOverride>`
- `commands.rs`: Gaps an der Durchgangswand (Span-Key), nicht am Stamm-Ende
- `junction_solver.rs` wendet Gaps an

### Key Decisions
1. Kern-Paarung über `function`, nicht Offset.
2. Structural → Butt auf Near-Face der Partner-Structural-Schicht.
3. Finish gleiches Material → Gehrung an Approach-Außenkante, kein Tunnel.
4. Pocket nur in überdeckten Finish-Schichten der Durchgangswand.

### File Structure
- `src/modules/aec/engine/miter.rs`
- `src/modules/aec/engine/junction_solver.rs`

Verwandte ältere Pläne: `aec-wall-style-and-layered-representation.md`, `wandstil-schicht-achsversatz.md`, `aec-wall-composite-overhaul.md`.