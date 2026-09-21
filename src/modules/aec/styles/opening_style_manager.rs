//! `AEC_OPENINGSTYLEMANAGER` ribbon tool and manager dispatch.
//!
//! Form draft / slot-buffer helpers live here so `update.rs` stays thin and
//! the 2D preview can bake generator paths without the CAD viewport.

use std::collections::HashMap;

use iced::Task;

use crate::app::{AecModalKind, AecPendingCopy, Message, OpenCADStudio};
use crate::modules::aec::engine::display_component::OpeningComponentSlot;
use crate::modules::aec::engine::opening_display::{bake_opening_generators, BakedPath, OpeningBakeParams};
use crate::modules::aec::engine::opening_shape::{clamp_spring, OpeningShape};
use crate::modules::aec::engine::opening_sketch::{
    commit_draft, draft_append, draft_set_last_arc, snap_ref_point, TwoRectBake,
};
use crate::modules::aec::engine::opening_style::{
    default_slots_for_kind, BlockPlacementMode, OpeningDisplayProfile, OpeningGenerator,
    OpeningSketch, OpeningStyle, SlotGeometry, HingeSide, DEFAULT_FRAME_THICKNESS,
    DEFAULT_OPENING_ANGLE_DEG,
};
use crate::modules::aec::engine::openings::OpeningKind;
use crate::modules::aec::engine::style::Style;
use crate::modules::aec::state::{AecOpeningSlotBuffer, AecOpeningSlotSource};
use crate::modules::{IconKind, ModuleEvent, ToolDef};

/// Plan-preview wall thickness used in the manager canvas (drawing units).
pub const PREVIEW_WALL_THICKNESS: f64 = 0.3;

/// Slots the manager table edits. `HostCut2D` stays on the wall regen path.
pub const EDITABLE_SLOTS: &[OpeningComponentSlot] = &[
    OpeningComponentSlot::Frame2D,
    OpeningComponentSlot::Leaf2D,
    OpeningComponentSlot::Swing2D,
    OpeningComponentSlot::Glazing2D,
    OpeningComponentSlot::Sill2D,
    OpeningComponentSlot::Threshold2D,
    OpeningComponentSlot::BreakthroughSymbol2D,
    OpeningComponentSlot::OpeningLabel2D,
    OpeningComponentSlot::ElevationContour2D,
    OpeningComponentSlot::ElevationMuntins2D,
    OpeningComponentSlot::ElevationSwing2D,
    OpeningComponentSlot::ElevationSill2D,
    OpeningComponentSlot::Frame3D,
    OpeningComponentSlot::Leaf3D,
    OpeningComponentSlot::Glazing3D,
    OpeningComponentSlot::Mark2D,
    OpeningComponentSlot::Solid3D,
];

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_OPENINGSTYLEMANAGER",
        label: "Opening Style Manager",
        icon: IconKind::Svg(include_bytes!(
            "../../../../assets/icons/aec/wall_style_manager.svg"
        )),
        event: ModuleEvent::Command("AEC_OPENINGSTYLEMANAGER".to_string()),
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["AEC_OPENINGSTYLEMANAGER"],
});

pub fn format_dim(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub fn parse_dim(s: &str) -> Option<f64> {
    let v = s.trim().parse::<f64>().ok()?;
    if v.is_finite() {
        Some(v)
    } else {
        None
    }
}

pub fn slot_buffers_from_map(
    slots: &HashMap<OpeningComponentSlot, SlotGeometry>,
) -> Vec<AecOpeningSlotBuffer> {
    EDITABLE_SLOTS
        .iter()
        .map(|slot| match slots.get(slot) {
            Some(SlotGeometry::Generator(gen)) => AecOpeningSlotBuffer {
                slot: *slot,
                source: AecOpeningSlotSource::Generator,
                generator: *gen,
                block_name: String::new(),
                block_placement: BlockPlacementMode::default(),
                sketch: OpeningSketch::default(),
                visible: true,
            },
            Some(SlotGeometry::Block {
                block_name,
                placement,
            }) => AecOpeningSlotBuffer {
                slot: *slot,
                source: AecOpeningSlotSource::Block,
                generator: OpeningGenerator::None,
                block_name: block_name.clone(),
                block_placement: *placement,
                sketch: OpeningSketch::default(),
                visible: true,
            },
            Some(SlotGeometry::Sketch(sketch)) => AecOpeningSlotBuffer {
                slot: *slot,
                source: AecOpeningSlotSource::Sketch,
                generator: OpeningGenerator::None,
                block_name: String::new(),
                block_placement: BlockPlacementMode::default(),
                sketch: sketch.clone(),
                visible: true,
            },
            None => AecOpeningSlotBuffer {
                slot: *slot,
                source: AecOpeningSlotSource::Generator,
                generator: OpeningGenerator::None,
                block_name: String::new(),
                block_placement: BlockPlacementMode::default(),
                sketch: OpeningSketch::default(),
                visible: true,
            },
        })
        .collect()
}

pub fn slot_buffers_for_kind(kind: OpeningKind) -> Vec<AecOpeningSlotBuffer> {
    slot_buffers_from_map(&default_slots_for_kind(kind))
}

pub fn slots_from_buffers(
    buffers: &[AecOpeningSlotBuffer],
) -> HashMap<OpeningComponentSlot, SlotGeometry> {
    let mut slots = HashMap::new();
    for buf in buffers {
        let geom = match buf.source {
            AecOpeningSlotSource::Generator => SlotGeometry::Generator(buf.generator),
            AecOpeningSlotSource::Block => SlotGeometry::Block {
                block_name: buf.block_name.clone(),
                placement: buf.block_placement,
            },
            AecOpeningSlotSource::Sketch => SlotGeometry::Sketch(buf.sketch.clone()),
        };
        slots.insert(buf.slot, geom);
    }
    slots
}

fn profile_from_buffers(buffers: &[AecOpeningSlotBuffer]) -> OpeningDisplayProfile {
    let mut profile = OpeningDisplayProfile::default();
    for buf in buffers {
        let geom = match buf.source {
            AecOpeningSlotSource::Generator => SlotGeometry::Generator(buf.generator),
            AecOpeningSlotSource::Block => SlotGeometry::Block {
                block_name: buf.block_name.clone(),
                placement: buf.block_placement,
            },
            AecOpeningSlotSource::Sketch => SlotGeometry::Sketch(buf.sketch.clone()),
        };
        profile.slots.insert(buf.slot, geom);
        if !buf.visible {
            profile.visibility.insert(buf.slot.key().to_string(), false);
        }
    }
    profile
}

fn buffers_for_profile(
    default_slots: &HashMap<OpeningComponentSlot, SlotGeometry>,
    profiles: &HashMap<String, OpeningDisplayProfile>,
    plan: Option<&str>,
) -> Vec<AecOpeningSlotBuffer> {
    if let Some(name) = plan {
        let profile = profiles.get(name).cloned().unwrap_or_default();
        let mut merged = default_slots.clone();
        for (slot, geom) in &profile.slots {
            merged.insert(*slot, geom.clone());
        }
        let mut bufs = slot_buffers_from_map(&merged);
        for buf in &mut bufs {
            buf.visible = profile
                .visibility
                .get(buf.slot.key())
                .copied()
                .unwrap_or(true);
        }
        bufs
    } else {
        slot_buffers_from_map(default_slots)
    }
}

fn apply_current_buffers_to_store(app: &mut OpenCADStudio) {
    commit_sketch_draft(app, false);
    if let Some(name) = app.aec.aec_opening_style_manager_profile_selected.clone() {
        app.aec
            .aec_opening_style_manager_display_profiles
            .insert(name, profile_from_buffers(&app.aec.aec_opening_style_manager_slots));
    } else {
        app.aec.aec_opening_style_manager_default_slots =
            slots_from_buffers(&app.aec.aec_opening_style_manager_slots);
    }
}

pub fn draft_opening_style(
    editing_id: Option<String>,
    name: &str,
    parent_id: Option<String>,
    kind: OpeningKind,
    width: f64,
    height: f64,
    sill: f64,
    hinge: HingeSide,
    frame_thickness: f64,
    opening_angle_deg: f64,
    shape: OpeningShape,
    spring_height: f64,
    slots: HashMap<OpeningComponentSlot, SlotGeometry>,
) -> Result<OpeningStyle, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("name empty".to_string());
    }
    if width <= 0.0 || height <= 0.0 {
        return Err("size".to_string());
    }
    let (width, height) = shape.lock_size(width, height, true);
    let spring_height = if shape == OpeningShape::Arch {
        let spring = if spring_height <= 1e-12 {
            OpeningShape::default_spring_height(width, height)
        } else {
            spring_height
        };
        clamp_spring(spring, height)
    } else {
        0.0
    };
    let id = editing_id.unwrap_or_else(|| {
        crate::modules::aec::engine::xdata::unique_id("ostyle", name)
    });
    Ok(OpeningStyle {
        style: Style {
            id,
            name: name.to_string(),
            object_kind: OpeningStyle::object_kind_for(kind).to_string(),
            parent_style_id: parent_id,
        },
        kind,
        default_width: width,
        default_height: height,
        default_sill: sill.max(0.0),
        hinge,
        frame_thickness: frame_thickness.max(0.0),
        opening_angle_deg,
        shape,
        spring_height,
        slots,
        display_profiles: HashMap::new(),
    })
}

/// Bake generator and sketch primitives for the manager canvas.
/// Empty sketches emit nothing (no generator fallback).
pub fn preview_baked_paths(style: &OpeningStyle, preview_width: f64) -> Vec<BakedPath> {
    let width = if preview_width > 1e-12 {
        preview_width
    } else {
        style.default_width
    };
    let (width, _) = style.shape.lock_size(width, style.default_height, true);
    let params = OpeningBakeParams {
        width,
        height: style.default_height,
        sill_height: style.default_sill,
        thickness: PREVIEW_WALL_THICKNESS,
        frame_thickness: style.frame_thickness,
        cross_axis_offset: 0.0,
        hinge: style.hinge,
        shape: style.shape,
        spring_height: style.spring_height,
        opening_angle_deg: style.opening_angle_deg,
        kind: style.kind,
    };
    bake_opening_generators(&style.slots, params, None)
}

fn load_buffers_from_style(app: &mut OpenCADStudio, style: &OpeningStyle) {
    app.aec.aec_opening_style_manager_editing_id = Some(style.style.id.clone());
    app.aec.aec_opening_style_manager_name = style.style.name.clone();
    app.aec.aec_opening_style_manager_parent = style.style.parent_style_id.clone();
    app.aec.aec_opening_style_manager_kind = style.kind;
    app.aec.aec_opening_style_manager_width = format_dim(style.default_width);
    app.aec.aec_opening_style_manager_height = format_dim(style.default_height);
    app.aec.aec_opening_style_manager_sill = format_dim(style.default_sill);
    app.aec.aec_opening_style_manager_frame = format_dim(style.frame_thickness);
    app.aec.aec_opening_style_manager_angle = format_dim(style.opening_angle_deg);
    app.aec.aec_opening_style_manager_spring = if style.shape == OpeningShape::Arch {
        format_dim(style.spring_height)
    } else {
        String::new()
    };
    app.aec.aec_opening_style_manager_hinge = style.hinge;
    app.aec.aec_opening_style_manager_shape = style.shape;
    app.aec.aec_opening_style_manager_default_slots = style.slots.clone();
    app.aec.aec_opening_style_manager_display_profiles = style.display_profiles.clone();
    app.aec.aec_opening_style_manager_profile_selected = None;
    app.aec.aec_opening_style_manager_slots = slot_buffers_from_map(&style.slots);
    reset_sketch_draft(app);
    app.aec.aec_opening_style_manager_sketch_slot = app
        .aec
        .aec_opening_style_manager_slots
        .iter()
        .position(|b| b.source == AecOpeningSlotSource::Sketch);
    app.aec.aec_opening_style_manager_form_open = true;
}

fn reset_sketch_draft(app: &mut OpenCADStudio) {
    app.aec.aec_opening_style_manager_sketch_draft.clear();
    app.aec.aec_opening_style_manager_sketch_draft_bulges.clear();
}

fn commit_sketch_draft(app: &mut OpenCADStudio, closed: bool) {
    let Some(index) = app.aec.aec_opening_style_manager_sketch_slot else {
        reset_sketch_draft(app);
        return;
    };
    if index >= app.aec.aec_opening_style_manager_slots.len() {
        reset_sketch_draft(app);
        return;
    }
    let buf = &mut app.aec.aec_opening_style_manager_slots[index];
    if buf.source != AecOpeningSlotSource::Sketch {
        reset_sketch_draft(app);
        return;
    }
    commit_draft(
        &mut buf.sketch,
        &mut app.aec.aec_opening_style_manager_sketch_draft,
        &mut app.aec.aec_opening_style_manager_sketch_draft_bulges,
        closed,
    );
}

fn ensure_sketch_ref_box(buf: &mut AecOpeningSlotBuffer, width: f64) {
    if buf.sketch.paths.is_empty() {
        buf.sketch.ref_width = width.max(1e-9);
        buf.sketch.ref_thickness = PREVIEW_WALL_THICKNESS;
    }
    if buf.sketch.ref_width <= 1e-12 {
        buf.sketch.ref_width = width.max(1e-9);
    }
    if buf.sketch.ref_thickness <= 1e-12 {
        buf.sketch.ref_thickness = PREVIEW_WALL_THICKNESS;
    }
}

fn apply_size_lock(app: &mut OpenCADStudio, prefer_width: bool) {
    let width = parse_dim(&app.aec.aec_opening_style_manager_width);
    let height = parse_dim(&app.aec.aec_opening_style_manager_height);
    match app.aec.aec_opening_style_manager_shape {
        OpeningShape::Circle => {
            if prefer_width {
                if let Some(w) = width.filter(|v| *v > 0.0) {
                    app.aec.aec_opening_style_manager_height = format_dim(w);
                }
            } else if let Some(h) = height.filter(|v| *v > 0.0) {
                app.aec.aec_opening_style_manager_width = format_dim(h);
            }
        }
        OpeningShape::Triangle(
            crate::modules::aec::engine::opening_shape::TriangleVariant::Equilateral,
        ) => {
            if let Some(w) = width.filter(|v| *v > 0.0) {
                app.aec.aec_opening_style_manager_height =
                    format_dim(OpeningShape::equilateral_height(w));
            }
        }
        OpeningShape::Arch => {
            let spring = parse_dim(&app.aec.aec_opening_style_manager_spring).unwrap_or(0.0);
            if spring <= 1e-12 {
                if let (Some(w), Some(h)) = (width, height) {
                    if h > 0.0 {
                        app.aec.aec_opening_style_manager_spring =
                            format_dim(OpeningShape::default_spring_height(w, h));
                    }
                }
            }
        }
        _ => {}
    }
}

impl OpenCADStudio {
    pub(crate) fn aec_opening_style_manager_open(&mut self) -> Task<Message> {
        self.ribbon.close_dropdown();
        self.aec_refresh_combined_style_library();
        self.aec.aec_opening_style_manager_filter.clear();
        self.aec.aec_opening_style_manager_selected = None;
        self.aec.aec_opening_style_manager_editing_id = None;
        self.aec.aec_opening_style_manager_form_open = false;
        self.aec.aec_opening_style_manager_slots.clear();
        self.aec.aec_opening_style_manager_default_slots.clear();
        self.aec.aec_opening_style_manager_display_profiles.clear();
        self.aec.aec_opening_style_manager_profile_selected = None;
        self.aec.aec_plan_library = Some(
            crate::modules::aec::engine::project::resolve_display_config_library(
                self.aec.aec_project_explorer_file.as_ref(),
            ),
        );
        self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::OpeningStyleManager));
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_select(&mut self, id: String) -> Task<Message> {
        let style = crate::modules::aec::engine::library::combined_opening_style_entries_with_session(
            self.aec.aec_project_explorer_file.as_ref(),
            self.aec.aec_session_style_library.as_ref(),
        )
        .into_iter()
        .find(|e| e.opening_style.style.id == id)
        .map(|e| e.opening_style)
        .or_else(|| {
            self.aec
                .aec_style_library
                .as_ref()
                .and_then(|lib| lib.find_opening_style(&id).cloned())
        });
        if let Some(style) = style {
            load_buffers_from_style(self, &style);
        }
        self.aec.aec_opening_style_manager_selected = Some(id);
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_new(&mut self) -> Task<Message> {
        self.aec.aec_opening_style_manager_selected = None;
        self.aec.aec_opening_style_manager_editing_id = None;
        self.aec.aec_opening_style_manager_name.clear();
        self.aec.aec_opening_style_manager_parent = None;
        self.aec.aec_opening_style_manager_kind = OpeningKind::Window;
        let seed = OpeningStyle::standard_window();
        load_buffers_from_style(self, &seed);
        self.aec.aec_opening_style_manager_editing_id = None;
        self.aec.aec_opening_style_manager_name.clear();
        self.aec.aec_opening_style_manager_form_open = true;
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_kind_changed(&mut self, value: String) -> Task<Message> {
        let kind = OpeningKind::from_str(&value);
        let was_new = self.aec.aec_opening_style_manager_editing_id.is_none();
        self.aec.aec_opening_style_manager_kind = kind;
        if was_new {
            let (w, h, sill) = kind.default_dimensions();
            self.aec.aec_opening_style_manager_width = format_dim(w);
            self.aec.aec_opening_style_manager_height = format_dim(h);
            self.aec.aec_opening_style_manager_sill = format_dim(sill);
            self.aec.aec_opening_style_manager_frame = match kind {
                OpeningKind::Breakthrough => "0".to_string(),
                _ => format_dim(DEFAULT_FRAME_THICKNESS),
            };
            self.aec.aec_opening_style_manager_angle = match kind {
                OpeningKind::Breakthrough => "0".to_string(),
                _ => format_dim(DEFAULT_OPENING_ANGLE_DEG),
            };
            self.aec.aec_opening_style_manager_slots = slot_buffers_for_kind(kind);
            self.aec.aec_opening_style_manager_default_slots =
                slots_from_buffers(&self.aec.aec_opening_style_manager_slots);
            self.aec.aec_opening_style_manager_display_profiles.clear();
            self.aec.aec_opening_style_manager_profile_selected = None;
            reset_sketch_draft(self);
            self.aec.aec_opening_style_manager_sketch_slot = None;
            apply_size_lock(self, true);
        }
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_shape_changed(&mut self, value: String) -> Task<Message> {
        self.aec.aec_opening_style_manager_shape = OpeningShape::from_str(&value);
        apply_size_lock(self, true);
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_width_changed(&mut self, value: String) -> Task<Message> {
        self.aec.aec_opening_style_manager_width = value;
        apply_size_lock(self, true);
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_height_changed(&mut self, value: String) -> Task<Message> {
        self.aec.aec_opening_style_manager_height = value;
        apply_size_lock(self, false);
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_slot_source_changed(
        &mut self,
        index: usize,
        value: String,
    ) -> Task<Message> {
        commit_sketch_draft(self, false);
        let width = parse_dim(&self.aec.aec_opening_style_manager_width).unwrap_or(1.0);
        if let Some(slot) = self.aec.aec_opening_style_manager_slots.get_mut(index) {
            slot.source = if value.eq_ignore_ascii_case("Sketch") {
                AecOpeningSlotSource::Sketch
            } else {
                AecOpeningSlotSource::Generator
            };
            if slot.source == AecOpeningSlotSource::Sketch {
                ensure_sketch_ref_box(slot, width);
            }
        }
        if self
            .aec
            .aec_opening_style_manager_slots
            .get(index)
            .is_some_and(|s| s.source == AecOpeningSlotSource::Sketch)
        {
            self.aec.aec_opening_style_manager_sketch_slot = Some(index);
        }
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_slot_generator_changed(
        &mut self,
        index: usize,
        value: String,
    ) -> Task<Message> {
        if let Some(slot) = self.aec.aec_opening_style_manager_slots.get_mut(index) {
            slot.generator = OpeningGenerator::from_str(&value);
            slot.source = AecOpeningSlotSource::Generator;
        }
        Task::none()
    }

    pub(crate) fn aec_opening_style_from_form(&self) -> Result<OpeningStyle, String> {
        let width = parse_dim(&self.aec.aec_opening_style_manager_width).ok_or("width")?;
        let height = parse_dim(&self.aec.aec_opening_style_manager_height).ok_or("height")?;
        let sill = parse_dim(&self.aec.aec_opening_style_manager_sill).unwrap_or(0.0);
        let frame = parse_dim(&self.aec.aec_opening_style_manager_frame).unwrap_or(0.0);
        let angle = parse_dim(&self.aec.aec_opening_style_manager_angle).unwrap_or(0.0);
        let spring = parse_dim(&self.aec.aec_opening_style_manager_spring).unwrap_or(0.0);
        let mut slots = self.aec.aec_opening_style_manager_default_slots.clone();
        let mut profiles = self.aec.aec_opening_style_manager_display_profiles.clone();
        if let Some(name) = &self.aec.aec_opening_style_manager_profile_selected {
            profiles.insert(
                name.clone(),
                profile_from_buffers(&self.aec.aec_opening_style_manager_slots),
            );
        } else {
            slots = slots_from_buffers(&self.aec.aec_opening_style_manager_slots);
        }
        let mut style = draft_opening_style(
            self.aec.aec_opening_style_manager_editing_id.clone(),
            &self.aec.aec_opening_style_manager_name,
            self.aec.aec_opening_style_manager_parent.clone(),
            self.aec.aec_opening_style_manager_kind,
            width,
            height,
            sill,
            self.aec.aec_opening_style_manager_hinge,
            frame,
            angle,
            self.aec.aec_opening_style_manager_shape,
            spring,
            slots,
        )?;
        style.display_profiles = profiles;
        Ok(style)
    }

    pub(crate) fn aec_opening_style_manager_profile_select(
        &mut self,
        value: String,
    ) -> Task<Message> {
        apply_current_buffers_to_store(self);
        self.aec.aec_opening_style_manager_profile_selected = if value.is_empty() {
            None
        } else {
            Some(value)
        };
        self.aec.aec_opening_style_manager_slots = buffers_for_profile(
            &self.aec.aec_opening_style_manager_default_slots,
            &self.aec.aec_opening_style_manager_display_profiles,
            self.aec.aec_opening_style_manager_profile_selected.as_deref(),
        );
        reset_sketch_draft(self);
        self.aec.aec_opening_style_manager_sketch_slot = self
            .aec
            .aec_opening_style_manager_slots
            .iter()
            .position(|b| b.source == AecOpeningSlotSource::Sketch);
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_slot_visible(
        &mut self,
        index: usize,
        visible: bool,
    ) -> Task<Message> {
        if let Some(slot) = self.aec.aec_opening_style_manager_slots.get_mut(index) {
            slot.visible = visible;
        }
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_sketch_slot_select(
        &mut self,
        index: usize,
    ) -> Task<Message> {
        commit_sketch_draft(self, false);
        let width = parse_dim(&self.aec.aec_opening_style_manager_width).unwrap_or(1.0);
        if let Some(slot) = self.aec.aec_opening_style_manager_slots.get_mut(index) {
            if slot.source == AecOpeningSlotSource::Sketch {
                ensure_sketch_ref_box(slot, width);
            }
            self.aec.aec_opening_style_manager_sketch_slot = Some(index);
        }
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_sketch_click(
        &mut self,
        inst_x: f64,
        inst_y: f64,
    ) -> Task<Message> {
        let Some(index) = self.aec.aec_opening_style_manager_sketch_slot else {
            return Task::none();
        };
        let width = parse_dim(&self.aec.aec_opening_style_manager_width).unwrap_or(1.0);
        let frame = parse_dim(&self.aec.aec_opening_style_manager_frame).unwrap_or(0.0);
        let snapped = {
            let Some(slot) = self.aec.aec_opening_style_manager_slots.get_mut(index) else {
                return Task::none();
            };
            if slot.source != AecOpeningSlotSource::Sketch {
                return Task::none();
            }
            ensure_sketch_ref_box(slot, width);
            let bake = TwoRectBake::from_sketch(
                &slot.sketch,
                width,
                PREVIEW_WALL_THICKNESS,
                frame.max(0.0),
            );
            let ref_pt = bake.unmap_point((inst_x, inst_y));
            snap_ref_point(
                ref_pt,
                slot.sketch.ref_width,
                slot.sketch.ref_thickness,
                frame.max(0.0),
                0.04,
            )
        };
        draft_append(
            &mut self.aec.aec_opening_style_manager_sketch_draft,
            &mut self.aec.aec_opening_style_manager_sketch_draft_bulges,
            snapped,
        );
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_sketch_close_path(&mut self) -> Task<Message> {
        commit_sketch_draft(self, true);
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_sketch_finish_path(&mut self) -> Task<Message> {
        commit_sketch_draft(self, false);
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_sketch_undo(&mut self) -> Task<Message> {
        self.aec.aec_opening_style_manager_sketch_draft.pop();
        self.aec.aec_opening_style_manager_sketch_draft_bulges.pop();
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_sketch_delete_path(&mut self) -> Task<Message> {
        reset_sketch_draft(self);
        let Some(index) = self.aec.aec_opening_style_manager_sketch_slot else {
            return Task::none();
        };
        if let Some(slot) = self.aec.aec_opening_style_manager_slots.get_mut(index) {
            slot.sketch.paths.pop();
        }
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_sketch_clear(&mut self) -> Task<Message> {
        reset_sketch_draft(self);
        let Some(index) = self.aec.aec_opening_style_manager_sketch_slot else {
            return Task::none();
        };
        if let Some(slot) = self.aec.aec_opening_style_manager_slots.get_mut(index) {
            slot.sketch.paths.clear();
        }
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_sketch_add_frame(&mut self) -> Task<Message> {
        reset_sketch_draft(self);
        let Some(index) = self.aec.aec_opening_style_manager_sketch_slot else {
            return Task::none();
        };
        let width = parse_dim(&self.aec.aec_opening_style_manager_width).unwrap_or(1.0);
        let frame = parse_dim(&self.aec.aec_opening_style_manager_frame).unwrap_or(DEFAULT_FRAME_THICKNESS);
        if let Some(slot) = self.aec.aec_opening_style_manager_slots.get_mut(index) {
            ensure_sketch_ref_box(slot, width);
            slot.source = AecOpeningSlotSource::Sketch;
            slot.sketch = OpeningSketch::frame_ring(
                slot.sketch.ref_width,
                slot.sketch.ref_thickness,
                frame.max(0.0),
            );
        }
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_sketch_arc(&mut self) -> Task<Message> {
        draft_set_last_arc(
            &mut self.aec.aec_opening_style_manager_sketch_draft_bulges,
            true,
        );
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_save(&mut self) -> Task<Message> {
        commit_sketch_draft(self, false);
        let style = match self.aec_opening_style_from_form() {
            Ok(style) => style,
            Err(_) => {
                self.command_line.push_error(
                    crate::t!("AEC Style Manager: opening style name cannot be empty.")
                        .as_ref(),
                );
                return Task::none();
            }
        };

        if let (Some(parent_id), Some(lib)) = (
            style.style.parent_style_id.as_ref(),
            self.aec.aec_style_library.as_ref(),
        ) {
            if parent_id == &style.style.id {
                self.command_line.push_error(
                    crate::t!("AEC Style Manager: an opening style cannot be its own parent.")
                        .as_ref(),
                );
                return Task::none();
            }
            let mut styles = HashMap::new();
            for os in &lib.opening_styles {
                if os.style.id != style.style.id {
                    styles.insert(os.style.id.clone(), os.style.clone());
                }
            }
            styles.insert(style.style.id.clone(), style.style.clone());
            if crate::modules::aec::engine::style::resolve_chain(&styles, &style.style.id)
                == Err(crate::modules::aec::engine::style::StyleError::CycleDetected)
            {
                self.command_line.push_error(
                    crate::t!("AEC Style Manager: cycle detected in opening style inheritance.")
                        .as_ref(),
                );
                return Task::none();
            }
        }

        let id = style.style.id.clone();
        let source =
            crate::modules::aec::engine::library::opening_style_library_source_with_session(
                self.aec.aec_project_explorer_file.as_ref(),
                self.aec.aec_session_style_library.as_ref(),
                &id,
            );
        let cow_from_standard =
            source == Some(crate::modules::aec::engine::library::LibrarySource::Standard);

        if self.aec.aec_project_explorer_file.is_some() {
            match self.aec_upsert_opening_style_into_project(style) {
                Ok(()) => {
                    if cow_from_standard {
                        self.command_line.push_info(
                            crate::t!(
                                "AEC Style Manager: Standard opening style was copied into the project and saved."
                            )
                            .as_ref(),
                        );
                    } else {
                        self.command_line.push_info(
                            crate::t!("AEC Style Manager: opening style saved.").as_ref(),
                        );
                    }
                    self.aec.aec_opening_style_manager_selected = Some(id.clone());
                    self.aec.aec_opening_style_manager_editing_id = Some(id);
                }
                Err(e) => {
                    self.command_line.push_error(
                        crate::tf!("AEC Style Manager: failed to save library: {e}").as_ref(),
                    );
                }
            }
        } else {
            self.aec_upsert_opening_style_into_session(style);
            self.aec.aec_opening_style_manager_selected = Some(id.clone());
            self.aec.aec_opening_style_manager_editing_id = Some(id);
        }
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_delete(&mut self) -> Task<Message> {
        if let Some(id) = self.aec.aec_opening_style_manager_selected.clone() {
            let lib_snapshot = if let Some(lib) = self.aec.aec_style_library.as_mut() {
                lib.remove_opening_style(&id);
                Some(lib.clone())
            } else {
                None
            };
            if let Some(lib_snapshot) = lib_snapshot {
                match self.aec_save_style_library_preferring_project(&lib_snapshot) {
                    Ok(()) => self.command_line.push_info(
                        crate::t!("AEC Style Manager: opening style deleted.").as_ref(),
                    ),
                    Err(e) => self.command_line.push_error(
                        crate::tf!("AEC Style Manager: failed to save library: {e}").as_ref(),
                    ),
                }
            }
        }
        self.aec.aec_opening_style_manager_selected = None;
        self.aec.aec_opening_style_manager_editing_id = None;
        self.aec.aec_opening_style_manager_form_open = false;
        Task::none()
    }

    pub(crate) fn aec_handle_copy_opening_style(&mut self, to_project: bool) -> Task<Message> {
        if to_project && self.aec.aec_project_explorer_file.is_none() {
            return Task::none();
        }
        let Some(id) = self.aec.aec_opening_style_manager_selected.clone() else {
            return Task::none();
        };
        let global_lib = crate::modules::aec::engine::library::load_or_seed();
        let project_lib = crate::modules::aec::engine::project::resolve_style_library(
            self.aec.aec_project_explorer_file.as_ref(),
        );
        let (source_lib, target_lib) = if to_project {
            (&global_lib, &project_lib)
        } else {
            (&project_lib, &global_lib)
        };
        let Some(opening_style) = source_lib
            .opening_styles
            .iter()
            .find(|s| s.style.id == id)
            .cloned()
        else {
            return Task::none();
        };
        let conflict = crate::modules::aec::engine::library::opening_style_copy_conflict(
            target_lib,
            &opening_style,
        );
        match conflict {
            crate::modules::aec::engine::library::CopyConflict::DifferentContentCollision => {
                self.aec.aec_style_manager_pending_copy = Some(AecPendingCopy::OpeningStyle {
                    opening_style,
                    to_project,
                });
                self.aec.aec_style_manager_copy_conflict_open = true;
                self.active_modal =
                    Some(crate::app::ModalKind::Aec(AecModalKind::StyleCopyConflict));
            }
            _ => {
                self.aec_execute_copy(AecPendingCopy::OpeningStyle {
                    opening_style,
                    to_project,
                });
            }
        }
        Task::none()
    }

    pub(crate) fn aec_apply_picked_opening_style(&mut self, selection: String) -> Task<Message> {
        let Some(lib) = self.aec.aec_style_library.clone() else {
            return Task::none();
        };
        let i = self.active_tab;
        self.push_undo_snapshot(i, "CHPROP");
        let handles = self.aec.aec_style_picker_wall_handles.clone();
        for handle in handles {
            let owner = crate::modules::aec::engine::opening_display::resolve_opening_package(
                &self.tabs[i].scene,
                handle,
            );
            let Some(entity) = self.tabs[i].scene.document.get_entity(owner).cloned() else {
                continue;
            };
            let Some(mut opening) =
                crate::modules::aec::engine::opening_xdata::opening_from_entity(&entity, owner)
            else {
                continue;
            };
            crate::modules::aec::properties::apply_opening_property(
                &mut opening,
                "opening_style",
                &selection,
                Some(&lib),
            );
            let (rules, _) =
                self.resolve_active_display_config_wall_rules(i, Some(opening.host_wall));
            let _ = crate::modules::aec::engine::opening_display::commit_opening_instance(
                &mut self.tabs[i].scene,
                &opening,
                Some(&lib),
                rules.as_ref(),
            );
            self.tabs[i].dirty = true;
        }
        self.refresh_properties();
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::opening_style::DEFAULT_FRAME_THICKNESS;

    #[test]
    fn circle_width_edit_locks_height_in_draft() {
        let style = draft_opening_style(
            None,
            "Round",
            None,
            OpeningKind::Window,
            1.2,
            0.8,
            0.9,
            HingeSide::Left,
            0.06,
            90.0,
            OpeningShape::Circle,
            0.0,
            default_slots_for_kind(OpeningKind::Window),
        )
        .unwrap();
        assert!((style.default_width - 1.2).abs() < 1e-12);
        assert!((style.default_height - 1.2).abs() < 1e-12);
    }

    #[test]
    fn equilateral_width_sets_height() {
        let style = draft_opening_style(
            None,
            "Tri",
            None,
            OpeningKind::Window,
            2.0,
            9.0,
            0.1,
            HingeSide::Left,
            0.06,
            90.0,
            OpeningShape::Triangle(
                crate::modules::aec::engine::opening_shape::TriangleVariant::Equilateral,
            ),
            0.0,
            default_slots_for_kind(OpeningKind::Window),
        )
        .unwrap();
        let expected = OpeningShape::equilateral_height(2.0);
        assert!((style.default_height - expected).abs() < 1e-12);
    }

    #[test]
    fn frame_thickness_independent_of_preview_width() {
        let mut style = OpeningStyle::standard_window();
        style.frame_thickness = DEFAULT_FRAME_THICKNESS;
        let narrow = preview_baked_paths(&style, 1.2);
        let wide = preview_baked_paths(&style, 1.8);
        let inset = |paths: &[BakedPath]| {
            let frame: Vec<_> = paths
                .iter()
                .filter(|p| p.slot == OpeningComponentSlot::Frame2D && p.closed)
                .collect();
            assert!(frame.len() >= 2);
            let outer = frame[0].points[1].0 - frame[0].points[0].0;
            let inner = frame[1].points[1].0 - frame[1].points[0].0;
            (outer - inner) * 0.5
        };
        assert!((inset(&narrow) - DEFAULT_FRAME_THICKNESS).abs() < 1e-9);
        assert!((inset(&wide) - DEFAULT_FRAME_THICKNESS).abs() < 1e-9);
        assert!((inset(&narrow) - inset(&wide)).abs() < 1e-12);
    }

    #[test]
    fn sketch_slot_does_not_fall_back_to_generator() {
        let mut slots = default_slots_for_kind(OpeningKind::Window);
        slots.insert(
            OpeningComponentSlot::Frame2D,
            SlotGeometry::Sketch(OpeningSketch::default()),
        );
        let mut style = OpeningStyle::standard_window();
        style.slots = slots;
        let paths = preview_baked_paths(&style, 1.2);
        assert!(paths
            .iter()
            .all(|p| p.slot != OpeningComponentSlot::Frame2D));
        assert!(paths
            .iter()
            .any(|p| p.slot == OpeningComponentSlot::Leaf2D));
    }

    #[test]
    fn empty_name_is_rejected() {
        assert!(draft_opening_style(
            None,
            "  ",
            None,
            OpeningKind::Window,
            1.0,
            1.0,
            0.1,
            HingeSide::Left,
            0.06,
            90.0,
            OpeningShape::Rectangle,
            0.0,
            HashMap::new(),
        )
        .is_err());
    }

    #[test]
    fn slot_roundtrip_preserves_sketch_source() {
        let mut buffers = slot_buffers_for_kind(OpeningKind::Window);
        let kept_gen = buffers[0].generator;
        buffers[0].source = AecOpeningSlotSource::Sketch;
        buffers[0].sketch = OpeningSketch {
            ref_width: 1.0,
            ref_thickness: 0.3,
            paths: Vec::new(),
        };
        let map = slots_from_buffers(&buffers);
        assert!(matches!(
            map.get(&OpeningComponentSlot::Frame2D),
            Some(SlotGeometry::Sketch(_))
        ));
        buffers[0].source = AecOpeningSlotSource::Generator;
        let map = slots_from_buffers(&buffers);
        assert_eq!(
            map.get(&OpeningComponentSlot::Frame2D),
            Some(&SlotGeometry::Generator(kept_gen))
        );
        assert!(!buffers[0].sketch.paths.is_empty() || buffers[0].sketch.ref_width == 1.0);
    }

    #[test]
    fn sketch_frame_ring_preview_keeps_inset_when_width_changes() {
        let mut slots = default_slots_for_kind(OpeningKind::Window);
        slots.insert(
            OpeningComponentSlot::Frame2D,
            SlotGeometry::Sketch(OpeningSketch::frame_ring(1.0, 0.3, DEFAULT_FRAME_THICKNESS)),
        );
        let mut style = OpeningStyle::standard_window();
        style.slots = slots;
        style.frame_thickness = DEFAULT_FRAME_THICKNESS;
        let inset = |paths: &[BakedPath], width: f64| {
            let frame: Vec<_> = paths
                .iter()
                .filter(|p| p.slot == OpeningComponentSlot::Frame2D && p.closed)
                .collect();
            assert_eq!(frame.len(), 2);
            let max_x = |pts: &[(f64, f64)]| {
                pts.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max)
            };
            let inset_x = max_x(&frame[0].points) - max_x(&frame[1].points);
            assert!((max_x(&frame[0].points) - width * 0.5).abs() < 1e-9);
            inset_x
        };
        let a = preview_baked_paths(&style, 1.2);
        let b = preview_baked_paths(&style, 1.8);
        assert!((inset(&a, 1.2) - DEFAULT_FRAME_THICKNESS).abs() < 1e-9);
        assert!((inset(&b, 1.8) - DEFAULT_FRAME_THICKNESS).abs() < 1e-9);
    }

    #[test]
    fn plan_profile_buffers_preserve_hidden_sill() {
        let defaults = default_slots_for_kind(OpeningKind::Window);
        let mut bufs = slot_buffers_from_map(&defaults);
        for buf in &mut bufs {
            if buf.slot == OpeningComponentSlot::Sill2D {
                buf.visible = false;
            }
        }
        let mut profiles = HashMap::new();
        profiles.insert("Ausführung".to_string(), profile_from_buffers(&bufs));
        let loaded = buffers_for_profile(&defaults, &profiles, Some("Ausführung"));
        let sill = loaded
            .iter()
            .find(|b| b.slot == OpeningComponentSlot::Sill2D)
            .unwrap();
        assert!(!sill.visible);
        let default_loaded = buffers_for_profile(&defaults, &profiles, None);
        let sill_default = default_loaded
            .iter()
            .find(|b| b.slot == OpeningComponentSlot::Sill2D)
            .unwrap();
        assert!(sill_default.visible);
    }
}
