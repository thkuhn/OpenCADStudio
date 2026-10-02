//! Property-panel contributions for AEC entities.

use acadrust::{EntityType, Handle};

use crate::modules::aec::engine::join_ops;
use crate::modules::aec::engine::junction_pick;
use crate::modules::aec::engine::opening_display;
use crate::modules::aec::engine::opening_shape::OpeningShape;
use crate::modules::aec::engine::opening_style::{apply_style_defaults, HingeSide};
use crate::modules::aec::engine::opening_xdata;
use crate::modules::aec::engine::openings::{
    NicheSide, Opening, OpeningKind, OpeningReferenceSide, SwingSide,
};
use crate::modules::aec::engine::slab::{Slab, SlabJustification};
use crate::modules::aec::engine::slab_opening::{SlabOpening, SlabOpeningDepth, SlabOpeningKind};
use crate::modules::aec::engine::slab_package;
use crate::modules::aec::engine::slab_xdata;
use crate::modules::aec::engine::storey_xdata;
use crate::modules::aec::engine::xdata;
use crate::modules::aec::engine::wall_package;
use crate::modules::aec::engine::library::StyleLibrary;
use crate::t;

pub fn aec_entity_title(
    scene: &crate::scene::Scene,
    handle: Handle,
    entity: &EntityType,
) -> Option<String> {
    if xdata::wall_from_entity(entity).is_some() {
        return Some(t!("Wall").into_owned());
    }
    if slab_xdata::slab_from_entity(entity).is_some() {
        return Some(crate::tr!("aec", "slab-title"));
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
            OpeningKind::Window => t!("Window").into_owned(),
            OpeningKind::Door => t!("Door").into_owned(),
            OpeningKind::Breakthrough => t!("Opening").into_owned(),
        };
        return Some(name);
    }
    if let Some(owner) = opening_display::opening_owner_if_any(scene, handle) {
        if let Some(owner_ent) = scene.document.get_entity(owner) {
            if let Some(opening) = opening_xdata::opening_from_entity(owner_ent, owner) {
                let name = match opening.kind {
                    OpeningKind::Window => t!("Window").into_owned(),
                    OpeningKind::Door => t!("Door").into_owned(),
                    OpeningKind::Breakthrough => t!("Opening").into_owned(),
                };
                return Some(name);
            }
        }
    }
    if storey_xdata::storey_from_entity(entity).is_some() {
        return Some(t!("Storey").into_owned());
    }
    if crate::modules::aec::project::preview::control_plane_from_entity(entity).is_some() {
        return Some(t!("aec.control-plane").into_owned());
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
    if xdata::wall_from_entity(entity).is_some() {
        return vec![(owner, entity)];
    }

    // Collapse slab derived children onto the carrier package.
    let slab_owners: Vec<Handle> = selected
        .iter()
        .map(|(handle, _)| {
            if let Some(op) = slab_package::slab_opening_owner_if_any(scene, *handle) {
                op
            } else {
                slab_package::resolve_slab_package(scene, *handle)
            }
        })
        .collect();
    let slab_owner = slab_owners[0];
    if !slab_owner.is_null()
        && slab_owners.iter().all(|h| *h == slab_owner)
        && scene
            .document
            .get_entity(slab_owner)
            .is_some_and(|e| {
                slab_xdata::slab_from_entity(e).is_some()
                    || slab_xdata::slab_opening_from_entity(e).is_some()
            })
    {
        if let Some(entity) = scene.document.get_entity(slab_owner) {
            return vec![(slab_owner, entity)];
        }
    }
    selected
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
            }
        }
    }
    if let Some(storey_section) = storey_prop_section(scene, entity) {
        sections.push(storey_section);
    }
    if let Some(plane_section) = control_plane_prop_section(entity) {
        sections.push(plane_section);
    }
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
    let base_label = plane_label(slab.base_plane_id, slab.base_plane_name.as_deref());
    let top_label = plane_label(slab.top_plane_id, slab.top_plane_name.as_deref());
    ensure_option(&mut plane_options, &base_label);
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

    let mut props = vec![
        crate::scene::model::object::Property {
            label: t!("aec.slab-style").into_owned(),
            field: "slab_style",
            value: crate::scene::model::object::PropValue::Picker {
                value: style_name,
                handles: vec![entity.common().handle],
            },
        },
        crate::scene::model::object::Property {
            label: t!("aec.slab-justification").into_owned(),
            field: "slab_justification",
            value: crate::scene::model::object::PropValue::Choice {
                selected: slab.justification.display_name().to_string(),
                options: vec![
                    SlabJustification::Top.display_name().to_string(),
                    SlabJustification::StructuralTop.display_name().to_string(),
                    SlabJustification::Bottom.display_name().to_string(),
                ],
            },
        },
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
        crate::scene::model::object::Property {
            label: t!("aec.slab-base-plane").into_owned(),
            field: "slab_base_plane",
            value: crate::scene::model::object::PropValue::Choice {
                selected: base_label,
                options: plane_options.clone(),
            },
        },
        crate::entities::common::edit_prop(
            t!("aec.slab-base-offset").as_ref(),
            "slab_base_offset",
            slab.base_offset,
        ),
        crate::scene::model::object::Property {
            label: t!("aec.slab-top-plane").into_owned(),
            field: "slab_top_plane",
            value: crate::scene::model::object::PropValue::Choice {
                selected: top_label,
                options: plane_options,
            },
        },
        crate::entities::common::edit_prop(
            t!("aec.slab-top-offset").as_ref(),
            "slab_top_offset",
            slab.top_offset,
        ),
        crate::entities::common::ro_prop(
            t!("aec.slab-thickness").as_ref(),
            "slab_thickness",
            format!("{:.3} m", slab.total_thickness()),
        ),
        crate::entities::common::ro_prop(
            t!("aec.slab-area").as_ref(),
            "slab_area",
            format!("{:.3} m²", area),
        ),
        crate::entities::common::ro_prop(
            t!("aec.slab-perimeter").as_ref(),
            "slab_perimeter",
            format!("{:.3} m", peri),
        ),
        crate::entities::common::ro_prop(
            t!("aec.slab-volume").as_ref(),
            "slab_volume",
            format!("{:.3} m³", vol),
        ),
        crate::entities::common::ro_prop(
            t!("aec.slab-storey").as_ref(),
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
            t!("aec.slab-layer").as_ref(),
            "slab_layer",
            format!("{} — {}", i + 1, layer_info),
        ));
    }

    Some(crate::scene::model::object::PropSection {
        title: t!("aec.slab-section").into_owned(),
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
            label: t!("aec.slabopening-kind").into_owned(),
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
            label: t!("aec.slabopening-depth-mode").into_owned(),
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
            t!("aec.slabopening-recess-depth").as_ref(),
            "slabopening_recess_depth",
            d,
        ));
    }
    props.push(crate::entities::common::ro_prop(
        t!("aec.slabopening-host").as_ref(),
        "slabopening_host",
        format!("{:X}", opening.host_slab.value()),
    ));
    props.push(crate::entities::common::ro_prop(
        t!("aec.slabopening-area").as_ref(),
        "slabopening_area",
        format!("{:.3} m²", area),
    ));
    props.push(crate::entities::common::ro_prop(
        t!("aec.slabopening-perimeter").as_ref(),
        "slabopening_perimeter",
        format!("{:.3} m", peri),
    ));
    let _ = opening_handle;
    crate::scene::model::object::PropSection {
        title: t!("aec.slabopening-section").into_owned(),
        props,
    }
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

pub fn is_aec_derived_non_carrier(scene: &crate::scene::Scene, handle: Handle) -> bool {
    wall_package::is_wall_derived_non_axis(scene, handle)
        || slab_package::is_slab_derived(scene, handle)
        || slab_package::slab_opening_owner_if_any(scene, handle).is_some_and(|owner| owner != handle)
        || opening_display::opening_owner_if_any(scene, handle).is_some_and(|owner| owner != handle)
}

pub fn resolve_wall_package(scene: &crate::scene::Scene, handle: Handle) -> Handle {
    wall_package::resolve_wall_package(scene, handle)
}

pub fn expand_handles_for_aec_packages(scene: &crate::scene::Scene, handles: &[Handle]) -> Vec<Handle> {
    let handles = wall_package::expand_handles_for_wall_packages(scene, handles);
    slab_package::expand_handles_for_slab_packages(scene, &handles)
}

pub fn resolve_aec_package(scene: &crate::scene::Scene, handle: Handle) -> Handle {
    if let Some(opening_owner) = opening_display::opening_owner_if_any(scene, handle) {
        return opening_owner;
    }
    if let Some(slab_opening_owner) = slab_package::slab_opening_owner_if_any(scene, handle) {
        return slab_opening_owner;
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

pub fn wall_from_entity(entity: &EntityType) -> bool {
    xdata::wall_from_entity(entity).is_some()
}

pub fn slab_from_entity(entity: &EntityType) -> bool {
    slab_xdata::slab_from_entity(entity).is_some()
}

pub fn slab_opening_from_entity(entity: &EntityType) -> bool {
    slab_xdata::slab_opening_from_entity(entity).is_some()
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
            || field.starts_with("slab_")
            || field.starts_with("slabopening_")
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
                    if let Some(v) = crate::entities::common::parse_f64(val) {
                        if v >= 0.0 {
                            opening.depth = SlabOpeningDepth::Recess(v);
                            let _ = slab_xdata::write_slab_opening_model(
                                &mut self.tabs[tab].scene,
                                owner,
                                &opening,
                            );
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
                "slab_base_offset" | "slab_top_offset" => {
                    if let Some(v) = crate::entities::common::parse_f64(val) {
                        let (base, top) = if field == "slab_base_offset" {
                            (Some(v), None)
                        } else {
                            (None, Some(v))
                        };
                        if slab_xdata::write_slab_plane_offsets(
                            &mut self.tabs[tab].scene,
                            slab_owner,
                            base,
                            top,
                        ) {
                            let _ = self
                                .regenerate_slab_respecting_active_display_config(tab, slab_owner);
                        }
                    }
                }
                "slab_base_plane" | "slab_top_plane" => {
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
        assert!(fields.contains(&"slab_base_plane"));
        assert!(fields.contains(&"slab_top_plane"));
        assert!(fields.contains(&"slab_base_offset"));
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
}
