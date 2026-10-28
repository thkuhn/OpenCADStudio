//! Bind/unbind wall base/top control planes from the properties panel.

use std::collections::{HashMap, HashSet};
use acadrust::xdata::ExtendedDataRecord;
use acadrust::Handle;
use uuid::Uuid;

use crate::modules::aec::engine::control_plane::ControlPlane;
use crate::modules::aec::engine::wall_regen::regenerate_wall_representation;
use crate::modules::aec::engine::xdata::{wall_from_entity, wall_record_for_wall, write_aec_record, AEC_APPID};
use crate::modules::aec::engine::library::StyleLibrary;
use crate::modules::aec::engine::project::ProjectFile;
use crate::scene::Scene;

pub const UNBOUND_LABEL: &str = "(none)";

pub(crate) fn is_unbound_label(choice: &str) -> bool {
    choice == UNBOUND_LABEL || choice == crate::t!("(none)").as_ref()
}

/// Formats a control plane label for UI dropdowns.
/// - Horizontal (< 0.1°): `{name} (Z = X.XXX m)` with `origin_z = plane.origin[2]`.
/// - Sloped (>= 0.1°): `{name} (Z ≈ X.XXX m, N.N°)` with `origin_z = first_facet.origin()[2]` and `slope = first_facet.slope_degrees()`.
/// - Multi-facet (`facets.len() > 1`): `{name} (Multi, Z ≈ X.XXX m, N.N°)` based on the first facet.
pub fn format_plane_display_label(display_name: &str, plane: &ControlPlane) -> String {
    if plane.facets.len() > 1 {
        let first = &plane.facets[0];
        let origin_z = first.origin()[2];
        let slope = first.slope_degrees();
        format!("{display_name} (Multi, Z ≈ {:.3} m, {:.1}°)", origin_z, slope)
    } else if let Some(first) = plane.facets.first() {
        let slope = first.slope_degrees();
        if slope >= 0.1 {
            let origin_z = first.origin()[2];
            format!("{display_name} (Z ≈ {:.3} m, {:.1}°)", origin_z, slope)
        } else {
            let origin_z = plane.origin[2];
            format!("{display_name} (Z = {:.3} m)", origin_z)
        }
    } else {
        let slope = plane.slope_degrees();
        if slope >= 0.1 {
            let origin_z = plane.origin[2];
            format!("{display_name} (Z ≈ {:.3} m, {:.1}°)", origin_z, slope)
        } else {
            let origin_z = plane.origin[2];
            format!("{display_name} (Z = {:.3} m)", origin_z)
        }
    }
}

pub struct ProjectPlaneInfo<'a> {
    pub id: Uuid,
    pub storey_name: &'a str,
    pub plane: &'a ControlPlane,
    pub disambiguated_name: String,
    pub formatted_label: String,
}

pub fn collect_project_planes<'a>(project: &'a ProjectFile) -> Vec<ProjectPlaneInfo<'a>> {
    let mut raw_planes = Vec::new();
    let mut seen_ids = HashSet::new();
    let mut name_counts: HashMap<String, usize> = HashMap::new();

    for b in &project.buildings {
        for s in &b.storeys {
            for p in &s.control_planes {
                if seen_ids.insert(p.id) {
                    *name_counts.entry(p.name.to_lowercase()).or_insert(0) += 1;
                    raw_planes.push((s.name.as_str(), p));
                }
            }
        }
    }

    raw_planes
        .into_iter()
        .map(|(storey_name, plane)| {
            let is_duplicate = name_counts.get(&plane.name.to_lowercase()).copied().unwrap_or(0) > 1;
            let disambiguated_name = if is_duplicate {
                format!("{} [{storey_name}]", plane.name)
            } else {
                plane.name.clone()
            };
            let formatted_label = format_plane_display_label(&disambiguated_name, plane);
            ProjectPlaneInfo {
                id: plane.id,
                storey_name,
                plane,
                disambiguated_name,
                formatted_label,
            }
        })
        .collect()
}

pub fn build_project_plane_options(project: Option<&ProjectFile>) -> Vec<String> {
    let mut options = vec![crate::t!("(none)").into_owned()];
    if let Some(project) = project {
        for info in collect_project_planes(project) {
            options.push(info.formatted_label);
        }
    }
    options
}

pub fn resolve_plane_display_label(
    project: Option<&ProjectFile>,
    id: Option<Uuid>,
    stored_name: Option<&str>,
) -> String {
    let none = crate::t!("(none)").into_owned();
    if let Some(id) = id {
        if let Some(project) = project {
            let planes = collect_project_planes(project);
            if let Some(info) = planes.iter().find(|info| info.id == id) {
                return info.formatted_label.clone();
            }
        }
    }
    if let Some(stored) = stored_name {
        let trimmed = stored.trim();
        if !trimmed.is_empty() && !is_unbound_label(trimmed) {
            return trimmed.to_string();
        }
    }
    if let Some(id) = id {
        return id.to_string();
    }
    none
}

pub fn clean_plane_name(choice: &str) -> String {
    let choice = choice.trim();
    let stripped = if let Some(idx) = choice.rfind(" (Z") {
        choice[..idx].trim()
    } else {
        choice
    };
    if let Some(open_bracket) = stripped.rfind(" [") {
        if stripped.ends_with(']') {
            return stripped[..open_bracket].trim().to_string();
        }
    }
    stripped.to_string()
}

pub fn resolve_plane_stored_name(
    project: Option<&ProjectFile>,
    id: Option<Uuid>,
    choice: &str,
) -> Option<String> {
    if is_unbound_label(choice) || choice.trim().is_empty() {
        return None;
    }
    if let Some(id) = id {
        if let Some(project) = project {
            for b in &project.buildings {
                for s in &b.storeys {
                    if let Some(p) = s.plane(id) {
                        return Some(p.name.clone());
                    }
                }
            }
        }
    }
    let cleaned = clean_plane_name(choice);
    if cleaned.is_empty() || is_unbound_label(&cleaned) {
        None
    } else {
        Some(cleaned)
    }
}

pub fn apply_wall_plane_choice(
    scene: &mut Scene,
    project: Option<&ProjectFile>,
    handle: Handle,
    base: bool,
    choice: &str,
    library: Option<&StyleLibrary>,
) -> bool {
    let Some(entity) = scene.document.get_entity(handle) else {
        return false;
    };
    let (x, y) = match entity {
        acadrust::EntityType::LwPolyline(pl) if !pl.vertices.is_empty() => {
            (pl.vertices[0].location.x, pl.vertices[0].location.y)
        }
        _ => (0.0, 0.0),
    };
    let Some(mut wall) = wall_from_entity(entity) else {
        return false;
    };
    let id = parse_plane_choice(project, choice);
    let name = resolve_plane_stored_name(project, id, choice);
    if base {
        wall.base_plane_id = id;
        wall.base_plane_name = name;
        if id.is_none() {
            wall.base_facets.clear();
            wall.base_normal = [0.0, 0.0, 1.0];
        }
        wall.base_offset = 0.0;
        wall.formulas.remove("wall_base_offset");
    } else {
        wall.top_plane_id = id;
        wall.top_plane_name = name;
        if id.is_none() {
            wall.top_facets.clear();
            wall.top_normal = wall.base_normal;
            wall.top_origin = [
                wall.base_origin[0],
                wall.base_origin[1],
                wall.base_origin[2] + wall.height,
            ];
        }
        wall.top_offset = 0.0;
        wall.formulas.remove("wall_top_offset");
    }
    if let Some(project) = project {
        wall.rebake_from_project(project, x, y);
    }
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    write_aec_record(&mut scene.document, handle, record);
    if let Some(acadrust::EntityType::LwPolyline(pl)) = scene.document.get_entity_mut(handle) {
        pl.elevation = wall.base_origin[2];
    }
    if let Ok(touched) = regenerate_wall_representation(scene, handle, library) {
        let changes: Vec<_> = touched
            .into_iter()
            .map(|h| (h, crate::scene::ChangeKind::Modified))
            .collect();
        if !changes.is_empty() {
            scene.bump_entities(&changes);
        }
    }
    true
}

pub fn apply_wall_base_z(scene: &mut Scene, handle: Handle, z: f64) -> bool {
    let Some(entity) = scene.document.get_entity(handle) else {
        return false;
    };
    let Some(mut wall) = wall_from_entity(entity) else {
        return false;
    };
    if wall.base_plane_id.is_some() {
        return false;
    }
    wall.base_origin[2] = z;
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    let ok = write_aec_record(&mut scene.document, handle, record);
    if ok {
        if let Some(acadrust::EntityType::LwPolyline(pl)) = scene.document.get_entity_mut(handle) {
            pl.elevation = z;
        }
    }
    ok
}

pub(crate) fn parse_plane_choice(project: Option<&ProjectFile>, choice: &str) -> Option<Uuid> {
    let choice = choice.trim();
    if choice.is_empty() || is_unbound_label(choice) {
        return None;
    }

    if let Some(project) = project {
        let planes = collect_project_planes(project);

        // 1. Exact match against formatted label (e.g. "Floor [EG] (Z = 0.000 m)")
        if let Some(info) = planes.iter().find(|info| info.formatted_label == choice) {
            return Some(info.id);
        }

        // 2. Exact match against disambiguated name (e.g. "Floor [EG]")
        if let Some(info) = planes
            .iter()
            .find(|info| info.disambiguated_name.eq_ignore_ascii_case(choice))
        {
            return Some(info.id);
        }

        // 3. Fallback suffix stripping via `rfind(" (Z")` (e.g. "Dach (Nord) (Z ≈ 3.000 m, 35.0°)" -> "Dach (Nord)")
        let stripped = if let Some(idx) = choice.rfind(" (Z") {
            choice[..idx].trim()
        } else {
            choice
        };

        // 3a. Match stripped against formatted labels / disambiguated names
        if let Some(info) = planes.iter().find(|info| {
            info.disambiguated_name.eq_ignore_ascii_case(stripped)
                || info.formatted_label.eq_ignore_ascii_case(stripped)
        }) {
            return Some(info.id);
        }

        // 3b. If stripped contains ` [Storey]` (e.g. "Floor [EG]"), parse plane_name and storey_name
        if let Some(open_bracket) = stripped.rfind(" [") {
            if stripped.ends_with(']') {
                let p_name = stripped[..open_bracket].trim();
                let s_name = &stripped[open_bracket + 2..stripped.len() - 1].trim();
                if let Some(info) = planes.iter().find(|info| {
                    info.plane.name.eq_ignore_ascii_case(p_name)
                        && info.storey_name.eq_ignore_ascii_case(s_name)
                }) {
                    return Some(info.id);
                }
            }
        }

        // 3c. Match stripped against pure plane name IF unique project-wide
        let matching_by_pure_name: Vec<&ProjectPlaneInfo> = planes
            .iter()
            .filter(|info| info.plane.name.eq_ignore_ascii_case(stripped))
            .collect();
        if matching_by_pure_name.len() == 1 {
            return Some(matching_by_pure_name[0].id);
        }
    }

    // 4. Fallback on Uuid::parse_str
    if let Ok(id) = Uuid::parse_str(choice) {
        return Some(id);
    }

    None
}

/// Deletes a custom control plane from a storey in the project, refusing to delete
/// the main floor or ceiling plane. Returns true if the plane was removed.
pub fn delete_control_plane_from_storey(
    storey: &mut crate::modules::aec::engine::project::StoreyRef,
    plane_id: Uuid,
) -> bool {
    storey.delete_plane(plane_id)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::control_plane::resolve_wall_height;
    use crate::modules::aec::engine::project::StoreyRef;
    use crate::modules::aec::engine::wall::Wall;

    #[test]
    fn offset_changes_resolved_height() {
        let storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        let base = storey.plane(storey.floor_plane_id).unwrap();
        let top = storey.plane(storey.ceiling_plane_id).unwrap();
        let h0 = resolve_wall_height(-5.0, -5.0, base, top, 0.0, 0.0).unwrap();
        let h1 = resolve_wall_height(-5.0, -5.0, base, top, 0.1, -0.2).unwrap();
        assert!((h0 - 3.0).abs() < 1e-6);
        assert!((h1 - 2.7).abs() < 1e-6);
    }

    #[test]
    fn first_plane_choice_is_kept_until_second() {
        let storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        let mut wall = Wall::new("s", 2.5, 0);
        wall.base_origin[2] = 1.0;
        wall.base_plane_id = Some(storey.floor_plane_id);
        wall.rebake_planes(&storey, 0.0, 0.0);
        assert_eq!(wall.base_plane_id, Some(storey.floor_plane_id));
        assert!(wall.top_plane_id.is_none());
        assert!((wall.height - 2.5).abs() < 1e-9);
        assert!((wall.base_origin[2] - 0.0).abs() < 1e-9);
        wall.top_plane_id = Some(storey.ceiling_plane_id);
        wall.rebake_planes(&storey, 0.0, 0.0);
        assert!((wall.height - 3.0).abs() < 1e-6);
    }

    #[test]
    fn parse_plane_choice_by_name() {
        use crate::modules::aec::engine::project::{Building, ProjectFile};
        let storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        let floor_id = storey.floor_plane_id;
        let floor_name = storey.plane(floor_id).unwrap().name.clone();
        let mut project = ProjectFile::default();
        let mut building = Building::new("B");
        building.storeys.push(storey);
        project.buildings.push(building);
        assert_eq!(
            super::parse_plane_choice(Some(&project), &floor_name),
            Some(floor_id)
        );
        assert!(super::parse_plane_choice(Some(&project), super::UNBOUND_LABEL).is_none());
    }

    #[test]
    fn unbind_ignores_plane_move() {
        let mut storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        let mut wall = Wall::new("s", 2.5, 0);
        wall.base_origin[2] = 1.0;
        wall.height = 2.5;
        storey.set_elevation(4.0);
        wall.rebake_planes(&storey, 0.0, 0.0);
        assert!((wall.height - 2.5).abs() < 1e-9);
        assert!((wall.base_origin[2] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_plane_choice_resets_offset_and_adapts_height() {
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2;
        use crate::modules::aec::engine::project::{Building, ProjectFile};
        let mut project = ProjectFile::default();
        let mut building = Building::new("Haus");
        let eg = StoreyRef::new_with_height("EG", 0.0, 2.80, "eg.dwg");
        let og = StoreyRef::new_with_height("OG", 2.80, 3.10, "og.dwg");
        building.storeys.push(eg);
        building.storeys.push(og);
        project.buildings.push(building);

        let mut scene = Scene::new();
        let mut wall = Wall::new("standard_wall", 2.50, 0);
        wall.base_offset = 0.15;
        wall.top_offset = -0.20;
        let mut poly = LwPolyline::new();
        poly.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let handle = scene.add_entity(acadrust::EntityType::LwPolyline(poly));
        let mut rec = ExtendedDataRecord::new(AEC_APPID);
        rec.values = wall_record_for_wall(&wall);
        write_aec_record(&mut scene.document, handle, rec);

        // Bind BasePlane to EG_ELEVATION
        let ok_base = apply_wall_plane_choice(&mut scene, Some(&project), handle, true, "EG_ELEVATION", None);
        assert!(ok_base);
        let w_base = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        assert_eq!(w_base.base_offset, 0.0);

        // Bind TopPlane to OG_ELEVATION (Z=2.80) -> Height should automatically adapt to 2.80m
        let ok_top = apply_wall_plane_choice(&mut scene, Some(&project), handle, false, "OG_ELEVATION", None);
        assert!(ok_top);
        let w_top = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        assert_eq!(w_top.top_offset, 0.0);
        assert!((w_top.height - 2.80).abs() < 1e-5);
    }

    #[test]
    fn explicit_unbound_choice_clears_plane_id_and_facets_and_sets_flat_height() {
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2;
        use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};
        use crate::modules::aec::engine::project::{Building, ProjectFile};

        let mut project = ProjectFile::default();
        let mut building = Building::new("Haus");
        let mut eg = StoreyRef::new_with_height("EG", 0.0, 2.80, "eg.dwg");

        // Sloped roof plane
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
        let wall = Wall::new("standard_wall", 2.80, 0);
        let mut poly = LwPolyline::new();
        poly.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let handle = scene.add_entity(acadrust::EntityType::LwPolyline(poly));
        let mut rec = ExtendedDataRecord::new(AEC_APPID);
        rec.values = wall_record_for_wall(&wall);
        write_aec_record(&mut scene.document, handle, rec);

        // Bind Base and Top planes
        apply_wall_plane_choice(&mut scene, Some(&project), handle, true, "EG_ELEVATION", None);
        apply_wall_plane_choice(&mut scene, Some(&project), handle, false, "ShedRoof", None);

        let w_bound = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        assert_eq!(w_bound.top_plane_id, Some(roof_id));
        assert!(w_bound.is_sloped());
        assert_eq!(w_bound.top_facets.len(), 1);

        // Explicit unbind to "(none)"
        apply_wall_plane_choice(&mut scene, Some(&project), handle, false, "(none)", None);

        let w_unbound = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        assert_eq!(w_unbound.top_plane_id, None);
        assert_eq!(w_unbound.top_plane_name, None);
        assert!(!w_unbound.is_sloped());
        assert!(w_unbound.top_facets.is_empty());
        assert_eq!(w_unbound.top_normal, [0.0, 0.0, 1.0]);
        assert_eq!(w_unbound.top_origin[2], w_unbound.base_origin[2] + w_unbound.height);
    }

    #[test]
    fn delete_control_plane_from_storey_removes_custom_plane_and_refuses_floor_ceiling() {
        use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};

        let mut eg = StoreyRef::new_with_height("EG", 0.0, 2.80, "eg.dwg");
        let floor_id = eg.floor_plane_id;
        let ceiling_id = eg.ceiling_plane_id;

        let f = ControlPlaneFacet::new("Facet", vec![[0.0, 0.0, 3.0], [5.0, 0.0, 3.0], [5.0, 5.0, 3.0], [0.0, 5.0, 3.0]]);
        let cp = ControlPlane::from_facets("CustomPlane", vec![f]);
        let cp_id = cp.id;
        eg.control_planes.push(cp);

        // Refuse to delete floor or ceiling plane
        assert!(!delete_control_plane_from_storey(&mut eg, floor_id));
        assert!(!delete_control_plane_from_storey(&mut eg, ceiling_id));
        assert!(eg.plane(floor_id).is_some());
        assert!(eg.plane(ceiling_id).is_some());

        // Successfully delete custom plane
        assert!(delete_control_plane_from_storey(&mut eg, cp_id));
        assert!(eg.plane(cp_id).is_none());

        // Deleting non-existent plane returns false
        assert!(!delete_control_plane_from_storey(&mut eg, cp_id));
    }

    #[test]
    fn test_dropdown_plane_formatting_and_disambiguation() {
        use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};
        use crate::modules::aec::engine::project::{Building, ProjectFile};

        let mut project = ProjectFile::default();
        let mut building = Building::new("Hauptgebaeude");
        let mut eg = StoreyRef::new_with_height("EG", 0.0, 2.80, "eg.dwg");
        let mut og = StoreyRef::new_with_height("OG", 2.80, 2.80, "og.dwg");

        // Two custom planes with identical name "Floor" in different storeys
        let eg_custom_floor = ControlPlane::horizontal("Floor", 0.0);
        let eg_custom_floor_id = eg_custom_floor.id;
        eg.control_planes.push(eg_custom_floor);

        let og_custom_floor = ControlPlane::horizontal("Floor", 2.80);
        let og_custom_floor_id = og_custom_floor.id;
        og.control_planes.push(og_custom_floor);

        // Custom sloped roof on OG with bracketed name
        let facet_roof = ControlPlaneFacet::new(
            "NordSeite",
            vec![
                [0.0, 0.0, 6.0],
                [10.0, 0.0, 6.0],
                [10.0, 10.0, 10.0],
                [0.0, 10.0, 10.0],
            ],
        );
        let roof_slope = facet_roof.slope_degrees();
        assert!(roof_slope >= 0.1);
        let roof_cp = ControlPlane::from_facets("Dach (Nord)", vec![facet_roof]);
        let roof_id = roof_cp.id;
        og.control_planes.push(roof_cp);

        // Multi-facet dormer plane
        let f1 = ControlPlaneFacet::new("F1", vec![[0.0, 0.0, 7.0], [5.0, 0.0, 7.0], [5.0, 5.0, 8.0], [0.0, 5.0, 8.0]]);
        let f2 = ControlPlaneFacet::new("F2", vec![[5.0, 0.0, 7.0], [10.0, 0.0, 7.0], [10.0, 5.0, 8.0], [5.0, 5.0, 8.0]]);
        let multi_cp = ControlPlane::from_facets("Gaube", vec![f1, f2]);
        let multi_id = multi_cp.id;
        og.control_planes.push(multi_cp);

        let eg_elevation_id = eg.floor_plane_id;

        building.storeys.push(eg);
        building.storeys.push(og);
        project.buildings.push(building);

        let planes = collect_project_planes(&project);
        let eg_floor_info = planes.iter().find(|p| p.id == eg_custom_floor_id).unwrap();
        assert_eq!(eg_floor_info.disambiguated_name, "Floor [EG]");
        assert_eq!(eg_floor_info.formatted_label, "Floor [EG] (Z = 0.000 m)");

        let og_floor_info = planes.iter().find(|p| p.id == og_custom_floor_id).unwrap();
        assert_eq!(og_floor_info.disambiguated_name, "Floor [OG]");
        assert_eq!(og_floor_info.formatted_label, "Floor [OG] (Z = 2.800 m)");

        let eg_elev_info = planes.iter().find(|p| p.id == eg_elevation_id).unwrap();
        assert_eq!(eg_elev_info.disambiguated_name, "EG_ELEVATION");
        assert_eq!(eg_elev_info.formatted_label, "EG_ELEVATION (Z = 0.000 m)");

        let roof_info = planes.iter().find(|p| p.id == roof_id).unwrap();
        assert_eq!(roof_info.disambiguated_name, "Dach (Nord)");
        assert!(roof_info.formatted_label.starts_with("Dach (Nord) (Z ≈ 6.000 m, "));
        assert!(roof_info.formatted_label.ends_with("°)"));

        let multi_info = planes.iter().find(|p| p.id == multi_id).unwrap();
        assert_eq!(multi_info.disambiguated_name, "Gaube");
        assert!(multi_info.formatted_label.starts_with("Gaube (Multi, Z ≈ 7.000 m, "));

        let options = build_project_plane_options(Some(&project));
        assert_eq!(options[0], "(none)");
        assert!(options.contains(&"Floor [EG] (Z = 0.000 m)".to_string()));
        assert!(options.contains(&"Floor [OG] (Z = 2.800 m)".to_string()));
        assert!(options.contains(&"EG_ELEVATION (Z = 0.000 m)".to_string()));
        assert!(options.contains(&roof_info.formatted_label));
        assert!(options.contains(&multi_info.formatted_label));

        // resolve_plane_display_label produces identical string
        assert_eq!(
            resolve_plane_display_label(Some(&project), Some(eg_custom_floor_id), None),
            "Floor [EG] (Z = 0.000 m)"
        );
        assert_eq!(
            resolve_plane_display_label(Some(&project), Some(roof_id), None),
            roof_info.formatted_label
        );
        assert_eq!(
            resolve_plane_display_label(Some(&project), None, None),
            "(none)"
        );
    }

    #[test]
    fn test_parse_plane_choice_resilience_and_stale_labels() {
        use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};
        use crate::modules::aec::engine::project::{Building, ProjectFile};

        let mut project = ProjectFile::default();
        let mut building = Building::new("Haus");
        let mut eg = StoreyRef::new_with_height("EG", 0.0, 2.80, "eg.dwg");
        let mut og = StoreyRef::new_with_height("OG", 2.80, 2.80, "og.dwg");

        let eg_custom_floor = ControlPlane::horizontal("Floor", 0.0);
        let eg_floor_id = eg_custom_floor.id;
        eg.control_planes.push(eg_custom_floor);

        let og_custom_floor = ControlPlane::horizontal("Floor", 2.80);
        let og_floor_id = og_custom_floor.id;
        og.control_planes.push(og_custom_floor);

        let facet_roof = ControlPlaneFacet::new(
            "NordSeite",
            vec![
                [0.0, 0.0, 6.0],
                [10.0, 0.0, 6.0],
                [10.0, 10.0, 10.0],
                [0.0, 10.0, 10.0],
            ],
        );
        let roof_cp = ControlPlane::from_facets("Dach (Nord)", vec![facet_roof]);
        let roof_id = roof_cp.id;
        og.control_planes.push(roof_cp);

        let eg_elevation_id = eg.floor_plane_id;

        building.storeys.push(eg);
        building.storeys.push(og);
        project.buildings.push(building);

        // 1. Exact match
        assert_eq!(
            parse_plane_choice(Some(&project), "Floor [EG] (Z = 0.000 m)"),
            Some(eg_floor_id)
        );
        assert_eq!(
            parse_plane_choice(Some(&project), "Floor [OG] (Z = 2.800 m)"),
            Some(og_floor_id)
        );
        assert_eq!(
            parse_plane_choice(Some(&project), "EG_ELEVATION (Z = 0.000 m)"),
            Some(eg_elevation_id)
        );

        // 2. Disambiguated name without Z
        assert_eq!(
            parse_plane_choice(Some(&project), "Floor [EG]"),
            Some(eg_floor_id)
        );
        assert_eq!(
            parse_plane_choice(Some(&project), "Floor [OG]"),
            Some(og_floor_id)
        );

        // 3. Ambiguous name returns None
        assert_eq!(parse_plane_choice(Some(&project), "Floor"), None);

        // 4. Bracketed name with Z suffix
        let roof_label = resolve_plane_display_label(Some(&project), Some(roof_id), None);
        assert_eq!(
            parse_plane_choice(Some(&project), &roof_label),
            Some(roof_id)
        );

        // 5. Pure unique bracketed name
        assert_eq!(
            parse_plane_choice(Some(&project), "Dach (Nord)"),
            Some(roof_id)
        );

        // 6. Stale label after height/slope change
        assert_eq!(
            parse_plane_choice(Some(&project), "Floor [EG] (Z = 9.999 m)"),
            Some(eg_floor_id)
        );
        assert_eq!(
            parse_plane_choice(Some(&project), "Dach (Nord) (Z ≈ 1.234 m, 12.0°)"),
            Some(roof_id)
        );

        // 7. Unbound labels
        assert_eq!(parse_plane_choice(Some(&project), "(none)"), None);
        assert_eq!(parse_plane_choice(Some(&project), ""), None);

        // 8. UUID fallback
        assert_eq!(
            parse_plane_choice(Some(&project), &roof_id.to_string()),
            Some(roof_id)
        );
    }

    #[test]
    fn test_apply_wall_plane_choice_stores_pure_name_in_xdata() {
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2;
        use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};
        use crate::modules::aec::engine::project::{Building, ProjectFile};

        let mut project = ProjectFile::default();
        let mut building = Building::new("Haus");
        let mut eg = StoreyRef::new_with_height("EG", 0.0, 2.80, "eg.dwg");
        let mut og = StoreyRef::new_with_height("OG", 2.80, 2.80, "og.dwg");

        let eg_custom_floor = ControlPlane::horizontal("Floor", 0.0);
        eg.control_planes.push(eg_custom_floor);
        let og_custom_floor = ControlPlane::horizontal("Floor", 2.80);
        og.control_planes.push(og_custom_floor);

        let facet_roof = ControlPlaneFacet::new(
            "NordSeite",
            vec![
                [0.0, 0.0, 6.0],
                [10.0, 0.0, 6.0],
                [10.0, 10.0, 10.0],
                [0.0, 10.0, 10.0],
            ],
        );
        let roof_cp = ControlPlane::from_facets("Dach (Nord)", vec![facet_roof]);
        og.control_planes.push(roof_cp);

        building.storeys.push(eg);
        building.storeys.push(og);
        project.buildings.push(building);

        let mut scene = Scene::new();
        let wall = Wall::new("standard_wall", 2.50, 0);
        let mut poly = LwPolyline::new();
        poly.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let handle = scene.add_entity(acadrust::EntityType::LwPolyline(poly));
        let mut rec = ExtendedDataRecord::new(AEC_APPID);
        rec.values = wall_record_for_wall(&wall);
        write_aec_record(&mut scene.document, handle, rec);

        // Apply formatted choice with storey disambiguation and Z value
        let ok_base = apply_wall_plane_choice(
            &mut scene,
            Some(&project),
            handle,
            true,
            "Floor [EG] (Z = 0.000 m)",
            None,
        );
        assert!(ok_base);
        let w_base = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        // XDATA must store pure name "Floor", NOT "Floor [EG] (Z = 0.000 m)"
        assert_eq!(w_base.base_plane_name, Some("Floor".to_string()));

        // Apply formatted choice with bracketed name and Z value
        let ok_top = apply_wall_plane_choice(
            &mut scene,
            Some(&project),
            handle,
            false,
            "Dach (Nord) (Z ≈ 6.000 m, 21.8°)",
            None,
        );
        assert!(ok_top);
        let w_top = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        // XDATA must store pure name "Dach (Nord)", NOT "Dach (Nord) (Z ≈ ...)"
        assert_eq!(w_top.top_plane_name, Some("Dach (Nord)".to_string()));

        // Explicit unbind
        apply_wall_plane_choice(&mut scene, Some(&project), handle, false, "(none)", None);
        let w_unbound = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        assert_eq!(w_unbound.top_plane_name, None);
        assert_eq!(w_unbound.top_plane_id, None);
    }
}
