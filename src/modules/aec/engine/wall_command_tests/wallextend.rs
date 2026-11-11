use super::*;

#[test]
fn aec_wallextend_do_extends_to_an_explicit_point() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene); // axis: (0,0) -> (5,0)
    let mut command_line = CommandLine::default();

    aec_wallextend_do(
        &mut scene, 
        &mut command_line, 
        &format!("{}|PT|8|0|0", wall_handle.value()), 
    None,  None,  None);

    let axis = get_wall_vertices(&scene, wall_handle);
    assert_eq!(axis.len(), 2);
    // The nearer endpoint (5,0) should have moved to the target (8,0);
    // the far endpoint (0,0) stays put.
    assert!(axis.iter().any(|p| (p.x - 8.0).abs() < 1e-9 && p.y.abs() < 1e-9));
    assert!(axis.iter().any(|p| p.x.abs() < 1e-9 && p.y.abs() < 1e-9));

    // Both the 2D (contour/hatch) and 3D (solid) derived representation
    // must reflect the new, extended axis length — not just the axis
    // polyline itself.
    let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();
    let mut max_x_2d: f64 = 0.0;
    let mut max_x_3d: f64 = 0.0;
    for h in &wall.derived_handles {
        match scene.document.get_entity(*h).unwrap() {
            EntityType::LwPolyline(pl) => {
                for v in &pl.vertices {
                    max_x_2d = max_x_2d.max(v.location.x);
                }
            }
            EntityType::Solid3D(s3d) => {
                for wire in &s3d.wires {
                    for pt in &wire.points {
                        max_x_3d = max_x_3d.max(pt.x as f64);
                    }
                }
            }
            _ => {}
        }
    }
    assert!(
        max_x_2d > 7.9,
        "2D contour should extend to the new endpoint, got max_x={max_x_2d}"
    );
    assert!(
        max_x_3d > 7.9,
        "3D solid should extend to the new endpoint, got max_x={max_x_3d}"
    );
}

#[test]
fn aec_wallextend_do_preserves_direction_for_an_off_line_target() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene); // axis: (0,0) -> (5,0)
    let mut command_line = CommandLine::default();

    // Target point is NOT collinear with the wall's (0,0)->(5,0) axis
    // (it has a non-zero Y). Extending must not bend the wall towards
    // this point — it must project onto the original direction line.
    aec_wallextend_do(
        &mut scene, 
        &mut command_line, 
        &format!("{}|PT|8|2|0", wall_handle.value()), 
    None,  None,  None);

    let axis = get_wall_vertices(&scene, wall_handle);
    assert_eq!(axis.len(), 2);
    // Original direction was purely along +X (Y=0 for both points);
    // the new endpoint must keep the same direction, i.e. still Y=0.
    assert!(
        axis.iter().any(|p| (p.x - 8.0).abs() < 1e-9 && p.y.abs() < 1e-9),
        "extended endpoint should stay on the original direction line, got {:?}",
        axis
    );
    assert!(axis.iter().any(|p| p.x.abs() < 1e-9 && p.y.abs() < 1e-9));
}

#[test]
fn aec_wallextend_do_preserves_direction_for_a_multi_vertex_bent_wall() {
    // Regression test for a real-world root cause: for a multi-vertex
    // (bent) wall polyline, the direction to preserve when extending an
    // endpoint must come from the segment immediately adjacent to that
    // endpoint, NOT from a line drawn to the opposite far end of the
    // whole polyline (which, for a bent wall, points in a different
    // direction and would visibly change the extended segment's angle).
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    // A bent, 3-vertex wall axis: (0,0) -> (5,0) -> (5,5).
    // The last segment (5,0)->(5,5) runs purely along +Y.
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
    let mut entity = EntityType::LwPolyline(pl);
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let wall_handle = scene.add_entity(entity);

    let mut command_line = CommandLine::default();

    // Extend the (5,5) endpoint further along +Y, to (5,8).
    aec_wallextend_do(
        &mut scene, 
        &mut command_line, 
        &format!("{}|PT|5|8|0", wall_handle.value()), 
    None,  None,  None);

    let axis = get_wall_vertices(&scene, wall_handle);
    assert_eq!(axis.len(), 3);
    // The extended endpoint must stay on the last segment's direction
    // (X=5), not bend towards the far opposite end (0,0).
    let last = axis.last().unwrap();
    assert!(
        (last.x - 5.0).abs() < 1e-9 && (last.y - 8.0).abs() < 1e-9,
        "extended endpoint should stay on the adjacent segment's direction line, got {:?}",
        axis
    );
}

#[test]
fn aec_wallextend_do_extends_to_intersect_another_wall() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    // Wall A: (0,0) -> (5,0), stopping short of Wall B's axis.
    let wall_a = add_multi_layer_wall(&mut scene);
    // Wall B: a "through" wall running vertically at x=8, so extending A
    // towards it produces a T-junction trim on A only.
    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, -5.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, 5.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record(
        "style1",
        3.0,
        0,
        &vec![wl("Concrete", 0.2, "Structural")],
        &[],
        WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    let mut command_line = CommandLine::default();
    aec_wallextend_do(
        &mut scene, 
        &mut command_line, 
        &format!("{}|WALL|{}", wall_a.value(), wall_b.value()), 
    None,  None,  None);

    let axis_a = get_wall_vertices(&scene, wall_a);
    // Wall A should now end exactly at the intersection with wall B's axis.
    assert!(axis_a
        .iter()
        .any(|p| (p.x - 8.0).abs() < 1e-6 && p.y.abs() < 1e-6));
    // Wall B (the "through" wall) stays unchanged.
    let axis_b = get_wall_vertices(&scene, wall_b);
    assert_eq!(axis_b.len(), 2);
    assert!((axis_b[0].y - (-5.0)).abs() < 1e-9);
    assert!((axis_b[1].y - 5.0).abs() < 1e-9);
}

/// Regression test for the reported bug: `AEC_WALLEXTEND` to a target
/// wall visually trims/miters the extended wall, but never registered
/// the two walls as joined peers (`JOINED_PEERS` via
/// `engine::owner_index::link_peers`) — unlike `AEC_WALLJOIN` and the
/// automatic join performed while drawing. Without that peer link, the
/// connection isn't recognized as a real join by later operations (e.g.
/// re-resolving the junction after a subsequent move), so it appears as
/// if "no join was created".
#[test]
fn aec_wallextend_do_links_peers_with_target_wall() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    let wall_a = add_multi_layer_wall(&mut scene); // (0,0) -> (5,0)
    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, -5.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, 5.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record(
        "style1",
        3.0,
        0,
        &vec![wl("Concrete", 0.2, "Structural")],
        &[],
        WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    let mut command_line = CommandLine::default();
    aec_wallextend_do(
        &mut scene, 
        &mut command_line, 
        &format!("{}|WALL|{}", wall_a.value(), wall_b.value()), 
    None,  None,  None);

    let peers_a = engine::owner_index::peers_of(&scene.document, wall_a);
    let peers_b = engine::owner_index::peers_of(&scene.document, wall_b);
    assert_eq!(
        peers_a,
        vec![wall_b],
        "extended wall must be linked as a peer of its target wall"
    );
    assert_eq!(
        peers_b,
        vec![wall_a],
        "target wall must be linked as a peer of the extended wall"
    );
}

#[test]
fn aec_walljoin_do_forces_l_even_when_one_wall_overhangs() {
    use crate::ui::command_line::CommandLine;

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
    let through = add_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let stem = add_wall(&mut scene, (5.0, 1.0), (5.0, 4.0));
    let mut command_line = CommandLine::default();
    aec_walljoin_do(
        &mut scene, 
        &mut command_line, 
        &format!("{}|{}", through.value(), stem.value()), 
    None,  None,  None);
    let axis_t = get_wall_vertices(&scene, through);
    let axis_s = get_wall_vertices(&scene, stem);
    assert!(
        axis_t
            .iter()
            .any(|p| (p.x - 5.0).abs() < 1e-6 && p.y.abs() < 1e-6),
        "through wall must be trimmed to the L corner, got {axis_t:?}"
    );
    assert!(
        axis_s
            .iter()
            .any(|p| (p.x - 5.0).abs() < 1e-6 && p.y.abs() < 1e-6),
        "stem must reach the L corner, got {axis_s:?}"
    );
    assert!(
        !axis_t.iter().any(|p| (p.x - 10.0).abs() < 1e-6),
        "overhang past the corner must be removed"
    );
}

#[test]
fn regenerate_after_axis_shorten_matches_new_length() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene); // (0,0)-(5,0)
    let _ = regenerate_wall_representation(&mut scene, wall, None);
    update_wall_vertices(
        &mut scene,
        wall,
        &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(3.0, 0.0, 0.0)],
    );
    let _ = regenerate_wall_representation(&mut scene, wall, None);
    let rec = wall_from_entity(scene.document.get_entity(wall).unwrap()).unwrap();
    let mut max_x = 0.0_f64;
    for h in rec.derived_handles {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(h) {
            for v in &pl.vertices {
                max_x = max_x.max(v.location.x);
            }
        }
    }
    assert!(
        max_x < 3.2,
        "2D contour must follow the shortened axis, got max_x={max_x}"
    );
    assert!(max_x > 2.8, "2D contour should still reach the new end");
}

#[test]
fn regenerate_rewrites_existing_contour_polyline() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    let _ = regenerate_wall_representation(&mut scene, wall, None);
    let rec = wall_from_entity(scene.document.get_entity(wall).unwrap()).unwrap();
    let contour_h = rec
        .derived_handles
        .iter()
        .copied()
        .find(|&h| matches!(scene.document.get_entity(h), Some(EntityType::LwPolyline(_))))
        .expect("contour");

    update_wall_vertices(
        &mut scene,
        wall,
        &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(4.0, 0.0, 0.0)],
    );
    let _ = regenerate_wall_representation(&mut scene, wall, None);

    let rec = wall_from_entity(scene.document.get_entity(wall).unwrap()).unwrap();
    assert!(
        rec.derived_handles.contains(&contour_h),
        "contour handle must be reused so tessellation can retessellate it"
    );
    let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(contour_h) else {
        panic!("contour still a polyline");
    };
    let max_x = pl
        .vertices
        .iter()
        .map(|v| v.location.x)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (max_x - 4.0).abs() < 0.3,
        "reused contour must follow new axis, max_x={max_x}"
    );
}

#[test]
fn erase_wall_live_preview_companions_drops_untagged_draw_contour() {
    // Bug B / Step 3: WallCommand commits an untagged outer-contour
    // polyline for live preview. On finish it must be erased so only
    // WALL_REP children remain (which regenerate with the axis).
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);

    let mut preview = LwPolyline::new();
    preview.is_closed = true;
    preview.add_vertex(LwVertex::new(Vector2::new(0.0, -0.1)));
    preview.add_vertex(LwVertex::new(Vector2::new(10.0, -0.1)));
    preview.add_vertex(LwVertex::new(Vector2::new(10.0, 0.1)));
    preview.add_vertex(LwVertex::new(Vector2::new(0.0, 0.1)));
    let preview_h = scene.add_entity(EntityType::LwPolyline(preview));

    // A properly tagged WALL_REP child must NOT be erased by the helper.
    let mut tagged = LwPolyline::new();
    tagged.is_closed = true;
    tagged.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    tagged.add_vertex(LwVertex::new(Vector2::new(1.0, 0.0)));
    tagged.add_vertex(LwVertex::new(Vector2::new(1.0, 1.0)));
    let tagged_h = scene.add_entity(EntityType::LwPolyline(tagged));
    write_wall_display_tag(&mut scene, tagged_h, wall, WALL_REP_ROLE_CONTOUR);

    erase_wall_live_preview_companions(&mut scene, wall, &[preview_h, tagged_h]);

    assert!(
        scene.document.get_entity(preview_h).is_none(),
        "untagged live preview contour must be erased on wall finish"
    );
    assert!(
        scene.document.get_entity(tagged_h).is_some(),
        "WALL_REP children must not be erased by preview cleanup"
    );
    assert!(
        scene.document.get_entity(wall).is_some(),
        "wall axis must remain"
    );
}

#[test]
fn regenerate_erases_orphan_wall_rep_contour() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    let _ = regenerate_wall_representation(&mut scene, wall, None);

    let mut orphan = LwPolyline::new();
    orphan.is_closed = true;
    orphan.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    orphan.add_vertex(LwVertex::new(Vector2::new(9.0, 0.0)));
    orphan.add_vertex(LwVertex::new(Vector2::new(9.0, 1.0)));
    orphan.add_vertex(LwVertex::new(Vector2::new(0.0, 1.0)));
    let orphan_h = scene.add_entity(EntityType::LwPolyline(orphan));
    write_wall_display_tag(&mut scene, orphan_h, wall, WALL_REP_ROLE_CONTOUR);

    update_wall_vertices(
        &mut scene,
        wall,
        &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(2.0, 0.0, 0.0)],
    );
    let _ = regenerate_wall_representation(&mut scene, wall, None);

    // Orphan handle may be reused as the new contour; it must not keep
    // the old 9-unit rectangle either way.
    if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(orphan_h) {
        let max_x = pl
            .vertices
            .iter()
            .map(|v| v.location.x)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            max_x < 2.3,
            "reused leftover contour must follow shortened axis, max_x={max_x}"
        );
    }
    let rec = wall_from_entity(scene.document.get_entity(wall).unwrap()).unwrap();
    let mut max_x = 0.0_f64;
    for h in rec.derived_handles {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(h) {
            for v in &pl.vertices {
                max_x = max_x.max(v.location.x);
            }
        }
    }
    assert!(max_x < 2.3, "new contour must follow shortened axis, max_x={max_x}");
}

#[test]
fn change_wall_justification_shifts_axis_by_expected_distance() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene); // total thickness 0.25
    let axis_before = get_wall_vertices(&scene, wall_handle);
    assert!(axis_before.iter().all(|p| p.y.abs() < 1e-9));

    assert!(change_wall_justification(
        &mut scene,
        wall_handle,
        WallJustification::Interior,
    None));

    let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();
    assert_eq!(wall.justification, WallJustification::Interior);

    let axis_after = get_wall_vertices(&scene, wall_handle);
    // Center -> Interior delta is -0.5 * total_thickness = -0.125; the
    // offset direction for a straight horizontal axis is +Y, so the axis
    // should have shifted to Y = -0.125.
    for p in &axis_after {
        assert!((p.y - (-0.125)).abs() < 1e-6);
    }
}

#[test]
fn wall_extend_command_auto_detects_target_wall_on_entity_pick() {
    // Once a source wall is selected, clicking another wall (without
    // typing W) must dispatch the WALL|target branch.
    let mut cmd = WallExtendCommand::new();
    assert!(cmd.needs_entity_pick());

    let source = Handle::new(10);
    let target = Handle::new(20);
    assert!(matches!(
        cmd.on_entity_pick(source, DVec3::ZERO),
        CmdResult::NeedPoint
    ));
    assert!(cmd.needs_entity_pick());

    match cmd.on_entity_pick(target, DVec3::new(8.0, 0.0, 0.0)) {
        CmdResult::Dispatch(s) => {
            assert_eq!(s, format!("AEC_WALLEXTEND_DO {}|WALL|{}", source.value(), target.value()));
        }
        _ => panic!("expected WALL| dispatch"),
    }
}

#[test]
fn wall_extend_command_empty_click_stays_in_to_wall_mode() {
    // A null-handle pick (empty space) after the source wall is selected
    // must NOT fall back to a point extend in the default `ToWall`
    // mode — the user must pick a different wall or switch to `Point`
    // mode explicitly.
    let mut cmd = WallExtendCommand::new();
    let source = Handle::new(10);
    let _ = cmd.on_entity_pick(source, DVec3::ZERO);

    assert!(matches!(
        cmd.on_entity_pick(Handle::NULL, DVec3::new(8.0, 2.0, 0.0)),
        CmdResult::NeedPoint
    ));
}

#[test]
fn wall_extend_command_point_mode_dispatches_pt_after_switch() {
    // Typing `P` switches to `ToPoint` mode; a subsequent point pick
    // must dispatch the PT| point-projection path.
    let mut cmd = WallExtendCommand::new();
    let source = Handle::new(10);
    let _ = cmd.on_entity_pick(source, DVec3::ZERO);
    assert!(matches!(cmd.on_text_input("P"), Some(CmdResult::NeedPoint)));
    assert!(!cmd.needs_entity_pick());

    match cmd.on_point(DVec3::new(8.0, 2.0, 0.0)) {
        CmdResult::Dispatch(s) => {
            assert!(
                s.starts_with(&format!("AEC_WALLEXTEND_DO {}|PT|", source.value())),
                "expected PT| dispatch, got {s}"
            );
        }
        _ => panic!("expected PT| dispatch after switching to Point mode"),
    }
}

#[test]
fn regenerate_wall_representation_returns_axis_and_derived_handles() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);

    let touched = regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed");

    assert!(
        touched.contains(&wall_handle),
        "returned set must include the axis handle"
    );
    let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();
    assert!(
        !wall.derived_handles.is_empty(),
        "regeneration should create derived entities"
    );
    for h in &wall.derived_handles {
        assert!(
            touched.contains(h),
            "returned set must include derived handle {}",
            h.value()
        );
    }
    // Axis + every derived.
    assert_eq!(touched.len(), 1 + wall.derived_handles.len());
}

/// Regression for the Properties-panel/vertex-edit staleness bug: after
/// moving a wall's axis vertex and regenerating its representation, the
/// *resident* (GPU-facing) wire set — the one `invalidate_property_targets`
/// feeds via `bump_entities` — must reflect the new contour geometry, not
/// a leftover outline from before the edit. `refresh_wall_after_axis_edit`
/// forces this via `scene.bump_geometry()`; any other axis-edit caller must
/// reach the same end state.
#[test]
fn wall_axis_edit_updates_resident_contour_wires() {
    use crate::scene::view::camera::Camera;

    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None).expect("initial regen");

    // Prime the resident (camera-independent, GPU-facing) wire cache at
    // the original axis position.
    let cam = Camera::default();
    let _ = scene.model_tile_wires_arc(0, &cam, 1.0, 1.0);

    // Simulate a Properties-panel vertex edit: move the wall's endpoint
    // far away, regenerate, then invalidate exactly like
    // `invalidate_property_targets` does today — only `bump_entities` on
    // the touched handles, no `bump_geometry()`.
    let new_vertices = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(500.0, 0.0, 0.0)];
    update_wall_vertices(&mut scene, wall_handle, &new_vertices);
    let touched = regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regen after vertex edit");
    let changes: Vec<_> = touched
        .iter()
        .map(|&h| (h, crate::scene::ChangeKind::Modified))
        .collect();
    scene.bump_entities(&changes);

    // The resident wire set must no longer contain any wire endpoint at
    // the old axis extent (x == 5.0); every wall-derived wire must reach
    // out to the new extent (x == 500.0).
    let wires = scene.model_tile_wires_arc(0, &cam, 1.0, 1.0);
    let mut saw_new_extent = false;
    for wire in wires.iter() {
        let Some(handle) = Scene::handle_from_wire_name(&wire.name) else {
            continue;
        };
        if !touched.contains(&handle) {
            continue;
        }
        for pt in &wire.points {
            if pt[0].is_nan() {
                // Tombstone slot from the resident-wire splice; not real
                // geometry.
                continue;
            }
            assert!(
                (pt[0] - 5.0).abs() > 1e-6,
                "resident wire for handle {} still shows the pre-edit contour \
                 at x=5.0 (stale tessellation); point={:?}",
                handle.value(),
                pt
            );
            if (pt[0] - 500.0).abs() < 1e-6 {
                saw_new_extent = true;
            }
        }
    }
    assert!(
        saw_new_extent,
        "resident wire set never reached the new axis extent (x=500.0); \
         contour/hatch did not visibly follow the moved wall point"
    );
}

/// Regression for the STRETCH bug: the wall axis lives on the invisible
/// `AEC_WALL_AXIS` layer, so a crossing-window stretch only ever sees the
/// visible contour handle. Moving that contour's own vertices in place
/// (the naive/buggy approach) is immediately reverted by the next
/// `refresh_wall_after_axis_edit` regeneration, because it rebuilds the
/// contour from the *unchanged* axis. Only moving the axis itself, then
/// regenerating, actually relocates the visible wall — this is exactly
/// the fix applied to `CmdResult::StretchEntities` in
/// `command_driver.rs`.
#[test]
fn stretching_only_the_contour_is_reverted_by_regen_but_stretching_the_axis_sticks() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    let touched = regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("initial regen");

    let contour_handle = *touched
        .iter()
        .find(|&&h| {
            h != wall_handle
                && matches!(
                    scene.document.get_entity(h),
                    Some(EntityType::LwPolyline(pl)) if pl.is_closed
                )
        })
        .expect("wall must have produced a closed contour");

    // --- Buggy path: mutate only the visible contour's vertices in
    // place (what STRETCH's generic LwPolyline branch used to do for a
    // wall's contour handle before the fix), then regenerate.
    let mut contour_before = match scene.document.get_entity(contour_handle) {
        Some(EntityType::LwPolyline(pl)) => pl.clone(),
        _ => panic!("expected contour LwPolyline"),
    };
    for v in &mut contour_before.vertices {
        v.location.x += 495.0;
    }
    update_wall_vertices(&mut scene, contour_handle, &[]); // no-op guard; contour isn't the axis
    if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity_mut(contour_handle) {
        *pl = contour_before;
    }
    let reverted = refresh_wall_after_axis_edit(&mut scene,  wall_handle,  None,  None,  None);
    let still_short = get_wall_vertices(&scene, wall_handle)
        .iter()
        .all(|v| v.x < 400.0);
    assert!(
        still_short,
        "regenerating from the untouched axis must revert a contour-only stretch \
         (this is the bug being fixed): axis vertices = {:?}",
        get_wall_vertices(&scene, wall_handle)
    );
    let contour_after_revert = reverted
        .iter()
        .find(|&&h| {
            matches!(
                scene.document.get_entity(h),
                Some(EntityType::LwPolyline(pl)) if pl.is_closed
            )
        })
        .and_then(|&h| match scene.document.get_entity(h) {
            Some(EntityType::LwPolyline(pl)) => Some(pl.clone()),
            _ => None,
        })
        .expect("regenerated contour");
    assert!(
        contour_after_revert
            .vertices
            .iter()
            .all(|v| v.location.x < 400.0),
        "contour-only stretch must not survive regeneration: {:?}",
        contour_after_revert.vertices
    );

    // --- Correct (fixed) path: `stretch_wall_axis_in_window` — the
    // exact helper `CmdResult::StretchEntities` now calls for wall
    // packages — moves the axis itself, then regenerates.
    let touched_after_fix = stretch_wall_axis_in_window(
        &mut scene, 
        wall_handle, 
        |x,  _y| x < 400.0, 
        DVec3::new(495.0, 0.0, 0.0), 
        None, 
        None,
        None)
    .expect("axis vertex fell inside the window; must return Some(touched)");
    let axis_moved = get_wall_vertices(&scene, wall_handle)
        .iter()
        .any(|v| v.x > 400.0);
    assert!(
        axis_moved,
        "moving the axis vertices must stick after regeneration"
    );
    let contour_moved = touched_after_fix
        .iter()
        .filter_map(|&h| match scene.document.get_entity(h) {
            Some(EntityType::LwPolyline(pl)) if pl.is_closed => Some(pl.clone()),
            _ => None,
        })
        .any(|pl| pl.vertices.iter().any(|v| v.location.x > 400.0));
    assert!(
        contour_moved,
        "the regenerated contour must reach the new axis extent after the fix"
    );

    // A window that covers none of the axis vertices must be a no-op.
    assert!(
        stretch_wall_axis_in_window(
            &mut scene, 
            wall_handle, 
            |_x,  _y| false, 
            DVec3::new(1.0, 0.0, 0.0), 
            None, 
            None,
            None)
        .is_none(),
        "a window matching no axis vertex must not move or regenerate the wall"
    );
}

#[test]
fn join_l_corner_layer_footprints_share_miter_boundary() {
    let mut scene = Scene::new();
    // Single-layer walls so both sides fully match for miter.
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
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
    let mut ent_b = EntityType::LwPolyline(pl_b);
    let mut rec_b = ExtendedDataRecord::new(AEC_APPID);
    rec_b.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent_b.common_mut().extended_data.add_record(rec_b);
    let wall_b = scene.add_entity(ent_b);

    let (kind, touched) = join_two_walls_in_document(&mut scene,  wall_a,  wall_b,  None,  None,  None).expect("L join");
    assert_eq!(kind, JoinKind::L);
    assert!(touched.contains(&wall_a) && touched.contains(&wall_b));

    // Persisted axes still meet exactly at the corner.
    let axis_a = get_wall_vertices(&scene, wall_a);
    let axis_b = get_wall_vertices(&scene, wall_b);
    assert_eq!(*axis_a.last().unwrap(), DVec3::new(6.0, 0.0, 0.0));
    assert_eq!(*axis_b.first().unwrap(), DVec3::new(6.0, 0.0, 0.0));

    // Collect closed contour polylines (layer footprints) for both walls.
    let contours = |scene: &Scene, h: Handle| -> Vec<Vec<(f64, f64)>> {
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
    };
    let fps_a = contours(&scene, wall_a);
    let fps_b = contours(&scene, wall_b);
    assert!(!fps_a.is_empty() && !fps_b.is_empty());

    // Expected shared miter corners for equal 0.2 walls at (6,0):
    // (6.1, -0.1) and (5.9, 0.1).
    let c1 = (6.1, -0.1);
    let c2 = (5.9, 0.1);
    let has = |fp: &[(f64, f64)], p: (f64, f64)| {
        fp.iter()
            .any(|(x, y)| (*x - p.0).abs() < 1e-6 && (*y - p.1).abs() < 1e-6)
    };
    assert!(
        fps_a.iter().any(|fp| has(fp, c1) && has(fp, c2)),
        "wall A footprint should contain both miter corners, got {fps_a:?}"
    );
    assert!(
        fps_b.iter().any(|fp| has(fp, c1) && has(fp, c2)),
        "wall B footprint should share the same miter corners (no gap), got {fps_b:?}"
    );
}

#[test]
fn join_t_corner_stem_footprint_reaches_through_wall_face() {
    let mut scene = Scene::new();
    // Stem A approaching through wall B from above.
    let mut pl_a = LwPolyline::new();
    pl_a.add_vertex(LwVertex::new(Vector2::new(5.0, 1.0)));
    pl_a.add_vertex(LwVertex::new(Vector2::new(5.0, 10.0)));
    let mut ent_a = EntityType::LwPolyline(pl_a);
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut rec_a = ExtendedDataRecord::new(AEC_APPID);
    rec_a.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent_a.common_mut().extended_data.add_record(rec_a);
    let wall_a = scene.add_entity(ent_a);

    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut ent_b = EntityType::LwPolyline(pl_b);
    let mut rec_b = ExtendedDataRecord::new(AEC_APPID);
    rec_b.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent_b.common_mut().extended_data.add_record(rec_b);
    let wall_b = scene.add_entity(ent_b);

    let (kind, _touched) = join_two_walls_in_document(&mut scene,  wall_a,  wall_b,  None,  None,  None).expect("T join");
    assert_eq!(kind, JoinKind::T);

    let axis_a = get_wall_vertices(&scene, wall_a);
    assert!(
        axis_a
            .iter()
            .any(|p| (p.x - 5.0).abs() < 1e-6 && p.y.abs() < 1e-6),
        "stem axis should end on the through wall axis, got {axis_a:?}"
    );

    let wall_a_v2 = wall_from_entity(scene.document.get_entity(wall_a).unwrap()).unwrap();
    let mut max_abs_y_near_join = 0.0_f64;
    for h in &wall_a_v2.derived_handles {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*h) {
            for v in &pl.vertices {
                if (v.location.x - 5.0).abs() < 0.15 {
                    max_abs_y_near_join = max_abs_y_near_join.max(v.location.y.abs());
                }
            }
        }
    }
    assert!(
        max_abs_y_near_join > 0.05,
        "stem footprint should reach the through wall's layer face, got max |y|={max_abs_y_near_join}"
    );
}

#[test]
fn join_t_does_not_shorten_through_axis() {
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let add = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    };
    let through = add(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let stem = add(&mut scene, (4.0, 1.0), (4.0, 8.0));
    let (kind, _) = join_two_walls_in_document(&mut scene,  stem,  through,  None,  None,  None).expect("T");
    assert_eq!(kind, JoinKind::T);
    let through_axis = get_wall_vertices(&scene, through);
    assert_eq!(through_axis[0], DVec3::new(0.0, 0.0, 0.0));
    assert_eq!(through_axis[1], DVec3::new(10.0, 0.0, 0.0));
    let stem_axis = get_wall_vertices(&scene, stem);
    assert!(
        stem_axis[0].distance(DVec3::new(4.0, 0.0, 0.0)) < 1e-6,
        "stem should end on the through axis, got {stem_axis:?}"
    );
}

#[test]
fn join_junction_resolves_two_wall_l_and_t() {
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let add = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    };
    let a = add(&mut scene, (0.0, 0.0), (5.0, 0.0));
    let b = add(&mut scene, (5.0, 0.0), (5.0, 5.0));
    let touched = join_junction_in_document(&mut scene,  &[a, b],  None,  None,  None,  None).expect("L junction");
    assert!(touched.contains(&a) && touched.contains(&b));
    let axis_a = get_wall_vertices(&scene, a);
    let axis_b = get_wall_vertices(&scene, b);
    assert_eq!(*axis_a.last().unwrap(), DVec3::new(5.0, 0.0, 0.0));
    assert_eq!(*axis_b.first().unwrap(), DVec3::new(5.0, 0.0, 0.0));

    let through = add(&mut scene, (0.0, 10.0), (10.0, 10.0));
    let stem = add(&mut scene, (3.0, 10.0), (3.0, 15.0));
    join_junction_in_document(&mut scene,  &[through, stem],  None,  None,  None,  None).expect("T junction");
    let through_axis = get_wall_vertices(&scene, through);
    assert_eq!(through_axis[0], DVec3::new(0.0, 10.0, 0.0));
    assert_eq!(through_axis[1], DVec3::new(10.0, 10.0, 0.0));
}

#[test]
fn join_junction_honors_display_rules_on_both_walls() {
    use engine::display_component::{ComponentRuleSet, WallComponentSlot};
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let add = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    };
    let a = add(&mut scene, (0.0, 0.0), (5.0, 0.0));
    let b = add(&mut scene, (5.0, 0.0), (5.0, 5.0));
    let mut rules = ComponentRuleSet::default();
    rules
        .visibility
        .insert(WallComponentSlot::Solid3D.key().to_string(), false);
    join_junction_in_document(&mut scene, &[a, b], None, None, Some(&rules), None)
        .expect("L junction");
    for h in [a, b] {
        let derived = wall_from_entity(scene.document.get_entity(h).unwrap())
            .expect("WALL")
            .derived_handles;
        let solids = derived
            .iter()
            .filter(|handle| matches!(scene.document.get_entity(**handle), Some(EntityType::Solid3D(_))))
            .count();
        assert_eq!(
            solids, 0,
            "join regen must honor the active plan (no 3D solids), wall {h:?}"
        );
        assert!(
            !derived.is_empty(),
            "2D representation should still be created"
        );
    }
}

#[test]
fn refresh_after_axis_edit_honors_display_rules_on_joined_walls() {
    use engine::display_component::{ComponentRuleSet, WallComponentSlot};
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let add = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    };
    let a = add(&mut scene, (0.0, 0.0), (5.0, 0.0));
    let b = add(&mut scene, (5.0, 0.0), (5.0, 5.0));
    let mut rules = ComponentRuleSet::default();
    rules
        .visibility
        .insert(WallComponentSlot::Solid3D.key().to_string(), false);
    join_junction_in_document(&mut scene, &[a, b], None, None, Some(&rules), None)
        .expect("L junction");
    let mut axis = get_wall_vertices(&scene, a);
    axis[0] = DVec3::new(-0.2, 0.0, 0.0);
    update_wall_vertices(&mut scene, a, &axis);
    refresh_wall_after_axis_edit(&mut scene, a, None, Some(&rules), None);
    for h in [a, b] {
        let derived = wall_from_entity(scene.document.get_entity(h).unwrap())
            .expect("WALL")
            .derived_handles;
        let solids = derived
            .iter()
            .filter(|handle| matches!(scene.document.get_entity(**handle), Some(EntityType::Solid3D(_))))
            .count();
        assert_eq!(
            solids, 0,
            "axis-edit regen must honor the active plan (no 3D solids), wall {h:?}"
        );
    }
}

#[test]
fn try_auto_join_after_grip_keeps_t_through_axis() {
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let add = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    };
    let through = add(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let stem = add(&mut scene, (4.0, 0.0), (4.0, 6.0));
    join_two_walls_in_document(&mut scene,  stem,  through,  None,  None,  None).expect("T");

    let mut stem_axis = get_wall_vertices(&scene, stem);
    stem_axis[0] = DVec3::new(4.05, 0.1, 0.0);
    update_wall_vertices(&mut scene, stem, &stem_axis);
    let _ = try_auto_join_nearby_walls(&mut scene,  stem,  None,  None,  None);

    let through_axis = get_wall_vertices(&scene, through);
    assert_eq!(through_axis[0], DVec3::new(0.0, 0.0, 0.0));
    assert_eq!(through_axis[1], DVec3::new(10.0, 0.0, 0.0));
    let stem_after = get_wall_vertices(&scene, stem);
    assert!(
        stem_after[0].distance(DVec3::new(4.05, 0.0, 0.0)) < 1e-6
            || stem_after[0].distance(DVec3::new(4.0, 0.0, 0.0)) < 0.2,
        "stem should re-join the through axis, got {stem_after:?}"
    );
}

#[test]
fn aec_wallextend_to_target_wall_matches_join_wall_axes_intersection() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    // Wall A: (0,0)->(5,0); wall B vertical at x=8.
    let wall_a = add_multi_layer_wall(&mut scene);
    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, -5.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, 5.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record(
        "style1",
        3.0,
        0,
        &vec![wl("Concrete", 0.2, "Structural")],
        &[],
        WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    let axis_a_before = get_wall_vertices(&scene, wall_a);
    let axis_b = get_wall_vertices(&scene, wall_b);
    let (expected_a, _, _, _, _) =
        join::join_wall_axes(&axis_a_before, &axis_b).expect("axes should intersect");

    // Simulate the interactive path: pick source wall, then pick target
    // wall (auto-detect, no W keystroke).
    let mut cmd = WallExtendCommand::new();
    assert!(matches!(
        cmd.on_entity_pick(wall_a, DVec3::new(2.0, 0.0, 0.0)),
        CmdResult::NeedPoint
    ));
    let dispatch = match cmd.on_entity_pick(wall_b, DVec3::new(8.0, 0.0, 0.0)) {
        CmdResult::Dispatch(s) => s,
        _ => panic!("expected WALL| dispatch"),
    };
    let args = dispatch
        .strip_prefix("AEC_WALLEXTEND_DO ")
        .expect("dispatch prefix");
    let mut command_line = CommandLine::default();
    aec_wallextend_do(&mut scene,  &mut command_line,  args,  None,  None,  None);

    let axis_a_after = get_wall_vertices(&scene, wall_a);
    assert_eq!(
        axis_a_after.len(),
        expected_a.len(),
        "axis vertex count should match join_wall_axes"
    );
    for (got, exp) in axis_a_after.iter().zip(expected_a.iter()) {
        assert!(
            got.distance(*exp) < 1e-9,
            "extended endpoint must match join_wall_axes intersection, got {got:?} expected {exp:?}"
        );
    }
    // Must NOT be the raw click point projected onto the wall direction
    // in a way that ignores B — the intersection is at x=8.
    assert!(axis_a_after
        .iter()
        .any(|p| (p.x - 8.0).abs() < 1e-9 && p.y.abs() < 1e-9));
    // Full join rebuilds both walls' representations (miter/T stem). For
    // this T configuration the through-wall axis vertices stay put, but
    // the stem must still land on x=8 and both walls keep derived handles.
    let wall_a_v2 = wall_from_entity(scene.document.get_entity(wall_a).unwrap()).unwrap();
    let wall_b_v2 = wall_from_entity(scene.document.get_entity(wall_b).unwrap()).unwrap();
    assert!(
        !wall_a_v2.derived_handles.is_empty() && !wall_b_v2.derived_handles.is_empty(),
        "both walls should have regenerated derived handles after extend-join"
    );
}

#[test]
fn find_wall_to_auto_join_picks_nearby_joinable_wall() {
    let mut scene = Scene::new();
    // Existing wall along X from (0,0) to (5,0).
    let existing = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, existing, None).expect("regen existing");

    // New wall ending 0.15 m short of existing's end — within snap radius.
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(5.15, 3.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.15, 0.15)));
    let mut entity = EntityType::LwPolyline(pl);
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let new_wall = scene.add_entity(entity);

    let found = find_wall_to_auto_join(&scene, new_wall, &[]);
    assert_eq!(
        found,
        Some(existing),
        "should find the nearby existing wall within WALL_JOIN_SNAP_RADIUS"
    );

    // Far wall: no candidate.
    let mut pl_far = LwPolyline::new();
    pl_far.add_vertex(LwVertex::new(Vector2::new(50.0, 0.0)));
    pl_far.add_vertex(LwVertex::new(Vector2::new(55.0, 0.0)));
    let mut ent_far = EntityType::LwPolyline(pl_far);
    let mut rec_far = ExtendedDataRecord::new(AEC_APPID);
    rec_far.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent_far.common_mut().extended_data.add_record(rec_far);
    let far = scene.add_entity(ent_far);
    assert!(
        find_wall_to_auto_join(&scene, far, &[]).is_none(),
        "far wall must not auto-join"
    );

    // Excluding the only candidate yields None.
    assert!(find_wall_to_auto_join(&scene, new_wall, &[existing]).is_none());
}

#[test]
fn find_wall_to_auto_join_prefers_clear_endpoint_match_over_closer_t_match() {
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];

    // Candidate L: horizontal wall (0,0)->(5,0), same as
    // `add_multi_layer_wall`. Its endpoint at (5,0) is ~0.18 away from
    // the new wall's lower endpoint below — a clear End-End match.
    let corner = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, corner, None).expect("regen corner");

    // Candidate T: a long horizontal wall running underneath, whose
    // *interior* (not an endpoint) is only ~0.1 away from the new wall's
    // lower endpoint — nominally closer, but a vaguer End-Mid match.
    let mut pl_through = LwPolyline::new();
    pl_through.add_vertex(LwVertex::new(Vector2::new(-5.0, 0.2)));
    pl_through.add_vertex(LwVertex::new(Vector2::new(15.0, 0.2)));
    let mut ent_through = EntityType::LwPolyline(pl_through);
    let mut rec_through = ExtendedDataRecord::new(AEC_APPID);
    rec_through.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent_through.common_mut().extended_data.add_record(rec_through);
    let through = scene.add_entity(ent_through);
    regenerate_wall_representation(&mut scene, through, None).expect("regen through");

    // New wall: vertical, ending at (5.15, 0.1) — closer in raw distance
    // to `through`'s interior (~0.1) than to `corner`'s endpoint (~0.18).
    let mut pl_new = LwPolyline::new();
    pl_new.add_vertex(LwVertex::new(Vector2::new(5.15, 3.0)));
    pl_new.add_vertex(LwVertex::new(Vector2::new(5.15, 0.1)));
    let mut entity = EntityType::LwPolyline(pl_new);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let new_wall = scene.add_entity(entity);

    let found = find_wall_to_auto_join(&scene, new_wall, &[]);
    assert_eq!(
        found,
        Some(corner),
        "a clear endpoint match should win over a nominally closer T-interior match"
    );
}

#[test]
fn find_wall_to_auto_join_never_returns_the_wall_itself() {
    let mut scene = Scene::new();
    let existing = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, existing, None).expect("regen existing");
    // Even without excluding it explicitly, the candidate loop skips
    // `wall_handle == other` unconditionally, so a wall can never
    // auto-join to itself while still being drawn/edited.
    assert_eq!(find_wall_to_auto_join(&scene, existing, &[]), None);
}
