//! `AEC_SLABSTYLEMANAGER` ribbon tool and manager dispatch.
//!
//! Form draft / layer-buffer helpers live here so `update.rs` stays thin and
//! the 2D/3D preview can bake geometry without the CAD viewport.

use std::collections::HashMap;

use iced::Task;

use crate::app::{AecModalKind, Message, OpenCADStudio};
use crate::modules::aec::engine::display_component::SlabComponentSlot;
use crate::modules::aec::engine::library::{combined_slab_style_entries_with_session, load_or_seed};
use crate::modules::aec::engine::slab_style::{LayerFunction, LayerValue, SlabStyle, SlabStyleLayer};
use crate::modules::aec::engine::slab_xdata::{layer_function_from_str, layer_function_to_str};
use crate::modules::aec::engine::style::Style;
use crate::modules::aec::state::{AecSlabLayerBuffer, AecSlabPreviewMode};
use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub const EDITABLE_SLOTS: &[SlabComponentSlot] = &[
    SlabComponentSlot::Contour2D,
    SlabComponentSlot::CeilingOutline2D,
    SlabComponentSlot::LayerHatch2D,
    SlabComponentSlot::OpeningContour2D,
    SlabComponentSlot::OpeningSymbol2D,
    SlabComponentSlot::Solid3D,
    SlabComponentSlot::SurfaceStyle3D,
];

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_SLABSTYLEMANAGER",
        label: "Slab Style Manager",
        icon: IconKind::Svg(include_bytes!(
            "../../../../assets/icons/aec/wall_style_manager.svg"
        )),
        event: ModuleEvent::Command("AEC_SLABSTYLEMANAGER".to_string()),
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["AEC_SLABSTYLEMANAGER"],
});

pub fn layer_to_buffer(layer: &SlabStyleLayer) -> AecSlabLayerBuffer {
    AecSlabLayerBuffer {
        material_id: layer.material_id.clone(),
        thickness: match &layer.thickness {
            LayerValue::Fixed(v) => format!("{:.1}", v * 100.0),
            LayerValue::Formula(f) => f.clone(),
        },
        function: layer_function_to_str(&layer.function),
        vertical_offset: match &layer.vertical_offset {
            LayerValue::Fixed(v) => format!("{:.1}", v * 100.0),
            LayerValue::Formula(f) => f.clone(),
        },
        layer_override: layer.layer_override.clone().unwrap_or_default(),
        hatch_override: layer.hatch_override.clone().unwrap_or_default(),
        role_tag: layer.role_tag.clone().unwrap_or_default(),
        layer_id: Some(layer.layer_id),
    }
}

pub fn buffer_to_layer(buf: &AecSlabLayerBuffer) -> Result<SlabStyleLayer, String> {
    if buf.material_id.trim().is_empty() {
        return Err("material is required".to_string());
    }
    let thickness = LayerValue::parse_cm_str(&buf.thickness);
    let vertical_offset = if buf.vertical_offset.trim().is_empty() {
        LayerValue::Fixed(0.0)
    } else {
        LayerValue::parse_cm_str(&buf.vertical_offset)
    };
    let function = layer_function_from_str(&buf.function);
    let layer_override = if buf.layer_override.trim().is_empty() {
        None
    } else {
        Some(buf.layer_override.trim().to_string())
    };
    let hatch_override = if buf.hatch_override.trim().is_empty() {
        None
    } else {
        Some(buf.hatch_override.trim().to_string())
    };
    let role_tag = if buf.role_tag.trim().is_empty() {
        None
    } else {
        Some(buf.role_tag.trim().to_string())
    };
    Ok(SlabStyleLayer {
        material_id: buf.material_id.clone(),
        thickness,
        function,
        vertical_offset,
        layer_override,
        hatch_override,
        role_tag,
        layer_id: buf.layer_id.unwrap_or_else(uuid::Uuid::new_v4),
    })
}

#[derive(Clone, Debug)]
pub struct SlabPreviewPath {
    pub points: Vec<(f64, f64)>,
    pub closed: bool,
    pub color: [f32; 4],
    pub line_width: f32,
    pub fill_color: Option<[f32; 4]>,
    pub dashed: bool,
    pub label: Option<(String, (f64, f64))>,
}

fn material_color(mat_name: &str, func: LayerFunction) -> [f32; 4] {
    let lower = mat_name.to_lowercase();
    if lower.contains("beton") || lower.contains("concrete") {
        [0.72, 0.74, 0.75, 0.85]
    } else if lower.contains("dämm") || lower.contains("insulation") || lower.contains("eps") || lower.contains("xps") || lower.contains("mineral") {
        [0.96, 0.88, 0.52, 0.85]
    } else if lower.contains("estrich") || lower.contains("screed") {
        [0.82, 0.82, 0.80, 0.85]
    } else if lower.contains("holz") || lower.contains("timber") || lower.contains("wood") || lower.contains("parkett") {
        [0.85, 0.68, 0.45, 0.85]
    } else if lower.contains("flies") || lower.contains("tile") || lower.contains("keramik") {
        [0.65, 0.78, 0.85, 0.85]
    } else if lower.contains("putz") || lower.contains("plaster") || lower.contains("gips") {
        [0.92, 0.92, 0.92, 0.85]
    } else if lower.contains("abdicht") || lower.contains("bitumen") {
        [0.35, 0.35, 0.38, 0.85]
    } else if lower.contains("kies") || lower.contains("gravel") {
        [0.60, 0.58, 0.55, 0.85]
    } else {
        match func {
            LayerFunction::Structural => [0.70, 0.72, 0.74, 0.85],
            LayerFunction::Insulation => [0.95, 0.88, 0.50, 0.85],
            LayerFunction::Finish => [0.85, 0.75, 0.60, 0.85],
            LayerFunction::Other(_) => [0.80, 0.80, 0.80, 0.85],
        }
    }
}

pub fn preview_paths_for_mode(
    layers: &[SlabStyleLayer],
    mode: AecSlabPreviewMode,
) -> Vec<SlabPreviewPath> {
    let mut paths = Vec::new();
    if layers.is_empty() {
        return paths;
    }

    match mode {
        AecSlabPreviewMode::CrossSection => {
            let width = 2.4;
            let hw = width * 0.5;
            let mut cur_y = 0.0;

            // Draw OKFF / Reference Datum line at Y = 0.0
            paths.push(SlabPreviewPath {
                points: vec![(-hw - 0.3, 0.0), (hw + 0.3, 0.0)],
                closed: false,
                color: [0.8, 0.2, 0.2, 0.9],
                line_width: 1.5,
                fill_color: None,
                dashed: true,
                label: Some(("OKFF ±0.00".to_string(), (hw + 0.35, 0.0))),
            });

            for layer in layers {
                let th = layer.thickness.as_fixed_or(0.10).max(0.005);
                let y_top = cur_y;
                let y_bot = cur_y - th;
                let col = material_color(&layer.material_id, layer.function.clone());

                // Layer box
                paths.push(SlabPreviewPath {
                    points: vec![
                        (-hw, y_top),
                        (hw, y_top),
                        (hw, y_bot),
                        (-hw, y_bot),
                    ],
                    closed: true,
                    color: [0.25, 0.28, 0.32, 1.0],
                    line_width: 1.0,
                    fill_color: Some(col),
                    dashed: false,
                    label: Some((
                        format!("{} ({:.1} cm)", layer.material_id, th * 100.0),
                        (0.0, (y_top + y_bot) * 0.5),
                    )),
                });

                cur_y = y_bot;
            }

            // Dimension bar on the left
            let total_th = -cur_y;
            paths.push(SlabPreviewPath {
                points: vec![(-hw - 0.15, 0.0), (-hw - 0.15, cur_y)],
                closed: false,
                color: [0.4, 0.4, 0.4, 0.8],
                line_width: 1.0,
                fill_color: None,
                dashed: false,
                label: Some((
                    format!("Σ = {:.1} cm", total_th * 100.0),
                    (-hw - 0.2, cur_y * 0.5),
                )),
            });
        }
        AecSlabPreviewMode::ReflectedCeilingPlan => {
            // Plan view: 4m x 3m slab with an opening
            let sx = 2.0;
            let sy = 1.5;

            // Outer perimeter
            paths.push(SlabPreviewPath {
                points: vec![
                    (-sx, -sy),
                    (sx, -sy),
                    (sx, sy),
                    (-sx, sy),
                ],
                closed: true,
                color: [0.15, 0.2, 0.3, 1.0],
                line_width: 2.0,
                fill_color: Some([0.94, 0.95, 0.97, 0.6]),
                dashed: false,
                label: None,
            });

            // Reflected ceiling finish dashed outline (Deckenspiegel)
            let inset = 0.15;
            paths.push(SlabPreviewPath {
                points: vec![
                    (-sx + inset, -sy + inset),
                    (sx - inset, -sy + inset),
                    (sx - inset, sy - inset),
                    (-sx + inset, sy - inset),
                ],
                closed: true,
                color: [0.45, 0.5, 0.6, 0.9],
                line_width: 1.2,
                fill_color: None,
                dashed: true,
                label: None,
            });

            // Associative Opening cutout (1.4m x 1.0m)
            let ox = 0.7;
            let oy = 0.5;
            paths.push(SlabPreviewPath {
                points: vec![
                    (-ox, -oy),
                    (ox, -oy),
                    (ox, oy),
                    (-ox, oy),
                ],
                closed: true,
                color: [0.8, 0.2, 0.2, 1.0],
                line_width: 1.5,
                fill_color: Some([1.0, 1.0, 1.0, 0.95]),
                dashed: false,
                label: Some(("DIN 1356 Opening".to_string(), (0.0, 0.0))),
            });

            // DIN 1356 diagonal cross (X) lines
            paths.push(SlabPreviewPath {
                points: vec![(-ox, -oy), (ox, oy)],
                closed: false,
                color: [0.8, 0.2, 0.2, 0.9],
                line_width: 1.0,
                fill_color: None,
                dashed: false,
                label: None,
            });
            paths.push(SlabPreviewPath {
                points: vec![(-ox, oy), (ox, -oy)],
                closed: false,
                color: [0.8, 0.2, 0.2, 0.9],
                line_width: 1.0,
                fill_color: None,
                dashed: false,
                label: None,
            });
        }
        AecSlabPreviewMode::Model3D => {
            // Isometric 3D multi-layer stack projection
            let sx = 1.8;
            let sy = 1.2;
            let ox = 0.6;
            let oy = 0.4;

            let cos30 = 0.8660254;
            let sin30 = 0.5;

            let project = |x: f64, y: f64, z: f64| -> (f64, f64) {
                (
                    (x - y) * cos30,
                    (x + y) * sin30 * 0.5 + z * 1.5,
                )
            };

            let mut cur_z = 0.0;

            for layer in layers {
                let th = layer.thickness.as_fixed_or(0.10).max(0.005);
                let z_top = cur_z;
                let z_bot = cur_z - th;
                let col = material_color(&layer.material_id, layer.function.clone());
                let side_col = [col[0] * 0.75, col[1] * 0.75, col[2] * 0.75, col[3]];
                let front_col = [col[0] * 0.88, col[1] * 0.88, col[2] * 0.88, col[3]];

                // Side faces of the layer (outer slab)
                // Front-Right Face: (sx, -sy) to (sx, sy)
                paths.push(SlabPreviewPath {
                    points: vec![
                        project(sx, -sy, z_top),
                        project(sx, sy, z_top),
                        project(sx, sy, z_bot),
                        project(sx, -sy, z_bot),
                    ],
                    closed: true,
                    color: [0.3, 0.3, 0.3, 1.0],
                    line_width: 1.0,
                    fill_color: Some(side_col),
                    dashed: false,
                    label: None,
                });

                // Front-Left Face: (-sx, -sy) to (sx, -sy)
                paths.push(SlabPreviewPath {
                    points: vec![
                        project(-sx, -sy, z_top),
                        project(sx, -sy, z_top),
                        project(sx, -sy, z_bot),
                        project(-sx, -sy, z_bot),
                    ],
                    closed: true,
                    color: [0.3, 0.3, 0.3, 1.0],
                    line_width: 1.0,
                    fill_color: Some(front_col),
                    dashed: false,
                    label: None,
                });

                // Top Face (for the topmost layer)
                if (z_top - 0.0).abs() < 1e-6 {
                    paths.push(SlabPreviewPath {
                        points: vec![
                            project(-sx, -sy, z_top),
                            project(sx, -sy, z_top),
                            project(sx, sy, z_top),
                            project(-sx, sy, z_top),
                        ],
                        closed: true,
                        color: [0.3, 0.3, 0.3, 1.0],
                        line_width: 1.2,
                        fill_color: Some(col),
                        dashed: false,
                    label: None,
                    });

                    // Opening hole on top face
                    paths.push(SlabPreviewPath {
                        points: vec![
                            project(-ox, -oy, z_top),
                            project(ox, -oy, z_top),
                            project(ox, oy, z_top),
                            project(-ox, oy, z_top),
                        ],
                        closed: true,
                        color: [0.8, 0.2, 0.2, 1.0],
                        line_width: 1.2,
                        fill_color: Some([0.2, 0.2, 0.2, 0.9]),
                        dashed: false,
                    label: None,
                    });
                }

                // Inner cutout faces for opening
                paths.push(SlabPreviewPath {
                    points: vec![
                        project(-ox, -oy, z_top),
                        project(ox, -oy, z_top),
                        project(ox, -oy, z_bot),
                        project(-ox, -oy, z_bot),
                    ],
                    closed: true,
                    color: [0.4, 0.4, 0.4, 1.0],
                    line_width: 0.8,
                    fill_color: Some([0.35, 0.35, 0.35, 0.95]),
                    dashed: false,
                    label: None,
                });

                paths.push(SlabPreviewPath {
                    points: vec![
                        project(ox, -oy, z_top),
                        project(ox, oy, z_top),
                        project(ox, oy, z_bot),
                        project(ox, -oy, z_bot),
                    ],
                    closed: true,
                    color: [0.4, 0.4, 0.4, 1.0],
                    line_width: 0.8,
                    fill_color: Some([0.25, 0.25, 0.25, 0.95]),
                    dashed: false,
                    label: None,
                });

                cur_z = z_bot;
            }
        }
    }

    paths
}

impl OpenCADStudio {
    pub(crate) fn aec_slab_style_manager_open(&mut self) -> Task<Message> {
        self.aec_refresh_combined_style_library();
        let lib = self.aec.aec_style_library.clone().unwrap_or_else(load_or_seed);
        let entries = combined_slab_style_entries_with_session(
            self.aec.aec_project_explorer_file.as_ref(),
            self.aec.aec_session_style_library.as_ref(),
        );

        let select_id = self
            .aec
            .aec_last_slab_style_id
            .clone()
            .or_else(|| entries.first().map(|e| e.slab_style.style.id.clone()))
            .or_else(|| lib.slab_styles.first().map(|s| s.style.id.clone()));

        self.aec.aec_slab_style_manager_filter.clear();
        self.aec.aec_slab_style_manager_editing_id = None;
        self.aec.aec_slab_style_manager_form_open = false;
        self.aec.aec_slab_style_manager_profile_selected = None;
        self.aec.aec_slab_style_manager_profile_slot_visibility.clear();
        self.aec.aec_slab_style_manager_profile_slot_overrides.clear();
        self.aec.aec_slab_style_manager_profile_editing_slot = None;

        if let Some(id) = select_id {
            let _ = self.aec_slab_style_manager_select(id);
        } else {
            let _ = self.aec_slab_style_manager_new();
        }

        self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::SlabStyleManager));
        Task::none()
    }

    pub(crate) fn aec_slab_style_manager_select(&mut self, id: String) -> Task<Message> {
        let entries = combined_slab_style_entries_with_session(
            self.aec.aec_project_explorer_file.as_ref(),
            self.aec.aec_session_style_library.as_ref(),
        );
        let style = entries
            .iter()
            .find(|e| e.slab_style.style.id == id)
            .map(|e| e.slab_style.clone())
            .or_else(|| {
                self.aec
                    .aec_style_library
                    .as_ref()
                    .and_then(|lib| lib.find_slab_style(&id).cloned())
            });

        if let Some(style) = style {
            self.aec.aec_slab_style_manager_selected = Some(style.style.id.clone());
            self.aec.aec_slab_style_manager_editing_id = Some(style.style.id.clone());
            self.aec.aec_last_slab_style_id = Some(style.style.id.clone());
            self.aec.aec_slab_style_manager_name = style.style.name.clone();
            self.aec.aec_slab_style_manager_parent = style.style.parent_style_id.clone();
            self.aec.aec_slab_style_manager_structural_style = style.structural_style_id.clone();
            self.aec.aec_slab_style_manager_finish_style = style.default_finish_style_id.clone();
            self.aec.aec_slab_style_manager_layers =
                style.layers.iter().map(layer_to_buffer).collect();
            self.aec.aec_slab_style_manager_form_open = true;
            self.aec.aec_slab_style_manager_drag_index = None;
            self.aec.aec_slab_style_manager_profile_selected = None;
        }

        Task::none()
    }

    pub(crate) fn aec_slab_style_manager_new(&mut self) -> Task<Message> {
        let first_struct = self
            .aec
            .aec_style_library
            .as_ref()
            .and_then(|l| l.slab_structural_styles.first().map(|s| s.style.id.clone()));
        let first_finish = self
            .aec
            .aec_style_library
            .as_ref()
            .and_then(|l| l.floor_finish_styles.first().map(|f| f.style.id.clone()));

        self.aec.aec_slab_style_manager_selected = None;
        self.aec.aec_slab_style_manager_editing_id = None;
        self.aec.aec_slab_style_manager_name = crate::t!("New Slab Style").to_string();
        self.aec.aec_slab_style_manager_parent = None;
        self.aec.aec_slab_style_manager_structural_style = first_struct;
        self.aec.aec_slab_style_manager_finish_style = first_finish;
        self.aec.aec_slab_style_manager_layers = Vec::new();
        self.aec.aec_slab_style_manager_form_open = true;
        self.aec.aec_slab_style_manager_drag_index = None;
        self.aec.aec_slab_style_manager_profile_selected = None;

        Task::none()
    }

    pub(crate) fn aec_slab_style_manager_duplicate(&mut self) -> Task<Message> {
        let current_layers = self.aec.aec_slab_style_manager_layers.clone();
        let current_name = self.aec.aec_slab_style_manager_name.clone();
        let current_parent = self.aec.aec_slab_style_manager_parent.clone();
        let current_structural = self.aec.aec_slab_style_manager_structural_style.clone();
        let current_finish = self.aec.aec_slab_style_manager_finish_style.clone();

        self.aec.aec_slab_style_manager_selected = None;
        self.aec.aec_slab_style_manager_editing_id = None;
        self.aec.aec_slab_style_manager_name = format!("{current_name} (Kopie)");
        self.aec.aec_slab_style_manager_parent = current_parent;
        self.aec.aec_slab_style_manager_structural_style = current_structural;
        self.aec.aec_slab_style_manager_finish_style = current_finish;
        self.aec.aec_slab_style_manager_layers = current_layers
            .into_iter()
            .map(|mut l| {
                l.layer_id = Some(uuid::Uuid::new_v4());
                l
            })
            .collect();
        self.aec.aec_slab_style_manager_form_open = true;
        self.aec.aec_slab_style_manager_drag_index = None;

        Task::none()
    }

    pub(crate) fn aec_slab_style_manager_structural_style_changed(&mut self, id: Option<String>) {
        self.aec.aec_slab_style_manager_structural_style = id;
    }

    pub(crate) fn aec_slab_style_manager_finish_style_changed(&mut self, id: Option<String>) {
        self.aec.aec_slab_style_manager_finish_style = id;
    }

    pub(crate) fn aec_slab_style_manager_load_modular_layers(&mut self) {
        let mut dummy = SlabStyle::new("temp", "temp");
        dummy.structural_style_id = self.aec.aec_slab_style_manager_structural_style.clone();
        dummy.default_finish_style_id = self.aec.aec_slab_style_manager_finish_style.clone();

        let s_map: HashMap<String, _> = self
            .aec
            .aec_style_library
            .as_ref()
            .map(|l| l.slab_structural_styles.iter().map(|s| (s.style.id.clone(), s.clone())).collect())
            .unwrap_or_default();
        let f_map: HashMap<String, _> = self
            .aec
            .aec_style_library
            .as_ref()
            .map(|l| l.floor_finish_styles.iter().map(|f| (f.style.id.clone(), f.clone())).collect())
            .unwrap_or_default();

        let resolved = crate::modules::aec::engine::slab_style::resolve_slab_style_layers(
            &dummy,
            Some(&s_map),
            Some(&f_map),
        );
        if !resolved.is_empty() {
            self.aec.aec_slab_style_manager_layers = resolved.iter().map(layer_to_buffer).collect();
        }
    }

    pub(crate) fn aec_slab_style_manager_layer_add(&mut self) {
        let first_mat = self
            .aec
            .aec_style_library
            .as_ref()
            .and_then(|l| l.materials.first().map(|m| m.id.clone()))
            .unwrap_or_else(|| "Stahlbeton C25/30".to_string());

        self.aec
            .aec_slab_style_manager_layers
            .push(AecSlabLayerBuffer {
                material_id: first_mat,
                thickness: "5.0".to_string(),
                function: layer_function_to_str(&LayerFunction::Finish),
                vertical_offset: "0.0".to_string(),
                layer_override: String::new(),
                hatch_override: String::new(),
                role_tag: String::new(),
                layer_id: Some(uuid::Uuid::new_v4()),
            });
    }

    pub(crate) fn aec_slab_style_manager_layer_remove(&mut self, index: usize) {
        if index < self.aec.aec_slab_style_manager_layers.len() {
            self.aec.aec_slab_style_manager_layers.remove(index);
        }
    }

    pub(crate) fn aec_slab_style_manager_layer_move_up(&mut self, index: usize) {
        if index > 0 && index < self.aec.aec_slab_style_manager_layers.len() {
            self.aec
                .aec_slab_style_manager_layers
                .swap(index, index - 1);
        }
    }

    pub(crate) fn aec_slab_style_manager_layer_move_down(&mut self, index: usize) {
        if index + 1 < self.aec.aec_slab_style_manager_layers.len() {
            self.aec
                .aec_slab_style_manager_layers
                .swap(index, index + 1);
        }
    }

    pub(crate) fn aec_slab_style_manager_save_internal(&mut self) -> Result<SlabStyle, String> {
        let name = self.aec.aec_slab_style_manager_name.trim().to_string();
        if name.is_empty() {
            return Err("Slab style name cannot be empty".to_string());
        }

        let id = self
            .aec
            .aec_slab_style_manager_editing_id
            .clone()
            .unwrap_or_else(|| {
                name.to_lowercase()
                    .replace(' ', "_")
                    .replace('/', "_")
                    .replace('\\', "_")
            });

        let slab_style = SlabStyle {
            style: Style {
                id: id.clone(),
                name: name.clone(),
                object_kind: "Slab".to_string(),
                parent_style_id: self.aec.aec_slab_style_manager_parent.clone(),
            },
            structural_style_id: self.aec.aec_slab_style_manager_structural_style.clone(),
            default_finish_style_id: self.aec.aec_slab_style_manager_finish_style.clone(),
            layers: Vec::new(),
            display_profiles: HashMap::new(),
        };

        // Self-parent validation
        if slab_style.style.parent_style_id.as_deref() == Some(id.as_str()) {
            return Err("Style cannot be its own parent".to_string());
        }

        // Validate cycle in hierarchy
        if let Some(parent_id) = &slab_style.style.parent_style_id {
            if let Some(lib) = &self.aec.aec_style_library {
                let mut visited = vec![id.clone()];
                let mut curr = Some(parent_id.clone());
                while let Some(c) = curr {
                    if visited.contains(&c) {
                        return Err(format!("Inheritance cycle detected involving '{c}'"));
                    }
                    visited.push(c.clone());
                    curr = lib.find_slab_style(&c).and_then(|s| s.style.parent_style_id.clone());
                }
            }
        }

        // Upsert into project or session
        let has_project = self.aec.aec_project_explorer_file.is_some();
        if has_project {
            self.aec_upsert_slab_style_into_project(slab_style.clone())?;
        } else {
            self.aec_upsert_slab_style_into_session(slab_style.clone());
        }

        self.aec.aec_slab_style_manager_selected = Some(id.clone());
        self.aec.aec_slab_style_manager_editing_id = Some(id.clone());
        self.aec.aec_last_slab_style_id = Some(id);

        Ok(slab_style)
    }

    pub(crate) fn aec_slab_style_manager_save(&mut self) -> Task<Message> {
        match self.aec_slab_style_manager_save_internal() {
            Ok(style) => {
                self.command_line.push_info(
                    crate::tf!("AEC Slab Style Manager: Saved style '{}'.", style.style.name).as_ref(),
                );
            }
            Err(e) => {
                self.command_line.push_error(
                    crate::tf!("AEC Slab Style Manager: Failed to save: {e}").as_ref(),
                );
            }
        }
        Task::none()
    }

    pub(crate) fn aec_slab_style_manager_save_and_apply(&mut self) -> Task<Message> {
        match self.aec_slab_style_manager_save_internal() {
            Ok(style) => {
                // Trigger dynamic slab regeneration for slabs using this style,
                // honoring the active DisplayConfig / representation mode.
                let tab = self.active_tab;
                let slab_handles: Vec<acadrust::Handle> = self.tabs.get(tab).map_or_else(
                    Vec::new,
                    |tab_data| {
                        tab_data
                            .scene
                            .document
                            .entities()
                            .filter_map(|e| {
                                let handle = e.common().handle;
                                let slab =
                                    crate::modules::aec::engine::slab_xdata::slab_from_entity(e)?;
                                if slab.style_id == style.style.id {
                                    Some(handle)
                                } else {
                                    None
                                }
                            })
                            .collect()
                    },
                );

                let mut count = 0;
                for handle in slab_handles {
                    if self.regenerate_slab_respecting_active_display_config(tab, handle) {
                        count += 1;
                    }
                }

                if count > 0 {
                    if let Some(tab_data) = self.tabs.get_mut(tab) {
                        tab_data.dirty = true;
                    }
                    self.command_line.push_info(
                        crate::tf!(
                            "AEC Slab Style Manager: Saved and updated {count} slab(s)."
                        )
                        .as_ref(),
                    );
                } else {
                    self.command_line.push_info(
                        crate::tf!("AEC Slab Style Manager: Saved style '{}'.", style.style.name)
                            .as_ref(),
                    );
                }
            }
            Err(e) => {
                self.command_line.push_error(
                    crate::tf!("AEC Slab Style Manager: Failed to save: {e}").as_ref(),
                );
            }
        }
        Task::none()
    }

    pub(crate) fn aec_slab_style_manager_delete(&mut self) -> Task<Message> {
        let Some(selected_id) = self.aec.aec_slab_style_manager_selected.clone() else {
            return Task::none();
        };

        let has_project = self.aec.aec_project_explorer_file.is_some();
        if has_project {
            if let Some(project) = self.aec.aec_project_explorer_file.as_mut() {
                project.material_wall_style_library.remove_slab_style(&selected_id);
                if let Some(path) = self.aec.aec_project_explorer_path.clone() {
                    let lib = project.material_wall_style_library.clone();
                    let _ = crate::modules::aec::engine::project::save_style_library_to_project(
                        project, &path, lib,
                    );
                }
            }
        } else if let Some(lib) = self.tabs[self.active_tab].aec_session_style_library_mut() {
            lib.remove_slab_style(&selected_id);
        }

        self.aec_refresh_combined_style_library();
        self.command_line.push_info(
            crate::tf!("AEC Slab Style Manager: Deleted style '{selected_id}'.").as_ref(),
        );

        self.aec.aec_slab_style_manager_selected = None;
        self.aec.aec_slab_style_manager_editing_id = None;
        self.aec.aec_slab_style_manager_form_open = false;

        let entries = combined_slab_style_entries_with_session(
            self.aec.aec_project_explorer_file.as_ref(),
            self.aec.aec_session_style_library.as_ref(),
        );
        if let Some(first) = entries.first() {
            let _ = self.aec_slab_style_manager_select(first.slab_style.style.id.clone());
        }

        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_buffer_roundtrip() {
        let layer = SlabStyleLayer {
            material_id: "Stahlbeton C25/30".to_string(),
            thickness: LayerValue::Fixed(0.20),
            function: LayerFunction::Structural,
            vertical_offset: LayerValue::Fixed(0.0),
            layer_override: Some("STR_SLAB".to_string()),
            hatch_override: Some("AR-CONC".to_string()),
            role_tag: Some("LoadBearing".to_string()),
            layer_id: uuid::Uuid::new_v4(),
        };

        let buf = layer_to_buffer(&layer);
        assert_eq!(buf.material_id, "Stahlbeton C25/30");
        assert_eq!(buf.thickness, "20.0");
        assert_eq!(buf.function, "Structural");
        assert_eq!(buf.layer_override, "STR_SLAB");

        let back = buffer_to_layer(&buf).expect("buffer to layer");
        assert_eq!(back.material_id, layer.material_id);
        assert_eq!(back.thickness, layer.thickness);
        assert_eq!(back.function, layer.function);
        assert_eq!(back.layer_override, layer.layer_override);
    }

    #[test]
    fn test_preview_paths_generation() {
        let layers = vec![
            SlabStyleLayer {
                material_id: "Fliesen".to_string(),
                thickness: LayerValue::Fixed(0.015),
                function: LayerFunction::Finish,
                vertical_offset: LayerValue::Fixed(0.0),
                layer_override: None,
                hatch_override: None,
                role_tag: None,
                layer_id: uuid::Uuid::new_v4(),
            },
            SlabStyleLayer {
                material_id: "Estrich".to_string(),
                thickness: LayerValue::Fixed(0.05),
                function: LayerFunction::Other("Estrich".to_string()),
                vertical_offset: LayerValue::Fixed(0.0),
                layer_override: None,
                hatch_override: None,
                role_tag: None,
                layer_id: uuid::Uuid::new_v4(),
            },
            SlabStyleLayer {
                material_id: "Stahlbeton".to_string(),
                thickness: LayerValue::Fixed(0.20),
                function: LayerFunction::Structural,
                vertical_offset: LayerValue::Fixed(0.0),
                layer_override: None,
                hatch_override: None,
                role_tag: None,
                layer_id: uuid::Uuid::new_v4(),
            },
        ];

        let cs_paths = preview_paths_for_mode(&layers, AecSlabPreviewMode::CrossSection);
        assert!(!cs_paths.is_empty());
        assert!(cs_paths.len() >= 4); // datum + 3 layers + dimension

        let rcp_paths = preview_paths_for_mode(&layers, AecSlabPreviewMode::ReflectedCeilingPlan);
        assert!(!rcp_paths.is_empty());

        let m3d_paths = preview_paths_for_mode(&layers, AecSlabPreviewMode::Model3D);
        assert!(!m3d_paths.is_empty());
    }
}
