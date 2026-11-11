use super::*;

#[test]
fn scene_cascade_l_at_end0_and_t_stem_at_end1_preserves_l_corner() {
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

    let layers_a = vec![
        wl("Plaster", 0.02, "Finish"),
        wl("Concrete", 0.20, "Structural"),
        wl("Insulation", 0.10, "Insulation"),
    ];
    let layers_b = layers_a.clone();
    let layers_c = vec![wl("Concrete", 0.24, "Structural"), wl("Insulation", 0.06, "Insulation")];

    // Wall B: (0, 10) -> (0, 0)
    // Wall A: (0, 0) -> (10, 0) - forms L corner at (0,0) (End 0 of Wall A)
    let w_b = add_w(&mut scene, (0.0, 10.0), (0.0, 0.0), &layers_b);
    let w_a = add_w(&mut scene, (0.0, 0.0), (10.0, 0.0), &layers_a);

    let (k1, _) = join_two_walls_in_document(&mut scene, w_b, w_a, None, None, None).unwrap();
    assert_eq!(k1, JoinKind::L);

    let wb_derived_before = wall_derived_layer_vertices(&scene, w_b);
    let wa_derived_before = wall_derived_layer_vertices(&scene, w_a);
    assert!(!wb_derived_before.is_empty());
    assert!(!wa_derived_before.is_empty());
    let wa_corner_before: Vec<_> = wa_derived_before
        .iter()
        .flatten()
        .filter(|p| p.0 < 2.0 && p.1.abs() < 2.0)
        .copied()
        .collect();
    assert!(!wa_corner_before.is_empty());

    // Wall C: (10, -5) -> (10, 5) - Through wall at x=10
    // Wall A joins Wall C at End 1 as a T-stem
    let w_c = add_w(&mut scene, (10.0, -5.0), (10.0, 5.0), &layers_c);
    let (k2, _) = join_two_walls_in_document(&mut scene, w_c, w_a, None, None, None).unwrap();
    assert_eq!(k2, JoinKind::T);

    // Wall B at corner (0,0) must be 100% invariant
    let wb_derived_after = wall_derived_layer_vertices(&scene, w_b);
    assert_eq!(wb_derived_before, wb_derived_after, "W_B at (0,0) must be invariant");

    // Wall A at End 0 L-corner must be preserved
    let wa_derived_after = wall_derived_layer_vertices(&scene, w_a);
    let wa_corner_after: Vec<_> = wa_derived_after
        .iter()
        .flatten()
        .filter(|p| p.0 < 2.0 && p.1.abs() < 2.0)
        .copied()
        .collect();
    assert_points_match(&wa_corner_before, &wa_corner_after, 1e-4);
}

#[test]
fn scene_cascade_closed_room_cycle_miters_all_four_corners() {
    let mut scene = Scene::new();
    fn add_w(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64)) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }

    let w1 = add_w(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let w2 = add_w(&mut scene, (10.0, 0.0), (10.0, 10.0));
    let w3 = add_w(&mut scene, (10.0, 10.0), (0.0, 10.0));
    let w4 = add_w(&mut scene, (0.0, 10.0), (0.0, 0.0));

    join_two_walls_in_document(&mut scene, w1, w2, None, None, None).unwrap();
    join_two_walls_in_document(&mut scene, w2, w3, None, None, None).unwrap();
    join_two_walls_in_document(&mut scene, w3, w4, None, None, None).unwrap();
    join_two_walls_in_document(&mut scene, w4, w1, None, None, None).unwrap();

    for &w in &[w1, w2, w3, w4] {
        let peers = engine::owner_index::peers_of(&scene.document, w);
        assert_eq!(peers.len(), 2, "each room wall must have exactly 2 adjacent peers");
        let polys = wall_derived_layer_vertices(&scene, w);
        assert!(!polys.is_empty(), "room wall must have derived layer footprints");
    }

    // Snapshot the opposite corner at (0,0) (shared by W4 and W1)
    let w4_polys_before = wall_derived_layer_vertices(&scene, w4);
    let w1_polys_before = wall_derived_layer_vertices(&scene, w1);

    // Now edit vertex at (10,10) to (12,10) on W2 and (12,10) on W3
    let mut ax2 = get_wall_vertices(&scene, w2);
    ax2[1] = DVec3::new(12.0, 10.0, 0.0);
    update_wall_vertices(&mut scene, w2, &ax2);

    let mut ax3 = get_wall_vertices(&scene, w3);
    ax3[0] = DVec3::new(12.0, 10.0, 0.0);
    update_wall_vertices(&mut scene, w3, &ax3);

    // Rebuild local network around W2-W3
    let touched = try_auto_join_nearby_walls(&mut scene, w2, None, None, None);
    assert!(!touched.is_empty());

    // Opposite wall W4 (from (0,10) to (0,0)) must be 100% invariant
    let w4_polys_after = wall_derived_layer_vertices(&scene, w4);
    assert_eq!(w4_polys_before, w4_polys_after, "W4 geometry at opposite wall must be invariant");

    // W1's corner at (0,0) (End 0) must retain its exact corner coordinates
    let w1_polys_after = wall_derived_layer_vertices(&scene, w1);
    assert_eq!(w1_polys_before.len(), w1_polys_after.len(), "W1 must retain same polygon count");
    for (poly_before, poly_after) in w1_polys_before.iter().zip(&w1_polys_after) {
        let end0_before: Vec<_> = poly_before.iter().filter(|(x, _)| *x < 2.0).collect();
        let end0_after: Vec<_> = poly_after.iter().filter(|(x, _)| *x < 2.0).collect();
        assert_eq!(end0_before, end0_after, "W1 corner geometry at (0,0) must be preserved");
    }

    // Verify moved corner at (12,10) is re-mitered
    let ax2_after = get_wall_vertices(&scene, w2);
    let ax3_after = get_wall_vertices(&scene, w3);
    assert!((ax2_after[1].x - 12.0).abs() < 1e-4 && (ax2_after[1].y - 10.0).abs() < 1e-4);
    assert!((ax3_after[0].x - 12.0).abs() < 1e-4 && (ax3_after[0].y - 10.0).abs() < 1e-4);
    let w2_polys_after = wall_derived_layer_vertices(&scene, w2);
    let w3_polys_after = wall_derived_layer_vertices(&scene, w3);
    // Expected miter points: intersection of the same-side offset lines of
    // W2 (10,0)->(12,10) and W3 (12,10)->(0,10), half thickness 0.1.
    let corner = (12.0_f64, 10.0_f64);
    let d2 = (2.0_f64 / 104.0_f64.sqrt(), 10.0_f64 / 104.0_f64.sqrt());
    let n2 = (-d2.1, d2.0);
    let d3 = (-1.0_f64, 0.0_f64);
    let n3 = (-d3.1, d3.0);
    let miter_pt = |s: f64| -> (f64, f64) {
        // line2: corner + s*0.1*n2 + t*d2 ; line3: corner + s*0.1*n3 + u*d3
        let p2 = (corner.0 + s * 0.1 * n2.0, corner.1 + s * 0.1 * n2.1);
        let p3 = (corner.0 + s * 0.1 * n3.0, corner.1 + s * 0.1 * n3.1);
        let det = d2.0 * (-d3.1) - d2.1 * (-d3.0);
        let t = ((p3.0 - p2.0) * (-d3.1) - (p3.1 - p2.1) * (-d3.0)) / det;
        (p2.0 + t * d2.0, p2.1 + t * d2.1)
    };
    let c1 = miter_pt(1.0);
    let c2 = miter_pt(-1.0);
    assert!(
        (c1.0 - c2.0).hypot(c1.1 - c2.1) > 0.1,
        "miter points must be distinct, got {c1:?} {c2:?}"
    );
    let has = |poly: &Vec<(f64, f64)>, p: (f64, f64)| {
        poly.iter().any(|q| (q.0 - p.0).abs() < 1e-4 && (q.1 - p.1).abs() < 1e-4)
    };
    assert!(
        has(&w2_polys_after.concat(), c1) && has(&w2_polys_after.concat(), c2),
        "W2 must share both miter vertices {c1:?} / {c2:?} at moved corner, got {w2_polys_after:?}"
    );
    assert!(
        has(&w3_polys_after.concat(), c1) && has(&w3_polys_after.concat(), c2),
        "W3 must share both miter vertices {c1:?} / {c2:?} at moved corner, got {w3_polys_after:?}"
    );
}

#[test]
fn scene_cascade_off_by_epsilon_endpoint_join_snaps_and_miters() {
    let mut scene = Scene::new();
    fn add_w(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64)) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }

    // W1 is drawn slightly off: ending at (10.05, 0.02)
    let w1 = add_w(&mut scene, (0.0, 0.0), (10.05, 0.02));
    let w2 = add_w(&mut scene, (10.0, 0.0), (10.0, 10.0));

    let (kind, touched) = join_two_walls_in_document(&mut scene, w1, w2, None, None, None).unwrap();
    assert_eq!(kind, JoinKind::L);
    assert!(touched.contains(&w1) && touched.contains(&w2));

    let ax1 = get_wall_vertices(&scene, w1);
    let ax2 = get_wall_vertices(&scene, w2);
    assert!((ax1.last().unwrap().x - ax2[0].x).hypot(ax1.last().unwrap().y - ax2[0].y) < 1e-4);

    let polys_w1 = wall_derived_layer_vertices(&scene, w1);
    let polys_w2 = wall_derived_layer_vertices(&scene, w2);
    assert!(!polys_w1.is_empty());
    assert!(!polys_w2.is_empty());
}

#[test]
fn scene_cascade_x_vs_l_near_threshold_03() {
    let mut scene = Scene::new();
    let snap_radius = crate::modules::aec::engine::join_ops::WALL_JOIN_SNAP_RADIUS;
    
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

    // L-corner: overshoot = 0.29 (inside snap radius 0.30)
    let offset_l = snap_radius - 0.01;
    let w1_l = add_w(&mut scene, (0.0, 0.0), (10.0 + offset_l, 0.0));
    let w2_l = add_w(&mut scene, (10.0, -offset_l), (10.0, 5.0));
    let (k_l, _) = join_two_walls_in_document(&mut scene, w1_l, w2_l, None, None, None).unwrap();
    assert_eq!(k_l, JoinKind::L, "Should be L-corner within snap radius");
    
    // X-crossing: overshoot = 0.31 (outside snap radius 0.30)
    let offset_x = snap_radius + 0.01;
    let ax_a = vec![DVec3::new(20.0, 0.0, 0.0), DVec3::new(30.0 + offset_x, 0.0, 0.0)];
    let ax_b = vec![DVec3::new(30.0, -offset_x, 0.0), DVec3::new(30.0, 5.0, 0.0)];
    let walls = vec![ax_a.as_slice(), ax_b.as_slice()];
    let junctions = join::detect_junctions(&walls, snap_radius);
    assert_eq!(junctions.len(), 1);
    assert!(
        junctions[0].participants.iter().all(|p| matches!(p.role, join::JunctionRole::Through(_))),
        "Both walls must be Through for an interior X-crossing outside snap radius"
    );

    // Same geometry through the document join path.
    let w1_x = add_w(&mut scene, (20.0, 0.0), (30.0 + offset_x, 0.0));
    let w2_x = add_w(&mut scene, (30.0, -offset_x), (30.0, 5.0));
    let (k_x, _) = join_two_walls_in_document(&mut scene, w1_x, w2_x, None, None, None).unwrap();
    assert_eq!(k_x, JoinKind::X, "overshoot beyond snap radius must be an X crossing, not L");
    let ax1 = get_wall_vertices(&scene, w1_x);
    let ax2 = get_wall_vertices(&scene, w2_x);
    assert!(
        (ax1.last().unwrap().x - (30.0 + offset_x)).abs() < 1e-6 && (ax1[0].x - 20.0).abs() < 1e-6,
        "axis A must not be truncated, got {ax1:?}"
    );
    assert!(
        (ax2[0].y + offset_x).abs() < 1e-6 && (ax2.last().unwrap().y - 5.0).abs() < 1e-6,
        "axis B must not be truncated, got {ax2:?}"
    );
}

#[test]
fn scene_cascade_opposite_end_override_is_preserved_when_joining_new_wall() {
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

    let layers_a = vec![wl("Concrete", 0.2, "Structural"), wl("Insulation", 0.05, "Insulation")];
    let layers_b = vec![wl("Concrete", 0.2, "Structural")];

    // Wall 0: (0, -5) -> (0, 0)
    // Wall 1: (0, 0) -> (10, 0)
    let w0 = add_w(&mut scene, (0.0, -5.0), (0.0, 0.0), &layers_b);
    let w1 = add_w(&mut scene, (0.0, 0.0), (10.0, 0.0), &layers_a);

    join_two_walls_in_document(&mut scene, w0, w1, None, None, None).unwrap();

    let w1_polys_default = wall_derived_layer_vertices(&scene, w1);
    let w1_end0_default: Vec<_> = w1_polys_default.iter().flatten().filter(|p| p.0 < 2.0).copied().collect();

    let override_data = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::NearFace),
        layer_pairs: Vec::new(),
        layer_gaps: Vec::new(),
    };
    write_junction_override(&mut scene, w1, 0, &override_data);
    regenerate_wall_representation(&mut scene, w1, None).unwrap();
    regenerate_wall_representation(&mut scene, w0, None).unwrap();

    let w0_polys_before = wall_derived_layer_vertices(&scene, w0);
    let w1_polys_overridden = wall_derived_layer_vertices(&scene, w1);
    let w1_end0_overridden: Vec<_> = w1_polys_overridden.iter().flatten().filter(|p| p.0 < 2.0).copied().collect();
    assert_ne!(
        w1_end0_default, w1_end0_overridden,
        "NearFace override must change W1 end-0 geometry compared to default miter"
    );

    // Wall 2: (10, 0) -> (10, 5) joins W1 at End 1
    let w2 = add_w(&mut scene, (10.0, 0.0), (10.0, 5.0), &layers_b);
    join_two_walls_in_document(&mut scene, w1, w2, None, None, None).unwrap();

    let ov_end0 = read_junction_override(&scene, w1, 0);
    assert!(ov_end0.is_some(), "Override at end 0 of W1 must survive join at end 1");
    assert_eq!(ov_end0.unwrap().default_style, Some(join::JoinOverrideStyle::NearFace));

    let w0_polys_after = wall_derived_layer_vertices(&scene, w0);
    assert_eq!(w0_polys_before, w0_polys_after, "W0 at end 0 corner must be invariant");

    let w1_polys_after = wall_derived_layer_vertices(&scene, w1);
    let w1_end0_after: Vec<_> = w1_polys_after.iter().flatten().filter(|p| p.0 < 2.0).copied().collect();
    assert_points_match(&w1_end0_overridden, &w1_end0_after, 1e-4);
}

#[test]
fn scene_cascade_through_span_gap_override_preserved_after_cascade() {
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
    let axis_2d: Vec<(f64, f64)> = get_wall_vertices(&scene, through)
        .iter()
        .map(|p| (p.x, p.y))
        .collect();

    // Baseline without a gap: the masonry is not interrupted.
    let cut_no_gap = find_through_span_cutout_footprints(&mut scene, through, &axis_2d);
    assert!(
        cut_no_gap
            .as_ref()
            .map(|c| c.get(1).and_then(|fp| fp.as_ref()).is_none())
            .unwrap_or(true),
        "precondition: masonry must not be interrupted without a gap, got {cut_no_gap:?}"
    );
    let loops_no_gap = wall_contour_loops(&scene, through);

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

    let split_pieces = |scene: &Scene| -> (bool, bool) {
        let loops = wall_contour_loops(scene, through);
        let left = loops.iter().any(|lp| {
            let min_x = lp.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
            let max_x = lp.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
            min_x < 0.1 && max_x < 4.95 && lp.iter().any(|p| p.1.abs() > 0.05 && p.1.abs() < 0.121)
        });
        let right = loops.iter().any(|lp| {
            let min_x = lp.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
            let max_x = lp.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
            min_x > 5.05 && max_x > 9.9 && lp.iter().any(|p| p.1.abs() > 0.05 && p.1.abs() < 0.121)
        });
        (left, right)
    };
    let cut = find_through_span_cutout_footprints(&mut scene, through, &axis_2d).expect("T cutout");
    assert!(
        cut.get(1).and_then(|fp| fp.as_ref()).is_some(),
        "gap must interrupt through masonry, got {cut:?}"
    );
    assert_eq!(split_pieces(&scene), (true, true), "masonry must be split around the stem");
    let loops_gap = wall_contour_loops(&scene, through);
    assert_ne!(loops_no_gap.len(), loops_gap.len(), "gap must change the through wall loops");
    let near_stem = |loops: &Vec<Vec<(f64, f64)>>| -> Vec<(f64, f64)> {
        loops.iter().flatten().filter(|p| (p.0 - 5.0).abs() < 1.5).copied().collect()
    };
    let notch_before = near_stem(&loops_gap);
    assert!(!notch_before.is_empty());
    let cut_before: Vec<(f64, f64)> = cut.iter().flatten().flatten().copied().filter(|p| p.0.is_finite()).collect();

    // Join a new wall at the OTHER end (end 0) of the through wall.
    let w3 = add_layered_wall(&mut scene, (0.0, 0.0), (0.0, 10.0), vec![wl("Concrete", 0.2, "Structural")]);
    let (k2, _) = join_two_walls_in_document(&mut scene, through, w3, None, None, None).unwrap();
    assert_eq!(k2, JoinKind::L);

    let ov_span = read_junction_override(&scene, through, THROUGH_SPAN_OVERRIDE_END);
    assert!(ov_span.is_some(), "span gap override must survive end-0 join");
    assert_eq!(ov_span.unwrap().layer_gaps.len(), 1);

    let axis_2d_after: Vec<(f64, f64)> = get_wall_vertices(&scene, through)
        .iter()
        .map(|p| (p.x, p.y))
        .collect();
    let cut_after = find_through_span_cutout_footprints(&mut scene, through, &axis_2d_after)
        .expect("T cutout after L join");
    assert!(
        cut_after.get(1).and_then(|fp| fp.as_ref()).is_some(),
        "masonry interruption must survive the end-0 join, got {cut_after:?}"
    );
    let cut_after_pts: Vec<(f64, f64)> = cut_after.iter().flatten().flatten().copied().filter(|p| p.0.is_finite()).collect();
    assert_points_match(&cut_before, &cut_after_pts, 1e-4);
    assert_eq!(split_pieces(&scene), (true, true), "split masonry pieces must survive");
    let loops_after = wall_contour_loops(&scene, through);
    assert_points_match(&notch_before, &near_stem(&loops_after), 1e-4);
}

#[test]
fn scene_collect_local_wall_network_bounds_cascade_depth() {
    let mut scene = Scene::new();
    fn add_w(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64)) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }

    // 5 chained walls: W1-W2-W3-W4-W5
    let w1 = add_w(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let w2 = add_w(&mut scene, (10.0, 0.0), (20.0, 0.0));
    let w3 = add_w(&mut scene, (20.0, 0.0), (30.0, 0.0));
    let w4 = add_w(&mut scene, (30.0, 0.0), (40.0, 0.0));
    let w5 = add_w(&mut scene, (40.0, 0.0), (50.0, 0.0));

    engine::owner_index::link_peers(&mut scene.document, w1, w2);
    engine::owner_index::link_peers(&mut scene.document, w2, w3);
    engine::owner_index::link_peers(&mut scene.document, w3, w4);
    engine::owner_index::link_peers(&mut scene.document, w4, w5);

    let cluster = collect_local_wall_network(&scene, &[w1], WALL_JOIN_SNAP_RADIUS);
    assert!(cluster.contains(&w1));
    assert!(cluster.contains(&w2));
    assert!(cluster.contains(&w3));
    assert!(!cluster.contains(&w4), "Cascade must be bounded to depth 2, not global BFS");
    assert!(!cluster.contains(&w5), "Cascade must be bounded to depth 2, not global BFS");
}

#[test]
fn scene_cascade_graph_visits_geometric_partner_not_distant_chain() {
    let mut scene = Scene::new();
    fn add_w(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64)) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }

    // Peer-linked chain W1–W5 plus an unlinked geometric partner at W1's
    // free end and a distant unrelated wall.
    let w1 = add_w(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let w2 = add_w(&mut scene, (10.0, 0.0), (10.0, 10.0));
    let w3 = add_w(&mut scene, (10.0, 10.0), (20.0, 10.0));
    let w4 = add_w(&mut scene, (20.0, 10.0), (30.0, 10.0));
    let w5 = add_w(&mut scene, (30.0, 10.0), (40.0, 10.0));
    let w_geo = add_w(&mut scene, (0.0, 0.0), (0.0, 10.0));
    let w_far = add_w(&mut scene, (100.0, 100.0), (110.0, 100.0));

    engine::owner_index::link_peers(&mut scene.document, w1, w2);
    engine::owner_index::link_peers(&mut scene.document, w2, w3);
    engine::owner_index::link_peers(&mut scene.document, w3, w4);
    engine::owner_index::link_peers(&mut scene.document, w4, w5);

    let cluster = collect_local_wall_network(&scene, &[w1], WALL_JOIN_SNAP_RADIUS);
    assert!(cluster.contains(&w1), "seed must be visited");
    assert!(cluster.contains(&w2), "peer partner must be visited");
    assert!(cluster.contains(&w3), "partner's opposite node must be visited");
    assert!(
        cluster.contains(&w_geo),
        "unlinked geometric partner at the seed end must be visited"
    );
    assert!(!cluster.contains(&w4), "depth-3 chain wall must not be visited");
    assert!(!cluster.contains(&w5), "distant chain wall must not be visited");
    assert!(!cluster.contains(&w_far), "unrelated distant wall must not be visited");

    for h in [w1, w2, w3, w4, w5, w_geo, w_far] {
        regenerate_wall_representation(&mut scene, h, None).expect("regen");
    }
    let w5_before = wall_derived_layer_vertices(&scene, w5);
    let far_before = wall_derived_layer_vertices(&scene, w_far);
    assert!(!w5_before.is_empty());
    assert!(!far_before.is_empty());

    let (k1, _) = join_two_walls_in_document(&mut scene, w1, w2, None, None, None).unwrap();
    assert_eq!(k1, JoinKind::L);
    let (k2, _) = join_two_walls_in_document(&mut scene, w2, w3, None, None, None).unwrap();
    assert_eq!(k2, JoinKind::L);

    assert_eq!(
        wall_derived_layer_vertices(&scene, w5),
        w5_before,
        "distant chain wall must not be regenerated by a local cascade"
    );
    assert_eq!(
        wall_derived_layer_vertices(&scene, w_far),
        far_before,
        "unrelated distant wall must not be regenerated by a local cascade"
    );

    // Live-path multi-end: W2 must carry real miters at both (10,0) and (10,10).
    let w2_pts: Vec<(f64, f64)> = wall_derived_layer_vertices(&scene, w2)
        .into_iter()
        .flatten()
        .collect();
    let has = |pts: &[(f64, f64)], t: (f64, f64)| {
        pts.iter().any(|p| (p.0 - t.0).hypot(p.1 - t.1) < 1e-4)
    };
    assert!(
        has(&w2_pts, (9.9, 0.1)) && has(&w2_pts, (10.1, -0.1)),
        "W2 live-path miter at (10,0) missing in {w2_pts:?}"
    );
    // W3 runs +X from (10,10), so same-side offsets meet at (9.9, 10.1) / (10.1, 9.9).
    assert!(
        has(&w2_pts, (9.9, 10.1)) && has(&w2_pts, (10.1, 9.9)),
        "W2 live-path miter at (10,10) missing in {w2_pts:?}"
    );
}

#[test]
fn scene_cascade_unjoined_opposite_end_wall_unchanged_on_join_two() {
    let mut scene = Scene::new();
    fn add_w(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64)) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }

    // W0: (0, -10) -> (0, 0)
    // W1: (0, 0) -> (10, 0)
    // W2: (10.2, 0) -> (10.2, 10) (unjoined, sitting 0.2m from W1's end 1)
    let w0 = add_w(&mut scene, (0.0, -10.0), (0.0, 0.0));
    let w1 = add_w(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let w2 = add_w(&mut scene, (10.2, 0.0), (10.2, 10.0));

    for h in [w0, w1, w2] {
        regenerate_wall_representation(&mut scene, h, None).expect("regen");
    }

    let w2_axis_before = get_wall_vertices(&scene, w2);
    let w2_geom_before = wall_derived_layer_vertices(&scene, w2);

    // Join W0 and W1 at (0, 0)
    let (kind, touched) = join_two_walls_in_document(&mut scene, w0, w1, None, None, None).expect("join w0-w1");
    assert_eq!(kind, JoinKind::L);

    // W1's end 1 axis must remain strictly at (10, 0)
    let w1_axis_after = get_wall_vertices(&scene, w1);
    assert_eq!(w1_axis_after[1].x, 10.0, "W1 end 1 must not be pulled toward W2");
    assert_eq!(w1_axis_after[1].y, 0.0);

    // W2 must remain completely untouched
    let w2_axis_after = get_wall_vertices(&scene, w2);
    assert_eq!(w2_axis_after, w2_axis_before, "W2 axis must not be moved");
    let w2_geom_after = wall_derived_layer_vertices(&scene, w2);
    assert_eq!(w2_geom_after, w2_geom_before, "W2 derived geometry must be byte-identical");
    assert!(!touched.contains(&w2), "W2 must not appear in touched");
    assert!(
        !engine::owner_index::peers_of(&scene.document, w1).contains(&w2),
        "W1 and W2 must not be linked as peers"
    );

    // W1's derived layer geometry at end 1 must be flat / untrimmed at x = 10.0
    let w1_pts: Vec<(f64, f64)> = wall_derived_layer_vertices(&scene, w1)
        .into_iter()
        .flatten()
        .collect();
    let max_x = w1_pts.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (max_x - 10.0).abs() < 1e-4,
        "W1 end 1 footprint must end at x = 10.0, got max_x = {max_x}"
    );
}

#[test]
fn scene_cascade_unjoined_opposite_end_wall_unchanged_on_override_rebuild() {
    let mut scene = Scene::new();
    fn add_w(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64)) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }

    let w0 = add_w(&mut scene, (0.0, -10.0), (0.0, 0.0));
    let w1 = add_w(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let w2 = add_w(&mut scene, (10.2, 0.0), (10.2, 10.0));

    let (kind, _) = join_two_walls_in_document(&mut scene, w0, w1, None, None, None).expect("join w0-w1");
    assert_eq!(kind, JoinKind::L);

    for h in [w0, w1, w2] {
        regenerate_wall_representation(&mut scene, h, None).expect("regen");
    }

    let w2_axis_before = get_wall_vertices(&scene, w2);
    let w2_geom_before = wall_derived_layer_vertices(&scene, w2);
    let w1_axis_before = get_wall_vertices(&scene, w1);

    // Apply / clear override on W1 at end 0 (junction with W0)
    let touched = apply_junction_override_and_rebuild(&mut scene, w1, 0, None, None, None, None);

    assert_eq!(get_wall_vertices(&scene, w1), w1_axis_before, "W1 axis must not change");
    assert_eq!(get_wall_vertices(&scene, w2), w2_axis_before, "W2 axis must not change");
    assert_eq!(
        wall_derived_layer_vertices(&scene, w2),
        w2_geom_before,
        "W2 derived geometry must be unchanged"
    );
    assert!(!touched.contains(&w2), "W2 must not appear in touched");
    assert!(
        !engine::owner_index::peers_of(&scene.document, w1).contains(&w2),
        "W1 and W2 must not be linked as peers"
    );

    let w1_pts: Vec<(f64, f64)> = wall_derived_layer_vertices(&scene, w1)
        .into_iter()
        .flatten()
        .collect();
    let max_x = w1_pts.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (max_x - 10.0).abs() < 1e-4,
        "W1 end 1 footprint must end at x = 10.0 without miter toward W2, got max_x = {max_x}"
    );
}

#[test]
fn scene_cascade_halo_depth2_wall_never_mutated_or_peer_linked() {
    let mut scene = Scene::new();
    fn add_w(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64)) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }

    // W0: (0, -10) -> (0, 0)
    // W1: (0, 0) -> (10, 0)
    // W2: (10, 0) -> (10, 10) (peer-linked to W1 at (10, 0))
    // W3: (10.2, 10) -> (20, 10) (unjoined, sits 0.2m from W2's far end (10, 10))
    let w0 = add_w(&mut scene, (0.0, -10.0), (0.0, 0.0));
    let w1 = add_w(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let w2 = add_w(&mut scene, (10.0, 0.0), (10.0, 10.0));
    let w3 = add_w(&mut scene, (10.2, 10.0), (20.0, 10.0));

    engine::owner_index::link_peers(&mut scene.document, w1, w2);

    for h in [w0, w1, w2, w3] {
        regenerate_wall_representation(&mut scene, h, None).expect("regen");
    }

    let w3_axis_before = get_wall_vertices(&scene, w3);
    let w3_geom_before = wall_derived_layer_vertices(&scene, w3);

    // Join W0 and W1 at (0, 0)
    let (kind, touched) = join_two_walls_in_document(&mut scene, w0, w1, None, None, None).expect("join w0-w1");
    assert_eq!(kind, JoinKind::L);

    // W3 is a depth-2 halo wall: it must not have its axis moved or peers linked
    let w3_axis_after = get_wall_vertices(&scene, w3);
    assert_eq!(w3_axis_after, w3_axis_before, "W3 halo axis must not be moved");
    let w3_geom_after = wall_derived_layer_vertices(&scene, w3);
    assert_eq!(w3_geom_after, w3_geom_before, "W3 halo derived geometry must be unchanged");
    assert!(!touched.contains(&w3), "W3 must not appear in touched");
    assert!(
        !engine::owner_index::peers_of(&scene.document, w2).contains(&w3),
        "W2 and W3 must not be linked as peers"
    );

    // W1 and W2 must maintain their miter at (10, 0)
    let w2_pts: Vec<(f64, f64)> = wall_derived_layer_vertices(&scene, w2)
        .into_iter()
        .flatten()
        .collect();
    let has = |pts: &[(f64, f64)], t: (f64, f64)| {
        pts.iter().any(|p| (p.0 - t.0).hypot(p.1 - t.1) < 1e-4)
    };
    assert!(
        has(&w2_pts, (9.9, 0.1)) && has(&w2_pts, (10.1, -0.1)),
        "W2 miter at (10,0) missing in {w2_pts:?}"
    );

    // Touched set must only contain walls whose representation was actually modified/regenerated
    assert!(touched.contains(&w0), "W0 must be in touched");
    assert!(touched.contains(&w1), "W1 must be in touched");
    assert!(touched.contains(&w2), "W2 must be in touched");

    // W2 far end at (10, 10) must remain square / unmitered (not pulled toward halo W3)
    assert!(
        has(&w2_pts, (9.9, 10.0)) && has(&w2_pts, (10.1, 10.0)),
        "W2 far end at (10,10) must be square / unmitered in {w2_pts:?}"
    );
}

#[test]
fn scene_cascade_unlinked_nearby_wall_at_trigger_not_swept_and_subsequent_join_preserves_peer_corner() {
    let mut scene = Scene::new();
    fn add_w(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64)) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }

    // W0: (0, -10) -> (0, 0)
    // W1: (0, 0) -> (10, 0)
    // W2: (10, 0) -> (10, 10)
    // W5: (10.15, -0.15) -> (15, -10) (unlinked wall sitting 0.212m from (10, 0), within snap radius 0.30m)
    let w0 = add_w(&mut scene, (0.0, -10.0), (0.0, 0.0));
    let w1 = add_w(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let w2 = add_w(&mut scene, (10.0, 0.0), (10.0, 10.0));
    let w5 = add_w(&mut scene, (10.15, -0.15), (15.0, -10.0));

    for h in [w0, w1, w2, w5] {
        regenerate_wall_representation(&mut scene, h, None).expect("regen");
    }

    let w5_axis_init = get_wall_vertices(&scene, w5);
    let w5_geom_init = wall_derived_layer_vertices(&scene, w5);

    // --- STEP 1: Join W1 and W2 at (10, 0) ---
    let (kind1, touched1) = join_two_walls_in_document(&mut scene, w1, w2, None, None, None).expect("join w1-w2");
    assert_eq!(kind1, JoinKind::L);

    // W1 and W2 axes must be exactly at (10, 0)
    let w1_axis_after1 = get_wall_vertices(&scene, w1);
    let w2_axis_after1 = get_wall_vertices(&scene, w2);
    assert_eq!(w1_axis_after1[1].x, 10.0);
    assert_eq!(w1_axis_after1[1].y, 0.0);
    assert_eq!(w2_axis_after1[0].x, 10.0);
    assert_eq!(w2_axis_after1[0].y, 0.0);

    // W5 must be completely untouched: axis unchanged, geometry unchanged, not in touched, not peer-linked
    let w5_axis_after1 = get_wall_vertices(&scene, w5);
    assert_eq!(w5_axis_after1, w5_axis_init, "W5 axis must remain completely untouched after join(W1, W2)");
    let w5_geom_after1 = wall_derived_layer_vertices(&scene, w5);
    assert_eq!(w5_geom_after1, w5_geom_init, "W5 derived geometry must remain byte-identical");
    assert!(!touched1.contains(&w5), "W5 must not appear in touched after join(W1, W2)");
    assert!(
        !engine::owner_index::peers_of(&scene.document, w1).contains(&w5),
        "W1 and W5 must not be linked as peers"
    );
    assert!(
        !engine::owner_index::peers_of(&scene.document, w2).contains(&w5),
        "W2 and W5 must not be linked as peers"
    );

    // Snapshot W2 geometry and axis after first join
    let w2_geom_snapshot1 = wall_derived_layer_vertices(&scene, w2);

    // Verify W2 has miter at (10, 0)
    let w2_pts1: Vec<(f64, f64)> = w2_geom_snapshot1.iter().flatten().copied().collect();
    let has = |pts: &[(f64, f64)], t: (f64, f64)| {
        pts.iter().any(|p| (p.0 - t.0).hypot(p.1 - t.1) < 1e-4)
    };
    assert!(
        has(&w2_pts1, (9.9, 0.1)) && has(&w2_pts1, (10.1, -0.1)),
        "W2 miter at (10,0) missing after first join in {w2_pts1:?}"
    );

    // --- STEP 2: Join W0 and W1 at (0, 0) ---
    let (kind2, touched2) = join_two_walls_in_document(&mut scene, w0, w1, None, None, None).expect("join w0-w1");
    assert_eq!(kind2, JoinKind::L);

    // W0 and W1 at (0, 0)
    let w0_axis_after2 = get_wall_vertices(&scene, w0);
    let w1_axis_after2 = get_wall_vertices(&scene, w1);
    assert_eq!(w0_axis_after2[1].x, 0.0);
    assert_eq!(w0_axis_after2[1].y, 0.0);
    assert_eq!(w1_axis_after2[0].x, 0.0);
    assert_eq!(w1_axis_after2[0].y, 0.0);

    // FR4 Critical Requirement: W1 end 1 and W2 start at (10, 0) MUST NOT MOVE
    assert_eq!(w1_axis_after2[1].x, 10.0, "W1 end 1 must remain exactly at (10, 0)");
    assert_eq!(w1_axis_after2[1].y, 0.0);
    let w2_axis_after2 = get_wall_vertices(&scene, w2);
    assert_eq!(w2_axis_after2, w2_axis_after1, "W2 axis must be byte-identical to post-join1 state");

    // W2 derived geometry must be byte-identical to post-join1 snapshot
    let w2_geom_after2 = wall_derived_layer_vertices(&scene, w2);
    assert_eq!(w2_geom_after2, w2_geom_snapshot1, "W2 derived geometry must be byte-identical after second join");

    // W5 must still remain completely untouched
    assert_eq!(get_wall_vertices(&scene, w5), w5_axis_init, "W5 axis must remain unchanged after second join");
    assert_eq!(wall_derived_layer_vertices(&scene, w5), w5_geom_init, "W5 geometry must remain unchanged after second join");
    assert!(!touched2.contains(&w5), "W5 must not appear in touched after second join");

    // W1 must have miter at (0, 0) with W0 AND miter at (10, 0) with W2
    let w1_pts2: Vec<(f64, f64)> = wall_derived_layer_vertices(&scene, w1).into_iter().flatten().collect();
    assert!(
        has(&w1_pts2, (-0.1, 0.1)) && has(&w1_pts2, (0.1, -0.1)),
        "W1 miter at (0,0) missing in {w1_pts2:?}"
    );
    assert!(
        has(&w1_pts2, (9.9, 0.1)) && has(&w1_pts2, (10.1, -0.1)),
        "W1 miter at (10,0) missing in {w1_pts2:?}"
    );
}

#[test]
fn scene_cascade_peer_linked_noncoincident_node_not_virtually_trimmed_and_preserves_plain_regen_geometry() {
    let mut scene = Scene::new();
    fn add_w(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64)) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }

    // Probe A setup:
    // W0: (0, -10) -> (0, 0)
    // W1: (0, 0) -> (10, 0)
    // W2: (10.05, 0.05) -> (10.05, 10)
    let w0 = add_w(&mut scene, (0.0, -10.0), (0.0, 0.0));
    let w1 = add_w(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let w2 = add_w(&mut scene, (10.05, 0.05), (10.05, 10.0));

    // W1 and W2 are peer-linked, but their ends at (10,0) and (10.05, 0.05) are not coincident (~0.07m apart)
    engine::owner_index::link_peers(&mut scene.document, w1, w2);

    for h in [w0, w1, w2] {
        regenerate_wall_representation(&mut scene, h, None).expect("regen");
    }

    let w1_axis_pre = get_wall_vertices(&scene, w1);
    let w2_axis_pre = get_wall_vertices(&scene, w2);
    let w2_geom_pre = wall_derived_layer_vertices(&scene, w2);

    // Join W0 and W1 at (0, 0)
    let (kind, touched) = join_two_walls_in_document(&mut scene, w0, w1, None, None, None).expect("join w0-w1");
    assert_eq!(kind, JoinKind::L);

    // Touched set must only contain walls that actually changed
    assert!(touched.contains(&w0), "W0 must be in touched");
    assert!(touched.contains(&w1), "W1 must be in touched");
    assert!(!touched.contains(&w2), "W2 must NOT appear in touched");

    // W1 and W2 axes
    assert_eq!(get_wall_vertices(&scene, w1)[1], w1_axis_pre[1], "W1 end 1 axis must remain strictly at (10, 0)");
    assert_eq!(get_wall_vertices(&scene, w2), w2_axis_pre, "W2 axis must be byte-identical");

    // W2 derived geometry must remain byte-identical to pre-join plain-regen state
    let w2_geom_after = wall_derived_layer_vertices(&scene, w2);
    assert_eq!(
        w2_geom_after,
        w2_geom_pre,
        "W2 derived geometry must remain byte-identical to pre-join plain-regen state"
    );

    // W1 geometry: end 0 has miter with W0 at (0, 0), end 1 has square end at (10, 0)
    let w1_pts: Vec<(f64, f64)> = wall_derived_layer_vertices(&scene, w1).into_iter().flatten().collect();
    let has = |pts: &[(f64, f64)], t: (f64, f64)| {
        pts.iter().any(|p| (p.0 - t.0).hypot(p.1 - t.1) < 1e-4)
    };
    assert!(
        has(&w1_pts, (-0.1, 0.1)) && has(&w1_pts, (0.1, -0.1)),
        "W1 must have miter at (0, 0) in {w1_pts:?}"
    );
    assert!(
        has(&w1_pts, (10.0, 0.1)) && has(&w1_pts, (10.0, -0.1)),
        "W1 end 1 must have square end at (10, 0) in {w1_pts:?}"
    );

    // Following plain regen must keep identical geometry
    regenerate_wall_representation(&mut scene, w2, None).expect("plain regen w2");
    assert_eq!(
        wall_derived_layer_vertices(&scene, w2),
        w2_geom_pre,
        "W2 derived geometry after plain regen must stay byte-identical"
    );

    regenerate_wall_representation(&mut scene, w1, None).expect("plain regen w1");
    let w1_pts_post: Vec<(f64, f64)> = wall_derived_layer_vertices(&scene, w1).into_iter().flatten().collect();
    assert!(
        has(&w1_pts_post, (-0.1, 0.1)) && has(&w1_pts_post, (0.1, -0.1)),
        "W1 must still have miter at (0, 0) after plain regen in {w1_pts_post:?}"
    );
    assert!(
        has(&w1_pts_post, (10.0, 0.1)) && has(&w1_pts_post, (10.0, -0.1)),
        "W1 end 1 must still have square end at (10, 0) after plain regen in {w1_pts_post:?}"
    );
}

#[test]
fn scene_cascade_try_auto_join_preserves_unrelated_nearby_walls_and_touches_all_axis_changes() {
    let mut scene = Scene::new();
    fn add_w(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64)) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }

    // W0: (0, 0) -> (10, 0)
    // W1: (10, 0) -> (10, 10) (peer-linked to W0 at (10, 0))
    // W2: (10.2, 10) -> (20, 10) (unjoined, sits 0.2m from W1's far end (10, 10))
    let w0 = add_w(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let w1 = add_w(&mut scene, (10.0, 0.0), (10.0, 10.0));
    let w2 = add_w(&mut scene, (10.2, 10.0), (20.0, 10.0));

    engine::owner_index::link_peers(&mut scene.document, w0, w1);

    for h in [w0, w1, w2] {
        regenerate_wall_representation(&mut scene, h, None).expect("regen");
    }

    // Newly drawn/placed wall near W0 end 0 (at 0,0)
    let w_new = add_w(&mut scene, (0.0, -10.0), (0.0, 0.1));
    regenerate_wall_representation(&mut scene, w_new, None).expect("regen");

    let w2_axis_before = get_wall_vertices(&scene, w2);
    let w2_geom_before = wall_derived_layer_vertices(&scene, w2);

    let touched = try_auto_join_nearby_walls(&mut scene, w_new, None, None, None);

    // W_new auto-joined with W0
    assert!(touched.contains(&w_new), "w_new must appear in touched");
    assert!(touched.contains(&w0), "w0 must appear in touched");

    // W2 was unjoined and 0.2m away from W1's other end: must be untouched
    assert_eq!(get_wall_vertices(&scene, w2), w2_axis_before, "W2 axis must not be moved");
    assert_eq!(
        wall_derived_layer_vertices(&scene, w2),
        w2_geom_before,
        "W2 derived geometry must be unchanged"
    );
    assert!(!touched.contains(&w2), "W2 must not appear in touched");
    assert!(
        !engine::owner_index::peers_of(&scene.document, w1).contains(&w2),
        "W1 and W2 must not be peer linked"
    );
}

#[test]
fn scene_auto_join_new_wall_at_existing_node_preserves_anchor_and_ignores_unlinked_extras() {
    let mut scene = Scene::new();
    let w1 = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let w2 = add_single_layer_wall(&mut scene, (10.0, 0.0), (10.0, 10.0));
    regenerate_wall_representation(&mut scene, w1, None).unwrap();
    regenerate_wall_representation(&mut scene, w2, None).unwrap();

    let (_kind, _) = join_two_walls_in_document(&mut scene, w1, w2, None, None, None).unwrap();

    // W5 sits at (10.15, -0.15) to (15, -10), unlinked.
    let w5 = add_single_layer_wall(&mut scene, (10.15, -0.15), (15.0, -10.0));
    regenerate_wall_representation(&mut scene, w5, None).unwrap();
    let w5_axis_before = get_wall_vertices(&scene, w5);
    let w5_pts_before = wall_contour_points(&scene, w5);

    // Newly drawn W3 sits at (10, -5) to (10, -0.05).
    let w3 = add_single_layer_wall(&mut scene, (10.0, -5.0), (10.0, -0.05));
    regenerate_wall_representation(&mut scene, w3, None).unwrap();

    // Auto-join W3.
    let touched = try_auto_join_nearby_walls(&mut scene, w3, None, None, None);

    // Assert W3's end snapped to (10, 0).
    let w3_axis = get_wall_vertices(&scene, w3);
    assert!((w3_axis[1].x - 10.0).abs() < 1e-6 && (w3_axis[1].y - 0.0).abs() < 1e-6, "W3 end should be at (10, 0), got {:?}", w3_axis[1]);

    // Assert W1 and W2 corner remained anchored at (10, 0).
    let w1_axis = get_wall_vertices(&scene, w1);
    let w2_axis = get_wall_vertices(&scene, w2);
    assert!((w1_axis[1].x - 10.0).abs() < 1e-6 && (w1_axis[1].y - 0.0).abs() < 1e-6);
    assert!((w2_axis[0].x - 10.0).abs() < 1e-6 && (w2_axis[0].y - 0.0).abs() < 1e-6);
    assert!((w1_axis[0].x - 0.0).abs() < 1e-6 && (w1_axis[0].y - 0.0).abs() < 1e-6);
    assert!((w2_axis[1].x - 10.0).abs() < 1e-6 && (w2_axis[1].y - 10.0).abs() < 1e-6);

    // Assert W5 was NOT touched: axis and rendered geometry identical, absent from touched.
    assert!(!touched.contains(&w5));
    let w5_axis_after = get_wall_vertices(&scene, w5);
    let w5_pts_after = wall_contour_points(&scene, w5);
    assert_eq!(w5_axis_before, w5_axis_after);
    assert_eq!(w5_pts_before, w5_pts_after);

    let peers_w5 = engine::owner_index::peers_of(&scene.document, w5);
    assert!(peers_w5.is_empty());

    let peers_w3 = engine::owner_index::peers_of(&scene.document, w3);
    assert!(peers_w3.contains(&w1) || peers_w3.contains(&w2));
}

#[test]
fn scene_reverse_wall_preserves_joined_miters_and_swaps_overrides() {
    let mut scene = Scene::new();
    let wa = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let wb = add_single_layer_wall(&mut scene, (10.0, 0.0), (10.0, 10.0));
    regenerate_wall_representation(&mut scene, wa, None).unwrap();
    regenerate_wall_representation(&mut scene, wb, None).unwrap();

    let (_kind, _) = join_two_walls_in_document(&mut scene, wa, wb, None, None, None).unwrap();

    // Apply NearFace override to Wall A at End 1 (10, 0).
    let ov = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::NearFace),
        layer_pairs: Vec::new(),
        layer_gaps: Vec::new(),
    };
    apply_junction_override_and_rebuild(&mut scene, wa, 1, Some(&ov), None, None, None);

    let wa_pts_before = wall_contour_points(&scene, wa);
    let wb_pts_before = wall_contour_points(&scene, wb);

    // NearFace butt at near face x = 9.9 on Wall A, end at y = 0.1 on Wall B.
    assert!(has_point(&wa_pts_before, (9.9, -0.1), 1e-4), "NearFace butt before reverse: {:?}", wa_pts_before);
    assert!(has_point(&wa_pts_before, (9.9, 0.1), 1e-4), "NearFace butt before reverse: {:?}", wa_pts_before);
    assert!(has_point(&wb_pts_before, (10.1, 0.1), 1e-4), "Partner corner before reverse: {:?}", wb_pts_before);

    // Reverse Wall A.
    let _ = reverse_wall_in_document(&mut scene, wa, None, None, None).unwrap();

    let wa_axis = get_wall_vertices(&scene, wa);
    assert_eq!(wa_axis[0], DVec3::new(10.0, 0.0, 0.0));
    assert_eq!(wa_axis[1], DVec3::new(0.0, 0.0, 0.0));

    // Override should now be on End 0 (since End 0 is at (10,0)).
    assert!(read_junction_override(&scene, wa, 0).is_some());
    assert!(read_junction_override(&scene, wa, 1).is_none());

    let wa_pts_after = wall_contour_points(&scene, wa);
    let wb_pts_after = wall_contour_points(&scene, wb);

    // NearFace butt on Wall A and partner contour on Wall B must be preserved.
    assert!(has_point(&wa_pts_after, (9.9, -0.1), 1e-4), "NearFace butt after reverse: {:?}", wa_pts_after);
    assert!(has_point(&wa_pts_after, (9.9, 0.1), 1e-4), "NearFace butt after reverse: {:?}", wa_pts_after);
    assert_eq!(wb_pts_before.len(), wb_pts_after.len());
    for pt in &wb_pts_before {
        assert!(has_point(&wb_pts_after, *pt, 1e-4), "Partner contour preserved: expected {:?}", pt);
    }
}

#[test]
fn scene_reverse_polyline_wall_preserves_different_end_overrides_and_curved_bulges() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    let mut v0 = LwVertex::new(Vector2::new(0.0, 0.0));
    v0.bulge = 0.5;
    let mut v1 = LwVertex::new(Vector2::new(5.0, 2.0));
    v1.bulge = -0.25;
    let v2 = LwVertex::new(Vector2::new(10.0, 0.0));
    pl.add_vertex(v0);
    pl.add_vertex(v1);
    pl.add_vertex(v2);

    let mut entity = EntityType::LwPolyline(pl);
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let wall_handle = scene.add_entity(entity);

    // Add partner walls joined at End 0 and End 2 (last vertex index)
    let w_start = add_single_layer_wall(&mut scene, (0.0, -5.0), (0.0, 0.0));
    let w_end = add_single_layer_wall(&mut scene, (10.0, 0.0), (10.0, -5.0));
    regenerate_wall_representation(&mut scene, w_start, None).unwrap();
    regenerate_wall_representation(&mut scene, w_end, None).unwrap();
    regenerate_wall_representation(&mut scene, wall_handle, None).unwrap();

    let (_kind, _) = join_two_walls_in_document(&mut scene, wall_handle, w_start, None, None, None).unwrap();
    let (_kind, _) = join_two_walls_in_document(&mut scene, wall_handle, w_end, None, None, None).unwrap();

    // Put NearFace on End 0 and FarFace on End 2 (last index).
    let ov_near = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::NearFace),
        layer_pairs: Vec::new(),
        layer_gaps: Vec::new(),
    };
    let ov_far = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::FarFace),
        layer_pairs: Vec::new(),
        layer_gaps: Vec::new(),
    };
    write_junction_override(&mut scene, wall_handle, 0, &ov_near);
    write_junction_override(&mut scene, wall_handle, 2, &ov_far);

    regenerate_wall_representation(&mut scene, wall_handle, None).unwrap();
    regenerate_wall_representation(&mut scene, w_start, None).unwrap();
    regenerate_wall_representation(&mut scene, w_end, None).unwrap();

    let w_start_pts_before = wall_contour_points(&scene, w_start);
    let w_end_pts_before = wall_contour_points(&scene, w_end);
    let wall_pts_before = wall_contour_points(&scene, wall_handle);
    let wall_bbox_before = points_bbox_2d(&wall_pts_before);

    // Physical end-face checks before reverse:
    // NearFace at End 0 (x=0) terminates the wall against w_start's near face (x = 0.1).
    let near_pts_before: Vec<(f64, f64)> = wall_pts_before.iter().filter(|p| (p.0 - 0.1).abs() < 1e-3).copied().collect();
    assert_eq!(near_pts_before.len(), 2, "2 NearFace end points at x=0.1: {:?}", wall_pts_before);
    assert!(!wall_pts_before.iter().any(|p| p.0 < 0.1 - 1e-3), "NearFace must not penetrate into w_start (x < 0.1)");

    // FarFace at End 2 (x=10) extends through w_end to its far face (x = 10.1).
    let far_pts_before: Vec<(f64, f64)> = wall_pts_before.iter().filter(|p| (p.0 - 10.1).abs() < 1e-3).copied().collect();
    assert_eq!(far_pts_before.len(), 2, "2 FarFace end points at x=10.1: {:?}", wall_pts_before);

    // Reverse the 3-vertex wall.
    let _ = reverse_wall_in_document(&mut scene, wall_handle, None, None, None).unwrap();

    let axis_after = get_wall_vertices(&scene, wall_handle);
    assert_eq!(axis_after.len(), 3);
    assert!((axis_after[0].x - 10.0).abs() < 1e-6 && axis_after[0].y.abs() < 1e-6);
    assert!((axis_after[1].x - 5.0).abs() < 1e-6 && (axis_after[1].y - 2.0).abs() < 1e-6);
    assert!(axis_after[2].x.abs() < 1e-6 && axis_after[2].y.abs() < 1e-6);

    // Overrides on 0 and 2 must have swapped: 0 gets FarFace, 2 gets NearFace.
    let read_0 = read_junction_override(&scene, wall_handle, 0).expect("override on new start (0)");
    assert_eq!(read_0.default_style, Some(join::JoinOverrideStyle::FarFace));
    let read_2 = read_junction_override(&scene, wall_handle, 2).expect("override on new end (2)");
    assert_eq!(read_2.default_style, Some(join::JoinOverrideStyle::NearFace));
    assert!(read_junction_override(&scene, wall_handle, 1).is_none());

    // Bulges must be reversed and negated: [-(-0.25), -(0.5), 0.0] -> [0.25, -0.5, 0.0].
    let bulges_after = get_wall_bulges(&scene, wall_handle);
    assert!((bulges_after[0] - 0.25).abs() < 1e-6, "bulge[0] was {}", bulges_after[0]);
    assert!((bulges_after[1] - (-0.5)).abs() < 1e-6, "bulge[1] was {}", bulges_after[1]);
    assert_eq!(bulges_after[2], 0.0);

    // Partner walls' contours must be completely preserved.
    let w_start_pts_after = wall_contour_points(&scene, w_start);
    let w_end_pts_after = wall_contour_points(&scene, w_end);
    assert_eq!(w_start_pts_before.len(), w_start_pts_after.len());
    for pt in &w_start_pts_before {
        assert!(has_point(&w_start_pts_after, *pt, 1e-4), "w_start contour preserved: {:?}", pt);
    }
    assert_eq!(w_end_pts_before.len(), w_end_pts_after.len());
    for pt in &w_end_pts_before {
        assert!(has_point(&w_end_pts_after, *pt, 1e-4), "w_end contour preserved: {:?}", pt);
    }

    // Physical end-face checks after reverse:
    // New End 0 is at (10,0) and has FarFace, terminating at x = 10.1.
    // New End 2 is at (0,0) and has NearFace, terminating at x = 0.1.
    let wall_pts_after = wall_contour_points(&scene, wall_handle);
    let near_pts_after: Vec<(f64, f64)> = wall_pts_after.iter().filter(|p| (p.0 - 0.1).abs() < 1e-3).copied().collect();
    assert_eq!(near_pts_after.len(), 2, "2 NearFace end points after reverse at x=0.1: {:?}", wall_pts_after);
    assert!(!wall_pts_after.iter().any(|p| p.0 < 0.1 - 1e-3), "NearFace after reverse must not penetrate into w_start (x < 0.1)");

    let far_pts_after: Vec<(f64, f64)> = wall_pts_after.iter().filter(|p| (p.0 - 10.1).abs() < 1e-3).copied().collect();
    assert_eq!(far_pts_after.len(), 2, "2 FarFace end points after reverse at x=10.1: {:?}", wall_pts_after);

    assert_point_sets_match(&near_pts_before, &near_pts_after, 1e-4, "NearFace end points match before and after reverse");
    assert_point_sets_match(&far_pts_before, &far_pts_after, 1e-4, "FarFace end points match before and after reverse");

    // Curved wall's footprint bbox is preserved.
    let wall_bbox_after = points_bbox_2d(&wall_pts_after);
    assert!((wall_bbox_before.0 - wall_bbox_after.0).abs() < 1e-3);
    assert!((wall_bbox_before.1 - wall_bbox_after.1).abs() < 1e-3);
    assert!((wall_bbox_before.2 - wall_bbox_after.2).abs() < 1e-3);
    assert!((wall_bbox_before.3 - wall_bbox_after.3).abs() < 1e-3);
}

#[test]
fn scene_wallextend_preserves_opposite_end_miters_and_target_geometry() {
    let mut scene = Scene::new();
    let wa = add_single_layer_wall(&mut scene, (0.0, 0.0), (5.0, 0.0));
    let wb = add_single_layer_wall(&mut scene, (0.0, 0.0), (0.0, 5.0));
    regenerate_wall_representation(&mut scene, wa, None).unwrap();
    regenerate_wall_representation(&mut scene, wb, None).unwrap();

    let (_kind, _) = join_two_walls_in_document(&mut scene, wa, wb, None, None, None).unwrap();

    let target = add_single_layer_wall(&mut scene, (10.0, -5.0), (10.0, 5.0));
    regenerate_wall_representation(&mut scene, target, None).unwrap();
    let target_axis_before = get_wall_vertices(&scene, target);
    let target_pts_before = wall_contour_points(&scene, target);

    use crate::ui::command_line::CommandLine;
    let mut cmd_line = CommandLine::new();
    crate::modules::aec::walls::extend::aec_wallextend_do(
        &mut scene,
        &mut cmd_line,
        &format!("{}|WALL|{}", wa.value(), target.value()),
        None,
        None,
        None,
    );

    let wa_axis = get_wall_vertices(&scene, wa);
    assert!((wa_axis[1].x - 10.0).abs() < 1e-6);

    // Opposite end at (0,0) must still share miter corner with wb.
    let peers_a = engine::owner_index::peers_of(&scene.document, wa);
    assert!(peers_a.contains(&wb));
    assert!(peers_a.contains(&target));

    let pts_a = wall_contour_points(&scene, wa);
    // Outer corner at (-0.1, -0.1) should be present.
    assert!(has_point(&pts_a, (-0.1, -0.1), 1e-4), "Opposite end corner miter preserved, got {:?}", pts_a);

    // Stem butt must terminate at target's near face (x = 9.9 for 0.2 thick target centered at x=10).
    assert!(has_point(&pts_a, (9.9, -0.1), 1e-4), "Stem butt near face: {:?}", pts_a);
    assert!(has_point(&pts_a, (9.9, 0.1), 1e-4), "Stem butt near face: {:?}", pts_a);

    // Target wall axis and geometry must remain unchanged.
    let target_axis_after = get_wall_vertices(&scene, target);
    let target_pts_after = wall_contour_points(&scene, target);
    assert_eq!(target_axis_before, target_axis_after);
    assert_eq!(target_pts_before, target_pts_after);
}

#[test]
fn scene_wallextend_point_mode_cleans_stale_peers_when_moved_away() {
    let mut scene = Scene::new();
    let wa = add_single_layer_wall(&mut scene, (0.0, 0.0), (5.0, 0.0));
    let wb = add_single_layer_wall(&mut scene, (0.0, 0.0), (0.0, 5.0));
    let wc = add_single_layer_wall(&mut scene, (5.0, 0.0), (5.0, 5.0));
    regenerate_wall_representation(&mut scene, wa, None).unwrap();
    regenerate_wall_representation(&mut scene, wb, None).unwrap();
    regenerate_wall_representation(&mut scene, wc, None).unwrap();

    let (_kind, _) = join_two_walls_in_document(&mut scene, wa, wb, None, None, None).unwrap();
    let (_kind, _) = join_two_walls_in_document(&mut scene, wa, wc, None, None, None).unwrap();

    // Set junction overrides on all ends:
    // Stable corner at (0,0): wa End 0 (NearFace), wb End 0 (FarFace)
    // Moved corner at (5,0): wa End 1 (FarFace), wc End 0 (NearFace)
    let ov_near = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::NearFace),
        layer_pairs: Vec::new(),
        layer_gaps: Vec::new(),
    };
    let ov_far = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::FarFace),
        layer_pairs: Vec::new(),
        layer_gaps: Vec::new(),
    };
    write_junction_override(&mut scene, wa, 0, &ov_near);
    write_junction_override(&mut scene, wa, 1, &ov_far);
    write_junction_override(&mut scene, wb, 0, &ov_far);
    write_junction_override(&mut scene, wc, 0, &ov_near);

    regenerate_wall_representation(&mut scene, wa, None).unwrap();
    regenerate_wall_representation(&mut scene, wb, None).unwrap();
    regenerate_wall_representation(&mut scene, wc, None).unwrap();

    assert!(read_junction_override(&scene, wa, 0).is_some());
    assert!(read_junction_override(&scene, wa, 1).is_some());
    assert!(read_junction_override(&scene, wb, 0).is_some());
    assert!(read_junction_override(&scene, wc, 0).is_some());
    assert!(engine::owner_index::peers_of(&scene.document, wa).contains(&wb));
    assert!(engine::owner_index::peers_of(&scene.document, wa).contains(&wc));

    use crate::ui::command_line::CommandLine;
    let mut cmd_line = CommandLine::new();
    // Shorten wa's end from 5.0 to 4.8 along X (PT mode, moving 0.2 away from wc, within snap radius 0.3).
    crate::modules::aec::walls::extend::aec_wallextend_do(
        &mut scene,
        &mut cmd_line,
        &format!("{}|PT|4.8|0.0|0.0", wa.value()),
        None,
        None,
        None,
    );

    let wa_axis = get_wall_vertices(&scene, wa);
    assert!((wa_axis[1].x - 4.8).abs() < 1e-6);

    // wc is no longer adjacent to wa's end (0.2 gap), so peer link should be unlinked.
    assert!(!engine::owner_index::peers_of(&scene.document, wa).contains(&wc));
    assert!(!engine::owner_index::peers_of(&scene.document, wc).contains(&wa));

    // Overrides on moved/orphaned ends (wa End 1 and wc End 0) must be dropped!
    assert!(read_junction_override(&scene, wa, 1).is_none(), "wa moved end override must be dropped");
    assert!(read_junction_override(&scene, wc, 0).is_none(), "wc orphaned end override must be dropped");

    // Overrides on stable joined ends (wa End 0 and wb End 0) MUST be retained!
    let wa_ov_0 = read_junction_override(&scene, wa, 0).expect("wa stable end 0 override retained");
    assert_eq!(wa_ov_0.default_style, Some(join::JoinOverrideStyle::NearFace));
    let wb_ov_0 = read_junction_override(&scene, wb, 0).expect("wb stable end 0 override retained");
    assert_eq!(wb_ov_0.default_style, Some(join::JoinOverrideStyle::FarFace));

    // Opposite end at (0,0) must still share miter corner with wb.
    assert!(engine::owner_index::peers_of(&scene.document, wa).contains(&wb));
    let pts_a = wall_contour_points(&scene, wa);
    let pts_b = wall_contour_points(&scene, wb);
    // wa's retained NearFace override at End 0 terminates against wb's near face (x = 0.1).
    assert!(has_point(&pts_a, (0.1, -0.1), 1e-4) && has_point(&pts_a, (0.1, 0.1), 1e-4), "wa near face at 0.1: {:?}", pts_a);
    // wb's retained FarFace override at End 0 extends to wa's far face (y = -0.1).
    assert!(has_point(&pts_b, (-0.1, -0.1), 1e-4) && has_point(&pts_b, (0.1, -0.1), 1e-4), "wb far face at -0.1: {:?}", pts_b);
    // wa has straight cap at x=4.8.
    assert!(has_point(&pts_a, (4.8, -0.1), 1e-4) && has_point(&pts_a, (4.8, 0.1), 1e-4), "wa straight cap at 4.8: {:?}", pts_a);

    // wc at (5,0) has no junction now and should have straight cap at (5.0, 0.0).
    let pts_c = wall_contour_points(&scene, wc);
    assert!(has_point(&pts_c, (4.9, 0.0), 1e-4) && has_point(&pts_c, (5.1, 0.0), 1e-4), "wc straight butt: {:?}", pts_c);
}

#[test]
fn scene_2d_and_3d_wall_representation_synchronous() {
    let mut scene = Scene::new();
    let wa = add_layered_wall(
        &mut scene,
        (0.0, 0.0),
        (10.0, 0.0),
        wls(&[
            ("Finish", 0.02, "Finish"),
            ("Concrete", 0.20, "Structural"),
            ("Finish", 0.02, "Finish"),
        ]),
    );
    let wb = add_layered_wall(
        &mut scene,
        (10.0, 0.0),
        (10.0, 10.0),
        wls(&[
            ("Finish", 0.02, "Finish"),
            ("Concrete", 0.20, "Structural"),
            ("Finish", 0.02, "Finish"),
        ]),
    );
    regenerate_wall_representation(&mut scene, wa, None).unwrap();
    regenerate_wall_representation(&mut scene, wb, None).unwrap();

    // 1. L-miter join
    join_two_walls_in_document(&mut scene, wa, wb, None, None, None).unwrap();

    // Check layer-wise contour, hatch, and solid base vertices for Wall A
    let contours_a = wall_layer_contour_loops(&scene, wa);
    let hatches_a = wall_layer_hatch_loops(&scene, wa);
    let solids_a = wall_layer_solid_wire_base_points(&scene, wa);

    assert_eq!(contours_a.len(), 3, "Wall A has 3 layer contour rings");
    assert_eq!(hatches_a.len(), 3, "Wall A has 3 layer hatch boundary loops");
    assert_eq!(solids_a.len(), 3, "Wall A has 3 layer solid wire sets");

    for i in 0..3 {
        assert!(contours_a[i].len() >= 4, "Wall A ring {} has >= 4 vertices", i);
        assert!(hatches_a[i].len() >= 4, "Wall A hatch {} has >= 4 vertices", i);
        assert!(solids_a[i].len() >= 4, "Wall A solid {} has >= 4 vertices", i);
        assert_point_sets_match(&contours_a[i], &hatches_a[i], 1e-4, "L-join Wall A contour vs hatch");
        assert_point_sets_match(&contours_a[i], &solids_a[i], 1e-4, "L-join Wall A contour vs solid base");
    }

    // Concrete outer corner (10.12, -0.12) in outer finish layer 0 of Wall A
    assert!(has_point(&contours_a[0], (10.12, -0.12), 1e-4), "Outer finish L-miter point (10.12, -0.12): {:?}", contours_a[0]);
    // Concrete miter point (10.10, -0.10) in concrete core layer 1
    assert!(has_point(&contours_a[1], (10.10, -0.10), 1e-4), "Core L-miter point (10.10, -0.10): {:?}", contours_a[1]);

    // Check Wall B L-miter synchronization as well
    let contours_b = wall_layer_contour_loops(&scene, wb);
    let hatches_b = wall_layer_hatch_loops(&scene, wb);
    let solids_b = wall_layer_solid_wire_base_points(&scene, wb);
    assert_eq!(contours_b.len(), 3);
    assert_eq!(hatches_b.len(), 3);
    assert_eq!(solids_b.len(), 3);
    for i in 0..3 {
        assert!(contours_b[i].len() >= 4);
        assert_point_sets_match(&contours_b[i], &hatches_b[i], 1e-4, "L-join Wall B contour vs hatch");
        assert_point_sets_match(&contours_b[i], &solids_b[i], 1e-4, "L-join Wall B contour vs solid base");
    }
    assert!(has_point(&contours_b[0], (10.12, -0.12), 1e-4), "Wall B outer finish L-miter: {:?}", contours_b[0]);
    assert!(has_point(&contours_b[1], (10.10, -0.10), 1e-4), "Wall B core L-miter: {:?}", contours_b[1]);

    // 2. T-junction with stem
    let stem = add_layered_wall(
        &mut scene,
        (5.0, 0.0),
        (5.0, -5.0),
        wls(&[
            ("Finish", 0.02, "Finish"),
            ("Concrete", 0.20, "Structural"),
            ("Finish", 0.02, "Finish"),
        ]),
    );
    regenerate_wall_representation(&mut scene, stem, None).unwrap();
    join_two_walls_in_document(&mut scene, stem, wa, None, None, None).unwrap();

    let contours_wa = wall_layer_contour_loops(&scene, wa);
    let hatches_wa = wall_layer_hatch_loops(&scene, wa);
    let solids_wa = wall_layer_solid_wire_base_points(&scene, wa);

    // wa's approach finish (layer 0) is notched into 2 rings, plus 1 core ring and 1 back finish ring = 4 rings
    assert_eq!(contours_wa.len(), 4, "T-join wa has 4 rings (approach finish notched into 2)");
    assert_eq!(contours_wa.len(), hatches_wa.len());
    assert_eq!(contours_wa.len(), solids_wa.len());

    for i in 0..contours_wa.len() {
        assert!(contours_wa[i].len() >= 4, "wa ring {} has >= 4 vertices", i);
        assert_point_sets_match(&contours_wa[i], &hatches_wa[i], 1e-4, "T-join wa contour vs hatch");
        assert_point_sets_match(&contours_wa[i], &solids_wa[i], 1e-4, "T-join wa contour vs solid base");
    }

    // Approach finish notch vertices on through wall wa at x in [4.88, 5.12], y in [-0.12, -0.10]
    let wa_all_pts: Vec<(f64, f64)> = contours_wa.iter().flatten().copied().collect();
    assert!(has_point(&wa_all_pts, (4.88, -0.12), 1e-4), "wa notch at (4.88, -0.12): {:?}", wa_all_pts);
    assert!(has_point(&wa_all_pts, (4.90, -0.10), 1e-4), "wa notch at (4.90, -0.10): {:?}", wa_all_pts);
    assert!(has_point(&wa_all_pts, (5.10, -0.10), 1e-4), "wa notch at (5.10, -0.10): {:?}", wa_all_pts);
    assert!(has_point(&wa_all_pts, (5.12, -0.12), 1e-4), "wa notch at (5.12, -0.12): {:?}", wa_all_pts);

    let contours_stem = wall_layer_contour_loops(&scene, stem);
    let hatches_stem = wall_layer_hatch_loops(&scene, stem);
    let solids_stem = wall_layer_solid_wire_base_points(&scene, stem);

    assert_eq!(contours_stem.len(), 3);
    assert_eq!(hatches_stem.len(), 3);
    assert_eq!(solids_stem.len(), 3);

    for i in 0..3 {
        assert!(contours_stem[i].len() >= 4, "stem ring {} has >= 4 vertices", i);
        assert_point_sets_match(&contours_stem[i], &hatches_stem[i], 1e-4, "T-join stem contour vs hatch");
        assert_point_sets_match(&contours_stem[i], &solids_stem[i], 1e-4, "T-join stem contour vs solid base");
    }

    // Stem structural core stops at near face of wa core (y = -0.10), with x in [4.9, 5.1]
    assert!(has_point(&contours_stem[1], (4.90, -0.10), 1e-4), "Stem core stop: {:?}", contours_stem[1]);
    assert!(has_point(&contours_stem[1], (5.10, -0.10), 1e-4), "Stem core stop: {:?}", contours_stem[1]);
    // Stem finish layers bevel-stop against wa finish and core face
    assert!(has_point(&contours_stem[0], (4.88, -0.12), 1e-4), "Stem finish stop: {:?}", contours_stem[0]);
    assert!(has_point(&contours_stem[0], (4.90, -0.10), 1e-4), "Stem finish stop: {:?}", contours_stem[0]);
    assert!(has_point(&contours_stem[2], (5.10, -0.10), 1e-4), "Stem finish stop: {:?}", contours_stem[2]);
    assert!(has_point(&contours_stem[2], (5.12, -0.12), 1e-4), "Stem finish stop: {:?}", contours_stem[2]);

    // 3. X-crossing
    let wx_thru = add_layered_wall(
        &mut scene,
        (0.0, 20.0),
        (20.0, 20.0),
        wls(&[
            ("Finish", 0.02, "Finish"),
            ("Concrete", 0.20, "Structural"),
            ("Finish", 0.02, "Finish"),
        ]),
    );
    let wx_cross = add_layered_wall(
        &mut scene,
        (10.0, 12.0),
        (10.0, 28.0),
        wls(&[
            ("Finish", 0.02, "Finish"),
            ("Concrete", 0.20, "Structural"),
            ("Finish", 0.02, "Finish"),
        ]),
    );
    regenerate_wall_representation(&mut scene, wx_thru, None).unwrap();
    regenerate_wall_representation(&mut scene, wx_cross, None).unwrap();

    let _ = join_two_walls_in_document(&mut scene, wx_thru, wx_cross, None, None, None).unwrap();

    let c_thru = wall_layer_contour_loops(&scene, wx_thru);
    let h_thru = wall_layer_hatch_loops(&scene, wx_thru);
    let s_thru = wall_layer_solid_wire_base_points(&scene, wx_thru);
    // wx_thru has 5 rings: 2 for near finish, 1 for continuous core, 2 for far finish
    assert_eq!(c_thru.len(), 5, "X through wall has 5 rings");
    assert_eq!(c_thru.len(), h_thru.len());
    assert_eq!(c_thru.len(), s_thru.len());
    for i in 0..c_thru.len() {
        assert!(c_thru[i].len() >= 4, "thru ring {} has >= 4 vertices", i);
        assert_point_sets_match(&c_thru[i], &h_thru[i], 1e-4, "X-crossing through contour vs hatch");
        assert_point_sets_match(&c_thru[i], &s_thru[i], 1e-4, "X-crossing through contour vs solid base");
    }

    // Both-sided finish notches on through wall wx_thru (at x in [9.88, 10.12]):
    let thru_pts: Vec<(f64, f64)> = c_thru.iter().flatten().copied().collect();
    // Near finish notches (y in [19.88, 19.90])
    assert!(has_point(&thru_pts, (9.88, 19.88), 1e-4), "thru near notch: {:?}", thru_pts);
    assert!(has_point(&thru_pts, (9.90, 19.90), 1e-4), "thru near notch: {:?}", thru_pts);
    assert!(has_point(&thru_pts, (10.12, 19.88), 1e-4), "thru near notch: {:?}", thru_pts);
    assert!(has_point(&thru_pts, (10.10, 19.90), 1e-4), "thru near notch: {:?}", thru_pts);
    // Far finish notches (y in [20.10, 20.12])
    assert!(has_point(&thru_pts, (9.90, 20.10), 1e-4), "thru far notch: {:?}", thru_pts);
    assert!(has_point(&thru_pts, (9.88, 20.12), 1e-4), "thru far notch: {:?}", thru_pts);
    assert!(has_point(&thru_pts, (10.10, 20.10), 1e-4), "thru far notch: {:?}", thru_pts);
    assert!(has_point(&thru_pts, (10.12, 20.12), 1e-4), "thru far notch: {:?}", thru_pts);

    let c_cross = wall_layer_contour_loops(&scene, wx_cross);
    let h_cross = wall_layer_hatch_loops(&scene, wx_cross);
    let s_cross = wall_layer_solid_wire_base_points(&scene, wx_cross);
    // wx_cross is split into two stems, each layer has 2 rings -> 6 rings total
    assert_eq!(c_cross.len(), 6, "X crossing wall has 6 rings (2 per layer)");
    assert_eq!(c_cross.len(), h_cross.len());
    assert_eq!(c_cross.len(), s_cross.len());
    for i in 0..c_cross.len() {
        assert!(c_cross[i].len() >= 4, "cross ring {} has >= 4 vertices", i);
        assert_point_sets_match(&c_cross[i], &h_cross[i], 1e-4, "X-crossing cross contour vs hatch");
        assert_point_sets_match(&c_cross[i], &s_cross[i], 1e-4, "X-crossing cross contour vs solid base");
    }

    // Stem stops on both sides for crossing wall wx_cross:
    let cross_pts: Vec<(f64, f64)> = c_cross.iter().flatten().copied().collect();
    // Lower stem stops at near face:
    assert!(has_point(&cross_pts, (9.88, 19.88), 1e-4), "cross lower finish stop: {:?}", cross_pts);
    assert!(has_point(&cross_pts, (9.90, 19.88), 1e-4), "cross lower finish stop: {:?}", cross_pts);
    assert!(has_point(&cross_pts, (9.90, 19.90), 1e-4), "cross lower core stop: {:?}", cross_pts);
    assert!(has_point(&cross_pts, (10.10, 19.90), 1e-4), "cross lower core stop: {:?}", cross_pts);
    assert!(has_point(&cross_pts, (10.10, 19.88), 1e-4), "cross lower finish stop: {:?}", cross_pts);
    assert!(has_point(&cross_pts, (10.12, 19.88), 1e-4), "cross lower finish stop: {:?}", cross_pts);
    // Upper stem stops at far face:
    assert!(has_point(&cross_pts, (9.88, 20.12), 1e-4), "cross upper finish stop: {:?}", cross_pts);
    assert!(has_point(&cross_pts, (9.90, 20.12), 1e-4), "cross upper finish stop: {:?}", cross_pts);
    assert!(has_point(&cross_pts, (9.90, 20.10), 1e-4), "cross upper core stop: {:?}", cross_pts);
    assert!(has_point(&cross_pts, (10.10, 20.10), 1e-4), "cross upper core stop: {:?}", cross_pts);
    assert!(has_point(&cross_pts, (10.10, 20.12), 1e-4), "cross upper finish stop: {:?}", cross_pts);
    assert!(has_point(&cross_pts, (10.12, 20.12), 1e-4), "cross upper finish stop: {:?}", cross_pts);

    // 4. Verify representation switches (ComponentRuleSet) and restoration
    use crate::modules::aec::engine::display_component::{ComponentRuleSet, WallComponentSlot};

    let all_walls = [wa, wb, stem, wx_thru, wx_cross];
    let expected_ring_counts = [4, 3, 3, 5, 6];
    let geometry_before_switch: Vec<_> = all_walls
        .iter()
        .map(|&w| {
            (
                wall_layer_contour_loops(&scene, w),
                wall_layer_hatch_loops(&scene, w),
                wall_layer_solid_wire_base_points(&scene, w),
            )
        })
        .collect();
    for (i, (contours, hatches, solids)) in geometry_before_switch.iter().enumerate() {
        assert_eq!(contours.len(), expected_ring_counts[i], "baseline contour ring count for wall {i}");
        assert_eq!(hatches.len(), expected_ring_counts[i], "baseline hatch ring count for wall {i}");
        assert_eq!(solids.len(), expected_ring_counts[i], "baseline solid ring count for wall {i}");
    }

    // 4a. Hide hatch and solid, show only contours
    let mut rules_hide_hs = ComponentRuleSet::default();
    rules_hide_hs.visibility.insert(WallComponentSlot::LayerHatch2D.key().to_string(), false);
    rules_hide_hs.visibility.insert(WallComponentSlot::ContourHatch2D.key().to_string(), false);
    rules_hide_hs.visibility.insert(WallComponentSlot::Solid3D.key().to_string(), false);
    rules_hide_hs.visibility.insert(WallComponentSlot::Layers2D.key().to_string(), true);
    rules_hide_hs.visibility.insert(WallComponentSlot::Contour2D.key().to_string(), true);
    for (i, &w) in all_walls.iter().enumerate() {
        regenerate_wall_representation_with_rules(&mut scene, w, Some(&rules_hide_hs), None).unwrap();
        let c = wall_layer_contour_loops(&scene, w);
        let h = wall_layer_hatch_loops(&scene, w);
        let s = wall_layer_solid_wire_base_points(&scene, w);
        assert_eq!(c.len(), expected_ring_counts[i], "hidden-rule contour ring count for wall {i}");
        assert!(!c.is_empty(), "contours must be present");
        assert_eq!(h.len(), 0, "hatches must be hidden");
        assert_eq!(s.len(), 0, "solids must be hidden");
    }

    // 4b. Hide contours and solids, show only hatches
    let mut rules_only_hatch = ComponentRuleSet::default();
    rules_only_hatch.visibility.insert(WallComponentSlot::Layers2D.key().to_string(), false);
    rules_only_hatch.visibility.insert(WallComponentSlot::Contour2D.key().to_string(), false);
    rules_only_hatch.visibility.insert(WallComponentSlot::Solid3D.key().to_string(), false);
    rules_only_hatch.visibility.insert(WallComponentSlot::LayerHatch2D.key().to_string(), true);
    for &w in &all_walls {
        regenerate_wall_representation_with_rules(&mut scene, w, Some(&rules_only_hatch), None).unwrap();
        let c = wall_layer_contour_loops(&scene, w);
        let h = wall_layer_hatch_loops(&scene, w);
        let s = wall_layer_solid_wire_base_points(&scene, w);
        assert_eq!(c.len(), 0, "contours must be hidden");
        assert!(!h.is_empty(), "hatches must be present");
        assert_eq!(s.len(), 0, "solids must be hidden");
    }

    // 4c. Restore all visible via ComponentRuleSet
    let mut rules_all = ComponentRuleSet::default();
    rules_all.visibility.insert(WallComponentSlot::Layers2D.key().to_string(), true);
    rules_all.visibility.insert(WallComponentSlot::Contour2D.key().to_string(), true);
    rules_all.visibility.insert(WallComponentSlot::LayerHatch2D.key().to_string(), true);
    rules_all.visibility.insert(WallComponentSlot::Solid3D.key().to_string(), true);
    for (i, &w) in all_walls.iter().enumerate() {
        regenerate_wall_representation_with_rules(&mut scene, w, Some(&rules_all), None).unwrap();
        let c = wall_layer_contour_loops(&scene, w);
        let h = wall_layer_hatch_loops(&scene, w);
        let s = wall_layer_solid_wire_base_points(&scene, w);
        assert_eq!(c.len(), expected_ring_counts[i], "restored contour ring count for wall {i}");
        assert!(!c.is_empty(), "restored contours must not be empty");
        assert_eq!(c.len(), h.len(), "restored contour vs hatch count");
        assert_eq!(c.len(), s.len(), "restored contour vs solid count");
        for i in 0..c.len() {
            assert!(c[i].len() >= 4);
            assert_point_sets_match(&c[i], &h[i], 1e-4, "restored contour vs hatch");
            assert_point_sets_match(&c[i], &s[i], 1e-4, "restored contour vs solid");
        }
        assert_layer_rings_match_snapshot(&geometry_before_switch[i].0, &c, "restored contour");
        assert_layer_rings_match_snapshot(&geometry_before_switch[i].1, &h, "restored hatch");
        assert_layer_rings_match_snapshot(&geometry_before_switch[i].2, &s, "restored solid");
    }

    // 4d. Plain regen without rules reproduces the exact same geometry
    for (i, &w) in all_walls.iter().enumerate() {
        regenerate_wall_representation(&mut scene, w, None).unwrap();
        let c = wall_layer_contour_loops(&scene, w);
        let h = wall_layer_hatch_loops(&scene, w);
        let s = wall_layer_solid_wire_base_points(&scene, w);
        assert_eq!(c.len(), expected_ring_counts[i], "plain regen contour ring count for wall {i}");
        assert!(!c.is_empty(), "plain regen contours must not be empty");
        assert_eq!(c.len(), h.len(), "plain regen contour vs hatch count");
        assert_eq!(c.len(), s.len(), "plain regen contour vs solid count");
        for i in 0..c.len() {
            assert!(c[i].len() >= 4);
            assert_point_sets_match(&c[i], &h[i], 1e-4, "plain regen contour vs hatch");
            assert_point_sets_match(&c[i], &s[i], 1e-4, "plain regen contour vs solid");
        }
        assert_layer_rings_match_snapshot(&geometry_before_switch[i].0, &c, "plain regen contour");
        assert_layer_rings_match_snapshot(&geometry_before_switch[i].1, &h, "plain regen hatch");
        assert_layer_rings_match_snapshot(&geometry_before_switch[i].2, &s, "plain regen solid");
    }
}

/// Collect min/max Y of every derived LwPolyline vertex for a wall package.
pub(super) fn wall_derived_y_bounds(scene: &Scene, wall: Handle) -> (f64, f64) {
    let v2 = wall_from_entity(scene.document.get_entity(wall).unwrap()).unwrap();
    let mut ymin = f64::INFINITY;
    let mut ymax = f64::NEG_INFINITY;
    for h in &v2.derived_handles {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*h) {
            for v in &pl.vertices {
                ymin = ymin.min(v.location.y);
                ymax = ymax.max(v.location.y);
            }
        }
    }
    (ymin, ymax)
}
