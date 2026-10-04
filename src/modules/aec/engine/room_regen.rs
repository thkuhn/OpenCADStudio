//! 2D and 3D geometry regeneration for AEC `Room` entities.
//!
//! Generates DIN 1356-conforming room stamps (MText with room number, name, DIN 277 function,
//! measured gross/net floor area, perimeter, clear height, volume, and floor finish),
//! optional floor finish pattern hatches, and associative 3D finish solid volumes.

#![allow(unused_imports)]
use std::collections::HashMap;

use acadrust::entities::{LwPolyline, LwVertex, MText, Point};
use acadrust::types::{Color, Vector2, Vector3};
use acadrust::{CadDocument, EntityType, Handle};

use crate::scene::model::hatch_model::{FillPlane, HatchModel, HatchPattern};
use crate::scene::Scene;

use super::geometry::{area, centroid, perimeter, Polygon2D};
use super::library::StyleLibrary;
use super::owner_index;
use super::room::Room;
use super::room_package::*;
use super::room_xdata::{room_from_entity, write_room_record};

/// Extracts the 2D polygon vertices from a room carrier entity.
pub fn room_boundary_points(entity: &EntityType) -> Vec<(f64, f64)> {
    let EntityType::LwPolyline(pl) = entity else {
        return Vec::new();
    };
    let mut points: Vec<(f64, f64)> = pl
        .vertices
        .iter()
        .map(|v| (v.location.x, v.location.y))
        .collect();

    if points.len() >= 3 {
        let first = points[0];
        let last = points[points.len() - 1];
        if (first.0 - last.0).hypot(first.1 - last.1) < 1e-6 {
            points.pop();
        }
    }
    points
}

/// Constructs a HatchModel for a 2D polygon with specified pattern and styling.
fn create_polygon_hatch(
    poly_pts: &[(f64, f64)],
    origin: (f64, f64),
    base_z: f64,
    pat_name: &str,
    color: [f32; 4],
) -> Option<HatchModel> {
    if poly_pts.len() < 3 {
        return None;
    }
    let mut wcs: Vec<[f64; 2]> = poly_pts.iter().map(|p| [p.0, p.1]).collect();
    if let Some(first) = wcs.first().copied() {
        wcs.push(first);
    }
    let rel: Vec<[f32; 2]> = poly_pts
        .iter()
        .map(|p| [
            (p.0 - origin.0) as f32,
            (p.1 - origin.1) as f32,
        ])
        .collect();
    let families = crate::scene::model::hatch_patterns::find(pat_name)
        .and_then(|e| {
            if let HatchPattern::Pattern(f) = &e.gpu {
                Some(f.clone())
            } else {
                None
            }
        })
        .unwrap_or_default();
    Some(HatchModel {
        pattern_origin: None,
        render_instance: None,
        boundary: std::sync::Arc::new(rel.clone()),
        pattern: HatchPattern::Pattern(families),
        name: pat_name.to_string(),
        color,
        aci: 8,
        line_weight_px: 1.0,
        angle_offset: 0.0,
        scale: 1.0,
        world_origin: [origin.0, origin.1],
        boundary_wcs: Some(std::sync::Arc::new(wcs)),
        fill_plane: Some(FillPlane {
            origin: [origin.0, origin.1, base_z],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        }),
        fill_plane_boundary: Some(std::sync::Arc::new(rel)),
        boundary_exterior: None,
        boundary_sources: None,
        boundary_paths: None,
        style: acadrust::entities::HatchStyleType::Normal,
        draw_depth: 0.0,
    })
}

/// Regenerates all derived representations (stamp, hatches, 3D solids) for a room carrier.
pub fn regenerate_room_representation(
    scene: &mut Scene,
    room_handle: Handle,
    _lib: Option<&StyleLibrary>,
) -> Option<()> {
    let entity = scene.document.get_entity(room_handle)?.clone();
    let mut room = room_from_entity(&entity)?;
    let points = room_boundary_points(&entity);
    if points.len() < 3 {
        return None;
    }

    // Recalculate metrics from actual boundary geometry
    let raw_area = area(&points);
    let poly_perimeter = perimeter(&points);
    room.area = raw_area;
    room.perimeter = poly_perimeter;
    room.volume = room.effective_volume();

    // Determine stamp position (use saved coordinate if set, or calculate centroid)
    let stamp_pos = room.stamp_pos.unwrap_or_else(|| centroid(&points));
    room.stamp_pos = Some(stamp_pos);

    // Update XDATA on carrier entity
    write_room_record(&mut scene.document, room_handle, &room);

    // Remove old derived display children
    remove_room_display_children(scene, room_handle);

    let mut child_handles = Vec::new();

    // 1. Build Room Stamp MText
    let mut text_lines = Vec::new();
    let title = if room.number.is_empty() {
        room.name.clone()
    } else {
        format!("{} {}", room.number, room.name)
    };
    text_lines.push(title);

    // Area line (DIN 277 / WoFlV)
    if (room.factor - 1.0).abs() < 1e-4 {
        text_lines.push(format!("F: {:.2} m²", room.area));
    } else {
        let pct = (room.factor * 100.0).round() as i64;
        text_lines.push(format!(
            "F: {:.2} m² (WF: {:.2} m² / {}%)",
            room.area,
            room.calculated_area(),
            pct
        ));
    }

    // Perimeter and clear height
    let ceiling_tag = if room.ceiling_finish_thickness() > 0.0 { " (UKD)" } else { "" };
    text_lines.push(format!(
        "U: {:.2} m | RH: {:.2} m{}",
        room.perimeter,
        room.effective_ceiling_height(),
        ceiling_tag
    ));

    // Floor finish if present
    let finish_str = room.floor_finish_summary();
    if finish_str != "-" {
        text_lines.push(format!("Boden: {}", finish_str));
    }

    // Ceiling finish if present
    let ceiling_str = room.ceiling_finish_summary();
    if ceiling_str != "-" {
        text_lines.push(format!("Decke: {}", ceiling_str));
    }

    let mtext_value = text_lines.join("\\P");

    let mut mtext = MText::new();
    mtext.value = mtext_value;
    mtext.insertion_point = Vector3::new(stamp_pos.0, stamp_pos.1, room.base_z);
    mtext.height = 0.22;
    mtext.rectangle_width = 0.0;
    mtext.attachment_point = acadrust::entities::mtext::AttachmentPoint::MiddleCenter;
    mtext.style = String::new();

    scene.ensure_layer(AEC_ROOM_CARRIER_LAYER);
    scene.ensure_layer(AEC_ROOM_STAMP_LAYER);

    let mut stamp_entity = EntityType::MText(mtext);
    stamp_entity.common_mut().layer = AEC_ROOM_STAMP_LAYER.to_string();

    let stamp_handle = scene.add_entity(stamp_entity);
    write_room_display_tag(scene, stamp_handle, room_handle, ROOM_REP_ROLE_STAMP);
    child_handles.push(stamp_handle);

    // 2. Build floor transition zones and threshold lines at door openings
    let transitions = super::floor_transition::find_floor_transitions_for_room(scene, &points);
    scene.ensure_layer(AEC_ROOM_THRESHOLD_LAYER);
    for tr in &transitions {
        let mut thresh_pl = LwPolyline::new();
        thresh_pl.add_vertex(LwVertex::new(Vector2::new(
            tr.threshold_line.0 .0,
            tr.threshold_line.0 .1,
        )));
        thresh_pl.add_vertex(LwVertex::new(Vector2::new(
            tr.threshold_line.1 .0,
            tr.threshold_line.1 .1,
        )));
        thresh_pl.is_closed = false;
        let mut thresh_entity = EntityType::LwPolyline(thresh_pl);
        thresh_entity.common_mut().layer = AEC_ROOM_THRESHOLD_LAYER.to_string();
        let thresh_handle = scene.add_entity(thresh_entity);
        write_room_display_tag(scene, thresh_handle, room_handle, ROOM_REP_ROLE_THRESHOLD);
        child_handles.push(thresh_handle);
    }

    // 3. Build optional floor finish pattern hatch (main room + door transition zones)
    if let Some(finishes) = &room.floor_finish {
        if let Some(hatch_finish) = finishes.iter().find(|f| f.hatch_pattern.is_some()) {
            if let Some(pat_name) = &hatch_finish.hatch_pattern {
                scene.ensure_layer(AEC_ROOM_HATCH_LAYER);

                // Main room floor hatch
                if let Some(model) = create_polygon_hatch(
                    &points,
                    stamp_pos,
                    room.base_z,
                    pat_name,
                    [0.6, 0.6, 0.6, 0.7],
                ) {
                    let hatch_handle = scene.add_hatch(model, None, None);
                    if let Some(e) = scene.document.get_entity_mut(hatch_handle) {
                        e.as_entity_mut().set_layer(AEC_ROOM_HATCH_LAYER.to_string());
                        e.common_mut().owner_handle = room_handle;
                    }
                    write_room_display_tag(scene, hatch_handle, room_handle, ROOM_REP_ROLE_HATCH);
                    child_handles.push(hatch_handle);
                }

                // Extension hatches for door transition zones
                for tr in &transitions {
                    if let Some(model) = create_polygon_hatch(
                        &tr.polygon,
                        stamp_pos,
                        room.base_z,
                        pat_name,
                        [0.6, 0.6, 0.6, 0.7],
                    ) {
                        let hatch_handle = scene.add_hatch(model, None, None);
                        if let Some(e) = scene.document.get_entity_mut(hatch_handle) {
                            e.as_entity_mut().set_layer(AEC_ROOM_HATCH_LAYER.to_string());
                            e.common_mut().owner_handle = room_handle;
                        }
                        write_room_display_tag(scene, hatch_handle, room_handle, ROOM_REP_ROLE_HATCH);
                        child_handles.push(hatch_handle);
                    }
                }
            }
        }
    }

    // 4. Build optional ceiling finish pattern hatch (Reflected Ceiling Plan / Deckenspiegel)
    if let Some(finishes) = &room.ceiling_finish {
        if let Some(hatch_finish) = finishes.iter().find(|f| f.hatch_pattern.is_some()) {
            if let Some(pat_name) = &hatch_finish.hatch_pattern {
                scene.ensure_layer(AEC_ROOM_CEILING_HATCH_LAYER);
                if let Some(model) = create_polygon_hatch(
                    &points,
                    stamp_pos,
                    room.base_z + room.effective_ceiling_height(),
                    pat_name,
                    [0.4, 0.6, 0.8, 0.6],
                ) {
                    let hatch_handle = scene.add_hatch(model, None, None);
                    if let Some(e) = scene.document.get_entity_mut(hatch_handle) {
                        e.as_entity_mut().set_layer(AEC_ROOM_CEILING_HATCH_LAYER.to_string());
                        e.common_mut().owner_handle = room_handle;
                    }
                    write_room_display_tag(
                        scene,
                        hatch_handle,
                        room_handle,
                        ROOM_REP_ROLE_CEILING_HATCH,
                    );
                    child_handles.push(hatch_handle);
                }
            }
        }
    }

    // Register all child handles in owner index
    owner_index::set_children(&mut scene.document, room_handle, &child_handles);

    // Synchronize any existing room schedule tables in the drawing
    crate::modules::aec::rooms::schedule::update_all_room_schedules(scene);

    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::traits::EntityTypeOps;
    use crate::modules::aec::engine::room::RoomFinish;
    use crate::scene::model::object::GripMenuAction;

    #[test]
    fn test_room_add_and_remove_vertex() {
        let mut scene = Scene::new();
        let pts = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let mut pl = LwPolyline::new();
        for p in &pts {
            pl.add_vertex(LwVertex::new(Vector2::new(p.0, p.1)));
        }
        pl.is_closed = true;
        let room_h = scene.add_entity(EntityType::LwPolyline(pl));

        let mut room = Room::from_polygon("Wohnzimmer", &pts, 2.5, 0).with_number("EG-01");
        room.floor_finish = Some(vec![RoomFinish::new("Parkett", 0.015).with_hatch("ANSI31")]);
        write_room_record(&mut scene.document, room_h, &room);

        assert!(regenerate_room_representation(&mut scene, room_h, None).is_some());

        let room_before = room_from_entity(scene.document.get_entity(room_h).unwrap()).unwrap();
        assert!((room_before.area - 100.0).abs() < 1e-4);

        // 1. Add vertex at edge 0 midpoint (between (0,0) and (10,0)) -> new vertex at (5,0)
        let ent = scene.document.get_entity_mut(room_h).unwrap();
        ent.apply_grip_menu(4, GripMenuAction::AddVertex);
        let pl_after_add = match scene.document.get_entity(room_h).unwrap() {
            EntityType::LwPolyline(p) => p.clone(),
            _ => panic!("expected LwPolyline"),
        };
        assert_eq!(pl_after_add.vertices.len(), 5);

        // Move the added vertex (index 1) to (5, -2) to change the area
        let ent = scene.document.get_entity_mut(room_h).unwrap();
        if let EntityType::LwPolyline(pl) = ent {
            pl.vertices[1].location = Vector2::new(5.0, -2.0);
        }

        assert!(regenerate_room_representation(&mut scene, room_h, None).is_some());
        let room_after_move = room_from_entity(scene.document.get_entity(room_h).unwrap()).unwrap();
        // Area is now 100 + triangle (base 10, height 2 / 2 = 10) = 110.0
        assert!((room_after_move.area - 110.0).abs() < 1e-4);

        // 2. Remove vertex at index 1
        let ent = scene.document.get_entity_mut(room_h).unwrap();
        ent.apply_grip_menu(1, GripMenuAction::RemoveVertex);
        let pl_after_remove = match scene.document.get_entity(room_h).unwrap() {
            EntityType::LwPolyline(p) => p.clone(),
            _ => panic!("expected LwPolyline"),
        };
        assert_eq!(pl_after_remove.vertices.len(), 4);

        assert!(regenerate_room_representation(&mut scene, room_h, None).is_some());
        let room_after_remove_entity = room_from_entity(scene.document.get_entity(room_h).unwrap()).unwrap();
        assert!((room_after_remove_entity.area - 100.0).abs() < 1e-4);
    }

    #[test]
    fn test_room_regeneration_with_ceiling_finish_and_thresholds() {
        let mut scene = Scene::new();
        let pts = [(0.0, 0.0), (5.0, 0.0), (5.0, 4.0), (0.0, 4.0)];
        let mut pl = LwPolyline::new();
        for p in &pts {
            pl.add_vertex(LwVertex::new(Vector2::new(p.0, p.1)));
        }
        pl.is_closed = true;
        let room_h = scene.add_entity(EntityType::LwPolyline(pl));

        let mut room = Room::from_polygon("Office", &pts, 3.0, 0).with_number("101");
        room.floor_finish = Some(vec![RoomFinish::new("Tiles", 0.015).with_hatch("SQUARE")]);
        room.ceiling_finish = Some(vec![RoomFinish::new("Acoustic Grid", 0.20).with_hatch("ANSI31")]);
        write_room_record(&mut scene.document, room_h, &room);

        assert!(regenerate_room_representation(&mut scene, room_h, None).is_some());

        let children = collect_room_display_children(&scene, room_h);
        // Stamp (1) + Floor hatch (1) + Ceiling hatch (1) = at least 3
        assert!(children.len() >= 3);

        // Verify stamp contents
        let stamp_h = room_stamp_handle(&scene, room_h).expect("Room stamp should exist");
        if let Some(EntityType::MText(mtext)) = scene.document.get_entity(stamp_h) {
            assert!(mtext.value.contains("Office"));
            assert!(mtext.value.contains("101"));
            assert!(mtext.value.contains("Boden: Tiles"));
            assert!(mtext.value.contains("Decke: Acoustic Grid"));
            assert!(mtext.value.contains("UKD"));
        } else {
            panic!("Expected MText stamp");
        }
    }
}
