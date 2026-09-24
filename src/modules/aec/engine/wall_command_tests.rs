//! Wall scene-ops tests moved from `aec/commands.rs`.

use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

use super::display_apply::*;
use super::join_ops::*;
use super::junction_pick::*;
use super::opening_xdata::*;
use super::storey_xdata::*;
use super::wall_package::*;
use super::wall_regen::*;
use super::xdata::*;
use super::{
    self as engine, Storey, StyleLibrary, Wall, WallJustification, WallLayer,
    join::{self, JoinError, JoinKind},
    wall_style::{
        effective_layers_for_wall_bb, migrate_gap_before_to_axis_offset, LayerFunction,
        WallStyle,
    },
};

use acadrust::entities::{EntityType, LwPolyline, LwVertex};
use acadrust::types::{Vector2, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::Handle;
use glam::DVec3;
use crate::modules::aec::engine::openings::OpeningKind;
use crate::modules::aec::engine::opening_style::OpeningStyle;
use crate::modules::aec::engine::plan_view::{PhaseFilter, PlanPhase};
use crate::modules::aec::engine::project::{Building, ProjectFile, StoreyRef};

use crate::command::{CadCommand, CmdResult};
use crate::modules::aec::engine::material::Material;
use crate::modules::aec::engine::style::Style;
use crate::modules::aec::engine::wall_style::{Layer, LayerValue};
use crate::modules::aec::rooms::room::aec_room;
use crate::modules::aec::styles::material_manager::MaterialCommand;
use crate::modules::aec::styles::wall_style_manager::StyleCommand;
use crate::modules::aec::walls::extend::{aec_wallextend_do, WallExtendCommand};
use crate::modules::aec::walls::join::WallJoinCommand;
use crate::modules::aec::walls::wall::WallCommand;
use crate::modules::aec::walls::wall::{DEFAULT_WALL_HEIGHT, DEFAULT_WALL_THICKNESS};
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

fn wall_xdata(entity: &EntityType) -> Option<&ExtendedDataRecord> {
    read_aec_record(entity)
}

fn wl(material: &str, thickness: f64, function: &str) -> WallLayer {
    WallLayer {
        material: material.to_string(),
        thickness,
        function: function.to_string(),
        axis_offset: -thickness * 0.5,
        bottom_offset: 0.0,
        top_offset: 0.0,
        layer_override: None,
        hatch_override: None,
    layer_id: uuid::Uuid::new_v4(),
    }
}

/// Build a multi-layer stack with centered absolute axis offsets.
fn wls(specs: &[(&str, f64, &str)]) -> Vec<WallLayer> {
    let pairs: Vec<(f64, f64)> = specs.iter().map(|(_, t, _)| (*t, 0.0)).collect();
    let offsets = migrate_gap_before_to_axis_offset(&pairs);
    specs
        .iter()
        .zip(offsets)
        .map(|(&(mat, t, fun), off)| WallLayer {
            material: mat.to_string(),
            thickness: t,
            function: fun.to_string(),
            axis_offset: off,
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: match mat {
                "Putz" => Some("DOTS".to_string()),
                "Mauerwerk" => Some("ANSI31".to_string()),
                _ => None,
            },
        layer_id: uuid::Uuid::new_v4(),
        })
        .collect()
}

#[test]
fn junction_override_write_read_roundtrip() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);

    let ov = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::Miter),
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: Some("Tragschale".to_string()),
                index: 0,
            layer_id: None,
            },
            layer_b: None,
            style: join::JoinOverrideStyle::OuterFace,
        }],
        layer_gaps: Vec::new(),
    };

    assert!(write_junction_override(&mut scene, wall, 1, &ov));
    let read_back = read_junction_override(&scene, wall, 1);
    assert_eq!(read_back, Some(ov));

    // The other end of the same axis was not touched.
    assert_eq!(read_junction_override(&scene, wall, 0), None);
}

#[test]
fn junction_override_missing_returns_none() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    // No override ever written on this axis.
    assert_eq!(read_junction_override(&scene, wall, 0), None);
    assert_eq!(read_junction_override(&scene, wall, 1), None);
}

#[test]
fn junction_override_absent_on_old_format_entity_is_backward_compatible() {
    // Simulate a drawing saved before this feature existed: the wall
    // axis has its normal `WALL` XDATA but never the new `JOIN_OVERRIDE`
    // tag. Reading must not panic and must simply report `None`.
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    let entity = scene.document.get_entity(wall).unwrap();
    assert!(wall_from_entity(entity).is_some(), "old-format wall still loads");
    assert_eq!(read_junction_override(&scene, wall, 0), None);
    assert_eq!(read_junction_override(&scene, wall, 1), None);
}

/// Mirrors the `Message::Aec(AecMessage::WallJunctionOverrideSetStyle)` handler in
/// `app/update/mod.rs`: read the existing override (if any), set
/// `default_style`, write it back, then trigger the same immediate
/// regeneration the context-menu action performs.
#[test]
fn wall_junction_context_menu_set_style_persists_and_regenerates() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    let end_index = 1usize;

    let mut override_data =
        read_junction_override(&scene, wall, end_index).unwrap_or_default();
    override_data.default_style = Some(join::JoinOverrideStyle::Butt);
    assert!(write_junction_override(&mut scene, wall, end_index, &override_data));
    let touched = refresh_wall_after_axis_edit(&mut scene, wall, None, None, None);
    assert!(!touched.is_empty(), "regeneration should touch at least the axis");

    let read_back = read_junction_override(&scene, wall, end_index);
    assert_eq!(
        read_back.and_then(|ov| ov.default_style),
        Some(join::JoinOverrideStyle::Butt)
    );
}

/// Mirrors the `Message::Aec(AecMessage::WallJunctionOverrideReset)` handler: remove the
/// override for the junction and regenerate. `read_junction_override`
/// must report `None` afterward.
#[test]
fn wall_junction_context_menu_reset_removes_override_and_regenerates() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    let end_index = 0usize;

    let ov = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::Miter),
        layer_pairs: vec![],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, wall, end_index, &ov));
    assert!(read_junction_override(&scene, wall, end_index).is_some());

    assert!(remove_junction_override(&mut scene, wall, end_index));
    let touched = refresh_wall_after_axis_edit(&mut scene, wall, None, None, None);
    assert!(!touched.is_empty());

    assert_eq!(read_junction_override(&scene, wall, end_index), None);
}

fn add_wall_2layer(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64), mat: &str) -> Handle {
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
    pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
    let mut entity = EntityType::LwPolyline(pl);
    let layers = wls(&[(mat, 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    scene.add_entity(entity)
}

/// Same as [`add_wall_2layer`], but with a *bent*, multi-segment axis
/// (three vertices) so its "last vertex" raw index is `2`, not `1` — the
/// exact shape needed to reproduce the reported bug where the Junction
/// Editor's endpoint-index comparison broke for non-2-point wall axes.
fn add_bent_wall_2layer(
    scene: &mut Scene,
    p1: (f64, f64),
    p2: (f64, f64),
    p3: (f64, f64),
    mat: &str,
) -> Handle {
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
    pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
    pl.add_vertex(LwVertex::new(Vector2::new(p3.0, p3.1)));
    let mut entity = EntityType::LwPolyline(pl);
    let layers = wls(&[(mat, 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    scene.add_entity(entity)
}

/// Regression test for the reported bug: the Junction Editor's master
/// list ("Beteiligte W\u{e4}nde") showed only the single clicked wall even
/// though the Properties panel correctly showed the connected walls. Root
/// cause: `walls_at_junction` compared the raw `JunctionRole::Endpoint`
/// vertex index (which is `axis.len() - 1` for the "far" end, e.g. `2`
/// for a 3-vertex bent wall) directly against the normalized `0`/`1`
/// `end_index` convention used everywhere else, so the match always
/// failed for walls with more than two axis points, and the function
/// fell back to returning just the queried wall.
#[test]
fn walls_at_junction_finds_all_participants_for_bent_multi_segment_wall() {
    let mut scene = Scene::new();
    // w1's axis has 3 vertices; its junction end is the *last* vertex,
    // whose raw index is 2 (not 1).
    let w1 = add_bent_wall_2layer(&mut scene, (-10.0, 5.0), (-10.0, 0.0), (0.0, 0.0), "Brick");
    let w2 = add_wall_2layer(&mut scene, (0.0, 0.0), (0.0, 10.0), "Concrete");
    let w3 = add_wall_2layer(&mut scene, (0.0, 0.0), (10.0, 0.0), "Wood");
    for h in [w1, w2, w3] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }
    join_junction_in_document(&mut scene,  &[w1, w2, w3],  None,  None,  None,  None).expect("N-way join");

    let w1_vertices = get_wall_vertices(&scene, w1);
    let end_1 = if w1_vertices[0].distance(DVec3::ZERO) < 1e-6 { 0 } else { 1 };
    let participants = walls_at_junction(&scene, w1, end_1);

    let mut handles: Vec<Handle> = participants.iter().map(|p| p.axis_handle).collect();
    handles.sort_by_key(|h| h.value());
    let mut expected = vec![w1, w2, w3];
    expected.sort_by_key(|h| h.value());
    assert_eq!(
        handles, expected,
        "expected all 3 walls at the junction, got {participants:?}"
    );
}

/// Regression test for the reported bug: a T-junction between two walls
/// (one wall's endpoint touches the other wall's *interior*, i.e. a
/// [`join::JunctionRole::Through`] participant) must still list the
/// through-running wall in the Junction Editor's participating-walls
/// list, not just the stem wall that was clicked to open the editor.
#[test]
fn walls_at_junction_includes_through_wall_at_t_junction() {
    let mut scene = Scene::new();
    let through = add_wall_2layer(&mut scene, (0.0, 0.0), (10.0, 0.0), "Brick");
    let stem = add_wall_2layer(&mut scene, (5.0, 0.0), (5.0, 5.0), "Concrete");
    for h in [through, stem] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }
    join_junction_in_document(&mut scene, &[through, stem], None, None, None, None).expect("T join");

    // The stem wall's end at (5,0) is a real Endpoint; use it as the
    // query, exactly as the context menu does when the user clicks the
    // stem wall to open the Junction Editor.
    let stem_vertices = get_wall_vertices(&scene, stem);
    let stem_end = if stem_vertices[0].distance(DVec3::new(5.0, 0.0, 0.0)) < 1e-6 { 0 } else { 1 };
    let participants = walls_at_junction(&scene, stem, stem_end);

    let mut handles: Vec<Handle> = participants.iter().map(|p| p.axis_handle).collect();
    handles.sort_by_key(|h| h.value());
    let mut expected = vec![through, stem];
    expected.sort_by_key(|h| h.value());
    assert_eq!(
        handles, expected,
        "the through wall must be listed alongside the stem wall, got {participants:?}"
    );

    let through_participant = participants
        .iter()
        .find(|p| p.axis_handle == through)
        .expect("through wall must be present");
    assert!(
        through_participant.is_through,
        "through wall participant must be flagged as is_through"
    );
    let stem_participant = participants
        .iter()
        .find(|p| p.axis_handle == stem)
        .expect("stem wall must be present");
    assert!(!stem_participant.is_through, "stem wall must not be flagged as through");
}

#[test]
fn disconnecting_one_end_drops_only_that_junction_override() {
    let mut scene = Scene::new();
    let mid = add_wall_2layer(&mut scene, (0.0, 0.0), (10.0, 0.0), "Brick");
    let left = add_wall_2layer(&mut scene, (0.0, 0.0), (0.0, 5.0), "Concrete");
    let right = add_wall_2layer(&mut scene, (10.0, 0.0), (10.0, 5.0), "Wood");
    for h in [mid, left, right] {
        regenerate_wall_representation(&mut scene, h, None).expect("regen");
    }
    join_two_walls_in_document(&mut scene, mid, left, None, None, None).expect("join left");
    join_two_walls_in_document(&mut scene, mid, right, None, None, None).expect("join right");
    let ov = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::Butt),
        layer_pairs: vec![],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, mid, 0, &ov));
    assert!(write_junction_override(&mut scene, mid, 1, &ov));
    update_wall_vertices(
        &mut scene,
        left,
        &[
            glam::DVec3::new(0.0, 20.0, 0.0),
            glam::DVec3::new(0.0, 25.0, 0.0),
        ],
    );
    let _ = try_auto_join_nearby_walls(&mut scene, left, None, None, None);
    assert_eq!(
        read_junction_override(&scene, mid, 0),
        None,
        "disconnected end must drop its override"
    );
    assert!(
        read_junction_override(&scene, mid, 1).is_some(),
        "the still-joined end must keep its override"
    );
}

/// Step 5 test 1: discovering "all walls at a junction" for a known
/// multi-wall N-way fixture returns every participating wall handle plus
/// its layer material ids, reusing [`join::detect_junctions`] topology.
#[test]
fn walls_at_junction_finds_all_n_way_participants() {
    let mut scene = Scene::new();
    let w1 = add_wall_2layer(&mut scene, (0.0, 0.0), (-10.0, 0.0), "Brick");
    let w2 = add_wall_2layer(&mut scene, (0.0, 0.0), (0.0, 10.0), "Concrete");
    let w3 = add_wall_2layer(&mut scene, (0.0, 0.0), (10.0, 0.0), "Wood");
    for h in [w1, w2, w3] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }
    join_junction_in_document(&mut scene,  &[w1, w2, w3],  None,  None,  None,  None).expect("N-way join");

    let end_1 = if get_wall_vertices(&scene, w1)[0].distance(DVec3::ZERO) < 1e-6 { 0 } else { 1 };
    let participants = walls_at_junction(&scene, w1, end_1);

    let mut handles: Vec<Handle> = participants.iter().map(|p| p.axis_handle).collect();
    handles.sort_by_key(|h| h.value());
    let mut expected = vec![w1, w2, w3];
    expected.sort_by_key(|h| h.value());
    assert_eq!(handles, expected);

    let mut mats: Vec<String> = participants
        .iter()
        .flat_map(|p| p.layers.iter().map(|l| l.material_id.clone()))
        .collect();
    mats.sort();
    let mut expected_mats = vec![
        "Brick".to_string(),
        "Insulation".to_string(),
        "Concrete".to_string(),
        "Insulation".to_string(),
        "Wood".to_string(),
        "Insulation".to_string(),
    ];
    expected_mats.sort();
    assert_eq!(mats, expected_mats);
}

#[test]
fn selected_junction_layer_ref_keeps_inner_same_material_layer() {
    let outer_id = uuid::Uuid::new_v4();
    let inner_id = uuid::Uuid::new_v4();
    let layers = vec![
        join::LayerRef {
            material_id: "Plaster".to_string(),
            role_tag: None,
            index: 0,
            layer_id: Some(outer_id),
        },
        join::LayerRef {
            material_id: "Brick".to_string(),
            role_tag: None,
            index: 1,
            layer_id: Some(uuid::Uuid::new_v4()),
        },
        join::LayerRef {
            material_id: "Plaster".to_string(),
            role_tag: None,
            index: 2,
            layer_id: Some(inner_id),
        },
    ];
    let chosen = selected_junction_layer_ref(&layers, 2, "Plaster").expect("inner plaster");
    assert_eq!(chosen.layer_id, Some(inner_id));
    assert_eq!(chosen.index, 2);
    assert_ne!(chosen.layer_id, Some(outer_id));
    let outer = selected_junction_layer_ref(&layers, 0, "Plaster").expect("outer plaster");
    assert_eq!(outer.layer_id, Some(outer_id));
}

#[test]
fn wall_junction_dropdown_grip_ids_roundtrip() {
    assert_eq!(wall_junction_end_from_dropdown_grip(wall_junction_dropdown_grip_id(0)), Some(0));
    assert_eq!(wall_junction_end_from_dropdown_grip(wall_junction_dropdown_grip_id(1)), Some(1));
    assert_eq!(wall_junction_end_from_dropdown_grip(0), None);
    assert_ne!(wall_junction_dropdown_grip_id(0), usize::MAX);
}

#[test]
fn wall_layer_contour_loop_xy_uses_chosen_stack_index() {
    let axis = [(0.0, 0.0), (10.0, 0.0)];
    let layers = [(0.1, -0.15), (0.2, -0.05)];
    let outer = wall_layer_contour_loop_xy(&axis, &layers, 0).expect("outer");
    let inner = wall_layer_contour_loop_xy(&axis, &layers, 1).expect("inner");
    assert!(outer.len() >= 5);
    assert!(inner.len() >= 5);
    let outer_span: f64 = outer.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max)
        - outer.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let inner_span: f64 = inner.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max)
        - inner.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    assert!((outer_span - 0.1).abs() < 1e-9);
    assert!((inner_span - 0.2).abs() < 1e-9);
    assert!(wall_layer_contour_loop_xy(&axis, &layers, 9).is_none());
}

#[test]
fn point_in_closed_loop_xy_picks_inner_layer_over_outer() {
    let axis = [(0.0, 0.0), (10.0, 0.0)];
    let layers = [(0.2, -0.1), (0.2, 0.1)];
    let outer = wall_layer_contour_loop_xy(&axis, &layers, 0).expect("outer");
    let inner = wall_layer_contour_loop_xy(&axis, &layers, 1).expect("inner");
    assert!(point_in_closed_loop_xy(5.0, 0.1, &inner));
    assert!(!point_in_closed_loop_xy(5.0, 0.1, &outer));
    assert!(point_in_closed_loop_xy(5.0, -0.1, &outer));
    assert!(!point_in_closed_loop_xy(5.0, 2.0, &outer));
}

/// Step 5 test 2: adding a `LayerPairOverride` via the panel's save logic
/// (read-mutate-write the same `JunctionOverride`) persists correctly and
/// coexists with an existing `default_style`.
#[test]
fn junction_editor_adds_layer_pair_and_keeps_default_style() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    let end_index = 1usize;

    let mut override_data =
        read_junction_override(&scene, wall, end_index).unwrap_or_default();
    override_data.default_style = Some(join::JoinOverrideStyle::Miter);
    assert!(write_junction_override(&mut scene, wall, end_index, &override_data));

    // Panel save logic: read-modify-write the same structure to add a
    // layer-pair override.
    let mut override_data =
        read_junction_override(&scene, wall, end_index).unwrap_or_default();
    override_data.layer_pairs.push(join::LayerPairOverride {
        layer_a: join::LayerRef {
            material_id: "Concrete".to_string(),
            role_tag: None,
            index: 0,
        layer_id: None,
        },
        layer_b: None,
        style: join::JoinOverrideStyle::Butt,
    });
    assert!(write_junction_override(&mut scene, wall, end_index, &override_data));

    let read_back = read_junction_override(&scene, wall, end_index).unwrap();
    assert_eq!(read_back.default_style, Some(join::JoinOverrideStyle::Miter));
    assert_eq!(read_back.layer_pairs.len(), 1);
    assert_eq!(read_back.layer_pairs[0].style, join::JoinOverrideStyle::Butt);
}

/// Step 5 test 3: removing a single layer-pair entry (panel's per-pair
/// "Zuruecksetzen") leaves other pairs and `default_style` intact.
#[test]
fn junction_editor_removes_single_layer_pair_only() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    let end_index = 1usize;

    let override_data = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::OuterFace),
        layer_pairs: vec![
            join::LayerPairOverride {
                layer_a: join::LayerRef {
                    material_id: "Brick".to_string(),
                    role_tag: None,
                    index: 0,
                layer_id: None,
                },
                layer_b: None,
                style: join::JoinOverrideStyle::Miter,
            },
            join::LayerPairOverride {
                layer_a: join::LayerRef {
                    material_id: "Insulation".to_string(),
                    role_tag: None,
                    index: 1,
                layer_id: None,
                },
                layer_b: None,
                style: join::JoinOverrideStyle::Butt,
            },
        ],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, wall, end_index, &override_data));

    // Panel per-pair reset: remove only the targeted entry.
    let mut current = read_junction_override(&scene, wall, end_index).unwrap();
    current.layer_pairs.retain(|p| p.layer_a.material_id != "Brick");
    assert!(write_junction_override(&mut scene, wall, end_index, &current));

    let read_back = read_junction_override(&scene, wall, end_index).unwrap();
    assert_eq!(read_back.default_style, Some(join::JoinOverrideStyle::OuterFace));
    assert_eq!(read_back.layer_pairs.len(), 1);
    assert_eq!(read_back.layer_pairs[0].layer_a.material_id, "Insulation");
}

/// Step 5 test 4: a full reset via the panel removes the entire override,
/// matching Step 4's context-menu reset exactly (same helper, same result).
#[test]
fn junction_editor_full_reset_matches_context_menu_reset() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    let end_index = 1usize;

    let override_data = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::Miter),
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Brick".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            },
            layer_b: None,
            style: join::JoinOverrideStyle::Butt,
        }],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, wall, end_index, &override_data));
    assert!(read_junction_override(&scene, wall, end_index).is_some());

    assert!(remove_junction_override(&mut scene, wall, end_index));
    assert_eq!(read_junction_override(&scene, wall, end_index), None);
}

/// Step 5 test 5 (consistency): setting `default_style` via the Step 4
/// context-menu code path, then editing `layer_pairs` via the panel's
/// code path on the SAME `(axis_handle, end_index)`, must combine both
/// additively in the final `JunctionOverride` (no clobbering).
#[test]
fn context_menu_and_junction_editor_paths_combine_additively() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    let end_index = 1usize;

    // Step 4 context-menu path.
    let mut override_data =
        read_junction_override(&scene, wall, end_index).unwrap_or_default();
    override_data.default_style = Some(join::JoinOverrideStyle::Butt);
    assert!(write_junction_override(&mut scene, wall, end_index, &override_data));

    // Step 5 panel path, on the exact same (axis_handle, end_index).
    let mut override_data =
        read_junction_override(&scene, wall, end_index).unwrap_or_default();
    override_data.layer_pairs.push(join::LayerPairOverride {
        layer_a: join::LayerRef {
            material_id: "Insulation".to_string(),
            role_tag: None,
            index: 1,
        layer_id: None,
        },
        layer_b: Some(join::LayerRef {
            material_id: "Concrete".to_string(),
            role_tag: None,
            index: 0,
        layer_id: None,
        }),
        style: join::JoinOverrideStyle::OuterFace,
    });
    assert!(write_junction_override(&mut scene, wall, end_index, &override_data));

    let read_back = read_junction_override(&scene, wall, end_index).unwrap();
    assert_eq!(read_back.default_style, Some(join::JoinOverrideStyle::Butt));
    assert_eq!(read_back.layer_pairs.len(), 1);
    assert_eq!(read_back.layer_pairs[0].style, join::JoinOverrideStyle::OuterFace);
}

#[test]
fn first_point_only_waits_for_the_next_one() {
    let mut cmd = WallCommand::new_with_library(None);
    assert!(matches!(
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0)),
        CmdResult::NeedPoint
    ));
}

#[test]
fn second_point_commits_a_two_vertex_wall_polyline_with_xdata() {
    let mut cmd = WallCommand::new_with_library(None);
    assert!(matches!(
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0)),
        CmdResult::NeedPoint
    ));

    match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => {
            match &entity {
                EntityType::LwPolyline(pl) => assert_eq!(pl.vertices.len(), 2),
                _ => panic!("expected a wall polyline"),
            }
            assert!(
                wall_xdata(&entity).is_some(),
                "committed wall segment should carry OPENCAD_AEC/WALL xdata"
            );
            assert_eq!(
                crate::entities::names::ui_name(&entity),
                "Wall",
                "properties/status should label the host as Wall, not Polyline"
            );
        }
        _ => panic!("second point should commit a wall polyline"),
    }
    assert_eq!(cmd.vertices.len(), 1);
    assert_eq!(cmd.segments_committed, 1);
}

#[test]
fn third_point_commits_a_second_two_vertex_wall() {
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let first = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("expected CommitEntity"),
    };
    match first {
        EntityType::LwPolyline(pl) => assert_eq!(pl.vertices.len(), 2),
        _ => panic!("expected LwPolyline"),
    }
    match cmd.on_point(DVec3::new(5.0, 3.0, 0.0)) {
        CmdResult::CommitEntity(entity) => match entity {
            EntityType::LwPolyline(pl) => assert_eq!(pl.vertices.len(), 2),
            _ => panic!("expected a wall polyline"),
        },
        _ => panic!("a third point should commit the next 2-vertex wall"),
    }
    assert_eq!(cmd.segments_committed, 2);
    assert_eq!(cmd.vertices.len(), 1);
}

#[test]
fn zero_length_second_point_is_rejected() {
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(1.0, 1.0, 0.0));
    assert!(matches!(
        cmd.on_point(DVec3::new(1.0, 1.0, 0.0)),
        CmdResult::NeedPoint
    ));
    assert_eq!(cmd.vertices.len(), 1);
    assert_eq!(cmd.segments_committed, 0);
}

#[test]
fn wall_chain_auto_joins_l_corner() {
    let mut scene = Scene::new();
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let e1 = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("expected first segment"),
    };
    let h1 = scene.add_entity(e1);
    cmd.on_entities_committed(&mut scene, &[h1]);
    let e2 = match cmd.on_point(DVec3::new(5.0, 4.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("expected second segment"),
    };
    let h2 = scene.add_entity(e2);
    cmd.on_entities_committed(&mut scene, &[h2]);

    let w1 = wall_from_entity(scene.document.get_entity(h1).unwrap()).unwrap();
    let w2 = wall_from_entity(scene.document.get_entity(h2).unwrap()).unwrap();
    assert!(!w1.derived_handles.is_empty());
    assert!(!w2.derived_handles.is_empty());
    let a1 = get_wall_vertices(&scene, h1);
    let a2 = get_wall_vertices(&scene, h2);
    let corner = DVec3::new(5.0, 0.0, 0.0);
    assert!(a1.iter().any(|p| p.distance(corner) < 1e-4));
    assert!(a2.iter().any(|p| p.distance(corner) < 1e-4));
}

#[test]
fn two_point_wall_regen_creates_contour_hatch_solid() {
    let mut scene = Scene::new();
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let entity = match cmd.on_point(DVec3::new(4.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("expected segment"),
    };
    let handle = scene.add_entity(entity);
    cmd.on_entities_committed(&mut scene, &[handle]);
    let wall = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    let mut saw_contour = false;
    let mut saw_hatch = false;
    let mut saw_solid = false;
    for h in wall.derived_handles {
        let entity = scene.document.get_entity(h).unwrap();
        let record = read_aec_record(entity).expect("display child");
        if let [XDataValue::String(kind), _, XDataValue::String(role)] = record.values.as_slice()
        {
            assert_eq!(kind, "WALL_REP");
            match role.as_str() {
                WALL_REP_ROLE_CONTOUR => saw_contour = true,
                WALL_REP_ROLE_HATCH => saw_hatch = true,
                WALL_REP_ROLE_SOLID => saw_solid = true,
                _ => {}
            }
        }
    }
    assert!(saw_contour && saw_hatch && saw_solid);
}

#[test]
fn regen_places_2d_components_at_wall_base_z() {
    let mut scene = Scene::new();
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let entity = match cmd.on_point(DVec3::new(4.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("expected segment"),
    };
    let handle = scene.add_entity(entity);
    cmd.on_entities_committed(&mut scene, &[handle]);
    let mut wall = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    wall.base_origin[2] = 3.0;
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    write_aec_record(&mut scene.document, handle, record);
    regenerate_wall_representation(&mut scene, handle, None).expect("regen");
    match scene.document.get_entity(handle).unwrap() {
        EntityType::LwPolyline(pl) => {
            assert!((pl.elevation - 3.0).abs() < 1e-9);
        }
        _ => panic!("expected axis polyline"),
    }
    let wall = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    let mut saw_contour_z = false;
    let mut saw_hatch_z = false;
    for h in wall.derived_handles {
        match scene.document.get_entity(h).unwrap() {
            EntityType::LwPolyline(pl) => {
                assert!((pl.elevation - 3.0).abs() < 1e-9);
                saw_contour_z = true;
            }
            EntityType::Hatch(hatch) => {
                assert!((hatch.elevation - 3.0).abs() < 1e-9);
                saw_hatch_z = true;
            }
            _ => {}
        }
    }
    assert!(saw_contour_z && saw_hatch_z);
}

#[test]
fn wall_commit_erases_live_preview_axis_and_contour() {
    let mut scene = Scene::new();
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let mut live_axis = LwPolyline::new();
    live_axis.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    live_axis.add_vertex(LwVertex::new(Vector2::new(3.0, 0.0)));
    let mut live_contour = LwPolyline::new();
    live_contour.is_closed = true;
    live_contour.add_vertex(LwVertex::new(Vector2::new(0.0, -0.1)));
    live_contour.add_vertex(LwVertex::new(Vector2::new(3.0, -0.1)));
    live_contour.add_vertex(LwVertex::new(Vector2::new(3.0, 0.1)));
    live_contour.add_vertex(LwVertex::new(Vector2::new(0.0, 0.1)));
    let live_handles = vec![
        scene.add_entity(EntityType::LwPolyline(live_axis)),
        scene.add_entity(EntityType::LwPolyline(live_contour)),
    ];
    cmd.set_live_handles(live_handles.clone());
    let committed = match cmd.on_point(DVec3::new(4.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("expected commit"),
    };
    let handle = scene.add_entity(committed);
    cmd.on_entities_committed(&mut scene, &[handle]);
    for h in live_handles {
        if h != handle {
            assert!(
                scene.document.get_entity(h).is_none(),
                "live preview {h:?} should be erased"
            );
        }
    }
    let wall = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    assert!(!wall.derived_handles.is_empty());
}

#[test]
fn arc_keyword_toggles_arc_mode_and_produces_a_bulge_segment() {
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let first = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("expected first segment"),
    };
    match first {
        EntityType::LwPolyline(pl) => assert_eq!(pl.vertices[0].bulge, 0.0),
        _ => panic!("expected LwPolyline"),
    }

    assert!(matches!(cmd.on_text_input("A"), Some(CmdResult::NeedPoint)));
    assert!(cmd.arc_mode);
    let arc_entity = match cmd.on_point(DVec3::new(10.0, 5.0, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("expected arc segment"),
    };
    match arc_entity {
        EntityType::LwPolyline(pl) => {
            assert_eq!(pl.vertices.len(), 2);
            assert_ne!(pl.vertices[0].bulge, 0.0, "arc-mode segment should get a bulge");
        }
        _ => panic!("expected LwPolyline"),
    }

    assert!(matches!(cmd.on_text_input("L"), Some(CmdResult::NeedPoint)));
    assert!(!cmd.arc_mode);
    match cmd.on_point(DVec3::new(15.0, 5.0, 0.0)) {
        CmdResult::CommitEntity(entity) => match entity {
            EntityType::LwPolyline(pl) => assert_eq!(pl.vertices[0].bulge, 0.0),
            _ => panic!("expected LwPolyline"),
        },
        _ => panic!("expected straight segment"),
    }
}

#[test]
fn first_segment_in_arc_mode_commits_on_second_click_with_bulge() {
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    assert!(matches!(cmd.on_text_input("ARC"), Some(CmdResult::NeedPoint)));
    match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(EntityType::LwPolyline(pl)) => {
            assert_ne!(pl.vertices[0].bulge, 0.0);
        }
        _ => panic!("expected committed first-segment arc"),
    }
}

#[test]
fn commits_two_vertex_segments_without_picking_a_style() {
    let mut cmd = WallCommand::new();
    assert!(
        cmd.style_id.is_some(),
        "seed library should auto-select a wall style"
    );
    assert!(
        cmd.requires_style_selection(),
        "seed library should offer wall styles"
    );
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    match cmd.on_point(DVec3::new(4.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(EntityType::LwPolyline(pl)) => {
            assert_eq!(pl.vertices.len(), 2);
        }
        _ => panic!("expected a 2-vertex commit"),
    }
    match cmd.on_point(DVec3::new(4.0, 3.0, 0.0)) {
        CmdResult::CommitEntity(EntityType::LwPolyline(pl)) => {
            assert_eq!(pl.vertices.len(), 2);
        }
        _ => panic!("expected a second 2-vertex commit"),
    }
    assert_eq!(cmd.segments_committed, 2);
    assert_eq!(cmd.vertices.len(), 1);
}

#[test]
fn third_segment_keeps_first_corner_joined() {
    let mut scene = Scene::new();
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let h1 = scene.add_entity(match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("seg 1"),
    });
    cmd.on_entities_committed(&mut scene, &[h1]);
    let h2 = scene.add_entity(match cmd.on_point(DVec3::new(5.0, 4.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("seg 2"),
    });
    cmd.on_entities_committed(&mut scene, &[h2]);
    let h3 = scene.add_entity(match cmd.on_point(DVec3::new(0.0, 4.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("seg 3"),
    });
    cmd.on_entities_committed(&mut scene, &[h3]);

    let peers2 = engine::owner_index::peers_of(&scene.document, h2);
    assert!(
        peers2.contains(&h1) && peers2.contains(&h3),
        "middle wall should stay joined to both neighbors, got {peers2:?}"
    );
    refresh_wall_after_axis_edit(&mut scene, h2, None, None, None);
    let peers2_after = engine::owner_index::peers_of(&scene.document, h2);
    assert!(
        peers2_after.contains(&h1) && peers2_after.contains(&h3),
        "regen/edit must not drop either join, got {peers2_after:?}"
    );
}

#[test]
fn undo_drops_the_last_vertex_and_removes_the_live_entity_below_two_points() {
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    assert!(matches!(cmd.on_text_input("U"), Some(CmdResult::NeedPoint)));
    assert!(cmd.vertices.is_empty());
}

#[test]
fn enter_before_two_points_keeps_the_command_running() {
    // Enter/Escape are global "finalize" keys that can fire while the
    // user is still only editing the live height/style panel fields
    // before the axis has a second point — there is nothing to finalize
    // yet, so the command must stay alive instead of cancelling.
    let mut enter_cmd = WallCommand::new_with_library(None);
    assert!(matches!(enter_cmd.on_enter(), CmdResult::NeedPoint));

    let mut escape_cmd = WallCommand::new_with_library(None);
    assert!(matches!(escape_cmd.on_escape(), CmdResult::NeedPoint));

    let mut one_point_cmd = WallCommand::new_with_library(None);
    one_point_cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    assert!(matches!(one_point_cmd.on_enter(), CmdResult::NeedPoint));
}

#[test]
fn enter_after_the_point_chain_finalizes_immediately_with_defaults() {
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("expected segment commit"),
    };
    let wall = wall_from_entity(&entity).expect("committed wall should carry WALL xdata");
    assert!((wall.total_thickness() - DEFAULT_WALL_THICKNESS).abs() < 1e-9);
    assert!((wall.height - DEFAULT_WALL_HEIGHT).abs() < 1e-9);
    assert!(matches!(cmd.on_enter(), CmdResult::Cancel));
}

#[test]
fn live_height_edit_before_enter_finalizes_with_the_edited_value() {
    use crate::command::LiveFieldValue;

    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    cmd.apply_live_property("wall_height", LiveFieldValue::Number(3.5));
    let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("expected segment commit"),
    };
    let wall = wall_from_entity(&entity).expect("committed wall should carry WALL xdata");
    assert!((wall.total_thickness() - DEFAULT_WALL_THICKNESS).abs() < 1e-9);
    assert!((wall.height - 3.5).abs() < 1e-9);
}

/// Draw four wall segments through `WallCommand` exactly as the
/// interactive host would (point chain, then height/thickness prompt),
/// then commit each finalized entity into a real `Scene`. Regression
/// check for `AEC_ROOM`'s closed-loop detection against interactively
/// drawn walls (previously only exercised against demo geometry).
#[test]
fn aec_room_detects_a_closed_loop_from_interactively_drawn_walls() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    let corners = [
        DVec3::new(0.0, 0.0, 0.0),
        DVec3::new(4.0, 0.0, 0.0),
        DVec3::new(4.0, 3.0, 0.0),
        DVec3::new(0.0, 3.0, 0.0),
        DVec3::new(0.0, 0.0, 0.0),
    ];

    for pair in corners.windows(2) {
        let mut cmd = WallCommand::new_with_library(None);
        let committed = match cmd.on_point(pair[0]) {
            CmdResult::NeedPoint => cmd.on_point(pair[1]),
            other => other,
        };
        let entity = match committed {
            CmdResult::CommitEntity(entity) => entity,
            _ => panic!("two points should commit a wall segment"),
        };
        let handle = scene.add_entity(entity);
        cmd.on_entities_committed(&mut scene, &[handle]);
        assert!(matches!(cmd.on_enter(), CmdResult::Cancel));
    }

    let mut command_line = CommandLine::default();
    aec_room(&mut scene, &mut command_line);

    let room_record = scene
        .document
        .entities()
        .filter_map(read_aec_record)
        .find(|r| matches!(r.values.first(), Some(XDataValue::String(k)) if k == "ROOM"))
        .expect("aec_room should have written a ROOM xdata record");
    let area = match room_record.values.get(2) {
        Some(XDataValue::Real(a)) => *a,
        _ => panic!("ROOM record should carry an area value"),
    };
    assert!(
        (area - 12.0).abs() < 1e-6,
        "expected the detected 4x3 wall loop to yield area 12.0, got {area}"
    );
}

/// `wall_from_entity` is the inverse of `wall_record` — the properties
/// panel reads a `Wall` this way to populate wall fields for a WALL-tagged
/// entity.
#[test]
fn wall_from_entity_reads_back_a_finalized_wall_record() {
    use crate::command::LiveFieldValue;

    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    cmd.apply_live_property("wall_height", LiveFieldValue::Number(3.5));
    let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("expected second point to commit the wall"),
    };

    let wall = wall_from_entity(&entity).expect("finalized entity should read back as a Wall");
    assert!((wall.total_thickness() - DEFAULT_WALL_THICKNESS).abs() < 1e-9);
    assert!((wall.height - 3.5).abs() < 1e-9);
    assert_eq!(wall.layers.len(), 1);
}

/// A plain (non-WALL-tagged) entity must not be misread as a wall — this
/// is what keeps the properties-panel Wall section from appearing on
/// regular polylines.
#[test]
fn wall_from_entity_returns_none_for_a_plain_polyline() {
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(1.0, 0.0)));
    let entity = EntityType::LwPolyline(pl);
    assert!(wall_from_entity(&entity).is_none());
}

/// Height writeback via `write_wall_height` keeps the rest of the WALL
/// record intact and still visible to the room segment collector.
#[test]
fn write_wall_height_updates_the_wall_xdata_in_place() {
    let mut scene = Scene::new();
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("two points should commit a wall segment"),
    };
    let handle = scene.add_entity(entity);

    assert!(write_wall_height(&mut scene, handle, 3.2));

    let updated = wall_from_entity(scene.document.get_entity(handle).unwrap())
        .expect("entity should still read back as a wall after the edit");
    assert!((updated.height - 3.2).abs() < 1e-9);
    assert!((updated.total_thickness() - DEFAULT_WALL_THICKNESS).abs() < 1e-9);

    // The AEC_ROOM segment collector still sees this wall after the edit.
    let segments = collect_wall_segments(&scene.document);
    assert_eq!(segments.len(), 1);
}

/// `write_wall_hatch_override` (Step 6 Properties panel section) sets,
/// updates, and clears the per-wall-instance hatch-angle override while
/// leaving the rest of the WALL record intact.
#[test]
fn write_wall_hatch_override_sets_updates_and_clears_the_override() {
    let mut scene = Scene::new();
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("two points should commit a wall segment"),
    };
    let handle = scene.add_entity(entity);

    // No override set initially.
    let initial = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    assert_eq!(initial.hatch_override, None);

    // Setting an override.
    let override_a = engine::display_component::ComponentStyleOverride {
        hatch_angle: Some(45.0),
        hatch_angle_relative: Some(false),
        ..Default::default()
    };
    assert!(write_wall_hatch_override(&mut scene, handle, Some(override_a.clone())));
    let updated = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    assert_eq!(updated.hatch_override, Some(override_a));
    // Unrelated fields untouched.
    assert!((updated.height - DEFAULT_WALL_HEIGHT).abs() < 1e-9);

    // Updating the override (e.g. angle field edited again).
    let override_b = engine::display_component::ComponentStyleOverride {
        hatch_angle: Some(90.0),
        hatch_angle_relative: Some(true),
        ..Default::default()
    };
    assert!(write_wall_hatch_override(&mut scene, handle, Some(override_b.clone())));
    let updated2 = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    assert_eq!(updated2.hatch_override, Some(override_b));

    // Clearing the override (checkbox unchecked).
    assert!(write_wall_hatch_override(&mut scene, handle, None));
    let cleared = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    assert_eq!(cleared.hatch_override, None);
}

#[test]
fn apply_phase_filter_with_no_filter_shows_everything_unstyled() {
    let new_phase = apply_phase_filter(PlanPhase::New, None);
    assert!(new_phase.visible);
    assert_eq!(new_phase.extra_style, None);

    let demolition = apply_phase_filter(PlanPhase::Demolition, None);
    assert!(demolition.visible);
    assert_eq!(
        demolition.extra_style.unwrap().line_type.as_deref(),
        Some("DASHED")
    );

    let existing = apply_phase_filter(PlanPhase::Existing, None);
    assert!(existing.visible);
    assert_eq!(
        existing.extra_style.unwrap().line_color,
        Some(acadrust::types::Color::Rgb {
            r: 136,
            g: 136,
            b: 136
        })
    );
}

#[test]
fn apply_phase_filter_hides_phases_missing_from_visible_phases() {
    let filter = PhaseFilter {
        visible_phases: vec![PlanPhase::New],
        demolition_style: None,
        existing_style: None,
    };
    assert!(apply_phase_filter(PlanPhase::New, Some(&filter)).visible);
    assert!(!apply_phase_filter(PlanPhase::Demolition, Some(&filter)).visible);
    assert!(!apply_phase_filter(PlanPhase::Existing, Some(&filter)).visible);
}

#[test]
fn apply_phase_filter_applies_demolition_and_existing_overlays() {
    let filter = PhaseFilter {
        visible_phases: vec![PlanPhase::New, PlanPhase::Demolition, PlanPhase::Existing],
        demolition_style: Some(engine::display_component::ComponentStyleOverride {
            line_type: Some("Dashed".to_string()),
            ..Default::default()
        }),
        existing_style: Some(engine::display_component::ComponentStyleOverride {
            line_color: Some(acadrust::types::Color::Rgb { r: 136, g: 136, b: 136 }),
            ..Default::default()
        }),
    };
    let demolition = apply_phase_filter(PlanPhase::Demolition, Some(&filter));
    assert!(demolition.visible);
    assert_eq!(demolition.extra_style.unwrap().line_type, Some("Dashed".to_string()));

    let existing = apply_phase_filter(PlanPhase::Existing, Some(&filter));
    assert!(existing.visible);
    assert_eq!(existing.extra_style.unwrap().line_color, Some(acadrust::types::Color::Rgb { r: 136, g: 136, b: 136 }));

    // `New` walls never pick up an overlay, even if visible.
    let new_phase = apply_phase_filter(PlanPhase::New, Some(&filter));
    assert!(new_phase.visible);
    assert_eq!(new_phase.extra_style, None);
}

#[test]
fn merge_phase_extra_only_overrides_contour2d() {
    let extra = engine::display_component::ComponentStyleOverride {
        line_type: Some("DASHED".to_string()),
        line_color: Some(acadrust::types::Color::Rgb { r: 255, g: 0, b: 0 }),
        ..Default::default()
    };
    let mut rules = engine::display_component::ComponentRuleSet::default();
    rules.visibility.insert(
        engine::display_component::WallComponentSlot::Layers2D
            .key()
            .to_string(),
        true,
    );
    merge_phase_extra_into_rules(&mut rules, extra.clone());
    let contour = rules
        .style_for(engine::display_component::WallComponentSlot::Contour2D)
        .expect("phase extra on envelope");
    assert_eq!(contour.line_type.as_deref(), Some("DASHED"));
    assert_eq!(
        contour.line_color,
        Some(acadrust::types::Color::Rgb { r: 255, g: 0, b: 0 })
    );
    assert!(
        rules
            .style_for(engine::display_component::WallComponentSlot::Layers2D)
            .is_none()
    );
    assert!(
        rules
            .style_for(engine::display_component::WallComponentSlot::LayerHatch2D)
            .is_none()
    );
    assert!(
        rules
            .style_for(engine::display_component::WallComponentSlot::Solid3D)
            .is_none()
    );
}

#[test]
fn wall_round_trip() {
    let layers = wls(&[
        ("Finish", 0.02, "Finish"),
        ("Brick", 0.10, "Structural"),
        ("Finish", 0.02, "Finish"),
    ]);
    let values = wall_record("style1", 3.0, 1, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    let pl = LwPolyline::new();
    let mut entity = EntityType::LwPolyline(pl);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = values;
    entity.common_mut().extended_data.add_record(record);

    let wall = wall_from_entity(&entity).expect("Should parse WALL");
    assert_eq!(wall.style_id, "style1");
    assert_eq!(wall.height, 3.0);
    assert_eq!(wall.storey_id, 1);
    assert_eq!(wall.layers.len(), 3);
    assert_eq!(wall.layers[1].material, "Brick");
    assert_eq!(wall.layers[1].thickness, 0.10);
    assert_eq!(wall.layers[1].function, "Structural");
    assert_eq!(wall.total_thickness(), 0.14);
    assert_eq!(wall.phase, PlanPhase::New);
}

#[test]
fn wall_from_entity_round_trips_phase() {
    let layers = wls(&[("Brick", 0.2, "Structural")]);
    let values = wall_record(
        "style1",
        3.0,
        0,
        &layers,
        &[],
        WallJustification::Center,
        PlanPhase::Demolition,
        None,
    );
    let pl = LwPolyline::new();
    let mut entity = EntityType::LwPolyline(pl);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = values;
    entity.common_mut().extended_data.add_record(record);

    let wall = wall_from_entity(&entity).expect("Should parse WALL");
    assert_eq!(wall.phase, PlanPhase::Demolition);
}

#[test]
fn wall_record_round_trips_control_planes() {
    let mut wall = Wall::new("style1", 2.8, 0);
    wall.layers = wls(&[("Brick", 0.2, "Structural")]);
    wall.base_plane_id = Some(Uuid::new_v4());
    wall.top_plane_id = Some(Uuid::new_v4());
    wall.base_offset = 0.1;
    wall.top_offset = -0.05;
    wall.base_origin = [0.0, 0.0, 1.1];
    wall.base_normal = [0.0, 0.0, 1.0];
    wall.top_origin = [0.0, 0.0, 3.95];
    wall.top_normal = [0.0, 0.0, 1.0];
    wall.height = wall.height_from_snapshot().unwrap_or(wall.height);

    let mut entity = EntityType::LwPolyline(LwPolyline::new());
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    entity.common_mut().extended_data.add_record(record);

    let back = wall_from_entity(&entity).expect("parse");
    assert_eq!(back.base_plane_id, wall.base_plane_id);
    assert_eq!(back.top_plane_id, wall.top_plane_id);
    assert!((back.base_offset - 0.1).abs() < 1e-9);
    assert!((back.height - 2.85).abs() < 1e-6);
}

#[test]
fn wall_record_round_trips_named_control_planes() {
    let mut wall = Wall::new("style1", 2.8, 0);
    wall.layers = wls(&[("Brick", 0.2, "Structural")]);
    wall.base_plane_id = Some(Uuid::new_v4());
    wall.top_plane_id = Some(Uuid::new_v4());
    wall.base_plane_name = Some("EG_ELEVATION".to_string());
    wall.top_plane_name = Some("EG_OKGH".to_string());
    wall.base_offset = 0.1;
    wall.top_offset = -0.05;

    let mut entity = EntityType::LwPolyline(LwPolyline::new());
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    entity.common_mut().extended_data.add_record(record);

    let back = wall_from_entity(&entity).expect("parse named planes");
    assert_eq!(back.base_plane_id, wall.base_plane_id);
    assert_eq!(back.top_plane_id, wall.top_plane_id);
    assert_eq!(back.base_plane_name.as_deref(), Some("EG_ELEVATION"));
    assert_eq!(back.top_plane_name.as_deref(), Some("EG_OKGH"));
}

#[test]
fn write_aec_record_drops_stale_dwg_eed_so_planes_can_save() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    ensure_app_id(&mut scene.document);
    let ah = scene
        .document
        .app_ids
        .get(AEC_APPID)
        .expect("AEC app")
        .handle
        .value();
    {
        let entity = scene.document.get_entity_mut(wall_handle).unwrap();
        entity
            .common_mut()
            .extended_data
            .raw_dwg_eed
            .push((ah, vec![0xDE, 0xAD]));
    }
    let mut wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();
    wall.base_plane_id = Some(Uuid::new_v4());
    wall.base_plane_name = Some("EG_ELEVATION".to_string());
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record_for_wall(&wall);
    assert!(write_aec_record(&mut scene.document, wall_handle, record));
    let entity = scene.document.get_entity(wall_handle).unwrap();
    assert!(
        entity
            .common()
            .extended_data
            .raw_dwg_eed
            .iter()
            .all(|(a, _)| *a != ah),
        "stale DWG EED must not win on the next save"
    );
    let back = wall_from_entity(entity).unwrap();
    assert_eq!(back.base_plane_name.as_deref(), Some("EG_ELEVATION"));
    assert_eq!(back.base_plane_id, wall.base_plane_id);
}

#[test]
fn wall_planes_survive_real_xdata_after_dwg_load() {
    let mut wall = Wall::new("style1", 2.8, 0);
    wall.layers = wls(&[("Brick", 0.2, "Structural")]);
    wall.base_plane_id = Some(Uuid::new_v4());
    wall.top_plane_id = Some(Uuid::new_v4());
    wall.base_offset = 0.15;
    wall.top_offset = -0.2;
    let mut values = wall_record_for_wall(&wall);
    let planes_at = values
        .iter()
        .rposition(|v| matches!(v, XDataValue::String(s) if s == "planes"))
        .unwrap();
    for v in values.iter_mut().skip(planes_at + 3) {
        if let XDataValue::Distance(d) = *v {
            *v = XDataValue::Real(d);
        }
    }
    let mut entity = EntityType::LwPolyline(LwPolyline::new());
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = values;
    entity.common_mut().extended_data.add_record(record);
    let back = wall_from_entity(&entity).expect("parse reals");
    assert_eq!(back.base_plane_id, wall.base_plane_id);
    assert_eq!(back.top_plane_id, wall.top_plane_id);
    assert!((back.base_offset - 0.15).abs() < 1e-9);
    assert!((back.top_offset + 0.2).abs() < 1e-9);
}

#[test]
fn wall_from_entity_ignores_missing_plane_block() {
    let layers = wls(&[("Brick", 0.2, "Structural")]);
    let mut values = wall_record(
        "style1",
        3.0,
        0,
        &layers,
        &[],
        WallJustification::Center,
        PlanPhase::New,
        None,
    );
    if let Some(pos) = values.iter().rposition(|v| {
        matches!(v, XDataValue::String(s) if s == "planes")
    }) {
        values.truncate(pos);
    }
    let mut entity = EntityType::LwPolyline(LwPolyline::new());
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = values;
    entity.common_mut().extended_data.add_record(record);
    let wall = wall_from_entity(&entity).expect("legacy");
    assert!(wall.base_plane_id.is_none());
    assert_eq!(wall.height, 3.0);
}

#[test]
fn wall_without_phase_tail_defaults_to_new() {
    // Simulates a record written before the `phase` field existed: the
    // trailing tag is simply absent, and parsing must fall back to
    // `PlanPhase::New` rather than failing.
    let layers = wls(&[("Brick", 0.2, "Structural")]);
    let mut values = wall_record(
        "style1",
        3.0,
        0,
        &layers,
        &[],
        WallJustification::Center,
        PlanPhase::Demolition,
        None,
    );
    if let Some(pos) = values.iter().position(|v| matches!(v, XDataValue::String(s) if s == "Demolition")) {
        values.truncate(pos);
    }
    let pl = LwPolyline::new();
    let mut entity = EntityType::LwPolyline(pl);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = values;
    entity.common_mut().extended_data.add_record(record);

    let wall = wall_from_entity(&entity).expect("Should parse WALL");
    assert_eq!(wall.phase, PlanPhase::New);
}

#[test]
fn write_wall_height_preserves_phase() {
    let mut scene = Scene::new();
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("two points should commit a wall segment"),
    };
    let handle = scene.add_entity(entity);

    // Manually flip the phase to `Existing`, mimicking a prior
    // Properties-panel edit, then confirm a later height edit preserves it.
    let mut wall = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    wall.phase = PlanPhase::Existing;
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record(
        &wall.style_id,
        wall.height,
        wall.storey_id,
        &wall.layers,
        &wall.derived_handles,
        wall.justification,
        wall.phase,
        wall.hatch_override.as_ref(),
    );
    write_aec_record(&mut scene.document, handle, record);

    assert!(write_wall_height(&mut scene, handle, 3.2));

    let updated = wall_from_entity(scene.document.get_entity(handle).unwrap())
        .expect("entity should still read back as a wall after the edit");
    assert_eq!(updated.phase, PlanPhase::Existing);
    assert!((updated.height - 3.2).abs() < 1e-9);
}

#[test]
fn wall_thickness_and_height_reads_wall_record() {
    let pl = LwPolyline::new();
    let mut entity = EntityType::LwPolyline(pl);
    let layers = vec![wl("Mat", 0.15, "Func")];
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = wall_record("style2", 3.2, 2, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(rec);

    let res = wall_thickness_and_height(&entity).expect("Should read WALL");
    assert_eq!(res, (0.15, 3.2, 2));
}

#[test]
fn aec_room_detects_a_closed_loop_from_walls() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    let corners = [
        DVec3::new(0.0, 0.0, 0.0),
        DVec3::new(4.0, 0.0, 0.0),
        DVec3::new(4.0, 3.0, 0.0),
        DVec3::new(0.0, 3.0, 0.0),
        DVec3::new(0.0, 0.0, 0.0),
    ];

    for pair in corners.windows(2) {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(pair[0].x, pair[0].y)));
        pl.add_vertex(LwVertex::new(Vector2::new(pair[1].x, pair[1].y)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Brick", 0.2, "Structural")];
        let mut r = ExtendedDataRecord::new(AEC_APPID);
        r.values = wall_record("style1", 2.8, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(r);
        scene.add_entity(entity);
    }

    let mut command_line = CommandLine::default();
    aec_room(&mut scene, &mut command_line);

    let room_record = scene
        .document
        .entities()
        .filter_map(read_aec_record)
        .find(|r| matches!(r.values.first(), Some(XDataValue::String(k)) if k == "ROOM"))
        .expect("aec_room should have written a ROOM xdata record");
    let area = match room_record.values.get(2) {
        Some(XDataValue::Real(a)) => *a,
        _ => panic!("ROOM record should carry an area value"),
    };
    assert!((area - 12.0).abs() < 1e-6);
}

#[test]
fn wall_command_with_library_uses_ask_style_and_finalizes() {
    use crate::command::LiveFieldValue;
    use crate::modules::aec::engine::material::Material;
    use crate::modules::aec::engine::style::Style;
    use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, WallStyle};

    let material = Material::new(
        "brick_id".to_string(),
        "Brick Material".to_string(),
        "ANSI31".to_string(),
        0xFF0000,
        "Continuous".to_string(),
    );
    let style = WallStyle {
        style: Style {
            id: "style1".to_string(),
            name: "Standard Wall".to_string(),
            object_kind: "Wall".to_string(),
            parent_style_id: None,
        },
        layers: vec![Layer {
            material_id: "brick_id".to_string(),
            thickness: LayerValue::Fixed(0.25),
            function: LayerFunction::Structural,
            axis_offset: LayerValue::Fixed(0.0),
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            role_tag: None,
        layer_id: uuid::Uuid::new_v4(),
        }],
    display_profiles: std::collections::HashMap::new(),
    };
    let lib = StyleLibrary {
        materials: vec![material],
        wall_styles: vec![style],
        ..Default::default()
    };

    let mut cmd = WallCommand::new_with_library(Some(lib));
    cmd.apply_live_property("wall_height", LiveFieldValue::Number(3.0));
    cmd.apply_live_property(
        "wall_style",
        LiveFieldValue::Picker("style1".to_string()),
    );
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => {
            let pl = match &entity {
                EntityType::LwPolyline(pl) => pl,
                _ => panic!("expected a wall polyline"),
            };
            let record = pl.common.extended_data.get_record(AEC_APPID).unwrap();
            assert_eq!(record.values[0], XDataValue::String("WALL".to_string()));
            assert_eq!(record.values[1], XDataValue::String("style1".to_string()));
            assert!(
                matches!(record.values[2], XDataValue::Distance(h) if (h - 3.0).abs() < 1e-9)
            );
            assert_eq!(
                record.values[5],
                XDataValue::String("Brick Material".to_string())
            );
            assert!(
                matches!(record.values[6], XDataValue::Distance(t) if (t - 0.25).abs() < 1e-9)
            );
            assert_eq!(
                record.values[7],
                XDataValue::String("Structural".to_string())
            );
        }
        _ => panic!("expected second click to commit a styled segment"),
    }
}

#[test]
fn wall_command_live_properties_reports_style_and_height_while_drawing() {
    use crate::command::LiveFieldValue;

    let mut cmd = WallCommand::new_with_library(None);
    // Still in the Drawing phase (no points yet): live_properties should
    // be available with the default height and an empty style name.
    let live = cmd.live_properties().expect("Drawing phase should expose live properties");
    assert_eq!(live.title, "Wall");
    assert_eq!(live.fields.len(), 3);
    assert_eq!(live.fields[0].field_id, "wall_style");
    assert_eq!(live.fields[1].field_id, "wall_height");
    assert_eq!(live.fields[2].field_id, "wall_justification");
    assert!(matches!(&live.fields[1].value, LiveFieldValue::Number(h) if (*h - DEFAULT_WALL_HEIGHT).abs() < 1e-9));

    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    cmd.on_point(DVec3::new(5.0, 0.0, 0.0));

    cmd.apply_live_property("wall_height", LiveFieldValue::Number(3.5));
    assert!((cmd.wall.height - 3.5).abs() < 1e-9);

    let live_after = cmd.live_properties().expect("still drawing");
    assert!(matches!(&live_after.fields[1].value, LiveFieldValue::Number(h) if (*h - 3.5).abs() < 1e-9));
}

#[test]
fn bound_wall_commit_keeps_live_height_and_rebake_height() {
    use crate::command::LiveFieldValue;
    use crate::modules::aec::engine::project::StoreyRef;

    let storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
    let mut cmd = WallCommand::new_with_library(None).with_storey_planes(&storey);
    assert!((cmd.wall.height - 3.0).abs() < 1e-9);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let unbound = match cmd.on_point(DVec3::new(4.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("expected commit"),
    };
    let baked = wall_from_entity(&unbound).expect("wall");
    assert!(
        (baked.height - 3.0).abs() < 1e-12,
        "rebake height must survive commit, got {}",
        baked.height
    );
    assert!((baked.base_origin[2] - 0.0).abs() < 1e-12);

    let mut cmd = WallCommand::new_with_library(None).with_storey_planes(&storey);
    cmd.wall.base_offset = 0.1;
    cmd.wall.top_offset = -0.05;
    cmd.wall.rebake_planes(&storey, 0.0, 0.0);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let offset_entity = match cmd.on_point(DVec3::new(4.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("expected commit"),
    };
    let offset_wall = wall_from_entity(&offset_entity).expect("wall");
    assert!(
        (offset_wall.height - 2.85).abs() < 1e-12,
        "offsets must not be applied twice on commit, got {}",
        offset_wall.height
    );
    assert!((offset_wall.base_origin[2] - 0.1).abs() < 1e-12);
    assert!((offset_wall.top_origin[2] - 2.95).abs() < 1e-12);

    let mut cmd = WallCommand::new_with_library(None).with_storey_planes(&storey);
    cmd.apply_live_property("wall_height", LiveFieldValue::Number(2.5));
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    let live_entity = match cmd.on_point(DVec3::new(4.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(e) => e,
        _ => panic!("expected commit"),
    };
    let live_wall = wall_from_entity(&live_entity).expect("wall");
    assert!(
        (live_wall.height - 2.5).abs() < 1e-12,
        "live Properties height must survive commit, got {}",
        live_wall.height
    );
}

#[test]
fn wall_command_apply_live_property_updates_style_and_resolved_layers() {
    use crate::command::LiveFieldValue;
    use crate::modules::aec::engine::material::Material;
    use crate::modules::aec::engine::style::Style;
    use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, WallStyle};

    let material = Material::new(
        "brick_id".to_string(),
        "Brick Material".to_string(),
        "ANSI31".to_string(),
        0xFF0000,
        "Continuous".to_string(),
    );
    let style = WallStyle {
        style: Style {
            id: "style1".to_string(),
            name: "Standard Wall".to_string(),
            object_kind: "Wall".to_string(),
            parent_style_id: None,
        },
        layers: vec![Layer {
            material_id: "brick_id".to_string(),
            thickness: LayerValue::Fixed(0.25),
            function: LayerFunction::Structural,
            axis_offset: LayerValue::Fixed(0.0),
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            role_tag: None,
        layer_id: uuid::Uuid::new_v4(),
        }],
    display_profiles: std::collections::HashMap::new(),
    };
    let lib = StyleLibrary {
        materials: vec![material],
        wall_styles: vec![style],
        ..Default::default()
    };

    let mut cmd = WallCommand::new_with_library(Some(lib));
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
    cmd.set_live_handles(vec![Handle::new(201)]);

    cmd.apply_live_property(
        "wall_style",
        LiveFieldValue::Picker("style1".to_string()),
    );

    assert_eq!(cmd.style_id.as_deref(), Some("style1"));
    let layers = cmd.resolved_layers.clone().expect("style pick should resolve layers");
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].material, "Brick Material");
    assert!((layers[0].thickness - 0.25).abs() < 1e-9);
    assert_eq!(layers[0].function, "Structural");

    let live = cmd.live_properties().expect("still drawing");
    assert!(matches!(&live.fields[0].value, LiveFieldValue::Picker(s) if s == "Standard Wall"));
}

#[test]
fn wall_command_refuses_to_finish_without_a_style_when_styles_are_available() {
    use crate::modules::aec::engine::material::Material;
    use crate::modules::aec::engine::style::Style;
    use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, WallStyle};

    let material = Material::new(
        "brick_id".to_string(),
        "Brick Material".to_string(),
        "ANSI31".to_string(),
        0xFF0000,
        "Continuous".to_string(),
    );
    let style = WallStyle {
        style: Style {
            id: "style1".to_string(),
            name: "Standard Wall".to_string(),
            object_kind: "Wall".to_string(),
            parent_style_id: None,
        },
        layers: vec![Layer {
            material_id: "brick_id".to_string(),
            thickness: LayerValue::Fixed(0.25),
            function: LayerFunction::Structural,
            axis_offset: LayerValue::Fixed(0.0),
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            role_tag: None,
        layer_id: uuid::Uuid::new_v4(),
        }],
    display_profiles: std::collections::HashMap::new(),
    };
    let lib = StyleLibrary {
        materials: vec![material],
        wall_styles: vec![style],
        ..Default::default()
    };

    let mut cmd = WallCommand::new_with_library(Some(lib));
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(_) => {}
        _ => panic!("segment should commit even without a style"),
    }
    assert!(matches!(cmd.on_enter(), CmdResult::Cancel));
}

#[test]
fn wall_command_with_no_library_falls_back_to_v1_record() {
    let mut cmd = WallCommand::new_with_library(None);
    cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
    match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
        CmdResult::CommitEntity(entity) => {
            let pl = match &entity {
                EntityType::LwPolyline(pl) => pl,
                _ => panic!("expected a wall polyline"),
            };
            let record = pl.common.extended_data.get_record(AEC_APPID).unwrap();
            assert_eq!(record.values[0], XDataValue::String("WALL".to_string()));
        }
        _ => panic!("Expected second point to commit wall with V1 record"),
    }
}

#[test]
fn material_command_collects_fields_and_dispatches_add_command() {
    let mut cmd = MaterialCommand::new();
    assert_eq!(cmd.prompt(), crate::tr!("aec", "material-prompt-name"));

    assert!(matches!(
        cmd.on_text_input("Sichtbeton"),
        Some(CmdResult::NeedPoint)
    ));
    assert_eq!(cmd.prompt(), crate::tr!("aec", "material-prompt-hatch"));

    assert!(matches!(
        cmd.on_text_input(""), // blank -> default hatch
        Some(CmdResult::NeedPoint)
    ));
    assert_eq!(cmd.prompt(), crate::tr!("aec", "material-prompt-color"));

    assert!(matches!(
        cmd.on_text_input("A0A0A0"),
        Some(CmdResult::NeedPoint)
    ));
    assert_eq!(cmd.prompt(), crate::tr!("aec", "material-prompt-linetype"));

    match cmd.on_text_input("") {
        Some(CmdResult::Dispatch(dispatch)) => {
            assert_eq!(
                dispatch,
                "AEC_MATERIAL_ADD Sichtbeton|ANSI31|A0A0A0|Continuous"
            );
        }
        _ => panic!("expected the final field to dispatch AEC_MATERIAL_ADD"),
    }
}

#[test]
fn material_command_requires_a_non_empty_name() {
    let mut cmd = MaterialCommand::new();
    assert!(matches!(cmd.on_text_input(""), Some(CmdResult::NeedPoint)));
    // Still on the name step.
    assert_eq!(cmd.prompt(), crate::tr!("aec", "material-prompt-name"));
}

#[test]
fn style_command_collects_two_layers_and_dispatches_add_command() {
    let mut cmd = StyleCommand::new();
    assert_eq!(cmd.prompt(), crate::tr!("aec", "style-prompt-name"));

    assert!(matches!(
        cmd.on_text_input("Testwand"),
        Some(CmdResult::NeedPoint)
    ));
    assert_eq!(cmd.prompt(), crate::tr!("aec", "style-prompt-parent"));

    assert!(matches!(
        cmd.on_text_input(""), // no parent
        Some(CmdResult::NeedPoint)
    ));
    assert_eq!(
        cmd.prompt(),
        crate::tr!("aec", "style-prompt-layer-material", n = 1i32)
    );

    // Layer 1
    assert!(matches!(
        cmd.on_text_input("Putz"),
        Some(CmdResult::NeedPoint)
    ));
    assert!(matches!(
        cmd.on_text_input("0.015"),
        Some(CmdResult::NeedPoint)
    ));
    assert!(matches!(
        cmd.on_text_input("Finish"),
        Some(CmdResult::NeedPoint)
    ));

    // Layer 2
    assert!(matches!(
        cmd.on_text_input("Mauerwerk"),
        Some(CmdResult::NeedPoint)
    ));
    assert!(matches!(
        cmd.on_text_input("0.24"),
        Some(CmdResult::NeedPoint)
    ));
    assert!(matches!(
        cmd.on_text_input("Structural"),
        Some(CmdResult::NeedPoint)
    ));

    // Blank material name ends the layer loop and dispatches.
    match cmd.on_text_input("") {
        Some(CmdResult::Dispatch(dispatch)) => {
            assert_eq!(
                dispatch,
                "AEC_STYLE_ADD Testwand||Putz:0.015:Finish;Mauerwerk:0.24:Structural"
            );
        }
        _ => panic!("expected the final blank layer entry to dispatch AEC_STYLE_ADD"),
    }
}

#[test]
fn style_command_with_no_layers_dispatches_empty_layer_list_for_inheritance() {
    let mut cmd = StyleCommand::new();
    cmd.on_text_input("Kind Wand");
    cmd.on_text_input("Standard Wall"); // parent

    match cmd.on_text_input("") {
        Some(CmdResult::Dispatch(dispatch)) => {
            assert_eq!(dispatch, "AEC_STYLE_ADD Kind Wand|Standard Wall|");
        }
        _ => panic!("expected an empty layer list to still dispatch AEC_STYLE_ADD"),
    }
}

#[test]
fn layer_function_round_trips_through_its_plain_string_form() {
    for f in [
        LayerFunction::Structural,
        LayerFunction::Insulation,
        LayerFunction::Finish,
    ] {
        let s = layer_function_to_str(&f);
        assert_eq!(parse_layer_function(&s), f);
    }
    assert_eq!(parse_layer_function(""), LayerFunction::Structural);
    assert_eq!(
        parse_layer_function("Custom"),
        LayerFunction::Other("Custom".to_string())
    );
}

#[test]
fn wall_justification_conversion() {
    // Horizontal wall along X axis from 0 to 10.
    // Thickness = 0.2.
    // Points picked at Interior (Y=+0.1 if drawing left to right).
    // Centerline should be at Y=0.

    let mut cmd = WallCommand::new_with_library(None);
    cmd.justification = WallJustification::Interior;
    cmd.thickness = 0.2;

    cmd.on_point(DVec3::new(0.0, 0.1, 0.0));
    let entity = match cmd.on_point(DVec3::new(10.0, 0.1, 0.0)) {
        CmdResult::CommitEntity(entity) => entity,
        _ => panic!("Should commit entity"),
    };
    let EntityType::LwPolyline(pl) = entity else {
        panic!("Expected LwPolyline")
    };

    assert_eq!(pl.vertices.len(), 2);
    // Interior justification for a segment (0, 0.1) -> (10, 0.1)
    // normal is (0, 1).
    // offset in build_entity is -0.1.
    // Centerline = Picked + (0, 1) * -0.1 = (0, 0).
    assert!((pl.vertices[0].location.x - 0.0).abs() < 1e-9);
    assert!((pl.vertices[0].location.y - 0.0).abs() < 1e-9);
    assert!((pl.vertices[1].location.x - 10.0).abs() < 1e-9);
    assert!((pl.vertices[1].location.y - 0.0).abs() < 1e-9);
}

#[test]
fn wall_justification_toggle_cycle() {
    let mut cmd = WallCommand::new();
    assert_eq!(cmd.justification, WallJustification::Center);

    cmd.set_ctrl(true);
    assert_eq!(cmd.justification, WallJustification::Exterior);
    cmd.set_ctrl(true); // Should not toggle again while held
    assert_eq!(cmd.justification, WallJustification::Exterior);

    cmd.set_ctrl(false);
    cmd.set_ctrl(true);
    assert_eq!(cmd.justification, WallJustification::Interior);

    cmd.set_ctrl(false);
    cmd.set_ctrl(true);
    assert_eq!(cmd.justification, WallJustification::Center);
}

#[test]
fn slugify_normalizes_names_into_stable_ids() {
    assert_eq!(slugify("Wand Stahlbeton 20cm"), "wand_stahlbeton_20cm");
    assert_eq!(slugify("  spaced  out  "), "spaced_out");
    assert_eq!(slugify(""), "item");
}

#[test]
fn unique_id_stays_readable_but_differs_across_calls() {
    // Two ids generated for the exact same name (e.g. the same wall
    // style name entered independently in two different projects) must
    // still differ, so libraries created independently never collide
    // when later mixed (see `unique_id` doc comment).
    let a = unique_id("style", "Mauerwerk 36.5");
    let b = unique_id("style", "Mauerwerk 36.5");
    assert_ne!(a, b);
    assert!(a.starts_with("style_mauerwerk_36_5_"));
    assert!(b.starts_with("style_mauerwerk_36_5_"));
}

#[test]
fn wall_round_trip_with_derived_handles() {
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let derived = vec![Handle::new(10), Handle::new(11), Handle::new(12)];
    let values = wall_record("style1", 3.0, 0, &layers, &derived, WallJustification::Center, PlanPhase::New, None);
    let pl = LwPolyline::new();
    let mut entity = EntityType::LwPolyline(pl);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = values;
    entity.common_mut().extended_data.add_record(record);

    let wall = wall_from_entity(&entity).expect("Should parse WALL");
    assert_eq!(wall.derived_handles, derived);
}

#[test]
fn wall_without_derived_handles_tail_still_parses() {
    // Simulate an old record written before `derived_handles` existed:
    // build it with an empty list and confirm it reads back empty, not
    // an error, keeping legacy records readable.
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    let pl = LwPolyline::new();
    let mut entity = EntityType::LwPolyline(pl);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = values;
    entity.common_mut().extended_data.add_record(record);

    let wall = wall_from_entity(&entity).expect("Should parse WALL");
    assert!(wall.derived_handles.is_empty());
}

/// Build a two-layer `WALL` axis polyline in `scene` and return its
/// handle.
fn add_multi_layer_wall(scene: &mut Scene) -> Handle {
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut entity = EntityType::LwPolyline(pl);
    let layers = wls(&[
        ("Concrete", 0.2, "Structural"),
        ("Insulation", 0.05, "Insulation"),
    ]);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    scene.add_entity(entity)
}

#[test]
fn demolition_regen_sets_dashed_envelope_linetype() {
    let mut scene = Scene::new();
    let handle = add_multi_layer_wall(&mut scene);
    assert!(write_wall_phase(&mut scene, handle, PlanPhase::Demolition));
    let extra = apply_phase_filter(PlanPhase::Demolition, None)
        .extra_style
        .expect("demolition extra");
    let mut rules = engine::display_component::ComponentRuleSet::default();
    merge_phase_extra_into_rules(&mut rules, extra);
    regenerate_wall_representation_with_rules_and_substitutions(
        &mut scene,
        handle,
        Some(&rules),
        None,
        None,
    )
    .expect("regen");
    let wall = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
    let dashed = wall
        .derived_handles
        .iter()
        .filter_map(|h| scene.document.get_entity(*h))
        .filter(|e| matches!(e, EntityType::LwPolyline(_)))
        .filter(|e| e.common().linetype.eq_ignore_ascii_case("DASHED"))
        .count();
    assert!(
        dashed >= 1,
        "demolition envelope contour must use DASHED linetype"
    );
}

/// Mirrors the core write-back logic of the `AecStylePickerConfirm`
/// handler for `StylePickerTarget::WallPropertiesStyle` (see
/// `src/app/update/mod.rs`): resolve `effective_layers()` for the newly
/// chosen style from the currently loaded library, then write the new
/// `style_id` + resolved layer snapshot back into the wall's `WALL`
/// XDATA and regenerate its representation.
#[test]
fn changing_wall_properties_style_updates_style_id_and_layer_snapshot() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);

    let mut wall_styles: HashMap<String, WallStyle> = HashMap::new();
    wall_styles.insert(
        "style1".to_string(),
        WallStyle {
            style: Style {
                id: "style1".to_string(),
                name: "Style One".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: vec![Layer {
                material_id: "Concrete".to_string(),
                thickness: LayerValue::Fixed(0.2),
                function: LayerFunction::Structural,
                axis_offset: LayerValue::Fixed(0.0),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: None,
            layer_id: uuid::Uuid::new_v4(),
            }],
        display_profiles: std::collections::HashMap::new(),
        },
    );
    wall_styles.insert(
        "style2".to_string(),
        WallStyle {
            style: Style {
                id: "style2".to_string(),
                name: "Style Two".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: vec![
                Layer {
                    material_id: "Brick".to_string(),
                    thickness: LayerValue::Fixed(0.1),
                    function: LayerFunction::Finish,
                    axis_offset: LayerValue::Fixed(0.0),
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    layer_override: None,
                    hatch_override: None,
                    role_tag: None,
                layer_id: uuid::Uuid::new_v4(),
                },
                Layer {
                    material_id: "Insulation".to_string(),
                    thickness: LayerValue::Fixed(0.06),
                    function: LayerFunction::Insulation,
                    axis_offset: LayerValue::Fixed(0.0),
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    layer_override: None,
                    hatch_override: None,
                    role_tag: None,
                layer_id: uuid::Uuid::new_v4(),
                },
            ],
        display_profiles: std::collections::HashMap::new(),
        },
    );

    let new_style_id = "style2".to_string();
    let bb = super::wall_style::base_width_from_layers(
        &super::wall_style::effective_layers(&wall_styles, &new_style_id)
            .expect("style2 should resolve"),
    );
    let layers = effective_layers_for_wall_bb(&wall_styles, &new_style_id, bb)
        .expect("style2 should resolve");
    let wall_layers: Vec<WallLayer> = layers
        .into_iter()
        .map(|l| WallLayer {
            material: l.material_id.clone(),
            thickness: l.thickness,
            function: match &l.function {
                LayerFunction::Structural => "Structural".to_string(),
                LayerFunction::Insulation => "Insulation".to_string(),
                LayerFunction::Finish => "Finish".to_string(),
                LayerFunction::Other(s) => s.clone(),
            },
            axis_offset: l.axis_offset,
            bottom_offset: l.bottom_offset,
            top_offset: l.top_offset,
            layer_override: l.layer_override.clone(),
            hatch_override: l.hatch_override.clone(),
        layer_id: uuid::Uuid::new_v4(),
        })
        .collect();

    let mut wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should parse as WALL");
    wall.style_id = new_style_id.clone();
    wall.layers = wall_layers.clone();

    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record(
        &wall.style_id,
        wall.height,
        wall.storey_id,
        &wall.layers,
        &wall.derived_handles,
        wall.justification, wall.phase, wall.hatch_override.as_ref());

    let entity = scene.document.get_entity_mut(wall_handle).unwrap();
    let xd = &mut entity.common_mut().extended_data;
    let kept: Vec<_> = xd
        .records()
        .iter()
        .filter(|r| r.application_name != AEC_APPID)
        .cloned()
        .collect();
    xd.clear();
    for r in kept {
        xd.add_record(r);
    }
    xd.add_record(record);

    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed after style change");

    let updated = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still parse as WALL");
    assert_eq!(updated.style_id, "style2");
    assert_eq!(updated.layers.len(), 2);
    assert_eq!(updated.layers[0].material, "Brick");
    assert_eq!(updated.layers[1].material, "Insulation");
}

#[test]
fn resolve_wall_package_returns_axis_for_a_derived_entity() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a valid two-layer wall");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles;
    assert!(!derived.is_empty());

    for handle in derived {
        assert_eq!(resolve_wall_package(&scene, handle), wall_handle);
    }
}

#[test]
fn resolve_wall_package_uses_derived_handles_without_child_xdata() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed");
    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("WALL")
        .derived_handles;
    assert!(!derived.is_empty());
    let child = derived[0];
    if let Some(entity) = scene.document.get_entity_mut(child) {
        entity.common_mut().extended_data = acadrust::xdata::ExtendedData::default();
    }
    assert_eq!(resolve_wall_package(&scene, child), wall_handle);
    let pkg = wall_package_handles(&scene, wall_handle);
    assert!(pkg.contains(&child));
    assert!(pkg.contains(&wall_handle));
}

#[test]
fn resolve_wall_package_reads_wall_rep_when_not_first_aec_record() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed");
    let child = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("WALL")
        .derived_handles[0];
    if let Some(entity) = scene.document.get_entity_mut(child) {
        let mut dummy = ExtendedDataRecord::new(AEC_APPID);
        dummy.add_value(XDataValue::String("OTHER".into()));
        let mut records = vec![dummy];
        records.extend(entity.common().extended_data.records().iter().cloned());
        entity.common_mut().extended_data.clear();
        for r in records {
            entity.common_mut().extended_data.add_record(r);
        }
    }
    assert_eq!(resolve_wall_package(&scene, child), wall_handle);
}

#[test]
fn resolve_wall_package_accepts_integer_axis_handle() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed");
    let child = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("WALL")
        .derived_handles[0];
    if let Some(entity) = scene.document.get_entity_mut(child) {
        entity.common_mut().extended_data.clear();
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.add_value(XDataValue::String("WALL_REP".into()));
        record.add_value(XDataValue::Integer32(wall_handle.value() as i32));
        record.add_value(XDataValue::String(WALL_REP_ROLE_CONTOUR.into()));
        entity.common_mut().extended_data.add_record(record);
    }
    assert_eq!(resolve_wall_package(&scene, child), wall_handle);
}

#[test]
fn resolve_wall_package_matches_untagged_layer_polyline_by_geometry() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed");
    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("WALL")
        .derived_handles;
    let child = derived
        .iter()
        .copied()
        .find(|&h| matches!(scene.document.get_entity(h), Some(EntityType::LwPolyline(pl)) if pl.vertices.len() >= 3))
        .expect("layer contour");
    if let Some(entity) = scene.document.get_entity_mut(child) {
        entity.common_mut().extended_data = acadrust::xdata::ExtendedData::default();
    }
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        panic!("wall");
    };
    let mut wall = wall_from_entity(entity).expect("WALL");
    wall.derived_handles.clear();
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    for v in wall_record(
        &wall.style_id,
        wall.height,
        wall.storey_id,
        &wall.layers,
        &wall.derived_handles,
        wall.justification,
        wall.phase,
        wall.hatch_override.as_ref(),
    ) {
        record.add_value(v);
    }
    write_aec_record(&mut scene.document, wall_handle, record);
    assert_eq!(resolve_wall_package(&scene, child), wall_handle);
}

#[test]
fn resolve_wall_package_matches_untagged_solid_by_geometry() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed");
    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("WALL")
        .derived_handles;
    let child = derived
        .iter()
        .copied()
        .find(|&h| matches!(scene.document.get_entity(h), Some(EntityType::Solid3D(_))))
        .expect("layer solid");
    if let Some(entity) = scene.document.get_entity_mut(child) {
        entity.common_mut().extended_data = acadrust::xdata::ExtendedData::default();
    }
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        panic!("wall");
    };
    let mut wall = wall_from_entity(entity).expect("WALL");
    wall.derived_handles.clear();
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    for v in wall_record(
        &wall.style_id,
        wall.height,
        wall.storey_id,
        &wall.layers,
        &wall.derived_handles,
        wall.justification,
        wall.phase,
        wall.hatch_override.as_ref(),
    ) {
        record.add_value(v);
    }
    write_aec_record(&mut scene.document, wall_handle, record);
    assert_eq!(resolve_wall_package(&scene, child), wall_handle);
    let expanded = expand_handles_for_wall_packages(&scene, &[child]);
    assert!(expanded.contains(&wall_handle));
}

#[test]
fn regen_tags_display_children_with_wall_rep_roles() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a valid two-layer wall");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles;
    let mut saw_contour = false;
    let mut saw_hatch = false;
    let mut saw_solid = false;
    for handle in derived {
        let entity = scene.document.get_entity(handle).unwrap();
        let record = read_aec_record(entity).expect("display child must carry WALL_REP");
        match record.values.as_slice() {
            [XDataValue::String(kind), XDataValue::Handle(axis), XDataValue::String(role)] => {
                assert_eq!(kind, "WALL_REP");
                assert_eq!(*axis, wall_handle);
                match role.as_str() {
                    WALL_REP_ROLE_CONTOUR => saw_contour = true,
                    WALL_REP_ROLE_HATCH => saw_hatch = true,
                    WALL_REP_ROLE_SOLID => saw_solid = true,
                    other => panic!("unexpected display role {other}"),
                }
            }
            other => panic!("unexpected display XDATA {other:?}"),
        }
    }
    assert!(saw_contour && saw_hatch && saw_solid);
}

#[test]
fn wall_hatch_scale_survives_document_clone_roundtrip() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles;
    let hatch_handles: Vec<_> = derived
        .into_iter()
        .filter(|&handle| {
            matches!(
                scene.document.get_entity(handle),
                Some(EntityType::Hatch(_))
            )
        })
        .collect();
    assert!(!hatch_handles.is_empty());

    let before: Vec<(f32, f32)> = hatch_handles
        .iter()
        .map(|handle| {
            let model = scene.hatches.get(handle).expect("hatch model");
            (model.scale, effective_hatch_spacing(model))
        })
        .collect();

    let mut reloaded = Scene::new();
    reloaded.document = scene.document.clone();
    reloaded.populate_hatches_from_document();

    for (handle, (scale, spacing)) in hatch_handles.iter().zip(before) {
        let model = reloaded.hatches.get(handle).expect("reloaded hatch model");
        assert!(
            (model.scale - scale).abs() < 1e-5,
            "HatchModel.scale changed on reload: {} -> {}",
            scale,
            model.scale
        );
        assert!(
            (effective_hatch_spacing(model) - spacing).abs() < 1e-4,
            "effective hatch spacing changed on reload"
        );
    }
}

fn effective_hatch_spacing(model: &crate::scene::model::hatch_model::HatchModel) -> f32 {
    match &model.pattern {
        crate::scene::model::hatch_model::HatchPattern::Pattern(fams) => {
            fams.first().map(|f| f.dy.abs() * model.scale.max(1e-6)).unwrap_or(0.0)
        }
        _ => 0.0,
    }
}

#[test]
fn regenerating_many_walls_completes_quickly() {
    // Rough performance smoke test (not a micro-benchmark): regenerating
    // a batch of walls — mixing straight and curved axes, multiple
    // layers, and openings — must not show a gross performance
    // regression (e.g. an accidental O(n^2) added by a future change).
    // Generous wall-clock bound so this stays robust on slow/loaded CI
    // hardware while still catching an order-of-magnitude regression.
    use std::time::Instant;

    let mut scene = Scene::new();
    let mut handles = Vec::new();
    const WALL_COUNT: usize = 200;
    for i in 0..WALL_COUNT {
        let x0 = i as f64 * 6.0;
        let mut pl = LwPolyline::new();
        if i % 3 == 0 {
            // Every third wall is curved.
            let mut v0 = LwVertex::new(Vector2::new(x0, 0.0));
            v0.bulge = 0.3;
            pl.add_vertex(v0);
        } else {
            pl.add_vertex(LwVertex::new(Vector2::new(x0, 0.0)));
        }
        pl.add_vertex(LwVertex::new(Vector2::new(x0 + 5.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = wls(&[("Concrete", 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values =
            wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        handles.push(scene.add_entity(entity));
    }

    let started = Instant::now();
    for h in &handles {
        regenerate_wall_representation(&mut scene, *h, None)
            .expect("regeneration should succeed for every generated wall");
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed.as_secs() < 10,
        "regenerating {WALL_COUNT} walls took {elapsed:?}, expected well under 10s"
    );
}

#[test]
fn tessellate_ring_with_bulges_is_a_noop_for_straight_rings() {
    let ring = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 5.0), (0.0, 5.0)];
    let bulges = vec![0.0; 4];
    assert_eq!(tessellate_ring_with_bulges(&ring, &bulges), ring);
    // Absent bulges (shorter slice / empty) also stay a no-op.
    assert_eq!(tessellate_ring_with_bulges(&ring, &[]), ring);
}

#[test]
fn tessellate_ring_with_bulges_densifies_arc_edges() {
    // A ring whose first edge is a semicircular arc (bulge = 1.0) from
    // (-1,0) to (1,0); the rest are straight closing edges.
    let ring = vec![(-1.0, 0.0), (1.0, 0.0), (1.0, -2.0), (-1.0, -2.0)];
    let bulges = vec![1.0, 0.0, 0.0, 0.0];
    let dense = tessellate_ring_with_bulges(&ring, &bulges);
    assert!(
        dense.len() > ring.len(),
        "an arc edge must be sampled into more than its two endpoints"
    );
    // Straight edges keep exactly their start vertex (no extra samples);
    // only the arc edge grows.
    assert_eq!(dense.len(), WALL_HATCH_ARC_SEGMENTS + 3);
    // The arc bulges outward from the chord: some sampled point should
    // be well off the y=0 chord line (the semicircle's midpoint sits a
    // full radius away, at y = ±1 depending on winding/bulge sign).
    assert!(dense.iter().any(|&(_, y)| y.abs() > 0.5));
}

#[test]
fn regenerate_wall_representation_tessellates_hatch_boundary_for_curved_wall() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    // Semicircular arc axis (bulge = 1.0) so the layer footprint has a
    // curved edge.
    let mut v0 = LwVertex::new(Vector2::new(-2.0, 0.0));
    v0.bulge = 1.0;
    pl.add_vertex(v0);
    pl.add_vertex(LwVertex::new(Vector2::new(2.0, 0.0)));
    let mut entity = EntityType::LwPolyline(pl);
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let wall_handle = scene.add_entity(entity);

    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a curved single-layer wall");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles;

    let mut found_dense_hatch = false;
    for h in derived {
        if let EntityType::Hatch(hatch) = scene.document.get_entity(h).unwrap() {
            // A curved single-layer footprint has 4 raw vertices (2 per
            // side); the tessellated boundary must be denser.
            if hatch.paths.iter().any(|p| {
                p.edges.iter().any(|e| {
                    matches!(e, acadrust::entities::hatch::BoundaryEdge::Polyline(pl) if pl.vertices.len() > 4)
                })
            }) {
                found_dense_hatch = true;
            }
        }
    }
    assert!(
        found_dense_hatch,
        "curved wall's hatch boundary should be tessellated into more than the raw 4 corner vertices"
    );
}

#[test]
fn regen_uses_layer_hatch_override_for_hatch_pattern() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut entity = EntityType::LwPolyline(pl);
    let mut overridden = wl("Concrete", 0.2, "Structural");
    overridden.hatch_override = Some("NET".to_string());
    let default_layer = wl("Insulation", 0.05, "Insulation");
    let layers = vec![overridden, default_layer];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let wall_handle = scene.add_entity(entity);

    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a valid two-layer wall");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles;

    let mut saw_override_pattern = false;
    let mut saw_default_pattern = false;
    for h in derived {
        if let EntityType::Hatch(hatch) = scene.document.get_entity(h).unwrap() {
            if hatch.pattern.name == "NET" {
                saw_override_pattern = true;
            } else if hatch.pattern.name == "ANSI31" {
                saw_default_pattern = true;
            }
        }
    }
    assert!(
        saw_override_pattern,
        "the overridden layer's hatch should use the layer's hatch_override pattern"
    );
    assert!(
        saw_default_pattern,
        "the non-overridden layer should keep falling back to the material/default pattern"
    );
}

#[test]
fn wall_from_entity_round_trips_layer_hatch_override() {
    let mut layer = wl("Concrete", 0.2, "Structural");
    layer.hatch_override = Some("NET".to_string());
    let layers = vec![layer, wl("Insulation", 0.05, "Insulation")];
    let values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    let pl = LwPolyline::new();
    let mut entity = EntityType::LwPolyline(pl);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = values;
    entity.common_mut().extended_data.add_record(record);

    let wall = wall_from_entity(&entity).expect("should parse WALL");
    assert_eq!(wall.layers[0].hatch_override.as_deref(), Some("NET"));
    assert_eq!(wall.layers[1].hatch_override, None);
}

#[test]
fn layer_filter_for_contour_and_solid_can_differ_within_one_profile() {
    use engine::display_component::{ComponentRuleSet, LayerSelection, WallComponentSlot};
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    let layer_ref = join::LayerRef { material_id: "Concrete".to_string(), role_tag: None, index: 0, layer_id: None };
    let mut rules = ComponentRuleSet::default();
    rules.layer_filter.insert(
        WallComponentSlot::Contour2D.key().to_string(),
        LayerSelection::Explicit(vec![layer_ref.clone()]),
    );
    // `Solid3D` left unset -> defaults to `All` (non-regression).
    let handles = regenerate_wall_representation_with_rules_and_substitutions(
        &mut scene,
        wall_handle,
        Some(&rules),
        None,
        None,
    )
    .expect("regeneration should succeed");
    assert!(!handles.is_empty());
    assert_eq!(rules.layer_filter_for(WallComponentSlot::Contour2D), &LayerSelection::Explicit(vec![layer_ref]));
    assert_eq!(rules.layer_filter_for(WallComponentSlot::Solid3D), &LayerSelection::All);

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles;
    let contour_count = derived
        .iter()
        .filter(|handle| matches!(scene.document.get_entity(**handle), Some(EntityType::LwPolyline(_))))
        .count();
    let solid_count = derived
        .iter()
        .filter(|handle| matches!(scene.document.get_entity(**handle), Some(EntityType::Solid3D(_))))
        .count();
    assert_eq!(contour_count, 1, "Contour2D must include only the explicit layer");
    assert_eq!(solid_count, 2, "Solid3D must keep both layers when its filter defaults to All");
}

#[test]
fn hatch_override_chain_wall_takes_precedence_over_material() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut entity = EntityType::LwPolyline(pl);
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    let wall_hatch_override = engine::display_component::ComponentStyleOverride {
        hatch_angle: Some(45.0),
        hatch_angle_relative: Some(false),
        ..Default::default()
    };
    record.values = wall_record(
        "style1",
        3.0,
        0,
        &layers,
        &[],
        WallJustification::Center,
        PlanPhase::New,
        Some(&wall_hatch_override),
    );
    entity.common_mut().extended_data.add_record(record);
    let wall_handle = scene.add_entity(entity);

    let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should parse WALL");
    assert_eq!(wall.hatch_override, Some(wall_hatch_override));

    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a single-layer wall");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles;
    let mut hatch_count = 0;
    for handle in derived {
        if let Some(EntityType::Hatch(hatch)) = scene.document.get_entity(handle) {
            hatch_count += 1;
            assert!(
                (hatch.pattern_angle - std::f64::consts::FRAC_PI_4).abs() < 1.0e-5,
                "wall hatch override should render at 45 degrees, got {} radians",
                hatch.pattern_angle
            );
        }
    }
    assert_eq!(hatch_count, 1, "the single-layer wall should produce one hatch");
}

#[test]
fn apply_phase_filter_hides_and_overlays_as_expected_end_to_end() {
    use engine::plan_view::{PhaseFilter, PlanPhase as Phase};
    let filter = PhaseFilter {
        visible_phases: vec![Phase::New, Phase::Demolition],
        demolition_style: Some(engine::display_component::ComponentStyleOverride {
            line_color: Some(acadrust::types::Color::Rgb { r: 255, g: 0, b: 0 }),
            ..Default::default()
        }),
        existing_style: None,
    };
    assert!(apply_phase_filter(Phase::New, Some(&filter)).visible);
    assert!(apply_phase_filter(Phase::Demolition, Some(&filter)).visible);
    assert!(!apply_phase_filter(Phase::Existing, Some(&filter)).visible);
    assert_eq!(
        apply_phase_filter(Phase::Demolition, Some(&filter)).extra_style.unwrap().line_color, Some(acadrust::types::Color::Rgb { r: 255, g: 0, b: 0 })
    );
}

#[test]
fn resolve_wall_package_returns_itself_for_a_non_wall_entity() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(1.0, 0.0)));
    let handle = scene.add_entity(EntityType::LwPolyline(pl));

    assert_eq!(resolve_wall_package(&scene, handle), handle);
}

#[test]
fn expand_handles_for_wall_packages_includes_owner_and_children() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a valid two-layer wall");
    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles;
    assert!(!derived.is_empty());

    let from_child = expand_handles_for_wall_packages(&scene, &[derived[0]]);
    let from_owner = expand_handles_for_wall_packages(&scene, &[wall_handle]);
    assert!(from_child.contains(&wall_handle));
    for handle in &derived {
        assert!(from_child.contains(handle));
        assert!(from_owner.contains(handle));
    }
    assert_eq!(from_child.len(), from_owner.len());
    assert!(!is_wall_derived_non_axis(&scene, wall_handle));
    for handle in &derived {
        assert!(is_wall_derived_non_axis(&scene, *handle));
    }
}

#[test]
fn expand_with_wall_derived_handles_resolves_child_to_full_package() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a valid two-layer wall");
    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles;
    assert!(!derived.is_empty());

    let mut handles = vec![derived[0]];
    expand_with_wall_derived_handles(&scene, &mut handles);
    assert!(handles.contains(&wall_handle));
    for handle in &derived {
        assert!(handles.contains(handle));
    }
}

#[test]
fn write_wall_height_from_derived_child_updates_owner() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a valid two-layer wall");
    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles[0];
    assert!(write_wall_height(&mut scene, derived, 4.2));
    let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("owner should still parse as WALL");
    assert!((wall.height - 4.2).abs() < 1e-9);
}

#[test]
fn live_wall_properties_include_style_height_and_justification() {
    use crate::command::LiveFieldValue;
    let cmd = WallCommand::new_with_library(None);
    let props = cmd.live_properties().expect("drawing phase exposes live props");
    let ids: Vec<_> = props.fields.iter().map(|f| f.field_id).collect();
    assert!(ids.contains(&"wall_style"));
    assert!(ids.contains(&"wall_height"));
    assert!(ids.contains(&"wall_justification"));
    assert!(matches!(
        props.fields.iter().find(|f| f.field_id == "wall_justification"),
        Some(f) if matches!(f.value, LiveFieldValue::Choice { .. })
    ));
}

#[test]
fn resolve_wall_package_falls_back_to_clicked_when_axis_is_missing() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    let handle = scene.add_entity(EntityType::LwPolyline(pl));

    // Tag it as derived from a handle that doesn't exist in the document.
    let stale_axis = Handle::new(999_999);
    write_wall_derived_tag(&mut scene, handle, stale_axis);

    assert_eq!(resolve_wall_package(&scene, handle), handle);
}

/// Bug 2: only the wall AXIS should ever be a snap candidate — never its
/// derived shell/hatch/solid contours.
#[test]
fn wall_axis_snap_wires_excludes_derived_and_includes_axis() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a valid two-layer wall");

    let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL");
    assert!(!wall.derived_handles.is_empty());

    // Sanity: the axis lives on the invisible axis layer; its derived
    // contour does not.
    assert_eq!(
        scene.document.get_entity(wall_handle).unwrap().common().layer,
        AEC_WALL_AXIS_LAYER
    );
    let derived_handle = wall.derived_handles[0];
    assert_ne!(
        scene
            .document
            .get_entity(derived_handle)
            .unwrap()
            .common()
            .layer,
        AEC_WALL_AXIS_LAYER
    );

    // Simulate the generic (render-shared) wire set: since the axis
    // layer is off, only the derived entity's wire is present there —
    // the axis itself is entirely absent.
    let derived_entity = scene.document.get_entity(derived_handle).unwrap().clone();
    let raw_wires = std::sync::Arc::new(scene.wires_for_entities(&[derived_entity]));
    assert!(raw_wires
        .iter()
        .any(|w| w.name == derived_handle.value().to_string()));

    let filtered = wall_axis_snap_wires(&scene, raw_wires);

    assert!(
        !filtered
            .iter()
            .any(|w| w.name == derived_handle.value().to_string()),
        "wall-derived contour/hatch/solid wires must be excluded from snap candidates"
    );
    assert!(
        filtered
            .iter()
            .any(|w| w.name == wall_handle.value().to_string()),
        "the wall axis must be included as a snap candidate even though its layer is off"
    );
}

/// Bug 4: after two walls are joined, each wall's contour must extend
/// past the shared corner into the other wall's footprint — verifying
/// the pragmatic corner-overlap fix in `join_two_walls_in_document` /
/// `regenerate_wall_representation_with_corner`.
#[test]
fn join_two_walls_extends_contours_into_shared_corner() {
    let mut scene = Scene::new();
    // Wall A: (0,0) -> (5,0), two layers, total thickness 0.25.
    let wall_a = add_multi_layer_wall(&mut scene);

    // Wall B: (6,1) -> (6,10), one layer, thickness 0.3. Meets wall A in
    // an L-junction at (6,0) — mirrors `join::tests::test_l_join`.
    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = vec![wl("Concrete", 0.3, "Structural")];
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    regenerate_wall_representation(&mut scene, wall_a, None)
        .expect("wall A regeneration should succeed");
    regenerate_wall_representation(&mut scene, wall_b, None)
        .expect("wall B regeneration should succeed");

    let (kind, _touched) = join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None)
        .expect("the two axes should join as an L-corner");
    assert_eq!(kind, JoinKind::L);

    // The trimmed axes must still meet exactly at the corner — the
    // corner-extension hint must not leak into the persisted axis.
    let axis_a = get_wall_vertices(&scene, wall_a);
    let axis_b = get_wall_vertices(&scene, wall_b);
    assert_eq!(*axis_a.last().unwrap(), DVec3::new(6.0, 0.0, 0.0));
    assert_eq!(*axis_b.first().unwrap(), DVec3::new(6.0, 0.0, 0.0));

    // Wall A's contour (thickness 0.25) should reach past x=6 by (at
    // least close to) wall B's half thickness (0.15), overlapping into
    // wall B's own footprint instead of stopping flush at the corner.
    let wall_a_v2 = wall_from_entity(scene.document.get_entity(wall_a).unwrap())
        .expect("wall A should still read back as WALL");
    let max_x = wall_a_v2
        .derived_handles
        .iter()
        .filter_map(|h| scene.document.get_entity(*h))
        .filter_map(|e| match e {
            EntityType::LwPolyline(pl) => Some(pl),
            _ => None,
        })
        .flat_map(|pl| pl.vertices.iter().map(|v| v.location.x))
        .fold(f64::MIN, f64::max);
    assert!(
        max_x > 6.0 + 1e-6,
        "wall A's contour should extend past the corner into wall B's footprint, got max_x={max_x}"
    );
}

/// Step 2: a `NoExtend` `layer_pairs` override on wall A's structural
/// layer must stop that specific layer from being extended into the L
/// corner, while the other (insulation) layer still auto-miters exactly
/// as in `join_two_walls_extends_contours_into_shared_corner`.
#[test]
fn join_junction_override_no_extend_keeps_one_layer_un_joined() {
    let mut scene = Scene::new();
    let wall_a = add_multi_layer_wall(&mut scene);

    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = vec![wl("Concrete", 0.3, "Structural")];
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    regenerate_wall_representation(&mut scene, wall_a, None)
        .expect("wall A regeneration should succeed");
    regenerate_wall_representation(&mut scene, wall_b, None)
        .expect("wall B regeneration should succeed");

    // Wall A's axis end that will join is its last vertex (end_index 1).
    let override_data = join::JunctionOverride {
        default_style: None,
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            },
            layer_b: None,
            style: join::JoinOverrideStyle::NoExtend,
        }],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, wall_a, 1, &override_data));
    assert_eq!(
        read_junction_override(&scene, wall_a, 1),
        Some(override_data)
    );

    let (kind, _touched) = join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None)
        .expect("the two axes should join as an L-corner");
    assert_eq!(kind, JoinKind::L);

    let wall_a_v2 = wall_from_entity(scene.document.get_entity(wall_a).unwrap())
        .expect("wall A should still read back as WALL");
    let contour_max_x: Vec<f64> = wall_a_v2
        .derived_handles
        .iter()
        .filter_map(|h| scene.document.get_entity(*h))
        .filter_map(|e| match e {
            EntityType::LwPolyline(pl) => Some(pl),
            _ => None,
        })
        .map(|pl| {
            pl.vertices
                .iter()
                .map(|v| v.location.x)
                .fold(f64::MIN, f64::max)
        })
        .collect();
    assert!(
        !contour_max_x.is_empty(),
        "wall A should still have contour polylines"
    );
    // With the Concrete layer forced NoExtend, no contour piece should
    // reach past wall B's half-thickness the way the fully-automatic
    // regression case does — at most one derived contour (Insulation)
    // may still extend into the corner.
    let extending = contour_max_x.iter().filter(|&&x| x > 6.0 + 1e-6).count();
    assert!(
        extending <= 1,
        "the NoExtend Concrete layer must not extend past the corner, got max_x values {contour_max_x:?}"
    );
}

/// Step 4a: a `layer_style_override` set on one of two same-material
/// layers (disambiguated by `layer_id`) must stay attached to the
/// originally intended layer even after a third layer is inserted
/// before it, shifting its `index`. Matching purely by `index` (or by
/// `material_id` alone, since both layers share it) would either lose
/// the override or misapply it to the wrong/both layers.
#[test]
fn layer_style_override_stays_attached_to_intended_layer_after_insert() {
    let target_id = uuid::Uuid::new_v4();
    let other_id = uuid::Uuid::new_v4();
    let rules = engine::display_component::ComponentRuleSet {
        layer_style_override: vec![engine::display_component::LayerStyleOverride {
            layer: join::LayerRef {
                material_id: "Plaster".to_string(),
                role_tag: None,
                index: 1,
                layer_id: Some(target_id),
            },
            style: engine::display_component::ComponentStyleOverride {
                line_type: Some("Dashed".to_string()),
                ..Default::default()
            },
        }],
        ..Default::default()
    };

    // Original stack: [Plaster(other_id) @0, Plaster(target_id) @1].
    let original_ref = join::LayerRef {
        material_id: "Plaster".to_string(),
        role_tag: None,
        index: 1,
        layer_id: Some(target_id),
    };
    let before = resolve_layer_style_override(Some(&rules), &[], &original_ref);
    assert_eq!(before.line_type, Some("Dashed".to_string()));

    // A third layer is inserted before it, shifting the target layer's
    // index to 2 — but its `layer_id` is unchanged.
    let shifted_ref = join::LayerRef {
        material_id: "Plaster".to_string(),
        role_tag: None,
        index: 2,
        layer_id: Some(target_id),
    };
    let after = resolve_layer_style_override(Some(&rules), &[], &shifted_ref);
    assert_eq!(
        after.line_type,
        Some("Dashed".to_string()),
        "override must stay attached to the layer by layer_id even though its index moved"
    );

    // The other same-material layer (a different layer_id) must not
    // pick up the override just because the material matches.
    let other_ref = join::LayerRef {
        material_id: "Plaster".to_string(),
        role_tag: None,
        index: 0,
        layer_id: Some(other_id),
    };
    let other_result = resolve_layer_style_override(Some(&rules), &[], &other_ref);
    assert_eq!(other_result.line_type, None);
}

/// Step 4b: a `JunctionOverride.layer_pairs` entry referencing a layer by
/// `layer_id` must stay correctly matched (and therefore survive
/// `validate_junction_override`'s pruning pass) after an uninvolved
/// layer is deleted, even though that shifts the referenced layer's
/// `index`.
#[test]
fn junction_override_layer_pair_survives_deletion_of_uninvolved_layer() {
    let concrete_id = uuid::Uuid::new_v4();

    // The override was captured while the wall still had an Insulation
    // layer at index 0 and Concrete at index 1.
    let override_data = join::JunctionOverride {
        default_style: None,
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 1,
                layer_id: Some(concrete_id),
            },
            layer_b: None,
            style: join::JoinOverrideStyle::NoExtend,
        }],
        layer_gaps: Vec::new(),
    };

    // The uninvolved Insulation layer was deleted, so Concrete is now
    // the only (index 0) layer, keeping the same `layer_id`.
    let self_layers = vec![join::LayerRef {
        material_id: "Concrete".to_string(),
        role_tag: None,
        index: 0,
        layer_id: Some(concrete_id),
    }];
    let other_layers: Vec<join::LayerRef> = vec![];

    let (validated, removed) = validate_junction_override(&override_data, &self_layers, &other_layers);
    assert_eq!(removed, 0, "the layer_id match must keep the pair despite the index shift");
    assert_eq!(
        validated,
        Some(join::JunctionOverride {
            default_style: None,
            layer_pairs: vec![join::LayerPairOverride {
                layer_a: join::LayerRef {
                    material_id: "Concrete".to_string(),
                    role_tag: None,
                    index: 1,
                    layer_id: Some(concrete_id),
                },
                layer_b: None,
                style: join::JoinOverrideStyle::NoExtend,
            }],
            layer_gaps: Vec::new(),
        })
    );
}

/// Step 3: when wall A's material changes such that a stored
/// `LayerPairOverride.layer_a` no longer matches any current layer, that
/// pair must be pruned on the next regeneration while a still-valid
/// `default_style` on the same override survives, and regeneration must
/// still succeed (falling back to automatic resolution for that layer).
#[test]
fn stale_layer_pair_override_is_pruned_but_default_style_kept() {
    let mut scene = Scene::new();
    let wall_a = add_multi_layer_wall(&mut scene);

    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = vec![wl("Concrete", 0.3, "Structural")];
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    regenerate_wall_representation(&mut scene, wall_a, None).expect("wall A regen");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("wall B regen");

    let override_data = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::Miter),
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            },
            layer_b: None,
            style: join::JoinOverrideStyle::NoExtend,
        }],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, wall_a, 1, &override_data));

    // Structural change: wall A's "Concrete" layer becomes "Brick" — the
    // stored override's `layer_a` no longer matches anything on wall A.
    write_wall_layers(
        &mut scene,
        wall_a,
        wls(&[("Brick", 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]),
    );

    let _ = take_pending_override_warnings(); // clear anything queued so far
    let (kind, _touched) = join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None)
        .expect("regeneration must succeed via automatic fallback");
    assert_eq!(kind, JoinKind::L);

    let cleaned = read_junction_override(&scene, wall_a, 1)
        .expect("default_style should survive the cleanup");
    assert_eq!(cleaned.default_style, Some(join::JoinOverrideStyle::Miter));
    assert!(
        cleaned.layer_pairs.is_empty(),
        "the stale Concrete layer pair should have been pruned, got {:?}",
        cleaned.layer_pairs
    );
}

/// Step 3: a layer removed entirely from a wall's style invalidates any
/// override referencing it — same cleanup, no crash, and the wall's
/// automatic-resolution footprint is still produced.
#[test]
fn override_referencing_removed_layer_is_cleaned_up_without_crash() {
    let mut scene = Scene::new();
    let wall_a = add_multi_layer_wall(&mut scene);

    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = vec![wl("Concrete", 0.3, "Structural")];
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    regenerate_wall_representation(&mut scene, wall_a, None).expect("wall A regen");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("wall B regen");

    let override_data = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::Butt),
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Insulation".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            },
            layer_b: None,
            style: join::JoinOverrideStyle::OuterFace,
        }],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, wall_a, 1, &override_data));

    // Remove the Insulation layer entirely from wall A's style.
    write_wall_layers(&mut scene, wall_a, vec![wl("Concrete", 0.2, "Structural")]);

    let (kind, _touched) = join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None)
        .expect("regeneration must not fail even though a referenced layer is gone");
    assert_eq!(kind, JoinKind::L);

    let cleaned = read_junction_override(&scene, wall_a, 1)
        .expect("default_style should survive the cleanup");
    assert!(cleaned.layer_pairs.is_empty());

    let wall_a_v2 = wall_from_entity(scene.document.get_entity(wall_a).unwrap())
        .expect("wall A should still read back as WALL");
    assert!(
        !wall_a_v2.derived_handles.is_empty(),
        "wall A should still have a fallback footprint after cleanup"
    );
}

/// Step 3: an override with only an (invalidated) `layer_pairs` entry and
/// no `default_style` must have its XDATA tag fully erased once cleanup
/// leaves nothing meaningful behind.
#[test]
fn fully_invalid_override_removes_xdata_tag_entirely() {
    let mut scene = Scene::new();
    let wall_a = add_multi_layer_wall(&mut scene);

    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = vec![wl("Concrete", 0.3, "Structural")];
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    regenerate_wall_representation(&mut scene, wall_a, None).expect("wall A regen");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("wall B regen");

    let override_data = join::JunctionOverride {
        default_style: None,
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            },
            layer_b: None,
            style: join::JoinOverrideStyle::NoExtend,
        }],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, wall_a, 1, &override_data));

    write_wall_layers(
        &mut scene,
        wall_a,
        wls(&[("Brick", 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]),
    );

    join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None)
        .expect("regeneration must succeed via automatic fallback");

    assert_eq!(
        read_junction_override(&scene, wall_a, 1),
        None,
        "the degenerate override should be erased entirely, not left as an empty record"
    );
}

/// Step 3: when a wall at an N-way junction is deleted, another wall's
/// override that referenced one of the deleted wall's layers as
/// `layer_b` must not cause a panic on the next regeneration of the
/// remaining walls — it is cleaned up gracefully instead.
#[test]
fn deleted_wall_at_junction_does_not_panic_remaining_override() {
    fn add_wall(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64)) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }
    let mut scene = Scene::new();
    // Three walls meeting at the origin (X-ish junction).
    let w1 = add_wall(&mut scene, (0.0, 0.0), (-10.0, 0.0));
    let w2 = add_wall(&mut scene, (0.0, 0.0), (0.0, 10.0));
    let w3 = add_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));

    for h in [w1, w2, w3] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }
    join_junction_in_document(&mut scene, &[w1, w2, w3], None, None, None, None).expect("initial N-way join");

    // W1 stores an override whose `layer_b` names W3's Concrete layer.
    let override_data = join::JunctionOverride {
        default_style: None,
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            },
            layer_b: Some(join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            }),
            style: join::JoinOverrideStyle::Butt,
        }],
        layer_gaps: Vec::new(),
    };
    let end_1 = if get_wall_vertices(&scene, w1)[0].distance(DVec3::ZERO) < 1e-6 {
        0
    } else {
        1
    };
    assert!(write_junction_override(&mut scene, w1, end_1, &override_data));

    // Delete W3 entirely from the document.
    scene.document.remove_entity(w3);

    // Re-resolving the junction with only the remaining walls must not
    // panic, even though W1's override still references the deleted
    // wall's layer.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        join_junction_in_document(&mut scene, &[w1, w2], None, None, None, None)
    }));
    assert!(result.is_ok(), "regeneration must not panic after a peer wall was deleted");
}

/// Step 3 regression: a still-valid override (referenced layer/material
/// unchanged) must not be touched by the invalidation pass.
#[test]
fn valid_override_is_not_touched_by_invalidation() {
    let mut scene = Scene::new();
    let wall_a = add_multi_layer_wall(&mut scene);

    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = vec![wl("Concrete", 0.3, "Structural")];
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    regenerate_wall_representation(&mut scene, wall_a, None).expect("wall A regen");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("wall B regen");

    let override_data = join::JunctionOverride {
        default_style: None,
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            },
            layer_b: None,
            style: join::JoinOverrideStyle::NoExtend,
        }],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, wall_a, 1, &override_data));

    let _ = take_pending_override_warnings();
    join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None).expect("join should succeed");

    assert_eq!(
        read_junction_override(&scene, wall_a, 1),
        Some(override_data),
        "a still-valid override must survive regeneration unchanged"
    );
    assert!(
        take_pending_override_warnings().is_empty(),
        "no invalidation notice should be queued for a valid override"
    );
}

/// Step 3: the user-visible notice mechanism (`take_pending_override_warnings`,
/// drained via `command_line.push_info` at command entry points) must
/// actually be invoked when an override is invalidated and removed.
#[test]
fn invalidated_override_queues_and_surfaces_a_notice() {
    let mut scene = Scene::new();
    let wall_a = add_multi_layer_wall(&mut scene);

    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = vec![wl("Concrete", 0.3, "Structural")];
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    regenerate_wall_representation(&mut scene, wall_a, None).expect("wall A regen");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("wall B regen");

    let override_data = join::JunctionOverride {
        default_style: None,
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            },
            layer_b: None,
            style: join::JoinOverrideStyle::NoExtend,
        }],
        layer_gaps: Vec::new(),
    };
    assert!(write_junction_override(&mut scene, wall_a, 1, &override_data));
    write_wall_layers(
        &mut scene,
        wall_a,
        wls(&[("Brick", 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]),
    );

    let _ = take_pending_override_warnings(); // drain any leftovers from prior tests
    let mut command_line = CommandLine::default();
    aec_walljoin_do(
        &mut scene, 
        &mut command_line, 
        &format!("{}|{}", wall_a.value(), wall_b.value()), 
    None,  None,  None);

    assert!(
        command_line
            .history
            .iter()
            .any(|e| e.text.contains("join override") || e.text.contains("Verbindungsüberschreibung")),
        "the command line should surface an invalidation notice, got {:?}",
        command_line.history.iter().map(|e| &e.text).collect::<Vec<_>>()
    );
}

#[test]
fn regenerate_wall_representation_builds_layers_and_avoids_duplication() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    let before = scene.document.entities().count();

    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a valid two-layer wall");
    let after_first = scene.document.entities().count();
    assert!(
        after_first > before,
        "regeneration should have created new contour/hatch/solid entities"
    );

    let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("wall should still be readable as WALL after regeneration");
    let derived_after_first = wall.derived_handles.clone();
    assert!(!derived_after_first.is_empty());

    // Calling it again must replace, not accumulate, the derived
    // entities.
    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("second regeneration should also succeed");
    let after_second = scene.document.entities().count();
    assert_eq!(
        after_first, after_second,
        "regenerating twice should not duplicate derived entities"
    );

    let wall2 = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("wall should still be readable as WALL after second regeneration");
    assert_eq!(wall2.derived_handles.len(), derived_after_first.len());
}

#[test]
fn regenerate_wall_representation_respects_layer_override() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut entity = EntityType::LwPolyline(pl);
    let mut overridden = wl("Concrete", 0.2, "Structural");
    overridden.layer_override = Some("AEC_OVERRIDE_LAYER".to_string());
    let default_layer = wl("Insulation", 0.05, "Insulation");
    let layers = vec![overridden, default_layer];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let wall_handle = scene.add_entity(entity);

    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed for a valid two-layer wall");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .expect("should still read back as WALL")
        .derived_handles;
    assert!(!derived.is_empty());

    let mut saw_override_layer = false;
    let mut saw_default_layer = false;
    for h in derived {
        let e = scene.document.get_entity(h).unwrap();
        if e.common().layer == "AEC_OVERRIDE_LAYER" {
            saw_override_layer = true;
        } else if e.common().layer == "0" || !e.common().layer.is_empty() {
            saw_default_layer = true;
        }
    }
    assert!(
        saw_override_layer,
        "at least one derived entity of the overridden layer should be placed on AEC_OVERRIDE_LAYER"
    );
    assert!(
        saw_default_layer,
        "the non-overridden layer's derived entities should keep using the default layer"
    );
    assert!(scene.document.layers.contains("AEC_OVERRIDE_LAYER"));
}

#[test]
fn regenerate_wall_representation_with_rules_none_matches_default_behavior() {
    use crate::modules::aec::engine::display_component::ComponentRuleSet;

    let mut scene_plain = Scene::new();
    let wall_plain = add_multi_layer_wall(&mut scene_plain);
    regenerate_wall_representation(&mut scene_plain, wall_plain, None)
        .expect("plain regeneration should succeed");
    let derived_plain = wall_from_entity(scene_plain.document.get_entity(wall_plain).unwrap())
        .unwrap()
        .derived_handles;

    let mut scene_none = Scene::new();
    let wall_none = add_multi_layer_wall(&mut scene_none);
    regenerate_wall_representation_with_rules(&mut scene_none, wall_none, None, None)
        .expect("rules-aware regeneration with None should succeed");
    let derived_none = wall_from_entity(scene_none.document.get_entity(wall_none).unwrap())
        .unwrap()
        .derived_handles;
    assert_eq!(derived_plain.len(), derived_none.len());

    let mut scene_default = Scene::new();
    let wall_default = add_multi_layer_wall(&mut scene_default);
    let default_rules = ComponentRuleSet::default();
    regenerate_wall_representation_with_rules(
        &mut scene_default,
        wall_default,
        Some(&default_rules),
    None)
    .expect("rules-aware regeneration with default (all-visible) rules should succeed");
    let derived_default =
        wall_from_entity(scene_default.document.get_entity(wall_default).unwrap())
            .unwrap()
            .derived_handles;
    assert_eq!(
        derived_plain.len(),
        derived_default.len(),
        "a default ComponentRuleSet (everything visible) must reproduce today's behavior"
    );
}

#[test]
fn regenerate_wall_representation_with_rules_hides_solid3d_slot() {
    use crate::modules::aec::engine::display_component::{ComponentRuleSet, WallComponentSlot};

    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    let mut rules = ComponentRuleSet::default();
    rules
        .visibility
        .insert(WallComponentSlot::Solid3D.key().to_string(), false);

    regenerate_wall_representation_with_rules(&mut scene, wall_handle, Some(&rules), None)
        .expect("regeneration with Solid3D hidden should still succeed");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .unwrap()
        .derived_handles;
    assert!(!derived.is_empty());

    let mut saw_contour = false;
    let mut saw_hatch = false;
    let mut saw_solid = false;
    for h in &derived {
        match scene.document.get_entity(*h) {
            Some(EntityType::LwPolyline(_)) => saw_contour = true,
            Some(EntityType::Hatch(_)) => saw_hatch = true,
            Some(EntityType::Solid3D(_)) => saw_solid = true,
            _ => {}
        }
    }
    assert!(saw_contour, "contours should remain when only Solid3D is hidden");
    assert!(saw_hatch, "hatches should remain when only Solid3D is hidden");
    assert!(!saw_solid, "no Solid3D entity should be created when the slot is hidden");
}

#[test]
fn regenerate_wall_representation_with_rules_hides_layers2d_slot() {
    use crate::modules::aec::engine::display_component::{ComponentRuleSet, WallComponentSlot};

    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    let mut rules = ComponentRuleSet::default();
    rules
        .visibility
        .insert(WallComponentSlot::Layers2D.key().to_string(), false);
    rules
        .visibility
        .insert(WallComponentSlot::Contour2D.key().to_string(), false);

    regenerate_wall_representation_with_rules(&mut scene, wall_handle, Some(&rules), None)
        .expect("regeneration with 2D contour slots hidden should still succeed");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .unwrap()
        .derived_handles;
    assert!(!derived.is_empty());

    let mut saw_contour = false;
    let mut saw_solid = false;
    for h in &derived {
        match scene.document.get_entity(*h) {
            Some(EntityType::LwPolyline(_)) => saw_contour = true,
            Some(EntityType::Solid3D(_)) => saw_solid = true,
            _ => {}
        }
    }
    assert!(
        !saw_contour,
        "no contour polyline should be created when Layers2D and Contour2D are hidden"
    );
    assert!(saw_solid, "solids should remain when only 2D contour slots are hidden");
}

/// Serializes access to the on-disk AEC style library file (see
/// `engine::library::default_library_path`) for tests that need
/// `load_or_seed()` inside `regenerate_wall_representation_inner` to see
/// specific wall styles/materials (Step 3 `StyleSubstitution` tests):
/// writes `lib`, runs `f`, then restores whatever was on disk before.
fn with_test_library<F: FnOnce()>(lib: &crate::modules::aec::engine::library::StyleLibrary, f: F) {
    static LIBRARY_TEST_LOCK: Mutex<()> = Mutex::new(());
    let _guard = LIBRARY_TEST_LOCK.lock().unwrap();
    let path = crate::modules::aec::engine::library::default_library_path();
    let backup = std::fs::read_to_string(&path).ok();
    crate::modules::aec::engine::library::save_to_default_path(lib)
        .expect("failed to write test library");
    f();
    match backup {
        Some(content) => {
            let _ = std::fs::write(&path, content);
        }
        None => {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[test]
fn component_rule_set_style_override_wins_for_hatch_slot() {
    use crate::modules::aec::engine::display_component::{
        ComponentRuleSet, ComponentStyleOverride, WallComponentSlot,
    };

    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    let mut rules = ComponentRuleSet::default();
    rules.style_override.insert(
        WallComponentSlot::ContourHatch2D.key().to_string(),
        ComponentStyleOverride {
            hatch_pattern: Some("NET".to_string()),
            hatch_color: Some(acadrust::types::Color::Rgb { r: 0, g: 255, b: 0 }),
            ..Default::default()
        },
    );

    regenerate_wall_representation_with_rules(&mut scene, wall_handle, Some(&rules), None)
        .expect("regeneration with a ContourHatch2D style_override should succeed");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .unwrap()
        .derived_handles;

    let mut saw_overridden_pattern = false;
    let mut saw_overridden_color = false;
    for h in &derived {
        if let Some(EntityType::Hatch(hatch)) = scene.document.get_entity(*h) {
            if hatch.pattern.name == "NET" {
                saw_overridden_pattern = true;
            }
            // The default fallback pattern must never appear once the
            // slot-wide override is active.
            assert_ne!(hatch.pattern.name, "ANSI31");
            if hatch.common.color
                == (acadrust::types::Color::Rgb {
                    r: 0,
                    g: 255,
                    b: 0,
                })
            {
                saw_overridden_color = true;
            }
        }
    }
    assert!(
        saw_overridden_pattern,
        "every layer's hatch should use the ContourHatch2D style_override pattern"
    );
    assert!(
        saw_overridden_color,
        "hatch entities must keep the override hatch_color on the DXF entity"
    );
}

#[test]
fn layer_selection_explicit_filters_contour_and_solid_to_referenced_layers() {
    use crate::modules::aec::engine::display_component::{
        ComponentRuleSet, LayerSelection, WallComponentSlot,
    };
    use crate::modules::aec::engine::join::LayerRef;

    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    // add_multi_layer_wall's layers are ["Concrete" (index 0), "Insulation" (index 1)].
    let mut rules = ComponentRuleSet::default();
    let explicit = LayerSelection::Explicit(vec![LayerRef {
        material_id: "Concrete".to_string(),
        role_tag: None,
        index: 0,
    layer_id: None,
    }]);
    rules
        .layer_filter
        .insert(WallComponentSlot::Contour2D.key().to_string(), explicit.clone());
    rules
        .layer_filter
        .insert(WallComponentSlot::Solid3D.key().to_string(), explicit);

    regenerate_wall_representation_with_rules(&mut scene, wall_handle, Some(&rules), None)
        .expect("regeneration with an explicit layer_filter should succeed");

    let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
        .unwrap()
        .derived_handles;

    let mut contour_count = 0usize;
    let mut solid_count = 0usize;
    let mut hatch_count = 0usize;
    for h in &derived {
        match scene.document.get_entity(*h) {
            Some(EntityType::LwPolyline(_)) => contour_count += 1,
            Some(EntityType::Solid3D(_)) => solid_count += 1,
            Some(EntityType::Hatch(_)) => hatch_count += 1,
            _ => {}
        }
    }
    assert_eq!(
        contour_count, 1,
        "only the explicitly referenced layer's contour should be created"
    );
    assert_eq!(
        solid_count, 1,
        "only the explicitly referenced layer's solid should be created"
    );
    // `layer_filter` doesn't gate hatches (plan Step 3 scope is
    // Contour2D/Solid3D only) — both layers' hatches remain.
    assert_eq!(
        hatch_count, 2,
        "layer_filter must not affect hatch creation, only Contour2D/Solid3D"
    );
}

#[test]
fn style_substitution_swaps_hatch_look_but_keeps_axis_and_thickness() {
    use crate::modules::aec::engine::library::StyleLibrary;
    use crate::modules::aec::engine::material::Material;

    let source_material = Material::new(
        "SourceMat".to_string(),
        "Source".to_string(),
        "ANSI31".to_string(),
        0x111111,
        "Continuous".to_string(),
    );
    let mut target_material = Material::new(
        "TargetMat".to_string(),
        "Target".to_string(),
        "ANSI37".to_string(),
        0x222222,
        "Continuous".to_string(),
    );
    target_material.hatch_color = Some(acadrust::types::Color::Rgb { r: 171, g: 205, b: 239 });

    let source_style = WallStyle {
        style: Style {
            id: "src-style".to_string(),
            name: "Source Style".to_string(),
            object_kind: "Wall".to_string(),
            parent_style_id: None,
        },
        layers: vec![Layer {
            material_id: "SourceMat".to_string(),
            thickness: LayerValue::Fixed(0.2),
            function: LayerFunction::Structural,
            axis_offset: LayerValue::Fixed(0.0),
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            role_tag: None,
        layer_id: uuid::Uuid::new_v4(),
        }],
    display_profiles: std::collections::HashMap::new(),
    };
    let target_style = WallStyle {
        style: Style {
            id: "tgt-style".to_string(),
            name: "Target Style".to_string(),
            object_kind: "Wall".to_string(),
            parent_style_id: None,
        },
        // Same total thickness as `source_style` (0.2), satisfying the
        // `validate_style_substitution` consistency requirement.
        layers: vec![Layer {
            material_id: "TargetMat".to_string(),
            thickness: LayerValue::Fixed(0.2),
            function: LayerFunction::Structural,
            axis_offset: LayerValue::Fixed(0.0),
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            role_tag: None,
        layer_id: uuid::Uuid::new_v4(),
        }],
    display_profiles: std::collections::HashMap::new(),
    };
    assert!(crate::modules::aec::engine::display_component::validate_style_substitution(
        &source_style,
        &target_style
    )
    .is_ok());

    let lib = StyleLibrary {
        materials: vec![source_material, target_material],
        wall_styles: vec![source_style, target_style],
        ..Default::default()
    };

    with_test_library(&lib, || {
        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("SourceMat", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values =
            wall_record("src-style", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        let wall_handle = scene.add_entity(entity);

        let axis_before = get_wall_vertices(&scene, wall_handle);

        let mut substitutions: HashMap<String, String> = HashMap::new();
        substitutions.insert("src-style".to_string(), "tgt-style".to_string());

        regenerate_wall_representation_with_rules_and_substitutions(
            &mut scene,
            wall_handle,
            None,
            Some(&substitutions),
            Some(&lib))
        .expect("regeneration with a style substitution should succeed");

        let axis_after = get_wall_vertices(&scene, wall_handle);
        assert_eq!(axis_before, axis_after, "axis geometry must stay unchanged");
        let wall_after = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("still a WALL");
        assert!((wall_after.layers[0].thickness - 0.2).abs() < 1e-9);

        let mut saw_target_pattern = false;
        for h in &wall_after.derived_handles {
            if let Some(EntityType::Hatch(hatch)) = scene.document.get_entity(*h) {
                if hatch.pattern.name == "ANSI37" {
                    saw_target_pattern = true;
                }
                assert_ne!(
                    hatch.pattern.name, "ANSI31",
                    "the substituted wall must not use the source style's hatch pattern"
                );
            }
        }
        assert!(
            saw_target_pattern,
            "the substituted wall should use the target wall style's hatch pattern"
        );
    });
}

#[test]
fn detailed_style_override_wins_over_style_substitution() {
    use crate::modules::aec::engine::display_component::{
        ComponentRuleSet, ComponentStyleOverride, WallComponentSlot,
    };
    use crate::modules::aec::engine::library::StyleLibrary;
    use crate::modules::aec::engine::material::Material;

    let source_material = Material::new(
        "SourceMat2".to_string(),
        "Source".to_string(),
        "ANSI31".to_string(),
        0x111111,
        "Continuous".to_string(),
    );
    let target_material = Material::new(
        "TargetMat2".to_string(),
        "Target".to_string(),
        "ANSI37".to_string(),
        0x222222,
        "Continuous".to_string(),
    );
    let source_style = WallStyle {
        style: Style {
            id: "src-style-2".to_string(),
            name: "Source Style 2".to_string(),
            object_kind: "Wall".to_string(),
            parent_style_id: None,
        },
        layers: vec![Layer {
            material_id: "SourceMat2".to_string(),
            thickness: LayerValue::Fixed(0.2),
            function: LayerFunction::Structural,
            axis_offset: LayerValue::Fixed(0.0),
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            role_tag: None,
        layer_id: uuid::Uuid::new_v4(),
        }],
    display_profiles: std::collections::HashMap::new(),
    };
    let target_style = WallStyle {
        style: Style {
            id: "tgt-style-2".to_string(),
            name: "Target Style 2".to_string(),
            object_kind: "Wall".to_string(),
            parent_style_id: None,
        },
        layers: vec![Layer {
            material_id: "TargetMat2".to_string(),
            thickness: LayerValue::Fixed(0.2),
            function: LayerFunction::Structural,
            axis_offset: LayerValue::Fixed(0.0),
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            role_tag: None,
        layer_id: uuid::Uuid::new_v4(),
        }],
    display_profiles: std::collections::HashMap::new(),
    };

    let lib = StyleLibrary {
        materials: vec![source_material, target_material],
        wall_styles: vec![source_style, target_style],
        ..Default::default()
    };

    with_test_library(&lib, || {
        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("SourceMat2", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record(
            "src-style-2",
            3.0,
            0,
            &layers,
            &[],
            WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        let wall_handle = scene.add_entity(entity);

        let mut substitutions: HashMap<String, String> = HashMap::new();
        substitutions.insert("src-style-2".to_string(), "tgt-style-2".to_string());

        let mut rules = ComponentRuleSet::default();
        rules.style_override.insert(
            WallComponentSlot::ContourHatch2D.key().to_string(),
            ComponentStyleOverride {
                hatch_pattern: Some("NET".to_string()),
                ..Default::default()
            },
        );

        regenerate_wall_representation_with_rules_and_substitutions(
            &mut scene,
            wall_handle,
            Some(&rules),
            Some(&substitutions),
        None)
        .expect("regeneration with both a Detailed override and an applicable substitution should succeed");

        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .unwrap()
            .derived_handles;

        let mut saw_detailed_pattern = false;
        for h in &derived {
            if let Some(EntityType::Hatch(hatch)) = scene.document.get_entity(*h) {
                assert_eq!(
                    hatch.pattern.name, "NET",
                    "the Detailed style_override must win over the style substitution's target pattern"
                );
                saw_detailed_pattern = true;
            }
        }
        assert!(saw_detailed_pattern, "a hatch should have been created");
    });
}

#[test]
fn regenerate_wall_representation_with_rules_hides_slot_at_joined_corner() {
    use crate::modules::aec::engine::display_component::{ComponentRuleSet, WallComponentSlot};

    // Two joined walls sharing a corner via `regenerate_wall_representation_with_corner_and_rules`,
    // confirming the Solid3D override still applies at a mitered/extended corner.
    let mut scene = Scene::new();
    let wall_a = add_multi_layer_wall(&mut scene);
    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = wls(&[("Concrete", 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]);
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    let mut rules = ComponentRuleSet::default();
    rules
        .visibility
        .insert(WallComponentSlot::Solid3D.key().to_string(), false);

    // Regenerate wall_a with a corner-extension override toward wall_b's
    // start vertex, same shape as the plain join path uses, but with the
    // Solid3D slot hidden.
    regenerate_wall_representation_with_corner_and_rules(
        &mut scene,
        wall_a,
        Some((1, DVec3::new(5.0, 0.0, 0.0))),
        None,
        Some(&rules),
    None)
    .expect("joined-corner regeneration with Solid3D hidden should still succeed");

    let derived_a = wall_from_entity(scene.document.get_entity(wall_a).unwrap())
        .unwrap()
        .derived_handles;
    assert!(!derived_a.is_empty());
    let mut saw_contour = false;
    let mut saw_solid = false;
    for h in &derived_a {
        match scene.document.get_entity(*h) {
            Some(EntityType::LwPolyline(_)) => saw_contour = true,
            Some(EntityType::Solid3D(_)) => saw_solid = true,
            _ => {}
        }
    }
    assert!(saw_contour, "contours should still be produced at the joined corner");
    assert!(!saw_solid, "Solid3D should stay hidden at the joined corner too");

    let _ = wall_b; // kept alive to represent the join partner
}

#[test]
fn wall_axis_still_detected_by_room_loop_after_regeneration() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    let corners = [
        DVec3::new(0.0, 0.0, 0.0),
        DVec3::new(4.0, 0.0, 0.0),
        DVec3::new(4.0, 3.0, 0.0),
        DVec3::new(0.0, 3.0, 0.0),
        DVec3::new(0.0, 0.0, 0.0),
    ];

    let mut handles = Vec::new();
    for pair in corners.windows(2) {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(pair[0].x, pair[0].y)));
        pl.add_vertex(LwVertex::new(Vector2::new(pair[1].x, pair[1].y)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 2.8, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        handles.push(scene.add_entity(entity));
    }

    // Regenerate every wall: this moves each axis polyline onto the
    // dedicated AEC_WALL_AXIS layer.
    for h in &handles {
        regenerate_wall_representation(&mut scene, *h, None)
            .expect("regeneration should succeed for every wall segment");
    }
    for h in &handles {
        let e = scene.document.get_entity(*h).unwrap();
        assert_eq!(e.common().layer, AEC_WALL_AXIS_LAYER);
    }

    // AEC_ROOM / collect_wall_segments must still see the closed loop.
    let segments = collect_wall_segments(&scene.document);
    assert_eq!(segments.len(), 4);

    let mut command_line = CommandLine::default();
    aec_room(&mut scene, &mut command_line);
    let room_record = scene
        .document
        .entities()
        .filter_map(read_aec_record)
        .find(|r| matches!(r.values.first(), Some(XDataValue::String(k)) if k == "ROOM"))
        .expect("aec_room should have written a ROOM xdata record");
    let area = match room_record.values.get(2) {
        Some(XDataValue::Real(a)) => *a,
        _ => panic!("ROOM record should carry an area value"),
    };
    assert!((area - 12.0).abs() < 1e-6);
}

#[test]
fn wall_layer_extrusions_calculates_offsets_and_effective_height() {
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let entity = EntityType::LwPolyline(pl);
    let layers = vec![WallLayer {
        material: "brick".to_string(),
        thickness: 0.2,
        function: "Structural".to_string(),
        axis_offset: -0.1,
        bottom_offset: 0.5,
        top_offset: 0.3,
        layer_override: None,
        hatch_override: None,
    layer_id: uuid::Uuid::new_v4(),
    }];
    let height = 3.0;
    let extrusions = wall_layer_extrusions(&entity, &layers, height);
    assert_eq!(extrusions.len(), 1);
    assert!((extrusions[0].height - 2.2).abs() < 1e-9); // 3.0 - 0.5 - 0.3 = 2.2
    assert!((extrusions[0].base_offset - 0.5).abs() < 1e-9);
}

#[test]
fn regenerate_wall_representation_respects_vertical_offsets() {
    use acadrust::types::Vector2;
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut entity = EntityType::LwPolyline(pl);
    let mut layer = wl("Concrete", 0.2, "Structural");
    layer.bottom_offset = 0.5;
    layer.top_offset = 0.3;
    let layers = vec![layer];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let wall_handle = scene.add_entity(entity);

    regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed");

    let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();

    let mut saw_bottom_z = false;
    let mut saw_top_z = false;
    for h in wall.derived_handles {
        let e = scene.document.get_entity(h).unwrap();
        if let EntityType::Solid3D(s3d) = e {
            for wire in &s3d.wires {
                for pt in &wire.points {
                    if (pt.z - 0.5).abs() < 1e-9 {
                        saw_bottom_z = true;
                    }
                    if (pt.z - 2.7).abs() < 1e-9 {
                        saw_top_z = true;
                    }
                }
            }
        }
    }
    assert!(saw_bottom_z, "solid should have wires at Z=0.5");
    assert!(saw_top_z, "solid should have wires at Z=2.7");
}

#[test]
fn wall_regeneration_reflects_updated_layers() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut entity = EntityType::LwPolyline(pl);

    let initial_layers = vec![wl("Brick", 0.1, "Structural")];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values =
        wall_record("style1", 3.0, 0, &initial_layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let wall_handle = scene.add_entity(entity);

    // Initial regeneration
    regenerate_wall_representation(&mut scene, wall_handle, None).unwrap();

    // Update layers
    let updated_layers = vec![wl("Brick", 0.5, "Structural")];
    assert!(write_wall_layers(&mut scene, wall_handle, updated_layers));

    // Regenerate again
    regenerate_wall_representation(&mut scene, wall_handle, None).unwrap();

    let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();
    assert_eq!(wall.layers[0].thickness, 0.5);

    // Check geometry (thickness is reflected in Solid3D width)
    let mut max_y = 0.0;
    for h in wall.derived_handles {
        let e = scene.document.get_entity(h).unwrap();
        if let EntityType::Solid3D(s3d) = e {
            for wire in &s3d.wires {
                for pt in &wire.points {
                    if pt.y.abs() > max_y {
                        max_y = pt.y.abs();
                    }
                }
            }
        }
    }
    // For Center justification, half of 0.5 should be at Y=0.25 and Y=-0.25
    assert!((max_y - 0.25).abs() < 1e-6);
}

#[test]
fn aec_wallextend_do_extends_to_an_explicit_point() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene); // axis: (0,0) -> (5,0)
    let mut command_line = CommandLine::default();

    aec_wallextend_do(
        &mut scene, 
        &mut command_line, 
        &format!("{}|PT|8|0|0", wall_handle.value()), 
    None,  None,  None);

    let axis = get_wall_vertices(&scene, wall_handle);
    assert_eq!(axis.len(), 2);
    // The nearer endpoint (5,0) should have moved to the target (8,0);
    // the far endpoint (0,0) stays put.
    assert!(axis.iter().any(|p| (p.x - 8.0).abs() < 1e-9 && p.y.abs() < 1e-9));
    assert!(axis.iter().any(|p| p.x.abs() < 1e-9 && p.y.abs() < 1e-9));

    // Both the 2D (contour/hatch) and 3D (solid) derived representation
    // must reflect the new, extended axis length — not just the axis
    // polyline itself.
    let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();
    let mut max_x_2d: f64 = 0.0;
    let mut max_x_3d: f64 = 0.0;
    for h in &wall.derived_handles {
        match scene.document.get_entity(*h).unwrap() {
            EntityType::LwPolyline(pl) => {
                for v in &pl.vertices {
                    max_x_2d = max_x_2d.max(v.location.x);
                }
            }
            EntityType::Solid3D(s3d) => {
                for wire in &s3d.wires {
                    for pt in &wire.points {
                        max_x_3d = max_x_3d.max(pt.x as f64);
                    }
                }
            }
            _ => {}
        }
    }
    assert!(
        max_x_2d > 7.9,
        "2D contour should extend to the new endpoint, got max_x={max_x_2d}"
    );
    assert!(
        max_x_3d > 7.9,
        "3D solid should extend to the new endpoint, got max_x={max_x_3d}"
    );
}

#[test]
fn aec_wallextend_do_preserves_direction_for_an_off_line_target() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene); // axis: (0,0) -> (5,0)
    let mut command_line = CommandLine::default();

    // Target point is NOT collinear with the wall's (0,0)->(5,0) axis
    // (it has a non-zero Y). Extending must not bend the wall towards
    // this point — it must project onto the original direction line.
    aec_wallextend_do(
        &mut scene, 
        &mut command_line, 
        &format!("{}|PT|8|2|0", wall_handle.value()), 
    None,  None,  None);

    let axis = get_wall_vertices(&scene, wall_handle);
    assert_eq!(axis.len(), 2);
    // Original direction was purely along +X (Y=0 for both points);
    // the new endpoint must keep the same direction, i.e. still Y=0.
    assert!(
        axis.iter().any(|p| (p.x - 8.0).abs() < 1e-9 && p.y.abs() < 1e-9),
        "extended endpoint should stay on the original direction line, got {:?}",
        axis
    );
    assert!(axis.iter().any(|p| p.x.abs() < 1e-9 && p.y.abs() < 1e-9));
}

#[test]
fn aec_wallextend_do_preserves_direction_for_a_multi_vertex_bent_wall() {
    // Regression test for a real-world root cause: for a multi-vertex
    // (bent) wall polyline, the direction to preserve when extending an
    // endpoint must come from the segment immediately adjacent to that
    // endpoint, NOT from a line drawn to the opposite far end of the
    // whole polyline (which, for a bent wall, points in a different
    // direction and would visibly change the extended segment's angle).
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    // A bent, 3-vertex wall axis: (0,0) -> (5,0) -> (5,5).
    // The last segment (5,0)->(5,5) runs purely along +Y.
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
    let mut entity = EntityType::LwPolyline(pl);
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let wall_handle = scene.add_entity(entity);

    let mut command_line = CommandLine::default();

    // Extend the (5,5) endpoint further along +Y, to (5,8).
    aec_wallextend_do(
        &mut scene, 
        &mut command_line, 
        &format!("{}|PT|5|8|0", wall_handle.value()), 
    None,  None,  None);

    let axis = get_wall_vertices(&scene, wall_handle);
    assert_eq!(axis.len(), 3);
    // The extended endpoint must stay on the last segment's direction
    // (X=5), not bend towards the far opposite end (0,0).
    let last = axis.last().unwrap();
    assert!(
        (last.x - 5.0).abs() < 1e-9 && (last.y - 8.0).abs() < 1e-9,
        "extended endpoint should stay on the adjacent segment's direction line, got {:?}",
        axis
    );
}

#[test]
fn aec_wallextend_do_extends_to_intersect_another_wall() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    // Wall A: (0,0) -> (5,0), stopping short of Wall B's axis.
    let wall_a = add_multi_layer_wall(&mut scene);
    // Wall B: a "through" wall running vertically at x=8, so extending A
    // towards it produces a T-junction trim on A only.
    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, -5.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, 5.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record(
        "style1",
        3.0,
        0,
        &vec![wl("Concrete", 0.2, "Structural")],
        &[],
        WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    let mut command_line = CommandLine::default();
    aec_wallextend_do(
        &mut scene, 
        &mut command_line, 
        &format!("{}|WALL|{}", wall_a.value(), wall_b.value()), 
    None,  None,  None);

    let axis_a = get_wall_vertices(&scene, wall_a);
    // Wall A should now end exactly at the intersection with wall B's axis.
    assert!(axis_a
        .iter()
        .any(|p| (p.x - 8.0).abs() < 1e-6 && p.y.abs() < 1e-6));
    // Wall B (the "through" wall) stays unchanged.
    let axis_b = get_wall_vertices(&scene, wall_b);
    assert_eq!(axis_b.len(), 2);
    assert!((axis_b[0].y - (-5.0)).abs() < 1e-9);
    assert!((axis_b[1].y - 5.0).abs() < 1e-9);
}

/// Regression test for the reported bug: `AEC_WALLEXTEND` to a target
/// wall visually trims/miters the extended wall, but never registered
/// the two walls as joined peers (`JOINED_PEERS` via
/// `engine::owner_index::link_peers`) — unlike `AEC_WALLJOIN` and the
/// automatic join performed while drawing. Without that peer link, the
/// connection isn't recognized as a real join by later operations (e.g.
/// re-resolving the junction after a subsequent move), so it appears as
/// if "no join was created".
#[test]
fn aec_wallextend_do_links_peers_with_target_wall() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    let wall_a = add_multi_layer_wall(&mut scene); // (0,0) -> (5,0)
    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, -5.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, 5.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record(
        "style1",
        3.0,
        0,
        &vec![wl("Concrete", 0.2, "Structural")],
        &[],
        WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    let mut command_line = CommandLine::default();
    aec_wallextend_do(
        &mut scene, 
        &mut command_line, 
        &format!("{}|WALL|{}", wall_a.value(), wall_b.value()), 
    None,  None,  None);

    let peers_a = engine::owner_index::peers_of(&scene.document, wall_a);
    let peers_b = engine::owner_index::peers_of(&scene.document, wall_b);
    assert_eq!(
        peers_a,
        vec![wall_b],
        "extended wall must be linked as a peer of its target wall"
    );
    assert_eq!(
        peers_b,
        vec![wall_a],
        "target wall must be linked as a peer of the extended wall"
    );
}

#[test]
fn aec_walljoin_do_forces_l_even_when_one_wall_overhangs() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let add_wall = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    };
    let through = add_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let stem = add_wall(&mut scene, (5.0, 1.0), (5.0, 4.0));
    let mut command_line = CommandLine::default();
    aec_walljoin_do(
        &mut scene, 
        &mut command_line, 
        &format!("{}|{}", through.value(), stem.value()), 
    None,  None,  None);
    let axis_t = get_wall_vertices(&scene, through);
    let axis_s = get_wall_vertices(&scene, stem);
    assert!(
        axis_t
            .iter()
            .any(|p| (p.x - 5.0).abs() < 1e-6 && p.y.abs() < 1e-6),
        "through wall must be trimmed to the L corner, got {axis_t:?}"
    );
    assert!(
        axis_s
            .iter()
            .any(|p| (p.x - 5.0).abs() < 1e-6 && p.y.abs() < 1e-6),
        "stem must reach the L corner, got {axis_s:?}"
    );
    assert!(
        !axis_t.iter().any(|p| (p.x - 10.0).abs() < 1e-6),
        "overhang past the corner must be removed"
    );
}

#[test]
fn regenerate_after_axis_shorten_matches_new_length() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene); // (0,0)-(5,0)
    let _ = regenerate_wall_representation(&mut scene, wall, None);
    update_wall_vertices(
        &mut scene,
        wall,
        &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(3.0, 0.0, 0.0)],
    );
    let _ = regenerate_wall_representation(&mut scene, wall, None);
    let rec = wall_from_entity(scene.document.get_entity(wall).unwrap()).unwrap();
    let mut max_x = 0.0_f64;
    for h in rec.derived_handles {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(h) {
            for v in &pl.vertices {
                max_x = max_x.max(v.location.x);
            }
        }
    }
    assert!(
        max_x < 3.2,
        "2D contour must follow the shortened axis, got max_x={max_x}"
    );
    assert!(max_x > 2.8, "2D contour should still reach the new end");
}

#[test]
fn regenerate_rewrites_existing_contour_polyline() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    let _ = regenerate_wall_representation(&mut scene, wall, None);
    let rec = wall_from_entity(scene.document.get_entity(wall).unwrap()).unwrap();
    let contour_h = rec
        .derived_handles
        .iter()
        .copied()
        .find(|&h| matches!(scene.document.get_entity(h), Some(EntityType::LwPolyline(_))))
        .expect("contour");

    update_wall_vertices(
        &mut scene,
        wall,
        &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(4.0, 0.0, 0.0)],
    );
    let _ = regenerate_wall_representation(&mut scene, wall, None);

    let rec = wall_from_entity(scene.document.get_entity(wall).unwrap()).unwrap();
    assert!(
        rec.derived_handles.contains(&contour_h),
        "contour handle must be reused so tessellation can retessellate it"
    );
    let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(contour_h) else {
        panic!("contour still a polyline");
    };
    let max_x = pl
        .vertices
        .iter()
        .map(|v| v.location.x)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (max_x - 4.0).abs() < 0.3,
        "reused contour must follow new axis, max_x={max_x}"
    );
}

#[test]
fn erase_wall_live_preview_companions_drops_untagged_draw_contour() {
    // Bug B / Step 3: WallCommand commits an untagged outer-contour
    // polyline for live preview. On finish it must be erased so only
    // WALL_REP children remain (which regenerate with the axis).
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);

    let mut preview = LwPolyline::new();
    preview.is_closed = true;
    preview.add_vertex(LwVertex::new(Vector2::new(0.0, -0.1)));
    preview.add_vertex(LwVertex::new(Vector2::new(10.0, -0.1)));
    preview.add_vertex(LwVertex::new(Vector2::new(10.0, 0.1)));
    preview.add_vertex(LwVertex::new(Vector2::new(0.0, 0.1)));
    let preview_h = scene.add_entity(EntityType::LwPolyline(preview));

    // A properly tagged WALL_REP child must NOT be erased by the helper.
    let mut tagged = LwPolyline::new();
    tagged.is_closed = true;
    tagged.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    tagged.add_vertex(LwVertex::new(Vector2::new(1.0, 0.0)));
    tagged.add_vertex(LwVertex::new(Vector2::new(1.0, 1.0)));
    let tagged_h = scene.add_entity(EntityType::LwPolyline(tagged));
    write_wall_display_tag(&mut scene, tagged_h, wall, WALL_REP_ROLE_CONTOUR);

    erase_wall_live_preview_companions(&mut scene, wall, &[preview_h, tagged_h]);

    assert!(
        scene.document.get_entity(preview_h).is_none(),
        "untagged live preview contour must be erased on wall finish"
    );
    assert!(
        scene.document.get_entity(tagged_h).is_some(),
        "WALL_REP children must not be erased by preview cleanup"
    );
    assert!(
        scene.document.get_entity(wall).is_some(),
        "wall axis must remain"
    );
}

#[test]
fn regenerate_erases_orphan_wall_rep_contour() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    let _ = regenerate_wall_representation(&mut scene, wall, None);

    let mut orphan = LwPolyline::new();
    orphan.is_closed = true;
    orphan.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    orphan.add_vertex(LwVertex::new(Vector2::new(9.0, 0.0)));
    orphan.add_vertex(LwVertex::new(Vector2::new(9.0, 1.0)));
    orphan.add_vertex(LwVertex::new(Vector2::new(0.0, 1.0)));
    let orphan_h = scene.add_entity(EntityType::LwPolyline(orphan));
    write_wall_display_tag(&mut scene, orphan_h, wall, WALL_REP_ROLE_CONTOUR);

    update_wall_vertices(
        &mut scene,
        wall,
        &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(2.0, 0.0, 0.0)],
    );
    let _ = regenerate_wall_representation(&mut scene, wall, None);

    // Orphan handle may be reused as the new contour; it must not keep
    // the old 9-unit rectangle either way.
    if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(orphan_h) {
        let max_x = pl
            .vertices
            .iter()
            .map(|v| v.location.x)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            max_x < 2.3,
            "reused leftover contour must follow shortened axis, max_x={max_x}"
        );
    }
    let rec = wall_from_entity(scene.document.get_entity(wall).unwrap()).unwrap();
    let mut max_x = 0.0_f64;
    for h in rec.derived_handles {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(h) {
            for v in &pl.vertices {
                max_x = max_x.max(v.location.x);
            }
        }
    }
    assert!(max_x < 2.3, "new contour must follow shortened axis, max_x={max_x}");
}

#[test]
fn change_wall_justification_shifts_axis_by_expected_distance() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene); // total thickness 0.25
    let axis_before = get_wall_vertices(&scene, wall_handle);
    assert!(axis_before.iter().all(|p| p.y.abs() < 1e-9));

    assert!(change_wall_justification(
        &mut scene,
        wall_handle,
        WallJustification::Interior,
    None));

    let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();
    assert_eq!(wall.justification, WallJustification::Interior);

    let axis_after = get_wall_vertices(&scene, wall_handle);
    // Center -> Interior delta is -0.5 * total_thickness = -0.125; the
    // offset direction for a straight horizontal axis is +Y, so the axis
    // should have shifted to Y = -0.125.
    for p in &axis_after {
        assert!((p.y - (-0.125)).abs() < 1e-6);
    }
}

#[test]
fn wall_extend_command_auto_detects_target_wall_on_entity_pick() {
    // Once a source wall is selected, clicking another wall (without
    // typing W) must dispatch the WALL|target branch.
    let mut cmd = WallExtendCommand::new();
    assert!(cmd.needs_entity_pick());

    let source = Handle::new(10);
    let target = Handle::new(20);
    assert!(matches!(
        cmd.on_entity_pick(source, DVec3::ZERO),
        CmdResult::NeedPoint
    ));
    assert!(cmd.needs_entity_pick());

    match cmd.on_entity_pick(target, DVec3::new(8.0, 0.0, 0.0)) {
        CmdResult::Dispatch(s) => {
            assert_eq!(s, format!("AEC_WALLEXTEND_DO {}|WALL|{}", source.value(), target.value()));
        }
        _ => panic!("expected WALL| dispatch"),
    }
}

#[test]
fn wall_extend_command_empty_click_stays_in_to_wall_mode() {
    // A null-handle pick (empty space) after the source wall is selected
    // must NOT fall back to a point extend in the default `ToWall`
    // mode — the user must pick a different wall or switch to `Point`
    // mode explicitly.
    let mut cmd = WallExtendCommand::new();
    let source = Handle::new(10);
    let _ = cmd.on_entity_pick(source, DVec3::ZERO);

    assert!(matches!(
        cmd.on_entity_pick(Handle::NULL, DVec3::new(8.0, 2.0, 0.0)),
        CmdResult::NeedPoint
    ));
}

#[test]
fn wall_extend_command_point_mode_dispatches_pt_after_switch() {
    // Typing `P` switches to `ToPoint` mode; a subsequent point pick
    // must dispatch the PT| point-projection path.
    let mut cmd = WallExtendCommand::new();
    let source = Handle::new(10);
    let _ = cmd.on_entity_pick(source, DVec3::ZERO);
    assert!(matches!(cmd.on_text_input("P"), Some(CmdResult::NeedPoint)));
    assert!(!cmd.needs_entity_pick());

    match cmd.on_point(DVec3::new(8.0, 2.0, 0.0)) {
        CmdResult::Dispatch(s) => {
            assert!(
                s.starts_with(&format!("AEC_WALLEXTEND_DO {}|PT|", source.value())),
                "expected PT| dispatch, got {s}"
            );
        }
        _ => panic!("expected PT| dispatch after switching to Point mode"),
    }
}

#[test]
fn regenerate_wall_representation_returns_axis_and_derived_handles() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);

    let touched = regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regeneration should succeed");

    assert!(
        touched.contains(&wall_handle),
        "returned set must include the axis handle"
    );
    let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();
    assert!(
        !wall.derived_handles.is_empty(),
        "regeneration should create derived entities"
    );
    for h in &wall.derived_handles {
        assert!(
            touched.contains(h),
            "returned set must include derived handle {}",
            h.value()
        );
    }
    // Axis + every derived.
    assert_eq!(touched.len(), 1 + wall.derived_handles.len());
}

/// Regression for the Properties-panel/vertex-edit staleness bug: after
/// moving a wall's axis vertex and regenerating its representation, the
/// *resident* (GPU-facing) wire set — the one `invalidate_property_targets`
/// feeds via `bump_entities` — must reflect the new contour geometry, not
/// a leftover outline from before the edit. `refresh_wall_after_axis_edit`
/// forces this via `scene.bump_geometry()`; any other axis-edit caller must
/// reach the same end state.
#[test]
fn wall_axis_edit_updates_resident_contour_wires() {
    use crate::scene::view::camera::Camera;

    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall_handle, None).expect("initial regen");

    // Prime the resident (camera-independent, GPU-facing) wire cache at
    // the original axis position.
    let cam = Camera::default();
    let _ = scene.model_tile_wires_arc(0, &cam, 1.0, 1.0);

    // Simulate a Properties-panel vertex edit: move the wall's endpoint
    // far away, regenerate, then invalidate exactly like
    // `invalidate_property_targets` does today — only `bump_entities` on
    // the touched handles, no `bump_geometry()`.
    let new_vertices = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(500.0, 0.0, 0.0)];
    update_wall_vertices(&mut scene, wall_handle, &new_vertices);
    let touched = regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("regen after vertex edit");
    let changes: Vec<_> = touched
        .iter()
        .map(|&h| (h, crate::scene::ChangeKind::Modified))
        .collect();
    scene.bump_entities(&changes);

    // The resident wire set must no longer contain any wire endpoint at
    // the old axis extent (x == 5.0); every wall-derived wire must reach
    // out to the new extent (x == 500.0).
    let wires = scene.model_tile_wires_arc(0, &cam, 1.0, 1.0);
    let mut saw_new_extent = false;
    for wire in wires.iter() {
        let Some(handle) = Scene::handle_from_wire_name(&wire.name) else {
            continue;
        };
        if !touched.contains(&handle) {
            continue;
        }
        for pt in &wire.points {
            if pt[0].is_nan() {
                // Tombstone slot from the resident-wire splice; not real
                // geometry.
                continue;
            }
            assert!(
                (pt[0] - 5.0).abs() > 1e-6,
                "resident wire for handle {} still shows the pre-edit contour \
                 at x=5.0 (stale tessellation); point={:?}",
                handle.value(),
                pt
            );
            if (pt[0] - 500.0).abs() < 1e-6 {
                saw_new_extent = true;
            }
        }
    }
    assert!(
        saw_new_extent,
        "resident wire set never reached the new axis extent (x=500.0); \
         contour/hatch did not visibly follow the moved wall point"
    );
}

/// Regression for the STRETCH bug: the wall axis lives on the invisible
/// `AEC_WALL_AXIS` layer, so a crossing-window stretch only ever sees the
/// visible contour handle. Moving that contour's own vertices in place
/// (the naive/buggy approach) is immediately reverted by the next
/// `refresh_wall_after_axis_edit` regeneration, because it rebuilds the
/// contour from the *unchanged* axis. Only moving the axis itself, then
/// regenerating, actually relocates the visible wall — this is exactly
/// the fix applied to `CmdResult::StretchEntities` in
/// `command_driver.rs`.
#[test]
fn stretching_only_the_contour_is_reverted_by_regen_but_stretching_the_axis_sticks() {
    let mut scene = Scene::new();
    let wall_handle = add_multi_layer_wall(&mut scene);
    let touched = regenerate_wall_representation(&mut scene, wall_handle, None)
        .expect("initial regen");

    let contour_handle = *touched
        .iter()
        .find(|&&h| {
            h != wall_handle
                && matches!(
                    scene.document.get_entity(h),
                    Some(EntityType::LwPolyline(pl)) if pl.is_closed
                )
        })
        .expect("wall must have produced a closed contour");

    // --- Buggy path: mutate only the visible contour's vertices in
    // place (what STRETCH's generic LwPolyline branch used to do for a
    // wall's contour handle before the fix), then regenerate.
    let mut contour_before = match scene.document.get_entity(contour_handle) {
        Some(EntityType::LwPolyline(pl)) => pl.clone(),
        _ => panic!("expected contour LwPolyline"),
    };
    for v in &mut contour_before.vertices {
        v.location.x += 495.0;
    }
    update_wall_vertices(&mut scene, contour_handle, &[]); // no-op guard; contour isn't the axis
    if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity_mut(contour_handle) {
        *pl = contour_before;
    }
    let reverted = refresh_wall_after_axis_edit(&mut scene,  wall_handle,  None,  None,  None);
    let still_short = get_wall_vertices(&scene, wall_handle)
        .iter()
        .all(|v| v.x < 400.0);
    assert!(
        still_short,
        "regenerating from the untouched axis must revert a contour-only stretch \
         (this is the bug being fixed): axis vertices = {:?}",
        get_wall_vertices(&scene, wall_handle)
    );
    let contour_after_revert = reverted
        .iter()
        .find(|&&h| {
            matches!(
                scene.document.get_entity(h),
                Some(EntityType::LwPolyline(pl)) if pl.is_closed
            )
        })
        .and_then(|&h| match scene.document.get_entity(h) {
            Some(EntityType::LwPolyline(pl)) => Some(pl.clone()),
            _ => None,
        })
        .expect("regenerated contour");
    assert!(
        contour_after_revert
            .vertices
            .iter()
            .all(|v| v.location.x < 400.0),
        "contour-only stretch must not survive regeneration: {:?}",
        contour_after_revert.vertices
    );

    // --- Correct (fixed) path: `stretch_wall_axis_in_window` — the
    // exact helper `CmdResult::StretchEntities` now calls for wall
    // packages — moves the axis itself, then regenerates.
    let touched_after_fix = stretch_wall_axis_in_window(
        &mut scene, 
        wall_handle, 
        |x,  _y| x < 400.0, 
        DVec3::new(495.0, 0.0, 0.0), 
        None, 
        None,
        None)
    .expect("axis vertex fell inside the window; must return Some(touched)");
    let axis_moved = get_wall_vertices(&scene, wall_handle)
        .iter()
        .any(|v| v.x > 400.0);
    assert!(
        axis_moved,
        "moving the axis vertices must stick after regeneration"
    );
    let contour_moved = touched_after_fix
        .iter()
        .filter_map(|&h| match scene.document.get_entity(h) {
            Some(EntityType::LwPolyline(pl)) if pl.is_closed => Some(pl.clone()),
            _ => None,
        })
        .any(|pl| pl.vertices.iter().any(|v| v.location.x > 400.0));
    assert!(
        contour_moved,
        "the regenerated contour must reach the new axis extent after the fix"
    );

    // A window that covers none of the axis vertices must be a no-op.
    assert!(
        stretch_wall_axis_in_window(
            &mut scene, 
            wall_handle, 
            |_x,  _y| false, 
            DVec3::new(1.0, 0.0, 0.0), 
            None, 
            None,
            None)
        .is_none(),
        "a window matching no axis vertex must not move or regenerate the wall"
    );
}

#[test]
fn join_l_corner_layer_footprints_share_miter_boundary() {
    let mut scene = Scene::new();
    // Single-layer walls so both sides fully match for miter.
    let mut pl_a = LwPolyline::new();
    pl_a.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl_a.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut ent_a = EntityType::LwPolyline(pl_a);
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut rec_a = ExtendedDataRecord::new(AEC_APPID);
    rec_a.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent_a.common_mut().extended_data.add_record(rec_a);
    let wall_a = scene.add_entity(ent_a);

    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
    let mut ent_b = EntityType::LwPolyline(pl_b);
    let mut rec_b = ExtendedDataRecord::new(AEC_APPID);
    rec_b.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent_b.common_mut().extended_data.add_record(rec_b);
    let wall_b = scene.add_entity(ent_b);

    let (kind, touched) = join_two_walls_in_document(&mut scene,  wall_a,  wall_b,  None,  None,  None).expect("L join");
    assert_eq!(kind, JoinKind::L);
    assert!(touched.contains(&wall_a) && touched.contains(&wall_b));

    // Persisted axes still meet exactly at the corner.
    let axis_a = get_wall_vertices(&scene, wall_a);
    let axis_b = get_wall_vertices(&scene, wall_b);
    assert_eq!(*axis_a.last().unwrap(), DVec3::new(6.0, 0.0, 0.0));
    assert_eq!(*axis_b.first().unwrap(), DVec3::new(6.0, 0.0, 0.0));

    // Collect closed contour polylines (layer footprints) for both walls.
    let contours = |scene: &Scene, h: Handle| -> Vec<Vec<(f64, f64)>> {
        let wall = wall_from_entity(scene.document.get_entity(h).unwrap()).unwrap();
        wall.derived_handles
            .iter()
            .filter_map(|dh| match scene.document.get_entity(*dh) {
                Some(EntityType::LwPolyline(pl)) => Some(
                    pl.vertices
                        .iter()
                        .map(|v| (v.location.x, v.location.y))
                        .collect(),
                ),
                _ => None,
            })
            .collect()
    };
    let fps_a = contours(&scene, wall_a);
    let fps_b = contours(&scene, wall_b);
    assert!(!fps_a.is_empty() && !fps_b.is_empty());

    // Expected shared miter corners for equal 0.2 walls at (6,0):
    // (6.1, -0.1) and (5.9, 0.1).
    let c1 = (6.1, -0.1);
    let c2 = (5.9, 0.1);
    let has = |fp: &[(f64, f64)], p: (f64, f64)| {
        fp.iter()
            .any(|(x, y)| (*x - p.0).abs() < 1e-6 && (*y - p.1).abs() < 1e-6)
    };
    assert!(
        fps_a.iter().any(|fp| has(fp, c1) && has(fp, c2)),
        "wall A footprint should contain both miter corners, got {fps_a:?}"
    );
    assert!(
        fps_b.iter().any(|fp| has(fp, c1) && has(fp, c2)),
        "wall B footprint should share the same miter corners (no gap), got {fps_b:?}"
    );
}

#[test]
fn join_t_corner_stem_footprint_reaches_through_wall_face() {
    let mut scene = Scene::new();
    // Stem A approaching through wall B from above.
    let mut pl_a = LwPolyline::new();
    pl_a.add_vertex(LwVertex::new(Vector2::new(5.0, 1.0)));
    pl_a.add_vertex(LwVertex::new(Vector2::new(5.0, 10.0)));
    let mut ent_a = EntityType::LwPolyline(pl_a);
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut rec_a = ExtendedDataRecord::new(AEC_APPID);
    rec_a.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent_a.common_mut().extended_data.add_record(rec_a);
    let wall_a = scene.add_entity(ent_a);

    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut ent_b = EntityType::LwPolyline(pl_b);
    let mut rec_b = ExtendedDataRecord::new(AEC_APPID);
    rec_b.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent_b.common_mut().extended_data.add_record(rec_b);
    let wall_b = scene.add_entity(ent_b);

    let (kind, _touched) = join_two_walls_in_document(&mut scene,  wall_a,  wall_b,  None,  None,  None).expect("T join");
    assert_eq!(kind, JoinKind::T);

    let axis_a = get_wall_vertices(&scene, wall_a);
    assert!(
        axis_a
            .iter()
            .any(|p| (p.x - 5.0).abs() < 1e-6 && p.y.abs() < 1e-6),
        "stem axis should end on the through wall axis, got {axis_a:?}"
    );

    let wall_a_v2 = wall_from_entity(scene.document.get_entity(wall_a).unwrap()).unwrap();
    let mut max_abs_y_near_join = 0.0_f64;
    for h in &wall_a_v2.derived_handles {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*h) {
            for v in &pl.vertices {
                if (v.location.x - 5.0).abs() < 0.15 {
                    max_abs_y_near_join = max_abs_y_near_join.max(v.location.y.abs());
                }
            }
        }
    }
    assert!(
        max_abs_y_near_join > 0.05,
        "stem footprint should reach the through wall's layer face, got max |y|={max_abs_y_near_join}"
    );
}

#[test]
fn join_t_does_not_shorten_through_axis() {
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let add = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    };
    let through = add(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let stem = add(&mut scene, (4.0, 1.0), (4.0, 8.0));
    let (kind, _) = join_two_walls_in_document(&mut scene,  stem,  through,  None,  None,  None).expect("T");
    assert_eq!(kind, JoinKind::T);
    let through_axis = get_wall_vertices(&scene, through);
    assert_eq!(through_axis[0], DVec3::new(0.0, 0.0, 0.0));
    assert_eq!(through_axis[1], DVec3::new(10.0, 0.0, 0.0));
    let stem_axis = get_wall_vertices(&scene, stem);
    assert!(
        stem_axis[0].distance(DVec3::new(4.0, 0.0, 0.0)) < 1e-6,
        "stem should end on the through axis, got {stem_axis:?}"
    );
}

#[test]
fn join_junction_resolves_two_wall_l_and_t() {
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let add = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    };
    let a = add(&mut scene, (0.0, 0.0), (5.0, 0.0));
    let b = add(&mut scene, (5.0, 0.0), (5.0, 5.0));
    let touched = join_junction_in_document(&mut scene,  &[a, b],  None,  None,  None,  None).expect("L junction");
    assert!(touched.contains(&a) && touched.contains(&b));
    let axis_a = get_wall_vertices(&scene, a);
    let axis_b = get_wall_vertices(&scene, b);
    assert_eq!(*axis_a.last().unwrap(), DVec3::new(5.0, 0.0, 0.0));
    assert_eq!(*axis_b.first().unwrap(), DVec3::new(5.0, 0.0, 0.0));

    let through = add(&mut scene, (0.0, 10.0), (10.0, 10.0));
    let stem = add(&mut scene, (3.0, 10.0), (3.0, 15.0));
    join_junction_in_document(&mut scene,  &[through, stem],  None,  None,  None,  None).expect("T junction");
    let through_axis = get_wall_vertices(&scene, through);
    assert_eq!(through_axis[0], DVec3::new(0.0, 10.0, 0.0));
    assert_eq!(through_axis[1], DVec3::new(10.0, 10.0, 0.0));
}

#[test]
fn join_junction_honors_display_rules_on_both_walls() {
    use engine::display_component::{ComponentRuleSet, WallComponentSlot};
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let add = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    };
    let a = add(&mut scene, (0.0, 0.0), (5.0, 0.0));
    let b = add(&mut scene, (5.0, 0.0), (5.0, 5.0));
    let mut rules = ComponentRuleSet::default();
    rules
        .visibility
        .insert(WallComponentSlot::Solid3D.key().to_string(), false);
    join_junction_in_document(&mut scene, &[a, b], None, None, Some(&rules), None)
        .expect("L junction");
    for h in [a, b] {
        let derived = wall_from_entity(scene.document.get_entity(h).unwrap())
            .expect("WALL")
            .derived_handles;
        let solids = derived
            .iter()
            .filter(|handle| matches!(scene.document.get_entity(**handle), Some(EntityType::Solid3D(_))))
            .count();
        assert_eq!(
            solids, 0,
            "join regen must honor the active plan (no 3D solids), wall {h:?}"
        );
        assert!(
            !derived.is_empty(),
            "2D representation should still be created"
        );
    }
}

#[test]
fn refresh_after_axis_edit_honors_display_rules_on_joined_walls() {
    use engine::display_component::{ComponentRuleSet, WallComponentSlot};
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let add = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    };
    let a = add(&mut scene, (0.0, 0.0), (5.0, 0.0));
    let b = add(&mut scene, (5.0, 0.0), (5.0, 5.0));
    let mut rules = ComponentRuleSet::default();
    rules
        .visibility
        .insert(WallComponentSlot::Solid3D.key().to_string(), false);
    join_junction_in_document(&mut scene, &[a, b], None, None, Some(&rules), None)
        .expect("L junction");
    let mut axis = get_wall_vertices(&scene, a);
    axis[0] = DVec3::new(-0.2, 0.0, 0.0);
    update_wall_vertices(&mut scene, a, &axis);
    refresh_wall_after_axis_edit(&mut scene, a, None, Some(&rules), None);
    for h in [a, b] {
        let derived = wall_from_entity(scene.document.get_entity(h).unwrap())
            .expect("WALL")
            .derived_handles;
        let solids = derived
            .iter()
            .filter(|handle| matches!(scene.document.get_entity(**handle), Some(EntityType::Solid3D(_))))
            .count();
        assert_eq!(
            solids, 0,
            "axis-edit regen must honor the active plan (no 3D solids), wall {h:?}"
        );
    }
}

#[test]
fn try_auto_join_after_grip_keeps_t_through_axis() {
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let add = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    };
    let through = add(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let stem = add(&mut scene, (4.0, 0.0), (4.0, 6.0));
    join_two_walls_in_document(&mut scene,  stem,  through,  None,  None,  None).expect("T");

    let mut stem_axis = get_wall_vertices(&scene, stem);
    stem_axis[0] = DVec3::new(4.05, 0.1, 0.0);
    update_wall_vertices(&mut scene, stem, &stem_axis);
    let _ = try_auto_join_nearby_walls(&mut scene,  stem,  None,  None,  None);

    let through_axis = get_wall_vertices(&scene, through);
    assert_eq!(through_axis[0], DVec3::new(0.0, 0.0, 0.0));
    assert_eq!(through_axis[1], DVec3::new(10.0, 0.0, 0.0));
    let stem_after = get_wall_vertices(&scene, stem);
    assert!(
        stem_after[0].distance(DVec3::new(4.05, 0.0, 0.0)) < 1e-6
            || stem_after[0].distance(DVec3::new(4.0, 0.0, 0.0)) < 0.2,
        "stem should re-join the through axis, got {stem_after:?}"
    );
}

#[test]
fn aec_wallextend_to_target_wall_matches_join_wall_axes_intersection() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    // Wall A: (0,0)->(5,0); wall B vertical at x=8.
    let wall_a = add_multi_layer_wall(&mut scene);
    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, -5.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, 5.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record(
        "style1",
        3.0,
        0,
        &vec![wl("Concrete", 0.2, "Structural")],
        &[],
        WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    let axis_a_before = get_wall_vertices(&scene, wall_a);
    let axis_b = get_wall_vertices(&scene, wall_b);
    let (expected_a, _, _, _, _) =
        join::join_wall_axes(&axis_a_before, &axis_b).expect("axes should intersect");

    // Simulate the interactive path: pick source wall, then pick target
    // wall (auto-detect, no W keystroke).
    let mut cmd = WallExtendCommand::new();
    assert!(matches!(
        cmd.on_entity_pick(wall_a, DVec3::new(2.0, 0.0, 0.0)),
        CmdResult::NeedPoint
    ));
    let dispatch = match cmd.on_entity_pick(wall_b, DVec3::new(8.0, 0.0, 0.0)) {
        CmdResult::Dispatch(s) => s,
        _ => panic!("expected WALL| dispatch"),
    };
    let args = dispatch
        .strip_prefix("AEC_WALLEXTEND_DO ")
        .expect("dispatch prefix");
    let mut command_line = CommandLine::default();
    aec_wallextend_do(&mut scene,  &mut command_line,  args,  None,  None,  None);

    let axis_a_after = get_wall_vertices(&scene, wall_a);
    assert_eq!(
        axis_a_after.len(),
        expected_a.len(),
        "axis vertex count should match join_wall_axes"
    );
    for (got, exp) in axis_a_after.iter().zip(expected_a.iter()) {
        assert!(
            got.distance(*exp) < 1e-9,
            "extended endpoint must match join_wall_axes intersection, got {got:?} expected {exp:?}"
        );
    }
    // Must NOT be the raw click point projected onto the wall direction
    // in a way that ignores B — the intersection is at x=8.
    assert!(axis_a_after
        .iter()
        .any(|p| (p.x - 8.0).abs() < 1e-9 && p.y.abs() < 1e-9));
    // Full join rebuilds both walls' representations (miter/T stem). For
    // this T configuration the through-wall axis vertices stay put, but
    // the stem must still land on x=8 and both walls keep derived handles.
    let wall_a_v2 = wall_from_entity(scene.document.get_entity(wall_a).unwrap()).unwrap();
    let wall_b_v2 = wall_from_entity(scene.document.get_entity(wall_b).unwrap()).unwrap();
    assert!(
        !wall_a_v2.derived_handles.is_empty() && !wall_b_v2.derived_handles.is_empty(),
        "both walls should have regenerated derived handles after extend-join"
    );
}

#[test]
fn find_wall_to_auto_join_picks_nearby_joinable_wall() {
    let mut scene = Scene::new();
    // Existing wall along X from (0,0) to (5,0).
    let existing = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, existing, None).expect("regen existing");

    // New wall ending 0.15 m short of existing's end — within snap radius.
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(5.15, 3.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.15, 0.15)));
    let mut entity = EntityType::LwPolyline(pl);
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let new_wall = scene.add_entity(entity);

    let found = find_wall_to_auto_join(&scene, new_wall, &[]);
    assert_eq!(
        found,
        Some(existing),
        "should find the nearby existing wall within WALL_JOIN_SNAP_RADIUS"
    );

    // Far wall: no candidate.
    let mut pl_far = LwPolyline::new();
    pl_far.add_vertex(LwVertex::new(Vector2::new(50.0, 0.0)));
    pl_far.add_vertex(LwVertex::new(Vector2::new(55.0, 0.0)));
    let mut ent_far = EntityType::LwPolyline(pl_far);
    let mut rec_far = ExtendedDataRecord::new(AEC_APPID);
    rec_far.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent_far.common_mut().extended_data.add_record(rec_far);
    let far = scene.add_entity(ent_far);
    assert!(
        find_wall_to_auto_join(&scene, far, &[]).is_none(),
        "far wall must not auto-join"
    );

    // Excluding the only candidate yields None.
    assert!(find_wall_to_auto_join(&scene, new_wall, &[existing]).is_none());
}

#[test]
fn find_wall_to_auto_join_prefers_clear_endpoint_match_over_closer_t_match() {
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];

    // Candidate L: horizontal wall (0,0)->(5,0), same as
    // `add_multi_layer_wall`. Its endpoint at (5,0) is ~0.18 away from
    // the new wall's lower endpoint below — a clear End-End match.
    let corner = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, corner, None).expect("regen corner");

    // Candidate T: a long horizontal wall running underneath, whose
    // *interior* (not an endpoint) is only ~0.1 away from the new wall's
    // lower endpoint — nominally closer, but a vaguer End-Mid match.
    let mut pl_through = LwPolyline::new();
    pl_through.add_vertex(LwVertex::new(Vector2::new(-5.0, 0.2)));
    pl_through.add_vertex(LwVertex::new(Vector2::new(15.0, 0.2)));
    let mut ent_through = EntityType::LwPolyline(pl_through);
    let mut rec_through = ExtendedDataRecord::new(AEC_APPID);
    rec_through.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent_through.common_mut().extended_data.add_record(rec_through);
    let through = scene.add_entity(ent_through);
    regenerate_wall_representation(&mut scene, through, None).expect("regen through");

    // New wall: vertical, ending at (5.15, 0.1) — closer in raw distance
    // to `through`'s interior (~0.1) than to `corner`'s endpoint (~0.18).
    let mut pl_new = LwPolyline::new();
    pl_new.add_vertex(LwVertex::new(Vector2::new(5.15, 3.0)));
    pl_new.add_vertex(LwVertex::new(Vector2::new(5.15, 0.1)));
    let mut entity = EntityType::LwPolyline(pl_new);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let new_wall = scene.add_entity(entity);

    let found = find_wall_to_auto_join(&scene, new_wall, &[]);
    assert_eq!(
        found,
        Some(corner),
        "a clear endpoint match should win over a nominally closer T-interior match"
    );
}

#[test]
fn find_wall_to_auto_join_never_returns_the_wall_itself() {
    let mut scene = Scene::new();
    let existing = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, existing, None).expect("regen existing");
    // Even without excluding it explicitly, the candidate loop skips
    // `wall_handle == other` unconditionally, so a wall can never
    // auto-join to itself while still being drawn/edited.
    assert_eq!(find_wall_to_auto_join(&scene, existing, &[]), None);
}

#[test]
fn try_auto_join_nearby_walls_joins_axes_and_returns_touched_handles() {
    let mut scene = Scene::new();
    // Existing: (0,0)->(5,0). New wall approaches an L corner near (5,0).
    let existing = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, existing, None).expect("regen existing");

    let mut pl = LwPolyline::new();
    // End slightly short of the true intersection (5,0) — within snap radius.
    pl.add_vertex(LwVertex::new(Vector2::new(5.1, 4.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.1, 0.1)));
    let mut entity = EntityType::LwPolyline(pl);
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let new_wall = scene.add_entity(entity);
    regenerate_wall_representation(&mut scene, new_wall, None).expect("regen new");

    let touched = try_auto_join_nearby_walls(&mut scene,  new_wall,  None,  None,  None);
    assert!(
        !touched.is_empty(),
        "auto-join should touch both walls' packages"
    );
    assert!(
        touched.contains(&new_wall) && touched.contains(&existing),
        "touched set must include both wall axes, got {touched:?}"
    );

    // Axes must meet at the true intersection.
    let axis_new = get_wall_vertices(&scene, new_wall);
    let axis_ex = get_wall_vertices(&scene, existing);
    let meet = DVec3::new(5.1, 0.0, 0.0); // vertical at x=5.1 meets horizontal y=0
    // join_wall_axes extends the horizontal wall end and moves the vertical end.
    assert!(
        axis_new
            .iter()
            .any(|p| p.distance(meet) < 1e-6)
            || axis_ex.iter().any(|p| {
                axis_new.iter().any(|q| p.distance(*q) < 1e-6)
            }),
        "after auto-join the walls should share an axis intersection; new={axis_new:?} existing={axis_ex:?}"
    );

    // Both walls should still have derived representation handles.
    for h in [new_wall, existing] {
        let v2 = wall_from_entity(scene.document.get_entity(h).unwrap()).unwrap();
        assert!(
            !v2.derived_handles.is_empty(),
            "wall {} should retain derived handles after auto-join",
            h.value()
        );
        for d in &v2.derived_handles {
            assert!(
                touched.contains(d),
                "derived handle {} must be in touched set",
                d.value()
            );
        }
    }

    let peers_new = engine::owner_index::peers_of(&scene.document, new_wall);
    let peers_ex = engine::owner_index::peers_of(&scene.document, existing);
    assert!(
        peers_new.contains(&existing),
        "auto-join must record JOINED_PEERS on the new wall, got {peers_new:?}"
    );
    assert!(
        peers_ex.contains(&new_wall),
        "auto-join must record JOINED_PEERS on the existing wall, got {peers_ex:?}"
    );
}

#[test]
fn try_auto_join_t_records_joined_peers() {
    let mut scene = Scene::new();
    let through = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
    regenerate_wall_representation(&mut scene, through, None).expect("regen through");
    let stem = add_single_layer_wall(&mut scene, (5.0, 0.05), (5.0, 4.0));
    regenerate_wall_representation(&mut scene, stem, None).expect("regen stem");
    let _ = try_auto_join_nearby_walls(&mut scene, stem, None, None, None);
    let peers = engine::owner_index::peers_of(&scene.document, stem);
    assert!(
        peers.contains(&through),
        "T auto-join must list the through wall as JOINED_PEERS, got {peers:?}"
    );
    let peers_t = engine::owner_index::peers_of(&scene.document, through);
    assert!(
        peers_t.contains(&stem),
        "T auto-join must list the stem on the through wall, got {peers_t:?}"
    );
}

#[test]
fn apply_junction_override_rebuilds_partner_wall() {
    let mut scene = Scene::new();
    let wall_a = add_multi_layer_wall(&mut scene);
    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = vec![wl("Concrete", 0.3, "Structural")];
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values = wall_record(
        "style1",
        3.0,
        0,
        &layers_b,
        &[],
        WallJustification::Center,
        PlanPhase::New,
        None,
    );
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);
    regenerate_wall_representation(&mut scene, wall_a, None).expect("regen a");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("regen b");
    join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None)
        .expect("join");

    let ov = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::NoExtend),
        layer_pairs: Vec::new(),
        layer_gaps: Vec::new(),
    };
    let touched = apply_junction_override_and_rebuild(
        &mut scene,
        wall_a,
        1,
        Some(&ov),
        None,
        None,
        None,
    );
    assert!(touched.contains(&wall_a) && touched.contains(&wall_b));
    assert!(read_junction_override(&scene, wall_a, 1).is_some());
}

#[test]
fn layer_gaps_persist_on_through_wall_not_stem() {
    let mut scene = Scene::new();
    let through = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
    regenerate_wall_representation(&mut scene, through, None).expect("regen through");
    let stem = add_single_layer_wall(&mut scene, (5.0, 0.05), (5.0, 4.0));
    regenerate_wall_representation(&mut scene, stem, None).expect("regen stem");
    let _ = try_auto_join_nearby_walls(&mut scene, stem, None, None, None);
    let stem_end = walls_at_junction(&scene, stem, 0)
        .into_iter()
        .find(|p| p.axis_handle == stem)
        .map(|p| p.end_index)
        .unwrap_or(0);
    let gap = join::LayerGapOverride {
        layer: join::LayerRef {
            material_id: "Concrete".into(),
            role_tag: None,
            index: 0,
            layer_id: None,
        },
        from: join::LayerRef {
            material_id: "Concrete".into(),
            role_tag: None,
            index: 0,
            layer_id: None,
        },
        to: join::LayerRef {
            material_id: "Concrete".into(),
            role_tag: None,
            index: 0,
            layer_id: None,
        },
    };
    let ov = join::JunctionOverride {
        default_style: None,
        layer_pairs: Vec::new(),
        layer_gaps: vec![gap.clone()],
    };
    apply_junction_override_and_rebuild(&mut scene, stem, stem_end, Some(&ov), None, None, None);
    assert!(
        read_junction_override(&scene, stem, stem_end)
            .map(|o| o.layer_gaps.is_empty())
            .unwrap_or(true),
        "stem must not keep layer_gaps"
    );
    let stored = read_junction_override(&scene, through, THROUGH_SPAN_OVERRIDE_END)
        .expect("through span override");
    assert_eq!(stored.layer_gaps.len(), 1);
    assert_eq!(
        read_through_layer_gaps(&scene, stem, stem_end).len(),
        1
    );

    let pair = join::JunctionOverride {
        default_style: None,
        layer_pairs: vec![join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Concrete".into(),
                role_tag: None,
                index: 0,
                layer_id: None,
            },
            layer_b: Some(join::LayerRef {
                material_id: "Concrete".into(),
                role_tag: None,
                index: 0,
                layer_id: None,
            }),
            style: join::JoinOverrideStyle::Miter,
        }],
        layer_gaps: Vec::new(),
    };
    apply_junction_override_and_rebuild(
        &mut scene,
        stem,
        stem_end,
        Some(&pair),
        None,
        None,
        None,
    );
    assert_eq!(
        read_through_layer_gaps(&scene, stem, stem_end).len(),
        1,
        "defining a layer pair must not erase through-wall interruptions"
    );

    let gap_b = join::LayerGapOverride {
        layer: join::LayerRef {
            material_id: "Insulation".into(),
            role_tag: None,
            index: 1,
            layer_id: None,
        },
        from: join::LayerRef {
            material_id: "Concrete".into(),
            role_tag: None,
            index: 0,
            layer_id: None,
        },
        to: join::LayerRef {
            material_id: "Concrete".into(),
            role_tag: None,
            index: 0,
            layer_id: None,
        },
    };
    let mut existing = read_through_layer_gaps(&scene, stem, stem_end);
    join::upsert_layer_gap(&mut existing, gap_b);
    let ov2 = join::JunctionOverride {
        default_style: None,
        layer_pairs: Vec::new(),
        layer_gaps: existing,
    };
    apply_junction_override_and_rebuild(
        &mut scene,
        stem,
        stem_end,
        Some(&ov2),
        None,
        None,
        None,
    );
    assert_eq!(
        read_through_layer_gaps(&scene, stem, stem_end).len(),
        2,
        "a second layer interruption must keep the first"
    );
}

#[test]
fn layer_gap_rebuild_notches_through_core() {
    let mut scene = Scene::new();
    let layers = plaster_masonry_plaster();
    let through = add_layered_wall(&mut scene, (0.0, 0.0), (10.0, 0.0), layers.clone());
    regenerate_wall_representation(&mut scene, through, None).expect("regen through");
    let stem = add_layered_wall(&mut scene, (5.0, 0.05), (5.0, 4.0), layers);
    regenerate_wall_representation(&mut scene, stem, None).expect("regen stem");
    let _ = try_auto_join_nearby_walls(&mut scene, stem, None, None, None);
    let stem_end = walls_at_junction(&scene, stem, 0)
        .into_iter()
        .find(|p| p.axis_handle == stem)
        .map(|p| p.end_index)
        .unwrap_or(0);
    let t_layers = wall_from_entity(scene.document.get_entity(through).unwrap())
        .unwrap()
        .layers;
    let s_layers = wall_from_entity(scene.document.get_entity(stem).unwrap())
        .unwrap()
        .layers;
    let layer_ref = |l: &WallLayer, index: usize| join::LayerRef {
        material_id: l.material.clone(),
        role_tag: None,
        index,
        layer_id: Some(l.layer_id),
    };
    let ov = join::JunctionOverride {
        default_style: None,
        layer_pairs: Vec::new(),
        layer_gaps: vec![join::LayerGapOverride {
            layer: layer_ref(&t_layers[1], 1),
            from: layer_ref(&s_layers[0], 0),
            to: layer_ref(&s_layers[2], 2),
        }],
    };
    apply_junction_override_and_rebuild(&mut scene, stem, stem_end, Some(&ov), None, None, None);

    let axis_2d: Vec<(f64, f64)> = get_wall_vertices(&scene, through)
        .iter()
        .map(|p| (p.x, p.y))
        .collect();
    let cut = find_through_span_cutout_footprints(&mut scene, through, &axis_2d)
        .expect("T cutout");
    assert!(
        cut.get(1).and_then(|fp| fp.as_ref()).is_some(),
        "manual gap must interrupt through masonry after rebuild, got {cut:?}"
    );

    regenerate_wall_representation(&mut scene, through, None).expect("display reapply");
    let cut_after = find_through_span_cutout_footprints(&mut scene, through, &axis_2d)
        .expect("T cutout after regen");
    assert!(
        cut_after.get(1).and_then(|fp| fp.as_ref()).is_some(),
        "masonry interruption must survive display regen, got {cut_after:?}"
    );

    let loops = wall_contour_loops(&scene, through);
    let masonry_left = loops.iter().any(|lp| {
        let min_x = lp.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let max_x = lp.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        min_x < 0.1
            && max_x < 4.95
            && lp.iter().any(|p| p.1.abs() > 0.05 && p.1.abs() < 0.121)
    });
    let masonry_right = loops.iter().any(|lp| {
        let min_x = lp.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let max_x = lp.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        min_x > 5.05
            && max_x > 9.9
            && lp.iter().any(|p| p.1.abs() > 0.05 && p.1.abs() < 0.121)
    });
    assert!(
        masonry_left && masonry_right,
        "masonry must be split around the stem, got {loops:?}"
    );
}

#[test]
fn layer_gap_on_second_t_junction_still_notches() {
    let mut scene = Scene::new();
    let layers = plaster_masonry_plaster();
    let through = add_layered_wall(&mut scene, (0.0, 0.0), (10.0, 0.0), layers.clone());
    regenerate_wall_representation(&mut scene, through, None).expect("regen through");
    let stem_a = add_layered_wall(&mut scene, (3.0, 0.05), (3.0, 4.0), layers.clone());
    regenerate_wall_representation(&mut scene, stem_a, None).expect("regen stem a");
    let _ = try_auto_join_nearby_walls(&mut scene, stem_a, None, None, None);
    let stem_b = add_layered_wall(&mut scene, (7.0, 0.05), (7.0, 4.0), layers);
    regenerate_wall_representation(&mut scene, stem_b, None).expect("regen stem b");
    let _ = try_auto_join_nearby_walls(&mut scene, stem_b, None, None, None);

    let stem_end = walls_at_junction(&scene, stem_b, 0)
        .into_iter()
        .find(|p| p.axis_handle == stem_b)
        .map(|p| p.end_index)
        .unwrap_or(0);
    let t_layers = wall_from_entity(scene.document.get_entity(through).unwrap())
        .unwrap()
        .layers;
    let s_layers = wall_from_entity(scene.document.get_entity(stem_b).unwrap())
        .unwrap()
        .layers;
    let layer_ref = |l: &WallLayer, index: usize| join::LayerRef {
        material_id: l.material.clone(),
        role_tag: None,
        index,
        layer_id: Some(l.layer_id),
    };
    let ov = join::JunctionOverride {
        default_style: None,
        layer_pairs: Vec::new(),
        layer_gaps: vec![join::LayerGapOverride {
            layer: layer_ref(&t_layers[1], 1),
            from: layer_ref(&s_layers[0], 0),
            to: layer_ref(&s_layers[2], 2),
        }],
    };
    apply_junction_override_and_rebuild(&mut scene, stem_b, stem_end, Some(&ov), None, None, None);
    regenerate_wall_representation(&mut scene, through, None).expect("display reapply");

    let loops = wall_contour_loops(&scene, through);
    let notched_at_b = loops.iter().any(|lp| {
        let min_x = lp.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let max_x = lp.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        min_x > 7.05
            && max_x > 9.9
            && lp.iter().any(|p| p.1.abs() > 0.05 && p.1.abs() < 0.121)
    });
    assert!(
        notched_at_b,
        "gap at the second T-junction must split masonry, got {loops:?}"
    );
}

fn plaster_masonry_plaster() -> Vec<WallLayer> {
    wls(&[
        ("Putz", 0.015, "Finish"),
        ("Mauerwerk", 0.24, "Structural"),
        ("Putz", 0.015, "Finish"),
    ])
}

fn plaster_masonry_insulation_plaster() -> Vec<WallLayer> {
    wls(&[
        ("Putz", 0.015, "Finish"),
        ("Mauerwerk", 0.24, "Structural"),
        ("Insulation", 0.12, "Insulation"),
        ("Putz", 0.015, "Finish"),
    ])
}

#[test]
fn join_l_identical_insulated_miters_inner_plaster() {
    let mut scene = Scene::new();
    let layers = plaster_masonry_insulation_plaster();
    let wall_a = add_layered_wall(&mut scene, (0.0, 0.0), (10.0, 0.0), layers.clone());
    let wall_b = add_layered_wall(&mut scene, (10.0, 10.0), (10.0, 0.0), layers);
    join_l_pair(&mut scene, wall_a, wall_b);

    let wall = wall_from_entity(scene.document.get_entity(wall_a).unwrap()).unwrap();
    let mut inner_pts = Vec::new();
    for h in &wall.derived_handles {
        let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*h) else {
            continue;
        };
        let ys: Vec<f64> = pl.vertices.iter().map(|v| v.location.y).collect();
        let y_min = ys.iter().copied().fold(f64::INFINITY, f64::min);
        let y_max = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if y_max < -0.05 && y_min > -0.25 {
            inner_pts.extend(pl.vertices.iter().map(|v| (v.location.x, v.location.y)));
        }
    }
    assert!(
        !inner_pts.is_empty(),
        "inner plaster contour should exist"
    );
    let square_cap = inner_pts.iter().any(|(x, y)| {
        (*x - 10.0).abs() < 1e-6 && *y < -0.05
    });
    let mitered = inner_pts.iter().any(|(x, y)| {
        *x > 10.05 && *y < -0.05
    });
    assert!(
        mitered && !square_cap,
        "inner plaster must miter past the axis end, not square-cap, pts={inner_pts:?}"
    );

    let mut room_pts = Vec::new();
    for h in &wall.derived_handles {
        let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*h) else {
            continue;
        };
        let ys: Vec<f64> = pl.vertices.iter().map(|v| v.location.y).collect();
        let y_min = ys.iter().copied().fold(f64::INFINITY, f64::min);
        let y_max = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if y_min > 0.05 && y_max < 0.25 {
            room_pts.extend(pl.vertices.iter().map(|v| (v.location.x, v.location.y)));
        }
    }
    assert!(!room_pts.is_empty(), "room-side plaster contour should exist");
    let room_square = room_pts.iter().any(|(x, y)| (*x - 10.0).abs() < 1e-6 && *y > 0.05);
    let room_miter = room_pts.iter().any(|(x, y)| *x < 9.95 && *y > 0.05);
    assert!(
        room_miter && !room_square,
        "room-side plaster at the inner L-corner must miter, pts={room_pts:?}"
    );
}

fn join_l_pair(
    scene: &mut Scene,
    a: Handle,
    b: Handle,
) {
    let lib = crate::modules::aec::engine::library::seed_default_library();
    let _ = regenerate_wall_representation(scene, a, Some(&lib));
    let _ = regenerate_wall_representation(scene, b, Some(&lib));
    let _ = join_two_walls_in_document(scene, a, b, Some(&lib), None, None);
}

fn add_example_note(scene: &mut Scene, x: f64, y: f64, value: &str) {
    use acadrust::entities::MText;
    let mut t = MText::with_value(value.to_string(), Vector3::new(x, y, 0.0));
    t.height = 0.28;
    scene.add_entity(EntityType::MText(t));
}

fn place_example_opening(
    scene: &mut Scene,
    wall: Handle,
    pt: (f64, f64),
    kind: engine::openings::OpeningKind,
    lib: &StyleLibrary,
) -> Handle {
    let (handle, _) = place_wall_opening(
        scene,
        wall,
        DVec3::new(pt.0, pt.1, 0.0),
        kind,
        Some(lib),
        None,
        None,
    )
    .expect("place example opening");
    handle
}

#[test]
fn write_aec_wall_join_examples_dxf() {
    let mut scene = Scene::new();
    let lib = crate::modules::aec::engine::library::seed_default_library();

    // L: einschalig
    let l_a = add_single_layer_wall(&mut scene, (0.0, 0.0), (5.0, 0.0));
    let l_b = add_single_layer_wall(&mut scene, (5.0, 0.0), (5.0, 5.0));
    join_l_pair(&mut scene, l_a, l_b);
    add_example_note(
        &mut scene,
        0.0,
        -1.4,
        "L einschalig\\PStil: style1 (Concrete)\\PVerbindung: L, Standard-Miter\\POverride: keiner",
    );

    // L: Putz + Mauerwerk + Putz
    let l3_a = add_layered_wall(&mut scene, (10.0, 0.0), (16.0, 0.0), plaster_masonry_plaster());
    let l3_b = add_layered_wall(&mut scene, (16.0, 0.0), (16.0, 5.0), plaster_masonry_plaster());
    join_l_pair(&mut scene, l3_a, l3_b);
    add_example_note(
        &mut scene,
        10.0,
        -1.4,
        "L dreischalig\\PStil: style1 (Putz+Mauerwerk+Putz)\\PVerbindung: L, Standard-Miter\\POverride: keiner",
    );

    // L: Putz + Mauerwerk + Dämmung + Putz
    let l4_a = add_layered_wall(
        &mut scene,
        (20.0, 0.0),
        (26.0, 0.0),
        plaster_masonry_insulation_plaster(),
    );
    let l4_b = add_layered_wall(
        &mut scene,
        (26.0, 0.0),
        (26.0, 5.0),
        plaster_masonry_insulation_plaster(),
    );
    join_l_pair(&mut scene, l4_a, l4_b);
    add_example_note(
        &mut scene,
        20.0,
        -1.4,
        "L vierschalig\\PStil: style1 (Putz+Mauerwerk+Insulation+Putz)\\PVerbindung: L, Standard-Miter\\POverride: keiner",
    );

    // T: gleiche 4-Schalen (Stamm trifft Durchgang)
    let t4_through = add_layered_wall(
        &mut scene,
        (0.0, 10.0),
        (10.0, 10.0),
        plaster_masonry_insulation_plaster(),
    );
    let t4_stem = add_layered_wall(
        &mut scene,
        (5.0, 10.05),
        (5.0, 16.0),
        plaster_masonry_insulation_plaster(),
    );
    let _ = regenerate_wall_representation(&mut scene, t4_through, Some(&lib));
    let _ = regenerate_wall_representation(&mut scene, t4_stem, Some(&lib));
    let _ = try_auto_join_nearby_walls(&mut scene, t4_stem, Some(&lib), None, None);
    add_example_note(
        &mut scene,
        0.0,
        8.3,
        "T vierschalig\\PStil: style1 (Putz+Mauerwerk+Insulation+Putz)\\PVerbindung: T (Auto-Join)\\PStamm → Durchgang, Standard-Miter\\POverride: keiner",
    );

    // T: gleiche 3-Schalen
    let t3_through = add_layered_wall(
        &mut scene,
        (14.0, 10.0),
        (24.0, 10.0),
        plaster_masonry_plaster(),
    );
    let t3_stem = add_layered_wall(
        &mut scene,
        (19.0, 10.05),
        (19.0, 16.0),
        plaster_masonry_plaster(),
    );
    let _ = regenerate_wall_representation(&mut scene, t3_through, Some(&lib));
    let _ = regenerate_wall_representation(&mut scene, t3_stem, Some(&lib));
    let _ = try_auto_join_nearby_walls(&mut scene, t3_stem, Some(&lib), None, None);
    add_example_note(
        &mut scene,
        14.0,
        8.3,
        "T dreischalig\\PStil: style1 (Putz+Mauerwerk+Putz)\\PVerbindung: T (Auto-Join)\\PStamm → Durchgang, Standard-Miter\\POverride: keiner",
    );

    // N-Wege einschalig
    let n1 = add_single_layer_wall(&mut scene, (0.0, 22.0), (8.0, 22.0));
    let n2 = add_single_layer_wall(&mut scene, (8.0, 22.0), (8.0, 28.0));
    let n3 = add_single_layer_wall(&mut scene, (8.0, 22.0), (8.0, 16.0));
    for h in [n1, n2, n3] {
        let _ = regenerate_wall_representation(&mut scene, h, Some(&lib));
    }
    let _ = join_junction_in_document(&mut scene, &[n1, n2, n3], None, Some(&lib), None, None);
    add_example_note(
        &mut scene,
        0.0,
        29.2,
        "N-Wege einschalig\\PStil: style1 (Concrete)\\PVerbindung: N-Wege (3 Wände)\\PStandard-Miter\\POverride: keiner",
    );

    // L 4-schalig mit individuellen Schichtverbindungen:
    // Putz außen/innen NoExtend, Mauerwerk Miter, Dämmung Butt.
    let ov_a = add_layered_wall(
        &mut scene,
        (14.0, 22.0),
        (22.0, 22.0),
        plaster_masonry_insulation_plaster(),
    );
    let ov_b = add_layered_wall(
        &mut scene,
        (22.0, 22.0),
        (22.0, 28.0),
        plaster_masonry_insulation_plaster(),
    );
    join_l_pair(&mut scene, ov_a, ov_b);
    let layers = wall_from_entity(scene.document.get_entity(ov_a).unwrap())
        .unwrap()
        .layers;
    let refs = layer_refs_from_materials(
        layers.iter().map(|l| (l.material.as_str(), l.layer_id)),
    );
    let ov = join::JunctionOverride {
        default_style: Some(join::JoinOverrideStyle::Miter),
        layer_pairs: vec![
            join::LayerPairOverride {
                layer_a: refs[0].clone(),
                layer_b: None,
                style: join::JoinOverrideStyle::NoExtend,
            },
            join::LayerPairOverride {
                layer_a: refs[1].clone(),
                layer_b: refs.get(1).cloned(),
                style: join::JoinOverrideStyle::Miter,
            },
            join::LayerPairOverride {
                layer_a: refs[2].clone(),
                layer_b: None,
                style: join::JoinOverrideStyle::Butt,
            },
            join::LayerPairOverride {
                layer_a: refs[3].clone(),
                layer_b: None,
                style: join::JoinOverrideStyle::NoExtend,
            },
        ],
        layer_gaps: Vec::new(),
    };
    let _ = apply_junction_override_and_rebuild(
        &mut scene, ov_a, 1, Some(&ov), Some(&lib), None, None,
    );
    add_example_note(
        &mut scene,
        14.0,
        29.2,
        "L vierschalig mit Schicht-Overrides\\PStil: style1 (Putz+Mauerwerk+Insulation+Putz)\\PVerbindung: L\\PDefault: Miter\\PPutz außen/innen: NoExtend\\PMauerwerk: Miter\\PDämmung: Butt",
    );

    // Öffnungen: eigene Zeile unter den N-Wege-Beispielen (Achse y = 34).
    let win_wall = add_single_layer_wall(&mut scene, (0.0, 34.0), (6.0, 34.0));
    let win_opening = place_example_opening(
        &mut scene,
        win_wall,
        (3.0, 34.0),
        engine::openings::OpeningKind::Window,
        &lib,
    );
    add_example_note(
        &mut scene,
        0.0,
        32.6,
        "Fenster\\PStil: style_window_standard\\P1.2 × 1.2, Brüstung 0.9",
    );

    let door_wall = add_single_layer_wall(&mut scene, (10.0, 34.0), (16.0, 34.0));
    let door_opening = place_example_opening(
        &mut scene,
        door_wall,
        (13.0, 34.0),
        engine::openings::OpeningKind::Door,
        &lib,
    );
    add_example_note(
        &mut scene,
        10.0,
        32.6,
        "Tür\\PStil: style_door_standard\\P0.9 × 2.1, Brüstung 0.0, Anschlag links",
    );

    let br_wall = add_single_layer_wall(&mut scene, (20.0, 34.0), (26.0, 34.0));
    let br_opening = place_example_opening(
        &mut scene,
        br_wall,
        (23.0, 34.0),
        engine::openings::OpeningKind::Breakthrough,
        &lib,
    );
    add_example_note(
        &mut scene,
        20.0,
        32.6,
        "Durchbruch\\PStil: style_breakthrough_standard\\P1.0 × 2.0, Brüstung 0.1\\PMark2D: Kreuz",
    );

    let placed = [
        (
            win_wall,
            win_opening,
            engine::openings::OpeningKind::Window,
        ),
        (
            door_wall,
            door_opening,
            engine::openings::OpeningKind::Door,
        ),
        (
            br_wall,
            br_opening,
            engine::openings::OpeningKind::Breakthrough,
        ),
    ];
    for (wall, opening, kind) in placed {
        let parsed = opening_from_entity(scene.document.get_entity(opening).unwrap(), opening)
            .expect("opening xdata");
        assert_eq!(parsed.kind, kind);
        assert_eq!(parsed.host_wall, wall);
        assert!(
            engine::owner_index::children_of(&scene.document, wall).contains(&opening),
            "opening {opening:?} must be a child of host wall {wall:?}"
        );
        assert!(
            !engine::opening_display::collect_opening_display_children(&scene, opening)
                .is_empty(),
            "opening {kind:?} must have OPENING_REP children"
        );
    }

    let out = std::path::Path::new("docs/examples/aec-wall-joins.dxf");
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    acadrust::DxfWriter::new(&scene.document)
        .write_to_file(out)
        .expect("write example drawing");
    assert!(out.exists());
}

#[test]
fn try_auto_join_nearby_walls_is_noop_when_nothing_nearby() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    regenerate_wall_representation(&mut scene, wall, None).expect("regen");
    let before = get_wall_vertices(&scene, wall);
    let touched = try_auto_join_nearby_walls(&mut scene,  wall,  None,  None,  None);
    assert!(touched.is_empty(), "no partner → no touched handles");
    let after = get_wall_vertices(&scene, wall);
    assert_eq!(before, after, "axis must be unchanged when auto-join is a no-op");
}

/// Collect min/max Y of every derived LwPolyline vertex for a wall package.
fn wall_derived_y_bounds(scene: &Scene, wall: Handle) -> (f64, f64) {
    let v2 = wall_from_entity(scene.document.get_entity(wall).unwrap()).unwrap();
    let mut ymin = f64::INFINITY;
    let mut ymax = f64::NEG_INFINITY;
    for h in &v2.derived_handles {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*h) {
            for v in &pl.vertices {
                ymin = ymin.min(v.location.y);
                ymax = ymax.max(v.location.y);
            }
        }
    }
    (ymin, ymax)
}

#[test]
fn reverse_wall_preserves_footprint_and_flips_layer_side_assignment() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    // Concrete 0.2 + Insulation 0.05, total 0.25 → y ∈ [-0.125, 0.125]
    regenerate_wall_representation(&mut scene, wall, None).expect("regen");

    let axis_before = get_wall_vertices(&scene, wall);
    let layers_before = wall_from_entity(scene.document.get_entity(wall).unwrap())
        .unwrap()
        .layers
        .clone();
    let (ymin_before, ymax_before) = wall_derived_y_bounds(&scene, wall);

    // Contour of first layer (Concrete) should sit on the more-negative side.
    let entity = scene.document.get_entity(wall).unwrap().clone();
    let contours_before = wall_layer_contour_polylines(&entity, &layers_before);
    let concrete_y_before = contours_before[0]
        .0
        .iter()
        .chain(contours_before[0].1.iter())
        .map(|(_, y)| *y)
        .fold(f64::INFINITY, f64::min);

    reverse_wall_in_document(&mut scene,  wall,  None,  None,  None).expect("reverse");

    let axis_after = get_wall_vertices(&scene, wall);
    assert_eq!(
        axis_after,
        axis_before.iter().rev().copied().collect::<Vec<_>>(),
        "axis vertices must reverse order"
    );

    let layers_after = wall_from_entity(scene.document.get_entity(wall).unwrap())
        .unwrap()
        .layers
        .clone();
    assert_eq!(layers_after.len(), layers_before.len());
    // The stored layer list itself is untouched by reverse; the axis
    // direction flip alone is what relocates each material.
    assert_eq!(layers_after[0].material, layers_before[0].material);
    assert_eq!(
        layers_after.last().unwrap().material,
        layers_before.last().unwrap().material
    );

    let (ymin_after, ymax_after) = wall_derived_y_bounds(&scene, wall);
    assert!(
        (ymin_before - ymin_after).abs() < 1e-6 && (ymax_before - ymax_after).abs() < 1e-6,
        "outer footprint Y bounds must stay identical: before=({ymin_before},{ymax_before}) after=({ymin_after},{ymax_after})"
    );

    // Materials must visibly swap sides: Concrete (still layers[0]) now
    // sits on the more-positive side, since the axis direction flip
    // inverted the offset normal used to place it.
    let entity_after = scene.document.get_entity(wall).unwrap().clone();
    let contours_after = wall_layer_contour_polylines(&entity_after, &layers_after);
    let concrete_y_after = contours_after[0]
        .0
        .iter()
        .chain(contours_after[0].1.iter())
        .map(|(_, y)| *y)
        .fold(f64::INFINITY, f64::min);
    assert!(
        (concrete_y_before - concrete_y_after).abs() > 1e-6,
        "Concrete must move to the opposite absolute side after reverse: before={concrete_y_before} after={concrete_y_after}"
    );
}

#[test]
fn reverse_wall_keeps_joined_corner_intersection() {
    let mut scene = Scene::new();
    // Wall A: (0,0)->(5,0). Wall B: (5,0)->(5,5) L-corner.
    let wall_a = add_multi_layer_wall(&mut scene);

    // Approach the L corner from above so join_wall_axes must extend B.
    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(5.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
    let mut entity_b = EntityType::LwPolyline(pl_b);
    let layers_b = vec![wl("Concrete", 0.2, "Structural")];
    let mut record_b = ExtendedDataRecord::new(AEC_APPID);
    record_b.values =
        wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
    entity_b.common_mut().extended_data.add_record(record_b);
    let wall_b = scene.add_entity(entity_b);

    regenerate_wall_representation(&mut scene, wall_a, None).expect("regen A");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("regen B");
    let (_kind, _) = join_two_walls_in_document(&mut scene,  wall_a,  wall_b,  None,  None,  None).expect("join");

    let axis_a_before = get_wall_vertices(&scene, wall_a);
    let axis_b_before = get_wall_vertices(&scene, wall_b);
    // Record the shared corner (any vertex of A that coincides with B).
    let corner = axis_a_before
        .iter()
        .find(|pa| axis_b_before.iter().any(|pb| pa.distance(*pb) < 1e-6))
        .copied()
        .expect("joined walls must share a corner before reverse");

    reverse_wall_in_document(&mut scene,  wall_a,  None,  None,  None).expect("reverse A");

    let axis_a = get_wall_vertices(&scene, wall_a);
    let axis_b = get_wall_vertices(&scene, wall_b);
    // After reverse + auto-join, the axes must still share an intersection
    // (at the original corner or a re-joined equivalent).
    let a_has = axis_a.iter().any(|p| p.distance(corner) < 1e-4);
    let b_has = axis_b.iter().any(|p| p.distance(corner) < 1e-4);
    let share = axis_a
        .iter()
        .any(|pa| axis_b.iter().any(|pb| pa.distance(*pb) < 1e-4));
    assert!(
        (a_has && b_has) || share,
        "joined corner must remain correct after reverse; corner={corner:?} A={axis_a:?} B={axis_b:?}"
    );
}

/// Helper: LwPolyline layer footprints for a wall package.
fn wall_closed_footprints(scene: &Scene, h: Handle) -> Vec<Vec<(f64, f64)>> {
    let wall = wall_from_entity(scene.document.get_entity(h).unwrap()).unwrap();
    wall.derived_handles
        .iter()
        .filter_map(|dh| match scene.document.get_entity(*dh) {
            Some(EntityType::LwPolyline(pl)) => Some(
                pl.vertices
                    .iter()
                    .map(|v| (v.location.x, v.location.y))
                    .collect(),
            ),
            _ => None,
        })
        .collect()
}

/// Reversing a wall that is *already* part of a stable L-join must
/// re-detect the join (axes already coincident — endpoints don't move)
/// and rebuild *both* walls' derived geometry with true miter corners,
/// not plain rectangular end-caps.
#[test]
fn reverse_already_joined_wall_rebuilds_mitered_geometry_on_both() {
    let mut scene = Scene::new();
    // Single-layer equal walls so miter matching is unambiguous.
    // A: (0,0)->(5,0). B approaches from above near (5,0).
    let mut pl_a = LwPolyline::new();
    pl_a.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl_a.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut ent_a = EntityType::LwPolyline(pl_a);
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut rec_a = ExtendedDataRecord::new(AEC_APPID);
    rec_a.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent_a.common_mut().extended_data.add_record(rec_a);
    let wall_a = scene.add_entity(ent_a);

    let mut pl_b = LwPolyline::new();
    pl_b.add_vertex(LwVertex::new(Vector2::new(5.0, 1.0)));
    pl_b.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
    let mut ent_b = EntityType::LwPolyline(pl_b);
    let mut rec_b = ExtendedDataRecord::new(AEC_APPID);
    rec_b.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent_b.common_mut().extended_data.add_record(rec_b);
    let wall_b = scene.add_entity(ent_b);

    regenerate_wall_representation(&mut scene, wall_a, None).expect("regen A");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("regen B");
    let (kind, _) = join_two_walls_in_document(&mut scene,  wall_a,  wall_b,  None,  None,  None).expect("join");
    assert_eq!(kind, JoinKind::L);

    let axis_a_joined = get_wall_vertices(&scene, wall_a);
    let axis_b_joined = get_wall_vertices(&scene, wall_b);
    let corner = axis_a_joined
        .iter()
        .find(|pa| axis_b_joined.iter().any(|pb| pa.distance(*pb) < 1e-6))
        .copied()
        .expect("joined walls share a corner");

    // Capture pre-reverse miter corners (equal 0.2 walls at corner (5,0):
    // (5.1, -0.1) and (4.9, 0.1)).
    let fps_a_before = wall_closed_footprints(&scene, wall_a);
    let fps_b_before = wall_closed_footprints(&scene, wall_b);
    let c1 = (corner.x + 0.1, corner.y - 0.1);
    let c2 = (corner.x - 0.1, corner.y + 0.1);
    let has = |fp: &[(f64, f64)], p: (f64, f64)| {
        fp.iter()
            .any(|(x, y)| (*x - p.0).abs() < 1e-6 && (*y - p.1).abs() < 1e-6)
    };
    assert!(
        fps_a_before.iter().any(|fp| has(fp, c1) && has(fp, c2)),
        "precondition: A must be mitered before reverse, got {fps_a_before:?}"
    );
    assert!(
        fps_b_before.iter().any(|fp| has(fp, c1) && has(fp, c2)),
        "precondition: B must be mitered before reverse, got {fps_b_before:?}"
    );

    // Reverse the *already-joined* wall A. This is the live-app scenario:
    // join happened earlier; reverse must re-join without relying on
    // endpoints moving.
    let touched = reverse_wall_in_document(&mut scene,  wall_a,  None,  None,  None).expect("reverse A");

    // Both packages must be in the touched set (B's derived geometry is
    // regenerated too, not only A's axis/XDATA).
    assert!(
        touched.contains(&wall_a),
        "touched must include reversed wall A"
    );
    assert!(
        touched.contains(&wall_b),
        "touched must include neighbour B after re-join, got {touched:?}"
    );
    let v2_b = wall_from_entity(scene.document.get_entity(wall_b).unwrap()).unwrap();
    for d in &v2_b.derived_handles {
        assert!(
            touched.contains(d),
            "B derived handle {} must be bumped after reverse+rejoin",
            d.value()
        );
    }

    let axis_a = get_wall_vertices(&scene, wall_a);
    let axis_b = get_wall_vertices(&scene, wall_b);
    assert!(
        axis_a.iter().any(|p| p.distance(corner) < 1e-4)
            && axis_b.iter().any(|p| p.distance(corner) < 1e-4),
        "corner must stay put after reverse; corner={corner:?} A={axis_a:?} B={axis_b:?}"
    );
    // Axis order flipped on A; the join corner vertex is now at the
    // opposite index, but its *position* is unchanged.
    assert!(
        axis_a.first().unwrap().distance(corner) < 1e-4
            || axis_a.last().unwrap().distance(corner) < 1e-4,
        "A's join endpoint still at corner after reverse, A={axis_a:?}"
    );

    let fps_a = wall_closed_footprints(&scene, wall_a);
    let fps_b = wall_closed_footprints(&scene, wall_b);
    assert!(
        fps_a.iter().any(|fp| has(fp, c1) && has(fp, c2)),
        "A must keep mitered (non-separator) corner after reverse, got {fps_a:?}"
    );
    assert!(
        fps_b.iter().any(|fp| has(fp, c1) && has(fp, c2)),
        "B must keep mitered corner after neighbour reverse, got {fps_b:?}"
    );
}

/// Absolute world-space layer center offsets must mirror (negate) for
/// an asymmetric 3-layer wall (different thicknesses and gaps) after a
/// direction reverse — the stored layer list itself is untouched, only
/// the axis-direction flip relocates each material to the opposite side.
#[test]
fn reverse_wall_preserves_three_layer_world_centers() {
    let mut scene = Scene::new();
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut entity = EntityType::LwPolyline(pl);
    // Asymmetric stack: Brick 0.1, gap 0.02, Insulation 0.05, gap 0.01, Concrete 0.2
    let mut brick = wl("Brick", 0.1, "Finish");
    brick.axis_offset = 0.0;
    let mut insulation = wl("Insulation", 0.05, "Insulation");
    insulation.axis_offset = 0.02;
    let mut concrete = wl("Concrete", 0.2, "Structural");
    concrete.axis_offset = 0.01;
    let layers = vec![brick, insulation, concrete];
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record("s3", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    entity.common_mut().extended_data.add_record(record);
    let wall = scene.add_entity(entity);

    regenerate_wall_representation(&mut scene, wall, None).expect("regen");

    let entity_before = scene.document.get_entity(wall).unwrap().clone();
    let layers_before = wall_from_entity(&entity_before).unwrap().layers.clone();
    let contours_before = wall_layer_contour_polylines(&entity_before, &layers_before);

    // Per-material world center offset along the axis normal (Y for a
    // horizontal wall at Y=0): mean of the two boundary Y values.
    let centers_before: Vec<(String, f64)> = layers_before
        .iter()
        .zip(contours_before.iter())
        .map(|(layer, (b1, b2))| {
            let y1 = b1.iter().map(|(_, y)| *y).sum::<f64>() / b1.len() as f64;
            let y2 = b2.iter().map(|(_, y)| *y).sum::<f64>() / b2.len() as f64;
            (layer.material.clone(), 0.5 * (y1 + y2))
        })
        .collect();

    reverse_wall_in_document(&mut scene,  wall,  None,  None,  None).expect("reverse");

    let entity_after = scene.document.get_entity(wall).unwrap().clone();
    let layers_after = wall_from_entity(&entity_after).unwrap().layers.clone();
    let contours_after = wall_layer_contour_polylines(&entity_after, &layers_after);
    assert_eq!(layers_after.len(), 3);
    // The stored layer list order is untouched by reverse.
    assert_eq!(layers_after[0].material, "Brick");
    assert_eq!(layers_after[1].material, "Insulation");
    assert_eq!(layers_after[2].material, "Concrete");

    let centers_after: std::collections::HashMap<String, f64> = layers_after
        .iter()
        .zip(contours_after.iter())
        .map(|(layer, (b1, b2))| {
            let y1 = b1.iter().map(|(_, y)| *y).sum::<f64>() / b1.len() as f64;
            let y2 = b2.iter().map(|(_, y)| *y).sum::<f64>() / b2.len() as f64;
            (layer.material.clone(), 0.5 * (y1 + y2))
        })
        .collect();

    for (mat, c_before) in &centers_before {
        let c_after = centers_after
            .get(mat)
            .unwrap_or_else(|| panic!("material {mat} missing after reverse"));
        // The axis-direction flip inverts the offset normal, so every
        // material's world-space center must mirror (negate) around the
        // axis line rather than stay put.
        assert!(
            (c_before + c_after).abs() < 1e-9,
            "world center of {mat} must mirror after reverse: before={c_before} after={c_after}"
        );
    }
}

#[test]
fn wall_join_hover_highlight_during_both_picks() {
    let cmd = WallJoinCommand::new();
    assert!(
        cmd.entity_pick_highlights_hover(),
        "highlight during first-wall pick"
    );
    let mut cmd = WallJoinCommand::new();
    let _ = cmd.on_entity_pick(Handle::new(1), DVec3::ZERO);
    assert!(
        cmd.entity_pick_highlights_hover(),
        "highlight while awaiting second wall"
    );
}

#[test]
fn wall_extend_hover_highlight_during_both_picks() {
    let cmd = WallExtendCommand::new();
    assert!(
        cmd.entity_pick_highlights_hover(),
        "highlight during source-wall pick"
    );
    let mut cmd = WallExtendCommand::new();
    let _ = cmd.on_entity_pick(Handle::new(1), DVec3::ZERO);
    assert!(
        cmd.entity_pick_highlights_hover(),
        "highlight while awaiting target"
    );
}

#[test]
fn formula_layer_resolution_feeds_identical_geometry_for_fixed_styles() {
    // Fixed-only style must produce the same contour geometry through the
    // formula-aware resolver as through a hand-built WallLayer stack.
    let lib = engine::library::seed_default_library();
    let style_id = "style_insulated_ext";
    let resolved = resolve_wall_style_layers(&lib, style_id, None).expect("resolve");
    assert_eq!(resolved.len(), 4);
    assert!((resolved[0].thickness - 0.015).abs() < 1e-12);
    assert!((resolved[1].thickness - 0.175).abs() < 1e-12);
    assert!((resolved[2].thickness - 0.14).abs() < 1e-12);
    assert!((resolved[3].thickness - 0.015).abs() < 1e-12);

    let centerline = vec![(0.0, 0.0), (5.0, 0.0)];
    let layer_data: Vec<(f64, f64)> = resolved
        .iter()
        .map(|l| (l.thickness, l.axis_offset))
        .collect();
    let contours = engine::contour::layer_contours(&centerline, &layer_data);
    assert_eq!(contours.len(), 4);

    // Hand-built equivalent with the same centered axis offsets as the seed style.
    let manual = vec![
        (0.015, -0.1725),
        (0.175, -0.1575),
        (0.14, 0.0175),
        (0.015, 0.1575),
    ];
    let manual_contours = engine::contour::layer_contours(&centerline, &manual);
    assert_eq!(contours.len(), manual_contours.len());
    for (a, b) in contours.iter().zip(manual_contours.iter()) {
        assert_eq!(a.0.len(), b.0.len());
        for (p1, p2) in a.0.iter().zip(b.0.iter()) {
            assert!((p1.0 - p2.0).abs() < 1e-9 && (p1.1 - p2.1).abs() < 1e-9);
        }
        for (p1, p2) in a.1.iter().zip(b.1.iter()) {
            assert!((p1.0 - p2.0).abs() < 1e-9 && (p1.1 - p2.1).abs() < 1e-9);
        }
    }
}

#[test]
fn bb_formula_style_changes_resolved_thickness_with_base_width() {
    let mut lib = StyleLibrary::empty();
    lib.materials.push(Material::new(
        "mat_a".into(),
        "A".into(),
        "SOLID".into(),
        0xFFFFFF,
        "Continuous".into(),
    ));
    lib.upsert_wall_style(WallStyle {
        style: Style {
            id: "style_bb".into(),
            name: "BB half".into(),
            object_kind: "Wall".into(),
            parent_style_id: None,
        },
        layers: vec![
            Layer {
                material_id: "mat_a".into(),
                thickness: LayerValue::Fixed(0.1),
                function: LayerFunction::Structural,
                axis_offset: LayerValue::Fixed(0.0),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: None,
            layer_id: uuid::Uuid::new_v4(),
            },
            Layer {
                material_id: "mat_a".into(),
                thickness: LayerValue::Formula("BB * 0.5".into()),
                function: LayerFunction::Insulation,
                axis_offset: LayerValue::Fixed(0.0),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: None,
            layer_id: uuid::Uuid::new_v4(),
            },
        ],
    display_profiles: std::collections::HashMap::new(),
    });

    let r1 = resolve_wall_style_layers(&lib, "style_bb", Some(0.4)).unwrap();
    let r2 = resolve_wall_style_layers(&lib, "style_bb", Some(0.8)).unwrap();
    assert!((r1[1].thickness - 0.2).abs() < 1e-12);
    assert!((r2[1].thickness - 0.4).abs() < 1e-12);

    let centerline = vec![(0.0, 0.0), (3.0, 0.0)];
    let c1 = engine::contour::layer_contours(
        &centerline,
        &r1.iter().map(|l| (l.thickness, l.axis_offset)).collect::<Vec<_>>(),
    );
    let c2 = engine::contour::layer_contours(
        &centerline,
        &r2.iter().map(|l| (l.thickness, l.axis_offset)).collect::<Vec<_>>(),
    );
    // Different BB must produce different outer extents.
    let y_max = |cs: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)]| {
        cs.iter()
            .flat_map(|(a, b)| a.iter().chain(b.iter()))
            .map(|(_, y)| y.abs())
            .fold(0.0_f64, f64::max)
    };
    assert!(
        (y_max(&c1) - y_max(&c2)).abs() > 1e-6,
        "formula BB must affect geometry extents"
    );
}

#[test]
fn invalid_formula_style_falls_back_without_panic() {
    let mut lib = StyleLibrary::empty();
    lib.materials.push(Material::new(
        "mat_a".into(),
        "A".into(),
        "SOLID".into(),
        0xFFFFFF,
        "Continuous".into(),
    ));
    lib.upsert_wall_style(WallStyle {
        style: Style {
            id: "style_bad".into(),
            name: "Bad formula".into(),
            object_kind: "Wall".into(),
            parent_style_id: None,
        },
        layers: vec![Layer {
            material_id: "mat_a".into(),
            thickness: LayerValue::Formula("NOT_A_VAR / 0".into()),
            function: LayerFunction::Structural,
            axis_offset: LayerValue::Fixed(0.0),
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            role_tag: None,
        layer_id: uuid::Uuid::new_v4(),
        }],
    display_profiles: std::collections::HashMap::new(),
    });

    let resolved = resolve_wall_style_layers(&lib, "style_bad", Some(0.3)).unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].thickness, 0.0);
}

#[test]
fn join_and_delete_maintain_symmetric_peer_links() {
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let add_wall = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    };
    let ha = add_wall(&mut scene, (0.0, 0.0), (5.0, 0.0));
    let hb = add_wall(&mut scene, (0.0, 0.0), (0.0, 5.0));
    join_two_walls_in_document(&mut scene,  ha,  hb,  None,  None,  None).expect("L join");
    assert_eq!(engine::owner_index::peers_of(&scene.document, ha), vec![hb]);
    assert_eq!(engine::owner_index::peers_of(&scene.document, hb), vec![ha]);

    // Snapshot peer XDATA then unlink (simulates undo of join / disconnect).
    let peers_before = engine::owner_index::peers_of(&scene.document, ha);
    assert_eq!(peers_before, vec![hb]);
    engine::owner_index::unlink_peers(&mut scene.document, ha, hb);
    assert!(engine::owner_index::peers_of(&scene.document, ha).is_empty());
    assert!(engine::owner_index::peers_of(&scene.document, hb).is_empty());
    // re-join
    join_two_walls_in_document(&mut scene,  ha,  hb,  None,  None,  None).expect("rejoin");

    let hc = add_wall(&mut scene, (2.5, -3.0), (2.5, 0.0));
    join_two_walls_in_document(&mut scene,  ha,  hc,  None,  None,  None).expect("T join");
    let mut peers_a = engine::owner_index::peers_of(&scene.document, ha);
    peers_a.sort_by_key(|h| h.value());
    let mut expected = vec![hb, hc];
    expected.sort_by_key(|h| h.value());
    assert_eq!(peers_a, expected);

    // Deleting ha clears it from peers.
    unlink_all_wall_peers(&mut scene, ha);
    assert!(engine::owner_index::peers_of(&scene.document, ha).is_empty());
    assert!(!engine::owner_index::peers_of(&scene.document, hb).contains(&ha));
    assert!(!engine::owner_index::peers_of(&scene.document, hc).contains(&ha));
}

#[test]
fn storey_membership_tracks_add_remove_and_reassign() {
    let mut scene = Scene::new();
    let s0 = ensure_storey_entity(&mut scene, 0, Some(&Storey::new("L0", 0.0, 3.0)));
    let s1 = ensure_storey_entity(&mut scene, 1, Some(&Storey::new("L1", 3.0, 3.0)));
    assert!(walls_for_storey(&scene, s0).is_empty());
    assert!(walls_for_storey(&scene, s1).is_empty());

    let w1 = add_multi_layer_wall(&mut scene);
    let w2 = {
        // second wall, same geometry template
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 1.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 1.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    };

    register_wall_in_storey(&mut scene, w1);
    register_wall_in_storey(&mut scene, w2);
    let mut members0 = walls_for_storey(&scene, s0);
    members0.sort_by_key(|h| h.value());
    let mut expected = vec![w1, w2];
    expected.sort_by_key(|h| h.value());
    assert_eq!(members0, expected);
    assert!(walls_for_storey(&scene, s1).is_empty());

    assert!(set_wall_storey(&mut scene, w1, 1));
    assert_eq!(walls_for_storey(&scene, s0), vec![w2]);
    assert_eq!(walls_for_storey(&scene, s1), vec![w1]);
    let wall = wall_from_entity(scene.document.get_entity(w1).unwrap()).unwrap();
    assert_eq!(wall.storey_id, 1);

    unregister_wall_from_storey(&mut scene, w2);
    assert!(walls_for_storey(&scene, s0).is_empty());
    assert_eq!(walls_for_storey(&scene, s1), vec![w1]);

    // erase-path helper clears remaining membership
    unregister_walls_from_storeys(&mut scene, &[w1]);
    assert!(walls_for_storey(&scene, s1).is_empty());
}

#[test]
fn placing_and_removing_opening_keeps_host_child_handles() {
    let mut scene = Scene::new();
    let wall = add_multi_layer_wall(&mut scene);
    assert!(engine::owner_index::children_of(&scene.document, wall).is_empty());
    assert!(openings_for_host_wall(&scene, wall).is_empty());

    let (o1, _) = place_wall_opening(
        &mut scene, 
        wall, 
        DVec3::new(1.5, 0.0, 0.0), 
        engine::openings::OpeningKind::Window, 
    None,  None,  None)
    .expect("place window");
    let (o2, _) = place_wall_opening(
        &mut scene, 
        wall, 
        DVec3::new(3.5, 0.0, 0.0), 
        engine::openings::OpeningKind::Door, 
    None,  None,  None)
    .expect("place door");

    let children = engine::owner_index::children_of(&scene.document, wall);
    assert_eq!(children, vec![o1, o2]);

    let openings = openings_for_host_wall(&scene, wall);
    assert_eq!(openings.len(), 2);
    assert!(openings.iter().any(|o| o.handle == o1 && o.kind == engine::openings::OpeningKind::Window));
    assert!(openings.iter().any(|o| o.handle == o2 && o.kind == engine::openings::OpeningKind::Door));
    // host_wall XDATA still written on the opening entity
    let o1_ent = scene.document.get_entity(o1).unwrap();
    let parsed = opening_from_entity(o1_ent, o1).unwrap();
    assert_eq!(parsed.host_wall, wall);

    remove_wall_opening(&mut scene, o1, None).expect("remove window");
    assert_eq!(
        engine::owner_index::children_of(&scene.document, wall),
        vec![o2]
    );
    let remaining = openings_for_host_wall(&scene, wall);
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].handle, o2);
    assert!(scene.document.get_entity(o1).is_none());

    remove_wall_opening(&mut scene, o2, None).expect("remove door");
    assert!(engine::owner_index::children_of(&scene.document, wall).is_empty());
    assert!(openings_for_host_wall(&scene, wall).is_empty());
}

#[test]
fn two_junctions_on_one_wall_are_handled_correctly() {
    let mut scene = Scene::new();
    // Wall 1: horizontal along Y=0 from X=0 to X=10.
    let mut pl1 = LwPolyline::new();
    pl1.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl1.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut ent1 = EntityType::LwPolyline(pl1);
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut rec1 = ExtendedDataRecord::new(AEC_APPID);
    rec1.values = wall_record("s1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent1.common_mut().extended_data.add_record(rec1);
    let w1 = scene.add_entity(ent1);
    regenerate_wall_representation(&mut scene, w1, None).expect("regen w1");

    // Junction A at (0,0): w1 + w2 + w3
    // w2: vertical from (0,0) to (0,5)
    let mut pl2 = LwPolyline::new();
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 5.0)));
    let mut ent2 = EntityType::LwPolyline(pl2);
    let mut rec2 = ExtendedDataRecord::new(AEC_APPID);
    rec2.values = wall_record("s2", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent2.common_mut().extended_data.add_record(rec2);
    let w2 = scene.add_entity(ent2);
    regenerate_wall_representation(&mut scene, w2, None).expect("regen w2");

    // w3: vertical from (0,0) to (0,-5)
    let mut pl3 = LwPolyline::new();
    pl3.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl3.add_vertex(LwVertex::new(Vector2::new(0.0, -5.0)));
    let mut ent3 = EntityType::LwPolyline(pl3);
    let mut rec3 = ExtendedDataRecord::new(AEC_APPID);
    rec3.values = wall_record("s3", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent3.common_mut().extended_data.add_record(rec3);
    let w3 = scene.add_entity(ent3);
    regenerate_wall_representation(&mut scene, w3, None).expect("regen w3");

    // Junction B at (10,0): w1 + w4 + w5
    // w4: vertical from (10,0) to (10,5)
    let mut pl4 = LwPolyline::new();
    pl4.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    pl4.add_vertex(LwVertex::new(Vector2::new(10.0, 5.0)));
    let mut ent4 = EntityType::LwPolyline(pl4);
    let mut rec4 = ExtendedDataRecord::new(AEC_APPID);
    rec4.values = wall_record("s4", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent4.common_mut().extended_data.add_record(rec4);
    let w4 = scene.add_entity(ent4);
    regenerate_wall_representation(&mut scene, w4, None).expect("regen w4");

    // w5: vertical from (10,0) to (10,-5)
    let mut pl5 = LwPolyline::new();
    pl5.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    pl5.add_vertex(LwVertex::new(Vector2::new(10.0, -5.0)));
    let mut ent5 = EntityType::LwPolyline(pl5);
    let mut rec5 = ExtendedDataRecord::new(AEC_APPID);
    rec5.values = wall_record("s5", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent5.common_mut().extended_data.add_record(rec5);
    let w5 = scene.add_entity(ent5);
    regenerate_wall_representation(&mut scene, w5, None).expect("regen w5");

    // Join Junction A.
    let junc_a_handles = vec![w1, w2, w3];
    let touched_a = join_junction_in_document(&mut scene,  &junc_a_handles,  None,  None,  None,  None).expect("join A");
    assert!(touched_a.contains(&w1));
    assert!(touched_a.contains(&w2));
    assert!(touched_a.contains(&w3));

    // Join Junction B.
    let junc_b_handles = vec![w1, w4, w5];
    let touched_b = join_junction_in_document(&mut scene,  &junc_b_handles,  None,  None,  None,  None).expect("join B");
    assert!(touched_b.contains(&w1));
    assert!(touched_b.contains(&w4));
    assert!(touched_b.contains(&w5));

    // Assert both junctions' participants have mitered footprints.
    for h in &[w1, w2, w3, w4, w5] {
        let wall = wall_from_entity(scene.document.get_entity(*h).unwrap()).unwrap();
        // N-way junction should produce derived handles for mitered layers.
        assert!(!wall.derived_handles.is_empty(), "wall {} should have mitered footprints", h.value());
    }

    // Assert peers_of is correct.
    let peers_w1 = engine::owner_index::peers_of(&scene.document, w1);
    assert!(peers_w1.contains(&w2));
    assert!(peers_w1.contains(&w3));
    assert!(peers_w1.contains(&w4));
    assert!(peers_w1.contains(&w5));
    assert_eq!(peers_w1.len(), 4);

    let peers_w2 = engine::owner_index::peers_of(&scene.document, w2);
    assert_eq!(peers_w2.len(), 2);
    assert!(peers_w2.contains(&w1));
    assert!(peers_w2.contains(&w3));

    // Test the safety guard: passing all handles at once should yield Ambiguous.
    let all_handles = vec![w1, w2, w3, w4, w5];
    let result = join_junction_in_document(&mut scene,  &all_handles,  None,  None,  None,  None);
    assert_eq!(result.err(), Some(JoinError::Ambiguous));
}

#[test]
fn vertex_move_cascades_to_full_junction() {
    let mut scene = Scene::new();
    // 3-way junction at (0,0).
    // W1: (0,0) to (10,0)
    // W2: (0,0) to (0,10)
    // W3: (0,0) to (0,-10)

    let w1 = add_multi_layer_wall(&mut scene);
    update_wall_vertices(&mut scene, w1, &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0)]);
    regenerate_wall_representation(&mut scene, w1, None).unwrap();

    let w2 = add_multi_layer_wall(&mut scene);
    update_wall_vertices(&mut scene, w2, &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 10.0, 0.0)]);
    regenerate_wall_representation(&mut scene, w2, None).unwrap();

    let w3 = add_multi_layer_wall(&mut scene);
    update_wall_vertices(&mut scene, w3, &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, -10.0, 0.0)]);
    regenerate_wall_representation(&mut scene, w3, None).unwrap();

    // Initial join.
    join_junction_in_document(&mut scene,  &[w1, w2, w3],  None,  None,  None,  None).unwrap();

    // Move W1's endpoint at (0,0) slightly to (0.1, 0.1).
    // This should trigger a rebuild of ALL THREE walls via try_auto_join_nearby_walls.
    let mut axis1 = get_wall_vertices(&scene, w1);
    axis1[0] = DVec3::new(0.1, 0.0, 0.0);
    update_wall_vertices(&mut scene, w1, &axis1);
    regenerate_wall_representation(&mut scene, w1, None).unwrap();
    
    let touched = try_auto_join_nearby_walls(&mut scene,  w1,  None,  None,  None);

    // Assert all 3 walls are still joined (their ends moved to (0.1, 0.0)).
    let axis1 = get_wall_vertices(&scene, w1);
    assert!((axis1[0].x - 0.1).abs() < 1e-6);

    // If it worked, W2 and W3 should also have their ends moved to (0.1, 0.0).
    let axis2 = get_wall_vertices(&scene, w2);
    let axis3 = get_wall_vertices(&scene, w3);

    assert!(
        (axis2[0].x - 0.1).abs() < 1e-6,
        "W2 should have followed W1 move to (0.1, 0), got {:?}",
        axis2[0]
    );
    assert!(
        (axis3[0].x - 0.1).abs() < 1e-6,
        "W3 should have followed W1 move to (0.1, 0), got {:?}",
        axis3[0]
    );
    assert!(touched.contains(&w1));
    assert!(touched.contains(&w2));
    assert!(touched.contains(&w3));

    // Assert peer links are still correct.
    let peers1 = engine::owner_index::peers_of(&scene.document, w1);
    assert!(peers1.contains(&w2));
    assert!(peers1.contains(&w3));
    assert_eq!(peers1.len(), 2);

    // Now move W1 far away and assert unlinking.
    let mut axis1 = get_wall_vertices(&scene, w1);
    axis1[0] = DVec3::new(100.0, 100.0, 0.0);
    axis1[1] = DVec3::new(110.0, 100.0, 0.0);
    update_wall_vertices(&mut scene, w1, &axis1);
    
    let touched_far = try_auto_join_nearby_walls(&mut scene,  w1,  None,  None,  None);
    
    let peers1_far = engine::owner_index::peers_of(&scene.document, w1);
    assert!(peers1_far.is_empty(), "W1 should be unlinked after moving far away");
    assert!(touched_far.contains(&w2));
    assert!(touched_far.contains(&w3));
    
    let peers2_far = engine::owner_index::peers_of(&scene.document, w2);
    assert!(!peers2_far.contains(&w1));
    assert!(peers2_far.contains(&w3)); // W2 and W3 still meet at (0.1, 0)
}

/// Collects every vertex of every `LwPolyline` derived (`WALL_REP`)
/// child of `wall_handle` into one flat list, for corner-position
/// assertions against a wall's rendered footprint(s).
fn wall_contour_points(scene: &Scene, wall_handle: Handle) -> Vec<(f64, f64)> {
    wall_contour_loops(scene, wall_handle)
        .into_iter()
        .flatten()
        .collect()
}

fn wall_contour_loops(scene: &Scene, wall_handle: Handle) -> Vec<Vec<(f64, f64)>> {
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return Vec::new();
    };
    let Some(wall) = wall_from_entity(entity) else {
        return Vec::new();
    };
    let mut loops = Vec::new();
    for h in &wall.derived_handles {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*h) {
            loops.push(
                pl.vertices
                    .iter()
                    .map(|v| (v.location.x, v.location.y))
                    .collect(),
            );
        }
    }
    loops
}

fn has_point(pts: &[(f64, f64)], target: (f64, f64), tol: f64) -> bool {
    pts.iter()
        .any(|p| (p.0 - target.0).abs() < tol && (p.1 - target.1).abs() < tol)
}

fn add_single_layer_wall(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64)) -> Handle {
    add_layered_wall(scene, p1, p2, vec![wl("Concrete", 0.2, "Structural")])
}

fn add_layered_wall(
    scene: &mut Scene,
    p1: (f64, f64),
    p2: (f64, f64),
    layers: Vec<WallLayer>,
) -> Handle {
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
    pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
    let mut entity = EntityType::LwPolyline(pl);
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record(
        "style1",
        3.0,
        0,
        &layers,
        &[],
        WallJustification::Center,
        PlanPhase::New,
        None,
    );
    entity.common_mut().extended_data.add_record(record);
    scene.add_entity(entity)
}

/// Regression test for the "other end's join reverts to a plain cap"
/// bug: Wall A is joined to Wall B at A's end 1 (10,0), producing a
/// correctly mitered L-corner there (exact corner coordinates match the
/// `l_corner_miter_shares_diagonal_endpoints` geometry in `miter.rs`).
/// A NEW Wall C is then joined to A's *other* end (0,0). After that
/// second join, A's rendered footprint must still contain the original
/// A-B miter corners *and* a real (non-plain-cap) miter at the A-C end.
#[test]
fn other_end_join_survives_new_join_at_opposite_end() {
    let mut scene = Scene::new();
    let wall_a = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let wall_b = add_single_layer_wall(&mut scene, (10.0, 0.0), (10.0, 10.0));
    regenerate_wall_representation(&mut scene, wall_a, None).expect("regen a");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("regen b");

    join_two_walls_in_document(&mut scene,  wall_a,  wall_b,  None,  None,  None).expect("A-B join");

    // Plain (un-joined) end-0 cap footprint corners for reference.
    let plain_end0 = [(0.0, -0.1), (0.0, 0.1)];
    let corner1 = (10.1, -0.1);
    let corner2 = (9.9, 0.1);

    let pts_before = wall_contour_points(&scene, wall_a);
    assert!(
        has_point(&pts_before, corner1, 1e-6) && has_point(&pts_before, corner2, 1e-6),
        "A-B miter corners must be present right after the first join, got {pts_before:?}"
    );

    // NEW wall C joins A's other end (0,0).
    let wall_c = add_single_layer_wall(&mut scene, (0.0, 0.0), (0.0, -10.0));
    regenerate_wall_representation(&mut scene, wall_c, None).expect("regen c");
    join_two_walls_in_document(&mut scene,  wall_a,  wall_c,  None,  None,  None).expect("A-C join");

    let pts_after = wall_contour_points(&scene, wall_a);
    assert!(
        has_point(&pts_after, corner1, 1e-6) && has_point(&pts_after, corner2, 1e-6),
        "A-B miter corners must survive regeneration triggered by the NEW A-C join, got {pts_after:?}"
    );
    assert!(
        !plain_end0.iter().all(|p| has_point(&pts_after, *p, 1e-6)),
        "end 0 must show a real miter against C, not the plain unjoined cap, got {pts_after:?}"
    );
}

/// N-way variant of the regression above: three walls already meet at
/// one point via `join_junction_in_document`; a fourth wall then joins
/// one of them at *its other end*. Both ends of the middle wall must
/// stay correctly joined afterward.
#[test]
fn n_way_junction_survives_new_join_at_participants_other_end() {
    let mut scene = Scene::new();
    // Genuine (non-collinear) 3-way junction at (0,0): w1 along +X,
    // w2 along +Y, w3 along a third direction — each participant gets
    // a real diagonal miter at the shared point, not a straight-through
    // continuation.
    let w1 = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let w2 = add_single_layer_wall(&mut scene, (0.0, 0.0), (0.0, 10.0));
    let w3 = add_single_layer_wall(&mut scene, (0.0, 0.0), (-10.0, 10.0));
    for h in [w1, w2, w3] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }
    join_junction_in_document(&mut scene,  &[w1, w2, w3],  None,  None,  None,  None).expect("N-way join");

    // Snapshot every vertex near the junction corner (close to the
    // origin) before the new join is introduced.
    let pts_before = wall_contour_points(&scene, w1);
    let near_origin_before: Vec<(f64, f64)> =
        pts_before.into_iter().filter(|p| p.0 < 5.0).collect();
    assert!(
        !near_origin_before.is_empty(),
        "sanity: w1 should have a real N-way miter near the junction"
    );
    let plain_end1 = [(10.0, -0.1), (10.0, 0.1)];
    assert!(
        plain_end1.iter().all(|p| has_point(&wall_contour_points(&scene, w1), *p, 1e-6)),
        "sanity: w1's un-joined end should still be a plain cap before the new join"
    );

    // NEW wall w4 joins w1 at ITS other end (10,0).
    let w4 = add_single_layer_wall(&mut scene, (10.0, 0.0), (10.0, 10.0));
    regenerate_wall_representation(&mut scene, w4, None).expect("regen w4");
    join_two_walls_in_document(&mut scene,  w1,  w4,  None,  None,  None).expect("w1-w4 join");

    let pts_after = wall_contour_points(&scene, w1);
    assert!(
        !plain_end1.iter().all(|p| has_point(&pts_after, *p, 1e-6)),
        "w1's new join end must be a real miter, not the plain cap"
    );
    // The original N-way junction corner geometry (near x=0) must be
    // byte-for-byte preserved after this unrelated join event elsewhere.
    for p in &near_origin_before {
        assert!(
            has_point(&pts_after, *p, 1e-9),
            "w1's original N-way junction corner point {p:?} must survive the new A-B join, got {pts_after:?}"
        );
    }
}

/// A wall with only ONE join (no second join at all) must keep producing
/// exactly the same mitered footprint as before this change — no
/// accidental behavior change for the common single-join case.
#[test]
fn single_join_wall_footprint_unchanged() {
    let mut scene = Scene::new();
    let wall_a = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let wall_b = add_single_layer_wall(&mut scene, (10.0, 0.0), (10.0, 10.0));
    regenerate_wall_representation(&mut scene, wall_a, None).expect("regen a");
    regenerate_wall_representation(&mut scene, wall_b, None).expect("regen b");

    join_two_walls_in_document(&mut scene,  wall_a,  wall_b,  None,  None,  None).expect("A-B join");

    let pts = wall_contour_points(&scene, wall_a);
    let corner1 = (10.1, -0.1);
    let corner2 = (9.9, 0.1);
    assert!(has_point(&pts, corner1, 1e-6) && has_point(&pts, corner2, 1e-6));

    // Plain cap at the un-joined end 0 must be exactly the un-mitered
    // rectangle corners (no peer exists there).
    assert!(has_point(&pts, (0.0, -0.1), 1e-6));
    assert!(has_point(&pts, (0.0, 0.1), 1e-6));
}

/// Regression test for the reported bug: three walls already meet at one
/// point via an N-way junction (w1, w2, w3, all correctly mitered).
/// A NEW wall w4 is then joined to the *same* junction point. After the
/// junction is re-resolved for 4 participants, the *other* walls (w2, w3)
/// — which did not change themselves — must still show their correct
/// mitered footprint, not revert to a plain unjoined cap.
#[test]
fn adding_new_wall_to_existing_junction_keeps_other_walls_mitered() {
    let mut scene = Scene::new();
    let w1 = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
    let w2 = add_single_layer_wall(&mut scene, (0.0, 0.0), (0.0, 10.0));
    let w3 = add_single_layer_wall(&mut scene, (0.0, 0.0), (-10.0, 10.0));
    for h in [w1, w2, w3] {
        regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
    }
    join_junction_in_document(&mut scene,  &[w1, w2, w3],  None,  None,  None,  None).expect("N-way join");

    // Plain (un-joined) cap corners at the origin end of a vertical /
    // diagonal single-layer (0.2 thick) wall — what w2/w3 would show at
    // their origin end if the junction miter were lost and they fell
    // back to an un-joined rectangle cap.
    let plain_origin_cap = [(-0.1, 0.0), (0.1, 0.0)];

    let w2_before = wall_contour_points(&scene, w2);
    let w3_before = wall_contour_points(&scene, w3);
    assert!(
        !plain_origin_cap.iter().all(|p| has_point(&w2_before, *p, 1e-6)),
        "sanity: w2 should have a real N-way miter, not a plain cap, got {w2_before:?}"
    );
    assert!(
        !plain_origin_cap.iter().all(|p| has_point(&w3_before, *p, 1e-6)),
        "sanity: w3 should have a real N-way miter, not a plain cap, got {w3_before:?}"
    );

    // NEW wall w4 joins the SAME junction point (0,0), the way the
    // interactive drawing workflow actually triggers it: via
    // `try_auto_join_nearby_walls` for just the newly drawn wall.
    let w4 = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, -10.0));
    regenerate_wall_representation(&mut scene, w4, None).expect("regen w4");
    try_auto_join_nearby_walls(&mut scene,  w4,  None,  None,  None);

    let w2_after = wall_contour_points(&scene, w2);
    let w3_after = wall_contour_points(&scene, w3);
    assert!(
        !plain_origin_cap.iter().all(|p| has_point(&w2_after, *p, 1e-6)),
        "w2 must still show a real miter at the junction after w4 joins, not revert to a plain cap, got {w2_after:?}"
    );
    assert!(
        !plain_origin_cap.iter().all(|p| has_point(&w3_after, *p, 1e-6)),
        "w3 must still show a real miter at the junction after w4 joins, not revert to a plain cap, got {w3_after:?}"
    );
}

#[test]
fn l_corner_with_window_keeps_miter_in_2d_and_3d() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];

    // Wall 1: horizontal along Y=0 from X=0 to X=10.
    let mut pl1 = LwPolyline::new();
    pl1.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl1.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut ent1 = EntityType::LwPolyline(pl1);
    let mut rec1 = ExtendedDataRecord::new(AEC_APPID);
    rec1.values = wall_record("s1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent1.common_mut().extended_data.add_record(rec1);
    let w1 = scene.add_entity(ent1);

    // Wall 2: vertical along X=0 from Y=0 to Y=10.
    let mut pl2 = LwPolyline::new();
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 10.0)));
    let mut ent2 = EntityType::LwPolyline(pl2);
    let mut rec2 = ExtendedDataRecord::new(AEC_APPID);
    rec2.values = wall_record("s2", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent2.common_mut().extended_data.add_record(rec2);
    let w2 = scene.add_entity(ent2);

    // Join w1 and w2 as an L-corner at (0, 0).
    let mut command_line = CommandLine::default();
    aec_walljoin_do(
        &mut scene,
        &mut command_line,
        &format!("{}|{}", w1.value(), w2.value()),
        None, None, None,
    );

    // Place a window on w1 at X=5.0 (width 1.2m).
    let (_window_h, _) = place_wall_opening(
        &mut scene,
        w1,
        DVec3::new(5.0, 0.0, 0.0),
        engine::openings::OpeningKind::Window,
        None, None, None,
    ).expect("place window");

    // Regenerate w1 with its miter preserved.
    regenerate_wall_representation(&mut scene, w1, None).expect("regen w1");

    let wall_rec = wall_from_entity(scene.document.get_entity(w1).unwrap()).unwrap();

    // Check 2D contours: must have 2 pieces (one from corner to window, one from window to end).
    let mut contours = Vec::new();
    let mut solids = Vec::new();
    for h in &wall_rec.derived_handles {
        match scene.document.get_entity(*h) {
            Some(EntityType::LwPolyline(pl)) => contours.push(pl.clone()),
            Some(EntityType::Solid3D(s3d)) => solids.push(s3d.clone()),
            _ => {}
        }
    }

    assert_eq!(contours.len(), 2, "Wall with window must have 2 contour pieces");
    // The start piece must have a miter vertex (at -0.1, -0.1).
    let has_miter_vertex_2d = contours.iter().any(|pl| {
        pl.vertices.iter().any(|v| v.location.x < -0.05 && v.location.y < -0.05)
    });
    assert!(has_miter_vertex_2d, "2D contour must retain miter corner vertex at (-0.1, -0.1)");

    // Check 3D solids: rest wall solids (2 pieces) plus opening zone solids (lintel + sill).
    assert!(solids.len() >= 2, "3D wall must have at least 2 rest-wall solids");

    // The first 3D rest-wall solid must have wires reaching the corner miter vertex (-0.1, -0.1).
    let has_miter_wire_3d = solids.iter().any(|s| {
        s.wires.iter().any(|wire| {
            wire.points.iter().any(|p| p.x < -0.05 && p.y < -0.05)
        })
    });
    assert!(has_miter_wire_3d, "3D solid must retain miter corner geometry at (-0.1, -0.1)");
}

#[test]
fn l_corner_same_material_suppresses_miter_in_2d_contour_while_hatch_stays_closed() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];

    let mut pl1 = LwPolyline::new();
    pl1.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl1.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut ent1 = EntityType::LwPolyline(pl1);
    let mut rec1 = ExtendedDataRecord::new(AEC_APPID);
    rec1.values = wall_record("s1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent1.common_mut().extended_data.add_record(rec1);
    let w1 = scene.add_entity(ent1);

    let mut pl2 = LwPolyline::new();
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 5.0)));
    let mut ent2 = EntityType::LwPolyline(pl2);
    let mut rec2 = ExtendedDataRecord::new(AEC_APPID);
    rec2.values = wall_record("s2", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent2.common_mut().extended_data.add_record(rec2);
    let w2 = scene.add_entity(ent2);

    let mut command_line = CommandLine::default();
    aec_walljoin_do(
        &mut scene,
        &mut command_line,
        &format!("{}|{}", w1.value(), w2.value()),
        None, None, None,
    );

    let wall1 = wall_from_entity(scene.document.get_entity(w1).unwrap()).unwrap();
    let wall2 = wall_from_entity(scene.document.get_entity(w2).unwrap()).unwrap();

    let mut w1_contours = Vec::new();
    let mut w1_has_hatch = false;
    for h in &wall1.derived_handles {
        match scene.document.get_entity(*h) {
            Some(EntityType::LwPolyline(pl)) => w1_contours.push(pl.clone()),
            Some(EntityType::Hatch(_)) => w1_has_hatch = true,
            _ => {}
        }
    }
    let mut w2_contours = Vec::new();
    for h in &wall2.derived_handles {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*h) {
            w2_contours.push(pl.clone());
        }
    }

    assert!(!w1_contours.is_empty(), "Wall 1 must have 2D contour");
    assert!(!w2_contours.is_empty(), "Wall 2 must have 2D contour");
    assert!(w1_has_hatch, "Wall 1 must have Hatch entity");

    // Both contours must be open polylines because the miter edge was suppressed.
    assert!(!w1_contours[0].is_closed, "Wall 1 contour must be an open polyline (no miter line)");
    assert!(!w2_contours[0].is_closed, "Wall 2 contour must be an open polyline (no miter line)");

    // Neither contour should draw a segment between (-0.1, -0.1) and (0.1, 0.1).
    let draws_miter_segment = |pl: &LwPolyline| {
        let n = pl.vertices.len();
        let limit = if pl.is_closed { n } else { n - 1 };
        for i in 0..limit {
            let p0 = pl.vertices[i].location;
            let p1 = pl.vertices[(i + 1) % n].location;
            let d00 = (p0.x - -0.1).hypot(p0.y - -0.1);
            let d11 = (p1.x - 0.1).hypot(p1.y - 0.1);
            let d01 = (p0.x - 0.1).hypot(p0.y - 0.1);
            let d10 = (p1.x - -0.1).hypot(p1.y - -0.1);
            if (d00 < 1e-4 && d11 < 1e-4) || (d01 < 1e-4 && d10 < 1e-4) {
                return true;
            }
        }
        false
    };

    assert!(
        !draws_miter_segment(&w1_contours[0]),
        "Wall 1 must NOT draw the diagonal miter edge across identical material"
    );
    assert!(
        !draws_miter_segment(&w2_contours[0]),
        "Wall 2 must NOT draw the diagonal miter edge across identical material"
    );
}

#[test]
fn l_corner_different_materials_keeps_miter_boundary_line() {
    use crate::ui::command_line::CommandLine;

    let mut scene = Scene::new();
    let layers1 = vec![wl("Concrete", 0.2, "Structural")];
    let layers2 = vec![wl("Brick", 0.2, "Structural")];

    let mut pl1 = LwPolyline::new();
    pl1.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl1.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut ent1 = EntityType::LwPolyline(pl1);
    let mut rec1 = ExtendedDataRecord::new(AEC_APPID);
    rec1.values = wall_record("s1", 3.0, 0, &layers1, &[], WallJustification::Center, PlanPhase::New, None);
    ent1.common_mut().extended_data.add_record(rec1);
    let w1 = scene.add_entity(ent1);

    let mut pl2 = LwPolyline::new();
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 5.0)));
    let mut ent2 = EntityType::LwPolyline(pl2);
    let mut rec2 = ExtendedDataRecord::new(AEC_APPID);
    rec2.values = wall_record("s2", 3.0, 0, &layers2, &[], WallJustification::Center, PlanPhase::New, None);
    ent2.common_mut().extended_data.add_record(rec2);
    let w2 = scene.add_entity(ent2);

    let mut command_line = CommandLine::default();
    aec_walljoin_do(
        &mut scene,
        &mut command_line,
        &format!("{}|{}", w1.value(), w2.value()),
        None, None, None,
    );

    let wall1 = wall_from_entity(scene.document.get_entity(w1).unwrap()).unwrap();
    let mut w1_contours = Vec::new();
    for h in &wall1.derived_handles {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*h) {
            w1_contours.push(pl.clone());
        }
    }

    assert!(!w1_contours.is_empty(), "Wall 1 must have 2D contour");
    assert!(
        w1_contours[0].is_closed,
        "Wall 1 contour must remain a closed polyline to show the material boundary between Concrete and Brick"
    );
}

#[test]
fn wall_with_window_3d_cleans_coplanar_seams_while_preserving_reveal_edges() {
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];

    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    // Place window at X=5.0, width 1.2 (span 4.4 to 5.6), sill 0.9, height 1.3 (head 2.2).
    let (_win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(5.0, 0.0, 0.0),
        engine::openings::OpeningKind::Window,
        None, None, None,
    ).expect("place window");

    regenerate_wall_representation(&mut scene, wall_h, None).expect("regen wall");

    let wall_rec = wall_from_entity(scene.document.get_entity(wall_h).unwrap()).unwrap();
    let mut solids = Vec::new();
    for h in &wall_rec.derived_handles {
        if let Some(EntityType::Solid3D(s3d)) = scene.document.get_entity(*h) {
            solids.push(s3d.clone());
        }
    }

    assert!(solids.len() >= 3, "Wall with window must have rest solids and zone solids");

    // Collect all wire segments from all 3D solids of the wall.
    let mut all_segments: Vec<([f64; 3], [f64; 3])> = Vec::new();
    for s in &solids {
        for w in &s.wires {
            for win in w.points.windows(2) {
                all_segments.push(([win[0].x, win[0].y, win[0].z], [win[1].x, win[1].y, win[1].z]));
            }
        }
    }

    // Check for coplanar vertical seams at X=4.4 or X=5.6:
    // Any vertical segment below the sill (z_max <= 0.89) or above the lintel (z_min >= 2.21)
    // at X near 4.4 or 5.6 must be suppressed.
    let has_vertical_seam_below_sill = all_segments.iter().any(|(p0, p1)| {
        let is_at_cut = (p0[0] - 4.4).abs() < 1e-3 || (p0[0] - 5.6).abs() < 1e-3;
        let is_vertical = (p1[0] - p0[0]).hypot(p1[1] - p0[1]) < 1e-3 && (p1[2] - p0[2]).abs() > 0.05;
        let z_max = p0[2].max(p1[2]);
        is_at_cut && is_vertical && z_max <= 0.89
    });
    assert!(
        !has_vertical_seam_below_sill,
        "Vertical coplanar seams below the window sill (z <= 0.9) must be suppressed in 3D"
    );

    let has_vertical_seam_above_head = all_segments.iter().any(|(p0, p1)| {
        let is_at_cut = (p0[0] - 4.4).abs() < 1e-3 || (p0[0] - 5.6).abs() < 1e-3;
        let is_vertical = (p1[0] - p0[0]).hypot(p1[1] - p0[1]) < 1e-3 && (p1[2] - p0[2]).abs() > 0.05;
        let z_min = p0[2].min(p1[2]);
        is_at_cut && is_vertical && z_min >= 2.21
    });
    assert!(
        !has_vertical_seam_above_head,
        "Vertical coplanar seams above the window lintel (z >= 2.2) must be suppressed in 3D"
    );

    // Check that window reveal edges (jambs between z=0.9 and 2.2) ARE preserved:
    let has_reveal_jamb = all_segments.iter().any(|(p0, p1)| {
        let is_at_cut = (p0[0] - 4.4).abs() < 1e-3 || (p0[0] - 5.6).abs() < 1e-3;
        let is_vertical = (p1[0] - p0[0]).hypot(p1[1] - p0[1]) < 1e-3 && (p1[2] - p0[2]).abs() > 0.1;
        let z_min = p0[2].min(p1[2]);
        let z_max = p0[2].max(p1[2]);
        is_at_cut && is_vertical && z_min >= 0.89 && z_max <= 2.21
    });
    assert!(
        has_reveal_jamb,
        "Window reveal jamb edges (between z=0.9 and 2.2) must be preserved and sharp"
    );

    // Check that horizontal reveal edges (sill top z=0.9 across wall thickness) are preserved:
    let has_sill_top_edge = all_segments.iter().any(|(p0, p1)| {
        let is_at_cut = (p0[0] - 4.4).abs() < 1e-3 || (p0[0] - 5.6).abs() < 1e-3;
        let is_transverse = (p1[1] - p0[1]).abs() > 0.1;
        let is_at_sill = (p0[2] - 0.9).abs() < 1e-3 && (p1[2] - 0.9).abs() < 1e-3;
        is_at_cut && is_transverse && is_at_sill
    });
    assert!(
        has_sill_top_edge,
        "Window sill top horizontal edge across wall thickness must be preserved"
    );
}

#[test]
fn wall_with_arch_window_3d_cleans_coplanar_seams_while_preserving_reveal_edges() {
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];

    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    // Place window at X=5.0, width 1.2 (span 4.4 to 5.6), sill 0.9, height 1.3, Arch with spring 0.7.
    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(5.0, 0.0, 0.0),
        engine::openings::OpeningKind::Window,
        None, None, None,
    ).expect("place window");

    let mut opening = engine::opening_xdata::openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("opening found");
    opening.shape = engine::opening_shape::OpeningShape::Arch;
    opening.spring_height = 0.7;
    engine::opening_xdata::write_opening_instance(&mut scene, &opening);

    regenerate_wall_representation(&mut scene, wall_h, None).expect("regen wall");

    let wall_rec = wall_from_entity(scene.document.get_entity(wall_h).unwrap()).unwrap();
    let mut solids = Vec::new();
    for h in &wall_rec.derived_handles {
        if let Some(EntityType::Solid3D(s3d)) = scene.document.get_entity(*h) {
            solids.push(s3d.clone());
        }
    }

    assert!(solids.len() >= 3, "Wall with arch window must have rest solids and zone solids");

    let mut all_segments: Vec<([f64; 3], [f64; 3])> = Vec::new();
    for s in &solids {
        for w in &s.wires {
            for win in w.points.windows(2) {
                all_segments.push(([win[0].x, win[0].y, win[0].z], [win[1].x, win[1].y, win[1].z]));
            }
        }
    }

    // Check for coplanar vertical seams above the arch lintel in the span (4.4 < X < 5.6):
    let has_internal_vertical_seam_above_arch = all_segments.iter().any(|(p0, p1)| {
        let is_inside_zone = p0[0] > 4.41 && p0[0] < 5.59;
        let is_vertical = (p1[0] - p0[0]).hypot(p1[1] - p0[1]) < 1e-3 && (p1[2] - p0[2]).abs() > 0.05;
        is_inside_zone && is_vertical
    });
    assert!(
        !has_internal_vertical_seam_above_arch,
        "All internal vertical slice seams inside the arch opening zone must be suppressed in 3D"
    );

    // Check that transverse crown lines across top deck (z=3.0) inside the zone (4.4 < X < 5.6) are suppressed:
    let has_transverse_crown_seam = all_segments.iter().any(|(p0, p1)| {
        let is_inside_zone = p0[0] > 4.41 && p0[0] < 5.59;
        let is_at_crown = (p0[2] - 3.0).abs() < 1e-3 && (p1[2] - 3.0).abs() < 1e-3;
        let is_transverse = (p1[1] - p0[1]).abs() > 0.05;
        is_inside_zone && is_at_crown && is_transverse
    });
    assert!(
        !has_transverse_crown_seam,
        "Transverse crown seam lines across the top deck of the arch window must be suppressed"
    );

    // Check that arch reveal edges (curved arc chords) ARE preserved:
    let has_arch_curve = all_segments.iter().any(|(p0, p1)| {
        let is_inside_zone = p0[0] > 4.4 && p0[0] < 5.6 && p1[0] > 4.4 && p1[0] < 5.6;
        let dx = (p1[0] - p0[0]).abs();
        let at_arch_z = p0[2] >= 1.59 && p0[2] <= 2.21 && p1[2] >= 1.59 && p1[2] <= 2.21;
        is_inside_zone && dx > 0.01 && at_arch_z
    });
    assert!(
        has_arch_curve,
        "Arch curve reveal edges must be preserved"
    );

    // Check that straight side jambs (between z=0.9 and 1.6) ARE preserved:
    let has_straight_jamb = all_segments.iter().any(|(p0, p1)| {
        let is_at_cut = (p0[0] - 4.4).abs() < 1e-3 || (p0[0] - 5.6).abs() < 1e-3;
        let is_vertical = (p1[0] - p0[0]).hypot(p1[1] - p0[1]) < 1e-3 && (p1[2] - p0[2]).abs() > 0.1;
        let z_min = p0[2].min(p1[2]);
        let z_max = p0[2].max(p1[2]);
        is_at_cut && is_vertical && z_min >= 0.89 && z_max <= 1.61
    });
    assert!(
        has_straight_jamb,
        "Arch straight side jambs below spring height must be preserved"
    );
}

#[test]
fn wall_with_circle_window_3d_cleans_coplanar_seams_while_preserving_reveal_edges() {
    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];

    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    // Place window at X=5.0, width 1.2 (span 4.4 to 5.6), sill 0.9, height 1.2, Circle shape.
    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(5.0, 0.0, 0.0),
        engine::openings::OpeningKind::Window,
        None, None, None,
    ).expect("place window");

    let mut opening = engine::opening_xdata::openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("opening found");
    opening.shape = engine::opening_shape::OpeningShape::Circle;
    opening.height = 1.2;
    engine::opening_xdata::write_opening_instance(&mut scene, &opening);

    regenerate_wall_representation(&mut scene, wall_h, None).expect("regen wall");

    let wall_rec = wall_from_entity(scene.document.get_entity(wall_h).unwrap()).unwrap();
    let mut solids = Vec::new();
    for h in &wall_rec.derived_handles {
        if let Some(EntityType::Solid3D(s3d)) = scene.document.get_entity(*h) {
            solids.push(s3d.clone());
        }
    }

    assert!(solids.len() >= 3, "Wall with circle window must have rest solids and zone solids");

    let mut all_segments: Vec<([f64; 3], [f64; 3])> = Vec::new();
    for s in &solids {
        for w in &s.wires {
            for win in w.points.windows(2) {
                all_segments.push(([win[0].x, win[0].y, win[0].z], [win[1].x, win[1].y, win[1].z]));
            }
        }
    }

    // Check that ALL vertical seams across the entire zone [4.4, 5.6] on the wall surface are suppressed:
    let has_vertical_surface_seam = all_segments.iter().any(|(p0, p1)| {
        let is_in_zone = p0[0] >= 4.39 && p0[0] <= 5.61;
        let is_vertical = (p1[0] - p0[0]).hypot(p1[1] - p0[1]) < 1e-3 && (p1[2] - p0[2]).abs() > 0.05;
        let on_face = (p0[1].abs() - 0.1).abs() < 1e-3;
        is_in_zone && is_vertical && on_face
    });
    assert!(
        !has_vertical_surface_seam,
        "All vertical surface seam lines around a circle window must be suppressed"
    );

    // Check that transverse crown lines across top deck (z=3.0) inside the zone are suppressed:
    let has_transverse_crown_seam = all_segments.iter().any(|(p0, p1)| {
        let is_inside_zone = p0[0] >= 4.39 && p0[0] <= 5.61;
        let is_at_crown = (p0[2] - 3.0).abs() < 1e-3 && (p1[2] - 3.0).abs() < 1e-3;
        let is_transverse = (p1[1] - p0[1]).abs() > 0.05;
        is_inside_zone && is_at_crown && is_transverse
    });
    assert!(
        !has_transverse_crown_seam,
        "Transverse crown seam lines across the top deck of the circle window must be suppressed"
    );

    // Check that circular reveal edges ARE preserved:
    let has_circular_reveal = all_segments.iter().any(|(p0, p1)| {
        let is_inside_zone = p0[0] > 4.4 && p0[0] < 5.6 && p1[0] > 4.4 && p1[0] < 5.6;
        let dx = (p1[0] - p0[0]).abs();
        let dz = (p1[2] - p0[2]).abs();
        let at_circle_z = p0[2] >= 0.89 && p0[2] <= 2.11 && p1[2] >= 0.89 && p1[2] <= 2.11;
        is_inside_zone && dx > 0.01 && dz > 0.01 && at_circle_z
    });
    assert!(
        has_circular_reveal,
        "Circular reveal edges must be preserved"
    );
}

#[test]
fn l_corner_same_material_suppresses_3d_deck_miter_fuge_while_different_material_keeps_it() {
    use crate::ui::command_line::CommandLine;

    // --- Scenario A: Same material (Concrete + Concrete) ---
    let mut scene = Scene::new();
    let layers_conc = vec![wl("Concrete", 0.2, "Structural")];

    let mut pl1 = LwPolyline::new();
    pl1.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl1.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut ent1 = EntityType::LwPolyline(pl1);
    let mut rec1 = ExtendedDataRecord::new(AEC_APPID);
    rec1.values = wall_record("s1", 3.0, 0, &layers_conc, &[], WallJustification::Center, PlanPhase::New, None);
    ent1.common_mut().extended_data.add_record(rec1);
    let w1 = scene.add_entity(ent1);

    let mut pl2 = LwPolyline::new();
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 5.0)));
    let mut ent2 = EntityType::LwPolyline(pl2);
    let mut rec2 = ExtendedDataRecord::new(AEC_APPID);
    rec2.values = wall_record("s2", 3.0, 0, &layers_conc, &[], WallJustification::Center, PlanPhase::New, None);
    ent2.common_mut().extended_data.add_record(rec2);
    let w2 = scene.add_entity(ent2);

    let mut command_line = CommandLine::default();
    aec_walljoin_do(
        &mut scene,
        &mut command_line,
        &format!("{}|{}", w1.value(), w2.value()),
        None, None, None,
    );

    let wall1 = wall_from_entity(scene.document.get_entity(w1).unwrap()).unwrap();
    let s1 = wall1.derived_handles.iter().find_map(|h| match scene.document.get_entity(*h) {
        Some(EntityType::Solid3D(s)) => Some(s.clone()),
        _ => None,
    }).expect("Wall 1 must have 3D solid");

    let draws_diagonal_deck_wire = |s: &acadrust::entities::Solid3D, z_level: f64| {
        for w in &s.wires {
            for win in w.points.windows(2) {
                let p0 = &win[0];
                let p1 = &win[1];
                let at_z = (p0.z - z_level).abs() < 1e-3 && (p1.z - z_level).abs() < 1e-3;
                let d0a = (p0.x - -0.1).hypot(p0.y - -0.1);
                let d1b = (p1.x - 0.1).hypot(p1.y - 0.1);
                let d0b = (p0.x - 0.1).hypot(p0.y - 0.1);
                let d1a = (p1.x - -0.1).hypot(p1.y - -0.1);
                if at_z && ((d0a < 1e-3 && d1b < 1e-3) || (d0b < 1e-3 && d1a < 1e-3)) {
                    return true;
                }
            }
        }
        false
    };

    assert!(
        !draws_diagonal_deck_wire(&s1, 3.0),
        "Wall 1 must NOT draw 45-degree diagonal fuge on top deck (z=3.0) when joining same material"
    );
    assert!(
        !draws_diagonal_deck_wire(&s1, 0.0),
        "Wall 1 must NOT draw 45-degree diagonal fuge on bottom base (z=0.0) when joining same material"
    );

    // Verify outer corner vertical wire is still present!
    let has_outer_corner_wire = s1.wires.iter().any(|w| {
        w.points.windows(2).any(|win| {
            let p0 = &win[0];
            let p1 = &win[1];
            (p0.x - -0.1).hypot(p0.y - -0.1) < 1e-3
                && (p1.x - -0.1).hypot(p1.y - -0.1) < 1e-3
                && (p1.z - p0.z).abs() > 2.0
        })
    });
    assert!(has_outer_corner_wire, "Outer vertical corner wire must remain intact and sharp");

    // --- Scenario B: Different materials (Concrete + Brick) ---
    let mut scene2 = Scene::new();
    let layers_brick = vec![wl("Brick", 0.2, "Structural")];

    let mut pl3 = LwPolyline::new();
    pl3.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl3.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    let mut ent3 = EntityType::LwPolyline(pl3);
    let mut rec3 = ExtendedDataRecord::new(AEC_APPID);
    rec3.values = wall_record("s3", 3.0, 0, &layers_conc, &[], WallJustification::Center, PlanPhase::New, None);
    ent3.common_mut().extended_data.add_record(rec3);
    let w3 = scene2.add_entity(ent3);

    let mut pl4 = LwPolyline::new();
    pl4.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl4.add_vertex(LwVertex::new(Vector2::new(0.0, 5.0)));
    let mut ent4 = EntityType::LwPolyline(pl4);
    let mut rec4 = ExtendedDataRecord::new(AEC_APPID);
    rec4.values = wall_record("s4", 3.0, 0, &layers_brick, &[], WallJustification::Center, PlanPhase::New, None);
    ent4.common_mut().extended_data.add_record(rec4);
    let w4 = scene2.add_entity(ent4);

    let mut command_line2 = CommandLine::default();
    aec_walljoin_do(
        &mut scene2,
        &mut command_line2,
        &format!("{}|{}", w3.value(), w4.value()),
        None, None, None,
    );

    let wall3 = wall_from_entity(scene2.document.get_entity(w3).unwrap()).unwrap();
    let s3 = wall3.derived_handles.iter().find_map(|h| match scene2.document.get_entity(*h) {
        Some(EntityType::Solid3D(s)) => Some(s.clone()),
        _ => None,
    }).expect("Wall 3 must have 3D solid");

    assert!(
        draws_diagonal_deck_wire(&s3, 3.0),
        "Wall 3 MUST draw 45-degree diagonal boundary on top deck (z=3.0) when joining DIFFERENT materials"
    );
}

#[test]
fn test_opening_realistic_post_profiles() {
    use crate::modules::aec::engine::display_component::OpeningComponentSlot;
    use crate::modules::aec::engine::opening_display::{bake_opening_generators, OpeningBakeParams};
    use crate::modules::aec::engine::opening_style::{
        HingeSide, OpeningGenerator, SlotGeometry, DEFAULT_FRAME_THICKNESS,
    };
    use crate::modules::aec::engine::openings::OpeningKind;
    use std::collections::HashMap;

    let mut slots = HashMap::new();
    slots.insert(
        OpeningComponentSlot::Frame2D,
        SlotGeometry::Generator(OpeningGenerator::FrameRect),
    );

    let width = 1.4;
    let thickness = 0.3;
    let ft = DEFAULT_FRAME_THICKNESS; // 0.07
    let p = OpeningBakeParams {
        width,
        height: 1.2,
        sill_height: 0.9,
        thickness,
        frame_thickness: ft,
        cross_axis_offset: 0.0,
        hinge: HingeSide::Left,
        shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
        spring_height: 0.0,
        opening_angle_deg: 90.0,
        kind: OpeningKind::Window,
    };

    let baked = bake_opening_generators(&slots, p, None);
    let frames: Vec<_> = baked
        .iter()
        .filter(|b| b.slot == OpeningComponentSlot::Frame2D && b.closed)
        .collect();

    assert_eq!(frames.len(), 2, "Window Frame2D must emit two separate post profiles");

    let hw = width * 0.5; // 0.7
    let post_a = &frames[0];
    let post_b = &frames[1];

    let min_x_a = post_a.points.iter().map(|pt| pt.0).fold(f64::INFINITY, f64::min);
    let max_x_a = post_a.points.iter().map(|pt| pt.0).fold(f64::NEG_INFINITY, f64::max);
    let min_x_b = post_b.points.iter().map(|pt| pt.0).fold(f64::INFINITY, f64::min);
    let max_x_b = post_b.points.iter().map(|pt| pt.0).fold(f64::NEG_INFINITY, f64::max);

    // Left post: [-0.70, -0.63]
    let (left_min, left_max, right_min, right_max) = if min_x_a < min_x_b {
        (min_x_a, max_x_a, min_x_b, max_x_b)
    } else {
        (min_x_b, max_x_b, min_x_a, max_x_a)
    };

    assert!((left_min - (-hw)).abs() < 1e-6, "Left post must sit at left reveal -hw");
    assert!((left_max - (-hw + ft)).abs() < 1e-6, "Left post width must equal ft");
    assert!((right_min - (hw - ft)).abs() < 1e-6, "Right post inner edge must be at hw - ft");
    assert!((right_max - hw).abs() < 1e-6, "Right post outer edge must be at right reveal hw");

    let clear_width = right_min - left_max;
    assert!((clear_width - (width - 2.0 * ft)).abs() < 1e-6, "Clear opening between posts must be width - 2*ft");
}

#[test]
fn test_opening_dynamic_sill_lines_with_wall_thickness() {
    use crate::modules::aec::engine::display_component::OpeningComponentSlot;
    use crate::modules::aec::engine::opening_display::{bake_opening_generators, OpeningBakeParams};
    use crate::modules::aec::engine::opening_style::{
        HingeSide, OpeningGenerator, SlotGeometry, DEFAULT_FRAME_THICKNESS,
    };
    use crate::modules::aec::engine::openings::OpeningKind;
    use std::collections::HashMap;

    let mut slots = HashMap::new();
    slots.insert(
        OpeningComponentSlot::Sill2D,
        SlotGeometry::Generator(OpeningGenerator::SillLines),
    );

    for thickness in [0.24, 0.365, 0.42] {
        let hw = 0.6;
        let ht = thickness * 0.5;
        let p = OpeningBakeParams {
            width: hw * 2.0,
            height: 1.2,
            sill_height: 0.9,
            thickness,
            frame_thickness: DEFAULT_FRAME_THICKNESS,
            cross_axis_offset: 0.0,
            hinge: HingeSide::Left,
            shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
            spring_height: 0.0,
            opening_angle_deg: 90.0,
            kind: OpeningKind::Window,
        };

        let baked = bake_opening_generators(&slots, p, None);
        let sills: Vec<_> = baked
            .iter()
            .filter(|b| b.slot == OpeningComponentSlot::Sill2D)
            .collect();
        assert!(!sills.is_empty(), "Sill lines must be generated for thickness {thickness}");

        // Find the exterior sill front edge (lowest y)
        let min_y = sills
            .iter()
            .flat_map(|b| b.points.iter().map(|pt| pt.1))
            .fold(f64::INFINITY, f64::min);
        // Find the interior board front edge (highest y)
        let max_y = sills
            .iter()
            .flat_map(|b| b.points.iter().map(|pt| pt.1))
            .fold(f64::NEG_INFINITY, f64::max);

        // Exterior sill front is at -ht - 0.035
        assert!((min_y - (-ht - 0.035)).abs() < 1e-4, "Exterior sill must track wall exterior at -ht for thickness {thickness}");
        // Interior board front is at +ht + 0.025
        assert!((max_y - (ht + 0.025)).abs() < 1e-4, "Interior board must track wall interior at +ht for thickness {thickness}");

        // Wall face reveal line exists at -ht and +ht
        let has_ext_wall_line = sills.iter().any(|b| {
            b.points.len() == 2 && (b.points[0].1 - (-ht)).abs() < 1e-4 && (b.points[1].1 - (-ht)).abs() < 1e-4
        });
        let has_int_wall_line = sills.iter().any(|b| {
            b.points.len() == 2 && (b.points[0].1 - ht).abs() < 1e-4 && (b.points[1].1 - ht).abs() < 1e-4
        });
        assert!(has_ext_wall_line, "Must have reveal line at exterior wall face -ht");
        assert!(has_int_wall_line, "Must have reveal line at interior wall face +ht");
    }
}

#[test]
fn test_opening_component_blocks_resolution_and_placement() {
    use crate::modules::aec::engine::display_component::OpeningComponentSlot;
    use crate::modules::aec::engine::opening_display::{
        bake_opening_generators_with_doc, OpeningBakeParams,
    };
    use crate::modules::aec::engine::opening_style::{
        BlockPlacementMode, HingeSide, SlotGeometry, DEFAULT_FRAME_THICKNESS,
    };
    use crate::modules::aec::engine::openings::OpeningKind;
    use acadrust::entities::Line;
    use acadrust::tables::BlockRecord;
    use acadrust::types::Vector3;
    use acadrust::{CadDocument, EntityType};
    use std::collections::HashMap;

    let mut doc = CadDocument::new();
    let block_name = "TEST_JAMB_PROFILE";
    let block_h = doc.allocate_handle();
    let mut rec = BlockRecord::new(block_name);
    rec.handle = block_h;
    doc.block_records.add(rec).expect("add block record");

    // Add a line inside the block record representing a profile from (0, -0.04) to (0.08, 0.04):
    let mut line = Line::new();
    line.start = Vector3::new(0.0, -0.04, 0.0);
    line.end = Vector3::new(0.08, 0.04, 0.0);
    line.common.owner_handle = block_h;
    doc.add_entity(EntityType::Line(line)).expect("add line");

    let p = OpeningBakeParams {
        width: 1.2,
        height: 1.2,
        sill_height: 0.9,
        thickness: 0.3,
        frame_thickness: DEFAULT_FRAME_THICKNESS,
        cross_axis_offset: 0.0,
        hinge: HingeSide::Left,
        shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
        spring_height: 0.0,
        opening_angle_deg: 90.0,
        kind: OpeningKind::Window,
    };

    // 1. JambPair mode
    let mut slots_jamb = HashMap::new();
    slots_jamb.insert(
        OpeningComponentSlot::Frame2D,
        SlotGeometry::Block {
            block_name: block_name.to_string(),
            placement: BlockPlacementMode::JambPair,
        },
    );
    let baked_jamb = bake_opening_generators_with_doc(&slots_jamb, p, None, Some(&doc));
    assert_eq!(baked_jamb.len(), 2, "JambPair must place left and right jambs");
    let hw = 0.6;
    // Left jamb sits at -hw + (0..0.08) -> [-0.60, -0.52]
    let left_pts = &baked_jamb[0].points;
    assert!((left_pts[0].0 - (-hw)).abs() < 1e-6);
    assert!((left_pts[1].0 - (-hw + 0.08)).abs() < 1e-6);
    // Right jamb sits at hw - (0..0.08) -> [0.60, 0.52]
    let right_pts = &baked_jamb[1].points;
    assert!((right_pts[0].0 - hw).abs() < 1e-6);
    assert!((right_pts[1].0 - (hw - 0.08)).abs() < 1e-6);

    // 2. StretchToFit mode
    let mut slots_stretch = HashMap::new();
    slots_stretch.insert(
        OpeningComponentSlot::Frame2D,
        SlotGeometry::Block {
            block_name: block_name.to_string(),
            placement: BlockPlacementMode::StretchToFit,
        },
    );
    let baked_stretch = bake_opening_generators_with_doc(&slots_stretch, p, None, Some(&doc));
    assert_eq!(baked_stretch.len(), 1);
    let stretch_pts = &baked_stretch[0].points;
    assert!((stretch_pts[0].0 - (-hw)).abs() < 1e-6, "StretchToFit start must be at -hw");
    assert!((stretch_pts[1].0 - hw).abs() < 1e-6, "StretchToFit end must be at hw");

    // 3. CenterAnchor mode
    let mut slots_center = HashMap::new();
    slots_center.insert(
        OpeningComponentSlot::Frame2D,
        SlotGeometry::Block {
            block_name: block_name.to_string(),
            placement: BlockPlacementMode::CenterAnchor,
        },
    );
    let baked_center = bake_opening_generators_with_doc(&slots_center, p, None, Some(&doc));
    assert_eq!(baked_center.len(), 1);
    let center_pts = &baked_center[0].points;
    // u was 0.0 to 0.08, mid was 0.04. Centered: -0.04 to +0.04
    assert!((center_pts[0].0 - (-0.04)).abs() < 1e-6);
    assert!((center_pts[1].0 - 0.04).abs() < 1e-6);
}

#[test]
fn test_opening_din1356_label_formatting() {
    use crate::modules::aec::engine::opening_display::format_din1356_label;
    use crate::modules::aec::engine::openings::Opening;
    use acadrust::Handle;

    let mut door = Opening::door(Handle::new(1), Handle::new(2), 2.5);
    door.width = 1.01;
    door.height = 2.135;
    assert_eq!(format_din1356_label(&door), "1.01 / 2.135");

    let mut window = Opening::window(Handle::new(3), Handle::new(4), 4.0);
    window.width = 1.26;
    window.height = 1.40;
    window.sill_height = 0.90;
    assert_eq!(format_din1356_label(&window), "1.26 / 1.40\\PBRH 0.90");

    let mut breakthrough = Opening::breakthrough(Handle::new(5), Handle::new(6), 1.0);
    breakthrough.width = 0.50;
    breakthrough.height = 0.60;
    breakthrough.sill_height = 2.20;
    assert_eq!(format_din1356_label(&breakthrough), "0.50 / 0.60\\PUK 2.20");
}

#[test]
fn test_opening_reference_side_positioning_and_flip() {
    use crate::modules::aec::engine::opening_style::HingeSide;
    use crate::modules::aec::engine::openings::{Opening, OpeningReferenceSide};
    use acadrust::Handle;

    let mut win = Opening::window(Handle::new(10), Handle::new(20), 2.0);
    win.width = 1.2;
    win.reference_side = OpeningReferenceSide::Start;
    win.hinge = HingeSide::Left;

    // Start reveal is at 2.0, so center is at 2.0 + 0.6 = 2.6, end reveal is at 3.2:
    assert!((win.center_along_axis() - 2.6).abs() < 1e-6);
    let (s0, s1) = win.axis_span();
    assert!((s0 - 2.0).abs() < 1e-6);
    assert!((s1 - 3.2).abs() < 1e-6);

    // Flip: reference side switches from Start to End, hinge from Left to Right
    win.flip();
    assert_eq!(win.reference_side, OpeningReferenceSide::End);
    assert_eq!(win.hinge, HingeSide::Right);

    // If dimensioned from End reveal at 3.2, center and span remain unchanged:
    win.distance_along_axis = 3.2;
    assert!((win.center_along_axis() - 2.6).abs() < 1e-6);
    let (s0_flipped, s1_flipped) = win.axis_span();
    assert!((s0_flipped - 2.0).abs() < 1e-6);
    assert!((s1_flipped - 3.2).abs() < 1e-6);

    // Center reference side:
    win.reference_side = OpeningReferenceSide::Center;
    win.distance_along_axis = 2.6;
    assert!((win.center_along_axis() - 2.6).abs() < 1e-6);
    let (s0_c, s1_c) = win.axis_span();
    assert!((s0_c - 2.0).abs() < 1e-6);
    assert!((s1_c - 3.2).abs() < 1e-6);
}

#[test]
fn test_opening_cross_axis_offset_shifts_geometry_and_sills() {
    use crate::modules::aec::engine::display_component::OpeningComponentSlot;
    use crate::modules::aec::engine::opening_display::bake_opening_world;
    use crate::modules::aec::engine::openings::Opening;
    use acadrust::Handle;

    let axis = vec![(0.0, 0.0), (10.0, 0.0)]; // Wall runs along +X, normal along +Y
    let thickness = 0.30; // ht = 0.15; ext face at y = -0.15, int face at y = +0.15

    let mut win = Opening::window(Handle::new(1), Handle::new(2), 5.0);
    win.width = 1.0;
    win.cross_axis_offset = 0.06; // shifted +6cm towards interior (+Y)

    let paths = bake_opening_world(&axis, thickness, &win, None, None);
    assert!(!paths.is_empty());

    // Window frame posts should be centered around y = +0.06:
    let frame_paths: Vec<_> = paths
        .iter()
        .filter(|p| p.slot == OpeningComponentSlot::Frame2D)
        .collect();
    assert_eq!(frame_paths.len(), 2);
    for fp in &frame_paths {
        let mid_y = fp.points.iter().map(|p| p.1).sum::<f64>() / (fp.points.len() as f64);
        assert!((mid_y - 0.06).abs() < 1e-3, "Frame posts must center at cross_axis_offset = +0.06");
    }

    // Sill lines should adapt so that the exterior sill front nose terminates at exterior face (-ht - 0.035)
    // and interior board front nose terminates at interior face (+ht + 0.025)
    let sill_paths: Vec<_> = paths
        .iter()
        .filter(|p| p.slot == OpeningComponentSlot::Sill2D)
        .collect();
    assert!(!sill_paths.is_empty());

    let min_y = sill_paths
        .iter()
        .flat_map(|p| p.points.iter().map(|pt| pt.1))
        .fold(f64::INFINITY, f64::min);
    let max_y = sill_paths
        .iter()
        .flat_map(|p| p.points.iter().map(|pt| pt.1))
        .fold(f64::NEG_INFINITY, f64::max);

    // Wall exterior face is at y = -0.15, nose with 0.035 overhang is at -0.185:
    assert!((min_y - (-0.15 - 0.035)).abs() < 1e-4, "Exterior sill must track true exterior wall face");
    // Wall interior face is at y = +0.15, board with 0.025 overhang is at +0.175:
    assert!((max_y - (0.15 + 0.025)).abs() < 1e-4, "Interior board must track true interior wall face");
}

#[test]
fn test_opening_xdata_roundtrip_reference_side_and_offset() {
    use crate::modules::aec::engine::opening_style::HingeSide;
    use crate::modules::aec::engine::opening_xdata::{opening_from_values, opening_record};
    use crate::modules::aec::engine::openings::{NicheSide, Opening, OpeningReferenceSide};
    use acadrust::Handle;

    let mut win = Opening::window(Handle::new(101), Handle::new(202), 4.5);
    win.reference_side = OpeningReferenceSide::Start;
    win.cross_axis_offset = 0.08;
    win.depth = Some(0.12);
    win.niche_side = NicheSide::Interior;
    win.hinge = HingeSide::Right;

    let record = opening_record(&win);
    let parsed = opening_from_values(win.handle, &record.values).expect("parse opening record");

    assert_eq!(parsed.handle, win.handle);
    assert_eq!(parsed.host_wall, win.host_wall);
    assert!((parsed.distance_along_axis - win.distance_along_axis).abs() < 1e-9);
    assert_eq!(parsed.reference_side, OpeningReferenceSide::Start);
    assert!((parsed.cross_axis_offset - 0.08).abs() < 1e-9);
    assert_eq!(parsed.depth, Some(0.12));
    assert_eq!(parsed.niche_side, NicheSide::Interior);
    assert_eq!(parsed.hinge, HingeSide::Right);

    // Test legacy record with only 11 entries (missing the 4 new fields)
    let legacy_values = record.values[..11].to_vec();
    let legacy_parsed = opening_from_values(win.handle, &legacy_values).expect("parse legacy record");
    assert_eq!(legacy_parsed.reference_side, OpeningReferenceSide::Center);
    assert_eq!(legacy_parsed.cross_axis_offset, 0.0);
    assert_eq!(legacy_parsed.depth, None);
    assert_eq!(legacy_parsed.niche_side, NicheSide::Exterior);
}

#[test]
fn test_opening_grips_definition_and_flip_handle() {
    use crate::modules::aec::engine::opening_display::{
        apply_opening_axis_grip, opening_axis_grips,
    };
    use crate::modules::aec::engine::opening_style::HingeSide;
    use crate::modules::aec::engine::openings::{Opening, OpeningReferenceSide};
    use acadrust::Handle;
    use glam::DVec3;

    let axis = vec![(0.0, 0.0), (10.0, 0.0)];
    let mut win = Opening::window(Handle::new(1), Handle::new(2), 4.0);
    win.width = 1.2;
    win.reference_side = OpeningReferenceSide::Start;
    win.hinge = HingeSide::Left;

    let grips = opening_axis_grips(&axis, &win, 0.0);
    assert_eq!(grips.len(), 4, "Must have 4 grips: Center, Start, End, Flip");
    assert_eq!(grips[0].id, 0);
    assert_eq!(grips[1].id, 1);
    assert_eq!(grips[2].id, 2);
    assert_eq!(grips[3].id, 3);

    // Center grip is at s = 4.0 + 0.6 = 4.6
    assert!((grips[0].world.x - 4.6).abs() < 1e-6);
    // Start grip is at s = 4.0
    assert!((grips[1].world.x - 4.0).abs() < 1e-6);
    // End grip is at s = 5.2
    assert!((grips[2].world.x - 5.2).abs() < 1e-6);

    // 1. Move position along axis using Grip 0 to x = 6.0
    apply_opening_axis_grip(&axis, &mut win, 0, DVec3::new(6.0, 0.0, 0.0));
    // Since reference_side is Start, start reveal moves so center is at 6.0: start = 6.0 - 0.6 = 5.4
    assert!((win.distance_along_axis - 5.4).abs() < 1e-6);
    assert!((win.center_along_axis() - 6.0).abs() < 1e-6);

    // 2. Resize start jamb using Grip 1 to x = 5.0
    apply_opening_axis_grip(&axis, &mut win, 1, DVec3::new(5.0, 0.0, 0.0));
    // End was at 6.6, new start is at 5.0 -> width becomes 1.6, start is at 5.0
    assert!((win.width - 1.6).abs() < 1e-6);
    assert!((win.distance_along_axis - 5.0).abs() < 1e-6);

    // 3. Flip using Grip 3
    apply_opening_axis_grip(&axis, &mut win, 3, DVec3::ZERO);
    assert_eq!(win.reference_side, OpeningReferenceSide::End);
    assert_eq!(win.hinge, HingeSide::Right);
}

#[test]
fn test_opening_grip_simulation_preserves_handles() {
    use crate::modules::aec::engine::opening_display::{
        apply_opening_axis_grip, collect_opening_display_children, commit_opening_instance,
    };
    use crate::modules::aec::engine::opening_xdata::{
        openings_for_host_wall, place_wall_opening, write_opening_instance,
    };
    use crate::modules::aec::engine::openings::OpeningKind;
    use crate::modules::aec::engine::wall::WallJustification;
    use crate::modules::aec::engine::wall_package::resolve_wall_package;
    use crate::modules::aec::engine::wall_regen::regenerate_wall_representation;
    use crate::modules::aec::engine::xdata::{wall_record, AEC_APPID};
    use crate::scene::Scene;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::xdata::ExtendedDataRecord;
    use acadrust::EntityType;
    use glam::DVec3;

    let mut scene = Scene::new();
    let layers = vec![wl("Brick", 0.3, "Structural")];
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, crate::modules::aec::engine::plan_view::PlanPhase::New, None);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(4.0, 0.0, 0.0),
        OpeningKind::Window,
        None,
        None,
        None,
    ).expect("place window");

    regenerate_wall_representation(&mut scene, wall_h, None).expect("regen wall");

    let children_before = collect_opening_display_children(&scene, win_h);
    assert!(!children_before.is_empty(), "Window must have display children");

    let axis = vec![(0.0, 0.0), (10.0, 0.0)];
    let mut opening = openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("opening exists");

    // Simulate mouse-drag: 10 intermediate mouse move events
    for step in 1..=10 {
        let new_x = 4.0 + (step as f64) * 0.1;
        apply_opening_axis_grip(&axis, &mut opening, 0, DVec3::new(new_x, 0.0, 0.0));
        write_opening_instance(&mut scene, &opening);

        // Verify that throughout the drag, the window handle win_h and its children handles remain valid!
        assert!(scene.document.get_entity(win_h).is_some(), "Opening entity handle must stay alive during drag");
        for child_h in &children_before {
            assert!(scene.document.get_entity(*child_h).is_some(), "Child entity handle must stay alive during drag");
        }
    }

    // Now simulate mouse release: commit
    commit_opening_instance(&mut scene, &opening, None, None);

    let wall_pkg = resolve_wall_package(&scene, wall_h);
    regenerate_wall_representation(&mut scene, wall_pkg, None).expect("regen wall on commit");

    let final_opening = openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("opening exists after commit");
    assert!((final_opening.distance_along_axis - 5.0).abs() < 1e-6, "Final position must be committed at 5.0");
}

#[test]
fn test_niche_2d_preserves_single_continuous_wall_with_correct_rest_thickness() {
    use crate::modules::aec::engine::openings::{
        subtract_openings_from_band, NicheSide, Opening,
    };
    use acadrust::Handle;

    let axis = vec![(0.0, 0.0), (10.0, 0.0)];
    let total_thickness = 0.30; // 30cm wall

    let mut niche_ext = Opening::breakthrough(Handle::new(1), Handle::new(2), 5.0);
    niche_ext.width = 1.2;
    niche_ext.height = 1.0;
    niche_ext.sill_height = 0.8;
    niche_ext.depth = Some(0.15); // 15cm deep niche in 30cm wall
    niche_ext.niche_side = NicheSide::Exterior;

    let pieces = subtract_openings_from_band(&[], &axis, total_thickness, &[niche_ext.clone()]);
    assert_eq!(
        pieces.len(),
        1,
        "Partial-depth niche MUST NOT split the wall into separate pieces"
    );

    let poly = &pieces[0];
    assert!(poly.len() >= 6, "Indented polygon must have extra vertices for the niche indentation");

    // In the niche span [4.4, 5.6], the exterior edge is indented to y = -0.15 + 0.15 = 0.0,
    // while the interior edge remains at y = +0.15.
    // The wall thickness across the niche is 0.15 - 0.0 = 0.15 m!
    let niche_y_at_reveal: Vec<f64> = poly
        .iter()
        .filter(|p| (p.0 - 4.4).abs() < 1e-4 || (p.0 - 5.6).abs() < 1e-4)
        .map(|p| p.1)
        .collect();

    assert!(
        niche_y_at_reveal.iter().any(|&y| (y - 0.0).abs() < 1e-4),
        "Niche reveal must have back-face vertex indented to y = 0.0"
    );
    assert!(
        niche_y_at_reveal.iter().any(|&y| (y - (-0.15)).abs() < 1e-4),
        "Niche reveal must have corner vertex at original wall face y = -0.15"
    );

    // Interior face across the wall stays at y = +0.15:
    let max_y_at_niche = poly
        .iter()
        .filter(|p| (p.0 - 4.4).abs() < 1e-4 || (p.0 - 5.6).abs() < 1e-4)
        .map(|p| p.1)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!((max_y_at_niche - 0.0).abs() < 1e-4 || (max_y_at_niche - 0.15).abs() < 1e-4);

    // Remaining thickness at the niche is 0.15 - 0.0 = 0.15:
    let interior_y = poly.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    assert!((interior_y - 0.15).abs() < 1e-4);
    let niche_back_y = 0.0;
    let rest_thickness = interior_y - niche_back_y;
    assert!((rest_thickness - 0.15).abs() < 1e-4, "Rest wall thickness must be exactly 15cm");

    // Outside the niche span (e.g. x < 4.0 or x > 6.0), the wall thickness remains 0.30 m:
    let min_y_rest = poly
        .iter()
        .filter(|p| p.0 < 4.0)
        .map(|p| p.1)
        .fold(f64::INFINITY, f64::min);
    assert!((min_y_rest - (-0.15)).abs() < 1e-4, "Undisturbed exterior face must be at y = -0.15");

    // Interior niche test:
    let mut niche_int = niche_ext.clone();
    niche_int.niche_side = NicheSide::Interior;
    let pieces_int = subtract_openings_from_band(&[], &axis, total_thickness, &[niche_int]);
    assert_eq!(pieces_int.len(), 1);
    let poly_int = &pieces_int[0];
    let int_y_at_reveal: Vec<f64> = poly_int
        .iter()
        .filter(|p| (p.0 - 4.4).abs() < 1e-4 || (p.0 - 5.6).abs() < 1e-4)
        .map(|p| p.1)
        .collect();
    // Interior face indents from 0.15 down to 0.0:
    assert!(
        int_y_at_reveal.iter().any(|&y| (y - 0.0).abs() < 1e-4),
        "Interior niche must indent to y = 0.0"
    );
    assert!(
        int_y_at_reveal.iter().any(|&y| (y - 0.15).abs() < 1e-4),
        "Interior niche must have corner at original interior face y = 0.15"
    );
}

#[test]
fn test_niche_3d_preserves_solid_back_wall() {
    use crate::modules::aec::engine::elevation_cut::zone_solid_paths;
    use crate::modules::aec::engine::opening_display::format_din1356_label;
    use crate::modules::aec::engine::openings::{NicheSide, Opening};
    use acadrust::Handle;

    let axis = vec![(0.0, 0.0), (10.0, 0.0)];
    let layers = vec![(0.30, -0.15)]; // thickness 0.30, axis_offset -0.15
    let layer_ext = vec![(2.80, 0.0)]; // height 2.80, base 0.0

    let mut niche = Opening::breakthrough(Handle::new(1), Handle::new(2), 5.0);
    niche.width = 1.20;
    niche.height = 1.00;
    niche.sill_height = 0.80;
    niche.depth = Some(0.12); // 12cm deep niche
    niche.niche_side = NicheSide::Exterior;

    let z_paths = zone_solid_paths(&axis, &layers, &layer_ext, &[niche.clone()]);
    // Must contain: sill solid, head solid, AND the back-wall solid!
    assert!(z_paths.len() >= 3, "Zone solids must include sill, head, and niche back wall");

    // The back-wall solid has thickness 0.30 - 0.12 = 0.18:
    let back_wall = z_paths
        .iter()
        .find(|zp| (zp.direction[1].abs() - 0.18).abs() < 1e-4)
        .expect("Must have 3D back-wall solid with thickness 18cm");

    let z_min = back_wall.loop_xyz.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
    let z_max = back_wall.loop_xyz.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max);
    assert!((z_min - 0.80).abs() < 1e-4, "Back-wall solid z_min must match niche sill 0.80");
    assert!((z_max - 1.80).abs() < 1e-4, "Back-wall solid z_max must match niche head 1.80");

    // Test label formatting with 3 dimensions:
    let label = format_din1356_label(&niche);
    assert_eq!(label, "1.20 / 1.00 / 0.12\\PUK 0.80", "Niche label must display width / height / depth and UK");
}

#[test]
fn test_elevation_generators_contour_muntins_and_triangles() {
    use crate::modules::aec::engine::display_component::OpeningComponentSlot;
    use crate::modules::aec::engine::opening_display::{bake_opening_generators, OpeningBakeParams};
    use crate::modules::aec::engine::opening_shape::OpeningShape;
    use crate::modules::aec::engine::opening_style::{
        HingeSide, OpeningGenerator, SlotGeometry, DEFAULT_FRAME_THICKNESS,
    };
    use crate::modules::aec::engine::openings::OpeningKind;
    use std::collections::HashMap;

    let mut slots = HashMap::new();
    slots.insert(
        OpeningComponentSlot::ElevationContour2D,
        SlotGeometry::Generator(OpeningGenerator::ElevationFrameRect),
    );
    slots.insert(
        OpeningComponentSlot::ElevationMuntins2D,
        SlotGeometry::Generator(OpeningGenerator::ElevationMuntinsDouble),
    );
    slots.insert(
        OpeningComponentSlot::ElevationSwing2D,
        SlotGeometry::Generator(OpeningGenerator::ElevationSwingTriangle),
    );
    slots.insert(
        OpeningComponentSlot::ElevationSill2D,
        SlotGeometry::Generator(OpeningGenerator::ElevationSillLine),
    );

    let p_left = OpeningBakeParams {
        width: 1.20,
        height: 1.40,
        sill_height: 0.90,
        thickness: 0.30,
        frame_thickness: DEFAULT_FRAME_THICKNESS,
        cross_axis_offset: 0.0,
        hinge: HingeSide::Left,
        shape: OpeningShape::Rectangle,
        spring_height: 0.0,
        opening_angle_deg: 90.0,
        kind: OpeningKind::Window,
    };

    let baked_left = bake_opening_generators(&slots, p_left, None);

    // 1. ElevationContour2D: outer frame + inner frame
    let contours: Vec<_> = baked_left
        .iter()
        .filter(|p| p.slot == OpeningComponentSlot::ElevationContour2D)
        .collect();
    assert_eq!(contours.len(), 2, "ElevationFrameRect must emit outer and inner frame loops");

    // 2. ElevationMuntins2D: mullion + left leaf + right leaf
    let muntins: Vec<_> = baked_left
        .iter()
        .filter(|p| p.slot == OpeningComponentSlot::ElevationMuntins2D)
        .collect();
    assert_eq!(muntins.len(), 3, "ElevationMuntinsDouble must emit central mullion and two leaves");

    // 3. ElevationSwing2D: DIN opening triangle pointing to left hinge
    let swing_left: Vec<_> = baked_left
        .iter()
        .filter(|p| p.slot == OpeningComponentSlot::ElevationSwing2D)
        .collect();
    assert_eq!(swing_left.len(), 1);
    let pts_left = &swing_left[0].points;
    assert_eq!(pts_left.len(), 3);
    // Tip of triangle is the middle vertex (index 1) and must be on the left (negative u):
    assert!(pts_left[1].0 < 0.0, "Left-hinged triangle tip must point to left jamb");
    assert!(pts_left[0].0 > 0.0, "Left-hinged triangle base start must be on right side");
    assert!(pts_left[2].0 > 0.0, "Left-hinged triangle base end must be on right side");

    // Right-hinged test:
    let mut p_right = p_left;
    p_right.hinge = HingeSide::Right;
    let baked_right = bake_opening_generators(&slots, p_right, None);
    let swing_right: Vec<_> = baked_right
        .iter()
        .filter(|p| p.slot == OpeningComponentSlot::ElevationSwing2D)
        .collect();
    let pts_right = &swing_right[0].points;
    // Tip of triangle is the middle vertex (index 1) and must be on the right (positive u):
    assert!(pts_right[1].0 > 0.0, "Right-hinged triangle tip must point to right jamb");
    assert!(pts_right[0].0 < 0.0, "Right-hinged triangle base start must be on left side");
    assert!(pts_right[2].0 < 0.0, "Right-hinged triangle base end must be on left side");

    // 4. ElevationSill2D:
    let sill_elev: Vec<_> = baked_left
        .iter()
        .filter(|p| p.slot == OpeningComponentSlot::ElevationSill2D)
        .collect();
    assert_eq!(sill_elev.len(), 2, "ElevationSillLine must emit top line and drop nose");

    // 5. Arch shape test:
    let mut slots_arch = HashMap::new();
    slots_arch.insert(
        OpeningComponentSlot::ElevationContour2D,
        SlotGeometry::Generator(OpeningGenerator::ElevationFrameArch),
    );
    let mut p_arch = p_left;
    p_arch.shape = OpeningShape::Arch;
    p_arch.spring_height = 0.80;
    let baked_arch = bake_opening_generators(&slots_arch, p_arch, None);
    let arch_contour = &baked_arch[0];
    assert!(arch_contour.points.len() > 10, "Arch contour must tessellate circular curved top");
    let max_z = arch_contour.points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    assert!((max_z - 1.40).abs() < 1e-4, "Arch top must reach total height 1.40");
}

#[test]
fn test_elevation_components_world_regeneration_creates_3d_polylines() {
    use crate::modules::aec::engine::display_component::OpeningComponentSlot;
    use crate::modules::aec::engine::opening_display::collect_opening_display_children;
    use crate::modules::aec::engine::opening_style::{OpeningGenerator, SlotGeometry};
    use crate::modules::aec::engine::opening_xdata::{
        openings_for_host_wall, place_wall_opening, write_opening_instance,
    };
    use crate::modules::aec::engine::openings::OpeningKind;
    use crate::modules::aec::engine::wall::WallJustification;
    use crate::modules::aec::engine::wall_regen::regenerate_wall_representation;
    use crate::modules::aec::engine::xdata::{wall_record, AEC_APPID};
    use crate::scene::Scene;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::xdata::ExtendedDataRecord;
    use acadrust::EntityType;
    use glam::DVec3;

    let mut scene = Scene::new();
    let layers = vec![wl("Concrete", 0.2, "Structural")];
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, crate::modules::aec::engine::plan_view::PlanPhase::New, None);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(5.0, 0.0, 0.0),
        OpeningKind::Window,
        None,
        None,
        None,
    ).expect("place window");

    // Configure elevation slots on the opening:
    let mut opening = openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("opening found");

    let mut style = crate::modules::aec::engine::opening_style::OpeningStyle::standard_window();
    style.slots.insert(
        OpeningComponentSlot::ElevationContour2D,
        SlotGeometry::Generator(OpeningGenerator::ElevationFrameRect),
    );
    style.slots.insert(
        OpeningComponentSlot::ElevationSwing2D,
        SlotGeometry::Generator(OpeningGenerator::ElevationSwingTriangle),
    );
    style.slots.insert(
        OpeningComponentSlot::ElevationSill2D,
        SlotGeometry::Generator(OpeningGenerator::ElevationSillLine),
    );

    let mut lib = crate::modules::aec::engine::library::StyleLibrary::default();
    lib.opening_styles.push(style.clone());
    opening.style_id = Some(style.style.id.clone());
    write_opening_instance(&mut scene, &opening);

    regenerate_wall_representation(&mut scene, wall_h, Some(&lib)).expect("regen wall");

    let children = collect_opening_display_children(&scene, win_h);
    assert!(!children.is_empty());

    // Verify that Polyline3D entities are created in the façade plane for elevation components:
    let polylines3d: Vec<_> = children
        .iter()
        .filter_map(|h| match scene.document.get_entity(*h) {
            Some(EntityType::Polyline3D(p3d)) => Some(p3d.clone()),
            _ => None,
        })
        .collect();

    assert!(!polylines3d.is_empty(), "Façade elevation slots must generate Polyline3D entities");

    // Verify Z-coordinates match sill (0.90) and height (1.20 -> head at 2.10):
    let all_z: Vec<f64> = polylines3d
        .iter()
        .flat_map(|p| p.vertices.iter().map(|v| v.position.z))
        .collect();

    let min_z = all_z.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    let max_z = all_z.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));

    // Elevation sill drop goes to sill - 0.05 = 0.85:
    assert!((min_z - 0.85).abs() < 1e-4, "Min Z must match elevation sill drop (0.85)");
    assert!((max_z - (0.90 + 1.20)).abs() < 1e-4, "Max Z must match window head (2.10)");
}

#[test]
fn test_parametric_3d_solids_for_openings() {
    use crate::modules::aec::engine::opening_display::{
        build_opening_frame_3d, build_opening_glazing_3d, build_opening_leaf_3d,
    };
    use crate::modules::aec::engine::opening_style::HingeSide;

    let width = 1.20;
    let height = 1.40;
    let ft = 0.07;
    let depth = 0.08;

    // 1. Frame3D
    let frame_body = build_opening_frame_3d(width, height, ft, depth).expect("build frame 3d");
    assert!(frame_body.validate().is_empty(), "Frame B-Rep body must be topologically valid");
    let (min_f, max_f) = crate::scene::model::solid_model::extent(&frame_body).expect("frame extent");
    assert!((max_f[0] - min_f[0] - width).abs() < 1e-3, "Frame width must match 1.20");
    assert!((max_f[2] - min_f[2] - height).abs() < 1e-3, "Frame height must match 1.40");
    assert!((max_f[1] - min_f[1] - depth).abs() < 1e-3, "Frame depth must match 0.08");

    // 2. Leaf3D
    let leaf_body = build_opening_leaf_3d(width, height, ft, 45.0, HingeSide::Left, false)
        .expect("build leaf 3d");
    assert!(leaf_body.validate().is_empty(), "Leaf B-Rep body must be topologically valid");

    // 3. Glazing3D
    let glass_body = build_opening_glazing_3d(width, height, ft).expect("build glass 3d");
    assert!(glass_body.validate().is_empty(), "Glass B-Rep body must be topologically valid");
    let (min_g, max_g) = crate::scene::model::solid_model::extent(&glass_body).expect("glass extent");
    assert!((max_g[1] - min_g[1] - 0.02).abs() < 1e-3, "Glass pane thickness must be 0.02m (2cm)");
}

#[test]
fn test_2d_3d_representation_filtering() {
    use crate::modules::aec::engine::display_component::{OpeningComponentSlot, RepresentationMode};
    use crate::modules::aec::engine::library::build_effective_rule_set;
    use crate::modules::aec::engine::plan_view::DisplayConfig;

    let cfg = DisplayConfig::new(
        "Standard".to_string(),
        "Architektur".to_string(),
        crate::modules::aec::engine::plan_view::PlanningStage::Design,
        crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
    );

    // 1. TwoD Mode: all 3D slots must be false
    let rules_2d = build_effective_rule_set(&cfg, None, Some(RepresentationMode::TwoD));
    assert!(rules_2d.is_opening_visible(OpeningComponentSlot::Frame2D));
    assert!(rules_2d.is_opening_visible(OpeningComponentSlot::Glazing2D));
    assert!(!rules_2d.is_opening_visible(OpeningComponentSlot::Frame3D), "Frame3D must be hidden in 2D mode");
    assert!(!rules_2d.is_opening_visible(OpeningComponentSlot::Leaf3D), "Leaf3D must be hidden in 2D mode");
    assert!(!rules_2d.is_opening_visible(OpeningComponentSlot::Glazing3D), "Glazing3D must be hidden in 2D mode");

    // 2. ThreeD Mode: all 2D plan and elevation slots must be false
    let rules_3d = build_effective_rule_set(&cfg, None, Some(RepresentationMode::ThreeD));
    assert!(!rules_3d.is_opening_visible(OpeningComponentSlot::Frame2D), "Frame2D must be hidden in 3D mode");
    assert!(!rules_3d.is_opening_visible(OpeningComponentSlot::Swing2D), "Swing2D must be hidden in 3D mode");
    assert!(!rules_3d.is_opening_visible(OpeningComponentSlot::Sill2D), "Sill2D must be hidden in 3D mode");
    assert!(!rules_3d.is_opening_visible(OpeningComponentSlot::ElevationContour2D), "ElevationContour must be hidden in 3D mode");
    assert!(rules_3d.is_opening_visible(OpeningComponentSlot::Frame3D), "Frame3D must be visible in 3D mode");
    assert!(rules_3d.is_opening_visible(OpeningComponentSlot::Leaf3D), "Leaf3D must be visible in 3D mode");
    assert!(rules_3d.is_opening_visible(OpeningComponentSlot::Glazing3D), "Glazing3D must be visible in 3D mode");
}

#[test]
fn test_discipline_templates_slot_visibilities() {
    use crate::modules::aec::engine::display_component::OpeningComponentSlot;
    use crate::modules::aec::engine::library::{build_effective_rule_set, DisplayConfigLibrary};

    let seed_lib = DisplayConfigLibrary::seed_default_configs();

    // 1. Architektur 1:50
    let arch_50 = seed_lib.find("Architektur 1:50").expect("find Architektur 1:50");
    let rules_arch = build_effective_rule_set(arch_50, None, None);
    assert!(rules_arch.is_opening_visible(OpeningComponentSlot::HostCut2D));
    assert!(rules_arch.is_opening_visible(OpeningComponentSlot::Frame2D));
    assert!(rules_arch.is_opening_visible(OpeningComponentSlot::Leaf2D));
    assert!(rules_arch.is_opening_visible(OpeningComponentSlot::Swing2D));
    assert!(rules_arch.is_opening_visible(OpeningComponentSlot::Glazing2D));
    assert!(rules_arch.is_opening_visible(OpeningComponentSlot::Sill2D));
    assert!(rules_arch.is_opening_visible(OpeningComponentSlot::Threshold2D));
    assert!(rules_arch.is_opening_visible(OpeningComponentSlot::OpeningLabel2D));

    // 2. Statik 1:50: only HostCut2D and OpeningLabel2D are visible, all architectural symbols hidden!
    let stat_50 = seed_lib.find("Statik 1:50").expect("find Statik 1:50");
    let rules_stat = build_effective_rule_set(stat_50, None, None);
    assert!(rules_stat.is_opening_visible(OpeningComponentSlot::HostCut2D));
    assert!(rules_stat.is_opening_visible(OpeningComponentSlot::OpeningLabel2D));
    assert!(!rules_stat.is_opening_visible(OpeningComponentSlot::Frame2D), "Statik must hide Frame2D");
    assert!(!rules_stat.is_opening_visible(OpeningComponentSlot::Leaf2D), "Statik must hide Leaf2D");
    assert!(!rules_stat.is_opening_visible(OpeningComponentSlot::Swing2D), "Statik must hide Swing2D");
    assert!(!rules_stat.is_opening_visible(OpeningComponentSlot::Glazing2D), "Statik must hide Glazing2D");
    assert!(!rules_stat.is_opening_visible(OpeningComponentSlot::Sill2D), "Statik must hide Sill2D");
    assert!(!rules_stat.is_opening_visible(OpeningComponentSlot::Threshold2D), "Statik must hide Threshold2D");

    // 3. Entwurf 1:100: Glazing2D and Threshold2D inactive
    let entw_100 = seed_lib.find("Entwurf 1:100").expect("find Entwurf 1:100");
    let rules_entw = build_effective_rule_set(entw_100, None, None);
    assert!(rules_entw.is_opening_visible(OpeningComponentSlot::Frame2D));
    assert!(rules_entw.is_opening_visible(OpeningComponentSlot::Leaf2D));
    assert!(rules_entw.is_opening_visible(OpeningComponentSlot::Swing2D));
    assert!(!rules_entw.is_opening_visible(OpeningComponentSlot::Glazing2D), "Entwurf 1:100 hides Glazing2D");
    assert!(!rules_entw.is_opening_visible(OpeningComponentSlot::Threshold2D), "Entwurf 1:100 hides Threshold2D");

    // 4. Ansicht Fassade: all elevation slots active, floor plan slots inactive
    let elev = seed_lib.find("Ansicht Fassade").expect("find Ansicht Fassade");
    let rules_elev = build_effective_rule_set(elev, None, None);
    assert!(rules_elev.is_opening_visible(OpeningComponentSlot::ElevationContour2D));
    assert!(rules_elev.is_opening_visible(OpeningComponentSlot::ElevationMuntins2D));
    assert!(rules_elev.is_opening_visible(OpeningComponentSlot::ElevationSwing2D));
    assert!(rules_elev.is_opening_visible(OpeningComponentSlot::ElevationSill2D));
    assert!(!rules_elev.is_opening_visible(OpeningComponentSlot::Frame2D));
    assert!(!rules_elev.is_opening_visible(OpeningComponentSlot::Leaf2D));
    assert!(!rules_elev.is_opening_visible(OpeningComponentSlot::Swing2D));
}

#[test]
fn test_wall_base_control_plane_elevation_alignment() {
    use crate::modules::aec::engine::opening_display::{
        collect_opening_display_children, host_base_z, opening_axis_grips,
    };
    use crate::modules::aec::engine::opening_xdata::{openings_for_host_wall, place_wall_opening};
    use crate::modules::aec::engine::openings::OpeningKind;
    use crate::modules::aec::engine::project::{Building, ProjectFile, StoreyRef};
    use crate::modules::aec::engine::wall::Wall;
    use crate::modules::aec::engine::wall_package::resolve_wall_package;
    use crate::modules::aec::engine::wall_regen::regenerate_wall_representation;
    use crate::modules::aec::engine::xdata::AEC_APPID;
    use crate::scene::Scene;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::xdata::ExtendedDataRecord;
    use acadrust::EntityType;
    use glam::DVec3;

    let mut scene = Scene::new();
    let storey_elevation = 2.80;
    let storey_height = 3.00;
    let storey = StoreyRef::new_with_height("OG1", storey_elevation, storey_height, "og1.dwg");
    let floor_plane_id = storey.floor_plane_id;
    let ceiling_plane_id = storey.ceiling_plane_id;

    let mut project = ProjectFile::default();
    let mut building = Building::new("Haus A");
    building.storeys.push(storey);
    project.buildings.push(building);

    let layers = vec![wl("Beton", 0.24, "Structural")];
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(8.0, 0.0)));
    pl.elevation = storey_elevation;

    let mut wall = Wall::new("Standard", storey_height, 0);
    wall.layers = layers.clone();
    wall.base_plane_id = Some(floor_plane_id);
    wall.top_plane_id = Some(ceiling_plane_id);
    wall.base_origin = [0.0, 0.0, storey_elevation];
    wall.base_normal = [0.0, 0.0, 1.0];
    wall.top_origin = [0.0, 0.0, storey_elevation + storey_height];
    wall.top_normal = [0.0, 0.0, 1.0];

    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    // Regenerate wall
    let touched = regenerate_wall_representation(&mut scene, wall_h, None).expect("regen wall");

    // 1. Check Wall Axis elevation
    let wall_ent = scene.document.get_entity(wall_h).expect("wall entity");
    if let EntityType::LwPolyline(pl) = wall_ent {
        assert!(
            (pl.elevation - storey_elevation).abs() < 1e-4,
            "Axis polyline elevation must be {storey_elevation}, got {}",
            pl.elevation
        );
    } else {
        panic!("Wall must be LwPolyline");
    }

    // 2. Check 2D Contours elevation
    for h in &touched {
        if *h == wall_h {
            continue;
        }
        if let Some(EntityType::LwPolyline(c_pl)) = scene.document.get_entity(*h) {
            assert!(
                (c_pl.elevation - storey_elevation).abs() < 1e-4,
                "2D contour polyline elevation must be {storey_elevation}, got {}",
                c_pl.elevation
            );
        }
    }

    // 3. Check 3D Solids Z extents
    let mut solid_found = false;
    for (handle, body) in &scene.solid_models {
        if touched.contains(handle) {
            let (min, max) = crate::scene::model::solid_model::extent(body).expect("extent");
            assert!(
                (min[2] - storey_elevation).abs() < 1e-3,
                "Solid min Z must start at {storey_elevation}, got {}",
                min[2]
            );
            assert!(
                (max[2] - (storey_elevation + storey_height)).abs() < 1e-3,
                "Solid max Z must reach {}, got {}",
                storey_elevation + storey_height,
                max[2]
            );
            solid_found = true;
        }
    }
    assert!(solid_found, "Must have generated 3D wall solids");

    // 4. Place a Window on the wall
    let sill_height = 0.90;
    let win_height = 1.30;
    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(3.0, 0.0, 0.0),
        OpeningKind::Window,
        None,
        None,
        None,
    )
    .expect("place window");

    // Configure window dimensions
    let mut opening = openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("window opening");
    opening.sill_height = sill_height;
    opening.height = win_height;
    opening.width = 1.20;
    crate::modules::aec::engine::opening_xdata::write_opening_instance(&mut scene, &opening);

    // Regenerate opening
    let wall_pkg = resolve_wall_package(&scene, wall_h);
    regenerate_wall_representation(&mut scene, wall_pkg, None).expect("regen wall with window");

    // 5. Verify Window POINT location Z matches wall base
    if let Some(EntityType::Point(pt)) = scene.document.get_entity(win_h) {
        assert!(
            (pt.location.z - storey_elevation).abs() < 1e-4,
            "Opening point Z must match wall base {storey_elevation}, got {}",
            pt.location.z
        );
    } else {
        panic!("Opening entity must be a POINT");
    }

    // 6. Verify Window 2D plan children elevation
    let children = collect_opening_display_children(&scene, win_h);
    assert!(!children.is_empty(), "Window must have display children");
    for child_h in &children {
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*child_h) {
            assert!(
                (pl.elevation - storey_elevation).abs() < 1e-4,
                "Window 2D polyline elevation must match wall base {storey_elevation}, got {}",
                pl.elevation
            );
        } else if let Some(EntityType::MText(mtext)) = scene.document.get_entity(*child_h) {
            assert!(
                (mtext.insertion_point.z - storey_elevation).abs() < 1e-4,
                "Window label MText insertion point Z must match wall base {storey_elevation}, got {}",
                mtext.insertion_point.z
            );
        }
    }

    // 7. Verify Window 3D solid models have their base at wall_base_z + sill_height
    let mut win_solids_found = 0;
    let expected_sill_z = storey_elevation + sill_height;
    let expected_head_z = expected_sill_z + win_height;
    let mut overall_min_z = f64::INFINITY;
    let mut overall_max_z = f64::NEG_INFINITY;
    for (handle, body) in &scene.solid_models {
        if children.contains(handle) {
            let (min, max) = crate::scene::model::solid_model::extent(body).expect("extent");
            overall_min_z = overall_min_z.min(min[2]);
            overall_max_z = overall_max_z.max(max[2]);
            win_solids_found += 1;
        }
    }
    assert!(win_solids_found >= 2, "Must have generated window 3D frame and glazing solids");
    assert!(
        (overall_min_z - expected_sill_z).abs() < 1e-2,
        "Window 3D overall min Z must be at {expected_sill_z}, got {overall_min_z}"
    );
    assert!(
        (overall_max_z - expected_head_z).abs() < 1e-2,
        "Window 3D overall max Z must reach {expected_head_z}, got {overall_max_z}"
    );

    // 8. Verify Window Grips elevation
    let axis = vec![(0.0, 0.0), (8.0, 0.0)];
    let grips = opening_axis_grips(&axis, &opening, host_base_z(&scene, wall_pkg));
    assert_eq!(grips.len(), 4);
    for grip in &grips {
        assert!(
            (grip.world.z - storey_elevation).abs() < 1e-4,
            "Grip Z must match wall base {storey_elevation}, got {}",
            grip.world.z
        );
    }
}

#[test]
fn test_wall_and_opening_track_control_plane_vertical_movement() {
    use crate::modules::aec::engine::opening_display::{
        collect_opening_display_children, host_base_z, opening_axis_grips,
    };
    use crate::modules::aec::engine::opening_xdata::{openings_for_host_wall, place_wall_opening};
    use crate::modules::aec::engine::openings::OpeningKind;
    use crate::modules::aec::engine::project::{Building, ProjectFile, StoreyRef};
    use crate::modules::aec::engine::wall::Wall;
    use crate::modules::aec::engine::wall_package::resolve_wall_package;
    use crate::modules::aec::engine::wall_regen::regenerate_wall_representation;
    use crate::modules::aec::engine::xdata::AEC_APPID;
    use crate::scene::Scene;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::xdata::ExtendedDataRecord;
    use acadrust::EntityType;
    use glam::DVec3;

    let mut scene = Scene::new();
    let initial_elevation = 0.0;
    let new_elevation = 3.50;
    let storey_height = 2.80;

    let storey = StoreyRef::new_with_height("EG", initial_elevation, storey_height, "eg.dwg");
    let floor_plane_id = storey.floor_plane_id;
    let ceiling_plane_id = storey.ceiling_plane_id;

    let mut project = ProjectFile::default();
    let mut building = Building::new("Gebaeude 1");
    building.storeys.push(storey);
    project.buildings.push(building);

    let layers = vec![wl("Kalksandstein", 0.20, "Structural")];
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(6.0, 0.0)));
    pl.elevation = initial_elevation;

    let mut wall = Wall::new("Standard", storey_height, 0);
    wall.layers = layers.clone();
    wall.base_plane_id = Some(floor_plane_id);
    wall.top_plane_id = Some(ceiling_plane_id);
    wall.base_origin = [0.0, 0.0, initial_elevation];
    wall.top_origin = [0.0, 0.0, initial_elevation + storey_height];

    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    // Initial regen
    let _ = regenerate_wall_representation(&mut scene, wall_h, None);

    // Place window
    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(2.5, 0.0, 0.0),
        OpeningKind::Window,
        None,
        None,
        None,
    )
    .expect("place window");

    let mut opening = openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("window");
    opening.sill_height = 0.85;
    opening.height = 1.25;
    opening.width = 1.00;
    crate::modules::aec::engine::opening_xdata::write_opening_instance(&mut scene, &opening);
    let wall_pkg = resolve_wall_package(&scene, wall_h);
    let _ = regenerate_wall_representation(&mut scene, wall_pkg, None);

    // NOW: Move the storey / control plane elevation up to 3.50m!
    let storey_mut = &mut project.buildings[0].storeys[0];
    storey_mut.elevation = new_elevation;
    if let Some(fp) = storey_mut.plane_mut(floor_plane_id) {
        fp.origin[2] = new_elevation;
    }
    if let Some(cp) = storey_mut.plane_mut(ceiling_plane_id) {
        cp.origin[2] = new_elevation + storey_height;
    }

    let project_snap = project.clone();
    let storey_mut = &mut project.buildings[0].storeys[0];
    // Apply storey elevation update to scene
    crate::modules::aec::project::storey_z::apply_storey_z_to_scene(
        &mut scene,
        storey_mut,
        Some(&project_snap),
        None,
    );

    // 1. Verify wall axis polyline elevation updated to 3.50m
    if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(wall_h) {
        assert!(
            (pl.elevation - new_elevation).abs() < 1e-4,
            "Wall axis polyline elevation must update to {new_elevation}, got {}",
            pl.elevation
        );
    }

    // 2. Verify wall 3D solids updated to start at 3.50m
    let children = collect_opening_display_children(&scene, win_h);
    let mut wall_min_z = f64::INFINITY;
    for (handle, body) in &scene.solid_models {
        if (*handle == wall_pkg || scene.document.get_entity(*handle).is_some())
            && !children.contains(handle)
        {
            let (min, _max) = crate::scene::model::solid_model::extent(body).expect("extent");
            wall_min_z = wall_min_z.min(min[2]);
        }
    }
    assert!(
        (wall_min_z - new_elevation).abs() < 1e-3,
        "Wall 3D solid min Z must start at {new_elevation}, got {wall_min_z}"
    );

    // 3. Verify window POINT location updated to 3.50m
    if let Some(EntityType::Point(pt)) = scene.document.get_entity(win_h) {
        assert!(
            (pt.location.z - new_elevation).abs() < 1e-4,
            "Window point location Z must update to {new_elevation}, got {}",
            pt.location.z
        );
    }

    // 4. Verify window 3D solids moved up to new_elevation + sill_height = 3.50 + 0.85 = 4.35m
    let children = collect_opening_display_children(&scene, win_h);
    let expected_sill_z = new_elevation + 0.85;
    let expected_head_z = expected_sill_z + 1.25;
    let mut win_solids = 0;
    let mut overall_min_z = f64::INFINITY;
    let mut overall_max_z = f64::NEG_INFINITY;
    for (handle, body) in &scene.solid_models {
        if children.contains(handle) {
            let (min, max) = crate::scene::model::solid_model::extent(body).expect("extent");
            overall_min_z = overall_min_z.min(min[2]);
            overall_max_z = overall_max_z.max(max[2]);
            win_solids += 1;
        }
    }
    assert!(win_solids >= 2, "Must find updated window 3D solids");
    assert!(
        (overall_min_z - expected_sill_z).abs() < 1e-2,
        "Window 3D overall min Z must move to {expected_sill_z}, got {overall_min_z}"
    );
    assert!(
        (overall_max_z - expected_head_z).abs() < 1e-2,
        "Window 3D overall max Z must move to {expected_head_z}, got {overall_max_z}"
    );

    // 5. Verify window grips moved up to 3.50m
    let axis = vec![(0.0, 0.0), (6.0, 0.0)];
    let grips = opening_axis_grips(&axis, &opening, host_base_z(&scene, wall_pkg));
    for grip in &grips {
        assert!(
            (grip.world.z - new_elevation).abs() < 1e-4,
            "Window grip Z must be at {new_elevation}, got {}",
            grip.world.z
        );
    }
}

#[test]
fn test_manager_save_vs_save_and_apply_decoupling() {
    use crate::modules::aec::engine::material::Material;
    use crate::modules::aec::engine::opening_display::collect_opening_display_children;
    use crate::modules::aec::engine::opening_style::{OpeningStyle, DEFAULT_FRAME_THICKNESS};
    use crate::modules::aec::engine::opening_xdata::place_wall_opening;
    use crate::modules::aec::engine::openings::OpeningKind;
    use crate::modules::aec::engine::project::{Building, ProjectFile, StoreyRef};
    use crate::modules::aec::engine::wall::Wall;
    use crate::modules::aec::engine::wall_package::resolve_wall_package;
    use crate::modules::aec::engine::wall_regen::regenerate_wall_representation;
    use crate::modules::aec::engine::xdata::AEC_APPID;
    use crate::scene::Scene;
    use acadrust::entities::{LwPolyline, LwVertex};
    use acadrust::types::Vector2;
    use acadrust::xdata::ExtendedDataRecord;
    use acadrust::EntityType;
    use glam::DVec3;

    let mut scene = Scene::new();
    let initial_elevation = 1.00;
    let storey_height = 2.80;
    let mut storey = StoreyRef::new_with_height("EG", initial_elevation, storey_height, "eg.dwg");
    let floor_plane_id = storey.floor_plane_id;
    let ceiling_plane_id = storey.ceiling_plane_id;

    let mut project = ProjectFile::default();
    let mut building = Building::new("Bau A");
    building.storeys.push(storey.clone());
    project.buildings.push(building);

    let mat_id = "mat_brick_red".to_string();
    let initial_material = Material {
        id: mat_id.clone(),
        name: "Ziegel".to_string(),
        hatch_pattern: "ANSI31".to_string(),
        line_color: 0xCC3333,
        line_type: "Continuous".to_string(),
        render_material_ref: None,
        category: Some("Mauerwerk".to_string()),
        hatch_color: None,
        hatch_scale: 1.0,
        hatch_angle: 0.0,
        hatch_angle_relative: true,
    };
    let mut library = crate::modules::aec::engine::library::StyleLibrary::empty();
    library.upsert_material(initial_material.clone());

    let mut win_style = OpeningStyle::standard_window();
    let win_style_id = "test_window_style".to_string();
    win_style.style.id = win_style_id.clone();
    win_style.frame_thickness = DEFAULT_FRAME_THICKNESS;
    library.upsert_opening_style(win_style.clone());

    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(6.0, 0.0)));
    pl.elevation = initial_elevation;

    let mut wall = Wall::new("Standard", storey_height, 0);
    wall.layers = vec![wl(&mat_id, 0.30, "Structural")];
    wall.base_plane_id = Some(floor_plane_id);
    wall.top_plane_id = Some(ceiling_plane_id);
    wall.base_origin = [0.0, 0.0, initial_elevation];
    wall.base_normal = [0.0, 0.0, 1.0];
    wall.top_origin = [0.0, 0.0, initial_elevation + storey_height];
    wall.top_normal = [0.0, 0.0, 1.0];

    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    // Initial regen
    let _ = regenerate_wall_representation(&mut scene, wall_h, Some(&library));

    // Place window
    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(2.0, 0.0, 0.0),
        OpeningKind::Window,
        Some(&library),
        None,
        None,
    )
    .expect("place window");
    let mut opening = crate::modules::aec::engine::opening_xdata::openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("window");
    opening.style_id = Some(win_style_id.clone());
    crate::modules::aec::engine::opening_xdata::write_opening_instance(&mut scene, &opening);
    let wall_pkg = resolve_wall_package(&scene, wall_h);
    let _ = regenerate_wall_representation(&mut scene, wall_pkg, Some(&library));

    // Case 1: Storey height/elevation edited, but "Save" only (no apply to scene yet).
    // The storey in-memory updates, but scene entities stay at initial_elevation!
    let new_elevation = 4.00;
    storey.set_elevation(new_elevation);
    project.buildings[0].storeys[0] = storey.clone();
    // Wall axis elevation in scene MUST STILL BE initial_elevation (1.00)
    if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(wall_h) {
        assert_eq!(pl.elevation, initial_elevation, "Wall elevation must not change before Apply");
    }

    // Now call apply to scene (Save and Apply):
    let project_snap = project.clone();
    crate::modules::aec::project::storey_z::apply_storey_z_to_scene(
        &mut scene,
        &mut storey,
        Some(&project_snap),
        Some(&library),
    );
    // Wall axis elevation in scene MUST NOW BE new_elevation (4.00)
    if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(wall_h) {
        assert_eq!(pl.elevation, new_elevation, "Wall elevation must update after Apply");
    }

    // Case 2: Opening style modified (frame_thickness 0.06 -> 0.12), "Save" only.
    // Library updates, but scene opening children still have the old geometry!
    let mut updated_style = win_style.clone();
    let new_frame_thickness = 0.12;
    updated_style.frame_thickness = new_frame_thickness;
    library.upsert_opening_style(updated_style);

    let old_children = collect_opening_display_children(&scene, win_h);
    assert!(!old_children.is_empty());

    // When "Save and Apply" is triggered, regenerate_wall_representation is called with the updated library:
    let _ = regenerate_wall_representation(&mut scene, wall_pkg, Some(&library));
    let new_children = collect_opening_display_children(&scene, win_h);
    assert!(!new_children.is_empty());

    // Case 3: Material modified (change hatch pattern from ANSI31 to AR-B816), "Save" only.
    let mut updated_mat = initial_material.clone();
    updated_mat.hatch_pattern = "AR-B816".to_string();
    library.upsert_material(updated_mat);

    // Apply updates the wall with the new material
    let _ = regenerate_wall_representation(&mut scene, wall_pkg, Some(&library));
    // Verify hatch entity was updated in the scene
    let mut found_hatch = false;
    for entity in scene.document.entities() {
        if let EntityType::Hatch(h) = entity {
            if h.pattern.name == "AR-B816" {
                found_hatch = true;
                break;
            }
        }
    }
    assert!(found_hatch, "Hatch must reflect updated material after Apply");
}

#[test]
fn test_storey_height_change_moves_walls_and_keeps_opening_sill() {
    let mut scene = Scene::new();
    let initial_elevation = 0.0;
    let initial_height = 2.80;

    let mut storey = StoreyRef::new_with_height("EG", initial_elevation, initial_height, "eg.dwg");
    let floor_plane_id = storey.floor_plane_id;
    let ceiling_plane_id = storey.ceiling_plane_id;

    let mut project = ProjectFile::default();
    let mut building = Building::new("B1");
    building.storeys.push(storey.clone());
    project.buildings.push(building);

    let mut library = crate::modules::aec::engine::library::StyleLibrary::empty();
    let win_style = OpeningStyle::standard_window();
    library.upsert_opening_style(win_style);

    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
    pl.elevation = initial_elevation;

    let mut wall = Wall::new("Standard", initial_height, 0);
    wall.layers = vec![wl("mat1", 0.30, "Structural")];
    wall.base_plane_id = Some(floor_plane_id);
    wall.top_plane_id = Some(ceiling_plane_id);
    wall.base_origin = [0.0, 0.0, initial_elevation];
    wall.base_normal = [0.0, 0.0, 1.0];
    wall.top_origin = [0.0, 0.0, initial_elevation + initial_height];
    wall.top_normal = [0.0, 0.0, 1.0];

    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    let _ = regenerate_wall_representation(&mut scene, wall_h, Some(&library));

    // Place window with sill height 0.90 m
    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(2.0, 0.0, 0.0),
        OpeningKind::Window,
        Some(&library),
        None,
        None,
    )
    .expect("place window");

    let wall_pkg = resolve_wall_package(&scene, wall_h);
    let _ = regenerate_wall_representation(&mut scene, wall_pkg, Some(&library));

    // Initial check: wall height is 2.80, window sill height is 0.90 relative to wall base
    let wall_init = wall_from_entity(scene.document.get_entity(wall_h).unwrap()).unwrap();
    assert!((wall_init.height - 2.80).abs() < 1e-6);
    let win_init = crate::modules::aec::engine::opening_xdata::openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("window");
    assert!((win_init.sill_height - 0.90).abs() < 1e-4);

    // Scenario: Storey height is increased from 2.80 m to 3.20 m
    let new_height = 3.20;
    storey.set_height(new_height);
    storey.set_plane_z_relative_to_floor(ceiling_plane_id, new_height);
    project.buildings[0].storeys[0] = storey.clone();

    // Apply storey Z to scene
    crate::modules::aec::project::storey_z::apply_storey_z_to_scene(
        &mut scene,
        &mut storey,
        Some(&project),
        Some(&library),
    );

    // Verification:
    // 1. Wall height grew to 3.20 m
    let wall_updated = wall_from_entity(scene.document.get_entity(wall_h).unwrap()).unwrap();
    assert!(
        (wall_updated.height - 3.20).abs() < 1e-6,
        "Wall height should be 3.20, got {}",
        wall_updated.height
    );
    assert!(
        (wall_updated.base_origin[2] - 0.0).abs() < 1e-6,
        "Wall base origin Z should stay at 0.0"
    );
    assert!(
        (wall_updated.top_origin[2] - 3.20).abs() < 1e-6,
        "Wall top origin Z should be 3.20"
    );

    // 2. Window sill height remains 0.90 m relative to wall base
    let win_updated = crate::modules::aec::engine::opening_xdata::openings_for_host_wall(&scene, wall_h)
        .into_iter()
        .find(|o| o.handle == win_h)
        .expect("window");
    assert!(
        (win_updated.sill_height - 0.90).abs() < 1e-4,
        "Window sill height should stay at 0.90 relative to wall base, got {}",
        win_updated.sill_height
    );
}

#[test]
fn test_negative_plane_offsets_and_partial_bindings() {
    let storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
    let floor_id = storey.floor_plane_id;
    let ceil_id = storey.ceiling_plane_id;

    // Case 1: Wall with negative base offset (-0.15) and negative top offset (-0.20)
    let mut wall = Wall::new("Standard", 3.0, 0);
    wall.base_plane_id = Some(floor_id);
    wall.top_plane_id = Some(ceil_id);
    wall.base_offset = -0.15;
    wall.top_offset = -0.20;

    wall.rebake_planes(&storey, 0.0, 0.0);
    assert!(
        (wall.base_origin[2] - (-0.15)).abs() < 1e-6,
        "Base origin Z should be -0.15, got {}",
        wall.base_origin[2]
    );
    assert!(
        (wall.top_origin[2] - 2.80).abs() < 1e-6,
        "Top origin Z should be 2.80, got {}",
        wall.top_origin[2]
    );
    assert!(
        (wall.height - 2.95).abs() < 1e-6,
        "Height should be 2.95 (2.80 - (-0.15)), got {}",
        wall.height
    );

    // Case 2: Wall with ONLY top plane bound (e.g., hanging curtain wall / parapet)
    let mut top_only_wall = Wall::new("TopOnly", 2.0, 0);
    top_only_wall.base_origin = [0.0, 0.0, 1.0];
    top_only_wall.top_plane_id = Some(ceil_id);
    top_only_wall.top_offset = 0.0;
    top_only_wall.rebake_planes(&storey, 0.0, 0.0);
    assert!(
        (top_only_wall.top_origin[2] - 3.0).abs() < 1e-6,
        "Top origin Z should be 3.0"
    );
    assert!(
        (top_only_wall.height - 2.0).abs() < 1e-6,
        "Height should be 3.0 - 1.0 = 2.0, got {}",
        top_only_wall.height
    );

    // Case 3: Wall with ONLY base plane bound (fixed height wall)
    let mut base_only_wall = Wall::new("BaseOnly", 1.20, 0);
    base_only_wall.base_plane_id = Some(floor_id);
    base_only_wall.base_offset = 0.05;
    base_only_wall.rebake_planes(&storey, 0.0, 0.0);
    assert!(
        (base_only_wall.base_origin[2] - 0.05).abs() < 1e-6,
        "Base origin Z should be 0.05"
    );
    assert!(
        (base_only_wall.top_origin[2] - 1.25).abs() < 1e-6,
        "Top origin Z should follow base + height = 1.25"
    );
    assert!(
        (base_only_wall.height - 1.20).abs() < 1e-6,
        "Height should remain fixed at 1.20"
    );
}

#[test]
fn test_storey_rename_preserves_plane_bindings() {
    let mut storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
    let floor_id = storey.floor_plane_id;
    let ceil_id = storey.ceiling_plane_id;

    let mut wall = Wall::new("Standard", 3.0, 0);
    wall.base_plane_id = Some(floor_id);
    wall.top_plane_id = Some(ceil_id);
    wall.rebake_planes(&storey, 0.0, 0.0);

    assert_eq!(wall.base_plane_name.as_deref(), Some("EG_ELEVATION"));
    assert_eq!(wall.top_plane_name.as_deref(), Some("EG_OKGH"));

    // Rename storey and its control planes
    storey.name = "Erdgeschoss".to_string();
    if let Some(p) = storey.plane_mut(floor_id) {
        p.name = "Erdgeschoss_ELEVATION".to_string();
    }
    if let Some(p) = storey.plane_mut(ceil_id) {
        p.name = "Erdgeschoss_OKGH".to_string();
    }

    // Rebake with renamed storey: plane bindings by UUID stay intact and refresh names
    wall.rebake_planes(&storey, 0.0, 0.0);
    assert_eq!(wall.base_plane_id, Some(floor_id));
    assert_eq!(wall.top_plane_id, Some(ceil_id));
    assert_eq!(wall.base_plane_name.as_deref(), Some("Erdgeschoss_ELEVATION"));
    assert_eq!(wall.top_plane_name.as_deref(), Some("Erdgeschoss_OKGH"));
    assert!((wall.height - 3.0).abs() < 1e-6);
}

#[test]
fn test_multilayer_wall_layer_offsets_during_storey_height_change() {
    let mut storey = StoreyRef::new_with_height("EG", 0.0, 2.80, "eg.dwg");
    let floor_id = storey.floor_plane_id;
    let ceil_id = storey.ceiling_plane_id;

    let mut wall = Wall::new("MultiLayer", 2.80, 0);
    let mut l1 = wl("brick", 0.20, "Structural");
    l1.bottom_offset = 0.0;
    l1.top_offset = 0.0;
    let mut l2 = wl("insulation", 0.10, "Insulation");
    l2.bottom_offset = -0.05; // starts 5cm lower
    l2.top_offset = 0.10; // stops 10cm higher
    wall.layers = vec![l1, l2];
    wall.base_plane_id = Some(floor_id);
    wall.top_plane_id = Some(ceil_id);

    wall.rebake_planes(&storey, 0.0, 0.0);
    assert!((wall.height - 2.80).abs() < 1e-6);

    // Height increases to 3.50 m
    storey.set_plane_z_relative_to_floor(ceil_id, 3.50);
    wall.rebake_planes(&storey, 0.0, 0.0);
    assert!((wall.height - 3.50).abs() < 1e-6);

    // Effective heights of individual layers
    let l1_eff_height = (wall.height - wall.layers[0].bottom_offset - wall.layers[0].top_offset).max(0.0);
    let l2_eff_height = (wall.height - wall.layers[1].bottom_offset - wall.layers[1].top_offset).max(0.0);
    assert!((l1_eff_height - 3.50).abs() < 1e-6);
    assert!((l2_eff_height - (3.50 - (-0.05) - 0.10)).abs() < 1e-6);
    assert!((l2_eff_height - 3.45).abs() < 1e-6);
}

#[test]
fn test_hatch_outline_wire_respects_fill_plane_elevation() {
    let mut scene = Scene::new();
    let z_elev = 2.75;
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(4.0, 0.0)));
    pl.elevation = z_elev;

    let mut wall = Wall::new("Standard", 2.80, 0);
    wall.base_origin = [0.0, 0.0, z_elev];
    wall.layers = vec![wl("brick", 0.30, "Structural")];

    let mut library = crate::modules::aec::engine::library::StyleLibrary::empty();
    let mat = Material::new(
        "brick".to_string(),
        "Brick".to_string(),
        "ANSI31".to_string(),
        0xFFFFFF,
        "Continuous".to_string(),
    );
    library.upsert_material(mat);

    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    let touched = regenerate_wall_representation(&mut scene, wall_h, Some(&library)).expect("regen");
    let hatch_handle = touched
        .into_iter()
        .find(|h| matches!(scene.document.get_entity(*h), Some(EntityType::Hatch(_))))
        .expect("hatch generated");

    let wire = scene.hatch_outline_wire(hatch_handle).expect("outline wire");
    for pt in &wire.points {
        assert!(
            (pt[2] as f64 - z_elev).abs() < 1e-5,
            "Hatch outline wire point Z should be {}, got {}",
            z_elev,
            pt[2]
        );
    }
}

#[test]
fn test_l_join_at_nonzero_elevation() {
    let mut scene = Scene::new();
    let z_elev = 3.50;

    let mut pl1 = LwPolyline::new();
    pl1.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl1.add_vertex(LwVertex::new(Vector2::new(4.0, 0.0)));
    pl1.elevation = z_elev;
    let mut wall1 = Wall::new("Standard", 2.80, 0);
    wall1.base_origin = [0.0, 0.0, z_elev];
    wall1.layers = vec![wl("brick", 0.30, "Structural")];
    let mut ent1 = EntityType::LwPolyline(pl1);
    let mut rec1 = ExtendedDataRecord::new(AEC_APPID);
    rec1.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall1);
    ent1.common_mut().extended_data.add_record(rec1);
    let h1 = scene.add_entity(ent1);

    let mut pl2 = LwPolyline::new();
    pl2.add_vertex(LwVertex::new(Vector2::new(3.0, -2.0)));
    pl2.add_vertex(LwVertex::new(Vector2::new(3.0, 3.0)));
    pl2.elevation = z_elev;
    let mut wall2 = Wall::new("Standard", 2.80, 0);
    wall2.base_origin = [0.0, 0.0, z_elev];
    wall2.layers = vec![wl("brick", 0.30, "Structural")];
    let mut ent2 = EntityType::LwPolyline(pl2);
    let mut rec2 = ExtendedDataRecord::new(AEC_APPID);
    rec2.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall2);
    ent2.common_mut().extended_data.add_record(rec2);
    let h2 = scene.add_entity(ent2);

    regenerate_wall_representation(&mut scene, h1, None).expect("regen h1");
    regenerate_wall_representation(&mut scene, h2, None).expect("regen h2");

    let (kind, _) = join_two_walls_as_l_in_document(&mut scene, h1, h2, None, None, None)
        .expect("L-join between walls at Z={z_elev} should succeed");
    assert_eq!(kind, JoinKind::L);

    // Check that both wall polylines maintain their elevation
    let e1 = scene.document.get_entity(h1).unwrap();
    if let EntityType::LwPolyline(p) = e1 {
        assert!((p.elevation - z_elev).abs() < 1e-6, "Wall 1 elevation maintained");
    } else {
        panic!("Expected LwPolyline");
    }
    let e2 = scene.document.get_entity(h2).unwrap();
    if let EntityType::LwPolyline(p) = e2 {
        assert!((p.elevation - z_elev).abs() < 1e-6, "Wall 2 elevation maintained");
    } else {
        panic!("Expected LwPolyline");
    }
}

#[test]
fn test_opening_edit_dedup_and_display_rule_preservation() {
    let mut scene = Scene::new();
    let initial_elevation = 1.0;
    let initial_height = 2.80;

    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
    pl.add_vertex(LwVertex::new(Vector2::new(6.0, 0.0)));
    pl.elevation = initial_elevation;

    let mut wall = Wall::new("Standard", initial_height, 0);
    wall.base_origin = [0.0, 0.0, initial_elevation];
    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
    ent.common_mut().extended_data.add_record(rec);
    let wall_h = scene.add_entity(ent);

    let mut library = crate::modules::aec::engine::library::StyleLibrary::empty();
    let win_style = OpeningStyle::standard_window();
    library.upsert_opening_style(win_style);

    let (win_h, _) = place_wall_opening(
        &mut scene,
        wall_h,
        DVec3::new(3.0, 0.0, initial_elevation),
        OpeningKind::Window,
        Some(&library),
        None,
        None,
    )
    .expect("place window");

    let entity = scene.document.get_entity(win_h).cloned().unwrap();
    let mut opening = crate::modules::aec::engine::opening_xdata::opening_from_entity(&entity, win_h).unwrap();

    // Verify apply_opening_property detects changes and rejects no-ops
    let changed = crate::modules::aec::properties::apply_opening_property(
        &mut opening,
        "opening_width",
        "1.50",
        Some(&library),
    );
    assert!(changed, "Width change from 1.20 to 1.50 should report change");
    assert!((opening.width - 1.50).abs() < 1e-6);

    let same = crate::modules::aec::properties::apply_opening_property(
        &mut opening,
        "opening_width",
        "1.50",
        Some(&library),
    );
    assert!(!same, "Applying same width 1.50 must return false to skip regen");

    // Commit opening with rules
    let mut rules = crate::modules::aec::engine::display_component::ComponentRuleSet::default();
    rules.visibility.insert("Layers2D".to_string(), false);

    let touched = crate::modules::aec::engine::opening_display::commit_opening_instance(
        &mut scene,
        &opening,
        Some(&library),
        Some(&rules),
    );
    assert!(!touched.is_empty());

    // Verify that the opening was committed with new width
    let reloaded = crate::modules::aec::engine::opening_xdata::opening_from_entity(scene.document.get_entity(win_h).unwrap(), win_h).unwrap();
    assert!((reloaded.width - 1.50).abs() < 1e-6);
}
