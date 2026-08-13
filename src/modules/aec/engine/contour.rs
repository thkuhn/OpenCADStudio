//! 2D layer-contour generation for walls.
//!
//! This module provides functions to compute parallel offset polylines for
//! multi-layer walls. It uses a simple per-vertex angle-bisector offset
//! for miters, which is suitable for standard architectural wall layouts.
//!
//! For an N-layer wall, this implementation returns N+1 boundary lines
//! (parallel offsets of the centerline). This choice (a) was preferred for
//! simplicity over returning closed loop-per-layer quads.

use super::geometry::get_offset_directions;

/// Computes parallel offset polylines (boundary pairs) for an open polyline
/// centerline and a list of layer (thickness, gap_before) pairs.
///
/// For a list of N layers, it returns N boundary pairs.
/// The centerline is assumed to be the middle of the total thickness (including gaps).
///
/// Corner handling uses a simple angle-bisector miter join. This may produce
/// minor imperfections at very sharp/acute corners, but matches the plan's
/// stated simplification for architectural walls.
pub fn layer_contours(
    centerline: &[(f64, f64)],
    layers: &[(f64, f64)],
) -> Vec<(Vec<(f64, f64)>, Vec<(f64, f64)>)> {
    if centerline.len() < 2 {
        return Vec::new();
    }

    let total_thickness: f64 = layers.iter().map(|(t, g)| t + g).sum();
    let mut current_offset = -total_thickness * 0.5;

    let directions = get_offset_directions(centerline);
    let mut results = Vec::with_capacity(layers.len());

    for &(t, g) in layers {
        let start_offset = current_offset + g;
        let end_offset = start_offset + t;

        let b1 = centerline
            .iter()
            .zip(directions.iter())
            .map(|(&(x, y), &(dx, dy))| (x + dx * start_offset, y + dy * start_offset))
            .collect();

        let b2 = centerline
            .iter()
            .zip(directions.iter())
            .map(|(&(x, y), &(dx, dy))| (x + dx * end_offset, y + dy * end_offset))
            .collect();

        results.push((b1, b2));
        current_offset = end_offset;
    }

    results
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
        let layers = vec![(0.1, 0.0), (0.2, 0.0)];
        let contours = layer_contours(&centerline, &layers);

        assert_eq!(contours.len(), 2);
        
        // Layer 0: Y = -0.15 to -0.05
        assert!((contours[0].0[0].1 - (-0.15)).abs() < 1e-9);
        assert!((contours[0].1[0].1 - (-0.05)).abs() < 1e-9);

        // Layer 1: Y = -0.05 to 0.15
        assert!((contours[1].0[0].1 - (-0.05)).abs() < 1e-9);
        assert!((contours[1].1[0].1 - 0.15).abs() < 1e-9);
    }

    #[test]
    fn three_point_bend_centerline() {
        // L-bend: (0,0) -> (10,0) -> (10,10)
        // One layer of 0.2. Total = 0.2.
        // Boundaries at -0.1 and 0.1.
        let centerline = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        let layers = vec![(0.2, 0.0)];
        let contours = layer_contours(&centerline, &layers);

        assert_eq!(contours.len(), 1);

        // Check first boundary (-0.1 offset)
        // Vertex 0: (0, 0) + (0, 1)*(-0.1) = (0, -0.1)
        assert!((contours[0].0[0].0 - 0.0).abs() < 1e-9);
        assert!((contours[0].0[0].1 - (-0.1)).abs() < 1e-9);

        // Vertex 1 (bend): (10, 0). 
        // Point = (10, 0) + (0.1, -0.1) = (10.1, -0.1)
        assert!((contours[0].0[1].0 - 10.1).abs() < 1e-9);
        assert!((contours[0].0[1].1 - (-0.1)).abs() < 1e-9);

        // Check second boundary (0.1 offset)
        // Vertex 1 bend: (10, 0) + (-0.1, 0.1) = (9.9, 0.1)
        assert!((contours[0].1[1].0 - 9.9).abs() < 1e-9);
        assert!((contours[0].1[1].1 - 0.1).abs() < 1e-9);
    }

    #[test]
    fn layer_with_gap() {
        // Horizontal wall along X axis from 0 to 10.
        // Layer 0: thickness 0.1, gap 0.0.
        // Layer 1: thickness 0.1, gap 0.05.
        // Total = 0.1 + 0.05 + 0.1 = 0.25.
        // Centerline at Y=0.
        // Layer 0: Y = -0.125 to -0.025
        // Gap: Y = -0.025 to 0.025
        // Layer 1: Y = 0.025 to 0.125
        let centerline = vec![(0.0, 0.0), (10.0, 0.0)];
        let layers = vec![(0.1, 0.0), (0.1, 0.05)];
        let contours = layer_contours(&centerline, &layers);

        assert_eq!(contours.len(), 2);
        
        // Layer 0
        assert!((contours[0].0[0].1 - (-0.125)).abs() < 1e-9);
        assert!((contours[0].1[0].1 - (-0.025)).abs() < 1e-9);

        // Layer 1
        assert!((contours[1].0[0].1 - 0.025).abs() < 1e-9);
        assert!((contours[1].1[0].1 - 0.125).abs() < 1e-9);
    }
}
