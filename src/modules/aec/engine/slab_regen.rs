//! Multi-layer 2D and 3D B-Rep geometry regeneration for Slabs and Slab Openings.
//!
//! Handles 2D floor plan outlines, reflected ceiling plan outlines (Deckenspiegel),
//! DIN 1356-1 diagonal opening cross symbols, multi-layer material hatches, and
//! 2-manifold faceted 3D B-Rep solids with subtracted opening cutouts.

#![allow(unused_imports)]
use std::collections::HashMap;
use uuid::Uuid;

use acadrust::entities::{Hatch, Line, LwPolyline, LwVertex, Point, Solid3D};
use acadrust::types::{Color, Vector2, Vector3};
use acadrust::{CadDocument, EntityType, Handle};

use crate::scene::model::hatch_model::{FillPlane, HatchModel, HatchPattern};
use crate::scene::model::hatch_patterns;
use crate::scene::Scene;

use super::control_plane::ControlPlaneFacet;
use super::display_component::{
    ComponentRuleSet, ComponentStyleOverride, LayerSelection, SlabComponentSlot,
};
use super::geometry::{area, signed_area, Polygon2D};
use super::library::{load_or_seed, StyleLibrary};
use super::material::Material;
use super::owner_index;
use super::plan_view::PlanPhase;
use super::slab::{Slab, SlabJustification, SlabLayer};
use super::slab_opening::{SlabOpening, SlabOpeningDepth, SlabOpeningKind};
use super::slab_package::*;
use super::slab_xdata::{
    set_slab_derived_handles, set_slab_opening_derived_handles, slab_from_entity,
    slab_opening_from_entity, write_slab_opening_record, write_slab_record,
};
use super::wall_regen::tessellate_bulge_segment;
use super::wall_style::LayerFunction;

/// Extract 2D boundary coordinates and per-vertex bulges from a slab carrier entity.
pub fn slab_boundary_points_and_bulges(entity: &EntityType) -> (Vec<(f64, f64)>, Vec<f64>) {
    let EntityType::LwPolyline(pl) = entity else {
        return (Vec::new(), Vec::new());
    };
    let mut points: Vec<(f64, f64)> = pl
        .vertices
        .iter()
        .map(|v| (v.location.x, v.location.y))
        .collect();
    let mut bulges: Vec<f64> = pl.vertices.iter().map(|v| v.bulge).collect();

    // Strip duplicate closing vertex if present
    if points.len() >= 3 {
        let first = points[0];
        let last = points[points.len() - 1];
        if (first.0 - last.0).hypot(first.1 - last.1) < 1e-6 {
            points.pop();
            bulges.pop();
        }
    }
    (points, bulges)
}

/// Tessellates a boundary ring with per-vertex bulges into a dense 2D polygon loop.
pub fn tessellate_ring_with_bulges(points: &[(f64, f64)], bulges: &[f64]) -> Vec<(f64, f64)> {
    let n = points.len();
    if n < 3 {
        return points.to_vec();
    }
    let mut out = Vec::new();
    for i in 0..n {
        let start = points[i];
        let end = points[(i + 1) % n];
        let bulge = bulges.get(i).copied().unwrap_or(0.0);
        if bulge.abs() > 1e-6 {
            let segs = tessellate_bulge_segment(start, end, bulge, 12);
            for pt in segs.into_iter().take_while(|p| (p.0 - end.0).hypot(p.1 - end.1) > 1e-6) {
                out.push(pt);
            }
        } else {
            out.push(start);
        }
    }
    out
}

/// Ensures a 2D polygon loop is oriented counter-clockwise (CCW).
pub fn ensure_ccw(points: &mut Vec<(f64, f64)>) {
    if signed_area(points) < 0.0 {
        points.reverse();
    }
}

/// Ensures a 2D polygon loop is oriented clockwise (CW).
pub fn ensure_cw(points: &mut Vec<(f64, f64)>) {
    if signed_area(points) > 0.0 {
        points.reverse();
    }
}

/// Finds the vertex index in `points` closest to `(x, y)` within tolerance.
fn find_vertex_index(points: &[[f64; 2]], x: f64, y: f64) -> Option<usize> {
    points
        .iter()
        .position(|pt| (pt[0] - x).hypot(pt[1] - y) < 1e-5)
}

/// Builds a 2-manifold faceted 3D B-Rep solid for a slab layer with optional opening cutouts.
///
/// Uses `cadkernel::geom2d::triangulate_rings` to triangulate the 2D polygon with holes,
/// extrudes vertices with sloped or horizontal top and bottom Z elevations, and constructs
/// top faces, bottom faces, outer side quads, and inner opening side quads.
pub fn build_faceted_slab_solid(
    outer_boundary: &[(f64, f64)],
    hole_boundaries: &[Vec<(f64, f64)>],
    z_eval: impl Fn(f64, f64) -> (f64, f64), // returns (z_base, z_top) at (x, y)
) -> Option<cadkernel::brep::Body> {
    if outer_boundary.len() < 3 {
        return None;
    }

    let mut outer_ccw = outer_boundary.to_vec();
    ensure_ccw(&mut outer_ccw);

    let mut rings: Vec<Vec<[f64; 2]>> = Vec::new();

    // Ring 0: Outer boundary (closed)
    let mut r0: Vec<[f64; 2]> = outer_ccw.iter().map(|&(x, y)| [x, y]).collect();
    if let Some(&first) = r0.first() {
        r0.push(first);
    }
    rings.push(r0);

    // Filter valid hole boundaries and add as closed rings
    let mut valid_holes: Vec<Vec<(f64, f64)>> = Vec::new();
    for hole in hole_boundaries {
        if hole.len() < 3 {
            continue;
        }
        let mut h_cw = hole.clone();
        ensure_cw(&mut h_cw);
        let mut rh: Vec<[f64; 2]> = h_cw.iter().map(|&(x, y)| [x, y]).collect();
        if let Some(&first) = rh.first() {
            rh.push(first);
        }
        rings.push(rh);
        valid_holes.push(h_cw);
    }

    let (points, triangles) = cadkernel::geom2d::triangulate_rings(&rings);
    if points.is_empty() || triangles.is_empty() {
        return None;
    }

    let m = points.len();
    let mut vertices: Vec<[f64; 3]> = Vec::with_capacity(2 * m);

    // 0..m: Base vertices (z_base)
    for pt in &points {
        let (zb, _) = z_eval(pt[0], pt[1]);
        vertices.push([pt[0], pt[1], zb]);
    }

    // m..2*m: Top vertices (z_top)
    for pt in &points {
        let (_, zt) = z_eval(pt[0], pt[1]);
        vertices.push([pt[0], pt[1], zt]);
    }

    let mut faces: Vec<Vec<usize>> = Vec::new();

    // Track directed edges in 2D triangulation: map from (u, v) -> count
    let mut edge_counts: HashMap<(usize, usize), usize> = HashMap::new();

    // Top and Bottom triangle faces from 2D triangulation
    for tri in &triangles {
        let (a, b, c) = (tri[0], tri[1], tri[2]);
        let pa = points[a];
        let pb = points[b];
        let pc = points[c];
        let signed_2d = (pb[0] - pa[0]) * (pc[1] - pa[1]) - (pb[1] - pa[1]) * (pc[0] - pa[0]);

        let (t0, t1, t2) = if signed_2d >= 0.0 {
            (a, b, c)
        } else {
            (a, c, b)
        };

        // Top face: CCW looking down (+Z normal)
        faces.push(vec![m + t0, m + t1, m + t2]);
        // Bottom face: CCW looking up (-Z normal)
        faces.push(vec![t2, t1, t0]);

        // Record directed edges (t0->t1, t1->t2, t2->t0)
        *edge_counts.entry((t0, t1)).or_insert(0) += 1;
        *edge_counts.entry((t1, t2)).or_insert(0) += 1;
        *edge_counts.entry((t2, t0)).or_insert(0) += 1;
    }

    // Every boundary edge in the 2D triangulation has (u, v) with no opposite (v, u).
    // Each boundary edge produces an outward/inward side quad [u, v, m + v, m + u].
    for (&(u, v), &count) in &edge_counts {
        if count == 1 && !edge_counts.contains_key(&(v, u)) {
            faces.push(vec![u, v, m + v, m + u]);
        }
    }

    cadkernel::brep::make::faceted_solid(&vertices, &faces)
}

/// Regenerates the 2D outlines, 2D hatches, and 3D solid representations for a single `Slab`.
pub fn regenerate_slab_representation(
    scene: &mut Scene,
    slab_handle: Handle,
    library_override: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> bool {
    let Some(entity) = scene.document.get_entity(slab_handle) else {
        return false;
    };
    let Some(mut slab) = slab_from_entity(entity) else {
        return false;
    };

    let (boundary_pts, boundary_bulges) = slab_boundary_points_and_bulges(entity);
    if boundary_pts.len() < 3 {
        return false;
    }

    let tessellated_boundary = tessellate_ring_with_bulges(&boundary_pts, &boundary_bulges);
    if tessellated_boundary.len() < 3 || area(&tessellated_boundary) < 1e-6 {
        return false;
    }

    // Default reference Z from storey or origin
    let ref_z = slab.base_origin[2];

    // Collect opening models and geometries
    let mut opening_data: Vec<(Handle, SlabOpening, Vec<(f64, f64)>)> = Vec::new();
    for op_handle in &slab.opening_handles {
        if let Some(op_entity) = scene.document.get_entity(*op_handle) {
            if let Some(op) = slab_opening_from_entity(op_entity) {
                let op_boundary = if !op.boundary.is_empty() {
                    op.boundary.clone()
                } else {
                    let (pts, bulges) = slab_boundary_points_and_bulges(op_entity);
                    tessellate_ring_with_bulges(&pts, &bulges)
                };
                if op_boundary.len() >= 3 {
                    opening_data.push((*op_handle, op, op_boundary));
                }
            }
        }
    }

    // Check style library for material hatch overrides
    let fallback_lib = load_or_seed();
    let lib = library_override.unwrap_or(&fallback_lib);

    // Component visibility flags from DisplayConfig rules
    let contour_visible = rules.map_or(true, |r| r.is_slab_visible(SlabComponentSlot::Contour2D));
    let ceiling_visible = rules.map_or(true, |r| {
        r.is_slab_visible(SlabComponentSlot::CeilingOutline2D)
    });
    let hatch_visible = rules.map_or(true, |r| {
        r.is_slab_visible(SlabComponentSlot::LayerHatch2D)
    });
    let solid_visible = rules.map_or(true, |r| r.is_slab_visible(SlabComponentSlot::Solid3D));

    // Style overrides
    let contour_style = rules.and_then(|r| r.slab_style_for(SlabComponentSlot::Contour2D));
    let ceiling_style = rules.and_then(|r| r.slab_style_for(SlabComponentSlot::CeilingOutline2D));

    // Gather old derived handles for reuse or cleanup
    let old_derived = collect_slab_display_children(scene, slab_handle);
    let mut reusable_contours: Vec<Handle> = Vec::new();
    let mut to_erase: Vec<Handle> = Vec::new();

    for h in old_derived {
        if let Some(e) = scene.document.get_entity(h) {
            if matches!(e, EntityType::LwPolyline(_)) {
                reusable_contours.push(h);
            } else {
                to_erase.push(h);
            }
        }
    }

    // Erase obsolete non-reusable entities (hatches, solids)
    for h in &to_erase {
        scene.solid_models.remove(h);
        scene.meshes.remove(h);
        scene.block_meshes.remove(h);
        scene.hatches.remove(h);
        scene.document.remove_entity(*h);
        owner_index::remove_child(&mut scene.document, slab_handle, *h);
    }

    let mut new_derived: Vec<Handle> = Vec::new();

    // ── 1. 2D Floor Plan Outer Boundary Contour ──────────────────────────────
    if contour_visible {
        let mut pl = LwPolyline::new();
        for (idx, &(x, y)) in boundary_pts.iter().enumerate() {
            let bulge = boundary_bulges.get(idx).copied().unwrap_or(0.0);
            pl.add_vertex(LwVertex::with_bulge(Vector2::new(x, y), bulge));
        }
        pl.is_closed = true;
        pl.elevation = slab.top_z_at_xy(boundary_pts[0].0, boundary_pts[0].1, ref_z);

        let contour_handle =
            reuse_or_add_slab_contour(scene, &mut reusable_contours, pl, Some(slab_handle));

        let layer_name = contour_style
            .and_then(|s| s.cad_layer.as_deref())
            .unwrap_or(AEC_SLAB_CONTOUR_LAYER);
        scene.ensure_layer(layer_name);

        if let Some(e) = scene.document.get_entity_mut(contour_handle) {
            e.as_entity_mut().set_layer(layer_name.to_string());
            if let Some(color) = contour_style.and_then(|s| s.line_color) {
                e.as_entity_mut().set_color(color);
            }
            if let Some(lt) = contour_style.and_then(|s| s.line_type.as_ref()) {
                e.common_mut().linetype = lt.clone();
            }
        }

        write_slab_display_tag(scene, contour_handle, slab_handle, SLAB_REP_ROLE_CONTOUR);
        owner_index::add_child(&mut scene.document, slab_handle, contour_handle);
        new_derived.push(contour_handle);
    }

    // ── 2. 2D Reflected Ceiling Plan (Deckenspiegel Outline) ─────────────────
    if ceiling_visible {
        let mut pl = LwPolyline::new();
        for (idx, &(x, y)) in boundary_pts.iter().enumerate() {
            let bulge = boundary_bulges.get(idx).copied().unwrap_or(0.0);
            pl.add_vertex(LwVertex::with_bulge(Vector2::new(x, y), bulge));
        }
        pl.is_closed = true;
        pl.elevation = slab.bottom_z_at_xy(boundary_pts[0].0, boundary_pts[0].1, ref_z);

        let ceiling_handle =
            reuse_or_add_slab_contour(scene, &mut reusable_contours, pl, Some(slab_handle));

        let layer_name = ceiling_style
            .and_then(|s| s.cad_layer.as_deref())
            .unwrap_or(AEC_SLAB_CEILING_LAYER);
        scene.ensure_layer(layer_name);

        if let Some(e) = scene.document.get_entity_mut(ceiling_handle) {
            e.as_entity_mut().set_layer(layer_name.to_string());
            if let Some(color) = ceiling_style.and_then(|s| s.line_color) {
                e.as_entity_mut().set_color(color);
            }
            let linetype = ceiling_style
                .and_then(|s| s.line_type.clone())
                .unwrap_or_else(|| "DASHED".to_string());
            e.common_mut().linetype = linetype;
        }

        write_slab_display_tag(scene, ceiling_handle, slab_handle, SLAB_REP_ROLE_CEILING);
        owner_index::add_child(&mut scene.document, slab_handle, ceiling_handle);
        new_derived.push(ceiling_handle);
    }

    // Clean up any remaining unused reusable contours
    for h in reusable_contours {
        scene.erase_entities(&[h]);
        owner_index::remove_child(&mut scene.document, slab_handle, h);
    }

    // ── 3. 2D Multi-layer Sectional / Plan Hatches ───────────────────────────
    let hatch_allowed = if hatch_visible && !slab.layers.is_empty() {
        match rules.and_then(|r| r.layer_filter.get(SlabComponentSlot::LayerHatch2D.key())) {
            Some(LayerSelection::Explicit(refs)) => refs.iter().any(|r| {
                slab.layers.iter().any(|l| {
                    (r.layer_id.is_some() && r.layer_id == Some(l.layer_id))
                        || (!r.material_id.is_empty() && r.material_id == l.material)
                })
            }),
            _ => true,
        }
    } else {
        false
    };

    if hatch_allowed {
        // Collect hole boundaries for hatch outer perimeter
        let hole_polys: Vec<Vec<(f64, f64)>> = opening_data
            .iter()
            .map(|(_, _, b)| b.clone())
            .collect();

        let origin = [tessellated_boundary[0].0, tessellated_boundary[0].1];
        let mut rel: Vec<[f32; 2]> = Vec::new();
        let mut wcs: Vec<[f64; 2]> = Vec::new();

        // Outer boundary
        for &(x, y) in &tessellated_boundary {
            rel.push([(x - origin[0]) as f32, (y - origin[1]) as f32]);
            wcs.push([x, y]);
        }
        if rel.len() >= 3 {
            let first_rel = rel[0];
            let last_rel = *rel.last().unwrap();
            if (first_rel[0] - last_rel[0]).abs() > 1e-6 || (first_rel[1] - last_rel[1]).abs() > 1e-6 {
                rel.push(first_rel);
                wcs.push(wcs[0]);
            }
        }

        // Holes
        for hole in &hole_polys {
            if hole.len() >= 3 {
                rel.push([f32::NAN, f32::NAN]);
                wcs.push([f64::NAN, f64::NAN]);
                let start_idx = rel.len();
                for &(hx, hy) in hole {
                    rel.push([(hx - origin[0]) as f32, (hy - origin[1]) as f32]);
                    wcs.push([hx, hy]);
                }
                let first_h_rel = rel[start_idx];
                let last_h_rel = *rel.last().unwrap();
                if (first_h_rel[0] - last_h_rel[0]).abs() > 1e-6 || (first_h_rel[1] - last_h_rel[1]).abs() > 1e-6 {
                    rel.push(first_h_rel);
                    wcs.push(wcs[start_idx]);
                }
            }
        }

        // Determine hatch pattern from structural layer or first layer material
        let primary_mat_id = slab
            .structural_layer_index()
            .and_then(|i| slab.layers.get(i))
            .or_else(|| slab.layers.first())
            .map(|l| l.material.as_str())
            .unwrap_or("Reinforced Concrete");

        let mat = lib
            .materials
            .iter()
            .find(|m| m.id == primary_mat_id || m.name == primary_mat_id);
        let pattern_name = mat
            .map(|m| m.hatch_pattern.clone())
            .unwrap_or_else(|| "AR-CONC".to_string());
        let hatch_scale = mat.map(|m| m.hatch_scale).unwrap_or(1.0);
        let hatch_angle: f64 = mat.map(|m| m.hatch_angle).unwrap_or(0.0);

        let hatch_acad_color = mat.and_then(|m| m.hatch_color).or_else(|| {
            mat.map(|m| Color::Rgb {
                r: ((m.line_color >> 16) & 0xFF) as u8,
                g: ((m.line_color >> 8) & 0xFF) as u8,
                b: (m.line_color & 0xFF) as u8,
            })
        });
        let color = hatch_acad_color
            .and_then(|c| c.rgb())
            .map(|(r, g, b)| [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 0.85])
            .unwrap_or([0.6, 0.6, 0.6, 0.85]);

        let families = hatch_patterns::find(&pattern_name)
            .and_then(|e| {
                if let HatchPattern::Pattern(f) = &e.gpu {
                    Some(f.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        let phased = crate::modules::aec::project::hatch_origin::phase_families_wcs0(
            &families, origin,
        );

        let hatch_model = HatchModel {
            pattern_origin: None,
            render_instance: None,
            boundary: std::sync::Arc::new(rel.clone()),
            pattern: HatchPattern::Pattern(phased),
            name: pattern_name.clone(),
            color,
            aci: 0,
            line_weight_px: 1.0,
            angle_offset: (hatch_angle.to_radians()) as f32,
            scale: hatch_scale as f32,
            world_origin: origin,
            boundary_wcs: Some(std::sync::Arc::new(wcs)),
            fill_plane: Some(FillPlane {
                origin: [origin[0], origin[1], ref_z],
                x_axis: [1.0, 0.0, 0.0],
                y_axis: [0.0, 1.0, 0.0],
            }),
            fill_plane_boundary: Some(std::sync::Arc::new(rel)),
            boundary_exterior: None,
            boundary_sources: None,
            boundary_paths: None,
            style: acadrust::entities::HatchStyleType::Normal,
            draw_depth: 0.0,
        };

        let hatch_style = hatch_acad_color.map(|c| {
            (c, acadrust::types::Transparency::from_percent(0.0))
        });

        let hatch_handle = scene.add_hatch(hatch_model, None, hatch_style);
        scene.ensure_layer(AEC_SLAB_HATCH_LAYER);

        if let Some(e) = scene.document.get_entity_mut(hatch_handle) {
            e.as_entity_mut().set_layer(AEC_SLAB_HATCH_LAYER.to_string());
            e.common_mut().owner_handle = slab_handle;
        }

        write_slab_display_tag(scene, hatch_handle, slab_handle, SLAB_REP_ROLE_HATCH);
        owner_index::add_child(&mut scene.document, slab_handle, hatch_handle);
        new_derived.push(hatch_handle);
    }

    // ── 4. 3D Multi-Layer Faceted B-Rep Solids ───────────────────────────────
    if solid_visible && slab.total_thickness() > 1e-6 {
        let mut current_offset_from_top = 0.0;

        for (_i, layer) in slab.layers.iter().enumerate() {
            if layer.thickness <= 1e-6 {
                continue;
            }

            let layer_top_offset = current_offset_from_top;
            let layer_bot_offset = current_offset_from_top + layer.thickness;
            current_offset_from_top = layer_bot_offset;

            let layer_included_in_solid = match rules
                .and_then(|r| r.layer_filter.get(SlabComponentSlot::Solid3D.key()))
            {
                Some(LayerSelection::Explicit(refs)) => refs.iter().any(|r| {
                    (r.layer_id.is_some() && r.layer_id == Some(layer.layer_id))
                        || (!r.material_id.is_empty() && r.material_id == layer.material)
                }),
                _ => true,
            };
            if !layer_included_in_solid {
                continue;
            }

            // Collect all split offsets within [layer_top_offset, layer_bot_offset]
            let mut split_offsets = vec![layer_top_offset, layer_bot_offset];
            for (_, op, _) in &opening_data {
                if let SlabOpeningDepth::Recess(recess_depth) = op.depth {
                    if recess_depth > layer_top_offset + 1e-6
                        && recess_depth < layer_bot_offset - 1e-6
                    {
                        split_offsets.push(recess_depth);
                    }
                }
            }

            split_offsets.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            split_offsets.dedup_by(|a, b| (*a - *b).abs() < 1e-6);

            for w in split_offsets.windows(2) {
                let sub_top_offset = w[0];
                let sub_bot_offset = w[1];
                if sub_bot_offset - sub_top_offset <= 1e-6 {
                    continue;
                }

                // Determine which opening cutouts affect this sub-layer
                let mut layer_holes: Vec<Vec<(f64, f64)>> = Vec::new();
                for (_, op, op_boundary) in &opening_data {
                    match op.depth {
                        SlabOpeningDepth::ThroughHole => {
                            layer_holes.push(op_boundary.clone());
                        }
                        SlabOpeningDepth::Recess(recess_depth) => {
                            // Recess cuts from top of slab (0.0) down to recess_depth.
                            // If recess covers this sub-layer segment, subtract cutout.
                            if recess_depth >= sub_bot_offset - 1e-6 {
                                layer_holes.push(op_boundary.clone());
                            }
                        }
                    }
                }

                // Build faceted B-Rep solid for this sub-layer
                let z_eval = |x: f64, y: f64| -> (f64, f64) {
                    let slab_top_z = slab.top_z_at_xy(x, y, ref_z);
                    let zt = slab_top_z - sub_top_offset + layer.vertical_offset;
                    let zb = slab_top_z - sub_bot_offset + layer.vertical_offset;
                    (zb, zt)
                };

                if let Some(body) =
                    build_faceted_slab_solid(&tessellated_boundary, &layer_holes, z_eval)
                {
                    let s3d = Solid3D::new();
                    let mut s3d_entity = EntityType::Solid3D(s3d);
                    s3d_entity.common_mut().owner_handle = slab_handle;

                    let layer_name = layer
                        .layer_override
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .unwrap_or(AEC_SLAB_SOLID_LAYER);
                    scene.ensure_layer(layer_name);
                    s3d_entity.as_entity_mut().set_layer(layer_name.to_string());

                    let solid_handle = scene.add_entity(s3d_entity);
                    scene.register_solid_model(solid_handle, body);

                    write_slab_display_tag(scene, solid_handle, slab_handle, SLAB_REP_ROLE_SOLID);
                    owner_index::add_child(&mut scene.document, slab_handle, solid_handle);
                    new_derived.push(solid_handle);
                }
            }
        }
    }

    // ── 5. Regenerate 2D representations for child openings ──────────────────
    for (op_handle, _, _) in &opening_data {
        regenerate_slab_opening_representation_inner(scene, *op_handle, rules);
    }

    // Update derived handles snapshot on the slab entity
    slab.derived_handles = new_derived;
    write_slab_record(&mut scene.document, slab_handle, &slab);

    true
}

/// Regenerates the 2D representations (cutout outline and DIN 1356 diagonal cross symbol)
/// for a single `SlabOpening`.
pub fn regenerate_slab_opening_representation(
    scene: &mut Scene,
    opening_handle: Handle,
    _library_override: Option<&StyleLibrary>,
    rules: Option<&ComponentRuleSet>,
) -> bool {
    let success = regenerate_slab_opening_representation_inner(scene, opening_handle, rules);

    // Also trigger regeneration of host slab if found
    if let Some(entity) = scene.document.get_entity(opening_handle) {
        if let Some(op) = slab_opening_from_entity(entity) {
            if scene.document.get_entity(op.host_slab).is_some() {
                regenerate_slab_representation(scene, op.host_slab, _library_override, rules);
            }
        }
    }

    success
}

/// Internal helper to regenerate 2D opening representation entities without triggering
/// host slab recursion.
fn regenerate_slab_opening_representation_inner(
    scene: &mut Scene,
    opening_handle: Handle,
    rules: Option<&ComponentRuleSet>,
) -> bool {
    let Some(entity) = scene.document.get_entity(opening_handle) else {
        return false;
    };
    let Some(mut opening) = slab_opening_from_entity(entity) else {
        return false;
    };

    let boundary_pts = if !opening.boundary.is_empty() {
        opening.boundary.clone()
    } else {
        let (pts, bulges) = slab_boundary_points_and_bulges(entity);
        tessellate_ring_with_bulges(&pts, &bulges)
    };
    if boundary_pts.len() < 3 {
        return false;
    }

    let contour_visible = rules.map_or(true, |r| {
        r.is_slab_visible(SlabComponentSlot::OpeningContour2D)
    });
    let symbol_visible = rules.map_or(true, |r| {
        r.is_slab_visible(SlabComponentSlot::OpeningSymbol2D)
    });

    let old_derived = collect_slab_opening_display_children(scene, opening_handle);
    let mut reusable_contours: Vec<Handle> = Vec::new();
    let mut to_erase: Vec<Handle> = Vec::new();

    for h in old_derived {
        if let Some(e) = scene.document.get_entity(h) {
            if matches!(e, EntityType::LwPolyline(_)) {
                reusable_contours.push(h);
            } else {
                to_erase.push(h);
            }
        }
    }

    for h in &to_erase {
        scene.document.remove_entity(*h);
        owner_index::remove_child(&mut scene.document, opening_handle, *h);
    }

    let mut new_derived: Vec<Handle> = Vec::new();

    // 1. Opening cutout outline (closed LwPolyline)
    if contour_visible {
        let mut pl = LwPolyline::new();
        for &(x, y) in &boundary_pts {
            pl.add_vertex(LwVertex::new(Vector2::new(x, y)));
        }
        pl.is_closed = true;

        let contour_h =
            reuse_or_add_slab_contour(scene, &mut reusable_contours, pl, Some(opening_handle));
        scene.ensure_layer(AEC_SLAB_OPENING_LAYER);

        if let Some(e) = scene.document.get_entity_mut(contour_h) {
            e.as_entity_mut().set_layer(AEC_SLAB_OPENING_LAYER.to_string());
        }

        write_slab_opening_display_tag(
            scene,
            contour_h,
            opening_handle,
            SLAB_REP_ROLE_OPENING_CONTOUR,
        );
        owner_index::add_child(&mut scene.document, opening_handle, contour_h);
        new_derived.push(contour_h);
    }

    // 2. DIN 1356 diagonal cross symbol ('X') lines
    if symbol_visible {
        let cross_lines = opening.din_1356_cross_lines();
        for ((x1, y1), (x2, y2)) in cross_lines {
            let mut pl = LwPolyline::new();
            pl.add_vertex(LwVertex::new(Vector2::new(x1, y1)));
            pl.add_vertex(LwVertex::new(Vector2::new(x2, y2)));
            pl.is_closed = false;

            let sym_h =
                reuse_or_add_slab_contour(scene, &mut reusable_contours, pl, Some(opening_handle));
            scene.ensure_layer(AEC_SLAB_OPENING_LAYER);

            if let Some(e) = scene.document.get_entity_mut(sym_h) {
                e.as_entity_mut().set_layer(AEC_SLAB_OPENING_LAYER.to_string());
            }

            write_slab_opening_display_tag(
                scene,
                sym_h,
                opening_handle,
                SLAB_REP_ROLE_OPENING_SYMBOL,
            );
            owner_index::add_child(&mut scene.document, opening_handle, sym_h);
            new_derived.push(sym_h);
        }
    }

    // Clean up leftover reusable contours
    for h in reusable_contours {
        scene.erase_entities(&[h]);
        owner_index::remove_child(&mut scene.document, opening_handle, h);
    }

    opening.derived_handles = new_derived;
    write_slab_opening_record(&mut scene.document, opening_handle, &opening);

    true
}

/// Regenerates all slabs in `scene.document`.
pub fn regenerate_all_slabs(scene: &mut Scene, rules: Option<&ComponentRuleSet>) {
    let slab_handles: Vec<Handle> = scene
        .document
        .entities()
        .filter_map(|e| {
            if slab_from_entity(e).is_some() {
                Some(e.common().handle)
            } else {
                None
            }
        })
        .collect();

    for h in slab_handles {
        regenerate_slab_representation(scene, h, None, rules);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::LwVertex;
    use acadrust::types::Vector2;

    #[test]
    fn test_build_faceted_slab_solid_simple() {
        let outer = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 8.0), (0.0, 8.0)];
        let holes = vec![];

        let body = build_faceted_slab_solid(&outer, &holes, |_x, _y| (0.0, 0.20));
        assert!(body.is_some(), "Faceted solid creation should succeed for rectangular slab");
        let body = body.unwrap();
        assert_eq!(body.faces.len(), 8, "A triangulated box slab has 8 faces (2 top, 2 bot, 4 sides)");
    }

    #[test]
    fn test_build_faceted_slab_solid_with_opening() {
        let outer = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let hole = vec![(2.0, 2.0), (6.0, 2.0), (6.0, 6.0), (2.0, 6.0)];

        let mut outer_ccw = outer.clone();
        ensure_ccw(&mut outer_ccw);
        let mut r0: Vec<[f64; 2]> = outer_ccw.iter().map(|&(x, y)| [x, y]).collect();
        r0.push(r0[0]);

        let mut h_cw = hole.clone();
        ensure_cw(&mut h_cw);
        let mut rh: Vec<[f64; 2]> = h_cw.iter().map(|&(x, y)| [x, y]).collect();
        rh.push(rh[0]);

        let (points, triangles) = cadkernel::geom2d::triangulate_rings(&[r0, rh]);
        println!("points: {:?}", points);
        println!("triangles: {:?}", triangles);

        let body = build_faceted_slab_solid(&outer, &[hole], |_x, _y| (0.0, 0.25));
        assert!(body.is_some(), "Faceted solid with opening should succeed");
    }

    #[test]
    fn test_slab_2d_and_3d_regeneration() {
        let mut scene = Scene::new();

        // Create carrier polyline
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(6.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(6.0, 4.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 4.0)));
        pl.is_closed = true;

        let carrier_handle = scene.add_entity(EntityType::LwPolyline(pl));

        let mut slab = Slab::new("style_slab_concrete_20", 1);
        slab.layers = vec![
            SlabLayer::new("Floor Screed", 0.05, LayerFunction::Finish),
            SlabLayer::new("Reinforced Concrete", 0.20, LayerFunction::Structural),
        ];
        slab.base_origin = [0.0, 0.0, 2.80];

        write_slab_record(&mut scene.document, carrier_handle, &slab);

        // Regenerate slab
        let ok = regenerate_slab_representation(&mut scene, carrier_handle, None, None);
        assert!(ok, "Slab regeneration should succeed");

        // Verify derived handles were generated
        let updated_slab = slab_from_entity(scene.document.get_entity(carrier_handle).unwrap()).unwrap();
        assert!(!updated_slab.derived_handles.is_empty(), "Derived handles should be populated");

        // Verify solid was cached in scene
        assert!(!scene.solid_models.is_empty(), "Scene should contain registered solid models");
        assert!(!scene.meshes.is_empty(), "Scene should contain cached meshes for solid models");
    }

    #[test]
    fn test_slab_with_associative_opening_regeneration() {
        let mut scene = Scene::new();

        // 1. Create slab carrier
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(10.0, 8.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 8.0)));
        pl.is_closed = true;
        let slab_h = scene.add_entity(EntityType::LwPolyline(pl));

        let mut slab = Slab::new("style_slab_concrete_25", 1);
        slab.layers = vec![
            SlabLayer::new("Reinforced Concrete", 0.25, LayerFunction::Structural),
        ];
        slab.base_origin = [0.0, 0.0, 0.0];

        // 2. Create opening carrier entity
        let mut op_pl = LwPolyline::new();
        op_pl.add_vertex(LwVertex::new(Vector2::new(2.0, 2.0)));
        op_pl.add_vertex(LwVertex::new(Vector2::new(5.0, 2.0)));
        op_pl.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
        op_pl.add_vertex(LwVertex::new(Vector2::new(2.0, 5.0)));
        op_pl.is_closed = true;
        let op_h = scene.add_entity(EntityType::LwPolyline(op_pl));

        let opening = SlabOpening::new_through_hole(
            slab_h,
            SlabOpeningKind::Stairwell,
            vec![(2.0, 2.0), (5.0, 2.0), (5.0, 5.0), (2.0, 5.0)],
        );
        write_slab_opening_record(&mut scene.document, op_h, &opening);

        // Associate opening with slab
        slab.opening_handles.push(op_h);
        write_slab_record(&mut scene.document, slab_h, &slab);

        // Regenerate opening and slab
        let op_ok = regenerate_slab_opening_representation(&mut scene, op_h, None, None);
        assert!(op_ok, "Opening regeneration should succeed");

        let updated_op = slab_opening_from_entity(scene.document.get_entity(op_h).unwrap()).unwrap();
        assert!(!updated_op.derived_handles.is_empty(), "Opening should have derived handles for symbol and boundary");

        let slab_ok = regenerate_slab_representation(&mut scene, slab_h, None, None);
        assert!(slab_ok, "Slab regeneration with hole should succeed");
        assert!(!scene.solid_models.is_empty(), "Solid model should be generated");
    }

    #[test]
    fn test_multi_layer_5_layer_slab_regeneration() {
        let mut scene = Scene::new();

        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(8.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(8.0, 6.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 6.0)));
        pl.is_closed = true;
        let slab_h = scene.add_entity(EntityType::LwPolyline(pl));

        let mut slab = Slab::new("style_slab_complex_5layer", 1);
        slab.layers = vec![
            SlabLayer::new("Tiles", 0.02, LayerFunction::Finish),
            SlabLayer::new("Floor Screed", 0.05, LayerFunction::Finish),
            SlabLayer::new("Thermal Insulation", 0.04, LayerFunction::Insulation),
            SlabLayer::new("Reinforced Concrete", 0.20, LayerFunction::Structural),
            SlabLayer::new("Internal Plaster", 0.015, LayerFunction::Finish),
        ];
        slab.base_origin = [0.0, 0.0, 3.0];
        slab.justification = SlabJustification::Top;

        write_slab_record(&mut scene.document, slab_h, &slab);

        let ok = regenerate_slab_representation(&mut scene, slab_h, None, None);
        assert!(ok, "5-layer slab regeneration should succeed");

        // Verify that 5 3D solids were generated (one per non-zero layer)
        assert_eq!(scene.solid_models.len(), 5, "5 separate solids should be generated for 5 layers");
        assert_eq!(scene.meshes.len(), 5, "5 meshes should be cached in Scene");
    }

    #[test]
    fn test_recess_opening_depth_cutout() {
        let mut scene = Scene::new();

        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(8.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(8.0, 8.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 8.0)));
        pl.is_closed = true;
        let slab_h = scene.add_entity(EntityType::LwPolyline(pl));

        let mut slab = Slab::new("style_slab_screed_concrete", 1);
        slab.layers = vec![
            SlabLayer::new("Floor Screed", 0.05, LayerFunction::Finish),
            SlabLayer::new("Reinforced Concrete", 0.20, LayerFunction::Structural),
        ];
        slab.base_origin = [0.0, 0.0, 0.0];

        // 3cm deep recess in top screed layer (depth = 0.03 < 0.05)
        let mut op_pl = LwPolyline::new();
        op_pl.add_vertex(LwVertex::new(Vector2::new(2.0, 2.0)));
        op_pl.add_vertex(LwVertex::new(Vector2::new(4.0, 2.0)));
        op_pl.add_vertex(LwVertex::new(Vector2::new(4.0, 4.0)));
        op_pl.add_vertex(LwVertex::new(Vector2::new(2.0, 4.0)));
        op_pl.is_closed = true;
        let op_h = scene.add_entity(EntityType::LwPolyline(op_pl));

        let opening = SlabOpening::new_recess(
            slab_h,
            SlabOpeningKind::Custom,
            0.03,
            vec![(2.0, 2.0), (4.0, 2.0), (4.0, 4.0), (2.0, 4.0)],
        );
        write_slab_opening_record(&mut scene.document, op_h, &opening);

        slab.opening_handles.push(op_h);
        write_slab_record(&mut scene.document, slab_h, &slab);

        let ok = regenerate_slab_representation(&mut scene, slab_h, None, None);
        assert!(ok, "Slab regeneration with recess should succeed");

        let updated_slab = slab_from_entity(scene.document.get_entity(slab_h).unwrap()).unwrap();
        let solid_handles: Vec<Handle> = updated_slab
            .derived_handles
            .into_iter()
            .filter(|h| scene.solid_models.contains_key(h))
            .collect();

        assert_eq!(solid_handles.len(), 3, "3 solid bodies: screed upper (cut), screed lower remainder (uncut), concrete (uncut)");

        let screed_upper_solid = scene.solid_models.get(&solid_handles[0]).unwrap();
        let screed_lower_solid = scene.solid_models.get(&solid_handles[1]).unwrap();
        let concrete_solid = scene.solid_models.get(&solid_handles[2]).unwrap();

        // Screed upper portion (0..3cm) has hole (cutout) -> more than 8 faces
        assert!(screed_upper_solid.faces.len() > 8, "Screed upper portion with recess cutout has hole side quads");
        // Screed lower portion (3..5cm) remains solid under 3cm recess -> simple 8 faces box
        assert_eq!(screed_lower_solid.faces.len(), 8, "Screed lower portion below recess depth remains uncut (8 faces)");
        // Concrete layer has NO hole (depth 3cm < screed 5cm) -> simple 8 faces box
        assert_eq!(concrete_solid.faces.len(), 8, "Concrete layer below recess depth remains uncut (8 faces)");
    }

    #[test]
    fn test_recess_penetrating_mid_structural_layer() {
        let mut scene = Scene::new();

        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(8.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(8.0, 8.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 8.0)));
        pl.is_closed = true;
        let slab_h = scene.add_entity(EntityType::LwPolyline(pl));

        let mut slab = Slab::new("style_slab_screed_concrete", 1);
        slab.layers = vec![
            SlabLayer::new("Floor Screed", 0.05, LayerFunction::Finish),
            SlabLayer::new("Reinforced Concrete", 0.20, LayerFunction::Structural),
        ];
        slab.base_origin = [0.0, 0.0, 0.0];

        // 10cm deep recess: penetrates 5cm screed and 5cm into the 20cm structural concrete
        let mut op_pl = LwPolyline::new();
        op_pl.add_vertex(LwVertex::new(Vector2::new(2.0, 2.0)));
        op_pl.add_vertex(LwVertex::new(Vector2::new(5.0, 2.0)));
        op_pl.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
        op_pl.add_vertex(LwVertex::new(Vector2::new(2.0, 5.0)));
        op_pl.is_closed = true;
        let op_h = scene.add_entity(EntityType::LwPolyline(op_pl));

        let opening = SlabOpening::new_recess(
            slab_h,
            SlabOpeningKind::Custom,
            0.10,
            vec![(2.0, 2.0), (5.0, 2.0), (5.0, 5.0), (2.0, 5.0)],
        );
        write_slab_opening_record(&mut scene.document, op_h, &opening);

        slab.opening_handles.push(op_h);
        write_slab_record(&mut scene.document, slab_h, &slab);

        let ok = regenerate_slab_representation(&mut scene, slab_h, None, None);
        assert!(ok, "Slab regeneration with mid-layer recess should succeed");

        let updated_slab = slab_from_entity(scene.document.get_entity(slab_h).unwrap()).unwrap();
        let solid_handles: Vec<Handle> = updated_slab
            .derived_handles
            .into_iter()
            .filter(|h| scene.solid_models.contains_key(h))
            .collect();

        // 3 solid bodies: Screed (0..0.05, cut), Concrete upper (0.05..0.10, cut), Concrete lower (0.10..0.25, uncut)
        assert_eq!(solid_handles.len(), 3, "3 solid bodies expected for mid-layer penetration");

        let screed_solid = scene.solid_models.get(&solid_handles[0]).unwrap();
        let concrete_upper = scene.solid_models.get(&solid_handles[1]).unwrap();
        let concrete_lower = scene.solid_models.get(&solid_handles[2]).unwrap();

        assert!(screed_solid.faces.len() > 8, "Screed is fully cut out by 10cm recess");
        assert!(concrete_upper.faces.len() > 8, "Concrete upper 5cm is cut out by 10cm recess");
        assert_eq!(concrete_lower.faces.len(), 8, "Concrete lower 15cm is preserved uncut under recess (8 faces)");

        // Verify Z extents of preserved lower concrete solid
        let z_min = concrete_lower.vertices.iter().map(|(_, v)| v.point[2]).fold(f64::INFINITY, f64::min);
        let z_max = concrete_lower.vertices.iter().map(|(_, v)| v.point[2]).fold(f64::NEG_INFINITY, f64::max);
        assert!((z_min - (-0.25)).abs() < 1e-4, "Concrete lower bottom Z is -0.25");
        assert!((z_max - (-0.10)).abs() < 1e-4, "Concrete lower top Z is -0.10");

        // Verify Z extents of cut upper concrete solid
        let z_up_min = concrete_upper.vertices.iter().map(|(_, v)| v.point[2]).fold(f64::INFINITY, f64::min);
        let z_up_max = concrete_upper.vertices.iter().map(|(_, v)| v.point[2]).fold(f64::NEG_INFINITY, f64::max);
        assert!((z_up_min - (-0.10)).abs() < 1e-4, "Concrete upper bottom Z is -0.10");
        assert!((z_up_max - (-0.05)).abs() < 1e-4, "Concrete upper top Z is -0.05");
    }

    #[test]
    fn test_recess_and_through_hole_combination() {
        let mut scene = Scene::new();

        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(12.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(12.0, 10.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 10.0)));
        pl.is_closed = true;
        let slab_h = scene.add_entity(EntityType::LwPolyline(pl));

        let mut slab = Slab::new("style_slab_screed_concrete", 1);
        slab.layers = vec![
            SlabLayer::new("Floor Screed", 0.05, LayerFunction::Finish),
            SlabLayer::new("Reinforced Concrete", 0.20, LayerFunction::Structural),
        ];
        slab.base_origin = [0.0, 0.0, 0.0];

        // 1. Through-hole (Stairwell) at [1..4, 1..4]
        let mut through_pl = LwPolyline::new();
        through_pl.add_vertex(LwVertex::new(Vector2::new(1.0, 1.0)));
        through_pl.add_vertex(LwVertex::new(Vector2::new(4.0, 1.0)));
        through_pl.add_vertex(LwVertex::new(Vector2::new(4.0, 4.0)));
        through_pl.add_vertex(LwVertex::new(Vector2::new(1.0, 4.0)));
        through_pl.is_closed = true;
        let through_h = scene.add_entity(EntityType::LwPolyline(through_pl));

        let through_op = SlabOpening::new_through_hole(
            slab_h,
            SlabOpeningKind::Stairwell,
            vec![(1.0, 1.0), (4.0, 1.0), (4.0, 4.0), (1.0, 4.0)],
        );
        write_slab_opening_record(&mut scene.document, through_h, &through_op);
        slab.opening_handles.push(through_h);

        // 2. Recess (10cm deep, penetrating 5cm screed + 5cm concrete) at [7..10, 6..9]
        let mut recess_pl = LwPolyline::new();
        recess_pl.add_vertex(LwVertex::new(Vector2::new(7.0, 6.0)));
        recess_pl.add_vertex(LwVertex::new(Vector2::new(10.0, 6.0)));
        recess_pl.add_vertex(LwVertex::new(Vector2::new(10.0, 9.0)));
        recess_pl.add_vertex(LwVertex::new(Vector2::new(7.0, 9.0)));
        recess_pl.is_closed = true;
        let recess_h = scene.add_entity(EntityType::LwPolyline(recess_pl));

        let recess_op = SlabOpening::new_recess(
            slab_h,
            SlabOpeningKind::Duct,
            0.10,
            vec![(7.0, 6.0), (10.0, 6.0), (10.0, 9.0), (7.0, 9.0)],
        );
        write_slab_opening_record(&mut scene.document, recess_h, &recess_op);
        slab.opening_handles.push(recess_h);

        write_slab_record(&mut scene.document, slab_h, &slab);

        let ok = regenerate_slab_representation(&mut scene, slab_h, None, None);
        assert!(ok, "Slab regeneration with both through-hole and recess should succeed");

        let updated_slab = slab_from_entity(scene.document.get_entity(slab_h).unwrap()).unwrap();
        let solid_handles: Vec<Handle> = updated_slab
            .derived_handles
            .into_iter()
            .filter(|h| scene.solid_models.contains_key(h))
            .collect();

        // 3 solids: Screed (cut by both), Concrete upper (cut by both), Concrete lower (cut ONLY by through-hole)
        assert_eq!(solid_handles.len(), 3, "3 solid bodies expected");

        let screed = scene.solid_models.get(&solid_handles[0]).unwrap();
        let concrete_up = scene.solid_models.get(&solid_handles[1]).unwrap();
        let concrete_low = scene.solid_models.get(&solid_handles[2]).unwrap();

        // Screed and upper concrete have 2 holes each (through-hole + recess)
        assert!(screed.faces.len() > 8);
        assert!(concrete_up.faces.len() > 8);
        // Lower concrete has 1 hole (through-hole)
        assert!(concrete_low.faces.len() > 8);
    }

    #[test]
    fn test_display_config_rules_component_hiding() {
        let mut scene = Scene::new();

        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 5.0)));
        pl.is_closed = true;
        let slab_h = scene.add_entity(EntityType::LwPolyline(pl));

        let mut slab = Slab::new("style_slab_concrete_20", 1);
        slab.layers = vec![
            SlabLayer::new("Reinforced Concrete", 0.20, LayerFunction::Structural),
        ];
        slab.base_origin = [0.0, 0.0, 0.0];
        write_slab_record(&mut scene.document, slab_h, &slab);

        // Hide solids and ceiling outline, keep 2D contour and hatch
        let mut rules = ComponentRuleSet::default();
        rules.visibility.insert("Solid3D".to_string(), false);
        rules.visibility.insert("CeilingOutline2D".to_string(), false);

        let ok = regenerate_slab_representation(&mut scene, slab_h, None, Some(&rules));
        assert!(ok);

        assert!(scene.solid_models.is_empty(), "Solids should not be generated when Solid3D is hidden");
        assert!(scene.meshes.is_empty(), "Meshes should not be generated when Solid3D is hidden");

        let updated_slab = slab_from_entity(scene.document.get_entity(slab_h).unwrap()).unwrap();
        assert!(!updated_slab.derived_handles.is_empty(), "2D contour and hatch handles should exist");
    }

    #[test]
    fn test_sloped_control_plane_slab_solid() {
        let mut scene = Scene::new();

        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(10.0, 10.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 10.0)));
        pl.is_closed = true;
        let slab_h = scene.add_entity(EntityType::LwPolyline(pl));

        let mut slab = Slab::new("style_slab_pitched_roof_30", 1);
        slab.layers = vec![
            SlabLayer::new("Timber", 0.24, LayerFunction::Structural),
        ];

        // Attach sloped top plane: Normal [0.0, -0.6, 0.8] (slopes upwards along +Y)
        slab.top_origin = [0.0, 0.0, 3.0];
        slab.top_normal = [0.0, -0.6, 0.8];
        slab.base_origin = [0.0, 0.0, 2.76];
        slab.base_normal = [0.0, -0.6, 0.8];
        write_slab_record(&mut scene.document, slab_h, &slab);

        let ok = regenerate_slab_representation(&mut scene, slab_h, None, None);
        assert!(ok, "Sloped slab regeneration should succeed");

        let solid = scene.solid_models.values().next().unwrap();
        // Check that vertex Z coordinates vary with Y (sloped)
        let z_at_y0 = solid.vertices.iter()
            .find(|(_, v)| v.point[1].abs() < 1e-4 && v.point[2] > 2.0)
            .map(|(_, v)| v.point[2])
            .unwrap();
        let z_at_y10 = solid.vertices.iter()
            .find(|(_, v)| (v.point[1] - 10.0).abs() < 1e-4 && v.point[2] > 2.0)
            .map(|(_, v)| v.point[2])
            .unwrap();

        assert!((z_at_y10 - z_at_y0).abs() > 1.0, "Top Z should slope upwards significantly along Y");
    }

    #[test]
    fn test_package_expansion_and_resolution() {
        let mut scene = Scene::new();

        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(6.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(6.0, 6.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 6.0)));
        pl.is_closed = true;
        let slab_h = scene.add_entity(EntityType::LwPolyline(pl));

        let mut slab = Slab::new("style_slab_concrete_20", 1);
        slab.layers = vec![SlabLayer::new("Concrete", 0.20, LayerFunction::Structural)];
        write_slab_record(&mut scene.document, slab_h, &slab);

        regenerate_slab_representation(&mut scene, slab_h, None, None);

        let updated_slab = slab_from_entity(scene.document.get_entity(slab_h).unwrap()).unwrap();
        let derived_h = updated_slab.derived_handles[0];

        assert_eq!(resolve_slab_package(&scene, derived_h), slab_h, "Derived entity should resolve to slab carrier");
        assert!(is_slab_derived(&scene, derived_h), "Derived entity should be recognized as slab derived");
        assert!(is_slab_carrier_entity(scene.document.get_entity(slab_h).unwrap()), "Carrier entity recognized");

        let package = expand_handles_for_slab_packages(&scene, &[derived_h]);
        assert!(package.contains(&slab_h), "Package expansion should include carrier");
        assert!(package.contains(&derived_h), "Package expansion should include derived child");
    }

    #[test]
    fn test_plan_type_and_scale_representation_switching() {
        use crate::modules::aec::engine::library::build_effective_slab_rule_set;
        use crate::modules::aec::engine::plan_view::{DisplayConfig, PlanningStage, ViewType};
        use crate::modules::aec::engine::slab_style::{SlabStyle, SlabStyleLayer};
        use crate::modules::aec::engine::wall_style::LayerValue;

        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 5.0)));
        pl.is_closed = true;
        let slab_h = scene.add_entity(EntityType::LwPolyline(pl));

        let mut slab = Slab::new("style_slab_multi", 1);
        slab.layers = vec![
            SlabLayer::new("Parquet", 0.02, LayerFunction::Finish),
            SlabLayer::new("Screed", 0.06, LayerFunction::Finish),
            SlabLayer::new("Insulation", 0.06, LayerFunction::Insulation),
            SlabLayer::new("Concrete", 0.20, LayerFunction::Structural),
            SlabLayer::new("Plaster", 0.01, LayerFunction::Finish),
        ];
        write_slab_record(&mut scene.document, slab_h, &slab);

        // 1. Werkplan 1:50 (Execution): Hatching enabled
        let werkplan_cfg = DisplayConfig::new(
            "Werkplan 1:50".to_string(),
            "Ausführung".to_string(),
            PlanningStage::Execution,
            ViewType::FloorPlan,
        );
        let rules_50 = build_effective_slab_rule_set(&werkplan_cfg, None, None);
        regenerate_slab_representation(&mut scene, slab_h, None, Some(&rules_50));
        assert!(
            !scene.hatches.is_empty(),
            "Werkplan 1:50 must generate 2D sectional layer hatch"
        );

        // 2. Entwurf 1:100 (Design): Simplified outer contour, hatching suppressed
        let entwurf_cfg = DisplayConfig::new(
            "Entwurf 1:100".to_string(),
            "Entwurf".to_string(),
            PlanningStage::Design,
            ViewType::FloorPlan,
        );
        let rules_100 = build_effective_slab_rule_set(&entwurf_cfg, None, None);
        regenerate_slab_representation(&mut scene, slab_h, None, Some(&rules_100));
        assert!(
            scene.hatches.is_empty(),
            "Entwurf 1:100 must suppress internal hatching"
        );

        // 3. Deckenspiegel (RCP): CeilingOutline2D active, Contour2D suppressed
        let rcp_cfg = DisplayConfig::new(
            "Deckenspiegel".to_string(),
            "Ausbau".to_string(),
            PlanningStage::Execution,
            ViewType::FloorPlan,
        );
        let rules_rcp = build_effective_slab_rule_set(&rcp_cfg, None, None);
        assert!(!rules_rcp.is_slab_visible(SlabComponentSlot::Contour2D));
        assert!(rules_rcp.is_slab_visible(SlabComponentSlot::CeilingOutline2D));

        // 4. Structural-only (Rohbau): 3D solids generate only the structural core
        let rohbau_cfg = DisplayConfig::new(
            "Rohbau 3D".to_string(),
            "Rohbau".to_string(),
            PlanningStage::Execution,
            ViewType::FloorPlan,
        );
        let style = SlabStyle::new("style_slab_multi", "Multi Slab").with_layers(vec![
            SlabStyleLayer::new(
                "Parquet",
                LayerValue::Fixed(0.02),
                LayerFunction::Finish,
            ),
            SlabStyleLayer::new(
                "Concrete",
                LayerValue::Fixed(0.20),
                LayerFunction::Structural,
            ),
        ]);
        let rules_rohbau = build_effective_slab_rule_set(&rohbau_cfg, Some(&style), None);
        regenerate_slab_representation(&mut scene, slab_h, None, Some(&rules_rohbau));
        assert_eq!(
            scene.solid_models.len(),
            1,
            "Rohbau display mode should only generate the structural core layer solid"
        );
    }
}
