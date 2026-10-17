//! DIN 1356 Architecture drawing symbols and section annotations.
//!
//! Provides generation of standardized section line paths, viewing direction arrows,
//! section identifier labels ("A - A"), and vertical level elevation markers (Höhenkoten).

use acadrust::entities::{Line, LwPolyline, LwVertex, MText};
use acadrust::types::{Vector2 as CadVec2, Vector3 as CadVec3};
use acadrust::EntityType;

use crate::modules::aec::engine::views::viewpoint::SectionPath;
use crate::modules::aec::engine::views::{Point2, Vector2};

/// Configuration for DIN 1356 section annotations in plan view.
#[derive(Clone, Debug, PartialEq)]
pub struct Din1356SectionSymbol {
    pub label: String,
    pub path: SectionPath,
    pub arrow_size: f64,
    pub text_height: f64,
    pub end_extension: f64,
}

impl Din1356SectionSymbol {
    pub fn new(label: impl Into<String>, path: SectionPath) -> Self {
        Self {
            label: label.into(),
            path,
            arrow_size: 0.40,
            text_height: 0.35,
            end_extension: 0.50,
        }
    }

    /// Generates wire preview segments for real-time viewport drawing.
    pub fn generate_preview_wires(&self) -> Vec<(Point2, Point2)> {
        let mut wires = Vec::new();
        let segments = self.path.segments();
        if segments.is_empty() {
            return wires;
        }

        // Draw main cut segments
        for seg in &segments {
            wires.push((seg.start, seg.end));
        }

        // Add start and end extension and arrows
        if let Some(first) = segments.first() {
            if let Ok(dir) = first.direction() {
                let start_ext = Point2::new(
                    first.start.x_m - dir.x * self.end_extension,
                    first.start.y_m - dir.y * self.end_extension,
                );
                wires.push((start_ext, first.start));

                // Direction arrow pointing left (normal = (-dir.y, dir.x))
                let normal = Vector2::new(-dir.y, dir.x);
                let arrow_tip = Point2::new(
                    start_ext.x_m + normal.x * self.arrow_size,
                    start_ext.y_m + normal.y * self.arrow_size,
                );
                let barb1 = Point2::new(
                    arrow_tip.x_m - (dir.x + normal.x) * (self.arrow_size * 0.4),
                    arrow_tip.y_m - (dir.y + normal.y) * (self.arrow_size * 0.4),
                );
                let barb2 = Point2::new(
                    arrow_tip.x_m + (dir.x - normal.x) * (self.arrow_size * 0.4),
                    arrow_tip.y_m + (dir.y - normal.y) * (self.arrow_size * 0.4),
                );
                wires.push((start_ext, arrow_tip));
                wires.push((arrow_tip, barb1));
                wires.push((arrow_tip, barb2));
            }
        }

        if let Some(last) = segments.last() {
            if let Ok(dir) = last.direction() {
                let end_ext = Point2::new(
                    last.end.x_m + dir.x * self.end_extension,
                    last.end.y_m + dir.y * self.end_extension,
                );
                wires.push((last.end, end_ext));

                let normal = Vector2::new(-dir.y, dir.x);
                let arrow_tip = Point2::new(
                    end_ext.x_m + normal.x * self.arrow_size,
                    end_ext.y_m + normal.y * self.arrow_size,
                );
                let barb1 = Point2::new(
                    arrow_tip.x_m - (dir.x + normal.x) * (self.arrow_size * 0.4),
                    arrow_tip.y_m - (dir.y + normal.y) * (self.arrow_size * 0.4),
                );
                let barb2 = Point2::new(
                    arrow_tip.x_m + (dir.x - normal.x) * (self.arrow_size * 0.4),
                    arrow_tip.y_m + (dir.y - normal.y) * (self.arrow_size * 0.4),
                );
                wires.push((end_ext, arrow_tip));
                wires.push((arrow_tip, barb1));
                wires.push((arrow_tip, barb2));
            }
        }

        wires
    }

    /// Generates native CAD entities to persist the DIN 1356 section line and arrows into the drawing.
    pub fn generate_cad_entities(&self, layer: &str) -> Vec<EntityType> {
        let mut entities = Vec::new();
        let segments = self.path.segments();
        if segments.is_empty() {
            return entities;
        }

        // 1. Build main cut polyline
        let mut cut_pline = LwPolyline::default();
        cut_pline.common.layer = layer.to_string();

        for (idx, seg) in segments.iter().enumerate() {
            if idx == 0 {
                cut_pline.vertices.push(LwVertex {
                    location: CadVec2::new(seg.start.x_m, seg.start.y_m),
                    bulge: 0.0,
                    start_width: 0.0,
                    end_width: 0.0,
                    vertex_id: 0,
                });
            }
            cut_pline.vertices.push(LwVertex {
                location: CadVec2::new(seg.end.x_m, seg.end.y_m),
                bulge: 0.0,
                start_width: 0.0,
                end_width: 0.0,
                vertex_id: 0,
            });
        }
        entities.push(EntityType::LwPolyline(cut_pline));

        // 2. Start symbol and label
        if let Some(first) = segments.first() {
            if let Ok(dir) = first.direction() {
                let start_ext = Point2::new(
                    first.start.x_m - dir.x * self.end_extension,
                    first.start.y_m - dir.y * self.end_extension,
                );
                let mut ext_line = Line::default();
                ext_line.common.layer = layer.to_string();
                ext_line.start = CadVec3::new(start_ext.x_m, start_ext.y_m, 0.0);
                ext_line.end = CadVec3::new(first.start.x_m, first.start.y_m, 0.0);
                entities.push(EntityType::Line(ext_line));

                let normal = Vector2::new(-dir.y, dir.x);
                let arrow_tip = Point2::new(
                    start_ext.x_m + normal.x * self.arrow_size,
                    start_ext.y_m + normal.y * self.arrow_size,
                );

                let mut arr_line = Line::default();
                arr_line.common.layer = layer.to_string();
                arr_line.start = CadVec3::new(start_ext.x_m, start_ext.y_m, 0.0);
                arr_line.end = CadVec3::new(arrow_tip.x_m, arrow_tip.y_m, 0.0);
                entities.push(EntityType::Line(arr_line));

                // Section Label MText
                let mut mtext = MText::default();
                mtext.common.layer = layer.to_string();
                mtext.insertion_point = CadVec3::new(
                    arrow_tip.x_m + normal.x * 0.15,
                    arrow_tip.y_m + normal.y * 0.15,
                    0.0,
                );
                mtext.height = self.text_height;
                mtext.value = self.label.clone();
                entities.push(EntityType::MText(mtext));
            }
        }

        // 3. End symbol and label
        if let Some(last) = segments.last() {
            if let Ok(dir) = last.direction() {
                let end_ext = Point2::new(
                    last.end.x_m + dir.x * self.end_extension,
                    last.end.y_m + dir.y * self.end_extension,
                );
                let mut ext_line = Line::default();
                ext_line.common.layer = layer.to_string();
                ext_line.start = CadVec3::new(last.end.x_m, last.end.y_m, 0.0);
                ext_line.end = CadVec3::new(end_ext.x_m, end_ext.y_m, 0.0);
                entities.push(EntityType::Line(ext_line));

                let normal = Vector2::new(-dir.y, dir.x);
                let arrow_tip = Point2::new(
                    end_ext.x_m + normal.x * self.arrow_size,
                    end_ext.y_m + normal.y * self.arrow_size,
                );

                let mut arr_line = Line::default();
                arr_line.common.layer = layer.to_string();
                arr_line.start = CadVec3::new(end_ext.x_m, end_ext.y_m, 0.0);
                arr_line.end = CadVec3::new(arrow_tip.x_m, arrow_tip.y_m, 0.0);
                entities.push(EntityType::Line(arr_line));

                let mut mtext = MText::default();
                mtext.common.layer = layer.to_string();
                mtext.insertion_point = CadVec3::new(
                    arrow_tip.x_m + normal.x * 0.15,
                    arrow_tip.y_m + normal.y * 0.15,
                    0.0,
                );
                mtext.height = self.text_height;
                mtext.value = self.label.clone();
                entities.push(EntityType::MText(mtext));
            }
        }

        entities
    }
}

/// Generates a standard DIN 1356 elevation height marker (Höhenkote, e.g. +2.80 or ±0.00).
pub fn generate_din1356_level_marker(
    origin: Point2,
    elevation_m: f64,
    prefix: Option<&str>,
    layer: &str,
) -> Vec<EntityType> {
    let mut entities = Vec::new();
    let size = 0.25;

    // Triangle marker pointing down to origin
    let mut tri = LwPolyline::default();
    tri.common.layer = layer.to_string();
    tri.is_closed = true;
    tri.vertices.push(LwVertex {
        location: CadVec2::new(origin.x_m, origin.y_m),
        bulge: 0.0,
        start_width: 0.0,
        end_width: 0.0,
        vertex_id: 0,
    });
    tri.vertices.push(LwVertex {
        location: CadVec2::new(origin.x_m - size, origin.y_m + size),
        bulge: 0.0,
        start_width: 0.0,
        end_width: 0.0,
        vertex_id: 0,
    });
    tri.vertices.push(LwVertex {
        location: CadVec2::new(origin.x_m + size, origin.y_m + size),
        bulge: 0.0,
        start_width: 0.0,
        end_width: 0.0,
        vertex_id: 0,
    });
    entities.push(EntityType::LwPolyline(tri));

    // Baseline marker
    let mut line = Line::default();
    line.common.layer = layer.to_string();
    line.start = CadVec3::new(origin.x_m - size * 1.5, origin.y_m, 0.0);
    line.end = CadVec3::new(origin.x_m + size * 1.5, origin.y_m, 0.0);
    entities.push(EntityType::Line(line));

    // Text formatting
    let sign = if elevation_m.abs() < 1e-4 {
        "±"
    } else if elevation_m > 0.0 {
        "+"
    } else {
        "-"
    };
    let text_val = format!(
        "{}{}{:.2}",
        prefix.unwrap_or(""),
        sign,
        elevation_m.abs()
    );

    let mut mtext = MText::default();
    mtext.common.layer = layer.to_string();
    mtext.insertion_point = CadVec3::new(origin.x_m + size * 1.2, origin.y_m + size * 0.5, 0.0);
    mtext.height = 0.25;
    mtext.value = text_val;
    entities.push(EntityType::MText(mtext));

    entities
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_din1356_section_symbol_wires() {
        let p1 = Point2::new(0.0, 0.0);
        let p2 = Point2::new(10.0, 0.0);
        let path = SectionPath::straight(p1, p2);
        let symbol = Din1356SectionSymbol::new("A - A", path);

        let wires = symbol.generate_preview_wires();
        assert!(!wires.is_empty());
        assert!(wires.len() >= 5); // cut line + 2 extensions + arrows
    }

    #[test]
    fn test_din1356_cad_entities() {
        let p1 = Point2::new(0.0, 0.0);
        let p2 = Point2::new(5.0, 5.0);
        let path = SectionPath::straight(p1, p2);
        let symbol = Din1356SectionSymbol::new("B - B", path);

        let entities = symbol.generate_cad_entities("AEC_ANNOTATION");
        assert!(entities.len() >= 5); // polyline + lines + text
    }

    #[test]
    fn test_din1356_level_marker() {
        let entities = generate_din1356_level_marker(Point2::new(0.0, 2.8), 2.8, Some("OKFF "), "AEC_ANNOTATION");
        assert_eq!(entities.len(), 3); // triangle, line, mtext
    }
}
