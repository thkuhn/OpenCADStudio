//! Opening-slot sketches in a style reference box, baked with a two-rectangle map.
//!
//! Geometry is authored in local XY (origin = insertion, X = width, Y = wall
//! thickness). Bake maps the **outer** reference rectangle onto the instance
//! box and the **inner** rectangle (outer minus absolute `frame_thickness`)
//! onto the instance inner box. Points in the frame ring keep their distance
//! from the outer edge, so profile thickness does not scale with instance
//! width. This is **not** affine uniform scaling.

use serde::{Deserialize, Serialize};

use crate::modules::aec::engine::arc::{arc_point_at, bulge_to_arc};

/// Chord count used to approximate a bulge arc in the reference box.
const BULGE_CHORD_COUNT: usize = 16;

/// A 2D sketch path in the style's local reference box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OpeningSketchPath {
    /// Vertices in local XY (origin = insertion, X = width, Y = wall thickness).
    pub points: Vec<(f64, f64)>,
    /// Optional bulge per segment (same convention as LWPOLYLINE); empty = all 0.
    #[serde(default)]
    pub bulges: Vec<f64>,
    #[serde(default)]
    pub closed: bool,
}

impl OpeningSketchPath {
    pub fn open(points: Vec<(f64, f64)>) -> Self {
        Self {
            points,
            bulges: Vec::new(),
            closed: false,
        }
    }

    pub fn closed_rect(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            points: vec![
                (min_x, min_y),
                (max_x, min_y),
                (max_x, max_y),
                (min_x, max_y),
            ],
            bulges: Vec::new(),
            closed: true,
        }
    }

    pub fn is_drawable(&self) -> bool {
        self.points.len() >= 2
    }
}

/// Sketch stored in a style reference box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningSketch {
    pub ref_width: f64,
    pub ref_thickness: f64,
    #[serde(default)]
    pub paths: Vec<OpeningSketchPath>,
}

impl Default for OpeningSketch {
    fn default() -> Self {
        Self {
            ref_width: 1.0,
            ref_thickness: 0.3,
            paths: Vec::new(),
        }
    }
}

impl OpeningSketch {
    pub fn is_empty(&self) -> bool {
        self.paths.iter().all(|p| !p.is_drawable())
    }

    /// Outer + inner rectangles inset by absolute `frame_thickness`.
    pub fn frame_ring(ref_width: f64, ref_thickness: f64, frame_thickness: f64) -> Self {
        let hw = (ref_width * 0.5).max(0.0);
        let ht = (ref_thickness * 0.5).max(0.0);
        let ft = frame_thickness.max(0.0);
        let mut paths = vec![OpeningSketchPath::closed_rect(-hw, -ht, hw, ht)];
        let iw = hw - ft;
        let it = ht - ft;
        if iw > 1e-9 && it > 1e-9 {
            paths.push(OpeningSketchPath::closed_rect(-iw, -it, iw, it));
        }
        Self {
            ref_width: ref_width.max(1e-9),
            ref_thickness: ref_thickness.max(1e-9),
            paths,
        }
    }
}

/// Two-rectangle mapping from a style reference box onto an instance box.
#[derive(Debug, Clone, Copy)]
pub struct TwoRectBake {
    pub ref_width: f64,
    pub ref_thickness: f64,
    pub inst_width: f64,
    pub inst_thickness: f64,
    pub frame_thickness: f64,
}

impl TwoRectBake {
    pub fn new(
        ref_width: f64,
        ref_thickness: f64,
        inst_width: f64,
        inst_thickness: f64,
        frame_thickness: f64,
    ) -> Self {
        Self {
            ref_width: ref_width.max(1e-12),
            ref_thickness: ref_thickness.max(1e-12),
            inst_width: inst_width.max(1e-12),
            inst_thickness: inst_thickness.max(1e-12),
            frame_thickness: frame_thickness.max(0.0),
        }
    }

    pub fn from_sketch(
        sketch: &OpeningSketch,
        inst_width: f64,
        inst_thickness: f64,
        frame_thickness: f64,
    ) -> Self {
        Self::new(
            sketch.ref_width,
            sketch.ref_thickness,
            inst_width,
            inst_thickness,
            frame_thickness,
        )
    }

    fn halves(outer: f64, frame: f64) -> (f64, f64) {
        let outer_h = (outer * 0.5).max(0.0);
        let inner_h = (outer_h - frame).max(0.0);
        (outer_h, inner_h)
    }

    fn map_axis(x: f64, outer_ref: f64, inner_ref: f64, outer_inst: f64, inner_inst: f64) -> f64 {
        if outer_ref <= 1e-12 {
            return 0.0;
        }
        let ax = x.abs();
        let sign = if x < 0.0 { -1.0 } else { 1.0 };
        if inner_ref <= 1e-12 {
            return x * (outer_inst / outer_ref);
        }
        if ax + 1e-15 >= inner_ref {
            let dist_from_outer = (outer_ref - ax).max(0.0);
            sign * (outer_inst - dist_from_outer)
        } else {
            x * (inner_inst / inner_ref)
        }
    }

    fn unmap_axis(y: f64, outer_ref: f64, inner_ref: f64, outer_inst: f64, inner_inst: f64) -> f64 {
        if outer_inst <= 1e-12 {
            return 0.0;
        }
        let ay = y.abs();
        let sign = if y < 0.0 { -1.0 } else { 1.0 };
        if inner_inst <= 1e-12 {
            return y * (outer_ref / outer_inst);
        }
        if ay + 1e-15 >= inner_inst {
            let dist_from_outer = (outer_inst - ay).max(0.0);
            sign * (outer_ref - dist_from_outer)
        } else {
            y * (inner_ref / inner_inst)
        }
    }

    /// Map a reference-space point into the instance box.
    pub fn map_point(&self, p: (f64, f64)) -> (f64, f64) {
        let (ow_r, iw_r) = Self::halves(self.ref_width, self.frame_thickness);
        let (ot_r, it_r) = Self::halves(self.ref_thickness, self.frame_thickness);
        let (ow_i, iw_i) = Self::halves(self.inst_width, self.frame_thickness);
        let (ot_i, it_i) = Self::halves(self.inst_thickness, self.frame_thickness);
        (
            Self::map_axis(p.0, ow_r, iw_r, ow_i, iw_i),
            Self::map_axis(p.1, ot_r, it_r, ot_i, it_i),
        )
    }

    /// Inverse of [`Self::map_point`] (instance → reference).
    pub fn unmap_point(&self, p: (f64, f64)) -> (f64, f64) {
        let (ow_r, iw_r) = Self::halves(self.ref_width, self.frame_thickness);
        let (ot_r, it_r) = Self::halves(self.ref_thickness, self.frame_thickness);
        let (ow_i, iw_i) = Self::halves(self.inst_width, self.frame_thickness);
        let (ot_i, it_i) = Self::halves(self.inst_thickness, self.frame_thickness);
        (
            Self::unmap_axis(p.0, ow_r, iw_r, ow_i, iw_i),
            Self::unmap_axis(p.1, ot_r, it_r, ot_i, it_i),
        )
    }

    /// Tessellate a path in reference space, then map each sample.
    pub fn bake_path(&self, path: &OpeningSketchPath) -> Vec<(f64, f64)> {
        tessellate_path(path)
            .into_iter()
            .map(|p| self.map_point(p))
            .collect()
    }
}

/// One baked polyline in **instance** local coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct BakedSketchPath {
    pub points: Vec<(f64, f64)>,
    pub closed: bool,
}

/// Bake every drawable path. An empty sketch yields no paths (no generator fallback).
pub fn bake_sketch(
    sketch: &OpeningSketch,
    inst_width: f64,
    inst_thickness: f64,
    frame_thickness: f64,
) -> Vec<BakedSketchPath> {
    if sketch.is_empty() {
        return Vec::new();
    }
    let bake = TwoRectBake::from_sketch(sketch, inst_width, inst_thickness, frame_thickness);
    sketch
        .paths
        .iter()
        .filter(|p| p.is_drawable())
        .map(|p| BakedSketchPath {
            points: bake.bake_path(p),
            closed: p.closed,
        })
        .filter(|p| p.points.len() >= 2)
        .collect()
}

/// Tessellate bulges in **reference** space (chords). Straight segments stay as-is.
pub fn tessellate_path(path: &OpeningSketchPath) -> Vec<(f64, f64)> {
    let n = path.points.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return path.points.clone();
    }
    let seg_count = if path.closed { n } else { n - 1 };
    let mut out = Vec::new();
    for i in 0..seg_count {
        let start = path.points[i];
        let end = path.points[(i + 1) % n];
        if i == 0 {
            out.push(start);
        }
        let bulge = path.bulges.get(i).copied().unwrap_or(0.0);
        if let Some(arc) = bulge_to_arc(start, end, bulge) {
            let samples = BULGE_CHORD_COUNT.max(4);
            for s in 1..=samples {
                let t = s as f64 / samples as f64;
                out.push(arc_point_at(&arc, t));
            }
        } else {
            out.push(end);
        }
    }
    out
}

/// Snap `p` to outer/inner corners and origin of the reference box.
pub fn snap_ref_point(
    p: (f64, f64),
    ref_width: f64,
    ref_thickness: f64,
    frame_thickness: f64,
    tolerance: f64,
) -> (f64, f64) {
    let hw = ref_width * 0.5;
    let ht = ref_thickness * 0.5;
    let ft = frame_thickness.max(0.0);
    let mut candidates = vec![
        (0.0, 0.0),
        (-hw, -ht),
        (hw, -ht),
        (hw, ht),
        (-hw, ht),
        (0.0, -ht),
        (0.0, ht),
        (-hw, 0.0),
        (hw, 0.0),
    ];
    let iw = hw - ft;
    let it = ht - ft;
    if iw > 1e-9 && it > 1e-9 {
        candidates.extend([
            (-iw, -it),
            (iw, -it),
            (iw, it),
            (-iw, it),
            (0.0, -it),
            (0.0, it),
            (-iw, 0.0),
            (iw, 0.0),
        ]);
    }
    let mut best = p;
    let mut best_d = tolerance;
    for c in candidates {
        let d = (p.0 - c.0).hypot(p.1 - c.1);
        if d <= best_d {
            best_d = d;
            best = c;
        }
    }
    best
}

/// Append a vertex to an in-progress draft polyline.
pub fn draft_append(points: &mut Vec<(f64, f64)>, bulges: &mut Vec<f64>, p: (f64, f64)) {
    if let Some(last) = points.last() {
        if (last.0 - p.0).abs() < 1e-12 && (last.1 - p.1).abs() < 1e-12 {
            return;
        }
    }
    points.push(p);
    if points.len() >= 2 {
        while bulges.len() < points.len() - 1 {
            bulges.push(0.0);
        }
    }
}

/// Mark the last completed draft segment as a semicircle (bulge = ±1).
pub fn draft_set_last_arc(bulges: &mut [f64], ccw: bool) {
    if let Some(b) = bulges.last_mut() {
        *b = if ccw { 1.0 } else { -1.0 };
    }
}

/// Commit a draft polyline onto the sketch (open or closed).
pub fn commit_draft(
    sketch: &mut OpeningSketch,
    points: &mut Vec<(f64, f64)>,
    bulges: &mut Vec<f64>,
    closed: bool,
) {
    if points.len() < 2 {
        points.clear();
        bulges.clear();
        return;
    }
    sketch.paths.push(OpeningSketchPath {
        points: std::mem::take(points),
        bulges: std::mem::take(bulges),
        closed,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const FT: f64 = 0.06;

    #[test]
    fn empty_sketch_bakes_to_nothing() {
        let sketch = OpeningSketch::default();
        assert!(sketch.is_empty());
        let baked = bake_sketch(&sketch, 1.2, 0.24, FT);
        assert!(baked.is_empty());
    }

    #[test]
    fn two_rect_preserves_absolute_inset_1x03_to_12x024() {
        let sketch = OpeningSketch::frame_ring(1.0, 0.3, FT);
        let baked = bake_sketch(&sketch, 1.2, 0.24, FT);
        assert_eq!(baked.len(), 2);

        let outer = &baked[0].points;
        let inner = &baked[1].points;
        let max_x = |pts: &[(f64, f64)]| pts.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        let max_y = |pts: &[(f64, f64)]| pts.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);

        assert!((max_x(outer) - 0.6).abs() < 1e-9);
        assert!((max_y(outer) - 0.12).abs() < 1e-9);
        assert!((max_x(inner) - (0.6 - FT)).abs() < 1e-9);
        assert!((max_y(inner) - (0.12 - FT)).abs() < 1e-9);

        let inset_x = max_x(outer) - max_x(inner);
        let inset_y = max_y(outer) - max_y(inner);
        assert!((inset_x - FT).abs() < 1e-9);
        assert!((inset_y - FT).abs() < 1e-9);

        // Uniform affine scale would shrink the Y inset with thickness 0.3 → 0.24.
        let uniform_y_inset = FT * (0.24 / 0.3);
        assert!((inset_y - uniform_y_inset).abs() > 1e-6);
        let uniform_x_inner = 0.44 * (1.2 / 1.0);
        assert!((max_x(inner) - uniform_x_inner).abs() > 1e-6);
    }

    #[test]
    fn map_then_unmap_roundtrips_frame_corners() {
        let bake = TwoRectBake::new(1.0, 0.3, 1.2, 0.24, FT);
        for p in [
            (0.5, 0.15),
            (-0.5, -0.15),
            (0.44, 0.09),
            (0.0, 0.0),
            (0.22, 0.0),
        ] {
            let mapped = bake.map_point(p);
            let back = bake.unmap_point(mapped);
            assert!((back.0 - p.0).abs() < 1e-9, "x {p:?} -> {mapped:?} -> {back:?}");
            assert!((back.1 - p.1).abs() < 1e-9, "y {p:?} -> {mapped:?} -> {back:?}");
        }
    }

    #[test]
    fn bulge_tessellates_to_more_than_two_points() {
        let path = OpeningSketchPath {
            points: vec![(-0.5, 0.0), (0.5, 0.0)],
            bulges: vec![1.0],
            closed: false,
        };
        let pts = tessellate_path(&path);
        assert!(pts.len() > 2);
        assert!((pts.first().unwrap().0 + 0.5).abs() < 1e-9);
        assert!((pts.last().unwrap().0 - 0.5).abs() < 1e-9);
    }

    #[test]
    fn commit_draft_requires_two_points() {
        let mut sketch = OpeningSketch::default();
        let mut pts = vec![(0.0, 0.0)];
        let mut bulges = Vec::new();
        commit_draft(&mut sketch, &mut pts, &mut bulges, true);
        assert!(sketch.is_empty());
        pts = vec![(-0.5, -0.15), (0.5, -0.15)];
        commit_draft(&mut sketch, &mut pts, &mut bulges, false);
        assert_eq!(sketch.paths.len(), 1);
        assert!(!sketch.paths[0].closed);
    }
}
