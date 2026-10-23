//! Sample AEC Project generator for OpenCADStudio.
//!
//! Creates a complete multi-storey residential building ("Einfamilienhaus")
//! with single storey drawings (EG, OG, DG), an aggregated master model
//! (Haus_Gesamt with XREFs and derived sections/elevations), and a project file (.ocsproj).

use std::fs;
use std::path::Path;

use codec::entities::{Insert, LwPolyline, LwVertex};
use codec::tables::block_record::BlockRecord;
use codec::types::{Vector2, Vector3 as CadVector3};
use codec::xdata::ExtendedDataRecord;
use codec::{DxfVersion, EntityType};
use glam::DVec3;

use crate::io::save_as_version;
use crate::modules::aec::engine::library::{DisplayConfigLibrary, StyleLibrary};
use crate::modules::aec::engine::opening_xdata::place_wall_opening;
use crate::modules::aec::engine::openings::OpeningKind;
use crate::modules::aec::engine::project::{Building, ProjectFile, StoreyRef};
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
use crate::modules::aec::engine::views::model_view::DetailLevel;
use crate::modules::aec::engine::views::scene_collector::SceneGeometryCollector;
use crate::modules::aec::engine::views::Vector3 as ViewVector3;
use crate::modules::aec::engine::views::*;
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

    let mut library = StyleLibrary::empty();
    let win_style = crate::modules::aec::engine::opening_style::OpeningStyle::standard_window();
    let door_style = crate::modules::aec::engine::opening_style::OpeningStyle::standard_door();
    library.upsert_opening_style(win_style);
    library.upsert_opening_style(door_style);

    // 1. Generate EG (Ground Floor, Z = 0.0m)
    let mut eg_scene = Scene::new();
    build_eg_geometry(&mut eg_scene, &library);
    save_drawing_variants(&eg_scene, &target_dir.join("EG"))?;

    // 2. Generate OG (Upper Floor, Z = 2.80m)
    let mut og_scene = Scene::new();
    build_og_geometry(&mut og_scene, &library);
    save_drawing_variants(&og_scene, &target_dir.join("OG"))?;

    // 3. Generate DG (Attic / Roof, Z = 5.60m)
    let mut dg_scene = Scene::new();
    build_dg_geometry(&mut dg_scene, &library);
    save_drawing_variants(&dg_scene, &target_dir.join("DG"))?;

    // 4. Generate Master Gesamtmodell (Haus_Gesamt, attaches EG, OG, DG with Z-offsets)
    let mut master_scene = Scene::new();
    build_master_geometry(&mut master_scene, target_dir, &library)?;
    save_drawing_variants(&master_scene, &target_dir.join("Haus_Gesamt"))?;

    // 5. Generate Project File (.ocsproj)
    let project_file = build_project_file(&library);
    let proj_path = target_dir.join("Projekt_Einfamilienhaus.ocsproj");
    project_file.save(&proj_path)?;

    // 6. Generate README Documentation
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

fn exterior_layers() -> Vec<WallLayer> {
    vec![
        wl("Außenputz", 0.015, "Substrate"),
        wl("Wärmedämmung", 0.140, "Thermal"),
        wl("Kalksandstein", 0.175, "Structural"),
        wl("Innenputz", 0.015, "Substrate"),
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

fn build_eg_geometry(scene: &mut Scene, library: &StyleLibrary) {
    let z = 0.0;
    let h = 2.80;

    // Floor slab (Bodenplatte)
    add_slab(
        scene,
        &[(0.0, 0.0), (10.0, 0.0), (10.0, 8.0), (0.0, 8.0)],
        z,
        0.25,
        vec![
            sl("Estrich_Dämmung", 0.10, LayerFunction::Finish),
            sl("Stahlbeton", 0.25, LayerFunction::Structural),
        ],
    );

    // Exterior walls
    let w_south = add_wall(scene, (0.0, 0.0), (10.0, 0.0), z, h, exterior_layers());
    let w_east = add_wall(scene, (10.0, 0.0), (10.0, 8.0), z, h, exterior_layers());
    let w_north = add_wall(scene, (10.0, 8.0), (0.0, 8.0), z, h, exterior_layers());
    let w_west = add_wall(scene, (0.0, 8.0), (0.0, 0.0), z, h, exterior_layers());

    // Interior walls
    let w_int_mid = add_wall(scene, (5.0, 0.0), (5.0, 8.0), z, h, interior_bearing_layers());
    let _w_int_wc = add_wall(scene, (0.0, 3.0), (5.0, 3.0), z, h, interior_light_layers());

    // Windows & Doors in EG
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
        OpeningKind::Door,
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
        DVec3::new(3.0, 8.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_west,
        DVec3::new(0.0, 5.0, z),
        OpeningKind::Door,
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
}

fn build_og_geometry(scene: &mut Scene, library: &StyleLibrary) {
    let z = 0.0;
    let h = 2.80;

    // Ceiling / Slab over EG
    add_slab(
        scene,
        &[(0.0, 0.0), (10.0, 0.0), (10.0, 8.0), (0.0, 8.0)],
        z,
        0.20,
        vec![
            sl("Parkett_Estrich", 0.08, LayerFunction::Finish),
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
    let _w_int_hall = add_wall(scene, (0.0, 4.0), (5.0, 4.0), z, h, interior_light_layers());

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
        DVec3::new(10.0, 4.0, z),
        OpeningKind::Window,
        Some(library),
        None,
        None,
    );
    let _ = place_wall_opening(
        scene,
        w_north,
        DVec3::new(3.0, 8.0, z),
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
        w_int_mid,
        DVec3::new(5.0, 3.0, z),
        OpeningKind::Door,
        Some(library),
        None,
        None,
    );
}

fn build_dg_geometry(scene: &mut Scene, _library: &StyleLibrary) {
    let z = 0.0;
    let h_drempel = 1.20;
    let h_giebel = 3.20;

    // Ceiling over OG
    add_slab(
        scene,
        &[(0.0, 0.0), (10.0, 0.0), (10.0, 8.0), (0.0, 8.0)],
        z,
        0.20,
        vec![sl("Holzbalkendecke", 0.20, LayerFunction::Structural)],
    );

    // Drempel / Knee walls
    let _w_south = add_wall(scene, (0.0, 0.0), (10.0, 0.0), z, h_drempel, exterior_layers());
    let _w_north = add_wall(scene, (10.0, 8.0), (0.0, 8.0), z, h_drempel, exterior_layers());

    // Gable walls
    let _w_east = add_wall(scene, (10.0, 0.0), (10.0, 8.0), z, h_giebel, exterior_layers());
    let _w_west = add_wall(scene, (0.0, 8.0), (0.0, 0.0), z, h_giebel, exterior_layers());
}

fn build_master_geometry(
    scene: &mut Scene,
    project_dir: &Path,
    library: &StyleLibrary,
) -> Result<(), Box<dyn std::error::Error>> {
    // Add XREF Block records and INSERT entities for EG, OG, DG
    let storeys = [
        ("EG", "EG.dwg", 0.0),
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

    // Resolve XREFs from the project directory so all entities of EG, OG, DG are in the block records
    let _ = crate::io::xref::resolve_xrefs(&mut scene.document, project_dir);

    // Add also native ground / terrain slab
    add_slab(
        scene,
        &[(-3.0, -3.0), (13.0, -3.0), (13.0, 11.0), (-3.0, 11.0)],
        -0.20,
        0.20,
        vec![sl("Gelaende_Erdreich", 0.20, LayerFunction::Other("Site".to_string()))],
    );

    // Generate Pre-configured Derived 2D Views (Schnitt A-A and Südansicht)
    let collector = SceneGeometryCollector::collect_from_document(&scene.document, ModelId::new());

    // 1. Schnitt A-A
    let section_vp = Viewpoint {
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
        z_range: VerticalRange::new(Metres::new(-0.5)?, Metres::new(9.5)?).unwrap(),
        mapping: Default::default(),
        crop: Some(Rect2::new(Point2::new(0.0, -0.5), Point2::new(12.0, 9.5))),
    };

    let section_mv = ModelView {
        id: ModelViewId::new(),
        name: "Schnitt A-A (Ausführung 1:50)".to_string(),
        viewpoint_id: section_vp.id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0)?,
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: UpdatePolicy::Automatic,
        revisions: Default::default(),
    };

    let section_prims = derive_section_view(
        &section_vp,
        &section_mv,
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
        section_vp.id,
        section_mv.id,
        Point2::new(18.0, 0.0),
        1.0,
        0.0,
        UpdatePolicy::Automatic,
    ) {
        let _ = sync_carrier_primitives(&mut scene.document, &mut carrier, &section_prims);
        write_view_carrier_definitions(&mut scene.document, carrier.handle, &section_vp, &section_mv);

        // DIN 1356 Schnittmarkierung im Grundrissbereich anlegen
        use crate::modules::aec::views::din1356::{create_section_marker, Din1356SectionMarker};
        let section_marker = Din1356SectionMarker::new_section(
            "A - A",
            SectionPath::Straight {
                start: Point2::new(-1.0, 4.0),
                end: Point2::new(11.0, 4.0),
            },
            section_vp.id,
            Some(carrier.handle),
        )
        .with_depth(Some(12.0));
        let marker_h = create_section_marker(&mut scene.document, &section_marker);
        crate::modules::aec::engine::owner_index::link_peers(&mut scene.document, marker_h, carrier.handle);
        carrier.marker_handle = Some(marker_h);
        crate::modules::aec::engine::views::carrier::write_view_carrier(&mut scene.document, &carrier);
    }

    // 2. Südansicht (Elevation)
    let elev_vp = Viewpoint {
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
        z_range: VerticalRange::new(Metres::new(0.0)?, Metres::new(9.5)?).unwrap(),
        mapping: Default::default(),
        crop: Some(Rect2::new(Point2::new(0.0, 0.0), Point2::new(12.0, 9.5))),
    };

    let elev_mv = ModelView {
        id: ModelViewId::new(),
        name: "Suedfassade (Ansicht 1:100)".to_string(),
        viewpoint_id: elev_vp.id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(100.0)?,
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: UpdatePolicy::Automatic,
        revisions: Default::default(),
    };

    let elev_prims = derive_elevation_view(
        &elev_vp,
        &elev_mv,
        &collector,
        &ElevationEngineOptions::default(),
    )
    .unwrap_or_default();

    if let Ok(mut carrier) = create_view_carrier(
        &mut scene.document,
        elev_vp.id,
        elev_mv.id,
        Point2::new(18.0, 12.0),
        1.0,
        0.0,
        UpdatePolicy::Automatic,
    ) {
        let _ = sync_carrier_primitives(&mut scene.document, &mut carrier, &elev_prims);
        write_view_carrier_definitions(&mut scene.document, carrier.handle, &elev_vp, &elev_mv);

        // DIN 1356 Ansichtsmarkierung anlegen
        use crate::modules::aec::views::din1356::{create_section_marker, Din1356SectionMarker};
        let elev_marker = Din1356SectionMarker::new_elevation(
            "Südfassade",
            Point2::new(0.0, -2.0),
            Point2::new(10.0, -2.0),
            elev_vp.id,
            Some(carrier.handle),
        )
        .with_depth(Some(15.0));
        let marker_h = create_section_marker(&mut scene.document, &elev_marker);
        crate::modules::aec::engine::owner_index::link_peers(&mut scene.document, marker_h, carrier.handle);
        carrier.marker_handle = Some(marker_h);
        crate::modules::aec::engine::views::carrier::write_view_carrier(&mut scene.document, &carrier);
    }

    // 3. EG Grundriss (Floor Plan 1:50) mit lokalisierter Projektion
    let fp_vp = Viewpoint {
        id: ViewpointId::new(),
        name: "EG Grundriss".to_string(),
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

    let fp_mv = ModelView {
        id: ModelViewId::new(),
        name: "EG Grundriss (1:50)".to_string(),
        viewpoint_id: fp_vp.id,
        profile_id: ViewProfileId::new(),
        scale: ScaleDenominator::new(50.0)?,
        detail: DetailPolicy::FromProfileAndScale,
        excluded_elements: Default::default(),
        graphic_overrides: Vec::new(),
        annotation_ids: Vec::new(),
        update_policy: UpdatePolicy::Automatic,
        revisions: Default::default(),
    };

    use crate::modules::aec::engine::views::floorplan_engine::{derive_floorplan_view, FloorPlanEngineOptions};
    let fp_prims = derive_floorplan_view(
        &fp_vp,
        &fp_mv,
        &collector,
        &FloorPlanEngineOptions {
            style_library: Some(library.clone()),
            ..FloorPlanEngineOptions::default()
        },
    )
    .unwrap_or_default();

    if let Ok(mut carrier) = create_view_carrier(
        &mut scene.document,
        fp_vp.id,
        fp_mv.id,
        Point2::new(18.0, 24.0),
        1.0,
        0.0,
        UpdatePolicy::Automatic,
    ) {
        let _ = sync_carrier_primitives(&mut scene.document, &mut carrier, &fp_prims);
        write_view_carrier_definitions(&mut scene.document, carrier.handle, &fp_vp, &fp_mv);
    }

    Ok(())
}

fn build_project_file(library: &StyleLibrary) -> ProjectFile {
    let mut building = Building::new("Einfamilienhaus");
    building.storeys = vec![
        StoreyRef::new("EG", 0.0, "EG.dwg"),
        StoreyRef::new("OG", 2.80, "OG.dwg"),
        StoreyRef::new("DG", 5.60, "DG.dwg"),
    ];

    let mut project = ProjectFile::default();
    project.buildings.push(building);
    project.material_wall_style_library = library.clone();
    project.display_config_library = DisplayConfigLibrary::default();

    project
}

fn generate_readme_text() -> String {
    r#"# OpenCADStudio AEC — Beispielprojekt: Einfamilienhaus

Dieses Beispielprojekt demonstriert das Zusammenspiel von **Multi-Geschoss-Modellen über XREFs**, **AEC-Bauteilen (Wände, Decken, Öffnungen)** und den neuen **modellassoziierten 2D-Schnitt-, Ansichts- und Grundriss-Funktionen nach DIN 1356**.

---

## Projektstruktur

* `Projekt_Einfamilienhaus.ocsproj`:
  Die zentrale Projektdatei für den OpenCADStudio Geschossmanager. Definiert die Geschosshöhen (EG $0{,}00\,\text{m}$, OG $2{,}80\,\text{m}$, DG $5{,}60\,\text{m}$) und verknüpft die Zeichnungsdateien.
* `EG.dwg` / `EG.dxf`:
  Erdgeschoss-Grundriss mit mehrschichtigen Außenwänden (Wärmedämmung, Kalksandstein), tragenden Innenwänden, Fenstern mit Brüstungen und Türen.
* `OG.dwg` / `OG.dxf`:
  Obergeschoss-Grundriss mit Geschossdecke, Schlafräumen, Bad und Fenstern.
* `DG.dwg` / `DG.dxf`:
  Dachgeschoss mit Kniestock-/Drempelwänden und Giebelwänden.
* `Haus_Gesamt.dwg` / `Haus_Gesamt.dxf`:
  Das aggregierte Master-Modell. Es bindet die Einzelgeschosse `EG`, `OG` und `DG` als externe Referenzen (XREFs) mit ihren jeweiligen Höhenversätzen ($Z = 0{,}00\,\text{m}$, $Z = 2{,}80\,\text{m}$, $Z = 5{,}60\,\text{m}$) ein.
  Enthält bereits vordefinierte abgeleitete Zeichnungsträger (`DerivedViewCarrier`) und DIN 1356 Markierungen:
  * **Schnitt A–A:** Mehrgeschossiger Gebäudeschnitt mit Materialschraffuren, Deckenauflagern und assoziativer DIN 1356 Schnittmarkierung.
  * **Südfassade:** Fassadenansicht mit Öffnungsprofilen, DIN-Flügelaufschlägen und Ansichtsmarkierung.
  * **EG Grundriss:** Assoziativer Horizontalschnitt mit lokalisierter Verortung und Türöffnungssymbolen.

---

## Interaktive Griffe & Assoziative Modellbearbeitung

1. **DIN 1356 Schnittmarkierungen im Grundriss:**
   * **Start- und Endgriff (Quadrat):** Verschieben oder Drehen der Schnittlinie. Der assoziierte Schnittträger wird bei Fertigstellung automatisch synchron regeneriert.
   * **Tiefengriff (Rechteck):** Blicktiefe interaktiv per Maus vergrößern oder verkleinern.
   * **Flip-Griff (Dreieck):** Ein Klick kehrt die Blickrichtung der Schnittpfeile und des Schnittträgers normgerecht um.
   * **Jog-Punkte:** Bei abgetreppten Schnittführungen können Zwischensegmente individuell verschoben werden.

2. **Semantische Modellgriffe im Schnitt (`resolve_handle_drag`):**
   * Beim Anklicken abgeleiteter Kanten im Schnitt (z. B. Wandoberkanten, Deckenoberkanten, Fensterbrüstungen oder Stürzen) erscheinen interaktive Modellgriffe.
   * Das Ziehen an einem Wandoberkantengriff hebt oder senkt die 3D-Wandhöhe im BIM-Modell.
   * Beim Loslassen des Griffs wird eine atomare BIM-Transaktion ausgeführt: Die Wandhöhe wird im Modell aktualisiert und alle abhängigen Ansichten werden synchron neu berechnet.

3. **Lokalisierte Grundriss-Engine (`AEC_FLOORPLAN` / `AEC_RCP`):**
   * Grundrisse werden relativ zum Modellursprung projiziert und erscheinen exakt am gewählten Klickpunkt.
   * **Einfügegriff:** Verschieben der gesamten platzierten 2D-Zeichnung im Modellbereich.
   * **Schnitthöhen-Griff:** Interaktive Justierung der horizontalen Schnittebene (`z_cut`) direkt am Plan oder über das Eigenschaften-Panel (`carrier_cut_height`).

4. **Eigenschaften-Panel & Synchronisation:**
   * Alle Parameter (Kennung, Blicktiefe, Z-Bereich, Schnitthöhe, Maßstab 1:50/1:100 und Aktualisierungsmodus) können im Eigenschaften-Panel angepasst werden.
   * Mit dem Befehl `AEC_VIEWUPDATE` können alle manuell verwalteten Ansichten auf Tastendruck auf den neuesten Modellstand gebracht werden.
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_generate_sample_project() {
        // Generate in user documents folder
        let user_folder = PathBuf::from("/home/tom/Dokumente/OpenCADStudio_Dateien/Beispielprojekt");
        let res1 = generate_sample_project(&user_folder);
        assert!(res1.is_ok(), "Failed to generate in user documents: {:?}", res1);

        // Generate also in examples/beispiel_haus
        let repo_folder = PathBuf::from("examples/beispiel_haus");
        let res2 = generate_sample_project(&repo_folder);
        assert!(res2.is_ok(), "Failed to generate in repo examples: {:?}", res2);

        // Verify files exist
        assert!(user_folder.join("Projekt_Einfamilienhaus.ocsproj").exists());
        assert!(user_folder.join("EG.dwg").exists());
        assert!(user_folder.join("EG.dxf").exists());
        assert!(user_folder.join("OG.dwg").exists());
        assert!(user_folder.join("DG.dwg").exists());
        assert!(user_folder.join("Haus_Gesamt.dwg").exists());
        assert!(user_folder.join("README.md").exists());

        // Check primitives in Haus_Gesamt
        let mut scene = crate::scene::Scene::new();
        let doc = crate::io::load_file(&repo_folder.join("Haus_Gesamt.dxf")).unwrap();
        scene.document = doc;
        let carriers = crate::modules::aec::engine::views::carrier::all_view_carriers(&scene.document);
        println!("SAMPLE PROJECT: CARRIERS COUNT = {}", carriers.len());
        for c in &carriers {
            println!("  carrier: {:?}, derived_handles: {}", c.handle, c.derived_handles.len());
            let mut feats = Vec::new();
            for &dh in &c.derived_handles {
                if let Some(ent) = scene.document.get_entity(dh) {
                    if let Some(meta) = crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(ent) {
                        if let Some(feat) = meta.feature {
                            feats.push(feat);
                        }
                    }
                }
            }
            println!("    features: {:?}", feats);
        }
    }
}
