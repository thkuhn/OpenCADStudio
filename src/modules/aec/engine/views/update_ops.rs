//! Dependency- and Dirty-Tracking System for Associative AEC Views.
//!
//! Tracks model modifications, invalidates affected view carriers, and coordinates
//! automatic/manual view regeneration across sections, elevations, and plans.

use acadrust::{CadDocument, Handle};

use super::carrier::{
    all_view_carriers, find_view_carrier_by_handle, find_view_carriers_for_model_view,
    sync_carrier_primitives, write_view_carrier,
};
use super::cut_engine::{derive_section_view, CutEngineOptions};
use super::elevation_engine::{derive_elevation_view, ElevationEngineOptions};
use super::floorplan_engine::{derive_floorplan_view, FloorPlanEngineOptions};
use super::model_view::{ModelView, UpdatePolicy};
use super::provenance::ElementRef;
use super::scene_collector::SceneGeometryCollector;
use super::viewpoint::{ViewKind, Viewpoint};
use super::{ModelId, ModelViewId, ValidationError, ViewProfileId};

/// Marks all carriers in the document as outdated (dirty).
/// Returns the number of carriers updated.
pub fn mark_all_carriers_dirty(doc: &mut CadDocument) -> usize {
    let mut carriers = all_view_carriers(doc);
    let mut count = 0;
    for carrier in &mut carriers {
        if !carrier.is_outdated {
            carrier.is_outdated = true;
            if write_view_carrier(doc, carrier) {
                count += 1;
            }
        }
    }
    count
}

/// Invalidate carriers whose viewpoint or model view depends on the modified elements.
/// Returns the number of carriers marked dirty.
pub fn mark_elements_modified(
    doc: &mut CadDocument,
    modified_elements: &[ElementRef],
    viewpoints: &[Viewpoint],
    _model_views: &[ModelView],
    scene: &SceneGeometryCollector,
) -> usize {
    if modified_elements.is_empty() {
        return 0;
    }

    let mut carriers = all_view_carriers(doc);
    let mut count = 0;

    for carrier in &mut carriers {
        if carrier.is_outdated {
            continue;
        }

        let vp = viewpoints.iter().find(|v| v.id == carrier.viewpoint_id);
        let Some(vp) = vp else {
            // Missing viewpoint -> mark dirty
            carrier.is_outdated = true;
            if write_view_carrier(doc, carrier) {
                count += 1;
            }
            continue;
        };

        let mut affected = false;

        // Check if any modified element intersects the viewpoint's vertical range or volume
        let z_min = vp.z_range.z_min.get();
        let z_max = vp.z_range.z_max.get();

        for mod_elem in modified_elements {
            // Check in walls
            if let Some(wall) = scene.walls.iter().find(|w| w.element_ref == *mod_elem) {
                if wall.top_z >= z_min && wall.base_z <= z_max {
                    affected = true;
                    break;
                }
            }
            // Check in slabs
            if let Some(slab) = scene.slabs.iter().find(|s| s.element_ref == *mod_elem) {
                let (s_min, s_max) = slab.z_range();
                if s_max >= z_min && s_min <= z_max {
                    affected = true;
                    break;
                }
            }
            // Check in openings
            for wall in &scene.walls {
                if wall.openings.iter().any(|op| op.element_ref == *mod_elem) {
                    if wall.top_z >= z_min && wall.base_z <= z_max {
                        affected = true;
                        break;
                    }
                }
            }
            if affected {
                break;
            }
        }

        if affected {
            carrier.is_outdated = true;
            if write_view_carrier(doc, carrier) {
                count += 1;
            }
        }
    }

    count
}

/// Regenerates a specific view carrier by handle.
pub fn regenerate_carrier(
    doc: &mut CadDocument,
    carrier_handle: Handle,
    viewpoints: &[Viewpoint],
    model_views: &[ModelView],
    scene: &SceneGeometryCollector,
    display_config_library: &crate::modules::aec::engine::library::DisplayConfigLibrary,
    style_library: &crate::modules::aec::engine::library::StyleLibrary,
    active_name: Option<&str>,
) -> Result<usize, ValidationError> {
    let mut carrier = find_view_carrier_by_handle(doc, carrier_handle).ok_or_else(|| {
        ValidationError(format!("carrier handle {:?} not found", carrier_handle))
    })?;

    let embedded_defs = crate::modules::aec::engine::views::carrier::read_view_carrier_definitions(doc, carrier_handle);

    let vp_found = viewpoints
        .iter()
        .find(|v| v.id == carrier.viewpoint_id)
        .cloned()
        .or_else(|| embedded_defs.as_ref().map(|(vp, _)| vp.clone()));

    let vp = vp_found.ok_or_else(|| {
        ValidationError(format!("viewpoint {:?} not found for carrier", carrier.viewpoint_id))
    })?;

    let mv_found = model_views
        .iter()
        .find(|m| m.id == carrier.model_view_id)
        .cloned()
        .or_else(|| embedded_defs.as_ref().map(|(_, mv)| mv.clone()));

    let mut mv = mv_found.ok_or_else(|| {
        ValidationError(format!("model view {:?} not found for carrier", carrier.model_view_id))
    })?;

    let target_view_type = match &vp.kind {
        ViewKind::Section { .. } => crate::modules::aec::engine::plan_view::ViewType::Section,
        ViewKind::Elevation { .. } => crate::modules::aec::engine::plan_view::ViewType::Elevation,
        ViewKind::FloorPlan { .. } | ViewKind::ReflectedCeilingPlan { .. } => {
            crate::modules::aec::engine::plan_view::ViewType::FloorPlan
        }
    };
    // A generated view keeps the plan art stored on it. Switching the drawing's
    // active plan must not rebind the view; only a missing stored plan falls
    // back, and that fallback is not written back over the stored id.
    let _ = active_name;
    let stored_id = mv.profile_id.as_uuid();
    let display_config = display_config_library
        .configs
        .iter()
        .find(|cfg| cfg.id == stored_id)
        .cloned()
        .unwrap_or_else(|| {
            super::style_resolver::resolve_view_display_config(
                display_config_library,
                target_view_type,
                None,
                None,
            )
        });
    if display_config_library
        .configs
        .iter()
        .any(|cfg| cfg.id == stored_id)
    {
        mv.profile_id = ViewProfileId::from_uuid(display_config.id);
    }

    let primitives = match &vp.kind {
        ViewKind::Section { .. } => {
            derive_section_view(&vp, &mv, scene, &CutEngineOptions {
                display_config: Some(display_config.clone()),
                style_library: Some(style_library.clone()),
                ..CutEngineOptions::default()
            })?
        }
        ViewKind::Elevation { .. } => {
            derive_elevation_view(&vp, &mv, scene, &ElevationEngineOptions {
                display_config: Some(display_config.clone()),
                style_library: Some(style_library.clone()),
                ..ElevationEngineOptions::default()
            })?
        }
        ViewKind::FloorPlan { .. } | ViewKind::ReflectedCeilingPlan { .. } => {
            derive_floorplan_view(&vp, &mv, scene, &FloorPlanEngineOptions {
                display_config: Some(display_config.clone()),
                style_library: Some(style_library.clone()),
                ..FloorPlanEngineOptions::default()
            })?
        }
    };

    crate::modules::aec::engine::views::carrier::write_view_carrier_definitions(doc, carrier_handle, &vp, &mv);
    sync_carrier_primitives(doc, &mut carrier, &primitives)?;
    super::sheet_annotation::append_overall_dimensions(doc, &mut carrier, &primitives)?;

    Ok(1)
}

/// Regenerates a carrier using document geometry. If regeneration succeeds,
/// the carrier geometry is refreshed and `is_outdated` is false.
/// If regeneration fails, the carrier is marked as outdated (`is_outdated = true`)
/// so the user is never shown a stale view that claims to be current.
pub fn regenerate_or_mark_outdated(
    doc: &mut CadDocument,
    carrier_handle: Handle,
    vp: &Viewpoint,
    mv: &ModelView,
) -> bool {
    let display_config_library = crate::modules::aec::engine::library::DisplayConfigLibrary::seed_default_configs();
    let style_library = crate::modules::aec::engine::library::seed_default_library();
    regenerate_or_mark_outdated_with_libraries(
        doc,
        carrier_handle,
        vp,
        mv,
        &display_config_library,
        &style_library,
        None,
    )
}

pub fn regenerate_or_mark_outdated_with_libraries(
    doc: &mut CadDocument,
    carrier_handle: Handle,
    vp: &Viewpoint,
    mv: &ModelView,
    display_config_library: &crate::modules::aec::engine::library::DisplayConfigLibrary,
    style_library: &crate::modules::aec::engine::library::StyleLibrary,
    active_name: Option<&str>,
) -> bool {
    let collector = SceneGeometryCollector::collect_from_document(doc, ModelId::new());
    match regenerate_carrier(
        doc,
        carrier_handle,
        &[vp.clone()],
        &[mv.clone()],
        &collector,
        display_config_library,
        style_library,
        active_name,
    ) {
        Ok(_) => true,
        Err(_) => {
            if let Some(mut carrier) = find_view_carrier_by_handle(doc, carrier_handle) {
                carrier.is_outdated = true;
                write_view_carrier(doc, &carrier);
            }
            false
        }
    }
}

/// Regenerates all carriers associated with a specific `ModelViewId`.
pub fn update_carrier_by_id(
    doc: &mut CadDocument,
    model_view_id: ModelViewId,
    viewpoints: &[Viewpoint],
    model_views: &[ModelView],
    scene: &SceneGeometryCollector,
    display_config_library: &crate::modules::aec::engine::library::DisplayConfigLibrary,
    style_library: &crate::modules::aec::engine::library::StyleLibrary,
    active_name: Option<&str>,
) -> Result<usize, ValidationError> {
    let carriers = find_view_carriers_for_model_view(doc, model_view_id);
    let mut count = 0;
    for carrier in carriers {
        regenerate_carrier(
            doc,
            carrier.handle,
            viewpoints,
            model_views,
            scene,
            display_config_library,
            style_library,
            active_name,
        )?;
        count += 1;
    }
    Ok(count)
}

/// Scans all carriers with `update_policy == UpdatePolicy::Automatic` that are `is_outdated`
/// and updates them synchronously.
pub fn update_outdated_automatic_carriers(
    doc: &mut CadDocument,
    viewpoints: &[Viewpoint],
    model_views: &[ModelView],
    scene: &SceneGeometryCollector,
) -> Result<usize, ValidationError> {
    let display_config_library = crate::modules::aec::engine::library::DisplayConfigLibrary::seed_default_configs();
    let style_library = crate::modules::aec::engine::library::seed_default_library();
    update_outdated_automatic_carriers_with_libraries(
        doc,
        viewpoints,
        model_views,
        scene,
        &display_config_library,
        &style_library,
        None,
    )
}

pub fn update_outdated_automatic_carriers_with_libraries(
    doc: &mut CadDocument,
    viewpoints: &[Viewpoint],
    model_views: &[ModelView],
    scene: &SceneGeometryCollector,
    display_config_library: &crate::modules::aec::engine::library::DisplayConfigLibrary,
    style_library: &crate::modules::aec::engine::library::StyleLibrary,
    active_name: Option<&str>,
) -> Result<usize, ValidationError> {
    let carriers = all_view_carriers(doc);
    let mut updated_count = 0;

    for carrier in carriers {
        if carrier.update_policy == UpdatePolicy::Automatic && carrier.is_outdated {
            regenerate_carrier(
                doc,
                carrier.handle,
                viewpoints,
                model_views,
                scene,
                display_config_library,
                style_library,
                active_name,
            )?;
            updated_count += 1;
        }
    }

    Ok(updated_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use crate::modules::aec::engine::plan_view::PlanPhase;
    use crate::modules::aec::engine::views::carrier::create_view_carrier;
    use crate::modules::aec::engine::views::model_view::{DetailPolicy, ViewRevisions};
    use crate::modules::aec::engine::views::scene_collector::CollectedWall;
    use crate::modules::aec::engine::views::viewpoint::{SectionPath, VerticalRange, ViewAxes, ViewOrigin};
    use crate::modules::aec::engine::views::{ElementId, Metres, ModelId, ModelViewId, Point2, Point3, Revision, ScaleDenominator, Vector2, ViewpointId, ViewProfileId};
    use crate::modules::aec::engine::wall::WallJustification;

    #[test]
    fn test_mark_dirty_and_update_automatic_carrier() {
        let mut doc = CadDocument::new();
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();

        let vp = Viewpoint {
            id: vp_id,
            name: "Section A-A".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::section(Vector2::new(1.0, 0.0)).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
                far_depth: None,
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap())
                .unwrap(),
            mapping: super::super::ProjectionMapping::default(),
            crop: None,
        };

        let mv = ModelView {
            id: mv_id,
            name: "1:50 Section".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let carrier = create_view_carrier(
            &mut doc,
            vp_id,
            mv_id,
            Point2::new(100.0, 0.0),
            1.0,
            0.0,
            UpdatePolicy::Automatic,
        )
        .expect("carrier created");

        assert!(!carrier.is_outdated);

        let elem = ElementRef::new(ModelId::new(), ElementId::new());
        let layer = crate::modules::aec::engine::views::scene_collector::CollectedWallLayer {
            material: "Concrete".into(),
            thickness: 0.24,
            function: "Structural".into(),
            axis_offset: -0.12,
            bottom_offset: 0.0,
            top_offset: 0.0,
            hatch_override: None,
            layer_id: uuid::Uuid::new_v4(),
            component_id: crate::modules::aec::engine::views::ComponentId::new(),
            is_structural: true,
        };
        let wall = CollectedWall {
            handle: Handle::new(1),
            element_ref: elem,
            xref_source: None,
            style_id: "W1".into(),
            height: 2.8,
            base_z: 0.0,
            top_z: 2.8,
            axis_points: vec![Point2::new(5.0, -2.0), Point2::new(5.0, 2.0)],
            layers: vec![layer],
            total_thickness: 0.24,
            justification: WallJustification::Center,
            openings: vec![],
            phase: PlanPhase::Existing,
        };

        let mut scene = SceneGeometryCollector::default();
        scene.walls.push(wall);

        // Mark modified
        let marked = mark_elements_modified(&mut doc, &[elem], &[vp.clone()], &[mv.clone()], &scene);
        assert_eq!(marked, 1);

        let c_read = find_view_carrier_by_handle(&doc, carrier.handle).unwrap();
        assert!(c_read.is_outdated);

        // Update automatic carriers
        let updated = update_outdated_automatic_carriers(&mut doc, &[vp], &[mv], &scene).unwrap();
        assert_eq!(updated, 1);

        let c_read2 = find_view_carrier_by_handle(&doc, carrier.handle).unwrap();
        assert!(!c_read2.is_outdated);
        assert!(!c_read2.derived_handles.is_empty());
    }

    #[test]
    fn regenerate_keeps_the_stored_plan_when_the_drawing_plan_changes() {
        use crate::modules::aec::engine::library::DisplayConfigLibrary;
        use crate::modules::aec::engine::plan_view::{DisplayConfig, PlanningStage, ViewType};
        use crate::modules::aec::engine::views::carrier::{
            read_view_carrier_definitions, write_view_carrier_definitions,
        };

        let mut doc = CadDocument::new();
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();
        let stored = DisplayConfig::new(
            "Stored Section".into(),
            "Architecture".into(),
            PlanningStage::Execution,
            ViewType::Section,
        );
        let active = DisplayConfig::new(
            "Active Floor".into(),
            "Architecture".into(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        let library = DisplayConfigLibrary {
            configs: vec![active.clone(), stored.clone()],
            ..Default::default()
        };
        let missing = ViewProfileId::new();
        let vp = Viewpoint {
            id: vp_id,
            name: "Section A-A".into(),
            revision: Revision::default(),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::section(Vector2::new(1.0, 0.0)).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
                far_depth: None,
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: super::super::ProjectionMapping::default(),
            crop: None,
        };
        let mut mv = ModelView {
            id: mv_id,
            name: "1:50 Section".into(),
            viewpoint_id: vp_id,
            profile_id: ViewProfileId::from_uuid(stored.id),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };
        let carrier = create_view_carrier(
            &mut doc,
            vp_id,
            mv_id,
            Point2::new(0.0, 0.0),
            1.0,
            0.0,
            UpdatePolicy::Automatic,
        )
        .expect("carrier");
        write_view_carrier_definitions(&mut doc, carrier.handle, &vp, &mv);
        let scene = SceneGeometryCollector::default();
        let styles = crate::modules::aec::engine::library::seed_default_library();
        regenerate_carrier(
            &mut doc,
            carrier.handle,
            &[vp.clone()],
            &[mv.clone()],
            &scene,
            &library,
            &styles,
            Some("Active Floor"),
        )
        .expect("regen");
        let (_, kept) = read_view_carrier_definitions(&doc, carrier.handle).expect("defs");
        assert_eq!(kept.profile_id.as_uuid(), stored.id);

        mv.profile_id = missing;
        write_view_carrier_definitions(&mut doc, carrier.handle, &vp, &mv);
        regenerate_carrier(
            &mut doc,
            carrier.handle,
            &[vp],
            &[mv],
            &scene,
            &library,
            &styles,
            Some(stored.name.as_str()),
        )
        .expect("regen missing");
        let (_, still_missing) = read_view_carrier_definitions(&doc, carrier.handle).expect("defs");
        assert_eq!(still_missing.profile_id, missing);
    }
}
