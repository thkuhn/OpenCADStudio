use super::*;
use crate::modules::aec::engine::material::Material;
use crate::modules::aec::engine::style::Style;
use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, LayerValue, WallStyle};

fn wall_with_default(default_profile: Option<ComponentRuleSet>) -> WallStyle {
    WallStyle {
        style: Style {
            id: "style1".to_string(),
            name: "Style 1".to_string(),
            category: crate::modules::aec::engine::style_common::CATEGORY_WALL.to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![],
        default_profile,
    }
}

fn named_cfg(name: &str) -> DisplayConfig {
    DisplayConfig::new(name.to_string(), "A".to_string(), crate::modules::aec::engine::plan_view::PlanningStage::Execution, crate::modules::aec::engine::plan_view::ViewType::FloorPlan)
}

#[test]
fn resolve_effective_rule_set_is_name_independent() {
    let mut default = ComponentRuleSet::default();
    default.visibility.insert("AxisLine".to_string(), false);
    let style = wall_with_default(Some(default.clone()));
    // Any plan kind name yields the style default; renaming a plan loses nothing.
    for name in ["Architekt 1:50", "Meine Planart", ""] {
        let rules = resolve_effective_rule_set(
            style.default_profile.as_ref(),
            &named_cfg(name),
            Some(&style.style.id),
        );
        assert_eq!(rules, default);
    }
    let none = resolve_effective_rule_set(None, &named_cfg("X"), Some("style1"));
    assert_eq!(none, ComponentRuleSet::default());
}

#[test]
fn plan_override_beats_style_default_in_resolution() {
    let mut default = ComponentRuleSet::default();
    default.visibility.insert("AxisLine".to_string(), false);
    default.visibility.insert("Contour2D".to_string(), false);
    let mut over = ComponentRuleSet::default();
    over.visibility.insert("AxisLine".to_string(), true);
    let mut cfg = named_cfg("P");
    cfg.style_overrides.insert(
        "style1".to_string(),
        StyleDisplayOverlay { rules: Some(over), ..Default::default() },
    );
    let rules = resolve_effective_rule_set(Some(&default), &cfg, Some("style1"));
    assert_eq!(rules.visibility.get("AxisLine"), Some(&true));
    assert_eq!(rules.visibility.get("Contour2D"), Some(&false));
    // Override of another style is ignored.
    let other = resolve_effective_rule_set(Some(&default), &cfg, Some("other"));
    assert_eq!(other.visibility.get("AxisLine"), Some(&false));
}

#[test]
fn wall_precedence_plan_override_beats_style_default_session_beats_both() {
    let mut default = ComponentRuleSet::default();
    default.visibility.insert("AxisLine".to_string(), false);
    let style = wall_with_default(Some(default));
    let mut cfg = named_cfg("P");

    let base = build_effective_rule_set(&cfg, Some(&style), None);
    assert_eq!(base.visibility.get("AxisLine"), Some(&false)); // style default

    let mut over = ComponentRuleSet::default();
    over.visibility.insert("AxisLine".to_string(), true);
    cfg.style_overrides.insert(
        style.style.id.clone(),
        StyleDisplayOverlay { rules: Some(over), ..Default::default() },
    );
    let plan = build_effective_rule_set(&cfg, Some(&style), None);
    assert_eq!(plan.visibility.get("AxisLine"), Some(&true)); // plan override > default

    let session = build_effective_rule_set(&cfg, Some(&style), Some(RepresentationMode::ThreeD));
    assert_eq!(session.visibility.get("AxisLine"), Some(&false)); // session representation wins
}

#[test]
fn slab_precedence_plan_override_beats_style_default_session_beats_both() {
    let mut default = ComponentRuleSet::default();
    default.visibility.insert("LayerHatch2D".to_string(), false);
    let mut style = SlabStyle::new("s1", "S1");
    style.default_profile = Some(default);
    let mut cfg = named_cfg("P");

    let base = build_effective_slab_rule_set(&cfg, Some(&style), None);
    assert_eq!(base.visibility.get("LayerHatch2D"), Some(&false)); // style default

    let mut over = ComponentRuleSet::default();
    over.visibility.insert("LayerHatch2D".to_string(), true);
    cfg.style_overrides.insert(
        "s1".to_string(),
        StyleDisplayOverlay { rules: Some(over), ..Default::default() },
    );
    let plan = build_effective_slab_rule_set(&cfg, Some(&style), None);
    assert_eq!(plan.visibility.get("LayerHatch2D"), Some(&true)); // plan override > default

    // Session ThreeD hides every 2D slot regardless of default / override.
    let session = build_effective_slab_rule_set(&cfg, Some(&style), Some(RepresentationMode::ThreeD));
    assert_eq!(session.visibility.get("LayerHatch2D"), Some(&false));

    // The plan's detail switches beat a style default that would show the slot.
    let mut shown = SlabStyle::new("s2", "S2");
    let mut vis = ComponentRuleSet::default();
    vis.visibility.insert("Contour2D".to_string(), true);
    shown.default_profile = Some(vis);
    let mut rcp = named_cfg("Q");
    rcp.detail.ceiling_view = true;
    let r = build_effective_slab_rule_set(&rcp, Some(&shown), None);
    assert_eq!(r.visibility.get("Contour2D"), Some(&false));
}

#[test]
fn opening_profiles_travel_with_rules_and_override_default() {
    use crate::modules::aec::engine::opening_style::OpeningDisplayProfile;
    let mut profile = OpeningDisplayProfile::default();
    profile.visibility.insert("Sill2D".to_string(), true);
    let mut cfg = named_cfg("P");
    cfg.style_overrides.insert(
        "ostyle".to_string(),
        StyleDisplayOverlay { opening: Some(profile.clone()), ..Default::default() },
    );
    let wall = build_effective_rule_set(&cfg, None, None);
    assert_eq!(wall.opening_profiles.get("ostyle"), Some(&profile));
    let slab = build_effective_slab_rule_set(&cfg, None, None);
    assert_eq!(slab.opening_profiles.get("ostyle"), Some(&profile));
    // Renaming the plan changes nothing.
    cfg.name = "Anders".to_string();
    let renamed = build_effective_rule_set(&cfg, None, None);
    assert_eq!(renamed.opening_profiles, wall.opening_profiles);
}

#[test]
fn stamp_origin_and_combined_entries_carry_provenance() {
    let mut lib = StyleLibrary::empty();
    lib.upsert_wall_style(wall_with_default(None));
    lib.upsert_material(Material::new("m".into(), "M".into(), "ANSI31".into(), 0, "Continuous".into()));
    lib.stamp_origin(StyleOrigin::Project);
    assert_eq!(lib.wall_styles[0].common().origin, StyleOrigin::Project);
    assert_eq!(lib.materials[0].style.origin, StyleOrigin::Project);
    // Content equality ignores origin: no false copy conflict.
    let mut other = StyleLibrary::empty();
    other.upsert_wall_style(wall_with_default(None));
    assert_eq!(
        wall_style_copy_conflict(&other, &lib.wall_styles[0]),
        CopyConflict::IdenticalAlreadyPresent
    );
    assert_eq!(
        material_copy_conflict(&other, &lib.materials[0]),
        CopyConflict::None
    );
    other.upsert_material(lib.materials[0].clone());
    other.stamp_origin(StyleOrigin::Default);
    assert_eq!(
        material_copy_conflict(&other, &lib.materials[0]),
        CopyConflict::IdenticalAlreadyPresent
    );

    // Project / session / standard provenance through the combined view.
    let mut project = crate::modules::aec::engine::project::ProjectFile::default();
    let mut pws = wall_with_default(None);
    pws.style.id = "proj_wall".into();
    project.material_wall_style_library.upsert_wall_style(pws);
    let mut session = StyleLibrary::empty();
    let mut sws = wall_with_default(None);
    sws.style.id = "sess_wall".into();
    session.upsert_wall_style(sws);
    let entries = combined_wall_style_entries_with_session(Some(&project), Some(&session));
    let origin_of = |id: &str| {
        entries
            .iter()
            .find(|e| e.wall_style.style.id == id)
            .map(|e| e.wall_style.style.origin)
    };
    assert_eq!(origin_of("proj_wall"), Some(StyleOrigin::Project));
    assert_eq!(origin_of("sess_wall"), Some(StyleOrigin::Session));
    assert!(entries
        .iter()
        .filter(|e| e.source == LibrarySource::Standard)
        .all(|e| e.wall_style.style.origin == StyleOrigin::Default));
}

#[test]
fn test_library_roundtrip() {
    let material = Material::new(
        "mat1".to_string(),
        "Material 1".to_string(),
        "HATCH1".to_string(),
        0xFF0000,
        "Continuous".to_string(),
    );

    let wall_style = WallStyle {
        style: Style {
            id: "style1".to_string(),
            name: "Style 1".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![
            Layer {
                material_id: "mat1".to_string(),
                thickness: LayerValue::Fixed(10.0),
                function: LayerFunction::Structural,
                axis_offset: LayerValue::Fixed(-7.5),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: Some("Tragschale".to_string()),
                layer_id: Uuid::new_v4(),
            },
            Layer {
                material_id: "mat1".to_string(),
                thickness: LayerValue::Fixed(5.0),
                function: LayerFunction::Finish,
                axis_offset: LayerValue::Fixed(2.5),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: Some("ANSI31".to_string()),
                role_tag: None,
                layer_id: Uuid::new_v4(),
            },
        ],
    default_profile: None,
    };

    let lib = StyleLibrary {
        materials: vec![material],
        wall_styles: vec![wall_style],
        ..Default::default()
    };

    let serialized = to_toml(&lib).expect("Serialization failed");
    let deserialized = from_toml(&serialized).expect("Deserialization failed");

    assert_eq!(lib, deserialized);
}

#[test]
fn seeded_section_profiles_are_not_assigned_to_opening_styles() {
    let library = seed_default_library();
    assert_eq!(library.profiles.len(), 3);
    assert!(library.opening_styles.iter().all(|style| {
        style.parts.fixed_frame.profile_id.is_none()
            && style.parts.leaf_frame.profile_id.is_none()
            && style.grid.muntin_profile_id.is_none()
    }));
}

#[test]
fn legacy_library_without_profiles_loads_with_an_empty_catalog() {
    let lib = from_toml(r#"{"materials":[],"wall_styles":[]}"#)
        .expect("legacy library without profiles must load");
    assert!(lib.profiles.is_empty());
}

#[test]
fn section_profile_library_roundtrips() {
    let mut library = StyleLibrary::empty();
    let profile = SectionProfile::new(
        "section_roundtrip",
        "Roundtrip",
        SectionShape::Custom {
            points: vec![(0.0, 0.0), (0.07, 0.0), (0.05, 0.04)],
        },
    );
    assert!(library.upsert_section_profile(profile));

    let serialized = to_toml(&library).expect("serialize profile library");
    let deserialized = from_toml(&serialized).expect("deserialize profile library");
    assert_eq!(library, deserialized);
}

#[test]
fn project_section_profile_survives_combined_library_merge() {
    let mut project = crate::modules::aec::engine::project::ProjectFile::default();
    let mut project_profile = SectionProfile::new(
        "project_frame",
        "Project frame",
        SectionShape::Rectangle {
            face: 0.07,
            depth: 0.08,
        },
    );
    project_profile.style.origin = StyleOrigin::Project;
    assert!(project
        .material_wall_style_library
        .upsert_section_profile(project_profile.clone()));

    let empty_session = StyleLibrary::empty();
    let combined = combined_style_library_with_session(Some(&project), Some(&empty_session));
    assert_eq!(combined.find_section_profile("project_frame"), Some(&project_profile));
    assert_eq!(
        combined.find_section_profile("project_frame").unwrap().style.origin,
        StyleOrigin::Project
    );
}

#[test]
fn section_profile_upsert_search_and_remove_validate_custom_shapes() {
    let mut library = StyleLibrary::empty();
    let invalid = SectionProfile::new(
        "invalid",
        "Invalid profile",
        SectionShape::Custom {
            points: vec![(0.0, 0.0), (0.1, 0.0)],
        },
    );
    assert!(!library.upsert_section_profile(invalid));
    assert!(library.profiles.is_empty());

    let zero_width = SectionProfile::new(
        "zero_width",
        "Zero width",
        SectionShape::Rectangle {
            face: 0.0,
            depth: 0.08,
        },
    );
    assert!(!library.upsert_section_profile(zero_width));
    assert!(library.profiles.is_empty());

    let profile = SectionProfile::new(
        "custom",
        "Custom profile",
        SectionShape::Custom {
            points: vec![(0.0, 0.0), (0.1, 0.0), (0.05, 0.04)],
        },
    );
    assert!(library.upsert_section_profile(profile));
    assert_eq!(library.profiles[0].shape, SectionShape::Custom {
        points: vec![(0.0, 0.0), (0.1, 0.0), (0.05, 0.04), (0.0, 0.0)],
    });
    assert_eq!(
        library.find_section_profile_by_name_or_id("Custom profile").unwrap().style.id,
        "custom"
    );
    assert!(library.remove_section_profile("custom"));
    assert!(!library.remove_section_profile("custom"));
}


#[test]
fn legacy_gap_before_library_migrates_to_axis_offset() {
    let json = r#"{
        "materials": [],
        "wall_styles": [{
            "id": "legacy",
            "name": "Legacy",
            "category": "Wall",
            "parent": null,
            "layers": [
                {
                    "material_id": "a",
                    "thickness": 0.1,
                    "function": "Structural",
                    "gap_before": 0.0
                },
                {
                    "material_id": "b",
                    "thickness": 0.1,
                    "function": "Finish",
                    "gap_before": 0.05
                }
            ]
        }]
    }"#;
    let lib = from_toml(json).expect("legacy library must load");
    let style = &lib.wall_styles[0];
    assert!((style.layers[0].axis_offset.as_fixed_or(0.0) - (-0.125)).abs() < 1e-12);
    assert!((style.layers[1].axis_offset.as_fixed_or(0.0) - 0.025).abs() < 1e-12);
}

#[test]
fn test_remove_material() {
    let material = Material::new(
        "mat1".to_string(),
        "Material 1".to_string(),
        "HATCH1".to_string(),
        0xFF0000,
        "Continuous".to_string(),
    );
    let mut lib = StyleLibrary {
        materials: vec![material],
        wall_styles: Vec::new(),
        ..Default::default()
    };

    assert!(lib.remove_material("mat1"));
    assert!(lib.materials.is_empty());
    assert!(!lib.remove_material("mat1"));
}

#[test]
fn test_remove_wall_style() {
    let wall_style = WallStyle {
        style: Style {
            id: "style1".to_string(),
            name: "Style 1".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: Vec::new(),
    default_profile: None,
    };
    let mut lib = StyleLibrary {
        materials: Vec::new(),
        wall_styles: vec![wall_style],
        ..Default::default()
    };

    assert!(lib.remove_wall_style("style1"));
    assert!(lib.wall_styles.is_empty());
    assert!(!lib.remove_wall_style("style1"));
}

#[test]
fn test_wall_style_tree() {
    let mut lib = StyleLibrary::empty();
    let s1 = WallStyle {
        style: Style {
            id: "s1".to_string(),
            name: "Style A".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![],
    default_profile: None,
    };
    let s2 = WallStyle {
        style: Style {
            id: "s2".to_string(),
            name: "Style B".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: Some("s1".to_string()),
        },
        layers: vec![],
    default_profile: None,
    };
    let s3 = WallStyle {
        style: Style {
            id: "s3".to_string(),
            name: "Style C".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: Some("s1".to_string()),
        },
        layers: vec![],
    default_profile: None,
    };
    let s4 = WallStyle {
        style: Style {
            id: "s4".to_string(),
            name: "Style D".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: Some("orphan".to_string()),
        },
        layers: vec![],
    default_profile: None,
    };

    lib.wall_styles = vec![s1, s2, s3, s4];
    let tree = lib.wall_style_tree();

    assert_eq!(tree.len(), 4);

    // s1 (root)
    assert_eq!(tree[0].style.style.id, "s1");
    assert_eq!(tree[0].depth, 0);

    // s1 children sorted by name: s2 (Style B) then s3 (Style C)
    assert_eq!(tree[1].style.style.id, "s2");
    assert_eq!(tree[1].depth, 1);
    assert_eq!(tree[2].style.style.id, "s3");
    assert_eq!(tree[2].depth, 1);

    // s4 (orphaned parent is root)
    assert_eq!(tree[3].style.style.id, "s4");
    assert_eq!(tree[3].depth, 0);
}

#[test]
fn seed_default_library_demonstrates_new_layer_attributes() {
    let lib = seed_default_library();

    // At least one layer per demo style uses `role_tag`, and at least
    // one layer overrides its hatch pattern / drawing layer.
    let any_role_tag = lib
        .wall_styles
        .iter()
        .flat_map(|ws| ws.layers.iter())
        .any(|l| l.role_tag.is_some());
    let any_hatch_override = lib
        .wall_styles
        .iter()
        .flat_map(|ws| ws.layers.iter())
        .any(|l| l.hatch_override.is_some());
    let any_layer_override = lib
        .wall_styles
        .iter()
        .flat_map(|ws| ws.layers.iter())
        .any(|l| l.layer_override.is_some());
    assert!(any_role_tag, "expected at least one seeded layer with a role_tag");
    assert!(
        any_hatch_override,
        "expected at least one seeded layer with a hatch_override"
    );
    assert!(
        any_layer_override,
        "expected at least one seeded layer with a layer_override"
    );
}

#[test]
fn seed_default_library_includes_an_inherited_wall_style() {
    let lib = seed_default_library();

    let inherited = lib
        .wall_styles
        .iter()
        .find(|ws| ws.style.parent.is_some())
        .expect("expected at least one demo style with a parent");
    assert!(
        inherited.layers.is_empty(),
        "the inherited demo style should rely on effective_layers, not duplicate the parent's buildup"
    );

    let styles: std::collections::HashMap<_, _> = lib
        .wall_styles
        .iter()
        .map(|ws| (ws.style.id.clone(), ws.clone()))
        .collect();
    let effective = crate::modules::aec::engine::wall_style::effective_layers(
        &styles,
        &inherited.style.id,
    )
    .unwrap();
    assert!(!effective.is_empty(), "inherited style must resolve to a non-empty buildup");
}

#[test]
fn library_without_new_layer_fields_loads_with_defaults() {
    // Simulates a library file saved before `hatch_override`/`role_tag`
    // existed: they must default to `None` rather than failing to load.
    let json = r#"{
        "materials": [],
        "wall_styles": [
            {
                "id": "style1",
                "name": "Style 1",
                "category": "Wall",
                "parent": null,
                "layers": [
                    {
                        "material_id": "mat1",
                        "thickness": 0.2,
                        "function": "Structural"
                    }
                ]
            }
        ]
    }"#;
    let lib = from_toml(json).expect("old-format library must still deserialize");
    assert_eq!(lib.wall_styles.len(), 1);
    let layer = &lib.wall_styles[0].layers[0];
    assert_eq!(layer.hatch_override, None);
    assert_eq!(layer.role_tag, None);
    assert!(lib.opening_styles.is_empty());
}

#[test]
fn seed_default_library_includes_opening_styles() {
    use crate::modules::aec::engine::opening_style::{
        default_slots_for_kind, OpeningStyle, DEFAULT_FRAME_THICKNESS,
        SEED_BREAKTHROUGH_STYLE_ID, SEED_DOOR_STYLE_ID,
        SEED_WINDOW_STYLE_ID,
    };
    use crate::modules::aec::engine::display_component::OpeningComponentSlot;
    use crate::modules::aec::engine::openings::OpeningKind;

    let lib = seed_default_library();
    assert_eq!(lib.opening_styles.len(), 10);
    let window = lib.find_opening_style(SEED_WINDOW_STYLE_ID).expect("window seed");
    assert_eq!(window.kind, OpeningKind::Window);
    assert!((window.parts.fixed_frame.face_width - DEFAULT_FRAME_THICKNESS).abs() < 1e-12);
    assert_eq!(window.grid, OpeningStyle::standard_window().grid);
    let door = lib.find_opening_style(SEED_DOOR_STYLE_ID).expect("door seed");
    assert_eq!(door.kind, OpeningKind::Door);
    let br = lib
        .find_opening_style(SEED_BREAKTHROUGH_STYLE_ID)
        .expect("breakthrough seed");
    assert_eq!(br.kind, OpeningKind::Breakthrough);
    assert!((br.default_width - 1.0).abs() < 1e-12);
    assert!((br.default_height - 2.0).abs() < 1e-12);
    assert!((br.default_sill - 0.1).abs() < 1e-12);
    let fallback = default_slots_for_kind(br.kind);
    assert!(fallback.contains_key(&OpeningComponentSlot::Mark2D));
    assert!(!fallback.contains_key(&OpeningComponentSlot::Swing2D));
}

#[test]
fn opening_style_library_roundtrip() {
    let lib = seed_default_library();
    let serialized = to_toml(&lib).expect("serialize");
    let back = from_toml(&serialized).expect("deserialize");
    assert_eq!(lib.opening_styles, back.opening_styles);
}

#[test]
fn unreadable_opening_style_is_dropped_without_losing_other_library_data() {
    let library = seed_default_library();
    let expected_style_count = library.opening_styles.len();
    let mut value = serde_json::to_value(library).expect("serialize library");
    value["opening_styles"]
        .as_array_mut()
        .expect("opening style array")
        .push(serde_json::json!({ "invalid": true }));

    let loaded = from_toml(&value.to_string()).expect("load remaining library");
    assert_eq!(loaded.opening_styles.len(), expected_style_count);
    assert!(!loaded.materials.is_empty());
    assert!(!loaded.wall_styles.is_empty());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn load_or_seed_persists_removed_opening_styles() {
    let path = default_library_path();
    let library = seed_default_library();
    let expected_style_count = library.opening_styles.len();
    let mut value = serde_json::to_value(library).expect("serialize library");
    value["opening_styles"]
        .as_array_mut()
        .expect("opening style array")
        .push(serde_json::json!({ "invalid": true }));
    std::fs::write(&path, value.to_string()).expect("write library with invalid style");

    let loaded = load_or_seed();
    assert_eq!(loaded.opening_styles.len(), expected_style_count);
    let persisted = std::fs::read_to_string(&path).expect("read cleaned library");
    let persisted_value: serde_json::Value =
        serde_json::from_str(&persisted).expect("parse cleaned library");
    assert_eq!(
        persisted_value["opening_styles"].as_array().unwrap().len(),
        expected_style_count
    );

    let _ = std::fs::remove_file(path);
}

#[test]
fn seed_default_library_includes_new_example_materials() {
    let lib = seed_default_library();

    let wood = lib
        .materials
        .iter()
        .find(|m| m.id == "mat_wood")
        .expect("Holz material");
    assert_eq!(wood.name, "Holz");
    assert_eq!(wood.group.as_deref(), Some("Holz"));
    assert_eq!(wood.hatch_color, Some(acadrust::types::Color::Rgb { r: 0xA6, g: 0x7C, b: 0x52 }));
    assert!((wood.hatch_scale - 0.5).abs() < 1e-12);

    let drywall = lib
        .materials
        .iter()
        .find(|m| m.id == "mat_drywall")
        .expect("Gipskarton material");
    assert_eq!(drywall.group.as_deref(), Some("Trockenbau"));

    let steel = lib
        .materials
        .iter()
        .find(|m| m.id == "mat_steel")
        .expect("Stahl material");
    assert_eq!(steel.group.as_deref(), Some("Metall"));
    assert!((steel.hatch_scale - 1.5).abs() < 1e-12);

    let glass = lib
        .materials
        .iter()
        .find(|m| m.id == "mat_glass")
        .expect("Glas material");
    assert_eq!(glass.group.as_deref(), Some("Verglasung"));
    assert_eq!(glass.hatch_color, Some(acadrust::types::Color::Rgb { r: 0xA8, g: 0xD4, b: 0xE8 }));

    let clinker = lib
        .materials
        .iter()
        .find(|m| m.id == "mat_clinker")
        .expect("Klinker material");
    assert_eq!(clinker.name, "Klinker / Verblender");
    assert_eq!(clinker.group.as_deref(), Some("Mauerwerk"));
}

#[test]
fn materials_using_reports_wall_styles_and_layer_indices() {
    let lib = seed_default_library();
    let masonry_uses = lib.materials_using("mat_masonry");
    assert!(
        !masonry_uses.is_empty(),
        "mat_masonry should be referenced by at least one wall style layer"
    );
    assert!(masonry_uses.iter().any(|(ws, _)| ws.style.id == "style_masonry"));

    let wood_uses = lib.materials_using("mat_wood");
    assert!(
        !wood_uses.is_empty(),
        "mat_wood is used by style_timber_exterior"
    );
    assert!(wood_uses.iter().any(|(ws, _)| ws.style.id == "style_timber_exterior"));
}

#[test]
fn display_config_library_roundtrip() {
    use crate::modules::aec::engine::plan_view::{PlanningStage, ViewType};

    let mut lib = DisplayConfigLibrary::empty();
    let mut cfg = DisplayConfig::new(
        "Architekt 1:50".to_string(),
        "Architektur".to_string(),
        PlanningStage::Design,
        ViewType::FloorPlan,
    );
    cfg.scale = Some(50.0);
    lib.upsert(cfg);

    let serialized = display_config_library_to_toml(&lib).expect("serialization failed");
    let deserialized =
        display_config_library_from_toml(&serialized).expect("deserialization failed");
    assert_eq!(lib, deserialized);
}

#[test]
fn display_config_library_upsert_replaces_by_name() {
    use crate::modules::aec::engine::plan_view::{PlanningStage, ViewType};

    let mut lib = DisplayConfigLibrary::empty();
    lib.upsert(DisplayConfig::new(
        "Statik 1:50".to_string(),
        "Statik".to_string(),
        PlanningStage::Design,
        ViewType::Section,
    ));
    assert_eq!(lib.configs.len(), 1);

    let mut updated = DisplayConfig::new(
        "Statik 1:50".to_string(),
        "Statik (überarbeitet)".to_string(),
        PlanningStage::Design,
        ViewType::Section,
    );
    updated.scale = Some(100.0);
    lib.upsert(updated);

    assert_eq!(lib.configs.len(), 1, "same name must replace, not duplicate");
    let found = lib.find("Statik 1:50").expect("config must still be found by name");
    assert_eq!(found.discipline, "Statik (überarbeitet)");
    assert_eq!(found.scale, Some(100.0));
}

#[test]
fn display_config_library_remove_by_name() {
    use crate::modules::aec::engine::plan_view::{PlanningStage, ViewType};

    let mut lib = DisplayConfigLibrary::empty();
    lib.upsert(DisplayConfig::new(
        "Präsentation 1:200".to_string(),
        "Präsentation".to_string(),
        PlanningStage::Design,
        ViewType::FloorPlan,
    ));

    assert!(lib.remove("Präsentation 1:200"));
    assert!(lib.configs.is_empty());
    assert!(!lib.remove("Präsentation 1:200"));
}

#[test]
fn display_config_library_without_configs_field_deserializes_empty() {
    let json = r#"{}"#;
    let lib = display_config_library_from_toml(json)
        .expect("missing configs field must default to empty");
    assert!(lib.configs.is_empty());
    assert!(lib.scale_display_config_mappings.is_empty());
}

#[test]
fn display_config_library_scale_mappings_roundtrip() {
    use crate::modules::aec::engine::plan_view::{
        PlanningStage, ScaleDisplayConfigMapping, ViewType,
    };

    let mut lib = DisplayConfigLibrary::empty();
    lib.upsert(DisplayConfig::new(
        "Architekt 1:50".to_string(),
        "Architektur".to_string(),
        PlanningStage::Design,
        ViewType::FloorPlan,
    ));
    lib.scale_display_config_mappings.push(ScaleDisplayConfigMapping {
        scale_name: "1:50".to_string(),
        display_config_name: "Architekt 1:50".to_string(),
    });

    let serialized = display_config_library_to_toml(&lib).expect("serialization failed");
    let deserialized =
        display_config_library_from_toml(&serialized).expect("deserialization failed");
    assert_eq!(lib, deserialized);
}

#[test]
fn display_config_library_without_scale_mappings_field_deserializes_empty() {
    // Simulates a library file saved before Step 7 introduced
    // `scale_display_config_mappings`: must default to empty, not fail.
    let json = r#"{
        "configs": [
            {
                "name": "Architekt 1:50",
                "discipline": "Architektur",
                "planning_stage": "Design",
                "view_type": "FloorPlan"
            }
        ]
    }"#;
    let lib = display_config_library_from_toml(json)
        .expect("old-format library without scale mappings must still deserialize");
    assert_eq!(lib.configs.len(), 1);
    assert!(lib.scale_display_config_mappings.is_empty());
}

#[test]
fn combined_material_entries_project_overrides_standard_on_id() {
    use crate::modules::aec::engine::project::ProjectFile;

    let standard_id = "shared_mat".to_string();
    let mut project = ProjectFile::default();
    project.material_wall_style_library.upsert_material(Material::new(
        standard_id.clone(),
        "Project Material".to_string(),
        "SOLID".to_string(),
        0x00FF00,
        "Continuous".to_string(),
    ));
    project.material_wall_style_library.upsert_material(Material::new(
        "proj_only".to_string(),
        "Project Only".to_string(),
        "SOLID".to_string(),
        0x0000FF,
        "Continuous".to_string(),
    ));

    let entries = combined_material_entries(Some(&project));
    let shared = entries
        .iter()
        .find(|e| e.material.id == standard_id)
        .expect("shared id present");
    assert_eq!(shared.source, LibrarySource::Project);
    assert_eq!(shared.material.name, "Project Material");
    assert!(entries
        .iter()
        .any(|e| e.material.id == "proj_only" && e.source == LibrarySource::Project));
    // Standard-only entries (from load_or_seed) still appear with Standard source.
    assert!(entries.iter().any(|e| e.source == LibrarySource::Standard));
}

fn join_example_walls() -> Vec<Wall> {
    fn layer(mat: &str, th: f64, func: &str, off: f64) -> WallLayer {
        WallLayer {
            material: mat.to_string(),
            thickness: th,
            function: func.to_string(),
            axis_offset: off,
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            layer_id: uuid::Uuid::nil(),
        }
    }
    let four = vec![
        layer("Putz", 0.015, "Finish", -0.195),
        layer("Mauerwerk", 0.24, "Structural", -0.18),
        layer("Insulation", 0.12, "Insulation", 0.06),
        layer("Putz", 0.015, "Finish", 0.18),
    ];
    let one = vec![layer("Concrete", 0.2, "Structural", -0.1)];
    vec![
        {
            let mut w = Wall::new("style1", 3.0, 0);
            w.layers = one;
            w
        },
        {
            let mut w = Wall::new("style1", 3.0, 0);
            w.layers = four;
            w
        },
    ]
}

#[test]
fn extract_style_library_from_join_example_walls() {
    let lib = extract_style_library_from_walls(&join_example_walls());
    assert!(lib.materials.iter().any(|m| m.id == "Putz"));
    assert!(lib.materials.iter().any(|m| m.id == "Mauerwerk"));
    assert!(lib.materials.iter().any(|m| m.id == "Insulation"));
    assert!(lib.materials.iter().any(|m| m.id == "Concrete"));
    assert!(lib.wall_styles.iter().any(|s| s.style.id == "style1"));
    assert!(lib.wall_styles.iter().any(|s| s.style.id == "style1#2"));
    let one = lib
        .wall_styles
        .iter()
        .find(|s| s.layers.len() == 1)
        .expect("1-layer style");
    let four = lib
        .wall_styles
        .iter()
        .find(|s| s.layers.len() == 4)
        .expect("4-layer style");
    assert_eq!(four.layers[1].material_id, "Mauerwerk");
    assert_eq!(one.style.name, "style1 (Concrete)");
    assert_eq!(
        four.style.name,
        "style1 (Putz+Mauerwerk+Insulation+Putz)"
    );
    assert_ne!(one.style.name, four.style.name);
}

#[test]
fn extract_single_stack_keeps_plain_style_name() {
    let walls = vec![join_example_walls().into_iter().next().unwrap()];
    let lib = extract_style_library_from_walls(&walls);
    let style = lib.wall_styles.iter().find(|s| s.style.id == "style1");
    assert_eq!(style.map(|s| s.style.name.as_str()), Some("style1"));
}

#[test]
fn combined_entries_include_session_without_project() {
    let extracted = extract_style_library_from_walls(&join_example_walls());
    let session = session_library_excluding_existing(&extracted, None);
    let materials = combined_material_entries_with_session(None, Some(&session));
    let styles = combined_wall_style_entries_with_session(None, Some(&session));
    assert!(materials.iter().any(|e| {
        e.source == LibrarySource::Session && e.material.id == "Putz"
    }));
    assert!(styles.iter().any(|e| {
        e.source == LibrarySource::Session && e.wall_style.style.id == "style1"
    }));
    assert!(materials.iter().any(|e| e.source == LibrarySource::Standard));
}

#[test]
fn session_library_skips_ids_already_in_standard() {
    let standard = load_or_seed();
    let Some(existing) = standard.materials.first().cloned() else {
        return;
    };
    let mut extracted = StyleLibrary::empty();
    extracted.upsert_material(existing.clone());
    extracted.upsert_material(Material::new(
        "session_only_mat".into(),
        "Session Only".into(),
        "SOLID".into(),
        0x111111,
        "Continuous".into(),
    ));
    let session = session_library_excluding_existing(&extracted, None);
    assert!(session.materials.iter().all(|m| m.id != existing.id));
    assert!(session.materials.iter().any(|m| m.id == "session_only_mat"));
}

#[test]
fn test_material_copy_conflict() {
    let mat_a = Material::new(
        "mat1".to_string(),
        "Material 1".to_string(),
        "HATCH1".to_string(),
        0xFF0000,
        "Continuous".to_string(),
    );
    let mut mat_b = mat_a.clone();
    mat_b.name = "Material 1 Updated".to_string();

    let mut target = StyleLibrary::empty();
    assert_eq!(material_copy_conflict(&target, &mat_a), CopyConflict::None);

    target.upsert_material(mat_a.clone());
    assert_eq!(
        material_copy_conflict(&target, &mat_a),
        CopyConflict::IdenticalAlreadyPresent
    );
    assert_eq!(
        material_copy_conflict(&target, &mat_b),
        CopyConflict::DifferentContentCollision
    );
}

#[test]
fn test_wall_style_copy_conflict() {
    let style_a = WallStyle {
        style: Style {
            id: "style1".to_string(),
            name: "Style 1".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![],
    default_profile: None,
    };
    let mut style_b = style_a.clone();
    style_b.style.name = "Style 1 Updated".to_string();

    let mut target = StyleLibrary::empty();
    assert_eq!(wall_style_copy_conflict(&target, &style_a), CopyConflict::None);

    target.upsert_wall_style(style_a.clone());
    assert_eq!(
        wall_style_copy_conflict(&target, &style_a),
        CopyConflict::IdenticalAlreadyPresent
    );
    assert_eq!(
        wall_style_copy_conflict(&target, &style_b),
        CopyConflict::DifferentContentCollision
    );
}

#[test]
fn build_effective_rule_set_session_3d_hides_2d_slots() {
    let mut cfg = DisplayConfig::new(
        "Entwurf".into(),
        "Architektur".into(),
        crate::modules::aec::engine::plan_view::PlanningStage::Design,
        crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
    );
    cfg.detail.representation = RepresentationMode::All;
    let rules = build_effective_rule_set(&cfg, None, Some(RepresentationMode::ThreeD));
    assert!(!rules.is_visible(WallComponentSlot::Layers2D));
    assert!(!rules.is_visible(WallComponentSlot::Contour2D));
    assert!(rules.is_visible(WallComponentSlot::Solid3D));
}

#[test]
fn viewport_3d_hides_2d_components_including_hatch() {
    let mut cfg = DisplayConfig::new(
        "Entwurf".into(),
        "Architektur".into(),
        crate::modules::aec::engine::plan_view::PlanningStage::Design,
        crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
    );
    cfg.detail.representation = RepresentationMode::TwoD;
    let mode = effective_representation(&cfg, Some(RepresentationMode::ThreeD));
    assert!(!component_is_visible(&cfg, WallComponentKind::Layers2D, mode));
    assert!(!component_is_visible(&cfg, WallComponentKind::LayerHatch2D, mode));
    assert!(component_is_visible(&cfg, WallComponentKind::Layers3D, mode));
}

#[test]
fn session_all_shows_3d_even_if_planart_hides_layers3d() {
    let mut cfg = DisplayConfig::new(
        "Entwurf".into(),
        "Architektur".into(),
        crate::modules::aec::engine::plan_view::PlanningStage::Design,
        crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
    );
    cfg.detail.representation = RepresentationMode::TwoD;
    cfg.component_visibility
        .insert(WallComponentKind::Layers3D, false);
    cfg.component_visibility
        .insert(WallComponentKind::SurfaceStyle3D, false);
    let rules = build_effective_rule_set(&cfg, None, Some(RepresentationMode::All));
    assert!(rules.is_visible(WallComponentSlot::Solid3D));
    assert!(rules.is_visible(WallComponentSlot::SurfaceStyle3D));
    assert!(rules.is_visible(WallComponentSlot::Layers2D));
    let rules_3d = build_effective_rule_set(&cfg, None, Some(RepresentationMode::ThreeD));
    assert!(rules_3d.is_visible(WallComponentSlot::Solid3D));
    assert!(!rules_3d.is_visible(WallComponentSlot::Layers2D));
}

#[test]
fn session_3d_overrides_hidden_solid_in_display_profiles() {
    use crate::modules::aec::engine::display_component::ComponentRuleSet;
    use crate::modules::aec::engine::wall_style::WallStyle;
    let mut cfg = DisplayConfig::new(
        "Entwurf".into(),
        "Architektur".into(),
        crate::modules::aec::engine::plan_view::PlanningStage::Design,
        crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
    );
    cfg.detail.representation = RepresentationMode::TwoD;
    let mut style = WallStyle {
        style: Style {
            id: "s".to_string(),
            name: "S".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![],
        default_profile: None,
    };
    let mut profile = ComponentRuleSet::default();
    profile
        .visibility
        .insert(WallComponentSlot::Solid3D.key().to_string(), false);
    style.default_profile = Some(profile);
    let rules = build_effective_rule_set(&cfg, Some(&style), Some(RepresentationMode::ThreeD));
    assert!(rules.is_visible(WallComponentSlot::Solid3D));
    assert!(!rules.is_visible(WallComponentSlot::Layers2D));
    let rules_2d = build_effective_rule_set(&cfg, Some(&style), Some(RepresentationMode::TwoD));
    assert!(!rules_2d.is_visible(WallComponentSlot::Solid3D));
    assert!(rules_2d.is_visible(WallComponentSlot::Layers2D));
}

#[test]
fn planart_can_show_layers2d_without_layer_hatch() {
    let mut cfg = DisplayConfig::new(
        "Statik".into(),
        "Statik".into(),
        crate::modules::aec::engine::plan_view::PlanningStage::Design,
        crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
    );
    cfg.component_visibility
        .insert(WallComponentKind::Layers2D, true);
    cfg.component_visibility
        .insert(WallComponentKind::LayerHatch2D, false);
    let mode = RepresentationMode::All;
    assert!(component_is_visible(&cfg, WallComponentKind::Layers2D, mode));
    assert!(!component_is_visible(
        &cfg,
        WallComponentKind::LayerHatch2D,
        mode
    ));
}

#[test]
fn field_override_color_keeps_material_hatch() {
    let layer_id = Uuid::new_v4();
    let mut cfg = DisplayConfig::new(
        "A".into(),
        "Arch".into(),
        crate::modules::aec::engine::plan_view::PlanningStage::Design,
        crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
    );
    let mut overlay = StyleDisplayOverlay::default();
    overlay.layer_props.insert(
        layer_id,
        ComponentStyleOverride {
            line_color: Some(acadrust::types::Color::Rgb { r: 255, g: 0, b: 0 }),
            ..Default::default()
        },
    );
    cfg.style_overrides.insert("style1".into(), overlay);
    let mat = Material::new(
        "m".into(),
        "M".into(),
        "ANSI31".into(),
        0x111111,
        "Continuous".into(),
    );
    let resolved = resolve_layer_property_override(&cfg, "style1", layer_id, None, Some(&mat));
    assert_eq!(
        resolved.line_color,
        Some(acadrust::types::Color::Rgb { r: 255, g: 0, b: 0 })
    );
    assert_eq!(resolved.hatch_pattern.as_deref(), Some("ANSI31"));
}

#[test]
fn field_override_hatch_scale_keeps_material_pattern() {
    let layer_id = Uuid::new_v4();
    let mut cfg = DisplayConfig::new(
        "A".into(),
        "Arch".into(),
        crate::modules::aec::engine::plan_view::PlanningStage::Design,
        crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
    );
    let mut overlay = StyleDisplayOverlay::default();
    overlay.layer_props.insert(
        layer_id,
        ComponentStyleOverride {
            hatch_scale: Some(2.5),
            ..Default::default()
        },
    );
    cfg.style_overrides.insert("style1".into(), overlay);
    let mut mat = Material::new(
        "m".into(),
        "M".into(),
        "ANSI31".into(),
        0x111111,
        "Continuous".into(),
    );
    mat.hatch_scale = 1.0;
    let resolved = resolve_layer_property_override(&cfg, "style1", layer_id, None, Some(&mat));
    assert_eq!(resolved.hatch_scale, Some(2.5));
    assert_eq!(resolved.hatch_pattern.as_deref(), Some("ANSI31"));
}

#[test]
fn contour_hatch_on_planart_merges_into_contour_hatch2d_slot() {
    let mut cfg = DisplayConfig::new(
        "A".into(),
        "Arch".into(),
        crate::modules::aec::engine::plan_view::PlanningStage::Design,
        crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
    );
    // Global plan-type hatch must not apply; only per-style overlay.
    cfg.contour_hatch = Some(ComponentStyleOverride {
        hatch_pattern: Some("SOLID".into()),
        hatch_scale: Some(9.0),
        ..Default::default()
    });
    let mut overlay = StyleDisplayOverlay::default();
    overlay.contour_hatch = Some(ComponentStyleOverride {
        hatch_pattern: Some("ANSI31".into()),
        hatch_scale: Some(2.0),
        ..Default::default()
    });
    cfg.style_overrides.insert("s".into(), overlay);
    let style = WallStyle {
        style: Style {
            id: "s".to_string(),
            name: "S".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![],
        default_profile: None,
    };
    let rules = build_effective_rule_set(&cfg, Some(&style), None);
    let ov = rules
        .style_for(WallComponentSlot::ContourHatch2D)
        .expect("contour hatch slot");
    assert_eq!(ov.hatch_pattern.as_deref(), Some("ANSI31"));
    assert_eq!(ov.hatch_scale, Some(2.0));
    let without = build_effective_rule_set(&cfg, None, None);
    assert!(without.style_for(WallComponentSlot::ContourHatch2D).is_none());
}

#[test]
fn overlay_layer_visibility_filters_2d_and_3d() {
    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();
    let mut overlay = StyleDisplayOverlay::default();
    overlay.layer_visibility.insert(
        id_b,
        crate::modules::aec::engine::display_component::LayerVis {
            visible_2d: false,
            visible_3d: true,
        },
    );
    let mut cfg = DisplayConfig::new(
        "A".into(),
        "Arch".into(),
        crate::modules::aec::engine::plan_view::PlanningStage::Design,
        crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
    );
    cfg.style_overrides.insert("s".into(), overlay);
    let style = WallStyle {
        style: Style {
            id: "s".to_string(),
            name: "S".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![
            Layer {
                material_id: "a".into(),
                thickness: LayerValue::Fixed(0.1),
                function: LayerFunction::Structural,
                axis_offset: LayerValue::Fixed(0.0),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: None,
                layer_id: id_a,
            },
            Layer {
                material_id: "b".into(),
                thickness: LayerValue::Fixed(0.1),
                function: LayerFunction::Insulation,
                axis_offset: LayerValue::Fixed(0.1),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: None,
                layer_id: id_b,
            },
        ],
        default_profile: None,
    };
    let rules = build_effective_rule_set(&cfg, Some(&style), None);
    match rules.layer_filter_for(WallComponentSlot::Layers2D) {
        LayerSelection::Explicit(refs) => {
            assert_eq!(refs.len(), 1);
            assert_eq!(refs[0].layer_id, Some(id_a));
        }
        other => panic!("expected explicit 2D filter, got {other:?}"),
    }
    assert_eq!(
        rules.layer_filter_for(WallComponentSlot::Solid3D),
        &LayerSelection::All
    );
}

#[test]
fn merge_overlay_hatch_scale_into_rules() {
    let layer_id = Uuid::new_v4();
    let mut overlay = StyleDisplayOverlay::default();
    overlay.layer_props.insert(
        layer_id,
        ComponentStyleOverride {
            hatch_scale: Some(3.0),
            ..Default::default()
        },
    );
    let mut rules = ComponentRuleSet::default();
    merge_style_overlay_into_rules(&mut rules, &overlay);
    assert_eq!(
        rules
            .layer_style_override
            .iter()
            .find(|l| l.layer.layer_id == Some(layer_id))
            .and_then(|l| l.style.hatch_scale),
        Some(3.0)
    );
}

#[test]
fn phase_style_only_on_contour2d() {
    let ov = ComponentStyleOverride {
        line_type: Some("Dashed".into()),
        ..Default::default()
    };
    assert!(phase_style_for_slot(Some(&ov), WallComponentSlot::Contour2D).is_some());
    assert!(phase_style_for_slot(Some(&ov), WallComponentSlot::ContourHatch2D).is_none());
    assert!(phase_style_for_slot(Some(&ov), WallComponentSlot::Layers2D).is_none());
}

#[test]
fn overlay_unknown_layer_id_is_ignored_by_resolver() {
    let mut cfg = DisplayConfig::new(
        "A".into(),
        "A".into(),
        crate::modules::aec::engine::plan_view::PlanningStage::Design,
        crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
    );
    let mut overlay = StyleDisplayOverlay::default();
    overlay.layer_props.insert(
        Uuid::new_v4(),
        ComponentStyleOverride {
            line_type: Some("Hidden".into()),
            ..Default::default()
        },
    );
    cfg.style_overrides.insert("style1".into(), overlay);
    let other = Uuid::new_v4();
    let resolved = resolve_layer_property_override(&cfg, "style1", other, None, None);
    assert!(resolved.line_type.is_none());
}

#[test]
fn seed_default_library_includes_slab_styles() {
    let lib = seed_default_library();
    assert_eq!(lib.slab_styles.len(), 10);
    let concrete_20 = lib.find_slab_style("style_slab_concrete_20").expect("concrete 20 seed");
    assert_eq!(concrete_20.structural_style_id.as_deref(), Some("struct_slab_concrete_20"));
    let resolved_layers = lib.resolve_slab_layers("style_slab_concrete_20");
    assert_eq!(resolved_layers.len(), 1);
    assert_eq!(resolved_layers[0].material_id, "mat_concrete");

    let floor_eg = lib.find_slab_style("style_slab_floor_eg_34").expect("floor eg seed");
    assert_eq!(floor_eg.default_finish_style_id.as_deref(), Some("finish_floor_tiles_80"));
    let resolved_floor_eg = lib.resolve_slab_layers("style_slab_floor_eg_34");
    assert_eq!(resolved_floor_eg.len(), 4);
}

#[test]
fn test_remove_slab_style() {
    let mut lib = StyleLibrary::empty();
    let ss = SlabStyle::new("slab1", "Slab 1");
    lib.upsert_slab_style(ss);
    assert_eq!(lib.slab_styles.len(), 1);
    assert!(lib.remove_slab_style("slab1"));
    assert!(lib.slab_styles.is_empty());
    assert!(!lib.remove_slab_style("slab1"));
}

#[test]
fn test_slab_style_tree() {
    let mut lib = StyleLibrary::empty();
    let s1 = SlabStyle::new("s1", "Root Slab");
    let mut s2 = SlabStyle::new("s2", "Child Slab");
    s2.style.parent = Some("s1".to_string());
    lib.upsert_slab_style(s1);
    lib.upsert_slab_style(s2);

    let tree = lib.slab_style_tree();
    assert_eq!(tree.len(), 2);
    assert_eq!(tree[0].style.style.id, "s1");
    assert_eq!(tree[0].depth, 0);
    assert_eq!(tree[1].style.style.id, "s2");
    assert_eq!(tree[1].depth, 1);
}

#[test]
fn test_extract_style_library_from_slabs() {
    let mut slab = Slab::new("style_slab_custom", 1);
    slab.layers = vec![
        crate::modules::aec::engine::slab::SlabLayer::new("mat_concrete", 0.22, LayerFunction::Structural),
        crate::modules::aec::engine::slab::SlabLayer::new("mat_plaster", 0.015, LayerFunction::Finish),
    ];

    let extracted = extract_style_library_from_slabs(&[slab]);
    assert_eq!(extracted.slab_styles.len(), 1);
    assert_eq!(extracted.slab_styles[0].style.id, "style_slab_custom");
    assert_eq!(extracted.slab_styles[0].layers.len(), 2);
}

#[test]
fn build_effective_slab_rule_set_respects_2d_session_mode() {
    use crate::modules::aec::engine::display_component::{
        RepresentationMode, SlabComponentSlot,
    };

    let cfg = DisplayConfig::new(
        "Plan".into(),
        "Plan".into(),
        crate::modules::aec::engine::plan_view::PlanningStage::Design,
        crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
    );
    let rules = build_effective_slab_rule_set(&cfg, None, Some(RepresentationMode::TwoD));
    assert!(rules.is_slab_visible(SlabComponentSlot::Contour2D));
    assert!(rules.is_slab_visible(SlabComponentSlot::OpeningSymbol2D));
    assert!(!rules.is_slab_visible(SlabComponentSlot::Solid3D));
    assert!(!rules.is_slab_visible(SlabComponentSlot::SurfaceStyle3D));
}

fn plain_cfg(name: &str) -> DisplayConfig {
    DisplayConfig::new(
        name.into(),
        "Plan".into(),
        crate::modules::aec::engine::plan_view::PlanningStage::Design,
        crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
    )
}

#[test]
fn slab_ceiling_view_comes_from_detail_not_name() {
    use crate::modules::aec::engine::display_component::SlabComponentSlot;
    // A name containing "Deckenspiegel" must not change anything.
    let named = build_effective_slab_rule_set(&plain_cfg("Deckenspiegel"), None, None);
    assert!(named.is_slab_visible(SlabComponentSlot::Contour2D));
    assert!(!named.is_slab_visible(SlabComponentSlot::CeilingOutline2D));

    let mut cfg = plain_cfg("Meine Decke");
    cfg.detail.ceiling_view = true;
    let rules = build_effective_slab_rule_set(&cfg, None, None);
    assert!(!rules.is_slab_visible(SlabComponentSlot::Contour2D));
    assert!(rules.is_slab_visible(SlabComponentSlot::CeilingOutline2D));
    assert!(!rules.is_slab_visible(SlabComponentSlot::OpeningSymbol2D));
}

#[test]
fn slab_structural_only_comes_from_detail_not_name() {
    use crate::modules::aec::engine::display_component::SlabComponentSlot;
    let mut style = SlabStyle::new("s", "S");
    style.layers = vec![
        crate::modules::aec::engine::slab_style::SlabStyleLayer::new("c", LayerValue::Fixed(0.2), LayerFunction::Structural),
        crate::modules::aec::engine::slab_style::SlabStyleLayer::new("f", LayerValue::Fixed(0.05), LayerFunction::Finish),
    ];
    let named = build_effective_slab_rule_set(&plain_cfg("Rohbau"), Some(&style), None);
    assert!(named.layer_filter.is_empty());
    let mut cfg = plain_cfg("Tragwerk");
    cfg.detail.layers = LayerDetail::StructuralOnly;
    let rules = build_effective_slab_rule_set(&cfg, Some(&style), None);
    assert!(rules.layer_filter.contains_key(SlabComponentSlot::Solid3D.key()));
}

#[test]
fn wall_detail_switches_hide_slots() {
    let key = WallComponentSlot::LayerHatch2D.key();
    let rules = build_effective_rule_set(&plain_cfg("P"), None, None);
    assert_eq!(rules.visibility.get(key), Some(&true));
    let mut cfg = plain_cfg("P");
    cfg.detail.hatches = false;
    let rules = build_effective_rule_set(&cfg, None, None);
    assert_eq!(rules.visibility.get(key), Some(&false));
}

#[test]
fn wall_precedence_session_over_plan_over_style_default() {
    let key = WallComponentSlot::Contour2D.key();
    let mut style = WallStyle {
        style: Style {
            id: "s".into(),
            name: "S".into(),
            category: "Wall".into(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![],
        default_profile: None,
    };
    let mut cfg = plain_cfg("P");
    cfg.detail.representation = RepresentationMode::TwoD;
    // Style default profile hides the contour for this plan kind.
    let mut profile = ComponentRuleSet::default();
    profile.visibility.insert(key.to_string(), false);
    style.default_profile = Some(profile);
    let rules = build_effective_rule_set(&cfg, Some(&style), None);
    assert_eq!(rules.visibility.get(key), Some(&false));
    // Session representation wins over the style default.
    let rules = build_effective_rule_set(&cfg, Some(&style), Some(RepresentationMode::TwoD));
    assert_eq!(rules.visibility.get(key), Some(&true));
    // Plan kind (representation 3D) hides 2D slots regardless of style default.
    cfg.detail.representation = RepresentationMode::ThreeD;
    let rules = build_effective_rule_set(&cfg, None, None);
    assert_eq!(rules.visibility.get(key), Some(&false));
}

#[test]
fn seed_presets_carry_detail_and_roundtrip() {
    let lib = DisplayConfigLibrary::seed_default_configs();
    let rcp = lib.configs.iter().find(|c| c.preset.as_deref() == Some(PRESET_DECKENSPIEGEL)).unwrap();
    assert!(rcp.detail.ceiling_view);
    let entwurf = lib.configs.iter().find(|c| c.preset.as_deref() == Some(PRESET_ENTWURF_100)).unwrap();
    assert!(!entwurf.detail.hatches);
    assert_eq!(
        entwurf.opening_levels.plan_frame,
        Some(crate::modules::aec::engine::opening_style::PlanFrameLevel::SingleLine)
    );
    assert_eq!(
        entwurf.opening_levels.swing,
        Some(crate::modules::aec::engine::opening_style::SwingGlyph::Off)
    );
    let detail = lib.configs.iter().find(|c| c.preset.as_deref() == Some(PRESET_WERKPLAN_50)).unwrap();
    assert_eq!(
        detail.opening_levels.plan_frame,
        Some(crate::modules::aec::engine::opening_style::PlanFrameLevel::Profile)
    );
    assert_eq!(
        detail.opening_levels.divisions,
        Some(crate::modules::aec::engine::opening_style::DivisionLevel::PostsAndMuntins)
    );
    let json = display_config_library_to_toml(&lib).unwrap();
    assert_eq!(display_config_library_from_toml(&json).unwrap(), lib);
}

#[test]
fn default_library_path_is_isolated_per_thread() {
    let main = default_library_path();
    let other = std::thread::spawn(default_library_path).join().unwrap();
    assert_ne!(main, other);
    assert!(main.to_string_lossy().contains("ocs-test-aec-lib-"));
    assert_eq!(main.file_name().and_then(|n| n.to_str()), Some("aec_styles.toml"));
    assert_eq!(
        default_display_config_library_path().parent(),
        main.parent()
    );
}

#[test]
fn load_or_seed_does_not_rewrite_when_layer_ids_present() {
    let path = default_library_path();
    let _ = std::fs::remove_file(&path);
    let _ = load_or_seed();
    let before = std::fs::read(&path).expect("seeded library");
    assert!(!library_source_missing_layer_ids(
        std::str::from_utf8(&before).unwrap()
    ));
    let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let _ = load_or_seed();
    let after = std::fs::read(&path).expect("library after second load");
    assert_eq!(before, after);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        mtime
    );
}

#[test]
fn load_or_seed_persists_missing_layer_ids_once() {
    let mut lib = seed_default_library();
    lib.wall_styles.truncate(1);
    let mut json = serde_json::to_value(&lib).unwrap();
    if let Some(layers) = json
        .pointer_mut("/wall_styles/0/layers")
        .and_then(|v| v.as_array_mut())
    {
        for layer in layers {
            if let Some(obj) = layer.as_object_mut() {
                obj.remove("layer_id");
            }
        }
    }
    let content = serde_json::to_string_pretty(&json).unwrap();
    assert!(library_source_missing_layer_ids(&content));
    let path = default_library_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, &content).unwrap();
    let loaded = load_or_seed();
    let persisted = std::fs::read_to_string(&path).unwrap();
    assert!(!library_source_missing_layer_ids(&persisted));
    assert!(!loaded.wall_styles[0].layers.is_empty());
    assert!(loaded.wall_styles[0].layers.iter().all(|l| l.layer_id != uuid::Uuid::nil()));
    let _ = save_to_default_path(&seed_default_library());
}
