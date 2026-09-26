//! `AEC_PLANE_FACET` command: interactive definition of a polygonal control plane facet.

use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};
use crate::modules::aec::engine::project::StoreyRef;
use crate::modules::aec::project::preview::regenerate_control_plane_previews;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_PLANE_FACET",
        label: "Plane facet",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/layers/panel.svg")),
        event: ModuleEvent::Command("AEC_PLANE_FACET".to_string()),
    }
}

pub struct PlaneFacetCommand {
    points: Vec<[f64; 3]>,
}

impl PlaneFacetCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            points: Vec::new(),
        }
    }

    fn finish_dispatch(&self) -> CmdResult {
        if self.points.len() < 3 {
            return CmdResult::Cancel;
        }
        let mut s = "AEC_PLANE_FACET_DO".to_string();
        for p in &self.points {
            s.push_str(&format!(" {} {} {}", p[0], p[1], p[2]));
        }
        CmdResult::Dispatch(s)
    }
}

impl CadCommand for PlaneFacetCommand {
    fn name(&self) -> &'static str {
        "AEC_PLANE_FACET"
    }

    fn prompt(&self) -> String {
        match self.points.len() {
            0 => crate::t!("AEC_PLANE_FACET: Ersten Eckpunkt des Ebenen-Polygons angeben:").into_owned(),
            1 => crate::t!("AEC_PLANE_FACET: Zweiten Eckpunkt angeben:").into_owned(),
            2 => crate::t!("AEC_PLANE_FACET: Dritten Eckpunkt angeben:").into_owned(),
            _ => crate::t!("AEC_PLANE_FACET: Nächsten Eckpunkt angeben [Eingabetaste=Abschließen]:").into_owned(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        self.points.push([pt.x, pt.y, pt.z]);
        if self.points.len() == 4 {
            self.finish_dispatch()
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.points.len() >= 3 {
            self.finish_dispatch()
        } else {
            CmdResult::Cancel
        }
    }
}

pub fn aec_plane_facet_do(
    scene: &mut Scene,
    command_line: &mut CommandLine,
    storey: Option<&mut StoreyRef>,
    args: &str,
) -> Option<ControlPlaneFacet> {
    let parts: Vec<f64> = args
        .split_whitespace()
        .filter_map(|s| s.parse::<f64>().ok())
        .collect();
    if parts.len() < 9 || parts.len() % 3 != 0 {
        command_line.push_error(crate::t!("AEC_PLANE_FACET: Mindestens 3 Raumpunkte erforderlich.").as_ref());
        return None;
    }
    let mut verts = Vec::with_capacity(parts.len() / 3);
    for chunk in parts.chunks_exact(3) {
        verts.push([chunk[0], chunk[1], chunk[2]]);
    }

    let facet_name = format!("Facet_{}", (verts[0][2] * 100.0).round() as i64);
    let facet = ControlPlaneFacet::new(facet_name, verts);

    command_line.push_info(&format!(
        "AEC_PLANE_FACET: Polygon-Ebene '{}' erstellt (Neigung: {:.1}°).",
        facet.name,
        facet.slope_degrees(),
    ));

    if let Some(s) = storey {
        // If there is an existing plane with facets, attach to it; otherwise create a new multi-facet plane
        if let Some(existing) = s.control_planes.iter_mut().find(|p| !p.facets.is_empty()) {
            existing.add_facet(facet.clone());
        } else {
            let plane_name = format!("CompositePlane_{}", (facet.origin()[2] * 100.0).round() as i64);
            let comp = ControlPlane::from_facets(plane_name, vec![facet.clone()]);
            s.control_planes.push(comp);
        }
        regenerate_control_plane_previews(scene, s);
    }

    Some(facet)
}

inventory::submit!(crate::command::CommandRegistration { names: &["AEC_PLANE_FACET"] });

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plane_facet_command_flow() {
        let mut cmd = PlaneFacetCommand::new();
        assert!(matches!(cmd.on_point(DVec3::new(0.0, 0.0, 3.0)), CmdResult::NeedPoint));
        assert!(matches!(cmd.on_point(DVec3::new(5.0, 0.0, 3.0)), CmdResult::NeedPoint));
        assert!(matches!(cmd.on_point(DVec3::new(5.0, 5.0, 4.0)), CmdResult::NeedPoint));
        let res = cmd.on_enter();
        match res {
            CmdResult::Dispatch(s) => {
                assert!(s.starts_with("AEC_PLANE_FACET_DO 0 0 3 5 0 3 5 5 4"));
            }
            _ => panic!("Expected dispatch"),
        }
    }

    #[test]
    fn test_plane_facet_do_adds_to_storey() {
        let mut scene = Scene::new();
        let mut cl = CommandLine::new();
        let mut storey = StoreyRef::new("Storey", 0.0, "dwg");
        let facet = aec_plane_facet_do(
            &mut scene,
            &mut cl,
            Some(&mut storey),
            "0.0 0.0 3.0 5.0 0.0 3.0 5.0 5.0 3.0 0.0 5.0 3.0",
        )
        .expect("facet created");
        assert_eq!(facet.vertices.len(), 4);
        assert_eq!(storey.control_planes.len(), 3);
        assert_eq!(storey.control_planes.last().unwrap().facets.len(), 1);
    }
}
