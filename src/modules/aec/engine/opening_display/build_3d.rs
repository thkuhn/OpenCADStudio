use super::*;

pub(super) fn format_din1356_num(v: f64) -> String {
    let s3 = format!("{v:.3}");
    if s3.ends_with('0') {
        format!("{v:.2}")
    } else {
        s3
    }
}

/// Formats the opening annotation according to DIN 1356:
/// - Doors: `W / H`
/// - Windows: `W / H` and `BRH ...`
/// - Breakthroughs: `W / H` and `UK ...`
pub fn format_din1356_label(opening: &Opening) -> String {
    let w = format_din1356_num(opening.width);
    let h = format_din1356_num(opening.height);
    let sill = format_din1356_num(opening.sill_height);
    match opening.kind {
        OpeningKind::Door => {
            format!("{w} / {h}")
        }
        OpeningKind::Window => {
            format!("{w} / {h}\\PBRH {sill}")
        }
        OpeningKind::Breakthrough => {
            if let Some(d) = opening.depth {
                let d_str = format_din1356_num(d);
                format!("{w} / {h} / {d_str}\\PUK {sill}")
            } else {
                format!("{w} / {h}\\PUK {sill}")
            }
        }
    }
}

pub fn opening_elevation_loops(
    shape: OpeningShape,
    width: f64,
    height: f64,
    spring_height: f64,
    frame_thickness: f64,
    is_door: bool,
) -> (Vec<(f64, f64)>, Vec<(f64, f64)>) {
    let hw = width * 0.5;
    let ft = frame_thickness.max(0.01).min(hw * 0.45);
    let bot_in = if is_door { 0.0 } else { ft };

    match shape {
        OpeningShape::Rectangle => {
            let outer = vec![
                (-hw, 0.0),
                (hw, 0.0),
                (hw, height),
                (-hw, height),
            ];
            let inner = vec![
                (-hw + ft, bot_in),
                (hw - ft, bot_in),
                (hw - ft, (height - ft).max(bot_in + 0.01)),
                (-hw + ft, (height - ft).max(bot_in + 0.01)),
            ];
            (outer, inner)
        }
        OpeningShape::Circle => {
            let (w, h) = shape.lock_size(width, height, true);
            let r_out = w * 0.5;
            let cy = h * 0.5;
            let r_in = (r_out - ft).max(0.01);
            let n = crate::modules::aec::engine::opening_shape::CIRCLE_CHORD_COUNT.max(16);
            let mut outer = Vec::with_capacity(n);
            let mut inner = Vec::with_capacity(n);
            for i in 0..n {
                let t = (i as f64) * std::f64::consts::TAU / (n as f64);
                outer.push((r_out * t.cos(), cy + r_out * t.sin()));
                inner.push((r_in * t.cos(), cy + r_in * t.sin()));
            }
            (outer, inner)
        }
        OpeningShape::Arch => {
            let spring = crate::modules::aec::engine::opening_shape::clamp_spring(spring_height, height);
            let rise = height - spring;
            let mut outer = vec![(-hw, 0.0), (hw, 0.0), (hw, spring)];
            let n = crate::modules::aec::engine::opening_shape::ARCH_CHORD_COUNT.max(8);
            if rise > 1e-6 {
                let chord = width;
                let radius = chord * chord / (8.0 * rise) + rise * 0.5;
                let cy = height - radius;
                let a0 = (spring - cy).atan2(hw);
                let mut a1 = (spring - cy).atan2(-hw);
                while a1 <= a0 {
                    a1 += std::f64::consts::TAU;
                }
                for i in 1..n {
                    let t = a0 + (a1 - a0) * (i as f64) / (n as f64);
                    outer.push((radius * t.cos(), cy + radius * t.sin()));
                }
            }
            outer.push((-hw, spring));

            let in_hw = (hw - ft).max(0.01);
            let in_h = (height - ft).max(bot_in + 0.02);
            let in_spring = crate::modules::aec::engine::opening_shape::clamp_spring(spring, in_h);
            let in_rise = in_h - in_spring;
            let mut inner = vec![(-in_hw, bot_in), (in_hw, bot_in), (in_hw, in_spring)];
            if in_rise > 1e-6 {
                let in_chord = 2.0 * in_hw;
                let in_radius = in_chord * in_chord / (8.0 * in_rise) + in_rise * 0.5;
                let in_cy = in_h - in_radius;
                let a0 = (in_spring - in_cy).atan2(in_hw);
                let mut a1 = (in_spring - in_cy).atan2(-in_hw);
                while a1 <= a0 {
                    a1 += std::f64::consts::TAU;
                }
                for i in 1..n {
                    let t = a0 + (a1 - a0) * (i as f64) / (n as f64);
                    inner.push((in_radius * t.cos(), in_cy + in_radius * t.sin()));
                }
            }
            inner.push((-in_hw, in_spring));

            (outer, inner)
        }
        OpeningShape::Triangle(variant) => {
            let outer_raw = crate::modules::aec::engine::opening_shape::triangle_polygon(variant, width, height);
            let outer: Vec<(f64, f64)> = outer_raw.into_iter().map(|(s, z)| (s - hw, z)).collect();
            let cx = outer.iter().map(|p| p.0).sum::<f64>() / 3.0;
            let cz = outer.iter().map(|p| p.1).sum::<f64>() / 3.0;
            let scale = ((hw - ft) / hw).max(0.1).min(0.95);
            let inner: Vec<(f64, f64)> = outer.iter().map(|&(x, z)| (cx + (x - cx) * scale, cz + (z - cz) * scale)).collect();
            (outer, inner)
        }
    }
}

pub fn build_faceted_hollow_prism(
    outer_loop: &[(f64, f64)],
    inner_loop: &[(f64, f64)],
    y_min: f64,
    y_max: f64,
) -> Option<cadkernel::brep::Body> {
    let n = outer_loop.len();
    if n < 3 || inner_loop.len() != n {
        return None;
    }

    let mut vertices: Vec<[f64; 3]> = Vec::with_capacity(4 * n);
    for &(x, z) in outer_loop {
        vertices.push([x, y_min, z]);
    }
    for &(x, z) in outer_loop {
        vertices.push([x, y_max, z]);
    }
    for &(x, z) in inner_loop {
        vertices.push([x, y_min, z]);
    }
    for &(x, z) in inner_loop {
        vertices.push([x, y_max, z]);
    }

    let mut faces: Vec<Vec<usize>> = Vec::with_capacity(4 * n);
    for i in 0..n {
        let next = (i + 1) % n;
        // Outer side face (y_min to y_max)
        faces.push(vec![i, next, next + n, i + n]);
        // Inner side face (normal points inwards)
        faces.push(vec![2 * n + i, 3 * n + i, 3 * n + next, 2 * n + next]);
        // Back face quad (y = y_min, normal -Y)
        faces.push(vec![i, 2 * n + i, 2 * n + next, next]);
        // Front face quad (y = y_max, normal +Y)
        faces.push(vec![n + i, n + next, 3 * n + next, 3 * n + i]);
    }

    cadkernel::brep::make::faceted_solid(&vertices, &faces)
}

pub fn build_faceted_prism(
    polygon: &[(f64, f64)],
    y_min: f64,
    y_max: f64,
) -> Option<cadkernel::brep::Body> {
    let n = polygon.len();
    if n < 3 {
        return None;
    }

    let mut vertices: Vec<[f64; 3]> = Vec::with_capacity(2 * n);
    for &(x, z) in polygon {
        vertices.push([x, y_min, z]);
    }
    for &(x, z) in polygon {
        vertices.push([x, y_max, z]);
    }

    let mut faces: Vec<Vec<usize>> = Vec::with_capacity(n + 2);
    for i in 0..n {
        let next = (i + 1) % n;
        faces.push(vec![i, next, next + n, i + n]);
    }
    let back_face: Vec<usize> = (0..n).rev().collect();
    faces.push(back_face);
    let front_face: Vec<usize> = (n..2 * n).collect();
    faces.push(front_face);

    cadkernel::brep::make::faceted_solid(&vertices, &faces)
}

pub fn build_opening_frame_3d(
    width: f64,
    height: f64,
    frame_thickness: f64,
    depth: f64,
    shape: OpeningShape,
    spring_height: f64,
) -> Option<cadkernel::brep::Body> {
    build_opening_frame_3d_with_threshold(
        width,
        height,
        frame_thickness,
        depth,
        shape,
        spring_height,
        true,
    )
}

pub fn build_opening_frame_3d_with_threshold(
    width: f64,
    height: f64,
    frame_thickness: f64,
    depth: f64,
    shape: OpeningShape,
    spring_height: f64,
    include_threshold: bool,
) -> Option<cadkernel::brep::Body> {
    let hd = depth * 0.5;
    let ft = frame_thickness.max(0.02).min(width * 0.5 * 0.45);
    let (outer, inner) = opening_elevation_loops(
        shape,
        width,
        height,
        spring_height,
        ft,
        !include_threshold,
    );
    build_faceted_hollow_prism(&outer, &inner, -hd, hd)
}

pub fn build_opening_leaf_3d(
    width: f64,
    height: f64,
    frame_thickness: f64,
    opening_angle_deg: f64,
    hinge: LeafOperation,
    swing: SwingSide,
    is_door: bool,
    shape: OpeningShape,
    spring_height: f64,
) -> Option<cadkernel::brep::Body> {
    build_opening_leaf_3d_with_depth(
        width,
        height,
        frame_thickness,
        0.04,
        opening_angle_deg,
        hinge,
        swing,
        is_door,
        shape,
        spring_height,
    )
}

pub fn build_opening_leaf_3d_with_depth(
    width: f64,
    height: f64,
    frame_thickness: f64,
    leaf_depth: f64,
    opening_angle_deg: f64,
    hinge: LeafOperation,
    swing: SwingSide,
    is_door: bool,
    shape: OpeningShape,
    spring_height: f64,
) -> Option<cadkernel::brep::Body> {
    build_opening_leaf_ring(
        width,
        height,
        frame_thickness,
        leaf_depth,
        0.0,
        0.0,
        opening_angle_deg,
        hinge,
        swing,
        is_door,
        shape,
        spring_height,
    )
}

/// Leaf as a frame ring. `lateral_overlap` grows the ring past the cell on both
/// sides; `depth_offset` shifts it along the wall normal. A ring that closes
/// the cell falls back to the solid plate.
pub fn build_opening_leaf_ring(
    width: f64,
    height: f64,
    frame_thickness: f64,
    leaf_depth: f64,
    lateral_overlap: f64,
    depth_offset: f64,
    opening_angle_deg: f64,
    hinge: LeafOperation,
    swing: SwingSide,
    is_door: bool,
    shape: OpeningShape,
    spring_height: f64,
) -> Option<cadkernel::brep::Body> {
    let grown = (width + lateral_overlap * 2.0).max(width);
    let hw = grown * 0.5;
    let ft = frame_thickness.max(0.02).min(hw * 0.45);
    let hinge_x = match hinge.bearing() {
        crate::modules::aec::engine::construction_grid::TrackEnd::Start => -hw + ft,
        crate::modules::aec::engine::construction_grid::TrackEnd::End => hw - ft,
    };
    build_opening_leaf_components_3d(
        grown,
        height,
        frame_thickness,
        leaf_depth,
        depth_offset,
        hinge_x,
        opening_angle_deg,
        hinge,
        swing,
        is_door,
        shape,
        spring_height,
        None,
    )
    .map(|(leaf, _)| leaf)
}

pub(crate) fn build_opening_leaf_components_3d(
    width: f64,
    height: f64,
    frame_thickness: f64,
    leaf_depth: f64,
    depth_offset: f64,
    pivot_x: f64,
    opening_angle_deg: f64,
    hinge: LeafOperation,
    swing: SwingSide,
    is_door: bool,
    shape: OpeningShape,
    spring_height: f64,
    glazing_thickness: Option<f64>,
) -> Option<(cadkernel::brep::Body, Option<cadkernel::brep::Body>)> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let hw = width * 0.5;
    let ft = frame_thickness.max(0.02).min(hw * 0.45);
    let (outer, inner) = opening_elevation_loops(shape, width, height, spring_height, ft, is_door);
    let leaf_half_depth = leaf_depth.max(0.001) * 0.5;
    let y0 = -leaf_half_depth + depth_offset;
    let y1 = leaf_half_depth + depth_offset;
    let closed = inner.len() < 3
        || inner.iter().all(|(x, z)| {
            outer
                .first()
                .map(|(ox, oz)| (x - ox).abs() < 1e-6 && (z - oz).abs() < 1e-6)
                .unwrap_or(true)
        });
    let leaf = if closed || is_door {
        build_faceted_prism(&outer, y0, y1)?
    } else {
        build_faceted_hollow_prism(&outer, &inner, y0, y1)?
    };
    let glass = if !is_door && inner.len() >= 3 {
        glazing_thickness.and_then(|thickness| {
            let half_glass = thickness.max(0.001).min(leaf_depth.max(0.001)) * 0.5;
            build_faceted_prism(
                &inner,
                depth_offset - half_glass,
                depth_offset + half_glass,
            )
        })
    } else {
        None
    };

    let swing_sign = swing_toward_exterior(swing);
    let ang = match hinge.bearing() {
        crate::modules::aec::engine::construction_grid::TrackEnd::Start => swing_sign * opening_angle_deg.to_radians(),
        crate::modules::aec::engine::construction_grid::TrackEnd::End => -swing_sign * opening_angle_deg.to_radians(),
    };
    let pivot = [pivot_x, depth_offset, 0.0];
    let leaf = if ang.abs() > 1e-6 {
        crate::scene::model::solid_model::turned(&leaf, 2, ang, pivot)?
    } else {
        leaf
    };
    let glass = glass.and_then(|glass| {
        if ang.abs() > 1e-6 {
            crate::scene::model::solid_model::turned(&glass, 2, ang, pivot)
        } else {
            Some(glass)
        }
    });
    Some((leaf, glass))
}

pub(crate) fn build_opening_box_3d(
    width: f64,
    height: f64,
    depth: f64,
) -> Option<cadkernel::brep::Body> {
    if width <= 0.0 || height <= 0.0 || depth <= 0.0 {
        return None;
    }
    build_faceted_prism(
        &[
            (-width * 0.5, -height * 0.5),
            (width * 0.5, -height * 0.5),
            (width * 0.5, height * 0.5),
            (-width * 0.5, height * 0.5),
        ],
        -depth * 0.5,
        depth * 0.5,
    )
}

pub(super) fn build_opening_box_3d_with_bounds(
    width: f64,
    height: f64,
    y_min: f64,
    y_max: f64,
) -> Option<cadkernel::brep::Body> {
    if width <= 0.0 || height <= 0.0 || y_max <= y_min {
        return None;
    }
    build_faceted_prism(
        &[
            (-width * 0.5, -height * 0.5),
            (width * 0.5, -height * 0.5),
            (width * 0.5, height * 0.5),
            (-width * 0.5, height * 0.5),
        ],
        y_min,
        y_max,
    )
}

pub(crate) fn build_opening_profile_prism_3d(
    width: f64,
    section: &ResolvedSection,
    y_origin: f64,
    inward: f64,
) -> Option<cadkernel::brep::Body> {
    if width <= 0.0 || section.face <= 0.0 || section.depth <= 0.0 {
        return None;
    }
    let mut outline = section.outline.clone();
    if outline.len() > 1 && outline.first() == outline.last() {
        outline.pop();
    }
    if outline.len() < 3 {
        return None;
    }
    let n = outline.len();
    let mut vertices = Vec::with_capacity(n * 2);
    for x in [-width * 0.5, width * 0.5] {
        for &(face, depth) in &outline {
            vertices.push([x, y_origin + inward * depth, face]);
        }
    }
    let mut faces = Vec::with_capacity(n + 2);
    for i in 0..n {
        let next = (i + 1) % n;
        faces.push(vec![i, next, next + n, i + n]);
    }
    faces.push((0..n).rev().collect());
    faces.push((n..2 * n).collect());
    cadkernel::brep::make::faceted_solid(&vertices, &faces)
}

pub(crate) fn build_opening_grid_bars_3d(
    grid: &ConstructionGrid,
    params: OpeningBakeParams,
    post_width: f64,
    depth: f64,
    library: Option<&StyleLibrary>,
    leaf: &ResolvedSection,
) -> Vec<(cadkernel::brep::Body, f64, f64)> {
    let resolved = resolve_grid(grid, params.width, params.height);
    let mut bars = Vec::new();
    collect_grid_bars_3d(
        &resolved,
        0.0,
        0.0,
        params,
        post_width,
        depth,
        library,
        leaf,
        &mut bars,
    );
    bars
}

pub(super) fn collect_grid_bars_3d(
    resolved: &ResolvedGrid,
    offset_x: f64,
    offset_z: f64,
    params: OpeningBakeParams,
    post_width: f64,
    depth: f64,
    library: Option<&StyleLibrary>,
    leaf: &ResolvedSection,
    bars: &mut Vec<(cadkernel::brep::Body, f64, f64)>,
) {
    let origin = params.width * 0.5;
    let mut add_bar = |left: f64, right: f64, bottom: f64, top: f64| {
        if right <= left || top <= bottom || depth <= 0.0 {
            return;
        }
        if let Some(body) = build_opening_box_3d(right - left, top - bottom, depth) {
            bars.push((body, (left + right) * 0.5, (bottom + top) * 0.5));
        }
    };
    for edge in &resolved.edges {
        if edge.meeting != TrackMeeting::Mullion {
            continue;
        }
        match edge.axis {
            BarAxis::Vertical => {
                let x = edge.position + offset_x - origin;
                let bottom = edge.start + offset_z;
                let top = (edge.end + offset_z).min(arch_height_at_x(params, x));
                add_bar(x - post_width * 0.5, x + post_width * 0.5, bottom, top);
            }
            BarAxis::Horizontal => {
                let z = edge.position + offset_z;
                let (left, right) = clipped_horizontal_span(
                    params,
                    z,
                    edge.start + offset_x - origin,
                    edge.end + offset_x - origin,
                );
                add_bar(left, right, z - post_width * 0.5, z + post_width * 0.5);
            }
        }
    }
    for edge in &resolved.edges {
        let Some(shift) = stulp_shift(edge.meeting, leaf.face) else {
            continue;
        };
        if leaf.depth <= 0.0 {
            continue;
        }
        match edge.axis {
            BarAxis::Vertical => {
                let x = edge.position + offset_x - origin + shift;
                let bottom = edge.start + offset_z;
                let top = (edge.end + offset_z).min(arch_height_at_x(params, x));
                if let Some(body) = build_opening_box_3d(leaf.face, top - bottom, leaf.depth) {
                    bars.push((body, x, (bottom + top) * 0.5));
                }
            }
            BarAxis::Horizontal => {
                let z = edge.position + offset_z + shift;
                let (left, right) = clipped_horizontal_span(
                    params,
                    z,
                    edge.start + offset_x - origin,
                    edge.end + offset_x - origin,
                );
                if let Some(body) = build_opening_box_3d(right - left, leaf.face, leaf.depth) {
                    bars.push((body, (left + right) * 0.5, z));
                }
            }
        }
    }
    for cell in &resolved.cells {
        for bar in &cell.bars {
            let Some(section) = muntin_bar_section(
                library,
                resolved.muntin_profile_id.as_deref(),
                bar.face_width,
                bar.depth,
                leaf,
            ) else {
                continue;
            };
            let face = section.face;
            let bar_depth = section.depth;
            match bar.axis {
                BarAxis::Vertical => {
                    let x = bar.position + offset_x - origin;
                    let bottom = bar.start + offset_z;
                    let top = (bar.end + offset_z).min(arch_height_at_x(params, x));
                    if let Some(body) = build_opening_box_3d(face, top - bottom, bar_depth) {
                        bars.push((body, x, (bottom + top) * 0.5));
                    }
                }
                BarAxis::Horizontal => {
                    let z = bar.position + offset_z;
                    let (left, right) = clipped_horizontal_span(
                        params,
                        z,
                        bar.start + offset_x - origin,
                        bar.end + offset_x - origin,
                    );
                    if let Some(body) = build_opening_box_3d(right - left, face, bar_depth) {
                        bars.push((body, (left + right) * 0.5, z));
                    }
                }
            }
        }
        if let Some(nested) = &cell.nested {
            collect_grid_bars_3d(
                nested,
                offset_x + cell.left,
                offset_z + cell.bottom,
                params,
                post_width,
                depth,
                library,
                leaf,
                bars,
            );
        }
    }
}

