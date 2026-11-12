use super::*;

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
