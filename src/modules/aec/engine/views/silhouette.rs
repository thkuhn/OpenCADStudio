//! Occlusion analysis, depth classification, and background edge projection (Silhouette).
//!
//! Evaluates visible component edges behind the cut plane or within the elevation depth range,
//! classifying primitives into foreground visible edges, background projected edges, and hidden lines,
//! and clipping occluded background lines against opaque foreground polygons.

use super::provenance::{
    DerivedPrimitive, FeatureKey, Geometry2d, PrimitiveId, PrimitiveRole, ResolvedStyle, Rgba8,
    SourceSet,
};
use super::{PaperMm, Point2, ValidationError};

const EPS: f64 = 1e-6;

/// Transient occluder layer used during floor plan derivation to occlude below-cut elements in openings without baking to CAD.
pub const TRANSIENT_OCCLUDER_LAYER: &str = "AEC_TRANSIENT_OCCLUDER";

/// Silhouette and occlusion options.
#[derive(Clone, Debug, PartialEq)]
pub struct SilhouetteOptions {
    pub emit_hidden_lines: bool,
    pub max_depth_distance: Option<f64>,
    pub hidden_layer: Option<String>,
}

impl Default for SilhouetteOptions {
    fn default() -> Self {
        Self {
            emit_hidden_lines: false,
            max_depth_distance: None,
            hidden_layer: None,
        }
    }
}

struct OpaquePolygon {
    depth_max: f64,
    sources: SourceSet,
    ring: Vec<Point2>,
    holes: Vec<Vec<Point2>>,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl OpaquePolygon {
    fn new(
        depth_max: f64,
        sources: SourceSet,
        ring: Vec<Point2>,
        holes: Vec<Vec<Point2>>,
    ) -> Self {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for p in &ring {
            min_x = min_x.min(p.x_m);
            min_y = min_y.min(p.y_m);
            max_x = max_x.max(p.x_m);
            max_y = max_y.max(p.y_m);
        }
        Self {
            depth_max,
            sources,
            ring,
            holes,
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }
}

/// Applies depth classification, sorting, and occlusion rules to derived primitives.
pub fn process_silhouette_and_depth(
    primitives: &mut Vec<DerivedPrimitive>,
    options: &SilhouetteOptions,
) -> Result<(), ValidationError> {
    // 1. Sort primitives by depth (foreground first)
    primitives.sort_by(|a, b| {
        a.depth
            .min_m
            .partial_cmp(&b.depth.min_m)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // 2. Perform 2D Occlusion Clipping
    let mut opaque_polygons: Vec<OpaquePolygon> = Vec::new();

    for prim in primitives.iter() {
        if prim.role == PrimitiveRole::CutFill || prim.role == PrimitiveRole::SurfaceFill {
            match &prim.geometry {
                Geometry2d::Region { outer, holes } if outer.len() >= 3 => {
                    opaque_polygons.push(OpaquePolygon::new(
                        prim.depth.max_m,
                        prim.sources.clone(),
                        outer.clone(),
                        holes.clone(),
                    ));
                }
                Geometry2d::Polyline { vertices, closed: true } if vertices.len() >= 3 => {
                    opaque_polygons.push(OpaquePolygon::new(
                        prim.depth.max_m,
                        prim.sources.clone(),
                        vertices.clone(),
                        vec![],
                    ));
                }
                _ => {}
            }
        }
    }

    opaque_polygons.sort_by(|a, b| a.depth_max.partial_cmp(&b.depth_max).unwrap_or(std::cmp::Ordering::Equal));

    let mut result_primitives = Vec::with_capacity(primitives.len());
    let mut next_id_index = primitives.len() as u32 + 1000;

    for prim in primitives.drain(..) {
        if opaque_polygons.is_empty() {
            result_primitives.push(prim);
            continue;
        }

        // Glass and leaf fills are surfaces. A closer wall must remove them,
        // otherwise the hatch paints over the wall that should hide the opening.
        if prim.role == PrimitiveRole::SurfaceFill {
            push_occluded_surface(
                &mut result_primitives,
                prim,
                &opaque_polygons,
                &mut next_id_index,
            )?;
            continue;
        }

        // Frames are regions and swing marks are symbols. Both used to skip
        // clipping, so an opening stayed visible through a nearer wall.
        if prim.role != PrimitiveRole::ProjectedEdge && prim.role != PrimitiveRole::Symbol {
            result_primitives.push(prim);
            continue;
        }

        let prim_depth = prim.depth.min_m;

        match prim.geometry {
            Geometry2d::Line { start, end } => {
                let segments = clip_segment_against_polygons(
                    start,
                    end,
                    prim_depth,
                    &prim.sources,
                    &opaque_polygons,
                    options.emit_hidden_lines,
                );

                for (p0, p1, is_hidden) in segments {
                    emit_clipped_segment(
                        &mut result_primitives,
                        &prim,
                        p0,
                        p1,
                        is_hidden,
                        options,
                        &mut next_id_index,
                    )?;
                }
            }
            Geometry2d::Polyline { ref vertices, closed } => {
                let mut hidden = false;
                let mut pieces = Vec::new();
                if loop_fully_visible(
                    vertices,
                    closed,
                    prim_depth,
                    &prim.sources,
                    &opaque_polygons,
                    options.emit_hidden_lines,
                    &mut pieces,
                    &mut hidden,
                ) {
                    result_primitives.push(prim);
                } else {
                    for (p0, p1, is_hidden) in pieces {
                        emit_clipped_segment(
                            &mut result_primitives,
                            &prim,
                            p0,
                            p1,
                            is_hidden,
                            options,
                            &mut next_id_index,
                        )?;
                    }
                }
            }
            Geometry2d::Region { ref outer, ref holes } => {
                let mut hidden = false;
                let mut pieces = Vec::new();
                let outer_ok = loop_fully_visible(
                    outer,
                    true,
                    prim_depth,
                    &prim.sources,
                    &opaque_polygons,
                    options.emit_hidden_lines,
                    &mut pieces,
                    &mut hidden,
                );
                let holes_ok = holes.iter().all(|hole| {
                    loop_fully_visible(
                        hole,
                        true,
                        prim_depth,
                        &prim.sources,
                        &opaque_polygons,
                        options.emit_hidden_lines,
                        &mut pieces,
                        &mut hidden,
                    )
                });
                if outer_ok && holes_ok {
                    result_primitives.push(prim);
                } else {
                    for (p0, p1, is_hidden) in pieces {
                        emit_clipped_segment(
                            &mut result_primitives,
                            &prim,
                            p0,
                            p1,
                            is_hidden,
                            options,
                            &mut next_id_index,
                        )?;
                    }
                }
            }
            other => {
                result_primitives.push(DerivedPrimitive {
                    geometry: other,
                    ..prim
                });
            }
        }
    }

    merge_and_deduplicate_segments(&mut result_primitives);
    suppress_coincident_hidden_edges(&mut result_primitives);
    merge_and_deduplicate_segments(&mut result_primitives);

    result_primitives.retain(|p| p.style.layer_name.as_deref() != Some(TRANSIENT_OCCLUDER_LAYER));

    *primitives = result_primitives;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SegmentState {
    Visible,
    Hidden,
    CollinearBoundary,
}

fn resolve_hidden_layer(prim: &DerivedPrimitive, options: &SilhouetteOptions) -> String {
    if let Some(l) = &prim.style.layer_name {
        if l == "AEC_OVERHEAD" {
            return "AEC_OVERHEAD".to_string();
        }
        if l.starts_with("AEC_SECTION") {
            return "AEC_SECTION_HIDDEN".to_string();
        }
    }
    if prim.role == PrimitiveRole::CutBoundary || prim.role == PrimitiveRole::CutFill {
        return "AEC_SECTION_HIDDEN".to_string();
    }
    if let Some(ref hl) = options.hidden_layer {
        return hl.clone();
    }
    "AEC_HIDDEN".to_string()
}

fn emit_clipped_segment(
    result_primitives: &mut Vec<DerivedPrimitive>,
    prim: &DerivedPrimitive,
    p0: Point2,
    p1: Point2,
    is_hidden: bool,
    options: &SilhouetteOptions,
    next_id_index: &mut u32,
) -> Result<(), ValidationError> {
    if (p1.x_m - p0.x_m).hypot(p1.y_m - p0.y_m) < 1e-4 {
        return Ok(());
    }
    if is_hidden {
        if options.emit_hidden_lines {
            let hidden_layer = resolve_hidden_layer(prim, options);
            let mut hidden_prim = prim.clone();
            *next_id_index += 1;
            hidden_prim.id = PrimitiveId::new(prim.id.view_id, 0, *next_id_index);
            hidden_prim.role = PrimitiveRole::HiddenEdge;
            hidden_prim.geometry = Geometry2d::Line { start: p0, end: p1 };
            hidden_prim.style = ResolvedStyle {
                line_weight_mm: PaperMm::new(0.18)?,
                line_color: Some(Rgba8::new(128, 128, 128, 255)),
                line_type: Some("DASHED".to_string()),
                fill_color: None,
                hatch_pattern: None,
                hatch_scale: None,
                hatch_angle_rad: None,
                layer_name: Some(hidden_layer),
            };
            result_primitives.push(hidden_prim);
        }
    } else {
        let mut vis_prim = prim.clone();
        *next_id_index += 1;
        vis_prim.id = PrimitiveId::new(prim.id.view_id, 0, *next_id_index);
        vis_prim.geometry = Geometry2d::Line { start: p0, end: p1 };
        result_primitives.push(vis_prim);
    }
    Ok(())
}

/// Returns true when every edge stays a single visible piece.
fn loop_fully_visible(
    vertices: &[Point2],
    closed: bool,
    prim_depth: f64,
    sources: &SourceSet,
    polygons: &[OpaquePolygon],
    emit_hidden: bool,
    pieces: &mut Vec<(Point2, Point2, bool)>,
    hidden: &mut bool,
) -> bool {
    let count = vertices.len();
    if count < 2 {
        return true;
    }
    let num_edges = if closed { count } else { count - 1 };
    let mut intact = true;
    for i in 0..num_edges {
        let p0 = vertices[i];
        let p1 = vertices[(i + 1) % count];
        let segs = clip_segment_against_polygons(p0, p1, prim_depth, sources, polygons, emit_hidden);
        if segs.len() != 1 || segs[0].2 {
            intact = false;
        } else if segs[0].0.distance_to(p0) > 1e-4 || segs[0].1.distance_to(p1) > 1e-4 {
            intact = false;
        }
        if segs.iter().any(|seg| seg.2) {
            *hidden = true;
        }
        pieces.extend(segs);
    }
    intact
}

fn occluder_hides_segment(poly: &OpaquePolygon, seg_depth: f64, segment_sources: &SourceSet) -> bool {
    if poly.depth_max >= seg_depth - 1e-4 {
        return false;
    }
    // The opening's own glass sits a millimetre in front of its symbol.
    // A cut-plane glass sheet is much closer and must still hide that symbol.
    let ahead = seg_depth - poly.depth_max;
    if ahead <= 5.0e-3
        && poly.sources.as_slice().iter().any(|occluder_source| {
            occluder_source.feature == Some(FeatureKey::OpeningSill)
                && segment_sources
                    .as_slice()
                    .iter()
                    .any(|segment_source| segment_source.element == occluder_source.element)
        })
    {
        return false;
    }
    true
}

fn polygon_area(ring: &[Point2]) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .take(ring.len())
        .map(|(a, b)| a.x_m * b.y_m - b.x_m * a.y_m)
        .sum::<f64>()
        .abs()
        * 0.5
}

fn signed_polygon_area(ring: &[Point2]) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .take(ring.len())
        .map(|(a, b)| a.x_m * b.y_m - b.x_m * a.y_m)
        .sum::<f64>()
        * 0.5
}

fn cross(a: Point2, b: Point2, c: Point2) -> f64 {
    (b.x_m - a.x_m) * (c.y_m - a.y_m) - (b.y_m - a.y_m) * (c.x_m - a.x_m)
}

fn point_in_triangle(point: Point2, a: Point2, b: Point2, c: Point2, orientation: f64) -> bool {
    cross(a, b, point) * orientation >= -EPS
        && cross(b, c, point) * orientation >= -EPS
        && cross(c, a, point) * orientation >= -EPS
}

/// Triangulates a simple ring using ear clipping. Triangles are used as convex
/// masks so both rotated rectangles and concave wall profiles can be clipped.
fn triangulate_ring(ring: &[Point2]) -> Option<Vec<Vec<Point2>>> {
    let mut vertices = ring.to_vec();
    if vertices.len() > 1 && vertices[0].distance_to(*vertices.last()?) < EPS {
        vertices.pop();
    }
    if vertices.len() < 3 {
        return Some(Vec::new());
    }
    let orientation = signed_polygon_area(&vertices).signum();
    if orientation == 0.0 {
        return None;
    }

    let mut indices: Vec<usize> = (0..vertices.len()).collect();
    let mut triangles = Vec::with_capacity(vertices.len() - 2);
    while indices.len() > 3 {
        let mut ear = None;
        for i in 0..indices.len() {
            let prev = indices[(i + indices.len() - 1) % indices.len()];
            let curr = indices[i];
            let next = indices[(i + 1) % indices.len()];
            let (a, b, c) = (vertices[prev], vertices[curr], vertices[next]);
            if cross(a, b, c) * orientation <= EPS {
                continue;
            }
            if indices.iter().copied().any(|index| {
                index != prev
                    && index != curr
                    && index != next
                    && point_in_triangle(vertices[index], a, b, c, orientation)
            }) {
                continue;
            }
            ear = Some((i, vec![a, b, c]));
            break;
        }
        let (index, triangle) = ear?;
        triangles.push(triangle);
        indices.remove(index);
    }
    triangles.push(indices.into_iter().map(|index| vertices[index]).collect());
    Some(triangles)
}

fn clip_half_plane(
    polygon: &[Point2],
    edge_start: Point2,
    edge_end: Point2,
    orientation: f64,
    keep_inside: bool,
) -> Vec<Point2> {
    if polygon.is_empty() {
        return Vec::new();
    }
    let signed_distance = |point| cross(edge_start, edge_end, point) * orientation;
    let is_kept = |distance: f64| {
        if keep_inside { distance >= -EPS } else { distance <= EPS }
    };
    let mut clipped = Vec::new();
    let mut previous = *polygon.last().unwrap();
    let mut previous_distance = signed_distance(previous);
    for current in polygon.iter().copied() {
        let current_distance = signed_distance(current);
        let previous_inside = is_kept(previous_distance);
        let current_inside = is_kept(current_distance);
        if previous_inside != current_inside {
            let denominator = previous_distance - current_distance;
            if denominator.abs() > EPS {
                let t = (previous_distance / denominator).clamp(0.0, 1.0);
                clipped.push(Point2::new(
                    previous.x_m + (current.x_m - previous.x_m) * t,
                    previous.y_m + (current.y_m - previous.y_m) * t,
                ));
            }
        }
        if current_inside {
            clipped.push(current);
        }
        previous = current;
        previous_distance = current_distance;
    }
    deduplicate_ring(&mut clipped);
    clipped
}

fn deduplicate_ring(ring: &mut Vec<Point2>) {
    ring.dedup_by(|a, b| a.distance_to(*b) < EPS);
    if ring.len() > 1 && ring[0].distance_to(*ring.last().unwrap()) < EPS {
        ring.pop();
    }
}

fn subtract_convex(subject: &[Point2], cutter: &[Point2]) -> Vec<Vec<Point2>> {
    if subject.len() < 3 || cutter.len() < 3 {
        return vec![subject.to_vec()];
    }
    let orientation = signed_polygon_area(cutter).signum();
    if orientation == 0.0 {
        return vec![subject.to_vec()];
    }
    let mut intersection_candidates = vec![subject.to_vec()];
    let mut visible = Vec::new();
    for index in 0..cutter.len() {
        let edge_start = cutter[index];
        let edge_end = cutter[(index + 1) % cutter.len()];
        let mut next_candidates = Vec::new();
        for candidate in intersection_candidates {
            let outside = clip_half_plane(&candidate, edge_start, edge_end, orientation, false);
            if polygon_area(&outside) > EPS {
                visible.push(outside);
            }
            let inside = clip_half_plane(&candidate, edge_start, edge_end, orientation, true);
            if polygon_area(&inside) > EPS {
                next_candidates.push(inside);
            }
        }
        intersection_candidates = next_candidates;
        if intersection_candidates.is_empty() {
            break;
        }
    }
    if visible.is_empty() && !intersection_candidates.is_empty() {
        return Vec::new();
    }
    visible
}

fn intersect_convex(subject: &[Point2], cutter: &[Point2]) -> Vec<Vec<Point2>> {
    if subject.len() < 3 || cutter.len() < 3 {
        return Vec::new();
    }
    let orientation = signed_polygon_area(cutter).signum();
    if orientation == 0.0 {
        return Vec::new();
    }
    let mut intersections = vec![subject.to_vec()];
    for index in 0..cutter.len() {
        let edge_start = cutter[index];
        let edge_end = cutter[(index + 1) % cutter.len()];
        intersections = intersections
            .iter()
            .map(|polygon| clip_half_plane(polygon, edge_start, edge_end, orientation, true))
            .filter(|polygon| polygon_area(polygon) > EPS)
            .collect();
        if intersections.is_empty() {
            break;
        }
    }
    intersections
}

fn subtract_ring(pieces: Vec<Vec<Point2>>, cutter_ring: &[Point2]) -> Vec<Vec<Point2>> {
    let Some(cutter_triangles) = triangulate_ring(cutter_ring) else {
        return pieces;
    };
    cutter_triangles.into_iter().fold(pieces, |pieces, triangle| {
        pieces
            .into_iter()
            .flat_map(|piece| subtract_convex(&piece, &triangle))
            .collect()
    })
}

fn apply_occluder_to_pieces(pieces: Vec<Vec<Point2>>, poly: &OpaquePolygon) -> Vec<Vec<Point2>> {
    let mut visible = subtract_ring(pieces.clone(), &poly.ring);
    for hole in &poly.holes {
        let Some(hole_triangles) = triangulate_ring(hole) else {
            continue;
        };
        for piece in &pieces {
            for triangle in &hole_triangles {
                visible.extend(intersect_convex(piece, triangle));
            }
        }
    }
    visible
}

fn push_occluded_surface(
    result_primitives: &mut Vec<DerivedPrimitive>,
    prim: DerivedPrimitive,
    polygons: &[OpaquePolygon],
    next_id_index: &mut u32,
) -> Result<(), ValidationError> {
    let Geometry2d::Region { outer, holes } = &prim.geometry else {
        result_primitives.push(prim);
        return Ok(());
    };
    let depth = prim.depth.min_m;
    let Some(mut remain) = triangulate_ring(outer) else {
        result_primitives.push(prim);
        return Ok(());
    };
    for hole in holes {
        remain = subtract_ring(remain, hole);
    }

    for poly in polygons {
        if !occluder_hides_segment(poly, depth, &prim.sources) {
            continue;
        }
        remain = apply_occluder_to_pieces(remain, poly);
        if remain.is_empty() {
            return Ok(());
        }
    }

    let original_area = polygon_area(outer) - holes.iter().map(|hole| polygon_area(hole)).sum::<f64>();
    let visible_area = remain.iter().map(|piece| polygon_area(piece)).sum::<f64>();
    if (visible_area - original_area).abs() < 1e-6 {
        result_primitives.push(prim);
        return Ok(());
    }

    for ring in remain {
        *next_id_index += 1;
        let mut piece = prim.clone();
        piece.id = PrimitiveId::new(prim.id.view_id, 0, *next_id_index);
        piece.geometry = Geometry2d::Region {
            outer: ring,
            holes: vec![],
        };
        piece.edit_handles.clear();
        result_primitives.push(piece);
    }
    Ok(())
}

fn clip_segment_against_polygons(
    start: Point2,
    end: Point2,
    seg_depth: f64,
    segment_sources: &SourceSet,
    polygons: &[OpaquePolygon],
    emit_hidden: bool,
) -> Vec<(Point2, Point2, bool)> {
    let dx = end.x_m - start.x_m;
    let dy = end.y_m - start.y_m;
    let total_len = dx.hypot(dy);
    if total_len < 1e-4 {
        return Vec::new();
    }

    let seg_min_x = start.x_m.min(end.x_m) - 1e-4;
    let seg_min_y = start.y_m.min(end.y_m) - 1e-4;
    let seg_max_x = start.x_m.max(end.x_m) + 1e-4;
    let seg_max_y = start.y_m.max(end.y_m) + 1e-4;

    let mut current_intervals: Vec<(f64, f64, SegmentState)> = vec![(0.0, 1.0, SegmentState::Visible)];

    for poly in polygons {
        if !occluder_hides_segment(poly, seg_depth, segment_sources) {
            continue;
        }

        if seg_max_x < poly.min_x || seg_min_x > poly.max_x || seg_max_y < poly.min_y || seg_min_y > poly.max_y {
            continue;
        }

        let mut next_intervals = Vec::new();
        for (t0, t1, state) in current_intervals {
            if state == SegmentState::CollinearBoundary {
                next_intervals.push((t0, t1, state));
                continue;
            }

            let sub_p0 = Point2::new(start.x_m + dx * t0, start.y_m + dy * t0);
            let sub_p1 = Point2::new(start.x_m + dx * t1, start.y_m + dy * t1);
            let sub_dx = sub_p1.x_m - sub_p0.x_m;
            let sub_dy = sub_p1.y_m - sub_p0.y_m;
            let sub_len = sub_dx.hypot(sub_dy);

            if sub_len < 1e-6 {
                continue;
            }

            let mut u_params = vec![0.0, 1.0];
            collect_edge_splits(sub_p0, sub_p1, &poly.ring, &mut u_params);
            for hole in &poly.holes {
                collect_edge_splits(sub_p0, sub_p1, hole, &mut u_params);
            }

            u_params.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            u_params.dedup_by(|a, b| (*a - *b).abs() < 1e-6);

            for window in u_params.windows(2) {
                let u_a = window[0];
                let u_b = window[1];
                if u_b - u_a < 1e-6 {
                    continue;
                }

                let mid = Point2::new(
                    sub_p0.x_m + sub_dx * (u_a + u_b) * 0.5,
                    sub_p0.y_m + sub_dy * (u_a + u_b) * 0.5,
                );

                let on_ring_bd = point_on_polygon_boundary(mid, &poly.ring, 1e-4);
                let on_hole_bd = poly.holes.iter().any(|h| point_on_polygon_boundary(mid, h, 1e-4));
                let inside_ring = point_in_polygon(mid, &poly.ring);
                let in_hole = poly.holes.iter().any(|h| point_in_polygon(mid, h));

                let sub_state = if on_ring_bd || on_hole_bd {
                    SegmentState::CollinearBoundary
                } else if inside_ring && !in_hole {
                    SegmentState::Hidden
                } else {
                    state
                };

                let global_ta = t0 + u_a * (t1 - t0);
                let global_tb = t0 + u_b * (t1 - t0);
                next_intervals.push((global_ta, global_tb, sub_state));
            }
        }
        current_intervals = next_intervals;
    }

    current_intervals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // Merge adjacent intervals with identical state
    let mut merged: Vec<(f64, f64, SegmentState)> = Vec::new();
    for (t0, t1, state) in current_intervals {
        if (t1 - t0) * total_len < 1e-4 {
            continue; // Filter zero/sub-millimeter segment
        }
        if let Some(last) = merged.last_mut() {
            if last.2 == state && (t0 - last.1).abs() < 1e-5 {
                last.1 = t1;
                continue;
            }
        }
        merged.push((t0, t1, state));
    }

    let mut results = Vec::new();
    for (t0, t1, state) in merged {
        match state {
            SegmentState::CollinearBoundary => {
                // Collinear with foreground boundary edge: suppressed to avoid double / artifact lines.
            }
            SegmentState::Hidden => {
                if emit_hidden {
                    let p0 = Point2::new(start.x_m + dx * t0, start.y_m + dy * t0);
                    let p1 = Point2::new(start.x_m + dx * t1, start.y_m + dy * t1);
                    results.push((p0, p1, true));
                }
            }
            SegmentState::Visible => {
                let p0 = Point2::new(start.x_m + dx * t0, start.y_m + dy * t0);
                let p1 = Point2::new(start.x_m + dx * t1, start.y_m + dy * t1);
                results.push((p0, p1, false));
            }
        }
    }

    results
}

fn collect_edge_splits(p0: Point2, p1: Point2, ring: &[Point2], t_params: &mut Vec<f64>) {
    let n = ring.len();
    for i in 0..n {
        let e0 = ring[i];
        let e1 = ring[(i + 1) % n];

        if let Some(t) = line_segment_intersection(p0, p1, e0, e1) {
            if t > EPS && t < 1.0 - EPS {
                t_params.push(t);
            }
        }

        if let Some((t0, t1)) = colinear_segment_projection(p0, p1, e0, e1, 1e-4) {
            if t0 > EPS && t0 < 1.0 - EPS {
                t_params.push(t0);
            }
            if t1 > EPS && t1 < 1.0 - EPS {
                t_params.push(t1);
            }
        }
    }
}

fn colinear_segment_projection(
    p0: Point2,
    p1: Point2,
    e0: Point2,
    e1: Point2,
    eps: f64,
) -> Option<(f64, f64)> {
    let p_dx = p1.x_m - p0.x_m;
    let p_dy = p1.y_m - p0.y_m;
    let p_len_sq = p_dx * p_dx + p_dy * p_dy;
    if p_len_sq < 1e-12 {
        return None;
    }
    let p_len = p_len_sq.sqrt();
    let cross0 = ((e0.x_m - p0.x_m) * p_dy - (e0.y_m - p0.y_m) * p_dx).abs() / p_len;
    let cross1 = ((e1.x_m - p0.x_m) * p_dy - (e1.y_m - p0.y_m) * p_dx).abs() / p_len;
    if cross0 <= eps && cross1 <= eps {
        let t0 = ((e0.x_m - p0.x_m) * p_dx + (e0.y_m - p0.y_m) * p_dy) / p_len_sq;
        let t1 = ((e1.x_m - p0.x_m) * p_dx + (e1.y_m - p0.y_m) * p_dy) / p_len_sq;
        Some((t0, t1))
    } else {
        None
    }
}

fn point_on_segment(pt: Point2, a: Point2, b: Point2, eps: f64) -> bool {
    let ab_x = b.x_m - a.x_m;
    let ab_y = b.y_m - a.y_m;
    let ap_x = pt.x_m - a.x_m;
    let ap_y = pt.y_m - a.y_m;
    let len_sq = ab_x * ab_x + ab_y * ab_y;
    if len_sq < 1e-12 {
        return ap_x.hypot(ap_y) <= eps;
    }
    let t = (ap_x * ab_x + ap_y * ab_y) / len_sq;
    if t < -eps || t > 1.0 + eps {
        return false;
    }
    let proj_x = a.x_m + t.clamp(0.0, 1.0) * ab_x;
    let proj_y = a.y_m + t.clamp(0.0, 1.0) * ab_y;
    (pt.x_m - proj_x).hypot(pt.y_m - proj_y) <= eps
}

pub(crate) fn point_on_polygon_boundary(pt: Point2, ring: &[Point2], eps: f64) -> bool {
    let n = ring.len();
    if n < 2 {
        return false;
    }
    for i in 0..n {
        let e0 = ring[i];
        let e1 = ring[(i + 1) % n];
        if point_on_segment(pt, e0, e1, eps) {
            return true;
        }
    }
    false
}

fn line_segment_intersection(
    p0: Point2,
    p1: Point2,
    e0: Point2,
    e1: Point2,
) -> Option<f64> {
    let p_dx = p1.x_m - p0.x_m;
    let p_dy = p1.y_m - p0.y_m;
    let e_dx = e1.x_m - e0.x_m;
    let e_dy = e1.y_m - e0.y_m;

    let denom = p_dx * e_dy - p_dy * e_dx;
    if denom.abs() < 1e-10 {
        return None;
    }

    let t = ((e0.x_m - p0.x_m) * e_dy - (e0.y_m - p0.y_m) * e_dx) / denom;
    let u = ((e0.x_m - p0.x_m) * p_dy - (e0.y_m - p0.y_m) * p_dx) / denom;

    if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
        Some(t)
    } else {
        None
    }
}

pub(crate) fn point_in_polygon(pt: Point2, ring: &[Point2]) -> bool {
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let x = pt.x_m;
    let y = pt.y_m;

    let mut j = n - 1;
    for i in 0..n {
        let xi = ring[i].x_m;
        let yi = ring[i].y_m;
        let xj = ring[j].x_m;
        let yj = ring[j].y_m;

        let intersect = ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi + 1e-14) + xi);
        if intersect {
            inside = !inside;
        }
        j = i;
    }

    inside
}

/// Merges collinear, overlapping, or touching line segments of identical role, style,
/// sources, and depth, and deduplicates redundant line segments across different primitives.
pub fn merge_and_deduplicate_segments(primitives: &mut Vec<DerivedPrimitive>) {
    let mut i = 0;
    while i < primitives.len() {
        let (role_i, layer_i, lt_i, weight_i, color_i, sources_i, depth_min_i, a0, a1) = match &primitives[i].geometry {
            Geometry2d::Line { start, end } => (
                primitives[i].role,
                primitives[i].style.layer_name.clone(),
                primitives[i].style.line_type.clone(),
                primitives[i].style.line_weight_mm,
                primitives[i].style.line_color,
                primitives[i].sources.clone(),
                primitives[i].depth.min_m,
                *start,
                *end,
            ),
            _ => {
                i += 1;
                continue;
            }
        };

        let dx_a = a1.x_m - a0.x_m;
        let dy_a = a1.y_m - a0.y_m;
        let len_a = dx_a.hypot(dy_a);
        if len_a < 1e-4 {
            primitives.swap_remove(i);
            continue;
        }

        let dir_a_x = dx_a / len_a;
        let dir_a_y = dy_a / len_a;

        let mut j = i + 1;
        let mut modified = false;

        while j < primitives.len() {
            if primitives[j].role != role_i
                || primitives[j].style.layer_name != layer_i
                || primitives[j].style.line_type != lt_i
                || primitives[j].style.line_weight_mm != weight_i
                || primitives[j].style.line_color != color_i
                || primitives[j].sources != sources_i
                || (primitives[j].depth.min_m - depth_min_i).abs() > 1e-4
            {
                j += 1;
                continue;
            }

            let (b0, b1) = match &primitives[j].geometry {
                Geometry2d::Line { start, end } => (*start, *end),
                _ => {
                    j += 1;
                    continue;
                }
            };

            let dist_b0 = ((b0.x_m - a0.x_m) * dir_a_y - (b0.y_m - a0.y_m) * dir_a_x).abs();
            let dist_b1 = ((b1.x_m - a0.x_m) * dir_a_y - (b1.y_m - a0.y_m) * dir_a_x).abs();

            if dist_b0 < 1e-4 && dist_b1 < 1e-4 {
                // Collinear! Project B onto A's axis
                let t_b0 = (b0.x_m - a0.x_m) * dir_a_x + (b0.y_m - a0.y_m) * dir_a_y;
                let t_b1 = (b1.x_m - a0.x_m) * dir_a_x + (b1.y_m - a0.y_m) * dir_a_y;
                let min_b = t_b0.min(t_b1);
                let max_b = t_b0.max(t_b1);

                // Check overlap or touching with A's [0.0, len_a]
                if max_b >= -1e-4 && min_b <= len_a + 1e-4 {
                    let new_min = 0.0_f64.min(min_b);
                    let new_max = len_a.max(max_b);
                    let new_start = Point2::new(a0.x_m + dir_a_x * new_min, a0.y_m + dir_a_y * new_min);
                    let new_end = Point2::new(a0.x_m + dir_a_x * new_max, a0.y_m + dir_a_y * new_max);

                    primitives[i].geometry = Geometry2d::Line {
                        start: new_start,
                        end: new_end,
                    };
                    primitives[i].depth.max_m = primitives[i].depth.max_m.max(primitives[j].depth.max_m);

                    let extra_handles = primitives[j].edit_handles.clone();
                    for eh in extra_handles {
                        if !primitives[i].edit_handles.contains(&eh) {
                            primitives[i].edit_handles.push(eh);
                        }
                    }

                    primitives.swap_remove(j);
                    modified = true;
                    break;
                } else {
                    j += 1;
                }
            } else {
                j += 1;
            }
        }

        if !modified {
            i += 1;
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SegmentAabb {
    p0: Point2,
    p1: Point2,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl SegmentAabb {
    fn new(p0: Point2, p1: Point2) -> Self {
        let (min_x, max_x) = if p0.x_m <= p1.x_m { (p0.x_m, p1.x_m) } else { (p1.x_m, p0.x_m) };
        let (min_y, max_y) = if p0.y_m <= p1.y_m { (p0.y_m, p1.y_m) } else { (p1.y_m, p0.y_m) };
        Self {
            p0,
            p1,
            min_x: min_x - 1e-4,
            min_y: min_y - 1e-4,
            max_x: max_x + 1e-4,
            max_y: max_y + 1e-4,
        }
    }

    #[inline]
    fn overlaps_aabb(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> bool {
        !(self.max_x < min_x || self.min_x > max_x || self.max_y < min_y || self.min_y > max_y)
    }
}

struct CutPolygonAabb {
    ring: Vec<Point2>,
    holes: Vec<Vec<Point2>>,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl CutPolygonAabb {
    fn new(ring: Vec<Point2>, holes: Vec<Vec<Point2>>) -> Self {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for p in &ring {
            min_x = min_x.min(p.x_m);
            min_y = min_y.min(p.y_m);
            max_x = max_x.max(p.x_m);
            max_y = max_y.max(p.y_m);
        }
        Self {
            ring,
            holes,
            min_x: min_x - 1e-4,
            min_y: min_y - 1e-4,
            max_x: max_x + 1e-4,
            max_y: max_y + 1e-4,
        }
    }

    #[inline]
    fn overlaps_aabb(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> bool {
        !(self.max_x < min_x || self.min_x > max_x || self.max_y < min_y || self.min_y > max_y)
    }
}

/// Suppresses hidden or overhead edges that coincide with visible or cut edges,
/// or lie inside cut regions, using interval-based subtraction and trimming.
pub fn suppress_coincident_hidden_edges(primitives: &mut Vec<DerivedPrimitive>) {
    // 1. Explode any Polyline with role HiddenEdge into Line segments
    let mut exploded = Vec::with_capacity(primitives.len());
    let mut next_id_idx = primitives.len() as u32 + 2000;
    for prim in primitives.drain(..) {
        if prim.role == PrimitiveRole::HiddenEdge {
            match prim.geometry {
                Geometry2d::Polyline { ref vertices, closed } => {
                    let count = vertices.len();
                    let num_edges = if closed { count } else { count.saturating_sub(1) };
                    for idx in 0..num_edges {
                        let p0 = vertices[idx];
                        let p1 = vertices[(idx + 1) % count];
                        if (p1.x_m - p0.x_m).hypot(p1.y_m - p0.y_m) >= 1e-4 {
                            next_id_idx += 1;
                            let mut seg_prim = prim.clone();
                            seg_prim.id = PrimitiveId::new(prim.id.view_id, 0, next_id_idx);
                            seg_prim.geometry = Geometry2d::Line { start: p0, end: p1 };
                            exploded.push(seg_prim);
                        }
                    }
                }
                _ => exploded.push(prim),
            }
        } else {
            exploded.push(prim);
        }
    }
    *primitives = exploded;

    // 2. Collect visible boundary segments and cut polygons with AABBs
    let mut visible_segments = Vec::new();
    let mut cut_polygons = Vec::new();

    for prim in primitives.iter() {
        if prim.role == PrimitiveRole::CutBoundary || prim.role == PrimitiveRole::ProjectedEdge {
            match &prim.geometry {
                Geometry2d::Line { start, end } => {
                    if (end.x_m - start.x_m).hypot(end.y_m - start.y_m) >= 1e-4 {
                        visible_segments.push(SegmentAabb::new(*start, *end));
                    }
                }
                Geometry2d::Polyline { vertices, closed } => {
                    let n = vertices.len();
                    let count = if *closed { n } else { n.saturating_sub(1) };
                    for idx in 0..count {
                        let p0 = vertices[idx];
                        let p1 = vertices[(idx + 1) % n];
                        if (p1.x_m - p0.x_m).hypot(p1.y_m - p0.y_m) >= 1e-4 {
                            visible_segments.push(SegmentAabb::new(p0, p1));
                        }
                    }
                }
                _ => {}
            }
        } else if prim.role == PrimitiveRole::CutFill {
            match &prim.geometry {
                Geometry2d::Region { outer, holes } if outer.len() >= 3 => {
                    cut_polygons.push(CutPolygonAabb::new(outer.clone(), holes.clone()));
                }
                Geometry2d::Polyline { vertices, closed: true } if vertices.len() >= 3 => {
                    cut_polygons.push(CutPolygonAabb::new(vertices.clone(), vec![]));
                }
                _ => {}
            }
        }
    }

    // 3. Trim each HiddenEdge segment against visible segments, cut polygons,
    // and already kept hidden segments (deduplicating overlapping hidden lines across distinct sources)
    let mut result_primitives = Vec::with_capacity(primitives.len());
    let mut kept_hidden_segments: Vec<SegmentAabb> = Vec::new();

    for prim in primitives.drain(..) {
        if prim.role != PrimitiveRole::HiddenEdge {
            result_primitives.push(prim);
            continue;
        }

        let (h0, h1) = match prim.geometry {
            Geometry2d::Line { start, end } => (start, end),
            _ => {
                result_primitives.push(prim);
                continue;
            }
        };

        let dx = h1.x_m - h0.x_m;
        let dy = h1.y_m - h0.y_m;
        let len_h = dx.hypot(dy);
        if len_h < 1e-4 {
            continue;
        }

        let dir_x = dx / len_h;
        let dir_y = dy / len_h;

        let (h_min_x, h_max_x) = if h0.x_m <= h1.x_m { (h0.x_m, h1.x_m) } else { (h1.x_m, h0.x_m) };
        let (h_min_y, h_max_y) = if h0.y_m <= h1.y_m { (h0.y_m, h1.y_m) } else { (h1.y_m, h0.y_m) };

        let is_overhead = prim
            .style
            .layer_name
            .as_deref()
            .map(|l| l == "AEC_OVERHEAD")
            .unwrap_or(false);

        let is_opening_overhead = prim.sources.as_slice().iter().any(|s| {
            matches!(
                s.feature,
                Some(FeatureKey::OpeningHead)
                    | Some(FeatureKey::OpeningLeftJamb)
                    | Some(FeatureKey::OpeningRightJamb)
                    | Some(FeatureKey::OpeningSill)
            )
        });

        let suppress_inside_cut = is_overhead && !is_opening_overhead;

        let mut split_params = vec![0.0, 1.0];
        let mut covered_intervals = Vec::new();

        // Check against visible segments (AABB pre-filter + collinear coverage)
        for seg in &visible_segments {
            if !seg.overlaps_aabb(h_min_x, h_min_y, h_max_x, h_max_y) {
                continue;
            }
            let v0 = seg.p0;
            let v1 = seg.p1;
            let dist_v0 = ((v0.x_m - h0.x_m) * dir_y - (v0.y_m - h0.y_m) * dir_x).abs();
            let dist_v1 = ((v1.x_m - h0.x_m) * dir_y - (v1.y_m - h0.y_m) * dir_x).abs();

            if dist_v0 < 1e-4 && dist_v1 < 1e-4 {
                let t_v0 = ((v0.x_m - h0.x_m) * dir_x + (v0.y_m - h0.y_m) * dir_y) / len_h;
                let t_v1 = ((v1.x_m - h0.x_m) * dir_x + (v1.y_m - h0.y_m) * dir_y) / len_h;
                let min_t = t_v0.min(t_v1);
                let max_t = t_v0.max(t_v1);

                if max_t > 1e-4 && min_t < 1.0 - 1e-4 {
                    let c_start = min_t.clamp(0.0, 1.0);
                    let c_end = max_t.clamp(0.0, 1.0);
                    if c_end - c_start > 1e-4 {
                        covered_intervals.push((c_start, c_end));
                        split_params.push(c_start);
                        split_params.push(c_end);
                    }
                }
            }
        }

        // Check against already kept hidden edge segments (AABB pre-filter + collinear deduplication)
        for seg in &kept_hidden_segments {
            if !seg.overlaps_aabb(h_min_x, h_min_y, h_max_x, h_max_y) {
                continue;
            }
            let k0 = seg.p0;
            let k1 = seg.p1;
            let dist_k0 = ((k0.x_m - h0.x_m) * dir_y - (k0.y_m - h0.y_m) * dir_x).abs();
            let dist_k1 = ((k1.x_m - h0.x_m) * dir_y - (k1.y_m - h0.y_m) * dir_x).abs();

            if dist_k0 < 1e-4 && dist_k1 < 1e-4 {
                let t_k0 = ((k0.x_m - h0.x_m) * dir_x + (k0.y_m - h0.y_m) * dir_y) / len_h;
                let t_k1 = ((k1.x_m - h0.x_m) * dir_x + (k1.y_m - h0.y_m) * dir_y) / len_h;
                let min_t = t_k0.min(t_k1);
                let max_t = t_k0.max(t_k1);

                if max_t > 1e-4 && min_t < 1.0 - 1e-4 {
                    let c_start = min_t.clamp(0.0, 1.0);
                    let c_end = max_t.clamp(0.0, 1.0);
                    if c_end - c_start > 1e-4 {
                        covered_intervals.push((c_start, c_end));
                        split_params.push(c_start);
                        split_params.push(c_end);
                    }
                }
            }
        }

        // Check against cut polygons (AABB pre-filter)
        if suppress_inside_cut {
            for poly in &cut_polygons {
                if !poly.overlaps_aabb(h_min_x, h_min_y, h_max_x, h_max_y) {
                    continue;
                }
                collect_edge_splits(h0, h1, &poly.ring, &mut split_params);
                for hole in &poly.holes {
                    collect_edge_splits(h0, h1, hole, &mut split_params);
                }
            }
        }

        split_params.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        split_params.dedup_by(|a, b| (*a - *b).abs() < 1e-4);

        let mut kept_intervals = Vec::new();
        for window in split_params.windows(2) {
            let t_a = window[0];
            let t_b = window[1];
            if (t_b - t_a) * len_h < 1e-4 {
                continue;
            }

            let t_mid = (t_a + t_b) * 0.5;

            let is_covered = covered_intervals
                .iter()
                .any(|(c_start, c_end)| t_mid >= *c_start - 1e-4 && t_mid <= *c_end + 1e-4);
            if is_covered {
                continue;
            }

            if suppress_inside_cut {
                let p_mid = Point2::new(h0.x_m + dx * t_mid, h0.y_m + dy * t_mid);
                let inside_cut = cut_polygons.iter().any(|poly| {
                    if !poly.overlaps_aabb(p_mid.x_m, p_mid.y_m, p_mid.x_m, p_mid.y_m) {
                        return false;
                    }
                    (point_in_polygon(p_mid, &poly.ring) || point_on_polygon_boundary(p_mid, &poly.ring, 1e-4))
                        && !poly.holes.iter().any(|h| point_in_polygon(p_mid, h))
                });
                if inside_cut {
                    continue;
                }
            }

            kept_intervals.push((t_a, t_b));
        }

        for (t_a, t_b) in kept_intervals {
            let p_sub0 = Point2::new(h0.x_m + dx * t_a, h0.y_m + dy * t_a);
            let p_sub1 = Point2::new(h0.x_m + dx * t_b, h0.y_m + dy * t_b);
            if (p_sub1.x_m - p_sub0.x_m).hypot(p_sub1.y_m - p_sub0.y_m) < 1e-4 {
                continue;
            }

            next_id_idx += 1;
            let mut sub_prim = prim.clone();
            sub_prim.id = PrimitiveId::new(prim.id.view_id, 0, next_id_idx);
            sub_prim.geometry = Geometry2d::Line {
                start: p_sub0,
                end: p_sub1,
            };
            kept_hidden_segments.push(SegmentAabb::new(p_sub0, p_sub1));
            result_primitives.push(sub_prim);
        }
    }

    *primitives = result_primitives;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::views::provenance::{DepthInterval, ElementRef, FeatureKey, SourceReference, SourceSet};
    use crate::modules::aec::engine::views::{ElementId, ModelId, ModelViewId};

    fn make_test_source() -> SourceSet {
        let elem = ElementRef::new(ModelId::new(), ElementId::new());
        let src = SourceReference {
            element: elem,
            xref_source: None,
            component_id: None,
            feature: Some(FeatureKey::WallTop),
            display_component_id: None,
        };
        SourceSet::new(vec![src]).unwrap()
    }

    fn region_area(outer: &[Point2]) -> f64 {
        let twice_area: f64 = outer
            .iter()
            .zip(outer.iter().cycle().skip(1))
            .take(outer.len())
            .map(|(a, b)| a.x_m * b.y_m - b.x_m * a.y_m)
            .sum();
        twice_area.abs() * 0.5
    }

    #[test]
    fn partially_occluded_skewed_surface_is_clipped() {
        let model_view = ModelViewId::new();
        let occluder = DerivedPrimitive {
            id: PrimitiveId::new(model_view, 0, 1),
            role: PrimitiveRole::CutFill,
            geometry: Geometry2d::Region {
                outer: vec![
                    Point2::new(0.8, 0.2),
                    Point2::new(1.4, 0.2),
                    Point2::new(1.4, 0.8),
                    Point2::new(0.8, 0.8),
                ],
                holes: vec![],
            },
            sources: make_test_source(),
            depth: DepthInterval::new(0.0, 0.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        };
        let surface = DerivedPrimitive {
            id: PrimitiveId::new(model_view, 0, 2),
            role: PrimitiveRole::SurfaceFill,
            geometry: Geometry2d::Region {
                outer: vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(2.0, 0.0),
                    Point2::new(1.6, 1.0),
                    Point2::new(0.0, 1.0),
                ],
                holes: vec![],
            },
            sources: make_test_source(),
            depth: DepthInterval::new(1.0, 1.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        };
        let expected_area = region_area(match &surface.geometry {
            Geometry2d::Region { outer, .. } => outer,
            _ => unreachable!(),
        });
        let mut primitives = vec![occluder, surface];

        process_silhouette_and_depth(&mut primitives, &SilhouetteOptions::default()).unwrap();

        let visible_area: f64 = primitives
            .iter()
            .filter(|primitive| primitive.role == PrimitiveRole::SurfaceFill)
            .map(|primitive| match &primitive.geometry {
                Geometry2d::Region { outer, .. } => region_area(outer),
                _ => 0.0,
            })
            .sum();
        assert!(visible_area > 0.0);
        assert!(visible_area < expected_area - 1e-6);
    }

    #[test]
    fn silhouette_keeps_resolved_background_line_weight() {
        let mut primitive = DerivedPrimitive {
            id: PrimitiveId::new(ModelViewId::new(), 0, 1),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            },
            sources: make_test_source(),
            depth: DepthInterval::new(1.0, 1.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.35).unwrap()),
            edit_handles: vec![],
        };
        let expected_weight = primitive.style.line_weight_mm;
        let mut primitives = vec![primitive.clone()];

        process_silhouette_and_depth(&mut primitives, &SilhouetteOptions::default()).unwrap();

        primitive = primitives.pop().unwrap();
        assert_eq!(primitive.style.line_weight_mm, expected_weight);
    }

    #[test]
    fn test_collinear_background_edge_on_boundary_dropped() {
        let mv_id = ModelViewId::new();
        let wall_sources = make_test_source();
        let background_sources = make_test_source();

        // Foreground cut wall at depth 0.0 covering [0,0] to [4,4]
        let wall_cut = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 1),
            role: PrimitiveRole::CutFill,
            geometry: Geometry2d::Region {
                outer: vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(4.0, 0.0),
                    Point2::new(4.0, 4.0),
                    Point2::new(0.0, 4.0),
                ],
                holes: vec![],
            },
            sources: wall_sources,
            depth: DepthInterval::new(0.0, 0.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        };

        // Background edge at depth 1.0 along the bottom boundary of the wall [0,0] -> [4,0]
        let bg_collinear = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 2),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(4.0, 0.0),
            },
            sources: background_sources,
            depth: DepthInterval::new(1.0, 1.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        };

        let mut prims = vec![wall_cut, bg_collinear];
        process_silhouette_and_depth(
            &mut prims,
            &SilhouetteOptions {
                emit_hidden_lines: true,
                max_depth_distance: None,
                hidden_layer: Some("AEC_SECTION_HIDDEN".into()),
            },
        )
        .unwrap();

        // The collinear edge on boundary must be dropped: no HiddenEdge and no ProjectedEdge from it
        let hidden_edges: Vec<_> = prims.iter().filter(|p| p.role == PrimitiveRole::HiddenEdge).collect();
        assert_eq!(hidden_edges.len(), 0, "Collinear boundary edge must not be emitted dashed");
        let proj_edges: Vec<_> = prims.iter().filter(|p| p.role == PrimitiveRole::ProjectedEdge).collect();
        assert_eq!(proj_edges.len(), 0, "Collinear boundary edge must not be emitted solid");
    }

    #[test]
    fn test_partially_occluded_segment_visible_and_hidden() {
        let mv_id = ModelViewId::new();
        let wall_sources = make_test_source();
        let background_sources = make_test_source();

        // Foreground cut wall covering [2,0] to [6,4] at depth 0.0
        let wall_cut = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 1),
            role: PrimitiveRole::CutFill,
            geometry: Geometry2d::Region {
                outer: vec![
                    Point2::new(2.0, 0.0),
                    Point2::new(6.0, 0.0),
                    Point2::new(6.0, 4.0),
                    Point2::new(2.0, 4.0),
                ],
                holes: vec![],
            },
            sources: wall_sources,
            depth: DepthInterval::new(0.0, 0.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        };

        // Background edge from [0,2] to [4,2] at depth 1.0 (partially occluded: 0..2 visible, 2..4 hidden)
        let bg_line = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 2),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Line {
                start: Point2::new(0.0, 2.0),
                end: Point2::new(4.0, 2.0),
            },
            sources: background_sources,
            depth: DepthInterval::new(1.0, 1.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        };

        let mut prims = vec![wall_cut, bg_line];
        process_silhouette_and_depth(
            &mut prims,
            &SilhouetteOptions {
                emit_hidden_lines: true,
                max_depth_distance: None,
                hidden_layer: Some("AEC_SECTION_HIDDEN".into()),
            },
        )
        .unwrap();

        let vis: Vec<_> = prims.iter().filter(|p| p.role == PrimitiveRole::ProjectedEdge).collect();
        assert_eq!(vis.len(), 1, "Expected 1 visible piece");
        if let Geometry2d::Line { start, end } = vis[0].geometry {
            assert!((start.x_m - 0.0).abs() < 1e-4);
            assert!((end.x_m - 2.0).abs() < 1e-4);
        } else {
            panic!("Expected Line geometry");
        }

        let hid: Vec<_> = prims.iter().filter(|p| p.role == PrimitiveRole::HiddenEdge).collect();
        assert_eq!(hid.len(), 1, "Expected 1 hidden piece");
        assert_eq!(hid[0].style.layer_name.as_deref(), Some("AEC_SECTION_HIDDEN"));
        assert_eq!(hid[0].style.line_type.as_deref(), Some("DASHED"));
        if let Geometry2d::Line { start, end } = hid[0].geometry {
            assert!((start.x_m - 2.0).abs() < 1e-4);
            assert!((end.x_m - 4.0).abs() < 1e-4);
            assert!((end.x_m - start.x_m).hypot(end.y_m - start.y_m) >= 1e-4);
        } else {
            panic!("Expected Line geometry");
        }
    }

    #[test]
    fn test_opening_occluder_preserves_own_contour_and_hides_background() {
        let mv_id = ModelViewId::new();
        let opening_element = ElementRef::new(ModelId::new(), ElementId::new());
        let opening_sources = SourceSet::new(vec![SourceReference {
            element: opening_element,
            xref_source: None,
            component_id: None,
            feature: Some(FeatureKey::OpeningSill),
            display_component_id: None,
        }])
        .unwrap();
        let wall_sources = make_test_source();

        let opening_fill = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 1),
            role: PrimitiveRole::SurfaceFill,
            geometry: Geometry2d::Region {
                outer: vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(2.0, 0.0),
                    Point2::new(2.0, 2.0),
                    Point2::new(0.0, 2.0),
                ],
                holes: vec![],
            },
            sources: opening_sources.clone(),
            depth: DepthInterval::new(0.0, 0.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        };
        let opening_contour = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 2),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Line {
                start: Point2::new(0.5, 1.0),
                end: Point2::new(1.5, 1.0),
            },
            sources: opening_sources,
            depth: DepthInterval::new(0.001, 0.001).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        };
        let background_edge = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 3),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Line {
                start: Point2::new(0.5, 1.5),
                end: Point2::new(1.5, 1.5),
            },
            sources: wall_sources,
            depth: DepthInterval::new(0.001, 0.001).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        };

        let mut prims = vec![opening_fill, opening_contour, background_edge];
        process_silhouette_and_depth(&mut prims, &SilhouetteOptions::default()).unwrap();

        let projected_edges: Vec<_> = prims
            .iter()
            .filter(|prim| prim.role == PrimitiveRole::ProjectedEdge)
            .collect();
        assert_eq!(projected_edges.len(), 1, "Only the opening's own contour should remain");
        assert_eq!(
            projected_edges[0].sources.primary_element(),
            Some(opening_element),
            "The opening contour must not be clipped by its own opaque fill"
        );
    }

    #[test]
    fn test_hidden_pieces_suppressed_when_option_off() {
        let mv_id = ModelViewId::new();
        let sources = make_test_source();

        let wall_cut = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 1),
            role: PrimitiveRole::CutFill,
            geometry: Geometry2d::Region {
                outer: vec![
                    Point2::new(2.0, 0.0),
                    Point2::new(6.0, 0.0),
                    Point2::new(6.0, 4.0),
                    Point2::new(2.0, 4.0),
                ],
                holes: vec![],
            },
            sources: sources.clone(),
            depth: DepthInterval::new(0.0, 0.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        };

        let bg_line = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 2),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Line {
                start: Point2::new(0.0, 2.0),
                end: Point2::new(4.0, 2.0),
            },
            sources,
            depth: DepthInterval::new(1.0, 1.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        };

        let mut prims = vec![wall_cut, bg_line];
        process_silhouette_and_depth(
            &mut prims,
            &SilhouetteOptions {
                emit_hidden_lines: false,
                max_depth_distance: None,
                hidden_layer: Some("AEC_SECTION_HIDDEN".into()),
            },
        )
        .unwrap();

        let hid: Vec<_> = prims.iter().filter(|p| p.role == PrimitiveRole::HiddenEdge).collect();
        assert_eq!(hid.len(), 0, "No hidden pieces when emit_hidden_lines is false");

        let vis: Vec<_> = prims.iter().filter(|p| p.role == PrimitiveRole::ProjectedEdge).collect();
        assert_eq!(vis.len(), 1, "Visible piece still emitted");
    }

    #[test]
    fn test_inter_primitive_collinear_segment_merging() {
        let mv_id = ModelViewId::new();
        let sources = make_test_source();

        let line1 = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 1),
            role: PrimitiveRole::HiddenEdge,
            geometry: Geometry2d::Line {
                start: Point2::new(0.0, 1.0),
                end: Point2::new(3.0, 1.0),
            },
            sources: sources.clone(),
            depth: DepthInterval::new(1.0, 1.0).unwrap(),
            style: ResolvedStyle {
                line_weight_mm: PaperMm::new(0.25).unwrap(),
                line_color: None,
                line_type: Some("DASHED".into()),
                fill_color: None,
                hatch_pattern: None,
                hatch_scale: None,
                hatch_angle_rad: None,
                layer_name: Some("AEC_OVERHEAD".into()),
            },
            edit_handles: vec![],
        };

        let line2 = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 2),
            role: PrimitiveRole::HiddenEdge,
            geometry: Geometry2d::Line {
                start: Point2::new(2.5, 1.0),
                end: Point2::new(6.0, 1.0),
            },
            sources,
            depth: DepthInterval::new(1.0, 1.0).unwrap(),
            style: ResolvedStyle {
                line_weight_mm: PaperMm::new(0.25).unwrap(),
                line_color: None,
                line_type: Some("DASHED".into()),
                fill_color: None,
                hatch_pattern: None,
                hatch_scale: None,
                hatch_angle_rad: None,
                layer_name: Some("AEC_OVERHEAD".into()),
            },
            edit_handles: vec![],
        };

        let mut prims = vec![line1, line2];
        merge_and_deduplicate_segments(&mut prims);
        assert_eq!(prims.len(), 1, "Expected overlapping collinear segments to merge into 1");
        if let Geometry2d::Line { start, end } = prims[0].geometry {
            assert!((start.x_m - 0.0).abs() < 1e-4);
            assert!((end.x_m - 6.0).abs() < 1e-4);
        } else {
            panic!("Expected merged line");
        }
    }

    #[test]
    fn test_merge_rejects_differing_depth_style_or_sources() {
        let mv_id = ModelViewId::new();
        let sources1 = make_test_source();
        let elem2 = ElementRef::new(ModelId::new(), ElementId::new());
        let sources2 = SourceSet::new(vec![SourceReference {
            element: elem2,
            xref_source: None,
            component_id: None,
            feature: Some(FeatureKey::WallTop),
            display_component_id: None,
        }]).unwrap();

        // 1. Differing depth: depth 0.0 vs depth 5.0
        let p_depth0 = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 1),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Line { start: Point2::new(0.0, 0.0), end: Point2::new(2.0, 0.0) },
            sources: sources1.clone(),
            depth: DepthInterval::new(0.0, 0.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.35).unwrap()),
            edit_handles: vec![],
        };
        let p_depth5 = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 2),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Line { start: Point2::new(2.0, 0.0), end: Point2::new(4.0, 0.0) },
            sources: sources1.clone(),
            depth: DepthInterval::new(5.0, 5.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.18).unwrap()),
            edit_handles: vec![],
        };
        let mut list_depth = vec![p_depth0, p_depth5];
        merge_and_deduplicate_segments(&mut list_depth);
        assert_eq!(list_depth.len(), 2, "Segments with different depths and weights must NOT merge");

        // 2. Differing sources: element1 vs element2
        let p_src1 = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 3),
            role: PrimitiveRole::CutBoundary,
            geometry: Geometry2d::Line { start: Point2::new(0.0, 0.0), end: Point2::new(2.0, 0.0) },
            sources: sources1,
            depth: DepthInterval::cut_plane(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.35).unwrap()),
            edit_handles: vec![],
        };
        let p_src2 = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 4),
            role: PrimitiveRole::CutBoundary,
            geometry: Geometry2d::Line { start: Point2::new(2.0, 0.0), end: Point2::new(4.0, 0.0) },
            sources: sources2,
            depth: DepthInterval::cut_plane(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.35).unwrap()),
            edit_handles: vec![],
        };
        let mut list_src = vec![p_src1, p_src2];
        merge_and_deduplicate_segments(&mut list_src);
        assert_eq!(list_src.len(), 2, "Segments with different element sources must NOT merge");
    }

    #[test]
    fn test_suppress_hidden_edges_interval_trimming() {
        let mv_id = ModelViewId::new();
        let sources = make_test_source();

        // Visible segment 1: [0, 3] along y = 0
        let vis1 = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 1),
            role: PrimitiveRole::CutBoundary,
            geometry: Geometry2d::Line { start: Point2::new(0.0, 0.0), end: Point2::new(3.0, 0.0) },
            sources: sources.clone(),
            depth: DepthInterval::cut_plane(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.35).unwrap()),
            edit_handles: vec![],
        };

        // Visible segment 2: [3, 7] along y = 0
        let vis2 = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 2),
            role: PrimitiveRole::CutBoundary,
            geometry: Geometry2d::Line { start: Point2::new(3.0, 0.0), end: Point2::new(7.0, 0.0) },
            sources: sources.clone(),
            depth: DepthInterval::cut_plane(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.35).unwrap()),
            edit_handles: vec![],
        };

        // Cut wall polygon from x = 8 to 9, y = -1 to 1
        let cut_poly = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 3),
            role: PrimitiveRole::CutFill,
            geometry: Geometry2d::Region {
                outer: vec![
                    Point2::new(8.0, -1.0),
                    Point2::new(9.0, -1.0),
                    Point2::new(9.0, 1.0),
                    Point2::new(8.0, 1.0),
                ],
                holes: vec![],
            },
            sources: sources.clone(),
            depth: DepthInterval::cut_plane(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        };

        // Overhead hidden line: [0, 10] along y = 0
        // Expected trimming:
        // [0, 3] suppressed by vis1
        // [3, 7] suppressed by vis2
        // [7, 8] KEPT!
        // [8, 9] suppressed by cut wall polygon!
        // [9, 10] KEPT!
        let hid = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 4),
            role: PrimitiveRole::HiddenEdge,
            geometry: Geometry2d::Line { start: Point2::new(0.0, 0.0), end: Point2::new(10.0, 0.0) },
            sources,
            depth: DepthInterval::new(1.0, 1.0).unwrap(),
            style: ResolvedStyle {
                line_weight_mm: PaperMm::new(0.25).unwrap(),
                line_color: None,
                line_type: Some("DASHED".into()),
                fill_color: None,
                hatch_pattern: None,
                hatch_scale: None,
                hatch_angle_rad: None,
                layer_name: Some("AEC_OVERHEAD".into()),
            },
            edit_handles: vec![],
        };

        let mut prims = vec![vis1, vis2, cut_poly, hid];
        suppress_coincident_hidden_edges(&mut prims);

        let hid_results: Vec<_> = prims.iter().filter(|p| p.role == PrimitiveRole::HiddenEdge).collect();
        assert_eq!(hid_results.len(), 2, "Expected exactly 2 trimmed hidden intervals: [7, 8] and [9, 10]");

        if let Geometry2d::Line { start, end } = hid_results[0].geometry {
            assert!((start.x_m - 7.0).abs() < 1e-4);
            assert!((end.x_m - 8.0).abs() < 1e-4);
        } else {
            panic!("Expected line geometry");
        }

        if let Geometry2d::Line { start, end } = hid_results[1].geometry {
            assert!((start.x_m - 9.0).abs() < 1e-4);
            assert!((end.x_m - 10.0).abs() < 1e-4);
        } else {
            panic!("Expected line geometry");
        }
    }

    #[test]
    fn test_overhead_polyline_exploded_and_trimmed() {
        let mv_id = ModelViewId::new();
        let sources = make_test_source();

        // Cut wall covering [0, 0] to [2, 4]
        let cut_poly = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 1),
            role: PrimitiveRole::CutFill,
            geometry: Geometry2d::Region {
                outer: vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(2.0, 0.0),
                    Point2::new(2.0, 4.0),
                    Point2::new(0.0, 4.0),
                ],
                holes: vec![],
            },
            sources: sources.clone(),
            depth: DepthInterval::cut_plane(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        };

        // Overhead slab polyline: [0, 0] -> [4, 0] -> [4, 4] -> [0, 4] (closed)
        // Edges:
        // 1. [0,0]->[4,0]: [0,2] coincides with cut wall boundary -> suppressed; [2,4] outside -> kept!
        // 2. [4,0]->[4,4]: outside cut wall -> kept!
        // 3. [4,4]->[0,4]: [4,4]->[2,4] kept; [2,4]->[0,4] coincides with cut wall boundary -> suppressed!
        // 4. [0,4]->[0,0]: coincides with cut wall boundary -> suppressed!
        let overhead_slab = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 2),
            role: PrimitiveRole::HiddenEdge,
            geometry: Geometry2d::Polyline {
                vertices: vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(4.0, 0.0),
                    Point2::new(4.0, 4.0),
                    Point2::new(0.0, 4.0),
                ],
                closed: true,
            },
            sources,
            depth: DepthInterval::new(1.0, 1.0).unwrap(),
            style: ResolvedStyle {
                line_weight_mm: PaperMm::new(0.25).unwrap(),
                line_color: None,
                line_type: Some("DASHED".into()),
                fill_color: None,
                hatch_pattern: None,
                hatch_scale: None,
                hatch_angle_rad: None,
                layer_name: Some("AEC_OVERHEAD".into()),
            },
            edit_handles: vec![],
        };

        let mut prims = vec![cut_poly, overhead_slab];
        suppress_coincident_hidden_edges(&mut prims);

        let hid_results: Vec<_> = prims.iter().filter(|p| p.role == PrimitiveRole::HiddenEdge).collect();
        // The kept edges must be: [2,0]->[4,0], [4,0]->[4,4], [4,4]->[2,4] (3 segments)
        assert_eq!(hid_results.len(), 3, "Overhead polyline must be exploded and trimmed where coinciding with cut wall");
    }

    #[test]
    fn test_opening_overhead_exempt_from_cut_polygon_suppression() {
        let mv_id = ModelViewId::new();
        let elem = ElementRef::new(ModelId::new(), ElementId::new());
        let head_source = SourceSet::new(vec![SourceReference {
            element: elem,
            xref_source: None,
            component_id: None,
            feature: Some(FeatureKey::OpeningHead),
            display_component_id: None,
        }]).unwrap();

        // Cut wall covering [0, 0] to [4, 4]
        let cut_poly = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 1),
            role: PrimitiveRole::CutFill,
            geometry: Geometry2d::Region {
                outer: vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(4.0, 0.0),
                    Point2::new(4.0, 4.0),
                    Point2::new(0.0, 4.0),
                ],
                holes: vec![],
            },
            sources: head_source.clone(),
            depth: DepthInterval::cut_plane(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        };

        // Lintel head line crossing inside the cut wall from [1, 2] to [3, 2]
        let lintel_line = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 2),
            role: PrimitiveRole::HiddenEdge,
            geometry: Geometry2d::Line {
                start: Point2::new(1.0, 2.0),
                end: Point2::new(3.0, 2.0),
            },
            sources: head_source,
            depth: DepthInterval::new(1.0, 1.0).unwrap(),
            style: ResolvedStyle {
                line_weight_mm: PaperMm::new(0.25).unwrap(),
                line_color: None,
                line_type: Some("DASHED".into()),
                fill_color: None,
                hatch_pattern: None,
                hatch_scale: None,
                hatch_angle_rad: None,
                layer_name: Some("AEC_OVERHEAD".into()),
            },
            edit_handles: vec![],
        };

        let mut prims = vec![cut_poly, lintel_line];
        suppress_coincident_hidden_edges(&mut prims);

        let hid_results: Vec<_> = prims.iter().filter(|p| p.role == PrimitiveRole::HiddenEdge).collect();
        assert_eq!(hid_results.len(), 1, "OpeningHead must NOT be suppressed by cut fill polygon");
    }

    #[test]
    fn test_suppress_hidden_edges_overlapping_distinct_sources() {
        let mv_id = ModelViewId::new();
        let elem_a = ElementRef::new(ModelId::new(), ElementId::new());
        let source_a = SourceSet::new(vec![SourceReference {
            element: elem_a,
            xref_source: None,
            component_id: None,
            feature: Some(FeatureKey::SlabBottom),
            display_component_id: None,
        }]).unwrap();

        let elem_b = ElementRef::new(ModelId::new(), ElementId::new());
        let source_b = SourceSet::new(vec![SourceReference {
            element: elem_b,
            xref_source: None,
            component_id: None,
            feature: Some(FeatureKey::OpeningHead),
            display_component_id: None,
        }]).unwrap();

        let style = ResolvedStyle {
            line_weight_mm: PaperMm::new(0.25).unwrap(),
            line_color: None,
            line_type: Some("DASHED".into()),
            fill_color: None,
            hatch_pattern: None,
            hatch_scale: None,
            hatch_angle_rad: None,
            layer_name: Some("AEC_OVERHEAD".into()),
        };

        // First hidden edge (Source A) along y=0 from x=0.0 to x=5.0
        let edge_a = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 1),
            role: PrimitiveRole::HiddenEdge,
            geometry: Geometry2d::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(5.0, 0.0),
            },
            sources: source_a.clone(),
            depth: DepthInterval::new(1.0, 1.0).unwrap(),
            style: style.clone(),
            edit_handles: vec![],
        };

        // Second hidden edge (Source B) along y=0 from x=3.0 to x=8.0 (overlaps x=3.0..5.0)
        let edge_b = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 2),
            role: PrimitiveRole::HiddenEdge,
            geometry: Geometry2d::Line {
                start: Point2::new(3.0, 0.0),
                end: Point2::new(8.0, 0.0),
            },
            sources: source_b.clone(),
            depth: DepthInterval::new(1.0, 1.0).unwrap(),
            style,
            edit_handles: vec![],
        };

        let mut prims = vec![edge_a, edge_b];
        suppress_coincident_hidden_edges(&mut prims);

        let hid_results: Vec<_> = prims.iter().filter(|p| p.role == PrimitiveRole::HiddenEdge).collect();
        assert_eq!(hid_results.len(), 2, "Overlapping hidden edges must be trimmed to non-overlapping pieces");

        // Edge A keeps [0, 5]
        assert_eq!(hid_results[0].sources, source_a);
        match &hid_results[0].geometry {
            Geometry2d::Line { start, end } => {
                assert!((start.x_m - 0.0).abs() < 1e-4);
                assert!((end.x_m - 5.0).abs() < 1e-4);
            }
            _ => panic!("Expected Line geometry"),
        }

        // Edge B is trimmed to [5, 8] with source B preserved
        assert_eq!(hid_results[1].sources, source_b);
        match &hid_results[1].geometry {
            Geometry2d::Line { start, end } => {
                assert!((start.x_m - 5.0).abs() < 1e-4);
                assert!((end.x_m - 8.0).abs() < 1e-4);
            }
            _ => panic!("Expected Line geometry"),
        }
    }

    fn fill(id: u32, role: PrimitiveRole, outer: Vec<Point2>, sources: SourceSet, depth: f64) -> DerivedPrimitive {
        DerivedPrimitive {
            id: PrimitiveId::new(ModelViewId::new(), 0, id),
            role,
            geometry: Geometry2d::Region { outer, holes: vec![] },
            sources,
            depth: DepthInterval::new(depth, depth).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        }
    }

    #[test]
    fn nearer_wall_hides_opening_frame_swing_and_glass() {
        let mv_id = ModelViewId::new();
        let wall = make_test_source();
        let opening = SourceSet::new(vec![SourceReference {
            element: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            component_id: None,
            feature: Some(FeatureKey::OpeningSill),
            display_component_id: None,
        }])
        .unwrap();
        let wall_fill = fill(
            1,
            PrimitiveRole::SurfaceFill,
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(4.0, 0.0),
                Point2::new(4.0, 3.0),
                Point2::new(0.0, 3.0),
            ],
            wall.clone(),
            1.0,
        );
        let glass = fill(
            2,
            PrimitiveRole::SurfaceFill,
            vec![
                Point2::new(1.0, 1.0),
                Point2::new(2.0, 1.0),
                Point2::new(2.0, 2.0),
                Point2::new(1.0, 2.0),
            ],
            opening.clone(),
            2.0,
        );
        let frame = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 3),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Region {
                outer: vec![
                    Point2::new(1.0, 1.0),
                    Point2::new(2.0, 1.0),
                    Point2::new(2.0, 2.0),
                    Point2::new(1.0, 2.0),
                ],
                holes: vec![],
            },
            sources: opening.clone(),
            depth: DepthInterval::new(2.0, 2.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        };
        let swing = DerivedPrimitive {
            id: PrimitiveId::new(mv_id, 0, 4),
            role: PrimitiveRole::Symbol,
            geometry: Geometry2d::Polyline {
                vertices: vec![
                    Point2::new(1.1, 1.1),
                    Point2::new(1.5, 1.8),
                    Point2::new(1.1, 1.9),
                ],
                closed: false,
            },
            sources: opening,
            depth: DepthInterval::new(2.0, 2.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        };

        let mut prims = vec![wall_fill, glass, frame, swing];
        process_silhouette_and_depth(&mut prims, &SilhouetteOptions::default()).unwrap();
        assert!(
            prims.iter().all(|prim| prim.sources.primary_element() == wall.primary_element()),
            "opening glass, frame and swing must not survive a nearer wall: {prims:?}"
        );
    }

    #[test]
    fn cut_plane_glass_hides_the_same_opening_symbol() {
        let opening = SourceSet::new(vec![SourceReference {
            element: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            component_id: None,
            feature: Some(FeatureKey::OpeningSill),
            display_component_id: None,
        }])
        .unwrap();
        let sheet = fill(
            1,
            PrimitiveRole::SurfaceFill,
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(2.0, 0.0),
                Point2::new(2.0, 2.0),
                Point2::new(0.0, 2.0),
            ],
            opening.clone(),
            1.0e-4,
        );
        let symbol = DerivedPrimitive {
            id: PrimitiveId::new(ModelViewId::new(), 0, 2),
            role: PrimitiveRole::Symbol,
            geometry: Geometry2d::Line {
                start: Point2::new(0.2, 0.2),
                end: Point2::new(1.8, 1.8),
            },
            sources: opening,
            depth: DepthInterval::new(0.4, 0.4).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        };
        let mut prims = vec![sheet, symbol];
        process_silhouette_and_depth(&mut prims, &SilhouetteOptions::default()).unwrap();
        assert!(
            prims.iter().all(|prim| prim.role != PrimitiveRole::Symbol),
            "a cut-plane glass sheet must hide the elevation symbol of the same opening"
        );
    }
}
