use super::*;

/// Like `extend_entity_sections`, but works without a scene (then only the
/// wall section is added).
pub fn extend_sections_opt(
    scene: Option<&crate::scene::Scene>,
    handle: Handle,
    entity: &EntityType,
    style_library: Option<&StyleLibrary>,
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    sections: &mut Vec<crate::scene::model::object::PropSection>,
) {
    if let Some(scene) = scene {
        extend_entity_sections(scene, handle, entity, style_library, project, sections);
    } else if let Some(wall_section) = wall_prop_section(entity, style_library, project) {
        sections.push(wall_section);
    }
}

pub fn extend_entity_sections(
    scene: &crate::scene::Scene,
    handle: Handle,
    entity: &EntityType,
    style_library: Option<&StyleLibrary>,
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    sections: &mut Vec<crate::scene::model::object::PropSection>,
) {
    if let Some(opening_handle) = opening_display::opening_owner_if_any(scene, handle) {
        if let Some(opening_entity) = scene.document.get_entity(opening_handle) {
            if let Some(opening) = opening_xdata::opening_from_entity(opening_entity, opening_handle)
            {
                sections.push(opening_prop_section(
                    &opening,
                    opening_handle,
                    style_library,
                    project,
                ));
            }
        }
    } else if let Some(slab_opening_handle) = slab_package::slab_opening_owner_if_any(scene, handle)
    {
        if let Some(opening_entity) = scene.document.get_entity(slab_opening_handle) {
            if let Some(opening) = slab_xdata::slab_opening_from_entity(opening_entity) {
                sections.push(slab_opening_prop_section(
                    &opening,
                    slab_opening_handle,
                    opening_entity,
                ));
            }
        }
    } else {
        let wall_handle = wall_package::resolve_wall_package(scene, handle);
        let wall_entity = scene.document.get_entity(wall_handle).unwrap_or(entity);
        if let Some(wall_section) = wall_prop_section(wall_entity, style_library, project) {
            sections.push(wall_section);
            sections.extend(wall_relation_sections(scene, wall_handle));
        } else {
            let slab_handle = slab_package::resolve_slab_package(scene, handle);
            let slab_entity = scene.document.get_entity(slab_handle).unwrap_or(entity);
            if let Some(slab_section) =
                slab_prop_section(slab_entity, style_library, project)
            {
                sections.push(slab_section);
            } else {
                let room_handle = scene
                    .document
                    .get_entity(handle)
                    .and_then(room_package::resolve_room_package)
                    .unwrap_or(handle);
                let room_entity = scene.document.get_entity(room_handle).unwrap_or(entity);
                if let Some(room_section) = room_prop_section(room_entity, style_library) {
                    sections.push(room_section);
                }
            }
        }
    }
    if let Some(storey_section) = storey_prop_section(scene, entity) {
        sections.push(storey_section);
    }
    if let Some(plane_section) = control_plane_prop_section(entity) {
        sections.push(plane_section);
    }
    let display_config_library = crate::modules::aec::engine::project::resolve_display_config_library(project);
    if let Some(carrier_section) = carrier_prop_section_with_library(
        entity,
        &scene.document,
        &display_config_library,
    ) {
        sections.push(carrier_section);
    }
    let marker_handle = resolve_section_marker_package(scene, handle).unwrap_or(handle);
    let marker_entity = scene.document.get_entity(marker_handle).unwrap_or(entity);
    if let Some(marker_section) = marker_prop_section(marker_entity, &scene.document) {
        sections.push(marker_section);
    }
    if let Some(prim_section) = derived_primitive_prop_section(entity) {
        sections.push(prim_section);
    }
}
