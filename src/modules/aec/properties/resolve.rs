use super::*;

pub fn aec_entity_title(
    scene: &crate::scene::Scene,
    handle: Handle,
    entity: &EntityType,
) -> Option<String> {
    if xdata::wall_from_entity(entity).is_some() {
        return Some(crate::tr!("aec", "wall-title"));
    }
    if slab_xdata::slab_from_entity(entity).is_some() {
        return Some(crate::tr!("aec", "slab-title"));
    }
    if room_xdata::room_from_entity(entity).is_some() {
        return Some(crate::tr!("aec", "room-title"));
    }
    if room_package::is_room_derived(entity) {
        return Some(crate::tr!("aec", "room-title"));
    }
    if let Some(opening) = slab_xdata::slab_opening_from_entity(entity) {
        let _ = opening;
        return Some(crate::tr!("aec", "slabopening-title"));
    }
    if let Some(owner) = slab_package::slab_opening_owner_if_any(scene, handle) {
        if scene
            .document
            .get_entity(owner)
            .and_then(slab_xdata::slab_opening_from_entity)
            .is_some()
        {
            return Some(crate::tr!("aec", "slabopening-title"));
        }
    }
    if slab_package::is_slab_derived(scene, handle)
        && scene
            .document
            .get_entity(slab_package::resolve_slab_package(scene, handle))
            .and_then(slab_xdata::slab_from_entity)
            .is_some()
    {
        return Some(crate::tr!("aec", "slab-title"));
    }
    if let Some(opening) = opening_xdata::opening_from_entity(entity, handle) {
        let name = match opening.kind {
            OpeningKind::Window => crate::tr!("aec", "window-title"),
            OpeningKind::Door => crate::tr!("aec", "door-title"),
            OpeningKind::Breakthrough => crate::tr!("aec", "opening-title"),
        };
        return Some(name);
    }
    if let Some(owner) = opening_display::opening_owner_if_any(scene, handle) {
        if let Some(owner_ent) = scene.document.get_entity(owner) {
            if let Some(opening) = opening_xdata::opening_from_entity(owner_ent, owner) {
                let name = match opening.kind {
                    OpeningKind::Window => crate::tr!("aec", "window-title"),
                    OpeningKind::Door => crate::tr!("aec", "door-title"),
                    OpeningKind::Breakthrough => crate::tr!("aec", "opening-title"),
                };
                return Some(name);
            }
        }
    }
    if storey_xdata::storey_from_entity(entity).is_some() {
        return Some(crate::tr!("aec", "storey-title"));
    }
    if crate::modules::aec::project::preview::control_plane_from_entity(entity).is_some() {
        return Some(crate::tr!("aec", "control-plane"));
    }
    if crate::modules::aec::engine::views::carrier::read_view_carrier(&scene.document, entity).is_some()
        || crate::modules::aec::engine::views::carrier::badge_carrier_handle(entity).is_some()
    {
        return Some(crate::tr!("aec", "view-carrier-title"));
    }
    if let Some(marker) = crate::modules::aec::views::din1356::read_section_marker(&scene.document, entity) {
        return Some(if marker.is_elevation {
            crate::tr!("aec", "elevation-marker-title")
        } else {
            crate::tr!("aec", "section-marker-title")
        });
    }
    if let Some(marker_owner) = resolve_section_marker_package(scene, handle) {
        if marker_owner != handle {
            if let Some(owner_ent) = scene.document.get_entity(marker_owner) {
                if let Some(marker) = crate::modules::aec::views::din1356::read_section_marker(&scene.document, owner_ent) {
                    return Some(if marker.is_elevation {
                        crate::tr!("aec", "elevation-marker-title")
                    } else {
                        crate::tr!("aec", "section-marker-title")
                    });
                }
            }
        }
    }
    if entity.common().extended_data.records().iter().any(|r| {
        r.application_name == crate::modules::aec::engine::xdata::AEC_APPID
            && matches!(r.values.first(), Some(acadrust::xdata::XDataValue::String(s)) if s == crate::modules::aec::engine::views::carrier::DERIVED_PRIMITIVE_TAG)
    }) {
        return Some("AEC Assoziiertes Bauteil".to_string());
    }
    None
}

pub fn collapse_selection_to_wall_package<'a>(
    scene: &'a crate::scene::Scene,
    selected: Vec<(Handle, &'a EntityType)>,
) -> Vec<(Handle, &'a EntityType)> {
    collapse_selection_to_aec_package(scene, selected)
}

pub fn collapse_selection_to_aec_package<'a>(
    scene: &'a crate::scene::Scene,
    selected: Vec<(Handle, &'a EntityType)>,
) -> Vec<(Handle, &'a EntityType)> {
    if selected.is_empty() {
        return selected;
    }
    let mut collapsed = Vec::with_capacity(selected.len());
    let mut seen = rustc_hash::FxHashSet::default();

    for (handle, entity) in selected {
        let owner_handle = resolve_aec_package(scene, handle);
        if seen.insert(owner_handle) {
            if let Some(owner_entity) = scene.document.get_entity(owner_handle) {
                collapsed.push((owner_handle, owner_entity));
            } else {
                collapsed.push((handle, entity));
            }
        }
    }

    let wall_handles: rustc_hash::FxHashSet<Handle> = collapsed
        .iter()
        .filter_map(|(h, e)| {
            if xdata::wall_from_entity(e).is_some() {
                Some(*h)
            } else {
                None
            }
        })
        .collect();

    let slab_handles: rustc_hash::FxHashSet<Handle> = collapsed
        .iter()
        .filter_map(|(h, e)| {
            if slab_xdata::slab_from_entity(e).is_some() {
                Some(*h)
            } else {
                None
            }
        })
        .collect();

    if !wall_handles.is_empty() || !slab_handles.is_empty() {
        collapsed.retain(|(h, e)| {
            if !wall_handles.is_empty() {
                if let Some(op) = opening_xdata::opening_from_entity(e, *h) {
                    let host = wall_package::resolve_wall_package(scene, op.host_wall);
                    if wall_handles.contains(&op.host_wall) || wall_handles.contains(&host) {
                        return false;
                    }
                }
            }
            if !slab_handles.is_empty() {
                if let Some(op) = slab_xdata::slab_opening_from_entity(e) {
                    let host = slab_package::resolve_slab_package(scene, op.host_slab);
                    if slab_handles.contains(&op.host_slab) || slab_handles.contains(&host) {
                        return false;
                    }
                }
            }
            true
        });
    }

    collapsed
}

pub fn session_styles_for_scene(
    scene: &crate::scene::Scene,
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> Option<StyleLibrary> {
    let extracted = xdata::extract_style_library_from_scene(scene);
    let session =
        crate::modules::aec::engine::library::session_library_excluding_existing(&extracted, project);
    if session.materials.is_empty()
        && session.wall_styles.is_empty()
        && session.opening_styles.is_empty()
        && session.slab_styles.is_empty()
    {
        None
    } else {
        Some(session)
    }
}

pub fn on_document_loaded(
    scene: &crate::scene::Scene,
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> (
    crate::modules::aec::engine::library::DisplayConfigLibrary,
    Option<StyleLibrary>,
) {
    let plan_library =
        crate::modules::aec::engine::project::resolve_display_config_library(project);
    (plan_library, session_styles_for_scene(scene, project))
}

pub fn is_wall_derived_non_axis(scene: &crate::scene::Scene, handle: Handle) -> bool {
    wall_package::is_wall_derived_non_axis(scene, handle)
}

pub fn resolve_section_marker_package(scene: &crate::scene::Scene, handle: Handle) -> Option<Handle> {
    let ent = scene.document.get_entity(handle)?;
    if crate::modules::aec::views::din1356::read_section_marker(&scene.document, ent).is_some() {
        return Some(handle);
    }
    let owner = crate::modules::aec::views::din1356::section_marker_owner_from_entity(ent)?;
    if scene
        .document
        .get_entity(owner)
        .is_some_and(|owner_ent| {
            crate::modules::aec::views::din1356::read_section_marker(&scene.document, owner_ent)
                .is_some()
        })
    {
        Some(owner)
    } else {
        None
    }
}

pub fn is_section_marker_derived(scene: &crate::scene::Scene, handle: Handle) -> bool {
    resolve_section_marker_package(scene, handle).is_some_and(|owner| owner != handle)
}

pub fn expand_handles_for_section_marker_packages(
    scene: &crate::scene::Scene,
    handles: &[Handle],
) -> Vec<Handle> {
    let mut out = Vec::with_capacity(handles.len());
    let mut seen = rustc_hash::FxHashSet::default();
    for &h in handles {
        if let Some(owner) = resolve_section_marker_package(scene, h) {
            if seen.insert(owner) {
                out.push(owner);
            }
            for child in crate::modules::aec::engine::owner_index::children_of(&scene.document, owner) {
                if scene.document.get_entity(child).is_some() && seen.insert(child) {
                    out.push(child);
                }
            }
        } else if seen.insert(h) {
            out.push(h);
        }
    }
    out
}

pub fn is_aec_derived_non_carrier(scene: &crate::scene::Scene, handle: Handle) -> bool {
    if let Some(ent) = scene.document.get_entity(handle) {
        if crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(ent).is_some() {
            return false;
        }
    }
    wall_package::is_wall_derived_non_axis(scene, handle)
        || slab_package::is_slab_derived(scene, handle)
        || slab_package::slab_opening_owner_if_any(scene, handle).is_some_and(|owner| owner != handle)
        || opening_display::opening_owner_if_any(scene, handle).is_some_and(|owner| owner != handle)
        || scene.document.get_entity(handle).map(room_package::is_room_derived).unwrap_or(false)
        || is_section_marker_derived(scene, handle)
}

pub fn resolve_wall_package(scene: &crate::scene::Scene, handle: Handle) -> Handle {
    wall_package::resolve_wall_package(scene, handle)
}

pub fn expand_handles_for_aec_packages(scene: &crate::scene::Scene, handles: &[Handle]) -> Vec<Handle> {
    let handles = wall_package::expand_handles_for_wall_packages(scene, handles);
    let handles = slab_package::expand_handles_for_slab_packages(scene, &handles);
    let handles = room_package::expand_handles_for_room_packages(scene, &handles);
    expand_handles_for_section_marker_packages(scene, &handles)
}

pub fn resolve_aec_package(scene: &crate::scene::Scene, handle: Handle) -> Handle {
    if let Some(ent) = scene.document.get_entity(handle) {
        if let Some(owner) = crate::modules::aec::engine::views::carrier::badge_carrier_handle(ent) {
            return owner;
        }
        if crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(ent).is_some() {
            return handle;
        }
    }
    if let Some(opening_owner) = opening_display::opening_owner_if_any(scene, handle) {
        return opening_owner;
    }
    if let Some(slab_opening_owner) = slab_package::slab_opening_owner_if_any(scene, handle) {
        return slab_opening_owner;
    }
    if let Some(ent) = scene.document.get_entity(handle) {
        if let Some(room_owner) = room_package::resolve_room_package(ent) {
            return room_owner;
        }
    }
    if let Some(marker_owner) = resolve_section_marker_package(scene, handle) {
        if marker_owner != handle {
            return marker_owner;
        }
    }
    let wall_owner = wall_package::resolve_wall_package(scene, handle);
    if wall_owner != handle {
        return wall_owner;
    }
    let slab_owner = slab_package::resolve_slab_package(scene, handle);
    if slab_owner != handle {
        return slab_owner;
    }
    handle
}

/// The single wall/slab/slab-opening/opening package that all `handles`
/// resolve to, if any.
pub fn single_selected_package(
    scene: &crate::scene::Scene,
    handles: impl IntoIterator<Item = Handle>,
) -> Option<Handle> {
    let owners: Vec<Handle> = handles
        .into_iter()
        .map(|h| resolve_aec_package(scene, h))
        .collect();
    owners.first().copied().filter(|owner| {
        !owner.is_null()
            && owners.iter().all(|h| h == owner)
            && scene.document.get_entity(*owner).is_some_and(|e| {
                wall_from_entity(e)
                    || slab_from_entity(e)
                    || slab_opening_from_entity(e)
                    || opening_from_entity(e)
            })
    })
}

/// Expands an erase selection with derived AEC handles and unregisters
/// erased walls from their storeys.
pub fn prepare_erase_handles(scene: &mut crate::scene::Scene, handles: &mut Vec<Handle>) {
    use crate::modules::aec::engine::{room_package, slab_package, storey_xdata, wall_package};
    wall_package::expand_with_wall_derived_handles(scene, handles);
    slab_package::expand_with_slab_derived_handles(scene, handles);
    room_package::expand_with_room_derived_handles(scene, handles);
    storey_xdata::unregister_walls_from_storeys(scene, handles);
}

pub fn wall_from_entity(entity: &EntityType) -> bool {
    xdata::wall_from_entity(entity).is_some()
}

pub fn slab_from_entity(entity: &EntityType) -> bool {
    slab_xdata::slab_from_entity(entity).is_some()
}

pub fn slab_opening_from_entity(entity: &EntityType) -> bool {
    slab_xdata::slab_opening_from_entity(entity).is_some()
}

pub fn opening_from_entity(entity: &EntityType) -> bool {
    opening_xdata::opening_from_entity(entity, entity.common().handle).is_some()
}

pub fn room_from_entity(entity: &EntityType) -> bool {
    room_xdata::room_from_entity(entity).is_some()
}

pub fn selection_is_all_walls(
    scene: &crate::scene::Scene,
    handles: impl IntoIterator<Item = Handle>,
) -> bool {
    let mut any = false;
    for h in handles {
        any = true;
        let resolved = wall_package::resolve_wall_package(scene, h);
        let Some(entity) = scene.document.get_entity(resolved) else {
            return false;
        };
        if xdata::wall_from_entity(entity).is_none() {
            return false;
        }
    }
    any
}
