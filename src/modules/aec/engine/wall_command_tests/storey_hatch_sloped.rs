use super::*;

#[test]
fn test_storey_height_change_moves_walls_and_keeps_opening_sill() {
    let mut scene = Scene::new();
    let initial_elevation = 0.0;
    let initial_height = 2.80;

    let mut storey = StoreyRef::new_with_height("EG", initial_elevation, initial_height, "eg.dwg");
    let floor_plane_id = storey.floor_plane_id;
    let ceiling_plane_id = storey.ceiling_plane_id;

    let mut project = ProjectFile::default();
    let mut building = Building::new("B1");
    building.storeys.push(storey.clone());
    project.buildings.push(building);

    let mut library = crate::modules::aec::engine::library::StyleLibrary::empty();
    let win_style = OpeningStyle::standard_window();
    library.upsert_opening_style(win_style);

    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    pl.elevation = initial_elevation;

    let mut wall = Wall::new("Standard", initial_height, 0);
    wall.layers = vec![wl("mat1", 0.30, "Structural")];
    wall.base_plane_id = Some(floor_plane_id);
    wall.top_plane_id = Some(ceiling_plane_id);
    wall.base_origin = [0.0, 0.0, initial_elevation];
    wall.base_normal = [0.0, 0.0, 1.0];
    wall.top_origin = [0.0, 0.0, initial_elevation + initial_height];
    wall.top_normal = [0.0, 0.0, 1.0];

    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    let _ = regenerate_wall_representation(&mut scene, wall_h, Some(&library));

    // Place window with sill height 0.90 m
    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(2.0, 0.0, 0.0),
        OpeningKind::Window,
        Some(&library),
        None,
        None,
    )
    .expect("place window");

    let wall_pkg = resolve_wall_package(&scene, wall_h);
    let _ = regenerate_wall_representation(&mut scene, wall_pkg, Some(&library));

    // Initial check: wall height is 2.80, window sill height is 0.90 relative to wall base
    let wall_init = wall_from_entity(scene.document.get_entity(wall_h).unwrap()).unwrap();
    assert!((wall_init.height - 2.80).abs() < 1e-6);
    let win_init = crate::modules::aec::engine::opening_xdata::openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("window");
    assert!((win_init.sill_height - 0.90).abs() < 1e-4);

    // Scenario: Storey height is increased from 2.80 m to 3.20 m
    let new_height = 3.20;
    storey.set_height(new_height);
    storey.set_plane_z_relative_to_floor(ceiling_plane_id, new_height);
    project.buildings[0].storeys[0] = storey.clone();

    // Apply storey Z to scene
    crate::modules::aec::project::storey_z::apply_storey_z_to_scene(
        &mut scene,
        &mut storey,
        Some(&project),
        Some(&library),
    );

    // Verification:
    // 1. Wall height grew to 3.20 m
    let wall_updated = wall_from_entity(scene.document.get_entity(wall_h).unwrap()).unwrap();
    assert!(
        (wall_updated.height - 3.20).abs() < 1e-6,
        "Wall height should be 3.20, got {}",
        wall_updated.height
    );
    assert!(
        (wall_updated.base_origin[2] - 0.0).abs() < 1e-6,
        "Wall base origin Z should stay at 0.0"
    );
    assert!(
        (wall_updated.top_origin[2] - 3.20).abs() < 1e-6,
        "Wall top origin Z should be 3.20"
    );

    // 2. Window sill height remains 0.90 m relative to wall base
    let win_updated = crate::modules::aec::engine::opening_xdata::openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("window");
    assert!(
        (win_updated.sill_height - 0.90).abs() < 1e-4,
        "Window sill height should stay at 0.90 relative to wall base, got {}",
        win_updated.sill_height
    );
}

#[test]
fn test_negative_plane_offsets_and_partial_bindings() {
    let storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
    let floor_id = storey.floor_plane_id;
    let ceil_id = storey.ceiling_plane_id;

    // Case 1: Wall with negative base offset (-0.15) and negative top offset (-0.20)
    let mut wall = Wall::new("Standard", 3.0, 0);
    wall.base_plane_id = Some(floor_id);
    wall.top_plane_id = Some(ceil_id);
    wall.base_offset = -0.15;
    wall.top_offset = -0.20;

    wall.rebake_planes(&storey, 0.0, 0.0);
    assert!(
        (wall.base_origin[2] - (-0.15)).abs() < 1e-6,
        "Base origin Z should be -0.15, got {}",
        wall.base_origin[2]
    );
    assert!(
        (wall.top_origin[2] - 2.80).abs() < 1e-6,
        "Top origin Z should be 2.80, got {}",
        wall.top_origin[2]
    );
    assert!(
        (wall.height - 2.95).abs() < 1e-6,
        "Height should be 2.95 (2.80 - (-0.15)), got {}",
        wall.height
    );

    // Case 2: Wall with ONLY top plane bound (e.g., hanging curtain wall / parapet)
    let mut top_only_wall = Wall::new("TopOnly", 2.0, 0);
    top_only_wall.base_origin = [0.0, 0.0, 1.0];
    top_only_wall.top_plane_id = Some(ceil_id);
    top_only_wall.top_offset = 0.0;
    top_only_wall.rebake_planes(&storey, 0.0, 0.0);
    assert!(
        (top_only_wall.top_origin[2] - 3.0).abs() < 1e-6,
        "Top origin Z should be 3.0"
    );
    assert!(
        (top_only_wall.height - 2.0).abs() < 1e-6,
        "Height should be 3.0 - 1.0 = 2.0, got {}",
        top_only_wall.height
    );

    // Case 3: Wall with ONLY base plane bound (fixed height wall)
    let mut base_only_wall = Wall::new("BaseOnly", 1.20, 0);
    base_only_wall.base_plane_id = Some(floor_id);
    base_only_wall.base_offset = 0.05;
    base_only_wall.rebake_planes(&storey, 0.0, 0.0);
    assert!(
        (base_only_wall.base_origin[2] - 0.05).abs() < 1e-6,
        "Base origin Z should be 0.05"
    );
    assert!(
        (base_only_wall.top_origin[2] - 1.25).abs() < 1e-6,
        "Top origin Z should follow base + height = 1.25"
    );
    assert!(
        (base_only_wall.height - 1.20).abs() < 1e-6,
        "Height should remain fixed at 1.20"
    );
}

#[test]
fn test_storey_rename_preserves_plane_bindings() {
    let mut storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
    let floor_id = storey.floor_plane_id;
    let ceil_id = storey.ceiling_plane_id;

    let mut wall = Wall::new("Standard", 3.0, 0);
    wall.base_plane_id = Some(floor_id);
    wall.top_plane_id = Some(ceil_id);
    wall.rebake_planes(&storey, 0.0, 0.0);

    assert_eq!(wall.base_plane_name.as_deref(), Some("EG_ELEVATION"));
    assert_eq!(wall.top_plane_name.as_deref(), Some("EG_OKGH"));

    // Rename storey and its control planes
    storey.name = "Erdgeschoss".to_string();
    if let Some(p) = storey.plane_mut(floor_id) {
        p.name = "Erdgeschoss_ELEVATION".to_string();
    }
    if let Some(p) = storey.plane_mut(ceil_id) {
        p.name = "Erdgeschoss_OKGH".to_string();
    }

    // Rebake with renamed storey: plane bindings by UUID stay intact and refresh names
    wall.rebake_planes(&storey, 0.0, 0.0);
    assert_eq!(wall.base_plane_id, Some(floor_id));
    assert_eq!(wall.top_plane_id, Some(ceil_id));
    assert_eq!(wall.base_plane_name.as_deref(), Some("Erdgeschoss_ELEVATION"));
    assert_eq!(wall.top_plane_name.as_deref(), Some("Erdgeschoss_OKGH"));
    assert!((wall.height - 3.0).abs() < 1e-6);
}

#[test]
fn test_multilayer_wall_layer_offsets_during_storey_height_change() {
    let mut storey = StoreyRef::new_with_height("EG", 0.0, 2.80, "eg.dwg");
    let floor_id = storey.floor_plane_id;
    let ceil_id = storey.ceiling_plane_id;

    let mut wall = Wall::new("MultiLayer", 2.80, 0);
    let mut l1 = wl("brick", 0.20, "Structural");
    l1.bottom_offset = 0.0;
    l1.top_offset = 0.0;
    let mut l2 = wl("insulation", 0.10, "Insulation");
    l2.bottom_offset = -0.05; // starts 5cm lower
    l2.top_offset = 0.10; // stops 10cm higher
    wall.layers = vec![l1, l2];
    wall.base_plane_id = Some(floor_id);
    wall.top_plane_id = Some(ceil_id);

    wall.rebake_planes(&storey, 0.0, 0.0);
    assert!((wall.height - 2.80).abs() < 1e-6);

    // Height increases to 3.50 m
    storey.set_plane_z_relative_to_floor(ceil_id, 3.50);
    wall.rebake_planes(&storey, 0.0, 0.0);
    assert!((wall.height - 3.50).abs() < 1e-6);

    // Effective heights of individual layers
    let l1_eff_height =
        (wall.height + wall.layers[0].top_offset - wall.layers[0].bottom_offset).max(0.0);
    let l2_eff_height =
        (wall.height + wall.layers[1].top_offset - wall.layers[1].bottom_offset).max(0.0);
    assert!((l1_eff_height - 3.50).abs() < 1e-6);
    assert!((l2_eff_height - (3.50 + 0.10 - (-0.05))).abs() < 1e-6);
    assert!((l2_eff_height - 3.65).abs() < 1e-6);
}

#[test]
fn test_hatch_outline_wire_respects_fill_plane_elevation() {
    let mut scene = Scene::new();
    let z_elev = 2.75;
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(4.0, 0.0)));
    pl.elevation = z_elev;

    let mut wall = Wall::new("Standard", 2.80, 0);
    wall.base_origin = [0.0, 0.0, z_elev];
    wall.layers = vec![wl("brick", 0.30, "Structural")];

    let mut library = crate::modules::aec::engine::library::StyleLibrary::empty();
    let mat = Material::new(
        "brick".to_string(),
        "Brick".to_string(),
        "ANSI31".to_string(),
        0xFFFFFF,
        "Continuous".to_string(),
    );
    library.upsert_material(mat);

    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    let touched = regenerate_wall_representation(&mut scene, wall_h, Some(&library)).expect("regen");
    let hatch_handle = touched
        .into_iter()
        .find(|h| matches!(scene.document.get_entity(*h), Some(EntityType::Hatch(_))))
        .expect("hatch generated");

    let wire = scene.hatch_outline_wire(hatch_handle).expect("outline wire");
    for pt in &wire.points {
        assert!(
            (pt[2] as f64 - z_elev).abs() < 1e-5,
            "Hatch outline wire point Z should be {}, got {}",
            z_elev,
            pt[2]
        );
    }
}

#[test]
fn test_l_join_at_nonzero_elevation() {
    let mut scene = Scene::new();
    let z_elev = 3.50;

    let mut pl1 = LwPolyline::new();
    pl1.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl1.add_vertex(LwVertex::new(Vector2::new(4.0, 0.0)));
    pl1.elevation = z_elev;
    let mut wall1 = Wall::new("Standard", 2.80, 0);
    wall1.base_origin = [0.0, 0.0, z_elev];
    wall1.layers = vec![wl("brick", 0.30, "Structural")];
    let mut ent1 = EntityType::LwPolyline(pl1);
    let mut rec1 = ExtendedDataRecord::new(AEC_APPID);
    rec1.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall1);
    ent1.common_mut().extended_data.add_record(rec1);
    let h1 = scene.add_entity(ent1);

    let mut pl2 = LwPolyline::new();
    pl2.add_vertex(LwVertex::new(Vector2::new(3.0, -2.0)));
    pl2.add_vertex(LwVertex::new(Vector2::new(3.0, 3.0)));
    pl2.elevation = z_elev;
    let mut wall2 = Wall::new("Standard", 2.80, 0);
    wall2.base_origin = [0.0, 0.0, z_elev];
    wall2.layers = vec![wl("brick", 0.30, "Structural")];
    let mut ent2 = EntityType::LwPolyline(pl2);
    let mut rec2 = ExtendedDataRecord::new(AEC_APPID);
    rec2.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall2);
    ent2.common_mut().extended_data.add_record(rec2);
    let h2 = scene.add_entity(ent2);

    regenerate_wall_representation(&mut scene, h1, None).expect("regen h1");
    regenerate_wall_representation(&mut scene, h2, None).expect("regen h2");

    let (kind, _) = join_two_walls_as_l_in_document(&mut scene, h1, h2, None, None, None)
        .expect("L-join between walls at Z={z_elev} should succeed");
    assert_eq!(kind, JoinKind::L);

    // Check that both wall polylines maintain their elevation
    let e1 = scene.document.get_entity(h1).unwrap();
    if let EntityType::LwPolyline(p) = e1 {
        assert!((p.elevation - z_elev).abs() < 1e-6, "Wall 1 elevation maintained");
    } else {
        panic!("Expected LwPolyline");
    }
    let e2 = scene.document.get_entity(h2).unwrap();
    if let EntityType::LwPolyline(p) = e2 {
        assert!((p.elevation - z_elev).abs() < 1e-6, "Wall 2 elevation maintained");
    } else {
        panic!("Expected LwPolyline");
    }
}

#[test]
fn test_opening_edit_dedup_and_display_rule_preservation() {
    let mut scene = Scene::new();
    let initial_elevation = 1.0;
    let initial_height = 2.80;

    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(6.0, 0.0)));
    pl.elevation = initial_elevation;

    let mut wall = Wall::new("Standard", initial_height, 0);
    wall.base_origin = [0.0, 0.0, initial_elevation];
    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    let mut library = crate::modules::aec::engine::library::StyleLibrary::empty();
    let win_style = OpeningStyle::standard_window();
    library.upsert_opening_style(win_style);

    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(3.0, 0.0, initial_elevation),
        OpeningKind::Window,
        Some(&library),
        None,
        None,
    )
    .expect("place window");

    let entity = scene.document.get_entity(win_h).cloned().unwrap();
    let mut opening = crate::modules::aec::engine::opening_xdata::opening_from_entity(&entity, win_h).unwrap();

    // Verify apply_opening_property detects changes and rejects no-ops
    let changed = crate::modules::aec::properties::apply_opening_property(
        &mut opening,
        "opening_width",
        "1.50",
        Some(&library),
    );
    assert!(changed, "Width change from 1.20 to 1.50 should report change");
    assert!((opening.width - 1.50).abs() < 1e-6);

    let same = crate::modules::aec::properties::apply_opening_property(
        &mut opening,
        "opening_width",
        "1.50",
        Some(&library),
    );
    assert!(!same, "Applying same width 1.50 must return false to skip regen");

    // Commit opening with rules
    let mut rules = crate::modules::aec::engine::display_component::ComponentRuleSet::default();
    rules.visibility.insert("Layers2D".to_string(), false);

    let touched = crate::modules::aec::engine::opening_display::commit_opening_instance(
        &mut scene,
        &opening,
        Some(&library),
        Some(&rules),
    );
    assert!(!touched.is_empty());

    // Verify that the opening was committed with new width
    let reloaded = crate::modules::aec::engine::opening_xdata::opening_from_entity(scene.document.get_entity(win_h).unwrap(), win_h).unwrap();
    assert!((reloaded.width - 1.50).abs() < 1e-6);
}

#[test]
fn test_wall_with_multiple_openings_regeneration_performance() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(30.0, 0.0)));
    let mut wall = Wall::new("Standard", 2.80, 0);
    wall.layers = vec![
        wl("Plaster", 0.02, "Finish"),
        wl("Insulation", 0.12, "Thermal"),
        wl("Concrete", 0.20, "Structural"),
        wl("Plaster", 0.02, "Finish"),
    ];
    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    let library = crate::modules::aec::engine::library::seed_default_library();

    // Place 10 openings (alternating windows and doors)
    for i in 0..10 {
        let x = 2.5 + i as f64 * 2.6;
        let kind = if i % 2 == 0 {
            OpeningKind::Window
        } else {
            OpeningKind::Door
        };
        place_wall_opening(
            &mut scene,
            wall_h,
            DVec3::new(x, 0.0, 0.0),
            kind,
            Some(&library),
            None,
            None,
        )
        .expect("place opening");
    }

    let start = std::time::Instant::now();
    let res = regenerate_wall_representation(&mut scene, wall_h, Some(&library));
    let elapsed = start.elapsed();
    assert!(res.is_ok(), "Wall regeneration with 10 openings must succeed");

    // Check that derived handles contain valid solids
    let wall_rec = wall_from_entity(scene.document.get_entity(wall_h).unwrap()).unwrap();
    let mut solid_count = 0;
    for h in &wall_rec.derived_handles {
        if let Some(EntityType::Solid3D(_)) = scene.document.get_entity(*h) {
            solid_count += 1;
            if let Some(body) = scene.solid_models.get(h) {
                assert!(body.validate().is_empty(), "Derived wall solid must be valid B-Rep");
            }
        }
    }
    assert!(solid_count > 0, "Must have generated 3D solids for wall");

    // Check openings 3D frames & leaves
    let openings = openings_for_host_wall(&scene, wall_h);
    assert_eq!(openings.len(), 10);
    for op in &openings {
        let children = crate::modules::aec::engine::opening_display::collect_opening_display_children(&scene, op.handle);
        for ch in children {
            if let Some(body) = scene.solid_models.get(&ch) {
                assert!(body.validate().is_empty(), "Opening 3D component must be valid B-Rep");
            }
        }
    }

    println!("Wall with 10 openings regenerated in {:?}", elapsed);
}

#[test]
fn test_sloped_wall_solid_3d_generation() {
    let mut wall = Wall::new("Standard", 2.50, 0);
    wall.base_origin = [0.0, 0.0, 0.0];
    wall.base_normal = [0.0, 0.0, 1.0];

    // Slope along X with pitch 15 degrees:
    let pitch_deg = 15.0;
    let roof_plane = crate::modules::aec::engine::control_plane::ControlPlane::from_slope(
        "Roof",
        [0.0, 0.0, 2.50],
        pitch_deg,
        0.0,
    );
    wall.top_origin = roof_plane.origin;
    wall.top_normal = roof_plane.unit_normal();
    assert!(wall.is_sloped());

    let footprint = vec![
        (0.0, -0.15),
        (10.0, -0.15),
        (10.0, 0.15),
        (0.0, 0.15),
    ];
    let bulges = vec![0.0; 4];

    let mut layer = wl("Concrete", 0.30, "Structural");
    layer.bottom_offset = 0.0;
    layer.top_offset = 0.0;

    let body = crate::modules::aec::engine::wall_regen::build_sloped_layer_solid_3d(
        &footprint,
        &bulges,
        &wall,
        Some(&layer),
        0.0,
        2.50,
    )
    .expect("sloped solid created");

    assert!(body.validate().is_empty(), "Sloped wall solid must be valid closed B-Rep");

    let z_start = wall.top_z_at_xy(0.0, 0.0);
    let z_end = wall.top_z_at_xy(10.0, 0.0);
    assert!((z_start - 2.50).abs() < 1e-6);
    let expected_z_end = 2.50 + 10.0 * 15.0_f64.to_radians().tan();
    assert!((z_end - expected_z_end).abs() < 1e-6);

    // Multilayer offsets under sloped roof
    let mut ins_layer = wl("Insulation", 0.10, "Thermal");
    ins_layer.bottom_offset = -0.10;
    ins_layer.top_offset = 0.05;

    let ins_body = crate::modules::aec::engine::wall_regen::build_sloped_layer_solid_3d(
        &footprint,
        &bulges,
        &wall,
        Some(&ins_layer),
        0.0,
        2.50,
    )
    .expect("sloped insulation solid created");

    assert!(ins_body.validate().is_empty(), "Insulation layer solid must be valid closed B-Rep");
}

#[test]
fn test_sloped_wall_scene_regeneration() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(8.0, 0.0)));

    let mut wall = Wall::new("Standard", 2.50, 0);
    let pitch_deg = 20.0;
    let roof_plane = crate::modules::aec::engine::control_plane::ControlPlane::from_slope(
        "Roof",
        [0.0, 0.0, 2.50],
        pitch_deg,
        0.0,
    );
    wall.top_origin = roof_plane.origin;
    wall.top_normal = roof_plane.unit_normal();
    wall.layers = vec![
        wl("Concrete", 0.24, "Structural"),
        wl("Insulation", 0.12, "Thermal"),
    ];

    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    let library = crate::modules::aec::engine::library::seed_default_library();
    let touched = regenerate_wall_representation(&mut scene, wall_h, Some(&library)).expect("regen sloped wall");
    assert!(!touched.is_empty());

    let mut solids_found = 0;
    for h in touched {
        if let Some(EntityType::Solid3D(_)) = scene.document.get_entity(h) {
            solids_found += 1;
            if let Some(body) = scene.solid_models.get(&h) {
                assert!(body.validate().is_empty(), "Sloped wall solid model must be valid 2-manifold B-Rep");
            }
        }
    }
    assert_eq!(solids_found, 2, "Both layers should produce valid 3D solid bodies");
}

#[test]
fn test_sloped_wall_with_opening_regeneration() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));

    let mut wall = Wall::new("Standard", 2.50, 0);
    let pitch_deg = 15.0;
    let roof_plane = crate::modules::aec::engine::control_plane::ControlPlane::from_slope(
        "Roof",
        [0.0, 0.0, 2.50],
        pitch_deg,
        0.0,
    );
    wall.top_origin = roof_plane.origin;
    wall.top_normal = roof_plane.unit_normal();
    wall.layers = vec![
        wl("Concrete", 0.30, "Structural"),
    ];

    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    let library = crate::modules::aec::engine::library::seed_default_library();

    // Place a window at x = 5.0 (width 1.20, height 1.20, sill 0.90)
    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(5.0, 0.0, 0.0),
        OpeningKind::Window,
        Some(&library),
        None,
        None,
    )
    .expect("place window in sloped wall");
    assert!(win_h.is_valid());

    let touched = regenerate_wall_representation(&mut scene, wall_h, Some(&library))
        .expect("regen sloped wall with window");
    assert!(!touched.is_empty());

    let mut solid_bodies = Vec::new();
    for h in touched {
        if let Some(EntityType::Solid3D(_)) = scene.document.get_entity(h) {
            if let Some(body) = scene.solid_models.get(&h) {
                assert!(body.validate().is_empty(), "Rest-wall body must be valid closed B-Rep");
                solid_bodies.push(body);
            }
        }
    }
    assert!(!solid_bodies.is_empty(), "Must have generated 3D rest-wall solids around opening");
}

#[test]
fn build_and_export_sloped_walls_dxf_example() {
    let mut scene = Scene::new();
    let library = crate::modules::aec::engine::library::seed_default_library();

    // 1. Mono-pitch / Pultdach wall (10m along X, slope 15°)
    let mut pl1 = LwPolyline::new();
    pl1.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl1.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));

    let mut pult_wall = Wall::new("Standard", 2.50, 0);
    let pult_plane = crate::modules::aec::engine::control_plane::ControlPlane::from_slope(
        "Pultdach_OK",
        [0.0, 0.0, 2.50],
        15.0,
        0.0,
    );
    pult_wall.top_origin = pult_plane.origin;
    pult_wall.top_normal = pult_plane.unit_normal();
    pult_wall.layers = vec![
        wl("Concrete", 0.24, "Structural"),
        wl("Insulation", 0.12, "Thermal"),
    ];

    let mut ent1 = EntityType::LwPolyline(pl1);
    let mut rec1 = ExtendedDataRecord::new(AEC_APPID);
    rec1.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&pult_wall);
    ent1.common_mut().extended_data.add_record(rec1);
    let wall1_h = scene.add_entity(ent1);

    // Place a window at x = 3.5 and a door at x = 7.5
    let _ = place_wall_opening(
        &mut scene,
        wall1_h,
        DVec3::new(3.5, 0.0, 0.0),
        OpeningKind::Window,
        Some(&library),
        None,
        None,
    );
    let _ = place_wall_opening(
        &mut scene,
        wall1_h,
        DVec3::new(7.5, 0.0, 0.0),
        OpeningKind::Door,
        Some(&library),
        None,
        None,
    );

    // 2. Gable wall (Satteldach): Left wing (0, 5) -> (5, 5), slope +25°
    let mut pl2 = LwPolyline::new();
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 5.0)));
    pl2.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));

    let mut gable_left = Wall::new("Standard", 2.50, 0);
    let gable_left_plane = crate::modules::aec::engine::control_plane::ControlPlane::from_slope(
        "Dach_Links",
        [0.0, 5.0, 2.50],
        25.0,
        0.0,
    );
    gable_left.top_origin = gable_left_plane.origin;
    gable_left.top_normal = gable_left_plane.unit_normal();
    gable_left.layers = vec![
        wl("Concrete", 0.30, "Structural"),
    ];

    let mut ent2 = EntityType::LwPolyline(pl2);
    let mut rec2 = ExtendedDataRecord::new(AEC_APPID);
    rec2.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&gable_left);
    ent2.common_mut().extended_data.add_record(rec2);
    let wall2_h = scene.add_entity(ent2);

    // Right wing (5, 5) -> (10, 5), slope -25°
    let mut pl3 = LwPolyline::new();
    pl3.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
    pl3.add_vertex(LwVertex::new(Vector2::new(10.0, 5.0)));

    let mut gable_right = Wall::new("Standard", 2.50, 0);
    let ridge_z = 2.50 + 5.0 * 25.0_f64.to_radians().tan();
    let gable_right_plane = crate::modules::aec::engine::control_plane::ControlPlane::from_slope(
        "Dach_Rechts",
        [5.0, 5.0, ridge_z],
        -25.0,
        0.0,
    );
    gable_right.top_origin = gable_right_plane.origin;
    gable_right.top_normal = gable_right_plane.unit_normal();
    gable_right.layers = vec![
        wl("Concrete", 0.30, "Structural"),
    ];

    let mut ent3 = EntityType::LwPolyline(pl3);
    let mut rec3 = ExtendedDataRecord::new(AEC_APPID);
    rec3.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&gable_right);
    ent3.common_mut().extended_data.add_record(rec3);
    let wall3_h = scene.add_entity(ent3);

    // Window in gable peak
    let _ = place_wall_opening(
        &mut scene,
        wall2_h,
        DVec3::new(2.5, 5.0, 0.0),
        OpeningKind::Window,
        Some(&library),
        None,
        None,
    );

    // Regenerate all walls
    for w in [wall1_h, wall2_h, wall3_h] {
        let touched = regenerate_wall_representation(&mut scene, w, Some(&library)).expect("regen wall");
        for h in touched {
            if let Some(EntityType::Solid3D(_)) = scene.document.get_entity(h) {
                if let Some(body) = scene.solid_models.get(&h) {
                    assert!(body.validate().is_empty(), "Solid B-Rep must be watertight and valid");
                }
            }
        }
    }

    // Add control plane preview Face3D entities on AEC_CONTROLPLANES
    let mut storey = StoreyRef::new("Dachgeschoss", 0.0, "dg.dxf");
    storey.control_planes.push(pult_plane);
    storey.control_planes.push(gable_left_plane);
    storey.control_planes.push(gable_right_plane);
    crate::modules::aec::project::preview::regenerate_control_plane_previews(&mut scene, &mut storey);

    let out = std::path::Path::new("docs/examples/aec-sloped-walls.dxf");
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    acadrust::DxfWriter::new(&scene.document)
        .write_to_file(out)
        .expect("write sloped walls example drawing");
    assert!(out.exists(), "Example file docs/examples/aec-sloped-walls.dxf must exist");

    // Also verify IFC Export containing these sloped walls
    let mut ifc_scene = crate::modules::aec::engine::Scene::default();
    ifc_scene.storeys.push((0, crate::modules::aec::engine::Storey { name: storey.name.clone(), elevation: storey.elevation, height: 2.80 }));
    ifc_scene.walls.push(pult_wall);
    ifc_scene.walls.push(gable_left);
    ifc_scene.walls.push(gable_right);

    let ifc_data = crate::modules::aec::engine::ifc::write_spf(&ifc_scene);
    assert!(ifc_data.contains("IFCWALL"), "IFC output must contain IFCWALL entities");
    assert!(ifc_data.contains("IFCBUILDINGSTOREY"), "IFC output must contain storey");
    assert!(ifc_data.contains("ISO-10303-21"), "Valid SPF header");
}

#[test]
fn xref_wall_and_openings_regenerate_with_active_display_config() {
    use crate::modules::aec::engine::display_component::RepresentationMode;
    use crate::modules::aec::engine::plan_view::{DisplayConfig, PlanningStage, ViewType};

    let mut scene = Scene::new();
    let library = StyleLibrary::default();
    let config = DisplayConfig::new(
        "Arch 1:50".to_string(),
        "Architektur".to_string(),
        PlanningStage::Design,
        ViewType::FloorPlan,
    );

    // 1. Create an XREF block record "EG"
    let mut br = acadrust::tables::BlockRecord::new("EG");
    let br_handle = scene.document.allocate_handle();
    br.handle = br_handle;
    br.flags.is_xref = true;
    scene.document.block_records.add(br).expect("add xref block record");

    // 2. Add a wall inside the XREF block
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));

    let mut wall = Wall::new("Standard", 2.80, 0);
    wall.layers = vec![
        wl("Plaster_Ext", 0.02, "Substrate"),
        wl("Brick", 0.24, "Structural"),
        wl("Plaster_Int", 0.015, "Substrate"),
    ];
    let mut wall_entity = EntityType::LwPolyline(pl);
    wall_entity.common_mut().owner_handle = br_handle;
    wall_entity.as_entity_mut().set_layer("EG|AEC_WALL_AXIS".to_string());
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
    wall_entity.common_mut().extended_data.add_record(rec);
    let wall_handle = scene.add_entity(wall_entity);

    // 3. Add an opening in this XREF wall
    let (open_handle, _) = place_wall_opening(
        &mut scene,
        wall_handle,
        DVec3::new(4.0, 0.0, 0.0),
        OpeningKind::Window,
        Some(&library),
        None,
        None,
    ).expect("place opening in xref wall");

    // Check that opening entity also has owner_handle set to br_handle
    assert_eq!(
        scene.document.get_entity(open_handle).unwrap().common().owner_handle,
        br_handle
    );

    // 4. Regenerate for 2D representation (e.g. Plan 1:50)
    let touched_2d = apply_display_config_to_scene_with_representation(
        &mut scene,
        &config,
        Some(&library),
        Some(RepresentationMode::TwoD),
    );

    assert!(!touched_2d.is_empty());
    for h in &touched_2d {
        let ent = scene.document.get_entity(*h).expect("derived entity in document");
        assert_eq!(
            ent.common().owner_handle,
            br_handle,
            "derived 2d entity must belong to xref block record"
        );
        let layer = ent.common().layer.as_str();
        assert!(
            layer.starts_with("EG|"),
            "derived layer must be prefixed with xref name, got: {layer}"
        );
    }

    // 5. Apply display config for 3D representation
    let touched_3d = apply_display_config_to_scene_with_representation(
        &mut scene,
        &config,
        Some(&library),
        Some(RepresentationMode::ThreeD),
    );

    assert!(!touched_3d.is_empty());
    let mut found_solid = false;
    for h in &touched_3d {
        let ent = scene.document.get_entity(*h).expect("derived 3d entity in document");
        assert_eq!(
            ent.common().owner_handle,
            br_handle,
            "derived 3d entity must belong to xref block record"
        );
        if let EntityType::Solid3D(_) = ent {
            found_solid = true;
            assert!(scene.solid_models.contains_key(h), "Solid3D model must be registered in scene");
            assert!(scene.block_meshes.contains_key(h), "Solid3D mesh for block must be in block_meshes");
        }
    }
    assert!(found_solid, "Model3D mode must generate Solid3D in xref block");
}

#[test]
fn xref_full_integration_with_file_attachment_and_display_switching() {
    use std::collections::HashSet;
    use crate::modules::aec::engine::display_component::RepresentationMode;
    use crate::modules::aec::engine::plan_view::{DisplayConfig, PlanningStage, ViewType};

    let dir = std::env::temp_dir().join(format!("ocs_aec_xref_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    // 1. Create a child drawing with an AEC wall
    let mut child_scene = Scene::new();
    let _child_wall_handle = add_multi_layer_wall(&mut child_scene);
    let bytes = crate::io::save_to_bytes(&child_scene.document, "dwg", child_scene.document.version).unwrap();
    let child_path = dir.join("eg.dwg");
    std::fs::write(&child_path, &bytes).unwrap();

    // 2. Create host scene and attach eg.dwg as XREF
    let mut host_scene = Scene::new();
    let library = StyleLibrary::default();
    let mut br = acadrust::tables::BlockRecord::new("EG");
    let br_handle = host_scene.document.allocate_handle();
    br.handle = br_handle;
    br.flags.is_xref = true;
    br.xref_path = child_path.to_string_lossy().to_string();
    host_scene.document.block_records.add(br).unwrap();

    let mut keys = HashSet::default();
    keys.insert(br_handle);
    let infos = crate::io::xref::resolve_xrefs_for_keys(&mut host_scene.document, &dir, &keys);
    assert!(!infos.is_empty(), "XREF EG must resolve");

    // Add an INSERT in host scene model space
    let ins = acadrust::entities::Insert::new("EG", acadrust::types::Vector3::ZERO);
    let ins_h = host_scene.add_entity(EntityType::Insert(ins));
    assert!(!ins_h.is_null());

    // Initial block epoch
    let initial_epoch = host_scene.block_epoch;

    // 3. Switch host scene to 3D representation
    let config = DisplayConfig::new(
        "Arch 1:50".to_string(),
        "Architektur".to_string(),
        PlanningStage::Design,
        ViewType::FloorPlan,
    );
    let touched_3d = apply_display_config_to_scene_with_representation(
        &mut host_scene,
        &config,
        Some(&library),
        Some(RepresentationMode::ThreeD),
    );
    assert!(!touched_3d.is_empty(), "walls in xref must be regenerated in 3d");
    assert!(host_scene.block_epoch > initial_epoch, "block epoch must increment");

    let solid_handle = touched_3d.iter().find(|&&h| matches!(host_scene.document.get_entity(h), Some(EntityType::Solid3D(_)))).copied().expect("solid in xref");
    assert!(host_scene.block_meshes.contains_key(&solid_handle), "mesh must be stored in block_meshes");

    // 4. Switch host scene back to 2D representation
    let epoch_before_2d = host_scene.block_epoch;
    let touched_2d = apply_display_config_to_scene_with_representation(
        &mut host_scene,
        &config,
        Some(&library),
        Some(RepresentationMode::TwoD),
    );
    assert!(!touched_2d.is_empty(), "walls in xref must be regenerated in 2d");
    assert!(host_scene.block_epoch > epoch_before_2d, "block epoch must increment for 2d switch");
    assert!(!host_scene.block_meshes.contains_key(&solid_handle), "solid mesh must be removed from block_meshes in 2D mode");

    std::fs::remove_dir_all(&dir).ok();
}
