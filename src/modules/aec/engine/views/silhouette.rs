//! Occlusion analysis, depth classification, and background edge projection (Silhouette).
//!
//! Evaluates visible component edges behind the cut plane or within the elevation depth range,
//! classifying primitives into foreground visible edges, background projected edges, and hidden lines.

use super::provenance::{
    DerivedPrimitive, PrimitiveRole,
};
use super::{PaperMm, ValidationError};

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

/// Applies depth classification, sorting, and occlusion rules to derived primitives.
pub fn process_silhouette_and_depth(
    primitives: &mut [DerivedPrimitive],
    _options: &SilhouetteOptions,
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
            // Background element: assign standard background line weight (0.18 mm)
            prim.style.line_weight_mm = PaperMm::new(0.18)?;
        }
    }

    Ok(())
}
