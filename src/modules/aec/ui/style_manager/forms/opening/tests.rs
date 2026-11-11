use super::*;
use crate::modules::aec::engine::display_component::OpeningComponentSlot;

fn rect(slot: OpeningComponentSlot, x0: f64, y0: f64, x1: f64, y1: f64) -> BakedPath {
    BakedPath {
        slot,
        look_role: None,
        points: vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)],
        points_3d: None,
        closed: true,
        filled: false,
    }
}

#[test]
fn inner_rect_wins_inside_and_edge_wins_near_border() {
    let outer = OpeningComponentSlot::HostCut2D;
    let inner = OpeningComponentSlot::Frame2D;
    let paths = vec![rect(outer, 0.0, 0.0, 10.0, 10.0), rect(inner, 4.0, 4.0, 6.0, 6.0)];
    assert_eq!(hit_slot(&paths, 5.0, 5.0, 0.1), Some(inner));
    assert_eq!(hit_slot(&paths, 1.0, 1.0, 0.1), Some(outer));
    assert_eq!(hit_slot(&paths, 0.05, 5.0, 0.1), Some(outer));
    assert_eq!(hit_slot(&paths, 20.0, 20.0, 0.1), None);
}

#[test]
fn preview_hit_resolves_role_override_and_clears_unmapped_or_missed_paths() {
    let role_path = BakedPath {
        slot: OpeningComponentSlot::Frame2D,
        look_role: Some(OpeningLookRole::ShutterBox),
        points: vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
        points_3d: None,
        closed: true,
        filled: true,
    };
    assert_eq!(
        hit_role(&[role_path], 5.0, 5.0, 0.1),
        Some(OpeningLookRole::ShutterBox)
    );

    let unassigned = rect(OpeningComponentSlot::HostCut2D, 0.0, 0.0, 10.0, 10.0);
    assert_eq!(hit_role(&[unassigned], 5.0, 5.0, 0.1), None);
    assert_eq!(hit_role(&[], 5.0, 5.0, 0.1), None);
}

#[test]
fn preview_wheel_zoom_is_available_in_every_view() {
    let bounds = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 300.0,
    };
    let cursor = mouse::Cursor::Available(Point::new(200.0, 150.0));
    let event = canvas::Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
    });
    for mode in [
        AecOpeningPreviewMode::Plan2D,
        AecOpeningPreviewMode::Elevation2D,
        AecOpeningPreviewMode::Section,
        AecOpeningPreviewMode::Model3D,
    ] {
        let preview = OpeningPreviewCanvas {
            paths: vec![rect(OpeningComponentSlot::Frame2D, -0.5, 0.0, 0.5, 1.0)],
            mode,
            width: 1.0,
            height: 1.0,
            sill: 0.9,
            thickness: PREVIEW_WALL_THICKNESS,
            look_styles: HashMap::new(),
            selected_role: None,
            isometric: AecOpeningIsometric::FrontLeft,
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
        };
        let mut state = None;
        assert!(preview
            .update(&mut state, &event, bounds, cursor)
            .is_some());
    }
}

#[test]
fn section_wheel_zoom_keeps_the_cursor_world_point_fixed() {
    let bounds = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 300.0,
    };
    let cursor = Point::new(260.0, 170.0);
    let paths = vec![rect(OpeningComponentSlot::Frame2D, -0.1, 0.9, 0.1, 2.4)];
    let camera = PreviewCamera::fit(
        bounds,
        &paths,
        AecOpeningPreviewMode::Section,
        1.2,
        1.5,
        0.9,
        PREVIEW_WALL_THICKNESS,
    );
    let (world_x, world_y) = camera.to_local(cursor);
    let zoom = 1.1;
    let fitted = PreviewCamera::fit(
        bounds,
        &paths,
        AecOpeningPreviewMode::Section,
        1.2,
        1.5,
        0.9,
        PREVIEW_WALL_THICKNESS,
    );
    let (pan_x, pan_y) = preview_pan_for_cursor(&fitted, cursor, world_x, world_y, zoom);
    let zoomed = fitted.with_view(zoom, pan_x, pan_y);
    let after = zoomed.to_screen(world_x, world_y);

    assert!((after.x - cursor.x).abs() < 1e-4);
    assert!((after.y - cursor.y).abs() < 1e-4);
}

#[test]
fn preview_role_colors_resolve_line_and_material_hatch_independently() {
    use crate::modules::aec::engine::material::Material;

    let mut style = OpeningStyle::standard_window();
    style.looks.fixed_frame.line_color = Some(acadrust::types::Color::Rgb {
        r: 210,
        g: 20,
        b: 30,
    });
    style.looks.fixed_frame.material_id = Some("frame_material".into());
    let mut library = StyleLibrary::empty();
    let mut material = Material::new(
        "frame_material".into(),
        "Frame material".into(),
        "ANSI31".into(),
        0x0080c0,
        "Continuous".into(),
    );
    material.hatch_color = Some(acadrust::types::Color::Rgb {
        r: 10,
        g: 40,
        b: 90,
    });
    library.materials.push(material);

    let colors = preview_look_styles(&style, &library)[&OpeningLookRole::FixedFrame];
    assert_eq!(colors.line, Some(IcedColor::from_rgb8(210, 20, 30)));
    assert_eq!(colors.hatch, Some(IcedColor::from_rgb8(10, 40, 90)));
}

#[test]
fn plan_and_section_fit_ignore_paths_outside_the_rough_opening() {
    let bounds = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 300.0,
    };
    let inside = vec![rect(OpeningComponentSlot::Frame2D, -0.2, -0.05, 0.2, 0.05)];
    let grown = vec![rect(OpeningComponentSlot::Frame2D, -2.0, -1.0, 2.0, 1.0)];
    for mode in [AecOpeningPreviewMode::Plan2D, AecOpeningPreviewMode::Section] {
        let a = PreviewCamera::fit(bounds, &inside, mode, 1.2, 1.4, 0.9, 0.3);
        let b = PreviewCamera::fit(bounds, &grown, mode, 1.2, 1.4, 0.9, 0.3);
        assert_eq!(a.scale, b.scale);
        assert_eq!(a.ox, b.ox);
        assert_eq!(a.oy, b.oy);
    }
}

#[test]
fn isometric_presets_reproject_cached_model_points_without_geometry_changes() {
    let camera = PreviewCamera3d::default();
    let points_3d = vec![[1.0, 0.0, 0.0], [0.0, 1.0, 1.0]];
    let path = BakedPath {
        slot: OpeningComponentSlot::Frame3D,
        look_role: Some(OpeningLookRole::FixedFrame),
        points: points_3d
            .iter()
            .map(|point| preview_project(camera, point[0], point[1], point[2]))
            .collect(),
        points_3d: Some(points_3d.clone()),
        closed: false,
        filled: false,
    };
    let make_canvas = |isometric, zoom| OpeningPreviewCanvas {
        paths: vec![path.clone()],
        mode: AecOpeningPreviewMode::Model3D,
        width: 1.0,
        height: 1.0,
        sill: 0.0,
        thickness: PREVIEW_WALL_THICKNESS,
        look_styles: HashMap::new(),
        selected_role: None,
        isometric,
        zoom,
        pan_x: 0.0,
        pan_y: 0.0,
    };
    let front_left = make_canvas(AecOpeningIsometric::FrontLeft, 1.0).projected_paths();
    let front_right = make_canvas(AecOpeningIsometric::FrontRight, 1.0).projected_paths();
    let back_left = make_canvas(AecOpeningIsometric::BackLeft, 1.0).projected_paths();
    let back_right = make_canvas(AecOpeningIsometric::BackRight, 1.0).projected_paths();
    let projections = [
        &front_left[0].points,
        &front_right[0].points,
        &back_left[0].points,
        &back_right[0].points,
    ];
    for first in 0..projections.len() {
        for second in first + 1..projections.len() {
            assert_ne!(projections[first], projections[second]);
        }
    }
    assert_eq!(front_left[0].points_3d.as_deref(), Some(points_3d.as_slice()));
    let zoomed = make_canvas(AecOpeningIsometric::FrontLeft, 2.0).projected_paths();
    assert_eq!(front_left[0].points, zoomed[0].points);
    assert_eq!(front_left[0].points_3d, zoomed[0].points_3d);
}
