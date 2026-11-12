use super::*;

/// Merges project, session and Standard items (in that order). The first
/// occurrence of an id wins; project items are all kept as-is.
fn merge_layers<T: Clone, E>(
    project: Option<&[T]>,
    session: Option<&[T]>,
    standard: Vec<T>,
    id: impl Fn(&T) -> String,
    make: impl Fn(T, LibrarySource) -> E,
) -> Vec<E> {
    let mut entries = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in project.unwrap_or_default() {
        seen.insert(id(item));
        entries.push(make(item.clone(), LibrarySource::Project));
    }
    for item in session.unwrap_or_default() {
        if seen.insert(id(item)) {
            entries.push(make(item.clone(), LibrarySource::Session));
        }
    }
    for item in standard {
        if seen.insert(id(&item)) {
            entries.push(make(item, LibrarySource::Standard));
        }
    }
    entries
}

/// Builds a combined material list from the Standard library and (when present)
/// the project's embedded library. Project entries win on id collision.
pub fn combined_material_entries(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> Vec<CombinedMaterialEntry> {
    combined_material_entries_with_session(project, None)
}

/// Like [`combined_material_entries`], with a session overlay after project
/// and before Standard. Project and session win on id; Standard fills gaps.
pub fn combined_material_entries_with_session(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    session: Option<&StyleLibrary>,
) -> Vec<CombinedMaterialEntry> {
    let standard = load_or_seed();
    let mut entries = merge_layers(
        project.map(|p| p.material_wall_style_library.materials.as_slice()),
        session.map(|s| s.materials.as_slice()),
        standard.materials,
        |m| m.id.clone(),
        |material, source| CombinedMaterialEntry { material, source },
    );
    for e in &mut entries {
        e.material.style.origin = e.source.into();
    }
    entries
}

/// Builds a combined wall-style list from the Standard library and (when
/// present) the project's embedded library. Project entries win on id collision.
pub fn combined_wall_style_entries(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> Vec<CombinedWallStyleEntry> {
    combined_wall_style_entries_with_session(project, None)
}

/// Like [`combined_wall_style_entries`], with a session overlay after project
/// and before Standard.
pub fn combined_wall_style_entries_with_session(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    session: Option<&StyleLibrary>,
) -> Vec<CombinedWallStyleEntry> {
    let standard = load_or_seed();
    let mut entries = merge_layers(
        project.map(|p| p.material_wall_style_library.wall_styles.as_slice()),
        session.map(|s| s.wall_styles.as_slice()),
        standard.wall_styles,
        |s| s.style.id.clone(),
        |wall_style, source| CombinedWallStyleEntry { wall_style, source },
    );
    for e in &mut entries {
        e.wall_style.common_mut().origin = e.source.into();
    }
    entries
}

/// Builds a combined opening-style list. Project wins, then session, then Standard.
pub fn combined_opening_style_entries(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> Vec<CombinedOpeningStyleEntry> {
    combined_opening_style_entries_with_session(project, None)
}

/// Like [`combined_opening_style_entries`], with a session overlay after project
/// and before Standard.
pub fn combined_opening_style_entries_with_session(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    session: Option<&StyleLibrary>,
) -> Vec<CombinedOpeningStyleEntry> {
    let standard = load_or_seed();
    let mut entries = merge_layers(
        project.map(|p| p.material_wall_style_library.opening_styles.as_slice()),
        session.map(|s| s.opening_styles.as_slice()),
        standard.opening_styles,
        |s| s.style.id.clone(),
        |opening_style, source| CombinedOpeningStyleEntry { opening_style, source },
    );
    for e in &mut entries {
        e.opening_style.common_mut().origin = e.source.into();
    }
    entries
}

/// Builds a combined slab-style list. Project wins, then session, then Standard.
pub fn combined_slab_style_entries(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> Vec<CombinedSlabStyleEntry> {
    combined_slab_style_entries_with_session(project, None)
}

/// Like [`combined_slab_style_entries`], with a session overlay after project
/// and before Standard.
pub fn combined_slab_style_entries_with_session(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    session: Option<&StyleLibrary>,
) -> Vec<CombinedSlabStyleEntry> {
    let standard = load_or_seed();
    let mut entries = merge_layers(
        project.map(|p| p.material_wall_style_library.slab_styles.as_slice()),
        session.map(|s| s.slab_styles.as_slice()),
        standard.slab_styles,
        |s| s.style.id.clone(),
        |slab_style, source| CombinedSlabStyleEntry { slab_style, source },
    );
    for e in &mut entries {
        e.slab_style.common_mut().origin = e.source.into();
    }
    entries
}

/// Builds a combined structural slab-style list. Project wins, then session, then Standard.
pub fn combined_slab_structural_style_entries(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> Vec<CombinedSlabStructuralStyleEntry> {
    combined_slab_structural_style_entries_with_session(project, None)
}

/// Like [`combined_slab_structural_style_entries`], with a session overlay after project
/// and before Standard.
pub fn combined_slab_structural_style_entries_with_session(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    session: Option<&StyleLibrary>,
) -> Vec<CombinedSlabStructuralStyleEntry> {
    let standard = load_or_seed();
    let mut entries = merge_layers(
        project.map(|p| p.material_wall_style_library.slab_structural_styles.as_slice()),
        session.map(|s| s.slab_structural_styles.as_slice()),
        standard.slab_structural_styles,
        |s| s.style.id.clone(),
        |structural_style, source| CombinedSlabStructuralStyleEntry { structural_style, source },
    );
    for e in &mut entries {
        e.structural_style.common_mut().origin = e.source.into();
    }
    entries
}

/// Builds a combined floor finish style list. Project wins, then session, then Standard.
pub fn combined_floor_finish_style_entries(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> Vec<CombinedFloorFinishStyleEntry> {
    combined_floor_finish_style_entries_with_session(project, None)
}

/// Like [`combined_floor_finish_style_entries`], with a session overlay after project
/// and before Standard.
pub fn combined_floor_finish_style_entries_with_session(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    session: Option<&StyleLibrary>,
) -> Vec<CombinedFloorFinishStyleEntry> {
    let standard = load_or_seed();
    let mut entries = merge_layers(
        project.map(|p| p.material_wall_style_library.floor_finish_styles.as_slice()),
        session.map(|s| s.floor_finish_styles.as_slice()),
        standard.floor_finish_styles,
        |s| s.style.id.clone(),
        |finish_style, source| CombinedFloorFinishStyleEntry { finish_style, source },
    );
    for e in &mut entries {
        e.finish_style.common_mut().origin = e.source.into();
    }
    entries
}

/// Builds a combined section-profile list. Project wins, then session, then Standard.
pub fn combined_section_profile_entries(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> Vec<CombinedSectionProfileEntry> {
    combined_section_profile_entries_with_session(project, None)
}

/// Like [`combined_section_profile_entries`], with a session overlay after project
/// and before Standard.
pub fn combined_section_profile_entries_with_session(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    session: Option<&StyleLibrary>,
) -> Vec<CombinedSectionProfileEntry> {
    let standard = load_or_seed();
    let mut entries = merge_layers(
        project.map(|p| p.material_wall_style_library.profiles.as_slice()),
        session.map(|s| s.profiles.as_slice()),
        standard.profiles,
        |s| s.style.id.clone(),
        |profile, source| CombinedSectionProfileEntry { profile, source },
    );
    for e in &mut entries {
        e.profile.style.origin = e.source.into();
    }
    entries
}

/// Merged Standard+Project style library for UI lookups (project wins on id).
pub fn combined_style_library(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
) -> StyleLibrary {
    combined_style_library_with_session(project, None)
}

/// Merged Project + Session + Standard library (project, then session, then Standard).
pub fn combined_style_library_with_session(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    session: Option<&StyleLibrary>,
) -> StyleLibrary {
    let mut lib = StyleLibrary::empty();
    for entry in combined_material_entries_with_session(project, session) {
        lib.upsert_material(entry.material);
    }
    for entry in combined_wall_style_entries_with_session(project, session) {
        lib.upsert_wall_style(entry.wall_style);
    }
    for entry in combined_opening_style_entries_with_session(project, session) {
        lib.upsert_opening_style(entry.opening_style);
    }
    for entry in combined_slab_style_entries_with_session(project, session) {
        lib.upsert_slab_style(entry.slab_style);
    }
    for entry in combined_slab_structural_style_entries_with_session(project, session) {
        lib.upsert_slab_structural_style(entry.structural_style);
    }
    for entry in combined_floor_finish_style_entries_with_session(project, session) {
        lib.upsert_floor_finish_style(entry.finish_style);
    }
    for entry in combined_section_profile_entries_with_session(project, session) {
        lib.upsert_section_profile(entry.profile);
    }
    lib
}

/// Source of a material id in the combined view, if present.
pub fn material_library_source(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    id: &str,
) -> Option<LibrarySource> {
    material_library_source_with_session(project, None, id)
}

pub fn material_library_source_with_session(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    session: Option<&StyleLibrary>,
    id: &str,
) -> Option<LibrarySource> {
    combined_material_entries_with_session(project, session)
        .into_iter()
        .find(|e| e.material.id == id)
        .map(|e| e.source)
}

/// Source of a wall-style id in the combined view, if present.
pub fn wall_style_library_source(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    id: &str,
) -> Option<LibrarySource> {
    wall_style_library_source_with_session(project, None, id)
}

pub fn wall_style_library_source_with_session(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    session: Option<&StyleLibrary>,
    id: &str,
) -> Option<LibrarySource> {
    combined_wall_style_entries_with_session(project, session)
        .into_iter()
        .find(|e| e.wall_style.style.id == id)
        .map(|e| e.source)
}

/// Source of an opening-style id in the combined view, if present.
pub fn opening_style_library_source(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    id: &str,
) -> Option<LibrarySource> {
    opening_style_library_source_with_session(project, None, id)
}

pub fn opening_style_library_source_with_session(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    session: Option<&StyleLibrary>,
    id: &str,
) -> Option<LibrarySource> {
    combined_opening_style_entries_with_session(project, session)
        .into_iter()
        .find(|e| e.opening_style.style.id == id)
        .map(|e| e.source)
}

/// Source of a slab-style id in the combined view, if present.
pub fn slab_style_library_source(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    id: &str,
) -> Option<LibrarySource> {
    slab_style_library_source_with_session(project, None, id)
}

pub fn slab_style_library_source_with_session(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    session: Option<&StyleLibrary>,
    id: &str,
) -> Option<LibrarySource> {
    combined_slab_style_entries_with_session(project, session)
        .into_iter()
        .find(|e| e.slab_style.style.id == id)
        .map(|e| e.source)
}

/// Source of a floor-finish-style id in the combined view, if present.
pub fn floor_finish_style_library_source(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    id: &str,
) -> Option<LibrarySource> {
    floor_finish_style_library_source_with_session(project, None, id)
}

pub fn floor_finish_style_library_source_with_session(
    project: Option<&crate::modules::aec::engine::project::ProjectFile>,
    session: Option<&StyleLibrary>,
    id: &str,
) -> Option<LibrarySource> {
    combined_floor_finish_style_entries_with_session(project, session)
        .into_iter()
        .find(|e| e.finish_style.style.id == id)
        .map(|e| e.source)
}

/// Checks whether copying `material` into `target` would collide.
pub fn material_copy_conflict(target: &StyleLibrary, material: &Material) -> CopyConflict {
    if let Some(existing) = target.materials.iter().find(|m| m.id == material.id) {
        if existing == material {
            CopyConflict::IdenticalAlreadyPresent
        } else {
            CopyConflict::DifferentContentCollision
        }
    } else {
        CopyConflict::None
    }
}

