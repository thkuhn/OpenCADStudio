//! Sync control-plane origins from in-drawing preview Face3Ds into the manager.

use acadrust::entities::EntityType;

use crate::app::OpenCADStudio;
use crate::modules::aec::engine::control_plane::ControlPlaneFacet;
use crate::modules::aec::engine::project::StoreyRef;
use crate::modules::aec::project::preview::control_plane_facet_from_entity;
use crate::scene::Scene;

fn normal_from_points(p1: [f64; 3], p2: [f64; 3], p3: [f64; 3]) -> [f64; 3] {
    let v1 = [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]];
    let v2 = [p3[0] - p1[0], p3[1] - p1[1], p3[2] - p1[2]];
    let mut n = [
        v1[1] * v2[2] - v1[2] * v2[1],
        v1[2] * v2[0] - v1[0] * v2[2],
        v1[0] * v2[1] - v1[1] * v2[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len < 1e-9 {
        [0.0, 0.0, 1.0]
    } else {
        n = [n[0] / len, n[1] / len, n[2] / len];
        if n[2] < -1e-9 || (n[2].abs() <= 1e-9 && (n[1] < -1e-9 || (n[1].abs() <= 1e-9 && n[0] < -1e-9))) {
            n = [-n[0], -n[1], -n[2]];
        }
        n
    }
}

fn approx_arr3(a: [f64; 3], b: [f64; 3]) -> bool {
    (a[0] - b[0]).abs() <= 1e-6 && (a[1] - b[1]).abs() <= 1e-6 && (a[2] - b[2]).abs() <= 1e-6
}

/// Copy Polyline3D/Face3D geometry (origin, normal, polygon facets) onto matching storey planes (drawing → manager).
pub fn sync_storey_planes_from_drawing(scene: &Scene, storey: &mut StoreyRef) -> bool {
    let mut changed = false;

    // 1. Group all preview entities in scene by plane_id:
    // (plane_id) -> Vec<(Option<usize> /* facet_idx */, String /* name */, Handle, Vec<[f64; 3]> /* verts */)>
    let mut scene_plane_entities: rustc_hash::FxHashMap<
        uuid::Uuid,
        Vec<(Option<usize>, String, acadrust::Handle, Vec<[f64; 3]>)>,
    > = rustc_hash::FxHashMap::default();

    for entity in scene.document.entities() {
        let Some((id, name, facet_idx)) = control_plane_facet_from_entity(entity) else {
            continue;
        };
        let verts: Vec<[f64; 3]> = match entity {
            EntityType::Polyline3D(p3) => p3
                .vertices
                .iter()
                .map(|v| [v.position.x, v.position.y, v.position.z])
                .collect(),
            EntityType::Face3D(face) => {
                let p1 = [face.first_corner.x, face.first_corner.y, face.first_corner.z];
                let p2 = [face.second_corner.x, face.second_corner.y, face.second_corner.z];
                let p3 = [face.third_corner.x, face.third_corner.y, face.third_corner.z];
                let p4 = [face.fourth_corner.x, face.fourth_corner.y, face.fourth_corner.z];
                let mut v = vec![p1, p2, p3];
                if !approx_arr3(p4, p3) && !approx_arr3(p4, p1) {
                    v.push(p4);
                }
                v
            }
            _ => continue,
        };
        if verts.len() < 3 {
            continue;
        }
        scene_plane_entities
            .entry(id)
            .or_default()
            .push((facet_idx, name, entity.as_entity().handle(), verts));
    }

    // 2. Synchronize each plane with scene entities
    for plane in &mut storey.control_planes {
        if let Some(items) = scene_plane_entities.get_mut(&plane.id) {
            // Sort items by facet index
            items.sort_by_key(|(idx, _, _, _)| idx.unwrap_or(0));

            // If the scene currently contains some facets for this plane, and some previously registered
            // preview entities were deleted/erased in the current scene:
            let scene_handles: rustc_hash::FxHashSet<u64> =
                items.iter().map(|(_, _, h, _)| h.value()).collect();
            let had_preview_handles = plane.facets.iter().any(|f| f.preview_handle.is_some());
            if had_preview_handles && items.len() < plane.facets.len() {
                let before_len = plane.facets.len();
                plane.facets.retain(|f| {
                    if let Some(h) = f.preview_handle {
                        scene_handles.contains(&h)
                    } else {
                        true
                    }
                });
                if plane.facets.len() != before_len {
                    changed = true;
                }
            }

            for (idx_opt, name, handle, verts) in items {
                let idx = idx_opt.unwrap_or(0);
                let p1 = verts[0];
                let p2 = verts[1];
                let p3 = verts[2];
                let n = normal_from_points(p1, p2, p3);

                let target_facet = if let Some(pos) = plane
                    .facets
                    .iter()
                    .position(|f| f.preview_handle == Some(handle.value()))
                {
                    Some(pos)
                } else if !name.is_empty() && plane.facets.iter().any(|f| f.name == *name) {
                    plane.facets.iter().position(|f| f.name == *name)
                } else if idx < plane.facets.len() && plane.facets[idx].preview_handle.is_none()
                {
                    Some(idx)
                } else {
                    None
                };

                if let Some(pos) = target_facet {
                    plane.facets[pos].preview_handle = Some(handle.value());
                    if plane.facets[pos].vertices != *verts {
                        plane.facets[pos].vertices = verts.clone();
                        changed = true;
                    }
                    if pos == 0
                        && (!approx_arr3(plane.origin, p1) || !approx_arr3(plane.normal, n))
                    {
                        plane.origin = p1;
                        plane.normal = n;
                        changed = true;
                    }
                } else {
                    let facet_name = if !name.is_empty() {
                        name.clone()
                    } else {
                        format!("Facet_{}", idx + 1)
                    };
                    let mut new_facet = ControlPlaneFacet::new(facet_name, verts.clone());
                    new_facet.preview_handle = Some(handle.value());
                    plane.facets.push(new_facet);
                    if plane.facets.len() == 1 {
                        plane.origin = p1;
                        plane.normal = n;
                    }
                    changed = true;
                }
            }
            if let Some(first) = plane.facets.first() {
                plane.origin = first.origin();
                plane.normal = first.unit_normal();
            }
        }
    }
    if changed {
        storey.sync_derived_elevation_height();
    }
    changed
}

impl OpenCADStudio {
    pub(crate) fn sync_storey_from_active_drawing(&mut self, bid: uuid::Uuid, sid: uuid::Uuid) {
        let i = self.active_tab;
        let scene = &self.tabs[i].scene;
        let changed = self.aec.aec_project_explorer_file.as_mut().and_then(|p| {
            p.buildings
                .iter_mut()
                .find(|b| b.id == bid)
                .and_then(|b| b.storeys.iter_mut().find(|s| s.id == sid))
                .map(|s| sync_storey_planes_from_drawing(scene, s))
        });
        if changed == Some(true) {
            self.aec_project_explorer_persist_if_pathed();
        }
    }
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
        if let Some(EntityType::Polyline3D(pl)) = scene.document.get_entity_mut(h) {
            for v in &mut pl.vertices {
                v.position.z = 2.5;
            }
        }
        assert!(sync_storey_planes_from_drawing(&scene, &mut storey));
        assert!((storey.derived_elevation() - 2.5).abs() < 1e-9);
    }

    #[test]
    fn sloped_plane_in_drawing_updates_manager_normal() {
        let mut scene = Scene::new();
        let mut storey = StoreyRef::new("DG", 3.0, "dg.dwg");
        let ceiling_id = storey.ceiling_plane_id;
        regenerate_control_plane_previews(&mut scene, &mut storey);

        let h = Handle::new(storey.plane(ceiling_id).unwrap().preview_handle.unwrap());
        // Modify preview polyline to represent a 15-degree roof slope
        if let Some(EntityType::Polyline3D(pl)) = scene.document.get_entity_mut(h) {
            pl.vertices[0].position = acadrust::types::Vector3::new(0.0, 0.0, 6.0);
            pl.vertices[1].position = acadrust::types::Vector3::new(10.0, 0.0, 6.0);
            pl.vertices[2].position = acadrust::types::Vector3::new(10.0, 10.0, 8.68);
            pl.vertices[3].position = acadrust::types::Vector3::new(0.0, 10.0, 8.68);
        }

        assert!(sync_storey_planes_from_drawing(&scene, &mut storey));
        let plane = storey.plane(ceiling_id).unwrap();
        assert!(plane.is_sloped());
        assert!((plane.origin[2] - 6.0).abs() < 1e-6);
        assert!((plane.slope_degrees() - 15.0).abs() < 0.5);
    }

    #[test]
    fn multi_facet_plane_sync_from_drawing() {
        let mut scene = Scene::new();
        let mut storey = StoreyRef::new("Staffel", 0.0, "staffel.dwg");
        let f1 = crate::modules::aec::engine::control_plane::ControlPlaneFacet::new(
            "Facet1",
            vec![
                [0.0, 0.0, 3.0],
                [5.0, 0.0, 3.0],
                [5.0, 5.0, 3.0],
                [0.0, 5.0, 3.0],
            ],
        );
        let f2 = crate::modules::aec::engine::control_plane::ControlPlaneFacet::new(
            "Facet2",
            vec![
                [5.0, 0.0, 2.0],
                [10.0, 0.0, 2.0],
                [10.0, 5.0, 2.0],
                [5.0, 5.0, 2.0],
            ],
        );
        let comp = crate::modules::aec::engine::control_plane::ControlPlane::from_facets("Shed", vec![f1, f2]);
        let comp_id = comp.id;
        storey.control_planes.push(comp);
        regenerate_control_plane_previews(&mut scene, &mut storey);

        let p = storey.plane(comp_id).unwrap();
        let h2 = Handle::new(p.facets[1].preview_handle.unwrap());
        // Modify facet 2 in drawing
        if let Some(EntityType::Polyline3D(pl)) = scene.document.get_entity_mut(h2) {
            for v in &mut pl.vertices {
                v.position.z = 2.5;
            }
        }

        assert!(sync_storey_planes_from_drawing(&scene, &mut storey));
        let updated = storey.plane(comp_id).unwrap();
        assert_eq!(updated.facets[1].vertices[0][2], 2.5);
    }

    #[test]
    fn erased_facet_in_drawing_removes_facet_from_control_plane() {
        let mut scene = Scene::new();
        let mut storey = StoreyRef::new("Staffel", 0.0, "staffel.dwg");
        let f1 = crate::modules::aec::engine::control_plane::ControlPlaneFacet::new(
            "Facet1",
            vec![
                [0.0, 0.0, 3.0],
                [5.0, 0.0, 3.0],
                [5.0, 5.0, 3.0],
                [0.0, 5.0, 3.0],
            ],
        );
        let f2 = crate::modules::aec::engine::control_plane::ControlPlaneFacet::new(
            "Facet2",
            vec![
                [5.0, 0.0, 2.0],
                [10.0, 0.0, 2.0],
                [10.0, 5.0, 2.0],
                [5.0, 5.0, 2.0],
            ],
        );
        let comp = crate::modules::aec::engine::control_plane::ControlPlane::from_facets("Shed", vec![f1, f2]);
        let comp_id = comp.id;
        storey.control_planes.push(comp);
        regenerate_control_plane_previews(&mut scene, &mut storey);

        let p = storey.plane(comp_id).unwrap();
        let h1 = Handle::new(p.facets[0].preview_handle.unwrap());
        // Erase facet 1 in drawing
        scene.erase_entities(&[h1]);

        assert!(sync_storey_planes_from_drawing(&scene, &mut storey));
        let updated = storey.plane(comp_id).unwrap();
        assert_eq!(updated.facets.len(), 1);
        assert_eq!(updated.facets[0].name, "Facet2");
    }
}
