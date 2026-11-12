use super::*;

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

pub(super) fn control_plane_prop_section(entity: &EntityType) -> Option<crate::scene::model::object::PropSection> {
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
