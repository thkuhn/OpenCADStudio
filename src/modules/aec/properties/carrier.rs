use super::*;

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

pub(super) fn entity_boundary_xy(entity: &EntityType) -> Vec<(f64, f64)> {
    match entity {
        EntityType::LwPolyline(pl) => pl
            .vertices
            .iter()
            .map(|v| (v.location.x, v.location.y))
            .collect(),
        _ => Vec::new(),
    }
}
