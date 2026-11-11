use super::*;

/// Standard preset ids stored in `DisplayConfig::preset`.
pub const PRESET_ENTWURF_100: &str = "entwurf-100";
pub const PRESET_NORMAL: &str = "normal";
pub const PRESET_WERKPLAN_50: &str = "werkplan-50";
pub const PRESET_STATIK_50: &str = "statik-50";
pub const PRESET_FASSADE: &str = "fassade-100";
pub const PRESET_MODEL_3D: &str = "model-3d";
pub const PRESET_DECKENSPIEGEL: &str = "deckenspiegel";

/// A collection of [`DisplayConfig`]s ("Plans"), persisted analogously to
/// [`StyleLibrary`] — see Step 5 of
/// `.junie/plans/aec-plan-view-display-variants.md`. Deliberately a
/// separate library/file (not folded into `StyleLibrary`) since
/// `DisplayConfig`s are a distinct kind of library entry with their own
/// lifecycle, and keeping them apart avoids growing every wall-style/
/// material save into a `DisplayConfig` roundtrip too.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DisplayConfigLibrary {
    /// List of available display configurations ("Plans").
    #[serde(default)]
    pub configs: Vec<DisplayConfig>,
    /// Legacy scale→DisplayConfig mapping table. Kept for deserialization of
    /// older project files; unused by the UI/runtime (feature removed).
    #[serde(default)]
    pub scale_display_config_mappings: Vec<ScaleDisplayConfigMapping>,
}

impl DisplayConfigLibrary {
    /// An empty library (no display configs, no scale mappings).
    pub fn empty() -> Self {
        Self {
            configs: Vec::new(),
            scale_display_config_mappings: Vec::new(),
        }
    }

    /// Seed default discipline configurations: Architektur 1:50, Statik 1:50, Entwurf 1:100, Ansicht Fassade.
    pub fn seed_default_configs() -> Self {
        use crate::modules::aec::engine::opening_style::OpeningLevelOverride;
        use crate::modules::aec::engine::plan_view::{PlanningStage, ViewType};

        let mut arch_50 = DisplayConfig::new(
            "Architektur 1:50".to_string(),
            "Architektur".to_string(),
            PlanningStage::Execution,
            ViewType::FloorPlan,
        );
        arch_50.scale = Some(50.0);
        arch_50.preset = Some(PRESET_WERKPLAN_50.to_string());
        arch_50.detail.representation = RepresentationMode::TwoD;
        arch_50.opening_levels = OpeningLevelOverride::detail();

        let mut normal = DisplayConfig::new(
            "Architektur 1:100".to_string(),
            "Architektur".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        normal.scale = Some(100.0);
        normal.preset = Some(PRESET_NORMAL.to_string());
        normal.detail.representation = RepresentationMode::TwoD;
        normal.opening_levels = OpeningLevelOverride::normal();

        let mut stat_50 = DisplayConfig::new(
            "Statik 1:50".to_string(),
            "Statik".to_string(),
            PlanningStage::Execution,
            ViewType::FloorPlan,
        );
        stat_50.scale = Some(50.0);
        stat_50.preset = Some(PRESET_STATIK_50.to_string());
        stat_50.detail.representation = RepresentationMode::TwoD;
        stat_50.opening_levels = OpeningLevelOverride::structural();

        let mut entw_100 = DisplayConfig::new(
            "Entwurf 1:100".to_string(),
            "Architektur".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        entw_100.scale = Some(100.0);
        entw_100.preset = Some(PRESET_ENTWURF_100.to_string());
        entw_100.detail.hatches = false;
        entw_100.detail.representation = RepresentationMode::TwoD;
        entw_100.opening_levels = OpeningLevelOverride::coarse();

        let mut elev = DisplayConfig::new(
            "Ansicht Fassade".to_string(),
            "Architektur".to_string(),
            PlanningStage::Design,
            ViewType::Elevation,
        );
        elev.preset = Some(PRESET_FASSADE.to_string());
        elev.detail.representation = RepresentationMode::TwoD;
        elev.opening_levels = OpeningLevelOverride::elevation();

        let mut model_3d = DisplayConfig::new(
            "3D Modell / Visualisierung".to_string(),
            "Visualisierung".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        model_3d.preset = Some(PRESET_MODEL_3D.to_string());
        model_3d.detail.representation = RepresentationMode::ThreeD;
        model_3d.opening_levels = OpeningLevelOverride::model_3d();

        let mut rcp = DisplayConfig::new(
            "Deckenspiegel".to_string(),
            "Architektur".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        rcp.scale = Some(50.0);
        rcp.preset = Some(PRESET_DECKENSPIEGEL.to_string());
        rcp.detail = DetailProfile {
            layers: LayerDetail::Off,
            hatches: false,
            ceiling_view: true,
            representation: RepresentationMode::TwoD,
            ..DetailProfile::default()
        };
        rcp.opening_levels = OpeningLevelOverride::coarse();

        Self {
            configs: vec![arch_50, normal, stat_50, entw_100, elev, model_3d, rcp],
            scale_display_config_mappings: Vec::new(),
        }
    }

    /// Inserts or replaces (by `name`) a display config.
    pub fn upsert(&mut self, config: DisplayConfig) {
        if let Some(existing) = self.configs.iter_mut().find(|c| c.name == config.name) {
            *existing = config;
        } else {
            self.configs.push(config);
        }
    }

    /// Removes a display config by `name`. Returns `true` if one was removed.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.configs.len();
        self.configs.retain(|c| c.name != name);
        self.configs.len() != before
    }

    /// Looks up a display config by `name`.
    pub fn find(&self, name: &str) -> Option<&DisplayConfig> {
        self.configs.iter().find(|c| c.name == name)
    }
}

/// Serializes the display-config library to a string (JSON, see [`to_toml`]
/// for the rationale of the misleadingly-named-but-consistent function).
pub fn display_config_library_to_toml(lib: &DisplayConfigLibrary) -> Result<String, String> {
    serde_json::to_string_pretty(lib).map_err(|e| e.to_string())
}

/// Deserializes the display-config library from a string.
pub fn display_config_library_from_toml(s: &str) -> Result<DisplayConfigLibrary, String> {
    let mut lib: DisplayConfigLibrary = serde_json::from_str(s).map_err(|e| e.to_string())?;
    for config in &mut lib.configs {
        config.apply_detail_defaults_if_unset();
        config.apply_preset_defaults_if_unset();
    }
    Ok(lib)
}

/// Returns the default path for the AEC display-config ("Plan") library
/// file. Reuses the application's standard configuration directory.
pub fn default_display_config_library_path() -> PathBuf {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    {
        test_library_dir().join("aec_display_configs.toml")
    }
    #[cfg(not(all(test, not(target_arch = "wasm32"))))]
    {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(p) = crate::config::config_dir() {
                return p.join("aec_display_configs.toml");
            }
        }

        let mut p = PathBuf::new();
        #[cfg(all(not(target_arch = "wasm32"), target_os = "linux"))]
        {
            if let Ok(home) = std::env::var("HOME") {
                p.push(home);
                p.push(".config");
            }
        }
        p.push("OpenCADStudio");
        p.push("aec_display_configs.toml");
        p
    }
}

/// Persists `lib` to [`default_display_config_library_path`], creating
/// parent directories as needed. On `wasm32` this is a no-op.
pub fn save_display_config_library_to_default_path(lib: &DisplayConfigLibrary) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = default_display_config_library_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = display_config_library_to_toml(lib)?;
        return std::fs::write(&path, content).map_err(|e| e.to_string());
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = lib;
        Ok(())
    }
}

/// Loads the display-config library from
/// [`default_display_config_library_path`], or — if it does not exist yet —
/// returns an empty library (no seed data: unlike materials/wall styles,
/// `DisplayConfig`s have no sensible non-empty default without a project's
/// own wall styles to reference). On `wasm32` this always returns an empty
/// in-memory library.
pub fn load_or_seed_display_config_library() -> DisplayConfigLibrary {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = default_display_config_library_path();
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(lib) = display_config_library_from_toml(&content) {
                    if !lib.configs.is_empty() {
                        return lib;
                    }
                }
            }
        }
    }
    let seed = DisplayConfigLibrary::seed_default_configs();
    let _ = save_display_config_library_to_default_path(&seed);
    seed
}

