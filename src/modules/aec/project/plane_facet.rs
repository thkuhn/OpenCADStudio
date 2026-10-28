//! `AEC_PLANE_FACET` command: interactive definition of a polygonal control plane facet.

use glam::DVec3;
use uuid::Uuid;

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

/// Parses a target string in the format `bid|sid|pid` (or `_|_|_` / empty).
/// - Returns `Ok(Some((bid, sid, pid)))` if a valid 3-part UUID target is present.
/// - Returns `Ok(None)` if input is empty, `_|_|_`, or does not contain `|`.
/// - Returns `Err(String)` if input contains `|` but fails UUID parsing or does not have exactly 3 parts.
pub fn parse_plane_target(token: &str) -> Result<Option<(Uuid, Uuid, Uuid)>, String> {
    let t = token.trim();
    if t.is_empty() || t == "_|_|_" || !t.contains('|') {
        return Ok(None);
    }
    let segs: Vec<&str> = t.split('|').collect();
    if segs.len() != 3 {
        return Err(format!("Ungültiges Ziel-Format '{t}' (erwartet: bid|sid|pid)"));
    }
    let bid = segs[0]
        .parse::<Uuid>()
        .map_err(|e| format!("Ungültige Gebäude-ID '{}': {e}", segs[0]))?;
    let sid = segs[1]
        .parse::<Uuid>()
        .map_err(|e| format!("Ungültige Geschoss-ID '{}': {e}", segs[1]))?;
    let pid = segs[2]
        .parse::<Uuid>()
        .map_err(|e| format!("Ungültige Ebenen-ID '{}': {e}", segs[2]))?;
    Ok(Some((bid, sid, pid)))
}

pub struct PlaneFacetCommand {
    target: Option<(Uuid, Uuid, Uuid)>,
    points: Vec<[f64; 3]>,
}

impl PlaneFacetCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            target: None,
            points: Vec::new(),
        }
    }

    pub fn with_target(bid: Uuid, sid: Uuid, pid: Uuid) -> Self {
        Self {
            target: Some((bid, sid, pid)),
            points: Vec::new(),
        }
    }

    pub fn prompt(&self) -> String {
        match self.points.len() {
            0 => crate::t!("AEC_PLANE_FACET: Ersten Eckpunkt des Ebenen-Polygons angeben:").into_owned(),
            1 => crate::t!("AEC_PLANE_FACET: Zweiten Eckpunkt angeben:").into_owned(),
            2 => crate::t!("AEC_PLANE_FACET: Dritten Eckpunkt angeben:").into_owned(),
            _ => crate::t!("AEC_PLANE_FACET: Nächsten Eckpunkt angeben [Eingabetaste=Abschließen]:").into_owned(),
        }
    }

    fn finish_dispatch(&self) -> CmdResult {
        if self.points.len() < 3 && self.target.is_none() {
            return CmdResult::Cancel;
        }
        let mut s = "AEC_PLANE_FACET_DO".to_string();
        if let Some((b, s_id, p)) = self.target {
            s.push_str(&format!(" {b}|{s_id}|{p}"));
        }
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
        self.prompt()
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        self.points.push([pt.x, pt.y, pt.z]);
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.points.len() >= 3 {
            self.finish_dispatch()
        } else if self.target.is_some() {
            self.points.clear();
            self.finish_dispatch()
        } else {
            CmdResult::Cancel
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        if self.target.is_some() {
            self.points.clear();
            self.finish_dispatch()
        } else {
            CmdResult::Cancel
        }
    }
}

pub fn aec_plane_facet_do(
    scene: Option<&mut Scene>,
    command_line: &mut CommandLine,
    storey: Option<&mut StoreyRef>,
    target_plane_id: Option<Uuid>,
    args: &str,
) -> Option<ControlPlaneFacet> {
    let mut target_id = target_plane_id;
    let mut parts_str = args.trim();
    if let Some(first_space) = parts_str.find(char::is_whitespace) {
        let first_token = &parts_str[..first_space];
        if first_token.contains('|') {
            match parse_plane_target(first_token) {
                Ok(Some((_b, _s, pid))) => {
                    if target_id.is_none() {
                        target_id = Some(pid);
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    command_line.push_error(&format!("AEC_PLANE_FACET: {err}"));
                    return None;
                }
            }
            parts_str = parts_str[first_space..].trim();
        }
    } else if parts_str.contains('|') {
        match parse_plane_target(parts_str) {
            Ok(Some((_b, _s, pid))) => {
                if target_id.is_none() {
                    target_id = Some(pid);
                }
            }
            Ok(None) => {}
            Err(err) => {
                command_line.push_error(&format!("AEC_PLANE_FACET: {err}"));
                return None;
            }
        }
        parts_str = "";
    }

    let Some(s) = storey else {
        command_line.push_error(
            crate::t!("AEC_PLANE_FACET: Kein Geschoss für die Facettenzuweisung gefunden.")
                .as_ref(),
        );
        return None;
    };

    if let Some(pid) = target_id {
        if s.plane(pid).is_none() {
            command_line.push_error(
                crate::t!("AEC_PLANE_FACET: Zielebene im Geschoss nicht gefunden.").as_ref(),
            );
            return None;
        }
    }

    let parts: Vec<f64> = parts_str
        .split_whitespace()
        .filter_map(|s| s.parse::<f64>().ok())
        .collect();
    if parts.len() < 9 || parts.len() % 3 != 0 {
        command_line.push_error(
            crate::t!("AEC_PLANE_FACET: Mindestens 3 Raumpunkte erforderlich.").as_ref(),
        );
        return None;
    }
    let mut verts = Vec::with_capacity(parts.len() / 3);
    for chunk in parts.chunks_exact(3) {
        verts.push([chunk[0], chunk[1], chunk[2]]);
    }

    let facet_name = format!("Facet_{}", (verts[0][2] * 100.0).round() as i64);
    let facet = ControlPlaneFacet::new(facet_name, verts);

    let plane_name: String;
    if let Some(pid) = target_id {
        let plane = s.plane_mut(pid).expect("plane was validated above");
        plane_name = plane.name.clone();
        plane.add_facet(facet.clone());
    } else {
        let comp_name = format!("CompositePlane_{}", (facet.origin()[2] * 100.0).round() as i64);
        plane_name = comp_name.clone();
        let comp = ControlPlane::from_facets(comp_name, vec![facet.clone()]);
        s.control_planes.push(comp);
    }

    if let Some(sc) = scene {
        regenerate_control_plane_previews(sc, s);
    }

    command_line.push_info(&crate::tf!(
        "AEC_PLANE_FACET: Facette der Kontrollebene '{}' hinzugefügt (Neigung: {:.1}°).",
        plane_name,
        facet.slope_degrees(),
    ));

    Some(facet)
}

inventory::submit!(crate::command::CommandRegistration { names: &["AEC_PLANE_FACET"] });

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plane_target_valid() {
        let bid = Uuid::new_v4();
        let sid = Uuid::new_v4();
        let pid = Uuid::new_v4();
        let token = format!("{bid}|{sid}|{pid}");
        let parsed = parse_plane_target(&token).expect("valid parse");
        assert_eq!(parsed, Some((bid, sid, pid)));
    }

    #[test]
    fn test_parse_plane_target_none_cases() {
        assert_eq!(parse_plane_target("").unwrap(), None);
        assert_eq!(parse_plane_target("_|_|_").unwrap(), None);
        assert_eq!(parse_plane_target("0 0 3 5 0 3").unwrap(), None);
    }

    #[test]
    fn test_parse_plane_target_errors() {
        assert!(parse_plane_target("not|valid").is_err());
        assert!(parse_plane_target("123|456|789").is_err());
        assert!(parse_plane_target("a|b|c|d").is_err());
    }

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
    fn test_plane_facet_command_arbitrary_polygon_five_points() {
        let mut cmd = PlaneFacetCommand::new();
        assert!(matches!(cmd.on_point(DVec3::new(0.0, 0.0, 3.0)), CmdResult::NeedPoint));
        assert!(matches!(cmd.on_point(DVec3::new(5.0, 0.0, 3.0)), CmdResult::NeedPoint));
        assert!(matches!(cmd.on_point(DVec3::new(5.0, 5.0, 4.0)), CmdResult::NeedPoint));
        assert!(matches!(cmd.on_point(DVec3::new(2.5, 7.0, 4.5)), CmdResult::NeedPoint));
        assert!(matches!(cmd.on_point(DVec3::new(0.0, 5.0, 4.0)), CmdResult::NeedPoint));
        let res = cmd.on_enter();
        match res {
            CmdResult::Dispatch(s) => {
                assert!(s.starts_with("AEC_PLANE_FACET_DO 0 0 3 5 0 3 5 5 4 2.5 7 4.5 0 5 4"));
            }
            _ => panic!("Expected dispatch for 5 points"),
        }
    }

    #[test]
    fn test_plane_facet_command_with_target_flow() {
        let bid = Uuid::new_v4();
        let sid = Uuid::new_v4();
        let pid = Uuid::new_v4();
        let mut cmd = PlaneFacetCommand::with_target(bid, sid, pid);
        assert!(matches!(cmd.on_point(DVec3::new(0.0, 0.0, 3.0)), CmdResult::NeedPoint));
        assert!(matches!(cmd.on_point(DVec3::new(5.0, 0.0, 3.0)), CmdResult::NeedPoint));
        assert!(matches!(cmd.on_point(DVec3::new(5.0, 5.0, 4.0)), CmdResult::NeedPoint));
        let res = cmd.on_enter();
        match res {
            CmdResult::Dispatch(s) => {
                assert!(s.starts_with(&format!("AEC_PLANE_FACET_DO {bid}|{sid}|{pid} 0 0 3 5 0 3 5 5 4")));
            }
            _ => panic!("Expected dispatch"),
        }

        // Test on_escape returns dispatch with target to reopen modal
        let mut cmd_esc = PlaneFacetCommand::with_target(bid, sid, pid);
        let res_esc = cmd_esc.on_escape();
        match res_esc {
            CmdResult::Dispatch(s) => {
                assert_eq!(s, format!("AEC_PLANE_FACET_DO {bid}|{sid}|{pid}"));
            }
            _ => panic!("Expected dispatch on escape with target"),
        }
    }

    #[test]
    fn test_plane_facet_do_adds_to_storey() {
        let mut scene = Scene::new();
        let mut cl = CommandLine::new();
        let mut storey = StoreyRef::new("Storey", 0.0, "dwg");
        let facet = aec_plane_facet_do(
            Some(&mut scene),
            &mut cl,
            Some(&mut storey),
            None,
            "0.0 0.0 3.0 5.0 0.0 3.0 5.0 5.0 3.0 0.0 5.0 3.0",
        )
        .expect("facet created");
        assert_eq!(facet.vertices.len(), 4);
        assert_eq!(storey.control_planes.len(), 3);
        assert_eq!(storey.control_planes.last().unwrap().facets.len(), 1);
    }

    #[test]
    fn test_plane_facet_do_adds_facet_to_target_plane() {
        let mut scene = Scene::new();
        let mut cl = CommandLine::new();
        let mut storey = StoreyRef::new("Storey", 0.0, "dwg");
        let ceiling_id = storey.ceiling_plane_id;
        let initial_planes_count = storey.control_planes.len();
        let initial_facets_count = storey.plane(ceiling_id).unwrap().facets.len();

        let facet = aec_plane_facet_do(
            Some(&mut scene),
            &mut cl,
            Some(&mut storey),
            Some(ceiling_id),
            "0.0 0.0 3.0 5.0 0.0 3.0 5.0 5.0 4.0 0.0 5.0 4.0",
        )
        .expect("facet created");

        assert_eq!(facet.vertices.len(), 4);
        assert_eq!(storey.control_planes.len(), initial_planes_count);
        let ceiling_plane = storey.plane(ceiling_id).unwrap();
        assert_eq!(ceiling_plane.facets.len(), initial_facets_count + 1);
        assert_eq!(ceiling_plane.facets.last().unwrap().vertices.len(), 4);
    }

    #[test]
    fn test_plane_facet_do_with_target_string_in_args() {
        let mut scene = Scene::new();
        let mut cl = CommandLine::new();
        let mut storey = StoreyRef::new("Storey", 0.0, "dwg");
        let floor_id = storey.floor_plane_id;
        let bid = Uuid::new_v4();
        let sid = storey.id;

        let args = format!("{bid}|{sid}|{floor_id} 0.0 0.0 0.0 4.0 0.0 0.0 4.0 4.0 0.5");
        let facet = aec_plane_facet_do(
            Some(&mut scene),
            &mut cl,
            Some(&mut storey),
            None,
            &args,
        )
        .expect("facet created");

        assert_eq!(facet.vertices.len(), 3);
        let floor_plane = storey.plane(floor_id).unwrap();
        assert_eq!(floor_plane.facets.len(), 2);
    }

    #[test]
    fn test_plane_facet_do_target_not_found_returns_none_and_does_not_create_plane() {
        let mut scene = Scene::new();
        let mut cl = CommandLine::new();
        let mut storey = StoreyRef::new("Storey", 0.0, "dwg");
        let initial_planes_count = storey.control_planes.len();
        let nonexistent_plane_id = Uuid::new_v4();

        let facet = aec_plane_facet_do(
            Some(&mut scene),
            &mut cl,
            Some(&mut storey),
            Some(nonexistent_plane_id),
            "0.0 0.0 0.0 4.0 0.0 0.0 4.0 4.0 0.5",
        );
        assert!(facet.is_none());
        assert_eq!(storey.control_planes.len(), initial_planes_count);
    }

    #[test]
    fn test_plane_facet_do_missing_storey_returns_none() {
        let mut scene = Scene::new();
        let mut cl = CommandLine::new();
        let facet = aec_plane_facet_do(
            Some(&mut scene),
            &mut cl,
            None,
            None,
            "0.0 0.0 0.0 4.0 0.0 0.0 4.0 4.0 0.5",
        );
        assert!(facet.is_none());
    }

    #[test]
    fn test_plane_facet_do_malformed_target_in_args_returns_none() {
        let mut scene = Scene::new();
        let mut cl = CommandLine::new();
        let mut storey = StoreyRef::new("Storey", 0.0, "dwg");
        let initial_planes_count = storey.control_planes.len();

        let facet = aec_plane_facet_do(
            Some(&mut scene),
            &mut cl,
            Some(&mut storey),
            None,
            "bad|target|string 0.0 0.0 0.0 4.0 0.0 0.0 4.0 4.0 0.5",
        );
        assert!(facet.is_none());
        assert_eq!(storey.control_planes.len(), initial_planes_count);
    }

    #[test]
    fn test_plane_facet_do_without_scene_updates_model_safely() {
        let mut cl = CommandLine::new();
        let mut storey = StoreyRef::new("Storey", 0.0, "dwg");
        let ceiling_id = storey.ceiling_plane_id;

        let facet = aec_plane_facet_do(
            None,
            &mut cl,
            Some(&mut storey),
            Some(ceiling_id),
            "0.0 0.0 3.0 5.0 0.0 3.0 5.0 5.0 4.0",
        )
        .expect("facet created");

        assert_eq!(facet.vertices.len(), 3);
        assert_eq!(storey.plane(ceiling_id).unwrap().facets.len(), 2);
    }
}
