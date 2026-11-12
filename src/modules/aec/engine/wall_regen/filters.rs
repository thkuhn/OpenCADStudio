use super::*;

pub(super) fn find_end_peer_layers(
    scene: &Scene,
    wall_handle: Handle,
    self_axis_2d: &[(f64, f64)],
    handled_end: usize,
) -> Option<(Vec<engine::miter::MiterLayer>, Option<usize>)> {
    if self_axis_2d.len() < 2 || handled_end >= self_axis_2d.len() {
        return None;
    }
    let pt = self_axis_2d[handled_end];
    let tol = join::JUNCTION_TOLERANCE.max(1e-4);

    let candidates = engine::owner_index::peers_of(&scene.document, wall_handle);
    if candidates.is_empty() {
        return None;
    }
    for peer in candidates {
        if peer == wall_handle {
            continue;
        }
        let peer_axis = get_wall_vertices(scene, peer);
        if peer_axis.len() < 2 {
            continue;
        }
        if (peer_axis[0].x - pt.0).hypot(peer_axis[0].y - pt.1) <= tol {
            let layers = wall_layer_data(scene, peer);
            return Some((layers, Some(0)));
        } else if (peer_axis.last().unwrap().x - pt.0).hypot(peer_axis.last().unwrap().y - pt.1) <= tol {
            let layers = wall_layer_data(scene, peer);
            return Some((layers, Some(peer_axis.len() - 1)));
        }
    }
    None
}

pub(super) fn layer_has_same_material_peer(
    layer: &WallLayer,
    layer_idx: usize,
    self_end: usize,
    all_layers: &[WallLayer],
    peer_data: Option<&(Vec<engine::miter::MiterLayer>, Option<usize>)>,
) -> bool {
    let Some((peer_layers, peer_end)) = peer_data else {
        return false;
    };
    let miter_self_layers: Vec<engine::miter::MiterLayer> = all_layers
        .iter()
        .map(|l| engine::miter::MiterLayer::with_id(l.thickness, l.axis_offset, l.material.clone(), l.function.clone(), l.layer_id))
        .collect();
    let pairing = engine::miter::match_layer_indices_for_l_join(&miter_self_layers, peer_layers, self_end, *peer_end);
    if let Some(Some(other_idx)) = pairing.get(layer_idx) {
        if let Some(other_l) = peer_layers.get(*other_idx) {
            return !layer.material.is_empty() && layer.material.eq_ignore_ascii_case(&other_l.material);
        }
    }
    false
}

pub(super) fn filter_opening_surface_edges(
    set: &mut crate::scene::model::mesh_model::MeshLodSet,
    wires: &mut Vec<acadrust::entities::Wire>,
    openings: &[engine::openings::Opening],
    centerline: &[(f64, f64)],
    solid_height: f64,
    solid_base: f64,
    wall_base_z: f64,
) {
    if openings.is_empty() || centerline.len() < 2 || (set.edge_verts.is_empty() && wires.is_empty()) {
        return;
    }

    struct CutPlane {
        origin: (f64, f64),
        tangent: (f64, f64),
        jamb_intervals: Vec<(f64, f64)>,
    }

    let mut cut_planes: Vec<CutPlane> = Vec::new();
    let zones = engine::elevation_cut::opening_zones(openings);
    for zone in &zones {
        let width = zone.width();
        let xs = engine::elevation_cut::opening_slice_x_positions(width, &zone.holes);
        for x in xs {
            let s_axis = zone.s0 + x;
            if let Some((origin, tangent)) =
                engine::openings::point_and_tangent_at_distance(centerline, s_axis)
            {
                let mut jambs = Vec::new();
                for hole in &zone.holes {
                    let n = hole.len();
                    for i in 0..n {
                        let p_a = hole[i];
                        let p_b = hole[(i + 1) % n];
                        if (p_a.0 - x).abs() <= 1e-3 && (p_b.0 - x).abs() <= 1e-3 {
                            let dz = (p_b.1 - p_a.1).abs();
                            if dz > 1e-3 {
                                let z_low = p_a.1.min(p_b.1) + wall_base_z;
                                let z_high = p_a.1.max(p_b.1) + wall_base_z;
                                jambs.push((z_low, z_high));
                            }
                        }
                    }
                }

                if let Some(existing) = cut_planes.iter_mut().find(|cp| {
                    (cp.origin.0 - origin.0).hypot(cp.origin.1 - origin.1) <= 1e-3
                }) {
                    existing.jamb_intervals.extend(jambs);
                } else {
                    cut_planes.push(CutPlane {
                        origin,
                        tangent,
                        jamb_intervals: jambs,
                    });
                }
            }
        }
    }

    if cut_planes.is_empty() {
        return;
    }

    let is_on_plane = |p: [f64; 3], cp: &CutPlane| -> bool {
        let dot = (p[0] - cp.origin.0) * cp.tangent.0 + (p[1] - cp.origin.1) * cp.tangent.1;
        dot.abs() <= 2e-3
    };

    let process_segment_cb = |p0: [f64; 3], p1: [f64; 3], emit: &mut dyn FnMut([f64; 3], [f64; 3])| {
        let matched_cp = cut_planes.iter().find(|cp| is_on_plane(p0, cp) && is_on_plane(p1, cp));
        let Some(cp) = matched_cp else {
            emit(p0, p1);
            return;
        };

        let dxy = (p1[0] - p0[0]).hypot(p1[1] - p0[1]);
        let dz = (p1[2] - p0[2]).abs();

        // Transverse horizontal edge at wall crown
        if (p0[2] - (solid_base + solid_height)).abs() <= 2e-3
            && (p1[2] - (solid_base + solid_height)).abs() <= 2e-3
        {
            return;
        }
        // Transverse horizontal edge at wall base
        if (p0[2] - solid_base).abs() <= 2e-3 && (p1[2] - solid_base).abs() <= 2e-3 {
            return;
        }

        // Vertical edge on wall face
        if dxy <= 1e-3 && dz > 1e-3 {
            let z_low = p0[2].min(p1[2]);
            let z_high = p0[2].max(p1[2]);
            for &(jamb_low, jamb_high) in &cp.jamb_intervals {
                let overlap_low = z_low.max(jamb_low);
                let overlap_high = z_high.min(jamb_high);
                if overlap_high - overlap_low > 1e-4 {
                    let v_a = [p0[0], p0[1], overlap_low];
                    let v_b = [p0[0], p0[1], overlap_high];
                    emit(v_a, v_b);
                }
            }
            return;
        }

        emit(p0, p1);
    };

    let old_verts = std::mem::take(&mut set.edge_verts);
    let mut new_verts = Vec::with_capacity(old_verts.len());
    let mut new_verts_low = Vec::with_capacity(old_verts.len());

    for chunk in old_verts.chunks_exact(2) {
        let p0 = [chunk[0][0] as f64, chunk[0][1] as f64, chunk[0][2] as f64];
        let p1 = [chunk[1][0] as f64, chunk[1][1] as f64, chunk[1][2] as f64];
        let mut emit = |v0: [f64; 3], v1: [f64; 3]| {
            let h0 = [v0[0] as f32, v0[1] as f32, v0[2] as f32];
            let l0 = [(v0[0] - h0[0] as f64) as f32, (v0[1] - h0[1] as f64) as f32, (v0[2] - h0[2] as f64) as f32];
            let h1 = [v1[0] as f32, v1[1] as f32, v1[2] as f32];
            let l1 = [(v1[0] - h1[0] as f64) as f32, (v1[1] - h1[1] as f64) as f32, (v1[2] - h1[2] as f64) as f32];
            new_verts.push(h0);
            new_verts_low.push(l0);
            new_verts.push(h1);
            new_verts_low.push(l1);
        };
        process_segment_cb(p0, p1, &mut emit);
    }
    set.edge_verts = new_verts;
    set.edge_verts_low = new_verts_low;

    let old_wires = std::mem::take(wires);
    let mut new_wires = Vec::with_capacity(old_wires.len());
    for wire in old_wires {
        if wire.points.len() < 2 {
            continue;
        }
        for window in wire.points.windows(2) {
            let p0 = [window[0].x, window[0].y, window[0].z];
            let p1 = [window[1].x, window[1].y, window[1].z];
            let mut emit = |v0: [f64; 3], v1: [f64; 3]| {
                new_wires.push(acadrust::entities::Wire::from_points(vec![
                    acadrust::types::Vector3::new(v0[0], v0[1], v0[2]),
                    acadrust::types::Vector3::new(v1[0], v1[1], v1[2]),
                ]));
            };
            process_segment_cb(p0, p1, &mut emit);
        }
    }
    *wires = new_wires;
}

pub(super) fn dist_sq_2d(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0) * (a.0 - b.0) + (a.1 - b.1) * (a.1 - b.1)
}

pub(super) fn dist_to_segment_2d(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len_sq = dx * dx + dy * dy;
    if len_sq <= 1e-12 {
        return (p.0 - a.0).hypot(p.1 - a.1);
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len_sq).clamp(0.0, 1.0);
    let proj = (a.0 + t * dx, a.1 + t * dy);
    (p.0 - proj.0).hypot(p.1 - proj.1)
}

/// Partition a 2D layer footprint polygon along facet boundary edges of multi-polygon control planes.
pub fn partition_footprint_by_facets(
    footprint: &[(f64, f64)],
    footprint_bulges: &[f64],
    facets: &[crate::modules::aec::engine::control_plane::ControlPlaneFacet],
) -> Vec<(Vec<(f64, f64)>, Vec<f64>)> {
    let n = footprint.len();
    if n < 3 || facets.is_empty() {
        return vec![(footprint.to_vec(), footprint_bulges.to_vec())];
    }

    let mut initial_poly: Vec<(f64, f64)> = Vec::with_capacity(n);
    for i in 0..n {
        let start = footprint[i];
        let end = footprint[(i + 1) % n];
        let bulge = footprint_bulges.get(i).copied().unwrap_or(0.0);
        if bulge.abs() > 1e-6 {
            let pts = tessellate_bulge_segment(start, end, bulge, 12);
            for pt in pts.into_iter().take_while(|p| (p.0 - end.0).hypot(p.1 - end.1) > 1e-6) {
                initial_poly.push(pt);
            }
        } else {
            initial_poly.push(start);
        }
    }

    if initial_poly.len() < 3 {
        return vec![(footprint.to_vec(), footprint_bulges.to_vec())];
    }

    // Collect all cutting line segments from facet edges
    let mut cut_lines: Vec<((f64, f64), (f64, f64))> = Vec::new();
    for facet in facets {
        let fpoly = facet.polygon_2d();
        let fn_verts = fpoly.len();
        if fn_verts < 3 {
            continue;
        }
        for i in 0..fn_verts {
            let a = fpoly[i];
            let b = fpoly[(i + 1) % fn_verts];
            if dist_sq_2d(a, b) > 1e-8 {
                cut_lines.push((a, b));
            }
        }
    }

    let mut current_pieces: Vec<Vec<(f64, f64)>> = vec![initial_poly];

    for (a, b) in cut_lines {
        let dx = b.0 - a.0;
        let dy = b.1 - a.1;
        let len = dx.hypot(dy);
        if len <= 1e-6 {
            continue;
        }
        let nx = -dy / len;
        let ny = dx / len;
        let keep1 = (a.0 + nx * 100.0, a.1 + ny * 100.0);
        let keep2 = (a.0 - nx * 100.0, a.1 - ny * 100.0);

        let mut next_pieces: Vec<Vec<(f64, f64)>> = Vec::new();
        for poly in current_pieces {
            let p1 = super::super::miter::clip_closed_to_halfplane(&poly, a, b, keep1);
            let p2 = super::super::miter::clip_closed_to_halfplane(&poly, a, b, keep2);
            let p1_valid = p1.len() >= 3 && super::super::geometry::area(&p1) > 1e-6;
            let p2_valid = p2.len() >= 3 && super::super::geometry::area(&p2) > 1e-6;
            if p1_valid && p2_valid {
                next_pieces.push(p1);
                next_pieces.push(p2);
            } else {
                next_pieces.push(poly);
            }
        }
        current_pieces = next_pieces;
    }

    current_pieces
        .into_iter()
        .filter(|p| p.len() >= 3 && super::super::geometry::area(p) > 1e-6)
        .map(|p| {
            let bulges = vec![0.0; p.len()];
            (p, bulges)
        })
        .collect()
}

/// Calculates (z_base, z_top) for a specific footprint piece of a wall layer at world (x, y).
pub fn piece_z_base_top_at_xy(
    piece_footprint: &[(f64, f64)],
    wall: &Wall,
    layer: Option<&WallLayer>,
    _fallback_base_z: f64,
    _fallback_height: f64,
    x: f64,
    y: f64,
) -> (f64, f64) {
    let m = piece_footprint.len().max(1);
    let cx: f64 = piece_footprint.iter().map(|p| p.0).sum::<f64>() / m as f64;
    let cy: f64 = piece_footprint.iter().map(|p| p.1).sum::<f64>() / m as f64;

    let dominant_top_facet = if !wall.top_facets.is_empty() {
        let dummy = crate::modules::aec::engine::control_plane::ControlPlane {
            id: uuid::Uuid::nil(),
            name: String::new(),
            origin: wall.top_origin,
            normal: wall.top_normal,
            facets: wall.top_facets.clone(),
            face_handle: None,
            preview_handle: None,
            visible: true,
        };
        dummy.min_facet_at_xy(cx, cy).cloned()
    } else {
        None
    };

    let dominant_base_facet = if !wall.base_facets.is_empty() {
        let dummy = crate::modules::aec::engine::control_plane::ControlPlane {
            id: uuid::Uuid::nil(),
            name: String::new(),
            origin: wall.base_origin,
            normal: wall.base_normal,
            facets: wall.base_facets.clone(),
            face_handle: None,
            preview_handle: None,
            visible: true,
        };
        dummy.min_facet_at_xy(cx, cy).cloned()
    } else {
        None
    };

    let zb = if let Some(ref f) = dominant_base_facet {
        let off = layer.map_or(wall.base_offset, |l| wall.base_offset + l.bottom_offset);
        f.z_offset_at_xy(x, y, off).unwrap_or_else(|| {
            if let Some(l) = layer {
                wall.layer_base_z_at_xy(l, x, y)
            } else {
                wall.base_z_at_xy(x, y)
            }
        })
    } else if !wall.base_facets.is_empty() {
        let bot_off = layer.map_or(0.0, |l| l.bottom_offset);
        wall.base_origin[2] + wall.base_offset + bot_off
    } else if let Some(l) = layer {
        wall.layer_base_z_at_xy(l, x, y)
    } else {
        wall.base_z_at_xy(x, y)
    };

    let zt = if let Some(ref f) = dominant_top_facet {
        let off = layer.map_or(wall.top_offset, |l| wall.top_offset + l.top_offset);
        f.z_offset_at_xy(x, y, off).unwrap_or_else(|| {
            if let Some(l) = layer {
                wall.layer_top_z_at_xy(l, x, y)
            } else {
                wall.top_z_at_xy(x, y)
            }
        })
    } else if !wall.top_facets.is_empty() {
        // Uncovered portion: strictly use the standard wall height above base
        let top_off = layer.map_or(0.0, |l| l.top_offset);
        zb + wall.height + top_off
    } else if let Some(l) = layer {
        wall.layer_top_z_at_xy(l, x, y)
    } else {
        wall.top_z_at_xy(x, y)
    };

    let zt = zt.max(zb + 1e-4);
    (zb, zt)
}

/// Constructs a 3D solid for a wall layer piece with sloped or horizontal base and top.
///
/// Computes per-vertex `(z_base, z_top)` heights across the layer footprint polygon and builds
/// a closed 2-manifold B-Rep via `cadkernel::brep::make::faceted_solid`.
pub fn build_sloped_layer_solid_3d(
    footprint: &[(f64, f64)],
    footprint_bulges: &[f64],
    wall: &Wall,
    layer: Option<&WallLayer>,
    _fallback_base_z: f64,
    _fallback_height: f64,
) -> Option<cadkernel::brep::Body> {
    let n = footprint.len();
    if n < 3 {
        return None;
    }

    let mut polygon: Vec<(f64, f64)> = Vec::with_capacity(n);
    for i in 0..n {
        let start = footprint[i];
        let end = footprint[(i + 1) % n];
        let bulge = footprint_bulges.get(i).copied().unwrap_or(0.0);
        if bulge.abs() > 1e-6 {
            let pts = tessellate_bulge_segment(start, end, bulge, 12);
            for pt in pts.into_iter().take_while(|p| (p.0 - end.0).hypot(p.1 - end.1) > 1e-6) {
                polygon.push(pt);
            }
        } else {
            polygon.push(start);
        }
    }

    let m = polygon.len();
    if m < 3 {
        return None;
    }

    // Ensure counter-clockwise (CCW) winding in the XY plane.
    let mut signed_area = 0.0;
    for i in 0..m {
        let (x0, y0) = polygon[i];
        let (x1, y1) = polygon[(i + 1) % m];
        signed_area += x0 * y1 - x1 * y0;
    }
    if signed_area < 0.0 {
        polygon.reverse();
    }

    let mut vertices: Vec<[f64; 3]> = Vec::with_capacity(2 * m);
    // Base vertices (0..m)
    for &(x, y) in &polygon {
        let (zb, _) = piece_z_base_top_at_xy(&polygon, wall, layer, _fallback_base_z, _fallback_height, x, y);
        vertices.push([x, y, zb]);
    }

    // Top vertices (m..2*m)
    for &(x, y) in &polygon {
        let (_, zt) = piece_z_base_top_at_xy(&polygon, wall, layer, _fallback_base_z, _fallback_height, x, y);
        vertices.push([x, y, zt]);
    }

    let mut faces: Vec<Vec<usize>> = Vec::with_capacity(m + 2);
    // Bottom face: looking from below (-Z), reverse of CCW is outward CCW
    let bottom_face: Vec<usize> = (0..m).rev().collect();
    faces.push(bottom_face);

    // Top face: looking from above (+Z), CCW is outward CCW
    let top_face: Vec<usize> = (m..2 * m).collect();
    faces.push(top_face);

    // Side faces: for each edge around the polygon
    for i in 0..m {
        let next = (i + 1) % m;
        // Quad: [base_i, base_next, top_next, top_i]
        faces.push(vec![i, next, next + m, i + m]);
    }

    cadkernel::brep::make::faceted_solid(&vertices, &faces)
}

pub fn filter_facet_partition_edges(
    set: &mut crate::scene::model::mesh_model::MeshLodSet,
    wires: &mut Vec<acadrust::entities::Wire>,
    current_piece: &[(f64, f64)],
    all_pieces: &[(Vec<(f64, f64)>, Vec<f64>)],
    wall: &Wall,
    layer: Option<&WallLayer>,
    fallback_base_z: f64,
    fallback_height: f64,
) {
    if all_pieces.len() <= 1 || (set.edge_verts.is_empty() && wires.is_empty()) {
        return;
    }

    struct FacetSeam {
        q0: (f64, f64),
        q1: (f64, f64),
        overlap_z0: (f64, f64),
        overlap_z1: (f64, f64),
        base_shared: bool,
        top_shared: bool,
    }

    let mut seams: Vec<FacetSeam> = Vec::new();
    let n_curr = current_piece.len();
    if n_curr < 3 {
        return;
    }

    for (other_poly, _) in all_pieces {
        let n_other = other_poly.len();
        if n_other < 3 {
            continue;
        }
        if n_other == n_curr
            && other_poly
                .iter()
                .zip(current_piece.iter())
                .all(|(a, b)| (a.0 - b.0).hypot(a.1 - b.1) <= 1e-4)
        {
            continue;
        }

        for i in 0..n_curr {
            let a1 = current_piece[i];
            let a2 = current_piece[(i + 1) % n_curr];
            let v = (a2.0 - a1.0, a2.1 - a1.1);
            let len = v.0.hypot(v.1);
            if len <= 1e-6 {
                continue;
            }
            let u = (v.0 / len, v.1 / len);
            let n = (-u.1, u.0);

            for j in 0..n_other {
                let b1 = other_poly[j];
                let b2 = other_poly[(j + 1) % n_other];
                let d1 = (b1.0 - a1.0) * n.0 + (b1.1 - a1.1) * n.1;
                let d2 = (b2.0 - a1.0) * n.0 + (b2.1 - a1.1) * n.1;
                if d1.abs() > 2e-3 || d2.abs() > 2e-3 {
                    continue;
                }

                let tb1 = (b1.0 - a1.0) * u.0 + (b1.1 - a1.1) * u.1;
                let tb2 = (b2.0 - a1.0) * u.0 + (b2.1 - a1.1) * u.1;
                let tb_min = tb1.min(tb2);
                let tb_max = tb1.max(tb2);

                let t_min = 0.0_f64.max(tb_min);
                let t_max = len.min(tb_max);

                if t_max - t_min > 1e-3 {
                    let q0 = (a1.0 + t_min * u.0, a1.1 + t_min * u.1);
                    let q1 = (a1.0 + t_max * u.0, a1.1 + t_max * u.1);

                    let (zb_c0, zt_c0) = piece_z_base_top_at_xy(
                        current_piece,
                        wall,
                        layer,
                        fallback_base_z,
                        fallback_height,
                        q0.0,
                        q0.1,
                    );
                    let (zb_c1, zt_c1) = piece_z_base_top_at_xy(
                        current_piece,
                        wall,
                        layer,
                        fallback_base_z,
                        fallback_height,
                        q1.0,
                        q1.1,
                    );

                    let (zb_o0, zt_o0) = piece_z_base_top_at_xy(
                        other_poly,
                        wall,
                        layer,
                        fallback_base_z,
                        fallback_height,
                        q0.0,
                        q0.1,
                    );
                    let (zb_o1, zt_o1) = piece_z_base_top_at_xy(
                        other_poly,
                        wall,
                        layer,
                        fallback_base_z,
                        fallback_height,
                        q1.0,
                        q1.1,
                    );

                    let overlap_z0 = (zb_c0.max(zb_o0), zt_c0.min(zt_o0));
                    let overlap_z1 = (zb_c1.max(zb_o1), zt_c1.min(zt_o1));

                    let base_shared =
                        (zb_c0 - zb_o0).abs() <= 5e-3 && (zb_c1 - zb_o1).abs() <= 5e-3;
                    let top_shared =
                        (zt_c0 - zt_o0).abs() <= 5e-3 && (zt_c1 - zt_o1).abs() <= 5e-3;

                    seams.push(FacetSeam {
                        q0,
                        q1,
                        overlap_z0,
                        overlap_z1,
                        base_shared,
                        top_shared,
                    });
                }
            }
        }
    }

    if seams.is_empty() {
        return;
    }

    let process_segment_cb = |p0: [f64; 3], p1: [f64; 3], emit: &mut dyn FnMut([f64; 3], [f64; 3])| {
        let dxy = (p1[0] - p0[0]).hypot(p1[1] - p0[1]);
        let dz = (p1[2] - p0[2]).abs();

        // 1. Vertical edges on seam endpoints
        if dxy <= 2e-3 && dz > 2e-3 {
            let z_min = p0[2].min(p1[2]);
            let z_max = p0[2].max(p1[2]);
            let pt = (p0[0], p0[1]);

            for seam in &seams {
                let d0 = (pt.0 - seam.q0.0).hypot(pt.1 - seam.q0.1);
                let d1 = (pt.0 - seam.q1.0).hypot(pt.1 - seam.q1.1);
                let matched_ov = if d0 <= 5e-3 {
                    Some(seam.overlap_z0)
                } else if d1 <= 5e-3 {
                    Some(seam.overlap_z1)
                } else {
                    None
                };

                if let Some((ov_min, ov_max)) = matched_ov {
                    if ov_max > ov_min + 1e-4 {
                        let rem_low = z_min.max(ov_min);
                        let rem_high = z_max.min(ov_max);
                        if rem_high > rem_low + 1e-4 {
                            if rem_low > z_min + 1e-4 {
                                emit([p0[0], p0[1], z_min], [p0[0], p0[1], rem_low]);
                            }
                            if z_max > rem_high + 1e-4 {
                                emit([p0[0], p0[1], rem_high], [p0[0], p0[1], z_max]);
                            }
                            return;
                        }
                    }
                }
            }
        }

        // 2. Transverse deck edges along seam
        if dxy > 2e-3 {
            let p0_2d = (p0[0], p0[1]);
            let p1_2d = (p1[0], p1[1]);
            for seam in &seams {
                let dist0 = dist_to_segment_2d(p0_2d, seam.q0, seam.q1);
                let dist1 = dist_to_segment_2d(p1_2d, seam.q0, seam.q1);
                if dist0 <= 5e-3 && dist1 <= 5e-3 {
                    let (zb0, zt0) = piece_z_base_top_at_xy(
                        current_piece,
                        wall,
                        layer,
                        fallback_base_z,
                        fallback_height,
                        p0[0],
                        p0[1],
                    );
                    let (zb1, zt1) = piece_z_base_top_at_xy(
                        current_piece,
                        wall,
                        layer,
                        fallback_base_z,
                        fallback_height,
                        p1[0],
                        p1[1],
                    );

                    if seam.base_shared && (p0[2] - zb0).abs() <= 5e-3 && (p1[2] - zb1).abs() <= 5e-3 {
                        return;
                    }
                    if seam.top_shared && (p0[2] - zt0).abs() <= 5e-3 && (p1[2] - zt1).abs() <= 5e-3 {
                        return;
                    }
                }
            }
        }

        emit(p0, p1);
    };

    let old_verts = std::mem::take(&mut set.edge_verts);
    let old_low = std::mem::take(&mut set.edge_verts_low);
    let mut new_verts = Vec::with_capacity(old_verts.len());
    let mut new_verts_low = Vec::with_capacity(old_low.len());

    for chunk in old_verts.chunks_exact(2) {
        let p0 = [chunk[0][0] as f64, chunk[0][1] as f64, chunk[0][2] as f64];
        let p1 = [chunk[1][0] as f64, chunk[1][1] as f64, chunk[1][2] as f64];
        let mut emit = |v0: [f64; 3], v1: [f64; 3]| {
            let h0 = [v0[0] as f32, v0[1] as f32, v0[2] as f32];
            let l0 = [(v0[0] - h0[0] as f64) as f32, (v0[1] - h0[1] as f64) as f32, (v0[2] - h0[2] as f64) as f32];
            let h1 = [v1[0] as f32, v1[1] as f32, v1[2] as f32];
            let l1 = [(v1[0] - h1[0] as f64) as f32, (v1[1] - h1[1] as f64) as f32, (v1[2] - h1[2] as f64) as f32];
            new_verts.push(h0);
            new_verts_low.push(l0);
            new_verts.push(h1);
            new_verts_low.push(l1);
        };
        process_segment_cb(p0, p1, &mut emit);
    }
    set.edge_verts = new_verts;
    set.edge_verts_low = new_verts_low;

    let old_wires = std::mem::take(wires);
    let mut new_wires = Vec::with_capacity(old_wires.len());
    for wire in old_wires {
        if wire.points.len() < 2 {
            continue;
        }
        for window in wire.points.windows(2) {
            let p0 = [window[0].x, window[0].y, window[0].z];
            let p1 = [window[1].x, window[1].y, window[1].z];
            let mut emit = |v0: [f64; 3], v1: [f64; 3]| {
                new_wires.push(acadrust::entities::Wire::from_points(vec![
                    acadrust::types::Vector3::new(v0[0], v0[1], v0[2]),
                    acadrust::types::Vector3::new(v1[0], v1[1], v1[2]),
                ]));
            };
            process_segment_cb(p0, p1, &mut emit);
        }
    }
    *wires = new_wires;
}

pub(super) fn filter_miter_deck_edges(
    set: &mut crate::scene::model::mesh_model::MeshLodSet,
    wires: &mut Vec<acadrust::entities::Wire>,
    miter_segments_2d: &[((f64, f64), (f64, f64))],
    wall: &Wall,
    layer: Option<&WallLayer>,
    fallback_z_base: f64,
    _fallback_z_top: f64,
) {
    if miter_segments_2d.is_empty() || (set.edge_verts.is_empty() && wires.is_empty()) {
        return;
    }

    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for &((x0, y0), (x1, y1)) in miter_segments_2d {
        min_x = min_x.min(x0.min(x1));
        max_x = max_x.max(x0.max(x1));
        min_y = min_y.min(y0.min(y1));
        max_y = max_y.max(y0.max(y1));
    }
    let margin = 0.02;
    let bbox_min_x = min_x - margin;
    let bbox_max_x = max_x + margin;
    let bbox_min_y = min_y - margin;
    let bbox_max_y = max_y + margin;

    let is_deck_miter_segment = |p0: [f64; 3], p1: [f64; 3]| -> bool {
        let (zt0, zb0) = if let Some(l) = layer {
            (wall.layer_top_z_at_xy(l, p0[0], p0[1]), wall.layer_base_z_at_xy(l, p0[0], p0[1]))
        } else {
            (wall.top_z_at_xy(p0[0], p0[1]) + fallback_z_base, wall.base_z_at_xy(p0[0], p0[1]) + fallback_z_base)
        };
        let (zt1, zb1) = if let Some(l) = layer {
            (wall.layer_top_z_at_xy(l, p1[0], p1[1]), wall.layer_base_z_at_xy(l, p1[0], p1[1]))
        } else {
            (wall.top_z_at_xy(p1[0], p1[1]) + fallback_z_base, wall.base_z_at_xy(p1[0], p1[1]) + fallback_z_base)
        };

        let is_at_deck = (p0[2] - zt0).abs() <= 5e-3 && (p1[2] - zt1).abs() <= 5e-3;
        let is_at_base = (p0[2] - zb0).abs() <= 5e-3 && (p1[2] - zb1).abs() <= 5e-3;
        if !is_at_deck && !is_at_base {
            return false;
        }

        let seg_min_x = p0[0].min(p1[0]);
        let seg_max_x = p0[0].max(p1[0]);
        let seg_min_y = p0[1].min(p1[1]);
        let seg_max_y = p0[1].max(p1[1]);
        if seg_max_x < bbox_min_x || seg_min_x > bbox_max_x || seg_max_y < bbox_min_y || seg_min_y > bbox_max_y {
            return false;
        }

        for &(v_a, v_b) in miter_segments_2d {
            let d0a = (p0[0] - v_a.0).hypot(p0[1] - v_a.1);
            let d1b = (p1[0] - v_b.0).hypot(p1[1] - v_b.1);
            let d0b = (p0[0] - v_b.0).hypot(p0[1] - v_b.1);
            let d1a = (p1[0] - v_a.0).hypot(p1[1] - v_a.1);
            if (d0a <= 5e-3 && d1b <= 5e-3) || (d0b <= 5e-3 && d1a <= 5e-3) {
                return true;
            }
        }
        false
    };

    let old_verts = std::mem::take(&mut set.edge_verts);
    let old_low = std::mem::take(&mut set.edge_verts_low);
    let mut new_verts = Vec::with_capacity(old_verts.len());
    let mut new_verts_low = Vec::with_capacity(old_low.len());

    for (i, chunk) in old_verts.chunks_exact(2).enumerate() {
        let p0 = [chunk[0][0] as f64, chunk[0][1] as f64, chunk[0][2] as f64];
        let p1 = [chunk[1][0] as f64, chunk[1][1] as f64, chunk[1][2] as f64];
        if !is_deck_miter_segment(p0, p1) {
            new_verts.push(chunk[0]);
            new_verts.push(chunk[1]);
            if let Some(low_chunk) = old_low.get(i * 2..i * 2 + 2) {
                new_verts_low.push(low_chunk[0]);
                new_verts_low.push(low_chunk[1]);
            }
        }
    }
    set.edge_verts = new_verts;
    set.edge_verts_low = new_verts_low;

    let old_wires = std::mem::take(wires);
    let mut new_wires = Vec::with_capacity(old_wires.len());
    for wire in old_wires {
        if wire.points.len() < 2 {
            continue;
        }
        let mut filtered_segments = Vec::new();
        for win in wire.points.windows(2) {
            let p0 = [win[0].x, win[0].y, win[0].z];
            let p1 = [win[1].x, win[1].y, win[1].z];
            if !is_deck_miter_segment(p0, p1) {
                filtered_segments.push((win[0], win[1]));
            }
        }
        for (w0, w1) in filtered_segments {
            new_wires.push(acadrust::entities::Wire::from_points(vec![w0, w1]));
        }
    }
    *wires = new_wires;
}

pub(super) fn find_cap_edge_index(
    footprint: &[(f64, f64)],
    target_pt: (f64, f64),
    axis_tangent: (f64, f64),
    max_dist: f64,
) -> Option<usize> {
    let n = footprint.len();
    if n < 3 {
        return None;
    }
    let mut best_idx = None;
    let mut best_dist = f64::MAX;

    for i in 0..n {
        let p0 = footprint[i];
        let p1 = footprint[(i + 1) % n];
        let mid = ((p0.0 + p1.0) * 0.5, (p0.1 + p1.1) * 0.5);
        let dist = ((mid.0 - target_pt.0).powi(2) + (mid.1 - target_pt.1).powi(2)).sqrt();
        if dist > max_dist {
            continue;
        }
        let edge = (p1.0 - p0.0, p1.1 - p0.1);
        let edge_len = (edge.0.powi(2) + edge.1.powi(2)).sqrt();
        if edge_len < 1e-6 {
            continue;
        }
        let edge_dir = (edge.0 / edge_len, edge.1 / edge_len);
        let dot = (edge_dir.0 * axis_tangent.0 + edge_dir.1 * axis_tangent.1).abs();
        if dot <= 0.85 && dist < best_dist {
            best_dist = dist;
            best_idx = Some(i);
        }
    }
    best_idx
}

/// Locate a wall's already-established join(s) — pairwise (L/T) or N-way —
/// at the axis end that is *not* `handled_end`, using the peer-link index
/// maintained by `engine::owner_index`, cross-referenced against which of
/// this wall's own axis ends is actually coincident with a peer's endpoint
/// or lies on a peer's span. Reuses the exact same junction-detection and
/// per-layer-miter machinery as `join_junction_in_document`
/// (`join::detect_junctions` + `engine::miter::junction_wall_geoms` +
/// `mitered_junction_layer_footprints_with_overrides`), but purely
/// read-only: it never mutates axis vertices or persists overrides for
/// participants other than confirming this wall's own existing
/// `JunctionOverride`.
///
/// Returns `None` when there is no peer at the other end (the common case:
/// a wall with only one join, or a freshly drawn unjoined end), which keeps
/// `regenerate_wall_representation_inner` byte-for-byte unchanged for that
/// case. Used so a NEW join event at one end doesn't silently drop an
/// already-established join at the other end during regeneration.
///
/// Walls whose AABB overlaps `self_axis_2d` inflated by `radius`, excluding
/// `wall_handle`. One O(N) pass with a cheap bbox reject; exact junction
/// tests then run only on the nearby subset.
pub(super) fn nearby_wall_axis_handles(
    scene: &Scene,
    wall_handle: Handle,
    self_axis_2d: &[(f64, f64)],
    radius: f64,
) -> Vec<Handle> {
    let self_axis: Vec<DVec3> = self_axis_2d
        .iter()
        .map(|(x, y)| DVec3::new(*x, *y, 0.0))
        .collect();
    let self_bulges = get_wall_bulges(scene, wall_handle);
    let self_boxes = segment_aabbs_with_bulges(&self_axis, &self_bulges);
    let self_aabb = union_aabbs(&self_boxes);
    all_wall_axis_handles(scene)
        .into_iter()
        .filter(|&h| {
            if h == wall_handle {
                return false;
            }
            let other = get_wall_vertices(scene, h);
            let other_boxes = segment_aabbs_with_bulges(&other, &get_wall_bulges(scene, h));
            segment_boxes_overlap(self_aabb, &self_boxes, &other_boxes, radius)
        })
        .collect()
}

/// Per-segment XY AABBs of a wall axis. Arc segments are inflated by their
/// sagitta (`|bulge| * chord / 2`) in every direction, which conservatively
/// bounds the true arc for any bulge (also beyond a semicircle). A single
/// vertex axis yields one point box.
pub(super) fn segment_aabbs_with_bulges(axis: &[DVec3], bulges: &[f64]) -> Vec<(f64, f64, f64, f64)> {
    if axis.len() < 2 {
        return axis.iter().map(|p| (p.x, p.y, p.x, p.y)).collect();
    }
    (0..axis.len() - 1)
        .map(|i| {
            let (a, b) = (axis[i], axis[i + 1]);
            let bulge = bulges.get(i).copied().unwrap_or(0.0);
            let sag = if bulge.is_finite() {
                bulge.abs() * (b.x - a.x).hypot(b.y - a.y) * 0.5
            } else {
                0.0
            };
            (
                a.x.min(b.x) - sag,
                a.y.min(b.y) - sag,
                a.x.max(b.x) + sag,
                a.y.max(b.y) + sag,
            )
        })
        .collect()
}

pub(super) fn union_aabbs(boxes: &[(f64, f64, f64, f64)]) -> (f64, f64, f64, f64) {
    boxes.iter().fold(
        (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        |acc, b| (acc.0.min(b.0), acc.1.min(b.1), acc.2.max(b.2), acc.3.max(b.3)),
    )
}

/// Whole-axis reject first, then segment-vs-segment so a long / L-shaped
/// polyline does not pull in everything inside its overall bounding box.
pub(super) fn segment_boxes_overlap(
    self_aabb: (f64, f64, f64, f64),
    self_boxes: &[(f64, f64, f64, f64)],
    other_boxes: &[(f64, f64, f64, f64)],
    radius: f64,
) -> bool {
    let other_aabb = union_aabbs(other_boxes);
    if !super::super::join_ops::aabb_overlaps_inflated(self_aabb, other_aabb, radius) {
        return false;
    }
    self_boxes.iter().any(|sb| {
        other_boxes
            .iter()
            .any(|ob| super::super::join_ops::aabb_overlaps_inflated(*sb, *ob, radius))
    })
}

