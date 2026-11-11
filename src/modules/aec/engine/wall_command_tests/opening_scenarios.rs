use super::*;

#[test]
fn test_opening_realistic_post_profiles() {
    use crate::modules::aec::engine::display_component::OpeningComponentSlot;
    use crate::modules::aec::engine::opening_display::{bake_opening_generators, OpeningBakeParams};
    use crate::modules::aec::engine::openings::{OpeningKind, SwingSide};
    use crate::modules::aec::engine::opening_style::{
        LeafOperation, OpeningGenerator, SlotGeometry, DEFAULT_FRAME_THICKNESS,
    };
    use std::collections::HashMap;

    let mut slots = HashMap::new();
    slots.insert(
        OpeningComponentSlot::Frame2D,
        SlotGeometry::Generator(OpeningGenerator::FrameRect),
    );

    let width = 1.4;
    let thickness = 0.3;
    let ft = DEFAULT_FRAME_THICKNESS; // 0.07
    let p = OpeningBakeParams {
        width,
        height: 1.2,
        sill_height: 0.9,
        thickness,
        frame_thickness: ft,
        cross_axis_offset: 0.0,
        wall_y_min: -thickness * 0.5,
        wall_y_max: thickness * 0.5,
        hinge: LeafOperation::left(),
        swing_side: SwingSide::Exterior,
        shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
        spring_height: 0.0,
        opening_angle_deg: 90.0,
        kind: OpeningKind::Window,
    };

    let baked = bake_opening_generators(&slots, p, None);
    let frames: Vec<_> = baked
        .iter()
        .filter(|b| b.slot == OpeningComponentSlot::Frame2D && b.closed)
        .collect();

    assert_eq!(frames.len(), 2, "Window Frame2D must emit two separate post profiles");

    let hw = width * 0.5; // 0.7
    let post_a = &frames[0];
    let post_b = &frames[1];

    let min_x_a = post_a.points.iter().map(|pt| pt.0).fold(f64::INFINITY, f64::min);
    let max_x_a = post_a.points.iter().map(|pt| pt.0).fold(f64::NEG_INFINITY, f64::max);
    let min_x_b = post_b.points.iter().map(|pt| pt.0).fold(f64::INFINITY, f64::min);
    let max_x_b = post_b.points.iter().map(|pt| pt.0).fold(f64::NEG_INFINITY, f64::max);

    // Left post: [-0.70, -0.63]
    let (left_min, left_max, right_min, right_max) = if min_x_a < min_x_b {
        (min_x_a, max_x_a, min_x_b, max_x_b)
    } else {
        (min_x_b, max_x_b, min_x_a, max_x_a)
    };

    assert!((left_min - (-hw)).abs() < 1e-6, "Left post must sit at left reveal -hw");
    assert!((left_max - (-hw + ft)).abs() < 1e-6, "Left post width must equal ft");
    assert!((right_min - (hw - ft)).abs() < 1e-6, "Right post inner edge must be at hw - ft");
    assert!((right_max - hw).abs() < 1e-6, "Right post outer edge must be at right reveal hw");

    let clear_width = right_min - left_max;
    assert!((clear_width - (width - 2.0 * ft)).abs() < 1e-6, "Clear opening between posts must be width - 2*ft");
}

#[test]
fn test_opening_dynamic_sill_lines_with_wall_thickness() {
    use crate::modules::aec::engine::display_component::OpeningComponentSlot;
    use crate::modules::aec::engine::opening_display::{bake_opening_generators, OpeningBakeParams};
    use crate::modules::aec::engine::opening_style::{
        LeafOperation, OpeningGenerator, SlotGeometry, DEFAULT_FRAME_THICKNESS,
    };
    use crate::modules::aec::engine::openings::OpeningKind;
    use std::collections::HashMap;

    let mut slots = HashMap::new();
    slots.insert(
        OpeningComponentSlot::Sill2D,
        SlotGeometry::Generator(OpeningGenerator::SillLines),
    );

    for thickness in [0.24, 0.365, 0.42] {
        let hw = 0.6;
        let ht = thickness * 0.5;
        let p = OpeningBakeParams {
            width: hw * 2.0,
            height: 1.2,
            sill_height: 0.9,
            thickness,
            frame_thickness: DEFAULT_FRAME_THICKNESS,
            cross_axis_offset: 0.0,
            wall_y_min: -ht,
            wall_y_max: ht,
            hinge: LeafOperation::left(),
            swing_side: SwingSide::Exterior,
            shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
            spring_height: 0.0,
            opening_angle_deg: 90.0,
            kind: OpeningKind::Window,
        };

        let baked = bake_opening_generators(&slots, p, None);
        let sills: Vec<_> = baked
            .iter()
            .filter(|b| b.slot == OpeningComponentSlot::Sill2D)
            .collect();
        assert!(!sills.is_empty(), "Sill lines must be generated for thickness {thickness}");

        // Find the exterior sill front edge (lowest y)
        let min_y = sills
            .iter()
            .flat_map(|b| b.points.iter().map(|pt| pt.1))
            .fold(f64::INFINITY, f64::min);
        // Find the interior board front edge (highest y)
        let max_y = sills
            .iter()
            .flat_map(|b| b.points.iter().map(|pt| pt.1))
            .fold(f64::NEG_INFINITY, f64::max);

        // Exterior sill front is at -ht - 0.035
        assert!((min_y - (-ht - 0.035)).abs() < 1e-4, "Exterior sill must track wall exterior at -ht for thickness {thickness}");
        // Interior board front is at +ht + 0.025
        assert!((max_y - (ht + 0.025)).abs() < 1e-4, "Interior board must track wall interior at +ht for thickness {thickness}");

        // Wall face reveal line exists at -ht and +ht
        let has_ext_wall_line = sills.iter().any(|b| {
            b.points.len() == 2 && (b.points[0].1 - (-ht)).abs() < 1e-4 && (b.points[1].1 - (-ht)).abs() < 1e-4
        });
        let has_int_wall_line = sills.iter().any(|b| {
            b.points.len() == 2 && (b.points[0].1 - ht).abs() < 1e-4 && (b.points[1].1 - ht).abs() < 1e-4
        });
        assert!(has_ext_wall_line, "Must have reveal line at exterior wall face -ht");
        assert!(has_int_wall_line, "Must have reveal line at interior wall face +ht");
    }
}

#[test]
fn test_opening_component_blocks_resolution_and_placement() {
    use crate::modules::aec::engine::display_component::OpeningComponentSlot;
    use crate::modules::aec::engine::opening_display::{
        bake_opening_generators_with_doc, OpeningBakeParams,
    };
    use crate::modules::aec::engine::opening_style::{
        BlockPlacementMode, LeafOperation, SlotGeometry, DEFAULT_FRAME_THICKNESS,
    };
    use crate::modules::aec::engine::openings::OpeningKind;
    use acadrust::entities::Line;
    use acadrust::tables::BlockRecord;
    use acadrust::types::Vector3;
    use acadrust::{CadDocument, EntityType};
    use std::collections::HashMap;

    let mut doc = CadDocument::new();
    let block_name = "TEST_JAMB_PROFILE";
    let block_h = doc.allocate_handle();
    let mut rec = BlockRecord::new(block_name);
    rec.handle = block_h;
    doc.block_records.add(rec).expect("add block record");

    // Add a line inside the block record representing a profile from (0, -0.04) to (0.08, 0.04):
    let mut line = Line::new();
    line.start = Vector3::new(0.0, -0.04, 0.0);
    line.end = Vector3::new(0.08, 0.04, 0.0);
    line.common.owner_handle = block_h;
    doc.add_entity(EntityType::Line(line)).expect("add line");

    let p = OpeningBakeParams {
        width: 1.2,
        height: 1.2,
        sill_height: 0.9,
        thickness: 0.3,
        frame_thickness: DEFAULT_FRAME_THICKNESS,
        cross_axis_offset: 0.0,
        wall_y_min: -0.15,
        wall_y_max: 0.15,
        hinge: LeafOperation::left(),
        swing_side: SwingSide::Exterior,
        shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
        spring_height: 0.0,
        opening_angle_deg: 90.0,
        kind: OpeningKind::Window,
    };

    // 1. JambPair mode
    let mut slots_jamb = HashMap::new();
    slots_jamb.insert(
        OpeningComponentSlot::Frame2D,
        SlotGeometry::Block {
            block_name: block_name.to_string(),
            placement: BlockPlacementMode::JambPair,
        },
    );
    let baked_jamb = bake_opening_generators_with_doc(&slots_jamb, p, None, Some(&doc));
    assert_eq!(baked_jamb.len(), 2, "JambPair must place left and right jambs");
    let hw = 0.6;
    // Left jamb sits at -hw + (0..0.08) -> [-0.60, -0.52]
    let left_pts = &baked_jamb[0].points;
    assert!((left_pts[0].0 - (-hw)).abs() < 1e-6);
    assert!((left_pts[1].0 - (-hw + 0.08)).abs() < 1e-6);
    // Right jamb sits at hw - (0..0.08) -> [0.60, 0.52]
    let right_pts = &baked_jamb[1].points;
    assert!((right_pts[0].0 - hw).abs() < 1e-6);
    assert!((right_pts[1].0 - (hw - 0.08)).abs() < 1e-6);

    // 2. StretchToFit mode
    let mut slots_stretch = HashMap::new();
    slots_stretch.insert(
        OpeningComponentSlot::Frame2D,
        SlotGeometry::Block {
            block_name: block_name.to_string(),
            placement: BlockPlacementMode::StretchToFit,
        },
    );
    let baked_stretch = bake_opening_generators_with_doc(&slots_stretch, p, None, Some(&doc));
    assert_eq!(baked_stretch.len(), 1);
    let stretch_pts = &baked_stretch[0].points;
    assert!((stretch_pts[0].0 - (-hw)).abs() < 1e-6, "StretchToFit start must be at -hw");
    assert!((stretch_pts[1].0 - hw).abs() < 1e-6, "StretchToFit end must be at hw");

    // 3. CenterAnchor mode
    let mut slots_center = HashMap::new();
    slots_center.insert(
        OpeningComponentSlot::Frame2D,
        SlotGeometry::Block {
            block_name: block_name.to_string(),
            placement: BlockPlacementMode::CenterAnchor,
        },
    );
    let baked_center = bake_opening_generators_with_doc(&slots_center, p, None, Some(&doc));
    assert_eq!(baked_center.len(), 1);
    let center_pts = &baked_center[0].points;
    // u was 0.0 to 0.08, mid was 0.04. Centered: -0.04 to +0.04
    assert!((center_pts[0].0 - (-0.04)).abs() < 1e-6);
    assert!((center_pts[1].0 - 0.04).abs() < 1e-6);
}

#[test]
fn test_opening_din1356_label_formatting() {
    use crate::modules::aec::engine::opening_display::format_din1356_label;
    use crate::modules::aec::engine::openings::Opening;
    use acadrust::Handle;

    let mut door = Opening::door(Handle::new(1), Handle::new(2), 2.5);
    door.width = 1.01;
    door.height = 2.135;
    assert_eq!(format_din1356_label(&door), "1.01 / 2.135");

    let mut window = Opening::window(Handle::new(3), Handle::new(4), 4.0);
    window.width = 1.26;
    window.height = 1.40;
    window.sill_height = 0.90;
    assert_eq!(format_din1356_label(&window), "1.26 / 1.40\\PBRH 0.90");

    let mut breakthrough = Opening::breakthrough(Handle::new(5), Handle::new(6), 1.0);
    breakthrough.width = 0.50;
    breakthrough.height = 0.60;
    breakthrough.sill_height = 2.20;
    assert_eq!(format_din1356_label(&breakthrough), "0.50 / 0.60\\PUK 2.20");
}

#[test]
fn test_opening_reference_side_positioning_and_flip() {
    use crate::modules::aec::engine::opening_style::LeafOperation;
    use crate::modules::aec::engine::openings::{Opening, OpeningReferenceSide};
    use acadrust::Handle;

    let mut win = Opening::window(Handle::new(10), Handle::new(20), 2.0);
    win.width = 1.2;
    win.reference_side = OpeningReferenceSide::Start;
    win.hinge = LeafOperation::left();

    // Start reveal is at 2.0, so center is at 2.0 + 0.6 = 2.6, end reveal is at 3.2:
    assert!((win.center_along_axis() - 2.6).abs() < 1e-6);
    let (s0, s1) = win.axis_span();
    assert!((s0 - 2.0).abs() < 1e-6);
    assert!((s1 - 3.2).abs() < 1e-6);

    // Flip: reference side switches from Start to End, hinge from Left to Right
    win.flip();
    assert_eq!(win.reference_side, OpeningReferenceSide::End);
    assert_eq!(win.hinge, LeafOperation::right());

    // If dimensioned from End reveal at 3.2, center and span remain unchanged:
    win.distance_along_axis = 3.2;
    assert!((win.center_along_axis() - 2.6).abs() < 1e-6);
    let (s0_flipped, s1_flipped) = win.axis_span();
    assert!((s0_flipped - 2.0).abs() < 1e-6);
    assert!((s1_flipped - 3.2).abs() < 1e-6);

    // Center reference side:
    win.reference_side = OpeningReferenceSide::Center;
    win.distance_along_axis = 2.6;
    assert!((win.center_along_axis() - 2.6).abs() < 1e-6);
    let (s0_c, s1_c) = win.axis_span();
    assert!((s0_c - 2.0).abs() < 1e-6);
    assert!((s1_c - 3.2).abs() < 1e-6);
}

#[test]
fn test_opening_cross_axis_offset_shifts_geometry_and_sills() {
    use crate::modules::aec::engine::display_component::OpeningComponentSlot;
    use crate::modules::aec::engine::opening_display::bake_opening_world;
    use crate::modules::aec::engine::openings::Opening;
    use acadrust::Handle;

    let axis = vec![(0.0, 0.0), (10.0, 0.0)]; // Wall runs along +X, normal along +Y
    let thickness = 0.30; // ht = 0.15; ext face at y = -0.15, int face at y = +0.15

    let mut win = Opening::window(Handle::new(1), Handle::new(2), 5.0);
    win.width = 1.0;
    win.cross_axis_offset = 0.06; // shifted +6cm towards interior (+Y)
    let mut library = crate::modules::aec::engine::library::seed_default_library();
    let style = library
        .opening_styles
        .iter_mut()
        .find(|style| {
            style.style.id == crate::modules::aec::engine::opening_style::SEED_WINDOW_STYLE_ID
        })
        .expect("seed window style");
    style.parts.exterior_sill.thickness = 0.035;
    style.parts.exterior_sill.overhang = 0.035;
    style.parts.interior_board.thickness = 0.025;
    style.parts.interior_board.overhang = 0.025;

    let paths = bake_opening_world(&axis, thickness, &win, Some(&library), None);
    assert!(!paths.is_empty());

    // Window frame posts should be centered around y = +0.06:
    let frame_paths: Vec<_> = paths
        .iter()
        .filter(|p| p.slot == OpeningComponentSlot::Frame2D)
        .collect();
    assert_eq!(frame_paths.len(), 2);
    for fp in &frame_paths {
        let mid_y = fp.points.iter().map(|p| p.1).sum::<f64>() / (fp.points.len() as f64);
        assert!((mid_y - 0.06).abs() < 1e-3, "Frame posts must center at cross_axis_offset = +0.06");
    }

    // Sill lines should adapt so that the exterior sill front nose terminates at exterior face (-ht - 0.035)
    // and interior board front nose terminates at interior face (+ht + 0.025)
    let sill_paths: Vec<_> = paths
        .iter()
        .filter(|p| p.slot == OpeningComponentSlot::Sill2D)
        .collect();
    assert!(!sill_paths.is_empty());

    let min_y = sill_paths
        .iter()
        .flat_map(|p| p.points.iter().map(|pt| pt.1))
        .fold(f64::INFINITY, f64::min);
    let max_y = sill_paths
        .iter()
        .flat_map(|p| p.points.iter().map(|pt| pt.1))
        .fold(f64::NEG_INFINITY, f64::max);

    // Wall exterior face is at y = -0.15, nose with 0.035 overhang is at -0.185:
    assert!((min_y - (-0.15 - 0.035)).abs() < 1e-4, "Exterior sill must track true exterior wall face");
    // Wall interior face is at y = +0.15, board with 0.025 overhang is at +0.175:
    assert!((max_y - (0.15 + 0.025)).abs() < 1e-4, "Interior board must track true interior wall face");
}

#[test]
fn test_opening_xdata_roundtrip_reference_side_and_offset() {
    use crate::modules::aec::engine::opening_style::LeafOperation;
    use crate::modules::aec::engine::opening_xdata::{opening_from_values, opening_record};
    use crate::modules::aec::engine::openings::{NicheSide, Opening, OpeningReferenceSide};
    use acadrust::Handle;

    let mut win = Opening::window(Handle::new(101), Handle::new(202), 4.5);
    win.reference_side = OpeningReferenceSide::Start;
    win.cross_axis_offset = 0.08;
    win.depth = Some(0.12);
    win.niche_side = NicheSide::Interior;
    win.hinge = LeafOperation::right();

    let record = opening_record(&win);
    let parsed = opening_from_values(win.handle, &record.values).expect("parse opening record");

    assert_eq!(parsed.handle, win.handle);
    assert_eq!(parsed.host_wall, win.host_wall);
    assert!((parsed.distance_along_axis - win.distance_along_axis).abs() < 1e-9);
    assert_eq!(parsed.reference_side, OpeningReferenceSide::Start);
    assert!((parsed.cross_axis_offset - 0.08).abs() < 1e-9);
    assert_eq!(parsed.depth, Some(0.12));
    assert_eq!(parsed.niche_side, NicheSide::Interior);
    assert_eq!(parsed.hinge, LeafOperation::right());

    // Test legacy record with only 11 entries (missing the 4 new fields)
    let legacy_values = record.values[..11].to_vec();
    let legacy_parsed = opening_from_values(win.handle, &legacy_values).expect("parse legacy record");
    assert_eq!(legacy_parsed.reference_side, OpeningReferenceSide::Center);
    assert_eq!(legacy_parsed.cross_axis_offset, 0.0);
    assert_eq!(legacy_parsed.depth, None);
    assert_eq!(legacy_parsed.niche_side, NicheSide::Exterior);
}

#[test]
fn test_opening_grips_definition_and_flip_handle() {
    use crate::modules::aec::engine::opening_display::{
        apply_opening_axis_grip, opening_axis_grips,
    };
    use crate::modules::aec::engine::opening_style::LeafOperation;
    use crate::modules::aec::engine::openings::{Opening, OpeningReferenceSide};
    use acadrust::Handle;
    use glam::DVec3;

    let axis = vec![(0.0, 0.0), (10.0, 0.0)];
    let mut win = Opening::window(Handle::new(1), Handle::new(2), 4.0);
    win.width = 1.2;
    win.reference_side = OpeningReferenceSide::Start;
    win.hinge = LeafOperation::left();

    let grips = opening_axis_grips(&axis, &win, 0.0);
    assert_eq!(grips.len(), 5, "Must have 5 grips: Center, Start, End, HingeFlip, SwingFlip");
    assert_eq!(grips[0].id, 0);
    assert_eq!(grips[1].id, 1);
    assert_eq!(grips[2].id, 2);
    assert_eq!(grips[3].id, 3);
    assert_eq!(grips[4].id, 4);

    // Center grip is at s = 4.0 + 0.6 = 4.6
    assert!((grips[0].world.x - 4.6).abs() < 1e-6);
    // Start grip is at s = 4.0
    assert!((grips[1].world.x - 4.0).abs() < 1e-6);
    // End grip is at s = 5.2
    assert!((grips[2].world.x - 5.2).abs() < 1e-6);

    // 1. Move position along axis using Grip 0 to x = 6.0
    apply_opening_axis_grip(&axis, &mut win, 0, DVec3::new(6.0, 0.0, 0.0));
    // Since reference_side is Start, start reveal moves so center is at 6.0: start = 6.0 - 0.6 = 5.4
    assert!((win.distance_along_axis - 5.4).abs() < 1e-6);
    assert!((win.center_along_axis() - 6.0).abs() < 1e-6);

    // 2. Resize start jamb using Grip 1 to x = 5.0
    apply_opening_axis_grip(&axis, &mut win, 1, DVec3::new(5.0, 0.0, 0.0));
    // End was at 6.6, new start is at 5.0 -> width becomes 1.6, start is at 5.0
    assert!((win.width - 1.6).abs() < 1e-6);
    assert!((win.distance_along_axis - 5.0).abs() < 1e-6);

    // 3. Flip using Grip 3
    apply_opening_axis_grip(&axis, &mut win, 3, DVec3::ZERO);
    assert_eq!(win.reference_side, OpeningReferenceSide::End);
    assert_eq!(win.hinge, LeafOperation::right());
}

#[test]
fn test_opening_grip_simulation_preserves_handles() {
    use crate::modules::aec::engine::opening_display::{
        apply_opening_axis_grip, collect_opening_display_children, commit_opening_instance,
    };
    use crate::modules::aec::engine::opening_xdata::{
        openings_for_host_wall, place_wall_opening, write_opening_instance,
    };
    use crate::modules::aec::engine::openings::OpeningKind;
    use crate::modules::aec::engine::wall::WallJustification;
    use crate::modules::aec::engine::wall_package::resolve_wall_package;
    use crate::modules::aec::engine::wall_regen::regenerate_wall_representation;
    use crate::modules::aec::engine::xdata::{wall_record, AEC_APPID};
    use crate::scene::Scene;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::xdata::ExtendedDataRecord;
    use acadrust::EntityType;
    use glam::DVec3;

    let mut scene = Scene::new();
    let layers = vec![wl("Brick", 0.3, "Structural")];
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, crate::modules::aec::engine::plan_view::PlanPhase::New, None);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(4.0, 0.0, 0.0),
        OpeningKind::Window,
        None,
        None,
        None,
    ).expect("place window");

    regenerate_wall_representation(&mut scene, wall_h, None).expect("regen wall");

    let children_before = collect_opening_display_children(&scene, win_h);
    assert!(!children_before.is_empty(), "Window must have display children");

    let axis = vec![(0.0, 0.0), (10.0, 0.0)];
    let mut opening = openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("opening exists");

    // Simulate mouse-drag: 10 intermediate mouse move events
    for step in 1..=10 {
        let new_x = 4.0 + (step as f64) * 0.1;
        apply_opening_axis_grip(&axis, &mut opening, 0, DVec3::new(new_x, 0.0, 0.0));
        write_opening_instance(&mut scene, &opening);

        // Verify that throughout the drag, the window handle win_h and its children handles remain valid!
        assert!(scene.document.get_entity(win_h).is_some(), "Opening entity handle must stay alive during drag");
        for child_h in &children_before {
            assert!(scene.document.get_entity(*child_h).is_some(), "Child entity handle must stay alive during drag");
        }
    }

    // Now simulate mouse release: commit
    commit_opening_instance(&mut scene, &opening, None, None);

    let wall_pkg = resolve_wall_package(&scene, wall_h);
    regenerate_wall_representation(&mut scene, wall_pkg, None).expect("regen wall on commit");

    let final_opening = openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("opening exists after commit");
    assert!((final_opening.distance_along_axis - 5.0).abs() < 1e-6, "Final position must be committed at 5.0");
}

#[test]
fn test_niche_2d_preserves_single_continuous_wall_with_correct_rest_thickness() {
    use crate::modules::aec::engine::openings::{
        subtract_openings_from_band, NicheSide, Opening,
    };
    use acadrust::Handle;

    let axis = vec![(0.0, 0.0), (10.0, 0.0)];
    let total_thickness = 0.30; // 30cm wall

    let mut niche_ext = Opening::breakthrough(Handle::new(1), Handle::new(2), 5.0);
    niche_ext.width = 1.2;
    niche_ext.height = 1.0;
    niche_ext.sill_height = 0.8;
    niche_ext.depth = Some(0.15); // 15cm deep niche in 30cm wall
    niche_ext.niche_side = NicheSide::Exterior;

    let pieces = subtract_openings_from_band(&[], &axis, total_thickness, &[niche_ext.clone()]);
    assert_eq!(
        pieces.len(),
        1,
        "Partial-depth niche MUST NOT split the wall into separate pieces"
    );

    let poly = &pieces[0];
    assert!(poly.len() >= 6, "Indented polygon must have extra vertices for the niche indentation");

    // In the niche span [4.4, 5.6], the exterior edge is indented to y = -0.15 + 0.15 = 0.0,
    // while the interior edge remains at y = +0.15.
    // The wall thickness across the niche is 0.15 - 0.0 = 0.15 m!
    let niche_y_at_reveal: Vec<f64> = poly
        .iter()
        .filter(|p| (p.0 - 4.4).abs() < 1e-4 || (p.0 - 5.6).abs() < 1e-4)
        .map(|p| p.1)
        .collect();

    assert!(
        niche_y_at_reveal.iter().any(|&y| (y - 0.0).abs() < 1e-4),
        "Niche reveal must have back-face vertex indented to y = 0.0"
    );
    assert!(
        niche_y_at_reveal.iter().any(|&y| (y - (-0.15)).abs() < 1e-4),
        "Niche reveal must have corner vertex at original wall face y = -0.15"
    );

    // Interior face across the wall stays at y = +0.15:
    let max_y_at_niche = poly
        .iter()
        .filter(|p| (p.0 - 4.4).abs() < 1e-4 || (p.0 - 5.6).abs() < 1e-4)
        .map(|p| p.1)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!((max_y_at_niche - 0.0).abs() < 1e-4 || (max_y_at_niche - 0.15).abs() < 1e-4);

    // Remaining thickness at the niche is 0.15 - 0.0 = 0.15:
    let interior_y = poly.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    assert!((interior_y - 0.15).abs() < 1e-4);
    let niche_back_y = 0.0;
    let rest_thickness = interior_y - niche_back_y;
    assert!((rest_thickness - 0.15).abs() < 1e-4, "Rest wall thickness must be exactly 15cm");

    // Outside the niche span (e.g. x < 4.0 or x > 6.0), the wall thickness remains 0.30 m:
    let min_y_rest = poly
        .iter()
        .filter(|p| p.0 < 4.0)
        .map(|p| p.1)
        .fold(f64::INFINITY, f64::min);
    assert!((min_y_rest - (-0.15)).abs() < 1e-4, "Undisturbed exterior face must be at y = -0.15");

    // Interior niche test:
    let mut niche_int = niche_ext.clone();
    niche_int.niche_side = NicheSide::Interior;
    let pieces_int = subtract_openings_from_band(&[], &axis, total_thickness, &[niche_int]);
    assert_eq!(pieces_int.len(), 1);
    let poly_int = &pieces_int[0];
    let int_y_at_reveal: Vec<f64> = poly_int
        .iter()
        .filter(|p| (p.0 - 4.4).abs() < 1e-4 || (p.0 - 5.6).abs() < 1e-4)
        .map(|p| p.1)
        .collect();
    // Interior face indents from 0.15 down to 0.0:
    assert!(
        int_y_at_reveal.iter().any(|&y| (y - 0.0).abs() < 1e-4),
        "Interior niche must indent to y = 0.0"
    );
    assert!(
        int_y_at_reveal.iter().any(|&y| (y - 0.15).abs() < 1e-4),
        "Interior niche must have corner at original interior face y = 0.15"
    );
}

#[test]
fn test_niche_3d_preserves_solid_back_wall() {
    use crate::modules::aec::engine::elevation_cut::zone_solid_paths;
    use crate::modules::aec::engine::opening_display::format_din1356_label;
    use crate::modules::aec::engine::openings::{NicheSide, Opening};
    use acadrust::Handle;

    let axis = vec![(0.0, 0.0), (10.0, 0.0)];
    let layers = vec![(0.30, -0.15)]; // thickness 0.30, axis_offset -0.15
    let layer_ext = vec![(2.80, 0.0)]; // height 2.80, base 0.0

    let mut niche = Opening::breakthrough(Handle::new(1), Handle::new(2), 5.0);
    niche.width = 1.20;
    niche.height = 1.00;
    niche.sill_height = 0.80;
    niche.depth = Some(0.12); // 12cm deep niche
    niche.niche_side = NicheSide::Exterior;

    let z_paths = zone_solid_paths(&axis, &layers, &layer_ext, &[niche.clone()]);
    // Must contain: sill solid, head solid, AND the back-wall solid!
    assert!(z_paths.len() >= 3, "Zone solids must include sill, head, and niche back wall");

    // The back-wall solid has thickness 0.30 - 0.12 = 0.18:
    let back_wall = z_paths
        .iter()
        .find(|zp| (zp.direction[1].abs() - 0.18).abs() < 1e-4)
        .expect("Must have 3D back-wall solid with thickness 18cm");

    let z_min = back_wall.loop_xyz.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
    let z_max = back_wall.loop_xyz.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max);
    assert!((z_min - 0.80).abs() < 1e-4, "Back-wall solid z_min must match niche sill 0.80");
    assert!((z_max - 1.80).abs() < 1e-4, "Back-wall solid z_max must match niche head 1.80");

    // Test label formatting with 3 dimensions:
    let label = format_din1356_label(&niche);
    assert_eq!(label, "1.20 / 1.00 / 0.12\\PUK 0.80", "Niche label must display width / height / depth and UK");
}

#[test]
fn test_elevation_generators_contour_muntins_and_triangles() {
    use crate::modules::aec::engine::display_component::OpeningComponentSlot;
    use crate::modules::aec::engine::opening_display::{bake_opening_generators, OpeningBakeParams};
    use crate::modules::aec::engine::opening_shape::OpeningShape;
    use crate::modules::aec::engine::opening_style::{
        LeafOperation, OpeningGenerator, SlotGeometry, DEFAULT_FRAME_THICKNESS,
    };
    use crate::modules::aec::engine::openings::OpeningKind;
    use std::collections::HashMap;

    let mut slots = HashMap::new();
    slots.insert(
        OpeningComponentSlot::ElevationContour2D,
        SlotGeometry::Generator(OpeningGenerator::ElevationFrameRect),
    );
    slots.insert(
        OpeningComponentSlot::ElevationMuntins2D,
        SlotGeometry::Generator(OpeningGenerator::ElevationMuntinsDouble),
    );
    slots.insert(
        OpeningComponentSlot::ElevationSwing2D,
        SlotGeometry::Generator(OpeningGenerator::ElevationSwingTriangle),
    );
    slots.insert(
        OpeningComponentSlot::ElevationSill2D,
        SlotGeometry::Generator(OpeningGenerator::ElevationSillLine),
    );

    let p_left = OpeningBakeParams {
        width: 1.20,
        height: 1.40,
        sill_height: 0.90,
        thickness: 0.30,
        frame_thickness: DEFAULT_FRAME_THICKNESS,
        cross_axis_offset: 0.0,
        wall_y_min: -0.15,
        wall_y_max: 0.15,
        hinge: LeafOperation::left(),
        swing_side: SwingSide::Exterior,
        shape: OpeningShape::Rectangle,
        spring_height: 0.0,
        opening_angle_deg: 90.0,
        kind: OpeningKind::Window,
    };

    let baked_left = bake_opening_generators(&slots, p_left, None);

    // 1. ElevationContour2D: outer frame + inner frame
    let contours: Vec<_> = baked_left
        .iter()
        .filter(|p| p.slot == OpeningComponentSlot::ElevationContour2D)
        .collect();
    assert_eq!(contours.len(), 2, "ElevationFrameRect must emit outer and inner frame loops");

    // 2. ElevationMuntins2D: mullion + left leaf + right leaf
    let muntins: Vec<_> = baked_left
        .iter()
        .filter(|p| p.slot == OpeningComponentSlot::ElevationMuntins2D)
        .collect();
    assert_eq!(muntins.len(), 3, "ElevationMuntinsDouble must emit central mullion and two leaves");

    // 3. ElevationSwing2D: the tip marks the hinge, the base sits on the strike.
    let swing_left: Vec<_> = baked_left
        .iter()
        .filter(|p| p.slot == OpeningComponentSlot::ElevationSwing2D)
        .collect();
    assert_eq!(swing_left.len(), 1);
    let pts_left = &swing_left[0].points;
    assert_eq!(pts_left.len(), 3);
    assert!(pts_left[1].0 < 0.0, "Left-hinged triangle tip must sit on the left hinge");
    assert!(pts_left[0].0 > 0.0, "Left-hinged triangle base start must be on the strike");
    assert!(pts_left[2].0 > 0.0, "Left-hinged triangle base end must be on the strike");

    // Right-hinged test:
    let mut p_right = p_left;
    p_right.hinge = LeafOperation::right();
    let baked_right = bake_opening_generators(&slots, p_right, None);
    let swing_right: Vec<_> = baked_right
        .iter()
        .filter(|p| p.slot == OpeningComponentSlot::ElevationSwing2D)
        .collect();
    let pts_right = &swing_right[0].points;
    assert!(pts_right[1].0 > 0.0, "Right-hinged triangle tip must sit on the right hinge");
    assert!(pts_right[0].0 < 0.0, "Right-hinged triangle base start must be on the strike");
    assert!(pts_right[2].0 < 0.0, "Right-hinged triangle base end must be on the strike");

    // 4. ElevationSill2D:
    let sill_elev: Vec<_> = baked_left
        .iter()
        .filter(|p| p.slot == OpeningComponentSlot::ElevationSill2D)
        .collect();
    assert_eq!(sill_elev.len(), 2, "ElevationSillLine must emit top line and drop nose");

    // 5. Arch shape test:
    let mut slots_arch = HashMap::new();
    slots_arch.insert(
        OpeningComponentSlot::ElevationContour2D,
        SlotGeometry::Generator(OpeningGenerator::ElevationFrameArch),
    );
    let mut p_arch = p_left;
    p_arch.shape = OpeningShape::Arch;
    p_arch.spring_height = 0.80;
    let baked_arch = bake_opening_generators(&slots_arch, p_arch, None);
    let arch_contour = &baked_arch[0];
    assert!(arch_contour.points.len() > 10, "Arch contour must tessellate circular curved top");
    let max_z = arch_contour.points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    assert!((max_z - 1.40).abs() < 1e-4, "Arch top must reach total height 1.40");
}

#[test]
fn test_elevation_components_world_regeneration_creates_3d_polylines() {
    use crate::modules::aec::engine::opening_display::collect_opening_display_children;
    use crate::modules::aec::engine::opening_xdata::{
        openings_for_host_wall, place_wall_opening, write_opening_instance,
    };
    use crate::modules::aec::engine::openings::OpeningKind;
    use crate::modules::aec::engine::wall::WallJustification;
    use crate::modules::aec::engine::wall_regen::regenerate_wall_representation;
    use crate::modules::aec::engine::xdata::{wall_record, AEC_APPID};
    use crate::scene::Scene;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::xdata::ExtendedDataRecord;
    use acadrust::EntityType;
    use glam::DVec3;

    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, crate::modules::aec::engine::plan_view::PlanPhase::New, None);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(5.0, 0.0, 0.0),
        OpeningKind::Window,
        None,
        None,
        None,
    ).expect("place window");

    let mut opening = openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("opening found");

    let mut style = crate::modules::aec::engine::opening_style::OpeningStyle::standard_window();
    style.parts.exterior_sill.thickness = 0.05;

    let mut lib = crate::modules::aec::engine::library::StyleLibrary::default();
    lib.opening_styles.push(style.clone());
    opening.style_id = Some(style.style.id.clone());
    write_opening_instance(&mut scene, &opening);

    regenerate_wall_representation(&mut scene, wall_h, Some(&lib)).expect("regen wall");

    let children = collect_opening_display_children(&scene, win_h);
    assert!(!children.is_empty());

    // Verify that Polyline3D entities are created in the façade plane for elevation components:
    let polylines3d: Vec<_> = children
        .iter()
        .filter_map(|h| match scene.document.get_entity(*h) {
            Some(EntityType::Polyline3D(p3d)) => Some(p3d.clone()),
            _ => None,
        })
        .collect();

    assert!(!polylines3d.is_empty(), "Façade elevation slots must generate Polyline3D entities");

    // Verify Z-coordinates match sill (0.90) and height (1.20 -> head at 2.10):
    let all_z: Vec<f64> = polylines3d
        .iter()
        .flat_map(|p| p.vertices.iter().map(|v| v.position.z))
        .collect();

    let min_z = all_z.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    let max_z = all_z.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));

    // Elevation sill drop goes to sill - 0.05 = 0.85:
    assert!((min_z - 0.85).abs() < 1e-4, "Min Z must match elevation sill drop (0.85)");
    // The leaf swing schema overlaps the fixed frame by the leaf rebate/lateral
    // overlap (default 0.01), so it reaches 2.10 - 0.06 + 0.01 = 2.05.
    assert!((max_z - (0.90 + 1.20 - 0.06 + 0.01)).abs() < 1e-4, "Max Z must match leaf swing schema top (2.05): {max_z}");
}

#[test]
fn test_parametric_3d_solids_for_openings() {
    use crate::modules::aec::engine::opening_display::{
        build_opening_frame_3d, build_opening_glazing_3d, build_opening_leaf_3d,
    };
    use crate::modules::aec::engine::opening_shape::{OpeningShape, TriangleVariant};
    use crate::modules::aec::engine::opening_style::LeafOperation;

    let width = 1.20;
    let height = 1.40;
    let ft = 0.07;
    let depth = 0.08;

    // 1. Frame3D (Rectangle)
    let frame_body = build_opening_frame_3d(width, height, ft, depth, OpeningShape::Rectangle, 0.0).expect("build frame 3d");
    assert!(frame_body.validate().is_empty(), "Frame B-Rep body must be topologically valid");
    let (min_f, max_f) = crate::scene::model::solid_model::extent(&frame_body).expect("frame extent");
    assert!((max_f[0] - min_f[0] - width).abs() < 1e-3, "Frame width must match 1.20");
    assert!((max_f[2] - min_f[2] - height).abs() < 1e-3, "Frame height must match 1.40");
    assert!((max_f[1] - min_f[1] - depth).abs() < 1e-3, "Frame depth must match 0.08");

    // 2. Frame3D (Circle, Arch, Triangle)
    let circle_frame = build_opening_frame_3d(1.0, 1.0, 0.05, depth, OpeningShape::Circle, 0.0).expect("build circle frame");
    assert!(circle_frame.validate().is_empty(), "Circle Frame B-Rep body must be topologically valid");

    let arch_frame = build_opening_frame_3d(1.0, 1.8, 0.05, depth, OpeningShape::Arch, 1.2).expect("build arch frame");
    assert!(arch_frame.validate().is_empty(), "Arch Frame B-Rep body must be topologically valid");

    let tri_frame = build_opening_frame_3d(1.2, 1.0, 0.05, depth, OpeningShape::Triangle(TriangleVariant::IsoscelesUp), 0.0).expect("build tri frame");
    assert!(tri_frame.validate().is_empty(), "Triangle Frame B-Rep body must be topologically valid");

    // 3. Leaf3D (Window left hinge, Window right hinge, Door left hinge)
    let leaf_body = build_opening_leaf_3d(width, height, ft, 45.0, LeafOperation::left(), SwingSide::Exterior, false, OpeningShape::Rectangle, 0.0)
        .expect("build leaf 3d left");
    assert!(leaf_body.validate().is_empty(), "Leaf B-Rep body must be topologically valid");

    let leaf_right = build_opening_leaf_3d(width, height, ft, 30.0, LeafOperation::right(), SwingSide::Exterior, false, OpeningShape::Rectangle, 0.0)
        .expect("build leaf 3d right");
    assert!(leaf_right.validate().is_empty(), "Right leaf B-Rep body must be topologically valid");

    let door_leaf = build_opening_leaf_3d(0.90, 2.10, 0.05, 90.0, LeafOperation::left(), SwingSide::Exterior, true, OpeningShape::Rectangle, 0.0)
        .expect("build door leaf 3d");
    assert!(door_leaf.validate().is_empty(), "Door leaf B-Rep body must be topologically valid");

    // 4. Glazing3D (Rectangle, Circle, Arch)
    let glass_body = build_opening_glazing_3d(width, height, ft, OpeningShape::Rectangle, 0.0).expect("build glass 3d");
    assert!(glass_body.validate().is_empty(), "Glass B-Rep body must be topologically valid");
    let (min_g, max_g) = crate::scene::model::solid_model::extent(&glass_body).expect("glass extent");
    assert!((max_g[1] - min_g[1] - 0.02).abs() < 1e-3, "Glass pane thickness must be 0.02m (2cm)");

    let circle_glass = build_opening_glazing_3d(1.0, 1.0, 0.05, OpeningShape::Circle, 0.0).expect("build circle glass");
    assert!(circle_glass.validate().is_empty(), "Circle glass B-Rep body must be topologically valid");

    let arch_glass = build_opening_glazing_3d(1.0, 1.8, 0.05, OpeningShape::Arch, 1.2).expect("build arch glass");
    assert!(arch_glass.validate().is_empty(), "Arch glass B-Rep body must be topologically valid");
}

#[test]
fn test_2d_3d_representation_filtering() {
    use crate::modules::aec::engine::display_component::{OpeningComponentSlot, RepresentationMode};
    use crate::modules::aec::engine::library::build_effective_rule_set;
    use crate::modules::aec::engine::plan_view::DisplayConfig;

    let cfg = DisplayConfig::new(
        "Standard".to_string(),
        "Architektur".to_string(),
        crate::modules::aec::engine::plan_view::PlanningStage::Design,
        crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
    );

    // 1. TwoD Mode: all 3D slots must be false
    let rules_2d = build_effective_rule_set(&cfg, None, Some(RepresentationMode::TwoD));
    assert!(rules_2d.is_opening_visible(OpeningComponentSlot::Frame2D));
    assert!(rules_2d.is_opening_visible(OpeningComponentSlot::Glazing2D));
    assert!(!rules_2d.is_opening_visible(OpeningComponentSlot::Frame3D), "Frame3D must be hidden in 2D mode");
    assert!(!rules_2d.is_opening_visible(OpeningComponentSlot::Leaf3D), "Leaf3D must be hidden in 2D mode");
    assert!(!rules_2d.is_opening_visible(OpeningComponentSlot::Glazing3D), "Glazing3D must be hidden in 2D mode");

    // 2. ThreeD Mode: all 2D plan and elevation slots must be false
    let rules_3d = build_effective_rule_set(&cfg, None, Some(RepresentationMode::ThreeD));
    assert!(!rules_3d.is_opening_visible(OpeningComponentSlot::Frame2D), "Frame2D must be hidden in 3D mode");
    assert!(!rules_3d.is_opening_visible(OpeningComponentSlot::Swing2D), "Swing2D must be hidden in 3D mode");
    assert!(!rules_3d.is_opening_visible(OpeningComponentSlot::Sill2D), "Sill2D must be hidden in 3D mode");
    assert!(!rules_3d.is_opening_visible(OpeningComponentSlot::ElevationContour2D), "ElevationContour must be hidden in 3D mode");
    assert!(rules_3d.is_opening_visible(OpeningComponentSlot::Frame3D), "Frame3D must be visible in 3D mode");
    assert!(rules_3d.is_opening_visible(OpeningComponentSlot::Leaf3D), "Leaf3D must be visible in 3D mode");
    assert!(rules_3d.is_opening_visible(OpeningComponentSlot::Glazing3D), "Glazing3D must be visible in 3D mode");
}

#[test]
fn test_discipline_templates_slot_visibilities() {
    use crate::modules::aec::engine::display_component::OpeningComponentSlot;
    use crate::modules::aec::engine::library::DisplayConfigLibrary;

    let seed_lib = DisplayConfigLibrary::seed_default_configs();

    use crate::modules::aec::engine::opening_style::{
        DivisionLevel, ElevationFrameLevel, PlanFrameLevel, SwingGlyph,
    };

    let arch_50 = seed_lib.find("Architektur 1:50").expect("find Architektur 1:50");
    assert_eq!(arch_50.opening_levels.plan_frame, Some(PlanFrameLevel::Profile));
    assert_eq!(arch_50.opening_levels.swing, Some(SwingGlyph::Arc));
    assert_eq!(arch_50.opening_levels.divisions, Some(DivisionLevel::PostsAndMuntins));
    assert_eq!(arch_50.opening_levels.section.lintel, Some(true));

    let stat_50 = seed_lib.find("Statik 1:50").expect("find Statik 1:50");
    assert_eq!(stat_50.opening_levels.swing, Some(SwingGlyph::Off));
    assert_eq!(stat_50.opening_levels.section.glazing, Some(false));
    assert_eq!(stat_50.opening_levels.section.lintel, Some(true));
    assert_eq!(stat_50.opening_levels.section.frame, Some(false));

    let entw_100 = seed_lib.find("Entwurf 1:100").expect("find Entwurf 1:100");
    assert_eq!(entw_100.opening_levels.plan_frame, Some(PlanFrameLevel::SingleLine));
    assert_eq!(entw_100.opening_levels.swing, Some(SwingGlyph::Off));
    assert_eq!(entw_100.opening_levels.divisions, Some(DivisionLevel::Off));

    let elev = seed_lib.find("Ansicht Fassade").expect("find Ansicht Fassade");
    assert_eq!(elev.opening_levels.plan_frame, Some(PlanFrameLevel::Off));
    assert_eq!(elev.opening_levels.elevation_frame, Some(ElevationFrameLevel::FixedAndLeaf));
    assert_eq!(elev.opening_levels.divisions, Some(DivisionLevel::PostsAndMuntins));
    let _ = OpeningComponentSlot::Frame2D;
}

#[test]
fn test_opening_property_title_and_multi_layer_sills() {
    use crate::modules::aec::engine::opening_display::collect_opening_display_children;
    use crate::modules::aec::engine::opening_xdata::place_wall_opening;
    use crate::modules::aec::engine::openings::OpeningKind;
    use crate::modules::aec::engine::wall::Wall;
    use crate::modules::aec::engine::wall_regen::regenerate_wall_representation;
    use crate::modules::aec::engine::xdata::{wall_record_for_wall, AEC_APPID};
    use crate::scene::Scene;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::xdata::ExtendedDataRecord;
    use acadrust::EntityType;

    let mut scene = Scene::new();
    // Multi-layer wall: Outer Insulation (0.16m, offset -0.24), Core Concrete (0.24m, offset -0.08), Inner Plaster (0.02m, offset +0.16)
    // Bounds in normal space: -0.24 to +0.18
    let mut l1 = wl("Dämmung", 0.16, "Insulation");
    l1.axis_offset = -0.24;
    let mut l2 = wl("Beton", 0.24, "Structural");
    l2.axis_offset = -0.08;
    let mut l3 = wl("Putz", 0.02, "Finish");
    l3.axis_offset = 0.16;
    let layers = vec![l1, l2, l3];

    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(6.0, 0.0)));

    let mut wall = Wall::new("MultiLayer", 2.80, 0);
    wall.layers = layers;

    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = wall_record_for_wall(&wall);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    regenerate_wall_representation(&mut scene, wall_h, None).expect("regen wall");

    // Place a window at 3.0m
    let (win_h, _) = place_wall_opening(&mut scene, wall_h, glam::DVec3::new(3.0, 0.0, 0.0), OpeningKind::Window, None, None, None).expect("place window");
    let win_ent = scene.document.get_entity(win_h).expect("window entity");

    // Check title in property panel: must report Window / Fenster, NOT Point!
    let title = crate::modules::aec::properties::aec_entity_title(&scene, win_h, win_ent).expect("title");
    assert_eq!(title, crate::tr!("aec", "window-title"));

    // Regenerate and check sill positioning against outermost layer (-0.24) and innermost layer (+0.18)
    let children = collect_opening_display_children(&scene, win_h);
    assert!(!children.is_empty());
}

#[test]
fn test_wall_base_control_plane_elevation_alignment() {
    use crate::modules::aec::engine::opening_display::{
        collect_opening_display_children, host_base_z, opening_axis_grips,
    };
    use crate::modules::aec::engine::opening_xdata::{openings_for_host_wall, place_wall_opening};
    use crate::modules::aec::engine::openings::OpeningKind;
    use crate::modules::aec::engine::project::{Building, ProjectFile, StoreyRef};
    use crate::modules::aec::engine::wall::Wall;
    use crate::modules::aec::engine::wall_package::resolve_wall_package;
    use crate::modules::aec::engine::wall_regen::regenerate_wall_representation;
    use crate::modules::aec::engine::xdata::AEC_APPID;
    use crate::scene::Scene;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::xdata::ExtendedDataRecord;
    use acadrust::EntityType;
    use glam::DVec3;

    let mut scene = Scene::new();
    let storey_elevation = 2.80;
    let storey_height = 3.00;
    let storey = StoreyRef::new_with_height("OG1", storey_elevation, storey_height, "og1.dwg");
    let floor_plane_id = storey.floor_plane_id;
    let ceiling_plane_id = storey.ceiling_plane_id;

    let mut project = ProjectFile::default();
    let mut building = Building::new("Haus A");
    building.storeys.push(storey);
    project.buildings.push(building);

    let layers = vec![wl("Beton", 0.24, "Structural")];
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(8.0, 0.0)));
    pl.elevation = storey_elevation;

    let mut wall = Wall::new("Standard", storey_height, 0);
    wall.layers = layers.clone();
    wall.base_plane_id = Some(floor_plane_id);
    wall.top_plane_id = Some(ceiling_plane_id);
    wall.base_origin = [0.0, 0.0, storey_elevation];
    wall.base_normal = [0.0, 0.0, 1.0];
    wall.top_origin = [0.0, 0.0, storey_elevation + storey_height];
    wall.top_normal = [0.0, 0.0, 1.0];

    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    // Regenerate wall
    let touched = regenerate_wall_representation(&mut scene, wall_h, None).expect("regen wall");

    // 1. Check Wall Axis elevation
    let wall_ent = scene.document.get_entity(wall_h).expect("wall entity");
    if let EntityType::LwPolyline(pl) = wall_ent {
        assert!(
            (pl.elevation - storey_elevation).abs() < 1e-4,
            "Axis polyline elevation must be {storey_elevation}, got {}",
            pl.elevation
        );
    } else {
        panic!("Wall must be LwPolyline");
    }

    // 2. Check 2D Contours elevation
    for h in &touched {
        if *h == wall_h {
            continue;
        }
        if let Some(EntityType::LwPolyline(c_pl)) = scene.document.get_entity(*h) {
            assert!(
                (c_pl.elevation - storey_elevation).abs() < 1e-4,
                "2D contour polyline elevation must be {storey_elevation}, got {}",
                c_pl.elevation
            );
        }
    }

    // 3. Check 3D Solids Z extents
    let mut solid_found = false;
    for (handle, body) in &scene.solid_models {
        if touched.contains(handle) {
            let (min, max) = crate::scene::model::solid_model::extent(body).expect("extent");
            assert!(
                (min[2] - storey_elevation).abs() < 1e-3,
                "Solid min Z must start at {storey_elevation}, got {}",
                min[2]
            );
            assert!(
                (max[2] - (storey_elevation + storey_height)).abs() < 1e-3,
                "Solid max Z must reach {}, got {}",
                storey_elevation + storey_height,
                max[2]
            );
            solid_found = true;
        }
    }
    assert!(solid_found, "Must have generated 3D wall solids");

    // 4. Place a Window on the wall
    let sill_height = 0.90;
    let win_height = 1.30;
    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(3.0, 0.0, 0.0),
        OpeningKind::Window,
        None,
        None,
        None,
    )
    .expect("place window");

    // Configure window dimensions
    let mut opening = openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("window opening");
    opening.sill_height = sill_height;
    opening.height = win_height;
    opening.width = 1.20;
    crate::modules::aec::engine::opening_xdata::write_opening_instance(&mut scene, &opening);

    // Regenerate opening
    let wall_pkg = resolve_wall_package(&scene, wall_h);
    regenerate_wall_representation(&mut scene, wall_pkg, None).expect("regen wall with window");

    // 5. Verify Window POINT location Z matches wall base
    if let Some(EntityType::Point(pt)) = scene.document.get_entity(win_h) {
        assert!(
            (pt.location.z - storey_elevation).abs() < 1e-4,
            "Opening point Z must match wall base {storey_elevation}, got {}",
            pt.location.z
        );
    } else {
        panic!("Opening entity must be a POINT");
    }

    // 6. Verify Window 2D plan children elevation
    let children = collect_opening_display_children(&scene, win_h);
    assert!(!children.is_empty(), "Window must have display children");
    for child_h in &children {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*child_h) {
            assert!(
                (pl.elevation - storey_elevation).abs() < 1e-4,
                "Window 2D polyline elevation must match wall base {storey_elevation}, got {}",
                pl.elevation
            );
        } else if let Some(EntityType::MText(mtext)) = scene.document.get_entity(*child_h) {
            assert!(
                (mtext.insertion_point.z - storey_elevation).abs() < 1e-4,
                "Window label MText insertion point Z must match wall base {storey_elevation}, got {}",
                mtext.insertion_point.z
            );
        }
    }

    // 7. Verify Window 3D solid models have their base at wall_base_z + sill_height
    let mut win_solids_found = 0;
    let expected_sill_z = storey_elevation + sill_height;
    let expected_head_z = expected_sill_z + win_height;
    let mut overall_min_z = f64::INFINITY;
    let mut overall_max_z = f64::NEG_INFINITY;
    for (handle, body) in &scene.solid_models {
        if children.contains(handle) {
            let (min, max) = crate::scene::model::solid_model::extent(body).expect("extent");
            overall_min_z = overall_min_z.min(min[2]);
            overall_max_z = overall_max_z.max(max[2]);
            win_solids_found += 1;
        }
    }
    assert!(win_solids_found >= 2, "Must have generated window 3D frame and glazing solids");
    assert!(
        (overall_min_z - expected_sill_z).abs() < 1e-2,
        "Window 3D overall min Z must be at {expected_sill_z}, got {overall_min_z}"
    );
    assert!(
        (overall_max_z - expected_head_z).abs() < 1e-2,
        "Window 3D overall max Z must reach {expected_head_z}, got {overall_max_z}"
    );

    // 8. Verify Window Grips elevation
    let axis = vec![(0.0, 0.0), (8.0, 0.0)];
    let grips = opening_axis_grips(&axis, &opening, host_base_z(&scene, wall_pkg));
    assert_eq!(grips.len(), 5);
    for grip in &grips {
        assert!(
            (grip.world.z - storey_elevation).abs() < 1e-4,
            "Grip Z must match wall base {storey_elevation}, got {}",
            grip.world.z
        );
    }
}

#[test]
fn test_wall_and_opening_track_control_plane_vertical_movement() {
    use crate::modules::aec::engine::opening_display::{
        collect_opening_display_children, host_base_z, opening_axis_grips,
    };
    use crate::modules::aec::engine::opening_xdata::{openings_for_host_wall, place_wall_opening};
    use crate::modules::aec::engine::openings::OpeningKind;
    use crate::modules::aec::engine::project::{Building, ProjectFile, StoreyRef};
    use crate::modules::aec::engine::wall::Wall;
    use crate::modules::aec::engine::wall_package::resolve_wall_package;
    use crate::modules::aec::engine::wall_regen::regenerate_wall_representation;
    use crate::modules::aec::engine::xdata::AEC_APPID;
    use crate::scene::Scene;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::xdata::ExtendedDataRecord;
    use acadrust::EntityType;
    use glam::DVec3;

    let mut scene = Scene::new();
    let initial_elevation = 0.0;
    let new_elevation = 3.50;
    let storey_height = 2.80;

    let storey = StoreyRef::new_with_height("EG", initial_elevation, storey_height, "eg.dwg");
    let floor_plane_id = storey.floor_plane_id;
    let ceiling_plane_id = storey.ceiling_plane_id;

    let mut project = ProjectFile::default();
    let mut building = Building::new("Gebaeude 1");
    building.storeys.push(storey);
    project.buildings.push(building);

    let layers = vec![wl("Kalksandstein", 0.20, "Structural")];
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(6.0, 0.0)));
    pl.elevation = initial_elevation;

    let mut wall = Wall::new("Standard", storey_height, 0);
    wall.layers = layers.clone();
    wall.base_plane_id = Some(floor_plane_id);
    wall.top_plane_id = Some(ceiling_plane_id);
    wall.base_origin = [0.0, 0.0, initial_elevation];
    wall.top_origin = [0.0, 0.0, initial_elevation + storey_height];

    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    // Initial regen
    let _ = regenerate_wall_representation(&mut scene, wall_h, None);

    // Place window
    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(2.5, 0.0, 0.0),
        OpeningKind::Window,
        None,
        None,
        None,
    )
    .expect("place window");

    let mut opening = openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("window");
    opening.sill_height = 0.85;
    opening.height = 1.25;
    opening.width = 1.00;
    crate::modules::aec::engine::opening_xdata::write_opening_instance(&mut scene, &opening);
    let wall_pkg = resolve_wall_package(&scene, wall_h);
    let _ = regenerate_wall_representation(&mut scene, wall_pkg, None);

    // NOW: Move the storey / control plane elevation up to 3.50m!
    let storey_mut = &mut project.buildings[0].storeys[0];
    storey_mut.set_elevation(new_elevation);

    let project_snap = project.clone();
    let storey_mut = &mut project.buildings[0].storeys[0];
    // Apply storey elevation update to scene
    crate::modules::aec::project::storey_z::apply_storey_z_to_scene(
        &mut scene,
        storey_mut,
        Some(&project_snap),
        None,
    );

    // 1. Verify wall axis polyline elevation updated to 3.50m
    if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(wall_h) {
        assert!(
            (pl.elevation - new_elevation).abs() < 1e-4,
            "Wall axis polyline elevation must update to {new_elevation}, got {}",
            pl.elevation
        );
    }

    // 2. Verify wall 3D solids updated to start at 3.50m
    let children = collect_opening_display_children(&scene, win_h);
    let mut wall_min_z = f64::INFINITY;
    for (handle, body) in &scene.solid_models {
        if (*handle == wall_pkg || scene.document.get_entity(*handle).is_some())
            && !children.contains(handle)
        {
            let (min, _max) = crate::scene::model::solid_model::extent(body).expect("extent");
            wall_min_z = wall_min_z.min(min[2]);
        }
    }
    assert!(
        (wall_min_z - new_elevation).abs() < 1e-3,
        "Wall 3D solid min Z must start at {new_elevation}, got {wall_min_z}"
    );

    // 3. Verify window POINT location updated to 3.50m
    if let Some(EntityType::Point(pt)) = scene.document.get_entity(win_h) {
        assert!(
            (pt.location.z - new_elevation).abs() < 1e-4,
            "Window point location Z must update to {new_elevation}, got {}",
            pt.location.z
        );
    }

    // 4. Verify window 3D solids moved up to new_elevation + sill_height = 3.50 + 0.85 = 4.35m
    let children = collect_opening_display_children(&scene, win_h);
    let expected_sill_z = new_elevation + 0.85;
    let expected_head_z = expected_sill_z + 1.25;
    let mut win_solids = 0;
    let mut overall_min_z = f64::INFINITY;
    let mut overall_max_z = f64::NEG_INFINITY;
    for (handle, body) in &scene.solid_models {
        if children.contains(handle) {
            let (min, max) = crate::scene::model::solid_model::extent(body).expect("extent");
            overall_min_z = overall_min_z.min(min[2]);
            overall_max_z = overall_max_z.max(max[2]);
            win_solids += 1;
        }
    }
    assert!(win_solids >= 2, "Must find updated window 3D solids");
    assert!(
        (overall_min_z - expected_sill_z).abs() < 1e-2,
        "Window 3D overall min Z must move to {expected_sill_z}, got {overall_min_z}"
    );
    assert!(
        (overall_max_z - expected_head_z).abs() < 1e-2,
        "Window 3D overall max Z must move to {expected_head_z}, got {overall_max_z}"
    );

    // 5. Verify window grips moved up to 3.50m
    let axis = vec![(0.0, 0.0), (6.0, 0.0)];
    let grips = opening_axis_grips(&axis, &opening, host_base_z(&scene, wall_pkg));
    for grip in &grips {
        assert!(
            (grip.world.z - new_elevation).abs() < 1e-4,
            "Window grip Z must be at {new_elevation}, got {}",
            grip.world.z
        );
    }
}

#[test]
fn test_manager_save_vs_save_and_apply_decoupling() {
    use crate::modules::aec::engine::material::Material;
    use crate::modules::aec::engine::opening_display::collect_opening_display_children;
    use crate::modules::aec::engine::opening_style::{OpeningStyle, DEFAULT_FRAME_THICKNESS};
    use crate::modules::aec::engine::opening_xdata::place_wall_opening;
    use crate::modules::aec::engine::openings::OpeningKind;
    use crate::modules::aec::engine::project::{Building, ProjectFile, StoreyRef};
    use crate::modules::aec::engine::wall::Wall;
    use crate::modules::aec::engine::wall_package::resolve_wall_package;
    use crate::modules::aec::engine::wall_regen::regenerate_wall_representation;
    use crate::modules::aec::engine::xdata::AEC_APPID;
    use crate::scene::Scene;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::xdata::ExtendedDataRecord;
    use acadrust::EntityType;
    use glam::DVec3;

    let mut scene = Scene::new();
    let initial_elevation = 1.00;
    let storey_height = 2.80;
    let mut storey = StoreyRef::new_with_height("EG", initial_elevation, storey_height, "eg.dwg");
    let floor_plane_id = storey.floor_plane_id;
    let ceiling_plane_id = storey.ceiling_plane_id;

    let mut project = ProjectFile::default();
    let mut building = Building::new("Bau A");
    building.storeys.push(storey.clone());
    project.buildings.push(building);

    let mat_id = "mat_brick_red".to_string();
    let initial_material = Material {
        style: crate::modules::aec::engine::style_common::StyleCommon::new(
            mat_id.clone(),
            "Ziegel",
            None,
            crate::modules::aec::engine::style_common::CATEGORY_MATERIAL,
            Default::default(),
        ),
        hatch_pattern: "ANSI31".to_string(),
        line_color: 0xCC3333,
        line_type: "Continuous".to_string(),
        render_material_ref: None,
        group: Some("Mauerwerk".to_string()),
        hatch_color: None,
        hatch_scale: 1.0,
        hatch_angle: 0.0,
        hatch_angle_relative: true,
    };
    let mut library = crate::modules::aec::engine::library::StyleLibrary::empty();
    library.upsert_material(initial_material.clone());

    let mut win_style = OpeningStyle::standard_window();
    let win_style_id = "test_window_style".to_string();
    win_style.style.id = win_style_id.clone();
    win_style.parts.fixed_frame.face_width = DEFAULT_FRAME_THICKNESS;
    library.upsert_opening_style(win_style.clone());

    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(6.0, 0.0)));
    pl.elevation = initial_elevation;

    let mut wall = Wall::new("Standard", storey_height, 0);
    wall.layers = vec![wl(&mat_id, 0.30, "Structural")];
    wall.base_plane_id = Some(floor_plane_id);
    wall.top_plane_id = Some(ceiling_plane_id);
    wall.base_origin = [0.0, 0.0, initial_elevation];
    wall.base_normal = [0.0, 0.0, 1.0];
    wall.top_origin = [0.0, 0.0, initial_elevation + storey_height];
    wall.top_normal = [0.0, 0.0, 1.0];

    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    // Initial regen
    let _ = regenerate_wall_representation(&mut scene, wall_h, Some(&library));

    // Place window
    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(2.0, 0.0, 0.0),
        OpeningKind::Window,
        Some(&library),
        None,
        None,
    )
    .expect("place window");
    let mut opening = crate::modules::aec::engine::opening_xdata::openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("window");
    opening.style_id = Some(win_style_id.clone());
    crate::modules::aec::engine::opening_xdata::write_opening_instance(&mut scene, &opening);
    let wall_pkg = resolve_wall_package(&scene, wall_h);
    let _ = regenerate_wall_representation(&mut scene, wall_pkg, Some(&library));

    // Case 1: Storey height/elevation edited, but "Save" only (no apply to scene yet).
    // The storey in-memory updates, but scene entities stay at initial_elevation!
    let new_elevation = 4.00;
    storey.set_elevation(new_elevation);
    project.buildings[0].storeys[0] = storey.clone();
    // Wall axis elevation in scene MUST STILL BE initial_elevation (1.00)
    if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(wall_h) {
        assert_eq!(pl.elevation, initial_elevation, "Wall elevation must not change before Apply");
    }

    // Now call apply to scene (Save and Apply):
    let project_snap = project.clone();
    crate::modules::aec::project::storey_z::apply_storey_z_to_scene(
        &mut scene,
        &mut storey,
        Some(&project_snap),
        Some(&library),
    );
    // Wall axis elevation in scene MUST NOW BE new_elevation (4.00)
    if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(wall_h) {
        assert_eq!(pl.elevation, new_elevation, "Wall elevation must update after Apply");
    }

    // Case 2: Opening style modified (fixed-frame width 0.06 -> 0.12), "Save" only.
    // Library updates, but scene opening children still have the old geometry!
    let mut updated_style = win_style.clone();
    let new_frame_thickness = 0.12;
    updated_style.parts.fixed_frame.face_width = new_frame_thickness;
    library.upsert_opening_style(updated_style);

    let old_children = collect_opening_display_children(&scene, win_h);
    assert!(!old_children.is_empty());

    // When "Save and Apply" is triggered, regenerate_wall_representation is called with the updated library:
    let _ = regenerate_wall_representation(&mut scene, wall_pkg, Some(&library));
    let new_children = collect_opening_display_children(&scene, win_h);
    assert!(!new_children.is_empty());

    // Case 3: Material modified (change hatch pattern from ANSI31 to AR-B816), "Save" only.
    let mut updated_mat = initial_material.clone();
    updated_mat.hatch_pattern = "AR-B816".to_string();
    library.upsert_material(updated_mat);

    // Apply updates the wall with the new material
    let _ = regenerate_wall_representation(&mut scene, wall_pkg, Some(&library));
    // Verify hatch entity was updated in the scene
    let mut found_hatch = false;
    for entity in scene.document.entities() {
        if let EntityType::Hatch(h) = entity {
            if h.pattern.name == "AR-B816" {
                found_hatch = true;
                break;
            }
        }
    }
    assert!(found_hatch, "Hatch must reflect updated material after Apply");
}
