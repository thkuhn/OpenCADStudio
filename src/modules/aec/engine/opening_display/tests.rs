use super::*;
use acadrust::entities::LwVertex;
use acadrust::types::Vector2;
use crate::modules::aec::engine::library::seed_default_library;
use crate::modules::aec::engine::opening_style::{
    DivisionGrid, DivisionLevel, ElevationFrameLevel, OpeningCell,
    OpeningParts, OpeningStyle, PlanFrameLevel, SwingGlyph, DEFAULT_FRAME_THICKNESS,
    SEED_WINDOW_STYLE_ID,
};
use crate::modules::aec::engine::opening_xdata::place_wall_opening;
use crate::modules::aec::engine::openings::{
    Opening, OpeningKind, DEFAULT_WINDOW_WIDTH, SwingSide,
};
use crate::modules::aec::engine::wall::{Wall, WallJustification, WallLayer};
use crate::modules::aec::engine::wall_regen::regenerate_wall_representation;
use crate::modules::aec::engine::xdata::wall_record_for_wall;
use crate::scene::Scene;

fn params(width: f64, kind: OpeningKind, hinge: LeafOperation) -> OpeningBakeParams {
    OpeningBakeParams {
        width,
        height: 1.2,
        sill_height: 0.9,
        thickness: 0.3,
        frame_thickness: DEFAULT_FRAME_THICKNESS,
        cross_axis_offset: 0.0,
        wall_y_min: -0.15,
        wall_y_max: 0.15,
        hinge,
        swing_side: SwingSide::Exterior,
        shape: OpeningShape::Rectangle,
        spring_height: 0.0,
        opening_angle_deg: 90.0,
        kind,
    }
}

fn inner_frame_half_width(paths: &[BakedPath]) -> Option<f64> {
    let frames: Vec<_> = paths
        .iter()
        .filter(|p| p.slot == OpeningComponentSlot::Frame2D && p.closed)
        .collect();
    if frames.is_empty() {
        return None;
    }
    if frames.len() == 1 {
        let max_x = frames[0]
            .points
            .iter()
            .map(|p| p.0)
            .fold(f64::NEG_INFINITY, f64::max);
        return Some(max_x);
    }
    let has_two_separated_posts = frames.len() == 2 && {
        let p0_center_x = frames[0].points.iter().map(|p| p.0).sum::<f64>()
            / (frames[0].points.len() as f64);
        let p1_center_x = frames[1].points.iter().map(|p| p.0).sum::<f64>()
            / (frames[1].points.len() as f64);
        (p0_center_x * p1_center_x) < 0.0
    };
    if has_two_separated_posts {
        let right = frames.iter().max_by(|a, b| {
            let ax = a.points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
            let bx = b.points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
            ax.partial_cmp(&bx).unwrap()
        })?;
        let min_x = right
            .points
            .iter()
            .map(|p| p.0)
            .fold(f64::INFINITY, f64::min);
        Some(min_x)
    } else {
        let inner = frames.iter().min_by(|a, b| {
            let wa = a.points.iter().map(|p| p.0).fold(f64::NAN, f64::max)
                - a.points.iter().map(|p| p.0).fold(f64::NAN, f64::min);
            let wb = b.points.iter().map(|p| p.0).fold(f64::NAN, f64::max)
                - b.points.iter().map(|p| p.0).fold(f64::NAN, f64::min);
            wa.partial_cmp(&wb).unwrap()
        })?;
        let max_x = inner
            .points
            .iter()
            .map(|p| p.0)
            .fold(f64::NEG_INFINITY, f64::max);
        Some(max_x)
    }
}

#[test]
fn leaf_cell_offsets_reach_all_rendered_frame_sides_and_keep_band_pivots() {
    let cells = leaf_cells_with_bar_offsets(
        &ConstructionGrid::default(),
        1.2,
        1.5,
        0.08,
        None,
        0.05,
        0.03,
        true,
        false,
    );
    assert_eq!(cells.len(), 1);
    let cell = &cells[0];
    assert!((cell.left - -0.55).abs() < 1e-9);
    assert!((cell.right - 0.55).abs() < 1e-9);
    assert!((cell.bottom - 0.05).abs() < 1e-9);
    assert!((cell.top - 1.45).abs() < 1e-9);
    // The hinge axis sits on the leaf's own edges.
    assert!((cell.pivot_start - -0.55).abs() < 1e-9);
    assert!((cell.pivot_end - 0.55).abs() < 1e-9);

    let door = leaf_cells_with_bar_offsets(
        &ConstructionGrid::default(),
        1.2,
        1.5,
        0.08,
        None,
        0.05,
        0.03,
        true,
        true,
    );
    assert_eq!(door.len(), 1);
    assert!(door[0].bottom.abs() < 1e-9);
}

#[test]
fn leaf_glazing_rotates_about_the_same_band_side_in_the_leaf_depth() {
    let depth_offset = 0.07;
    let pivot_x = -0.31;
    let (leaf, glass) = build_opening_leaf_components_3d(
        0.8,
        1.0,
        0.06,
        0.05,
        depth_offset,
        pivot_x,
        90.0,
        LeafOperation::left(),
        SwingSide::Exterior,
        false,
        OpeningShape::Rectangle,
        0.0,
        Some(0.02),
    )
    .expect("leaf and glass bodies");
    let glass = glass.expect("leaf glazing");
    let leaf_bounds = crate::scene::model::solid_model::extent(&leaf).expect("leaf extent");
    let glass_bounds = crate::scene::model::solid_model::extent(&glass).expect("glass extent");
    let center = |bounds: ([f64; 3], [f64; 3])| {
        std::array::from_fn::<_, 3, _>(|axis| (bounds.0[axis] + bounds.1[axis]) * 0.5)
    };
    let leaf_center = center(leaf_bounds);
    let glass_center = center(glass_bounds);
    for axis in 0..3 {
        assert!((leaf_center[axis] - glass_center[axis]).abs() < 1e-5);
    }
    assert!((leaf_center[1] - depth_offset).abs() > 0.1);
    // The hinge axis sits on the swing-side face of the leaf (exterior: -y).
    let pivot_y = depth_offset - 0.025;
    let radius = ((leaf_center[0] - pivot_x).powi(2) + (leaf_center[1] - pivot_y).powi(2)).sqrt();
    assert!((radius - (0.31f64.powi(2) + 0.025f64.powi(2)).sqrt()).abs() < 1e-5);
}

#[test]
fn unstyled_opening_resolves_to_standard_style_parts() {
    let opening = Opening::window(
        acadrust::Handle::new(1),
        acadrust::Handle::new(2),
        1.2,
    );
    let (slots, style) = resolved_slots(&opening, None);
    let style = style.expect("standard fallback style");
    assert_eq!(style.style.id, SEED_WINDOW_STYLE_ID);
    assert_eq!(style.parts.fixed_frame.face_width, DEFAULT_FRAME_THICKNESS);
    assert!(slots.contains_key(&OpeningComponentSlot::Frame2D));
}

#[test]
fn opening_style_swing_is_the_fallback_for_cell_overrides() {
    let opening = Opening::window(
        acadrust::Handle::new(1),
        acadrust::Handle::new(2),
        1.2,
    );
    let mut style = OpeningStyle::standard_window();
    style.swing_side = SwingSide::Interior;
    let params = OpeningBakeParams::from_opening(&opening, 0.3, Some(&style));
    assert_eq!(params.swing_side, SwingSide::Interior);
}

#[test]
fn plan_and_elevation_posts_share_the_same_grid_coordinates() {
    let grid = DivisionGrid {
        vertical: vec![1.0 / 3.0, 2.0 / 3.0],
        cells: vec![OpeningCell::default(), OpeningCell::default()],
        ..DivisionGrid::default()
    };
    let params = params(1.2, OpeningKind::Window, LeafOperation::left());
    let parts = OpeningParts::default();
    let plan = bake_opening_plan_paths(
        params,
        &parts,
        None,
        &grid,
        PlanFrameLevel::Off,
        SwingGlyph::Off,
        DivisionLevel::Posts,
    );
    let elevation = bake_opening_elevation_paths(
        params,
        &parts,
        None,
        &grid,
        ElevationFrameLevel::Off,
        DivisionLevel::Posts,
    );
    let plan_post = plan
        .iter()
        .find(|path| {
            path.slot == OpeningComponentSlot::Division2D
                && path.points[0].0 == path.points[1].0
        })
        .unwrap();
    let elevation_post = elevation
        .iter()
        .find(|path| {
            path.slot == OpeningComponentSlot::ElevationMuntins2D
                && path.points[0].0 == path.points[1].0
        })
        .unwrap();
    let expected = -params.width * 0.5 + params.width / 3.0;
    assert!((plan_post.points[0].0 - expected).abs() < 1e-12);
    assert!((elevation_post.points[0].0 - expected).abs() < 1e-12);
}

#[test]
fn grid_bars_3d_are_built_at_primary_and_muntin_positions() {
    let grid = DivisionGrid {
        vertical: vec![0.4, 0.6],
        vertical_muntins: vec![0.5],
        cells: vec![OpeningCell::default(), OpeningCell::default()],
        ..DivisionGrid::default()
    };
    let bars = build_opening_grid_bars_3d(
        &construction_from_division(&grid, 2.0, 1.2),
        params(2.0, OpeningKind::Window, LeafOperation::left()),
        0.06,
        0.06,
        None,
        &resolve_section(None, 0.04, 0.06),
    );
    assert_eq!(bars.len(), 3);

    let stulp = OpeningStyle::window_double();
    let stulp_paths = bake_opening_plan_from_grid(
        params(stulp.default_width, OpeningKind::Window, LeafOperation::left()),
        &stulp.parts,
        None,
        &stulp.grid,
        PlanFrameLevel::Off,
        SwingGlyph::Off,
        DivisionLevel::Posts,
    );
    assert!(stulp_paths
        .iter()
        .all(|path| path.resolved_look_role() != Some(OpeningLookRole::Division)));
    assert!((bars[0].1 + 0.2).abs() < 1e-12);
    assert!((bars[1].1 + 0.6).abs() < 1e-12);
    assert!((bars[2].1 - 0.4).abs() < 1e-12);
}

#[test]
fn muntin_profile_covers_every_bar_and_an_unknown_id_keeps_local_sizes() {
    use crate::modules::aec::engine::construction_grid::{CellBar, CellFill, GridCell};
    use crate::modules::aec::engine::section_profile::{SectionProfile, SectionShape};

    let mut library = StyleLibrary::empty();
    assert!(library.upsert_section_profile(SectionProfile::new(
        "sash-bar",
        "Sash bar",
        SectionShape::Rectangle {
            face: 0.018,
            depth: 0.024,
        },
    )));
    let grid = ConstructionGrid {
        muntin_profile_id: Some("sash-bar".into()),
        cells: vec![GridCell {
            fill: CellFill::Glass,
            bars: vec![CellBar {
                face_width: 0.9,
                depth: 0.9,
                ..CellBar::default()
            }],
            ..GridCell::default()
        }],
        ..ConstructionGrid::default()
    };
    let leaf = resolve_section(None, 0.05, 0.04);
    let covered = muntin_bar_section(Some(&library), Some("sash-bar"), 0.9, 0.9, &leaf)
        .expect("profile");
    assert!((covered.face - 0.018).abs() < 1e-12);
    let kept = muntin_bar_section(Some(&library), Some("missing"), 0.03, 0.02, &leaf)
        .expect("local");
    assert!((kept.face - 0.03).abs() < 1e-12);
    let paths = bake_grid_plan_divisions(
        &grid,
        params(1.2, OpeningKind::Window, LeafOperation::left()),
        DivisionLevel::PostsAndMuntins,
        Some(&library),
        &leaf,
        &leaf,
    );
    assert_eq!(paths.len(), 1);
    let width = paths[0]
        .points
        .iter()
        .map(|point| point.0)
        .fold(f64::NEG_INFINITY, f64::max)
        - paths[0]
            .points
            .iter()
            .map(|point| point.0)
            .fold(f64::INFINITY, f64::min);
    assert!((width - 0.018).abs() < 1e-9);
}

#[test]
fn stulp_bar_sits_on_the_bearer_and_the_neighbour_edge_stays_empty() {
    use crate::modules::aec::engine::construction_grid::division_preset;

    let grid = division_preset("double", crate::modules::aec::engine::construction_grid::CellFill::Glass)
        .expect("preset");
    let leaf = resolve_section(None, 0.04, 0.06);
    let paths = bake_grid_elevation_divisions(
        &grid,
        params(1.2, OpeningKind::Window, LeafOperation::left()),
        DivisionLevel::Posts,
        None,
        &leaf,
        &leaf,
    );
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].look_role, Some(OpeningLookRole::Leaf));
    let center = paths[0]
        .points
        .iter()
        .map(|point| point.0)
        .sum::<f64>()
        / paths[0].points.len() as f64;
    assert!((center - (0.6 - 0.6 + 0.02)).abs() < 1e-9);
}

#[test]
fn fold_chain_draws_one_schema_per_leaf_and_no_interior_post() {
    use crate::modules::aec::engine::construction_grid::{
        CellFill, ConstructionGrid, GridCell, GridTrack, LeafOperation, TrackEnd, TrackMeasure,
        TrackMeeting,
    };

    let fold = LeafOperation::Fold { stack: TrackEnd::End };
    let grid = ConstructionGrid {
        columns: (0..4)
            .map(|index| GridTrack {
                measure: TrackMeasure::Fraction(1.0),
                meeting: if index < 3 {
                    TrackMeeting::Flush
                } else {
                    TrackMeeting::Mullion
                },
                profile_id: None,
            })
            .collect(),
        rows: vec![GridTrack::default()],
        cells: (0..4)
            .map(|column| GridCell {
                column,
                fill: CellFill::Glass,
                operation: Some(fold),
                ..GridCell::default()
            })
            .collect(),
        muntin_profile_id: None,
    };
    let baked = params(2.0, OpeningKind::Window, fold);
    let schemas = bake_grid_elevation_schema(&grid, baked, 0.0);
    assert_eq!(schemas.len(), 4);
    for pair in schemas.windows(2) {
        assert_eq!(pair[0].points[1], pair[1].points[0]);
    }
    let leaf = resolve_section(None, 0.04, 0.06);
    let posts = bake_grid_elevation_divisions(
        &grid,
        baked,
        DivisionLevel::Posts,
        None,
        &leaf,
        &leaf,
    );
    assert!(posts.is_empty());

    let mut mixed = grid.clone();
    mixed.cells[3].operation = Some(LeafOperation::Slide {
        direction: TrackEnd::End,
    });
    let mixed_schemas = bake_grid_elevation_schema(&mixed, baked, 0.0);
    assert_eq!(mixed_schemas.len(), 4);
    assert_ne!(mixed_schemas[2].points[1], mixed_schemas[3].points[0]);
}

#[test]
fn glass_bites_into_the_surrounding_bar_and_stops_at_its_outer_edge() {
    assert!((glass_edge_into_bar(0.08, 1.0, 0.08, 0.012) - 0.148).abs() < 1e-12);
    assert!((glass_edge_into_bar(0.0, 1.0, 0.08, 0.2) - 0.0).abs() < 1e-12);
}

#[test]
fn section_profile_outline_drives_plan_frame_paths_and_hull_dimensions() {
    use crate::modules::aec::engine::section_profile::{SectionProfile, SectionShape};

    let mut library = StyleLibrary::empty();
    assert!(library.upsert_section_profile(SectionProfile::new(
        "triangle_frame",
        "Triangle frame",
        SectionShape::Triangle {
            base: 0.06,
            height: 0.05,
        },
    )));
    let mut parts = OpeningParts::default();
    parts.fixed_frame.profile_id = Some("triangle_frame".to_string());
    parts.fixed_frame.face_width = 0.0;
    parts.fixed_frame.wall_depth = 0.0;
    let paths = bake_opening_plan_paths(
        params(1.2, OpeningKind::Window, LeafOperation::left()),
        &parts,
        Some(&library),
        &DivisionGrid::default(),
        PlanFrameLevel::Profile,
        SwingGlyph::Off,
        DivisionLevel::Off,
    );
    let frames: Vec<_> = paths
        .iter()
        .filter(|path| path.slot == OpeningComponentSlot::Frame2D)
        .collect();
    assert_eq!(frames.len(), 2);
    assert!(frames.iter().all(|path| path.closed && path.points.len() == 3));
    let first = frames[0];
    let min_x = first.points.iter().map(|point| point.0).fold(f64::INFINITY, f64::min);
    let max_x = first.points.iter().map(|point| point.0).fold(f64::NEG_INFINITY, f64::max);
    let min_y = first.points.iter().map(|point| point.1).fold(f64::INFINITY, f64::min);
    let max_y = first.points.iter().map(|point| point.1).fold(f64::NEG_INFINITY, f64::max);
    assert!((max_x - min_x - 0.06).abs() < 1e-12);
    assert!((max_y - min_y - 0.05).abs() < 1e-12);

    let hull = resolve_component_section(Some(&library), Some("triangle_frame"), 1.0, 1.0);
    assert_eq!((hull.face, hull.depth), (0.06, 0.05));

    let grid = DivisionGrid {
        vertical: vec![0.5, 0.5],
        cells: vec![crate::modules::aec::engine::opening_style::OpeningCell::default(); 2],
        profile_id: Some("triangle_frame".into()),
        ..DivisionGrid::default()
    };
    let division_paths = bake_opening_plan_paths(
        params(1.2, OpeningKind::Window, LeafOperation::left()),
        &OpeningParts::default(),
        Some(&library),
        &grid,
        PlanFrameLevel::Off,
        SwingGlyph::Off,
        DivisionLevel::Posts,
    );
    let division = division_paths
        .iter()
        .find(|path| path.resolved_look_role() == Some(OpeningLookRole::Division))
        .expect("division profile contour");
    assert!(division.closed);
    assert_eq!(division.points.len(), 3);

    let elevation_paths = bake_opening_elevation_paths(
        params(1.2, OpeningKind::Window, LeafOperation::left()),
        &OpeningParts::default(),
        Some(&library),
        &grid,
        ElevationFrameLevel::Off,
        DivisionLevel::Posts,
    );
    let elevation_division = elevation_paths
        .iter()
        .find(|path| path.slot == OpeningComponentSlot::ElevationMuntins2D)
        .expect("elevation division profile");
    assert!(elevation_division.closed);
    assert_eq!(elevation_division.points.len(), 4);
}

#[test]
fn plan_frame_levels_draw_line_double_line_and_absolute_profile() {
    let parts = OpeningParts::default();
    let grid = DivisionGrid::default();
    let params = params(1.2, OpeningKind::Window, LeafOperation::left());
    let frame_paths = |level| {
        bake_opening_plan_paths(
            params,
            &parts,
            None,
            &grid,
            level,
            SwingGlyph::Off,
            DivisionLevel::Off,
        )
        .into_iter()
        .filter(|path| path.slot == OpeningComponentSlot::Frame2D)
        .collect::<Vec<_>>()
    };
    let single = frame_paths(PlanFrameLevel::SingleLine);
    assert_eq!(single.len(), 1);
    assert_eq!(single[0].points.len(), 2);
    let double = frame_paths(PlanFrameLevel::DoubleLine);
    assert_eq!(double.len(), 2);
    assert!(double.iter().all(|path| !path.closed));
    let profile = frame_paths(PlanFrameLevel::Profile);
    assert_eq!(profile.len(), 2);
    assert!(profile.iter().all(|path| path.closed));
    assert!((profile[0].points[1].0 - profile[0].points[0].0 - 0.06).abs() < 1e-12);
}

#[test]
fn plan_triangle_ignores_opening_angle_but_arc_uses_it() {
    let parts = OpeningParts::default();
    let grid = DivisionGrid::default();
    let mut first_params = params(1.0, OpeningKind::Door, LeafOperation::left());
    first_params.opening_angle_deg = 35.0;
    let mut second_params = first_params;
    second_params.opening_angle_deg = 120.0;
    let glyph_paths = |p, glyph| {
        bake_opening_plan_paths(
            p,
            &parts,
            None,
            &grid,
            PlanFrameLevel::Off,
            glyph,
            DivisionLevel::Off,
        )
        .into_iter()
        .filter(|path| path.slot == OpeningComponentSlot::Swing2D)
        .collect::<Vec<_>>()
    };
    assert_eq!(
        glyph_paths(first_params, SwingGlyph::Triangle),
        glyph_paths(second_params, SwingGlyph::Triangle)
    );
    assert_ne!(
        glyph_paths(first_params, SwingGlyph::Arc),
        glyph_paths(second_params, SwingGlyph::Arc)
    );
}

#[test]
fn window_frame_thickness_does_not_scale_with_width() {
    let slots = default_slots_for_kind(OpeningKind::Window);
    let a = bake_opening_generators(&slots, params(1.2, OpeningKind::Window, LeafOperation::left()), None);
    let b = bake_opening_generators(&slots, params(1.8, OpeningKind::Window, LeafOperation::left()), None);
    let ia = inner_frame_half_width(&a).unwrap();
    let ib = inner_frame_half_width(&b).unwrap();
    assert!((ia - (1.2 * 0.5 - DEFAULT_FRAME_THICKNESS)).abs() < 1e-9);
    assert!((ib - (1.8 * 0.5 - DEFAULT_FRAME_THICKNESS)).abs() < 1e-9);
    let inset_a = 1.2 * 0.5 - ia;
    let inset_b = 1.8 * 0.5 - ib;
    assert!((inset_a - DEFAULT_FRAME_THICKNESS).abs() < 1e-9);
    assert!((inset_b - DEFAULT_FRAME_THICKNESS).abs() < 1e-9);
}

#[test]
fn door_swing_mirrors_with_hinge() {
    let slots = default_slots_for_kind(OpeningKind::Door);
    let left = bake_opening_generators(
        &slots,
        params(0.9, OpeningKind::Door, LeafOperation::left()),
        None,
    );
    let right = bake_opening_generators(
        &slots,
        params(0.9, OpeningKind::Door, LeafOperation::right()),
        None,
    );
    let leaf_l = left
        .iter()
        .find(|p| p.slot == OpeningComponentSlot::Leaf2D)
        .unwrap();
    let leaf_r = right
        .iter()
        .find(|p| p.slot == OpeningComponentSlot::Leaf2D)
        .unwrap();
    assert!(leaf_l.points[0].0 < 0.0);
    assert!(leaf_r.points[0].0 > 0.0);
    assert!((leaf_l.points[1].0 + leaf_r.points[1].0).abs() < 1e-9);
    assert!(left.iter().any(|p| p.slot == OpeningComponentSlot::Swing2D));
    assert!(right.iter().any(|p| p.slot == OpeningComponentSlot::Swing2D));
}

#[test]
fn door_swing_direction_interior_vs_exterior_flips_y_deflection() {
    use crate::modules::aec::engine::openings::SwingSide;

    let slots = default_slots_for_kind(OpeningKind::Door);

    // Exterior is the negative side of the wall-local Y axis.
    let mut params_ext = params(0.9, OpeningKind::Door, LeafOperation::left());
    params_ext.swing_side = SwingSide::Exterior;
    let exterior = bake_opening_generators(&slots, params_ext, None);

    // Interior swing is the positive side of the wall-local Y axis.
    let mut params_int = params(0.9, OpeningKind::Door, LeafOperation::left());
    params_int.swing_side = SwingSide::Interior;
    let interior = bake_opening_generators(&slots, params_int, None);

    // Extract leaf segments (Leaf2D slot)
    let leaf_ext = exterior
        .iter()
        .find(|p| p.slot == OpeningComponentSlot::Leaf2D)
        .expect("exterior should have Leaf2D");
    let leaf_int = interior
        .iter()
        .find(|p| p.slot == OpeningComponentSlot::Leaf2D)
        .expect("interior should have Leaf2D");

    // Both should have 2 points: start and end
    assert_eq!(leaf_ext.points.len(), 2, "exterior leaf should have 2 points");
    assert_eq!(leaf_int.points.len(), 2, "interior leaf should have 2 points");

    // Start points (hinge) should be identical
    assert!(
        (leaf_ext.points[0].0 - leaf_int.points[0].0).abs() < 1e-9,
        "hinge X should be identical"
    );
    assert!(
        (leaf_ext.points[0].1 - leaf_int.points[0].1).abs() < 1e-9,
        "hinge Y should be identical"
    );

    // End points (leaf tip) should have opposite Y deflection
    let ext_y = leaf_ext.points[1].1;
    let int_y = leaf_int.points[1].1;
    assert!(ext_y < 0.0, "exterior leaf should deflect in -Y");
    assert!(int_y > 0.0, "interior leaf should deflect in +Y");
    assert!((ext_y + int_y).abs() < 1e-9, "Y deflections should be symmetric");

    // Extract swing arcs (Swing2D slot)
    let swing_ext = exterior
        .iter()
        .find(|p| p.slot == OpeningComponentSlot::Swing2D)
        .expect("exterior should have Swing2D");
    let swing_int = interior
        .iter()
        .find(|p| p.slot == OpeningComponentSlot::Swing2D)
        .expect("interior should have Swing2D");

    // Swing arcs should have same number of points
    assert_eq!(
        swing_ext.points.len(),
        swing_int.points.len(),
        "swing arcs should have same point count"
    );

    // Exterior swing stays at/beyond wall-local -Y, interior at/beyond +Y.
    for (i, &(_, y_ext)) in swing_ext.points.iter().enumerate() {
        let (_, y_int) = swing_int.points[i];
        assert!(
            y_ext <= 1e-9,
            "exterior swing arc point {} should have Y >= 0, got {}",
            i,
            y_ext
        );
        assert!(
            y_int >= -1e-9,
            "interior swing arc point {} should have Y <= 0, got {}",
            i,
            y_int
        );
        // Symmetric around Y=0
        assert!(
            (y_ext + y_int).abs() < 1e-9,
            "swing arc point {} Y values should be symmetric, got {} and {}",
            i,
            y_ext,
            y_int
        );
    }
}

#[test]
fn breakthrough_mark2d_has_no_swing_or_frame() {
    let slots = default_slots_for_kind(OpeningKind::Breakthrough);
    let baked = bake_opening_generators(
        &slots,
        params(1.0, OpeningKind::Breakthrough, LeafOperation::left()),
        None,
    );
    assert!(baked.iter().all(|p| {
        p.slot == OpeningComponentSlot::Mark2D
            || p.slot == OpeningComponentSlot::BreakthroughSymbol2D
            || p.slot == OpeningComponentSlot::OpeningLabel2D
            || p.slot == OpeningComponentSlot::ElevationContour2D
    }));
    assert!(baked
        .iter()
        .any(|p| p.slot == OpeningComponentSlot::Mark2D && p.points.len() == 2));
    assert!(!baked
        .iter()
        .any(|p| p.slot == OpeningComponentSlot::Swing2D
            || p.slot == OpeningComponentSlot::Frame2D));
}

#[test]
fn display_config_can_hide_swing() {
    let slots = default_slots_for_kind(OpeningKind::Door);
    let mut rules = ComponentRuleSet::default();
    rules
        .visibility
        .insert(OpeningComponentSlot::Swing2D.key().to_string(), false);
    let baked = bake_opening_generators(
        &slots,
        params(0.9, OpeningKind::Door, LeafOperation::left()),
        Some(&rules),
    );
    assert!(!baked
        .iter()
        .any(|p| p.slot == OpeningComponentSlot::Swing2D));
    assert!(baked
        .iter()
        .any(|p| p.slot == OpeningComponentSlot::Frame2D));
}

#[test]
fn disabled_interior_board_does_not_add_plan_sill_paths_on_inside_face() {
    let mut parts = OpeningParts::default();
    parts.exterior_sill.thickness = 0.05;
    parts.interior_board.enabled = false;
    parts.interior_board.thickness = 0.05;

    let paths = bake_opening_plan_paths(
        params(1.2, OpeningKind::Window, LeafOperation::left()),
        &parts,
        None,
        &DivisionGrid::default(),
        PlanFrameLevel::Off,
        SwingGlyph::Off,
        DivisionLevel::Off,
    );
    let sill_paths: Vec<_> = paths
        .iter()
        .filter(|path| path.slot == OpeningComponentSlot::Sill2D)
        .collect();

    assert!(!sill_paths.is_empty());
    assert!(sill_paths
        .iter()
        .flat_map(|path| &path.points)
        .all(|(_, y)| *y <= 1e-9), "{sill_paths:#?}");
}

#[test]
fn sill_profile_gap_covers_only_the_uncovered_overhang() {
    // Exterior (outward = -1): profile ends at -0.10, overhang wants -0.19.
    assert_eq!(sill_profile_gap(-0.10, -0.19, -1.0), Some((-0.19, -0.10)));
    // Profile already reaches or passes the overhang: no gap.
    assert_eq!(sill_profile_gap(-0.20, -0.19, -1.0), None);
    assert_eq!(sill_profile_gap(-0.19, -0.19, -1.0), None);
    // Interior (outward = +1).
    assert_eq!(sill_profile_gap(0.10, 0.19, 1.0), Some((0.10, 0.19)));
    assert_eq!(sill_profile_gap(0.20, 0.19, 1.0), None);
}

#[test]
fn sill_upstand_interval_stands_outside_frame_and_clamps_to_frame_depth() {
    assert_eq!(sill_upstand_interval(-0.04, -1.0, 0.03, 0.08), (-0.07, -0.04));
    assert_eq!(sill_upstand_interval(0.04, 1.0, 0.03, 0.08), (0.04, 0.07));
    assert_eq!(sill_upstand_interval(0.04, 1.0, 0.2, 0.08), (0.04, 0.12));
}

#[test]
fn sill_accessory_bounds_meet_the_fixed_frame_face() {
    let wall_y_min = -0.15;
    let wall_y_max = 0.15;
    let exterior = exterior_sill_y_bounds(wall_y_min, wall_y_max, 0.04, 0.08);
    let interior = interior_board_y_bounds(wall_y_min, wall_y_max, 0.03, 0.06);
    assert_eq!(exterior, (-0.19, -0.04));
    assert_eq!(interior, (0.03, 0.18));
    assert!(exterior.1 < wall_y_max);
    assert!(interior.0 > wall_y_min);

    let oversized = exterior_sill_y_bounds(wall_y_min, wall_y_max, 0.0, 0.5);
    assert_eq!(oversized, (wall_y_min, wall_y_min));
}

#[test]
fn profile_sill_prism_uses_the_section_shape_and_frame_edge_origin() {
    let section = resolve_section(
        Some(&crate::modules::aec::engine::section_profile::SectionProfile::new(
            "triangular-sill",
            "Triangular sill",
            crate::modules::aec::engine::section_profile::SectionShape::Triangle {
                base: 0.12,
                height: 0.035,
            },
        )),
        0.0,
        0.0,
    );
    let body = build_opening_profile_prism_3d(1.0, &section, -0.04, 1.0)
        .expect("profile section prism");
    let mut x_bounds = (f64::INFINITY, f64::NEG_INFINITY);
    let mut y_bounds = (f64::INFINITY, f64::NEG_INFINITY);
    let mut z_bounds = (f64::INFINITY, f64::NEG_INFINITY);
    for edge_key in body.edge_keys() {
        let edge = body.edges.get(edge_key).expect("body edge");
        for vertex_key in [edge.start, edge.end] {
            let point = body.vertices.get(vertex_key).expect("edge vertex").point;
            x_bounds.0 = x_bounds.0.min(point[0]);
            x_bounds.1 = x_bounds.1.max(point[0]);
            y_bounds.0 = y_bounds.0.min(point[1]);
            y_bounds.1 = y_bounds.1.max(point[1]);
            z_bounds.0 = z_bounds.0.min(point[2]);
            z_bounds.1 = z_bounds.1.max(point[2]);
        }
    }
    assert!((x_bounds.0 + 0.5).abs() < 1e-12);
    assert!((x_bounds.1 - 0.5).abs() < 1e-12);
    assert!((y_bounds.0 + 0.04).abs() < 1e-12);
    assert!((y_bounds.1 + 0.005).abs() < 1e-12);
    assert!(z_bounds.0.abs() < 1e-12);
    assert!((z_bounds.1 - 0.12).abs() < 1e-12);
}

fn add_test_wall(scene: &mut Scene) -> Handle {
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut wall = Wall::new("s", 3.0, 0);
    wall.justification = WallJustification::Center;
    wall.layers = vec![WallLayer {
        material: "Concrete".into(),
        thickness: 0.3,
        function: "Structural".into(),
        axis_offset: -0.15,
        ..WallLayer::default()
    }];
    let mut entity = EntityType::LwPolyline(pl);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    entity.common_mut().extended_data.add_record(record);
    let handle = scene.add_entity(entity);
    regenerate_wall_representation(scene, handle, None).expect("regen");
    handle
}

fn child_slots(scene: &Scene, opening: Handle) -> Vec<OpeningComponentSlot> {
    let mut slots: Vec<_> = collect_opening_display_children(scene, opening)
        .into_iter()
        .filter_map(|h| {
            scene
                .document
                .get_entity(h)
                .and_then(opening_rep_slot_from_entity)
        })
        .collect();
    slots.sort_by_key(|s| s.key());
    slots
}

fn opening_label_handle(scene: &Scene, opening: Handle) -> Option<Handle> {
    collect_opening_display_children(scene, opening)
        .into_iter()
        .find(|handle| {
            matches!(scene.document.get_entity(*handle), Some(EntityType::MText(_)))
                && scene
                    .document
                    .get_entity(*handle)
                    .and_then(opening_rep_slot_from_entity)
                    == Some(OpeningComponentSlot::OpeningLabel2D)
        })
}

#[test]
fn disabled_fixed_frame_keeps_glazing_and_removes_frame_children() {
    let mut scene = Scene::new();
    let wall = add_test_wall(&mut scene);
    let mut lib = seed_default_library();
    let style = lib
        .opening_styles
        .iter_mut()
        .find(|style| style.style.id == SEED_WINDOW_STYLE_ID)
        .expect("seed window style");
    style.parts.fixed_frame.enabled = false;

    let (opening, _) = place_wall_opening(
        &mut scene,
        wall,
        DVec3::new(1.5, 0.0, 0.0),
        OpeningKind::Window,
        Some(&lib),
        None,
        None,
    )
    .expect("place window");
    let slots = child_slots(&scene, opening);

    assert!(!slots.contains(&OpeningComponentSlot::Frame2D));
    assert!(!slots.contains(&OpeningComponentSlot::Frame3D));
    assert!(slots.contains(&OpeningComponentSlot::Glazing2D));
    assert!(slots.contains(&OpeningComponentSlot::Glazing3D));
}

#[test]
fn division_posts_use_the_grid_layer_and_keep_a_changed_layer_color() {
    use crate::modules::aec::engine::construction_grid::{
        ConstructionGrid, GridTrack, TrackMeasure,
    };

    let mut scene = Scene::new();
    let wall = add_test_wall(&mut scene);
    let mut lib = seed_default_library();
    let style = lib
        .opening_styles
        .iter_mut()
        .find(|style| style.style.id == SEED_WINDOW_STYLE_ID)
        .expect("seed window style");
    let fraction = GridTrack {
        measure: TrackMeasure::Fraction(1.0),
        ..GridTrack::default()
    };
    style.grid = ConstructionGrid {
        columns: vec![fraction.clone(), fraction],
        ..ConstructionGrid::default()
    };
    style.looks.division.line_color = None;

    let (opening, _) = place_wall_opening(
        &mut scene,
        wall,
        DVec3::new(1.5, 0.0, 0.0),
        OpeningKind::Window,
        Some(&lib),
        None,
        None,
    )
    .expect("place window");

    let children = collect_opening_display_children(&scene, opening);
    let posts: Vec<_> = children
        .iter()
        .copied()
        .filter(|handle| {
            scene
                .document
                .get_entity(*handle)
                .and_then(opening_rep_slot_from_entity)
                == Some(OpeningComponentSlot::Division2D)
        })
        .collect();
    assert!(!posts.is_empty());
    for handle in &posts {
        let entity = scene.document.get_entity(*handle).expect("post");
        assert_eq!(entity.as_entity().layer(), AEC_OPENING_GRID_LAYER);
        assert!(matches!(
            entity.as_entity().color(),
            acadrust::types::Color::ByLayer
        ));
    }
    let glass = children.iter().copied().find(|handle| {
        scene
            .document
            .get_entity(*handle)
            .and_then(opening_rep_slot_from_entity)
            == Some(OpeningComponentSlot::Glazing2D)
    });
    let glass = glass.expect("glass");
    assert_ne!(
        scene.document.get_entity(glass).unwrap().as_entity().layer(),
        AEC_OPENING_GRID_LAYER
    );
    assert_eq!(
        scene
            .document
            .layers
            .get(AEC_OPENING_GRID_LAYER)
            .expect("grid layer")
            .color,
        acadrust::types::Color::Index(8)
    );

    scene
        .document
        .layers
        .get_mut(AEC_OPENING_GRID_LAYER)
        .expect("grid layer")
        .color = acadrust::types::Color::Index(3);
    regenerate_opening_display(&mut scene, opening, Some(&lib), None);
    assert_eq!(
        scene
            .document
            .layers
            .get(AEC_OPENING_GRID_LAYER)
            .expect("grid layer")
            .color,
        acadrust::types::Color::Index(3)
    );
}

#[test]
fn opening_look_is_written_to_generated_frame_polyline() {
    let mut scene = Scene::new();
    let wall = add_test_wall(&mut scene);
    let mut lib = seed_default_library();
    let color = acadrust::types::Color::Rgb { r: 12, g: 34, b: 56 };
    let style = lib
        .opening_styles
        .iter_mut()
        .find(|style| style.style.id == SEED_WINDOW_STYLE_ID)
        .expect("seed window style");
    style.looks.fixed_frame.line_type = Some("DASHED".into());
    style.looks.fixed_frame.line_color = Some(color);

    let (opening, _) = place_wall_opening(
        &mut scene,
        wall,
        DVec3::new(1.5, 0.0, 0.0),
        OpeningKind::Window,
        Some(&lib),
        None,
        None,
    )
    .expect("place window");
    let frame = collect_opening_display_children(&scene, opening)
        .into_iter()
        .find(|handle| {
            scene
                .document
                .get_entity(*handle)
                .and_then(opening_rep_slot_from_entity)
                == Some(OpeningComponentSlot::Frame2D)
        })
        .expect("frame polyline");
    let entity = scene.document.get_entity(frame).expect("frame entity");

    assert_eq!(entity.common().linetype, "DASHED");
    assert_eq!(entity.common().color, color);
}

#[test]
fn opening_material_look_resolves_native_render_material_when_present() {
    let mut scene = Scene::new();
    let wall = add_test_wall(&mut scene);
    let mut lib = seed_default_library();
    let mut aec_material = crate::modules::aec::engine::material::Material::new(
        "wood-look".into(),
        "Oak finish".into(),
        "ANSI31".into(),
        0x123456,
        "DASHED".into(),
    );
    aec_material.render_material_ref = Some("oak_native".into());
    lib.materials.push(aec_material);
    let style = lib
        .opening_styles
        .iter_mut()
        .find(|style| style.style.id == SEED_WINDOW_STYLE_ID)
        .expect("seed window style");
    style.looks.fixed_frame.material_id = Some("wood-look".into());

    let material_handle = scene.document.allocate_handle();
    let mut native = codec::objects::Material::new();
    native.handle = material_handle;
    native.name = "oak_native".into();
    scene.document.objects.insert(
        material_handle,
        codec::objects::ObjectType::Material(native),
    );

    let (opening, _) = place_wall_opening(
        &mut scene,
        wall,
        DVec3::new(1.5, 0.0, 0.0),
        OpeningKind::Window,
        Some(&lib),
        None,
        None,
    )
    .expect("place window");
    let children = collect_opening_display_children(&scene, opening);
    let frame = children
        .iter()
        .find(|handle| {
            scene
                .document
                .get_entity(**handle)
                .and_then(opening_rep_slot_from_entity)
                == Some(OpeningComponentSlot::Frame2D)
        })
        .copied()
        .expect("frame polyline");
    let frame_entity = scene.document.get_entity(frame).unwrap();
    assert_eq!(frame_entity.common().linetype, "DASHED");
    assert_eq!(
        frame_entity.common().color,
        acadrust::types::Color::Rgb {
            r: 0x12,
            g: 0x34,
            b: 0x56,
        }
    );

    let frame_solid = children
        .into_iter()
        .find(|handle| {
            matches!(scene.document.get_entity(*handle), Some(EntityType::Solid3D(_)))
                && scene
                    .document
                    .get_entity(*handle)
                    .and_then(opening_rep_slot_from_entity)
                    == Some(OpeningComponentSlot::Frame3D)
        })
        .expect("frame solid");
    let solid = scene.document.get_entity(frame_solid).unwrap();
    assert_eq!(solid.common().material_flags, 3);
    assert_eq!(solid.common().material_handle, Some(material_handle));
}

#[test]
fn plan_style_override_wins_over_opening_look() {
    let mut scene = Scene::new();
    let wall = add_test_wall(&mut scene);
    let mut lib = seed_default_library();
    let style = lib
        .opening_styles
        .iter_mut()
        .find(|style| style.style.id == SEED_WINDOW_STYLE_ID)
        .expect("seed window style");
    style.looks.fixed_frame.line_type = Some("DASHED".into());
    let override_color = acadrust::types::Color::Rgb { r: 200, g: 10, b: 20 };
    let mut rules = ComponentRuleSet::default();
    rules.style_override.insert(
        OpeningComponentSlot::Frame2D.key().to_string(),
        crate::modules::aec::engine::display_component::ComponentStyleOverride {
            line_type: Some("HIDDEN".into()),
            line_color: Some(override_color),
            ..Default::default()
        },
    );

    let (opening, _) = place_wall_opening(
        &mut scene,
        wall,
        DVec3::new(1.5, 0.0, 0.0),
        OpeningKind::Window,
        Some(&lib),
        Some(&rules),
        None,
    )
    .expect("place window");
    let frame = collect_opening_display_children(&scene, opening)
        .into_iter()
        .find(|handle| {
            scene
                .document
                .get_entity(*handle)
                .and_then(opening_rep_slot_from_entity)
                == Some(OpeningComponentSlot::Frame2D)
        })
        .expect("frame polyline");
    let entity = scene.document.get_entity(frame).expect("frame entity");

    assert_eq!(entity.common().linetype, "HIDDEN");
    assert_eq!(entity.common().color, override_color);
}

#[test]
fn opening_plan_mullions_do_not_depend_on_elevation_slot() {
    let mut slots = default_slots_for_kind(OpeningKind::Window);
    slots.remove(&OpeningComponentSlot::ElevationMuntins2D);
    let grid = DivisionGrid {
        vertical: vec![0.5, 0.5],
        cells: vec![OpeningCell::default(), OpeningCell::default()],
        ..DivisionGrid::default()
    };

    assert_eq!(
        representation_levels(&slots, &grid, None).divisions,
        DivisionLevel::Posts
    );
    assert_eq!(
        representation_levels(&slots, &grid, Some(DivisionLevel::Off)).divisions,
        DivisionLevel::Off
    );
}

#[test]
fn positive_lintel_height_emits_a_solid3d_child() {
    let mut scene = Scene::new();
    let wall = add_test_wall(&mut scene);
    let mut lib = seed_default_library();
    let style = lib
        .opening_styles
        .iter_mut()
        .find(|style| style.style.id == SEED_WINDOW_STYLE_ID)
        .expect("seed window style");
    style.parts.lintel.height = 0.2;
    style.parts.lintel.bearing_left = 0.1;
    style.parts.lintel.bearing_right = 0.1;
    let lintel_color = acadrust::types::Color::Rgb { r: 90, g: 120, b: 150 };
    style.looks.lintel.line_color = Some(lintel_color);

    let (opening, _) = place_wall_opening(
        &mut scene,
        wall,
        DVec3::new(1.5, 0.0, 0.0),
        OpeningKind::Window,
        Some(&lib),
        None,
        None,
    )
    .expect("place window");

    let lintel = collect_opening_display_children(&scene, opening)
        .into_iter()
        .find(|handle| {
            matches!(scene.document.get_entity(*handle), Some(EntityType::Solid3D(_)))
                && scene
                    .document
                    .get_entity(*handle)
                    .and_then(opening_rep_slot_from_entity)
                    == Some(OpeningComponentSlot::Solid3D)
        })
        .expect("lintel solid");
    assert_eq!(
        scene.document.get_entity(lintel).unwrap().common().color,
        lintel_color
    );
}

#[test]
fn zero_accessory_dimensions_emit_no_solid3d_child() {
    let mut scene = Scene::new();
    let wall = add_test_wall(&mut scene);
    let lib = seed_default_library();

    let (opening, _) = place_wall_opening(
        &mut scene,
        wall,
        DVec3::new(1.5, 0.0, 0.0),
        OpeningKind::Window,
        Some(&lib),
        None,
        None,
    )
    .expect("place window");

    assert!(!collect_opening_display_children(&scene, opening)
        .into_iter()
        .any(|handle| {
            matches!(scene.document.get_entity(handle), Some(EntityType::Solid3D(_)))
                && scene
                    .document
                    .get_entity(handle)
                    .and_then(opening_rep_slot_from_entity)
                    == Some(OpeningComponentSlot::Solid3D)
        }));
}

#[test]
fn disabled_lintel_emits_no_solid3d_child() {
    let mut scene = Scene::new();
    let wall = add_test_wall(&mut scene);
    let mut lib = seed_default_library();
    let style = lib
        .opening_styles
        .iter_mut()
        .find(|style| style.style.id == SEED_WINDOW_STYLE_ID)
        .expect("seed window style");
    style.parts.lintel.height = 0.2;
    style.parts.lintel.enabled = false;

    let (opening, _) = place_wall_opening(
        &mut scene,
        wall,
        DVec3::new(1.5, 0.0, 0.0),
        OpeningKind::Window,
        Some(&lib),
        None,
        None,
    )
    .expect("place window");

    assert!(!collect_opening_display_children(&scene, opening)
        .into_iter()
        .any(|handle| {
            matches!(scene.document.get_entity(handle), Some(EntityType::Solid3D(_)))
                && scene
                    .document
                    .get_entity(handle)
                    .and_then(opening_rep_slot_from_entity)
                    == Some(OpeningComponentSlot::Solid3D)
        }));
}

#[test]
fn place_window_emits_opening_rep_children_and_resolves_pick() {
    let mut scene = Scene::new();
    let wall = add_test_wall(&mut scene);
    let lib = seed_default_library();
    let (opening, _) = place_wall_opening(
        &mut scene,
        wall,
        DVec3::new(1.5, 0.0, 0.0),
        OpeningKind::Window,
        Some(&lib),
        None,
        None,
    )
    .expect("place");
    let slots = child_slots(&scene, opening);
    assert!(slots.contains(&OpeningComponentSlot::Frame2D));
    assert!(slots.contains(&OpeningComponentSlot::Leaf2D));
    assert!(!slots.contains(&OpeningComponentSlot::Sill2D));
    assert!(!slots.contains(&OpeningComponentSlot::Swing2D));
    let child = collect_opening_display_children(&scene, opening)[0];
    assert_eq!(resolve_opening_package(&scene, child), opening);
    assert_eq!(
        engine::wall_package::resolve_wall_package(&scene, child),
        child
    );
    let parsed = opening_from_entity(scene.document.get_entity(opening).unwrap(), opening)
        .unwrap();
    assert_eq!(parsed.style_id.as_deref(), Some(SEED_WINDOW_STYLE_ID));
    assert!((parsed.width - OpeningStyle::standard_window().default_width).abs() < 1e-12);
}

#[test]
fn opening_label_keeps_its_native_annotation_scale_after_regeneration() {
    let mut scene = Scene::new();
    scene.document.header.current_annotation_scale = "1:50".to_string();
    let native_scale = crate::scene::annotative::ensure_scale_object(
        &mut scene.document,
        &codec::objects::Scale::new("1:50", 1.0, 50.0),
    );
    let wall = add_test_wall(&mut scene);
    let lib = seed_default_library();
    let (opening, _) = place_wall_opening(
        &mut scene,
        wall,
        DVec3::new(1.5, 0.0, 0.0),
        OpeningKind::Window,
        Some(&lib),
        None,
        None,
    )
    .expect("place window");

    let first_label = opening_label_handle(&scene, opening).expect("first opening label");
    assert!(matches!(
        scene.document.get_entity(first_label),
        Some(EntityType::MText(mtext)) if mtext.is_annotative
    ));
    assert_eq!(
        engine::annotative_label::native_annotation_scale(&scene.document, first_label),
        Some(native_scale)
    );

    scene.document.header.current_annotation_scale = "1:20".to_string();
    let _ = crate::scene::annotative::ensure_scale_object(
        &mut scene.document,
        &codec::objects::Scale::new("1:20", 1.0, 20.0),
    );
    regenerate_opening_display(&mut scene, opening, Some(&lib), None);

    let second_label = opening_label_handle(&scene, opening).expect("regenerated opening label");
    assert_eq!(
        engine::annotative_label::native_annotation_scale(&scene.document, second_label),
        Some(native_scale)
    );
    assert!(matches!(
        scene.document.get_entity(second_label),
        Some(EntityType::MText(mtext))
            if mtext.is_annotative && (mtext.height - 0.15).abs() < 1.0e-9
    ));
    assert_eq!(
        crate::scene::annotative::object_scale_memberships(&scene.document, second_label).len(),
        1
    );
}

#[test]
fn place_breakthrough_has_mark_not_frame() {
    let mut scene = Scene::new();
    let wall = add_test_wall(&mut scene);
    let lib = seed_default_library();
    let (opening, _) = place_wall_opening(
        &mut scene,
        wall,
        DVec3::new(2.0, 0.0, 0.0),
        OpeningKind::Breakthrough,
        Some(&lib),
        None,
        None,
    )
    .expect("place");
    let slots = child_slots(&scene, opening);
    assert!(slots.contains(&OpeningComponentSlot::Mark2D));
    assert!(!slots.contains(&OpeningComponentSlot::Frame2D));
    assert!(!slots.contains(&OpeningComponentSlot::Swing2D));
}

#[test]
fn legacy_opening_without_style_id_uses_kind_generators() {
    let mut scene = Scene::new();
    let wall = add_test_wall(&mut scene);
    let (opening, _) = place_wall_opening(
        &mut scene,
        wall,
        DVec3::new(1.5, 0.0, 0.0),
        OpeningKind::Window,
        None,
        None,
        None,
    )
    .expect("place");
    let parsed = opening_from_entity(scene.document.get_entity(opening).unwrap(), opening)
        .unwrap();
    assert_eq!(parsed.style_id, None);
    assert!((parsed.width - DEFAULT_WINDOW_WIDTH).abs() < 1e-12);
    let slots = child_slots(&scene, opening);
    assert!(slots.contains(&OpeningComponentSlot::Frame2D));
    assert!(!slots.contains(&OpeningComponentSlot::Sill2D));
}

#[test]
fn axis_width_grip_keeps_frame_inset() {
    let axis = vec![(0.0, 0.0), (5.0, 0.0)];
    let mut opening = Opening::window(Handle::new(1), Handle::new(2), 2.0);
    opening.width = 1.2;
    apply_opening_axis_grip(&axis, &mut opening, 2, DVec3::new(3.2, 0.0, 0.0));
    assert!((opening.width - 1.8).abs() < 1e-9);
    let slots = default_slots_for_kind(OpeningKind::Window);
    let baked = bake_opening_generators(
        &slots,
        params(opening.width, OpeningKind::Window, LeafOperation::left()),
        None,
    );
    let inner = inner_frame_half_width(&baked).unwrap();
    assert!((opening.width * 0.5 - inner - DEFAULT_FRAME_THICKNESS).abs() < 1e-9);
}

#[test]
fn lintel_and_shutter_start_at_the_rough_head_inside_the_wall() {
    let wall_min = -0.15;
    let wall_max = 0.15;
    let head = 1.2;
    assert!((accessory_bottom(head, 0.04) - 1.24).abs() < 1e-12);
    assert!((accessory_bottom(head, -0.2) - head).abs() < 1e-12);

    let (y0, y1) = wall_clamped_interval(wall_min, wall_max, wall_min - 0.05, 0.4);
    assert!((y0 - wall_min).abs() < 1e-12);
    assert!((y1 - wall_max).abs() < 1e-12);

    let (lintel_y0, lintel_y1) = lintel_depth_interval(wall_min, wall_max, 0.5);
    assert!(lintel_y0 >= wall_min - 1e-12);
    assert!(lintel_y1 <= wall_max + 1e-12);
    assert!(lintel_y1 > lintel_y0);
}

#[test]
fn side_overhangs_extend_only_the_named_sill_and_side() {
    let mut parts = OpeningParts::default();
    parts.exterior_sill.thickness = 0.04;
    parts.exterior_sill.overhang = 0.02;
    parts.exterior_sill.side_overhang_left = 0.05;
    parts.exterior_sill.side_overhang_right = 0.0;
    parts.interior_board.thickness = 0.03;
    parts.interior_board.overhang = 0.02;
    parts.interior_board.side_overhang_left = 0.0;
    parts.interior_board.side_overhang_right = 0.08;
    let section = ResolvedSection {
        outline: Vec::new(),
        face: 0.04,
        depth: 0.06,
    };
    let baked = bake_sill_lines_from_parts(
        OpeningComponentSlot::Sill2D,
        params(1.2, OpeningKind::Window, LeafOperation::left()),
        &parts,
        None,
        &section,
        &section,
        &section,
    );
    let span = |role: OpeningLookRole| {
        let path = baked
            .iter()
            .find(|path| path.look_role == Some(role) && path.closed)
            .expect("sill nose");
        let min_x = path.points.iter().map(|point| point.0).fold(f64::INFINITY, f64::min);
        let max_x = path.points.iter().map(|point| point.0).fold(f64::NEG_INFINITY, f64::max);
        (min_x, max_x)
    };
    let (ext_min, ext_max) = span(OpeningLookRole::ExteriorSill);
    let (int_min, int_max) = span(OpeningLookRole::InteriorBoard);
    assert!((ext_min - (-0.6 - 0.05)).abs() < 1e-12);
    assert!((ext_max - 0.6).abs() < 1e-12);
    assert!((int_min - -0.6).abs() < 1e-12);
    assert!((int_max - (0.6 + 0.08)).abs() < 1e-12);
}

#[test]
fn exterior_swing_points_to_the_exterior_edge() {
    let mut baked = params(1.2, OpeningKind::Door, LeafOperation::left());
    baked.swing_side = SwingSide::Exterior;
    baked.wall_y_min = -0.15;
    baked.wall_y_max = 0.15;
    let exterior = bake_swing_arc(OpeningComponentSlot::Swing2D, baked, 0.6, 0.06);
    assert!(exterior[0].points.last().unwrap().1 < 0.0);
    baked.swing_side = SwingSide::Interior;
    let interior = bake_swing_arc(OpeningComponentSlot::Swing2D, baked, 0.6, 0.06);
    assert!(interior[0].points.last().unwrap().1 > 0.0);
    assert!((swing_toward_exterior(SwingSide::Exterior) + 1.0).abs() < 1e-12);
    assert!((swing_toward_exterior(SwingSide::Interior) - 1.0).abs() < 1e-12);
}

#[test]
fn nested_grid_bakes_an_inner_post_and_one_leaf_per_child() {
    use crate::modules::aec::engine::construction_grid::{
        CellFill, ConstructionGrid, GridCell, GridTrack, TrackMeasure,
    };

    let fraction = GridTrack {
        measure: TrackMeasure::Fraction(1.0),
        ..GridTrack::default()
    };
    let grid = ConstructionGrid {
        cells: vec![GridCell {
            columns: 1,
            rows: 1,
            fill: CellFill::Nested(ConstructionGrid {
                columns: vec![fraction.clone(), fraction],
                cells: vec![
                    GridCell {
                        fill: CellFill::Glass,
                        operation: Some(LeafOperation::left()),
                        ..GridCell::default()
                    },
                    GridCell {
                        column: 1,
                        fill: CellFill::Glass,
                        operation: Some(LeafOperation::right()),
                        ..GridCell::default()
                    },
                ],
                ..ConstructionGrid::default()
            }),
            ..GridCell::default()
        }],
        ..ConstructionGrid::default()
    };
    let params = params(1.2, OpeningKind::Window, LeafOperation::left());
    let posts = bake_grid_plan_divisions(
        &grid,
        params,
        DivisionLevel::Posts,
        None,
        &ResolvedSection {
            outline: Vec::new(),
            face: 0.04,
            depth: 0.06,
        },
        &ResolvedSection {
            outline: Vec::new(),
            face: 0.04,
            depth: 0.04,
        },
    );
    assert_eq!(posts.len(), 1);
    assert!((posts[0].points[0].0).abs() < 1e-9);
    let leaves = bake_grid_leaf_lines(&grid, params, 0.06, 0.04, 0.04);
    assert_eq!(leaves.len(), 2);
}

#[test]
fn glazing_line_bites_into_a_rendered_bar_and_stays_flush_without_one() {
    let grid = ConstructionGrid::default();
    let lines = bake_grid_glazing_lines(
        &grid,
        params(1.2, OpeningKind::Window, LeafOperation::left()),
        0.01,
        0.06,
    );
    let (left, right) = (lines[0].points[0].0, lines[0].points[1].0);
    assert!((left - (-0.6 + 0.05)).abs() < 1e-12);
    assert!((right - (0.6 - 0.05)).abs() < 1e-12);
    assert!((glass_edge_into_bar(0.0, 1.0, 0.0, 0.02) - 0.0).abs() < 1e-12);
}

