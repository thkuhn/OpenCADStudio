//! `AEC_OPENINGSTYLEMANAGER` ribbon tool and manager dispatch.
//!
//! Form draft / slot-buffer helpers live here so `update.rs` stays thin and
//! the 2D preview can bake generator paths without the CAD viewport.

use std::collections::HashMap;

use iced::Task;

use crate::app::{AecModalKind, AecPendingCopy, Message, OpenCADStudio};
use crate::modules::aec::engine::display_component::OpeningComponentSlot;
use crate::modules::aec::engine::opening_display::{
    bake_opening_elevation_from_grid, bake_opening_plan_from_grid, BakedPath, OpeningBakeParams,
};
use crate::modules::aec::engine::library::StyleLibrary;
use crate::modules::aec::engine::opening_shape::{clamp_spring, OpeningShape};
use crate::modules::aec::engine::opening_style::{
    construction_from_division, default_slots_for_kind, project_division_sized,
    BlockPlacementMode, DivisionGrid, FamilyPlacement,
    FamilySymbol, OpeningDisplayProfile, OpeningFamilySymbols, OpeningGenerator, OpeningParts,
    OpeningCell, OpeningEditorGroup, OpeningLookRole, OpeningLooks, OpeningStyle, SlotGeometry,
    opening_component_should_render, LeafOperation, DEFAULT_FRAME_THICKNESS,
    DEFAULT_OPENING_ANGLE_DEG,
};
use crate::modules::aec::engine::openings::{OpeningKind, SwingSide};
use crate::modules::aec::engine::style::Style;
use crate::modules::aec::state::{
    AecOpeningCellBuffer, AecOpeningLookBuffer, AecOpeningPreviewMode,
    AecOpeningSlotBuffer, AecOpeningSlotSource,
};
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
        label: "Opening",
        icon: IconKind::Svg(include_bytes!(
            "../../../../assets/icons/aec/style_opening.svg"
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

fn parse_list(s: &str) -> Vec<f64> {
    s.split([',', ';', ' '])
        .filter_map(parse_dim)
        .collect()
}

fn format_list(values: &[f64]) -> String {
    values.iter().map(|v| format_dim(*v)).collect::<Vec<_>>().join(", ")
}

fn format_vertical_weights(values: &[f64]) -> String {
    if values.is_empty() {
        return String::new();
    }
    let mut rounded: Vec<f64> = values
        .iter()
        .map(|value| (value * 10_000.0).round() / 10_000.0)
        .collect();
    let total: f64 = rounded.iter().sum();
    let correction_index = rounded
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(index, _)| index)
        .unwrap_or(0);
    rounded[correction_index] += 1.0 - total;
    format_list(&rounded)
}

fn remove_list_value(field: &mut String, index: usize) -> bool {
    let mut values = parse_list(field);
    if index >= values.len() {
        return false;
    }
    values.remove(index);
    *field = format_list(&values);
    true
}

fn dim_or(s: &str, fallback: f64) -> f64 {
    parse_dim(s).unwrap_or(fallback)
}

fn resize_opening_cells(
    cells: &mut Vec<AecOpeningCellBuffer>,
    old_columns: usize,
    old_rows: usize,
    columns: usize,
    rows: usize,
    default_kind: crate::modules::aec::engine::opening_style::OpeningCellKind,
) {
    let old_cells = std::mem::take(cells);
    let mut resized = vec![
        AecOpeningCellBuffer {
            kind: default_kind,
            hinge: None,
            sill: "0".to_string(),
        };
        columns * rows
    ];
    for row in 0..old_rows.min(rows) {
        for column in 0..old_columns.min(columns) {
            let old_index = row * old_columns + column;
            let new_index = row * columns + column;
            if let Some(cell) = old_cells.get(old_index) {
                resized[new_index] = cell.clone();
            }
        }
    }
    *cells = resized;
}

fn default_opening_cell_kind(
    kind: OpeningKind,
) -> crate::modules::aec::engine::opening_style::OpeningCellKind {
    use crate::modules::aec::engine::opening_style::OpeningCellKind;
    match kind {
        OpeningKind::Window => OpeningCellKind::Glass,
        OpeningKind::Door => OpeningCellKind::DoorLeaf,
        OpeningKind::Breakthrough => OpeningCellKind::Opaque,
    }
}

fn symbol_from(name: &str, placement: FamilyPlacement) -> Option<FamilySymbol> {
    let block_name = name.trim().to_string();
    if block_name.is_empty() {
        None
    } else {
        Some(FamilySymbol { block_name, placement })
    }
}

fn load_part_buffers(app: &mut OpenCADStudio, style: &OpeningStyle) {
    let parts = &style.parts;
    let division = project_division_sized(&style.grid, style.default_width, style.default_height);
    use crate::modules::aec::engine::opening_style::OpeningProfilePart as Part;
    app.aec.styles.opening_frame = format_dim(parts.fixed_frame.face_width);
    app.aec.styles.opening_frame_depth = format_dim(parts.fixed_frame.wall_depth);
    app.aec.styles.opening_frame_offset = format_dim(parts.fixed_frame.axis_offset);
    app.aec.styles.opening_leaf_face = format_dim(parts.leaf_frame.face_width);
    app.aec.styles.opening_leaf_depth = format_dim(parts.leaf_frame.depth);
    app.aec.styles.opening_leaf_rebate = format_dim(parts.leaf_frame.rebate_overlap);
    app.aec.styles.opening_glazing_thickness = format_dim(parts.glazing.thickness);
    app.aec.styles.opening_glazing_inset = format_dim(parts.glazing.inset);
    app.aec.styles.opening_ext_sill_thickness = format_dim(parts.exterior_sill.thickness);
    app.aec.styles.opening_ext_sill_overhang = format_dim(parts.exterior_sill.overhang);
    app.aec.styles.opening_ext_sill_upstand = format_dim(parts.exterior_sill.upstand);
    app.aec.styles.opening_int_board_thickness = format_dim(parts.interior_board.thickness);
    app.aec.styles.opening_int_board_overhang = format_dim(parts.interior_board.overhang);
    app.aec.styles.opening_int_board_upstand = format_dim(parts.interior_board.upstand);
    app.aec.styles.opening_threshold_height = format_dim(parts.threshold.height);
    app.aec.styles.opening_threshold_width = format_dim(parts.threshold.width);
    app.aec.styles.opening_shutter_height = format_dim(parts.shutter_box.height);
    app.aec.styles.opening_shutter_depth = format_dim(parts.shutter_box.depth);
    app.aec.styles.opening_shutter_seat = format_dim(parts.shutter_box.seat_above_frame);
    app.aec.styles.opening_shutter_install = format_dim(parts.shutter_box.installation_depth);
    app.aec.styles.opening_lintel_height = format_dim(parts.lintel.height);
    app.aec.styles.opening_lintel_left = format_dim(parts.lintel.bearing_left);
    app.aec.styles.opening_lintel_right = format_dim(parts.lintel.bearing_right);
    app.aec.styles.opening_profile_ids = [
        (Part::FixedFrame, parts.fixed_frame.profile_id.as_ref()),
        (Part::LeafFrame, parts.leaf_frame.profile_id.as_ref()),
        (Part::ExteriorSill, parts.exterior_sill.profile_id.as_ref()),
        (Part::InteriorBoard, parts.interior_board.profile_id.as_ref()),
        (Part::Threshold, parts.threshold.profile_id.as_ref()),
        (Part::ShutterBox, parts.shutter_box.profile_id.as_ref()),
        (Part::Lintel, parts.lintel.profile_id.as_ref()),
        (Part::Division, division.profile_id.as_ref()),
    ]
    .into_iter()
    .filter_map(|(part, id)| id.map(|id| (part, id.clone())))
    .collect();
    app.aec.styles.opening_look_buffers = opening_look_buffers_from_style(style);
    app.aec.styles.opening_editor_tab = OpeningEditorGroup::FixedFrame;
    app.aec.styles.opening_component_enabled = HashMap::from([
        (OpeningEditorGroup::FixedFrame, parts.fixed_frame.enabled),
        (OpeningEditorGroup::Leaf, parts.leaf_frame.enabled),
        (OpeningEditorGroup::Glazing, parts.glazing.enabled),
        (OpeningEditorGroup::ExteriorSill, parts.exterior_sill.enabled),
        (OpeningEditorGroup::InteriorBoard, parts.interior_board.enabled),
        (OpeningEditorGroup::Threshold, parts.threshold.enabled),
        (OpeningEditorGroup::ShutterBox, parts.shutter_box.enabled),
        (OpeningEditorGroup::Lintel, parts.lintel.enabled),
    ]);
    app.aec.styles.opening_look_color_picker_role = None;
    app.aec.styles.opening_selected_look_role = None;
    app.aec.styles.opening_authored_grid = if style.grid.columns.is_empty() && style.grid.cells.is_empty()
    {
        None
    } else {
        Some(style.grid.clone())
    };
    app.aec.styles.opening_vertical = format_vertical_weights(&division.vertical);
    app.aec.styles.opening_horizontal = format_list(&division.horizontal);
    app.aec.styles.opening_vertical_muntins = format_list(&division.vertical_muntins);
    app.aec.styles.opening_horizontal_muntins = format_list(&division.horizontal_muntins);
    app.aec.styles.opening_cells = division
        .cells
        .iter()
        .map(|cell| AecOpeningCellBuffer {
            kind: cell.kind,
            hinge: cell.hinge,
            sill: format_dim(cell.sill),
        })
        .collect();
    if !division.is_implicit_single_cell() {
        let columns = division.vertical.len().max(1);
        let rows = division.horizontal.len() + 1;
        if app.aec.styles.opening_cells.len() != columns * rows {
            resize_opening_cells(
                &mut app.aec.styles.opening_cells,
                columns,
                rows,
                columns,
                rows,
                default_opening_cell_kind(style.kind),
            );
        }
    }
    let symbols = &style.family_symbols;
    app.aec.styles.opening_plan_block = symbols
        .plan
        .as_ref()
        .map(|s| s.block_name.clone())
        .unwrap_or_default();
    app.aec.styles.opening_plan_placement = symbols
        .plan
        .as_ref()
        .map(|s| s.placement)
        .unwrap_or_default();
    app.aec.styles.opening_section_block = symbols
        .section
        .as_ref()
        .map(|s| s.block_name.clone())
        .unwrap_or_default();
    app.aec.styles.opening_section_placement = symbols
        .section
        .as_ref()
        .map(|s| s.placement)
        .unwrap_or_default();
    app.aec.styles.opening_elevation_block = symbols
        .elevation
        .as_ref()
        .map(|s| s.block_name.clone())
        .unwrap_or_default();
    app.aec.styles.opening_elevation_placement = symbols
        .elevation
        .as_ref()
        .map(|s| s.placement)
        .unwrap_or_default();
    app.aec.styles.opening_selected_cell = Some(0);
    refresh_opening_division_measure_buffers(app);
}

fn opening_look_buffers_from_style(
    style: &OpeningStyle,
) -> HashMap<OpeningLookRole, AecOpeningLookBuffer> {
    OpeningLookRole::all()
        .iter()
        .copied()
        .map(|role| {
            let look = style.looks.get(role);
            let material_id = style
                .material_id_for_look(role)
                .unwrap_or_default()
                .to_string();
            (
                role,
                AecOpeningLookBuffer {
                    line_type: look.line_type.clone().unwrap_or_default(),
                    line_color: look
                        .line_color
                        .as_ref()
                        .map(|color| {
                            crate::modules::aec::ui::aec_ui_util::acad_color_to_editor_string(
                                color.clone(),
                            )
                        })
                        .unwrap_or_default(),
                    material_id,
                },
            )
        })
        .collect()
}

pub fn opening_looks_from_buffers(
    buffers: &HashMap<OpeningLookRole, AecOpeningLookBuffer>,
) -> OpeningLooks {
    let mut looks = OpeningLooks::default();
    for role in OpeningLookRole::all() {
        let Some(buffer) = buffers.get(role) else {
            continue;
        };
        let look = looks.get_mut(*role);
        look.line_type = (!buffer.line_type.trim().is_empty())
            .then(|| buffer.line_type.trim().to_string());
        look.line_color = crate::modules::aec::ui::aec_ui_util::editor_string_to_acad_color(
            &buffer.line_color,
        );
        look.material_id = (!buffer.material_id.trim().is_empty())
            .then(|| buffer.material_id.trim().to_string());
    }
    looks
}

fn parts_from_buffers(app: &OpenCADStudio) -> OpeningParts {
    use crate::modules::aec::engine::opening_style::OpeningProfilePart as Part;
    let s = &app.aec.styles;
    let mut parts = OpeningParts::default();
    parts.fixed_frame.profile_id = s.opening_profile_ids.get(&Part::FixedFrame).cloned();
    parts.fixed_frame.enabled = enabled_buffer(&s.opening_component_enabled, OpeningEditorGroup::FixedFrame);
    parts.fixed_frame.face_width = dim_or(&s.opening_frame, parts.fixed_frame.face_width);
    parts.fixed_frame.wall_depth = dim_or(&s.opening_frame_depth, parts.fixed_frame.wall_depth);
    parts.fixed_frame.axis_offset = dim_or(&s.opening_frame_offset, 0.0);
    parts.leaf_frame.face_width = dim_or(&s.opening_leaf_face, parts.leaf_frame.face_width);
    parts.leaf_frame.profile_id = s.opening_profile_ids.get(&Part::LeafFrame).cloned();
    parts.leaf_frame.depth = dim_or(&s.opening_leaf_depth, parts.leaf_frame.depth);
    parts.leaf_frame.rebate_overlap = dim_or(&s.opening_leaf_rebate, parts.leaf_frame.rebate_overlap);
    parts.glazing.thickness = dim_or(&s.opening_glazing_thickness, parts.glazing.thickness);
    parts.glazing.inset = dim_or(&s.opening_glazing_inset, parts.glazing.inset);
    parts.exterior_sill.thickness = dim_or(&s.opening_ext_sill_thickness, 0.0);
    parts.exterior_sill.profile_id = s.opening_profile_ids.get(&Part::ExteriorSill).cloned();
    parts.exterior_sill.overhang = dim_or(&s.opening_ext_sill_overhang, 0.0);
    parts.exterior_sill.upstand = dim_or(&s.opening_ext_sill_upstand, 0.0);
    parts.interior_board.thickness = dim_or(&s.opening_int_board_thickness, 0.0);
    parts.interior_board.profile_id = s.opening_profile_ids.get(&Part::InteriorBoard).cloned();
    parts.interior_board.overhang = dim_or(&s.opening_int_board_overhang, 0.0);
    parts.interior_board.upstand = dim_or(&s.opening_int_board_upstand, 0.0);
    parts.threshold.height = dim_or(&s.opening_threshold_height, 0.0);
    parts.threshold.profile_id = s.opening_profile_ids.get(&Part::Threshold).cloned();
    parts.threshold.width = dim_or(&s.opening_threshold_width, 0.0);
    parts.shutter_box.height = dim_or(&s.opening_shutter_height, 0.0);
    parts.shutter_box.profile_id = s.opening_profile_ids.get(&Part::ShutterBox).cloned();
    parts.shutter_box.depth = dim_or(&s.opening_shutter_depth, 0.0);
    parts.shutter_box.seat_above_frame = dim_or(&s.opening_shutter_seat, 0.0);
    parts.shutter_box.installation_depth = dim_or(&s.opening_shutter_install, 0.0);
    parts.lintel.height = dim_or(&s.opening_lintel_height, 0.0);
    parts.lintel.profile_id = s.opening_profile_ids.get(&Part::Lintel).cloned();
    parts.lintel.bearing_left = dim_or(&s.opening_lintel_left, 0.0);
    parts.lintel.bearing_right = dim_or(&s.opening_lintel_right, 0.0);
    parts.leaf_frame.enabled = enabled_buffer(&s.opening_component_enabled, OpeningEditorGroup::Leaf);
    parts.glazing.enabled = enabled_buffer(&s.opening_component_enabled, OpeningEditorGroup::Glazing);
    parts.exterior_sill.enabled = enabled_buffer(&s.opening_component_enabled, OpeningEditorGroup::ExteriorSill);
    parts.interior_board.enabled = enabled_buffer(&s.opening_component_enabled, OpeningEditorGroup::InteriorBoard);
    parts.threshold.enabled = enabled_buffer(&s.opening_component_enabled, OpeningEditorGroup::Threshold);
    parts.shutter_box.enabled = enabled_buffer(&s.opening_component_enabled, OpeningEditorGroup::ShutterBox);
    parts.lintel.enabled = enabled_buffer(&s.opening_component_enabled, OpeningEditorGroup::Lintel);
    parts
}

fn enabled_buffer(
    enabled: &HashMap<OpeningEditorGroup, bool>,
    group: OpeningEditorGroup,
) -> bool {
    enabled
        .get(&group)
        .copied()
        .unwrap_or(true)
}

pub(crate) fn opening_grid_from_fields(
    vertical: &str,
    horizontal: &str,
    vertical_muntins: &str,
    horizontal_muntins: &str,
    cell_buffers: &[AecOpeningCellBuffer],
) -> DivisionGrid {
    let cells = cell_buffers
        .iter()
        .map(|cell| OpeningCell {
            kind: cell.kind,
            hinge: if cell.kind.permits_hinge() {
                cell.hinge
            } else {
                None
            },
            sill: parse_dim(&cell.sill).unwrap_or(0.0),
        })
        .collect();
    DivisionGrid {
        vertical: parse_list(vertical),
        horizontal: parse_list(horizontal),
        vertical_muntins: parse_list(vertical_muntins),
        horizontal_muntins: parse_list(horizontal_muntins),
        profile_id: None,
        cells,
    }
}

fn grid_from_buffers(app: &OpenCADStudio, height: f64) -> Result<DivisionGrid, String> {
    let s = &app.aec.styles;
    let mut grid = opening_grid_from_fields(
        &s.opening_vertical,
        &s.opening_horizontal,
        &s.opening_vertical_muntins,
        &s.opening_horizontal_muntins,
        &s.opening_cells,
    );
    grid.profile_id = s
        .opening_profile_ids
        .get(&crate::modules::aec::engine::opening_style::OpeningProfilePart::Division)
        .cloned();
    grid.validate(height)?;
    Ok(grid)
}

fn grid_cell_metrics(
    grid: &DivisionGrid,
    width: f64,
    height: f64,
    kind: OpeningKind,
    selected_cell: Option<usize>,
) -> (f64, f64, f64, f64, f64) {
    let fallback_width = width.max(0.08);
    let fallback_height = height.max(0.08);
    let Ok(layout) = grid.layout(fallback_width, fallback_height, kind) else {
        return (
            fallback_width,
            0.0,
            fallback_height,
            fallback_width,
            fallback_height,
        );
    };
    let selected = layout
        .get(selected_cell.unwrap_or(0).min(layout.len().saturating_sub(1)))
        .expect("a valid division grid has at least one cell");
    let min_width = layout
        .iter()
        .map(|cell| cell.right - cell.left)
        .fold(f64::INFINITY, f64::min);
    let min_height = layout
        .iter()
        .map(|cell| cell.top - cell.bottom)
        .fold(f64::INFINITY, f64::min);
    (
        selected.right - selected.left,
        selected.bottom,
        selected.top - selected.bottom,
        min_width,
        min_height,
    )
}

fn horizontal_division_extent(grid: &DivisionGrid, height: f64, kind: OpeningKind) -> f64 {
    grid.resolved_cells(kind)
        .iter()
        .map(|cell| (height - cell.sill).max(0.0))
        .reduce(f64::min)
        .unwrap_or(height)
}

fn vertical_position_fields(weights: &[f64], width: f64) -> Vec<String> {
    let mut cumulative = 0.0;
    weights
        .iter()
        .take(weights.len().saturating_sub(1))
        .map(|weight| {
            cumulative += weight;
            format_dim(cumulative * width)
        })
        .collect()
}

fn horizontal_position_fields(values: &[f64]) -> Vec<String> {
    values.iter().map(|value| format_dim(*value)).collect()
}

fn vertical_muntin_position_fields(fractions: &[f64], cell_width: f64) -> Vec<String> {
    fractions
        .iter()
        .map(|fraction| format_dim(fraction * cell_width))
        .collect()
}

fn horizontal_muntin_position_fields(values: &[f64], cell_bottom: f64) -> Vec<String> {
    values
        .iter()
        .map(|height| format_dim(cell_bottom + height))
        .collect()
}

fn sync_authored_cell(app: &mut OpenCADStudio, index: usize) {
    use crate::modules::aec::engine::construction_grid::CellFill;
    let Some(buffer) = app.aec.styles.opening_cells.get(index).cloned() else {
        return;
    };
    let Some(cell) = app
        .aec
        .styles
        .opening_authored_grid
        .as_mut()
        .and_then(|grid| grid.cells.get_mut(index))
    else {
        return;
    };
    cell.fill = match buffer.kind {
        crate::modules::aec::engine::opening_style::OpeningCellKind::Opaque => CellFill::Opaque,
        crate::modules::aec::engine::opening_style::OpeningCellKind::DoorLeaf => CellFill::Door,
        crate::modules::aec::engine::opening_style::OpeningCellKind::Fixed => CellFill::Fixed,
        crate::modules::aec::engine::opening_style::OpeningCellKind::Glass => CellFill::Glass,
    };
    cell.operation = buffer.hinge;
    cell.sill = parse_dim(&buffer.sill).unwrap_or(0.0);
}

fn refresh_opening_division_measure_buffers(app: &mut OpenCADStudio) {
    let s = &app.aec.styles;
    let width = parse_dim(&s.opening_width).unwrap_or(1.2).max(0.08);
    let height = parse_dim(&s.opening_height).unwrap_or(1.2).max(0.08);
    let grid = opening_grid_from_fields(
        &s.opening_vertical,
        &s.opening_horizontal,
        &s.opening_vertical_muntins,
        &s.opening_horizontal_muntins,
        &s.opening_cells,
    );
    let columns = grid.vertical.len().max(1);
    let rows = grid.horizontal.len() + 1;
    let cell_count = columns * rows;
    let selected_cell = s
        .opening_selected_cell
        .unwrap_or(0)
        .min(cell_count.saturating_sub(1));
    let (selected_width, selected_bottom, _, _, _) = grid_cell_metrics(
        &grid,
        width,
        height,
        s.opening_kind,
        Some(selected_cell),
    );
    let vertical_positions = vertical_position_fields(&grid.vertical, width);
    let horizontal_positions = horizontal_position_fields(&grid.horizontal);
    let vertical_muntin_positions =
        vertical_muntin_position_fields(&grid.vertical_muntins, selected_width);
    let horizontal_muntin_positions =
        horizontal_muntin_position_fields(&grid.horizontal_muntins, selected_bottom);

    let s = &mut app.aec.styles;
    s.opening_selected_cell = Some(selected_cell);
    s.opening_vertical_positions = vertical_positions;
    s.opening_horizontal_positions = horizontal_positions;
    s.opening_vertical_muntin_positions = vertical_muntin_positions;
    s.opening_horizontal_muntin_positions = horizontal_muntin_positions;
}

fn set_vertical_position(weights: &mut [f64], index: usize, position: f64, width: f64) -> bool {
    if index + 1 >= weights.len() || width <= 0.08 || weights.iter().any(|w| !w.is_finite() || *w <= 0.0) {
        return false;
    }
    let total: f64 = weights.iter().sum();
    if total <= 0.0 {
        return false;
    }
    for weight in weights.iter_mut() {
        *weight /= total;
    }
    let start: f64 = weights[..index].iter().sum();
    let span = weights[index] + weights[index + 1];
    let min_fraction = 0.04 / width;
    let boundary = (position / width).clamp(start + min_fraction, start + span - min_fraction);
    weights[index] = boundary - start;
    weights[index + 1] = span - weights[index];
    true
}

fn set_ordered_position(
    values: &mut [f64],
    index: usize,
    position: f64,
    extent: f64,
    min_gap: f64,
) -> bool {
    if index >= values.len() || extent <= 0.08 || values.iter().any(|value| !value.is_finite()) {
        return false;
    }
    let lower = if index == 0 { min_gap } else { values[index - 1] + min_gap };
    let upper = values
        .get(index + 1)
        .map(|next| next - min_gap)
        .unwrap_or(extent - min_gap);
    if upper < lower {
        return false;
    }
    values[index] = position.clamp(lower, upper);
    true
}

fn insert_midpoint_in_largest_interval(values: &mut Vec<f64>, extent: f64, gap: f64) -> bool {
    let mut boundaries = Vec::with_capacity(values.len() + 2);
    boundaries.push(0.0);
    boundaries.extend(values.iter().copied());
    boundaries.push(extent);
    let Some((index, interval)) = boundaries
        .windows(2)
        .enumerate()
        .map(|(index, pair)| (index, pair[1] - pair[0]))
        .max_by(|a, b| a.1.total_cmp(&b.1))
    else {
        return false;
    };
    if interval < gap * 2.0 {
        return false;
    }
    values.insert(index, (boundaries[index] + boundaries[index + 1]) * 0.5);
    true
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
                visible: true,
            },
            None => AecOpeningSlotBuffer {
                slot: *slot,
                source: AecOpeningSlotSource::Generator,
                generator: OpeningGenerator::None,
                block_name: String::new(),
                block_placement: BlockPlacementMode::default(),
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
        };
        profile.slots.insert(buf.slot, geom);
        profile.visibility.insert(buf.slot.key().to_string(), buf.visible);
    }
    profile
}

fn buffers_for_profile(
    default_slots: &HashMap<OpeningComponentSlot, SlotGeometry>,
    style_profile: Option<&OpeningDisplayProfile>,
    profiles: &HashMap<String, OpeningDisplayProfile>,
    plan: Option<&str>,
) -> Vec<AecOpeningSlotBuffer> {
    let mut merged = default_slots.clone();
    for profile in [style_profile, plan.and_then(|name| profiles.get(name))].into_iter().flatten() {
        for (slot, geom) in &profile.slots {
            merged.insert(*slot, geom.clone());
        }
    }
    let mut bufs = slot_buffers_from_map(&merged);
    for buf in &mut bufs {
        buf.visible = plan.and_then(|name| profiles.get(name))
            .and_then(|p| p.visibility.get(buf.slot.key()))
            .or_else(|| style_profile.and_then(|p| p.visibility.get(buf.slot.key())))
            .copied().unwrap_or(true);
    }
    bufs
}

fn apply_current_buffers_to_store(app: &mut OpenCADStudio) {
    if let Some(name) = app.aec.styles.opening_profile_selected.clone() {
        let previous = buffers_for_profile(
            &app.aec.styles.opening_default_slots,
            app.aec.styles.opening_default_profile.as_ref(),
            &app.aec.styles.opening_display_profiles,
            Some(&name),
        );
        let current = &app.aec.styles.opening_slots;
        if slots_from_buffers(current) != slots_from_buffers(&previous)
            || current.iter().any(|b| previous.iter().find(|p| p.slot == b.slot).is_some_and(|p| p.visible != b.visible))
        {
            app.aec.styles.opening_display_profiles.insert(name, profile_from_buffers(current));
        }
    } else {
        let previous = buffers_for_profile(
            &app.aec.styles.opening_default_slots,
            app.aec.styles.opening_default_profile.as_ref(),
            &app.aec.styles.opening_display_profiles,
            None,
        );
        let current = &app.aec.styles.opening_slots;
        if slots_from_buffers(current) != slots_from_buffers(&previous)
            || current.iter().any(|b| previous.iter().find(|p| p.slot == b.slot).is_some_and(|p| p.visible != b.visible))
        {
            app.aec.styles.opening_default_profile = Some(profile_from_buffers(current));
        }
    }
}

/// Detaches the form from plan overrides (used by New / Duplicate): the
/// buffers show the style default again and no plan profile is carried over.
pub(super) fn detach_plan_profiles(app: &mut OpenCADStudio, keep_default_profile: bool) {
    if !keep_default_profile {
        app.aec.styles.opening_default_profile = None;
    }
    app.aec.styles.opening_display_profiles.clear();
    app.aec.styles.opening_profile_selected = None;
    app.aec.styles.opening_slots = buffers_for_profile(
        &app.aec.styles.opening_default_slots,
        app.aec.styles.opening_default_profile.as_ref(),
        &app.aec.styles.opening_display_profiles,
        None,
    );
}

/// Flushes the visible buffers into the profile store (default or plan).
pub(super) fn flush_buffers(app: &mut OpenCADStudio) {
    apply_current_buffers_to_store(app);
}

pub fn draft_opening_style(
    editing_id: Option<String>,
    name: &str,
    parent_id: Option<String>,
    kind: OpeningKind,
    width: f64,
    height: f64,
    sill: f64,
    hinge: LeafOperation,
    frame_thickness: f64,
    opening_angle_deg: f64,
    shape: OpeningShape,
    spring_height: f64,
) -> Result<OpeningStyle, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(crate::t!("name empty").into_owned());
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
            category: crate::modules::aec::engine::style_common::CATEGORY_OPENING.to_string(),
            origin: Default::default(),
            parent: parent_id,
        },
        kind,
        default_width: width,
        default_height: height,
        default_sill: sill.max(0.0),
        hinge,
        parts: OpeningParts {
            fixed_frame: crate::modules::aec::engine::opening_style::FixedFrameDimensions {
                face_width: frame_thickness.max(0.0),
                ..Default::default()
            },
            ..OpeningParts::default()
        },
        looks: crate::modules::aec::engine::opening_style::OpeningLooks::default(),
        grid: Default::default(),
        division_presets: Vec::new(),
        opening_angle_deg,
        shape,
        spring_height,
        suggested_levels: crate::modules::aec::engine::opening_style::OpeningRepresentationLevels::suggested_for(kind),
        suggested_section: crate::modules::aec::engine::opening_style::SectionRoleSwitches::default(),
        family_symbols: OpeningFamilySymbols::default(),
    })
}

/// Bake the kind-based symbol fallback for the manager canvas.
pub fn preview_baked_paths(style: &OpeningStyle, preview_width: f64) -> Vec<BakedPath> {
    preview_baked_paths_for_mode(style, preview_width, AecOpeningPreviewMode::Plan2D)
}

fn push_preview_edge(
    paths: &mut Vec<BakedPath>,
    project: &impl Fn(f64, f64, f64) -> (f64, f64),
    start: [f64; 3],
    end: [f64; 3],
    slot: OpeningComponentSlot,
    look_role: Option<OpeningLookRole>,
) {
    paths.push(BakedPath {
        slot,
        look_role,
        points: vec![
            project(start[0], start[1], start[2]),
            project(end[0], end[1], end[2]),
        ],
        closed: false,
        filled: false,
    });
}

fn push_preview_wire_box(
    paths: &mut Vec<BakedPath>,
    project: &impl Fn(f64, f64, f64) -> (f64, f64),
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    z_min: f64,
    z_max: f64,
    slot: OpeningComponentSlot,
    look_role: Option<OpeningLookRole>,
) {
    if x_max <= x_min || y_max <= y_min || z_max <= z_min {
        return;
    }
    let points = [
        [x_min, y_min, z_min],
        [x_max, y_min, z_min],
        [x_max, y_max, z_min],
        [x_min, y_max, z_min],
        [x_min, y_min, z_max],
        [x_max, y_min, z_max],
        [x_max, y_max, z_max],
        [x_min, y_max, z_max],
    ];
    for (a, b) in [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ] {
        push_preview_edge(paths, project, points[a], points[b], slot, look_role);
    }
}

fn preview_wall_section_bounds(
    half_opening_width: f64,
    sill_z: f64,
    head_z: f64,
    wall_top: f64,
) -> Vec<(f64, f64, f64, f64)> {
    let wall_min_x = -half_opening_width - 0.3;
    let wall_max_x = half_opening_width + 0.3;
    let mut sections = Vec::new();
    if sill_z > 0.0 {
        sections.push((wall_min_x, wall_max_x, 0.0, sill_z));
    }
    sections.push((wall_min_x, -half_opening_width, 0.0, head_z));
    sections.push((half_opening_width, wall_max_x, 0.0, head_z));
    sections.push((wall_min_x, wall_max_x, head_z, wall_top));
    sections
}

fn push_preview_body_edges(
    paths: &mut Vec<BakedPath>,
    body: &cadkernel::brep::Body,
    project: &impl Fn(f64, f64, f64) -> (f64, f64),
    offset_x: f64,
    offset_z: f64,
    slot: OpeningComponentSlot,
    look_role: OpeningLookRole,
) {
    for edge_key in body.edge_keys() {
        if let Some(edge) = body.edges.get(edge_key) {
            if let (Some(start), Some(end)) = (
                body.vertices.get(edge.start),
                body.vertices.get(edge.end),
            ) {
                push_preview_edge(
                    paths,
                    project,
                    [
                        start.point[0] + offset_x,
                        start.point[1],
                        start.point[2] + offset_z,
                    ],
                    [
                        end.point[0] + offset_x,
                        end.point[1],
                        end.point[2] + offset_z,
                    ],
                    slot,
                    Some(look_role),
                );
            }
        }
    }
}

/// Bake geometry for the requested preview mode (Plan2D, Elevation2D, Model3D).
pub fn preview_baked_paths_for_mode(
    style: &OpeningStyle,
    preview_width: f64,
    mode: AecOpeningPreviewMode,
) -> Vec<BakedPath> {
    preview_baked_paths_for_mode_with_library(style, preview_width, mode, None)
}

pub fn preview_baked_paths_for_mode_with_library(
    style: &OpeningStyle,
    preview_width: f64,
    mode: AecOpeningPreviewMode,
    library: Option<&StyleLibrary>,
) -> Vec<BakedPath> {
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
        frame_thickness: resolve_profile_section(
            library,
            style.parts.fixed_frame.profile_id.as_deref(),
            style.parts.fixed_frame.face_width,
            style.parts.fixed_frame.wall_depth,
        )
        .face,
        cross_axis_offset: 0.0,
        wall_y_min: -PREVIEW_WALL_THICKNESS * 0.5,
        wall_y_max: PREVIEW_WALL_THICKNESS * 0.5,
        hinge: style.hinge,
        swing_side: SwingSide::Exterior,
        shape: style.shape,
        spring_height: style.spring_height,
        opening_angle_deg: style.opening_angle_deg,
        kind: style.kind,
    };
    let fixed_frame_section = resolve_profile_section(
        library,
        style.parts.fixed_frame.profile_id.as_deref(),
        style.parts.fixed_frame.face_width,
        style.parts.fixed_frame.wall_depth,
    );
    let leaf_frame_section = resolve_profile_section(
        library,
        style.parts.leaf_frame.profile_id.as_deref(),
        style.parts.leaf_frame.face_width,
        style.parts.leaf_frame.depth,
    );
    let division = project_division_sized(&style.grid, width, style.default_height);
    let division_profile_valid = division
        .profile_id
        .as_deref()
        .and_then(|id| library?.find_section_profile(id))
        .is_some_and(|profile| profile.is_valid());
    let division_section = resolve_profile_section(
        library,
        division.profile_id.as_deref(),
        style.parts.fixed_frame.face_width,
        style.parts.fixed_frame.wall_depth,
    );

    match mode {
        AecOpeningPreviewMode::Plan2D => {
            bake_opening_plan_from_grid(
                params,
                &style.parts,
                library,
                &style.grid,
                style.suggested_levels.plan_frame,
                style.suggested_levels.swing,
                style.suggested_levels.divisions,
            )
        }
        AecOpeningPreviewMode::Section => section_preview_paths(style, library),
        AecOpeningPreviewMode::Elevation2D => {
            bake_opening_elevation_from_grid(
                params,
                &style.parts,
                library,
                &style.grid,
                style.suggested_levels.elevation_frame,
                style.suggested_levels.divisions,
            )
        }
        AecOpeningPreviewMode::Model3D => {
            let iso_proj = |x: f64, y: f64, z: f64| -> (f64, f64) {
                ((x - y) * 0.8660254, (x + y) * 0.25 + z)
            };
            let mut paths = Vec::new();

            let hw = width * 0.5;
            let ht = PREVIEW_WALL_THICKNESS * 0.5;
            let h = style.default_height;
            let sill_z = style.default_sill.max(0.0);
            let head_z = sill_z + h;
            let exterior_sill_section = resolve_profile_section(
                library,
                style.parts.exterior_sill.profile_id.as_deref(),
                style.parts.exterior_sill.thickness,
                style.parts.exterior_sill.thickness,
            );
            let interior_board_section = resolve_profile_section(
                library,
                style.parts.interior_board.profile_id.as_deref(),
                style.parts.interior_board.thickness,
                style.parts.interior_board.thickness,
            );
            let threshold_section = resolve_profile_section(
                library,
                style.parts.threshold.profile_id.as_deref(),
                style.parts.threshold.height,
                style.parts.threshold.width,
            );
            let shutter_section = resolve_profile_section(
                library,
                style.parts.shutter_box.profile_id.as_deref(),
                style.parts.shutter_box.height,
                style.parts.shutter_box.depth,
            );
            let lintel_section = resolve_profile_section(
                library,
                style.parts.lintel.profile_id.as_deref(),
                style.parts.lintel.height,
                PREVIEW_WALL_THICKNESS,
            );
            let shutter = &style.parts.shutter_box;
            let lintel = &style.parts.lintel;
            let wall_top = head_z
                + lintel
                    .height
                    .max(shutter.seat_above_frame + shutter.height)
                    .max(0.0)
                + 0.4;
            for (x_min, x_max, z_min, z_max) in
                preview_wall_section_bounds(hw, sill_z, head_z, wall_top)
            {
                push_preview_wire_box(
                    &mut paths,
                    &iso_proj,
                    x_min,
                    x_max,
                    -ht,
                    ht,
                    z_min,
                    z_max,
                    OpeningComponentSlot::HostCut2D,
                    None,
                );
            }

            if crate::modules::aec::engine::opening_display::opening_component_should_render_with_library(
                &style.parts,
                style.kind,
                OpeningLookRole::ExteriorSill,
                library,
            ) {
                let sill = &style.parts.exterior_sill;
                let (y_min, y_max) =
                    crate::modules::aec::engine::opening_display::exterior_sill_y_bounds(
                        -ht,
                        ht,
                        sill.overhang,
                        exterior_sill_section.depth,
                    );
                push_preview_wire_box(
                    &mut paths,
                    &iso_proj,
                    -hw,
                    hw,
                    y_min,
                    y_max,
                    sill_z - exterior_sill_section.face,
                    sill_z + sill.upstand.max(0.0),
                    OpeningComponentSlot::Solid3D,
                    Some(OpeningLookRole::ExteriorSill),
                );
            }
            if crate::modules::aec::engine::opening_display::opening_component_should_render_with_library(
                &style.parts,
                style.kind,
                OpeningLookRole::InteriorBoard,
                library,
            ) {
                let board = &style.parts.interior_board;
                let (y_min, y_max) =
                    crate::modules::aec::engine::opening_display::interior_board_y_bounds(
                        -ht,
                        ht,
                        board.overhang,
                        interior_board_section.depth,
                    );
                push_preview_wire_box(
                    &mut paths,
                    &iso_proj,
                    -hw,
                    hw,
                    y_min,
                    y_max,
                    sill_z - interior_board_section.face,
                    sill_z + board.upstand.max(0.0),
                    OpeningComponentSlot::Solid3D,
                    Some(OpeningLookRole::InteriorBoard),
                );
            }
            if crate::modules::aec::engine::opening_display::opening_component_should_render_with_library(
                &style.parts,
                style.kind,
                OpeningLookRole::Threshold,
                library,
            ) {
                push_preview_wire_box(
                    &mut paths,
                    &iso_proj,
                    -hw,
                    hw,
                    -threshold_section.depth * 0.5,
                    threshold_section.depth * 0.5,
                    sill_z,
                    sill_z + threshold_section.face,
                    OpeningComponentSlot::Solid3D,
                    Some(OpeningLookRole::Threshold),
                );
            }
            if crate::modules::aec::engine::opening_display::opening_component_should_render_with_library(
                &style.parts,
                style.kind,
                OpeningLookRole::ShutterBox,
                library,
            ) {
                let y_min = -ht + shutter.installation_depth;
                push_preview_wire_box(
                    &mut paths,
                    &iso_proj,
                    -hw,
                    hw,
                    y_min,
                    y_min + shutter_section.depth,
                    head_z + shutter.seat_above_frame,
                    head_z + shutter.seat_above_frame + shutter_section.face,
                    OpeningComponentSlot::Solid3D,
                    Some(OpeningLookRole::ShutterBox),
                );
            }
            if crate::modules::aec::engine::opening_display::opening_component_should_render_with_library(
                &style.parts,
                style.kind,
                OpeningLookRole::Lintel,
                library,
            ) {
                let left = lintel.bearing_left.max(0.0);
                let right = lintel.bearing_right.max(0.0);
                push_preview_wire_box(
                    &mut paths,
                    &iso_proj,
                    -hw - left,
                    hw + right,
                    -lintel_section.depth * 0.5,
                    lintel_section.depth * 0.5,
                    head_z,
                    head_z + lintel_section.face,
                    OpeningComponentSlot::Solid3D,
                    Some(OpeningLookRole::Lintel),
                );
            }

            let ft = fixed_frame_section.face;
            let h = style.default_height;
            let sh = style.spring_height;
            let shape = style.shape;
            let is_door = style.kind == OpeningKind::Door;
            let angle = style.opening_angle_deg;
            let hinge = style.hinge;
            let project_opening = |x: f64, y: f64, z: f64| iso_proj(x, y, z + sill_z);

            if let Some(frame_solid) =
                crate::modules::aec::engine::opening_display::build_opening_frame_3d(
                    width,
                    h,
                    ft,
                    fixed_frame_section.depth,
                    shape,
                    sh,
                )
            {
                push_preview_body_edges(
                    &mut paths,
                    &frame_solid,
                    &project_opening,
                    0.0,
                    0.0,
                    OpeningComponentSlot::Frame3D,
                    OpeningLookRole::FixedFrame,
                );
            }
            if let Some(glazing_solid) =
                crate::modules::aec::engine::opening_display::build_opening_glazing_3d(
                    width, h, ft, shape, sh,
                )
            {
                push_preview_body_edges(
                    &mut paths,
                    &glazing_solid,
                    &project_opening,
                    0.0,
                    0.0,
                    OpeningComponentSlot::Glazing3D,
                    OpeningLookRole::Glazing,
                );
            }
            if let Some(leaf_solid) =
                crate::modules::aec::engine::opening_display::build_opening_leaf_3d_with_depth(
                    width,
                    h,
                    leaf_frame_section.face,
                    leaf_frame_section.depth,
                    angle,
                    hinge,
                    SwingSide::Exterior,
                    is_door,
                    shape,
                    sh,
                )
            {
                push_preview_body_edges(
                    &mut paths,
                    &leaf_solid,
                    &project_opening,
                    0.0,
                    0.0,
                    OpeningComponentSlot::Leaf3D,
                    OpeningLookRole::Leaf,
                );
            }

            for (bar, center_x, center_z) in
                crate::modules::aec::engine::opening_display::build_opening_grid_bars_3d(
                    &style.grid,
                    params,
                    if division_profile_valid {
                        division_section.face
                    } else {
                        fixed_frame_section.face
                    },
                    if division_profile_valid {
                        division_section.depth
                    } else {
                        fixed_frame_section.depth
                    },
                    library,
                    &leaf_frame_section,
                )
            {
                push_preview_body_edges(
                    &mut paths,
                    &bar,
                    &project_opening,
                    center_x,
                    center_z,
                    OpeningComponentSlot::Frame3D,
                    OpeningLookRole::Division,
                );
            }

            paths.retain(|path| {
                path.resolved_look_role().map_or(true, |role| {
                    crate::modules::aec::engine::opening_display::opening_component_should_render_with_library(
                        &style.parts,
                        style.kind,
                        role,
                        library,
                    )
                })
            });
            paths
        }
    }
}

fn resolve_profile_section(
    library: Option<&StyleLibrary>,
    profile_id: Option<&str>,
    fallback_face: f64,
    fallback_depth: f64,
) -> crate::modules::aec::engine::section_profile::ResolvedSection {
    let profile = profile_id.and_then(|id| library?.find_section_profile(id));
    crate::modules::aec::engine::section_profile::resolve_section(
        profile,
        fallback_face,
        fallback_depth,
    )
}

/// Wall drawn above the rough head. Independent of lintel, shutter box and frame.
const SECTION_PREVIEW_ABOVE: f64 = 0.3;

fn section_preview_paths(style: &OpeningStyle, library: Option<&StyleLibrary>) -> Vec<BakedPath> {
    let wall_half = PREVIEW_WALL_THICKNESS * 0.5;
    let sill_z = style.default_sill.max(0.0);
    let head_z = sill_z + style.default_height.max(1e-6);
    let fixed_frame = &style.parts.fixed_frame;
    let frame_section = resolve_profile_section(
        library,
        fixed_frame.profile_id.as_deref(),
        fixed_frame.face_width,
        fixed_frame.wall_depth,
    );
    let frame_depth = frame_section.depth.max(0.0).min(PREVIEW_WALL_THICKNESS);
    let frame_center = fixed_frame.axis_offset;
    let frame_x0 = (frame_center - frame_depth * 0.5).max(-wall_half);
    let frame_x1 = (frame_center + frame_depth * 0.5).min(wall_half);
    let frame_face = frame_section.face.max(0.0);
    let shutter = &style.parts.shutter_box;
    let top_z = head_z + SECTION_PREVIEW_ABOVE;

    let mut paths = Vec::new();
    let mut profile_frame_paths = Vec::new();
    if sill_z > 0.0 {
        paths.push(BakedPath {
            slot: OpeningComponentSlot::HostCut2D,
            look_role: None,
            points: vec![
                (-wall_half, 0.0),
                (wall_half, 0.0),
                (wall_half, sill_z),
                (-wall_half, sill_z),
            ],
            closed: true,
            filled: true,
        });
    }
    paths.push(BakedPath {
        slot: OpeningComponentSlot::HostCut2D,
        look_role: None,
        points: vec![
            (-wall_half, head_z),
            (wall_half, head_z),
            (wall_half, top_z),
            (-wall_half, top_z),
        ],
        closed: true,
        filled: true,
    });
    paths.push(BakedPath {
        slot: OpeningComponentSlot::HostCut2D,
        look_role: None,
        points: vec![(-wall_half, 0.0), (-wall_half, top_z), (wall_half, top_z), (wall_half, 0.0)],
        closed: false,
        filled: false,
    });

    let mut push_rect = |role: OpeningLookRole,
                         slot: OpeningComponentSlot,
                         x0: f64,
                         z0: f64,
                         x1: f64,
                         z1: f64| {
        if x1 - x0 > 1e-9 && z1 - z0 > 1e-9 {
            paths.push(BakedPath {
                slot,
                look_role: Some(role),
                points: vec![(x0, z0), (x1, z0), (x1, z1), (x0, z1)],
                closed: true,
                filled: true,
            });
        }
    };

    if frame_face > 0.0
        && crate::modules::aec::engine::opening_display::opening_component_should_render_with_library(
            &style.parts,
            style.kind,
            OpeningLookRole::FixedFrame,
            library,
        )
    {
        let outline = if frame_section.outline.len() > 1
            && frame_section.outline.first() == frame_section.outline.last()
        {
            &frame_section.outline[..frame_section.outline.len() - 1]
        } else {
            &frame_section.outline
        };
        for origin_z in [sill_z - frame_face, head_z] {
            profile_frame_paths.push(BakedPath {
                slot: OpeningComponentSlot::Frame2D,
                look_role: Some(OpeningLookRole::FixedFrame),
                points: outline
                    .iter()
                    .map(|(face, depth)| (frame_x0 + depth, origin_z + face))
                    .collect(),
                closed: true,
                filled: true,
            });
        }
    }

    let exterior_sill = &style.parts.exterior_sill;
    if opening_component_should_render(
        &style.parts,
        style.kind,
        OpeningLookRole::ExteriorSill,
    ) {
        // The sill seats on the frame's exterior face, not the wall face.
        let frame_outer = frame_x0.min(frame_x1);
        push_rect(
            OpeningLookRole::ExteriorSill,
            OpeningComponentSlot::Sill2D,
            frame_outer - exterior_sill.overhang,
            sill_z - exterior_sill.thickness,
            frame_outer + exterior_sill.thickness,
            sill_z,
        );
        if exterior_sill.upstand > 0.0 {
            push_rect(
                OpeningLookRole::ExteriorSill,
                OpeningComponentSlot::Sill2D,
                frame_outer,
                sill_z,
                frame_outer + exterior_sill.thickness,
                sill_z + exterior_sill.upstand,
            );
        }
    }

    let interior_board = &style.parts.interior_board;
    if opening_component_should_render(
        &style.parts,
        style.kind,
        OpeningLookRole::InteriorBoard,
    ) {
        push_rect(
            OpeningLookRole::InteriorBoard,
            OpeningComponentSlot::Sill2D,
            wall_half - interior_board.thickness,
            sill_z - interior_board.thickness,
            wall_half + interior_board.overhang,
            sill_z,
        );
    }

    let threshold = &style.parts.threshold;
    if opening_component_should_render(&style.parts, style.kind, OpeningLookRole::Threshold) {
        push_rect(
            OpeningLookRole::Threshold,
            OpeningComponentSlot::Threshold2D,
            frame_x0,
            sill_z,
            frame_x1,
            sill_z + threshold.height,
        );
    }

    if opening_component_should_render(&style.parts, style.kind, OpeningLookRole::ShutterBox) {
        let x0 = -wall_half + shutter.installation_depth;
        push_rect(
            OpeningLookRole::ShutterBox,
            OpeningComponentSlot::Frame2D,
            x0,
            head_z + shutter.seat_above_frame,
            x0 + shutter.depth,
            head_z + shutter.seat_above_frame + shutter.height,
        );
    }

    if opening_component_should_render(&style.parts, style.kind, OpeningLookRole::Lintel) {
        let lintel = &style.parts.lintel;
        let lintel_depth = if lintel.depth > 1e-9 {
            lintel.depth
        } else {
            PREVIEW_WALL_THICKNESS
        };
        let lintel_x0 = -wall_half;
        push_rect(
            OpeningLookRole::Lintel,
            OpeningComponentSlot::Frame2D,
            lintel_x0,
            head_z,
            lintel_x0 + lintel_depth,
            head_z + lintel.height,
        );
    }

    paths.extend(profile_frame_paths);
    paths
}

fn load_buffers_from_style(app: &mut OpenCADStudio, style: &OpeningStyle) {
    app.aec.styles.opening_editing_id = Some(style.style.id.clone());
    app.aec.styles.opening_name = style.style.name.clone();
    app.aec.styles.opening_parent = style.style.parent.clone();
    app.aec.styles.opening_kind = style.kind;
    app.aec.styles.opening_width = format_dim(style.default_width);
    app.aec.styles.opening_height = format_dim(style.default_height);
    app.aec.styles.opening_sill = format_dim(style.default_sill);
    load_part_buffers(app, style);
    app.aec.styles.opening_angle = format_dim(style.opening_angle_deg);
    app.aec.styles.opening_spring = if style.shape == OpeningShape::Arch {
        format_dim(style.spring_height)
    } else {
        String::new()
    };
    app.aec.styles.opening_hinge = style.hinge;
    app.aec.styles.opening_shape = style.shape;
    app.aec.styles.opening_default_slots = default_slots_for_kind(style.kind);
    app.aec.styles.opening_default_profile = None;
    app.aec.styles.opening_display_profiles = app
        .aec
        .aec_plan_library
        .as_ref()
        .map(|lib| {
            lib.configs
                .iter()
                .filter_map(|c| {
                    c.style_overrides
                        .get(&style.style.id)
                        .and_then(|o| o.opening.clone())
                        .map(|p| (c.name.clone(), p))
                })
                .collect()
        })
        .unwrap_or_default();
    app.aec.styles.opening_profile_selected = None;
    app.aec.styles.opening_slots = buffers_for_profile(
        &app.aec.styles.opening_default_slots, None, &app.aec.styles.opening_display_profiles, None,
    );
    app.aec.styles.opening_form_open = true;
}


fn apply_size_lock(app: &mut OpenCADStudio, prefer_width: bool) {
    let width = parse_dim(&app.aec.styles.opening_width);
    let height = parse_dim(&app.aec.styles.opening_height);
    match app.aec.styles.opening_shape {
        OpeningShape::Circle => {
            if prefer_width {
                if let Some(w) = width.filter(|v| *v > 0.0) {
                    app.aec.styles.opening_height = format_dim(w);
                }
            } else if let Some(h) = height.filter(|v| *v > 0.0) {
                app.aec.styles.opening_width = format_dim(h);
            }
        }
        OpeningShape::Triangle(
            crate::modules::aec::engine::opening_shape::TriangleVariant::Equilateral,
        ) => {
            if let Some(w) = width.filter(|v| *v > 0.0) {
                app.aec.styles.opening_height =
                    format_dim(OpeningShape::equilateral_height(w));
            }
        }
        OpeningShape::Arch => {
            let spring = parse_dim(&app.aec.styles.opening_spring).unwrap_or(0.0);
            if spring <= 1e-12 {
                if let (Some(w), Some(h)) = (width, height) {
                    if h > 0.0 {
                        app.aec.styles.opening_spring =
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
        self.aec.styles.opening_filter.clear();
        self.aec.styles.opening_selected = None;
        self.aec.styles.opening_editing_id = None;
        self.aec.styles.opening_form_open = false;
        self.aec.styles.opening_slots.clear();
        self.aec.styles.opening_default_slots.clear();
        self.aec.styles.opening_display_profiles.clear();
        self.aec.styles.opening_default_profile = None;
        self.aec.styles.opening_profile_selected = None;
        self.aec.aec_plan_library = Some(
            crate::modules::aec::engine::project::resolve_display_config_library(
                self.aec.aec_project_explorer_file.as_ref(),
            ),
        );
        self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::Styles));
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
        self.aec.styles.opening_selected = Some(id);
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_new(&mut self) -> Task<Message> {
        self.aec.styles.opening_selected = None;
        self.aec.styles.opening_editing_id = None;
        self.aec.styles.opening_name.clear();
        self.aec.styles.opening_parent = None;
        self.aec.styles.opening_kind = OpeningKind::Window;
        let seed = OpeningStyle::standard_window();
        load_buffers_from_style(self, &seed);
        detach_plan_profiles(self, false);
        self.aec.styles.opening_editing_id = None;
        self.aec.styles.opening_name.clear();
        self.aec.styles.opening_form_open = true;
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_kind_changed(&mut self, value: String) -> Task<Message> {
        let kind = OpeningKind::from_str(&value);
        let was_new = self.aec.styles.opening_editing_id.is_none();
        self.aec.styles.opening_kind = kind;
        if !self.aec.styles.opening_editor_tab.applies_to(kind) {
            self.aec.styles.opening_editor_tab = OpeningEditorGroup::all()
                .iter()
                .copied()
                .find(|group| group.applies_to(kind))
                .unwrap_or(OpeningEditorGroup::FixedFrame);
        }
        if was_new {
            let (w, h, sill) = kind.default_dimensions();
            self.aec.styles.opening_width = format_dim(w);
            self.aec.styles.opening_height = format_dim(h);
            self.aec.styles.opening_sill = format_dim(sill);
            self.aec.styles.opening_frame = match kind {
                OpeningKind::Breakthrough => "0".to_string(),
                _ => format_dim(DEFAULT_FRAME_THICKNESS),
            };
            self.aec.styles.opening_angle = match kind {
                OpeningKind::Breakthrough => "0".to_string(),
                _ => format_dim(DEFAULT_OPENING_ANGLE_DEG),
            };
            self.aec.styles.opening_slots = slot_buffers_for_kind(kind);
            self.aec.styles.opening_default_slots =
                slots_from_buffers(&self.aec.styles.opening_slots);
            self.aec.styles.opening_display_profiles.clear();
            self.aec.styles.opening_default_profile = None;
            self.aec.styles.opening_profile_selected = None;
            apply_size_lock(self, true);
        }
        refresh_opening_division_measure_buffers(self);
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_shape_changed(&mut self, value: String) -> Task<Message> {
        self.aec.styles.opening_shape = OpeningShape::from_str(&value);
        apply_size_lock(self, true);
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_width_changed(&mut self, value: String) -> Task<Message> {
        self.aec.styles.opening_width = value;
        apply_size_lock(self, true);
        refresh_opening_division_measure_buffers(self);
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_height_changed(&mut self, value: String) -> Task<Message> {
        self.aec.styles.opening_height = value;
        apply_size_lock(self, false);
        refresh_opening_division_measure_buffers(self);
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_dim_changed(&mut self, key: &str, value: String) {
        if matches!(
            key,
            "vertical" | "horizontal" | "vertical_muntins" | "horizontal_muntins"
        ) || key.contains("position")
        {
            self.aec.styles.opening_authored_grid = None;
        }
        for (prefix, axis) in [
            ("vertical_position", "vertical"),
            ("horizontal_position", "horizontal"),
            ("vertical_muntin_position", "vertical_muntin"),
            ("horizontal_muntin_position", "horizontal_muntin"),
        ] {
            if let Some(index) = key
                .strip_prefix(prefix)
                .and_then(|suffix| suffix.strip_prefix(':'))
                .and_then(|suffix| suffix.parse::<usize>().ok())
            {
                self.aec_opening_style_manager_grid_position_changed(axis, index, value);
                return;
            }
        }
        let old_columns = parse_list(&self.aec.styles.opening_vertical).len().max(1);
        let old_rows = parse_list(&self.aec.styles.opening_horizontal).len() + 1;
        let field = match key {
            "frame" => &mut self.aec.styles.opening_frame,
            "frame_depth" => &mut self.aec.styles.opening_frame_depth,
            "frame_offset" => &mut self.aec.styles.opening_frame_offset,
            "leaf_face" => &mut self.aec.styles.opening_leaf_face,
            "leaf_depth" => &mut self.aec.styles.opening_leaf_depth,
            "leaf_rebate" => &mut self.aec.styles.opening_leaf_rebate,
            "glazing_thickness" => &mut self.aec.styles.opening_glazing_thickness,
            "glazing_inset" => &mut self.aec.styles.opening_glazing_inset,
            "ext_sill_thickness" => &mut self.aec.styles.opening_ext_sill_thickness,
            "ext_sill_overhang" => &mut self.aec.styles.opening_ext_sill_overhang,
            "ext_sill_upstand" => &mut self.aec.styles.opening_ext_sill_upstand,
            "int_board_thickness" => &mut self.aec.styles.opening_int_board_thickness,
            "int_board_overhang" => &mut self.aec.styles.opening_int_board_overhang,
            "int_board_upstand" => &mut self.aec.styles.opening_int_board_upstand,
            "threshold_height" => &mut self.aec.styles.opening_threshold_height,
            "threshold_width" => &mut self.aec.styles.opening_threshold_width,
            "shutter_height" => &mut self.aec.styles.opening_shutter_height,
            "shutter_depth" => &mut self.aec.styles.opening_shutter_depth,
            "shutter_seat" => &mut self.aec.styles.opening_shutter_seat,
            "shutter_install" => &mut self.aec.styles.opening_shutter_install,
            "lintel_height" => &mut self.aec.styles.opening_lintel_height,
            "lintel_left" => &mut self.aec.styles.opening_lintel_left,
            "lintel_right" => &mut self.aec.styles.opening_lintel_right,
            "vertical" => &mut self.aec.styles.opening_vertical,
            "horizontal" => &mut self.aec.styles.opening_horizontal,
            "vertical_muntins" => &mut self.aec.styles.opening_vertical_muntins,
            "horizontal_muntins" => &mut self.aec.styles.opening_horizontal_muntins,
            _ => return,
        };
        *field = value;
        if matches!(key, "vertical" | "horizontal") {
            let columns = parse_list(&self.aec.styles.opening_vertical).len().max(1);
            let rows = parse_list(&self.aec.styles.opening_horizontal).len() + 1;
            resize_opening_cells(
                &mut self.aec.styles.opening_cells,
                old_columns,
                old_rows,
                columns,
                rows,
                default_opening_cell_kind(self.aec.styles.opening_kind),
            );
            self.aec.styles.opening_selected_cell = Some(
                self.aec
                    .styles
                    .opening_selected_cell
                    .unwrap_or(0)
                    .min(columns * rows - 1),
            );
        }
        refresh_opening_division_measure_buffers(self);
    }

    fn aec_opening_style_manager_grid_position_changed(
        &mut self,
        axis: &str,
        index: usize,
        value: String,
    ) {
        let Some(position) = parse_dim(&value) else {
            let fields = match axis {
                "vertical" => &mut self.aec.styles.opening_vertical_positions,
                "horizontal" => &mut self.aec.styles.opening_horizontal_positions,
                "vertical_muntin" => &mut self.aec.styles.opening_vertical_muntin_positions,
                "horizontal_muntin" => &mut self.aec.styles.opening_horizontal_muntin_positions,
                _ => return,
            };
            if let Some(field) = fields.get_mut(index) {
                *field = value;
            }
            return;
        };
        let s = &mut self.aec.styles;
        let width = parse_dim(&s.opening_width).unwrap_or(1.2).max(0.08);
        let height = parse_dim(&s.opening_height).unwrap_or(1.2).max(0.08);
        let grid = opening_grid_from_fields(
            &s.opening_vertical,
            &s.opening_horizontal,
            &s.opening_vertical_muntins,
            &s.opening_horizontal_muntins,
            &s.opening_cells,
        );
        let (cell_width, cell_bottom, _cell_height, min_width, min_height) = grid_cell_metrics(
            &grid,
            width,
            height,
            s.opening_kind,
            s.opening_selected_cell,
        );
        let horizontal_extent = horizontal_division_extent(&grid, height, s.opening_kind);
        match axis {
            "vertical" => {
                let mut weights = grid.vertical;
                if set_vertical_position(&mut weights, index, position, width) {
                    s.opening_vertical = format_vertical_weights(&weights);
                } else {
                    if let Some(field) = s.opening_vertical_positions.get_mut(index) {
                        *field = value;
                    }
                    return;
                }
            }
            "horizontal" => {
                let mut values = grid.horizontal;
                if set_ordered_position(&mut values, index, position, horizontal_extent, 0.04) {
                    s.opening_horizontal = format_list(&values);
                } else {
                    if let Some(field) = s.opening_horizontal_positions.get_mut(index) {
                        *field = value;
                    }
                    return;
                }
            }
            "vertical_muntin" => {
                let mut values = grid.vertical_muntins;
                if set_ordered_position(
                    &mut values,
                    index,
                    position / cell_width,
                    1.0,
                    0.04 / min_width,
                ) {
                    s.opening_vertical_muntins = format_list(&values);
                } else {
                    if let Some(field) = s.opening_vertical_muntin_positions.get_mut(index) {
                        *field = value;
                    }
                    return;
                }
            }
            "horizontal_muntin" => {
                let mut values = grid.horizontal_muntins;
                if set_ordered_position(
                    &mut values,
                    index,
                    position - cell_bottom,
                    min_height,
                    0.04,
                ) {
                    s.opening_horizontal_muntins = format_list(&values);
                } else {
                    if let Some(field) = s.opening_horizontal_muntin_positions.get_mut(index) {
                        *field = value;
                    }
                    return;
                }
            }
            _ => return,
        }
        refresh_opening_division_measure_buffers(self);
    }

    pub(crate) fn aec_opening_style_manager_set_editor_tab(
        &mut self,
        tab: OpeningEditorGroup,
    ) {
        self.aec.styles.opening_editor_tab = tab;
    }

    pub(crate) fn aec_opening_style_manager_component_enabled_changed(
        &mut self,
        group: OpeningEditorGroup,
        enabled: bool,
    ) {
        if group.applies_to(self.aec.styles.opening_kind)
            && !matches!(
                group,
                OpeningEditorGroup::Divisions
                    | OpeningEditorGroup::Symbol
                    | OpeningEditorGroup::Ratings
            )
        {
            self.aec
                .styles
                .opening_component_enabled
                .insert(group, enabled);
        }
    }

    pub(crate) fn aec_opening_style_manager_cell_select(&mut self, index: usize) {
        let columns = parse_list(&self.aec.styles.opening_vertical).len().max(1);
        let rows = parse_list(&self.aec.styles.opening_horizontal).len() + 1;
        self.aec.styles.opening_selected_cell = Some(index.min(columns * rows - 1));
        refresh_opening_division_measure_buffers(self);
    }

    pub(crate) fn aec_opening_style_manager_division_preset(&mut self, name: &str) {
        use crate::modules::aec::engine::construction_grid::{division_preset, CellFill};
        let fill = match self.aec.styles.opening_kind {
            OpeningKind::Door => CellFill::Door,
            _ => CellFill::Glass,
        };
        let Some(grid) = division_preset(name, fill) else {
            return;
        };
        let width = parse_dim(&self.aec.styles.opening_width).unwrap_or(1.2).max(0.08);
        let height = parse_dim(&self.aec.styles.opening_height).unwrap_or(1.2).max(0.08);
        let division = project_division_sized(&grid, width, height);
        self.aec.styles.opening_authored_grid = Some(grid);
        self.aec.styles.opening_vertical = format_vertical_weights(&division.vertical);
        self.aec.styles.opening_horizontal = format_list(&division.horizontal);
        self.aec.styles.opening_vertical_muntins = format_list(&division.vertical_muntins);
        self.aec.styles.opening_horizontal_muntins = format_list(&division.horizontal_muntins);
        self.aec.styles.opening_cells = division
            .cells
            .iter()
            .map(|cell| AecOpeningCellBuffer {
                kind: cell.kind,
                hinge: cell.hinge,
                sill: format_dim(cell.sill),
            })
            .collect();
        self.aec.styles.opening_selected_cell = Some(0);
        refresh_opening_division_measure_buffers(self);
    }

    pub(crate) fn aec_opening_style_manager_division_add(&mut self, axis: &str) {
        self.aec.styles.opening_authored_grid = None;
        let width = parse_dim(&self.aec.styles.opening_width).unwrap_or(1.2).max(0.08);
        let height = parse_dim(&self.aec.styles.opening_height).unwrap_or(1.2).max(0.08);
        let old_columns = parse_list(&self.aec.styles.opening_vertical).len().max(1);
        let old_rows = parse_list(&self.aec.styles.opening_horizontal).len() + 1;
        let grid = opening_grid_from_fields(
            &self.aec.styles.opening_vertical,
            &self.aec.styles.opening_horizontal,
            &self.aec.styles.opening_vertical_muntins,
            &self.aec.styles.opening_horizontal_muntins,
            &self.aec.styles.opening_cells,
        );
        let (_, _, _, min_width, min_height) = grid_cell_metrics(
            &grid,
            width,
            height,
            self.aec.styles.opening_kind,
            self.aec.styles.opening_selected_cell,
        );
        let horizontal_extent = horizontal_division_extent(&grid, height, self.aec.styles.opening_kind);
        let s = &mut self.aec.styles;
        let changed = match axis {
            "vertical" => {
                let mut weights = grid.vertical;
                if weights.is_empty() {
                    weights.push(1.0);
                }
                let total: f64 = weights.iter().sum();
                if weights.len() >= 16 || total <= 0.0 {
                    false
                } else {
                    for weight in &mut weights {
                        *weight /= total;
                    }
                    let index = weights
                        .iter()
                        .enumerate()
                        .max_by(|a, b| a.1.total_cmp(b.1))
                        .map(|(index, _)| index)
                        .unwrap_or(0);
                    let half = weights[index] * 0.5;
                    weights[index] = half;
                    weights.insert(index + 1, half);
                    s.opening_vertical = format_vertical_weights(&weights);
                    true
                }
            }
            "horizontal" => {
                let mut values = grid.horizontal;
                if values.len() >= 15
                    || !insert_midpoint_in_largest_interval(&mut values, horizontal_extent, 0.04)
                {
                    false
                } else {
                    s.opening_horizontal = format_list(&values);
                    true
                }
            }
            "vertical_muntin" => {
                let mut values = grid.vertical_muntins;
                if values.len() >= 15
                    || !insert_midpoint_in_largest_interval(
                        &mut values,
                        1.0,
                        0.04 / min_width,
                    )
                {
                    false
                } else {
                    s.opening_vertical_muntins = format_list(&values);
                    true
                }
            }
            "horizontal_muntin" => {
                let mut values = grid.horizontal_muntins;
                if values.len() >= 15
                    || !insert_midpoint_in_largest_interval(&mut values, min_height, 0.04)
                {
                    false
                } else {
                    s.opening_horizontal_muntins = format_list(&values);
                    true
                }
            }
            _ => false,
        };
        if !changed {
            return;
        }
        let columns = parse_list(&s.opening_vertical).len().max(1);
        let rows = parse_list(&s.opening_horizontal).len() + 1;
        if matches!(axis, "vertical" | "horizontal") {
            resize_opening_cells(
                &mut s.opening_cells,
                old_columns,
                old_rows,
                columns,
                rows,
                default_opening_cell_kind(s.opening_kind),
            );
            s.opening_selected_cell = Some(
                s.opening_selected_cell
                    .unwrap_or(0)
                    .min(columns * rows - 1),
            );
        }
        refresh_opening_division_measure_buffers(self);
    }

    pub(crate) fn aec_opening_style_manager_division_remove(&mut self, axis: &str, index: usize) {
        self.aec.styles.opening_authored_grid = None;
        let old_columns = parse_list(&self.aec.styles.opening_vertical).len().max(1);
        let old_rows = parse_list(&self.aec.styles.opening_horizontal).len() + 1;
        let s = &mut self.aec.styles;
        let changed = match axis {
            "vertical" => {
                let mut weights = parse_list(&s.opening_vertical);
                if index + 1 >= weights.len() {
                    false
                } else {
                    weights[index] += weights[index + 1];
                    weights.remove(index + 1);
                    s.opening_vertical = format_vertical_weights(&weights);
                    true
                }
            }
            "horizontal" => remove_list_value(&mut s.opening_horizontal, index),
            "vertical_muntin" => remove_list_value(&mut s.opening_vertical_muntins, index),
            "horizontal_muntin" => remove_list_value(&mut s.opening_horizontal_muntins, index),
            _ => false,
        };
        if !changed {
            return;
        }
        let columns = parse_list(&s.opening_vertical).len().max(1);
        let rows = parse_list(&s.opening_horizontal).len() + 1;
        if matches!(axis, "vertical" | "horizontal") {
            resize_opening_cells(
                &mut s.opening_cells,
                old_columns,
                old_rows,
                columns,
                rows,
                default_opening_cell_kind(s.opening_kind),
            );
            s.opening_selected_cell = Some(
                s.opening_selected_cell
                    .unwrap_or(0)
                    .min(columns * rows - 1),
            );
        }
        refresh_opening_division_measure_buffers(self);
    }

    pub(crate) fn aec_opening_style_manager_preview_slot_select(
        &mut self,
        role: Option<OpeningLookRole>,
    ) -> Task<Message> {
        self.aec.styles.opening_selected_look_role = role;
        let Some(role) = role else {
            return Task::none();
        };

        self.aec.styles.opening_editor_tab = match role {
            OpeningLookRole::FixedFrame => OpeningEditorGroup::FixedFrame,
            OpeningLookRole::Leaf | OpeningLookRole::Swing => OpeningEditorGroup::Leaf,
            OpeningLookRole::Glazing => OpeningEditorGroup::Glazing,
            OpeningLookRole::ExteriorSill => OpeningEditorGroup::ExteriorSill,
            OpeningLookRole::InteriorBoard => OpeningEditorGroup::InteriorBoard,
            OpeningLookRole::Threshold => OpeningEditorGroup::Threshold,
            OpeningLookRole::ShutterBox => OpeningEditorGroup::ShutterBox,
            OpeningLookRole::Lintel => OpeningEditorGroup::Lintel,
            OpeningLookRole::Division => OpeningEditorGroup::Divisions,
        };
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_look_line_type_changed(
        &mut self,
        role: OpeningLookRole,
        value: String,
    ) {
        self.ensure_aec_catalog_linetype(&value);
        self.aec
            .styles
            .opening_look_buffers
            .entry(role)
            .or_default()
            .line_type = value;
    }

    pub(crate) fn aec_opening_style_manager_look_line_color_changed(
        &mut self,
        role: OpeningLookRole,
        value: String,
    ) {
        self.aec
            .styles
            .opening_look_buffers
            .entry(role)
            .or_default()
            .line_color = value;
    }

    pub(crate) fn aec_opening_style_manager_look_color_picker_toggle(
        &mut self,
        role: OpeningLookRole,
    ) {
        self.aec.styles.opening_look_color_picker_role =
            (self.aec.styles.opening_look_color_picker_role != Some(role)).then_some(role);
    }

    pub(crate) fn aec_opening_style_manager_look_material_changed(
        &mut self,
        role: OpeningLookRole,
        material_id: String,
    ) {
        self.aec
            .styles
            .opening_look_buffers
            .entry(role)
            .or_default()
            .material_id = material_id;
    }

    pub(crate) fn aec_opening_style_manager_look_material_clear(
        &mut self,
        role: OpeningLookRole,
    ) {
        self.aec_opening_style_manager_look_material_changed(role, String::new());
    }

    pub(crate) fn aec_opening_style_manager_cell_kind(&mut self, index: usize, value: &str) {
        use crate::modules::aec::engine::opening_style::OpeningCellKind;
        if self.aec.styles.opening_cells.len() <= index {
            self.aec.styles.opening_cells.resize_with(index + 1, || {
                AecOpeningCellBuffer {
                    kind: OpeningCellKind::Glass,
                    hinge: None,
                    sill: "0".to_string(),
                }
            });
        }
        if let Some(cell) = self.aec.styles.opening_cells.get_mut(index) {
            cell.kind = OpeningCellKind::from_str(value);
            if !cell.kind.permits_hinge() {
                cell.hinge = None;
            }
            sync_authored_cell(self, index);
        }
        self.aec_opening_style_manager_cell_select(index);
        refresh_opening_division_measure_buffers(self);
    }

    pub(crate) fn aec_opening_style_manager_cell_hinge(&mut self, index: usize, value: &str) {
        if self.aec.styles.opening_cells.len() <= index {
            self.aec.styles.opening_cells.resize_with(index + 1, || {
                AecOpeningCellBuffer {
                    kind: crate::modules::aec::engine::opening_style::OpeningCellKind::Glass,
                    hinge: None,
                    sill: "0".to_string(),
                }
            });
        }
        if let Some(cell) = self.aec.styles.opening_cells.get_mut(index) {
            cell.hinge = if value.is_empty() || value == "-" {
                None
            } else {
                Some(LeafOperation::from_str(value))
            };
            sync_authored_cell(self, index);
        }
        self.aec_opening_style_manager_cell_select(index);
    }

    pub(crate) fn aec_opening_style_manager_cell_sill(&mut self, index: usize, value: String) {
        if let Some(cell) = self.aec.styles.opening_cells.get_mut(index) {
            cell.sill = value;
            sync_authored_cell(self, index);
        }
        refresh_opening_division_measure_buffers(self);
    }

    pub(crate) fn aec_opening_style_manager_cell_add(&mut self) {
        use crate::modules::aec::engine::opening_style::OpeningCellKind;
        self.aec.styles.opening_cells.push(AecOpeningCellBuffer {
            kind: OpeningCellKind::Glass,
            hinge: None,
            sill: "0".to_string(),
        });
    }

    pub(crate) fn aec_opening_style_manager_cell_remove(&mut self, index: usize) {
        if index < self.aec.styles.opening_cells.len() {
            self.aec.styles.opening_cells.remove(index);
        }
    }

    pub(crate) fn aec_opening_style_manager_family_block(&mut self, family: &str, name: String) {
        match family {
            "plan" => self.aec.styles.opening_plan_block = name,
            "section" => self.aec.styles.opening_section_block = name,
            "elevation" => self.aec.styles.opening_elevation_block = name,
            _ => {}
        }
    }

    pub(crate) fn aec_opening_style_manager_family_placement(&mut self, family: &str, value: &str) {
        let placement = FamilyPlacement::from_str(value);
        match family {
            "plan" => self.aec.styles.opening_plan_placement = placement,
            "section" => self.aec.styles.opening_section_placement = placement,
            "elevation" => self.aec.styles.opening_elevation_placement = placement,
            _ => {}
        }
    }

    pub(crate) fn aec_opening_style_manager_slot_source_changed(
        &mut self,
        index: usize,
        value: String,
    ) -> Task<Message> {
        if let Some(slot) = self.aec.styles.opening_slots.get_mut(index) {
            slot.source = if value.eq_ignore_ascii_case("Block") {
                AecOpeningSlotSource::Block
            } else {
                AecOpeningSlotSource::Generator
            };
        }
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_slot_generator_changed(
        &mut self,
        index: usize,
        value: String,
    ) -> Task<Message> {
        if let Some(slot) = self.aec.styles.opening_slots.get_mut(index) {
            slot.generator = OpeningGenerator::from_str(&value);
            slot.source = AecOpeningSlotSource::Generator;
        }
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_slot_block_name_changed(
        &mut self,
        index: usize,
        value: String,
    ) -> Task<Message> {
        if let Some(slot) = self.aec.styles.opening_slots.get_mut(index) {
            slot.block_name = value;
            slot.source = AecOpeningSlotSource::Block;
        }
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_slot_block_placement_changed(
        &mut self,
        index: usize,
        value: String,
    ) -> Task<Message> {
        if let Some(slot) = self.aec.styles.opening_slots.get_mut(index) {
            slot.block_placement = match value.as_str() {
                "JambPair" => BlockPlacementMode::JambPair,
                "CenterAnchor" => BlockPlacementMode::CenterAnchor,
                _ => BlockPlacementMode::StretchToFit,
            };
            slot.source = AecOpeningSlotSource::Block;
        }
        Task::none()
    }

    pub(crate) fn aec_opening_style_from_form(&self) -> Result<OpeningStyle, String> {
        let width = parse_dim(&self.aec.styles.opening_width).ok_or("width")?;
        let height = parse_dim(&self.aec.styles.opening_height).ok_or("height")?;
        let sill = parse_dim(&self.aec.styles.opening_sill).unwrap_or(0.0);
        let frame = parse_dim(&self.aec.styles.opening_frame).unwrap_or(0.0);
        let angle = parse_dim(&self.aec.styles.opening_angle).unwrap_or(0.0);
        let spring = parse_dim(&self.aec.styles.opening_spring).unwrap_or(0.0);
        let style = draft_opening_style(
            self.aec.styles.opening_editing_id.clone(),
            &self.aec.styles.opening_name,
            self.aec.styles.opening_parent.clone(),
            self.aec.styles.opening_kind,
            width,
            height,
            sill,
            self.aec.styles.opening_hinge,
            frame,
            angle,
            self.aec.styles.opening_shape,
            spring,
        )?;
        let mut style = style;
        style.parts = parts_from_buffers(self);
        style.looks = opening_looks_from_buffers(&self.aec.styles.opening_look_buffers);
        let division = grid_from_buffers(self, height)?;
        style.grid = self
            .aec
            .styles
            .opening_authored_grid
            .clone()
            .unwrap_or_else(|| construction_from_division(&division, width, height));
        style.family_symbols = OpeningFamilySymbols {
            plan: symbol_from(
                &self.aec.styles.opening_plan_block,
                self.aec.styles.opening_plan_placement,
            ),
            section: symbol_from(
                &self.aec.styles.opening_section_block,
                self.aec.styles.opening_section_placement,
            ),
            elevation: symbol_from(
                &self.aec.styles.opening_elevation_block,
                self.aec.styles.opening_elevation_placement,
            ),
        };
        Ok(style)
    }

    pub(crate) fn aec_opening_style_manager_profile_select(
        &mut self,
        value: String,
    ) -> Task<Message> {
        apply_current_buffers_to_store(self);
        self.aec.styles.opening_profile_selected = if value.is_empty() {
            None
        } else {
            Some(value)
        };
        self.aec.styles.opening_slots = buffers_for_profile(
            &self.aec.styles.opening_default_slots,
            self.aec.styles.opening_default_profile.as_ref(),
            &self.aec.styles.opening_display_profiles,
            self.aec.styles.opening_profile_selected.as_deref(),
        );
        Task::none()
    }

    /// Drops the named plan type's exception from the form (written on save).
    pub(crate) fn aec_opening_style_manager_profile_remove(
        &mut self,
        name: String,
    ) -> Task<Message> {
        self.aec.styles.opening_display_profiles.remove(&name);
        if self.aec.styles.opening_profile_selected.as_deref() == Some(name.as_str()) {
            self.aec.styles.opening_profile_selected = None;
            self.aec.styles.opening_slots = buffers_for_profile(
                &self.aec.styles.opening_default_slots,
                self.aec.styles.opening_default_profile.as_ref(),
                &self.aec.styles.opening_display_profiles,
                None,
            );
        }
        Task::none()
    }

    pub(crate) fn aec_opening_style_manager_slot_visible(
        &mut self,
        index: usize,
        visible: bool,
    ) -> Task<Message> {
        if let Some(slot) = self.aec.styles.opening_slots.get_mut(index) {
            slot.visible = visible;
        }
        Task::none()
    }



    pub(crate) fn aec_opening_style_manager_save(&mut self) -> Task<Message> {
        self.aec_opening_style_manager_save_internal();
        Task::none()
    }

    /// Writes the buffered plan-kind overrides of the edited opening style
    /// into the plan library (`DisplayConfig::style_overrides[style_id].opening`).
    fn aec_opening_style_manager_write_plan_overrides(&mut self, style_id: &str) {
        let profiles = self.aec.styles.opening_display_profiles.clone();
        let Some(lib) = self.aec.aec_plan_library.as_mut() else {
            return;
        };
        let mut changed = false;
        for cfg in &mut lib.configs {
            let new = profiles
                .get(&cfg.name)
                .filter(|p| !p.slots.is_empty() || !p.visibility.is_empty())
                .cloned();
            let current = cfg.style_overrides.get(style_id).and_then(|o| o.opening.clone());
            if new != current {
                cfg.style_overrides
                    .entry(style_id.to_string())
                    .or_default()
                    .opening = new;
                changed = true;
            }
        }
        if !changed {
            return;
        }
        let snapshot = lib.clone();
        if let Err(e) = self.aec_save_display_config_library_preferring_project(&snapshot) {
            self.command_line.push_error(
                crate::tf!("AEC Style Manager: failed to save library: {e}").as_ref(),
            );
        }
    }

    pub(crate) fn aec_opening_style_manager_save_internal(
        &mut self,
    ) -> Option<(String, crate::modules::aec::engine::library::StyleLibrary)> {
        apply_current_buffers_to_store(self);
        let style = match self.aec_opening_style_from_form() {
            Ok(style) => style,
            Err(err) => {
                if self.aec.styles.opening_name.trim().is_empty() {
                    self.command_line.push_error(
                        crate::t!("AEC Style Manager: opening style name cannot be empty.")
                            .as_ref(),
                    );
                } else {
                    self.command_line.push_error(
                        crate::tf!("AEC Style Manager: opening grid is invalid: {err}").as_ref(),
                    );
                }
                return None;
            }
        };

        if let (Some(parent_id), Some(lib)) = (
            style.style.parent.as_ref(),
            self.aec.aec_style_library.as_ref(),
        ) {
            if parent_id == &style.style.id {
                self.command_line.push_error(
                    crate::t!("AEC Style Manager: an opening style cannot be its own parent.")
                        .as_ref(),
                );
                return None;
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
                return None;
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
                    self.aec.styles.opening_selected = Some(id.clone());
                    self.aec.styles.opening_editing_id = Some(id.clone());
                    self.aec_opening_style_manager_write_plan_overrides(&id);
                    let lib_snapshot = self
                        .aec
                        .aec_style_library
                        .clone()
                        .unwrap_or_else(crate::modules::aec::engine::library::StyleLibrary::empty);
                    Some((id, lib_snapshot))
                }
                Err(e) => {
                    self.command_line.push_error(
                        crate::tf!("AEC Style Manager: failed to save library: {e}").as_ref(),
                    );
                    None
                }
            }
        } else {
            self.aec_upsert_opening_style_into_session(style);
            self.aec.styles.opening_selected = Some(id.clone());
            self.aec.styles.opening_editing_id = Some(id.clone());
            self.aec_opening_style_manager_write_plan_overrides(&id);
            let lib_snapshot = self
                .aec
                .aec_style_library
                .clone()
                .unwrap_or_else(crate::modules::aec::engine::library::StyleLibrary::empty);
            Some((id, lib_snapshot))
        }
    }

    pub(crate) fn aec_opening_style_manager_delete(&mut self) -> Task<Message> {
        if let Some(id) = self.aec.styles.opening_selected.clone() {
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
        self.aec.styles.opening_selected = None;
        self.aec.styles.opening_editing_id = None;
        self.aec.styles.opening_form_open = false;
        Task::none()
    }

    pub(crate) fn aec_handle_copy_opening_style(&mut self, to_project: bool) -> Task<Message> {
        if to_project && self.aec.aec_project_explorer_file.is_none() {
            return Task::none();
        }
        let Some(id) = self.aec.styles.opening_selected.clone() else {
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
                self.aec.styles.pending_copy = Some(AecPendingCopy::OpeningStyle {
                    opening_style,
                    to_project,
                });
                self.aec.styles.copy_conflict_open = true;
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

    #[test]
    fn parts_grid_and_family_block_persist() {
        use crate::app::{AecMessage, OpenCADStudio};
        use crate::modules::aec::engine::opening_style::{FamilyPlacement, OpeningCellKind};

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let mut style = OpeningStyle::standard_window();
        style.style.id = "t_opening_parts".into();
        style.style.name = "T Parts".into();
        app.aec_upsert_opening_style_into_session(style.clone());
        let _ = app.aec_opening_style_manager_select(style.style.id.clone());
        let _ = app.update_aec(AecMessage::AecOpeningStyleManagerComponentEnabledChanged(
            OpeningEditorGroup::FixedFrame,
            false,
        ));
        let _ = app.update_aec(AecMessage::AecOpeningStyleManagerDimChanged(
            "frame".into(),
            "0.08".into(),
        ));
        let _ = app.update_aec(AecMessage::AecOpeningStyleManagerDimChanged(
            "vertical".into(),
            "0.333, 0.667".into(),
        ));
        let _ = app.update_aec(AecMessage::AecOpeningStyleManagerDimChanged(
            "vertical_position:0".into(),
            "0.7".into(),
        ));
        assert_eq!(app.aec.styles.opening_vertical_positions, vec!["0.7"]);
        let _ = app.update_aec(AecMessage::AecOpeningStyleManagerCellKindChanged(
            0,
            OpeningCellKind::DoorLeaf.as_str().into(),
        ));
        let _ = app.update_aec(AecMessage::AecOpeningStyleManagerCellSelect(1));
        let _ = app.update_aec(AecMessage::AecOpeningStyleManagerCellKindChanged(
            1,
            OpeningCellKind::DoorLeaf.as_str().into(),
        ));
        let _ = app.update_aec(AecMessage::AecOpeningStyleManagerCellHingeChanged(
            1,
            "Right".into(),
        ));
        assert_eq!(app.aec.styles.opening_selected_cell, Some(1));
        let _ = app.update_aec(AecMessage::AecOpeningStyleManagerFamilyBlockChanged(
            "plan".into(),
            "Fenster".into(),
        ));
        let _ = app.update_aec(AecMessage::AecOpeningStyleManagerFamilyPlacementChanged(
            "plan".into(),
            FamilyPlacement::JambPair.as_str().into(),
        ));
        app.aec_opening_style_manager_look_line_type_changed(
            OpeningLookRole::FixedFrame,
            "CENTER".into(),
        );
        app.aec_opening_style_manager_look_line_color_changed(
            OpeningLookRole::FixedFrame,
            crate::modules::aec::ui::aec_ui_util::acad_color_to_editor_string(
                acadrust::types::Color::Index(3),
            ),
        );
        app.aec_opening_style_manager_look_material_changed(
            OpeningLookRole::FixedFrame,
            "frame-material".into(),
        );
        let _ = app.update_aec(AecMessage::AecOpeningStyleManagerSave);

        let stored = app
            .aec
            .aec_style_library
            .as_ref()
            .unwrap()
            .find_opening_style(&style.style.id)
            .unwrap();
        assert!((stored.parts.fixed_frame.face_width - 0.08).abs() < 1e-9);
        assert!(!stored.parts.fixed_frame.enabled);
        let stored_grid = crate::modules::aec::engine::opening_style::project_division(&stored.grid);
        assert!((stored_grid.vertical[0] - 0.5833).abs() < 1e-4);
        assert!((stored_grid.vertical.iter().sum::<f64>() - 1.0).abs() < 1e-6);
        assert_eq!(stored_grid.cells.len(), 2);
        assert_eq!(stored_grid.cells[0].kind, OpeningCellKind::DoorLeaf);
        assert_eq!(stored_grid.cells[1].kind, OpeningCellKind::DoorLeaf);
        assert_eq!(stored_grid.cells[1].hinge, Some(crate::modules::aec::engine::opening_style::LeafOperation::right()));
        let symbol = stored.family_symbols.plan.as_ref().unwrap();
        assert_eq!(symbol.block_name, "Fenster");
        assert_eq!(symbol.placement, FamilyPlacement::JambPair);
        assert_eq!(stored.looks.fixed_frame.line_type.as_deref(), Some("CENTER"));
        assert_eq!(
            stored.looks.fixed_frame.line_color,
            Some(acadrust::types::Color::Index(3))
        );
        assert_eq!(
            stored.looks.fixed_frame.material_id.as_deref(),
            Some("frame-material")
        );
        assert!(stored.default_width != stored.parts.fixed_frame.face_width);
    }

    #[test]
    fn opening_look_buffers_round_trip_without_changing_frame_dimensions() {
        let mut style = OpeningStyle::standard_window();
        style.parts.fixed_frame.face_width = 0.073;
        style.looks.fixed_frame.line_type = Some("CENTER".to_string());
        style.looks.fixed_frame.line_color = Some(acadrust::types::Color::Index(3));
        style.looks.fixed_frame.material_id = Some("frame-material".to_string());
        style.looks.swing.line_color = Some(acadrust::types::Color::ByLayer);

        let buffers = opening_look_buffers_from_style(&style);
        let round_tripped = opening_looks_from_buffers(&buffers);
        assert_eq!(round_tripped, style.looks);

        let mut saved = style.clone();
        saved.looks = round_tripped;
        assert_eq!(saved.parts.fixed_frame.face_width, 0.073);
    }

    #[test]
    fn profile_ids_round_trip_through_opening_style_editor_buffers() {
        use crate::app::OpenCADStudio;
        use crate::modules::aec::engine::opening_style::OpeningProfilePart as Part;

        let mut app = OpenCADStudio::new_for_test();
        let mut style = OpeningStyle::standard_window();
        style.parts.fixed_frame.profile_id = Some("frame-profile".into());
        style.parts.leaf_frame.profile_id = Some("leaf-profile".into());
        style.parts.exterior_sill.profile_id = Some("sill-profile".into());
        style.parts.interior_board.profile_id = Some("board-profile".into());
        style.parts.threshold.profile_id = Some("threshold-profile".into());
        style.parts.shutter_box.profile_id = Some("box-profile".into());
        style.parts.lintel.profile_id = Some("lintel-profile".into());
        style.grid.muntin_profile_id = Some("division-profile".into());

        load_part_buffers(&mut app, &style);
        let parts = parts_from_buffers(&app);
        let grid = grid_from_buffers(&app, style.default_height).expect("buffered grid");
        assert_eq!(parts.fixed_frame.profile_id.as_deref(), Some("frame-profile"));
        assert_eq!(parts.leaf_frame.profile_id.as_deref(), Some("leaf-profile"));
        assert_eq!(parts.exterior_sill.profile_id.as_deref(), Some("sill-profile"));
        assert_eq!(parts.interior_board.profile_id.as_deref(), Some("board-profile"));
        assert_eq!(parts.threshold.profile_id.as_deref(), Some("threshold-profile"));
        assert_eq!(parts.shutter_box.profile_id.as_deref(), Some("box-profile"));
        assert_eq!(parts.lintel.profile_id.as_deref(), Some("lintel-profile"));
        assert_eq!(grid.profile_id.as_deref(), Some("division-profile"));
        assert_eq!(app.aec.styles.opening_profile_ids.get(&Part::Division).unwrap(), "division-profile");
    }

    #[test]
    fn preview_role_select_switches_to_its_component_group() {
        use crate::app::OpenCADStudio;

        let mut app = OpenCADStudio::new_for_test();
        app.aec.styles.opening_editor_tab = OpeningEditorGroup::FixedFrame;
        let _ = app.aec_opening_style_manager_preview_slot_select(Some(OpeningLookRole::Swing));
        assert_eq!(app.aec.styles.opening_editor_tab, OpeningEditorGroup::Leaf);
        assert_eq!(app.aec.styles.opening_selected_look_role, Some(OpeningLookRole::Swing));

        app.aec.styles.opening_editor_tab = OpeningEditorGroup::ShutterBox;
        let _ = app.aec_opening_style_manager_preview_slot_select(Some(OpeningLookRole::Lintel));
        assert_eq!(app.aec.styles.opening_editor_tab, OpeningEditorGroup::Lintel);
        assert_eq!(app.aec.styles.opening_selected_look_role, Some(OpeningLookRole::Lintel));

        let _ = app.aec_opening_style_manager_preview_slot_select(None);
        assert_eq!(app.aec.styles.opening_selected_look_role, None);
        assert_eq!(app.aec.styles.opening_editor_tab, OpeningEditorGroup::Lintel);
    }

    #[test]
    fn section_preview_contains_the_opening_component_roles() {
        use crate::modules::aec::engine::opening_style::{
            LintelDimensions, ShutterBoxDimensions, SillDimensions, ThresholdDimensions,
        };

        let mut style = OpeningStyle::standard_window();
        style.parts.exterior_sill = SillDimensions {
            enabled: true,
            profile_id: None,
            thickness: 0.04,
            overhang: 0.08,
            upstand: 0.02,
        };
        style.parts.interior_board = SillDimensions {
            enabled: true,
            profile_id: None,
            thickness: 0.03,
            overhang: 0.12,
            upstand: 0.01,
        };
        style.parts.shutter_box = ShutterBoxDimensions {
            enabled: true,
            profile_id: None,
            height: 0.18,
            depth: 0.22,
            seat_above_frame: 0.03,
            installation_depth: 0.01,
        };
        style.parts.lintel = LintelDimensions {
            height: 0.20,
            ..Default::default()
        };

        let paths = preview_baked_paths_for_mode(&style, style.default_width, AecOpeningPreviewMode::Section);
        let roles: std::collections::HashSet<_> = paths
            .iter()
            .filter_map(BakedPath::resolved_look_role)
            .collect();
        for role in [
            OpeningLookRole::FixedFrame,
            OpeningLookRole::ExteriorSill,
            OpeningLookRole::InteriorBoard,
            OpeningLookRole::ShutterBox,
            OpeningLookRole::Lintel,
        ] {
            assert!(roles.contains(&role), "missing section preview role {role:?}");
        }

        let mut door = OpeningStyle::standard_door();
        door.parts.threshold = ThresholdDimensions {
            enabled: true,
            profile_id: None,
            height: 0.025,
            width: 0.12,
        };
        door.parts.interior_board.thickness = 0.03;
        door.parts.lintel.height = 0.20;
        let paths = preview_baked_paths_for_mode(&door, door.default_width, AecOpeningPreviewMode::Section);
        let door_roles: std::collections::HashSet<_> = paths
            .iter()
            .filter_map(BakedPath::resolved_look_role)
            .collect();
        assert!(door_roles.contains(&OpeningLookRole::Threshold));
        assert!(door_roles.contains(&OpeningLookRole::InteriorBoard));
        assert!(!door_roles.contains(&OpeningLookRole::ExteriorSill));
        assert!(!door_roles.contains(&OpeningLookRole::ShutterBox));
    }

    #[test]
    fn section_preview_wall_stays_on_the_rough_opening() {
        let mut short = OpeningStyle::standard_window();
        short.parts.lintel.height = 0.12;
        short.parts.fixed_frame.axis_offset = 0.04;
        let mut tall = short.clone();
        tall.parts.lintel.height = 0.8;
        tall.parts.shutter_box.enabled = true;
        tall.parts.shutter_box.height = 0.5;
        tall.parts.shutter_box.seat_above_frame = 0.2;

        let head = short.default_sill + short.default_height;
        let wall_top = |style: &OpeningStyle| {
            preview_baked_paths_for_mode(style, style.default_width, AecOpeningPreviewMode::Section)
                .into_iter()
                .filter(|path| path.slot == OpeningComponentSlot::HostCut2D)
                .flat_map(|path| path.points)
                .map(|(_, z)| z)
                .fold(0.0_f64, f64::max)
        };
        assert!((wall_top(&short) - (head + 0.3)).abs() < 1e-9);
        assert!((wall_top(&tall) - wall_top(&short)).abs() < 1e-9);

        let lintel = preview_baked_paths_for_mode(
            &short,
            short.default_width,
            AecOpeningPreviewMode::Section,
        )
        .into_iter()
        .find(|path| path.resolved_look_role() == Some(OpeningLookRole::Lintel))
        .expect("lintel");
        let lintel_x0 = lintel.points.iter().map(|(x, _)| *x).fold(f64::INFINITY, f64::min);
        assert!((lintel_x0 + PREVIEW_WALL_THICKNESS * 0.5).abs() < 1e-9);
    }

    #[test]
    fn duplicate_and_new_do_not_inherit_plan_overrides() {
        use crate::app::{AecMessage, OpenCADStudio};
        use crate::modules::aec::engine::library::DisplayConfigLibrary;
        use crate::modules::aec::engine::plan_view::{DisplayConfig, PlanningStage, ViewType};

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let mut style = OpeningStyle::standard_window();
        style.style.id = "t_opening_dup".into();
        style.style.name = "T Dup".into();
        let slot = OpeningComponentSlot::Frame2D;
        app.aec_upsert_opening_style_into_session(style.clone());
        let mut plans = DisplayConfigLibrary::empty();
        let mut plan = DisplayConfig::new("Dup plan".into(), "A".into(), PlanningStage::Execution, ViewType::FloorPlan);
        let mut over = OpeningDisplayProfile::default();
        over.slots.insert(slot, SlotGeometry::Generator(OpeningGenerator::DoorFrame));
        over.visibility.insert(slot.key().into(), false);
        plan.style_overrides.entry(style.style.id.clone()).or_default().opening = Some(over);
        plans.configs.push(plan);
        app.aec.aec_plan_library = Some(plans);

        let _ = app.aec_opening_style_manager_select(style.style.id.clone());
        let _ = app.update_aec(AecMessage::AecOpeningStyleManagerDuplicate);
        let _ = app.update_aec(AecMessage::AecOpeningStyleManagerSave);
        let lib = app.aec.aec_style_library.as_ref().unwrap();
        let copy = lib.opening_styles.iter().find(|s| s.style.name == "T Dup (Kopie)").unwrap();
        let copy_id = copy.style.id.clone();
        let cfg = app.aec.aec_plan_library.as_ref().unwrap().find("Dup plan").unwrap();
        assert!(cfg.style_overrides.get(&copy_id).and_then(|o| o.opening.as_ref()).is_none());
        assert!(cfg.style_overrides[&style.style.id].opening.is_some());

        // New does not inherit the standard window's plan overrides.
        let std_id = OpeningStyle::standard_window().style.id;
        let cfg = app.aec.aec_plan_library.as_mut().unwrap().configs.first_mut().unwrap();
        let mut over = OpeningDisplayProfile::default();
        over.visibility.insert(slot.key().into(), false);
        cfg.style_overrides.entry(std_id).or_default().opening = Some(over);
        let _ = app.update_aec(AecMessage::AecOpeningStyleManagerNew);
        assert!(app.aec.styles.opening_display_profiles.is_empty());
        assert!(app.aec.styles.opening_default_profile.is_none());
        assert!(app.aec.styles.opening_plan_block.is_empty());
        app.aec.styles.opening_name = "T New".into();
        let _ = app.update_aec(AecMessage::AecOpeningStyleManagerSave);
        let new_id = app.aec.aec_style_library.as_ref().unwrap().opening_styles.iter()
            .find(|s| s.style.name == "T New").unwrap().style.id.clone();
        let cfg = app.aec.aec_plan_library.as_ref().unwrap().find("Dup plan").unwrap();
        assert!(cfg.style_overrides.get(&new_id).and_then(|o| o.opening.as_ref()).is_none());
    }
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
            LeafOperation::left(),
            0.06,
            90.0,
            OpeningShape::Circle,
            0.0,
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
            LeafOperation::left(),
            0.06,
            90.0,
            OpeningShape::Triangle(
                crate::modules::aec::engine::opening_shape::TriangleVariant::Equilateral,
            ),
            0.0,
        )
        .unwrap();
        let expected = OpeningShape::equilateral_height(2.0);
        assert!((style.default_height - expected).abs() < 1e-12);
    }

    #[test]
    fn frame_thickness_independent_of_preview_width() {
        let mut style = OpeningStyle::standard_window();
        style.parts.fixed_frame.face_width = DEFAULT_FRAME_THICKNESS;
        let narrow = preview_baked_paths(&style, 1.2);
        let wide = preview_baked_paths(&style, 1.8);
        let post_width = |paths: &[BakedPath]| {
            let frame: Vec<_> = paths
                .iter()
                .filter(|p| p.slot == OpeningComponentSlot::Frame2D && p.closed)
                .collect();
            assert!(frame.len() >= 2);
            let left_w = (frame[0].points[1].0 - frame[0].points[0].0).abs();
            let right_w = (frame[1].points[1].0 - frame[1].points[0].0).abs();
            assert!((left_w - right_w).abs() < 1e-9);
            left_w
        };
        assert!((post_width(&narrow) - DEFAULT_FRAME_THICKNESS).abs() < 1e-9);
        assert!((post_width(&wide) - DEFAULT_FRAME_THICKNESS).abs() < 1e-9);
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
            LeafOperation::left(),
            0.06,
            90.0,
            OpeningShape::Rectangle,
            0.0,
        )
        .is_err());
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
        let loaded = buffers_for_profile(&defaults, None, &profiles, Some("Ausführung"));
        let sill = loaded
            .iter()
            .find(|b| b.slot == OpeningComponentSlot::Sill2D)
            .unwrap();
        assert!(!sill.visible);
        let default_loaded = buffers_for_profile(&defaults, None, &profiles, None);
        let sill_default = default_loaded
            .iter()
            .find(|b| b.slot == OpeningComponentSlot::Sill2D)
            .unwrap();
        assert!(sill_default.visible);
    }

    #[test]
    fn preview_baked_paths_for_mode_plan_elevation_and_3d() {
        let mut style = OpeningStyle::standard_window();
        let mut legacy = DivisionGrid::default();
        legacy.vertical = vec![0.5, 0.5];
        legacy.vertical_muntins = vec![0.5];
        legacy.cells = vec![OpeningCell::default(), OpeningCell::default()];
        style.grid = construction_from_division(&legacy, 1.2, style.default_height);
        style.suggested_levels.divisions =
            crate::modules::aec::engine::opening_style::DivisionLevel::PostsAndMuntins;
        assert_eq!(
            project_division_sized(&style.grid, 1.2, style.default_height)
                .layout(1.2, style.default_height, style.kind)
                .expect("valid grid")
                .len(),
            2
        );
        let plan_paths = preview_baked_paths_for_mode(&style, 1.2, AecOpeningPreviewMode::Plan2D);
        assert!(plan_paths.iter().any(|p| p.slot == OpeningComponentSlot::Frame2D));
        assert!(plan_paths.iter().all(|path| !path.slot.is_elevation()));
        assert!(plan_paths
            .iter()
            .any(|path| path.resolved_look_role() == Some(OpeningLookRole::Division)),
            "{plan_paths:#?}"
        );

        let elev_paths = preview_baked_paths_for_mode(&style, 1.2, AecOpeningPreviewMode::Elevation2D);
        assert!(elev_paths.iter().any(|p| p.slot == OpeningComponentSlot::ElevationContour2D || p.slot == OpeningComponentSlot::ElevationSwing2D));
        assert!(elev_paths.iter().all(|path| path.slot.is_elevation()));

        let model_paths = preview_baked_paths_for_mode(&style, 1.2, AecOpeningPreviewMode::Model3D);
        assert!(model_paths.iter().any(|p| p.slot == OpeningComponentSlot::HostCut2D));
        assert!(model_paths.iter().any(|p| p.slot == OpeningComponentSlot::Frame3D || p.slot == OpeningComponentSlot::Glazing3D));
        assert!(model_paths
            .iter()
            .any(|path| path.resolved_look_role() == Some(OpeningLookRole::Division)));

        style.parts.fixed_frame.enabled = false;
        let plan_paths = preview_baked_paths_for_mode(&style, 1.2, AecOpeningPreviewMode::Plan2D);
        assert!(!plan_paths
            .iter()
            .any(|path| path.slot == OpeningComponentSlot::Frame2D));

        let model_paths = preview_baked_paths_for_mode(&style, 1.2, AecOpeningPreviewMode::Model3D);
        assert!(model_paths
            .iter()
            .any(|path| path.slot == OpeningComponentSlot::HostCut2D));
        assert!(model_paths
            .iter()
            .any(|path| path.resolved_look_role() == Some(OpeningLookRole::Glazing)));
        assert!(!model_paths
            .iter()
            .any(|path| path.resolved_look_role() == Some(OpeningLookRole::FixedFrame)));
    }

    #[test]
    fn form_grid_fields_build_two_columns_for_preview_and_save() {
        let cells = [
            AecOpeningCellBuffer {
                kind: crate::modules::aec::engine::opening_style::OpeningCellKind::Glass,
                hinge: None,
                sill: "0".to_string(),
            },
            AecOpeningCellBuffer {
                kind: crate::modules::aec::engine::opening_style::OpeningCellKind::Glass,
                hinge: None,
                sill: "0".to_string(),
            },
        ];
        let grid = opening_grid_from_fields("0.5, 0.5", "", "", "", &cells);
        let cells = grid
            .layout(1.2, 1.4, OpeningKind::Window)
            .expect("valid form grid");
        assert_eq!(cells.len(), 2);
        assert!((cells[0].left + 0.6).abs() < 1e-12);
        assert!((cells[0].right - 0.0).abs() < 1e-12);
        assert!((cells[1].left - 0.0).abs() < 1e-12);
        assert!((cells[1].right - 0.6).abs() < 1e-12);

        let mut style = OpeningStyle::standard_window();
        style.grid = construction_from_division(&grid, 1.2, 1.4);
        style.suggested_levels.divisions =
            crate::modules::aec::engine::opening_style::DivisionLevel::PostsAndMuntins;
        let paths = preview_baked_paths_for_mode(&style, 1.2, AecOpeningPreviewMode::Plan2D);
        assert!(paths.iter().any(|path| {
            path.resolved_look_role() == Some(OpeningLookRole::Division)
                && path.points.iter().any(|(x, _)| x.abs() < 1e-12)
        }));
    }

    #[test]
    fn meter_position_edits_preserve_weights_and_split_largest_gap() {
        let mut weights = vec![0.5, 0.5];
        assert!(set_vertical_position(&mut weights, 0, 0.7, 1.2));
        assert!((weights.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        assert!((vertical_position_fields(&weights, 1.2)[0].parse::<f64>().unwrap() - 0.7).abs() < 1e-4);
        assert!((vertical_position_fields(&weights, 2.4)[0].parse::<f64>().unwrap() - 1.4).abs() < 1e-4);

        let mut horizontal = vec![0.5];
        assert!(set_ordered_position(&mut horizontal, 0, 0.8, 1.4, 0.04));
        assert!((horizontal[0] - 0.8).abs() < 1e-12);

        let mut muntins = vec![0.2, 0.3];
        assert!(insert_midpoint_in_largest_interval(&mut muntins, 1.0, 0.04));
        assert!((muntins[2] - 0.65).abs() < 1e-12);

        let normalized = parse_list(&format_vertical_weights(&[1.0 / 3.0; 3]));
        assert!((normalized.iter().sum::<f64>() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn model_preview_wall_sections_leave_the_opening_unoccupied() {
        let half_width = 0.6;
        let sill_z = 0.9;
        let head_z = 2.1;
        let sections = preview_wall_section_bounds(half_width, sill_z, head_z, 2.6);

        assert_eq!(sections.len(), 4);
        assert!(sections.iter().all(|&(x_min, x_max, z_min, z_max)| {
            !(x_min < half_width
                && x_max > -half_width
                && z_min < head_z
                && z_max > sill_z)
        }));
    }

    #[test]
    fn section_and_model_previews_include_wall_and_sized_attachments() {
        let mut style = OpeningStyle::standard_window();
        style.parts.exterior_sill.thickness = 0.05;
        style.parts.exterior_sill.overhang = 0.1;
        style.parts.interior_board.thickness = 0.04;
        style.parts.interior_board.overhang = 0.08;
        style.parts.shutter_box.height = 0.2;
        style.parts.shutter_box.depth = 0.25;
        style.parts.lintel.height = 0.15;

        let section = preview_baked_paths_for_mode(&style, 1.2, AecOpeningPreviewMode::Section);
        assert!(section
            .iter()
            .any(|path| path.slot == OpeningComponentSlot::HostCut2D && path.filled));

        let model = preview_baked_paths_for_mode(&style, 1.2, AecOpeningPreviewMode::Model3D);
        for role in [
            OpeningLookRole::ExteriorSill,
            OpeningLookRole::InteriorBoard,
            OpeningLookRole::ShutterBox,
            OpeningLookRole::Lintel,
        ] {
            assert!(model
                .iter()
                .any(|path| path.resolved_look_role() == Some(role)));
        }
    }
}
