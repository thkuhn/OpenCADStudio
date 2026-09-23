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
                self.with_storey_mut(bid, sid, |s| s.set_height(v));
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
