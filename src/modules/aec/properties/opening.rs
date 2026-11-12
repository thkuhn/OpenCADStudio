use super::*;

/// Append wall/storey property sections when the selection is AEC-related.
pub(super) fn shape_options() -> Vec<String> {
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
