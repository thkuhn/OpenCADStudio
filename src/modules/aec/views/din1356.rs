//! DIN 1356 Architecture drawing symbols and section annotations.
//!
//! Provides generation of standardized section line paths, viewing direction arrows,
//! section identifier labels ("A - A"), and vertical level elevation markers (Höhenkoten).

use acadrust::entities::{Line, LwPolyline, LwVertex, MText};
use acadrust::types::{Vector2 as CadVec2, Vector3 as CadVec3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, EntityType, Handle};
use codec::entities::MultiLeader;
use serde::{Deserialize, Serialize};

use crate::modules::aec::engine::owner_index;
use crate::modules::aec::engine::views::viewpoint::{SectionPath, SectionSegment};
use crate::modules::aec::engine::views::{Point2, Vector2, ViewpointId};
use crate::modules::aec::engine::xdata::{
    aec_value_as_handle, ensure_app_id, write_aec_record, AEC_APPID,
};
use crate::modules::aec::properties::{
    SECTION_MARKER_DEPTH_GRIP, SECTION_MARKER_END_GRIP, SECTION_MARKER_FLIP_GRIP,
    SECTION_MARKER_JOG_BASE, SECTION_MARKER_START_GRIP,
};
use crate::scene::model::object::GripApply;

/// Configuration for DIN 1356 section annotations in plan view.
#[derive(Clone, Debug, PartialEq)]
pub struct Din1356SectionSymbol {
    pub label: String,
    pub path: SectionPath,
    pub arrow_size: f64,
    pub text_height: f64,
    pub end_extension: f64,
    pub flip_direction: bool,
}

impl Din1356SectionSymbol {
    pub fn new(label: impl Into<String>, path: SectionPath) -> Self {
        Self {
            label: label.into(),
            path,
            arrow_size: 0.40,
            text_height: 0.35,
            end_extension: 0.50,
            flip_direction: false,
        }
    }

    pub fn with_flip(mut self, flip: bool) -> Self {
        self.flip_direction = flip;
        self
    }

    fn label_leader(&self, layer: &str, arrow_tip: Point2, normal: Vector2) -> EntityType {
        let arrow = CadVec3::new(arrow_tip.x_m, arrow_tip.y_m, 0.0);
        let elbow = CadVec3::new(
            arrow_tip.x_m + normal.x * 0.15,
            arrow_tip.y_m + normal.y * 0.15,
            0.0,
        );
        let mut leader = MultiLeader::with_text(&self.label, elbow, vec![arrow]);
        leader.common.layer = layer.to_string();
        leader.text_height = self.text_height;
        leader.context.text_height = self.text_height;
        leader.arrowhead_size = 0.0;
        leader.context.arrowhead_size = 0.0;
        EntityType::MultiLeader(Box::new(leader))
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

                // Direction arrow pointing in viewing direction
                let normal = if !self.flip_direction {
                    Vector2::new(-dir.y, dir.x)
                } else {
                    Vector2::new(dir.y, -dir.x)
                };
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

                let normal = if !self.flip_direction {
                    Vector2::new(-dir.y, dir.x)
                } else {
                    Vector2::new(dir.y, -dir.x)
                };
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

                let normal = if !self.flip_direction {
                    Vector2::new(-dir.y, dir.x)
                } else {
                    Vector2::new(dir.y, -dir.x)
                };
                let arrow_tip = Point2::new(
                    start_ext.x_m + normal.x * self.arrow_size,
                    start_ext.y_m + normal.y * self.arrow_size,
                );

                let mut arr_line = Line::default();
                arr_line.common.layer = layer.to_string();
                arr_line.start = CadVec3::new(start_ext.x_m, start_ext.y_m, 0.0);
                arr_line.end = CadVec3::new(arrow_tip.x_m, arrow_tip.y_m, 0.0);
                entities.push(EntityType::Line(arr_line));

                entities.push(self.label_leader(layer, arrow_tip, normal));
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

                let normal = if !self.flip_direction {
                    Vector2::new(-dir.y, dir.x)
                } else {
                    Vector2::new(dir.y, -dir.x)
                };
                let arrow_tip = Point2::new(
                    end_ext.x_m + normal.x * self.arrow_size,
                    end_ext.y_m + normal.y * self.arrow_size,
                );

                let mut arr_line = Line::default();
                arr_line.common.layer = layer.to_string();
                arr_line.start = CadVec3::new(end_ext.x_m, end_ext.y_m, 0.0);
                arr_line.end = CadVec3::new(arrow_tip.x_m, arrow_tip.y_m, 0.0);
                entities.push(EntityType::Line(arr_line));

                entities.push(self.label_leader(layer, arrow_tip, normal));
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
    entities.push(EntityType::MText(Box::new(mtext)));

    entities
}

pub const AEC_SECTION_MARKER_TAG: &str = "AEC_SECTION_MARKER";
pub const AEC_ELEVATION_MARKER_TAG: &str = "AEC_ELEVATION_MARKER";
/// Owner back-reference written on marker child CAD entities (arrows, labels).
pub const AEC_SECTION_MARKER_REP_TAG: &str = "SECTION_MARKER_REP";

/// Parametric AEC carrier object for DIN 1356 Section and Elevation markers.
/// Coupled to a `Viewpoint` and optionally to a `DerivedViewCarrier`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Din1356SectionMarker {
    pub viewpoint_id: ViewpointId,
    pub carrier_handle: Option<Handle>,
    pub label: String,
    pub path: SectionPath,
    pub far_depth: Option<f64>,
    pub z_min: f64,
    pub z_max: f64,
    pub flip_direction: bool,
    #[serde(default)]
    pub is_elevation: bool,
}

impl Din1356SectionMarker {
    pub fn new_section(
        label: impl Into<String>,
        path: SectionPath,
        viewpoint_id: ViewpointId,
        carrier_handle: Option<Handle>,
    ) -> Self {
        Self {
            viewpoint_id,
            carrier_handle,
            label: label.into(),
            path,
            far_depth: Some(5.0),
            z_min: 0.0,
            z_max: 3.0,
            flip_direction: false,
            is_elevation: false,
        }
    }

    pub fn new_elevation(
        label: impl Into<String>,
        baseline_start: Point2,
        baseline_end: Point2,
        viewpoint_id: ViewpointId,
        carrier_handle: Option<Handle>,
    ) -> Self {
        Self {
            viewpoint_id,
            carrier_handle,
            label: label.into(),
            path: SectionPath::straight(baseline_start, baseline_end),
            far_depth: Some(15.0),
            z_min: 0.0,
            z_max: 3.0,
            flip_direction: false,
            is_elevation: true,
        }
    }

    pub fn with_depth(mut self, depth: Option<f64>) -> Self {
        self.far_depth = depth;
        self
    }

    pub fn with_z_range(mut self, z_min: f64, z_max: f64) -> Self {
        self.z_min = z_min;
        self.z_max = z_max;
        self
    }

    pub fn with_flip(mut self, flip: bool) -> Self {
        self.flip_direction = flip;
        self
    }

    pub fn tag(&self) -> &'static str {
        if self.is_elevation {
            AEC_ELEVATION_MARKER_TAG
        } else {
            AEC_SECTION_MARKER_TAG
        }
    }

    pub fn layer_name(&self) -> &'static str {
        if self.is_elevation {
            "AEC_ELEVATION_MARK"
        } else {
            "AEC_SECTION_MARK"
        }
    }
}

/// Writes the `Din1356SectionMarker` record into entity's XDATA.
pub fn write_section_marker(
    doc: &mut CadDocument,
    handle: Handle,
    marker: &Din1356SectionMarker,
) -> bool {
    ensure_app_id(doc);
    let app_handle = doc.app_ids.get(AEC_APPID).map(|a| a.handle.value());

    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.add_value(XDataValue::String(marker.tag().to_string()));
    rec.add_value(XDataValue::String(marker.viewpoint_id.as_uuid().to_string()));
    rec.add_value(XDataValue::String(
        marker.carrier_handle.map(|h| h.to_string()).unwrap_or_default(),
    ));
    rec.add_value(XDataValue::String(marker.label.clone()));
    rec.add_value(XDataValue::Real(marker.far_depth.unwrap_or(-1.0)));
    rec.add_value(XDataValue::Real(marker.z_min));
    rec.add_value(XDataValue::Real(marker.z_max));
    rec.add_value(XDataValue::Integer32(if marker.flip_direction { 1 } else { 0 }));

    let Ok(json_blob) = serde_json::to_string(marker) else {
        return false;
    };
    rec.add_value(XDataValue::String(json_blob));

    let Some(entity) = doc.get_entity_mut(handle) else {
        return false;
    };

    let xd = &mut entity.common_mut().extended_data;
    let kept: Vec<_> = xd
        .records()
        .iter()
        .filter(|r| {
            if r.application_name != AEC_APPID {
                return true;
            }
            !matches!(
                r.values.first(),
                Some(XDataValue::String(s))
                    if s == AEC_SECTION_MARKER_TAG || s == AEC_ELEVATION_MARKER_TAG
            )
        })
        .cloned()
        .collect();
    xd.clear();
    for r in kept {
        xd.add_record(r);
    }
    xd.add_record(rec);

    if let Some(ah) = app_handle {
        xd.raw_dwg_eed.retain(|(a, _)| *a != ah);
    }
    true
}

/// Reads `Din1356SectionMarker` from entity's XDATA.
pub fn read_section_marker(
    _doc: &CadDocument,
    entity: &EntityType,
) -> Option<Din1356SectionMarker> {
    for r in entity.common().extended_data.records() {
        if r.application_name != AEC_APPID {
            continue;
        }
        let tag = match r.values.first() {
            Some(XDataValue::String(s)) if s == AEC_SECTION_MARKER_TAG || s == AEC_ELEVATION_MARKER_TAG => s.as_str(),
            _ => continue,
        };

        // Try reading JSON blob from last value first
        if let Some(XDataValue::String(json_str)) = r.values.last() {
            if let Ok(marker) = serde_json::from_str::<Din1356SectionMarker>(json_str) {
                return Some(marker);
            }
        }

        // Fallback to positional fields
        let vp_str = match r.values.get(1) {
            Some(XDataValue::String(s)) => s,
            _ => continue,
        };
        let vp_id = ViewpointId::from_uuid(uuid::Uuid::parse_str(vp_str).ok()?);
        let carrier_handle = match r.values.get(2) {
            Some(XDataValue::String(s)) => s.parse::<u64>().ok().map(Handle::new),
            _ => None,
        };
        let label = match r.values.get(3) {
            Some(XDataValue::String(s)) => s.clone(),
            _ => "A - A".to_string(),
        };
        let far_depth = match r.values.get(4) {
            Some(XDataValue::Real(d)) if *d >= 0.0 => Some(*d),
            _ => None,
        };
        let z_min = match r.values.get(5) {
            Some(XDataValue::Real(z)) => *z,
            _ => 0.0,
        };
        let z_max = match r.values.get(6) {
            Some(XDataValue::Real(z)) => *z,
            _ => 3.0,
        };
        let flip_direction = match r.values.get(7) {
            Some(XDataValue::Integer32(i)) => *i != 0,
            _ => false,
        };

        let path = if let EntityType::LwPolyline(pl) = entity {
            if pl.vertices.len() == 2 {
                let v0 = &pl.vertices[0].location;
                let v1 = &pl.vertices[1].location;
                SectionPath::straight(Point2::new(v0.x, v0.y), Point2::new(v1.x, v1.y))
            } else if pl.vertices.len() > 2 {
                let mut segs = Vec::new();
                for w in pl.vertices.windows(2) {
                    segs.push(SectionSegment::new(
                        Point2::new(w[0].location.x, w[0].location.y),
                        Point2::new(w[1].location.x, w[1].location.y),
                        false,
                    ));
                }
                SectionPath::jogged(segs)
            } else {
                SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0))
            }
        } else {
            SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0))
        };

        return Some(Din1356SectionMarker {
            viewpoint_id: vp_id,
            carrier_handle,
            label,
            path,
            far_depth,
            z_min,
            z_max,
            flip_direction,
            is_elevation: tag == AEC_ELEVATION_MARKER_TAG,
        });
    }
    None
}

/// Creates an associative DIN 1356 Section / Elevation Marker entity in `doc`,
/// along with child CAD entities (direction arrows, extensions, text labels).
pub fn create_section_marker(
    doc: &mut CadDocument,
    marker: &Din1356SectionMarker,
) -> Handle {
    ensure_app_id(doc);

    // 1. Create main cut polyline entity
    let mut cut_pline = LwPolyline::default();
    cut_pline.common.layer = marker.layer_name().to_string();

    let segments = marker.path.segments();
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

    let marker_handle = doc.allocate_handle();
    cut_pline.common.handle = marker_handle;
    let _ = doc.add_entity(EntityType::LwPolyline(cut_pline));

    // 2. Write marker record
    write_section_marker(doc, marker_handle, marker);

    // 3. Generate child entities (arrows, labels) with owner back-references.
    rebuild_section_marker_children(doc, marker_handle, marker);
    marker_handle
}

/// Synchronizes the CAD entities of a `Din1356SectionMarker` when its properties or geometry change.
pub fn sync_section_marker_cad_entities(
    doc: &mut CadDocument,
    marker_handle: Handle,
    marker: &Din1356SectionMarker,
) -> bool {
    ensure_app_id(doc);

    // 1. Remove old child entities
    for child in owner_index::children_of(doc, marker_handle) {
        let _ = doc.remove_entity(child);
    }

    // 2. Update polyline carrier vertices
    let segments = marker.path.segments();
    if let Some(EntityType::LwPolyline(pl)) = doc.get_entity_mut(marker_handle) {
        pl.vertices.clear();
        for (idx, seg) in segments.iter().enumerate() {
            if idx == 0 {
                pl.vertices.push(LwVertex {
                    location: CadVec2::new(seg.start.x_m, seg.start.y_m),
                    bulge: 0.0,
                    start_width: 0.0,
                    end_width: 0.0,
                    vertex_id: 0,
                });
            }
            pl.vertices.push(LwVertex {
                location: CadVec2::new(seg.end.x_m, seg.end.y_m),
                bulge: 0.0,
                start_width: 0.0,
                end_width: 0.0,
                vertex_id: 0,
            });
        }
        pl.common.layer = marker.layer_name().to_string();
    }

    // 3. Write updated XDATA
    write_section_marker(doc, marker_handle, marker);

    // 4. Generate new child entities with owner back-references.
    rebuild_section_marker_children(doc, marker_handle, marker);
    true
}

/// Tags `child` as a display child of the section/elevation marker at `owner`.
pub fn write_section_marker_child_tag(doc: &mut CadDocument, child: Handle, owner: Handle) {
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.add_value(XDataValue::String(AEC_SECTION_MARKER_REP_TAG.to_string()));
    rec.add_value(XDataValue::Handle(owner));
    write_aec_record(doc, child, rec);
}

/// Extracts the owner marker handle from a `SECTION_MARKER_REP` child entity.
pub fn section_marker_owner_from_entity(entity: &EntityType) -> Option<Handle> {
    for record in entity.common().extended_data.records() {
        if record.application_name != AEC_APPID {
            continue;
        }
        match record.values.first() {
            Some(XDataValue::String(s)) if s == AEC_SECTION_MARKER_REP_TAG => {
                return record.values.get(1).and_then(aec_value_as_handle);
            }
            _ => {}
        }
    }
    None
}

fn rebuild_section_marker_children(
    doc: &mut CadDocument,
    marker_handle: Handle,
    marker: &Din1356SectionMarker,
) -> Vec<Handle> {
    let sym = Din1356SectionSymbol::new(&marker.label, marker.path.clone())
        .with_flip(marker.flip_direction);
    let generated = sym.generate_cad_entities(marker.layer_name());

    let mut child_handles = Vec::new();
    // Skip index 0: that is the cut polyline already stored as the marker carrier.
    for mut child in generated.into_iter().skip(1) {
        let ch = doc.allocate_handle();
        child.common_mut().handle = ch;
        child.common_mut().layer = marker.layer_name().to_string();
        let _ = doc.add_entity(child);
        write_section_marker_child_tag(doc, ch, marker_handle);
        child_handles.push(ch);
    }
    owner_index::set_children(doc, marker_handle, &child_handles);
    child_handles
}

/// Finds the section marker entity associated with a given `ViewpointId`.
pub fn find_section_marker_by_viewpoint(
    doc: &CadDocument,
    viewpoint_id: ViewpointId,
) -> Option<(Handle, Din1356SectionMarker)> {
    for ent in doc.entities() {
        if let Some(marker) = read_section_marker(doc, ent) {
            if marker.viewpoint_id == viewpoint_id {
                return Some((ent.common().handle, marker));
            }
        }
    }
    None
}

/// Finds the section marker entity associated with a given `DerivedViewCarrier` handle.
pub fn find_section_marker_by_carrier(
    doc: &CadDocument,
    carrier_handle: Handle,
) -> Option<(Handle, Din1356SectionMarker)> {
    for ent in doc.entities() {
        if let Some(marker) = read_section_marker(doc, ent) {
            if marker.carrier_handle == Some(carrier_handle) {
                return Some((ent.common().handle, marker));
            }
        }
    }
    None
}

/// Synchronizes the marker's parameters with its associated `DerivedViewCarrier` and embedded `Viewpoint`.
/// If the carrier has `UpdatePolicy::Automatic`, regenerates its primitives immediately.
/// If `UpdatePolicy::Manual`, marks the carrier as outdated.
pub fn sync_marker_to_carrier(doc: &mut CadDocument, marker: &Din1356SectionMarker) -> bool {
    let display_config_library = crate::modules::aec::engine::library::DisplayConfigLibrary::seed_default_configs();
    let style_library = crate::modules::aec::engine::library::seed_default_library();
    sync_marker_to_carrier_with_libraries(
        doc,
        marker,
        &display_config_library,
        &style_library,
        None,
    )
}

pub fn sync_marker_to_carrier_with_libraries(
    doc: &mut CadDocument,
    marker: &Din1356SectionMarker,
    display_config_library: &crate::modules::aec::engine::library::DisplayConfigLibrary,
    style_library: &crate::modules::aec::engine::library::StyleLibrary,
    active_name: Option<&str>,
) -> bool {
    use crate::modules::aec::engine::views::carrier::{
        find_view_carrier_by_handle, find_view_carrier_by_viewpoint,
        read_view_carrier_definitions, write_view_carrier, write_view_carrier_definitions,
    };
    use crate::modules::aec::engine::views::model_view::UpdatePolicy;
    use crate::modules::aec::engine::views::update_ops::regenerate_or_mark_outdated_with_libraries;
    use crate::modules::aec::engine::views::viewpoint::{VerticalRange, ViewAxes, ViewKind, ViewOrigin};
    use crate::modules::aec::engine::views::{Metres, Point3, PositiveMetres};

    let carrier_handle = if let Some(h) = marker.carrier_handle {
        h
    } else if let Some(c) = find_view_carrier_by_viewpoint(doc, marker.viewpoint_id) {
        c.handle
    } else {
        return false;
    };

    let Some(mut carrier) = find_view_carrier_by_handle(doc, carrier_handle) else {
        return false;
    };

    let (mut vp, mut mv) = match read_view_carrier_definitions(doc, carrier_handle) {
        Some(defs) => defs,
        None => return false,
    };

    vp.name = marker.label.clone();
    mv.name = match &vp.kind {
        ViewKind::Section { .. } => format!("Section {}", marker.label),
        ViewKind::Elevation { .. } => format!("Elevation {}", marker.label),
        _ => marker.label.clone(),
    };
    if let (Ok(z_min), Ok(z_max)) = (Metres::new(marker.z_min), Metres::new(marker.z_max)) {
        if let Ok(vr) = VerticalRange::new(z_min, z_max) {
            vp.z_range = vr;
        }
    }

    match &mut vp.kind {
        ViewKind::Section { path, far_depth } => {
            *path = marker.path.clone();
            *far_depth = marker.far_depth.and_then(|d| PositiveMetres::new(d).ok());
            if let Some(first_seg) = marker.path.segments().first() {
                if let Ok(dir) = first_seg.direction() {
                    if let Ok(axes) = ViewAxes::section_with_side(dir, marker.flip_direction) {
                        vp.axes = axes;
                    }
                }
                vp.origin = ViewOrigin::World(Point3::new(
                    first_seg.start.x_m,
                    first_seg.start.y_m,
                    marker.z_min,
                ));
            }
        }
        ViewKind::Elevation { depth } => {
            depth.far = marker.far_depth.and_then(|d| PositiveMetres::new(d).ok());
            if let Some(first_seg) = marker.path.segments().first() {
                if let Ok(dir) = first_seg.direction() {
                    if let Ok(axes) = ViewAxes::elevation_with_side(dir, marker.flip_direction) {
                        vp.axes = axes;
                    }
                }
                vp.origin = ViewOrigin::World(Point3::new(
                    first_seg.start.x_m,
                    first_seg.start.y_m,
                    marker.z_min,
                ));
            }
        }
        _ => {}
    }

    write_view_carrier_definitions(doc, carrier_handle, &vp, &mv);

    match carrier.update_policy {
        UpdatePolicy::Automatic => {
            regenerate_or_mark_outdated_with_libraries(
                doc,
                carrier_handle,
                &vp,
                &mv,
                display_config_library,
                style_library,
                active_name,
            );
        }
        UpdatePolicy::Manual => {
            carrier.is_outdated = true;
            write_view_carrier(doc, &carrier);
        }
    }

    true
}

/// Result of applying a grip edit to a section or elevation marker.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionMarkerGripResult {
    pub marker: Din1356SectionMarker,
    pub preview_wires: Vec<(Point2, Point2)>,
}

/// Modifies the section marker entity on `doc` in-place according to `grip_id` and `apply`.
/// Updates the marker's XDATA and polyline vertices in-place, without modifying child entities
/// or regenerating the carrier view (which should only happen upon commit).
pub fn apply_section_marker_grip_pure(
    doc: &mut CadDocument,
    marker_handle: Handle,
    grip_id: usize,
    apply: &GripApply,
    orig_marker: Option<&Din1356SectionMarker>,
) -> Option<SectionMarkerGripResult> {
    let ent = doc.get_entity(marker_handle)?;
    let mut marker = read_section_marker(doc, ent)?;
    let segments = marker.path.segments();
    if segments.is_empty() {
        return None;
    }

    let first = &segments[0];
    let last = &segments[segments.len() - 1];

    match grip_id {
        SECTION_MARKER_START_GRIP => {
            let current_start = first.start;
            let new_start = match apply {
                GripApply::Absolute(pt) => Point2::new(pt.x, pt.y),
                GripApply::Translate(delta) => {
                    Point2::new(current_start.x_m + delta.x, current_start.y_m + delta.y)
                }
            };
            if new_start.distance_to(first.end) < 1e-4 {
                return None;
            }
            match &mut marker.path {
                SectionPath::Straight { start, .. } => {
                    *start = new_start;
                }
                SectionPath::Jogged { segments } => {
                    if let Some(first_seg) = segments.first_mut() {
                        first_seg.start = new_start;
                    }
                }
            }
            if let Some(EntityType::LwPolyline(pl)) = doc.get_entity_mut(marker_handle) {
                if let Some(v) = pl.vertices.first_mut() {
                    v.location = acadrust::types::Vector2::new(new_start.x_m, new_start.y_m);
                }
            }
        }
        SECTION_MARKER_END_GRIP => {
            let current_end = last.end;
            let new_end = match apply {
                GripApply::Absolute(pt) => Point2::new(pt.x, pt.y),
                GripApply::Translate(delta) => {
                    Point2::new(current_end.x_m + delta.x, current_end.y_m + delta.y)
                }
            };
            if new_end.distance_to(last.start) < 1e-4 {
                return None;
            }
            match &mut marker.path {
                SectionPath::Straight { end, .. } => {
                    *end = new_end;
                }
                SectionPath::Jogged { segments } => {
                    if let Some(last_seg) = segments.last_mut() {
                        last_seg.end = new_end;
                    }
                }
            }
            if let Some(EntityType::LwPolyline(pl)) = doc.get_entity_mut(marker_handle) {
                if let Some(v) = pl.vertices.last_mut() {
                    v.location = acadrust::types::Vector2::new(new_end.x_m, new_end.y_m);
                }
            }
        }
        SECTION_MARKER_DEPTH_GRIP => {
            let mid_x = (first.start.x_m + last.end.x_m) * 0.5;
            let mid_y = (first.start.y_m + last.end.y_m) * 0.5;
            let dir = first.direction().unwrap_or(Vector2::new(1.0, 0.0));
            let orig_flip = orig_marker
                .map(|m| m.flip_direction)
                .unwrap_or(marker.flip_direction);
            let normal = if !orig_flip {
                Vector2::new(-dir.y, dir.x)
            } else {
                Vector2::new(dir.y, -dir.x)
            };

            let new_depth = match apply {
                GripApply::Absolute(pt) => {
                    let to_target = Vector2::new(pt.x - mid_x, pt.y - mid_y);
                    to_target.dot(normal).max(0.1)
                }
                GripApply::Translate(delta) => {
                    let current_depth = marker.far_depth.unwrap_or(5.0);
                    let delta_proj = delta.x * normal.x + delta.y * normal.y;
                    (current_depth + delta_proj).max(0.1)
                }
            };
            marker.far_depth = Some(new_depth);
        }
        SECTION_MARKER_FLIP_GRIP => {
            let mid_x = (first.start.x_m + last.end.x_m) * 0.5;
            let mid_y = (first.start.y_m + last.end.y_m) * 0.5;
            let dir = first.direction().unwrap_or(Vector2::new(1.0, 0.0));
            let base_normal = Vector2::new(-dir.y, dir.x);
            let orig_flip = orig_marker
                .map(|m| m.flip_direction)
                .unwrap_or(marker.flip_direction);
            let orig_dir_normal = if !orig_flip {
                base_normal
            } else {
                -base_normal
            };
            let orig_pos = glam::DVec3::new(
                mid_x + orig_dir_normal.x * 1.0,
                mid_y + orig_dir_normal.y * 1.0,
                0.0,
            );
            match apply {
                GripApply::Absolute(pt) => {
                    let dist = (*pt - orig_pos).length();
                    if dist < 0.3 {
                        marker.flip_direction = !orig_flip;
                    } else {
                        let to_target = Vector2::new(pt.x - mid_x, pt.y - mid_y);
                        marker.flip_direction = to_target.dot(base_normal) < 0.0;
                    }
                }
                GripApply::Translate(delta) => {
                    let target = orig_pos + *delta;
                    let dist = delta.length();
                    if dist < 0.3 {
                        marker.flip_direction = !orig_flip;
                    } else {
                        let to_target = Vector2::new(target.x - mid_x, target.y - mid_y);
                        marker.flip_direction = to_target.dot(base_normal) < 0.0;
                    }
                }
            }
        }
        id if id >= SECTION_MARKER_JOG_BASE => {
            let jog_idx = id - SECTION_MARKER_JOG_BASE;
            if jog_idx >= segments.len().saturating_sub(1) {
                return None;
            }
            let current_jog = segments[jog_idx].end;
            let new_jog = match apply {
                GripApply::Absolute(pt) => Point2::new(pt.x, pt.y),
                GripApply::Translate(delta) => {
                    Point2::new(current_jog.x_m + delta.x, current_jog.y_m + delta.y)
                }
            };
            match &mut marker.path {
                SectionPath::Straight { .. } => return None,
                SectionPath::Jogged { segments } => {
                    if jog_idx < segments.len().saturating_sub(1) {
                        segments[jog_idx].end = new_jog;
                        segments[jog_idx + 1].start = new_jog;
                    }
                }
            }
            if let Some(EntityType::LwPolyline(pl)) = doc.get_entity_mut(marker_handle) {
                if jog_idx + 1 < pl.vertices.len() {
                    pl.vertices[jog_idx + 1].location =
                        acadrust::types::Vector2::new(new_jog.x_m, new_jog.y_m);
                }
            }
        }
        _ => return None,
    }

    write_section_marker(doc, marker_handle, &marker);

    let sym = Din1356SectionSymbol::new(&marker.label, marker.path.clone())
        .with_flip(marker.flip_direction);
    let mut preview_wires = sym.generate_preview_wires();
    if let Some(depth) = marker.far_depth {
        let segs = marker.path.segments();
        if let (Some(f), Some(l)) = (segs.first(), segs.last()) {
            let dir = f.direction().unwrap_or(Vector2::new(1.0, 0.0));
            let norm = if !marker.flip_direction {
                Vector2::new(-dir.y, dir.x)
            } else {
                Vector2::new(dir.y, -dir.x)
            };
            let off = norm * depth;
            let d_start = Point2::new(f.start.x_m + off.x, f.start.y_m + off.y);
            let d_end = Point2::new(l.end.x_m + off.x, l.end.y_m + off.y);
            preview_wires.push((f.start, d_start));
            preview_wires.push((d_start, d_end));
            preview_wires.push((d_end, l.end));
        }
    }

    Some(SectionMarkerGripResult {
        marker,
        preview_wires,
    })
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
        assert_eq!(
            entities
                .iter()
                .filter(|entity| matches!(entity, EntityType::MultiLeader(_)))
                .count(),
            2
        );
        assert!(!entities
            .iter()
            .any(|entity| matches!(entity, EntityType::MText(_))));
        assert_eq!(
            entities
                .iter()
                .filter(|entity| matches!(entity, EntityType::Line(_)))
                .count(),
            4,
            "both section direction arrows and extension lines remain"
        );
        let cut_line = entities
            .iter()
            .find_map(|entity| match entity {
                EntityType::LwPolyline(polyline) => Some(polyline),
                _ => None,
            })
            .expect("section cut polyline remains");
        assert_eq!(cut_line.vertices.len(), 2);
        assert_eq!(cut_line.vertices[0].location, CadVec2::new(0.0, 0.0));
        assert_eq!(cut_line.vertices[1].location, CadVec2::new(5.0, 5.0));
    }

    #[test]
    fn test_din1356_level_marker() {
        let entities = generate_din1356_level_marker(Point2::new(0.0, 2.8), 2.8, Some("OKFF "), "AEC_ANNOTATION");
        assert_eq!(entities.len(), 3); // triangle, line, mtext
    }

    #[test]
    fn test_section_marker_persistence() {
        let mut doc = CadDocument::new();
        let vp_id = ViewpointId::new();
        let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(12.0, 0.0));
        let marker = Din1356SectionMarker::new_section("A - A", path.clone(), vp_id, Some(Handle::new(42)))
            .with_depth(Some(4.5))
            .with_z_range(-0.5, 3.2)
            .with_flip(true);

        let handle = create_section_marker(&mut doc, &marker);
        assert_ne!(handle, Handle::new(0));

        let ent = doc.get_entity(handle).expect("marker entity exists");
        let read_back = read_section_marker(&doc, ent).expect("read marker from xdata");

        assert_eq!(read_back.viewpoint_id, vp_id);
        assert_eq!(read_back.carrier_handle, Some(Handle::new(42)));
        assert_eq!(read_back.label, "A - A");
        assert_eq!(read_back.far_depth, Some(4.5));
        assert_eq!(read_back.z_min, -0.5);
        assert_eq!(read_back.z_max, 3.2);
        assert!(read_back.flip_direction);
        assert!(!read_back.is_elevation);
        assert_eq!(read_back.path.cut_length(), 12.0);
    }

    #[test]
    fn test_elevation_marker_persistence() {
        let mut doc = CadDocument::new();
        let vp_id = ViewpointId::new();
        let marker = Din1356SectionMarker::new_elevation(
            "Nordansicht",
            Point2::new(0.0, 10.0),
            Point2::new(15.0, 10.0),
            vp_id,
            Some(Handle::new(99)),
        )
        .with_depth(Some(20.0));

        let handle = create_section_marker(&mut doc, &marker);
        let ent = doc.get_entity(handle).expect("elevation entity exists");
        let read_back = read_section_marker(&doc, ent).expect("read elevation marker");

        assert_eq!(read_back.viewpoint_id, vp_id);
        assert_eq!(read_back.carrier_handle, Some(Handle::new(99)));
        assert_eq!(read_back.label, "Nordansicht");
        assert!(read_back.is_elevation);
        assert_eq!(read_back.far_depth, Some(20.0));
    }

    #[test]
    fn test_section_marker_cad_entities_sync() {
        let mut doc = CadDocument::new();
        let vp_id = ViewpointId::new();
        let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(8.0, 0.0));
        let mut marker = Din1356SectionMarker::new_section("Schnitt 1", path, vp_id, None);

        let handle = create_section_marker(&mut doc, &marker);
        let initial_children = owner_index::children_of(&doc, handle);
        assert!(!initial_children.is_empty());

        // Update marker path and label
        marker.label = "Schnitt 1-1".to_string();
        marker.path = SectionPath::straight(Point2::new(1.0, 1.0), Point2::new(9.0, 1.0));
        let ok = sync_section_marker_cad_entities(&mut doc, handle, &marker);
        assert!(ok);

        let updated_children = owner_index::children_of(&doc, handle);
        assert!(!updated_children.is_empty());
        // Old children should have been removed
        for old in &initial_children {
            assert!(doc.get_entity(*old).is_none());
        }

        let ent = doc.get_entity(handle).expect("marker entity exists");
        let read_back = read_section_marker(&doc, ent).expect("read updated marker");
        assert_eq!(read_back.label, "Schnitt 1-1");
        assert_eq!(read_back.path.cut_length(), 8.0);
    }

    #[test]
    fn test_marker_viewpoint_and_carrier_lookup() {
        let mut doc = CadDocument::new();
        let vp_id = ViewpointId::new();
        let marker = Din1356SectionMarker::new_section(
            "A - A",
            SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(5.0, 0.0)),
            vp_id,
            Some(Handle::new(105)),
        );

        let marker_handle = create_section_marker(&mut doc, &marker);

        let (found_h_vp, found_by_vp) = find_section_marker_by_viewpoint(&doc, vp_id).expect("find by vp");
        assert_eq!(found_h_vp, marker_handle);
        assert_eq!(found_by_vp.label, "A - A");

        let (found_h_c, found_by_c) = find_section_marker_by_carrier(&doc, Handle::new(105)).expect("find by carrier");
        assert_eq!(found_h_c, marker_handle);
        assert_eq!(found_by_c.viewpoint_id, vp_id);
    }

    #[test]
    fn test_marker_grip_start_end_moves() {
        let mut doc = CadDocument::new();
        let vp_id = ViewpointId::new();
        let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let marker = Din1356SectionMarker::new_section("A - A", path, vp_id, None);
        let handle = create_section_marker(&mut doc, &marker);

        // 1. Straight path start grip Absolute
        let res = apply_section_marker_grip_pure(
            &mut doc,
            handle,
            SECTION_MARKER_START_GRIP,
            &GripApply::Absolute(glam::DVec3::new(1.0, 2.0, 0.0)),
            None,
        ).expect("start absolute succeeded");
        let start_pt = res.marker.path.segments()[0].start;
        assert_eq!(start_pt, Point2::new(1.0, 2.0));
        if let Some(EntityType::LwPolyline(pl)) = doc.get_entity(handle) {
            assert_eq!(pl.vertices[0].location.x, 1.0);
            assert_eq!(pl.vertices[0].location.y, 2.0);
        } else {
            panic!("expected lwpolyline");
        }

        // 2. Straight path start grip Translate
        let res2 = apply_section_marker_grip_pure(
            &mut doc,
            handle,
            SECTION_MARKER_START_GRIP,
            &GripApply::Translate(glam::DVec3::new(0.5, -1.0, 0.0)),
            None,
        ).expect("start translate succeeded");
        assert_eq!(res2.marker.path.segments()[0].start, Point2::new(1.5, 1.0));

        // 3. Straight path end grip Absolute
        let res3 = apply_section_marker_grip_pure(
            &mut doc,
            handle,
            SECTION_MARKER_END_GRIP,
            &GripApply::Absolute(glam::DVec3::new(12.0, 4.0, 0.0)),
            None,
        ).expect("end absolute succeeded");
        let end_pt = res3.marker.path.segments().last().unwrap().end;
        assert_eq!(end_pt, Point2::new(12.0, 4.0));
        if let Some(EntityType::LwPolyline(pl)) = doc.get_entity(handle) {
            assert_eq!(pl.vertices.last().unwrap().location.x, 12.0);
            assert_eq!(pl.vertices.last().unwrap().location.y, 4.0);
        }

        // 4. Straight path end grip Translate
        let res4 = apply_section_marker_grip_pure(
            &mut doc,
            handle,
            SECTION_MARKER_END_GRIP,
            &GripApply::Translate(glam::DVec3::new(1.0, -1.0, 0.0)),
            None,
        ).expect("end translate succeeded");
        assert_eq!(res4.marker.path.segments().last().unwrap().end, Point2::new(13.0, 3.0));

        // 5. Jogged path start/end moves
        let jog_path = SectionPath::jogged(vec![
            SectionSegment::new(Point2::new(0.0, 0.0), Point2::new(5.0, 0.0), false),
            SectionSegment::new(Point2::new(5.0, 0.0), Point2::new(5.0, 5.0), false),
            SectionSegment::new(Point2::new(5.0, 5.0), Point2::new(10.0, 5.0), false),
        ]);
        let jog_marker = Din1356SectionMarker::new_section("Jog", jog_path, vp_id, None);
        let jog_handle = create_section_marker(&mut doc, &jog_marker);

        let res_j1 = apply_section_marker_grip_pure(
            &mut doc,
            jog_handle,
            SECTION_MARKER_START_GRIP,
            &GripApply::Absolute(glam::DVec3::new(-1.0, 0.0, 0.0)),
            None,
        ).expect("jog start absolute succeeded");
        assert_eq!(res_j1.marker.path.segments()[0].start, Point2::new(-1.0, 0.0));

        let res_j2 = apply_section_marker_grip_pure(
            &mut doc,
            jog_handle,
            SECTION_MARKER_END_GRIP,
            &GripApply::Translate(glam::DVec3::new(2.0, 1.0, 0.0)),
            None,
        ).expect("jog end translate succeeded");
        assert_eq!(res_j2.marker.path.segments().last().unwrap().end, Point2::new(12.0, 6.0));
    }

    #[test]
    fn test_marker_grip_jog_moves() {
        let mut doc = CadDocument::new();
        let vp_id = ViewpointId::new();
        let jog_path = SectionPath::jogged(vec![
            SectionSegment::new(Point2::new(0.0, 0.0), Point2::new(5.0, 0.0), false),
            SectionSegment::new(Point2::new(5.0, 0.0), Point2::new(10.0, 0.0), false),
        ]);
        let jog_marker = Din1356SectionMarker::new_section("Jog", jog_path, vp_id, None);
        let handle = create_section_marker(&mut doc, &jog_marker);

        // Absolute move of intermediate jog point (id 100)
        let res = apply_section_marker_grip_pure(
            &mut doc,
            handle,
            SECTION_MARKER_JOG_BASE,
            &GripApply::Absolute(glam::DVec3::new(6.0, 2.0, 0.0)),
            None,
        ).expect("jog 0 absolute succeeded");
        let segs = res.marker.path.segments();
        assert_eq!(segs[0].end, Point2::new(6.0, 2.0));
        assert_eq!(segs[1].start, Point2::new(6.0, 2.0));
        if let Some(EntityType::LwPolyline(pl)) = doc.get_entity(handle) {
            assert_eq!(pl.vertices[1].location.x, 6.0);
            assert_eq!(pl.vertices[1].location.y, 2.0);
        }

        // Translate move of intermediate jog point
        let res2 = apply_section_marker_grip_pure(
            &mut doc,
            handle,
            SECTION_MARKER_JOG_BASE,
            &GripApply::Translate(glam::DVec3::new(0.5, 0.5, 0.0)),
            None,
        ).expect("jog 0 translate succeeded");
        let segs2 = res2.marker.path.segments();
        assert_eq!(segs2[0].end, Point2::new(6.5, 2.5));
        assert_eq!(segs2[1].start, Point2::new(6.5, 2.5));
    }

    #[test]
    fn test_marker_grip_rejection_of_degenerate_input() {
        let mut doc = CadDocument::new();
        let vp_id = ViewpointId::new();
        let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let marker = Din1356SectionMarker::new_section("A - A", path, vp_id, None);
        let handle = create_section_marker(&mut doc, &marker);

        // Start moved to equal end point (< 1e-4) is rejected
        let res1 = apply_section_marker_grip_pure(
            &mut doc,
            handle,
            SECTION_MARKER_START_GRIP,
            &GripApply::Absolute(glam::DVec3::new(10.0, 0.0, 0.0)),
            None,
        );
        assert!(res1.is_none());

        // End moved to equal start point is rejected
        let res2 = apply_section_marker_grip_pure(
            &mut doc,
            handle,
            SECTION_MARKER_END_GRIP,
            &GripApply::Absolute(glam::DVec3::new(0.0, 0.0, 0.0)),
            None,
        );
        assert!(res2.is_none());

        // Jog grip applied to a straight path is rejected
        let res3 = apply_section_marker_grip_pure(
            &mut doc,
            handle,
            SECTION_MARKER_JOG_BASE,
            &GripApply::Absolute(glam::DVec3::new(5.0, 2.0, 0.0)),
            None,
        );
        assert!(res3.is_none());

        // Out-of-bounds jog index on jogged path is rejected
        let jog_path = SectionPath::jogged(vec![
            SectionSegment::new(Point2::new(0.0, 0.0), Point2::new(5.0, 0.0), false),
            SectionSegment::new(Point2::new(5.0, 0.0), Point2::new(10.0, 0.0), false),
        ]);
        let jog_marker = Din1356SectionMarker::new_section("Jog", jog_path, vp_id, None);
        let jog_handle = create_section_marker(&mut doc, &jog_marker);
        let res4 = apply_section_marker_grip_pure(
            &mut doc,
            jog_handle,
            SECTION_MARKER_JOG_BASE + 5,
            &GripApply::Absolute(glam::DVec3::new(5.0, 2.0, 0.0)),
            None,
        );
        assert!(res4.is_none());
    }

    #[test]
    fn test_marker_grip_depth_projection_and_clamp() {
        let mut doc = CadDocument::new();
        let vp_id = ViewpointId::new();
        let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let marker = Din1356SectionMarker::new_section("A - A", path, vp_id, None)
            .with_depth(Some(5.0));
        let handle = create_section_marker(&mut doc, &marker);

        // Normal is (0.0, 1.0). Absolute move to y = 8.0 sets depth to 8.0.
        let res1 = apply_section_marker_grip_pure(
            &mut doc,
            handle,
            SECTION_MARKER_DEPTH_GRIP,
            &GripApply::Absolute(glam::DVec3::new(5.0, 8.0, 0.0)),
            None,
        ).expect("depth absolute succeeded");
        assert_eq!(res1.marker.far_depth, Some(8.0));

        // Incremental Translate by delta (0.0, 2.5) accumulates to 10.5.
        let res2 = apply_section_marker_grip_pure(
            &mut doc,
            handle,
            SECTION_MARKER_DEPTH_GRIP,
            &GripApply::Translate(glam::DVec3::new(0.0, 2.5, 0.0)),
            None,
        ).expect("depth translate succeeded");
        assert_eq!(res2.marker.far_depth, Some(10.5));

        // Clamping: Absolute move to negative side (y = -5.0) clamps to 0.1.
        let res3 = apply_section_marker_grip_pure(
            &mut doc,
            handle,
            SECTION_MARKER_DEPTH_GRIP,
            &GripApply::Absolute(glam::DVec3::new(5.0, -5.0, 0.0)),
            None,
        ).expect("depth negative clamped succeeded");
        assert_eq!(res3.marker.far_depth, Some(0.1));

        // Clamping: Translate with negative delta clamps to 0.1.
        let res4 = apply_section_marker_grip_pure(
            &mut doc,
            handle,
            SECTION_MARKER_DEPTH_GRIP,
            &GripApply::Translate(glam::DVec3::new(0.0, -10.0, 0.0)),
            None,
        ).expect("depth negative translate clamped succeeded");
        assert_eq!(res4.marker.far_depth, Some(0.1));
    }

    #[test]
    fn test_marker_grip_flip_stable_sequence() {
        let mut doc = CadDocument::new();
        let vp_id = ViewpointId::new();
        let path = SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let marker = Din1356SectionMarker::new_section("A - A", path, vp_id, None)
            .with_flip(false);
        let handle = create_section_marker(&mut doc, &marker);

        // Snapshot original marker
        let orig_marker = marker.clone();

        // 1. Initial click on flip grip (within 0.3 m of original flip grip position at (5.0, 1.0, 0.0))
        let res1 = apply_section_marker_grip_pure(
            &mut doc,
            handle,
            SECTION_MARKER_FLIP_GRIP,
            &GripApply::Absolute(glam::DVec3::new(5.0, 1.05, 0.0)),
            Some(&orig_marker),
        ).expect("flip click succeeded");
        assert!(res1.marker.flip_direction);

        // 2. Sequence of 5 mouse moves during drag, all close to the flip grip (< 0.3 m from orig_pos).
        // Must remain stably true relative to orig_marker, never oscillating.
        for offset in [0.02, 0.05, 0.10, 0.15, 0.20] {
            let res = apply_section_marker_grip_pure(
                &mut doc,
                handle,
                SECTION_MARKER_FLIP_GRIP,
                &GripApply::Absolute(glam::DVec3::new(5.0, 1.0 + offset, 0.0)),
                Some(&orig_marker),
            ).expect("flip move succeeded");
            assert!(res.marker.flip_direction, "must remain stably flipped during drag sequence");
        }

        // 3. Drag across the cut line to the negative Y side (> 0.3 m away)
        let res_neg = apply_section_marker_grip_pure(
            &mut doc,
            handle,
            SECTION_MARKER_FLIP_GRIP,
            &GripApply::Absolute(glam::DVec3::new(5.0, -2.0, 0.0)),
            Some(&orig_marker),
        ).expect("drag across cut line succeeded");
        assert!(res_neg.marker.flip_direction);

        // 4. Drag to the positive Y side (> 0.3 m away)
        let res_pos = apply_section_marker_grip_pure(
            &mut doc,
            handle,
            SECTION_MARKER_FLIP_GRIP,
            &GripApply::Absolute(glam::DVec3::new(5.0, 3.0, 0.0)),
            Some(&orig_marker),
        ).expect("drag to positive side succeeded");
        assert!(!res_pos.marker.flip_direction);
    }

    #[test]
    fn test_marker_commit_sync_children_and_carrier_viewpoint() {
        let mut doc = CadDocument::new();
        let vp_id = ViewpointId::new();
        let mv_id = crate::modules::aec::engine::views::ModelViewId::new();

        let carrier = crate::modules::aec::engine::views::carrier::create_view_carrier(
            &mut doc,
            vp_id,
            mv_id,
            Point2::new(50.0, 0.0),
            1.0,
            0.0,
            crate::modules::aec::engine::views::model_view::UpdatePolicy::Manual,
        ).expect("carrier created");

        let vp = crate::modules::aec::engine::views::viewpoint::Viewpoint {
            id: vp_id,
            name: "Initial Section".into(),
            revision: crate::modules::aec::engine::views::Revision(1),
            origin: crate::modules::aec::engine::views::viewpoint::ViewOrigin::World(
                crate::modules::aec::engine::views::Point3::new(0.0, 0.0, 0.0),
            ),
            axes: crate::modules::aec::engine::views::viewpoint::ViewAxes::section(
                Vector2::new(1.0, 0.0),
            ).unwrap(),
            kind: crate::modules::aec::engine::views::viewpoint::ViewKind::Section {
                path: crate::modules::aec::engine::views::viewpoint::SectionPath::straight(
                    Point2::new(0.0, 0.0),
                    Point2::new(10.0, 0.0),
                ),
                far_depth: None,
            },
            z_range: crate::modules::aec::engine::views::viewpoint::VerticalRange::new(
                crate::modules::aec::engine::views::Metres::new(0.0).unwrap(),
                crate::modules::aec::engine::views::Metres::new(3.0).unwrap(),
            ).unwrap(),
            mapping: Default::default(),
            crop: None,
        };
        let mv = crate::modules::aec::engine::views::model_view::ModelView {
            id: mv_id,
            name: "Initial MV".into(),
            viewpoint_id: vp_id,
            profile_id: crate::modules::aec::engine::views::ViewProfileId::new(),
            scale: crate::modules::aec::engine::views::ScaleDenominator::new(100.0).unwrap(),
            detail: crate::modules::aec::engine::views::model_view::DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: crate::modules::aec::engine::views::model_view::UpdatePolicy::Manual,
            revisions: Default::default(),
        };
        crate::modules::aec::engine::views::carrier::write_view_carrier_definitions(
            &mut doc,
            carrier.handle,
            &vp,
            &mv,
        );

        let mut marker = Din1356SectionMarker::new_section(
            "Initial Section",
            crate::modules::aec::engine::views::viewpoint::SectionPath::straight(
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
            ),
            vp_id,
            Some(carrier.handle),
        );
        let marker_handle = create_section_marker(&mut doc, &marker);

        // 1. Sync CAD entities and verify child back-references
        let ok = sync_section_marker_cad_entities(&mut doc, marker_handle, &marker);
        assert!(ok);
        let children = owner_index::children_of(&doc, marker_handle);
        assert!(!children.is_empty());
        for &child in &children {
            let child_ent = doc.get_entity(child).expect("child entity exists");
            assert_eq!(
                section_marker_owner_from_entity(child_ent),
                Some(marker_handle),
                "child must reference marker as owner"
            );
        }

        // 2. Modify marker geometry and properties
        marker.path = crate::modules::aec::engine::views::viewpoint::SectionPath::straight(
            Point2::new(2.0, 1.0),
            Point2::new(14.0, 1.0),
        );
        marker.far_depth = Some(7.5);
        marker.z_min = -1.0;
        marker.z_max = 4.0;
        marker.flip_direction = true;
        write_section_marker(&mut doc, marker_handle, &marker);

        // 3. Sync marker to carrier
        let carrier_synced = sync_marker_to_carrier(&mut doc, &marker);
        assert!(carrier_synced);

        // Verify carrier's embedded definitions are updated
        let (updated_vp, _) = crate::modules::aec::engine::views::carrier::read_view_carrier_definitions(
            &doc,
            carrier.handle,
        ).expect("carrier definitions exist");

        match updated_vp.kind {
            crate::modules::aec::engine::views::viewpoint::ViewKind::Section { path, far_depth } => {
                assert_eq!(path.cut_length(), 12.0);
                assert_eq!(far_depth.map(|d| d.get()), Some(7.5));
            }
            _ => panic!("expected ViewKind::Section"),
        }
        assert_eq!(updated_vp.z_range.z_min.get(), -1.0);
        assert_eq!(updated_vp.z_range.z_max.get(), 4.0);
        assert_eq!(updated_vp.axes.forward.y, -1.0);

        let updated_carrier = crate::modules::aec::engine::views::carrier::find_view_carrier_by_handle(&doc, carrier.handle).unwrap();
        assert!(updated_carrier.is_outdated);
    }

    #[test]
    fn test_marker_grip_undo_restores_marker_children_and_carrier() {
        use crate::app::OpenCADStudio;
        use crate::modules::aec::engine::owner_index;
        use crate::modules::aec::engine::views::carrier::{create_view_carrier, read_view_carrier_definitions, write_view_carrier_definitions};
        use crate::modules::aec::engine::views::model_view::{DetailPolicy, ModelView, UpdatePolicy};
        use crate::modules::aec::engine::views::viewpoint::{SectionPath, VerticalRange, ViewAxes, ViewKind, ViewOrigin, Viewpoint};
        use crate::modules::aec::engine::views::{Metres, ModelViewId, Point2, Point3, Revision, ScaleDenominator, Vector2, ViewProfileId, ViewpointId};

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;

        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let carrier = create_view_carrier(
            &mut app.tabs[i].scene.document,
            vp_id,
            mv_id,
            Point2::new(30.0, 0.0),
            1.0,
            0.0,
            UpdatePolicy::Automatic,
        ).unwrap();

        let vp = Viewpoint {
            id: vp_id,
            name: "Cut A".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::section(Vector2::new(1.0, 0.0)).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
                far_depth: None,
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: Default::default(),
            crop: None,
        };
        let mv = ModelView {
            id: mv_id,
            name: "Cut A MV".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(100.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: Default::default(),
            graphic_overrides: Vec::new(),
            annotation_ids: Vec::new(),
            update_policy: UpdatePolicy::Automatic,
            revisions: Default::default(),
        };
        write_view_carrier_definitions(&mut app.tabs[i].scene.document, carrier.handle, &vp, &mv);

        let marker = Din1356SectionMarker::new_section(
            "Cut A",
            SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
            vp_id,
            Some(carrier.handle),
        );
        let marker_handle = create_section_marker(&mut app.tabs[i].scene.document, &marker);

        let initial_children = owner_index::children_of(&app.tabs[i].scene.document, marker_handle);
        assert!(!initial_children.is_empty());

        // Prepare viewport for camera projection and hit testing
        {
            let tab = &mut app.tabs[i];
            tab.scene.selection.borrow_mut().view.vp_size = (1920.0, 1080.0);
            tab.scene.sync_tiles_from_panes(1920.0, 1080.0);
            tab.scene.fit_all();
        }

        // Move start grip (grip_id 0) to (2.0, 3.0, 0.0) via grip_drag automation
        let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
        let doc_id = state["document_id"].as_u64().unwrap();
        let drag_req = serde_json::json!({
            "protocol": 1,
            "request_id": "req-grip-1",
            "document_id": doc_id,
            "op": "grip_drag",
            "handle": format!("0x{:X}", marker_handle.value()),
            "grip_id": 0,
            "point": [2.0, 3.0, 0.0]
        });
        let drag_res = app.automation_op(&drag_req.to_string());
        assert_eq!(drag_res["ok"], true, "grip_drag failed: {drag_res}");

        // Verify committed state
        let committed_marker = read_section_marker(
            &app.tabs[i].scene.document,
            app.tabs[i].scene.document.get_entity(marker_handle).unwrap(),
        ).unwrap();
        let p = committed_marker.path.segments()[0].start;
        assert!((p.x_m - 2.0).abs() < 1e-4 && (p.y_m - 3.0).abs() < 1e-4, "got {p:?}");

        // Now test single Undo: restores marker, children, and carrier definitions
        let undo_req = serde_json::json!({
            "protocol": 1,
            "request_id": "req-undo-1",
            "document_id": doc_id,
            "op": "undo"
        });
        let undo_res = app.automation_op(&undo_req.to_string());
        assert_eq!(undo_res["ok"], true, "undo failed: {undo_res}");

        let restored_marker = read_section_marker(
            &app.tabs[i].scene.document,
            app.tabs[i].scene.document.get_entity(marker_handle).unwrap(),
        ).unwrap();
        let p_restored = restored_marker.path.segments()[0].start;
        assert!((p_restored.x_m - 0.0).abs() < 1e-4 && (p_restored.y_m - 0.0).abs() < 1e-4, "got {p_restored:?}");

        let restored_children = owner_index::children_of(&app.tabs[i].scene.document, marker_handle);
        assert_eq!(restored_children, initial_children);
        for &child in &restored_children {
            assert!(app.tabs[i].scene.document.get_entity(child).is_some());
        }

        let (restored_vp, _) = read_view_carrier_definitions(
            &app.tabs[i].scene.document,
            carrier.handle,
        ).unwrap();
        match restored_vp.kind {
            ViewKind::Section { path, .. } => {
                let vp_start = path.segments()[0].start;
                assert!((vp_start.x_m - 0.0).abs() < 1e-4 && (vp_start.y_m - 0.0).abs() < 1e-4, "got {vp_start:?}");
            }
            _ => panic!("expected section"),
        }
    }
}
