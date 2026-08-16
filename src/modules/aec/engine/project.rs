//! `.ocsproj` project file — Building → Storey drawing map.
//!
//! A drawing remains fully usable standalone: missing project files load as
//! an empty default project rather than erroring.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Generates a unique id for a new `Building`/`StoreyRef`.
///
/// Names are intentionally allowed to collide (the user may legitimately
/// have two storeys named "EG" in different buildings, or want to rename a
/// building to match another one) — the `id` is what unambiguously
/// identifies an element for rename/delete/select operations, independent
/// of its current position in the list or its display name.
///
/// Seeded from the current time on first use so ids stay unique-enough
/// across process restarts, then simply incremented for the rest of the
/// process lifetime.
fn new_entity_id() -> u64 {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    if NEXT_ID.load(Ordering::Relaxed) == 0 {
        use std::time::{SystemTime, UNIX_EPOCH};
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1)
            .max(1);
        // If another thread already seeded it, this just loses the race
        // harmlessly (both values are equally valid, unique-enough seeds).
        let _ = NEXT_ID.compare_exchange(0, seed, Ordering::Relaxed, Ordering::Relaxed);
    }
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Top-level OpenCADStudio project (`.ocsproj` JSON).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectFile {
    pub buildings: Vec<Building>,
}

/// One building containing ordered storey references.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Building {
    /// Stable identity, unrelated to `name` — see `new_entity_id()`.
    pub id: u64,
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
    pub fn storey_index(&self, id: u64) -> Option<usize> {
        self.storeys.iter().position(|s| s.id == id)
    }
}

/// Reference to a storey drawing within a building.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoreyRef {
    /// Stable identity, unrelated to `name` — see `new_entity_id()`.
    pub id: u64,
    pub name: String,
    pub elevation: f64,
    pub drawing_path: String,
}

impl StoreyRef {
    /// Creates a new storey reference with a fresh unique `id`.
    pub fn new(name: impl Into<String>, elevation: f64, drawing_path: impl Into<String>) -> Self {
        StoreyRef {
            id: new_entity_id(),
            name: name.into(),
            elevation,
            drawing_path: drawing_path.into(),
        }
    }
}

impl ProjectFile {
    /// Finds the index of the building with the given `id`, if present.
    pub fn building_index(&self, id: u64) -> Option<usize> {
        self.buildings.iter().position(|b| b.id == id)
    }

    /// Load a project from `path`. A missing file yields `Ok(ProjectFile::default())`
    /// so drawings work without an accompanying project.
    pub fn load(path: &Path) -> io::Result<ProjectFile> {
        match fs::read_to_string(path) {
            Ok(text) => {
                let project: ProjectFile = serde_json::from_str(&text)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
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
        };
        let path = temp_path("roundtrip");
        project.save(&path).expect("save");
        let loaded = ProjectFile::load(&path).expect("load");
        assert_eq!(loaded, project);
        let _ = fs::remove_file(&path);
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
}
