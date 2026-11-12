use super::*;
use crate::modules::aec::engine::opening_shape::OpeningShape;
use crate::modules::aec::engine::opening_style::LeafOperation;
use crate::modules::aec::engine::openings::{OpeningKind, OpeningReferenceSide};
use crate::modules::aec::engine::views::model_view::{DetailPolicy, ViewRevisions};
use crate::modules::aec::engine::views::scene_collector::{
    CollectedOpening, CollectedSlab, CollectedSlabLayer, CollectedWall,
    CollectedWallJoinData, CollectedWallLayer,
};
use crate::modules::aec::engine::views::viewpoint::{PlanRange, VerticalRange, ViewAxes, ViewOrigin};
use crate::modules::aec::engine::views::{
    ComponentId, ElementId, ElementRef, Metres, ModelId, ModelViewId, NonNegativeMetres, Point3,
    Revision, ScaleDenominator, ViewProfileId, ViewpointId,
};
use crate::modules::aec::engine::wall::WallJustification;
use crate::modules::aec::engine::plan_view::PlanPhase;

#[test]
fn test_floorplan_derivation_with_wall_and_slab() {
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();

    let viewpoint = Viewpoint {
        id: vp_id,
        name: "Ground Floor".into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.8).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(2.8).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    let model_view = ModelView {
        id: mv_id,
        name: "1:50 Floor Plan".into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
        revisions: ViewRevisions::default(),
    };

    let wall = CollectedWall {
        handle: acadrust::Handle::from(100u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "WallStyle_Exterior".into(),
        height: 2.8,
        base_z: 0.0,
        top_z: 2.8,
        axis_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
        layers: vec![CollectedWallLayer {
            material: "Masonry".into(),
            thickness: 0.24,
            function: "Structural".into(),
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: Some("ANSI31".into()),
            layer_id: uuid::Uuid::new_v4(),
            component_id: ComponentId::new(),
            is_structural: true,
        }],
        total_thickness: 0.24,
        justification: WallJustification::Center,
        openings: Vec::new(),
        phase: PlanPhase::New,
    };

    let slab = CollectedSlab {
        handle: acadrust::Handle::from(200u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "Slab_20".into(),
        base_z: 0.0,
        total_thickness: 0.20,
        polygon: vec![
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 0.0),
            Point2::new(10.0, 8.0),
            Point2::new(0.0, 8.0),
        ],
        layers: Vec::new(),
        justification: crate::modules::aec::engine::slab::SlabJustification::Top,
        openings: Vec::new(),
        phase: PlanPhase::New,
    };

    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);
    collector.slabs.push(slab);

    let primitives = derive_floorplan_view(
        &viewpoint,
        &model_view,
        &collector,
        &FloorPlanEngineOptions::default(),
    )
    .expect("floor plan derived successfully");

    assert!(!primitives.is_empty(), "primitives should not be empty");
    let has_cut_fill = primitives.iter().any(|p| p.role == PrimitiveRole::CutFill);
    let has_cut_boundary = primitives.iter().any(|p| p.role == PrimitiveRole::CutBoundary);
    assert!(has_cut_fill, "should contain cut fill with hatch");
    assert!(has_cut_boundary, "should contain cut boundary");
}

#[test]
fn test_floorplan_hatchless_suppresses_cut_fill_and_retains_masks() {
    use crate::modules::aec::engine::plan_view::{PlanningStage, ViewType};
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();

    let viewpoint = Viewpoint {
        id: vp_id,
        name: "Ground Floor Hatchless".into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(0.0, 0.0, 1.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.8).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    let model_view = ModelView {
        id: mv_id,
        name: "1:50 Hatchless Floor Plan".into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
        revisions: ViewRevisions::default(),
    };

    let wall = CollectedWall {
        handle: acadrust::Handle::from(100u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "Wall_24".into(),
        height: 2.8,
        base_z: 0.0,
        top_z: 2.8,
        axis_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
        layers: vec![CollectedWallLayer {
            material: "Masonry".into(),
            thickness: 0.24,
            function: "Structural".into(),
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: Some("ANSI31".into()),
            layer_id: uuid::Uuid::new_v4(),
            component_id: ComponentId::new(),
            is_structural: true,
        }],
        total_thickness: 0.24,
        justification: WallJustification::Center,
        openings: Vec::new(),
        phase: PlanPhase::New,
    };

    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);

    let mut dc = DisplayConfig::new(
        "Draft".to_string(),
        "Architektur".to_string(),
        PlanningStage::Design,
        ViewType::FloorPlan,
    );
    dc.detail.hatches = false;

    let primitives = derive_floorplan_view(
        &viewpoint,
        &model_view,
        &collector,
        &FloorPlanEngineOptions {
            display_config: Some(dc),
            ..FloorPlanEngineOptions::default()
        },
    )
    .expect("floor plan derived successfully");

    assert!(!primitives.is_empty(), "primitives should not be empty");
    let has_cut_fill = primitives.iter().any(|p| p.role == PrimitiveRole::CutFill);
    let has_cut_boundary = primitives.iter().any(|p| p.role == PrimitiveRole::CutBoundary);
    assert!(!has_cut_fill, "should NOT contain cut fill when hatches are disabled");
    assert!(has_cut_boundary, "should still contain cut boundary");
    assert!(!primitives.iter().any(|p| p.style.layer_name.as_deref() == Some(TRANSIENT_OCCLUDER_LAYER)), "transient occluders must be pruned");
}

fn plan_fixture() -> (Viewpoint, ModelView) {
    let vp_id = ViewpointId::new();
    let viewpoint = Viewpoint {
        id: vp_id,
        name: "Style Plan".into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(0.0, 0.0, 1.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.8).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };
    let model_view = ModelView {
        id: ModelViewId::new(),
        name: "Style Plan 1:50".into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
        revisions: ViewRevisions::default(),
    };
    (viewpoint, model_view)
}

fn style_layer(material: &str, thickness: f64, axis_offset: f64, structural: bool) -> CollectedWallLayer {
    CollectedWallLayer {
        material: material.into(),
        thickness,
        function: if structural { "Structural" } else { "Finish" }.into(),
        axis_offset,
        bottom_offset: 0.0,
        top_offset: 0.0,
        hatch_override: None,
        layer_id: uuid::Uuid::new_v4(),
        component_id: ComponentId::new(),
        is_structural: structural,
    }
}

fn style_wall(layers: Vec<CollectedWallLayer>, axis: [(f64, f64); 2]) -> CollectedWall {
    let total = layers.iter().map(|l| l.thickness).sum();
    CollectedWall {
        handle: acadrust::Handle::from(100u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "Wall_Style".into(),
        height: 2.8,
        base_z: 0.0,
        top_z: 2.8,
        axis_points: vec![Point2::new(axis[0].0, axis[0].1), Point2::new(axis[1].0, axis[1].1)],
        layers,
        total_thickness: total,
        justification: WallJustification::Center,
        openings: Vec::new(),
        phase: PlanPhase::New,
    }
}

#[test]
fn test_wall_join_input_preserves_bulges_overrides_and_l_footprint() {
    use crate::modules::aec::engine::join::{JoinOverrideStyle, JunctionOverride};
    use crate::modules::aec::engine::junction_solver::{self, SolvedJoinKind};
    use crate::modules::aec::engine::join::JUNCTION_TOLERANCE;

    let wall_a = style_wall(
        vec![style_layer("core", 0.2, -0.1, true)],
        [(0.0, 0.0), (10.0, 0.0)],
    );
    let wall_b = style_wall(
        vec![style_layer("core", 0.2, -0.1, true)],
        [(10.0, 0.0), (10.0, 10.0)],
    );
    let override_end = JunctionOverride {
        default_style: Some(JoinOverrideStyle::FarFace),
        ..Default::default()
    };
    let override_span = JunctionOverride {
        default_style: Some(JoinOverrideStyle::NoExtend),
        ..Default::default()
    };
    let join_data = CollectedWallJoinData {
        bulges: vec![0.0, 0.0],
        override_start: None,
        override_end: Some(override_end.clone()),
        override_span: Some(override_span.clone()),
    };
    let input_a = wall_join_input(&wall_a, Some(&join_data));
    let input_b = wall_join_input(&wall_b, None);
    assert_eq!(input_a.bulges, vec![0.0, 0.0]);
    assert_eq!(input_a.override_end, Some(override_end));
    assert_eq!(input_a.override_span, Some(override_span));

    let automatic = junction_solver::solve_network(
        &[wall_join_input(&wall_a, None), input_b.clone()],
        JUNCTION_TOLERANCE,
    );
    let overridden = junction_solver::solve_network(
        &[input_a, input_b],
        JUNCTION_TOLERANCE,
    );
    assert_eq!(overridden.junctions[0].kind, SolvedJoinKind::L);
    assert_ne!(
        automatic.footprints[0][0], overridden.footprints[0][0],
        "the collected endpoint override should reach the L-junction footprint"
    );
}

fn concrete_material() -> crate::modules::aec::engine::material::Material {
    let mut mat = crate::modules::aec::engine::material::Material::new(
        "Concrete".to_string(),
        "Stahlbeton".to_string(),
        "AR-CONC".to_string(),
        0x303030,
        "Continuous".to_string(),
    );
    mat.hatch_scale = 0.5;
    mat.hatch_angle = 45.0;
    mat.hatch_angle_relative = true;
    mat
}

#[test]
fn test_floorplan_material_pattern_scale_angle_reach_cut_fill() {
    let (viewpoint, model_view) = plan_fixture();
    // Wall along +Y: the plan hatch angle is wall-relative (90° + 45°).
    let wall = style_wall(vec![style_layer("Concrete", 0.24, 0.0, true)], [(0.0, 0.0), (0.0, 10.0)]);
    let mut lib = StyleLibrary::default();
    lib.materials.push(concrete_material());
    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);

    let primitives = derive_floorplan_view(
        &viewpoint,
        &model_view,
        &collector,
        &FloorPlanEngineOptions {
            style_library: Some(lib),
            ..FloorPlanEngineOptions::default()
        },
    )
    .unwrap();
    let fill = primitives
        .iter()
        .find(|p| p.role == PrimitiveRole::CutFill)
        .expect("cut fill");
    assert_eq!(fill.style.hatch_pattern.as_deref(), Some("AR-CONC"));
    assert!((fill.style.hatch_scale.unwrap() - 0.5).abs() < 1e-9);
    assert!((fill.style.hatch_angle_rad.unwrap() - 135f64.to_radians()).abs() < 1e-9);
    assert_eq!(fill.style.layer_name.as_deref(), Some("AEC_WALL_HATCH"));
}

#[test]
fn test_floorplan_structural_only_keeps_only_structural_layer() {
    use crate::modules::aec::engine::plan_view::{LayerDetail, PlanningStage, ViewType};
    let (viewpoint, model_view) = plan_fixture();
    let wall = style_wall(
        vec![
            style_layer("Plaster", 0.02, 0.0, false),
            style_layer("Concrete", 0.20, 0.02, true),
            style_layer("Plaster", 0.02, 0.22, false),
        ],
        [(0.0, 0.0), (10.0, 0.0)],
    );
    let core_component = wall.layers[1].component_id;
    let mut lib = StyleLibrary::default();
    lib.materials.push(concrete_material());
    lib.wall_styles
        .push(crate::modules::aec::engine::views::style_resolver::tests::wall_style_for(&wall));
    let mut dc = DisplayConfig::new(
        "Statik".to_string(),
        "Statik".to_string(),
        PlanningStage::Design,
        ViewType::FloorPlan,
    );
    dc.detail.layers = LayerDetail::StructuralOnly;
    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);

    let primitives = derive_floorplan_view(
        &viewpoint,
        &model_view,
        &collector,
        &FloorPlanEngineOptions {
            display_config: Some(dc),
            style_library: Some(lib),
            ..FloorPlanEngineOptions::default()
        },
    )
    .unwrap();
    let fills: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::CutFill).collect();
    assert!(!fills.is_empty());
    for f in &fills {
        assert!(
            f.sources.as_slice().iter().all(|s| s.component_id == Some(core_component)),
            "StructuralOnly must only emit the structural core layer"
        );
    }
    for b in primitives.iter().filter(|p| p.role == PrimitiveRole::CutBoundary) {
        assert!(b.sources.as_slice().iter().all(|s| s.component_id == Some(core_component)));
    }
}

#[test]
fn test_floorplan_demolition_phase_is_dashed_without_display_config() {
    let (viewpoint, model_view) = plan_fixture();
    let mut wall = style_wall(vec![style_layer("Concrete", 0.24, 0.0, true)], [(0.0, 0.0), (10.0, 0.0)]);
    wall.phase = PlanPhase::Demolition;
    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);

    let primitives = derive_floorplan_view(
        &viewpoint,
        &model_view,
        &collector,
        &FloorPlanEngineOptions::default(),
    )
    .unwrap();
    let boundaries: Vec<_> = primitives
        .iter()
        .filter(|p| p.role == PrimitiveRole::CutBoundary)
        .collect();
    assert!(!boundaries.is_empty());
    assert!(boundaries.iter().all(|b| b.style.line_type.as_deref() == Some("DASHED")));
}

#[test]
fn test_floorplan_derivation_is_deterministic_without_options() {
    // No filesystem libraries are consulted: two derivations without options are identical.
    let (viewpoint, model_view) = plan_fixture();
    let wall = style_wall(vec![style_layer("Concrete", 0.24, 0.0, true)], [(0.0, 0.0), (10.0, 0.0)]);
    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);
    let a = derive_floorplan_view(&viewpoint, &model_view, &collector, &FloorPlanEngineOptions::default()).unwrap();
    let b = derive_floorplan_view(&viewpoint, &model_view, &collector, &FloorPlanEngineOptions::default()).unwrap();
    let styles = |v: &Vec<DerivedPrimitive>| v.iter().map(|p| (p.role, p.style.clone())).collect::<Vec<_>>();
    assert_eq!(styles(&a), styles(&b));
}

#[test]
fn test_floorplan_contour_hidden_while_hatch_visible_emits_no_cut_boundary() {
    use crate::modules::aec::engine::display_component::{ComponentRuleSet, WallComponentSlot};
    let (viewpoint, model_view) = plan_fixture();
    let wall = style_wall(vec![style_layer("Concrete", 0.24, 0.0, true)], [(0.0, 0.0), (10.0, 0.0)]);
    let mut lib = StyleLibrary::default();
    lib.materials.push(concrete_material());

    let mut ws = crate::modules::aec::engine::views::style_resolver::tests::wall_style_for(&wall);
    let mut profile = ComponentRuleSet::default();
    profile.visibility.insert(WallComponentSlot::Contour2D.key().to_string(), false);
    profile.visibility.insert(WallComponentSlot::Layers2D.key().to_string(), false);
    profile.visibility.insert(WallComponentSlot::LayerHatch2D.key().to_string(), true);
    ws.default_profile = Some(profile);
    lib.wall_styles.push(ws);

    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);

    let primitives = derive_floorplan_view(
        &viewpoint,
        &model_view,
        &collector,
        &FloorPlanEngineOptions {
            style_library: Some(lib),
            ..FloorPlanEngineOptions::default()
        },
    )
    .unwrap();

    let boundaries: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::CutBoundary).collect();
    assert!(boundaries.is_empty(), "When contour slots are hidden, no CutBoundary primitives must be emitted");
    let fills: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::CutFill).collect();
    assert!(!fills.is_empty(), "CutFill must still be emitted when LayerHatch2D is visible");
}

#[test]
fn test_floorplan_hidden_core_does_not_occlude_but_structural_core_does() {
    use crate::modules::aec::engine::plan_view::{LayerDetail, PlanningStage, ViewType};
    let (viewpoint, model_view) = plan_fixture();

    // 3-layer cut wall: plaster 0.02 | core 0.20 | plaster 0.02. Total thickness = 0.24m.
    // Wall runs along X from x=0.0 to x=4.0, centered at y=0.0 (so y extends from -0.12 to +0.12).
    let wall = style_wall(
        vec![
            style_layer("Plaster", 0.02, 0.0, false),
            style_layer("Concrete", 0.20, 0.02, true),
            style_layer("Plaster", 0.02, 0.22, false),
        ],
        [(0.0, 0.0), (4.0, 0.0)],
    );
    let core_comp = wall.layers[1].component_id;
    let mut lib = StyleLibrary::default();
    lib.materials.push(concrete_material());
    lib.wall_styles
        .push(crate::modules::aec::engine::views::style_resolver::tests::wall_style_for(&wall));

    // Background slab below cut plane (z from -0.2 to 0.0, cut is at z=1.0).
    // It spans x from 1.0 to 3.0, y from -1.0 to 1.0.
    // Its edge runs across the cut wall at y ∈ [-1.0, 1.0]!
    let slab = CollectedSlab {
        handle: acadrust::Handle::from(200u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "Slab_Test".into(),
        base_z: -0.2,
        total_thickness: 0.2,
        polygon: vec![
            Point2::new(1.0, -1.0),
            Point2::new(3.0, -1.0),
            Point2::new(3.0, 1.0),
            Point2::new(1.0, 1.0),
        ],
        layers: vec![CollectedSlabLayer {
            material: "Concrete".into(),
            thickness: 0.2,
            vertical_offset: 0.0,
            function: "Structural".into(),
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
            component_id: ComponentId::new(),
            is_structural: true,
        }],
        justification: crate::modules::aec::engine::slab::SlabJustification::Top,
        openings: Vec::new(),
        phase: PlanPhase::New,
    };

    let mut dc = DisplayConfig::new(
        "DetailOff".to_string(),
        "Architektur".to_string(),
        PlanningStage::Design,
        ViewType::FloorPlan,
    );
    dc.detail.layers = LayerDetail::Off;

    let slab_element = slab.element_ref;
    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);
    collector.slabs.push(slab);

    let primitives = derive_floorplan_view(
        &viewpoint,
        &model_view,
        &collector,
        &FloorPlanEngineOptions {
            display_config: Some(dc.clone()),
            style_library: Some(lib.clone()),
            ..FloorPlanEngineOptions::default()
        },
    )
    .unwrap();

    // LayerDetail::Off hides build-up layers, so the hidden core is not drawn or a mask.
    for p in primitives.iter().filter(|p| p.role == PrimitiveRole::CutBoundary) {
        assert!(
            !p.sources.as_slice().iter().any(|source| source.component_id == Some(core_comp)),
            "Core build-up layer must have no CutBoundary when LayerDetail::Off"
        );
    }

    let background_crosses_core = |primitives: &[DerivedPrimitive]| {
        primitives.iter().any(|primitive| {
            primitive.role == PrimitiveRole::ProjectedEdge
                && primitive.sources.as_slice().iter().any(|source| source.element == slab_element)
                && match &primitive.geometry {
                    Geometry2d::Line { start, end } => {
                        start.y_m < -1e-6 && end.y_m > 1e-6 || end.y_m < -1e-6 && start.y_m > 1e-6
                    }
                    Geometry2d::Polyline { vertices, closed } => {
                        let edge_count = if *closed { vertices.len() } else { vertices.len().saturating_sub(1) };
                        (0..edge_count).any(|index| {
                            let start = vertices[index];
                            let end = vertices[(index + 1) % vertices.len()];
                            start.y_m < -1e-6 && end.y_m > 1e-6 || end.y_m < -1e-6 && start.y_m > 1e-6
                        })
                    }
                    _ => false,
                }
        })
    };
    assert!(background_crosses_core(&primitives), "hidden core must not occlude the slab edge");

    let mut structural_config = dc;
    structural_config.detail.layers = LayerDetail::StructuralOnly;
    let structural_primitives = derive_floorplan_view(
        &viewpoint,
        &model_view,
        &collector,
        &FloorPlanEngineOptions {
            display_config: Some(structural_config),
            style_library: Some(lib),
            ..FloorPlanEngineOptions::default()
        },
    )
    .unwrap();
    assert!(
        structural_primitives.iter().any(|primitive| {
            primitive.role == PrimitiveRole::CutBoundary
                && primitive.sources.as_slice().iter().any(|source| source.component_id == Some(core_comp))
        }),
        "visible structural core must still be drawn"
    );
    assert!(
        !background_crosses_core(&structural_primitives),
        "visible structural core must occlude the background slab edge"
    );
}

#[test]
fn test_localized_floorplan_projection_with_origin_offset() {
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();

    // Building is located at (10.0, 20.0) in world coordinates.
    // Viewpoint origin is set to (10.0, 20.0, 1.0).
    let viewpoint = Viewpoint {
        id: vp_id,
        name: "Local Ground Floor".into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(10.0, 20.0, 1.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.8).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    let model_view = ModelView {
        id: mv_id,
        name: "1:50 Local Floor Plan".into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
        revisions: ViewRevisions::default(),
    };

    let wall = CollectedWall {
        handle: acadrust::Handle::from(101u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "WallStyle_Exterior".into(),
        height: 2.8,
        base_z: 0.0,
        top_z: 2.8,
        // Axis in world coordinates: from (10.0, 20.0) to (20.0, 20.0)
        axis_points: vec![Point2::new(10.0, 20.0), Point2::new(20.0, 20.0)],
        layers: vec![CollectedWallLayer {
            material: "Masonry".into(),
            thickness: 0.30,
            function: "Structural".into(),
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: Some("ANSI31".into()),
            layer_id: uuid::Uuid::new_v4(),
            component_id: ComponentId::new(),
            is_structural: true,
        }],
        total_thickness: 0.30,
        justification: WallJustification::Center,
        openings: vec![
            CollectedOpening {
                handle: acadrust::Handle::from(102u64),
                element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
                xref_source: None,
                host_wall_handle: acadrust::Handle::from(101u64),
                kind: OpeningKind::Door,
                style_id: None,
                division_override: None,
                distance_along_axis: 3.0, // center at world x = 13.0, relative x = 3.0
                width: 1.0,
                height: 2.1,
                sill_height: 0.0,
                spring_height: 0.0,
                shape: OpeningShape::Rectangle,
                hinge: LeafOperation::left(),
                swing_side: Default::default(),
                reference_side: OpeningReferenceSide::Start,
                cross_axis_offset: 0.0,
                depth: None,
                world_center: Point3::new(13.0, 20.0, 1.05),
                axis_span_local: (2.5, 3.5),
            },
        ],
        phase: PlanPhase::New,
    };

    let slab = CollectedSlab {
        handle: acadrust::Handle::from(201u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "Slab_20".into(),
        base_z: 0.0,
        total_thickness: 0.20,
        polygon: vec![
            Point2::new(10.0, 20.0),
            Point2::new(20.0, 20.0),
            Point2::new(20.0, 28.0),
            Point2::new(10.0, 28.0),
        ],
        layers: Vec::new(),
        justification: crate::modules::aec::engine::slab::SlabJustification::Top,
        openings: Vec::new(),
        phase: PlanPhase::New,
    };

    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);
    collector.slabs.push(slab);

    // Verify bounding_box_2d on collector: wall includes thickness margin
    let bbox = collector.bounding_box_2d().expect("bbox calculated");
    assert!(bbox.0.x_m >= 9.7 && bbox.0.x_m <= 10.0, "bbox.0.x_m was {}", bbox.0.x_m);
    assert!(bbox.0.y_m >= 19.7 && bbox.0.y_m <= 20.0, "bbox.0.y_m was {}", bbox.0.y_m);

    let primitives = derive_floorplan_view(
        &viewpoint,
        &model_view,
        &collector,
        &FloorPlanEngineOptions::default(),
    )
    .expect("floor plan derived successfully");

    // Verify that primitives are transformed relative to origin (10.0, 20.0):
    // Lowest X must be near 0.0, NOT 10.0!
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;

    for p in &primitives {
        match &p.geometry {
            Geometry2d::Line { start, end } => {
                min_x = min_x.min(start.x_m).min(end.x_m);
                max_x = max_x.max(start.x_m).max(end.x_m);
            }
            Geometry2d::Polyline { vertices, .. } => {
                for v in vertices {
                    min_x = min_x.min(v.x_m);
                    max_x = max_x.max(v.x_m);
                }
            }
            Geometry2d::Region { outer, .. } => {
                for v in outer {
                    min_x = min_x.min(v.x_m);
                    max_x = max_x.max(v.x_m);
                }
            }
            Geometry2d::CircularArc { center, .. } => {
                min_x = min_x.min(center.x_m);
                max_x = max_x.max(center.x_m);
            }
        }
    }

    assert!(min_x >= -0.2 && min_x <= 0.2, "Minimum relative coordinate must be ~0.0 (got {})", min_x);
    assert!(max_x >= 9.8 && max_x <= 10.2, "Maximum relative coordinate must be ~10.0 (got {})", max_x);

    // Door swing arc center must be relative to origin:
    let door_swing = primitives.iter().find(|p| matches!(p.geometry, Geometry2d::CircularArc { .. })).expect("door swing arc");
    if let Geometry2d::CircularArc { center, radius, .. } = door_swing.geometry {
        assert!((radius.get() - 1.0).abs() < 1e-4);
        // Left jamb was at s = 2.5m along axis from (10, 20), so relative X is 2.5m
        assert!((center.x_m - 2.5).abs() < 1e-3, "Door hinge X must be ~2.5m (got {})", center.x_m);
    }
}

#[test]
fn test_floorplan_opening_lintel_overhead_edges() {
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();

    let viewpoint = Viewpoint {
        id: vp_id,
        name: "Ground Floor".into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.8).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(2.8).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    let model_view = ModelView {
        id: mv_id,
        name: "1:50 Floor Plan".into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
        revisions: ViewRevisions::default(),
    };

    let wall = CollectedWall {
        handle: acadrust::Handle::from(100u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "WallStyle".into(),
        height: 2.8,
        base_z: 0.0,
        top_z: 2.8,
        axis_points: vec![Point2::new(0.0, 0.0), Point2::new(6.0, 0.0)],
        layers: vec![CollectedWallLayer {
            material: "Masonry".into(),
            thickness: 0.24,
            function: "Structural".into(),
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
            component_id: ComponentId::new(),
            is_structural: true,
        }],
        total_thickness: 0.24,
        justification: WallJustification::Center,
        openings: vec![CollectedOpening {
            handle: acadrust::Handle::from(101u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            host_wall_handle: acadrust::Handle::from(100u64),
            kind: OpeningKind::Door,
            style_id: None,
            division_override: None,
            distance_along_axis: 3.0,
            width: 1.0,
            height: 2.1, // lintel head = 2.1 < 2.8
            sill_height: 0.0,
            spring_height: 0.0,
            shape: OpeningShape::Rectangle,
            hinge: LeafOperation::left(),
            swing_side: Default::default(),
            reference_side: OpeningReferenceSide::Start,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(3.0, 0.0, 1.05),
            axis_span_local: (2.5, 3.5),
        }],
        phase: PlanPhase::New,
    };

    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);

    let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &FloorPlanEngineOptions::default()).unwrap();
    let lintel_lines: Vec<_> = prims
        .iter()
        .filter(|p| {
            p.role == PrimitiveRole::HiddenEdge
                && p.style.layer_name.as_deref() == Some("AEC_OVERHEAD")
                && p.sources.as_slice().iter().any(|s| s.feature == Some(FeatureKey::OpeningHead))
        })
        .collect();

    assert_eq!(lintel_lines.len(), 2, "Expected 2 lintel lines (inner and outer) on AEC_OVERHEAD");
    for l in lintel_lines {
        assert_eq!(l.style.line_type.as_deref(), Some("DASHED"));
    }
}

#[test]
fn test_floorplan_full_height_opening_emits_no_lintel() {
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();

    let viewpoint = Viewpoint {
        id: vp_id,
        name: "Ground Floor".into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.8).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(2.8).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    let model_view = ModelView {
        id: mv_id,
        name: "1:50 Floor Plan".into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
        revisions: ViewRevisions::default(),
    };

    // Full-height opening (height = 2.8, equal to wall top_z)
    let wall = CollectedWall {
        handle: acadrust::Handle::from(100u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "WallStyle".into(),
        height: 2.8,
        base_z: 0.0,
        top_z: 2.8,
        axis_points: vec![Point2::new(0.0, 0.0), Point2::new(6.0, 0.0)],
        layers: vec![CollectedWallLayer {
            material: "Masonry".into(),
            thickness: 0.24,
            function: "Structural".into(),
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
            component_id: ComponentId::new(),
            is_structural: true,
        }],
        total_thickness: 0.24,
        justification: WallJustification::Center,
        openings: vec![CollectedOpening {
            handle: acadrust::Handle::from(101u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            host_wall_handle: acadrust::Handle::from(100u64),
            kind: OpeningKind::Door,
            style_id: None,
            division_override: None,
            distance_along_axis: 3.0,
            width: 1.0,
            height: 2.8, // Full-height: op_head == 2.8 == wall.top_z
            sill_height: 0.0,
            spring_height: 0.0,
            shape: OpeningShape::Rectangle,
            hinge: LeafOperation::left(),
            swing_side: Default::default(),
            reference_side: OpeningReferenceSide::Start,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(3.0, 0.0, 1.4),
            axis_span_local: (2.5, 3.5),
        }],
        phase: PlanPhase::New,
    };

    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);

    let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &FloorPlanEngineOptions::default()).unwrap();
    let lintel_lines: Vec<_> = prims
        .iter()
        .filter(|p| {
            p.role == PrimitiveRole::HiddenEdge
                && p.sources.as_slice().iter().any(|s| s.feature == Some(FeatureKey::OpeningHead))
        })
        .collect();

    assert_eq!(lintel_lines.len(), 0, "Full-height opening must emit zero lintel lines");
}

#[test]
fn test_floorplan_show_overhead_edges_false_emits_none() {
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();

    let viewpoint = Viewpoint {
        id: vp_id,
        name: "Ground Floor".into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.8).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(2.8).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    let model_view = ModelView {
        id: mv_id,
        name: "1:50 Floor Plan".into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
        revisions: ViewRevisions::default(),
    };

    let wall = CollectedWall {
        handle: acadrust::Handle::from(100u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "WallStyle".into(),
        height: 2.8,
        base_z: 0.0,
        top_z: 2.8,
        axis_points: vec![Point2::new(0.0, 0.0), Point2::new(6.0, 0.0)],
        layers: vec![CollectedWallLayer {
            material: "Masonry".into(),
            thickness: 0.24,
            function: "Structural".into(),
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
            component_id: ComponentId::new(),
            is_structural: true,
        }],
        total_thickness: 0.24,
        justification: WallJustification::Center,
        openings: vec![CollectedOpening {
            handle: acadrust::Handle::from(101u64),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            host_wall_handle: acadrust::Handle::from(100u64),
            kind: OpeningKind::Door,
            style_id: None,
            division_override: None,
            distance_along_axis: 3.0,
            width: 1.0,
            height: 2.1,
            sill_height: 0.0,
            spring_height: 0.0,
            shape: OpeningShape::Rectangle,
            hinge: LeafOperation::left(),
            swing_side: Default::default(),
            reference_side: OpeningReferenceSide::Start,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(3.0, 0.0, 1.05),
            axis_span_local: (2.5, 3.5),
        }],
        phase: PlanPhase::New,
    };

    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);

    let options = FloorPlanEngineOptions {
        show_overhead_edges: false,
        ..Default::default()
    };
    let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &options).unwrap();
    let overhead_lines: Vec<_> = prims
        .iter()
        .filter(|p| p.style.layer_name.as_deref() == Some("AEC_OVERHEAD"))
        .collect();

    assert_eq!(overhead_lines.len(), 0, "show_overhead_edges=false must emit zero overhead lines");
}

#[test]
fn test_floorplan_overhead_opening_and_slab_above_cut() {
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();

    let viewpoint = Viewpoint {
        id: vp_id,
        name: "Ground Floor".into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.8).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    let model_view = ModelView {
        id: mv_id,
        name: "1:50 Floor Plan".into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
        revisions: ViewRevisions::default(),
    };

    let op_elem = ElementRef::new(ModelId::new(), ElementId::new());
    // High window fully above cut plane (sill = 1.5 > z_cut = 1.0)
    let wall = CollectedWall {
        handle: acadrust::Handle::from(100u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "WallStyle".into(),
        height: 3.0,
        base_z: 0.0,
        top_z: 3.0,
        axis_points: vec![Point2::new(0.0, 0.0), Point2::new(6.0, 0.0)],
        layers: vec![CollectedWallLayer {
            material: "Masonry".into(),
            thickness: 0.24,
            function: "Structural".into(),
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
            component_id: ComponentId::new(),
            is_structural: true,
        }],
        total_thickness: 0.24,
        justification: WallJustification::Center,
        openings: vec![CollectedOpening {
            handle: acadrust::Handle::from(101u64),
            element_ref: op_elem,
            xref_source: None,
            host_wall_handle: acadrust::Handle::from(100u64),
            kind: OpeningKind::Window,
            style_id: None,
            division_override: None,
            distance_along_axis: 3.0,
            width: 1.0,
            height: 0.8,
            sill_height: 1.5, // > z_cut (1.0)
            spring_height: 0.0,
            shape: OpeningShape::Rectangle,
            hinge: LeafOperation::left(),
            swing_side: Default::default(),
            reference_side: OpeningReferenceSide::Start,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(3.0, 0.0, 1.9),
            axis_span_local: (2.5, 3.5),
        }],
        phase: PlanPhase::New,
    };

    // Overhead drop ceiling / slab strictly above cut plane (z = 2.4 > z_cut = 1.0)
    let slab_elem = ElementRef::new(ModelId::new(), ElementId::new());
    let slab = CollectedSlab {
        handle: acadrust::Handle::from(200u64),
        element_ref: slab_elem,
        xref_source: None,
        style_id: "Ceiling".into(),
        base_z: 2.4,
        total_thickness: 0.20,
        polygon: vec![
            Point2::new(1.0, 1.0),
            Point2::new(5.0, 1.0),
            Point2::new(5.0, 3.0),
            Point2::new(1.0, 3.0),
        ],
        layers: Vec::new(),
        justification: crate::modules::aec::engine::slab::SlabJustification::Bottom,
        openings: Vec::new(),
        phase: PlanPhase::New,
    };

    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);
    collector.slabs.push(slab);

    let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &FloorPlanEngineOptions::default()).unwrap();

    // 1. Verify overhead high window jamb lines
    let op_overhead: Vec<_> = prims
        .iter()
        .filter(|p| {
            p.role == PrimitiveRole::HiddenEdge
                && p.style.layer_name.as_deref() == Some("AEC_OVERHEAD")
                && p.sources.as_slice().iter().any(|s| s.element == op_elem)
        })
        .collect();
    assert_eq!(op_overhead.len(), 2, "Expected 2 overhead opening jamb lines crossing the wall");
    for p in op_overhead {
        assert_eq!(p.style.line_type.as_deref(), Some("DASHED"));
    }

    // 2. Verify overhead slab boundary
    let slab_overhead: Vec<_> = prims
        .iter()
        .filter(|p| {
            p.role == PrimitiveRole::HiddenEdge
                && p.style.layer_name.as_deref() == Some("AEC_OVERHEAD")
                && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
        })
        .collect();
    assert!(!slab_overhead.is_empty(), "Expected overhead slab boundary to be emitted");
    for s in slab_overhead {
        assert_eq!(s.style.line_type.as_deref(), Some("DASHED"));
    }
}

#[test]
fn test_floorplan_slab_under_cut_wall_occlusion_and_hidden_lines() {
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();

    let viewpoint = Viewpoint {
        id: vp_id,
        name: "Ground Floor".into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.8).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    let model_view = ModelView {
        id: mv_id,
        name: "1:50 Floor Plan".into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
        revisions: ViewRevisions::default(),
    };

    // Cut wall along y = 0 from x = 0 to 4, thickness 0.24 (y in [-0.12, 0.12])
    let wall = CollectedWall {
        handle: acadrust::Handle::from(100u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "WallStyle".into(),
        height: 3.0,
        base_z: 0.0,
        top_z: 3.0,
        axis_points: vec![Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)],
        layers: vec![CollectedWallLayer {
            material: "Concrete".into(),
            thickness: 0.24,
            function: "Structural".into(),
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
            component_id: ComponentId::new(),
            is_structural: true,
        }],
        total_thickness: 0.24,
        justification: WallJustification::Center,
        openings: Vec::new(),
        phase: PlanPhase::New,
    };

    // Slab below cut plane (base_z = -0.2, top_z = 0.0, thickness = 0.2)
    // Polygon: x in [1, 3], y in [-1, 1].
    // The slab crosses directly through the wall (y in [-0.12, 0.12]).
    // Edges at x = 1 and x = 3 cross from y = -1 to y = 1 across the wall!
    let slab_elem = ElementRef::new(ModelId::new(), ElementId::new());
    let slab = CollectedSlab {
        handle: acadrust::Handle::from(200u64),
        element_ref: slab_elem,
        xref_source: None,
        style_id: "FloorSlab".into(),
        base_z: 0.0,
        total_thickness: 0.20,
        polygon: vec![
            Point2::new(1.0, -1.0),
            Point2::new(3.0, -1.0),
            Point2::new(3.0, 1.0),
            Point2::new(1.0, 1.0),
        ],
        layers: Vec::new(),
        justification: crate::modules::aec::engine::slab::SlabJustification::Top,
        openings: Vec::new(),
        phase: PlanPhase::New,
    };

    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);
    collector.slabs.push(slab);

    // Case A: emit_hidden_lines = false
    let opts_off = FloorPlanEngineOptions {
        emit_hidden_lines: false,
        ..Default::default()
    };
    let prims_off = derive_floorplan_view(&viewpoint, &model_view, &collector, &opts_off).unwrap();
    let hidden_off: Vec<_> = prims_off.iter().filter(|p| p.role == PrimitiveRole::HiddenEdge).collect();
    assert_eq!(hidden_off.len(), 0, "No hidden lines when emit_hidden_lines is false");

    // The slab contour segments across the cut wall should be clipped out of visible projected edges!
    let slab_vis_off: Vec<_> = prims_off
        .iter()
        .filter(|p| p.role == PrimitiveRole::ProjectedEdge && p.sources.as_slice().iter().any(|s| s.element == slab_elem))
        .collect();
    // Since the 2 vertical edges [1, -1]..[1, 1] and [3, -1]..[3, 1] cross through the wall [-0.12, 0.12],
    // they are split into 2 visible segments each (below wall and above wall), plus 2 horizontal edges = 6 visible segments.
    assert_eq!(slab_vis_off.len(), 6, "Expected slab contour to be clipped by cut wall into 6 visible pieces");

    // Case B: emit_hidden_lines = true
    let opts_on = FloorPlanEngineOptions {
        emit_hidden_lines: true,
        ..Default::default()
    };
    let prims_on = derive_floorplan_view(&viewpoint, &model_view, &collector, &opts_on).unwrap();
    let hidden_on: Vec<_> = prims_on
        .iter()
        .filter(|p| {
            p.role == PrimitiveRole::HiddenEdge
                && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
        })
        .collect();
    assert_eq!(hidden_on.len(), 2, "Expected 2 occluded slab contour segments under the cut wall to be emitted dashed");
    for h in hidden_on {
        assert_eq!(h.style.line_type.as_deref(), Some("DASHED"));
        assert_eq!(h.style.layer_name.as_deref(), Some("AEC_HIDDEN"));
    }
}

#[test]
fn test_floorplan_overhead_slab_coinciding_with_cut_wall_suppressed() {
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();

    let viewpoint = Viewpoint {
        id: vp_id,
        name: "Ground Floor".into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.8).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    let model_view = ModelView {
        id: mv_id,
        name: "1:50 Floor Plan".into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
        revisions: ViewRevisions::default(),
    };

    // Cut wall along y = 0 from x = 0 to 4, thickness 0.24 (y in [-0.12, 0.12])
    let wall = CollectedWall {
        handle: acadrust::Handle::from(100u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "WallStyle".into(),
        height: 3.0,
        base_z: 0.0,
        top_z: 3.0,
        axis_points: vec![Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)],
        layers: vec![CollectedWallLayer {
            material: "Concrete".into(),
            thickness: 0.24,
            function: "Structural".into(),
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
            component_id: ComponentId::new(),
            is_structural: true,
        }],
        total_thickness: 0.24,
        justification: WallJustification::Center,
        openings: Vec::new(),
        phase: PlanPhase::New,
    };

    // Overhead ceiling slab at z = 2.5 (above cut plane z = 1.0)
    // Polygon: [0.0, 0.12] -> [4.0, 0.12] -> [4.0, 2.0] -> [0.0, 2.0]
    // Edge 1: [0, 0.12] -> [4, 0.12] coincides exactly with the exterior face of the cut wall!
    // It must be suppressed so dashed overhead lines do NOT draw over the solid wall cut boundary!
    let slab_elem = ElementRef::new(ModelId::new(), ElementId::new());
    let slab = CollectedSlab {
        handle: acadrust::Handle::from(200u64),
        element_ref: slab_elem,
        xref_source: None,
        style_id: "Ceiling".into(),
        base_z: 2.5,
        total_thickness: 0.20,
        polygon: vec![
            Point2::new(0.0, 0.12),
            Point2::new(4.0, 0.12),
            Point2::new(4.0, 2.0),
            Point2::new(0.0, 2.0),
        ],
        layers: Vec::new(),
        justification: crate::modules::aec::engine::slab::SlabJustification::Bottom,
        openings: Vec::new(),
        phase: PlanPhase::New,
    };

    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);
    collector.slabs.push(slab);

    let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &FloorPlanEngineOptions::default()).unwrap();

    let overhead_slab: Vec<_> = prims
        .iter()
        .filter(|p| {
            p.role == PrimitiveRole::HiddenEdge
                && p.style.layer_name.as_deref() == Some("AEC_OVERHEAD")
                && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
        })
        .collect();

    // Edge [0, 0.12] -> [4, 0.12] is suppressed!
    // Edges [4, 0.12]->[4, 2], [4, 2]->[0, 2], [0, 2]->[0, 0.12] are kept (3 edges).
    assert_eq!(overhead_slab.len(), 3, "Coinciding overhead slab edge must be suppressed, remaining 3 edges kept");
    for s in overhead_slab {
        if let Geometry2d::Line { start, end } = s.geometry {
            // Ensure none of the kept lines are the edge along y = 0.12 from 0 to 4
            let is_coinciding = (start.y_m - 0.12).abs() < 1e-4 && (end.y_m - 0.12).abs() < 1e-4;
            assert!(!is_coinciding, "Coinciding edge along y = 0.12 must not be present in overhead lines");
        }
    }
}

#[test]
fn test_cut_opening_lintel_suppressed_when_above_z_max() {
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();

    // Viewpoint with z_range = 0.0 .. 2.0 (z_max = 2.0)
    let viewpoint_low = Viewpoint {
        id: vp_id,
        name: "Ground Floor".into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.0).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(2.0).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    // Viewpoint with z_range = 0.0 .. 3.0 (z_max = 3.0)
    let viewpoint_high = Viewpoint {
        id: vp_id,
        name: "Ground Floor".into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(2.0).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    let model_view = ModelView {
        id: mv_id,
        name: "1:50 Floor Plan".into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
        revisions: ViewRevisions::default(),
    };

    // Door with sill = 0.0, height = 2.1 => op_head = 2.1
    let op_elem = ElementRef::new(ModelId::new(), ElementId::new());
    let wall = CollectedWall {
        handle: acadrust::Handle::from(100u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "WallStyle".into(),
        height: 3.0,
        base_z: 0.0,
        top_z: 3.0,
        axis_points: vec![Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)],
        layers: vec![CollectedWallLayer {
            material: "Concrete".into(),
            thickness: 0.24,
            function: "Structural".into(),
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
            component_id: ComponentId::new(),
            is_structural: true,
        }],
        total_thickness: 0.24,
        justification: WallJustification::Center,
        openings: vec![CollectedOpening {
            handle: acadrust::Handle::from(101u64),
            element_ref: op_elem,
            xref_source: None,
            host_wall_handle: acadrust::Handle::from(100u64),
            kind: OpeningKind::Door,
            style_id: None,
            division_override: None,
            distance_along_axis: 2.0,
            width: 1.0,
            height: 2.1,
            sill_height: 0.0,
            spring_height: 0.0,
            shape: OpeningShape::Rectangle,
            hinge: LeafOperation::left(),
            swing_side: Default::default(),
            reference_side: OpeningReferenceSide::Start,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(2.0, 0.0, 1.05),
            axis_span_local: (1.5, 2.5),
        }],
        phase: PlanPhase::New,
    };

    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);

    // Low view range: z_max = 2.0 < op_head = 2.1 => lintel must NOT be emitted
    let prims_low = derive_floorplan_view(&viewpoint_low, &model_view, &collector, &FloorPlanEngineOptions::default()).unwrap();
    let lintel_low: Vec<_> = prims_low
        .iter()
        .filter(|p| {
            p.role == PrimitiveRole::HiddenEdge
                && p.sources.as_slice().iter().any(|s| s.feature == Some(FeatureKey::OpeningHead))
        })
        .collect();
    assert_eq!(lintel_low.len(), 0, "Door head above z_max must not emit lintel lines");

    // High view range: z_max = 3.0 > op_head = 2.1 => lintel lines MUST be emitted
    let prims_high = derive_floorplan_view(&viewpoint_high, &model_view, &collector, &FloorPlanEngineOptions::default()).unwrap();
    let lintel_high: Vec<_> = prims_high
        .iter()
        .filter(|p| {
            p.role == PrimitiveRole::HiddenEdge
                && p.sources.as_slice().iter().any(|s| s.feature == Some(FeatureKey::OpeningHead))
        })
        .collect();
    assert_eq!(lintel_high.len(), 2, "Door head within z_max must emit 2 lintel lines");
}

#[test]
fn test_floorplan_window_parapet_occludes_slab_and_preserves_door_transparency() {
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();

    let viewpoint = Viewpoint {
        id: vp_id,
        name: "Ground Floor".into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.8).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    let model_view = ModelView {
        id: mv_id,
        name: "1:50 Floor Plan".into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
        revisions: ViewRevisions::default(),
    };

    // Wall from x=0 to x=10 along y=0, thickness 0.24 (y in [-0.12, 0.12]), base_z=0.0, top_z=3.0
    let win_elem = ElementRef::new(ModelId::new(), ElementId::new());
    let door_elem = ElementRef::new(ModelId::new(), ElementId::new());
    let wall = CollectedWall {
        handle: acadrust::Handle::from(100u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "WallStyle".into(),
        height: 3.0,
        base_z: 0.0,
        top_z: 3.0,
        axis_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
        layers: vec![CollectedWallLayer {
            material: "Concrete".into(),
            thickness: 0.24,
            function: "Structural".into(),
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
            component_id: ComponentId::new(),
            is_structural: true,
        }],
        total_thickness: 0.24,
        justification: WallJustification::Center,
        openings: vec![
            // Window at x=2.5, width=1.0 (span 2.0..3.0), sill=0.8, height=1.3
            CollectedOpening {
                handle: acadrust::Handle::from(101u64),
                element_ref: win_elem,
                xref_source: None,
                host_wall_handle: acadrust::Handle::from(100u64),
                kind: OpeningKind::Window,
                style_id: None,
                division_override: None,
                distance_along_axis: 2.5,
                width: 1.0,
                height: 1.3,
                sill_height: 0.8,
                spring_height: 0.0,
                shape: OpeningShape::Rectangle,
                hinge: LeafOperation::left(),
                swing_side: Default::default(),
                reference_side: OpeningReferenceSide::Start,
                cross_axis_offset: 0.0,
                depth: None,
                world_center: Point3::new(2.5, 0.0, 1.45),
                axis_span_local: (2.0, 3.0),
            },
            // Door at x=7.5, width=1.0 (span 7.0..8.0), sill=0.0, height=2.1
            CollectedOpening {
                handle: acadrust::Handle::from(102u64),
                element_ref: door_elem,
                xref_source: None,
                host_wall_handle: acadrust::Handle::from(100u64),
                kind: OpeningKind::Door,
                style_id: None,
                division_override: None,
                distance_along_axis: 7.5,
                width: 1.0,
                height: 2.1,
                sill_height: 0.0,
                spring_height: 0.0,
                shape: OpeningShape::Rectangle,
                hinge: LeafOperation::left(),
                swing_side: Default::default(),
                reference_side: OpeningReferenceSide::Start,
                cross_axis_offset: 0.0,
                depth: None,
                world_center: Point3::new(7.5, 0.0, 1.05),
                axis_span_local: (7.0, 8.0),
            },
        ],
        phase: PlanPhase::New,
    };

    // Below-cut slab from x=1.0 to x=9.0, running from y=-1.0 to y=0.0 (top edge lies along y=0.0 directly on wall axis)
    let slab_elem = ElementRef::new(ModelId::new(), ElementId::new());
    let slab = CollectedSlab {
        handle: acadrust::Handle::from(200u64),
        element_ref: slab_elem,
        xref_source: None,
        style_id: "FloorSlab".into(),
        base_z: -0.20,
        total_thickness: 0.20, // top_z = 0.0
        polygon: vec![
            Point2::new(1.0, -1.0),
            Point2::new(9.0, -1.0),
            Point2::new(9.0, 0.0),
            Point2::new(1.0, 0.0),
        ],
        layers: Vec::new(),
        justification: crate::modules::aec::engine::slab::SlabJustification::Top,
        openings: Vec::new(),
        phase: PlanPhase::New,
    };

    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);
    collector.slabs.push(slab);

    // Case A: emit_hidden_lines = false
    // The slab edge along y=0.0 must be occluded by wall (1..2, 3..7, 8..9) AND by window parapet (2..3).
    // Only the piece inside the door opening (7.0..8.0) must remain visible!
    let opts_off = FloorPlanEngineOptions {
        emit_hidden_lines: false,
        ..Default::default()
    };
    let prims_off = derive_floorplan_view(&viewpoint, &model_view, &collector, &opts_off).unwrap();
    let visible_slab_edges: Vec<_> = prims_off
        .iter()
        .filter(|p| {
            p.role == PrimitiveRole::ProjectedEdge
                && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
        })
        .collect();

    // Check if there is any visible slab edge along y=0.0 in the window gap (x in [2.0, 3.0])
    let win_slab_edge = visible_slab_edges.iter().find(|p| match &p.geometry {
        Geometry2d::Line { start, end } => {
            let mid_x = (start.x_m + end.x_m) * 0.5;
            let mid_y = (start.y_m + end.y_m) * 0.5;
            mid_x >= 2.0 && mid_x <= 3.0 && mid_y.abs() < 1e-4
        }
        _ => false,
    });
    assert!(win_slab_edge.is_none(), "Slab edge inside window parapet gap must be occluded");

    // Check that the slab edge inside the door gap (x in [7.0, 8.0]) is visible!
    let door_slab_edge = visible_slab_edges.iter().find(|p| match &p.geometry {
        Geometry2d::Line { start, end } => {
            let min_x = start.x_m.min(end.x_m);
            let max_x = start.x_m.max(end.x_m);
            let mid_y = (start.y_m + end.y_m) * 0.5;
            (min_x - 7.0).abs() < 1e-4 && (max_x - 8.0).abs() < 1e-4 && mid_y.abs() < 1e-4
        }
        _ => false,
    });
    assert!(door_slab_edge.is_some(), "Slab edge inside door opening must remain transparent and visible");

    // Case B: emit_hidden_lines = true
    // The slab edge under the wall and window parapet (x in [1.0, 7.0]) becomes a dashed line on AEC_HIDDEN!
    let opts_on = FloorPlanEngineOptions {
        emit_hidden_lines: true,
        ..Default::default()
    };
    let prims_on = derive_floorplan_view(&viewpoint, &model_view, &collector, &opts_on).unwrap();
    let hidden_slab_edges: Vec<_> = prims_on
        .iter()
        .filter(|p| {
            p.role == PrimitiveRole::HiddenEdge
                && p.style.layer_name.as_deref() == Some("AEC_HIDDEN")
                && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
        })
        .collect();

    // The occluded portion across [1.0, 7.0] is emitted as hidden dashed line
    let hidden_across_win = hidden_slab_edges.iter().find(|p| match &p.geometry {
        Geometry2d::Line { start, end } => {
            let min_x = start.x_m.min(end.x_m);
            let max_x = start.x_m.max(end.x_m);
            let mid_y = (start.y_m + end.y_m) * 0.5;
            min_x <= 2.0 + 1e-4 && max_x >= 3.0 - 1e-4 && mid_y.abs() < 1e-4
        }
        _ => false,
    });
    assert!(hidden_across_win.is_some(), "Slab edge across wall and window parapet must be emitted dashed");

    // Inside the door opening (x in [7.0, 8.0]), it stays visible solid
    let visible_door_edge_on = prims_on.iter().find(|p| {
        p.role == PrimitiveRole::ProjectedEdge
            && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
            && match &p.geometry {
                Geometry2d::Line { start, end } => {
                    let min_x = start.x_m.min(end.x_m);
                    let max_x = start.x_m.max(end.x_m);
                    let mid_y = (start.y_m + end.y_m) * 0.5;
                    (min_x - 7.0).abs() < 1e-4 && (max_x - 8.0).abs() < 1e-4 && mid_y.abs() < 1e-4
                }
                _ => false,
            }
    });
    assert!(visible_door_edge_on.is_some(), "Slab edge in door opening must stay visible solid when hidden lines on");

    // Case C: include_openings = false (with emit_hidden_lines = false)
    // Even when opening symbols are not rendered, parapet geometry occludes below-cut slab edge!
    let opts_no_openings = FloorPlanEngineOptions {
        include_openings: false,
        emit_hidden_lines: false,
        ..Default::default()
    };
    let prims_no_openings = derive_floorplan_view(&viewpoint, &model_view, &collector, &opts_no_openings).unwrap();
    let visible_slab_edges_no_op: Vec<_> = prims_no_openings
        .iter()
        .filter(|p| {
            p.role == PrimitiveRole::ProjectedEdge
                && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
        })
        .collect();

    // Parapet still occludes slab in window gap (x in [2.0, 3.0])
    let win_slab_edge_no_op = visible_slab_edges_no_op.iter().find(|p| match &p.geometry {
        Geometry2d::Line { start, end } => {
            let mid_x = (start.x_m + end.x_m) * 0.5;
            let mid_y = (start.y_m + end.y_m) * 0.5;
            mid_x >= 2.0 && mid_x <= 3.0 && mid_y.abs() < 1e-4
        }
        _ => false,
    });
    assert!(
        win_slab_edge_no_op.is_none(),
        "Slab edge inside window gap must be occluded even when include_openings is false"
    );

    // Door gap (x in [7.0, 8.0]) remains transparent!
    let door_slab_edge_no_op = visible_slab_edges_no_op.iter().find(|p| match &p.geometry {
        Geometry2d::Line { start, end } => {
            let min_x = start.x_m.min(end.x_m);
            let max_x = start.x_m.max(end.x_m);
            let mid_y = (start.y_m + end.y_m) * 0.5;
            (min_x - 7.0).abs() < 1e-4 && (max_x - 8.0).abs() < 1e-4 && mid_y.abs() < 1e-4
        }
        _ => false,
    });
    assert!(
        door_slab_edge_no_op.is_some(),
        "Slab edge inside door gap must remain visible when include_openings is false"
    );
}

#[test]
fn test_floorplan_hidden_wall_does_not_emit_openings_or_occlude() {
    use crate::modules::aec::engine::views::model_view::{
        GraphicOverride, GraphicPatch, OverrideSelector,
    };
    use crate::modules::aec::engine::views::OverrideId;

    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();

    let viewpoint = Viewpoint {
        id: vp_id,
        name: "Ground Floor".into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.8).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    // Wall from x=0 to x=10 along y=0, thickness 0.24 (y in [-0.12, 0.12]), base_z=0.0, top_z=3.0
    let win_elem = ElementRef::new(ModelId::new(), ElementId::new());
    let door_elem = ElementRef::new(ModelId::new(), ElementId::new());
    let wall_elem = ElementRef::new(ModelId::new(), ElementId::new());
    let wall = CollectedWall {
        handle: acadrust::Handle::from(100u64),
        element_ref: wall_elem,
        xref_source: None,
        style_id: "WallStyle".into(),
        height: 3.0,
        base_z: 0.0,
        top_z: 3.0,
        axis_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
        layers: vec![CollectedWallLayer {
            material: "Concrete".into(),
            thickness: 0.24,
            function: "Structural".into(),
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
            component_id: ComponentId::new(),
            is_structural: true,
        }],
        total_thickness: 0.24,
        justification: WallJustification::Center,
        openings: vec![
            CollectedOpening {
                handle: acadrust::Handle::from(101u64),
                element_ref: win_elem,
                xref_source: None,
                host_wall_handle: acadrust::Handle::from(100u64),
                kind: OpeningKind::Window,
                style_id: None,
                division_override: None,
                distance_along_axis: 2.5,
                width: 1.0,
                height: 1.3,
                sill_height: 0.8,
                spring_height: 0.0,
                shape: OpeningShape::Rectangle,
                hinge: LeafOperation::left(),
                swing_side: Default::default(),
                reference_side: OpeningReferenceSide::Start,
                cross_axis_offset: 0.0,
                depth: None,
                world_center: Point3::new(2.5, 0.0, 1.45),
                axis_span_local: (2.0, 3.0),
            },
            CollectedOpening {
                handle: acadrust::Handle::from(102u64),
                element_ref: door_elem,
                xref_source: None,
                host_wall_handle: acadrust::Handle::from(100u64),
                kind: OpeningKind::Door,
                style_id: None,
                division_override: None,
                distance_along_axis: 7.5,
                width: 1.0,
                height: 2.1,
                sill_height: 0.0,
                spring_height: 0.0,
                shape: OpeningShape::Rectangle,
                hinge: LeafOperation::left(),
                swing_side: Default::default(),
                reference_side: OpeningReferenceSide::Start,
                cross_axis_offset: 0.0,
                depth: None,
                world_center: Point3::new(7.5, 0.0, 1.05),
                axis_span_local: (7.0, 8.0),
            },
        ],
        phase: PlanPhase::New,
    };

    // Below-cut slab from x=1.0 to x=9.0, running from y=-1.0 to y=0.0 (top edge lies along y=0.0 directly on wall axis)
    let slab_elem = ElementRef::new(ModelId::new(), ElementId::new());
    let slab = CollectedSlab {
        handle: acadrust::Handle::from(200u64),
        element_ref: slab_elem,
        xref_source: None,
        style_id: "SlabStyle".into(),
        polygon: vec![
            Point2::new(1.0, -1.0),
            Point2::new(9.0, -1.0),
            Point2::new(9.0, 0.0),
            Point2::new(1.0, 0.0),
        ],
        openings: vec![],
        base_z: -0.2,
        total_thickness: 0.20,
        layers: Vec::new(),
        justification: crate::modules::aec::engine::slab::SlabJustification::Top,
        phase: PlanPhase::New,
    };

    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);
    collector.slabs.push(slab);

    // Hide the host wall via ModelView GraphicOverride (draw = false)
    let model_view = ModelView {
        id: mv_id,
        name: "1:50 Floor Plan".into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: vec![GraphicOverride {
            id: OverrideId::new(),
            priority: 1,
            enabled: true,
            selector: OverrideSelector::Element(wall_elem),
            patch: GraphicPatch {
                draw: Some(false),
                ..Default::default()
            },
        }],
        annotation_ids: Vec::new(),
        update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
        revisions: ViewRevisions::default(),
    };

    let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &FloorPlanEngineOptions::default()).unwrap();

    // 1. No primitives from the hidden wall or its openings must be emitted
    let wall_and_opening_prims: Vec<_> = prims
        .iter()
        .filter(|p| {
            p.sources.as_slice().iter().any(|s| {
                s.element == wall_elem || s.element == win_elem || s.element == door_elem
            })
        })
        .collect();
    assert!(
        wall_and_opening_prims.is_empty(),
        "Hidden wall and its openings must emit zero primitives, but emitted {}",
        wall_and_opening_prims.len()
    );

    // 2. The slab edge along y=0.0 across x ∈ [1.0, 9.0] must NOT be occluded by the hidden wall or its parapet!
    let slab_edges: Vec<_> = prims
        .iter()
        .filter(|p| {
            p.role == PrimitiveRole::ProjectedEdge
                && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
        })
        .collect();

    let top_edge_full = slab_edges.iter().find(|p| match &p.geometry {
        Geometry2d::Line { start, end } => {
            let min_x = start.x_m.min(end.x_m);
            let max_x = start.x_m.max(end.x_m);
            let mid_y = (start.y_m + end.y_m) * 0.5;
            (min_x - 1.0).abs() < 1e-3 && (max_x - 9.0).abs() < 1e-3 && mid_y.abs() < 1e-3
        }
        Geometry2d::Polyline { vertices, closed } => {
            let n = vertices.len();
            let edge_count = if *closed { n } else { n.saturating_sub(1) };
            (0..edge_count).any(|i| {
                let start = vertices[i];
                let end = vertices[(i + 1) % n];
                let min_x = start.x_m.min(end.x_m);
                let max_x = start.x_m.max(end.x_m);
                let mid_y = (start.y_m + end.y_m) * 0.5;
                (min_x - 1.0).abs() < 1e-3 && (max_x - 9.0).abs() < 1e-3 && mid_y.abs() < 1e-3
            })
        }
        _ => false,
    });
    assert!(
        top_edge_full.is_some(),
        "Slab edge along y=0.0 must span the full interval [1.0, 9.0] unoccluded"
    );

    let slab_hidden_edges: Vec<_> = prims
        .iter()
        .filter(|p| {
            p.role == PrimitiveRole::HiddenEdge
                && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
        })
        .collect();
    assert!(
        slab_hidden_edges.is_empty(),
        "Hidden wall must not cause slab edges to become hidden lines, but found {}",
        slab_hidden_edges.len()
    );
}

#[test]
fn test_floorplan_door_reaching_overhead_slab_underside_emits_no_lintel() {
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();

    let viewpoint = Viewpoint {
        id: vp_id,
        name: "Ground Floor".into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.8).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    let model_view = ModelView {
        id: mv_id,
        name: "1:50 Floor Plan".into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
        revisions: ViewRevisions::default(),
    };

    // Wall from x=0 to x=5 along y=0, base_z=0.0, top_z=3.0
    // Overhead ceiling slab at base_z=2.5, thickness=0.2 (underside at 2.5) covering x in [0, 5], y in [-1, 1]
    let door_full = ElementRef::new(ModelId::new(), ElementId::new());
    let door_std = ElementRef::new(ModelId::new(), ElementId::new());
    let wall = CollectedWall {
        handle: acadrust::Handle::from(100u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "WallStyle".into(),
        height: 3.0,
        base_z: 0.0,
        top_z: 3.0,
        axis_points: vec![Point2::new(0.0, 0.0), Point2::new(5.0, 0.0)],
        layers: vec![CollectedWallLayer {
            material: "Concrete".into(),
            thickness: 0.24,
            function: "Structural".into(),
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
            component_id: ComponentId::new(),
            is_structural: true,
        }],
        total_thickness: 0.24,
        justification: WallJustification::Center,
        openings: vec![
            // Door 1: height=2.5 (reaches slab underside at 2.5 => full-height / headless)
            CollectedOpening {
                handle: acadrust::Handle::from(101u64),
                element_ref: door_full,
                xref_source: None,
                host_wall_handle: acadrust::Handle::from(100u64),
                kind: OpeningKind::Door,
                style_id: None,
                division_override: None,
                distance_along_axis: 1.5,
                width: 1.0,
                height: 2.5,
                sill_height: 0.0,
                spring_height: 0.0,
                shape: OpeningShape::Rectangle,
                hinge: LeafOperation::left(),
                swing_side: Default::default(),
                reference_side: OpeningReferenceSide::Start,
                cross_axis_offset: 0.0,
                depth: None,
                world_center: Point3::new(1.5, 0.0, 1.25),
                axis_span_local: (1.0, 2.0),
            },
            // Door 2: height=2.1 (below slab underside 2.5 => has lintel)
            CollectedOpening {
                handle: acadrust::Handle::from(102u64),
                element_ref: door_std,
                xref_source: None,
                host_wall_handle: acadrust::Handle::from(100u64),
                kind: OpeningKind::Door,
                style_id: None,
                division_override: None,
                distance_along_axis: 3.5,
                width: 1.0,
                height: 2.1,
                sill_height: 0.0,
                spring_height: 0.0,
                shape: OpeningShape::Rectangle,
                hinge: LeafOperation::left(),
                swing_side: Default::default(),
                reference_side: OpeningReferenceSide::Start,
                cross_axis_offset: 0.0,
                depth: None,
                world_center: Point3::new(3.5, 0.0, 1.05),
                axis_span_local: (3.0, 4.0),
            },
        ],
        phase: PlanPhase::New,
    };

    let slab = CollectedSlab {
        handle: acadrust::Handle::from(200u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "CeilingSlab".into(),
        base_z: 2.5,
        total_thickness: 0.20,
        polygon: vec![
            Point2::new(0.0, -1.0),
            Point2::new(5.0, -1.0),
            Point2::new(5.0, 1.0),
            Point2::new(0.0, 1.0),
        ],
        layers: Vec::new(),
        justification: crate::modules::aec::engine::slab::SlabJustification::Top,
        openings: Vec::new(),
        phase: PlanPhase::New,
    };

    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);
    collector.slabs.push(slab);

    let opts = FloorPlanEngineOptions {
        show_overhead_edges: true,
        ..Default::default()
    };
    let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &opts).unwrap();

    // Full-height door (reaches slab underside 2.5) must NOT have OpeningHead lintel lines
    let full_lintels: Vec<_> = prims
        .iter()
        .filter(|p| {
            p.role == PrimitiveRole::HiddenEdge
                && p.sources.as_slice().iter().any(|s| s.element == door_full && s.feature == Some(FeatureKey::OpeningHead))
        })
        .collect();
    assert_eq!(full_lintels.len(), 0, "Door reaching ceiling slab underside must not emit lintel lines");

    // Standard door (height 2.1 < 2.5) MUST have OpeningHead lintel lines
    let std_lintels: Vec<_> = prims
        .iter()
        .filter(|p| {
            p.role == PrimitiveRole::HiddenEdge
                && p.sources.as_slice().iter().any(|s| s.element == door_std && s.feature == Some(FeatureKey::OpeningHead))
        })
        .collect();
    assert_eq!(std_lintels.len(), 2, "Standard door below ceiling slab underside must emit lintel lines");
}

#[test]
fn test_floorplan_window_sill_at_cut_plane_emits_sill_lines() {
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();

    let viewpoint = Viewpoint {
        id: vp_id,
        name: "Ground Floor".into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.8).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(), // z_cut = 1.0
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    let model_view = ModelView {
        id: mv_id,
        name: "1:50 Floor Plan".into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
        revisions: ViewRevisions::default(),
    };

    let win_elem = ElementRef::new(ModelId::new(), ElementId::new());
    let wall = CollectedWall {
        handle: acadrust::Handle::from(100u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "WallStyle".into(),
        height: 3.0,
        base_z: 0.0,
        top_z: 3.0,
        axis_points: vec![Point2::new(0.0, 0.0), Point2::new(5.0, 0.0)],
        layers: vec![CollectedWallLayer {
            material: "Concrete".into(),
            thickness: 0.24,
            function: "Structural".into(),
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
            component_id: ComponentId::new(),
            is_structural: true,
        }],
        total_thickness: 0.24,
        justification: WallJustification::Center,
        openings: vec![
            // Window with sill_height = 1.0 (exactly at cut plane z_cut = 1.0)
            CollectedOpening {
                handle: acadrust::Handle::from(101u64),
                element_ref: win_elem,
                xref_source: None,
                host_wall_handle: acadrust::Handle::from(100u64),
                kind: OpeningKind::Window,
                style_id: None,
                division_override: None,
                distance_along_axis: 2.5,
                width: 1.2,
                height: 1.2,
                sill_height: 1.0,
                spring_height: 0.0,
                shape: OpeningShape::Rectangle,
                hinge: LeafOperation::left(),
                swing_side: Default::default(),
                reference_side: OpeningReferenceSide::Start,
                cross_axis_offset: 0.0,
                depth: None,
                world_center: Point3::new(2.5, 0.0, 1.6),
                axis_span_local: (1.9, 3.1),
            },
        ],
        phase: PlanPhase::New,
    };

    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);

    let opts = FloorPlanEngineOptions {
        show_window_sills: true,
        include_openings: true,
        ..Default::default()
    };
    let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &opts).unwrap();

    let sill_lines: Vec<_> = prims
        .iter()
        .filter(|p| {
            p.role == PrimitiveRole::ProjectedEdge
                && p.style.layer_name.as_deref() == Some("AEC_OPENING")
                && p.sources.as_slice().iter().any(|s| s.element == win_elem)
        })
        .collect();

    assert_eq!(
        sill_lines.len(),
        2,
        "Window whose sill equals cut elevation must retain its 2 sill lines"
    );
}

#[test]
fn test_floorplan_nearby_slab_not_covering_door_does_not_suppress_lintel() {
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();

    let viewpoint = Viewpoint {
        id: vp_id,
        name: "Ground Floor".into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.8).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(), // z_cut = 1.0
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    let model_view = ModelView {
        id: mv_id,
        name: "1:50 Floor Plan".into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
        revisions: ViewRevisions::default(),
    };

    let door_elem = ElementRef::new(ModelId::new(), ElementId::new());
    // Long stairwell wall from x=0 to x=6
    let wall = CollectedWall {
        handle: acadrust::Handle::from(100u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "WallStyle".into(),
        height: 3.0,
        base_z: 0.0,
        top_z: 3.0,
        axis_points: vec![Point2::new(0.0, 0.0), Point2::new(6.0, 0.0)],
        layers: vec![CollectedWallLayer {
            material: "Concrete".into(),
            thickness: 0.24,
            function: "Structural".into(),
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
            component_id: ComponentId::new(),
            is_structural: true,
        }],
        total_thickness: 0.24,
        justification: WallJustification::Center,
        openings: vec![
            // Door to hallway at x=4.5 (span 4.0..5.0), height=2.1, sill=0.0
            CollectedOpening {
                handle: acadrust::Handle::from(101u64),
                element_ref: door_elem,
                xref_source: None,
                host_wall_handle: acadrust::Handle::from(100u64),
                kind: OpeningKind::Door,
                style_id: None,
                division_override: None,
                distance_along_axis: 4.5,
                width: 1.0,
                height: 2.1,
                sill_height: 0.0,
                spring_height: 0.0,
                shape: OpeningShape::Rectangle,
                hinge: LeafOperation::left(),
                swing_side: Default::default(),
                reference_side: OpeningReferenceSide::Start,
                cross_axis_offset: 0.0,
                depth: None,
                world_center: Point3::new(4.5, 0.0, 1.05),
                axis_span_local: (4.0, 5.0),
            },
        ],
        phase: PlanPhase::New,
    };

    // Lower stair landing (Zwischenpodest) at base_z=1.3, polygon at x in [0, 2], y in [0.12, 2.0]
    // Its AABB overlaps the wall's AABB [0, 6] x [-0.12, 0.12], but it does NOT cover the door at x=4.5!
    let stair_landing = CollectedSlab {
        handle: acadrust::Handle::from(200u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "LandingSlab".into(),
        base_z: 1.3,
        total_thickness: 0.20,
        polygon: vec![
            Point2::new(0.0, 0.12),
            Point2::new(2.0, 0.12),
            Point2::new(2.0, 2.0),
            Point2::new(0.0, 2.0),
        ],
        layers: Vec::new(),
        justification: crate::modules::aec::engine::slab::SlabJustification::Top,
        openings: Vec::new(),
        phase: PlanPhase::New,
    };

    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);
    collector.slabs.push(stair_landing);

    let opts = FloorPlanEngineOptions {
        show_overhead_edges: true,
        ..Default::default()
    };
    let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &opts).unwrap();

    let lintels: Vec<_> = prims
        .iter()
        .filter(|p| {
            p.role == PrimitiveRole::HiddenEdge
                && p.sources.as_slice().iter().any(|s| s.element == door_elem && s.feature == Some(FeatureKey::OpeningHead))
        })
        .collect();

    assert_eq!(
        lintels.len(),
        2,
        "Lower nearby slab that does not cover the door must NOT suppress the door's lintel"
    );
}

#[test]
fn test_floorplan_breakthrough_with_sill_occludes_slab() {
    let vp_id = ViewpointId::new();
    let mv_id = ModelViewId::new();

    let viewpoint = Viewpoint {
        id: vp_id,
        name: "Ground Floor".into(),
        revision: Revision::default(),
        origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.8).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    let model_view = ModelView {
        id: mv_id,
        name: "1:50 Floor Plan".into(),
        viewpoint_id: vp_id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0).unwrap(),
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic,
        revisions: ViewRevisions::default(),
    };

    let bt_elem = ElementRef::new(ModelId::new(), ElementId::new());
    let wall = CollectedWall {
        handle: acadrust::Handle::from(100u64),
        element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
        xref_source: None,
        style_id: "WallStyle".into(),
        height: 3.0,
        base_z: 0.0,
        top_z: 3.0,
        axis_points: vec![Point2::new(0.0, 0.0), Point2::new(5.0, 0.0)],
        layers: vec![CollectedWallLayer {
            material: "Concrete".into(),
            thickness: 0.24,
            function: "Structural".into(),
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
            component_id: ComponentId::new(),
            is_structural: true,
        }],
        total_thickness: 0.24,
        justification: WallJustification::Center,
        openings: vec![
            CollectedOpening {
                handle: acadrust::Handle::from(101u64),
                element_ref: bt_elem,
                xref_source: None,
                host_wall_handle: acadrust::Handle::from(100u64),
                kind: OpeningKind::Breakthrough,
                style_id: None,
                division_override: None,
                distance_along_axis: 2.5,
                width: 1.0,
                height: 1.0,
                sill_height: 0.5,
                spring_height: 0.0,
                shape: OpeningShape::Rectangle,
                hinge: LeafOperation::left(),
                swing_side: Default::default(),
                reference_side: OpeningReferenceSide::Start,
                cross_axis_offset: 0.0,
                depth: None,
                world_center: Point3::new(2.5, 0.0, 1.0),
                axis_span_local: (2.0, 3.0),
            },
        ],
        phase: PlanPhase::New,
    };

    let slab_elem = ElementRef::new(ModelId::new(), ElementId::new());
    let slab = CollectedSlab {
        handle: acadrust::Handle::from(200u64),
        element_ref: slab_elem,
        xref_source: None,
        style_id: "FloorSlab".into(),
        base_z: -0.20,
        total_thickness: 0.20,
        polygon: vec![
            Point2::new(1.0, -1.0),
            Point2::new(4.0, -1.0),
            Point2::new(4.0, 0.0),
            Point2::new(1.0, 0.0),
        ],
        layers: Vec::new(),
        justification: crate::modules::aec::engine::slab::SlabJustification::Top,
        openings: Vec::new(),
        phase: PlanPhase::New,
    };

    let mut collector = SceneGeometryCollector::new();
    collector.walls.push(wall);
    collector.slabs.push(slab);

    let opts = FloorPlanEngineOptions {
        emit_hidden_lines: false,
        ..Default::default()
    };
    let prims = derive_floorplan_view(&viewpoint, &model_view, &collector, &opts).unwrap();

    let visible_slab_in_bt = prims.iter().find(|p| {
        p.role == PrimitiveRole::ProjectedEdge
            && p.sources.as_slice().iter().any(|s| s.element == slab_elem)
            && match &p.geometry {
                Geometry2d::Line { start, end } => {
                    let mid_x = (start.x_m + end.x_m) * 0.5;
                    let mid_y = (start.y_m + end.y_m) * 0.5;
                    mid_x >= 2.0 && mid_x <= 3.0 && mid_y.abs() < 1e-4
                }
                _ => false,
            }
    });

    assert!(
        visible_slab_in_bt.is_none(),
        "Breakthrough with sill must occlude below-cut slab edge"
    );
}
