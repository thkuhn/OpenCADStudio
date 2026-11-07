//! `AEC_IFCEXPORT` one-shot command.

use crate::modules::aec::engine::room_xdata::room_from_entity;
use crate::modules::aec::engine::slab_xdata::{slab_from_entity, slab_opening_from_entity};
use crate::modules::aec::engine;
use crate::modules::aec::engine::storey_xdata::linked_storey;
use crate::modules::aec::engine::xdata::{read_aec_record, wall_from_entity};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;
use acadrust::xdata::XDataValue;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_IFCEXPORT",
        label: "Export IFC",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/cui_export.svg")),
        event: ModuleEvent::Command("AEC_IFCEXPORT".to_string()),
    }
}

/// Collects walls, rooms, slabs, and slab openings referenced by `SCENE`'s
/// document XDATA (plus its linked project storey) into a minimal
/// [`engine::Scene`] ready for [`engine::ifc::write_spf`].
///
/// Split out from [`aec_ifc_export`] so the scanning logic can be exercised
/// directly in unit tests without going through `CommandLine` output.
fn collect_aec_ifc_scene(scene: &Scene) -> engine::Scene {
    let mut ifc_scene = engine::Scene::default();

    // Include the single project storey linked from this drawing, if present.
    let linked_storey = linked_storey(&scene.document);
    let linked_storey_id = linked_storey.as_ref().map(|(id, _, _)| *id);
    if let Some((storey_id, storey, _)) = linked_storey {
        ifc_scene.storeys.push((storey_id, storey));
    }

    // Collect walls, rooms, slabs, and slab openings from document XDATA
    for entity in scene.document.entities() {
        let Some(record) = read_aec_record(entity) else {
            continue;
        };
        match record.values.first() {
            Some(XDataValue::String(kind)) if kind == "WALL" => {
                if let Some(mut wall) = wall_from_entity(entity) {
                    if let Some(storey_id) = linked_storey_id {
                        wall.storey_id = storey_id;
                    }
                    ifc_scene.walls.push(wall);
                }
            }
            Some(XDataValue::String(kind)) if kind == "SLAB" => {
                if let Some(mut slab) = slab_from_entity(entity) {
                    if let Some(storey_id) = linked_storey_id {
                        slab.storey_id = storey_id;
                    }
                    ifc_scene.slabs.push((entity.common().handle, slab));
                }
            }
            Some(XDataValue::String(kind)) if kind == "SLAB_OPENING" => {
                if let Some(opening) = slab_opening_from_entity(entity) {
                    ifc_scene
                        .slab_openings
                        .push((entity.common().handle, opening));
                }
            }
            Some(XDataValue::String(kind)) if kind == "ROOM" => {
                if let Some(mut room) = room_from_entity(entity) {
                    if let Some(storey_id) = linked_storey_id {
                        room.storey_id = storey_id;
                    }
                    ifc_scene.rooms.push(room);
                }
            }
            _ => {}
        }
    }

    ifc_scene
}

/// Builds the IFC4 SPF text of `scene` and writes it to `path`.
/// Returns the number of bytes written.
pub fn write_ifc_file(scene: &Scene, path: &std::path::Path) -> std::io::Result<usize> {
    let ifc_scene = collect_aec_ifc_scene(scene);
    let ifc_data = engine::ifc::write_spf(&ifc_scene);
    std::fs::write(path, ifc_data.as_bytes())?;
    Ok(ifc_data.len())
}

/// `AEC_IFCEXPORT` — collect walls/rooms/slabs/openings/storeys, ask for a
/// target file and write the IFC4 SPF there.
pub fn aec_ifc_export(scene: &mut Scene, command_line: &mut CommandLine) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let picked = crate::sys::blocking_file_dialog()
            .add_filter("IFC", &["ifc"])
            .set_file_name("aec-export.ifc")
            .save_file();
        let Some(mut path) = picked else {
            command_line.push_info(&crate::tr!("aec", "ifc-cancelled"));
            return;
        };
        if path.extension().is_none() {
            path.set_extension("ifc");
        }
        match write_ifc_file(scene, &path) {
            Ok(bytes) => {
                crate::config::remember_dialog_dir(&path);
                command_line.push_info(&crate::tr!(
                    "aec",
                    "ifc-exported",
                    bytes = bytes
                ));
                command_line.push_info(&crate::tr!(
                    "aec",
                    "ifc-saved",
                    path = path.display().to_string()
                ));
            }
            Err(e) => command_line.push_error(&crate::tr!(
                "aec",
                "ifc-write-failed",
                error = e.to_string()
            )),
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (scene, command_line);
    }
}


inventory::submit!(crate::command::CommandRegistration { names: &["AEC_IFCEXPORT"] });

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::room::Room;
    use crate::modules::aec::engine::project::StoreyRef;
    use crate::modules::aec::engine::room_xdata::write_room_record;
    use crate::modules::aec::engine::slab::Slab;
    use crate::modules::aec::engine::slab_opening::{SlabOpening, SlabOpeningKind};
    use crate::modules::aec::engine::slab_xdata::{write_slab_opening_record, write_slab_record};
    use crate::modules::aec::engine::storey_xdata::write_storey_link;
    use crate::modules::aec::engine::wall::Wall;
    use crate::modules::aec::engine::xdata::{wall_record_for_wall, write_aec_record, AEC_APPID};
    use acadrust::entities::{LwPolyline, LwVertex, Point};
    use acadrust::types::{Vector2, Vector3};
    use acadrust::{EntityType, xdata::ExtendedDataRecord};

    fn add_slab_carrier(scene: &mut Scene, style_id: &str, storey_id: u32) -> acadrust::Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(4.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(4.0, 4.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 4.0)));
        let entity = EntityType::LwPolyline(pl);
        let handle = scene.add_entity(entity);
        let slab = Slab::new(style_id, storey_id);
        assert!(write_slab_record(&mut scene.document, handle, &slab));
        handle
    }

    fn add_slab_opening(
        scene: &mut Scene,
        host_slab: acadrust::Handle,
        kind: SlabOpeningKind,
    ) -> acadrust::Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(1.0, 1.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(2.0, 1.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(2.0, 2.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(1.0, 2.0)));
        let entity = EntityType::LwPolyline(pl);
        let handle = scene.add_entity(entity);
        let opening = SlabOpening::new_through_hole(
            host_slab,
            kind,
            vec![(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0)],
        );
        assert!(write_slab_opening_record(&mut scene.document, handle, &opening));
        handle
    }

    fn add_wall_carrier(scene: &mut Scene, storey_id: u32) -> acadrust::Handle {
        let mut polyline = LwPolyline::new();
        polyline.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        polyline.add_vertex(LwVertex::new(Vector2::new(2.0, 0.0)));
        let handle = scene.add_entity(EntityType::LwPolyline(polyline));
        let wall = Wall::new("standard_wall", 2.8, storey_id);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record_for_wall(&wall);
        assert!(write_aec_record(&mut scene.document, handle, record));
        handle
    }

    fn add_room_carrier(scene: &mut Scene, storey_id: u32) -> acadrust::Handle {
        let handle = scene.add_entity(EntityType::Point(Point::at(Vector3::ZERO)));
        let room = Room::from_polygon("Office", &[(0.0, 0.0), (3.0, 0.0), (3.0, 2.0)], 2.8, storey_id);
        assert!(write_room_record(&mut scene.document, handle, &room));
        handle
    }

    #[test]
    fn collects_slab_and_slab_opening_from_document_xdata() {
        let mut scene = Scene::new();
        let slab_handle = add_slab_carrier(&mut scene, "style_slab_conc_20", 0);
        let opening_handle = add_slab_opening(&mut scene, slab_handle, SlabOpeningKind::Stairwell);

        let ifc_scene = collect_aec_ifc_scene(&scene);

        assert_eq!(ifc_scene.slabs.len(), 1);
        assert_eq!(ifc_scene.slabs[0].0, slab_handle);
        assert_eq!(ifc_scene.slab_openings.len(), 1);
        assert_eq!(ifc_scene.slab_openings[0].0, opening_handle);
        assert_eq!(ifc_scene.slab_openings[0].1.host_slab, slab_handle);
    }

    #[test]
    fn exported_spf_voids_slab_with_its_opening() {
        let mut scene = Scene::new();
        let slab_handle = add_slab_carrier(&mut scene, "style_slab_flat_roof_40", 0);
        add_slab_opening(&mut scene, slab_handle, SlabOpeningKind::Skylight);

        let ifc_scene = collect_aec_ifc_scene(&scene);
        let spf = engine::ifc::write_spf(&ifc_scene);

        assert!(spf.contains("IFCSLAB("));
        assert!(spf.contains(".ROOF."));
        assert!(spf.contains("IFCOPENINGELEMENT("));
        assert!(spf.contains("IFCRELVOIDSELEMENT("));
    }

    #[test]
    fn write_ifc_file_creates_spf_file() {
        let mut scene = Scene::new();
        add_slab_carrier(&mut scene, "style_slab_conc_20", 0);
        let path = std::env::temp_dir().join(format!("ocs_ifc_test_{}.ifc", std::process::id()));
        let bytes = write_ifc_file(&scene, &path).expect("write");
        let text = std::fs::read_to_string(&path).expect("read");
        let _ = std::fs::remove_file(&path);
        assert_eq!(bytes, text.len());
        assert!(text.contains("IFCSLAB("));
    }

    #[test]
    fn scene_without_slabs_collects_none() {
        let scene = Scene::new();
        let ifc_scene = collect_aec_ifc_scene(&scene);
        assert!(ifc_scene.slabs.is_empty());
        assert!(ifc_scene.slab_openings.is_empty());
        assert!(ifc_scene.storeys.is_empty());
    }

    #[test]
    fn linked_storey_is_the_only_ifc_storey_and_owns_all_exported_elements() {
        let mut scene = Scene::new();
        let wall_handle = add_wall_carrier(&mut scene, 41);
        let room_handle = add_room_carrier(&mut scene, 42);
        let slab_handle = add_slab_carrier(&mut scene, "style_slab_conc_20", 43);
        let storey_ref = StoreyRef::new_with_height("First Floor", 3.4, 2.9, "first.dwg");
        write_storey_link(&mut scene, &storey_ref);

        let ifc_scene = collect_aec_ifc_scene(&scene);

        assert_eq!(ifc_scene.storeys.len(), 1);
        assert_eq!(ifc_scene.storeys[0].0, 0);
        assert_eq!(ifc_scene.storeys[0].1.name, "First Floor");
        assert_eq!(ifc_scene.storeys[0].1.elevation, storey_ref.derived_elevation());
        assert_eq!(ifc_scene.walls.len(), 1);
        assert_eq!(ifc_scene.walls[0].storey_id, ifc_scene.storeys[0].0);
        assert_eq!(ifc_scene.rooms.len(), 1);
        assert_eq!(ifc_scene.rooms[0].storey_id, ifc_scene.storeys[0].0);
        assert_eq!(ifc_scene.slabs.len(), 1);
        assert_eq!(ifc_scene.slabs[0].1.storey_id, ifc_scene.storeys[0].0);
        assert_eq!(wall_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap().storey_id, 41);
        assert_eq!(room_from_entity(scene.document.get_entity(room_handle).unwrap()).unwrap().storey_id, 42);
        assert_eq!(slab_from_entity(scene.document.get_entity(slab_handle).unwrap()).unwrap().storey_id, 43);
    }

    #[test]
    fn owner_index_without_project_link_is_not_exported_as_a_storey() {
        let mut scene = Scene::new();
        crate::modules::aec::engine::storey_xdata::ensure_storey_entity(&mut scene, 7, None);
        let wall_handle = add_wall_carrier(&mut scene, 51);
        let room_handle = add_room_carrier(&mut scene, 52);
        let slab_handle = add_slab_carrier(&mut scene, "style_slab_conc_20", 53);

        let ifc_scene = collect_aec_ifc_scene(&scene);

        assert!(ifc_scene.storeys.is_empty());
        assert_eq!(ifc_scene.walls[0].storey_id, 51);
        assert_eq!(ifc_scene.rooms[0].storey_id, 52);
        assert_eq!(ifc_scene.slabs[0].1.storey_id, 53);
        assert_eq!(wall_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap().storey_id, 51);
        assert_eq!(room_from_entity(scene.document.get_entity(room_handle).unwrap()).unwrap().storey_id, 52);
        assert_eq!(slab_from_entity(scene.document.get_entity(slab_handle).unwrap()).unwrap().storey_id, 53);
    }
}
