//! Property-panel contributions for AEC entities.

use acadrust::{CadDocument, EntityType, Handle};

use crate::modules::aec::engine::join_ops;
use crate::modules::aec::engine::junction_pick;
use crate::modules::aec::engine::opening_display;
use crate::modules::aec::engine::opening_shape::OpeningShape;
use crate::modules::aec::engine::opening_style::{apply_style_defaults, LeafOperation};
use crate::modules::aec::engine::opening_xdata;
use crate::modules::aec::engine::openings::{
    NicheSide, Opening, OpeningKind, OpeningReferenceSide, SwingSide,
};
use crate::modules::aec::engine::room::{RoomFinish, RoomFunction};
use crate::modules::aec::engine::room_package;
use crate::modules::aec::engine::room_regen::{self, room_boundary_points};
use crate::modules::aec::engine::room_xdata;
use crate::modules::aec::engine::slab::{
    Slab, SlabJustification, SLAB_BASE_OFFSET_KEYS, SLAB_TOP_OFFSET_KEYS,
};
use crate::modules::aec::engine::slab_opening::{SlabOpening, SlabOpeningDepth, SlabOpeningKind};
use crate::modules::aec::engine::slab_package;
use crate::modules::aec::engine::slab_xdata;
use crate::modules::aec::engine::storey_xdata;
use crate::modules::aec::engine::xdata;
use crate::modules::aec::engine::wall_package;
use crate::modules::aec::engine::library::{self, StyleLibrary};
use crate::t;

pub fn aec_entity_title(
    scene: &crate::scene::Scene,
    handle: Handle,
    entity: &EntityType,
) -> Option<String> {
    if xdata::wall_from_entity(entity).is_some() {
        return Some(crate::tr!("aec", "wall-title"));
    }
    if slab_xdata::slab_from_entity(entity).is_some() {
        return Some(crate::tr!("aec", "slab-title"));
    }
    if room_xdata::room_from_entity(entity).is_some() {
        return Some(crate::tr!("aec", "room-title"));
    }
    if room_package::is_room_derived(entity) {
        return Some(crate::tr!("aec", "room-title"));
    }
    if let Some(opening) = slab_xdata::slab_opening_from_entity(entity) {
        let _ = opening;
        return Some(crate::tr!("aec", "slabopening-title"));
    }
    if let Some(owner) = slab_package::slab_opening_owner_if_any(scene, handle) {
        if scene
            .document
            .get_entity(owner)
            .and_then(slab_xdata::slab_opening_from_entity)
            .is_some()
        {
            return Some(crate::tr!("aec", "slabopening-title"));
        }
    }
    if slab_package::is_slab_derived(scene, handle)
        && scene
            .document
            .get_entity(slab_package::resolve_slab_package(scene, handle))
            .and_then(slab_xdata::slab_from_entity)
            .is_some()
    {
        return Some(crate::tr!("aec", "slab-title"));
    }
    if let Some(opening) = opening_xdata::opening_from_entity(entity, handle) {
        let name = match opening.kind {
            OpeningKind::Window => crate::tr!("aec", "window-title"),
            OpeningKind::Door => crate::tr!("aec", "door-title"),
            OpeningKind::Breakthrough => crate::tr!("aec", "opening-title"),
        };
        return Some(name);
    }
    if let Some(owner) = opening_display::opening_owner_if_any(scene, handle) {
        if let Some(owner_ent) = scene.document.get_entity(owner) {
            if let Some(opening) = opening_xdata::opening_from_entity(owner_ent, owner) {
                let name = match opening.kind {
                    OpeningKind::Window => crate::tr!("aec", "window-title"),
                    OpeningKind::Door => crate::tr!("aec", "door-title"),
                    OpeningKind::Breakthrough => crate::tr!("aec", "opening-title"),
                };
                return Some(name);
            }
        }
    }
    if storey_xdata::storey_from_entity(entity).is_some() {
        return Some(crate::tr!("aec", "storey-title"));
    }
    if crate::modules::aec::project::preview::control_plane_from_entity(entity).is_some() {
        return Some(crate::tr!("aec", "control-plane"));
    }
    if crate::modules::aec::engine::views::carrier::read_view_carrier(&scene.document, entity).is_some()
        || crate::modules::aec::engine::views::carrier::badge_carrier_handle(entity).is_some()
    {
        return Some(crate::tr!("aec", "view-carrier-title"));
    }
    if let Some(marker) = crate::modules::aec::views::din1356::read_section_marker(&scene.document, entity) {
        return Some(if marker.is_elevation {
            crate::tr!("aec", "elevation-marker-title")
        } else {
            crate::tr!("aec", "section-marker-title")
        });
    }
    if let Some(marker_owner) = resolve_section_marker_package(scene, handle) {
        if marker_owner != handle {
            if let Some(owner_ent) = scene.document.get_entity(marker_owner) {
                if let Some(marker) = crate::modules::aec::views::din1356::read_section_marker(&scene.document, owner_ent) {
                    return Some(if marker.is_elevation {
                        crate::tr!("aec", "elevation-marker-title")
                    } else {
                        crate::tr!("aec", "section-marker-title")
                    });
                }
            }
        }
    }
    if entity.common().extended_data.records().iter().any(|r| {
        r.application_name == crate::modules::aec::engine::xdata::AEC_APPID
            && matches!(r.values.first(), Some(acadrust::xdata::XDataValue::String(s)) if s == crate::modules::aec::engine::views::carrier::DERIVED_PRIMITIVE_TAG)
    }) {
        return Some("AEC Assoziiertes Bauteil".to_string());
    }
    None
}

pub fn collapse_selection_to_wall_package<'a>(
    scene: &'a crate::scene::Scene,
    selected: Vec<(Handle, &'a EntityType)>,
) -> Vec<(Handle, &'a EntityType)> {
    collapse_selection_to_aec_package(scene, selected)
}

pub fn collapse_selection_to_aec_package<'a>(
    scene: &'a crate::scene::Scene,
    selected: Vec<(Handle, &'a EntityType)>,
) -> Vec<(Handle, &'a EntityType)> {
    if selected.is_empty() {
        return selected;
    }
    let mut collapsed = Vec::with_capacity(selected.len());
    let mut seen = rustc_hash::FxHashSet::default();

    for (handle, entity) in selected {
        let owner_handle = resolve_aec_package(scene, handle);
        if seen.insert(owner_handle) {
            if let Some(owner_entity) = scene.document.get_entity(owner_handle) {
                collapsed.push((owner_handle, owner_entity));
            } else {
                collapsed.push((handle, entity));
            }
        }
    }

    let wall_handles: rustc_hash::FxHashSet<Handle> = collapsed
        .iter()
        .filter_map(|(h, e)| {
            if xdata::wall_from_entity(e).is_some() {
                Some(*h)
            } else {
                None
            }
        })
        .collect();

    let slab_handles: rustc_hash::FxHashSet<Handle> = collapsed
        .iter()
        .filter_map(|(h, e)| {
            if slab_xdata::slab_from_entity(e).is_some() {
                Some(*h)
            } else {
                None
            }
        })
        .collect();

    if !wall_handles.is_empty() || !slab_handles.is_empty() {
        collapsed.retain(|(h, e)| {
            if !wall_handles.is_empty() {
                if let Some(op) = opening_xdata::opening_from_entity(e, *h) {
                    let host = wall_package::resolve_wall_package(scene, op.host_wall);
                    if wall_handles.contains(&op.host_wall) || wall_handles.contains(&host) {
                        return false;
                    }
                }
            }
            if !slab_handles.is_empty() {
                if let Some(op) = slab_xdata::slab_opening_from_entity(e) {
                    let host = slab_package::resolve_slab_package(scene, op.host_slab);
                    if slab_handles.contains(&op.host_slab) || slab_handles.contains(&host) {
                        return false;
                    }
                }
            }
            true
        });
    }

    collapsed
}

/// Builds the "Wall"/"Wall Layers" property section for a single entity, if
/// it carries an `OPENCAD_AEC` `WALL` XDATA record.
pub fn wall_prop_section(
    entity: &EntityType,
    style_library: Option<&StyleLibrary>,
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> Option<crate::scene::model::object::PropSection> {
    if let Some(wall) = xdata::wall_from_entity(entity) {
        let style_name = style_library
            .and_then(|lib| lib.wall_styles.iter().find(|ws| ws.style.id == wall.style_id))
            .map(|ws| ws.style.name.clone())
            .unwrap_or_else(|| wall.style_id.clone());

        let base_bound = wall.base_plane_id.is_some();
        let mut plane_options =
            crate::modules::aec::project::wall_planes::build_project_plane_options(project);
        let base_label = crate::modules::aec::project::wall_planes::resolve_plane_display_label(
            project,
            wall.base_plane_id,
            wall.base_plane_name.as_deref(),
        );
        let top_label = crate::modules::aec::project::wall_planes::resolve_plane_display_label(
            project,
            wall.top_plane_id,
            wall.top_plane_name.as_deref(),
        );
        let ensure_option = |opts: &mut Vec<String>, label: &str| {
            if !opts.iter().any(|n| n == label) {
                opts.push(label.to_string());
            }
        };
        ensure_option(&mut plane_options, &base_label);
        ensure_option(&mut plane_options, &top_label);
        let mut props = vec![
            formula_edit_prop(t!("Height").as_ref(), "wall_height", wall.height, &wall.formulas),
            crate::scene::model::object::Property {
                label: t!("Base plane").into_owned(),
                field: "wall_base_plane",
                value: crate::scene::model::object::PropValue::Choice {
                    selected: base_label,
                    options: plane_options.clone(),
                },
            },
            formula_edit_prop(
                t!("Base offset").as_ref(),
                "wall_base_offset",
                wall.base_offset,
                &wall.formulas,
            ),
            crate::scene::model::object::Property {
                label: t!("Top plane").into_owned(),
                field: "wall_top_plane",
                value: crate::scene::model::object::PropValue::Choice {
                    selected: top_label,
                    options: plane_options,
                },
            },
            formula_edit_prop(
                t!("Top offset").as_ref(),
                "wall_top_offset",
                wall.top_offset,
                &wall.formulas,
            ),
            crate::scene::model::object::Property {
                label: t!("Style").into_owned(),
                field: "wall_style",
                value: crate::scene::model::object::PropValue::Picker {
                    value: style_name,
                    handles: vec![entity.common().handle],
                },
            },
            crate::scene::model::object::Property {
                label: t!("Justification").into_owned(),
                field: "wall_justification",
                value: crate::scene::model::object::PropValue::Choice {
                    selected: wall.justification.as_str().to_string(),
                    options: vec![
                        "Interior".to_string(),
                        "Center".to_string(),
                        "Exterior".to_string(),
                    ],
                },
            },
            crate::scene::model::object::Property {
                label: t!("Phase").into_owned(),
                field: "wall_phase",
                value: crate::scene::model::object::PropValue::Choice {
                    selected: wall.phase.display_label().to_string(),
                    options: vec![
                        crate::modules::aec::engine::plan_view::PlanPhase::New
                            .display_label()
                            .to_string(),
                        crate::modules::aec::engine::plan_view::PlanPhase::Demolition
                            .display_label()
                            .to_string(),
                        crate::modules::aec::engine::plan_view::PlanPhase::Existing
                            .display_label()
                            .to_string(),
                    ],
                },
            },
        ];
        if !base_bound {
            props.insert(
                1,
                formula_edit_prop(
                    crate::tr!("aec", "wall-base-z").as_ref(),
                    "wall_base_z",
                    wall.base_origin[2],
                    &wall.formulas,
                ),
            );
        }
        if wall.is_sloped() {
            let pts = match entity {
                EntityType::LwPolyline(pl) => pl.vertices.iter().map(|v| (v.location.x, v.location.y)).collect::<Vec<_>>(),
                EntityType::Line(l) => vec![(l.start.x, l.start.y), (l.end.x, l.end.y)],
                _ => Vec::new(),
            };
            if !pts.is_empty() {
                let mut min_h = f64::INFINITY;
                let mut max_h = f64::NEG_INFINITY;
                for &(x, y) in &pts {
                    let h = wall.height_at_xy(x, y);
                    min_h = min_h.min(h);
                    max_h = max_h.max(h);
                }
                props.insert(
                    1,
                    crate::entities::common::ro_prop(
                        t!("Height range").as_ref(),
                        "wall_height_range",
                        format!("{:.2} m – {:.2} m", min_h, max_h),
                    ),
                );
            }
        }

        let hatch_enabled = wall.hatch_override.is_some();
        props.push(crate::scene::model::object::Property {
            label: t!("Hatch Angle Override").into_owned(),
            field: "wall_hatch_override_enabled",
            value: crate::scene::model::object::PropValue::BoolToggle {
                field: "wall_hatch_override_enabled",
                value: hatch_enabled,
            },
        });
        if let Some(ov) = wall.hatch_override.as_ref() {
            props.push(crate::entities::common::edit_scalar_prop(
                t!("Hatch Angle").as_ref(),
                "wall_hatch_angle",
                ov.hatch_angle.unwrap_or(0.0),
            ));
            props.push(crate::scene::model::object::Property {
                label: t!("Relative to Wall").into_owned(),
                field: "wall_hatch_relative",
                value: crate::scene::model::object::PropValue::BoolToggle {
                    field: "wall_hatch_relative",
                    value: ov.hatch_angle_relative.unwrap_or(true),
                },
            });
        }
        let effective_text = if let Some(ov) = wall.hatch_override.as_ref() {
            format!(
                "{}° ({}) — {}",
                ov.hatch_angle.unwrap_or(0.0),
                if ov.hatch_angle_relative.unwrap_or(true) {
                    t!("relative")
                } else {
                    t!("absolute")
                },
                t!("Wall override")
            )
        } else {
            format!("{}", t!("No wall override (uses style/material default)"))
        };
        props.push(crate::entities::common::ro_prop(
            t!("Effective Hatch Angle").as_ref(),
            "wall_hatch_effective",
            effective_text,
        ));

        for (i, layer) in wall.layers.iter().enumerate() {
            let thickness_str = format!("{:.3} m", layer.thickness);
            let layer_info = format!("{} — {} ({})", layer.material, thickness_str, layer.function);
            props.push(crate::entities::common::ro_prop(
                t!("Layer").as_ref(),
                "wall_layer",
                format!("{} — {}", i + 1, layer_info),
            ));
        }

        Some(crate::scene::model::object::PropSection {
            title: t!("Wall Layers").into_owned(),
            props,
        })
    } else {
        None
    }
}

pub fn wall_relation_sections(
    scene: &crate::scene::Scene,
    wall_handle: Handle,
) -> Vec<crate::scene::model::object::PropSection> {
    use crate::scene::model::object::{PropSection, PropValue, Property};

    let openings = opening_xdata::openings_for_host_wall(scene, wall_handle);
    let mut opening_props = Vec::with_capacity(openings.len().max(1));
    if openings.is_empty() {
        opening_props.push(crate::entities::common::ro_prop(
            t!("(none)").as_ref(),
            "wall_opening_none",
            String::new(),
        ));
    } else {
        for (idx, opening) in openings.iter().enumerate() {
            let kind = opening.kind.as_str();
            let display = format!(
                "{kind}  w={} h={} sill={}  (#{:X})",
                crate::entities::common::format_length(opening.width),
                crate::entities::common::format_length(opening.height),
                crate::entities::common::format_length(opening.sill_height),
                opening.handle.value()
            );
            opening_props.push(Property {
                label: format!("{} {}", t!("Opening"), idx + 1),
                field: "wall_opening",
                value: PropValue::EntityRef {
                    display,
                    handle: opening.handle,
                },
            });
        }
    }

    let peers = crate::modules::aec::engine::owner_index::peers_of(&scene.document, wall_handle);
    let mut peer_props = Vec::with_capacity(peers.len().max(1));
    if peers.is_empty() {
        peer_props.push(crate::entities::common::ro_prop(
            t!("(none)").as_ref(),
            "wall_peer_none",
            String::new(),
        ));
    } else {
        for (idx, peer) in peers.iter().enumerate() {
            let display = crate::tf!("Wall #{:X}", peer.value()).into_owned();
            peer_props.push(Property {
                label: format!("{} {}", t!("Wall"), idx + 1),
                field: "wall_peer",
                value: PropValue::EntityRef {
                    display,
                    handle: *peer,
                },
            });
        }
    }

    vec![
        PropSection {
            title: t!("Linked Openings").into_owned(),
            props: opening_props,
        },
        PropSection {
            title: t!("Joined Walls").into_owned(),
            props: peer_props,
        },
    ]
}

pub fn storey_prop_section(
    scene: &crate::scene::Scene,
    entity: &EntityType,
) -> Option<crate::scene::model::object::PropSection> {
    use crate::scene::model::object::{PropSection, PropValue, Property};

    let (storey_id, storey, _) = storey_xdata::storey_from_entity(entity)?;
    let storey_handle = entity.common().handle;
    let members = storey_xdata::walls_for_storey(scene, storey_handle);

    let mut props = vec![
        crate::entities::common::ro_prop(t!("Name").as_ref(), "storey_name", storey.name.clone()),
        crate::entities::common::ro_prop(t!("Id").as_ref(), "storey_id", storey_id.to_string()),
        crate::entities::common::ro_prop(
            t!("Elevation").as_ref(),
            "storey_elevation",
            crate::entities::common::format_length(storey.elevation),
        ),
        crate::entities::common::ro_prop(
            t!("Height").as_ref(),
            "storey_height",
            crate::entities::common::format_length(storey.height),
        ),
        crate::entities::common::ro_prop(
            t!("Members").as_ref(),
            "storey_members_count",
            members.len().to_string(),
        ),
    ];

    for (idx, member) in members.iter().enumerate() {
        let display = if scene
            .document
            .get_entity(*member)
            .and_then(xdata::wall_from_entity)
            .is_some()
        {
            crate::tf!("Wall #{:X}", member.value()).into_owned()
        } else {
            crate::tf!("Entity #{:X}", member.value()).into_owned()
        };
        props.push(Property {
            label: format!("{} {}", t!("Member"), idx + 1),
            field: "storey_member",
            value: PropValue::EntityRef {
                display,
                handle: *member,
            },
        });
    }

    Some(PropSection {
        title: t!("Storey").into_owned(),
        props,
    })
}

fn control_plane_prop_section(entity: &EntityType) -> Option<crate::scene::model::object::PropSection> {
    let (_id, name) = crate::modules::aec::project::preview::control_plane_from_entity(entity)?;
    Some(crate::scene::model::object::PropSection {
        title: crate::tr!("aec", "control-plane"),
        props: vec![crate::scene::model::object::Property {
            label: crate::tr!("aec", "control-plane-name"),
            field: "control_plane_name",
            value: crate::scene::model::object::PropValue::PlainText(name),
        }],
    })
}

pub fn apply_control_plane_name(
    scene: &mut crate::scene::Scene,
    project: Option<&mut crate::modules::aec::engine::project::ProjectFile>,
    handle: Handle,
    name: &str,
) -> bool {
    let Some(entity) = scene.document.get_entity(handle) else {
        return false;
    };
    let Some((id, _)) = crate::modules::aec::project::preview::control_plane_from_entity(entity)
    else {
        return false;
    };
    crate::modules::aec::project::preview::write_control_plane_tag(scene, handle, id, name);
    if let Some(project) = project {
        for building in &mut project.buildings {
            for storey in &mut building.storeys {
                if let Some(plane) = storey.plane_mut(id) {
                    plane.name = name.to_string();
                    return true;
                }
            }
        }
    }
    true
}

/// Append wall/storey property sections when the selection is AEC-related.
fn shape_options() -> Vec<String> {
    vec![
        OpeningShape::Rectangle.as_str().to_string(),
        OpeningShape::Circle.as_str().to_string(),
        OpeningShape::Triangle(
            crate::modules::aec::engine::opening_shape::TriangleVariant::IsoscelesUp,
        )
        .as_str()
        .to_string(),
        OpeningShape::Triangle(
            crate::modules::aec::engine::opening_shape::TriangleVariant::IsoscelesDown,
        )
        .as_str()
        .to_string(),
        OpeningShape::Triangle(
            crate::modules::aec::engine::opening_shape::TriangleVariant::Equilateral,
        )
        .as_str()
        .to_string(),
        OpeningShape::Triangle(
            crate::modules::aec::engine::opening_shape::TriangleVariant::RightLeft,
        )
        .as_str()
        .to_string(),
        OpeningShape::Triangle(
            crate::modules::aec::engine::opening_shape::TriangleVariant::RightRight,
        )
        .as_str()
        .to_string(),
        OpeningShape::Arch.as_str().to_string(),
    ]
}

/// Apply a properties-panel field onto an opening instance (no I/O).
pub fn apply_opening_property(
    opening: &mut Opening,
    field: &str,
    val: &str,
    library: Option<&StyleLibrary>,
) -> bool {
    match field {
        "opening_style" => {
            let Some(lib) = library else {
                return false;
            };
            let style = lib
                .opening_styles
                .iter()
                .find(|s| s.style.name == val || s.style.id == val);
            if let Some(style) = style {
                if opening.style_id.as_deref() == Some(style.style.id.as_str()) {
                    return false;
                }
                apply_style_defaults(opening, style);
                true
            } else {
                false
            }
        }
        "opening_width" => {
            let Some(v) = crate::entities::common::parse_f64(val) else {
                return false;
            };
            if v <= 0.0 {
                return false;
            }
            let (w, h) = opening.shape.lock_size(v, opening.height, true);
            if (opening.width - w).abs() < 1e-9 && (opening.height - h).abs() < 1e-9 {
                return false;
            }
            opening.width = w;
            opening.height = h;
            true
        }
        "opening_height" => {
            let Some(v) = crate::entities::common::parse_f64(val) else {
                return false;
            };
            if v <= 0.0 {
                return false;
            }
            let (w, h) = opening.shape.lock_size(opening.width, v, false);
            if (opening.width - w).abs() < 1e-9
                && (opening.height - h).abs() < 1e-9
                && opening.head_plane_id.is_none()
            {
                return false;
            }
            opening.width = w;
            opening.height = h;
            opening.unbind_head_plane();
            true
        }
        "opening_sill" => {
            let Some(v) = crate::entities::common::parse_f64(val) else {
                return false;
            };
            let v = v.max(0.0);
            if (opening.sill_height - v).abs() < 1e-9 && opening.sill_plane_id.is_none() {
                return false;
            }
            opening.sill_height = v;
            opening.unbind_sill_plane();
            true
        }
        "opening_spring" => {
            let Some(v) = crate::entities::common::parse_f64(val) else {
                return false;
            };
            let clamped =
                crate::modules::aec::engine::opening_shape::clamp_spring(v, opening.height);
            if (opening.spring_height - clamped).abs() < 1e-9 {
                return false;
            }
            opening.spring_height = clamped;
            true
        }
        "opening_hinge" => {
            let h = LeafOperation::from_str(val);
            if opening.hinge == h {
                return false;
            }
            opening.hinge = h;
            true
        }
        "opening_swing_side" => {
            let ss = SwingSide::from_str(val);
            if opening.swing_side == ss {
                return false;
            }
            opening.swing_side = ss;
            true
        }
        "opening_reference_side" => {
            let r = OpeningReferenceSide::from_str(val);
            if opening.reference_side == r {
                return false;
            }
            opening.reference_side = r;
            true
        }
        "opening_cross_offset" => {
            if let Some(v) = crate::entities::common::parse_f64(val) {
                if (opening.cross_axis_offset - v).abs() < 1e-9 {
                    return false;
                }
                opening.cross_axis_offset = v;
                true
            } else {
                false
            }
        }
        "opening_flip" => {
            opening.flip();
            true
        }
        "opening_depth" => {
            let new_depth = if let Some(v) = crate::entities::common::parse_f64(val) {
                if v > 0.0 {
                    Some(v)
                } else {
                    None
                }
            } else if val.trim().is_empty() || val.eq_ignore_ascii_case("none") {
                None
            } else {
                return false;
            };
            if opening.depth == new_depth {
                return false;
            }
            opening.depth = new_depth;
            true
        }
        "opening_niche_side" => {
            let ns = NicheSide::from_str(val);
            if opening.niche_side == ns {
                return false;
            }
            opening.niche_side = ns;
            true
        }
        "opening_shape" => {
            let sh = OpeningShape::from_str(val);
            let (w, h) = sh.lock_size(opening.width, opening.height, true);
            let spring = if sh == OpeningShape::Arch && opening.spring_height <= 1e-12 {
                OpeningShape::default_spring_height(w, h)
            } else {
                opening.spring_height
            };
            if opening.shape == sh
                && (opening.width - w).abs() < 1e-9
                && (opening.height - h).abs() < 1e-9
                && (opening.spring_height - spring).abs() < 1e-9
            {
                return false;
            }
            opening.shape = sh;
            opening.width = w;
            opening.height = h;
            opening.spring_height = spring;
            true
        }
        _ => false,
    }
}

pub fn opening_prop_section(
    opening: &Opening,
    opening_handle: Handle,
    style_library: Option<&StyleLibrary>,
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> crate::scene::model::object::PropSection {
    let style_name = opening
        .style_id
        .as_deref()
        .and_then(|id| {
            style_library.and_then(|lib| {
                lib.find_opening_style(id)
                    .map(|s| s.style.name.clone())
            })
        })
        .unwrap_or_else(|| {
            opening
                .style_id
                .clone()
                .unwrap_or_else(|| crate::tr!("aec", "opening-no-style"))
        });
    let mut plane_options =
        crate::modules::aec::project::wall_planes::build_project_plane_options(project);
    let sill_label = crate::modules::aec::project::wall_planes::resolve_plane_display_label(
        project,
        opening.sill_plane_id,
        opening.sill_plane_name.as_deref(),
    );
    let head_label = crate::modules::aec::project::wall_planes::resolve_plane_display_label(
        project,
        opening.head_plane_id,
        opening.head_plane_name.as_deref(),
    );
    let ensure_option = |opts: &mut Vec<String>, label: &str| {
        if !opts.iter().any(|n| n == label) {
            opts.push(label.to_string());
        }
    };
    ensure_option(&mut plane_options, &sill_label);
    ensure_option(&mut plane_options, &head_label);
    let mut props = vec![
        crate::scene::model::object::Property {
            label: crate::tr!("aec", "opening-kind"),
            field: "opening_kind",
            value: crate::scene::model::object::PropValue::ReadOnly(opening.kind.as_str().into()),
        },
        crate::scene::model::object::Property {
            label: t!("Style").into_owned(),
            field: "opening_style",
            value: crate::scene::model::object::PropValue::Picker {
                value: style_name,
                handles: vec![opening_handle],
            },
        },
        crate::scene::model::object::Property {
            label: crate::tr!("aec", "opening-shape"),
            field: "opening_shape",
            value: crate::scene::model::object::PropValue::Choice {
                selected: opening.shape.as_str().to_string(),
                options: shape_options(),
            },
        },
        formula_edit_prop(t!("Width").as_ref(), "opening_width", opening.width, &opening.formulas),
        formula_edit_prop(t!("Height").as_ref(), "opening_height", opening.height, &opening.formulas),
        formula_edit_prop(
            crate::tr!("aec", "opening-sill").as_str(),
            "opening_sill",
            opening.sill_height,
            &opening.formulas,
        ),
        crate::scene::model::object::Property {
            label: crate::tr!("aec", "opening-sill-plane"),
            field: "opening_sill_plane",
            value: crate::scene::model::object::PropValue::Choice {
                selected: sill_label,
                options: plane_options.clone(),
            },
        },
        formula_edit_prop(
            crate::tr!("aec", "opening-sill-offset").as_str(),
            "opening_sill_offset",
            opening.sill_offset,
            &opening.formulas,
        ),
        crate::scene::model::object::Property {
            label: crate::tr!("aec", "opening-head-plane"),
            field: "opening_head_plane",
            value: crate::scene::model::object::PropValue::Choice {
                selected: head_label,
                options: plane_options,
            },
        },
        formula_edit_prop(
            crate::tr!("aec", "opening-head-offset").as_str(),
            "opening_head_offset",
            opening.head_offset,
            &opening.formulas,
        ),
        crate::scene::model::object::Property {
            label: crate::tr!("aec", "opening-hinge"),
            field: "opening_hinge",
            value: crate::scene::model::object::PropValue::Choice {
                selected: opening.hinge.as_str().to_string(),
                options: vec![
                    LeafOperation::left().as_str().into(),
                    LeafOperation::right().as_str().into(),
                ],
            },
        },
        crate::scene::model::object::Property {
            label: crate::tr!("aec", "opening-swing-side"),
            field: "opening_swing_side",
            value: crate::scene::model::object::PropValue::Choice {
                selected: opening.swing_side.as_str().to_string(),
                options: vec![
                    SwingSide::Exterior.as_str().into(),
                    SwingSide::Interior.as_str().into(),
                ],
            },
        },
        crate::scene::model::object::Property {
            label: crate::tr!("aec", "opening-reference-side"),
            field: "opening_reference_side",
            value: crate::scene::model::object::PropValue::Choice {
                selected: opening.reference_side.as_str().to_string(),
                options: vec![
                    OpeningReferenceSide::Start.as_str().into(),
                    OpeningReferenceSide::Center.as_str().into(),
                    OpeningReferenceSide::End.as_str().into(),
                ],
            },
        },
        formula_edit_prop(
            crate::tr!("aec", "opening-cross-offset").as_str(),
            "opening_cross_offset",
            opening.cross_axis_offset,
            &opening.formulas,
        ),
        crate::scene::model::object::Property {
            label: crate::tr!("aec", "opening-flip"),
            field: "opening_flip",
            value: crate::scene::model::object::PropValue::Choice {
                selected: format!("{} / {}", opening.reference_side.as_str(), opening.hinge.as_str()),
                options: vec![
                    "Start / Left".into(),
                    "Start / Right".into(),
                    "End / Left".into(),
                    "End / Right".into(),
                    "Center / Left".into(),
                    "Center / Right".into(),
                ],
            },
        },
    ];
    if opening.kind == OpeningKind::Breakthrough {
        props.push(crate::entities::common::edit_prop(
            crate::tr!("aec", "opening-depth").as_str(),
            "opening_depth",
            opening.depth.unwrap_or(0.0),
        ));
        props.push(crate::scene::model::object::Property {
            label: crate::tr!("aec", "opening-niche-side"),
            field: "opening_niche_side",
            value: crate::scene::model::object::PropValue::Choice {
                selected: opening.niche_side.as_str().to_string(),
                options: vec![
                    NicheSide::Exterior.as_str().into(),
                    NicheSide::Interior.as_str().into(),
                ],
            },
        });
    }
    if opening.shape == OpeningShape::Arch {
        props.push(crate::entities::common::edit_prop(
            crate::tr!("aec", "opening-spring").as_str(),
            "opening_spring",
            opening.spring_height,
        ));
    }
    crate::scene::model::object::PropSection {
        title: crate::tr!("aec", "opening-section"),
        props,
    }
}

pub fn extend_entity_sections(
    scene: &crate::scene::Scene,
    handle: Handle,
    entity: &EntityType,
    style_library: Option<&StyleLibrary>,
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    sections: &mut Vec<crate::scene::model::object::PropSection>,
) {
    if let Some(opening_handle) = opening_display::opening_owner_if_any(scene, handle) {
        if let Some(opening_entity) = scene.document.get_entity(opening_handle) {
            if let Some(opening) = opening_xdata::opening_from_entity(opening_entity, opening_handle)
            {
                sections.push(opening_prop_section(
                    &opening,
                    opening_handle,
                    style_library,
                    project,
                ));
            }
        }
    } else if let Some(slab_opening_handle) = slab_package::slab_opening_owner_if_any(scene, handle)
    {
        if let Some(opening_entity) = scene.document.get_entity(slab_opening_handle) {
            if let Some(opening) = slab_xdata::slab_opening_from_entity(opening_entity) {
                sections.push(slab_opening_prop_section(
                    &opening,
                    slab_opening_handle,
                    opening_entity,
                ));
            }
        }
    } else {
        let wall_handle = wall_package::resolve_wall_package(scene, handle);
        let wall_entity = scene.document.get_entity(wall_handle).unwrap_or(entity);
        if let Some(wall_section) = wall_prop_section(wall_entity, style_library, project) {
            sections.push(wall_section);
            sections.extend(wall_relation_sections(scene, wall_handle));
        } else {
            let slab_handle = slab_package::resolve_slab_package(scene, handle);
            let slab_entity = scene.document.get_entity(slab_handle).unwrap_or(entity);
            if let Some(slab_section) =
                slab_prop_section(slab_entity, style_library, project)
            {
                sections.push(slab_section);
            } else {
                let room_handle = scene
                    .document
                    .get_entity(handle)
                    .and_then(room_package::resolve_room_package)
                    .unwrap_or(handle);
                let room_entity = scene.document.get_entity(room_handle).unwrap_or(entity);
                if let Some(room_section) = room_prop_section(room_entity, style_library) {
                    sections.push(room_section);
                }
            }
        }
    }
    if let Some(storey_section) = storey_prop_section(scene, entity) {
        sections.push(storey_section);
    }
    if let Some(plane_section) = control_plane_prop_section(entity) {
        sections.push(plane_section);
    }
    let display_config_library = crate::modules::aec::engine::project::resolve_display_config_library(project);
    if let Some(carrier_section) = carrier_prop_section_with_library(
        entity,
        &scene.document,
        &display_config_library,
    ) {
        sections.push(carrier_section);
    }
    let marker_handle = resolve_section_marker_package(scene, handle).unwrap_or(handle);
    let marker_entity = scene.document.get_entity(marker_handle).unwrap_or(entity);
    if let Some(marker_section) = marker_prop_section(marker_entity, &scene.document) {
        sections.push(marker_section);
    }
    if let Some(prim_section) = derived_primitive_prop_section(entity) {
        sections.push(prim_section);
    }
}

pub fn carrier_prop_section(
    entity: &EntityType,
    doc: &CadDocument,
) -> Option<crate::scene::model::object::PropSection> {
    let display_config_library = crate::modules::aec::engine::project::resolve_display_config_library(None);
    carrier_prop_section_with_library(entity, doc, &display_config_library)
}

pub fn carrier_prop_section_with_library(
    entity: &EntityType,
    doc: &CadDocument,
    dc_lib: &crate::modules::aec::engine::library::DisplayConfigLibrary,
) -> Option<crate::scene::model::object::PropSection> {
    use crate::modules::aec::engine::views::carrier::{read_view_carrier, read_view_carrier_definitions};
    let carrier = read_view_carrier(doc, entity)?;
    let mut props = Vec::new();

    props.push(crate::entities::common::ro_prop(
        crate::tr!("aec", "prop-type").as_ref(),
        "carrier_type",
        crate::tr!("aec", "carrier-type"),
    ));

    let defs = read_view_carrier_definitions(doc, carrier.handle);
    if let Some((ref vp, _)) = defs {
        props.push(crate::scene::model::object::Property {
            label: crate::tr!("aec", "prop-label"),
            field: "carrier_name",
            value: crate::scene::model::object::PropValue::PlainText(vp.name.clone()),
        });
    }

    props.push(crate::entities::common::ro_prop(
        crate::tr!("aec", "prop-viewpoint-id").as_ref(),
        "carrier_vp_id",
        carrier.viewpoint_id.as_uuid().to_string(),
    ));
    props.push(crate::entities::common::ro_prop(
        crate::tr!("aec", "prop-model-view-id").as_ref(),
        "carrier_mv_id",
        carrier.model_view_id.as_uuid().to_string(),
    ));
    let scale_val = defs.as_ref().map(|(_, mv)| mv.scale.get()).unwrap_or(100.0);
    props.push(crate::entities::common::edit_scalar_prop(
        crate::tr!("aec", "prop-scale").as_ref(),
        "carrier_scale",
        scale_val,
    ));

    // Show only the plan art stored on the view. A missing id must not look
    // like the first section plan, or applying the inspector would rebind it.
    let current_config_name = if let Some((_, ref mv)) = defs {
        dc_lib
            .configs
            .iter()
            .find(|c| c.id == mv.profile_id.as_uuid())
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "—".to_string())
    } else {
        "—".to_string()
    };

    let mut config_options: Vec<String> = dc_lib.configs.iter().map(|c| c.name.clone()).collect();
    if config_options.is_empty() {
        config_options.push(current_config_name.clone());
    } else if !config_options.contains(&current_config_name) {
        config_options.insert(0, current_config_name.clone());
    }

    props.push(crate::scene::model::object::Property {
        label: crate::tr!("aec", "prop-display-config"),
        field: "carrier_display_config",
        value: crate::scene::model::object::PropValue::Choice {
            selected: current_config_name,
            options: config_options,
        },
    });

    if let Some((ref vp, _)) = defs {
        match &vp.kind {
            crate::modules::aec::engine::views::viewpoint::ViewKind::Section { far_depth, .. } => {
                let depth_val = far_depth.as_ref().map(|d| d.get()).unwrap_or(0.0);
                props.push(crate::entities::common::edit_prop(
                    crate::tr!("aec", "prop-far-depth").as_ref(),
                    "carrier_far_depth",
                    depth_val,
                ));
            }
            crate::modules::aec::engine::views::viewpoint::ViewKind::Elevation { depth } => {
                let depth_val = depth.far.as_ref().map(|d| d.get()).unwrap_or(0.0);
                props.push(crate::entities::common::edit_prop(
                    crate::tr!("aec", "prop-far-depth").as_ref(),
                    "carrier_far_depth",
                    depth_val,
                ));
            }
            crate::modules::aec::engine::views::viewpoint::ViewKind::FloorPlan { range } => {
                let cut_h = (vp.z_range.z_min.get() + range.below_cut.get())
                    .clamp(vp.z_range.z_min.get(), vp.z_range.z_max.get());
                props.push(crate::entities::common::edit_prop(
                    crate::tr!("aec", "prop-cut-height").as_ref(),
                    "carrier_cut_height",
                    cut_h,
                ));
            }
            crate::modules::aec::engine::views::viewpoint::ViewKind::ReflectedCeilingPlan { range } => {
                let cut_h = (vp.z_range.z_max.get() - range.above_cut.get())
                    .clamp(vp.z_range.z_min.get(), vp.z_range.z_max.get());
                props.push(crate::entities::common::edit_prop(
                    crate::tr!("aec", "prop-cut-height").as_ref(),
                    "carrier_cut_height",
                    cut_h,
                ));
            }
        }
        props.push(crate::entities::common::edit_prop(
            crate::tr!("aec", "prop-z-min").as_ref(),
            "carrier_z_min",
            vp.z_range.z_min.get(),
        ));
        props.push(crate::entities::common::edit_prop(
            crate::tr!("aec", "prop-z-max").as_ref(),
            "carrier_z_max",
            vp.z_range.z_max.get(),
        ));
    }

    props.push(crate::entities::common::ro_prop(
        crate::tr!("aec", "prop-status").as_ref(),
        "carrier_status",
        if carrier.is_outdated {
            crate::tr!("aec", "carrier-status-outdated")
        } else {
            crate::tr!("aec", "carrier-status-current")
        },
    ));
    let policy_auto = crate::tr!("aec", "carrier-policy-automatic");
    let policy_manual = crate::tr!("aec", "carrier-policy-manual");
    props.push(crate::scene::model::object::Property {
        label: crate::tr!("aec", "prop-update-policy"),
        field: "carrier_update_policy",
        value: crate::scene::model::object::PropValue::Choice {
            selected: match carrier.update_policy {
                crate::modules::aec::engine::views::model_view::UpdatePolicy::Automatic => {
                    policy_auto.clone()
                }
                crate::modules::aec::engine::views::model_view::UpdatePolicy::Manual => {
                    policy_manual.clone()
                }
            },
            options: vec![policy_auto, policy_manual],
        },
    });
    let action_none = crate::tr!("aec", "carrier-action-none");
    let action_update = crate::tr!("aec", "carrier-action-update");
    props.push(crate::scene::model::object::Property {
        label: crate::tr!("aec", "prop-action"),
        field: "carrier_action",
        value: crate::scene::model::object::PropValue::Choice {
            selected: action_none.clone(),
            options: vec![action_none, action_update],
        },
    });

    let marker_h = carrier.marker_handle.or_else(|| {
        crate::modules::aec::engine::owner_index::peers_of(doc, carrier.handle).into_iter().next()
    });
    if let Some(mh) = marker_h {
        let is_elev = doc
            .get_entity(mh)
            .and_then(|e| crate::modules::aec::views::din1356::read_section_marker(doc, e))
            .map(|m| m.is_elevation)
            .unwrap_or(false);
        let title = if is_elev {
            crate::tr!("aec", "elevation-marker-title")
        } else {
            crate::tr!("aec", "section-marker-title")
        };
        let display = format!("{} #{}", title, mh);
        props.push(crate::scene::model::object::Property {
            label: title.to_string(),
            field: "carrier_marker_entity",
            value: crate::scene::model::object::PropValue::EntityRef {
                display,
                handle: mh,
            },
        });
    }

    Some(crate::scene::model::object::PropSection {
        title: crate::tr!("aec", "view-carrier-title"),
        props,
    })
}

pub fn marker_prop_section(
    entity: &EntityType,
    doc: &CadDocument,
) -> Option<crate::scene::model::object::PropSection> {
    use crate::modules::aec::views::din1356::read_section_marker;
    let marker = read_section_marker(doc, entity)?;
    let mut props = Vec::new();

    props.push(crate::entities::common::ro_prop(
        crate::tr!("aec", "prop-type").as_ref(),
        "marker_type",
        if marker.is_elevation {
            crate::tr!("aec", "marker-type-elevation")
        } else {
            crate::tr!("aec", "marker-type-section")
        },
    ));
    props.push(crate::scene::model::object::Property {
        label: crate::tr!("aec", "prop-label"),
        field: "marker_label",
        value: crate::scene::model::object::PropValue::PlainText(marker.label.clone()),
    });
    props.push(crate::entities::common::edit_prop(
        crate::tr!("aec", "prop-far-depth").as_ref(),
        "marker_far_depth",
        marker.far_depth.unwrap_or(0.0),
    ));
    props.push(crate::entities::common::edit_prop(
        crate::tr!("aec", "prop-z-min").as_ref(),
        "marker_z_min",
        marker.z_min,
    ));
    props.push(crate::entities::common::edit_prop(
        crate::tr!("aec", "prop-z-max").as_ref(),
        "marker_z_max",
        marker.z_max,
    ));
    let flip_normal = crate::tr!("aec", "marker-flip-normal");
    let flip_flipped = crate::tr!("aec", "marker-flip-flipped");
    props.push(crate::scene::model::object::Property {
        label: crate::tr!("aec", "prop-view-direction"),
        field: "marker_flip",
        value: crate::scene::model::object::PropValue::Choice {
            selected: if marker.flip_direction {
                flip_flipped.clone()
            } else {
                flip_normal.clone()
            },
            options: vec![flip_normal, flip_flipped],
        },
    });
    props.push(crate::entities::common::ro_prop(
        crate::tr!("aec", "prop-viewpoint-id").as_ref(),
        "marker_vp_id",
        marker.viewpoint_id.as_uuid().to_string(),
    ));
    let carrier_h = marker.carrier_handle.or_else(|| {
        crate::modules::aec::engine::owner_index::peers_of(doc, entity.common().handle).into_iter().next()
    });
    if let Some(ch) = carrier_h {
        let title = crate::tr!("aec", "view-carrier-title");
        let display = format!("{} #{}", title, ch);
        props.push(crate::scene::model::object::Property {
            label: crate::tr!("aec", "prop-carrier-handle"),
            field: "marker_carrier",
            value: crate::scene::model::object::PropValue::EntityRef {
                display,
                handle: ch,
            },
        });
    } else {
        props.push(crate::entities::common::ro_prop(
            crate::tr!("aec", "prop-carrier-handle").as_ref(),
            "marker_carrier",
            "-".to_string(),
        ));
    }

    Some(crate::scene::model::object::PropSection {
        title: if marker.is_elevation {
            crate::tr!("aec", "elevation-marker-title")
        } else {
            crate::tr!("aec", "section-marker-title")
        },
        props,
    })
}

pub fn derived_primitive_prop_section(
    entity: &EntityType,
) -> Option<crate::scene::model::object::PropSection> {
    let meta = crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(entity)?;

    let mut props = Vec::new();
    props.push(crate::entities::common::ro_prop("Rolle", "prim_role", format!("{:?}", meta.role)));
    if let Some(ref elem_ref) = meta.element_ref {
        props.push(crate::entities::common::ro_prop("Modell ID", "prim_model_id", elem_ref.model_id.as_uuid().to_string()));
        props.push(crate::entities::common::ro_prop("Element ID", "prim_elem_id", elem_ref.element_id.as_uuid().to_string()));
    }
    if let Some(feat) = meta.feature {
        props.push(crate::entities::common::ro_prop("Feature", "prim_feature", format!("{:?}", feat)));
    }
    if let Some(ref xref) = meta.xref_name {
        props.push(crate::entities::common::ro_prop("XREF Quelle", "prim_xref", xref.clone()));
    }
    if let Some(source_h) = meta.source_handle.or_else(|| meta.element_ref.and_then(|r| crate::modules::aec::engine::views::scene_collector::element_id_to_handle(r.element_id))) {
        let display = format!("Bauteil #{}", source_h);
        props.push(crate::scene::model::object::Property {
            label: "Quell-Bauteil".to_string(),
            field: "prim_source_entity",
            value: crate::scene::model::object::PropValue::EntityRef {
                display,
                handle: source_h,
            },
        });
    }
    if let Some(carrier_h) = meta.carrier_handle {
        let title = crate::tr!("aec", "view-carrier-title");
        let display = format!("{} #{}", title, carrier_h);
        props.push(crate::scene::model::object::Property {
            label: crate::tr!("aec", "prop-carrier-handle"),
            field: "prim_carrier_entity",
            value: crate::scene::model::object::PropValue::EntityRef {
                display,
                handle: carrier_h,
            },
        });
    }

    if props.is_empty() {
        return None;
    }

    Some(crate::scene::model::object::PropSection {
        title: "AEC Assoziiertes Bauteil".to_string(),
        props,
    })
}

fn entity_boundary_xy(entity: &EntityType) -> Vec<(f64, f64)> {
    match entity {
        EntityType::LwPolyline(pl) => pl
            .vertices
            .iter()
            .map(|v| (v.location.x, v.location.y))
            .collect(),
        _ => Vec::new(),
    }
}

/// Builds the "Slab" property section for a carrier entity with `SLAB` XDATA.
pub fn slab_prop_section(
    entity: &EntityType,
    style_library: Option<&StyleLibrary>,
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> Option<crate::scene::model::object::PropSection> {
    let slab = slab_xdata::slab_from_entity(entity)?;
    let style_name = style_library
        .and_then(|lib| lib.slab_styles.iter().find(|ss| ss.style.id == slab.style_id))
        .map(|ss| ss.style.name.clone())
        .unwrap_or_else(|| slab.style_id.clone());

    let mut plane_options =
        crate::modules::aec::project::wall_planes::build_project_plane_options(project);
    let top_label = crate::modules::aec::project::wall_planes::resolve_plane_display_label(
        project,
        slab.top_plane_id,
        slab.top_plane_name.as_deref(),
    );
    let ensure_option = |opts: &mut Vec<String>, label: &str| {
        if !opts.iter().any(|n| n == label) {
            opts.push(label.to_string());
        }
    };
    ensure_option(&mut plane_options, &top_label);

    let boundary = entity_boundary_xy(entity);
    let area = if boundary.len() >= 3 {
        Slab::area(&boundary)
    } else {
        0.0
    };
    let peri = if boundary.len() >= 2 {
        Slab::perimeter(&boundary)
    } else {
        0.0
    };
    let vol = if boundary.len() >= 3 {
        slab.volume(&boundary)
    } else {
        0.0
    };

    let none_label = "- (Standard)".to_string();

    let mut struct_style_options = vec![none_label.clone()];
    if let Some(lib) = style_library {
        for ss in &lib.slab_structural_styles {
            let label = if !ss.style.name.is_empty() {
                ss.style.name.clone()
            } else {
                ss.style.id.clone()
            };
            if !struct_style_options.contains(&label) {
                struct_style_options.push(label);
            }
        }
    }
    for ss in library::seed_slab_structural_styles() {
        if !struct_style_options.contains(&ss.style.name) {
            struct_style_options.push(ss.style.name);
        }
    }
    let selected_struct = slab.structural_style_id.as_deref().and_then(|id| {
        if let Some(lib) = style_library {
            if let Some(ss) = lib.find_slab_structural_style(id) {
                return Some(ss.style.name.clone());
            }
        }
        for ss in library::seed_slab_structural_styles() {
            if ss.style.id == id || ss.style.name == id {
                return Some(ss.style.name);
            }
        }
        Some(id.to_string())
    }).unwrap_or_else(|| none_label.clone());
    if !struct_style_options.contains(&selected_struct) {
        struct_style_options.push(selected_struct.clone());
    }

    let mut finish_style_options = vec![none_label.clone()];
    if let Some(lib) = style_library {
        for fs in &lib.floor_finish_styles {
            let label = if !fs.style.name.is_empty() {
                fs.style.name.clone()
            } else {
                fs.style.id.clone()
            };
            if !finish_style_options.contains(&label) {
                finish_style_options.push(label);
            }
        }
    }
    for fs in library::seed_floor_finish_styles() {
        if !finish_style_options.contains(&fs.style.name) {
            finish_style_options.push(fs.style.name);
        }
    }
    let selected_finish = slab.finish_style_id.as_deref().and_then(|id| {
        if let Some(lib) = style_library {
            if let Some(fs) = lib.find_floor_finish_style(id) {
                return Some(fs.style.name.clone());
            }
        }
        for fs in library::seed_floor_finish_styles() {
            if fs.style.id == id || fs.style.name == id {
                return Some(fs.style.name);
            }
        }
        Some(id.to_string())
    }).unwrap_or_else(|| none_label.clone());
    if !finish_style_options.contains(&selected_finish) {
        finish_style_options.push(selected_finish.clone());
    }

    let mut props = vec![
        crate::scene::model::object::Property {
            label: crate::tr!("aec", "slab-style"),
            field: "slab_style",
            value: crate::scene::model::object::PropValue::Picker {
                value: style_name,
                handles: vec![entity.common().handle],
            },
        },
        crate::scene::model::object::Property {
            label: crate::tr!("aec", "slab-structural-style"),
            field: "slab_structural_style",
            value: crate::scene::model::object::PropValue::Choice {
                selected: selected_struct,
                options: struct_style_options,
            },
        },
        crate::scene::model::object::Property {
            label: crate::tr!("aec", "slab-finish-style"),
            field: "slab_finish_style",
            value: crate::scene::model::object::PropValue::Choice {
                selected: selected_finish,
                options: finish_style_options,
            },
        },
        crate::scene::model::object::Property {
            label: crate::tr!("aec", "slab-justification"),
            field: "slab_justification",
            value: crate::scene::model::object::PropValue::Choice {
                selected: slab.justification.localized_name(),
                options: vec![
                    SlabJustification::Top.localized_name(),
                    SlabJustification::StructuralTop.localized_name(),
                    SlabJustification::Bottom.localized_name(),
                ],
            },
        },
        crate::scene::model::object::Property {
            label: crate::tr!("aec", "slab-top-plane"),
            field: "slab_top_plane",
            value: crate::scene::model::object::PropValue::Choice {
                selected: top_label,
                options: plane_options,
            },
        },
        formula_edit_prop_with_aliases(
            crate::tr!("aec", "slab-top-offset").as_ref(),
            "slab_top_offset",
            SLAB_TOP_OFFSET_KEYS,
            slab.top_offset,
            &slab.formulas,
        ),
        crate::scene::model::object::Property {
            label: t!("Phase").into_owned(),
            field: "slab_phase",
            value: crate::scene::model::object::PropValue::Choice {
                selected: slab.phase.display_label().to_string(),
                options: vec![
                    crate::modules::aec::engine::plan_view::PlanPhase::New
                        .display_label()
                        .to_string(),
                    crate::modules::aec::engine::plan_view::PlanPhase::Demolition
                        .display_label()
                        .to_string(),
                    crate::modules::aec::engine::plan_view::PlanPhase::Existing
                        .display_label()
                        .to_string(),
                ],
            },
        },
        crate::entities::common::ro_prop(
            crate::tr!("aec", "slab-thickness").as_ref(),
            "slab_thickness",
            format!("{:.3} m", slab.total_thickness()),
        ),
        crate::entities::common::ro_prop(
            crate::tr!("aec", "slab-area").as_ref(),
            "slab_area",
            format!("{:.3} m²", area),
        ),
        crate::entities::common::ro_prop(
            crate::tr!("aec", "slab-perimeter").as_ref(),
            "slab_perimeter",
            format!("{:.3} m", peri),
        ),
        crate::entities::common::ro_prop(
            crate::tr!("aec", "slab-volume").as_ref(),
            "slab_volume",
            format!("{:.3} m³", vol),
        ),
        crate::entities::common::ro_prop(
            crate::tr!("aec", "slab-storey").as_ref(),
            "slab_storey",
            slab.storey_id.to_string(),
        ),
    ];

    for (i, layer) in slab.layers.iter().enumerate() {
        let layer_info = format!(
            "{} — {:.3} m ({})",
            layer.material,
            layer.thickness,
            slab_xdata::layer_function_to_str(&layer.function)
        );
        props.push(crate::entities::common::ro_prop(
            crate::tr!("aec", "slab-layer").as_ref(),
            "slab_layer",
            format!("{} — {}", i + 1, layer_info),
        ));
    }

    Some(crate::scene::model::object::PropSection {
        title: crate::tr!("aec", "slab-section"),
        props,
    })
}

/// Builds the "Slab Opening" property section.
pub fn slab_opening_prop_section(
    opening: &SlabOpening,
    opening_handle: Handle,
    entity: &EntityType,
) -> crate::scene::model::object::PropSection {
    let boundary = if opening.boundary.len() >= 3 {
        opening.boundary.clone()
    } else {
        entity_boundary_xy(entity)
    };
    let area = if boundary.len() >= 3 {
        crate::modules::aec::engine::geometry::area(&boundary)
    } else {
        opening.area()
    };
    let peri = if boundary.len() >= 2 {
        crate::modules::aec::engine::geometry::perimeter(&boundary)
    } else {
        opening.perimeter()
    };

    let mut props = vec![
        crate::scene::model::object::Property {
            label: crate::tr!("aec", "slabopening-kind"),
            field: "slabopening_kind",
            value: crate::scene::model::object::PropValue::Choice {
                selected: opening.kind.as_str().to_string(),
                options: SlabOpeningKind::all()
                    .iter()
                    .map(|k| k.as_str().to_string())
                    .collect(),
            },
        },
        crate::scene::model::object::Property {
            label: crate::tr!("aec", "slabopening-depth-mode"),
            field: "slabopening_depth_mode",
            value: crate::scene::model::object::PropValue::Choice {
                selected: opening.depth.as_str().to_string(),
                options: SlabOpeningDepth::all_modes()
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect(),
            },
        },
    ];
    if let SlabOpeningDepth::Recess(d) = opening.depth {
        props.push(crate::entities::common::edit_prop(
            crate::tr!("aec", "slabopening-recess-depth").as_ref(),
            "slabopening_recess_depth",
            d,
        ));
    }
    props.push(crate::entities::common::ro_prop(
        crate::tr!("aec", "slabopening-host").as_ref(),
        "slabopening_host",
        format!("{:X}", opening.host_slab.value()),
    ));
    props.push(crate::entities::common::ro_prop(
        crate::tr!("aec", "slabopening-area").as_ref(),
        "slabopening_area",
        format!("{:.3} m²", area),
    ));
    props.push(crate::entities::common::ro_prop(
        crate::tr!("aec", "slabopening-perimeter").as_ref(),
        "slabopening_perimeter",
        format!("{:.3} m", peri),
    ));
    let _ = opening_handle;
    crate::scene::model::object::PropSection {
        title: crate::tr!("aec", "slabopening-section"),
        props,
    }
}

/// Builds the "Room" property section for a carrier entity with `ROOM` XDATA.
pub fn room_prop_section(
    entity: &EntityType,
    style_library: Option<&StyleLibrary>,
) -> Option<crate::scene::model::object::PropSection> {
    let room = room_xdata::room_from_entity(entity)?;
    let mut props = Vec::new();
    props.push(crate::scene::model::object::Property {
        label: crate::tr!("aec", "room-name"),
        field: "room_name",
        value: crate::scene::model::object::PropValue::EditText(room.name.clone()),
    });
    props.push(crate::scene::model::object::Property {
        label: crate::tr!("aec", "room-number"),
        field: "room_number",
        value: crate::scene::model::object::PropValue::EditText(room.number.clone()),
    });
    props.push(crate::scene::model::object::Property {
        label: crate::tr!("aec", "room-function"),
        field: "room_function",
        value: crate::scene::model::object::PropValue::Choice {
            selected: room.function.localized_name(),
            options: crate::modules::aec::engine::room::RoomFunction::BUILT_IN
                .iter()
                .map(|f| f.localized_name())
                .collect(),
        },
    });
    let (factor_label, factor_val) = if let Some(formula) = room.formulas.get("room_factor") {
        (
            format!("{} (fx = {:.4})", crate::tr!("aec", "room-factor"), room.factor),
            formula.to_string(),
        )
    } else {
        (crate::tr!("aec", "room-factor"), format!("{:.4}", room.factor))
    };
    props.push(crate::scene::model::object::Property {
        label: factor_label,
        field: "room_factor",
        value: crate::scene::model::object::PropValue::EditText(factor_val),
    });
    props.push(formula_edit_prop(
        &crate::tr!("aec", "room-height"),
        "room_clear_height",
        room.clear_height,
        &room.formulas,
    ));

    // Modular floor finish style choice
    let none_label = "- (Standard)".to_string();
    let mut finish_style_options = vec![none_label.clone()];
    if let Some(lib) = style_library {
        for fs in &lib.floor_finish_styles {
            let label = if !fs.style.name.is_empty() {
                fs.style.name.clone()
            } else {
                fs.style.id.clone()
            };
            if !finish_style_options.contains(&label) {
                finish_style_options.push(label);
            }
        }
    }
    for fs in library::seed_floor_finish_styles() {
        if !finish_style_options.contains(&fs.style.name) {
            finish_style_options.push(fs.style.name);
        }
    }
    let selected_style = room.finish_style_id.as_deref().and_then(|id| {
        if let Some(lib) = style_library {
            if let Some(fs) = lib.find_floor_finish_style(id) {
                return Some(fs.style.name.clone());
            }
        }
        for fs in library::seed_floor_finish_styles() {
            if fs.style.id == id || fs.style.name == id {
                return Some(fs.style.name);
            }
        }
        Some(id.to_string())
    }).unwrap_or_else(|| none_label.clone());
    if !finish_style_options.contains(&selected_style) {
        finish_style_options.push(selected_style.clone());
    }

    props.push(crate::scene::model::object::Property {
        label: crate::tr!("aec", "room-floor-finish-style"),
        field: "room_floor_finish_style",
        value: crate::scene::model::object::PropValue::Choice {
            selected: selected_style,
            options: finish_style_options,
        },
    });

    props.push(crate::scene::model::object::Property {
        label: crate::tr!("aec", "room-floor-finish"),
        field: "room_floor_finish",
        value: crate::scene::model::object::PropValue::EditText(room.floor_finish_summary()),
    });
    props.push(crate::scene::model::object::Property {
        label: crate::tr!("aec", "room-ceiling-finish"),
        field: "room_ceiling_finish",
        value: crate::scene::model::object::PropValue::EditText(room.ceiling_finish_summary()),
    });
    let (sx, sy) = room.stamp_pos.unwrap_or_else(|| {
        let pts = room_boundary_points(entity);
        if pts.len() >= 3 {
            crate::modules::aec::engine::geometry::centroid(&pts)
        } else {
            (0.0, 0.0)
        }
    });
    props.push(crate::entities::common::edit_prop(
        &crate::tr!("aec", "room-stamp-x"),
        "room_stamp_x",
        sx,
    ));
    props.push(crate::entities::common::edit_prop(
        &crate::tr!("aec", "room-stamp-y"),
        "room_stamp_y",
        sy,
    ));
    props.push(crate::entities::common::ro_prop(
        &crate::tr!("aec", "room-area-gross"),
        "room_area_gross",
        format!("{:.2} m²", room.area),
    ));
    props.push(crate::entities::common::ro_prop(
        &crate::tr!("aec", "room-area-calc"),
        "room_area_calc",
        format!("{:.2} m²", room.calculated_area()),
    ));
    props.push(crate::entities::common::ro_prop(
        &crate::tr!("aec", "room-perimeter"),
        "room_perimeter",
        format!("{:.2} m", room.perimeter),
    ));
    props.push(crate::entities::common::ro_prop(
        &crate::tr!("aec", "room-volume"),
        "room_volume",
        format!("{:.2} m³", room.effective_volume()),
    ));

    Some(crate::scene::model::object::PropSection {
        title: crate::tr!("aec", "room-section"),
        props,
    })
}

pub fn session_styles_for_scene(
    scene: &crate::scene::Scene,
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> Option<StyleLibrary> {
    let extracted = xdata::extract_style_library_from_scene(scene);
    let session =
        crate::modules::aec::engine::library::session_library_excluding_existing(&extracted, project);
    if session.materials.is_empty()
        && session.wall_styles.is_empty()
        && session.opening_styles.is_empty()
        && session.slab_styles.is_empty()
    {
        None
    } else {
        Some(session)
    }
}

pub fn on_document_loaded(
    scene: &crate::scene::Scene,
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> (
    crate::modules::aec::engine::library::DisplayConfigLibrary,
    Option<StyleLibrary>,
) {
    let plan_library =
        crate::modules::aec::engine::project::resolve_display_config_library(project);
    (plan_library, session_styles_for_scene(scene, project))
}

pub fn is_wall_derived_non_axis(scene: &crate::scene::Scene, handle: Handle) -> bool {
    wall_package::is_wall_derived_non_axis(scene, handle)
}

pub fn resolve_section_marker_package(scene: &crate::scene::Scene, handle: Handle) -> Option<Handle> {
    let ent = scene.document.get_entity(handle)?;
    if crate::modules::aec::views::din1356::read_section_marker(&scene.document, ent).is_some() {
        return Some(handle);
    }
    let owner = crate::modules::aec::views::din1356::section_marker_owner_from_entity(ent)?;
    if scene
        .document
        .get_entity(owner)
        .is_some_and(|owner_ent| {
            crate::modules::aec::views::din1356::read_section_marker(&scene.document, owner_ent)
                .is_some()
        })
    {
        Some(owner)
    } else {
        None
    }
}

pub fn is_section_marker_derived(scene: &crate::scene::Scene, handle: Handle) -> bool {
    resolve_section_marker_package(scene, handle).is_some_and(|owner| owner != handle)
}

pub fn expand_handles_for_section_marker_packages(
    scene: &crate::scene::Scene,
    handles: &[Handle],
) -> Vec<Handle> {
    let mut out = Vec::with_capacity(handles.len());
    let mut seen = rustc_hash::FxHashSet::default();
    for &h in handles {
        if let Some(owner) = resolve_section_marker_package(scene, h) {
            if seen.insert(owner) {
                out.push(owner);
            }
            for child in crate::modules::aec::engine::owner_index::children_of(&scene.document, owner) {
                if scene.document.get_entity(child).is_some() && seen.insert(child) {
                    out.push(child);
                }
            }
        } else if seen.insert(h) {
            out.push(h);
        }
    }
    out
}

pub fn is_aec_derived_non_carrier(scene: &crate::scene::Scene, handle: Handle) -> bool {
    if let Some(ent) = scene.document.get_entity(handle) {
        if crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(ent).is_some() {
            return false;
        }
    }
    wall_package::is_wall_derived_non_axis(scene, handle)
        || slab_package::is_slab_derived(scene, handle)
        || slab_package::slab_opening_owner_if_any(scene, handle).is_some_and(|owner| owner != handle)
        || opening_display::opening_owner_if_any(scene, handle).is_some_and(|owner| owner != handle)
        || scene.document.get_entity(handle).map(room_package::is_room_derived).unwrap_or(false)
        || is_section_marker_derived(scene, handle)
}

pub fn resolve_wall_package(scene: &crate::scene::Scene, handle: Handle) -> Handle {
    wall_package::resolve_wall_package(scene, handle)
}

pub fn expand_handles_for_aec_packages(scene: &crate::scene::Scene, handles: &[Handle]) -> Vec<Handle> {
    let handles = wall_package::expand_handles_for_wall_packages(scene, handles);
    let handles = slab_package::expand_handles_for_slab_packages(scene, &handles);
    let handles = room_package::expand_handles_for_room_packages(scene, &handles);
    expand_handles_for_section_marker_packages(scene, &handles)
}

pub fn resolve_aec_package(scene: &crate::scene::Scene, handle: Handle) -> Handle {
    if let Some(ent) = scene.document.get_entity(handle) {
        if let Some(owner) = crate::modules::aec::engine::views::carrier::badge_carrier_handle(ent) {
            return owner;
        }
        if crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(ent).is_some() {
            return handle;
        }
    }
    if let Some(opening_owner) = opening_display::opening_owner_if_any(scene, handle) {
        return opening_owner;
    }
    if let Some(slab_opening_owner) = slab_package::slab_opening_owner_if_any(scene, handle) {
        return slab_opening_owner;
    }
    if let Some(ent) = scene.document.get_entity(handle) {
        if let Some(room_owner) = room_package::resolve_room_package(ent) {
            return room_owner;
        }
    }
    if let Some(marker_owner) = resolve_section_marker_package(scene, handle) {
        if marker_owner != handle {
            return marker_owner;
        }
    }
    let wall_owner = wall_package::resolve_wall_package(scene, handle);
    if wall_owner != handle {
        return wall_owner;
    }
    let slab_owner = slab_package::resolve_slab_package(scene, handle);
    if slab_owner != handle {
        return slab_owner;
    }
    handle
}

/// The single wall/slab/slab-opening/opening package that all `handles`
/// resolve to, if any.
pub fn single_selected_package(
    scene: &crate::scene::Scene,
    handles: impl IntoIterator<Item = Handle>,
) -> Option<Handle> {
    let owners: Vec<Handle> = handles
        .into_iter()
        .map(|h| resolve_aec_package(scene, h))
        .collect();
    owners.first().copied().filter(|owner| {
        !owner.is_null()
            && owners.iter().all(|h| h == owner)
            && scene.document.get_entity(*owner).is_some_and(|e| {
                wall_from_entity(e)
                    || slab_from_entity(e)
                    || slab_opening_from_entity(e)
                    || opening_from_entity(e)
            })
    })
}

pub fn wall_from_entity(entity: &EntityType) -> bool {
    xdata::wall_from_entity(entity).is_some()
}

pub fn slab_from_entity(entity: &EntityType) -> bool {
    slab_xdata::slab_from_entity(entity).is_some()
}

pub fn slab_opening_from_entity(entity: &EntityType) -> bool {
    slab_xdata::slab_opening_from_entity(entity).is_some()
}

pub fn opening_from_entity(entity: &EntityType) -> bool {
    opening_xdata::opening_from_entity(entity, entity.common().handle).is_some()
}

pub fn room_from_entity(entity: &EntityType) -> bool {
    room_xdata::room_from_entity(entity).is_some()
}

pub fn selection_is_all_walls(
    scene: &crate::scene::Scene,
    handles: impl IntoIterator<Item = Handle>,
) -> bool {
    let mut any = false;
    for h in handles {
        any = true;
        let resolved = wall_package::resolve_wall_package(scene, h);
        let Some(entity) = scene.document.get_entity(resolved) else {
            return false;
        };
        if xdata::wall_from_entity(entity).is_none() {
            return false;
        }
    }
    any
}

const CHOICE_NORMAL: &str = "normal";
const CHOICE_FLIPPED: &str = "flipped";
const CHOICE_AUTOMATIC: &str = "automatic";
const CHOICE_MANUAL: &str = "manual";
const CHOICE_ACTION_NONE: &str = "-";
const CHOICE_ACTION_UPDATE: &str = "update-now";

fn choice_matches(val: &str, id: &str, localized: &str) -> bool {
    let trimmed = val.trim();
    trimmed.eq_ignore_ascii_case(id) || trimmed == localized
}

/// Parses a viewing-depth field. `0` and negative values mean unlimited (`None`).
/// Returns `None` when the input cannot be parsed as a finite number.
fn parse_view_depth(val: &str) -> Option<Option<f64>> {
    let v = crate::entities::common::parse_f64(val)?;
    if !v.is_finite() {
        return None;
    }
    if v > 0.0 {
        Some(Some(v))
    } else {
        Some(None)
    }
}

fn parse_finite_metres(val: &str) -> Option<f64> {
    let v = crate::entities::common::parse_f64(val)?;
    v.is_finite().then_some(v)
}

fn is_flipped_choice(val: &str) -> bool {
    let trimmed = val.trim();
    choice_matches(trimmed, CHOICE_FLIPPED, &crate::tr!("aec", "marker-flip-flipped"))
        || trimmed.eq_ignore_ascii_case("true")
        || trimmed == "1"
}

fn is_normal_choice(val: &str) -> bool {
    let trimmed = val.trim();
    choice_matches(trimmed, CHOICE_NORMAL, &crate::tr!("aec", "marker-flip-normal"))
        || trimmed.eq_ignore_ascii_case("false")
        || trimmed == "0"
}

/// Applies an editable property to a `Din1356SectionMarker` entity.
/// Synchronizes the CAD entities and associated carrier/viewpoint definitions.
pub fn apply_marker_property(
    doc: &mut CadDocument,
    marker_handle: Handle,
    field: &str,
    val: &str,
) -> bool {
    let display_config_library = crate::modules::aec::engine::library::DisplayConfigLibrary::seed_default_configs();
    let style_library = crate::modules::aec::engine::library::seed_default_library();
    apply_marker_property_with_libraries(
        doc,
        marker_handle,
        field,
        val,
        &display_config_library,
        &style_library,
        None,
    )
}

pub fn apply_marker_property_with_libraries(
    doc: &mut CadDocument,
    marker_handle: Handle,
    field: &str,
    val: &str,
    display_config_library: &crate::modules::aec::engine::library::DisplayConfigLibrary,
    style_library: &crate::modules::aec::engine::library::StyleLibrary,
    active_name: Option<&str>,
) -> bool {
    use crate::modules::aec::views::din1356::{
        read_section_marker, sync_marker_to_carrier_with_libraries,
        sync_section_marker_cad_entities,
    };

    let Some(entity) = doc.get_entity(marker_handle) else {
        return false;
    };
    let Some(mut marker) = read_section_marker(doc, entity) else {
        return false;
    };

    match field {
        "marker_label" => {
            let name = val.trim();
            if name.is_empty() {
                return false;
            }
            marker.label = name.to_string();
        }
        "marker_far_depth" => {
            let Some(depth) = parse_view_depth(val) else {
                return false;
            };
            marker.far_depth = depth;
        }
        "marker_z_min" => {
            let Some(v) = parse_finite_metres(val) else {
                return false;
            };
            if v > marker.z_max {
                return false;
            }
            marker.z_min = v;
        }
        "marker_z_max" => {
            let Some(v) = parse_finite_metres(val) else {
                return false;
            };
            if v < marker.z_min {
                return false;
            }
            marker.z_max = v;
        }
        "marker_flip" => {
            if is_flipped_choice(val) {
                marker.flip_direction = true;
            } else if is_normal_choice(val) {
                marker.flip_direction = false;
            } else {
                return false;
            }
        }
        _ => return false,
    }

    sync_section_marker_cad_entities(doc, marker_handle, &marker);
    sync_marker_to_carrier_with_libraries(
        doc,
        &marker,
        display_config_library,
        style_library,
        active_name,
    );

    true
}

/// Applies an editable property to a `DerivedViewCarrier` entity.
/// Optionally regenerates the view carrier or marks it outdated.
pub fn apply_carrier_property(
    doc: &mut CadDocument,
    carrier_handle: Handle,
    field: &str,
    val: &str,
) -> bool {
    let display_config_library = crate::modules::aec::engine::library::DisplayConfigLibrary::seed_default_configs();
    let style_library = crate::modules::aec::engine::library::seed_default_library();
    apply_carrier_property_with_libraries(
        doc,
        carrier_handle,
        field,
        val,
        &display_config_library,
        &style_library,
        None,
    )
}

pub fn apply_carrier_property_with_libraries(
    doc: &mut CadDocument,
    carrier_handle: Handle,
    field: &str,
    val: &str,
    display_config_library: &crate::modules::aec::engine::library::DisplayConfigLibrary,
    style_library: &crate::modules::aec::engine::library::StyleLibrary,
    active_name: Option<&str>,
) -> bool {
    use crate::modules::aec::engine::views::carrier::{
        find_view_carrier_by_handle, read_view_carrier_definitions, write_view_carrier,
        write_view_carrier_definitions,
    };
    use crate::modules::aec::engine::views::model_view::UpdatePolicy;
    use crate::modules::aec::engine::views::update_ops::regenerate_or_mark_outdated_with_libraries;
    use crate::modules::aec::engine::views::viewpoint::VerticalRange;
    use crate::modules::aec::engine::views::{Metres, PositiveMetres, ScaleDenominator};
    use crate::modules::aec::views::din1356::{
        find_section_marker_by_carrier, sync_section_marker_cad_entities,
    };

    let Some(mut carrier) = find_view_carrier_by_handle(doc, carrier_handle) else {
        return false;
    };

    let mut defs = read_view_carrier_definitions(doc, carrier_handle);
    let mut needs_regen = false;
    let mut linked_marker = find_section_marker_by_carrier(doc, carrier_handle);

    match field {
        "carrier_name" => {
            let name = val.trim();
            if name.is_empty() {
                return false;
            }
            let Some((ref mut vp, ref mut mv)) = defs else {
                return false;
            };
            vp.name = name.to_string();
            mv.name = match &vp.kind {
                crate::modules::aec::engine::views::viewpoint::ViewKind::Section { .. } => {
                    format!("Section {}", name)
                }
                crate::modules::aec::engine::views::viewpoint::ViewKind::Elevation { .. } => {
                    format!("Elevation {}", name)
                }
                _ => name.to_string(),
            };
            if let Some((_, ref mut marker)) = linked_marker {
                marker.label = name.to_string();
            }
        }
        "carrier_scale" => {
            let Some(v) = crate::entities::common::parse_f64(val) else {
                return false;
            };
            let Ok(scale_denom) = ScaleDenominator::new(v) else {
                return false;
            };
            let Some((_, ref mut mv)) = defs else {
                return false;
            };
            mv.scale = scale_denom;
            if carrier.update_policy == UpdatePolicy::Automatic {
                needs_regen = true;
            } else {
                carrier.is_outdated = true;
            }
        }
        "carrier_display_config" => {
            let Some(matched_config) = display_config_library.configs.iter().find(|c| c.name == val) else {
                return false;
            };
            let Some((_, ref mut mv)) = defs else {
                return false;
            };
            mv.profile_id = crate::modules::aec::engine::views::ViewProfileId::from_uuid(matched_config.id);
            if carrier.update_policy == UpdatePolicy::Automatic {
                needs_regen = true;
            } else {
                carrier.is_outdated = true;
            }
        }
        "carrier_update_policy" => {
            if choice_matches(val, CHOICE_AUTOMATIC, &crate::tr!("aec", "carrier-policy-automatic")) {
                carrier.update_policy = UpdatePolicy::Automatic;
                if carrier.is_outdated {
                    needs_regen = true;
                }
            } else if choice_matches(val, CHOICE_MANUAL, &crate::tr!("aec", "carrier-policy-manual")) {
                carrier.update_policy = UpdatePolicy::Manual;
            } else {
                return false;
            }
        }
        "carrier_far_depth" => {
            let Some(depth) = parse_view_depth(val) else {
                return false;
            };
            let Some((ref mut vp, _)) = defs else {
                return false;
            };
            match &mut vp.kind {
                crate::modules::aec::engine::views::viewpoint::ViewKind::Section { far_depth, .. } => {
                    *far_depth = depth.and_then(|d| PositiveMetres::new(d).ok());
                }
                crate::modules::aec::engine::views::viewpoint::ViewKind::Elevation { depth: d } => {
                    d.far = depth.and_then(|d| PositiveMetres::new(d).ok());
                }
                _ => return false,
            }
            if let Some((_, ref mut marker)) = linked_marker {
                marker.far_depth = depth;
            }
            if carrier.update_policy == UpdatePolicy::Automatic {
                needs_regen = true;
            } else {
                carrier.is_outdated = true;
            }
        }
        "carrier_cut_height" | "view_cut_height" => {
            let Some(new_cut_h) = parse_finite_metres(val) else {
                return false;
            };
            let Some((ref mut vp, _)) = defs else {
                return false;
            };
            let z_min = vp.z_range.z_min.get();
            let z_max = vp.z_range.z_max.get();
            if new_cut_h < z_min || new_cut_h > z_max {
                return false;
            }
            match &mut vp.kind {
                crate::modules::aec::engine::views::viewpoint::ViewKind::FloorPlan { range } => {
                    let offset = (new_cut_h - z_min).max(0.0);
                    if let Ok(m) = crate::modules::aec::engine::views::NonNegativeMetres::new(offset) {
                        range.below_cut = m;
                    }
                    if let crate::modules::aec::engine::views::viewpoint::ViewOrigin::World(ref mut p) = vp.origin {
                        p.z_m = new_cut_h;
                    }
                }
                crate::modules::aec::engine::views::viewpoint::ViewKind::ReflectedCeilingPlan { range } => {
                    let offset = (z_max - new_cut_h).max(0.0);
                    if let Ok(m) = crate::modules::aec::engine::views::NonNegativeMetres::new(offset) {
                        range.above_cut = m;
                    }
                    if let crate::modules::aec::engine::views::viewpoint::ViewOrigin::World(ref mut p) = vp.origin {
                        p.z_m = new_cut_h;
                    }
                }
                _ => return false,
            }
            if carrier.update_policy == UpdatePolicy::Automatic {
                needs_regen = true;
            } else {
                carrier.is_outdated = true;
            }
        }
        "carrier_z_min" | "carrier_z_max" => {
            let Some(v) = parse_finite_metres(val) else {
                return false;
            };
            let Some((ref mut vp, _)) = defs else {
                return false;
            };
            let mut z_min = vp.z_range.z_min.get();
            let mut z_max = vp.z_range.z_max.get();
            if field == "carrier_z_min" {
                z_min = v;
            } else {
                z_max = v;
            }
            if z_min > z_max {
                return false;
            }
            let (Ok(zm), Ok(zx)) = (Metres::new(z_min), Metres::new(z_max)) else {
                return false;
            };
            let Ok(vr) = VerticalRange::new(zm, zx) else {
                return false;
            };
            vp.z_range = vr;
            if let Some((_, ref mut marker)) = linked_marker {
                marker.z_min = z_min;
                marker.z_max = z_max;
            }
            if carrier.update_policy == UpdatePolicy::Automatic {
                needs_regen = true;
            } else {
                carrier.is_outdated = true;
            }
        }
        "carrier_action" => {
            if choice_matches(val, CHOICE_ACTION_UPDATE, &crate::tr!("aec", "carrier-action-update")) {
                needs_regen = true;
            } else if choice_matches(val, CHOICE_ACTION_NONE, &crate::tr!("aec", "carrier-action-none")) {
                // no-op
            } else {
                return false;
            }
        }
        _ => return false,
    }

    if let Some((ref vp, ref mv)) = defs {
        write_view_carrier_definitions(doc, carrier_handle, vp, mv);
    }
    write_view_carrier(doc, &carrier);

    if let Some((m_handle, marker)) = linked_marker {
        match field {
            "carrier_name" | "carrier_far_depth" | "carrier_z_min" | "carrier_z_max" => {
                sync_section_marker_cad_entities(doc, m_handle, &marker);
            }
            _ => {}
        }
    }

    if needs_regen {
        if let Some((ref vp, ref mv)) = defs {
            regenerate_or_mark_outdated_with_libraries(
                doc,
                carrier_handle,
                vp,
                mv,
                display_config_library,
                style_library,
                active_name,
            );
        }
    }

    true
}

pub const SECTION_MARKER_START_GRIP: usize = 0;
pub const SECTION_MARKER_END_GRIP: usize = 1;
pub const SECTION_MARKER_DEPTH_GRIP: usize = 2;
pub const SECTION_MARKER_FLIP_GRIP: usize = 3;
pub const SECTION_MARKER_JOG_BASE: usize = 100;

pub fn append_section_marker_grips(
    scene: &crate::scene::Scene,
    handle: Handle,
    entity_grips: &mut Vec<crate::scene::model::object::GripDef>,
) {
    let marker_handle = resolve_section_marker_package(scene, handle).unwrap_or(handle);
    let Some(entity) = scene.document.get_entity(marker_handle) else {
        return;
    };
    let Some(marker) = crate::modules::aec::views::din1356::read_section_marker(&scene.document, entity) else {
        return;
    };

    let segments = marker.path.segments();
    if segments.is_empty() {
        return;
    }

    entity_grips.clear();

    let z = match entity {
        EntityType::LwPolyline(pl) => pl.elevation,
        _ => 0.0,
    };

    let first = &segments[0];
    let last = &segments[segments.len() - 1];

    // 0: Start grip
    entity_grips.push(crate::entities::common::square_grip(
        SECTION_MARKER_START_GRIP,
        glam::DVec3::new(first.start.x_m, first.start.y_m, z),
    ));

    // 1: End grip
    entity_grips.push(crate::entities::common::square_grip(
        SECTION_MARKER_END_GRIP,
        glam::DVec3::new(last.end.x_m, last.end.y_m, z),
    ));

    // 100+: Intermediate jog transition grips
    for i in 0..segments.len().saturating_sub(1) {
        let jog_pt = glam::DVec3::new(segments[i].end.x_m, segments[i].end.y_m, z);
        entity_grips.push(crate::entities::common::square_grip(
            SECTION_MARKER_JOG_BASE + i,
            jog_pt,
        ));
    }

    // Midpoint of the cut path
    let mid_x = (first.start.x_m + last.end.x_m) * 0.5;
    let mid_y = (first.start.y_m + last.end.y_m) * 0.5;
    let mid = glam::DVec3::new(mid_x, mid_y, z);

    let dir = first.direction().unwrap_or(crate::modules::aec::engine::views::Vector2::new(1.0, 0.0));
    let normal = if !marker.flip_direction {
        crate::modules::aec::engine::views::Vector2::new(-dir.y, dir.x)
    } else {
        crate::modules::aec::engine::views::Vector2::new(dir.y, -dir.x)
    };

    // 2: Depth grip along normal
    if let Some(depth) = marker.far_depth {
        let depth_pos = mid + glam::DVec3::new(normal.x * depth, normal.y * depth, 0.0);
        let mut depth_grip = crate::entities::common::rectangle_grip(
            SECTION_MARKER_DEPTH_GRIP,
            depth_pos,
            [normal.x as f32, normal.y as f32],
        );
        depth_grip.axis = Some(glam::DVec3::new(normal.x, normal.y, 0.0));
        entity_grips.push(depth_grip);
    }

    // 3: Flip grip pointing along normal
    let flip_offset = 1.0;
    let flip_pos = mid + glam::DVec3::new(normal.x * flip_offset, normal.y * flip_offset, 0.0);
    let flip_grip = crate::entities::common::oriented_triangle_grip(
        SECTION_MARKER_FLIP_GRIP,
        flip_pos,
        glam::DVec3::new(normal.x, normal.y, 0.0),
    );
    entity_grips.push(flip_grip);
}

pub const DERIVED_VIEW_GRIP_BASE: usize = 50_000;

/// Appends interactive semantic model grips for derived section primitives.
pub fn append_derived_view_grips(
    scene: &crate::scene::Scene,
    handle: Handle,
    entity_grips: &mut Vec<crate::scene::model::object::GripDef>,
) {
    let Some(entity) = scene.document.get_entity(handle) else {
        return;
    };
    let Some(meta) = crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(entity) else {
        return;
    };
    let Some(feat) = meta.feature else {
        return;
    };
    let is_horizontal_grip = matches!(
        feat,
        crate::modules::aec::engine::views::provenance::FeatureKey::OpeningLeftJamb
            | crate::modules::aec::engine::views::provenance::FeatureKey::OpeningRightJamb
    );

    match feat {
        crate::modules::aec::engine::views::provenance::FeatureKey::WallTop
        | crate::modules::aec::engine::views::provenance::FeatureKey::WallBase
        | crate::modules::aec::engine::views::provenance::FeatureKey::OpeningSill
        | crate::modules::aec::engine::views::provenance::FeatureKey::OpeningHead
        | crate::modules::aec::engine::views::provenance::FeatureKey::OpeningLeftJamb
        | crate::modules::aec::engine::views::provenance::FeatureKey::OpeningRightJamb
        | crate::modules::aec::engine::views::provenance::FeatureKey::SlabTop
        | crate::modules::aec::engine::views::provenance::FeatureKey::SlabBottom => {}
        _ => return,
    }

    let pos = match entity {
        EntityType::Line(l) => {
            glam::DVec3::new(
                (l.start.x + l.end.x) * 0.5,
                (l.start.y + l.end.y) * 0.5,
                (l.start.z + l.end.z) * 0.5,
            )
        }
        EntityType::LwPolyline(pl) => {
            if pl.vertices.is_empty() {
                return;
            }
            let mut sx = 0.0;
            let mut sy = 0.0;
            for v in &pl.vertices {
                sx += v.location.x;
                sy += v.location.y;
            }
            let n = pl.vertices.len() as f64;
            glam::DVec3::new(sx / n, sy / n, pl.elevation)
        }
        _ => return,
    };

    let carrier = meta
        .carrier_handle
        .and_then(|ch| scene.document.get_entity(ch))
        .and_then(|e| crate::modules::aec::engine::views::carrier::read_view_carrier(&scene.document, e))
        .or_else(|| crate::modules::aec::engine::views::carrier::find_view_carrier_owning_handle(&scene.document, handle));

    let rot = carrier.as_ref().map(|c| c.rotation_rad).unwrap_or(0.0);
    let cos_r = rot.cos();
    let sin_r = rot.sin();

    let is_vertical_view = carrier
        .as_ref()
        .and_then(|c| crate::modules::aec::engine::views::carrier::read_view_carrier_definitions(&scene.document, c.handle))
        .map(|(vp, _)| matches!(vp.kind, crate::modules::aec::engine::views::viewpoint::ViewKind::Section { .. } | crate::modules::aec::engine::views::viewpoint::ViewKind::Elevation { .. }))
        .unwrap_or(true);

    let axis_dir = if is_horizontal_grip {
        if is_vertical_view {
            let rx = cos_r;
            let ry = sin_r;
            glam::DVec3::new(rx, ry, 0.0)
        } else {
            // In a horizontal view (FloorPlan / RCP), opening jambs move along the 2D host wall axis.
            let world_wall_dir = (|| {
                let elem_ref = meta.element_ref?;
                let uuid = elem_ref.element_id.as_uuid();
                let bytes = uuid.as_bytes();
                let handle_val = u64::from_be_bytes(bytes[8..16].try_into().ok()?);
                let op_handle = Handle::new(handle_val);
                let op_ent = scene.document.get_entity(op_handle)?;
                let opening = crate::modules::aec::engine::opening_xdata::opening_from_entity(op_ent, op_handle)?;
                let wall = crate::modules::aec::engine::wall_package::resolve_wall_package(scene, opening.host_wall);
                let pts = crate::modules::aec::engine::xdata::get_wall_vertices(scene, wall);
                if pts.len() >= 2 {
                    let dx = pts[1].x - pts[0].x;
                    let dy = pts[1].y - pts[0].y;
                    let len = dx.hypot(dy);
                    if len > 1e-6 {
                        return Some((dx / len, dy / len));
                    }
                }
                None
            })();

            if let Some((dir_x, dir_y)) = world_wall_dir {
                let rx = cos_r * dir_x - sin_r * dir_y;
                let ry = sin_r * dir_x + cos_r * dir_y;
                let v = glam::DVec3::new(rx, ry, 0.0);
                if v.length_squared() > 1e-6 {
                    v.normalize()
                } else {
                    glam::DVec3::new(1.0, 0.0, 0.0)
                }
            } else if let EntityType::Line(l) = entity {
                // Fallback: the jamb line segment in CAD space is perpendicular to the wall axis.
                let jx = l.end.x - l.start.x;
                let jy = l.end.y - l.start.y;
                let j_len = jx.hypot(jy);
                if j_len > 1e-6 {
                    glam::DVec3::new(-jy / j_len, jx / j_len, 0.0)
                } else {
                    glam::DVec3::new(cos_r, sin_r, 0.0)
                }
            } else {
                glam::DVec3::new(cos_r, sin_r, 0.0)
            }
        }
    } else {
        let rx = -sin_r;
        let ry = cos_r;
        glam::DVec3::new(rx, ry, 0.0)
    };

    entity_grips.clear();
    let mut grip = crate::entities::common::square_grip(DERIVED_VIEW_GRIP_BASE, pos);
    grip.axis = Some(axis_dir);
    entity_grips.push(grip);

    match feat {
        crate::modules::aec::engine::views::provenance::FeatureKey::OpeningLeftJamb
        | crate::modules::aec::engine::views::provenance::FeatureKey::OpeningRightJamb
        | crate::modules::aec::engine::views::provenance::FeatureKey::SlabBottom => {
            let dropdown = crate::entities::common::dropdown_grip(DERIVED_VIEW_DROPDOWN_GRIP_BASE, pos);
            entity_grips.push(dropdown);
        }
        _ => {}
    }
}

pub const DERIVED_VIEW_DROPDOWN_GRIP_BASE: usize = 50_100;
pub const DERIVED_VIEW_RESIZE_GRIP_BASE: usize = 50_200;
pub const OPENING_DROPDOWN_GRIP_ID: usize = 50_300;
pub const WALL_BASE_DROPDOWN_GRIP_ID: usize = 50_400;
pub const SLAB_BOTTOM_DROPDOWN_GRIP_ID: usize = 50_500;

pub fn opening_handle_from_derived_primitive(doc: &acadrust::CadDocument, derived_handle: Handle) -> Option<Handle> {
    let ent = doc.get_entity(derived_handle)?;
    let meta = crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(ent)?;
    let elem_ref = meta.element_ref?;
    let uuid = elem_ref.element_id.as_uuid();
    let bytes = uuid.as_bytes();
    let handle_val = u64::from_be_bytes(bytes[8..16].try_into().ok()?);
    Some(Handle::new(handle_val))
}

pub fn carrier_handle_from_derived_primitive(doc: &acadrust::CadDocument, derived_handle: Handle) -> Option<Handle> {
    let ent = doc.get_entity(derived_handle)?;
    let meta = crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(ent)?;
    meta.carrier_handle.or_else(|| crate::modules::aec::engine::views::carrier::find_view_carrier_owning_handle(doc, derived_handle).map(|c| c.handle))
}

pub const CARRIER_GRIP_INSERTION: usize = 60_000;
pub const CARRIER_GRIP_CUT_HEIGHT: usize = 60_001;

/// Appends interactive grips for a `DerivedViewCarrier` entity (insertion relocation and cut height).
pub fn append_view_carrier_grips(
    scene: &crate::scene::Scene,
    handle: Handle,
    entity_grips: &mut Vec<crate::scene::model::object::GripDef>,
) {
    let handle = crate::modules::aec::engine::views::carrier::resolve_carrier_handle(&scene.document, handle);
    let Some(entity) = scene.document.get_entity(handle) else {
        return;
    };
    let Some(carrier) = crate::modules::aec::engine::views::carrier::read_view_carrier(&scene.document, entity) else {
        return;
    };

    // Grip 0: Insertion point (to move the drawing)
    let ins = glam::DVec3::new(carrier.insertion_point.x_m, carrier.insertion_point.y_m, 0.0);
    entity_grips.push(crate::entities::common::square_grip(CARRIER_GRIP_INSERTION, ins));

    // Grip 1: Cut height adjustment for FloorPlan / RCP
    let defs = crate::modules::aec::engine::views::carrier::read_view_carrier_definitions(&scene.document, carrier.handle);
    if let Some((vp, _)) = defs {
        match &vp.kind {
            crate::modules::aec::engine::views::viewpoint::ViewKind::FloorPlan { range } => {
                let cut_h = (vp.z_range.z_min.get() + range.below_cut.get())
                    .clamp(vp.z_range.z_min.get(), vp.z_range.z_max.get());
                let cut_pos = glam::DVec3::new(carrier.insertion_point.x_m, carrier.insertion_point.y_m + cut_h, 0.0);
                let mut grip = crate::entities::common::round_grip(CARRIER_GRIP_CUT_HEIGHT, cut_pos);
                grip.axis = Some(glam::DVec3::new(0.0, 1.0, 0.0));
                entity_grips.push(grip);
            }
            crate::modules::aec::engine::views::viewpoint::ViewKind::ReflectedCeilingPlan { range } => {
                let cut_h = (vp.z_range.z_max.get() - range.above_cut.get())
                    .clamp(vp.z_range.z_min.get(), vp.z_range.z_max.get());
                let cut_pos = glam::DVec3::new(carrier.insertion_point.x_m, carrier.insertion_point.y_m + cut_h, 0.0);
                let mut grip = crate::entities::common::round_grip(CARRIER_GRIP_CUT_HEIGHT, cut_pos);
                grip.axis = Some(glam::DVec3::new(0.0, 1.0, 0.0));
                entity_grips.push(grip);
            }
            _ => {}
        }
    }
}

/// Evaluates a numeric property expression using the drawing's named parameters and standard math functions.
pub fn eval_aec_property_expression(
    scene: &crate::scene::Scene,
    raw_input: &str,
) -> Result<f64, String> {
    let trimmed = raw_input.trim();
    if trimmed.is_empty() {
        return Err("Empty expression".to_string());
    }
    if let Some(val) = crate::entities::common::parse_f64(trimmed) {
        return Ok(val);
    }
    scene
        .named_parameters()
        .evaluate_expression(trimmed)
        .map_err(|e| e.to_string())
}

pub(crate) fn formula_edit_prop(
    label: &str,
    field: &'static str,
    value: f64,
    formulas: &crate::modules::aec::engine::formulas::AecFormulas,
) -> crate::scene::model::object::Property {
    formula_edit_prop_with_aliases(label, field, &[field], value, formulas)
}

pub(crate) fn formula_edit_prop_with_aliases(
    label: &str,
    field: &'static str,
    aliases: &[&str],
    value: f64,
    formulas: &crate::modules::aec::engine::formulas::AecFormulas,
) -> crate::scene::model::object::Property {
    let (full_label, display_val) = if let Some((_, formula)) = formulas.get_any(aliases) {
        (
            format!("{label} (fx = {})", crate::entities::common::format_length(value)),
            formula.to_string(),
        )
    } else {
        (label.to_string(), crate::entities::common::format_length(value))
    };
    crate::scene::model::object::Property {
        label: full_label,
        field,
        value: crate::scene::model::object::PropValue::EditText(display_val),
    }
}

impl crate::app::OpenCADStudio {
    /// Apply an AEC properties field. Returns `true` when `field` is AEC-owned
    /// so Core must skip the generic geom path.
    pub(crate) fn aec_apply_property_field(
        &mut self,
        tab: usize,
        handle: acadrust::Handle,
        field: &str,
        val: &str,
    ) -> bool {
        let is_aec = field.starts_with("opening_")
            || field.starts_with("slab_")
            || field.starts_with("slabopening_")
            || field.starts_with("room_")
            || field.starts_with("marker_")
            || field.starts_with("carrier_")
            || field == "view_cut_height"
            || matches!(
                field,
                "wall_justification"
                    | "wall_base_plane"
                    | "wall_top_plane"
                    | "wall_phase"
                    | "control_plane_name"
                    | "wall_height"
                    | "wall_thickness"
                    | "wall_material"
                    | "wall_base_offset"
                    | "wall_top_offset"
                    | "wall_base_z"
                    | "wall_hatch_angle"
            );
        if !is_aec {
            return false;
        }
        if self.tabs[tab].scene.is_layer_locked(handle) {
            return true;
        }
        if field.starts_with("marker_") {
            let marker_owner = resolve_section_marker_package(&self.tabs[tab].scene, handle).unwrap_or(handle);
            if self.aec.aec_last_applied_property
                == Some((marker_owner, field.to_string(), val.to_string()))
            {
                return true;
            }
            self.aec.aec_last_applied_property =
                Some((marker_owner, field.to_string(), val.to_string()));

            let display_config_library = crate::modules::aec::engine::project::resolve_display_config_library(
                self.aec.aec_project_explorer_file.as_ref(),
            );
            let style_library = crate::modules::aec::engine::project::resolve_style_library(
                self.aec.aec_project_explorer_file.as_ref(),
            );
            let (active_name, _) = self.aec_tab_display_state(tab);
            let ok = apply_marker_property_with_libraries(
                &mut self.tabs[tab].scene.document,
                marker_owner,
                field,
                val,
                &display_config_library,
                &style_library,
                active_name.as_deref(),
            );
            if ok {
                self.tabs[tab].dirty = true;
                self.tabs[tab].scene.bump_geometry();
            }
            return true;
        }
        if field.starts_with("carrier_") || field == "view_cut_height" {
            let carrier_handle = crate::modules::aec::engine::views::carrier::resolve_carrier_handle(
                &self.tabs[tab].scene.document,
                handle,
            );
            // `carrier_action` is a one-shot command; skip dedup so "Update now"
            // can be triggered repeatedly on the same carrier.
            let skip_dedup = field == "carrier_action";
            if !skip_dedup
                && self.aec.aec_last_applied_property
                    == Some((carrier_handle, field.to_string(), val.to_string()))
            {
                return true;
            }
            if skip_dedup {
                self.aec.aec_last_applied_property = None;
            } else {
                self.aec.aec_last_applied_property =
                    Some((carrier_handle, field.to_string(), val.to_string()));
            }

            let display_config_library = crate::modules::aec::engine::project::resolve_display_config_library(
                self.aec.aec_project_explorer_file.as_ref(),
            );
            let style_library = crate::modules::aec::engine::project::resolve_style_library(
                self.aec.aec_project_explorer_file.as_ref(),
            );
            let (active_name, _) = self.aec_tab_display_state(tab);
            let ok = apply_carrier_property_with_libraries(
                &mut self.tabs[tab].scene.document,
                carrier_handle,
                field,
                val,
                &display_config_library,
                &style_library,
                active_name.as_deref(),
            );
            if ok {
                self.tabs[tab].dirty = true;
                self.tabs[tab].scene.bump_geometry();
            }
            return true;
        }
        if field.starts_with("room_") {
            let room_owner = self.tabs[tab]
                .scene
                .document
                .get_entity(handle)
                .and_then(room_package::resolve_room_package)
                .unwrap_or(handle);
            if self.aec.aec_last_applied_property
                == Some((room_owner, field.to_string(), val.to_string()))
            {
                return true;
            }
            self.aec.aec_last_applied_property =
                Some((room_owner, field.to_string(), val.to_string()));

            let Some(entity) = self.tabs[tab].scene.document.get_entity(room_owner) else {
                return true;
            };
            let Some(mut room) = room_xdata::room_from_entity(entity) else {
                return true;
            };

            match field {
                "room_name" => room.name = val.to_string(),
                "room_number" => room.number = val.to_string(),
                "room_function" => {
                    room.function = RoomFunction::from_str(val);
                    room.factor = room.function.default_factor();
                }
                "room_factor" => {
                    match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                        Ok(v) => {
                            room.factor = v;
                            if crate::modules::aec::engine::formulas::is_formula_expression(val) {
                                room.formulas.set("room_factor", val.to_string());
                            } else {
                                room.formulas.remove("room_factor");
                            }
                        }
                        Err(e) => {
                            self.command_line.push_error(&format!("AEC: {e}"));
                            return true;
                        }
                    }
                }
                "room_clear_height" => {
                    match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                        Ok(v) => {
                            room.clear_height = v;
                            if crate::modules::aec::engine::formulas::is_formula_expression(val) {
                                room.formulas.set("room_clear_height", val.to_string());
                            } else {
                                room.formulas.remove("room_clear_height");
                            }
                        }
                        Err(e) => {
                            self.command_line.push_error(&format!("AEC: {e}"));
                            return true;
                        }
                    }
                }
                "room_floor_finish_style" => {
                    let trimmed = val.trim();
                    if trimmed.is_empty() || trimmed == "-" || trimmed == "- (Standard)" {
                        room.finish_style_id = None;
                        room.floor_finish = None;
                    } else {
                        let style_library = crate::modules::aec::engine::project::resolve_style_library(
                            self.aec.aec_project_explorer_file.as_ref(),
                        );
                        let fallback_lib = library::seed_floor_finish_styles();
                        let found = style_library
                            .find_floor_finish_style_by_name_or_id(trimmed)
                            .cloned()
                            .or_else(|| {
                                fallback_lib
                                    .into_iter()
                                    .find(|s| s.style.name == trimmed || s.style.id == trimmed)
                            });
                        if let Some(style) = found {
                            room.finish_style_id = Some(style.style.id.clone());
                            room.floor_finish = Some(style.to_room_finishes());
                        } else {
                            room.finish_style_id = Some(trimmed.to_string());
                        }
                    }
                }
                "room_floor_finish" => {
                    let trimmed = val.trim();
                    if trimmed.is_empty() || trimmed == "-" {
                        room.floor_finish = None;
                    } else {
                        let lower = trimmed.to_lowercase();
                        let hatch = if lower.contains("fliese") || lower.contains("tile") {
                            Some("SQUARE".to_string())
                        } else if lower.contains("parkett") || lower.contains("wood") {
                            Some("ANSI31".to_string())
                        } else {
                            None
                        };
                        let mut finish = RoomFinish::new(trimmed, 0.05);
                        if let Some(h) = hatch {
                            finish = finish.with_hatch(h);
                        }
                        room.floor_finish = Some(vec![finish]);
                    }
                }
                "room_ceiling_finish" => {
                    let trimmed = val.trim();
                    if trimmed.is_empty() || trimmed == "-" {
                        room.ceiling_finish = None;
                    } else {
                        let lower = trimmed.to_lowercase();
                        let hatch = if lower.contains("raster") || lower.contains("grid") {
                            Some("NET".to_string())
                        } else if lower.contains("fliese") || lower.contains("tile") || lower.contains("kassette") {
                            Some("SQUARE".to_string())
                        } else if lower.contains("gips") || lower.contains("wood") || lower.contains("holz") {
                            Some("ANSI31".to_string())
                        } else {
                            None
                        };
                        let mut finish = RoomFinish::new(trimmed, 0.02);
                        if let Some(h) = hatch {
                            finish = finish.with_hatch(h);
                        }
                        room.ceiling_finish = Some(vec![finish]);
                    }
                }
                "room_stamp_x" => {
                    match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                        Ok(v) => {
                            let (_, sy) = room.stamp_pos.unwrap_or_else(|| {
                                let pts = room_boundary_points(entity);
                                if pts.len() >= 3 {
                                    crate::modules::aec::engine::geometry::centroid(&pts)
                                } else {
                                    (0.0, 0.0)
                                }
                            });
                            room.stamp_pos = Some((v, sy));
                        }
                        Err(e) => {
                            self.command_line.push_error(&format!("AEC: {e}"));
                            return true;
                        }
                    }
                }
                "room_stamp_y" => {
                    match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                        Ok(v) => {
                            let (sx, _) = room.stamp_pos.unwrap_or_else(|| {
                                let pts = room_boundary_points(entity);
                                if pts.len() >= 3 {
                                    crate::modules::aec::engine::geometry::centroid(&pts)
                                } else {
                                    (0.0, 0.0)
                                }
                            });
                            room.stamp_pos = Some((sx, v));
                        }
                        Err(e) => {
                            self.command_line.push_error(&format!("AEC: {e}"));
                            return true;
                        }
                    }
                }
                _ => return true,
            }

            room_xdata::write_room_record(&mut self.tabs[tab].scene.document, room_owner, &room);
            let _ = self.regenerate_room_respecting_active_display_config(tab, room_owner);
            let _ = crate::modules::aec::rooms::schedule::update_all_room_schedules(
                &mut self.tabs[tab].scene,
            );
            self.tabs[tab].dirty = true;
            self.tabs[tab].scene.bump_geometry();
            return true;
        }
        if field.starts_with("slabopening_") {
            let owner = slab_package::slab_opening_owner_if_any(&self.tabs[tab].scene, handle)
                .unwrap_or(handle);
            if self.aec.aec_last_applied_property
                == Some((owner, field.to_string(), val.to_string()))
            {
                return true;
            }
            self.aec.aec_last_applied_property =
                Some((owner, field.to_string(), val.to_string()));

            let Some(entity) = self.tabs[tab].scene.document.get_entity(owner) else {
                return true;
            };
            let Some(mut opening) = slab_xdata::slab_opening_from_entity(entity) else {
                return true;
            };
            let host = opening.host_slab;
            match field {
                "slabopening_kind" => {
                    opening.kind = SlabOpeningKind::from_str(val);
                    let _ = slab_xdata::write_slab_opening_model(
                        &mut self.tabs[tab].scene,
                        owner,
                        &opening,
                    );
                }
                "slabopening_depth_mode" => {
                    let depth_val = opening.depth.depth_value().unwrap_or(0.10);
                    opening.depth = SlabOpeningDepth::from_mode_and_depth(val, depth_val);
                    let _ = slab_xdata::write_slab_opening_model(
                        &mut self.tabs[tab].scene,
                        owner,
                        &opening,
                    );
                }
                "slabopening_recess_depth" => {
                    match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                        Ok(v) => {
                            if v >= 0.0 {
                                opening.depth = SlabOpeningDepth::Recess(v);
                                let _ = slab_xdata::write_slab_opening_model(
                                    &mut self.tabs[tab].scene,
                                    owner,
                                    &opening,
                                );
                            }
                        }
                        Err(e) => {
                            self.command_line.push_error(&format!("AEC: {e}"));
                            return true;
                        }
                    }
                }
                _ => return true,
            }
            let _ = self.regenerate_slab_respecting_active_display_config(tab, host);
            let style_library = crate::modules::aec::engine::project::resolve_style_library(
                self.aec.aec_project_explorer_file.as_ref(),
            );
            let rules = self.resolve_active_display_config_slab_rules(tab, Some(host));
            let _ = crate::modules::aec::engine::slab_regen::regenerate_slab_opening_representation(
                &mut self.tabs[tab].scene,
                owner,
                Some(&style_library),
                rules.as_ref(),
            );
            return true;
        }
        if field.starts_with("slab_") {
            let slab_owner = slab_package::resolve_slab_package(&self.tabs[tab].scene, handle);
            if self.aec.aec_last_applied_property
                == Some((slab_owner, field.to_string(), val.to_string()))
            {
                return true;
            }
            self.aec.aec_last_applied_property =
                Some((slab_owner, field.to_string(), val.to_string()));

            match field {
                "slab_justification" => {
                    let justification = SlabJustification::from_str(val);
                    if slab_xdata::write_slab_justification(
                        &mut self.tabs[tab].scene,
                        slab_owner,
                        justification,
                    ) {
                        let _ = self.regenerate_slab_respecting_active_display_config(tab, slab_owner);
                    }
                }
                "slab_phase" => {
                    let new_phase =
                        crate::modules::aec::engine::plan_view::PlanPhase::from_str(val);
                    if slab_xdata::write_slab_phase(
                        &mut self.tabs[tab].scene,
                        slab_owner,
                        new_phase,
                    ) {
                        let _ = self.regenerate_slab_respecting_active_display_config(tab, slab_owner);
                    }
                }
                "slab_base_offset" | "slab_top_offset" | "slab_offset" | "top_offset" | "base_offset" | "offset" => {
                    match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                        Ok(v) => {
                            let is_base = field == "slab_base_offset" || field == "base_offset";
                            let (base, top) = if is_base {
                                (Some(v), None)
                            } else {
                                (None, Some(v))
                            };
                            let canonical_field = if is_base {
                                "slab_base_offset"
                            } else {
                                "slab_top_offset"
                            };
                            let alias_group = if is_base {
                                SLAB_BASE_OFFSET_KEYS
                            } else {
                                SLAB_TOP_OFFSET_KEYS
                            };
                            if slab_xdata::write_slab_plane_offsets(
                                &mut self.tabs[tab].scene,
                                slab_owner,
                                base,
                                top,
                            ) {
                                if let Some(entity) = self.tabs[tab].scene.document.get_entity(slab_owner) {
                                    if let Some(mut slab) = slab_xdata::slab_from_entity(entity) {
                                        slab.formulas.remove_all(alias_group);
                                        if crate::modules::aec::engine::formulas::is_formula_expression(val) {
                                            slab.formulas.set(canonical_field, val.to_string());
                                        }
                                        let _ = slab_xdata::write_slab_record(&mut self.tabs[tab].scene.document, slab_owner, &slab);
                                    }
                                }
                                let _ = self
                                    .regenerate_slab_respecting_active_display_config(tab, slab_owner);
                            }
                        }
                        Err(e) => {
                            self.command_line.push_error(&format!("AEC: {e}"));
                        }
                    }
                }
                "slab_base_plane" | "slab_top_plane" | "slab_plane" => {
                    let style_library =
                        crate::modules::aec::engine::project::resolve_style_library(
                            self.aec.aec_project_explorer_file.as_ref(),
                        );
                    let rules =
                        self.resolve_active_display_config_slab_rules(tab, Some(slab_owner));
                    crate::modules::aec::project::slab_planes::apply_slab_plane_choice(
                        &mut self.tabs[tab].scene,
                        self.aec.aec_project_explorer_file.as_ref(),
                        slab_owner,
                        field == "slab_base_plane",
                        val,
                        Some(&style_library),
                        rules.as_ref(),
                    );
                }
                "slab_structural_style" => {
                    let trimmed = val.trim();
                    let style_library = crate::modules::aec::engine::project::resolve_style_library(
                        self.aec.aec_project_explorer_file.as_ref(),
                    );
                    if let Some(entity) = self.tabs[tab].scene.document.get_entity(slab_owner) {
                        if let Some(mut slab) = slab_xdata::slab_from_entity(entity) {
                            if trimmed.is_empty() || trimmed == "-" || trimmed == "- (Standard)" {
                                slab.structural_style_id = None;
                            } else {
                                let fallback = library::seed_slab_structural_styles();
                                let found = style_library
                                    .find_slab_structural_style_by_name_or_id(trimmed)
                                    .or_else(|| {
                                        fallback
                                            .iter()
                                            .find(|s| s.style.name == trimmed || s.style.id == trimmed)
                                    });
                                slab.structural_style_id = found
                                    .map(|s| s.style.id.clone())
                                    .or_else(|| Some(trimmed.to_string()));
                            }
                            let _ = slab_xdata::write_slab_model(&mut self.tabs[tab].scene, slab_owner, &slab);
                            let _ = self.regenerate_slab_respecting_active_display_config(tab, slab_owner);
                        }
                    }
                }
                "slab_finish_style" => {
                    let trimmed = val.trim();
                    let style_library = crate::modules::aec::engine::project::resolve_style_library(
                        self.aec.aec_project_explorer_file.as_ref(),
                    );
                    if let Some(entity) = self.tabs[tab].scene.document.get_entity(slab_owner) {
                        if let Some(mut slab) = slab_xdata::slab_from_entity(entity) {
                            if trimmed.is_empty() || trimmed == "-" || trimmed == "- (Standard)" {
                                slab.finish_style_id = None;
                            } else {
                                let fallback = library::seed_floor_finish_styles();
                                let found = style_library
                                    .find_floor_finish_style_by_name_or_id(trimmed)
                                    .or_else(|| {
                                        fallback
                                            .iter()
                                            .find(|s| s.style.name == trimmed || s.style.id == trimmed)
                                    });
                                slab.finish_style_id = found
                                    .map(|s| s.style.id.clone())
                                    .or_else(|| Some(trimmed.to_string()));
                            }
                            let _ = slab_xdata::write_slab_model(&mut self.tabs[tab].scene, slab_owner, &slab);
                            let _ = self.regenerate_slab_respecting_active_display_config(tab, slab_owner);
                        }
                    }
                }
                // Style is applied via the style picker modal.
                "slab_style" => {}
                _ => {}
            }
            return true;
        }
        if field.starts_with("opening_") {
            let owner = opening_display::opening_owner_if_any(&self.tabs[tab].scene, handle)
                .unwrap_or(handle);
            if self.aec.aec_last_applied_property
                == Some((owner, field.to_string(), val.to_string()))
            {
                return true;
            }
            self.aec.aec_last_applied_property =
                Some((owner, field.to_string(), val.to_string()));

            let style_library = crate::modules::aec::engine::project::resolve_style_library(
                self.aec.aec_project_explorer_file.as_ref(),
            );
            let Some(entity) = self.tabs[tab].scene.document.get_entity(owner).cloned() else {
                return true;
            };
            let Some(mut opening) = opening_xdata::opening_from_entity(&entity, owner) else {
                return true;
            };
            let (rules, _) =
                self.resolve_active_display_config_wall_rules(tab, Some(opening.host_wall));
            match field {
                "opening_sill_plane" | "opening_head_plane" => {
                    crate::modules::aec::project::opening_planes::apply_opening_plane_choice(
                        &mut self.tabs[tab].scene,
                        self.aec.aec_project_explorer_file.as_ref(),
                        owner,
                        field == "opening_sill_plane",
                        val,
                        Some(&style_library),
                        rules.as_ref(),
                    );
                    return true;
                }
                "opening_sill_offset" | "opening_head_offset" => {
                    match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                        Ok(v) => {
                            let (sill, head) = if field == "opening_sill_offset" {
                                (Some(v), None)
                            } else {
                                (None, Some(v))
                            };
                            crate::modules::aec::project::opening_planes::apply_opening_plane_offsets(
                                &mut self.tabs[tab].scene,
                                self.aec.aec_project_explorer_file.as_ref(),
                                owner,
                                sill,
                                head,
                                Some(&style_library),
                                rules.as_ref(),
                            );
                            if let Some(entity) = self.tabs[tab].scene.document.get_entity(owner) {
                                if let Some(mut op) = opening_xdata::opening_from_entity(entity, owner) {
                                    if crate::modules::aec::engine::formulas::is_formula_expression(val) {
                                        op.formulas.set(field, val.to_string());
                                    } else {
                                        op.formulas.remove(field);
                                    }
                                    let _ = opening_display::commit_opening_instance(
                                        &mut self.tabs[tab].scene,
                                        &op,
                                        Some(&style_library),
                                        rules.as_ref(),
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            self.command_line.push_error(&format!("AEC: {e}"));
                        }
                    }
                    return true;
                }
                _ => {}
            }
            let evaluated_val = match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                Ok(v) => v.to_string(),
                Err(_) => val.to_string(),
            };
            if apply_opening_property(&mut opening, field, &evaluated_val, Some(&style_library)) {
                if crate::modules::aec::engine::formulas::is_formula_expression(val) {
                    opening.formulas.set(field, val.to_string());
                } else {
                    opening.formulas.remove(field);
                }
                let _ = opening_display::commit_opening_instance(
                    &mut self.tabs[tab].scene,
                    &opening,
                    Some(&style_library),
                    rules.as_ref(),
                );
            }
            return true;
        }
        let wall_owner = crate::modules::aec::engine::wall_package::resolve_wall_package(
            &self.tabs[tab].scene,
            handle,
        );
        if self.aec.aec_last_applied_property
            == Some((wall_owner, field.to_string(), val.to_string()))
        {
            return true;
        }
        self.aec.aec_last_applied_property =
            Some((wall_owner, field.to_string(), val.to_string()));

        match field {
            "wall_justification" => {
                let new_justification =
                    crate::modules::aec::engine::WallJustification::from_str(val);
                let style_library = crate::modules::aec::engine::project::resolve_style_library(
                    self.aec.aec_project_explorer_file.as_ref(),
                );
                crate::modules::aec::engine::wall_regen::change_wall_justification(
                    &mut self.tabs[tab].scene,
                    wall_owner,
                    new_justification,
                    Some(&style_library),
                );
            }
            "wall_base_plane" | "wall_top_plane" => {
                let style_library = crate::modules::aec::engine::project::resolve_style_library(
                    self.aec.aec_project_explorer_file.as_ref(),
                );
                crate::modules::aec::project::wall_planes::apply_wall_plane_choice(
                    &mut self.tabs[tab].scene,
                    self.aec.aec_project_explorer_file.as_ref(),
                    wall_owner,
                    field == "wall_base_plane",
                    val,
                    Some(&style_library),
                );
            }
            "wall_phase" => {
                let new_phase = crate::modules::aec::engine::plan_view::PlanPhase::from_str(val);
                crate::modules::aec::engine::xdata::write_wall_phase(
                    &mut self.tabs[tab].scene,
                    wall_owner,
                    new_phase,
                );
                let _ = self.regenerate_wall_respecting_active_display_config(tab, wall_owner);
            }
            "control_plane_name" => {
                apply_control_plane_name(
                    &mut self.tabs[tab].scene,
                    self.aec.aec_project_explorer_file.as_mut(),
                    wall_owner,
                    val,
                );
                self.aec_project_explorer_persist_if_pathed();
            }
            "wall_height" => {
                match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                    Ok(v) => {
                        if v > 0.0 {
                            if let Some(entity) = self.tabs[tab].scene.document.get_entity(wall_owner) {
                                if let Some(mut wall) = crate::modules::aec::engine::xdata::wall_from_entity(entity) {
                                    if crate::modules::aec::engine::formulas::is_formula_expression(val) {
                                        wall.formulas.set("wall_height", val.to_string());
                                    } else {
                                        wall.formulas.remove("wall_height");
                                    }
                                    wall.height = v;
                                    let mut record = acadrust::xdata::ExtendedDataRecord::new(crate::modules::aec::engine::xdata::AEC_APPID);
                                    record.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
                                    crate::modules::aec::engine::xdata::write_aec_record(&mut self.tabs[tab].scene.document, wall_owner, record);
                                } else {
                                    crate::modules::aec::engine::xdata::write_wall_height(
                                        &mut self.tabs[tab].scene,
                                        wall_owner,
                                        v,
                                    );
                                }
                            }
                            let _ =
                                self.regenerate_wall_respecting_active_display_config(tab, wall_owner);
                        }
                    }
                    Err(e) => {
                        self.command_line.push_error(&format!("AEC: {e}"));
                    }
                }
            }
            "wall_base_z" => {
                match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                    Ok(v) => {
                        crate::modules::aec::project::wall_planes::apply_wall_base_z(
                            &mut self.tabs[tab].scene,
                            wall_owner,
                            v,
                        );
                        if let Some(entity) = self.tabs[tab].scene.document.get_entity(wall_owner) {
                            if let Some(mut wall) = crate::modules::aec::engine::xdata::wall_from_entity(entity) {
                                if crate::modules::aec::engine::formulas::is_formula_expression(val) {
                                    wall.formulas.set("wall_base_z", val.to_string());
                                } else {
                                    wall.formulas.remove("wall_base_z");
                                }
                                let mut record = acadrust::xdata::ExtendedDataRecord::new(crate::modules::aec::engine::xdata::AEC_APPID);
                                record.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
                                crate::modules::aec::engine::xdata::write_aec_record(&mut self.tabs[tab].scene.document, wall_owner, record);
                            }
                        }
                    }
                    Err(e) => {
                        self.command_line.push_error(&format!("AEC: {e}"));
                    }
                }
            }
            "wall_base_offset" | "wall_top_offset" => {
                match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                    Ok(v) => {
                        let (base, top) = if field == "wall_base_offset" {
                            (Some(v), None)
                        } else {
                            (None, Some(v))
                        };
                        crate::modules::aec::engine::xdata::write_wall_plane_offsets(
                            &mut self.tabs[tab].scene,
                            wall_owner,
                            base,
                            top,
                        );
                        if let Some(entity) = self.tabs[tab].scene.document.get_entity(wall_owner) {
                            if let Some(mut wall) = crate::modules::aec::engine::xdata::wall_from_entity(entity) {
                                if crate::modules::aec::engine::formulas::is_formula_expression(val) {
                                    wall.formulas.set(field, val.to_string());
                                } else {
                                    wall.formulas.remove(field);
                                }
                                let mut record = acadrust::xdata::ExtendedDataRecord::new(crate::modules::aec::engine::xdata::AEC_APPID);
                                record.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
                                crate::modules::aec::engine::xdata::write_aec_record(&mut self.tabs[tab].scene.document, wall_owner, record);
                            }
                        }
                        let _ =
                            self.regenerate_wall_respecting_active_display_config(tab, wall_owner);
                    }
                    Err(e) => {
                        self.command_line.push_error(&format!("AEC: {e}"));
                    }
                }
            }
            "wall_hatch_angle" => {
                match eval_aec_property_expression(&self.tabs[tab].scene, val) {
                    Ok(v) => {
                        let existing_override = self.tabs[tab]
                            .scene
                            .document
                            .get_entity(wall_owner)
                            .and_then(crate::modules::aec::engine::xdata::wall_from_entity)
                            .and_then(|wall| wall.hatch_override);
                        if let Some(mut ov) = existing_override {
                            ov.hatch_angle = Some(v);
                            crate::modules::aec::engine::xdata::write_wall_hatch_override(
                                &mut self.tabs[tab].scene,
                                wall_owner,
                                Some(ov),
                            );
                        }
                    }
                    Err(e) => {
                        self.command_line.push_error(&format!("AEC: {e}"));
                    }
                }
            }
            "wall_thickness" | "wall_material" => {}
            _ => {}
        }
        true
    }

    /// Re-evaluates all stored formulas across all AEC entities (Walls, Slabs, Openings, Rooms)
    /// in the specified tab after named drawing parameters have changed.
    pub(crate) fn aec_recompute_all_formulas(&mut self, tab: usize) {
        if tab >= self.tabs.len() {
            return;
        }
        let style_library = crate::modules::aec::engine::project::resolve_style_library(
            self.aec.aec_project_explorer_file.as_ref(),
        );

        let entities: Vec<Handle> = self.tabs[tab]
            .scene
            .document
            .entities()
            .map(|e| e.common().handle)
            .collect();
        let mut modified_walls = Vec::new();
        let mut modified_slabs = Vec::new();
        let mut modified_rooms = Vec::new();

        for handle in entities {
            let Some(entity) = self.tabs[tab].scene.document.get_entity(handle).cloned() else {
                continue;
            };

            // 1. Walls
            if let Some(mut wall) = xdata::wall_from_entity(&entity) {
                if !wall.formulas.is_empty() {
                    let mut changed = false;
                    for (field, expr) in &wall.formulas.fields {
                        if let Ok(val) = eval_aec_property_expression(&self.tabs[tab].scene, expr) {
                            match field.as_str() {
                                "wall_height" | "height" => {
                                    if (wall.height - val).abs() > 1e-9 && val > 0.0 {
                                        wall.height = val;
                                        changed = true;
                                    }
                                }
                                "wall_base_offset" | "base_offset" => {
                                    if (wall.base_offset - val).abs() > 1e-9 {
                                        wall.base_offset = val;
                                        changed = true;
                                    }
                                }
                                "wall_top_offset" | "top_offset" => {
                                    if (wall.top_offset - val).abs() > 1e-9 {
                                        wall.top_offset = val;
                                        changed = true;
                                    }
                                }
                                "wall_base_z" => {
                                    if (wall.base_origin[2] - val).abs() > 1e-9 {
                                        wall.base_origin[2] = val;
                                        changed = true;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    if changed {
                        let mut record = acadrust::xdata::ExtendedDataRecord::new(xdata::AEC_APPID);
                        record.values = xdata::wall_record_for_wall(&wall);
                        xdata::write_aec_record(&mut self.tabs[tab].scene.document, handle, record);
                        modified_walls.push(handle);
                    }
                }
            }

            // 2. Slabs
            if let Some(mut slab) = slab_xdata::slab_from_entity(&entity) {
                if !slab.formulas.is_empty() {
                    let mut changed = false;
                    if let Some((_, expr)) = slab.formulas.get_any(SLAB_BASE_OFFSET_KEYS) {
                        if let Ok(val) = eval_aec_property_expression(&self.tabs[tab].scene, expr) {
                            if (slab.base_offset - val).abs() > 1e-9 {
                                slab.base_offset = val;
                                changed = true;
                            }
                        }
                    }
                    if let Some((_, expr)) = slab.formulas.get_any(SLAB_TOP_OFFSET_KEYS) {
                        if let Ok(val) = eval_aec_property_expression(&self.tabs[tab].scene, expr) {
                            if (slab.top_offset - val).abs() > 1e-9 {
                                slab.top_offset = val;
                                changed = true;
                            }
                        }
                    }
                    if changed {
                        slab_xdata::write_slab_record(&mut self.tabs[tab].scene.document, handle, &slab);
                        modified_slabs.push(handle);
                    }
                }
            }

            // 3. Openings
            if let Some(mut opening) = opening_xdata::opening_from_entity(&entity, handle) {
                if !opening.formulas.is_empty() {
                    let mut changed = false;
                    for (field, expr) in &opening.formulas.fields {
                        if let Ok(val) = eval_aec_property_expression(&self.tabs[tab].scene, expr) {
                            match field.as_str() {
                                "opening_width" | "width" => {
                                    if (opening.width - val).abs() > 1e-9 && val > 0.0 {
                                        opening.width = val;
                                        changed = true;
                                    }
                                }
                                "opening_height" | "height" => {
                                    if (opening.height - val).abs() > 1e-9 && val > 0.0 {
                                        opening.height = val;
                                        changed = true;
                                    }
                                }
                                "opening_sill" | "opening_sill_height" | "sill_height" => {
                                    if (opening.sill_height - val).abs() > 1e-9 && val >= 0.0 {
                                        opening.sill_height = val;
                                        changed = true;
                                    }
                                }
                                "opening_sill_offset" | "sill_offset" => {
                                    if (opening.sill_offset - val).abs() > 1e-9 {
                                        opening.sill_offset = val;
                                        changed = true;
                                    }
                                }
                                "opening_head_offset" | "head_offset" => {
                                    if (opening.head_offset - val).abs() > 1e-9 {
                                        opening.head_offset = val;
                                        changed = true;
                                    }
                                }
                                "opening_spring" | "spring_height" => {
                                    if (opening.spring_height - val).abs() > 1e-9 {
                                        opening.spring_height = val;
                                        changed = true;
                                    }
                                }
                                "opening_cross_offset" | "cross_axis_offset" => {
                                    if (opening.cross_axis_offset - val).abs() > 1e-9 {
                                        opening.cross_axis_offset = val;
                                        changed = true;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    if changed {
                        let (rules, _) = self.resolve_active_display_config_wall_rules(tab, Some(opening.host_wall));
                        let _ = opening_display::commit_opening_instance(
                            &mut self.tabs[tab].scene,
                            &opening,
                            Some(&style_library),
                            rules.as_ref(),
                        );
                    }
                }
            }

            // 4. Rooms
            if let Some(mut room) = room_xdata::room_from_entity(&entity) {
                if !room.formulas.is_empty() {
                    let mut changed = false;
                    for (field, expr) in &room.formulas.fields {
                        if let Ok(val) = eval_aec_property_expression(&self.tabs[tab].scene, expr) {
                            match field.as_str() {
                                "room_factor" | "factor" => {
                                    if (room.factor - val).abs() > 1e-9 {
                                        room.factor = val;
                                        changed = true;
                                    }
                                }
                                "room_clear_height" | "clear_height" => {
                                    if (room.clear_height - val).abs() > 1e-9 {
                                        room.clear_height = val;
                                        changed = true;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    if changed {
                        room_xdata::write_room_record(&mut self.tabs[tab].scene.document, handle, &room);
                        modified_rooms.push(handle);
                    }
                }
            }
        }

        for wall_handle in modified_walls {
            let _ = self.regenerate_wall_respecting_active_display_config(tab, wall_handle);
        }
        for slab_handle in modified_slabs {
            let _ = self.regenerate_slab_respecting_active_display_config(tab, slab_handle);
        }
        for room_handle in modified_rooms {
            let _ = self.regenerate_room_respecting_active_display_config(tab, room_handle);
        }
    }
}

pub fn append_opening_axis_grips(
    scene: &crate::scene::Scene,
    handle: Handle,
    entity_grips: &mut Vec<crate::scene::model::object::GripDef>,
) {
    let Some(owner) = opening_display::opening_owner_if_any(scene, handle) else {
        return;
    };
    let Some(entity) = scene.document.get_entity(owner) else {
        return;
    };
    let Some(opening) = opening_xdata::opening_from_entity(entity, owner) else {
        return;
    };
    let wall = wall_package::resolve_wall_package(scene, opening.host_wall);
    let base_z = opening_display::host_base_z(scene, wall);
    let axis: Vec<(f64, f64)> = xdata::get_wall_vertices(scene, wall)
        .iter()
        .map(|v| (v.x, v.y))
        .collect();
    let grips = opening_display::opening_axis_grips(&axis, &opening, base_z);
    if grips.is_empty() {
        return;
    }
    entity_grips.clear();
    entity_grips.extend(grips);
    let center = if let Some(((cx, cy), _)) = crate::modules::aec::engine::openings::point_and_tangent_at_distance(&axis, opening.center_along_axis()) {
        glam::DVec3::new(cx, cy, base_z)
    } else {
        glam::DVec3::ZERO
    };
    entity_grips.push(crate::entities::common::dropdown_grip(OPENING_DROPDOWN_GRIP_ID, center));
}

pub const ROOM_STAMP_GRIP_ID: usize = usize::MAX - 10;

pub fn append_room_grips(
    scene: &crate::scene::Scene,
    handle: Handle,
    _entity: &EntityType,
    entity_grips: &mut Vec<crate::scene::model::object::GripDef>,
) {
    let owner = room_package::resolve_room_package_handle(scene, handle);
    let Some(carrier) = scene.document.get_entity(owner) else {
        return;
    };
    let Some(room) = room_xdata::room_from_entity(carrier) else {
        return;
    };

    let (sx, sy) = room.stamp_pos.unwrap_or_else(|| {
        let pts = room_regen::room_boundary_points(carrier);
        if pts.len() >= 3 {
            crate::modules::aec::engine::geometry::centroid(&pts)
        } else {
            (0.0, 0.0)
        }
    });

    let z = match carrier {
        EntityType::LwPolyline(pl) => pl.elevation,
        _ => 0.0,
    };

    entity_grips.push(crate::entities::common::round_grip(
        ROOM_STAMP_GRIP_ID,
        glam::DVec3::new(sx, sy, z),
    ));
}

pub fn append_wall_junction_grips(
    scene: &crate::scene::Scene,
    handle: Handle,
    entity: &EntityType,
    entity_grips: &mut Vec<crate::scene::model::object::GripDef>,
) {
    if xdata::wall_from_entity(entity).is_none() {
        return;
    }
    let verts = xdata::get_wall_vertices(scene, handle);
    if verts.len() < 2 {
        return;
    }
    for (end_index, world) in [(0usize, verts[0]), (1usize, verts[verts.len() - 1])] {
        let participants = join_ops::walls_at_junction(scene, handle, end_index);
        if participants.len() > 1 {
            entity_grips.push(crate::entities::common::dropdown_grip(
                junction_pick::wall_junction_dropdown_grip_id(end_index),
                world,
            ));
        }
    }
}

pub fn append_slab_base_grips(
    _scene: &crate::scene::Scene,
    _handle: Handle,
    entity: &EntityType,
    entity_grips: &mut Vec<crate::scene::model::object::GripDef>,
) {
    if slab_xdata::slab_from_entity(entity).is_none() {
        return;
    }
    let pos = match entity {
        EntityType::LwPolyline(pl) if !pl.vertices.is_empty() => {
            glam::DVec3::new(pl.vertices[0].location.x, pl.vertices[0].location.y, pl.elevation)
        }
        _ => return,
    };
    entity_grips.push(crate::entities::common::dropdown_grip(
        SLAB_BOTTOM_DROPDOWN_GRIP_ID,
        pos,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::LwPolyline;
    use acadrust::xdata::ExtendedDataRecord;
    use crate::modules::aec::engine::xdata::{wall_record_for_wall, AEC_APPID};
    use crate::modules::aec::engine::project::{Building, ProjectFile, StoreyRef};
    use crate::modules::aec::engine::wall::Wall;

    #[test]
    fn wall_plane_fields_are_base_then_offset_then_top() {
        let storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        let mut wall = Wall::new("s", 2.5, 0);
        wall.base_plane_id = Some(storey.floor_plane_id);
        wall.top_plane_id = Some(storey.ceiling_plane_id);
        wall.base_offset = 0.1;
        wall.top_offset = -0.05;
        let mut entity = EntityType::LwPolyline(LwPolyline::new());
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record_for_wall(&wall);
        entity.common_mut().extended_data.add_record(record);

        let mut project = ProjectFile::default();
        let mut building = Building::new("B");
        building.storeys.push(storey);
        project.buildings.push(building);

        let section = wall_prop_section(&entity, None, Some(&project)).expect("section");
        let fields: Vec<&str> = section.props.iter().map(|p| p.field).collect();
        let base = fields.iter().position(|f| *f == "wall_base_plane").unwrap();
        let base_off = fields.iter().position(|f| *f == "wall_base_offset").unwrap();
        let top = fields.iter().position(|f| *f == "wall_top_plane").unwrap();
        let top_off = fields.iter().position(|f| *f == "wall_top_offset").unwrap();
        assert!(base < base_off && base_off < top && top < top_off);

        let base_sel = match &section.props[base].value {
            crate::scene::model::object::PropValue::Choice { selected, .. } => selected,
            _ => panic!("choice"),
        };
        assert!(base_sel.contains("ELEVATION") || !base_sel.is_empty());
        assert_ne!(base_sel, "(none)");
    }

    #[test]
    fn circle_width_edit_locks_height() {
        let mut o = Opening::window(Handle::new(1), Handle::new(2), 1.0);
        o.shape = OpeningShape::Circle;
        o.width = 1.2;
        o.height = 1.2;
        assert!(apply_opening_property(&mut o, "opening_width", "1.8", None));
        assert!((o.width - 1.8).abs() < 1e-12);
        assert!((o.height - 1.8).abs() < 1e-12);
        assert!(apply_opening_property(&mut o, "opening_height", "1.0", None));
        assert!((o.width - 1.0).abs() < 1e-12);
        assert!((o.height - 1.0).abs() < 1e-12);
    }

    #[test]
    fn equilateral_width_sets_height() {
        let mut o = Opening::window(Handle::new(1), Handle::new(2), 1.0);
        o.shape = OpeningShape::Triangle(
            crate::modules::aec::engine::opening_shape::TriangleVariant::Equilateral,
        );
        assert!(apply_opening_property(&mut o, "opening_width", "2.0", None));
        assert!((o.height - OpeningShape::equilateral_height(2.0)).abs() < 1e-12);
    }

    #[test]
    fn hinge_and_arch_spring_apply() {
        let mut o = Opening::door(Handle::new(1), Handle::new(2), 1.0);
        assert!(apply_opening_property(&mut o, "opening_hinge", "Right", None));
        assert_eq!(o.hinge, LeafOperation::right());
        assert!(apply_opening_property(&mut o, "opening_shape", "Arch", None));
        assert_eq!(o.shape, OpeningShape::Arch);
        assert!(o.spring_height > 0.0);
        assert!(apply_opening_property(&mut o, "opening_spring", "0.4", None));
        assert!((o.spring_height - 0.4).abs() < 1e-12);
    }

    #[test]
    fn numeric_sill_and_height_unbind_planes() {
        let mut o = Opening::window(Handle::new(1), Handle::new(2), 1.0);
        o.sill_plane_id = Some(uuid::Uuid::new_v4());
        o.head_plane_id = Some(uuid::Uuid::new_v4());
        o.sill_plane_name = Some("A".into());
        o.head_plane_name = Some("B".into());
        assert!(apply_opening_property(&mut o, "opening_sill", "0.4", None));
        assert!((o.sill_height - 0.4).abs() < 1e-12);
        assert!(o.sill_plane_id.is_none());
        assert!(o.head_plane_id.is_some());
        assert!(apply_opening_property(&mut o, "opening_height", "1.5", None));
        assert!((o.height - 1.5).abs() < 1e-12);
        assert!(o.head_plane_id.is_none());
    }

    #[test]
    fn opening_plane_fields_follow_sill_then_offsets() {
        let storey = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        let mut o = Opening::window(Handle::new(1), Handle::new(2), 1.0);
        o.sill_plane_id = Some(storey.floor_plane_id);
        o.head_plane_id = Some(storey.ceiling_plane_id);
        let mut project = ProjectFile::default();
        let mut building = Building::new("B");
        building.storeys.push(storey);
        project.buildings.push(building);
        let section = opening_prop_section(&o, Handle::new(1), None, Some(&project));
        let fields: Vec<&str> = section.props.iter().map(|p| p.field).collect();
        let sill = fields.iter().position(|f| *f == "opening_sill").unwrap();
        let sill_pl = fields
            .iter()
            .position(|f| *f == "opening_sill_plane")
            .unwrap();
        let sill_off = fields
            .iter()
            .position(|f| *f == "opening_sill_offset")
            .unwrap();
        let head_pl = fields
            .iter()
            .position(|f| *f == "opening_head_plane")
            .unwrap();
        let head_off = fields
            .iter()
            .position(|f| *f == "opening_head_offset")
            .unwrap();
        assert!(sill < sill_pl && sill_pl < sill_off && sill_off < head_pl && head_pl < head_off);
    }

    #[test]
    fn slab_prop_section_exposes_style_justification_and_metrics() {
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2;
        use crate::modules::aec::engine::slab::{Slab, SlabJustification, SlabLayer};
        use crate::modules::aec::engine::slab_xdata::{slab_record_for_slab, write_slab_record};
        use crate::modules::aec::engine::wall_style::LayerFunction;
        use crate::modules::aec::engine::xdata::AEC_APPID;

        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(4.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(4.0, 3.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 3.0)));
        pl.is_closed = true;
        let mut entity = EntityType::LwPolyline(pl);
        let mut slab = Slab::new("style_slab_conc", 1);
        slab.justification = SlabJustification::StructuralTop;
        slab.layers = vec![
            SlabLayer::new("Tiles", 0.02, LayerFunction::Finish),
            SlabLayer::new("Concrete", 0.20, LayerFunction::Structural),
        ];
        slab.base_offset = 0.05;
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = slab_record_for_slab(&slab);
        entity.common_mut().extended_data.add_record(record);

        let section = slab_prop_section(&entity, None, None).expect("slab section");
        let fields: Vec<&str> = section.props.iter().map(|p| p.field).collect();
        assert!(fields.contains(&"slab_style"));
        assert!(fields.contains(&"slab_justification"));
        assert!(fields.contains(&"slab_phase"));
        assert!(!fields.contains(&"slab_base_plane"));
        assert!(fields.contains(&"slab_top_plane"));
        assert!(!fields.contains(&"slab_base_offset"));
        assert!(fields.contains(&"slab_top_offset"));
        assert!(fields.contains(&"slab_thickness"));
        assert!(fields.contains(&"slab_area"));
        assert!(fields.contains(&"slab_perimeter"));
        assert!(fields.contains(&"slab_volume"));
        assert!(fields.iter().any(|f| *f == "slab_layer"));

        let just = section
            .props
            .iter()
            .find(|p| p.field == "slab_justification")
            .expect("justification");
        match &just.value {
            crate::scene::model::object::PropValue::Choice { selected, .. } => {
                assert!(selected.contains("OKRD"));
            }
            _ => panic!("expected choice"),
        }

        // Keep write helper referenced for compile linkage in tests module.
        let _ = write_slab_record;
    }

    #[test]
    fn test_property_section_plane_dropdown_formatting_and_disambiguation() {
        use acadrust::entities::LwPolyline;
        use crate::modules::aec::engine::control_plane::{ControlPlane, ControlPlaneFacet};
        use crate::modules::aec::engine::openings::Opening;
        use crate::modules::aec::engine::slab::Slab;
        use crate::modules::aec::engine::slab_xdata::slab_record_for_slab;
        use crate::modules::aec::engine::wall::Wall;
        use crate::modules::aec::engine::xdata::{wall_record_for_wall, AEC_APPID};

        let mut project = ProjectFile::default();
        let mut building = Building::new("Hauptbau");
        let mut eg = StoreyRef::new_with_height("EG", 0.0, 2.80, "eg.dwg");
        let mut og = StoreyRef::new_with_height("OG", 2.80, 2.80, "og.dwg");

        let eg_custom_floor = ControlPlane::horizontal("Floor", 0.0);
        let eg_custom_floor_id = eg_custom_floor.id;
        eg.control_planes.push(eg_custom_floor);

        let og_custom_floor = ControlPlane::horizontal("Floor", 2.80);
        og.control_planes.push(og_custom_floor);

        let facet_roof = ControlPlaneFacet::new(
            "Nord",
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

        let eg_ceiling_id = eg.ceiling_plane_id;

        building.storeys.push(eg);
        building.storeys.push(og);
        project.buildings.push(building);

        // 1. Wall Property Section
        let mut wall = Wall::new("s", 2.80, 0);
        wall.base_plane_id = Some(eg_custom_floor_id);
        wall.top_plane_id = Some(roof_id);
        let mut wall_entity = EntityType::LwPolyline(LwPolyline::new());
        let mut rec = ExtendedDataRecord::new(AEC_APPID);
        rec.values = wall_record_for_wall(&wall);
        wall_entity.common_mut().extended_data.add_record(rec);

        let wall_sec = wall_prop_section(&wall_entity, None, Some(&project)).expect("wall section");
        let base_prop = wall_sec.props.iter().find(|p| p.field == "wall_base_plane").unwrap();
        match &base_prop.value {
            crate::scene::model::object::PropValue::Choice { selected, options } => {
                assert_eq!(selected, "Floor [EG] (Z = 0.000 m)");
                assert!(options.contains(&"Floor [EG] (Z = 0.000 m)".to_string()));
                assert!(options.contains(&"Floor [OG] (Z = 2.800 m)".to_string()));
                assert!(options.iter().any(|o| o.starts_with("Dach (Nord) (Z ≈ 6.000 m, ")));
            }
            _ => panic!("expected choice"),
        }
        let top_prop = wall_sec.props.iter().find(|p| p.field == "wall_top_plane").unwrap();
        match &top_prop.value {
            crate::scene::model::object::PropValue::Choice { selected, .. } => {
                assert!(selected.starts_with("Dach (Nord) (Z ≈ 6.000 m, "));
            }
            _ => panic!("expected choice"),
        }

        // 2. Opening Property Section
        let mut opening = Opening::window(Handle::new(10), Handle::new(20), 1.0);
        opening.sill_plane_id = Some(eg_custom_floor_id);
        opening.head_plane_id = Some(eg_ceiling_id);
        let op_sec = opening_prop_section(&opening, Handle::new(10), None, Some(&project));
        let sill_prop = op_sec.props.iter().find(|p| p.field == "opening_sill_plane").unwrap();
        match &sill_prop.value {
            crate::scene::model::object::PropValue::Choice { selected, .. } => {
                assert_eq!(selected, "Floor [EG] (Z = 0.000 m)");
            }
            _ => panic!("expected choice"),
        }

        // 3. Slab Property Section
        let mut slab = Slab::new("style_slab", 1);
        slab.top_plane_id = Some(roof_id);
        let mut slab_entity = EntityType::LwPolyline(LwPolyline::new());
        let mut srec = ExtendedDataRecord::new(AEC_APPID);
        srec.values = slab_record_for_slab(&slab);
        slab_entity.common_mut().extended_data.add_record(srec);

        let slab_sec = slab_prop_section(&slab_entity, None, Some(&project)).expect("slab section");
        let slab_top_prop = slab_sec.props.iter().find(|p| p.field == "slab_top_plane").unwrap();
        match &slab_top_prop.value {
            crate::scene::model::object::PropValue::Choice { selected, options } => {
                assert!(selected.starts_with("Dach (Nord) (Z ≈ 6.000 m, "));
                assert!(options.contains(&"Floor [EG] (Z = 0.000 m)".to_string()));
            }
            _ => panic!("expected choice"),
        }
    }

    #[test]
    fn slab_opening_prop_section_exposes_kind_and_depth() {
        use crate::modules::aec::engine::slab_opening::{
            SlabOpening, SlabOpeningDepth, SlabOpeningKind,
        };

        let opening = SlabOpening::new_recess(
            Handle::new(10),
            SlabOpeningKind::Shaft,
            0.12,
            vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
        );
        let entity = EntityType::LwPolyline(LwPolyline::new());
        let section = slab_opening_prop_section(&opening, Handle::new(11), &entity);
        let fields: Vec<&str> = section.props.iter().map(|p| p.field).collect();
        assert!(fields.contains(&"slabopening_kind"));
        assert!(fields.contains(&"slabopening_depth_mode"));
        assert!(fields.contains(&"slabopening_recess_depth"));
        assert!(fields.contains(&"slabopening_host"));
        assert!(fields.contains(&"slabopening_area"));
        assert_eq!(opening.depth, SlabOpeningDepth::Recess(0.12));
    }

    #[test]
    fn test_aec_entity_title_and_package_collapse_for_slabs_and_openings() {
        use crate::modules::aec::engine::library;
        use crate::modules::aec::engine::slab::Slab;
        use crate::modules::aec::engine::slab_package::{
            collect_slab_display_children, is_slab_derived, resolve_slab_package,
        };
        use crate::modules::aec::engine::slab_regen::regenerate_slab_representation;
        use crate::modules::aec::engine::slab_xdata::write_slab_record;
        use crate::scene::Scene;
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2;

        let mut scene = Scene::new();
        let lib = library::seed_default_library();

        let mut pl = LwPolyline::new();
        pl.is_closed = true;
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(6.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(6.0, 4.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 4.0)));
        let slab_h = scene.add_entity(EntityType::LwPolyline(pl));

        let slab = Slab::new("style_slab_concrete_20", 0);
        write_slab_record(&mut scene.document, slab_h, &slab);
        regenerate_slab_representation(&mut scene, slab_h, Some(&lib), None);

        let children = collect_slab_display_children(&scene, slab_h);
        assert!(!children.is_empty(), "must produce derived children");

        // Carrier entity title
        let carrier_ent = scene.document.get_entity(slab_h).unwrap();
        let title = aec_entity_title(&scene, slab_h, carrier_ent).expect("title");
        assert!(title.contains("Slab") || title.contains("Geschossdecke"));

        // For each derived child, resolve_aec_package and aec_entity_title must resolve to slab
        for &child_h in &children {
            assert!(is_slab_derived(&scene, child_h));
            assert_eq!(resolve_slab_package(&scene, child_h), slab_h);
            assert_eq!(resolve_aec_package(&scene, child_h), slab_h);
            let child_ent = scene.document.get_entity(child_h).unwrap();
            let child_title = aec_entity_title(&scene, child_h, child_ent).expect("child title");
            assert!(child_title.contains("Slab") || child_title.contains("Geschossdecke"));

            // Package collapse
            let selected = vec![(child_h, child_ent)];
            let collapsed = collapse_selection_to_aec_package(&scene, selected);
            assert_eq!(collapsed.len(), 1);
            assert_eq!(collapsed[0].0, slab_h);
        }
    }

    #[test]
    fn test_aec_entity_title_and_package_collapse_for_rooms() {
        use crate::modules::aec::engine::room::{Room, RoomFinish, RoomFunction};
        use crate::modules::aec::engine::room_package::{
            collect_room_display_children, expand_handles_for_room_packages, is_room_derived,
            resolve_room_package_handle,
        };
        use crate::modules::aec::engine::room_regen::regenerate_room_representation;
        use crate::modules::aec::engine::room_xdata::{room_from_entity, write_room_record};
        use crate::scene::Scene;
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2;

        let mut scene = Scene::new();

        let mut pl = LwPolyline::new();
        pl.is_closed = true;
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 4.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 4.0)));
        let room_h = scene.add_entity(EntityType::LwPolyline(pl));

        let room = Room::from_polygon("Living", &[(0.0, 0.0), (5.0, 0.0), (5.0, 4.0), (0.0, 4.0)], 2.50, 0)
            .with_number("101")
            .with_function(RoomFunction::Living)
            .with_floor_finish(vec![RoomFinish::new("Parkett", 0.05).with_hatch("ANSI31".to_string())]);
        write_room_record(&mut scene.document, room_h, &room);
        regenerate_room_representation(&mut scene, room_h, None);

        let children = collect_room_display_children(&scene, room_h);
        assert!(!children.is_empty(), "must produce derived children (stamp, hatch)");

        // Carrier entity title
        let carrier_ent = scene.document.get_entity(room_h).unwrap();
        let title = aec_entity_title(&scene, room_h, carrier_ent).expect("title");
        assert!(title.contains("Room") || title.contains("Raum"));

        // Carrier area
        let parsed_room = room_from_entity(carrier_ent).expect("parsed room");
        assert!((parsed_room.area - 20.0).abs() < 1e-4);

        // Package expansion
        let expanded = expand_handles_for_room_packages(&scene, &[room_h]);
        assert!(expanded.contains(&room_h));
        for &child_h in &children {
            assert!(expanded.contains(&child_h));
        }

        // For each derived child, resolve_aec_package and aec_entity_title must resolve to room
        for &child_h in &children {
            assert!(is_room_derived(scene.document.get_entity(child_h).unwrap()));
            assert_eq!(resolve_room_package_handle(&scene, child_h), room_h);
            assert_eq!(resolve_aec_package(&scene, child_h), room_h);
            let child_ent = scene.document.get_entity(child_h).unwrap();
            let child_title = aec_entity_title(&scene, child_h, child_ent).expect("child title");
            assert!(child_title.contains("Room") || child_title.contains("Raum"));

            // Package collapse for child
            let selected = vec![(child_h, child_ent)];
            let collapsed = collapse_selection_to_aec_package(&scene, selected);
            assert_eq!(collapsed.len(), 1);
            assert_eq!(collapsed[0].0, room_h);
        }

        // Multiple children + carrier collapse to single room
        let mut all_selected = vec![(room_h, carrier_ent)];
        for &child_h in &children {
            all_selected.push((child_h, scene.document.get_entity(child_h).unwrap()));
        }
        let collapsed_all = collapse_selection_to_aec_package(&scene, all_selected);
        assert_eq!(collapsed_all.len(), 1);
        assert_eq!(collapsed_all[0].0, room_h);
    }

    #[test]
    fn test_room_prop_section_modular_styles() {
        use crate::modules::aec::engine::room::{Room, RoomFunction};
        use crate::modules::aec::engine::room_xdata::write_room_record;
        use crate::modules::aec::engine::library;
        use crate::scene::Scene;
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2;

        let mut scene = Scene::new();
        let lib = library::seed_default_library();

        let mut pl = LwPolyline::new();
        pl.is_closed = true;
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 4.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 4.0)));
        let room_h = scene.add_entity(EntityType::LwPolyline(pl));

        let room = Room::from_polygon("Office", &[(0.0, 0.0), (5.0, 0.0), (5.0, 4.0), (0.0, 4.0)], 2.80, 1)
            .with_number("O-01")
            .with_function(RoomFunction::Office)
            .with_finish_style("finish_floor_parquet_80");
        write_room_record(&mut scene.document, room_h, &room);

        let entity = scene.document.get_entity(room_h).unwrap();
        let section = room_prop_section(entity, Some(&lib)).expect("room prop section");
        let fields: Vec<&str> = section.props.iter().map(|p| p.field).collect();
        assert!(fields.contains(&"room_name"));
        assert!(fields.contains(&"room_number"));
        assert!(fields.contains(&"room_function"));
        assert!(fields.contains(&"room_floor_finish_style"));
        assert!(fields.contains(&"room_floor_finish"));
        assert!(fields.contains(&"room_ceiling_finish"));

        let style_prop = section.props.iter().find(|p| p.field == "room_floor_finish_style").unwrap();
        if let crate::scene::model::object::PropValue::Choice { selected, options } = &style_prop.value {
            assert!(selected.contains("Parkett") || selected == "finish_floor_parquet_80");
            assert!(options.len() >= 5);
        } else {
            panic!("Expected choice prop");
        }
    }

    #[test]
    fn test_entity_property_sections_title_for_aec_slab_and_openings() {
        use crate::modules::aec::engine::openings::OpeningKind;
        use crate::modules::aec::engine::opening_xdata::place_wall_opening;
        use crate::modules::aec::engine::slab::Slab;
        use crate::modules::aec::engine::slab_xdata::write_slab_record;
        use crate::modules::aec::engine::wall::Wall;
        use crate::modules::aec::engine::wall_regen::regenerate_wall_representation;
        use crate::modules::aec::engine::xdata::{AEC_APPID, wall_record_for_wall};
        use crate::scene::Scene;
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2;
        use acadrust::xdata::ExtendedDataRecord;

        let mut scene = Scene::new();

        // 1. Slab carrier
        let mut pl = LwPolyline::new();
        pl.is_closed = true;
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(4.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(4.0, 4.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 4.0)));
        let slab_h = scene.add_entity(EntityType::LwPolyline(pl));
        let slab = Slab::new("style_slab_concrete_20", 0);
        write_slab_record(&mut scene.document, slab_h, &slab);

        let slab_ent = scene.document.get_entity(slab_h).unwrap();
        let slab_title = aec_entity_title(&scene, slab_h, slab_ent).expect("slab title");
        assert_eq!(slab_title, crate::tr!("aec", "slab-title"));

        // 2. Wall and Openings
        let mut wall_pl = LwPolyline::new();
        wall_pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        wall_pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        let mut wall_ent = EntityType::LwPolyline(wall_pl);
        let mut rec = ExtendedDataRecord::new(AEC_APPID);
        let mut wall = Wall::new("Standard", 2.80, 0);
        wall.layers = vec![crate::modules::aec::engine::wall::WallLayer {
            material: "Brick".to_string(),
            thickness: 0.24,
            ..Default::default()
        }];
        rec.values = wall_record_for_wall(&wall);
        wall_ent.common_mut().extended_data.add_record(rec);
        let wall_h = scene.add_entity(wall_ent);
        regenerate_wall_representation(&mut scene, wall_h, None).expect("regen wall");

        let (win_h, _) = place_wall_opening(
            &mut scene,
            wall_h,
            glam::DVec3::new(2.5, 0.0, 0.0),
            OpeningKind::Window,
            None,
            None,
            None,
        ).expect("place window");
        let win_ent = scene.document.get_entity(win_h).unwrap();
        let win_title = aec_entity_title(&scene, win_h, win_ent).expect("win title");
        assert_eq!(win_title, crate::tr!("aec", "window-title"));

        let (door_h, _) = place_wall_opening(
            &mut scene,
            wall_h,
            glam::DVec3::new(6.0, 0.0, 0.0),
            OpeningKind::Door,
            None,
            None,
            None,
        ).expect("place door");
        let door_ent = scene.document.get_entity(door_h).unwrap();
        let door_title = aec_entity_title(&scene, door_h, door_ent).expect("door title");
        assert_eq!(door_title, crate::tr!("aec", "door-title"));
    }

    #[test]
    fn test_slab_with_openings_selection_collapse_and_title() {
        use crate::modules::aec::engine::library;
        use crate::modules::aec::engine::slab::Slab;
        use crate::modules::aec::engine::slab_opening::{SlabOpening, SlabOpeningKind};
        use crate::modules::aec::engine::slab_package::{
            expand_handles_for_slab_packages, slab_opening_package_handles, slab_package_handles,
        };
        use crate::modules::aec::engine::slab_regen::{
            regenerate_slab_opening_representation, regenerate_slab_representation,
        };
        use crate::modules::aec::engine::slab_xdata::{
            add_slab_opening_handle, write_slab_opening_record, write_slab_record,
        };
        use crate::scene::Scene;
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2;

        let mut scene = Scene::new();
        let lib = library::seed_default_library();

        // 1. Create slab carrier (10x10)
        let mut pl = LwPolyline::new();
        pl.is_closed = true;
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(10.0, 10.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 10.0)));
        let slab_h = scene.add_entity(EntityType::LwPolyline(pl));

        let slab = Slab::new("style_slab_concrete_20", 0);
        write_slab_record(&mut scene.document, slab_h, &slab);

        // 2. Create slab opening carrier (2x2 inside slab)
        let mut op_pl = LwPolyline::new();
        op_pl.is_closed = true;
        op_pl.add_vertex(LwVertex::new(Vector2::new(3.0, 3.0)));
        op_pl.add_vertex(LwVertex::new(Vector2::new(5.0, 3.0)));
        op_pl.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
        op_pl.add_vertex(LwVertex::new(Vector2::new(3.0, 5.0)));
        let op_h = scene.add_entity(EntityType::LwPolyline(op_pl));

        let opening = SlabOpening::new(
            slab_h,
            SlabOpeningKind::Shaft,
            vec![(3.0, 3.0), (5.0, 3.0), (5.0, 5.0), (3.0, 5.0)],
        );
        write_slab_opening_record(&mut scene.document, op_h, &opening);
        add_slab_opening_handle(&mut scene.document, slab_h, op_h);

        // 3. Regenerate representations
        regenerate_slab_representation(&mut scene, slab_h, Some(&lib), None);
        regenerate_slab_opening_representation(&mut scene, op_h, Some(&lib), None);

        // 4. Check slab package handles: must NOT include the opening carrier or opening children
        let slab_pkg = slab_package_handles(&scene, slab_h);
        assert!(slab_pkg.contains(&slab_h));
        assert!(!slab_pkg.contains(&op_h), "Slab package must not contain opening carrier");

        let op_pkg = slab_opening_package_handles(&scene, op_h);
        assert!(op_pkg.contains(&op_h));
        assert!(!op_pkg.contains(&slab_h), "Opening package must not contain slab carrier");

        for op_child in &op_pkg {
            assert!(!slab_pkg.contains(op_child), "Slab package must not contain opening display children");
        }

        // 5. Check selection expansion for slab
        let expanded_slab = expand_handles_for_slab_packages(&scene, &[slab_h]);
        assert!(expanded_slab.contains(&slab_h));
        assert!(!expanded_slab.contains(&op_h));

        // 6. When slab is selected (with all its derived geometry), collapse must yield exactly 1 entity
        let selected_slab_entities: Vec<(Handle, &EntityType)> = expanded_slab
            .iter()
            .map(|h| (*h, scene.document.get_entity(*h).unwrap()))
            .collect();
        let collapsed_slab = collapse_selection_to_aec_package(&scene, selected_slab_entities);
        assert_eq!(collapsed_slab.len(), 1, "Slab with openings must collapse to exactly 1 entity");
        assert_eq!(collapsed_slab[0].0, slab_h);

        let slab_title = aec_entity_title(&scene, collapsed_slab[0].0, collapsed_slab[0].1).expect("slab title");
        assert_eq!(slab_title, crate::tr!("aec", "slab-title"));

        // 7. When opening is selected, collapse must yield exactly 1 entity
        let expanded_op = expand_handles_for_slab_packages(&scene, &[op_h]);
        let selected_op_entities: Vec<(Handle, &EntityType)> = expanded_op
            .iter()
            .map(|h| (*h, scene.document.get_entity(*h).unwrap()))
            .collect();
        let collapsed_op = collapse_selection_to_aec_package(&scene, selected_op_entities);
        assert_eq!(collapsed_op.len(), 1, "Opening must collapse to exactly 1 entity");
        assert_eq!(collapsed_op[0].0, op_h);

        let op_title = aec_entity_title(&scene, collapsed_op[0].0, collapsed_op[0].1).expect("opening title");
        assert_eq!(op_title, crate::tr!("aec", "slabopening-title"));
    }

    fn prop_text(section: &crate::scene::model::object::PropSection, field: &str) -> String {
        match &section.props.iter().find(|p| p.field == field).expect(field).value {
            crate::scene::model::object::PropValue::ReadOnly(s)
            | crate::scene::model::object::PropValue::PlainText(s)
            | crate::scene::model::object::PropValue::EditText(s) => s.clone(),
            crate::scene::model::object::PropValue::EntityRef { handle, .. } => handle.to_string(),
            other => panic!("{field} unexpected {other:?}"),
        }
    }

    fn prop_edit_number(section: &crate::scene::model::object::PropSection, field: &str) -> f64 {
        match &section.props.iter().find(|p| p.field == field).expect(field).value {
            crate::scene::model::object::PropValue::EditText(s) => {
                crate::entities::common::parse_f64(s).unwrap_or_else(|| s.parse().expect(field))
            }
            other => panic!("{field} unexpected {other:?}"),
        }
    }

    fn sample_section_viewpoint(
        vp_id: crate::modules::aec::engine::views::ViewpointId,
        name: &str,
        path: crate::modules::aec::engine::views::viewpoint::SectionPath,
        far_depth: Option<f64>,
        z_min: f64,
        z_max: f64,
        flip: bool,
    ) -> crate::modules::aec::engine::views::viewpoint::Viewpoint {
        use crate::modules::aec::engine::views::viewpoint::{
            ProjectionMapping, VerticalRange, ViewAxes, ViewKind, ViewOrigin, Viewpoint,
        };
        use crate::modules::aec::engine::views::{Metres, Point3, PositiveMetres, Revision, Vector2};
        Viewpoint {
            id: vp_id,
            name: name.into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, z_min)),
            axes: ViewAxes::section_with_side(Vector2::new(1.0, 0.0), flip).unwrap(),
            kind: ViewKind::Section {
                path,
                far_depth: far_depth.and_then(|d| PositiveMetres::new(d).ok()),
            },
            z_range: VerticalRange::new(Metres::new(z_min).unwrap(), Metres::new(z_max).unwrap())
                .unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        }
    }

    fn sample_model_view(
        mv_id: crate::modules::aec::engine::views::ModelViewId,
        vp_id: crate::modules::aec::engine::views::ViewpointId,
        name: &str,
        policy: crate::modules::aec::engine::views::model_view::UpdatePolicy,
    ) -> crate::modules::aec::engine::views::model_view::ModelView {
        use crate::modules::aec::engine::views::model_view::{DetailPolicy, ModelView, ViewRevisions};
        use crate::modules::aec::engine::views::{ScaleDenominator, ViewProfileId};
        ModelView {
            id: mv_id,
            name: name.into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: std::collections::BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: policy,
            revisions: ViewRevisions::default(),
        }
    }

    fn viewpoint_far_depth(
        vp: &crate::modules::aec::engine::views::viewpoint::Viewpoint,
    ) -> Option<f64> {
        use crate::modules::aec::engine::views::viewpoint::ViewKind;
        match &vp.kind {
            ViewKind::Section { far_depth, .. } => far_depth.as_ref().map(|d| d.get()),
            ViewKind::Elevation { depth } => depth.far.as_ref().map(|d| d.get()),
            _ => None,
        }
    }

    fn add_test_wall(doc: &mut acadrust::CadDocument) -> Handle {
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2 as CadVec2;
        use acadrust::xdata::ExtendedDataRecord;
        use crate::modules::aec::engine::plan_view::PlanPhase;
        use crate::modules::aec::engine::wall::WallJustification;
        use crate::modules::aec::engine::WallLayer;
        use crate::modules::aec::engine::xdata::{wall_record, AEC_APPID};

        let mut pl = LwPolyline::default();
        pl.vertices.push(LwVertex {
            location: CadVec2::new(5.0, -2.0),
            bulge: 0.0,
            start_width: 0.0,
            end_width: 0.0,
            vertex_id: 0,
        });
        pl.vertices.push(LwVertex {
            location: CadVec2::new(5.0, 2.0),
            bulge: 0.0,
            start_width: 0.0,
            end_width: 0.0,
            vertex_id: 1,
        });
        let handle = doc.allocate_handle();
        pl.common.handle = handle;
        let layer = WallLayer {
            material: "Concrete".into(),
            thickness: 0.24,
            function: "Structural".into(),
            axis_offset: -0.12,
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
        };
        let rec_values = wall_record(
            "W1",
            2.8,
            0,
            &[layer],
            &[],
            WallJustification::Center,
            PlanPhase::Existing,
            None,
        );
        let mut rec = ExtendedDataRecord::new(AEC_APPID);
        rec.values = rec_values;
        pl.common.extended_data.add_record(rec);
        let _ = doc.add_entity(EntityType::LwPolyline(pl));
        handle
    }

    #[test]
    fn test_section_marker_prop_section() {
        use crate::modules::aec::engine::views::viewpoint::SectionPath;
        use crate::modules::aec::engine::views::{Point2, ViewpointId};
        use crate::modules::aec::views::din1356::{create_section_marker, Din1356SectionMarker};
        use crate::scene::Scene;

        let mut scene = Scene::new();
        let vp_id = ViewpointId::new();
        let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let marker = Din1356SectionMarker::new_section("A - A", path, vp_id, Some(Handle::new(77)))
            .with_depth(Some(6.0))
            .with_z_range(0.0, 3.5);
        let marker_h = create_section_marker(&mut scene.document, &marker);

        let entity = scene.document.get_entity(marker_h).expect("marker entity");
        let section = marker_prop_section(entity, &scene.document).expect("marker prop section");

        assert_eq!(section.title, crate::tr!("aec", "section-marker-title"));
        assert_eq!(
            prop_text(&section, "marker_type"),
            crate::tr!("aec", "marker-type-section")
        );
        assert_eq!(prop_text(&section, "marker_label"), "A - A");
        assert!((prop_edit_number(&section, "marker_far_depth") - 6.0).abs() < 1e-6);
        assert!((prop_edit_number(&section, "marker_z_min") - 0.0).abs() < 1e-6);
        assert!((prop_edit_number(&section, "marker_z_max") - 3.5).abs() < 1e-6);
        match &section
            .props
            .iter()
            .find(|p| p.field == "marker_flip")
            .expect("marker_flip")
            .value
        {
            crate::scene::model::object::PropValue::Choice { selected, options } => {
                assert_eq!(selected, &crate::tr!("aec", "marker-flip-normal"));
                assert!(options.contains(&crate::tr!("aec", "marker-flip-flipped")));
            }
            other => panic!("wrong prop type for marker_flip: {other:?}"),
        }
        assert_eq!(
            prop_text(&section, "marker_carrier"),
            Handle::new(77).to_string()
        );
    }

    #[test]
    fn test_elevation_marker_prop_section() {
        use crate::modules::aec::engine::views::{Point2, ViewpointId};
        use crate::modules::aec::views::din1356::{create_section_marker, Din1356SectionMarker};
        use crate::scene::Scene;

        let mut scene = Scene::new();
        let vp_id = ViewpointId::new();
        let marker = Din1356SectionMarker::new_elevation(
            "Südansicht",
            Point2::new(0.0, 0.0),
            Point2::new(12.0, 0.0),
            vp_id,
            None,
        )
        .with_depth(Some(15.0))
        .with_flip(true);

        let marker_h = create_section_marker(&mut scene.document, &marker);
        let entity = scene.document.get_entity(marker_h).expect("elevation entity");
        let section = marker_prop_section(entity, &scene.document).expect("marker prop section");

        assert_eq!(section.title, crate::tr!("aec", "elevation-marker-title"));
        assert_eq!(
            prop_text(&section, "marker_type"),
            crate::tr!("aec", "marker-type-elevation")
        );
        match &section
            .props
            .iter()
            .find(|p| p.field == "marker_flip")
            .expect("marker_flip")
            .value
        {
            crate::scene::model::object::PropValue::Choice { selected, .. } => {
                assert_eq!(selected, &crate::tr!("aec", "marker-flip-flipped"));
            }
            other => panic!("wrong prop type for marker_flip: {other:?}"),
        }
    }

    #[test]
    fn test_apply_marker_property() {
        use crate::modules::aec::engine::views::viewpoint::SectionPath;
        use crate::modules::aec::engine::views::{Point2, ViewpointId};
        use crate::modules::aec::views::din1356::{
            create_section_marker, read_section_marker, Din1356SectionMarker,
        };
        use crate::scene::Scene;

        let mut scene = Scene::new();
        let vp_id = ViewpointId::new();
        let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(8.0, 0.0));
        let marker = Din1356SectionMarker::new_section("A - A", path, vp_id, None);
        let marker_h = create_section_marker(&mut scene.document, &marker);

        assert!(apply_marker_property(
            &mut scene.document,
            marker_h,
            "marker_label",
            "B - B"
        ));
        assert!(apply_marker_property(
            &mut scene.document,
            marker_h,
            "marker_far_depth",
            "12.5"
        ));
        assert!(apply_marker_property(
            &mut scene.document,
            marker_h,
            "marker_z_min",
            "-0.8"
        ));
        assert!(apply_marker_property(
            &mut scene.document,
            marker_h,
            "marker_z_max",
            "4.2"
        ));
        assert!(apply_marker_property(
            &mut scene.document,
            marker_h,
            "marker_flip",
            CHOICE_FLIPPED
        ));

        let ent = scene.document.get_entity(marker_h).expect("marker ent");
        let updated = read_section_marker(&scene.document, ent).expect("read updated marker");

        assert_eq!(updated.label, "B - B");
        assert_eq!(updated.far_depth, Some(12.5));
        assert!((updated.z_min - (-0.8)).abs() < 1e-6);
        assert!((updated.z_max - 4.2).abs() < 1e-6);
        assert!(updated.flip_direction);
    }

    #[test]
    fn test_carrier_prop_section_and_apply() {
        use crate::modules::aec::engine::views::carrier::{
            create_view_carrier, find_view_carrier_by_handle, read_view_carrier_definitions,
            write_view_carrier_definitions,
        };
        use crate::modules::aec::engine::views::model_view::UpdatePolicy;
        use crate::modules::aec::engine::views::viewpoint::SectionPath;
        use crate::modules::aec::engine::views::{ModelViewId, Point2, ViewpointId};
        use crate::scene::Scene;

        let mut scene = Scene::new();
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();
        let carrier = create_view_carrier(
            &mut scene.document,
            vp_id,
            mv_id,
            Point2::new(0.0, 0.0),
            1.0,
            0.0,
            UpdatePolicy::Manual,
        )
        .expect("carrier created");
        let pl_h = carrier.handle;

        let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let vp = sample_section_viewpoint(vp_id, "Schnitt 1", path, None, 0.0, 3.0, false);
        let mv = sample_model_view(mv_id, vp_id, "MV 1", UpdatePolicy::Manual);
        write_view_carrier_definitions(&mut scene.document, pl_h, &vp, &mv);

        let entity = scene.document.get_entity(pl_h).unwrap();
        let section = carrier_prop_section(entity, &scene.document).expect("carrier section");

        assert_eq!(section.title, crate::tr!("aec", "view-carrier-title"));
        // Displays ModelView scale denominator 1:50, not geometric multiplier (1.0)
        assert!((prop_edit_number(&section, "carrier_scale") - 50.0).abs() < 1e-6);
        assert_eq!(prop_text(&section, "carrier_name"), "Schnitt 1");

        assert!(apply_carrier_property(
            &mut scene.document,
            pl_h,
            "carrier_scale",
            "100.0"
        ));
        assert!(apply_carrier_property(
            &mut scene.document,
            pl_h,
            "carrier_update_policy",
            CHOICE_AUTOMATIC
        ));

        let updated_carrier = find_view_carrier_by_handle(&scene.document, pl_h).expect("carrier");
        // Carrier geometric multiplier remains 1.0
        assert!((updated_carrier.scale - 1.0).abs() < 1e-6);
        assert_eq!(updated_carrier.update_policy, UpdatePolicy::Automatic);

        // ModelView drawing scale denominator is updated to 100.0 (1:100)
        let (_, updated_mv) = read_view_carrier_definitions(&scene.document, pl_h).expect("defs");
        assert!((updated_mv.scale.get() - 100.0).abs() < 1e-6);
    }

    #[test]
    fn test_floorplan_carrier_cut_height_prop_and_grips() {
        use crate::modules::aec::engine::views::carrier::{
            create_view_carrier, read_view_carrier_definitions,
            write_view_carrier_definitions,
        };
        use crate::modules::aec::engine::views::model_view::{DetailPolicy, ModelView, ViewRevisions};
        use crate::modules::aec::engine::views::viewpoint::{PlanRange, VerticalRange, ViewAxes, ViewKind, ViewOrigin, Viewpoint};
        use crate::modules::aec::engine::views::{
            Metres, ModelViewId, NonNegativeMetres, Point2, Point3, Revision, ScaleDenominator,
            ViewProfileId, ViewpointId,
        };
        use crate::modules::aec::engine::views::model_view::UpdatePolicy;
        use crate::scene::Scene;

        let mut scene = Scene::new();
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let viewpoint = Viewpoint {
            id: vp_id,
            name: "EG Grundriss".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(10.0, 20.0, 1.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.8).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(2.8).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };

        let model_view = ModelView {
            id: mv_id,
            name: "1:50 EG".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: UpdatePolicy::Manual,
            revisions: ViewRevisions::default(),
        };

        let carrier = create_view_carrier(
            &mut scene.document,
            vp_id,
            mv_id,
            Point2::new(40.0, 10.0),
            1.0,
            0.0,
            UpdatePolicy::Manual,
        )
        .expect("carrier created");
        let carrier_h = carrier.handle;
        write_view_carrier_definitions(&mut scene.document, carrier_h, &viewpoint, &model_view);

        let entity = scene.document.get_entity(carrier_h).unwrap();
        let section = carrier_prop_section(entity, &scene.document).expect("carrier section");

        // Verify that cut height is displayed (0.0 + 1.0 = 1.0)
        assert!((prop_edit_number(&section, "carrier_cut_height") - 1.0).abs() < 1e-4);

        // Apply new cut height: 1.5m
        assert!(apply_carrier_property(
            &mut scene.document,
            carrier_h,
            "carrier_cut_height",
            "1.5"
        ));

        let (updated_vp, _) = read_view_carrier_definitions(&scene.document, carrier_h).expect("defs");
        match updated_vp.kind {
            ViewKind::FloorPlan { range } => {
                assert!((range.below_cut.get() - 1.5).abs() < 1e-4);
            }
            _ => panic!("expected floor plan"),
        }

        // Verify grips
        let mut grips = Vec::new();
        append_view_carrier_grips(&scene, carrier_h, &mut grips);
        assert_eq!(grips.len(), 2);
        let ins_grip = grips.iter().find(|g| g.id == CARRIER_GRIP_INSERTION).unwrap();
        assert_eq!(ins_grip.world, glam::DVec3::new(40.0, 10.0, 0.0));
        let cut_grip = grips.iter().find(|g| g.id == CARRIER_GRIP_CUT_HEIGHT).unwrap();
        // Cut height 1.5m offset in Y from insertion point (10.0 + 1.5 = 11.5)
        assert!((cut_grip.world.y - 11.5).abs() < 1e-4);
    }

    #[test]
    fn test_append_section_marker_grips_straight_and_jogged() {
        use crate::modules::aec::engine::views::viewpoint::{SectionPath, SectionSegment};
        use crate::modules::aec::engine::views::{Point2, ViewpointId};
        use crate::modules::aec::views::din1356::{create_section_marker, Din1356SectionMarker};
        use crate::scene::model::object::GripShape;
        use crate::scene::Scene;

        let mut scene = Scene::new();
        let vp_id = ViewpointId::new();

        // 1. Straight marker
        let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let marker = Din1356SectionMarker::new_section("A - A", path, vp_id, None)
            .with_depth(Some(4.0));
        let straight_h = create_section_marker(&mut scene.document, &marker);

        let mut grips = Vec::new();
        append_section_marker_grips(&scene, straight_h, &mut grips);

        assert_eq!(grips.len(), 4); // Start, End, Depth, Flip
        let start_grip = grips.iter().find(|g| g.id == SECTION_MARKER_START_GRIP).unwrap();
        assert_eq!(start_grip.world, glam::DVec3::new(0.0, 0.0, 0.0));
        assert_eq!(start_grip.shape, GripShape::Square);

        let end_grip = grips.iter().find(|g| g.id == SECTION_MARKER_END_GRIP).unwrap();
        assert_eq!(end_grip.world, glam::DVec3::new(10.0, 0.0, 0.0));
        assert_eq!(end_grip.shape, GripShape::Square);

        let depth_grip = grips.iter().find(|g| g.id == SECTION_MARKER_DEPTH_GRIP).unwrap();
        assert_eq!(depth_grip.world, glam::DVec3::new(5.0, 4.0, 0.0));
        assert!(matches!(depth_grip.shape, GripShape::Rectangle { .. }));
        assert_eq!(depth_grip.axis, Some(glam::DVec3::new(0.0, 1.0, 0.0)));

        let flip_grip = grips.iter().find(|g| g.id == SECTION_MARKER_FLIP_GRIP).unwrap();
        assert_eq!(flip_grip.world, glam::DVec3::new(5.0, 1.0, 0.0));
        assert!(matches!(flip_grip.shape, GripShape::Triangle { .. }));

        // 2. Jogged marker (3 segments)
        let jogged_path = SectionPath::jogged(vec![
            SectionSegment::new(Point2::new(0.0, 0.0), Point2::new(5.0, 0.0), false),
            SectionSegment::new(Point2::new(5.0, 0.0), Point2::new(5.0, 2.0), true),
            SectionSegment::new(Point2::new(5.0, 2.0), Point2::new(10.0, 2.0), false),
        ]);
        let jogged_marker = Din1356SectionMarker::new_section("B - B", jogged_path, vp_id, None)
            .with_depth(Some(3.0));
        let jogged_h = create_section_marker(&mut scene.document, &jogged_marker);

        let mut jog_grips = Vec::new();
        append_section_marker_grips(&scene, jogged_h, &mut jog_grips);

        // Start (0), End (1), Jog0 (100), Jog1 (101), Depth (2), Flip (3) => 6 grips
        assert_eq!(jog_grips.len(), 6);
        let jog0 = jog_grips.iter().find(|g| g.id == SECTION_MARKER_JOG_BASE).unwrap();
        assert_eq!(jog0.world, glam::DVec3::new(5.0, 0.0, 0.0));
        let jog1 = jog_grips.iter().find(|g| g.id == SECTION_MARKER_JOG_BASE + 1).unwrap();
        assert_eq!(jog1.world, glam::DVec3::new(5.0, 2.0, 0.0));
    }

    #[test]
    fn test_section_marker_package_resolution_and_expansion() {
        use crate::modules::aec::engine::owner_index;
        use crate::modules::aec::engine::views::viewpoint::SectionPath;
        use crate::modules::aec::engine::views::{Point2, ViewpointId};
        use crate::modules::aec::views::din1356::{
            create_section_marker, section_marker_owner_from_entity, Din1356SectionMarker,
        };
        use crate::scene::Scene;
        use acadrust::entities::LwPolyline;

        let mut scene = Scene::new();
        let vp_id = ViewpointId::new();
        let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let marker = Din1356SectionMarker::new_section("A - A", path, vp_id, None);
        let marker_h = create_section_marker(&mut scene.document, &marker);

        let children = owner_index::children_of(&scene.document, marker_h);
        assert!(!children.is_empty(), "Marker should have child CAD entities");

        let child_h = children[0];
        {
            let child_ent = scene.document.get_entity(child_h).unwrap();
            assert_eq!(section_marker_owner_from_entity(child_ent), Some(marker_h));
        }

        // 1. Package resolution is O(1) via the child owner back-reference.
        assert_eq!(resolve_section_marker_package(&scene, marker_h), Some(marker_h));
        assert_eq!(resolve_section_marker_package(&scene, child_h), Some(marker_h));
        assert_eq!(resolve_aec_package(&scene, child_h), marker_h);

        let unrelated = scene.add_entity(EntityType::LwPolyline(LwPolyline::new()));
        assert_eq!(resolve_section_marker_package(&scene, unrelated), None);

        // 2. Derived non-carrier check
        assert!(!is_section_marker_derived(&scene, marker_h));
        assert!(is_section_marker_derived(&scene, child_h));
        assert!(is_aec_derived_non_carrier(&scene, child_h));

        // 3. Selection expansion
        let expanded = expand_handles_for_aec_packages(&scene, &[child_h]);
        assert!(expanded.contains(&marker_h));
        for c in &children {
            assert!(expanded.contains(c));
        }

        // 4. Title resolution on child entity
        let child_ent = scene.document.get_entity(child_h).unwrap();
        let title = aec_entity_title(&scene, child_h, child_ent).expect("title");
        assert_eq!(title, crate::tr!("aec", "section-marker-title"));
    }

    #[test]
    fn test_marker_carrier_sync_roundtrip_manual_and_automatic() {
        use crate::modules::aec::engine::views::carrier::{
            create_view_carrier, find_view_carrier_by_handle, read_view_carrier_definitions,
            write_view_carrier_definitions,
        };
        use crate::modules::aec::engine::views::model_view::UpdatePolicy;
        use crate::modules::aec::engine::views::viewpoint::{SectionPath, ViewAxes};
        use crate::modules::aec::engine::views::{ModelViewId, Point2, Vector2, ViewpointId};
        use crate::modules::aec::views::din1356::{
            create_section_marker, read_section_marker, Din1356SectionMarker,
        };
        use crate::scene::Scene;

        let mut scene = Scene::new();
        // Add real wall geometry to the scene document so cuts produce derived geometry.
        add_test_wall(&mut scene.document);
        let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));

        // --- Manual: marker edits mark the carrier outdated and update Viewpoint ---
        let vp_id_manual = ViewpointId::new();
        let mv_id_manual = ModelViewId::new();
        let carrier_manual = create_view_carrier(
            &mut scene.document,
            vp_id_manual,
            mv_id_manual,
            Point2::new(20.0, 0.0),
            1.0, // Placement scale multiplier stays 1.0
            0.0,
            UpdatePolicy::Manual,
        )
        .expect("manual carrier");
        let vp_manual = sample_section_viewpoint(
            vp_id_manual,
            "A - A",
            path.clone(),
            Some(5.0),
            0.0,
            3.0,
            false,
        );
        let mv_manual = sample_model_view(
            mv_id_manual,
            vp_id_manual,
            "Section 1:50",
            UpdatePolicy::Manual,
        );
        write_view_carrier_definitions(&mut scene.document, carrier_manual.handle, &vp_manual, &mv_manual);

        let marker_manual = Din1356SectionMarker::new_section(
            "A - A",
            path.clone(),
            vp_id_manual,
            Some(carrier_manual.handle),
        )
        .with_depth(Some(5.0))
        .with_z_range(0.0, 3.0);
        let marker_manual_h = create_section_marker(&mut scene.document, &marker_manual);

        assert!(apply_marker_property(
            &mut scene.document,
            marker_manual_h,
            "marker_far_depth",
            "8.0"
        ));
        assert!(apply_marker_property(
            &mut scene.document,
            marker_manual_h,
            "marker_z_min",
            "-0.5"
        ));
        assert!(apply_marker_property(
            &mut scene.document,
            marker_manual_h,
            "marker_z_max",
            "4.0"
        ));
        assert!(apply_marker_property(
            &mut scene.document,
            marker_manual_h,
            "marker_flip",
            CHOICE_FLIPPED
        ));
        assert!(apply_marker_property(
            &mut scene.document,
            marker_manual_h,
            "marker_label",
            "A1 - A1"
        ));

        let (vp_after, mv_after) =
            read_view_carrier_definitions(&scene.document, carrier_manual.handle).expect("defs");
        let carrier_after =
            find_view_carrier_by_handle(&scene.document, carrier_manual.handle).expect("carrier");
        assert!(carrier_after.is_outdated);
        assert_eq!(carrier_after.scale, 1.0);
        assert_eq!(vp_after.name, "A1 - A1");
        assert_eq!(mv_after.name, "Section A1 - A1");
        assert_eq!(viewpoint_far_depth(&vp_after), Some(8.0));
        assert!((vp_after.z_range.z_min.get() - (-0.5)).abs() < 1e-9);
        assert!((vp_after.z_range.z_max.get() - 4.0).abs() < 1e-9);
        assert_eq!(
            vp_after.axes,
            ViewAxes::section_with_side(Vector2::new(1.0, 0.0), true).unwrap()
        );

        // "Update now" action on outdated manual carrier must regenerate and clear outdated flag
        assert!(apply_carrier_property(
            &mut scene.document,
            carrier_manual.handle,
            "carrier_action",
            CHOICE_ACTION_UPDATE
        ));
        let carrier_manual_updated =
            find_view_carrier_by_handle(&scene.document, carrier_manual.handle).expect("carrier");
        assert!(!carrier_manual_updated.is_outdated, "Update now clears is_outdated");
        assert!(!carrier_manual_updated.derived_handles.is_empty(), "Update now rebuilds derived CAD handles");

        // Changing marker parameter again marks it outdated
        assert!(apply_marker_property(
            &mut scene.document,
            marker_manual_h,
            "marker_far_depth",
            "6.0"
        ));
        let carrier_dirty_again =
            find_view_carrier_by_handle(&scene.document, carrier_manual.handle).expect("carrier");
        assert!(carrier_dirty_again.is_outdated);

        // Switching policy from Manual to Automatic while outdated must immediately regenerate
        assert!(apply_carrier_property(
            &mut scene.document,
            carrier_manual.handle,
            "carrier_update_policy",
            CHOICE_AUTOMATIC
        ));
        let carrier_switched =
            find_view_carrier_by_handle(&scene.document, carrier_manual.handle).expect("carrier");
        assert_eq!(carrier_switched.update_policy, UpdatePolicy::Automatic);
        assert!(!carrier_switched.is_outdated, "Switching manual->auto while outdated must regenerate");
        assert!(!carrier_switched.derived_handles.is_empty());

        // --- Automatic: marker edits regenerate and keep the carrier current ---
        let vp_id_auto = ViewpointId::new();
        let mv_id_auto = ModelViewId::new();
        let carrier_auto = create_view_carrier(
            &mut scene.document,
            vp_id_auto,
            mv_id_auto,
            Point2::new(40.0, 0.0),
            1.0, // Placement scale multiplier stays 1.0
            0.0,
            UpdatePolicy::Automatic,
        )
        .expect("auto carrier");
        let vp_auto = sample_section_viewpoint(
            vp_id_auto,
            "B - B",
            path.clone(),
            Some(5.0),
            0.0,
            3.0,
            false,
        );
        let mv_auto = sample_model_view(
            mv_id_auto,
            vp_id_auto,
            "Section auto",
            UpdatePolicy::Automatic,
        );
        write_view_carrier_definitions(&mut scene.document, carrier_auto.handle, &vp_auto, &mv_auto);

        let marker_auto = Din1356SectionMarker::new_section(
            "B - B",
            path,
            vp_id_auto,
            Some(carrier_auto.handle),
        )
        .with_depth(Some(5.0))
        .with_z_range(0.0, 3.0);
        let marker_auto_h = create_section_marker(&mut scene.document, &marker_auto);

        assert!(apply_marker_property(
            &mut scene.document,
            marker_auto_h,
            "marker_far_depth",
            "9.5"
        ));

        let (vp_auto_after, _) =
            read_view_carrier_definitions(&scene.document, carrier_auto.handle).expect("auto defs");
        let carrier_auto_after =
            find_view_carrier_by_handle(&scene.document, carrier_auto.handle).expect("auto carrier");
        assert!(!carrier_auto_after.is_outdated, "Automatic carrier must be current after marker edit");
        assert!(!carrier_auto_after.derived_handles.is_empty(), "Automatic carrier must have derived geometry");
        assert_eq!(carrier_auto_after.scale, 1.0);
        assert_eq!(viewpoint_far_depth(&vp_auto_after), Some(9.5));

        // Carrier edits propagate back onto the linked marker.
        assert!(apply_carrier_property(
            &mut scene.document,
            carrier_auto.handle,
            "carrier_name",
            "Nord"
        ));
        assert!(apply_carrier_property(
            &mut scene.document,
            carrier_auto.handle,
            "carrier_far_depth",
            "11.0"
        ));
        assert!(apply_carrier_property(
            &mut scene.document,
            carrier_auto.handle,
            "carrier_z_min",
            "0.2"
        ));
        assert!(apply_carrier_property(
            &mut scene.document,
            carrier_auto.handle,
            "carrier_z_max",
            "3.8"
        ));

        let marker_ent = scene.document.get_entity(marker_auto_h).expect("marker");
        let marker_back = read_section_marker(&scene.document, marker_ent).expect("read marker");
        assert_eq!(marker_back.label, "Nord");
        assert_eq!(marker_back.far_depth, Some(11.0));
        assert!((marker_back.z_min - 0.2).abs() < 1e-9);
        assert!((marker_back.z_max - 3.8).abs() < 1e-9);

        let (vp_back, mv_back) =
            read_view_carrier_definitions(&scene.document, carrier_auto.handle).expect("back defs");
        assert_eq!(vp_back.name, "Nord");
        assert_eq!(mv_back.name, "Section Nord");
        assert_eq!(viewpoint_far_depth(&vp_back), Some(11.0));

        // Repeated "Update now" is a valid no-op that still succeeds.
        assert!(apply_carrier_property(
            &mut scene.document,
            carrier_auto.handle,
            "carrier_action",
            CHOICE_ACTION_UPDATE
        ));
        assert!(apply_carrier_property(
            &mut scene.document,
            carrier_auto.handle,
            "carrier_action",
            CHOICE_ACTION_UPDATE
        ));
    }

    #[test]
    fn test_apply_marker_and_carrier_reject_invalid() {
        use crate::modules::aec::engine::views::carrier::{
            create_view_carrier, find_view_carrier_by_handle, read_view_carrier_definitions,
            write_view_carrier_definitions,
        };
        use crate::modules::aec::engine::views::model_view::UpdatePolicy;
        use crate::modules::aec::engine::views::viewpoint::SectionPath;
        use crate::modules::aec::engine::views::{ModelViewId, Point2, ViewpointId};
        use crate::modules::aec::views::din1356::{
            create_section_marker, read_section_marker, Din1356SectionMarker,
        };
        use crate::scene::Scene;

        let mut scene = Scene::new();
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();
        let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let carrier = create_view_carrier(
            &mut scene.document,
            vp_id,
            mv_id,
            Point2::new(0.0, 0.0),
            1.0, // Placement scale multiplier stays 1.0
            0.0,
            UpdatePolicy::Manual,
        )
        .expect("carrier");
        let vp = sample_section_viewpoint(vp_id, "A - A", path.clone(), Some(5.0), 0.0, 3.0, false);
        let mv = sample_model_view(mv_id, vp_id, "MV", UpdatePolicy::Manual);
        write_view_carrier_definitions(&mut scene.document, carrier.handle, &vp, &mv);

        let marker = Din1356SectionMarker::new_section(
            "A - A",
            path,
            vp_id,
            Some(carrier.handle),
        )
        .with_depth(Some(5.0))
        .with_z_range(0.0, 3.0);
        let marker_h = create_section_marker(&mut scene.document, &marker);

        assert!(!apply_marker_property(
            &mut scene.document,
            marker_h,
            "marker_label",
            "   "
        ));
        assert!(!apply_marker_property(
            &mut scene.document,
            marker_h,
            "marker_z_min",
            "10.0"
        ));
        assert!(!apply_marker_property(
            &mut scene.document,
            marker_h,
            "marker_z_max",
            "-1.0"
        ));
        assert!(!apply_carrier_property(
            &mut scene.document,
            carrier.handle,
            "carrier_z_min",
            "9.0"
        ));
        assert!(!apply_carrier_property(
            &mut scene.document,
            carrier.handle,
            "carrier_name",
            ""
        ));
        assert!(!apply_carrier_property(
            &mut scene.document,
            carrier.handle,
            "carrier_scale",
            "0"
        ));
        assert!(!apply_carrier_property(
            &mut scene.document,
            carrier.handle,
            "carrier_scale",
            "-10.0"
        ));

        let marker_ent = scene.document.get_entity(marker_h).expect("marker");
        let unchanged = read_section_marker(&scene.document, marker_ent).expect("read");
        assert_eq!(unchanged.label, "A - A");
        assert_eq!(unchanged.far_depth, Some(5.0));
        assert!((unchanged.z_min - 0.0).abs() < 1e-9);
        assert!((unchanged.z_max - 3.0).abs() < 1e-9);

        let (vp_unchanged, mv_unchanged) =
            read_view_carrier_definitions(&scene.document, carrier.handle).expect("defs");
        let carrier_unchanged =
            find_view_carrier_by_handle(&scene.document, carrier.handle).expect("carrier");
        assert_eq!(carrier_unchanged.scale, 1.0);
        assert_eq!(mv_unchanged.scale.get(), 50.0);
        assert_eq!(vp_unchanged.name, "A - A");
        assert!((vp_unchanged.z_range.z_min.get() - 0.0).abs() < 1e-9);
        assert!((vp_unchanged.z_range.z_max.get() - 3.0).abs() < 1e-9);

        // 0 means unlimited depth on both sides.
        assert!(apply_marker_property(
            &mut scene.document,
            marker_h,
            "marker_far_depth",
            "0"
        ));
        let marker_ent = scene.document.get_entity(marker_h).expect("marker");
        let unlimited = read_section_marker(&scene.document, marker_ent).expect("read");
        assert_eq!(unlimited.far_depth, None);
        let (vp_unlimited, _) =
            read_view_carrier_definitions(&scene.document, carrier.handle).expect("defs");
        assert_eq!(viewpoint_far_depth(&vp_unlimited), None);

        assert!(apply_carrier_property(
            &mut scene.document,
            carrier.handle,
            "carrier_far_depth",
            "0"
        ));
        let marker_ent = scene.document.get_entity(marker_h).expect("marker");
        let still_unlimited = read_section_marker(&scene.document, marker_ent).expect("read");
        assert_eq!(still_unlimited.far_depth, None);
        let carrier_after =
            find_view_carrier_by_handle(&scene.document, carrier.handle).expect("carrier");
        assert!(carrier_after.is_outdated);
    }

    #[test]
    fn test_regeneration_failure_marks_carrier_outdated() {
        use crate::modules::aec::engine::views::carrier::{
            create_view_carrier, find_view_carrier_by_handle, write_view_carrier_definitions,
        };
        use crate::modules::aec::engine::views::model_view::UpdatePolicy;
        use crate::modules::aec::engine::views::update_ops::regenerate_or_mark_outdated;
        use crate::modules::aec::engine::views::viewpoint::SectionPath;
        use crate::modules::aec::engine::views::{ModelViewId, Point2, ViewpointId};
        use crate::scene::Scene;

        let mut scene = Scene::new();
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();
        let carrier = create_view_carrier(
            &mut scene.document,
            vp_id,
            mv_id,
            Point2::new(0.0, 0.0),
            1.0,
            0.0,
            UpdatePolicy::Automatic,
        )
        .expect("carrier");
        assert!(!carrier.is_outdated);

        // Use the matching viewpoint ID but invalid definitions so regeneration
        // fails validation rather than falling back to a valid embedded viewpoint.
        let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let vp_invalid = sample_section_viewpoint(vp_id, "", path, None, 0.0, 3.0, false);
        let mv = sample_model_view(mv_id, vp_id, "MV", UpdatePolicy::Automatic);
        write_view_carrier_definitions(&mut scene.document, carrier.handle, &vp_invalid, &mv);

        let success = regenerate_or_mark_outdated(&mut scene.document, carrier.handle, &vp_invalid, &mv);
        assert!(!success, "Regeneration must fail for an invalid viewpoint");

        let carrier_after_fail = find_view_carrier_by_handle(&scene.document, carrier.handle).expect("carrier");
        assert!(
            carrier_after_fail.is_outdated,
            "Regeneration failure must mark carrier as outdated instead of falsely claiming current"
        );
    }

    #[test]
    fn test_derived_primitive_prop_section_entity_ref_and_all_grips() {
        use crate::modules::aec::engine::views::carrier::{
            create_view_carrier, sync_carrier_primitives,
        };
        use crate::modules::aec::engine::views::model_view::UpdatePolicy;
        use crate::modules::aec::engine::views::provenance::{
            DepthInterval, DerivedPrimitive, ElementRef, FeatureKey, Geometry2d, PrimitiveRole,
            ResolvedStyle, SourceReference, SourceSet,
        };
        use crate::modules::aec::engine::views::{ModelId, ModelViewId, PaperMm, Point2, ViewpointId};
        use crate::scene::Scene;

        let mut scene = Scene::new();
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();
        let mut carrier = create_view_carrier(
            &mut scene.document,
            vp_id,
            mv_id,
            Point2::new(0.0, 0.0),
            1.0,
            0.0,
            UpdatePolicy::Automatic,
        )
        .expect("carrier");

        let source_handle = Handle::new(42);
        let elem_id = crate::modules::aec::engine::views::scene_collector::handle_to_element_id(source_handle);
        let elem_ref = ElementRef::new(ModelId::new(), elem_id);

        let test_cases = vec![
            (FeatureKey::WallBase, glam::DVec3::new(0.0, 1.0, 0.0)),
            (FeatureKey::SlabBottom, glam::DVec3::new(0.0, 1.0, 0.0)),
            (FeatureKey::OpeningLeftJamb, glam::DVec3::new(1.0, 0.0, 0.0)),
            (FeatureKey::OpeningRightJamb, glam::DVec3::new(1.0, 0.0, 0.0)),
        ];

        for (feat, expected_axis) in test_cases {
            let src = SourceReference {
                element: elem_ref,
                xref_source: None,
                component_id: None,
                feature: Some(feat.clone()),
                display_component_id: None,
            };

            let prim = DerivedPrimitive {
                id: crate::modules::aec::engine::views::provenance::PrimitiveId::new(mv_id, 1, 1),
                role: PrimitiveRole::CutBoundary,
                geometry: Geometry2d::Line {
                    start: Point2::new(0.0, 0.0),
                    end: Point2::new(4.0, 0.0),
                },
                sources: SourceSet::new(vec![src]).unwrap(),
                depth: DepthInterval::new(0.0, 0.0).unwrap(),
                style: ResolvedStyle::solid_stroke(PaperMm::new(0.35).unwrap()),
                edit_handles: vec![],
            };

            sync_carrier_primitives(&mut scene.document, &mut carrier, &[prim]).expect("sync");
            assert_eq!(carrier.derived_handles.len(), 1);
            let child_handle = carrier.derived_handles[0];
            let child_ent = scene.document.get_entity(child_handle).expect("child");

            // 1. Verify Properties Section with EntityRef
            let section = derived_primitive_prop_section(child_ent).expect("prop section");
            assert_eq!(section.title, "AEC Assoziiertes Bauteil");
            let source_prop = section.props.iter().find(|p| p.field == "prim_source_entity").expect("source entity prop");
            assert_eq!(source_prop.label, "Quell-Bauteil");
            match &source_prop.value {
                crate::scene::model::object::PropValue::EntityRef { display, handle } => {
                    assert_eq!(*handle, source_handle);
                    assert_eq!(display, &format!("Bauteil #{}", source_handle));
                }
                _ => panic!("Expected PropValue::EntityRef"),
            }

            let carrier_prop = section.props.iter().find(|p| p.field == "prim_carrier_entity").expect("carrier entity prop");
            match &carrier_prop.value {
                crate::scene::model::object::PropValue::EntityRef { handle, .. } => {
                    assert_eq!(*handle, carrier.handle);
                }
                _ => panic!("Expected PropValue::EntityRef"),
            }

            // 2. Verify Viewport Grips & Constraints
            let mut grips = Vec::new();
            append_derived_view_grips(&scene, child_handle, &mut grips);
            let expected_len = if feat == FeatureKey::WallBase { 1 } else { 2 };
            assert_eq!(grips.len(), expected_len, "Expected {} grips for {:?}", expected_len, feat);
            let grip = &grips[0];
            assert_eq!(grip.id, DERIVED_VIEW_GRIP_BASE);
            assert_eq!(grip.world, glam::DVec3::new(2.0, 0.0, 0.0));
            assert_eq!(grip.axis, Some(expected_axis), "Axis mismatch for {:?}", feat);
            if feat != FeatureKey::WallBase {
                assert_eq!(grips[1].shape, crate::scene::model::object::GripShape::Dropdown);
            }
        }
    }

    #[test]
    fn test_carrier_display_config_and_marker_navigation() {
        use crate::modules::aec::engine::views::carrier::{
            create_view_carrier, read_view_carrier_definitions, write_view_carrier_definitions,
        };
        use crate::modules::aec::engine::views::model_view::UpdatePolicy;
        use crate::modules::aec::engine::views::viewpoint::SectionPath;
        use crate::modules::aec::engine::views::{ModelViewId, Point2, ViewpointId};
        use crate::modules::aec::views::din1356::{create_section_marker, Din1356SectionMarker};
        use crate::scene::Scene;

        let mut scene = Scene::new();
        add_test_wall(&mut scene.document);

        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();
        let carrier = create_view_carrier(
            &mut scene.document,
            vp_id,
            mv_id,
            Point2::new(10.0, 0.0),
            1.0,
            0.0,
            UpdatePolicy::Automatic,
        )
        .expect("carrier");

        let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let vp = sample_section_viewpoint(vp_id, "Schnitt 1", path.clone(), Some(5.0), 0.0, 3.0, false);
        let mv = sample_model_view(mv_id, vp_id, "Section 1", UpdatePolicy::Automatic);
        let original_scale = mv.scale;
        write_view_carrier_definitions(&mut scene.document, carrier.handle, &vp, &mv);

        let marker = Din1356SectionMarker::new_section("Schnitt 1", path, vp_id, Some(carrier.handle));
        let marker_h = create_section_marker(&mut scene.document, &marker);

        crate::modules::aec::engine::owner_index::link_peers(&mut scene.document, marker_h, carrier.handle);

        // 1. Verify carrier property section has display config choice & marker entity ref
        let carrier_ent = scene.document.get_entity(carrier.handle).expect("carrier entity");
        let dc_lib = crate::modules::aec::engine::project::resolve_display_config_library(None);
        let carrier_section = carrier_prop_section_with_library(
            carrier_ent,
            &scene.document,
            &dc_lib,
        )
        .expect("carrier prop section");

        let dc_prop = carrier_section.props.iter().find(|p| p.field == "carrier_display_config").expect("dc prop");
        match &dc_prop.value {
            crate::scene::model::object::PropValue::Choice { selected, options } => {
                assert_eq!(selected, "—", "an unbound profile must not display the first section plan");
                assert!(options.iter().any(|option| option == "—"));
                assert!(!options.is_empty());
            }
            _ => panic!("Expected Choice for display config"),
        }

        let marker_ref_prop = carrier_section.props.iter().find(|p| p.field == "carrier_marker_entity").expect("marker entity prop");
        match &marker_ref_prop.value {
            crate::scene::model::object::PropValue::EntityRef { handle, .. } => {
                assert_eq!(*handle, marker_h);
            }
            _ => panic!("Expected EntityRef for marker on carrier"),
        }

        // 2. Verify marker property section has carrier entity ref
        let marker_ent = scene.document.get_entity(marker_h).expect("marker entity");
        let marker_section = marker_prop_section(marker_ent, &scene.document).expect("marker prop section");
        let carrier_ref_prop = marker_section.props.iter().find(|p| p.field == "marker_carrier").expect("carrier ref prop");
        match &carrier_ref_prop.value {
            crate::scene::model::object::PropValue::EntityRef { handle, .. } => {
                assert_eq!(*handle, carrier.handle);
            }
            _ => panic!("Expected EntityRef for carrier on marker"),
        }

        // 3. Test changing display config via apply_carrier_property
        let first_cfg = &dc_lib.configs[0];
        let style_lib = crate::modules::aec::engine::project::resolve_style_library(None);
        assert!(apply_carrier_property_with_libraries(
            &mut scene.document,
            carrier.handle,
            "carrier_display_config",
            &first_cfg.name,
            &dc_lib,
            &style_lib,
            None,
        ));

        let (_, mv_after) = read_view_carrier_definitions(&scene.document, carrier.handle).expect("defs after");
        assert_eq!(mv_after.profile_id.as_uuid(), first_cfg.id);
        assert_eq!(mv_after.scale, original_scale);
    }

    #[test]
    fn test_opening_jamb_grip_axis_aligns_with_floorplan_wall() {
        use crate::modules::aec::engine::openings::Opening;
        use crate::modules::aec::engine::opening_xdata::write_opening_instance;
        use crate::modules::aec::engine::views::carrier::{
            create_view_carrier, sync_carrier_primitives, write_view_carrier_definitions,
        };
        use crate::modules::aec::engine::views::model_view::UpdatePolicy;
        use crate::modules::aec::engine::views::provenance::{
            DepthInterval, DerivedPrimitive, ElementRef, FeatureKey, Geometry2d, PrimitiveRole,
            ResolvedStyle, SourceReference, SourceSet,
        };
        use crate::modules::aec::engine::views::viewpoint::{PlanRange, ViewAxes, ViewKind, ViewOrigin};
        use crate::modules::aec::engine::views::{
            Metres, ModelId, ModelViewId, NonNegativeMetres, PaperMm, Point2, Point3, Revision,
            ScaleDenominator, VerticalRange, ViewProfileId, ViewRevisions, ViewpointId,
        };
        use crate::scene::Scene;

        let mut scene = Scene::new();

        // 1. Create a vertical wall along Y using add_test_wall
        let wall_h = add_test_wall(&mut scene.document);

        // 2. Create an opening on this wall
        let op_h = scene.document.allocate_handle();
        let mut op_ent = codec::EntityType::Point(codec::Point::default());
        op_ent.common_mut().handle = op_h;
        scene.document.add_entity(op_ent).expect("add op entity");
        let opening = Opening::door(op_h, wall_h, 1.0);
        write_opening_instance(&mut scene, &opening);

        // 3. Create a FloorPlan carrier
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();
        let mut carrier = create_view_carrier(
            &mut scene.document,
            vp_id,
            mv_id,
            Point2::new(0.0, 0.0),
            1.0,
            0.0,
            UpdatePolicy::Automatic,
        )
        .expect("carrier");

        let vp = crate::modules::aec::engine::views::viewpoint::Viewpoint {
            id: vp_id,
            name: "Floor Plan".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 1.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.0).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::new(0.0).unwrap(),
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };
        let mv = crate::modules::aec::engine::views::model_view::ModelView {
            id: mv_id,
            name: "Floor Plan MV".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: crate::modules::aec::engine::views::model_view::DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };
        write_view_carrier_definitions(&mut scene.document, carrier.handle, &vp, &mv);

        let elem_id = crate::modules::aec::engine::views::scene_collector::handle_to_element_id(op_h);
        let elem_ref = ElementRef::new(ModelId::new(), elem_id);

        let src = SourceReference {
            element: elem_ref,
            xref_source: None,
            component_id: None,
            feature: Some(FeatureKey::OpeningLeftJamb),
            display_component_id: None,
        };

        let prim = DerivedPrimitive {
            id: crate::modules::aec::engine::views::provenance::PrimitiveId::new(mv_id, 1, 1),
            role: PrimitiveRole::CutBoundary,
            geometry: Geometry2d::Line {
                start: Point2::new(0.0, 3.0),
                end: Point2::new(0.24, 3.0),
            },
            sources: SourceSet::new(vec![src]).unwrap(),
            depth: DepthInterval::new(0.0, 0.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.35).unwrap()),
            edit_handles: vec![],
        };

        sync_carrier_primitives(&mut scene.document, &mut carrier, &[prim]).expect("sync");
        let child_handle = carrier.derived_handles[0];

        // 4. Verify grip axis is dynamically aligned with host wall axis along Y: (0, 1, 0)
        let mut grips = Vec::new();
        append_derived_view_grips(&scene, child_handle, &mut grips);
        assert_eq!(grips.len(), 2);
        let axis = grips[0].axis.expect("axis must be present");
        assert!((axis.x - 0.0).abs() < 1e-4, "axis.x was {}", axis.x);
        assert!((axis.y - 1.0).abs() < 1e-4, "axis.y was {}", axis.y);
        assert_eq!(grips[1].shape, crate::scene::model::object::GripShape::Dropdown);
    }

    #[test]
    fn test_model_elements_dropdown_grips() {
        use crate::modules::aec::engine::openings::Opening;
        use crate::modules::aec::engine::opening_xdata::write_opening_instance;
        use crate::modules::aec::engine::slab::Slab;
        use crate::modules::aec::engine::slab_xdata::write_slab_record;
        use crate::scene::Scene;

        let mut scene = Scene::new();

        // 1. Wall Dropdown Grip (removed from model walls to preserve standard context menu)
        let wall_h = add_test_wall(&mut scene.document);
        let mut wall_grips = Vec::new();
        let wall_ent = scene.document.get_entity(wall_h).unwrap().clone();
        append_wall_junction_grips(&scene, wall_h, &wall_ent, &mut wall_grips);
        assert!(
            !wall_grips.iter().any(|g| g.id == WALL_BASE_DROPDOWN_GRIP_ID),
            "Wall must not have a WallBase dropdown grip in model view"
        );

        // 2. Opening Dropdown Grip
        let op_h = scene.document.allocate_handle();
        let mut op_ent = codec::EntityType::Point(codec::Point::default());
        op_ent.common_mut().handle = op_h;
        scene.document.add_entity(op_ent).expect("add op entity");
        let opening = Opening::door(op_h, wall_h, 1.0);
        write_opening_instance(&mut scene, &opening);

        let mut op_grips = Vec::new();
        append_opening_axis_grips(&scene, op_h, &mut op_grips);
        assert!(
            op_grips.iter().any(|g| g.id == OPENING_DROPDOWN_GRIP_ID && g.shape == crate::scene::model::object::GripShape::Dropdown),
            "Opening must have an Opening dropdown grip"
        );

        // 3. Slab Dropdown Grip
        let slab_h = scene.document.allocate_handle();
        let mut slab_pl = acadrust::entities::LwPolyline::default();
        slab_pl.common.handle = slab_h;
        slab_pl.vertices.push(acadrust::entities::LwVertex {
            location: acadrust::types::Vector2::new(0.0, 0.0),
            bulge: 0.0,
            start_width: 0.0,
            end_width: 0.0,
            vertex_id: 0,
        });
        slab_pl.vertices.push(acadrust::entities::LwVertex {
            location: acadrust::types::Vector2::new(5.0, 0.0),
            bulge: 0.0,
            start_width: 0.0,
            end_width: 0.0,
            vertex_id: 1,
        });
        slab_pl.vertices.push(acadrust::entities::LwVertex {
            location: acadrust::types::Vector2::new(5.0, 5.0),
            bulge: 0.0,
            start_width: 0.0,
            end_width: 0.0,
            vertex_id: 2,
        });
        let slab_ent = EntityType::LwPolyline(slab_pl);
        scene.document.add_entity(slab_ent).expect("add slab entity");
        let slab = Slab::new("default", 0);
        write_slab_record(&mut scene.document, slab_h, &slab);

        let slab_ent_updated = scene.document.get_entity(slab_h).unwrap().clone();
        let mut slab_grips = Vec::new();
        append_slab_base_grips(&scene, slab_h, &slab_ent_updated, &mut slab_grips);
        assert!(
            slab_grips.iter().any(|g| g.id == SLAB_BOTTOM_DROPDOWN_GRIP_ID && g.shape == crate::scene::model::object::GripShape::Dropdown),
            "Slab must have a SlabBottom dropdown grip"
        );
    }

    #[test]
    fn test_eval_aec_property_expression() {
        let mut scene = crate::scene::Scene::new();
        scene.named_parameters_mut().set("_STORY_H", "3.00").unwrap();
        scene.named_parameters_mut().set("_BRH", "0.90").unwrap();

        // 1. Plain numeric literals
        assert_eq!(eval_aec_property_expression(&scene, "2.75"), Ok(2.75));
        assert_eq!(eval_aec_property_expression(&scene, " 3.20 "), Ok(3.20));

        // 2. Pure arithmetic expressions
        assert_eq!(eval_aec_property_expression(&scene, "2.80 - 0.25"), Ok(2.55));
        assert_eq!(eval_aec_property_expression(&scene, "1.0 + 0.15 * 2"), Ok(1.30));
        assert_eq!(eval_aec_property_expression(&scene, "(2.0 + 0.5) * 2"), Ok(5.0));

        // 3. Named parameters with case-insensitivity
        assert_eq!(eval_aec_property_expression(&scene, "_STORY_H - 0.25"), Ok(2.75));
        assert_eq!(eval_aec_property_expression(&scene, "_story_h - 0.25"), Ok(2.75));
        assert_eq!(eval_aec_property_expression(&scene, "_BRH + 0.125"), Ok(1.025));
        assert_eq!(eval_aec_property_expression(&scene, "_brh + 0.125"), Ok(1.025));

        // 4. Mathematical functions and constants
        assert_eq!(eval_aec_property_expression(&scene, "round(2.75)"), Ok(3.0));
        assert_eq!(eval_aec_property_expression(&scene, "sqrt(9)"), Ok(3.0));
        assert!((eval_aec_property_expression(&scene, "pi").unwrap() - std::f64::consts::PI).abs() < 1e-12);

        // 5. Invalid expressions
        assert!(eval_aec_property_expression(&scene, "").is_err());
        assert!(eval_aec_property_expression(&scene, "_UNKNOWN_VAR + 1").is_err());
        assert!(eval_aec_property_expression(&scene, "1 / 0").is_err());
    }

    #[test]
    fn test_associative_parameter_update_recomputes_aec_geometry() {
        use crate::app::OpenCADStudio;
        use crate::modules::aec::engine::wall::Wall;
        use crate::modules::aec::engine::xdata::{wall_from_entity, wall_record_for_wall, write_aec_record, AEC_APPID};
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2;
        use acadrust::xdata::ExtendedDataRecord;

        let mut app = OpenCADStudio::new_for_test();
        let tab = app.active_tab;

        // 1. Set initial parameters
        app.tabs[tab].scene.named_parameters_mut().set("_STORY_H", "3.00").unwrap();
        app.tabs[tab].scene.named_parameters_mut().set("_BRH", "0.90").unwrap();

        // 2. Create a wall with a formula for height
        let mut wall = Wall::new("style_standard_24", 2.75, 1);
        wall.formulas.set("wall_height", "_STORY_H - 0.25".to_string());

        let mut poly = LwPolyline::new();
        poly.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(6.0, 0.0)));
        let wall_handle = app.tabs[tab].scene.add_entity(EntityType::LwPolyline(poly));
        let mut rec = ExtendedDataRecord::new(AEC_APPID);
        rec.values = wall_record_for_wall(&wall);
        write_aec_record(&mut app.tabs[tab].scene.document, wall_handle, rec);

        // 3. Mutate parameter _STORY_H to 3.40 and trigger recompute
        app.tabs[tab].scene.named_parameters_mut().set("_STORY_H", "3.40").unwrap();
        app.aec_recompute_all_formulas(tab);

        // 4. Verify wall height automatically updated to 3.15 (3.40 - 0.25)
        let ent = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
        let updated_wall = wall_from_entity(ent).expect("wall");
        assert!((updated_wall.height - 3.15).abs() < 1e-6);
        assert_eq!(updated_wall.formulas.get("wall_height"), Some("_STORY_H - 0.25"));
    }

    #[test]
    fn test_property_commit_stores_formula_and_formats_label() {
        use crate::app::OpenCADStudio;
        use crate::modules::aec::engine::wall::Wall;
        use crate::modules::aec::engine::xdata::{wall_from_entity, wall_record_for_wall, write_aec_record, AEC_APPID};
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2;
        use acadrust::xdata::ExtendedDataRecord;

        let mut app = OpenCADStudio::new_for_test();
        let tab = app.active_tab;

        app.tabs[tab].scene.named_parameters_mut().set("_STORY_H", "3.00").unwrap();

        // Create a wall with plain height 2.80
        let wall = Wall::new("style_standard_24", 2.80, 1);
        let mut poly = LwPolyline::new();
        poly.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let wall_handle = app.tabs[tab].scene.add_entity(EntityType::LwPolyline(poly));
        let mut rec = ExtendedDataRecord::new(AEC_APPID);
        rec.values = wall_record_for_wall(&wall);
        write_aec_record(&mut app.tabs[tab].scene.document, wall_handle, rec);

        // Select the wall
        app.tabs[tab].scene.select_entity(wall_handle, true);
        app.refresh_properties();

        // 1. Initially, no formula attached -> label is "Height"
        let ent = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
        let sec = wall_prop_section(ent, None, None).expect("wall section");
        let height_prop = sec.props.iter().find(|p| p.field == "wall_height").unwrap();
        assert_eq!(height_prop.label, "Height");

        // 2. Type formula "_STORY_H - 0.25" and commit
        let _ = app.update(crate::app::Message::PropGeomInput {
            field: "wall_height",
            value: "_STORY_H - 0.25".to_string(),
        });
        let _ = app.update(crate::app::Message::PropGeomCommit("wall_height"));

        // 3. Verify wall entity now has calculated height 2.75 and stored formula
        let ent = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
        let updated_wall = wall_from_entity(ent).expect("wall");
        assert_eq!(updated_wall.height, 2.75);
        assert_eq!(updated_wall.formulas.get("wall_height"), Some("_STORY_H - 0.25"));

        // 4. Verify property section formats label with (fx = 2.7500) and value contains formula string
        crate::entities::common::set_unit_context(
            crate::entities::common::UnitContext::from_header(
                &app.tabs[tab].scene.document.header,
            ),
        );
        let sec = wall_prop_section(ent, None, None).expect("wall section");
        let height_prop = sec.props.iter().find(|p| p.field == "wall_height").unwrap();
        assert!(height_prop.label.contains("fx = 2.75"));
        assert_eq!(height_prop.value, crate::scene::model::object::PropValue::EditText("_STORY_H - 0.25".to_string()));

        // 5. In-place edit of the formula string (e.g. adjust offset to -0.10)
        let _ = app.update(crate::app::Message::PropGeomInput {
            field: "wall_height",
            value: "_STORY_H - 0.10".to_string(),
        });
        let _ = app.update(crate::app::Message::PropGeomCommit("wall_height"));

        let ent = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
        let updated_wall = wall_from_entity(ent).expect("wall");
        assert_eq!(updated_wall.height, 2.90);
        assert_eq!(updated_wall.formulas.get("wall_height"), Some("_STORY_H - 0.10"));

        crate::entities::common::set_unit_context(
            crate::entities::common::UnitContext::from_header(
                &app.tabs[tab].scene.document.header,
            ),
        );
        let sec = wall_prop_section(ent, None, None).expect("wall section");
        let height_prop = sec.props.iter().find(|p| p.field == "wall_height").unwrap();
        assert!(height_prop.label.contains("fx = 2.9"));
        assert_eq!(height_prop.value, crate::scene::model::object::PropValue::EditText("_STORY_H - 0.10".to_string()));

        // 6. Overwrite with plain number "3.10"
        let _ = app.update(crate::app::Message::PropGeomInput {
            field: "wall_height",
            value: "3.10".to_string(),
        });
        let _ = app.update(crate::app::Message::PropGeomCommit("wall_height"));

        let ent = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
        let updated_wall = wall_from_entity(ent).expect("wall");
        assert_eq!(updated_wall.height, 3.10);
        assert_eq!(updated_wall.formulas.get("wall_height"), None);

        let sec = wall_prop_section(ent, None, None).expect("wall section");
        let height_prop = sec.props.iter().find(|p| p.field == "wall_height").unwrap();
        assert_eq!(height_prop.label, "Height");
        assert_eq!(height_prop.value, crate::scene::model::object::PropValue::EditText("3.1".to_string()));
    }

    #[test]
    fn test_formula_recompute_wall_top_offset_with_dangling_plane() {
        use crate::app::OpenCADStudio;
        use crate::modules::aec::engine::control_plane::ControlPlaneFacet;
        use crate::modules::aec::engine::wall::Wall;
        use crate::modules::aec::engine::xdata::{wall_from_entity, wall_record_for_wall, write_aec_record, AEC_APPID};
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2;
        use acadrust::xdata::ExtendedDataRecord;
        use uuid::Uuid;

        let mut app = OpenCADStudio::new_for_test();
        let tab = app.active_tab;

        app.tabs[tab].scene.named_parameters_mut().set("_ROOF_OFF", "0.20").unwrap();

        let mut wall = Wall::new("style_standard_24", 2.80, 1);
        let f1 = ControlPlaneFacet::new(
            "RoofSlope",
            vec![
                [0.0, -5.0, 4.0],
                [10.0, -5.0, 6.0],
                [10.0, 5.0, 6.0],
                [0.0, 5.0, 4.0],
            ],
        );
        wall.top_plane_id = Some(Uuid::new_v4()); // Dangling plane ID
        wall.top_facets = vec![f1];
        wall.top_origin = [0.0, -5.0, 4.0];
        wall.formulas.set("wall_top_offset", "_ROOF_OFF + 0.05".to_string());

        let mut poly = LwPolyline::new();
        poly.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        let wall_handle = app.tabs[tab].scene.add_entity(EntityType::LwPolyline(poly));
        let mut rec = ExtendedDataRecord::new(AEC_APPID);
        rec.values = wall_record_for_wall(&wall);
        write_aec_record(&mut app.tabs[tab].scene.document, wall_handle, rec);

        // 1. Initial recompute
        app.aec_recompute_all_formulas(tab);

        let ent = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
        let updated_wall = wall_from_entity(ent).expect("wall");
        assert!((updated_wall.top_offset - 0.25).abs() < 1e-6);
        let normal_scale = 1.04_f64.sqrt();
        let z0 = updated_wall.top_z_at_xy(0.0, 0.0);
        let z5 = updated_wall.top_z_at_xy(5.0, 0.0);
        let z10 = updated_wall.top_z_at_xy(10.0, 0.0);
        assert!((z5 - z0 - 1.0).abs() < 1e-6);
        assert!((z10 - z0 - 2.0).abs() < 1e-6);
        assert!((z0 - (4.0 + 0.25 * normal_scale)).abs() < 1e-6);
        assert!((z5 - (5.0 + 0.25 * normal_scale)).abs() < 1e-6);
        assert!((z10 - (6.0 + 0.25 * normal_scale)).abs() < 1e-6);

        // 2. Mutate parameter _ROOF_OFF and recompute
        app.tabs[tab].scene.named_parameters_mut().set("_ROOF_OFF", "0.50").unwrap();
        app.aec_recompute_all_formulas(tab);

        let ent = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
        let updated_wall2 = wall_from_entity(ent).expect("wall");
        assert!((updated_wall2.top_offset - 0.55).abs() < 1e-6);
        let z0_2 = updated_wall2.top_z_at_xy(0.0, 0.0);
        let z5_2 = updated_wall2.top_z_at_xy(5.0, 0.0);
        let z10_2 = updated_wall2.top_z_at_xy(10.0, 0.0);
        assert!((z5_2 - z0_2 - 1.0).abs() < 1e-6);
        assert!((z10_2 - z0_2 - 2.0).abs() < 1e-6);
        assert!((z0_2 - (4.0 + 0.55 * normal_scale)).abs() < 1e-6);
        assert!((z5_2 - (5.0 + 0.55 * normal_scale)).abs() < 1e-6);
        assert!((z10_2 - (6.0 + 0.55 * normal_scale)).abs() < 1e-6);
        assert!((z0_2 - z0 - 0.30 * normal_scale).abs() < 1e-6);
        assert!((z5_2 - z5 - 0.30 * normal_scale).abs() < 1e-6);
        assert!((z10_2 - z10 - 0.30 * normal_scale).abs() < 1e-6);
    }

    #[test]
    fn test_formula_recompute_slab_offsets_with_aliases() {
        use crate::app::OpenCADStudio;
        use crate::modules::aec::engine::slab::Slab;
        use crate::modules::aec::engine::slab_xdata::{slab_from_entity, write_slab_record};
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2;

        let mut app = OpenCADStudio::new_for_test();
        let tab = app.active_tab;

        app.tabs[tab].scene.named_parameters_mut().set("_SLAB_OFF", "0.15").unwrap();
        app.tabs[tab].scene.named_parameters_mut().set("_BASE_OFF", "-0.05").unwrap();

        // 1. Slab with slab_top_offset formula
        let mut poly1 = LwPolyline::new();
        poly1.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        poly1.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let s1_handle = app.tabs[tab].scene.add_entity(EntityType::LwPolyline(poly1));
        let mut s1 = Slab::new("style_slab_concrete_20", 1);
        s1.formulas.set("slab_top_offset", "_SLAB_OFF + 0.10".to_string());
        write_slab_record(&mut app.tabs[tab].scene.document, s1_handle, &s1);

        // 2. Slab with slab_offset formula alias
        let mut poly2 = LwPolyline::new();
        poly2.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        poly2.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let s2_handle = app.tabs[tab].scene.add_entity(EntityType::LwPolyline(poly2));
        let mut s2 = Slab::new("style_slab_concrete_20", 1);
        s2.formulas.set("slab_offset", "_SLAB_OFF + 0.05".to_string());
        write_slab_record(&mut app.tabs[tab].scene.document, s2_handle, &s2);

        // 3. Slab with slab_base_offset formula
        let mut poly3 = LwPolyline::new();
        poly3.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        poly3.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let s3_handle = app.tabs[tab].scene.add_entity(EntityType::LwPolyline(poly3));
        let mut s3 = Slab::new("style_slab_concrete_20", 1);
        s3.formulas.set("slab_base_offset", "_BASE_OFF - 0.02".to_string());
        write_slab_record(&mut app.tabs[tab].scene.document, s3_handle, &s3);

        // Initial recompute
        app.aec_recompute_all_formulas(tab);

        let ent1 = app.tabs[tab].scene.document.get_entity(s1_handle).unwrap();
        let slab1 = slab_from_entity(ent1).unwrap();
        assert!((slab1.top_offset - 0.25).abs() < 1e-9);

        let ent2 = app.tabs[tab].scene.document.get_entity(s2_handle).unwrap();
        let slab2 = slab_from_entity(ent2).unwrap();
        assert!((slab2.top_offset - 0.20).abs() < 1e-9);

        let ent3 = app.tabs[tab].scene.document.get_entity(s3_handle).unwrap();
        let slab3 = slab_from_entity(ent3).unwrap();
        assert!((slab3.base_offset - (-0.07)).abs() < 1e-9);

        // Verify property section displays (fx = ...) for alias formulas as well
        let sec2 = slab_prop_section(ent2, None, None).expect("slab prop section");
        let top_off_prop = sec2.props.iter().find(|p| p.field == "slab_top_offset").unwrap();
        assert!(top_off_prop.label.contains("fx ="));
        match &top_off_prop.value {
            crate::scene::model::object::PropValue::EditText(txt) => {
                assert_eq!(txt, "_SLAB_OFF + 0.05");
            }
            _ => panic!("expected EditText"),
        }

        // Mutate parameters and recompute
        app.tabs[tab].scene.named_parameters_mut().set("_SLAB_OFF", "0.30").unwrap();
        app.tabs[tab].scene.named_parameters_mut().set("_BASE_OFF", "0.10").unwrap();
        app.aec_recompute_all_formulas(tab);

        let ent1 = app.tabs[tab].scene.document.get_entity(s1_handle).unwrap();
        let slab1 = slab_from_entity(ent1).unwrap();
        assert!((slab1.top_offset - 0.40).abs() < 1e-9);

        let ent2 = app.tabs[tab].scene.document.get_entity(s2_handle).unwrap();
        let slab2 = slab_from_entity(ent2).unwrap();
        assert!((slab2.top_offset - 0.35).abs() < 1e-9);

        let ent3 = app.tabs[tab].scene.document.get_entity(s3_handle).unwrap();
        let slab3 = slab_from_entity(ent3).unwrap();
        assert!((slab3.base_offset - 0.08).abs() < 1e-9);

        // Verify property section structure has slab_top_plane & slab_top_offset right after justification
        let sec = slab_prop_section(ent1, None, None).expect("slab prop section");
        let fields: Vec<&str> = sec.props.iter().map(|p| p.field).collect();
        let just_idx = fields.iter().position(|&f| f == "slab_justification").unwrap();
        let top_plane_idx = fields.iter().position(|&f| f == "slab_top_plane").unwrap();
        let top_offset_idx = fields.iter().position(|&f| f == "slab_top_offset").unwrap();
        assert_eq!(top_plane_idx, just_idx + 1);
        assert_eq!(top_offset_idx, just_idx + 2);

        // Apply a plain numeric value to s2 (which had alias formula `slab_offset`):
        // All aliases must be cleared, and recompute must not resurrect the formula.
        let ok = app.aec_apply_property_field(tab, s2_handle, "slab_top_offset", "0.80");
        assert!(ok);
        let ent2_after_num = app.tabs[tab].scene.document.get_entity(s2_handle).unwrap();
        let s2_after_num = slab_from_entity(ent2_after_num).unwrap();
        assert!((s2_after_num.top_offset - 0.80).abs() < 1e-9);
        assert!(s2_after_num.formulas.is_empty());

        // Recompute formulas should NOT overwrite the plain numeric value 0.80
        app.aec_recompute_all_formulas(tab);
        let ent2_after_recompute = app.tabs[tab].scene.document.get_entity(s2_handle).unwrap();
        let s2_after_recompute = slab_from_entity(ent2_after_recompute).unwrap();
        assert!((s2_after_recompute.top_offset - 0.80).abs() < 1e-9);
        assert!(s2_after_recompute.formulas.is_empty());
    }

    #[test]
    fn test_aec_apply_property_field_for_slab_offsets_and_justifications() {
        use crate::app::OpenCADStudio;
        use crate::modules::aec::engine::slab::{Slab, SlabJustification};
        use crate::modules::aec::engine::slab_xdata::{slab_from_entity, write_slab_record};
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2;

        let mut app = OpenCADStudio::new_for_test();
        let tab = app.active_tab;

        app.tabs[tab].scene.named_parameters_mut().set("_H", "2.80").unwrap();

        let mut poly = LwPolyline::new();
        poly.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(5.0, 4.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(0.0, 4.0)));
        poly.is_closed = true;
        let handle = app.tabs[tab].scene.add_entity(EntityType::LwPolyline(poly));
        let slab = Slab::new("style_slab_concrete_20", 1);
        write_slab_record(&mut app.tabs[tab].scene.document, handle, &slab);

        // 1. Change justification via panel apply
        let ok = app.aec_apply_property_field(tab, handle, "slab_justification", "OKRD (Oberkante Rohdecke)");
        assert!(ok);
        let ent = app.tabs[tab].scene.document.get_entity(handle).unwrap();
        let s = slab_from_entity(ent).unwrap();
        assert_eq!(s.justification, SlabJustification::StructuralTop);

        // 2. Set formula on slab_top_offset via panel apply
        let ok = app.aec_apply_property_field(tab, handle, "slab_top_offset", "_H * 0.10");
        assert!(ok);
        let ent = app.tabs[tab].scene.document.get_entity(handle).unwrap();
        let s = slab_from_entity(ent).unwrap();
        assert!((s.top_offset - 0.28).abs() < 1e-9);
        assert_eq!(s.formulas.get("slab_top_offset"), Some("_H * 0.10"));

        // 3. Mutate parameter and recompute
        app.tabs[tab].scene.named_parameters_mut().set("_H", "3.00").unwrap();
        app.aec_recompute_all_formulas(tab);
        let ent = app.tabs[tab].scene.document.get_entity(handle).unwrap();
        let s = slab_from_entity(ent).unwrap();
        assert!((s.top_offset - 0.30).abs() < 1e-9);

        // 4. Overwrite formula with plain number
        let ok = app.aec_apply_property_field(tab, handle, "slab_top_offset", "0.45");
        assert!(ok);
        let ent = app.tabs[tab].scene.document.get_entity(handle).unwrap();
        let s = slab_from_entity(ent).unwrap();
        assert!((s.top_offset - 0.45).abs() < 1e-9);
        assert_eq!(s.formulas.get("slab_top_offset"), None);
        assert!(s.formulas.is_empty());
    }

    #[test]
    fn test_prop_geom_choice_changed_applies_aec_control_planes_and_justifications() {
        use crate::app::{Message, OpenCADStudio};
        use crate::modules::aec::engine::control_plane::ControlPlane;
        use crate::modules::aec::engine::project::{Building, ProjectFile, StoreyRef};
        use crate::modules::aec::engine::wall::Wall;
        use crate::modules::aec::engine::xdata::{wall_from_entity, wall_record_for_wall, AEC_APPID};
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2;
        use acadrust::xdata::ExtendedDataRecord;

        let mut app = OpenCADStudio::new_for_test();
        let tab = app.active_tab;

        let mut project = ProjectFile::default();
        let mut building = Building::new("Haus");
        let mut eg = StoreyRef::new_with_height("EG", 0.0, 2.80, "eg.dwg");
        let eg_elev_id = eg.floor_plane_id;
        let eg_custom = ControlPlane::horizontal("Rohdecke", 0.15);
        let eg_custom_id = eg_custom.id;
        eg.control_planes.push(eg_custom);

        let og = StoreyRef::new_with_height("OG", 2.80, 2.80, "og.dwg");
        let og_elev_id = og.floor_plane_id;

        building.storeys.push(eg);
        building.storeys.push(og);
        project.buildings.push(building);

        app.aec.aec_project_explorer_file = Some(project.clone());

        // Create a wall
        let mut poly = LwPolyline::new();
        poly.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        poly.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let wall_handle = app.tabs[tab].scene.add_entity(EntityType::LwPolyline(poly));
        let wall = Wall::new("standard_wall", 2.50, 0);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record_for_wall(&wall);
        app.tabs[tab]
            .scene
            .document
            .get_entity_mut(wall_handle)
            .unwrap()
            .common_mut()
            .extended_data
            .add_record(record);

        // Select the wall for properties panel
        app.tabs[tab].scene.select_entity(wall_handle, true);
        app.refresh_properties();

        // 1. Initial property section shows (none) for base plane
        let ent = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
        let sec = wall_prop_section(ent, None, Some(&project)).unwrap();
        let base_prop = sec.props.iter().find(|p| p.field == "wall_base_plane").unwrap();
        match &base_prop.value {
            crate::scene::model::object::PropValue::Choice { selected, .. } => {
                assert_eq!(selected, "(none)");
            }
            _ => panic!("expected Choice"),
        }

        // 2. Select "EG_ELEVATION (Z = 0.000 m)" via PropGeomChoiceChanged
        let _ = app.update(Message::PropGeomChoiceChanged {
            field: "wall_base_plane",
            value: "EG_ELEVATION (Z = 0.000 m)".into(),
        });

        // Verify wall has base_plane_id set
        let ent_after = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
        let wall_after = wall_from_entity(ent_after).unwrap();
        assert_eq!(wall_after.base_plane_id, Some(eg_elev_id));
        assert_eq!(wall_after.base_plane_name.as_deref(), Some("EG_ELEVATION"));

        // Verify property panel now reflects the chosen base plane
        let sec_after = wall_prop_section(ent_after, None, Some(&project)).unwrap();
        let base_prop_after = sec_after.props.iter().find(|p| p.field == "wall_base_plane").unwrap();
        match &base_prop_after.value {
            crate::scene::model::object::PropValue::Choice { selected, .. } => {
                assert_eq!(selected, "EG_ELEVATION (Z = 0.000 m)");
            }
            _ => panic!("expected Choice"),
        }

        // 3. Select custom plane "Rohdecke (Z = 0.150 m)"
        let _ = app.update(Message::PropGeomChoiceChanged {
            field: "wall_base_plane",
            value: "Rohdecke (Z = 0.150 m)".into(),
        });
        let ent_after2 = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
        let wall_after2 = wall_from_entity(ent_after2).unwrap();
        assert_eq!(wall_after2.base_plane_id, Some(eg_custom_id));
        assert_eq!(wall_after2.base_plane_name.as_deref(), Some("Rohdecke"));

        // 4. Select top plane "OG_ELEVATION (Z = 2.800 m)"
        let _ = app.update(Message::PropGeomChoiceChanged {
            field: "wall_top_plane",
            value: "OG_ELEVATION (Z = 2.800 m)".into(),
        });
        let ent_after3 = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
        let wall_after3 = wall_from_entity(ent_after3).unwrap();
        assert_eq!(wall_after3.top_plane_id, Some(og_elev_id));
        assert_eq!(wall_after3.top_plane_name.as_deref(), Some("OG_ELEVATION"));

        // 5. Reset base plane to "(none)"
        let _ = app.update(Message::PropGeomChoiceChanged {
            field: "wall_base_plane",
            value: "(none)".into(),
        });
        let ent_after4 = app.tabs[tab].scene.document.get_entity(wall_handle).unwrap();
        let wall_after4 = wall_from_entity(ent_after4).unwrap();
        assert_eq!(wall_after4.base_plane_id, None);
        assert_eq!(wall_after4.base_plane_name, None);

        // 6. Test Slab Plane and Justification via PropGeomChoiceChanged
        use crate::modules::aec::engine::slab::{Slab, SlabJustification};
        use crate::modules::aec::engine::slab_xdata::{slab_from_entity, slab_record_for_slab};

        let mut slab_poly = LwPolyline::new();
        slab_poly.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        slab_poly.add_vertex(LwVertex::new(Vector2::new(4.0, 0.0)));
        slab_poly.add_vertex(LwVertex::new(Vector2::new(4.0, 4.0)));
        slab_poly.add_vertex(LwVertex::new(Vector2::new(0.0, 4.0)));
        slab_poly.is_closed = true;
        let slab_handle = app.tabs[tab].scene.add_entity(EntityType::LwPolyline(slab_poly));
        let slab = Slab::new("style_slab_concrete_20", 1);
        let mut srec = ExtendedDataRecord::new(AEC_APPID);
        srec.values = slab_record_for_slab(&slab);
        app.tabs[tab]
            .scene
            .document
            .get_entity_mut(slab_handle)
            .unwrap()
            .common_mut()
            .extended_data
            .add_record(srec);

        app.tabs[tab].scene.deselect_all();
        app.tabs[tab].scene.select_entity(slab_handle, true);
        app.refresh_properties();

        // Slab initial property section exposes only slab_top_plane & slab_top_offset
        let s_ent = app.tabs[tab].scene.document.get_entity(slab_handle).unwrap();
        let s_sec = slab_prop_section(s_ent, None, Some(&project)).unwrap();
        let s_fields: Vec<&str> = s_sec.props.iter().map(|p| p.field).collect();
        assert!(s_fields.contains(&"slab_top_plane"));
        assert!(s_fields.contains(&"slab_top_offset"));
        assert!(!s_fields.contains(&"slab_base_plane"));
        assert!(!s_fields.contains(&"slab_base_offset"));

        // Choose "Rohdecke (Z = 0.150 m)" for slab_top_plane
        let _ = app.update(Message::PropGeomChoiceChanged {
            field: "slab_top_plane",
            value: "Rohdecke (Z = 0.150 m)".into(),
        });
        let s_ent_after = app.tabs[tab].scene.document.get_entity(slab_handle).unwrap();
        let slab_after = slab_from_entity(s_ent_after).unwrap();
        assert_eq!(slab_after.top_plane_id, Some(eg_custom_id));
        assert_eq!(slab_after.top_plane_name.as_deref(), Some("Rohdecke"));

        // Change justification to OKRD
        let _ = app.update(Message::PropGeomChoiceChanged {
            field: "slab_justification",
            value: "OKRD (Oberkante Rohdecke)".into(),
        });
        let s_ent_after2 = app.tabs[tab].scene.document.get_entity(slab_handle).unwrap();
        let slab_after2 = slab_from_entity(s_ent_after2).unwrap();
        assert_eq!(slab_after2.justification, SlabJustification::StructuralTop);
    }
}
