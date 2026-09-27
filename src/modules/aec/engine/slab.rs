//! Slab domain model and justification/layer calculations.
//!
//! A [`Slab`] represents a parametric floor, ceiling, or roof plate. It carries
//! a multi-layer composition, reference storey/plane binding, justification
//! (OKFF, OKRD, UKD), derived child entities (2D contours/hatches, 3D solids),
//! and associated opening handles.

use acadrust::Handle;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::control_plane::{ControlPlane, ControlPlaneFacet};
use super::display_component::ComponentStyleOverride;
use super::geometry::{area, perimeter, volume, Polygon2D};
use super::plan_view::PlanPhase;
use super::project::{ProjectFile, StoreyRef};
use super::wall_style::LayerFunction;

/// Vertical reference justification for a slab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SlabJustification {
    /// Top of finished floor (OKFF - Oberkante Fertigfußboden).
    /// Reference Z aligns with the top surface of the uppermost layer.
    #[default]
    Top,
    /// Top of structural core slab (OKRD - Oberkante Rohdecke).
    /// Reference Z aligns with the top surface of the topmost `Structural` layer.
    StructuralTop,
    /// Bottom of slab (UKD - Unterkante Decke).
    /// Reference Z aligns with the bottom surface of the lowest layer.
    Bottom,
}

impl SlabJustification {
    pub fn as_str(self) -> &'static str {
        match self {
            SlabJustification::Top => "Top",
            SlabJustification::StructuralTop => "StructuralTop",
            SlabJustification::Bottom => "Bottom",
        }
    }

    pub fn from_str(s: &str) -> Self {
        let trimmed = s.trim();
        let upper = trimmed.to_ascii_uppercase();
        if trimmed.eq_ignore_ascii_case("Top")
            || upper.starts_with("OKFF")
            || trimmed == Self::Top.display_name()
        {
            return SlabJustification::Top;
        }
        if trimmed.eq_ignore_ascii_case("StructuralTop")
            || trimmed.eq_ignore_ascii_case("structural_top")
            || upper.starts_with("OKRD")
            || trimmed == Self::StructuralTop.display_name()
        {
            return SlabJustification::StructuralTop;
        }
        if trimmed.eq_ignore_ascii_case("Bottom")
            || upper.starts_with("UKD")
            || trimmed == Self::Bottom.display_name()
        {
            return SlabJustification::Bottom;
        }
        SlabJustification::Top
    }

    pub fn display_name(self) -> &'static str {
        match self {
            SlabJustification::Top => "OKFF (Oberkante Fertigfußboden)",
            SlabJustification::StructuralTop => "OKRD (Oberkante Rohdecke)",
            SlabJustification::Bottom => "UKD (Unterkante Decke)",
        }
    }

    pub fn display_short(self) -> &'static str {
        match self {
            SlabJustification::Top => "OKFF",
            SlabJustification::StructuralTop => "OKRD",
            SlabJustification::Bottom => "UKD",
        }
    }

    pub fn next(self) -> Self {
        match self {
            SlabJustification::Top => SlabJustification::StructuralTop,
            SlabJustification::StructuralTop => SlabJustification::Bottom,
            SlabJustification::Bottom => SlabJustification::Top,
        }
    }

    /// Offset from reference plane Z to the top surface of the uppermost layer.
    ///
    /// - `Top` (OKFF): top is at Z (offset = 0.0)
    /// - `Bottom` (UKD): bottom is at Z, so top is at Z + total_thickness
    /// - `StructuralTop` (OKRD): structural top is at Z, so top is at Z + (thickness of layers above structural core)
    pub fn top_offset_from_ref(self, layers: &[SlabLayer]) -> f64 {
        let total: f64 = layers.iter().map(|l| l.thickness).sum();
        match self {
            SlabJustification::Top => 0.0,
            SlabJustification::Bottom => total,
            SlabJustification::StructuralTop => {
                let mut above_structural = 0.0;
                let mut found_structural = false;
                for l in layers {
                    if l.function == LayerFunction::Structural {
                        found_structural = true;
                        break;
                    }
                    above_structural += l.thickness;
                }
                if found_structural {
                    above_structural
                } else {
                    0.0
                }
            }
        }
    }
}

/// A single material layer in a parametric slab instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlabLayer {
    /// Material identifier or display name.
    pub material: String,
    /// Layer thickness in meters.
    pub thickness: f64,
    /// Functional category of this layer (Structural, Insulation, Finish, Other).
    pub function: LayerFunction,
    /// Vertical offset in meters (relative to layer stack position).
    #[serde(default)]
    pub vertical_offset: f64,
    /// Optional CAD layer override for derived entities.
    #[serde(default)]
    pub layer_override: Option<String>,
    /// Optional hatch pattern override for cross-sections.
    #[serde(default)]
    pub hatch_override: Option<String>,
    /// Optional role tag.
    #[serde(default)]
    pub role_tag: Option<String>,
    /// Stable layer UUID.
    #[serde(default = "Uuid::new_v4")]
    pub layer_id: Uuid,
}

impl SlabLayer {
    pub fn new(material: impl Into<String>, thickness: f64, function: LayerFunction) -> Self {
        Self {
            material: material.into(),
            thickness,
            function,
            vertical_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            role_tag: None,
            layer_id: Uuid::new_v4(),
        }
    }

    pub fn is_structural(&self) -> bool {
        self.function == LayerFunction::Structural
    }
}

/// Parametric slab entity model.
#[derive(Debug, Clone, PartialEq)]
pub struct Slab {
    /// Slab style identifier (e.g. `"style_slab_concrete_20"`).
    pub style_id: String,
    /// Associated storey ID (1-based index).
    pub storey_id: u32,
    /// Layer snapshot at creation/regeneration time.
    pub layers: Vec<SlabLayer>,
    /// Handles of 2D/3D derived child entities.
    pub derived_handles: Vec<Handle>,
    /// Handles of child [`SlabOpening`] entities hosted in this slab.
    pub opening_handles: Vec<Handle>,
    /// Vertical justification relative to storey/plane level.
    pub justification: SlabJustification,
    /// Planning phase (New, Existing, Demolish).
    pub phase: PlanPhase,
    /// Hatch override for section representations.
    pub hatch_override: Option<ComponentStyleOverride>,

    /// Attached base control plane UUID (if any).
    pub base_plane_id: Option<Uuid>,
    /// Attached top control plane UUID (if any).
    pub top_plane_id: Option<Uuid>,
    /// Attached base control plane display name.
    pub base_plane_name: Option<String>,
    /// Attached top control plane display name.
    pub top_plane_name: Option<String>,
    /// Base vertical offset in meters.
    pub base_offset: f64,
    /// Top vertical offset in meters.
    pub top_offset: f64,

    /// Baked base plane origin point `[x, y, z]`.
    pub base_origin: [f64; 3],
    /// Baked base plane unit normal `[nx, ny, nz]`.
    pub base_normal: [f64; 3],
    /// Baked top plane origin point `[x, y, z]`.
    pub top_origin: [f64; 3],
    /// Baked top plane unit normal `[nx, ny, nz]`.
    pub top_normal: [f64; 3],

    /// Baked base facet polygons for sloped/faceted control planes.
    pub base_facets: Vec<ControlPlaneFacet>,
    /// Baked top facet polygons for sloped/faceted control planes.
    pub top_facets: Vec<ControlPlaneFacet>,
}

impl Slab {
    pub fn new(style_id: impl Into<String>, storey_id: u32) -> Self {
        Self {
            style_id: style_id.into(),
            storey_id,
            layers: Vec::new(),
            derived_handles: Vec::new(),
            opening_handles: Vec::new(),
            justification: SlabJustification::Top,
            phase: PlanPhase::New,
            hatch_override: None,
            base_plane_id: None,
            top_plane_id: None,
            base_plane_name: None,
            top_plane_name: None,
            base_offset: 0.0,
            top_offset: 0.0,
            base_origin: [0.0, 0.0, 0.0],
            base_normal: [0.0, 0.0, 1.0],
            top_origin: [0.0, 0.0, 0.0],
            top_normal: [0.0, 0.0, 1.0],
            base_facets: Vec::new(),
            top_facets: Vec::new(),
        }
    }

    /// Total composite slab thickness in meters across all layers.
    pub fn total_thickness(&self) -> f64 {
        self.layers.iter().map(|l| l.thickness).sum()
    }

    /// Combined thickness of all `Structural` layers in meters.
    pub fn structural_thickness(&self) -> f64 {
        self.layers
            .iter()
            .filter(|l| l.is_structural())
            .map(|l| l.thickness)
            .sum()
    }

    /// Index of the first `Structural` layer, if any.
    pub fn structural_layer_index(&self) -> Option<usize> {
        self.layers.iter().position(|l| l.is_structural())
    }

    /// Offset from slab top surface down to the top surface of the structural core.
    pub fn structural_top_offset(&self) -> f64 {
        let mut sum = 0.0;
        for l in &self.layers {
            if l.is_structural() {
                break;
            }
            sum += l.thickness;
        }
        sum
    }

    /// Offset from reference plane Z to slab top surface.
    pub fn top_offset_from_ref(&self) -> f64 {
        self.justification.top_offset_from_ref(&self.layers)
    }

    /// Offset from reference plane Z to slab bottom surface.
    pub fn bottom_offset_from_ref(&self) -> f64 {
        self.top_offset_from_ref() - self.total_thickness()
    }

    /// Computes `(top_offset, bottom_offset)` relative to reference Z for each layer
    /// in order from top layer (index 0) to bottom layer.
    pub fn layer_z_offsets(&self) -> Vec<(f64, f64)> {
        let mut offsets = Vec::with_capacity(self.layers.len());
        let mut current_top = self.top_offset_from_ref();
        for l in &self.layers {
            let layer_top = current_top + l.vertical_offset;
            let layer_bottom = layer_top - l.thickness;
            offsets.push((layer_top, layer_bottom));
            current_top -= l.thickness;
        }
        offsets
    }

    /// Computes `(layer_top_z, layer_bottom_z)` in world elevation for each layer
    /// given a reference elevation `ref_z`.
    pub fn layer_z_ranges(&self, ref_z: f64) -> Vec<(f64, f64)> {
        self.layer_z_offsets()
            .into_iter()
            .map(|(top_off, bot_off)| (ref_z + top_off, ref_z + bot_off))
            .collect()
    }

    /// Binds this slab to a storey reference elevation and bakes default horizontal planes.
    pub fn bind_storey_planes(&mut self, storey: &StoreyRef, x: f64, y: f64) {
        self.rebake_planes(storey, x, y);
    }

    /// Rebakes plane snapshots from `storey`.
    pub fn rebake_planes(&mut self, storey: &StoreyRef, x: f64, y: f64) {
        let ref_z = storey.elevation;
        if self.top_plane_id.is_none() {
            self.top_origin = [x, y, ref_z];
            self.top_normal = [0.0, 0.0, 1.0];
        }
        if self.base_plane_id.is_none() {
            self.base_origin = [x, y, ref_z - self.total_thickness()];
            self.base_normal = [0.0, 0.0, 1.0];
        }
    }

    /// Rebakes plane attachments by looking up control plane IDs in a project file.
    pub fn rebake_from_project(&mut self, project: &ProjectFile, x: f64, y: f64) {
        self.rebake_lookup(|id| project.control_plane(id).cloned(), x, y);
    }

    /// Generic rebake implementation given a control plane lookup closure.
    pub fn rebake_lookup(&mut self, lookup: impl Fn(Uuid) -> Option<ControlPlane>, x: f64, y: f64) {
        if let Some(top_id) = self.top_plane_id {
            if let Some(cp) = lookup(top_id) {
                self.top_plane_name = Some(cp.name.clone());
                self.top_facets = cp.facets.clone();
                if let Some(facet) = cp.facet_at_xy(x, y).or_else(|| cp.facets.first()) {
                    self.top_origin = facet.origin();
                    self.top_normal = facet.unit_normal();
                }
            }
        }
        if let Some(base_id) = self.base_plane_id {
            if let Some(cp) = lookup(base_id) {
                self.base_plane_name = Some(cp.name.clone());
                self.base_facets = cp.facets.clone();
                if let Some(facet) = cp.facet_at_xy(x, y).or_else(|| cp.facets.first()) {
                    self.base_origin = facet.origin();
                    self.base_normal = facet.unit_normal();
                }
            }
        }
    }

    /// Evaluates top surface elevation Z at (x, y) taking planes and offsets into account.
    pub fn top_z_at_xy(&self, x: f64, y: f64, default_ref_z: f64) -> f64 {
        let plane_z = if let Some(z) = self.eval_facets_or_plane(&self.top_facets, self.top_origin, self.top_normal, x, y) {
            z
        } else {
            default_ref_z
        };
        plane_z + self.top_offset_from_ref() + self.top_offset
    }

    /// Evaluates bottom surface elevation Z at (x, y) taking planes and offsets into account.
    pub fn bottom_z_at_xy(&self, x: f64, y: f64, default_ref_z: f64) -> f64 {
        if self.base_plane_id.is_some() || !self.base_facets.is_empty() {
            if let Some(z) = self.eval_facets_or_plane(&self.base_facets, self.base_origin, self.base_normal, x, y) {
                return z + self.base_offset;
            }
        }
        self.top_z_at_xy(x, y, default_ref_z) - self.total_thickness() + self.base_offset
    }

    fn eval_facets_or_plane(
        &self,
        facets: &[ControlPlaneFacet],
        origin: [f64; 3],
        normal: [f64; 3],
        x: f64,
        y: f64,
    ) -> Option<f64> {
        for facet in facets {
            if facet.contains_xy(x, y) {
                if let Some(z) = facet.z_at_xy(x, y) {
                    return Some(z);
                }
            }
        }
        if let Some(first) = facets.first() {
            if let Some(z) = first.z_at_xy(x, y) {
                return Some(z);
            }
        }
        if normal[2].abs() > 1e-9 {
            let d = normal[0] * (x - origin[0]) + normal[1] * (y - origin[1]);
            Some(origin[2] - d / normal[2])
        } else {
            None
        }
    }

    /// 2D area of a closed boundary polygon in square meters.
    pub fn area(points: &Polygon2D) -> f64 {
        area(points)
    }

    /// 2D perimeter of a closed boundary polygon in meters.
    pub fn perimeter(points: &Polygon2D) -> f64 {
        perimeter(points)
    }

    /// Gross volume in cubic meters for this slab with given boundary points.
    pub fn volume(&self, points: &Polygon2D) -> f64 {
        volume(points, self.total_thickness())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_layers() -> Vec<SlabLayer> {
        vec![
            SlabLayer::new("Tiles", 0.02, LayerFunction::Finish),
            SlabLayer::new("Screed", 0.06, LayerFunction::Other("Screed".to_string())),
            SlabLayer::new("Insulation", 0.04, LayerFunction::Insulation),
            SlabLayer::new("Concrete", 0.20, LayerFunction::Structural),
            SlabLayer::new("Plaster", 0.01, LayerFunction::Finish),
        ]
    }

    #[test]
    fn test_total_and_structural_thickness() {
        let mut slab = Slab::new("test_slab", 1);
        slab.layers = test_layers();

        assert!((slab.total_thickness() - 0.33).abs() < 1e-6);
        assert!((slab.structural_thickness() - 0.20).abs() < 1e-6);
        assert_eq!(slab.structural_layer_index(), Some(3));
        assert!((slab.structural_top_offset() - 0.12).abs() < 1e-6);
    }

    #[test]
    fn test_justifications() {
        let mut slab = Slab::new("test_slab", 1);
        slab.layers = test_layers();

        // Top / OKFF
        slab.justification = SlabJustification::Top;
        assert_eq!(slab.top_offset_from_ref(), 0.0);
        assert!((slab.bottom_offset_from_ref() - (-0.33)).abs() < 1e-6);

        // StructuralTop / OKRD
        slab.justification = SlabJustification::StructuralTop;
        assert!((slab.top_offset_from_ref() - 0.12).abs() < 1e-6);
        assert!((slab.bottom_offset_from_ref() - (-0.21)).abs() < 1e-6);

        // Bottom / UKD
        slab.justification = SlabJustification::Bottom;
        assert!((slab.top_offset_from_ref() - 0.33).abs() < 1e-6);
        assert_eq!(slab.bottom_offset_from_ref(), 0.0);
    }

    #[test]
    fn test_layer_z_ranges() {
        let mut slab = Slab::new("test_slab", 1);
        slab.layers = test_layers();
        slab.justification = SlabJustification::Top;

        let ranges = slab.layer_z_ranges(3.0);
        assert_eq!(ranges.len(), 5);

        // Layer 0: Tiles (0.02) -> [3.0, 2.98]
        assert!((ranges[0].0 - 3.00).abs() < 1e-6);
        assert!((ranges[0].1 - 2.98).abs() < 1e-6);

        // Layer 1: Screed (0.06) -> [2.98, 2.92]
        assert!((ranges[1].0 - 2.98).abs() < 1e-6);
        assert!((ranges[1].1 - 2.92).abs() < 1e-6);

        // Layer 2: Insulation (0.04) -> [2.92, 2.88]
        assert!((ranges[2].0 - 2.92).abs() < 1e-6);
        assert!((ranges[2].1 - 2.88).abs() < 1e-6);

        // Layer 3: Concrete (0.20) -> [2.88, 2.68]
        assert!((ranges[3].0 - 2.88).abs() < 1e-6);
        assert!((ranges[3].1 - 2.68).abs() < 1e-6);

        // Layer 4: Plaster (0.01) -> [2.68, 2.67]
        assert!((ranges[4].0 - 2.68).abs() < 1e-6);
        assert!((ranges[4].1 - 2.67).abs() < 1e-6);
    }

    #[test]
    fn test_polygon_geometry_helpers() {
        let pts = [(0.0, 0.0), (10.0, 0.0), (10.0, 5.0), (0.0, 5.0)];
        let mut slab = Slab::new("test_slab", 1);
        slab.layers = vec![SlabLayer::new("Concrete", 0.20, LayerFunction::Structural)];

        assert_eq!(Slab::area(&pts), 50.0);
        assert_eq!(Slab::perimeter(&pts), 30.0);
        assert!((slab.volume(&pts) - 10.0).abs() < 1e-6);
    }
}
