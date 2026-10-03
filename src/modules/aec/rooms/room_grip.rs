//! Grip affordances and interactive manipulation for Room stamps.

use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};

use crate::app::OpenCADStudio;
use crate::modules::aec::engine::room_package::{
    is_room_carrier, resolve_room_package_handle, room_stamp_handle,
};
use crate::modules::aec::engine::room_regen::regenerate_room_representation;
use crate::modules::aec::engine::room_xdata::{room_from_entity, write_room_record};
use crate::modules::aec::properties::ROOM_STAMP_GRIP_ID;
use crate::scene::model::object::GripApply;
use crate::scene::Scene;

/// Moves the room stamp of a Room carrier entity to `new_pos` (local 2D coords).
pub fn move_room_stamp(scene: &mut Scene, room_handle: Handle, new_pos: (f64, f64)) -> bool {
    let owner = resolve_room_package_handle(scene, room_handle);
    let Some(entity) = scene.document.get_entity_mut(owner) else {
        return false;
    };
    let Some(mut room) = room_from_entity(entity) else {
        return false;
    };
    room.stamp_pos = Some(new_pos);
    write_room_record(&mut scene.document, owner, &room);

    regenerate_room_representation(scene, owner, None);
    scene.bump_geometry();
    true
}

impl OpenCADStudio {
    /// Apply an interactive grip edit for Room entities (e.g. moving room stamp).
    pub(crate) fn apply_aec_room_grip(
        &mut self,
        tab: usize,
        handle: Handle,
        grip_id: usize,
        apply: &GripApply,
    ) -> bool {
        if grip_id != ROOM_STAMP_GRIP_ID {
            return false;
        }

        let scene = &mut self.tabs[tab].scene;
        let owner = resolve_room_package_handle(scene, handle);
        let Some(carrier) = scene.document.get_entity(owner) else {
            return false;
        };
        if !is_room_carrier(carrier) {
            return false;
        }
        let Some(mut room) = room_from_entity(carrier) else {
            return false;
        };

        let current_pos = room.stamp_pos.unwrap_or_else(|| {
            let pts = crate::modules::aec::engine::room_regen::room_boundary_points(carrier);
            if pts.len() >= 3 {
                crate::modules::aec::engine::geometry::centroid(&pts)
            } else {
                (0.0, 0.0)
            }
        });

        let new_pos = match apply {
            GripApply::Absolute(pt) => (pt.x, pt.y),
            GripApply::Translate(delta) => (current_pos.0 + delta.x, current_pos.1 + delta.y),
        };

        room.stamp_pos = Some(new_pos);
        write_room_record(&mut scene.document, owner, &room);

        if let Some(stamp_h) = room_stamp_handle(scene, owner) {
            if let Some(EntityType::MText(mtext)) = scene.document.get_entity_mut(stamp_h) {
                mtext.insertion_point = Vector3::new(new_pos.0, new_pos.1, mtext.insertion_point.z);
            }
        }

        self.tabs[tab].dirty = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::LwPolyline;
    use acadrust::entities::LwVertex;
    use acadrust::types::Vector2;
    use crate::modules::aec::engine::room::Room;

    #[test]
    fn test_move_room_stamp() {
        let mut scene = Scene::new();
        let pts = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let mut pl = LwPolyline::new();
        pl.is_closed = true;
        for &(x, y) in &pts {
            pl.add_vertex(LwVertex::new(Vector2::new(x, y)));
        }
        let room_h = scene.add_entity(EntityType::LwPolyline(pl));

        let room = Room::from_polygon("Testraum", &pts, 2.5, 0);
        write_room_record(&mut scene.document, room_h, &room);
        regenerate_room_representation(&mut scene, room_h, None);

        let ok = move_room_stamp(&mut scene, room_h, (7.0, 8.0));
        assert!(ok);

        let entity = scene.document.get_entity(room_h).expect("get entity");
        let updated_room = room_from_entity(entity).expect("room from entity");
        assert_eq!(updated_room.stamp_pos, Some((7.0, 8.0)));
    }
}
