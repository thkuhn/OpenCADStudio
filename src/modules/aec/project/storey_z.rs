//! After storey elevation/height: refresh previews and rebake bound walls.

use acadrust::xdata::ExtendedDataRecord;
use acadrust::Handle;

use crate::modules::aec::engine::wall_regen::regenerate_wall_representation;
use crate::modules::aec::engine::xdata::{wall_from_entity, wall_record_for_wall, write_aec_record, AEC_APPID};
use crate::modules::aec::engine::library::StyleLibrary;
use crate::modules::aec::engine::project::{ProjectFile, StoreyRef};
use crate::modules::aec::project::preview::regenerate_control_plane_previews;
use crate::app::OpenCADStudio;
use crate::scene::Scene;

pub fn apply_storey_z_to_scene(
    scene: &mut Scene,
    storey: &mut StoreyRef,
    project: Option<&ProjectFile>,
    library: Option<&StyleLibrary>,
) {
    regenerate_control_plane_previews(scene, storey);
    rebake_bound_walls(scene, storey, project, library);
    crate::modules::aec::project::opening_planes::rebake_bound_openings(
        scene, storey, project, library,
    );
}

fn rebake_bound_walls(
    scene: &mut Scene,
    storey: &StoreyRef,
    project: Option<&ProjectFile>,
    library: Option<&StyleLibrary>,
) {
    let mut jobs: Vec<(Handle, f64, f64)> = Vec::new();
    for entity in scene.document.entities() {
        let handle = entity.common().handle;
        let Some(wall) = wall_from_entity(entity) else {
            continue;
        };
        let bound_here = wall
            .base_plane_id
            .and_then(|id| storey.plane(id))
            .is_some()
            || wall
                .top_plane_id
                .and_then(|id| storey.plane(id))
                .is_some();
        if !bound_here {
            continue;
        }
        let (x, y) = match entity {
            acadrust::EntityType::LwPolyline(pl) if !pl.vertices.is_empty() => {
                (pl.vertices[0].location.x, pl.vertices[0].location.y)
            }
            _ => (0.0, 0.0),
        };
        jobs.push((handle, x, y));
    }
    for (handle, x, y) in jobs {
        let Some(entity) = scene.document.get_entity(handle) else {
            continue;
        };
        let Some(mut wall) = wall_from_entity(entity) else {
            continue;
        };
        if let Some(project) = project {
            wall.rebake_from_project(project, x, y);
        } else {
            wall.rebake_planes(storey, x, y);
        }
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record_for_wall(&wall);
        write_aec_record(&mut scene.document, handle, record);
        if let Ok(touched) = regenerate_wall_representation(scene, handle, library) {
            let changes: Vec<_> = touched
                .into_iter()
                .map(|h| (h, crate::scene::ChangeKind::Modified))
                .collect();
            if !changes.is_empty() {
                scene.bump_entities(&changes);
            }
        }
    }
}

impl OpenCADStudio {
    pub(crate) fn commit_storey_settings_buffers(&mut self, bid: uuid::Uuid, sid: uuid::Uuid) {
        if let Ok(v) = self.aec.aec_storey_settings_elevation.trim().parse::<f64>() {
            self.with_storey_mut(bid, sid, |s| s.set_elevation(v));
        }
        if let Ok(v) = self.aec.aec_storey_settings_height.trim().parse::<f64>() {
            if v > 0.0 {
                self.with_storey_mut(bid, sid, |s| {
                    s.set_height(v);
                    let cid = s.ceiling_plane_id;
                    s.set_plane_z_relative_to_floor(cid, v);
                });
            }
        }
        let plane_z_map = self.aec.aec_storey_settings_plane_z.clone();
        for (pid, text) in plane_z_map {
            if let Ok(v) = text.trim().parse::<f64>() {
                self.with_storey_mut(bid, sid, |s| {
                    s.set_plane_z_relative_to_floor(pid, v);
                });
            }
        }
        let facet_z_map = self.aec.aec_storey_settings_facet_z.clone();
        for ((pid, f_idx), text) in facet_z_map {
            if let Ok(v) = text.trim().parse::<f64>() {
                self.with_storey_mut(bid, sid, |s| {
                    s.set_facet_z_relative_to_floor(pid, f_idx, v);
                });
            }
        }
    }

    pub(crate) fn apply_storey_z_to_active_scene(&mut self, bid: uuid::Uuid, sid: uuid::Uuid) {
        let i = self.active_tab;
        let library = crate::modules::aec::engine::project::resolve_style_library(
            self.aec.aec_project_explorer_file.as_ref(),
        );
        let project_snap = self.aec.aec_project_explorer_file.clone();
        if let Some(storey) = self.aec.aec_project_explorer_file.as_mut().and_then(|p| {
            p.buildings
                .iter_mut()
                .find(|b| b.id == bid)
                .and_then(|b| b.storeys.iter_mut().find(|s| s.id == sid))
        }) {
            apply_storey_z_to_scene(
                &mut self.tabs[i].scene,
                storey,
                project_snap.as_ref(),
                Some(&library),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};
    use crate::modules::aec::engine::project::Building;
    use crate::modules::aec::engine::wall::Wall;

    #[test]
    fn rebake_bound_walls_preserves_sloped_top_after_plane_deletion_and_storey_z_change() {
        let mut project = ProjectFile::default();
        let mut building = Building::new("Haus");
        let mut eg = StoreyRef::new_with_height("EG", 0.0, 2.80, "eg.dwg");
        let floor_id = eg.floor_plane_id;

        // Sloped roof plane: Z=4.0 at x=0, Z=6.0 at x=10
        let f1 = ControlPlaneFacet::new(
            "RoofSlope",
            vec![
                [0.0, -5.0, 4.0],
                [10.0, -5.0, 6.0],
                [10.0, 5.0, 6.0],
                [0.0, 5.0, 4.0],
            ],
        );
        let roof_cp = ControlPlane::from_facets("ShedRoof", vec![f1]);
        let roof_id = roof_cp.id;
        eg.control_planes.push(roof_cp);
        building.storeys.push(eg);
        project.buildings.push(building);

        let mut scene = Scene::new();
        let mut wall = Wall::new("standard_wall", 2.80, 0);
        wall.base_plane_id = Some(floor_id);
        wall.top_plane_id = Some(roof_id);
        wall.formulas.set("wall_top_offset", "_STORY_H - 2.80".to_string());

        let mut poly = LwPolyline::new();
        poly.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        let handle = scene.add_entity(acadrust::EntityType::LwPolyline(poly));

        let mut rec = ExtendedDataRecord::new(AEC_APPID);
        rec.values = wall_record_for_wall(&wall);
        write_aec_record(&mut scene.document, handle, rec);

        // Initial rebake
        let storey = &project.buildings[0].storeys[0];
        let mut storey_mut = storey.clone();
        apply_storey_z_to_scene(&mut scene, &mut storey_mut, Some(&project), None);

        let w_initial = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        let top_z_p1 = w_initial.top_z_at_xy(0.0, 0.0);
        let top_z_p2 = w_initial.top_z_at_xy(5.0, 0.0);
        assert!((top_z_p1 - 4.0).abs() < 1e-9);
        assert!((top_z_p2 - 5.0).abs() < 1e-9);
        assert!(w_initial.is_sloped());
        assert_eq!(w_initial.formulas.get("wall_top_offset"), Some("_STORY_H - 2.80"));

        // Delete the custom roof plane from the project
        project.buildings[0].storeys[0].control_planes.retain(|p| p.id != roof_id);
        assert!(project.control_plane(roof_id).is_none());

        // Change storey floor elevation (base moves from Z=0.0 to Z=1.0)
        project.buildings[0].storeys[0].set_elevation(1.0);
        let mut eg_storey = project.buildings[0].storeys[0].clone();

        // Apply storey Z change to active scene
        apply_storey_z_to_scene(&mut scene, &mut eg_storey, Some(&project), None);

        // Verify wall geometry after plane deletion and storey elevation change
        let w_after = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();

        // Base moved to 1.0
        assert!((w_after.base_origin[2] - 1.0).abs() < 1e-9);
        // Baked top facets and sloped geometry remain exact at both points
        assert_eq!(w_after.top_facets.len(), 1);
        assert_eq!(w_after.top_facets[0].name, "RoofSlope");
        assert!(w_after.is_sloped());
        assert!((w_after.top_z_at_xy(0.0, 0.0) - top_z_p1).abs() < 1e-9);
        assert!((w_after.top_z_at_xy(5.0, 0.0) - top_z_p2).abs() < 1e-9);

        // Formula remains intact
        assert_eq!(w_after.formulas.get("wall_top_offset"), Some("_STORY_H - 2.80"));

        // Re-evaluate formula with a new parameter value (_STORY_H = 3.10 => offset = +0.30)
        scene.named_parameters_mut().set("_STORY_H", "3.10").unwrap();
        let expr = w_after.formulas.get("wall_top_offset").unwrap();
        let new_offset = crate::modules::aec::properties::eval_aec_property_expression(&scene, expr).unwrap();
        assert!((new_offset - 0.30).abs() < 1e-9);

        let mut w_recomputed = w_after.clone();
        w_recomputed.top_offset = new_offset;
        // Verify sloped snapshot keeps exact slope (1.0m rise per 5m) and shifts top elevation along plane normal:
        let top_z_recomputed_p1 = w_recomputed.top_z_at_xy(0.0, 0.0);
        let top_z_recomputed_p2 = w_recomputed.top_z_at_xy(5.0, 0.0);
        assert!((top_z_recomputed_p2 - top_z_recomputed_p1 - 1.0).abs() < 1e-9);
        let normal_scale = 1.04_f64.sqrt(); // 1 / nz for facet unit normal [-2, 0, 10]/sqrt(104)
        assert!((top_z_recomputed_p1 - (top_z_p1 + new_offset * normal_scale)).abs() < 1e-9);
        assert!((top_z_recomputed_p2 - (top_z_p2 + new_offset * normal_scale)).abs() < 1e-9);
        assert!((top_z_recomputed_p1 - (4.0 + 0.30 * normal_scale)).abs() < 1e-9);
        assert!((top_z_recomputed_p2 - (5.0 + 0.30 * normal_scale)).abs() < 1e-9);
    }
}
