use super::*;

#[test]
fn demolition_regen_sets_dashed_envelope_linetype() {
    let mut scene = Scene::new();
    let handle = add_multi_layer_wall(&mut scene);
    assert!(write_wall_phase(&mut scene, handle, PlanPhase::Demolition));
    let extra = apply_phase_filter(PlanPhase::Demolition, None)
        .extra_style
        .expect("demolition extra");
    let mut rules = engine::display_component::ComponentRuleSet::default();
    merge_phase_extra_into_rules(&mut rules, extra);
    regenerate_wall_representation_with_rules_and_substitutions(
        &mut scene,
        handle,
        Some(&rules),
        None,
        None,
    )
    .expect("regen");
    let wall = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    let dashed = wall
        .derived_handles
        .iter()
        .filter_map(|h| scene.document.get_entity(*h))
        .filter(|e| matches!(e, EntityType::LwPolyline(_)))
        .filter(|e| e.common().linetype.eq_ignore_ascii_case("DASHED"))
        .count();
    assert!(
        dashed >= 1,
        "demolition envelope contour must use DASHED linetype"
    );
}

/// Mirrors the core write-back logic of the `AecStylePickerConfirm`
/// handler for `StylePickerTarget::WallPropertiesStyle` (see
/// `src/app/update/mod.rs`): resolve `effective_layers()` for the newly
/// chosen style from the currently loaded library, then write the new
/// `style_id` + resolved layer snapshot back into the wall's `WALL`
/// XDATA and regenerate its representation.
#[test]
fn changing_wall_properties_style_updates_style_id_and_layer_snapshot() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);

    let mut wall_styles: HashMap<String, WallStyle> = HashMap::new();
    wall_styles.insert(
        "style1".to_string(),
        WallStyle {
            style: Style {
                id: "style1".to_string(),
                name: "Style One".to_string(),
                category: "Wall".to_string(),
                origin: Default::default(),
                parent: None,
            },
            layers: vec![Layer {
                material_id: "Concrete".to_string(),
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
        },
    );
    wall_styles.insert(
        "style2".to_string(),
        WallStyle {
            style: Style {
                id: "style2".to_string(),
                name: "Style Two".to_string(),
                category: "Wall".to_string(),
                origin: Default::default(),
                parent: None,
            },
            layers: vec![
                Layer {
                    material_id: "Brick".to_string(),
                    thickness: LayerValue::Fixed(0.1),
                    function: LayerFunction::Finish,
                    axis_offset: LayerValue::Fixed(0.0),
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    layer_override: None,
                    hatch_override: None,
                    role_tag: None,
                layer_id: uuid::Uuid::new_v4(),
                },
                Layer {
                    material_id: "Insulation".to_string(),
                    thickness: LayerValue::Fixed(0.06),
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
        },
    );

    let new_style_id = "style2".to_string();
    let bb = engine::wall_style::base_width_from_layers(
        &engine::wall_style::effective_layers(&wall_styles, &new_style_id)
            .expect("style2 should resolve"),
    );
    let layers = effective_layers_for_wall_bb(&wall_styles, &new_style_id, bb)
        .expect("style2 should resolve");
    let wall_layers: Vec<WallLayer> = layers
        .into_iter()
        .map(|l| WallLayer {
            material: l.material_id.clone(),
            thickness: l.thickness,
            function: match &l.function {
                LayerFunction::Structural => "Structural".to_string(),
                LayerFunction::Insulation => "Insulation".to_string(),
                LayerFunction::Finish => "Finish".to_string(),
                LayerFunction::Other(s) => s.clone(),
            },
            axis_offset: l.axis_offset,
            bottom_offset: l.bottom_offset,
            top_offset: l.top_offset,
            layer_override: l.layer_override.clone(),
            hatch_override: l.hatch_override.clone(),
        layer_id: uuid::Uuid::new_v4(),
        })
        .collect();

    assert!(write_wall_style(&mut scene, wall_handle, &new_style_id, wall_layers));

    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed after style change");

    let updated = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still parse as WALL");
    assert_eq!(updated.style_id, "style2");
    assert_eq!(updated.layers.len(), 2);
    assert_eq!(updated.layers[0].material, "Brick");
    assert_eq!(updated.layers[1].material, "Insulation");
}

#[test]
fn changing_wall_properties_style_preserves_planes_and_other_properties() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);

    let base_id = uuid::Uuid::new_v4();
    let top_id = uuid::Uuid::new_v4();

    // Set initial custom properties on the wall
    let entity = scene.document.get_entity_mut(wall_handle).unwrap();
    let mut wall = wall_from_entity(entity).expect("WALL");
    wall.base_plane_id = Some(base_id);
    wall.top_plane_id = Some(top_id);
    wall.base_plane_name = Some("Floor Level 0".to_string());
    wall.top_plane_name = Some("Ceiling Level 0".to_string());
    wall.base_offset = 0.15;
    wall.top_offset = -0.25;
    wall.base_origin = [1.0, 2.0, 0.5];
    wall.base_normal = [0.0, 0.0, 1.0];
    wall.top_origin = [1.0, 2.0, 3.5];
    wall.top_normal = [0.0, 0.0, 1.0];
    wall.height = 3.0;
    wall.justification = WallJustification::Interior;
    wall.phase = PlanPhase::Existing;
    wall.storey_id = 42;
    wall.hatch_override = Some(engine::display_component::ComponentStyleOverride {
        hatch_angle: Some(45.0),
        hatch_angle_relative: Some(true),
        ..Default::default()
    });

    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    write_aec_record(&mut scene.document, wall_handle, record);

    // Now change the wall style via write_wall_style
    let new_layers = vec![
        WallLayer {
            material: "Timber".to_string(),
            thickness: 0.14,
            function: "Structural".to_string(),
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
        },
    ];

    assert!(write_wall_style(&mut scene, wall_handle, "TimberWallStyle", new_layers));

    let updated = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still parse as WALL");
    assert_eq!(updated.style_id, "TimberWallStyle");
    assert_eq!(updated.layers.len(), 1);
    assert_eq!(updated.layers[0].material, "Timber");

    // All other properties must be completely preserved!
    assert_eq!(updated.base_plane_id, Some(base_id));
    assert_eq!(updated.top_plane_id, Some(top_id));
    assert_eq!(updated.base_plane_name.as_deref(), Some("Floor Level 0"));
    assert_eq!(updated.top_plane_name.as_deref(), Some("Ceiling Level 0"));
    assert!((updated.base_offset - 0.15).abs() < 1e-9);
    assert!((updated.top_offset - (-0.25)).abs() < 1e-9);
    assert_eq!(updated.base_origin, [1.0, 2.0, 0.5]);
    assert_eq!(updated.base_normal, [0.0, 0.0, 1.0]);
    assert_eq!(updated.top_origin, [1.0, 2.0, 3.5]);
    assert_eq!(updated.top_normal, [0.0, 0.0, 1.0]);
    assert!((updated.height - 3.0).abs() < 1e-9);
    assert_eq!(updated.justification, WallJustification::Interior);
    assert_eq!(updated.phase, PlanPhase::Existing);
    assert_eq!(updated.storey_id, 42);
    assert!(updated.hatch_override.is_some());
    assert_eq!(updated.hatch_override.unwrap().hatch_angle, Some(45.0));
}

#[test]
fn resolve_wall_package_returns_axis_for_a_derived_entity() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a valid two-layer wall");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles;
    assert!(!derived.is_empty());

    for handle in derived {
        assert_eq!(resolve_wall_package(&scene, handle), wall_handle);
    }
}

#[test]
fn resolve_wall_package_uses_derived_handles_without_child_xdata() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed");
    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("WALL")
        .derived_handles;
    assert!(!derived.is_empty());
    let child = derived[0];
    if let Some(entity) = scene.document.get_entity_mut(child) {
        entity.common_mut().extended_data = acadrust::xdata::ExtendedData::default();
    }
    assert_eq!(resolve_wall_package(&scene, child), wall_handle);
    let pkg = wall_package_handles(&scene, wall_handle);
    assert!(pkg.contains(&child));
    assert!(pkg.contains(&wall_handle));
}

#[test]
fn resolve_wall_package_reads_wall_rep_when_not_first_aec_record() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed");
    let child = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("WALL")
        .derived_handles[0];
    if let Some(entity) = scene.document.get_entity_mut(child) {
        let mut dummy = ExtendedDataRecord::new(AEC_APPID);
        dummy.add_value(XDataValue::String("OTHER".into()));
        let mut records = vec![dummy];
        records.extend(entity.common().extended_data.records().iter().cloned());
        entity.common_mut().extended_data.clear();
        for r in records {
            entity.common_mut().extended_data.add_record(r);
        }
    }
    assert_eq!(resolve_wall_package(&scene, child), wall_handle);
}

#[test]
fn resolve_wall_package_accepts_integer_axis_handle() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed");
    let child = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("WALL")
        .derived_handles[0];
    if let Some(entity) = scene.document.get_entity_mut(child) {
        entity.common_mut().extended_data.clear();
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.add_value(XDataValue::String("WALL_REP".into()));
        record.add_value(XDataValue::Integer32(wall_handle.value() as i32));
        record.add_value(XDataValue::String(WALL_REP_ROLE_CONTOUR.into()));
        entity.common_mut().extended_data.add_record(record);
    }
    assert_eq!(resolve_wall_package(&scene, child), wall_handle);
}

#[test]
fn resolve_wall_package_matches_untagged_layer_polyline_by_geometry() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed");
    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("WALL")
        .derived_handles;
    let child = derived
        .iter()
        .copied()
        .find(|&h| matches!(scene.document.get_entity(h), Some(EntityType::LwPolyline(pl)) if pl.vertices.len() >= 3))
        .expect("layer contour");
    if let Some(entity) = scene.document.get_entity_mut(child) {
        entity.common_mut().extended_data = acadrust::xdata::ExtendedData::default();
    }
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        panic!("wall");
    };
    let mut wall = wall_from_entity(entity).expect("WALL");
    wall.derived_handles.clear();
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    for v in wall_record(
        &wall.style_id,
        wall.height,
        wall.storey_id,
        &wall.layers,
        &wall.derived_handles,
        wall.justification,
        wall.phase,
        wall.hatch_override.as_ref(),
    ) {
        record.add_value(v);
    }
    write_aec_record(&mut scene.document, wall_handle, record);
    assert_eq!(resolve_wall_package(&scene, child), wall_handle);
}

#[test]
fn resolve_wall_package_matches_untagged_solid_by_geometry() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed");
    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("WALL")
        .derived_handles;
    let child = derived
        .iter()
        .copied()
        .find(|&h| matches!(scene.document.get_entity(h), Some(EntityType::Solid3D(_))))
        .expect("layer solid");
    if let Some(entity) = scene.document.get_entity_mut(child) {
        entity.common_mut().extended_data = acadrust::xdata::ExtendedData::default();
    }
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        panic!("wall");
    };
    let mut wall = wall_from_entity(entity).expect("WALL");
    wall.derived_handles.clear();
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    for v in wall_record(
        &wall.style_id,
        wall.height,
        wall.storey_id,
        &wall.layers,
        &wall.derived_handles,
        wall.justification,
        wall.phase,
        wall.hatch_override.as_ref(),
    ) {
        record.add_value(v);
    }
    write_aec_record(&mut scene.document, wall_handle, record);
    assert_eq!(resolve_wall_package(&scene, child), wall_handle);
    let expanded = expand_handles_for_wall_packages(&scene, &[child]);
    assert!(expanded.contains(&wall_handle));
}

#[test]
fn regen_tags_display_children_with_wall_rep_roles() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a valid two-layer wall");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles;
    let mut saw_contour = false;
    let mut saw_hatch = false;
    let mut saw_solid = false;
    for handle in derived {
        let entity = scene.document.get_entity(handle).unwrap();
        let record = read_aec_record(entity).expect("display child must carry WALL_REP");
        match record.values.as_slice() {
            [XDataValue::String(kind), XDataValue::Handle(axis), XDataValue::String(role)] => {
                assert_eq!(kind, "WALL_REP");
                assert_eq!(*axis, wall_handle);
                match role.as_str() {
                    WALL_REP_ROLE_CONTOUR => saw_contour = true,
                    WALL_REP_ROLE_HATCH => saw_hatch = true,
                    WALL_REP_ROLE_SOLID => saw_solid = true,
                    other => panic!("unexpected display role {other}"),
                }
            }
            other => panic!("unexpected display XDATA {other:?}"),
        }
    }
    assert!(saw_contour && saw_hatch && saw_solid);
}

#[test]
fn wall_hatch_scale_survives_document_clone_roundtrip() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles;
    let hatch_handles: Vec<_> = derived
        .into_iter()
        .filter(|&handle| {
            matches!(
                scene.document.get_entity(handle),
                Some(EntityType::Hatch(_))
            )
        })
        .collect();
    assert!(!hatch_handles.is_empty());

    let before: Vec<(f32, f32)> = hatch_handles
        .iter()
        .map(|handle| {
            let model = scene.hatches.get(handle).expect("hatch model");
            (model.scale, effective_hatch_spacing(model))
        })
        .collect();

    let mut reloaded = Scene::new();
    reloaded.document = scene.document.clone();
    reloaded.populate_hatches_from_document();

    for (handle, (scale, spacing)) in hatch_handles.iter().zip(before) {
        let model = reloaded.hatches.get(handle).expect("reloaded hatch model");
        assert!(
            (model.scale - scale).abs() < 1e-5,
            "HatchModel.scale changed on reload: {} -> {}",
            scale,
            model.scale
        );
        assert!(
            (effective_hatch_spacing(model) - spacing).abs() < 1e-4,
            "effective hatch spacing changed on reload"
        );
    }
}

pub(super) fn effective_hatch_spacing(model: &crate::scene::model::hatch_model::HatchModel) -> f32 {
    match &model.pattern {
        crate::scene::model::hatch_model::HatchPattern::Pattern(fams) => {
            fams.first().map(|f| f.dy.abs() * model.scale.max(1e-6)).unwrap_or(0.0)
        }
        _ => 0.0,
    }
}

#[test]
fn regenerating_many_walls_completes_quickly() {
    // Rough performance smoke test (not a micro-benchmark): regenerating
    // a batch of walls — mixing straight and curved axes, multiple
    // layers, and openings — must not show a gross performance
    // regression (e.g. an accidental O(n^2) added by a future change).
    // Generous wall-clock bound so this stays robust on slow/loaded CI
    // hardware while still catching an order-of-magnitude regression.
    use std::time::Instant;

    let mut scene = Scene::new();
    let mut handles = Vec::new();
    const WALL_COUNT: usize = 200;
    for i in 0..WALL_COUNT {
        let x0 = i as f64 * 6.0;
        let mut pl = LwPolyline::new();
        if i % 3 == 0 {
            // Every third wall is curved.
            let mut v0 = LwVertex::new(Vector2::new(x0, 0.0));
            v0.bulge = 0.3;
            pl.add_vertex(v0);
        } else {
            pl.add_vertex(LwVertex::new(Vector2::new(x0, 0.0)));
        }
        pl.add_vertex(LwVertex::new(Vector2::new(x0 + 5.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = wls(&[("Concrete", 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values =
            wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        handles.push(scene.add_entity(entity));
    }

    let started = Instant::now();
    for h in &handles {
        regenerate_wall_representation(&mut scene, *h, None)
            .expect("regeneration should succeed for every generated wall");
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed.as_secs() < 25,
        "regenerating {WALL_COUNT} walls took {elapsed:?}, expected well under 25s"
    );
}

#[test]
fn tessellate_ring_with_bulges_is_a_noop_for_straight_rings() {
    let ring = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 5.0), (0.0, 5.0)];
    let bulges = vec![0.0; 4];
    assert_eq!(tessellate_ring_with_bulges(&ring, &bulges), ring);
    // Absent bulges (shorter slice / empty) also stay a no-op.
    assert_eq!(tessellate_ring_with_bulges(&ring, &[]), ring);
}

#[test]
fn tessellate_ring_with_bulges_densifies_arc_edges() {
    // A ring whose first edge is a semicircular arc (bulge = 1.0) from
    // (-1,0) to (1,0); the rest are straight closing edges.
    let ring = vec![(-1.0, 0.0), (1.0, 0.0), (1.0, -2.0), (-1.0, -2.0)];
    let bulges = vec![1.0, 0.0, 0.0, 0.0];
    let dense = tessellate_ring_with_bulges(&ring, &bulges);
    assert!(
        dense.len() > ring.len(),
        "an arc edge must be sampled into more than its two endpoints"
    );
    // Straight edges keep exactly their start vertex (no extra samples);
    // only the arc edge grows.
    assert_eq!(dense.len(), WALL_HATCH_ARC_SEGMENTS + 3);
    // The arc bulges outward from the chord: some sampled point should
    // be well off the y=0 chord line (the semicircle's midpoint sits a
    // full radius away, at y = ±1 depending on winding/bulge sign).
    assert!(dense.iter().any(|&(_, y)| y.abs() > 0.5));
}

#[test]
fn regenerate_wall_representation_tessellates_hatch_boundary_for_curved_wall() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    // Semicircular arc axis (bulge = 1.0) so the layer footprint has a
    // curved edge.
    let mut v0 = LwVertex::new(Vector2::new(-2.0, 0.0));
    v0.bulge = 1.0;
    pl.add_vertex(v0);
    pl.add_vertex(LwVertex::new(Vector2::new(2.0, 0.0)));
    let mut entity = EntityType::LwPolyline(pl);
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let wall_handle = scene.add_entity(entity);

    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a curved single-layer wall");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles;

    let mut found_dense_hatch = false;
    for h in derived {
        if let EntityType::Hatch(hatch) = scene.document.get_entity(h).unwrap() {
            // A curved single-layer footprint has 4 raw vertices (2 per
            // side); the tessellated boundary must be denser.
            if hatch.paths.iter().any(|p| {
                p.edges.iter().any(|e| {
                    matches!(e, acadrust::entities::hatch::BoundaryEdge::Polyline(pl) if pl.vertices.len() > 4)
                })
            }) {
                found_dense_hatch = true;
            }
        }
    }
    assert!(
        found_dense_hatch,
        "curved wall's hatch boundary should be tessellated into more than the raw 4 corner vertices"
    );
}

#[test]
fn regen_uses_layer_hatch_override_for_hatch_pattern() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut entity = EntityType::LwPolyline(pl);
    let mut overridden = wl("Concrete", 0.2, "Structural");
    overridden.hatch_override = Some("NET".to_string());
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

    let mut saw_override_pattern = false;
    let mut saw_default_pattern = false;
    for h in derived {
        if let EntityType::Hatch(hatch) = scene.document.get_entity(h).unwrap() {
            if hatch.pattern.name == "NET" {
                saw_override_pattern = true;
            } else if hatch.pattern.name == "ANSI31" {
                saw_default_pattern = true;
            }
        }
    }
    assert!(
        saw_override_pattern,
        "the overridden layer's hatch should use the layer's hatch_override pattern"
    );
    assert!(
        saw_default_pattern,
        "the non-overridden layer should keep falling back to the material/default pattern"
    );
}

#[test]
fn wall_from_entity_round_trips_layer_hatch_override() {
    let mut layer = wl("Concrete", 0.2, "Structural");
    layer.hatch_override = Some("NET".to_string());
    let layers = vec![layer, wl("Insulation", 0.05, "Insulation")];
    let values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    let pl = LwPolyline::new();
    let mut entity = EntityType::LwPolyline(pl);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = values;
    entity.common_mut().extended_data.add_record(record);

    let wall = wall_from_entity(&entity).expect("should parse WALL");
    assert_eq!(wall.layers[0].hatch_override.as_deref(), Some("NET"));
    assert_eq!(wall.layers[1].hatch_override, None);
}

#[test]
fn hidden_host_cut_keeps_closed_2d_contours_but_still_cuts_wall_solids() {
    use engine::display_component::{ComponentRuleSet, OpeningComponentSlot};

    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None).expect("initial regen");
    let uncut_contours = wall_layer_contour_loops(&scene, wall_handle);
    assert_eq!(uncut_contours.len(), 2);

    place_wall_opening(
        &mut scene,
        wall_handle,
        DVec3::new(2.5, 0.0, 0.0),
        OpeningKind::Window,
        None,
        None,
        None,
    )
    .expect("place window");
    let cut_contours = wall_layer_contour_loops(&scene, wall_handle);
    let cut_solids = wall_layer_solid_wire_base_points(&scene, wall_handle);
    assert!(cut_contours.len() > uncut_contours.len());
    assert!(!cut_solids.is_empty());

    let mut rules = ComponentRuleSet::default();
    rules
        .visibility
        .insert(OpeningComponentSlot::HostCut2D.key().to_string(), false);
    regenerate_wall_representation_with_rules(&mut scene, wall_handle, Some(&rules), None)
        .expect("regen with host cut hidden");

    let hidden_cut_contours = wall_layer_contour_loops(&scene, wall_handle);
    assert_eq!(hidden_cut_contours.len(), uncut_contours.len());
    for (uncut, hidden) in uncut_contours.iter().zip(&hidden_cut_contours) {
        assert_point_sets_match(uncut, hidden, 1e-4, "host-cut-hidden contour");
    }

    let hidden_cut_solids = wall_layer_solid_wire_base_points(&scene, wall_handle);
    assert_eq!(hidden_cut_solids.len(), cut_solids.len());
    for (cut, hidden) in cut_solids.iter().zip(&hidden_cut_solids) {
        assert_point_sets_match(cut, hidden, 1e-4, "host-cut-hidden solid");
    }
}

#[test]
fn layer_filter_for_contour_and_solid_can_differ_within_one_profile() {
    use engine::display_component::{ComponentRuleSet, LayerSelection, WallComponentSlot};
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    let layer_ref = join::LayerRef { material_id: "Concrete".to_string(), role_tag: None, index: 0, layer_id: None };
    let mut rules = ComponentRuleSet::default();
    rules.layer_filter.insert(
        WallComponentSlot::Contour2D.key().to_string(),
        LayerSelection::Explicit(vec![layer_ref.clone()]),
    );
    // `Solid3D` left unset -> defaults to `All` (non-regression).
    let handles = regenerate_wall_representation_with_rules_and_substitutions(
        &mut scene,
        wall_handle,
        Some(&rules),
        None,
        None,
    )
    .expect("regeneration should succeed");
    assert!(!handles.is_empty());
    assert_eq!(rules.layer_filter_for(WallComponentSlot::Contour2D), &LayerSelection::Explicit(vec![layer_ref]));
    assert_eq!(rules.layer_filter_for(WallComponentSlot::Solid3D), &LayerSelection::All);

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles;
    let contour_count = derived
        .iter()
        .filter(|handle| matches!(scene.document.get_entity(**handle), Some(EntityType::LwPolyline(_))))
        .count();
    let solid_count = derived
        .iter()
        .filter(|handle| matches!(scene.document.get_entity(**handle), Some(EntityType::Solid3D(_))))
        .count();
    assert_eq!(contour_count, 1, "Contour2D must include only the explicit layer");
    assert_eq!(solid_count, 2, "Solid3D must keep both layers when its filter defaults to All");
}

#[test]
fn hatch_override_chain_wall_takes_precedence_over_material() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut entity = EntityType::LwPolyline(pl);
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    let wall_hatch_override = engine::display_component::ComponentStyleOverride {
        hatch_angle: Some(45.0),
        hatch_angle_relative: Some(false),
        ..Default::default()
    };
    record.values = wall_record(
        "style1",
        3.0,
        0,
        &layers,
        &[],
        WallJustification::Center,
        PlanPhase::New,
        Some(&wall_hatch_override),
    );
    entity.common_mut().extended_data.add_record(record);
    let wall_handle = scene.add_entity(entity);

    let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should parse WALL");
    assert_eq!(wall.hatch_override, Some(wall_hatch_override));

    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a single-layer wall");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles;
    let mut hatch_count = 0;
    for handle in derived {
        if let Some(EntityType::Hatch(hatch)) = scene.document.get_entity(handle) {
            hatch_count += 1;
            assert!(
                (hatch.pattern_angle - std::f64::consts::FRAC_PI_4).abs() < 1.0e-5,
                "wall hatch override should render at 45 degrees, got {} radians",
                hatch.pattern_angle
            );
        }
    }
    assert_eq!(hatch_count, 1, "the single-layer wall should produce one hatch");
}

#[test]
fn apply_phase_filter_hides_and_overlays_as_expected_end_to_end() {
    use engine::plan_view::{PhaseFilter, PlanPhase as Phase};
    let filter = PhaseFilter {
        visible_phases: vec![Phase::New, Phase::Demolition],
        demolition_style: Some(engine::display_component::ComponentStyleOverride {
            line_color: Some(acadrust::types::Color::Rgb { r: 255, g: 0, b: 0 }),
            ..Default::default()
        }),
        existing_style: None,
    };
    assert!(apply_phase_filter(Phase::New, Some(&filter)).visible);
    assert!(apply_phase_filter(Phase::Demolition, Some(&filter)).visible);
    assert!(!apply_phase_filter(Phase::Existing, Some(&filter)).visible);
    assert_eq!(
        apply_phase_filter(Phase::Demolition, Some(&filter)).extra_style.unwrap().line_color, Some(acadrust::types::Color::Rgb { r: 255, g: 0, b: 0 })
    );
}

#[test]
fn resolve_wall_package_returns_itself_for_a_non_wall_entity() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(1.0, 0.0)));
    let handle = scene.add_entity(EntityType::LwPolyline(pl));

    assert_eq!(resolve_wall_package(&scene, handle), handle);
}

#[test]
fn expand_handles_for_wall_packages_includes_owner_and_children() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a valid two-layer wall");
    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles;
    assert!(!derived.is_empty());

    let from_child = expand_handles_for_wall_packages(&scene, &[derived[0]]);
    let from_owner = expand_handles_for_wall_packages(&scene, &[wall_handle]);
    assert!(from_child.contains(&wall_handle));
    for handle in &derived {
        assert!(from_child.contains(handle));
        assert!(from_owner.contains(handle));
    }
    assert_eq!(from_child.len(), from_owner.len());
    assert!(!is_wall_derived_non_axis(&scene, wall_handle));
    for handle in &derived {
        assert!(is_wall_derived_non_axis(&scene, *handle));
    }
}

#[test]
fn expand_with_wall_derived_handles_resolves_child_to_full_package() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a valid two-layer wall");
    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles;
    assert!(!derived.is_empty());

    let mut handles = vec![derived[0]];
    expand_with_wall_derived_handles(&scene, &mut handles);
    assert!(handles.contains(&wall_handle));
    for handle in &derived {
        assert!(handles.contains(handle));
    }
}

#[test]
fn write_wall_height_from_derived_child_updates_owner() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a valid two-layer wall");
    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles[0];
    assert!(write_wall_height(&mut scene, derived, 4.2));
    let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("owner should still parse as WALL");
    assert!((wall.height - 4.2).abs() < 1e-9);
}

#[test]
fn live_wall_properties_include_style_height_and_justification() {
    use crate::command::LiveFieldValue;
    let cmd = WallCommand::new_with_library(None);
    let props = cmd.live_properties().expect("drawing phase exposes live props");
    let ids: Vec<_> = props.fields.iter().map(|f| f.field_id).collect();
    assert!(ids.contains(&"wall_style"));
    assert!(ids.contains(&"wall_height"));
    assert!(ids.contains(&"wall_justification"));
    assert!(matches!(
        props.fields.iter().find(|f| f.field_id == "wall_justification"),
        Some(f) if matches!(f.value, LiveFieldValue::Choice { .. })
    ));
}
