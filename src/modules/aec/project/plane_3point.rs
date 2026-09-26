//! `AEC_PLANE_3POINT` command: interactive definition of a control plane via 3 points.

use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::modules::aec::engine::control_plane::ControlPlane;
use crate::modules::aec::engine::project::StoreyRef;
use crate::modules::aec::project::preview::regenerate_control_plane_previews;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_PLANE_3POINT",
        label: "3-point plane",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/layers/panel.svg")),
        event: ModuleEvent::Command("AEC_PLANE_3POINT".to_string()),
    }
}

pub struct Plane3PointCommand {
    points: Vec<[f64; 3]>,
}

impl Plane3PointCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            points: Vec::with_capacity(3),
        }
    }
}

impl CadCommand for Plane3PointCommand {
    fn name(&self) -> &'static str {
        "AEC_PLANE_3POINT"
    }

    fn prompt(&self) -> String {
        match self.points.len() {
            0 => crate::t!("AEC_PLANE_3POINT: Ersten Punkt angeben:").into_owned(),
            1 => crate::t!("AEC_PLANE_3POINT: Zweiten Punkt angeben:").into_owned(),
            _ => crate::t!("AEC_PLANE_3POINT: Dritten Punkt angeben:").into_owned(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        self.points.push([pt.x, pt.y, pt.z]);
        if self.points.len() >= 3 {
            CmdResult::Dispatch(format!(
                "AEC_PLANE_3POINT_DO {} {} {} {} {} {} {} {} {}",
                self.points[0][0], self.points[0][1], self.points[0][2],
                self.points[1][0], self.points[1][1], self.points[1][2],
                self.points[2][0], self.points[2][1], self.points[2][2],
            ))
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

pub fn aec_plane_3point_do(
    scene: &mut Scene,
    command_line: &mut CommandLine,
    storey: Option<&mut StoreyRef>,
    args: &str,
) -> Option<ControlPlane> {
    let parts: Vec<f64> = args
        .split_whitespace()
        .filter_map(|s| s.parse::<f64>().ok())
        .collect();
    if parts.len() < 9 {
        command_line.push_error(crate::t!("AEC_PLANE_3POINT: Ungültige Koordinaten.").as_ref());
        return None;
    }
    let p1 = [parts[0], parts[1], parts[2]];
    let p2 = [parts[3], parts[4], parts[5]];
    let p3 = [parts[6], parts[7], parts[8]];

    let name = format!("Plane_3Pt_{}", (p1[2] * 100.0).round() as i64);
    let plane = match ControlPlane::from_three_points(name, p1, p2, p3) {
        Some(p) => p,
        None => {
            command_line.push_error(crate::t!("AEC_PLANE_3POINT: Die 3 Punkte sind kollinear oder identisch.").as_ref());
            return None;
        }
    };

    command_line.push_info(&format!(
        "AEC_PLANE_3POINT: Ebene '{}' erstellt (Neigung: {:.1}°, Normale: [{:.2}, {:.2}, {:.2}]).",
        plane.name,
        plane.slope_degrees(),
        plane.normal[0],
        plane.normal[1],
        plane.normal[2],
    ));

    if let Some(s) = storey {
        s.control_planes.push(plane.clone());
        regenerate_control_plane_previews(scene, s);
    }
    Some(plane)
}

inventory::submit!(crate::command::CommandRegistration { names: &["AEC_PLANE_3POINT"] });

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plane_3point_command_points_flow() {
        let mut cmd = Plane3PointCommand::new();
        assert!(matches!(cmd.on_point(DVec3::new(0.0, 0.0, 0.0)), CmdResult::NeedPoint));
        assert!(matches!(cmd.on_point(DVec3::new(5.0, 0.0, 0.0)), CmdResult::NeedPoint));
        let res = cmd.on_point(DVec3::new(0.0, 5.0, 2.5));
        match res {
            CmdResult::Dispatch(s) => {
                assert!(s.starts_with("AEC_PLANE_3POINT_DO 0 0 0 5 0 0 0 5 2.5"));
            }
            _ => panic!("Expected dispatch"),
        }
    }

    #[test]
    fn test_plane_3point_do_creates_sloped_plane() {
        let mut scene = Scene::new();
        let mut cl = CommandLine::new();
        let mut storey = StoreyRef::new("Storey", 0.0, "dwg");
        let plane = aec_plane_3point_do(
            &mut scene,
            &mut cl,
            Some(&mut storey),
            "0.0 0.0 3.0 10.0 0.0 3.0 0.0 10.0 6.0",
        )
        .expect("plane created");
        assert!(plane.is_sloped());
        assert!((plane.slope_degrees() - 16.699).abs() < 0.1);
        assert_eq!(storey.control_planes.len(), 3); // floor, ceiling + new plane
    }
}
