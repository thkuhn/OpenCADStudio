//! Property-panel contributions for AEC entities.

use acadrust::{EntityType, Handle};

use crate::modules::aec::engine::join_ops;
use crate::modules::aec::engine::junction_pick;
use crate::modules::aec::engine::opening_display;
use crate::modules::aec::engine::opening_shape::OpeningShape;
use crate::modules::aec::engine::opening_style::{apply_style_defaults, HingeSide};
use crate::modules::aec::engine::opening_xdata;
use crate::modules::aec::engine::openings::{NicheSide, Opening, OpeningKind, OpeningReferenceSide};
use crate::modules::aec::engine::storey_xdata;
use crate::modules::aec::engine::xdata;
use crate::modules::aec::engine::wall_package;
use crate::modules::aec::engine::library::StyleLibrary;
use crate::t;

pub fn collapse_selection_to_wall_package<'a>(
    scene: &'a crate::scene::Scene,
    selected: Vec<(Handle, &'a EntityType)>,
) -> Vec<(Handle, &'a EntityType)> {
    if selected.is_empty() {
        return selected;
    }
    let opening_owners: Vec<Option<Handle>> = selected
        .iter()
        .map(|(handle, _)| opening_display::opening_owner_if_any(scene, *handle))
        .collect();
    if opening_owners.iter().all(|o| o.is_some()) {
        let owner = opening_owners[0].unwrap();
        if opening_owners.iter().all(|o| *o == Some(owner)) {
            if let Some(entity) = scene.document.get_entity(owner) {
                return vec![(owner, entity)];
            }
        }
    }
    let owners: Vec<Handle> = selected
        .iter()
        .map(|(handle, _)| wall_package::resolve_wall_package(scene, *handle))
        .collect();
    let owner = owners[0];
    if owner.is_null() || owners.iter().any(|h| *h != owner) {
        return selected;
    }
    let Some(entity) = scene.document.get_entity(owner) else {
        return selected;
    };
    if xdata::wall_from_entity(entity).is_none() {
        return selected;
    }
    vec![(owner, entity)]
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
        let none = t!("(none)").into_owned();
        let mut plane_options = vec![none.clone()];
        if let Some(project) = project {
            for b in &project.buildings {
                for s in &b.storeys {
                    for p in &s.control_planes {
                        if !plane_options.iter().any(|n| n == &p.name) {
                            plane_options.push(p.name.clone());
                        }
                    }
                }
            }
        }
        let plane_label = |id: Option<uuid::Uuid>, stored_name: Option<&str>| {
            id.and_then(|id| {
                project.and_then(|proj| {
                    proj.buildings.iter().find_map(|b| {
                        b.storeys
                            .iter()
                            .find_map(|s| s.plane(id).map(|p| p.name.clone()))
                    })
                })
            })
            .or_else(|| {
                stored_name
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .map(|n| n.to_string())
            })
            .or_else(|| id.map(|id| id.to_string()))
            .unwrap_or_else(|| none.clone())
        };
        let ensure_option = |opts: &mut Vec<String>, label: &str| {
            if !opts.iter().any(|n| n == label) {
                opts.push(label.to_string());
            }
        };
        let base_label = plane_label(wall.base_plane_id, wall.base_plane_name.as_deref());
        let top_label = plane_label(wall.top_plane_id, wall.top_plane_name.as_deref());
        ensure_option(&mut plane_options, &base_label);
        ensure_option(&mut plane_options, &top_label);
        let mut props = vec![
            crate::entities::common::edit_prop(t!("Height").as_ref(), "wall_height", wall.height),
            crate::scene::model::object::Property {
                label: t!("Base plane").into_owned(),
                field: "wall_base_plane",
                value: crate::scene::model::object::PropValue::Choice {
                    selected: base_label,
                    options: plane_options.clone(),
                },
            },
            crate::entities::common::edit_prop(
                t!("Base offset").as_ref(),
                "wall_base_offset",
                wall.base_offset,
            ),
            crate::scene::model::object::Property {
                label: t!("Top plane").into_owned(),
                field: "wall_top_plane",
                value: crate::scene::model::object::PropValue::Choice {
                    selected: top_label,
                    options: plane_options,
                },
            },
            crate::entities::common::edit_prop(
                t!("Top offset").as_ref(),
                "wall_top_offset",
                wall.top_offset,
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
                crate::entities::common::edit_prop(
                    t!("aec.wall-base-z").as_ref(),
                    "wall_base_z",
                    wall.base_origin[2],
                ),
            );
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
            let thickness_str = crate::entities::common::format_length(layer.thickness);
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
            let display = format!("Wall #{:X}", peer.value());
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

    let (storey_id, storey) = storey_xdata::storey_from_entity(entity)?;
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
            format!("Wall #{:X}", member.value())
        } else {
            format!("Entity #{:X}", member.value())
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
        title: t!("aec.control-plane").into_owned(),
        props: vec![crate::scene::model::object::Property {
            label: t!("aec.control-plane-name").into_owned(),
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
            let h = HingeSide::from_str(val);
            if opening.hinge == h {
                return false;
            }
            opening.hinge = h;
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
    let none = t!("(none)").into_owned();
    let mut plane_options = vec![none.clone()];
    if let Some(project) = project {
        for b in &project.buildings {
            for s in &b.storeys {
                for p in &s.control_planes {
                    if !plane_options.iter().any(|n| n == &p.name) {
                        plane_options.push(p.name.clone());
                    }
                }
            }
        }
    }
    let plane_label = |id: Option<uuid::Uuid>, stored_name: Option<&str>| {
        id.and_then(|id| {
            project.and_then(|proj| {
                proj.buildings.iter().find_map(|b| {
                    b.storeys
                        .iter()
                        .find_map(|s| s.plane(id).map(|p| p.name.clone()))
                })
            })
        })
        .or_else(|| {
            stored_name
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(|n| n.to_string())
        })
        .or_else(|| id.map(|id| id.to_string()))
        .unwrap_or_else(|| none.clone())
    };
    let ensure_option = |opts: &mut Vec<String>, label: &str| {
        if !opts.iter().any(|n| n == label) {
            opts.push(label.to_string());
        }
    };
    let sill_label = plane_label(opening.sill_plane_id, opening.sill_plane_name.as_deref());
    let head_label = plane_label(opening.head_plane_id, opening.head_plane_name.as_deref());
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
        crate::entities::common::edit_prop(t!("Width").as_ref(), "opening_width", opening.width),
        crate::entities::common::edit_prop(t!("Height").as_ref(), "opening_height", opening.height),
        crate::entities::common::edit_prop(
            crate::tr!("aec", "opening-sill").as_str(),
            "opening_sill",
            opening.sill_height,
        ),
        crate::scene::model::object::Property {
            label: crate::tr!("aec", "opening-sill-plane"),
            field: "opening_sill_plane",
            value: crate::scene::model::object::PropValue::Choice {
                selected: sill_label,
                options: plane_options.clone(),
            },
        },
        crate::entities::common::edit_prop(
            crate::tr!("aec", "opening-sill-offset").as_str(),
            "opening_sill_offset",
            opening.sill_offset,
        ),
        crate::scene::model::object::Property {
            label: crate::tr!("aec", "opening-head-plane"),
            field: "opening_head_plane",
            value: crate::scene::model::object::PropValue::Choice {
                selected: head_label,
                options: plane_options,
            },
        },
        crate::entities::common::edit_prop(
            crate::tr!("aec", "opening-head-offset").as_str(),
            "opening_head_offset",
            opening.head_offset,
        ),
        crate::scene::model::object::Property {
            label: crate::tr!("aec", "opening-hinge"),
            field: "opening_hinge",
            value: crate::scene::model::object::PropValue::Choice {
                selected: opening.hinge.as_str().to_string(),
                options: vec![
                    HingeSide::Left.as_str().into(),
                    HingeSide::Right.as_str().into(),
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
        crate::entities::common::edit_prop(
            crate::tr!("aec", "opening-cross-offset").as_str(),
            "opening_cross_offset",
            opening.cross_axis_offset,
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
    } else {
        let wall_handle = wall_package::resolve_wall_package(scene, handle);
        let wall_entity = scene.document.get_entity(wall_handle).unwrap_or(entity);
        if let Some(wall_section) = wall_prop_section(wall_entity, style_library, project) {
            sections.push(wall_section);
            sections.extend(wall_relation_sections(scene, wall_handle));
        }
    }
    if let Some(storey_section) = storey_prop_section(scene, entity) {
        sections.push(storey_section);
    }
    if let Some(plane_section) = control_plane_prop_section(entity) {
        sections.push(plane_section);
    }
}

pub fn session_styles_for_scene(
    scene: &crate::scene::Scene,
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> Option<StyleLibrary> {
    let extracted = xdata::extract_style_library_from_scene(scene);
    let session =
        crate::modules::aec::engine::library::session_library_excluding_existing(&extracted, project);
    if session.materials.is_empty() && session.wall_styles.is_empty() {
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

pub fn resolve_wall_package(scene: &crate::scene::Scene, handle: Handle) -> Handle {
    wall_package::resolve_wall_package(scene, handle)
}

pub fn wall_from_entity(entity: &EntityType) -> bool {
    xdata::wall_from_entity(entity).is_some()
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
                    if let Some(v) = crate::entities::common::parse_f64(val) {
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
                    }
                    return true;
                }
                _ => {}
            }
            if apply_opening_property(&mut opening, field, val, Some(&style_library)) {
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
                if let Some(v) = crate::entities::common::parse_f64(val) {
                    if v > 0.0 {
                        crate::modules::aec::engine::xdata::write_wall_height(
                            &mut self.tabs[tab].scene,
                            wall_owner,
                            v,
                        );
                        let _ =
                            self.regenerate_wall_respecting_active_display_config(tab, wall_owner);
                    }
                }
            }
            "wall_base_z" => {
                if let Some(v) = crate::entities::common::parse_f64(val) {
                    crate::modules::aec::project::wall_planes::apply_wall_base_z(
                        &mut self.tabs[tab].scene,
                        wall_owner,
                        v,
                    );
                }
            }
            "wall_base_offset" | "wall_top_offset" => {
                if let Some(v) = crate::entities::common::parse_f64(val) {
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
                    let _ =
                        self.regenerate_wall_respecting_active_display_config(tab, wall_owner);
                }
            }
            "wall_hatch_angle" => {
                if let Some(v) = crate::entities::common::parse_f64(val) {
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
            }
            "wall_thickness" | "wall_material" => {}
            _ => {}
        }
        true
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
        assert_eq!(o.hinge, HingeSide::Right);
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
}
