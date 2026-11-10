use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TrackMeasure {
    Fixed(f64),
    Percent(f64),
    Fraction(f64),
}

pub fn parse_track_measure(value: &str) -> Option<TrackMeasure> {
    let value = value.trim();
    let measure = if let Some(percent) = value.strip_suffix('%') {
        TrackMeasure::Percent(percent.trim().parse().ok()?)
    } else if let Some(fraction) = value.strip_suffix('*') {
        let fraction = fraction.trim();
        TrackMeasure::Fraction(if fraction.is_empty() {
            1.0
        } else {
            fraction.parse().ok()?
        })
    } else {
        TrackMeasure::Fixed(value.parse().ok()?)
    };
    let value = match measure {
        TrackMeasure::Fixed(value)
        | TrackMeasure::Percent(value)
        | TrackMeasure::Fraction(value) => value,
    };
    (value.is_finite() && value >= 0.0).then_some(measure)
}

pub fn format_track_measure(measure: TrackMeasure) -> String {
    fn number(value: f64) -> String {
        format!("{value:.4}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
    match measure {
        TrackMeasure::Fixed(value) => number(value),
        TrackMeasure::Percent(value) => format!("{}%", number(value)),
        TrackMeasure::Fraction(value) => format!("{}*", number(value)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TrackMeeting {
    #[default]
    Mullion,
    Stulp {
        bearer: TrackEnd,
    },
    Flush,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TrackEnd {
    #[default]
    Start,
    End,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GridTrack {
    pub measure: TrackMeasure,
    pub profile_id: Option<String>,
    pub meeting: TrackMeeting,
}

impl Default for GridTrack {
    fn default() -> Self {
        Self {
            measure: TrackMeasure::Fraction(1.0),
            profile_id: None,
            meeting: TrackMeeting::Mullion,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeRule {
    Suppress,
    Profile(Option<String>),
}

impl Default for EdgeRule {
    fn default() -> Self {
        Self::Profile(None)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum CellFill {
    #[default]
    Glass,
    Opaque,
    Door,
    Fixed,
    Nested(ConstructionGrid),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LeafOperation {
    Turn { side: TrackEnd },
    Tilt,
    TiltTurn { side: TrackEnd },
    Pivot { axis: TrackEnd },
    Slide { direction: TrackEnd },
    Fold { stack: TrackEnd },
}

impl Default for LeafOperation {
    fn default() -> Self {
        Self::left()
    }
}

impl LeafOperation {
    pub const fn left() -> Self {
        Self::Turn { side: TrackEnd::Start }
    }

    pub const fn right() -> Self {
        Self::Turn { side: TrackEnd::End }
    }

    pub fn bearing(self) -> TrackEnd {
        match self {
            Self::Turn { side }
            | Self::TiltTurn { side }
            | Self::Pivot { axis: side }
            | Self::Slide { direction: side }
            | Self::Fold { stack: side } => side,
            Self::Tilt => TrackEnd::Start,
        }
    }

    pub fn mirrored(self) -> Self {
        match self {
            Self::Turn { side } => Self::Turn {
                side: side.flipped(),
            },
            Self::Tilt => Self::Tilt,
            Self::TiltTurn { side } => Self::TiltTurn {
                side: side.flipped(),
            },
            Self::Pivot { axis } => Self::Pivot {
                axis: axis.flipped(),
            },
            Self::Slide { direction } => Self::Slide {
                direction: direction.flipped(),
            },
            Self::Fold { stack } => Self::Fold {
                stack: stack.flipped(),
            },
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Turn { side: TrackEnd::Start } => "TurnStart",
            Self::Turn { side: TrackEnd::End } => "TurnEnd",
            Self::Tilt => "Tilt",
            Self::TiltTurn { side: TrackEnd::Start } => "TiltTurnStart",
            Self::TiltTurn { side: TrackEnd::End } => "TiltTurnEnd",
            Self::Pivot { axis: TrackEnd::Start } => "PivotStart",
            Self::Pivot { axis: TrackEnd::End } => "PivotEnd",
            Self::Slide { direction: TrackEnd::Start } => "SlideStart",
            Self::Slide { direction: TrackEnd::End } => "SlideEnd",
            Self::Fold { stack: TrackEnd::Start } => "FoldStart",
            Self::Fold { stack: TrackEnd::End } => "FoldEnd",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "Right" | "right" | "R" | "r" | "TurnEnd" => Self::right(),
            "Tilt" => Self::Tilt,
            "TiltTurnStart" => Self::TiltTurn { side: TrackEnd::Start },
            "TiltTurnEnd" => Self::TiltTurn { side: TrackEnd::End },
            "PivotStart" => Self::Pivot { axis: TrackEnd::Start },
            "PivotEnd" => Self::Pivot { axis: TrackEnd::End },
            "SlideStart" => Self::Slide { direction: TrackEnd::Start },
            "SlideEnd" => Self::Slide { direction: TrackEnd::End },
            "FoldStart" => Self::Fold { stack: TrackEnd::Start },
            "FoldEnd" => Self::Fold { stack: TrackEnd::End },
            _ => Self::left(),
        }
    }
}

impl TrackEnd {
    pub const fn flipped(self) -> Self {
        match self {
            Self::Start => Self::End,
            Self::End => Self::Start,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GridCell {
    pub column: u32,
    pub row: u32,
    pub columns: u32,
    pub rows: u32,
    pub fill: CellFill,
    pub operation: Option<LeafOperation>,
    pub swing: Option<crate::modules::aec::engine::openings::SwingSide>,
    pub angle_deg: Option<f64>,
    pub sill: f64,
    pub left: Option<EdgeRule>,
    pub right: Option<EdgeRule>,
    pub bottom: Option<EdgeRule>,
    pub top: Option<EdgeRule>,
    pub bars: Vec<CellBar>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BarAxis {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CellBar {
    pub along: BarAxis,
    pub position: f64,
    pub face_width: f64,
    pub depth: f64,
}

impl Default for CellBar {
    fn default() -> Self {
        Self {
            along: BarAxis::Vertical,
            position: 0.5,
            face_width: 0.0,
            depth: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ConstructionGrid {
    pub columns: Vec<GridTrack>,
    pub rows: Vec<GridTrack>,
    pub cells: Vec<GridCell>,
    pub muntin_profile_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTrack {
    pub start: f64,
    pub end: f64,
    pub profile_id: Option<String>,
    pub meeting: TrackMeeting,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedCell {
    pub column: usize,
    pub row: usize,
    pub left: f64,
    pub right: f64,
    pub bottom: f64,
    pub top: f64,
    pub cell: GridCell,
    pub nested: Option<Box<ResolvedGrid>>,
    pub bars: Vec<ResolvedBar>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedBar {
    pub axis: BarAxis,
    pub position: f64,
    pub start: f64,
    pub end: f64,
    pub face_width: f64,
    pub depth: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedEdge {
    pub axis: BarAxis,
    pub position: f64,
    pub start: f64,
    pub end: f64,
    pub profile_id: Option<String>,
    pub meeting: TrackMeeting,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedGrid {
    pub columns: Vec<ResolvedTrack>,
    pub rows: Vec<ResolvedTrack>,
    pub cells: Vec<ResolvedCell>,
    pub edges: Vec<ResolvedEdge>,
    pub muntin_profile_id: Option<String>,
}

/// Writes a division template as axes and cells only. The name is not stored.
pub fn division_preset(name: &str, fill: CellFill) -> Option<ConstructionGrid> {
    match name {
        "double" => Some(paired_cells(
            fill,
            TrackMeeting::Stulp {
                bearer: TrackEnd::End,
            },
            LeafOperation::Turn {
                side: TrackEnd::Start,
            },
            LeafOperation::Turn {
                side: TrackEnd::End,
            },
        )),
        "fold" => Some(paired_cells(
            fill,
            TrackMeeting::Flush,
            LeafOperation::Fold {
                stack: TrackEnd::End,
            },
            LeafOperation::Fold {
                stack: TrackEnd::End,
            },
        )),
        "slide" => Some(paired_cells(
            fill,
            TrackMeeting::Flush,
            LeafOperation::Slide {
                direction: TrackEnd::End,
            },
            LeafOperation::Slide {
                direction: TrackEnd::End,
            },
        )),
        _ => None,
    }
}

fn paired_cells(
    fill: CellFill,
    meeting: TrackMeeting,
    left: LeafOperation,
    right: LeafOperation,
) -> ConstructionGrid {
    ConstructionGrid {
        columns: vec![
            GridTrack {
                measure: TrackMeasure::Fraction(1.0),
                profile_id: None,
                meeting,
            },
            GridTrack {
                measure: TrackMeasure::Fraction(1.0),
                ..GridTrack::default()
            },
        ],
        rows: vec![GridTrack::default()],
        cells: vec![
            preset_cell(0, fill.clone(), left),
            preset_cell(1, fill, right),
        ],
        muntin_profile_id: None,
    }
}

fn preset_cell(column: u32, fill: CellFill, operation: LeafOperation) -> GridCell {
    GridCell {
        column,
        row: 0,
        columns: 1,
        rows: 1,
        fill,
        operation: Some(operation),
        ..GridCell::default()
    }
}

/// The rough-opening threshold is omitted when door cells suppress the full bottom
/// and neither an axis nor a cell stores another rule.
pub fn omits_threshold(resolved: &ResolvedGrid, width: f64) -> bool {
    let width = finite_extent(width);
    if width <= 1e-9 {
        return false;
    }
    let mut spans: Vec<(f64, f64)> = resolved
        .cells
        .iter()
        .filter(|cell| {
            cell.bottom <= 1e-6 && matches!(cell.cell.bottom, Some(EdgeRule::Suppress))
        })
        .map(|cell| (cell.left, cell.right))
        .collect();
    if spans.is_empty() {
        return false;
    }
    spans.sort_by(|left, right| left.0.partial_cmp(&right.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut cursor = 0.0;
    for (start, end) in spans {
        if start > cursor + 1e-6 {
            return false;
        }
        cursor = cursor.max(end);
    }
    cursor + 1e-6 >= width
}

pub fn ensure_explicit_cells(grid: &mut ConstructionGrid) {
    if !grid.cells.is_empty() {
        return;
    }
    let columns = grid.columns.len().max(1) as u32;
    let rows = grid.rows.len().max(1) as u32;
    for row in 0..rows {
        for column in 0..columns {
            grid.cells.push(GridCell {
                column,
                row,
                columns: 1,
                rows: 1,
                ..GridCell::default()
            });
        }
    }
}

pub fn grid_nesting_depth(grid: &ConstructionGrid) -> u32 {
    1 + grid
        .cells
        .iter()
        .map(|cell| match &cell.fill {
            CellFill::Nested(child) => grid_nesting_depth(child),
            _ => 0,
        })
        .max()
        .unwrap_or(0)
}

pub fn merge_adjacent_cell(
    grid: &mut ConstructionGrid,
    column: u32,
    row: u32,
    across_columns: bool,
) -> bool {
    ensure_explicit_cells(grid);
    let Some(index) = grid
        .cells
        .iter()
        .position(|cell| cell.column == column && cell.row == row)
    else {
        return false;
    };
    let span_columns = grid.cells[index].columns.max(1);
    let span_rows = grid.cells[index].rows.max(1);
    let (target_column, target_row) = if across_columns {
        (column + span_columns, row)
    } else {
        (column, row + span_rows)
    };
    let Some(other) = grid.cells.iter().position(|cell| {
        cell.column == target_column && cell.row == target_row
    }) else {
        return false;
    };
    let compatible = if across_columns {
        grid.cells[other].rows.max(1) == span_rows
    } else {
        grid.cells[other].columns.max(1) == span_columns
    };
    if !compatible {
        return false;
    }
    if across_columns {
        grid.cells[index].columns = span_columns + grid.cells[other].columns.max(1);
    } else {
        grid.cells[index].rows = span_rows + grid.cells[other].rows.max(1);
    }
    grid.cells.remove(other);
    true
}

pub fn split_cell_span(grid: &mut ConstructionGrid, column: u32, row: u32) -> bool {
    ensure_explicit_cells(grid);
    let Some(index) = grid
        .cells
        .iter()
        .position(|cell| cell.column == column && cell.row == row)
    else {
        return false;
    };
    let span_columns = grid.cells[index].columns.max(1);
    let span_rows = grid.cells[index].rows.max(1);
    if span_columns == 1 && span_rows == 1 {
        return false;
    }
    let fill = grid.cells[index].fill.clone();
    grid.cells[index].columns = 1;
    grid.cells[index].rows = 1;
    for row_offset in 0..span_rows {
        for column_offset in 0..span_columns {
            if row_offset == 0 && column_offset == 0 {
                continue;
            }
            let column = column + column_offset;
            let row = row + row_offset;
            if grid
                .cells
                .iter()
                .any(|cell| cell.column == column && cell.row == row)
            {
                continue;
            }
            grid.cells.push(GridCell {
                column,
                row,
                columns: 1,
                rows: 1,
                fill: fill.clone(),
                ..GridCell::default()
            });
        }
    }
    true
}

pub fn nest_empty_grid(grid: &mut ConstructionGrid, column: u32, row: u32) -> bool {
    if grid_nesting_depth(grid) >= 3 {
        return false;
    }
    ensure_explicit_cells(grid);
    let Some(cell) = grid
        .cells
        .iter_mut()
        .find(|cell| cell.column == column && cell.row == row)
    else {
        return false;
    };
    if matches!(cell.fill, CellFill::Nested(_)) {
        return false;
    }
    cell.fill = CellFill::Nested(ConstructionGrid::default());
    true
}

pub fn grid_at_path<'a>(
    grid: &'a ConstructionGrid,
    mut width: f64,
    mut height: f64,
    path: &[(u32, u32)],
) -> Option<(&'a ConstructionGrid, f64, f64)> {
    let mut current = grid;
    for &(column, row) in path {
        let resolved = resolve_grid(current, width, height);
        let cell = resolved
            .cells
            .iter()
            .find(|cell| cell.column == column as usize && cell.row == row as usize)?;
        let child = current
            .cells
            .iter()
            .find(|cell| cell.column == column && cell.row == row)?;
        let CellFill::Nested(child_grid) = &child.fill else {
            return None;
        };
        width = (cell.right - cell.left).max(1e-6);
        height = (cell.top - cell.bottom).max(1e-6);
        current = child_grid;
    }
    Some((current, width, height))
}

pub fn with_grid_at_path<R>(
    grid: &mut ConstructionGrid,
    width: f64,
    height: f64,
    path: &[(u32, u32)],
    f: impl FnMut(&mut ConstructionGrid, f64, f64) -> R,
) -> Option<R> {
    fn walk<R>(
        grid: &mut ConstructionGrid,
        width: f64,
        height: f64,
        path: &[(u32, u32)],
        f: &mut impl FnMut(&mut ConstructionGrid, f64, f64) -> R,
    ) -> Option<R> {
        if path.is_empty() {
            return Some(f(grid, width, height));
        }
        let (column, row) = path[0];
        let resolved = resolve_grid(grid, width, height);
        let cell = resolved
            .cells
            .iter()
            .find(|cell| cell.column == column as usize && cell.row == row as usize)?;
        let child_width = (cell.right - cell.left).max(1e-6);
        let child_height = (cell.top - cell.bottom).max(1e-6);
        let child = grid
            .cells
            .iter_mut()
            .find(|cell| cell.column == column && cell.row == row)?;
        let CellFill::Nested(child_grid) = &mut child.fill else {
            return None;
        };
        walk(child_grid, child_width, child_height, &path[1..], f)
    }
    let mut f = f;
    walk(grid, width, height, path, &mut f)
}

pub fn insert_axis_division(
    grid: &mut ConstructionGrid,
    vertical: bool,
    position: f64,
    width: f64,
    height: f64,
) -> bool {
    let total = if vertical { width } else { height };
    if total <= 0.1 {
        return false;
    }
    let resolved = resolve_grid(grid, width, height);
    let tracks = if vertical {
        &resolved.columns
    } else {
        &resolved.rows
    };
    if tracks.len() >= 16 {
        return false;
    }
    let position = position.clamp(0.05, total - 0.05);
    let Some(index) = tracks.iter().position(|track| {
        position > track.start + 0.04 && position < track.end - 0.04
    }) else {
        return false;
    };
    ensure_explicit_cells(grid);
    let mut widths: Vec<f64> = tracks
        .iter()
        .map(|track| (track.end - track.start).max(1e-6))
        .collect();
    let start = tracks[index].start;
    let first = (position - start).max(1e-6);
    let second = (widths[index] - first).max(1e-6);
    widths[index] = first;
    widths.insert(index + 1, second);
    let mut previous = tracks.clone();
    let old_meeting = previous[index].meeting;
    let old_profile = previous[index].profile_id.clone();
    let inserted = previous[index].clone();
    previous.insert(index + 1, inserted);
    previous[index].meeting = TrackMeeting::Mullion;
    previous[index].profile_id = None;
    previous[index + 1].meeting = old_meeting;
    previous[index + 1].profile_id = old_profile;
    write_fraction_tracks(grid, vertical, &widths, &previous);
    shift_cells_after_insert(grid, vertical, index as u32 + 1);
    true
}

pub fn move_axis_division(
    grid: &mut ConstructionGrid,
    vertical: bool,
    index: usize,
    position: f64,
    width: f64,
    height: f64,
) -> bool {
    let resolved = resolve_grid(grid, width, height);
    let tracks = if vertical {
        &resolved.columns
    } else {
        &resolved.rows
    };
    if index + 1 >= tracks.len() {
        return false;
    }
    let start = tracks[index].start;
    let end = tracks[index + 1].end;
    let position = position.clamp(start + 0.04, (end - 0.04).max(start + 0.04));
    let mut widths: Vec<f64> = tracks
        .iter()
        .map(|track| (track.end - track.start).max(1e-6))
        .collect();
    let span = widths[index] + widths[index + 1];
    widths[index] = (position - start).max(1e-6);
    widths[index + 1] = (span - widths[index]).max(1e-6);
    write_fraction_tracks(grid, vertical, &widths, tracks);
    true
}

pub fn remove_last_axis_division(grid: &mut ConstructionGrid, vertical: bool) -> bool {
    let tracks = if vertical {
        &mut grid.columns
    } else {
        &mut grid.rows
    };
    if tracks.len() < 2 {
        return false;
    }
    let removed = tracks.len() - 1;
    tracks.pop();
    let removed = removed as u32;
    for cell in &mut grid.cells {
        let (start, span) = if vertical {
            (&mut cell.column, &mut cell.columns)
        } else {
            (&mut cell.row, &mut cell.rows)
        };
        if *start >= removed {
            *start = start.saturating_sub(1);
        } else if *start + (*span).max(1) > removed && *span > 1 {
            *span -= 1;
        }
    }
    true
}

fn write_fraction_tracks(
    grid: &mut ConstructionGrid,
    vertical: bool,
    widths: &[f64],
    previous: &[ResolvedTrack],
) {
    let tracks = if vertical {
        &mut grid.columns
    } else {
        &mut grid.rows
    };
    *tracks = widths
        .iter()
        .enumerate()
        .map(|(index, width)| {
            let previous = previous.get(index);
            GridTrack {
                measure: TrackMeasure::Fraction(*width),
                profile_id: previous.and_then(|track| track.profile_id.clone()),
                meeting: previous
                    .map(|track| track.meeting)
                    .unwrap_or(TrackMeeting::Mullion),
            }
        })
        .collect();
}

fn shift_cells_after_insert(grid: &mut ConstructionGrid, vertical: bool, boundary: u32) {
    let split = boundary.saturating_sub(1);
    let mut extras = Vec::new();
    for cell in &mut grid.cells {
        let (start, span) = if vertical {
            (&mut cell.column, &mut cell.columns)
        } else {
            (&mut cell.row, &mut cell.rows)
        };
        let covered = (*span).max(1);
        let end = *start + covered;
        if *start > split {
            *start += 1;
        } else if *start <= split && end > split && covered == 1 {
            extras.push((
                boundary,
                if vertical { cell.row } else { cell.column },
                cell.fill.clone(),
                cell.operation,
                cell.swing,
                cell.angle_deg,
                cell.sill,
            ));
        } else if *start <= split && end > split {
            *span = covered + 1;
        }
    }
    for (start, cross, fill, operation, swing, angle_deg, sill) in extras {
        grid.cells.push(if vertical {
            GridCell {
                column: start,
                row: cross,
                columns: 1,
                rows: 1,
                fill,
                operation,
                swing,
                angle_deg,
                sill,
                ..GridCell::default()
            }
        } else {
            GridCell {
                column: cross,
                row: start,
                columns: 1,
                rows: 1,
                fill,
                operation,
                swing,
                angle_deg,
                sill,
                ..GridCell::default()
            }
        });
    }
}

fn uncover_edges(edges: Vec<ResolvedEdge>, cells: &[ResolvedCell]) -> Vec<ResolvedEdge> {
    let mut uncovered = Vec::new();
    for edge in edges {
        let mut spans = vec![(edge.start, edge.end)];
        for cell in cells {
            let (cover_start, cover_end, covers) = match edge.axis {
                BarAxis::Vertical => (
                    cell.bottom,
                    cell.top,
                    cell.left + 1e-6 < edge.position && edge.position < cell.right - 1e-6,
                ),
                BarAxis::Horizontal => (
                    cell.left,
                    cell.right,
                    cell.bottom + 1e-6 < edge.position && edge.position < cell.top - 1e-6,
                ),
            };
            if covers {
                spans = subtract_spans(spans, cover_start, cover_end);
            }
        }
        for (start, end) in spans {
            if end - start > 1e-6 {
                let mut piece = edge.clone();
                piece.start = start;
                piece.end = end;
                uncovered.push(piece);
            }
        }
    }
    uncovered
}

fn subtract_spans(spans: Vec<(f64, f64)>, cover_start: f64, cover_end: f64) -> Vec<(f64, f64)> {
    let mut next = Vec::new();
    for (start, end) in spans {
        if cover_end <= start || cover_start >= end {
            next.push((start, end));
            continue;
        }
        if cover_start > start {
            next.push((start, cover_start));
        }
        if cover_end < end {
            next.push((cover_end, end));
        }
    }
    next
}

pub fn resolve_grid(grid: &ConstructionGrid, width: f64, height: f64) -> ResolvedGrid {
    let width = finite_extent(width);
    let height = finite_extent(height);
    let columns = resolve_tracks(&grid.columns, width);
    let rows = resolve_tracks(&grid.rows, height);
    let (columns, rows) = if columns.is_empty() || rows.is_empty() {
        let columns = if columns.is_empty() {
            vec![ResolvedTrack {
                start: 0.0,
                end: width.max(0.0),
                profile_id: None,
                meeting: TrackMeeting::Mullion,
            }]
        } else {
            columns
        };
        let rows = if rows.is_empty() {
            vec![ResolvedTrack {
                start: 0.0,
                end: height.max(0.0),
                profile_id: None,
                meeting: TrackMeeting::Mullion,
            }]
        } else {
            rows
        };
        (columns, rows)
    } else {
        (columns, rows)
    };

    let mut cells = Vec::new();
    let mut source_cells = Vec::new();
    if grid.cells.is_empty() {
        let cell = GridCell {
            columns: columns.len() as u32,
            rows: rows.len() as u32,
            ..GridCell::default()
        };
        source_cells.push(cell.clone());
        cells.push(resolve_cell(&cell, &columns, &rows));
    } else {
        for (index, cell) in grid.cells.iter().enumerate() {
            if let Some(resolved) =
                resolve_cell_if_visible(index, &grid.cells, cell, &columns, &rows)
            {
                source_cells.push(cell.clone());
                cells.push(resolved);
            }
        }
    }

    let mut edges = Vec::new();
    append_track_edges(
        &mut edges,
        &columns,
        &source_cells,
        BarAxis::Vertical,
        rows.last().map_or(height, |track| track.end),
        &None,
    );
    append_track_edges(
        &mut edges,
        &rows,
        &source_cells,
        BarAxis::Horizontal,
        columns.last().map_or(width, |track| track.end),
        &None,
    );
    if !grid.cells.is_empty() {
        edges = uncover_edges(edges, &cells);
    }

    ResolvedGrid {
        columns,
        rows,
        cells,
        edges,
        muntin_profile_id: grid.muntin_profile_id.clone(),
    }
}

fn finite_extent(extent: f64) -> f64 {
    if extent.is_finite() {
        extent.max(0.0)
    } else {
        0.0
    }
}

fn append_track_edges(
    edges: &mut Vec<ResolvedEdge>,
    tracks: &[ResolvedTrack],
    cells: &[GridCell],
    axis: BarAxis,
    span: f64,
    default_profile_id: &Option<String>,
) {
    for (index, track) in tracks
        .iter()
        .enumerate()
        .take(tracks.len().saturating_sub(1))
    {
        let next = &tracks[index + 1];
        let mut meeting = track.meeting;
        if matches!(meeting, TrackMeeting::Stulp { .. }) {
            let boundary = index as u32 + 1;
            let has_left = cells.iter().any(|cell| match axis {
                BarAxis::Vertical => cell.column.saturating_add(cell.columns.max(1)) == boundary,
                BarAxis::Horizontal => cell.row.saturating_add(cell.rows.max(1)) == boundary,
            });
            let has_right = cells.iter().any(|cell| match axis {
                BarAxis::Vertical => cell.column == boundary,
                BarAxis::Horizontal => cell.row == boundary,
            });
            if !has_left || !has_right {
                meeting = TrackMeeting::Mullion;
            }
        }
        let boundary = index as u32 + 1;
        let touching_cells = cells.iter().filter_map(|cell| {
            let (cell_start, cell_end, edge_rule) = match axis {
                BarAxis::Vertical => (
                    cell.column,
                    cell.column.saturating_add(cell.columns.max(1)),
                    if cell.column == boundary {
                        cell.left.as_ref()
                    } else if cell.column.saturating_add(cell.columns.max(1)) == boundary {
                        cell.right.as_ref()
                    } else {
                        None
                    },
                ),
                BarAxis::Horizontal => (
                    cell.row,
                    cell.row.saturating_add(cell.rows.max(1)),
                    if cell.row == boundary {
                        cell.bottom.as_ref()
                    } else if cell.row.saturating_add(cell.rows.max(1)) == boundary {
                        cell.top.as_ref()
                    } else {
                        None
                    },
                ),
            };
            (cell_start < boundary && cell_end == boundary
                || cell_start == boundary && cell_end > boundary)
                .then_some(edge_rule)
                .flatten()
        });
        let rules: Vec<_> = touching_cells.collect();
        let suppressed = rules.iter().any(|rule| matches!(rule, EdgeRule::Suppress));
        if suppressed {
            meeting = TrackMeeting::Flush;
        }
        let edge_profile = if meeting == TrackMeeting::Flush {
            None
        } else {
            rules
                .iter()
                .find_map(|rule| match rule {
                    EdgeRule::Profile(profile_id) => profile_id.clone(),
                    EdgeRule::Suppress => None,
                })
                .or_else(|| track.profile_id.clone())
                .or_else(|| next.profile_id.clone())
                .or_else(|| default_profile_id.clone())
        };
        edges.push(ResolvedEdge {
            axis,
            position: (track.end + next.start) * 0.5,
            start: 0.0,
            end: span,
            profile_id: edge_profile,
            meeting,
        });
    }
}

fn resolve_tracks(tracks: &[GridTrack], total: f64) -> Vec<ResolvedTrack> {
    if tracks.is_empty() {
        return Vec::new();
    }
    let fixed_sum: f64 = tracks
        .iter()
        .filter_map(|track| match track.measure {
            TrackMeasure::Fixed(value) if value.is_finite() => Some(value.max(0.0)),
            TrackMeasure::Percent(value) if value.is_finite() => {
                Some(total.max(0.0) * value.max(0.0) / 100.0)
            }
            TrackMeasure::Percent(_) => None,
            TrackMeasure::Fraction(_) => None,
            TrackMeasure::Fixed(_) => None,
        })
        .sum();
    let fixed_scale = if fixed_sum > total && fixed_sum > 0.0 {
        total / fixed_sum
    } else {
        1.0
    };
    let remaining = (total - fixed_sum.min(total)).max(0.0);
    let fraction_max: f64 = tracks
        .iter()
        .filter_map(|track| match track.measure {
            TrackMeasure::Fraction(value) if value.is_finite() => Some(value.max(0.0)),
            TrackMeasure::Fixed(_) => None,
            TrackMeasure::Percent(_) => None,
            TrackMeasure::Fraction(_) => None,
        })
        .fold(0.0_f64, f64::max);
    let fraction_sum: f64 = if fraction_max > 0.0 {
        tracks
            .iter()
            .filter_map(|track| match track.measure {
                TrackMeasure::Fraction(value) if value.is_finite() => {
                    Some(value.max(0.0) / fraction_max)
                }
                _ => None,
            })
            .sum()
    } else {
        0.0
    };
    let mut cursor = 0.0;
    tracks
        .iter()
        .map(|track| {
            let extent = match track.measure {
                TrackMeasure::Fixed(value) if value.is_finite() => value.max(0.0) * fixed_scale,
                TrackMeasure::Percent(value) if value.is_finite() => {
                    total.max(0.0) * value.max(0.0) / 100.0 * fixed_scale
                }
                TrackMeasure::Fraction(value)
                    if value.is_finite() && fraction_max > 0.0 && fraction_sum > 0.0 =>
                {
                    remaining * (value.max(0.0) / fraction_max) / fraction_sum
                }
                _ => 0.0,
            };
            let start = cursor;
            cursor += extent;
            ResolvedTrack {
                start,
                end: cursor,
                profile_id: track.profile_id.clone(),
                meeting: track.meeting,
            }
        })
        .collect()
}

fn resolve_cell_if_visible(
    index: usize,
    cells: &[GridCell],
    cell: &GridCell,
    columns: &[ResolvedTrack],
    rows: &[ResolvedTrack],
) -> Option<ResolvedCell> {
    let col = cell.column as usize;
    let row = cell.row as usize;
    let col_end = col.saturating_add(cell.columns.max(1) as usize);
    let row_end = row.saturating_add(cell.rows.max(1) as usize);
    if col_end > columns.len() || row_end > rows.len() {
        return None;
    }
    let covered = cells.iter().enumerate().any(|(other_index, other)| {
        if other_index == index {
            return false;
        }
        let other_col = other.column as usize;
        let other_row = other.row as usize;
        let other_col_end = other_col.saturating_add(other.columns.max(1) as usize);
        let other_row_end = other_row.saturating_add(other.rows.max(1) as usize);
        let fully_contains = other_col <= col
            && other_row <= row
            && other_col_end >= col_end
            && other_row_end >= row_end;
        let strictly_larger = other_col < col
            || other_row < row
            || other_col_end > col_end
            || other_row_end > row_end;
        fully_contains && (strictly_larger || other_index < index)
    });
    if covered {
        return None;
    }
    Some(resolve_cell(cell, columns, rows))
}

fn resolve_cell(
    cell: &GridCell,
    columns: &[ResolvedTrack],
    rows: &[ResolvedTrack],
) -> ResolvedCell {
    let column = cell.column as usize;
    let row = cell.row as usize;
    let column_end = (column + cell.columns.max(1) as usize).min(columns.len());
    let row_end = (row + cell.rows.max(1) as usize).min(rows.len());
    let left = columns.get(column).map_or(0.0, |track| track.start);
    let right = columns
        .get(column_end.saturating_sub(1))
        .map_or(left, |track| track.end);
    let bottom = rows.get(row).map_or(0.0, |track| track.start);
    let top = rows
        .get(row_end.saturating_sub(1))
        .map_or(bottom, |track| track.end);
    let nested = match &cell.fill {
        CellFill::Nested(grid) => Some(Box::new(resolve_grid(grid, right - left, top - bottom))),
        _ => None,
    };
    let mut cell = cell.clone();
    if matches!(cell.fill, CellFill::Door) && cell.bottom.is_none() {
        let axis_profile = rows.get(row).and_then(|track| track.profile_id.clone());
        if axis_profile.is_none() {
            cell.bottom = Some(EdgeRule::Suppress);
        }
    }
    let bars = cell
        .bars
        .iter()
        .filter_map(|bar| {
            if !bar.position.is_finite() {
                return None;
            }
            let (start, end) = match bar.along {
                BarAxis::Vertical => (bottom, top),
                BarAxis::Horizontal => (left, right),
            };
            let (cross_start, cross_end) = match bar.along {
                BarAxis::Vertical => (left, right),
                BarAxis::Horizontal => (bottom, top),
            };
            let position = cross_start + (cross_end - cross_start) * bar.position.clamp(0.0, 1.0);
            Some(ResolvedBar {
                axis: bar.along,
                position,
                start,
                end,
                face_width: finite_extent(bar.face_width),
                depth: finite_extent(bar.depth),
            })
        })
        .collect();
    ResolvedCell {
        column,
        row,
        left,
        right,
        bottom,
        top,
        cell,
        nested,
        bars,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fraction() -> GridTrack {
        GridTrack {
            measure: TrackMeasure::Fraction(1.0),
            ..GridTrack::default()
        }
    }

    #[test]
    fn fixed_track_and_fractions_split_remaining_width() {
        let grid = ConstructionGrid {
            columns: vec![
                GridTrack {
                    measure: TrackMeasure::Fixed(0.4),
                    ..GridTrack::default()
                },
                fraction(),
                fraction(),
            ],
            ..ConstructionGrid::default()
        };
        let resolved = resolve_grid(&grid, 1.2, 1.0);
        let extents: Vec<_> = resolved
            .columns
            .iter()
            .map(|track| track.end - track.start)
            .collect();
        assert!(extents.iter().all(|extent| (extent - 0.4).abs() < 1e-9));
    }

    #[test]
    fn track_measure_text_accepts_fixed_percent_and_fraction_values() {
        assert_eq!(parse_track_measure("0.60"), Some(TrackMeasure::Fixed(0.6)));
        assert_eq!(parse_track_measure("50%"), Some(TrackMeasure::Percent(50.0)));
        assert_eq!(parse_track_measure("1*"), Some(TrackMeasure::Fraction(1.0)));
        assert_eq!(format_track_measure(TrackMeasure::Fixed(0.6)), "0.6");
        assert_eq!(format_track_measure(TrackMeasure::Percent(50.0)), "50%");
        assert_eq!(format_track_measure(TrackMeasure::Fraction(1.0)), "1*");

        let grid = ConstructionGrid {
            columns: vec![
                GridTrack {
                    measure: TrackMeasure::Percent(50.0),
                    ..GridTrack::default()
                },
                GridTrack {
                    measure: TrackMeasure::Fraction(2.0),
                    ..GridTrack::default()
                },
                GridTrack {
                    measure: TrackMeasure::Fraction(1.0),
                    ..GridTrack::default()
                },
            ],
            ..ConstructionGrid::default()
        };
        let resolved = resolve_grid(&grid, 1.2, 1.0);
        let extents: Vec<_> = resolved
            .columns
            .iter()
            .map(|track| track.end - track.start)
            .collect();
        assert!((extents[0] - 0.6).abs() < 1e-9);
        assert!((extents[1] - 0.4).abs() < 1e-9);
        assert!((extents[2] - 0.2).abs() < 1e-9);
    }

    #[test]
    fn legacy_grid_cells_default_new_leaf_overrides_to_none() {
        let cell: GridCell = serde_json::from_str(
            r#"{"columns":1}"#,
        )
        .expect("legacy cell without swing or angle fields");
        assert_eq!(cell.swing, None);
        assert_eq!(cell.angle_deg, None);
    }

    #[test]
    fn inserting_a_divider_defaults_to_mullion_and_preserves_the_next_meeting() {
        let mut grid = ConstructionGrid {
            columns: vec![
                GridTrack {
                    meeting: TrackMeeting::Stulp {
                        bearer: TrackEnd::End,
                    },
                    ..fraction()
                },
                fraction(),
            ],
            cells: vec![
                GridCell {
                    column: 0,
                    swing: Some(crate::modules::aec::engine::openings::SwingSide::Interior),
                    angle_deg: Some(45.0),
                    ..GridCell::default()
                },
                GridCell {
                    column: 1,
                    ..GridCell::default()
                },
            ],
            ..ConstructionGrid::default()
        };
        assert!(insert_axis_division(&mut grid, true, 0.3, 1.0, 1.0));
        assert_eq!(grid.columns.len(), 3);
        assert_eq!(grid.columns[0].meeting, TrackMeeting::Mullion);
        assert_eq!(
            grid.columns[1].meeting,
            TrackMeeting::Stulp {
                bearer: TrackEnd::End
            }
        );
        assert!(grid.cells.iter().any(|cell| {
            cell.column == 1
                && cell.swing == Some(crate::modules::aec::engine::openings::SwingSide::Interior)
                && cell.angle_deg == Some(45.0)
        }));
    }

    #[test]
    fn fixed_tracks_are_scaled_when_they_exceed_the_opening() {
        let grid = ConstructionGrid {
            columns: vec![
                GridTrack {
                    measure: TrackMeasure::Fixed(2.0),
                    ..GridTrack::default()
                },
                GridTrack {
                    measure: TrackMeasure::Fixed(1.0),
                    ..GridTrack::default()
                },
            ],
            ..ConstructionGrid::default()
        };
        let resolved = resolve_grid(&grid, 1.5, 1.0);
        assert_eq!(resolved.columns.last().unwrap().end, 1.5);
    }

    #[test]
    fn empty_grid_resolves_to_one_cell_over_the_opening() {
        let resolved = resolve_grid(&ConstructionGrid::default(), 1.2, 0.9);
        assert_eq!(resolved.cells.len(), 1);
        assert_eq!(resolved.cells[0].right, 1.2);
        assert_eq!(resolved.cells[0].top, 0.9);
    }

    #[test]
    fn nested_grid_is_limited_to_its_parent_cell() {
        let grid = ConstructionGrid {
            columns: vec![fraction(), fraction()],
            cells: vec![GridCell {
                column: 1,
                columns: 1,
                rows: 1,
                fill: CellFill::Nested(ConstructionGrid {
                    columns: vec![fraction(), fraction()],
                    ..ConstructionGrid::default()
                }),
                ..GridCell::default()
            }],
            ..ConstructionGrid::default()
        };
        let resolved = resolve_grid(&grid, 2.0, 1.0);
        let nested = resolved.cells[0].nested.as_ref().unwrap();
        assert_eq!(nested.columns[0].end, 0.5);
        assert_eq!(nested.columns[1].end, 1.0);
    }

    #[test]
    fn cell_fully_covered_by_spanning_cell_is_discarded() {
        let grid = ConstructionGrid {
            columns: vec![fraction(), fraction()],
            cells: vec![
                GridCell {
                    columns: 2,
                    rows: 1,
                    ..GridCell::default()
                },
                GridCell {
                    column: 1,
                    columns: 1,
                    rows: 1,
                    ..GridCell::default()
                },
            ],
            ..ConstructionGrid::default()
        };
        let resolved = resolve_grid(&grid, 2.0, 1.0);
        assert_eq!(resolved.cells.len(), 1);
        assert_eq!(resolved.cells[0].right, 2.0);
    }

    #[test]
    fn stulp_without_two_adjacent_cells_falls_back_to_mullion() {
        let grid = ConstructionGrid {
            columns: vec![
                GridTrack {
                    meeting: TrackMeeting::Stulp {
                        bearer: TrackEnd::Start,
                    },
                    ..fraction()
                },
                fraction(),
            ],
            ..ConstructionGrid::default()
        };
        let resolved = resolve_grid(&grid, 1.0, 1.0);
        assert_eq!(resolved.edges[0].meeting, TrackMeeting::Mullion);
    }

    #[test]
    fn door_cell_suppresses_the_bottom_unless_an_axis_or_cell_rule_wins() {
        let door = GridCell {
            fill: CellFill::Door,
            ..GridCell::default()
        };
        let resolved = resolve_grid(
            &ConstructionGrid {
                cells: vec![door],
                ..ConstructionGrid::default()
            },
            1.0,
            2.0,
        );
        assert_eq!(resolved.cells[0].cell.bottom, Some(EdgeRule::Suppress));
        assert!(omits_threshold(&resolved, 1.0));

        let kept = resolve_grid(
            &ConstructionGrid {
                rows: vec![GridTrack {
                    profile_id: Some("sill".into()),
                    ..GridTrack::default()
                }],
                cells: vec![GridCell {
                    fill: CellFill::Door,
                    ..GridCell::default()
                }],
                ..ConstructionGrid::default()
            },
            1.0,
            2.0,
        );
        assert!(kept.cells[0].cell.bottom.is_none());
        assert!(!omits_threshold(&kept, 1.0));
    }

    #[test]
    fn muntin_profile_stays_on_the_grid_and_not_on_an_axis() {
        let resolved = resolve_grid(
            &ConstructionGrid {
                columns: vec![
                    GridTrack {
                        measure: TrackMeasure::Fraction(1.0),
                        ..GridTrack::default()
                    },
                    GridTrack {
                        measure: TrackMeasure::Fraction(1.0),
                        ..GridTrack::default()
                    },
                ],
                muntin_profile_id: Some("sash-bar".into()),
                ..ConstructionGrid::default()
            },
            1.2,
            1.0,
        );
        assert_eq!(resolved.muntin_profile_id.as_deref(), Some("sash-bar"));
        assert!(resolved
            .edges
            .iter()
            .all(|edge| edge.profile_id.is_none()));
    }

    #[test]
    fn double_preset_writes_axes_and_cells_without_a_flag() {
        let grid = division_preset("double", CellFill::Glass).expect("preset");
        assert_eq!(grid.columns.len(), 2);
        assert!(grid.columns.iter().all(|track| {
            matches!(track.measure, TrackMeasure::Fraction(value) if (value - 1.0).abs() < 1e-9)
        }));
        assert_eq!(
            grid.columns[0].meeting,
            TrackMeeting::Stulp {
                bearer: TrackEnd::End
            }
        );
        assert_eq!(grid.cells.len(), 2);
        assert_eq!(
            grid.cells[0].operation,
            Some(LeafOperation::Turn {
                side: TrackEnd::Start
            })
        );
        assert_eq!(
            grid.cells[1].operation,
            Some(LeafOperation::Turn {
                side: TrackEnd::End
            })
        );
        let encoded = serde_json::to_string(&grid).expect("encode");
        assert!(!encoded.contains("preset"));
    }

    #[test]
    fn fold_and_slide_presets_write_flush_cells() {
        for name in ["fold", "slide"] {
            let grid = division_preset(name, CellFill::Glass).expect(name);
            assert_eq!(grid.columns[0].meeting, TrackMeeting::Flush);
            assert_eq!(grid.cells.len(), 2);
        }
    }

    #[test]
    fn spanning_cell_hides_the_covered_post() {
        let grid = ConstructionGrid {
            columns: vec![fraction(), fraction()],
            cells: vec![GridCell {
                columns: 2,
                rows: 1,
                ..GridCell::default()
            }],
            ..ConstructionGrid::default()
        };
        let resolved = resolve_grid(&grid, 1.2, 1.0);
        assert_eq!(resolved.cells.len(), 1);
        assert!(resolved.edges.iter().all(|edge| {
            edge.axis != BarAxis::Vertical || (edge.position - 0.6).abs() > 1e-6
        }));
    }

    #[test]
    fn cell_edge_can_suppress_an_internal_bar() {
        let grid = ConstructionGrid {
            columns: vec![fraction(), fraction()],
            cells: vec![
                GridCell {
                    columns: 1,
                    rows: 1,
                    right: Some(EdgeRule::Suppress),
                    ..GridCell::default()
                },
                GridCell {
                    column: 1,
                    columns: 1,
                    rows: 1,
                    ..GridCell::default()
                },
            ],
            ..ConstructionGrid::default()
        };
        let resolved = resolve_grid(&grid, 1.0, 1.0);
        assert_eq!(resolved.edges[0].meeting, TrackMeeting::Flush);
    }
}
