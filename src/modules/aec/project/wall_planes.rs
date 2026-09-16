//! Bind/unbind wall base/top control planes from the properties panel.

use acadrust::xdata::ExtendedDataRecord;
use acadrust::Handle;
use uuid::Uuid;

use crate::modules::aec::commands::{
    regenerate_wall_representation, wall_from_entity, wall_record_for_wall, write_aec_record,
    AEC_APPID,
};
use crate::modules::aec::engine::library::StyleLibrary;
use crate::modules::aec::engine::project::ProjectFile;
use crate::scene::Scene;

pub const UNBOUND_LABEL: &str = "(none)";

fn is_unbound_label(choice: &str) -> bool {
    choice == UNBOUND_LABEL || choice == crate::t!("(none)").as_ref()
}

pub fn apply_wall_plane_choice(
    scene: &mut Scene,
    project: Option<&ProjectFile>,
    handle: Handle,
    base: bool,
    choice: &str,
    library: Option<&StyleLibrary>,
) -> bool {
    let Some(entity) = scene.document.get_entity(handle) else {
        return false;
    };
    let (x, y) = match entity {
        acadrust::EntityType::LwPolyline(pl) if !pl.vertices.is_empty() => {
            (pl.vertices[0].location.x, pl.vertices[0].location.y)
        }
        _ => (0.0, 0.0),
    };
    let Some(mut wall) = wall_from_entity(entity) else {
        return false;
    };
    let id = parse_plane_choice(project, choice);
    let name = if is_unbound_label(choice) || choice.trim().is_empty() {
        None
    } else {
        Some(choice.trim().to_string())
    };
    if base {
        wall.base_plane_id = id;
        wall.base_plane_name = name;
    } else {
        wall.top_plane_id = id;
        wall.top_plane_name = name;
    }
    if wall.base_plane_id.is_some() {
        if let Some(project) = project {
            wall.rebake_from_project(project, x, y);
        }
    }
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    write_aec_record(&mut scene.document, handle, record);
    if wall.base_plane_id.is_some() {
        if let Ok(touched) = regenerate_wall_representation(scene, handle, library) {
            let changes: Vec<_> = touched
                .into_iter()
                .map(|h| (h, crate::scene::ChangeKind::Modified))
                .collect();
            if !changes.is_empty() {
                scene.bump_entities(&changes);
            }
        }
    }
    true
}

pub fn apply_wall_base_z(scene: &mut Scene, handle: Handle, z: f64) -> bool {
    let Some(entity) = scene.document.get_entity(handle) else {
        return false;
    };
    let Some(mut wall) = wall_from_entity(entity) else {
        return false;
    };
    if wall.base_plane_id.is_some() {
        return false;
    }
    wall.base_origin[2] = z;
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    write_aec_record(&mut scene.document, handle, record)
}

fn parse_plane_choice(project: Option<&ProjectFile>, choice: &str) -> Option<Uuid> {
    let choice = choice.trim();
    if choice.is_empty() || is_unbound_label(choice) {
        return None;
    }
    if let Ok(id) = Uuid::parse_str(choice) {
        return Some(id);
    }
    project.and_then(|p| {
        for b in &p.buildings {
            for s in &b.storeys {
                if let Some(pl) = s.control_planes.iter().find(|pl| pl.name == choice) {
                    return Some(pl.id);
                }
            }
        }
        None
    })
}


#[cfg(test)]
mod tests {
    use crate::modules::aec::engine::control_plane::resolve_wall_height;
    use crate::modules::aec::engine::project::StoreyRef;
    use crate::modules::aec::engine::wall::Wall;

    #[test]
    fn offset_changes_resolved_height() {
        let storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        let base = storey.plane(storey.floor_plane_id).unwrap();
        let top = storey.plane(storey.ceiling_plane_id).unwrap();
        let h0 = resolve_wall_height(-5.0, -5.0, base, top, 0.0, 0.0).unwrap();
        let h1 = resolve_wall_height(-5.0, -5.0, base, top, 0.1, -0.2).unwrap();
        assert!((h0 - 3.0).abs() < 1e-6);
        assert!((h1 - 2.7).abs() < 1e-6);
    }

    #[test]
    fn first_plane_choice_is_kept_until_second() {
        let storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        let mut wall = Wall::new("s", 2.5, 0);
        wall.base_origin[2] = 1.0;
        wall.base_plane_id = Some(storey.floor_plane_id);
        wall.rebake_planes(&storey, 0.0, 0.0);
        assert_eq!(wall.base_plane_id, Some(storey.floor_plane_id));
        assert!(wall.top_plane_id.is_none());
        assert!((wall.height - 2.5).abs() < 1e-9);
        assert!((wall.base_origin[2] - 0.0).abs() < 1e-9);
        wall.top_plane_id = Some(storey.ceiling_plane_id);
        wall.rebake_planes(&storey, 0.0, 0.0);
        assert!((wall.height - 3.0).abs() < 1e-6);
    }

    #[test]
    fn parse_plane_choice_by_name() {
        use crate::modules::aec::engine::project::{Building, ProjectFile};
        let storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        let floor_id = storey.floor_plane_id;
        let floor_name = storey.plane(floor_id).unwrap().name.clone();
        let mut project = ProjectFile::default();
        let mut building = Building::new("B");
        building.storeys.push(storey);
        project.buildings.push(building);
        assert_eq!(
            super::parse_plane_choice(Some(&project), &floor_name),
            Some(floor_id)
        );
        assert!(super::parse_plane_choice(Some(&project), super::UNBOUND_LABEL).is_none());
    }

    #[test]
    fn unbind_ignores_plane_move() {
        let mut storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        let mut wall = Wall::new("s", 2.5, 0);
        wall.base_origin[2] = 1.0;
        wall.height = 2.5;
        storey.set_elevation(4.0);
        wall.rebake_planes(&storey, 0.0, 0.0);
        assert!((wall.height - 2.5).abs() < 1e-9);
        assert!((wall.base_origin[2] - 1.0).abs() < 1e-9);
    }
}
