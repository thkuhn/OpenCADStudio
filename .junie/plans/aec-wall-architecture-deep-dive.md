---
sessionId: session-260815-191831-1p7r
---

# Follow-up: N-way wall junction hardening ("Punkt 1")

The owner-index/`.ocsproj` plan above (all 8 steps) is fully implemented and committed. This section is a **new** follow-up plan for "Punkt 1" from the last open-items review: the N-way wall junction fix (3+ walls meeting at one point).

### Investigation finding
Code search shows the core N-way join/miter machinery **already exists and is tested**, contrary to the last review's "teilweise" (partial) label:
- `join::detect_junctions()` (`src/modules/aec/engine/join.rs`) clusters wall endpoints/through-hits into a `Junction` with `JunctionRole::Endpoint`/`Through` per participant, already covered by tests `detect_junctions_x_crossing_four_endpoints`, `detect_junctions_t_with_third_endpoint`, `detect_junctions_simple_l_is_two_participants`.
- `miter::mitered_junction_layer_footprints()` + `junction_wall_geoms()` (`src/modules/aec/engine/miter.rs`) order participants angularly and compute clipped per-layer footprints for every wall in the junction, not just pairs.
- `commands::join_junction_in_document()` snaps endpoints, rebuilds all participants' representations, and is called both from `try_auto_join_nearby_walls()` (new/moved wall) and from the explicit `AEC_WALLJOIN` command when 3+ walls are selected (`commands.rs:3294-3296`).

So this plan is **not** "build N-way joins from scratch" — it is a **hardening/completion pass** closing the concrete gaps found during this review:
1. `join_junction_in_document()` re-runs `detect_junctions()` and always takes `max_by_key(participants.len())` — if a wall's *other* end also sits at a (smaller) junction, correctness depends entirely on the caller (`try_auto_join_nearby_walls`) restricting `handles` per-junction first; this coupling is implicit and untested for a wall touching two different multi-wall junctions at its two ends in a single edit.
2. No test coverage exists for **acute/obtuse angle robustness** of the angular-ordering + clipping in `mitered_junction_layer_footprints` (e.g. near-180° or near-0° angles between participants), nor for junctions where participants have **different layer counts/materials** (3+ walls with dissimilar wall styles meeting at one point) — `match_layer_indices` is only proven for the 2-wall case.
3. Interactive **grip/vertex-edit** of an existing wall endpoint that is already part of a 3+ junction is not verified to re-run `join_junction_in_document` for the *whole* junction (as opposed to a pairwise re-join with only the nearest neighbor), risking a stale/broken miter on the third+ wall after a drag.
4. No hover-highlight/UI feedback exists specifically for N-way junction targets during `AEC_WALLJOIN` (today's hover highlight, added for pairwise join/extend, does not indicate "this will form/extend a 3+ way junction").

# Requirements

### Overview & Goals
Harden and complete the already-existing N-way wall junction (3+ walls meeting at a point) support so it is verifiably correct and consistent across creation, interactive editing (grips), and undo — without rebuilding the core algorithm, which already exists (`detect_junctions`, `mitered_junction_layer_footprints`, `join_junction_in_document`).

### Scope
**In Scope:**
1. Make `join_junction_in_document()`'s junction selection explicit and safe when a wall's two ends belong to two different multi-wall junctions in the same edit, instead of implicitly relying on caller-side handle filtering.
2. Add angle-robustness handling to `mitered_junction_layer_footprints`/angular ordering for near-degenerate angles (near-0°/near-180° between participants), with a documented, tested fallback (single-vertex extension) when a clean miter isn't geometrically well-defined.
3. Extend `match_layer_indices`-based pairing to the N-way case: verify/hardened behavior when 3+ participants at one junction have differing layer counts/materials (some layers match across all participants, some don't).
4. Ensure interactive grip/vertex-edit of a wall endpoint that is part of an existing 3+ junction re-triggers a full `join_junction_in_document` rebuild for that whole junction (all participants), not just a pairwise re-join with the dragged neighbor.
5. Add hover highlighting during `AEC_WALLJOIN` for all participants of a would-be 3+ way junction (not just the single hovered target), reusing the existing `entity_pick_highlights_hover()` override pattern already used for `WallJoinCommand`/`WallExtendCommand`.

**Out of Scope:**
- Rebuilding `detect_junctions`/`mitered_junction_layer_footprints` from scratch — both already exist and are functionally correct for the basic X-crossing/T+third-wall cases per existing tests.
- Arc/bulge-aware junctions (bulge support for wall axes is a separate, not-yet-implemented topic per earlier discussions).
- Polygon-/area-based wall junctions (explicitly deferred/rejected in earlier sessions).
- Any change to the owner-index peer-link *mechanism* (`link_peers`/`unlink_peers`) itself — already implemented and tested; this plan only ensures the *geometry* rebuild stays consistent with it, and additionally verifies (see Step 1/Step 4) that peer links stay in sync after junction rebuilds — no new peer-link API is introduced.

### User Stories
- As a user drawing a wall that connects to two different existing junctions at its two ends, I want both junctions to end up correctly mitered, not just one.
- As a user with walls meeting at a very acute or very obtuse angle, I want the miter to degrade gracefully (fallback extension) instead of producing a self-intersecting or visibly wrong footprint.
- As a user with 3+ walls of different wall styles (different layer counts/materials) meeting at one point, I want each wall's own layers to still miter sensibly against whichever neighbor layers actually match.
- As a user dragging a wall endpoint grip that is part of a 3-way (or more) junction, I want all connected walls at that junction to update their miters, not just the one I directly moved.
- As a user about to join walls into a 3+ way junction, I want visual hover feedback showing all the walls that will participate, not just the one under the cursor.

### Functional Requirements
- `join_junction_in_document()` must resolve unambiguously to the intended junction even when the passed wall handles could plausibly belong to two overlapping/adjacent junctions — verified with a regression test where a wall's two ends sit at two distinct multi-wall junctions and both get rebuilt correctly in one edit pass.
- Angular ordering + clipping in `mitered_junction_layer_footprints` must not panic/produce NaN/self-intersecting output for near-0°/near-180° participant angles; a documented threshold triggers the existing single-vertex extension fallback instead.
- `match_layer_indices`/junction layer pairing must be exercised by at least one test with 3 participants that do **not** all share the same layer stack (mixed materials/layer counts), confirming unmatched layers correctly fall back per-layer rather than failing the whole junction.
- Dragging a grip on a wall endpoint that is part of a junction with 3+ participants must call `join_junction_in_document` (or equivalent) for the full participant set, verified by a regression test that drags one wall and asserts all *other* junction participants' representations were also touched/regenerated.
- Hovering a valid join target during `AEC_WALLJOIN` that would form/extend a 3+ way junction must highlight every wall that would participate, not only the directly hovered one.
- `cargo test --lib aec` must keep passing (baseline 180 at start of this plan) with new regression tests added for each of the above, none removed.

# Technical Design

### Current Implementation (verified via this session's code review)
- `join::detect_junctions()` (`src/modules/aec/engine/join.rs`) clusters wall endpoints (`JunctionRole::Endpoint`) and through-hits (`JunctionRole::Through`) within `JUNCTION_TOLERANCE`, returning `Junction { participants: Vec<Participant> }`; already tested for X-crossings (4 endpoints) and T-with-third-endpoint.
- `commands::join_junction_in_document()` (`src/modules/aec/commands.rs:3079`) re-derives axes for the given `handles`, calls `detect_junctions()` again internally, and picks `.max_by_key(|j| j.participants.len())` among multi-wall junctions — meaning **only the single largest junction found among the given handles is rebuilt per call**.
- The only caller that assembles a *restricted* handle set per junction is `commands.rs:3050-3068`, which iterates `detect_junctions(...).filter(is_multi_wall)` and calls `join_junction_in_document` once per junction, restricting `part_handles` to that junction's own participants — this pattern is correct today but is easy to break if a future call site passes an unfiltered handle list (e.g. "all walls touching this one") directly into `join_junction_in_document`, which would then silently only fix the larger of two junctions.
- `miter::mitered_junction_layer_footprints()`/`junction_wall_geoms()` (`src/modules/aec/engine/miter.rs`) order participants angularly around the shared point and clip each wall's layer footprint against its angular neighbors; `match_layer_indices()` (2-wall matcher) is reused per-neighbor-pair, but there is no existing test with 3+ participants carrying **different** layer stacks.
- `AEC_WALLJOIN`'s interactive picking (`src/app/view/overlay.rs`, `entity_pick_highlights_hover()` overrides for `WallJoinCommand`/`WallExtendCommand`) highlights only the single hovered candidate wall — there is no junction-aware "highlight the whole future cluster" behavior.
- Grip/vertex-edit dragging of a wall endpoint calls `regenerate_wall_representation()` + `try_auto_join_nearby_walls()` on the dragged wall only (see `commands.rs:3792-3793`, used by `reverse_wall_in_document` and, per the same pattern, wall vertex-move commands) — whether this correctly cascades to rebuild *every other* participant of an existing 3+ junction (not just the nearest single neighbor) has not been verified by a test.

### Key Decisions
- **Junction selection safety**: make the "restrict handles to one junction's participants before calling `join_junction_in_document`" pattern the *only* supported call convention — add an assertion/early-return (not a panic) if the passed handles yield more than one multi-wall junction, and cover the two-different-junctions-on-one-wall scenario with a dedicated test at the existing call site (`commands.rs:3050-3068`) rather than changing the function's picking heuristic.
- **Degenerate angle handling**: add an explicit minimum-angle threshold check before attempting the diagonal miter; below threshold, fall back to the already-existing single-vertex `corner_override` extension path (already used when layer matching fails) instead of introducing a new geometry special case.
- **Mixed layer-stack junctions**: no new matching algorithm — extend test coverage of the existing `match_layer_indices` to the N-way path to confirm current unmatched-layer fallback behavior is correct and intentional for 3+ dissimilar participants; only fix if a test reveals an actual bug.
- **Grip-edit cascade**: reuse `join_junction_in_document`/`try_auto_join_nearby_walls` (no new function) from the vertex-move command path, ensuring the full junction (all participants) is passed, not just the two nearest walls.
- **Hover highlight for N-way**: extend the existing `entity_pick_highlights_hover()` override mechanism (already used for `WallJoinCommand`) to compute and highlight the full prospective junction participant set via `detect_junctions()`, not just the single hovered handle.

### Components
- `src/modules/aec/engine/join.rs` — no functional change to `detect_junctions`, but a small documented guard/assertion added at the point where callers must pre-restrict handles; degenerate-angle threshold constant added here or in `miter.rs`.
- `src/modules/aec/engine/miter.rs` — degenerate-angle fallback branch in `mitered_junction_layer_footprints`; new tests for mixed-layer-stack 3-way junctions and near-degenerate angles.
- `src/modules/aec/commands.rs` — regression test added at the `join_junction_in_document`/multi-junction-per-wall call site (`commands.rs:3050-3068`); grip/vertex-move command path updated (if a gap is found) to pass the full junction participant set instead of a pairwise neighbor.
- `src/app/view/overlay.rs` — `entity_pick_highlights_hover()` override for `WallJoinCommand` extended to include all prospective junction participants.

### Risks
- The "restrict handles per junction before calling `join_junction_in_document`" convention is implicit today; enforcing it via an assertion could surface latent bugs at other call sites not yet identified — mitigated by running the full `cargo test --lib aec` suite after adding the guard, before considering the step done.
- Degenerate-angle threshold tuning is a judgment call (too low misses real edge cases, too high triggers the fallback unnecessarily for legitimate sharp corners) — mitigated by deriving the threshold from a documented geometric argument (e.g. minimum angle where the diagonal miter line remains within the layer thickness) and covering it with a boundary-value test.
- Extending hover highlighting to a full junction cluster touches interactive/overlay code shared with `WallExtendCommand` — mitigated by scoping the change strictly to the `WallJoinCommand` branch.

# Delivery Steps

### ✓ Step 1: Add a regression test proving two-junctions-on-one-wall is handled correctly
A wall whose two ends each sit at a different multi-wall junction gets both junctions correctly rebuilt by a single higher-level edit (e.g. `try_auto_join_nearby_walls`), with no participant silently skipped.
- Add a test in `src/modules/aec/commands.rs` constructing a wall with a 3-way junction at one end and a different 3-way (or 4-way) junction at the other end.
- Assert both junctions' participants are present in the returned touched-handles list and each has correctly mitered layer footprints.
- If the test reveals a real gap, fix `join_junction_in_document`'s calling convention (restrict handles per-junction, not globally) at the failing call site — do not change the picking heuristic inside `join_junction_in_document` itself.
- Additionally assert that `owner_index::peers_of()` for every participant of both junctions reflects the correct symmetric peer set after the rebuild (no stale/missing peer links left over from the two separate junctions).

### ✓ Step 2: Harden degenerate-angle handling in N-way miter
Junctions with near-0°/near-180° participant angles no longer risk NaN/self-intersecting footprints; they cleanly fall back to single-vertex extension.
- Add a minimum-angle threshold check in `mitered_junction_layer_footprints`/`junction_wall_geoms` (`src/modules/aec/engine/miter.rs`).
- Below threshold, return `None` for the affected layer(s) so the caller's existing `corner_override` single-vertex fallback applies, exactly like the current unmatched-layer fallback.
- Add regression tests with a 3-way junction at ~5° and ~175° between two participants, asserting no panic/NaN and that the fallback path is taken.

### ✓ Step 3: Add regression coverage for mixed layer-stack N-way junctions
Junctions where 3+ participants have differing layer counts/materials are proven to miter each matchable layer correctly and fall back per-layer for the rest.
- Add tests in `src/modules/aec/engine/miter.rs` with 3 participants: two sharing an identical layer stack, one with a different material/layer count.
- Assert matching layers get proper clipped footprints and non-matching layers correctly return `None` (triggering the existing single-vertex fallback), without affecting the other participants' layers.
- Fix `match_layer_indices`/`mitered_junction_layer_footprints` only if the test uncovers an actual incorrect pairing (not just an already-expected fallback).

### ✓ Step 4: Ensure grip/vertex-edit dragging cascades to the full junction
Dragging a wall endpoint that belongs to an existing 3+ way junction updates every participant's miter, not only the nearest neighbor.
- Locate the wall vertex-move/grip-edit command path and verify (via a new test) whether it calls `join_junction_in_document` with the full junction participant set or only a pairwise re-join.
- If it only does a pairwise re-join, update it to detect and pass the full junction (reusing `join::detect_junctions`/`try_auto_join_nearby_walls`), matching the pattern already used for new-wall creation.
- Add a regression test: build a 3-way junction, drag one participant's shared endpoint slightly, and assert all three participants' representations are touched and remain correctly mitered.
- Also assert `owner_index::peers_of()` stays symmetric and correct across all participants after the drag (a participant leaving the junction is unlinked, one newly joining is linked).

### ✓ Step 5: Add N-way-aware hover highlighting for `AEC_WALLJOIN`
Hovering a wall that would form/extend a 3+ way junction highlights every wall that would participate, not just the one under the cursor.
- Extend the `entity_pick_highlights_hover()` override for `WallJoinCommand` (`src/app/view/overlay.rs`) to run `join::detect_junctions()` against the current selection + hovered candidate and highlight all resulting participants.
- Keep the existing single-target highlight behavior unchanged for plain L/T (2-wall) joins.
- Verify manually via `cargo build --bin OpenCADStudio` (no automated UI test required, consistent with how the existing hover-highlight override was verified).