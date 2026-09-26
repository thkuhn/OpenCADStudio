//! Free 3D control planes (origin + unit normal) used as storey floor/ceiling
//! references and as wall base/top attachments.
//!
//! Intersection with a vertical wall axis is evaluated at a given XY point
//! (typically the wall start). Parallel / no-hit cases return `None` so the
//! caller can keep a baked snapshot.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

const EPS: f64 = 1e-9;

/// Default preview rectangle: origin at (−5 m, −5 m), 50 × 50 m.
pub const DEFAULT_PREVIEW_ORIGIN_XY: f64 = -5.0;
pub const DEFAULT_PREVIEW_SIZE: f64 = 50.0;

/// A planar polygonal facet / patch belonging to a composite control plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlPlaneFacet {
    #[serde(default)]
    pub name: String,
    /// 3D polygon vertices defining this facet (at least 3 vertices).
    pub vertices: Vec<[f64; 3]>,
    /// Optional preview entity handle in the CAD scene.
    #[serde(default)]
    pub preview_handle: Option<u64>,
}

impl ControlPlaneFacet {
    pub fn new(name: impl Into<String>, vertices: Vec<[f64; 3]>) -> Self {
        Self {
            name: name.into(),
            vertices,
            preview_handle: None,
        }
    }

    pub fn origin(&self) -> [f64; 3] {
        self.vertices.first().copied().unwrap_or([0.0, 0.0, 0.0])
    }

    /// Unit normal oriented with Z >= 0.
    pub fn unit_normal(&self) -> [f64; 3] {
        if self.vertices.len() < 3 {
            return [0.0, 0.0, 1.0];
        }
        let p1 = self.vertices[0];
        let p2 = self.vertices[1];
        for p3 in &self.vertices[2..] {
            let v1 = [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]];
            let v2 = [p3[0] - p1[0], p3[1] - p1[1], p3[2] - p1[2]];
            let mut n = [
                v1[1] * v2[2] - v1[2] * v2[1],
                v1[2] * v2[0] - v1[0] * v2[2],
                v1[0] * v2[1] - v1[1] * v2[0],
            ];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len >= EPS {
                n = [n[0] / len, n[1] / len, n[2] / len];
                if n[2] < -EPS || (n[2].abs() <= EPS && (n[1] < -EPS || (n[1].abs() <= EPS && n[0] < -EPS))) {
                    n = [-n[0], -n[1], -n[2]];
                }
                return n;
            }
        }
        [0.0, 0.0, 1.0]
    }

    pub fn polygon_2d(&self) -> Vec<(f64, f64)> {
        self.vertices.iter().map(|v| (v[0], v[1])).collect()
    }

    pub fn contains_xy(&self, x: f64, y: f64) -> bool {
        let poly = self.polygon_2d();
        if poly.len() < 3 {
            return false;
        }
        crate::modules::aec::engine::elevation_cut::point_in_polygon(&poly, (x, y))
    }

    /// Evaluates Z on this facet at (x, y).
    pub fn z_at_xy(&self, x: f64, y: f64) -> Option<f64> {
        let n = self.unit_normal();
        if n[2].abs() < EPS {
            return None;
        }
        let orig = self.origin();
        let d = n[0] * (x - orig[0]) + n[1] * (y - orig[1]);
        Some(orig[2] - d / n[2])
    }

    /// Evaluates Z with normal offset at (x, y).
    pub fn z_offset_at_xy(&self, x: f64, y: f64, offset: f64) -> Option<f64> {
        let n = self.unit_normal();
        if n[2].abs() < EPS {
            return None;
        }
        let orig = self.origin();
        let off_orig = [
            orig[0] + n[0] * offset,
            orig[1] + n[1] * offset,
            orig[2] + n[2] * offset,
        ];
        let d = n[0] * (x - off_orig[0]) + n[1] * (y - off_orig[1]);
        Some(off_orig[2] - d / n[2])
    }

    /// Distance from (x, y) to the 2D bounding centroid.
    pub fn distance_to_xy(&self, x: f64, y: f64) -> f64 {
        if self.vertices.is_empty() {
            return f64::INFINITY;
        }
        let cx: f64 = self.vertices.iter().map(|v| v[0]).sum::<f64>() / self.vertices.len() as f64;
        let cy: f64 = self.vertices.iter().map(|v| v[1]).sum::<f64>() / self.vertices.len() as f64;
        (x - cx).hypot(y - cy)
    }

    pub fn slope_degrees(&self) -> f64 {
        let n = self.unit_normal();
        let nz = n[2].abs().clamp(0.0, 1.0);
        nz.acos().to_degrees()
    }

    /// Primary elevation (Z of the origin vertex).
    pub fn elevation(&self) -> f64 {
        self.origin()[2]
    }

    /// Adjusts the elevation of all vertices by the difference `new_z - current_origin_z`.
    pub fn set_elevation(&mut self, new_z: f64) {
        let cur_z = self.origin()[2];
        let delta = new_z - cur_z;
        for v in &mut self.vertices {
            v[2] += delta;
        }
    }
}

/// A named plane in world space (either infinite analytical or composed of polygonal facets).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlPlane {
    pub id: Uuid,
    pub name: String,
    pub origin: [f64; 3],
    /// Unit normal; deserialized values are renormalized on use.
    pub normal: [f64; 3],
    /// Optional polygonal facets/regions for stepped storeys, shed roofs, or piecewise planes.
    #[serde(default)]
    pub facets: Vec<ControlPlaneFacet>,
    /// Later: bind to a DWG face. Unused in this step.
    #[serde(default)]
    pub face_handle: Option<u64>,
    /// Preview mesh handle in the storey drawing, if generated.
    #[serde(default)]
    pub preview_handle: Option<u64>,
    #[serde(default = "default_visible")]
    pub visible: bool,
}

fn default_visible() -> bool {
    true
}

impl ControlPlane {
    pub fn new(name: impl Into<String>, origin: [f64; 3], normal: [f64; 3]) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            origin,
            normal: unit_or_up(normal),
            facets: Vec::new(),
            face_handle: None,
            preview_handle: None,
            visible: true,
        }
    }

    /// Create a composite control plane with multiple polygonal facets (e.g. for stepped storeys or shed roofs).
    pub fn from_facets(name: impl Into<String>, facets: Vec<ControlPlaneFacet>) -> Self {
        let origin = facets.first().map(|f| f.origin()).unwrap_or([DEFAULT_PREVIEW_ORIGIN_XY, DEFAULT_PREVIEW_ORIGIN_XY, 0.0]);
        let normal = facets.first().map(|f| f.unit_normal()).unwrap_or([0.0, 0.0, 1.0]);
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            origin,
            normal,
            facets,
            face_handle: None,
            preview_handle: None,
            visible: true,
        }
    }

    pub fn add_facet(&mut self, facet: ControlPlaneFacet) {
        self.facets.push(facet);
    }

    pub fn remove_facet(&mut self, facet_idx: usize) -> Option<ControlPlaneFacet> {
        if facet_idx < self.facets.len() {
            let removed = self.facets.remove(facet_idx);
            if let Some(first) = self.facets.first() {
                self.origin = first.origin();
                self.normal = first.unit_normal();
            }
            Some(removed)
        } else {
            None
        }
    }

    pub fn facet_at_xy(&self, x: f64, y: f64) -> Option<&ControlPlaneFacet> {
        self.facets.iter().find(|f| f.contains_xy(x, y))
    }

    /// Returns the facet with lowest evaluated Z height at (x, y) among all containing facets.
    pub fn min_facet_at_xy(&self, x: f64, y: f64) -> Option<&ControlPlaneFacet> {
        let containing: Vec<&ControlPlaneFacet> = self.facets.iter().filter(|f| f.contains_xy(x, y)).collect();
        if !containing.is_empty() {
            containing
                .into_iter()
                .min_by(|a, b| {
                    let za = a.z_at_xy(x, y).unwrap_or(f64::INFINITY);
                    let zb = b.z_at_xy(x, y).unwrap_or(f64::INFINITY);
                    za.total_cmp(&zb)
                })
        } else {
            None
        }
    }

    pub fn closest_facet(&self, x: f64, y: f64) -> Option<&ControlPlaneFacet> {
        self.facets
            .iter()
            .min_by(|a, b| a.distance_to_xy(x, y).total_cmp(&b.distance_to_xy(x, y)))
    }

    pub fn horizontal(name: impl Into<String>, z: f64) -> Self {
        Self::new(name, [DEFAULT_PREVIEW_ORIGIN_XY, DEFAULT_PREVIEW_ORIGIN_XY, z], [0.0, 0.0, 1.0])
    }

    pub fn unit_normal(&self) -> [f64; 3] {
        unit_or_up(self.normal)
    }

    /// Create a plane from 3 non-collinear 3D points.
    /// The normal is oriented upwards (Z >= 0).
    pub fn from_three_points(
        name: impl Into<String>,
        p1: [f64; 3],
        p2: [f64; 3],
        p3: [f64; 3],
    ) -> Option<Self> {
        let v1 = [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]];
        let v2 = [p3[0] - p1[0], p3[1] - p1[1], p3[2] - p1[2]];
        let mut n = [
            v1[1] * v2[2] - v1[2] * v2[1],
            v1[2] * v2[0] - v1[0] * v2[2],
            v1[0] * v2[1] - v1[1] * v2[0],
        ];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len < EPS {
            return None;
        }
        n = [n[0] / len, n[1] / len, n[2] / len];
        if n[2] < -EPS || (n[2].abs() <= EPS && (n[1] < -EPS || (n[1].abs() <= EPS && n[0] < -EPS))) {
            n = [-n[0], -n[1], -n[2]];
        }
        Some(Self::new(name, p1, n))
    }

    /// Create a plane from origin point, pitch angle (in degrees above horizontal)
    /// and azimuth direction angle (in radians counter-clockwise from +X).
    pub fn from_slope(
        name: impl Into<String>,
        origin: [f64; 3],
        pitch_degrees: f64,
        azimuth_rad: f64,
    ) -> Self {
        if pitch_degrees.abs() < 1e-7 {
            return Self::new(name, origin, [0.0, 0.0, 1.0]);
        }
        let pitch_rad = pitch_degrees.to_radians();
        let cos_p = pitch_rad.cos();
        let sin_p = pitch_rad.sin();
        let cos_a = azimuth_rad.cos();
        let sin_a = azimuth_rad.sin();
        let normal = [-cos_a * sin_p, -sin_a * sin_p, cos_p];
        Self::new(name, origin, normal)
    }

    /// Slope angle in degrees relative to the horizontal XY plane (0° = flat).
    pub fn slope_degrees(&self) -> f64 {
        if !self.facets.is_empty() {
            let max_slope = self.facets.iter().map(|f| f.slope_degrees()).fold(0.0, f64::max);
            max_slope
        } else {
            let n = self.unit_normal();
            let nz = n[2].abs().clamp(0.0, 1.0);
            nz.acos().to_degrees()
        }
    }

    /// Direction of steepest ascent in radians counter-clockwise from +X [0, 2π).
    pub fn slope_azimuth_rad(&self) -> f64 {
        let n = self.unit_normal();
        let sign = if n[2] < 0.0 { -1.0 } else { 1.0 };
        let nx = n[0] * sign;
        let ny = n[1] * sign;
        if nx.hypot(ny) < 1e-9 {
            0.0
        } else {
            let mut a = (-ny).atan2(-nx);
            if a < 0.0 {
                a += 2.0 * std::f64::consts::PI;
            }
            a
        }
    }

    /// True if the plane's normal deviates from pure vertical (horizontal plane) or has sloped facets.
    pub fn is_sloped(&self) -> bool {
        let n = self.unit_normal();
        if (n[2].abs() - 1.0).abs() > 1e-5 {
            return true;
        }
        self.facets.iter().any(|f| f.slope_degrees() > 1e-4)
    }

    /// Offset along the plane normal.
    pub fn offset(&self, distance: f64) -> Self {
        let n = self.unit_normal();
        let mut clone = self.clone();
        clone.origin = [
            self.origin[0] + n[0] * distance,
            self.origin[1] + n[1] * distance,
            self.origin[2] + n[2] * distance,
        ];
        clone
    }

    /// Z of the plane at world XY, taking multi-polygon facets into account if present.
    /// When multiple facets contain (x, y), the minimum Z height among them is evaluated.
    pub fn z_at_xy(&self, x: f64, y: f64) -> Option<f64> {
        if self.facets.is_empty() {
            let n = self.unit_normal();
            if n[2].abs() < EPS {
                return None;
            }
            let d = n[0] * (x - self.origin[0]) + n[1] * (y - self.origin[1]);
            Some(self.origin[2] - d / n[2])
        } else {
            let containing: Vec<&ControlPlaneFacet> = self.facets.iter().filter(|f| f.contains_xy(x, y)).collect();
            if !containing.is_empty() {
                containing
                    .into_iter()
                    .filter_map(|f| f.z_at_xy(x, y))
                    .min_by(f64::total_cmp)
            } else {
                None
            }
        }
    }

    /// Evaluates the Z-height of the plane shifted by `offset` along its normal at `(x, y)`.
    /// When multiple facets contain (x, y), the minimum Z height among them is evaluated.
    pub fn z_offset_at_xy(&self, x: f64, y: f64, offset: f64) -> Option<f64> {
        if self.facets.is_empty() {
            self.offset(offset).z_at_xy(x, y)
        } else {
            let containing: Vec<&ControlPlaneFacet> = self.facets.iter().filter(|f| f.contains_xy(x, y)).collect();
            if !containing.is_empty() {
                containing
                    .into_iter()
                    .filter_map(|f| f.z_offset_at_xy(x, y, offset))
                    .min_by(f64::total_cmp)
            } else {
                None
            }
        }
    }
}

fn unit_or_up(n: [f64; 3]) -> [f64; 3] {
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len < EPS {
        [0.0, 0.0, 1.0]
    } else {
        [n[0] / len, n[1] / len, n[2] / len]
    }
}

/// Intersect ray `origin + t * dir` with a plane. `None` if parallel.
pub fn intersect_ray_plane(
    ray_origin: [f64; 3],
    ray_dir: [f64; 3],
    plane: &ControlPlane,
) -> Option<[f64; 3]> {
    let n = plane.unit_normal();
    let denom = n[0] * ray_dir[0] + n[1] * ray_dir[1] + n[2] * ray_dir[2];
    if denom.abs() < EPS {
        return None;
    }
    let w = [
        plane.origin[0] - ray_origin[0],
        plane.origin[1] - ray_origin[1],
        plane.origin[2] - ray_origin[2],
    ];
    let t = (n[0] * w[0] + n[1] * w[1] + n[2] * w[2]) / denom;
    Some([
        ray_origin[0] + t * ray_dir[0],
        ray_origin[1] + t * ray_dir[1],
        ray_origin[2] + t * ray_dir[2],
    ])
}

/// Vertical world-Z axis through `(x, y)` intersected with `plane`.
pub fn intersect_vertical_at_xy(x: f64, y: f64, plane: &ControlPlane) -> Option<[f64; 3]> {
    intersect_ray_plane([x, y, 0.0], [0.0, 0.0, 1.0], plane)
}

/// Wall height along world Z from base/top planes plus offsets along each
/// plane normal. Intersection is taken at `(x, y)` (wall start).
pub fn resolve_wall_height(
    x: f64,
    y: f64,
    base: &ControlPlane,
    top: &ControlPlane,
    base_offset: f64,
    top_offset: f64,
) -> Option<f64> {
    let base_pt = intersect_vertical_at_xy(x, y, &base.offset(base_offset))?;
    let top_pt = intersect_vertical_at_xy(x, y, &top.offset(top_offset))?;
    Some(top_pt[2] - base_pt[2])
}

/// Four corners of a preview rectangle of `size` starting at `origin` (SW).
pub fn preview_rectangle(plane: &ControlPlane, size: f64) -> [[f64; 3]; 4] {
    let n = plane.unit_normal();
    let is_flat = (n[2].abs() - 1.0).abs() <= 1e-4;
    // Horizontal: +X then +Y so origin is the SW (lower-left) corner.
    let (mut u, mut v) = if is_flat {
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
    } else {
        let up = [0.0, 0.0, 1.0];
        let u = [
            n[1] * up[2] - n[2] * up[1],
            n[2] * up[0] - n[0] * up[2],
            n[0] * up[1] - n[1] * up[0],
        ];
        let ulen = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt().max(EPS);
        let u = [u[0] / ulen, u[1] / ulen, u[2] / ulen];
        let v = [
            n[1] * u[2] - n[2] * u[1],
            n[2] * u[0] - n[0] * u[2],
            n[0] * u[1] - n[1] * u[0],
        ];
        (u, v)
    };
    let ulen = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt().max(EPS);
    u = [u[0] / ulen, u[1] / ulen, u[2] / ulen];
    let vlen = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(EPS);
    v = [v[0] / vlen, v[1] / vlen, v[2] / vlen];
    // Horizontal previews always use SW origin (-5, -5); Z stays on the plane.
    let o = if is_flat {
        [DEFAULT_PREVIEW_ORIGIN_XY, DEFAULT_PREVIEW_ORIGIN_XY, plane.origin[2]]
    } else {
        plane.origin
    };
    let corner = |su: f64, sv: f64| {
        [
            o[0] + su * size * u[0] + sv * size * v[0],
            o[1] + su * size * u[1] + sv * size * v[1],
            o[2] + su * size * u[2] + sv * size * v[2],
        ]
    };
    [corner(0.0, 0.0), corner(1.0, 0.0), corner(1.0, 1.0), corner(0.0, 1.0)]
}

/// Default floor (z = elevation) and ceiling (z = elevation + height).
pub fn default_floor_ceiling(
    storey_name: &str,
    elevation: f64,
    height: f64,
) -> (ControlPlane, ControlPlane) {
    let floor = ControlPlane::horizontal(format!("{storey_name}_ELEVATION"), elevation);
    let ceiling = ControlPlane::horizontal(format!("{storey_name}_OKGH"), elevation + height);
    (floor, ceiling)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_intersect_is_plane_z() {
        let p = ControlPlane::horizontal("F", 3.2);
        let hit = intersect_vertical_at_xy(10.0, 4.0, &p).unwrap();
        assert!((hit[2] - 3.2).abs() < 1e-9);
        assert!((hit[0] - 10.0).abs() < 1e-9);
    }

    #[test]
    fn vertical_plane_has_no_vertical_intersect() {
        let p = ControlPlane::new("V", [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        assert!(intersect_vertical_at_xy(1.0, 0.0, &p).is_none());
    }

    #[test]
    fn tilted_plane_z_varies_with_xy() {
        // n = (0, 0.6, 0.8) unit-ish; origin at 0.
        let p = ControlPlane::new("T", [0.0, 0.0, 0.0], [0.0, 0.6, 0.8]);
        let z0 = p.z_at_xy(0.0, 0.0).unwrap();
        let z1 = p.z_at_xy(0.0, 4.0).unwrap();
        assert!((z0 - 0.0).abs() < 1e-9);
        assert!(z1 < 0.0);
    }

    #[test]
    fn wall_height_horizontal_plus_offsets() {
        let base = ControlPlane::horizontal("B", 0.0);
        let top = ControlPlane::horizontal("T", 3.0);
        let h = resolve_wall_height(0.0, 0.0, &base, &top, 0.1, -0.2).unwrap();
        // base z = 0.1 (normal +Z), top z = 2.8
        assert!((h - 2.7).abs() < 1e-9);
    }

    #[test]
    fn default_planes_use_elevation_and_height() {
        let (f, c) = default_floor_ceiling("EG", 1.0, 3.2);
        assert!((f.origin[2] - 1.0).abs() < 1e-9);
        assert!((c.origin[2] - 4.2).abs() < 1e-9);
        assert!(f.name.ends_with("_ELEVATION"));
        assert!(c.name.ends_with("_OKGH"));
        assert!(!c.name.contains("UKRD"));
        assert!((f.origin[0] + 5.0).abs() < 1e-9);
        assert!((f.origin[1] + 5.0).abs() < 1e-9);
    }

    #[test]
    fn preview_rectangle_sw_origin_plus_xy_50() {
        let p = ControlPlane::horizontal("F", 0.0);
        let c = preview_rectangle(&p, DEFAULT_PREVIEW_SIZE);
        assert!((c[0][0] + 5.0).abs() < 1e-9);
        assert!((c[0][1] + 5.0).abs() < 1e-9);
        assert!((c[1][0] - 45.0).abs() < 1e-9);
        assert!((c[1][1] + 5.0).abs() < 1e-9);
        assert!((c[2][0] - 45.0).abs() < 1e-9);
        assert!((c[2][1] - 45.0).abs() < 1e-9);
        assert!((c[3][0] + 5.0).abs() < 1e-9);
        assert!((c[3][1] - 45.0).abs() < 1e-9);
    }

    #[test]
    fn three_points_defines_sloped_plane() {
        let p1 = [0.0, 0.0, 3.0];
        let p2 = [10.0, 0.0, 3.0];
        let p3 = [0.0, 10.0, 6.0]; // Slope along Y
        let plane = ControlPlane::from_three_points("Roof", p1, p2, p3).unwrap();
        assert!(plane.is_sloped());
        let z_p1 = plane.z_at_xy(0.0, 0.0).unwrap();
        let z_p2 = plane.z_at_xy(10.0, 0.0).unwrap();
        let z_p3 = plane.z_at_xy(0.0, 10.0).unwrap();
        let z_mid = plane.z_at_xy(5.0, 5.0).unwrap();
        assert!((z_p1 - 3.0).abs() < 1e-9);
        assert!((z_p2 - 3.0).abs() < 1e-9);
        assert!((z_p3 - 6.0).abs() < 1e-9);
        assert!((z_mid - 4.5).abs() < 1e-9);
    }

    #[test]
    fn collinear_points_fail_three_points() {
        let p1 = [0.0, 0.0, 0.0];
        let p2 = [1.0, 1.0, 1.0];
        let p3 = [2.0, 2.0, 2.0];
        assert!(ControlPlane::from_three_points("Bad", p1, p2, p3).is_none());
    }

    #[test]
    fn from_slope_and_properties() {
        let origin = [0.0, 0.0, 2.5];
        let pitch_deg = 30.0;
        let azimuth = 0.0; // Slope towards +X
        let plane = ControlPlane::from_slope("Pultdach", origin, pitch_deg, azimuth);
        assert!(plane.is_sloped());
        assert!((plane.slope_degrees() - 30.0).abs() < 1e-6);
        assert!((plane.slope_azimuth_rad() - 0.0).abs() < 1e-6);
        let z0 = plane.z_at_xy(0.0, 0.0).unwrap();
        let z10 = plane.z_at_xy(10.0, 0.0).unwrap();
        assert!((z0 - 2.5).abs() < 1e-9);
        let expected_rise = 10.0 * 30.0_f64.to_radians().tan();
        assert!((z10 - (2.5 + expected_rise)).abs() < 1e-6);

        // Z-offset at xy: offset by 0.2 along normal
        let z_off = plane.z_offset_at_xy(0.0, 0.0, 0.2).unwrap();
        let expected_z_off = 2.5 + 0.2 / 30.0_f64.to_radians().cos();
        assert!((z_off - expected_z_off).abs() < 1e-6);
    }

    #[test]
    fn multi_facet_staffelgeschoss_and_sheddach() {
        // Staffelgeschoss: Main area (x in 0..10, y in 0..10) has ceiling at 3.20 m,
        // terrace setback (x in 10..20, y in 0..10) has ceiling at 2.40 m.
        let facet_main = ControlPlaneFacet::new(
            "MainRoof",
            vec![
                [0.0, 0.0, 3.2],
                [10.0, 0.0, 3.2],
                [10.0, 10.0, 3.2],
                [0.0, 10.0, 3.2],
            ],
        );
        let facet_terrace = ControlPlaneFacet::new(
            "TerraceRoof",
            vec![
                [10.0, 0.0, 2.4],
                [20.0, 0.0, 2.4],
                [20.0, 10.0, 2.4],
                [10.0, 10.0, 2.4],
            ],
        );

        let composite = ControlPlane::from_facets("StaffelCeiling", vec![facet_main, facet_terrace]);
        assert_eq!(composite.facets.len(), 2);

        // Point inside main area
        let z_main = composite.z_at_xy(5.0, 5.0).unwrap();
        assert!((z_main - 3.2).abs() < 1e-9);

        // Point inside terrace area
        let z_terrace = composite.z_at_xy(15.0, 5.0).unwrap();
        assert!((z_terrace - 2.4).abs() < 1e-9);

        // Sheddach (sawtooth roof): Bay 1 (x: 0..5) slopes 30 deg; Bay 2 (x: 5..10) slopes 30 deg
        let shed1 = ControlPlaneFacet::new(
            "Shed1",
            vec![
                [0.0, 0.0, 3.0],
                [5.0, 0.0, 3.0 + 5.0 * 30.0_f64.to_radians().tan()],
                [5.0, 10.0, 3.0 + 5.0 * 30.0_f64.to_radians().tan()],
                [0.0, 10.0, 3.0],
            ],
        );
        let shed2 = ControlPlaneFacet::new(
            "Shed2",
            vec![
                [5.0, 0.0, 3.0],
                [10.0, 0.0, 3.0 + 5.0 * 30.0_f64.to_radians().tan()],
                [10.0, 10.0, 3.0 + 5.0 * 30.0_f64.to_radians().tan()],
                [5.0, 10.0, 3.0],
            ],
        );

        let sheddach = ControlPlane::from_facets("Sheddach", vec![shed1, shed2]);
        assert!(sheddach.is_sloped());
        let z_s1_start = sheddach.z_at_xy(0.0, 5.0).unwrap();
        let z_s1_mid = sheddach.z_at_xy(2.5, 5.0).unwrap();
        let z_s2_inside = sheddach.z_at_xy(6.0, 5.0).unwrap();
        let z_s2_mid = sheddach.z_at_xy(7.5, 5.0).unwrap();

        assert!((z_s1_start - 3.0).abs() < 1e-9);
        assert!((z_s1_mid - (3.0 + 2.5 * 30.0_f64.to_radians().tan())).abs() < 1e-6);
        assert!((z_s2_inside - (3.0 + 1.0 * 30.0_f64.to_radians().tan())).abs() < 1e-6);
        assert!((z_s2_mid - (3.0 + 2.5 * 30.0_f64.to_radians().tan())).abs() < 1e-6);
    }

    #[test]
    fn multi_facet_lowest_height_selected_on_overlap() {
        let f_high = ControlPlaneFacet::new(
            "HighRoof",
            vec![
                [0.0, 0.0, 4.0],
                [10.0, 0.0, 4.0],
                [10.0, 10.0, 4.0],
                [0.0, 10.0, 4.0],
            ],
        );
        let f_low = ControlPlaneFacet::new(
            "LowRoof",
            vec![
                [0.0, 0.0, 2.5],
                [10.0, 0.0, 2.5],
                [10.0, 10.0, 2.5],
                [0.0, 10.0, 2.5],
            ],
        );
        let composite = ControlPlane::from_facets("OverlapPlane", vec![f_high, f_low]);
        let min_z = composite.z_at_xy(5.0, 5.0).unwrap();
        assert_eq!(min_z, 2.5);

        let min_facet = composite.min_facet_at_xy(5.0, 5.0).unwrap();
        assert_eq!(min_facet.name, "LowRoof");
    }
}
