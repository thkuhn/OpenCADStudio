use super::*;

#[test]
fn junction_override_write_read_roundtrip() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);

    let ov = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::Miter),
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: Some("Tragschale".to_string()),
                index: 0,
            layer_id: None,
            },
            layer_b: None,
            style: join::JoinOverrideStyle::OuterFace,
        }],
        layer_gaps: Vec::new(),
    };

    assert!(write_junction_override(&mut scene, wall, 1, &ov));
    let read_back = read_junction_override(&scene, wall, 1);
    assert_eq!(read_back, Some(ov));

    // The other end of the same axis was not touched.
    assert_eq!(read_junction_override(&scene, wall, 0), None);
}

#[test]
fn junction_override_missing_returns_none() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    // No override ever written on this axis.
    assert_eq!(read_junction_override(&scene, wall, 0), None);
    assert_eq!(read_junction_override(&scene, wall, 1), None);
}

#[test]
fn junction_override_absent_on_old_format_entity_is_backward_compatible() {
    // Simulate a drawing saved before this feature existed: the wall
    // axis has its normal `WALL` XDATA but never the new `JOIN_OVERRIDE`
    // tag. Reading must not panic and must simply report `None`.
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    let entity = scene.document.get_entity(wall).unwrap();
    assert!(wall_from_entity(entity).is_some(), "old-format wall still loads");
    assert_eq!(read_junction_override(&scene, wall, 0), None);
    assert_eq!(read_junction_override(&scene, wall, 1), None);
}

/// Mirrors the `Message::Aec(AecMessage::WallJunctionOverrideSetStyle)` handler in
/// `app/update/mod.rs`: read the existing override (if any), set
/// `default_style`, write it back, then trigger the same immediate
/// regeneration the context-menu action performs.
#[test]
fn wall_junction_context_menu_set_style_persists_and_regenerates() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    let end_index = 1usize;

    let mut override_data =
        read_junction_override(&scene, wall, end_index).unwrap_or_default();
    override_data.default_style = Some(join::JoinOverrideStyle::Butt);
    assert!(write_junction_override(&mut scene, wall, end_index, &override_data));
    let touched = refresh_wall_after_axis_edit(&mut scene, wall, None, None, None);
    assert!(!touched.is_empty(), "regeneration should touch at least the axis");

    let read_back = read_junction_override(&scene, wall, end_index);
    assert_eq!(
        read_back.and_then(|ov| ov.default_style),
        Some(join::JoinOverrideStyle::Butt)
    );
}

/// Mirrors the `Message::Aec(AecMessage::WallJunctionOverrideReset)` handler: remove the
/// override for the junction and regenerate. `read_junction_override`
/// must report `None` afterward.
#[test]
fn wall_junction_context_menu_reset_removes_override_and_regenerates() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    let end_index = 0usize;

    let ov = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::Miter),
        layer_pairs: vec![],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, wall, end_index, &ov));
    assert!(read_junction_override(&scene, wall, end_index).is_some());

    assert!(remove_junction_override(&mut scene, wall, end_index));
    let touched = refresh_wall_after_axis_edit(&mut scene, wall, None, None, None);
    assert!(!touched.is_empty());

    assert_eq!(read_junction_override(&scene, wall, end_index), None);
}

pub(super) fn add_wall_2layer(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64), mat: &str) -> Handle {
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
    pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
    let mut entity = EntityType::LwPolyline(pl);
    let layers = wls(&[(mat, 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    scene.add_entity(entity)
}

/// Same as [`add_wall_2layer`], but with a *bent*, multi-segment axis
/// (three vertices) so its "last vertex" raw index is `2`, not `1` — the
/// exact shape needed to reproduce the reported bug where the Junction
/// Editor's endpoint-index comparison broke for non-2-point wall axes.
pub(super) fn add_bent_wall_2layer(
    scene: &mut Scene,
    p1: (f64, f64),
    p2: (f64, f64),
    p3: (f64, f64),
    mat: &str,
) -> Handle {
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
    pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
    pl.add_vertex(LwVertex::new(Vector2::new(p3.0, p3.1)));
    let mut entity = EntityType::LwPolyline(pl);
    let layers = wls(&[(mat, 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    scene.add_entity(entity)
}

/// Regression test for the reported bug: the Junction Editor's master
/// list ("Beteiligte W\u{e4}nde") showed only the single clicked wall even
/// though the Properties panel correctly showed the connected walls. Root
/// cause: `walls_at_junction` compared the raw `JunctionRole::Endpoint`
/// vertex index (which is `axis.len() - 1` for the "far" end, e.g. `2`
/// for a 3-vertex bent wall) directly against the normalized `0`/`1`
/// `end_index` convention used everywhere else, so the match always
/// failed for walls with more than two axis points, and the function
/// fell back to returning just the queried wall.
#[test]
fn walls_at_junction_finds_all_participants_for_bent_multi_segment_wall() {
    let mut scene = Scene::new();
    // w1's axis has 3 vertices; its junction end is the *last* vertex,
    // whose raw index is 2 (not 1).
    let w1 = add_bent_wall_2layer(&mut scene, (-10.0, 5.0), (-10.0, 0.0), (0.0, 0.0), "Brick");
    let w2 = add_wall_2layer(&mut scene, (0.0, 0.0), (0.0, 10.0), "Concrete");
    let w3 = add_wall_2layer(&mut scene, (0.0, 0.0), (10.0, 0.0), "Wood");
    for h in [w1, w2, w3] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }
    join_junction_in_document(&mut scene,  &[w1, w2, w3],  None,  None,  None,  None).expect("N-way join");

    let w1_vertices = get_wall_vertices(&scene, w1);
    let end_1 = if w1_vertices[0].distance(DVec3::ZERO) < 1e-6 { 0 } else { 1 };
    let participants = walls_at_junction(&scene, w1, end_1);

    let mut handles: Vec<Handle> = participants.iter().map(|p| p.axis_handle).collect();
    handles.sort_by_key(|h| h.value());
    let mut expected = vec![w1, w2, w3];
    expected.sort_by_key(|h| h.value());
    assert_eq!(
        handles, expected,
        "expected all 3 walls at the junction, got {participants:?}"
    );
}

/// Regression test for the reported bug: a T-junction between two walls
/// (one wall's endpoint touches the other wall's *interior*, i.e. a
/// [`join::JunctionRole::Through`] participant) must still list the
/// through-running wall in the Junction Editor's participating-walls
/// list, not just the stem wall that was clicked to open the editor.
#[test]
fn walls_at_junction_includes_through_wall_at_t_junction() {
    let mut scene = Scene::new();
    let through = add_wall_2layer(&mut scene, (0.0, 0.0), (10.0, 0.0), "Brick");
    let stem = add_wall_2layer(&mut scene, (5.0, 0.0), (5.0, 5.0), "Concrete");
    for h in [through, stem] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }
    join_junction_in_document(&mut scene, &[through, stem], None, None, None, None).expect("T join");

    // The stem wall's end at (5,0) is a real Endpoint; use it as the
    // query, exactly as the context menu does when the user clicks the
    // stem wall to open the Junction Editor.
    let stem_vertices = get_wall_vertices(&scene, stem);
    let stem_end = if stem_vertices[0].distance(DVec3::new(5.0, 0.0, 0.0)) < 1e-6 { 0 } else { 1 };
    let participants = walls_at_junction(&scene, stem, stem_end);

    let mut handles: Vec<Handle> = participants.iter().map(|p| p.axis_handle).collect();
    handles.sort_by_key(|h| h.value());
    let mut expected = vec![through, stem];
    expected.sort_by_key(|h| h.value());
    assert_eq!(
        handles, expected,
        "the through wall must be listed alongside the stem wall, got {participants:?}"
    );

    let through_participant = participants
        .iter()
        .find(|p| p.axis_handle == through)
        .expect("through wall must be present");
    assert!(
        through_participant.is_through,
        "through wall participant must be flagged as is_through"
    );
    let stem_participant = participants
        .iter()
        .find(|p| p.axis_handle == stem)
        .expect("stem wall must be present");
    assert!(!stem_participant.is_through, "stem wall must not be flagged as through");
}

#[test]
fn disconnecting_one_end_drops_only_that_junction_override() {
    let mut scene = Scene::new();
    let mid = add_wall_2layer(&mut scene, (0.0, 0.0), (10.0, 0.0), "Brick");
    let left = add_wall_2layer(&mut scene, (0.0, 0.0), (0.0, 5.0), "Concrete");
    let right = add_wall_2layer(&mut scene, (10.0, 0.0), (10.0, 5.0), "Wood");
    for h in [mid, left, right] {
        regenerate_wall_representation(&mut scene, h, None).expect("regen");
    }
    join_two_walls_in_document(&mut scene, mid, left, None, None, None).expect("join left");
    join_two_walls_in_document(&mut scene, mid, right, None, None, None).expect("join right");
    let ov = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::Butt),
        layer_pairs: vec![],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, mid, 0, &ov));
    assert!(write_junction_override(&mut scene, mid, 1, &ov));
    update_wall_vertices(
        &mut scene,
        left,
        &[
            glam::DVec3::new(0.0, 20.0, 0.0),
            glam::DVec3::new(0.0, 25.0, 0.0),
        ],
    );
    let _ = try_auto_join_nearby_walls(&mut scene, left, None, None, None);
    assert_eq!(
        read_junction_override(&scene, mid, 0),
        None,
        "disconnected end must drop its override"
    );
    assert!(
        read_junction_override(&scene, mid, 1).is_some(),
        "the still-joined end must keep its override"
    );
}

/// Step 5 test 1: discovering "all walls at a junction" for a known
/// multi-wall N-way fixture returns every participating wall handle plus
/// its layer material ids, reusing [`join::detect_junctions`] topology.
#[test]
fn walls_at_junction_finds_all_n_way_participants() {
    let mut scene = Scene::new();
    let w1 = add_wall_2layer(&mut scene, (0.0, 0.0), (-10.0, 0.0), "Brick");
    let w2 = add_wall_2layer(&mut scene, (0.0, 0.0), (0.0, 10.0), "Concrete");
    let w3 = add_wall_2layer(&mut scene, (0.0, 0.0), (10.0, 0.0), "Wood");
    for h in [w1, w2, w3] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }
    join_junction_in_document(&mut scene,  &[w1, w2, w3],  None,  None,  None,  None).expect("N-way join");

    let end_1 = if get_wall_vertices(&scene, w1)[0].distance(DVec3::ZERO) < 1e-6 { 0 } else { 1 };
    let participants = walls_at_junction(&scene, w1, end_1);

    let mut handles: Vec<Handle> = participants.iter().map(|p| p.axis_handle).collect();
    handles.sort_by_key(|h| h.value());
    let mut expected = vec![w1, w2, w3];
    expected.sort_by_key(|h| h.value());
    assert_eq!(handles, expected);

    let mut mats: Vec<String> = participants
        .iter()
        .flat_map(|p| p.layers.iter().map(|l| l.material_id.clone()))
        .collect();
    mats.sort();
    let mut expected_mats = vec![
        "Brick".to_string(),
        "Insulation".to_string(),
        "Concrete".to_string(),
        "Insulation".to_string(),
        "Wood".to_string(),
        "Insulation".to_string(),
    ];
    expected_mats.sort();
    assert_eq!(mats, expected_mats);
}

#[test]
fn selected_junction_layer_ref_keeps_inner_same_material_layer() {
    let outer_id = uuid::Uuid::new_v4();
    let inner_id = uuid::Uuid::new_v4();
    let layers = vec![
        join::LayerRef {
            material_id: "Plaster".to_string(),
            role_tag: None,
            index: 0,
            layer_id: Some(outer_id),
        },
        join::LayerRef {
            material_id: "Brick".to_string(),
            role_tag: None,
            index: 1,
            layer_id: Some(uuid::Uuid::new_v4()),
        },
        join::LayerRef {
            material_id: "Plaster".to_string(),
            role_tag: None,
            index: 2,
            layer_id: Some(inner_id),
        },
    ];
    let chosen = selected_junction_layer_ref(&layers, 2, "Plaster").expect("inner plaster");
    assert_eq!(chosen.layer_id, Some(inner_id));
    assert_eq!(chosen.index, 2);
    assert_ne!(chosen.layer_id, Some(outer_id));
    let outer = selected_junction_layer_ref(&layers, 0, "Plaster").expect("outer plaster");
    assert_eq!(outer.layer_id, Some(outer_id));
}

#[test]
fn wall_junction_dropdown_grip_ids_roundtrip() {
    assert_eq!(wall_junction_end_from_dropdown_grip(wall_junction_dropdown_grip_id(0)), Some(0));
    assert_eq!(wall_junction_end_from_dropdown_grip(wall_junction_dropdown_grip_id(1)), Some(1));
    assert_eq!(wall_junction_end_from_dropdown_grip(0), None);
    assert_ne!(wall_junction_dropdown_grip_id(0), usize::MAX);
}

#[test]
fn wall_layer_contour_loop_xy_uses_chosen_stack_index() {
    let axis = [(0.0, 0.0), (10.0, 0.0)];
    let layers = [(0.1, -0.15), (0.2, -0.05)];
    let outer = wall_layer_contour_loop_xy(&axis, &layers, 0).expect("outer");
    let inner = wall_layer_contour_loop_xy(&axis, &layers, 1).expect("inner");
    assert!(outer.len() >= 5);
    assert!(inner.len() >= 5);
    let outer_span: f64 = outer.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max)
        - outer.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let inner_span: f64 = inner.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max)
        - inner.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    assert!((outer_span - 0.1).abs() < 1e-9);
    assert!((inner_span - 0.2).abs() < 1e-9);
    assert!(wall_layer_contour_loop_xy(&axis, &layers, 9).is_none());
}

#[test]
fn point_in_closed_loop_xy_picks_inner_layer_over_outer() {
    let axis = [(0.0, 0.0), (10.0, 0.0)];
    let layers = [(0.2, -0.1), (0.2, 0.1)];
    let outer = wall_layer_contour_loop_xy(&axis, &layers, 0).expect("outer");
    let inner = wall_layer_contour_loop_xy(&axis, &layers, 1).expect("inner");
    assert!(point_in_closed_loop_xy(5.0, 0.1, &inner));
    assert!(!point_in_closed_loop_xy(5.0, 0.1, &outer));
    assert!(point_in_closed_loop_xy(5.0, -0.1, &outer));
    assert!(!point_in_closed_loop_xy(5.0, 2.0, &outer));
}

/// Step 5 test 2: adding a `LayerPairOverride` via the panel's save logic
/// (read-mutate-write the same `JunctionOverride`) persists correctly and
/// coexists with an existing `default_style`.
#[test]
fn junction_editor_adds_layer_pair_and_keeps_default_style() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    let end_index = 1usize;

    let mut override_data =
        read_junction_override(&scene, wall, end_index).unwrap_or_default();
    override_data.default_style = Some(join::JoinOverrideStyle::Miter);
    assert!(write_junction_override(&mut scene, wall, end_index, &override_data));

    // Panel save logic: read-modify-write the same structure to add a
    // layer-pair override.
    let mut override_data =
        read_junction_override(&scene, wall, end_index).unwrap_or_default();
    override_data.layer_pairs.push(join::LayerPairOverride {
        layer_a: join::LayerRef {
            material_id: "Concrete".to_string(),
            role_tag: None,
            index: 0,
        layer_id: None,
        },
        layer_b: None,
        style: join::JoinOverrideStyle::Butt,
    });
    assert!(write_junction_override(&mut scene, wall, end_index, &override_data));

    let read_back = read_junction_override(&scene, wall, end_index).unwrap();
    assert_eq!(read_back.default_style, Some(join::JoinOverrideStyle::Miter));
    assert_eq!(read_back.layer_pairs.len(), 1);
    assert_eq!(read_back.layer_pairs[0].style, join::JoinOverrideStyle::Butt);
}

/// Step 5 test 3: removing a single layer-pair entry (panel's per-pair
/// "Zuruecksetzen") leaves other pairs and `default_style` intact.
#[test]
fn junction_editor_removes_single_layer_pair_only() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    let end_index = 1usize;

    let override_data = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::OuterFace),
        layer_pairs: vec![
            join::LayerPairOverride {
                layer_a: join::LayerRef {
                    material_id: "Brick".to_string(),
                    role_tag: None,
                    index: 0,
                layer_id: None,
                },
                layer_b: None,
                style: join::JoinOverrideStyle::Miter,
            },
            join::LayerPairOverride {
                layer_a: join::LayerRef {
                    material_id: "Insulation".to_string(),
                    role_tag: None,
                    index: 1,
                layer_id: None,
                },
                layer_b: None,
                style: join::JoinOverrideStyle::Butt,
            },
        ],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, wall, end_index, &override_data));

    // Panel per-pair reset: remove only the targeted entry.
    let mut current = read_junction_override(&scene, wall, end_index).unwrap();
    current.layer_pairs.retain(|p| p.layer_a.material_id != "Brick");
    assert!(write_junction_override(&mut scene, wall, end_index, &current));

    let read_back = read_junction_override(&scene, wall, end_index).unwrap();
    assert_eq!(read_back.default_style, Some(join::JoinOverrideStyle::OuterFace));
    assert_eq!(read_back.layer_pairs.len(), 1);
    assert_eq!(read_back.layer_pairs[0].layer_a.material_id, "Insulation");
}

/// Step 5 test 4: a full reset via the panel removes the entire override,
/// matching Step 4's context-menu reset exactly (same helper, same result).
#[test]
fn junction_editor_full_reset_matches_context_menu_reset() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    let end_index = 1usize;

    let override_data = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::Miter),
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Brick".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            },
            layer_b: None,
            style: join::JoinOverrideStyle::Butt,
        }],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, wall, end_index, &override_data));
    assert!(read_junction_override(&scene, wall, end_index).is_some());

    assert!(remove_junction_override(&mut scene, wall, end_index));
    assert_eq!(read_junction_override(&scene, wall, end_index), None);
}

/// Step 5 test 5 (consistency): setting `default_style` via the Step 4
/// context-menu code path, then editing `layer_pairs` via the panel's
/// code path on the SAME `(axis_handle, end_index)`, must combine both
/// additively in the final `JunctionOverride` (no clobbering).
#[test]
fn context_menu_and_junction_editor_paths_combine_additively() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    let end_index = 1usize;

    // Step 4 context-menu path.
    let mut override_data =
        read_junction_override(&scene, wall, end_index).unwrap_or_default();
    override_data.default_style = Some(join::JoinOverrideStyle::Butt);
    assert!(write_junction_override(&mut scene, wall, end_index, &override_data));

    // Step 5 panel path, on the exact same (axis_handle, end_index).
    let mut override_data =
        read_junction_override(&scene, wall, end_index).unwrap_or_default();
    override_data.layer_pairs.push(join::LayerPairOverride {
        layer_a: join::LayerRef {
            material_id: "Insulation".to_string(),
            role_tag: None,
            index: 1,
        layer_id: None,
        },
        layer_b: Some(join::LayerRef {
            material_id: "Concrete".to_string(),
            role_tag: None,
            index: 0,
        layer_id: None,
        }),
        style: join::JoinOverrideStyle::OuterFace,
    });
    assert!(write_junction_override(&mut scene, wall, end_index, &override_data));

    let read_back = read_junction_override(&scene, wall, end_index).unwrap();
    assert_eq!(read_back.default_style, Some(join::JoinOverrideStyle::Butt));
    assert_eq!(read_back.layer_pairs.len(), 1);
    assert_eq!(read_back.layer_pairs[0].style, join::JoinOverrideStyle::OuterFace);
}

#[test]
fn t_junction_multilayer_regeneration_preserves_boundaries() {
    let mut scene = Scene::new();
    // Through wall: (0,0) -> (10,0)
    let through = add_wall_2layer(&mut scene, (0.0, 0.0), (10.0, 0.0), "Brick");
    // Stem wall: (5,0) -> (5,5)
    let stem = add_wall_2layer(&mut scene, (5.0, 0.0), (5.0, 5.0), "Concrete");
    for h in [through, stem] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }
    join_junction_in_document(&mut scene, &[through, stem], None, None, None, None).expect("T join");

    // Regenerate both walls with precomputed miters or standard regen
    assert!(regenerate_wall_representation(&mut scene, through, None).is_ok());
    assert!(regenerate_wall_representation(&mut scene, stem, None).is_ok());

    let stem_geom = get_wall_vertices(&scene, stem);
    assert_eq!(stem_geom.len(), 2);

    let stem_wall = wall_from_entity(scene.document.get_entity(stem).unwrap()).unwrap();
    assert!(!stem_wall.derived_handles.is_empty(), "stem must have derived display entities");

    let mut contour_min_ys: Vec<f64> = Vec::new();
    for dh in &stem_wall.derived_handles {
        if let Some(entity) = scene.document.get_entity(*dh) {
            if let Some(rec) = read_aec_record(entity) {
                if let [XDataValue::String(kind), _, XDataValue::String(role)] = rec.values.as_slice() {
                    if kind == "WALL_REP" && role == WALL_REP_ROLE_CONTOUR {
                        if let EntityType::LwPolyline(pl) = entity {
                            let min_y = pl.vertices.iter().map(|v| v.location.y).fold(f64::INFINITY, f64::min);
                            contour_min_ys.push(min_y);
                        }
                    }
                }
            }
        }
    }

    assert_eq!(contour_min_ys.len(), 2, "stem wall must generate 2 layer contours");
    // Core (Concrete) butts at near face of through core (Brick) at y ≈ 0.075.
    // Insulation miters into matching approach-side through Insulation at y ≈ 0.075.
    // Neither layer penetrates into the through core (y < 0.075).
    for (i, min_y) in contour_min_ys.iter().enumerate() {
        assert!(
            (min_y - 0.075).abs() < 1e-4,
            "stem layer {i} must stop at through core near face (y=0.075), got {min_y}"
        );
    }
}

#[test]
fn x_crossing_multilayer_regeneration_preserves_boundaries() {
    let mut scene = Scene::new();
    // Priority Through wall: (0,0) -> (10,0)
    let through = add_wall_2layer(&mut scene, (0.0, 0.0), (10.0, 0.0), "Brick");
    // Crossing wall: (5,-5) -> (5,5)
    let cross = add_wall_2layer(&mut scene, (5.0, -5.0), (5.0, 5.0), "Concrete");
    for h in [through, cross] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }
    join_junction_in_document(&mut scene, &[through, cross], None, None, None, None).expect("X join");

    // Standard document regeneration of both walls
    assert!(regenerate_wall_representation(&mut scene, through, None).is_ok());
    assert!(regenerate_wall_representation(&mut scene, cross, None).is_ok());

    let cross_wall = wall_from_entity(scene.document.get_entity(cross).unwrap()).unwrap();
    assert!(!cross_wall.derived_handles.is_empty(), "crossing wall must have derived entities");

    let mut cross_contour_polys: Vec<LwPolyline> = Vec::new();
    for dh in &cross_wall.derived_handles {
        if let Some(entity) = scene.document.get_entity(*dh) {
            if let Some(rec) = read_aec_record(entity) {
                if let [XDataValue::String(kind), _, XDataValue::String(role)] = rec.values.as_slice() {
                    if kind == "WALL_REP" && role == WALL_REP_ROLE_CONTOUR {
                        if let EntityType::LwPolyline(pl) = entity {
                            cross_contour_polys.push(pl.clone());
                        }
                    }
                }
            }
        }
    }

    // Since the crossing wall is split into 2 rings for each of its 2 layers,
    // wall_regen should produce 4 contour polyline entities (2 per layer).
    assert_eq!(
        cross_contour_polys.len(),
        4,
        "crossing wall must generate 4 contour polylines (2 split rings x 2 layers)"
    );

    // Stem 1 rings (south) must stay below y = -0.075 (south core face of through wall)
    // Stem 2 rings (north) must stay above y = 0.075 (north core face of through wall)
    for poly in &cross_contour_polys {
        let max_y = poly.vertices.iter().map(|v| v.location.y).fold(f64::NEG_INFINITY, f64::max);
        let min_y = poly.vertices.iter().map(|v| v.location.y).fold(f64::INFINITY, f64::min);
        let is_south_stem = max_y <= -0.075 + 1e-4;
        let is_north_stem = min_y >= 0.075 - 1e-4;
        assert!(
            is_south_stem || is_north_stem,
            "contour polyline with min_y={min_y}, max_y={max_y} must not penetrate through core (-0.075..0.075)"
        );
    }

    // Through wall finish (insulation on y in [0.1, 0.15]) receives notch around x=5
    let through_wall = wall_from_entity(scene.document.get_entity(through).unwrap()).unwrap();
    let mut through_contour_polys: Vec<LwPolyline> = Vec::new();
    for dh in &through_wall.derived_handles {
        if let Some(entity) = scene.document.get_entity(*dh) {
            if let Some(rec) = read_aec_record(entity) {
                if let [XDataValue::String(kind), _, XDataValue::String(role)] = rec.values.as_slice() {
                    if kind == "WALL_REP" && role == WALL_REP_ROLE_CONTOUR {
                        if let EntityType::LwPolyline(pl) = entity {
                            through_contour_polys.push(pl.clone());
                        }
                    }
                }
            }
        }
    }
    // Insulation layer on through wall is notched into 2 rings (or has notch vertices near x=5)
    let has_notched_insulation = through_contour_polys.iter().any(|pl| {
        let is_insulation = pl.vertices.iter().all(|v| v.location.y >= 0.07);
        let has_notch = pl.vertices.iter().any(|v| (v.location.x - 4.875).abs() < 0.02 || (v.location.x - 5.125).abs() < 0.02);
        is_insulation && has_notch
    });
    assert!(
        has_notched_insulation,
        "through wall finish layer must have notch around x=5 from crossing stem, got polys: {through_contour_polys:?}"
    );
}

#[test]
fn x_crossing_preserves_endpoint_l_miter_on_regeneration() {
    let mut scene = Scene::new();
    // Priority Through wall: (0,0) -> (10,0)
    let through = add_wall_2layer(&mut scene, (0.0, 0.0), (10.0, 0.0), "Brick");
    // Crossing wall: (5,-5) -> (5,5)
    let cross = add_wall_2layer(&mut scene, (5.0, -5.0), (5.0, 5.0), "Concrete");
    // L-corner wall: (5,5) -> (10,5)
    let corner = add_wall_2layer(&mut scene, (5.0, 5.0), (10.0, 5.0), "Concrete");

    for h in [through, cross, corner] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }

    // 1. Join cross and corner as L-join at (5,5)
    join_two_walls_in_document(&mut scene, cross, corner, None, None, None).expect("L join");
    assert!(regenerate_wall_representation(&mut scene, cross, None).is_ok());
    let l_pts_before = wall_contour_points(&scene, cross);
    let l_miter_verts: Vec<(f64, f64)> = l_pts_before
        .iter()
        .copied()
        .filter(|p| p.1 > 5.0 + 1e-4)
        .collect();
    assert!(
        !l_miter_verts.is_empty(),
        "L-miter must extend beyond the axis end y=5 (not a square cap), got {l_pts_before:?}"
    );

    // 2. Join through and cross as X-junction at (5,0)
    join_junction_in_document(&mut scene, &[through, cross], Some(DVec3::new(5.0, 0.0, 0.0)), None, None, None).expect("X join");

    // Standard document regeneration of all three walls
    assert!(regenerate_wall_representation(&mut scene, through, None).is_ok());
    assert!(regenerate_wall_representation(&mut scene, cross, None).is_ok());
    assert!(regenerate_wall_representation(&mut scene, corner, None).is_ok());

    let cross_wall = wall_from_entity(scene.document.get_entity(cross).unwrap()).unwrap();
    let mut cross_contour_polys: Vec<LwPolyline> = Vec::new();
    for dh in &cross_wall.derived_handles {
        if let Some(entity) = scene.document.get_entity(*dh) {
            if let Some(rec) = read_aec_record(entity) {
                if let [XDataValue::String(kind), _, XDataValue::String(role)] = rec.values.as_slice() {
                    if kind == "WALL_REP" && role == WALL_REP_ROLE_CONTOUR {
                        if let EntityType::LwPolyline(pl) = entity {
                            cross_contour_polys.push(pl.clone());
                        }
                    }
                }
            }
        }
    }

    assert_eq!(
        cross_contour_polys.len(),
        4,
        "crossing wall with L at end must still generate 4 contour polylines (2 split rings x 2 layers)"
    );

    // South stem stays below the through core; north stem keeps the L-miter vertices.
    let north_stems: Vec<_> = cross_contour_polys
        .iter()
        .filter(|p| p.vertices.iter().map(|v| v.location.y).fold(f64::INFINITY, f64::min) >= 0.075 - 1e-4)
        .collect();
    assert_eq!(north_stems.len(), 2, "must have 2 north stem rings");

    let pts_after = wall_contour_points(&scene, cross);
    for p in &l_miter_verts {
        assert!(
            has_point(&pts_after, *p, 1e-3),
            "L-miter vertex {p:?} must survive X regeneration, got {pts_after:?}"
        );
    }
    for poly in &north_stems {
        let max_y = poly.vertices.iter().map(|v| v.location.y).fold(f64::NEG_INFINITY, f64::max);
        assert!(
            max_y > 5.0 + 1e-4,
            "north stem must keep the L-miter beyond y=5 (square cap is not enough), got max_y={max_y}"
        );
    }
}

#[test]
fn x_crossing_multiple_crossings_splits_into_three_rings() {
    let mut scene = Scene::new();
    // Priority Through wall 1: (0,-4) -> (10,-4)
    let through1 = add_wall_2layer(&mut scene, (0.0, -4.0), (10.0, -4.0), "Brick");
    // Priority Through wall 2: (0,4) -> (10,4)
    let through2 = add_wall_2layer(&mut scene, (0.0, 4.0), (10.0, 4.0), "Brick");
    // Crossing wall: (5,-10) -> (5,10)
    let cross = add_wall_2layer(&mut scene, (5.0, -10.0), (5.0, 10.0), "Concrete");

    for h in [through1, through2, cross] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }

    // Join at both crossings
    join_junction_in_document(&mut scene, &[through1, cross], Some(DVec3::new(5.0, -4.0, 0.0)), None, None, None).expect("X join 1");
    join_junction_in_document(&mut scene, &[through2, cross], Some(DVec3::new(5.0, 4.0, 0.0)), None, None, None).expect("X join 2");

    // Standard document regeneration
    assert!(regenerate_wall_representation(&mut scene, through1, None).is_ok());
    assert!(regenerate_wall_representation(&mut scene, through2, None).is_ok());
    assert!(regenerate_wall_representation(&mut scene, cross, None).is_ok());

    let cross_wall = wall_from_entity(scene.document.get_entity(cross).unwrap()).unwrap();
    let mut cross_contour_polys: Vec<LwPolyline> = Vec::new();
    for dh in &cross_wall.derived_handles {
        if let Some(entity) = scene.document.get_entity(*dh) {
            if let Some(rec) = read_aec_record(entity) {
                if let [XDataValue::String(kind), _, XDataValue::String(role)] = rec.values.as_slice() {
                    if kind == "WALL_REP" && role == WALL_REP_ROLE_CONTOUR {
                        if let EntityType::LwPolyline(pl) = entity {
                            cross_contour_polys.push(pl.clone());
                        }
                    }
                }
            }
        }
    }

    // 3 rings per layer x 2 layers = 6 contour polylines
    assert_eq!(
        cross_contour_polys.len(),
        6,
        "crossing wall with 2 X-crossings must generate 6 contour polylines (3 rings x 2 layers)"
    );

    // Ring 1 (south): max y <= -4.075
    // Ring 2 (middle): min y >= -3.925 and max y <= 3.925
    // Ring 3 (north): min y >= 4.075
    let mut south_count = 0;
    let mut middle_count = 0;
    let mut north_count = 0;

    for poly in &cross_contour_polys {
        let max_y = poly.vertices.iter().map(|v| v.location.y).fold(f64::NEG_INFINITY, f64::max);
        let min_y = poly.vertices.iter().map(|v| v.location.y).fold(f64::INFINITY, f64::min);
        if max_y <= -4.075 + 1e-4 {
            south_count += 1;
        } else if min_y >= -3.925 - 1e-4 && max_y <= 3.925 + 1e-4 {
            middle_count += 1;
        } else if min_y >= 4.075 - 1e-4 {
            north_count += 1;
        } else {
            panic!("poly spans across through wall core: min_y={min_y}, max_y={max_y}");
        }
    }

    assert_eq!(south_count, 2, "must have 2 south rings (1 per layer)");
    assert_eq!(middle_count, 2, "must have 2 middle rings (1 per layer)");
    assert_eq!(north_count, 2, "must have 2 north rings (1 per layer)");
}

#[test]
fn x_crossing_l_miter_plus_two_crossings_three_rings() {
    let mut scene = Scene::new();
    // Crossing wall along +Y with an L-corner at the north end, crossed by
    // two priority through walls. Regen must keep 3 rings (none inside either
    // through core) *and* the exact L-miter vertices.
    let through1 = add_wall_2layer(&mut scene, (0.0, -4.0), (10.0, -4.0), "Brick");
    let through2 = add_wall_2layer(&mut scene, (0.0, 4.0), (10.0, 4.0), "Brick");
    let cross = add_wall_2layer(&mut scene, (5.0, -10.0), (5.0, 10.0), "Concrete");
    let corner = add_wall_2layer(&mut scene, (5.0, 10.0), (12.0, 10.0), "Concrete");

    for h in [through1, through2, cross, corner] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }

    join_two_walls_in_document(&mut scene, cross, corner, None, None, None).expect("L join");
    assert!(regenerate_wall_representation(&mut scene, cross, None).is_ok());
    let l_pts_before = wall_contour_points(&scene, cross);
    let l_miter_verts: Vec<(f64, f64)> = l_pts_before
        .iter()
        .copied()
        .filter(|p| p.1 > 10.0 + 1e-4)
        .collect();
    assert!(
        !l_miter_verts.is_empty(),
        "L-miter must extend beyond y=10 before the X joins, got {l_pts_before:?}"
    );

    join_junction_in_document(
        &mut scene,
        &[through1, cross],
        Some(DVec3::new(5.0, -4.0, 0.0)),
        None,
        None,
        None,
    )
    .expect("X join 1");
    join_junction_in_document(
        &mut scene,
        &[through2, cross],
        Some(DVec3::new(5.0, 4.0, 0.0)),
        None,
        None,
        None,
    )
    .expect("X join 2");

    for h in [through1, through2, cross, corner] {
        assert!(regenerate_wall_representation(&mut scene, h, None).is_ok());
    }

    let cross_wall = wall_from_entity(scene.document.get_entity(cross).unwrap()).unwrap();
    let mut cross_contour_polys: Vec<LwPolyline> = Vec::new();
    for dh in &cross_wall.derived_handles {
        if let Some(entity) = scene.document.get_entity(*dh) {
            if let Some(rec) = read_aec_record(entity) {
                if let [XDataValue::String(kind), _, XDataValue::String(role)] = rec.values.as_slice() {
                    if kind == "WALL_REP" && role == WALL_REP_ROLE_CONTOUR {
                        if let EntityType::LwPolyline(pl) = entity {
                            cross_contour_polys.push(pl.clone());
                        }
                    }
                }
            }
        }
    }
    assert_eq!(
        cross_contour_polys.len(),
        6,
        "L + two X-crossings must yield 6 contour polylines (3 rings x 2 layers), got {}",
        cross_contour_polys.len()
    );

    let mut south_count = 0;
    let mut middle_count = 0;
    let mut north_count = 0;
    for poly in &cross_contour_polys {
        let max_y = poly.vertices.iter().map(|v| v.location.y).fold(f64::NEG_INFINITY, f64::max);
        let min_y = poly.vertices.iter().map(|v| v.location.y).fold(f64::INFINITY, f64::min);
        if max_y <= -4.075 + 1e-4 {
            south_count += 1;
        } else if min_y >= -3.925 - 1e-4 && max_y <= 3.925 + 1e-4 {
            middle_count += 1;
        } else if min_y >= 4.075 - 1e-4 {
            north_count += 1;
            assert!(
                max_y > 10.0 + 1e-4,
                "north ring must keep the L-miter beyond y=10, max_y={max_y}"
            );
        } else {
            panic!(
                "poly spans a through-wall core band: min_y={min_y}, max_y={max_y} verts={:?}",
                poly.vertices.iter().map(|v| (v.location.x, v.location.y)).collect::<Vec<_>>()
            );
        }
    }
    assert_eq!(south_count, 2, "must have 2 south rings");
    assert_eq!(middle_count, 2, "must have 2 middle rings clear of both cores");
    assert_eq!(north_count, 2, "must have 2 north rings");

    let pts_after = wall_contour_points(&scene, cross);
    for p in &l_miter_verts {
        assert!(
            has_point(&pts_after, *p, 1e-3),
            "L-miter vertex {p:?} must be unchanged after two X joins, got {pts_after:?}"
        );
    }
}

#[test]
fn x_crossing_join_two_walls_in_document_direct_command_path() {
    let mut scene = Scene::new();
    // Priority Through horizontal wall along X: (0, 0) to (20, 0) with core + finish layers
    let layers_h = plaster_masonry_plaster();
    let through = add_layered_wall(&mut scene, (0.0, 0.0), (20.0, 0.0), layers_h);
    // Crossing vertical wall along Y: (10, -8) to (10, 8)
    let layers_v = plaster_masonry_plaster();
    let cross = add_layered_wall(&mut scene, (10.0, -8.0), (10.0, 8.0), layers_v);

    for h in [through, cross] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }

    // Call join_two_walls_in_document (the direct AEC_WALLJOIN command path for 2 walls)
    let (kind, touched) = join_two_walls_in_document(&mut scene, through, cross, None, None, None)
        .expect("join_two_walls_in_document on crossing pair must succeed");
    assert_eq!(kind, JoinKind::X, "direct command join on crossing walls must return JoinKind::X");
    assert!(!touched.is_empty(), "must return touched entity handles");

    for h in [through, cross] {
        regenerate_wall_representation(&mut scene, h, None).expect("regen after join");
    }

    // Crossing wall: must have 2 split stem rings per layer (6 contour polylines total),
    // and neither ring penetrates the through core band y in [-0.10, 0.10].
    let cross_wall = wall_from_entity(scene.document.get_entity(cross).unwrap()).unwrap();
    let mut cross_polys: Vec<LwPolyline> = Vec::new();
    for dh in &cross_wall.derived_handles {
        if let Some(entity) = scene.document.get_entity(*dh) {
            if let Some(rec) = read_aec_record(entity) {
                if let [XDataValue::String(kind), _, XDataValue::String(role)] = rec.values.as_slice() {
                    if kind == "WALL_REP" && role == WALL_REP_ROLE_CONTOUR {
                        if let EntityType::LwPolyline(pl) = entity {
                            cross_polys.push(pl.clone());
                        }
                    }
                }
            }
        }
    }
    assert_eq!(
        cross_polys.len(),
        6,
        "crossing wall must produce 6 contour polylines (2 stems x 3 layers)"
    );

    let mut south_count = 0;
    let mut north_count = 0;
    for poly in &cross_polys {
        let max_y = poly.vertices.iter().map(|v| v.location.y).fold(f64::NEG_INFINITY, f64::max);
        let min_y = poly.vertices.iter().map(|v| v.location.y).fold(f64::INFINITY, f64::min);
        if max_y <= -0.10 + 1e-3 {
            south_count += 1;
        } else if min_y >= 0.10 - 1e-3 {
            north_count += 1;
        } else {
            panic!("crossing poly penetrates through core: min_y={min_y}, max_y={max_y}");
        }
    }
    assert_eq!(south_count, 3, "must have 3 south layer polygons");
    assert_eq!(north_count, 3, "must have 3 north layer polygons");

    // Through wall: finish layers notched on both north and south faces
    let through_pts = wall_contour_points(&scene, through);
    assert!(
        has_point(&through_pts, (9.88, -0.12), 1e-2)
            && has_point(&through_pts, (10.12, -0.12), 1e-2),
        "through south finish must have notch cutout points on both inner corners, got {through_pts:?}"
    );
    assert!(
        has_point(&through_pts, (9.88, 0.12), 1e-2)
            && has_point(&through_pts, (10.12, 0.12), 1e-2),
        "through north finish must have notch cutout points on both inner corners, got {through_pts:?}"
    );
}

#[test]
fn x_crossing_polyline_double_cross_via_direct_command_join() {
    let mut scene = Scene::new();
    // Straight through horizontal wall along X: (0, 0) to (20, 0)
    let layers_h = plaster_masonry_plaster();
    let through = add_layered_wall(&mut scene, (0.0, 0.0), (20.0, 0.0), layers_h);

    // Polyline U-shaped wall crossing the straight wall at x=5 and x=15:
    // (5, -8) -> (5, 8) -> (15, 8) -> (15, -8)
    let layers_poly = plaster_masonry_plaster();
    let poly = add_polyline_layered_wall(
        &mut scene,
        &[(5.0, -8.0), (5.0, 8.0), (15.0, 8.0), (15.0, -8.0)],
        layers_poly,
    );

    for h in [through, poly] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }

    // Call join_two_walls_in_document (the direct AEC_WALLJOIN command path for 2 walls)
    let (kind, touched) = join_two_walls_in_document(&mut scene, through, poly, None, None, None)
        .expect("join_two_walls_in_document on double crossing pair must succeed");
    assert_eq!(kind, JoinKind::X, "direct command join on crossing walls must return JoinKind::X");
    assert!(!touched.is_empty(), "must return touched entity handles");

    for h in [through, poly] {
        regenerate_wall_representation(&mut scene, h, None).expect("regen after join");
    }

    // Through wall: finish layers must have notch cutout points at both x=5 and x=15 on both faces
    let through_pts = wall_contour_points(&scene, through);
    assert!(
        has_point(&through_pts, (4.88, -0.12), 1e-2)
            && has_point(&through_pts, (5.12, -0.12), 1e-2),
        "through south finish must have notch cutout points at x=5, got {through_pts:?}"
    );
    assert!(
        has_point(&through_pts, (14.88, -0.12), 1e-2)
            && has_point(&through_pts, (15.12, -0.12), 1e-2),
        "through south finish must have notch cutout points at x=15, got {through_pts:?}"
    );
    assert!(
        has_point(&through_pts, (4.88, 0.12), 1e-2)
            && has_point(&through_pts, (5.12, 0.12), 1e-2),
        "through north finish must have notch cutout points at x=5, got {through_pts:?}"
    );
    assert!(
        has_point(&through_pts, (14.88, 0.12), 1e-2)
            && has_point(&through_pts, (15.12, 0.12), 1e-2),
        "through north finish must have notch cutout points at x=15, got {through_pts:?}"
    );

    // Polyline crossing wall: all contour polylines must be clear of through core band y in [-0.10, 0.10]
    let poly_wall = wall_from_entity(scene.document.get_entity(poly).unwrap()).unwrap();
    let mut poly_polys: Vec<LwPolyline> = Vec::new();
    for dh in &poly_wall.derived_handles {
        if let Some(entity) = scene.document.get_entity(*dh) {
            if let Some(rec) = read_aec_record(entity) {
                if let [XDataValue::String(kind), _, XDataValue::String(role)] = rec.values.as_slice() {
                    if kind == "WALL_REP" && role == WALL_REP_ROLE_CONTOUR {
                        if let EntityType::LwPolyline(pl) = entity {
                            poly_polys.push(pl.clone());
                        }
                    }
                }
            }
        }
    }
    assert!(
        !poly_polys.is_empty(),
        "crossing polyline wall must have contour polylines"
    );
    for pl in &poly_polys {
        let max_y = pl.vertices.iter().map(|v| v.location.y).fold(f64::NEG_INFINITY, f64::max);
        let min_y = pl.vertices.iter().map(|v| v.location.y).fold(f64::INFINITY, f64::min);
        assert!(
            max_y <= -0.10 + 1e-3 || min_y >= 0.10 - 1e-3,
            "polyline crossing ring must not penetrate through core: min_y={min_y}, max_y={max_y}"
        );
    }
}
