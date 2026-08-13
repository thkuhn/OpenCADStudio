//! 2D layer-contour generation for walls.
//!
//! This module provides functions to compute parallel offset polylines for
//! multi-layer walls. It uses a simple per-vertex angle-bisector offset
//! for miters, which is suitable for standard architectural wall layouts.
//!
//! For an N-layer wall, this implementation returns N+1 boundary lines
//! (parallel offsets of the centerline). This choice (a) was preferred for
//! simplicity over returning closed loop-per-layer quads.

/// Computes parallel offset polylines (boundary lines) for an open polyline
/// centerline and a list of layer thicknesses.
///
/// For a list of N layer thicknesses, it returns N+1 boundary lines.
/// The centerline is assumed to be the middle of the total thickness.
///
/// Corner handling uses a simple angle-bisector miter join. This may produce
/// minor imperfections at very sharp/acute corners, but matches the plan's
/// stated simplification for architectural walls.
pub fn layer_contours(
    centerline: &[(f64, f64)],
    layers: &[f64],
) -> Vec<Vec<(f64, f64)>> {
    if centerline.len() < 2 {
        return Vec::new();
    }

    let total_thickness: f64 = layers.iter().sum();
    let mut boundary_offsets = Vec::with_capacity(layers.len() + 1);
    let mut current_offset = -total_thickness * 0.5;
    boundary_offsets.push(current_offset);
    for &t in layers {
        current_offset += t;
        boundary_offsets.push(current_offset);
    }

    let directions = get_offset_directions(centerline);
    
    boundary_offsets.into_iter().map(|d| {
        centerline.iter().zip(directions.iter())
            .map(|(&(x, y), &(dx, dy))| (x + dx * d, y + dy * d))
            .collect()
    }).collect()
}

/// Pre-calculates the offset direction and miter scale for each vertex.
fn get_offset_directions(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let n = points.len();
    let mut directions = Vec::with_capacity(n);

    for i in 0..n {
        let dir = if i == 0 {
            // Start point: perpendicular to first segment.
            let (x0, y0) = points[0];
            let (x1, y1) = points[1];
            normal(x0, y0, x1, y1)
        } else if i == n - 1 {
            // End point: perpendicular to last segment.
            let (x0, y0) = points[n - 2];
            let (x1, y1) = points[n - 1];
            normal(x0, y0, x1, y1)
        } else {
            // Interior point: angle bisector of normals.
            let (x0, y0) = points[i - 1];
            let (x1, y1) = points[i];
            let (x2, y2) = points[i + 1];

            let n1 = normal(x0, y0, x1, y1);
            let n2 = normal(x1, y1, x2, y2);

            let bx = n1.0 + n2.0;
            let by = n1.1 + n2.1;
            let b_len = (bx * bx + by * by).sqrt();
            
            if b_len < 1e-9 {
                // Parallel or anti-parallel.
                n1
            } else {
                let bx = bx / b_len;
                let by = by / b_len;
                // miter_scale = 1 / cos(half_angle) = 1 / (n1 dot bisector)
                let dot = n1.0 * bx + n1.1 * by;
                let scale = 1.0 / dot;
                (bx * scale, by * scale)
            }
        };
        directions.push(dir);
    }

    directions
}

/// Returns the unit normal (-dy, dx) of the segment from (x0, y0) to (x1, y1).
fn normal(x0: f64, y0: f64, x1: f64, y1: f64) -> (f64, f64) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-9 {
        (0.0, 0.0)
    } else {
        (-dy / len, dx / len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_wall_two_layers() {
        // Horizontal wall along X axis from 0 to 10.
        // Layers: 0.1 and 0.2. Total = 0.3.
        // Centerline at Y=0.
        // Boundaries at Y = -0.15, -0.05, 0.15.
        let centerline = vec![(0.0, 0.0), (10.0, 0.0)];
        let layers = vec![0.1, 0.2];
        let contours = layer_contours(&centerline, &layers);

        assert_eq!(contours.len(), 3);
        
        // Boundary 0: Y = -0.15
        assert!((contours[0][0].0 - 0.0).abs() < 1e-9);
        assert!((contours[0][0].1 - (-0.15)).abs() < 1e-9);
        assert!((contours[0][1].0 - 10.0).abs() < 1e-9);
        assert!((contours[0][1].1 - (-0.15)).abs() < 1e-9);

        // Boundary 1: Y = -0.15 + 0.1 = -0.05
        assert!((contours[1][0].0 - 0.0).abs() < 1e-9);
        assert!((contours[1][0].1 - (-0.05)).abs() < 1e-9);
        assert!((contours[1][1].0 - 10.0).abs() < 1e-9);
        assert!((contours[1][1].1 - (-0.05)).abs() < 1e-9);

        // Boundary 2: Y = -0.15 + 0.1 + 0.2 = 0.15
        assert!((contours[2][0].0 - 0.0).abs() < 1e-9);
        assert!((contours[2][0].1 - 0.15).abs() < 1e-9);
        assert!((contours[2][1].0 - 10.0).abs() < 1e-9);
        assert!((contours[2][1].1 - 0.15).abs() < 1e-9);
    }

    #[test]
    fn three_point_bend_centerline() {
        // L-bend: (0,0) -> (10,0) -> (10,10)
        // One layer of 0.2. Total = 0.2.
        // Boundaries at -0.1 and 0.1.
        let centerline = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        let layers = vec![0.2];
        let contours = layer_contours(&centerline, &layers);

        assert_eq!(contours.len(), 2);

        // Check first boundary (-0.1 offset)
        // Segment 1 normal: (0, 1). Segment 2 normal: (-1, 0).
        // Vertex 0: (0, 0) + (0, 1)*(-0.1) = (0, -0.1)
        assert!((contours[0][0].0 - 0.0).abs() < 1e-9);
        assert!((contours[0][0].1 - (-0.1)).abs() < 1e-9);

        // Vertex 1 (bend): (10, 0). 
        // n1 = (0, 1), n2 = (-1, 0). 
        // bisector = normalize((0-1, 1+0)) = normalize((-1, 1)) = (-1/sqrt2, 1/sqrt2)
        // dot = n1 dot bisector = 1/sqrt2.
        // scale = sqrt2.
        // offset = (-1/sqrt2 * sqrt2 * -0.1, 1/sqrt2 * sqrt2 * -0.1) = (0.1, -0.1)
        // Point = (10, 0) + (0.1, -0.1) = (10.1, -0.1)
        assert!((contours[0][1].0 - 10.1).abs() < 1e-9);
        assert!((contours[0][1].1 - (-0.1)).abs() < 1e-9);

        // Vertex 2: (10, 10) + (-1, 0)*(-0.1) = (10.1, 10.0)
        assert!((contours[0][2].0 - 10.1).abs() < 1e-9);
        assert!((contours[0][2].1 - 10.0).abs() < 1e-9);

        // Check second boundary (0.1 offset)
        // Vertex 1 bend: (10, 0) + (-0.1, 0.1) = (9.9, 0.1)
        assert!((contours[1][1].0 - 9.9).abs() < 1e-9);
        assert!((contours[1][1].1 - 0.1).abs() < 1e-9);
    }
}
