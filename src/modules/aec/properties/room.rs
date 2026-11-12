use super::*;

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
