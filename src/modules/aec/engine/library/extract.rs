use super::*;

pub(super) fn parse_layer_function(s: &str) -> LayerFunction {
    match s {
        "Structural" => LayerFunction::Structural,
        "Insulation" => LayerFunction::Insulation,
        "Finish" => LayerFunction::Finish,
        other => LayerFunction::Other(other.to_string()),
    }
}

pub(super) fn wall_layer_signature(layers: &[WallLayer]) -> Vec<(String, String, i64, i64)> {
    layers
        .iter()
        .map(|l| {
            (
                l.material.clone(),
                l.function.clone(),
                (l.thickness * 1_000_000.0).round() as i64,
                (l.axis_offset * 1_000_000.0).round() as i64,
            )
        })
        .collect()
}

pub(super) fn wall_layers_to_style_layers(layers: &[WallLayer]) -> Vec<Layer> {
    layers
        .iter()
        .map(|l| Layer {
            material_id: l.material.clone(),
            thickness: LayerValue::Fixed(l.thickness),
            function: parse_layer_function(&l.function),
            axis_offset: LayerValue::Fixed(l.axis_offset),
            bottom_offset: l.bottom_offset,
            top_offset: l.top_offset,
            layer_override: l.layer_override.clone(),
            hatch_override: l.hatch_override.clone(),
            role_tag: None,
            layer_id: l.layer_id,
        })
        .collect()
}

pub(super) const SESSION_STYLE_NAME_MAX_CHARS: usize = 56;

/// Display name for a reconstructed session style: `style1 (Putz+Mauerwerk+Putz)`
/// or, if that is too long / materials are empty, `style1 (3 Schalen)`.
pub(super) fn session_wall_style_display_name(base: &str, layers: &[WallLayer]) -> String {
    let n = layers.len();
    let mats: Vec<&str> = layers
        .iter()
        .map(|l| l.material.as_str())
        .filter(|m| !m.is_empty())
        .collect();
    if !mats.is_empty() {
        let candidate = format!("{base} ({})", mats.join("+"));
        if candidate.chars().count() <= SESSION_STYLE_NAME_MAX_CHARS {
            return candidate;
        }
    }
    let unit = if n == 1 { "Schale" } else { "Schalen" };
    format!("{base} ({n} {unit})")
}

/// Reconstruct materials and wall styles from wall XDATA snapshots.
///
/// One [`WallStyle`] per distinct layer stack under a `style_id`; the first
/// stack keeps the original id, further stacks get a synthetic id
/// (`style1#2`, …). When several stacks share a base id, display names include
/// the construction (materials or shell count). Materials are keyed by the
/// snapshot material name.
pub fn extract_style_library_from_walls<'a, I>(walls: I) -> StyleLibrary
where
    I: IntoIterator<Item = &'a Wall>,
{
    let mut lib = StyleLibrary::empty();
    let standard = load_or_seed();
    let mut variants: Vec<(String, String, Vec<WallLayer>)> = Vec::new();
    let mut style_sigs: std::collections::HashMap<String, Vec<(String, String, i64, i64)>> =
        std::collections::HashMap::new();
    let mut style_dupes: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut base_variant_count: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for wall in walls {
        for layer in &wall.layers {
            if layer.material.is_empty() {
                continue;
            }
            if lib.materials.iter().any(|m| m.id == layer.material) {
                continue;
            }
            if let Some(src) = standard.materials.iter().find(|m| {
                m.id == layer.material || m.name.eq_ignore_ascii_case(&layer.material)
            }) {
                let mut copied = src.clone();
                copied.id = layer.material.clone();
                copied.name = layer.material.clone();
                lib.upsert_material(copied);
            } else {
                lib.upsert_material(Material::new(
                    layer.material.clone(),
                    layer.material.clone(),
                    "SOLID".to_string(),
                    0x808080,
                    "Continuous".to_string(),
                ));
            }
        }

        if wall.style_id.trim().is_empty() {
            continue;
        }
        let sig = wall_layer_signature(&wall.layers);
        let (id, base) = if let Some(existing) = style_sigs.get(&wall.style_id) {
            if existing == &sig {
                continue;
            }
            let n = style_dupes.entry(wall.style_id.clone()).or_insert(1);
            *n += 1;
            let synthetic = format!("{}#{}", wall.style_id, *n);
            if style_sigs.contains_key(&synthetic) {
                continue;
            }
            style_sigs.insert(synthetic.clone(), sig);
            (synthetic, wall.style_id.clone())
        } else {
            style_sigs.insert(wall.style_id.clone(), sig);
            (wall.style_id.clone(), wall.style_id.clone())
        };
        *base_variant_count.entry(base.clone()).or_insert(0) += 1;
        variants.push((id, base, wall.layers.clone()));
    }

    for (id, base, layers) in variants {
        let name = if base_variant_count.get(&base).copied().unwrap_or(0) > 1 {
            session_wall_style_display_name(&base, &layers)
        } else {
            base
        };
        lib.upsert_wall_style(WallStyle {
            style: Style {
                id,
                name,
                category: "Wall".to_string(),
                origin: Default::default(),
                parent: None,
            },
            layers: wall_layers_to_style_layers(&layers),
            default_profile: None,
        });
    }
    lib
}

pub(super) fn slab_layer_signature(layers: &[crate::modules::aec::engine::slab::SlabLayer]) -> Vec<(String, String, i64)> {
    layers
        .iter()
        .map(|l| {
            (
                l.material.clone(),
                crate::modules::aec::engine::slab_xdata::layer_function_to_str(&l.function),
                (l.thickness * 1_000_000.0).round() as i64,
            )
        })
        .collect()
}

pub(super) fn slab_layers_to_style_layers(layers: &[crate::modules::aec::engine::slab::SlabLayer]) -> Vec<SlabStyleLayer> {
    layers
        .iter()
        .map(|l| SlabStyleLayer {
            material_id: l.material.clone(),
            thickness: LayerValue::Fixed(l.thickness),
            function: l.function.clone(),
            vertical_offset: LayerValue::Fixed(l.vertical_offset),
            layer_override: l.layer_override.clone(),
            hatch_override: l.hatch_override.clone(),
            role_tag: l.role_tag.clone(),
            layer_id: l.layer_id,
        })
        .collect()
}

/// Reconstruct materials and slab styles from slab XDATA snapshots.
pub fn extract_style_library_from_slabs<'a, I>(slabs: I) -> StyleLibrary
where
    I: IntoIterator<Item = &'a Slab>,
{
    let mut lib = StyleLibrary::empty();
    let standard = load_or_seed();
    let mut variants: Vec<(String, String, Vec<crate::modules::aec::engine::slab::SlabLayer>)> = Vec::new();
    let mut style_sigs: std::collections::HashMap<String, Vec<(String, String, i64)>> =
        std::collections::HashMap::new();
    let mut style_dupes: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut base_variant_count: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for slab in slabs {
        for layer in &slab.layers {
            if layer.material.is_empty() {
                continue;
            }
            if lib.materials.iter().any(|m| m.id == layer.material) {
                continue;
            }
            if let Some(src) = standard.materials.iter().find(|m| {
                m.id == layer.material || m.name.eq_ignore_ascii_case(&layer.material)
            }) {
                let mut copied = src.clone();
                copied.id = layer.material.clone();
                copied.name = layer.material.clone();
                lib.upsert_material(copied);
            } else {
                lib.upsert_material(Material::new(
                    layer.material.clone(),
                    layer.material.clone(),
                    "SOLID".to_string(),
                    0x808080,
                    "Continuous".to_string(),
                ));
            }
        }

        if slab.style_id.trim().is_empty() {
            continue;
        }
        let sig = slab_layer_signature(&slab.layers);
        let (id, base) = if let Some(existing) = style_sigs.get(&slab.style_id) {
            if existing == &sig {
                continue;
            }
            let n = style_dupes.entry(slab.style_id.clone()).or_insert(1);
            *n += 1;
            let synthetic = format!("{}#{}", slab.style_id, *n);
            if style_sigs.contains_key(&synthetic) {
                continue;
            }
            style_sigs.insert(synthetic.clone(), sig);
            (synthetic, slab.style_id.clone())
        } else {
            style_sigs.insert(slab.style_id.clone(), sig);
            (slab.style_id.clone(), slab.style_id.clone())
        };
        *base_variant_count.entry(base.clone()).or_insert(0) += 1;
        variants.push((id, base, slab.layers.clone()));
    }

    for (id, base, layers) in variants {
        let name = if base_variant_count.get(&base).copied().unwrap_or(0) > 1 {
            format!("{base} ({} Schichten)", layers.len())
        } else {
            base
        };
        lib.upsert_slab_style(SlabStyle {
            style: Style {
                id,
                name,
                category: "Slab".to_string(),
                origin: Default::default(),
                parent: None,
            },
            structural_style_id: None,
            default_finish_style_id: None,
            layers: slab_layers_to_style_layers(&layers),
            default_profile: None,
        });
    }
    lib
}

/// Keep only extracted ids that are absent from Standard and (when present)
/// the project library. Identical or colliding ids are not copied.
pub fn session_library_excluding_existing(
    extracted: &StyleLibrary,
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> StyleLibrary {
    let standard = load_or_seed();
    let project_lib = project.map(|p| &p.material_wall_style_library);
    let mut session = StyleLibrary::empty();
    for material in &extracted.materials {
        if material_copy_conflict(&standard, material) != CopyConflict::None {
            continue;
        }
        if let Some(plib) = project_lib {
            if material_copy_conflict(plib, material) != CopyConflict::None {
                continue;
            }
        }
        session.upsert_material(material.clone());
    }
    for wall_style in &extracted.wall_styles {
        if wall_style_copy_conflict(&standard, wall_style) != CopyConflict::None {
            continue;
        }
        if let Some(plib) = project_lib {
            if wall_style_copy_conflict(plib, wall_style) != CopyConflict::None {
                continue;
            }
        }
        session.upsert_wall_style(wall_style.clone());
    }
    for opening_style in &extracted.opening_styles {
        if opening_style_copy_conflict(&standard, opening_style) != CopyConflict::None {
            continue;
        }
        if let Some(plib) = project_lib {
            if opening_style_copy_conflict(plib, opening_style) != CopyConflict::None {
                continue;
            }
        }
        session.upsert_opening_style(opening_style.clone());
    }
    for slab_style in &extracted.slab_styles {
        if slab_style_copy_conflict(&standard, slab_style) != CopyConflict::None {
            continue;
        }
        if let Some(plib) = project_lib {
            if slab_style_copy_conflict(plib, slab_style) != CopyConflict::None {
                continue;
            }
        }
        session.upsert_slab_style(slab_style.clone());
    }
    session.stamp_origin(StyleOrigin::Session);
    session
}

