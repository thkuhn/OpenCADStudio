//! Control-plane Face3D previews in the storey drawing.

use acadrust::entities::EntityType;
use acadrust::types::Vector3;
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::Handle;
use uuid::Uuid;

use crate::modules::aec::commands::{
    ensure_controlplanes_layer, read_aec_record, write_aec_record, AEC_APPID,
    AEC_CONTROLPLANES_LAYER,
};
use crate::modules::aec::engine::control_plane::{
    preview_rectangle, DEFAULT_PREVIEW_ORIGIN_XY, DEFAULT_PREVIEW_SIZE,
};
use crate::modules::aec::engine::project::StoreyRef;
use crate::scene::Scene;

pub const CONTROLPLANE_TAG: &str = "CONTROLPLANE";
/// ACI cyan for extra planes; yellow for the storey elevation (main) plane.
const PREVIEW_COLOR_OTHER: i16 = 4;
const PREVIEW_COLOR_MAIN: i16 = 2;

pub fn write_control_plane_tag(scene: &mut Scene, handle: Handle, plane_id: Uuid, name: &str) {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String(CONTROLPLANE_TAG.to_string()));
    record.add_value(XDataValue::String(plane_id.to_string()));
    record.add_value(XDataValue::String(name.to_string()));
    write_aec_record(&mut scene.document, handle, record);
}

pub fn control_plane_from_entity(entity: &EntityType) -> Option<(Uuid, String)> {
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
    Some((uuid, name))
}

/// Rebuild Face3D previews for a storey's control planes on `AEC_CONTROLPLANES`.
pub fn regenerate_control_plane_previews(scene: &mut Scene, storey: &mut StoreyRef) {
    ensure_controlplanes_layer(scene);
    let mut stale = Vec::new();
    for plane in &storey.control_planes {
        if let Some(h) = plane.preview_handle {
            stale.push(Handle::new(h));
        }
    }
    if !stale.is_empty() {
        scene.erase_entities(&stale);
    }
    let floor_id = storey.floor_plane_id;
    for plane in &mut storey.control_planes {
        plane.preview_handle = None;
        if !plane.visible {
            continue;
        }
        plane.origin[0] = DEFAULT_PREVIEW_ORIGIN_XY;
        plane.origin[1] = DEFAULT_PREVIEW_ORIGIN_XY;
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
            let aci = if plane.id == floor_id {
                PREVIEW_COLOR_MAIN
            } else {
                PREVIEW_COLOR_OTHER
            };
            ent.set_color(acadrust::types::Color::from_index(aci));
        }
        write_control_plane_tag(scene, handle, plane.id, &plane.name);
        plane.preview_handle = Some(handle.value());
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
}
