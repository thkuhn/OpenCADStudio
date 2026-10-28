//! Bind/unbind opening sill/head control planes from the properties panel.
//!
//! Numeric `sill_height` / `height` stay the baked cache relative to the host
//! wall base at the opening XY (independent of the wall's own plane refs).

use acadrust::Handle;

use crate::modules::aec::engine::control_plane::intersect_vertical_at_xy;
use crate::modules::aec::engine::library::StyleLibrary;
use crate::modules::aec::engine::opening_display::commit_opening_instance;
use crate::modules::aec::engine::opening_xdata::{opening_from_entity, write_opening_instance};
use crate::modules::aec::engine::openings::Opening;
use crate::modules::aec::engine::project::{ProjectFile, StoreyRef};
use crate::modules::aec::engine::wall::Wall;
use crate::modules::aec::engine::xdata::wall_from_entity;
use crate::modules::aec::project::wall_planes::{
    parse_plane_choice, resolve_plane_stored_name,
};
use crate::scene::Scene;

pub fn host_base_z_at_xy(
    wall: &Wall,
    project: Option<&ProjectFile>,
    x: f64,
    y: f64,
) -> f64 {
    let Some(project) = project else {
        return wall.base_origin[2];
    };
    let Some(base) = wall
        .base_plane_id
        .and_then(|id| project.control_plane(id).cloned())
    else {
        return wall.base_origin[2];
    };
    let offset_base = base.offset(wall.base_offset);
    intersect_vertical_at_xy(x, y, &offset_base)
        .map(|pt| pt[2])
        .unwrap_or(offset_base.origin[2])
}

pub fn opening_xy(entity: &acadrust::EntityType) -> (f64, f64) {
    match entity {
        acadrust::EntityType::Point(pt) => (pt.location.x, pt.location.y),
        acadrust::EntityType::LwPolyline(pl) if !pl.vertices.is_empty() => {
            (pl.vertices[0].location.x, pl.vertices[0].location.y)
        }
        _ => (0.0, 0.0),
    }
}

fn load_opening(scene: &Scene, handle: Handle) -> Option<(Opening, (f64, f64))> {
    let entity = scene.document.get_entity(handle)?;
    let opening = opening_from_entity(entity, handle)?;
    Some((opening, opening_xy(entity)))
}

fn host_wall(scene: &Scene, opening: &Opening) -> Option<Wall> {
    scene
        .document
        .get_entity(opening.host_wall)
        .and_then(wall_from_entity)
}

fn rebake_opening(
    opening: &mut Opening,
    project: Option<&ProjectFile>,
    storey: Option<&StoreyRef>,
    x: f64,
    y: f64,
    host_base_z: f64,
) {
    if let Some(project) = project {
        opening.rebake_from_project(project, x, y, host_base_z);
    } else if let Some(storey) = storey {
        opening.rebake_planes(storey, x, y, host_base_z);
    }
}

fn persist_and_regen(
    scene: &mut Scene,
    opening: &Opening,
    library: Option<&StyleLibrary>,
    rules: Option<&crate::modules::aec::engine::display_component::ComponentRuleSet>,
) {
    let _ = commit_opening_instance(scene, opening, library, rules);
}

pub fn apply_opening_plane_choice(
    scene: &mut Scene,
    project: Option<&ProjectFile>,
    handle: Handle,
    sill: bool,
    choice: &str,
    library: Option<&StyleLibrary>,
    rules: Option<&crate::modules::aec::engine::display_component::ComponentRuleSet>,
) -> bool {
    let Some((mut opening, (x, y))) = load_opening(scene, handle) else {
        return false;
    };
    let id = parse_plane_choice(project, choice);
    let name = resolve_plane_stored_name(project, id, choice);
    if sill {
        if opening.sill_plane_id == id {
            return false;
        }
        opening.sill_plane_id = id;
        opening.sill_plane_name = name;
        opening.sill_offset = 0.0;
        opening.formulas.remove("opening_sill_offset");
        if id.is_none() {
            opening.sill_normal = [0.0, 0.0, 1.0];
        }
    } else {
        if opening.head_plane_id == id {
            return false;
        }
        opening.head_plane_id = id;
        opening.head_plane_name = name;
        opening.head_offset = 0.0;
        opening.formulas.remove("opening_head_offset");
        if id.is_none() {
            opening.head_normal = [0.0, 0.0, 1.0];
        }
    }
    if opening.has_plane_binding() {
        let host_base_z = host_wall(scene, &opening)
            .map(|w| host_base_z_at_xy(&w, project, x, y))
            .unwrap_or(0.0);
        rebake_opening(&mut opening, project, None, x, y, host_base_z);
    }
    persist_and_regen(scene, &opening, library, rules);
    true
}

pub fn apply_opening_plane_offsets(
    scene: &mut Scene,
    project: Option<&ProjectFile>,
    handle: Handle,
    sill_offset: Option<f64>,
    head_offset: Option<f64>,
    library: Option<&StyleLibrary>,
    rules: Option<&crate::modules::aec::engine::display_component::ComponentRuleSet>,
) -> bool {
    let Some((mut opening, (x, y))) = load_opening(scene, handle) else {
        return false;
    };
    if sill_offset.map(|s| (opening.sill_offset - s).abs() < 1e-9).unwrap_or(true)
        && head_offset.map(|h| (opening.head_offset - h).abs() < 1e-9).unwrap_or(true)
    {
        return false;
    }
    let host_base_z = host_wall(scene, &opening)
        .map(|w| host_base_z_at_xy(&w, project, x, y))
        .unwrap_or(0.0);
    opening.apply_plane_offsets(sill_offset, head_offset, host_base_z);
    if opening.has_plane_binding() {
        rebake_opening(&mut opening, project, None, x, y, host_base_z);
    }
    persist_and_regen(scene, &opening, library, rules);
    true
}

/// Rebake openings that reference `storey` planes, or that stay bound while
/// their host wall on this storey moved (so relative cache tracks world Z).
pub fn rebake_bound_openings(
    scene: &mut Scene,
    storey: &StoreyRef,
    project: Option<&ProjectFile>,
    library: Option<&StyleLibrary>,
) {
    let mut jobs: Vec<Handle> = Vec::new();
    for entity in scene.document.entities() {
        let handle = entity.common().handle;
        let Some(opening) = opening_from_entity(entity, handle) else {
            continue;
        };
        if !opening_needs_rebake(&opening, host_wall(scene, &opening).as_ref(), storey) {
            continue;
        }
        jobs.push(handle);
    }
    let mut walls: Vec<Handle> = Vec::new();
    for handle in jobs {
        let Some((mut opening, (x, y))) = load_opening(scene, handle) else {
            continue;
        };
        let host_base_z = host_wall(scene, &opening)
            .map(|w| host_base_z_at_xy(&w, project, x, y))
            .unwrap_or(0.0);
        rebake_opening(&mut opening, project, Some(storey), x, y, host_base_z);
        write_opening_instance(scene, &opening);
        walls.push(opening.host_wall);
    }
    walls.sort_by_key(|h| h.value());
    walls.dedup();
    for wall in walls {
        if let Ok(touched) =
            crate::modules::aec::engine::wall_regen::regenerate_wall_representation(
                scene, wall, library,
            )
        {
            let changes: Vec<_> = touched
                .into_iter()
                .map(|h| (h, crate::scene::ChangeKind::Modified))
                .collect();
            if !changes.is_empty() {
                scene.bump_entities(&changes);
            }
        }
    }
}

fn opening_needs_rebake(opening: &Opening, wall: Option<&Wall>, storey: &StoreyRef) -> bool {
    let refs_storey = opening
        .sill_plane_id
        .and_then(|id| storey.plane(id))
        .is_some()
        || opening
            .head_plane_id
            .and_then(|id| storey.plane(id))
            .is_some();
    if refs_storey {
        return true;
    }
    if !opening.has_plane_binding() {
        return false;
    }
    wall.map(|w| {
        w.base_plane_id
            .and_then(|id| storey.plane(id))
            .is_some()
            || w.top_plane_id.and_then(|id| storey.plane(id)).is_some()
    })
    .unwrap_or(false)
}

pub fn rebake_opening_in_scene(
    scene: &mut Scene,
    project: Option<&ProjectFile>,
    handle: Handle,
    library: Option<&StyleLibrary>,
    rules: Option<&crate::modules::aec::engine::display_component::ComponentRuleSet>,
) -> bool {
    let Some((mut opening, (x, y))) = load_opening(scene, handle) else {
        return false;
    };
    if !opening.has_plane_binding() {
        return false;
    }
    let host_base_z = host_wall(scene, &opening)
        .map(|w| host_base_z_at_xy(&w, project, x, y))
        .unwrap_or(0.0);
    rebake_opening(&mut opening, project, None, x, y, host_base_z);
    persist_and_regen(scene, &opening, library, rules);
    true
}

#[cfg(test)]
mod tests {
    use acadrust::entities::{LwPolyline, LwVertex, Point};
    use acadrust::types::{Vector2, Vector3};
    use acadrust::xdata::ExtendedDataRecord;
    use acadrust::EntityType;
    use glam::DVec3;

    use super::*;
    use crate::modules::aec::engine::opening_xdata::{opening_from_entity, place_wall_opening};
    use crate::modules::aec::project::wall_planes::UNBOUND_LABEL;
    use crate::modules::aec::engine::openings::{Opening, OpeningKind};
    use crate::modules::aec::engine::project::{Building, ProjectFile, StoreyRef};
    use crate::modules::aec::engine::wall::{Wall, WallJustification, WallLayer};
    use crate::modules::aec::engine::xdata::{wall_record_for_wall, AEC_APPID};
    use crate::scene::Scene;

    fn sample_project() -> (ProjectFile, StoreyRef) {
        let storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        let mut project = ProjectFile::default();
        let mut building = Building::new("B");
        building.storeys.push(storey.clone());
        project.buildings.push(building);
        (project, storey)
    }

    fn add_test_wall(scene: &mut Scene, wall: Wall) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record_for_wall(&wall);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }

    #[test]
    fn only_sill_bound_keeps_height() {
        let (_, storey) = sample_project();
        let mut o = Opening::window(Handle::new(1), Handle::new(2), 1.5);
        let height = o.height;
        o.sill_plane_id = Some(storey.floor_plane_id);
        o.sill_offset = 0.1;
        o.rebake_planes(&storey, 1.5, 0.0, 0.0);
        assert!((o.sill_height - 0.1).abs() < 1e-9);
        assert!((o.height - height).abs() < 1e-9);
    }

    #[test]
    fn both_planes_set_height_from_head_minus_sill() {
        let (_, storey) = sample_project();
        let mut o = Opening::door(Handle::new(1), Handle::new(2), 1.5);
        o.sill_plane_id = Some(storey.floor_plane_id);
        o.head_plane_id = Some(storey.ceiling_plane_id);
        o.sill_offset = 0.0;
        o.head_offset = -0.2;
        o.rebake_planes(&storey, 1.5, 0.0, 0.0);
        assert!((o.sill_height - 0.0).abs() < 1e-9);
        assert!((o.height - 2.8).abs() < 1e-6);
    }

    #[test]
    fn unbind_ignores_plane_move() {
        let mut storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        let mut o = Opening::window(Handle::new(1), Handle::new(2), 1.0);
        o.sill_height = 0.9;
        o.height = 1.2;
        storey.set_elevation(4.0);
        o.rebake_planes(&storey, 0.0, 0.0, 0.0);
        assert!((o.sill_height - 0.9).abs() < 1e-9);
        assert!((o.height - 1.2).abs() < 1e-9);
    }

    #[test]
    fn storey_z_cascade_follows_bound_sill() {
        let mut storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        let mut o = Opening::window(Handle::new(1), Handle::new(2), 1.0);
        o.sill_plane_id = Some(storey.floor_plane_id);
        o.head_plane_id = Some(storey.ceiling_plane_id);
        o.rebake_planes(&storey, 0.0, 0.0, 0.0);
        assert!((o.sill_height - 0.0).abs() < 1e-9);
        assert!((o.height - 3.0).abs() < 1e-6);
        storey.set_elevation(2.5);
        o.rebake_planes(&storey, 0.0, 0.0, 0.0);
        assert!((o.sill_height - 2.5).abs() < 1e-9);
        assert!((o.height - 3.0).abs() < 1e-6);
    }

    #[test]
    fn opening_plane_differs_from_wall_base() {
        let (_, storey) = sample_project();
        let mut wall = Wall::new("s", 3.0, 0);
        wall.base_origin[2] = 0.0;
        let host_z = host_base_z_at_xy(&wall, None, 1.0, 0.0);
        assert!((host_z - 0.0).abs() < 1e-12);
        let mut o = Opening::window(Handle::new(1), Handle::new(2), 1.0);
        o.sill_plane_id = Some(storey.floor_plane_id);
        o.head_plane_id = Some(storey.ceiling_plane_id);
        o.rebake_planes(&storey, 1.0, 0.0, 1.0);
        assert!((o.sill_height + 1.0).abs() < 1e-9);
        assert!((o.height - 3.0).abs() < 1e-6);
    }

    #[test]
    fn breakthrough_binds_sill_and_head() {
        let (_, storey) = sample_project();
        let mut o = Opening::breakthrough(Handle::new(1), Handle::new(2), 1.0);
        let seed_sill = o.sill_height;
        let seed_h = o.height;
        assert!((seed_sill - 0.1).abs() < 1e-12);
        assert!((seed_h - 2.0).abs() < 1e-12);
        o.sill_plane_id = Some(storey.floor_plane_id);
        o.head_plane_id = Some(storey.ceiling_plane_id);
        o.rebake_planes(&storey, 2.0, 0.0, 0.0);
        assert!((o.sill_height - 0.0).abs() < 1e-9);
        assert!((o.height - 3.0).abs() < 1e-6);
        assert_eq!(o.kind, OpeningKind::Breakthrough);
    }

    #[test]
    fn parse_plane_choice_by_name() {
        let (project, storey) = sample_project();
        let floor_id = storey.floor_plane_id;
        let floor_name = storey.plane(floor_id).unwrap().name.clone();
        assert_eq!(
            parse_plane_choice(Some(&project), &floor_name),
            Some(floor_id)
        );
        assert!(parse_plane_choice(Some(&project), UNBOUND_LABEL).is_none());
    }

    #[test]
    fn place_stays_unbound() {
        let mut scene = Scene::new();
        let mut wall = Wall::new("s", 3.0, 0);
        wall.justification = WallJustification::Center;
        wall.layers = vec![WallLayer {
            material: "Concrete".into(),
            thickness: 0.3,
            function: "Structural".into(),
            axis_offset: -0.15,
            ..WallLayer::default()
        }];
        let wall_h = add_test_wall(&mut scene, wall);
        let (opening, _) = place_wall_opening(
            &mut scene,
            wall_h,
            DVec3::new(1.5, 0.0, 0.0),
            OpeningKind::Window,
            None,
            None,
            None,
        )
        .expect("place");
        let parsed = opening_from_entity(scene.document.get_entity(opening).unwrap(), opening)
            .unwrap();
        assert!(parsed.sill_plane_id.is_none());
        assert!(parsed.head_plane_id.is_none());
        assert!(!parsed.has_plane_binding());
    }

    #[test]
    fn apply_choice_rebakes_and_unbind_keeps_cache() {
        let (project, storey) = sample_project();
        let mut scene = Scene::new();
        let mut wall = Wall::new("s", 3.0, 0);
        wall.justification = WallJustification::Center;
        wall.layers = vec![WallLayer {
            material: "Concrete".into(),
            thickness: 0.3,
            function: "Structural".into(),
            axis_offset: -0.15,
            ..WallLayer::default()
        }];
        let wall_h = add_test_wall(&mut scene, wall);
        let mut opening = Opening::window(Handle::NULL, wall_h, 1.5);
        let pt = EntityType::Point(Point::at(Vector3::new(1.5, 0.0, 0.0)));
        let handle = scene.add_entity(pt);
        opening.handle = handle;
        write_opening_instance(&mut scene, &opening);

        let floor_name = storey.plane(storey.floor_plane_id).unwrap().name.clone();
        assert!(apply_opening_plane_choice(
            &mut scene,
            Some(&project),
            handle,
            true,
            &floor_name,
            None,
            None,
        ));
        let bound = opening_from_entity(scene.document.get_entity(handle).unwrap(), handle).unwrap();
        assert_eq!(bound.sill_plane_id, Some(storey.floor_plane_id));
        assert!((bound.sill_height - 0.0).abs() < 1e-9);
        let height = bound.height;

        assert!(apply_opening_plane_choice(
            &mut scene,
            Some(&project),
            handle,
            true,
            UNBOUND_LABEL,
            None,
            None,
        ));
        let unbound =
            opening_from_entity(scene.document.get_entity(handle).unwrap(), handle).unwrap();
        assert!(unbound.sill_plane_id.is_none());
        assert!((unbound.sill_height - 0.0).abs() < 1e-9);
        assert!((unbound.height - height).abs() < 1e-9);
    }

    #[test]
    fn host_base_uses_wall_plane_at_opening_xy() {
        let (project, storey) = sample_project();
        let mut wall = Wall::new("s", 3.0, 0);
        wall.base_plane_id = Some(storey.floor_plane_id);
        wall.base_origin[2] = 99.0;
        let z = host_base_z_at_xy(&wall, Some(&project), 2.0, 1.0);
        assert!((z - 0.0).abs() < 1e-9);
    }

    #[test]
    fn rebake_bound_openings_follows_storey_even_if_host_wall_is_elsewhere() {
        let mut storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        let mut scene = Scene::new();
        let mut wall = Wall::new("s", 3.0, 0);
        wall.justification = WallJustification::Center;
        wall.layers = vec![WallLayer {
            material: "Concrete".into(),
            thickness: 0.3,
            function: "Structural".into(),
            axis_offset: -0.15,
            ..WallLayer::default()
        }];
        // Host wall is unbound at Z=0, opening binds EG floor/ceiling.
        let wall_h = add_test_wall(&mut scene, wall);
        let mut opening = Opening::window(Handle::NULL, wall_h, 1.5);
        opening.sill_plane_id = Some(storey.floor_plane_id);
        opening.head_plane_id = Some(storey.ceiling_plane_id);
        opening.sill_plane_name = storey.plane(storey.floor_plane_id).map(|p| p.name.clone());
        opening.head_plane_name = storey.plane(storey.ceiling_plane_id).map(|p| p.name.clone());
        opening.rebake_planes(&storey, 1.5, 0.0, 0.0);
        let pt = EntityType::Point(Point::at(Vector3::new(1.5, 0.0, 0.0)));
        let handle = scene.add_entity(pt);
        opening.handle = handle;
        write_opening_instance(&mut scene, &opening);

        storey.set_elevation(2.0);
        let mut project = ProjectFile::default();
        let mut building = Building::new("B");
        building.storeys.push(storey.clone());
        project.buildings.push(building);
        rebake_bound_openings(&mut scene, &storey, Some(&project), None);

        let baked = opening_from_entity(scene.document.get_entity(handle).unwrap(), handle).unwrap();
        assert!((baked.sill_height - 2.0).abs() < 1e-9);
        assert!((baked.height - 3.0).abs() < 1e-6);
    }

    #[test]
    fn sloped_plane_unbind_resets_normal_and_keeps_sill_and_head_height() {
        use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};

        let mut project = ProjectFile::default();
        let mut building = Building::new("B");
        let mut storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");

        // Sloped control plane: Z=1.0 at x=0, Z=3.0 at x=10
        let f1 = ControlPlaneFacet::new(
            "SlopedSillPlane",
            vec![
                [0.0, -5.0, 1.0],
                [10.0, -5.0, 3.0],
                [10.0, 5.0, 3.0],
                [0.0, 5.0, 1.0],
            ],
        );
        let sloped_cp = ControlPlane::from_facets("SlopedSillPlane", vec![f1]);
        let sloped_id = sloped_cp.id;
        storey.control_planes.push(sloped_cp);
        building.storeys.push(storey);
        project.buildings.push(building);

        let mut scene = Scene::new();
        let mut wall = Wall::new("s", 3.0, 0);
        wall.justification = WallJustification::Center;
        wall.layers = vec![WallLayer {
            material: "Concrete".into(),
            thickness: 0.3,
            function: "Structural".into(),
            axis_offset: -0.15,
            ..WallLayer::default()
        }];
        let wall_h = add_test_wall(&mut scene, wall);

        let mut opening = Opening::window(Handle::NULL, wall_h, 5.0);
        let pt = EntityType::Point(Point::at(Vector3::new(5.0, 0.0, 0.0)));
        let handle = scene.add_entity(pt);
        opening.handle = handle;
        write_opening_instance(&mut scene, &opening);

        // 1. Bind sill to sloped plane (Z at x=5 is 2.0)
        assert!(apply_opening_plane_choice(
            &mut scene,
            Some(&project),
            handle,
            true,
            "SlopedSillPlane",
            None,
            None,
        ));
        let bound = opening_from_entity(scene.document.get_entity(handle).unwrap(), handle).unwrap();
        assert_eq!(bound.sill_plane_id, Some(sloped_id));
        assert!((bound.sill_height - 2.0).abs() < 1e-9);
        assert!((bound.sill_normal[2] - 1.0).abs() > 1e-4);
        let initial_height = bound.height;

        // 2. Explicit unbind sill to (none)
        assert!(apply_opening_plane_choice(
            &mut scene,
            Some(&project),
            handle,
            true,
            UNBOUND_LABEL,
            None,
            None,
        ));
        let unbound = opening_from_entity(scene.document.get_entity(handle).unwrap(), handle).unwrap();
        assert_eq!(unbound.sill_plane_id, None);
        assert_eq!(unbound.sill_plane_name, None);
        // Normal is reset to vertical [0, 0, 1]
        assert_eq!(unbound.sill_normal, [0.0, 0.0, 1.0]);
        // Sill height is numerically stable at 2.0
        assert!((unbound.sill_height - 2.0).abs() < 1e-9);
        assert!((unbound.height - initial_height).abs() < 1e-9);

        // 3. Bind head to sloped plane and then unbind
        assert!(apply_opening_plane_choice(
            &mut scene,
            Some(&project),
            handle,
            false,
            "SlopedSillPlane",
            None,
            None,
        ));
        let bound_head = opening_from_entity(scene.document.get_entity(handle).unwrap(), handle).unwrap();
        assert_eq!(bound_head.head_plane_id, Some(sloped_id));
        assert!((bound_head.head_normal[2] - 1.0).abs() > 1e-4);

        assert!(apply_opening_plane_choice(
            &mut scene,
            Some(&project),
            handle,
            false,
            UNBOUND_LABEL,
            None,
            None,
        ));
        let unbound_head = opening_from_entity(scene.document.get_entity(handle).unwrap(), handle).unwrap();
        assert_eq!(unbound_head.head_plane_id, None);
        assert_eq!(unbound_head.head_plane_name, None);
        assert_eq!(unbound_head.head_normal, [0.0, 0.0, 1.0]);
        assert!((unbound_head.height - bound_head.height).abs() < 1e-9);
        assert!((unbound_head.sill_height - bound_head.sill_height).abs() < 1e-9);
    }

    #[test]
    fn test_apply_opening_plane_choice_stores_pure_name_in_xdata() {
        use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};

        let mut project = ProjectFile::default();
        let mut building = Building::new("B");
        let mut eg = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        let mut og = StoreyRef::new_with_height("OG", 3.0, 3.0, "og.dwg");

        let eg_custom_floor = ControlPlane::horizontal("Floor", 0.0);
        eg.control_planes.push(eg_custom_floor);
        let og_custom_floor = ControlPlane::horizontal("Floor", 3.0);
        og.control_planes.push(og_custom_floor);

        let f = ControlPlaneFacet::new("Facet", vec![[0.0, 0.0, 1.2], [10.0, 0.0, 1.2], [10.0, 10.0, 1.2], [0.0, 10.0, 1.2]]);
        let cp = ControlPlane::from_facets("FensterBank (EG)", vec![f]);
        eg.control_planes.push(cp);

        building.storeys.push(eg);
        building.storeys.push(og);
        project.buildings.push(building);

        let mut scene = Scene::new();
        let wall = Wall::new("s", 3.0, 0);
        let wall_h = add_test_wall(&mut scene, wall);

        let mut opening = Opening::window(Handle::NULL, wall_h, 5.0);
        let pt = EntityType::Point(Point::at(Vector3::new(5.0, 0.0, 0.0)));
        let handle = scene.add_entity(pt);
        opening.handle = handle;
        write_opening_instance(&mut scene, &opening);

        // Apply formatted choice with storey disambiguation
        assert!(apply_opening_plane_choice(
            &mut scene,
            Some(&project),
            handle,
            true,
            "Floor [EG] (Z = 0.000 m)",
            None,
            None,
        ));
        let o_bound = opening_from_entity(scene.document.get_entity(handle).unwrap(), handle).unwrap();
        assert_eq!(o_bound.sill_plane_name, Some("Floor".to_string()));

        // Apply formatted choice with bracketed name
        assert!(apply_opening_plane_choice(
            &mut scene,
            Some(&project),
            handle,
            true,
            "FensterBank (EG) (Z = 1.200 m)",
            None,
            None,
        ));
        let o_bound_bracket = opening_from_entity(scene.document.get_entity(handle).unwrap(), handle).unwrap();
        assert_eq!(o_bound_bracket.sill_plane_name, Some("FensterBank (EG)".to_string()));
    }
}
