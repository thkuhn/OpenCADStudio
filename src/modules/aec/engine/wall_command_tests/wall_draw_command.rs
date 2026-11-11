use super::*;

#[test]
fn first_point_only_waits_for_the_next_one() {
    let mut cmd = WallCommand::new_with_library(None);
    assert!(matches!(
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0)),
        CmdResult::NeedPoint
    ));
}

#[test]
fn second_point_commits_a_two_vertex_wall_polyline_with_xdata() {
    let mut cmd = WallCommand::new_with_library(None);
    assert!(matches!(
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0)),
        CmdResult::NeedPoint
    ));

    match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => {
            match &entity {
                EntityType::LwPolyline(pl) => assert_eq!(pl.vertices.len(), 2),
                _ => panic!("expected a wall polyline"),
            }
            assert!(
                wall_xdata(&entity).is_some(),
                "committed wall segment should carry OPENCAD_AEC/WALL xdata"
            );
            assert_eq!(
                crate::entities::names::ui_name(&entity),
                "Wall",
                "properties/status should label the host as Wall, not Polyline"
            );
        }
        _ => panic!("second point should commit a wall polyline"),
    }
    assert_eq!(cmd.vertices.len(), 1);
    assert_eq!(cmd.segments_committed, 1);
}

#[test]
fn third_point_commits_a_second_two_vertex_wall() {
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let first = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("expected CommitEntity"),
    };
    match first {
        EntityType::LwPolyline(pl) => assert_eq!(pl.vertices.len(), 2),
        _ => panic!("expected LwPolyline"),
    }
    match cmd.on_point(DVec3::new(5.0, 3.0, 0.0)) {
        CmdResult::CommitEntity(entity) => match entity {
            EntityType::LwPolyline(pl) => assert_eq!(pl.vertices.len(), 2),
            _ => panic!("expected a wall polyline"),
        },
        _ => panic!("a third point should commit the next 2-vertex wall"),
    }
    assert_eq!(cmd.segments_committed, 2);
    assert_eq!(cmd.vertices.len(), 1);
}

#[test]
fn zero_length_second_point_is_rejected() {
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(1.0, 1.0, 0.0));
    assert!(matches!(
        cmd.on_point(DVec3::new(1.0, 1.0, 0.0)),
        CmdResult::NeedPoint
    ));
    assert_eq!(cmd.vertices.len(), 1);
    assert_eq!(cmd.segments_committed, 0);
}

#[test]
fn wall_chain_auto_joins_l_corner() {
    let mut scene = Scene::new();
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let e1 = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("expected first segment"),
    };
    let h1 = scene.add_entity(e1);
    cmd.on_entities_committed_in_scene(&mut scene, &[h1]);
    let e2 = match cmd.on_point(DVec3::new(5.0, 4.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("expected second segment"),
    };
    let h2 = scene.add_entity(e2);
    cmd.on_entities_committed_in_scene(&mut scene, &[h2]);

    let w1 = wall_from_entity(scene.document.get_entity(h1).unwrap()).unwrap();
    let w2 = wall_from_entity(scene.document.get_entity(h2).unwrap()).unwrap();
    assert!(!w1.derived_handles.is_empty());
    assert!(!w2.derived_handles.is_empty());
    let a1 = get_wall_vertices(&scene, h1);
    let a2 = get_wall_vertices(&scene, h2);
    let corner = DVec3::new(5.0, 0.0, 0.0);
    assert!(a1.iter().any(|p| p.distance(corner) < 1e-4));
    assert!(a2.iter().any(|p| p.distance(corner) < 1e-4));
}

#[test]
fn two_point_wall_regen_creates_contour_hatch_solid() {
    let mut scene = Scene::new();
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let entity = match cmd.on_point(DVec3::new(4.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("expected segment"),
    };
    let handle = scene.add_entity(entity);
    cmd.on_entities_committed_in_scene(&mut scene, &[handle]);
    let wall = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    let mut saw_contour = false;
    let mut saw_hatch = false;
    let mut saw_solid = false;
    for h in wall.derived_handles {
        let entity = scene.document.get_entity(h).unwrap();
        let record = read_aec_record(entity).expect("display child");
        if let [XDataValue::String(kind), _, XDataValue::String(role)] = record.values.as_slice()
        {
            assert_eq!(kind, "WALL_REP");
            match role.as_str() {
                WALL_REP_ROLE_CONTOUR => saw_contour = true,
                WALL_REP_ROLE_HATCH => saw_hatch = true,
                WALL_REP_ROLE_SOLID => saw_solid = true,
                _ => {}
            }
        }
    }
    assert!(saw_contour && saw_hatch && saw_solid);
}

#[test]
fn regen_places_2d_components_at_wall_base_z() {
    let mut scene = Scene::new();
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let entity = match cmd.on_point(DVec3::new(4.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("expected segment"),
    };
    let handle = scene.add_entity(entity);
    cmd.on_entities_committed_in_scene(&mut scene, &[handle]);
    let mut wall = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    wall.base_origin[2] = 3.0;
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    write_aec_record(&mut scene.document, handle, record);
    regenerate_wall_representation(&mut scene, handle, None).expect("regen");
    match scene.document.get_entity(handle).unwrap() {
        EntityType::LwPolyline(pl) => {
            assert!((pl.elevation - 3.0).abs() < 1e-9);
        }
        _ => panic!("expected axis polyline"),
    }
    let wall = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    let mut saw_contour_z = false;
    let mut saw_hatch_z = false;
    for h in wall.derived_handles {
        match scene.document.get_entity(h).unwrap() {
            EntityType::LwPolyline(pl) => {
                assert!((pl.elevation - 3.0).abs() < 1e-9);
                saw_contour_z = true;
            }
            EntityType::Hatch(hatch) => {
                assert!((hatch.elevation - 3.0).abs() < 1e-9);
                saw_hatch_z = true;
            }
            _ => {}
        }
    }
    assert!(saw_contour_z && saw_hatch_z);
}

#[test]
fn wall_commit_erases_live_preview_axis_and_contour() {
    let mut scene = Scene::new();
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let mut live_axis = LwPolyline::new();
    live_axis.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    live_axis.add_vertex(LwVertex::new(Vector2::new(3.0, 0.0)));
    let mut live_contour = LwPolyline::new();
    live_contour.is_closed = true;
    live_contour.add_vertex(LwVertex::new(Vector2::new(0.0, -0.1)));
    live_contour.add_vertex(LwVertex::new(Vector2::new(3.0, -0.1)));
    live_contour.add_vertex(LwVertex::new(Vector2::new(3.0, 0.1)));
    live_contour.add_vertex(LwVertex::new(Vector2::new(0.0, 0.1)));
    let live_handles = vec![
        scene.add_entity(EntityType::LwPolyline(live_axis)),
        scene.add_entity(EntityType::LwPolyline(live_contour)),
    ];
    cmd.set_live_handles(live_handles.clone());
    let committed = match cmd.on_point(DVec3::new(4.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("expected commit"),
    };
    let handle = scene.add_entity(committed);
    cmd.on_entities_committed_in_scene(&mut scene, &[handle]);
    for h in live_handles {
        if h != handle {
            assert!(
                scene.document.get_entity(h).is_none(),
                "live preview {h:?} should be erased"
            );
        }
    }
    let wall = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    assert!(!wall.derived_handles.is_empty());
}

#[test]
fn arc_keyword_toggles_arc_mode_and_produces_a_bulge_segment() {
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let first = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("expected first segment"),
    };
    match first {
        EntityType::LwPolyline(pl) => assert_eq!(pl.vertices[0].bulge, 0.0),
        _ => panic!("expected LwPolyline"),
    }

    assert!(matches!(cmd.on_text_input("A"), Some(CmdResult::NeedPoint)));
    assert!(cmd.arc_mode);
    let arc_entity = match cmd.on_point(DVec3::new(10.0, 5.0, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("expected arc segment"),
    };
    match arc_entity {
        EntityType::LwPolyline(pl) => {
            assert_eq!(pl.vertices.len(), 2);
            assert_ne!(pl.vertices[0].bulge, 0.0, "arc-mode segment should get a bulge");
        }
        _ => panic!("expected LwPolyline"),
    }

    assert!(matches!(cmd.on_text_input("L"), Some(CmdResult::NeedPoint)));
    assert!(!cmd.arc_mode);
    match cmd.on_point(DVec3::new(15.0, 5.0, 0.0)) {
        CmdResult::CommitEntity(entity) => match entity {
            EntityType::LwPolyline(pl) => assert_eq!(pl.vertices[0].bulge, 0.0),
            _ => panic!("expected LwPolyline"),
        },
        _ => panic!("expected straight segment"),
    }
}

#[test]
fn first_segment_in_arc_mode_commits_on_second_click_with_bulge() {
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    assert!(matches!(cmd.on_text_input("ARC"), Some(CmdResult::NeedPoint)));
    match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(EntityType::LwPolyline(pl)) => {
            assert_ne!(pl.vertices[0].bulge, 0.0);
        }
        _ => panic!("expected committed first-segment arc"),
    }
}

#[test]
fn commits_two_vertex_segments_without_picking_a_style() {
    let mut cmd = WallCommand::new();
    assert!(
        cmd.style_id.is_some(),
        "seed library should auto-select a wall style"
    );
    assert!(
        cmd.requires_style_selection(),
        "seed library should offer wall styles"
    );
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    match cmd.on_point(DVec3::new(4.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(EntityType::LwPolyline(pl)) => {
            assert_eq!(pl.vertices.len(), 2);
        }
        _ => panic!("expected a 2-vertex commit"),
    }
    match cmd.on_point(DVec3::new(4.0, 3.0, 0.0)) {
        CmdResult::CommitEntity(EntityType::LwPolyline(pl)) => {
            assert_eq!(pl.vertices.len(), 2);
        }
        _ => panic!("expected a second 2-vertex commit"),
    }
    assert_eq!(cmd.segments_committed, 2);
    assert_eq!(cmd.vertices.len(), 1);
}

#[test]
fn third_segment_keeps_first_corner_joined() {
    let mut scene = Scene::new();
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let h1 = scene.add_entity(match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("seg 1"),
    });
    cmd.on_entities_committed_in_scene(&mut scene, &[h1]);
    let h2 = scene.add_entity(match cmd.on_point(DVec3::new(5.0, 4.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("seg 2"),
    });
    cmd.on_entities_committed_in_scene(&mut scene, &[h2]);
    let h3 = scene.add_entity(match cmd.on_point(DVec3::new(0.0, 4.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("seg 3"),
    });
    cmd.on_entities_committed_in_scene(&mut scene, &[h3]);

    let peers2 = engine::owner_index::peers_of(&scene.document, h2);
    assert!(
        peers2.contains(&h1) && peers2.contains(&h3),
        "middle wall should stay joined to both neighbors, got {peers2:?}"
    );
    refresh_wall_after_axis_edit(&mut scene, h2, None, None, None);
    let peers2_after = engine::owner_index::peers_of(&scene.document, h2);
    assert!(
        peers2_after.contains(&h1) && peers2_after.contains(&h3),
        "regen/edit must not drop either join, got {peers2_after:?}"
    );
}

#[test]
fn undo_drops_the_last_vertex_and_removes_the_live_entity_below_two_points() {
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    assert!(matches!(cmd.on_text_input("U"), Some(CmdResult::NeedPoint)));
    assert!(cmd.vertices.is_empty());
}

#[test]
fn enter_before_two_points_keeps_the_command_running() {
    // Enter/Escape are global "finalize" keys that can fire while the
    // user is still only editing the live height/style panel fields
    // before the axis has a second point — there is nothing to finalize
    // yet, so the command must stay alive instead of cancelling.
    let mut enter_cmd = WallCommand::new_with_library(None);
    assert!(matches!(enter_cmd.on_enter(), CmdResult::NeedPoint));

    let mut escape_cmd = WallCommand::new_with_library(None);
    assert!(matches!(escape_cmd.on_escape(), CmdResult::NeedPoint));

    let mut one_point_cmd = WallCommand::new_with_library(None);
    one_point_cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    assert!(matches!(one_point_cmd.on_enter(), CmdResult::NeedPoint));
}

#[test]
fn enter_after_the_point_chain_finalizes_immediately_with_defaults() {
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("expected segment commit"),
    };
    let wall = wall_from_entity(&entity).expect("committed wall should carry WALL xdata");
    assert!((wall.total_thickness() - DEFAULT_WALL_THICKNESS).abs() < 1e-9);
    assert!((wall.height - DEFAULT_WALL_HEIGHT).abs() < 1e-9);
    assert!(matches!(cmd.on_enter(), CmdResult::Cancel));
}

#[test]
fn live_height_edit_before_enter_finalizes_with_the_edited_value() {
    use crate::command::LiveFieldValue;

    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    cmd.apply_live_property("wall_height", LiveFieldValue::Number(3.5));
    let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("expected segment commit"),
    };
    let wall = wall_from_entity(&entity).expect("committed wall should carry WALL xdata");
    assert!((wall.total_thickness() - DEFAULT_WALL_THICKNESS).abs() < 1e-9);
    assert!((wall.height - 3.5).abs() < 1e-9);
}

/// Draw four wall segments through `WallCommand` exactly as the
/// interactive host would (point chain, then height/thickness prompt),
/// then commit each finalized entity into a real `Scene`. Regression
/// check for `AEC_ROOM`'s closed-loop detection against interactively
/// drawn walls (previously only exercised against demo geometry).
#[test]
fn aec_room_detects_a_closed_loop_from_interactively_drawn_walls() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    let corners = [
        DVec3::new(0.0, 0.0, 0.0),
        DVec3::new(4.0, 0.0, 0.0),
        DVec3::new(4.0, 3.0, 0.0),
        DVec3::new(0.0, 3.0, 0.0),
        DVec3::new(0.0, 0.0, 0.0),
    ];

    for pair in corners.windows(2) {
        let mut cmd = WallCommand::new_with_library(None);
        let committed = match cmd.on_point(pair[0]) {
            CmdResult::NeedPoint => cmd.on_point(pair[1]),
            other => other,
        };
        let entity = match committed {
            CmdResult::CommitEntity(entity) => entity,
            _ => panic!("two points should commit a wall segment"),
        };
        let handle = scene.add_entity(entity);
        cmd.on_entities_committed_in_scene(&mut scene, &[handle]);
        assert!(matches!(cmd.on_enter(), CmdResult::Cancel));
    }

    let mut command_line = CommandLine::default();
    aec_room(&mut scene, &mut command_line);

    let room_record = scene
        .document
        .entities()
        .filter_map(read_aec_record)
        .find(|r| matches!(r.values.first(), Some(XDataValue::String(k)) if k == "ROOM"))
        .expect("aec_room should have written a ROOM xdata record");
    let area = match room_record.values.get(2) {
        Some(XDataValue::Real(a)) => *a,
        _ => panic!("ROOM record should carry an area value"),
    };
    assert!(
        (area - 12.0).abs() < 1e-6,
        "expected the detected 4x3 wall loop to yield area 12.0, got {area}"
    );
}

/// `wall_from_entity` is the inverse of `wall_record` — the properties
/// panel reads a `Wall` this way to populate wall fields for a WALL-tagged
/// entity.
#[test]
fn wall_from_entity_reads_back_a_finalized_wall_record() {
    use crate::command::LiveFieldValue;

    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    cmd.apply_live_property("wall_height", LiveFieldValue::Number(3.5));
    let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("expected second point to commit the wall"),
    };

    let wall = wall_from_entity(&entity).expect("finalized entity should read back as a Wall");
    assert!((wall.total_thickness() - DEFAULT_WALL_THICKNESS).abs() < 1e-9);
    assert!((wall.height - 3.5).abs() < 1e-9);
    assert_eq!(wall.layers.len(), 1);
}

/// A plain (non-WALL-tagged) entity must not be misread as a wall — this
/// is what keeps the properties-panel Wall section from appearing on
/// regular polylines.
#[test]
fn wall_from_entity_returns_none_for_a_plain_polyline() {
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(1.0, 0.0)));
    let entity = EntityType::LwPolyline(pl);
    assert!(wall_from_entity(&entity).is_none());
}

/// Height writeback via `write_wall_height` keeps the rest of the WALL
/// record intact and still visible to the room segment collector.
#[test]
fn write_wall_height_updates_the_wall_xdata_in_place() {
    let mut scene = Scene::new();
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("two points should commit a wall segment"),
    };
    let handle = scene.add_entity(entity);

    assert!(write_wall_height(&mut scene, handle, 3.2));

    let updated = wall_from_entity(scene.document.get_entity(handle).unwrap())
        .expect("entity should still read back as a wall after the edit");
    assert!((updated.height - 3.2).abs() < 1e-9);
    assert!((updated.total_thickness() - DEFAULT_WALL_THICKNESS).abs() < 1e-9);

    // The AEC_ROOM segment collector still sees this wall after the edit.
    let segments = collect_wall_segments(&scene.document);
    assert_eq!(segments.len(), 1);
}

/// `write_wall_hatch_override` (Step 6 Properties panel section) sets,
/// updates, and clears the per-wall-instance hatch-angle override while
/// leaving the rest of the WALL record intact.
#[test]
fn write_wall_hatch_override_sets_updates_and_clears_the_override() {
    let mut scene = Scene::new();
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("two points should commit a wall segment"),
    };
    let handle = scene.add_entity(entity);

    // No override set initially.
    let initial = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    assert_eq!(initial.hatch_override, None);

    // Setting an override.
    let override_a = engine::display_component::ComponentStyleOverride {
        hatch_angle: Some(45.0),
        hatch_angle_relative: Some(false),
        ..Default::default()
    };
    assert!(write_wall_hatch_override(&mut scene, handle, Some(override_a.clone())));
    let updated = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    assert_eq!(updated.hatch_override, Some(override_a));
    // Unrelated fields untouched.
    assert!((updated.height - DEFAULT_WALL_HEIGHT).abs() < 1e-9);

    // Updating the override (e.g. angle field edited again).
    let override_b = engine::display_component::ComponentStyleOverride {
        hatch_angle: Some(90.0),
        hatch_angle_relative: Some(true),
        ..Default::default()
    };
    assert!(write_wall_hatch_override(&mut scene, handle, Some(override_b.clone())));
    let updated2 = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    assert_eq!(updated2.hatch_override, Some(override_b));

    // Clearing the override (checkbox unchecked).
    assert!(write_wall_hatch_override(&mut scene, handle, None));
    let cleared = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    assert_eq!(cleared.hatch_override, None);
}
