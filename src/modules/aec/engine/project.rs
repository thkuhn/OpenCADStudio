//! `.ocsproj` project file — Building → Storey drawing map.
//!
//! A drawing remains fully usable standalone: missing project files load as
//! an empty default project rather than erroring.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

/// Top-level OpenCADStudio project (`.ocsproj` JSON).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectFile {
    pub buildings: Vec<Building>,
}

/// One building containing ordered storey references.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Building {
    pub name: String,
    pub storeys: Vec<StoreyRef>,
}

/// Reference to a storey drawing within a building.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoreyRef {
    pub name: String,
    pub elevation: f64,
    pub drawing_path: String,
}

impl ProjectFile {
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
        let project = ProjectFile {
            buildings: vec![
                Building {
                    name: "Building A".into(),
                    storeys: vec![
                        StoreyRef {
                            name: "GF".into(),
                            elevation: 0.0,
                            drawing_path: "a_gf.dwg".into(),
                        },
                        StoreyRef {
                            name: "L1".into(),
                            elevation: 3.2,
                            drawing_path: "a_l1.dwg".into(),
                        },
                    ],
                },
                Building {
                    name: "Building B".into(),
                    storeys: vec![StoreyRef {
                        name: "Basement".into(),
                        elevation: -3.0,
                        drawing_path: "b_b1.dwg".into(),
                    }],
                },
            ],
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
}
