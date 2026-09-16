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

/// A named plane in world space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlPlane {
    pub id: Uuid,
    pub name: String,
    pub origin: [f64; 3],
    /// Unit normal; deserialized values are renormalized on use.
    pub normal: [f64; 3],
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
            face_handle: None,
            preview_handle: None,
            visible: true,
        }
    }

    pub fn horizontal(name: impl Into<String>, z: f64) -> Self {
        Self::new(name, [DEFAULT_PREVIEW_ORIGIN_XY, DEFAULT_PREVIEW_ORIGIN_XY, z], [0.0, 0.0, 1.0])
    }

    pub fn unit_normal(&self) -> [f64; 3] {
        unit_or_up(self.normal)
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

    /// Z of the plane at world XY, if the plane is not vertical.
    pub fn z_at_xy(&self, x: f64, y: f64) -> Option<f64> {
        let n = self.unit_normal();
        if n[2].abs() < EPS {
            return None;
        }
        let d = n[0] * (x - self.origin[0]) + n[1] * (y - self.origin[1]);
        Some(self.origin[2] - d / n[2])
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
    // Horizontal: +X then +Y so origin is the SW (lower-left) corner.
    let (mut u, mut v) = if n[2].abs() >= 0.9 {
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
    let o = if n[2].abs() >= 0.9 {
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
}
