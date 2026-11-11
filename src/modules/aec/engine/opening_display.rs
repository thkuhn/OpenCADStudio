//! Plan-view display children for wall openings (`OPENING_REP`).
//!
//! Parametric generators bake world geometry in the opening's local box
//! (X along the host axis, Y across wall thickness).

use std::collections::HashMap;

use acadrust::entities::{LwPolyline, LwVertex};
use acadrust::types::Vector2;
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::modules::aec::engine::display_component::{ComponentRuleSet, OpeningComponentSlot};
use crate::modules::aec::engine::opening_shape::OpeningShape;
use crate::modules::aec::engine::construction_grid::{
    omits_threshold, resolve_grid, BarAxis, ConstructionGrid, ResolvedGrid, TrackEnd, TrackMeeting,
};
use crate::modules::aec::engine::opening_style::{
    apply_plan_visibility, construction_from_division, default_slots_for_kind,
    default_style_for_kind, effective_slots_for_plan, project_division_sized, BlockPlacementMode,
    DivisionGrid, DivisionLevel,
    ElevationFrameLevel, OpeningParts, OpeningPlanProfiles, OpeningRepresentationLevels,
    opening_component_applies, opening_component_should_render, OpeningLookRole, PlanFrameLevel, SwingGlyph,
    OpeningGenerator, OpeningStyle, SlotGeometry, LeafOperation, DEFAULT_FRAME_THICKNESS,
    DEFAULT_OPENING_ANGLE_DEG,
};
use crate::modules::aec::engine::section_profile::{resolve_section, ResolvedSection};
use crate::modules::aec::engine::opening_xdata::{
    opening_from_entity, openings_for_host_wall, write_opening_instance,
};
use crate::modules::aec::engine::openings::{
    point_and_tangent_at_distance, Opening, OpeningKind, SwingSide,
};
use crate::modules::aec::engine::wall_package::resolve_wall_package;
use crate::modules::aec::engine::xdata::{
    get_wall_vertices, wall_from_entity, write_aec_record, AEC_APPID,
};
use crate::modules::aec::engine::{self, StyleLibrary};
use crate::scene::model::hatch_model::{HatchModel, HatchPattern};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;

/// XDATA kind tag on opening display children.
pub const OPENING_REP_TAG: &str = "OPENING_REP";

/// Number of chords used to approximate a swing arc.
const SWING_CHORD_COUNT: usize = 16;

/// One baked 2D primitive in **local** opening coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct BakedPath {
    pub slot: OpeningComponentSlot,
    /// Optional role override for view-specific preview geometry.
    pub look_role: Option<OpeningLookRole>,
    pub points: Vec<(f64, f64)>,
    /// Unprojected wire points used by the opening style manager's 3D canvas.
    pub points_3d: Option<Vec<[f64; 3]>>,
    pub closed: bool,
    /// When true the closed path is also emitted as a solid hatch.
    pub filled: bool,
}

impl BakedPath {
    pub fn resolved_look_role(&self) -> Option<OpeningLookRole> {
        self.look_role
            .or_else(|| OpeningLookRole::for_slot(self.slot))
    }

    fn with_look_role(mut self, role: OpeningLookRole) -> Self {
        self.look_role = Some(role);
        self
    }
}

/// Parameters that drive generator bake (instance box + absolute profile).
#[derive(Debug, Clone, Copy)]
pub struct OpeningBakeParams {
    pub width: f64,
    pub height: f64,
    pub sill_height: f64,
    pub thickness: f64,
    pub frame_thickness: f64,
    pub cross_axis_offset: f64,
    pub wall_y_min: f64,
    pub wall_y_max: f64,
    pub hinge: LeafOperation,
    pub swing_side: SwingSide,
    pub shape: OpeningShape,
    pub spring_height: f64,
    pub opening_angle_deg: f64,
    pub kind: OpeningKind,
}

impl OpeningBakeParams {
    pub fn from_opening(opening: &Opening, thickness: f64, style: Option<&OpeningStyle>) -> Self {
        let (frame_thickness, angle, swing_side) = if let Some(style) = style {
            (
                style.parts.fixed_frame.face_width,
                style.opening_angle_deg,
                style.swing_side,
            )
        } else if opening.kind == OpeningKind::Breakthrough {
            (0.0, 0.0, opening.swing_side)
        } else {
            (
                DEFAULT_FRAME_THICKNESS,
                DEFAULT_OPENING_ANGLE_DEG,
                opening.swing_side,
            )
        };
        let ht = thickness * 0.5;
        Self {
            width: opening.width,
            height: opening.height,
            sill_height: opening.sill_height,
            thickness,
            frame_thickness,
            cross_axis_offset: opening.cross_axis_offset,
            wall_y_min: -ht,
            wall_y_max: ht,
            hinge: opening.hinge,
            swing_side,
            shape: opening.shape,
            spring_height: opening.spring_height,
            opening_angle_deg: angle,
            kind: opening.kind,
        }
    }

    pub fn with_wall_bounds(mut self, y_min: f64, y_max: f64) -> Self {
        self.wall_y_min = y_min;
        self.wall_y_max = y_max;
        self
    }
}

/// Resolved symbol map and style for an instance (style, else kind defaults).
pub fn resolved_slots(
    opening: &Opening,
    library: Option<&StyleLibrary>,
) -> (HashMap<OpeningComponentSlot, SlotGeometry>, Option<OpeningStyle>) {
    resolved_slots_for_plan(opening, library, None)
}

/// Like [`resolved_slots`], overlaying the plan-type display profile when set.
pub fn resolved_slots_for_plan(
    opening: &Opening,
    library: Option<&StyleLibrary>,
    plan_profiles: Option<&OpeningPlanProfiles>,
) -> (HashMap<OpeningComponentSlot, SlotGeometry>, Option<OpeningStyle>) {
    let fallback_style = default_style_for_kind(opening.kind);
    let requested_id = opening.style_id.as_deref().unwrap_or(&fallback_style.style.id);
    if let Some(lib) = library {
        if let Some(style) = lib
            .find_opening_style(requested_id)
            .or_else(|| lib.find_opening_style(&fallback_style.style.id))
        {
            let map: HashMap<_, _> = lib
                .opening_styles
                .iter()
                .map(|s| (s.style.id.clone(), s.clone()))
                .collect();
            let slots = effective_slots_for_plan(&map, &style.style.id, plan_profiles)
                .unwrap_or_else(|_| default_slots_for_kind(style.kind));
            return (slots, Some(style.clone()));
        }
    }
    let mut slots = default_slots_for_kind(fallback_style.kind);
    if let Some(profile) = plan_profiles.and_then(|profiles| profiles.get(&fallback_style.style.id)) {
        for (slot, geometry) in &profile.slots {
            slots.insert(*slot, geometry.clone());
        }
    }
    (slots, Some(fallback_style))
}

fn rules_with_plan_visibility(
    opening: &Opening,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> Option<ComponentRuleSet> {
    let mut merged = rules.cloned().unwrap_or_default();
    let plan = merged.opening_profiles.clone();
    let style_id = opening
        .style_id
        .as_deref()
        .unwrap_or_else(|| crate::modules::aec::engine::opening_style::seed_style_id_for_kind(opening.kind));
    if let Some(lib) = library {
        let map: HashMap<_, _> = lib
            .opening_styles
            .iter()
            .map(|s| (s.style.id.clone(), s.clone()))
            .collect();
        apply_plan_visibility(&map, &style_id.to_string(), Some(&plan), &mut merged.visibility);
    }
    if rules.is_none() && merged.visibility.is_empty() {
        None
    } else {
        Some(merged)
    }
}

/// Bake generator primitives in the local opening box.
///
/// Origin is the insertion (axis point). X is along the wall, Y across
/// thickness. Outer box is `width × thickness`, inner inset is
/// `frame_thickness` in drawing units (not a fraction of width).
pub fn bake_opening_generators(
    slots: &HashMap<OpeningComponentSlot, SlotGeometry>,
    params: OpeningBakeParams,
    rules: Option<&ComponentRuleSet>,
) -> Vec<BakedPath> {
    bake_opening_generators_with_doc(slots, params, rules, None)
}

pub fn bake_opening_generators_with_doc(
    slots: &HashMap<OpeningComponentSlot, SlotGeometry>,
    params: OpeningBakeParams,
    rules: Option<&ComponentRuleSet>,
    doc: Option<&acadrust::CadDocument>,
) -> Vec<BakedPath> {
    let mut out = Vec::new();
    let mut keys: Vec<_> = slots.keys().copied().collect();
    keys.sort_by_key(|s| s.key());
    for slot in keys {
        if !rules.map_or(true, |r| r.is_opening_visible(slot)) {
            continue;
        }
        match slots.get(&slot) {
            Some(SlotGeometry::Generator(gen)) => {
                out.extend(bake_generator(slot, *gen, params));
            }
            Some(SlotGeometry::Block {
                block_name,
                placement,
            }) => {
                out.extend(bake_block_slot(slot, block_name, *placement, params, doc));
            }
            None => {}
        }
    }
    out
}

pub fn bake_opening_representation(
    slots: &HashMap<OpeningComponentSlot, SlotGeometry>,
    params: OpeningBakeParams,
    parts: &OpeningParts,
    library: Option<&StyleLibrary>,
    grid: &ConstructionGrid,
    levels: OpeningRepresentationLevels,
    rules: Option<&ComponentRuleSet>,
    doc: Option<&acadrust::CadDocument>,
) -> Vec<BakedPath> {
    let _projected = project_division_sized(grid, params.width, params.height);
    let fixed_frame_section = resolve_component_section(
        library,
        parts.fixed_frame.profile_id.as_deref(),
        parts.fixed_frame.face_width,
        parts.fixed_frame.wall_depth,
    );
    let leaf_frame_section = resolve_component_section(
        library,
        parts.leaf_frame.profile_id.as_deref(),
        parts.leaf_frame.face_width,
        parts.leaf_frame.depth,
    );
    let exterior_sill_section = resolve_component_section(
        library,
        parts.exterior_sill.profile_id.as_deref(),
        parts.exterior_sill.thickness,
        parts.exterior_sill.thickness,
    );
    let interior_board_section = resolve_component_section(
        library,
        parts.interior_board.profile_id.as_deref(),
        parts.interior_board.thickness,
        parts.interior_board.thickness,
    );
    let division_fallback = division_profile_section(library, grid, parts).unwrap_or(fixed_frame_section.clone());
    let replaced_slots = [
        OpeningComponentSlot::Frame2D,
        OpeningComponentSlot::Leaf2D,
        OpeningComponentSlot::Swing2D,
        OpeningComponentSlot::Glazing2D,
        OpeningComponentSlot::ElevationContour2D,
        OpeningComponentSlot::ElevationMuntins2D,
    ];
    let mut paths = bake_opening_generators_with_doc(slots, params, rules, doc);
    for slot in replaced_slots {
        if matches!(slots.get(&slot), Some(SlotGeometry::Generator(_))) {
            paths.retain(|baked| baked.slot != slot);
        }
    }

    let slot_enabled = |slot| {
        matches!(slots.get(&slot), Some(SlotGeometry::Generator(_)))
            && rules.is_none_or(|rules| rules.is_opening_visible(slot))
    };

    if matches!(
        slots.get(&OpeningComponentSlot::Sill2D),
        Some(SlotGeometry::Generator(OpeningGenerator::SillLines))
    ) {
        paths.retain(|baked| baked.slot != OpeningComponentSlot::Sill2D);
        if slot_enabled(OpeningComponentSlot::Sill2D) {
            paths.extend(bake_sill_lines_from_parts(
                OpeningComponentSlot::Sill2D,
                params,
                parts,
                library,
                &fixed_frame_section,
                &exterior_sill_section,
                &interior_board_section,
            ));
        }
    }

    if slot_enabled(OpeningComponentSlot::Frame2D) {
        paths.extend(bake_plan_frame_level(
            params,
            &parts.fixed_frame,
            &fixed_frame_section,
            levels.plan_frame,
        ));
    }
    if slot_enabled(OpeningComponentSlot::Leaf2D) {
        paths.extend(bake_grid_leaf_lines(grid, params));
    }
    if slot_enabled(OpeningComponentSlot::Glazing2D) {
        paths.extend(bake_grid_glazing_lines(
            grid,
            params,
            parts.glazing.inset,
            fixed_frame_section.face,
        ));
    }
    paths.extend(bake_grid_plan_divisions(
        grid,
        params,
        levels.divisions,
        library,
        &division_fallback,
        &leaf_frame_section,
    ));
    if slot_enabled(OpeningComponentSlot::Swing2D) {
        paths.extend(bake_grid_swing_glyphs(grid, params, levels.swing));
    }
    if slot_enabled(OpeningComponentSlot::ElevationContour2D) {
        paths.extend(bake_elevation_frame_level(
            params,
            fixed_frame_section.face,
            leaf_frame_section.face,
            levels.elevation_frame,
            omits_threshold(&resolve_grid(grid, params.width, params.height), params.width)
                || params.kind == OpeningKind::Door,
        ));
        paths.extend(bake_grid_elevation_schema(
            grid,
            params,
            (parts.leaf_frame.rebate_overlap + parts.leaf_frame.lateral_overlap).max(0.0),
        ));
    }
    if slot_enabled(OpeningComponentSlot::ElevationMuntins2D) {
        paths.extend(bake_grid_elevation_divisions(
            grid,
            params,
            levels.divisions,
            library,
            &division_fallback,
            &leaf_frame_section,
        ));
    }
    paths.retain(|path| {
        path.resolved_look_role().map_or(true, |role| {
            opening_component_should_render_with_library(parts, params.kind, role, library)
        })
    });
    paths
}

pub fn bake_opening_elevation_paths(
    params: OpeningBakeParams,
    parts: &OpeningParts,
    library: Option<&StyleLibrary>,
    grid: &DivisionGrid,
    frame_level: ElevationFrameLevel,
    division_level: DivisionLevel,
) -> Vec<BakedPath> {
    let built = construction_from_division(grid, params.width, params.height);
    bake_opening_elevation_from_grid(
        params,
        parts,
        library,
        &built,
        frame_level,
        division_level,
    )
}

pub fn bake_opening_elevation_from_grid(
    params: OpeningBakeParams,
    parts: &OpeningParts,
    library: Option<&StyleLibrary>,
    grid: &ConstructionGrid,
    frame_level: ElevationFrameLevel,
    division_level: DivisionLevel,
) -> Vec<BakedPath> {
    let fixed_frame_section = resolve_component_section(
        library,
        parts.fixed_frame.profile_id.as_deref(),
        parts.fixed_frame.face_width,
        parts.fixed_frame.wall_depth,
    );
    let leaf_frame_section = resolve_component_section(
        library,
        parts.leaf_frame.profile_id.as_deref(),
        parts.leaf_frame.face_width,
        parts.leaf_frame.depth,
    );
    let division_fallback =
        division_profile_section(library, grid, parts).unwrap_or(fixed_frame_section.clone());
    let mut paths = vec![rough_opening_elevation_guide(params)];
    paths.extend(bake_elevation_frame_level(
        params,
        fixed_frame_section.face,
        leaf_frame_section.face,
        frame_level,
        omits_threshold(&resolve_grid(grid, params.width, params.height), params.width)
            || params.kind == OpeningKind::Door,
    ));
    paths.extend(bake_grid_elevation_divisions(
        grid,
        params,
        division_level,
        library,
        &division_fallback,
        &leaf_frame_section,
    ));
    paths.extend(bake_grid_elevation_schema(grid, params, 0.0));
    paths.retain(|path| {
        path.resolved_look_role().map_or(true, |role| {
            opening_component_should_render_with_library(parts, params.kind, role, library)
        })
    });
    paths
}

pub fn bake_opening_plan_paths(
    params: OpeningBakeParams,
    parts: &OpeningParts,
    library: Option<&StyleLibrary>,
    grid: &DivisionGrid,
    frame_level: PlanFrameLevel,
    swing_glyph: SwingGlyph,
    division_level: DivisionLevel,
) -> Vec<BakedPath> {
    let built = construction_from_division(grid, params.width, params.height);
    bake_opening_plan_from_grid(
        params,
        parts,
        library,
        &built,
        frame_level,
        swing_glyph,
        division_level,
    )
}

pub fn bake_opening_plan_from_grid(
    params: OpeningBakeParams,
    parts: &OpeningParts,
    library: Option<&StyleLibrary>,
    grid: &ConstructionGrid,
    frame_level: PlanFrameLevel,
    swing_glyph: SwingGlyph,
    division_level: DivisionLevel,
) -> Vec<BakedPath> {
    let slots = default_slots_for_kind(params.kind);
    let levels = OpeningRepresentationLevels {
        plan_frame: frame_level,
        swing: swing_glyph,
        elevation_frame: ElevationFrameLevel::Off,
        divisions: division_level,
    };
    bake_opening_representation(&slots, params, parts, library, grid, levels, None, None)
        .into_iter()
        .filter(|path| !path.slot.is_elevation())
        .collect()
}

fn division_profile_section(
    library: Option<&StyleLibrary>,
    grid: &ConstructionGrid,
    parts: &OpeningParts,
) -> Option<ResolvedSection> {
    grid.muntin_profile_id
        .as_deref()
        .and_then(|id| library?.find_section_profile(id))
        .filter(|profile| profile.is_valid())
        .map(|_| {
            resolve_component_section(
                library,
                grid.muntin_profile_id.as_deref(),
                parts.fixed_frame.face_width,
                parts.fixed_frame.wall_depth,
            )
        })
}

fn resolve_component_section(
    library: Option<&StyleLibrary>,
    profile_id: Option<&str>,
    fallback_face: f64,
    fallback_depth: f64,
) -> ResolvedSection {
    let profile = profile_id.and_then(|id| library?.find_section_profile(id));
    resolve_section(profile, fallback_face, fallback_depth)
}

pub(crate) fn opening_component_should_render_with_library(
    parts: &OpeningParts,
    kind: OpeningKind,
    role: OpeningLookRole,
    library: Option<&StyleLibrary>,
) -> bool {
    if opening_component_should_render(parts, kind, role) {
        return true;
    }
    if !opening_component_applies(kind, role) {
        return false;
    }
    let (enabled, profile_id) = match role {
        OpeningLookRole::FixedFrame => (parts.fixed_frame.enabled, parts.fixed_frame.profile_id.as_deref()),
        OpeningLookRole::Leaf | OpeningLookRole::Swing => {
            (parts.leaf_frame.enabled, parts.leaf_frame.profile_id.as_deref())
        }
        OpeningLookRole::ExteriorSill => (
            parts.exterior_sill.enabled,
            parts.exterior_sill.profile_id.as_deref(),
        ),
        OpeningLookRole::InteriorBoard => (
            parts.interior_board.enabled,
            parts.interior_board.profile_id.as_deref(),
        ),
        OpeningLookRole::Threshold => (parts.threshold.enabled, parts.threshold.profile_id.as_deref()),
        OpeningLookRole::ShutterBox => (parts.shutter_box.enabled, parts.shutter_box.profile_id.as_deref()),
        OpeningLookRole::Lintel => (parts.lintel.enabled, parts.lintel.profile_id.as_deref()),
        OpeningLookRole::Glazing | OpeningLookRole::Division => return false,
    };
    enabled
        && profile_id
            .and_then(|id| library?.find_section_profile(id))
            .is_some_and(|profile| profile.is_valid())
}

fn representation_levels(
    slots: &HashMap<OpeningComponentSlot, SlotGeometry>,
    grid: &DivisionGrid,
    plan_division_level: Option<DivisionLevel>,
) -> OpeningRepresentationLevels {
    let divisions = plan_division_level.unwrap_or_else(|| {
        if grid.vertical.is_empty()
            && grid.horizontal.is_empty()
            && grid.vertical_muntins.is_empty()
            && grid.horizontal_muntins.is_empty()
        {
            DivisionLevel::Off
        } else if grid.horizontal.is_empty()
            && grid.vertical_muntins.is_empty()
            && grid.horizontal_muntins.is_empty()
        {
            DivisionLevel::Posts
        } else {
            DivisionLevel::PostsAndMuntins
        }
    });
    OpeningRepresentationLevels {
        plan_frame: if slots.contains_key(&OpeningComponentSlot::Frame2D) {
            PlanFrameLevel::Profile
        } else {
            PlanFrameLevel::Off
        },
        swing: if slots.contains_key(&OpeningComponentSlot::Swing2D) {
            SwingGlyph::Arc
        } else {
            SwingGlyph::Off
        },
        elevation_frame: if slots.contains_key(&OpeningComponentSlot::ElevationContour2D) {
            ElevationFrameLevel::FixedAndLeaf
        } else {
            ElevationFrameLevel::Off
        },
        divisions,
    }
}

fn bake_plan_frame_level(
    params: OpeningBakeParams,
    fixed_frame: &crate::modules::aec::engine::opening_style::FixedFrameDimensions,
    section: &ResolvedSection,
    level: PlanFrameLevel,
) -> Vec<BakedPath> {
    let slot = OpeningComponentSlot::Frame2D;
    let half_width = params.width * 0.5;
    match level {
        PlanFrameLevel::Off => Vec::new(),
        PlanFrameLevel::SingleLine => vec![path(
            slot,
            vec![(-half_width, 0.0), (half_width, 0.0)],
            false,
            false,
        )],
        PlanFrameLevel::DoubleLine => vec![
            path(
                slot,
                vec![( -half_width, params.wall_y_min), (half_width, params.wall_y_min)],
                false,
                false,
            ),
            path(
                slot,
                vec![(-half_width, params.wall_y_max), (half_width, params.wall_y_max)],
                false,
                false,
            ),
        ],
        PlanFrameLevel::Profile => {
            let face_width = section.face;
            let depth = section.depth;
            if face_width <= 1e-8 || depth <= 1e-8 {
                return Vec::new();
            }
            let y_min = fixed_frame.axis_offset - depth * 0.5;
            let outline = unclosed_section_outline(section);
            vec![
                path(
                    slot,
                    outline
                        .iter()
                        .map(|(x, y)| (-half_width + x, y_min + y))
                        .collect(),
                    true,
                    false,
                ),
                path(
                    slot,
                    outline
                        .iter()
                        .map(|(x, y)| (half_width - face_width + x, y_min + y))
                        .collect(),
                    true,
                    false,
                ),
            ]
        }
    }
}

fn unclosed_section_outline(section: &ResolvedSection) -> &[(f64, f64)] {
    if section.outline.len() > 1 && section.outline.first() == section.outline.last() {
        &section.outline[..section.outline.len() - 1]
    } else {
        &section.outline
    }
}

fn bake_grid_leaf_lines(grid: &ConstructionGrid, params: OpeningBakeParams) -> Vec<BakedPath> {
    let cells = flatten_resolved_cells(grid, params.width, params.height);
    let mut paths = Vec::new();
    for cell in cells {
        if !matches!(
            cell.cell.fill,
            crate::modules::aec::engine::construction_grid::CellFill::Door
                | crate::modules::aec::engine::construction_grid::CellFill::Glass
        ) {
            continue;
        }
        let center_x = (cell.left + cell.right) * 0.5;
        let cell_width = cell.right - cell.left;
        if params.kind == OpeningKind::Door {
            let hinge = cell.cell.operation.unwrap_or(params.hinge);
            let mut cell_params = params;
            cell_params.swing_side = cell.cell.swing.unwrap_or(params.swing_side);
            cell_params.opening_angle_deg =
                cell.cell.angle_deg.unwrap_or(params.opening_angle_deg);
            let (_, end) = leaf_segment(
                hinge,
                cell_params.swing_side,
                cell_width * 0.5,
                cell_params.opening_angle_deg,
                params.frame_thickness,
            );
            let start_x = match hinge.bearing() {
                crate::modules::aec::engine::construction_grid::TrackEnd::Start => {
                    cell.left + params.frame_thickness * 0.5
                }
                crate::modules::aec::engine::construction_grid::TrackEnd::End => {
                    cell.right - params.frame_thickness * 0.5
                }
            };
            paths.push(path(
                OpeningComponentSlot::Leaf2D,
                vec![(start_x, 0.0), (center_x + end.0, end.1)],
                false,
                false,
            ));
        } else {
            paths.push(path(
                OpeningComponentSlot::Leaf2D,
                vec![(cell.left, 0.0), (cell.right, 0.0)],
                false,
                false,
            ));
        }
    }
    paths
}

fn bake_grid_glazing_lines(
    grid: &ConstructionGrid,
    params: OpeningBakeParams,
    inset: f64,
    bar: f64,
) -> Vec<BakedPath> {
    flatten_resolved_cells(grid, params.width, params.height)
        .into_iter()
        .filter(|cell| {
            matches!(
                cell.cell.fill,
                crate::modules::aec::engine::construction_grid::CellFill::Glass
                    | crate::modules::aec::engine::construction_grid::CellFill::Fixed
            )
        })
        .filter_map(|cell| {
            let left = glass_edge_into_bar(cell.left, 1.0, bar, inset);
            let right = glass_edge_into_bar(cell.right, -1.0, bar, inset);
            if right - left <= 1e-9 {
                return None;
            }
            Some(path(
                OpeningComponentSlot::Glazing2D,
                vec![(left, 0.0), (right, 0.0)],
                false,
                false,
            ))
        })
        .collect()
}

fn bake_grid_plan_divisions(
    grid: &ConstructionGrid,
    params: OpeningBakeParams,
    level: DivisionLevel,
    library: Option<&StyleLibrary>,
    fallback: &ResolvedSection,
    leaf: &ResolvedSection,
) -> Vec<BakedPath> {
    if level == DivisionLevel::Off {
        return Vec::new();
    }
    let resolved = resolve_grid(grid, params.width, params.height);
    let mut paths = Vec::new();
    collect_plan_divisions(
        &resolved,
        0.0,
        params,
        level,
        library,
        fallback,
        leaf,
        &mut paths,
    );
    paths
}

fn collect_plan_divisions(
    resolved: &ResolvedGrid,
    offset_x: f64,
    params: OpeningBakeParams,
    level: DivisionLevel,
    library: Option<&StyleLibrary>,
    fallback: &ResolvedSection,
    leaf: &ResolvedSection,
    paths: &mut Vec<BakedPath>,
) {
    let origin = params.width * 0.5;
    for edge in &resolved.edges {
        if edge.axis != BarAxis::Vertical || edge.meeting != TrackMeeting::Mullion {
            continue;
        }
        let section = named_division_section(
            library,
            edge.profile_id.as_deref(),
            resolved.muntin_profile_id.as_deref(),
            fallback,
        );
        paths.push(bake_plan_division_path(
            edge.position + offset_x - origin,
            params,
            section.as_ref(),
        ));
    }
    for edge in &resolved.edges {
        if edge.axis != BarAxis::Vertical {
            continue;
        }
        let Some(shift) = stulp_shift(edge.meeting, leaf.face) else {
            continue;
        };
        paths.push(
            bake_plan_division_path(edge.position + offset_x - origin + shift, params, Some(leaf))
                .with_look_role(OpeningLookRole::Leaf),
        );
    }
    for cell in &resolved.cells {
        if level == DivisionLevel::PostsAndMuntins {
            for bar in &cell.bars {
                if bar.axis != BarAxis::Vertical {
                    continue;
                }
                let Some(section) = muntin_bar_section(
                    library,
                    resolved.muntin_profile_id.as_deref(),
                    bar.face_width,
                    bar.depth,
                    leaf,
                ) else {
                    continue;
                };
                paths.push(bake_plan_division_path(
                    bar.position + offset_x - origin,
                    params,
                    Some(&section),
                ));
            }
        }
        if let Some(nested) = &cell.nested {
            collect_plan_divisions(
                nested,
                offset_x + cell.left,
                params,
                level,
                library,
                fallback,
                leaf,
                paths,
            );
        }
    }
}

fn bake_plan_division_path(
    x: f64,
    params: OpeningBakeParams,
    profile: Option<&ResolvedSection>,
) -> BakedPath {
    if let Some(profile) = profile {
        let y_center = (params.wall_y_min + params.wall_y_max) * 0.5;
        let outline = unclosed_section_outline(profile)
            .iter()
            .map(|(face, depth)| {
                (
                    x - profile.face * 0.5 + face,
                    y_center - profile.depth * 0.5 + depth,
                )
            })
            .collect();
        path(OpeningComponentSlot::Division2D, outline, true, false)
            .with_look_role(OpeningLookRole::Division)
    } else {
        path(
            OpeningComponentSlot::Division2D,
            vec![(x, params.wall_y_min), (x, params.wall_y_max)],
            false,
            false,
        )
        .with_look_role(OpeningLookRole::Division)
    }
}

fn bake_grid_swing_glyphs(
    grid: &ConstructionGrid,
    params: OpeningBakeParams,
    glyph: SwingGlyph,
) -> Vec<BakedPath> {
    if glyph == SwingGlyph::Off {
        return Vec::new();
    }
    let projected = project_division_sized(grid, params.width, params.height);
    let Ok(cells) = projected.layout(params.width, params.height, params.kind) else {
        return Vec::new();
    };
    let resolved = resolve_grid(grid, params.width, params.height);
    let chained = chain_cells(&resolved, params.hinge);
    let mut paths = Vec::new();
    for cell in &cells {
        if chained.contains(&(cell.column, cell.row)) {
            continue;
        }
        let hinge = cell.cell.hinge.unwrap_or(params.hinge);
        let source = resolved
            .cells
            .iter()
            .find(|source| source.column == cell.column && source.row == cell.row);
        let mut cell_params = params;
        if let Some(source) = source {
            cell_params.swing_side = source.cell.swing.unwrap_or(params.swing_side);
            cell_params.opening_angle_deg =
                source.cell.angle_deg.unwrap_or(params.opening_angle_deg);
        }
        let is_door = cell.cell.kind
            == crate::modules::aec::engine::opening_style::OpeningCellKind::DoorLeaf
            || params.kind == OpeningKind::Door;
        if !is_door && matches!(hinge, LeafOperation::Turn { .. }) && glyph == SwingGlyph::Arc {
            continue;
        }
        paths.extend(plan_operation_schema(cell, hinge, cell_params, glyph));
    }
    for cell in flatten_resolved_cells(grid, params.width, params.height) {
        let is_root = resolved.cells.iter().any(|root_cell| {
            (root_cell.left - params.width * 0.5 - cell.left).abs() < 1e-6
                && (root_cell.right - params.width * 0.5 - cell.right).abs() < 1e-6
                && (root_cell.bottom - cell.bottom).abs() < 1e-6
                && (root_cell.top - cell.top).abs() < 1e-6
        });
        if is_root {
            continue;
        }
        let hinge = cell.cell.operation.unwrap_or(params.hinge);
        let mut cell_params = params;
        cell_params.swing_side = cell.cell.swing.unwrap_or(params.swing_side);
        cell_params.opening_angle_deg = cell.cell.angle_deg.unwrap_or(params.opening_angle_deg);
        let kind = match cell.cell.fill {
            crate::modules::aec::engine::construction_grid::CellFill::Opaque => {
                crate::modules::aec::engine::opening_style::OpeningCellKind::Opaque
            }
            crate::modules::aec::engine::construction_grid::CellFill::Door => {
                crate::modules::aec::engine::opening_style::OpeningCellKind::DoorLeaf
            }
            crate::modules::aec::engine::construction_grid::CellFill::Fixed => {
                crate::modules::aec::engine::opening_style::OpeningCellKind::Fixed
            }
            crate::modules::aec::engine::construction_grid::CellFill::Glass
            | crate::modules::aec::engine::construction_grid::CellFill::Nested(_) => {
                crate::modules::aec::engine::opening_style::OpeningCellKind::Glass
            }
        };
        let layout = crate::modules::aec::engine::opening_style::DivisionCellLayout {
            column: cell.column,
            row: cell.row,
            left: cell.left,
            right: cell.right,
            bottom: cell.bottom,
            top: cell.top,
            cell: crate::modules::aec::engine::opening_style::OpeningCell {
                kind,
                hinge: cell.cell.operation,
                sill: cell.cell.sill,
            },
        };
        paths.extend(plan_operation_schema(&layout, hinge, cell_params, glyph));
    }
    let free_y = match params.swing_side {
        SwingSide::Exterior => params.wall_y_min,
        SwingSide::Interior => params.wall_y_max,
    };
    paths.extend(chain_schema_paths(
        &resolve_grid(grid, params.width, params.height),
        params.hinge,
        OpeningComponentSlot::Swing2D,
        ChainPlane::Plan { free_y },
    ));
    paths
}

enum ChainPlane {
    Plan { free_y: f64 },
    Elevation,
}

fn cell_operation(cell: &crate::modules::aec::engine::construction_grid::ResolvedCell, fallback: LeafOperation) -> LeafOperation {
    cell.cell.operation.unwrap_or(fallback)
}

fn slide_or_fold(operation: LeafOperation) -> Option<LeafOperation> {
    match operation {
        LeafOperation::Slide { .. } | LeafOperation::Fold { .. } => Some(operation),
        _ => None,
    }
}

/// Adjacent cells of the same slide or fold kind, joined by a non-mullion edge.
fn chain_runs(
    resolved: &crate::modules::aec::engine::construction_grid::ResolvedGrid,
    fallback: LeafOperation,
) -> Vec<Vec<usize>> {
    use crate::modules::aec::engine::construction_grid::TrackMeeting;
    let mut order: Vec<usize> = (0..resolved.cells.len()).collect();
    order.sort_by_key(|&index| (resolved.cells[index].row, resolved.cells[index].column));
    let mut runs = Vec::new();
    let mut current = Vec::new();
    for index in order {
        let cell = &resolved.cells[index];
        let joins = current.last().is_some_and(|&previous| {
            let left: &crate::modules::aec::engine::construction_grid::ResolvedCell =
                &resolved.cells[previous];
            left.row == cell.row
                && cell.column == left.column + 1
                && (cell.left - left.right).abs() < 1e-6
                && slide_or_fold(cell_operation(left, fallback))
                    == slide_or_fold(cell_operation(cell, fallback))
                && resolved
                    .columns
                    .get(left.column)
                    .is_some_and(|track| !matches!(track.meeting, TrackMeeting::Mullion))
        });
        if !joins && !current.is_empty() {
            if current.len() >= 2 {
                runs.push(current);
            }
            current = Vec::new();
        }
        if slide_or_fold(cell_operation(cell, fallback)).is_some() {
            current.push(index);
        } else if !current.is_empty() {
            if current.len() >= 2 {
                runs.push(current);
            }
            current = Vec::new();
        }
    }
    if current.len() >= 2 {
        runs.push(current);
    }
    runs
}

fn chain_cells(
    resolved: &crate::modules::aec::engine::construction_grid::ResolvedGrid,
    fallback: LeafOperation,
) -> std::collections::HashSet<(usize, usize)> {
    chain_runs(resolved, fallback)
        .into_iter()
        .flat_map(|run| {
            run.into_iter()
                .map(|index| (resolved.cells[index].column, resolved.cells[index].row))
        })
        .collect()
}

fn chain_schema_paths(
    resolved: &crate::modules::aec::engine::construction_grid::ResolvedGrid,
    fallback: LeafOperation,
    slot: OpeningComponentSlot,
    plane: ChainPlane,
) -> Vec<BakedPath> {
    use crate::modules::aec::engine::construction_grid::TrackEnd;
    let mut paths = Vec::new();
    for run in chain_runs(resolved, fallback) {
        let operation = cell_operation(&resolved.cells[run[0]], fallback);
        let toward_end = match operation.bearing() {
            TrackEnd::End => true,
            TrackEnd::Start => false,
        };
        for (step, index) in run.iter().enumerate() {
            let cell = &resolved.cells[*index];
            let (low, high) = match plane {
                ChainPlane::Plan { free_y } => (0.0, free_y),
                ChainPlane::Elevation => (cell.bottom, cell.top),
            };
            let from_x = if toward_end { cell.left } else { cell.right };
            let to_x = if toward_end { cell.right } else { cell.left };
            let from_y = if step % 2 == 0 { low } else { high };
            let to_y = if step % 2 == 0 { high } else { low };
            paths.push(path(slot, vec![(from_x, from_y), (to_x, to_y)], false, false));
        }
    }
    paths
}

fn plan_operation_schema(
    cell: &crate::modules::aec::engine::opening_style::DivisionCellLayout,
    hinge: LeafOperation,
    params: OpeningBakeParams,
    glyph: SwingGlyph,
) -> Vec<BakedPath> {
    use crate::modules::aec::engine::construction_grid::TrackEnd;
    let width = cell.right - cell.left;
    let center_x = (cell.left + cell.right) * 0.5;
    let free_y = match params.swing_side {
        SwingSide::Exterior => params.wall_y_min,
        SwingSide::Interior => params.wall_y_max,
    };
    let slot = OpeningComponentSlot::Swing2D;
    match hinge {
        LeafOperation::Turn { .. } | LeafOperation::TiltTurn { .. } => {
            if glyph == SwingGlyph::Triangle || matches!(hinge, LeafOperation::TiltTurn { .. }) {
                let hinge_x = match hinge.bearing() {
                    TrackEnd::Start => cell.left,
                    TrackEnd::End => cell.right,
                };
                let strike_x = match hinge.bearing() {
                    TrackEnd::Start => cell.right,
                    TrackEnd::End => cell.left,
                };
                vec![path(
                    slot,
                    vec![(hinge_x, 0.0), (strike_x, 0.0), (hinge_x, free_y)],
                    true,
                    false,
                )]
            } else {
                let mut cell_params = params;
                cell_params.width = width;
                cell_params.hinge = hinge;
                bake_swing_arc(slot, cell_params, width * 0.5, params.frame_thickness)
                    .into_iter()
                    .map(|mut baked| {
                        for point in &mut baked.points {
                            point.0 += center_x;
                        }
                        baked
                    })
                    .collect()
            }
        }
        LeafOperation::Tilt => vec![path(
            slot,
            vec![(cell.left, 0.0), (center_x, free_y), (cell.right, 0.0)],
            false,
            false,
        )],
        LeafOperation::Pivot { .. } => vec![path(
            slot,
            vec![(center_x, params.wall_y_min), (center_x, params.wall_y_max)],
            false,
            false,
        )],
        LeafOperation::Slide { direction } | LeafOperation::Fold { stack: direction } => {
            let (from, to) = match direction {
                TrackEnd::Start => (cell.right, cell.left),
                TrackEnd::End => (cell.left, cell.right),
            };
            vec![path(slot, vec![(from, 0.0), (to, 0.0)], false, false)]
        }
    }
}

fn bake_grid_elevation_schema(
    grid: &ConstructionGrid,
    params: OpeningBakeParams,
    overlap: f64,
) -> Vec<BakedPath> {
    let slot = OpeningComponentSlot::ElevationMuntins2D;
    let resolved = resolve_grid(grid, params.width, params.height);
    let chained = chain_cells(&resolved, params.hinge);
    let chained_bounds: Vec<(f64, f64, f64, f64)> = resolved
        .cells
        .iter()
        .filter(|cell| chained.contains(&(cell.column, cell.row)))
        .map(|cell| {
            (
                cell.left - params.width * 0.5,
                cell.right - params.width * 0.5,
                cell.bottom,
                cell.top,
            )
        })
        .collect();
    let mut paths = chain_schema_paths(&resolved, params.hinge, slot, ChainPlane::Elevation);
    for cell in flatten_resolved_cells(grid, params.width, params.height) {
        if chained_bounds.iter().any(|bounds| {
            (bounds.0 - cell.left).abs() < 1e-6
                && (bounds.1 - cell.right).abs() < 1e-6
                && (bounds.2 - cell.bottom).abs() < 1e-6
                && (bounds.3 - cell.top).abs() < 1e-6
        }) {
            continue;
        }
        let hinge = cell.cell.operation.unwrap_or(params.hinge);
        let bar = params.frame_thickness.max(0.0);
        let into = (bar - overlap.max(0.0).min(bar)).max(0.0);
        let bottom_inset = if params.kind == OpeningKind::Door {
            0.0
        } else {
            into
        };
        let left = cell.left + into;
        let right = cell.right - into;
        let bottom = cell.bottom + bottom_inset;
        let top = cell.top - into;
        if right - left <= 1e-9 || top - bottom <= 1e-9 {
            continue;
        }
        let mid_x = (left + right) * 0.5;
        let mid_z = (bottom + top) * 0.5;
        use crate::modules::aec::engine::construction_grid::TrackEnd;
        match hinge {
            LeafOperation::Turn { side } | LeafOperation::TiltTurn { side } => {
                let (hinge_x, strike_x) = match side {
                    TrackEnd::Start => (left, right),
                    TrackEnd::End => (right, left),
                };
                paths.push(path(
                    slot,
                    vec![(strike_x, bottom), (hinge_x, mid_z), (strike_x, top)],
                    false,
                    false,
                ));
                if matches!(hinge, LeafOperation::TiltTurn { .. }) {
                    paths.push(path(
                        slot,
                        vec![(left, bottom), (mid_x, top), (right, bottom)],
                        false,
                        false,
                    ));
                }
            }
            LeafOperation::Tilt => {
                paths.push(path(
                    slot,
                    vec![(left, bottom), (mid_x, top), (right, bottom)],
                    false,
                    false,
                ));
            }
            LeafOperation::Pivot { .. } => {
                paths.push(path(slot, vec![(mid_x, bottom), (mid_x, top)], false, false));
                paths.push(path(slot, vec![(left, mid_z), (right, mid_z)], false, false));
            }
            LeafOperation::Slide { direction } | LeafOperation::Fold { stack: direction } => {
                let (from, to) = match direction {
                    TrackEnd::Start => (right, left),
                    TrackEnd::End => (left, right),
                };
                paths.push(path(slot, vec![(from, mid_z), (to, mid_z)], false, false));
            }
        }
    }
    paths
}

fn bake_elevation_frame_level(
    params: OpeningBakeParams,
    fixed_face_width: f64,
    leaf_face_width: f64,
    level: ElevationFrameLevel,
    omit_threshold: bool,
) -> Vec<BakedPath> {
    let slot = OpeningComponentSlot::ElevationContour2D;
    let width_half = params.width * 0.5;
    match level {
        ElevationFrameLevel::Off => Vec::new(),
        ElevationFrameLevel::Outline => {
            bake_elevation_contour(slot, params, width_half, params.height, 0.0, omit_threshold)
                .into_iter()
                .take(1)
                .collect()
        }
        ElevationFrameLevel::FixedFrame => bake_elevation_contour(
            slot,
            params,
            width_half,
            params.height,
            fixed_face_width,
            omit_threshold,
        )
        .into_iter()
        .nth(1)
        .into_iter()
        .collect(),
        ElevationFrameLevel::FixedAndLeaf => {
            let fixed = bake_elevation_contour(
                slot,
                params,
                width_half,
                params.height,
                fixed_face_width,
                omit_threshold,
            );
            let leaf = bake_elevation_contour(
                slot,
                params,
                width_half,
                params.height,
                fixed_face_width + leaf_face_width,
                omit_threshold,
            );
            fixed
                .into_iter()
                .nth(1)
                .into_iter()
                .chain(leaf.into_iter().nth(1))
                .collect()
        }
    }
}

pub(crate) fn arch_height_at_x(params: OpeningBakeParams, x: f64) -> f64 {
    if params.shape != OpeningShape::Arch {
        return params.height;
    }
    let half_width = params.width * 0.5;
    if half_width <= 0.0 {
        return 0.0;
    }
    let spring = params.spring_height.min(params.height).max(0.0);
    let t = (1.0 - (x / half_width).powi(2)).max(0.0).sqrt();
    spring + (params.height - spring) * t
}

fn clipped_horizontal_span(params: OpeningBakeParams, y: f64, left: f64, right: f64) -> (f64, f64) {
    if params.shape != OpeningShape::Arch || y <= params.spring_height {
        return (left, right);
    }
    let half_width = params.width * 0.5;
    let rise = (params.height - params.spring_height).max(1e-9);
    let ratio = ((y - params.spring_height) / rise).clamp(0.0, 1.0);
    let arch_x = half_width * (1.0 - ratio * ratio).sqrt();
    (left.max(-arch_x), right.min(arch_x))
}

fn bake_grid_elevation_divisions(
    grid: &ConstructionGrid,
    params: OpeningBakeParams,
    level: DivisionLevel,
    library: Option<&StyleLibrary>,
    fallback: &ResolvedSection,
    leaf: &ResolvedSection,
) -> Vec<BakedPath> {
    if level == DivisionLevel::Off {
        return Vec::new();
    }
    let resolved = resolve_grid(grid, params.width, params.height);
    let mut paths = Vec::new();
    collect_elevation_divisions(
        &resolved,
        0.0,
        0.0,
        params,
        level,
        library,
        fallback,
        leaf,
        &mut paths,
    );
    paths
}

fn collect_elevation_divisions(
    resolved: &ResolvedGrid,
    offset_x: f64,
    offset_z: f64,
    params: OpeningBakeParams,
    level: DivisionLevel,
    library: Option<&StyleLibrary>,
    fallback: &ResolvedSection,
    leaf: &ResolvedSection,
    paths: &mut Vec<BakedPath>,
) {
    let origin = params.width * 0.5;
    for edge in &resolved.edges {
        if edge.meeting != TrackMeeting::Mullion {
            continue;
        }
        let section = named_division_section(
            library,
            edge.profile_id.as_deref(),
            resolved.muntin_profile_id.as_deref(),
            fallback,
        );
        push_elevation_bar(
            edge.axis,
            edge.position,
            edge.start,
            edge.end,
            offset_x,
            offset_z,
            origin,
            params,
            section.as_ref(),
            paths,
        );
    }
    for edge in &resolved.edges {
        let Some(shift) = stulp_shift(edge.meeting, leaf.face) else {
            continue;
        };
        push_elevation_bar(
            edge.axis,
            edge.position + shift,
            edge.start,
            edge.end,
            offset_x,
            offset_z,
            origin,
            params,
            Some(leaf),
            paths,
        );
        if let Some(path) = paths.last_mut() {
            path.look_role = Some(OpeningLookRole::Leaf);
        }
    }
    for cell in &resolved.cells {
        if level == DivisionLevel::PostsAndMuntins {
            for bar in &cell.bars {
                let Some(section) = muntin_bar_section(
                    library,
                    resolved.muntin_profile_id.as_deref(),
                    bar.face_width,
                    bar.depth,
                    leaf,
                ) else {
                    continue;
                };
                push_elevation_bar(
                    bar.axis,
                    bar.position,
                    bar.start,
                    bar.end,
                    offset_x,
                    offset_z,
                    origin,
                    params,
                    Some(&section),
                    paths,
                );
            }
        }
        if let Some(nested) = &cell.nested {
            collect_elevation_divisions(
                nested,
                offset_x + cell.left,
                offset_z + cell.bottom,
                params,
                level,
                library,
                fallback,
                leaf,
                paths,
            );
        }
    }
}

fn push_elevation_bar(
    axis: BarAxis,
    position: f64,
    start: f64,
    end: f64,
    offset_x: f64,
    offset_z: f64,
    origin: f64,
    params: OpeningBakeParams,
    profile: Option<&ResolvedSection>,
    paths: &mut Vec<BakedPath>,
) {
    match axis {
        BarAxis::Vertical => {
            let x = position + offset_x - origin;
            let bottom = start + offset_z;
            let top = (end + offset_z).min(arch_height_at_x(params, x));
            if top > bottom {
                paths.push(bake_elevation_vertical_division_path(x, bottom, top, profile));
            }
        }
        BarAxis::Horizontal => {
            let y = position + offset_z;
            let (left, right) = clipped_horizontal_span(
                params,
                y,
                start + offset_x - origin,
                end + offset_x - origin,
            );
            if right > left {
                paths.push(bake_elevation_horizontal_division_path(left, right, y, profile));
            }
        }
    }
}

fn section_for_profile(
    library: Option<&StyleLibrary>,
    profile_id: Option<&str>,
    fallback: &ResolvedSection,
) -> ResolvedSection {
    if profile_id.is_some() {
        resolve_component_section(library, profile_id, fallback.face, fallback.depth)
    } else {
        fallback.clone()
    }
}

/// A closed bar only when a named profile actually resolves. Otherwise the
/// post is the shared grid axis, not a copy of the frame section.
fn named_division_section(
    library: Option<&StyleLibrary>,
    edge_profile: Option<&str>,
    muntin_profile: Option<&str>,
    fallback: &ResolvedSection,
) -> Option<ResolvedSection> {
    let id = edge_profile.or(muntin_profile)?;
    let resolved = library
        .and_then(|lib| lib.find_section_profile(id))
        .is_some_and(|profile| profile.is_valid());
    if resolved || edge_profile.is_some() {
        Some(section_for_profile(library, Some(id), fallback))
    } else {
        None
    }
}

/// A valid muntin profile covers every bar of this grid. An unknown id keeps
/// the stored bar sizes. Empty sizes fall back to the leaf envelope. A zero
/// size without a resolvable profile draws no bar.
pub(crate) fn muntin_bar_section(
    library: Option<&StyleLibrary>,
    profile_id: Option<&str>,
    face_width: f64,
    depth: f64,
    leaf: &ResolvedSection,
) -> Option<ResolvedSection> {
    if let Some(profile) = profile_id
        .and_then(|id| library?.find_section_profile(id))
        .filter(|profile| profile.is_valid())
    {
        return Some(resolve_section(Some(profile), face_width, depth));
    }
    if face_width > 1e-9 && depth > 1e-9 {
        return Some(resolve_section(None, face_width, depth));
    }
    if face_width <= 1e-9 && depth <= 1e-9 && leaf.face > 1e-9 && leaf.depth > 1e-9 {
        return Some(leaf.clone());
    }
    None
}

/// A stulp sits on the bearer leaf only. `End` is the track after the meeting.
fn stulp_shift(meeting: TrackMeeting, face: f64) -> Option<f64> {
    let TrackMeeting::Stulp { bearer } = meeting else {
        return None;
    };
    if face <= 1e-9 {
        return None;
    }
    Some(match bearer {
        TrackEnd::End => face * 0.5,
        TrackEnd::Start => -face * 0.5,
    })
}

/// Glass stops at the inner edge of the surrounding bar and bites `inset` into
/// it, never past the bar.
fn flatten_resolved_cells(
    grid: &ConstructionGrid,
    width: f64,
    height: f64,
) -> Vec<crate::modules::aec::engine::construction_grid::ResolvedCell> {
    let resolved = resolve_grid(grid, width, height);
    let mut cells = Vec::new();
    collect_parent_cells(&resolved, -width * 0.5, 0.0, &mut cells);
    cells
}

fn collect_parent_cells(
    resolved: &ResolvedGrid,
    offset_x: f64,
    offset_z: f64,
    cells: &mut Vec<crate::modules::aec::engine::construction_grid::ResolvedCell>,
) {
    for cell in &resolved.cells {
        if let Some(nested) = &cell.nested {
            collect_parent_cells(nested, offset_x + cell.left, offset_z + cell.bottom, cells);
        } else {
            let mut shifted = cell.clone();
            shifted.left += offset_x;
            shifted.right += offset_x;
            shifted.bottom += offset_z;
            shifted.top += offset_z;
            cells.push(shifted);
        }
    }
}

pub(crate) fn glass_edge_into_bar(edge: f64, inward: f64, bar: f64, inset: f64) -> f64 {
    let bar = bar.max(0.0);
    let bite = inset.max(0.0).min(bar);
    edge + inward * (bar - bite)
}

#[derive(Clone, Copy)]
struct LeafBarBoundary {
    low: f64,
    high: f64,
}

#[derive(Clone, Copy, Default)]
struct LeafCellBoundaries {
    left: Option<LeafBarBoundary>,
    right: Option<LeafBarBoundary>,
    bottom: Option<LeafBarBoundary>,
    top: Option<LeafBarBoundary>,
}

#[derive(Clone)]
pub(crate) struct LeafCellGeometry {
    pub cell: crate::modules::aec::engine::construction_grid::ResolvedCell,
    pub left: f64,
    pub right: f64,
    pub bottom: f64,
    pub top: f64,
    pub pivot_start: f64,
    pub pivot_end: f64,
}

pub(crate) fn leaf_cells_with_bar_offsets(
    grid: &ConstructionGrid,
    width: f64,
    height: f64,
    frame_face: f64,
    division_face: Option<f64>,
    leaf_face: f64,
    overlap: f64,
    frame_visible: bool,
    door_opening: bool,
) -> Vec<LeafCellGeometry> {
    let width = width.max(0.0);
    let height = height.max(0.0);
    let half_width = width * 0.5;
    let frame_face = if frame_visible { frame_face.max(0.0) } else { 0.0 };
    let root = LeafCellBoundaries {
        left: (frame_face > 0.0).then_some(LeafBarBoundary {
            low: -half_width,
            high: -half_width + frame_face,
        }),
        right: (frame_face > 0.0).then_some(LeafBarBoundary {
            low: half_width - frame_face,
            high: half_width,
        }),
        bottom: (frame_face > 0.0 && !door_opening).then_some(LeafBarBoundary {
            low: 0.0,
            high: frame_face,
        }),
        top: (frame_face > 0.0).then_some(LeafBarBoundary {
            low: height - frame_face,
            high: height,
        }),
    };
    let mut cells = Vec::new();
    collect_leaf_cells_with_bar_offsets(
        &resolve_grid(grid, width, height),
        -half_width,
        0.0,
        width,
        height,
        root,
        division_face.filter(|face| *face > 0.0),
        leaf_face.max(0.0),
        overlap.max(0.0),
        &mut cells,
    );
    cells
}

fn collect_leaf_cells_with_bar_offsets(
    resolved: &ResolvedGrid,
    offset_x: f64,
    offset_z: f64,
    width: f64,
    height: f64,
    inherited: LeafCellBoundaries,
    division_face: Option<f64>,
    leaf_face: f64,
    overlap: f64,
    cells: &mut Vec<LeafCellGeometry>,
) {
    for cell in &resolved.cells {
        let left_bar = leaf_cell_side_bar(
            resolved,
            BarAxis::Vertical,
            cell.left,
            cell.bottom,
            cell.top,
            true,
            width,
            height,
            offset_x,
            offset_z,
            inherited.left,
            division_face,
            leaf_face,
        );
        let right_bar = leaf_cell_side_bar(
            resolved,
            BarAxis::Vertical,
            cell.right,
            cell.bottom,
            cell.top,
            false,
            width,
            height,
            offset_x,
            offset_z,
            inherited.right,
            division_face,
            leaf_face,
        );
        let bottom_bar = leaf_cell_side_bar(
            resolved,
            BarAxis::Horizontal,
            cell.bottom,
            cell.left,
            cell.right,
            true,
            width,
            height,
            offset_x,
            offset_z,
            inherited.bottom,
            division_face,
            leaf_face,
        );
        let top_bar = leaf_cell_side_bar(
            resolved,
            BarAxis::Horizontal,
            cell.top,
            cell.left,
            cell.right,
            false,
            width,
            height,
            offset_x,
            offset_z,
            inherited.top,
            division_face,
            leaf_face,
        );
        let boundaries = LeafCellBoundaries {
            left: left_bar,
            right: right_bar,
            bottom: bottom_bar,
            top: top_bar,
        };
        let left = leaf_edge_inside_bar(
            cell.left + offset_x,
            left_bar,
            true,
            overlap,
        );
        let right = leaf_edge_inside_bar(
            cell.right + offset_x,
            right_bar,
            false,
            overlap,
        );
        let bottom = leaf_edge_inside_bar(
            cell.bottom + offset_z,
            bottom_bar,
            true,
            overlap,
        );
        let top = leaf_edge_inside_bar(
            cell.top + offset_z,
            top_bar,
            false,
            overlap,
        );
        if let Some(nested) = &cell.nested {
            collect_leaf_cells_with_bar_offsets(
                nested,
                offset_x + cell.left,
                offset_z + cell.bottom,
                cell.right - cell.left,
                cell.top - cell.bottom,
                boundaries,
                division_face,
                leaf_face,
                overlap,
                cells,
            );
        } else if right > left && top > bottom {
            let mut leaf = cell.clone();
            leaf.nested = None;
            cells.push(LeafCellGeometry {
                cell: leaf,
                left,
                right,
                bottom,
                top,
                pivot_start: left_bar.map_or(cell.left + offset_x, |bar| bar.high),
                pivot_end: right_bar.map_or(cell.right + offset_x, |bar| bar.low),
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn leaf_cell_side_bar(
    resolved: &ResolvedGrid,
    axis: BarAxis,
    position: f64,
    span_start: f64,
    span_end: f64,
    positive_side: bool,
    width: f64,
    height: f64,
    offset_x: f64,
    offset_z: f64,
    inherited: Option<LeafBarBoundary>,
    division_face: Option<f64>,
    leaf_face: f64,
) -> Option<LeafBarBoundary> {
    let midpoint = (span_start + span_end) * 0.5;
    let edge = resolved.edges.iter().find(|edge| {
        edge.axis == axis
            && (edge.position - position).abs() <= 1e-6
            && edge.start <= midpoint + 1e-6
            && edge.end >= midpoint - 1e-6
    });
    if let Some(edge) = edge {
        let face = match edge.meeting {
            TrackMeeting::Mullion => division_face?,
            TrackMeeting::Stulp { .. } => leaf_face,
            TrackMeeting::Flush => 0.0,
        };
        if face > 0.0 {
            let center = edge.position + stulp_shift(edge.meeting, face).unwrap_or(0.0);
            return Some(LeafBarBoundary {
                low: center - face * 0.5 + if axis == BarAxis::Vertical { offset_x } else { offset_z },
                high: center + face * 0.5 + if axis == BarAxis::Vertical { offset_x } else { offset_z },
            });
        }
    }

    let at_outer = match axis {
        BarAxis::Vertical if positive_side => position.abs() <= 1e-6,
        BarAxis::Vertical => (position - width).abs() <= 1e-6,
        BarAxis::Horizontal if positive_side => position.abs() <= 1e-6,
        BarAxis::Horizontal => (position - height).abs() <= 1e-6,
    };
    at_outer.then_some(inherited).flatten()
}

fn leaf_edge_inside_bar(
    cell_edge: f64,
    bar: Option<LeafBarBoundary>,
    positive_side: bool,
    overlap: f64,
) -> f64 {
    let Some(bar) = bar else {
        return cell_edge;
    };
    if positive_side {
        (bar.high - overlap.min(bar.high - bar.low)).clamp(bar.low, bar.high)
    } else {
        (bar.low + overlap.min(bar.high - bar.low)).clamp(bar.low, bar.high)
    }
}

fn bake_elevation_vertical_division_path(
    x: f64,
    bottom: f64,
    top: f64,
    profile: Option<&ResolvedSection>,
) -> BakedPath {
    if let Some(profile) = profile {
        let half_face = profile.face * 0.5;
        path(
            OpeningComponentSlot::ElevationMuntins2D,
            vec![
                (x - half_face, bottom),
                (x + half_face, bottom),
                (x + half_face, top),
                (x - half_face, top),
            ],
            true,
            false,
        )
    } else {
        path(
            OpeningComponentSlot::ElevationMuntins2D,
            vec![(x, bottom), (x, top)],
            false,
            false,
        )
    }
}

fn bake_elevation_horizontal_division_path(
    start: f64,
    end: f64,
    position: f64,
    profile: Option<&ResolvedSection>,
) -> BakedPath {
    if let Some(profile) = profile {
        let half_face = profile.face * 0.5;
        path(
            OpeningComponentSlot::ElevationMuntins2D,
            vec![
                (start, position - half_face),
                (end, position - half_face),
                (end, position + half_face),
                (start, position + half_face),
            ],
            true,
            false,
        )
    } else {
        path(
            OpeningComponentSlot::ElevationMuntins2D,
            vec![(start, position), (end, position)],
            false,
            false,
        )
    }
}

pub fn extract_block_paths(
    doc: &acadrust::CadDocument,
    block_name: &str,
) -> Vec<(Vec<(f64, f64)>, bool)> {
    let Some(rec) = doc.block_records.get(block_name) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for entity in doc.entities() {
        if entity.common().owner_handle != rec.handle {
            continue;
        }
        match entity {
            EntityType::Line(line) => {
                paths.push((
                    vec![(line.start.x, line.start.y), (line.end.x, line.end.y)],
                    false,
                ));
            }
            EntityType::LwPolyline(pl) => {
                if pl.vertices.len() >= 2 {
                    let pts: Vec<(f64, f64)> = pl
                        .vertices
                        .iter()
                        .map(|v| (v.location.x, v.location.y))
                        .collect();
                    paths.push((pts, pl.is_closed));
                }
            }
            EntityType::Arc(arc) => {
                let n = 16;
                let start_a = arc.start_angle.to_radians();
                let end_a = arc.end_angle.to_radians();
                let sweep = if end_a >= start_a {
                    end_a - start_a
                } else {
                    end_a + 2.0 * std::f64::consts::PI - start_a
                };
                let pts: Vec<(f64, f64)> = (0..=n)
                    .map(|i| {
                        let a = start_a + sweep * (i as f64) / (n as f64);
                        (
                            arc.center.x + arc.radius * a.cos(),
                            arc.center.y + arc.radius * a.sin(),
                        )
                    })
                    .collect();
                paths.push((pts, false));
            }
            EntityType::Circle(circ) => {
                let n = 32;
                let pts: Vec<(f64, f64)> = (0..=n)
                    .map(|i| {
                        let a = 2.0 * std::f64::consts::PI * (i as f64) / (n as f64);
                        (
                            circ.center.x + circ.radius * a.cos(),
                            circ.center.y + circ.radius * a.sin(),
                        )
                    })
                    .collect();
                paths.push((pts, true));
            }
            _ => {}
        }
    }
    paths
}

fn bake_block_slot(
    slot: OpeningComponentSlot,
    block_name: &str,
    placement: BlockPlacementMode,
    p: OpeningBakeParams,
    doc: Option<&acadrust::CadDocument>,
) -> Vec<BakedPath> {
    let Some(doc) = doc else {
        return Vec::new();
    };
    let raw_paths = extract_block_paths(doc, block_name);
    if raw_paths.is_empty() {
        return Vec::new();
    }
    let mut min_u = f64::INFINITY;
    let mut max_u = f64::NEG_INFINITY;
    for (pts, _) in &raw_paths {
        for &(u, _) in pts {
            min_u = min_u.min(u);
            max_u = max_u.max(u);
        }
    }
    if !min_u.is_finite() || !max_u.is_finite() {
        return Vec::new();
    }
    let u_span = (max_u - min_u).max(1e-9);
    let hw = p.width * 0.5;

    let mut out = Vec::new();
    match placement {
        BlockPlacementMode::JambPair => {
            for (pts, closed) in &raw_paths {
                let left_pts: Vec<(f64, f64)> = pts
                    .iter()
                    .map(|&(u, v)| (-hw + (u - min_u), v))
                    .collect();
                out.push(path(slot, left_pts, *closed, false));
            }
            for (pts, closed) in &raw_paths {
                let right_pts: Vec<(f64, f64)> = pts
                    .iter()
                    .map(|&(u, v)| (hw - (u - min_u), v))
                    .collect();
                out.push(path(slot, right_pts, *closed, false));
            }
        }
        BlockPlacementMode::StretchToFit => {
            for (pts, closed) in &raw_paths {
                let stretched: Vec<(f64, f64)> = pts
                    .iter()
                    .map(|&(u, v)| (-hw + ((u - min_u) / u_span) * (2.0 * hw), v))
                    .collect();
                out.push(path(slot, stretched, *closed, false));
            }
        }
        BlockPlacementMode::CenterAnchor => {
            let u_mid = (min_u + max_u) * 0.5;
            for (pts, closed) in &raw_paths {
                let centered: Vec<(f64, f64)> = pts
                    .iter()
                    .map(|&(u, v)| (u - u_mid, v))
                    .collect();
                out.push(path(slot, centered, *closed, false));
            }
        }
    }
    out
}

fn bake_generator(
    slot: OpeningComponentSlot,
    gen: OpeningGenerator,
    p: OpeningBakeParams,
) -> Vec<BakedPath> {
    if p.width <= 1e-12 || p.thickness <= 1e-12 {
        return Vec::new();
    }
    let hw = p.width * 0.5;
    let ht = p.thickness * 0.5;
    let ft = p.frame_thickness.max(0.0);
    match gen {
        OpeningGenerator::None => Vec::new(),
        OpeningGenerator::FrameRect => bake_frame_rect(slot, p, hw, ht, ft),
        OpeningGenerator::DoorFrame => bake_door_frame(slot, hw, ht, ft),
        OpeningGenerator::LeafLine => bake_leaf_line(slot, p, hw, ht, ft),
        OpeningGenerator::SwingArc => bake_swing_arc(slot, p, hw, ft),
        OpeningGenerator::SillLines => bake_sill_lines(
            slot,
            hw,
            ht,
            ft,
            p.cross_axis_offset,
            p.wall_y_min,
            p.wall_y_max,
        ),
        OpeningGenerator::Cross => {
            let y_ext = p.wall_y_min - p.cross_axis_offset;
            let y_int = p.wall_y_max - p.cross_axis_offset;
            vec![
                path(slot, vec![(-hw, y_ext), (hw, y_int)], false, false),
                path(slot, vec![(-hw, y_int), (hw, y_ext)], false, false),
            ]
        }
        OpeningGenerator::DiagonalFill => {
            let y_ext = p.wall_y_min - p.cross_axis_offset;
            let y_int = p.wall_y_max - p.cross_axis_offset;
            vec![
                path(slot, vec![(-hw, y_ext), (hw, y_int)], false, false),
                path(
                    slot,
                    vec![(-hw, y_ext), (hw, y_ext), (hw, y_int), (-hw, y_int)],
                    true,
                    true,
                ),
            ]
        }
        OpeningGenerator::GlazingLine => bake_glazing_line(slot, hw, ft),
        OpeningGenerator::ThresholdLine => bake_threshold_line(slot, hw, ht),
        OpeningGenerator::OpeningLabel => bake_opening_label_preview(slot, hw),
        OpeningGenerator::ElevationFrameRect => {
            bake_elevation_contour(slot, p, hw, p.height, ft, p.kind == OpeningKind::Door)
        }
        OpeningGenerator::ElevationFrameArch => {
            bake_elevation_contour(slot, p, hw, p.height, ft, p.kind == OpeningKind::Door)
        }
        OpeningGenerator::ElevationMuntinsSingle => {
            bake_elevation_muntins_single(slot, hw, p.height, ft)
        }
        OpeningGenerator::ElevationMuntinsDouble => {
            bake_elevation_muntins_double(slot, hw, p.height, ft)
        }
        OpeningGenerator::ElevationSwingTriangle => {
            bake_elevation_swing_triangle(slot, p, hw, p.height, ft)
        }
        OpeningGenerator::ElevationSillLine => {
            bake_elevation_sill_line(slot, hw, p.height)
        }
        _ => Vec::new(),
    }
}

fn rough_opening_elevation_guide(params: OpeningBakeParams) -> BakedPath {
    let hw = params.width * 0.5;
    let head = params.height.max(0.0);
    path(
        OpeningComponentSlot::Mark2D,
        vec![
            (-hw, 0.0),
            (hw, 0.0),
            (hw, head),
            (-hw, head),
            (-hw, 0.0),
        ],
        false,
        false,
    )
}

fn path(
    slot: OpeningComponentSlot,
    points: Vec<(f64, f64)>,
    closed: bool,
    filled: bool,
) -> BakedPath {
    BakedPath {
        slot,
        look_role: None,
        points,
        points_3d: None,
        closed,
        filled,
    }
}

fn bake_frame_rect(
    slot: OpeningComponentSlot,
    p: OpeningBakeParams,
    hw: f64,
    ht: f64,
    ft: f64,
) -> Vec<BakedPath> {
    if p.kind == OpeningKind::Door {
        bake_door_frame(slot, hw, ht, ft)
    } else {
        bake_window_posts(slot, hw, ht, ft)
    }
}

fn bake_window_posts(
    slot: OpeningComponentSlot,
    hw: f64,
    ht: f64,
    ft: f64,
) -> Vec<BakedPath> {
    let frame_depth = (ft.max(0.07)).min(ht * 2.0);
    let half_depth = frame_depth * 0.5;
    let post_w = ft.min(hw * 0.45).max(1e-4);

    let left_post = vec![
        (-hw, -half_depth),
        (-hw + post_w, -half_depth),
        (-hw + post_w, half_depth),
        (-hw, half_depth),
    ];
    let right_post = vec![
        (hw - post_w, -half_depth),
        (hw, -half_depth),
        (hw, half_depth),
        (hw - post_w, half_depth),
    ];

    vec![
        path(slot, left_post, true, false),
        path(slot, right_post, true, false),
    ]
}

fn bake_door_frame(
    slot: OpeningComponentSlot,
    hw: f64,
    ht: f64,
    ft: f64,
) -> Vec<BakedPath> {
    let post_w = ft.min(hw * 0.45).max(1e-4);
    let rebate_w = post_w * 0.5;
    let left_jamb = vec![
        (-hw, -ht),
        (-hw + post_w, -ht),
        (-hw + post_w, 0.0),
        (-hw + rebate_w, 0.0),
        (-hw + rebate_w, ht),
        (-hw, ht),
    ];
    let right_jamb = vec![
        (hw, -ht),
        (hw - post_w, -ht),
        (hw - post_w, 0.0),
        (hw - rebate_w, 0.0),
        (hw - rebate_w, ht),
        (hw, ht),
    ];
    vec![
        path(slot, left_jamb, true, false),
        path(slot, right_jamb, true, false),
    ]
}

fn bake_leaf_line(
    slot: OpeningComponentSlot,
    p: OpeningBakeParams,
    hw: f64,
    _ht: f64,
    ft: f64,
) -> Vec<BakedPath> {
    match p.kind {
        OpeningKind::Door => {
            let (start, end) = leaf_segment(p.hinge, p.swing_side, hw, p.opening_angle_deg, ft);
            vec![path(slot, vec![start, end], false, false)]
        }
        _ => {
            let inset = ft.min(hw * 0.45);
            vec![path(
                slot,
                vec![(-hw + inset, 0.0), (hw - inset, 0.0)],
                false,
                false,
            )]
        }
    }
}

fn swing_toward_exterior(swing: SwingSide) -> f64 {
    match swing {
        SwingSide::Exterior => -1.0,
        SwingSide::Interior => 1.0,
    }
}

fn leaf_segment(
    hinge: LeafOperation,
    swing: SwingSide,
    hw: f64,
    angle_deg: f64,
    ft: f64,
) -> ((f64, f64), (f64, f64)) {
    let post_w = ft.min(hw * 0.45).max(1e-4);
    let rebate_w = post_w * 0.5;
    let clear_w = (hw - rebate_w) * 2.0;
    let ang = angle_deg.to_radians();
    let swing_sign = swing_toward_exterior(swing);
    match hinge.bearing() {
        crate::modules::aec::engine::construction_grid::TrackEnd::Start => {
            let start = (-hw + rebate_w, 0.0);
            let end = (
                -hw + rebate_w + clear_w * ang.cos(),
                swing_sign * clear_w * ang.sin(),
            );
            (start, end)
        }
        crate::modules::aec::engine::construction_grid::TrackEnd::End => {
            let start = (hw - rebate_w, 0.0);
            let end = (
                hw - rebate_w - clear_w * ang.cos(),
                swing_sign * clear_w * ang.sin(),
            );
            (start, end)
        }
    }
}

fn bake_swing_arc(slot: OpeningComponentSlot, p: OpeningBakeParams, hw: f64, ft: f64) -> Vec<BakedPath> {
    let post_w = if p.kind == OpeningKind::Door {
        ft.min(hw * 0.45).max(1e-4)
    } else {
        0.0
    };
    let rebate_w = post_w * 0.5;
    let clear_w = (hw - rebate_w) * 2.0;
    if clear_w <= 1e-12 {
        return Vec::new();
    }
    let ang = p.opening_angle_deg.to_radians().abs().max(1e-6);
    let n = SWING_CHORD_COUNT.max(4);
    let swing_sign = swing_toward_exterior(p.swing_side);
    let pts = match p.hinge.bearing() {
        crate::modules::aec::engine::construction_grid::TrackEnd::Start => {
            let cx = -hw + rebate_w;
            (0..=n)
                .map(|i| {
                    let t = ang * (i as f64) / (n as f64);
                    (cx + clear_w * t.cos(), swing_sign * clear_w * t.sin())
                })
                .collect()
        }
        crate::modules::aec::engine::construction_grid::TrackEnd::End => {
            let cx = hw - rebate_w;
            (0..=n)
                .map(|i| {
                    let t = ang * (i as f64) / (n as f64);
                    (cx - clear_w * t.cos(), swing_sign * clear_w * t.sin())
                })
                .collect()
        }
    };
    vec![path(slot, pts, false, false)]
}

fn bake_sill_lines(
    slot: OpeningComponentSlot,
    hw: f64,
    ht: f64,
    ft: f64,
    cross_offset: f64,
    wall_y_min: f64,
    wall_y_max: f64,
) -> Vec<BakedPath> {
    let frame_depth = (ft.max(0.07)).min(ht * 2.0);
    let frame_half_d = frame_depth * 0.5;

    let mut out = Vec::new();
    let sill_overhang = 0.035;
    let wall_ext_y = wall_y_min - cross_offset;
    let wall_int_y = wall_y_max - cross_offset;
    let sill_y_outer = wall_ext_y - sill_overhang;
    let sill_ear = 0.0;

    // Exterior sill front nose
    out.push(
        path(
            slot,
            vec![
                (-hw - sill_ear, sill_y_outer),
                (hw + sill_ear, sill_y_outer),
                (hw + sill_ear, wall_ext_y),
                (-hw - sill_ear, wall_ext_y),
            ],
            true,
            false,
        )
        .with_look_role(OpeningLookRole::ExteriorSill),
    );
    // Reveal line along exterior wall face
    out.push(
        path(
            slot,
            vec![(-hw, wall_ext_y), (hw, wall_ext_y)],
            false,
            false,
        )
        .with_look_role(OpeningLookRole::ExteriorSill),
    );

    // Exterior reveal side lines (connecting frame to exterior wall)
    if -frame_half_d > wall_ext_y + 1e-4 {
        out.push(
            path(
                slot,
                vec![(-hw, -frame_half_d), (-hw, wall_ext_y)],
                false,
                false,
            )
            .with_look_role(OpeningLookRole::ExteriorSill),
        );
        out.push(
            path(
                slot,
                vec![(hw, -frame_half_d), (hw, wall_ext_y)],
                false,
                false,
            )
            .with_look_role(OpeningLookRole::ExteriorSill),
        );
    }

    let board_overhang = 0.025;
    let board_y_inner = wall_int_y + board_overhang;
    let board_ear = 0.0;

    // Interior board front nose
    out.push(
        path(
            slot,
            vec![
                (-hw - board_ear, board_y_inner),
                (hw + board_ear, board_y_inner),
                (hw + board_ear, wall_int_y),
                (-hw - board_ear, wall_int_y),
            ],
            true,
            false,
        )
        .with_look_role(OpeningLookRole::InteriorBoard),
    );
    // Reveal line along interior wall face
    out.push(
        path(
            slot,
            vec![(-hw, wall_int_y), (hw, wall_int_y)],
            false,
            false,
        )
        .with_look_role(OpeningLookRole::InteriorBoard),
    );

    // Interior reveal side lines (connecting frame to interior wall)
    if frame_half_d < wall_int_y - 1e-4 {
        out.push(
            path(
                slot,
                vec![(-hw, frame_half_d), (-hw, wall_int_y)],
                false,
                false,
            )
            .with_look_role(OpeningLookRole::InteriorBoard),
        );
        out.push(
            path(
                slot,
                vec![(hw, frame_half_d), (hw, wall_int_y)],
                false,
                false,
            )
            .with_look_role(OpeningLookRole::InteriorBoard),
        );
    }

    out
}

fn bake_sill_lines_from_parts(
    slot: OpeningComponentSlot,
    params: OpeningBakeParams,
    parts: &OpeningParts,
    library: Option<&StyleLibrary>,
    fixed_frame: &ResolvedSection,
    exterior_sill: &ResolvedSection,
    interior_board: &ResolvedSection,
) -> Vec<BakedPath> {
    let hw = params.width * 0.5;
    let cross_offset = params.cross_axis_offset;
    let wall_ext_y = params.wall_y_min - cross_offset;
    let wall_int_y = params.wall_y_max - cross_offset;
    let frame_y_min = parts.fixed_frame.axis_offset - fixed_frame.depth * 0.5;
    let frame_y_max = parts.fixed_frame.axis_offset + fixed_frame.depth * 0.5;
    let mut out = Vec::new();

    if opening_component_should_render_with_library(
        parts,
        params.kind,
        OpeningLookRole::ExteriorSill,
        library,
    ) && exterior_sill.depth > 0.0
    {
        let overhang = parts.exterior_sill.overhang.max(0.0);
        let (x0, x1) = sill_x_span(
            hw,
            parts.exterior_sill.side_overhang_left,
            parts.exterior_sill.side_overhang_right,
        );
        let sill_outer_y = wall_ext_y - overhang;
        if overhang > 1e-9 || x0 < -hw - 1e-9 || x1 > hw + 1e-9 {
            out.push(
                path(
                    slot,
                    vec![(x0, sill_outer_y), (x1, sill_outer_y), (x1, wall_ext_y), (x0, wall_ext_y)],
                    true,
                    false,
                )
                .with_look_role(OpeningLookRole::ExteriorSill),
            );
        }
        out.push(
            path(slot, vec![(-hw, wall_ext_y), (hw, wall_ext_y)], false, false)
                .with_look_role(OpeningLookRole::ExteriorSill),
        );
        if frame_y_min > wall_ext_y + 1e-4 {
            for x in [-hw, hw] {
                out.push(
                    path(slot, vec![(x, frame_y_min), (x, wall_ext_y)], false, false)
                        .with_look_role(OpeningLookRole::ExteriorSill),
                );
            }
        }
    }

    if opening_component_should_render_with_library(
        parts,
        params.kind,
        OpeningLookRole::InteriorBoard,
        library,
    ) && interior_board.depth > 0.0
    {
        let overhang = parts.interior_board.overhang.max(0.0);
        let (x0, x1) = sill_x_span(
            hw,
            parts.interior_board.side_overhang_left,
            parts.interior_board.side_overhang_right,
        );
        let board_inner_y = wall_int_y + overhang;
        if overhang > 1e-9 || x0 < -hw - 1e-9 || x1 > hw + 1e-9 {
            out.push(
                path(
                    slot,
                    vec![
                        (x0, board_inner_y),
                        (x1, board_inner_y),
                        (x1, wall_int_y),
                        (x0, wall_int_y),
                    ],
                    true,
                    false,
                )
                .with_look_role(OpeningLookRole::InteriorBoard),
            );
        }
        out.push(
            path(slot, vec![(-hw, wall_int_y), (hw, wall_int_y)], false, false)
                .with_look_role(OpeningLookRole::InteriorBoard),
        );
        if frame_y_max < wall_int_y - 1e-4 {
            for x in [-hw, hw] {
                out.push(
                    path(slot, vec![(x, frame_y_max), (x, wall_int_y)], false, false)
                        .with_look_role(OpeningLookRole::InteriorBoard),
                );
            }
        }
    }
    out
}

fn bake_glazing_line(slot: OpeningComponentSlot, hw: f64, ft: f64) -> Vec<BakedPath> {
    let post_w = ft.min(hw * 0.45).max(1e-4);
    let clear_hw = (hw - post_w).max(0.0);
    if clear_hw <= 1e-6 {
        return Vec::new();
    }
    vec![
        path(slot, vec![(-clear_hw, -0.012), (clear_hw, -0.012)], false, false),
        path(slot, vec![(-clear_hw, 0.012), (clear_hw, 0.012)], false, false),
    ]
}

fn bake_threshold_line(slot: OpeningComponentSlot, hw: f64, ht: f64) -> Vec<BakedPath> {
    let t_w = (0.05_f64).min(ht);
    vec![path(
        slot,
        vec![(-hw, -t_w), (hw, -t_w), (hw, t_w), (-hw, t_w)],
        true,
        false,
    )]
}

fn bake_opening_label_preview(slot: OpeningComponentSlot, hw: f64) -> Vec<BakedPath> {
    let stroke = hw.min(0.2);
    vec![path(slot, vec![(-stroke, 0.0), (stroke, 0.0)], false, false)]
}

fn bake_elevation_contour(
    slot: OpeningComponentSlot,
    p: OpeningBakeParams,
    hw: f64,
    h: f64,
    ft: f64,
    omit_threshold: bool,
) -> Vec<BakedPath> {
    match p.shape {
        OpeningShape::Arch => {
            let spring = p.spring_height.min(h).max(0.0);
            let mut outer = vec![(-hw, 0.0), (hw, 0.0), (hw, spring)];
            let n = 16;
            for i in 0..=n {
                let angle = (i as f64) / (n as f64) * std::f64::consts::PI;
                let x = hw * angle.cos();
                let z = spring + (h - spring) * angle.sin();
                outer.push((x, z));
            }
            outer.push((-hw, spring));
            let mut res = vec![path(slot, outer, true, false)];
            if ft > 1e-4 && hw > ft && h > 2.0 * ft {
                let inner_hw = hw - ft;
                let inner_h = h - ft;
                let bot = if omit_threshold { 0.0 } else { ft };
                let mut inner = vec![(-inner_hw, bot), (inner_hw, bot), (inner_hw, spring)];
                for i in 0..=n {
                    let angle = (i as f64) / (n as f64) * std::f64::consts::PI;
                    let x = inner_hw * angle.cos();
                    let z = spring + (inner_h - spring) * angle.sin();
                    inner.push((x, z));
                }
                inner.push((-inner_hw, spring));
                res.push(path(slot, inner, true, false));
            }
            res
        }
        _ => {
            let outer = vec![(-hw, 0.0), (hw, 0.0), (hw, h), (-hw, h)];
            let mut res = vec![path(slot, outer, true, false)];
            if ft > 1e-4 && hw > ft && h > 2.0 * ft {
                let bot = if omit_threshold { 0.0 } else { ft };
                let inner = vec![
                    (-hw + ft, bot),
                    (hw - ft, bot),
                    (hw - ft, h - ft),
                    (-hw + ft, h - ft),
                ];
                res.push(path(slot, inner, true, false));
            }
            res
        }
    }
}

fn bake_elevation_muntins_single(
    slot: OpeningComponentSlot,
    hw: f64,
    h: f64,
    ft: f64,
) -> Vec<BakedPath> {
    if hw <= ft || h <= 2.0 * ft {
        return Vec::new();
    }
    vec![path(
        slot,
        vec![
            (-hw + ft, ft),
            (hw - ft, ft),
            (hw - ft, h - ft),
            (-hw + ft, h - ft),
        ],
        true,
        false,
    )]
}

fn bake_elevation_muntins_double(
    slot: OpeningComponentSlot,
    hw: f64,
    h: f64,
    ft: f64,
) -> Vec<BakedPath> {
    if hw <= ft || h <= 2.0 * ft {
        return Vec::new();
    }
    let mullion_half = ft * 0.5;
    let left_leaf = vec![
        (-hw + ft, ft),
        (-mullion_half, ft),
        (-mullion_half, h - ft),
        (-hw + ft, h - ft),
    ];
    let right_leaf = vec![
        (mullion_half, ft),
        (hw - ft, ft),
        (hw - ft, h - ft),
        (mullion_half, h - ft),
    ];
    let mullion = vec![
        (-mullion_half, 0.0),
        (mullion_half, 0.0),
        (mullion_half, h),
        (-mullion_half, h),
    ];
    vec![
        path(slot, mullion, true, false),
        path(slot, left_leaf, true, false),
        path(slot, right_leaf, true, false),
    ]
}

fn bake_elevation_swing_triangle(
    slot: OpeningComponentSlot,
    p: OpeningBakeParams,
    hw: f64,
    h: f64,
    ft: f64,
) -> Vec<BakedPath> {
    let inset_w = (hw - ft).max(0.0);
    let inset_bot = ft;
    let inset_top = (h - ft).max(ft);
    let mid_h = (inset_bot + inset_top) * 0.5;

    match p.hinge.bearing() {
        crate::modules::aec::engine::construction_grid::TrackEnd::Start => {
            let pts = vec![
                (inset_w, inset_bot),
                (-inset_w, mid_h),
                (inset_w, inset_top),
            ];
            vec![path(slot, pts, false, false)]
        }
        crate::modules::aec::engine::construction_grid::TrackEnd::End => {
            let pts = vec![
                (-inset_w, inset_bot),
                (inset_w, mid_h),
                (-inset_w, inset_top),
            ];
            vec![path(slot, pts, false, false)]
        }
    }
}

fn bake_elevation_sill_line(slot: OpeningComponentSlot, hw: f64, _h: f64) -> Vec<BakedPath> {
    let ear = 0.0;
    let sill_drop = -0.05;
    vec![
        path(slot, vec![(-hw - ear, 0.0), (hw + ear, 0.0)], false, false),
        path(
            slot,
            vec![
                (-hw - ear, 0.0),
                (-hw - ear, sill_drop),
                (hw + ear, sill_drop),
                (hw + ear, 0.0),
            ],
            true,
            false,
        ),
    ]
}

fn format_din1356_num(v: f64) -> String {
    let s3 = format!("{v:.3}");
    if s3.ends_with('0') {
        format!("{v:.2}")
    } else {
        s3
    }
}

/// Formats the opening annotation according to DIN 1356:
/// - Doors: `W / H`
/// - Windows: `W / H` and `BRH ...`
/// - Breakthroughs: `W / H` and `UK ...`
pub fn format_din1356_label(opening: &Opening) -> String {
    let w = format_din1356_num(opening.width);
    let h = format_din1356_num(opening.height);
    let sill = format_din1356_num(opening.sill_height);
    match opening.kind {
        OpeningKind::Door => {
            format!("{w} / {h}")
        }
        OpeningKind::Window => {
            format!("{w} / {h}\\PBRH {sill}")
        }
        OpeningKind::Breakthrough => {
            if let Some(d) = opening.depth {
                let d_str = format_din1356_num(d);
                format!("{w} / {h} / {d_str}\\PUK {sill}")
            } else {
                format!("{w} / {h}\\PUK {sill}")
            }
        }
    }
}

pub fn opening_elevation_loops(
    shape: OpeningShape,
    width: f64,
    height: f64,
    spring_height: f64,
    frame_thickness: f64,
    is_door: bool,
) -> (Vec<(f64, f64)>, Vec<(f64, f64)>) {
    let hw = width * 0.5;
    let ft = frame_thickness.max(0.01).min(hw * 0.45);
    let bot_in = if is_door { 0.0 } else { ft };

    match shape {
        OpeningShape::Rectangle => {
            let outer = vec![
                (-hw, 0.0),
                (hw, 0.0),
                (hw, height),
                (-hw, height),
            ];
            let inner = vec![
                (-hw + ft, bot_in),
                (hw - ft, bot_in),
                (hw - ft, (height - ft).max(bot_in + 0.01)),
                (-hw + ft, (height - ft).max(bot_in + 0.01)),
            ];
            (outer, inner)
        }
        OpeningShape::Circle => {
            let (w, h) = shape.lock_size(width, height, true);
            let r_out = w * 0.5;
            let cy = h * 0.5;
            let r_in = (r_out - ft).max(0.01);
            let n = crate::modules::aec::engine::opening_shape::CIRCLE_CHORD_COUNT.max(16);
            let mut outer = Vec::with_capacity(n);
            let mut inner = Vec::with_capacity(n);
            for i in 0..n {
                let t = (i as f64) * std::f64::consts::TAU / (n as f64);
                outer.push((r_out * t.cos(), cy + r_out * t.sin()));
                inner.push((r_in * t.cos(), cy + r_in * t.sin()));
            }
            (outer, inner)
        }
        OpeningShape::Arch => {
            let spring = crate::modules::aec::engine::opening_shape::clamp_spring(spring_height, height);
            let rise = height - spring;
            let mut outer = vec![(-hw, 0.0), (hw, 0.0), (hw, spring)];
            let n = crate::modules::aec::engine::opening_shape::ARCH_CHORD_COUNT.max(8);
            if rise > 1e-6 {
                let chord = width;
                let radius = chord * chord / (8.0 * rise) + rise * 0.5;
                let cy = height - radius;
                let a0 = (spring - cy).atan2(hw);
                let mut a1 = (spring - cy).atan2(-hw);
                while a1 <= a0 {
                    a1 += std::f64::consts::TAU;
                }
                for i in 1..n {
                    let t = a0 + (a1 - a0) * (i as f64) / (n as f64);
                    outer.push((radius * t.cos(), cy + radius * t.sin()));
                }
            }
            outer.push((-hw, spring));

            let in_hw = (hw - ft).max(0.01);
            let in_h = (height - ft).max(bot_in + 0.02);
            let in_spring = crate::modules::aec::engine::opening_shape::clamp_spring(spring, in_h);
            let in_rise = in_h - in_spring;
            let mut inner = vec![(-in_hw, bot_in), (in_hw, bot_in), (in_hw, in_spring)];
            if in_rise > 1e-6 {
                let in_chord = 2.0 * in_hw;
                let in_radius = in_chord * in_chord / (8.0 * in_rise) + in_rise * 0.5;
                let in_cy = in_h - in_radius;
                let a0 = (in_spring - in_cy).atan2(in_hw);
                let mut a1 = (in_spring - in_cy).atan2(-in_hw);
                while a1 <= a0 {
                    a1 += std::f64::consts::TAU;
                }
                for i in 1..n {
                    let t = a0 + (a1 - a0) * (i as f64) / (n as f64);
                    inner.push((in_radius * t.cos(), in_cy + in_radius * t.sin()));
                }
            }
            inner.push((-in_hw, in_spring));

            (outer, inner)
        }
        OpeningShape::Triangle(variant) => {
            let outer_raw = crate::modules::aec::engine::opening_shape::triangle_polygon(variant, width, height);
            let outer: Vec<(f64, f64)> = outer_raw.into_iter().map(|(s, z)| (s - hw, z)).collect();
            let cx = outer.iter().map(|p| p.0).sum::<f64>() / 3.0;
            let cz = outer.iter().map(|p| p.1).sum::<f64>() / 3.0;
            let scale = ((hw - ft) / hw).max(0.1).min(0.95);
            let inner: Vec<(f64, f64)> = outer.iter().map(|&(x, z)| (cx + (x - cx) * scale, cz + (z - cz) * scale)).collect();
            (outer, inner)
        }
    }
}

pub fn build_faceted_hollow_prism(
    outer_loop: &[(f64, f64)],
    inner_loop: &[(f64, f64)],
    y_min: f64,
    y_max: f64,
) -> Option<cadkernel::brep::Body> {
    let n = outer_loop.len();
    if n < 3 || inner_loop.len() != n {
        return None;
    }

    let mut vertices: Vec<[f64; 3]> = Vec::with_capacity(4 * n);
    for &(x, z) in outer_loop {
        vertices.push([x, y_min, z]);
    }
    for &(x, z) in outer_loop {
        vertices.push([x, y_max, z]);
    }
    for &(x, z) in inner_loop {
        vertices.push([x, y_min, z]);
    }
    for &(x, z) in inner_loop {
        vertices.push([x, y_max, z]);
    }

    let mut faces: Vec<Vec<usize>> = Vec::with_capacity(4 * n);
    for i in 0..n {
        let next = (i + 1) % n;
        // Outer side face (y_min to y_max)
        faces.push(vec![i, next, next + n, i + n]);
        // Inner side face (normal points inwards)
        faces.push(vec![2 * n + i, 3 * n + i, 3 * n + next, 2 * n + next]);
        // Back face quad (y = y_min, normal -Y)
        faces.push(vec![i, 2 * n + i, 2 * n + next, next]);
        // Front face quad (y = y_max, normal +Y)
        faces.push(vec![n + i, n + next, 3 * n + next, 3 * n + i]);
    }

    cadkernel::brep::make::faceted_solid(&vertices, &faces)
}

pub fn build_faceted_prism(
    polygon: &[(f64, f64)],
    y_min: f64,
    y_max: f64,
) -> Option<cadkernel::brep::Body> {
    let n = polygon.len();
    if n < 3 {
        return None;
    }

    let mut vertices: Vec<[f64; 3]> = Vec::with_capacity(2 * n);
    for &(x, z) in polygon {
        vertices.push([x, y_min, z]);
    }
    for &(x, z) in polygon {
        vertices.push([x, y_max, z]);
    }

    let mut faces: Vec<Vec<usize>> = Vec::with_capacity(n + 2);
    for i in 0..n {
        let next = (i + 1) % n;
        faces.push(vec![i, next, next + n, i + n]);
    }
    let back_face: Vec<usize> = (0..n).rev().collect();
    faces.push(back_face);
    let front_face: Vec<usize> = (n..2 * n).collect();
    faces.push(front_face);

    cadkernel::brep::make::faceted_solid(&vertices, &faces)
}

pub fn build_opening_frame_3d(
    width: f64,
    height: f64,
    frame_thickness: f64,
    depth: f64,
    shape: OpeningShape,
    spring_height: f64,
) -> Option<cadkernel::brep::Body> {
    build_opening_frame_3d_with_threshold(
        width,
        height,
        frame_thickness,
        depth,
        shape,
        spring_height,
        true,
    )
}

pub fn build_opening_frame_3d_with_threshold(
    width: f64,
    height: f64,
    frame_thickness: f64,
    depth: f64,
    shape: OpeningShape,
    spring_height: f64,
    include_threshold: bool,
) -> Option<cadkernel::brep::Body> {
    let hd = depth * 0.5;
    let ft = frame_thickness.max(0.02).min(width * 0.5 * 0.45);
    let (outer, inner) = opening_elevation_loops(
        shape,
        width,
        height,
        spring_height,
        ft,
        !include_threshold,
    );
    build_faceted_hollow_prism(&outer, &inner, -hd, hd)
}

pub fn build_opening_leaf_3d(
    width: f64,
    height: f64,
    frame_thickness: f64,
    opening_angle_deg: f64,
    hinge: LeafOperation,
    swing: SwingSide,
    is_door: bool,
    shape: OpeningShape,
    spring_height: f64,
) -> Option<cadkernel::brep::Body> {
    build_opening_leaf_3d_with_depth(
        width,
        height,
        frame_thickness,
        0.04,
        opening_angle_deg,
        hinge,
        swing,
        is_door,
        shape,
        spring_height,
    )
}

pub fn build_opening_leaf_3d_with_depth(
    width: f64,
    height: f64,
    frame_thickness: f64,
    leaf_depth: f64,
    opening_angle_deg: f64,
    hinge: LeafOperation,
    swing: SwingSide,
    is_door: bool,
    shape: OpeningShape,
    spring_height: f64,
) -> Option<cadkernel::brep::Body> {
    build_opening_leaf_ring(
        width,
        height,
        frame_thickness,
        leaf_depth,
        0.0,
        0.0,
        opening_angle_deg,
        hinge,
        swing,
        is_door,
        shape,
        spring_height,
    )
}

/// Leaf as a frame ring. `lateral_overlap` grows the ring past the cell on both
/// sides; `depth_offset` shifts it along the wall normal. A ring that closes
/// the cell falls back to the solid plate.
pub fn build_opening_leaf_ring(
    width: f64,
    height: f64,
    frame_thickness: f64,
    leaf_depth: f64,
    lateral_overlap: f64,
    depth_offset: f64,
    opening_angle_deg: f64,
    hinge: LeafOperation,
    swing: SwingSide,
    is_door: bool,
    shape: OpeningShape,
    spring_height: f64,
) -> Option<cadkernel::brep::Body> {
    let grown = (width + lateral_overlap * 2.0).max(width);
    let hw = grown * 0.5;
    let ft = frame_thickness.max(0.02).min(hw * 0.45);
    let hinge_x = match hinge.bearing() {
        crate::modules::aec::engine::construction_grid::TrackEnd::Start => -hw + ft,
        crate::modules::aec::engine::construction_grid::TrackEnd::End => hw - ft,
    };
    build_opening_leaf_components_3d(
        grown,
        height,
        frame_thickness,
        leaf_depth,
        depth_offset,
        hinge_x,
        opening_angle_deg,
        hinge,
        swing,
        is_door,
        shape,
        spring_height,
        None,
    )
    .map(|(leaf, _)| leaf)
}

pub(crate) fn build_opening_leaf_components_3d(
    width: f64,
    height: f64,
    frame_thickness: f64,
    leaf_depth: f64,
    depth_offset: f64,
    pivot_x: f64,
    opening_angle_deg: f64,
    hinge: LeafOperation,
    swing: SwingSide,
    is_door: bool,
    shape: OpeningShape,
    spring_height: f64,
    glazing_thickness: Option<f64>,
) -> Option<(cadkernel::brep::Body, Option<cadkernel::brep::Body>)> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let hw = width * 0.5;
    let ft = frame_thickness.max(0.02).min(hw * 0.45);
    let (outer, inner) = opening_elevation_loops(shape, width, height, spring_height, ft, is_door);
    let leaf_half_depth = leaf_depth.max(0.001) * 0.5;
    let y0 = -leaf_half_depth + depth_offset;
    let y1 = leaf_half_depth + depth_offset;
    let closed = inner.len() < 3
        || inner.iter().all(|(x, z)| {
            outer
                .first()
                .map(|(ox, oz)| (x - ox).abs() < 1e-6 && (z - oz).abs() < 1e-6)
                .unwrap_or(true)
        });
    let leaf = if closed || is_door {
        build_faceted_prism(&outer, y0, y1)?
    } else {
        build_faceted_hollow_prism(&outer, &inner, y0, y1)?
    };
    let glass = if !is_door && inner.len() >= 3 {
        glazing_thickness.and_then(|thickness| {
            let half_glass = thickness.max(0.001).min(leaf_depth.max(0.001)) * 0.5;
            build_faceted_prism(
                &inner,
                depth_offset - half_glass,
                depth_offset + half_glass,
            )
        })
    } else {
        None
    };

    let swing_sign = swing_toward_exterior(swing);
    let ang = match hinge.bearing() {
        crate::modules::aec::engine::construction_grid::TrackEnd::Start => swing_sign * opening_angle_deg.to_radians(),
        crate::modules::aec::engine::construction_grid::TrackEnd::End => -swing_sign * opening_angle_deg.to_radians(),
    };
    let pivot = [pivot_x, depth_offset, 0.0];
    let leaf = if ang.abs() > 1e-6 {
        crate::scene::model::solid_model::turned(&leaf, 2, ang, pivot)?
    } else {
        leaf
    };
    let glass = glass.and_then(|glass| {
        if ang.abs() > 1e-6 {
            crate::scene::model::solid_model::turned(&glass, 2, ang, pivot)
        } else {
            Some(glass)
        }
    });
    Some((leaf, glass))
}

pub(crate) fn build_opening_box_3d(
    width: f64,
    height: f64,
    depth: f64,
) -> Option<cadkernel::brep::Body> {
    if width <= 0.0 || height <= 0.0 || depth <= 0.0 {
        return None;
    }
    build_faceted_prism(
        &[
            (-width * 0.5, -height * 0.5),
            (width * 0.5, -height * 0.5),
            (width * 0.5, height * 0.5),
            (-width * 0.5, height * 0.5),
        ],
        -depth * 0.5,
        depth * 0.5,
    )
}

fn build_opening_box_3d_with_bounds(
    width: f64,
    height: f64,
    y_min: f64,
    y_max: f64,
) -> Option<cadkernel::brep::Body> {
    if width <= 0.0 || height <= 0.0 || y_max <= y_min {
        return None;
    }
    build_faceted_prism(
        &[
            (-width * 0.5, -height * 0.5),
            (width * 0.5, -height * 0.5),
            (width * 0.5, height * 0.5),
            (-width * 0.5, height * 0.5),
        ],
        y_min,
        y_max,
    )
}

pub(crate) fn build_opening_profile_prism_3d(
    width: f64,
    section: &ResolvedSection,
    y_origin: f64,
    inward: f64,
) -> Option<cadkernel::brep::Body> {
    if width <= 0.0 || section.face <= 0.0 || section.depth <= 0.0 {
        return None;
    }
    let mut outline = section.outline.clone();
    if outline.len() > 1 && outline.first() == outline.last() {
        outline.pop();
    }
    if outline.len() < 3 {
        return None;
    }
    let n = outline.len();
    let mut vertices = Vec::with_capacity(n * 2);
    for x in [-width * 0.5, width * 0.5] {
        for &(face, depth) in &outline {
            vertices.push([x, y_origin + inward * depth, face]);
        }
    }
    let mut faces = Vec::with_capacity(n + 2);
    for i in 0..n {
        let next = (i + 1) % n;
        faces.push(vec![i, next, next + n, i + n]);
    }
    faces.push((0..n).rev().collect());
    faces.push((n..2 * n).collect());
    cadkernel::brep::make::faceted_solid(&vertices, &faces)
}

pub(crate) fn build_opening_grid_bars_3d(
    grid: &ConstructionGrid,
    params: OpeningBakeParams,
    post_width: f64,
    depth: f64,
    library: Option<&StyleLibrary>,
    leaf: &ResolvedSection,
) -> Vec<(cadkernel::brep::Body, f64, f64)> {
    let resolved = resolve_grid(grid, params.width, params.height);
    let mut bars = Vec::new();
    collect_grid_bars_3d(
        &resolved,
        0.0,
        0.0,
        params,
        post_width,
        depth,
        library,
        leaf,
        &mut bars,
    );
    bars
}

fn collect_grid_bars_3d(
    resolved: &ResolvedGrid,
    offset_x: f64,
    offset_z: f64,
    params: OpeningBakeParams,
    post_width: f64,
    depth: f64,
    library: Option<&StyleLibrary>,
    leaf: &ResolvedSection,
    bars: &mut Vec<(cadkernel::brep::Body, f64, f64)>,
) {
    let origin = params.width * 0.5;
    let mut add_bar = |left: f64, right: f64, bottom: f64, top: f64| {
        if right <= left || top <= bottom || depth <= 0.0 {
            return;
        }
        if let Some(body) = build_opening_box_3d(right - left, top - bottom, depth) {
            bars.push((body, (left + right) * 0.5, (bottom + top) * 0.5));
        }
    };
    for edge in &resolved.edges {
        if edge.meeting != TrackMeeting::Mullion {
            continue;
        }
        match edge.axis {
            BarAxis::Vertical => {
                let x = edge.position + offset_x - origin;
                let bottom = edge.start + offset_z;
                let top = (edge.end + offset_z).min(arch_height_at_x(params, x));
                add_bar(x - post_width * 0.5, x + post_width * 0.5, bottom, top);
            }
            BarAxis::Horizontal => {
                let z = edge.position + offset_z;
                let (left, right) = clipped_horizontal_span(
                    params,
                    z,
                    edge.start + offset_x - origin,
                    edge.end + offset_x - origin,
                );
                add_bar(left, right, z - post_width * 0.5, z + post_width * 0.5);
            }
        }
    }
    for edge in &resolved.edges {
        let Some(shift) = stulp_shift(edge.meeting, leaf.face) else {
            continue;
        };
        if leaf.depth <= 0.0 {
            continue;
        }
        match edge.axis {
            BarAxis::Vertical => {
                let x = edge.position + offset_x - origin + shift;
                let bottom = edge.start + offset_z;
                let top = (edge.end + offset_z).min(arch_height_at_x(params, x));
                if let Some(body) = build_opening_box_3d(leaf.face, top - bottom, leaf.depth) {
                    bars.push((body, x, (bottom + top) * 0.5));
                }
            }
            BarAxis::Horizontal => {
                let z = edge.position + offset_z + shift;
                let (left, right) = clipped_horizontal_span(
                    params,
                    z,
                    edge.start + offset_x - origin,
                    edge.end + offset_x - origin,
                );
                if let Some(body) = build_opening_box_3d(right - left, leaf.face, leaf.depth) {
                    bars.push((body, (left + right) * 0.5, z));
                }
            }
        }
    }
    for cell in &resolved.cells {
        for bar in &cell.bars {
            let Some(section) = muntin_bar_section(
                library,
                resolved.muntin_profile_id.as_deref(),
                bar.face_width,
                bar.depth,
                leaf,
            ) else {
                continue;
            };
            let face = section.face;
            let bar_depth = section.depth;
            match bar.axis {
                BarAxis::Vertical => {
                    let x = bar.position + offset_x - origin;
                    let bottom = bar.start + offset_z;
                    let top = (bar.end + offset_z).min(arch_height_at_x(params, x));
                    if let Some(body) = build_opening_box_3d(face, top - bottom, bar_depth) {
                        bars.push((body, x, (bottom + top) * 0.5));
                    }
                }
                BarAxis::Horizontal => {
                    let z = bar.position + offset_z;
                    let (left, right) = clipped_horizontal_span(
                        params,
                        z,
                        bar.start + offset_x - origin,
                        bar.end + offset_x - origin,
                    );
                    if let Some(body) = build_opening_box_3d(right - left, face, bar_depth) {
                        bars.push((body, (left + right) * 0.5, z));
                    }
                }
            }
        }
        if let Some(nested) = &cell.nested {
            collect_grid_bars_3d(
                nested,
                offset_x + cell.left,
                offset_z + cell.bottom,
                params,
                post_width,
                depth,
                library,
                leaf,
                bars,
            );
        }
    }
}

fn opening_look_line_style(
    style: &OpeningStyle,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
    role: OpeningLookRole,
    slot: OpeningComponentSlot,
) -> (Option<String>, Option<acadrust::types::Color>) {
    let look = style.looks.get(role);
    let material = style
        .material_id_for_look(role)
        .and_then(|id| library?.materials.iter().find(|material| material.id == id));
    let mut line_type = look
        .line_type
        .clone()
        .or_else(|| material.map(|material| material.line_type.clone()));
    let mut line_color = look.line_color.or_else(|| {
        material.map(|material| {
            let rgb = material.line_color;
            acadrust::types::Color::Rgb {
                r: ((rgb >> 16) & 0xff) as u8,
                g: ((rgb >> 8) & 0xff) as u8,
                b: (rgb & 0xff) as u8,
            }
        })
    });
    if let Some(override_style) = rules.and_then(|rules| rules.style_override.get(slot.key())) {
        if override_style.line_type.is_some() {
            line_type = override_style.line_type.clone();
        }
        if override_style.line_color.is_some() {
            line_color = override_style.line_color;
        }
    }
    (line_type, line_color)
}

pub const AEC_OPENING_GRID_LAYER: &str = "AEC_OPENING_GRID";

fn ensure_opening_grid_layer(scene: &mut Scene, prefix: Option<&str>) -> String {
    let name = crate::modules::aec::engine::wall_package::prefix_layer(AEC_OPENING_GRID_LAYER, prefix);
    let existed = scene.document.layers.contains(&name);
    scene.ensure_layer(&name);
    if !existed {
        if let Some(layer) = scene.document.layers.get_mut(&name) {
            layer.color = acadrust::types::Color::Index(8);
            layer.flags.off = false;
            layer.is_plottable = true;
        }
    }
    name
}

fn apply_opening_look_to_entity(
    entity: &mut EntityType,
    style: &OpeningStyle,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
    role: OpeningLookRole,
    slot: OpeningComponentSlot,
) {
    let (line_type, line_color) = opening_look_line_style(style, library, rules, role, slot);
    if let Some(line_type) = line_type {
        entity.common_mut().linetype = line_type;
    }
    if let Some(line_color) = line_color {
        entity.common_mut().color = line_color;
    }
}

fn opening_native_material_handle(
    style: &OpeningStyle,
    library: Option<&StyleLibrary>,
    role: OpeningLookRole,
    scene: &Scene,
) -> Option<Handle> {
    let material_id = style.material_id_for_look(role)?;
    let material = library?
        .materials
        .iter()
        .find(|material| material.id == material_id)?;
    let names = [
        Some(material_id),
        Some(material.name.as_str()),
        material.render_material_ref.as_deref(),
    ];
    scene.document.objects.iter().find_map(|(handle, object)| {
        let acadrust::objects::ObjectType::Material(native) = object else {
            return None;
        };
        names
            .iter()
            .flatten()
            .any(|name| native.name.eq_ignore_ascii_case(name))
            .then_some(*handle)
    })
}

fn emit_opening_solid_3d(
    scene: &mut Scene,
    body: cadkernel::brep::Body,
    opening_handle: Handle,
    opening_owner_handle: Handle,
    opening_layer: Option<&str>,
    origin: (f64, f64),
    tangent: (f64, f64),
    normal: (f64, f64),
    base_z: f64,
    offset_x: f64,
    offset_z: f64,
    slot: OpeningComponentSlot,
    look_role: OpeningLookRole,
    style: &OpeningStyle,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) {
    let Some(world_body) = crate::scene::model::solid_model::placed(
        &body,
        [tangent.0, tangent.1, 0.0],
        [normal.0, normal.1, 0.0],
        [0.0, 0.0, 1.0],
        [
            origin.0 + tangent.0 * offset_x,
            origin.1 + tangent.1 * offset_x,
            base_z + offset_z,
        ],
    ) else {
        return;
    };
    let solid = crate::modules::aec::engine::geometry::create_solid3d_from_body(&world_body);
    let mut entity = EntityType::Solid3D(solid);
    if !opening_owner_handle.is_null()
        && opening_owner_handle != scene.document.header.model_space_block_handle
    {
        entity.common_mut().owner_handle = opening_owner_handle;
    }
    if let Some(layer) = opening_layer {
        entity.as_entity_mut().set_layer(layer.to_string());
    }
    apply_opening_look_to_entity(&mut entity, style, library, rules, look_role, slot);
    if let Some(material_handle) = opening_native_material_handle(style, library, look_role, scene) {
        let common = entity.common_mut();
        common.material_flags = 3;
        common.material_handle = Some(material_handle);
    }
    let handle = scene.add_entity(entity);
    if let Some(display) = scene.prepare_solid_model_display(handle, &world_body) {
        scene.register_prepared_solid_model(handle, world_body, display);
    }
    write_opening_display_tag(scene, handle, opening_handle, slot);
    engine::owner_index::add_child(&mut scene.document, opening_handle, handle);
}

fn emit_opening_accessory_3d(
    scene: &mut Scene,
    body: Option<cadkernel::brep::Body>,
    opening_handle: Handle,
    opening_owner_handle: Handle,
    opening_layer: Option<&str>,
    origin: (f64, f64),
    tangent: (f64, f64),
    normal: (f64, f64),
    base_z: f64,
    offset_x: f64,
    offset_z: f64,
    role: OpeningLookRole,
    style: &OpeningStyle,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) {
    if let Some(body) = body {
        emit_opening_solid_3d(
            scene,
            body,
            opening_handle,
            opening_owner_handle,
            opening_layer,
            origin,
            tangent,
            normal,
            base_z,
            offset_x,
            offset_z,
            OpeningComponentSlot::Solid3D,
            role,
            style,
            library,
            rules,
        );
    }
}

pub(crate) fn sill_x_span(half_width: f64, left: f64, right: f64) -> (f64, f64) {
    (-half_width - left.max(0.0), half_width + right.max(0.0))
}

pub(crate) fn wall_clamped_interval(wall_min: f64, wall_max: f64, start: f64, depth: f64) -> (f64, f64) {
    let lo = wall_min.min(wall_max);
    let hi = wall_min.max(wall_max);
    let start = start.clamp(lo, hi);
    let end = (start + depth.max(0.0)).clamp(lo, hi);
    (start.min(end), start.max(end))
}

pub(crate) fn accessory_bottom(head: f64, seat: f64) -> f64 {
    head + seat.max(0.0)
}

pub(crate) fn lintel_depth_interval(wall_min: f64, wall_max: f64, depth: f64) -> (f64, f64) {
    let lo = wall_min.min(wall_max);
    let hi = wall_min.max(wall_max);
    let depth = depth.max(0.0).min((hi - lo).max(0.0));
    let center = (lo + hi) * 0.5;
    (center - depth * 0.5, center + depth * 0.5)
}

pub(crate) fn exterior_sill_y_bounds(
    wall_y_min: f64,
    wall_y_max: f64,
    overhang: f64,
    frame_depth: f64,
) -> (f64, f64) {
    let wall_min = wall_y_min.min(wall_y_max);
    let wall_max = wall_y_min.max(wall_y_max);
    let frame_outer = (-frame_depth.max(0.0) * 0.5).clamp(wall_min, wall_max);
    (wall_min - overhang.max(0.0), frame_outer)
}

pub(crate) fn interior_board_y_bounds(
    wall_y_min: f64,
    wall_y_max: f64,
    overhang: f64,
    frame_depth: f64,
) -> (f64, f64) {
    let wall_min = wall_y_min.min(wall_y_max);
    let wall_max = wall_y_min.max(wall_y_max);
    let frame_inner = (frame_depth.max(0.0) * 0.5).clamp(wall_min, wall_max);
    (frame_inner, wall_max + overhang.max(0.0))
}

/// Depth interval (min, max) of a sill/board upstand: it stands on the sill at
/// the outer face of the fixed frame and runs `away` (-1 exterior, +1
/// interior) from `frame_edge`, at most as deep as the frame.
pub(crate) fn sill_upstand_interval(
    frame_edge: f64,
    away: f64,
    section_depth: f64,
    frame_depth: f64,
) -> (f64, f64) {
    let depth = section_depth.min(frame_depth).max(0.0);
    let far = frame_edge + away * depth;
    (far.min(frame_edge), far.max(frame_edge))
}

pub fn build_opening_glazing_3d(
    width: f64,
    height: f64,
    frame_thickness: f64,
    shape: OpeningShape,
    spring_height: f64,
) -> Option<cadkernel::brep::Body> {
    let hw = width * 0.5;
    let ft = frame_thickness.max(0.02).min(hw * 0.45);
    let (_outer, inner) = opening_elevation_loops(shape, width, height, spring_height, ft, false);
    let half_t = 0.01;
    build_faceted_prism(&inner, -half_t, half_t)
}

fn local_to_world(
    lx: f64,
    ly: f64,
    origin: (f64, f64),
    tangent: (f64, f64),
    normal: (f64, f64),
) -> (f64, f64) {
    (
        origin.0 + tangent.0 * lx + normal.0 * ly,
        origin.1 + tangent.1 * lx + normal.1 * ly,
    )
}

fn transform_paths(
    paths: &[BakedPath],
    origin: (f64, f64),
    tangent: (f64, f64),
    normal: (f64, f64),
) -> Vec<BakedPath> {
    paths
        .iter()
        .map(|p| {
            if p.slot.is_elevation() {
                BakedPath {
                    slot: p.slot,
                    look_role: p.look_role,
                    closed: p.closed,
                    filled: p.filled,
                    points: p
                        .points
                        .iter()
                        .map(|&(u, v)| (origin.0 + tangent.0 * u, origin.1 + tangent.1 * u + v))
                        .collect(),
                    points_3d: p.points_3d.clone(),
                }
            } else {
                BakedPath {
                    slot: p.slot,
                    look_role: p.look_role,
                    closed: p.closed,
                    filled: p.filled,
                    points: p
                        .points
                        .iter()
                        .map(|&(x, y)| local_to_world(x, y, origin, tangent, normal))
                        .collect(),
                    points_3d: p.points_3d.clone(),
                }
            }
        })
        .collect()
}

/// World-space generator bake for an opening on `axis`.
pub fn bake_opening_world(
    axis: &[(f64, f64)],
    thickness: f64,
    opening: &Opening,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> Vec<BakedPath> {
    bake_opening_world_with_doc(axis, thickness, opening, library, rules, None)
}

pub fn bake_opening_world_with_doc(
    axis: &[(f64, f64)],
    thickness: f64,
    opening: &Opening,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
    doc: Option<&acadrust::CadDocument>,
) -> Vec<BakedPath> {
    let center_s = opening.center_along_axis();
    let Some(((cx, cy), (tx, ty))) = point_and_tangent_at_distance(axis, center_s)
    else {
        return Vec::new();
    };
    let normal = (-ty, tx);
    let origin = (
        cx + normal.0 * opening.cross_axis_offset,
        cy + normal.1 * opening.cross_axis_offset,
    );
    let (wall_y_min, wall_y_max) = doc
        .and_then(|d| d.get_entity(opening.host_wall))
        .and_then(wall_from_entity)
        .filter(|w| !w.layers.is_empty())
        .map(|w| {
            let mut y_min = f64::INFINITY;
            let mut y_max = f64::NEG_INFINITY;
            for l in &w.layers {
                y_min = y_min.min(l.axis_offset).min(l.axis_offset + l.thickness);
                y_max = y_max.max(l.axis_offset).max(l.axis_offset + l.thickness);
            }
            (y_min, y_max)
        })
        .unwrap_or((-thickness * 0.5, thickness * 0.5));
    let plan = rules.map(|r| &r.opening_profiles);
    let (slots, style) = resolved_slots_for_plan(opening, library, plan);
    let style = style.unwrap_or_else(|| default_style_for_kind(opening.kind));
    let grid = opening.division_override.as_ref().unwrap_or(&style.grid);
    let projected = project_division_sized(grid, opening.width, opening.height);
    let params = OpeningBakeParams::from_opening(opening, thickness, Some(&style))
        .with_wall_bounds(wall_y_min, wall_y_max);
    let merged = rules_with_plan_visibility(opening, library, rules);
    let local = bake_opening_representation(
        &slots,
        params,
        &style.parts,
        library,
        grid,
        representation_levels(
            &slots,
            &projected,
            merged
                .as_ref()
                .and_then(|rules| rules.opening_division_level),
        ),
        merged.as_ref(),
        doc,
    );
    transform_paths(&local, origin, (tx, ty), normal)
}

/// Rubber-band wires for live placement preview.
pub fn preview_opening_wires(
    axis: &[(f64, f64)],
    thickness: f64,
    opening: &Opening,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
    base_z: f64,
) -> Vec<WireModel> {
    let paths = bake_opening_world(axis, thickness, opening, library, rules);
    let mut wires = Vec::new();
    let z = base_z as f32;
    if let Some(cut) = engine::openings::opening_footprint_2d(axis, thickness, opening) {
        let mut pts: Vec<[f32; 3]> = cut
            .iter()
            .map(|&(x, y)| [x as f32, y as f32, z])
            .collect();
        if let Some(first) = pts.first().copied() {
            pts.push(first);
        }
        wires.push(WireModel::solid(
            "opening_cut".into(),
            pts,
            WireModel::CYAN,
            false,
        ));
    }
    for (i, path) in paths.iter().enumerate() {
        if path.points.len() < 2 {
            continue;
        }
        let mut pts: Vec<[f32; 3]> = path
            .points
            .iter()
            .map(|&(x, y)| [x as f32, y as f32, z])
            .collect();
        if path.closed {
            if let Some(first) = pts.first().copied() {
                pts.push(first);
            }
        }
        wires.push(WireModel::solid(
            format!("opening_slot_{i}"),
            pts,
            WireModel::CYAN,
            false,
        ));
    }
    wires
}

/// Solid-fill preview hatches (e.g. `DiagonalFill`).
pub fn preview_opening_hatches(
    axis: &[(f64, f64)],
    thickness: f64,
    opening: &Opening,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> Vec<HatchModel> {
    bake_opening_world(axis, thickness, opening, library, rules)
        .into_iter()
        .filter(|p| p.filled && p.closed && p.points.len() >= 3)
        .filter_map(|p| solid_hatch_from_ring(&p.points))
        .collect()
}

fn solid_hatch_from_ring(ring: &[(f64, f64)]) -> Option<HatchModel> {
    if ring.len() < 3 {
        return None;
    }
    let origin = [ring[0].0, ring[0].1];
    let boundary: Vec<[f32; 2]> = ring
        .iter()
        .map(|(x, y)| [(*x - origin[0]) as f32, (*y - origin[1]) as f32])
        .collect();
    Some(HatchModel {
        pattern_origin: None,
        render_instance: None,
        world_origin: origin,
        boundary: std::sync::Arc::new(boundary),
        boundary_wcs: None,
        fill_plane: None,
        fill_plane_boundary: None,
        boundary_exterior: None,
        boundary_sources: None,
        boundary_paths: None,
        style: acadrust::entities::HatchStyleType::Normal,
        pattern: HatchPattern::Solid,
        name: "AEC_OPENING_FILL".into(),
        color: [0.55, 0.55, 0.55, 0.35],
        aci: 0,
        line_weight_px: 1.0,
        angle_offset: 0.0,
        scale: 1.0,
        draw_depth: 0.0,
    })
}

pub(crate) fn write_opening_display_tag(
    scene: &mut Scene,
    handle: Handle,
    owner: Handle,
    slot: OpeningComponentSlot,
) {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String(OPENING_REP_TAG.to_string()));
    record.add_value(XDataValue::Handle(owner));
    record.add_value(XDataValue::String(slot.key().to_string()));
    write_aec_record(&mut scene.document, handle, record);
}

pub fn opening_rep_owner_from_entity(entity: &EntityType) -> Option<Handle> {
    for record in entity.common().extended_data.records() {
        if record.application_name != AEC_APPID {
            continue;
        }
        match record.values.first() {
            Some(XDataValue::String(s)) if s == OPENING_REP_TAG => {
                return record.values.get(1).and_then(engine::xdata::aec_value_as_handle);
            }
            _ => {}
        }
    }
    None
}

pub fn opening_rep_slot_from_entity(entity: &EntityType) -> Option<OpeningComponentSlot> {
    for record in entity.common().extended_data.records() {
        if record.application_name != AEC_APPID {
            continue;
        }
        match record.values.first() {
            Some(XDataValue::String(s)) if s == OPENING_REP_TAG => {
                let key = match record.values.get(2) {
                    Some(XDataValue::String(k)) => k.as_str(),
                    _ => return None,
                };
                return OpeningComponentSlot::from_key(key);
            }
            _ => {}
        }
    }
    None
}

pub fn collect_opening_display_children(scene: &Scene, owner: Handle) -> Vec<Handle> {
    let mut out = Vec::new();
    for h in engine::owner_index::children_of(&scene.document, owner) {
        let Some(entity) = scene.document.get_entity(h) else {
            continue;
        };
        if opening_from_entity(entity, h).is_some() {
            continue;
        }
        if opening_rep_owner_from_entity(entity) == Some(owner) && !out.contains(&h) {
            out.push(h);
        }
    }
    for entity in scene.document.entities() {
        let handle = entity.common().handle;
        if handle == owner {
            continue;
        }
        if opening_rep_owner_from_entity(entity) == Some(owner) && !out.contains(&handle) {
            out.push(handle);
        }
    }
    out
}

/// Resolve an `OPENING_REP` child (or the opening POINT) to the opening owner.
pub fn resolve_opening_package(scene: &Scene, clicked: Handle) -> Handle {
    let Some(entity) = scene.document.get_entity(clicked) else {
        return clicked;
    };
    if opening_from_entity(entity, clicked).is_some() {
        return clicked;
    }
    if let Some(owner) = opening_rep_owner_from_entity(entity) {
        if scene.document.get_entity(owner).is_some() {
            return owner;
        }
    }
    for entity in scene.document.entities() {
        let owner = entity.common().handle;
        if opening_from_entity(entity, owner).is_none() {
            continue;
        }
        if engine::owner_index::children_of(&scene.document, owner)
            .iter()
            .any(|h| *h == clicked)
        {
            return owner;
        }
    }
    clicked
}

pub fn opening_owner_if_any(scene: &Scene, handle: Handle) -> Option<Handle> {
    let owner = resolve_opening_package(scene, handle);
    scene
        .document
        .get_entity(owner)
        .and_then(|e| opening_from_entity(e, owner))
        .map(|_| owner)
}

pub fn opening_package_handles(scene: &Scene, opening_handle: Handle) -> Vec<Handle> {
    let mut handles = vec![opening_handle];
    for child in collect_opening_display_children(scene, opening_handle) {
        if !handles.contains(&child) {
            handles.push(child);
        }
    }
    handles
}

/// Grips along the host axis: center (0), start jamb (1), end jamb (2), hinge flip handle (3), swing flip handle (4).
pub fn opening_axis_grips(
    axis: &[(f64, f64)],
    opening: &Opening,
    base_z: f64,
) -> Vec<crate::scene::model::object::GripDef> {
    let center_s = opening.center_along_axis();
    let Some(((cx, cy), (tx, ty))) = point_and_tangent_at_distance(axis, center_s)
    else {
        return Vec::new();
    };
    let normal = (-ty, tx);
    let offset_cx = cx + normal.0 * opening.cross_axis_offset;
    let offset_cy = cy + normal.1 * opening.cross_axis_offset;

    let hw = opening.width * 0.5;
    let center = DVec3::new(offset_cx, offset_cy, base_z);
    let start = DVec3::new(offset_cx - tx * hw, offset_cy - ty * hw, base_z);
    let end = DVec3::new(offset_cx + tx * hw, offset_cy + ty * hw, base_z);

    let swing_sign = match opening.swing_side {
        SwingSide::Exterior => 1.0,
        SwingSide::Interior => -1.0,
    };
    let hinge_sign = match opening.hinge.bearing() {
        crate::modules::aec::engine::construction_grid::TrackEnd::Start => -1.0,
        crate::modules::aec::engine::construction_grid::TrackEnd::End => 1.0,
    };

    let hinge_handle = DVec3::new(
        offset_cx + tx * (hinge_sign * hw * 0.4) + normal.0 * (swing_sign * 0.25),
        offset_cy + ty * (hinge_sign * hw * 0.4) + normal.1 * (swing_sign * 0.25),
        base_z,
    );
    let swing_handle = DVec3::new(
        offset_cx - normal.0 * (swing_sign * 0.25),
        offset_cy - normal.1 * (swing_sign * 0.25),
        base_z,
    );

    vec![
        crate::entities::common::square_grip(0, center),
        crate::entities::common::rectangle_grip(1, start, [tx as f32, ty as f32]),
        crate::entities::common::rectangle_grip(2, end, [tx as f32, ty as f32]),
        crate::entities::common::square_grip(3, hinge_handle),
        crate::entities::common::square_grip(4, swing_handle),
    ]
}

/// Apply a center/width/flip grip. `grip_id` 0 moves the opening along the axis;
/// 1/2 stretch a jamb (width + recentre); 3 flips hinge (left/right); 4 flips swing side (interior/exterior).
pub fn apply_opening_axis_grip(
    axis: &[(f64, f64)],
    opening: &mut Opening,
    grip_id: usize,
    world: DVec3,
) {
    if grip_id == 3 {
        opening.flip_hinge();
        return;
    }
    if grip_id == 4 {
        opening.flip_swing();
        return;
    }
    let Some(s) = engine::openings::distance_along_axis_from_point(axis, (world.x, world.y)) else {
        return;
    };
    match grip_id {
        0 => match opening.reference_side {
            engine::openings::OpeningReferenceSide::Center => {
                opening.distance_along_axis = s.max(0.0);
            }
            engine::openings::OpeningReferenceSide::Start => {
                opening.distance_along_axis = (s - opening.width * 0.5).max(0.0);
            }
            engine::openings::OpeningReferenceSide::End => {
                opening.distance_along_axis = (s + opening.width * 0.5).max(0.0);
            }
        },
        1 => {
            let (_start, end) = opening.axis_span();
            let new_start = s;
            let lo = new_start.min(end);
            let hi = new_start.max(end);
            let width = (hi - lo).max(1e-4);
            opening.width = width;
            let center = (lo + hi) * 0.5;
            match opening.reference_side {
                engine::openings::OpeningReferenceSide::Center => {
                    opening.distance_along_axis = center;
                }
                engine::openings::OpeningReferenceSide::Start => {
                    opening.distance_along_axis = lo;
                }
                engine::openings::OpeningReferenceSide::End => {
                    opening.distance_along_axis = hi;
                }
            }
            let (w, h) = opening.shape.lock_size(opening.width, opening.height, true);
            opening.width = w;
            opening.height = h;
        }
        2 => {
            let (start, _end) = opening.axis_span();
            let new_end = s;
            let lo = start.min(new_end);
            let hi = start.max(new_end);
            let width = (hi - lo).max(1e-4);
            opening.width = width;
            let center = (lo + hi) * 0.5;
            match opening.reference_side {
                engine::openings::OpeningReferenceSide::Center => {
                    opening.distance_along_axis = center;
                }
                engine::openings::OpeningReferenceSide::Start => {
                    opening.distance_along_axis = lo;
                }
                engine::openings::OpeningReferenceSide::End => {
                    opening.distance_along_axis = hi;
                }
            }
            let (w, h) = opening.shape.lock_size(opening.width, opening.height, true);
            opening.width = w;
            opening.height = h;
        }
        _ => {}
    }
}

pub fn host_thickness(scene: &Scene, wall_handle: Handle) -> f64 {
    scene
        .document
        .get_entity(wall_handle)
        .and_then(wall_from_entity)
        .map(|w| w.total_thickness())
        .unwrap_or(0.0)
}

pub fn host_base_z(scene: &Scene, wall_handle: Handle) -> f64 {
    scene
        .document
        .get_entity(wall_handle)
        .and_then(wall_from_entity)
        .map(|w| w.base_origin[2])
        .unwrap_or(0.0)
}

/// Move the opening POINT onto the current axis location.
pub fn sync_opening_point_to_axis(
    scene: &mut Scene,
    opening: &Opening,
    axis: &[(f64, f64)],
    base_z: f64,
) {
    let Some(((x, y), (tx, ty))) = point_and_tangent_at_distance(axis, opening.distance_along_axis) else {
        return;
    };
    let normal = (-ty, tx);
    let px = x + normal.0 * opening.cross_axis_offset;
    let py = y + normal.1 * opening.cross_axis_offset;
    if let Some(EntityType::Point(pt)) = scene.document.get_entity_mut(opening.handle) {
        pt.location.x = px;
        pt.location.y = py;
        pt.location.z = base_z;
    }
    scene.bump_entities(&[(opening.handle, crate::scene::ChangeKind::Modified)]);
}

/// Recreate `OPENING_REP` children for one opening.
pub fn regenerate_opening_display(
    scene: &mut Scene,
    opening_handle: Handle,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) {
    let Some(entity) = scene.document.get_entity(opening_handle).cloned() else {
        return;
    };
    let Some(opening) = opening_from_entity(&entity, opening_handle) else {
        return;
    };
    let wall_handle = resolve_wall_package(scene, opening.host_wall);
    let wall_base_z = host_base_z(scene, wall_handle);
    let axis: Vec<(f64, f64)> = get_wall_vertices(scene, wall_handle)
        .iter()
        .map(|v| (v.x, v.y))
        .collect();
    if axis.len() >= 2 {
        sync_opening_point_to_axis(scene, &opening, &axis, wall_base_z);
    }
    let stale = collect_opening_display_children(scene, opening_handle);
    let retained_label_scale = stale.iter().find_map(|handle| {
        let entity = scene.document.get_entity(*handle)?;
        if !matches!(entity, EntityType::MText(_))
            || opening_rep_slot_from_entity(entity) != Some(OpeningComponentSlot::OpeningLabel2D)
        {
            return None;
        }
        engine::annotative_label::native_annotation_scale(&scene.document, *handle)
    });
    if !stale.is_empty() {
        scene.erase_entities(&stale);
        for h in &stale {
            engine::owner_index::remove_child(&mut scene.document, opening_handle, *h);
        }
    }
    let thickness = host_thickness(scene, wall_handle);
    let (wall_y_min, wall_y_max) = scene
        .document
        .get_entity(wall_handle)
        .and_then(wall_from_entity)
        .filter(|w| !w.layers.is_empty())
        .map(|w| {
            let mut y_min = f64::INFINITY;
            let mut y_max = f64::NEG_INFINITY;
            for l in &w.layers {
                y_min = y_min.min(l.axis_offset).min(l.axis_offset + l.thickness);
                y_max = y_max.max(l.axis_offset).max(l.axis_offset + l.thickness);
            }
            (y_min, y_max)
        })
        .unwrap_or((-thickness * 0.5, thickness * 0.5));
    let plan = rules.map(|r| &r.opening_profiles);
    let (slots, style) = resolved_slots_for_plan(&opening, library, plan);
    let style = style.unwrap_or_else(|| default_style_for_kind(opening.kind));
    let grid = opening.division_override.as_ref().unwrap_or(&style.grid);
    let projected = project_division_sized(grid, opening.width, opening.height);
    let params = OpeningBakeParams::from_opening(&opening, thickness, Some(&style))
        .with_wall_bounds(wall_y_min, wall_y_max);
    let merged_rules = rules_with_plan_visibility(&opening, library, rules);
    let levels = representation_levels(
        &slots,
        &projected,
        merged_rules
            .as_ref()
            .and_then(|rules| rules.opening_division_level),
    );
    let local = bake_opening_representation(
        &slots,
        params,
        &style.parts,
        library,
        &grid,
        levels,
        merged_rules.as_ref(),
        Some(&scene.document),
    );

    let center_s = opening.center_along_axis();
    let Some(((cx, cy), (tx, ty))) = point_and_tangent_at_distance(&axis, center_s) else {
        return;
    };
    let normal = (-ty, tx);
    let origin = (
        cx + normal.0 * opening.cross_axis_offset,
        cy + normal.1 * opening.cross_axis_offset,
    );

    let (opening_owner_handle, xref_layer_prefix) =
        crate::modules::aec::engine::wall_package::wall_owner_and_layer_prefix(scene, opening_handle);
    let default_opening_layer = match xref_layer_prefix.as_deref() {
        Some(p) => {
            let l = format!("{p}0");
            scene.ensure_layer(&l);
            Some(l)
        }
        None => None,
    };
    let layer_for = |scene: &mut Scene, role: Option<OpeningLookRole>| -> Option<String> {
        if role == Some(OpeningLookRole::Division) {
            Some(ensure_opening_grid_layer(
                scene,
                xref_layer_prefix.as_deref(),
            ))
        } else {
            default_opening_layer.clone()
        }
    };

    for baked in local {
        if baked.points.len() < 2 {
            continue;
        }
        if baked.slot.is_elevation() {
            let pts: Vec<acadrust::types::Vector3> = baked
                .points
                .iter()
                .map(|&(u, v)| {
                    acadrust::types::Vector3::new(
                        origin.0 + tx * u,
                        origin.1 + ty * u,
                        wall_base_z + opening.sill_height + v,
                    )
                })
                .collect();
            let mut pl = acadrust::entities::Polyline3D::from_points(pts);
            pl.flags.closed = baked.closed;
            let mut entity = EntityType::Polyline3D(pl);
            if !opening_owner_handle.is_null() && opening_owner_handle != scene.document.header.model_space_block_handle {
                entity.common_mut().owner_handle = opening_owner_handle;
            }
            if let Some(layer) = layer_for(scene, baked.resolved_look_role()) {
                entity.as_entity_mut().set_layer(layer);
            }
            if let Some(role) = baked.resolved_look_role() {
                apply_opening_look_to_entity(
                    &mut entity,
                    &style,
                    library,
                    merged_rules.as_ref(),
                    role,
                    baked.slot,
                );
            }
            let handle = scene.add_entity(entity);
            write_opening_display_tag(scene, handle, opening_handle, baked.slot);
            engine::owner_index::add_child(&mut scene.document, opening_handle, handle);
            continue;
        }

        let world_pts: Vec<(f64, f64)> = baked
            .points
            .iter()
            .map(|&(x, y)| local_to_world(x, y, origin, (tx, ty), normal))
            .collect();

        if baked.filled {
            if let Some(model) = solid_hatch_from_ring(&world_pts) {
                let hatch = scene.add_hatch(model, None, None);
                if !opening_owner_handle.is_null() && opening_owner_handle != scene.document.header.model_space_block_handle {
                    if let Some(e) = scene.document.get_entity_mut(hatch) {
                        e.common_mut().owner_handle = opening_owner_handle;
                    }
                }
                if let Some(layer) = layer_for(scene, baked.resolved_look_role()) {
                    if let Some(e) = scene.document.get_entity_mut(hatch) {
                        e.as_entity_mut().set_layer(layer);
                    }
                }
                if let Some(EntityType::Hatch(h)) = scene.document.get_entity_mut(hatch) {
                    h.elevation = wall_base_z;
                }
                write_opening_display_tag(scene, hatch, opening_handle, baked.slot);
                engine::owner_index::add_child(&mut scene.document, opening_handle, hatch);
            }
            continue;
        }
        let mut pl = LwPolyline::new();
        for &(x, y) in &world_pts {
            pl.add_vertex(LwVertex::new(Vector2::new(x, y)));
        }
        pl.is_closed = baked.closed;
        pl.elevation = wall_base_z;
        let mut entity = EntityType::LwPolyline(pl);
        if !opening_owner_handle.is_null() && opening_owner_handle != scene.document.header.model_space_block_handle {
            entity.common_mut().owner_handle = opening_owner_handle;
        }
        if let Some(layer) = layer_for(scene, baked.resolved_look_role()) {
            entity.as_entity_mut().set_layer(layer);
        }
        if let Some(role) = baked.resolved_look_role() {
            apply_opening_look_to_entity(
                &mut entity,
                &style,
                library,
                merged_rules.as_ref(),
                role,
                baked.slot,
            );
        }
        let handle = scene.add_entity(entity);
        write_opening_display_tag(scene, handle, opening_handle, baked.slot);
        engine::owner_index::add_child(&mut scene.document, opening_handle, handle);
    }

    let plan = rules.map(|r| &r.opening_profiles);
    let (slots, _) = resolved_slots_for_plan(&opening, library, plan);
    let merged_rules = rules_with_plan_visibility(&opening, library, rules);
    if merged_rules
        .as_ref()
        .map_or(true, |r| r.is_opening_visible(OpeningComponentSlot::OpeningLabel2D))
        && slots.contains_key(&OpeningComponentSlot::OpeningLabel2D)
    {
        if let Some(((cx, cy), (tx, ty))) =
            point_and_tangent_at_distance(&axis, opening.center_along_axis())
        {
            let normal = (-ty, tx);
            let lx = cx + normal.0 * opening.cross_axis_offset;
            let ly = cy + normal.1 * opening.cross_axis_offset;
            let label_text = format_din1356_label(&opening);
            let mut mtext = acadrust::entities::MText::new();
            mtext.value = label_text;
            mtext.insertion_point = acadrust::types::Vector3::new(lx, ly, wall_base_z);
            mtext.height = 0.15;
            let mut rot = ty.atan2(tx);
            if rot > std::f64::consts::FRAC_PI_2 {
                rot -= std::f64::consts::PI;
            } else if rot < -std::f64::consts::FRAC_PI_2 {
                rot += std::f64::consts::PI;
            }
            mtext.rotation = rot;
            mtext.attachment_point = acadrust::entities::mtext::AttachmentPoint::MiddleCenter;
            let mut entity = EntityType::MText(Box::new(mtext));
            if !opening_owner_handle.is_null() && opening_owner_handle != scene.document.header.model_space_block_handle {
                entity.common_mut().owner_handle = opening_owner_handle;
            }
            if let Some(ref l) = default_opening_layer {
                entity.as_entity_mut().set_layer(l.clone());
            }
            let handle = scene.add_entity(entity);
            let _ = engine::annotative_label::bind_annotative_mtext(
                &mut scene.document,
                handle,
                retained_label_scale,
            );
            write_opening_display_tag(
                scene,
                handle,
                opening_handle,
                OpeningComponentSlot::OpeningLabel2D,
            );
            engine::owner_index::add_child(&mut scene.document, opening_handle, handle);
        }
    }

    let frame_3d_vis = merged_rules
        .as_ref()
        .map_or(true, |r| r.is_opening_visible(OpeningComponentSlot::Frame3D))
        && (slots.contains_key(&OpeningComponentSlot::Frame3D)
            || slots.contains_key(&OpeningComponentSlot::Solid3D));
    let fixed_frame_section = resolve_component_section(
        library,
        style.parts.fixed_frame.profile_id.as_deref(),
        style.parts.fixed_frame.face_width,
        style.parts.fixed_frame.wall_depth,
    );
    let leaf_frame_section = resolve_component_section(
        library,
        style.parts.leaf_frame.profile_id.as_deref(),
        style.parts.leaf_frame.face_width,
        style.parts.leaf_frame.depth,
    );
    let division_profile = grid
        .muntin_profile_id
        .as_deref()
        .and_then(|id| library?.find_section_profile(id))
        .filter(|profile| profile.is_valid());
    let division_section = resolve_component_section(
        library,
        grid.muntin_profile_id.as_deref(),
        style.parts.fixed_frame.face_width,
        style.parts.fixed_frame.wall_depth,
    );
    let exterior_sill_section = resolve_component_section(
        library,
        style.parts.exterior_sill.profile_id.as_deref(),
        style.parts.exterior_sill.thickness,
        style.parts.exterior_sill.thickness,
    );
    let interior_board_section = resolve_component_section(
        library,
        style.parts.interior_board.profile_id.as_deref(),
        style.parts.interior_board.thickness,
        style.parts.interior_board.thickness,
    );
    let threshold_section = resolve_component_section(
        library,
        style.parts.threshold.profile_id.as_deref(),
        style.parts.threshold.height,
        style.parts.threshold.width,
    );
    let shutter_section = resolve_component_section(
        library,
        style.parts.shutter_box.profile_id.as_deref(),
        style.parts.shutter_box.height,
        style.parts.shutter_box.depth,
    );
    let lintel_section = resolve_component_section(
        library,
        style.parts.lintel.profile_id.as_deref(),
        style.parts.lintel.height,
        wall_y_max - wall_y_min,
    );
    let division_post_width = division_profile.map_or(fixed_frame_section.face, |_| division_section.face);
    let division_depth = division_profile.map_or(fixed_frame_section.depth, |_| division_section.depth);

    if frame_3d_vis && opening.kind != OpeningKind::Breakthrough {
        if opening_component_should_render_with_library(
            &style.parts,
            opening.kind,
            OpeningLookRole::FixedFrame,
            library,
        ) {
            if let Some(body) = build_opening_frame_3d_with_threshold(
                opening.width,
                opening.height,
                fixed_frame_section.face,
                fixed_frame_section.depth,
                params.shape,
                params.spring_height,
                !omits_threshold(&resolve_grid(grid, opening.width, opening.height), opening.width)
                    && opening.kind != OpeningKind::Door,
            ) {
                emit_opening_solid_3d(
                    scene,
                    body,
                    opening_handle,
                    opening_owner_handle,
                    default_opening_layer.as_deref(),
                    origin,
                    (tx, ty),
                    normal,
                    wall_base_z + opening.sill_height,
                    0.0,
                    0.0,
                    OpeningComponentSlot::Frame3D,
                    OpeningLookRole::FixedFrame,
                    &style,
                    library,
                    merged_rules.as_ref(),
                );
            }
        }
        if levels.divisions != DivisionLevel::Off
            && opening_component_should_render_with_library(
                &style.parts,
                opening.kind,
                OpeningLookRole::Division,
                library,
            )
        {
            for (body, center_x, center_z) in build_opening_grid_bars_3d(
                grid,
                params,
                division_post_width,
                division_depth,
                library,
                &leaf_frame_section,
            ) {
                let grid_layer = layer_for(scene, Some(OpeningLookRole::Division));
                emit_opening_solid_3d(
                    scene,
                    body,
                    opening_handle,
                    opening_owner_handle,
                    grid_layer.as_deref(),
                    origin,
                    (tx, ty),
                    normal,
                    wall_base_z + opening.sill_height,
                    center_x,
                    center_z,
                    OpeningComponentSlot::Frame3D,
                    OpeningLookRole::Division,
                    &style,
                    library,
                    merged_rules.as_ref(),
                );
            }
        }
    }

    let leaf_3d_vis = merged_rules
        .as_ref()
        .map_or(true, |r| r.is_opening_visible(OpeningComponentSlot::Leaf3D))
        && (slots.contains_key(&OpeningComponentSlot::Leaf3D)
            || slots.contains_key(&OpeningComponentSlot::Solid3D));
    let leaf_component_visible = leaf_3d_vis
        && opening_component_should_render_with_library(
            &style.parts,
            opening.kind,
            OpeningLookRole::Leaf,
            library,
        );
    let glazing_3d_vis = merged_rules
        .as_ref()
        .map_or(true, |r| r.is_opening_visible(OpeningComponentSlot::Glazing3D))
        && (slots.contains_key(&OpeningComponentSlot::Glazing3D)
            || slots.contains_key(&OpeningComponentSlot::Solid3D));
    let leaf_glazing_visible = glazing_3d_vis
        && opening_component_should_render_with_library(
            &style.parts,
            opening.kind,
            OpeningLookRole::Glazing,
            library,
        );

    if leaf_component_visible {
        {
            let divisions_visible = levels.divisions != DivisionLevel::Off
                && opening_component_should_render_with_library(
                    &style.parts,
                    opening.kind,
                    OpeningLookRole::Division,
                    library,
                );
            let overlap = (style.parts.leaf_frame.rebate_overlap.max(0.0)
                + style.parts.leaf_frame.lateral_overlap.max(0.0))
                .max(0.0);
            let cells = leaf_cells_with_bar_offsets(
                grid,
                opening.width,
                opening.height,
                fixed_frame_section.face,
                divisions_visible.then_some(division_post_width),
                leaf_frame_section.face,
                overlap,
                frame_3d_vis
                    && opening_component_should_render_with_library(
                        &style.parts,
                        opening.kind,
                        OpeningLookRole::FixedFrame,
                        library,
                    ),
                opening.kind == OpeningKind::Door,
            );
            for cell in cells {
                if !matches!(
                    cell.cell.cell.fill,
                    crate::modules::aec::engine::construction_grid::CellFill::Door
                        | crate::modules::aec::engine::construction_grid::CellFill::Glass
                ) {
                    continue;
                }
                let top = cell.top.min(arch_height_at_x(params, cell.left)).min(arch_height_at_x(params, cell.right));
                let height = top - cell.bottom;
                let width = cell.right - cell.left;
                if width <= 0.0 || height <= 0.0 {
                    continue;
                }
                let is_door = matches!(
                    cell.cell.cell.fill,
                    crate::modules::aec::engine::construction_grid::CellFill::Door
                );
                let center_x = (cell.left + cell.right) * 0.5;
                let operation = cell.cell.cell.operation.unwrap_or(opening.hinge);
                let pivot_x = match operation.bearing() {
                    TrackEnd::Start => cell.pivot_start - center_x,
                    TrackEnd::End => cell.pivot_end - center_x,
                };
                let swing = cell.cell.cell.swing.unwrap_or(params.swing_side);
                let angle = cell
                    .cell
                    .cell
                    .angle_deg
                    .unwrap_or(style.opening_angle_deg);
                if let Some((body, glass)) = build_opening_leaf_components_3d(
                    width,
                    height,
                    leaf_frame_section.face,
                    leaf_frame_section.depth,
                    style.parts.leaf_frame.depth_offset,
                    pivot_x,
                    angle,
                    operation,
                    swing,
                    is_door,
                    OpeningShape::Rectangle,
                    0.0,
                    (!is_door && leaf_glazing_visible)
                        .then_some(style.parts.glazing.thickness),
                ) {
                    emit_opening_solid_3d(
                        scene,
                        body,
                        opening_handle,
                        opening_owner_handle,
                        default_opening_layer.as_deref(),
                        origin,
                        (tx, ty),
                        normal,
                        wall_base_z + opening.sill_height,
                        center_x,
                        cell.bottom,
                        OpeningComponentSlot::Leaf3D,
                        OpeningLookRole::Leaf,
                        &style,
                        library,
                        merged_rules.as_ref(),
                    );
                    if let Some(glass) = glass {
                        emit_opening_solid_3d(
                            scene,
                            glass,
                            opening_handle,
                            opening_owner_handle,
                            default_opening_layer.as_deref(),
                            origin,
                            (tx, ty),
                            normal,
                            wall_base_z + opening.sill_height,
                            center_x,
                            cell.bottom,
                            OpeningComponentSlot::Glazing3D,
                            OpeningLookRole::Glazing,
                            &style,
                            library,
                            merged_rules.as_ref(),
                        );
                    }
                }
            }
        }
    }

    if glazing_3d_vis
        && opening_component_should_render_with_library(
            &style.parts,
            opening.kind,
            OpeningLookRole::Glazing,
            library,
        )
    {
        {
            for cell in flatten_resolved_cells(grid, opening.width, opening.height) {
                let is_leaf_glass = matches!(
                    cell.cell.fill,
                    crate::modules::aec::engine::construction_grid::CellFill::Glass
                );
                if (!is_leaf_glass
                    && !matches!(
                        cell.cell.fill,
                        crate::modules::aec::engine::construction_grid::CellFill::Fixed
                    ))
                    || (is_leaf_glass && leaf_component_visible)
                {
                    continue;
                }
                let inset = style.parts.glazing.inset.max(0.0);
                let leaf_here = is_leaf_glass && leaf_component_visible;
                let bar = if leaf_here {
                    leaf_frame_section.face
                } else {
                    fixed_frame_section.face
                };
                let overlap = if leaf_here {
                    (style.parts.leaf_frame.rebate_overlap + style.parts.leaf_frame.lateral_overlap)
                        .max(0.0)
                } else {
                    0.0
                };
                let surround = (bar - overlap).max(0.0);
                let left = glass_edge_into_bar(cell.left, 1.0, surround, inset);
                let right = glass_edge_into_bar(cell.right, -1.0, surround, inset);
                let bottom_bar = if matches!(
                    cell.cell.fill,
                    crate::modules::aec::engine::construction_grid::CellFill::Door
                ) {
                    0.0
                } else {
                    surround
                };
                let bottom = glass_edge_into_bar(cell.bottom, 1.0, bottom_bar, inset);
                let top = glass_edge_into_bar(
                    cell.top
                        .min(arch_height_at_x(params, left))
                        .min(arch_height_at_x(params, right)),
                    -1.0,
                    surround,
                    inset,
                );
                let width = right - left;
                let height = top - bottom;
                if width <= 0.0 || height <= 0.0 {
                    continue;
                }
                if let Some(body) = build_opening_box_3d(
                    width,
                    height,
                    style.parts.glazing.thickness,
                ) {
                    emit_opening_solid_3d(
                        scene,
                        body,
                        opening_handle,
                        opening_owner_handle,
                        default_opening_layer.as_deref(),
                        origin,
                        (tx, ty),
                        normal,
                        wall_base_z + opening.sill_height,
                        (left + right) * 0.5,
                        (bottom + top) * 0.5,
                        OpeningComponentSlot::Glazing3D,
                        OpeningLookRole::Glazing,
                        &style,
                        library,
                        merged_rules.as_ref(),
                    );
                }
            }
        }
    }

    let accessories_3d_vis = merged_rules
        .as_ref()
        .map_or(true, |r| r.is_opening_visible(OpeningComponentSlot::Solid3D));
    if accessories_3d_vis {
        let sill = &style.parts.exterior_sill;
        if opening_component_should_render_with_library(
            &style.parts,
            opening.kind,
            OpeningLookRole::ExteriorSill,
            library,
        )
            && wall_y_max > wall_y_min
        {
            let overhang = sill.overhang.max(0.0);
            let upstand = sill.upstand.max(0.0);
            let left = sill.side_overhang_left.max(0.0);
            let right = sill.side_overhang_right.max(0.0);
            let width = opening.width + left + right;
            let x_offset = (right - left) * 0.5;
            let frame_outer = (-fixed_frame_section.depth * 0.5)
                .clamp(wall_y_min.min(wall_y_max), wall_y_min.max(wall_y_max));
            let has_profile = sill
                .profile_id
                .as_deref()
                .and_then(|id| library?.find_section_profile(id))
                .is_some_and(|profile| profile.is_valid());
            if has_profile {
                let profile_outer = frame_outer - exterior_sill_section.depth;
                let requested_outer = wall_y_min - overhang;
                let y_min = requested_outer.min(profile_outer);
                emit_opening_accessory_3d(
                    scene,
                    build_opening_profile_prism_3d(
                        width,
                        &exterior_sill_section,
                        profile_outer,
                        1.0,
                    ),
                    opening_handle,
                    opening_owner_handle,
                    default_opening_layer.as_deref(),
                    origin,
                    (tx, ty),
                    normal,
                    wall_base_z + opening.sill_height,
                    x_offset,
                    0.0,
                    OpeningLookRole::ExteriorSill,
                    &style,
                    library,
                    merged_rules.as_ref(),
                );
                emit_opening_accessory_3d(
                    scene,
                    build_opening_box_3d_with_bounds(
                        width,
                        exterior_sill_section.face,
                        y_min,
                        profile_outer,
                    ),
                    opening_handle,
                    opening_owner_handle,
                    default_opening_layer.as_deref(),
                    origin,
                    (tx, ty),
                    normal,
                    wall_base_z + opening.sill_height,
                    x_offset,
                    exterior_sill_section.face * 0.5,
                    OpeningLookRole::ExteriorSill,
                    &style,
                    library,
                    merged_rules.as_ref(),
                );
            } else {
                let (y_min, y_max) = exterior_sill_y_bounds(
                    wall_y_min,
                    wall_y_max,
                    overhang,
                    fixed_frame_section.depth,
                );
                emit_opening_accessory_3d(
                    scene,
                    build_opening_box_3d_with_bounds(
                        width,
                        exterior_sill_section.face,
                        y_min,
                        y_max,
                    ),
                    opening_handle,
                    opening_owner_handle,
                    default_opening_layer.as_deref(),
                    origin,
                    (tx, ty),
                    normal,
                    wall_base_z + opening.sill_height,
                    x_offset,
                    exterior_sill_section.face * 0.5,
                    OpeningLookRole::ExteriorSill,
                    &style,
                    library,
                    merged_rules.as_ref(),
                );
                if upstand > 0.0 {
                    let (up_min, up_max) = sill_upstand_interval(
                        frame_outer,
                        -1.0,
                        exterior_sill_section.depth,
                        fixed_frame_section.depth,
                    );
                    emit_opening_accessory_3d(
                        scene,
                        build_opening_box_3d_with_bounds(width, upstand, up_min, up_max),
                        opening_handle,
                        opening_owner_handle,
                        default_opening_layer.as_deref(),
                        origin,
                        (tx, ty),
                        normal,
                        wall_base_z + opening.sill_height,
                        x_offset,
                        exterior_sill_section.face + upstand * 0.5,
                        OpeningLookRole::ExteriorSill,
                        &style,
                        library,
                        merged_rules.as_ref(),
                    );
                }
            }
        }

        let board = &style.parts.interior_board;
        if opening_component_should_render_with_library(
            &style.parts,
            opening.kind,
            OpeningLookRole::InteriorBoard,
            library,
        )
            && wall_y_max > wall_y_min
        {
            let overhang = board.overhang.max(0.0);
            let upstand = board.upstand.max(0.0);
            let left = board.side_overhang_left.max(0.0);
            let right = board.side_overhang_right.max(0.0);
            let width = opening.width + left + right;
            let x_offset = (right - left) * 0.5;
            let frame_inner = (fixed_frame_section.depth * 0.5)
                .clamp(wall_y_min.min(wall_y_max), wall_y_min.max(wall_y_max));
            let has_profile = board
                .profile_id
                .as_deref()
                .and_then(|id| library?.find_section_profile(id))
                .is_some_and(|profile| profile.is_valid());
            if has_profile {
                let profile_inner = frame_inner + interior_board_section.depth;
                let requested_inner = wall_y_max + overhang;
                let y_max = requested_inner.max(profile_inner);
                emit_opening_accessory_3d(
                    scene,
                    build_opening_profile_prism_3d(
                        width,
                        &interior_board_section,
                        frame_inner,
                        1.0,
                    ),
                    opening_handle,
                    opening_owner_handle,
                    default_opening_layer.as_deref(),
                    origin,
                    (tx, ty),
                    normal,
                    wall_base_z + opening.sill_height,
                    x_offset,
                    0.0,
                    OpeningLookRole::InteriorBoard,
                    &style,
                    library,
                    merged_rules.as_ref(),
                );
                emit_opening_accessory_3d(
                    scene,
                    build_opening_box_3d_with_bounds(
                        width,
                        interior_board_section.face,
                        profile_inner,
                        y_max,
                    ),
                    opening_handle,
                    opening_owner_handle,
                    default_opening_layer.as_deref(),
                    origin,
                    (tx, ty),
                    normal,
                    wall_base_z + opening.sill_height,
                    x_offset,
                    interior_board_section.face * 0.5,
                    OpeningLookRole::InteriorBoard,
                    &style,
                    library,
                    merged_rules.as_ref(),
                );
            } else {
                let (y_min, y_max) = interior_board_y_bounds(
                    wall_y_min,
                    wall_y_max,
                    overhang,
                    fixed_frame_section.depth,
                );
                emit_opening_accessory_3d(
                    scene,
                    build_opening_box_3d_with_bounds(
                        width,
                        interior_board_section.face,
                        y_min,
                        y_max,
                    ),
                    opening_handle,
                    opening_owner_handle,
                    default_opening_layer.as_deref(),
                    origin,
                    (tx, ty),
                    normal,
                    wall_base_z + opening.sill_height,
                    x_offset,
                    interior_board_section.face * 0.5,
                    OpeningLookRole::InteriorBoard,
                    &style,
                    library,
                    merged_rules.as_ref(),
                );
                if upstand > 0.0 {
                    let (up_min, up_max) = sill_upstand_interval(
                        frame_inner,
                        1.0,
                        interior_board_section.depth,
                        fixed_frame_section.depth,
                    );
                    emit_opening_accessory_3d(
                        scene,
                        build_opening_box_3d_with_bounds(width, upstand, up_min, up_max),
                        opening_handle,
                        opening_owner_handle,
                        default_opening_layer.as_deref(),
                        origin,
                        (tx, ty),
                        normal,
                        wall_base_z + opening.sill_height,
                        x_offset,
                        interior_board_section.face + upstand * 0.5,
                        OpeningLookRole::InteriorBoard,
                        &style,
                        library,
                        merged_rules.as_ref(),
                    );
                }
            }
        }

        if opening_component_should_render_with_library(
            &style.parts,
            opening.kind,
            OpeningLookRole::Threshold,
            library,
        ) {
            emit_opening_accessory_3d(
                scene,
                build_opening_box_3d_with_bounds(
                    opening.width,
                    threshold_section.face,
                    -threshold_section.depth * 0.5,
                    threshold_section.depth * 0.5,
                ),
                opening_handle,
                opening_owner_handle,
                default_opening_layer.as_deref(),
                origin,
                (tx, ty),
                normal,
                wall_base_z + opening.sill_height,
                0.0,
                threshold_section.face * 0.5,
                OpeningLookRole::Threshold,
                &style,
                library,
                merged_rules.as_ref(),
            );
        }

        let shutter = &style.parts.shutter_box;
        if opening_component_should_render_with_library(
            &style.parts,
            opening.kind,
            OpeningLookRole::ShutterBox,
            library,
        ) {
            let (y_min, y_max) = wall_clamped_interval(
                wall_y_min,
                wall_y_max,
                wall_y_min + shutter.installation_depth.max(0.0),
                shutter_section.depth,
            );
            emit_opening_accessory_3d(
                scene,
                build_opening_box_3d_with_bounds(
                    opening.width,
                    shutter_section.face,
                    y_min,
                    y_max,
                ),
                opening_handle,
                opening_owner_handle,
                default_opening_layer.as_deref(),
                origin,
                (tx, ty),
                normal,
                wall_base_z + opening.sill_height,
                0.0,
                accessory_bottom(opening.height, shutter.seat_above_frame) + shutter_section.face * 0.5,
                OpeningLookRole::ShutterBox,
                &style,
                library,
                merged_rules.as_ref(),
            );
        }

        let lintel = &style.parts.lintel;
        if opening_component_should_render_with_library(
            &style.parts,
            opening.kind,
            OpeningLookRole::Lintel,
            library,
        ) {
            let bearing_left = lintel.bearing_left.max(0.0);
            let bearing_right = lintel.bearing_right.max(0.0);
            let (y_min, y_max) = lintel_depth_interval(wall_y_min, wall_y_max, lintel_section.depth);
            emit_opening_accessory_3d(
                scene,
                build_opening_box_3d_with_bounds(
                    opening.width + bearing_left + bearing_right,
                    lintel_section.face,
                    y_min,
                    y_max,
                ),
                opening_handle,
                opening_owner_handle,
                default_opening_layer.as_deref(),
                origin,
                (tx, ty),
                normal,
                wall_base_z + opening.sill_height,
                (bearing_right - bearing_left) * 0.5,
                opening.height + lintel_section.face * 0.5,
                OpeningLookRole::Lintel,
                &style,
                library,
                merged_rules.as_ref(),
            );
        }
    }
}

/// Regen every opening hosted by `wall_handle`.
pub fn regenerate_openings_for_wall(
    scene: &mut Scene,
    wall_handle: Handle,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let openings = openings_for_host_wall(scene, wall_handle);
    for opening in openings {
        regenerate_opening_display(scene, opening.handle, library, rules);
    }
}

/// Write instance XDATA, snap the POINT, and regen host cut + symbols.
pub fn commit_opening_instance(
    scene: &mut Scene,
    opening: &Opening,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> Vec<Handle> {
    write_opening_instance(scene, opening);
    let wall_handle = resolve_wall_package(scene, opening.host_wall);
    let mut touched = match engine::wall_regen::regenerate_wall_representation_with_rules_and_substitutions(
        scene,
        wall_handle,
        rules,
        None,
        library,
    ) {
        Ok(t) => t,
        Err(_) => {
            regenerate_opening_display(scene, opening.handle, library, rules);
            vec![wall_handle, opening.handle]
        }
    };
    touched.push(opening.handle);
    touched.extend(collect_opening_display_children(scene, opening.handle));
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    touched
}

/// Project a dragged opening POINT back onto the host axis and rebake.
pub fn sync_opening_from_point_location(
    scene: &mut Scene,
    opening_handle: Handle,
    library: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> bool {
    let Some(entity) = scene.document.get_entity(opening_handle).cloned() else {
        return false;
    };
    let Some(mut opening) = opening_from_entity(&entity, opening_handle) else {
        return false;
    };
    let EntityType::Point(pt) = &entity else {
        return false;
    };
    let wall_handle = resolve_wall_package(scene, opening.host_wall);
    let axis: Vec<(f64, f64)> = get_wall_vertices(scene, wall_handle)
        .iter()
        .map(|v| (v.x, v.y))
        .collect();
    let Some(s) = engine::openings::distance_along_axis_from_point(&axis, (pt.location.x, pt.location.y))
    else {
        return false;
    };
    opening.distance_along_axis = s.max(0.0);
    let _ = commit_opening_instance(scene, &opening, library, rules);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::LwVertex;
    use acadrust::types::Vector2;
    use crate::modules::aec::engine::library::seed_default_library;
    use crate::modules::aec::engine::opening_style::{
        DivisionGrid, DivisionLevel, ElevationFrameLevel, OpeningCell,
        OpeningParts, OpeningStyle, PlanFrameLevel, SwingGlyph, DEFAULT_FRAME_THICKNESS,
        SEED_WINDOW_STYLE_ID,
    };
    use crate::modules::aec::engine::opening_xdata::place_wall_opening;
    use crate::modules::aec::engine::openings::{
        Opening, OpeningKind, DEFAULT_WINDOW_WIDTH, SwingSide,
    };
    use crate::modules::aec::engine::wall::{Wall, WallJustification, WallLayer};
    use crate::modules::aec::engine::wall_regen::regenerate_wall_representation;
    use crate::modules::aec::engine::xdata::wall_record_for_wall;
    use crate::scene::Scene;

    fn params(width: f64, kind: OpeningKind, hinge: LeafOperation) -> OpeningBakeParams {
        OpeningBakeParams {
            width,
            height: 1.2,
            sill_height: 0.9,
            thickness: 0.3,
            frame_thickness: DEFAULT_FRAME_THICKNESS,
            cross_axis_offset: 0.0,
            wall_y_min: -0.15,
            wall_y_max: 0.15,
            hinge,
            swing_side: SwingSide::Exterior,
            shape: OpeningShape::Rectangle,
            spring_height: 0.0,
            opening_angle_deg: 90.0,
            kind,
        }
    }

    fn inner_frame_half_width(paths: &[BakedPath]) -> Option<f64> {
        let frames: Vec<_> = paths
            .iter()
            .filter(|p| p.slot == OpeningComponentSlot::Frame2D && p.closed)
            .collect();
        if frames.is_empty() {
            return None;
        }
        if frames.len() == 1 {
            let max_x = frames[0]
                .points
                .iter()
                .map(|p| p.0)
                .fold(f64::NEG_INFINITY, f64::max);
            return Some(max_x);
        }
        let has_two_separated_posts = frames.len() == 2 && {
            let p0_center_x = frames[0].points.iter().map(|p| p.0).sum::<f64>()
                / (frames[0].points.len() as f64);
            let p1_center_x = frames[1].points.iter().map(|p| p.0).sum::<f64>()
                / (frames[1].points.len() as f64);
            (p0_center_x * p1_center_x) < 0.0
        };
        if has_two_separated_posts {
            let right = frames.iter().max_by(|a, b| {
                let ax = a.points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
                let bx = b.points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
                ax.partial_cmp(&bx).unwrap()
            })?;
            let min_x = right
                .points
                .iter()
                .map(|p| p.0)
                .fold(f64::INFINITY, f64::min);
            Some(min_x)
        } else {
            let inner = frames.iter().min_by(|a, b| {
                let wa = a.points.iter().map(|p| p.0).fold(f64::NAN, f64::max)
                    - a.points.iter().map(|p| p.0).fold(f64::NAN, f64::min);
                let wb = b.points.iter().map(|p| p.0).fold(f64::NAN, f64::max)
                    - b.points.iter().map(|p| p.0).fold(f64::NAN, f64::min);
                wa.partial_cmp(&wb).unwrap()
            })?;
            let max_x = inner
                .points
                .iter()
                .map(|p| p.0)
                .fold(f64::NEG_INFINITY, f64::max);
            Some(max_x)
        }
    }

    #[test]
    fn leaf_cell_offsets_reach_all_rendered_frame_sides_and_keep_band_pivots() {
        let cells = leaf_cells_with_bar_offsets(
            &ConstructionGrid::default(),
            1.2,
            1.5,
            0.08,
            None,
            0.05,
            0.03,
            true,
            false,
        );
        assert_eq!(cells.len(), 1);
        let cell = &cells[0];
        assert!((cell.left - -0.55).abs() < 1e-9);
        assert!((cell.right - 0.55).abs() < 1e-9);
        assert!((cell.bottom - 0.05).abs() < 1e-9);
        assert!((cell.top - 1.45).abs() < 1e-9);
        assert!((cell.pivot_start - -0.52).abs() < 1e-9);
        assert!((cell.pivot_end - 0.52).abs() < 1e-9);

        let door = leaf_cells_with_bar_offsets(
            &ConstructionGrid::default(),
            1.2,
            1.5,
            0.08,
            None,
            0.05,
            0.03,
            true,
            true,
        );
        assert_eq!(door.len(), 1);
        assert!(door[0].bottom.abs() < 1e-9);
    }

    #[test]
    fn leaf_glazing_rotates_about_the_same_band_side_in_the_leaf_depth() {
        let depth_offset = 0.07;
        let pivot_x = -0.31;
        let (leaf, glass) = build_opening_leaf_components_3d(
            0.8,
            1.0,
            0.06,
            0.05,
            depth_offset,
            pivot_x,
            90.0,
            LeafOperation::left(),
            SwingSide::Exterior,
            false,
            OpeningShape::Rectangle,
            0.0,
            Some(0.02),
        )
        .expect("leaf and glass bodies");
        let glass = glass.expect("leaf glazing");
        let leaf_bounds = crate::scene::model::solid_model::extent(&leaf).expect("leaf extent");
        let glass_bounds = crate::scene::model::solid_model::extent(&glass).expect("glass extent");
        let center = |bounds: ([f64; 3], [f64; 3])| {
            std::array::from_fn::<_, 3, _>(|axis| (bounds.0[axis] + bounds.1[axis]) * 0.5)
        };
        let leaf_center = center(leaf_bounds);
        let glass_center = center(glass_bounds);
        for axis in 0..3 {
            assert!((leaf_center[axis] - glass_center[axis]).abs() < 1e-5);
        }
        assert!((leaf_center[1] - depth_offset).abs() > 0.1);
        let radius = ((leaf_center[0] - pivot_x).powi(2)
            + (leaf_center[1] - depth_offset).powi(2))
        .sqrt();
        assert!((radius - 0.31).abs() < 1e-5);
    }

    #[test]
    fn unstyled_opening_resolves_to_standard_style_parts() {
        let opening = Opening::window(
            acadrust::Handle::new(1),
            acadrust::Handle::new(2),
            1.2,
        );
        let (slots, style) = resolved_slots(&opening, None);
        let style = style.expect("standard fallback style");
        assert_eq!(style.style.id, SEED_WINDOW_STYLE_ID);
        assert_eq!(style.parts.fixed_frame.face_width, DEFAULT_FRAME_THICKNESS);
        assert!(slots.contains_key(&OpeningComponentSlot::Frame2D));
    }

    #[test]
    fn opening_style_swing_is_the_fallback_for_cell_overrides() {
        let opening = Opening::window(
            acadrust::Handle::new(1),
            acadrust::Handle::new(2),
            1.2,
        );
        let mut style = OpeningStyle::standard_window();
        style.swing_side = SwingSide::Interior;
        let params = OpeningBakeParams::from_opening(&opening, 0.3, Some(&style));
        assert_eq!(params.swing_side, SwingSide::Interior);
    }

    #[test]
    fn plan_and_elevation_posts_share_the_same_grid_coordinates() {
        let grid = DivisionGrid {
            vertical: vec![1.0 / 3.0, 2.0 / 3.0],
            cells: vec![OpeningCell::default(), OpeningCell::default()],
            ..DivisionGrid::default()
        };
        let params = params(1.2, OpeningKind::Window, LeafOperation::left());
        let parts = OpeningParts::default();
        let plan = bake_opening_plan_paths(
            params,
            &parts,
            None,
            &grid,
            PlanFrameLevel::Off,
            SwingGlyph::Off,
            DivisionLevel::Posts,
        );
        let elevation = bake_opening_elevation_paths(
            params,
            &parts,
            None,
            &grid,
            ElevationFrameLevel::Off,
            DivisionLevel::Posts,
        );
        let plan_post = plan
            .iter()
            .find(|path| {
                path.slot == OpeningComponentSlot::Division2D
                    && path.points[0].0 == path.points[1].0
            })
            .unwrap();
        let elevation_post = elevation
            .iter()
            .find(|path| {
                path.slot == OpeningComponentSlot::ElevationMuntins2D
                    && path.points[0].0 == path.points[1].0
            })
            .unwrap();
        let expected = -params.width * 0.5 + params.width / 3.0;
        assert!((plan_post.points[0].0 - expected).abs() < 1e-12);
        assert!((elevation_post.points[0].0 - expected).abs() < 1e-12);
    }

    #[test]
    fn grid_bars_3d_are_built_at_primary_and_muntin_positions() {
        let grid = DivisionGrid {
            vertical: vec![0.4, 0.6],
            vertical_muntins: vec![0.5],
            cells: vec![OpeningCell::default(), OpeningCell::default()],
            ..DivisionGrid::default()
        };
        let bars = build_opening_grid_bars_3d(
            &construction_from_division(&grid, 2.0, 1.2),
            params(2.0, OpeningKind::Window, LeafOperation::left()),
            0.06,
            0.06,
            None,
            &resolve_section(None, 0.04, 0.06),
        );
        assert_eq!(bars.len(), 3);

        let stulp = OpeningStyle::window_double();
        let stulp_paths = bake_opening_plan_from_grid(
            params(stulp.default_width, OpeningKind::Window, LeafOperation::left()),
            &stulp.parts,
            None,
            &stulp.grid,
            PlanFrameLevel::Off,
            SwingGlyph::Off,
            DivisionLevel::Posts,
        );
        assert!(stulp_paths
            .iter()
            .all(|path| path.resolved_look_role() != Some(OpeningLookRole::Division)));
        assert!((bars[0].1 + 0.2).abs() < 1e-12);
        assert!((bars[1].1 + 0.6).abs() < 1e-12);
        assert!((bars[2].1 - 0.4).abs() < 1e-12);
    }

    #[test]
    fn muntin_profile_covers_every_bar_and_an_unknown_id_keeps_local_sizes() {
        use crate::modules::aec::engine::construction_grid::{CellBar, CellFill, GridCell};
        use crate::modules::aec::engine::section_profile::{SectionProfile, SectionShape};

        let mut library = StyleLibrary::empty();
        assert!(library.upsert_section_profile(SectionProfile::new(
            "sash-bar",
            "Sash bar",
            SectionShape::Rectangle {
                face: 0.018,
                depth: 0.024,
            },
        )));
        let grid = ConstructionGrid {
            muntin_profile_id: Some("sash-bar".into()),
            cells: vec![GridCell {
                fill: CellFill::Glass,
                bars: vec![CellBar {
                    face_width: 0.9,
                    depth: 0.9,
                    ..CellBar::default()
                }],
                ..GridCell::default()
            }],
            ..ConstructionGrid::default()
        };
        let leaf = resolve_section(None, 0.05, 0.04);
        let covered = muntin_bar_section(Some(&library), Some("sash-bar"), 0.9, 0.9, &leaf)
            .expect("profile");
        assert!((covered.face - 0.018).abs() < 1e-12);
        let kept = muntin_bar_section(Some(&library), Some("missing"), 0.03, 0.02, &leaf)
            .expect("local");
        assert!((kept.face - 0.03).abs() < 1e-12);
        let paths = bake_grid_plan_divisions(
            &grid,
            params(1.2, OpeningKind::Window, LeafOperation::left()),
            DivisionLevel::PostsAndMuntins,
            Some(&library),
            &leaf,
            &leaf,
        );
        assert_eq!(paths.len(), 1);
        let width = paths[0]
            .points
            .iter()
            .map(|point| point.0)
            .fold(f64::NEG_INFINITY, f64::max)
            - paths[0]
                .points
                .iter()
                .map(|point| point.0)
                .fold(f64::INFINITY, f64::min);
        assert!((width - 0.018).abs() < 1e-9);
    }

    #[test]
    fn stulp_bar_sits_on_the_bearer_and_the_neighbour_edge_stays_empty() {
        use crate::modules::aec::engine::construction_grid::division_preset;

        let grid = division_preset("double", crate::modules::aec::engine::construction_grid::CellFill::Glass)
            .expect("preset");
        let leaf = resolve_section(None, 0.04, 0.06);
        let paths = bake_grid_elevation_divisions(
            &grid,
            params(1.2, OpeningKind::Window, LeafOperation::left()),
            DivisionLevel::Posts,
            None,
            &leaf,
            &leaf,
        );
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].look_role, Some(OpeningLookRole::Leaf));
        let center = paths[0]
            .points
            .iter()
            .map(|point| point.0)
            .sum::<f64>()
            / paths[0].points.len() as f64;
        assert!((center - (0.6 - 0.6 + 0.02)).abs() < 1e-9);
    }

    #[test]
    fn fold_chain_draws_one_schema_per_leaf_and_no_interior_post() {
        use crate::modules::aec::engine::construction_grid::{
            CellFill, ConstructionGrid, GridCell, GridTrack, LeafOperation, TrackEnd, TrackMeasure,
            TrackMeeting,
        };

        let fold = LeafOperation::Fold { stack: TrackEnd::End };
        let grid = ConstructionGrid {
            columns: (0..4)
                .map(|index| GridTrack {
                    measure: TrackMeasure::Fraction(1.0),
                    meeting: if index < 3 {
                        TrackMeeting::Flush
                    } else {
                        TrackMeeting::Mullion
                    },
                    profile_id: None,
                })
                .collect(),
            rows: vec![GridTrack::default()],
            cells: (0..4)
                .map(|column| GridCell {
                    column,
                    fill: CellFill::Glass,
                    operation: Some(fold),
                    ..GridCell::default()
                })
                .collect(),
            muntin_profile_id: None,
        };
        let baked = params(2.0, OpeningKind::Window, fold);
        let schemas = bake_grid_elevation_schema(&grid, baked, 0.0);
        assert_eq!(schemas.len(), 4);
        for pair in schemas.windows(2) {
            assert_eq!(pair[0].points[1], pair[1].points[0]);
        }
        let leaf = resolve_section(None, 0.04, 0.06);
        let posts = bake_grid_elevation_divisions(
            &grid,
            baked,
            DivisionLevel::Posts,
            None,
            &leaf,
            &leaf,
        );
        assert!(posts.is_empty());

        let mut mixed = grid.clone();
        mixed.cells[3].operation = Some(LeafOperation::Slide {
            direction: TrackEnd::End,
        });
        let mixed_schemas = bake_grid_elevation_schema(&mixed, baked, 0.0);
        assert_eq!(mixed_schemas.len(), 4);
        assert_ne!(mixed_schemas[2].points[1], mixed_schemas[3].points[0]);
    }

    #[test]
    fn glass_bites_into_the_surrounding_bar_and_stops_at_its_outer_edge() {
        assert!((glass_edge_into_bar(0.08, 1.0, 0.08, 0.012) - 0.148).abs() < 1e-12);
        assert!((glass_edge_into_bar(0.0, 1.0, 0.08, 0.2) - 0.0).abs() < 1e-12);
    }

    #[test]
    fn section_profile_outline_drives_plan_frame_paths_and_hull_dimensions() {
        use crate::modules::aec::engine::section_profile::{SectionProfile, SectionShape};

        let mut library = StyleLibrary::empty();
        assert!(library.upsert_section_profile(SectionProfile::new(
            "triangle_frame",
            "Triangle frame",
            SectionShape::Triangle {
                base: 0.06,
                height: 0.05,
            },
        )));
        let mut parts = OpeningParts::default();
        parts.fixed_frame.profile_id = Some("triangle_frame".to_string());
        parts.fixed_frame.face_width = 0.0;
        parts.fixed_frame.wall_depth = 0.0;
        let paths = bake_opening_plan_paths(
            params(1.2, OpeningKind::Window, LeafOperation::left()),
            &parts,
            Some(&library),
            &DivisionGrid::default(),
            PlanFrameLevel::Profile,
            SwingGlyph::Off,
            DivisionLevel::Off,
        );
        let frames: Vec<_> = paths
            .iter()
            .filter(|path| path.slot == OpeningComponentSlot::Frame2D)
            .collect();
        assert_eq!(frames.len(), 2);
        assert!(frames.iter().all(|path| path.closed && path.points.len() == 3));
        let first = frames[0];
        let min_x = first.points.iter().map(|point| point.0).fold(f64::INFINITY, f64::min);
        let max_x = first.points.iter().map(|point| point.0).fold(f64::NEG_INFINITY, f64::max);
        let min_y = first.points.iter().map(|point| point.1).fold(f64::INFINITY, f64::min);
        let max_y = first.points.iter().map(|point| point.1).fold(f64::NEG_INFINITY, f64::max);
        assert!((max_x - min_x - 0.06).abs() < 1e-12);
        assert!((max_y - min_y - 0.05).abs() < 1e-12);

        let hull = resolve_component_section(Some(&library), Some("triangle_frame"), 1.0, 1.0);
        assert_eq!((hull.face, hull.depth), (0.06, 0.05));

        let grid = DivisionGrid {
            vertical: vec![0.5, 0.5],
            cells: vec![crate::modules::aec::engine::opening_style::OpeningCell::default(); 2],
            profile_id: Some("triangle_frame".into()),
            ..DivisionGrid::default()
        };
        let division_paths = bake_opening_plan_paths(
            params(1.2, OpeningKind::Window, LeafOperation::left()),
            &OpeningParts::default(),
            Some(&library),
            &grid,
            PlanFrameLevel::Off,
            SwingGlyph::Off,
            DivisionLevel::Posts,
        );
        let division = division_paths
            .iter()
            .find(|path| path.resolved_look_role() == Some(OpeningLookRole::Division))
            .expect("division profile contour");
        assert!(division.closed);
        assert_eq!(division.points.len(), 3);

        let elevation_paths = bake_opening_elevation_paths(
            params(1.2, OpeningKind::Window, LeafOperation::left()),
            &OpeningParts::default(),
            Some(&library),
            &grid,
            ElevationFrameLevel::Off,
            DivisionLevel::Posts,
        );
        let elevation_division = elevation_paths
            .iter()
            .find(|path| path.slot == OpeningComponentSlot::ElevationMuntins2D)
            .expect("elevation division profile");
        assert!(elevation_division.closed);
        assert_eq!(elevation_division.points.len(), 4);
    }

    #[test]
    fn plan_frame_levels_draw_line_double_line_and_absolute_profile() {
        let parts = OpeningParts::default();
        let grid = DivisionGrid::default();
        let params = params(1.2, OpeningKind::Window, LeafOperation::left());
        let frame_paths = |level| {
            bake_opening_plan_paths(
                params,
                &parts,
                None,
                &grid,
                level,
                SwingGlyph::Off,
                DivisionLevel::Off,
            )
            .into_iter()
            .filter(|path| path.slot == OpeningComponentSlot::Frame2D)
            .collect::<Vec<_>>()
        };
        let single = frame_paths(PlanFrameLevel::SingleLine);
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].points.len(), 2);
        let double = frame_paths(PlanFrameLevel::DoubleLine);
        assert_eq!(double.len(), 2);
        assert!(double.iter().all(|path| !path.closed));
        let profile = frame_paths(PlanFrameLevel::Profile);
        assert_eq!(profile.len(), 2);
        assert!(profile.iter().all(|path| path.closed));
        assert!((profile[0].points[1].0 - profile[0].points[0].0 - 0.06).abs() < 1e-12);
    }

    #[test]
    fn plan_triangle_ignores_opening_angle_but_arc_uses_it() {
        let parts = OpeningParts::default();
        let grid = DivisionGrid::default();
        let mut first_params = params(1.0, OpeningKind::Door, LeafOperation::left());
        first_params.opening_angle_deg = 35.0;
        let mut second_params = first_params;
        second_params.opening_angle_deg = 120.0;
        let glyph_paths = |p, glyph| {
            bake_opening_plan_paths(
                p,
                &parts,
                None,
                &grid,
                PlanFrameLevel::Off,
                glyph,
                DivisionLevel::Off,
            )
            .into_iter()
            .filter(|path| path.slot == OpeningComponentSlot::Swing2D)
            .collect::<Vec<_>>()
        };
        assert_eq!(
            glyph_paths(first_params, SwingGlyph::Triangle),
            glyph_paths(second_params, SwingGlyph::Triangle)
        );
        assert_ne!(
            glyph_paths(first_params, SwingGlyph::Arc),
            glyph_paths(second_params, SwingGlyph::Arc)
        );
    }

    #[test]
    fn window_frame_thickness_does_not_scale_with_width() {
        let slots = default_slots_for_kind(OpeningKind::Window);
        let a = bake_opening_generators(&slots, params(1.2, OpeningKind::Window, LeafOperation::left()), None);
        let b = bake_opening_generators(&slots, params(1.8, OpeningKind::Window, LeafOperation::left()), None);
        let ia = inner_frame_half_width(&a).unwrap();
        let ib = inner_frame_half_width(&b).unwrap();
        assert!((ia - (1.2 * 0.5 - DEFAULT_FRAME_THICKNESS)).abs() < 1e-9);
        assert!((ib - (1.8 * 0.5 - DEFAULT_FRAME_THICKNESS)).abs() < 1e-9);
        let inset_a = 1.2 * 0.5 - ia;
        let inset_b = 1.8 * 0.5 - ib;
        assert!((inset_a - DEFAULT_FRAME_THICKNESS).abs() < 1e-9);
        assert!((inset_b - DEFAULT_FRAME_THICKNESS).abs() < 1e-9);
    }

    #[test]
    fn door_swing_mirrors_with_hinge() {
        let slots = default_slots_for_kind(OpeningKind::Door);
        let left = bake_opening_generators(
            &slots,
            params(0.9, OpeningKind::Door, LeafOperation::left()),
            None,
        );
        let right = bake_opening_generators(
            &slots,
            params(0.9, OpeningKind::Door, LeafOperation::right()),
            None,
        );
        let leaf_l = left
            .iter()
            .find(|p| p.slot == OpeningComponentSlot::Leaf2D)
            .unwrap();
        let leaf_r = right
            .iter()
            .find(|p| p.slot == OpeningComponentSlot::Leaf2D)
            .unwrap();
        assert!(leaf_l.points[0].0 < 0.0);
        assert!(leaf_r.points[0].0 > 0.0);
        assert!((leaf_l.points[1].0 + leaf_r.points[1].0).abs() < 1e-9);
        assert!(left.iter().any(|p| p.slot == OpeningComponentSlot::Swing2D));
        assert!(right.iter().any(|p| p.slot == OpeningComponentSlot::Swing2D));
    }

    #[test]
    fn door_swing_direction_interior_vs_exterior_flips_y_deflection() {
        use crate::modules::aec::engine::openings::SwingSide;

        let slots = default_slots_for_kind(OpeningKind::Door);

        // Exterior is the negative side of the wall-local Y axis.
        let mut params_ext = params(0.9, OpeningKind::Door, LeafOperation::left());
        params_ext.swing_side = SwingSide::Exterior;
        let exterior = bake_opening_generators(&slots, params_ext, None);

        // Interior swing is the positive side of the wall-local Y axis.
        let mut params_int = params(0.9, OpeningKind::Door, LeafOperation::left());
        params_int.swing_side = SwingSide::Interior;
        let interior = bake_opening_generators(&slots, params_int, None);

        // Extract leaf segments (Leaf2D slot)
        let leaf_ext = exterior
            .iter()
            .find(|p| p.slot == OpeningComponentSlot::Leaf2D)
            .expect("exterior should have Leaf2D");
        let leaf_int = interior
            .iter()
            .find(|p| p.slot == OpeningComponentSlot::Leaf2D)
            .expect("interior should have Leaf2D");

        // Both should have 2 points: start and end
        assert_eq!(leaf_ext.points.len(), 2, "exterior leaf should have 2 points");
        assert_eq!(leaf_int.points.len(), 2, "interior leaf should have 2 points");

        // Start points (hinge) should be identical
        assert!(
            (leaf_ext.points[0].0 - leaf_int.points[0].0).abs() < 1e-9,
            "hinge X should be identical"
        );
        assert!(
            (leaf_ext.points[0].1 - leaf_int.points[0].1).abs() < 1e-9,
            "hinge Y should be identical"
        );

        // End points (leaf tip) should have opposite Y deflection
        let ext_y = leaf_ext.points[1].1;
        let int_y = leaf_int.points[1].1;
        assert!(ext_y < 0.0, "exterior leaf should deflect in -Y");
        assert!(int_y > 0.0, "interior leaf should deflect in +Y");
        assert!((ext_y + int_y).abs() < 1e-9, "Y deflections should be symmetric");

        // Extract swing arcs (Swing2D slot)
        let swing_ext = exterior
            .iter()
            .find(|p| p.slot == OpeningComponentSlot::Swing2D)
            .expect("exterior should have Swing2D");
        let swing_int = interior
            .iter()
            .find(|p| p.slot == OpeningComponentSlot::Swing2D)
            .expect("interior should have Swing2D");

        // Swing arcs should have same number of points
        assert_eq!(
            swing_ext.points.len(),
            swing_int.points.len(),
            "swing arcs should have same point count"
        );

        // Exterior swing stays at/beyond wall-local -Y, interior at/beyond +Y.
        for (i, &(_, y_ext)) in swing_ext.points.iter().enumerate() {
            let (_, y_int) = swing_int.points[i];
            assert!(
                y_ext <= 1e-9,
                "exterior swing arc point {} should have Y >= 0, got {}",
                i,
                y_ext
            );
            assert!(
                y_int >= -1e-9,
                "interior swing arc point {} should have Y <= 0, got {}",
                i,
                y_int
            );
            // Symmetric around Y=0
            assert!(
                (y_ext + y_int).abs() < 1e-9,
                "swing arc point {} Y values should be symmetric, got {} and {}",
                i,
                y_ext,
                y_int
            );
        }
    }

    #[test]
    fn breakthrough_mark2d_has_no_swing_or_frame() {
        let slots = default_slots_for_kind(OpeningKind::Breakthrough);
        let baked = bake_opening_generators(
            &slots,
            params(1.0, OpeningKind::Breakthrough, LeafOperation::left()),
            None,
        );
        assert!(baked.iter().all(|p| {
            p.slot == OpeningComponentSlot::Mark2D
                || p.slot == OpeningComponentSlot::BreakthroughSymbol2D
                || p.slot == OpeningComponentSlot::OpeningLabel2D
                || p.slot == OpeningComponentSlot::ElevationContour2D
        }));
        assert!(baked
            .iter()
            .any(|p| p.slot == OpeningComponentSlot::Mark2D && p.points.len() == 2));
        assert!(!baked
            .iter()
            .any(|p| p.slot == OpeningComponentSlot::Swing2D
                || p.slot == OpeningComponentSlot::Frame2D));
    }

    #[test]
    fn display_config_can_hide_swing() {
        let slots = default_slots_for_kind(OpeningKind::Door);
        let mut rules = ComponentRuleSet::default();
        rules
            .visibility
            .insert(OpeningComponentSlot::Swing2D.key().to_string(), false);
        let baked = bake_opening_generators(
            &slots,
            params(0.9, OpeningKind::Door, LeafOperation::left()),
            Some(&rules),
        );
        assert!(!baked
            .iter()
            .any(|p| p.slot == OpeningComponentSlot::Swing2D));
        assert!(baked
            .iter()
            .any(|p| p.slot == OpeningComponentSlot::Frame2D));
    }

    #[test]
    fn disabled_interior_board_does_not_add_plan_sill_paths_on_inside_face() {
        let mut parts = OpeningParts::default();
        parts.exterior_sill.thickness = 0.05;
        parts.interior_board.enabled = false;
        parts.interior_board.thickness = 0.05;

        let paths = bake_opening_plan_paths(
            params(1.2, OpeningKind::Window, LeafOperation::left()),
            &parts,
            None,
            &DivisionGrid::default(),
            PlanFrameLevel::Off,
            SwingGlyph::Off,
            DivisionLevel::Off,
        );
        let sill_paths: Vec<_> = paths
            .iter()
            .filter(|path| path.slot == OpeningComponentSlot::Sill2D)
            .collect();

        assert!(!sill_paths.is_empty());
        assert!(sill_paths
            .iter()
            .flat_map(|path| &path.points)
            .all(|(_, y)| *y <= 1e-9), "{sill_paths:#?}");
    }

    #[test]
    fn sill_upstand_interval_stands_outside_frame_and_clamps_to_frame_depth() {
        assert_eq!(sill_upstand_interval(-0.04, -1.0, 0.03, 0.08), (-0.07, -0.04));
        assert_eq!(sill_upstand_interval(0.04, 1.0, 0.03, 0.08), (0.04, 0.07));
        assert_eq!(sill_upstand_interval(0.04, 1.0, 0.2, 0.08), (0.04, 0.12));
    }

    #[test]
    fn sill_accessory_bounds_meet_the_fixed_frame_face() {
        let wall_y_min = -0.15;
        let wall_y_max = 0.15;
        let exterior = exterior_sill_y_bounds(wall_y_min, wall_y_max, 0.04, 0.08);
        let interior = interior_board_y_bounds(wall_y_min, wall_y_max, 0.03, 0.06);
        assert_eq!(exterior, (-0.19, -0.04));
        assert_eq!(interior, (0.03, 0.18));
        assert!(exterior.1 < wall_y_max);
        assert!(interior.0 > wall_y_min);

        let oversized = exterior_sill_y_bounds(wall_y_min, wall_y_max, 0.0, 0.5);
        assert_eq!(oversized, (wall_y_min, wall_y_min));
    }

    #[test]
    fn profile_sill_prism_uses_the_section_shape_and_frame_edge_origin() {
        let section = resolve_section(
            Some(&crate::modules::aec::engine::section_profile::SectionProfile::new(
                "triangular-sill",
                "Triangular sill",
                crate::modules::aec::engine::section_profile::SectionShape::Triangle {
                    base: 0.12,
                    height: 0.035,
                },
            )),
            0.0,
            0.0,
        );
        let body = build_opening_profile_prism_3d(1.0, &section, -0.04, 1.0)
            .expect("profile section prism");
        let mut x_bounds = (f64::INFINITY, f64::NEG_INFINITY);
        let mut y_bounds = (f64::INFINITY, f64::NEG_INFINITY);
        let mut z_bounds = (f64::INFINITY, f64::NEG_INFINITY);
        for edge_key in body.edge_keys() {
            let edge = body.edges.get(edge_key).expect("body edge");
            for vertex_key in [edge.start, edge.end] {
                let point = body.vertices.get(vertex_key).expect("edge vertex").point;
                x_bounds.0 = x_bounds.0.min(point[0]);
                x_bounds.1 = x_bounds.1.max(point[0]);
                y_bounds.0 = y_bounds.0.min(point[1]);
                y_bounds.1 = y_bounds.1.max(point[1]);
                z_bounds.0 = z_bounds.0.min(point[2]);
                z_bounds.1 = z_bounds.1.max(point[2]);
            }
        }
        assert!((x_bounds.0 + 0.5).abs() < 1e-12);
        assert!((x_bounds.1 - 0.5).abs() < 1e-12);
        assert!((y_bounds.0 + 0.04).abs() < 1e-12);
        assert!((y_bounds.1 + 0.005).abs() < 1e-12);
        assert!(z_bounds.0.abs() < 1e-12);
        assert!((z_bounds.1 - 0.12).abs() < 1e-12);
    }

    fn add_test_wall(scene: &mut Scene) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let mut wall = Wall::new("s", 3.0, 0);
        wall.justification = WallJustification::Center;
        wall.layers = vec![WallLayer {
            material: "Concrete".into(),
            thickness: 0.3,
            function: "Structural".into(),
            axis_offset: -0.15,
            ..WallLayer::default()
        }];
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record_for_wall(&wall);
        entity.common_mut().extended_data.add_record(record);
        let handle = scene.add_entity(entity);
        regenerate_wall_representation(scene, handle, None).expect("regen");
        handle
    }

    fn child_slots(scene: &Scene, opening: Handle) -> Vec<OpeningComponentSlot> {
        let mut slots: Vec<_> = collect_opening_display_children(scene, opening)
            .into_iter()
            .filter_map(|h| {
                scene
                    .document
                    .get_entity(h)
                    .and_then(opening_rep_slot_from_entity)
            })
            .collect();
        slots.sort_by_key(|s| s.key());
        slots
    }

    fn opening_label_handle(scene: &Scene, opening: Handle) -> Option<Handle> {
        collect_opening_display_children(scene, opening)
            .into_iter()
            .find(|handle| {
                matches!(scene.document.get_entity(*handle), Some(EntityType::MText(_)))
                    && scene
                        .document
                        .get_entity(*handle)
                        .and_then(opening_rep_slot_from_entity)
                        == Some(OpeningComponentSlot::OpeningLabel2D)
            })
    }

    #[test]
    fn disabled_fixed_frame_keeps_glazing_and_removes_frame_children() {
        let mut scene = Scene::new();
        let wall = add_test_wall(&mut scene);
        let mut lib = seed_default_library();
        let style = lib
            .opening_styles
            .iter_mut()
            .find(|style| style.style.id == SEED_WINDOW_STYLE_ID)
            .expect("seed window style");
        style.parts.fixed_frame.enabled = false;

        let (opening, _) = place_wall_opening(
            &mut scene,
            wall,
            DVec3::new(1.5, 0.0, 0.0),
            OpeningKind::Window,
            Some(&lib),
            None,
            None,
        )
        .expect("place window");
        let slots = child_slots(&scene, opening);

        assert!(!slots.contains(&OpeningComponentSlot::Frame2D));
        assert!(!slots.contains(&OpeningComponentSlot::Frame3D));
        assert!(slots.contains(&OpeningComponentSlot::Glazing2D));
        assert!(slots.contains(&OpeningComponentSlot::Glazing3D));
    }

    #[test]
    fn division_posts_use_the_grid_layer_and_keep_a_changed_layer_color() {
        use crate::modules::aec::engine::construction_grid::{
            ConstructionGrid, GridTrack, TrackMeasure,
        };

        let mut scene = Scene::new();
        let wall = add_test_wall(&mut scene);
        let mut lib = seed_default_library();
        let style = lib
            .opening_styles
            .iter_mut()
            .find(|style| style.style.id == SEED_WINDOW_STYLE_ID)
            .expect("seed window style");
        let fraction = GridTrack {
            measure: TrackMeasure::Fraction(1.0),
            ..GridTrack::default()
        };
        style.grid = ConstructionGrid {
            columns: vec![fraction.clone(), fraction],
            ..ConstructionGrid::default()
        };
        style.looks.division.line_color = None;

        let (opening, _) = place_wall_opening(
            &mut scene,
            wall,
            DVec3::new(1.5, 0.0, 0.0),
            OpeningKind::Window,
            Some(&lib),
            None,
            None,
        )
        .expect("place window");

        let children = collect_opening_display_children(&scene, opening);
        let posts: Vec<_> = children
            .iter()
            .copied()
            .filter(|handle| {
                scene
                    .document
                    .get_entity(*handle)
                    .and_then(opening_rep_slot_from_entity)
                    == Some(OpeningComponentSlot::Division2D)
            })
            .collect();
        assert!(!posts.is_empty());
        for handle in &posts {
            let entity = scene.document.get_entity(*handle).expect("post");
            assert_eq!(entity.as_entity().layer(), AEC_OPENING_GRID_LAYER);
            assert!(matches!(
                entity.as_entity().color(),
                acadrust::types::Color::ByLayer
            ));
        }
        let glass = children.iter().copied().find(|handle| {
            scene
                .document
                .get_entity(*handle)
                .and_then(opening_rep_slot_from_entity)
                == Some(OpeningComponentSlot::Glazing2D)
        });
        let glass = glass.expect("glass");
        assert_ne!(
            scene.document.get_entity(glass).unwrap().as_entity().layer(),
            AEC_OPENING_GRID_LAYER
        );
        assert_eq!(
            scene
                .document
                .layers
                .get(AEC_OPENING_GRID_LAYER)
                .expect("grid layer")
                .color,
            acadrust::types::Color::Index(8)
        );

        scene
            .document
            .layers
            .get_mut(AEC_OPENING_GRID_LAYER)
            .expect("grid layer")
            .color = acadrust::types::Color::Index(3);
        regenerate_opening_display(&mut scene, opening, Some(&lib), None);
        assert_eq!(
            scene
                .document
                .layers
                .get(AEC_OPENING_GRID_LAYER)
                .expect("grid layer")
                .color,
            acadrust::types::Color::Index(3)
        );
    }

    #[test]
    fn opening_look_is_written_to_generated_frame_polyline() {
        let mut scene = Scene::new();
        let wall = add_test_wall(&mut scene);
        let mut lib = seed_default_library();
        let color = acadrust::types::Color::Rgb { r: 12, g: 34, b: 56 };
        let style = lib
            .opening_styles
            .iter_mut()
            .find(|style| style.style.id == SEED_WINDOW_STYLE_ID)
            .expect("seed window style");
        style.looks.fixed_frame.line_type = Some("DASHED".into());
        style.looks.fixed_frame.line_color = Some(color);

        let (opening, _) = place_wall_opening(
            &mut scene,
            wall,
            DVec3::new(1.5, 0.0, 0.0),
            OpeningKind::Window,
            Some(&lib),
            None,
            None,
        )
        .expect("place window");
        let frame = collect_opening_display_children(&scene, opening)
            .into_iter()
            .find(|handle| {
                scene
                    .document
                    .get_entity(*handle)
                    .and_then(opening_rep_slot_from_entity)
                    == Some(OpeningComponentSlot::Frame2D)
            })
            .expect("frame polyline");
        let entity = scene.document.get_entity(frame).expect("frame entity");

        assert_eq!(entity.common().linetype, "DASHED");
        assert_eq!(entity.common().color, color);
    }

    #[test]
    fn opening_material_look_resolves_native_render_material_when_present() {
        let mut scene = Scene::new();
        let wall = add_test_wall(&mut scene);
        let mut lib = seed_default_library();
        let mut aec_material = crate::modules::aec::engine::material::Material::new(
            "wood-look".into(),
            "Oak finish".into(),
            "ANSI31".into(),
            0x123456,
            "DASHED".into(),
        );
        aec_material.render_material_ref = Some("oak_native".into());
        lib.materials.push(aec_material);
        let style = lib
            .opening_styles
            .iter_mut()
            .find(|style| style.style.id == SEED_WINDOW_STYLE_ID)
            .expect("seed window style");
        style.looks.fixed_frame.material_id = Some("wood-look".into());

        let material_handle = scene.document.allocate_handle();
        let mut native = codec::objects::Material::new();
        native.handle = material_handle;
        native.name = "oak_native".into();
        scene.document.objects.insert(
            material_handle,
            codec::objects::ObjectType::Material(native),
        );

        let (opening, _) = place_wall_opening(
            &mut scene,
            wall,
            DVec3::new(1.5, 0.0, 0.0),
            OpeningKind::Window,
            Some(&lib),
            None,
            None,
        )
        .expect("place window");
        let children = collect_opening_display_children(&scene, opening);
        let frame = children
            .iter()
            .find(|handle| {
                scene
                    .document
                    .get_entity(**handle)
                    .and_then(opening_rep_slot_from_entity)
                    == Some(OpeningComponentSlot::Frame2D)
            })
            .copied()
            .expect("frame polyline");
        let frame_entity = scene.document.get_entity(frame).unwrap();
        assert_eq!(frame_entity.common().linetype, "DASHED");
        assert_eq!(
            frame_entity.common().color,
            acadrust::types::Color::Rgb {
                r: 0x12,
                g: 0x34,
                b: 0x56,
            }
        );

        let frame_solid = children
            .into_iter()
            .find(|handle| {
                matches!(scene.document.get_entity(*handle), Some(EntityType::Solid3D(_)))
                    && scene
                        .document
                        .get_entity(*handle)
                        .and_then(opening_rep_slot_from_entity)
                        == Some(OpeningComponentSlot::Frame3D)
            })
            .expect("frame solid");
        let solid = scene.document.get_entity(frame_solid).unwrap();
        assert_eq!(solid.common().material_flags, 3);
        assert_eq!(solid.common().material_handle, Some(material_handle));
    }

    #[test]
    fn plan_style_override_wins_over_opening_look() {
        let mut scene = Scene::new();
        let wall = add_test_wall(&mut scene);
        let mut lib = seed_default_library();
        let style = lib
            .opening_styles
            .iter_mut()
            .find(|style| style.style.id == SEED_WINDOW_STYLE_ID)
            .expect("seed window style");
        style.looks.fixed_frame.line_type = Some("DASHED".into());
        let override_color = acadrust::types::Color::Rgb { r: 200, g: 10, b: 20 };
        let mut rules = ComponentRuleSet::default();
        rules.style_override.insert(
            OpeningComponentSlot::Frame2D.key().to_string(),
            crate::modules::aec::engine::display_component::ComponentStyleOverride {
                line_type: Some("HIDDEN".into()),
                line_color: Some(override_color),
                ..Default::default()
            },
        );

        let (opening, _) = place_wall_opening(
            &mut scene,
            wall,
            DVec3::new(1.5, 0.0, 0.0),
            OpeningKind::Window,
            Some(&lib),
            Some(&rules),
            None,
        )
        .expect("place window");
        let frame = collect_opening_display_children(&scene, opening)
            .into_iter()
            .find(|handle| {
                scene
                    .document
                    .get_entity(*handle)
                    .and_then(opening_rep_slot_from_entity)
                    == Some(OpeningComponentSlot::Frame2D)
            })
            .expect("frame polyline");
        let entity = scene.document.get_entity(frame).expect("frame entity");

        assert_eq!(entity.common().linetype, "HIDDEN");
        assert_eq!(entity.common().color, override_color);
    }

    #[test]
    fn opening_plan_mullions_do_not_depend_on_elevation_slot() {
        let mut slots = default_slots_for_kind(OpeningKind::Window);
        slots.remove(&OpeningComponentSlot::ElevationMuntins2D);
        let grid = DivisionGrid {
            vertical: vec![0.5, 0.5],
            cells: vec![OpeningCell::default(), OpeningCell::default()],
            ..DivisionGrid::default()
        };

        assert_eq!(
            representation_levels(&slots, &grid, None).divisions,
            DivisionLevel::Posts
        );
        assert_eq!(
            representation_levels(&slots, &grid, Some(DivisionLevel::Off)).divisions,
            DivisionLevel::Off
        );
    }

    #[test]
    fn positive_lintel_height_emits_a_solid3d_child() {
        let mut scene = Scene::new();
        let wall = add_test_wall(&mut scene);
        let mut lib = seed_default_library();
        let style = lib
            .opening_styles
            .iter_mut()
            .find(|style| style.style.id == SEED_WINDOW_STYLE_ID)
            .expect("seed window style");
        style.parts.lintel.height = 0.2;
        style.parts.lintel.bearing_left = 0.1;
        style.parts.lintel.bearing_right = 0.1;
        let lintel_color = acadrust::types::Color::Rgb { r: 90, g: 120, b: 150 };
        style.looks.lintel.line_color = Some(lintel_color);

        let (opening, _) = place_wall_opening(
            &mut scene,
            wall,
            DVec3::new(1.5, 0.0, 0.0),
            OpeningKind::Window,
            Some(&lib),
            None,
            None,
        )
        .expect("place window");

        let lintel = collect_opening_display_children(&scene, opening)
            .into_iter()
            .find(|handle| {
                matches!(scene.document.get_entity(*handle), Some(EntityType::Solid3D(_)))
                    && scene
                        .document
                        .get_entity(*handle)
                        .and_then(opening_rep_slot_from_entity)
                        == Some(OpeningComponentSlot::Solid3D)
            })
            .expect("lintel solid");
        assert_eq!(
            scene.document.get_entity(lintel).unwrap().common().color,
            lintel_color
        );
    }

    #[test]
    fn zero_accessory_dimensions_emit_no_solid3d_child() {
        let mut scene = Scene::new();
        let wall = add_test_wall(&mut scene);
        let lib = seed_default_library();

        let (opening, _) = place_wall_opening(
            &mut scene,
            wall,
            DVec3::new(1.5, 0.0, 0.0),
            OpeningKind::Window,
            Some(&lib),
            None,
            None,
        )
        .expect("place window");

        assert!(!collect_opening_display_children(&scene, opening)
            .into_iter()
            .any(|handle| {
                matches!(scene.document.get_entity(handle), Some(EntityType::Solid3D(_)))
                    && scene
                        .document
                        .get_entity(handle)
                        .and_then(opening_rep_slot_from_entity)
                        == Some(OpeningComponentSlot::Solid3D)
            }));
    }

    #[test]
    fn disabled_lintel_emits_no_solid3d_child() {
        let mut scene = Scene::new();
        let wall = add_test_wall(&mut scene);
        let mut lib = seed_default_library();
        let style = lib
            .opening_styles
            .iter_mut()
            .find(|style| style.style.id == SEED_WINDOW_STYLE_ID)
            .expect("seed window style");
        style.parts.lintel.height = 0.2;
        style.parts.lintel.enabled = false;

        let (opening, _) = place_wall_opening(
            &mut scene,
            wall,
            DVec3::new(1.5, 0.0, 0.0),
            OpeningKind::Window,
            Some(&lib),
            None,
            None,
        )
        .expect("place window");

        assert!(!collect_opening_display_children(&scene, opening)
            .into_iter()
            .any(|handle| {
                matches!(scene.document.get_entity(handle), Some(EntityType::Solid3D(_)))
                    && scene
                        .document
                        .get_entity(handle)
                        .and_then(opening_rep_slot_from_entity)
                        == Some(OpeningComponentSlot::Solid3D)
            }));
    }

    #[test]
    fn place_window_emits_opening_rep_children_and_resolves_pick() {
        let mut scene = Scene::new();
        let wall = add_test_wall(&mut scene);
        let lib = seed_default_library();
        let (opening, _) = place_wall_opening(
            &mut scene,
            wall,
            DVec3::new(1.5, 0.0, 0.0),
            OpeningKind::Window,
            Some(&lib),
            None,
            None,
        )
        .expect("place");
        let slots = child_slots(&scene, opening);
        assert!(slots.contains(&OpeningComponentSlot::Frame2D));
        assert!(slots.contains(&OpeningComponentSlot::Leaf2D));
        assert!(!slots.contains(&OpeningComponentSlot::Sill2D));
        assert!(!slots.contains(&OpeningComponentSlot::Swing2D));
        let child = collect_opening_display_children(&scene, opening)[0];
        assert_eq!(resolve_opening_package(&scene, child), opening);
        assert_eq!(
            engine::wall_package::resolve_wall_package(&scene, child),
            child
        );
        let parsed = opening_from_entity(scene.document.get_entity(opening).unwrap(), opening)
            .unwrap();
        assert_eq!(parsed.style_id.as_deref(), Some(SEED_WINDOW_STYLE_ID));
        assert!((parsed.width - OpeningStyle::standard_window().default_width).abs() < 1e-12);
    }

    #[test]
    fn opening_label_keeps_its_native_annotation_scale_after_regeneration() {
        let mut scene = Scene::new();
        scene.document.header.current_annotation_scale = "1:50".to_string();
        let native_scale = crate::scene::annotative::ensure_scale_object(
            &mut scene.document,
            &codec::objects::Scale::new("1:50", 1.0, 50.0),
        );
        let wall = add_test_wall(&mut scene);
        let lib = seed_default_library();
        let (opening, _) = place_wall_opening(
            &mut scene,
            wall,
            DVec3::new(1.5, 0.0, 0.0),
            OpeningKind::Window,
            Some(&lib),
            None,
            None,
        )
        .expect("place window");

        let first_label = opening_label_handle(&scene, opening).expect("first opening label");
        assert!(matches!(
            scene.document.get_entity(first_label),
            Some(EntityType::MText(mtext)) if mtext.is_annotative
        ));
        assert_eq!(
            engine::annotative_label::native_annotation_scale(&scene.document, first_label),
            Some(native_scale)
        );

        scene.document.header.current_annotation_scale = "1:20".to_string();
        let _ = crate::scene::annotative::ensure_scale_object(
            &mut scene.document,
            &codec::objects::Scale::new("1:20", 1.0, 20.0),
        );
        regenerate_opening_display(&mut scene, opening, Some(&lib), None);

        let second_label = opening_label_handle(&scene, opening).expect("regenerated opening label");
        assert_eq!(
            engine::annotative_label::native_annotation_scale(&scene.document, second_label),
            Some(native_scale)
        );
        assert!(matches!(
            scene.document.get_entity(second_label),
            Some(EntityType::MText(mtext))
                if mtext.is_annotative && (mtext.height - 0.15).abs() < 1.0e-9
        ));
        assert_eq!(
            crate::scene::annotative::object_scale_memberships(&scene.document, second_label).len(),
            1
        );
    }

    #[test]
    fn place_breakthrough_has_mark_not_frame() {
        let mut scene = Scene::new();
        let wall = add_test_wall(&mut scene);
        let lib = seed_default_library();
        let (opening, _) = place_wall_opening(
            &mut scene,
            wall,
            DVec3::new(2.0, 0.0, 0.0),
            OpeningKind::Breakthrough,
            Some(&lib),
            None,
            None,
        )
        .expect("place");
        let slots = child_slots(&scene, opening);
        assert!(slots.contains(&OpeningComponentSlot::Mark2D));
        assert!(!slots.contains(&OpeningComponentSlot::Frame2D));
        assert!(!slots.contains(&OpeningComponentSlot::Swing2D));
    }

    #[test]
    fn legacy_opening_without_style_id_uses_kind_generators() {
        let mut scene = Scene::new();
        let wall = add_test_wall(&mut scene);
        let (opening, _) = place_wall_opening(
            &mut scene,
            wall,
            DVec3::new(1.5, 0.0, 0.0),
            OpeningKind::Window,
            None,
            None,
            None,
        )
        .expect("place");
        let parsed = opening_from_entity(scene.document.get_entity(opening).unwrap(), opening)
            .unwrap();
        assert_eq!(parsed.style_id, None);
        assert!((parsed.width - DEFAULT_WINDOW_WIDTH).abs() < 1e-12);
        let slots = child_slots(&scene, opening);
        assert!(slots.contains(&OpeningComponentSlot::Frame2D));
        assert!(!slots.contains(&OpeningComponentSlot::Sill2D));
    }

    #[test]
    fn axis_width_grip_keeps_frame_inset() {
        let axis = vec![(0.0, 0.0), (5.0, 0.0)];
        let mut opening = Opening::window(Handle::new(1), Handle::new(2), 2.0);
        opening.width = 1.2;
        apply_opening_axis_grip(&axis, &mut opening, 2, DVec3::new(3.2, 0.0, 0.0));
        assert!((opening.width - 1.8).abs() < 1e-9);
        let slots = default_slots_for_kind(OpeningKind::Window);
        let baked = bake_opening_generators(
            &slots,
            params(opening.width, OpeningKind::Window, LeafOperation::left()),
            None,
        );
        let inner = inner_frame_half_width(&baked).unwrap();
        assert!((opening.width * 0.5 - inner - DEFAULT_FRAME_THICKNESS).abs() < 1e-9);
    }

    #[test]
    fn lintel_and_shutter_start_at_the_rough_head_inside_the_wall() {
        let wall_min = -0.15;
        let wall_max = 0.15;
        let head = 1.2;
        assert!((accessory_bottom(head, 0.04) - 1.24).abs() < 1e-12);
        assert!((accessory_bottom(head, -0.2) - head).abs() < 1e-12);

        let (y0, y1) = wall_clamped_interval(wall_min, wall_max, wall_min - 0.05, 0.4);
        assert!((y0 - wall_min).abs() < 1e-12);
        assert!((y1 - wall_max).abs() < 1e-12);

        let (lintel_y0, lintel_y1) = lintel_depth_interval(wall_min, wall_max, 0.5);
        assert!(lintel_y0 >= wall_min - 1e-12);
        assert!(lintel_y1 <= wall_max + 1e-12);
        assert!(lintel_y1 > lintel_y0);
    }

    #[test]
    fn side_overhangs_extend_only_the_named_sill_and_side() {
        let mut parts = OpeningParts::default();
        parts.exterior_sill.thickness = 0.04;
        parts.exterior_sill.overhang = 0.02;
        parts.exterior_sill.side_overhang_left = 0.05;
        parts.exterior_sill.side_overhang_right = 0.0;
        parts.interior_board.thickness = 0.03;
        parts.interior_board.overhang = 0.02;
        parts.interior_board.side_overhang_left = 0.0;
        parts.interior_board.side_overhang_right = 0.08;
        let section = ResolvedSection {
            outline: Vec::new(),
            face: 0.04,
            depth: 0.06,
        };
        let baked = bake_sill_lines_from_parts(
            OpeningComponentSlot::Sill2D,
            params(1.2, OpeningKind::Window, LeafOperation::left()),
            &parts,
            None,
            &section,
            &section,
            &section,
        );
        let span = |role: OpeningLookRole| {
            let path = baked
                .iter()
                .find(|path| path.look_role == Some(role) && path.closed)
                .expect("sill nose");
            let min_x = path.points.iter().map(|point| point.0).fold(f64::INFINITY, f64::min);
            let max_x = path.points.iter().map(|point| point.0).fold(f64::NEG_INFINITY, f64::max);
            (min_x, max_x)
        };
        let (ext_min, ext_max) = span(OpeningLookRole::ExteriorSill);
        let (int_min, int_max) = span(OpeningLookRole::InteriorBoard);
        assert!((ext_min - (-0.6 - 0.05)).abs() < 1e-12);
        assert!((ext_max - 0.6).abs() < 1e-12);
        assert!((int_min - -0.6).abs() < 1e-12);
        assert!((int_max - (0.6 + 0.08)).abs() < 1e-12);
    }

    #[test]
    fn exterior_swing_points_to_the_exterior_edge() {
        let mut baked = params(1.2, OpeningKind::Door, LeafOperation::left());
        baked.swing_side = SwingSide::Exterior;
        baked.wall_y_min = -0.15;
        baked.wall_y_max = 0.15;
        let exterior = bake_swing_arc(OpeningComponentSlot::Swing2D, baked, 0.6, 0.06);
        assert!(exterior[0].points.last().unwrap().1 < 0.0);
        baked.swing_side = SwingSide::Interior;
        let interior = bake_swing_arc(OpeningComponentSlot::Swing2D, baked, 0.6, 0.06);
        assert!(interior[0].points.last().unwrap().1 > 0.0);
        assert!((swing_toward_exterior(SwingSide::Exterior) + 1.0).abs() < 1e-12);
        assert!((swing_toward_exterior(SwingSide::Interior) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn nested_grid_bakes_an_inner_post_and_one_leaf_per_child() {
        use crate::modules::aec::engine::construction_grid::{
            CellFill, ConstructionGrid, GridCell, GridTrack, TrackMeasure,
        };

        let fraction = GridTrack {
            measure: TrackMeasure::Fraction(1.0),
            ..GridTrack::default()
        };
        let grid = ConstructionGrid {
            cells: vec![GridCell {
                columns: 1,
                rows: 1,
                fill: CellFill::Nested(ConstructionGrid {
                    columns: vec![fraction.clone(), fraction],
                    cells: vec![
                        GridCell {
                            fill: CellFill::Glass,
                            operation: Some(LeafOperation::left()),
                            ..GridCell::default()
                        },
                        GridCell {
                            column: 1,
                            fill: CellFill::Glass,
                            operation: Some(LeafOperation::right()),
                            ..GridCell::default()
                        },
                    ],
                    ..ConstructionGrid::default()
                }),
                ..GridCell::default()
            }],
            ..ConstructionGrid::default()
        };
        let params = params(1.2, OpeningKind::Window, LeafOperation::left());
        let posts = bake_grid_plan_divisions(
            &grid,
            params,
            DivisionLevel::Posts,
            None,
            &ResolvedSection {
                outline: Vec::new(),
                face: 0.04,
                depth: 0.06,
            },
            &ResolvedSection {
                outline: Vec::new(),
                face: 0.04,
                depth: 0.04,
            },
        );
        assert_eq!(posts.len(), 1);
        assert!((posts[0].points[0].0).abs() < 1e-9);
        let leaves = bake_grid_leaf_lines(&grid, params);
        assert_eq!(leaves.len(), 2);
    }

    #[test]
    fn glazing_line_bites_into_a_rendered_bar_and_stays_flush_without_one() {
        let grid = ConstructionGrid::default();
        let lines = bake_grid_glazing_lines(
            &grid,
            params(1.2, OpeningKind::Window, LeafOperation::left()),
            0.01,
            0.06,
        );
        let (left, right) = (lines[0].points[0].0, lines[0].points[1].0);
        assert!((left - (-0.6 + 0.05)).abs() < 1e-12);
        assert!((right - (0.6 - 0.05)).abs() < 1e-12);
        assert!((glass_edge_into_bar(0.0, 1.0, 0.0, 0.02) - 0.0).abs() < 1e-12);
    }

}
