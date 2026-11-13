use super::*;

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

pub(super) fn rules_with_plan_visibility(
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
        paths.extend(bake_grid_leaf_lines(
            grid,
            params,
            fixed_frame_section.face,
            leaf_frame_section.face,
            parts.leaf_frame.lateral_overlap.max(0.0),
        ));
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
            parts.leaf_frame.lateral_overlap.max(0.0),
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
    // The leaf sits on the swing side of the fixed frame (see
    // `LeafFrameDimensions::depth_center`); its plan line and swing glyph
    // follow that plane instead of the wall mid-plane.
    let leaf_y = parts.leaf_frame.depth_center(
        0.0,
        fixed_frame_section.depth,
        leaf_frame_section.depth,
        params.swing_side,
    );
    for baked in &mut paths {
        if matches!(
            baked.slot,
            OpeningComponentSlot::Leaf2D | OpeningComponentSlot::Swing2D
        ) {
            for point in &mut baked.points {
                point.1 += leaf_y;
            }
        }
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

pub(super) fn division_profile_section(
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

pub(super) fn resolve_component_section(
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

pub(super) fn representation_levels(
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

pub(super) fn bake_plan_frame_level(
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

pub(super) fn unclosed_section_outline(section: &ResolvedSection) -> &[(f64, f64)] {
    if section.outline.len() > 1 && section.outline.first() == section.outline.last() {
        &section.outline[..section.outline.len() - 1]
    } else {
        &section.outline
    }
}

pub(super) fn bake_grid_leaf_lines(
    grid: &ConstructionGrid,
    params: OpeningBakeParams,
    frame_face: f64,
    leaf_face: f64,
    overlap: f64,
) -> Vec<BakedPath> {
    // Same leaf edges as the 3D model: the leaf overlaps the fixed frame
    // laterally by `overlap`.
    let cells = leaf_cells_with_bar_offsets(
        grid,
        params.width,
        params.height,
        frame_face,
        None,
        leaf_face,
        overlap,
        true,
        params.kind == OpeningKind::Door,
    );
    let mut paths = Vec::new();
    for geo in cells {
        let cell = &geo.cell;
        if !matches!(
            cell.cell.fill,
            crate::modules::aec::engine::construction_grid::CellFill::Door
                | crate::modules::aec::engine::construction_grid::CellFill::Glass
        ) {
            continue;
        }
        let (left, right) = (geo.left, geo.right);
        let center_x = (left + right) * 0.5;
        let cell_width = right - left;
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
                    left + params.frame_thickness * 0.5
                }
                crate::modules::aec::engine::construction_grid::TrackEnd::End => {
                    right - params.frame_thickness * 0.5
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
                vec![(left, 0.0), (right, 0.0)],
                false,
                false,
            ));
        }
    }
    paths
}

pub(super) fn bake_grid_glazing_lines(
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

pub(super) fn bake_grid_plan_divisions(
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

pub(super) fn collect_plan_divisions(
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

pub(super) fn bake_plan_division_path(
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

pub(super) fn bake_grid_swing_glyphs(
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

pub(super) enum ChainPlane {
    Plan { free_y: f64 },
    Elevation,
}

pub(super) fn cell_operation(cell: &crate::modules::aec::engine::construction_grid::ResolvedCell, fallback: LeafOperation) -> LeafOperation {
    cell.cell.operation.unwrap_or(fallback)
}

pub(super) fn slide_or_fold(operation: LeafOperation) -> Option<LeafOperation> {
    match operation {
        LeafOperation::Slide { .. } | LeafOperation::Fold { .. } => Some(operation),
        _ => None,
    }
}

/// Adjacent cells of the same slide or fold kind, joined by a non-mullion edge.
pub(super) fn chain_runs(
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

pub(super) fn chain_cells(
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

pub(super) fn chain_schema_paths(
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

pub(super) fn plan_operation_schema(
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

pub(super) fn bake_grid_elevation_schema(
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
                // Apex on the strike side, open side at the hinge.
                paths.push(path(
                    slot,
                    vec![(hinge_x, bottom), (strike_x, mid_z), (hinge_x, top)],
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

pub(super) fn bake_elevation_frame_level(
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

pub(super) fn clipped_horizontal_span(params: OpeningBakeParams, y: f64, left: f64, right: f64) -> (f64, f64) {
    if params.shape != OpeningShape::Arch || y <= params.spring_height {
        return (left, right);
    }
    let half_width = params.width * 0.5;
    let rise = (params.height - params.spring_height).max(1e-9);
    let ratio = ((y - params.spring_height) / rise).clamp(0.0, 1.0);
    let arch_x = half_width * (1.0 - ratio * ratio).sqrt();
    (left.max(-arch_x), right.min(arch_x))
}

pub(super) fn bake_grid_elevation_divisions(
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

pub(super) fn collect_elevation_divisions(
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

pub(super) fn push_elevation_bar(
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

pub(super) fn section_for_profile(
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
pub(super) fn named_division_section(
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
pub(super) fn stulp_shift(meeting: TrackMeeting, face: f64) -> Option<f64> {
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
pub(super) fn flatten_resolved_cells(
    grid: &ConstructionGrid,
    width: f64,
    height: f64,
) -> Vec<crate::modules::aec::engine::construction_grid::ResolvedCell> {
    let resolved = resolve_grid(grid, width, height);
    let mut cells = Vec::new();
    collect_parent_cells(&resolved, -width * 0.5, 0.0, &mut cells);
    cells
}

pub(super) fn collect_parent_cells(
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
pub(super) struct LeafBarBoundary {
    low: f64,
    high: f64,
}

#[derive(Clone, Copy, Default)]
pub(super) struct LeafCellBoundaries {
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

pub(super) fn collect_leaf_cells_with_bar_offsets(
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
                // The hinge axis sits on the leaf's own edge, which already
                // includes the lateral overlap into the fixed frame.
                pivot_start: left,
                pivot_end: right,
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn leaf_cell_side_bar(
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

pub(super) fn leaf_edge_inside_bar(
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

pub(super) fn bake_elevation_vertical_division_path(
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

pub(super) fn bake_elevation_horizontal_division_path(
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

