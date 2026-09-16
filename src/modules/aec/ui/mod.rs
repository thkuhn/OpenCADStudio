//! AEC modal windows (formerly `src/ui/window/aec_*.rs`).

pub mod aec_junction_editor;
pub mod aec_material_manager;
pub mod aec_plan_manager;
pub mod aec_project_explorer;
pub mod aec_storey_settings;
pub mod aec_style_picker;
pub(crate) mod aec_ui_util;
pub mod aec_wall_style_manager;

pub use aec_ui_util::{acad_color_to_editor_string, editor_string_to_acad_color};
