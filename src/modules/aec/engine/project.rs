//! `.ocsproj` project file — Building → Storey drawing map.
//!
//! A drawing remains fully usable standalone: missing project files load as
//! an empty default project rather than erroring.
//!
//! Step 6 of `.junie/plans/aec-plan-view-display-variants.md` ("Projektweite
//! Bibliotheks-Persistenz") extends this file to also carry the
//! project-wide material/wall-style and `DisplayConfig` libraries, so all
//! drawing files (storeys) referenced by the same project see the exact
//! same library entries instead of each drawing keeping its own copy. The
//! previous per-machine global library files (`aec_styles.toml`,
//! `aec_display_configs.toml`, see `library.rs`) remain as the fallback for
//! drawings that are not (yet) part of a project.

use crate::modules::aec::engine::control_plane::{
    default_floor_ceiling, ControlPlane, intersect_vertical_at_xy,
};
use crate::modules::aec::engine::library::{DisplayConfigLibrary, StyleLibrary};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;
use uuid::Uuid;

/// Generates a unique id for a new `Building`/`StoreyRef`.
///
/// Names are intentionally allowed to collide (the user may legitimately
/// have two storeys named "EG" in different buildings, or want to rename a
/// building to match another one) — the `id` is what unambiguously
/// identifies an element for rename/delete/select operations, independent
/// of its current position in the list or its display name.
///
/// A random (v4) UUID is used so ids stay globally unique even across
/// different machines/processes (e.g. project files merged/copied between
/// users), unlike a simple process-local counter.
fn new_entity_id() -> Uuid {
    Uuid::new_v4()
}

/// Top-level OpenCADStudio project (`.ocsproj` JSON).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectFile {
    pub buildings: Vec<Building>,
    /// Project-wide material/wall-style library. `#[serde(default)]` so
    /// `.ocsproj` files saved before Step 6 (without this field) still
    /// deserialize successfully as an empty library.
    #[serde(default)]
    pub material_wall_style_library: StyleLibrary,
    /// Project-wide `DisplayConfig` ("Plan") library. `#[serde(default)]`
    /// for the same old-format-compatibility reason as above.
    #[serde(default)]
    pub display_config_library: DisplayConfigLibrary,
    /// Absolute height of finished floor level 0 / EG above NN (metres). Optional.
    #[serde(default)]
    pub ffl0_nn_m: Option<f64>,
}

/// One building containing ordered storey references.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Building {
    /// Stable identity, unrelated to `name` — see `new_entity_id()`.
    pub id: Uuid,
    pub name: String,
    pub storeys: Vec<StoreyRef>,
}

impl Building {
    /// Creates a new building with a fresh unique `id`.
    pub fn new(name: impl Into<String>) -> Self {
        Building {
            id: new_entity_id(),
            name: name.into(),
            storeys: Vec::new(),
        }
    }

    /// Finds the index of the storey with the given `id`, if present.
    pub fn storey_index(&self, id: Uuid) -> Option<usize> {
        self.storeys.iter().position(|s| s.id == id)
    }
}

/// Reference to a storey drawing within a building.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoreyRef {
    /// Stable identity, unrelated to `name` — see `new_entity_id()`.
    pub id: Uuid,
    pub name: String,
    /// Cached floor Z at origin; kept for old `.ocsproj` files. Prefer
    /// [`StoreyRef::derived_elevation`].
    #[serde(default)]
    pub elevation: f64,
    /// Cached floor-to-floor height; default 3.0 for old files without the field.
    #[serde(default = "default_storey_height")]
    pub height: f64,
    pub drawing_path: String,
    #[serde(default)]
    pub control_planes: Vec<ControlPlane>,
    #[serde(default = "Uuid::nil")]
    pub floor_plane_id: Uuid,
    #[serde(default = "Uuid::nil")]
    pub ceiling_plane_id: Uuid,
}

fn default_storey_height() -> f64 {
    3.0
}

impl StoreyRef {
    /// Creates a new storey reference with a fresh unique `id` and default
    /// floor/ceiling planes.
    pub fn new(name: impl Into<String>, elevation: f64, drawing_path: impl Into<String>) -> Self {
        Self::new_with_height(name, elevation, default_storey_height(), drawing_path)
    }

    pub fn new_with_height(
        name: impl Into<String>,
        elevation: f64,
        height: f64,
        drawing_path: impl Into<String>,
    ) -> Self {
        let name = name.into();
        let (floor, ceiling) = default_floor_ceiling(&name, elevation, height);
        let floor_plane_id = floor.id;
        let ceiling_plane_id = ceiling.id;
        StoreyRef {
            id: new_entity_id(),
            name,
            elevation,
            height,
            drawing_path: drawing_path.into(),
            control_planes: vec![floor, ceiling],
            floor_plane_id,
            ceiling_plane_id,
        }
    }

    /// Ensure floor/ceiling planes exist (migration for old `.ocsproj`).
    pub fn ensure_control_planes(&mut self) {
        if self.control_planes.is_empty() {
            let (floor, ceiling) =
                default_floor_ceiling(&self.name, self.elevation, self.height.max(0.01));
            self.floor_plane_id = floor.id;
            self.ceiling_plane_id = ceiling.id;
            self.control_planes = vec![floor, ceiling];
            return;
        }
        if self.plane(self.floor_plane_id).is_none() {
            self.floor_plane_id = self.control_planes[0].id;
        }
        if self.plane(self.ceiling_plane_id).is_none() {
            if self.control_planes.len() > 1 {
                self.ceiling_plane_id = self.control_planes[1].id;
            } else {
                let extra = ControlPlane::horizontal(
                    format!("{}_OKGH", self.name),
                    self.elevation + self.height.max(0.01),
                );
                self.ceiling_plane_id = extra.id;
                self.control_planes.push(extra);
            }
        }
        self.sync_derived_elevation_height();
    }

    pub fn plane(&self, id: Uuid) -> Option<&ControlPlane> {
        self.control_planes.iter().find(|p| p.id == id)
    }

    pub fn plane_mut(&mut self, id: Uuid) -> Option<&mut ControlPlane> {
        self.control_planes.iter_mut().find(|p| p.id == id)
    }

    pub fn derived_elevation(&self) -> f64 {
        self.plane(self.floor_plane_id)
            .and_then(|p| intersect_vertical_at_xy(p.origin[0], p.origin[1], p).map(|h| h[2]))
            .or_else(|| self.plane(self.floor_plane_id).map(|p| p.origin[2]))
            .unwrap_or(self.elevation)
    }

    pub fn derived_height(&self) -> f64 {
        self.height.max(0.01)
    }

    pub fn sync_derived_elevation_height(&mut self) {
        self.elevation = self.derived_elevation();
    }

    /// Main plane Z becomes `z`; other planes keep their ΔZ relative to it.
    /// Storey height is unchanged and does not move any plane.
    pub fn set_elevation(&mut self, z: f64) {
        let dz = z - self.derived_elevation();
        if dz.abs() < 1e-12 {
            return;
        }
        for plane in &mut self.control_planes {
            plane.origin[2] += dz;
        }
        self.sync_derived_elevation_height();
    }

    /// Storey height is a numeric property only; control planes are not moved.
    /// `height <= 0` is a no-op.
    pub fn set_height(&mut self, height: f64) {
        if !(height > 0.0) {
            return;
        }
        self.height = height;
    }

    pub fn add_control_plane(&mut self, plane: ControlPlane) {
        self.control_planes.push(plane);
    }

    /// World-Z of one plane's origin; floor/OKGH caches follow if that plane is a role.
    /// Moving the floor plane is [`set_elevation`] so relative offsets of other planes stay.
    pub fn set_plane_origin_z(&mut self, id: Uuid, z: f64) -> bool {
        if id == self.floor_plane_id {
            self.set_elevation(z);
            return self.plane(id).is_some();
        }
        if let Some(p) = self.plane_mut(id) {
            p.origin[2] = z;
            self.sync_derived_elevation_height();
            true
        } else {
            false
        }
    }

    /// Vertical offset of `id` relative to the main (floor) control plane.
    pub fn plane_z_relative_to_floor(&self, id: Uuid) -> Option<f64> {
        let floor_z = self.derived_elevation();
        self.plane(id).map(|p| p.origin[2] - floor_z)
    }

    /// Set a non-floor plane by ΔZ from the main control plane. Floor is a no-op.
    pub fn set_plane_z_relative_to_floor(&mut self, id: Uuid, rel: f64) -> bool {
        if id == self.floor_plane_id {
            return false;
        }
        let floor_z = self.derived_elevation();
        self.set_plane_origin_z(id, floor_z + rel)
    }
}

impl ProjectFile {
    /// Finds the index of the building with the given `id`, if present.
    pub fn building_index(&self, id: Uuid) -> Option<usize> {
        self.buildings.iter().position(|b| b.id == id)
    }

    /// Control plane by id in any building/storey.
    pub fn control_plane(&self, id: Uuid) -> Option<&ControlPlane> {
        self.buildings
            .iter()
            .flat_map(|b| b.storeys.iter())
            .find_map(|s| s.plane(id))
    }

    /// Load a project from `path`. A missing file yields `Ok(ProjectFile::default())`
    /// so drawings work without an accompanying project.
    pub fn load(path: &Path) -> io::Result<ProjectFile> {
        match fs::read_to_string(path) {
            Ok(text) => {
                let mut project: ProjectFile = serde_json::from_str(&text)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                crate::modules::aec::engine::library::migrate_display_profiles_into_planarts(
                    &mut project.display_config_library.configs,
                    &mut project.material_wall_style_library.wall_styles,
                );
                for building in &mut project.buildings {
                    for storey in &mut building.storeys {
                        storey.ensure_control_planes();
                    }
                }
                Ok(project)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(ProjectFile::default()),
            Err(e) => Err(e),
        }
    }

    /// Write this project as pretty-printed JSON to `path`.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(path, text)
    }
}

/// Returns `true` if a `StyleLibrary` has no materials and no wall styles.
fn style_library_is_empty(lib: &StyleLibrary) -> bool {
    lib.materials.is_empty() && lib.wall_styles.is_empty()
}

/// Resolves the effective material/wall-style library: "project overrides
/// global". If `project` is given and its embedded
/// `material_wall_style_library` is non-empty, a clone of it is returned;
/// otherwise this falls back to the existing machine-wide global library
/// (`crate::modules::aec::engine::library::load_or_seed`), preserving
/// today's standalone-drawing behavior.
pub fn resolve_style_library(project: Option<&ProjectFile>) -> StyleLibrary {
    if let Some(project) = project {
        if !style_library_is_empty(&project.material_wall_style_library) {
            return project.material_wall_style_library.clone();
        }
    }
    crate::modules::aec::engine::library::load_or_seed()
}

/// Resolves the effective `DisplayConfig` library: "project overrides
/// global", analogous to [`resolve_style_library`].
pub fn resolve_display_config_library(project: Option<&ProjectFile>) -> DisplayConfigLibrary {
    if let Some(project) = project {
        if !project.display_config_library.configs.is_empty() {
            return project.display_config_library.clone();
        }
    }
    crate::modules::aec::engine::library::load_or_seed_display_config_library()
}

/// Writes `lib` into `project`'s embedded material/wall-style library and
/// persists `project` to `project_path`. Since every drawing file (storey)
/// referencing the same `.ocsproj` re-reads it via [`resolve_style_library`],
/// this single write is what fans a library change out to all referencing
/// files.
pub fn save_style_library_to_project(
    project: &mut ProjectFile,
    project_path: &Path,
    lib: StyleLibrary,
) -> io::Result<()> {
    project.material_wall_style_library = lib;
    project.save(project_path)
}

/// Writes `lib` into `project`'s embedded `DisplayConfig` library and
/// persists `project` to `project_path`, analogous to
/// [`save_style_library_to_project`].
pub fn save_display_config_library_to_project(
    project: &mut ProjectFile,
    project_path: &Path,
    lib: DisplayConfigLibrary,
) -> io::Result<()> {
    project.display_config_library = lib;
    project.save(project_path)
}

/// Migrates the current machine-wide global libraries
/// (`load_or_seed`/`load_or_seed_display_config_library`) into `project`'s
/// embedded library fields, for a drawing that used to rely on the global,
/// file-bound library and now wants a shared, project-wide one.
///
/// Design decision (lossless migrate, no silent overwrite): if the
/// project already has a *non-empty* embedded library, this is a no-op for
/// that library (the already-populated project library is considered
/// authoritative and is never silently replaced). If the project's embedded
/// library is empty, it is populated with a copy of the current global
/// library. Either way the source global library on disk is left untouched
/// (this function has no side effect on the global files), so no data is
/// ever lost on either side.
pub fn migrate_file_library_to_project(project: &mut ProjectFile) {
    if style_library_is_empty(&project.material_wall_style_library) {
        project.material_wall_style_library = crate::modules::aec::engine::library::load_or_seed();
    }
    if project.display_config_library.configs.is_empty() {
        project.display_config_library =
            crate::modules::aec::engine::library::load_or_seed_display_config_library();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("opencadstudio_ocsproj_{name}_{nanos}.ocsproj"))
    }

    #[test]
    fn round_trip_multi_building_multi_storey() {
        let mut building_a = Building::new("Building A");
        building_a
            .storeys
            .push(StoreyRef::new("GF", 0.0, "a_gf.dwg"));
        building_a
            .storeys
            .push(StoreyRef::new("L1", 3.2, "a_l1.dwg"));
        let mut building_b = Building::new("Building B");
        building_b
            .storeys
            .push(StoreyRef::new("Basement", -3.0, "b_b1.dwg"));
        let project = ProjectFile {
            buildings: vec![building_a, building_b],
            ..ProjectFile::default()
        };
        let path = temp_path("roundtrip");
        project.save(&path).expect("save");
        let loaded = ProjectFile::load(&path).expect("load");
        assert_eq!(loaded, project);
        let _ = fs::remove_file(&path);
    }

    fn seeded_style_library() -> StyleLibrary {
        crate::modules::aec::engine::library::seed_default_library()
    }

    fn seeded_display_config_library() -> DisplayConfigLibrary {
        use crate::modules::aec::engine::plan_view::{DisplayConfig, PlanningStage, ViewType};
        let mut lib = DisplayConfigLibrary::empty();
        lib.upsert(DisplayConfig::new(
            "Architekt 1:50".to_string(),
            "Architektur".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        ));
        lib
    }

    #[test]
    fn round_trip_with_project_libraries() {
        let project = ProjectFile {
            buildings: vec![Building::new("Building A")],
            material_wall_style_library: seeded_style_library(),
            display_config_library: seeded_display_config_library(),
            ..ProjectFile::default()
        };
        let path = temp_path("roundtrip_with_libraries");
        project.save(&path).expect("save");
        let loaded = ProjectFile::load(&path).expect("load");
        assert_eq!(loaded, project);
        assert!(!loaded.material_wall_style_library.materials.is_empty());
        assert!(!loaded.display_config_library.configs.is_empty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn project_display_config_library_scale_mappings_still_load() {
        use crate::modules::aec::engine::plan_view::ScaleDisplayConfigMapping;

        let mut project = ProjectFile::default();
        project.display_config_library = seeded_display_config_library();
        project
            .display_config_library
            .scale_display_config_mappings
            .push(ScaleDisplayConfigMapping {
                scale_name: "1:50".to_string(),
                display_config_name: "Architekt 1:50".to_string(),
            });

        let path = temp_path("project_scale_mappings_roundtrip");
        project.save(&path).expect("save");
        let loaded = ProjectFile::load(&path).expect("load");
        assert_eq!(
            loaded
                .display_config_library
                .scale_display_config_mappings
                .len(),
            1
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn old_format_project_without_library_fields_deserializes_with_empty_libraries() {
        let json = r#"{"buildings": []}"#;
        let project: ProjectFile =
            serde_json::from_str(json).expect("old-format project must still deserialize");
        assert!(project.material_wall_style_library.materials.is_empty());
        assert!(project.material_wall_style_library.wall_styles.is_empty());
        assert!(project.display_config_library.configs.is_empty());
    }

    /// Regression test: `.ocsproj` files saved before `DisplayConfig` gained
    /// its `planning_stage`/`view_type` fields (see
    /// `aec-display-fixes-planning-stage.md`) must still load instead of
    /// failing the whole `ProjectFile` deserialization, which previously
    /// left the project silently unloaded (required fields with no
    /// `#[serde(default)]`).
    #[test]
    fn project_with_pre_planning_stage_display_configs_still_loads() {
        let json = r#"{
            "buildings": [],
            "display_config_library": {
                "configs": [
                    {
                        "name": "Architekt 1:50",
                        "discipline": "Architektur"
                    }
                ]
            }
        }"#;
        let project: ProjectFile = serde_json::from_str(json)
            .expect("pre-planning_stage/view_type DisplayConfig entries must still deserialize");
        assert_eq!(project.display_config_library.configs.len(), 1);
        let cfg = &project.display_config_library.configs[0];
        assert_eq!(cfg.name, "Architekt 1:50");
        assert_eq!(
            cfg.planning_stage,
            crate::modules::aec::engine::plan_view::PlanningStage::Design
        );
        assert_eq!(
            cfg.view_type,
            crate::modules::aec::engine::plan_view::ViewType::FloorPlan
        );
    }

    #[test]
    fn multiple_storey_files_of_same_project_see_identical_display_configs() {
        let mut project = ProjectFile::default();
        project.display_config_library = seeded_display_config_library();
        let path = temp_path("shared_display_configs");
        project.save(&path).expect("save");

        // Simulate two different drawing files (storeys) referencing the
        // same project by loading it independently, twice.
        let loaded_a = ProjectFile::load(&path).expect("load a");
        let loaded_b = ProjectFile::load(&path).expect("load b");
        let resolved_a = resolve_display_config_library(Some(&loaded_a));
        let resolved_b = resolve_display_config_library(Some(&loaded_b));
        assert!(!resolved_a.configs.is_empty());
        assert_eq!(resolved_a, resolved_b);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn change_in_project_style_library_is_visible_to_a_fresh_reload() {
        let mut project = ProjectFile::default();
        let path = temp_path("style_library_change_propagates");
        project.save(&path).expect("initial save");

        let updated_lib = seeded_style_library();
        save_style_library_to_project(&mut project, &path, updated_lib.clone())
            .expect("save updated library");

        // A second "drawing file" reloading the same project sees the change.
        let reloaded = ProjectFile::load(&path).expect("reload");
        assert_eq!(reloaded.material_wall_style_library, updated_lib);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn migration_populates_empty_project_libraries_from_global_without_mutating_global() {
        let mut project = ProjectFile::default();
        assert!(style_library_is_empty(&project.material_wall_style_library));
        assert!(project.display_config_library.configs.is_empty());

        let global_style_before = crate::modules::aec::engine::library::load_or_seed();
        let global_display_before =
            crate::modules::aec::engine::library::load_or_seed_display_config_library();

        migrate_file_library_to_project(&mut project);

        assert_eq!(project.material_wall_style_library, global_style_before);
        assert_eq!(project.display_config_library, global_display_before);

        // Migration must not mutate the global libraries themselves.
        let global_style_after = crate::modules::aec::engine::library::load_or_seed();
        let global_display_after =
            crate::modules::aec::engine::library::load_or_seed_display_config_library();
        assert_eq!(global_style_before, global_style_after);
        assert_eq!(global_display_before, global_display_after);
    }

    #[test]
    fn migration_is_a_no_op_when_project_display_config_library_already_populated() {
        let mut project = ProjectFile::default();
        project.display_config_library = seeded_display_config_library();
        let existing = project.display_config_library.clone();

        migrate_file_library_to_project(&mut project);

        assert_eq!(
            project.display_config_library, existing,
            "an already-populated project display-config library must not be silently replaced"
        );
    }

    #[test]
    fn load_missing_project_returns_default() {
        let path = temp_path("missing_should_not_exist");
        let _ = fs::remove_file(&path);
        let loaded = ProjectFile::load(&path).expect("missing is ok");
        assert_eq!(loaded, ProjectFile::default());
        assert!(loaded.buildings.is_empty());
    }

    #[test]
    fn ids_are_unique_and_independent_of_name() {
        // Same name is allowed for two buildings/storeys, but their ids must differ.
        let b1 = Building::new("Erdgeschoss");
        let b2 = Building::new("Erdgeschoss");
        assert_ne!(b1.id, b2.id);
        let s1 = StoreyRef::new("EG", 0.0, "a.dwg");
        let s2 = StoreyRef::new("EG", 0.0, "b.dwg");
        assert_ne!(s1.id, s2.id);
    }

    #[test]
    fn lookup_by_id_finds_correct_index_even_after_reorder() {
        let mut project = ProjectFile::default();
        project.buildings.push(Building::new("A"));
        project.buildings.push(Building::new("B"));
        let id_b = project.buildings[1].id;
        // Simulate a reorder/removal shifting indices.
        project.buildings.remove(0);
        assert_eq!(project.building_index(id_b), Some(0));

        let mut building = Building::new("Haus");
        building.storeys.push(StoreyRef::new("EG", 0.0, "eg.dwg"));
        building.storeys.push(StoreyRef::new("OG1", 3.0, "og1.dwg"));
        let id_og1 = building.storeys[1].id;
        building.storeys.remove(0);
        assert_eq!(building.storey_index(id_og1), Some(0));
    }

    #[test]
    fn old_storey_without_control_planes_migrates_floor_and_ceiling() {
        let json = r#"{
            "buildings": [{
                "id": "11111111-1111-1111-1111-111111111111",
                "name": "Haus",
                "storeys": [{
                    "id": "22222222-2222-2222-2222-222222222222",
                    "name": "EG",
                    "elevation": 1.5,
                    "drawing_path": "eg.dwg"
                }]
            }]
        }"#;
        let path = temp_path("migrate_planes");
        fs::write(&path, json).expect("write");
        let loaded = ProjectFile::load(&path).expect("load");
        let storey = &loaded.buildings[0].storeys[0];
        assert_eq!(storey.control_planes.len(), 2);
        assert!(storey.plane(storey.floor_plane_id).is_some());
        assert!(storey.plane(storey.ceiling_plane_id).is_some());
        assert!((storey.derived_elevation() - 1.5).abs() < 1e-9);
        assert!((storey.derived_height() - 3.0).abs() < 1e-9);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn set_elevation_translates_all_planes_and_keeps_height() {
        let mut s = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        s.add_control_plane(ControlPlane::horizontal("extra", 1.5));
        s.set_elevation(3.0);
        assert!((s.derived_elevation() - 3.0).abs() < 1e-9);
        assert!((s.derived_height() - 3.0).abs() < 1e-9);
        let top = s.plane(s.ceiling_plane_id).unwrap();
        assert!((top.origin[2] - 6.0).abs() < 1e-9);
        let extra = s.control_planes.iter().find(|p| p.name == "extra").unwrap();
        assert!((extra.origin[2] - 4.5).abs() < 1e-9);
    }

    #[test]
    fn set_height_does_not_move_planes() {
        let mut s = StoreyRef::new_with_height("EG", 1.0, 3.0, "eg.dwg");
        let extra = ControlPlane::horizontal("UKRD", 2.8);
        let extra_id = extra.id;
        s.add_control_plane(extra);
        let top_z = s.plane(s.ceiling_plane_id).unwrap().origin[2];
        s.set_height(4.0);
        assert!((s.derived_elevation() - 1.0).abs() < 1e-9);
        assert!((s.derived_height() - 4.0).abs() < 1e-9);
        assert!((s.plane(s.ceiling_plane_id).unwrap().origin[2] - top_z).abs() < 1e-9);
        assert!((s.plane(extra_id).unwrap().origin[2] - 2.8).abs() < 1e-9);
    }

    #[test]
    fn set_height_non_positive_is_noop() {
        let mut s = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        s.set_height(0.0);
        s.set_height(-1.0);
        assert!((s.derived_height() - 3.0).abs() < 1e-9);
        assert!((s.plane(s.ceiling_plane_id).unwrap().origin[2] - 3.0).abs() < 1e-9);
    }

    #[test]
    fn default_top_plane_is_okgh_not_ukrd() {
        let s = StoreyRef::new("EG", 0.0, "eg.dwg");
        let top = s.plane(s.ceiling_plane_id).unwrap();
        assert!(top.name.ends_with("_OKGH"));
        assert!(!top.name.contains("UKRD"));
    }

    #[test]
    fn set_plane_origin_z_does_not_move_other_planes() {
        let mut s = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        let extra = ControlPlane::horizontal("UKRD", 2.8);
        let id = extra.id;
        s.add_control_plane(extra);
        assert!(s.set_plane_origin_z(id, 2.65));
        assert!((s.plane(id).unwrap().origin[2] - 2.65).abs() < 1e-9);
        assert!((s.derived_elevation() - 0.0).abs() < 1e-9);
        assert!((s.derived_height() - 3.0).abs() < 1e-9);
        assert!((s.plane(s.ceiling_plane_id).unwrap().origin[2] - 3.0).abs() < 1e-9);
    }

    #[test]
    fn floor_origin_z_translates_other_planes() {
        let mut s = StoreyRef::new_with_height("EG", 0.0, 3.0, "eg.dwg");
        let extra = ControlPlane::horizontal("UKRD", 2.8);
        let id = extra.id;
        s.add_control_plane(extra);
        assert!(s.set_plane_origin_z(s.floor_plane_id, 1.0));
        assert!((s.derived_elevation() - 1.0).abs() < 1e-9);
        assert!((s.plane(id).unwrap().origin[2] - 3.8).abs() < 1e-9);
        assert!((s.plane_z_relative_to_floor(id).unwrap() - 2.8).abs() < 1e-9);
    }

    #[test]
    fn relative_z_is_from_floor() {
        let mut s = StoreyRef::new_with_height("EG", 1.0, 3.0, "eg.dwg");
        let ceil = s.ceiling_plane_id;
        assert!((s.plane_z_relative_to_floor(ceil).unwrap() - 3.0).abs() < 1e-9);
        assert!(s.set_plane_z_relative_to_floor(ceil, 2.7));
        assert!((s.plane(ceil).unwrap().origin[2] - 3.7).abs() < 1e-9);
        assert!(!s.set_plane_z_relative_to_floor(s.floor_plane_id, 5.0));
        assert!((s.derived_elevation() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn ffl0_nn_round_trips() {
        let mut project = ProjectFile::default();
        project.ffl0_nn_m = Some(112.4);
        let path = temp_path("ffl0_nn");
        project.save(&path).expect("save");
        let loaded = ProjectFile::load(&path).expect("load");
        assert_eq!(loaded.ffl0_nn_m, Some(112.4));
        let _ = fs::remove_file(&path);
    }
}
