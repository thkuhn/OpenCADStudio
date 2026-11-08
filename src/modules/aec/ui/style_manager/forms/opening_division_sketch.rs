use iced::mouse;
use iced::widget::canvas as canvas_widget;
use iced::widget::canvas::{self, Frame, Path, Program, Stroke};
use iced::{Color, Element, Fill, Point, Rectangle, Size, Theme};

use crate::app::Message;
use crate::modules::aec::engine::opening_style::{DivisionGrid, OpeningCellKind};
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
    grid: DivisionGrid,
    width: f64,
    height: f64,
    kind: OpeningKind,
    selected_cell: Option<usize>,
}

pub fn view<'a>(
    grid: &DivisionGrid,
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
        let x = (point.x - origin_x) as f64 / scale as f64 - self.width * 0.5;
        let z = self.height - (point.y - origin_y) as f64 / scale as f64;
        (x, z)
    }

    fn vertical_split_screen_positions(&self, bounds: Rectangle) -> Vec<f32> {
        if self.grid.vertical.len() < 2 {
            return Vec::new();
        }
        let (scale, origin_x, _) = self.transform(bounds);
        let mut positions = Vec::new();
        let mut cumulative = 0.0;
        let total: f64 = self.grid.vertical.iter().sum();
        if !total.is_finite() || total <= 0.0 {
            return positions;
        }
        for weight in self.grid.vertical.iter().take(self.grid.vertical.len() - 1) {
            cumulative += *weight;
            positions.push(origin_x + ((cumulative / total - 0.5) * self.width) as f32 * scale);
        }
        positions
    }

    fn horizontal_split_screen_positions(&self, bounds: Rectangle) -> Vec<f32> {
        let (scale, _, origin_y) = self.transform(bounds);
        self.grid
            .horizontal
            .iter()
            .map(|height| origin_y + ((self.height - height) as f32 * scale))
            .collect()
    }

    fn cell_at(
        &self,
        x: f64,
        z: f64,
    ) -> Option<(
        usize,
        OpeningCellKind,
        Option<crate::modules::aec::engine::opening_style::LeafOperation>,
    )> {
        let layout = self.grid.layout(self.width, self.height, self.kind).ok()?;
        let columns = self.grid.vertical.len().max(1);
        layout
            .into_iter()
            .find(|cell| x >= cell.left && x <= cell.right && z >= cell.bottom && z <= cell.top)
            .map(|cell| {
                (
                    cell.row * columns + cell.column,
                    cell.cell.kind,
                    cell.cell.hinge,
                )
            })
    }

    fn insert_vertical(&self, x: f64) -> Option<String> {
        if self.width <= 0.1 {
            return None;
        }
        let mut weights = if self.grid.vertical.is_empty() {
            vec![1.0]
        } else {
            self.grid.vertical.clone()
        };
        if weights.len() >= 16 {
            return None;
        }
        let target = ((x / self.width) + 0.5).clamp(0.05, 0.95);
        let mut start = 0.0;
        for index in 0..weights.len() {
            let end = start + weights[index];
            if target > start + 0.04 && target < end - 0.04 {
                let first = target - start;
                let second = end - target;
                weights[index] = first;
                weights.insert(index + 1, second);
                return Some(format_values(&weights));
            }
            start = end;
        }
        None
    }

    fn insert_horizontal(&self, z: f64) -> Option<String> {
        if self.height <= 0.1 || self.grid.horizontal.len() >= 15 {
            return None;
        }
        let target = z.clamp(0.05, self.height - 0.05);
        let mut values = self.grid.horizontal.clone();
        if values.iter().any(|value| (value - target).abs() < 0.05) {
            return None;
        }
        let index = values.partition_point(|value| *value < target);
        values.insert(index, target);
        Some(format_values(&values))
    }

    fn drag_vertical(&self, index: usize, x: f64) -> Option<String> {
        let mut weights = self.grid.vertical.clone();
        if index + 1 >= weights.len() {
            return None;
        }
        let start: f64 = weights[..index].iter().sum();
        let span = weights[index] + weights[index + 1];
        let boundary = ((x / self.width) + 0.5).clamp(start + 0.04, start + span - 0.04);
        weights[index] = boundary - start;
        weights[index + 1] = span - weights[index];
        Some(format_values(&weights))
    }

    fn drag_horizontal(&self, index: usize, z: f64) -> Option<String> {
        if index >= self.grid.horizontal.len() {
            return None;
        }
        let lower = if index == 0 {
            0.04
        } else {
            self.grid.horizontal[index - 1] + 0.04
        };
        let upper = self
            .grid
            .horizontal
            .get(index + 1)
            .copied()
            .unwrap_or(self.height - 0.04)
            - 0.04;
        let mut values = self.grid.horizontal.clone();
        values[index] = z.clamp(lower, upper.max(lower));
        Some(format_values(&values))
    }

    fn dimension_message(axis: DividerAxis, value: String) -> Message {
        let key = match axis {
            DividerAxis::Vertical => "vertical",
            DividerAxis::Horizontal => "horizontal",
        };
        Message::Aec(AecMessage::AecOpeningStyleManagerDimChanged(
            key.to_string(),
            value,
        ))
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
                    let value = match drag.axis {
                        DividerAxis::Vertical => self.drag_vertical(drag.index, x),
                        DividerAxis::Horizontal => self.drag_horizontal(drag.index, z),
                    }?;
                    return Some(
                        canvas::Action::publish(Self::dimension_message(drag.axis, value))
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
                if let Some(value) = self.insert_vertical(x) {
                    return Some(
                        canvas::Action::publish(Self::dimension_message(
                            DividerAxis::Vertical,
                            value,
                        ))
                        .and_capture(),
                    );
                }
            }
            if (point.x - canvas_left).abs() <= tolerance
                || (point.x - canvas_right).abs() <= tolerance
            {
                if let Some(value) = self.insert_horizontal(z) {
                    return Some(
                        canvas::Action::publish(Self::dimension_message(
                            DividerAxis::Horizontal,
                            value,
                        ))
                        .and_capture(),
                    );
                }
            }
        }

        let (index, kind, hinge) = self.cell_at(x, z)?;
        let message = if *button == mouse::Button::Left {
            AecMessage::AecOpeningStyleManagerCellSelect(index)
        } else if kind.permits_hinge() {
            let next = match hinge.map(|op| op.bearing()) {
                Some(crate::modules::aec::engine::construction_grid::TrackEnd::Start) => "Right",
                Some(crate::modules::aec::engine::construction_grid::TrackEnd::End) | None => "Left",
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
        let cells = self.grid.layout(self.width, self.height, self.kind).ok();
        if let Some(cells) = cells {
            let columns = self.grid.vertical.len().max(1);
            for cell in cells {
                let cell_index = cell.row * columns + cell.column;
                let x = origin_x + (cell.left + self.width * 0.5) as f32 * scale;
                let y = origin_y + (self.height - cell.top) as f32 * scale;
                let size = Size::new(
                    (cell.right - cell.left) as f32 * scale,
                    (cell.top - cell.bottom) as f32 * scale,
                );
                let rect = Path::rectangle(Point::new(x, y), size);
                frame.fill(&rect, cell_color(cell.cell.kind, theme));
                frame.stroke(
                    &rect,
                    Stroke::default()
                        .with_width(1.5)
                        .with_color(theme.palette().background.base.text),
                );
                if self.selected_cell == Some(cell_index) {
                    frame.stroke(
                        &rect,
                        Stroke::default()
                            .with_width(2.5)
                            .with_color(theme.palette().primary.base.color),
                    );
                }
                if let Some(hinge) = cell.cell.hinge {
                    let marker_x = match hinge.bearing() {
                        crate::modules::aec::engine::construction_grid::TrackEnd::Start => x + 3.0,
                        crate::modules::aec::engine::construction_grid::TrackEnd::End => {
                            x + size.width - 3.0
                        }
                    };
                    frame.fill_rectangle(
                        Point::new(marker_x, y + 3.0),
                        Size::new(2.0, (size.height - 6.0).max(1.0)),
                        theme.palette().primary.base.color,
                    );
                }
            }
        } else {
            let rect = Path::rectangle(
                Point::new(origin_x, origin_y),
                Size::new(self.width as f32 * scale, self.height as f32 * scale),
            );
            frame.stroke(
                &rect,
                Stroke::default()
                    .with_width(1.5)
                    .with_color(theme.palette().background.base.text),
            );
        }
        vec![frame.into_geometry()]
    }
}

fn cell_color(kind: OpeningCellKind, theme: &Theme) -> Color {
    match kind {
        OpeningCellKind::Glass => Color::from_rgba(0.30, 0.68, 0.86, 0.30),
        OpeningCellKind::Opaque => theme.palette().background.weak.color.scale_alpha(0.5),
        OpeningCellKind::DoorLeaf => Color::from_rgba(0.72, 0.56, 0.34, 0.35),
        OpeningCellKind::Fixed => Color::from_rgba(0.40, 0.72, 0.48, 0.32),
    }
}

fn format_values(values: &[f64]) -> String {
    values
        .iter()
        .map(|value| format!("{value:.4}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sketch(grid: DivisionGrid) -> OpeningDivisionCanvas {
        OpeningDivisionCanvas {
            grid,
            width: 1.2,
            height: 1.4,
            kind: OpeningKind::Window,
            selected_cell: None,
        }
    }

    #[test]
    fn inserting_and_dragging_dividers_preserves_grid_weights() {
        let empty = sketch(DivisionGrid::default());
        let inserted = empty.insert_vertical(0.0).expect("vertical split");
        let weights: Vec<f64> = inserted
            .split(',')
            .map(|value| value.trim().parse().unwrap())
            .collect();
        assert_eq!(weights.len(), 2);
        assert!((weights.iter().sum::<f64>() - 1.0).abs() < 1e-9);

        let divided = sketch(DivisionGrid {
            vertical: weights,
            ..DivisionGrid::default()
        });
        let dragged = divided.drag_vertical(0, 0.12).expect("drag split");
        let weights: Vec<f64> = dragged
            .split(',')
            .map(|value| value.trim().parse().unwrap())
            .collect();
        assert!((weights.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        assert!(weights[0] > 0.5);
    }

    #[test]
    fn horizontal_divisions_insert_and_drag_in_order() {
        let sketch = sketch(DivisionGrid {
            horizontal: vec![0.4, 0.9],
            ..DivisionGrid::default()
        });
        let inserted = sketch.insert_horizontal(0.7).expect("insert transom");
        let values: Vec<f64> = inserted
            .split(',')
            .map(|value| value.trim().parse().unwrap())
            .collect();
        assert_eq!(values, vec![0.4, 0.7, 0.9]);

        let moved = sketch.drag_horizontal(0, 0.6).expect("drag transom");
        let values: Vec<f64> = moved
            .split(',')
            .map(|value| value.trim().parse().unwrap())
            .collect();
        assert!(values.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn cell_at_returns_stable_row_major_index_for_selection() {
        let canvas = sketch(DivisionGrid {
            vertical: vec![0.5, 0.5],
            cells: vec![Default::default(), Default::default()],
            ..DivisionGrid::default()
        });
        assert_eq!(canvas.cell_at(0.3, 0.7).map(|cell| cell.0), Some(1));
    }
}
