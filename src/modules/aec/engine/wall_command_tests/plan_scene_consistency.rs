use super::*;
use crate::app::OpenCADStudio;
use crate::modules::aec::engine::display_component::RepresentationMode;
use crate::modules::aec::engine::library::DisplayConfigLibrary;
use crate::modules::aec::engine::plan_view::{
    DetailProfile, DisplayConfig, LayerDetail, PlanningStage, ViewType,
};
use crate::modules::aec::engine::room::{Room, RoomFinish};
use crate::modules::aec::engine::room_package::collect_room_display_children;
use crate::modules::aec::engine::room_regen::regenerate_room_representation;
use crate::modules::aec::engine::room_xdata::write_room_record;
use crate::modules::aec::engine::slab::{Slab, SlabLayer};
use crate::modules::aec::engine::slab_package::slab_package_handles;
use crate::modules::aec::engine::wall_style::LayerFunction;

fn plan(name: &str, detail: DetailProfile) -> DisplayConfig {
    let mut c = DisplayConfig::new(name.into(), name.into(), PlanningStage::Design, ViewType::FloorPlan);
    c.detail = detail;
    c
}

struct Fixture {
    app: OpenCADStudio,
    tab: usize,
    wall: Handle,
    opening: Handle,
    door: Handle,
    slab: Handle,
    room: Handle,
}

/// Three layers, so the middle one is not part of the envelope contour.
fn add_three_layer_wall(scene: &mut Scene, p0: (f64, f64), p1: (f64, f64)) -> Handle {
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(p0.0, p0.1)));
    pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
    let mut entity = EntityType::LwPolyline(pl);
    let layers = wls(&[
        ("Concrete", 0.2, "Structural"),
        ("Insulation", 0.05, "Insulation"),
        ("Plaster", 0.02, "Finish"),
    ]);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values =
        wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    scene.add_entity(entity)
}

fn fixture() -> Fixture {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let tab = app.active_tab;
    assert!(!app.tabs[tab].is_start);
    let wall = add_three_layer_wall(&mut app.tabs[tab].scene, (0.0, 0.0), (5.0, 0.0));
    let (opening, _) = place_wall_opening(
        &mut app.tabs[tab].scene,
        wall,
        DVec3::new(2.0, 0.0, 0.0),
        engine::openings::OpeningKind::Window,
        None,
        None,
        None,
    )
    .expect("place window");

    // Door wall on the south edge of the room (10..14, 0..4).
    let door_wall = add_three_layer_wall(&mut app.tabs[tab].scene, (10.0, 0.0), (14.0, 0.0));
    let (door, _) = place_wall_opening(
        &mut app.tabs[tab].scene,
        door_wall,
        DVec3::new(12.0, 0.0, 0.0),
        engine::openings::OpeningKind::Door,
        None,
        None,
        None,
    )
    .expect("place door");

    let scene = &mut app.tabs[tab].scene;
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64| {
        let mut pl = LwPolyline::new();
        for (x, y) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
            pl.add_vertex(LwVertex::new(Vector2::new(x, y)));
        }
        pl.is_closed = true;
        EntityType::LwPolyline(pl)
    };
    let slab = scene.add_entity(rect(0.0, 5.0, 5.0, 9.0));
    let mut s = Slab::new("style_slab_concrete_20", 1);
    s.layers = vec![SlabLayer::new("Concrete", 0.20, LayerFunction::Structural)];
    assert!(crate::modules::aec::engine::slab_xdata::write_slab_record(&mut scene.document, slab, &s));
    let room = scene.add_entity(rect(10.0, 0.0, 14.0, 4.0));
    let pts = [(10.0, 0.0), (14.0, 0.0), (14.0, 4.0), (10.0, 4.0)];
    let mut r = Room::from_polygon("Wohnen", &pts, 2.5, 0).with_number("EG-01");
    r.floor_finish = Some(vec![RoomFinish::new("Parkett", 0.015).with_hatch("ANSI31")]);
    r.ceiling_finish = Some(vec![RoomFinish::new("Putz", 0.01).with_hatch("ANSI31")]);
    write_room_record(&mut scene.document, room, &r);
    assert!(regenerate_room_representation(scene, room, None).is_some());
    Fixture { app, tab, wall, opening, door, slab, room }
}

fn put_plans(f: &mut Fixture, mut plans: Vec<DisplayConfig>) {
    for plan in &mut plans {
        plan.apply_detail_defaults_if_unset();
    }
    let mut lib = DisplayConfigLibrary::empty();
    lib.configs = plans;
    f.app.aec.aec_plan_library = Some(lib);
}

fn activate(f: &mut Fixture, name: &str) {
    f.app.aec_set_active_display_config(f.tab, Some(name.into()));
    f.app.apply_active_display_config_to_tab(f.tab);
}

use crate::modules::aec::engine::display_component::{ComponentRuleSet, StyleDisplayOverlay};
use crate::modules::aec::engine::room_package::{
    AEC_ROOM_CEILING_HATCH_LAYER, AEC_ROOM_HATCH_LAYER, AEC_ROOM_STAMP_LAYER,
    AEC_ROOM_THRESHOLD_LAYER,
};
use crate::modules::aec::engine::slab_package::{
    AEC_SLAB_CEILING_LAYER, AEC_SLAB_CONTOUR_LAYER, AEC_SLAB_HATCH_LAYER,
};
use crate::modules::aec::engine::wall_style::WallStyle;
use std::collections::BTreeMap;

fn entity_kind(f: &Fixture, h: Handle) -> String {
    let e = f.app.tabs[f.tab].scene.document.get_entity(h).expect("entity");
    format!("{e:?}").split('(').next().unwrap_or("").to_string()
}

/// Wall children counted by entity type (`Hatch`, `LwPolyline`, ...).
fn wall_kinds(f: &Fixture) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for h in wall_package_handles(&f.app.tabs[f.tab].scene, f.wall) {
        if h != f.wall {
            *m.entry(entity_kind(f, h)).or_insert(0) += 1;
        }
    }
    m
}

fn kind(m: &BTreeMap<String, usize>, k: &str) -> usize {
    m.get(k).copied().unwrap_or(0)
}

/// Opening display children counted by slot name.
fn opening_slots(f: &Fixture, opening: Handle) -> BTreeMap<String, usize> {
    let scene = &f.app.tabs[f.tab].scene;
    let mut m = BTreeMap::new();
    for h in engine::opening_display::collect_opening_display_children(scene, opening) {
        let slot = scene
            .document
            .get_entity(h)
            .and_then(engine::opening_display::opening_rep_slot_from_entity)
            .map(|s| s.key().to_string())
            .unwrap_or_else(|| "?".into());
        *m.entry(slot).or_insert(0) += 1;
    }
    m
}

fn layer_counts(f: &Fixture, handles: Vec<Handle>) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for h in handles {
        if let Some(e) = f.app.tabs[f.tab].scene.document.get_entity(h) {
            *m.entry(e.common().layer.clone()).or_insert(0) += 1;
        }
    }
    m
}

fn slab_layers(f: &Fixture) -> BTreeMap<String, usize> {
    layer_counts(f, slab_package_handles(&f.app.tabs[f.tab].scene, f.slab))
}

fn room_layers(f: &Fixture) -> BTreeMap<String, usize> {
    layer_counts(f, collect_room_display_children(&f.app.tabs[f.tab].scene, f.room))
}

fn n(m: &BTreeMap<String, usize>, k: &str) -> usize {
    m.get(k).copied().unwrap_or(0)
}

const SYMBOL_SLOTS: [&str; 2] = ["Swing2D", "BreakthroughSymbol2D"];

fn symbols(m: &BTreeMap<String, usize>) -> usize {
    SYMBOL_SLOTS.iter().map(|k| n(m, k)).sum()
}

fn vis(entries: &[(&str, bool)]) -> ComponentRuleSet {
    let mut r = ComponentRuleSet::default();
    for (k, v) in entries {
        r.visibility.insert((*k).to_string(), *v);
    }
    r
}

/// Project style library with the fixture's wall style `style1` and slab
/// style carrying the given default profiles.
fn set_style_defaults(f: &mut Fixture, wall: Option<ComponentRuleSet>, slab: Option<ComponentRuleSet>) {
    let mut lib = crate::modules::aec::engine::library::load_or_seed();
    let mut ws = WallStyle {
        style: Style {
            id: "style1".into(),
            name: "Style 1".into(),
            category: "Wall".into(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![],
        default_profile: None,
    };
    ws.default_profile = wall;
    lib.upsert_wall_style(ws);
    let mut ss = lib.find_slab_style("style_slab_concrete_20").cloned().expect("seed slab style");
    ss.default_profile = slab;
    lib.upsert_slab_style(ss);
    let mut project = ProjectFile::default();
    project.material_wall_style_library = lib;
    f.app.aec.aec_project_explorer_file = Some(project);
}

fn overlay(wall_or_slab: ComponentRuleSet) -> StyleDisplayOverlay {
    StyleDisplayOverlay { rules: Some(wall_or_slab), ..Default::default() }
}

fn reapply(f: &mut Fixture) {
    f.app.apply_active_display_config_to_tab(f.tab);
}

fn rough_detail() -> DetailProfile {
    DetailProfile {
        layers: LayerDetail::Off,
        hatches: false,
        annotation: false,
        ..DetailProfile::default()
    }
}

#[test]
fn user_plan_switch_changes_wall_opening_slab_and_room_by_role() {
    let mut f = fixture();
    put_plans(&mut f, vec![plan("Voll", DetailProfile::default()), plan("Grob", rough_detail())]);
    if let Some(config) = f.app.aec.aec_plan_library.as_mut().and_then(|lib| {
        lib.configs.iter_mut().find(|config| config.name == "Grob")
    }) {
        config.opening_levels.swing = Some(crate::modules::aec::engine::opening_style::SwingGlyph::Off);
    }

    activate(&mut f, "Voll");
    let (w_full, win_full, door_full) =
        (wall_kinds(&f), opening_slots(&f, f.opening), opening_slots(&f, f.door));
    let (s_full, r_full) = (slab_layers(&f), room_layers(&f));
    assert!(kind(&w_full, "Hatch") > 0, "wall hatches present: {w_full:?}");
    assert!(symbols(&door_full) > 0, "door symbols present: {door_full:?}");
    assert_eq!(n(&s_full, AEC_SLAB_HATCH_LAYER), 1);
    assert_eq!(n(&s_full, AEC_SLAB_CONTOUR_LAYER), 1);
    assert_eq!(n(&r_full, AEC_ROOM_STAMP_LAYER), 1);
    assert_eq!(n(&r_full, AEC_ROOM_THRESHOLD_LAYER), 1, "door threshold: {r_full:?}");
    assert!(n(&r_full, AEC_ROOM_HATCH_LAYER) >= 1 && n(&r_full, AEC_ROOM_CEILING_HATCH_LAYER) >= 1);

    activate(&mut f, "Grob");
    let w = wall_kinds(&f);
    assert_eq!(kind(&w, "Hatch"), 0, "wall hatches hidden: {w:?}");
    assert!(kind(&w, "LwPolyline") < kind(&w_full, "LwPolyline"), "wall layers hidden");
    assert!(kind(&w, "LwPolyline") >= 1, "wall contour stays");
    for opening in [f.opening, f.door] {
        let slots = opening_slots(&f, opening);
        assert_eq!(symbols(&slots), 0, "opening symbols hidden: {slots:?}");
        assert_eq!(n(&slots, "OpeningLabel2D"), 0, "opening label hidden: {slots:?}");
        assert_eq!(n(&slots, "Frame2D"), 2, "frame stays: {slots:?}");
    }
    assert_eq!(n(&door_full, "OpeningLabel2D"), 2);
    let s = slab_layers(&f);
    assert_eq!(n(&s, AEC_SLAB_HATCH_LAYER), 0, "slab hatch hidden");
    assert_eq!(n(&s, AEC_SLAB_CONTOUR_LAYER), 1, "slab contour stays");
    assert!(room_layers(&f).is_empty(), "room stamp, hatches, thresholds hidden");

    activate(&mut f, "Voll");
    assert_eq!(wall_kinds(&f), w_full);
    assert_eq!(opening_slots(&f, f.opening), win_full);
    assert_eq!(opening_slots(&f, f.door), door_full);
    assert_eq!(slab_layers(&f), s_full);
    assert_eq!(room_layers(&f), r_full);
}

#[test]
fn opening_symbols_off_hides_thresholds_only() {
    let mut f = fixture();
    put_plans(&mut f, vec![plan("OhneSymbole", DetailProfile::default())]);
    if let Some(config) = f.app.aec.aec_plan_library.as_mut().and_then(|lib| {
        lib.configs.iter_mut().find(|config| config.name == "OhneSymbole")
    }) {
        config.opening_levels.swing = Some(crate::modules::aec::engine::opening_style::SwingGlyph::Off);
    }
    activate(&mut f, "OhneSymbole");
    let r = room_layers(&f);
    assert_eq!(n(&r, AEC_ROOM_THRESHOLD_LAYER), 0);
    assert_eq!(n(&r, AEC_ROOM_STAMP_LAYER), 1);
    assert!(n(&r, AEC_ROOM_HATCH_LAYER) >= 1);
    assert_eq!(symbols(&opening_slots(&f, f.door)), 0);
}

#[test]
fn ceiling_view_changes_slab_and_room_and_floor_plan_policy_is_pinned() {
    let mut f = fixture();
    let d = DetailProfile { ceiling_view: true, ..DetailProfile::default() };
    put_plans(&mut f, vec![plan("Decke", d), plan("Grundriss", DetailProfile::default())]);

    // Floor plan policy (unchanged): floor AND ceiling hatch are drawn.
    activate(&mut f, "Grundriss");
    let (s_floor, r_floor) = (slab_layers(&f), room_layers(&f));
    assert_eq!(n(&s_floor, AEC_SLAB_CONTOUR_LAYER), 1);
    assert_eq!(n(&s_floor, AEC_SLAB_CEILING_LAYER), 0);
    assert!(n(&r_floor, AEC_ROOM_HATCH_LAYER) >= 1);
    assert!(n(&r_floor, AEC_ROOM_CEILING_HATCH_LAYER) >= 1);
    assert_eq!(n(&r_floor, AEC_ROOM_THRESHOLD_LAYER), 1);

    // Reflected ceiling plan.
    activate(&mut f, "Decke");
    let (s, r) = (slab_layers(&f), room_layers(&f));
    assert_eq!(n(&s, AEC_SLAB_CONTOUR_LAYER), 0, "floor contour hidden: {s:?}");
    assert_eq!(n(&s, AEC_SLAB_CEILING_LAYER), 1, "ceiling outline shown: {s:?}");
    assert_eq!(n(&s, AEC_SLAB_HATCH_LAYER), 0, "slab layer hatch hidden: {s:?}");
    assert_eq!(n(&r, AEC_ROOM_THRESHOLD_LAYER), 0);
    assert_eq!(n(&r, AEC_ROOM_HATCH_LAYER), 0, "floor hatch hidden");
    assert!(n(&r, AEC_ROOM_CEILING_HATCH_LAYER) >= 1);
    assert_eq!(n(&r, AEC_ROOM_STAMP_LAYER), 1);

    activate(&mut f, "Grundriss");
    assert_eq!((slab_layers(&f), room_layers(&f)), (s_floor, r_floor));
}

#[test]
fn wall_precedence_session_over_plan_override_over_style_default() {
    let mut f = fixture();
    put_plans(&mut f, vec![plan("P", DetailProfile::default())]);
    let layers_key = "Layers2D";
    let polylines = |f: &Fixture| kind(&wall_kinds(f), "LwPolyline");

    // Baseline: style without default profile shows layers.
    set_style_defaults(&mut f, None, None);
    activate(&mut f, "P");
    let full = polylines(&f);

    // Style default hides the layer build-up.
    set_style_defaults(&mut f, Some(vis(&[(layers_key, false)])), None);
    reapply(&mut f);
    let hidden = polylines(&f);
    assert!(hidden < full, "style default hides layers ({hidden} < {full})");

    // Plan override with an explicit `true` beats the hidden style default.
    let mut p = plan("P", DetailProfile::default());
    p.style_overrides.insert("style1".into(), overlay(vis(&[(layers_key, true)])));
    put_plans(&mut f, vec![p]);
    reapply(&mut f);
    assert_eq!(polylines(&f), full, "plan override true beats style default");

    // Plan override hiding beats a style default that shows.
    set_style_defaults(&mut f, None, None);
    let mut p = plan("P", DetailProfile::default());
    p.style_overrides.insert("style1".into(), overlay(vis(&[(layers_key, false)])));
    put_plans(&mut f, vec![p]);
    reapply(&mut f);
    assert_eq!(polylines(&f), hidden, "plan override false beats visible default");

    // Session 2D beats the hiding plan override ...
    f.app.aec_set_representation_override(f.tab, Some(RepresentationMode::TwoD));
    reapply(&mut f);
    assert_eq!(polylines(&f), full, "session 2D beats plan override");
    // ... and session 3D hides the 2D layers although the override shows them.
    let mut p = plan("P", DetailProfile::default());
    p.style_overrides.insert("style1".into(), overlay(vis(&[(layers_key, true)])));
    put_plans(&mut f, vec![p]);
    f.app.aec_set_representation_override(f.tab, Some(RepresentationMode::ThreeD));
    reapply(&mut f);
    assert!(polylines(&f) < full, "session 3D beats plan override");
    // Dropping the session override restores the plan override.
    f.app.aec_set_representation_override(f.tab, None);
    reapply(&mut f);
    assert_eq!(polylines(&f), full);
}

#[test]
fn slab_precedence_session_over_plan_override_over_style_default() {
    let mut f = fixture();
    put_plans(&mut f, vec![plan("P", DetailProfile::default())]);
    let hatch = |f: &Fixture| n(&slab_layers(f), AEC_SLAB_HATCH_LAYER);
    let key = "LayerHatch2D";

    set_style_defaults(&mut f, None, None);
    activate(&mut f, "P");
    assert_eq!(hatch(&f), 1);

    set_style_defaults(&mut f, None, Some(vis(&[(key, false)])));
    reapply(&mut f);
    assert_eq!(hatch(&f), 0, "style default hides slab hatch");

    let mut p = plan("P", DetailProfile::default());
    p.style_overrides
        .insert("style_slab_concrete_20".into(), overlay(vis(&[(key, true)])));
    put_plans(&mut f, vec![p]);
    reapply(&mut f);
    assert_eq!(hatch(&f), 1, "plan override true beats hidden style default");

    set_style_defaults(&mut f, None, None);
    let mut p = plan("P", DetailProfile::default());
    p.style_overrides
        .insert("style_slab_concrete_20".into(), overlay(vis(&[(key, false)])));
    put_plans(&mut f, vec![p]);
    reapply(&mut f);
    assert_eq!(hatch(&f), 0, "plan override false beats visible style default");

    f.app.aec_set_representation_override(f.tab, Some(RepresentationMode::TwoD));
    reapply(&mut f);
    assert_eq!(hatch(&f), 1, "session 2D beats plan override");
    f.app.aec_set_representation_override(f.tab, Some(RepresentationMode::ThreeD));
    reapply(&mut f);
    assert_eq!(hatch(&f), 0, "session 3D hides 2D hatch");
}

#[test]
fn session_representation_beats_plan_detail_for_rooms() {
    let mut f = fixture();
    let d = DetailProfile { representation: RepresentationMode::TwoD, ..DetailProfile::default() };
    put_plans(&mut f, vec![plan("Zweid", d)]);
    activate(&mut f, "Zweid");
    assert_eq!(n(&room_layers(&f), AEC_ROOM_STAMP_LAYER), 1);
    f.app.aec_set_representation_override(f.tab, Some(RepresentationMode::ThreeD));
    reapply(&mut f);
    assert!(room_layers(&f).is_empty());
    f.app.aec_set_representation_override(f.tab, Some(RepresentationMode::TwoD));
    reapply(&mut f);
    assert_eq!(n(&room_layers(&f), AEC_ROOM_STAMP_LAYER), 1);
}

#[test]
fn missing_or_unknown_active_plan_is_graceful() {
    let mut f = fixture();
    put_plans(&mut f, vec![plan("Voll", DetailProfile::default())]);
    let snap = |f: &Fixture| (room_layers(f), wall_kinds(f), slab_layers(f), opening_slots(f, f.door));
    let before = snap(&f);
    reapply(&mut f);
    f.app.aec_set_active_display_config(f.tab, Some("Gibt es nicht".into()));
    reapply(&mut f);
    assert_eq!(before, snap(&f));
}

#[test]
fn session_override_without_or_with_unknown_plan_is_consistent_in_both_paths() {
    for unknown in [false, true] {
        let mut f = fixture();
        put_plans(&mut f, vec![plan("Voll", DetailProfile::default())]);
        f.app.aec_set_active_display_config(f.tab, unknown.then(|| "Gibt es nicht".to_string()));
        f.app.aec_set_representation_override(f.tab, Some(RepresentationMode::ThreeD));
        reapply(&mut f);
        assert!(room_layers(&f).is_empty(), "full apply hides rooms (unknown={unknown})");
        let (tab, room) = (f.tab, f.room);
        f.app.aec_regenerate_entity_packages(tab, &[room]);
        assert!(room_layers(&f).is_empty(), "entity regen keeps rooms hidden (unknown={unknown})");
        // 2D session override brings them back in both paths.
        f.app.aec_set_representation_override(f.tab, Some(RepresentationMode::TwoD));
        f.app.aec_regenerate_entity_packages(tab, &[room]);
        assert_eq!(n(&room_layers(&f), AEC_ROOM_STAMP_LAYER), 1);
    }
}

fn rough_room_fixture() -> Fixture {
    let mut f = fixture();
    put_plans(&mut f, vec![plan("Grob", rough_detail())]);
    activate(&mut f, "Grob");
    assert!(room_layers(&f).is_empty());
    f
}

#[test]
fn selection_regeneration_keeps_active_plan_for_rooms() {
    let mut f = rough_room_fixture();
    let (tab, room) = (f.tab, f.room);
    f.app.aec_regenerate_entity_packages(tab, &[room]);
    assert!(room_layers(&f).is_empty());
}

#[test]
fn room_property_edit_keeps_active_plan() {
    let mut f = rough_room_fixture();
    let (tab, room) = (f.tab, f.room);
    assert!(f.app.aec_apply_property_field(tab, room, "room_name", "Umbenannt"));
    let e = f.app.tabs[tab].scene.document.get_entity(room).unwrap();
    assert_eq!(crate::modules::aec::engine::room_xdata::room_from_entity(e).unwrap().name, "Umbenannt");
    assert!(room_layers(&f).is_empty(), "stamp/hatches stay hidden: {:?}", room_layers(&f));
}

#[test]
fn room_creation_keeps_active_plan() {
    use crate::modules::aec::rooms::room::RoomCommand;
    let mut f = rough_room_fixture();
    let tab = f.tab;
    let display = f.app.resolve_active_display_config_room_display(tab);
    let mut cmd = RoomCommand::new().with_display(display);
    let pts = [(20.0, 0.0), (24.0, 0.0), (24.0, 4.0), (20.0, 4.0)];
    let h = cmd.commit_polygon(&mut f.app.tabs[tab].scene, &pts).expect("room");
    assert!(collect_room_display_children(&f.app.tabs[tab].scene, h).is_empty());
    // Without plan awareness the stamp is created.
    let mut plain = RoomCommand::new();
    let h2 = plain.commit_polygon(&mut f.app.tabs[tab].scene, &pts).expect("room");
    assert!(!collect_room_display_children(&f.app.tabs[tab].scene, h2).is_empty());
}

#[test]
fn room_stamp_move_keeps_active_plan() {
    use crate::modules::aec::rooms::room_grip::move_room_stamp_with_display;
    let mut f = rough_room_fixture();
    let (tab, room) = (f.tab, f.room);
    let display = f.app.resolve_active_display_config_room_display(tab);
    assert!(move_room_stamp_with_display(&mut f.app.tabs[tab].scene, room, (11.0, 1.0), display));
    assert!(room_layers(&f).is_empty());
    // Interactive grip path.
    let grip = crate::scene::model::object::GripApply::Absolute(DVec3::new(12.0, 2.0, 0.0));
    assert!(f.app.apply_aec_room_grip(
        tab,
        room,
        crate::modules::aec::properties::ROOM_STAMP_GRIP_ID,
        &grip
    ));
    assert!(room_layers(&f).is_empty());
}

#[test]
fn floor_finish_save_and_apply_keeps_active_plan() {
    use crate::modules::aec::message::AecMessage;
    let mut f = rough_room_fixture();
    let tab = f.tab;
    let _ = f.app.update_aec(AecMessage::AecFloorFinishStyleManagerNew);
    let _ = f.app.update_aec(AecMessage::AecFloorFinishStyleManagerNameChanged("Belag Plan".into()));
    let _ = f.app.update_aec(AecMessage::AecFloorFinishStyleManagerSave);
    let id = f
        .app
        .aec
        .aec_style_library
        .as_ref()
        .and_then(|l| l.floor_finish_styles.iter().find(|s| s.style.name == "Belag Plan"))
        .map(|s| s.style.id.clone())
        .expect("finish saved");
    let scene = &mut f.app.tabs[tab].scene;
    let e = scene.document.get_entity(f.room).unwrap();
    let mut room = crate::modules::aec::engine::room_xdata::room_from_entity(e).unwrap();
    room.finish_style_id = Some(id);
    write_room_record(&mut scene.document, f.room, &room);
    let _ = f.app.update_aec(AecMessage::AecFloorFinishStyleManagerSaveAndApply);
    assert!(room_layers(&f).is_empty(), "plan hiding kept: {:?}", room_layers(&f));
}
