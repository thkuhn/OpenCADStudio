use super::*;

pub(super) fn loop_bbox(l: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    (
        l.iter().map(|p| p.0).fold(f64::INFINITY, f64::min),
        l.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max),
        l.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
        l.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max),
    )
}

/// U wall (5,-8)→(5,8)→(15,8)→(15,-8) crossing the straight wall (0,0)–(20,0)
/// twice must keep **three pieces per layer** (west leg south of the through
/// wall, the bridge from the first crossing over the top to the second, east
/// leg south of the through wall) — each with its exact extent.
pub(super) fn assert_u_wall_has_all_legs(scene: &Scene, poly: Handle, ctx: &str) {
    let loops = wall_contour_loops(scene, poly);
    assert_eq!(
        loops.len(),
        9,
        "{ctx}: U wall must have 3 pieces x 3 layers = 9 loops, got {}: {loops:?}",
        loops.len()
    );
    let (mut west, mut bridge, mut east) = (0, 0, 0);
    for l in &loops {
        let (min_x, max_x, min_y, max_y) = loop_bbox(l);
        if max_y <= -0.12 + 1e-3 && max_x < 5.5 {
            // south-west leg: x≈5, from y=-8 up to the through-wall face
            assert!((min_y + 8.0).abs() < 1e-3, "{ctx}: west leg must start at y=-8, got {min_y}");
            assert!(min_x > 4.8 && max_x < 5.2, "{ctx}: west leg x extent {min_x}..{max_x}");
            west += 1;
        } else if max_y <= -0.12 + 1e-3 && min_x > 14.5 {
            assert!((min_y + 8.0).abs() < 1e-3, "{ctx}: east leg must start at y=-8, got {min_y}");
            assert!(min_x > 14.8 && max_x < 15.2, "{ctx}: east leg x extent {min_x}..{max_x}");
            east += 1;
        } else if min_y >= 0.12 - 1e-3 {
            assert!(min_x < 5.2 && max_x > 14.8, "{ctx}: bridge must span x 5..15, got {min_x}..{max_x}");
            assert!(max_y > 7.8, "{ctx}: bridge must reach the top near y=8, got {max_y}");
            bridge += 1;
        } else {
            panic!("{ctx}: loop penetrates through core: {l:?}");
        }
    }
    assert_eq!((west, bridge, east), (3, 3, 3), "{ctx}: pieces per class");
}

#[test]
fn x_priority_l_two_leg_crossings_keep_local_finish_pieces() {
    let mut scene = Scene::new();
    let priority = add_polyline_layered_wall(&mut scene, &[(0.0, -5.0), (0.0, 5.0), (10.0, 5.0)],
        wls(&[("Putz", 0.015, "Finish"), ("Mauerwerk", 0.30, "Structural"), ("Putz", 0.015, "Finish")]));
    let a = add_layered_wall(&mut scene, (-3.0, 0.0), (3.0, 0.0), plaster_masonry_plaster());
    let b = add_layered_wall(&mut scene, (5.0, 2.0), (5.0, 8.0), plaster_masonry_plaster());
    for h in [priority, a, b] { regenerate_wall_representation(&mut scene, h, None).unwrap(); }
    let check = |scene: &Scene| {
        let loops = wall_contour_loops(scene, priority);
        assert_eq!(loops.len(), 7, "one core + three pieces on each finish: {loops:?}");
        let boxes: Vec<_> = loops.iter().map(|l| loop_bbox(l)).collect();
        assert!(boxes.iter().any(|&(x0,x1,y0,y1)| x0 < -0.14 && x1 > 9.9 && y0 < -4.9 && y1 > 5.1), "core continuous around L: {boxes:?}");
        for &(x0,x1,y0,y1) in &boxes {
            assert!(x0 >= -0.17 && x1 <= 10.17 && y0 >= -5.01 && y1 <= 5.17, "no diagonal strip: {boxes:?}");
            assert!(x1 > 9.9 && y0 < -4.9 || !(x1 > 2.0 && y0 < -0.30), "finish must follow a leg, not cut diagonally across L: {boxes:?}");
            if !(x1 > 9.9 && y0 < -4.9) {
                assert!(y1 <= -0.12 + 1e-4 || y0 >= 0.12 - 1e-4,
                    "priority finish must not overlap the crossing core band: {boxes:?}");
            }
        }
        assert_eq!(boxes.iter().filter(|b| b.2 < -4.9 && b.1 < 1.0).count(), 2, "two lower finish pieces: {boxes:?}");
        assert_eq!(boxes.iter().filter(|b| b.0 > 5.1).count(), 2, "two right finish pieces: {boxes:?}");
    };
    assert_eq!(join_two_walls_in_document(&mut scene, priority, a, None, None, None).unwrap().0, JoinKind::X);
    assert_eq!(join_two_walls_in_document(&mut scene, priority, b, None, None, None).unwrap().0, JoinKind::X);
    check(&scene);
    for h in [priority, a, b] { regenerate_wall_representation(&mut scene, h, None).unwrap(); }
    check(&scene);
}

#[test]
fn x_priority_u_two_leg_crossings_notch_both_finishes() {
    for swap in [false, true] {
        let mut scene = Scene::new();
        let priority = add_polyline_layered_wall(&mut scene,
            &[(5.0, -8.0), (5.0, 8.0), (15.0, 8.0), (15.0, -8.0)],
            wls(&[("Putz", 0.015, "Finish"), ("Mauerwerk", 0.30, "Structural"), ("Putz", 0.015, "Finish")]));
        let cross = add_layered_wall(&mut scene, (0.0, 0.0), (20.0, 0.0), plaster_masonry_plaster());
        for h in [priority, cross] { regenerate_wall_representation(&mut scene, h, None).unwrap(); }
        let (a,b) = if swap { (cross, priority) } else { (priority, cross) };
        assert_eq!(join_two_walls_in_document(&mut scene, a, b, None, None, None).unwrap().0, JoinKind::X);
        let check = |scene: &Scene| {
            let loops = wall_contour_loops(scene, priority);
            assert_eq!(loops.len(), 7, "core + 3 pieces per finish: {loops:?}");
            let boxes: Vec<_> = loops.iter().map(|l| loop_bbox(l)).collect();
            for &(x0,x1,y0,y1) in &boxes {
                if x0 < 4.86 && x1 > 15.14 && y0 < -7.9 && y1 > 8.0 { continue; }
                assert!(y1 <= -0.12 + 1e-4 || y0 >= 0.12 - 1e-4,
                    "priority finish must not overlap either crossing core: {boxes:?}");
            }
            assert_eq!(boxes.iter().filter(|b| b.2 < -7.9 && b.1 < 5.5 && b.3 < -0.10).count(), 2, "left lower finish: {boxes:?}");
            assert_eq!(boxes.iter().filter(|b| b.2 < -7.9 && b.0 > 14.5 && b.3 < -0.10).count(), 2, "right lower finish: {boxes:?}");
            assert_eq!(boxes.iter().filter(|b| b.2 > 0.10 && b.0 < 5.2 && b.1 > 14.8).count(), 2, "upper bridge finishes: {boxes:?}");
            let cross_core = wall_contour_loops(scene, cross).into_iter().filter(|r| {
                let (x0,x1,y0,y1) = loop_bbox(r);
                x0 < 4.0 && (x1 - 4.85).abs() < 1e-3 && y0 >= -0.12 - 1e-4 && y1 <= 0.12 + 1e-4
            }).count();
            assert!(cross_core > 0, "crossing core reaches priority core face");
        };
        check(&scene);
        for h in [priority, cross] { regenerate_wall_representation(&mut scene, h, None).unwrap(); }
        check(&scene);
    }
}

#[test]
fn x_crossing_u_wall_keeps_every_leg_after_join_and_regen() {
    for swap in [false, true] {
        let mut scene = Scene::new();
        let through = add_layered_wall(&mut scene, (0.0, 0.0), (20.0, 0.0), plaster_masonry_plaster());
        let poly = add_polyline_layered_wall(
            &mut scene,
            &[(5.0, -8.0), (5.0, 8.0), (15.0, 8.0), (15.0, -8.0)],
            plaster_masonry_plaster(),
        );
        for h in [through, poly] {
            regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
        }
        let (a, b) = if swap { (poly, through) } else { (through, poly) };
        let (kind, _) = join_two_walls_in_document(&mut scene, a, b, None, None, None)
            .expect("join U wall with through wall");
        assert_eq!(kind, JoinKind::X);
        assert_u_wall_has_all_legs(&scene, poly, &format!("after join (swap={swap})"));

        for h in [through, poly] {
            regenerate_wall_representation(&mut scene, h, None).expect("plain regen");
        }
        assert_u_wall_has_all_legs(&scene, poly, &format!("after plain regen (swap={swap})"));
        regenerate_wall_representation(&mut scene, poly, None).expect("second regen is stable");
        assert_u_wall_has_all_legs(&scene, poly, &format!("after second regen (swap={swap})"));
    }
}

#[test]
fn x_crossing_u_wall_crossing_two_different_walls_keeps_every_leg() {
    // The same U wall, now crossing two *separate* through walls.
    let mut scene = Scene::new();
    // Two separate through walls whose ends stay clear of each other so
    // each pair is a genuine interior-interior crossing.
    let t1 = add_layered_wall(&mut scene, (0.0, 0.0), (8.0, 0.0), plaster_masonry_plaster());
    let t2 = add_layered_wall(&mut scene, (12.0, 0.0), (20.0, 0.0), plaster_masonry_plaster());
    let poly = add_polyline_layered_wall(
        &mut scene,
        &[(5.0, -8.0), (5.0, 8.0), (15.0, 8.0), (15.0, -8.0)],
        plaster_masonry_plaster(),
    );
    for h in [t1, t2, poly] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }
    join_two_walls_in_document(&mut scene, t1, poly, None, None, None).expect("join first crossing");
    join_two_walls_in_document(&mut scene, t2, poly, None, None, None).expect("join second crossing");
    for h in [t1, t2, poly] {
        regenerate_wall_representation(&mut scene, h, None).expect("regen");
    }
    assert_u_wall_has_all_legs(&scene, poly, "two different through walls");
}

pub(super) fn add_arc_layered_wall(
    scene: &mut Scene,
    a: (f64, f64),
    b: (f64, f64),
    bulge: f64,
    layers: Vec<WallLayer>,
) -> Handle {
    let mut pl = LwPolyline::new();
    let mut v0 = LwVertex::new(Vector2::new(a.0, a.1));
    v0.bulge = bulge;
    pl.add_vertex(v0);
    pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
    let mut entity = EntityType::LwPolyline(pl);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    scene.add_entity(entity)
}

pub(super) fn wall_axis_and_bulges(scene: &Scene, h: Handle) -> (Vec<DVec3>, Vec<f64>) {
    (get_wall_vertices(scene, h), get_wall_bulges(scene, h))
}

pub(super) fn wall_contours_with_bulges(scene: &Scene, h: Handle) -> Vec<Vec<(f64, f64, f64)>> {
    let wall = wall_from_entity(scene.document.get_entity(h).unwrap()).unwrap();
    wall.derived_handles.iter().filter_map(|child| {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*child) {
            Some(pl.vertices.iter().map(|v| (v.location.x, v.location.y, v.bulge)).collect())
        } else {
            None
        }
    }).collect()
}

#[test]
fn x_crossing_curved_priority_wall_is_not_cut_or_flattened_by_command_join() {
    // An unsupported X must not silently replace an arc with straight chord
    // quads, either via the command or via a later plain regeneration.
    for swap in [false, true] {
        let mut scene = Scene::new();
        let arc = add_arc_layered_wall(&mut scene, (-5.0, 0.0), (5.0, 0.0), 0.2, plaster_masonry_plaster());
        regenerate_wall_representation(&mut scene, arc, None).expect("standalone arc");
        let original = wall_contours_with_bulges(&scene, arc);
        assert!(original.iter().any(|ring| ring.iter().any(|v| v.2.abs() > 1e-4)));
        let straight =
            add_layered_wall(&mut scene, (0.0, -5.0), (0.0, 5.0), plaster_masonry_plaster());
        for h in [arc, straight] {
            regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
        }
        assert_eq!(wall_contours_with_bulges(&scene, arc), original, "regen must preserve curvature");
        let arc_before = wall_axis_and_bulges(&scene, arc);
        let straight_before = wall_axis_and_bulges(&scene, straight);
        let (a, b) = if swap { (straight, arc) } else { (arc, straight) };
        let straight_before_contours = wall_contours_with_bulges(&scene, straight);
        assert_eq!(join_two_walls_in_document(&mut scene, a, b, None, None, None).err(), Some(JoinError::Unsupported));
        assert_eq!(wall_axis_and_bulges(&scene, arc), arc_before, "arc axis untouched, swap={swap}");
        assert_eq!(
            wall_axis_and_bulges(&scene, straight),
            straight_before,
            "straight axis untouched, swap={swap}"
        );
        assert_eq!(wall_contours_with_bulges(&scene, arc), original);
        assert_eq!(wall_contours_with_bulges(&scene, straight), straight_before_contours);
        for h in [arc, straight] {
            regenerate_wall_representation(&mut scene, h, None).expect("regen");
        }
        assert_eq!(wall_contours_with_bulges(&scene, arc), original, "regen must retain arc after rejection");
        assert_eq!(wall_contours_with_bulges(&scene, straight), straight_before_contours);
    }
}

#[test]
fn x_crossing_partly_curved_polyline_is_rejected_without_flattening_other_leg() {
    let mut scene = Scene::new();
    let mut axis = LwPolyline::new();
    axis.add_vertex(LwVertex::new(Vector2::new(-5.0, 0.0)));
    let mut middle = LwVertex::new(Vector2::new(5.0, 0.0));
    middle.bulge = 0.2;
    axis.add_vertex(middle);
    axis.add_vertex(LwVertex::new(Vector2::new(10.0, 5.0)));
    let mut entity = EntityType::LwPolyline(axis);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("style1", 3.0, 0, &plaster_masonry_plaster(), &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let curved = scene.add_entity(entity);
    regenerate_wall_representation(&mut scene, curved, None).unwrap();
    let before = wall_contours_with_bulges(&scene, curved);
    assert!(before.iter().any(|ring| ring.iter().any(|v| v.2.abs() > 1e-4)));
    let cross = add_layered_wall(&mut scene, (0.0, -5.0), (0.0, 5.0), plaster_masonry_plaster());
    regenerate_wall_representation(&mut scene, cross, None).unwrap();
    let before_cross = wall_contours_with_bulges(&scene, cross);
    assert_eq!(join_two_walls_in_document(&mut scene, curved, cross, None, None, None).err(), Some(JoinError::Unsupported));
    assert_eq!(wall_contours_with_bulges(&scene, curved), before);
    assert_eq!(wall_contours_with_bulges(&scene, cross), before_cross);
    for h in [curved, cross] { regenerate_wall_representation(&mut scene, h, None).unwrap(); }
    assert_eq!(wall_contours_with_bulges(&scene, curved), before);
    assert_eq!(wall_contours_with_bulges(&scene, cross), before_cross);
}

#[test]
fn x_crossing_two_curved_walls_is_rejected_without_modifying_anything() {
    let mut scene = Scene::new();
    let a = add_arc_layered_wall(&mut scene, (-5.0, 0.0), (5.0, 0.0), 0.2, plaster_masonry_plaster());
    let b = add_arc_layered_wall(&mut scene, (0.0, -5.0), (0.0, 5.0), 0.1, plaster_masonry_plaster());
    for h in [a, b] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }
    let before_a = (wall_axis_and_bulges(&scene, a), wall_contour_loops(&scene, a));
    let before_b = (wall_axis_and_bulges(&scene, b), wall_contour_loops(&scene, b));
    let res = join_two_walls_in_document(&mut scene, a, b, None, None, None);
    assert_eq!(res.err(), Some(JoinError::Unsupported));
    assert_eq!(wall_axis_and_bulges(&scene, a), before_a.0);
    assert_eq!(wall_axis_and_bulges(&scene, b), before_b.0);
    assert_eq!(wall_contour_loops(&scene, a), before_a.1, "arc A geometry untouched");
    assert_eq!(wall_contour_loops(&scene, b), before_b.1, "arc B geometry untouched");
    // A later plain regen must not invent chord geometry either.
    for h in [a, b] {
        regenerate_wall_representation(&mut scene, h, None).expect("regen");
    }
    assert_eq!(wall_contour_loops(&scene, a).len(), before_a.1.len());
    assert_eq!(wall_contour_loops(&scene, b).len(), before_b.1.len());
}

#[test]
fn x_crossing_override_far_face_is_applied_by_join_and_regen() {
    let mut scene = Scene::new();
    let through = add_layered_wall(&mut scene, (0.0, 0.0), (20.0, 0.0), plaster_masonry_plaster());
    let cross = add_layered_wall(&mut scene, (10.0, -8.0), (10.0, 8.0), plaster_masonry_plaster());
    for h in [through, cross] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }
    let ov = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::FarFace),
        ..Default::default()
    };
    assert!(write_junction_override(&mut scene, cross, THROUGH_SPAN_OVERRIDE_END, &ov));
    let (kind, _) = join_two_walls_in_document(&mut scene, through, cross, None, None, None)
        .expect("X join with FarFace override");
    assert_eq!(kind, JoinKind::X);
    for pass in 0..2 {
        if pass == 1 {
            for h in [through, cross] {
                regenerate_wall_representation(&mut scene, h, None).expect("regen");
            }
        }
        // FarFace: the south core stem reaches the far (north) core face
        // y=+0.12 and the north core stem the south face y=-0.12 — the
        // automatic near-face butt (-0.12 / +0.12) was silently kept before.
        let loops = wall_contour_loops(&scene, cross);
        let reaches_north_face = loops.iter().any(|l| {
            let (_, _, min_y, max_y) = loop_bbox(l);
            (min_y + 8.0).abs() < 1e-3 && (max_y - 0.12).abs() < 1e-3
        });
        let reaches_south_face = loops.iter().any(|l| {
            let (_, _, min_y, max_y) = loop_bbox(l);
            (max_y - 8.0).abs() < 1e-3 && (min_y + 0.12).abs() < 1e-3
        });
        assert!(
            reaches_north_face && reaches_south_face,
            "pass {pass}: FarFace override must reach the far core faces, got {loops:?}"
        );
    }
}

#[test]
fn x_crossing_override_near_face_and_butt_keep_near_face_butt() {
    for style in [join::JoinOverrideStyle::NearFace, join::JoinOverrideStyle::Butt] {
        let mut scene = Scene::new();
        let through = add_layered_wall(&mut scene, (0.0, 0.0), (20.0, 0.0), plaster_masonry_plaster());
        let cross = add_layered_wall(&mut scene, (10.0, -8.0), (10.0, 8.0), plaster_masonry_plaster());
        for h in [through, cross] {
            regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
        }
        let ov = join::JunctionOverride {
            default_style: Some(style),
            ..Default::default()
        };
        assert!(write_junction_override(&mut scene, cross, THROUGH_SPAN_OVERRIDE_END, &ov));
        join_two_walls_in_document(&mut scene, through, cross, None, None, None).expect("X join");
        let loops = wall_contour_loops(&scene, cross);
        assert_eq!(loops.len(), 6, "{style:?}: 2 stems x 3 layers, got {loops:?}");
        for l in &loops {
            let (_, _, min_y, max_y) = loop_bbox(l);
            assert!(
                max_y <= -0.12 + 1e-3 || min_y >= 0.12 - 1e-3,
                "{style:?}: stem must stop at the near faces, got {min_y}..{max_y}"
            );
        }
    }
}

#[test]
fn through_wall_l_miter_crossed_twice_notches_both_faces() {
    let mut scene = Scene::new();
    // Through wall with finishes on both faces and an L at end 1, crossed
    // twice. Finish layers must notch at both x positions on both faces,
    // and the L-miter vertices must survive regen.
    let layers = plaster_masonry_plaster();
    let through = add_layered_wall(&mut scene, (0.0, 0.0), (20.0, 0.0), layers.clone());
    let corner = add_layered_wall(&mut scene, (20.0, 0.0), (20.0, 8.0), layers);
    let cross1 = add_wall_2layer(&mut scene, (5.0, -6.0), (5.0, 6.0), "Concrete");
    let cross2 = add_wall_2layer(&mut scene, (15.0, -6.0), (15.0, 6.0), "Concrete");

    for h in [through, corner, cross1, cross2] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }

    join_two_walls_in_document(&mut scene, through, corner, None, None, None).expect("L join");
    assert!(regenerate_wall_representation(&mut scene, through, None).is_ok());
    let l_pts_before = wall_contour_points(&scene, through);
    let l_miter_verts: Vec<(f64, f64)> = l_pts_before
        .iter()
        .copied()
        .filter(|p| p.0 > 20.0 + 1e-4)
        .collect();
    assert!(
        !l_miter_verts.is_empty(),
        "through-wall L-miter must extend beyond x=20, got {l_pts_before:?}"
    );

    join_junction_in_document(
        &mut scene,
        &[through, cross1],
        Some(DVec3::new(5.0, 0.0, 0.0)),
        None,
        None,
        None,
    )
    .expect("X join 1");
    join_junction_in_document(
        &mut scene,
        &[through, cross2],
        Some(DVec3::new(15.0, 0.0, 0.0)),
        None,
        None,
        None,
    )
    .expect("X join 2");

    for h in [through, corner, cross1, cross2] {
        assert!(regenerate_wall_representation(&mut scene, h, None).is_ok());
    }

    let pts_after = wall_contour_points(&scene, through);
    for p in &l_miter_verts {
        assert!(
            has_point(&pts_after, *p, 1e-3),
            "through-wall L-miter vertex {p:?} must survive two crossings, got {pts_after:?}"
        );
    }

    let loops = wall_contour_loops(&scene, through);
    let finish_notch_ok = |face_sign: f64| {
        let face_loops: Vec<_> = loops
            .iter()
            .filter(|lp| {
                !lp.is_empty() && lp.iter().all(|p| p.1 * face_sign > 0.10)
            })
            .collect();
        assert!(
            !face_loops.is_empty(),
            "expected finish loops on sign={face_sign} side, got {loops:?}"
        );
        let mut has_notch_5 = false;
        let mut has_notch_15 = false;
        for lp in &face_loops {
            let min_x = lp.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
            let max_x = lp.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
            assert!(
                !(min_x < 4.87 && max_x > 5.13),
                "finish ring must not span the crossing core at x=5, min_x={min_x} max_x={max_x} lp={lp:?}"
            );
            assert!(
                !(min_x < 14.87 && max_x > 15.13),
                "finish ring must not span the crossing core at x=15, min_x={min_x} max_x={max_x} lp={lp:?}"
            );
            has_notch_5 |= lp.iter().any(|p| (p.0 - 4.875).abs() < 0.05 || (p.0 - 5.125).abs() < 0.05);
            has_notch_15 |= lp.iter().any(|p| (p.0 - 14.875).abs() < 0.05 || (p.0 - 15.125).abs() < 0.05);
        }
        assert!(
            has_notch_5 && has_notch_15,
            "finish face sign={face_sign} must notch at both x=5 and x=15, loops={face_loops:?}"
        );
    };
    finish_notch_ok(1.0);
    finish_notch_ok(-1.0);
}
