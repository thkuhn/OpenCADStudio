---
sessionId: session-260815-061135-2zb7
---

# Requirements

### Overview & Goals
This is the "Step 5" follow-up on branch `feature/aec-core-module`, turning the three previously-noted follow-up items into a concrete, implementable plan on top of the already-completed wall Join/Extend/grip-editing/miter work (`src/modules/aec/commands.rs`, `src/modules/aec/engine/join.rs`, `src/modules/aec/engine/miter.rs`, grip handling in `src/app/update/viewport.rs`):

1. **New "Reverse Wall Direction" action.** There is currently no way to flip a wall axis's start/end direction, which also determines the left/right side the layer stack is offset toward (`WallLayer` order in `Wall::layers`, consumed by `wall_layer_contour_polylines`/`engine/contour.rs`). Users need an explicit command/context-menu action to reverse this.
2. **More robust L/T layer matching for joins.** `join_two_walls_in_document`/`engine/miter.rs` currently matches layers between two joined walls purely by index from the reference axis outward (per the Step 3 "Key Decisions" in this same plan file). This does not account for differing layer thickness/function between the two walls, so mismatched wall types (e.g. different total thickness or layer composition) can still miter incorrectly or fall back ungracefully.
3. **Mouse-over highlight of the pick-target wall for `AEC_WALLEXTEND`/`AEC_WALLJOIN`.** The project already has a generic rollover-highlight mechanism for exactly this purpose (`CadCommand::entity_pick_highlights_hover()` in `src/command.rs`, consumed in `src/app/update/viewport.rs` and `src/app/command_driver.rs`, already used by `BlendCommand` in `src/modules/draw/modify/blend.rs` and `MatchPropCommand`), but `WallJoinCommand`/`WallExtendCommand` do not override it, so no visual feedback is shown before the user clicks the target wall.

### Scope
**In Scope:**
1. New `AEC_WALLREVERSE` command (plus a "Reverse Direction" wall context-menu entry) that reverses a wall axis's vertex order and, correspondingly, the layer stack's offset side, then regenerates the wall's derived representation.
2. Improved layer-matching strategy in the L/T miter geometry (`src/modules/aec/engine/miter.rs`): match layers by material/function and cumulative offset-from-axis instead of raw index, with a documented, tested fallback for genuinely unmatched layers.
3. Hover-highlight for `WallJoinCommand`'s and `WallExtendCommand`'s target-wall pick step, using the existing `entity_pick_highlights_hover()`/`set_hover_highlight` mechanism, restricted to wall entities only.
4. Regression coverage for all three changes.

**Out of Scope:**
- Any change to the already-fixed axis-intersection/2D-3D-sync/basic-miter behavior from Steps 1–4 of this plan (still correct, untouched).
- Reversing multiple walls at once / a bulk "reverse all" command.
- Highlighting anything beyond the single hovered target wall (e.g. no preview of the resulting joined/extended geometry beyond what already exists).

### User Stories
- As a user who drew a wall in the wrong direction (so the layers/finish face the wrong side), I want a single action to reverse the wall's direction and immediately see the layers swap sides.
- As a user joining two walls of different types (e.g. different thickness or layer composition) at an L or T corner, I want the layers to still miter sensibly by matching corresponding materials/functions, not just by position in the list.
- As a user about to pick a target wall for `AEC_WALLEXTEND`/`AEC_WALLJOIN`, I want the wall under my cursor to visually light up before I click, so I know exactly which wall will be affected.

### Functional Requirements
- A new command `AEC_WALLREVERSE` (and a "Reverse Direction" entry in the wall's right-click context menu in `src/app/view/overlay.rs`, next to "Join Walls"/"Extend Wall"/"Change Justification") picks a wall, reverses its axis vertex order, keeps the wall's absolute position and shape identical, but flips which side of the axis each layer is offset toward, and regenerates+bumps 2D/3D exactly like other wall edits.
- The miter layer-matching in `engine/miter.rs` pairs layers between two joined walls primarily by `material`/`LayerFunction` equality (when available) and, if that's ambiguous or absent, secondarily by matching offset ranges from the reference axis; layers that still can't be matched fall back to the existing single-vertex `corner_override` extension (unchanged fallback behavior, but only for the genuinely-unmatched subset instead of the whole join whenever the layer *count* differs).
- While `WallJoinCommand`/`WallExtendCommand` are awaiting the target-wall pick, hovering over a wall entity shows the standard rollover highlight (same visual as normal entity hover elsewhere), and hovering over non-wall entities/empty space shows no highlight.
- No regression to existing single-wall (non-joined/non-reversed) contour/hatch/solid generation, nor to Steps 1–4's axis-intersection, 2D/3D-sync, or basic miter fixes.

### Non-Functional Requirements
- Existing AEC test suite (`cargo test --lib aec`) remains green; new/adjusted tests are added for each of the three additions.
- No changes to the `WALL_V2` XDATA persistence format beyond what a normal wall regeneration already writes (layers array content may reorder on reverse, but its schema is unchanged).

# Technical Design

### Current Implementation (verified via code search on `feature/aec-core-module`, current state after Steps 1–4)
- `Wall`/`WallLayer` (`src/modules/aec/commands.rs`, ~lines 300–320): a wall stores `axis` vertices plus `layers: Vec<WallLayer>` (`material`, `thickness`, presumably `function`); layer offsets from the axis are computed in order by `wall_layer_contour_polylines`/`engine/contour.rs::layer_contours`, so simply reversing `axis`'s vertex order without touching `layers` would flip which physical side each layer ends up on relative to the wall's new direction — there is currently **no** command that does this (`grep` for `reverse`/`REVERSE` in `src/modules/aec` returns no wall-related hits).
- `src/modules/aec/engine/miter.rs` (added in Step 3): computes the diagonal miter line at an L/T corner and clips **matching layer footprints by index**, with an explicit fallback to the old single-vertex extension when layer *counts* differ (per this plan's own Step 3/Key Decisions); it does not look at `material`/`thickness`/function to decide which layer of wall A corresponds to which layer of wall B.
- `CadCommand::entity_pick_highlights_hover()` (`src/command.rs`, ~line 1695) defaults to `false`; `src/app/update/viewport.rs` (~lines 1929–1938) and `src/app/command_driver.rs` (~lines 632–636) already call it and call `scene.set_hover_highlight(hovered)` whenever the active command overrides it to `true` — `BlendCommand` (`src/modules/draw/modify/blend.rs`) and `MatchPropCommand` (`src/modules/draw/properties/match_prop.rs`) already override this trivially (`fn entity_pick_highlights_hover(&self) -> bool { true }` / conditionally). `WallJoinCommand`/`WallExtendCommand` in `src/modules/aec/commands.rs` have no such override today.
- The wall context menu (`src/app/view/overlay.rs`, ~lines 1063–1083) already lists `Join Walls` (`AEC_WALLJOIN`), `Extend Wall` (`AEC_WALLEXTEND`), and a `Change Justification` submenu as sibling entries — the natural place to add `Reverse Direction`.

### Key Decisions
- **Reverse is a new, small `CadCommand`, not a flag on `WallCommand`**: add `AEC_WALLREVERSE` as its own single-entity-pick command (mirroring the shape of `AEC_WALLJOIN`'s pick step), because reversing is a one-shot edit on an existing wall, not part of the drawing workflow.
- **Reversing swaps axis vertex order AND re-indexes/mirrors the layer list** so that the layer previously nearest side A of the axis is regenerated nearest side A again after the direction flip cancels it out — i.e., reverse `axis.vertices` and simultaneously reverse the sign/side convention consumed by `layer_contours`, keeping the wall's absolute footprint pixel-identical and only changing the semantic start/end and left/right assignment. This avoids introducing a new persisted concept; it's implemented as a pure transform on the existing `Wall`/`WallLayer` data followed by the existing regeneration pipeline.
- **Improved miter layer matching is a refinement inside `engine/miter.rs`, not a new module**: change the matching function to first try `material` equality (and `function` if present) between candidate layer pairs, falling back to matching by cumulative offset-distance from the axis when materials collide/are ambiguous, and only falling back to the existing per-layer "unmatched → old single-vertex extension" behavior for layers that still can't be resolved uniquely — keeping the already-tested L/T/mismatched-count code paths as the base case.
- **Hover highlight reuses the existing generic mechanism verbatim**: `WallJoinCommand`/`WallExtendCommand` override `entity_pick_highlights_hover()` to return `true` only while awaiting the target-wall pick step (not during the initial wall selection, to avoid double-highlighting), and additionally override `entity_pick_filter`-style logic (or reuse whatever existing wall-only pick-filtering helper `resolve_wall_package` already provides) so hovering a non-wall entity does not highlight it, consistent with how `on_hover_entity`/`entity_pick_acquire_hint` are already wired.

### Proposed Changes
1. **`AEC_WALLREVERSE` command** (`src/modules/aec/commands.rs`): new `struct WallReverseCommand` implementing `CadCommand` with a single entity pick (reuse `resolve_wall_package`/wall-detection helper from `WallJoinCommand`); on pick, reverse the wall's `axis` vertex order and adjust the `layers` side convention, call `regenerate_wall_representation`, and `bump_entities` with the full returned handle set (reusing the Step 2 handle-set plumbing). Register the command string and wire a `Reverse Direction` entry into the wall context menu in `src/app/view/overlay.rs` next to `Join Walls`/`Extend Wall`/`Change Justification`.
2. **Material/offset-aware layer matching** (`src/modules/aec/engine/miter.rs`): replace the index-only pairing loop with a matching pass that scores candidate pairs by `material` equality first, then by closeness of cumulative offset-from-axis, picking the best unambiguous pairing per layer; unresolved layers keep falling back to the existing single-vertex extension logic (already covered by the existing mismatched-count test) rather than a hard error.
3. **Hover highlight on `WallJoinCommand`/`WallExtendCommand`** (`src/modules/aec/commands.rs`): add `entity_pick_highlights_hover()` overrides that return `true` while the command is in its target-wall-pick phase (mirroring `MatchPropCommand`'s phase-conditional pattern), `false` otherwise; verify via `src/app/update/viewport.rs`'s existing `on_hover_entity`/`set_hover_highlight` plumbing that only wall entities light up (reject/pass-through non-wall hovers the same way the pick-resolution logic already does for clicks).

### File Structure
- `src/modules/aec/commands.rs` (modified): new `WallReverseCommand`, `entity_pick_highlights_hover()` overrides on `WallJoinCommand`/`WallExtendCommand`, plus the `AEC_WALLREVERSE` command registration.
- `src/modules/aec/engine/miter.rs` (modified): material/offset-aware layer matching function replacing the index-only pairing.
- `src/app/view/overlay.rs` (modified): new `Reverse Direction` context-menu entry dispatching `Message::Command("AEC_WALLREVERSE".to_string())`.

### Risks
- **Reversing a wall that is already joined to neighbors at its endpoints** could desynchronize the joined corner's stored axis intersection — mitigated by re-running the existing auto-join detection (`try_auto_join_nearby_walls`) after a reverse, exactly as already done after grip edits and finish-drawing.
- **Material-based layer matching can still be ambiguous** (e.g. two layers of the same material at different depths) — mitigated by using offset-distance as a tie-breaker and by keeping the existing single-vertex-extension fallback for any layer that can't be uniquely resolved, so the change is additive/best-effort rather than a hard requirement.
- **Hover-highlighting only during the pick phase, not wall selection**, requires precise phase tracking inside `WallJoinCommand`/`WallExtendCommand` — mitigated by mirroring `MatchPropCommand`'s existing `!self.phase1_done()`-style conditional pattern rather than inventing new state.

# Delivery Steps

### ✓ Step 1: Fix AEC_WALLEXTEND to snap to the target wall's axis intersection
*(already implemented, kept for history)*

### ✓ Step 2: Make wall regeneration return its full touched-handle set and bump 2D/3D together
*(already implemented, kept for history)*

### ✓ Step 3: Implement per-layer miter geometry for L/T wall joins
*(already implemented, kept for history)*

### ✓ Step 4: Regression-test the full join/extend/grip-edit flow end to end
*(already implemented, kept for history)*

### ✓ Step 5: Add a "Reverse Wall Direction" command
A user can select a wall and reverse its axis direction; the wall's footprint stays identical but the layer stack's side assignment flips consistently in both 2D and 3D.
- Add `WallReverseCommand` (`AEC_WALLREVERSE`) in `src/modules/aec/commands.rs`, reusing the existing wall entity-pick/`resolve_wall_package` pattern from `WallJoinCommand`.
- Reverse the picked wall's `axis` vertex order and adjust the `layers` side convention so the visible footprint is unchanged but left/right assignment flips.
- Call `regenerate_wall_representation` and bump the full returned handle set (reusing the Step 2 plumbing) so 2D and 3D update together.
- Re-run `try_auto_join_nearby_walls` after the reverse so any existing joined corners stay consistent.
- Add a `Reverse Direction` entry to the wall context menu in `src/app/view/overlay.rs`, next to `Join Walls`/`Extend Wall`/`Change Justification`.
- Add unit tests covering: reversing a simple wall preserves its footprint but flips layer side assignment; reversing a wall already joined at a corner keeps the join intersection correct.

### ✓ Step 6: Improve L/T layer matching in the miter geometry using material/offset
Joined walls of differing thickness or layer composition still miter their corresponding layers correctly instead of relying on raw list position.
- In `src/modules/aec/engine/miter.rs`, replace the index-only layer pairing with a matching pass that first pairs layers by `material` equality, then by closest cumulative offset-from-axis for ties/ambiguous cases.
- Keep the existing single-vertex-extension fallback for any layer that still can't be uniquely resolved (reusing the already-tested mismatched-count code path).
- Add unit tests covering: two walls with the same layer composition but different order still match correctly by material; two walls with genuinely different layer counts/materials fall back gracefully for the unmatched layers only.

### ✓ Step 7: Add hover highlight for the AEC_WALLEXTEND/AEC_WALLJOIN target-wall pick step
While choosing the target wall for an extend or join operation, the wall currently under the cursor visually highlights before the user clicks.
- Override `entity_pick_highlights_hover()` on `WallJoinCommand` and `WallExtendCommand` (`src/modules/aec/commands.rs`) to return `true` only while the command is awaiting the target-wall pick (mirroring `MatchPropCommand`'s phase-conditional pattern), `false` during the initial wall-selection step.
- Verify via `src/app/update/viewport.rs`'s existing `on_hover_entity`/`set_hover_highlight` wiring that only wall entities are highlighted; non-wall hovers show no highlight.
- Add/adjust tests or code-reviewed verification confirming no regression to `BlendCommand`/`MatchPropCommand`'s existing hover-highlight behavior from the shared trait method.

### ✓ Step 8: Regression-test reverse, improved miter matching, and hover highlight together
All three Step 5 additions work correctly together and existing wall functionality (join/extend/grip-edit/basic miter) is unaffected.
- Run `cargo test --lib aec` to confirm all previously passing wall tests remain green alongside the new ones.
- Run `cargo build --lib` to confirm the codebase compiles cleanly.
- Manually verify (via code review of the wiring, consistent with prior sessions' verification approach) that the context-menu `Reverse Direction` entry and hover highlight behave correctly end to end in the running application.

### ✓ Step 9: Make "Wall Extend" default to extend-to-wall with an explicit point mode
Previously `AEC_WALLEXTEND` defaulted to an ambiguous auto-detect (miss → point extend), which behaved like "extend to point" by default and didn't show a proper point-snap preview. Changed so the default is "extend to wall" (with the existing wall mouse-over highlight), and "extend to point" (with the normal point-snap preview) is only entered via an explicit `Point`/`Wall` command-line option toggle.
- Replaced the internal `target_is_wall: bool` auto-detect flag on `WallExtendCommand` (`src/modules/aec/commands.rs`) with an explicit `WallExtendMode { ToWall, ToPoint }`, defaulting to `ToWall`.
- In `ToWall` mode: `needs_entity_pick()` stays `true` and `entity_pick_highlights_hover()` stays `true`, so the target wall lights up on hover exactly as before; a miss or re-click on the source wall no longer silently falls back to a point extend — it just keeps waiting.
- In `ToPoint` mode (entered via typing `P`/the `Point` option): `needs_entity_pick()` returns `false`, so the viewport falls back to the normal point-picking flow with its usual point-snap preview (same as e.g. `LINE`/`PLINE`), and `on_point` dispatches the existing `PT|` path.
- Typing `W` while in `ToPoint` mode switches back to `ToWall` mode.
- Updated/added unit tests: removed the now-invalid "empty click falls back to point extend" test (replaced with "empty click stays in ToWall mode, no fallback"), added a test that switching to `Point` mode via `on_text_input` and then picking a point dispatches `PT|`.
- `cargo test --lib aec`: 114 passed, 0 failed. `cargo build --bin OpenCADStudio`: succeeded (only pre-existing warnings).