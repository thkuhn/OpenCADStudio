//! Opening entity XDATA and host-wall place/remove.

#![allow(unused_imports)]
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

use acadrust::entities::{LwPolyline, LwVertex, Point};
use acadrust::tables::AppId;
use acadrust::types::{Vector2, Vector3};
use acadrust::{CadDocument, EntityType, Handle};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use glam::DVec3;

use crate::scene::model::hatch_model::{HatchModel, HatchPattern};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

use super::{
    self as engine, Storey, StyleLibrary, Wall, WallJustification, WallLayer,
    join::{self, JoinError, JoinKind},
    junction_solver::{self, WallJoinInput},
    library::load_or_seed,
    plan_view::{PhaseFilter, PlanPhase},
    wall_style::{
        base_width_from_layers, effective_layers_for_wall_bb, migrate_gap_before_to_axis_offset,
        LayerFunction, ResolvedLayer, WallStyle,
    },
};

#[allow(unused_imports)]
use super::display_apply::*;
use super::junction_pick::*;
use super::xdata::*;
use super::wall_package::*;
use super::wall_regen::*;
use super::join_ops::*;
use super::storey_xdata::*;

// aec_wallreverse_do moved to walls (reverse.rs)


// ── Wall openings (window / door) ──────────────────────────────────────────

/// Build an `OPENING` XDATA record for a standalone opening entity.
///
/// Layout: `OPENING, host, distance, width, height, sill, kind`
/// then optional `style_id, hinge, shape, spring`, then optional `"planes"`
/// trailer (sill/head refs, offsets, baked origins/normals). Legacy 7-value
/// records remain valid (extras and trailer omitted → unbound rectangle).
pub(crate) fn opening_record(opening: &engine::openings::Opening) -> ExtendedDataRecord {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("OPENING".to_string()));
    record.add_value(XDataValue::Handle(opening.host_wall));
    record.add_value(XDataValue::Distance(opening.distance_along_axis));
    record.add_value(XDataValue::Distance(opening.width));
    record.add_value(XDataValue::Distance(opening.height));
    record.add_value(XDataValue::Distance(opening.sill_height));
    record.add_value(XDataValue::String(opening.kind.as_str().to_string()));
    record.add_value(XDataValue::String(
        opening.style_id.clone().unwrap_or_default(),
    ));
    record.add_value(XDataValue::String(opening.hinge.as_str().to_string()));
    record.add_value(XDataValue::String(opening.shape.as_str().to_string()));
    record.add_value(XDataValue::Distance(opening.spring_height));
    record.add_value(XDataValue::String(
        opening.reference_side.as_str().to_string(),
    ));
    record.add_value(XDataValue::Distance(opening.cross_axis_offset));
    record.add_value(XDataValue::Distance(opening.depth.unwrap_or(-1.0)));
    record.add_value(XDataValue::String(opening.niche_side.as_str().to_string()));
    record.add_value(XDataValue::String(opening.swing_side.as_str().to_string()));
    encode_opening_planes(&mut record.values, opening);
    record
}

fn encode_opening_planes(values: &mut Vec<XDataValue>, opening: &engine::openings::Opening) {
    values.push(XDataValue::String("planes".to_string()));
    values.push(XDataValue::String(encode_plane_ref(
        opening.sill_plane_id,
        opening.sill_plane_name.as_deref(),
    )));
    values.push(XDataValue::String(encode_plane_ref(
        opening.head_plane_id,
        opening.head_plane_name.as_deref(),
    )));
    values.push(XDataValue::Distance(opening.sill_offset));
    values.push(XDataValue::Distance(opening.head_offset));
    for c in opening.sill_origin {
        values.push(XDataValue::Distance(c));
    }
    for c in opening.sill_normal {
        values.push(XDataValue::Distance(c));
    }
    for c in opening.head_origin {
        values.push(XDataValue::Distance(c));
    }
    for c in opening.head_normal {
        values.push(XDataValue::Distance(c));
    }
}

fn parse_opening_planes_trailer(
    v: &[XDataValue],
) -> (
    Option<uuid::Uuid>,
    Option<uuid::Uuid>,
    Option<String>,
    Option<String>,
    f64,
    f64,
    [f64; 3],
    [f64; 3],
    [f64; 3],
    [f64; 3],
) {
    let mut sill_plane_id = None;
    let mut head_plane_id = None;
    let mut sill_plane_name = None;
    let mut head_plane_name = None;
    let mut sill_offset = 0.0;
    let mut head_offset = 0.0;
    let mut sill_origin = [0.0, 0.0, 0.0];
    let mut sill_normal = [0.0, 0.0, 1.0];
    let mut head_origin = [0.0, 0.0, 0.0];
    let mut head_normal = [0.0, 0.0, 1.0];
    let Some(pos) = v
        .iter()
        .rposition(|x| matches!(x, XDataValue::String(s) if s == "planes"))
    else {
        return (
            sill_plane_id,
            head_plane_id,
            sill_plane_name,
            head_plane_name,
            sill_offset,
            head_offset,
            sill_origin,
            sill_normal,
            head_origin,
            head_normal,
        );
    };
    if v.len() >= pos + 1 + 2 + 2 + 12 {
        if let XDataValue::String(s) = &v[pos + 1] {
            let (id, name) = parse_plane_ref(s);
            sill_plane_id = id;
            sill_plane_name = name;
        }
        if let XDataValue::String(s) = &v[pos + 2] {
            let (id, name) = parse_plane_ref(s);
            head_plane_id = id;
            head_plane_name = name;
        }
        let xf64 = |val: &XDataValue| -> Option<f64> {
            match val {
                XDataValue::Distance(d) | XDataValue::Real(d) | XDataValue::ScaleFactor(d) => {
                    Some(*d)
                }
                _ => None,
            }
        };
        if let Some(d) = xf64(&v[pos + 3]) {
            sill_offset = d;
        }
        if let Some(d) = xf64(&v[pos + 4]) {
            head_offset = d;
        }
        let read3 = |at: usize, dest: &mut [f64; 3]| {
            for i in 0..3 {
                if let Some(d) = xf64(&v[at + i]) {
                    dest[i] = d;
                }
            }
        };
        read3(pos + 5, &mut sill_origin);
        read3(pos + 8, &mut sill_normal);
        read3(pos + 11, &mut head_origin);
        read3(pos + 14, &mut head_normal);
    }
    (
        sill_plane_id,
        head_plane_id,
        sill_plane_name,
        head_plane_name,
        sill_offset,
        head_offset,
        sill_origin,
        sill_normal,
        head_origin,
        head_normal,
    )
}

/// Parse opening fields from an `OPENING` record's values.
pub fn opening_from_values(
    handle: Handle,
    v: &[XDataValue],
) -> Option<engine::openings::Opening> {
    if v.len() < 7 {
        return None;
    }
    let XDataValue::String(kind) = &v[0] else {
        return None;
    };
    if kind != "OPENING" {
        return None;
    }
    let host_wall = match &v[1] {
        XDataValue::Handle(h) => *h,
        _ => return None,
    };
    let distance_along_axis = match v[2] {
        XDataValue::Distance(d) => d,
        _ => return None,
    };
    let width = match v[3] {
        XDataValue::Distance(d) => d,
        _ => return None,
    };
    let height = match v[4] {
        XDataValue::Distance(d) => d,
        _ => return None,
    };
    let sill_height = match v[5] {
        XDataValue::Distance(d) => d,
        _ => return None,
    };
    let opening_kind = match &v[6] {
        XDataValue::String(s) => engine::openings::OpeningKind::from_str(s),
        _ => engine::openings::OpeningKind::Window,
    };

    let mut style_id = None;
    let mut hinge = engine::opening_style::HingeSide::Left;
    let mut shape = engine::opening_shape::OpeningShape::Rectangle;
    let mut spring_height = 0.0;
    let mut reference_side = engine::openings::OpeningReferenceSide::Center;
    let mut cross_axis_offset = 0.0;
    let mut depth = None;
    let mut niche_side = engine::openings::NicheSide::Exterior;
    let mut swing_side = engine::openings::SwingSide::Exterior;

    let planes_pos = v
        .iter()
        .position(|x| matches!(x, XDataValue::String(s) if s == "planes"))
        .unwrap_or(v.len());

    if planes_pos > 7 {
        if let Some(XDataValue::String(s)) = v.get(7) {
            if !s.is_empty() {
                style_id = Some(s.clone());
            }
        }
    }
    if planes_pos > 8 {
        if let Some(XDataValue::String(h)) = v.get(8) {
            hinge = engine::opening_style::HingeSide::from_str(h);
        }
    }
    if planes_pos > 9 {
        if let Some(XDataValue::String(sh)) = v.get(9) {
            shape = engine::opening_shape::OpeningShape::from_str(sh);
        }
    }
    if planes_pos > 10 {
        if let Some(XDataValue::Distance(d)) = v.get(10) {
            spring_height = *d;
        }
    }
    if planes_pos > 11 {
        if let Some(XDataValue::String(s)) = v.get(11) {
            reference_side = engine::openings::OpeningReferenceSide::from_str(s);
        }
    }
    if planes_pos > 12 {
        if let Some(XDataValue::Distance(d)) = v.get(12) {
            cross_axis_offset = *d;
        }
    }
    if planes_pos > 13 {
        if let Some(XDataValue::Distance(d)) = v.get(13) {
            if *d >= 0.0 {
                depth = Some(*d);
            }
        }
    }
    if planes_pos > 14 {
        if let Some(XDataValue::String(s)) = v.get(14) {
            niche_side = engine::openings::NicheSide::from_str(s);
        }
    }
    if planes_pos > 15 {
        if let Some(XDataValue::String(s)) = v.get(15) {
            swing_side = engine::openings::SwingSide::from_str(s);
        }
    }

    let (
        sill_plane_id,
        head_plane_id,
        sill_plane_name,
        head_plane_name,
        sill_offset,
        head_offset,
        sill_origin,
        sill_normal,
        head_origin,
        head_normal,
    ) = parse_opening_planes_trailer(v);

    Some(engine::openings::Opening {
        handle,
        host_wall,
        distance_along_axis,
        width,
        height,
        sill_height,
        kind: opening_kind,
        style_id,
        hinge,
        shape,
        spring_height,
        reference_side,
        cross_axis_offset,
        depth,
        niche_side,
        swing_side,
        sill_plane_id,
        head_plane_id,
        sill_plane_name,
        head_plane_name,
        sill_offset,
        head_offset,
        sill_origin,
        sill_normal,
        head_origin,
        head_normal,
    })
}

/// Parse an `OPENING` XDATA record. `handle` is the entity that carries it.
pub fn opening_from_entity(entity: &EntityType, handle: Handle) -> Option<engine::openings::Opening> {
    let record = read_aec_record(entity)?;
    opening_from_values(handle, &record.values)
}

/// All openings whose host wall is `wall_handle`, looked up via the wall's
/// `CHILD_HANDLES` owner index (no document scan).
pub fn openings_for_host_wall(scene: &Scene, wall_handle: Handle) -> Vec<engine::openings::Opening> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let mut out = Vec::new();
    for child in engine::owner_index::children_of(&scene.document, wall_handle) {
        let Some(entity) = scene.document.get_entity(child) else {
            continue;
        };
        if let Some(o) = opening_from_entity(entity, child) {
            out.push(o);
        }
    }
    out
}

/// Place a window/door opening on `wall_handle` at world point `pt`.
///
/// Creates a POINT entity carrying `OPENING` XDATA and regenerates the host
/// wall's 2D representation so the opening cut appears. Returns the new
/// opening handle plus every wall-derived handle touched.
pub fn place_wall_opening(
    scene: &mut Scene,
    wall_handle: Handle,
    pt: DVec3,
    kind: engine::openings::OpeningKind,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Result<(Handle, Vec<Handle>), String> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    if !is_wall_pick_target(scene, wall_handle) {
        return Err("select a wall entity".into());
    }
    let axis = get_wall_vertices(scene, wall_handle);
    if axis.len() < 2 {
        return Err("wall axis is degenerate".into());
    }
    let axis_2d: Vec<(f64, f64)> = axis.iter().map(|v| (v.x, v.y)).collect();
    let distance = engine::openings::distance_along_axis_from_point(&axis_2d, (pt.x, pt.y))
        .ok_or_else(|| "could not project point onto wall axis".to_string())?;

    let mut opening = engine::openings::Opening::from_kind(
        Handle::NULL,
        wall_handle,
        distance,
        kind,
    );
    // Anchor the POINT at the projected axis location (not the raw click).
    let (anchor, tangent) = engine::openings::point_and_tangent_at_distance(&axis_2d, distance)
        .unwrap_or(((pt.x, pt.y), (1.0, 0.0)));
    let normal = (-tangent.1, tangent.0);
    let cross_d = (pt.x - anchor.0) * normal.0 + (pt.y - anchor.1) * normal.1;
    if cross_d < -1e-3 {
        opening.swing_side = engine::openings::SwingSide::Interior;
    } else {
        opening.swing_side = engine::openings::SwingSide::Exterior;
    }

    if let Some(lib) = library_override {
        if let Some(style) = lib.opening_style_for_kind(kind) {
            engine::opening_style::apply_style_defaults(&mut opening, style);
        }
    }

    let point_entity = EntityType::Point(Point::at(Vector3::new(anchor.0, anchor.1, 0.0)));
    let opening_handle = scene.add_entity(point_entity);

    opening.handle = opening_handle;
    write_aec_record(&mut scene.document, opening_handle, opening_record(&opening));
    engine::owner_index::add_child(&mut scene.document, wall_handle, opening_handle);

    let mut touched = match regenerate_wall_representation_with_rules_and_substitutions(
        scene,
        wall_handle,
        display_rules,
        style_substitutions,
        library_override,
    ) {
        Ok(t) => t,
        Err(_) => {
            engine::opening_display::regenerate_opening_display(
                scene,
                opening_handle,
                library_override,
                display_rules,
            );
            vec![wall_handle]
        }
    };
    touched.push(opening_handle);
    touched.extend(engine::opening_display::collect_opening_display_children(
        scene,
        opening_handle,
    ));
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    Ok((opening_handle, touched))
}

/// Persist `OPENING` XDATA on the instance POINT.
pub fn write_opening_instance(scene: &mut Scene, opening: &engine::openings::Opening) -> bool {
    write_aec_record(
        &mut scene.document,
        opening.handle,
        opening_record(opening),
    )
}

/// Remove an opening entity, drop it from the host wall's `CHILD_HANDLES`
/// index, and regenerate the host wall. Returns touched handles (wall +
/// former opening). No-op error when `opening_handle` is not an opening.
pub fn remove_wall_opening(
    scene: &mut Scene,
    opening_handle: Handle,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, String> {
    remove_wall_opening_with_rules(scene, opening_handle, library_override, None)
}

/// Remove an opening entity with explicit display rules for host wall regeneration.
pub fn remove_wall_opening_with_rules(
    scene: &mut Scene,
    opening_handle: Handle,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
) -> Result<Vec<Handle>, String> {
    remove_wall_opening_with_rules_and_substitutions(
        scene,
        opening_handle,
        library_override,
        display_rules,
        None,
    )
}

/// Remove an opening entity with explicit display rules and style substitutions for host wall regeneration.
pub fn remove_wall_opening_with_rules_and_substitutions(
    scene: &mut Scene,
    opening_handle: Handle,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Result<Vec<Handle>, String> {
    let Some(entity) = scene.document.get_entity(opening_handle).cloned() else {
        return Err("opening entity not found".into());
    };
    let Some(opening) = opening_from_entity(&entity, opening_handle) else {
        return Err("entity is not an opening".into());
    };
    let wall_handle = resolve_wall_package(scene, opening.host_wall);
    let mut erase = engine::opening_display::collect_opening_display_children(scene, opening_handle);
    erase.push(opening_handle);
    engine::owner_index::remove_child(&mut scene.document, wall_handle, opening_handle);
    scene.erase_entities(&erase);
    let mut touched = match regenerate_wall_representation_with_rules_and_substitutions(
        scene,
        wall_handle,
        display_rules,
        style_substitutions,
        library_override,
    ) {
        Ok(t) => t,
        Err(_) => vec![wall_handle],
    };
    touched.push(opening_handle);
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    Ok(touched)
}

// WallOpeningCommand moved to walls/window.rs

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::opening_shape::{OpeningShape, TriangleVariant};
    use crate::modules::aec::engine::opening_style::HingeSide;
    use crate::modules::aec::engine::openings::{Opening, OpeningKind};

    fn sample_opening() -> Opening {
        let mut o = Opening::from_kind(
            Handle::new(10),
            Handle::new(2),
            1.5,
            OpeningKind::Door,
        );
        o.style_id = Some("style_door_standard".into());
        o.hinge = HingeSide::Right;
        o.shape = OpeningShape::Arch;
        o.spring_height = 1.65;
        o
    }

    #[test]
    fn legacy_seven_value_record_loads_as_unbound_rectangle() {
        let v = vec![
            XDataValue::String("OPENING".into()),
            XDataValue::Handle(Handle::new(2)),
            XDataValue::Distance(3.0),
            XDataValue::Distance(1.2),
            XDataValue::Distance(1.2),
            XDataValue::Distance(0.9),
            XDataValue::String("Window".into()),
        ];
        let o = opening_from_values(Handle::new(9), &v).expect("legacy");
        assert_eq!(o.kind, OpeningKind::Window);
        assert_eq!(o.style_id, None);
        assert_eq!(o.hinge, HingeSide::Left);
        assert_eq!(o.shape, OpeningShape::Rectangle);
        assert_eq!(o.spring_height, 0.0);
        assert!((o.width - 1.2).abs() < 1e-12);
        assert!(o.sill_plane_id.is_none());
        assert!(o.head_plane_id.is_none());
        assert!((o.sill_offset).abs() < 1e-12);
        assert!((o.head_offset).abs() < 1e-12);
    }

    #[test]
    fn new_record_roundtrips_style_hinge_shape_spring() {
        let o = sample_opening();
        let rec = opening_record(&o);
        let back = opening_from_values(o.handle, &rec.values).expect("parse");
        assert_eq!(back.style_id.as_deref(), Some("style_door_standard"));
        assert_eq!(back.hinge, HingeSide::Right);
        assert_eq!(back.shape, OpeningShape::Arch);
        assert!((back.spring_height - 1.65).abs() < 1e-12);
        assert_eq!(back.kind, OpeningKind::Door);
    }

    #[test]
    fn empty_style_id_is_none() {
        let mut o = Opening::window(Handle::new(1), Handle::new(2), 1.0);
        o.style_id = None;
        let rec = opening_record(&o);
        let back = opening_from_values(o.handle, &rec.values).unwrap();
        assert_eq!(back.style_id, None);
    }

    #[test]
    fn breakthrough_kind_roundtrips() {
        let o = Opening::breakthrough(Handle::new(1), Handle::new(2), 0.5);
        assert_eq!(o.kind, OpeningKind::Breakthrough);
        assert!((o.width - 1.0).abs() < 1e-12);
        assert!((o.height - 2.0).abs() < 1e-12);
        assert!((o.sill_height - 0.1).abs() < 1e-12);
        let rec = opening_record(&o);
        let back = opening_from_values(o.handle, &rec.values).unwrap();
        assert_eq!(back.kind, OpeningKind::Breakthrough);
        assert_eq!(back.shape, OpeningShape::Rectangle);
    }

    #[test]
    fn triangle_shape_string_roundtrips() {
        let mut o = Opening::window(Handle::new(1), Handle::new(2), 1.0);
        o.shape = OpeningShape::Triangle(TriangleVariant::RightLeft);
        let rec = opening_record(&o);
        let back = opening_from_values(o.handle, &rec.values).unwrap();
        assert_eq!(
            back.shape,
            OpeningShape::Triangle(TriangleVariant::RightLeft)
        );
    }

    #[test]
    fn planes_trailer_roundtrips() {
        let mut o = sample_opening();
        let sill = Uuid::new_v4();
        let head = Uuid::new_v4();
        o.sill_plane_id = Some(sill);
        o.head_plane_id = Some(head);
        o.sill_plane_name = Some("EG_ELEVATION".into());
        o.head_plane_name = Some("EG_OKGH".into());
        o.sill_offset = 0.1;
        o.head_offset = -0.05;
        o.sill_origin = [1.0, 2.0, 0.3];
        o.sill_normal = [0.0, 0.0, 1.0];
        o.head_origin = [1.0, 2.0, 2.4];
        o.head_normal = [0.0, 0.0, 1.0];
        let rec = opening_record(&o);
        let back = opening_from_values(o.handle, &rec.values).expect("parse");
        assert_eq!(back.sill_plane_id, Some(sill));
        assert_eq!(back.head_plane_id, Some(head));
        assert_eq!(back.sill_plane_name.as_deref(), Some("EG_ELEVATION"));
        assert_eq!(back.head_plane_name.as_deref(), Some("EG_OKGH"));
        assert!((back.sill_offset - 0.1).abs() < 1e-12);
        assert!((back.head_offset + 0.05).abs() < 1e-12);
        assert!((back.sill_origin[2] - 0.3).abs() < 1e-12);
        assert!((back.head_origin[2] - 2.4).abs() < 1e-12);
        assert_eq!(back.style_id.as_deref(), Some("style_door_standard"));
    }

    #[test]
    fn extras_without_planes_trailer_stay_unbound() {
        let v = vec![
            XDataValue::String("OPENING".into()),
            XDataValue::Handle(Handle::new(2)),
            XDataValue::Distance(3.0),
            XDataValue::Distance(1.2),
            XDataValue::Distance(1.2),
            XDataValue::Distance(0.9),
            XDataValue::String("Window".into()),
            XDataValue::String("style_window_standard".into()),
            XDataValue::String("Left".into()),
            XDataValue::String("Rectangle".into()),
            XDataValue::Distance(0.0),
        ];
        let o = opening_from_values(Handle::new(9), &v).expect("extras");
        assert_eq!(o.style_id.as_deref(), Some("style_window_standard"));
        assert_eq!(o.swing_side, engine::openings::SwingSide::Exterior);
        assert!(o.sill_plane_id.is_none());
        assert!(o.head_plane_id.is_none());
    }

    #[test]
    fn swing_side_roundtrips_and_flips() {
        let mut o = sample_opening();
        o.swing_side = engine::openings::SwingSide::Interior;
        let rec = opening_record(&o);
        let back = opening_from_values(o.handle, &rec.values).expect("parse swing");
        assert_eq!(back.swing_side, engine::openings::SwingSide::Interior);

        let mut o_flip = back;
        o_flip.flip_swing();
        assert_eq!(o_flip.swing_side, engine::openings::SwingSide::Exterior);
    }
}
