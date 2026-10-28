//! Bind/unbind slab base/top control planes from the properties panel.

use acadrust::Handle;

use crate::modules::aec::engine::library::StyleLibrary;
use crate::modules::aec::engine::project::ProjectFile;
use crate::modules::aec::engine::slab::{SLAB_BASE_OFFSET_KEYS, SLAB_TOP_OFFSET_KEYS};
use crate::modules::aec::engine::slab_package::resolve_slab_package;
use crate::modules::aec::engine::slab_regen::regenerate_slab_representation;
use crate::modules::aec::engine::slab_xdata::{slab_from_entity, write_slab_model};
use crate::modules::aec::project::wall_planes::{
    parse_plane_choice, resolve_plane_stored_name,
};
use crate::scene::Scene;

/// Applies a base/top control-plane choice to a slab and regenerates it.
pub fn apply_slab_plane_choice(
    scene: &mut Scene,
    project: Option<&ProjectFile>,
    handle: Handle,
    base: bool,
    choice: &str,
    library: Option<&StyleLibrary>,
    rules: Option<&crate::modules::aec::engine::display_component::ComponentRuleSet>,
) -> bool {
    let handle = resolve_slab_package(scene, handle);
    let Some(entity) = scene.document.get_entity(handle) else {
        return false;
    };
    let (x, y) = match entity {
        acadrust::EntityType::LwPolyline(pl) if !pl.vertices.is_empty() => {
            (pl.vertices[0].location.x, pl.vertices[0].location.y)
        }
        _ => (0.0, 0.0),
    };
    let Some(mut slab) = slab_from_entity(entity) else {
        return false;
    };
    let id = parse_plane_choice(project, choice);
    let name = resolve_plane_stored_name(project, id, choice);
    if base {
        slab.base_plane_id = id;
        slab.base_plane_name = name;
        if id.is_none() {
            // Note: When base_plane_id is None and base_facets is empty, Slab::bottom_z_at_xy deliberately
            // falls back to (top_z_at_xy - total_thickness + base_offset) for standard thickness-derived bottom elevation.
            let prior_ref_z = slab
                .eval_facets_or_plane(&slab.base_facets, slab.base_origin, slab.base_normal, x, y)
                .unwrap_or(slab.base_origin[2]);
            slab.base_facets.clear();
            slab.base_normal = [0.0, 0.0, 1.0];
            slab.base_origin = [x, y, prior_ref_z + slab.base_offset];
        }
        slab.base_offset = 0.0;
        slab.formulas.remove_all(SLAB_BASE_OFFSET_KEYS);
    } else {
        slab.top_plane_id = id;
        slab.top_plane_name = name;
        if id.is_none() {
            let prior_ref_z = slab
                .eval_facets_or_plane(&slab.top_facets, slab.top_origin, slab.top_normal, x, y)
                .unwrap_or(slab.top_origin[2]);
            slab.top_facets.clear();
            slab.top_normal = [0.0, 0.0, 1.0];
            slab.top_origin = [x, y, prior_ref_z + slab.top_offset];
        }
        slab.top_offset = 0.0;
        slab.formulas.remove_all(SLAB_TOP_OFFSET_KEYS);
    }
    if let Some(project) = project {
        slab.rebake_from_project(project, x, y);
    }
    if !write_slab_model(scene, handle, &slab) {
        return false;
    }
    regenerate_slab_representation(scene, handle, library, rules)
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};
    use crate::modules::aec::engine::project::{Building, StoreyRef};
    use crate::modules::aec::engine::slab::{Slab, SlabLayer};
    use crate::modules::aec::engine::wall_style::LayerFunction;

    #[test]
    fn slab_explicit_unbound_clears_plane_id_and_facets() {
        let mut project = ProjectFile::default();
        let mut building = Building::new("Haus");
        let mut eg = StoreyRef::new_with_height("EG", 0.0, 2.80, "eg.dwg");

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
        let slab = Slab::new("standard_slab", 0);

        let mut poly = LwPolyline::new();
        poly.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(10.0, 5.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
        let handle = scene.add_entity(acadrust::EntityType::LwPolyline(poly));
        write_slab_model(&mut scene, handle, &slab);

        // Bind slab top plane to ShedRoof
        apply_slab_plane_choice(&mut scene, Some(&project), handle, false, "ShedRoof", None, None);

        // Set top_offset and formula on the bound slab (e.g. from user input)
        let mut s_bound = slab_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        s_bound.top_offset = 0.25;
        s_bound.formulas.set("slab_top_offset", "_OFFSET + 0.05".to_string());
        write_slab_model(&mut scene, handle, &s_bound);

        assert_eq!(s_bound.top_plane_id, Some(roof_id));
        assert_eq!(s_bound.top_facets.len(), 1);
        assert!((s_bound.top_offset - 0.25).abs() < 1e-9);
        assert_eq!(s_bound.formulas.get("slab_top_offset"), Some("_OFFSET + 0.05"));
        // Sloped top plane values + offset (0.25): Z=4.25 at x=0, Z=5.25 at x=5, Z=6.25 at x=10
        assert!((s_bound.top_z_at_xy(0.0, 0.0, 0.0) - 4.25).abs() < 1e-9);
        assert!((s_bound.top_z_at_xy(5.0, 0.0, 0.0) - 5.25).abs() < 1e-9);
        assert!((s_bound.top_z_at_xy(10.0, 0.0, 0.0) - 6.25).abs() < 1e-9);

        let prior_ref_top_z = s_bound.top_z_at_xy(5.0, 0.0, 0.0);

        // Explicit unbind to "(none)"
        apply_slab_plane_choice(&mut scene, Some(&project), handle, false, "(none)", None, None);
        let s_unbound = slab_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        assert_eq!(s_unbound.top_plane_id, None);
        assert_eq!(s_unbound.top_plane_name, None);
        assert!(s_unbound.top_facets.is_empty());
        assert_eq!(s_unbound.top_normal, [0.0, 0.0, 1.0]);
        assert_eq!(s_unbound.top_offset, 0.0);
        assert_eq!(s_unbound.formulas.get("slab_top_offset"), None);
        // The previous top_offset is folded into top_origin, so the reference XY elevation is strictly continuous
        assert!((s_unbound.top_origin[2] - prior_ref_top_z).abs() < 1e-9);
        // After unbind: top surface is flat at Z=5.25 across all XY coordinates (no jump)
        assert!((s_unbound.top_z_at_xy(0.0, 0.0, 0.0) - 5.25).abs() < 1e-9);
        assert!((s_unbound.top_z_at_xy(5.0, 0.0, 0.0) - 5.25).abs() < 1e-9);
        assert!((s_unbound.top_z_at_xy(10.0, 0.0, 0.0) - 5.25).abs() < 1e-9);
    }

    #[test]
    fn slab_base_explicit_unbound_returns_to_thickness_derived_bottom() {
        let mut project = ProjectFile::default();
        let mut building = Building::new("Haus");
        let mut eg = StoreyRef::new_with_height("EG", 0.0, 2.80, "eg.dwg");

        let base_cp = ControlPlane::horizontal("CustomBasePlane", 1.50);
        let base_id = base_cp.id;
        eg.control_planes.push(base_cp);
        building.storeys.push(eg);
        project.buildings.push(building);

        let mut scene = Scene::new();
        let mut slab = Slab::new("standard_slab", 0);
        slab.layers = vec![
            SlabLayer::new("Concrete", 0.20, LayerFunction::Structural),
            SlabLayer::new("Screed", 0.05, LayerFunction::Finish),
        ]; // Total thickness = 0.25
        slab.top_origin = [0.0, 0.0, 3.00];

        let mut poly = LwPolyline::new();
        poly.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(0.0, 5.0)));
        let handle = scene.add_entity(acadrust::EntityType::LwPolyline(poly));
        write_slab_model(&mut scene, handle, &slab);

        // 1. Bind base plane to CustomBasePlane (Z=1.50)
        apply_slab_plane_choice(&mut scene, Some(&project), handle, true, "CustomBasePlane", None, None);
        let s_bound = slab_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        assert_eq!(s_bound.base_plane_id, Some(base_id));
        assert!((s_bound.bottom_z_at_xy(0.0, 0.0, 0.0) - 1.50).abs() < 1e-9);

        // 2. Explicit unbind base plane to "(none)"
        apply_slab_plane_choice(&mut scene, Some(&project), handle, true, "(none)", None, None);
        let s_unbound = slab_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        assert_eq!(s_unbound.base_plane_id, None);
        assert_eq!(s_unbound.base_plane_name, None);
        assert!(s_unbound.base_facets.is_empty());
        assert_eq!(s_unbound.base_normal, [0.0, 0.0, 1.0]);
        // Bottom elevation deliberately returns to thickness-derived: top_z (3.00) - thickness (0.25) = 2.75
        let expected_bottom_z = s_unbound.top_z_at_xy(0.0, 0.0, 0.0) - s_unbound.total_thickness();
        assert!((s_unbound.bottom_z_at_xy(0.0, 0.0, 0.0) - expected_bottom_z).abs() < 1e-9);
        assert!((s_unbound.bottom_z_at_xy(0.0, 0.0, 0.0) - 2.75).abs() < 1e-9);
    }

    #[test]
    fn test_apply_slab_plane_choice_stores_pure_name_in_xdata() {
        let mut project = ProjectFile::default();
        let mut building = Building::new("Haus");
        let mut eg = StoreyRef::new_with_height("EG", 0.0, 2.80, "eg.dwg");
        let mut og = StoreyRef::new_with_height("OG", 2.80, 2.80, "og.dwg");

        let eg_custom_floor = ControlPlane::horizontal("Floor", 0.0);
        eg.control_planes.push(eg_custom_floor);
        let og_custom_floor = ControlPlane::horizontal("Floor", 2.80);
        og.control_planes.push(og_custom_floor);

        let roof_cp = ControlPlane::horizontal("Dach (Sued)", 5.60);
        og.control_planes.push(roof_cp);
        building.storeys.push(eg);
        building.storeys.push(og);
        project.buildings.push(building);

        let mut scene = Scene::new();
        let slab = Slab::new("standard_slab", 0);
        let mut poly = LwPolyline::new();
        poly.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(0.0, 5.0)));
        let handle = scene.add_entity(acadrust::EntityType::LwPolyline(poly));
        write_slab_model(&mut scene, handle, &slab);

        // Apply formatted choice with storey disambiguation
        apply_slab_plane_choice(
            &mut scene,
            Some(&project),
            handle,
            false,
            "Floor [EG] (Z = 0.000 m)",
            None,
            None,
        );
        let s_bound = slab_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        assert_eq!(s_bound.top_plane_name, Some("Floor".to_string()));

        // Apply formatted choice with bracketed name
        apply_slab_plane_choice(
            &mut scene,
            Some(&project),
            handle,
            false,
            "Dach (Sued) (Z = 5.600 m)",
            None,
            None,
        );
        let s_bound_roof = slab_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        assert_eq!(s_bound_roof.top_plane_name, Some("Dach (Sued)".to_string()));
    }
}
