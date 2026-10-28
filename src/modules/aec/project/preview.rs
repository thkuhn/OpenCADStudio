//! Control-plane wireframe previews in the storey drawing.

use acadrust::entities::EntityType;
use acadrust::types::Vector3;
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::Handle;
use uuid::Uuid;

use crate::modules::aec::engine::wall_regen::{ensure_controlplanes_layer, AEC_CONTROLPLANES_LAYER};
use crate::modules::aec::engine::xdata::{read_aec_record, write_aec_record, AEC_APPID};
use crate::modules::aec::engine::project::StoreyRef;
use crate::scene::Scene;

pub const CONTROLPLANE_TAG: &str = "CONTROLPLANE";
/// ACI cyan for extra planes; yellow for the storey elevation (main) plane.
const PREVIEW_COLOR_OTHER: i16 = 4;
const PREVIEW_COLOR_MAIN: i16 = 2;
/// Orange color (ACI 30 / RGB 255, 127, 0) for temporarily highlighting a control plane.
pub const PREVIEW_COLOR_HIGHLIGHT: i16 = 30;

/// Temporarily highlights the preview entities belonging to a specific control plane in orange (ACI 30).
/// Resets all other control plane preview entities to their default colors (cyan / yellow).
pub fn highlight_control_plane(scene: &mut Scene, storey: &StoreyRef, plane_id: Uuid) -> Vec<Handle> {
    let floor_id = storey.floor_plane_id;
    let mut touched = Vec::new();
    for plane in &storey.control_planes {
        let is_target = plane.id == plane_id;
        let default_color = if plane.id == floor_id {
            PREVIEW_COLOR_MAIN
        } else {
            PREVIEW_COLOR_OTHER
        };
        let color = if is_target {
            PREVIEW_COLOR_HIGHLIGHT
        } else {
            default_color
        };

        for facet in &plane.facets {
            if let Some(h) = facet.preview_handle {
                let handle = Handle::new(h);
                if let Some(e) = scene.document.get_entity_mut(handle) {
                    if let Some((pid, _, _)) = control_plane_facet_from_entity(e) {
                        if pid == plane.id {
                            e.as_entity_mut().set_color(acadrust::types::Color::from_index(color));
                            touched.push(handle);
                        }
                    }
                }
            }
        }
    }
    touched
}

/// Temporarily highlights a specific facet belonging to a control plane in orange (ACI 30).
/// Resets all other control plane preview entities to their default colors (cyan / yellow).
pub fn highlight_control_plane_facet(
    scene: &mut Scene,
    storey: &StoreyRef,
    plane_id: Uuid,
    facet_idx: usize,
) -> Vec<Handle> {
    let floor_id = storey.floor_plane_id;
    let mut touched = Vec::new();
    for plane in &storey.control_planes {
        let default_color = if plane.id == floor_id {
            PREVIEW_COLOR_MAIN
        } else {
            PREVIEW_COLOR_OTHER
        };

        for (idx, facet) in plane.facets.iter().enumerate() {
            let is_target = plane.id == plane_id && idx == facet_idx;
            let color = if is_target {
                PREVIEW_COLOR_HIGHLIGHT
            } else {
                default_color
            };
            if let Some(h) = facet.preview_handle {
                let handle = Handle::new(h);
                if let Some(e) = scene.document.get_entity_mut(handle) {
                    if let Some((pid, _, _)) = control_plane_facet_from_entity(e) {
                        if pid == plane.id {
                            e.as_entity_mut().set_color(acadrust::types::Color::from_index(color));
                            touched.push(handle);
                        }
                    }
                }
            }
        }
    }
    touched
}

/// Resets all control plane and facet preview entities in `scene` for `storey` back to their default colors (yellow for floor, cyan for others).
pub fn reset_control_plane_preview_colors(scene: &mut Scene, storey: &StoreyRef) -> Vec<Handle> {
    let floor_id = storey.floor_plane_id;
    let mut touched = Vec::new();
    for plane in &storey.control_planes {
        let default_color = if plane.id == floor_id {
            PREVIEW_COLOR_MAIN
        } else {
            PREVIEW_COLOR_OTHER
        };

        for facet in &plane.facets {
            if let Some(h) = facet.preview_handle {
                let handle = Handle::new(h);
                if let Some(e) = scene.document.get_entity_mut(handle) {
                    if let Some((pid, _, _)) = control_plane_facet_from_entity(e) {
                        if pid == plane.id {
                            e.as_entity_mut().set_color(acadrust::types::Color::from_index(default_color));
                            touched.push(handle);
                        }
                    }
                }
            }
        }
    }
    touched
}

pub fn write_control_plane_tag(scene: &mut Scene, handle: Handle, plane_id: Uuid, name: &str) {
    write_control_plane_tag_with_facet(scene, handle, plane_id, name, None);
}

pub fn write_control_plane_tag_with_facet(
    scene: &mut Scene,
    handle: Handle,
    plane_id: Uuid,
    name: &str,
    facet_idx: Option<usize>,
) {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String(CONTROLPLANE_TAG.to_string()));
    record.add_value(XDataValue::String(plane_id.to_string()));
    record.add_value(XDataValue::String(name.to_string()));
    if let Some(idx) = facet_idx {
        record.add_value(XDataValue::Integer32(idx as i32));
    }
    write_aec_record(&mut scene.document, handle, record);
}

pub fn control_plane_from_entity(entity: &EntityType) -> Option<(Uuid, String)> {
    control_plane_facet_from_entity(entity).map(|(id, name, _)| (id, name))
}

pub fn control_plane_facet_from_entity(entity: &EntityType) -> Option<(Uuid, String, Option<usize>)> {
    let record = read_aec_record(entity)?;
    let v = &record.values;
    let XDataValue::String(tag) = v.first()? else {
        return None;
    };
    if tag != CONTROLPLANE_TAG {
        return None;
    }
    let XDataValue::String(id) = v.get(1)? else {
        return None;
    };
    let uuid = Uuid::parse_str(id).ok()?;
    let name = match v.get(2) {
        Some(XDataValue::String(s)) => s.clone(),
        _ => String::new(),
    };
    let facet_idx = match v.get(3) {
        Some(XDataValue::Integer32(i)) if *i >= 0 => Some(*i as usize),
        _ => None,
    };
    Some((uuid, name, facet_idx))
}

/// Rebuild Face3D previews for a storey's control planes on `AEC_CONTROLPLANES`.
pub fn regenerate_control_plane_previews(scene: &mut Scene, storey: &mut StoreyRef) {
    ensure_controlplanes_layer(scene);
    let mut stale = Vec::new();
    for entity in scene.document.entities() {
        if let Some((pid, _, _)) = control_plane_facet_from_entity(entity) {
            if storey.plane(pid).is_some() {
                let h = entity.as_entity().handle();
                if !stale.contains(&h) {
                    stale.push(h);
                }
            }
        }
    }
    if !stale.is_empty() {
        scene.erase_entities(&stale);
    }
    let floor_id = storey.floor_plane_id;
    for plane in &mut storey.control_planes {
        plane.preview_handle = None;
        for facet in &mut plane.facets {
            facet.preview_handle = None;
        }
        if !plane.visible {
            continue;
        }
        let aci = if plane.id == floor_id {
            PREVIEW_COLOR_MAIN
        } else {
            PREVIEW_COLOR_OTHER
        };
        for (idx, facet) in plane.facets.iter_mut().enumerate() {
            if facet.vertices.len() < 3 {
                continue;
            }
            let pts: Vec<Vector3> = facet
                .vertices
                .iter()
                .map(|c| Vector3::new(c[0], c[1], c[2]))
                .collect();
            let mut poly = acadrust::entities::Polyline3D::from_points(pts);
            poly.flags.closed = true;
            let handle = scene.add_entity(EntityType::Polyline3D(poly));
            if let Some(e) = scene.document.get_entity_mut(handle) {
                let ent = e.as_entity_mut();
                ent.set_layer(AEC_CONTROLPLANES_LAYER.to_string());
                ent.set_color(acadrust::types::Color::from_index(aci));
            }
            write_control_plane_tag_with_facet(scene, handle, plane.id, &facet.name, Some(idx));
            facet.preview_handle = Some(handle.value());
            if idx == 0 {
                plane.preview_handle = Some(handle.value());
            }
        }
    }
}

/// Erases the stored facet preview entity `handle` from `scene` only if that entity exists in the
/// active drawing and carries a `CONTROLPLANE` tag for `plane_id`. Entity handles are only unique
/// per document, so an untagged or foreign-plane entity with the same handle is never touched.
pub fn erase_facet_preview_if_tagged(scene: &mut Scene, plane_id: Uuid, handle: u64) -> bool {
    let h = Handle::new(handle);
    let owned = scene
        .document
        .get_entity(h)
        .and_then(control_plane_facet_from_entity)
        .is_some_and(|(pid, _, _)| pid == plane_id);
    if owned {
        scene.erase_entities(&[h]);
    }
    owned
}

/// Erases all Face3D/Polyline3D preview entities for `plane_id` from `scene` that carry a
/// `CONTROLPLANE` tag matching `plane_id`. Entity handles are verified to prevent foreign document collisions.
pub fn erase_plane_previews_if_tagged(scene: &mut Scene, plane_id: Uuid) -> usize {
    let mut to_erase = Vec::new();
    for entity in scene.document.entities() {
        if let Some((pid, _, _)) = control_plane_facet_from_entity(entity) {
            if pid == plane_id {
                let h = entity.as_entity().handle();
                if !to_erase.contains(&h) {
                    to_erase.push(h);
                }
            }
        }
    }
    let count = to_erase.len();
    if !to_erase.is_empty() {
        scene.erase_entities(&to_erase);
    }
    count
}

/// When Face3D/Polyline3D preview entities for control planes or facets are deleted in the drawing,
/// remove the corresponding facet from the storey's control plane while strictly preserving the
/// invariant that a control plane must have at least one facet (`facets.len() >= 1`).
pub fn erase_control_plane_facets_for_storey(
    scene: &Scene,
    storey: &mut StoreyRef,
    handles: &[Handle],
) -> bool {
    let mut modified = false;
    for plane in &mut storey.control_planes {
        let mut indices_to_remove = Vec::new();
        for &h in handles {
            // Require matching active entity in scene with AEC XDATA matching this plane's UUID
            let Some(entity) = scene.document.get_entity(h) else {
                continue;
            };
            let Some((tagged_pid, tagged_name, tagged_facet_idx)) =
                control_plane_facet_from_entity(entity)
            else {
                continue;
            };
            if tagged_pid != plane.id {
                continue;
            }

            // Inside tagged same plane:
            // 1. Prefer facet with matching preview_handle
            let mut matched_idx = plane
                .facets
                .iter()
                .position(|f| f.preview_handle == Some(h.value()));

            // 2. Fallback: match by tag facet name
            if matched_idx.is_none() && !tagged_name.is_empty() {
                matched_idx = plane.facets.iter().position(|f| f.name == tagged_name);
            }

            // 3. Fallback: match by tag facet index only if safely within bounds
            if matched_idx.is_none() {
                if let Some(idx) = tagged_facet_idx {
                    if idx < plane.facets.len() {
                        matched_idx = Some(idx);
                    }
                }
            }

            if let Some(idx) = matched_idx {
                if !indices_to_remove.contains(&idx) {
                    indices_to_remove.push(idx);
                }
            }
        }

        // Sort descending so removing by index does not shift earlier indices
        indices_to_remove.sort_unstable_by(|a, b| b.cmp(a));
        for idx in indices_to_remove {
            if plane.remove_facet(idx).is_some() {
                modified = true;
            }
        }
    }
    modified
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::project::StoreyRef;

    #[test]
    fn preview_default_size_and_name_tag() {
        let mut scene = Scene::new();
        let mut storey = StoreyRef::new("EG", 0.0, "eg.dwg");
        regenerate_control_plane_previews(&mut scene, &mut storey);
        let floor = storey.plane(storey.floor_plane_id).unwrap();
        assert!((floor.origin[0] + 5.0).abs() < 1e-9);
        assert!((floor.origin[1] + 5.0).abs() < 1e-9);
        let h = Handle::new(floor.preview_handle.unwrap());
        let ent = scene.document.get_entity(h).unwrap();
        let (id, name) = control_plane_from_entity(ent).expect("tag");
        assert_eq!(id, floor.id);
        assert!(name.ends_with("_ELEVATION"));
        assert_eq!(ent.as_entity().color().index(), Some(PREVIEW_COLOR_MAIN as u16));
        if let EntityType::Polyline3D(pl) = ent {
            assert!((pl.vertices[0].position.x + 5.0).abs() < 1e-6);
            assert!((pl.vertices[0].position.y + 5.0).abs() < 1e-6);
            assert!((pl.vertices[1].position.x - 45.0).abs() < 1e-6);
            assert!((pl.vertices[3].position.y - 45.0).abs() < 1e-6);
        } else {
            panic!("expected Polyline3D");
        }
        let ceil = storey.plane(storey.ceiling_plane_id).unwrap();
        let ch = Handle::new(ceil.preview_handle.unwrap());
        let cent = scene.document.get_entity(ch).unwrap();
        assert_eq!(cent.as_entity().color().index(), Some(PREVIEW_COLOR_OTHER as u16));
    }

    #[test]
    fn preview_sloped_control_plane() {
        let mut scene = Scene::new();
        let mut storey = StoreyRef::new("DG", 5.0, "dg.dwg");
        let roof_plane = crate::modules::aec::engine::control_plane::ControlPlane::from_slope(
            "RoofSlope",
            [0.0, 0.0, 6.0],
            25.0,
            0.0,
        );
        storey.control_planes.push(roof_plane);
        regenerate_control_plane_previews(&mut scene, &mut storey);

        let p = storey.control_planes.iter().find(|p| p.name == "RoofSlope").unwrap();
        let h = Handle::new(p.preview_handle.unwrap());
        let ent = scene.document.get_entity(h).unwrap();
        if let EntityType::Polyline3D(pl) = ent {
            assert!((pl.vertices[0].position.z - 6.0).abs() > 0.01 || (pl.vertices[2].position.z - 6.0).abs() > 0.01);
        } else {
            panic!("expected Polyline3D");
        }
    }

    #[test]
    fn highlight_control_plane_changes_color_to_orange() {
        let mut scene = Scene::new();
        let mut storey = StoreyRef::new("EG", 0.0, "eg.dwg");
        let ceiling_id = storey.ceiling_plane_id;
        regenerate_control_plane_previews(&mut scene, &mut storey);

        let floor_handle = Handle::new(storey.plane(storey.floor_plane_id).unwrap().preview_handle.unwrap());
        let ceiling_handle = Handle::new(storey.plane(ceiling_id).unwrap().preview_handle.unwrap());

        assert_eq!(scene.document.get_entity(floor_handle).unwrap().as_entity().color().index(), Some(PREVIEW_COLOR_MAIN as u16));
        assert_eq!(scene.document.get_entity(ceiling_handle).unwrap().as_entity().color().index(), Some(PREVIEW_COLOR_OTHER as u16));

        let touched = highlight_control_plane(&mut scene, &storey, ceiling_id);
        assert!(touched.contains(&ceiling_handle));
        assert_eq!(scene.document.get_entity(ceiling_handle).unwrap().as_entity().color().index(), Some(PREVIEW_COLOR_HIGHLIGHT as u16));
        assert_eq!(scene.document.get_entity(floor_handle).unwrap().as_entity().color().index(), Some(PREVIEW_COLOR_MAIN as u16));

        let reset = reset_control_plane_preview_colors(&mut scene, &storey);
        assert!(reset.contains(&ceiling_handle));
        assert_eq!(scene.document.get_entity(ceiling_handle).unwrap().as_entity().color().index(), Some(PREVIEW_COLOR_OTHER as u16));
    }

    #[test]
    fn highlight_control_plane_facet_only_highlights_specific_facet() {
        use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};
        let mut scene = Scene::new();
        let mut storey = StoreyRef::new("EG", 0.0, "eg.dwg");
        let f1 = ControlPlaneFacet::new("Shed_1", vec![[0.0, 0.0, 3.0], [5.0, 0.0, 4.0], [5.0, 5.0, 4.0], [0.0, 5.0, 3.0]]);
        let f2 = ControlPlaneFacet::new("Shed_2", vec![[5.0, 0.0, 3.0], [10.0, 0.0, 4.0], [10.0, 5.0, 4.0], [5.0, 5.0, 3.0]]);
        let cp = ControlPlane::from_facets("ShedRoof", vec![f1, f2]);
        let cp_id = cp.id;
        storey.control_planes.push(cp);
        regenerate_control_plane_previews(&mut scene, &mut storey);

        let p = storey.plane(cp_id).unwrap();
        let h1 = Handle::new(p.facets[0].preview_handle.unwrap());
        let h2 = Handle::new(p.facets[1].preview_handle.unwrap());

        let touched = highlight_control_plane_facet(&mut scene, &storey, cp_id, 1);
        assert!(touched.contains(&h2));
        assert_eq!(scene.document.get_entity(h2).unwrap().as_entity().color().index(), Some(PREVIEW_COLOR_HIGHLIGHT as u16));
        assert_eq!(scene.document.get_entity(h1).unwrap().as_entity().color().index(), Some(PREVIEW_COLOR_OTHER as u16));
    }

    #[test]
    fn erase_preview_for_single_facet_plane_does_not_delete_last_facet() {
        let mut scene = Scene::new();
        let mut storey = StoreyRef::new("EG", 0.0, "eg.dwg");
        regenerate_control_plane_previews(&mut scene, &mut storey);

        let floor_id = storey.floor_plane_id;
        let floor_handle = Handle::new(storey.plane(floor_id).unwrap().preview_handle.unwrap());

        // Attempting to erase the preview for the only facet
        let modified = erase_control_plane_facets_for_storey(&scene, &mut storey, &[floor_handle]);
        assert!(!modified);
        let plane = storey.plane(floor_id).unwrap();
        assert_eq!(plane.facets.len(), 1);
    }

    #[test]
    fn erase_preview_for_multi_facet_plane_removes_exact_facet_and_updates_normal() {
        use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};
        let mut scene = Scene::new();
        let mut storey = StoreyRef::new("DG", 3.0, "dg.dwg");
        let f1 = ControlPlaneFacet::new(
            "FlatFacet",
            vec![
                [0.0, 0.0, 3.0],
                [5.0, 0.0, 3.0],
                [5.0, 5.0, 3.0],
                [0.0, 5.0, 3.0],
            ],
        );
        let f2 = ControlPlaneFacet::new(
            "SlopedFacet",
            vec![
                [5.0, 0.0, 3.0],
                [10.0, 0.0, 3.0],
                [10.0, 5.0, 6.0],
                [5.0, 5.0, 6.0],
            ],
        );
        let cp = ControlPlane::from_facets("HybridRoof", vec![f1, f2]);
        let cp_id = cp.id;
        storey.control_planes.push(cp);
        regenerate_control_plane_previews(&mut scene, &mut storey);

        let p = storey.plane(cp_id).unwrap();
        let h1 = Handle::new(p.facets[0].preview_handle.unwrap());
        let h2 = Handle::new(p.facets[1].preview_handle.unwrap());

        // Erase facet 1 (FlatFacet)
        let modified = erase_control_plane_facets_for_storey(&scene, &mut storey, &[h1]);
        assert!(modified);

        let updated = storey.plane(cp_id).unwrap();
        assert_eq!(updated.facets.len(), 1);
        assert_eq!(updated.facets[0].name, "SlopedFacet");
        assert!(updated.is_sloped());

        // Erasing remaining facet h2 is guarded and keeps the facet
        let modified2 = erase_control_plane_facets_for_storey(&scene, &mut storey, &[h2]);
        assert!(!modified2);
        assert_eq!(storey.plane(cp_id).unwrap().facets.len(), 1);
    }

    #[test]
    fn erase_both_previews_at_once_keeps_last_facet() {
        use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};
        let mut scene = Scene::new();
        let mut storey = StoreyRef::new("DG", 3.0, "dg.dwg");
        let f1 = ControlPlaneFacet::new(
            "FacetA",
            vec![
                [0.0, 0.0, 3.0],
                [5.0, 0.0, 3.0],
                [5.0, 5.0, 3.0],
                [0.0, 5.0, 3.0],
            ],
        );
        let f2 = ControlPlaneFacet::new(
            "FacetB",
            vec![
                [5.0, 0.0, 4.0],
                [10.0, 0.0, 4.0],
                [10.0, 5.0, 4.0],
                [5.0, 5.0, 4.0],
            ],
        );
        let cp = ControlPlane::from_facets("TwoFacets", vec![f1, f2]);
        let cp_id = cp.id;
        storey.control_planes.push(cp);
        regenerate_control_plane_previews(&mut scene, &mut storey);

        let p = storey.plane(cp_id).unwrap();
        let h1 = Handle::new(p.facets[0].preview_handle.unwrap());
        let h2 = Handle::new(p.facets[1].preview_handle.unwrap());

        // Erase both handles in a single operation
        let modified = erase_control_plane_facets_for_storey(&scene, &mut storey, &[h1, h2]);
        assert!(modified);

        let updated = storey.plane(cp_id).unwrap();
        assert_eq!(updated.facets.len(), 1);
        assert_eq!(updated.facets[0].name, "FacetA");
    }

    #[test]
    fn erase_untagged_ordinary_entity_with_colliding_handle_does_not_delete_facet() {
        use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};
        use acadrust::entities::Line;
        use acadrust::types::Vector3;

        let mut scene = Scene::new();
        let mut storey = StoreyRef::new("EG", 0.0, "eg.dwg");

        // Create an untagged line entity in the active drawing
        let mut line = Line::new();
        line.start = Vector3::new(0.0, 0.0, 0.0);
        line.end = Vector3::new(10.0, 0.0, 0.0);
        let ordinary_handle = scene.add_entity(EntityType::Line(line));

        // Create a plane with 2 facets where facet 1 happens to have the same preview_handle value
        // from a previous drawing session
        let mut f1 = ControlPlaneFacet::new("Facet1", vec![[0.0, 0.0, 0.0], [5.0, 0.0, 0.0], [5.0, 5.0, 0.0], [0.0, 5.0, 0.0]]);
        f1.preview_handle = Some(ordinary_handle.value());
        let f2 = ControlPlaneFacet::new("Facet2", vec![[5.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 5.0, 0.0], [5.0, 5.0, 0.0]]);
        let cp = ControlPlane::from_facets("TwoFacetPlane", vec![f1, f2]);
        let cp_id = cp.id;
        storey.control_planes.push(cp);

        // Erasing the untagged ordinary entity must NOT match or delete facet 1 of the plane
        let modified = erase_control_plane_facets_for_storey(&scene, &mut storey, &[ordinary_handle]);
        assert!(!modified);

        let plane = storey.plane(cp_id).unwrap();
        assert_eq!(plane.facets.len(), 2, "Both facets must remain intact");
    }

    #[test]
    fn erase_other_plane_tagged_entity_with_colliding_handle_does_not_delete_facet() {
        use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};
        use acadrust::entities::Polyline3D;
        use acadrust::types::Vector3;

        let mut scene = Scene::new();
        let mut storey = StoreyRef::new("EG", 0.0, "eg.dwg");

        // Plane A has 2 facets
        let f1 = ControlPlaneFacet::new("A_Facet1", vec![[0.0, 0.0, 0.0], [5.0, 0.0, 0.0], [5.0, 5.0, 0.0], [0.0, 5.0, 0.0]]);
        let f2 = ControlPlaneFacet::new("A_Facet2", vec![[5.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 5.0, 0.0], [5.0, 5.0, 0.0]]);
        let cp_a = ControlPlane::from_facets("PlaneA", vec![f1.clone(), f2]);
        let cp_a_id = cp_a.id;
        storey.control_planes.push(cp_a);

        // Plane B is another plane
        let cp_b = ControlPlane::from_facets("PlaneB", vec![f1]);
        let cp_b_id = cp_b.id;
        storey.control_planes.push(cp_b);

        // In scene, add an entity tagged for Plane B
        let pts = vec![Vector3::new(0.0, 0.0, 0.0), Vector3::new(5.0, 0.0, 0.0), Vector3::new(5.0, 5.0, 0.0)];
        let poly = Polyline3D::from_points(pts);
        let handle_b = scene.add_entity(EntityType::Polyline3D(poly));
        write_control_plane_tag_with_facet(&mut scene, handle_b, cp_b_id, "B_Facet1", Some(0));

        // Set Plane A's facet 1 preview_handle to match handle_b (cross-drawing collision scenario)
        storey.plane_mut(cp_a_id).unwrap().facets[0].preview_handle = Some(handle_b.value());

        // Erasing handle_b must NOT delete Plane A's facet because entity is tagged for Plane B
        let modified = erase_control_plane_facets_for_storey(&scene, &mut storey, &[handle_b]);
        assert!(!modified); // Plane B only has 1 facet, so it's protected; Plane A shouldn't match

        let plane_a = storey.plane(cp_a_id).unwrap();
        assert_eq!(plane_a.facets.len(), 2, "Plane A must not lose facets due to Plane B's entity");
    }

    #[test]
    fn erase_facet_preview_if_tagged_erases_own_preview() {
        use acadrust::entities::Polyline3D;

        let mut scene = Scene::new();
        let plane_id = Uuid::new_v4();
        let pts = vec![Vector3::new(0.0, 0.0, 0.0), Vector3::new(5.0, 0.0, 0.0), Vector3::new(5.0, 5.0, 0.0)];
        let h = scene.add_entity(EntityType::Polyline3D(Polyline3D::from_points(pts)));
        write_control_plane_tag_with_facet(&mut scene, h, plane_id, "F", Some(0));

        assert!(erase_facet_preview_if_tagged(&mut scene, plane_id, h.value()));
        assert!(scene.document.get_entity(h).is_none());
    }

    #[test]
    fn erase_facet_preview_if_tagged_keeps_untagged_entity_with_colliding_handle() {
        use acadrust::entities::Line;

        let mut scene = Scene::new();
        let mut line = Line::new();
        line.start = Vector3::new(0.0, 0.0, 0.0);
        line.end = Vector3::new(10.0, 0.0, 0.0);
        let h = scene.add_entity(EntityType::Line(line));

        assert!(!erase_facet_preview_if_tagged(&mut scene, Uuid::new_v4(), h.value()));
        assert!(scene.document.get_entity(h).is_some());
    }

    #[test]
    fn erase_facet_preview_if_tagged_keeps_entity_tagged_for_other_plane() {
        use acadrust::entities::Polyline3D;

        let mut scene = Scene::new();
        let other_plane = Uuid::new_v4();
        let pts = vec![Vector3::new(0.0, 0.0, 0.0), Vector3::new(5.0, 0.0, 0.0), Vector3::new(5.0, 5.0, 0.0)];
        let h = scene.add_entity(EntityType::Polyline3D(Polyline3D::from_points(pts)));
        write_control_plane_tag_with_facet(&mut scene, h, other_plane, "F", Some(0));

        assert!(!erase_facet_preview_if_tagged(&mut scene, Uuid::new_v4(), h.value()));
        assert!(scene.document.get_entity(h).is_some());
    }

    #[test]
    fn erase_facet_preview_if_tagged_missing_handle_is_noop() {
        let mut scene = Scene::new();
        assert!(!erase_facet_preview_if_tagged(&mut scene, Uuid::new_v4(), 0xDEAD_BEEF));
    }

    #[test]
    fn erase_stale_tag_index_matches_by_facet_name() {
        use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};
        use acadrust::entities::Polyline3D;
        use acadrust::types::Vector3;

        let mut scene = Scene::new();
        let mut storey = StoreyRef::new("EG", 0.0, "eg.dwg");

        // Plane has 2 facets: Facet0 and Facet2 (Facet1 was deleted earlier)
        let f0 = ControlPlaneFacet::new("Facet0", vec![[0.0, 0.0, 0.0], [5.0, 0.0, 0.0], [5.0, 5.0, 0.0], [0.0, 5.0, 0.0]]);
        let f2 = ControlPlaneFacet::new("Facet2", vec![[5.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 5.0, 0.0], [5.0, 5.0, 0.0]]);
        let cp = ControlPlane::from_facets("PlaneWithStaleTag", vec![f0, f2]);
        let cp_id = cp.id;
        storey.control_planes.push(cp);

        // Entity in drawing is tagged for cp_id with name "Facet2" and stale facet_idx 2 (which is >= facets.len() = 2)
        let pts = vec![Vector3::new(5.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0), Vector3::new(10.0, 5.0, 0.0)];
        let poly = Polyline3D::from_points(pts);
        let h = scene.add_entity(EntityType::Polyline3D(poly));
        write_control_plane_tag_with_facet(&mut scene, h, cp_id, "Facet2", Some(2));

        // Erasing h matches Facet2 by name, removing Facet2 and keeping Facet0
        let modified = erase_control_plane_facets_for_storey(&scene, &mut storey, &[h]);
        assert!(modified);

        let plane = storey.plane(cp_id).unwrap();
        assert_eq!(plane.facets.len(), 1);
        assert_eq!(plane.facets[0].name, "Facet0");
    }

    #[test]
    fn regenerate_control_plane_previews_does_not_erase_untagged_entity_with_colliding_handle() {
        use acadrust::entities::Line;
        use acadrust::types::Vector3;

        let mut scene = Scene::new();
        let mut storey = StoreyRef::new("EG", 0.0, "eg.dwg");

        // Create an untagged line in the scene
        let mut line = Line::new();
        line.start = Vector3::new(0.0, 0.0, 0.0);
        line.end = Vector3::new(10.0, 0.0, 0.0);
        let untagged_handle = scene.add_entity(EntityType::Line(line));

        // Storey's floor plane happens to have preview_handle matching untagged_handle from another drawing
        let floor_id = storey.floor_plane_id;
        storey.plane_mut(floor_id).unwrap().preview_handle = Some(untagged_handle.value());
        storey.plane_mut(floor_id).unwrap().facets[0].preview_handle = Some(untagged_handle.value());

        // Regenerate previews
        regenerate_control_plane_previews(&mut scene, &mut storey);

        // The untagged entity must NOT have been erased!
        assert!(scene.document.get_entity(untagged_handle).is_some(), "Untagged entity must not be erased");
    }

    #[test]
    fn erase_plane_previews_if_tagged_erases_all_and_only_matching_plane_previews() {
        use acadrust::entities::Polyline3D;
        use acadrust::types::Vector3;

        let mut scene = Scene::new();
        let target_plane = Uuid::new_v4();
        let other_plane = Uuid::new_v4();

        // 2 preview entities for target_plane
        let pts = vec![Vector3::new(0.0, 0.0, 0.0), Vector3::new(5.0, 0.0, 0.0), Vector3::new(5.0, 5.0, 0.0)];
        let h1 = scene.add_entity(EntityType::Polyline3D(Polyline3D::from_points(pts.clone())));
        write_control_plane_tag_with_facet(&mut scene, h1, target_plane, "F1", Some(0));
        let h2 = scene.add_entity(EntityType::Polyline3D(Polyline3D::from_points(pts.clone())));
        write_control_plane_tag_with_facet(&mut scene, h2, target_plane, "F2", Some(1));

        // 1 preview entity for other_plane
        let h_other = scene.add_entity(EntityType::Polyline3D(Polyline3D::from_points(pts)));
        write_control_plane_tag_with_facet(&mut scene, h_other, other_plane, "F_Other", Some(0));

        let erased = erase_plane_previews_if_tagged(&mut scene, target_plane);
        assert_eq!(erased, 2);
        assert!(scene.document.get_entity(h1).is_none());
        assert!(scene.document.get_entity(h2).is_none());
        assert!(scene.document.get_entity(h_other).is_some());
    }
}
