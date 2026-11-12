use super::*;
use super::super::join::{JoinKind, JunctionParticipant, LayerPairOverride};

fn close(a: (f64, f64), b: (f64, f64), tol: f64) -> bool {
    dist(a, b) < tol
}

/// Single-layer helper: centered on the axis (`axis_offset = -t/2`).
fn g(t: f64) -> MiterLayer {
    MiterLayer::geom(t, -t * 0.5)
}

/// Multi-layer helper: centered stack matching the legacy gap_before=0 layout.
fn gs(thicknesses: &[f64]) -> Vec<MiterLayer> {
    let pairs: Vec<(f64, f64)> = thicknesses.iter().map(|&th| (th, 0.0)).collect();
    let offsets = crate::modules::aec::engine::wall_style::migrate_gap_before_to_axis_offset(&pairs);
    thicknesses
        .iter()
        .zip(offsets)
        .map(|(&th, off)| MiterLayer::geom(th, off))
        .collect()
}

#[test]
fn merge_end_footprints_picks_the_end_that_actually_changed() {
    let base = vec![(0.0, -0.1), (10.0, -0.1), (10.0, 0.1), (0.0, 0.1)];
    // End-0 result only moves the vertices near x=0.
    let end0 = vec![(-0.2, -0.1), (10.0, -0.1), (10.0, 0.1), (-0.2, 0.1)];
    // End-1 result only moves the vertices near x=10.
    let end1 = vec![(0.0, -0.1), (10.2, -0.1), (10.2, 0.1), (0.0, 0.1)];

    let merged = merge_end_footprints(&base, Some(&end0), Some(&end1))
        .expect("both ends supplied should merge");
    assert_eq!(
        merged,
        vec![(-0.2, -0.1), (10.2, -0.1), (10.2, 0.1), (-0.2, 0.1)],
        "merge should keep each end's own change and combine them, got {merged:?}"
    );

    // Only one side supplied falls back to it unchanged.
    assert_eq!(merge_end_footprints(&base, Some(&end0), None), Some(end0.clone()));
    assert_eq!(merge_end_footprints(&base, None, Some(&end1)), Some(end1.clone()));
    assert_eq!(merge_end_footprints(&base, None, None), None);

    // Mismatched vertex count is ignored (falls back to the other side).
    let bad = vec![(0.0, 0.0)];
    assert_eq!(merge_end_footprints(&base, Some(&bad), Some(&end1)), Some(end1.clone()));
}

#[test]
fn through_cutout_does_not_square_off_mitered_end() {
    let mitered = vec![
        (0.0, -0.1),
        (10.2, -0.1),
        (10.2, 0.1),
        (0.0, 0.1),
    ];
    let mut cutout = vec![
        (0.0, -0.1),
        (4.5, -0.1),
        (4.5, 0.1),
        (0.0, 0.1),
    ];
    cutout.push((f64::NAN, f64::NAN));
    cutout.extend([
        (5.5, -0.1),
        (10.0, -0.1),
        (10.0, 0.1),
        (5.5, 0.1),
    ]);
    let merged = merge_through_cutout_preserving_miter(Some(&mitered), &cutout);
    let rings = split_footprint_rings(&merged);
    assert_eq!(rings.len(), 2, "gap must stay two rings, got {merged:?}");
    let max_x = merged
        .iter()
        .filter(|p| p.0.is_finite())
        .map(|p| p.0)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        max_x > 10.05,
        "L-miter beyond the axis end must survive the T cutout, max_x={max_x} pts={merged:?}"
    );
    assert!(
        merged.iter().any(|p| p.0.is_finite() && p.0 < 4.6)
            && merged.iter().any(|p| p.0.is_finite() && (p.0 - 5.5).abs() < 0.2),
        "notch around x=5 must remain, pts={merged:?}"
    );
}

#[test]
fn through_cutout_multiple_crossings_preserves_miters() {
    // Mitered wall with L-miters at both end 0 (x = -0.2) and end 1 (x = 10.2)
    let mitered = vec![
        (-0.2, -0.1),
        (10.2, -0.1),
        (10.2, 0.1),
        (-0.2, 0.1),
    ];
    // Crossing 1 at x=3 (gap 2.9..3.1)
    let mut cut1 = vec![
        (0.0, -0.1),
        (2.9, -0.1),
        (2.9, 0.1),
        (0.0, 0.1),
    ];
    cut1.push((f64::NAN, f64::NAN));
    cut1.extend([
        (3.1, -0.1),
        (10.0, -0.1),
        (10.0, 0.1),
        (3.1, 0.1),
    ]);
    let merged1 = merge_through_cutout_preserving_miter(Some(&mitered), &cut1);
    let rings1 = split_footprint_rings(&merged1);
    assert_eq!(rings1.len(), 2, "first cut produces 2 rings");

    // Crossing 2 at x=7 (gap 6.9..7.1)
    let mut cut2 = vec![
        (0.0, -0.1),
        (6.9, -0.1),
        (6.9, 0.1),
        (0.0, 0.1),
    ];
    cut2.push((f64::NAN, f64::NAN));
    cut2.extend([
        (7.1, -0.1),
        (10.0, -0.1),
        (10.0, 0.1),
        (7.1, 0.1),
    ]);
    let merged2 = merge_through_cutout_preserving_miter(Some(&merged1), &cut2);
    let rings2 = split_footprint_rings(&merged2);
    assert_eq!(rings2.len(), 3, "second cut produces 3 rings, got {merged2:?}");

    // Ring 1 must preserve end 0 L-miter (x = -0.2)
    let min_x = rings2[0].iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    assert!(min_x < -0.15, "ring 1 must keep end 0 miter, got min_x={min_x}");

    // Ring 3 must preserve end 1 L-miter (x = 10.2)
    let max_x = rings2[2].iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    assert!(max_x > 10.15, "ring 3 must keep end 1 miter, got max_x={max_x}");

    // Ring 2 must span from 3.1 to 6.9
    let r2_min_x = rings2[1].iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let r2_max_x = rings2[1].iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    assert!((r2_min_x - 3.1).abs() < 1e-4, "ring 2 min x = 3.1, got {r2_min_x}");
    assert!((r2_max_x - 6.9).abs() < 1e-4, "ring 2 max x = 6.9, got {r2_max_x}");
}

#[test]
fn through_cutout_three_ring_one_shot_preserves_end_miter_and_bounds_middle() {
    // Crossing wall along +Y (x ∈ [4.925, 5.075], y −10…10) with an L-miter
    // vertex at (4.925, 10.075). Combining a pre-built 3-ring cutout in one
    // call used to clip the middle ring on only one notch, so it ran through
    // the second through-wall core up to the L end.
    let mitered = vec![
        (4.925, -10.0),
        (5.075, -10.0),
        (5.075, 10.0),
        (4.925, 10.075),
    ];
    let mut cutout = vec![
        (4.925, -10.0),
        (5.075, -10.0),
        (5.075, -4.1),
        (4.925, -4.1),
    ];
    cutout.push((f64::NAN, f64::NAN));
    cutout.extend([
        (4.925, -3.9),
        (5.075, -3.9),
        (5.075, 3.9),
        (4.925, 3.9),
    ]);
    cutout.push((f64::NAN, f64::NAN));
    cutout.extend([
        (4.925, 4.1),
        (5.075, 4.1),
        (5.075, 10.0),
        (4.925, 10.0),
    ]);
    let merged = merge_through_cutout_preserving_miter(Some(&mitered), &cutout);
    let mut rings = split_footprint_rings(&merged);
    assert_eq!(rings.len(), 3, "3-ring cutout + L miter must stay 3 rings, got {merged:?}");
    rings.sort_by(|a, b| {
        let ay = a.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        let by = b.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        ay.partial_cmp(&by).unwrap()
    });
    let y_range = |r: &[(f64, f64)]| {
        let min = r.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        let max = r.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        (min, max)
    };
    let (_smin, smax) = y_range(&rings[0]);
    let (mmin, mmax) = y_range(&rings[1]);
    let (nmin, nmax) = y_range(&rings[2]);
    assert!(
        smax <= -4.1 + 1e-6,
        "south ring must stop at the first notch, smax={smax}"
    );
    assert!(
        mmin >= -3.9 - 1e-6,
        "middle ring must start after the first core, mmin={mmin}"
    );
    assert!(
        mmax <= 3.9 + 1e-6,
        "middle ring must stop before the second core (not run to the L end), mmax={mmax} ring={:?}",
        rings[1]
    );
    assert!(
        nmin >= 4.1 - 1e-6,
        "north ring must start after the second core, nmin={nmin}"
    );
    assert!(
        nmax > 10.05,
        "north ring must keep the L-miter beyond y=10, nmax={nmax} ring={:?}",
        rings[2]
    );
    assert!(
        rings[2]
            .iter()
            .any(|p| (p.0 - 4.925).abs() < 1e-6 && (p.1 - 10.075).abs() < 1e-6),
        "exact L vertex (4.925, 10.075) must survive, got {:?}",
        rings[2]
    );
}

#[test]
fn l_corner_miter_shares_diagonal_endpoints() {
    // Wall A along +X ending at (10,0); wall B along +Y starting at (10,0).
    // Single layer, thickness 0.2 each → half = 0.1.
    let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
    let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
    let layers = vec![g(0.2)];

    let fps_a = mitered_layer_footprints(
        &axis_a,
        &layers,
        1, // end at (10,0)
        &axis_b,
        &layers,
        Some(0),
        JoinKind::L,
    );
    let fps_b = mitered_layer_footprints(
        &axis_b,
        &layers,
        0,
        &axis_a,
        &layers,
        Some(1),
        JoinKind::L,
    );

    let fp_a = fps_a[0].as_ref().expect("A layer 0 should miter");
    let fp_b = fps_b[0].as_ref().expect("B layer 0 should miter");

    // Expected outer/inner miter corners for equal 0.2 walls at 90°:
    // A boundaries y=±0.1, B boundaries x=10±0.1.
    // Pairing the short diagonal: (10.1, -0.1) and (9.9, 0.1).
    let c1 = (10.1, -0.1);
    let c2 = (9.9, 0.1);
    assert!(
        fp_a.iter().any(|p| close(*p, c1, 1e-6)) && fp_a.iter().any(|p| close(*p, c2, 1e-6)),
        "A footprint should contain both miter corners, got {fp_a:?}"
    );
    assert!(
        fp_b.iter().any(|p| close(*p, c1, 1e-6)) && fp_b.iter().any(|p| close(*p, c2, 1e-6)),
        "B footprint should share the same miter corners, got {fp_b:?}"
    );
}

#[test]
fn t_corner_stem_miters_against_through_wall() {
    // Stem A: (5,0)->(5,10) joins onto through wall B at (5,0).
    // T-join butts the stem straight into the near face of the matching
    // through-wall layer (y = +0.1 from the +Y stem), not a diagonal miter.
    let axis_a = vec![(5.0, 0.0), (5.0, 10.0)];
    let axis_b = vec![(0.0, 0.0), (10.0, 0.0)];
    let layers = vec![g(0.2)];

    let fps_a = mitered_layer_footprints(
        &axis_a,
        &layers,
        0, // stem end at (5,0)
        &axis_b,
        &layers,
        None, // B is through-wall
        JoinKind::T,
    );
    let fp_a = fps_a[0].as_ref().expect("stem layer should extend");

    // Near face of through layer from +Y is y = +0.1. Both stem end-cap
    // vertices must lie on that face (square butt), not on a diagonal.
    let expected = [(4.9, 0.1), (5.1, 0.1)];
    for e in expected {
        assert!(
            fp_a.iter().any(|p| close(*p, e, 1e-6)),
            "stem footprint missing butt corner {e:?}, got {fp_a:?}"
        );
    }
    // Reject the old diagonal miter corners.
    assert!(
        !fp_a.iter().any(|p| close(*p, (5.1, -0.1), 1e-6))
            && !fp_a.iter().any(|p| close(*p, (4.9, -0.1), 1e-6)),
        "stem must not use a diagonal/far-face miter, got {fp_a:?}"
    );
}

#[test]
fn t_corner_right_angle_single_layer_butts_near_face() {
    // Explicit regression for Bug A / Step 3: orthogonal T, one layer.
    let stem = vec![(5.0, 0.0), (5.0, 8.0)];
    let through = vec![(0.0, 0.0), (10.0, 0.0)];
    let layers = vec![g(0.2)];
    let fps = mitered_layer_footprints(
        &stem,
        &layers,
        0,
        &through,
        &layers,
        None,
        JoinKind::T,
    );
    let fp = fps[0].as_ref().expect("stem layer");
    assert!(fp.iter().any(|p| close(*p, (4.9, 0.1), 1e-6)));
    assert!(fp.iter().any(|p| close(*p, (5.1, 0.1), 1e-6)));
    // Join-end cap is horizontal (same y) — not diagonal.
    let end_ys: Vec<f64> = fp
        .iter()
        .filter(|(_, y)| y.abs() < 0.5) // near the joint, not the far end
        .map(|(_, y)| *y)
        .collect();
    assert!(
        end_ys.len() >= 2,
        "expected both stem join-end vertices, got {fp:?}"
    );
    let y0 = end_ys[0];
    assert!(
        end_ys.iter().all(|y| (y - y0).abs() < 1e-6),
        "T butt end-cap must be square (equal y), got {end_ys:?} in {fp:?}"
    );
    assert!(
        (y0 - 0.1).abs() < 1e-5,
        "join end must sit on near face y=+0.1, got {y0}"
    );
}

#[test]
fn t_corner_skewed_60_deg_butts_without_crossing() {
    // Stem approaches through wall at 60° to the through axis.
    let angle = 60.0_f64.to_radians();
    let stem = vec![
        (5.0, 0.0),
        (5.0 + 8.0 * angle.cos(), 8.0 * angle.sin()),
    ];
    let through = vec![(0.0, 0.0), (10.0, 0.0)];
    let layers = vec![g(0.2)];
    let fps = mitered_layer_footprints(
        &stem,
        &layers,
        0,
        &through,
        &layers,
        None,
        JoinKind::T,
    );
    let fp = fps[0].as_ref().expect("skewed stem layer");
    // Both end-cap vertices must lie on the near through face y=+0.1.
    let end_pts: Vec<(f64, f64)> = fp
        .iter()
        .copied()
        .filter(|(_, y)| (*y - 0.1).abs() < 1e-5)
        .collect();
    assert!(
        end_pts.len() >= 2,
        "both stem ends should butt y=+0.1, got {fp:?}"
    );
    // No vertex on the far face (would indicate diagonal or overshoot).
    assert!(
        !fp.iter().any(|(_, y)| (*y + 0.1).abs() < 1e-5),
        "skewed T must not reach far face y=-0.1, got {fp:?}"
    );
}

#[test]
fn t_corner_multi_layer_each_matched_layer_butts_independently() {
    // Through wall: two layers stacked about the axis (each 0.1 thick).
    //   layer0 (core):   y ∈ [-0.1,  0.0]
    //   layer1 (finish): y ∈ [ 0.0,  0.1]
    // Structural cores butt at the core near face; finish stays on the
    // approach outer face and must not tunnel to the far side.
    let stem = vec![(5.0, 0.0), (5.0, 8.0)];
    let through = vec![(0.0, 0.0), (10.0, 0.0)];
    let layers = vec![
        MiterLayer::with_id(0.1, -0.1, "core", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.1, 0.0, "finish", "Finish", uuid::Uuid::new_v4()),
    ];
    let fps = mitered_layer_footprints(
        &stem,
        &layers,
        0,
        &through,
        &layers,
        None,
        JoinKind::T,
    );
    assert_eq!(fps.len(), 2);
    let fp0 = fps[0].as_ref().expect("core layer");
    let fp1 = fps[1].as_ref().expect("finish layer");

    let join_ys = |fp: &[(f64, f64)]| -> Vec<f64> {
        fp.iter()
            .filter(|(_, y)| y.abs() < 0.5)
            .map(|(_, y)| *y)
            .collect()
    };

    let y1 = join_ys(fp1);
    assert!(
        !y1.is_empty()
            && y1.iter().any(|y| (*y - 0.1).abs() < 1e-4)
            && y1.iter().all(|y| *y > -1e-4),
        "finish should miter on the approach side (touch y=+0.1, not far face), got {y1:?} in {fp1:?}"
    );
    // Inner→outer diagonal: inner stem edge meets inner through edge (y=0),
    // outer stem edge meets outer through edge (y=0.1).
    let has_inner = fp1.iter().any(|(x, y)| {
        y.abs() < 1e-4 && (*x - 5.0).abs() < 1e-3
    });
    let has_outer = fp1.iter().any(|(_, y)| (*y - 0.1).abs() < 1e-4);
    assert!(
        has_inner && has_outer,
        "plaster miter must run inner-to-outer, got {fp1:?}"
    );
    assert!(
        !fp1.iter().any(|(x, y)| (*y - 0.1).abs() < 1e-4 && (*x - 5.0).abs() < 1e-3),
        "wrong miter diagonal (outer at inner x), got {fp1:?}"
    );

    let y0 = join_ys(fp0);
    assert!(
        !y0.is_empty() && y0.iter().all(|y| y.abs() < 1e-5),
        "core layer should butt y=0.0, got {y0:?} in {fp0:?}"
    );
    assert!(
        !fp0.iter().any(|(_, y)| (*y + 0.1).abs() < 1e-5),
        "core must not reach far face y=-0.1, got {fp0:?}"
    );
}

#[test]
fn t_corner_unmatched_stem_layer_extends_to_through_outer_face() {
    // Stem has an extra material ("orphan") with no counterpart on the
    // through wall. The through wall has TWO layers (brick + concrete)
    // so the outer face (y=+0.15, outer edge of concrete) is geometrically
    // distinct from the matched brick layer's near face (y=+0.05). The
    // unmatched orphan layer must reach the true outer face, not the
    // brick's inner face.
    let stem = vec![(5.0, 0.0), (5.0, 8.0)];
    let through = vec![(0.0, 0.0), (10.0, 0.0)];
    let layers_stem = vec![
        MiterLayer::with_id(0.2, -0.15, "brick", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.1, 0.05, "orphan", "Other", uuid::Uuid::new_v4()),
    ];
    let layers_through = vec![
        MiterLayer::with_id(0.2, -0.15, "brick", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.1, 0.05, "concrete", "Structure", uuid::Uuid::new_v4()),
    ];

    let fps = mitered_layer_footprints(
        &stem,
        &layers_stem,
        0,
        &through,
        &layers_through,
        None,
        JoinKind::T,
    );
    assert_eq!(fps.len(), 2);
    let fp_match = fps[0].as_ref().expect("matched brick layer");
    let fp_orphan = fps[1].as_ref().expect("unmatched orphan must still extend");

    let join_ys = |fp: &[(f64, f64)]| -> Vec<f64> {
        fp.iter()
            .filter(|(_, y)| y.abs() < 0.5)
            .map(|(_, y)| *y)
            .collect()
    };

    // Thickest fallback cores (no Structural): brick butts at brick near
    // face y=+0.05; unmatched orphan still reaches the outer face.
    let y_match = join_ys(fp_match);
    assert!(
        !y_match.is_empty() && y_match.iter().all(|y| (*y - 0.05).abs() < 1e-5),
        "matched layer should butt at brick's near face y=+0.05, got {y_match:?} in {fp_match:?}"
    );
    let y_orphan = join_ys(fp_orphan);
    assert!(
        !y_orphan.is_empty() && y_orphan.iter().all(|y| (*y - 0.15).abs() < 1e-5),
        "unmatched layer must reach through outer face y=+0.15 (all vertices), got {y_orphan:?} in {fp_orphan:?}"
    );
}

#[test]
fn t_unmatched_outer_face_uses_geometric_extents_not_list_order() {
    // Finish layer listed first but sitting on the +offset side; core
    // listed second with a more negative offset. Outer face toward the
    // stem (+Y) must still be the finish outer edge, not the first list
    // entry's b1.
    let stem = vec![(5.0, 0.0), (5.0, 8.0)];
    let through = vec![(0.0, 0.0), (10.0, 0.0)];
    let layers_stem = vec![MiterLayer::with_id(
        0.1,
        0.05,
        "orphan",
        "Other",
        uuid::Uuid::new_v4(),
    )];
    let layers_through = vec![
        MiterLayer::with_id(0.1, 0.05, "finish", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.2, -0.15, "core", "Structure", uuid::Uuid::new_v4()),
    ];
    let fps = mitered_layer_footprints(
        &stem,
        &layers_stem,
        0,
        &through,
        &layers_through,
        None,
        JoinKind::T,
    );
    let fp = fps[0].as_ref().expect("orphan");
    let y_join: Vec<f64> = fp
        .iter()
        .filter(|(_, y)| y.abs() < 0.5)
        .map(|(_, y)| *y)
        .collect();
    assert!(
        !y_join.is_empty() && y_join.iter().all(|y| (*y - 0.05).abs() < 1e-5),
        "no Structural: thickest through layer is core; stem butts its near face y=+0.05, got {y_join:?} in {fp:?}"
    );
}

#[test]
fn mismatched_layer_count_returns_none_for_unmatched() {
    let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
    let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
    // A has two layers, B has one — only one can match (by offset).
    let layers_a = gs(&[0.2, 0.05]);
    let layers_b = vec![g(0.3)];

    let fps = mitered_layer_footprints(
        &axis_a,
        &layers_a,
        1,
        &axis_b,
        &layers_b,
        Some(0),
        JoinKind::L,
    );
    assert_eq!(fps.len(), 2);
    let matched = fps.iter().filter(|f| f.is_some()).count();
    let unmatched = fps.iter().filter(|f| f.is_none()).count();
    assert_eq!(matched, 1, "exactly one layer should miter, got {fps:?}");
    assert_eq!(
        unmatched, 1,
        "unmatched layer must fall back (None) to corner_override, got {fps:?}"
    );
}

#[test]
fn miter_degenerate_angle_5_deg_falls_back() {
    // sin(5 deg) approx 0.087.
    // Wall A at 0 deg: (0,0) to (10,0)
    // Wall B at 5 deg: (0,0) to (10 * cos(5), 10 * sin(5))
    let angle = 5.0 * std::f64::consts::PI / 180.0;
    let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
    let axis_b = vec![(0.0, 0.0), (10.0 * angle.cos(), 10.0 * angle.sin())];
    let layers = vec![g(0.2)];

    let fps = mitered_layer_footprints(
        &axis_a,
        &layers,
        0,
        &axis_b,
        &layers,
        Some(0),
        JoinKind::L,
    );
    assert!(fps[0].is_none(), "5 degree join should be degenerate and fall back");
}

#[test]
fn miter_degenerate_angle_175_deg_falls_back() {
    // sin(175 deg) = sin(5 deg) approx 0.087.
    let angle = 175.0 * std::f64::consts::PI / 180.0;
    let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
    let axis_b = vec![(0.0, 0.0), (10.0 * angle.cos(), 10.0 * angle.sin())];
    let layers = vec![g(0.2)];

    let fps = mitered_layer_footprints(
        &axis_a,
        &layers,
        0,
        &axis_b,
        &layers,
        Some(0),
        JoinKind::L,
    );
    assert!(fps[0].is_none(), "175 degree join should be degenerate and fall back");
}

#[test]
fn mitered_junction_degenerate_angles() {
    // 3-way junction at (0,0).
    // Wall 0: (0,0) to (10,0)  [0 deg outgoing ray]
    // Wall 1: (0,0) to (10*cos(5), 10*sin(5)) [5 deg outgoing ray]
    // Wall 2: (0,0) to (0, 10) [90 deg outgoing ray]
    let a5 = 5.0 * std::f64::consts::PI / 180.0;
    let junction = Junction {
        point: glam::DVec3::new(0.0, 0.0, 0.0),
        participants: vec![
            JunctionParticipant { wall_index: 0, role: JunctionRole::Endpoint(0) },
            JunctionParticipant { wall_index: 1, role: JunctionRole::Endpoint(0) },
            JunctionParticipant { wall_index: 2, role: JunctionRole::Endpoint(0) },
        ],
    };
    let walls = vec![
        JunctionWallGeom { axis: vec![(0.0, 0.0), (10.0, 0.0)], layers: vec![g(0.2)], end: Some(0) },
        JunctionWallGeom { axis: vec![(0.0, 0.0), (10.0 * a5.cos(), 10.0 * a5.sin())], layers: vec![g(0.2)], end: Some(0) },
        JunctionWallGeom { axis: vec![(0.0, 0.0), (0.0, 10.0)], layers: vec![g(0.2)], end: Some(0) },
    ];

    let res = mitered_junction_layer_footprints(&junction, &walls);
    // Wall 0 (0 deg) has neighbors Wall 2 (90 deg, safe) and Wall 1 (5 deg, degenerate).
    // Wall 1 (5 deg) has neighbors Wall 0 (5 deg, degenerate) and Wall 2 (85 deg, safe).

    // Wall 1's miter with Wall 0 is degenerate.
    // It should not panic or produce NaN.
    let fp1 = &res[1][0];
    if let Some(fp) = fp1 {
        for &(x, y) in fp {
            assert!(!x.is_nan() && !y.is_nan());
            // Miter point shouldn't be extremely far away (e.g. > 100m)
            assert!(x.abs() < 100.0 && y.abs() < 100.0, "miter point too far: ({x}, {y})");
        }
    }
}

#[test]
fn same_composition_different_order_matches_by_material() {
    // A: Brick (outer), Insulation, Concrete (inner)
    // B: same materials reversed in the list — must still pair Brick↔Brick etc.
    let layers_a = vec![
        MiterLayer::with_id(0.1, -0.17500000000000002, "Brick", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.05, -0.07500000000000001, "Insulation", "Insulation", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.2, -0.02500000000000001, "Concrete", "Structural", uuid::Uuid::new_v4()),
    ];
    let layers_b = vec![
        MiterLayer::with_id(0.2, -0.17500000000000002, "Concrete", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.05, 0.024999999999999994, "Insulation", "Insulation", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.1, 0.075, "Brick", "Finish", uuid::Uuid::new_v4()),
    ];
    let pairing = match_layer_indices(&layers_a, &layers_b);
    assert_eq!(pairing, vec![Some(2), Some(1), Some(0)]);

    let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
    let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
    let fps = mitered_layer_footprints(
        &axis_a,
        &layers_a,
        1,
        &axis_b,
        &layers_b,
        Some(0),
        JoinKind::L,
    );
    assert_eq!(fps.len(), 3);
    assert!(
        fps.iter().all(|f| f.is_some()),
        "all three material-matched layers should miter, got {fps:?}"
    );
}

#[test]
fn different_materials_and_counts_fallback_only_unmatched() {
    // A: Brick + Concrete. B: only Insulation (no shared material).
    // Offset-only pairing may still match the closer layer; the other
    // must remain unmatched (None → corner_override).
    let layers_a = vec![
        MiterLayer::with_id(0.1, -0.15000000000000002, "Brick", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.2, -0.05000000000000002, "Concrete", "Structural", uuid::Uuid::new_v4()),
    ];
    let layers_b = vec![MiterLayer::with_id(0.15, -0.075, "Insulation", "Insulation", uuid::Uuid::new_v4())];

    let pairing = match_layer_indices(&layers_a, &layers_b);
    assert_eq!(pairing.len(), 2);
    let matched = pairing.iter().filter(|p| p.is_some()).count();
    let unmatched = pairing.iter().filter(|p| p.is_none()).count();
    assert_eq!(matched, 1, "one offset-based pair expected, got {pairing:?}");
    assert_eq!(
        unmatched, 1,
        "genuinely unmatched layer stays None, got {pairing:?}"
    );

    let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
    let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
    let fps = mitered_layer_footprints(
        &axis_a,
        &layers_a,
        1,
        &axis_b,
        &layers_b,
        Some(0),
        JoinKind::L,
    );
    assert_eq!(fps.iter().filter(|f| f.is_some()).count(), 1);
    assert_eq!(fps.iter().filter(|f| f.is_none()).count(), 1);
}

fn two_layer() -> Vec<MiterLayer> {
    vec![
        MiterLayer::with_id(0.1, -0.15000000000000002, "Brick", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.2, -0.05000000000000002, "Concrete", "Structural", uuid::Uuid::new_v4()),
    ]
}

#[test]
fn x_crossing_four_walls_two_layers_all_miter() {
    // Four walls meeting at origin, each with 2 layers. Every endpoint
    // wall must get a consistent mitered footprint for both layers.
    let layers = two_layer();
    let geoms = vec![
        JunctionWallGeom {
            axis: vec![(0.0, 0.0), (10.0, 0.0)],
            layers: layers.clone(),
            end: Some(0),
        },
        JunctionWallGeom {
            axis: vec![(0.0, 0.0), (0.0, 10.0)],
            layers: layers.clone(),
            end: Some(0),
        },
        JunctionWallGeom {
            axis: vec![(0.0, 0.0), (-10.0, 0.0)],
            layers: layers.clone(),
            end: Some(0),
        },
        JunctionWallGeom {
            axis: vec![(0.0, 0.0), (0.0, -10.0)],
            layers: layers.clone(),
            end: Some(0),
        },
    ];
    let junction = Junction {
        point: glam::DVec3::ZERO,
        participants: (0..4)
            .map(|i| JunctionParticipant {
                wall_index: i,
                role: JunctionRole::Endpoint(0),
            })
            .collect(),
    };

    let all = mitered_junction_layer_footprints(&junction, &geoms);
    assert_eq!(all.len(), 4);
    for (wi, fps) in all.iter().enumerate() {
        assert_eq!(fps.len(), 2, "wall {wi} should have 2 layer slots");
        for (li, fp) in fps.iter().enumerate() {
            let poly = fp
                .as_ref()
                .unwrap_or_else(|| panic!("wall {wi} layer {li} should miter, got None"));
            assert!(
                poly.len() >= 3,
                "wall {wi} layer {li} footprint too short: {poly:?}"
            );
        }
    }

    // East wall (index 0) end-cap should sit near x = +half_thickness of
    // the outer layer stack (total 0.3 → half 0.15) after N-way miter
    // against N and S — i.e. not left at the axis (x=0).
    let east_outer = all[0][0].as_ref().unwrap();
    let max_abs_x_near_origin = east_outer
        .iter()
        .filter(|(x, y)| x.abs() < 0.5 && y.abs() < 0.5)
        .map(|(x, _)| x.abs())
        .fold(0.0_f64, f64::max);
    assert!(
        max_abs_x_near_origin > 0.05,
        "east wall end should leave the axis after N-way miter, got max |x|={max_abs_x_near_origin}, fp={east_outer:?}"
    );
}

#[test]
fn n_way_collinear_pair_does_not_miter_into_each_other() {
    // Three endpoints at (8,22): horizontal stem + two collinear vertical
    // walls (the example "N-Wege einschalig"). The vertical pair must not
    // receive overlapping L-miters.
    let layers = vec![g(0.2)];
    let geoms = vec![
        JunctionWallGeom {
            axis: vec![(0.0, 22.0), (8.0, 22.0)],
            layers: layers.clone(),
            end: Some(1),
        },
        JunctionWallGeom {
            axis: vec![(8.0, 22.0), (8.0, 28.0)],
            layers: layers.clone(),
            end: Some(0),
        },
        JunctionWallGeom {
            axis: vec![(8.0, 22.0), (8.0, 16.0)],
            layers: layers.clone(),
            end: Some(0),
        },
    ];
    let junction = Junction {
        point: glam::DVec3::new(8.0, 22.0, 0.0),
        participants: (0..3)
            .map(|i| JunctionParticipant {
                wall_index: i,
                role: JunctionRole::Endpoint(if i == 0 { 1 } else { 0 }),
            })
            .collect(),
    };
    let all = mitered_junction_layer_footprints(&junction, &geoms);
    let north = all[1][0].as_ref().expect("north wall should miter against stem only");
    let south = all[2][0].as_ref().expect("south wall should miter against stem only");
    let north_ymin = north.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let south_ymax = south.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    assert!(
        north_ymin > 21.85,
        "north wall must not L-miter through the collinear south wall, ymin={north_ymin} fp={north:?}"
    );
    assert!(
        south_ymax < 22.15,
        "south wall must not L-miter through the collinear north wall, ymax={south_ymax} fp={south:?}"
    );
    let stem = all[0][0].as_ref().expect("stem should still miter");
    assert!(stem.len() >= 3);
}

#[test]
fn t_junction_with_third_wall_two_layers() {
    // Through wall on X, stem from +Y, third wall from NE — all meet at
    // (5,0). Stem and third are endpoints; through is Through.
    let layers = two_layer();
    let geoms = vec![
        JunctionWallGeom {
            axis: vec![(0.0, 0.0), (10.0, 0.0)],
            layers: layers.clone(),
            end: None, // through
        },
        JunctionWallGeom {
            axis: vec![(5.0, 0.0), (5.0, 10.0)],
            layers: layers.clone(),
            end: Some(0),
        },
        JunctionWallGeom {
            axis: vec![(5.0, 0.0), (10.0, 5.0)],
            layers: layers.clone(),
            end: Some(0),
        },
    ];
    let junction = Junction {
        point: glam::DVec3::new(5.0, 0.0, 0.0),
        participants: vec![
            JunctionParticipant {
                wall_index: 0,
                role: JunctionRole::Through(0),
            },
            JunctionParticipant {
                wall_index: 1,
                role: JunctionRole::Endpoint(0),
            },
            JunctionParticipant {
                wall_index: 2,
                role: JunctionRole::Endpoint(0),
            },
        ],
    };

    let all = mitered_junction_layer_footprints(&junction, &geoms);
    assert_eq!(all.len(), 3);

    // Through-wall: no endpoint miter of its own.
    assert!(
        all[0].iter().all(|f| f.is_none()),
        "through wall footprints stay None (rectangular fallback), got {:?}",
        all[0]
    );

    // Stem and third wall: both layers should resolve.
    for wi in [1usize, 2] {
        assert_eq!(all[wi].len(), 2);
        let matched = all[wi].iter().filter(|f| f.is_some()).count();
        assert!(
            matched >= 1,
            "wall {wi} should miter at least one layer at the 3-way junction, got {:?}",
            all[wi]
        );
        for fp in all[wi].iter().flatten() {
            assert!(fp.len() >= 3);
        }
    }
}

#[test]
fn junction_two_wall_l_matches_pairwise() {
    // N-way path with exactly two endpoint walls must agree with pairwise.
    let layers = vec![g(0.2)];
    let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
    let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
    let pairwise = mitered_layer_footprints(
        &axis_a,
        &layers,
        1,
        &axis_b,
        &layers,
        Some(0),
        JoinKind::L,
    );
    let geoms = vec![
        JunctionWallGeom {
            axis: axis_a.clone(),
            layers: layers.clone(),
            end: Some(1),
        },
        JunctionWallGeom {
            axis: axis_b.clone(),
            layers: layers.clone(),
            end: Some(0),
        },
    ];
    let junction = Junction {
        point: glam::DVec3::new(10.0, 0.0, 0.0),
        participants: vec![
            JunctionParticipant {
                wall_index: 0,
                role: JunctionRole::Endpoint(1),
            },
            JunctionParticipant {
                wall_index: 1,
                role: JunctionRole::Endpoint(0),
            },
        ],
    };
    let nway = mitered_junction_layer_footprints(&junction, &geoms);
    let pa = pairwise[0].as_ref().expect("pairwise A");
    let na = nway[0][0].as_ref().expect("nway A");
    assert_eq!(pa.len(), na.len());
    for (p, n) in pa.iter().zip(na.iter()) {
        assert!(close(*p, *n, 1e-6), "mismatch pairwise={p:?} nway={n:?}");
    }
}

/// Build an L-corner test axis pair where wall A arrives from the west
/// ending at the origin, and wall B leaves the origin at `interior_deg`
/// measured as the angle between A's back-direction and B's forward
/// direction (i.e. the physical wedge angle of the corner). 90° matches
/// `l_corner_miter_shares_diagonal_endpoints`'s configuration (rotated).
fn l_corner_axes_at_interior_angle(interior_deg: f64) -> (Vec<(f64, f64)>, Vec<(f64, f64)>) {
    let theta = (180.0 - interior_deg).to_radians();
    let axis_a = vec![(-10.0, 0.0), (0.0, 0.0)];
    let axis_b = vec![(0.0, 0.0), (10.0 * theta.cos(), 10.0 * theta.sin())];
    (axis_a, axis_b)
}

/// Ground-truth for the two candidate corner-boundary pairings at a
/// mitered join, computed *independently* of the pairing-selection logic
/// under test in `miter_end_points`: it only reuses generic 2D line
/// intersection (`intersect_lines_2d`) plus the real per-layer offset
/// boundaries from `layer_contours` (the exact same boundaries
/// production feeds into `miter_one_layer`/`miter_end_points`).
///
/// Returns `(direct1, direct2, cross1, cross2)` where `direct` is the
/// `a_b1`\u{2194}`b_b1` / `a_b2`\u{2194}`b_b2` pairing and `cross` is
/// `a_b1`\u{2194}`b_b2` / `a_b2`\u{2194}`b_b1`. The convention is
/// anchored to the pre-existing, trusted right-angle test
/// `l_corner_miter_shares_diagonal_endpoints`: its expected corners
/// `(10.1,-0.1)`/`(9.9,0.1)` are exactly this `direct` pair for that
/// axis configuration, confirmed below.
fn direct_and_cross_corner_pairs(
    axis_a: &[(f64, f64)],
    axis_b: &[(f64, f64)],
    geom: &[(f64, f64)],
    layer_index: usize,
) -> ((f64, f64), (f64, f64), (f64, f64), (f64, f64)) {
    let contours_a = layer_contours(axis_a, geom);
    let contours_b = layer_contours(axis_b, geom);
    let (a_b1, a_b2) = &contours_a[layer_index];
    let (b_b1, b_b2) = &contours_b[layer_index];
    let a1b1 = intersect_lines_2d(a_b1[0], a_b1[1], b_b1[0], b_b1[1])
        .expect("a1/b1 boundaries must intersect for a non-degenerate corner");
    let a2b2 = intersect_lines_2d(a_b2[0], a_b2[1], b_b2[0], b_b2[1])
        .expect("a2/b2 boundaries must intersect for a non-degenerate corner");
    let a1b2 = intersect_lines_2d(a_b1[0], a_b1[1], b_b2[0], b_b2[1])
        .expect("a1/b2 boundaries must intersect for a non-degenerate corner");
    let a2b1 = intersect_lines_2d(a_b2[0], a_b2[1], b_b1[0], b_b1[1])
        .expect("a2/b1 boundaries must intersect for a non-degenerate corner");
    (a1b1, a2b2, a1b2, a2b1)
}

/// Assert that a mitered layer footprint contains the analytically
/// correct sharp ("direct") corner pairing, and NOT the wrong
/// ("cross"/beveled) one that the historical shortest-segment heuristic
/// picked for acute angles. This is discriminating: it fails on the old
/// buggy pairing selection and passes only once the true continuation of
/// each boundary line is chosen.
fn assert_sharp_miter(
    fp: &[(f64, f64)],
    axis_a: &[(f64, f64)],
    axis_b: &[(f64, f64)],
    geom: &[(f64, f64)],
    layer_index: usize,
) {
    let (direct1, direct2, cross1, cross2) =
        direct_and_cross_corner_pairs(axis_a, axis_b, geom, layer_index);
    let has_direct =
        fp.iter().any(|p| close(*p, direct1, 1e-6)) && fp.iter().any(|p| close(*p, direct2, 1e-6));
    let has_cross =
        fp.iter().any(|p| close(*p, cross1, 1e-6)) && fp.iter().any(|p| close(*p, cross2, 1e-6));
    assert!(
        has_direct && !has_cross,
        "expected the sharp ('direct') miter pairing {direct1:?}/{direct2:?}, but got the \
         beveled ('cross') pairing {cross1:?}/{cross2:?} instead; fp={fp:?}"
    );
}

#[test]
fn acute_30_deg_l_corner_is_mitered_sharp_not_beveled() {
    let (axis_a, axis_b) = l_corner_axes_at_interior_angle(30.0);
    let layers = vec![g(0.2)];
    let fps = mitered_layer_footprints(
        &axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L,
    );
    let fp = fps[0].as_ref().expect("30 degree L-join should miter");
    assert_sharp_miter(fp, &axis_a, &axis_b, &[(0.2, -0.1)], 0);
}

#[test]
fn acute_45_deg_l_corner_is_mitered_sharp_not_beveled() {
    let (axis_a, axis_b) = l_corner_axes_at_interior_angle(45.0);
    let layers = vec![g(0.2)];
    let fps = mitered_layer_footprints(
        &axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L,
    );
    let fp = fps[0].as_ref().expect("45 degree L-join should miter");
    assert_sharp_miter(fp, &axis_a, &axis_b, &[(0.2, -0.1)], 0);

    // The exact expected corner points, verified analytically: A's
    // right-hand (south, y=-0.1) boundary continues into B's right-hand
    // boundary (the true outer/convex spike), while A's left-hand
    // (north, y=+0.1) boundary is pulled back to B's left-hand boundary
    // (the concave/inner side) — i.e. the "direct" (same-handedness)
    // pairing, not the shorter "cross" pairing a naive distance
    // heuristic would pick.
    let outer = (0.1 * (1.0 + std::f64::consts::SQRT_2), -0.1);
    let inner = (-0.1 * (1.0 + std::f64::consts::SQRT_2), 0.1);
    assert!(
        fp.iter().any(|p| close(*p, outer, 1e-6)),
        "expected outer miter spike at {outer:?}, got {fp:?}"
    );
    assert!(
        fp.iter().any(|p| close(*p, inner, 1e-6)),
        "expected inner miter point at {inner:?}, got {fp:?}"
    );
}

#[test]
fn acute_60_deg_l_corner_is_mitered_sharp_not_beveled() {
    let (axis_a, axis_b) = l_corner_axes_at_interior_angle(60.0);
    let layers = vec![g(0.2)];
    let fps = mitered_layer_footprints(
        &axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L,
    );
    let fp = fps[0].as_ref().expect("60 degree L-join should miter");
    assert_sharp_miter(fp, &axis_a, &axis_b, &[(0.2, -0.1)], 0);
}

#[test]
fn right_angle_90_deg_l_corner_regression_unchanged() {
    // Same configuration as `l_corner_miter_shares_diagonal_endpoints`,
    // just re-expressed via the shared helper — must keep producing a
    // sharp miter (this already worked before the fix).
    let (axis_a, axis_b) = l_corner_axes_at_interior_angle(90.0);
    let layers = vec![g(0.2)];
    let fps = mitered_layer_footprints(
        &axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L,
    );
    let fp = fps[0].as_ref().expect("90 degree L-join should miter");
    assert_sharp_miter(fp, &axis_a, &axis_b, &[(0.2, -0.1)], 0);
}

#[test]
fn obtuse_120_deg_l_corner_regression_unchanged() {
    let (axis_a, axis_b) = l_corner_axes_at_interior_angle(120.0);
    let layers = vec![g(0.2)];
    let fps = mitered_layer_footprints(
        &axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L,
    );
    let fp = fps[0].as_ref().expect("120 degree L-join should miter");
    assert_sharp_miter(fp, &axis_a, &axis_b, &[(0.2, -0.1)], 0);
}

#[test]
fn acute_45_deg_l_corner_multi_layer_each_layer_sharp() {
    // Two matching layers on both walls at a 45° corner: each layer
    // must independently form its own sharp miter, not just the outer.
    let (axis_a, axis_b) = l_corner_axes_at_interior_angle(45.0);
    let layers = two_layer();
    let fps = mitered_layer_footprints(
        &axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L,
    );
    let geom: Vec<(f64, f64)> = layers.iter().map(MiterLayer::as_geom).collect();
    for (li, fp) in fps.iter().enumerate() {
        let fp = fp.as_ref().unwrap_or_else(|| panic!("layer {li} should miter"));
        assert_sharp_miter(fp, &axis_a, &axis_b, &geom, li);
    }
}

#[test]
fn acute_45_deg_n_way_junction_endpoint_is_sharp_not_beveled() {
    // 3-way junction with one acute (45°) sub-angle between walls 0 and 1;
    // wall 2 is far away at 90° from wall 0 so it doesn't interfere.
    // Reuses the same corner/orientation as the pairwise 45° test so the
    // expected sharp-miter geometry between walls 0 and 1 is identical.
    let (axis_a, axis_b) = l_corner_axes_at_interior_angle(45.0);
    let layers = vec![g(0.2)];
    let junction = Junction {
        point: glam::DVec3::new(0.0, 0.0, 0.0),
        participants: vec![
            JunctionParticipant { wall_index: 0, role: JunctionRole::Endpoint(1) },
            JunctionParticipant { wall_index: 1, role: JunctionRole::Endpoint(0) },
            JunctionParticipant { wall_index: 2, role: JunctionRole::Endpoint(0) },
        ],
    };
    let walls = vec![
        JunctionWallGeom { axis: axis_a.clone(), layers: layers.clone(), end: Some(1) },
        JunctionWallGeom { axis: axis_b.clone(), layers: layers.clone(), end: Some(0) },
        // Far third wall, straight up — keeps wall 0/1 as pure angular
        // neighbors of each other on one side.
        JunctionWallGeom { axis: vec![(0.0, 0.0), (0.0, -10.0)], layers: layers.clone(), end: Some(0) },
    ];
    let res = mitered_junction_layer_footprints(&junction, &walls);
    let fp0 = res[0][0].as_ref().expect("wall 0 layer should miter against wall 1");

    // Wall 2 sits at a *different* 90° angle from wall 0 (south), so it
    // legitimately claims wall 0's other (north) boundary side in this
    // 3-way arrangement — only the boundary side that actually faces
    // wall 1 is meant to carry the acute miter under test here. Compute
    // the ground-truth direct/cross candidates for the wall0/wall1 pair
    // exactly like the pairwise tests above, and assert the correct
    // ("direct") corner is present while the wrong ("cross"/beveled)
    // one that the historical shortest-segment heuristic would have
    // picked for this acute angle is absent — genuinely exercising the
    // acute-angle path inside `mitered_junction_layer_footprints`
    // itself, not just the underlying pairwise helper.
    let (direct1, _direct2, cross1, _cross2) =
        direct_and_cross_corner_pairs(&axis_a, &axis_b, &[(0.2, -0.1)], 0);
    assert!(
        fp0.iter().any(|p| close(*p, direct1, 1e-6)),
        "expected the sharp miter corner {direct1:?} facing wall 1 in the n-way \
         footprint, got {fp0:?}"
    );
    assert!(
        !fp0.iter().any(|p| close(*p, cross1, 1e-6)),
        "n-way footprint contains the beveled/cross corner {cross1:?} instead of the \
         sharp one, got {fp0:?}"
    );

    // And it must agree with the plain pairwise helper on that same
    // corner, confirming the N-way path reuses the corrected geometry
    // rather than coincidentally landing on it.
    let pairwise = mitered_layer_footprints(
        &axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L,
    );
    let pfp0 = pairwise[0].as_ref().expect("pairwise wall 0 layer should miter");
    assert!(
        pfp0.iter().any(|p| close(*p, direct1, 1e-6)),
        "sanity: pairwise result should also contain {direct1:?}, got {pfp0:?}"
    );
}

#[test]
fn junction_mixed_layer_stacks() {
    // 3-way junction at (0,0).
    // Wall 0: (0,0) to (10,0). Layers: [Brick, Concrete]
    // Wall 1: (0,0) to (0,10). Layers: [Brick, Concrete] (identical stack)
    // Wall 2: (0,0) to (-10,0). Layers: [Concrete] (different stack)

    let l_brick = MiterLayer::with_id(0.1, -0.15, "Brick", "Finish", uuid::Uuid::new_v4());
    let l_concrete = MiterLayer::with_id(0.2, -0.05, "Concrete", "Structural", uuid::Uuid::new_v4());
    let l_concrete_alone = MiterLayer::with_id(0.2, -0.1, "Concrete", "Structural", uuid::Uuid::new_v4());

    let junction = Junction {
        point: glam::DVec3::new(0.0, 0.0, 0.0),
        participants: vec![
            JunctionParticipant { wall_index: 0, role: JunctionRole::Endpoint(0) },
            JunctionParticipant { wall_index: 1, role: JunctionRole::Endpoint(0) },
            JunctionParticipant { wall_index: 2, role: JunctionRole::Endpoint(0) },
        ],
    };
    let walls = vec![
        JunctionWallGeom { axis: vec![(0.0, 0.0), (10.0, 0.0)], layers: vec![l_brick.clone(), l_concrete.clone()], end: Some(0) },
        JunctionWallGeom { axis: vec![(0.0, 0.0), (0.0, 10.0)], layers: vec![l_brick.clone(), l_concrete.clone()], end: Some(0) },
        JunctionWallGeom { axis: vec![(0.0, 0.0), (-10.0, 0.0)], layers: vec![l_concrete_alone], end: Some(0) },
    ];

    let res = mitered_junction_layer_footprints(&junction, &walls);
    // Wall 0 layers: [Brick, Concrete].
    // Neighbor W1: [Brick, Concrete]. Both should match.
    // Neighbor W2: [Concrete]. Only Concrete should match.

    // Wall 0 layer 0 (Brick):
    //   - matches W1 (Brick)
    //   - no match W2
    //   => Should miter against W1.
    assert!(res[0][0].is_some(), "W0 layer 0 (Brick) should miter against W1");

    // Wall 0 layer 1 (Concrete):
    //   - matches W1 (Concrete)
    //   - matches W2 (Concrete)
    //   => Should miter against both.
    assert!(res[0][1].is_some(), "W0 layer 1 (Concrete) should miter against W1 and W2");

    // Wall 2 layer 0 (Concrete):
    //   - matches W1 (Concrete)
    //   - matches W0 (Concrete)
    assert!(res[2][0].is_some(), "W2 layer 0 (Concrete) should miter against W1 and W0");

    // Now test a layer that matches NO neighbor.
    let l_brick_g = MiterLayer::with_id(0.1, -0.075, "Brick", "Finish", uuid::Uuid::new_v4());
    let l_glass = MiterLayer::with_id(0.05, 0.025, "Glass", "Finish", uuid::Uuid::new_v4());
    let l_brick_alone = MiterLayer::with_id(0.1, -0.05, "Brick", "Finish", uuid::Uuid::new_v4());
    let walls_mixed = vec![
        JunctionWallGeom { axis: vec![(0.0, 0.0), (10.0, 0.0)], layers: vec![l_brick_g, l_glass], end: Some(0) },
        JunctionWallGeom { axis: vec![(0.0, 0.0), (0.0, 10.0)], layers: vec![l_brick_alone.clone()], end: Some(0) },
        JunctionWallGeom { axis: vec![(0.0, 0.0), (-10.0, 0.0)], layers: vec![l_brick_alone], end: Some(0) },
    ];
    let res_mixed = mitered_junction_layer_footprints(&junction, &walls_mixed);
    // Wall 0 layer 1 (Glass) matches NO neighbor.
    assert!(res_mixed[0][1].is_none(), "Wall 0 layer 1 (Glass) should fall back when no neighbor matches");
}

#[test]
fn nway_t_junction_unmatched_stem_layer_extends_to_through_outer_face() {
    // N-way (3+ walls) T-junction: a through-wall plus two endpoint
    // (stem) walls. One stem wall carries an extra "orphan" layer with
    // no material match in the through-wall's stack — it must extend to
    // the through-wall's outer face (parity with the pairwise
    // `t_junction_layer_footprints` path), not fall back to `None`.
    //
    // Through wall: (-10,0) to (10,0), layers [Brick(0.2), Concrete(0.1)]
    //   stacked from y=-0.15 to y=+0.15 (outer face on +y side is +0.15).
    // Stem wall 1 (endpoint): (0,0) to (0,10), layers [Brick, Orphan].
    // Stem wall 2 (endpoint): (0,0) to (5,-8), layers [Brick] only, at a
    //   distinct angle so this is a genuine 3-way (not 2-wall) junction.
    let through_layers = vec![
        MiterLayer::with_id(0.2, -0.15, "Brick", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.1, 0.05, "Concrete", "Structural", uuid::Uuid::new_v4()),
    ];
    let stem1_layers = vec![
        MiterLayer::with_id(0.2, -0.125, "Brick", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.05, 0.075, "Orphan", "Other", uuid::Uuid::new_v4()),
    ];
    let stem2_layers = vec![MiterLayer::with_id(0.2, -0.1, "Brick", "Finish", uuid::Uuid::new_v4())];

    let junction = Junction {
        point: glam::DVec3::new(0.0, 0.0, 0.0),
        participants: vec![
            JunctionParticipant { wall_index: 0, role: JunctionRole::Through(0) },
            JunctionParticipant { wall_index: 1, role: JunctionRole::Endpoint(0) },
            JunctionParticipant { wall_index: 2, role: JunctionRole::Endpoint(0) },
        ],
    };
    let walls = vec![
        JunctionWallGeom {
            axis: vec![(-10.0, 0.0), (10.0, 0.0)],
            layers: through_layers,
            end: None,
        },
        JunctionWallGeom {
            axis: vec![(0.0, 0.0), (0.0, 10.0)],
            layers: stem1_layers,
            end: Some(0),
        },
        JunctionWallGeom {
            axis: vec![(0.0, 0.0), (5.0, -8.0)],
            layers: stem2_layers,
            end: Some(0),
        },
    ];

    let res = mitered_junction_layer_footprints(&junction, &walls);
    assert_eq!(res.len(), 3);
    let fp_orphan = res[1][1]
        .as_ref()
        .expect("orphan layer must extend to through-wall outer face, not fall back to None");

    // Outer face of the through-wall stack on the +y approach side is
    // y=+0.15 (outer edge of the concrete layer).
    let join_ys: Vec<f64> = fp_orphan
        .iter()
        .filter(|(_, y)| y.abs() < 0.5)
        .map(|(_, y)| *y)
        .collect();
    assert!(
        !join_ys.is_empty() && join_ys.iter().all(|y| (*y - 0.15).abs() < 1e-5),
        "orphan layer should land on through-wall outer face y=+0.15, got {join_ys:?} in {fp_orphan:?}"
    );
}

// ---- Step 2: JunctionOverride integration -----------------------------

fn lref_at(material: &str, index: usize) -> LayerRef {
    LayerRef {
        material_id: material.to_string(),
        role_tag: None,
        index,
    layer_id: None,
    }
}

fn lref(material: &str) -> LayerRef {
    lref_at(material, 0)
}

#[test]
fn override_none_matches_automatic_l_corner_regression() {
    // No override at all: byte-for-byte identical to the plain helper.
    let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
    let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
    let layers = vec![g(0.2)];
    let refs = vec![lref("brick")];

    let automatic =
        mitered_layer_footprints(&axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L);
    let overridden = mitered_layer_footprints_with_override(
        &axis_a, &layers, &refs, 1, &axis_b, &layers, Some(0), JoinKind::L, None,
    );
    assert_eq!(automatic, overridden);
}

#[test]
fn override_none_matches_automatic_t_junction_regression() {
    let stem = vec![(5.0, 0.0), (5.0, 8.0)];
    let through = vec![(0.0, 0.0), (10.0, 0.0)];
    let layers = vec![g(0.2)];
    let refs = vec![lref("brick")];

    let automatic =
        mitered_layer_footprints(&stem, &layers, 0, &through, &layers, None, JoinKind::T);
    let overridden = mitered_layer_footprints_with_override(
        &stem, &layers, &refs, 0, &through, &layers, None, JoinKind::T, None,
    );
    assert_eq!(automatic, overridden);
}

#[test]
fn override_none_matches_automatic_n_way_junction_regression() {
    let l_brick = MiterLayer::with_id(0.1, -0.15, "Brick", "Finish", uuid::Uuid::new_v4());
    let l_concrete = MiterLayer::with_id(0.2, -0.05, "Concrete", "Structural", uuid::Uuid::new_v4());
    let l_concrete_alone = MiterLayer::with_id(0.2, -0.1, "Concrete", "Structural", uuid::Uuid::new_v4());
    let junction = Junction {
        point: glam::DVec3::new(0.0, 0.0, 0.0),
        participants: vec![
            JunctionParticipant { wall_index: 0, role: JunctionRole::Endpoint(0) },
            JunctionParticipant { wall_index: 1, role: JunctionRole::Endpoint(0) },
            JunctionParticipant { wall_index: 2, role: JunctionRole::Endpoint(0) },
        ],
    };
    let walls = vec![
        JunctionWallGeom { axis: vec![(0.0, 0.0), (10.0, 0.0)], layers: vec![l_brick.clone(), l_concrete.clone()], end: Some(0) },
        JunctionWallGeom { axis: vec![(0.0, 0.0), (0.0, 10.0)], layers: vec![l_brick.clone(), l_concrete.clone()], end: Some(0) },
        JunctionWallGeom { axis: vec![(0.0, 0.0), (-10.0, 0.0)], layers: vec![l_concrete_alone], end: Some(0) },
    ];
    let layer_refs = vec![
        vec![lref_at("Brick", 0), lref_at("Concrete", 1)],
        vec![lref_at("Brick", 0), lref_at("Concrete", 1)],
        vec![lref_at("Concrete", 0)],
    ];
    let overrides: Vec<Option<JunctionOverride>> = vec![None, None, None];

    let automatic = mitered_junction_layer_footprints(&junction, &walls);
    let overridden = mitered_junction_layer_footprints_with_overrides(
        &junction, &walls, &layer_refs, &overrides,
    );
    assert_eq!(automatic, overridden);
}

#[test]
fn junction_override_default_outer_face_changes_t_join_from_butt() {
    // T-junction against a two-layer through wall; the stem's single
    // layer matches the *inner* through layer ("core", y in [-0.2, 0]),
    // so the automatic butt lands on the inner boundary (y = 0.0). A
    // `default_style: OuterFace` override must instead push it to the
    // true outer face of the whole through-wall stack (y = +0.2).
    let stem = vec![(5.0, 0.0), (5.0, 8.0)];
    let through = vec![(0.0, 0.0), (10.0, 0.0)];
    let stem_layers = vec![MiterLayer::with_id(0.2, -0.1, "core", "Structural", uuid::Uuid::new_v4())];
    let through_layers = vec![
        MiterLayer::with_id(0.2, -0.2, "core", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.2, 0.0, "other", "Finish", uuid::Uuid::new_v4()),
    ];
    let refs = vec![lref("core")];

    let automatic = mitered_layer_footprints(
        &stem, &stem_layers, 0, &through, &through_layers, None, JoinKind::T,
    );
    let fp_auto = automatic[0].as_ref().expect("automatic butt should succeed");
    assert!(
        fp_auto.iter().any(|p| close(*p, (4.9, 0.0), 1e-6)),
        "expected automatic butt at the inner boundary y=0.0, got {fp_auto:?}"
    );

    let ov = JunctionOverride {
        default_style: Some(JoinOverrideStyle::OuterFace),
        layer_pairs: Vec::new(),
        layer_gaps: Vec::new(),
    };
    let overridden = mitered_layer_footprints_with_override(
        &stem, &stem_layers, &refs, 0, &through, &through_layers, None, JoinKind::T,
        Some(&ov),
    );
    let fp_over = overridden[0].as_ref().expect("outer-face override should succeed");
    assert!(
        fp_over.iter().any(|p| close(*p, (4.9, 0.2), 1e-5)),
        "expected outer-face butt at y=0.2, got {fp_over:?}"
    );
    assert!(
        !fp_over.iter().any(|p| close(*p, (4.9, 0.0), 1e-5)),
        "override result should not match the automatic near-face butt, got {fp_over:?}"
    );
}

#[test]
fn junction_override_layer_pair_no_extend_keeps_one_layer_unjoined() {
    // L-corner with two layers; a `layer_pairs` NoExtend override on the
    // outer ("brick") layer only must leave that layer at its original,
    // un-joined boundary (endpoint still at x=10) while the inner layer
    // still auto-miters normally (endpoint pushed past x=10).
    let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
    let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
    let layers = vec![
        MiterLayer::with_id(0.1, -0.1, "concrete", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.1, 0.0, "brick", "Finish", uuid::Uuid::new_v4()),
    ];
    let refs = vec![lref_at("concrete", 0), lref_at("brick", 1)];

    let automatic =
        mitered_layer_footprints(&axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L);
    assert!(automatic[0].is_some(), "sanity: inner layer should auto-miter");
    assert!(automatic[1].is_some(), "sanity: outer layer should auto-miter");

    let ov = JunctionOverride {
        default_style: None,
        layer_pairs: vec![LayerPairOverride {
            layer_a: lref_at("brick", 1),
            layer_b: None,
            style: JoinOverrideStyle::NoExtend,
        }],
        layer_gaps: Vec::new(),
    };
    let overridden = mitered_layer_footprints_with_override(
        &axis_a, &layers, &refs, 1, &axis_b, &layers, Some(0), JoinKind::L, Some(&ov),
    );
    assert_eq!(
        overridden[0], automatic[0],
        "layer without a matching override must auto-resolve unchanged"
    );
    let fp_brick = overridden[1]
        .as_ref()
        .expect("NoExtend must still yield the un-joined original footprint");
    assert_ne!(
        overridden[1], automatic[1],
        "NoExtend must differ from the automatic miter result"
    );
    assert!(
        fp_brick.iter().all(|(x, _)| *x <= 10.0 + 1e-9),
        "NoExtend brick layer must stay at its original (un-extended) boundary, got {fp_brick:?}"
    );
}

/// Regression for the reported bug: a wall with two layers that use the
/// *same* material (e.g. two plaster layers) must let a `LayerPairOverride`
/// target only one specific occurrence, identified by its `index`, not
/// both indistinguishably (which would happen if only `material_id`
/// were compared).
#[test]
fn junction_override_layer_pair_targets_only_matching_index_for_duplicate_material() {
    let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
    let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
    let layers = vec![
        MiterLayer::with_id(0.1, -0.1, "plaster", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.1, 0.0, "plaster", "Finish", uuid::Uuid::new_v4()),
    ];
    // Both layers share the same material id; only their `index` differs.
    let refs = vec![lref_at("plaster", 0), lref_at("plaster", 1)];

    let automatic =
        mitered_layer_footprints(&axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L);

    // Override targets only the *second* "plaster" occurrence (index 1).
    let ov = JunctionOverride {
        default_style: None,
        layer_pairs: vec![LayerPairOverride {
            layer_a: lref_at("plaster", 1),
            layer_b: None,
            style: JoinOverrideStyle::NoExtend,
        }],
        layer_gaps: Vec::new(),
    };
    let overridden = mitered_layer_footprints_with_override(
        &axis_a, &layers, &refs, 1, &axis_b, &layers, Some(0), JoinKind::L, Some(&ov),
    );

    assert_eq!(
        overridden[0], automatic[0],
        "index 0's plaster layer must auto-resolve unchanged even though it shares a \
         material id with the overridden layer"
    );
    assert_ne!(
        overridden[1], automatic[1],
        "index 1's plaster layer must be affected by its own layer-pair override"
    );
}

#[test]
fn junction_override_layer_pair_wins_over_default_style() {
    // Both a default_style and a layer-specific override are set; the
    // layer-pair override must win for its layer, while default_style
    // (Miter, same as automatic L result here) applies elsewhere.
    let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
    let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
    let layers = vec![
        MiterLayer::with_id(0.1, -0.1, "concrete", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.1, 0.0, "brick", "Finish", uuid::Uuid::new_v4()),
    ];
    let refs = vec![lref_at("concrete", 0), lref_at("brick", 1)];

    let ov = JunctionOverride {
        default_style: Some(JoinOverrideStyle::Miter),
        layer_pairs: vec![LayerPairOverride {
            layer_a: lref_at("brick", 1),
            layer_b: None,
            style: JoinOverrideStyle::NoExtend,
        }],
        layer_gaps: Vec::new(),
    };
    let overridden = mitered_layer_footprints_with_override(
        &axis_a, &layers, &refs, 1, &axis_b, &layers, Some(0), JoinKind::L, Some(&ov),
    );
    let automatic =
        mitered_layer_footprints(&axis_a, &layers, 1, &axis_b, &layers, Some(0), JoinKind::L);
    // Concrete layer: default_style Miter == automatic miter result.
    assert_eq!(overridden[0], automatic[0]);
    // Brick layer: layer-pair NoExtend wins over default_style Miter,
    // keeping the layer at its original (un-extended) boundary.
    assert_ne!(overridden[1], automatic[1]);
    let fp_brick = overridden[1]
        .as_ref()
        .expect("NoExtend must still yield the un-joined original footprint");
    assert!(fp_brick.iter().all(|(x, _)| *x <= 10.0 + 1e-9));
}

#[test]
fn near_face_override_matches_automatic_t_butt() {
    let stem = vec![(5.0, 0.0), (5.0, 8.0)];
    let through = vec![(0.0, 0.0), (10.0, 0.0)];
    let stem_layers = vec![MiterLayer::with_id(0.2, -0.1, "core", "Structural", uuid::Uuid::new_v4())];
    let through_layers = vec![
        MiterLayer::with_id(0.2, -0.2, "core", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.2, 0.0, "other", "Structural", uuid::Uuid::new_v4()),
    ];
    let refs = vec![lref("core")];
    let automatic = mitered_layer_footprints(
        &stem, &stem_layers, 0, &through, &through_layers, None, JoinKind::T,
    );
    let ov = JunctionOverride {
        default_style: Some(JoinOverrideStyle::NearFace),
        layer_pairs: Vec::new(),
        layer_gaps: Vec::new(),
    };
    let overridden = mitered_layer_footprints_with_override(
        &stem, &stem_layers, &refs, 0, &through, &through_layers, None, JoinKind::T,
        Some(&ov),
    );
    assert_eq!(overridden[0], automatic[0]);
}

#[test]
fn far_face_override_extends_past_matched_near_face() {
    let stem = vec![(5.0, 0.0), (5.0, 8.0)];
    let through = vec![(0.0, 0.0), (10.0, 0.0)];
    let stem_layers = vec![MiterLayer::with_id(0.2, -0.1, "core", "Structural", uuid::Uuid::new_v4())];
    let through_layers = vec![
        MiterLayer::with_id(0.2, -0.2, "core", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.2, 0.0, "other", "Structural", uuid::Uuid::new_v4()),
    ];
    let refs = vec![lref("core")];
    let ov = JunctionOverride {
        default_style: Some(JoinOverrideStyle::FarFace),
        layer_pairs: Vec::new(),
        layer_gaps: Vec::new(),
    };
    let overridden = mitered_layer_footprints_with_override(
        &stem, &stem_layers, &refs, 0, &through, &through_layers, None, JoinKind::T,
        Some(&ov),
    );
    let fp = overridden[0].as_ref().expect("far-face override");
    assert!(
        fp.iter().any(|p| close(*p, (4.9, -0.2), 1e-5) || close(*p, (5.1, -0.2), 1e-5)
            || p.1 < -0.05),
        "far face of matched core is y=-0.2, got {fp:?}"
    );
}

#[test]
fn near_face_override_uses_explicit_target_layer_not_same_material() {
    // Stem "core" would auto-pair to through "core" (y in [-0.2, 0]).
    // An explicit layer_b pointing at "other" (y in [0, 0.2]) must NearFace
    // that layer (approach y=+0.2), not the same-material core (y=0).
    let stem = vec![(5.0, 0.0), (5.0, 8.0)];
    let through = vec![(0.0, 0.0), (10.0, 0.0)];
    let stem_layers = vec![MiterLayer::with_id(0.2, -0.1, "core", "Structural", uuid::Uuid::new_v4())];
    let through_layers = vec![
        MiterLayer::with_id(0.2, -0.2, "core", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.2, 0.0, "other", "Finish", uuid::Uuid::new_v4()),
    ];
    let refs = vec![lref("core")];
    let ov = JunctionOverride {
        default_style: None,
        layer_pairs: vec![LayerPairOverride {
            layer_a: lref("core"),
            layer_b: Some(lref_at("other", 1)),
            style: JoinOverrideStyle::NearFace,
        }],
        layer_gaps: Vec::new(),
    };
    let overridden = mitered_layer_footprints_with_override(
        &stem, &stem_layers, &refs, 0, &through, &through_layers, None, JoinKind::T,
        Some(&ov),
    );
    let fp = overridden[0]
        .as_ref()
        .expect("near-face to explicit other layer");
    assert!(
        fp.iter().any(|p| close(*p, (4.9, 0.2), 1e-5) || close(*p, (5.1, 0.2), 1e-5)),
        "NearFace must use chosen through layer (y=+0.2), not same-material core (y=0), got {fp:?}"
    );
    assert!(
        !fp.iter().any(|p| close(*p, (4.9, 0.0), 1e-5) || close(*p, (5.1, 0.0), 1e-5)),
        "must not land on same-material core near face, got {fp:?}"
    );
}

#[test]
fn unmatched_stem_notches_through_layers() {
    let through = vec![(0.0, 0.0), (10.0, 0.0)];
    let stem = vec![(5.0, 0.0), (5.0, 8.0)];
    let through_layers = vec![
        MiterLayer::with_id(0.2, -0.1, "core", "Structural", uuid::Uuid::new_v4()),
    ];
    let stem_layers = vec![
        MiterLayer::with_id(0.2, -0.1, "core", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.2, 0.1, "finish", "Finish", uuid::Uuid::new_v4()),
    ];
    let cut = through_wall_cutout_footprints(&through, &through_layers, &stem, &stem_layers, 0);
    assert!(
        cut[0].is_none(),
        "through core must stay rectangular, got {cut:?}"
    );
}

#[test]
fn through_cutout_uses_full_stem_envelope_on_every_layer() {
    let through = vec![(0.0, 0.0), (10.0, 0.0)];
    let stem = vec![(5.0, 0.0), (5.0, 8.0)];
    let through_layers = vec![
        MiterLayer::with_id(0.015, -0.195, "putz", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.24, -0.18, "mw", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.12, 0.06, "ins", "Insulation", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.015, 0.18, "putz2", "Finish", uuid::Uuid::new_v4()),
    ];
    let stem_layers = through_layers.clone();
    let cut = through_wall_cutout_footprints(&through, &through_layers, &stem, &stem_layers, 0);
    assert_eq!(cut.len(), 4);
    assert!(cut[0].is_none(), "far-side putz must stay rectangular, got {cut:?}");
    assert!(cut[1].is_none(), "through core must stay rectangular, got {cut:?}");
    assert!(
        cut[2].is_some(),
        "approach insulation must be notched, got {cut:?}"
    );
    assert!(
        cut[3].is_some(),
        "approach putz should receive a stem-core pocket, got {cut:?}"
    );
    let fp = cut[3].as_ref().unwrap();
    let rings = split_footprint_rings(fp);
    assert_eq!(rings.len(), 2, "through-cut must be two remainders, got {fp:?}");
    // Same-material putz: cut edges are miters (inner/outer X differ).
    let mitered_edge = rings.iter().any(|r| {
        r.windows(2).any(|w| {
            (w[0].1 - 0.195).abs() < 0.02
                && (w[1].1 - 0.18).abs() < 0.02
                && (w[0].0 - w[1].0).abs() > 0.005
        }) || r.windows(2).any(|w| {
            (w[0].1 - 0.18).abs() < 0.02
                && (w[1].1 - 0.195).abs() < 0.02
                && (w[0].0 - w[1].0).abs() > 0.005
        })
    });
    assert!(
        mitered_edge,
        "approach putz pocket should miter with stem putz, got {rings:?}"
    );
    let in_ring = |ring: &[(f64, f64)], p: (f64, f64)| -> bool {
        let mut inside = false;
        let n = ring.len();
        let mut j = n - 1;
        for i in 0..n {
            let (xi, yi) = ring[i];
            let (xj, yj) = ring[j];
            if ((yi > p.1) != (yj > p.1))
                && (p.0 < (xj - xi) * (p.1 - yi) / (yj - yi + 1e-30) + xi)
            {
                inside = !inside;
            }
            j = i;
        }
        inside
    };
    let overlap = (5.0, 0.187);
    assert!(
        rings.iter().all(|r| !in_ring(r, overlap)),
        "approach putz must be open at the stem core, got {rings:?}"
    );
}

#[test]
fn manual_layer_gap_notches_through_core() {
    let through = vec![(0.0, 0.0), (10.0, 0.0)];
    let stem = vec![(5.0, 0.0), (5.0, 8.0)];
    let through_layers = vec![
        MiterLayer::with_id(0.2, -0.1, "core", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.2, 0.1, "ins", "Insulation", uuid::Uuid::new_v4()),
    ];
    let stem_layers = through_layers.clone();
    let auto = through_wall_cutout_footprints(&through, &through_layers, &stem, &stem_layers, 0);
    assert!(auto[0].is_none(), "core stays rectangular without a gap");
    let gap = LayerGapOverride {
        layer: LayerRef {
            material_id: "core".into(),
            role_tag: None,
            index: 0,
            layer_id: Some(through_layers[0].layer_id),
        },
        from: LayerRef {
            material_id: "core".into(),
            role_tag: None,
            index: 0,
            layer_id: Some(stem_layers[0].layer_id),
        },
        to: LayerRef {
            material_id: "ins".into(),
            role_tag: None,
            index: 1,
            layer_id: Some(stem_layers[1].layer_id),
        },
    };
    let cut = through_wall_cutout_footprints_with_gaps(
        &through,
        &through_layers,
        &stem,
        &stem_layers,
        0,
        &[],
        &[],
        &[gap],
    );
    assert!(
        cut[0].is_some(),
        "manual gap must interrupt the through core, got {cut:?}"
    );
}

#[test]
fn through_insulation_notched_by_core_not_full_envelope_on_3_to_4() {
    let through = vec![(0.0, 0.0), (10.0, 0.0)];
    let stem = vec![(5.0, 0.0), (5.0, 8.0)];
    let through_layers = vec![
        MiterLayer::with_id(0.015, -0.195, "putz", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.24, -0.18, "mw", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.12, 0.06, "ins", "Insulation", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.015, 0.18, "putz2", "Finish", uuid::Uuid::new_v4()),
    ];
    let stem_layers = vec![
        MiterLayer::with_id(0.015, -0.195, "putz", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.36, -0.18, "mw", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.015, 0.18, "putz2", "Finish", uuid::Uuid::new_v4()),
    ];
    let cut = through_wall_cutout_footprints(&through, &through_layers, &stem, &stem_layers, 0);
    let ins = cut[2].as_ref().expect("through insulation must be notched");
    let rings = split_footprint_rings(ins);
    assert_eq!(rings.len(), 2, "insulation cut must be two remainders, got {ins:?}");
    let mut xs: Vec<f64> = rings
        .iter()
        .flat_map(|r| r.iter().map(|p| p.0))
        .collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let left_inner = rings[0].iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max)
        .min(rings[1].iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max));
    let right_inner = rings[0].iter().map(|p| p.0).fold(f64::INFINITY, f64::min)
        .max(rings[1].iter().map(|p| p.0).fold(f64::INFINITY, f64::min));
    let gap = (right_inner - left_inner).abs();
    assert!(
        gap < 0.38,
        "insulation pocket must be stem-core width, not full stem, gap={gap} rings={rings:?}"
    );
    assert!(
        gap > 0.30,
        "insulation pocket should still clear the stem core, gap={gap} rings={rings:?}"
    );
    let butt = rings.iter().any(|r| {
        r.windows(2).any(|w| {
            (w[0].0 - w[1].0).abs() < 0.002
                && (w[0].1 - w[1].1).abs() > 0.05
        })
    });
    assert!(
        butt,
        "unmatched insulation should butt (vertical cut), got {rings:?}"
    );
}

#[test]
fn through_plaster_hatch_tessellation_leaves_stem_pocket() {
    let through = vec![(0.0, 0.0), (10.0, 0.0)];
    let stem = vec![(5.0, 0.0), (5.0, 8.0)];
    let through_layers = vec![
        MiterLayer::with_id(0.015, -0.195, "putz", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.24, -0.18, "mw", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.12, 0.06, "ins", "Insulation", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.015, 0.18, "putz", "Finish", uuid::Uuid::new_v4()),
    ];
    let stem_layers = vec![
        MiterLayer::with_id(0.015, -0.195, "putz", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.36, -0.18, "mw", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.015, 0.18, "putz", "Finish", uuid::Uuid::new_v4()),
    ];
    let cut = through_wall_cutout_footprints(&through, &through_layers, &stem, &stem_layers, 0);
    let plaster = cut[3]
        .as_ref()
        .or(cut[0].as_ref())
        .expect("approach plaster must be notched");
    let rings = split_footprint_rings(plaster);
    let closed: Vec<Vec<[f64; 2]>> = rings
        .iter()
        .map(|r| {
            let mut pts: Vec<[f64; 2]> = r.iter().map(|&(x, y)| [x, y]).collect();
            if pts.len() >= 3 {
                let first = pts[0];
                let last = *pts.last().unwrap();
                if (first[0] - last[0]).abs() > 1e-9 || (first[1] - last[1]).abs() > 1e-9 {
                    pts.push(first);
                }
            }
            pts
        })
        .collect();
    let (points, triangles) = cadkernel::geom2d::triangulate_rings(&closed);
    let pocket = [5.0, 0.187];
    let in_tri = |a: [f64; 2], b: [f64; 2], c: [f64; 2], p: [f64; 2]| {
        let s = (a[1] * c[0] - a[0] * c[1] + (c[1] - a[1]) * p[0] + (a[0] - c[0]) * p[1])
            .signum();
        let t = (a[0] * b[1] - a[1] * b[0] + (a[1] - b[1]) * p[0] + (b[0] - a[0]) * p[1])
            .signum();
        let area2 = -b[1] * c[0] + a[1] * (c[0] - b[0]) + a[0] * (b[1] - c[1]) + b[0] * c[1];
        if area2.abs() < 1e-18 {
            return false;
        }
        let bary_a = (b[1] * c[0] - b[0] * c[1] + (c[1] - b[1]) * p[0] + (b[0] - c[0]) * p[1])
            / area2;
        let bary_b = (a[0] * c[1] - a[1] * c[0] + (a[1] - c[1]) * p[0] + (c[0] - a[0]) * p[1])
            / area2;
        let bary_c = 1.0 - bary_a - bary_b;
        bary_a >= -1e-6 && bary_b >= -1e-6 && bary_c >= -1e-6 && s * t >= 0.0
    };
    let filled = triangles.iter().any(|&[i, j, k]| {
        in_tri(points[i], points[j], points[k], pocket)
    });
    assert!(
        !filled,
        "hatch tessellation must not fill the stem pocket, triangles={triangles:?} pts={points:?}"
    );
}

fn insulated_masonry_layers() -> Vec<MiterLayer> {
    vec![
        MiterLayer::with_id(0.015, -0.1725, "Putz", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.175, -0.1575, "Mauerwerk", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.14, 0.0175, "Daemmung", "Insulation", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.015, 0.1575, "Putz", "Finish", uuid::Uuid::new_v4()),
    ]
}

#[test]
fn l_join_identical_insulated_stack_miters_inner_plaster() {
    let axis_a = vec![(0.0, 0.0), (10.0, 0.0)];
    let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
    let layers_a = insulated_masonry_layers();
    let layers_b = insulated_masonry_layers();
    let pairing = match_layer_indices(&layers_a, &layers_b);
    assert_eq!(
        pairing,
        vec![Some(0), Some(1), Some(2), Some(3)],
        "identical stacks must pair same-index layers, got {pairing:?}"
    );
    let fps = mitered_layer_footprints(
        &axis_a,
        &layers_a,
        1,
        &axis_b,
        &layers_b,
        Some(0),
        JoinKind::L,
    );
    assert_eq!(fps.len(), 4);
    for (i, fp) in fps.iter().enumerate() {
        assert!(
            fp.as_ref().is_some_and(|p| p.len() >= 3),
            "layer {i} should miter, pairing={pairing:?} fps={fps:?}"
        );
    }
    let inner = fps[0].as_ref().unwrap();
    // Same-hand inner plaster sits on the right of each axis (+X → −Y, +Y → +X).
    let inner_outer = (10.0 + 0.1725, -0.1725);
    let inner_core = (10.0 + 0.1575, -0.1575);
    assert!(
        inner.iter().any(|p| close(*p, inner_outer, 1e-6))
            && inner.iter().any(|p| close(*p, inner_core, 1e-6)),
        "inner plaster must meet on the layer miter, got {inner:?}"
    );

    let fps_b = mitered_layer_footprints(
        &axis_b,
        &layers_b,
        0,
        &axis_a,
        &layers_a,
        Some(1),
        JoinKind::L,
    );
    for i in 0..4 {
        let a = fps[i].as_ref().expect("A layer miter");
        let b = fps_b[i].as_ref().expect("B layer miter");
        let shared = a.iter().any(|pa| b.iter().any(|pb| close(*pa, *pb, 1e-5)));
        assert!(
            shared,
            "layer {i} must share a miter vertex between A and B, A={a:?} B={b:?}"
        );
    }
    let outer = fps[3].as_ref().unwrap();
    let concave_outer = (10.0 - 0.1725, 0.1725);
    let concave_core = (10.0 - 0.1575, 0.1575);
    assert!(
        outer.iter().any(|p| close(*p, concave_outer, 1e-6))
            && outer.iter().any(|p| close(*p, concave_core, 1e-6)),
        "outer plaster (inner L-corner) must miter, got {outer:?}"
    );
    let max_x = outer.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    assert!(
        max_x < 9.99,
        "concave plaster must shorten behind the axis end, max_x={max_x} pts={outer:?}"
    );

    // Room L: B drawn toward the joint so both interiors face the concave side.
    let axis_b_rev = vec![(10.0, 10.0), (10.0, 0.0)];
    let fps_rev = mitered_layer_footprints(
        &axis_a,
        &layers_a,
        1,
        &axis_b_rev,
        &layers_b,
        Some(1),
        JoinKind::L,
    );
    let inner_rev = fps_rev[0]
        .as_ref()
        .expect("reversed-B inner plaster should miter");
    // Layer 0 lives south of A; with B arriving at the joint it must miter
    // on the convex SE bisector, not stop short of x=10.
    assert!(
        inner_rev.iter().any(|p| close(*p, inner_outer, 1e-6))
            && inner_rev.iter().any(|p| close(*p, inner_core, 1e-6)),
        "inner plaster must miter at the convex corner when both walls arrive, got {inner_rev:?}"
    );
    let outer_rev = fps_rev[3]
        .as_ref()
        .expect("reversed-B outer plaster should miter");
    assert!(
        outer_rev.iter().any(|p| close(*p, concave_outer, 1e-6))
            && outer_rev.iter().any(|p| close(*p, concave_core, 1e-6)),
        "outer plaster must miter at the concave L-corner when both walls arrive, got {outer_rev:?}"
    );
}

#[test]
fn concave_first_layer_shortens_when_walls_run_clockwise() {
    // Innenputz is stack index 0 (negative offset). Clockwise room walls
    // put that finish on the concave L side — it must not keep the square
    // cap at the axis endpoint.
    let axis_a = vec![(10.0, 0.0), (0.0, 0.0)];
    let axis_b = vec![(10.0, 0.0), (10.0, 10.0)];
    let layers_a = insulated_masonry_layers();
    let layers_b = insulated_masonry_layers();
    let fps = mitered_layer_footprints(
        &axis_a,
        &layers_a,
        0,
        &axis_b,
        &layers_b,
        Some(0),
        JoinKind::L,
    );
    let inner = fps[0]
        .as_ref()
        .expect("index-0 plaster on the concave side must miter");
    let max_x = inner.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    assert!(
        max_x < 9.99,
        "concave layer 0 must shorten behind the axis end, max_x={max_x} pts={inner:?}"
    );
    let concave_outer = (10.0 - 0.1725, 0.1725);
    let concave_core = (10.0 - 0.1575, 0.1575);
    assert!(
        inner.iter().any(|p| close(*p, concave_outer, 1e-6))
            && inner.iter().any(|p| close(*p, concave_core, 1e-6)),
        "layer 0 must meet on the inner bisector, got {inner:?}"
    );

    let refs_a: Vec<LayerRef> = layers_a
        .iter()
        .enumerate()
        .map(|(i, l)| LayerRef {
            material_id: l.material.clone(),
            role_tag: None,
            index: i,
            layer_id: Some(l.layer_id),
        })
        .collect();
    let ov = JunctionOverride {
        default_style: None,
        layer_pairs: vec![super::super::join::LayerPairOverride {
            layer_a: refs_a[0].clone(),
            layer_b: Some(LayerRef {
                material_id: layers_b[0].material.clone(),
                role_tag: None,
                index: 0,
                layer_id: Some(layers_b[0].layer_id),
            }),
            style: JoinOverrideStyle::Miter,
        }],
        layer_gaps: Vec::new(),
    };
    let fps_ov = mitered_layer_footprints_with_override(
        &axis_a,
        &layers_a,
        &refs_a,
        0,
        &axis_b,
        &layers_b,
        Some(0),
        JoinKind::L,
        Some(&ov),
    );
    let inner_ov = fps_ov[0]
        .as_ref()
        .expect("manual miter on concave layer 0");
    let max_x_ov = inner_ov
        .iter()
        .map(|p| p.0)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        max_x_ov < 9.99,
        "manual miter must still shorten concave layer 0, max_x={max_x_ov} pts={inner_ov:?}"
    );
}

#[test]
fn different_materials_same_index_still_pair() {
    let a = vec![MiterLayer::with_id(0.2, -0.1, "brick", "Finish", uuid::Uuid::new_v4())];
    let b = vec![MiterLayer::with_id(0.2, -0.1, "block", "Structure", uuid::Uuid::new_v4())];
    let pairing = match_layer_indices(&a, &b);
    assert_eq!(pairing, vec![Some(0)]);
}

#[test]
fn arc_l_join_uses_concentric_offset_not_chord() {
    let arc = vec![(-1.0, 0.0), (1.0, 0.0)];
    let bulges = vec![1.0];
    let stem = vec![(1.0, 0.0), (1.0, 2.0)];
    let layers = vec![MiterLayer::geom(0.2, -0.1)];
    let chord = mitered_layer_footprints(
        &arc, &layers, 1, &stem, &layers, Some(0), JoinKind::L,
    );
    let radial = mitered_layer_footprints_with_bulges(
        &arc, &layers, 1, &stem, &layers, Some(0), JoinKind::L, &bulges, &[],
    );
    let chord_fp = chord[0].as_ref().expect("chord miter");
    let radial_fp = radial[0].as_ref().expect("radial miter");
    assert_ne!(
        chord_fp, radial_fp,
        "arc join must not reuse the diameter-chord offset"
    );
    assert!(
        radial_fp.iter().any(|(x, _)| (*x - 1.1).abs() < 0.08),
        "concentric offset at the join should sit near x=1.1, got {radial_fp:?}"
    );
}

#[test]
fn t_junction_interior_wall_meeting_multilayer_exterior_wall() {
    // Exterior through wall: horizontal (0,0) -> (10,0).
    // Total thickness = 0.015 (ext putz) + 0.16 (WDVS) + 0.24 (core) + 0.015 (int putz) = 0.43m
    // Centered around axis y=0:
    // Layer 0: Exterior Putz, thickness 0.015, offset 0.200 to 0.215 (+Y, exterior)
    // Layer 1: WDVS Insulation, thickness 0.160, offset 0.040 to 0.200 (+Y, exterior)
    // Layer 2: Core Concrete, thickness 0.240, offset -0.200 to 0.040
    // Layer 3: Interior Putz, thickness 0.015, offset -0.215 to -0.200 (-Y, interior)
    let through_axis = vec![(0.0, 0.0), (10.0, 0.0)];
    let through_layers = vec![
        MiterLayer::with_id(0.015, 0.200, "Putz", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.160, 0.040, "WDVS", "Thermal", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.240, -0.200, "Concrete", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.015, -0.215, "Putz", "Finish", uuid::Uuid::new_v4()),
    ];

    // Interior stem wall: vertical from (5, -5) to (5, 0), joining at end 1 (5, 0) from interior side (-Y).
    // Total thickness = 0.015 (putz) + 0.200 (core) + 0.015 (putz) = 0.23m
    // Stem axis from (5, -5) to (5, 0) -> normal is (+X, 0).
    // Layer 0: Putz, thickness 0.015, offset -0.115 to -0.100
    // Layer 1: Core Concrete, thickness 0.200, offset -0.100 to 0.100
    // Layer 2: Putz, thickness 0.015, offset 0.100 to 0.115
    let stem_axis = vec![(5.0, -5.0), (5.0, 0.0)];
    let stem_layers = vec![
        MiterLayer::with_id(0.015, -0.115, "Putz", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.200, -0.100, "Concrete", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.015, 0.100, "Putz", "Finish", uuid::Uuid::new_v4()),
    ];

    let fps = mitered_layer_footprints(
        &stem_axis,
        &stem_layers,
        1,
        &through_axis,
        &through_layers,
        None,
        JoinKind::T,
    );

    // 1. Stem core (index 1) must butt cleanly against the near face of the through structural core (y = -0.200).
    let stem_core = fps[1].as_ref().expect("stem core footprint");
    let max_y_core = stem_core.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (max_y_core - (-0.200)).abs() < 1e-6,
        "stem core must stop at through core near face (y=-0.200), got max_y={max_y_core}"
    );

    // 2. Stem finishes (indices 0 and 2) must terminate precisely at the near face of through core (y = -0.200)
    // when mitering into the approach-side Putz layer (which extends from y=-0.215 to y=-0.200).
    for (idx, name) in [(0, "stem putz 0"), (2, "stem putz 2")] {
        let fp = fps[idx].as_ref().expect(name);
        let max_y = fp.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (max_y - (-0.200)).abs() < 1e-6,
            "{name} must miter to the through interior putz boundary (y=-0.200), got max_y={max_y}"
        );
    }

    // 3. Cutouts on the through wall:
    // Only the interior finish (Layer 3) on the approach side should receive a cutout.
    // Exterior finish (Layer 0), WDVS (Layer 1), and Core (Layer 2) must NOT be cut out (None).
    let cutouts = through_wall_cutout_footprints(
        &through_axis,
        &through_layers,
        &stem_axis,
        &stem_layers,
        1,
    );
    assert_eq!(cutouts.len(), 4);
    assert!(cutouts[0].is_none(), "exterior putz on far side must remain untouched");
    assert!(cutouts[1].is_none(), "WDVS insulation on far side must remain untouched");
    assert!(cutouts[2].is_none(), "through core must remain continuous (not notched)");
    assert!(cutouts[3].is_some(), "interior putz on approach side must be notched for stem core");

    let int_putz_fp = cutouts[3].as_ref().unwrap();
    let rings = split_footprint_rings(int_putz_fp);
    assert_eq!(rings.len(), 2, "interior putz must be split into two disjoint rings");
}

#[test]
fn t_junction_default_style_overrides_near_butt_far() {
    let through_axis = vec![(0.0, 0.0), (10.0, 0.0)];
    let through_layers = vec![
        MiterLayer::with_id(0.015, 0.200, "Putz", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.160, 0.040, "WDVS", "Thermal", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.240, -0.200, "Concrete", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.015, -0.215, "Putz", "Finish", uuid::Uuid::new_v4()),
    ];

    let stem_axis = vec![(5.0, -5.0), (5.0, 0.0)];
    let stem_layers = vec![
        MiterLayer::with_id(0.015, -0.115, "Putz", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.200, -0.100, "Concrete", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.015, 0.100, "Putz", "Finish", uuid::Uuid::new_v4()),
    ];
    let stem_refs: Vec<LayerRef> = stem_layers
        .iter()
        .enumerate()
        .map(|(i, l)| LayerRef {
            material_id: l.material.clone(),
            role_tag: None,
            index: i,
            layer_id: Some(l.layer_id),
        })
        .collect();

    // 1. default_style: NearFace (context menu "Nähere Kante")
    let ov_near = JunctionOverride {
        default_style: Some(JoinOverrideStyle::NearFace),
        layer_pairs: vec![],
        layer_gaps: vec![],
    };
    let fps_near = mitered_layer_footprints_with_override(
        &stem_axis,
        &stem_layers,
        &stem_refs,
        1,
        &through_axis,
        &through_layers,
        None,
        JoinKind::T,
        Some(&ov_near),
    );
    // Stem core stops at through core near face (-0.200)
    let core_near = fps_near[1].as_ref().expect("stem core");
    let max_y_core_near = core_near.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (max_y_core_near - (-0.200)).abs() < 1e-6,
        "default_style NearFace: core must stop at y=-0.200, got {max_y_core_near}"
    );
    // Stem finishes stop at approach-side outer face (-0.215)
    for idx in [0, 2] {
        let fp = fps_near[idx].as_ref().expect("stem putz");
        let max_y = fp.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (max_y - (-0.215)).abs() < 1e-6,
            "default_style NearFace: finish {idx} must stop at y=-0.215, got {max_y}"
        );
    }

    // 2. default_style: Butt (context menu "Stoß")
    let ov_butt = JunctionOverride {
        default_style: Some(JoinOverrideStyle::Butt),
        layer_pairs: vec![],
        layer_gaps: vec![],
    };
    let fps_butt = mitered_layer_footprints_with_override(
        &stem_axis,
        &stem_layers,
        &stem_refs,
        1,
        &through_axis,
        &through_layers,
        None,
        JoinKind::T,
        Some(&ov_butt),
    );
    let core_butt = fps_butt[1].as_ref().expect("stem core");
    let max_y_core_butt = core_butt.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (max_y_core_butt - (-0.200)).abs() < 1e-6,
        "default_style Butt: core must stop at y=-0.200, got {max_y_core_butt}"
    );
    for idx in [0, 2] {
        let fp = fps_butt[idx].as_ref().expect("stem putz");
        let max_y = fp.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (max_y - (-0.215)).abs() < 1e-6,
            "default_style Butt: finish {idx} must stop at y=-0.215, got {max_y}"
        );
    }

    // 3. default_style: FarFace (context menu "Entferntere Kante")
    let ov_far = JunctionOverride {
        default_style: Some(JoinOverrideStyle::FarFace),
        layer_pairs: vec![],
        layer_gaps: vec![],
    };
    let fps_far = mitered_layer_footprints_with_override(
        &stem_axis,
        &stem_layers,
        &stem_refs,
        1,
        &through_axis,
        &through_layers,
        None,
        JoinKind::T,
        Some(&ov_far),
    );
    // Stem core stops at through core far face (+0.040)
    let core_far = fps_far[1].as_ref().expect("stem core");
    let max_y_core_far = core_far.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (max_y_core_far - 0.040).abs() < 1e-6,
        "default_style FarFace: core must extend to far face of core (y=0.040), got {max_y_core_far}"
    );
    // Stem finishes stop at far face of approach-side interior putz (-0.200) - NO tunneling across core/WDVS!
    for idx in [0, 2] {
        let fp = fps_far[idx].as_ref().expect("stem putz");
        let max_y = fp.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (max_y - (-0.200)).abs() < 1e-6,
            "default_style FarFace: finish {idx} must stop at far face of interior putz (y=-0.200), got {max_y}"
        );
    }
}

#[test]
fn t_junction_reversed_stem_axis_at_end_0() {
    let through_axis = vec![(0.0, 0.0), (10.0, 0.0)];
    let through_layers = vec![
        MiterLayer::with_id(0.015, 0.200, "Putz", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.160, 0.040, "WDVS", "Thermal", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.240, -0.200, "Concrete", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.015, -0.215, "Putz", "Finish", uuid::Uuid::new_v4()),
    ];

    // Stem runs from (5.0, 0.0) down to (5.0, -5.0), joining at end 0 (5.0, 0.0) from interior side (-Y).
    let stem_axis = vec![(5.0, 0.0), (5.0, -5.0)];
    let stem_layers = vec![
        MiterLayer::with_id(0.015, -0.115, "Putz", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.200, -0.100, "Concrete", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.015, 0.100, "Putz", "Finish", uuid::Uuid::new_v4()),
    ];

    let fps = mitered_layer_footprints(
        &stem_axis,
        &stem_layers,
        0,
        &through_axis,
        &through_layers,
        None,
        JoinKind::T,
    );

    let stem_core = fps[1].as_ref().expect("stem core footprint");
    let max_y_core = stem_core.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (max_y_core - (-0.200)).abs() < 1e-6,
        "reversed stem at end 0: core must stop at y=-0.200, got {max_y_core}"
    );

    for (idx, name) in [(0, "stem putz 0"), (2, "stem putz 2")] {
        let fp = fps[idx].as_ref().expect(name);
        let max_y = fp.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (max_y - (-0.200)).abs() < 1e-6,
            "reversed stem at end 0: {name} must miter at y=-0.200, got {max_y}"
        );
    }
}

#[test]
fn t_junction_two_structural_layers_in_stem() {
    let through_axis = vec![(0.0, 0.0), (10.0, 0.0)];
    let through_layers = vec![
        MiterLayer::with_id(0.015, 0.200, "Putz", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.160, 0.040, "WDVS", "Thermal", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.240, -0.200, "Concrete", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.015, -0.215, "Putz", "Finish", uuid::Uuid::new_v4()),
    ];

    // Stem has 2 structural layers: Layer 0 (Concrete 0.15) and Layer 1 (Brick 0.15), plus Layer 2 (Finish 0.015).
    let stem_axis = vec![(5.0, -5.0), (5.0, 0.0)];
    let stem_layers = vec![
        MiterLayer::with_id(0.150, -0.165, "Concrete", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.150, -0.015, "Brick", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.015, 0.135, "Putz", "Finish", uuid::Uuid::new_v4()),
    ];

    let fps = mitered_layer_footprints(
        &stem_axis,
        &stem_layers,
        1,
        &through_axis,
        &through_layers,
        None,
        JoinKind::T,
    );

    // Both structural layers of the stem must butt against the near face of the through core (y = -0.200).
    for (idx, name) in [(0, "stem concrete structural"), (1, "stem brick structural")] {
        let fp = fps[idx].as_ref().expect(name);
        let max_y = fp.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (max_y - (-0.200)).abs() < 1e-6,
            "{name} must butt at through core near face y=-0.200, got {max_y}"
        );
    }

    // Cutouts: both structural layers should punch the approach-side interior putz.
    let cutouts = through_wall_cutout_footprints(
        &through_axis,
        &through_layers,
        &stem_axis,
        &stem_layers,
        1,
    );
    assert!(cutouts[3].is_some(), "approach-side interior putz must be notched for dual-core stem");
    assert!(cutouts[2].is_none(), "through core must remain continuous");
}

#[test]
fn t_junction_exterior_approach_notches_wdvs_leaves_interior_untouched() {
    let through_axis = vec![(0.0, 0.0), (10.0, 0.0)];
    let through_layers = vec![
        MiterLayer::with_id(0.015, 0.200, "Putz", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.160, 0.040, "WDVS", "Thermal", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.240, -0.200, "Concrete", "Structure", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.015, -0.215, "Putz", "Finish", uuid::Uuid::new_v4()),
    ];

    // Stem approaches from exterior (+Y): from (5, 5) to (5, 0), joining at end 1.
    let stem_axis = vec![(5.0, 5.0), (5.0, 0.0)];
    let stem_layers = vec![
        MiterLayer::with_id(0.015, -0.115, "Putz", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.200, -0.100, "Concrete", "Structure", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.015, 0.100, "Putz", "Finish", uuid::Uuid::new_v4()),
    ];

    let fps = mitered_layer_footprints(
        &stem_axis,
        &stem_layers,
        1,
        &through_axis,
        &through_layers,
        None,
        JoinKind::T,
    );

    // Stem core stops at through core near face from exterior (y = 0.040).
    let stem_core = fps[1].as_ref().expect("stem core footprint");
    let min_y_core = stem_core.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    assert!(
        (min_y_core - 0.040).abs() < 1e-6,
        "stem core approaching from +Y must stop at y=0.040, got min_y={min_y_core}"
    );

    // Through wall cutouts:
    // Exterior finish (0) and WDVS (1) on approach side must be notched by stem core.
    // Core (2) and Interior finish (3) on far side must remain untouched (None).
    let cutouts = through_wall_cutout_footprints(
        &through_axis,
        &through_layers,
        &stem_axis,
        &stem_layers,
        1,
    );
    assert!(cutouts[0].is_some(), "exterior putz on approach side must be notched");
    assert!(cutouts[1].is_some(), "WDVS insulation on approach side must be notched");
    assert!(cutouts[2].is_none(), "through core must stay continuous");
    assert!(cutouts[3].is_none(), "interior putz on far side must stay untouched");
}

#[test]
fn t_junction_reversed_through_axis_orientation() {
    // Through wall running in reversed direction: (10, 0) -> (0, 0).
    // Normal (-dy, dx) is now (0, -10)/10 = (0, -1) [-Y is positive offset].
    // Layer 0: Exterior Putz, offset 0.200 to 0.215 (at y = -0.215 to -0.200)
    // Layer 1: Core Concrete, offset -0.200 to 0.000 (at y = 0.000 to 0.200)
    let through_axis = vec![(10.0, 0.0), (0.0, 0.0)];
    let through_layers = vec![
        MiterLayer::with_id(0.015, 0.200, "Putz", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.200, -0.200, "Concrete", "Structure", uuid::Uuid::new_v4()),
    ];

    // Stem wall from (5, 0) to (5, 5) with join at end 0 (5, 0) approaching from +Y (core side).
    let stem_axis = vec![(5.0, 0.0), (5.0, 5.0)];
    let stem_layers = vec![
        MiterLayer::with_id(0.200, -0.100, "Concrete", "Structure", uuid::Uuid::new_v4()),
    ];

    let fps = mitered_layer_footprints(
        &stem_axis,
        &stem_layers,
        0,
        &through_axis,
        &through_layers,
        None,
        JoinKind::T,
    );

    let core = fps[0].as_ref().expect("stem core");
    let min_y = core.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    // Near face of through core is at y = 0.200 (since core runs y = 0.000 to 0.200)
    assert!(
        (min_y - 0.200).abs() < 1e-6,
        "stem core must butt at y=0.200 for reversed through axis, got min_y={min_y}"
    );
}

#[test]
fn t_junction_manual_near_and_far_face_overrides() {
    let through_axis = vec![(0.0, 0.0), (10.0, 0.0)];
    let through_layers = vec![
        MiterLayer::with_id(0.015, 0.200, "Putz", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.160, 0.040, "WDVS", "Thermal", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.240, -0.200, "Concrete", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.015, -0.215, "Putz", "Finish", uuid::Uuid::new_v4()),
    ];

    let stem_axis = vec![(5.0, -5.0), (5.0, 0.0)];
    let stem_layers = vec![
        MiterLayer::with_id(0.015, -0.115, "Putz", "Finish", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.200, -0.100, "Concrete", "Structural", uuid::Uuid::new_v4()),
        MiterLayer::with_id(0.015, 0.100, "Putz", "Finish", uuid::Uuid::new_v4()),
    ];

    let stem_refs: Vec<LayerRef> = stem_layers
        .iter()
        .enumerate()
        .map(|(i, l)| LayerRef {
            material_id: l.material.clone(),
            role_tag: None,
            index: i,
            layer_id: Some(l.layer_id),
        })
        .collect();

    // 1. Override Layer 0 (Putz) to NearFace of through core (Layer 2).
    let ov_near_core = JunctionOverride {
        default_style: None,
        layer_pairs: vec![super::super::join::LayerPairOverride {
            layer_a: stem_refs[0].clone(),
            layer_b: Some(LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 2,
                layer_id: Some(through_layers[2].layer_id),
            }),
            style: JoinOverrideStyle::NearFace,
        }],
        layer_gaps: Vec::new(),
    };

    let fps_near = mitered_layer_footprints_with_override(
        &stem_axis,
        &stem_layers,
        &stem_refs,
        1,
        &through_axis,
        &through_layers,
        None,
        JoinKind::T,
        Some(&ov_near_core),
    );
    let fp0_near = fps_near[0].as_ref().expect("layer 0 with near-core override");
    let max_y_near = fp0_near.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (max_y_near - (-0.200)).abs() < 1e-6,
        "NearFace of core must snap to y=-0.200, got max_y={max_y_near}"
    );

    // 2. Override Layer 0 (Putz) to FarFace of through core (Layer 2).
    let ov_far_core = JunctionOverride {
        default_style: None,
        layer_pairs: vec![super::super::join::LayerPairOverride {
            layer_a: stem_refs[0].clone(),
            layer_b: Some(LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 2,
                layer_id: Some(through_layers[2].layer_id),
            }),
            style: JoinOverrideStyle::FarFace,
        }],
        layer_gaps: Vec::new(),
    };

    let fps_far = mitered_layer_footprints_with_override(
        &stem_axis,
        &stem_layers,
        &stem_refs,
        1,
        &through_axis,
        &through_layers,
        None,
        JoinKind::T,
        Some(&ov_far_core),
    );
    let fp0_far = fps_far[0].as_ref().expect("layer 0 with far-core override");
    let max_y_far = fp0_far.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (max_y_far - 0.040).abs() < 1e-6,
        "FarFace of core must snap to y=0.040, got max_y={max_y_far}"
    );

    // 3. Override Layer 0 (Putz) to Butt of through WDVS (Layer 1).
    let ov_butt_wdvs = JunctionOverride {
        default_style: None,
        layer_pairs: vec![super::super::join::LayerPairOverride {
            layer_a: stem_refs[0].clone(),
            layer_b: Some(LayerRef {
                material_id: "WDVS".to_string(),
                role_tag: None,
                index: 1,
                layer_id: Some(through_layers[1].layer_id),
            }),
            style: JoinOverrideStyle::Butt,
        }],
        layer_gaps: Vec::new(),
    };

    let fps_butt = mitered_layer_footprints_with_override(
        &stem_axis,
        &stem_layers,
        &stem_refs,
        1,
        &through_axis,
        &through_layers,
        None,
        JoinKind::T,
        Some(&ov_butt_wdvs),
    );
    let fp0_butt = fps_butt[0].as_ref().expect("layer 0 with butt-wdvs override");
    let max_y_butt = fp0_butt.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    // Near face of WDVS from interior (-Y) is y = 0.040
    assert!(
        (max_y_butt - 0.040).abs() < 1e-6,
        "Butt of WDVS from interior must snap to y=0.040, got max_y={max_y_butt}"
    );
}
