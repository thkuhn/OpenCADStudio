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

    // Determine stamp position (use saved coordinate if still inside polygon, or recalculate centroid)
    let stamp_pos = match room.stamp_pos {
        Some(pos) if super::geometry::point_in_polygon((pos.0, pos.1), &points) => pos,
        _ => centroid(&points),
    };
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
    text_lines.push(format!("{{\\b {}}}", title));

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
    text_lines.push(format!(
        "U: {:.2} m | RH: {:.2} m",
        room.perimeter, room.clear_height
    ));

    // Floor finish if present
    let finish_str = room.floor_finish_summary();
    if finish_str != "-" {
        text_lines.push(format!("Boden: {}", finish_str));
    }

    let mtext_value = text_lines.join("\\P");

    let mut mtext = MText::new();
    mtext.value = mtext_value;
    mtext.insertion_point = Vector3::new(stamp_pos.0, stamp_pos.1, room.base_z);
    mtext.height = 0.22;
    mtext.attachment_point = acadrust::entities::mtext::AttachmentPoint::MiddleCenter;

    let mut stamp_entity = EntityType::MText(mtext);
    stamp_entity.common_mut().layer = AEC_ROOM_STAMP_LAYER.to_string();

    let stamp_handle = scene.add_entity(stamp_entity);
    write_room_display_tag(scene, stamp_handle, room_handle, ROOM_REP_ROLE_STAMP);
    child_handles.push(stamp_handle);

    // 2. Build optional floor finish pattern hatch
    if let Some(finishes) = &room.floor_finish {
        if let Some(hatch_finish) = finishes.iter().find(|f| f.hatch_pattern.is_some()) {
            if let Some(pat_name) = &hatch_finish.hatch_pattern {
                let mut wcs: Vec<[f64; 2]> = points.iter().map(|p| [p.0, p.1]).collect();
                if let Some(first) = wcs.first().copied() {
                    wcs.push(first);
                }
                let origin = [stamp_pos.0, stamp_pos.1];
                let rel: Vec<[f32; 2]> = points
                    .iter()
                    .map(|p| [
                        (p.0 - stamp_pos.0) as f32,
                        (p.1 - stamp_pos.1) as f32,
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
                let hatch_model = HatchModel {
                    pattern_origin: None,
                    render_instance: None,
                    boundary: std::sync::Arc::new(rel.clone()),
                    pattern: HatchPattern::Pattern(families),
                    name: pat_name.clone(),
                    color: [0.6, 0.6, 0.6, 0.7],
                    aci: 8,
                    line_weight_px: 1.0,
                    angle_offset: 0.0,
                    scale: 1.0,
                    world_origin: origin,
                    boundary_wcs: Some(std::sync::Arc::new(wcs)),
                    fill_plane: Some(FillPlane {
                        origin: [origin[0], origin[1], room.base_z],
                        x_axis: [1.0, 0.0, 0.0],
                        y_axis: [0.0, 1.0, 0.0],
                    }),
                    fill_plane_boundary: Some(std::sync::Arc::new(rel)),
                    boundary_exterior: None,
                    boundary_sources: None,
                    boundary_paths: None,
                    style: acadrust::entities::HatchStyleType::Normal,
                    draw_depth: 0.0,
                };

                let hatch_handle = scene.add_hatch(hatch_model, None, None);
                scene.ensure_layer(AEC_ROOM_HATCH_LAYER);
                if let Some(e) = scene.document.get_entity_mut(hatch_handle) {
                    e.as_entity_mut().set_layer(AEC_ROOM_HATCH_LAYER.to_string());
                    e.common_mut().owner_handle = room_handle;
                }
                write_room_display_tag(scene, hatch_handle, room_handle, ROOM_REP_ROLE_HATCH);
                child_handles.push(hatch_handle);
            }
        }
    }

    // Register all child handles in owner index
    owner_index::set_children(&mut scene.document, room_handle, &child_handles);

    Some(())
}
