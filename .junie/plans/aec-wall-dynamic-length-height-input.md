---
sessionId: session-260815-061135-2zb7
---

# Requirements

### Overview & Goals
This is a bugfix follow-up on branch `feature/aec-core-module`, targeting three concrete defects in the already-implemented wall Join/Extend/grip-editing subsystem (`src/modules/aec/commands.rs`, `src/modules/aec/engine/join.rs`, grip handling in `src/app/update/viewport.rs`).

1. **`AEC_WALLEXTEND` to a target wall uses the clicked point instead of the axis intersection.** `WallExtendCommand`/`aec_wallextend_do` (`src/modules/aec/commands.rs`) has two paths: an explicit `WALL` mode (type `W`, pick target wall — already correctly calls `join::join_wall_axes` for the crossing point) and a default `PT` mode (plain click — projects the raw clicked point onto the wall's own direction, ignoring the target wall entirely). Since `needs_entity_pick()` only returns `true` once `target_is_wall` is already set, an ordinary click near another wall always falls into `PT` mode. This is the root cause of the wrong-endpoint bug.
2. **2D and 3D fall out of sync after moving a wall endpoint (grip) or `AEC_WALLEXTEND`.** Grip release in `src/app/update/viewport.rs` (~line 2757) calls `regenerate_wall_representation` for edited wall axes, then `bump_entities` only for the original handle list — newly created derived contour/hatch/`Solid3D` entities are not included, so the 3D mesh cache and the 2D redraw are not guaranteed to refresh from the same change set. `aec_wallextend_do` has the same gap.
3. **L/T wall joins don't visually merge overlapping layers.** `regenerate_wall_representation_with_corner` accepts a single-vertex `corner_override` hint, but each wall's layers are still contoured independently — there is no shared-seam/miter computation between the two walls' layer contours, so joins show a visible seam or incorrect overlap instead of a clean mitered corner with the diagonal as the (invisible) divider.


### Scope
**In Scope:**
1. Fix `WallExtendCommand`/`aec_wallextend_do` so extending toward another wall always resolves to the axis intersection via `join::join_wall_axes`, never the raw clicked point.
2. Make `regenerate_wall_representation*` call sites (grip release, `aec_wallextend_do`, join) bump/notify all newly created derived entities together with the axis, so 2D and 3D refresh from the same complete change set.
3. Implement a mitered corner for `AEC_WALLJOIN`/auto-snap-join: matching layer footprints between the two walls are clipped to the corner's diagonal so they connect without a visible seam.
4. Regression coverage for `join_wall_axes`, `aec_wallextend_do`, and the new miter-clipping helper.

**Out of Scope:**
- Any change to the interactive front-ends of `AEC_WALLJOIN`/`AEC_WALLEXTEND` beyond the auto-detect-target-wall fix (prompts, options, dispatch format unchanged).
- Dynamic length/height in-viewport input (separate, already-completed workstream).
- Multi-wall (3+) junction miter joints; this iteration covers pairwise L/T joins only, matching `join_wall_axes`'s existing scope.

### User Stories
- As a user extending a wall toward another wall, I want the wall to stop exactly at the other wall's axis, not at wherever I happened to click on it.
- As a user dragging a wall endpoint or extending a wall, I want the 2D plan view and the 3D model to always show the same, correct result immediately after I finish the edit.
- As a user joining two walls at a corner, I want the layers (outer render, insulation, inner render, etc.) to visually connect across the corner without a seam, with the corner's diagonal acting as the implicit boundary between the two walls' material regions.

### Functional Requirements
- `AEC_WALLEXTEND`: clicking near an existing wall's axis (within the same pick tolerance used elsewhere) while specifying the extend target automatically extends to the intersection point with that wall's axis, without requiring the explicit `W` keystroke; explicit `W` + pick continues to work identically.
- After any wall endpoint move (grip) or `AEC_WALLEXTEND`, the regenerated contour/hatch/solid entities are included in the same `bump_entities`/change-notification call as the axis update, so both 2D and 3D views reflect the new geometry after a single redraw with no stale intermediate state.
- `AEC_WALLJOIN` (and the auto-snap-join-while-drawing variant) produces, for an L corner and a T corner, layer footprints that touch/overlap cleanly at the corner with no visible gap or crossing seam between matching layers of the two walls; the shared boundary between the two walls' layers is the corner's diagonal.
- No regression to existing single-wall (non-joined) contour/hatch/solid generation.

### Non-Functional Requirements
- Existing AEC test suite (`cargo test --lib aec`) remains green; new/adjusted tests are added for each of the three fixes.
- No changes to the `WALL_V2` XDATA persistence format.

# Technical Design

### Current Implementation (verified via code search on `feature/aec-core-module`)
- `WallExtendCommand`/`aec_wallextend_do` (`src/modules/aec/commands.rs`, ~lines 2636–2824): `needs_entity_pick()` returns `true` only once `target_is_wall` is already `true` (set by typing `W`); a plain click always goes through `on_point` → `PT|x|y|z` → the code block at ~line 2751 that projects the clicked point onto the wall's own direction, ignoring any wall under the cursor. The `WALL|target` branch (~line 2800) already correctly uses `join::join_wall_axes(&axis, &axis_b)` for the intersection.
- Grip release handling in `src/app/update/viewport.rs` (~lines 2730–2777): iterates `handles` (moved/edited entities from the grip drag), calls `regenerate_wall_representation` for each wall axis found among them, then builds `changes`/`bump_entities` strictly from that same `handles` list — the derived entities created inside `regenerate_wall_representation` are never added to `changes`.
- `regenerate_wall_representation_with_corner` (`src/modules/aec/commands.rs`, ~lines 758–920+) returns `Result<(), WallRegenError>` — it does not report which handles it created/erased, so callers can't bump them.
- `join::join_wall_axes` (`src/modules/aec/engine/join.rs`) only computes trimmed/extended axis vertices and a `JoinKind` (L/T); it has no per-layer footprint or miter geometry — layer-level contour generation in `wall_layer_contour_polylines`/`engine/contour.rs::layer_contours` runs per-wall, independently, bridged only by the single-vertex `corner_override` hint, which is insufficient for a true per-layer miter.

### Key Decisions
- **Fix 1 is a targeted change to pick-resolution, not a new command**: make `WallExtendCommand::needs_entity_pick()` always return `true` once a wall is selected, so every subsequent click resolves to an entity pick first; if it resolves to a different wall (via `resolve_wall_package`), dispatch through the existing `WALL|target` path; only fall back to `PT` semantics when the click hits empty space/a non-wall entity. Reuses the already-correct `join_wall_axes` logic instead of new geometry code.
- **Fix 2 is solved by making `regenerate_wall_representation*` return the full set of touched handles** (newly created derived handles plus the axis handle), and updating every call site (grip release, `aec_wallextend_do`, `join_two_walls_in_document`) to fold that set into the single `bump_entities` call for that user action.
- **Fix 3 is implemented as a new geometry helper in `engine/join.rs` (or a new `engine/miter.rs`)** that, given both walls' resolved layers and the join corner, computes a shared miter line per matching layer pair and clips both walls' footprints against it — a pure, testable geometry function consistent with `join_wall_axes`'s style, wired into `regenerate_wall_representation_with_corner`'s footprint loop.
- **Layer matching strategy for the miter**: match layers between the two joined walls by index from the reference axis outward (consistent with existing layer ordering), falling back to the existing single-vertex `corner_override` extension for any layer pair that can't be matched (documented edge case, not a hard error).

### Proposed Changes
1. **`WallExtendCommand` auto-detect target wall** (`src/modules/aec/commands.rs`): change `needs_entity_pick()` to always pick-first once a wall is chosen; in `on_entity_pick`, when a wall is already selected and the picked handle resolves to a different wall (via `resolve_wall_package`), dispatch `AEC_WALLEXTEND_DO wall|WALL|target`; when the pick misses any entity, fall back to `CmdResult::NeedPoint` → existing `PT` branch. Keep the `W` option for discoverability but no longer require it for the common case.
2. **Handle-set return from wall regeneration** (`src/modules/aec/commands.rs`): change `regenerate_wall_representation`/`regenerate_wall_representation_with_corner` to return `Result<Vec<Handle>, WallRegenError>` (axis handle + all newly created derived handles), update internal callers (`join_two_walls_in_document`, `aec_wallextend_do`), and update `src/app/update/viewport.rs`'s grip-release block to collect these handles across all edited walls and include them in the single `bump_entities` call.
3. **Per-layer miter geometry** (`src/modules/aec/engine/join.rs` or new `engine/miter.rs`): add a helper that computes the diagonal miter line at the corner and clips matching layer footprints from both walls against it; wire it into `regenerate_wall_representation_with_corner`'s per-layer footprint loop (~line 832+) when a join context is present, replacing the single-vertex extension for the layers involved.
4. Update `join_two_walls_in_document` to call the new miter helper for both walls, keeping the persisted axis vertices (used by `AEC_ROOM` loop detection) exactly as `join_wall_axes` computes them today — only the visible footprint geometry changes.

### File Structure
- `src/modules/aec/commands.rs` (modified): `WallExtendCommand` pick logic, `regenerate_wall_representation`/`_with_corner` return type change and all call sites, `aec_wallextend_do`, `join_two_walls_in_document`.
- `src/modules/aec/engine/join.rs` (modified) or new `src/modules/aec/engine/miter.rs`: new miter-footprint geometry helper + unit tests.
- `src/app/update/viewport.rs` (modified): grip-release block collects and bumps the full returned handle set from wall regeneration.

### Risks
- **Auto-detecting the target wall on a plain click could misfire near wall-dense areas** (snapping to an unintended nearby wall instead of extending to open space) — mitigated by only triggering the `WALL` path when the pick genuinely resolves to a wall entity under the cursor, keeping the explicit `PT` fallback otherwise.
- **Changing `regenerate_wall_representation`'s return type is a signature-breaking change** touching multiple call sites — mitigated by doing this as an isolated, mechanical step verified by `cargo test --lib aec` before moving to the miter geometry work.
- **Per-layer miter geometry is the most complex, novel geometry code in this plan** (degenerate/very-different-layer-count cases, near-parallel or acute corners) — mitigated by scoping it to the already-classified `JoinKind::L`/`JoinKind::T` cases only, with unit tests for both, and falling back to the existing single-vertex extension for unmatched layer pairs.

# Delivery Steps

### ✓ Step 1: Fix AEC_WALLEXTEND to snap to the target wall's axis intersection
Extending a wall toward another wall's geometry always stops at the true intersection of the two axes, whether triggered by an explicit wall pick or an ordinary click landing on the target wall.
- Change `WallExtendCommand::needs_entity_pick`/`on_entity_pick` in `src/modules/aec/commands.rs` so that, once a wall is selected, the next click is resolved as an entity pick first; if it resolves (via `resolve_wall_package`) to a different wall, dispatch through the existing `WALL|target` branch.
- Keep a fallback to the existing `PT|x|y|z` point-projection branch when the click doesn't land on another wall.
- Add/adjust unit tests covering: click directly on a crossing wall extends to the intersection; click on empty space still extends along the wall's own direction as before.

### ✓ Step 2: Make wall regeneration return its full touched-handle set and bump 2D/3D together
After a grip-edited wall endpoint or an `AEC_WALLEXTEND`, both the 2D plan view and the 3D model refresh from the same complete set of changed entities in one step.
- Change `regenerate_wall_representation`/`regenerate_wall_representation_with_corner` in `src/modules/aec/commands.rs` to return `Result<Vec<Handle>, WallRegenError>` including the axis handle plus every newly created derived (contour/hatch/solid) handle.
- Update all internal callers (`aec_wallextend_do`, `join_two_walls_in_document`) to use the returned handles.
- Update the grip-release block in `src/app/update/viewport.rs` (~line 2757) to collect the returned handles for every edited wall and include them in the single `bump_entities` call alongside the axis handles.
- Add/adjust tests confirming the returned handle vector contains the expected derived entities after a regeneration call.

### ✓ Step 3: Implement per-layer miter geometry for L/T wall joins
Joined walls show their matching layers connecting cleanly across the corner, with the corner's diagonal as the implicit boundary instead of a visible seam.
- Add a new geometry helper (in `src/modules/aec/engine/join.rs` or a new `src/modules/aec/engine/miter.rs`) that computes the diagonal miter line at an L or T corner and clips each wall's matching layer footprints against it.
- Wire the helper into `regenerate_wall_representation_with_corner`'s per-layer footprint construction (`src/modules/aec/commands.rs`, ~line 832+), replacing the single-vertex `corner_override` extension for layers that can be matched between the two joined walls.
- Update `join_two_walls_in_document` to pass both walls' resolved layers into the new helper so both sides of the join are regenerated with mitered footprints.
- Add unit tests for the miter helper covering an L corner, a T corner, and a mismatched-layer-count fallback case.

### ✓ Step 4: Regression-test the full join/extend/grip-edit flow end to end
All three fixes work together correctly and existing wall functionality is unaffected.
- Run `cargo test --lib aec` to confirm existing wall drawing/join/extend/style/regeneration tests remain green after the signature and geometry changes.
- Add an integration-style test that joins two walls at an L corner and asserts the resulting layer footprints from both walls share the same boundary/no gap, and one for a T corner.
- Add a test exercising `AEC_WALLEXTEND` toward a picked target wall end-to-end confirming the resulting axis endpoint matches `join_wall_axes`'s computed intersection, not the originally clicked point.