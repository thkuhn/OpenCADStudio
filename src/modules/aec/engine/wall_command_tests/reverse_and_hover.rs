use super::*;

#[test]
fn reverse_wall_preserves_footprint_and_flips_layer_side_assignment() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    // Concrete 0.2 + Insulation 0.05, total 0.25 → y ∈ [-0.125, 0.125]
    regenerate_wall_representation(&mut scene, wall, None).expect("regen");

    let axis_before = get_wall_vertices(&scene, wall);
    let layers_before = wall_from_entity(scene.document.get_entity(wall).unwrap())
        .unwrap()
        .layers
        .clone();
    let (ymin_before, ymax_before) = wall_derived_y_bounds(&scene, wall);

    // Contour of first layer (Concrete) should sit on the more-negative side.
    let entity = scene.document.get_entity(wall).unwrap().clone();
    let contours_before = wall_layer_contour_polylines(&entity, &layers_before);
    let concrete_y_before = contours_before[0]
        .0
        .iter()
        .chain(contours_before[0].1.iter())
        .map(|(_, y)| *y)
        .fold(f64::INFINITY, f64::min);

    reverse_wall_in_document(&mut scene,  wall,  None,  None,  None).expect("reverse");

    let axis_after = get_wall_vertices(&scene, wall);
    assert_eq!(
        axis_after,
        axis_before.iter().rev().copied().collect::<Vec<_>>(),
        "axis vertices must reverse order"
    );

    let layers_after = wall_from_entity(scene.document.get_entity(wall).unwrap())
        .unwrap()
        .layers
        .clone();
    assert_eq!(layers_after.len(), layers_before.len());
    // The stored layer list itself is untouched by reverse; the axis
    // direction flip alone is what relocates each material.
    assert_eq!(layers_after[0].material, layers_before[0].material);
    assert_eq!(
        layers_after.last().unwrap().material,
        layers_before.last().unwrap().material
    );

    let (ymin_after, ymax_after) = wall_derived_y_bounds(&scene, wall);
    assert!(
        (ymin_before - ymin_after).abs() < 1e-6 && (ymax_before - ymax_after).abs() < 1e-6,
        "outer footprint Y bounds must stay identical: before=({ymin_before},{ymax_before}) after=({ymin_after},{ymax_after})"
    );

    // Materials must visibly swap sides: Concrete (still layers[0]) now
    // sits on the more-positive side, since the axis direction flip
    // inverted the offset normal used to place it.
    let entity_after = scene.document.get_entity(wall).unwrap().clone();
    let contours_after = wall_layer_contour_polylines(&entity_after, &layers_after);
    let concrete_y_after = contours_after[0]
        .0
        .iter()
        .chain(contours_after[0].1.iter())
        .map(|(_, y)| *y)
        .fold(f64::INFINITY, f64::min);
    assert!(
        (concrete_y_before - concrete_y_after).abs() > 1e-6,
        "Concrete must move to the opposite absolute side after reverse: before={concrete_y_before} after={concrete_y_after}"
    );
}

#[test]
fn reverse_wall_keeps_joined_corner_intersection() {
    let mut scene = Scene::new();
    // Wall A: (0,0)->(5,0). Wall B: (5,0)->(5,5) L-corner.
    let wall_a = add_multi_layer_wall(&mut scene);

    // Approach the L corner from above so join_wall_axes must extend B.
    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(5.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = vec![wl("Concrete", 0.2, "Structural")];
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values =
        wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    regenerate_wall_representation(&mut scene, wall_a, None).expect("regen A");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("regen B");
    let (_kind, _) = join_two_walls_in_document(&mut scene,  wall_a,  wall_b,  None,  None,  None).expect("join");

    let axis_a_before = get_wall_vertices(&scene, wall_a);
    let axis_b_before = get_wall_vertices(&scene, wall_b);
    // Record the shared corner (any vertex of A that coincides with B).
    let corner = axis_a_before
        .iter()
        .find(|pa| axis_b_before.iter().any(|pb| pa.distance(*pb) < 1e-6))
        .copied()
        .expect("joined walls must share a corner before reverse");

    reverse_wall_in_document(&mut scene,  wall_a,  None,  None,  None).expect("reverse A");

    let axis_a = get_wall_vertices(&scene, wall_a);
    let axis_b = get_wall_vertices(&scene, wall_b);
    // After reverse + auto-join, the axes must still share an intersection
    // (at the original corner or a re-joined equivalent).
    let a_has = axis_a.iter().any(|p| p.distance(corner) < 1e-4);
    let b_has = axis_b.iter().any(|p| p.distance(corner) < 1e-4);
    let share = axis_a
        .iter()
        .any(|pa| axis_b.iter().any(|pb| pa.distance(*pb) < 1e-4));
    assert!(
        (a_has && b_has) || share,
        "joined corner must remain correct after reverse; corner={corner:?} A={axis_a:?} B={axis_b:?}"
    );
}

/// Helper: LwPolyline layer footprints for a wall package.
pub(super) fn wall_closed_footprints(scene: &Scene, h: Handle) -> Vec<Vec<(f64, f64)>> {
    let wall = wall_from_entity(scene.document.get_entity(h).unwrap()).unwrap();
    wall.derived_handles
        .iter()
        .filter_map(|dh| match scene.document.get_entity(*dh) {
            Some(EntityType::LwPolyline(pl)) => Some(
                pl.vertices
                    .iter()
                    .map(|v| (v.location.x, v.location.y))
                    .collect(),
            ),
            _ => None,
        })
        .collect()
}

/// Reversing a wall that is *already* part of a stable L-join must
/// re-detect the join (axes already coincident — endpoints don't move)
/// and rebuild *both* walls' derived geometry with true miter corners,
/// not plain rectangular end-caps.
#[test]
fn reverse_already_joined_wall_rebuilds_mitered_geometry_on_both() {
    let mut scene = Scene::new();
    // Single-layer equal walls so miter matching is unambiguous.
    // A: (0,0)->(5,0). B approaches from above near (5,0).
    let mut pl_a = LwPolyline::new();
    pl_a.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl_a.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut ent_a = EntityType::LwPolyline(pl_a);
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut rec_a = ExtendedDataRecord::new(AEC_APPID);
    rec_a.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent_a.common_mut().extended_data.add_record(rec_a);
    let wall_a = scene.add_entity(ent_a);

    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(5.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
    let mut ent_b = EntityType::LwPolyline(pl_b);
    let mut rec_b = ExtendedDataRecord::new(AEC_APPID);
    rec_b.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent_b.common_mut().extended_data.add_record(rec_b);
    let wall_b = scene.add_entity(ent_b);

    regenerate_wall_representation(&mut scene, wall_a, None).expect("regen A");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("regen B");
    let (kind, _) = join_two_walls_in_document(&mut scene,  wall_a,  wall_b,  None,  None,  None).expect("join");
    assert_eq!(kind, JoinKind::L);

    let axis_a_joined = get_wall_vertices(&scene, wall_a);
    let axis_b_joined = get_wall_vertices(&scene, wall_b);
    let corner = axis_a_joined
        .iter()
        .find(|pa| axis_b_joined.iter().any(|pb| pa.distance(*pb) < 1e-6))
        .copied()
        .expect("joined walls share a corner");

    // Capture pre-reverse miter corners (equal 0.2 walls at corner (5,0):
    // (5.1, -0.1) and (4.9, 0.1)).
    let fps_a_before = wall_closed_footprints(&scene, wall_a);
    let fps_b_before = wall_closed_footprints(&scene, wall_b);
    let c1 = (corner.x + 0.1, corner.y - 0.1);
    let c2 = (corner.x - 0.1, corner.y + 0.1);
    let has = |fp: &[(f64, f64)], p: (f64, f64)| {
        fp.iter()
            .any(|(x, y)| (*x - p.0).abs() < 1e-6 && (*y - p.1).abs() < 1e-6)
    };
    assert!(
        fps_a_before.iter().any(|fp| has(fp, c1) && has(fp, c2)),
        "precondition: A must be mitered before reverse, got {fps_a_before:?}"
    );
    assert!(
        fps_b_before.iter().any(|fp| has(fp, c1) && has(fp, c2)),
        "precondition: B must be mitered before reverse, got {fps_b_before:?}"
    );

    // Reverse the *already-joined* wall A. This is the live-app scenario:
    // join happened earlier; reverse must re-join without relying on
    // endpoints moving.
    let touched = reverse_wall_in_document(&mut scene,  wall_a,  None,  None,  None).expect("reverse A");

    // Both packages must be in the touched set (B's derived geometry is
    // regenerated too, not only A's axis/XDATA).
    assert!(
        touched.contains(&wall_a),
        "touched must include reversed wall A"
    );
    assert!(
        touched.contains(&wall_b),
        "touched must include neighbour B after re-join, got {touched:?}"
    );
    let v2_b = wall_from_entity(scene.document.get_entity(wall_b).unwrap()).unwrap();
    for d in &v2_b.derived_handles {
        assert!(
            touched.contains(d),
            "B derived handle {} must be bumped after reverse+rejoin",
            d.value()
        );
    }

    let axis_a = get_wall_vertices(&scene, wall_a);
    let axis_b = get_wall_vertices(&scene, wall_b);
    assert!(
        axis_a.iter().any(|p| p.distance(corner) < 1e-4)
            && axis_b.iter().any(|p| p.distance(corner) < 1e-4),
        "corner must stay put after reverse; corner={corner:?} A={axis_a:?} B={axis_b:?}"
    );
    // Axis order flipped on A; the join corner vertex is now at the
    // opposite index, but its *position* is unchanged.
    assert!(
        axis_a.first().unwrap().distance(corner) < 1e-4
            || axis_a.last().unwrap().distance(corner) < 1e-4,
        "A's join endpoint still at corner after reverse, A={axis_a:?}"
    );

    let fps_a = wall_closed_footprints(&scene, wall_a);
    let fps_b = wall_closed_footprints(&scene, wall_b);
    assert!(
        fps_a.iter().any(|fp| has(fp, c1) && has(fp, c2)),
        "A must keep mitered (non-separator) corner after reverse, got {fps_a:?}"
    );
    assert!(
        fps_b.iter().any(|fp| has(fp, c1) && has(fp, c2)),
        "B must keep mitered corner after neighbour reverse, got {fps_b:?}"
    );
}

/// Absolute world-space layer center offsets must mirror (negate) for
/// an asymmetric 3-layer wall (different thicknesses and gaps) after a
/// direction reverse — the stored layer list itself is untouched, only
/// the axis-direction flip relocates each material to the opposite side.
#[test]
fn reverse_wall_preserves_three_layer_world_centers() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut entity = EntityType::LwPolyline(pl);
    // Asymmetric stack: Brick 0.1, gap 0.02, Insulation 0.05, gap 0.01, Concrete 0.2
    let mut brick = wl("Brick", 0.1, "Finish");
    brick.axis_offset = 0.0;
    let mut insulation = wl("Insulation", 0.05, "Insulation");
    insulation.axis_offset = 0.02;
    let mut concrete = wl("Concrete", 0.2, "Structural");
    concrete.axis_offset = 0.01;
    let layers = vec![brick, insulation, concrete];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("s3", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let wall = scene.add_entity(entity);

    regenerate_wall_representation(&mut scene, wall, None).expect("regen");

    let entity_before = scene.document.get_entity(wall).unwrap().clone();
    let layers_before = wall_from_entity(&entity_before).unwrap().layers.clone();
    let contours_before = wall_layer_contour_polylines(&entity_before, &layers_before);

    // Per-material world center offset along the axis normal (Y for a
    // horizontal wall at Y=0): mean of the two boundary Y values.
    let centers_before: Vec<(String, f64)> = layers_before
        .iter()
        .zip(contours_before.iter())
        .map(|(layer, (b1, b2))| {
            let y1 = b1.iter().map(|(_, y)| *y).sum::<f64>() / b1.len() as f64;
            let y2 = b2.iter().map(|(_, y)| *y).sum::<f64>() / b2.len() as f64;
            (layer.material.clone(), 0.5 * (y1 + y2))
        })
        .collect();

    reverse_wall_in_document(&mut scene,  wall,  None,  None,  None).expect("reverse");

    let entity_after = scene.document.get_entity(wall).unwrap().clone();
    let layers_after = wall_from_entity(&entity_after).unwrap().layers.clone();
    let contours_after = wall_layer_contour_polylines(&entity_after, &layers_after);
    assert_eq!(layers_after.len(), 3);
    // The stored layer list order is untouched by reverse.
    assert_eq!(layers_after[0].material, "Brick");
    assert_eq!(layers_after[1].material, "Insulation");
    assert_eq!(layers_after[2].material, "Concrete");

    let centers_after: std::collections::HashMap<String, f64> = layers_after
        .iter()
        .zip(contours_after.iter())
        .map(|(layer, (b1, b2))| {
            let y1 = b1.iter().map(|(_, y)| *y).sum::<f64>() / b1.len() as f64;
            let y2 = b2.iter().map(|(_, y)| *y).sum::<f64>() / b2.len() as f64;
            (layer.material.clone(), 0.5 * (y1 + y2))
        })
        .collect();

    for (mat, c_before) in &centers_before {
        let c_after = centers_after
            .get(mat)
            .unwrap_or_else(|| panic!("material {mat} missing after reverse"));
        // The axis-direction flip inverts the offset normal, so every
        // material's world-space center must mirror (negate) around the
        // axis line rather than stay put.
        assert!(
            (c_before + c_after).abs() < 1e-9,
            "world center of {mat} must mirror after reverse: before={c_before} after={c_after}"
        );
    }
}

#[test]
fn wall_join_hover_highlight_during_both_picks() {
    let cmd = WallJoinCommand::new();
    assert!(
        cmd.entity_pick_highlights_hover(),
        "highlight during first-wall pick"
    );
    let mut cmd = WallJoinCommand::new();
    let _ = cmd.on_entity_pick(Handle::new(1), DVec3::ZERO);
    assert!(
        cmd.entity_pick_highlights_hover(),
        "highlight while awaiting second wall"
    );
}

#[test]
fn wall_extend_hover_highlight_during_both_picks() {
    let cmd = WallExtendCommand::new();
    assert!(
        cmd.entity_pick_highlights_hover(),
        "highlight during source-wall pick"
    );
    let mut cmd = WallExtendCommand::new();
    let _ = cmd.on_entity_pick(Handle::new(1), DVec3::ZERO);
    assert!(
        cmd.entity_pick_highlights_hover(),
        "highlight while awaiting target"
    );
}

#[test]
fn formula_layer_resolution_feeds_identical_geometry_for_fixed_styles() {
    // Fixed-only style must produce the same contour geometry through the
    // formula-aware resolver as through a hand-built WallLayer stack.
    let lib = engine::library::seed_default_library();
    let style_id = "style_insulated_ext";
    let resolved = resolve_wall_style_layers(&lib, style_id, None).expect("resolve");
    assert_eq!(resolved.len(), 4);
    assert!((resolved[0].thickness - 0.015).abs() < 1e-12);
    assert!((resolved[1].thickness - 0.175).abs() < 1e-12);
    assert!((resolved[2].thickness - 0.14).abs() < 1e-12);
    assert!((resolved[3].thickness - 0.015).abs() < 1e-12);

    let centerline = vec![(0.0, 0.0), (5.0, 0.0)];
    let layer_data: Vec<(f64, f64)> = resolved
        .iter()
        .map(|l| (l.thickness, l.axis_offset))
        .collect();
    let contours = engine::contour::layer_contours(&centerline, &layer_data);
    assert_eq!(contours.len(), 4);

    // Hand-built equivalent with the same centered axis offsets as the seed style.
    let manual = vec![
        (0.015, -0.1725),
        (0.175, -0.1575),
        (0.14, 0.0175),
        (0.015, 0.1575),
    ];
    let manual_contours = engine::contour::layer_contours(&centerline, &manual);
    assert_eq!(contours.len(), manual_contours.len());
    for (a, b) in contours.iter().zip(manual_contours.iter()) {
        assert_eq!(a.0.len(), b.0.len());
        for (p1, p2) in a.0.iter().zip(b.0.iter()) {
            assert!((p1.0 - p2.0).abs() < 1e-9 && (p1.1 - p2.1).abs() < 1e-9);
        }
        for (p1, p2) in a.1.iter().zip(b.1.iter()) {
            assert!((p1.0 - p2.0).abs() < 1e-9 && (p1.1 - p2.1).abs() < 1e-9);
        }
    }
}

#[test]
fn bb_formula_style_changes_resolved_thickness_with_base_width() {
    let mut lib = StyleLibrary::empty();
    lib.materials.push(Material::new(
        "mat_a".into(),
        "A".into(),
        "SOLID".into(),
        0xFFFFFF,
        "Continuous".into(),
    ));
    lib.upsert_wall_style(WallStyle {
        style: Style {
            id: "style_bb".into(),
            name: "BB half".into(),
            category: "Wall".into(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![
            Layer {
                material_id: "mat_a".into(),
                thickness: LayerValue::Fixed(0.1),
                function: LayerFunction::Structural,
                axis_offset: LayerValue::Fixed(0.0),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: None,
            layer_id: uuid::Uuid::new_v4(),
            },
            Layer {
                material_id: "mat_a".into(),
                thickness: LayerValue::Formula("BB * 0.5".into()),
                function: LayerFunction::Insulation,
                axis_offset: LayerValue::Fixed(0.0),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: None,
            layer_id: uuid::Uuid::new_v4(),
            },
        ],
    default_profile: None,
    });

    let r1 = resolve_wall_style_layers(&lib, "style_bb", Some(0.4)).unwrap();
    let r2 = resolve_wall_style_layers(&lib, "style_bb", Some(0.8)).unwrap();
    assert!((r1[1].thickness - 0.2).abs() < 1e-12);
    assert!((r2[1].thickness - 0.4).abs() < 1e-12);

    let centerline = vec![(0.0, 0.0), (3.0, 0.0)];
    let c1 = engine::contour::layer_contours(
        &centerline,
        &r1.iter().map(|l| (l.thickness, l.axis_offset)).collect::<Vec<_>>(),
    );
    let c2 = engine::contour::layer_contours(
        &centerline,
        &r2.iter().map(|l| (l.thickness, l.axis_offset)).collect::<Vec<_>>(),
    );
    // Different BB must produce different outer extents.
    let y_max = |cs: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)]| {
        cs.iter()
            .flat_map(|(a, b)| a.iter().chain(b.iter()))
            .map(|(_, y)| y.abs())
            .fold(0.0_f64, f64::max)
    };
    assert!(
        (y_max(&c1) - y_max(&c2)).abs() > 1e-6,
        "formula BB must affect geometry extents"
    );
}

#[test]
fn invalid_formula_style_falls_back_without_panic() {
    let mut lib = StyleLibrary::empty();
    lib.materials.push(Material::new(
        "mat_a".into(),
        "A".into(),
        "SOLID".into(),
        0xFFFFFF,
        "Continuous".into(),
    ));
    lib.upsert_wall_style(WallStyle {
        style: Style {
            id: "style_bad".into(),
            name: "Bad formula".into(),
            category: "Wall".into(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![Layer {
            material_id: "mat_a".into(),
            thickness: LayerValue::Formula("NOT_A_VAR / 0".into()),
            function: LayerFunction::Structural,
            axis_offset: LayerValue::Fixed(0.0),
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            role_tag: None,
        layer_id: uuid::Uuid::new_v4(),
        }],
    default_profile: None,
    });

    let resolved = resolve_wall_style_layers(&lib, "style_bad", Some(0.3)).unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].thickness, 0.0);
}

#[test]
fn join_and_delete_maintain_symmetric_peer_links() {
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let add_wall = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    };
    let ha = add_wall(&mut scene, (0.0, 0.0), (5.0, 0.0));
    let hb = add_wall(&mut scene, (0.0, 0.0), (0.0, 5.0));
    join_two_walls_in_document(&mut scene,  ha,  hb,  None,  None,  None).expect("L join");
    assert_eq!(engine::owner_index::peers_of(&scene.document, ha), vec![hb]);
    assert_eq!(engine::owner_index::peers_of(&scene.document, hb), vec![ha]);

    // Snapshot peer XDATA then unlink (simulates undo of join / disconnect).
    let peers_before = engine::owner_index::peers_of(&scene.document, ha);
    assert_eq!(peers_before, vec![hb]);
    engine::owner_index::unlink_peers(&mut scene.document, ha, hb);
    assert!(engine::owner_index::peers_of(&scene.document, ha).is_empty());
    assert!(engine::owner_index::peers_of(&scene.document, hb).is_empty());
    // re-join
    join_two_walls_in_document(&mut scene,  ha,  hb,  None,  None,  None).expect("rejoin");

    let hc = add_wall(&mut scene, (2.5, -3.0), (2.5, 0.0));
    join_two_walls_in_document(&mut scene,  ha,  hc,  None,  None,  None).expect("T join");
    let mut peers_a = engine::owner_index::peers_of(&scene.document, ha);
    peers_a.sort_by_key(|h| h.value());
    let mut expected = vec![hb, hc];
    expected.sort_by_key(|h| h.value());
    assert_eq!(peers_a, expected);

    // Deleting ha clears it from peers.
    unlink_all_wall_peers(&mut scene, ha);
    assert!(engine::owner_index::peers_of(&scene.document, ha).is_empty());
    assert!(!engine::owner_index::peers_of(&scene.document, hb).contains(&ha));
    assert!(!engine::owner_index::peers_of(&scene.document, hc).contains(&ha));
}

#[test]
fn storey_membership_tracks_add_remove_and_reassign() {
    let mut scene = Scene::new();
    let s0 = ensure_storey_entity(&mut scene, 0, Some(&Storey::new("L0", 0.0, 3.0)));
    let s1 = ensure_storey_entity(&mut scene, 1, Some(&Storey::new("L1", 3.0, 3.0)));
    assert!(walls_for_storey(&scene, s0).is_empty());
    assert!(walls_for_storey(&scene, s1).is_empty());

    let w1 = add_multi_layer_wall(&mut scene);
    let w2 = {
        // second wall, same geometry template
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 1.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 1.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    };

    register_wall_in_storey(&mut scene, w1);
    register_wall_in_storey(&mut scene, w2);
    let mut members0 = walls_for_storey(&scene, s0);
    members0.sort_by_key(|h| h.value());
    let mut expected = vec![w1, w2];
    expected.sort_by_key(|h| h.value());
    assert_eq!(members0, expected);
    assert!(walls_for_storey(&scene, s1).is_empty());

    assert!(set_wall_storey(&mut scene, w1, 1));
    assert_eq!(walls_for_storey(&scene, s0), vec![w2]);
    assert_eq!(walls_for_storey(&scene, s1), vec![w1]);
    let wall = wall_from_entity(scene.document.get_entity(w1).unwrap()).unwrap();
    assert_eq!(wall.storey_id, 1);

    unregister_wall_from_storey(&mut scene, w2);
    assert!(walls_for_storey(&scene, s0).is_empty());
    assert_eq!(walls_for_storey(&scene, s1), vec![w1]);

    // erase-path helper clears remaining membership
    unregister_walls_from_storeys(&mut scene, &[w1]);
    assert!(walls_for_storey(&scene, s1).is_empty());
}

#[test]
fn placing_and_removing_opening_keeps_host_child_handles() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    assert!(engine::owner_index::children_of(&scene.document, wall).is_empty());
    assert!(openings_for_host_wall(&scene, wall).is_empty());

    let (o1, _) = place_wall_opening(
        &mut scene, 
        wall, 
        DVec3::new(1.5, 0.0, 0.0), 
        engine::openings::OpeningKind::Window, 
    None,  None,  None)
    .expect("place window");
    let (o2, _) = place_wall_opening(
        &mut scene, 
        wall, 
        DVec3::new(3.5, 0.0, 0.0), 
        engine::openings::OpeningKind::Door, 
    None,  None,  None)
    .expect("place door");

    let children = engine::owner_index::children_of(&scene.document, wall);
    assert_eq!(children, vec![o1, o2]);

    let openings = openings_for_host_wall(&scene, wall);
    assert_eq!(openings.len(), 2);
    assert!(openings.iter().any(|o| o.handle == o1 && o.kind == engine::openings::OpeningKind::Window));
    assert!(openings.iter().any(|o| o.handle == o2 && o.kind == engine::openings::OpeningKind::Door));
    // host_wall XDATA still written on the opening entity
    let o1_ent = scene.document.get_entity(o1).unwrap();
    let parsed = opening_from_entity(o1_ent, o1).unwrap();
    assert_eq!(parsed.host_wall, wall);

    remove_wall_opening(&mut scene, o1, None).expect("remove window");
    assert_eq!(
        engine::owner_index::children_of(&scene.document, wall),
        vec![o2]
    );
    let remaining = openings_for_host_wall(&scene, wall);
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].handle, o2);
    assert!(scene.document.get_entity(o1).is_none());

    remove_wall_opening(&mut scene, o2, None).expect("remove door");
    assert!(engine::owner_index::children_of(&scene.document, wall).is_empty());
    assert!(openings_for_host_wall(&scene, wall).is_empty());
}

#[test]
fn removing_opening_with_display_rules_preserves_plan_dependent_representation() {
    use crate::modules::aec::engine::display_component::{RepresentationMode, WallComponentSlot};
    use crate::modules::aec::engine::library::build_effective_rule_set;
    use crate::modules::aec::engine::plan_view::DisplayConfig;

    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);

    let (op, _) = place_wall_opening(
        &mut scene,
        wall,
        DVec3::new(2.0, 0.0, 0.0),
        engine::openings::OpeningKind::Window,
        None,
        None,
        None,
    )
    .expect("place window");

    // Build 3D-only display rules where 2D layers and contours are hidden
    let cfg = DisplayConfig::new(
        "3D Model".to_string(),
        "Visualisierung".to_string(),
        crate::modules::aec::engine::plan_view::PlanningStage::Design,
        crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
    );
    let rules_3d = build_effective_rule_set(&cfg, None, Some(RepresentationMode::ThreeD));
    assert!(!rules_3d.is_visible(WallComponentSlot::Layers2D));
    assert!(rules_3d.is_visible(WallComponentSlot::Solid3D));

    // Remove opening using the active display rules
    let touched = remove_wall_opening_with_rules(&mut scene, op, None, Some(&rules_3d))
        .expect("remove opening with rules");
    assert!(touched.contains(&wall));
    assert!(scene.document.get_entity(op).is_none());

    // Verify the regenerated wall only has 3D solid representation, no 2D layer hatch
    let wall_data = wall_from_entity(scene.document.get_entity(wall).unwrap()).unwrap();
    let hatches = wall_data
        .derived_handles
        .iter()
        .filter_map(|h| scene.document.get_entity(*h))
        .filter(|e| matches!(e, EntityType::Hatch(_)))
        .count();
    assert_eq!(hatches, 0, "3D display rules must not generate 2D layer hatches after opening removal");
}

#[test]
fn expand_with_wall_derived_handles_expands_opening_child_to_opening_package_without_host_wall() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    let (op, _) = place_wall_opening(
        &mut scene,
        wall,
        DVec3::new(2.0, 0.0, 0.0),
        engine::openings::OpeningKind::Window,
        None,
        None,
        None,
    )
    .expect("place window");

    let children = engine::opening_display::collect_opening_display_children(&scene, op);
    assert!(!children.is_empty(), "opening should have display children");

    // Pass only the opening child entity to expand_with_wall_derived_handles
    let mut handles = vec![children[0]];
    expand_with_wall_derived_handles(&scene, &mut handles);

    assert!(handles.contains(&op), "should resolve child to opening owner");
    for child in &children {
        assert!(handles.contains(child), "should include all opening children");
    }
    assert!(!handles.contains(&wall), "should NOT include the host wall");
}

#[test]
fn interactive_delete_opening_respects_active_display_config() {
    use crate::app::{Message, OpenCADStudio};

    let mut app = OpenCADStudio::new_for_test();
    let tab = app.active_tab;
    app.aec.aec_plan_library = Some(crate::modules::aec::engine::library::DisplayConfigLibrary::seed_default_configs());
    let wall = add_multi_layer_wall(&mut app.tabs[tab].scene);
    let (op, _) = place_wall_opening(
        &mut app.tabs[tab].scene,
        wall,
        DVec3::new(2.0, 0.0, 0.0),
        engine::openings::OpeningKind::Window,
        None,
        None,
        None,
    )
    .expect("place window");

    // Set active plan configuration to "3D Modell / Visualisierung"
    app.aec_set_active_display_config(tab, Some("3D Modell / Visualisierung".to_string()));
    app.apply_active_display_config_to_tab(tab);

    // Select the opening
    app.tabs[tab].scene.selected.clear();
    app.tabs[tab].scene.selected.insert(op);

    // Trigger interactive deletion
    let _ = app.update(Message::DeleteSelected);

    // Verify opening is erased and unlinked from wall
    assert!(app.tabs[tab].scene.document.get_entity(op).is_none(), "opening should be deleted");
    assert!(
        engine::owner_index::children_of(&app.tabs[tab].scene.document, wall).is_empty(),
        "opening should be unlinked from host wall"
    );

    // Verify host wall regenerated under 3D display rules (no 2D layer hatch entities)
    let wall_data = wall_from_entity(app.tabs[tab].scene.document.get_entity(wall).unwrap()).unwrap();
    let hatches = wall_data
        .derived_handles
        .iter()
        .filter_map(|h| app.tabs[tab].scene.document.get_entity(*h))
        .filter(|e| matches!(e, EntityType::Hatch(_)))
        .count();
    assert_eq!(hatches, 0, "3D display rules must not generate 2D layer hatches after interactive deletion");
}

#[test]
fn two_junctions_on_one_wall_are_handled_correctly() {
    let mut scene = Scene::new();
    // Wall 1: horizontal along Y=0 from X=0 to X=10.
    let mut pl1 = LwPolyline::new();
    pl1.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl1.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut ent1 = EntityType::LwPolyline(pl1);
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut rec1 = ExtendedDataRecord::new(AEC_APPID);
    rec1.values = wall_record("s1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent1.common_mut().extended_data.add_record(rec1);
    let w1 = scene.add_entity(ent1);
    regenerate_wall_representation(&mut scene, w1, None).expect("regen w1");

    // Junction A at (0,0): w1 + w2 + w3
    // w2: vertical from (0,0) to (0,5)
    let mut pl2 = LwPolyline::new();
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 5.0)));
    let mut ent2 = EntityType::LwPolyline(pl2);
    let mut rec2 = ExtendedDataRecord::new(AEC_APPID);
    rec2.values = wall_record("s2", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent2.common_mut().extended_data.add_record(rec2);
    let w2 = scene.add_entity(ent2);
    regenerate_wall_representation(&mut scene, w2, None).expect("regen w2");

    // w3: vertical from (0,0) to (0,-5)
    let mut pl3 = LwPolyline::new();
    pl3.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl3.add_vertex(LwVertex::new(Vector2::new(0.0, -5.0)));
    let mut ent3 = EntityType::LwPolyline(pl3);
    let mut rec3 = ExtendedDataRecord::new(AEC_APPID);
    rec3.values = wall_record("s3", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent3.common_mut().extended_data.add_record(rec3);
    let w3 = scene.add_entity(ent3);
    regenerate_wall_representation(&mut scene, w3, None).expect("regen w3");

    // Junction B at (10,0): w1 + w4 + w5
    // w4: vertical from (10,0) to (10,5)
    let mut pl4 = LwPolyline::new();
    pl4.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    pl4.add_vertex(LwVertex::new(Vector2::new(10.0, 5.0)));
    let mut ent4 = EntityType::LwPolyline(pl4);
    let mut rec4 = ExtendedDataRecord::new(AEC_APPID);
    rec4.values = wall_record("s4", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent4.common_mut().extended_data.add_record(rec4);
    let w4 = scene.add_entity(ent4);
    regenerate_wall_representation(&mut scene, w4, None).expect("regen w4");

    // w5: vertical from (10,0) to (10,-5)
    let mut pl5 = LwPolyline::new();
    pl5.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    pl5.add_vertex(LwVertex::new(Vector2::new(10.0, -5.0)));
    let mut ent5 = EntityType::LwPolyline(pl5);
    let mut rec5 = ExtendedDataRecord::new(AEC_APPID);
    rec5.values = wall_record("s5", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent5.common_mut().extended_data.add_record(rec5);
    let w5 = scene.add_entity(ent5);
    regenerate_wall_representation(&mut scene, w5, None).expect("regen w5");

    // Join Junction A.
    let junc_a_handles = vec![w1, w2, w3];
    let touched_a = join_junction_in_document(&mut scene,  &junc_a_handles,  None,  None,  None,  None).expect("join A");
    assert!(touched_a.contains(&w1));
    assert!(touched_a.contains(&w2));
    assert!(touched_a.contains(&w3));

    // Join Junction B.
    let junc_b_handles = vec![w1, w4, w5];
    let touched_b = join_junction_in_document(&mut scene,  &junc_b_handles,  None,  None,  None,  None).expect("join B");
    assert!(touched_b.contains(&w1));
    assert!(touched_b.contains(&w4));
    assert!(touched_b.contains(&w5));

    // Assert both junctions' participants have mitered footprints.
    for h in &[w1, w2, w3, w4, w5] {
        let wall = wall_from_entity(scene.document.get_entity(*h).unwrap()).unwrap();
        // N-way junction should produce derived handles for mitered layers.
        assert!(!wall.derived_handles.is_empty(), "wall {} should have mitered footprints", h.value());
    }

    // Assert peers_of is correct.
    let peers_w1 = engine::owner_index::peers_of(&scene.document, w1);
    assert!(peers_w1.contains(&w2));
    assert!(peers_w1.contains(&w3));
    assert!(peers_w1.contains(&w4));
    assert!(peers_w1.contains(&w5));
    assert_eq!(peers_w1.len(), 4);

    let peers_w2 = engine::owner_index::peers_of(&scene.document, w2);
    assert_eq!(peers_w2.len(), 2);
    assert!(peers_w2.contains(&w1));
    assert!(peers_w2.contains(&w3));

    // Test the safety guard: passing all handles at once should yield Ambiguous.
    let all_handles = vec![w1, w2, w3, w4, w5];
    let result = join_junction_in_document(&mut scene,  &all_handles,  None,  None,  None,  None);
    assert_eq!(result.err(), Some(JoinError::Ambiguous));
}
