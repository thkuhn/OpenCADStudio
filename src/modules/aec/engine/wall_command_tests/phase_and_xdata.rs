use super::*;

#[test]
fn apply_phase_filter_with_no_filter_shows_everything_unstyled() {
    let new_phase = apply_phase_filter(PlanPhase::New, None);
    assert!(new_phase.visible);
    assert_eq!(new_phase.extra_style, None);

    let demolition = apply_phase_filter(PlanPhase::Demolition, None);
    assert!(demolition.visible);
    assert_eq!(
        demolition.extra_style.unwrap().line_type.as_deref(),
        Some("DASHED")
    );

    let existing = apply_phase_filter(PlanPhase::Existing, None);
    assert!(existing.visible);
    assert_eq!(
        existing.extra_style.unwrap().line_color,
        Some(acadrust::types::Color::Rgb {
            r: 136,
            g: 136,
            b: 136
        })
    );
}

#[test]
fn apply_phase_filter_hides_phases_missing_from_visible_phases() {
    let filter = PhaseFilter {
        visible_phases: vec![PlanPhase::New],
        demolition_style: None,
        existing_style: None,
    };
    assert!(apply_phase_filter(PlanPhase::New, Some(&filter)).visible);
    assert!(!apply_phase_filter(PlanPhase::Demolition, Some(&filter)).visible);
    assert!(!apply_phase_filter(PlanPhase::Existing, Some(&filter)).visible);
}

#[test]
fn apply_phase_filter_applies_demolition_and_existing_overlays() {
    let filter = PhaseFilter {
        visible_phases: vec![PlanPhase::New, PlanPhase::Demolition, PlanPhase::Existing],
        demolition_style: Some(engine::display_component::ComponentStyleOverride {
            line_type: Some("Dashed".to_string()),
            ..Default::default()
        }),
        existing_style: Some(engine::display_component::ComponentStyleOverride {
            line_color: Some(acadrust::types::Color::Rgb { r: 136, g: 136, b: 136 }),
            ..Default::default()
        }),
    };
    let demolition = apply_phase_filter(PlanPhase::Demolition, Some(&filter));
    assert!(demolition.visible);
    assert_eq!(demolition.extra_style.unwrap().line_type, Some("Dashed".to_string()));

    let existing = apply_phase_filter(PlanPhase::Existing, Some(&filter));
    assert!(existing.visible);
    assert_eq!(existing.extra_style.unwrap().line_color, Some(acadrust::types::Color::Rgb { r: 136, g: 136, b: 136 }));

    // `New` walls never pick up an overlay, even if visible.
    let new_phase = apply_phase_filter(PlanPhase::New, Some(&filter));
    assert!(new_phase.visible);
    assert_eq!(new_phase.extra_style, None);
}

#[test]
fn merge_phase_extra_only_overrides_contour2d() {
    let extra = engine::display_component::ComponentStyleOverride {
        line_type: Some("DASHED".to_string()),
        line_color: Some(acadrust::types::Color::Rgb { r: 255, g: 0, b: 0 }),
        ..Default::default()
    };
    let mut rules = engine::display_component::ComponentRuleSet::default();
    rules.visibility.insert(
        engine::display_component::WallComponentSlot::Layers2D
            .key()
            .to_string(),
        true,
    );
    merge_phase_extra_into_rules(&mut rules, extra.clone());
    let contour = rules
        .style_for(engine::display_component::WallComponentSlot::Contour2D)
        .expect("phase extra on envelope");
    assert_eq!(contour.line_type.as_deref(), Some("DASHED"));
    assert_eq!(
        contour.line_color,
        Some(acadrust::types::Color::Rgb { r: 255, g: 0, b: 0 })
    );
    assert!(
        rules
            .style_for(engine::display_component::WallComponentSlot::Layers2D)
            .is_none()
    );
    assert!(
        rules
            .style_for(engine::display_component::WallComponentSlot::LayerHatch2D)
            .is_none()
    );
    assert!(
        rules
            .style_for(engine::display_component::WallComponentSlot::Solid3D)
            .is_none()
    );
}

#[test]
fn wall_round_trip() {
    let layers = wls(&[
        ("Finish", 0.02, "Finish"),
        ("Brick", 0.10, "Structural"),
        ("Finish", 0.02, "Finish"),
    ]);
    let values = wall_record("style1", 3.0, 1, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    let pl = LwPolyline::new();
    let mut entity = EntityType::LwPolyline(pl);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = values;
    entity.common_mut().extended_data.add_record(record);

    let wall = wall_from_entity(&entity).expect("Should parse WALL");
    assert_eq!(wall.style_id, "style1");
    assert_eq!(wall.height, 3.0);
    assert_eq!(wall.storey_id, 1);
    assert_eq!(wall.layers.len(), 3);
    assert_eq!(wall.layers[1].material, "Brick");
    assert_eq!(wall.layers[1].thickness, 0.10);
    assert_eq!(wall.layers[1].function, "Structural");
    assert_eq!(wall.total_thickness(), 0.14);
    assert_eq!(wall.phase, PlanPhase::New);
}

#[test]
fn wall_from_entity_round_trips_phase() {
    let layers = wls(&[("Brick", 0.2, "Structural")]);
    let values = wall_record(
        "style1",
        3.0,
        0,
        &layers,
        &[],
        WallJustification::Center,
        PlanPhase::Demolition,
        None,
    );
    let pl = LwPolyline::new();
    let mut entity = EntityType::LwPolyline(pl);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = values;
    entity.common_mut().extended_data.add_record(record);

    let wall = wall_from_entity(&entity).expect("Should parse WALL");
    assert_eq!(wall.phase, PlanPhase::Demolition);
}

#[test]
fn wall_record_round_trips_control_planes() {
    let mut wall = Wall::new("style1", 2.8, 0);
    wall.layers = wls(&[("Brick", 0.2, "Structural")]);
    wall.base_plane_id = Some(Uuid::new_v4());
    wall.top_plane_id = Some(Uuid::new_v4());
    wall.base_offset = 0.1;
    wall.top_offset = -0.05;
    wall.base_origin = [0.0, 0.0, 1.1];
    wall.base_normal = [0.0, 0.0, 1.0];
    wall.top_origin = [0.0, 0.0, 3.95];
    wall.top_normal = [0.0, 0.0, 1.0];
    wall.height = wall.height_from_snapshot().unwrap_or(wall.height);

    let mut entity = EntityType::LwPolyline(LwPolyline::new());
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    entity.common_mut().extended_data.add_record(record);

    let back = wall_from_entity(&entity).expect("parse");
    assert_eq!(back.base_plane_id, wall.base_plane_id);
    assert_eq!(back.top_plane_id, wall.top_plane_id);
    assert!((back.base_offset - 0.1).abs() < 1e-9);
    assert!((back.height - 2.85).abs() < 1e-6);
}

#[test]
fn wall_record_round_trips_named_control_planes() {
    let mut wall = Wall::new("style1", 2.8, 0);
    wall.layers = wls(&[("Brick", 0.2, "Structural")]);
    wall.base_plane_id = Some(Uuid::new_v4());
    wall.top_plane_id = Some(Uuid::new_v4());
    wall.base_plane_name = Some("EG_ELEVATION".to_string());
    wall.top_plane_name = Some("EG_OKGH".to_string());
    wall.base_offset = 0.1;
    wall.top_offset = -0.05;

    let mut entity = EntityType::LwPolyline(LwPolyline::new());
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    entity.common_mut().extended_data.add_record(record);

    let back = wall_from_entity(&entity).expect("parse named planes");
    assert_eq!(back.base_plane_id, wall.base_plane_id);
    assert_eq!(back.top_plane_id, wall.top_plane_id);
    assert_eq!(back.base_plane_name.as_deref(), Some("EG_ELEVATION"));
    assert_eq!(back.top_plane_name.as_deref(), Some("EG_OKGH"));
}

#[test]
fn write_aec_record_drops_stale_dwg_eed_so_planes_can_save() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    ensure_app_id(&mut scene.document);
    let ah = scene
        .document
        .app_ids
        .get(AEC_APPID)
        .expect("AEC app")
        .handle
        .value();
    {
        let entity = scene.document.get_entity_mut(wall_handle).unwrap();
        entity
            .common_mut()
            .extended_data
            .raw_dwg_eed
            .push((ah, vec![0xDE, 0xAD]));
    }
    let mut wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();
    wall.base_plane_id = Some(Uuid::new_v4());
    wall.base_plane_name = Some("EG_ELEVATION".to_string());
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    assert!(write_aec_record(&mut scene.document, wall_handle, record));
    let entity = scene.document.get_entity(wall_handle).unwrap();
    assert!(
        entity
            .common()
            .extended_data
            .raw_dwg_eed
            .iter()
            .all(|(a, _)| *a != ah),
        "stale DWG EED must not win on the next save"
    );
    let back = wall_from_entity(entity).unwrap();
    assert_eq!(back.base_plane_name.as_deref(), Some("EG_ELEVATION"));
    assert_eq!(back.base_plane_id, wall.base_plane_id);
}

#[test]
fn wall_planes_survive_real_xdata_after_dwg_load() {
    let mut wall = Wall::new("style1", 2.8, 0);
    wall.layers = wls(&[("Brick", 0.2, "Structural")]);
    wall.base_plane_id = Some(Uuid::new_v4());
    wall.top_plane_id = Some(Uuid::new_v4());
    wall.base_offset = 0.15;
    wall.top_offset = -0.2;
    let mut values = wall_record_for_wall(&wall);
    let planes_at = values
        .iter()
        .rposition(|v| matches!(v, XDataValue::String(s) if s == "planes"))
        .unwrap();
    for v in values.iter_mut().skip(planes_at + 3) {
        if let XDataValue::Distance(d) = *v {
            *v = XDataValue::Real(d);
        }
    }
    let mut entity = EntityType::LwPolyline(LwPolyline::new());
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = values;
    entity.common_mut().extended_data.add_record(record);
    let back = wall_from_entity(&entity).expect("parse reals");
    assert_eq!(back.base_plane_id, wall.base_plane_id);
    assert_eq!(back.top_plane_id, wall.top_plane_id);
    assert!((back.base_offset - 0.15).abs() < 1e-9);
    assert!((back.top_offset + 0.2).abs() < 1e-9);
}

#[test]
fn wall_from_entity_ignores_missing_plane_block() {
    let layers = wls(&[("Brick", 0.2, "Structural")]);
    let mut values = wall_record(
        "style1",
        3.0,
        0,
        &layers,
        &[],
        WallJustification::Center,
        PlanPhase::New,
        None,
    );
    if let Some(pos) = values.iter().rposition(|v| {
        matches!(v, XDataValue::String(s) if s == "planes")
    }) {
        values.truncate(pos);
    }
    let mut entity = EntityType::LwPolyline(LwPolyline::new());
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = values;
    entity.common_mut().extended_data.add_record(record);
    let wall = wall_from_entity(&entity).expect("legacy");
    assert!(wall.base_plane_id.is_none());
    assert_eq!(wall.height, 3.0);
}

#[test]
fn wall_without_phase_tail_defaults_to_new() {
    // Simulates a record written before the `phase` field existed: the
    // trailing tag is simply absent, and parsing must fall back to
    // `PlanPhase::New` rather than failing.
    let layers = wls(&[("Brick", 0.2, "Structural")]);
    let mut values = wall_record(
        "style1",
        3.0,
        0,
        &layers,
        &[],
        WallJustification::Center,
        PlanPhase::Demolition,
        None,
    );
    if let Some(pos) = values.iter().position(|v| matches!(v, XDataValue::String(s) if s == "Demolition")) {
        values.truncate(pos);
    }
    let pl = LwPolyline::new();
    let mut entity = EntityType::LwPolyline(pl);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = values;
    entity.common_mut().extended_data.add_record(record);

    let wall = wall_from_entity(&entity).expect("Should parse WALL");
    assert_eq!(wall.phase, PlanPhase::New);
}

#[test]
fn write_wall_height_preserves_phase() {
    let mut scene = Scene::new();
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("two points should commit a wall segment"),
    };
    let handle = scene.add_entity(entity);

    // Manually flip the phase to `Existing`, mimicking a prior
    // Properties-panel edit, then confirm a later height edit preserves it.
    let mut wall = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    wall.phase = PlanPhase::Existing;
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record(
        &wall.style_id,
        wall.height,
        wall.storey_id,
        &wall.layers,
        &wall.derived_handles,
        wall.justification,
        wall.phase,
        wall.hatch_override.as_ref(),
    );
    write_aec_record(&mut scene.document, handle, record);

    assert!(write_wall_height(&mut scene, handle, 3.2));

    let updated = wall_from_entity(scene.document.get_entity(handle).unwrap())
        .expect("entity should still read back as a wall after the edit");
    assert_eq!(updated.phase, PlanPhase::Existing);
    assert!((updated.height - 3.2).abs() < 1e-9);
}

#[test]
fn wall_thickness_and_height_reads_wall_record() {
    let pl = LwPolyline::new();
    let mut entity = EntityType::LwPolyline(pl);
    let layers = vec![wl("Mat", 0.15, "Func")];
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = wall_record("style2", 3.2, 2, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(rec);

    let res = wall_thickness_and_height(&entity).expect("Should read WALL");
    assert_eq!(res, (0.15, 3.2, 2));
}

#[test]
fn aec_room_detects_a_closed_loop_from_walls() {
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
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(pair[0].x, pair[0].y)));
        pl.add_vertex(LwVertex::new(Vector2::new(pair[1].x, pair[1].y)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Brick", 0.2, "Structural")];
        let mut r = ExtendedDataRecord::new(AEC_APPID);
        r.values = wall_record("style1", 2.8, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(r);
        scene.add_entity(entity);
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
    assert!((area - 12.0).abs() < 1e-6);
}

#[test]
fn wall_command_with_library_uses_ask_style_and_finalizes() {
    use crate::command::LiveFieldValue;
    use crate::modules::aec::engine::material::Material;
    use crate::modules::aec::engine::style::Style;
    use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, WallStyle};

    let material = Material::new(
        "brick_id".to_string(),
        "Brick Material".to_string(),
        "ANSI31".to_string(),
        0xFF0000,
        "Continuous".to_string(),
    );
    let style = WallStyle {
        style: Style {
            id: "style1".to_string(),
            name: "Standard Wall".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![Layer {
            material_id: "brick_id".to_string(),
            thickness: LayerValue::Fixed(0.25),
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
        materials: vec![material],
        wall_styles: vec![style],
        ..Default::default()
    };

    let mut cmd = WallCommand::new_with_library(Some(lib));
    cmd.apply_live_property("wall_height", LiveFieldValue::Number(3.0));
    cmd.apply_live_property(
        "wall_style",
        LiveFieldValue::Picker("style1".to_string()),
    );
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => {
            let pl = match &entity {
                EntityType::LwPolyline(pl) => pl,
                _ => panic!("expected a wall polyline"),
            };
            let record = pl.common.extended_data.get_record(AEC_APPID).unwrap();
            assert_eq!(record.values[0], XDataValue::String("WALL".to_string()));
            assert_eq!(record.values[1], XDataValue::String("style1".to_string()));
            assert!(
                matches!(record.values[2], XDataValue::Distance(h) if (h - 3.0).abs() < 1e-9)
            );
            assert_eq!(
                record.values[5],
                XDataValue::String("Brick Material".to_string())
            );
            assert!(
                matches!(record.values[6], XDataValue::Distance(t) if (t - 0.25).abs() < 1e-9)
            );
            assert_eq!(
                record.values[7],
                XDataValue::String("Structural".to_string())
            );
        }
        _ => panic!("expected second click to commit a styled segment"),
    }
}

#[test]
fn wall_command_live_properties_reports_style_and_height_while_drawing() {
    use crate::command::LiveFieldValue;

    let mut cmd = WallCommand::new_with_library(None);
    // Still in the Drawing phase (no points yet): live_properties should
    // be available with the default height and an empty style name.
    let live = cmd.live_properties().expect("Drawing phase should expose live properties");
    assert_eq!(live.title, "Wall");
    assert_eq!(live.fields.len(), 3);
    assert_eq!(live.fields[0].field_id, "wall_style");
    assert_eq!(live.fields[1].field_id, "wall_height");
    assert_eq!(live.fields[2].field_id, "wall_justification");
    assert!(matches!(&live.fields[1].value, LiveFieldValue::Number(h) if (*h - DEFAULT_WALL_HEIGHT).abs() < 1e-9));

    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    cmd.on_point(DVec3::new(5.0, 0.0, 0.0));

    cmd.apply_live_property("wall_height", LiveFieldValue::Number(3.5));
    assert!((cmd.wall.height - 3.5).abs() < 1e-9);

    let live_after = cmd.live_properties().expect("still drawing");
    assert!(matches!(&live_after.fields[1].value, LiveFieldValue::Number(h) if (*h - 3.5).abs() < 1e-9));
}

#[test]
fn bound_wall_commit_keeps_live_height_and_rebake_height() {
    use crate::command::LiveFieldValue;
    use crate::modules::aec::engine::project::StoreyRef;

    let storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
    let mut cmd = WallCommand::new_with_library(None).with_storey_planes(&storey);
    assert!((cmd.wall.height - 3.0).abs() < 1e-9);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let unbound = match cmd.on_point(DVec3::new(4.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("expected commit"),
    };
    let baked = wall_from_entity(&unbound).expect("wall");
    assert!(
        (baked.height - 3.0).abs() < 1e-12,
        "rebake height must survive commit, got {}",
        baked.height
    );
    assert!((baked.base_origin[2] - 0.0).abs() < 1e-12);

    let mut cmd = WallCommand::new_with_library(None).with_storey_planes(&storey);
    cmd.wall.base_offset = 0.1;
    cmd.wall.top_offset = -0.05;
    cmd.wall.rebake_planes(&storey, 0.0, 0.0);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let offset_entity = match cmd.on_point(DVec3::new(4.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("expected commit"),
    };
    let offset_wall = wall_from_entity(&offset_entity).expect("wall");
    assert!(
        (offset_wall.height - 2.85).abs() < 1e-12,
        "offsets must not be applied twice on commit, got {}",
        offset_wall.height
    );
    assert!((offset_wall.base_origin[2] - 0.1).abs() < 1e-12);
    assert!((offset_wall.top_origin[2] - 2.95).abs() < 1e-12);

    let mut cmd = WallCommand::new_with_library(None).with_storey_planes(&storey);
    cmd.apply_live_property("wall_height", LiveFieldValue::Number(2.5));
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let live_entity = match cmd.on_point(DVec3::new(4.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("expected commit"),
    };
    let live_wall = wall_from_entity(&live_entity).expect("wall");
    assert!(
        (live_wall.height - 2.5).abs() < 1e-12,
        "live Properties height must survive commit, got {}",
        live_wall.height
    );
}

#[test]
fn wall_command_apply_live_property_updates_style_and_resolved_layers() {
    use crate::command::LiveFieldValue;
    use crate::modules::aec::engine::material::Material;
    use crate::modules::aec::engine::style::Style;
    use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, WallStyle};

    let material = Material::new(
        "brick_id".to_string(),
        "Brick Material".to_string(),
        "ANSI31".to_string(),
        0xFF0000,
        "Continuous".to_string(),
    );
    let style = WallStyle {
        style: Style {
            id: "style1".to_string(),
            name: "Standard Wall".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![Layer {
            material_id: "brick_id".to_string(),
            thickness: LayerValue::Fixed(0.25),
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
        materials: vec![material],
        wall_styles: vec![style],
        ..Default::default()
    };

    let mut cmd = WallCommand::new_with_library(Some(lib));
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
    cmd.set_live_handles(vec![Handle::new(201)]);

    cmd.apply_live_property(
        "wall_style",
        LiveFieldValue::Picker("style1".to_string()),
    );

    assert_eq!(cmd.style_id.as_deref(), Some("style1"));
    let layers = cmd.resolved_layers.clone().expect("style pick should resolve layers");
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].material, "Brick Material");
    assert!((layers[0].thickness - 0.25).abs() < 1e-9);
    assert_eq!(layers[0].function, "Structural");

    let live = cmd.live_properties().expect("still drawing");
    assert!(matches!(&live.fields[0].value, LiveFieldValue::Picker(s) if s == "Standard Wall"));
}

#[test]
fn wall_command_refuses_to_finish_without_a_style_when_styles_are_available() {
    use crate::modules::aec::engine::material::Material;
    use crate::modules::aec::engine::style::Style;
    use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, WallStyle};

    let material = Material::new(
        "brick_id".to_string(),
        "Brick Material".to_string(),
        "ANSI31".to_string(),
        0xFF0000,
        "Continuous".to_string(),
    );
    let style = WallStyle {
        style: Style {
            id: "style1".to_string(),
            name: "Standard Wall".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![Layer {
            material_id: "brick_id".to_string(),
            thickness: LayerValue::Fixed(0.25),
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
        materials: vec![material],
        wall_styles: vec![style],
        ..Default::default()
    };

    let mut cmd = WallCommand::new_with_library(Some(lib));
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(_) => {}
        _ => panic!("segment should commit even without a style"),
    }
    assert!(matches!(cmd.on_enter(), CmdResult::Cancel));
}

#[test]
fn wall_command_with_no_library_falls_back_to_v1_record() {
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => {
            let pl = match &entity {
                EntityType::LwPolyline(pl) => pl,
                _ => panic!("expected a wall polyline"),
            };
            let record = pl.common.extended_data.get_record(AEC_APPID).unwrap();
            assert_eq!(record.values[0], XDataValue::String("WALL".to_string()));
        }
        _ => panic!("Expected second point to commit wall with V1 record"),
    }
}

#[test]
fn material_command_collects_fields_and_dispatches_add_command() {
    let mut cmd = MaterialCommand::new();
    assert_eq!(cmd.prompt(), crate::tr!("aec", "material-prompt-name"));

    assert!(matches!(
        cmd.on_text_input("Sichtbeton"),
        Some(CmdResult::NeedPoint)
    ));
    assert_eq!(cmd.prompt(), crate::tr!("aec", "material-prompt-hatch"));

    assert!(matches!(
        cmd.on_text_input(""), // blank -> default hatch
        Some(CmdResult::NeedPoint)
    ));
    assert_eq!(cmd.prompt(), crate::tr!("aec", "material-prompt-color"));

    assert!(matches!(
        cmd.on_text_input("A0A0A0"),
        Some(CmdResult::NeedPoint)
    ));
    assert_eq!(cmd.prompt(), crate::tr!("aec", "material-prompt-linetype"));

    match cmd.on_text_input("") {
        Some(CmdResult::Dispatch(dispatch)) => {
            assert_eq!(
                dispatch,
                "AEC_MATERIAL_ADD Sichtbeton|ANSI31|A0A0A0|Continuous"
            );
        }
        _ => panic!("expected the final field to dispatch AEC_MATERIAL_ADD"),
    }
}

#[test]
fn material_command_requires_a_non_empty_name() {
    let mut cmd = MaterialCommand::new();
    assert!(matches!(cmd.on_text_input(""), Some(CmdResult::NeedPoint)));
    // Still on the name step.
    assert_eq!(cmd.prompt(), crate::tr!("aec", "material-prompt-name"));
}

#[test]
fn style_command_collects_two_layers_and_dispatches_add_command() {
    let mut cmd = StyleCommand::new();
    assert_eq!(cmd.prompt(), crate::tr!("aec", "style-prompt-name"));

    assert!(matches!(
        cmd.on_text_input("Testwand"),
        Some(CmdResult::NeedPoint)
    ));
    assert_eq!(cmd.prompt(), crate::tr!("aec", "style-prompt-parent"));

    assert!(matches!(
        cmd.on_text_input(""), // no parent
        Some(CmdResult::NeedPoint)
    ));
    assert_eq!(
        cmd.prompt(),
        crate::tr!("aec", "style-prompt-layer-material", n = 1i32)
    );

    // Layer 1
    assert!(matches!(
        cmd.on_text_input("Putz"),
        Some(CmdResult::NeedPoint)
    ));
    assert!(matches!(
        cmd.on_text_input("0.015"),
        Some(CmdResult::NeedPoint)
    ));
    assert!(matches!(
        cmd.on_text_input("Finish"),
        Some(CmdResult::NeedPoint)
    ));

    // Layer 2
    assert!(matches!(
        cmd.on_text_input("Mauerwerk"),
        Some(CmdResult::NeedPoint)
    ));
    assert!(matches!(
        cmd.on_text_input("0.24"),
        Some(CmdResult::NeedPoint)
    ));
    assert!(matches!(
        cmd.on_text_input("Structural"),
        Some(CmdResult::NeedPoint)
    ));

    // Blank material name ends the layer loop and dispatches.
    match cmd.on_text_input("") {
        Some(CmdResult::Dispatch(dispatch)) => {
            assert_eq!(
                dispatch,
                "AEC_STYLE_ADD Testwand||Putz:0.015:Finish;Mauerwerk:0.24:Structural"
            );
        }
        _ => panic!("expected the final blank layer entry to dispatch AEC_STYLE_ADD"),
    }
}

#[test]
fn style_command_with_no_layers_dispatches_empty_layer_list_for_inheritance() {
    let mut cmd = StyleCommand::new();
    cmd.on_text_input("Kind Wand");
    cmd.on_text_input("Standard Wall"); // parent

    match cmd.on_text_input("") {
        Some(CmdResult::Dispatch(dispatch)) => {
            assert_eq!(dispatch, "AEC_STYLE_ADD Kind Wand|Standard Wall|");
        }
        _ => panic!("expected an empty layer list to still dispatch AEC_STYLE_ADD"),
    }
}

#[test]
fn layer_function_round_trips_through_its_plain_string_form() {
    for f in [
        LayerFunction::Structural,
        LayerFunction::Insulation,
        LayerFunction::Finish,
    ] {
        let s = layer_function_to_str(&f);
        assert_eq!(parse_layer_function(&s), f);
    }
    assert_eq!(parse_layer_function(""), LayerFunction::Structural);
    assert_eq!(
        parse_layer_function("Custom"),
        LayerFunction::Other("Custom".to_string())
    );
}

#[test]
fn wall_justification_conversion() {
    // Horizontal wall along X axis from 0 to 10.
    // Thickness = 0.2.
    // Points picked at Interior (Y=+0.1 if drawing left to right).
    // Centerline should be at Y=0.

    let mut cmd = WallCommand::new_with_library(None);
    cmd.justification = WallJustification::Interior;
    cmd.thickness = 0.2;

    cmd.on_point(DVec3::new(0.0, 0.1, 0.0));
    let entity = match cmd.on_point(DVec3::new(10.0, 0.1, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("Should commit entity"),
    };
    let EntityType::LwPolyline(pl) = entity else {
        panic!("Expected LwPolyline")
    };

    assert_eq!(pl.vertices.len(), 2);
    // Interior justification for a segment (0, 0.1) -> (10, 0.1)
    // normal is (0, 1).
    // offset in build_entity is -0.1.
    // Centerline = Picked + (0, 1) * -0.1 = (0, 0).
    assert!((pl.vertices[0].location.x - 0.0).abs() < 1e-9);
    assert!((pl.vertices[0].location.y - 0.0).abs() < 1e-9);
    assert!((pl.vertices[1].location.x - 10.0).abs() < 1e-9);
    assert!((pl.vertices[1].location.y - 0.0).abs() < 1e-9);
}

#[test]
fn wall_justification_toggle_cycle() {
    let mut cmd = WallCommand::new();
    assert_eq!(cmd.justification, WallJustification::Center);

    cmd.set_ctrl(true);
    assert_eq!(cmd.justification, WallJustification::Exterior);
    cmd.set_ctrl(true); // Should not toggle again while held
    assert_eq!(cmd.justification, WallJustification::Exterior);

    cmd.set_ctrl(false);
    cmd.set_ctrl(true);
    assert_eq!(cmd.justification, WallJustification::Interior);

    cmd.set_ctrl(false);
    cmd.set_ctrl(true);
    assert_eq!(cmd.justification, WallJustification::Center);
}

#[test]
fn slugify_normalizes_names_into_stable_ids() {
    assert_eq!(slugify("Wand Stahlbeton 20cm"), "wand_stahlbeton_20cm");
    assert_eq!(slugify("  spaced  out  "), "spaced_out");
    assert_eq!(slugify(""), "item");
}

#[test]
fn unique_id_stays_readable_but_differs_across_calls() {
    // Two ids generated for the exact same name (e.g. the same wall
    // style name entered independently in two different projects) must
    // still differ, so libraries created independently never collide
    // when later mixed (see `unique_id` doc comment).
    let a = unique_id("style", "Mauerwerk 36.5");
    let b = unique_id("style", "Mauerwerk 36.5");
    assert_ne!(a, b);
    assert!(a.starts_with("style_mauerwerk_36_5_"));
    assert!(b.starts_with("style_mauerwerk_36_5_"));
}

#[test]
fn wall_round_trip_with_derived_handles() {
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let derived = vec![Handle::new(10), Handle::new(11), Handle::new(12)];
    let values = wall_record("style1", 3.0, 0, &layers, &derived, WallJustification::Center, PlanPhase::New, None);
    let pl = LwPolyline::new();
    let mut entity = EntityType::LwPolyline(pl);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = values;
    entity.common_mut().extended_data.add_record(record);

    let wall = wall_from_entity(&entity).expect("Should parse WALL");
    assert_eq!(wall.derived_handles, derived);
}

#[test]
fn wall_without_derived_handles_tail_still_parses() {
    // Simulate an old record written before `derived_handles` existed:
    // build it with an empty list and confirm it reads back empty, not
    // an error, keeping legacy records readable.
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    let pl = LwPolyline::new();
    let mut entity = EntityType::LwPolyline(pl);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = values;
    entity.common_mut().extended_data.add_record(record);

    let wall = wall_from_entity(&entity).expect("Should parse WALL");
    assert!(wall.derived_handles.is_empty());
}

/// Build a two-layer `WALL` axis polyline in `scene` and return its
/// handle.
pub(super) fn add_multi_layer_wall(scene: &mut Scene) -> Handle {
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut entity = EntityType::LwPolyline(pl);
    let layers = wls(&[
        ("Concrete", 0.2, "Structural"),
        ("Insulation", 0.05, "Insulation"),
    ]);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    scene.add_entity(entity)
}
