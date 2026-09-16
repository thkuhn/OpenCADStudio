//! `AEC_STYLEMANAGER` / `AEC_STYLE` command.

use crate::command::{CadCommand, CmdResult};
use crate::modules::aec::commands::{layer_function_to_str, parse_layer_function, slugify, unique_id};
use crate::modules::aec::walls::WallCommand;
use uuid::Uuid;
use crate::modules::aec::engine::library::load_or_seed;
use crate::modules::aec::engine::style::Style;
use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, LayerValue, WallStyle};
use crate::modules::aec::engine;
use glam::DVec3;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::ui::command_line::CommandLine;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_STYLEMANAGER",
        label: "Wall Style Manager",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/aec/wall_style_manager.svg")),
        event: ModuleEvent::Command("AEC_STYLEMANAGER".to_string()),
    }
}

/// Step of an in-progress `AEC_STYLE` command.
enum StyleStep {
    Name,
    Parent {
        name: String,
    },
    /// Collecting layers; `layers` accumulates `(material_name, thickness, function)`.
    LayerMaterial {
        name: String,
        parent: Option<String>,
        layers: Vec<(String, f64, LayerFunction)>,
    },
    LayerThickness {
        name: String,
        parent: Option<String>,
        layers: Vec<(String, f64, LayerFunction)>,
        material: String,
    },
    LayerFunctionStep {
        name: String,
        parent: Option<String>,
        layers: Vec<(String, f64, LayerFunction)>,
        material: String,
        thickness: f64,
    },
}

pub struct StyleCommand {
    step: StyleStep,
}

impl StyleCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            step: StyleStep::Name,
        }
    }

    fn finish(name: &str, parent: &Option<String>, layers: &[(String, f64, LayerFunction)]) -> CmdResult {
        let parent_part = parent.clone().unwrap_or_default();
        let layers_part = layers
            .iter()
            .map(|(mat, thick, func)| format!("{mat}:{thick}:{}", layer_function_to_str(func)))
            .collect::<Vec<_>>()
            .join(";");
        CmdResult::Dispatch(format!("AEC_STYLE_ADD {name}|{parent_part}|{layers_part}"))
    }
}

impl CadCommand for StyleCommand {
    fn name(&self) -> &'static str {
        "AEC_STYLE"
    }

    fn prompt(&self) -> String {
        match &self.step {
            StyleStep::Name => crate::tr!("aec", "style-prompt-name"),
            StyleStep::Parent { .. } => crate::tr!("aec", "style-prompt-parent"),
            StyleStep::LayerMaterial { layers, .. } => crate::tr!(
                "aec",
                "style-prompt-layer-material",
                n = { (layers.len() + 1) as i32 }
            ),
            StyleStep::LayerThickness { material, .. } => crate::tr!(
                "aec",
                "style-prompt-layer-thickness",
                material = material.as_str()
            ),
            StyleStep::LayerFunctionStep { material, .. } => crate::tr!(
                "aec",
                "style-prompt-layer-function",
                material = material.as_str()
            ),
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
            StyleStep::Name => {
                if t.is_empty() {
                    return Some(CmdResult::NeedPoint);
                }
                self.step = StyleStep::Parent {
                    name: t.to_string(),
                };
                Some(CmdResult::NeedPoint)
            }
            StyleStep::Parent { name } => {
                let parent = if t.is_empty() {
                    None
                } else {
                    Some(t.to_string())
                };
                self.step = StyleStep::LayerMaterial {
                    name: name.clone(),
                    parent,
                    layers: Vec::new(),
                };
                Some(CmdResult::NeedPoint)
            }
            StyleStep::LayerMaterial { name, parent, layers } => {
                if t.is_empty() {
                    // No (more) layers: finish, possibly inheriting layers from
                    // the parent style if none were entered here.
                    return Some(Self::finish(name, parent, layers));
                }
                self.step = StyleStep::LayerThickness {
                    name: name.clone(),
                    parent: parent.clone(),
                    layers: layers.clone(),
                    material: t.to_string(),
                };
                Some(CmdResult::NeedPoint)
            }
            StyleStep::LayerThickness {
                name,
                parent,
                layers,
                material,
            } => {
                let thickness = WallCommand::parse_dimension(t, 0.2);
                self.step = StyleStep::LayerFunctionStep {
                    name: name.clone(),
                    parent: parent.clone(),
                    layers: layers.clone(),
                    material: material.clone(),
                    thickness,
                };
                Some(CmdResult::NeedPoint)
            }
            StyleStep::LayerFunctionStep {
                name,
                parent,
                layers,
                material,
                thickness,
            } => {
                let function = parse_layer_function(t);
                let mut layers = layers.clone();
                layers.push((material.clone(), *thickness, function));
                self.step = StyleStep::LayerMaterial {
                    name: name.clone(),
                    parent: parent.clone(),
                    layers,
                };
                Some(CmdResult::NeedPoint)
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

/// `AEC_STYLE_ADD name|parent|mat1:thick1:func1;mat2:thick2:func2...` — the
/// non-interactive handler `StyleCommand` dispatches to once all fields are
/// collected; upserts the wall style (by name → stable id) into the style
/// library and persists it. An empty layer list inherits layers from the
/// parent style at resolution time (see `effective_layers`).
pub fn aec_style_add(command_line: &mut CommandLine, args: &str) {
    let mut parts = args.splitn(3, '|');
    let (Some(name), Some(parent_raw), Some(layers_raw)) =
        (parts.next(), parts.next(), parts.next())
    else {
        command_line.push_error(&crate::tr!("aec", "style-malformed"));
        return;
    };

    let mut lib = load_or_seed();

    let parent_style_id = if parent_raw.is_empty() {
        None
    } else {
        match lib
            .wall_styles
            .iter()
            .find(|s| s.style.name.eq_ignore_ascii_case(parent_raw))
        {
            Some(p) => Some(p.style.id.clone()),
            None => {
                command_line.push_error(&crate::tr!(
                    "aec",
                    "style-unknown-parent",
                    parent = parent_raw
                ));
                None
            }
        }
    };

    let mut layers = Vec::new();
    if !layers_raw.is_empty() {
        for entry in layers_raw.split(';') {
            // `material:thickness:function[:role_tag]`. The role tag is an
            // optional, purely informational 4th field (e.g. "Tragschale").
            let fields: Vec<&str> = entry.splitn(4, ':').collect();
            if fields.len() < 3 {
                continue;
            }
            let mat_name = fields[0];
            let thick_str = fields[1];
            let func_str = fields[2];
            let role_tag = fields.get(3).filter(|s| !s.is_empty()).map(|s| s.to_string());
            let material_id = lib
                .materials
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case(mat_name))
                .map(|m| m.id.clone())
                .unwrap_or_else(|| format!("mat_{}", slugify(mat_name)));
            let thickness = LayerValue::parse_str(thick_str);
            layers.push(Layer {
                material_id,
                thickness,
                function: parse_layer_function(func_str),
                axis_offset: LayerValue::Fixed(0.0),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag,
                layer_id: Uuid::new_v4(),
            });
        }
    }

    // Re-use the id of an existing wall style with the same name so
    // re-running AEC_STYLE_ADD on the same name still updates it in place;
    // only a genuinely new name gets a fresh, globally unique id.
    let id = lib
        .wall_styles
        .iter()
        .find(|s| s.style.name.eq_ignore_ascii_case(name))
        .map(|s| s.style.id.clone())
        .unwrap_or_else(|| unique_id("style", name));
    lib.upsert_wall_style(WallStyle {
        style: Style {
            id,
            name: name.to_string(),
            object_kind: "Wall".to_string(),
            parent_style_id,
        },
        layers,
    display_profiles: std::collections::HashMap::new(),
    });

    match engine::library::save_to_default_path(&lib) {
        Ok(()) => {
            let count = lib.wall_styles
                .iter()
                .find(|s| s.style.name == name)
                .map(|s| s.layers.len())
                .unwrap_or(0);
            command_line.push_info(&crate::tr!(
                "aec",
                "style-saved",
                name = name,
                count = count
            ));
        }
        Err(e) => command_line.push_error(&crate::tr!(
            "aec",
            "style-save-failed",
            error = e.to_string()
        )),
    }
}


inventory::submit!(crate::command::CommandRegistration {
    names: &["AEC_STYLE", "AEC_STYLEMANAGER", "AEC_STYLE_ADD"],
});
