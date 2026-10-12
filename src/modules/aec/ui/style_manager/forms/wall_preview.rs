//! Wall style preview: 2D layer buildup (cross-section) or an isometric 3D
//! block with the layers as coloured slices. Modelled on the opening and
//! slab previews (`canvas` program + toggle buttons above it).

use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Program, Stroke, Style as CanvasStyle, Text};
use iced::widget::{button, column, container, row, text, Space};
use iced::{Border, Color, Element, Fill, Point, Rectangle, Theme};

use crate::app::{AecLayerBuffer, AecMessage, Message};
use crate::modules::aec::state::AecWallPreviewMode;
use crate::modules::aec::ui::aec_ui_util::muted;
use crate::t;

/// Default thickness (cm) for layers whose thickness is a formula or empty.
const FALLBACK_THICKNESS_CM: f32 = 10.0;
/// Height (cm) of the 3D preview block.
const BLOCK_HEIGHT_CM: f32 = 100.0;
/// Length (cm) of the 3D preview block along the wall run.
const BLOCK_LENGTH_CM: f32 = 80.0;

/// One layer as shown in the preview.
#[derive(Clone, Debug, PartialEq)]
pub struct WallPreviewLayer {
    pub thickness_cm: f32,
    /// Start of the layer relative to the wall axis (cm, axis = 0).
    pub start_cm: f32,
    /// Vertical offset from the wall base (cm, >= 0).
    pub bottom_cm: f32,
    /// Vertical offset from the wall top (cm, >= 0).
    pub top_cm: f32,
    /// Material colour (0xRRGGBB).
    pub color: u32,
    /// Effective hatch pattern name (layer override, else material); empty = none.
    pub hatch: String,
}

impl WallPreviewLayer {
    /// Layers stacked contiguously and centred on the axis (no explicit offsets).
    pub fn stack(thicknesses_cm: &[f32]) -> Vec<WallPreviewLayer> {
        let mut cursor = -thicknesses_cm.iter().sum::<f32>() * 0.5;
        thicknesses_cm
            .iter()
            .map(|&t| {
                let l = WallPreviewLayer {
                    thickness_cm: t,
                    start_cm: cursor,
                    bottom_cm: 0.0,
                    top_cm: 0.0,
                    color: 0,
                    hatch: String::new(),
                };
                cursor += t;
                l
            })
            .collect()
    }
}

fn parse_cm(s: &str) -> Option<f32> {
    s.trim().replace(',', ".").parse::<f32>().ok().filter(|v| v.is_finite())
}

/// Builds the preview layers from the edit buffers; `material_info` maps a
/// material id to `(line colour 0xRRGGBB, hatch pattern name)`.
pub fn preview_layers(
    buffers: &[AecLayerBuffer],
    material_info: &dyn Fn(&str) -> Option<(u32, String)>,
) -> Vec<WallPreviewLayer> {
    let thick: Vec<f32> = buffers
        .iter()
        .map(|b| parse_cm(&b.thickness).filter(|v| *v > 0.0).unwrap_or(FALLBACK_THICKNESS_CM))
        .collect();
    let any_axis = buffers.iter().any(|b| parse_cm(&b.axis_offset).is_some());
    let mut cursor = -thick.iter().sum::<f32>() * 0.5;
    buffers
        .iter()
        .zip(thick)
        .map(|(b, thickness_cm)| {
            let start_cm = if any_axis {
                parse_cm(&b.axis_offset).unwrap_or(cursor)
            } else {
                cursor
            };
            cursor = start_cm + thickness_cm;
            let (color, mat_hatch) =
                material_info(&b.material_id).unwrap_or((0x808080, String::new()));
            let hatch = if b.hatch_override.trim().is_empty() {
                mat_hatch
            } else {
                b.hatch_override.trim().to_string()
            };
            WallPreviewLayer {
                thickness_cm,
                start_cm,
                bottom_cm: parse_cm(&b.bottom_offset).unwrap_or(0.0).max(0.0),
                top_cm: parse_cm(&b.top_offset).unwrap_or(0.0).max(0.0),
                color,
                hatch,
            }
        })
        .collect()
}

/// Horizontal extent (cm) of the buildup including the axis at 0.
fn extent(layers: &[WallPreviewLayer]) -> Option<(f32, f32)> {
    if layers.is_empty() {
        return None;
    }
    let min = layers.iter().map(|l| l.start_cm).fold(0.0, f32::min);
    let max = layers.iter().map(|l| l.start_cm + l.thickness_cm).fold(0.0, f32::max);
    (max - min > 0.0).then_some((min, max))
}

/// Maximum pixels per cm of the 2D buildup (keeps the section narrow).
const MAX_PX_PER_CM: f32 = 5.0;
/// Fraction of the canvas width the 2D buildup may occupy.
const WIDTH_FRACTION_2D: f32 = 0.5;
const PAD_2D: f32 = 22.0;

/// `(pixels per cm, screen x of the wall axis)` of the 2D buildup.
fn layout_2d(layers: &[WallPreviewLayer], bounds: Rectangle) -> Option<(f32, f32)> {
    let (min, max) = extent(layers)?;
    let span = max - min;
    let avail = (bounds.width - PAD_2D * 2.0).max(10.0);
    let scale = (avail * WIDTH_FRACTION_2D / span).min(MAX_PX_PER_CM);
    let axis_x = bounds.x + bounds.width * 0.5 - (min + max) * 0.5 * scale;
    Some((scale, axis_x))
}

/// Screen x of the wall axis in the 2D buildup.
pub fn axis_x_2d(layers: &[WallPreviewLayer], bounds: Rectangle) -> Option<f32> {
    layout_2d(layers, bounds).map(|(_, x)| x)
}

/// Rectangles of the layers in the 2D buildup inside `bounds`, positioned by
/// their axis offsets.
pub fn layer_rects_2d(layers: &[WallPreviewLayer], bounds: Rectangle) -> Vec<Rectangle> {
    let Some((scale, axis_x)) = layout_2d(layers, bounds) else {
        return Vec::new();
    };
    let h = (bounds.height - PAD_2D * 2.0).max(10.0);
    layers
        .iter()
        .map(|l| Rectangle {
            x: axis_x + l.start_cm * scale,
            y: bounds.y + PAD_2D,
            width: l.thickness_cm * scale,
            height: h,
        })
        .collect()
}

fn color_of(rgb: u32, alpha: f32) -> Color {
    Color::from_rgba8(((rgb >> 16) & 0xff) as u8, ((rgb >> 8) & 0xff) as u8, (rgb & 0xff) as u8, alpha)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HatchKind {
    Solid,
    Diagonal,
    Cross,
    Horizontal,
    Vertical,
    Dots,
    Brick,
}

/// Rough visual class of a hatch pattern name (`None` = no hatch).
pub fn hatch_kind(pattern: &str) -> Option<HatchKind> {
    let p = pattern.trim().to_ascii_uppercase();
    if p.is_empty() {
        return None;
    }
    let has = |keys: &[&str]| keys.iter().any(|k| p.contains(k));
    Some(if p == "SOLID" {
        HatchKind::Solid
    } else if has(&["BRICK", "BRSTONE", "AR-B"]) {
        HatchKind::Brick
    } else if has(&["CROSS", "NET", "GRID", "ANSI37", "ANSI38", "HONEY", "SQUARE"]) {
        HatchKind::Cross
    } else if has(&["DOT", "SAND", "GRAVEL", "EARTH", "CONC", "STARS", "AR-S"]) {
        HatchKind::Dots
    } else if has(&["LINE", "ANSI35", "STEEL", "HORIZ"]) {
        HatchKind::Horizontal
    } else if has(&["VERT"]) {
        HatchKind::Vertical
    } else {
        HatchKind::Diagonal
    })
}

/// Line segments of a hatch inside `r` (screen coordinates).
pub fn hatch_segments(kind: HatchKind, r: Rectangle, step: f32) -> Vec<(Point, Point)> {
    let mut out = Vec::new();
    let (x_end, y_end) = (r.x + r.width, r.y + r.height);
    let diagonal = |out: &mut Vec<(Point, Point)>, down: bool| {
        if down {
            let mut c = r.x - r.y - r.height;
            while c < x_end - r.y {
                let x0 = r.x.max(r.y + c);
                let x1 = x_end.min(y_end + c);
                if x0 < x1 {
                    out.push((Point::new(x0, x0 - c), Point::new(x1, x1 - c)));
                }
                c += step;
            }
        } else {
            let mut c = r.x + r.y;
            while c < x_end + y_end {
                let x0 = r.x.max(c - y_end);
                let x1 = x_end.min(c - r.y);
                if x0 < x1 {
                    out.push((Point::new(x0, c - x0), Point::new(x1, c - x1)));
                }
                c += step;
            }
        }
    };
    let horizontal = |out: &mut Vec<(Point, Point)>| {
        let mut y = r.y + step * 0.5;
        while y < y_end {
            out.push((Point::new(r.x, y), Point::new(x_end, y)));
            y += step;
        }
    };
    match kind {
        HatchKind::Solid => {}
        HatchKind::Diagonal => diagonal(&mut out, true),
        HatchKind::Cross => {
            diagonal(&mut out, true);
            diagonal(&mut out, false);
        }
        HatchKind::Horizontal => horizontal(&mut out),
        HatchKind::Vertical => {
            let mut x = r.x + step * 0.5;
            while x < x_end {
                out.push((Point::new(x, r.y), Point::new(x, y_end)));
                x += step;
            }
        }
        HatchKind::Dots => {
            let mut row = 0;
            let mut y = r.y + step * 0.5;
            while y < y_end {
                let mut x = r.x + step * 0.5 + if row % 2 == 1 { step * 0.5 } else { 0.0 };
                while x < x_end {
                    out.push((Point::new(x, y), Point::new(x + 1.0, y)));
                    x += step;
                }
                y += step;
                row += 1;
            }
        }
        HatchKind::Brick => {
            horizontal(&mut out);
            let mut row = 0;
            let mut y = r.y + step * 0.5;
            while y < y_end {
                let mut x = r.x + if row % 2 == 1 { step } else { 0.0 };
                while x < x_end {
                    out.push((Point::new(x, y), Point::new(x, (y + step).min(y_end))));
                    x += step * 2.0;
                }
                y += step;
                row += 1;
            }
        }
    }
    out
}

pub struct WallPreviewCanvas {
    pub layers: Vec<WallPreviewLayer>,
    pub mode: AecWallPreviewMode,
    pub selected: Option<usize>,
}

const AXIS_COLOR: Color = Color { r: 0.85, g: 0.2, b: 0.2, a: 1.0 };

fn axis_stroke() -> Stroke<'static> {
    Stroke {
        width: 1.0,
        style: CanvasStyle::Solid(AXIS_COLOR),
        line_dash: canvas::LineDash { segments: &[10.0, 3.0, 2.0, 3.0], offset: 0 },
        ..Default::default()
    }
}

impl Program<Message> for WallPreviewCanvas {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event else {
            return None;
        };
        let pos = cursor.position_in(bounds)?;
        let local = Rectangle { x: 0.0, y: 0.0, width: bounds.width, height: bounds.height };
        let index = layer_at(&self.layers, self.mode, local, pos)?;
        Some(
            canvas::Action::publish(Message::Aec(AecMessage::AecWallStyleManagerPreviewLayerSelect(
                index,
            )))
            .and_capture(),
        )
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let local = Rectangle { x: 0.0, y: 0.0, width: bounds.width, height: bounds.height };
        match cursor.position_in(bounds) {
            Some(p) if layer_at(&self.layers, self.mode, local, p).is_some() => {
                mouse::Interaction::Pointer
            }
            _ => mouse::Interaction::default(),
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let outline = theme.palette().background.base.text;
        let local = Rectangle { x: 0.0, y: 0.0, width: bounds.width, height: bounds.height };
        match self.mode {
            AecWallPreviewMode::CrossSection2D => self.draw_2d(&mut frame, local, outline),
            AecWallPreviewMode::Model3D => self.draw_3d(&mut frame, local, outline),
        }
        vec![frame.into_geometry()]
    }
}

impl WallPreviewCanvas {
    fn draw_2d(&self, frame: &mut Frame, bounds: Rectangle, outline: Color) {
        let rects = layer_rects_2d(&self.layers, bounds);
        for (i, (layer, r)) in self.layers.iter().zip(&rects).enumerate() {
            let path = Path::rectangle(Point::new(r.x, r.y), r.size());
            frame.fill(&path, color_of(layer.color, 0.35));
            if let Some(kind) = hatch_kind(&layer.hatch) {
                if kind == HatchKind::Solid {
                    frame.fill(&path, color_of(layer.color, 0.7));
                }
                let width = if kind == HatchKind::Dots { 2.0 } else { 1.0 };
                for (a, b) in hatch_segments(kind, *r, 7.0) {
                    frame.stroke(
                        &Path::line(a, b),
                        Stroke {
                            width,
                            style: CanvasStyle::Solid(color_of(layer.color, 0.9)),
                            ..Default::default()
                        },
                    );
                }
            }
            frame.stroke(
                &path,
                Stroke {
                    width: if self.selected == Some(i) { 3.0 } else { 1.2 },
                    style: CanvasStyle::Solid(if self.selected == Some(i) {
                        Color::from_rgb(0.1, 0.6, 0.9)
                    } else {
                        outline
                    }),
                    ..Default::default()
                },
            );
            frame.fill_text(Text {
                content: format!("{}", i + 1),
                position: Point::new(r.x + r.width * 0.5, r.y - 8.0),
                color: outline,
                size: iced::Pixels(10.0),
                align_x: iced::advanced::text::Alignment::Center,
                align_y: iced::alignment::Vertical::Center,
                ..Default::default()
            });
        }
        if let Some(ax) = axis_x_2d(&self.layers, bounds) {
            let line = Path::line(
                Point::new(ax, bounds.y + 6.0),
                Point::new(ax, bounds.y + bounds.height - 6.0),
            );
            frame.stroke(&line, axis_stroke());
        }
    }

    /// Isometric block: x = layer stacking, y = wall run, z = height.
    fn draw_3d(&self, frame: &mut Frame, bounds: Rectangle, outline: Color) {
        for (i, [end, side, top]) in faces_3d(&self.layers, bounds).into_iter().enumerate() {
            let base = color_of(self.layers[i].color, 0.9);
            let shade = |f: f32| Color { r: base.r * f, g: base.g * f, b: base.b * f, a: base.a };
            let width = if self.selected == Some(i) { 2.5 } else { 1.0 };
            for (pts, fill) in [(end, shade(0.75)), (side, shade(0.55)), (top, shade(1.0))] {
                let path = Path::new(|p| {
                    p.move_to(pts[0]);
                    for q in &pts[1..] {
                        p.line_to(*q);
                    }
                    p.close();
                });
                frame.fill(&path, fill);
                frame.stroke(
                    &path,
                    Stroke { width, style: CanvasStyle::Solid(outline), ..Default::default() },
                );
            }
        }
        if let Some(iso) = iso(&self.layers, bounds) {
            let (len, h) = (BLOCK_LENGTH_CM, BLOCK_HEIGHT_CM);
            frame.stroke(&Path::line(iso.pt(0.0, 0.0, h), iso.pt(0.0, len, h)), axis_stroke());
            frame.stroke(&Path::line(iso.pt(0.0, len, 0.0), iso.pt(0.0, len, h)), axis_stroke());
        }
    }
}

fn proj(x: f32, y: f32, z: f32) -> (f32, f32) {
    ((x - y) * 0.866_025_4, (x + y) * 0.5 - z)
}

/// Isometric projection of the 3D block into the canvas.
struct Iso {
    ox: f32,
    oy: f32,
    scale: f32,
    /// Smallest x (cm) of the buildup; x is shifted by it before projecting.
    shift: f32,
}

impl Iso {
    fn pt(&self, x: f32, y: f32, z: f32) -> Point {
        let (sx, sy) = proj(x - self.shift, y, z);
        Point::new(self.ox + sx * self.scale, self.oy + sy * self.scale)
    }
}

fn iso(layers: &[WallPreviewLayer], bounds: Rectangle) -> Option<Iso> {
    let (min, max) = extent(layers)?;
    let span = max - min;
    let (len, h) = (BLOCK_LENGTH_CM, BLOCK_HEIGHT_CM);
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for x in [0.0, span] {
        for y in [0.0, len] {
            for z in [0.0, h] {
                let (sx, sy) = proj(x, y, z);
                min_x = min_x.min(sx);
                max_x = max_x.max(sx);
                min_y = min_y.min(sy);
                max_y = max_y.max(sy);
            }
        }
    }
    let pad = 16.0;
    let scale = ((bounds.width - pad * 2.0) / (max_x - min_x))
        .min((bounds.height - pad * 2.0) / (max_y - min_y))
        .max(0.01);
    Some(Iso {
        ox: bounds.x + bounds.width * 0.5 - (min_x + max_x) * 0.5 * scale,
        oy: bounds.y + bounds.height * 0.5 - (min_y + max_y) * 0.5 * scale,
        scale,
        shift: min,
    })
}

/// Screen quads `[end face (+y), side face (+x), top]` of every layer in the
/// isometric 3D block, in layer order. Layers honour their axis position and
/// their vertical bottom/top offsets.
pub fn faces_3d(layers: &[WallPreviewLayer], bounds: Rectangle) -> Vec<[[Point; 4]; 3]> {
    let Some(iso) = iso(layers, bounds) else {
        return Vec::new();
    };
    let (len, h) = (BLOCK_LENGTH_CM, BLOCK_HEIGHT_CM);
    layers
        .iter()
        .map(|l| {
            let (x0, x1) = (l.start_cm, l.start_cm + l.thickness_cm);
            let z0 = l.bottom_cm.min(h - 1.0);
            let z1 = (h - l.top_cm).max(z0 + 1.0);
            [
                [iso.pt(x0, len, z0), iso.pt(x1, len, z0), iso.pt(x1, len, z1), iso.pt(x0, len, z1)],
                [iso.pt(x1, 0.0, z0), iso.pt(x1, len, z0), iso.pt(x1, len, z1), iso.pt(x1, 0.0, z1)],
                [iso.pt(x0, 0.0, z1), iso.pt(x1, 0.0, z1), iso.pt(x1, len, z1), iso.pt(x0, len, z1)],
            ]
        })
        .collect()
}

fn point_in_quad(p: Point, quad: &[Point; 4]) -> bool {
    let mut inside = false;
    for i in 0..4 {
        let (a, b) = (quad[i], quad[(i + 1) % 4]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}

/// Index of the layer under `p` (local canvas coordinates), if any.
pub fn layer_at(
    layers: &[WallPreviewLayer],
    mode: AecWallPreviewMode,
    bounds: Rectangle,
    p: Point,
) -> Option<usize> {
    match mode {
        AecWallPreviewMode::CrossSection2D => layer_rects_2d(layers, bounds)
            .iter()
            .position(|r| r.contains(p)),
        AecWallPreviewMode::Model3D => faces_3d(layers, bounds)
            .iter()
            .enumerate()
            .rev()
            .find(|(_, faces)| faces.iter().any(|q| point_in_quad(p, q)))
            .map(|(i, _)| i),
    }
}

/// Toggle buttons + canvas for the wall form.
pub fn view<'a>(
    layers: Vec<WallPreviewLayer>,
    mode: AecWallPreviewMode,
    selected: Option<usize>,
) -> Element<'a, Message> {
    let is_2d = mode == AecWallPreviewMode::CrossSection2D;
    let is_3d = mode == AecWallPreviewMode::Model3D;
    let header = row![
        text(t!("Vorschau:")).size(10).style(muted),
        Space::new().width(Fill),
        button(text(t!("2D layer buildup")).size(10))
            .style(if is_2d { button::primary } else { button::secondary })
            .padding([2, 8])
            .on_press(Message::Aec(AecMessage::AecWallStyleManagerSetPreviewMode(
                AecWallPreviewMode::CrossSection2D,
            ))),
        button(text(t!("3D")).size(10))
            .style(if is_3d { button::primary } else { button::secondary })
            .padding([2, 8])
            .on_press(Message::Aec(AecMessage::AecWallStyleManagerSetPreviewMode(
                AecWallPreviewMode::Model3D,
            ))),
    ]
    .spacing(6)
    .align_y(iced::Center);
    column![
        header,
        container(iced::widget::canvas(WallPreviewCanvas { layers, mode, selected }).width(Fill).height(170)).style(
            |theme: &Theme| container::Style {
                background: Some(theme.palette().background.neutral.color.scale_alpha(0.15).into()),
                border: Border {
                    color: theme.palette().background.neutral.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            }
        ),
    ]
    .spacing(4)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(material: &str, thickness: &str) -> AecLayerBuffer {
        AecLayerBuffer {
            material_id: material.into(),
            thickness: thickness.into(),
            function: String::new(),
            axis_offset: String::new(),
            bottom_offset: String::new(),
            top_offset: String::new(),
            layer_override: String::new(),
            hatch_override: String::new(),
            role_tag: String::new(),
            layer_id: None,
        }
    }

    #[test]
    fn layers_use_thickness_with_fallback_for_formulas() {
        let bufs = [buf("a", "1,5"), buf("b", "BB*0.5"), buf("c", "30")];
        let layers = preview_layers(&bufs, &|id| (id == "a").then_some((0xff0000, "ANSI31".to_string())));
        assert_eq!(layers[0].thickness_cm, 1.5);
        assert_eq!(layers[0].color, 0xff0000);
        assert_eq!(layers[0].hatch, "ANSI31");
        assert_eq!(layers[1].thickness_cm, FALLBACK_THICKNESS_CM);
        assert_eq!(layers[2].color, 0x808080);
    }

    #[test]
    fn rects_are_contiguous_and_proportional() {
        let layers = WallPreviewLayer::stack(&[10.0, 30.0]);
        let b = Rectangle { x: 0.0, y: 0.0, width: 432.0, height: 100.0 };
        let r = layer_rects_2d(&layers, b);
        assert_eq!(r.len(), 2);
        assert!((r[0].x + r[0].width - r[1].x).abs() < 1e-3);
        assert!((r[1].width / r[0].width - 3.0).abs() < 1e-3);
    }

    #[test]
    fn empty_layers_give_no_rects() {
        let b = Rectangle { x: 0.0, y: 0.0, width: 100.0, height: 100.0 };
        assert!(layer_rects_2d(&[], b).is_empty());
    }

    #[test]
    fn preview_mode_switches_via_message() {
        let mut app = crate::app::OpenCADStudio::new_for_test();
        assert_eq!(app.aec.styles.wall_preview_mode, AecWallPreviewMode::CrossSection2D);
        let _ = app.update_aec(AecMessage::AecWallStyleManagerSetPreviewMode(
            AecWallPreviewMode::Model3D,
        ));
        assert_eq!(app.aec.styles.wall_preview_mode, AecWallPreviewMode::Model3D);
        let _ = app.update_aec(AecMessage::AecWallStyleManagerSetPreviewMode(
            AecWallPreviewMode::CrossSection2D,
        ));
        assert_eq!(app.aec.styles.wall_preview_mode, AecWallPreviewMode::CrossSection2D);
    }

    #[test]
    fn hit_test_2d_picks_layer_under_point() {
        let layers = WallPreviewLayer::stack(&[10.0, 30.0]);
        let b = Rectangle { x: 0.0, y: 0.0, width: 432.0, height: 100.0 };
        let r = layer_rects_2d(&layers, b);
        let mid = |r: &Rectangle| Point::new(r.x + r.width * 0.5, r.y + r.height * 0.5);
        let m = AecWallPreviewMode::CrossSection2D;
        assert_eq!(layer_at(&layers, m, b, mid(&r[0])), Some(0));
        assert_eq!(layer_at(&layers, m, b, mid(&r[1])), Some(1));
        assert_eq!(layer_at(&layers, m, b, Point::new(1.0, 1.0)), None);
    }

    #[test]
    fn hit_test_3d_picks_layer_top_face() {
        let layers = WallPreviewLayer::stack(&[10.0, 30.0]);
        let b = Rectangle { x: 0.0, y: 0.0, width: 400.0, height: 300.0 };
        let faces = faces_3d(&layers, b);
        let centre = |q: &[Point; 4]| {
            Point::new(
                q.iter().map(|p| p.x).sum::<f32>() / 4.0,
                q.iter().map(|p| p.y).sum::<f32>() / 4.0,
            )
        };
        let m = AecWallPreviewMode::Model3D;
        assert_eq!(layer_at(&layers, m, b, centre(&faces[0][2])), Some(0));
        assert_eq!(layer_at(&layers, m, b, centre(&faces[1][2])), Some(1));
        assert_eq!(layer_at(&layers, m, b, Point::new(0.0, 0.0)), None);
    }

    #[test]
    fn explicit_axis_offsets_position_layers_and_override_hatch() {
        let mut a = buf("a", "10");
        a.axis_offset = "-5".into();
        a.hatch_override = "BRICK".into();
        let mut b = buf("b", "20");
        b.axis_offset = "5".into();
        b.bottom_offset = "10".into();
        b.top_offset = "20".into();
        let layers = preview_layers(&[a, b], &|_| Some((0, "ANSI31".to_string())));
        assert_eq!(layers[0].start_cm, -5.0);
        assert_eq!(layers[1].start_cm, 5.0);
        assert_eq!(layers[0].hatch, "BRICK");
        assert_eq!(layers[1].hatch, "ANSI31");
        assert_eq!((layers[1].bottom_cm, layers[1].top_cm), (10.0, 20.0));
        let b = Rectangle { x: 0.0, y: 0.0, width: 400.0, height: 200.0 };
        let ax = axis_x_2d(&layers, b).unwrap();
        let r = layer_rects_2d(&layers, b);
        assert!(r[0].x < ax && ax < r[0].x + r[0].width);
    }

    #[test]
    fn section_is_narrow() {
        let layers = WallPreviewLayer::stack(&[10.0, 30.0]);
        let b = Rectangle { x: 0.0, y: 0.0, width: 600.0, height: 200.0 };
        let r = layer_rects_2d(&layers, b);
        let total = r[1].x + r[1].width - r[0].x;
        assert!(total <= 600.0 * WIDTH_FRACTION_2D + 0.1);
    }

    #[test]
    fn vertical_offsets_shorten_3d_layer() {
        let mut layers = WallPreviewLayer::stack(&[10.0, 10.0]);
        layers[1].bottom_cm = 20.0;
        layers[1].top_cm = 20.0;
        let b = Rectangle { x: 0.0, y: 0.0, width: 400.0, height: 300.0 };
        let f = faces_3d(&layers, b);
        let height = |q: &[Point; 4]| (q[0].y - q[3].y).abs();
        assert!(height(&f[1][0]) < height(&f[0][0]) - 1.0);
    }

    #[test]
    fn hatch_kinds_and_segments() {
        assert_eq!(hatch_kind(""), None);
        assert_eq!(hatch_kind("SOLID"), Some(HatchKind::Solid));
        assert_eq!(hatch_kind("ansi31"), Some(HatchKind::Diagonal));
        assert_eq!(hatch_kind("ANSI37"), Some(HatchKind::Cross));
        let r = Rectangle { x: 0.0, y: 0.0, width: 30.0, height: 40.0 };
        assert!(!hatch_segments(HatchKind::Diagonal, r, 7.0).is_empty());
        assert!(hatch_segments(HatchKind::Cross, r, 7.0).len() > hatch_segments(HatchKind::Diagonal, r, 7.0).len());
        assert!(hatch_segments(HatchKind::Solid, r, 7.0).is_empty());
    }
}
