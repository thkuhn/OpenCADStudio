use super::*;

#[test]
fn resolve_wall_package_falls_back_to_clicked_when_axis_is_missing() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    let handle = scene.add_entity(EntityType::LwPolyline(pl));

    // Tag it as derived from a handle that doesn't exist in the document.
    let stale_axis = Handle::new(999_999);
    write_wall_derived_tag(&mut scene, handle, stale_axis);

    assert_eq!(resolve_wall_package(&scene, handle), handle);
}

/// Bug 2: only the wall AXIS should ever be a snap candidate — never its
/// derived shell/hatch/solid contours.
#[test]
fn wall_axis_snap_wires_excludes_derived_and_includes_axis() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a valid two-layer wall");

    let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL");
    assert!(!wall.derived_handles.is_empty());

    // Sanity: the axis lives on the invisible axis layer; its derived
    // contour does not.
    assert_eq!(
        scene.document.get_entity(wall_handle).unwrap().common().layer,
        AEC_WALL_AXIS_LAYER
    );
    let derived_handle = wall.derived_handles[0];
    assert_ne!(
        scene
            .document
            .get_entity(derived_handle)
            .unwrap()
            .common()
            .layer,
        AEC_WALL_AXIS_LAYER
    );

    // Simulate the generic (render-shared) wire set: since the axis
    // layer is off, only the derived entity's wire is present there —
    // the axis itself is entirely absent.
    let derived_entity = scene.document.get_entity(derived_handle).unwrap().clone();
    let raw_wires = std::sync::Arc::new(scene.wires_for_entities(&[derived_entity]));
    assert!(raw_wires
        .iter()
        .any(|w| w.name == derived_handle.value().to_string()));

    let filtered = wall_axis_snap_wires(&scene, raw_wires);

    assert!(
        !filtered
            .iter()
            .any(|w| w.name == derived_handle.value().to_string()),
        "wall-derived contour/hatch/solid wires must be excluded from snap candidates"
    );
    assert!(
        filtered
            .iter()
            .any(|w| w.name == wall_handle.value().to_string()),
        "the wall axis must be included as a snap candidate even though its layer is off"
    );
}

/// Bug 4: after two walls are joined, each wall's contour must extend
/// past the shared corner into the other wall's footprint — verifying
/// the pragmatic corner-overlap fix in `join_two_walls_in_document` /
/// `regenerate_wall_representation_with_corner`.
#[test]
fn join_two_walls_extends_contours_into_shared_corner() {
    let mut scene = Scene::new();
    // Wall A: (0,0) -> (5,0), two layers, total thickness 0.25.
    let wall_a = add_multi_layer_wall(&mut scene);

    // Wall B: (6,1) -> (6,10), one layer, thickness 0.3. Meets wall A in
    // an L-junction at (6,0) — mirrors `join::tests::test_l_join`.
    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = vec![wl("Concrete", 0.3, "Structural")];
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    regenerate_wall_representation(&mut scene, wall_a, None)
        .expect("wall A regeneration should succeed");
    regenerate_wall_representation(&mut scene, wall_b, None)
        .expect("wall B regeneration should succeed");

    let (kind, _touched) = join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None)
        .expect("the two axes should join as an L-corner");
    assert_eq!(kind, JoinKind::L);

    // The trimmed axes must still meet exactly at the corner — the
    // corner-extension hint must not leak into the persisted axis.
    let axis_a = get_wall_vertices(&scene, wall_a);
    let axis_b = get_wall_vertices(&scene, wall_b);
    assert_eq!(*axis_a.last().unwrap(), DVec3::new(6.0, 0.0, 0.0));
    assert_eq!(*axis_b.first().unwrap(), DVec3::new(6.0, 0.0, 0.0));

    // Wall A's contour (thickness 0.25) should reach past x=6 by (at
    // least close to) wall B's half thickness (0.15), overlapping into
    // wall B's own footprint instead of stopping flush at the corner.
    let wall_a_v2 = wall_from_entity(scene.document.get_entity(wall_a).unwrap())
        .expect("wall A should still read back as WALL");
    let max_x = wall_a_v2
        .derived_handles
        .iter()
        .filter_map(|h| scene.document.get_entity(*h))
        .filter_map(|e| match e {
            EntityType::LwPolyline(pl) => Some(pl),
            _ => None,
        })
        .flat_map(|pl| pl.vertices.iter().map(|v| v.location.x))
        .fold(f64::MIN, f64::max);
    assert!(
        max_x > 6.0 + 1e-6,
        "wall A's contour should extend past the corner into wall B's footprint, got max_x={max_x}"
    );
}

/// Step 2: a `NoExtend` `layer_pairs` override on wall A's structural
/// layer must stop that specific layer from being extended into the L
/// corner, while the other (insulation) layer still auto-miters exactly
/// as in `join_two_walls_extends_contours_into_shared_corner`.
#[test]
fn join_junction_override_no_extend_keeps_one_layer_un_joined() {
    let mut scene = Scene::new();
    let wall_a = add_multi_layer_wall(&mut scene);

    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = vec![wl("Concrete", 0.3, "Structural")];
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    regenerate_wall_representation(&mut scene, wall_a, None)
        .expect("wall A regeneration should succeed");
    regenerate_wall_representation(&mut scene, wall_b, None)
        .expect("wall B regeneration should succeed");

    // Wall A's axis end that will join is its last vertex (end_index 1).
    let override_data = join::JunctionOverride {
        default_style: None,
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            },
            layer_b: None,
            style: join::JoinOverrideStyle::NoExtend,
        }],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, wall_a, 1, &override_data));
    assert_eq!(
        read_junction_override(&scene, wall_a, 1),
        Some(override_data)
    );

    let (kind, _touched) = join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None)
        .expect("the two axes should join as an L-corner");
    assert_eq!(kind, JoinKind::L);

    let wall_a_v2 = wall_from_entity(scene.document.get_entity(wall_a).unwrap())
        .expect("wall A should still read back as WALL");
    let contour_max_x: Vec<f64> = wall_a_v2
        .derived_handles
        .iter()
        .filter_map(|h| scene.document.get_entity(*h))
        .filter_map(|e| match e {
            EntityType::LwPolyline(pl) => Some(pl),
            _ => None,
        })
        .map(|pl| {
            pl.vertices
                .iter()
                .map(|v| v.location.x)
                .fold(f64::MIN, f64::max)
        })
        .collect();
    assert!(
        !contour_max_x.is_empty(),
        "wall A should still have contour polylines"
    );
    // With the Concrete layer forced NoExtend, no contour piece should
    // reach past wall B's half-thickness the way the fully-automatic
    // regression case does — at most one derived contour (Insulation)
    // may still extend into the corner.
    let extending = contour_max_x.iter().filter(|&&x| x > 6.0 + 1e-6).count();
    assert!(
        extending <= 1,
        "the NoExtend Concrete layer must not extend past the corner, got max_x values {contour_max_x:?}"
    );
}

/// Step 4a: a `layer_style_override` set on one of two same-material
/// layers (disambiguated by `layer_id`) must stay attached to the
/// originally intended layer even after a third layer is inserted
/// before it, shifting its `index`. Matching purely by `index` (or by
/// `material_id` alone, since both layers share it) would either lose
/// the override or misapply it to the wrong/both layers.
#[test]
fn layer_style_override_stays_attached_to_intended_layer_after_insert() {
    let target_id = uuid::Uuid::new_v4();
    let other_id = uuid::Uuid::new_v4();
    let rules = engine::display_component::ComponentRuleSet {
        layer_style_override: vec![engine::display_component::LayerStyleOverride {
            layer: join::LayerRef {
                material_id: "Plaster".to_string(),
                role_tag: None,
                index: 1,
                layer_id: Some(target_id),
            },
            style: engine::display_component::ComponentStyleOverride {
                line_type: Some("Dashed".to_string()),
                ..Default::default()
            },
        }],
        ..Default::default()
    };

    // Original stack: [Plaster(other_id) @0, Plaster(target_id) @1].
    let original_ref = join::LayerRef {
        material_id: "Plaster".to_string(),
        role_tag: None,
        index: 1,
        layer_id: Some(target_id),
    };
    let before = resolve_layer_style_override(Some(&rules), &[], &original_ref);
    assert_eq!(before.line_type, Some("Dashed".to_string()));

    // A third layer is inserted before it, shifting the target layer's
    // index to 2 — but its `layer_id` is unchanged.
    let shifted_ref = join::LayerRef {
        material_id: "Plaster".to_string(),
        role_tag: None,
        index: 2,
        layer_id: Some(target_id),
    };
    let after = resolve_layer_style_override(Some(&rules), &[], &shifted_ref);
    assert_eq!(
        after.line_type,
        Some("Dashed".to_string()),
        "override must stay attached to the layer by layer_id even though its index moved"
    );

    // The other same-material layer (a different layer_id) must not
    // pick up the override just because the material matches.
    let other_ref = join::LayerRef {
        material_id: "Plaster".to_string(),
        role_tag: None,
        index: 0,
        layer_id: Some(other_id),
    };
    let other_result = resolve_layer_style_override(Some(&rules), &[], &other_ref);
    assert_eq!(other_result.line_type, None);
}

/// Step 4b: a `JunctionOverride.layer_pairs` entry referencing a layer by
/// `layer_id` must stay correctly matched (and therefore survive
/// `validate_junction_override`'s pruning pass) after an uninvolved
/// layer is deleted, even though that shifts the referenced layer's
/// `index`.
#[test]
fn junction_override_layer_pair_survives_deletion_of_uninvolved_layer() {
    let concrete_id = uuid::Uuid::new_v4();

    // The override was captured while the wall still had an Insulation
    // layer at index 0 and Concrete at index 1.
    let override_data = join::JunctionOverride {
        default_style: None,
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 1,
                layer_id: Some(concrete_id),
            },
            layer_b: None,
            style: join::JoinOverrideStyle::NoExtend,
        }],
        layer_gaps: Vec::new(),
    };

    // The uninvolved Insulation layer was deleted, so Concrete is now
    // the only (index 0) layer, keeping the same `layer_id`.
    let self_layers = vec![join::LayerRef {
        material_id: "Concrete".to_string(),
        role_tag: None,
        index: 0,
        layer_id: Some(concrete_id),
    }];
    let other_layers: Vec<join::LayerRef> = vec![];

    let (validated, removed) = validate_junction_override(&override_data, &self_layers, &other_layers);
    assert_eq!(removed, 0, "the layer_id match must keep the pair despite the index shift");
    assert_eq!(
        validated,
        Some(join::JunctionOverride {
            default_style: None,
            layer_pairs: vec![join::LayerPairOverride {
                layer_a: join::LayerRef {
                    material_id: "Concrete".to_string(),
                    role_tag: None,
                    index: 1,
                    layer_id: Some(concrete_id),
                },
                layer_b: None,
                style: join::JoinOverrideStyle::NoExtend,
            }],
            layer_gaps: Vec::new(),
        })
    );
}

/// Step 3: when wall A's material changes such that a stored
/// `LayerPairOverride.layer_a` no longer matches any current layer, that
/// pair must be pruned on the next regeneration while a still-valid
/// `default_style` on the same override survives, and regeneration must
/// still succeed (falling back to automatic resolution for that layer).
#[test]
fn stale_layer_pair_override_is_pruned_but_default_style_kept() {
    let mut scene = Scene::new();
    let wall_a = add_multi_layer_wall(&mut scene);

    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = vec![wl("Concrete", 0.3, "Structural")];
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    regenerate_wall_representation(&mut scene, wall_a, None).expect("wall A regen");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("wall B regen");

    let override_data = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::Miter),
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            },
            layer_b: None,
            style: join::JoinOverrideStyle::NoExtend,
        }],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, wall_a, 1, &override_data));

    // Structural change: wall A's "Concrete" layer becomes "Brick" — the
    // stored override's `layer_a` no longer matches anything on wall A.
    write_wall_layers(
        &mut scene,
        wall_a,
        wls(&[("Brick", 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]),
    );

    let _ = take_pending_override_warnings(); // clear anything queued so far
    let (kind, _touched) = join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None)
        .expect("regeneration must succeed via automatic fallback");
    assert_eq!(kind, JoinKind::L);

    let cleaned = read_junction_override(&scene, wall_a, 1)
        .expect("default_style should survive the cleanup");
    assert_eq!(cleaned.default_style, Some(join::JoinOverrideStyle::Miter));
    assert!(
        cleaned.layer_pairs.is_empty(),
        "the stale Concrete layer pair should have been pruned, got {:?}",
        cleaned.layer_pairs
    );
}

/// Step 3: a layer removed entirely from a wall's style invalidates any
/// override referencing it — same cleanup, no crash, and the wall's
/// automatic-resolution footprint is still produced.
#[test]
fn override_referencing_removed_layer_is_cleaned_up_without_crash() {
    let mut scene = Scene::new();
    let wall_a = add_multi_layer_wall(&mut scene);

    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = vec![wl("Concrete", 0.3, "Structural")];
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    regenerate_wall_representation(&mut scene, wall_a, None).expect("wall A regen");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("wall B regen");

    let override_data = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::Butt),
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Insulation".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            },
            layer_b: None,
            style: join::JoinOverrideStyle::OuterFace,
        }],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, wall_a, 1, &override_data));

    // Remove the Insulation layer entirely from wall A's style.
    write_wall_layers(&mut scene, wall_a, vec![wl("Concrete", 0.2, "Structural")]);

    let (kind, _touched) = join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None)
        .expect("regeneration must not fail even though a referenced layer is gone");
    assert_eq!(kind, JoinKind::L);

    let cleaned = read_junction_override(&scene, wall_a, 1)
        .expect("default_style should survive the cleanup");
    assert!(cleaned.layer_pairs.is_empty());

    let wall_a_v2 = wall_from_entity(scene.document.get_entity(wall_a).unwrap())
        .expect("wall A should still read back as WALL");
    assert!(
        !wall_a_v2.derived_handles.is_empty(),
        "wall A should still have a fallback footprint after cleanup"
    );
}

/// Step 3: an override with only an (invalidated) `layer_pairs` entry and
/// no `default_style` must have its XDATA tag fully erased once cleanup
/// leaves nothing meaningful behind.
#[test]
fn fully_invalid_override_removes_xdata_tag_entirely() {
    let mut scene = Scene::new();
    let wall_a = add_multi_layer_wall(&mut scene);

    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = vec![wl("Concrete", 0.3, "Structural")];
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    regenerate_wall_representation(&mut scene, wall_a, None).expect("wall A regen");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("wall B regen");

    let override_data = join::JunctionOverride {
        default_style: None,
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            },
            layer_b: None,
            style: join::JoinOverrideStyle::NoExtend,
        }],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, wall_a, 1, &override_data));

    write_wall_layers(
        &mut scene,
        wall_a,
        wls(&[("Brick", 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]),
    );

    join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None)
        .expect("regeneration must succeed via automatic fallback");

    assert_eq!(
        read_junction_override(&scene, wall_a, 1),
        None,
        "the degenerate override should be erased entirely, not left as an empty record"
    );
}

/// Step 3: when a wall at an N-way junction is deleted, another wall's
/// override that referenced one of the deleted wall's layers as
/// `layer_b` must not cause a panic on the next regeneration of the
/// remaining walls — it is cleaned up gracefully instead.
#[test]
fn deleted_wall_at_junction_does_not_panic_remaining_override() {
    fn add_wall(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64)) -> Handle {
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
    let mut scene = Scene::new();
    // Three walls meeting at the origin (X-ish junction).
    let w1 = add_wall(&mut scene, (0.0, 0.0), (-10.0, 0.0));
    let w2 = add_wall(&mut scene, (0.0, 0.0), (0.0, 10.0));
    let w3 = add_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));

    for h in [w1, w2, w3] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }
    join_junction_in_document(&mut scene, &[w1, w2, w3], None, None, None, None).expect("initial N-way join");

    // W1 stores an override whose `layer_b` names W3's Concrete layer.
    let override_data = join::JunctionOverride {
        default_style: None,
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            },
            layer_b: Some(join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            }),
            style: join::JoinOverrideStyle::Butt,
        }],
        layer_gaps: Vec::new(),
    };
    let end_1 = if get_wall_vertices(&scene, w1)[0].distance(DVec3::ZERO) < 1e-6 {
        0
    } else {
        1
    };
    assert!(write_junction_override(&mut scene, w1, end_1, &override_data));

    // Delete W3 entirely from the document.
    scene.document.remove_entity(w3);

    // Re-resolving the junction with only the remaining walls must not
    // panic, even though W1's override still references the deleted
    // wall's layer.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        join_junction_in_document(&mut scene, &[w1, w2], None, None, None, None)
    }));
    assert!(result.is_ok(), "regeneration must not panic after a peer wall was deleted");
}

/// Step 3 regression: a still-valid override (referenced layer/material
/// unchanged) must not be touched by the invalidation pass.
#[test]
fn valid_override_is_not_touched_by_invalidation() {
    let mut scene = Scene::new();
    let wall_a = add_multi_layer_wall(&mut scene);

    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = vec![wl("Concrete", 0.3, "Structural")];
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    regenerate_wall_representation(&mut scene, wall_a, None).expect("wall A regen");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("wall B regen");

    let override_data = join::JunctionOverride {
        default_style: None,
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            },
            layer_b: None,
            style: join::JoinOverrideStyle::NoExtend,
        }],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, wall_a, 1, &override_data));

    let _ = take_pending_override_warnings();
    join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None).expect("join should succeed");

    assert_eq!(
        read_junction_override(&scene, wall_a, 1),
        Some(override_data),
        "a still-valid override must survive regeneration unchanged"
    );
    assert!(
        take_pending_override_warnings().is_empty(),
        "no invalidation notice should be queued for a valid override"
    );
}

/// Step 3: the user-visible notice mechanism (`take_pending_override_warnings`,
/// drained via `command_line.push_info` at command entry points) must
/// actually be invoked when an override is invalidated and removed.
#[test]
fn invalidated_override_queues_and_surfaces_a_notice() {
    let mut scene = Scene::new();
    let wall_a = add_multi_layer_wall(&mut scene);

    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = vec![wl("Concrete", 0.3, "Structural")];
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    regenerate_wall_representation(&mut scene, wall_a, None).expect("wall A regen");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("wall B regen");

    let override_data = join::JunctionOverride {
        default_style: None,
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            },
            layer_b: None,
            style: join::JoinOverrideStyle::NoExtend,
        }],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, wall_a, 1, &override_data));
    write_wall_layers(
        &mut scene,
        wall_a,
        wls(&[("Brick", 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]),
    );

    let _ = take_pending_override_warnings(); // drain any leftovers from prior tests
    let mut command_line = CommandLine::default();
    aec_walljoin_do(
        &mut scene, 
        &mut command_line, 
        &format!("{}|{}", wall_a.value(), wall_b.value()), 
    None,  None,  None);

    assert!(
        command_line
            .history
            .iter()
            .any(|e| e.text.contains("join override") || e.text.contains("Verbindungsüberschreibung")),
        "the command line should surface an invalidation notice, got {:?}",
        command_line.history.iter().map(|e| &e.text).collect::<Vec<_>>()
    );
}

#[test]
fn regenerate_wall_representation_builds_layers_and_avoids_duplication() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    let before = scene.document.entities().count();

    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a valid two-layer wall");
    let after_first = scene.document.entities().count();
    assert!(
        after_first > before,
        "regeneration should have created new contour/hatch/solid entities"
    );

    let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("wall should still be readable as WALL after regeneration");
    let derived_after_first = wall.derived_handles.clone();
    assert!(!derived_after_first.is_empty());

    // Calling it again must replace, not accumulate, the derived
    // entities.
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("second regeneration should also succeed");
    let after_second = scene.document.entities().count();
    assert_eq!(
        after_first, after_second,
        "regenerating twice should not duplicate derived entities"
    );

    let wall2 = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("wall should still be readable as WALL after second regeneration");
    assert_eq!(wall2.derived_handles.len(), derived_after_first.len());
}

#[test]
fn regenerate_wall_representation_respects_layer_override() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut entity = EntityType::LwPolyline(pl);
    let mut overridden = wl("Concrete", 0.2, "Structural");
    overridden.layer_override = Some("AEC_OVERRIDE_LAYER".to_string());
    let default_layer = wl("Insulation", 0.05, "Insulation");
    let layers = vec![overridden, default_layer];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let wall_handle = scene.add_entity(entity);

    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a valid two-layer wall");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles;
    assert!(!derived.is_empty());

    let mut saw_override_layer = false;
    let mut saw_default_layer = false;
    for h in derived {
        let e = scene.document.get_entity(h).unwrap();
        if e.common().layer == "AEC_OVERRIDE_LAYER" {
            saw_override_layer = true;
        } else if e.common().layer == "0" || !e.common().layer.is_empty() {
            saw_default_layer = true;
        }
    }
    assert!(
        saw_override_layer,
        "at least one derived entity of the overridden layer should be placed on AEC_OVERRIDE_LAYER"
    );
    assert!(
        saw_default_layer,
        "the non-overridden layer's derived entities should keep using the default layer"
    );
    assert!(scene.document.layers.contains("AEC_OVERRIDE_LAYER"));
}

#[test]
fn regenerate_wall_representation_with_rules_none_matches_default_behavior() {
    use crate::modules::aec::engine::display_component::ComponentRuleSet;

    let mut scene_plain = Scene::new();
    let wall_plain = add_multi_layer_wall(&mut scene_plain);
    regenerate_wall_representation(&mut scene_plain, wall_plain, None)
        .expect("plain regeneration should succeed");
    let derived_plain = wall_from_entity(scene_plain.document.get_entity(wall_plain).unwrap())
        .unwrap()
        .derived_handles;

    let mut scene_none = Scene::new();
    let wall_none = add_multi_layer_wall(&mut scene_none);
    regenerate_wall_representation_with_rules(&mut scene_none, wall_none, None, None)
        .expect("rules-aware regeneration with None should succeed");
    let derived_none = wall_from_entity(scene_none.document.get_entity(wall_none).unwrap())
        .unwrap()
        .derived_handles;
    assert_eq!(derived_plain.len(), derived_none.len());

    let mut scene_default = Scene::new();
    let wall_default = add_multi_layer_wall(&mut scene_default);
    let default_rules = ComponentRuleSet::default();
    regenerate_wall_representation_with_rules(
        &mut scene_default,
        wall_default,
        Some(&default_rules),
    None)
    .expect("rules-aware regeneration with default (all-visible) rules should succeed");
    let derived_default =
        wall_from_entity(scene_default.document.get_entity(wall_default).unwrap())
            .unwrap()
            .derived_handles;
    assert_eq!(
        derived_plain.len(),
        derived_default.len(),
        "a default ComponentRuleSet (everything visible) must reproduce today's behavior"
    );
}

#[test]
fn regenerate_wall_representation_with_rules_hides_solid3d_slot() {
    use crate::modules::aec::engine::display_component::{ComponentRuleSet, WallComponentSlot};

    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    let mut rules = ComponentRuleSet::default();
    rules
        .visibility
        .insert(WallComponentSlot::Solid3D.key().to_string(), false);

    regenerate_wall_representation_with_rules(&mut scene, wall_handle, Some(&rules), None)
        .expect("regeneration with Solid3D hidden should still succeed");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .unwrap()
        .derived_handles;
    assert!(!derived.is_empty());

    let mut saw_contour = false;
    let mut saw_hatch = false;
    let mut saw_solid = false;
    for h in &derived {
        match scene.document.get_entity(*h) {
            Some(EntityType::LwPolyline(_)) => saw_contour = true,
            Some(EntityType::Hatch(_)) => saw_hatch = true,
            Some(EntityType::Solid3D(_)) => saw_solid = true,
            _ => {}
        }
    }
    assert!(saw_contour, "contours should remain when only Solid3D is hidden");
    assert!(saw_hatch, "hatches should remain when only Solid3D is hidden");
    assert!(!saw_solid, "no Solid3D entity should be created when the slot is hidden");
}

#[test]
fn regenerate_wall_representation_with_rules_hides_layers2d_slot() {
    use crate::modules::aec::engine::display_component::{ComponentRuleSet, WallComponentSlot};

    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    let mut rules = ComponentRuleSet::default();
    rules
        .visibility
        .insert(WallComponentSlot::Layers2D.key().to_string(), false);
    rules
        .visibility
        .insert(WallComponentSlot::Contour2D.key().to_string(), false);

    regenerate_wall_representation_with_rules(&mut scene, wall_handle, Some(&rules), None)
        .expect("regeneration with 2D contour slots hidden should still succeed");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .unwrap()
        .derived_handles;
    assert!(!derived.is_empty());

    let mut saw_contour = false;
    let mut saw_solid = false;
    for h in &derived {
        match scene.document.get_entity(*h) {
            Some(EntityType::LwPolyline(_)) => saw_contour = true,
            Some(EntityType::Solid3D(_)) => saw_solid = true,
            _ => {}
        }
    }
    assert!(
        !saw_contour,
        "no contour polyline should be created when Layers2D and Contour2D are hidden"
    );
    assert!(saw_solid, "solids should remain when only 2D contour slots are hidden");
}

/// Serializes access to the on-disk AEC style library file (see
/// `engine::library::default_library_path`) for tests that need
/// `load_or_seed()` inside `regenerate_wall_representation_inner` to see
/// specific wall styles/materials (Step 3 `StyleSubstitution` tests):
/// writes `lib`, runs `f`, then restores whatever was on disk before.
pub(super) fn with_test_library<F: FnOnce()>(lib: &crate::modules::aec::engine::library::StyleLibrary, f: F) {
    static LIBRARY_TEST_LOCK: Mutex<()> = Mutex::new(());
    let _guard = LIBRARY_TEST_LOCK.lock().unwrap();
    let path = crate::modules::aec::engine::library::default_library_path();
    let backup = std::fs::read_to_string(&path).ok();
    crate::modules::aec::engine::library::save_to_default_path(lib)
        .expect("failed to write test library");
    f();
    match backup {
        Some(content) => {
            let _ = std::fs::write(&path, content);
        }
        None => {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[test]
fn component_rule_set_style_override_wins_for_hatch_slot() {
    use crate::modules::aec::engine::display_component::{
        ComponentRuleSet, ComponentStyleOverride, WallComponentSlot,
    };

    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    let mut rules = ComponentRuleSet::default();
    rules.style_override.insert(
        WallComponentSlot::ContourHatch2D.key().to_string(),
        ComponentStyleOverride {
            hatch_pattern: Some("NET".to_string()),
            hatch_color: Some(acadrust::types::Color::Rgb { r: 0, g: 255, b: 0 }),
            ..Default::default()
        },
    );

    regenerate_wall_representation_with_rules(&mut scene, wall_handle, Some(&rules), None)
        .expect("regeneration with a ContourHatch2D style_override should succeed");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .unwrap()
        .derived_handles;

    let mut saw_overridden_pattern = false;
    let mut saw_overridden_color = false;
    for h in &derived {
        if let Some(EntityType::Hatch(hatch)) = scene.document.get_entity(*h) {
            if hatch.pattern.name == "NET" {
                saw_overridden_pattern = true;
            }
            // The default fallback pattern must never appear once the
            // slot-wide override is active.
            assert_ne!(hatch.pattern.name, "ANSI31");
            if hatch.common.color
                == (acadrust::types::Color::Rgb {
                    r: 0,
                    g: 255,
                    b: 0,
                })
            {
                saw_overridden_color = true;
            }
        }
    }
    assert!(
        saw_overridden_pattern,
        "every layer's hatch should use the ContourHatch2D style_override pattern"
    );
    assert!(
        saw_overridden_color,
        "hatch entities must keep the override hatch_color on the DXF entity"
    );
}

#[test]
fn layer_selection_explicit_filters_contour_and_solid_to_referenced_layers() {
    use crate::modules::aec::engine::display_component::{
        ComponentRuleSet, LayerSelection, WallComponentSlot,
    };
    use crate::modules::aec::engine::join::LayerRef;

    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    // add_multi_layer_wall's layers are ["Concrete" (index 0), "Insulation" (index 1)].
    let mut rules = ComponentRuleSet::default();
    let explicit = LayerSelection::Explicit(vec![LayerRef {
        material_id: "Concrete".to_string(),
        role_tag: None,
        index: 0,
    layer_id: None,
    }]);
    rules
        .layer_filter
        .insert(WallComponentSlot::Contour2D.key().to_string(), explicit.clone());
    rules
        .layer_filter
        .insert(WallComponentSlot::Solid3D.key().to_string(), explicit);

    regenerate_wall_representation_with_rules(&mut scene, wall_handle, Some(&rules), None)
        .expect("regeneration with an explicit layer_filter should succeed");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .unwrap()
        .derived_handles;

    let mut contour_count = 0usize;
    let mut solid_count = 0usize;
    let mut hatch_count = 0usize;
    for h in &derived {
        match scene.document.get_entity(*h) {
            Some(EntityType::LwPolyline(_)) => contour_count += 1,
            Some(EntityType::Solid3D(_)) => solid_count += 1,
            Some(EntityType::Hatch(_)) => hatch_count += 1,
            _ => {}
        }
    }
    assert_eq!(
        contour_count, 1,
        "only the explicitly referenced layer's contour should be created"
    );
    assert_eq!(
        solid_count, 1,
        "only the explicitly referenced layer's solid should be created"
    );
    // `layer_filter` doesn't gate hatches (plan Step 3 scope is
    // Contour2D/Solid3D only) — both layers' hatches remain.
    assert_eq!(
        hatch_count, 2,
        "layer_filter must not affect hatch creation, only Contour2D/Solid3D"
    );
}

#[test]
fn style_substitution_swaps_hatch_look_but_keeps_axis_and_thickness() {
    use crate::modules::aec::engine::library::StyleLibrary;
    use crate::modules::aec::engine::material::Material;

    let source_material = Material::new(
        "SourceMat".to_string(),
        "Source".to_string(),
        "ANSI31".to_string(),
        0x111111,
        "Continuous".to_string(),
    );
    let mut target_material = Material::new(
        "TargetMat".to_string(),
        "Target".to_string(),
        "ANSI37".to_string(),
        0x222222,
        "Continuous".to_string(),
    );
    target_material.hatch_color = Some(acadrust::types::Color::Rgb { r: 171, g: 205, b: 239 });

    let source_style = WallStyle {
        style: Style {
            id: "src-style".to_string(),
            name: "Source Style".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![Layer {
            material_id: "SourceMat".to_string(),
            thickness: LayerValue::Fixed(0.2),
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
    };
    let target_style = WallStyle {
        style: Style {
            id: "tgt-style".to_string(),
            name: "Target Style".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        // Same total thickness as `source_style` (0.2), satisfying the
        // `validate_style_substitution` consistency requirement.
        layers: vec![Layer {
            material_id: "TargetMat".to_string(),
            thickness: LayerValue::Fixed(0.2),
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
    };
    assert!(crate::modules::aec::engine::display_component::validate_style_substitution(
        &source_style,
        &target_style
    )
    .is_ok());

    let lib = StyleLibrary {
        materials: vec![source_material, target_material],
        wall_styles: vec![source_style, target_style],
        ..Default::default()
    };

    with_test_library(&lib, || {
        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("SourceMat", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values =
            wall_record("src-style", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        let wall_handle = scene.add_entity(entity);

        let axis_before = get_wall_vertices(&scene, wall_handle);

        let mut substitutions: HashMap<String, String> = HashMap::new();
        substitutions.insert("src-style".to_string(), "tgt-style".to_string());

        regenerate_wall_representation_with_rules_and_substitutions(
            &mut scene,
            wall_handle,
            None,
            Some(&substitutions),
            Some(&lib))
        .expect("regeneration with a style substitution should succeed");

        let axis_after = get_wall_vertices(&scene, wall_handle);
        assert_eq!(axis_before, axis_after, "axis geometry must stay unchanged");
        let wall_after = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("still a WALL");
        assert!((wall_after.layers[0].thickness - 0.2).abs() < 1e-9);

        let mut saw_target_pattern = false;
        for h in &wall_after.derived_handles {
            if let Some(EntityType::Hatch(hatch)) = scene.document.get_entity(*h) {
                if hatch.pattern.name == "ANSI37" {
                    saw_target_pattern = true;
                }
                assert_ne!(
                    hatch.pattern.name, "ANSI31",
                    "the substituted wall must not use the source style's hatch pattern"
                );
            }
        }
        assert!(
            saw_target_pattern,
            "the substituted wall should use the target wall style's hatch pattern"
        );
    });
}

#[test]
fn detailed_style_override_wins_over_style_substitution() {
    use crate::modules::aec::engine::display_component::{
        ComponentRuleSet, ComponentStyleOverride, WallComponentSlot,
    };
    use crate::modules::aec::engine::library::StyleLibrary;
    use crate::modules::aec::engine::material::Material;

    let source_material = Material::new(
        "SourceMat2".to_string(),
        "Source".to_string(),
        "ANSI31".to_string(),
        0x111111,
        "Continuous".to_string(),
    );
    let target_material = Material::new(
        "TargetMat2".to_string(),
        "Target".to_string(),
        "ANSI37".to_string(),
        0x222222,
        "Continuous".to_string(),
    );
    let source_style = WallStyle {
        style: Style {
            id: "src-style-2".to_string(),
            name: "Source Style 2".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![Layer {
            material_id: "SourceMat2".to_string(),
            thickness: LayerValue::Fixed(0.2),
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
    };
    let target_style = WallStyle {
        style: Style {
            id: "tgt-style-2".to_string(),
            name: "Target Style 2".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![Layer {
            material_id: "TargetMat2".to_string(),
            thickness: LayerValue::Fixed(0.2),
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
    };

    let lib = StyleLibrary {
        materials: vec![source_material, target_material],
        wall_styles: vec![source_style, target_style],
        ..Default::default()
    };

    with_test_library(&lib, || {
        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("SourceMat2", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record(
            "src-style-2",
            3.0,
            0,
            &layers,
            &[],
            WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        let wall_handle = scene.add_entity(entity);

        let mut substitutions: HashMap<String, String> = HashMap::new();
        substitutions.insert("src-style-2".to_string(), "tgt-style-2".to_string());

        let mut rules = ComponentRuleSet::default();
        rules.style_override.insert(
            WallComponentSlot::ContourHatch2D.key().to_string(),
            ComponentStyleOverride {
                hatch_pattern: Some("NET".to_string()),
                ..Default::default()
            },
        );

        regenerate_wall_representation_with_rules_and_substitutions(
            &mut scene,
            wall_handle,
            Some(&rules),
            Some(&substitutions),
        None)
        .expect("regeneration with both a Detailed override and an applicable substitution should succeed");

        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .unwrap()
            .derived_handles;

        let mut saw_detailed_pattern = false;
        for h in &derived {
            if let Some(EntityType::Hatch(hatch)) = scene.document.get_entity(*h) {
                assert_eq!(
                    hatch.pattern.name, "NET",
                    "the Detailed style_override must win over the style substitution's target pattern"
                );
                saw_detailed_pattern = true;
            }
        }
        assert!(saw_detailed_pattern, "a hatch should have been created");
    });
}

#[test]
fn regenerate_wall_representation_with_rules_hides_slot_at_joined_corner() {
    use crate::modules::aec::engine::display_component::{ComponentRuleSet, WallComponentSlot};

    // Two joined walls sharing a corner via `regenerate_wall_representation_with_corner_and_rules`,
    // confirming the Solid3D override still applies at a mitered/extended corner.
    let mut scene = Scene::new();
    let wall_a = add_multi_layer_wall(&mut scene);
    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = wls(&[("Concrete", 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]);
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    let mut rules = ComponentRuleSet::default();
    rules
        .visibility
        .insert(WallComponentSlot::Solid3D.key().to_string(), false);

    // Regenerate wall_a with a corner-extension override toward wall_b's
    // start vertex, same shape as the plain join path uses, but with the
    // Solid3D slot hidden.
    regenerate_wall_representation_with_corner_and_rules(
        &mut scene,
        wall_a,
        Some((1, DVec3::new(5.0, 0.0, 0.0))),
        None,
        Some(&rules),
    None)
    .expect("joined-corner regeneration with Solid3D hidden should still succeed");

    let derived_a = wall_from_entity(scene.document.get_entity(wall_a).unwrap())
        .unwrap()
        .derived_handles;
    assert!(!derived_a.is_empty());
    let mut saw_contour = false;
    let mut saw_solid = false;
    for h in &derived_a {
        match scene.document.get_entity(*h) {
            Some(EntityType::LwPolyline(_)) => saw_contour = true,
            Some(EntityType::Solid3D(_)) => saw_solid = true,
            _ => {}
        }
    }
    assert!(saw_contour, "contours should still be produced at the joined corner");
    assert!(!saw_solid, "Solid3D should stay hidden at the joined corner too");

    let _ = wall_b; // kept alive to represent the join partner
}

#[test]
fn wall_axis_still_detected_by_room_loop_after_regeneration() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    let corners = [
        DVec3::new(0.0, 0.0, 0.0),
        DVec3::new(4.0, 0.0, 0.0),
        DVec3::new(4.0, 3.0, 0.0),
        DVec3::new(0.0, 3.0, 0.0),
        DVec3::new(0.0, 0.0, 0.0),
    ];

    let mut handles = Vec::new();
    for pair in corners.windows(2) {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(pair[0].x, pair[0].y)));
        pl.add_vertex(LwVertex::new(Vector2::new(pair[1].x, pair[1].y)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 2.8, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        handles.push(scene.add_entity(entity));
    }

    // Regenerate every wall: this moves each axis polyline onto the
    // dedicated AEC_WALL_AXIS layer.
    for h in &handles {
        regenerate_wall_representation(&mut scene, *h, None)
            .expect("regeneration should succeed for every wall segment");
    }
    for h in &handles {
        let e = scene.document.get_entity(*h).unwrap();
        assert_eq!(e.common().layer, AEC_WALL_AXIS_LAYER);
    }

    // AEC_ROOM / collect_wall_segments must still see the closed loop.
    let segments = collect_wall_segments(&scene.document);
    assert_eq!(segments.len(), 4);

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
    assert!((area - 12.0).abs() < 1e-6);
}

#[test]
fn wall_layer_extrusions_calculates_offsets_and_effective_height() {
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let entity = EntityType::LwPolyline(pl);
    let layers = vec![WallLayer {
        material: "brick".to_string(),
        thickness: 0.2,
        function: "Structural".to_string(),
        axis_offset: -0.1,
        bottom_offset: 0.5,
        top_offset: 0.3,
        layer_override: None,
        hatch_override: None,
    layer_id: uuid::Uuid::new_v4(),
    }];
    let height = 3.0;
    let extrusions = wall_layer_extrusions(&entity, &layers, height);
    assert_eq!(extrusions.len(), 1);
    assert!((extrusions[0].height - 2.8).abs() < 1e-9); // 3.0 + 0.3 - 0.5 = 2.8
    assert!((extrusions[0].base_offset - 0.5).abs() < 1e-9);
}

#[test]
fn regenerate_wall_representation_respects_vertical_offsets() {
    use acadrust::types::Vector2;
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut entity = EntityType::LwPolyline(pl);
    let mut layer = wl("Concrete", 0.2, "Structural");
    layer.bottom_offset = 0.5;
    layer.top_offset = 0.3;
    let layers = vec![layer];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let wall_handle = scene.add_entity(entity);

    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed");

    let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();

    let mut saw_bottom_z = false;
    let mut saw_top_z = false;
    for h in wall.derived_handles {
        let e = scene.document.get_entity(h).unwrap();
        if let EntityType::Solid3D(s3d) = e {
            for wire in &s3d.wires {
                for pt in &wire.points {
                    if (pt.z - 0.5).abs() < 1e-9 {
                        saw_bottom_z = true;
                    }
                    if (pt.z - 3.3).abs() < 1e-9 {
                        saw_top_z = true;
                    }
                }
            }
        }
    }
    assert!(saw_bottom_z, "solid should have wires at Z=0.5");
    assert!(saw_top_z, "solid should have wires at Z=3.3");
}

#[test]
fn wall_regeneration_reflects_updated_layers() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut entity = EntityType::LwPolyline(pl);

    let initial_layers = vec![wl("Brick", 0.1, "Structural")];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values =
        wall_record("style1", 3.0, 0, &initial_layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let wall_handle = scene.add_entity(entity);

    // Initial regeneration
    regenerate_wall_representation(&mut scene, wall_handle, None).unwrap();

    // Update layers
    let updated_layers = vec![wl("Brick", 0.5, "Structural")];
    assert!(write_wall_layers(&mut scene, wall_handle, updated_layers));

    // Regenerate again
    regenerate_wall_representation(&mut scene, wall_handle, None).unwrap();

    let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();
    assert_eq!(wall.layers[0].thickness, 0.5);

    // Check geometry (thickness is reflected in Solid3D width)
    let mut max_y = 0.0;
    for h in wall.derived_handles {
        let e = scene.document.get_entity(h).unwrap();
        if let EntityType::Solid3D(s3d) = e {
            for wire in &s3d.wires {
                for pt in &wire.points {
                    if pt.y.abs() > max_y {
                        max_y = pt.y.abs();
                    }
                }
            }
        }
    }
    // For Center justification, half of 0.5 should be at Y=0.25 and Y=-0.25
    assert!((max_y - 0.25).abs() < 1e-6);
}
