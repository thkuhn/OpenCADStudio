//! `AEC_MATERIAL` / `AEC_MATERIALMANAGER`.

use crate::command::{CadCommand, CmdResult};
use crate::modules::aec::commands::unique_id;
use crate::modules::aec::engine;
use crate::modules::aec::engine::library::load_or_seed;
use crate::modules::aec::engine::material::Material;
use glam::DVec3;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::ui::command_line::CommandLine;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_MATERIALMANAGER",
        label: "Material Manager",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/aec/material_manager.svg")),
        event: ModuleEvent::Command("AEC_MATERIALMANAGER".to_string()),
    }
}

/// Step of an in-progress `AEC_MATERIAL` command.
enum MaterialStep {
    Name,
    Hatch { name: String },
    Color { name: String, hatch: String },
    LineType { name: String, hatch: String, color: u32 },
}

/// `AEC_MATERIAL` — create (or update) a material in the AEC style library,
/// prompting step by step for name, hatch pattern, line color and line type.
pub struct MaterialCommand {
    step: MaterialStep,
}

impl MaterialCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            step: MaterialStep::Name,
        }
    }
}

impl CadCommand for MaterialCommand {
    fn name(&self) -> &'static str {
        "AEC_MATERIAL"
    }

    fn prompt(&self) -> String {
        match &self.step {
            MaterialStep::Name => crate::tr!("aec", "material-prompt-name"),
            MaterialStep::Hatch { .. } => crate::tr!("aec", "material-prompt-hatch"),
            MaterialStep::Color { .. } => crate::tr!("aec", "material-prompt-color"),
            MaterialStep::LineType { .. } => crate::tr!("aec", "material-prompt-linetype"),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        match &self.step {
            MaterialStep::Name => {
                if t.is_empty() {
                    // A material needs a name; keep prompting.
                    return Some(CmdResult::NeedPoint);
                }
                self.step = MaterialStep::Hatch {
                    name: t.to_string(),
                };
                Some(CmdResult::NeedPoint)
            }
            MaterialStep::Hatch { name } => {
                let hatch = if t.is_empty() {
                    "ANSI31".to_string()
                } else {
                    t.to_string()
                };
                self.step = MaterialStep::Color {
                    name: name.clone(),
                    hatch,
                };
                Some(CmdResult::NeedPoint)
            }
            MaterialStep::Color { name, hatch } => {
                let color = u32::from_str_radix(t.trim_start_matches('#'), 16).unwrap_or(0);
                self.step = MaterialStep::LineType {
                    name: name.clone(),
                    hatch: hatch.clone(),
                    color,
                };
                Some(CmdResult::NeedPoint)
            }
            MaterialStep::LineType { name, hatch, color } => {
                let line_type = if t.is_empty() {
                    "Continuous".to_string()
                } else {
                    t.to_string()
                };
                Some(CmdResult::Dispatch(format!(
                    "AEC_MATERIAL_ADD {name}|{hatch}|{color:06X}|{line_type}"
                )))
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").unwrap_or(CmdResult::Cancel)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// `AEC_MATERIAL_ADD name|hatch|color_hex|line_type` — the non-interactive
/// handler `MaterialCommand` dispatches to once all fields are collected;
/// upserts the material (by name → stable id) into the style library and
/// persists it.
pub fn aec_material_add(command_line: &mut CommandLine, args: &str) {
    let parts: Vec<&str> = args.split('|').collect();
    let [name, hatch, color_hex, line_type] = parts.as_slice() else {
        command_line.push_error(&crate::tr!("aec", "material-malformed"));
        return;
    };
    let color = u32::from_str_radix(color_hex, 16).unwrap_or(0);

    let mut lib = load_or_seed();
    // Re-use the id of an existing material with the same name so re-running
    // AEC_MATERIAL_ADD on the same name still updates it in place; only a
    // genuinely new name gets a fresh, globally unique id (see `unique_id`).
    let id = lib
        .materials
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case(name))
        .map(|m| m.id.clone())
        .unwrap_or_else(|| unique_id("mat", name));
    lib.upsert_material(Material::new(
        id,
        name.to_string(),
        hatch.to_string(),
        color,
        line_type.to_string(),
    ));
    match engine::library::save_to_default_path(&lib) {
        Ok(()) => command_line.push_info(&crate::tr!(
            "aec",
            "material-saved",
            name = name.to_string(),
            hatch = hatch.to_string(),
            color = format!("{color:06X}"),
            line_type = line_type.to_string()
        )),
        Err(e) => command_line.push_error(&crate::tr!(
            "aec",
            "material-save-failed",
            error = e.to_string()
        )),
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["AEC_MATERIAL", "AEC_MATERIALMANAGER", "AEC_MATERIAL_ADD"],
});
