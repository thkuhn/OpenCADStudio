//! Bind/unbind slab base/top control planes from the properties panel.

use acadrust::Handle;

use crate::modules::aec::engine::library::StyleLibrary;
use crate::modules::aec::engine::project::ProjectFile;
use crate::modules::aec::engine::slab_package::resolve_slab_package;
use crate::modules::aec::engine::slab_regen::regenerate_slab_representation;
use crate::modules::aec::engine::slab_xdata::{slab_from_entity, write_slab_model};
use crate::modules::aec::project::wall_planes::{is_unbound_label, parse_plane_choice};
use crate::scene::Scene;

/// Applies a base/top control-plane choice to a slab and regenerates it.
pub fn apply_slab_plane_choice(
    scene: &mut Scene,
    project: Option<&ProjectFile>,
    handle: Handle,
    base: bool,
    choice: &str,
    library: Option<&StyleLibrary>,
    rules: Option<&crate::modules::aec::engine::display_component::ComponentRuleSet>,
) -> bool {
    let handle = resolve_slab_package(scene, handle);
    let Some(entity) = scene.document.get_entity(handle) else {
        return false;
    };
    let (x, y) = match entity {
        acadrust::EntityType::LwPolyline(pl) if !pl.vertices.is_empty() => {
            (pl.vertices[0].location.x, pl.vertices[0].location.y)
        }
        _ => (0.0, 0.0),
    };
    let Some(mut slab) = slab_from_entity(entity) else {
        return false;
    };
    let id = parse_plane_choice(project, choice);
    let name = if is_unbound_label(choice) || choice.trim().is_empty() {
        None
    } else {
        Some(choice.trim().to_string())
    };
    if base {
        slab.base_plane_id = id;
        slab.base_plane_name = name;
    } else {
        slab.top_plane_id = id;
        slab.top_plane_name = name;
    }
    if slab.base_plane_id.is_some() || slab.top_plane_id.is_some() {
        if let Some(project) = project {
            slab.rebake_from_project(project, x, y);
        }
    }
    if !write_slab_model(scene, handle, &slab) {
        return false;
    }
    regenerate_slab_representation(scene, handle, library, rules)
}
