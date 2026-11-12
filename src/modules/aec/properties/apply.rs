use super::*;

pub(super) const CHOICE_NORMAL: &str = "normal";
pub(super) const CHOICE_FLIPPED: &str = "flipped";
pub(super) const CHOICE_AUTOMATIC: &str = "automatic";
pub(super) const CHOICE_MANUAL: &str = "manual";
pub(super) const CHOICE_ACTION_NONE: &str = "-";
pub(super) const CHOICE_ACTION_UPDATE: &str = "update-now";

pub(super) fn choice_matches(val: &str, id: &str, localized: &str) -> bool {
    let trimmed = val.trim();
    trimmed.eq_ignore_ascii_case(id) || trimmed == localized
}

/// Parses a viewing-depth field. `0` and negative values mean unlimited (`None`).
/// Returns `None` when the input cannot be parsed as a finite number.
pub(super) fn parse_view_depth(val: &str) -> Option<Option<f64>> {
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

pub(super) fn parse_finite_metres(val: &str) -> Option<f64> {
    let v = crate::entities::common::parse_f64(val)?;
    v.is_finite().then_some(v)
}

pub(super) fn is_flipped_choice(val: &str) -> bool {
    let trimmed = val.trim();
    choice_matches(trimmed, CHOICE_FLIPPED, &crate::tr!("aec", "marker-flip-flipped"))
        || trimmed.eq_ignore_ascii_case("true")
        || trimmed == "1"
}

pub(super) fn is_normal_choice(val: &str) -> bool {
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
