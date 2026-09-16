//! Phase wall hatch families so the pattern origin is WCS (0, 0).
//! `pack_wall_ring` still anchors the GPU boundary at the first ring vertex.

use crate::scene::model::hatch_model::PatFamily;

/// Shift PAT family origins so GPU local coords (relative to `world_origin`)
/// match a pattern defined in world XY with origin at (0, 0).
pub fn phase_families_wcs0(families: &[PatFamily], world_origin: [f64; 2]) -> Vec<PatFamily> {
    let ox = world_origin[0] as f32;
    let oy = world_origin[1] as f32;
    families
        .iter()
        .map(|f| {
            let mut g = f.clone();
            g.x0 -= ox;
            g.y0 -= oy;
            g
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_rings_share_wcs0_phase() {
        let fam = PatFamily {
            angle_deg: 0.0,
            x0: 0.0,
            y0: 0.0,
            dx: 0.0,
            dy: 0.2,
            dashes: Vec::new(),
        };
        let a = phase_families_wcs0(&[fam.clone()], [10.0, 0.0]);
        let b = phase_families_wcs0(&[fam], [20.0, 5.0]);
        assert!((a[0].x0 + 10.0).abs() < 1e-5);
        assert!((b[0].x0 + 20.0).abs() < 1e-5);
        assert!((b[0].y0 + 5.0).abs() < 1e-5);
    }
}
