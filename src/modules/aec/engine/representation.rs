//! Shared per-wall 2D display representation.
//!
//! [`WallRepresentation`] is the single intermediate structure that holds the
//! axis, outer contour, per-layer footprints, and a cheap drag-ghost outline
//! for one wall. Geometry math stays in [`super::contour`] — this module only
//! organizes the call site so Axis / Outer Contour / Layer Contours are derived
//! together instead of via ad-hoc separate calls.
//!
//! The 3D solid path (`sweep_model` / `solid_model`) remains a separate pipeline
//! and is intentionally not folded into this structure.

use super::arc::offset_polyline_with_bulges;
use super::contour::{
    closed_layer_footprint, layer_contours_with_bulges, outer_contour_with_bulges,
};
use super::geometry::normal;
use super::openings::{
    opening_footprint_2d, subtract_openings_from_band, subtract_openings_from_band_with_builder,
    Opening,
};

/// Shared 2D display components for a single wall.
///
/// Produced by [`build_wall_representation`] from an axis polyline and layer
/// stack. Callers that need the wall's visual 2D geometry should read these
/// fields rather than invoking `outer_contour` / `layer_contours` piecemeal.
///
/// ## Openings (through-cuts)
///
/// A full-thickness opening splits the wall band into **disconnected pieces**
/// (not an interior hole). When openings are applied via
/// [`build_wall_representation_with_openings`]:
/// - [`Self::cut_outer_pieces_2d`] holds every remaining outer body polygon
/// - [`Self::cut_layer_pieces_2d`]`[i]` holds pieces for layer `i`
/// - [`Self::outer_contour_2d`] / [`Self::layer_contours_2d`] keep the first
///   piece (or stay empty if the wall is fully consumed) so single-polygon
///   callers still see a valid ring
/// - [`Self::opening_footprints_2d`] lists each opening's 2D rectangle
///
/// When no openings are applied the `cut_*` / `opening_footprints_2d` fields
/// are empty and the uncut contours are authoritative.
#[derive(Debug, Clone, PartialEq)]
pub struct WallRepresentation {
    /// Wall axis / centerline polyline (open).
    pub axis: Vec<(f64, f64)>,
    /// Per-vertex LWPOLYLINE bulges on the axis (`0.0` = straight). Same length
    /// convention as `acadrust::LwVertex::bulge` (one entry per axis vertex).
    pub axis_bulges: Vec<f64>,
    /// Combined outer boundary polygon (closed loop, first point not repeated).
    /// With openings: first remaining piece (see [`Self::cut_outer_pieces_2d`]).
    pub outer_contour_2d: Vec<(f64, f64)>,
    /// Bulges for [`Self::outer_contour_2d`] (closed LWPOLYLINE convention).
    pub outer_contour_bulges: Vec<f64>,
    /// One closed footprint polygon per layer (same winding convention as the
    /// extrusion path: forward along the inner boundary, back along the outer).
    /// With openings: first remaining piece per layer.
    pub layer_contours_2d: Vec<Vec<(f64, f64)>>,
    /// Per-layer bulges matching [`Self::layer_contours_2d`].
    pub layer_contour_bulges: Vec<Vec<f64>>,
    /// Lightweight approximate outer outline for interactive drag / grip /
    /// move previews. Uses simple parallel offsets without full corner mitering.
    pub drag_ghost: Vec<(f64, f64)>,
    /// Opening footprint rectangles in world XY (empty when no openings).
    pub opening_footprints_2d: Vec<Vec<(f64, f64)>>,
    /// Disconnected outer body pieces after opening subtraction. Empty means
    /// the wall is uncut and [`Self::outer_contour_2d`] is the sole contour.
    pub cut_outer_pieces_2d: Vec<Vec<(f64, f64)>>,
    /// Per-layer disconnected pieces after opening subtraction. Empty outer
    /// vec, or empty per-layer entry, means use [`Self::layer_contours_2d`].
    pub cut_layer_pieces_2d: Vec<Vec<Vec<(f64, f64)>>>,
}

/// Build a complete [`WallRepresentation`] from an axis and layer stack.
///
/// `layers` is a list of `(thickness, gap_before)` pairs — the same layout
/// consumed by [`layer_contours_with_bulges`]. `centerline_offset` is forwarded
/// to [`outer_contour_with_bulges`] (0.0 for a center-justified / already-shifted
/// axis). `axis_bulges` may be empty (all straight) or shorter than `axis`
/// (missing entries default to `0.0`).
///
/// Does not reimplement geometry: outer and layer contours are produced by the
/// contour helpers so straight-axis results stay bit-identical to the legacy
/// path.
pub fn build_wall_representation(
    axis: &[(f64, f64)],
    layers: &[(f64, f64)],
    centerline_offset: f64,
) -> WallRepresentation {
    build_wall_representation_with_bulges(axis, &[], layers, centerline_offset)
}

/// Bulge-aware variant of [`build_wall_representation`].
pub fn build_wall_representation_with_bulges(
    axis: &[(f64, f64)],
    axis_bulges: &[f64],
    layers: &[(f64, f64)],
    centerline_offset: f64,
) -> WallRepresentation {
    let total_thickness: f64 = layers.iter().map(|(t, g)| t + g).sum();

    let pairs = layer_contours_with_bulges(axis, axis_bulges, layers);
    let mut layer_contours_2d = Vec::with_capacity(pairs.len());
    let mut layer_contour_bulges = Vec::with_capacity(pairs.len());
    for (b1, b2) in &pairs {
        let fp = closed_layer_footprint(b1, b2);
        layer_contours_2d.push(fp.points);
        layer_contour_bulges.push(fp.bulges);
    }

    let outer = outer_contour_with_bulges(axis, axis_bulges, total_thickness, centerline_offset);
    let drag_ghost = build_drag_ghost_with_bulges(axis, axis_bulges, total_thickness);

    let mut axis_bulges_out = vec![0.0; axis.len()];
    let n = axis_bulges.len().min(axis.len());
    axis_bulges_out[..n].copy_from_slice(&axis_bulges[..n]);

    WallRepresentation {
        axis: axis.to_vec(),
        axis_bulges: axis_bulges_out,
        outer_contour_2d: outer.points,
        outer_contour_bulges: outer.bulges,
        layer_contours_2d,
        layer_contour_bulges,
        drag_ghost,
        opening_footprints_2d: Vec::new(),
        cut_outer_pieces_2d: Vec::new(),
        cut_layer_pieces_2d: Vec::new(),
    }
}

/// Like [`build_wall_representation_with_bulges`], then subtracts each opening's
/// full-thickness footprint from the outer and per-layer contours.
///
/// Empty `openings` is equivalent to [`build_wall_representation_with_bulges`].
///
/// **Contour semantics:** through-openings split the wall band into disconnected
/// pieces stored in [`WallRepresentation::cut_outer_pieces_2d`] /
/// [`WallRepresentation::cut_layer_pieces_2d`]. The legacy single-polygon fields
/// hold the first piece for back-compat. 3D solid cutting is **not** performed
/// here (deferred — see `openings` module docs).
///
/// Opening placement uses straight-segment axis distance (chord length on arcs).
pub fn build_wall_representation_with_openings(
    axis: &[(f64, f64)],
    axis_bulges: &[f64],
    layers: &[(f64, f64)],
    centerline_offset: f64,
    openings: &[Opening],
) -> WallRepresentation {
    let mut repr =
        build_wall_representation_with_bulges(axis, axis_bulges, layers, centerline_offset);
    if openings.is_empty() || axis.len() < 2 {
        return repr;
    }

    let total_thickness: f64 = layers.iter().map(|(t, g)| t + g).sum();

    // Footprints for diagnostics / future hatch-hole paths.
    repr.opening_footprints_2d = openings
        .iter()
        .filter_map(|o| opening_footprint_2d(axis, total_thickness, o))
        .collect();

    // Outer band: rebuild free spans via outer_contour (centerline_offset applied
    // on the uncut path already shifted the stored axis for drawn walls, so
    // openings operate in axis-local coordinates with offset 0).
    // When centerline_offset != 0 the uncut outer was built with that offset;
    // re-cut pieces must use the same offset for consistency.
    let outer_pieces = if centerline_offset.abs() < 1e-15 {
        subtract_openings_from_band(&repr.outer_contour_2d, axis, total_thickness, openings)
    } else {
        subtract_openings_from_band_with_builder(axis, openings, |sub| {
            outer_contour_with_bulges(sub, &[], total_thickness, centerline_offset).points
        })
    };
    repr.cut_outer_pieces_2d = outer_pieces.clone();
    if let Some(first) = outer_pieces.first() {
        repr.outer_contour_2d = first.clone();
        repr.outer_contour_bulges = vec![0.0; first.len()];
    } else {
        repr.outer_contour_2d.clear();
        repr.outer_contour_bulges.clear();
    }

    // Per-layer footprints: rebuild each free span with the same layer stack.
    let mut cut_layers: Vec<Vec<Vec<(f64, f64)>>> = Vec::with_capacity(layers.len());
    let n_layers = layers.len();
    for li in 0..n_layers {
        let layer_spec = layers[li];
        let pieces = subtract_openings_from_band_with_builder(axis, openings, |sub| {
            // Single-layer footprint at this layer's position in the stack:
            // rebuild full stack on the sub-axis and pick layer `li`.
            let pairs = layer_contours_with_bulges(sub, &[], layers);
            if let Some((b1, b2)) = pairs.get(li) {
                closed_layer_footprint(b1, b2).points
            } else {
                // Fallback: treat as a simple band of this layer's thickness.
                let _ = layer_spec;
                Vec::new()
            }
        });
        if let Some(first) = pieces.first() {
            if li < repr.layer_contours_2d.len() {
                repr.layer_contours_2d[li] = first.clone();
                if li < repr.layer_contour_bulges.len() {
                    repr.layer_contour_bulges[li] = vec![0.0; first.len()];
                }
            }
        } else if li < repr.layer_contours_2d.len() {
            repr.layer_contours_2d[li].clear();
            if li < repr.layer_contour_bulges.len() {
                repr.layer_contour_bulges[li].clear();
            }
        }
        cut_layers.push(pieces);
    }
    repr.cut_layer_pieces_2d = cut_layers;

    repr
}

/// Cheap approximate outer contour for interactive drag operations.
///
/// Uses per-vertex unit offset directions **without** the miter scale applied
/// by [`super::geometry::get_offset_directions`], so sharp corners stay cheap
/// and do not spike. Suitable for move/grip/draw previews where a full mitered
/// outline is unnecessary.
///
/// Assumes center justification (offset 0). Returns an empty vec when the axis
/// has fewer than two points or `total_thickness` is non-positive.
pub fn build_drag_ghost(axis: &[(f64, f64)], total_thickness: f64) -> Vec<(f64, f64)> {
    build_drag_ghost_with_bulges(axis, &[], total_thickness)
}

/// Bulge-aware drag ghost. Arc segments use a cheap radial offset (exact for a
/// single arc); straight vertices keep the non-mitered unit bisector.
pub fn build_drag_ghost_with_bulges(
    axis: &[(f64, f64)],
    axis_bulges: &[f64],
    total_thickness: f64,
) -> Vec<(f64, f64)> {
    if axis.len() < 2 || total_thickness <= 0.0 {
        return Vec::new();
    }

    let half = total_thickness * 0.5;
    let has_arc = axis_bulges.iter().any(|b| b.abs() > 1e-12);
    if has_arc {
        let (b1, _) = offset_polyline_with_bulges(axis, axis_bulges, -half);
        let (mut b2, _) = offset_polyline_with_bulges(axis, axis_bulges, half);
        let mut result = b1;
        b2.reverse();
        result.extend(b2);
        return result;
    }

    let directions = simple_offset_directions(axis);

    let b1: Vec<(f64, f64)> = axis
        .iter()
        .zip(directions.iter())
        .map(|(&(x, y), &(dx, dy))| (x + dx * (-half), y + dy * (-half)))
        .collect();

    let mut b2: Vec<(f64, f64)> = axis
        .iter()
        .zip(directions.iter())
        .map(|(&(x, y), &(dx, dy))| (x + dx * half, y + dy * half))
        .collect();

    let mut result = b1;
    b2.reverse();
    result.extend(b2);
    result
}

/// Unit offset directions per vertex without miter scaling.
///
/// Endpoints use the adjacent segment normal; interior vertices use the unit
/// angle-bisector of the two segment normals (scale = 1).
fn simple_offset_directions(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let n = points.len();
    let mut directions = Vec::with_capacity(n);
    if n < 2 {
        directions.resize(n, (0.0, 0.0));
        return directions;
    }

    for i in 0..n {
        let dir = if i == 0 {
            let (x0, y0) = points[0];
            let (x1, y1) = points[1];
            normal(x0, y0, x1, y1)
        } else if i == n - 1 {
            let (x0, y0) = points[n - 2];
            let (x1, y1) = points[n - 1];
            normal(x0, y0, x1, y1)
        } else {
            let (x0, y0) = points[i - 1];
            let (x1, y1) = points[i];
            let (x2, y2) = points[i + 1];
            let n1 = normal(x0, y0, x1, y1);
            let n2 = normal(x1, y1, x2, y2);
            let bx = n1.0 + n2.0;
            let by = n1.1 + n2.1;
            let b_len = (bx * bx + by * by).sqrt();
            if b_len < 1e-9 {
                n1
            } else {
                (bx / b_len, by / b_len)
            }
        };
        directions.push(dir);
    }
    directions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::contour::{layer_contours, outer_contour};

    fn pts_close(a: &[(f64, f64)], b: &[(f64, f64)]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        a.iter()
            .zip(b.iter())
            .all(|(&(x0, y0), &(x1, y1))| (x0 - x1).abs() < 1e-9 && (y0 - y1).abs() < 1e-9)
    }

    #[test]
    fn build_wall_representation_matches_direct_contour_calls() {
        // Straight single-layer wall — proves the refactor is behavior-preserving.
        let axis = vec![(0.0, 0.0), (10.0, 0.0)];
        let layers = vec![(0.2, 0.0)];
        let centerline_offset = 0.0;

        let repr = build_wall_representation(&axis, &layers, centerline_offset);

        let expected_outer = outer_contour(&axis, 0.2, centerline_offset);
        assert!(
            pts_close(&repr.outer_contour_2d, &expected_outer),
            "outer_contour_2d must match outer_contour() directly"
        );

        let pairs = layer_contours(&axis, &layers);
        assert_eq!(repr.layer_contours_2d.len(), pairs.len());
        for (footprint, (b1, b2)) in repr.layer_contours_2d.iter().zip(pairs.iter()) {
            let mut expected = Vec::with_capacity(b1.len() + b2.len());
            expected.extend(b1.iter().copied());
            expected.extend(b2.iter().rev().copied());
            assert!(
                pts_close(footprint, &expected),
                "layer_contours_2d must match closed footprints from layer_contours()"
            );
        }

        assert_eq!(repr.axis, axis);
    }

    #[test]
    fn build_wall_representation_multi_layer_matches_direct_calls() {
        let axis = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        let layers = vec![(0.1, 0.0), (0.2, 0.05)];
        let centerline_offset = -0.1;

        let repr = build_wall_representation(&axis, &layers, centerline_offset);
        let total: f64 = layers.iter().map(|(t, g)| t + g).sum();
        let expected_outer = outer_contour(&axis, total, centerline_offset);
        assert!(pts_close(&repr.outer_contour_2d, &expected_outer));

        let pairs = layer_contours(&axis, &layers);
        assert_eq!(repr.layer_contours_2d.len(), 2);
        for (footprint, (b1, b2)) in repr.layer_contours_2d.iter().zip(pairs.iter()) {
            let mut expected = Vec::with_capacity(b1.len() + b2.len());
            expected.extend(b1.iter().copied());
            expected.extend(b2.iter().rev().copied());
            assert!(pts_close(footprint, &expected));
        }
    }

    #[test]
    fn build_drag_ghost_returns_non_empty_reasonable_polygon() {
        let axis = vec![(0.0, 0.0), (10.0, 0.0)];
        let ghost = build_drag_ghost(&axis, 0.2);

        // Closed strip: 2 points per side → 4 vertices.
        assert_eq!(ghost.len(), 4);
        // Bottom side at y = -0.1, top side at y = 0.1 (after reverse).
        assert!((ghost[0].1 - (-0.1)).abs() < 1e-9);
        assert!((ghost[1].1 - (-0.1)).abs() < 1e-9);
        assert!((ghost[2].1 - 0.1).abs() < 1e-9);
        assert!((ghost[3].1 - 0.1).abs() < 1e-9);
        // Spans the axis length in x.
        assert!((ghost[0].0 - 0.0).abs() < 1e-9);
        assert!((ghost[1].0 - 10.0).abs() < 1e-9);
    }

    #[test]
    fn build_drag_ghost_empty_for_degenerate_input() {
        assert!(build_drag_ghost(&[], 0.2).is_empty());
        assert!(build_drag_ghost(&[(0.0, 0.0)], 0.2).is_empty());
        assert!(build_drag_ghost(&[(0.0, 0.0), (1.0, 0.0)], 0.0).is_empty());
    }

    #[test]
    fn build_wall_representation_includes_drag_ghost() {
        let axis = vec![(0.0, 0.0), (5.0, 0.0)];
        let layers = vec![(0.3, 0.0)];
        let repr = build_wall_representation(&axis, &layers, 0.0);
        assert!(!repr.drag_ghost.is_empty());
        assert_eq!(repr.drag_ghost, build_drag_ghost(&axis, 0.3));
    }

    #[test]
    fn build_wall_representation_curved_preserves_bulge_on_layers() {
        let axis = vec![(-1.0, 0.0), (1.0, 0.0)];
        let bulges = vec![1.0];
        let layers = vec![(0.2, 0.0)];
        let repr = build_wall_representation_with_bulges(&axis, &bulges, &layers, 0.0);
        assert_eq!(repr.layer_contours_2d.len(), 1);
        // Closed footprint: 2 + 2 vertices; first segment keeps bulge ≈ 1.
        assert!(repr.layer_contour_bulges[0][0].abs() > 0.5);
        assert!((repr.axis_bulges[0] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn build_wall_representation_with_openings_cuts_outer_and_layer() {
        use crate::modules::aec::engine::geometry::area;
        use crate::modules::aec::engine::openings::Opening;
        use acadrust::Handle;

        let axis = vec![(0.0, 0.0), (10.0, 0.0)];
        let layers = vec![(0.2, 0.0)];
        let opening = Opening {
            handle: Handle::new(10),
            host_wall: Handle::new(1),
            distance_along_axis: 5.0,
            width: 1.2,
            height: 1.2,
            sill_height: 0.9,
            kind: crate::modules::aec::engine::openings::OpeningKind::Window,
        };

        let uncut = build_wall_representation(&axis, &layers, 0.0);
        let cut = build_wall_representation_with_openings(&axis, &[], &layers, 0.0, &[opening]);

        assert_eq!(cut.cut_outer_pieces_2d.len(), 2);
        assert_eq!(cut.opening_footprints_2d.len(), 1);
        assert!(!cut.outer_contour_2d.is_empty());
        assert_eq!(cut.outer_contour_2d, cut.cut_outer_pieces_2d[0]);

        let uncut_area = area(&uncut.outer_contour_2d);
        let cut_area: f64 = cut.cut_outer_pieces_2d.iter().map(|p| area(p)).sum();
        assert!((cut_area - (uncut_area - 1.2 * 0.2)).abs() < 1e-9);

        assert_eq!(cut.cut_layer_pieces_2d.len(), 1);
        assert_eq!(cut.cut_layer_pieces_2d[0].len(), 2);
        let layer_cut: f64 = cut.cut_layer_pieces_2d[0].iter().map(|p| area(p)).sum();
        assert!((layer_cut - cut_area).abs() < 1e-9);
    }

    #[test]
    fn build_wall_representation_with_openings_empty_is_uncut() {
        let axis = vec![(0.0, 0.0), (5.0, 0.0)];
        let layers = vec![(0.3, 0.0)];
        let a = build_wall_representation(&axis, &layers, 0.0);
        let b = build_wall_representation_with_openings(&axis, &[], &layers, 0.0, &[]);
        assert_eq!(a.outer_contour_2d, b.outer_contour_2d);
        assert!(b.cut_outer_pieces_2d.is_empty());
        assert!(b.opening_footprints_2d.is_empty());
    }
}
