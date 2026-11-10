use iced::mouse;
use iced::widget::canvas as canvas_widget;
use iced::widget::canvas::{self, Frame, Path, Program, Stroke};
use iced::{Color, Element, Fill, Point, Rectangle, Size, Theme};

use crate::app::Message;
use crate::modules::aec::engine::construction_grid::{
    resolve_grid, CellFill, ConstructionGrid, LeafOperation, TrackEnd,
};
use crate::modules::aec::engine::openings::OpeningKind;
use crate::modules::aec::message::AecMessage;

#[derive(Clone, Copy)]
enum DividerAxis {
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy)]
struct DividerDrag {
    axis: DividerAxis,
    index: usize,
}

struct OpeningDivisionCanvas {
    grid: ConstructionGrid,
    width: f64,
    height: f64,
    kind: OpeningKind,
    selected_cell: Option<usize>,
}

pub fn view<'a>(
    grid: &ConstructionGrid,
    width: f64,
    height: f64,
    kind: OpeningKind,
    selected_cell: Option<usize>,
) -> Element<'a, Message> {
    canvas_widget(OpeningDivisionCanvas {
        grid: grid.clone(),
        width: width.max(1e-6),
        height: height.max(1e-6),
        kind,
        selected_cell,
    })
    .width(Fill)
    .height(210)
    .into()
}

impl OpeningDivisionCanvas {
    fn transform(&self, bounds: Rectangle) -> (f32, f32, f32) {
        let pad = 14.0;
        let scale = ((bounds.width - 2.0 * pad) / self.width as f32)
            .min((bounds.height - 2.0 * pad) / self.height as f32)
            .max(1e-4);
        let origin_x = (bounds.width - self.width as f32 * scale) * 0.5;
        let origin_y = (bounds.height - self.height as f32 * scale) * 0.5;
        (scale, origin_x, origin_y)
    }

    fn to_world(&self, bounds: Rectangle, point: Point) -> (f64, f64) {
        let (scale, origin_x, origin_y) = self.transform(bounds);
        let x = (point.x - origin_x) as f64 / scale as f64;
        let z = self.height - (point.y - origin_y) as f64 / scale as f64;
        (x, z)
    }

    fn resolved(&self) -> crate::modules::aec::engine::construction_grid::ResolvedGrid {
        resolve_grid(&self.grid, self.width, self.height)
    }

    fn vertical_split_screen_positions(&self, bounds: Rectangle) -> Vec<f32> {
        let resolved = self.resolved();
        if resolved.columns.len() < 2 {
            return Vec::new();
        }
        let (scale, origin_x, _) = self.transform(bounds);
        resolved
            .columns
            .iter()
            .take(resolved.columns.len() - 1)
            .map(|track| origin_x + track.end as f32 * scale)
            .collect()
    }

    fn horizontal_split_screen_positions(&self, bounds: Rectangle) -> Vec<f32> {
        let resolved = self.resolved();
        if resolved.rows.len() < 2 {
            return Vec::new();
        }
        let (scale, _, origin_y) = self.transform(bounds);
        resolved
            .rows
            .iter()
            .take(resolved.rows.len() - 1)
            .map(|track| origin_y + (self.height - track.end) as f32 * scale)
            .collect()
    }

    fn cell_at(&self, x: f64, z: f64) -> Option<(usize, CellFill, Option<LeafOperation>)> {
        self.resolved()
            .cells
            .into_iter()
            .enumerate()
            .find(|(_, cell)| x >= cell.left && x <= cell.right && z >= cell.bottom && z <= cell.top)
            .map(|(index, cell)| (index, cell.cell.fill, cell.cell.operation))
    }
}

impl Program<Message> for OpeningDivisionCanvas {
    type State = Option<DividerDrag>;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if let Some(drag) = *state {
            match event {
                canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                    let point = cursor.position_in(bounds)?;
                    let (x, z) = self.to_world(bounds, point);
                    let (axis, position) = match drag.axis {
                        DividerAxis::Vertical => ("vertical", x),
                        DividerAxis::Horizontal => ("horizontal", z),
                    };
                    return Some(
                        canvas::Action::publish(Message::Aec(
                            AecMessage::AecOpeningStyleManagerGridDivider(
                                axis.to_string(),
                                drag.index,
                                format!("{position:.6}"),
                            ),
                        ))
                        .and_capture(),
                    );
                }
                canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    *state = None;
                    return Some(canvas::Action::capture());
                }
                _ => return None,
            }
        }

        let canvas::Event::Mouse(mouse::Event::ButtonPressed(button)) = event else {
            return None;
        };
        if !matches!(button, mouse::Button::Left | mouse::Button::Right) {
            return None;
        }
        let point = cursor.position_in(bounds)?;
        let (scale, origin_x, origin_y) = self.transform(bounds);
        let (x, z) = self.to_world(bounds, point);
        let tolerance = 9.0;

        if *button == mouse::Button::Left {
            if let Some((index, _)) = self
                .vertical_split_screen_positions(bounds)
                .iter()
                .enumerate()
                .map(|(index, split)| (index, (point.x - split).abs()))
                .filter(|(_, distance)| *distance <= tolerance)
                .min_by(|a, b| a.1.total_cmp(&b.1))
            {
                *state = Some(DividerDrag {
                    axis: DividerAxis::Vertical,
                    index,
                });
                return Some(canvas::Action::capture());
            }
            if let Some((index, _)) = self
                .horizontal_split_screen_positions(bounds)
                .iter()
                .enumerate()
                .map(|(index, split)| (index, (point.y - split).abs()))
                .filter(|(_, distance)| *distance <= tolerance)
                .min_by(|a, b| a.1.total_cmp(&b.1))
            {
                *state = Some(DividerDrag {
                    axis: DividerAxis::Horizontal,
                    index,
                });
                return Some(canvas::Action::capture());
            }

            let canvas_left = origin_x;
            let canvas_right = origin_x + self.width as f32 * scale;
            let canvas_top = origin_y;
            let canvas_bottom = origin_y + self.height as f32 * scale;
            if (point.y - canvas_top).abs() <= tolerance
                || (point.y - canvas_bottom).abs() <= tolerance
            {
                return Some(
                    canvas::Action::publish(Message::Aec(AecMessage::AecOpeningStyleManagerGridInsert(
                        "vertical".to_string(),
                        format!("{x:.6}"),
                    )))
                    .and_capture(),
                );
            }
            if (point.x - canvas_left).abs() <= tolerance
                || (point.x - canvas_right).abs() <= tolerance
            {
                return Some(
                    canvas::Action::publish(Message::Aec(AecMessage::AecOpeningStyleManagerGridInsert(
                        "horizontal".to_string(),
                        format!("{z:.6}"),
                    )))
                    .and_capture(),
                );
            }
        }

        let (index, fill, hinge) = self.cell_at(x, z)?;
        let message = if *button == mouse::Button::Left {
            if matches!(fill, CellFill::Nested(_)) {
                AecMessage::AecOpeningStyleManagerGridEnter(index)
            } else {
                AecMessage::AecOpeningStyleManagerCellSelect(index)
            }
        } else if fill_permits_hinge(&fill) {
            let next = match hinge.map(|operation| operation.bearing()) {
                Some(TrackEnd::Start) => "Right",
                _ => "Left",
            };
            AecMessage::AecOpeningStyleManagerCellHingeChanged(index, next.to_string())
        } else {
            return None;
        };
        Some(canvas::Action::publish(Message::Aec(message)).and_capture())
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.is_some() {
            mouse::Interaction::Grabbing
        } else {
            mouse::Interaction::Pointer
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (scale, origin_x, origin_y) = self.transform(bounds);
        let resolved = self.resolved();
        if resolved.cells.is_empty() {
            let rect = Path::rectangle(
                Point::new(origin_x, origin_y),
                Size::new(self.width as f32 * scale, self.height as f32 * scale),
            );
            frame.stroke(
                &rect,
                Stroke::default()
                    .with_width(1.5)
                    .with_color(super::opening::opening_grid_preview_color()),
            );
        } else {
            for (index, cell) in resolved.cells.iter().enumerate() {
                let x = origin_x + cell.left as f32 * scale;
                let y = origin_y + (self.height - cell.top) as f32 * scale;
                let size = Size::new(
                    (cell.right - cell.left) as f32 * scale,
                    (cell.top - cell.bottom) as f32 * scale,
                );
                let rect = Path::rectangle(Point::new(x, y), size);
                frame.fill(&rect, cell_color(&cell.cell.fill, theme));
                frame.stroke(
                    &rect,
                    Stroke::default()
                        .with_width(1.5)
                        .with_color(super::opening::opening_grid_preview_color()),
                );
                if self.selected_cell == Some(index) {
                    frame.stroke(
                        &rect,
                        Stroke::default()
                            .with_width(2.5)
                            .with_color(theme.palette().primary.base.color),
                    );
                }
                if let Some(hinge) = cell.cell.operation {
                    let marker_x = match hinge.bearing() {
                        TrackEnd::Start => x + 3.0,
                        TrackEnd::End => x + size.width - 3.0,
                    };
                    frame.fill_rectangle(
                        Point::new(marker_x, y + 3.0),
                        Size::new(2.0, (size.height - 6.0).max(1.0)),
                        theme.palette().primary.base.color,
                    );
                }
            }
        }
        let _ = self.kind;
        vec![frame.into_geometry()]
    }
}

fn fill_permits_hinge(fill: &CellFill) -> bool {
    matches!(fill, CellFill::Glass | CellFill::Door)
}

fn cell_color(fill: &CellFill, theme: &Theme) -> Color {
    match fill {
        CellFill::Glass => Color::from_rgba(0.30, 0.68, 0.86, 0.30),
        CellFill::Opaque => theme.palette().background.weak.color.scale_alpha(0.5),
        CellFill::Door => Color::from_rgba(0.72, 0.56, 0.34, 0.35),
        CellFill::Fixed => Color::from_rgba(0.40, 0.72, 0.48, 0.32),
        CellFill::Nested(_) => Color::from_rgba(0.55, 0.55, 0.62, 0.35),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::construction_grid::{insert_axis_division, move_axis_division};

    #[test]
    fn inserting_and_dragging_a_post_keeps_two_columns() {
        let mut grid = ConstructionGrid::default();
        assert!(insert_axis_division(&mut grid, true, 0.6, 1.2, 1.4));
        let resolved = resolve_grid(&grid, 1.2, 1.4);
        assert_eq!(resolved.columns.len(), 2);
        assert!(move_axis_division(&mut grid, true, 0, 0.72, 1.2, 1.4));
        let resolved = resolve_grid(&grid, 1.2, 1.4);
        assert!((resolved.columns[0].end - 0.72).abs() < 1e-6);
    }

    #[test]
    fn cell_at_uses_resolved_bounds() {
        let mut grid = ConstructionGrid::default();
        insert_axis_division(&mut grid, true, 0.6, 1.2, 1.4);
        let canvas = OpeningDivisionCanvas {
            grid,
            width: 1.2,
            height: 1.4,
            kind: OpeningKind::Window,
            selected_cell: None,
        };
        assert_eq!(canvas.cell_at(0.9, 0.7).map(|cell| cell.0), Some(1));
    }
}
