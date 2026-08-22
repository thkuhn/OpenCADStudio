//! `Wall` domain model.
//!
//! Mirrors the `WALL` XDATA record (APPID `OPENCAD_AEC`): style id, height,
//! storey id, multi-layer snapshot, derived representation handles, and
//! justification.

use acadrust::Handle;

/// A parametric multi-layer wall: a baseline polyline (owned by the host
/// entity, not by this struct) plus the metadata needed to extrude/render it
/// and to write the `WALL` XDATA record.
#[derive(Debug, Clone, PartialEq)]
pub struct Wall {
    pub style_id: String,
    pub height: f64,
    pub storey_id: u32,
    pub layers: Vec<WallLayer>,
    /// Handles of the contour/hatch/solid entities most recently derived
    /// from this wall's axis, so they can be cleanly replaced or removed.
    pub derived_handles: Vec<Handle>,
    /// Informational field: which justification was used when drawing.
    pub justification: WallJustification,
}

/// One material layer in a wall's cross-section snapshot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WallLayer {
    pub material: String,
    pub thickness: f64,
    pub function: String,
    pub gap_before: f64,
    pub bottom_offset: f64,
    pub top_offset: f64,
    pub layer_override: Option<String>,
    /// Optional hatch pattern override, taking precedence over the
    /// material's own hatch pattern when rendering this layer's 2D hatch.
    /// Additive field (mirrors [`crate::modules::aec::engine::wall_style::Layer::hatch_override`]).
    pub hatch_override: Option<String>,
}

/// Axis justification relative to the wall's total thickness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallJustification {
    Interior,
    Center,
    Exterior,
}

impl WallJustification {
    pub fn next(self) -> Self {
        match self {
            WallJustification::Interior => WallJustification::Center,
            WallJustification::Center => WallJustification::Exterior,
            WallJustification::Exterior => WallJustification::Interior,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            WallJustification::Interior => "Interior",
            WallJustification::Center => "Center",
            WallJustification::Exterior => "Exterior",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "Interior" => WallJustification::Interior,
            "Exterior" => WallJustification::Exterior,
            _ => WallJustification::Center,
        }
    }

    pub fn offset(&self, total_thickness: f64) -> f64 {
        match self {
            WallJustification::Center => 0.0,
            WallJustification::Interior => total_thickness * -0.5,
            WallJustification::Exterior => total_thickness * 0.5,
        }
    }
}

impl Wall {
    /// Creates a wall with no layers and center justification.
    pub fn new(style_id: impl Into<String>, height: f64, storey_id: u32) -> Self {
        Self {
            style_id: style_id.into(),
            height,
            storey_id,
            layers: Vec::new(),
            derived_handles: Vec::new(),
            justification: WallJustification::Center,
        }
    }

    /// Total thickness across all layers including gaps.
    pub fn total_thickness(&self) -> f64 {
        self.layers
            .iter()
            .map(|l| l.thickness + l.gap_before)
            .sum()
    }

    /// Volume of the wall given the length of its (2D) baseline centerline.
    /// `volume = length * total_thickness * height`.
    pub fn volume(&self, baseline_length: f64) -> f64 {
        baseline_length * self.total_thickness() * self.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_is_length_times_thickness_times_height() {
        let mut wall = Wall::new("s", 2.8, 0);
        wall.layers.push(WallLayer {
            material: "Concrete".into(),
            thickness: 0.2,
            function: "Structural".into(),
            gap_before: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
        });
        assert!((wall.volume(5.0) - 2.8).abs() < 1e-9);
    }

    #[test]
    fn new_wall_has_no_layers() {
        let wall = Wall::new("s", 2.8, 1);
        assert!(wall.layers.is_empty());
        assert_eq!(wall.storey_id, 1);
        assert_eq!(wall.justification, WallJustification::Center);
    }

    #[test]
    fn total_thickness_includes_gaps() {
        let mut wall = Wall::new("s", 3.0, 0);
        wall.layers = vec![
            WallLayer {
                material: "A".into(),
                thickness: 0.1,
                function: "Finish".into(),
                gap_before: 0.05,
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
            },
            WallLayer {
                material: "B".into(),
                thickness: 0.2,
                function: "Structural".into(),
                gap_before: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
            },
        ];
        assert!((wall.total_thickness() - 0.35).abs() < 1e-9);
    }
}
