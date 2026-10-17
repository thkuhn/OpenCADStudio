//! Occlusion analysis, depth classification, and background edge projection (Silhouette).
//!
//! Evaluates visible component edges behind the cut plane or within the elevation depth range,
//! classifying primitives into foreground visible edges, background projected edges, and hidden lines,
//! and clipping occluded background lines against opaque foreground polygons.

use super::provenance::{DerivedPrimitive, Geometry2d, PrimitiveId, PrimitiveRole, ResolvedStyle, Rgba8};
use super::{PaperMm, Point2, ValidationError};

const EPS: f64 = 1e-6;

/// Silhouette and occlusion options.
#[derive(Clone, Debug, PartialEq)]
pub struct SilhouetteOptions {
    pub emit_hidden_lines: bool,
    pub max_depth_distance: Option<f64>,
}

impl Default for SilhouetteOptions {
    fn default() -> Self {
        Self {
            emit_hidden_lines: false,
            max_depth_distance: None,
        }
    }
}

struct OpaquePolygon {
    depth_max: f64,
    ring: Vec<Point2>,
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

    // 2. Adjust line weights according to depth tiers
    for prim in primitives.iter_mut() {
        if prim.depth.min_m > 1e-4 && prim.role == PrimitiveRole::ProjectedEdge {
            prim.style.line_weight_mm = PaperMm::new(0.18)?;
        }
    }

    // 3. Perform 2D Occlusion Clipping
    let mut opaque_polygons: Vec<OpaquePolygon> = Vec::new();

    for prim in primitives.iter() {
        if prim.role == PrimitiveRole::CutFill || prim.role == PrimitiveRole::SurfaceFill {
            match &prim.geometry {
                Geometry2d::Region { outer, .. } if outer.len() >= 3 => {
                    opaque_polygons.push(OpaquePolygon {
                        depth_max: prim.depth.max_m,
                        ring: outer.clone(),
                    });
                }
                Geometry2d::Polyline { vertices, closed: true } if vertices.len() >= 3 => {
                    opaque_polygons.push(OpaquePolygon {
                        depth_max: prim.depth.max_m,
                        ring: vertices.clone(),
                    });
                }
                _ => {}
            }
        }
    }

    if opaque_polygons.is_empty() {
        return Ok(());
    }

    let mut result_primitives = Vec::with_capacity(primitives.len());
    let mut next_id_index = primitives.len() as u32 + 1000;

    for prim in primitives.drain(..) {
        // Only clip projected edges and background lines
        if prim.role != PrimitiveRole::ProjectedEdge {
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
                    &opaque_polygons,
                    options.emit_hidden_lines,
                );

                for (p0, p1, is_hidden) in segments {
                    if (p1.x_m - p0.x_m).hypot(p1.y_m - p0.y_m) < EPS {
                        continue;
                    }
                    if is_hidden {
                        if options.emit_hidden_lines {
                            let mut hidden_prim = prim.clone();
                            next_id_index += 1;
                            hidden_prim.id = PrimitiveId::new(prim.id.view_id, 0, next_id_index);
                            hidden_prim.role = PrimitiveRole::HiddenEdge;
                            hidden_prim.geometry = Geometry2d::Line { start: p0, end: p1 };
                            hidden_prim.style = ResolvedStyle {
                                line_weight_mm: PaperMm::new(0.13)?,
                                line_color: Some(Rgba8::new(150, 150, 150, 255)),
                                line_type: Some("DASHED".to_string()),
                                fill_color: None,
                                hatch_pattern: None,
                                hatch_scale: None,
                                hatch_angle_rad: None,
                                layer_name: Some("AEC_HIDDEN".to_string()),
                            };
                            result_primitives.push(hidden_prim);
                        }
                    } else {
                        let mut vis_prim = prim.clone();
                        next_id_index += 1;
                        vis_prim.id = PrimitiveId::new(prim.id.view_id, 0, next_id_index);
                        vis_prim.geometry = Geometry2d::Line { start: p0, end: p1 };
                        result_primitives.push(vis_prim);
                    }
                }
            }
            Geometry2d::Polyline { ref vertices, closed } => {
                let count = vertices.len();
                if count < 2 {
                    continue;
                }
                let mut clipped_segments = Vec::new();
                let num_edges = if closed { count } else { count - 1 };

                for i in 0..num_edges {
                    let p0 = vertices[i];
                    let p1 = vertices[(i + 1) % count];
                    let segs = clip_segment_against_polygons(
                        p0,
                        p1,
                        prim_depth,
                        &opaque_polygons,
                        options.emit_hidden_lines,
                    );
                    clipped_segments.extend(segs);
                }

                for (p0, p1, is_hidden) in clipped_segments {
                    if (p1.x_m - p0.x_m).hypot(p1.y_m - p0.y_m) < EPS {
                        continue;
                    }
                    if is_hidden {
                        if options.emit_hidden_lines {
                            let mut hidden_prim = prim.clone();
                            next_id_index += 1;
                            hidden_prim.id = PrimitiveId::new(prim.id.view_id, 0, next_id_index);
                            hidden_prim.role = PrimitiveRole::HiddenEdge;
                            hidden_prim.geometry = Geometry2d::Line { start: p0, end: p1 };
                            hidden_prim.style = ResolvedStyle {
                                line_weight_mm: PaperMm::new(0.13)?,
                                line_color: Some(Rgba8::new(150, 150, 150, 255)),
                                line_type: Some("DASHED".to_string()),
                                fill_color: None,
                                hatch_pattern: None,
                                hatch_scale: None,
                                hatch_angle_rad: None,
                                layer_name: Some("AEC_HIDDEN".to_string()),
                            };
                            result_primitives.push(hidden_prim);
                        }
                    } else {
                        let mut vis_prim = prim.clone();
                        next_id_index += 1;
                        vis_prim.id = PrimitiveId::new(prim.id.view_id, 0, next_id_index);
                        vis_prim.geometry = Geometry2d::Line { start: p0, end: p1 };
                        result_primitives.push(vis_prim);
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

    *primitives = result_primitives;
    Ok(())
}

fn clip_segment_against_polygons(
    start: Point2,
    end: Point2,
    seg_depth: f64,
    polygons: &[OpaquePolygon],
    _emit_hidden: bool,
) -> Vec<(Point2, Point2, bool)> {
    let mut current_segments = vec![(start, end, false)];

    for poly in polygons {
        if poly.depth_max >= seg_depth - 1e-4 {
            continue; // Polygon is behind or at the same depth as the segment
        }

        let mut next_segments = Vec::new();
        for (p0, p1, is_hidden) in current_segments {
            if is_hidden {
                next_segments.push((p0, p1, true));
                continue;
            }

            let split_segs = clip_subsegment_against_poly(p0, p1, &poly.ring);
            next_segments.extend(split_segs);
        }
        current_segments = next_segments;
    }

    current_segments
}

fn clip_subsegment_against_poly(
    p0: Point2,
    p1: Point2,
    ring: &[Point2],
) -> Vec<(Point2, Point2, bool)> {
    let dx = p1.x_m - p0.x_m;
    let dy = p1.y_m - p0.y_m;
    let len = dx.hypot(dy);
    if len < EPS {
        return vec![(p0, p1, point_in_polygon(p0, ring))];
    }

    let mut t_params = vec![0.0, 1.0];
    let n = ring.len();

    for i in 0..n {
        let e0 = ring[i];
        let e1 = ring[(i + 1) % n];

        if let Some(t) = line_segment_intersection(p0, p1, e0, e1) {
            if t > EPS && t < 1.0 - EPS {
                t_params.push(t);
            }
        }
    }

    t_params.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    t_params.dedup_by(|a, b| (*a - *b).abs() < EPS);

    let mut results = Vec::new();
    for window in t_params.windows(2) {
        let t_a = window[0];
        let t_b = window[1];
        if t_b - t_a < EPS {
            continue;
        }

        let sub_p0 = Point2::new(p0.x_m + dx * t_a, p0.y_m + dy * t_a);
        let sub_p1 = Point2::new(p0.x_m + dx * t_b, p0.y_m + dy * t_b);

        let mid = Point2::new(
            p0.x_m + dx * (t_a + t_b) * 0.5,
            p0.y_m + dy * (t_a + t_b) * 0.5,
        );

        let occluded = point_in_polygon(mid, ring);
        results.push((sub_p0, sub_p1, occluded));
    }

    results
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

fn point_in_polygon(pt: Point2, ring: &[Point2]) -> bool {
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
