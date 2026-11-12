use super::*;

/// Error returned by [`regenerate_wall_representation`]; never a panic —
/// malformed axis/XDATA just leaves the wall without a rebuilt
/// representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallRegenError {
    /// `wall_handle` doesn't resolve to an entity carrying `WALL`
    /// XDATA.
    NotAWall,
    /// The wall has no material layers to build a representation from.
    NoLayers,
    /// The axis geometry didn't yield usable layer contours (e.g. fewer
    /// than two vertices).
    NoContours,
}

/// (Re)build the visible 2D contour + hatch and 3D solid representation for
/// the wall at `wall_handle`, from its axis polyline + `WALL`
/// XDATA.
///
/// The axis polyline is moved onto the invisible `AEC_WALL_AXIS` layer (kept
/// as reference geometry for `AEC_ROOM` / loop detection and as the XDATA
/// carrier). Every entity handle previously recorded in `derived_handles` is
/// erased first, so calling this repeatedly on the same wall never
/// accumulates duplicates. Derived handles are always persisted on the
/// wall's `WALL` XDATA record so subsequent regenerations can erase them.
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation(
    scene: &mut Scene,
    wall_handle: Handle,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    regenerate_wall_representation_with_corner(scene, wall_handle, None, None, library_override)
}

/// Like [`regenerate_wall_representation`], but honors per-slot visibility
/// from `rules` (see [`engine::display_component::ComponentRuleSet`]):
/// a `WALL_REP` child whose corresponding [`engine::display_component::WallComponentSlot`]
/// is hidden simply isn't created, instead of a global LOD switch. `None`
/// (or a default rule set) reproduces today's behavior exactly (every slot
/// defaults to visible).
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_rules(
    scene: &mut Scene,
    wall_handle: Handle,
    rules: Option<&engine::display_component::ComponentRuleSet>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    regenerate_wall_representation_with_corner_and_rules(
        scene,
        wall_handle,
        None,
        None,
        rules,
        library_override,
    )
}

/// Like [`regenerate_wall_representation_with_rules`], but additionally
/// honors a `DisplayConfig`'s `style_substitutions` map (source wall style
/// id -> target wall style id, see [`engine::plan_view::DisplayConfig`]):
/// when the wall's own style id has an entry here, the *style* (material/
/// hatch/color) of each layer is taken from the corresponding layer (by
/// index) of the target wall style, while axis, thickness and layer count
/// are always derived from the wall's own (unchanged) layers. `None`
/// reproduces today's behavior exactly (no substitution applied).
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_rules_and_substitutions(
    scene: &mut Scene,
    wall_handle: Handle,
    rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    regenerate_wall_representation_with_corner_rules_and_substitutions(
        scene,
        wall_handle,
        None,
        None,
        rules,
        style_substitutions,
        library_override,
    )
}

/// Rebuild a wall after its axis vertices changed (grip / stretch) and
/// re-resolve nearby L/T/N junctions. Returns every axis + derived handle
/// that the scene tessellation must refresh.
pub fn refresh_wall_after_axis_edit(
    scene: &mut Scene,
    wall_handle: Handle,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Vec<Handle> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let mut touched = try_auto_join_nearby_walls(
        scene,
        wall_handle,
        library_override,
        display_rules,
        style_substitutions,
    );
    if !touched.iter().any(|h| *h == wall_handle) {
        match regenerate_wall_representation_with_rules_and_substitutions(
            scene,
            wall_handle,
            display_rules,
            style_substitutions,
            library_override,
        ) {
            Ok(t) => touched.extend(t),
            Err(_) => touched.push(wall_handle),
        }
    }
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    // Resident wire cache can keep the previous 2D outline if only
    // Added/Removed deltas are replayed after a delete+recreate. Force a
    // full rebuild so the contour on screen matches the new axis.
    scene.bump_geometry();
    touched
}

/// Move a wall's **axis** vertices (not its visible contour/hatch/solid
/// children) by `delta` wherever `in_win` reports the vertex as selected,
/// then regenerate + re-join via [`refresh_wall_after_axis_edit`].
///
/// The wall axis lives on the invisible `AEC_WALL_AXIS` layer, so commands
/// like STRETCH that hit-test only visible geometry never see it — they only
/// ever get a handle to the derived contour. Moving that derived contour's
/// own vertices instead of the axis is reverted by the very next
/// regeneration (which rebuilds the contour from the unchanged axis), so any
/// wall-aware caller must resolve to the axis and move *it* first. This
/// function is that shared operation; used by STRETCH in
/// `command_driver.rs`.
///
/// Returns `None` if `owner` isn't a wall, has no axis vertices, or none of
/// them fall inside the window (no-op). Otherwise returns every axis +
/// derived handle touched by the regeneration, exactly like
/// [`refresh_wall_after_axis_edit`].
pub fn stretch_wall_axis_in_window(
    scene: &mut Scene,
    owner: Handle,
    in_win: impl Fn(f64, f64) -> bool,
    delta: DVec3,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Option<Vec<Handle>> {
    let axis_vertices = get_wall_vertices(scene, owner);
    if axis_vertices.is_empty() {
        return None;
    }
    let mut moved = false;
    let new_vertices: Vec<DVec3> = axis_vertices
        .iter()
        .map(|v| {
            if in_win(v.x, v.y) {
                moved = true;
                DVec3::new(v.x + delta.x, v.y + delta.y, v.z + delta.z)
            } else {
                *v
            }
        })
        .collect();
    if !moved {
        return None;
    }
    update_wall_vertices(scene, owner, &new_vertices);
    Some(refresh_wall_after_axis_edit(
        scene,
        owner,
        library_override,
        display_rules,
        style_substitutions,
    ))
}

/// Like [`regenerate_wall_representation`], but lets a caller supply a
/// corner-extension hint: `(vertex_index, extended_position)` moves one axis
/// vertex further out — past a joined corner and into the other wall's
/// footprint — for contour/hatch/solid generation only. The persisted axis
/// polyline (and therefore `AEC_ROOM` loop detection, which depends on the
/// exact trimmed corner) is left untouched; only the *visible representation*
/// uses the extended point.
///
/// When `join_miter` is supplied, matched layers (by material/function then
/// offset-from-axis) are rebuilt with a true diagonal miter against the other
/// wall's corresponding layer; unmatched layers fall back to the single-vertex
/// `corner_override` extension.
///
/// Used by [`join_two_walls_in_document`] so an L/T join's two walls share a
/// clean mitered corner instead of leaving a seam where their
/// independently-capped rectangles merely touch.
///
/// On success returns the axis handle plus every newly created derived
/// (contour/hatch/solid) handle, so callers (e.g. grip-release) can bump 2D
/// and 3D representations together.
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_corner(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    join_miter: Option<&engine::miter::JoinMiterContext>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    regenerate_wall_representation_with_corner_and_rules(
        scene,
        wall_handle,
        corner_override,
        join_miter,
        None,
        library_override,
    )
}

/// Like [`regenerate_wall_representation_with_corner`], but also honors
/// per-slot visibility from `rules` (see [`regenerate_wall_representation_with_rules`]).
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_corner_and_rules(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    join_miter: Option<&engine::miter::JoinMiterContext>,
    rules: Option<&engine::display_component::ComponentRuleSet>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    regenerate_wall_representation_with_corner_rules_and_substitutions(
        scene,
        wall_handle,
        corner_override,
        join_miter,
        rules,
        None,
        library_override,
    )
}

/// Like [`regenerate_wall_representation_with_corner_and_rules`], but also
/// honors `style_substitutions` (see
/// [`regenerate_wall_representation_with_rules_and_substitutions`]).
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_corner_rules_and_substitutions(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    join_miter: Option<&engine::miter::JoinMiterContext>,
    rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    regenerate_wall_representation_inner(
        scene,
        wall_handle,
        corner_override,
        join_miter,
        None,
        rules,
        style_substitutions,
        library_override,
        false,
    )
}

/// Like [`regenerate_wall_representation_with_corner`], but takes precomputed
/// per-layer miter footprints (from N-way junction resolution). `None` entries
/// fall back to `corner_override` / base contours exactly as unmatched layers do.
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_precomputed_miters(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    mitered_footprints: &[Option<Vec<(f64, f64)>>],
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    regenerate_wall_representation_with_precomputed_miters_and_rules(
        scene,
        wall_handle,
        corner_override,
        mitered_footprints,
        None,
        library_override,
    )
}

/// Like [`regenerate_wall_representation_with_precomputed_miters`], but also
/// honors per-slot visibility from `rules` (see
/// [`regenerate_wall_representation_with_rules`]).
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_precomputed_miters_and_rules(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    mitered_footprints: &[Option<Vec<(f64, f64)>>],
    rules: Option<&engine::display_component::ComponentRuleSet>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    regenerate_wall_representation_with_precomputed_miters_rules_and_substitutions(
        scene,
        wall_handle,
        corner_override,
        mitered_footprints,
        rules,
        None,
        library_override,
    )
}

/// Like [`regenerate_wall_representation_with_precomputed_miters_and_rules`],
/// but also honors `style_substitutions` (see
/// [`regenerate_wall_representation_with_rules_and_substitutions`]).
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_precomputed_miters_rules_and_substitutions(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    mitered_footprints: &[Option<Vec<(f64, f64)>>],
    rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    regenerate_wall_representation_inner(
        scene,
        wall_handle,
        corner_override,
        None,
        Some(mitered_footprints),
        rules,
        style_substitutions,
        library_override,
        false,
    )
}

/// Like [`regenerate_wall_representation_with_precomputed_miters_rules_and_substitutions`],
/// but treats `mitered_footprints` as already composed across both ends and
/// any through-span cutouts. Skips the O(N) peer/span rescan that would
/// otherwise walk every wall in the document.
pub fn regenerate_wall_representation_with_complete_network_miters(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    mitered_footprints: &[Option<Vec<(f64, f64)>>],
    rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    regenerate_wall_representation_inner(
        scene,
        wall_handle,
        corner_override,
        None,
        Some(mitered_footprints),
        rules,
        style_substitutions,
        library_override,
        true,
    )
}

/// Changes an existing `WALL` wall's justification (Interior/Center/
/// Exterior), shifting its axis polyline sideways by the delta between the
/// old and new justification offsets (same `WallJustification::offset` math
/// used by [`WallCommand::build_entity`]), then regenerates its
/// contour/hatch/solid representation. No-op (returns `false`) if
/// `wall_handle` doesn't carry a `WALL` record.
pub fn change_wall_justification(
    scene: &mut Scene,
    wall_handle: Handle,
    new_justification: WallJustification,
    library_override: Option<&StyleLibrary>,
) -> bool {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(v2) = wall_from_entity(entity) else {
        return false;
    };

    let total_thickness = v2.total_thickness();
    let old_offset = v2.justification.offset(total_thickness);
    let new_offset = new_justification.offset(total_thickness);
    let delta = new_offset - old_offset;

    if delta.abs() > 1e-9 {
        let vertices = get_wall_vertices(scene, wall_handle);
        if vertices.len() >= 2 {
            let points: Vec<(f64, f64)> = vertices.iter().map(|v| (v.x, v.y)).collect();
            let directions = engine::get_offset_directions(&points);
            let shifted: Vec<DVec3> = points
                .iter()
                .zip(directions.iter())
                .map(|(&(x, y), &(dx, dy))| DVec3::new(x + dx * delta, y + dy * delta, 0.0))
                .collect();
            update_wall_vertices(scene, wall_handle, &shifted);
        }
    }

    let mut record = ExtendedDataRecord::new(AEC_APPID);
    let mut wall = v2;
    wall.justification = new_justification;
    record.values = wall_record_for_wall(&wall);
    if !write_aec_record(&mut scene.document, wall_handle, record) {
        return false;
    }

    let _ = regenerate_wall_representation(scene, wall_handle, library_override);
    true
}
