//! `AEC_IFCEXPORT` one-shot command.

use crate::modules::aec::engine::room_xdata::room_from_entity;
use crate::modules::aec::engine::slab_xdata::{slab_from_entity, slab_opening_from_entity};
use crate::modules::aec::engine::xdata::{read_aec_record, wall_from_entity};
use crate::modules::aec::engine::storey_xdata::STOREYS;
use crate::modules::aec::engine;
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
/// document XDATA (plus the in-memory storeys) into a minimal
/// [`engine::Scene`] ready for [`engine::ifc::write_spf`].
///
/// Split out from [`aec_ifc_export`] so the scanning logic can be exercised
/// directly in unit tests without going through `CommandLine` output.
fn collect_aec_ifc_scene(scene: &Scene) -> engine::Scene {
    let mut ifc_scene = engine::Scene::default();

    // Add in-memory storeys
    {
        let storeys = STOREYS.lock().unwrap();
        for (i, s) in storeys.iter().enumerate() {
            ifc_scene.storeys.push((i as u32, s.clone()));
        }
    }

    // Collect walls, rooms, slabs, and slab openings from document XDATA
    for entity in scene.document.entities() {
        let Some(record) = read_aec_record(entity) else {
            continue;
        };
        match record.values.first() {
            Some(XDataValue::String(kind)) if kind == "WALL" => {
                if let Some(wall) = wall_from_entity(entity) {
                    ifc_scene.walls.push(wall);
                }
            }
            Some(XDataValue::String(kind)) if kind == "SLAB" => {
                if let Some(slab) = slab_from_entity(entity) {
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
                if let Some(room) = room_from_entity(entity) {
                    ifc_scene.rooms.push(room);
                }
            }
            _ => {}
        }
    }

    ifc_scene
}

/// `AEC_IFCEXPORT` — collect walls/rooms/slabs/openings/storeys and emit
/// IFC4 SPF (in-memory).
pub fn aec_ifc_export(scene: &mut Scene, command_line: &mut CommandLine) {
    let ifc_scene = collect_aec_ifc_scene(scene);
    let ifc_data = engine::ifc::write_spf(&ifc_scene);
    command_line.push_info(&crate::tr!(
        "aec",
        "ifc-exported",
        bytes = ifc_data.len()
    ));
    command_line.push_info(&crate::tr!("aec", "ifc-note"));
}


inventory::submit!(crate::command::CommandRegistration { names: &["AEC_IFCEXPORT"] });

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::slab::Slab;
    use crate::modules::aec::engine::slab_opening::{SlabOpening, SlabOpeningKind};
    use crate::modules::aec::engine::slab_xdata::{write_slab_opening_record, write_slab_record};
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::EntityType;

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
    fn scene_without_slabs_collects_none() {
        let scene = Scene::new();
        let ifc_scene = collect_aec_ifc_scene(&scene);
        assert!(ifc_scene.slabs.is_empty());
        assert!(ifc_scene.slab_openings.is_empty());
    }
}
