//! `AEC_PLANE_ASSIGN` command: assign existing drawing polygons/faces to a control plane.

use acadrust::entities::EntityType;
use acadrust::types::Handle;
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
        id: "AEC_PLANE_ASSIGN",
        label: "Assign polygons",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/layers/panel.svg")),
        event: ModuleEvent::Command("AEC_PLANE_ASSIGN".to_string()),
    }
}

fn approx_arr3(a: [f64; 3], b: [f64; 3]) -> bool {
    (a[0] - b[0]).abs() <= 1e-6 && (a[1] - b[1]).abs() <= 1e-6 && (a[2] - b[2]).abs() <= 1e-6
}

/// Extract polygon facets from supported CAD entity types (Face3D, Polyline, LwPolyline, Polyline3D, Solid).
pub fn extract_facets_from_entity(entity: &EntityType, name_prefix: &str) -> Vec<ControlPlaneFacet> {
    match entity {
        EntityType::Face3D(f) => {
            let p1 = [f.first_corner.x, f.first_corner.y, f.first_corner.z];
            let p2 = [f.second_corner.x, f.second_corner.y, f.second_corner.z];
            let p3 = [f.third_corner.x, f.third_corner.y, f.third_corner.z];
            let p4 = [f.fourth_corner.x, f.fourth_corner.y, f.fourth_corner.z];
            let mut verts = vec![p1, p2, p3];
            if !approx_arr3(p4, p3) && !approx_arr3(p4, p1) {
                verts.push(p4);
            }
            vec![ControlPlaneFacet::new(format!("{name_prefix}_Face3D"), verts)]
        }
        EntityType::LwPolyline(lw) => {
            let elev = lw.elevation;
            let verts: Vec<[f64; 3]> = lw
                .vertices
                .iter()
                .map(|v| [v.location.x, v.location.y, elev])
                .collect();
            if verts.len() >= 3 {
                vec![ControlPlaneFacet::new(format!("{name_prefix}_Polyline"), verts)]
            } else {
                vec![]
            }
        }
        EntityType::Polyline(poly) => {
            let verts: Vec<[f64; 3]> = poly
                .vertices
                .iter()
                .map(|v| [v.location.x, v.location.y, v.location.z])
                .collect();
            if verts.len() >= 3 {
                vec![ControlPlaneFacet::new(format!("{name_prefix}_Polyline"), verts)]
            } else {
                vec![]
            }
        }
        EntityType::Polyline2D(p2) => {
            let elev = p2.elevation;
            let verts: Vec<[f64; 3]> = p2
                .vertices
                .iter()
                .map(|v| [v.location.x, v.location.y, elev])
                .collect();
            if verts.len() >= 3 {
                vec![ControlPlaneFacet::new(format!("{name_prefix}_Polyline2D"), verts)]
            } else {
                vec![]
            }
        }
        EntityType::Polyline3D(p3) => {
            let verts: Vec<[f64; 3]> = p3
                .vertices
                .iter()
                .map(|v| [v.position.x, v.position.y, v.position.z])
                .collect();
            if verts.len() >= 3 {
                vec![ControlPlaneFacet::new(format!("{name_prefix}_3DPoly"), verts)]
            } else {
                vec![]
            }
        }
        EntityType::Solid(s) => {
            let p1 = [s.first_corner.x, s.first_corner.y, s.first_corner.z];
            let p2 = [s.second_corner.x, s.second_corner.y, s.second_corner.z];
            let p3 = [s.third_corner.x, s.third_corner.y, s.third_corner.z];
            let p4 = [s.fourth_corner.x, s.fourth_corner.y, s.fourth_corner.z];
            let mut verts = vec![p1, p2, p3];
            if !approx_arr3(p4, p3) && !approx_arr3(p4, p1) {
                verts.push(p4);
            }
            vec![ControlPlaneFacet::new(format!("{name_prefix}_Solid"), verts)]
        }
        _ => vec![],
    }
}

pub struct PlaneAssignCommand {
    target: Option<(Uuid, Uuid, Uuid)>,
    selected_handles: Vec<Handle>,
}

impl PlaneAssignCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            target: None,
            selected_handles: Vec::new(),
        }
    }

    pub fn with_target(bid: Uuid, sid: Uuid, pid: Uuid) -> Self {
        Self {
            target: Some((bid, sid, pid)),
            selected_handles: Vec::new(),
        }
    }

    pub fn prompt(&self) -> String {
        crate::t!("AEC_PLANE_ASSIGN: Flächen oder Polylinien im Viewport auswählen [Eingabetaste=Abschließen]:").into_owned()
    }

    fn finish_dispatch(&self) -> CmdResult {
        if self.selected_handles.is_empty() && self.target.is_none() {
            return CmdResult::Cancel;
        }
        let target_str = match self.target {
            Some((b, s, p)) => format!("{b}|{s}|{p}"),
            None => "_|_|_".to_string(),
        };
        let handles_str = self
            .selected_handles
            .iter()
            .map(|h| h.value().to_string())
            .collect::<Vec<_>>()
            .join(" ");
        CmdResult::Dispatch(format!("AEC_PLANE_ASSIGN_DO {target_str} {handles_str}"))
    }
}

impl CadCommand for PlaneAssignCommand {
    fn name(&self) -> &'static str {
        "AEC_PLANE_ASSIGN"
    }

    fn prompt(&self) -> String {
        crate::t!("AEC_PLANE_ASSIGN: Flächen oder Polylinien im Viewport auswählen [Eingabetaste=Abschließen]:").into_owned()
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn entity_pick_includes_fills(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if !handle.is_null() && !self.selected_handles.contains(&handle) {
            self.selected_handles.push(handle);
        }
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        self.finish_dispatch()
    }

    fn on_escape(&mut self) -> CmdResult {
        if self.target.is_some() {
            self.selected_handles.clear();
            self.finish_dispatch()
        } else {
            CmdResult::Cancel
        }
    }
}

pub fn aec_plane_assign_do(
    scene: &mut Scene,
    command_line: &mut CommandLine,
    storey: Option<&mut StoreyRef>,
    target_plane_id: Option<Uuid>,
    handles: &[Handle],
) -> usize {
    if handles.is_empty() {
        return 0;
    }
    let mut extracted_facets = Vec::new();
    for (idx, handle) in handles.iter().enumerate() {
        if let Some(entity) = scene.document.get_entity(*handle) {
            let prefix = format!("Facet_{}", idx + 1);
            let facets = extract_facets_from_entity(entity, &prefix);
            extracted_facets.extend(facets);
        }
    }

    if extracted_facets.is_empty() {
        command_line.push_error(crate::t!("AEC_PLANE_ASSIGN: Keine gültigen 3D-Flächen oder Polylinien gefunden.").as_ref());
        return 0;
    }

    let count = extracted_facets.len();
    if let Some(s) = storey {
        let plane_name;
        if let Some(pid) = target_plane_id {
            if let Some(plane) = s.plane_mut(pid) {
                plane_name = plane.name.clone();
                // If plane only had a single default facet (name matching plane.name), replace it; otherwise append
                if plane.facets.len() == 1 && plane.facets[0].name == plane.name {
                    plane.facets = extracted_facets;
                    if let Some(first) = plane.facets.first() {
                        plane.origin = first.origin();
                        plane.normal = first.unit_normal();
                    }
                } else {
                    for f in extracted_facets {
                        plane.add_facet(f);
                    }
                }
            } else {
                plane_name = "AssignedPlane".to_string();
                let comp = ControlPlane::from_facets(plane_name.clone(), extracted_facets);
                s.control_planes.push(comp);
            }
        } else {
            plane_name = "CompositePlane".to_string();
            let comp = ControlPlane::from_facets(plane_name.clone(), extracted_facets);
            s.control_planes.push(comp);
        }

        regenerate_control_plane_previews(scene, s);

        command_line.push_info(&format!(
            "AEC: {count} Polygon(e) der Kontrollebene '{plane_name}' zugewiesen."
        ));
    }

    count
}

inventory::submit!(crate::command::CommandRegistration { names: &["AEC_PLANE_ASSIGN"] });

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::{Face3D, LwPolyline, LwVertex};
    use acadrust::types::{Vector2, Vector3};

    #[test]
    fn test_extract_facets_from_face3d() {
        let face = Face3D::new(
            Vector3::new(0.0, 0.0, 3.0),
            Vector3::new(5.0, 0.0, 3.0),
            Vector3::new(5.0, 5.0, 4.0),
            Vector3::new(0.0, 5.0, 4.0),
        );
        let entity = EntityType::Face3D(face);
        let facets = extract_facets_from_entity(&entity, "Test");
        assert_eq!(facets.len(), 1);
        assert_eq!(facets[0].vertices.len(), 4);
        assert_eq!(facets[0].vertices[0], [0.0, 0.0, 3.0]);
        assert_eq!(facets[0].vertices[2], [5.0, 5.0, 4.0]);
    }

    #[test]
    fn test_extract_facets_from_lwpolyline() {
        let mut lw = LwPolyline::new();
        lw.elevation = 2.8;
        lw.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        lw.add_vertex(LwVertex::new(Vector2::new(6.0, 0.0)));
        lw.add_vertex(LwVertex::new(Vector2::new(6.0, 4.0)));
        lw.add_vertex(LwVertex::new(Vector2::new(0.0, 4.0)));
        let entity = EntityType::LwPolyline(lw);
        let facets = extract_facets_from_entity(&entity, "RoofPart");
        assert_eq!(facets.len(), 1);
        assert_eq!(facets[0].vertices.len(), 4);
        assert_eq!(facets[0].vertices[0], [0.0, 0.0, 2.8]);
    }

    #[test]
    fn test_plane_assign_command_flow() {
        let bid = Uuid::new_v4();
        let sid = Uuid::new_v4();
        let pid = Uuid::new_v4();
        let mut cmd = PlaneAssignCommand::with_target(bid, sid, pid);
        assert!(matches!(cmd.on_entity_pick(Handle::new(42), DVec3::ZERO), CmdResult::NeedPoint));
        let res = cmd.on_enter();
        match res {
            CmdResult::Dispatch(s) => {
                assert!(s.starts_with(&format!("AEC_PLANE_ASSIGN_DO {bid}|{sid}|{pid} 42")));
            }
            _ => panic!("Expected dispatch"),
        }
    }

    #[test]
    fn test_aec_plane_assign_do_assigns_to_storey_plane() {
        let mut scene = Scene::new();
        let mut cl = CommandLine::new();
        let mut storey = StoreyRef::new("EG", 0.0, "eg.dwg");
        let ceiling_id = storey.ceiling_plane_id;

        let face = Face3D::new(
            Vector3::new(0.0, 0.0, 3.0),
            Vector3::new(5.0, 0.0, 3.0),
            Vector3::new(5.0, 5.0, 4.0),
            Vector3::new(0.0, 5.0, 4.0),
        );
        let h = scene.add_entity(EntityType::Face3D(face));

        let count = aec_plane_assign_do(
            &mut scene,
            &mut cl,
            Some(&mut storey),
            Some(ceiling_id),
            &[h],
        );
        assert_eq!(count, 1);
        let plane = storey.plane(ceiling_id).unwrap();
        assert_eq!(plane.facets.len(), 1);
        assert_eq!(plane.facets[0].vertices.len(), 4);
    }
}
