//! Property-panel contributions for AEC entities.

use acadrust::{EntityType, Handle};

use crate::modules::aec::commands;
use crate::modules::aec::engine::library::StyleLibrary;
use crate::t;

pub fn collapse_selection_to_wall_package<'a>(
    scene: &'a crate::scene::Scene,
    selected: Vec<(Handle, &'a EntityType)>,
) -> Vec<(Handle, &'a EntityType)> {
    if selected.is_empty() {
        return selected;
    }
    let owners: Vec<Handle> = selected
        .iter()
        .map(|(handle, _)| commands::resolve_wall_package(scene, *handle))
        .collect();
    let owner = owners[0];
    if owner.is_null() || owners.iter().any(|h| *h != owner) {
        return selected;
    }
    let Some(entity) = scene.document.get_entity(owner) else {
        return selected;
    };
    if commands::wall_from_entity(entity).is_none() {
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
    if let Some(wall) = commands::wall_from_entity(entity) {
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

    let openings = commands::openings_for_host_wall(scene, wall_handle);
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

    let (storey_id, storey) = commands::storey_from_entity(entity)?;
    let storey_handle = entity.common().handle;
    let members = commands::walls_for_storey(scene, storey_handle);

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
            .and_then(commands::wall_from_entity)
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
pub fn extend_entity_sections(
    scene: &crate::scene::Scene,
    handle: Handle,
    entity: &EntityType,
    style_library: Option<&StyleLibrary>,
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    sections: &mut Vec<crate::scene::model::object::PropSection>,
) {
    let wall_handle = commands::resolve_wall_package(scene, handle);
    let wall_entity = scene.document.get_entity(wall_handle).unwrap_or(entity);
    if let Some(wall_section) = wall_prop_section(wall_entity, style_library, project) {
        sections.push(wall_section);
        sections.extend(wall_relation_sections(scene, wall_handle));
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
    let extracted = commands::extract_style_library_from_scene(scene);
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
    commands::is_wall_derived_non_axis(scene, handle)
}

pub fn resolve_wall_package(scene: &crate::scene::Scene, handle: Handle) -> Handle {
    commands::resolve_wall_package(scene, handle)
}

pub fn wall_from_entity(entity: &EntityType) -> bool {
    commands::wall_from_entity(entity).is_some()
}

pub fn selection_is_all_walls(
    scene: &crate::scene::Scene,
    handles: impl IntoIterator<Item = Handle>,
) -> bool {
    let mut any = false;
    for h in handles {
        any = true;
        let resolved = commands::resolve_wall_package(scene, h);
        let Some(entity) = scene.document.get_entity(resolved) else {
            return false;
        };
        if commands::wall_from_entity(entity).is_none() {
            return false;
        }
    }
    any
}

pub fn append_wall_junction_grips(
    scene: &crate::scene::Scene,
    handle: Handle,
    entity: &EntityType,
    entity_grips: &mut Vec<crate::scene::model::object::GripDef>,
) {
    if commands::wall_from_entity(entity).is_none() {
        return;
    }
    let verts = commands::get_wall_vertices(scene, handle);
    if verts.len() < 2 {
        return;
    }
    for (end_index, world) in [(0usize, verts[0]), (1usize, verts[verts.len() - 1])] {
        let participants = commands::walls_at_junction(scene, handle, end_index);
        if participants.len() > 1 {
            entity_grips.push(crate::entities::common::dropdown_grip(
                commands::wall_junction_dropdown_grip_id(end_index),
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
    use crate::modules::aec::commands::{wall_record_for_wall, AEC_APPID};
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
}
