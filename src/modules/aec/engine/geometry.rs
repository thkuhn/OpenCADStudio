//! Pure geometry helpers for closed 2D polygons.
//!
//! These functions have no dependency on any drawing/entity type — they
//! operate on plain `(f64, f64)` point lists so they can be unit tested in
//! isolation and reused by both `room.rs` (area/perimeter/volume) and the
//! IFC writer.

/// A closed polygon boundary, expressed as an ordered list of 2D points.
/// The polygon is implicitly closed (the last point connects back to the
/// first); callers should *not* repeat the first point at the end.
pub type Polygon2D = [(f64, f64)];

/// Signed area of a closed polygon via the shoelace formula.
///
/// Positive for counter-clockwise point order, negative for clockwise.
/// Returns `0.0` for fewer than 3 points.
pub fn signed_area(points: &Polygon2D) -> f64 {
    let n = points.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..n {
        let (x0, y0) = points[i];
        let (x1, y1) = points[(i + 1) % n];
        sum += x0 * y1 - x1 * y0;
    }
    sum * 0.5
}

/// Unsigned area of a closed polygon via the shoelace formula.
pub fn area(points: &Polygon2D) -> f64 {
    signed_area(points).abs()
}

/// Perimeter of a closed polygon: sum of the Euclidean lengths of every
/// boundary segment, including the implicit closing segment.
pub fn perimeter(points: &Polygon2D) -> f64 {
    let n = points.len();
    if n < 2 {
        return 0.0;
    }
    let mut total = 0.0;
    for i in 0..n {
        let (x0, y0) = points[i];
        let (x1, y1) = points[(i + 1) % n];
        total += ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
    }
    total
}

/// Volume of a room derived from a closed floor polygon and a storey height:
/// `area(points) * height`.
pub fn volume(points: &Polygon2D, height: f64) -> f64 {
    area(points) * height
}

/// Pre-calculates the offset direction and miter scale for each vertex of an
/// open polyline.
pub fn get_offset_directions(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let n = points.len();
    let mut directions = Vec::with_capacity(n);
    if n < 2 {
        for _ in 0..n {
            directions.push((0.0, 0.0));
        }
        return directions;
    }

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
pub fn normal(x0: f64, y0: f64, x1: f64, y1: f64) -> (f64, f64) {
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

    fn rect() -> Vec<(f64, f64)> {
        // 4m x 3m rectangle, counter-clockwise.
        vec![(0.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 3.0)]
    }

    fn l_shape() -> Vec<(f64, f64)> {
        // L-shape: a 4x4 square with a 2x2 notch removed from the top-right
        // corner. Area = 16 - 4 = 12. Counter-clockwise winding.
        vec![
            (0.0, 0.0),
            (4.0, 0.0),
            (4.0, 2.0),
            (2.0, 2.0),
            (2.0, 4.0),
            (0.0, 4.0),
        ]
    }

    #[test]
    fn rectangle_area_matches_width_times_height() {
        assert_eq!(area(&rect()), 12.0);
    }

    #[test]
    fn rectangle_area_sign_flips_with_winding_order() {
        let mut pts = rect();
        pts.reverse();
        assert_eq!(signed_area(&rect()), -signed_area(&pts));
        assert_eq!(area(&pts), 12.0);
    }

    #[test]
    fn rectangle_perimeter_matches_expected() {
        assert_eq!(perimeter(&rect()), 14.0);
    }

    #[test]
    fn rectangle_volume_scales_with_height() {
        assert_eq!(volume(&rect(), 2.5), 30.0);
    }

    #[test]
    fn l_shape_area_matches_expected() {
        assert_eq!(area(&l_shape()), 12.0);
    }

    #[test]
    fn l_shape_perimeter_matches_expected() {
        // 4 + 2 + 2 + 2 + 2 + 4 = 16
        assert_eq!(perimeter(&l_shape()), 16.0);
    }

    #[test]
    fn l_shape_volume_scales_with_height() {
        assert_eq!(volume(&l_shape(), 3.0), 36.0);
    }

    #[test]
    fn degenerate_polygon_has_zero_area_and_perimeter() {
        assert_eq!(area(&[]), 0.0);
        assert_eq!(area(&[(0.0, 0.0), (1.0, 0.0)]), 0.0);
        assert_eq!(perimeter(&[]), 0.0);
    }
}
