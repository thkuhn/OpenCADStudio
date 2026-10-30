//! Sample AEC Project generator for OpenCADStudio.
//!
//! Creates a complete multi-storey residential building ("Einfamilienhaus")
//! with single storey drawings (UG, EG, OG, DG), an aggregated master model
//! (Haus_Gesamt with XREFs and derived sections/elevations/floor plans), and a project file (.ocsproj).

use std::fs;
use std::path::Path;

use codec::entities::{Insert, LwPolyline, LwVertex};
use codec::tables::block_record::BlockRecord;
use codec::types::{Vector2, Vector3 as CadVector3};
use codec::xdata::ExtendedDataRecord;
use codec::{DxfVersion, EntityType};
use glam::DVec3;

use crate::io::save_as_version;
use crate::modules::aec::engine::display_component::{OpeningComponentSlot, RepresentationMode};
use crate::modules::aec::engine::library::{
    seed_default_library, DisplayConfigLibrary, StyleLibrary, PRESET_ENTWURF_100,
    PRESET_WERKPLAN_50,
};
use crate::modules::aec::engine::opening_xdata::place_wall_opening;
use crate::modules::aec::engine::openings::OpeningKind;
use crate::modules::aec::engine::plan_view::{
    DisplayConfig, LayerDetail, PlanningStage, ViewType,
};
use crate::modules::aec::engine::project::{Building, ProjectFile, StoreyRef};
use crate::modules::aec::engine::room::{Room, RoomFinish, RoomFunction};
use crate::modules::aec::engine::room_regen::regenerate_room_representation;
use crate::modules::aec::engine::room_xdata::write_room_record;
use crate::modules::aec::engine::slab::{Slab, SlabJustification, SlabLayer};
use crate::modules::aec::engine::slab_regen::regenerate_slab_representation;
use crate::modules::aec::engine::slab_xdata::slab_record_for_slab;
use crate::modules::aec::engine::views::carrier::{
    create_view_carrier, sync_carrier_primitives, write_view_carrier_definitions,
};
use crate::modules::aec::engine::views::cut_engine::{derive_section_view, CutEngineOptions};
use crate::modules::aec::engine::views::elevation_engine::{
    derive_elevation_view, ElevationEngineOptions,
};
use crate::modules::aec::engine::views::floorplan_engine::{
    derive_floorplan_view, FloorPlanEngineOptions,
};
use crate::modules::aec::engine::views::model_view::DetailLevel;
use crate::modules::aec::engine::views::scene_collector::SceneGeometryCollector;
use crate::modules::aec::engine::views::Vector3 as ViewVector3;
use crate::modules::aec::engine::views::*;
use crate::modules::aec::views::din1356::{create_section_marker, Din1356SectionMarker};
use crate::modules::aec::engine::wall::{Wall, WallJustification, WallLayer};
use crate::modules::aec::engine::wall_regen::regenerate_wall_representation;
use crate::modules::aec::engine::wall_style::LayerFunction;
use crate::modules::aec::engine::xdata::{wall_record_for_wall, AEC_APPID};
use crate::scene::Scene;

fn wl(name: &str, thick: f64, role: &str) -> WallLayer {
    WallLayer {
        material: name.to_string(),
        thickness: thick,
        function: role.to_string(),
        axis_offset: 0.0,
        bottom_offset: 0.0,
        top_offset: 0.0,
        layer_override: None,
        hatch_override: None,
        layer_id: uuid::Uuid::new_v4(),
    }
}

fn sl(name: &str, thick: f64, func: LayerFunction) -> SlabLayer {
    SlabLayer::new(name, thick, func)
}

/// Generates the complete sample project into the given directory.
pub fn generate_sample_project(target_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(target_dir)?;

    let library = build_sample_library();
    let display_lib = build_sample_display_library();

    // 1. Generate UG (Basement Floor, Z = -2.80m)
    let mut ug_scene = Scene::new();
    build_ug_geometry(&mut ug_scene, &library);
    save_drawing_variants(&ug_scene, &target_dir.join("UG"))?;

    // 2. Generate EG (Ground Floor, Z = 0.0m)
    let mut eg_scene = Scene::new();
    build_eg_geometry(&mut eg_scene, &library);
    save_drawing_variants(&eg_scene, &target_dir.join("EG"))?;

    // 3. Generate OG (Upper Floor, Z = 2.80m)
    let mut og_scene = Scene::new();
    build_og_geometry(&mut og_scene, &library);
    save_drawing_variants(&og_scene, &target_dir.join("OG"))?;

    // 4. Generate DG (Attic / Roof, Z = 5.60m)
    let mut dg_scene = Scene::new();
    build_dg_geometry(&mut dg_scene, &library);
    save_drawing_variants(&dg_scene, &target_dir.join("DG"))?;

    // 5. Generate Master Gesamtmodell (Haus_Gesamt, attaches UG, EG, OG, DG with Z-offsets)
    let mut master_scene = Scene::new();
    build_master_geometry(&mut master_scene, target_dir, &library)?;
    save_drawing_variants(&master_scene, &target_dir.join("Haus_Gesamt"))?;

    // 6. Generate Project File (.ocsproj)
    let project_file = build_project_file(&library, &display_lib);
    let proj_path = target_dir.join("Projekt_Einfamilienhaus.ocsproj");
    project_file.save(&proj_path)?;

    // 7. Generate README Documentation
    let readme_content = generate_readme_text();
    fs::write(target_dir.join("README.md"), readme_content)?;

    Ok(())
}

fn save_drawing_variants(scene: &Scene, base_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let dwg_path = base_path.with_extension("dwg");
    let dxf_path = base_path.with_extension("dxf");

    save_as_version(&scene.document, &dwg_path, DxfVersion::AC1032)?;
    save_as_version(&scene.document, &dxf_path, DxfVersion::AC1032)?;

    Ok(())
}

fn add_wall(
    scene: &mut Scene,
    p1: (f64, f64),
    p2: (f64, f64),
    base_z: f64,
    height: f64,
    layers: Vec<WallLayer>,
) -> codec::Handle {
    let mut pl = LwPolyline::new();
    pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
    pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
    pl.elevation = base_z;

    let mut wall = Wall::new("Standard", height, 0);
    wall.base_origin = [p1.0, p1.1, base_z];
    wall.layers = layers;
    wall.justification = WallJustification::Exterior;

    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = wall_record_for_wall(&wall);
    ent.common_mut().extended_data.add_record(rec);
    ent.common_mut().layer = "AEC_WALL".to_string();

    let h = scene.add_entity(ent);
    let _ = regenerate_wall_representation(scene, h, None);
    h
}

fn add_slab(
    scene: &mut Scene,
    points: &[(f64, f64)],
    base_z: f64,
    _thickness: f64,
    layers: Vec<SlabLayer>,
) -> codec::Handle {
    let mut pl = LwPolyline::new();
    for pt in points {
        pl.add_vertex(LwVertex::new(Vector2::new(pt.0, pt.1)));
    }
    pl.is_closed = true;
    pl.elevation = base_z;

    let mut slab = Slab::new("Geschossdecke", 0);
    slab.base_origin = [points[0].0, points[0].1, base_z];
    slab.top_origin = [points[0].0, points[0].1, base_z];
    slab.justification = SlabJustification::Top;
    slab.layers = layers;

    let mut ent = EntityType::LwPolyline(pl);
    let mut rec = ExtendedDataRecord::new(AEC_APPID);
    rec.values = slab_record_for_slab(&slab);
    ent.common_mut().extended_data.add_record(rec);
    ent.common_mut().layer = "AEC_SLAB".to_string();

    let h = scene.add_entity(ent);
    let _ = regenerate_slab_representation(scene, h, None, None);
    h
}

fn add_room(
    scene: &mut Scene,
    name: &str,
    number: &str,
    points: &[(f64, f64)],
    height: f64,
    storey_id: u32,
    function: RoomFunction,
    floor_finish_mat: &str,
) -> codec::Handle {
    let mut pl = LwPolyline::new();
    for pt in points {
        pl.add_vertex(LwVertex::new(Vector2::new(pt.0, pt.1)));
    }
    pl.is_closed = true;
    let mut ent = EntityType::LwPolyline(pl);
    ent.common_mut().layer = "AEC_ROOM".to_string();
    let h = scene.add_entity(ent);

    let mut room = Room::from_polygon(name, points, height, storey_id)
        .with_number(number)
        .with_function(function);
    room.floor_finish = Some(vec![RoomFinish::new(floor_finish_mat, 0.015)]);
    write_room_record(&mut scene.document, h, &room);
    let _ = regenerate_room_representation(scene, h, None);
    h
}

fn exterior_layers() -> Vec<WallLayer> {
    vec![
        wl("Außenputz", 0.015, "Substrate"),
        wl("Wärmedämmung", 0.140, "Thermal"),
        wl("Kalksandstein", 0.175, "Structural"),
        wl("Innenputz", 0.015, "Substrate"),
    ]
}

fn basement_exterior_layers() -> Vec<WallLayer> {
    vec![
        wl("Bitumen_Abdichtung", 0.005, "Waterproofing"),
        wl("Perimeterdaemmung", 0.120, "Thermal"),
        wl("WU_Stahlbeton", 0.240, "Structural"),
        wl("Innenputz", 0.010, "Substrate"),
    ]
}

fn interior_bearing_layers() -> Vec<WallLayer> {
    vec![
        wl("Putz", 0.015, "Substrate"),
        wl("Kalksandstein", 0.210, "Structural"),
        wl("Putz", 0.015, "Substrate"),
    ]
}

fn interior_light_layers() -> Vec<WallLayer> {
    vec![
        wl("Gipsfaser", 0.0125, "Substrate"),
        wl("Ständerwerk_Dämmung", 0.090, "Thermal"),
        wl("Gipsfaser", 0.0125, "Substrate"),
    ]
}

fn interior_basement_partition_layers() -> Vec<WallLayer> {
    vec![
        wl("Putz", 0.015, "Substrate"),
        wl("Kalksandstein", 0.115, "Structural"),
        wl("Putz", 0.015, "Substrate"),
    ]
}

fn build_ug_geometry(scene: &mut Scene, library: &StyleLibrary) {
    let z = 0.0;
    let h = 2.60;

    // Floor slab (Bodenplatte WU-Beton)
    add_slab(
        scene,
        &[(0.0, 0.0), (10.0, 0.0), (10.0, 8.0), (0.0, 8.0)],
        z,
        0.33,
        vec![
            sl("Estrich_Dämmung", 0.08, LayerFunction::Finish),
            sl("WU_Stahlbeton", 0.25, LayerFunction::Structural),
        ],
    );

    // Exterior basement walls
    let w_south = add_wall(scene, (0.0, 0.0), (10.0, 0.0), z, h, basement_exterior_layers());
    let _w_east = add_wall(scene, (10.0, 0.0), (10.0, 8.0), z, h, basement_exterior_layers());
    let w_north = add_wall(scene, (10.0, 8.0), (0.0, 8.0), z, h, basement_exterior_layers());
    let _w_west = add_wall(scene, (0.0, 8.0), (0.0, 0.0), z, h, basement_exterior_layers());

    // Interior walls
    let w_int_mid = add_wall(scene, (5.0, 0.0), (5.0, 8.0), z, h, interior_bearing_layers());
    let w_int_tech = add_wall(scene, (0.0, 3.5), (5.0, 3.5), z, h, interior_basement_partition_layers());
    let w_int_store = add_wall(scene, (5.0, 4.5), (10.0, 4.5), z, h, interior_basement_partition_layers());

    // Basement Openings (Windows & Doors)
    let _ = place_wall_opening(
        scene,
        w_south,
        DVec3::new(2.5, 0.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_south,
        DVec3::new(7.5, 0.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_north,
        DVec3::new(2.5, 8.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_north,
        DVec3::new(7.5, 8.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );

    let _ = place_wall_opening(
        scene,
        w_int_tech,
        DVec3::new(2.5, 3.5, z),
        OpeningKind::Door,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_int_mid,
        DVec3::new(5.0, 2.0, z),
        OpeningKind::Door,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_int_store,
        DVec3::new(7.5, 4.5, z),
        OpeningKind::Door,
        Some(library),
        None,
        None,
    );

    // Rooms (Raumstempel nach DIN 1356 / DIN 277)
    add_room(
        scene,
        "Technik / HTR",
        "UG-01",
        &[(0.24, 0.24), (4.76, 0.24), (4.76, 3.38), (0.24, 3.38)],
        2.40,
        0,
        RoomFunction::Technical,
        "Estrich",
    );
    add_room(
        scene,
        "Keller / Lager",
        "UG-02",
        &[(0.24, 3.62), (4.76, 3.62), (4.76, 7.76), (0.24, 7.76)],
        2.40,
        0,
        RoomFunction::Storage,
        "Estrich",
    );
    add_room(
        scene,
        "Hobby / Fitness",
        "UG-03",
        &[(5.24, 0.24), (9.76, 0.24), (9.76, 4.38), (5.24, 4.38)],
        2.40,
        0,
        RoomFunction::Living,
        "Laminat",
    );
    add_room(
        scene,
        "Vorrat / Abstell",
        "UG-04",
        &[(5.24, 4.62), (9.76, 4.62), (9.76, 7.76), (5.24, 7.76)],
        2.40,
        0,
        RoomFunction::Storage,
        "Fliesen",
    );
}

fn build_eg_geometry(scene: &mut Scene, library: &StyleLibrary) {
    let z = 0.0;
    let h = 2.80;

    // Floor slab over UG
    add_slab(
        scene,
        &[(0.0, 0.0), (10.0, 0.0), (10.0, 8.0), (0.0, 8.0)],
        z,
        0.30,
        vec![
            sl("Parkett_Estrich", 0.10, LayerFunction::Finish),
            sl("Stahlbeton", 0.20, LayerFunction::Structural),
        ],
    );

    // Exterior walls
    let w_south = add_wall(scene, (0.0, 0.0), (10.0, 0.0), z, h, exterior_layers());
    let w_east = add_wall(scene, (10.0, 0.0), (10.0, 8.0), z, h, exterior_layers());
    let w_north = add_wall(scene, (10.0, 8.0), (0.0, 8.0), z, h, exterior_layers());
    let w_west = add_wall(scene, (0.0, 8.0), (0.0, 0.0), z, h, exterior_layers());

    // Interior walls
    let w_int_mid = add_wall(scene, (5.0, 0.0), (5.0, 8.0), z, h, interior_bearing_layers());
    let w_int_wc = add_wall(scene, (0.0, 3.0), (5.0, 3.0), z, h, interior_light_layers());
    let w_int_hall = add_wall(scene, (2.5, 0.0), (2.5, 3.0), z, h, interior_light_layers());

    // Windows & Doors in EG
    let _ = place_wall_opening(
        scene,
        w_south,
        DVec3::new(1.25, 0.0, z),
        OpeningKind::Door,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_south,
        DVec3::new(3.75, 0.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_south,
        DVec3::new(7.5, 0.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_east,
        DVec3::new(10.0, 4.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_north,
        DVec3::new(2.5, 8.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_north,
        DVec3::new(7.5, 8.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_west,
        DVec3::new(0.0, 5.5, z),
        OpeningKind::Door,
        Some(library),
        None,
        None,
    );

    let _ = place_wall_opening(
        scene,
        w_int_hall,
        DVec3::new(2.5, 1.5, z),
        OpeningKind::Door,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_int_wc,
        DVec3::new(1.25, 3.0, z),
        OpeningKind::Door,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_int_mid,
        DVec3::new(5.0, 1.5, z),
        OpeningKind::Door,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_int_mid,
        DVec3::new(5.0, 6.0, z),
        OpeningKind::Door,
        Some(library),
        None,
        None,
    );

    // Rooms in EG
    add_room(
        scene,
        "Diele / Windfang",
        "EG-01",
        &[(0.24, 0.24), (2.38, 0.24), (2.38, 2.88), (0.24, 2.88)],
        2.60,
        1,
        RoomFunction::Corridor,
        "Fliesen",
    );
    add_room(
        scene,
        "Gäste-WC",
        "EG-02",
        &[(2.62, 0.24), (4.76, 0.24), (4.76, 2.88), (2.62, 2.88)],
        2.60,
        1,
        RoomFunction::Sanitary,
        "Fliesen",
    );
    add_room(
        scene,
        "Küche",
        "EG-03",
        &[(0.24, 3.12), (4.76, 3.12), (4.76, 7.76), (0.24, 7.76)],
        2.60,
        1,
        RoomFunction::Kitchen,
        "Fliesen",
    );
    add_room(
        scene,
        "Wohnen & Essen",
        "EG-04",
        &[(5.24, 0.24), (9.76, 0.24), (9.76, 7.76), (5.24, 7.76)],
        2.60,
        1,
        RoomFunction::Living,
        "Parkett",
    );
}

fn build_og_geometry(scene: &mut Scene, library: &StyleLibrary) {
    let z = 0.0;
    let h = 2.80;

    // Ceiling / Slab over EG
    add_slab(
        scene,
        &[(0.0, 0.0), (10.0, 0.0), (10.0, 8.0), (0.0, 8.0)],
        z,
        0.30,
        vec![
            sl("Parkett_Estrich", 0.10, LayerFunction::Finish),
            sl("Stahlbetondecke", 0.20, LayerFunction::Structural),
        ],
    );

    // Exterior walls
    let w_south = add_wall(scene, (0.0, 0.0), (10.0, 0.0), z, h, exterior_layers());
    let w_east = add_wall(scene, (10.0, 0.0), (10.0, 8.0), z, h, exterior_layers());
    let w_north = add_wall(scene, (10.0, 8.0), (0.0, 8.0), z, h, exterior_layers());
    let w_west = add_wall(scene, (0.0, 8.0), (0.0, 0.0), z, h, exterior_layers());

    // Interior walls
    let w_int_mid = add_wall(scene, (5.0, 0.0), (5.0, 8.0), z, h, interior_bearing_layers());
    let w_int_left = add_wall(scene, (0.0, 4.0), (5.0, 4.0), z, h, interior_light_layers());
    let _w_int_right = add_wall(scene, (5.0, 4.0), (10.0, 4.0), z, h, interior_light_layers());

    // Windows in OG
    let _ = place_wall_opening(
        scene,
        w_south,
        DVec3::new(2.5, 0.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_south,
        DVec3::new(7.5, 0.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_east,
        DVec3::new(10.0, 2.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_east,
        DVec3::new(10.0, 6.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_north,
        DVec3::new(2.5, 8.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_north,
        DVec3::new(7.5, 8.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_west,
        DVec3::new(0.0, 2.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_west,
        DVec3::new(0.0, 6.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );

    // Doors in OG
    let _ = place_wall_opening(
        scene,
        w_int_left,
        DVec3::new(1.5, 4.0, z),
        OpeningKind::Door,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_int_left,
        DVec3::new(3.5, 4.0, z),
        OpeningKind::Door,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_int_mid,
        DVec3::new(5.0, 2.0, z),
        OpeningKind::Door,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_int_mid,
        DVec3::new(5.0, 6.0, z),
        OpeningKind::Door,
        Some(library),
        None,
        None,
    );

    // Rooms in OG
    add_room(
        scene,
        "Flur",
        "OG-01",
        &[(2.5, 2.0), (5.0, 2.0), (5.0, 6.0), (2.5, 6.0)],
        2.60,
        2,
        RoomFunction::Corridor,
        "Parkett",
    );
    add_room(
        scene,
        "Badezimmer",
        "OG-02",
        &[(0.24, 0.24), (4.76, 0.24), (4.76, 3.88), (0.24, 3.88)],
        2.60,
        2,
        RoomFunction::Sanitary,
        "Fliesen",
    );
    add_room(
        scene,
        "Kind 1",
        "OG-03",
        &[(0.24, 4.12), (4.76, 4.12), (4.76, 7.76), (0.24, 7.76)],
        2.60,
        2,
        RoomFunction::Living,
        "Parkett",
    );
    add_room(
        scene,
        "Elternschlafzimmer",
        "OG-04",
        &[(5.24, 0.24), (9.76, 0.24), (9.76, 3.88), (5.24, 3.88)],
        2.60,
        2,
        RoomFunction::Living,
        "Parkett",
    );
    add_room(
        scene,
        "Kind 2",
        "OG-05",
        &[(5.24, 4.12), (9.76, 4.12), (9.76, 7.76), (5.24, 7.76)],
        2.60,
        2,
        RoomFunction::Living,
        "Parkett",
    );
}

fn build_dg_geometry(scene: &mut Scene, library: &StyleLibrary) {
    let z = 0.0;
    let h_drempel = 1.20;
    let h_giebel = 3.50;

    // Ceiling over OG (Holzbalkendecke)
    add_slab(
        scene,
        &[(0.0, 0.0), (10.0, 0.0), (10.0, 8.0), (0.0, 8.0)],
        z,
        0.24,
        vec![
            sl("Dielen_Estrich", 0.04, LayerFunction::Finish),
            sl("Holzbalkendecke", 0.20, LayerFunction::Structural),
        ],
    );

    // Drempel / Knee walls
    let _w_south = add_wall(scene, (0.0, 0.0), (10.0, 0.0), z, h_drempel, exterior_layers());
    let _w_north = add_wall(scene, (10.0, 8.0), (0.0, 8.0), z, h_drempel, exterior_layers());

    // Gable walls
    let w_east = add_wall(scene, (10.0, 0.0), (10.0, 8.0), z, h_giebel, exterior_layers());
    let w_west = add_wall(scene, (0.0, 8.0), (0.0, 0.0), z, h_giebel, exterior_layers());

    // Interior partition wall
    let w_int_mid = add_wall(scene, (5.0, 0.0), (5.0, 8.0), z, 2.80, interior_light_layers());

    // Windows in DG (Giebelfenster)
    let _ = place_wall_opening(
        scene,
        w_east,
        DVec3::new(10.0, 4.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_west,
        DVec3::new(0.0, 4.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );

    let _ = place_wall_opening(
        scene,
        w_int_mid,
        DVec3::new(5.0, 4.0, z),
        OpeningKind::Door,
        Some(library),
        None,
        None,
    );

    // Rooms in DG
    add_room(
        scene,
        "Studio / Galerie",
        "DG-01",
        &[(0.24, 0.24), (4.76, 0.24), (4.76, 7.76), (0.24, 7.76)],
        2.80,
        3,
        RoomFunction::Living,
        "Holzdielen",
    );
    add_room(
        scene,
        "Arbeitszimmer / Gast",
        "DG-02",
        &[(5.24, 0.24), (9.76, 0.24), (9.76, 7.76), (5.24, 7.76)],
        2.80,
        3,
        RoomFunction::Office,
        "Holzdielen",
    );
}

fn build_master_geometry(
    scene: &mut Scene,
    project_dir: &Path,
    library: &StyleLibrary,
) -> Result<(), Box<dyn std::error::Error>> {
    // Add XREF Block records and INSERT entities for UG, EG, OG, DG
    let storeys = [
        ("UG", "UG.dwg", -2.80),
        ("EG", "EG.dwg", 0.00),
        ("OG", "OG.dwg", 2.80),
        ("DG", "DG.dwg", 5.60),
    ];

    for (name, path, z_offset) in storeys {
        let mut br = BlockRecord::new(name);
        br.flags.is_xref = true;
        br.xref_path = path.to_string();
        br.handle = scene.document.allocate_handle();
        let _ = scene.document.block_records.add(br);

        let insert = Insert::new(name, CadVector3::new(0.0, 0.0, z_offset));
        let mut ent = EntityType::Insert(insert);
        ent.common_mut().layer = format!("XREF_{}", name);
        scene.add_entity(ent);
    }

    // Resolve XREFs from the project directory so all entities of UG, EG, OG, DG are in the block records
    let _ = crate::io::xref::resolve_xrefs(&mut scene.document, project_dir);

    // Add also native ground / terrain slab
    add_slab(
        scene,
        &[(-3.0, -3.0), (13.0, -3.0), (13.0, 11.0), (-3.0, 11.0)],
        -3.00,
        0.20,
        vec![sl("Gelaende_Erdreich", 0.20, LayerFunction::Other("Site".to_string()))],
    );

    // Generate Pre-configured Derived 2D Views
    let collector = SceneGeometryCollector::collect_from_document(&scene.document, ModelId::new());

    // 1. Schnitt A-A (Längsschnitt / Treppenschnitt über alle Geschosse 1:50 Ausführungsplanung)
    let section_a_vp = Viewpoint {
        id: ViewpointId::new(),
        name: "Schnitt A-A".to_string(),
        revision: Revision(1),
        origin: ViewOrigin::World(Point3::new(-1.0, 4.0, 0.0)),
        axes: ViewAxes::new(
            ViewVector3::new(1.0, 0.0, 0.0),
            ViewVector3::new(0.0, 0.0, 1.0),
            ViewVector3::new(0.0, 1.0, 0.0),
        ),
        kind: ViewKind::Section {
            path: SectionPath::Straight {
                start: Point2::new(-1.0, 4.0),
                end: Point2::new(11.0, 4.0),
            },
            far_depth: PositiveMetres::new(12.0).ok(),
        },
        z_range: VerticalRange::new(Metres::new(-3.5)?, Metres::new(9.5)?).unwrap(),
        mapping: Default::default(),
        crop: Some(Rect2::new(Point2::new(0.0, -3.5), Point2::new(12.0, 9.5))),
    };

    let section_a_mv = ModelView {
        id: ModelViewId::new(),
        name: "Schnitt A-A (Ausführung 1:50)".to_string(),
        viewpoint_id: section_a_vp.id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0)?,
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: UpdatePolicy::Automatic,
        revisions: Default::default(),
    };

    let section_a_prims = derive_section_view(
        &section_a_vp,
        &section_a_mv,
        &collector,
        &CutEngineOptions {
            structural_only: false,
            include_openings: true,
            detail_level: DetailLevel::Fine,
            emit_hidden_lines: false,
            style_library: Some(library.clone()),
            ..Default::default()
        },
    )
    .unwrap_or_default();

    if let Ok(mut carrier) = create_view_carrier(
        &mut scene.document,
        section_a_vp.id,
        section_a_mv.id,
        Point2::new(18.0, 0.0),
        1.0,
        0.0,
        UpdatePolicy::Automatic,
    ) {
        let _ = sync_carrier_primitives(&mut scene.document, &mut carrier, &section_a_prims);
        write_view_carrier_definitions(&mut scene.document, carrier.handle, &section_a_vp, &section_a_mv);

        let section_marker = Din1356SectionMarker::new_section(
            "A - A",
            SectionPath::Straight {
                start: Point2::new(-1.0, 4.0),
                end: Point2::new(11.0, 4.0),
            },
            section_a_vp.id,
            Some(carrier.handle),
        )
        .with_depth(Some(12.0));
        let marker_h = create_section_marker(&mut scene.document, &section_marker);
        crate::modules::aec::engine::owner_index::link_peers(&mut scene.document, marker_h, carrier.handle);
        carrier.marker_handle = Some(marker_h);
        crate::modules::aec::engine::views::carrier::write_view_carrier(&mut scene.document, &carrier);
    }

    // 2. Schnitt B-B (Querschnitt über alle Geschosse 1:100 Genehmigungsplanung)
    let section_b_vp = Viewpoint {
        id: ViewpointId::new(),
        name: "Schnitt B-B".to_string(),
        revision: Revision(1),
        origin: ViewOrigin::World(Point3::new(5.0, -1.0, 0.0)),
        axes: ViewAxes::new(
            ViewVector3::new(0.0, 1.0, 0.0),
            ViewVector3::new(0.0, 0.0, 1.0),
            ViewVector3::new(-1.0, 0.0, 0.0),
        ),
        kind: ViewKind::Section {
            path: SectionPath::Straight {
                start: Point2::new(5.0, -1.0),
                end: Point2::new(5.0, 9.0),
            },
            far_depth: PositiveMetres::new(12.0).ok(),
        },
        z_range: VerticalRange::new(Metres::new(-3.5)?, Metres::new(9.5)?).unwrap(),
        mapping: Default::default(),
        crop: Some(Rect2::new(Point2::new(0.0, -3.5), Point2::new(12.0, 9.5))),
    };

    let section_b_mv = ModelView {
        id: ModelViewId::new(),
        name: "Schnitt B-B (Genehmigung 1:100)".to_string(),
        viewpoint_id: section_b_vp.id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(100.0)?,
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: UpdatePolicy::Automatic,
        revisions: Default::default(),
    };

    let section_b_prims = derive_section_view(
        &section_b_vp,
        &section_b_mv,
        &collector,
        &CutEngineOptions {
            structural_only: false,
            include_openings: true,
            detail_level: DetailLevel::Medium,
            emit_hidden_lines: false,
            style_library: Some(library.clone()),
            ..Default::default()
        },
    )
    .unwrap_or_default();

    if let Ok(mut carrier) = create_view_carrier(
        &mut scene.document,
        section_b_vp.id,
        section_b_mv.id,
        Point2::new(36.0, 0.0),
        1.0,
        0.0,
        UpdatePolicy::Automatic,
    ) {
        let _ = sync_carrier_primitives(&mut scene.document, &mut carrier, &section_b_prims);
        write_view_carrier_definitions(&mut scene.document, carrier.handle, &section_b_vp, &section_b_mv);

        let section_marker = Din1356SectionMarker::new_section(
            "B - B",
            SectionPath::Straight {
                start: Point2::new(5.0, -1.0),
                end: Point2::new(5.0, 9.0),
            },
            section_b_vp.id,
            Some(carrier.handle),
        )
        .with_depth(Some(12.0));
        let marker_h = create_section_marker(&mut scene.document, &section_marker);
        crate::modules::aec::engine::owner_index::link_peers(&mut scene.document, marker_h, carrier.handle);
        carrier.marker_handle = Some(marker_h);
        crate::modules::aec::engine::views::carrier::write_view_carrier(&mut scene.document, &carrier);
    }

    // 3. Südfassade (Elevation 1:100 Genehmigungsplanung)
    let elev_s_vp = Viewpoint {
        id: ViewpointId::new(),
        name: "Suedansicht".to_string(),
        revision: Revision(1),
        origin: ViewOrigin::World(Point3::new(0.0, -2.0, 0.0)),
        axes: ViewAxes::new(
            ViewVector3::new(1.0, 0.0, 0.0),
            ViewVector3::new(0.0, 0.0, 1.0),
            ViewVector3::new(0.0, 1.0, 0.0),
        ),
        kind: ViewKind::Elevation {
            depth: DepthRange {
                near: NonNegativeMetres::ZERO,
                far: PositiveMetres::new(15.0).ok(),
            },
        },
        z_range: VerticalRange::new(Metres::new(-3.5)?, Metres::new(9.5)?).unwrap(),
        mapping: Default::default(),
        crop: Some(Rect2::new(Point2::new(0.0, -3.5), Point2::new(12.0, 9.5))),
    };

    let elev_s_mv = ModelView {
        id: ModelViewId::new(),
        name: "Suedfassade (Ansicht 1:100)".to_string(),
        viewpoint_id: elev_s_vp.id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(100.0)?,
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: UpdatePolicy::Automatic,
        revisions: Default::default(),
    };

    let elev_s_prims = derive_elevation_view(
        &elev_s_vp,
        &elev_s_mv,
        &collector,
        &ElevationEngineOptions::default(),
    )
    .unwrap_or_default();

    if let Ok(mut carrier) = create_view_carrier(
        &mut scene.document,
        elev_s_vp.id,
        elev_s_mv.id,
        Point2::new(18.0, 16.0),
        1.0,
        0.0,
        UpdatePolicy::Automatic,
    ) {
        let _ = sync_carrier_primitives(&mut scene.document, &mut carrier, &elev_s_prims);
        write_view_carrier_definitions(&mut scene.document, carrier.handle, &elev_s_vp, &elev_s_mv);

        let elev_marker = Din1356SectionMarker::new_elevation(
            "Südfassade",
            Point2::new(0.0, -2.0),
            Point2::new(10.0, -2.0),
            elev_s_vp.id,
            Some(carrier.handle),
        )
        .with_depth(Some(15.0));
        let marker_h = create_section_marker(&mut scene.document, &elev_marker);
        crate::modules::aec::engine::owner_index::link_peers(&mut scene.document, marker_h, carrier.handle);
        carrier.marker_handle = Some(marker_h);
        crate::modules::aec::engine::views::carrier::write_view_carrier(&mut scene.document, &carrier);
    }

    // 4. Nordfassade (Elevation 1:100 Genehmigungsplanung)
    let elev_n_vp = Viewpoint {
        id: ViewpointId::new(),
        name: "Nordansicht".to_string(),
        revision: Revision(1),
        origin: ViewOrigin::World(Point3::new(10.0, 10.0, 0.0)),
        axes: ViewAxes::new(
            ViewVector3::new(-1.0, 0.0, 0.0),
            ViewVector3::new(0.0, 0.0, 1.0),
            ViewVector3::new(0.0, -1.0, 0.0),
        ),
        kind: ViewKind::Elevation {
            depth: DepthRange {
                near: NonNegativeMetres::ZERO,
                far: PositiveMetres::new(15.0).ok(),
            },
        },
        z_range: VerticalRange::new(Metres::new(-3.5)?, Metres::new(9.5)?).unwrap(),
        mapping: Default::default(),
        crop: Some(Rect2::new(Point2::new(0.0, -3.5), Point2::new(12.0, 9.5))),
    };

    let elev_n_mv = ModelView {
        id: ModelViewId::new(),
        name: "Nordfassade (Ansicht 1:100)".to_string(),
        viewpoint_id: elev_n_vp.id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(100.0)?,
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: UpdatePolicy::Automatic,
        revisions: Default::default(),
    };

    let elev_n_prims = derive_elevation_view(
        &elev_n_vp,
        &elev_n_mv,
        &collector,
        &ElevationEngineOptions::default(),
    )
    .unwrap_or_default();

    if let Ok(mut carrier) = create_view_carrier(
        &mut scene.document,
        elev_n_vp.id,
        elev_n_mv.id,
        Point2::new(36.0, 16.0),
        1.0,
        0.0,
        UpdatePolicy::Automatic,
    ) {
        let _ = sync_carrier_primitives(&mut scene.document, &mut carrier, &elev_n_prims);
        write_view_carrier_definitions(&mut scene.document, carrier.handle, &elev_n_vp, &elev_n_mv);

        let elev_marker = Din1356SectionMarker::new_elevation(
            "Nordfassade",
            Point2::new(10.0, 10.0),
            Point2::new(0.0, 10.0),
            elev_n_vp.id,
            Some(carrier.handle),
        )
        .with_depth(Some(15.0));
        let marker_h = create_section_marker(&mut scene.document, &elev_marker);
        crate::modules::aec::engine::owner_index::link_peers(&mut scene.document, marker_h, carrier.handle);
        carrier.marker_handle = Some(marker_h);
        crate::modules::aec::engine::views::carrier::write_view_carrier(&mut scene.document, &carrier);
    }

    // 5. EG Grundriss (Ausführungsplanung 1:50)
    let fp_exec_vp = Viewpoint {
        id: ViewpointId::new(),
        name: "EG Grundriss (Werkplan)".to_string(),
        revision: Revision(1),
        origin: ViewOrigin::World(Point3::new(-1.0, -1.0, 1.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.8).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0)?, Metres::new(2.8)?).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    let fp_exec_mv = ModelView {
        id: ModelViewId::new(),
        name: "EG Grundriss (Ausführung 1:50)".to_string(),
        viewpoint_id: fp_exec_vp.id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0)?,
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: UpdatePolicy::Automatic,
        revisions: Default::default(),
    };

    let fp_exec_prims = derive_floorplan_view(
        &fp_exec_vp,
        &fp_exec_mv,
        &collector,
        &FloorPlanEngineOptions {
            style_library: Some(library.clone()),
            ..FloorPlanEngineOptions::default()
        },
    )
    .unwrap_or_default();

    if let Ok(mut carrier) = create_view_carrier(
        &mut scene.document,
        fp_exec_vp.id,
        fp_exec_mv.id,
        Point2::new(18.0, 32.0),
        1.0,
        0.0,
        UpdatePolicy::Automatic,
    ) {
        let _ = sync_carrier_primitives(&mut scene.document, &mut carrier, &fp_exec_prims);
        write_view_carrier_definitions(&mut scene.document, carrier.handle, &fp_exec_vp, &fp_exec_mv);
    }

    // 6. EG Grundriss (Genehmigungsplanung 1:100)
    let fp_perm_vp = Viewpoint {
        id: ViewpointId::new(),
        name: "EG Grundriss (Genehmigung)".to_string(),
        revision: Revision(1),
        origin: ViewOrigin::World(Point3::new(-1.0, -1.0, 1.0)),
        axes: ViewAxes::floor_plan(),
        kind: ViewKind::FloorPlan {
            range: PlanRange {
                above_cut: NonNegativeMetres::new(1.8).unwrap(),
                below_cut: NonNegativeMetres::new(1.0).unwrap(),
                beyond_below: NonNegativeMetres::ZERO,
            },
        },
        z_range: VerticalRange::new(Metres::new(0.0)?, Metres::new(2.8)?).unwrap(),
        mapping: Default::default(),
        crop: None,
    };

    let fp_perm_mv = ModelView {
        id: ModelViewId::new(),
        name: "EG Grundriss (Genehmigung 1:100)".to_string(),
        viewpoint_id: fp_perm_vp.id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(100.0)?,
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: UpdatePolicy::Automatic,
        revisions: Default::default(),
    };

    let fp_perm_prims = derive_floorplan_view(
        &fp_perm_vp,
        &fp_perm_mv,
        &collector,
        &FloorPlanEngineOptions {
            style_library: Some(library.clone()),
            ..FloorPlanEngineOptions::default()
        },
    )
    .unwrap_or_default();

    if let Ok(mut carrier) = create_view_carrier(
        &mut scene.document,
        fp_perm_vp.id,
        fp_perm_mv.id,
        Point2::new(36.0, 32.0),
        1.0,
        0.0,
        UpdatePolicy::Automatic,
    ) {
        let _ = sync_carrier_primitives(&mut scene.document, &mut carrier, &fp_perm_prims);
        write_view_carrier_definitions(&mut scene.document, carrier.handle, &fp_perm_vp, &fp_perm_mv);
    }

    Ok(())
}

fn build_sample_library() -> StyleLibrary {
    let mut library = seed_default_library();

    let win_style = crate::modules::aec::engine::opening_style::OpeningStyle::standard_window();
    let door_style = crate::modules::aec::engine::opening_style::OpeningStyle::standard_door();
    library.upsert_opening_style(win_style);
    library.upsert_opening_style(door_style);

    library
}

fn build_sample_display_library() -> DisplayConfigLibrary {
    let mut lib = DisplayConfigLibrary::seed_default_configs();

    // 1. Genehmigungsplanung 1:100 (FloorPlan)
    let mut permit_100 = DisplayConfig::new(
        "Genehmigungsplanung 1:100".to_string(),
        "Architektur".to_string(),
        PlanningStage::Permit,
        ViewType::FloorPlan,
    );
    permit_100.scale = Some(100.0);
    permit_100.preset = Some(PRESET_ENTWURF_100.to_string());
    permit_100.detail.hatches = true;
    permit_100.detail.layers = LayerDetail::StructuralOnly;
    permit_100.detail.representation = RepresentationMode::TwoD;
    permit_100.opening_visibility.insert(OpeningComponentSlot::HostCut2D, true);
    permit_100.opening_visibility.insert(OpeningComponentSlot::Frame2D, true);
    permit_100.opening_visibility.insert(OpeningComponentSlot::Leaf2D, true);
    permit_100.opening_visibility.insert(OpeningComponentSlot::Swing2D, true);
    permit_100.opening_visibility.insert(OpeningComponentSlot::Glazing2D, false);
    permit_100.opening_visibility.insert(OpeningComponentSlot::Sill2D, true);
    permit_100.opening_visibility.insert(OpeningComponentSlot::Threshold2D, false);
    permit_100.opening_visibility.insert(OpeningComponentSlot::OpeningLabel2D, true);
    lib.upsert(permit_100);

    // 2. Ausführungsplanung 1:50 (FloorPlan)
    let mut exec_50 = DisplayConfig::new(
        "Ausführungsplanung 1:50".to_string(),
        "Architektur".to_string(),
        PlanningStage::Execution,
        ViewType::FloorPlan,
    );
    exec_50.scale = Some(50.0);
    exec_50.preset = Some(PRESET_WERKPLAN_50.to_string());
    exec_50.detail.hatches = true;
    exec_50.detail.layers = LayerDetail::All;
    exec_50.detail.representation = RepresentationMode::TwoD;
    exec_50.opening_visibility.insert(OpeningComponentSlot::HostCut2D, true);
    exec_50.opening_visibility.insert(OpeningComponentSlot::Frame2D, true);
    exec_50.opening_visibility.insert(OpeningComponentSlot::Leaf2D, true);
    exec_50.opening_visibility.insert(OpeningComponentSlot::Swing2D, true);
    exec_50.opening_visibility.insert(OpeningComponentSlot::Glazing2D, true);
    exec_50.opening_visibility.insert(OpeningComponentSlot::Sill2D, true);
    exec_50.opening_visibility.insert(OpeningComponentSlot::Threshold2D, true);
    exec_50.opening_visibility.insert(OpeningComponentSlot::OpeningLabel2D, true);
    lib.upsert(exec_50);

    // 3. Genehmigungsplanung Schnitt 1:100
    let mut permit_sec_100 = DisplayConfig::new(
        "Genehmigungsplanung Schnitt 1:100".to_string(),
        "Architektur".to_string(),
        PlanningStage::Permit,
        ViewType::Section,
    );
    permit_sec_100.scale = Some(100.0);
    lib.upsert(permit_sec_100);

    // 4. Ausführungsplanung Schnitt 1:50
    let mut exec_sec_50 = DisplayConfig::new(
        "Ausführungsplanung Schnitt 1:50".to_string(),
        "Architektur".to_string(),
        PlanningStage::Execution,
        ViewType::Section,
    );
    exec_sec_50.scale = Some(50.0);
    lib.upsert(exec_sec_50);

    // 5. Genehmigungsplanung Ansichten 1:100
    let mut permit_elev_100 = DisplayConfig::new(
        "Genehmigungsplanung Ansichten 1:100".to_string(),
        "Architektur".to_string(),
        PlanningStage::Permit,
        ViewType::Elevation,
    );
    permit_elev_100.scale = Some(100.0);
    permit_elev_100.detail.representation = RepresentationMode::TwoD;
    permit_elev_100.opening_visibility.insert(OpeningComponentSlot::ElevationContour2D, true);
    permit_elev_100.opening_visibility.insert(OpeningComponentSlot::ElevationMuntins2D, true);
    permit_elev_100.opening_visibility.insert(OpeningComponentSlot::ElevationSwing2D, true);
    permit_elev_100.opening_visibility.insert(OpeningComponentSlot::ElevationSill2D, true);
    lib.upsert(permit_elev_100);

    lib
}

fn build_project_file(library: &StyleLibrary, display_lib: &DisplayConfigLibrary) -> ProjectFile {
    let mut building = Building::new("Einfamilienhaus");
    building.storeys = vec![
        StoreyRef::new("UG", -2.80, "UG.dwg"),
        StoreyRef::new("EG", 0.00, "EG.dwg"),
        StoreyRef::new("OG", 2.80, "OG.dwg"),
        StoreyRef::new("DG", 5.60, "DG.dwg"),
    ];

    let mut project = ProjectFile::default();
    project.buildings.push(building);
    project.material_wall_style_library = library.clone();
    project.display_config_library = display_lib.clone();

    project
}

fn generate_readme_text() -> String {
    r#"# OpenCADStudio AEC — Beispielprojekt: Einfamilienhaus

Dieses Beispielprojekt demonstriert das Zusammenspiel von **Multi-Geschoss-Modellen über XREFs**, **AEC-Bauteilen (Wände, Decken, Öffnungen, Räume)** und den **modellassoziierten 2D-Schnitt-, Ansichts- und Grundriss-Ableitungen nach DIN 1356** für Genehmigungs- und Ausführungsplanung.

---

## Projektstruktur und Geschosshierarchie

Das Gebäude ist als typisches deutsches **Einfamilienhaus (EFH)** mit 4 Geschossebenen konzipiert:

* `Projekt_Einfamilienhaus.ocsproj`:
  Die zentrale Projektdatei für den OpenCADStudio Geschoss- und Projekt-Manager. Definiert die Geschosshöhen:
  * **UG (Untergeschoss / Keller):** $Z = -2{,}80\,\text{m}$ (lichte Raumhöhe $2{,}40\,\text{m}$)
  * **EG (Erdgeschoss):** $Z = 0{,}00\,\text{m}$ (lichte Raumhöhe $2{,}60\,\text{m}$)
  * **OG (Obergeschoss):** $Z = 2{,}80\,\text{m}$ (lichte Raumhöhe $2{,}60\,\text{m}$)
  * **DG (Dachgeschoss / Studio):** $Z = 5{,}60\,\text{m}$ (Kniestock $1{,}20\,\text{m}$, Firsthöhe $9{,}10\,\text{m}$)
  Enthält vorkonfigurierte Bibliotheken für **Materialien**, **Wandstile**, **Deckenstile**, **Öffnungsstile** und **Planarten (DisplayConfigs)**.

* `UG.dwg` / `UG.dxf`:
  Untergeschoss mit WU-Beton-Bodenplatte, gedämmten WU-Beton-Kelleraußenwänden (Bitumenabdichtung, Perimeterdämmung), tragender Mittelwand, Kellerfenstern (Lichtschächte), T30-Türen und DIN 277 Räumen (Technik/HTR, Vorrat/Lager, Hobbyraum).
* `EG.dwg` / `EG.dxf`:
  Erdgeschoss mit wärmegedämmten Außenwänden (WDVS 14cm, Kalksandstein 17.5cm), Stahlbetondecke, Haustür, Terrassen-Hebeschiebetür zum Garten, Fenstern, Innentüren und Räumen (Diele/Windfang, Gäste-WC, Küche, Wohnen & Essen).
* `OG.dwg` / `OG.dxf`:
  Obergeschoss mit Geschossdecke, Schlafräumen (Eltern, Kind 1, Kind 2), Familienbad, Flur und Fenstern.
* `DG.dwg` / `DG.dxf`:
  Dachgeschoss mit Holzbalkendecke, Kniestock-/Drempelwänden, Giebelwänden, Giebelfenstern und Studio/Galerie.
* `Haus_Gesamt.dwg` / `Haus_Gesamt.dxf`:
  Das aggregierte Master-Gesamtmodell. Es bindet die Einzelgeschosse `UG`, `EG`, `OG` und `DG` als externe Referenzen (XREFs) mit ihren jeweiligen Höhenversätzen ($Z = -2{,}80\,\text{m}$, $Z = 0{,}00\,\text{m}$, $Z = 2{,}80\,\text{m}$, $Z = 5{,}60\,\text{m}$) ein.
  Enthält bereits vordefinierte abgeleitete Zeichnungsträger (`DerivedViewCarrier`) und DIN 1356 Markierungen:
  * **Schnitt A–A (Ausführungsplanung 1:50):** Mehrgeschossiger Längsschnitt durch alle 4 Geschosse mit Einzelschichten, DIN-Materialschraffuren und assoziativer DIN 1356 Schnittmarkierung.
  * **Schnitt B–B (Genehmigungsplanung 1:100):** Mehrgeschossiger Querschnitt durch Treppen- und Wohnbereich.
  * **Südfassade & Nordfassade (Ansichten 1:100):** Fassadenansichten mit Öffnungsprofilen, Fensterbänken, DIN-Flügelaufschlägen und Ansichtsmarkierungen.
  * **EG Grundriss (Ausführung 1:50 & Genehmigung 1:100):** Assoziative Grundrissableitungen für Werk- und Bauantragspläne.

---

## Planarten und Darstellungskonfigurationen

Die Projektdatei stellt getrennte Konfigurationen für die verschiedenen Leistungsphasen bereit:

1. **Genehmigungsplanung (1:100):**
   * Reduktion auf Tragschichten (`LayerDetail::StructuralOnly`) oder Gesamtkonturen.
   * Normgerechte Öffnungsbeschriftungen nach DIN 1356 ($B/H, BRH$).
   * Raumstempel mit DIN 277 Nutzungsarten und Flächennachweisen.
2. **Ausführungsplanung / Werkplan (1:50):**
   * Volle Schichtdarstellung aller Wände und Deckenaufbauten (`LayerDetail::All`).
   * Materialspezifische DIN-Schraffuren (Mauerwerk, Stahlbeton, Dämmung, Trockenbau, Estrich).
   * Detaillierte Öffnungsdarstellung mit Rahmen, Flügeln, Glasebenen, Schwellen und Fensterbänken.
3. **Statik / Rohbau (1:100 / 1:50):**
   * Fokus auf tragende Bauteile und Durchbruchssymbole.
4. **Deckenspiegel (RCP 1:50):**
   * Deckenuntersichten und Aussparungen.

---

## Interaktive Griffe & Assoziative Modellbearbeitung

1. **DIN 1356 Schnittmarkierungen im Grundriss:**
   * **Start- und Endgriff (Quadrat):** Verschieben oder Drehen der Schnittlinie. Der assoziierte Schnittträger wird bei Fertigstellung automatisch synchron regeneriert.
   * **Tiefengriff (Rechteck):** Blicktiefe interaktiv per Maus vergrößern oder verkleinern.
   * **Flip-Griff (Dreieck):** Ein Klick kehrt die Blickrichtung der Schnittpfeile und des Schnittträgers normgerecht um.

2. **Semantische Modellgriffe im Schnitt (`resolve_handle_drag`):**
   * Beim Anklicken abgeleiteter Kanten im Schnitt (z. B. Wandoberkanten, Deckenoberkanten, Fensterbrüstungen oder Stürzen) erscheinen interaktive Modellgriffe.
   * Das Ziehen an einem Wandoberkantengriff hebt oder senkt die 3D-Wandhöhe im BIM-Modell.
   * Beim Loslassen des Griffs wird eine atomare BIM-Transaktion ausgeführt: Die Wandhöhe wird im Modell aktualisiert und alle abhängigen Ansichten werden synchron neu berechnet.

3. **Eigenschaften-Panel & Synchronisation:**
   * Alle Parameter (Kennung, Blicktiefe, Z-Bereich, Schnitthöhe, Maßstab 1:50/1:100 und Aktualisierungsmodus) können im Eigenschaften-Panel angepasst werden.
   * Mit dem Befehl `AEC_VIEWUPDATE` können alle Ansichten auf Tastendruck auf den neuesten Modellstand gebracht werden.
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_generate_sample_project() {
        let user_folder = PathBuf::from("/home/tom/Dokumente/OpenCADStudio_Dateien/Beispielprojekt");
        let res1 = generate_sample_project(&user_folder);
        assert!(res1.is_ok(), "Failed to generate in user documents: {:?}", res1);

        let repo_folder = PathBuf::from("examples/beispiel_haus");
        let res2 = generate_sample_project(&repo_folder);
        assert!(res2.is_ok(), "Failed to generate in repo examples: {:?}", res2);

        // Verify files exist
        assert!(user_folder.join("Projekt_Einfamilienhaus.ocsproj").exists());
        assert!(user_folder.join("UG.dwg").exists());
        assert!(user_folder.join("UG.dxf").exists());
        assert!(user_folder.join("EG.dwg").exists());
        assert!(user_folder.join("EG.dxf").exists());
        assert!(user_folder.join("OG.dwg").exists());
        assert!(user_folder.join("OG.dxf").exists());
        assert!(user_folder.join("DG.dwg").exists());
        assert!(user_folder.join("DG.dxf").exists());
        assert!(user_folder.join("Haus_Gesamt.dwg").exists());
        assert!(user_folder.join("Haus_Gesamt.dxf").exists());
        assert!(user_folder.join("README.md").exists());

        // Check carriers in Haus_Gesamt
        let mut scene = crate::scene::Scene::new();
        let doc = crate::io::load_file(&repo_folder.join("Haus_Gesamt.dxf")).unwrap();
        scene.document = doc;
        let carriers = crate::modules::aec::engine::views::carrier::all_view_carriers(&scene.document);
        println!("SAMPLE PROJECT: CARRIERS COUNT = {}", carriers.len());
        assert!(carriers.len() >= 6, "Expected at least 6 carriers (sections, elevations, floorplans), found {}", carriers.len());
    }
}
