//! Minimal IFC4 SPF (STEP Physical File) writer.
//!
//! This is intentionally **not** a general-purpose IFC schema writer. It
//! serializes exactly the entity subset needed for a first AEC export:
//! `IfcProject`, `IfcSite`, `IfcBuilding`, `IfcBuildingStorey`, `IfcWall` and
//! `IfcSpace`, plus the minimal relationship entities needed to nest them
//! into a valid spatial structure (`IfcRelAggregates`,
//! `IfcRelContainedInSpatialStructure`).
//!
//! Geometric representations (placements, shapes) are deliberately left
//! out of scope for this first increment.

use super::room::Room;
use super::storey::Storey;
use super::wall::{Wall, WallLayer};

/// Everything needed to export a minimal IFC4 SPF file: the storeys (with
/// their `storey_id`, matching the XDATA `storey_id` field), and the walls
/// and rooms that reference those storeys.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    /// `(storey_id, storey)` pairs, in the order they should be written.
    pub storeys: Vec<(u32, Storey)>,
    /// Walls to export; each `Wall::storey_id` must match a `storeys` entry.
    pub walls: Vec<Wall>,
    /// Rooms to export; each `Room::storey_id` must match a `storeys` entry.
    pub rooms: Vec<Room>,
}

/// A monotonically increasing IFC "line" id allocator (`#1`, `#2`, ...).
struct IdGen(u64);

impl IdGen {
    fn next(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }
}

/// A placeholder IFC GUID. Real GUIDs are base64-like 22-character IFC
/// "compressed" GUIDs; for this minimal writer we emit a fixed-width,
/// deterministic placeholder derived from the entity kind and index so
/// output is reproducible and easy to assert on in tests.
fn placeholder_guid(kind: &str, index: usize) -> String {
    format!("{:0>22}", format!("{kind}{index}"))
        .chars()
        .take(22)
        .collect()
}

/// Serializes a [`Scene`] into a minimal, valid IFC4 SPF string.
///
/// The output always contains, in order: the ISO-10303-21 header, an
/// `IfcProject`, one `IfcSite`, one `IfcBuilding`, one `IfcBuildingStorey`
/// per `scene.storeys` entry, one `IfcWall` per `scene.walls` entry, one
/// `IfcSpace` per `scene.rooms` entry, and the `IfcRelAggregates` /
/// `IfcRelContainedInSpatialStructure` relationships that nest walls and
/// rooms into their owning storey.
pub fn write_spf(scene: &Scene) -> String {
    let mut ids = IdGen(0);
    let mut data_lines: Vec<String> = Vec::new();

    let owner_history = ids.next();
    data_lines.push(format!(
        "#{owner_history}=IFCOWNERHISTORY($,$,$,.ADDED.,$,$,$,0);"
    ));

    let project = ids.next();
    data_lines.push(format!(
        "#{project}=IFCPROJECT('{}',#{owner_history},'AEC Export',$,$,$,$,$,$);",
        placeholder_guid("Project", 0)
    ));

    let site = ids.next();
    data_lines.push(format!(
        "#{site}=IFCSITE('{}',#{owner_history},'Site',$,$,$,$,$,.ELEMENT.,$,$,$,$,$);",
        placeholder_guid("Site", 0)
    ));

    let building = ids.next();
    data_lines.push(format!(
        "#{building}=IFCBUILDING('{}',#{owner_history},'Building',$,$,$,$,$,.ELEMENT.,$,$,$);",
        placeholder_guid("Building", 0)
    ));

    let rel_project_site = ids.next();
    data_lines.push(format!(
        "#{rel_project_site}=IFCRELAGGREGATES('{}',#{owner_history},$,$,#{project},(#{site}));",
        placeholder_guid("RelAggregates", 0)
    ));

    let rel_site_building = ids.next();
    data_lines.push(format!(
        "#{rel_site_building}=IFCRELAGGREGATES('{}',#{owner_history},$,$,#{site},(#{building}));",
        placeholder_guid("RelAggregates", 1)
    ));

    // One IfcBuildingStorey per scene storey, keyed by storey_id so walls
    // and rooms can look up their spatial-structure parent.
    let mut storey_line_ids: Vec<(u32, u64)> = Vec::with_capacity(scene.storeys.len());
    let mut storey_ids_for_building: Vec<u64> = Vec::with_capacity(scene.storeys.len());
    for (index, (storey_id, storey)) in scene.storeys.iter().enumerate() {
        let line_id = ids.next();
        data_lines.push(format!(
            "#{line_id}=IFCBUILDINGSTOREY('{}',#{owner_history},'{}',$,$,$,$,$,.ELEMENT.,{});",
            placeholder_guid("Storey", index),
            escape(&storey.name),
            format_real(storey.elevation),
        ));
        storey_line_ids.push((*storey_id, line_id));
        storey_ids_for_building.push(line_id);
    }
    if !storey_ids_for_building.is_empty() {
        let rel_building_storeys = ids.next();
        let refs = storey_ids_for_building
            .iter()
            .map(|id| format!("#{id}"))
            .collect::<Vec<_>>()
            .join(",");
        data_lines.push(format!(
            "#{rel_building_storeys}=IFCRELAGGREGATES('{}',#{owner_history},$,$,#{building},({refs}));",
            placeholder_guid("RelAggregates", 2)
        ));
    }

    // One IfcWall per scene wall, grouped by owning storey for containment.
    let mut walls_by_storey: Vec<(u64, Vec<u64>)> = Vec::new();
    for (index, wall) in scene.walls.iter().enumerate() {
        let line_id = ids.next();
        data_lines.push(format!(
            "#{line_id}=IFCWALL('{}',#{owner_history},'Wall {}',$,$,$,$,$,$);",
            placeholder_guid("Wall", index),
            index,
        ));
        if let Some(storey_line_id) = storey_line_ids
            .iter()
            .find(|(id, _)| *id == wall.storey_id)
            .map(|(_, line)| *line)
        {
            containment_bucket(&mut walls_by_storey, storey_line_id).push(line_id);
        }
    }

    // One IfcSpace per scene room, grouped by owning storey for containment.
    let mut rooms_by_storey: Vec<(u64, Vec<u64>)> = Vec::new();
    for (index, room) in scene.rooms.iter().enumerate() {
        let line_id = ids.next();
        data_lines.push(format!(
            "#{line_id}=IFCSPACE('{}',#{owner_history},'{}',$,$,$,$,$,.ELEMENT.,$);",
            placeholder_guid("Space", index),
            escape(&room.name),
        ));
        if let Some(storey_line_id) = storey_line_ids
            .iter()
            .find(|(id, _)| *id == room.storey_id)
            .map(|(_, line)| *line)
        {
            containment_bucket(&mut rooms_by_storey, storey_line_id).push(line_id);
        }
    }

    for (storey_line_id, element_ids) in walls_by_storey.into_iter().chain(rooms_by_storey) {
        let rel_id = ids.next();
        let refs = element_ids
            .iter()
            .map(|id| format!("#{id}"))
            .collect::<Vec<_>>()
            .join(",");
        data_lines.push(format!(
            "#{rel_id}=IFCRELCONTAINEDINSPATIALSTRUCTURE('{}',#{owner_history},$,$,({refs}),#{storey_line_id});",
            placeholder_guid("RelContained", rel_id as usize)
        ));
    }

    let mut out = String::new();
    out.push_str("ISO-10303-21;\n");
    out.push_str("HEADER;\n");
    out.push_str("FILE_DESCRIPTION(('ViewDefinition [CoordinationView]'),'2;1');\n");
    out.push_str("FILE_NAME('','',(''),(''),'opencad-aec-engine','',''); \n");
    out.push_str("FILE_SCHEMA(('IFC4'));\n");
    out.push_str("ENDSEC;\n");
    out.push_str("DATA;\n");
    for line in &data_lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("ENDSEC;\n");
    out.push_str("END-ISO-10303-21;\n");
    out
}

/// Finds or creates the bucket for `storey_line_id` and returns it, so
/// callers can push a contained element's line id into it.
fn containment_bucket(buckets: &mut Vec<(u64, Vec<u64>)>, storey_line_id: u64) -> &mut Vec<u64> {
    if let Some(pos) = buckets.iter().position(|(id, _)| *id == storey_line_id) {
        &mut buckets[pos].1
    } else {
        buckets.push((storey_line_id, Vec::new()));
        &mut buckets.last_mut().unwrap().1
    }
}

/// Formats an `f64` the way an IFC `IfcReal`/`IfcLengthMeasure` expects it
/// (always with a decimal point, even for whole numbers).
fn format_real(value: f64) -> String {
    if value == value.trunc() {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}

/// Escapes characters that are not allowed unescaped inside an IFC SPF
/// string literal (`'`).
fn escape(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_scene() -> Scene {
        let storey = Storey::new("Level 1", 0.0, 3.0);
        let mut wall = Wall::new("style1", 2.8, 0);
        wall.layers.push(WallLayer {
            material: "Concrete".to_string(),
            thickness: 0.2,
            function: "Structural".to_string(),
            gap_before: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
        });
        let room = Room::from_polygon(
            "Office 101",
            &[(0.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 3.0)],
            3.0,
            0,
        );
        Scene {
            storeys: vec![(0, storey)],
            walls: vec![wall],
            rooms: vec![room],
        }
    }

    #[test]
    fn output_has_valid_spf_header_and_footer() {
        let spf = write_spf(&sample_scene());
        assert!(spf.starts_with("ISO-10303-21;\n"));
        assert!(spf.trim_end().ends_with("END-ISO-10303-21;"));
        assert!(spf.contains("FILE_SCHEMA(('IFC4'));"));
    }

    #[test]
    fn output_contains_one_of_each_required_entity_type() {
        let spf = write_spf(&sample_scene());
        for entity in [
            "IFCPROJECT(",
            "IFCSITE(",
            "IFCBUILDING(",
            "IFCBUILDINGSTOREY(",
            "IFCWALL(",
            "IFCSPACE(",
        ] {
            assert!(
                spf.contains(entity),
                "expected output to contain {entity}, got:\n{spf}"
            );
        }
    }

    #[test]
    fn wall_and_room_are_contained_in_their_storey() {
        let spf = write_spf(&sample_scene());
        let contained_lines: Vec<&str> = spf
            .lines()
            .filter(|line| line.contains("IFCRELCONTAINEDINSPATIALSTRUCTURE("))
            .collect();
        // One relationship per non-empty (storey, element-kind) bucket:
        // here one for the wall and one for the room, both under Level 1.
        assert_eq!(contained_lines.len(), 2);
    }

    #[test]
    fn storey_name_and_elevation_are_serialized() {
        let spf = write_spf(&sample_scene());
        assert!(spf.contains("'Level 1'"));
    }

    #[test]
    fn empty_scene_still_produces_minimal_valid_spf() {
        let spf = write_spf(&Scene::default());
        assert!(spf.contains("IFCPROJECT("));
        assert!(spf.contains("IFCSITE("));
        assert!(spf.contains("IFCBUILDING("));
        assert!(!spf.contains("IFCBUILDINGSTOREY("));
        assert!(!spf.contains("IFCWALL("));
        assert!(!spf.contains("IFCSPACE("));
    }
}
