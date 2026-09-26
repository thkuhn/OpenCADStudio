//! Control-plane Face3D previews in the storey drawing.

use acadrust::entities::EntityType;
use acadrust::types::Vector3;
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::Handle;
use uuid::Uuid;

use crate::modules::aec::engine::wall_regen::{ensure_controlplanes_layer, AEC_CONTROLPLANES_LAYER};
use crate::modules::aec::engine::xdata::{read_aec_record, write_aec_record, AEC_APPID};
use crate::modules::aec::engine::control_plane::{
    preview_rectangle, DEFAULT_PREVIEW_ORIGIN_XY, DEFAULT_PREVIEW_SIZE,
};
use crate::modules::aec::engine::project::StoreyRef;
use crate::scene::Scene;

pub const CONTROLPLANE_TAG: &str = "CONTROLPLANE";
/// ACI cyan for extra planes; yellow for the storey elevation (main) plane.
const PREVIEW_COLOR_OTHER: i16 = 4;
const PREVIEW_COLOR_MAIN: i16 = 2;
/// Orange color (ACI 30 / RGB 255, 127, 0) for temporarily highlighting a control plane.
pub const PREVIEW_COLOR_HIGHLIGHT: i16 = 30;

/// Spawns temporary transparent Face3D fill entities in orange (ACI 30, 60% transparent)
/// covering the target control plane or its facets without altering existing line colors.
pub fn create_control_plane_transparent_highlight(
    scene: &mut Scene,
    storey: &StoreyRef,
    plane_id: Uuid,
) -> Vec<Handle> {
    ensure_controlplanes_layer(scene);
    let mut spawned = Vec::new();
    let Some(plane) = storey.plane(plane_id) else {
        return spawned;
    };
    let v = |c: [f64; 3]| Vector3::new(c[0], c[1], c[2]);
    if !plane.facets.is_empty() {
        for facet in &plane.facets {
            if facet.vertices.len() < 3 {
                continue;
            }
            let v0 = facet.vertices[0];
            let v1 = facet.vertices[1];
            let v2 = facet.vertices[2];
            let v3 = facet.vertices.get(3).copied().unwrap_or(facet.vertices[0]);
            let mut face = acadrust::entities::Face3D::new(v(v0), v(v1), v(v2), v(v3));
            face.invisible_edges = acadrust::entities::face3d::InvisibleEdgeFlags::from_bits(15);
            let handle = scene.add_entity(EntityType::Face3D(face));
            if let Some(e) = scene.document.get_entity_mut(handle) {
                let ent = e.as_entity_mut();
                ent.set_layer(AEC_CONTROLPLANES_LAYER.to_string());
                ent.set_color(acadrust::types::Color::from_index(PREVIEW_COLOR_HIGHLIGHT));
                ent.set_transparency(acadrust::types::Transparency::from_percent(0.6));
            }
            spawned.push(handle);
        }
    } else {
        let corners = preview_rectangle(plane, DEFAULT_PREVIEW_SIZE);
        let mut face = acadrust::entities::Face3D::new(
            v(corners[0]),
            v(corners[1]),
            v(corners[2]),
            v(corners[3]),
        );
        face.invisible_edges = acadrust::entities::face3d::InvisibleEdgeFlags::from_bits(15);
        let handle = scene.add_entity(EntityType::Face3D(face));
        if let Some(e) = scene.document.get_entity_mut(handle) {
            let ent = e.as_entity_mut();
            ent.set_layer(AEC_CONTROLPLANES_LAYER.to_string());
            ent.set_color(acadrust::types::Color::from_index(PREVIEW_COLOR_HIGHLIGHT));
            ent.set_transparency(acadrust::types::Transparency::from_percent(0.6));
        }
        spawned.push(handle);
    }
    spawned
}

/// Spawns a temporary transparent Face3D fill entity covering a specific facet.
pub fn create_facet_transparent_highlight(
    scene: &mut Scene,
    storey: &StoreyRef,
    plane_id: Uuid,
    facet_idx: usize,
) -> Vec<Handle> {
    ensure_controlplanes_layer(scene);
    let mut spawned = Vec::new();
    let Some(plane) = storey.plane(plane_id) else {
        return spawned;
    };
    let Some(facet) = plane.facets.get(facet_idx) else {
        return spawned;
    };
    if facet.vertices.len() < 3 {
        return spawned;
    }
    let v = |c: [f64; 3]| Vector3::new(c[0], c[1], c[2]);
    let v0 = facet.vertices[0];
    let v1 = facet.vertices[1];
    let v2 = facet.vertices[2];
    let v3 = facet.vertices.get(3).copied().unwrap_or(facet.vertices[0]);
    let mut face = acadrust::entities::Face3D::new(v(v0), v(v1), v(v2), v(v3));
    face.invisible_edges = acadrust::entities::face3d::InvisibleEdgeFlags::from_bits(15);
    let handle = scene.add_entity(EntityType::Face3D(face));
    if let Some(e) = scene.document.get_entity_mut(handle) {
        let ent = e.as_entity_mut();
        ent.set_layer(AEC_CONTROLPLANES_LAYER.to_string());
        ent.set_color(acadrust::types::Color::from_index(PREVIEW_COLOR_HIGHLIGHT));
        ent.set_transparency(acadrust::types::Transparency::from_percent(0.6));
    }
    spawned.push(handle);
    spawned
}

pub fn highlight_control_plane(scene: &mut Scene, storey: &StoreyRef, plane_id: Uuid) -> Vec<Handle> {
    create_control_plane_transparent_highlight(scene, storey, plane_id)
}

pub fn highlight_control_plane_facet(
    scene: &mut Scene,
    storey: &StoreyRef,
    plane_id: Uuid,
    facet_idx: usize,
) -> Vec<Handle> {
    create_facet_transparent_highlight(scene, storey, plane_id, facet_idx)
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

        if let Some(h) = plane.preview_handle {
            let handle = Handle::new(h);
            if let Some(e) = scene.document.get_entity_mut(handle) {
                e.as_entity_mut().set_color(acadrust::types::Color::from_index(default_color));
                touched.push(handle);
            }
        }
        for facet in &plane.facets {
            if let Some(h) = facet.preview_handle {
                let handle = Handle::new(h);
                if let Some(e) = scene.document.get_entity_mut(handle) {
                    e.as_entity_mut().set_color(acadrust::types::Color::from_index(default_color));
                    touched.push(handle);
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
    for plane in &storey.control_planes {
        if let Some(h) = plane.preview_handle {
            stale.push(Handle::new(h));
        }
        for facet in &plane.facets {
            if let Some(h) = facet.preview_handle {
                stale.push(Handle::new(h));
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
        if !plane.facets.is_empty() {
            for (idx, facet) in plane.facets.iter_mut().enumerate() {
                if facet.vertices.len() < 3 {
                    continue;
                }
                let v0 = facet.vertices[0];
                let v1 = facet.vertices[1];
                let v2 = facet.vertices[2];
                let v3 = facet.vertices.get(3).copied().unwrap_or(facet.vertices[0]);
                let v = |c: [f64; 3]| Vector3::new(c[0], c[1], c[2]);
                let face = acadrust::entities::Face3D::new(v(v0), v(v1), v(v2), v(v3));
                let handle = scene.add_entity(EntityType::Face3D(face));
                if let Some(e) = scene.document.get_entity_mut(handle) {
                    let ent = e.as_entity_mut();
                    ent.set_layer(AEC_CONTROLPLANES_LAYER.to_string());
                    ent.set_color(acadrust::types::Color::from_index(aci));
                }
                write_control_plane_tag_with_facet(scene, handle, plane.id, &facet.name, Some(idx));
                facet.preview_handle = Some(handle.value());
            }
        } else {
            if !plane.is_sloped() {
                plane.origin[0] = DEFAULT_PREVIEW_ORIGIN_XY;
                plane.origin[1] = DEFAULT_PREVIEW_ORIGIN_XY;
            }
            let corners = preview_rectangle(plane, DEFAULT_PREVIEW_SIZE);
            let v = |c: [f64; 3]| Vector3::new(c[0], c[1], c[2]);
            let face = acadrust::entities::Face3D::new(
                v(corners[0]),
                v(corners[1]),
                v(corners[2]),
                v(corners[3]),
            );
            let handle = scene.add_entity(EntityType::Face3D(face));
            if let Some(e) = scene.document.get_entity_mut(handle) {
                let ent = e.as_entity_mut();
                ent.set_layer(AEC_CONTROLPLANES_LAYER.to_string());
                ent.set_color(acadrust::types::Color::from_index(aci));
            }
            write_control_plane_tag_with_facet(scene, handle, plane.id, &plane.name, None);
            plane.preview_handle = Some(handle.value());
        }
    }
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
        if let EntityType::Face3D(f) = ent {
            assert!((f.first_corner.x + 5.0).abs() < 1e-6);
            assert!((f.first_corner.y + 5.0).abs() < 1e-6);
            assert!((f.second_corner.x - 45.0).abs() < 1e-6);
            assert!((f.fourth_corner.y - 45.0).abs() < 1e-6);
        } else {
            panic!("expected Face3D");
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
        if let EntityType::Face3D(f) = ent {
            assert!((f.first_corner.z - 6.0).abs() > 0.01 || (f.third_corner.z - 6.0).abs() > 0.01);
        } else {
            panic!("expected Face3D");
        }
    }

    #[test]
    fn highlight_control_plane_creates_transparent_highlight() {
        let mut scene = Scene::new();
        let mut storey = StoreyRef::new("EG", 0.0, "eg.dwg");
        let ceiling_id = storey.ceiling_plane_id;
        regenerate_control_plane_previews(&mut scene, &mut storey);

        let floor_handle = Handle::new(storey.plane(storey.floor_plane_id).unwrap().preview_handle.unwrap());
        let ceiling_handle = Handle::new(storey.plane(ceiling_id).unwrap().preview_handle.unwrap());

        assert_eq!(scene.document.get_entity(floor_handle).unwrap().as_entity().color().index(), Some(PREVIEW_COLOR_MAIN as u16));
        assert_eq!(scene.document.get_entity(ceiling_handle).unwrap().as_entity().color().index(), Some(PREVIEW_COLOR_OTHER as u16));

        let spawned = highlight_control_plane(&mut scene, &storey, ceiling_id);
        assert!(!spawned.is_empty());
        let hl = scene.document.get_entity(spawned[0]).unwrap();
        assert_eq!(hl.as_entity().color().index(), Some(PREVIEW_COLOR_HIGHLIGHT as u16));
        assert!(hl.as_entity().transparency().as_percent() > 0.0);
        // Original line colors are untouched
        assert_eq!(scene.document.get_entity(ceiling_handle).unwrap().as_entity().color().index(), Some(PREVIEW_COLOR_OTHER as u16));
        assert_eq!(scene.document.get_entity(floor_handle).unwrap().as_entity().color().index(), Some(PREVIEW_COLOR_MAIN as u16));
    }

    #[test]
    fn highlight_control_plane_facet_creates_transparent_facet_highlight() {
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

        let spawned = highlight_control_plane_facet(&mut scene, &storey, cp_id, 1);
        assert_eq!(spawned.len(), 1);
        let hl = scene.document.get_entity(spawned[0]).unwrap();
        assert_eq!(hl.as_entity().color().index(), Some(PREVIEW_COLOR_HIGHLIGHT as u16));
        assert!(hl.as_entity().transparency().as_percent() > 0.0);

        // Preview entity lines remain untouched
        assert_eq!(scene.document.get_entity(h1).unwrap().as_entity().color().index(), Some(PREVIEW_COLOR_OTHER as u16));
        assert_eq!(scene.document.get_entity(h2).unwrap().as_entity().color().index(), Some(PREVIEW_COLOR_OTHER as u16));
    }
}
