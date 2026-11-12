//! Per-layer miter geometry for L/T and N-way wall joins.
//!
//! Given joined wall axes and their material-layer stacks, this module
//! computes a diagonal miter line at the shared corner and clips each wall's
//! matching layer footprint against it. Layers are paired first by
//! `material`/`function` equality, then by closest cumulative offset-from-axis
//! for ties or remaining unmatched candidates; layers that still cannot be
//! uniquely resolved return `None` so the caller can fall back to the
//! single-vertex `corner_override` path.
//!
//! Multi-wall junctions (3+ walls at one point) are handled by
//! [`mitered_junction_layer_footprints`], which orders participants by angle
//! and miters each endpoint wall against its angular neighbors.

use super::contour::{layer_contours, layer_contours_with_bulges};
use super::join::{
    JoinKind, JoinOverrideStyle, Junction, JunctionOverride, JunctionRole, LayerGapOverride,
    LayerRef,
};
use super::openings::{axis_length, point_and_tangent_at_distance, remaining_axis_spans, Opening};

/// Minimum angle (5°) below which diagonal miters are rejected as degenerate.
/// At 5°, the miter intersection distance is ~23x the layer half-offset (1/sin(2.5°)).
/// Beyond this, numerical instability and extreme geometry extensions make
/// the miter unreliable; we fall back to a simple corner extension.
/// This also rejects angles near 180°, where walls are nearly collinear.
pub(super) const MIN_MITER_SINE: f64 = 0.0872; // slightly more than sin(5°)

/// One wall layer as consumed by the miter matcher: geometry plus optional
/// identity fields used for cross-wall pairing.
#[derive(Debug, Clone, PartialEq)]
pub struct MiterLayer {
    pub thickness: f64,
    pub axis_offset: f64,
    pub material: String,
    pub function: String,
    pub layer_id: uuid::Uuid,
}

impl MiterLayer {
    /// Geometry-only layer (empty material/function). Used by tests and
    /// callers that don't carry identity metadata.
    pub fn geom(thickness: f64, axis_offset: f64) -> Self {
        Self {
            thickness,
            axis_offset,
            material: String::new(),
            function: String::new(),
            layer_id: uuid::Uuid::nil(),
        }
    }

    pub fn with_id(
        thickness: f64,
        axis_offset: f64,
        material: impl Into<String>,
        function: impl Into<String>,
        layer_id: uuid::Uuid,
    ) -> Self {
        Self {
            thickness,
            axis_offset,
            material: material.into(),
            function: function.into(),
            layer_id,
        }
    }

    fn as_geom(&self) -> (f64, f64) {
        (self.thickness, self.axis_offset)
    }
}

/// Context describing the *other* wall at a join, used when regenerating one
/// wall's visible representation with per-layer miters.
#[derive(Debug, Clone)]
pub struct JoinMiterContext {
    /// Vertex index on *this* wall's axis that sits at the join (0 or last).
    pub self_end: usize,
    /// Other wall's axis polyline as 2D points (already trimmed/extended by
    /// [`super::join::join_wall_axes`]).
    pub other_axis: Vec<(f64, f64)>,
    /// Other wall's layers in stack order (index 0 = first from the reference
    /// side), including material/function for matching.
    pub other_layers: Vec<MiterLayer>,
    /// Joined end on the other wall (`Some(0|last)` for L / T-stem). `None`
    /// when the other wall is the through-wall of a T-junction (no endpoint
    /// moved); the helper then locates the segment under the join point.
    pub other_end: Option<usize>,
    pub kind: JoinKind,
    /// Axis bulges on this wall (LWPOLYLINE convention). Empty = straight.
    pub self_bulges: Vec<f64>,
    /// Axis bulges on the other wall. Empty = straight.
    pub other_bulges: Vec<f64>,
    /// This wall is the through-wall of a T-junction: compute layer cutouts
    /// instead of stem miters.
    pub as_through: bool,
}

/// For each layer of wall A, return either a miter-clipped closed footprint
/// polygon, or `None` when that layer can't be matched against wall B
/// (caller should fall back to `corner_override` for that layer).
///
/// `end_a` is the joined vertex index on A. `end_b` is `Some` for an L-join
/// or the stem of a T-join; `None` means B is the through-wall of a T.
///
/// **L-joins** (`end_b = Some`) produce a diagonal per-layer miter.
/// **T-joins** against a through-wall (`end_b = None`, or `kind = T`) butt
/// the stem **Structural** layer to the near face of the through-wall's
/// Structural partner. Other stem layers still stop on the approach outer
/// face so finishes do not tunnel to the far side. Per-layer depth is
/// available via join overrides.
pub fn mitered_layer_footprints(
    axis_a: &[(f64, f64)],
    layers_a: &[MiterLayer],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    end_b: Option<usize>,
    kind: JoinKind,
) -> Vec<Option<Vec<(f64, f64)>>> {
    mitered_layer_footprints_with_bulges(
        axis_a, layers_a, end_a, axis_b, layers_b, end_b, kind, &[], &[],
    )
}

/// Like [`mitered_layer_footprints`], offsetting layer edges with axis bulges
/// so arc walls miter on concentric arcs rather than chords.
pub fn mitered_layer_footprints_with_bulges(
    axis_a: &[(f64, f64)],
    layers_a: &[MiterLayer],
    end_a: usize,
    axis_b: &[(f64, f64)],
    layers_b: &[MiterLayer],
    end_b: Option<usize>,
    kind: JoinKind,
    bulges_a: &[f64],
    bulges_b: &[f64],
) -> Vec<Option<Vec<(f64, f64)>>> {
    if axis_a.len() < 2 || axis_b.len() < 2 || layers_a.is_empty() {
        return vec![None; layers_a.len()];
    }
    if end_a >= axis_a.len() {
        return vec![None; layers_a.len()];
    }

    let geom_a: Vec<(f64, f64)> = layers_a.iter().map(MiterLayer::as_geom).collect();
    let geom_b: Vec<(f64, f64)> = layers_b.iter().map(MiterLayer::as_geom).collect();
    let contours_a = contours_xy(axis_a, bulges_a, &geom_a);
    let contours_b = contours_xy(axis_b, bulges_b, &geom_b);
    if contours_a.is_empty() || contours_b.is_empty() {
        return vec![None; layers_a.len()];
    }

    // T against a through-wall (`end_b = None`): straight butt extension,
    // not a diagonal L-style miter. `kind` is kept in the signature for
    // call-site clarity (L vs T) even though the end marker is authoritative.
    let _ = kind;
    if end_b.is_none() {
        return t_junction_layer_footprints(
            axis_a,
            layers_a,
            end_a,
            axis_b,
            layers_b,
            &contours_a,
            &contours_b,
        );
    }

    let pairing = match_layer_indices_for_l_join(layers_a, layers_b, end_a, end_b);
    let mut out = Vec::with_capacity(layers_a.len());

    for (i, b_idx) in pairing.into_iter().enumerate() {
        let Some(j) = b_idx else {
            out.push(None);
            continue;
        };
        if i >= contours_a.len() || j >= contours_b.len() {
            out.push(None);
            continue;
        }
        let (ref a_b1, ref a_b2) = contours_a[i];
        let (ref b_b1, ref b_b2) = contours_b[j];
        match miter_one_layer(a_b1, a_b2, end_a, b_b1, b_b2, end_b, axis_a, axis_b) {
            Some(fp) if fp.len() >= 3 => out.push(Some(fp)),
            _ => out.push(None),
        }
    }
    out
}


mod overrides;
pub use overrides::*;
mod t_junction;
pub(crate) use t_junction::*;
mod layer_match;
pub use layer_match::*;
mod junction;
pub use junction::*;
mod clip;
pub use clip::*;
mod through;
pub use through::*;
mod geom;
pub use geom::*;

#[cfg(test)]
mod tests;
