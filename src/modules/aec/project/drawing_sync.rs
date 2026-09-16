//! Sync control-plane origins from in-drawing preview Face3Ds into the manager.

use acadrust::entities::EntityType;

use crate::modules::aec::engine::project::StoreyRef;
use crate::modules::aec::project::preview::control_plane_from_entity;
use crate::scene::Scene;

/// Copy Face3D first-corner XYZ onto matching storey planes (drawing → manager).
pub fn sync_storey_planes_from_drawing(scene: &Scene, storey: &mut StoreyRef) -> bool {
    let mut changed = false;
    for entity in scene.document.entities() {
        let Some((id, _)) = control_plane_from_entity(entity) else {
            continue;
        };
        let EntityType::Face3D(face) = entity else {
            continue;
        };
        let origin = [face.first_corner.x, face.first_corner.y, face.first_corner.z];
        if let Some(plane) = storey.plane_mut(id) {
            if plane.origin != origin {
                plane.origin = origin;
                changed = true;
            }
        }
    }
    if changed {
        storey.sync_derived_elevation_height();
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::project::StoreyRef;
    use crate::modules::aec::project::preview::regenerate_control_plane_previews;
    use acadrust::Handle;

    #[test]
    fn drawing_z_updates_manager() {
        let mut scene = Scene::new();
        let mut storey = StoreyRef::new("EG", 0.0, "eg.dwg");
        regenerate_control_plane_previews(&mut scene, &mut storey);
        let pid = storey.floor_plane_id;
        let h = Handle::new(storey.plane(pid).unwrap().preview_handle.unwrap());
        if let Some(EntityType::Face3D(f)) = scene.document.get_entity_mut(h) {
            f.first_corner.z = 2.5;
            f.second_corner.z = 2.5;
            f.third_corner.z = 2.5;
            f.fourth_corner.z = 2.5;
        }
        assert!(sync_storey_planes_from_drawing(&scene, &mut storey));
        assert!((storey.derived_elevation() - 2.5).abs() < 1e-9);
    }
}
