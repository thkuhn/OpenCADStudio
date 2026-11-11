use super::*;

#[test]
fn try_auto_join_nearby_walls_joins_axes_and_returns_touched_handles() {
    let mut scene = Scene::new();
    // Existing: (0,0)->(5,0). New wall approaches an L corner near (5,0).
    let existing = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, existing, None).expect("regen existing");

    let mut pl = LwPolyline::new();
    // End slightly short of the true intersection (5,0) — within snap radius.
    pl.add_vertex(LwVertex::new(Vector2::new(5.1, 4.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.1, 0.1)));
    let mut entity = EntityType::LwPolyline(pl);
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let new_wall = scene.add_entity(entity);
    regenerate_wall_representation(&mut scene, new_wall, None).expect("regen new");

    let touched = try_auto_join_nearby_walls(&mut scene,  new_wall,  None,  None,  None);
    assert!(
        !touched.is_empty(),
        "auto-join should touch both walls' packages"
    );
    assert!(
        touched.contains(&new_wall) && touched.contains(&existing),
        "touched set must include both wall axes, got {touched:?}"
    );

    // Axes must meet at the true intersection.
    let axis_new = get_wall_vertices(&scene, new_wall);
    let axis_ex = get_wall_vertices(&scene, existing);
    let meet = DVec3::new(5.1, 0.0, 0.0); // vertical at x=5.1 meets horizontal y=0
    // join_wall_axes extends the horizontal wall end and moves the vertical end.
    assert!(
        axis_new
            .iter()
            .any(|p| p.distance(meet) < 1e-6)
            || axis_ex.iter().any(|p| {
                axis_new.iter().any(|q| p.distance(*q) < 1e-6)
            }),
        "after auto-join the walls should share an axis intersection; new={axis_new:?} existing={axis_ex:?}"
    );

    // Both walls should still have derived representation handles.
    for h in [new_wall, existing] {
        let v2 = wall_from_entity(scene.document.get_entity(h).unwrap()).unwrap();
        assert!(
            !v2.derived_handles.is_empty(),
            "wall {} should retain derived handles after auto-join",
            h.value()
        );
        for d in &v2.derived_handles {
            assert!(
                touched.contains(d),
                "derived handle {} must be in touched set",
                d.value()
            );
        }
    }

    let peers_new = engine::owner_index::peers_of(&scene.document, new_wall);
    let peers_ex = engine::owner_index::peers_of(&scene.document, existing);
    assert!(
        peers_new.contains(&existing),
        "auto-join must record JOINED_PEERS on the new wall, got {peers_new:?}"
    );
    assert!(
        peers_ex.contains(&new_wall),
        "auto-join must record JOINED_PEERS on the existing wall, got {peers_ex:?}"
    );
}

#[test]
fn try_auto_join_t_records_joined_peers() {
    let mut scene = Scene::new();
    let through = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
    regenerate_wall_representation(&mut scene, through, None).expect("regen through");
    let stem = add_single_layer_wall(&mut scene, (5.0, 0.05), (5.0, 4.0));
    regenerate_wall_representation(&mut scene, stem, None).expect("regen stem");
    let _ = try_auto_join_nearby_walls(&mut scene, stem, None, None, None);
    let peers = engine::owner_index::peers_of(&scene.document, stem);
    assert!(
        peers.contains(&through),
        "T auto-join must list the through wall as JOINED_PEERS, got {peers:?}"
    );
    let peers_t = engine::owner_index::peers_of(&scene.document, through);
    assert!(
        peers_t.contains(&stem),
        "T auto-join must list the stem on the through wall, got {peers_t:?}"
    );
}

#[test]
fn apply_junction_override_rebuilds_partner_wall() {
    let mut scene = Scene::new();
    let wall_a = add_multi_layer_wall(&mut scene);
    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = vec![wl("Concrete", 0.3, "Structural")];
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record(
        "style1",
        3.0,
        0,
        &layers_b,
        &[],
        WallJustification::Center,
        PlanPhase::New,
        None,
    );
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);
    regenerate_wall_representation(&mut scene, wall_a, None).expect("regen a");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("regen b");
    join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None)
        .expect("join");

    let ov = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::NoExtend),
        layer_pairs: Vec::new(),
        layer_gaps: Vec::new(),
    };
    let touched = apply_junction_override_and_rebuild(
        &mut scene,
        wall_a,
        1,
        Some(&ov),
        None,
        None,
        None,
    );
    assert!(touched.contains(&wall_a) && touched.contains(&wall_b));
    assert!(read_junction_override(&scene, wall_a, 1).is_some());
}

#[test]
fn layer_gaps_persist_on_through_wall_not_stem() {
    let mut scene = Scene::new();
    let through = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
    regenerate_wall_representation(&mut scene, through, None).expect("regen through");
    let stem = add_single_layer_wall(&mut scene, (5.0, 0.05), (5.0, 4.0));
    regenerate_wall_representation(&mut scene, stem, None).expect("regen stem");
    let _ = try_auto_join_nearby_walls(&mut scene, stem, None, None, None);
    let stem_end = walls_at_junction(&scene, stem, 0)
        .into_iter()
        .find(|p| p.axis_handle == stem)
        .map(|p| p.end_index)
        .unwrap_or(0);
    let gap = join::LayerGapOverride {
        layer: join::LayerRef {
            material_id: "Concrete".into(),
            role_tag: None,
            index: 0,
            layer_id: None,
        },
        from: join::LayerRef {
            material_id: "Concrete".into(),
            role_tag: None,
            index: 0,
            layer_id: None,
        },
        to: join::LayerRef {
            material_id: "Concrete".into(),
            role_tag: None,
            index: 0,
            layer_id: None,
        },
    };
    let ov = join::JunctionOverride {
        default_style: None,
        layer_pairs: Vec::new(),
        layer_gaps: vec![gap.clone()],
    };
    apply_junction_override_and_rebuild(&mut scene, stem, stem_end, Some(&ov), None, None, None);
    assert!(
        read_junction_override(&scene, stem, stem_end)
            .map(|o| o.layer_gaps.is_empty())
            .unwrap_or(true),
        "stem must not keep layer_gaps"
    );
    let stored = read_junction_override(&scene, through, THROUGH_SPAN_OVERRIDE_END)
        .expect("through span override");
    assert_eq!(stored.layer_gaps.len(), 1);
    assert_eq!(
        read_through_layer_gaps(&scene, stem, stem_end).len(),
        1
    );

    let pair = join::JunctionOverride {
        default_style: None,
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Concrete".into(),
                role_tag: None,
                index: 0,
                layer_id: None,
            },
            layer_b: Some(join::LayerRef {
                material_id: "Concrete".into(),
                role_tag: None,
                index: 0,
                layer_id: None,
            }),
            style: join::JoinOverrideStyle::Miter,
        }],
        layer_gaps: Vec::new(),
    };
    apply_junction_override_and_rebuild(
        &mut scene,
        stem,
        stem_end,
        Some(&pair),
        None,
        None,
        None,
    );
    assert_eq!(
        read_through_layer_gaps(&scene, stem, stem_end).len(),
        1,
        "defining a layer pair must not erase through-wall interruptions"
    );

    let gap_b = join::LayerGapOverride {
        layer: join::LayerRef {
            material_id: "Insulation".into(),
            role_tag: None,
            index: 1,
            layer_id: None,
        },
        from: join::LayerRef {
            material_id: "Concrete".into(),
            role_tag: None,
            index: 0,
            layer_id: None,
        },
        to: join::LayerRef {
            material_id: "Concrete".into(),
            role_tag: None,
            index: 0,
            layer_id: None,
        },
    };
    let mut existing = read_through_layer_gaps(&scene, stem, stem_end);
    join::upsert_layer_gap(&mut existing, gap_b);
    let ov2 = join::JunctionOverride {
        default_style: None,
        layer_pairs: Vec::new(),
        layer_gaps: existing,
    };
    apply_junction_override_and_rebuild(
        &mut scene,
        stem,
        stem_end,
        Some(&ov2),
        None,
        None,
        None,
    );
    assert_eq!(
        read_through_layer_gaps(&scene, stem, stem_end).len(),
        2,
        "a second layer interruption must keep the first"
    );
}

#[test]
fn layer_gap_rebuild_notches_through_core() {
    let mut scene = Scene::new();
    let layers = plaster_masonry_plaster();
    let through = add_layered_wall(&mut scene, (0.0, 0.0), (10.0, 0.0), layers.clone());
    regenerate_wall_representation(&mut scene, through, None).expect("regen through");
    let stem = add_layered_wall(&mut scene, (5.0, 0.05), (5.0, 4.0), layers);
    regenerate_wall_representation(&mut scene, stem, None).expect("regen stem");
    let _ = try_auto_join_nearby_walls(&mut scene, stem, None, None, None);
    let stem_end = walls_at_junction(&scene, stem, 0)
        .into_iter()
        .find(|p| p.axis_handle == stem)
        .map(|p| p.end_index)
        .unwrap_or(0);
    let t_layers = wall_from_entity(scene.document.get_entity(through).unwrap())
        .unwrap()
        .layers;
    let s_layers = wall_from_entity(scene.document.get_entity(stem).unwrap())
        .unwrap()
        .layers;
    let layer_ref = |l: &WallLayer, index: usize| join::LayerRef {
        material_id: l.material.clone(),
        role_tag: None,
        index,
        layer_id: Some(l.layer_id),
    };
    let ov = join::JunctionOverride {
        default_style: None,
        layer_pairs: Vec::new(),
        layer_gaps: vec![join::LayerGapOverride {
            layer: layer_ref(&t_layers[1], 1),
            from: layer_ref(&s_layers[0], 0),
            to: layer_ref(&s_layers[2], 2),
        }],
    };
    apply_junction_override_and_rebuild(&mut scene, stem, stem_end, Some(&ov), None, None, None);

    let axis_2d: Vec<(f64, f64)> = get_wall_vertices(&scene, through)
        .iter()
        .map(|p| (p.x, p.y))
        .collect();
    let cut = find_through_span_cutout_footprints(&mut scene, through, &axis_2d)
        .expect("T cutout");
    assert!(
        cut.get(1).and_then(|fp| fp.as_ref()).is_some(),
        "manual gap must interrupt through masonry after rebuild, got {cut:?}"
    );

    regenerate_wall_representation(&mut scene, through, None).expect("display reapply");
    let cut_after = find_through_span_cutout_footprints(&mut scene, through, &axis_2d)
        .expect("T cutout after regen");
    assert!(
        cut_after.get(1).and_then(|fp| fp.as_ref()).is_some(),
        "masonry interruption must survive display regen, got {cut_after:?}"
    );

    let loops = wall_contour_loops(&scene, through);
    let masonry_left = loops.iter().any(|lp| {
        let min_x = lp.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let max_x = lp.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        min_x < 0.1
            && max_x < 4.95
            && lp.iter().any(|p| p.1.abs() > 0.05 && p.1.abs() < 0.121)
    });
    let masonry_right = loops.iter().any(|lp| {
        let min_x = lp.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let max_x = lp.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        min_x > 5.05
            && max_x > 9.9
            && lp.iter().any(|p| p.1.abs() > 0.05 && p.1.abs() < 0.121)
    });
    assert!(
        masonry_left && masonry_right,
        "masonry must be split around the stem, got {loops:?}"
    );
}

#[test]
fn layer_gap_on_second_t_junction_still_notches() {
    let mut scene = Scene::new();
    let layers = plaster_masonry_plaster();
    let through = add_layered_wall(&mut scene, (0.0, 0.0), (10.0, 0.0), layers.clone());
    regenerate_wall_representation(&mut scene, through, None).expect("regen through");
    let stem_a = add_layered_wall(&mut scene, (3.0, 0.05), (3.0, 4.0), layers.clone());
    regenerate_wall_representation(&mut scene, stem_a, None).expect("regen stem a");
    let _ = try_auto_join_nearby_walls(&mut scene, stem_a, None, None, None);
    let stem_b = add_layered_wall(&mut scene, (7.0, 0.05), (7.0, 4.0), layers);
    regenerate_wall_representation(&mut scene, stem_b, None).expect("regen stem b");
    let _ = try_auto_join_nearby_walls(&mut scene, stem_b, None, None, None);

    let stem_end = walls_at_junction(&scene, stem_b, 0)
        .into_iter()
        .find(|p| p.axis_handle == stem_b)
        .map(|p| p.end_index)
        .unwrap_or(0);
    let t_layers = wall_from_entity(scene.document.get_entity(through).unwrap())
        .unwrap()
        .layers;
    let s_layers = wall_from_entity(scene.document.get_entity(stem_b).unwrap())
        .unwrap()
        .layers;
    let layer_ref = |l: &WallLayer, index: usize| join::LayerRef {
        material_id: l.material.clone(),
        role_tag: None,
        index,
        layer_id: Some(l.layer_id),
    };
    let ov = join::JunctionOverride {
        default_style: None,
        layer_pairs: Vec::new(),
        layer_gaps: vec![join::LayerGapOverride {
            layer: layer_ref(&t_layers[1], 1),
            from: layer_ref(&s_layers[0], 0),
            to: layer_ref(&s_layers[2], 2),
        }],
    };
    apply_junction_override_and_rebuild(&mut scene, stem_b, stem_end, Some(&ov), None, None, None);
    regenerate_wall_representation(&mut scene, through, None).expect("display reapply");

    let loops = wall_contour_loops(&scene, through);
    let notched_at_b = loops.iter().any(|lp| {
        let min_x = lp.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let max_x = lp.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        min_x > 7.05
            && max_x > 9.9
            && lp.iter().any(|p| p.1.abs() > 0.05 && p.1.abs() < 0.121)
    });
    assert!(
        notched_at_b,
        "gap at the second T-junction must split masonry, got {loops:?}"
    );
}

pub(super) fn plaster_masonry_plaster() -> Vec<WallLayer> {
    wls(&[
        ("Putz", 0.015, "Finish"),
        ("Mauerwerk", 0.24, "Structural"),
        ("Putz", 0.015, "Finish"),
    ])
}

pub(super) fn plaster_masonry_insulation_plaster() -> Vec<WallLayer> {
    wls(&[
        ("Putz", 0.015, "Finish"),
        ("Mauerwerk", 0.24, "Structural"),
        ("Insulation", 0.12, "Insulation"),
        ("Putz", 0.015, "Finish"),
    ])
}

#[test]
fn join_l_identical_insulated_miters_inner_plaster() {
    let mut scene = Scene::new();
    let layers = plaster_masonry_insulation_plaster();
    let wall_a = add_layered_wall(&mut scene, (0.0, 0.0), (10.0, 0.0), layers.clone());
    let wall_b = add_layered_wall(&mut scene, (10.0, 10.0), (10.0, 0.0), layers);
    join_l_pair(&mut scene, wall_a, wall_b);

    let wall = wall_from_entity(scene.document.get_entity(wall_a).unwrap()).unwrap();
    let mut inner_pts = Vec::new();
    for h in &wall.derived_handles {
        let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*h) else {
            continue;
        };
        let ys: Vec<f64> = pl.vertices.iter().map(|v| v.location.y).collect();
        let y_min = ys.iter().copied().fold(f64::INFINITY, f64::min);
        let y_max = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if y_max < -0.05 && y_min > -0.25 {
            inner_pts.extend(pl.vertices.iter().map(|v| (v.location.x, v.location.y)));
        }
    }
    assert!(
        !inner_pts.is_empty(),
        "inner plaster contour should exist"
    );
    let square_cap = inner_pts.iter().any(|(x, y)| {
        (*x - 10.0).abs() < 1e-6 && *y < -0.05
    });
    let mitered = inner_pts.iter().any(|(x, y)| {
        *x > 10.05 && *y < -0.05
    });
    assert!(
        mitered && !square_cap,
        "inner plaster must miter past the axis end, not square-cap, pts={inner_pts:?}"
    );

    let mut room_pts = Vec::new();
    for h in &wall.derived_handles {
        let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*h) else {
            continue;
        };
        let ys: Vec<f64> = pl.vertices.iter().map(|v| v.location.y).collect();
        let y_min = ys.iter().copied().fold(f64::INFINITY, f64::min);
        let y_max = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if y_min > 0.05 && y_max < 0.25 {
            room_pts.extend(pl.vertices.iter().map(|v| (v.location.x, v.location.y)));
        }
    }
    assert!(!room_pts.is_empty(), "room-side plaster contour should exist");
    let room_square = room_pts.iter().any(|(x, y)| (*x - 10.0).abs() < 1e-6 && *y > 0.05);
    let room_miter = room_pts.iter().any(|(x, y)| *x < 9.95 && *y > 0.05);
    assert!(
        room_miter && !room_square,
        "room-side plaster at the inner L-corner must miter, pts={room_pts:?}"
    );
}

pub(super) fn join_l_pair(
    scene: &mut Scene,
    a: Handle,
    b: Handle,
) {
    let lib = crate::modules::aec::engine::library::seed_default_library();
    let _ = regenerate_wall_representation(scene, a, Some(&lib));
    let _ = regenerate_wall_representation(scene, b, Some(&lib));
    let _ = join_two_walls_in_document(scene, a, b, Some(&lib), None, None);
}

pub(super) fn add_example_note(scene: &mut Scene, x: f64, y: f64, value: &str) {
    use acadrust::entities::MText;
    let mut t = MText::with_value(value.to_string(), Vector3::new(x, y, 0.0));
    t.height = 0.28;
    scene.add_entity(EntityType::MText(Box::new(t)));
}

pub(super) fn place_example_opening(
    scene: &mut Scene,
    wall: Handle,
    pt: (f64, f64),
    kind: engine::openings::OpeningKind,
    lib: &StyleLibrary,
) -> Handle {
    let (handle, _) = place_wall_opening(
        scene,
        wall,
        DVec3::new(pt.0, pt.1, 0.0),
        kind,
        Some(lib),
        None,
        None,
    )
    .expect("place example opening");
    handle
}

pub(super) fn add_wall_with_style(
    scene: &mut Scene,
    p1: (f64, f64),
    p2: (f64, f64),
    style_id: &str,
    lib: &StyleLibrary,
) -> Handle {
    use crate::modules::aec::engine::xdata::resolve_wall_style_layers;
    let resolved = resolve_wall_style_layers(lib, style_id, None)
        .unwrap_or_else(|| vec![wl("Concrete", 0.20, "Structural")]);
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
    pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
    let mut entity = EntityType::LwPolyline(pl);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record(
        style_id,
        2.80,
        0,
        &resolved,
        &[],
        WallJustification::Center,
        PlanPhase::New,
        None,
    );
    entity.common_mut().extended_data.add_record(record);
    let h = scene.add_entity(entity);
    let _ = regenerate_wall_representation(scene, h, Some(lib));
    h
}

pub(super) fn place_example_opening_styled(
    scene: &mut Scene,
    wall: Handle,
    pt: (f64, f64),
    style_id: &str,
    lib: &StyleLibrary,
) -> Handle {
    let opening_style = lib
        .find_opening_style(style_id)
        .expect("opening style must exist in library");
    let (handle, _) = place_wall_opening(
        scene,
        wall,
        DVec3::new(pt.0, pt.1, 0.0),
        opening_style.kind,
        Some(lib),
        None,
        None,
    )
    .expect("place example opening");
    let mut opening = openings_for_host_wall(scene, wall)
        .into_iter()
        .find(|o| o.handle == handle)
        .expect("opening found");
    crate::modules::aec::engine::opening_style::apply_style_defaults(&mut opening, opening_style);
    write_opening_instance(scene, &opening);
    regenerate_wall_representation(scene, wall, Some(lib)).expect("regen wall representation");
    handle
}

#[test]
fn write_aec_wall_join_examples_dxf() {
    use crate::modules::aec::engine::opening_style::{
        SEED_BREAKTHROUGH_PIPE_STYLE_ID, SEED_BREAKTHROUGH_STYLE_ID, SEED_DOOR_DOUBLE_STYLE_ID,
        SEED_DOOR_RIGHT_STYLE_ID, SEED_DOOR_STYLE_ID, SEED_WINDOW_ARCH_STYLE_ID,
        SEED_WINDOW_CIRCLE_STYLE_ID, SEED_WINDOW_DOUBLE_STYLE_ID, SEED_WINDOW_FLOOR_STYLE_ID,
        SEED_WINDOW_STYLE_ID,
    };

    let mut scene = Scene::new();
    let lib = crate::modules::aec::engine::library::seed_default_library();

    // Titel-Kopfzeile
    add_example_note(
        &mut scene,
        0.0,
        70.0,
        "OpenCADStudio AEC — Referenz- und Konstruktionsbeispiele\\PWandstile, Wandverbindungen (L/T/N), Oeffnungen, 2D-Grundriss, 2D-Ansichten & 3D-B-Rep-Modelle",
    );

    // =========================================================================
    // SEKTION 1: L-Wandverbindungen (y = 0..5)
    // =========================================================================
    let l_a = add_single_layer_wall(&mut scene, (0.0, 0.0), (5.0, 0.0));
    let l_b = add_single_layer_wall(&mut scene, (5.0, 0.0), (5.0, 5.0));
    join_l_pair(&mut scene, l_a, l_b);
    add_example_note(
        &mut scene,
        0.0,
        -1.4,
        "L einschalig\\PStil: style_concrete_20 (Beton 20cm)\\PVerbindung: L, Standard-Miter\\POverride: keiner",
    );

    let l3_a = add_layered_wall(&mut scene, (10.0, 0.0), (16.0, 0.0), plaster_masonry_plaster());
    let l3_b = add_layered_wall(&mut scene, (16.0, 0.0), (16.0, 5.0), plaster_masonry_plaster());
    join_l_pair(&mut scene, l3_a, l3_b);
    add_example_note(
        &mut scene,
        10.0,
        -1.4,
        "L dreischalig\\PStil: Mauerwerk + beids. Putz (27cm)\\PVerbindung: L, Standard-Miter\\POverride: keiner",
    );

    let l4_a = add_wall_with_style(&mut scene, (20.0, 0.0), (26.0, 0.0), "style_insulated_ext", &lib);
    let l4_b = add_wall_with_style(&mut scene, (26.0, 0.0), (26.0, 5.0), "style_insulated_ext", &lib);
    join_l_pair(&mut scene, l4_a, l4_b);
    add_example_note(
        &mut scene,
        20.0,
        -1.4,
        "L vierschalig\\PStil: style_insulated_ext (WDVS 34.5cm)\\PVerbindung: L, Standard-Miter\\POverride: keiner",
    );

    let l5_a = add_wall_with_style(&mut scene, (30.0, 0.0), (36.0, 0.0), "style_cavity_brick", &lib);
    let l5_b = add_wall_with_style(&mut scene, (36.0, 0.0), (36.0, 5.0), "style_cavity_brick", &lib);
    join_l_pair(&mut scene, l5_a, l5_b);
    add_example_note(
        &mut scene,
        30.0,
        -1.4,
        "L fuenfschalig\\PStil: style_cavity_brick (Klinkerwand 42cm)\\PVerbindung: L, Mehrschicht-Miter",
    );

    // =========================================================================
    // SEKTION 2: T-Wandverbindungen (y = 12..18)
    // =========================================================================
    let t4_through = add_wall_with_style(&mut scene, (0.0, 12.0), (10.0, 12.0), "style_insulated_ext", &lib);
    let t4_stem = add_wall_with_style(&mut scene, (5.0, 12.05), (5.0, 18.0), "style_insulated_ext", &lib);
    let _ = regenerate_wall_representation(&mut scene, t4_through, Some(&lib));
    let _ = regenerate_wall_representation(&mut scene, t4_stem, Some(&lib));
    let _ = try_auto_join_nearby_walls(&mut scene, t4_stem, Some(&lib), None, None);
    add_example_note(
        &mut scene,
        0.0,
        10.3,
        "T vierschalig (WDVS)\\PStil: style_insulated_ext\\PVerbindung: T (Auto-Join)\\PStamm -> Durchgang",
    );

    let t3_through = add_layered_wall(&mut scene, (14.0, 12.0), (24.0, 12.0), plaster_masonry_plaster());
    let t3_stem = add_layered_wall(&mut scene, (19.0, 12.05), (19.0, 18.0), plaster_masonry_plaster());
    let _ = regenerate_wall_representation(&mut scene, t3_through, Some(&lib));
    let _ = regenerate_wall_representation(&mut scene, t3_stem, Some(&lib));
    let _ = try_auto_join_nearby_walls(&mut scene, t3_stem, Some(&lib), None, None);
    add_example_note(
        &mut scene,
        14.0,
        10.3,
        "T dreischalig\\PStil: Mauerwerk + Putz\\PVerbindung: T (Auto-Join)\\PStamm -> Durchgang",
    );

    let t_dry_through = add_wall_with_style(&mut scene, (28.0, 12.0), (38.0, 12.0), "style_concrete_25", &lib);
    let t_dry_stem = add_wall_with_style(&mut scene, (33.0, 12.05), (33.0, 18.0), "style_drywall_10", &lib);
    let _ = regenerate_wall_representation(&mut scene, t_dry_through, Some(&lib));
    let _ = regenerate_wall_representation(&mut scene, t_dry_stem, Some(&lib));
    let _ = try_auto_join_nearby_walls(&mut scene, t_dry_stem, Some(&lib), None, None);
    add_example_note(
        &mut scene,
        28.0,
        10.3,
        "T-Stoss Materialwechsel\\PTrockenbauwand 10cm an Betonwand 25cm\\PVerbindung: T (Auto-Join)",
    );

    // =========================================================================
    // SEKTION 3: N-Wege & Schicht-Overrides (y = 24..30)
    // =========================================================================
    let n1 = add_single_layer_wall(&mut scene, (0.0, 24.0), (8.0, 24.0));
    let n2 = add_single_layer_wall(&mut scene, (8.0, 24.0), (8.0, 30.0));
    let n3 = add_single_layer_wall(&mut scene, (8.0, 24.0), (8.0, 18.0));
    for h in [n1, n2, n3] {
        let _ = regenerate_wall_representation(&mut scene, h, Some(&lib));
    }
    let _ = join_junction_in_document(&mut scene, &[n1, n2, n3], None, Some(&lib), None, None);
    add_example_note(
        &mut scene,
        0.0,
        31.2,
        "N-Wege einschalig\\PStil: style_concrete_20\\PVerbindung: N-Wege (3 Waende)\\PStandard-Miter",
    );

    let ov_a = add_layered_wall(
        &mut scene,
        (14.0, 24.0),
        (22.0, 24.0),
        plaster_masonry_insulation_plaster(),
    );
    let ov_b = add_layered_wall(
        &mut scene,
        (22.0, 24.0),
        (22.0, 30.0),
        plaster_masonry_insulation_plaster(),
    );
    join_l_pair(&mut scene, ov_a, ov_b);
    let layers = wall_from_entity(scene.document.get_entity(ov_a).unwrap())
        .unwrap()
        .layers;
    let refs = layer_refs_from_materials(
        layers.iter().map(|l| (l.material.as_str(), l.layer_id)),
    );
    let ov = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::Miter),
        layer_pairs: vec![
            join::LayerPairOverride {
                layer_a: refs[0].clone(),
                layer_b: None,
                style: join::JoinOverrideStyle::NoExtend,
            },
            join::LayerPairOverride {
                layer_a: refs[1].clone(),
                layer_b: refs.get(1).cloned(),
                style: join::JoinOverrideStyle::Miter,
            },
            join::LayerPairOverride {
                layer_a: refs[2].clone(),
                layer_b: None,
                style: join::JoinOverrideStyle::Butt,
            },
            join::LayerPairOverride {
                layer_a: refs[3].clone(),
                layer_b: None,
                style: join::JoinOverrideStyle::NoExtend,
            },
        ],
        layer_gaps: Vec::new(),
    };
    let _ = apply_junction_override_and_rebuild(
        &mut scene, ov_a, 1, Some(&ov), Some(&lib), None, None,
    );
    add_example_note(
        &mut scene,
        14.0,
        31.2,
        "L vierschalig mit Schicht-Overrides\\PStil: Putz+Mauerwerk+Insulation+Putz\\PVerbindung: L\\PPutz: NoExtend, Mauerwerk: Miter, Daemmung: Butt",
    );

    // =========================================================================
    // SEKTION 4: Wandstile-Galerie / Wandaufbauten (y = 36..40)
    // =========================================================================
    let _w_masonry = add_wall_with_style(&mut scene, (0.0, 36.0), (6.0, 36.0), "style_masonry", &lib);
    add_example_note(
        &mut scene,
        0.0,
        34.6,
        "Wandstil: Mauerwerk 24cm\\PID: style_masonry\\P1-schalig tragend\\P24cm Ziegel",
    );

    let _w_conc = add_wall_with_style(&mut scene, (8.0, 36.0), (14.0, 36.0), "style_concrete_25", &lib);
    add_example_note(
        &mut scene,
        8.0,
        34.6,
        "Wandstil: Stahlbeton 25cm\\PID: style_concrete_25\\P1-schalig tragend\\P25cm Stahlbeton",
    );

    let _w_dry = add_wall_with_style(&mut scene, (16.0, 36.0), (22.0, 36.0), "style_drywall_10", &lib);
    add_example_note(
        &mut scene,
        16.0,
        34.6,
        "Wandstil: Trockenbau 10cm\\PID: style_drywall_10\\P3-schalig nichttragend\\P1.25cm GKB + 7.5cm Daemmung + 1.25cm GKB",
    );

    let _w_timber = add_wall_with_style(&mut scene, (24.0, 36.0), (30.0, 36.0), "style_timber_exterior", &lib);
    add_example_note(
        &mut scene,
        24.0,
        34.6,
        "Wandstil: Holzstaender 28cm\\PID: style_timber_exterior\\P4-schalig oekologisch\\P1.5cm GKB + 6cm Inst. + 16cm Holz + 4.5cm Verschalung",
    );

    let _w_cavity = add_wall_with_style(&mut scene, (32.0, 36.0), (38.0, 36.0), "style_cavity_brick", &lib);
    add_example_note(
        &mut scene,
        32.0,
        34.6,
        "Wandstil: Verblendwand 42cm\\PID: style_cavity_brick\\P5-schalig zweischalig\\P1.5cm Putz + 17.5cm MW + 12cm Daemmung + 2cm Luft + 9cm Klinker",
    );

    // =========================================================================
    // SEKTION 5: Fenster-Stile & Ansichten (y = 48..52)
    // =========================================================================
    let wf1 = add_wall_with_style(&mut scene, (0.0, 48.0), (6.0, 48.0), "style_insulated_ext", &lib);
    let of1 = place_example_opening_styled(&mut scene, wf1, (3.0, 48.0), SEED_WINDOW_STYLE_ID, &lib);
    add_example_note(
        &mut scene,
        0.0,
        46.4,
        "Standardfenster 1.20x1.20m\\PID: style_window_standard\\P1 Fluegel, Bruestung 0.90m\\PAnsicht: Rahmen + Aufschlagdreieck + Sill\\P3D: Profilrahmen + Glas",
    );

    let wf2 = add_wall_with_style(&mut scene, (8.0, 48.0), (14.0, 48.0), "style_insulated_ext", &lib);
    let of2 = place_example_opening_styled(&mut scene, wf2, (11.0, 48.0), SEED_WINDOW_DOUBLE_STYLE_ID, &lib);
    add_example_note(
        &mut scene,
        8.0,
        46.4,
        "Zweifluegelfenster 1.60x1.40m\\PID: style_window_double\\P2 Fluegel, Mittelpfosten, Bruestung 0.90m\\PAnsicht: 2 Aufschlagdreiecke + Doppelsprosse",
    );

    let wf3 = add_wall_with_style(&mut scene, (16.0, 48.0), (22.0, 48.0), "style_insulated_ext", &lib);
    let of3 = place_example_opening_styled(&mut scene, wf3, (19.0, 48.0), SEED_WINDOW_ARCH_STYLE_ID, &lib);
    add_example_note(
        &mut scene,
        16.0,
        46.4,
        "Rundbogenfenster 1.00x1.80m\\PID: style_window_arch\\PBogenform (Spring 1.30m), Bruestung 0.80m\\PAnsicht: Bogenrahmen + Aufschlagdreieck\\P3D: Exakter B-Rep-Bogenkoerper",
    );

    let wf4 = add_wall_with_style(&mut scene, (24.0, 48.0), (30.0, 48.0), "style_insulated_ext", &lib);
    let of4 = place_example_opening_styled(&mut scene, wf4, (27.0, 48.0), SEED_WINDOW_CIRCLE_STYLE_ID, &lib);
    add_example_note(
        &mut scene,
        24.0,
        46.4,
        "Rundfenster / Ochsenauge 0.90x0.90m\\PID: style_window_circle\\PKreisform, Bruestung 1.20m\\PAnsicht: Runder Rahmen\\P3D: Runder B-Rep-Toruskörper",
    );

    let wf5 = add_wall_with_style(&mut scene, (32.0, 48.0), (38.0, 48.0), "style_insulated_ext", &lib);
    let of5 = place_example_opening_styled(&mut scene, wf5, (35.0, 48.0), SEED_WINDOW_FLOOR_STYLE_ID, &lib);
    add_example_note(
        &mut scene,
        32.0,
        46.4,
        "Bodentiefes Fenster 1.00x2.20m\\PID: style_window_floor\\PBruestung 0.00m, Schwelle / Threshold\\PAnsicht: Rahmen + Aufschlagdreieck",
    );

    // =========================================================================
    // SEKTION 6: Tueren, Tore & Durchbrueche (y = 60..64)
    // =========================================================================
    let wt1 = add_wall_with_style(&mut scene, (0.0, 60.0), (6.0, 60.0), "style_drywall_10", &lib);
    let ot1 = place_example_opening_styled(&mut scene, wt1, (3.0, 60.0), SEED_DOOR_STYLE_ID, &lib);
    add_example_note(
        &mut scene,
        0.0,
        58.4,
        "Standard-Innentuer links 0.90x2.10m\\PID: style_door_standard\\PAnschlag links, Aufschlagbogen 90 Grad\\PAnsicht: Zarge + Aufschlagdreieck\\P3D: Zarge + Tuerblatt",
    );

    let wt2 = add_wall_with_style(&mut scene, (8.0, 60.0), (14.0, 60.0), "style_drywall_10", &lib);
    let ot2 = place_example_opening_styled(&mut scene, wt2, (11.0, 60.0), SEED_DOOR_RIGHT_STYLE_ID, &lib);
    add_example_note(
        &mut scene,
        8.0,
        58.4,
        "Innentuer rechts 0.90x2.10m\\PID: style_door_right\\PAnschlag rechts\\PAnsicht: Zarge + Aufschlagdreieck gespiegelt\\P3D: Zarge + Tuerblatt",
    );

    let wt3 = add_wall_with_style(&mut scene, (16.0, 60.0), (22.0, 60.0), "style_concrete_25", &lib);
    let ot3 = place_example_opening_styled(&mut scene, wt3, (19.0, 60.0), SEED_DOOR_DOUBLE_STYLE_ID, &lib);
    add_example_note(
        &mut scene,
        16.0,
        58.4,
        "Zweifluegelige Eingangstuer 1.80x2.20m\\PID: style_door_double\\PDoppelfluegel mit Mittelstoss\\PAnsicht: 2 Aufschlagdreiecke + Mittelpfosten\\P3D: Zarge + Tuerblaetter",
    );

    let wt4 = add_wall_with_style(&mut scene, (24.0, 60.0), (30.0, 60.0), "style_concrete_25", &lib);
    let ot4 = place_example_opening_styled(&mut scene, wt4, (27.0, 60.0), SEED_BREAKTHROUGH_STYLE_ID, &lib);
    add_example_note(
        &mut scene,
        24.0,
        58.4,
        "Wanddurchbruch 1.00x2.00m\\PID: style_breakthrough_standard\\PBruestung 0.10m\\P2D: Kreuz-Symbolik (Mark2D)\\PAnsicht: Oeffnungskontur",
    );

    let wt5 = add_wall_with_style(&mut scene, (32.0, 60.0), (38.0, 60.0), "style_concrete_25", &lib);
    let ot5 = place_example_opening_styled(&mut scene, wt5, (35.0, 60.0), SEED_BREAKTHROUGH_PIPE_STYLE_ID, &lib);
    add_example_note(
        &mut scene,
        32.0,
        58.4,
        "Installationsdurchbruch rund 0.40x0.40m\\PID: style_breakthrough_pipe\\PKreisform, Bruestung 1.80m\\P2D: Schraffur / DiagonalFill\\PAnsicht: Runder Aussparungsring",
    );

    // Validierung aller erzeugten Oeffnungen
    let all_placed = [
        (wf1, of1, engine::openings::OpeningKind::Window),
        (wf2, of2, engine::openings::OpeningKind::Window),
        (wf3, of3, engine::openings::OpeningKind::Window),
        (wf4, of4, engine::openings::OpeningKind::Window),
        (wf5, of5, engine::openings::OpeningKind::Window),
        (wt1, ot1, engine::openings::OpeningKind::Door),
        (wt2, ot2, engine::openings::OpeningKind::Door),
        (wt3, ot3, engine::openings::OpeningKind::Door),
        (wt4, ot4, engine::openings::OpeningKind::Breakthrough),
        (wt5, ot5, engine::openings::OpeningKind::Breakthrough),
    ];
    for (wall, opening, kind) in all_placed {
        let parsed = opening_from_entity(scene.document.get_entity(opening).unwrap(), opening)
            .expect("opening xdata");
        assert_eq!(parsed.kind, kind);
        assert_eq!(parsed.host_wall, wall);
        assert!(
            engine::owner_index::children_of(&scene.document, wall).contains(&opening),
            "opening {opening:?} must be a child of host wall {wall:?}"
        );
        let children = engine::opening_display::collect_opening_display_children(&scene, opening);
        assert!(
            !children.is_empty(),
            "opening {kind:?} must have OPENING_REP children"
        );
    }

    let out = std::path::Path::new("docs/examples/aec-wall-joins.dxf");
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    acadrust::DxfWriter::new(&scene.document)
        .write_to_file(out)
        .expect("write example drawing");
    assert!(out.exists());
}

#[test]
fn try_auto_join_nearby_walls_is_noop_when_nothing_nearby() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall, None).expect("regen");
    let before = get_wall_vertices(&scene, wall);
    let touched = try_auto_join_nearby_walls(&mut scene,  wall,  None,  None,  None);
    assert!(touched.is_empty(), "no partner → no touched handles");
    let after = get_wall_vertices(&scene, wall);
    assert_eq!(before, after, "axis must be unchanged when auto-join is a no-op");
}

/// Collect layer boundary polylines (XY vertices) for a wall's derived 2D representation.
pub(super) fn wall_derived_layer_vertices(scene: &Scene, wall: Handle) -> Vec<Vec<(f64, f64)>> {
    let v2 = wall_from_entity(scene.document.get_entity(wall).unwrap()).unwrap();
    let mut polylines = Vec::new();
    for h in &v2.derived_handles {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*h) {
            let pts: Vec<(f64, f64)> = pl.vertices.iter().map(|v| (v.location.x, v.location.y)).collect();
            if !pts.is_empty() {
                polylines.push(pts);
            }
        }
    }
    polylines
}

pub(super) fn assert_points_match(pts_a: &[(f64, f64)], pts_b: &[(f64, f64)], tol: f64) {
    assert_eq!(pts_a.len(), pts_b.len(), "point count mismatch: {} vs {}", pts_a.len(), pts_b.len());
    let mut a = pts_a.to_vec();
    let mut b = pts_b.to_vec();
    a.sort_by(|p1, p2| p1.0.partial_cmp(&p2.0).unwrap().then(p1.1.partial_cmp(&p2.1).unwrap()));
    b.sort_by(|p1, p2| p1.0.partial_cmp(&p2.0).unwrap().then(p1.1.partial_cmp(&p2.1).unwrap()));
    for (p1, p2) in a.iter().zip(&b) {
        assert!(
            (p1.0 - p2.0).hypot(p1.1 - p2.1) <= tol,
            "point mismatch: {:?} vs {:?}",
            p1,
            p2
        );
    }
}

#[test]
fn scene_cascade_u_chain_joining_third_wall_preserves_first_corner_contour() {
    let mut scene = Scene::new();
    fn add_w(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64)) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural"), wl("Insulation", 0.05, "Insulation")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }

    let w1 = add_w(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let w2 = add_w(&mut scene, (10.0, 0.0), (10.0, 10.0));
    regenerate_wall_representation(&mut scene, w1, None).unwrap();
    regenerate_wall_representation(&mut scene, w2, None).unwrap();

    let (k1, _) = join_two_walls_in_document(&mut scene, w1, w2, None, None, None).unwrap();
    assert_eq!(k1, JoinKind::L);

    let w1_derived_before = wall_derived_layer_vertices(&scene, w1);
    let w2_derived_before = wall_derived_layer_vertices(&scene, w2);
    assert!(!w1_derived_before.is_empty());
    assert!(!w2_derived_before.is_empty());
    let w2_corner_before: Vec<_> = w2_derived_before
        .iter()
        .flatten()
        .filter(|p| (p.0 - 10.0).abs() < 2.0 && (p.1 - 0.0).abs() < 2.0)
        .copied()
        .collect();
    assert!(!w2_corner_before.is_empty(), "W2 corner at (10,0) must exist before joining W3");
    let w1_axis_before = get_wall_vertices(&scene, w1);

    // Now add and join third wall
    let w3 = add_w(&mut scene, (10.0, 10.0), (0.0, 10.0));
    regenerate_wall_representation(&mut scene, w3, None).unwrap();
    let (k2, _) = join_two_walls_in_document(&mut scene, w2, w3, None, None, None).unwrap();
    assert_eq!(k2, JoinKind::L);

    let w1_axis_after = get_wall_vertices(&scene, w1);
    assert_eq!(w1_axis_before, w1_axis_after, "W1 axis must be untouched by W2-W3 join");

    let w1_derived_after = wall_derived_layer_vertices(&scene, w1);
    assert_eq!(
        w1_derived_before, w1_derived_after,
        "W1 derived layer contours at corner (10,0) must remain identical when W2 joins W3"
    );

    // W2 must maintain peer links to both W1 and W3
    let peers_w2 = engine::owner_index::peers_of(&scene.document, w2);
    assert!(peers_w2.contains(&w1), "W2 must keep peer link to W1");
    assert!(peers_w2.contains(&w3), "W2 must have peer link to W3");

    let w2_derived_after = wall_derived_layer_vertices(&scene, w2);
    assert!(!w2_derived_after.is_empty());
    let w2_corner_after: Vec<_> = w2_derived_after
        .iter()
        .flatten()
        .filter(|p| (p.0 - 10.0).abs() < 2.0 && (p.1 - 0.0).abs() < 2.0)
        .copied()
        .collect();
    assert_points_match(&w2_corner_before, &w2_corner_after, 1e-4);
}

#[test]
fn scene_cascade_z_chain_joining_walls_preserves_opposite_miters() {
    let mut scene = Scene::new();
    fn add_w(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64)) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.24, "Structural"), wl("Insulation", 0.06, "Insulation")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }

    let w1 = add_w(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let w2 = add_w(&mut scene, (10.0, 0.0), (10.0, 10.0));
    let w3 = add_w(&mut scene, (10.0, 10.0), (20.0, 10.0));

    join_two_walls_in_document(&mut scene, w1, w2, None, None, None).unwrap();
    let w1_axis = get_wall_vertices(&scene, w1);
    let w1_derived_before = wall_derived_layer_vertices(&scene, w1);
    let w2_derived_before = wall_derived_layer_vertices(&scene, w2);
    assert!(!w1_derived_before.is_empty());
    assert!(!w2_derived_before.is_empty());
    let w2_corner_before: Vec<_> = w2_derived_before
        .iter()
        .flatten()
        .filter(|p| (p.0 - 10.0).abs() < 2.0 && (p.1 - 0.0).abs() < 2.0)
        .copied()
        .collect();
    assert!(!w2_corner_before.is_empty(), "W2 corner at (10,0) must exist");

    join_two_walls_in_document(&mut scene, w2, w3, None, None, None).unwrap();
    let w1_axis_after = get_wall_vertices(&scene, w1);
    assert_eq!(w1_axis, w1_axis_after, "W1 axis must be invariant when W2 connects to W3");

    let w1_derived_after = wall_derived_layer_vertices(&scene, w1);
    assert_eq!(
        w1_derived_before, w1_derived_after,
        "W1 derived geometry at first corner must be invariant when W2-W3 join occurs"
    );

    let w2_derived_after = wall_derived_layer_vertices(&scene, w2);
    let w2_corner_after: Vec<_> = w2_derived_after
        .iter()
        .flatten()
        .filter(|p| (p.0 - 10.0).abs() < 2.0 && (p.1 - 0.0).abs() < 2.0)
        .copied()
        .collect();
    assert_points_match(&w2_corner_before, &w2_corner_after, 1e-4);

    let peers_w2 = engine::owner_index::peers_of(&scene.document, w2);
    assert!(peers_w2.contains(&w1));
    assert!(peers_w2.contains(&w3));
}

#[test]
fn scene_cascade_l_corner_then_t_junction_preserves_l_corner_contour() {
    let mut scene = Scene::new();
    fn add_w(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64), layers: &[WallLayer]) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }

    let w1_layers = vec![
        wl("Plaster_In", 0.02, "Finish"),
        wl("Concrete", 0.20, "Structural"),
        wl("Insulation", 0.10, "Insulation"),
    ];
    let w2_layers = vec![
        wl("Plaster_In", 0.02, "Finish"),
        wl("Concrete", 0.20, "Structural"),
        wl("Insulation", 0.10, "Insulation"),
    ];
    let w3_layers = vec![
        wl("Concrete", 0.15, "Structural"),
    ];

    // Wall 1: (0,0) -> (20,0) (through wall along X)
    // Wall 2: (0,0) -> (0,10) (L-corner with Wall 1 at (0,0))
    let w1 = add_w(&mut scene, (0.0, 0.0), (20.0, 0.0), &w1_layers);
    let w2 = add_w(&mut scene, (0.0, 0.0), (0.0, 10.0), &w2_layers);

    let (k1, _) = join_two_walls_in_document(&mut scene, w1, w2, None, None, None).unwrap();
    assert_eq!(k1, JoinKind::L);

    let w2_polys_before = wall_derived_layer_vertices(&scene, w2);
    let w1_polys_before = wall_derived_layer_vertices(&scene, w1);
    assert!(!w2_polys_before.is_empty());
    assert!(!w1_polys_before.is_empty());

    // Wall 3: (10,0) -> (10,10) (Stem wall hitting W1 at x=10)
    let w3 = add_w(&mut scene, (10.0, 0.0), (10.0, 10.0), &w3_layers);
    let (k2, _) = join_two_walls_in_document(&mut scene, w1, w3, None, None, None).unwrap();
    assert_eq!(k2, JoinKind::T);

    // Wall 2 at the L-corner at (0,0) must be 100% untouched
    let w2_polys_after = wall_derived_layer_vertices(&scene, w2);
    assert_eq!(w2_polys_before, w2_polys_after, "W2 derived polylines at L corner must be identical");

    // Wall 1 must keep its peer links to both W2 and W3
    let peers_w1 = engine::owner_index::peers_of(&scene.document, w1);
    assert!(peers_w1.contains(&w2), "W1 must keep peer link to W2");
    assert!(peers_w1.contains(&w3), "W1 must have peer link to W3");

    // Wall 1's derived footprint at x=0 (the L-corner) must be preserved
    let w1_polys_after = wall_derived_layer_vertices(&scene, w1);
    let w1_before_corner = w1_polys_before.iter().flatten().filter(|p| p.0 < 5.0).collect::<Vec<_>>();
    let w1_after_corner = w1_polys_after.iter().flatten().filter(|p| p.0 < 5.0).collect::<Vec<_>>();
    assert_eq!(w1_before_corner.len(), w1_after_corner.len());
    for (b, a) in w1_before_corner.iter().zip(w1_after_corner.iter()) {
        assert!((b.0 - a.0).abs() < 1e-4);
        assert!((b.1 - a.1).abs() < 1e-4);
    }
}
