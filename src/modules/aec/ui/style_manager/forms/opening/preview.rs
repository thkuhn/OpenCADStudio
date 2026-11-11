use super::*;

pub(super) struct PreviewCamera {
    pub(super) scale: f32,
    pub(super) ox: f32,
    pub(super) oy: f32,
    /// Section depth runs right-to-left in model space. Mirror it so interior is left.
    pub(super) mirror_x: bool,
    pub(super) zoom: f32,
    pub(super) pan_x: f32,
    pub(super) pan_y: f32,
}

/// Wall drawn above the rough head in the section preview, independent of lintel or frame.
pub(super) const SECTION_SAMPLE_ABOVE: f64 = 0.3;

impl PreviewCamera {
    pub(super) fn fit(
        bounds: Rectangle,
        paths: &[BakedPath],
        mode: AecOpeningPreviewMode,
        width: f64,
        height: f64,
        sill: f64,
        thickness: f64,
    ) -> Self {
        let pad = 14.0f32;
        let hw = width.max(1e-6) * 0.5;
        let ht = thickness.max(1e-6) * 0.5;
        let (mut min_x, mut max_x, mut min_y, mut max_y) = match mode {
            AecOpeningPreviewMode::Plan2D => (-hw, hw, -ht, ht),
            AecOpeningPreviewMode::Section => {
                let head = sill.max(0.0) + height.max(1e-6) + SECTION_SAMPLE_ABOVE;
                (-ht, ht, 0.0, head)
            }
            AecOpeningPreviewMode::Elevation2D | AecOpeningPreviewMode::Model3D => (
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
            ),
        };

        if matches!(
            mode,
            AecOpeningPreviewMode::Elevation2D | AecOpeningPreviewMode::Model3D
        ) {
            for path in paths {
                for &(x, y) in &path.points {
                    if x.is_finite() && y.is_finite() {
                        min_x = min_x.min(x);
                        max_x = max_x.max(x);
                        min_y = min_y.min(y);
                        max_y = max_y.max(y);
                    }
                }
            }
        }

        if mode == AecOpeningPreviewMode::Elevation2D {
            min_x = min_x.min(-hw);
            max_x = max_x.max(hw);
            min_y = min_y.min(0.0);
            max_y = max_y.max(height.max(1e-6));
        }

        if !min_x.is_finite() || !max_x.is_finite() || (max_x - min_x) < 1e-4 {
            min_x = -hw;
            max_x = hw;
        }
        if !min_y.is_finite() || !max_y.is_finite() || (max_y - min_y) < 1e-4 {
            let (ref_y_min, ref_y_max) = match mode {
                AecOpeningPreviewMode::Plan2D => {
                    let ht = thickness.max(1e-6) * 0.5;
                    (-ht, ht)
                }
                AecOpeningPreviewMode::Elevation2D
                | AecOpeningPreviewMode::Section
                | AecOpeningPreviewMode::Model3D => {
                    (0.0, height.max(1e-6))
                }
            };
            min_y = ref_y_min;
            max_y = ref_y_max;
        }

        let span_x = (max_x - min_x).max(1e-4);
        let span_y = (max_y - min_y).max(1e-4);
        let center_x = (min_x + max_x) * 0.5;
        let center_y = (min_y + max_y) * 0.5;

        let world_w = (span_x * 1.15) as f32;
        let world_h = (span_y * 1.15) as f32;
        let avail_w = (bounds.width - 2.0 * pad).max(1.0);
        let avail_h = (bounds.height - 2.0 * pad).max(1.0);
        let scale = (avail_w / world_w).min(avail_h / world_h);

        Self {
            scale,
            ox: bounds.width * 0.5 - (center_x as f32 * scale),
            oy: bounds.height * 0.5 + (center_y as f32 * scale),
            mirror_x: mode == AecOpeningPreviewMode::Section,
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
        }
    }

    pub(super) fn with_view(mut self, zoom: f64, pan_x: f64, pan_y: f64) -> Self {
        self.zoom = zoom.max(0.05) as f32;
        self.pan_x = pan_x as f32;
        self.pan_y = pan_y as f32;
        self
    }

    pub(super) fn to_screen(&self, x: f64, y: f64) -> Point {
        let x = if self.mirror_x { -x } else { x };
        Point::new(
            self.ox + self.pan_x + x as f32 * self.scale * self.zoom,
            self.oy + self.pan_y - y as f32 * self.scale * self.zoom,
        )
    }

    pub(super) fn to_local(&self, p: Point) -> (f64, f64) {
        let x = (p.x - self.ox - self.pan_x) as f64 / (self.scale * self.zoom) as f64;
        (
            if self.mirror_x { -x } else { x },
            (self.oy + self.pan_y - p.y) as f64 / (self.scale * self.zoom) as f64,
        )
    }
}

pub(super) fn preview_pan_for_cursor(
    camera: &PreviewCamera,
    cursor: Point,
    world_x: f64,
    world_y: f64,
    zoom: f64,
) -> (f64, f64) {
    let screen_wx = if camera.mirror_x { -world_x } else { world_x };
    (
        cursor.x as f64 - camera.ox as f64 - screen_wx * camera.scale as f64 * zoom,
        cursor.y as f64 - camera.oy as f64 + world_y * camera.scale as f64 * zoom,
    )
}

/// Slot of the preview path under the model-space point `(x, y)`: an edge
/// within `tol` wins (nearest first), otherwise the smallest closed path
/// containing the point.
pub(super) fn hit_path<'a>(
    paths: &'a [BakedPath],
    x: f64,
    y: f64,
    tol: f64,
) -> Option<&'a BakedPath> {
    pub(super) fn seg_dist(px: f64, py: f64, a: (f64, f64), b: (f64, f64)) -> f64 {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len2 = dx * dx + dy * dy;
        let t = if len2 < 1e-18 { 0.0 } else { (((px - a.0) * dx + (py - a.1) * dy) / len2).clamp(0.0, 1.0) };
        ((px - (a.0 + t * dx)).powi(2) + (py - (a.1 + t * dy)).powi(2)).sqrt()
    }
    let mut best_edge: Option<(f64, &'a BakedPath)> = None;
    let mut best_area: Option<(f64, &'a BakedPath)> = None;
    for path in paths {
        let pts = &path.points;
        if pts.len() < 2 {
            continue;
        }
        let n = if path.closed { pts.len() } else { pts.len() - 1 };
        for i in 0..n {
            let d = seg_dist(x, y, pts[i], pts[(i + 1) % pts.len()]);
            if d <= tol && best_edge.map_or(true, |(bd, _)| d < bd) {
                best_edge = Some((d, path));
            }
        }
        if path.closed && pts.len() >= 3 {
            let mut inside = false;
            let mut area = 0.0;
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                area += a.0 * b.1 - b.0 * a.1;
                if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
                    inside = !inside;
                }
            }
            let area = (area * 0.5).abs();
            if inside && best_area.map_or(true, |(ba, _)| area < ba) {
                best_area = Some((area, path));
            }
        }
    }
    best_edge.map(|(_, path)| path).or(best_area.map(|(_, path)| path))
}

pub(crate) fn hit_slot(
    paths: &[BakedPath],
    x: f64,
    y: f64,
    tol: f64,
) -> Option<crate::modules::aec::engine::display_component::OpeningComponentSlot> {
    hit_path(paths, x, y, tol).map(|path| path.slot)
}

pub(super) fn hit_role(
    paths: &[BakedPath],
    x: f64,
    y: f64,
    tol: f64,
) -> Option<OpeningLookRole> {
    hit_path(paths, x, y, tol).and_then(BakedPath::resolved_look_role)
}

pub(super) struct OpeningPreviewCanvas {
    pub(super) paths: Vec<BakedPath>,
    pub(super) mode: AecOpeningPreviewMode,
    pub(super) width: f64,
    pub(super) height: f64,
    pub(super) sill: f64,
    pub(super) thickness: f64,
    pub(super) look_styles: HashMap<OpeningLookRole, PreviewLookColors>,
    pub(super) selected_role: Option<OpeningLookRole>,
    pub(super) isometric: AecOpeningIsometric,
    pub(super) zoom: f64,
    pub(super) pan_x: f64,
    pub(super) pan_y: f64,
}

pub(super) struct PreviewGesture {
    pub(super) origin: Point,
    pub(super) dragging: bool,
}

impl OpeningPreviewCanvas {
    pub(super) fn projected_paths(&self) -> Vec<BakedPath> {
        if self.mode != AecOpeningPreviewMode::Model3D {
            return self.paths.clone();
        }
        let (azimuth, elevation) = self.isometric.camera_angles();
        let camera = PreviewCamera3d {
            azimuth,
            elevation,
        };
        self.paths
            .iter()
            .cloned()
            .map(|mut path| {
                if let Some(points) = &path.points_3d {
                    path.points = points
                        .iter()
                        .map(|point| preview_project(camera, point[0], point[1], point[2]))
                        .collect();
                }
                path
            })
            .collect()
    }

    pub(super) fn camera(&self, bounds: Rectangle, paths: &[BakedPath]) -> PreviewCamera {
        PreviewCamera::fit(
            bounds,
            paths,
            self.mode,
            self.width,
            self.height,
            self.sill,
            self.thickness,
        )
        .with_view(self.zoom, self.pan_x, self.pan_y)
    }
}

impl Program<Message> for OpeningPreviewCanvas {
    type State = Option<PreviewGesture>;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let paths = self.projected_paths();
        let cam = self.camera(bounds, &paths);
        match event {
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let pos = cursor.position_in(bounds)?;
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 40.0,
                };
                if lines.abs() < 1e-6 {
                    return None;
                }
                let factor = if lines > 0.0 { 1.1_f64 } else { 1.0 / 1.1 };
                let new_zoom = (self.zoom * factor).clamp(0.25, 12.0);
                let (wx, wy) = cam.to_local(pos);
                let fitted = PreviewCamera::fit(
                    bounds,
                    &paths,
                    self.mode,
                    self.width,
                    self.height,
                    self.sill,
                    self.thickness,
                );
                let (new_pan_x, new_pan_y) =
                    preview_pan_for_cursor(&fitted, pos, wx, wy, new_zoom);
                return Some(
                    canvas::Action::publish(Message::Aec(
                        AecMessage::AecOpeningStyleManagerPreviewView(
                            new_zoom,
                            new_pan_x,
                            new_pan_y,
                        ),
                    ))
                    .and_capture(),
                );
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor.position_in(bounds)?;
                if self.mode == AecOpeningPreviewMode::Model3D {
                    *state = Some(PreviewGesture {
                        origin: pos,
                        dragging: false,
                    });
                    return Some(canvas::Action::capture());
                }
                let (x, y) = cam.to_local(pos);
                return Some(
                    canvas::Action::publish(Message::Aec(
                        AecMessage::AecOpeningStyleManagerPreviewSlotSelect(hit_role(
                            &paths,
                            x,
                            y,
                            8.0 / cam.scale.max(1e-6) as f64,
                        )),
                    ))
                    .and_capture(),
                );
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let gesture = state.as_mut()?;
                let pos = cursor.position_in(bounds)?;
                let moved = (pos.x - gesture.origin.x).hypot(pos.y - gesture.origin.y);
                if moved < 4.0 && !gesture.dragging {
                    return Some(canvas::Action::capture());
                }
                gesture.dragging = true;
                return Some(canvas::Action::capture());
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let gesture = state.take()?;
                if gesture.dragging {
                    return Some(canvas::Action::capture());
                }
                let (x, y) = cam.to_local(gesture.origin);
                return Some(
                    canvas::Action::publish(Message::Aec(
                        AecMessage::AecOpeningStyleManagerPreviewSlotSelect(hit_role(
                            &paths,
                            x,
                            y,
                            8.0 / cam.scale.max(1e-6) as f64,
                        )),
                    ))
                    .and_capture(),
                );
            }
            _ => None,
        }
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if self.mode == AecOpeningPreviewMode::Model3D {
            return if state.is_some() {
                mouse::Interaction::Grabbing
            } else {
                mouse::Interaction::Pointer
            };
        }
        let Some(pos) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        let paths = self.projected_paths();
        let cam = self.camera(bounds, &paths);
        let (x, y) = cam.to_local(pos);
        if hit_path(&paths, x, y, 8.0 / cam.scale.max(1e-6) as f64).is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
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
        let paths = self.projected_paths();
        let cam = self.camera(bounds, &paths);

        if self.mode == AecOpeningPreviewMode::Plan2D {
            let hw = self.width * 0.5;
            let ht = self.thickness * 0.5;
            let guide = theme.palette().background.neutral.color;
            let outer = Path::new(|p| {
                p.move_to(cam.to_screen(-hw, -ht));
                p.line_to(cam.to_screen(hw, -ht));
                p.line_to(cam.to_screen(hw, ht));
                p.line_to(cam.to_screen(-hw, ht));
                p.close();
            });
            frame.stroke(
                &outer,
                Stroke::default().with_width(1.0).with_color(guide),
            );
        }

        if self.mode == AecOpeningPreviewMode::Elevation2D {
            let hw = self.width * 0.5;
            let head = self.height.max(0.0);
            let guide = theme.palette().background.neutral.color;
            let rough = Path::new(|p| {
                p.move_to(cam.to_screen(-hw, 0.0));
                p.line_to(cam.to_screen(hw, 0.0));
                p.line_to(cam.to_screen(hw, head));
                p.line_to(cam.to_screen(-hw, head));
                p.close();
            });
            frame.stroke(
                &rough,
                Stroke::default().with_width(1.0).with_color(guide),
            );
        }

        let fallback_stroke_color = theme.palette().background.base.text;
        let fallback_fill_color = fallback_stroke_color.scale_alpha(0.18);
        for path in &paths {
            if path.points.len() < 2 {
                continue;
            }
            if self.mode == AecOpeningPreviewMode::Elevation2D
                && path.slot == OpeningComponentSlot::Mark2D
                && path.look_role.is_none()
            {
                continue;
            }
            let first = cam.to_screen(path.points[0].0, path.points[0].1);
            let closed = path.closed;
            let pts = path.points.clone();
            let built = Path::new(|p| {
                p.move_to(first);
                for &(x, y) in &pts[1..] {
                    p.line_to(cam.to_screen(x, y));
                }
                if closed {
                    p.close();
                }
            });
            let role_colors = path
                .resolved_look_role()
                .and_then(|role| self.look_styles.get(&role));
            let selected = self.selected_role.is_some()
                && path.resolved_look_role() == self.selected_role;
            let stroke_color = if selected {
                theme.palette().primary.base.color
            } else if path.resolved_look_role() == Some(OpeningLookRole::Division)
                && role_colors.and_then(|colors| colors.line).is_none()
            {
                opening_grid_preview_color()
            } else {
                role_colors
                    .and_then(|colors| colors.line)
                    .unwrap_or(fallback_stroke_color)
            };
            if path.filled && path.closed {
                let fill_color = if self.mode == AecOpeningPreviewMode::Section {
                    role_colors
                        .and_then(|colors| colors.hatch)
                        .or_else(|| role_colors.and_then(|colors| colors.line))
                        .unwrap_or(stroke_color)
                        .scale_alpha(0.18)
                } else {
                    fallback_fill_color
                };
                frame.fill(&built, fill_color);
            }
            frame.stroke(
                &built,
                Stroke::default()
                    .with_width(if selected { 3.0 } else { 1.2 })
                    .with_color(stroke_color),
            );
        }

        vec![frame.into_geometry()]
    }
}

