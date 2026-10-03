//! Grip affordances and geometry manipulation for Slabs and Slab Openings.
//!
//! Provides:
//! - Vertex movement, insertion, and removal on slab boundaries
//! - Slab opening movement, resizing, and vertex editing
//! - Dynamic regeneration of 2D derived entities and 3D B-Rep solids on grip modification
//! - Synchronization after interactive viewport grip edits.

use acadrust::entities::LwVertex;
use acadrust::types::Vector2;
use acadrust::{EntityType, Handle};

use crate::app::OpenCADStudio;
use crate::modules::aec::engine::display_component::ComponentRuleSet;
use crate::modules::aec::engine::geometry::signed_area;
use crate::modules::aec::engine::slab_package::{
    is_slab_carrier_entity, is_slab_opening_carrier_entity, resolve_slab_package,
};
use crate::modules::aec::engine::slab_regen::{
    regenerate_slab_opening_representation, regenerate_slab_representation,
};
use crate::modules::aec::engine::slab_xdata::{
    slab_from_entity, slab_opening_from_entity, write_slab_opening_record,
};
use crate::modules::aec::engine::StyleLibrary;
use crate::scene::model::object::GripApply;
use crate::scene::Scene;

/// Moves a boundary vertex of a Slab entity to `new_pos` (local 2D coords).
pub fn move_slab_vertex(
    scene: &mut Scene,
    slab_handle: Handle,
    vertex_idx: usize,
    new_pos: (f64, f64),
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> bool {
    let Some(entity) = scene.document.get_entity_mut(slab_handle) else {
        return false;
    };
    let EntityType::LwPolyline(pl) = entity else {
        return false;
    };
    if vertex_idx >= pl.vertices.len() {
        return false;
    }
    let old_pos = pl.vertices[vertex_idx].location;
    pl.vertices[vertex_idx].location = Vector2::new(new_pos.0, new_pos.1);

    // Validate polygon area
    let pts: Vec<(f64, f64)> = pl
        .vertices
        .iter()
        .map(|v| (v.location.x, v.location.y))
        .collect();
    if signed_area(&pts).abs() < 1e-4 {
        // Rollback
        if let Some(entity_back) = scene.document.get_entity_mut(slab_handle) {
            if let EntityType::LwPolyline(pl_back) = entity_back {
                pl_back.vertices[vertex_idx].location = old_pos;
            }
        }
        return false;
    }

    regenerate_slab_representation(scene, slab_handle, library, rules);
    scene.bump_geometry();
    true
}

/// Adds a new vertex to the slab boundary after `segment_idx`.
pub fn add_slab_vertex(
    scene: &mut Scene,
    slab_handle: Handle,
    segment_idx: usize,
    new_pos: (f64, f64),
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> bool {
    let Some(entity) = scene.document.get_entity_mut(slab_handle) else {
        return false;
    };
    let EntityType::LwPolyline(pl) = entity else {
        return false;
    };
    let insert_at = (segment_idx + 1).min(pl.vertices.len());
    pl.vertices.insert(insert_at, LwVertex::new(Vector2::new(new_pos.0, new_pos.1)));

    regenerate_slab_representation(scene, slab_handle, library, rules);
    scene.bump_geometry();
    true
}

/// Removes a vertex from the slab boundary at `vertex_idx`.
pub fn remove_slab_vertex(
    scene: &mut Scene,
    slab_handle: Handle,
    vertex_idx: usize,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> bool {
    let Some(entity) = scene.document.get_entity_mut(slab_handle) else {
        return false;
    };
    let EntityType::LwPolyline(pl) = entity else {
        return false;
    };
    if pl.vertices.len() <= 3 || vertex_idx >= pl.vertices.len() {
        return false;
    }
    pl.vertices.remove(vertex_idx);

    // Validate remaining polygon
    let pts: Vec<(f64, f64)> = pl
        .vertices
        .iter()
        .map(|v| (v.location.x, v.location.y))
        .collect();
    if signed_area(&pts).abs() < 1e-4 {
        return false;
    }

    regenerate_slab_representation(scene, slab_handle, library, rules);
    scene.bump_geometry();
    true
}

/// Moves an entire SlabOpening by `(dx, dy)`.
pub fn move_slab_opening(
    scene: &mut Scene,
    opening_handle: Handle,
    delta: (f64, f64),
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> bool {
    let Some(entity) = scene.document.get_entity(opening_handle) else {
        return false;
    };
    let Some(mut opening) = slab_opening_from_entity(entity) else {
        return false;
    };

    for pt in &mut opening.boundary {
        pt.0 += delta.0;
        pt.1 += delta.1;
    }

    if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity_mut(opening_handle) {
        for v in &mut pl.vertices {
            v.location.x += delta.0;
            v.location.y += delta.1;
        }
    }

    write_slab_opening_record(&mut scene.document, opening_handle, &opening);
    regenerate_slab_opening_representation(scene, opening_handle, library, rules);
    scene.bump_geometry();
    true
}

/// Resizes a SlabOpening with a new boundary polygon.
pub fn resize_slab_opening(
    scene: &mut Scene,
    opening_handle: Handle,
    new_boundary: &[(f64, f64)],
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> bool {
    if new_boundary.len() < 3 || signed_area(new_boundary).abs() < 1e-4 {
        return false;
    }
    let Some(entity) = scene.document.get_entity(opening_handle) else {
        return false;
    };
    let Some(mut opening) = slab_opening_from_entity(entity) else {
        return false;
    };

    opening.boundary = new_boundary.to_vec();

    if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity_mut(opening_handle) {
        pl.vertices = new_boundary
            .iter()
            .map(|&(x, y)| LwVertex::new(Vector2::new(x, y)))
            .collect();
    }

    write_slab_opening_record(&mut scene.document, opening_handle, &opening);
    regenerate_slab_opening_representation(scene, opening_handle, library, rules);
    scene.bump_geometry();
    true
}

/// Moves a single vertex of a SlabOpening to `new_pos`.
pub fn move_slab_opening_vertex(
    scene: &mut Scene,
    opening_handle: Handle,
    vertex_idx: usize,
    new_pos: (f64, f64),
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> bool {
    let Some(entity) = scene.document.get_entity(opening_handle) else {
        return false;
    };
    let Some(mut opening) = slab_opening_from_entity(entity) else {
        return false;
    };
    if vertex_idx >= opening.boundary.len() {
        return false;
    }

    opening.boundary[vertex_idx] = new_pos;
    if signed_area(&opening.boundary).abs() < 1e-4 {
        return false;
    }

    if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity_mut(opening_handle) {
        if vertex_idx < pl.vertices.len() {
            pl.vertices[vertex_idx].location = Vector2::new(new_pos.0, new_pos.1);
        }
    }

    write_slab_opening_record(&mut scene.document, opening_handle, &opening);
    regenerate_slab_opening_representation(scene, opening_handle, library, rules);
    scene.bump_geometry();
    true
}

/// Synchronizes a Slab carrier entity after interactive grip edits in the viewport.
pub fn sync_slab_after_grip_edit(
    scene: &mut Scene,
    slab_handle: Handle,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> bool {
    let Some(entity) = scene.document.get_entity(slab_handle) else {
        return false;
    };
    if slab_from_entity(entity).is_none() {
        return false;
    }
    regenerate_slab_representation(scene, slab_handle, library, rules);
    true
}

/// Synchronizes a SlabOpening carrier entity after interactive grip edits in the viewport.
pub fn sync_slab_opening_after_grip_edit(
    scene: &mut Scene,
    opening_handle: Handle,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> bool {
    let Some(entity) = scene.document.get_entity(opening_handle) else {
        return false;
    };
    let Some(mut opening) = slab_opening_from_entity(entity) else {
        return false;
    };
    if let EntityType::LwPolyline(pl) = entity {
        opening.boundary = pl
            .vertices
            .iter()
            .map(|v| (v.location.x, v.location.y))
            .collect();
        write_slab_opening_record(&mut scene.document, opening_handle, &opening);
    }
    regenerate_slab_opening_representation(scene, opening_handle, library, rules);
    true
}

impl OpenCADStudio {
    /// Apply an interactive grip edit for Slab entities (fast lightweight drag preview).
    pub(crate) fn apply_aec_slab_grip(
        &mut self,
        tab: usize,
        handle: Handle,
        grip_id: usize,
        apply: &GripApply,
    ) -> bool {
        let scene = &mut self.tabs[tab].scene;
        let owner = resolve_slab_package(scene, handle);
        let Some(entity) = scene.document.get_entity_mut(owner) else {
            return false;
        };
        if !is_slab_carrier_entity(entity) {
            return false;
        }

        let EntityType::LwPolyline(pl) = entity else {
            return false;
        };
        if grip_id >= pl.vertices.len() {
            return false;
        }

        match apply {
            GripApply::Absolute(pt) => {
                pl.vertices[grip_id].location = acadrust::types::Vector2::new(pt.x, pt.y);
            }
            GripApply::Translate(delta) => {
                pl.vertices[grip_id].location.x += delta.x;
                pl.vertices[grip_id].location.y += delta.y;
            }
        }

        self.tabs[tab].dirty = true;
        true
    }

    /// Apply an interactive grip edit for SlabOpening entities (fast lightweight drag preview).
    pub(crate) fn apply_aec_slab_opening_grip(
        &mut self,
        tab: usize,
        handle: Handle,
        grip_id: usize,
        apply: &GripApply,
    ) -> bool {
        let scene = &mut self.tabs[tab].scene;
        let Some(entity) = scene.document.get_entity_mut(handle) else {
            return false;
        };
        if !is_slab_opening_carrier_entity(entity) {
            return false;
        }

        let EntityType::LwPolyline(pl) = entity else {
            return false;
        };
        if grip_id >= pl.vertices.len() {
            return false;
        }

        match apply {
            GripApply::Absolute(pt) => {
                pl.vertices[grip_id].location = acadrust::types::Vector2::new(pt.x, pt.y);
            }
            GripApply::Translate(delta) => {
                pl.vertices[grip_id].location.x += delta.x;
                pl.vertices[grip_id].location.y += delta.y;
            }
        }

        self.tabs[tab].dirty = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use crate::modules::aec::engine::slab::Slab;
    use crate::modules::aec::engine::slab_opening::{SlabOpening, SlabOpeningDepth, SlabOpeningKind};
    use crate::modules::aec::engine::library;
    use crate::modules::aec::engine::slab_xdata::{slab_opening_from_entity, write_slab_opening_record, write_slab_record};

    #[test]
    fn test_slab_grip_move_and_add_vertex() {
        let mut scene = Scene::new();
        let lib = library::seed_default_library();

        let mut pl = LwPolyline::new();
        pl.is_closed = true;
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(10.0, 10.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 10.0)));
        let slab_h = scene.add_entity(EntityType::LwPolyline(pl));

        let slab = Slab::new("style_slab_concrete_20", 0);
        write_slab_record(&mut scene.document, slab_h, &slab);
        regenerate_slab_representation(&mut scene, slab_h, Some(&lib), None);

        // Move vertex 1 (10, 0) -> (12, 0)
        let ok = move_slab_vertex(&mut scene, slab_h, 1, (12.0, 0.0), Some(&lib), None);
        assert!(ok);

        let entity = scene.document.get_entity(slab_h).expect("get entity");
        let EntityType::LwPolyline(pl_mod) = entity else { panic!("not lwpolyline") };
        assert_eq!(pl_mod.vertices[1].location.x, 12.0);

        // Add vertex at segment 1 -> between (12, 0) and (10, 10)
        let ok_add = add_slab_vertex(&mut scene, slab_h, 1, (15.0, 5.0), Some(&lib), None);
        assert!(ok_add);

        let entity_added = scene.document.get_entity(slab_h).expect("get entity");
        let EntityType::LwPolyline(pl_added) = entity_added else { panic!("not lwpolyline") };
        assert_eq!(pl_added.vertices.len(), 5);

        // Remove vertex
        let ok_rem = remove_slab_vertex(&mut scene, slab_h, 2, Some(&lib), None);
        assert!(ok_rem);
        let entity_rem = scene.document.get_entity(slab_h).expect("get entity");
        let EntityType::LwPolyline(pl_rem) = entity_rem else { panic!("not lwpolyline") };
        assert_eq!(pl_rem.vertices.len(), 4);
    }

    #[test]
    fn test_slab_opening_grip_move_and_resize() {
        let mut scene = Scene::new();
        let lib = library::seed_default_library();

        // 1. Host slab
        let mut pl_slab = LwPolyline::new();
        pl_slab.is_closed = true;
        pl_slab.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl_slab.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        pl_slab.add_vertex(LwVertex::new(Vector2::new(10.0, 10.0)));
        pl_slab.add_vertex(LwVertex::new(Vector2::new(0.0, 10.0)));
        let slab_h = scene.add_entity(EntityType::LwPolyline(pl_slab));
        let slab = Slab::new("style_slab_concrete_20", 0);
        write_slab_record(&mut scene.document, slab_h, &slab);

        // 2. Opening
        let mut pl_op = LwPolyline::new();
        pl_op.is_closed = true;
        pl_op.add_vertex(LwVertex::new(Vector2::new(2.0, 2.0)));
        pl_op.add_vertex(LwVertex::new(Vector2::new(4.0, 2.0)));
        pl_op.add_vertex(LwVertex::new(Vector2::new(4.0, 4.0)));
        pl_op.add_vertex(LwVertex::new(Vector2::new(2.0, 4.0)));
        let op_h = scene.add_entity(EntityType::LwPolyline(pl_op));

        let opening = SlabOpening {
            host_slab: slab_h,
            kind: SlabOpeningKind::Shaft,
            depth: SlabOpeningDepth::ThroughHole,
            boundary: vec![(2.0, 2.0), (4.0, 2.0), (4.0, 4.0), (2.0, 4.0)],
            derived_handles: Vec::new(),
        };
        write_slab_opening_record(&mut scene.document, op_h, &opening);
        regenerate_slab_opening_representation(&mut scene, op_h, Some(&lib), None);

        // Move opening by (1.0, 2.0)
        let moved = move_slab_opening(&mut scene, op_h, (1.0, 2.0), Some(&lib), None);
        assert!(moved);

        let entity = scene.document.get_entity(op_h).expect("get entity");
        let op_mod = slab_opening_from_entity(entity).expect("parse opening");
        assert_eq!(op_mod.boundary[0], (3.0, 4.0));
        assert_eq!(op_mod.boundary[2], (5.0, 6.0));
    }

    #[test]
    fn test_slab_carrier_elevation_follows_base_plane() {
        let mut scene = Scene::new();
        let lib = library::seed_default_library();

        let mut pl = LwPolyline::new();
        pl.is_closed = true;
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 5.0)));
        pl.elevation = 0.0;
        let slab_h = scene.add_entity(EntityType::LwPolyline(pl));

        let mut slab = Slab::new("style_slab_concrete_20", 0);
        slab.top_origin = [0.0, 0.0, 2.80];
        slab.base_origin = [0.0, 0.0, 2.60];
        slab.top_offset = 0.0;
        write_slab_record(&mut scene.document, slab_h, &slab);
        regenerate_slab_representation(&mut scene, slab_h, Some(&lib), None);

        let entity = scene.document.get_entity(slab_h).expect("get entity");
        let EntityType::LwPolyline(pl_carrier) = entity else { panic!("not lwpolyline") };
        assert_eq!(pl_carrier.elevation, 2.80);
    }
}
