use super::*;

/// Serializes the library to a string.
///
/// Chosen format: JSON (via `serde_json`) because the `toml` crate is not
/// present in Cargo.toml.
pub fn to_toml(lib: &StyleLibrary) -> Result<String, String> {
    serde_json::to_string_pretty(lib).map_err(|e| e.to_string())
}

/// Deserializes the library from a string.
///
/// Chosen format: JSON (via `serde_json`) because the `toml` crate is not
/// present in Cargo.toml.
pub fn from_toml(s: &str) -> Result<StyleLibrary, String> {
    let mut value: serde_json::Value = serde_json::from_str(s).map_err(|e| e.to_string())?;
    discard_unreadable_opening_styles(&mut value);
    serde_json::from_value(value).map_err(|e| e.to_string())
}

/// Removes individually unreadable opening styles without discarding the rest
/// of a style library or project document.
pub(crate) fn discard_unreadable_opening_styles(value: &mut serde_json::Value) -> bool {
    let Some(styles) = value
        .get_mut("opening_styles")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return false;
    };
    let original_len = styles.len();
    styles.retain(|style| serde_json::from_value::<OpeningStyle>(style.clone()).is_ok());
    styles.len() != original_len
}

/// Per-thread scratch dir for the on-disk style / display-config libraries
/// under `cfg(test)`. Cargo reuses worker threads, so this is one folder per
/// test thread — enough to stop parallel tests from clobbering a shared
/// `aec_styles.toml`. Sequential tests on the same thread still share the file.
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) fn test_library_dir() -> PathBuf {
    thread_local! {
        static DIR: PathBuf = {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(1);
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let p = std::env::temp_dir().join(format!(
                "ocs-test-aec-lib-{}-{n}",
                std::process::id()
            ));
            let _ = std::fs::create_dir_all(&p);
            p
        };
    }
    DIR.with(PathBuf::clone)
}

/// Returns the default path for the AEC styles library file.
///
/// Reuses the application's standard configuration directory. Tests get a
/// thread-local file so parallel `load_or_seed` / `save_to_default_path`
/// calls cannot overwrite each other.
pub fn default_library_path() -> PathBuf {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    {
        test_library_dir().join("aec_styles.toml")
    }
    #[cfg(not(all(test, not(target_arch = "wasm32"))))]
    {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(p) = crate::config::config_dir() {
                return p.join("aec_styles.toml");
            }
        }

        // Fallback if config_dir fails or on web
        let mut p = PathBuf::new();
        #[cfg(all(not(target_arch = "wasm32"), target_os = "linux"))]
        {
            if let Ok(home) = std::env::var("HOME") {
                p.push(home);
                p.push(".config");
            }
        }
        p.push("OpenCADStudio");
        p.push("aec_styles.toml");
        p
    }
}

/// Persists `lib` to [`default_library_path`], creating parent directories
/// as needed. On `wasm32` there is no local filesystem to persist to, so
/// this is a no-op there.
pub fn save_to_default_path(lib: &StyleLibrary) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = default_library_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = to_toml(lib)?;
        return std::fs::write(&path, content).map_err(|e| e.to_string());
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = lib;
        Ok(())
    }
}

/// Loads the library from [`default_library_path`], or — if it does not
/// exist yet — creates it from [`seed_default_library`] and persists it, so
/// `AEC_WALL`'s style selection always has something to offer without
/// requiring the user to define materials/styles first. On `wasm32` there
/// is no local filesystem, so this always returns an in-memory seed.
pub fn load_or_seed() -> StyleLibrary {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = default_library_path();
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(lib) = from_toml(&content) {
                    // Legacy files written before `Layer::layer_id` existed
                    // get a freshly-migrated set of stable IDs assigned
                    // in-memory (see `layers_from_serde`). Persist only then
                    // so subsequent loads keep those IDs — a plain read must
                    // not rewrite the file (parallel tests share no lock).
                    if library_source_missing_layer_ids(&content)
                        || library_has_unreadable_opening_styles(&content)
                    {
                        let _ = save_to_default_path(&lib);
                    }
                    return lib;
                }
            }
        }
    }
    let seed = seed_default_library();
    let _ = save_to_default_path(&seed);
    seed
}

pub(super) fn library_has_unreadable_opening_styles(content: &str) -> bool {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(content) else {
        return false;
    };
    discard_unreadable_opening_styles(&mut value)
}

/// True when any persisted layer is missing `layer_id` (the deserializer
/// fills those with a fresh UUID). Used to skip the write-on-every-read
/// that used to race parallel tests on the library file.
pub(super) fn library_source_missing_layer_ids(content: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return false;
    };
    let missing = |layers: &serde_json::Value| {
        layers
            .as_array()
            .is_some_and(|arr| arr.iter().any(|layer| layer.get("layer_id").is_none()))
    };
    ["wall_styles", "slab_styles", "slab_structural_styles", "floor_finish_styles"]
        .iter()
        .any(|key| {
            value
                .get(*key)
                .and_then(|v| v.as_array())
                .is_some_and(|arr| arr.iter().any(|style| missing(&style["layers"])))
        })
}

