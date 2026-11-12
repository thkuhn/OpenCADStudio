use super::*;

pub fn extract_block_paths(
    doc: &acadrust::CadDocument,
    block_name: &str,
) -> Vec<(Vec<(f64, f64)>, bool)> {
    let Some(rec) = doc.block_records.get(block_name) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for entity in doc.entities() {
        if entity.common().owner_handle != rec.handle {
            continue;
        }
        match entity {
            EntityType::Line(line) => {
                paths.push((
                    vec![(line.start.x, line.start.y), (line.end.x, line.end.y)],
                    false,
                ));
            }
            EntityType::LwPolyline(pl) => {
                if pl.vertices.len() >= 2 {
                    let pts: Vec<(f64, f64)> = pl
                        .vertices
                        .iter()
                        .map(|v| (v.location.x, v.location.y))
                        .collect();
                    paths.push((pts, pl.is_closed));
                }
            }
            EntityType::Arc(arc) => {
                let n = 16;
                let start_a = arc.start_angle.to_radians();
                let end_a = arc.end_angle.to_radians();
                let sweep = if end_a >= start_a {
                    end_a - start_a
                } else {
                    end_a + 2.0 * std::f64::consts::PI - start_a
                };
                let pts: Vec<(f64, f64)> = (0..=n)
                    .map(|i| {
                        let a = start_a + sweep * (i as f64) / (n as f64);
                        (
                            arc.center.x + arc.radius * a.cos(),
                            arc.center.y + arc.radius * a.sin(),
                        )
                    })
                    .collect();
                paths.push((pts, false));
            }
            EntityType::Circle(circ) => {
                let n = 32;
                let pts: Vec<(f64, f64)> = (0..=n)
                    .map(|i| {
                        let a = 2.0 * std::f64::consts::PI * (i as f64) / (n as f64);
                        (
                            circ.center.x + circ.radius * a.cos(),
                            circ.center.y + circ.radius * a.sin(),
                        )
                    })
                    .collect();
                paths.push((pts, true));
            }
            _ => {}
        }
    }
    paths
}

pub(super) fn bake_block_slot(
    slot: OpeningComponentSlot,
    block_name: &str,
    placement: BlockPlacementMode,
    p: OpeningBakeParams,
    doc: Option<&acadrust::CadDocument>,
) -> Vec<BakedPath> {
    let Some(doc) = doc else {
        return Vec::new();
    };
    let raw_paths = extract_block_paths(doc, block_name);
    if raw_paths.is_empty() {
        return Vec::new();
    }
    let mut min_u = f64::INFINITY;
    let mut max_u = f64::NEG_INFINITY;
    for (pts, _) in &raw_paths {
        for &(u, _) in pts {
            min_u = min_u.min(u);
            max_u = max_u.max(u);
        }
    }
    if !min_u.is_finite() || !max_u.is_finite() {
        return Vec::new();
    }
    let u_span = (max_u - min_u).max(1e-9);
    let hw = p.width * 0.5;

    let mut out = Vec::new();
    match placement {
        BlockPlacementMode::JambPair => {
            for (pts, closed) in &raw_paths {
                let left_pts: Vec<(f64, f64)> = pts
                    .iter()
                    .map(|&(u, v)| (-hw + (u - min_u), v))
                    .collect();
                out.push(path(slot, left_pts, *closed, false));
            }
            for (pts, closed) in &raw_paths {
                let right_pts: Vec<(f64, f64)> = pts
                    .iter()
                    .map(|&(u, v)| (hw - (u - min_u), v))
                    .collect();
                out.push(path(slot, right_pts, *closed, false));
            }
        }
        BlockPlacementMode::StretchToFit => {
            for (pts, closed) in &raw_paths {
                let stretched: Vec<(f64, f64)> = pts
                    .iter()
                    .map(|&(u, v)| (-hw + ((u - min_u) / u_span) * (2.0 * hw), v))
                    .collect();
                out.push(path(slot, stretched, *closed, false));
            }
        }
        BlockPlacementMode::CenterAnchor => {
            let u_mid = (min_u + max_u) * 0.5;
            for (pts, closed) in &raw_paths {
                let centered: Vec<(f64, f64)> = pts
                    .iter()
                    .map(|&(u, v)| (u - u_mid, v))
                    .collect();
                out.push(path(slot, centered, *closed, false));
            }
        }
    }
    out
}

pub(super) fn bake_generator(
    slot: OpeningComponentSlot,
    gen: OpeningGenerator,
    p: OpeningBakeParams,
) -> Vec<BakedPath> {
    if p.width <= 1e-12 || p.thickness <= 1e-12 {
        return Vec::new();
    }
    let hw = p.width * 0.5;
    let ht = p.thickness * 0.5;
    let ft = p.frame_thickness.max(0.0);
    match gen {
        OpeningGenerator::None => Vec::new(),
        OpeningGenerator::FrameRect => bake_frame_rect(slot, p, hw, ht, ft),
        OpeningGenerator::DoorFrame => bake_door_frame(slot, hw, ht, ft),
        OpeningGenerator::LeafLine => bake_leaf_line(slot, p, hw, ht, ft),
        OpeningGenerator::SwingArc => bake_swing_arc(slot, p, hw, ft),
        OpeningGenerator::SillLines => bake_sill_lines(
            slot,
            hw,
            ht,
            ft,
            p.cross_axis_offset,
            p.wall_y_min,
            p.wall_y_max,
        ),
        OpeningGenerator::Cross => {
            let y_ext = p.wall_y_min - p.cross_axis_offset;
            let y_int = p.wall_y_max - p.cross_axis_offset;
            vec![
                path(slot, vec![(-hw, y_ext), (hw, y_int)], false, false),
                path(slot, vec![(-hw, y_int), (hw, y_ext)], false, false),
            ]
        }
        OpeningGenerator::DiagonalFill => {
            let y_ext = p.wall_y_min - p.cross_axis_offset;
            let y_int = p.wall_y_max - p.cross_axis_offset;
            vec![
                path(slot, vec![(-hw, y_ext), (hw, y_int)], false, false),
                path(
                    slot,
                    vec![(-hw, y_ext), (hw, y_ext), (hw, y_int), (-hw, y_int)],
                    true,
                    true,
                ),
            ]
        }
        OpeningGenerator::GlazingLine => bake_glazing_line(slot, hw, ft),
        OpeningGenerator::ThresholdLine => bake_threshold_line(slot, hw, ht),
        OpeningGenerator::OpeningLabel => bake_opening_label_preview(slot, hw),
        OpeningGenerator::ElevationFrameRect => {
            bake_elevation_contour(slot, p, hw, p.height, ft, p.kind == OpeningKind::Door)
        }
        OpeningGenerator::ElevationFrameArch => {
            bake_elevation_contour(slot, p, hw, p.height, ft, p.kind == OpeningKind::Door)
        }
        OpeningGenerator::ElevationMuntinsSingle => {
            bake_elevation_muntins_single(slot, hw, p.height, ft)
        }
        OpeningGenerator::ElevationMuntinsDouble => {
            bake_elevation_muntins_double(slot, hw, p.height, ft)
        }
        OpeningGenerator::ElevationSwingTriangle => {
            bake_elevation_swing_triangle(slot, p, hw, p.height, ft)
        }
        OpeningGenerator::ElevationSillLine => {
            bake_elevation_sill_line(slot, hw, p.height)
        }
        _ => Vec::new(),
    }
}

pub(super) fn rough_opening_elevation_guide(params: OpeningBakeParams) -> BakedPath {
    let hw = params.width * 0.5;
    let head = params.height.max(0.0);
    path(
        OpeningComponentSlot::Mark2D,
        vec![
            (-hw, 0.0),
            (hw, 0.0),
            (hw, head),
            (-hw, head),
            (-hw, 0.0),
        ],
        false,
        false,
    )
}

pub(super) fn path(
    slot: OpeningComponentSlot,
    points: Vec<(f64, f64)>,
    closed: bool,
    filled: bool,
) -> BakedPath {
    BakedPath {
        slot,
        look_role: None,
        points,
        points_3d: None,
        closed,
        filled,
    }
}

pub(super) fn bake_frame_rect(
    slot: OpeningComponentSlot,
    p: OpeningBakeParams,
    hw: f64,
    ht: f64,
    ft: f64,
) -> Vec<BakedPath> {
    if p.kind == OpeningKind::Door {
        bake_door_frame(slot, hw, ht, ft)
    } else {
        bake_window_posts(slot, hw, ht, ft)
    }
}

pub(super) fn bake_window_posts(
    slot: OpeningComponentSlot,
    hw: f64,
    ht: f64,
    ft: f64,
) -> Vec<BakedPath> {
    let frame_depth = (ft.max(0.07)).min(ht * 2.0);
    let half_depth = frame_depth * 0.5;
    let post_w = ft.min(hw * 0.45).max(1e-4);

    let left_post = vec![
        (-hw, -half_depth),
        (-hw + post_w, -half_depth),
        (-hw + post_w, half_depth),
        (-hw, half_depth),
    ];
    let right_post = vec![
        (hw - post_w, -half_depth),
        (hw, -half_depth),
        (hw, half_depth),
        (hw - post_w, half_depth),
    ];

    vec![
        path(slot, left_post, true, false),
        path(slot, right_post, true, false),
    ]
}

pub(super) fn bake_door_frame(
    slot: OpeningComponentSlot,
    hw: f64,
    ht: f64,
    ft: f64,
) -> Vec<BakedPath> {
    let post_w = ft.min(hw * 0.45).max(1e-4);
    let rebate_w = post_w * 0.5;
    let left_jamb = vec![
        (-hw, -ht),
        (-hw + post_w, -ht),
        (-hw + post_w, 0.0),
        (-hw + rebate_w, 0.0),
        (-hw + rebate_w, ht),
        (-hw, ht),
    ];
    let right_jamb = vec![
        (hw, -ht),
        (hw - post_w, -ht),
        (hw - post_w, 0.0),
        (hw - rebate_w, 0.0),
        (hw - rebate_w, ht),
        (hw, ht),
    ];
    vec![
        path(slot, left_jamb, true, false),
        path(slot, right_jamb, true, false),
    ]
}

pub(super) fn bake_leaf_line(
    slot: OpeningComponentSlot,
    p: OpeningBakeParams,
    hw: f64,
    _ht: f64,
    ft: f64,
) -> Vec<BakedPath> {
    match p.kind {
        OpeningKind::Door => {
            let (start, end) = leaf_segment(p.hinge, p.swing_side, hw, p.opening_angle_deg, ft);
            vec![path(slot, vec![start, end], false, false)]
        }
        _ => {
            let inset = ft.min(hw * 0.45);
            vec![path(
                slot,
                vec![(-hw + inset, 0.0), (hw - inset, 0.0)],
                false,
                false,
            )]
        }
    }
}

pub(super) fn swing_toward_exterior(swing: SwingSide) -> f64 {
    match swing {
        SwingSide::Exterior => -1.0,
        SwingSide::Interior => 1.0,
    }
}

pub(super) fn leaf_segment(
    hinge: LeafOperation,
    swing: SwingSide,
    hw: f64,
    angle_deg: f64,
    ft: f64,
) -> ((f64, f64), (f64, f64)) {
    let post_w = ft.min(hw * 0.45).max(1e-4);
    let rebate_w = post_w * 0.5;
    let clear_w = (hw - rebate_w) * 2.0;
    let ang = angle_deg.to_radians();
    let swing_sign = swing_toward_exterior(swing);
    match hinge.bearing() {
        crate::modules::aec::engine::construction_grid::TrackEnd::Start => {
            let start = (-hw + rebate_w, 0.0);
            let end = (
                -hw + rebate_w + clear_w * ang.cos(),
                swing_sign * clear_w * ang.sin(),
            );
            (start, end)
        }
        crate::modules::aec::engine::construction_grid::TrackEnd::End => {
            let start = (hw - rebate_w, 0.0);
            let end = (
                hw - rebate_w - clear_w * ang.cos(),
                swing_sign * clear_w * ang.sin(),
            );
            (start, end)
        }
    }
}

pub(super) fn bake_swing_arc(slot: OpeningComponentSlot, p: OpeningBakeParams, hw: f64, ft: f64) -> Vec<BakedPath> {
    let post_w = if p.kind == OpeningKind::Door {
        ft.min(hw * 0.45).max(1e-4)
    } else {
        0.0
    };
    let rebate_w = post_w * 0.5;
    let clear_w = (hw - rebate_w) * 2.0;
    if clear_w <= 1e-12 {
        return Vec::new();
    }
    let ang = p.opening_angle_deg.to_radians().abs().max(1e-6);
    let n = SWING_CHORD_COUNT.max(4);
    let swing_sign = swing_toward_exterior(p.swing_side);
    let pts = match p.hinge.bearing() {
        crate::modules::aec::engine::construction_grid::TrackEnd::Start => {
            let cx = -hw + rebate_w;
            (0..=n)
                .map(|i| {
                    let t = ang * (i as f64) / (n as f64);
                    (cx + clear_w * t.cos(), swing_sign * clear_w * t.sin())
                })
                .collect()
        }
        crate::modules::aec::engine::construction_grid::TrackEnd::End => {
            let cx = hw - rebate_w;
            (0..=n)
                .map(|i| {
                    let t = ang * (i as f64) / (n as f64);
                    (cx - clear_w * t.cos(), swing_sign * clear_w * t.sin())
                })
                .collect()
        }
    };
    vec![path(slot, pts, false, false)]
}

/// Fixed overhangs of the style-less sill generator (no style values available).
const LEGACY_SILL_OVERHANG: f64 = 0.035;
const LEGACY_BOARD_OVERHANG: f64 = 0.025;

pub(super) fn bake_sill_lines(
    slot: OpeningComponentSlot,
    hw: f64,
    ht: f64,
    ft: f64,
    cross_offset: f64,
    wall_y_min: f64,
    wall_y_max: f64,
) -> Vec<BakedPath> {
    let frame_depth = (ft.max(0.07)).min(ht * 2.0);
    let frame_half_d = frame_depth * 0.5;

    let mut out = Vec::new();
    let sill_overhang = LEGACY_SILL_OVERHANG;
    let wall_ext_y = wall_y_min - cross_offset;
    let wall_int_y = wall_y_max - cross_offset;
    let sill_y_outer = wall_ext_y - sill_overhang;
    let sill_ear = 0.0;

    // Exterior sill front nose
    out.push(
        path(
            slot,
            vec![
                (-hw - sill_ear, sill_y_outer),
                (hw + sill_ear, sill_y_outer),
                (hw + sill_ear, wall_ext_y),
                (-hw - sill_ear, wall_ext_y),
            ],
            true,
            false,
        )
        .with_look_role(OpeningLookRole::ExteriorSill),
    );
    // Reveal line along exterior wall face
    out.push(
        path(
            slot,
            vec![(-hw, wall_ext_y), (hw, wall_ext_y)],
            false,
            false,
        )
        .with_look_role(OpeningLookRole::ExteriorSill),
    );

    // Exterior reveal side lines (connecting frame to exterior wall)
    if -frame_half_d > wall_ext_y + 1e-4 {
        out.push(
            path(
                slot,
                vec![(-hw, -frame_half_d), (-hw, wall_ext_y)],
                false,
                false,
            )
            .with_look_role(OpeningLookRole::ExteriorSill),
        );
        out.push(
            path(
                slot,
                vec![(hw, -frame_half_d), (hw, wall_ext_y)],
                false,
                false,
            )
            .with_look_role(OpeningLookRole::ExteriorSill),
        );
    }

    let board_overhang = LEGACY_BOARD_OVERHANG;
    let board_y_inner = wall_int_y + board_overhang;
    let board_ear = 0.0;

    // Interior board front nose
    out.push(
        path(
            slot,
            vec![
                (-hw - board_ear, board_y_inner),
                (hw + board_ear, board_y_inner),
                (hw + board_ear, wall_int_y),
                (-hw - board_ear, wall_int_y),
            ],
            true,
            false,
        )
        .with_look_role(OpeningLookRole::InteriorBoard),
    );
    // Reveal line along interior wall face
    out.push(
        path(
            slot,
            vec![(-hw, wall_int_y), (hw, wall_int_y)],
            false,
            false,
        )
        .with_look_role(OpeningLookRole::InteriorBoard),
    );

    // Interior reveal side lines (connecting frame to interior wall)
    if frame_half_d < wall_int_y - 1e-4 {
        out.push(
            path(
                slot,
                vec![(-hw, frame_half_d), (-hw, wall_int_y)],
                false,
                false,
            )
            .with_look_role(OpeningLookRole::InteriorBoard),
        );
        out.push(
            path(
                slot,
                vec![(hw, frame_half_d), (hw, wall_int_y)],
                false,
                false,
            )
            .with_look_role(OpeningLookRole::InteriorBoard),
        );
    }

    out
}

pub(super) fn bake_sill_lines_from_parts(
    slot: OpeningComponentSlot,
    params: OpeningBakeParams,
    parts: &OpeningParts,
    library: Option<&StyleLibrary>,
    fixed_frame: &ResolvedSection,
    exterior_sill: &ResolvedSection,
    interior_board: &ResolvedSection,
) -> Vec<BakedPath> {
    let hw = params.width * 0.5;
    let cross_offset = params.cross_axis_offset;
    let wall_ext_y = params.wall_y_min - cross_offset;
    let wall_int_y = params.wall_y_max - cross_offset;
    let frame_y_min = parts.fixed_frame.axis_offset - fixed_frame.depth * 0.5;
    let frame_y_max = parts.fixed_frame.axis_offset + fixed_frame.depth * 0.5;
    let mut out = Vec::new();

    if opening_component_should_render_with_library(
        parts,
        params.kind,
        OpeningLookRole::ExteriorSill,
        library,
    ) && exterior_sill.depth > 0.0
    {
        let overhang = parts.exterior_sill.overhang.max(0.0);
        let (x0, x1) = sill_x_span(
            hw,
            parts.exterior_sill.side_overhang_left,
            parts.exterior_sill.side_overhang_right,
        );
        let sill_outer_y = exterior_sill_y_bounds(wall_ext_y, wall_int_y, overhang, fixed_frame.depth).0;
        if overhang > 1e-9 || x0 < -hw - 1e-9 || x1 > hw + 1e-9 {
            out.push(
                path(
                    slot,
                    vec![(x0, sill_outer_y), (x1, sill_outer_y), (x1, wall_ext_y), (x0, wall_ext_y)],
                    true,
                    false,
                )
                .with_look_role(OpeningLookRole::ExteriorSill),
            );
        }
        out.push(
            path(slot, vec![(-hw, wall_ext_y), (hw, wall_ext_y)], false, false)
                .with_look_role(OpeningLookRole::ExteriorSill),
        );
        if frame_y_min > wall_ext_y + 1e-4 {
            for x in [-hw, hw] {
                out.push(
                    path(slot, vec![(x, frame_y_min), (x, wall_ext_y)], false, false)
                        .with_look_role(OpeningLookRole::ExteriorSill),
                );
            }
        }
    }

    if opening_component_should_render_with_library(
        parts,
        params.kind,
        OpeningLookRole::InteriorBoard,
        library,
    ) && interior_board.depth > 0.0
    {
        let overhang = parts.interior_board.overhang.max(0.0);
        let (x0, x1) = sill_x_span(
            hw,
            parts.interior_board.side_overhang_left,
            parts.interior_board.side_overhang_right,
        );
        let board_inner_y = interior_board_y_bounds(wall_ext_y, wall_int_y, overhang, fixed_frame.depth).1;
        if overhang > 1e-9 || x0 < -hw - 1e-9 || x1 > hw + 1e-9 {
            out.push(
                path(
                    slot,
                    vec![
                        (x0, board_inner_y),
                        (x1, board_inner_y),
                        (x1, wall_int_y),
                        (x0, wall_int_y),
                    ],
                    true,
                    false,
                )
                .with_look_role(OpeningLookRole::InteriorBoard),
            );
        }
        out.push(
            path(slot, vec![(-hw, wall_int_y), (hw, wall_int_y)], false, false)
                .with_look_role(OpeningLookRole::InteriorBoard),
        );
        if frame_y_max < wall_int_y - 1e-4 {
            for x in [-hw, hw] {
                out.push(
                    path(slot, vec![(x, frame_y_max), (x, wall_int_y)], false, false)
                        .with_look_role(OpeningLookRole::InteriorBoard),
                );
            }
        }
    }
    out
}

pub(super) fn bake_glazing_line(slot: OpeningComponentSlot, hw: f64, ft: f64) -> Vec<BakedPath> {
    let post_w = ft.min(hw * 0.45).max(1e-4);
    let clear_hw = (hw - post_w).max(0.0);
    if clear_hw <= 1e-6 {
        return Vec::new();
    }
    vec![
        path(slot, vec![(-clear_hw, -0.012), (clear_hw, -0.012)], false, false),
        path(slot, vec![(-clear_hw, 0.012), (clear_hw, 0.012)], false, false),
    ]
}

pub(super) fn bake_threshold_line(slot: OpeningComponentSlot, hw: f64, ht: f64) -> Vec<BakedPath> {
    let t_w = (0.05_f64).min(ht);
    vec![path(
        slot,
        vec![(-hw, -t_w), (hw, -t_w), (hw, t_w), (-hw, t_w)],
        true,
        false,
    )]
}

pub(super) fn bake_opening_label_preview(slot: OpeningComponentSlot, hw: f64) -> Vec<BakedPath> {
    let stroke = hw.min(0.2);
    vec![path(slot, vec![(-stroke, 0.0), (stroke, 0.0)], false, false)]
}

pub(super) fn bake_elevation_contour(
    slot: OpeningComponentSlot,
    p: OpeningBakeParams,
    hw: f64,
    h: f64,
    ft: f64,
    omit_threshold: bool,
) -> Vec<BakedPath> {
    match p.shape {
        OpeningShape::Arch => {
            let spring = p.spring_height.min(h).max(0.0);
            let mut outer = vec![(-hw, 0.0), (hw, 0.0), (hw, spring)];
            let n = 16;
            for i in 0..=n {
                let angle = (i as f64) / (n as f64) * std::f64::consts::PI;
                let x = hw * angle.cos();
                let z = spring + (h - spring) * angle.sin();
                outer.push((x, z));
            }
            outer.push((-hw, spring));
            let mut res = vec![path(slot, outer, true, false)];
            if ft > 1e-4 && hw > ft && h > 2.0 * ft {
                let inner_hw = hw - ft;
                let inner_h = h - ft;
                let bot = if omit_threshold { 0.0 } else { ft };
                let mut inner = vec![(-inner_hw, bot), (inner_hw, bot), (inner_hw, spring)];
                for i in 0..=n {
                    let angle = (i as f64) / (n as f64) * std::f64::consts::PI;
                    let x = inner_hw * angle.cos();
                    let z = spring + (inner_h - spring) * angle.sin();
                    inner.push((x, z));
                }
                inner.push((-inner_hw, spring));
                res.push(path(slot, inner, true, false));
            }
            res
        }
        _ => {
            let outer = vec![(-hw, 0.0), (hw, 0.0), (hw, h), (-hw, h)];
            let mut res = vec![path(slot, outer, true, false)];
            if ft > 1e-4 && hw > ft && h > 2.0 * ft {
                let bot = if omit_threshold { 0.0 } else { ft };
                let inner = vec![
                    (-hw + ft, bot),
                    (hw - ft, bot),
                    (hw - ft, h - ft),
                    (-hw + ft, h - ft),
                ];
                res.push(path(slot, inner, true, false));
            }
            res
        }
    }
}

pub(super) fn bake_elevation_muntins_single(
    slot: OpeningComponentSlot,
    hw: f64,
    h: f64,
    ft: f64,
) -> Vec<BakedPath> {
    if hw <= ft || h <= 2.0 * ft {
        return Vec::new();
    }
    vec![path(
        slot,
        vec![
            (-hw + ft, ft),
            (hw - ft, ft),
            (hw - ft, h - ft),
            (-hw + ft, h - ft),
        ],
        true,
        false,
    )]
}

pub(super) fn bake_elevation_muntins_double(
    slot: OpeningComponentSlot,
    hw: f64,
    h: f64,
    ft: f64,
) -> Vec<BakedPath> {
    if hw <= ft || h <= 2.0 * ft {
        return Vec::new();
    }
    let mullion_half = ft * 0.5;
    let left_leaf = vec![
        (-hw + ft, ft),
        (-mullion_half, ft),
        (-mullion_half, h - ft),
        (-hw + ft, h - ft),
    ];
    let right_leaf = vec![
        (mullion_half, ft),
        (hw - ft, ft),
        (hw - ft, h - ft),
        (mullion_half, h - ft),
    ];
    let mullion = vec![
        (-mullion_half, 0.0),
        (mullion_half, 0.0),
        (mullion_half, h),
        (-mullion_half, h),
    ];
    vec![
        path(slot, mullion, true, false),
        path(slot, left_leaf, true, false),
        path(slot, right_leaf, true, false),
    ]
}

pub(super) fn bake_elevation_swing_triangle(
    slot: OpeningComponentSlot,
    p: OpeningBakeParams,
    hw: f64,
    h: f64,
    ft: f64,
) -> Vec<BakedPath> {
    let inset_w = (hw - ft).max(0.0);
    let inset_bot = ft;
    let inset_top = (h - ft).max(ft);
    let mid_h = (inset_bot + inset_top) * 0.5;

    match p.hinge.bearing() {
        crate::modules::aec::engine::construction_grid::TrackEnd::Start => {
            let pts = vec![
                (inset_w, inset_bot),
                (-inset_w, mid_h),
                (inset_w, inset_top),
            ];
            vec![path(slot, pts, false, false)]
        }
        crate::modules::aec::engine::construction_grid::TrackEnd::End => {
            let pts = vec![
                (-inset_w, inset_bot),
                (inset_w, mid_h),
                (-inset_w, inset_top),
            ];
            vec![path(slot, pts, false, false)]
        }
    }
}

pub(super) fn bake_elevation_sill_line(slot: OpeningComponentSlot, hw: f64, _h: f64) -> Vec<BakedPath> {
    let ear = 0.0;
    let sill_drop = -0.05;
    vec![
        path(slot, vec![(-hw - ear, 0.0), (hw + ear, 0.0)], false, false),
        path(
            slot,
            vec![
                (-hw - ear, 0.0),
                (-hw - ear, sill_drop),
                (hw + ear, sill_drop),
                (hw + ear, 0.0),
            ],
            true,
            false,
        ),
    ]
}

