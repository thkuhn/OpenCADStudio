use super::*;
use crate::modules::aec::engine::control_plane::ControlPlaneFacet;

#[test]
fn opening_grid_uses_style_template_and_preserves_instance_override() {
    use crate::modules::aec::engine::opening_style::{
        DivisionGrid, OpeningCell, OpeningStyle,
    };
    use crate::modules::aec::engine::openings::{Opening, OpeningKind};

    let mut library = StyleLibrary::default();
    let mut standard = OpeningStyle::standard_window();
    standard.grid = crate::modules::aec::engine::opening_style::construction_from_division(
        &DivisionGrid {
            vertical: vec![0.25, 0.75],
            cells: vec![OpeningCell::default(), OpeningCell::default()],
            ..DivisionGrid::default()
        },
        standard.default_width,
        standard.default_height,
    );
    library.opening_styles.push(standard.clone());

    let mut custom = OpeningStyle::standard_window();
    custom.style.id = "custom_window_grid".into();
    custom.grid = crate::modules::aec::engine::opening_style::construction_from_division(
        &DivisionGrid {
            vertical: vec![0.4, 0.6],
            cells: vec![OpeningCell::default(), OpeningCell::default()],
            ..DivisionGrid::default()
        },
        custom.default_width,
        custom.default_height,
    );
    library.opening_styles.push(custom.clone());

    let mut unstyled = Opening::from_kind(
        Handle::new(1),
        Handle::new(2),
        3.0,
        OpeningKind::Window,
    );
    fill_missing_opening_grids(std::slice::from_mut(&mut unstyled), &library);
    assert_eq!(unstyled.division_override, Some(standard.grid));

    let mut selected = Opening::from_kind(
        Handle::new(3),
        Handle::new(2),
        5.0,
        OpeningKind::Window,
    );
    selected.style_id = Some(custom.style.id.clone());
    fill_missing_opening_grids(std::slice::from_mut(&mut selected), &library);
    assert_eq!(selected.division_override, Some(custom.grid.clone()));

    let instance_grid = crate::modules::aec::engine::construction_grid::ConstructionGrid::default();
    selected.division_override = Some(instance_grid.clone());
    fill_missing_opening_grids(std::slice::from_mut(&mut selected), &library);
    assert_eq!(selected.division_override, Some(instance_grid));
}

#[test]
fn test_wall_layer_extrusions_apply_signed_top_offsets() {
    let mut axis = LwPolyline::new();
    axis.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    axis.add_vertex(LwVertex::new(Vector2::new(4.0, 0.0)));
    let axis = EntityType::LwPolyline(axis);

    let layers = [-0.20, 0.20]
        .into_iter()
        .map(|top_offset| WallLayer {
            material: "test".into(),
            thickness: 0.20,
            function: "Structural".into(),
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset,
            layer_override: None,
            hatch_override: None,
            layer_id: Uuid::new_v4(),
        })
        .collect::<Vec<_>>();

    let extrusions = wall_layer_extrusions(&axis, &layers, 3.0);
    assert_eq!(extrusions.len(), 2);
    assert!((extrusions[0].height - 2.80).abs() < 1e-9);
    assert!((extrusions[1].height - 3.20).abs() < 1e-9);
}

#[test]
fn test_partition_footprint_by_facets() {
    let footprint = vec![
        (0.0, -0.15),
        (10.0, -0.15),
        (10.0, 0.15),
        (0.0, 0.15),
    ];
    let bulges = vec![0.0; 4];

    let f1 = ControlPlaneFacet::new(
        "F1",
        vec![
            [0.0, -5.0, 3.2],
            [5.0, -5.0, 3.2],
            [5.0, 5.0, 3.2],
            [0.0, 5.0, 3.2],
        ],
    );
    let f2 = ControlPlaneFacet::new(
        "F2",
        vec![
            [5.0, -5.0, 2.4],
            [10.0, -5.0, 2.4],
            [10.0, 5.0, 2.4],
            [5.0, 5.0, 2.4],
        ],
    );

    let pieces = partition_footprint_by_facets(&footprint, &bulges, &[f1, f2]);
    assert_eq!(pieces.len(), 2);

    // First piece should span roughly 0..5 in X
    let (p1, _) = &pieces[0];
    let min_x1 = p1.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let max_x1 = p1.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    assert!((min_x1 - 0.0).abs() < 1e-4);
    assert!((max_x1 - 5.0).abs() < 1e-4);

    // Second piece should span roughly 5..10 in X
    let (p2, _) = &pieces[1];
    let min_x2 = p2.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let max_x2 = p2.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    assert!((min_x2 - 5.0).abs() < 1e-4);
    assert!((max_x2 - 10.0).abs() < 1e-4);
}

#[test]
fn test_build_sloped_layer_solid_stepped_facets() {
    let footprint1 = vec![
        (0.0, -0.15),
        (5.0, -0.15),
        (5.0, 0.15),
        (0.0, 0.15),
    ];
    let footprint2 = vec![
        (5.0, -0.15),
        (10.0, -0.15),
        (10.0, 0.15),
        (5.0, 0.15),
    ];
    let bulges = vec![0.0; 4];

    let f1 = ControlPlaneFacet::new(
        "Roof1",
        vec![
            [0.0, -5.0, 3.2],
            [5.0, -5.0, 3.2],
            [5.0, 5.0, 3.2],
            [0.0, 5.0, 3.2],
        ],
    );
    let f2 = ControlPlaneFacet::new(
        "Roof2",
        vec![
            [5.0, -5.0, 2.4],
            [10.0, -5.0, 2.4],
            [10.0, 5.0, 2.4],
            [5.0, 5.0, 2.4],
        ],
    );

    let mut wall = Wall::new("s", 3.0, 0);
    wall.base_origin = [0.0, 0.0, 0.0];
    wall.base_normal = [0.0, 0.0, 1.0];
    wall.top_origin = [0.0, 0.0, 3.2];
    wall.top_normal = [0.0, 0.0, 1.0];
    wall.top_facets = vec![f1, f2];

    let solid1 = build_sloped_layer_solid_3d(&footprint1, &bulges, &wall, None, 0.0, 3.0).unwrap();
    let solid2 = build_sloped_layer_solid_3d(&footprint2, &bulges, &wall, None, 0.0, 3.0).unwrap();

    assert!(solid1.validate().is_empty());
    assert!(solid2.validate().is_empty());

    let (min1, max1) = crate::scene::model::solid_model::extent(&solid1).expect("solid1 extent");
    let (min2, max2) = crate::scene::model::solid_model::extent(&solid2).expect("solid2 extent");
    assert!((max1[2] - min1[2] - 3.2).abs() < 1e-4);
    assert!((max2[2] - min2[2] - 2.4).abs() < 1e-4);
}

#[test]
fn test_build_sloped_layer_solid_partial_facet_fallback() {
    let footprint1 = vec![
        (0.0, -0.15),
        (5.0, -0.15),
        (5.0, 0.15),
        (0.0, 0.15),
    ];
    let footprint2 = vec![
        (5.0, -0.15),
        (10.0, -0.15),
        (10.0, 0.15),
        (5.0, 0.15),
    ];
    let bulges = vec![0.0; 4];

    // Facet covers 0..5 in X at Z = 4.0
    let f1 = ControlPlaneFacet::new(
        "Roof1",
        vec![
            [0.0, -5.0, 4.0],
            [5.0, -5.0, 4.0],
            [5.0, 5.0, 4.0],
            [0.0, 5.0, 4.0],
        ],
    );

    let mut wall = Wall::new("s", 2.8, 0);
    wall.base_origin = [0.0, 0.0, 0.0];
    wall.base_normal = [0.0, 0.0, 1.0];
    wall.top_origin = [0.0, 0.0, 4.0];
    wall.top_normal = [0.0, 0.0, 1.0];
    wall.top_facets = vec![f1];

    let solid1 = build_sloped_layer_solid_3d(&footprint1, &bulges, &wall, None, 0.0, 2.8).unwrap();
    let solid2 = build_sloped_layer_solid_3d(&footprint2, &bulges, &wall, None, 0.0, 2.8).unwrap();

    assert!(solid1.validate().is_empty());
    assert!(solid2.validate().is_empty());

    let (min1, max1) = crate::scene::model::solid_model::extent(&solid1).expect("solid1 extent");
    let (min2, max2) = crate::scene::model::solid_model::extent(&solid2).expect("solid2 extent");

    // Covered portion has top Z = 4.0
    assert!((max1[2] - min1[2] - 4.0).abs() < 1e-4);
    // Uncovered portion falls back strictly to standard wall height Z = 2.8 with vertical step at x = 5.0
    assert!((max2[2] - min2[2] - 2.8).abs() < 1e-4);
}

#[test]
fn test_partition_and_build_multi_facets_with_uncovered_section() {
    let full_footprint = vec![
        (0.0, -0.15),
        (15.0, -0.15),
        (15.0, 0.15),
        (0.0, 0.15),
    ];
    let full_bulges = vec![0.0; 4];

    let f1 = ControlPlaneFacet::new(
        "Roof1",
        vec![
            [0.0, -5.0, 4.0],
            [5.0, -5.0, 4.0],
            [5.0, 5.0, 4.0],
            [0.0, 5.0, 4.0],
        ],
    );
    let f2 = ControlPlaneFacet::new(
        "Roof2",
        vec![
            [5.0, -5.0, 3.5],
            [10.0, -5.0, 3.5],
            [10.0, 5.0, 3.5],
            [5.0, 5.0, 3.5],
        ],
    );

    let mut wall = Wall::new("s", 2.8, 0);
    wall.base_origin = [0.0, 0.0, 0.0];
    wall.base_normal = [0.0, 0.0, 1.0];
    wall.top_origin = [0.0, 0.0, 4.0];
    wall.top_normal = [0.0, 0.0, 1.0];
    wall.top_facets = vec![f1, f2];

    let partitions = partition_footprint_by_facets(&full_footprint, &full_bulges, &wall.top_facets);
    assert_eq!(partitions.len(), 3);

    let mut solids = Vec::new();
    for (fp, bulges) in &partitions {
        let s = build_sloped_layer_solid_3d(fp, bulges, &wall, None, 0.0, 2.8).expect("solid build");
        assert!(s.validate().is_empty());
        solids.push((fp.clone(), s));
    }

    // Check height for each section by X position
    for (fp, solid) in solids {
        let cx = fp.iter().map(|p| p.0).sum::<f64>() / fp.len() as f64;
        let (min, max) = crate::scene::model::solid_model::extent(&solid).expect("extent");
        let height = max[2] - min[2];
        if cx < 5.0 {
            // Facet 1 section
            assert!((height - 4.0).abs() < 1e-4, "Facet 1 height was {height}");
        } else if cx < 10.0 {
            // Facet 2 section
            assert!((height - 3.5).abs() < 1e-4, "Facet 2 height was {height}");
        } else {
            // Uncovered section: standard wall height 2.8
            assert!((height - 2.8).abs() < 1e-4, "Uncovered height was {height}");
        }
    }
}

#[test]
fn test_filter_facet_partition_edges_removes_internal_surface_lines() {
    let footprint1 = vec![
        (0.0, -0.15),
        (5.0, -0.15),
        (5.0, 0.15),
        (0.0, 0.15),
    ];
    let footprint2 = vec![
        (5.0, -0.15),
        (10.0, -0.15),
        (10.0, 0.15),
        (5.0, 0.15),
    ];
    let all_pieces = vec![
        (footprint1.clone(), vec![0.0; 4]),
        (footprint2.clone(), vec![0.0; 4]),
    ];

    // Facet covers 0..5 at Z = 4.0, while 5..10 falls back to Z = 2.8
    let f1 = ControlPlaneFacet::new(
        "Roof1",
        vec![
            [0.0, -5.0, 4.0],
            [5.0, -5.0, 4.0],
            [5.0, 5.0, 4.0],
            [0.0, 5.0, 4.0],
        ],
    );

    let mut wall = Wall::new("s", 2.8, 0);
    wall.base_origin = [0.0, 0.0, 0.0];
    wall.base_normal = [0.0, 0.0, 1.0];
    wall.top_origin = [0.0, 0.0, 4.0];
    wall.top_normal = [0.0, 0.0, 1.0];
    wall.top_facets = vec![f1];

    // --- Test Piece 1 (taller piece, 0..5, height 4.0) ---
    let mut wires1 = vec![
        // Vertical edge at boundary seam (x = 5.0, y = 0.15) from base Z=0 to top Z=4.0
        acadrust::entities::Wire::from_points(vec![
            acadrust::types::Vector3::new(5.0, 0.15, 0.0),
            acadrust::types::Vector3::new(5.0, 0.15, 4.0),
        ]),
        // Transverse base edge across seam at Z=0
        acadrust::entities::Wire::from_points(vec![
            acadrust::types::Vector3::new(5.0, -0.15, 0.0),
            acadrust::types::Vector3::new(5.0, 0.15, 0.0),
        ]),
        // Transverse top edge across seam at Z=4.0
        acadrust::entities::Wire::from_points(vec![
            acadrust::types::Vector3::new(5.0, 0.15, 4.0),
            acadrust::types::Vector3::new(5.0, -0.15, 4.0),
        ]),
        // Outer wall edge at x = 0.0 from Z=0 to Z=4.0
        acadrust::entities::Wire::from_points(vec![
            acadrust::types::Vector3::new(0.0, 0.15, 0.0),
            acadrust::types::Vector3::new(0.0, 0.15, 4.0),
        ]),
    ];
    let mut mesh_set1 = crate::scene::model::mesh_model::MeshLodSet::from_lods(Vec::new());
    filter_facet_partition_edges(
        &mut mesh_set1,
        &mut wires1,
        &footprint1,
        &all_pieces,
        &wall,
        None,
        0.0,
        2.8,
    );

    // In Piece 1:
    // 1. The vertical edge at (5.0, 0.15) should be trimmed to [2.8, 4.0] (the exposed step edge).
    // 2. The base seam at Z=0 should be removed (shared base).
    // 3. The top edge at Z=4.0 should be kept (step top roof edge).
    // 4. The outer edge at x=0 should be unchanged.
    let vert_seam1: Vec<&acadrust::entities::Wire> = wires1
        .iter()
        .filter(|w| {
            (w.points[0].x - 5.0).abs() < 1e-3
                && (w.points[0].y - 0.15).abs() < 1e-3
                && (w.points[1].x - 5.0).abs() < 1e-3
                && (w.points[1].y - 0.15).abs() < 1e-3
                && (w.points[0].z - w.points[1].z).abs() > 1e-3
        })
        .collect();
    assert_eq!(vert_seam1.len(), 1);
    assert!((vert_seam1[0].points[0].z - 2.8).abs() < 1e-3);
    assert!((vert_seam1[0].points[1].z - 4.0).abs() < 1e-3);

    let base_seam1 = wires1.iter().find(|w| {
        (w.points[0].x - 5.0).abs() < 1e-3
            && (w.points[1].x - 5.0).abs() < 1e-3
            && (w.points[0].z - 0.0).abs() < 1e-3
            && (w.points[1].z - 0.0).abs() < 1e-3
    });
    assert!(base_seam1.is_none(), "Base seam should be removed");

    let top_seam1 = wires1.iter().find(|w| {
        (w.points[0].x - 5.0).abs() < 1e-3
            && (w.points[1].x - 5.0).abs() < 1e-3
            && (w.points[0].z - 4.0).abs() < 1e-3
            && (w.points[1].z - 4.0).abs() < 1e-3
    });
    assert!(top_seam1.is_some(), "Top roof edge of step should be retained");

    // --- Test Piece 2 (shorter piece, 5..10, height 2.8) ---
    let mut wires2 = vec![
        // Vertical edge at boundary seam (x = 5.0, y = 0.15) from base Z=0 to top Z=2.8
        acadrust::entities::Wire::from_points(vec![
            acadrust::types::Vector3::new(5.0, 0.15, 0.0),
            acadrust::types::Vector3::new(5.0, 0.15, 2.8),
        ]),
        // Transverse base edge across seam at Z=0
        acadrust::entities::Wire::from_points(vec![
            acadrust::types::Vector3::new(5.0, 0.15, 0.0),
            acadrust::types::Vector3::new(5.0, -0.15, 0.0),
        ]),
        // Transverse top edge across seam at Z=2.8 (top of step)
        acadrust::entities::Wire::from_points(vec![
            acadrust::types::Vector3::new(5.0, -0.15, 2.8),
            acadrust::types::Vector3::new(5.0, 0.15, 2.8),
        ]),
    ];
    let mut mesh_set2 = crate::scene::model::mesh_model::MeshLodSet::from_lods(Vec::new());
    filter_facet_partition_edges(
        &mut mesh_set2,
        &mut wires2,
        &footprint2,
        &all_pieces,
        &wall,
        None,
        0.0,
        2.8,
    );

    // In Piece 2:
    // 1. The vertical edge at (5.0, 0.15) from 0 to 2.8 is completely inside overlap [0, 2.8] -> fully removed!
    // 2. The base seam at Z=0 should be removed.
    // 3. The top edge at Z=2.8 should be KEPT (horizontal step corner).
    let vert_seam2 = wires2
        .iter()
        .find(|w| (w.points[0].x - 5.0).abs() < 1e-3 && (w.points[0].y - 0.15).abs() < 1e-3 && w.points[0].z != w.points[1].z);
    assert!(vert_seam2.is_none(), "Vertical seam on wall face below step should be completely removed");

    let base_seam2 = wires2.iter().find(|w| {
        (w.points[0].x - 5.0).abs() < 1e-3
            && (w.points[1].x - 5.0).abs() < 1e-3
            && (w.points[0].z - 0.0).abs() < 1e-3
            && (w.points[1].z - 0.0).abs() < 1e-3
    });
    assert!(base_seam2.is_none(), "Base seam should be removed");

    let step_top_seam2 = wires2.iter().find(|w| {
        (w.points[0].x - 5.0).abs() < 1e-3
            && (w.points[1].x - 5.0).abs() < 1e-3
            && (w.points[0].z - 2.8).abs() < 1e-3
            && (w.points[1].z - 2.8).abs() < 1e-3
    });
    assert!(step_top_seam2.is_some(), "Step top corner edge should be retained");
}

#[test]
fn test_filter_facet_partition_edges_coplanar_removes_all_seams() {
    let footprint1 = vec![
        (0.0, -0.15),
        (5.0, -0.15),
        (5.0, 0.15),
        (0.0, 0.15),
    ];
    let footprint2 = vec![
        (5.0, -0.15),
        (10.0, -0.15),
        (10.0, 0.15),
        (5.0, 0.15),
    ];
    let all_pieces = vec![
        (footprint1.clone(), vec![0.0; 4]),
        (footprint2.clone(), vec![0.0; 4]),
    ];

    let mut wall = Wall::new("s", 3.0, 0);
    wall.base_origin = [0.0, 0.0, 0.0];
    wall.base_normal = [0.0, 0.0, 1.0];
    wall.top_origin = [0.0, 0.0, 3.0];
    wall.top_normal = [0.0, 0.0, 1.0];

    let mut wires = vec![
        // Vertical edge at seam x=5.0
        acadrust::entities::Wire::from_points(vec![
            acadrust::types::Vector3::new(5.0, 0.15, 0.0),
            acadrust::types::Vector3::new(5.0, 0.15, 3.0),
        ]),
        // Base transverse edge at seam x=5.0
        acadrust::entities::Wire::from_points(vec![
            acadrust::types::Vector3::new(5.0, -0.15, 0.0),
            acadrust::types::Vector3::new(5.0, 0.15, 0.0),
        ]),
        // Top transverse edge at seam x=5.0
        acadrust::entities::Wire::from_points(vec![
            acadrust::types::Vector3::new(5.0, 0.15, 3.0),
            acadrust::types::Vector3::new(5.0, -0.15, 3.0),
        ]),
    ];
    let mut mesh_set = crate::scene::model::mesh_model::MeshLodSet::from_lods(Vec::new());
    filter_facet_partition_edges(
        &mut mesh_set,
        &mut wires,
        &footprint1,
        &all_pieces,
        &wall,
        None,
        0.0,
        3.0,
    );

    // When coplanar / equal heights, all internal seam edges at x=5.0 should be removed!
    assert_eq!(wires.len(), 0);
}
