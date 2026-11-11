use super::*;

/// Builds a small, ready-to-use default library so `AEC_WALL`'s style
/// selection has something to offer out of the box, without requiring the
/// user to define materials/styles first via `AEC_MATERIAL`/`AEC_STYLE`.
pub fn seed_default_library() -> StyleLibrary {
    let masonry = Material {
        group: Some("Mauerwerk".to_string()),
        hatch_scale: 0.0025,
        ..Material::new(
            "mat_masonry".to_string(),
            "Mauerwerk".to_string(),
            "ANSI31".to_string(),
            0x8B7355,
            "Continuous".to_string(),
        )
    };
    let concrete = Material {
        group: Some("Beton".to_string()),
        ..Material::new(
            "mat_concrete".to_string(),
            "Stahlbeton".to_string(),
            "AR-CONC".to_string(),
            0x888888,
            "Continuous".to_string(),
        )
    };
    let insulation = Material {
        group: Some("Daemmung".to_string()),
        hatch_scale: 0.75,
        ..Material::new(
            "mat_insulation".to_string(),
            "Daemmung".to_string(),
            "ANSI37".to_string(),
            0xFFDD88,
            "Continuous".to_string(),
        )
    };
    let plaster = Material {
        group: Some("Putz".to_string()),
        hatch_scale: 0.0012,
        hatch_color: Some(acadrust::types::Color::Rgb { r: 180, g: 180, b: 176 }),
        ..Material::new(
            "mat_plaster".to_string(),
            "Putz".to_string(),
            "DOTS".to_string(),
            0xC8C8C4,
            "Continuous".to_string(),
        )
    };
    let wood = Material {
        group: Some("Holz".to_string()),
        hatch_color: Some(acadrust::types::Color::Rgb { r: 166, g: 124, b: 82 }),
        hatch_scale: 0.5,
        ..Material::new(
            "mat_wood".to_string(),
            "Holz".to_string(),
            "ANSI31".to_string(),
            0xC4A35A,
            "Continuous".to_string(),
        )
    };
    let drywall = Material {
        group: Some("Trockenbau".to_string()),
        hatch_color: Some(acadrust::types::Color::Rgb { r: 232, g: 232, b: 224 }),
        hatch_scale: 1.0,
        ..Material::new(
            "mat_drywall".to_string(),
            "Gipskarton".to_string(),
            "ANSI31".to_string(),
            0xF5F5F0,
            "Continuous".to_string(),
        )
    };
    let steel = Material {
        group: Some("Metall".to_string()),
        hatch_color: Some(acadrust::types::Color::Rgb { r: 74, g: 85, b: 104 }),
        hatch_scale: 1.5,
        ..Material::new(
            "mat_steel".to_string(),
            "Stahl".to_string(),
            "ANSI32".to_string(),
            0x708090,
            "Continuous".to_string(),
        )
    };
    let glass = Material {
        group: Some("Verglasung".to_string()),
        hatch_color: Some(acadrust::types::Color::Rgb { r: 168, g: 212, b: 232 }),
        hatch_scale: 2.0,
        ..Material::new(
            "mat_glass".to_string(),
            "Glas".to_string(),
            "ANSI33".to_string(),
            0xC8E6F0,
            "Continuous".to_string(),
        )
    };
    let clinker = Material {
        group: Some("Mauerwerk".to_string()),
        hatch_color: Some(acadrust::types::Color::Rgb { r: 178, g: 34, b: 34 }),
        hatch_scale: 0.002,
        ..Material::new(
            "mat_clinker".to_string(),
            "Klinker / Verblender".to_string(),
            "ANSI31".to_string(),
            0x8B2500,
            "Continuous".to_string(),
        )
    };
    let air = Material {
        group: Some("Daemmung".to_string()),
        hatch_color: None,
        hatch_scale: 1.0,
        ..Material::new(
            "mat_air".to_string(),
            "Luftschicht".to_string(),
            "SOLID".to_string(),
            0xE0E0E0,
            "Continuous".to_string(),
        )
    };
    let screed = Material {
        group: Some("Bodenaufbau".to_string()),
        hatch_scale: 0.0015,
        hatch_color: Some(acadrust::types::Color::Rgb { r: 160, g: 160, b: 155 }),
        ..Material::new(
            "mat_screed".to_string(),
            "Estrich".to_string(),
            "DOTS".to_string(),
            0xB0B0A8,
            "Continuous".to_string(),
        )
    };
    let tile = Material {
        group: Some("Bodenbelag".to_string()),
        hatch_scale: 0.05,
        hatch_color: Some(acadrust::types::Color::Rgb { r: 210, g: 180, b: 140 }),
        ..Material::new(
            "mat_tile".to_string(),
            "Fliesen / Belag".to_string(),
            "NET".to_string(),
            0xD2B48C,
            "Continuous".to_string(),
        )
    };
    let bitumen = Material {
        group: Some("Abdichtung".to_string()),
        hatch_scale: 0.5,
        hatch_color: Some(acadrust::types::Color::Rgb { r: 50, g: 50, b: 50 }),
        ..Material::new(
            "mat_bitumen".to_string(),
            "Bitumen / Abdichtung".to_string(),
            "SOLID".to_string(),
            0x333333,
            "Continuous".to_string(),
        )
    };

    let masonry_wall = WallStyle {
        style: Style {
            id: "style_masonry".to_string(),
            name: "Wand Mauerwerk 24cm".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![Layer {
            material_id: masonry.id.clone(),
            thickness: LayerValue::Fixed(0.24),
            function: LayerFunction::Structural,
            axis_offset: LayerValue::Fixed(-0.12),
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            role_tag: Some("Tragschale".to_string()),
            layer_id: Uuid::new_v4(),
        }],
        default_profile: None,
    };

    let masonry_partition = WallStyle {
        style: Style {
            id: "style_masonry_115".to_string(),
            name: "Nichttragende Trennwand 11.5cm".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![Layer {
            material_id: masonry.id.clone(),
            thickness: LayerValue::Fixed(0.115),
            function: LayerFunction::Finish,
            axis_offset: LayerValue::Fixed(-0.0575),
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            role_tag: Some("Trennwand".to_string()),
            layer_id: Uuid::new_v4(),
        }],
        default_profile: None,
    };

    let concrete_wall = WallStyle {
        style: Style {
            id: "style_concrete_20".to_string(),
            name: "Wand Stahlbeton 20cm".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![Layer {
            material_id: concrete.id.clone(),
            thickness: LayerValue::Fixed(0.20),
            function: LayerFunction::Structural,
            axis_offset: LayerValue::Fixed(-0.10),
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: Some("AR-CONC".to_string()),
            role_tag: Some("Tragschale".to_string()),
            layer_id: Uuid::new_v4(),
        }],
        default_profile: None,
    };

    let concrete_wall_25 = WallStyle {
        style: Style {
            id: "style_concrete_25".to_string(),
            name: "Wand Stahlbeton 25cm".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![Layer {
            material_id: concrete.id.clone(),
            thickness: LayerValue::Fixed(0.25),
            function: LayerFunction::Structural,
            axis_offset: LayerValue::Fixed(-0.125),
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: Some("AR-CONC".to_string()),
            role_tag: Some("Tragschale".to_string()),
            layer_id: Uuid::new_v4(),
        }],
        default_profile: None,
    };

    let drywall_10 = WallStyle {
        style: Style {
            id: "style_drywall_10".to_string(),
            name: "Trockenbauwand 10cm".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![
            Layer {
                material_id: drywall.id.clone(),
                thickness: LayerValue::Fixed(0.0125),
                function: LayerFunction::Finish,
                axis_offset: LayerValue::Fixed(-0.05),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: Some("Beplankung".to_string()),
                layer_id: Uuid::new_v4(),
            },
            Layer {
                material_id: insulation.id.clone(),
                thickness: LayerValue::Fixed(0.075),
                function: LayerFunction::Insulation,
                axis_offset: LayerValue::Fixed(-0.0375),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: Some("ANSI37".to_string()),
                role_tag: Some("Daemmung / Staender".to_string()),
                layer_id: Uuid::new_v4(),
            },
            Layer {
                material_id: drywall.id.clone(),
                thickness: LayerValue::Fixed(0.0125),
                function: LayerFunction::Finish,
                axis_offset: LayerValue::Fixed(0.0375),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: Some("Beplankung".to_string()),
                layer_id: Uuid::new_v4(),
            },
        ],
        default_profile: None,
    };

    let insulated_wall = WallStyle {
        style: Style {
            id: "style_insulated_ext".to_string(),
            name: "Aussenwand gedaemmt".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![
            Layer {
                material_id: plaster.id.clone(),
                thickness: LayerValue::Fixed(0.015),
                function: LayerFunction::Finish,
                axis_offset: LayerValue::Fixed(-0.1725),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: Some("Innenputz".to_string()),
                layer_id: Uuid::new_v4(),
            },
            Layer {
                material_id: masonry.id.clone(),
                thickness: LayerValue::Fixed(0.175),
                function: LayerFunction::Structural,
                axis_offset: LayerValue::Fixed(-0.1575),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: Some("Tragschale".to_string()),
                layer_id: Uuid::new_v4(),
            },
            Layer {
                material_id: insulation.id.clone(),
                thickness: LayerValue::Fixed(0.14),
                function: LayerFunction::Insulation,
                axis_offset: LayerValue::Fixed(0.0175),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: Some("ANSI37".to_string()),
                role_tag: Some("Daemmschicht".to_string()),
                layer_id: Uuid::new_v4(),
            },
            Layer {
                material_id: plaster.id.clone(),
                thickness: LayerValue::Fixed(0.015),
                function: LayerFunction::Finish,
                axis_offset: LayerValue::Fixed(0.1575),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: Some("A-WALL-FINISH".to_string()),
                hatch_override: None,
                role_tag: Some("Aussenputz".to_string()),
                layer_id: Uuid::new_v4(),
            },
        ],
        default_profile: None,
    };

    let cavity_brick_wall = WallStyle {
        style: Style {
            id: "style_cavity_brick".to_string(),
            name: "Zweischalige Verblendwand 42cm".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![
            Layer {
                material_id: plaster.id.clone(),
                thickness: LayerValue::Fixed(0.015),
                function: LayerFunction::Finish,
                axis_offset: LayerValue::Fixed(-0.21),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: Some("Innenputz".to_string()),
                layer_id: Uuid::new_v4(),
            },
            Layer {
                material_id: masonry.id.clone(),
                thickness: LayerValue::Fixed(0.175),
                function: LayerFunction::Structural,
                axis_offset: LayerValue::Fixed(-0.195),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: Some("Hintermauerwerk".to_string()),
                layer_id: Uuid::new_v4(),
            },
            Layer {
                material_id: insulation.id.clone(),
                thickness: LayerValue::Fixed(0.12),
                function: LayerFunction::Insulation,
                axis_offset: LayerValue::Fixed(-0.02),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: Some("ANSI37".to_string()),
                role_tag: Some("Kerndaemmung".to_string()),
                layer_id: Uuid::new_v4(),
            },
            Layer {
                material_id: air.id.clone(),
                thickness: LayerValue::Fixed(0.02),
                function: LayerFunction::Insulation,
                axis_offset: LayerValue::Fixed(0.10),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: Some("Luftschicht".to_string()),
                layer_id: Uuid::new_v4(),
            },
            Layer {
                material_id: clinker.id.clone(),
                thickness: LayerValue::Fixed(0.09),
                function: LayerFunction::Finish,
                axis_offset: LayerValue::Fixed(0.12),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: Some("A-WALL-CLINKER".to_string()),
                hatch_override: Some("ANSI31".to_string()),
                role_tag: Some("Verblender".to_string()),
                layer_id: Uuid::new_v4(),
            },
        ],
        default_profile: None,
    };

    let timber_wall = WallStyle {
        style: Style {
            id: "style_timber_exterior".to_string(),
            name: "Holzstaenderwand 28cm".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: None,
        },
        layers: vec![
            Layer {
                material_id: drywall.id.clone(),
                thickness: LayerValue::Fixed(0.015),
                function: LayerFunction::Finish,
                axis_offset: LayerValue::Fixed(-0.14),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: Some("Innenbeplankung".to_string()),
                layer_id: Uuid::new_v4(),
            },
            Layer {
                material_id: air.id.clone(),
                thickness: LayerValue::Fixed(0.06),
                function: LayerFunction::Insulation,
                axis_offset: LayerValue::Fixed(-0.125),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: Some("Installationsebene".to_string()),
                layer_id: Uuid::new_v4(),
            },
            Layer {
                material_id: insulation.id.clone(),
                thickness: LayerValue::Fixed(0.16),
                function: LayerFunction::Structural,
                axis_offset: LayerValue::Fixed(-0.065),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: Some("ANSI37".to_string()),
                role_tag: Some("Holzstaender / Daemmung".to_string()),
                layer_id: Uuid::new_v4(),
            },
            Layer {
                material_id: wood.id.clone(),
                thickness: LayerValue::Fixed(0.045),
                function: LayerFunction::Finish,
                axis_offset: LayerValue::Fixed(0.095),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: Some("A-WALL-WOOD".to_string()),
                hatch_override: Some("ANSI31".to_string()),
                role_tag: Some("Holzverschalung".to_string()),
                layer_id: Uuid::new_v4(),
            },
        ],
        default_profile: None,
    };

    // Derived style demonstrating style-manager inheritance: shares the
    // insulated exterior wall's layer buildup (empty `layers`, resolved via
    // `effective_layers`) but with a distinct name/id for a different
    // context (e.g. a garden-facing facade using the same buildup).
    let insulated_wall_variant = WallStyle {
        style: Style {
            id: "style_insulated_ext_garden".to_string(),
            name: "Aussenwand gedaemmt (Gartenseite)".to_string(),
            category: "Wall".to_string(),
            origin: Default::default(),
            parent: Some(insulated_wall.style.id.clone()),
        },
        layers: vec![],
        default_profile: None,
    };

    StyleLibrary {
        materials: vec![
            masonry, concrete, insulation, plaster, wood, drywall, steel, glass, clinker, air,
            screed, tile, bitumen,
        ],
        wall_styles: vec![
            masonry_wall,
            masonry_partition,
            concrete_wall,
            concrete_wall_25,
            drywall_10,
            insulated_wall,
            insulated_wall_variant,
            cavity_brick_wall,
            timber_wall,
        ],
        opening_styles: crate::modules::aec::engine::opening_style::seed_opening_styles(),
        slab_styles: seed_slab_styles(),
        slab_structural_styles: seed_slab_structural_styles(),
        floor_finish_styles: seed_floor_finish_styles(),
        profiles: vec![
            SectionProfile::new(
                "section_frame_70_80",
                "Zarge 70/80",
                SectionShape::Rectangle {
                    face: 0.07,
                    depth: 0.08,
                },
            ),
            SectionProfile::new(
                "section_round_40",
                "Rund 40",
                SectionShape::Circle { diameter: 0.04 },
            ),
            SectionProfile::new(
                "section_triangle_60_50",
                "Blechdreieck",
                SectionShape::Triangle {
                    base: 0.06,
                    height: 0.05,
                },
            ),
        ],
    }
}

/// Builds the default standard structural slab styles library.
pub fn seed_slab_structural_styles() -> Vec<SlabStructuralStyle> {
    vec![
        // 1. Reinforced Concrete Slab 20cm
        SlabStructuralStyle {
            style: Style {
                id: "struct_slab_concrete_20".to_string(),
                name: "Stahlbetondecke 20cm".to_string(),
                category: "SlabStructural".to_string(),
                origin: Default::default(),
                parent: None,
            },
            layers: vec![SlabStyleLayer {
                material_id: "mat_concrete".to_string(),
                thickness: LayerValue::Fixed(0.20),
                function: LayerFunction::Structural,
                vertical_offset: LayerValue::Fixed(0.0),
                layer_override: Some("A-FLOR-STRC".to_string()),
                hatch_override: Some("AR-CONC".to_string()),
                role_tag: Some("Stahlbetonplatte".to_string()),
                layer_id: Uuid::new_v4(),
            }],
            default_profile: None,
        },
        // 2. Reinforced Concrete Slab 25cm
        SlabStructuralStyle {
            style: Style {
                id: "struct_slab_concrete_25".to_string(),
                name: "Stahlbetondecke 25cm".to_string(),
                category: "SlabStructural".to_string(),
                origin: Default::default(),
                parent: None,
            },
            layers: vec![SlabStyleLayer {
                material_id: "mat_concrete".to_string(),
                thickness: LayerValue::Fixed(0.25),
                function: LayerFunction::Structural,
                vertical_offset: LayerValue::Fixed(0.0),
                layer_override: Some("A-FLOR-STRC".to_string()),
                hatch_override: Some("AR-CONC".to_string()),
                role_tag: Some("Stahlbetonplatte".to_string()),
                layer_id: Uuid::new_v4(),
            }],
            default_profile: None,
        },
        // 3. Reinforced Concrete Slab 30cm
        SlabStructuralStyle {
            style: Style {
                id: "struct_slab_concrete_30".to_string(),
                name: "Stahlbetondecke 30cm".to_string(),
                category: "SlabStructural".to_string(),
                origin: Default::default(),
                parent: None,
            },
            layers: vec![SlabStyleLayer {
                material_id: "mat_concrete".to_string(),
                thickness: LayerValue::Fixed(0.30),
                function: LayerFunction::Structural,
                vertical_offset: LayerValue::Fixed(0.0),
                layer_override: Some("A-FLOR-STRC".to_string()),
                hatch_override: Some("AR-CONC".to_string()),
                role_tag: Some("Stahlbetonplatte".to_string()),
                layer_id: Uuid::new_v4(),
            }],
            default_profile: None,
        },
        // 4. Timber Beam Deck 24cm
        SlabStructuralStyle {
            style: Style {
                id: "struct_slab_timber_24".to_string(),
                name: "Holzbalkendecke 24cm".to_string(),
                category: "SlabStructural".to_string(),
                origin: Default::default(),
                parent: None,
            },
            layers: vec![SlabStyleLayer {
                material_id: "mat_wood".to_string(),
                thickness: LayerValue::Fixed(0.24),
                function: LayerFunction::Structural,
                vertical_offset: LayerValue::Fixed(0.0),
                layer_override: Some("A-FLOR-STRC".to_string()),
                hatch_override: Some("ANSI31".to_string()),
                role_tag: Some("Holzbalkenlage".to_string()),
                layer_id: Uuid::new_v4(),
            }],
            default_profile: None,
        },
    ]
}

/// Builds the default standard floor finish styles library.
pub fn seed_floor_finish_styles() -> Vec<FloorFinishStyle> {
    vec![
        // 1. Standard Parkett 80mm
        FloorFinishStyle {
            style: Style {
                id: "finish_floor_parquet_80".to_string(),
                name: "Standard Parkett (80mm)".to_string(),
                category: "FloorFinish".to_string(),
                origin: Default::default(),
                parent: None,
            },
            layers: vec![
                SlabStyleLayer {
                    material_id: "mat_wood".to_string(),
                    thickness: LayerValue::Fixed(0.015),
                    function: LayerFunction::Finish,
                    vertical_offset: LayerValue::Fixed(0.0),
                    layer_override: Some("A-FLOR-FINI".to_string()),
                    hatch_override: Some("ANSI31".to_string()),
                    role_tag: Some("Parkett".to_string()),
                    layer_id: Uuid::new_v4(),
                },
                SlabStyleLayer {
                    material_id: "mat_screed".to_string(),
                    thickness: LayerValue::Fixed(0.045),
                    function: LayerFunction::Other("Screed".to_string()),
                    vertical_offset: LayerValue::Fixed(0.0),
                    layer_override: Some("A-FLOR-SCREED".to_string()),
                    hatch_override: Some("DOTS".to_string()),
                    role_tag: Some("Heizestrich".to_string()),
                    layer_id: Uuid::new_v4(),
                },
                SlabStyleLayer {
                    material_id: "mat_insulation".to_string(),
                    thickness: LayerValue::Fixed(0.020),
                    function: LayerFunction::Insulation,
                    vertical_offset: LayerValue::Fixed(0.0),
                    layer_override: Some("A-FLOR-INSU".to_string()),
                    hatch_override: Some("ANSI37".to_string()),
                    role_tag: Some("Trittschalldaemmung".to_string()),
                    layer_id: Uuid::new_v4(),
                },
            ],
            default_profile: None,
        },
        // 2. Fliesenaufbau Nass 80mm
        FloorFinishStyle {
            style: Style {
                id: "finish_floor_tiles_80".to_string(),
                name: "Fliesenaufbau Nass (80mm)".to_string(),
                category: "FloorFinish".to_string(),
                origin: Default::default(),
                parent: None,
            },
            layers: vec![
                SlabStyleLayer {
                    material_id: "mat_tile".to_string(),
                    thickness: LayerValue::Fixed(0.015),
                    function: LayerFunction::Finish,
                    vertical_offset: LayerValue::Fixed(0.0),
                    layer_override: Some("A-FLOR-FINI".to_string()),
                    hatch_override: Some("SQUARE".to_string()),
                    role_tag: Some("Fliesenbelag".to_string()),
                    layer_id: Uuid::new_v4(),
                },
                SlabStyleLayer {
                    material_id: "mat_screed".to_string(),
                    thickness: LayerValue::Fixed(0.045),
                    function: LayerFunction::Other("Screed".to_string()),
                    vertical_offset: LayerValue::Fixed(0.0),
                    layer_override: Some("A-FLOR-SCREED".to_string()),
                    hatch_override: Some("DOTS".to_string()),
                    role_tag: Some("Zementestrich".to_string()),
                    layer_id: Uuid::new_v4(),
                },
                SlabStyleLayer {
                    material_id: "mat_insulation".to_string(),
                    thickness: LayerValue::Fixed(0.020),
                    function: LayerFunction::Insulation,
                    vertical_offset: LayerValue::Fixed(0.0),
                    layer_override: Some("A-FLOR-INSU".to_string()),
                    hatch_override: Some("ANSI37".to_string()),
                    role_tag: Some("Trittschalldaemmung".to_string()),
                    layer_id: Uuid::new_v4(),
                },
            ],
            default_profile: None,
        },
        // 3. Verbundestrich 50mm
        FloorFinishStyle {
            style: Style {
                id: "finish_floor_screed_50".to_string(),
                name: "Verbundestrich (50mm)".to_string(),
                category: "FloorFinish".to_string(),
                origin: Default::default(),
                parent: None,
            },
            layers: vec![
                SlabStyleLayer {
                    material_id: "mat_screed".to_string(),
                    thickness: LayerValue::Fixed(0.050),
                    function: LayerFunction::Finish,
                    vertical_offset: LayerValue::Fixed(0.0),
                    layer_override: Some("A-FLOR-SCREED".to_string()),
                    hatch_override: Some("DOTS".to_string()),
                    role_tag: Some("Industrieestrich".to_string()),
                    layer_id: Uuid::new_v4(),
                },
            ],
            default_profile: None,
        },
        // 4. Teppichboden 70mm
        FloorFinishStyle {
            style: Style {
                id: "finish_floor_carpet_70".to_string(),
                name: "Teppichboden (70mm)".to_string(),
                category: "FloorFinish".to_string(),
                origin: Default::default(),
                parent: None,
            },
            layers: vec![
                SlabStyleLayer {
                    material_id: "mat_plaster".to_string(),
                    thickness: LayerValue::Fixed(0.010),
                    function: LayerFunction::Finish,
                    vertical_offset: LayerValue::Fixed(0.0),
                    layer_override: Some("A-FLOR-FINI".to_string()),
                    hatch_override: Some("DOTS".to_string()),
                    role_tag: Some("Teppich".to_string()),
                    layer_id: Uuid::new_v4(),
                },
                SlabStyleLayer {
                    material_id: "mat_screed".to_string(),
                    thickness: LayerValue::Fixed(0.040),
                    function: LayerFunction::Other("Screed".to_string()),
                    vertical_offset: LayerValue::Fixed(0.0),
                    layer_override: Some("A-FLOR-SCREED".to_string()),
                    hatch_override: Some("DOTS".to_string()),
                    role_tag: Some("Estrich".to_string()),
                    layer_id: Uuid::new_v4(),
                },
                SlabStyleLayer {
                    material_id: "mat_insulation".to_string(),
                    thickness: LayerValue::Fixed(0.020),
                    function: LayerFunction::Insulation,
                    vertical_offset: LayerValue::Fixed(0.0),
                    layer_override: Some("A-FLOR-INSU".to_string()),
                    hatch_override: Some("ANSI37".to_string()),
                    role_tag: Some("Trittschall".to_string()),
                    layer_id: Uuid::new_v4(),
                },
            ],
            default_profile: None,
        },
        // 5. Balkonbelag Gefälle 120mm
        FloorFinishStyle {
            style: Style {
                id: "finish_floor_balcony_120".to_string(),
                name: "Balkonbelag Gefälle (120mm)".to_string(),
                category: "FloorFinish".to_string(),
                origin: Default::default(),
                parent: None,
            },
            layers: vec![
                SlabStyleLayer {
                    material_id: "mat_tile".to_string(),
                    thickness: LayerValue::Fixed(0.020),
                    function: LayerFunction::Finish,
                    vertical_offset: LayerValue::Fixed(0.0),
                    layer_override: Some("A-FLOR-FINI".to_string()),
                    hatch_override: Some("SQUARE".to_string()),
                    role_tag: Some("Plattenbelag".to_string()),
                    layer_id: Uuid::new_v4(),
                },
                SlabStyleLayer {
                    material_id: "mat_bitumen".to_string(),
                    thickness: LayerValue::Fixed(0.010),
                    function: LayerFunction::Other("Sealing".to_string()),
                    vertical_offset: LayerValue::Fixed(0.0),
                    layer_override: Some("A-FLOR-SEAL".to_string()),
                    hatch_override: Some("SOLID".to_string()),
                    role_tag: Some("Abdichtung".to_string()),
                    layer_id: Uuid::new_v4(),
                },
                SlabStyleLayer {
                    material_id: "mat_insulation".to_string(),
                    thickness: LayerValue::Fixed(0.090),
                    function: LayerFunction::Insulation,
                    vertical_offset: LayerValue::Fixed(0.0),
                    layer_override: Some("A-FLOR-INSU".to_string()),
                    hatch_override: Some("ANSI37".to_string()),
                    role_tag: Some("Gefaelledaemmung".to_string()),
                    layer_id: Uuid::new_v4(),
                },
            ],
            default_profile: None,
        },
    ]
}

/// Builds the default standard slab styles library.
pub fn seed_slab_styles() -> Vec<SlabStyle> {
    vec![
        // 1. Reinforced Concrete Slab 20cm
        SlabStyle {
            style: Style {
                id: "style_slab_concrete_20".to_string(),
                name: "Stahlbetondecke 20cm".to_string(),
                category: "Slab".to_string(),
                origin: Default::default(),
                parent: None,
            },
            structural_style_id: Some("struct_slab_concrete_20".to_string()),
            default_finish_style_id: None,
            layers: Vec::new(),
            default_profile: None,
        },
        // 2. Reinforced Concrete Slab 25cm
        SlabStyle {
            style: Style {
                id: "style_slab_concrete_25".to_string(),
                name: "Stahlbetondecke 25cm".to_string(),
                category: "Slab".to_string(),
                origin: Default::default(),
                parent: None,
            },
            structural_style_id: Some("struct_slab_concrete_25".to_string()),
            default_finish_style_id: None,
            layers: Vec::new(),
            default_profile: None,
        },
        // 3. Reinforced Concrete Slab 30cm
        SlabStyle {
            style: Style {
                id: "style_slab_concrete_30".to_string(),
                name: "Stahlbetondecke 30cm".to_string(),
                category: "Slab".to_string(),
                origin: Default::default(),
                parent: None,
            },
            structural_style_id: Some("struct_slab_concrete_30".to_string()),
            default_finish_style_id: None,
            layers: Vec::new(),
            default_profile: None,
        },
        // 4. Geschossdecke EG mit Fliesen & Estrich 28cm
        SlabStyle {
            style: Style {
                id: "style_slab_floor_eg_34".to_string(),
                name: "Geschossdecke EG 28cm (Fliesen)".to_string(),
                category: "Slab".to_string(),
                origin: Default::default(),
                parent: None,
            },
            structural_style_id: Some("struct_slab_concrete_20".to_string()),
            default_finish_style_id: Some("finish_floor_tiles_80".to_string()),
            layers: Vec::new(),
            default_profile: None,
        },
        // 5. Geschossdecke OG 28cm mit Parkett & Trittschall
        SlabStyle {
            style: Style {
                id: "style_slab_floor_og_32".to_string(),
                name: "Geschossdecke OG 28cm (Parkett)".to_string(),
                category: "Slab".to_string(),
                origin: Default::default(),
                parent: None,
            },
            structural_style_id: Some("struct_slab_concrete_20".to_string()),
            default_finish_style_id: Some("finish_floor_parquet_80".to_string()),
            layers: Vec::new(),
            default_profile: None,
        },
        // 6. Holzbalkendecke 24cm
        SlabStyle {
            style: Style {
                id: "style_slab_timber_24".to_string(),
                name: "Holzbalkendecke 24cm".to_string(),
                category: "Slab".to_string(),
                origin: Default::default(),
                parent: None,
            },
            structural_style_id: Some("struct_slab_timber_24".to_string()),
            default_finish_style_id: None,
            layers: Vec::new(),
            default_profile: None,
        },
        // 7. Flachdach warm gedaemmt 32cm
        SlabStyle {
            style: Style {
                id: "style_slab_flat_roof_40".to_string(),
                name: "Flachdach warm gedaemmt 32cm".to_string(),
                category: "Slab".to_string(),
                origin: Default::default(),
                parent: None,
            },
            structural_style_id: Some("struct_slab_concrete_20".to_string()),
            default_finish_style_id: Some("finish_floor_balcony_120".to_string()),
            layers: Vec::new(),
            default_profile: None,
        },
        // 8. Bodenplatte Stahlbeton 30cm
        SlabStyle {
            style: Style {
                id: "style_slab_foundation_30".to_string(),
                name: "Bodenplatte Stahlbeton 30cm".to_string(),
                category: "Slab".to_string(),
                origin: Default::default(),
                parent: None,
            },
            structural_style_id: Some("struct_slab_concrete_30".to_string()),
            default_finish_style_id: None,
            layers: Vec::new(),
            default_profile: None,
        },
        // 9. Bodenplatte gedaemmt 30cm
        SlabStyle {
            style: Style {
                id: "style_slab_foundation_insulated_40".to_string(),
                name: "Bodenplatte gedaemmt mit Estrich 30cm".to_string(),
                category: "Slab".to_string(),
                origin: Default::default(),
                parent: None,
            },
            structural_style_id: Some("struct_slab_concrete_25".to_string()),
            default_finish_style_id: Some("finish_floor_screed_50".to_string()),
            layers: Vec::new(),
            default_profile: None,
        },
        // 10. Balkonplatte Stahlbeton 32cm
        SlabStyle {
            style: Style {
                id: "style_slab_balcony_22".to_string(),
                name: "Balkonplatte Stahlbeton 32cm".to_string(),
                category: "Slab".to_string(),
                origin: Default::default(),
                parent: None,
            },
            structural_style_id: Some("struct_slab_concrete_20".to_string()),
            default_finish_style_id: Some("finish_floor_balcony_120".to_string()),
            layers: Vec::new(),
            default_profile: None,
        },
    ]
}

