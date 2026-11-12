use super::*;
use acadrust::entities::LwPolyline;
use acadrust::xdata::ExtendedDataRecord;
use crate::modules::aec::engine::xdata::{wall_record_for_wall, AEC_APPID};
use crate::modules::aec::engine::project::{Building, ProjectFile, StoreyRef};
use crate::modules::aec::engine::wall::Wall;

#[test]
fn wall_plane_fields_are_base_then_offset_then_top() {
    let storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
    let mut wall = Wall::new("s", 2.5, 0);
    wall.base_plane_id = Some(storey.floor_plane_id);
    wall.top_plane_id = Some(storey.ceiling_plane_id);
    wall.base_offset = 0.1;
    wall.top_offset = -0.05;
    let mut entity = EntityType::LwPolyline(LwPolyline::new());
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    entity.common_mut().extended_data.add_record(record);

    let mut project = ProjectFile::default();
    let mut building = Building::new("B");
    building.storeys.push(storey);
    project.buildings.push(building);

    let section = wall_prop_section(&entity, None, Some(&project)).expect("section");
    let fields: Vec<&str> = section.props.iter().map(|p| p.field).collect();
    let base = fields.iter().position(|f| *f == "wall_base_plane").unwrap();
    let base_off = fields.iter().position(|f| *f == "wall_base_offset").unwrap();
    let top = fields.iter().position(|f| *f == "wall_top_plane").unwrap();
    let top_off = fields.iter().position(|f| *f == "wall_top_offset").unwrap();
    assert!(base < base_off && base_off < top && top < top_off);

    let base_sel = match &section.props[base].value {
        crate::scene::model::object::PropValue::Choice { selected, .. } => selected,
        _ => panic!("choice"),
    };
    assert!(base_sel.contains("ELEVATION") || !base_sel.is_empty());
    assert_ne!(base_sel, "(none)");
}

#[test]
fn circle_width_edit_locks_height() {
    let mut o = Opening::window(Handle::new(1), Handle::new(2), 1.0);
    o.shape = OpeningShape::Circle;
    o.width = 1.2;
    o.height = 1.2;
    assert!(apply_opening_property(&mut o, "opening_width", "1.8", None));
    assert!((o.width - 1.8).abs() < 1e-12);
    assert!((o.height - 1.8).abs() < 1e-12);
    assert!(apply_opening_property(&mut o, "opening_height", "1.0", None));
    assert!((o.width - 1.0).abs() < 1e-12);
    assert!((o.height - 1.0).abs() < 1e-12);
}

#[test]
fn equilateral_width_sets_height() {
    let mut o = Opening::window(Handle::new(1), Handle::new(2), 1.0);
    o.shape = OpeningShape::Triangle(
        crate::modules::aec::engine::opening_shape::TriangleVariant::Equilateral,
    );
    assert!(apply_opening_property(&mut o, "opening_width", "2.0", None));
    assert!((o.height - OpeningShape::equilateral_height(2.0)).abs() < 1e-12);
}

#[test]
fn hinge_and_arch_spring_apply() {
    let mut o = Opening::door(Handle::new(1), Handle::new(2), 1.0);
    assert!(apply_opening_property(&mut o, "opening_hinge", "Right", None));
    assert_eq!(o.hinge, LeafOperation::right());
    assert!(apply_opening_property(&mut o, "opening_shape", "Arch", None));
    assert_eq!(o.shape, OpeningShape::Arch);
    assert!(o.spring_height > 0.0);
    assert!(apply_opening_property(&mut o, "opening_spring", "0.4", None));
    assert!((o.spring_height - 0.4).abs() < 1e-12);
}

#[test]
fn numeric_sill_and_height_unbind_planes() {
    let mut o = Opening::window(Handle::new(1), Handle::new(2), 1.0);
    o.sill_plane_id = Some(uuid::Uuid::new_v4());
    o.head_plane_id = Some(uuid::Uuid::new_v4());
    o.sill_plane_name = Some("A".into());
    o.head_plane_name = Some("B".into());
    assert!(apply_opening_property(&mut o, "opening_sill", "0.4", None));
    assert!((o.sill_height - 0.4).abs() < 1e-12);
    assert!(o.sill_plane_id.is_none());
    assert!(o.head_plane_id.is_some());
    assert!(apply_opening_property(&mut o, "opening_height", "1.5", None));
    assert!((o.height - 1.5).abs() < 1e-12);
    assert!(o.head_plane_id.is_none());
}

#[test]
fn opening_plane_fields_follow_sill_then_offsets() {
    let storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
    let mut o = Opening::window(Handle::new(1), Handle::new(2), 1.0);
    o.sill_plane_id = Some(storey.floor_plane_id);
    o.head_plane_id = Some(storey.ceiling_plane_id);
    let mut project = ProjectFile::default();
    let mut building = Building::new("B");
    building.storeys.push(storey);
    project.buildings.push(building);
    let section = opening_prop_section(&o, Handle::new(1), None, Some(&project));
    let fields: Vec<&str> = section.props.iter().map(|p| p.field).collect();
    let sill = fields.iter().position(|f| *f == "opening_sill").unwrap();
    let sill_pl = fields
        .iter()
        .position(|f| *f == "opening_sill_plane")
        .unwrap();
    let sill_off = fields
        .iter()
        .position(|f| *f == "opening_sill_offset")
        .unwrap();
    let head_pl = fields
        .iter()
        .position(|f| *f == "opening_head_plane")
        .unwrap();
    let head_off = fields
        .iter()
        .position(|f| *f == "opening_head_offset")
        .unwrap();
    assert!(sill < sill_pl && sill_pl < sill_off && sill_off < head_pl && head_pl < head_off);
}

#[test]
fn slab_prop_section_exposes_style_justification_and_metrics() {
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use crate::modules::aec::engine::slab::{Slab, SlabJustification, SlabLayer};
    use crate::modules::aec::engine::slab_xdata::{slab_record_for_slab, write_slab_record};
    use crate::modules::aec::engine::wall_style::LayerFunction;
    use crate::modules::aec::engine::xdata::AEC_APPID;

    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(4.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(4.0, 3.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 3.0)));
    pl.is_closed = true;
    let mut entity = EntityType::LwPolyline(pl);
    let mut slab = Slab::new("style_slab_conc", 1);
    slab.justification = SlabJustification::StructuralTop;
    slab.layers = vec![
        SlabLayer::new("Tiles", 0.02, LayerFunction::Finish),
        SlabLayer::new("Concrete", 0.20, LayerFunction::Structural),
    ];
    slab.base_offset = 0.05;
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = slab_record_for_slab(&slab);
    entity.common_mut().extended_data.add_record(record);

    let section = slab_prop_section(&entity, None, None).expect("slab section");
    let fields: Vec<&str> = section.props.iter().map(|p| p.field).collect();
    assert!(fields.contains(&"slab_style"));
    assert!(fields.contains(&"slab_justification"));
    assert!(fields.contains(&"slab_phase"));
    assert!(!fields.contains(&"slab_base_plane"));
    assert!(fields.contains(&"slab_top_plane"));
    assert!(!fields.contains(&"slab_base_offset"));
    assert!(fields.contains(&"slab_top_offset"));
    assert!(fields.contains(&"slab_thickness"));
    assert!(fields.contains(&"slab_area"));
    assert!(fields.contains(&"slab_perimeter"));
    assert!(fields.contains(&"slab_volume"));
    assert!(fields.iter().any(|f| *f == "slab_layer"));

    let just = section
        .props
        .iter()
        .find(|p| p.field == "slab_justification")
        .expect("justification");
    match &just.value {
        crate::scene::model::object::PropValue::Choice { selected, .. } => {
            assert!(selected.contains("OKRD"));
        }
        _ => panic!("expected choice"),
    }

    // Keep write helper referenced for compile linkage in tests module.
    let _ = write_slab_record;
}

#[test]
fn test_property_section_plane_dropdown_formatting_and_disambiguation() {
    use acadrust::entities::LwPolyline;
    use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};
    use crate::modules::aec::engine::openings::Opening;
    use crate::modules::aec::engine::slab::Slab;
    use crate::modules::aec::engine::slab_xdata::slab_record_for_slab;
    use crate::modules::aec::engine::wall::Wall;
    use crate::modules::aec::engine::xdata::{wall_record_for_wall, AEC_APPID};

    let mut project = ProjectFile::default();
    let mut building = Building::new("Hauptbau");
    let mut eg = StoreyRef::new_with_height("EG", 0.0, 2.80, "eg.dwg");
    let mut og = StoreyRef::new_with_height("OG", 2.80, 2.80, "og.dwg");

    let eg_custom_floor = ControlPlane::horizontal("Floor", 0.0);
    let eg_custom_floor_id = eg_custom_floor.id;
    eg.control_planes.push(eg_custom_floor);

    let og_custom_floor = ControlPlane::horizontal("Floor", 2.80);
    og.control_planes.push(og_custom_floor);

    let facet_roof = ControlPlaneFacet::new(
        "Nord",
        vec![
            [0.0, 0.0, 6.0],
            [10.0, 0.0, 6.0],
            [10.0, 10.0, 10.0],
            [0.0, 10.0, 10.0],
        ],
    );
    let roof_cp = ControlPlane::from_facets("Dach (Nord)", vec![facet_roof]);
    let roof_id = roof_cp.id;
    og.control_planes.push(roof_cp);

    let eg_ceiling_id = eg.ceiling_plane_id;

    building.storeys.push(eg);
    building.storeys.push(og);
    project.buildings.push(building);

    // 1. Wall Property Section
    let mut wall = Wall::new("s", 2.80, 0);
    wall.base_plane_id = Some(eg_custom_floor_id);
    wall.top_plane_id = Some(roof_id);
    let mut wall_entity = EntityType::LwPolyline(LwPolyline::new());
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = wall_record_for_wall(&wall);
    wall_entity.common_mut().extended_data.add_record(rec);

    let wall_sec = wall_prop_section(&wall_entity, None, Some(&project)).expect("wall section");
    let base_prop = wall_sec.props.iter().find(|p| p.field == "wall_base_plane").unwrap();
    match &base_prop.value {
        crate::scene::model::object::PropValue::Choice { selected, options } => {
            assert_eq!(selected, "Floor [EG] (Z = 0.000 m)");
            assert!(options.contains(&"Floor [EG] (Z = 0.000 m)".to_string()));
            assert!(options.contains(&"Floor [OG] (Z = 2.800 m)".to_string()));
            assert!(options.iter().any(|o| o.starts_with("Dach (Nord) (Z ≈ 6.000 m, ")));
        }
        _ => panic!("expected choice"),
    }
    let top_prop = wall_sec.props.iter().find(|p| p.field == "wall_top_plane").unwrap();
    match &top_prop.value {
        crate::scene::model::object::PropValue::Choice { selected, .. } => {
            assert!(selected.starts_with("Dach (Nord) (Z ≈ 6.000 m, "));
        }
        _ => panic!("expected choice"),
    }

    // 2. Opening Property Section
    let mut opening = Opening::window(Handle::new(10), Handle::new(20), 1.0);
    opening.sill_plane_id = Some(eg_custom_floor_id);
    opening.head_plane_id = Some(eg_ceiling_id);
    let op_sec = opening_prop_section(&opening, Handle::new(10), None, Some(&project));
    let sill_prop = op_sec.props.iter().find(|p| p.field == "opening_sill_plane").unwrap();
    match &sill_prop.value {
        crate::scene::model::object::PropValue::Choice { selected, .. } => {
            assert_eq!(selected, "Floor [EG] (Z = 0.000 m)");
        }
        _ => panic!("expected choice"),
    }

    // 3. Slab Property Section
    let mut slab = Slab::new("style_slab", 1);
    slab.top_plane_id = Some(roof_id);
    let mut slab_entity = EntityType::LwPolyline(LwPolyline::new());
    let mut srec = ExtendedDataRecord::new(AEC_APPID);
    srec.values = slab_record_for_slab(&slab);
    slab_entity.common_mut().extended_data.add_record(srec);

    let slab_sec = slab_prop_section(&slab_entity, None, Some(&project)).expect("slab section");
    let slab_top_prop = slab_sec.props.iter().find(|p| p.field == "slab_top_plane").unwrap();
    match &slab_top_prop.value {
        crate::scene::model::object::PropValue::Choice { selected, options } => {
            assert!(selected.starts_with("Dach (Nord) (Z ≈ 6.000 m, "));
            assert!(options.contains(&"Floor [EG] (Z = 0.000 m)".to_string()));
        }
        _ => panic!("expected choice"),
    }
}

#[test]
fn slab_opening_prop_section_exposes_kind_and_depth() {
    use crate::modules::aec::engine::slab_opening::{
        SlabOpening, SlabOpeningDepth, SlabOpeningKind,
    };

    let opening = SlabOpening::new_recess(
        Handle::new(10),
        SlabOpeningKind::Shaft,
        0.12,
        vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
    );
    let entity = EntityType::LwPolyline(LwPolyline::new());
    let section = slab_opening_prop_section(&opening, Handle::new(11), &entity);
    let fields: Vec<&str> = section.props.iter().map(|p| p.field).collect();
    assert!(fields.contains(&"slabopening_kind"));
    assert!(fields.contains(&"slabopening_depth_mode"));
    assert!(fields.contains(&"slabopening_recess_depth"));
    assert!(fields.contains(&"slabopening_host"));
    assert!(fields.contains(&"slabopening_area"));
    assert_eq!(opening.depth, SlabOpeningDepth::Recess(0.12));
}

#[test]
fn test_aec_entity_title_and_package_collapse_for_slabs_and_openings() {
    use crate::modules::aec::engine::library;
    use crate::modules::aec::engine::slab::Slab;
    use crate::modules::aec::engine::slab_package::{
        collect_slab_display_children, is_slab_derived, resolve_slab_package,
    };
    use crate::modules::aec::engine::slab_regen::regenerate_slab_representation;
    use crate::modules::aec::engine::slab_xdata::write_slab_record;
    use crate::scene::Scene;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;

    let mut scene = Scene::new();
    let lib = library::seed_default_library();

    let mut pl = LwPolyline::new();
    pl.is_closed = true;
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(6.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(6.0, 4.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 4.0)));
    let slab_h = scene.add_entity(EntityType::LwPolyline(pl));

    let slab = Slab::new("style_slab_concrete_20", 0);
    write_slab_record(&mut scene.document, slab_h, &slab);
    regenerate_slab_representation(&mut scene, slab_h, Some(&lib), None);

    let children = collect_slab_display_children(&scene, slab_h);
    assert!(!children.is_empty(), "must produce derived children");

    // Carrier entity title
    let carrier_ent = scene.document.get_entity(slab_h).unwrap();
    let title = aec_entity_title(&scene, slab_h, carrier_ent).expect("title");
    assert!(title.contains("Slab") || title.contains("Geschossdecke"));

    // For each derived child, resolve_aec_package and aec_entity_title must resolve to slab
    for &child_h in &children {
        assert!(is_slab_derived(&scene, child_h));
        assert_eq!(resolve_slab_package(&scene, child_h), slab_h);
        assert_eq!(resolve_aec_package(&scene, child_h), slab_h);
        let child_ent = scene.document.get_entity(child_h).unwrap();
        let child_title = aec_entity_title(&scene, child_h, child_ent).expect("child title");
        assert!(child_title.contains("Slab") || child_title.contains("Geschossdecke"));

        // Package collapse
        let selected = vec![(child_h, child_ent)];
        let collapsed = collapse_selection_to_aec_package(&scene, selected);
        assert_eq!(collapsed.len(), 1);
        assert_eq!(collapsed[0].0, slab_h);
    }
}

#[test]
fn test_aec_entity_title_and_package_collapse_for_rooms() {
    use crate::modules::aec::engine::room::{Room, RoomFinish, RoomFunction};
    use crate::modules::aec::engine::room_package::{
        collect_room_display_children, expand_handles_for_room_packages, is_room_derived,
        resolve_room_package_handle,
    };
    use crate::modules::aec::engine::room_regen::regenerate_room_representation;
    use crate::modules::aec::engine::room_xdata::{room_from_entity, write_room_record};
    use crate::scene::Scene;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;

    let mut scene = Scene::new();

    let mut pl = LwPolyline::new();
    pl.is_closed = true;
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 4.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 4.0)));
    let room_h = scene.add_entity(EntityType::LwPolyline(pl));

    let room = Room::from_polygon("Living", &[(0.0, 0.0), (5.0, 0.0), (5.0, 4.0), (0.0, 4.0)], 2.50, 0)
        .with_number("101")
        .with_function(RoomFunction::Living)
        .with_floor_finish(vec![RoomFinish::new("Parkett", 0.05).with_hatch("ANSI31".to_string())]);
    write_room_record(&mut scene.document, room_h, &room);
    regenerate_room_representation(&mut scene, room_h, None);

    let children = collect_room_display_children(&scene, room_h);
    assert!(!children.is_empty(), "must produce derived children (stamp, hatch)");

    // Carrier entity title
    let carrier_ent = scene.document.get_entity(room_h).unwrap();
    let title = aec_entity_title(&scene, room_h, carrier_ent).expect("title");
    assert!(title.contains("Room") || title.contains("Raum"));

    // Carrier area
    let parsed_room = room_from_entity(carrier_ent).expect("parsed room");
    assert!((parsed_room.area - 20.0).abs() < 1e-4);

    // Package expansion
    let expanded = expand_handles_for_room_packages(&scene, &[room_h]);
    assert!(expanded.contains(&room_h));
    for &child_h in &children {
        assert!(expanded.contains(&child_h));
    }

    // For each derived child, resolve_aec_package and aec_entity_title must resolve to room
    for &child_h in &children {
        assert!(is_room_derived(scene.document.get_entity(child_h).unwrap()));
        assert_eq!(resolve_room_package_handle(&scene, child_h), room_h);
        assert_eq!(resolve_aec_package(&scene, child_h), room_h);
        let child_ent = scene.document.get_entity(child_h).unwrap();
        let child_title = aec_entity_title(&scene, child_h, child_ent).expect("child title");
        assert!(child_title.contains("Room") || child_title.contains("Raum"));

        // Package collapse for child
        let selected = vec![(child_h, child_ent)];
        let collapsed = collapse_selection_to_aec_package(&scene, selected);
        assert_eq!(collapsed.len(), 1);
        assert_eq!(collapsed[0].0, room_h);
    }

    // Multiple children + carrier collapse to single room
    let mut all_selected = vec![(room_h, carrier_ent)];
    for &child_h in &children {
        all_selected.push((child_h, scene.document.get_entity(child_h).unwrap()));
    }
    let collapsed_all = collapse_selection_to_aec_package(&scene, all_selected);
    assert_eq!(collapsed_all.len(), 1);
    assert_eq!(collapsed_all[0].0, room_h);
}

#[test]
fn test_room_prop_section_modular_styles() {
    use crate::modules::aec::engine::room::{Room, RoomFunction};
    use crate::modules::aec::engine::room_xdata::write_room_record;
    use crate::modules::aec::engine::library;
    use crate::scene::Scene;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;

    let mut scene = Scene::new();
    let lib = library::seed_default_library();

    let mut pl = LwPolyline::new();
    pl.is_closed = true;
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 4.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 4.0)));
    let room_h = scene.add_entity(EntityType::LwPolyline(pl));

    let room = Room::from_polygon("Office", &[(0.0, 0.0), (5.0, 0.0), (5.0, 4.0), (0.0, 4.0)], 2.80, 1)
        .with_number("O-01")
        .with_function(RoomFunction::Office)
        .with_finish_style("finish_floor_parquet_80");
    write_room_record(&mut scene.document, room_h, &room);

    let entity = scene.document.get_entity(room_h).unwrap();
    let section = room_prop_section(entity, Some(&lib)).expect("room prop section");
    let fields: Vec<&str> = section.props.iter().map(|p| p.field).collect();
    assert!(fields.contains(&"room_name"));
    assert!(fields.contains(&"room_number"));
    assert!(fields.contains(&"room_function"));
    assert!(fields.contains(&"room_floor_finish_style"));
    assert!(fields.contains(&"room_floor_finish"));
    assert!(fields.contains(&"room_ceiling_finish"));

    let style_prop = section.props.iter().find(|p| p.field == "room_floor_finish_style").unwrap();
    if let crate::scene::model::object::PropValue::Choice { selected, options } = &style_prop.value {
        assert!(selected.contains("Parkett") || selected == "finish_floor_parquet_80");
        assert!(options.len() >= 5);
    } else {
        panic!("Expected choice prop");
    }
}

#[test]
fn test_entity_property_sections_title_for_aec_slab_and_openings() {
    use crate::modules::aec::engine::openings::OpeningKind;
    use crate::modules::aec::engine::opening_xdata::place_wall_opening;
    use crate::modules::aec::engine::slab::Slab;
    use crate::modules::aec::engine::slab_xdata::write_slab_record;
    use crate::modules::aec::engine::wall::Wall;
    use crate::modules::aec::engine::wall_regen::regenerate_wall_representation;
    use crate::modules::aec::engine::xdata::{AEC_APPID, wall_record_for_wall};
    use crate::scene::Scene;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::xdata::ExtendedDataRecord;

    let mut scene = Scene::new();

    // 1. Slab carrier
    let mut pl = LwPolyline::new();
    pl.is_closed = true;
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(4.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(4.0, 4.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 4.0)));
    let slab_h = scene.add_entity(EntityType::LwPolyline(pl));
    let slab = Slab::new("style_slab_concrete_20", 0);
    write_slab_record(&mut scene.document, slab_h, &slab);

    let slab_ent = scene.document.get_entity(slab_h).unwrap();
    let slab_title = aec_entity_title(&scene, slab_h, slab_ent).expect("slab title");
    assert_eq!(slab_title, crate::tr!("aec", "slab-title"));

    // 2. Wall and Openings
    let mut wall_pl = LwPolyline::new();
    wall_pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    wall_pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut wall_ent = EntityType::LwPolyline(wall_pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    let mut wall = Wall::new("Standard", 2.80, 0);
    wall.layers = vec![crate::modules::aec::engine::wall::WallLayer {
        material: "Brick".to_string(),
        thickness: 0.24,
        ..Default::default()
    }];
    rec.values = wall_record_for_wall(&wall);
    wall_ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(wall_ent);
    regenerate_wall_representation(&mut scene, wall_h, None).expect("regen wall");

    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        glam::DVec3::new(2.5, 0.0, 0.0),
        OpeningKind::Window,
        None,
        None,
        None,
    ).expect("place window");
    let win_ent = scene.document.get_entity(win_h).unwrap();
    let win_title = aec_entity_title(&scene, win_h, win_ent).expect("win title");
    assert_eq!(win_title, crate::tr!("aec", "window-title"));

    let (door_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        glam::DVec3::new(6.0, 0.0, 0.0),
        OpeningKind::Door,
        None,
        None,
        None,
    ).expect("place door");
    let door_ent = scene.document.get_entity(door_h).unwrap();
    let door_title = aec_entity_title(&scene, door_h, door_ent).expect("door title");
    assert_eq!(door_title, crate::tr!("aec", "door-title"));
}

#[test]
fn test_slab_with_openings_selection_collapse_and_title() {
    use crate::modules::aec::engine::library;
    use crate::modules::aec::engine::slab::Slab;
    use crate::modules::aec::engine::slab_opening::{SlabOpening, SlabOpeningKind};
    use crate::modules::aec::engine::slab_package::{
        expand_handles_for_slab_packages, slab_opening_package_handles, slab_package_handles,
    };
    use crate::modules::aec::engine::slab_regen::{
        regenerate_slab_opening_representation, regenerate_slab_representation,
    };
    use crate::modules::aec::engine::slab_xdata::{
        add_slab_opening_handle, write_slab_opening_record, write_slab_record,
    };
    use crate::scene::Scene;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;

    let mut scene = Scene::new();
    let lib = library::seed_default_library();

    // 1. Create slab carrier (10x10)
    let mut pl = LwPolyline::new();
    pl.is_closed = true;
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 10.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 10.0)));
    let slab_h = scene.add_entity(EntityType::LwPolyline(pl));

    let slab = Slab::new("style_slab_concrete_20", 0);
    write_slab_record(&mut scene.document, slab_h, &slab);

    // 2. Create slab opening carrier (2x2 inside slab)
    let mut op_pl = LwPolyline::new();
    op_pl.is_closed = true;
    op_pl.add_vertex(LwVertex::new(Vector2::new(3.0, 3.0)));
    op_pl.add_vertex(LwVertex::new(Vector2::new(5.0, 3.0)));
    op_pl.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
    op_pl.add_vertex(LwVertex::new(Vector2::new(3.0, 5.0)));
    let op_h = scene.add_entity(EntityType::LwPolyline(op_pl));

    let opening = SlabOpening::new(
        slab_h,
        SlabOpeningKind::Shaft,
        vec![(3.0, 3.0), (5.0, 3.0), (5.0, 5.0), (3.0, 5.0)],
    );
    write_slab_opening_record(&mut scene.document, op_h, &opening);
    add_slab_opening_handle(&mut scene.document, slab_h, op_h);

    // 3. Regenerate representations
    regenerate_slab_representation(&mut scene, slab_h, Some(&lib), None);
    regenerate_slab_opening_representation(&mut scene, op_h, Some(&lib), None);

    // 4. Check slab package handles: must NOT include the opening carrier or opening children
    let slab_pkg = slab_package_handles(&scene, slab_h);
    assert!(slab_pkg.contains(&slab_h));
    assert!(!slab_pkg.contains(&op_h), "Slab package must not contain opening carrier");

    let op_pkg = slab_opening_package_handles(&scene, op_h);
    assert!(op_pkg.contains(&op_h));
    assert!(!op_pkg.contains(&slab_h), "Opening package must not contain slab carrier");

    for op_child in &op_pkg {
        assert!(!slab_pkg.contains(op_child), "Slab package must not contain opening display children");
    }

    // 5. Check selection expansion for slab
    let expanded_slab = expand_handles_for_slab_packages(&scene, &[slab_h]);
    assert!(expanded_slab.contains(&slab_h));
    assert!(!expanded_slab.contains(&op_h));

    // 6. When slab is selected (with all its derived geometry), collapse must yield exactly 1 entity
    let selected_slab_entities: Vec<(Handle, &EntityType)> = expanded_slab
        .iter()
        .map(|h| (*h, scene.document.get_entity(*h).unwrap()))
        .collect();
    let collapsed_slab = collapse_selection_to_aec_package(&scene, selected_slab_entities);
    assert_eq!(collapsed_slab.len(), 1, "Slab with openings must collapse to exactly 1 entity");
    assert_eq!(collapsed_slab[0].0, slab_h);

    let slab_title = aec_entity_title(&scene, collapsed_slab[0].0, collapsed_slab[0].1).expect("slab title");
    assert_eq!(slab_title, crate::tr!("aec", "slab-title"));

    // 7. When opening is selected, collapse must yield exactly 1 entity
    let expanded_op = expand_handles_for_slab_packages(&scene, &[op_h]);
    let selected_op_entities: Vec<(Handle, &EntityType)> = expanded_op
        .iter()
        .map(|h| (*h, scene.document.get_entity(*h).unwrap()))
        .collect();
    let collapsed_op = collapse_selection_to_aec_package(&scene, selected_op_entities);
    assert_eq!(collapsed_op.len(), 1, "Opening must collapse to exactly 1 entity");
    assert_eq!(collapsed_op[0].0, op_h);

    let op_title = aec_entity_title(&scene, collapsed_op[0].0, collapsed_op[0].1).expect("opening title");
    assert_eq!(op_title, crate::tr!("aec", "slabopening-title"));
}

fn prop_text(section: &crate::scene::model::object::PropSection, field: &str) -> String {
    match &section.props.iter().find(|p| p.field == field).expect(field).value {
        crate::scene::model::object::PropValue::ReadOnly(s)
        | crate::scene::model::object::PropValue::PlainText(s)
        | crate::scene::model::object::PropValue::EditText(s) => s.clone(),
        crate::scene::model::object::PropValue::EntityRef { handle, .. } => handle.to_string(),
        other => panic!("{field} unexpected {other:?}"),
    }
}

fn prop_edit_number(section: &crate::scene::model::object::PropSection, field: &str) -> f64 {
    match &section.props.iter().find(|p| p.field == field).expect(field).value {
        crate::scene::model::object::PropValue::EditText(s) => {
            crate::entities::common::parse_f64(s).unwrap_or_else(|| s.parse().expect(field))
        }
        other => panic!("{field} unexpected {other:?}"),
    }
}

fn sample_section_viewpoint(
    vp_id: crate::modules::aec::engine::views::ViewpointId,
    name: &str,
    path: crate::modules::aec::engine::views::viewpoint::SectionPath,
    far_depth: Option<f64>,
    z_min: f64,
    z_max: f64,
    flip: bool,
) -> crate::modules::aec::engine::views::viewpoint::Viewpoint {
    use crate::modules::aec::engine::views::viewpoint::{
        ProjectionMapping, VerticalRange, ViewAxes, ViewKind, ViewOrigin, Viewpoint,
    };
    use crate::modules::aec::engine::views::{Metres, Point3, PositiveMetres, Revision, Vector2};
    Viewpoint {
        id: vp_id,
        name: name.into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(0.0, 0.0, z_min)),
        axes: ViewAxes::section_with_side(Vector2::new(1.0, 0.0), flip).unwrap(),
        kind: ViewKind::Section {
            path,
            far_depth: far_depth.and_then(|d| PositiveMetres::new(d).ok()),
        },
        z_range: VerticalRange::new(Metres::new(z_min).unwrap(), Metres::new(z_max).unwrap())
            .unwrap(),
        mapping: ProjectionMapping::default(),
        crop: None,
    }
}

fn sample_model_view(
    mv_id: crate::modules::aec::engine::views::ModelViewId,
    vp_id: crate::modules::aec::engine::views::ViewpointId,
    name: &str,
    policy: crate::modules::aec::engine::views::model_view::UpdatePolicy,
) -> crate::modules::aec::engine::views::model_view::ModelView {
    use crate::modules::aec::engine::views::model_view::{DetailPolicy, ModelView, ViewRevisions};
    use crate::modules::aec::engine::views::{ScaleDenominator, ViewProfileId};
    ModelView {
        id: mv_id,
        name: name.into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: std::collections::BTreeSet::new(),
        graphic_overrides: vec![],
        annotation_ids: vec![],
        update_policy: policy,
        revisions: ViewRevisions::default(),
    }
}

fn viewpoint_far_depth(
    vp: &crate::modules::aec::engine::views::viewpoint::Viewpoint,
) -> Option<f64> {
    use crate::modules::aec::engine::views::viewpoint::ViewKind;
    match &vp.kind {
        ViewKind::Section { far_depth, .. } => far_depth.as_ref().map(|d| d.get()),
        ViewKind::Elevation { depth } => depth.far.as_ref().map(|d| d.get()),
        _ => None,
    }
}

fn add_test_wall(doc: &mut acadrust::CadDocument) -> Handle {
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2 as CadVec2;
    use acadrust::xdata::ExtendedDataRecord;
    use crate::modules::aec::engine::plan_view::PlanPhase;
    use crate::modules::aec::engine::wall::WallJustification;
    use crate::modules::aec::engine::WallLayer;
    use crate::modules::aec::engine::xdata::{wall_record, AEC_APPID};

    let mut pl = LwPolyline::default();
    pl.vertices.push(LwVertex {
        location: CadVec2::new(5.0, -2.0),
        bulge: 0.0,
        start_width: 0.0,
        end_width: 0.0,
        vertex_id: 0,
    });
    pl.vertices.push(LwVertex {
        location: CadVec2::new(5.0, 2.0),
        bulge: 0.0,
        start_width: 0.0,
        end_width: 0.0,
        vertex_id: 1,
    });
    let handle = doc.allocate_handle();
    pl.common.handle = handle;
    let layer = WallLayer {
        material: "Concrete".into(),
        thickness: 0.24,
        function: "Structural".into(),
        axis_offset: -0.12,
        bottom_offset: 0.0,
        top_offset: 0.0,
        layer_override: None,
        hatch_override: None,
        layer_id: uuid::Uuid::new_v4(),
    };
    let rec_values = wall_record(
        "W1",
        2.8,
        0,
        &[layer],
        &[],
        WallJustification::Center,
        PlanPhase::Existing,
        None,
    );
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = rec_values;
    pl.common.extended_data.add_record(rec);
    let _ = doc.add_entity(EntityType::LwPolyline(pl));
    handle
}

#[test]
fn test_section_marker_prop_section() {
    use crate::modules::aec::engine::views::viewpoint::SectionPath;
    use crate::modules::aec::engine::views::{Point2, ViewpointId};
    use crate::modules::aec::views::din1356::{create_section_marker, Din1356SectionMarker};
    use crate::scene::Scene;

    let mut scene = Scene::new();
    let vp_id = ViewpointId::new();
    let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
    let marker = Din1356SectionMarker::new_section("A - A", path, vp_id, Some(Handle::new(77)))
        .with_depth(Some(6.0))
        .with_z_range(0.0, 3.5);
    let marker_h = create_section_marker(&mut scene.document, &marker);

    let entity = scene.document.get_entity(marker_h).expect("marker entity");
    let section = marker_prop_section(entity, &scene.document).expect("marker prop section");

    assert_eq!(section.title, crate::tr!("aec", "section-marker-title"));
    assert_eq!(
        prop_text(&section, "marker_type"),
        crate::tr!("aec", "marker-type-section")
    );
    assert_eq!(prop_text(&section, "marker_label"), "A - A");
    assert!((prop_edit_number(&section, "marker_far_depth") - 6.0).abs() < 1e-6);
    assert!((prop_edit_number(&section, "marker_z_min") - 0.0).abs() < 1e-6);
    assert!((prop_edit_number(&section, "marker_z_max") - 3.5).abs() < 1e-6);
    match &section
        .props
        .iter()
        .find(|p| p.field == "marker_flip")
        .expect("marker_flip")
        .value
    {
        crate::scene::model::object::PropValue::Choice { selected, options } => {
            assert_eq!(selected, &crate::tr!("aec", "marker-flip-normal"));
            assert!(options.contains(&crate::tr!("aec", "marker-flip-flipped")));
        }
        other => panic!("wrong prop type for marker_flip: {other:?}"),
    }
    assert_eq!(
        prop_text(&section, "marker_carrier"),
        Handle::new(77).to_string()
    );
}

#[test]
fn test_elevation_marker_prop_section() {
    use crate::modules::aec::engine::views::{Point2, ViewpointId};
    use crate::modules::aec::views::din1356::{create_section_marker, Din1356SectionMarker};
    use crate::scene::Scene;

    let mut scene = Scene::new();
    let vp_id = ViewpointId::new();
    let marker = Din1356SectionMarker::new_elevation(
        "Südansicht",
        Point2::new(0.0, 0.0),
        Point2::new(12.0, 0.0),
        vp_id,
        None,
    )
    .with_depth(Some(15.0))
    .with_flip(true);

    let marker_h = create_section_marker(&mut scene.document, &marker);
    let entity = scene.document.get_entity(marker_h).expect("elevation entity");
    let section = marker_prop_section(entity, &scene.document).expect("marker prop section");

    assert_eq!(section.title, crate::tr!("aec", "elevation-marker-title"));
    assert_eq!(
        prop_text(&section, "marker_type"),
        crate::tr!("aec", "marker-type-elevation")
    );
    match &section
        .props
        .iter()
        .find(|p| p.field == "marker_flip")
        .expect("marker_flip")
        .value
    {
        crate::scene::model::object::PropValue::Choice { selected, .. } => {
            assert_eq!(selected, &crate::tr!("aec", "marker-flip-flipped"));
        }
        other => panic!("wrong prop type for marker_flip: {other:?}"),
    }
}

#[test]
fn test_apply_marker_property() {
    use crate::modules::aec::engine::views::viewpoint::SectionPath;
    use crate::modules::aec::engine::views::{Point2, ViewpointId};
    use crate::modules::aec::views::din1356::{
        create_section_marker, read_section_marker, Din1356SectionMarker,
    };
    use crate::scene::Scene;

    let mut scene = Scene::new();
    let vp_id = ViewpointId::new();
    let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(8.0, 0.0));
    let marker = Din1356SectionMarker::new_section("A - A", path, vp_id, None);
    let marker_h = create_section_marker(&mut scene.document, &marker);

    assert!(apply_marker_property(
        &mut scene.document,
        marker_h,
        "marker_label",
        "B - B"
    ));
    assert!(apply_marker_property(
        &mut scene.document,
        marker_h,
        "marker_far_depth",
        "12.5"
    ));
    assert!(apply_marker_property(
        &mut scene.document,
        marker_h,
        "marker_z_min",
        "-0.8"
    ));
    assert!(apply_marker_property(
        &mut scene.document,
        marker_h,
        "marker_z_max",
        "4.2"
    ));
    assert!(apply_marker_property(
        &mut scene.document,
        marker_h,
        "marker_flip",
        CHOICE_FLIPPED
    ));

    let ent = scene.document.get_entity(marker_h).expect("marker ent");
    let updated = read_section_marker(&scene.document, ent).expect("read updated marker");

    assert_eq!(updated.label, "B - B");
    assert_eq!(updated.far_depth, Some(12.5));
    assert!((updated.z_min - (-0.8)).abs() < 1e-6);
    assert!((updated.z_max - 4.2).abs() < 1e-6);
    assert!(updated.flip_direction);
}

#[test]
fn test_carrier_prop_section_and_apply() {
    use crate::modules::aec::engine::views::carrier::{
        create_view_carrier, find_view_carrier_by_handle, read_view_carrier_definitions,
        write_view_carrier_definitions,
    };
    use crate::modules::aec::engine::views::model_view::UpdatePolicy;
    use crate::modules::aec::engine::views::viewpoint::SectionPath;
    use crate::modules::aec::engine::views::{ModelViewId, Point2, ViewpointId};
    use crate::scene::Scene;

    let mut scene = Scene::new();
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();
    let carrier = create_view_carrier(
        &mut scene.document,
        vp_id,
        mv_id,
        Point2::new(0.0, 0.0),
        1.0,
        0.0,
        UpdatePolicy::Manual,
    )
    .expect("carrier created");
    let pl_h = carrier.handle;

    let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
    let vp = sample_section_viewpoint(vp_id, "Schnitt 1", path, None, 0.0, 3.0, false);
    let mv = sample_model_view(mv_id, vp_id, "MV 1", UpdatePolicy::Manual);
    write_view_carrier_definitions(&mut scene.document, pl_h, &vp, &mv);

    let entity = scene.document.get_entity(pl_h).unwrap();
    let section = carrier_prop_section(entity, &scene.document).expect("carrier section");

    assert_eq!(section.title, crate::tr!("aec", "view-carrier-title"));
    // Displays ModelView scale denominator 1:50, not geometric multiplier (1.0)
    assert!((prop_edit_number(&section, "carrier_scale") - 50.0).abs() < 1e-6);
    assert_eq!(prop_text(&section, "carrier_name"), "Schnitt 1");

    assert!(apply_carrier_property(
        &mut scene.document,
        pl_h,
        "carrier_scale",
        "100.0"
    ));
    assert!(apply_carrier_property(
        &mut scene.document,
        pl_h,
        "carrier_update_policy",
        CHOICE_AUTOMATIC
    ));

    let updated_carrier = find_view_carrier_by_handle(&scene.document, pl_h).expect("carrier");
    // Carrier geometric multiplier remains 1.0
    assert!((updated_carrier.scale - 1.0).abs() < 1e-6);
    assert_eq!(updated_carrier.update_policy, UpdatePolicy::Automatic);

    // ModelView drawing scale denominator is updated to 100.0 (1:100)
    let (_, updated_mv) = read_view_carrier_definitions(&scene.document, pl_h).expect("defs");
    assert!((updated_mv.scale.get() - 100.0).abs() < 1e-6);
}

#[test]
fn test_floorplan_carrier_cut_height_prop_and_grips() {
    use crate::modules::aec::engine::views::carrier::{
        create_view_carrier, read_view_carrier_definitions,
        write_view_carrier_definitions,
    };
    use crate::modules::aec::engine::views::model_view::{DetailPolicy, ModelView, ViewRevisions};
    use crate::modules::aec::engine::views::viewpoint::{PlanRange, VerticalRange, ViewAxes, ViewKind, ViewOrigin, Viewpoint};
    use crate::modules::aec::engine::views::{
        Metres, ModelViewId, NonNegativeMetres, Point2, Point3, Revision, ScaleDenominator,
        ViewProfileId, ViewpointId,
    };
    use crate::modules::aec::engine::views::model_view::UpdatePolicy;
    use crate::scene::Scene;

    let mut scene = Scene::new();
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();

    let viewpoint = Viewpoint {
        id: vp_id,
        name: "EG Grundriss".into(),
        revision: Revision(1),
        origin: ViewOrigin::World(Point3::new(10.0, 20.0, 1.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.8).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(2.8).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    let model_view = ModelView {
        id: mv_id,
        name: "1:50 EG".into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: UpdatePolicy::Manual,
        revisions: ViewRevisions::default(),
    };

    let carrier = create_view_carrier(
        &mut scene.document,
        vp_id,
        mv_id,
        Point2::new(40.0, 10.0),
        1.0,
        0.0,
        UpdatePolicy::Manual,
    )
    .expect("carrier created");
    let carrier_h = carrier.handle;
    write_view_carrier_definitions(&mut scene.document, carrier_h, &viewpoint, &model_view);

    let entity = scene.document.get_entity(carrier_h).unwrap();
    let section = carrier_prop_section(entity, &scene.document).expect("carrier section");

    // Verify that cut height is displayed (0.0 + 1.0 = 1.0)
    assert!((prop_edit_number(&section, "carrier_cut_height") - 1.0).abs() < 1e-4);

    // Apply new cut height: 1.5m
    assert!(apply_carrier_property(
        &mut scene.document,
        carrier_h,
        "carrier_cut_height",
        "1.5"
    ));

    let (updated_vp, _) = read_view_carrier_definitions(&scene.document, carrier_h).expect("defs");
    match updated_vp.kind {
        ViewKind::FloorPlan { range } => {
            assert!((range.below_cut.get() - 1.5).abs() < 1e-4);
        }
        _ => panic!("expected floor plan"),
    }

    // Verify grips
    let mut grips = Vec::new();
    append_view_carrier_grips(&scene, carrier_h, &mut grips);
    assert_eq!(grips.len(), 2);
    let ins_grip = grips.iter().find(|g| g.id == CARRIER_GRIP_INSERTION).unwrap();
    assert_eq!(ins_grip.world, glam::DVec3::new(40.0, 10.0, 0.0));
    let cut_grip = grips.iter().find(|g| g.id == CARRIER_GRIP_CUT_HEIGHT).unwrap();
    // Cut height 1.5m offset in Y from insertion point (10.0 + 1.5 = 11.5)
    assert!((cut_grip.world.y - 11.5).abs() < 1e-4);
}

#[test]
fn test_append_section_marker_grips_straight_and_jogged() {
    use crate::modules::aec::engine::views::viewpoint::{SectionPath, SectionSegment};
    use crate::modules::aec::engine::views::{Point2, ViewpointId};
    use crate::modules::aec::views::din1356::{create_section_marker, Din1356SectionMarker};
    use crate::scene::model::object::GripShape;
    use crate::scene::Scene;

    let mut scene = Scene::new();
    let vp_id = ViewpointId::new();

    // 1. Straight marker
    let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
    let marker = Din1356SectionMarker::new_section("A - A", path, vp_id, None)
        .with_depth(Some(4.0));
    let straight_h = create_section_marker(&mut scene.document, &marker);

    let mut grips = Vec::new();
    append_section_marker_grips(&scene, straight_h, &mut grips);

    assert_eq!(grips.len(), 4); // Start, End, Depth, Flip
    let start_grip = grips.iter().find(|g| g.id == SECTION_MARKER_START_GRIP).unwrap();
    assert_eq!(start_grip.world, glam::DVec3::new(0.0, 0.0, 0.0));
    assert_eq!(start_grip.shape, GripShape::Square);

    let end_grip = grips.iter().find(|g| g.id == SECTION_MARKER_END_GRIP).unwrap();
    assert_eq!(end_grip.world, glam::DVec3::new(10.0, 0.0, 0.0));
    assert_eq!(end_grip.shape, GripShape::Square);

    let depth_grip = grips.iter().find(|g| g.id == SECTION_MARKER_DEPTH_GRIP).unwrap();
    assert_eq!(depth_grip.world, glam::DVec3::new(5.0, 4.0, 0.0));
    assert!(matches!(depth_grip.shape, GripShape::Rectangle { .. }));
    assert_eq!(depth_grip.axis, Some(glam::DVec3::new(0.0, 1.0, 0.0)));

    let flip_grip = grips.iter().find(|g| g.id == SECTION_MARKER_FLIP_GRIP).unwrap();
    assert_eq!(flip_grip.world, glam::DVec3::new(5.0, 1.0, 0.0));
    assert!(matches!(flip_grip.shape, GripShape::Triangle { .. }));

    // 2. Jogged marker (3 segments)
    let jogged_path = SectionPath::jogged(vec![
        SectionSegment::new(Point2::new(0.0, 0.0), Point2::new(5.0, 0.0), false),
        SectionSegment::new(Point2::new(5.0, 0.0), Point2::new(5.0, 2.0), true),
        SectionSegment::new(Point2::new(5.0, 2.0), Point2::new(10.0, 2.0), false),
    ]);
    let jogged_marker = Din1356SectionMarker::new_section("B - B", jogged_path, vp_id, None)
        .with_depth(Some(3.0));
    let jogged_h = create_section_marker(&mut scene.document, &jogged_marker);

    let mut jog_grips = Vec::new();
    append_section_marker_grips(&scene, jogged_h, &mut jog_grips);

    // Start (0), End (1), Jog0 (100), Jog1 (101), Depth (2), Flip (3) => 6 grips
    assert_eq!(jog_grips.len(), 6);
    let jog0 = jog_grips.iter().find(|g| g.id == SECTION_MARKER_JOG_BASE).unwrap();
    assert_eq!(jog0.world, glam::DVec3::new(5.0, 0.0, 0.0));
    let jog1 = jog_grips.iter().find(|g| g.id == SECTION_MARKER_JOG_BASE + 1).unwrap();
    assert_eq!(jog1.world, glam::DVec3::new(5.0, 2.0, 0.0));
}

#[test]
fn test_section_marker_package_resolution_and_expansion() {
    use crate::modules::aec::engine::owner_index;
    use crate::modules::aec::engine::views::viewpoint::SectionPath;
    use crate::modules::aec::engine::views::{Point2, ViewpointId};
    use crate::modules::aec::views::din1356::{
        create_section_marker, section_marker_owner_from_entity, Din1356SectionMarker,
    };
    use crate::scene::Scene;
    use acadrust::entities::LwPolyline;

    let mut scene = Scene::new();
    let vp_id = ViewpointId::new();
    let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
    let marker = Din1356SectionMarker::new_section("A - A", path, vp_id, None);
    let marker_h = create_section_marker(&mut scene.document, &marker);

    let children = owner_index::children_of(&scene.document, marker_h);
    assert!(!children.is_empty(), "Marker should have child CAD entities");

    let child_h = children[0];
    {
        let child_ent = scene.document.get_entity(child_h).unwrap();
        assert_eq!(section_marker_owner_from_entity(child_ent), Some(marker_h));
    }

    // 1. Package resolution is O(1) via the child owner back-reference.
    assert_eq!(resolve_section_marker_package(&scene, marker_h), Some(marker_h));
    assert_eq!(resolve_section_marker_package(&scene, child_h), Some(marker_h));
    assert_eq!(resolve_aec_package(&scene, child_h), marker_h);

    let unrelated = scene.add_entity(EntityType::LwPolyline(LwPolyline::new()));
    assert_eq!(resolve_section_marker_package(&scene, unrelated), None);

    // 2. Derived non-carrier check
    assert!(!is_section_marker_derived(&scene, marker_h));
    assert!(is_section_marker_derived(&scene, child_h));
    assert!(is_aec_derived_non_carrier(&scene, child_h));

    // 3. Selection expansion
    let expanded = expand_handles_for_aec_packages(&scene, &[child_h]);
    assert!(expanded.contains(&marker_h));
    for c in &children {
        assert!(expanded.contains(c));
    }

    // 4. Title resolution on child entity
    let child_ent = scene.document.get_entity(child_h).unwrap();
    let title = aec_entity_title(&scene, child_h, child_ent).expect("title");
    assert_eq!(title, crate::tr!("aec", "section-marker-title"));
}

#[test]
fn test_marker_carrier_sync_roundtrip_manual_and_automatic() {
    use crate::modules::aec::engine::views::carrier::{
        create_view_carrier, find_view_carrier_by_handle, read_view_carrier_definitions,
        write_view_carrier_definitions,
    };
    use crate::modules::aec::engine::views::model_view::UpdatePolicy;
    use crate::modules::aec::engine::views::viewpoint::{SectionPath, ViewAxes};
    use crate::modules::aec::engine::views::{ModelViewId, Point2, Vector2, ViewpointId};
    use crate::modules::aec::views::din1356::{
        create_section_marker, read_section_marker, Din1356SectionMarker,
    };
    use crate::scene::Scene;

    let mut scene = Scene::new();
    // Add real wall geometry to the scene document so cuts produce derived geometry.
    add_test_wall(&mut scene.document);
    let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));

    // --- Manual: marker edits mark the carrier outdated and update Viewpoint ---
    let vp_id_manual = ViewpointId::new();
    let mv_id_manual = ModelViewId::new();
    let carrier_manual = create_view_carrier(
        &mut scene.document,
        vp_id_manual,
        mv_id_manual,
        Point2::new(20.0, 0.0),
        1.0, // Placement scale multiplier stays 1.0
        0.0,
        UpdatePolicy::Manual,
    )
    .expect("manual carrier");
    let vp_manual = sample_section_viewpoint(
        vp_id_manual,
        "A - A",
        path.clone(),
        Some(5.0),
        0.0,
        3.0,
        false,
    );
    let mv_manual = sample_model_view(
        mv_id_manual,
        vp_id_manual,
        "Section 1:50",
        UpdatePolicy::Manual,
    );
    write_view_carrier_definitions(&mut scene.document, carrier_manual.handle, &vp_manual, &mv_manual);

    let marker_manual = Din1356SectionMarker::new_section(
        "A - A",
        path.clone(),
        vp_id_manual,
        Some(carrier_manual.handle),
    )
    .with_depth(Some(5.0))
    .with_z_range(0.0, 3.0);
    let marker_manual_h = create_section_marker(&mut scene.document, &marker_manual);

    assert!(apply_marker_property(
        &mut scene.document,
        marker_manual_h,
        "marker_far_depth",
        "8.0"
    ));
    assert!(apply_marker_property(
        &mut scene.document,
        marker_manual_h,
        "marker_z_min",
        "-0.5"
    ));
    assert!(apply_marker_property(
        &mut scene.document,
        marker_manual_h,
        "marker_z_max",
        "4.0"
    ));
    assert!(apply_marker_property(
        &mut scene.document,
        marker_manual_h,
        "marker_flip",
        CHOICE_FLIPPED
    ));
    assert!(apply_marker_property(
        &mut scene.document,
        marker_manual_h,
        "marker_label",
        "A1 - A1"
    ));

    let (vp_after, mv_after) =
        read_view_carrier_definitions(&scene.document, carrier_manual.handle).expect("defs");
    let carrier_after =
        find_view_carrier_by_handle(&scene.document, carrier_manual.handle).expect("carrier");
    assert!(carrier_after.is_outdated);
    assert_eq!(carrier_after.scale, 1.0);
    assert_eq!(vp_after.name, "A1 - A1");
    assert_eq!(mv_after.name, "Section A1 - A1");
    assert_eq!(viewpoint_far_depth(&vp_after), Some(8.0));
    assert!((vp_after.z_range.z_min.get() - (-0.5)).abs() < 1e-9);
    assert!((vp_after.z_range.z_max.get() - 4.0).abs() < 1e-9);
    assert_eq!(
        vp_after.axes,
        ViewAxes::section_with_side(Vector2::new(1.0, 0.0), true).unwrap()
    );

    // "Update now" action on outdated manual carrier must regenerate and clear outdated flag
    assert!(apply_carrier_property(
        &mut scene.document,
        carrier_manual.handle,
        "carrier_action",
        CHOICE_ACTION_UPDATE
    ));
    let carrier_manual_updated =
        find_view_carrier_by_handle(&scene.document, carrier_manual.handle).expect("carrier");
    assert!(!carrier_manual_updated.is_outdated, "Update now clears is_outdated");
    assert!(!carrier_manual_updated.derived_handles.is_empty(), "Update now rebuilds derived CAD handles");

    // Changing marker parameter again marks it outdated
    assert!(apply_marker_property(
        &mut scene.document,
        marker_manual_h,
        "marker_far_depth",
        "6.0"
    ));
    let carrier_dirty_again =
        find_view_carrier_by_handle(&scene.document, carrier_manual.handle).expect("carrier");
    assert!(carrier_dirty_again.is_outdated);

    // Switching policy from Manual to Automatic while outdated must immediately regenerate
    assert!(apply_carrier_property(
        &mut scene.document,
        carrier_manual.handle,
        "carrier_update_policy",
        CHOICE_AUTOMATIC
    ));
    let carrier_switched =
        find_view_carrier_by_handle(&scene.document, carrier_manual.handle).expect("carrier");
    assert_eq!(carrier_switched.update_policy, UpdatePolicy::Automatic);
    assert!(!carrier_switched.is_outdated, "Switching manual->auto while outdated must regenerate");
    assert!(!carrier_switched.derived_handles.is_empty());

    // --- Automatic: marker edits regenerate and keep the carrier current ---
    let vp_id_auto = ViewpointId::new();
    let mv_id_auto = ModelViewId::new();
    let carrier_auto = create_view_carrier(
        &mut scene.document,
        vp_id_auto,
        mv_id_auto,
        Point2::new(40.0, 0.0),
        1.0, // Placement scale multiplier stays 1.0
        0.0,
        UpdatePolicy::Automatic,
    )
    .expect("auto carrier");
    let vp_auto = sample_section_viewpoint(
        vp_id_auto,
        "B - B",
        path.clone(),
        Some(5.0),
        0.0,
        3.0,
        false,
    );
    let mv_auto = sample_model_view(
        mv_id_auto,
        vp_id_auto,
        "Section auto",
        UpdatePolicy::Automatic,
    );
    write_view_carrier_definitions(&mut scene.document, carrier_auto.handle, &vp_auto, &mv_auto);

    let marker_auto = Din1356SectionMarker::new_section(
        "B - B",
        path,
        vp_id_auto,
        Some(carrier_auto.handle),
    )
    .with_depth(Some(5.0))
    .with_z_range(0.0, 3.0);
    let marker_auto_h = create_section_marker(&mut scene.document, &marker_auto);

    assert!(apply_marker_property(
        &mut scene.document,
        marker_auto_h,
        "marker_far_depth",
        "9.5"
    ));

    let (vp_auto_after, _) =
        read_view_carrier_definitions(&scene.document, carrier_auto.handle).expect("auto defs");
    let carrier_auto_after =
        find_view_carrier_by_handle(&scene.document, carrier_auto.handle).expect("auto carrier");
    assert!(!carrier_auto_after.is_outdated, "Automatic carrier must be current after marker edit");
    assert!(!carrier_auto_after.derived_handles.is_empty(), "Automatic carrier must have derived geometry");
    assert_eq!(carrier_auto_after.scale, 1.0);
    assert_eq!(viewpoint_far_depth(&vp_auto_after), Some(9.5));

    // Carrier edits propagate back onto the linked marker.
    assert!(apply_carrier_property(
        &mut scene.document,
        carrier_auto.handle,
        "carrier_name",
        "Nord"
    ));
    assert!(apply_carrier_property(
        &mut scene.document,
        carrier_auto.handle,
        "carrier_far_depth",
        "11.0"
    ));
    assert!(apply_carrier_property(
        &mut scene.document,
        carrier_auto.handle,
        "carrier_z_min",
        "0.2"
    ));
    assert!(apply_carrier_property(
        &mut scene.document,
        carrier_auto.handle,
        "carrier_z_max",
        "3.8"
    ));

    let marker_ent = scene.document.get_entity(marker_auto_h).expect("marker");
    let marker_back = read_section_marker(&scene.document, marker_ent).expect("read marker");
    assert_eq!(marker_back.label, "Nord");
    assert_eq!(marker_back.far_depth, Some(11.0));
    assert!((marker_back.z_min - 0.2).abs() < 1e-9);
    assert!((marker_back.z_max - 3.8).abs() < 1e-9);

    let (vp_back, mv_back) =
        read_view_carrier_definitions(&scene.document, carrier_auto.handle).expect("back defs");
    assert_eq!(vp_back.name, "Nord");
    assert_eq!(mv_back.name, "Section Nord");
    assert_eq!(viewpoint_far_depth(&vp_back), Some(11.0));

    // Repeated "Update now" is a valid no-op that still succeeds.
    assert!(apply_carrier_property(
        &mut scene.document,
        carrier_auto.handle,
        "carrier_action",
        CHOICE_ACTION_UPDATE
    ));
    assert!(apply_carrier_property(
        &mut scene.document,
        carrier_auto.handle,
        "carrier_action",
        CHOICE_ACTION_UPDATE
    ));
}

#[test]
fn test_apply_marker_and_carrier_reject_invalid() {
    use crate::modules::aec::engine::views::carrier::{
        create_view_carrier, find_view_carrier_by_handle, read_view_carrier_definitions,
        write_view_carrier_definitions,
    };
    use crate::modules::aec::engine::views::model_view::UpdatePolicy;
    use crate::modules::aec::engine::views::viewpoint::SectionPath;
    use crate::modules::aec::engine::views::{ModelViewId, Point2, ViewpointId};
    use crate::modules::aec::views::din1356::{
        create_section_marker, read_section_marker, Din1356SectionMarker,
    };
    use crate::scene::Scene;

    let mut scene = Scene::new();
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();
    let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
    let carrier = create_view_carrier(
        &mut scene.document,
        vp_id,
        mv_id,
        Point2::new(0.0, 0.0),
        1.0, // Placement scale multiplier stays 1.0
        0.0,
        UpdatePolicy::Manual,
    )
    .expect("carrier");
    let vp = sample_section_viewpoint(vp_id, "A - A", path.clone(), Some(5.0), 0.0, 3.0, false);
    let mv = sample_model_view(mv_id, vp_id, "MV", UpdatePolicy::Manual);
    write_view_carrier_definitions(&mut scene.document, carrier.handle, &vp, &mv);

    let marker = Din1356SectionMarker::new_section(
        "A - A",
        path,
        vp_id,
        Some(carrier.handle),
    )
    .with_depth(Some(5.0))
    .with_z_range(0.0, 3.0);
    let marker_h = create_section_marker(&mut scene.document, &marker);

    assert!(!apply_marker_property(
        &mut scene.document,
        marker_h,
        "marker_label",
        "   "
    ));
    assert!(!apply_marker_property(
        &mut scene.document,
        marker_h,
        "marker_z_min",
        "10.0"
    ));
    assert!(!apply_marker_property(
        &mut scene.document,
        marker_h,
        "marker_z_max",
        "-1.0"
    ));
    assert!(!apply_carrier_property(
        &mut scene.document,
        carrier.handle,
        "carrier_z_min",
        "9.0"
    ));
    assert!(!apply_carrier_property(
        &mut scene.document,
        carrier.handle,
        "carrier_name",
        ""
    ));
    assert!(!apply_carrier_property(
        &mut scene.document,
        carrier.handle,
        "carrier_scale",
        "0"
    ));
    assert!(!apply_carrier_property(
        &mut scene.document,
        carrier.handle,
        "carrier_scale",
        "-10.0"
    ));

    let marker_ent = scene.document.get_entity(marker_h).expect("marker");
    let unchanged = read_section_marker(&scene.document, marker_ent).expect("read");
    assert_eq!(unchanged.label, "A - A");
    assert_eq!(unchanged.far_depth, Some(5.0));
    assert!((unchanged.z_min - 0.0).abs() < 1e-9);
    assert!((unchanged.z_max - 3.0).abs() < 1e-9);

    let (vp_unchanged, mv_unchanged) =
        read_view_carrier_definitions(&scene.document, carrier.handle).expect("defs");
    let carrier_unchanged =
        find_view_carrier_by_handle(&scene.document, carrier.handle).expect("carrier");
    assert_eq!(carrier_unchanged.scale, 1.0);
    assert_eq!(mv_unchanged.scale.get(), 50.0);
    assert_eq!(vp_unchanged.name, "A - A");
    assert!((vp_unchanged.z_range.z_min.get() - 0.0).abs() < 1e-9);
    assert!((vp_unchanged.z_range.z_max.get() - 3.0).abs() < 1e-9);

    // 0 means unlimited depth on both sides.
    assert!(apply_marker_property(
        &mut scene.document,
        marker_h,
        "marker_far_depth",
        "0"
    ));
    let marker_ent = scene.document.get_entity(marker_h).expect("marker");
    let unlimited = read_section_marker(&scene.document, marker_ent).expect("read");
    assert_eq!(unlimited.far_depth, None);
    let (vp_unlimited, _) =
        read_view_carrier_definitions(&scene.document, carrier.handle).expect("defs");
    assert_eq!(viewpoint_far_depth(&vp_unlimited), None);

    assert!(apply_carrier_property(
        &mut scene.document,
        carrier.handle,
        "carrier_far_depth",
        "0"
    ));
    let marker_ent = scene.document.get_entity(marker_h).expect("marker");
    let still_unlimited = read_section_marker(&scene.document, marker_ent).expect("read");
    assert_eq!(still_unlimited.far_depth, None);
    let carrier_after =
        find_view_carrier_by_handle(&scene.document, carrier.handle).expect("carrier");
    assert!(carrier_after.is_outdated);
}

#[test]
fn test_regeneration_failure_marks_carrier_outdated() {
    use crate::modules::aec::engine::views::carrier::{
        create_view_carrier, find_view_carrier_by_handle, write_view_carrier_definitions,
    };
    use crate::modules::aec::engine::views::model_view::UpdatePolicy;
    use crate::modules::aec::engine::views::update_ops::regenerate_or_mark_outdated;
    use crate::modules::aec::engine::views::viewpoint::SectionPath;
    use crate::modules::aec::engine::views::{ModelViewId, Point2, ViewpointId};
    use crate::scene::Scene;

    let mut scene = Scene::new();
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();
    let carrier = create_view_carrier(
        &mut scene.document,
        vp_id,
        mv_id,
        Point2::new(0.0, 0.0),
        1.0,
        0.0,
        UpdatePolicy::Automatic,
    )
    .expect("carrier");
    assert!(!carrier.is_outdated);

    // Use the matching viewpoint ID but invalid definitions so regeneration
    // fails validation rather than falling back to a valid embedded viewpoint.
    let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
    let vp_invalid = sample_section_viewpoint(vp_id, "", path, None, 0.0, 3.0, false);
    let mv = sample_model_view(mv_id, vp_id, "MV", UpdatePolicy::Automatic);
    write_view_carrier_definitions(&mut scene.document, carrier.handle, &vp_invalid, &mv);

    let success = regenerate_or_mark_outdated(&mut scene.document, carrier.handle, &vp_invalid, &mv);
    assert!(!success, "Regeneration must fail for an invalid viewpoint");

    let carrier_after_fail = find_view_carrier_by_handle(&scene.document, carrier.handle).expect("carrier");
    assert!(
        carrier_after_fail.is_outdated,
        "Regeneration failure must mark carrier as outdated instead of falsely claiming current"
    );
}

#[test]
fn test_derived_primitive_prop_section_entity_ref_and_all_grips() {
    use crate::modules::aec::engine::views::carrier::{
        create_view_carrier, sync_carrier_primitives,
    };
    use crate::modules::aec::engine::views::model_view::UpdatePolicy;
    use crate::modules::aec::engine::views::provenance::{
        DepthInterval, DerivedPrimitive, ElementRef, FeatureKey, Geometry2d, PrimitiveRole,
        ResolvedStyle, SourceReference, SourceSet,
    };
    use crate::modules::aec::engine::views::{ModelId, ModelViewId, PaperMm, Point2, ViewpointId};
    use crate::scene::Scene;

    let mut scene = Scene::new();
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();
    let mut carrier = create_view_carrier(
        &mut scene.document,
        vp_id,
        mv_id,
        Point2::new(0.0, 0.0),
        1.0,
        0.0,
        UpdatePolicy::Automatic,
    )
    .expect("carrier");

    let source_handle = Handle::new(42);
    let elem_id = crate::modules::aec::engine::views::scene_collector::handle_to_element_id(source_handle);
    let elem_ref = ElementRef::new(ModelId::new(), elem_id);

    let test_cases = vec![
        (FeatureKey::WallBase, glam::DVec3::new(0.0, 1.0, 0.0)),
        (FeatureKey::SlabBottom, glam::DVec3::new(0.0, 1.0, 0.0)),
        (FeatureKey::OpeningLeftJamb, glam::DVec3::new(1.0, 0.0, 0.0)),
        (FeatureKey::OpeningRightJamb, glam::DVec3::new(1.0, 0.0, 0.0)),
    ];

    for (feat, expected_axis) in test_cases {
        let src = SourceReference {
            element: elem_ref,
            xref_source: None,
            component_id: None,
            feature: Some(feat.clone()),
            display_component_id: None,
        };

        let prim = DerivedPrimitive {
            id: crate::modules::aec::engine::views::provenance::PrimitiveId::new(mv_id, 1, 1),
            role: PrimitiveRole::CutBoundary,
            geometry: Geometry2d::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(4.0, 0.0),
            },
            sources: SourceSet::new(vec![src]).unwrap(),
            depth: DepthInterval::new(0.0, 0.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.35).unwrap()),
            edit_handles: vec![],
        };

        sync_carrier_primitives(&mut scene.document, &mut carrier, &[prim]).expect("sync");
        assert_eq!(carrier.derived_handles.len(), 1);
        let child_handle = carrier.derived_handles[0];
        let child_ent = scene.document.get_entity(child_handle).expect("child");

        // 1. Verify Properties Section with EntityRef
        let section = derived_primitive_prop_section(child_ent).expect("prop section");
        assert_eq!(section.title, "AEC Assoziiertes Bauteil");
        let source_prop = section.props.iter().find(|p| p.field == "prim_source_entity").expect("source entity prop");
        assert_eq!(source_prop.label, "Quell-Bauteil");
        match &source_prop.value {
            crate::scene::model::object::PropValue::EntityRef { display, handle } => {
                assert_eq!(*handle, source_handle);
                assert_eq!(display, &format!("Bauteil #{}", source_handle));
            }
            _ => panic!("Expected PropValue::EntityRef"),
        }

        let carrier_prop = section.props.iter().find(|p| p.field == "prim_carrier_entity").expect("carrier entity prop");
        match &carrier_prop.value {
            crate::scene::model::object::PropValue::EntityRef { handle, .. } => {
                assert_eq!(*handle, carrier.handle);
            }
            _ => panic!("Expected PropValue::EntityRef"),
        }

        // 2. Verify Viewport Grips & Constraints
        let mut grips = Vec::new();
        append_derived_view_grips(&scene, child_handle, &mut grips);
        let expected_len = if feat == FeatureKey::WallBase { 1 } else { 2 };
        assert_eq!(grips.len(), expected_len, "Expected {} grips for {:?}", expected_len, feat);
        let grip = &grips[0];
        assert_eq!(grip.id, DERIVED_VIEW_GRIP_BASE);
        assert_eq!(grip.world, glam::DVec3::new(2.0, 0.0, 0.0));
        assert_eq!(grip.axis, Some(expected_axis), "Axis mismatch for {:?}", feat);
        if feat != FeatureKey::WallBase {
            assert_eq!(grips[1].shape, crate::scene::model::object::GripShape::Dropdown);
        }
    }
}

#[test]
fn test_carrier_display_config_and_marker_navigation() {
    use crate::modules::aec::engine::views::carrier::{
        create_view_carrier, read_view_carrier_definitions, write_view_carrier_definitions,
    };
    use crate::modules::aec::engine::views::model_view::UpdatePolicy;
    use crate::modules::aec::engine::views::viewpoint::SectionPath;
    use crate::modules::aec::engine::views::{ModelViewId, Point2, ViewpointId};
    use crate::modules::aec::views::din1356::{create_section_marker, Din1356SectionMarker};
    use crate::scene::Scene;

    let mut scene = Scene::new();
    add_test_wall(&mut scene.document);

    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();
    let carrier = create_view_carrier(
        &mut scene.document,
        vp_id,
        mv_id,
        Point2::new(10.0, 0.0),
        1.0,
        0.0,
        UpdatePolicy::Automatic,
    )
    .expect("carrier");

    let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
    let vp = sample_section_viewpoint(vp_id, "Schnitt 1", path.clone(), Some(5.0), 0.0, 3.0, false);
    let mv = sample_model_view(mv_id, vp_id, "Section 1", UpdatePolicy::Automatic);
    let original_scale = mv.scale;
    write_view_carrier_definitions(&mut scene.document, carrier.handle, &vp, &mv);

    let marker = Din1356SectionMarker::new_section("Schnitt 1", path, vp_id, Some(carrier.handle));
    let marker_h = create_section_marker(&mut scene.document, &marker);

    crate::modules::aec::engine::owner_index::link_peers(&mut scene.document, marker_h, carrier.handle);

    // 1. Verify carrier property section has display config choice & marker entity ref
    let carrier_ent = scene.document.get_entity(carrier.handle).expect("carrier entity");
    let dc_lib = crate::modules::aec::engine::project::resolve_display_config_library(None);
    let carrier_section = carrier_prop_section_with_library(
        carrier_ent,
        &scene.document,
        &dc_lib,
    )
    .expect("carrier prop section");

    let dc_prop = carrier_section.props.iter().find(|p| p.field == "carrier_display_config").expect("dc prop");
    match &dc_prop.value {
        crate::scene::model::object::PropValue::Choice { selected, options } => {
            assert_eq!(selected, "—", "an unbound profile must not display the first section plan");
            assert!(options.iter().any(|option| option == "—"));
            assert!(!options.is_empty());
        }
        _ => panic!("Expected Choice for display config"),
    }

    let marker_ref_prop = carrier_section.props.iter().find(|p| p.field == "carrier_marker_entity").expect("marker entity prop");
    match &marker_ref_prop.value {
        crate::scene::model::object::PropValue::EntityRef { handle, .. } => {
            assert_eq!(*handle, marker_h);
        }
        _ => panic!("Expected EntityRef for marker on carrier"),
    }

    // 2. Verify marker property section has carrier entity ref
    let marker_ent = scene.document.get_entity(marker_h).expect("marker entity");
    let marker_section = marker_prop_section(marker_ent, &scene.document).expect("marker prop section");
    let carrier_ref_prop = marker_section.props.iter().find(|p| p.field == "marker_carrier").expect("carrier ref prop");
    match &carrier_ref_prop.value {
        crate::scene::model::object::PropValue::EntityRef { handle, .. } => {
            assert_eq!(*handle, carrier.handle);
        }
        _ => panic!("Expected EntityRef for carrier on marker"),
    }

    // 3. Test changing display config via apply_carrier_property
    let first_cfg = &dc_lib.configs[0];
    let style_lib = crate::modules::aec::engine::project::resolve_style_library(None);
    assert!(apply_carrier_property_with_libraries(
        &mut scene.document,
        carrier.handle,
        "carrier_display_config",
        &first_cfg.name,
        &dc_lib,
        &style_lib,
        None,
    ));

    let (_, mv_after) = read_view_carrier_definitions(&scene.document, carrier.handle).expect("defs after");
    assert_eq!(mv_after.profile_id.as_uuid(), first_cfg.id);
    assert_eq!(mv_after.scale, original_scale);
}

#[test]
fn test_opening_jamb_grip_axis_aligns_with_floorplan_wall() {
    use crate::modules::aec::engine::openings::Opening;
    use crate::modules::aec::engine::opening_xdata::write_opening_instance;
    use crate::modules::aec::engine::views::carrier::{
        create_view_carrier, sync_carrier_primitives, write_view_carrier_definitions,
    };
    use crate::modules::aec::engine::views::model_view::UpdatePolicy;
    use crate::modules::aec::engine::views::provenance::{
        DepthInterval, DerivedPrimitive, ElementRef, FeatureKey, Geometry2d, PrimitiveRole,
        ResolvedStyle, SourceReference, SourceSet,
    };
    use crate::modules::aec::engine::views::viewpoint::{PlanRange, ViewAxes, ViewKind, ViewOrigin};
    use crate::modules::aec::engine::views::{
        Metres, ModelId, ModelViewId, NonNegativeMetres, PaperMm, Point2, Point3, Revision,
        ScaleDenominator, VerticalRange, ViewProfileId, ViewRevisions, ViewpointId,
    };
    use crate::scene::Scene;

    let mut scene = Scene::new();

    // 1. Create a vertical wall along Y using add_test_wall
    let wall_h = add_test_wall(&mut scene.document);

    // 2. Create an opening on this wall
    let op_h = scene.document.allocate_handle();
    let mut op_ent = codec::EntityType::Point(codec::Point::default());
    op_ent.common_mut().handle = op_h;
    scene.document.add_entity(op_ent).expect("add op entity");
    let opening = Opening::door(op_h, wall_h, 1.0);
    write_opening_instance(&mut scene, &opening);

    // 3. Create a FloorPlan carrier
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();
    let mut carrier = create_view_carrier(
        &mut scene.document,
        vp_id,
        mv_id,
        Point2::new(0.0, 0.0),
        1.0,
        0.0,
        UpdatePolicy::Automatic,
    )
    .expect("carrier");

    let vp = crate::modules::aec::engine::views::viewpoint::Viewpoint {
        id: vp_id,
        name: "Floor Plan".into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(0.0, 0.0, 1.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.0).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::new(0.0).unwrap(),
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };
    let mv = crate::modules::aec::engine::views::model_view::ModelView {
        id: mv_id,
        name: "Floor Plan MV".into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: crate::modules::aec::engine::views::model_view::DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: UpdatePolicy::Automatic,
        revisions: ViewRevisions::default(),
    };
    write_view_carrier_definitions(&mut scene.document, carrier.handle, &vp, &mv);

    let elem_id = crate::modules::aec::engine::views::scene_collector::handle_to_element_id(op_h);
    let elem_ref = ElementRef::new(ModelId::new(), elem_id);

    let src = SourceReference {
        element: elem_ref,
        xref_source: None,
        component_id: None,
        feature: Some(FeatureKey::OpeningLeftJamb),
        display_component_id: None,
    };

    let prim = DerivedPrimitive {
        id: crate::modules::aec::engine::views::provenance::PrimitiveId::new(mv_id, 1, 1),
        role: PrimitiveRole::CutBoundary,
        geometry: Geometry2d::Line {
            start: Point2::new(0.0, 3.0),
            end: Point2::new(0.24, 3.0),
        },
        sources: SourceSet::new(vec![src]).unwrap(),
        depth: DepthInterval::new(0.0, 0.0).unwrap(),
        style: ResolvedStyle::solid_stroke(PaperMm::new(0.35).unwrap()),
        edit_handles: vec![],
    };

    sync_carrier_primitives(&mut scene.document, &mut carrier, &[prim]).expect("sync");
    let child_handle = carrier.derived_handles[0];

    // 4. Verify grip axis is dynamically aligned with host wall axis along Y: (0, 1, 0)
    let mut grips = Vec::new();
    append_derived_view_grips(&scene, child_handle, &mut grips);
    assert_eq!(grips.len(), 2);
    let axis = grips[0].axis.expect("axis must be present");
    assert!((axis.x - 0.0).abs() < 1e-4, "axis.x was {}", axis.x);
    assert!((axis.y - 1.0).abs() < 1e-4, "axis.y was {}", axis.y);
    assert_eq!(grips[1].shape, crate::scene::model::object::GripShape::Dropdown);
}

#[test]
fn test_model_elements_dropdown_grips() {
    use crate::modules::aec::engine::openings::Opening;
    use crate::modules::aec::engine::opening_xdata::write_opening_instance;
    use crate::modules::aec::engine::slab::Slab;
    use crate::modules::aec::engine::slab_xdata::write_slab_record;
    use crate::scene::Scene;

    let mut scene = Scene::new();

    // 1. Wall Dropdown Grip (removed from model walls to preserve standard context menu)
    let wall_h = add_test_wall(&mut scene.document);
    let mut wall_grips = Vec::new();
    let wall_ent = scene.document.get_entity(wall_h).unwrap().clone();
    append_wall_junction_grips(&scene, wall_h, &wall_ent, &mut wall_grips);
    assert!(
        !wall_grips.iter().any(|g| g.id == WALL_BASE_DROPDOWN_GRIP_ID),
        "Wall must not have a WallBase dropdown grip in model view"
    );

    // 2. Opening Dropdown Grip
    let op_h = scene.document.allocate_handle();
    let mut op_ent = codec::EntityType::Point(codec::Point::default());
    op_ent.common_mut().handle = op_h;
    scene.document.add_entity(op_ent).expect("add op entity");
    let opening = Opening::door(op_h, wall_h, 1.0);
    write_opening_instance(&mut scene, &opening);

    let mut op_grips = Vec::new();
    append_opening_axis_grips(&scene, op_h, &mut op_grips);
    assert!(
        op_grips.iter().any(|g| g.id == OPENING_DROPDOWN_GRIP_ID && g.shape == crate::scene::model::object::GripShape::Dropdown),
        "Opening must have an Opening dropdown grip"
    );

    // 3. Slab Dropdown Grip
    let slab_h = scene.document.allocate_handle();
    let mut slab_pl = acadrust::entities::LwPolyline::default();
    slab_pl.common.handle = slab_h;
    slab_pl.vertices.push(acadrust::entities::LwVertex {
        location: acadrust::types::Vector2::new(0.0, 0.0),
        bulge: 0.0,
        start_width: 0.0,
        end_width: 0.0,
        vertex_id: 0,
    });
    slab_pl.vertices.push(acadrust::entities::LwVertex {
        location: acadrust::types::Vector2::new(5.0, 0.0),
        bulge: 0.0,
        start_width: 0.0,
        end_width: 0.0,
        vertex_id: 1,
    });
    slab_pl.vertices.push(acadrust::entities::LwVertex {
        location: acadrust::types::Vector2::new(5.0, 5.0),
        bulge: 0.0,
        start_width: 0.0,
        end_width: 0.0,
        vertex_id: 2,
    });
    let slab_ent = EntityType::LwPolyline(slab_pl);
    scene.document.add_entity(slab_ent).expect("add slab entity");
    let slab = Slab::new("default", 0);
    write_slab_record(&mut scene.document, slab_h, &slab);

    let slab_ent_updated = scene.document.get_entity(slab_h).unwrap().clone();
    let mut slab_grips = Vec::new();
    append_slab_base_grips(&scene, slab_h, &slab_ent_updated, &mut slab_grips);
    assert!(
        slab_grips.iter().any(|g| g.id == SLAB_BOTTOM_DROPDOWN_GRIP_ID && g.shape == crate::scene::model::object::GripShape::Dropdown),
        "Slab must have a SlabBottom dropdown grip"
    );
}

#[test]
fn test_eval_aec_property_expression() {
    let mut scene = crate::scene::Scene::new();
    scene.named_parameters_mut().set("_STORY_H", "3.00").unwrap();
    scene.named_parameters_mut().set("_BRH", "0.90").unwrap();

    // 1. Plain numeric literals
    assert_eq!(eval_aec_property_expression(&scene, "2.75"), Ok(2.75));
    assert_eq!(eval_aec_property_expression(&scene, " 3.20 "), Ok(3.20));

    // 2. Pure arithmetic expressions
    assert_eq!(eval_aec_property_expression(&scene, "2.80 - 0.25"), Ok(2.55));
    assert_eq!(eval_aec_property_expression(&scene, "1.0 + 0.15 * 2"), Ok(1.30));
    assert_eq!(eval_aec_property_expression(&scene, "(2.0 + 0.5) * 2"), Ok(5.0));

    // 3. Named parameters with case-insensitivity
    assert_eq!(eval_aec_property_expression(&scene, "_STORY_H - 0.25"), Ok(2.75));
    assert_eq!(eval_aec_property_expression(&scene, "_story_h - 0.25"), Ok(2.75));
    assert_eq!(eval_aec_property_expression(&scene, "_BRH + 0.125"), Ok(1.025));
    assert_eq!(eval_aec_property_expression(&scene, "_brh + 0.125"), Ok(1.025));

    // 4. Mathematical functions and constants
    assert_eq!(eval_aec_property_expression(&scene, "round(2.75)"), Ok(3.0));
    assert_eq!(eval_aec_property_expression(&scene, "sqrt(9)"), Ok(3.0));
    assert!((eval_aec_property_expression(&scene, "pi").unwrap() - std::f64::consts::PI).abs() < 1e-12);

    // 5. Invalid expressions
    assert!(eval_aec_property_expression(&scene, "").is_err());
    assert!(eval_aec_property_expression(&scene, "_UNKNOWN_VAR + 1").is_err());
    assert!(eval_aec_property_expression(&scene, "1 / 0").is_err());
}

#[test]
fn test_associative_parameter_update_recomputes_aec_geometry() {
    use crate::app::OpenCADStudio;
    use crate::modules::aec::engine::wall::Wall;
    use crate::modules::aec::engine::xdata::{wall_from_entity, wall_record_for_wall, write_aec_record, AEC_APPID};
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::xdata::ExtendedDataRecord;

    let mut app = OpenCADStudio::new_for_test();
    let tab = app.active_tab;

    // 1. Set initial parameters
    app.tabs[tab].scene.named_parameters_mut().set("_STORY_H", "3.00").unwrap();
    app.tabs[tab].scene.named_parameters_mut().set("_BRH", "0.90").unwrap();

    // 2. Create a wall with a formula for height
    let mut wall = Wall::new("style_standard_24", 2.75, 1);
    wall.formulas.set("wall_height", "_STORY_H - 0.25".to_string());

    let mut poly = LwPolyline::new();
    poly.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    poly.add_vertex(LwVertex::new(Vector2::new(6.0, 0.0)));
    let wall_handle = app.tabs[tab].scene.add_entity(EntityType::LwPolyline(poly));
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = wall_record_for_wall(&wall);
    write_aec_record(&mut app.tabs[tab].scene.document, wall_handle, rec);

    // 3. Mutate parameter _STORY_H to 3.40 and trigger recompute
    app.tabs[tab].scene.named_parameters_mut().set("_STORY_H", "3.40").unwrap();
    app.aec_recompute_all_formulas(tab);

    // 4. Verify wall height automatically updated to 3.15 (3.40 - 0.25)
    let ent = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
    let updated_wall = wall_from_entity(ent).expect("wall");
    assert!((updated_wall.height - 3.15).abs() < 1e-6);
    assert_eq!(updated_wall.formulas.get("wall_height"), Some("_STORY_H - 0.25"));
}

#[test]
fn test_property_commit_stores_formula_and_formats_label() {
    use crate::app::OpenCADStudio;
    use crate::modules::aec::engine::wall::Wall;
    use crate::modules::aec::engine::xdata::{wall_from_entity, wall_record_for_wall, write_aec_record, AEC_APPID};
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::xdata::ExtendedDataRecord;

    let mut app = OpenCADStudio::new_for_test();
    let tab = app.active_tab;

    app.tabs[tab].scene.named_parameters_mut().set("_STORY_H", "3.00").unwrap();

    // Create a wall with plain height 2.80
    let wall = Wall::new("style_standard_24", 2.80, 1);
    let mut poly = LwPolyline::new();
    poly.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    poly.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let wall_handle = app.tabs[tab].scene.add_entity(EntityType::LwPolyline(poly));
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = wall_record_for_wall(&wall);
    write_aec_record(&mut app.tabs[tab].scene.document, wall_handle, rec);

    // Select the wall
    app.tabs[tab].scene.select_entity(wall_handle, true);
    app.refresh_properties();

    // 1. Initially, no formula attached -> label is "Height"
    let ent = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
    let sec = wall_prop_section(ent, None, None).expect("wall section");
    let height_prop = sec.props.iter().find(|p| p.field == "wall_height").unwrap();
    assert_eq!(height_prop.label, "Height");

    // 2. Type formula "_STORY_H - 0.25" and commit
    let _ = app.update(crate::app::Message::PropGeomInput {
        field: "wall_height",
        value: "_STORY_H - 0.25".to_string(),
    });
    let _ = app.update(crate::app::Message::PropGeomCommit("wall_height"));

    // 3. Verify wall entity now has calculated height 2.75 and stored formula
    let ent = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
    let updated_wall = wall_from_entity(ent).expect("wall");
    assert_eq!(updated_wall.height, 2.75);
    assert_eq!(updated_wall.formulas.get("wall_height"), Some("_STORY_H - 0.25"));

    // 4. Verify property section formats label with (fx = 2.7500) and value contains formula string
    crate::entities::common::set_unit_context(
        crate::entities::common::UnitContext::from_header(
            &app.tabs[tab].scene.document.header,
        ),
    );
    let sec = wall_prop_section(ent, None, None).expect("wall section");
    let height_prop = sec.props.iter().find(|p| p.field == "wall_height").unwrap();
    assert!(height_prop.label.contains("fx = 2.75"));
    assert_eq!(height_prop.value, crate::scene::model::object::PropValue::EditText("_STORY_H - 0.25".to_string()));

    // 5. In-place edit of the formula string (e.g. adjust offset to -0.10)
    let _ = app.update(crate::app::Message::PropGeomInput {
        field: "wall_height",
        value: "_STORY_H - 0.10".to_string(),
    });
    let _ = app.update(crate::app::Message::PropGeomCommit("wall_height"));

    let ent = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
    let updated_wall = wall_from_entity(ent).expect("wall");
    assert_eq!(updated_wall.height, 2.90);
    assert_eq!(updated_wall.formulas.get("wall_height"), Some("_STORY_H - 0.10"));

    crate::entities::common::set_unit_context(
        crate::entities::common::UnitContext::from_header(
            &app.tabs[tab].scene.document.header,
        ),
    );
    let sec = wall_prop_section(ent, None, None).expect("wall section");
    let height_prop = sec.props.iter().find(|p| p.field == "wall_height").unwrap();
    assert!(height_prop.label.contains("fx = 2.9"));
    assert_eq!(height_prop.value, crate::scene::model::object::PropValue::EditText("_STORY_H - 0.10".to_string()));

    // 6. Overwrite with plain number "3.10"
    let _ = app.update(crate::app::Message::PropGeomInput {
        field: "wall_height",
        value: "3.10".to_string(),
    });
    let _ = app.update(crate::app::Message::PropGeomCommit("wall_height"));

    let ent = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
    let updated_wall = wall_from_entity(ent).expect("wall");
    assert_eq!(updated_wall.height, 3.10);
    assert_eq!(updated_wall.formulas.get("wall_height"), None);

    let sec = wall_prop_section(ent, None, None).expect("wall section");
    let height_prop = sec.props.iter().find(|p| p.field == "wall_height").unwrap();
    assert_eq!(height_prop.label, "Height");
    assert_eq!(height_prop.value, crate::scene::model::object::PropValue::EditText("3.1".to_string()));
}

#[test]
fn test_formula_recompute_wall_top_offset_with_dangling_plane() {
    use crate::app::OpenCADStudio;
    use crate::modules::aec::engine::control_plane::ControlPlaneFacet;
    use crate::modules::aec::engine::wall::Wall;
    use crate::modules::aec::engine::xdata::{wall_from_entity, wall_record_for_wall, write_aec_record, AEC_APPID};
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::xdata::ExtendedDataRecord;
    use uuid::Uuid;

    let mut app = OpenCADStudio::new_for_test();
    let tab = app.active_tab;

    app.tabs[tab].scene.named_parameters_mut().set("_ROOF_OFF", "0.20").unwrap();

    let mut wall = Wall::new("style_standard_24", 2.80, 1);
    let f1 = ControlPlaneFacet::new(
        "RoofSlope",
        vec![
            [0.0, -5.0, 4.0],
            [10.0, -5.0, 6.0],
            [10.0, 5.0, 6.0],
            [0.0, 5.0, 4.0],
        ],
    );
    wall.top_plane_id = Some(Uuid::new_v4()); // Dangling plane ID
    wall.top_facets = vec![f1];
    wall.top_origin = [0.0, -5.0, 4.0];
    wall.formulas.set("wall_top_offset", "_ROOF_OFF + 0.05".to_string());

    let mut poly = LwPolyline::new();
    poly.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    poly.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let wall_handle = app.tabs[tab].scene.add_entity(EntityType::LwPolyline(poly));
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = wall_record_for_wall(&wall);
    write_aec_record(&mut app.tabs[tab].scene.document, wall_handle, rec);

    // 1. Initial recompute
    app.aec_recompute_all_formulas(tab);

    let ent = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
    let updated_wall = wall_from_entity(ent).expect("wall");
    assert!((updated_wall.top_offset - 0.25).abs() < 1e-6);
    let normal_scale = 1.04_f64.sqrt();
    let z0 = updated_wall.top_z_at_xy(0.0, 0.0);
    let z5 = updated_wall.top_z_at_xy(5.0, 0.0);
    let z10 = updated_wall.top_z_at_xy(10.0, 0.0);
    assert!((z5 - z0 - 1.0).abs() < 1e-6);
    assert!((z10 - z0 - 2.0).abs() < 1e-6);
    assert!((z0 - (4.0 + 0.25 * normal_scale)).abs() < 1e-6);
    assert!((z5 - (5.0 + 0.25 * normal_scale)).abs() < 1e-6);
    assert!((z10 - (6.0 + 0.25 * normal_scale)).abs() < 1e-6);

    // 2. Mutate parameter _ROOF_OFF and recompute
    app.tabs[tab].scene.named_parameters_mut().set("_ROOF_OFF", "0.50").unwrap();
    app.aec_recompute_all_formulas(tab);

    let ent = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
    let updated_wall2 = wall_from_entity(ent).expect("wall");
    assert!((updated_wall2.top_offset - 0.55).abs() < 1e-6);
    let z0_2 = updated_wall2.top_z_at_xy(0.0, 0.0);
    let z5_2 = updated_wall2.top_z_at_xy(5.0, 0.0);
    let z10_2 = updated_wall2.top_z_at_xy(10.0, 0.0);
    assert!((z5_2 - z0_2 - 1.0).abs() < 1e-6);
    assert!((z10_2 - z0_2 - 2.0).abs() < 1e-6);
    assert!((z0_2 - (4.0 + 0.55 * normal_scale)).abs() < 1e-6);
    assert!((z5_2 - (5.0 + 0.55 * normal_scale)).abs() < 1e-6);
    assert!((z10_2 - (6.0 + 0.55 * normal_scale)).abs() < 1e-6);
    assert!((z0_2 - z0 - 0.30 * normal_scale).abs() < 1e-6);
    assert!((z5_2 - z5 - 0.30 * normal_scale).abs() < 1e-6);
    assert!((z10_2 - z10 - 0.30 * normal_scale).abs() < 1e-6);
}

#[test]
fn test_formula_recompute_slab_offsets_with_aliases() {
    use crate::app::OpenCADStudio;
    use crate::modules::aec::engine::slab::Slab;
    use crate::modules::aec::engine::slab_xdata::{slab_from_entity, write_slab_record};
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;

    let mut app = OpenCADStudio::new_for_test();
    let tab = app.active_tab;

    app.tabs[tab].scene.named_parameters_mut().set("_SLAB_OFF", "0.15").unwrap();
    app.tabs[tab].scene.named_parameters_mut().set("_BASE_OFF", "-0.05").unwrap();

    // 1. Slab with slab_top_offset formula
    let mut poly1 = LwPolyline::new();
    poly1.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    poly1.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let s1_handle = app.tabs[tab].scene.add_entity(EntityType::LwPolyline(poly1));
    let mut s1 = Slab::new("style_slab_concrete_20", 1);
    s1.formulas.set("slab_top_offset", "_SLAB_OFF + 0.10".to_string());
    write_slab_record(&mut app.tabs[tab].scene.document, s1_handle, &s1);

    // 2. Slab with slab_offset formula alias
    let mut poly2 = LwPolyline::new();
    poly2.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    poly2.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let s2_handle = app.tabs[tab].scene.add_entity(EntityType::LwPolyline(poly2));
    let mut s2 = Slab::new("style_slab_concrete_20", 1);
    s2.formulas.set("slab_offset", "_SLAB_OFF + 0.05".to_string());
    write_slab_record(&mut app.tabs[tab].scene.document, s2_handle, &s2);

    // 3. Slab with slab_base_offset formula
    let mut poly3 = LwPolyline::new();
    poly3.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    poly3.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let s3_handle = app.tabs[tab].scene.add_entity(EntityType::LwPolyline(poly3));
    let mut s3 = Slab::new("style_slab_concrete_20", 1);
    s3.formulas.set("slab_base_offset", "_BASE_OFF - 0.02".to_string());
    write_slab_record(&mut app.tabs[tab].scene.document, s3_handle, &s3);

    // Initial recompute
    app.aec_recompute_all_formulas(tab);

    let ent1 = app.tabs[tab].scene.document.get_entity(s1_handle).unwrap();
    let slab1 = slab_from_entity(ent1).unwrap();
    assert!((slab1.top_offset - 0.25).abs() < 1e-9);

    let ent2 = app.tabs[tab].scene.document.get_entity(s2_handle).unwrap();
    let slab2 = slab_from_entity(ent2).unwrap();
    assert!((slab2.top_offset - 0.20).abs() < 1e-9);

    let ent3 = app.tabs[tab].scene.document.get_entity(s3_handle).unwrap();
    let slab3 = slab_from_entity(ent3).unwrap();
    assert!((slab3.base_offset - (-0.07)).abs() < 1e-9);

    // Verify property section displays (fx = ...) for alias formulas as well
    let sec2 = slab_prop_section(ent2, None, None).expect("slab prop section");
    let top_off_prop = sec2.props.iter().find(|p| p.field == "slab_top_offset").unwrap();
    assert!(top_off_prop.label.contains("fx ="));
    match &top_off_prop.value {
        crate::scene::model::object::PropValue::EditText(txt) => {
            assert_eq!(txt, "_SLAB_OFF + 0.05");
        }
        _ => panic!("expected EditText"),
    }

    // Mutate parameters and recompute
    app.tabs[tab].scene.named_parameters_mut().set("_SLAB_OFF", "0.30").unwrap();
    app.tabs[tab].scene.named_parameters_mut().set("_BASE_OFF", "0.10").unwrap();
    app.aec_recompute_all_formulas(tab);

    let ent1 = app.tabs[tab].scene.document.get_entity(s1_handle).unwrap();
    let slab1 = slab_from_entity(ent1).unwrap();
    assert!((slab1.top_offset - 0.40).abs() < 1e-9);

    let ent2 = app.tabs[tab].scene.document.get_entity(s2_handle).unwrap();
    let slab2 = slab_from_entity(ent2).unwrap();
    assert!((slab2.top_offset - 0.35).abs() < 1e-9);

    let ent3 = app.tabs[tab].scene.document.get_entity(s3_handle).unwrap();
    let slab3 = slab_from_entity(ent3).unwrap();
    assert!((slab3.base_offset - 0.08).abs() < 1e-9);

    // Verify property section structure has slab_top_plane & slab_top_offset right after justification
    let sec = slab_prop_section(ent1, None, None).expect("slab prop section");
    let fields: Vec<&str> = sec.props.iter().map(|p| p.field).collect();
    let just_idx = fields.iter().position(|&f| f == "slab_justification").unwrap();
    let top_plane_idx = fields.iter().position(|&f| f == "slab_top_plane").unwrap();
    let top_offset_idx = fields.iter().position(|&f| f == "slab_top_offset").unwrap();
    assert_eq!(top_plane_idx, just_idx + 1);
    assert_eq!(top_offset_idx, just_idx + 2);

    // Apply a plain numeric value to s2 (which had alias formula `slab_offset`):
    // All aliases must be cleared, and recompute must not resurrect the formula.
    let ok = app.aec_apply_property_field(tab, s2_handle, "slab_top_offset", "0.80");
    assert!(ok);
    let ent2_after_num = app.tabs[tab].scene.document.get_entity(s2_handle).unwrap();
    let s2_after_num = slab_from_entity(ent2_after_num).unwrap();
    assert!((s2_after_num.top_offset - 0.80).abs() < 1e-9);
    assert!(s2_after_num.formulas.is_empty());

    // Recompute formulas should NOT overwrite the plain numeric value 0.80
    app.aec_recompute_all_formulas(tab);
    let ent2_after_recompute = app.tabs[tab].scene.document.get_entity(s2_handle).unwrap();
    let s2_after_recompute = slab_from_entity(ent2_after_recompute).unwrap();
    assert!((s2_after_recompute.top_offset - 0.80).abs() < 1e-9);
    assert!(s2_after_recompute.formulas.is_empty());
}

#[test]
fn test_aec_apply_property_field_for_slab_offsets_and_justifications() {
    use crate::app::OpenCADStudio;
    use crate::modules::aec::engine::slab::{Slab, SlabJustification};
    use crate::modules::aec::engine::slab_xdata::{slab_from_entity, write_slab_record};
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;

    let mut app = OpenCADStudio::new_for_test();
    let tab = app.active_tab;

    app.tabs[tab].scene.named_parameters_mut().set("_H", "2.80").unwrap();

    let mut poly = LwPolyline::new();
    poly.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    poly.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    poly.add_vertex(LwVertex::new(Vector2::new(5.0, 4.0)));
    poly.add_vertex(LwVertex::new(Vector2::new(0.0, 4.0)));
    poly.is_closed = true;
    let handle = app.tabs[tab].scene.add_entity(EntityType::LwPolyline(poly));
    let slab = Slab::new("style_slab_concrete_20", 1);
    write_slab_record(&mut app.tabs[tab].scene.document, handle, &slab);

    // 1. Change justification via panel apply
    let ok = app.aec_apply_property_field(tab, handle, "slab_justification", "OKRD (Oberkante Rohdecke)");
    assert!(ok);
    let ent = app.tabs[tab].scene.document.get_entity(handle).unwrap();
    let s = slab_from_entity(ent).unwrap();
    assert_eq!(s.justification, SlabJustification::StructuralTop);

    // 2. Set formula on slab_top_offset via panel apply
    let ok = app.aec_apply_property_field(tab, handle, "slab_top_offset", "_H * 0.10");
    assert!(ok);
    let ent = app.tabs[tab].scene.document.get_entity(handle).unwrap();
    let s = slab_from_entity(ent).unwrap();
    assert!((s.top_offset - 0.28).abs() < 1e-9);
    assert_eq!(s.formulas.get("slab_top_offset"), Some("_H * 0.10"));

    // 3. Mutate parameter and recompute
    app.tabs[tab].scene.named_parameters_mut().set("_H", "3.00").unwrap();
    app.aec_recompute_all_formulas(tab);
    let ent = app.tabs[tab].scene.document.get_entity(handle).unwrap();
    let s = slab_from_entity(ent).unwrap();
    assert!((s.top_offset - 0.30).abs() < 1e-9);

    // 4. Overwrite formula with plain number
    let ok = app.aec_apply_property_field(tab, handle, "slab_top_offset", "0.45");
    assert!(ok);
    let ent = app.tabs[tab].scene.document.get_entity(handle).unwrap();
    let s = slab_from_entity(ent).unwrap();
    assert!((s.top_offset - 0.45).abs() < 1e-9);
    assert_eq!(s.formulas.get("slab_top_offset"), None);
    assert!(s.formulas.is_empty());
}

#[test]
fn test_prop_geom_choice_changed_applies_aec_control_planes_and_justifications() {
    use crate::app::{Message, OpenCADStudio};
    use crate::modules::aec::engine::control_plane::ControlPlane;
    use crate::modules::aec::engine::project::{Building, ProjectFile, StoreyRef};
    use crate::modules::aec::engine::wall::Wall;
    use crate::modules::aec::engine::xdata::{wall_from_entity, wall_record_for_wall, AEC_APPID};
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::xdata::ExtendedDataRecord;

    let mut app = OpenCADStudio::new_for_test();
    let tab = app.active_tab;

    let mut project = ProjectFile::default();
    let mut building = Building::new("Haus");
    let mut eg = StoreyRef::new_with_height("EG", 0.0, 2.80, "eg.dwg");
    let eg_elev_id = eg.floor_plane_id;
    let eg_custom = ControlPlane::horizontal("Rohdecke", 0.15);
    let eg_custom_id = eg_custom.id;
    eg.control_planes.push(eg_custom);

    let og = StoreyRef::new_with_height("OG", 2.80, 2.80, "og.dwg");
    let og_elev_id = og.floor_plane_id;

    building.storeys.push(eg);
    building.storeys.push(og);
    project.buildings.push(building);

    app.aec.aec_project_explorer_file = Some(project.clone());

    // Create a wall
    let mut poly = LwPolyline::new();
    poly.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    poly.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let wall_handle = app.tabs[tab].scene.add_entity(EntityType::LwPolyline(poly));
    let wall = Wall::new("standard_wall", 2.50, 0);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    app.tabs[tab]
        .scene
        .document
        .get_entity_mut(wall_handle)
        .unwrap()
        .common_mut()
        .extended_data
        .add_record(record);

    // Select the wall for properties panel
    app.tabs[tab].scene.select_entity(wall_handle, true);
    app.refresh_properties();

    // 1. Initial property section shows (none) for base plane
    let ent = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
    let sec = wall_prop_section(ent, None, Some(&project)).unwrap();
    let base_prop = sec.props.iter().find(|p| p.field == "wall_base_plane").unwrap();
    match &base_prop.value {
        crate::scene::model::object::PropValue::Choice { selected, .. } => {
            assert_eq!(selected, "(none)");
        }
        _ => panic!("expected Choice"),
    }

    // 2. Select "EG_ELEVATION (Z = 0.000 m)" via PropGeomChoiceChanged
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "wall_base_plane",
        value: "EG_ELEVATION (Z = 0.000 m)".into(),
    });

    // Verify wall has base_plane_id set
    let ent_after = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
    let wall_after = wall_from_entity(ent_after).unwrap();
    assert_eq!(wall_after.base_plane_id, Some(eg_elev_id));
    assert_eq!(wall_after.base_plane_name.as_deref(), Some("EG_ELEVATION"));

    // Verify property panel now reflects the chosen base plane
    let sec_after = wall_prop_section(ent_after, None, Some(&project)).unwrap();
    let base_prop_after = sec_after.props.iter().find(|p| p.field == "wall_base_plane").unwrap();
    match &base_prop_after.value {
        crate::scene::model::object::PropValue::Choice { selected, .. } => {
            assert_eq!(selected, "EG_ELEVATION (Z = 0.000 m)");
        }
        _ => panic!("expected Choice"),
    }

    // 3. Select custom plane "Rohdecke (Z = 0.150 m)"
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "wall_base_plane",
        value: "Rohdecke (Z = 0.150 m)".into(),
    });
    let ent_after2 = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
    let wall_after2 = wall_from_entity(ent_after2).unwrap();
    assert_eq!(wall_after2.base_plane_id, Some(eg_custom_id));
    assert_eq!(wall_after2.base_plane_name.as_deref(), Some("Rohdecke"));

    // 4. Select top plane "OG_ELEVATION (Z = 2.800 m)"
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "wall_top_plane",
        value: "OG_ELEVATION (Z = 2.800 m)".into(),
    });
    let ent_after3 = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
    let wall_after3 = wall_from_entity(ent_after3).unwrap();
    assert_eq!(wall_after3.top_plane_id, Some(og_elev_id));
    assert_eq!(wall_after3.top_plane_name.as_deref(), Some("OG_ELEVATION"));

    // 5. Reset base plane to "(none)"
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "wall_base_plane",
        value: "(none)".into(),
    });
    let ent_after4 = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
    let wall_after4 = wall_from_entity(ent_after4).unwrap();
    assert_eq!(wall_after4.base_plane_id, None);
    assert_eq!(wall_after4.base_plane_name, None);

    // 6. Test Slab Plane and Justification via PropGeomChoiceChanged
    use crate::modules::aec::engine::slab::{Slab, SlabJustification};
    use crate::modules::aec::engine::slab_xdata::{slab_from_entity, slab_record_for_slab};

    let mut slab_poly = LwPolyline::new();
    slab_poly.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    slab_poly.add_vertex(LwVertex::new(Vector2::new(4.0, 0.0)));
    slab_poly.add_vertex(LwVertex::new(Vector2::new(4.0, 4.0)));
    slab_poly.add_vertex(LwVertex::new(Vector2::new(0.0, 4.0)));
    slab_poly.is_closed = true;
    let slab_handle = app.tabs[tab].scene.add_entity(EntityType::LwPolyline(slab_poly));
    let slab = Slab::new("style_slab_concrete_20", 1);
    let mut srec = ExtendedDataRecord::new(AEC_APPID);
    srec.values = slab_record_for_slab(&slab);
    app.tabs[tab]
        .scene
        .document
        .get_entity_mut(slab_handle)
        .unwrap()
        .common_mut()
        .extended_data
        .add_record(srec);

    app.tabs[tab].scene.deselect_all();
    app.tabs[tab].scene.select_entity(slab_handle, true);
    app.refresh_properties();

    // Slab initial property section exposes only slab_top_plane & slab_top_offset
    let s_ent = app.tabs[tab].scene.document.get_entity(slab_handle).unwrap();
    let s_sec = slab_prop_section(s_ent, None, Some(&project)).unwrap();
    let s_fields: Vec<&str> = s_sec.props.iter().map(|p| p.field).collect();
    assert!(s_fields.contains(&"slab_top_plane"));
    assert!(s_fields.contains(&"slab_top_offset"));
    assert!(!s_fields.contains(&"slab_base_plane"));
    assert!(!s_fields.contains(&"slab_base_offset"));

    // Choose "Rohdecke (Z = 0.150 m)" for slab_top_plane
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "slab_top_plane",
        value: "Rohdecke (Z = 0.150 m)".into(),
    });
    let s_ent_after = app.tabs[tab].scene.document.get_entity(slab_handle).unwrap();
    let slab_after = slab_from_entity(s_ent_after).unwrap();
    assert_eq!(slab_after.top_plane_id, Some(eg_custom_id));
    assert_eq!(slab_after.top_plane_name.as_deref(), Some("Rohdecke"));

    // Change justification to OKRD
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "slab_justification",
        value: "OKRD (Oberkante Rohdecke)".into(),
    });
    let s_ent_after2 = app.tabs[tab].scene.document.get_entity(slab_handle).unwrap();
    let slab_after2 = slab_from_entity(s_ent_after2).unwrap();
    assert_eq!(slab_after2.justification, SlabJustification::StructuralTop);
}
