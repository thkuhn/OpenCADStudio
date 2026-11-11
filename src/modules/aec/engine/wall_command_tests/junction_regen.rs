use super::*;

#[test]
fn vertex_move_cascades_to_full_junction() {
    let mut scene = Scene::new();
    // 3-way junction at (0,0).
    // W1: (0,0) to (10,0)
    // W2: (0,0) to (0,10)
    // W3: (0,0) to (0,-10)

    let w1 = add_multi_layer_wall(&mut scene);
    update_wall_vertices(&mut scene, w1, &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0)]);
    regenerate_wall_representation(&mut scene, w1, None).unwrap();

    let w2 = add_multi_layer_wall(&mut scene);
    update_wall_vertices(&mut scene, w2, &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 10.0, 0.0)]);
    regenerate_wall_representation(&mut scene, w2, None).unwrap();

    let w3 = add_multi_layer_wall(&mut scene);
    update_wall_vertices(&mut scene, w3, &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, -10.0, 0.0)]);
    regenerate_wall_representation(&mut scene, w3, None).unwrap();

    // Initial join.
    join_junction_in_document(&mut scene,  &[w1, w2, w3],  None,  None,  None,  None).unwrap();

    // Move W1's endpoint at (0,0) slightly to (0.1, 0.1).
    // This should trigger a rebuild of ALL THREE walls via try_auto_join_nearby_walls.
    let mut axis1 = get_wall_vertices(&scene, w1);
    axis1[0] = DVec3::new(0.1, 0.0, 0.0);
    update_wall_vertices(&mut scene, w1, &axis1);
    regenerate_wall_representation(&mut scene, w1, None).unwrap();
    
    let touched = try_auto_join_nearby_walls(&mut scene,  w1,  None,  None,  None);

    // Assert all 3 walls are still joined (their ends moved to (0.1, 0.0)).
    let axis1 = get_wall_vertices(&scene, w1);
    assert!((axis1[0].x - 0.1).abs() < 1e-6);

    // If it worked, W2 and W3 should also have their ends moved to (0.1, 0.0).
    let axis2 = get_wall_vertices(&scene, w2);
    let axis3 = get_wall_vertices(&scene, w3);

    assert!(
        (axis2[0].x - 0.1).abs() < 1e-6,
        "W2 should have followed W1 move to (0.1, 0), got {:?}",
        axis2[0]
    );
    assert!(
        (axis3[0].x - 0.1).abs() < 1e-6,
        "W3 should have followed W1 move to (0.1, 0), got {:?}",
        axis3[0]
    );
    assert!(touched.contains(&w1));
    assert!(touched.contains(&w2));
    assert!(touched.contains(&w3));

    // Assert peer links are still correct.
    let peers1 = engine::owner_index::peers_of(&scene.document, w1);
    assert!(peers1.contains(&w2));
    assert!(peers1.contains(&w3));
    assert_eq!(peers1.len(), 2);

    // Now move W1 far away and assert unlinking.
    let mut axis1 = get_wall_vertices(&scene, w1);
    axis1[0] = DVec3::new(100.0, 100.0, 0.0);
    axis1[1] = DVec3::new(110.0, 100.0, 0.0);
    update_wall_vertices(&mut scene, w1, &axis1);
    
    let touched_far = try_auto_join_nearby_walls(&mut scene,  w1,  None,  None,  None);
    
    let peers1_far = engine::owner_index::peers_of(&scene.document, w1);
    assert!(peers1_far.is_empty(), "W1 should be unlinked after moving far away");
    assert!(touched_far.contains(&w2));
    assert!(touched_far.contains(&w3));
    
    let peers2_far = engine::owner_index::peers_of(&scene.document, w2);
    assert!(!peers2_far.contains(&w1));
    assert!(peers2_far.contains(&w3)); // W2 and W3 still meet at (0.1, 0)
}

/// Collects every vertex of every `LwPolyline` derived (`WALL_REP`)
/// child of `wall_handle` into one flat list, for corner-position
/// assertions against a wall's rendered footprint(s).
pub(super) fn wall_contour_points(scene: &Scene, wall_handle: Handle) -> Vec<(f64, f64)> {
    wall_contour_loops(scene, wall_handle)
        .into_iter()
        .flatten()
        .collect()
}

pub(super) fn wall_layer_contour_loops(scene: &Scene, wall_handle: Handle) -> Vec<Vec<(f64, f64)>> {
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return Vec::new();
    };
    let Some(wall) = wall_from_entity(entity) else {
        return Vec::new();
    };
    let mut loops = Vec::new();
    for h in &wall.derived_handles {
        let ent = scene.document.get_entity(*h).unwrap();
        if let Some(record) = read_aec_record(ent) {
            if let [XDataValue::String(kind), _, XDataValue::String(role)] = record.values.as_slice() {
                if kind == "WALL_REP" && role == WALL_REP_ROLE_CONTOUR {
                    if let EntityType::LwPolyline(pl) = ent {
                        loops.push(
                            pl.vertices
                                .iter()
                                .map(|v| (v.location.x, v.location.y))
                                .collect(),
                        );
                    }
                }
            }
        }
    }
    loops
}

pub(super) fn wall_layer_hatch_loops(scene: &Scene, wall_handle: Handle) -> Vec<Vec<(f64, f64)>> {
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return Vec::new();
    };
    let Some(wall) = wall_from_entity(entity) else {
        return Vec::new();
    };
    let mut loops = Vec::new();
    for h in &wall.derived_handles {
        let ent = scene.document.get_entity(*h).unwrap();
        if let Some(record) = read_aec_record(ent) {
            if let [XDataValue::String(kind), _, XDataValue::String(role)] = record.values.as_slice() {
                if kind == "WALL_REP" && role == WALL_REP_ROLE_HATCH {
                    if let EntityType::Hatch(hatch) = ent {
                        for path in &hatch.paths {
                            let mut pts = Vec::new();
                            for edge in &path.edges {
                                match edge {
                                    acadrust::entities::hatch::BoundaryEdge::Polyline(pl) => {
                                        for v in &pl.vertices {
                                            pts.push((v.x, v.y));
                                        }
                                    }
                                    acadrust::entities::hatch::BoundaryEdge::Line(l) => {
                                        pts.push((l.start.x, l.start.y));
                                    }
                                    _ => {}
                                }
                            }
                            if !pts.is_empty() {
                                loops.push(pts);
                            }
                        }
                    }
                }
            }
        }
    }
    loops
}

pub(super) fn wall_layer_solid_wire_base_points(scene: &Scene, wall_handle: Handle) -> Vec<Vec<(f64, f64)>> {
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return Vec::new();
    };
    let Some(wall) = wall_from_entity(entity) else {
        return Vec::new();
    };
    let base_z = wall.base_origin[2];
    let mut out = Vec::new();
    for h in &wall.derived_handles {
        let ent = scene.document.get_entity(*h).unwrap();
        if let Some(record) = read_aec_record(ent) {
            if let [XDataValue::String(kind), _, XDataValue::String(role)] = record.values.as_slice() {
                if kind == "WALL_REP" && role == WALL_REP_ROLE_SOLID {
                    if let EntityType::Solid3D(solid) = ent {
                        let mut pts = Vec::new();
                        for wire in &solid.wires {
                            for p in &wire.points {
                                if (p.z - base_z).abs() < 1e-3 {
                                    pts.push((p.x, p.y));
                                }
                            }
                        }
                        if !pts.is_empty() {
                            out.push(pts);
                        }
                    }
                }
            }
        }
    }
    out
}

pub(super) fn points_bbox_2d(pts: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for &(x, y) in pts {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    (min_x, min_y, max_x, max_y)
}

pub(super) fn wall_contour_loops(scene: &Scene, wall_handle: Handle) -> Vec<Vec<(f64, f64)>> {
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return Vec::new();
    };
    let Some(wall) = wall_from_entity(entity) else {
        return Vec::new();
    };
    let mut loops = Vec::new();
    for h in &wall.derived_handles {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*h) {
            loops.push(
                pl.vertices
                    .iter()
                    .map(|v| (v.location.x, v.location.y))
                    .collect(),
            );
        }
    }
    loops
}

pub(super) fn has_point(pts: &[(f64, f64)], target: (f64, f64), tol: f64) -> bool {
    pts.iter()
        .any(|p| (p.0 - target.0).abs() < tol && (p.1 - target.1).abs() < tol)
}

pub(super) fn assert_point_sets_match(set_a: &[(f64, f64)], set_b: &[(f64, f64)], tol: f64, context: &str) {
    for pt in set_a {
        assert!(
            set_b.iter().any(|b| (b.0 - pt.0).abs() <= tol && (b.1 - pt.1).abs() <= tol),
            "Point {:?} from A not found in B ({context}). A: {:?}, B: {:?}",
            pt, set_a, set_b
        );
    }
    for pt in set_b {
        assert!(
            set_a.iter().any(|a| (a.0 - pt.0).abs() <= tol && (a.1 - pt.1).abs() <= tol),
            "Point {:?} from B not found in A ({context}). A: {:?}, B: {:?}",
            pt, set_a, set_b
        );
    }
}

pub(super) fn assert_layer_rings_match_snapshot(
    expected: &[Vec<(f64, f64)>],
    actual: &[Vec<(f64, f64)>],
    context: &str,
) {
    assert_eq!(actual.len(), expected.len(), "{context} ring count");
    for (i, (expected_ring, actual_ring)) in expected.iter().zip(actual).enumerate() {
        assert_eq!(actual_ring.len(), expected_ring.len(), "{context} vertex count for ring {i}");
        assert_point_sets_match(
            expected_ring,
            actual_ring,
            1e-4,
            &format!("{context} ring {i} matches pre-switch geometry"),
        );
    }
}

pub(super) fn x_scan_layers() -> Vec<WallLayer> {
    wls(&[
        ("Finish", 0.02, "Finish"),
        ("Concrete", 0.20, "Structural"),
        ("Finish", 0.02, "Finish"),
    ])
}

/// Contour rings of a wall, each ring as a sorted, de-duplicated point set on
/// a 1e-4 grid, rings sorted — comparable across scenes / insertion orders.
pub(super) fn normalized_contour_rings(scene: &Scene, wall: Handle) -> Vec<Vec<(i64, i64)>> {
    let mut rings: Vec<Vec<(i64, i64)>> = wall_layer_contour_loops(scene, wall)
        .into_iter()
        .map(|ring| {
            let mut pts: Vec<(i64, i64)> = ring
                .iter()
                .map(|p| ((p.0 * 1e4).round() as i64, (p.1 * 1e4).round() as i64))
                .collect();
            pts.sort();
            pts.dedup();
            pts
        })
        .collect();
    rings.sort();
    rings
}

/// Adds `decoys` short unrelated walls far away on row `y` (use distinct rows
/// >= 500 per call), shifting the global document index of everything added
/// afterwards.
pub(super) fn add_far_decoy_walls(scene: &mut Scene, decoys: usize, y: f64) -> Vec<Handle> {
    (0..decoys)
        .map(|i| {
            let x = i as f64 * 7.0;
            add_layered_wall(scene, (x, y), (x + 5.0, y), x_scan_layers())
        })
        .collect()
}

/// Through wall (0,0)-(20,0) crossed by two walls at x=6 and x=14. `order`
/// is a permutation of `[0 = through, 1 = cross@6, 2 = cross@14]` giving the
/// document insertion order. Returns `[through, cross6, cross14]` after a
/// plain regeneration of every wall (no explicit join). `decoys` far walls are
/// inserted before *each* of the three walls, so every global index shifts by
/// a different amount while the relative order stays `order`.
pub(super) fn build_double_x_scene(scene: &mut Scene, order: [usize; 3], decoys: usize) -> [Handle; 3] {
    let specs = [
        ((0.0, 0.0), (20.0, 0.0)),
        ((6.0, -5.0), (6.0, 5.0)),
        ((14.0, -5.0), (14.0, 5.0)),
    ];
    let mut out = [Handle::NULL; 3];
    for (n, &k) in order.iter().enumerate() {
        add_far_decoy_walls(scene, decoys, 500.0 + 10.0 * n as f64);
        out[k] = add_layered_wall(scene, specs[k].0, specs[k].1, x_scan_layers());
    }
    for h in all_wall_axis_handles(scene) {
        regenerate_wall_representation(scene, h, None).unwrap();
    }
    out
}

pub(super) fn add_bulged_layered_wall(
    scene: &mut Scene,
    p1: (f64, f64),
    p2: (f64, f64),
    bulge: f64,
    layers: Vec<WallLayer>,
) -> Handle {
    let mut pl = LwPolyline::new();
    let mut v0 = LwVertex::new(Vector2::new(p1.0, p1.1));
    v0.bulge = bulge;
    pl.add_vertex(v0);
    pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
    let mut entity = EntityType::LwPolyline(pl);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record(
        "style1",
        3.0,
        0,
        &layers,
        &[],
        WallJustification::Center,
        PlanPhase::New,
        None,
    );
    entity.common_mut().extended_data.add_record(record);
    scene.add_entity(entity)
}

pub(super) fn ring_with_points(rings: &[Vec<(f64, f64)>], pts: &[(f64, f64)]) -> bool {
    rings
        .iter()
        .any(|r| pts.iter().all(|p| has_point(r, *p, 1e-6)))
}

/// Plain regeneration (no explicit join) of a through wall crossed twice must
/// resolve both X crossings: continuous core, finish layers notched on both
/// sides at both crossings, every crossing wall split into two stems per
/// layer. Equal-priority X ties go to the earlier wall in document order
/// (`x_priority_pair`); the local scan keeps that relative order, so far
/// decoy walls interleaved anywhere (shifting every global index, self being
/// local index 0 or last) must give identical geometry.
#[test]
fn plain_regen_double_x_crossing_is_local_and_keeps_document_order_tie_break() {
    let mut scene = Scene::new();
    let [thru, c6, c14] = build_double_x_scene(&mut scene, [0, 1, 2], 0);
    let c_thru = wall_layer_contour_loops(&scene, thru);
    assert_eq!(c_thru.len(), 7, "core + 2x3 notched finish rings: {c_thru:?}");
    assert!(
        ring_with_points(&c_thru, &[(0.0, -0.1), (20.0, -0.1), (20.0, 0.1), (0.0, 0.1)]),
        "through core must stay continuous over x in [0, 20]: {c_thru:?}"
    );
    for y in [-0.12, 0.12] {
        for x in [5.88, 6.12, 13.88, 14.12] {
            assert!(
                c_thru.iter().any(|r| has_point(r, (x, y), 1e-6)),
                "through finish notch vertex ({x}, {y}) missing: {c_thru:?}"
            );
        }
    }
    let s_thru = wall_layer_solid_wire_base_points(&scene, thru);
    assert_eq!(s_thru.len(), c_thru.len());
    for i in 0..c_thru.len() {
        assert_point_sets_match(&c_thru[i], &s_thru[i], 1e-4, "double X through contour vs solid base");
    }
    for (h, x) in [(c6, 6.0), (c14, 14.0)] {
        let rings = wall_layer_contour_loops(&scene, h);
        assert_eq!(rings.len(), 6, "crossing wall at x={x} split into two stems per layer: {rings:?}");
        for y in [-0.1, 0.1] {
            assert!(
                ring_with_points(&rings, &[(x - 0.1, y), (x + 0.1, y)]),
                "crossing core at x={x} must stop at through core face y={y}: {rings:?}"
            );
        }
    }

    // Through wall last: both (earlier) crossing walls win the tie and the
    // 20 m wall is the one split, 3 layers x 3 pieces.
    let mut scene_last = Scene::new();
    let [thru_last, c6_last, _] = build_double_x_scene(&mut scene_last, [2, 1, 0], 0);
    assert_eq!(wall_layer_contour_loops(&scene_last, thru_last).len(), 9);
    let c6_rings = wall_layer_contour_loops(&scene_last, c6_last);
    assert!(
        ring_with_points(&c6_rings, &[(5.9, -5.0), (5.9, 5.0), (6.1, 5.0), (6.1, -5.0)]),
        "earlier crossing wall keeps a continuous core: {c6_rings:?}"
    );

    for order in [[0, 1, 2], [2, 1, 0], [1, 0, 2]] {
        let mut base = Scene::new();
        let base_handles = build_double_x_scene(&mut base, order, 0);
        let mut shifted = Scene::new();
        let shifted_handles = build_double_x_scene(&mut shifted, order, 7);
        for k in 0..3 {
            let expected = normalized_contour_rings(&base, base_handles[k]);
            assert!(!expected.is_empty());
            assert_eq!(
                normalized_contour_rings(&shifted, shifted_handles[k]),
                expected,
                "wall {k} differs with interleaved far decoys for order {order:?}"
            );
        }
    }
}

/// Polyline through wall (L shape) crossed on both legs: priority and the
/// per-leg notches must not depend on the absolute global document index.
#[test]
fn plain_regen_polyline_x_priority_is_independent_of_far_walls() {
    fn build(order: [usize; 3], decoys: usize) -> (Scene, [Handle; 3]) {
        let mut scene = Scene::new();
        let mut out = [Handle::NULL; 3];
        for (n, &k) in order.iter().enumerate() {
            add_far_decoy_walls(&mut scene, decoys, 500.0 + 10.0 * n as f64);
            out[k] = match k {
                0 => add_polyline_layered_wall(
                    &mut scene,
                    &[(0.0, 0.0), (20.0, 0.0), (20.0, 20.0)],
                    x_scan_layers(),
                ),
                1 => add_layered_wall(&mut scene, (6.0, -5.0), (6.0, 5.0), x_scan_layers()),
                _ => add_layered_wall(&mut scene, (15.0, 10.0), (25.0, 10.0), x_scan_layers()),
            };
        }
        for h in all_wall_axis_handles(&scene) {
            regenerate_wall_representation(&mut scene, h, None).unwrap();
        }
        (scene, out)
    }

    let (scene, [poly, c6, ch]) = build([0, 1, 2], 0);
    let c_poly = wall_layer_contour_loops(&scene, poly);
    assert_eq!(c_poly.len(), 7, "{c_poly:?}");
    assert!(
        ring_with_points(
            &c_poly,
            &[(0.0, -0.1), (20.1, -0.1), (20.1, 20.0), (19.9, 20.0), (19.9, 0.1), (0.0, 0.1)],
        ),
        "polyline core keeps priority and stays continuous over both legs: {c_poly:?}"
    );
    assert!(c_poly.iter().any(|r| has_point(r, (20.12, 9.88), 1e-6)));
    assert!(c_poly.iter().any(|r| has_point(r, (5.88, -0.12), 1e-6)));
    let c_ch = wall_layer_contour_loops(&scene, ch);
    assert_eq!(c_ch.len(), 6, "{c_ch:?}");
    assert!(ring_with_points(&c_ch, &[(19.9, 9.9), (19.9, 10.1)]), "{c_ch:?}");
    assert!(ring_with_points(&c_ch, &[(20.1, 9.9), (20.1, 10.1)]), "{c_ch:?}");
    assert_eq!(wall_layer_contour_loops(&scene, c6).len(), 6);

    for order in [[0, 1, 2], [2, 1, 0], [1, 0, 2]] {
        let (base, base_handles) = build(order, 0);
        let (shifted, shifted_handles) = build(order, 6);
        for k in 0..3 {
            let expected = normalized_contour_rings(&base, base_handles[k]);
            assert!(!expected.is_empty());
            assert_eq!(
                normalized_contour_rings(&shifted, shifted_handles[k]),
                expected,
                "wall {k} differs with interleaved far decoys for order {order:?}"
            );
        }
    }
}

/// The local candidate set is bounded to touching walls, keeps document
/// order (self index may be anything) and is `None` for isolated walls.
#[test]
fn local_junction_scan_is_bounded_and_keeps_document_order() {
    let mut scene = Scene::new();
    let decoys = add_far_decoy_walls(&mut scene, 3, 600.0);
    let [thru, c6, c14] = build_double_x_scene(&mut scene, [1, 0, 2], 0);
    let tail = add_far_decoy_walls(&mut scene, 2, 700.0);
    let axis2d = |s: &Scene, h: Handle| -> Vec<(f64, f64)> {
        get_wall_vertices(s, h).iter().map(|p| (p.x, p.y)).collect()
    };

    let scan = local_junction_scan(&scene, thru, &axis2d(&scene, thru)).expect("thru has neighbors");
    assert_eq!(scan.handles, vec![c6, thru, c14]);
    assert_eq!(scan.self_i, 1);
    assert_eq!(scan.axes.len(), 3);
    assert_eq!(scan.bulges.len(), 3);

    let scan = local_junction_scan(&scene, c14, &axis2d(&scene, c14)).expect("c14 crosses thru");
    assert_eq!(scan.handles, vec![thru, c14], "c6 is 8 units away and must be skipped");
    assert_eq!(scan.self_i, 1);

    let scan = local_junction_scan(&scene, c6, &axis2d(&scene, c6)).expect("c6 crosses thru");
    assert_eq!(scan.handles, vec![c6, thru]);
    assert_eq!(scan.self_i, 0);

    // Decoys are 7 apart along y = 500 with length 5: nothing within margin.
    for h in decoys.iter().chain(tail.iter()) {
        assert!(local_junction_scan(&scene, *h, &axis2d(&scene, *h)).is_none());
    }

    // A wall inside the overall bbox of an L polyline but far from both legs
    // is filtered segment-wise.
    let mut scene = Scene::new();
    let poly = add_polyline_layered_wall(&mut scene, &[(0.0, 0.0), (20.0, 0.0), (20.0, 20.0)], x_scan_layers());
    let inner = add_layered_wall(&mut scene, (10.0, 10.0), (12.0, 10.0), x_scan_layers());
    assert!(local_junction_scan(&scene, poly, &axis2d(&scene, poly)).is_none());
    assert!(local_junction_scan(&scene, inner, &axis2d(&scene, inner)).is_none());
    regenerate_wall_representation(&mut scene, poly, None).unwrap();
    assert_eq!(wall_layer_contour_loops(&scene, poly).len(), 3);
}

/// A T stem ending on the arc of a curved through wall lies far outside the
/// axis chord box; the bulge-aware scan must still find it so plain regen
/// keeps the approach-side finish cutout.
#[test]
fn plain_regen_t_stem_on_arc_found_by_bulge_aware_scan() {
    let mut scene = Scene::new();
    // Chord (0,5)-(10,5), bulge 0.9 → sagitta 4.5, arc bottom at (5, 0.5).
    let arc = add_bulged_layered_wall(&mut scene, (0.0, 5.0), (10.0, 5.0), 0.9, x_scan_layers());
    let far = add_layered_wall(&mut scene, (40.0, 0.0), (45.0, 0.0), x_scan_layers());
    regenerate_wall_representation(&mut scene, arc, None).unwrap();
    assert_eq!(wall_layer_contour_loops(&scene, arc).len(), 3);

    let stem = add_layered_wall(&mut scene, (5.0, -3.0), (5.0, 0.5), x_scan_layers());
    let chord_box = axis_aabb_2d(&get_wall_vertices(&scene, arc));
    let stem_box = axis_aabb_2d(&get_wall_vertices(&scene, stem));
    assert!(
        !aabb_overlaps_inflated(chord_box, stem_box, WALL_JOIN_SNAP_RADIUS),
        "precondition: a chord-only box filter would miss the stem"
    );
    let arc_axis: Vec<(f64, f64)> = get_wall_vertices(&scene, arc).iter().map(|p| (p.x, p.y)).collect();
    let scan = local_junction_scan(&scene, arc, &arc_axis).expect("stem touches the arc");
    assert_eq!(scan.handles, vec![arc, stem]);
    assert!(!scan.handles.contains(&far));

    regenerate_wall_representation(&mut scene, stem, None).unwrap();
    regenerate_wall_representation(&mut scene, arc, None).unwrap();
    assert_eq!(
        wall_layer_contour_loops(&scene, arc).len(),
        4,
        "approach-side finish of the arc wall must be cut by the stem"
    );
}

pub(super) fn add_single_layer_wall(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64)) -> Handle {
    add_layered_wall(scene, p1, p2, vec![wl("Concrete", 0.2, "Structural")])
}

pub(super) fn add_layered_wall(
    scene: &mut Scene,
    p1: (f64, f64),
    p2: (f64, f64),
    layers: Vec<WallLayer>,
) -> Handle {
    add_polyline_layered_wall(scene, &[p1, p2], layers)
}

pub(super) fn add_polyline_layered_wall(
    scene: &mut Scene,
    points: &[(f64, f64)],
    layers: Vec<WallLayer>,
) -> Handle {
    let mut pl = LwPolyline::new();
    for pt in points {
        pl.add_vertex(LwVertex::new(Vector2::new(pt.0, pt.1)));
    }
    let mut entity = EntityType::LwPolyline(pl);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record(
        "style1",
        3.0,
        0,
        &layers,
        &[],
        WallJustification::Center,
        PlanPhase::New,
        None,
    );
    entity.common_mut().extended_data.add_record(record);
    scene.add_entity(entity)
}

/// Regression test for the "other end's join reverts to a plain cap"
/// bug: Wall A is joined to Wall B at A's end 1 (10,0), producing a
/// correctly mitered L-corner there (exact corner coordinates match the
/// `l_corner_miter_shares_diagonal_endpoints` geometry in `miter.rs`).
/// A NEW Wall C is then joined to A's *other* end (0,0). After that
/// second join, A's rendered footprint must still contain the original
/// A-B miter corners *and* a real (non-plain-cap) miter at the A-C end.
#[test]
fn other_end_join_survives_new_join_at_opposite_end() {
    let mut scene = Scene::new();
    let wall_a = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let wall_b = add_single_layer_wall(&mut scene, (10.0, 0.0), (10.0, 10.0));
    regenerate_wall_representation(&mut scene, wall_a, None).expect("regen a");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("regen b");

    join_two_walls_in_document(&mut scene,  wall_a,  wall_b,  None,  None,  None).expect("A-B join");

    // Plain (un-joined) end-0 cap footprint corners for reference.
    let plain_end0 = [(0.0, -0.1), (0.0, 0.1)];
    let corner1 = (10.1, -0.1);
    let corner2 = (9.9, 0.1);

    let pts_before = wall_contour_points(&scene, wall_a);
    assert!(
        has_point(&pts_before, corner1, 1e-6) && has_point(&pts_before, corner2, 1e-6),
        "A-B miter corners must be present right after the first join, got {pts_before:?}"
    );

    // NEW wall C joins A's other end (0,0).
    let wall_c = add_single_layer_wall(&mut scene, (0.0, 0.0), (0.0, -10.0));
    regenerate_wall_representation(&mut scene, wall_c, None).expect("regen c");
    join_two_walls_in_document(&mut scene,  wall_a,  wall_c,  None,  None,  None).expect("A-C join");

    let pts_after = wall_contour_points(&scene, wall_a);
    assert!(
        has_point(&pts_after, corner1, 1e-6) && has_point(&pts_after, corner2, 1e-6),
        "A-B miter corners must survive regeneration triggered by the NEW A-C join, got {pts_after:?}"
    );
    assert!(
        !plain_end0.iter().all(|p| has_point(&pts_after, *p, 1e-6)),
        "end 0 must show a real miter against C, not the plain unjoined cap, got {pts_after:?}"
    );
}

/// N-way variant of the regression above: three walls already meet at
/// one point via `join_junction_in_document`; a fourth wall then joins
/// one of them at *its other end*. Both ends of the middle wall must
/// stay correctly joined afterward.
#[test]
fn n_way_junction_survives_new_join_at_participants_other_end() {
    let mut scene = Scene::new();
    // Genuine (non-collinear) 3-way junction at (0,0): w1 along +X,
    // w2 along +Y, w3 along a third direction — each participant gets
    // a real diagonal miter at the shared point, not a straight-through
    // continuation.
    let w1 = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let w2 = add_single_layer_wall(&mut scene, (0.0, 0.0), (0.0, 10.0));
    let w3 = add_single_layer_wall(&mut scene, (0.0, 0.0), (-10.0, 10.0));
    for h in [w1, w2, w3] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }
    join_junction_in_document(&mut scene,  &[w1, w2, w3],  None,  None,  None,  None).expect("N-way join");

    // Snapshot every vertex near the junction corner (close to the
    // origin) before the new join is introduced.
    let pts_before = wall_contour_points(&scene, w1);
    let near_origin_before: Vec<(f64, f64)> =
        pts_before.into_iter().filter(|p| p.0 < 5.0).collect();
    assert!(
        !near_origin_before.is_empty(),
        "sanity: w1 should have a real N-way miter near the junction"
    );
    let plain_end1 = [(10.0, -0.1), (10.0, 0.1)];
    assert!(
        plain_end1.iter().all(|p| has_point(&wall_contour_points(&scene, w1), *p, 1e-6)),
        "sanity: w1's un-joined end should still be a plain cap before the new join"
    );

    // NEW wall w4 joins w1 at ITS other end (10,0).
    let w4 = add_single_layer_wall(&mut scene, (10.0, 0.0), (10.0, 10.0));
    regenerate_wall_representation(&mut scene, w4, None).expect("regen w4");
    join_two_walls_in_document(&mut scene,  w1,  w4,  None,  None,  None).expect("w1-w4 join");

    let pts_after = wall_contour_points(&scene, w1);
    assert!(
        !plain_end1.iter().all(|p| has_point(&pts_after, *p, 1e-6)),
        "w1's new join end must be a real miter, not the plain cap"
    );
    // The original N-way junction corner geometry (near x=0) must be
    // byte-for-byte preserved after this unrelated join event elsewhere.
    for p in &near_origin_before {
        assert!(
            has_point(&pts_after, *p, 1e-9),
            "w1's original N-way junction corner point {p:?} must survive the new A-B join, got {pts_after:?}"
        );
    }
}

/// A wall with only ONE join (no second join at all) must keep producing
/// exactly the same mitered footprint as before this change — no
/// accidental behavior change for the common single-join case.
#[test]
fn single_join_wall_footprint_unchanged() {
    let mut scene = Scene::new();
    let wall_a = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let wall_b = add_single_layer_wall(&mut scene, (10.0, 0.0), (10.0, 10.0));
    regenerate_wall_representation(&mut scene, wall_a, None).expect("regen a");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("regen b");

    join_two_walls_in_document(&mut scene,  wall_a,  wall_b,  None,  None,  None).expect("A-B join");

    let pts = wall_contour_points(&scene, wall_a);
    let corner1 = (10.1, -0.1);
    let corner2 = (9.9, 0.1);
    assert!(has_point(&pts, corner1, 1e-6) && has_point(&pts, corner2, 1e-6));

    // Plain cap at the un-joined end 0 must be exactly the un-mitered
    // rectangle corners (no peer exists there).
    assert!(has_point(&pts, (0.0, -0.1), 1e-6));
    assert!(has_point(&pts, (0.0, 0.1), 1e-6));
}

/// Regression test for the reported bug: three walls already meet at one
/// point via an N-way junction (w1, w2, w3, all correctly mitered).
/// A NEW wall w4 is then joined to the *same* junction point. After the
/// junction is re-resolved for 4 participants, the *other* walls (w2, w3)
/// — which did not change themselves — must still show their correct
/// mitered footprint, not revert to a plain unjoined cap.
#[test]
fn adding_new_wall_to_existing_junction_keeps_other_walls_mitered() {
    let mut scene = Scene::new();
    let w1 = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let w2 = add_single_layer_wall(&mut scene, (0.0, 0.0), (0.0, 10.0));
    let w3 = add_single_layer_wall(&mut scene, (0.0, 0.0), (-10.0, 10.0));
    for h in [w1, w2, w3] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }
    join_junction_in_document(&mut scene,  &[w1, w2, w3],  None,  None,  None,  None).expect("N-way join");

    // Plain (un-joined) cap corners at the origin end of a vertical /
    // diagonal single-layer (0.2 thick) wall — what w2/w3 would show at
    // their origin end if the junction miter were lost and they fell
    // back to an un-joined rectangle cap.
    let plain_origin_cap = [(-0.1, 0.0), (0.1, 0.0)];

    let w2_before = wall_contour_points(&scene, w2);
    let w3_before = wall_contour_points(&scene, w3);
    assert!(
        !plain_origin_cap.iter().all(|p| has_point(&w2_before, *p, 1e-6)),
        "sanity: w2 should have a real N-way miter, not a plain cap, got {w2_before:?}"
    );
    assert!(
        !plain_origin_cap.iter().all(|p| has_point(&w3_before, *p, 1e-6)),
        "sanity: w3 should have a real N-way miter, not a plain cap, got {w3_before:?}"
    );

    // NEW wall w4 joins the SAME junction point (0,0), the way the
    // interactive drawing workflow actually triggers it: via
    // `try_auto_join_nearby_walls` for just the newly drawn wall.
    let w4 = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, -10.0));
    regenerate_wall_representation(&mut scene, w4, None).expect("regen w4");
    try_auto_join_nearby_walls(&mut scene,  w4,  None,  None,  None);

    let w2_after = wall_contour_points(&scene, w2);
    let w3_after = wall_contour_points(&scene, w3);
    assert!(
        !plain_origin_cap.iter().all(|p| has_point(&w2_after, *p, 1e-6)),
        "w2 must still show a real miter at the junction after w4 joins, not revert to a plain cap, got {w2_after:?}"
    );
    assert!(
        !plain_origin_cap.iter().all(|p| has_point(&w3_after, *p, 1e-6)),
        "w3 must still show a real miter at the junction after w4 joins, not revert to a plain cap, got {w3_after:?}"
    );
}

#[test]
fn l_corner_with_window_keeps_miter_in_2d_and_3d() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];

    // Wall 1: horizontal along Y=0 from X=0 to X=10.
    let mut pl1 = LwPolyline::new();
    pl1.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl1.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut ent1 = EntityType::LwPolyline(pl1);
    let mut rec1 = ExtendedDataRecord::new(AEC_APPID);
    rec1.values = wall_record("s1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent1.common_mut().extended_data.add_record(rec1);
    let w1 = scene.add_entity(ent1);

    // Wall 2: vertical along X=0 from Y=0 to Y=10.
    let mut pl2 = LwPolyline::new();
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 10.0)));
    let mut ent2 = EntityType::LwPolyline(pl2);
    let mut rec2 = ExtendedDataRecord::new(AEC_APPID);
    rec2.values = wall_record("s2", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent2.common_mut().extended_data.add_record(rec2);
    let w2 = scene.add_entity(ent2);

    // Join w1 and w2 as an L-corner at (0, 0).
    let mut command_line = CommandLine::default();
    aec_walljoin_do(
        &mut scene,
        &mut command_line,
        &format!("{}|{}", w1.value(), w2.value()),
        None, None, None,
    );

    // Place a window on w1 at X=5.0 (width 1.2m).
    let (_window_h, _) = place_wall_opening(
        &mut scene,
        w1,
        DVec3::new(5.0, 0.0, 0.0),
        engine::openings::OpeningKind::Window,
        None, None, None,
    ).expect("place window");

    // Regenerate w1 with its miter preserved.
    regenerate_wall_representation(&mut scene, w1, None).expect("regen w1");

    let wall_rec = wall_from_entity(scene.document.get_entity(w1).unwrap()).unwrap();

    // Check 2D contours: must have 2 pieces (one from corner to window, one from window to end).
    let mut contours = Vec::new();
    let mut solids = Vec::new();
    for h in &wall_rec.derived_handles {
        match scene.document.get_entity(*h) {
            Some(EntityType::LwPolyline(pl)) => contours.push(pl.clone()),
            Some(EntityType::Solid3D(s3d)) => solids.push(s3d.clone()),
            _ => {}
        }
    }

    assert_eq!(contours.len(), 2, "Wall with window must have 2 contour pieces");
    // The start piece must have a miter vertex (at -0.1, -0.1).
    let has_miter_vertex_2d = contours.iter().any(|pl| {
        pl.vertices.iter().any(|v| v.location.x < -0.05 && v.location.y < -0.05)
    });
    assert!(has_miter_vertex_2d, "2D contour must retain miter corner vertex at (-0.1, -0.1)");

    // Check 3D solids: rest wall solids (2 pieces) plus opening zone solids (lintel + sill).
    assert!(solids.len() >= 2, "3D wall must have at least 2 rest-wall solids");

    // The first 3D rest-wall solid must have wires reaching the corner miter vertex (-0.1, -0.1).
    let has_miter_wire_3d = solids.iter().any(|s| {
        s.wires.iter().any(|wire| {
            wire.points.iter().any(|p| p.x < -0.05 && p.y < -0.05)
        })
    });
    assert!(has_miter_wire_3d, "3D solid must retain miter corner geometry at (-0.1, -0.1)");
}

#[test]
fn l_corner_same_material_suppresses_miter_in_2d_contour_while_hatch_stays_closed() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];

    let mut pl1 = LwPolyline::new();
    pl1.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl1.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut ent1 = EntityType::LwPolyline(pl1);
    let mut rec1 = ExtendedDataRecord::new(AEC_APPID);
    rec1.values = wall_record("s1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent1.common_mut().extended_data.add_record(rec1);
    let w1 = scene.add_entity(ent1);

    let mut pl2 = LwPolyline::new();
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 5.0)));
    let mut ent2 = EntityType::LwPolyline(pl2);
    let mut rec2 = ExtendedDataRecord::new(AEC_APPID);
    rec2.values = wall_record("s2", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent2.common_mut().extended_data.add_record(rec2);
    let w2 = scene.add_entity(ent2);

    let mut command_line = CommandLine::default();
    aec_walljoin_do(
        &mut scene,
        &mut command_line,
        &format!("{}|{}", w1.value(), w2.value()),
        None, None, None,
    );

    let wall1 = wall_from_entity(scene.document.get_entity(w1).unwrap()).unwrap();
    let wall2 = wall_from_entity(scene.document.get_entity(w2).unwrap()).unwrap();

    let mut w1_contours = Vec::new();
    let mut w1_has_hatch = false;
    for h in &wall1.derived_handles {
        match scene.document.get_entity(*h) {
            Some(EntityType::LwPolyline(pl)) => w1_contours.push(pl.clone()),
            Some(EntityType::Hatch(_)) => w1_has_hatch = true,
            _ => {}
        }
    }
    let mut w2_contours = Vec::new();
    for h in &wall2.derived_handles {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*h) {
            w2_contours.push(pl.clone());
        }
    }

    assert!(!w1_contours.is_empty(), "Wall 1 must have 2D contour");
    assert!(!w2_contours.is_empty(), "Wall 2 must have 2D contour");
    assert!(w1_has_hatch, "Wall 1 must have Hatch entity");

    // Both contours must be open polylines because the miter edge was suppressed.
    assert!(!w1_contours[0].is_closed, "Wall 1 contour must be an open polyline (no miter line)");
    assert!(!w2_contours[0].is_closed, "Wall 2 contour must be an open polyline (no miter line)");

    // Neither contour should draw a segment between (-0.1, -0.1) and (0.1, 0.1).
    let draws_miter_segment = |pl: &LwPolyline| {
        let n = pl.vertices.len();
        let limit = if pl.is_closed { n } else { n - 1 };
        for i in 0..limit {
            let p0 = pl.vertices[i].location;
            let p1 = pl.vertices[(i + 1) % n].location;
            let d00 = (p0.x - -0.1).hypot(p0.y - -0.1);
            let d11 = (p1.x - 0.1).hypot(p1.y - 0.1);
            let d01 = (p0.x - 0.1).hypot(p0.y - 0.1);
            let d10 = (p1.x - -0.1).hypot(p1.y - -0.1);
            if (d00 < 1e-4 && d11 < 1e-4) || (d01 < 1e-4 && d10 < 1e-4) {
                return true;
            }
        }
        false
    };

    assert!(
        !draws_miter_segment(&w1_contours[0]),
        "Wall 1 must NOT draw the diagonal miter edge across identical material"
    );
    assert!(
        !draws_miter_segment(&w2_contours[0]),
        "Wall 2 must NOT draw the diagonal miter edge across identical material"
    );
}

#[test]
fn l_corner_different_materials_keeps_miter_boundary_line() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    let layers1 = vec![wl("Concrete", 0.2, "Structural")];
    let layers2 = vec![wl("Brick", 0.2, "Structural")];

    let mut pl1 = LwPolyline::new();
    pl1.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl1.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut ent1 = EntityType::LwPolyline(pl1);
    let mut rec1 = ExtendedDataRecord::new(AEC_APPID);
    rec1.values = wall_record("s1", 3.0, 0, &layers1, &[], WallJustification::Center, PlanPhase::New, None);
    ent1.common_mut().extended_data.add_record(rec1);
    let w1 = scene.add_entity(ent1);

    let mut pl2 = LwPolyline::new();
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 5.0)));
    let mut ent2 = EntityType::LwPolyline(pl2);
    let mut rec2 = ExtendedDataRecord::new(AEC_APPID);
    rec2.values = wall_record("s2", 3.0, 0, &layers2, &[], WallJustification::Center, PlanPhase::New, None);
    ent2.common_mut().extended_data.add_record(rec2);
    let w2 = scene.add_entity(ent2);

    let mut command_line = CommandLine::default();
    aec_walljoin_do(
        &mut scene,
        &mut command_line,
        &format!("{}|{}", w1.value(), w2.value()),
        None, None, None,
    );

    let wall1 = wall_from_entity(scene.document.get_entity(w1).unwrap()).unwrap();
    let mut w1_contours = Vec::new();
    for h in &wall1.derived_handles {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*h) {
            w1_contours.push(pl.clone());
        }
    }

    assert!(!w1_contours.is_empty(), "Wall 1 must have 2D contour");
    assert!(
        w1_contours[0].is_closed,
        "Wall 1 contour must remain a closed polyline to show the material boundary between Concrete and Brick"
    );
}

#[test]
fn wall_with_window_3d_cleans_coplanar_seams_while_preserving_reveal_edges() {
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];

    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    // Place window at X=5.0, width 1.2 (span 4.4 to 5.6), sill 0.9, height 1.3 (head 2.2).
    let (_win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(5.0, 0.0, 0.0),
        engine::openings::OpeningKind::Window,
        None, None, None,
    ).expect("place window");

    regenerate_wall_representation(&mut scene, wall_h, None).expect("regen wall");

    let wall_rec = wall_from_entity(scene.document.get_entity(wall_h).unwrap()).unwrap();
    let mut solids = Vec::new();
    for h in &wall_rec.derived_handles {
        if let Some(EntityType::Solid3D(s3d)) = scene.document.get_entity(*h) {
            solids.push(s3d.clone());
        }
    }

    assert!(solids.len() >= 3, "Wall with window must have rest solids and zone solids");

    // Collect all wire segments from all 3D solids of the wall.
    let mut all_segments: Vec<([f64; 3], [f64; 3])> = Vec::new();
    for s in &solids {
        for w in &s.wires {
            for win in w.points.windows(2) {
                all_segments.push(([win[0].x, win[0].y, win[0].z], [win[1].x, win[1].y, win[1].z]));
            }
        }
    }

    // Check for coplanar vertical seams at X=4.4 or X=5.6:
    // Any vertical segment below the sill (z_max <= 0.89) or above the lintel (z_min >= 2.21)
    // at X near 4.4 or 5.6 must be suppressed.
    let has_vertical_seam_below_sill = all_segments.iter().any(|(p0, p1)| {
        let is_at_cut = (p0[0] - 4.4).abs() < 1e-3 || (p0[0] - 5.6).abs() < 1e-3;
        let is_vertical = (p1[0] - p0[0]).hypot(p1[1] - p0[1]) < 1e-3 && (p1[2] - p0[2]).abs() > 0.05;
        let z_max = p0[2].max(p1[2]);
        is_at_cut && is_vertical && z_max <= 0.89
    });
    assert!(
        !has_vertical_seam_below_sill,
        "Vertical coplanar seams below the window sill (z <= 0.9) must be suppressed in 3D"
    );

    let has_vertical_seam_above_head = all_segments.iter().any(|(p0, p1)| {
        let is_at_cut = (p0[0] - 4.4).abs() < 1e-3 || (p0[0] - 5.6).abs() < 1e-3;
        let is_vertical = (p1[0] - p0[0]).hypot(p1[1] - p0[1]) < 1e-3 && (p1[2] - p0[2]).abs() > 0.05;
        let z_min = p0[2].min(p1[2]);
        is_at_cut && is_vertical && z_min >= 2.21
    });
    assert!(
        !has_vertical_seam_above_head,
        "Vertical coplanar seams above the window lintel (z >= 2.2) must be suppressed in 3D"
    );

    // Check that window reveal edges (jambs between z=0.9 and 2.2) ARE preserved:
    let has_reveal_jamb = all_segments.iter().any(|(p0, p1)| {
        let is_at_cut = (p0[0] - 4.4).abs() < 1e-3 || (p0[0] - 5.6).abs() < 1e-3;
        let is_vertical = (p1[0] - p0[0]).hypot(p1[1] - p0[1]) < 1e-3 && (p1[2] - p0[2]).abs() > 0.1;
        let z_min = p0[2].min(p1[2]);
        let z_max = p0[2].max(p1[2]);
        is_at_cut && is_vertical && z_min >= 0.89 && z_max <= 2.21
    });
    assert!(
        has_reveal_jamb,
        "Window reveal jamb edges (between z=0.9 and 2.2) must be preserved and sharp"
    );

    // Check that horizontal reveal edges (sill top z=0.9 across wall thickness) are preserved:
    let has_sill_top_edge = all_segments.iter().any(|(p0, p1)| {
        let is_at_cut = (p0[0] - 4.4).abs() < 1e-3 || (p0[0] - 5.6).abs() < 1e-3;
        let is_transverse = (p1[1] - p0[1]).abs() > 0.1;
        let is_at_sill = (p0[2] - 0.9).abs() < 1e-3 && (p1[2] - 0.9).abs() < 1e-3;
        is_at_cut && is_transverse && is_at_sill
    });
    assert!(
        has_sill_top_edge,
        "Window sill top horizontal edge across wall thickness must be preserved"
    );
}

#[test]
fn wall_with_arch_window_3d_cleans_coplanar_seams_while_preserving_reveal_edges() {
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];

    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    // Place window at X=5.0, width 1.2 (span 4.4 to 5.6), sill 0.9, height 1.3, Arch with spring 0.7.
    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(5.0, 0.0, 0.0),
        engine::openings::OpeningKind::Window,
        None, None, None,
    ).expect("place window");

    let mut opening = engine::opening_xdata::openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("opening found");
    opening.shape = engine::opening_shape::OpeningShape::Arch;
    opening.spring_height = 0.7;
    engine::opening_xdata::write_opening_instance(&mut scene, &opening);

    regenerate_wall_representation(&mut scene, wall_h, None).expect("regen wall");

    let wall_rec = wall_from_entity(scene.document.get_entity(wall_h).unwrap()).unwrap();
    let mut solids = Vec::new();
    for h in &wall_rec.derived_handles {
        if let Some(EntityType::Solid3D(s3d)) = scene.document.get_entity(*h) {
            solids.push(s3d.clone());
        }
    }

    assert!(solids.len() >= 3, "Wall with arch window must have rest solids and zone solids");

    let mut all_segments: Vec<([f64; 3], [f64; 3])> = Vec::new();
    for s in &solids {
        for w in &s.wires {
            for win in w.points.windows(2) {
                all_segments.push(([win[0].x, win[0].y, win[0].z], [win[1].x, win[1].y, win[1].z]));
            }
        }
    }

    // Check for coplanar vertical seams above the arch lintel in the span (4.4 < X < 5.6):
    let has_internal_vertical_seam_above_arch = all_segments.iter().any(|(p0, p1)| {
        let is_inside_zone = p0[0] > 4.41 && p0[0] < 5.59;
        let is_vertical = (p1[0] - p0[0]).hypot(p1[1] - p0[1]) < 1e-3 && (p1[2] - p0[2]).abs() > 0.05;
        is_inside_zone && is_vertical
    });
    assert!(
        !has_internal_vertical_seam_above_arch,
        "All internal vertical slice seams inside the arch opening zone must be suppressed in 3D"
    );

    // Check that transverse crown lines across top deck (z=3.0) inside the zone (4.4 < X < 5.6) are suppressed:
    let has_transverse_crown_seam = all_segments.iter().any(|(p0, p1)| {
        let is_inside_zone = p0[0] > 4.41 && p0[0] < 5.59;
        let is_at_crown = (p0[2] - 3.0).abs() < 1e-3 && (p1[2] - 3.0).abs() < 1e-3;
        let is_transverse = (p1[1] - p0[1]).abs() > 0.05;
        is_inside_zone && is_at_crown && is_transverse
    });
    assert!(
        !has_transverse_crown_seam,
        "Transverse crown seam lines across the top deck of the arch window must be suppressed"
    );

    // Check that arch reveal edges (curved arc chords) ARE preserved:
    let has_arch_curve = all_segments.iter().any(|(p0, p1)| {
        let is_inside_zone = p0[0] > 4.4 && p0[0] < 5.6 && p1[0] > 4.4 && p1[0] < 5.6;
        let dx = (p1[0] - p0[0]).abs();
        let at_arch_z = p0[2] >= 1.59 && p0[2] <= 2.21 && p1[2] >= 1.59 && p1[2] <= 2.21;
        is_inside_zone && dx > 0.01 && at_arch_z
    });
    assert!(
        has_arch_curve,
        "Arch curve reveal edges must be preserved"
    );

    // Check that straight side jambs (between z=0.9 and 1.6) ARE preserved:
    let has_straight_jamb = all_segments.iter().any(|(p0, p1)| {
        let is_at_cut = (p0[0] - 4.4).abs() < 1e-3 || (p0[0] - 5.6).abs() < 1e-3;
        let is_vertical = (p1[0] - p0[0]).hypot(p1[1] - p0[1]) < 1e-3 && (p1[2] - p0[2]).abs() > 0.1;
        let z_min = p0[2].min(p1[2]);
        let z_max = p0[2].max(p1[2]);
        is_at_cut && is_vertical && z_min >= 0.89 && z_max <= 1.61
    });
    assert!(
        has_straight_jamb,
        "Arch straight side jambs below spring height must be preserved"
    );
}

#[test]
fn wall_with_circle_window_3d_cleans_coplanar_seams_while_preserving_reveal_edges() {
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];

    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    // Place window at X=5.0, width 1.2 (span 4.4 to 5.6), sill 0.9, height 1.2, Circle shape.
    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(5.0, 0.0, 0.0),
        engine::openings::OpeningKind::Window,
        None, None, None,
    ).expect("place window");

    let mut opening = engine::opening_xdata::openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("opening found");
    opening.shape = engine::opening_shape::OpeningShape::Circle;
    opening.height = 1.2;
    engine::opening_xdata::write_opening_instance(&mut scene, &opening);

    regenerate_wall_representation(&mut scene, wall_h, None).expect("regen wall");

    let wall_rec = wall_from_entity(scene.document.get_entity(wall_h).unwrap()).unwrap();
    let mut solids = Vec::new();
    for h in &wall_rec.derived_handles {
        if let Some(EntityType::Solid3D(s3d)) = scene.document.get_entity(*h) {
            solids.push(s3d.clone());
        }
    }

    assert!(solids.len() >= 3, "Wall with circle window must have rest solids and zone solids");

    let mut all_segments: Vec<([f64; 3], [f64; 3])> = Vec::new();
    for s in &solids {
        for w in &s.wires {
            for win in w.points.windows(2) {
                all_segments.push(([win[0].x, win[0].y, win[0].z], [win[1].x, win[1].y, win[1].z]));
            }
        }
    }

    // Check that ALL vertical seams across the entire zone [4.4, 5.6] on the wall surface are suppressed:
    let has_vertical_surface_seam = all_segments.iter().any(|(p0, p1)| {
        let is_in_zone = p0[0] >= 4.39 && p0[0] <= 5.61;
        let is_vertical = (p1[0] - p0[0]).hypot(p1[1] - p0[1]) < 1e-3 && (p1[2] - p0[2]).abs() > 0.05;
        let on_face = (p0[1].abs() - 0.1).abs() < 1e-3;
        is_in_zone && is_vertical && on_face
    });
    assert!(
        !has_vertical_surface_seam,
        "All vertical surface seam lines around a circle window must be suppressed"
    );

    // Check that transverse crown lines across top deck (z=3.0) inside the zone are suppressed:
    let has_transverse_crown_seam = all_segments.iter().any(|(p0, p1)| {
        let is_inside_zone = p0[0] >= 4.39 && p0[0] <= 5.61;
        let is_at_crown = (p0[2] - 3.0).abs() < 1e-3 && (p1[2] - 3.0).abs() < 1e-3;
        let is_transverse = (p1[1] - p0[1]).abs() > 0.05;
        is_inside_zone && is_at_crown && is_transverse
    });
    assert!(
        !has_transverse_crown_seam,
        "Transverse crown seam lines across the top deck of the circle window must be suppressed"
    );

    // Check that circular reveal edges ARE preserved:
    let has_circular_reveal = all_segments.iter().any(|(p0, p1)| {
        let is_inside_zone = p0[0] > 4.4 && p0[0] < 5.6 && p1[0] > 4.4 && p1[0] < 5.6;
        let dx = (p1[0] - p0[0]).abs();
        let dz = (p1[2] - p0[2]).abs();
        let at_circle_z = p0[2] >= 0.89 && p0[2] <= 2.11 && p1[2] >= 0.89 && p1[2] <= 2.11;
        is_inside_zone && dx > 0.01 && dz > 0.01 && at_circle_z
    });
    assert!(
        has_circular_reveal,
        "Circular reveal edges must be preserved"
    );
}

#[test]
fn l_corner_same_material_suppresses_3d_deck_miter_fuge_while_different_material_keeps_it() {
    use crate::ui::command_line::CommandLine;

    // --- Scenario A: Same material (Concrete + Concrete) ---
    let mut scene = Scene::new();
    let layers_conc = vec![wl("Concrete", 0.2, "Structural")];

    let mut pl1 = LwPolyline::new();
    pl1.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl1.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut ent1 = EntityType::LwPolyline(pl1);
    let mut rec1 = ExtendedDataRecord::new(AEC_APPID);
    rec1.values = wall_record("s1", 3.0, 0, &layers_conc, &[], WallJustification::Center, PlanPhase::New, None);
    ent1.common_mut().extended_data.add_record(rec1);
    let w1 = scene.add_entity(ent1);

    let mut pl2 = LwPolyline::new();
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 5.0)));
    let mut ent2 = EntityType::LwPolyline(pl2);
    let mut rec2 = ExtendedDataRecord::new(AEC_APPID);
    rec2.values = wall_record("s2", 3.0, 0, &layers_conc, &[], WallJustification::Center, PlanPhase::New, None);
    ent2.common_mut().extended_data.add_record(rec2);
    let w2 = scene.add_entity(ent2);

    let mut command_line = CommandLine::default();
    aec_walljoin_do(
        &mut scene,
        &mut command_line,
        &format!("{}|{}", w1.value(), w2.value()),
        None, None, None,
    );

    let wall1 = wall_from_entity(scene.document.get_entity(w1).unwrap()).unwrap();
    let s1 = wall1.derived_handles.iter().find_map(|h| match scene.document.get_entity(*h) {
        Some(EntityType::Solid3D(s)) => Some(s.clone()),
        _ => None,
    }).expect("Wall 1 must have 3D solid");

    let draws_diagonal_deck_wire = |s: &acadrust::entities::Solid3D, z_level: f64| {
        for w in &s.wires {
            for win in w.points.windows(2) {
                let p0 = &win[0];
                let p1 = &win[1];
                let at_z = (p0.z - z_level).abs() < 1e-3 && (p1.z - z_level).abs() < 1e-3;
                let d0a = (p0.x - -0.1).hypot(p0.y - -0.1);
                let d1b = (p1.x - 0.1).hypot(p1.y - 0.1);
                let d0b = (p0.x - 0.1).hypot(p0.y - 0.1);
                let d1a = (p1.x - -0.1).hypot(p1.y - -0.1);
                if at_z && ((d0a < 1e-3 && d1b < 1e-3) || (d0b < 1e-3 && d1a < 1e-3)) {
                    return true;
                }
            }
        }
        false
    };

    assert!(
        !draws_diagonal_deck_wire(&s1, 3.0),
        "Wall 1 must NOT draw 45-degree diagonal fuge on top deck (z=3.0) when joining same material"
    );
    assert!(
        !draws_diagonal_deck_wire(&s1, 0.0),
        "Wall 1 must NOT draw 45-degree diagonal fuge on bottom base (z=0.0) when joining same material"
    );

    // Verify outer corner vertical wire is still present!
    let has_outer_corner_wire = s1.wires.iter().any(|w| {
        w.points.windows(2).any(|win| {
            let p0 = &win[0];
            let p1 = &win[1];
            (p0.x - -0.1).hypot(p0.y - -0.1) < 1e-3
                && (p1.x - -0.1).hypot(p1.y - -0.1) < 1e-3
                && (p1.z - p0.z).abs() > 2.0
        })
    });
    assert!(has_outer_corner_wire, "Outer vertical corner wire must remain intact and sharp");

    // --- Scenario B: Different materials (Concrete + Brick) ---
    let mut scene2 = Scene::new();
    let layers_brick = vec![wl("Brick", 0.2, "Structural")];

    let mut pl3 = LwPolyline::new();
    pl3.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl3.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut ent3 = EntityType::LwPolyline(pl3);
    let mut rec3 = ExtendedDataRecord::new(AEC_APPID);
    rec3.values = wall_record("s3", 3.0, 0, &layers_conc, &[], WallJustification::Center, PlanPhase::New, None);
    ent3.common_mut().extended_data.add_record(rec3);
    let w3 = scene2.add_entity(ent3);

    let mut pl4 = LwPolyline::new();
    pl4.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl4.add_vertex(LwVertex::new(Vector2::new(0.0, 5.0)));
    let mut ent4 = EntityType::LwPolyline(pl4);
    let mut rec4 = ExtendedDataRecord::new(AEC_APPID);
    rec4.values = wall_record("s4", 3.0, 0, &layers_brick, &[], WallJustification::Center, PlanPhase::New, None);
    ent4.common_mut().extended_data.add_record(rec4);
    let w4 = scene2.add_entity(ent4);

    let mut command_line2 = CommandLine::default();
    aec_walljoin_do(
        &mut scene2,
        &mut command_line2,
        &format!("{}|{}", w3.value(), w4.value()),
        None, None, None,
    );

    let wall3 = wall_from_entity(scene2.document.get_entity(w3).unwrap()).unwrap();
    let s3 = wall3.derived_handles.iter().find_map(|h| match scene2.document.get_entity(*h) {
        Some(EntityType::Solid3D(s)) => Some(s.clone()),
        _ => None,
    }).expect("Wall 3 must have 3D solid");

    assert!(
        draws_diagonal_deck_wire(&s3, 3.0),
        "Wall 3 MUST draw 45-degree diagonal boundary on top deck (z=3.0) when joining DIFFERENT materials"
    );
}
