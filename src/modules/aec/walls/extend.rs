//! `AEC_WALLEXTEND` interactive command.

use acadrust::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult};
use crate::modules::aec::commands::{
    get_wall_bulges, get_wall_vertices, is_wall_pick_target, regenerate_wall_representation_with_corner_rules_and_substitutions,
    regenerate_wall_representation_with_rules_and_substitutions, resolve_wall_package,
    update_wall_vertices, wall_layer_data,
};
use crate::modules::aec::engine::{self, join, StyleLibrary};
use crate::modules::aec::engine::join::JoinKind;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;
use std::collections::HashMap;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_WALLEXTEND",
        label: "Extend Wall",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/aec/wall_extend.svg")),
        event: ModuleEvent::Command("AEC_WALLEXTEND".to_string()),
    }
}

/// Target-acquisition mode for [`WallExtendCommand`], selectable via the
/// `Point`/`Wall` command-line option once the source wall is picked.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WallExtendMode {
    /// Default: extend to the intersection with another wall's axis.
    /// The wall under the cursor is highlighted like any other entity pick.
    ToWall,
    /// Extend to an explicitly-typed/snapped point. Behaves like the normal
    /// point-picking flow (with the usual point-snap preview), not an
    /// entity pick.
    ToPoint,
}

/// `AEC_WALLEXTEND` — interactive front-end: pick a wall, then either extend
/// it to another wall's axis intersection (default `ToWall` mode, reusing
/// [`join::join_wall_axes`]) or to an explicit point (`ToPoint` mode,
/// switched into via the `Point` command option and back via `Wall`).
/// Delegates the actual write to [`aec_wallextend_do`] via
/// [`CmdResult::Dispatch`].
pub struct WallExtendCommand {
    wall: Option<Handle>,
    mode: WallExtendMode,
}

impl WallExtendCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            wall: None,
            mode: WallExtendMode::ToWall,
        }
    }
}

impl CadCommand for WallExtendCommand {
    fn name(&self) -> &'static str {
        "AEC_WALLEXTEND"
    }

    fn prompt(&self) -> String {
        if self.wall.is_none() {
            crate::tr!("aec", "wallextend-select")
        } else if self.mode == WallExtendMode::ToPoint {
            crate::tr!("aec", "wallextend-point")
        } else {
            crate::tr!("aec", "wallextend-wall")
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.wall.is_none() {
            return Vec::new();
        }
        match self.mode {
            WallExtendMode::ToWall => vec![CmdOption::new("Point", "P")],
            WallExtendMode::ToPoint => vec![CmdOption::new("Wall", "W")],
        }
    }

    fn wants_text_input(&self) -> bool {
        self.wall.is_some()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.wall.is_none() {
            return None;
        }
        let text = text.trim();
        if self.mode == WallExtendMode::ToWall && text.eq_ignore_ascii_case("p") {
            self.mode = WallExtendMode::ToPoint;
            return Some(CmdResult::NeedPoint);
        }
        if self.mode == WallExtendMode::ToPoint && text.eq_ignore_ascii_case("w") {
            self.mode = WallExtendMode::ToWall;
            return Some(CmdResult::NeedPoint);
        }
        None
    }

    fn needs_entity_pick(&self) -> bool {
        // Entity-pick for the initial wall-to-extend selection, and again
        // while in `ToWall` mode so the target wall gets the normal rollover
        // highlight. In `ToPoint` mode we fall back to plain point-picking
        // so the usual point-snap preview is shown instead.
        self.wall.is_none() || self.mode == WallExtendMode::ToWall
    }

    /// Highlight wall packages for the source pick and the target-wall pick.
    fn entity_pick_highlights_hover(&self) -> bool {
        self.needs_entity_pick()
    }

    /// Restrict the rollover highlight to wall packages (axis or derived).
    fn entity_pick_hover_highlights_handle(&self, scene: &Scene, handle: Handle) -> bool {
        is_wall_pick_target(scene, handle)
    }

    /// Walls are rendered as filled contours, so a click anywhere inside the
    /// wall's body (not just precisely on its outline) must resolve to the
    /// wall entity; otherwise clicking a wall almost always misses.
    fn entity_pick_includes_fills(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if self.wall.is_none() {
            if handle.is_null() {
                return CmdResult::NeedPoint;
            }
            self.wall = Some(handle);
            return CmdResult::NeedPoint;
        }
        let wall = self.wall.unwrap();
        // In `ToWall` mode a miss or a click back on the source wall itself
        // doesn't extend anything — the user must pick a *different* wall,
        // or switch to `ToPoint` mode explicitly via the option.
        if handle.is_null() || handle == wall {
            return CmdResult::NeedPoint;
        }
        // The DO handler runs resolve_wall_package so a click on a derived
        // contour/hatch/solid of another wall still joins correctly.
        CmdResult::Dispatch(format!(
            "AEC_WALLEXTEND_DO {}|WALL|{}",
            wall.value(),
            handle.value()
        ))
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let Some(wall) = self.wall {
            if self.mode == WallExtendMode::ToPoint {
                return CmdResult::Dispatch(format!(
                    "AEC_WALLEXTEND_DO {}|PT|{}|{}|{}",
                    wall.value(),
                    pt.x,
                    pt.y,
                    pt.z
                ));
            }
        }
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// `AEC_WALLEXTEND_DO wall|PT|x|y|z` or `AEC_WALLEXTEND_DO wall|WALL|target`
/// — the non-interactive handler `WallExtendCommand` dispatches to once the
/// target is picked. Resolves the wall pick(s) to their axis, extends the
/// nearer endpoint of `wall`'s axis (to the point, or to the intersection
/// with `target`'s axis via [`join::join_wall_axes`]), writes it back and
/// regenerates the wall's representation. Reports [`JoinError`] via the
/// command line instead of panicking.
pub fn aec_wallextend_do(
    scene: &mut Scene,
    command_line: &mut CommandLine,
    args: &str,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) {
    let mut parts = args.splitn(2, '|');
    let (Some(wall_str), Some(rest)) = (parts.next(), parts.next()) else {
        command_line.push_error(&crate::tr!("aec", "wallextend-malformed-args"));
        return;
    };
    let Ok(wall_val) = wall_str.parse::<u64>() else {
        command_line.push_error(&crate::tr!("aec", "wallextend-malformed-handle"));
        return;
    };
    let wall_handle = resolve_wall_package(scene, Handle::new(wall_val));
    let mut axis = get_wall_vertices(scene, wall_handle);
    if axis.len() < 2 {
        command_line.push_error(&crate::tr!("aec", "wallextend-need-axis"));
        return;
    }

    if let Some(pt_args) = rest.strip_prefix("PT|") {
        let coords: Vec<&str> = pt_args.split('|').collect();
        let [x, y, z] = coords.as_slice() else {
            command_line.push_error(&crate::tr!("aec", "wallextend-malformed-point"));
            return;
        };
        let (Ok(x), Ok(y), Ok(z)) = (x.parse::<f64>(), y.parse::<f64>(), z.parse::<f64>()) else {
            command_line.push_error(&crate::tr!("aec", "wallextend-malformed-point"));
            return;
        };
        let pt = DVec3::new(x, y, z);
        let d1 = axis[0].distance(pt);
        let d2 = axis.last().unwrap().distance(pt);
        // Project the picked point onto the wall's existing direction line
        // instead of using it directly as the new endpoint — this preserves
        // the wall's original direction exactly, even if `pt` isn't
        // perfectly collinear (e.g. a slightly imprecise pick).
        //
        // The direction must be taken from the segment immediately adjacent
        // to the endpoint being moved (i.e. the endpoint and its neighbour),
        // NOT from the endpoint and the opposite end of the whole axis: for
        // multi-vertex wall polylines (more than two vertices) those differ,
        // and anchoring on the far end would bend the extended segment away
        // from its actual direction.
        if d1 < d2 {
            let anchor = axis[1];
            let near = axis[0];
            let dir = near - anchor;
            if dir.length_squared() > 1e-12 {
                let t = (pt - anchor).dot(dir) / dir.length_squared();
                axis[0] = anchor + dir * t;
            } else {
                axis[0] = pt;
            }
        } else {
            let last = axis.len() - 1;
            let anchor = axis[last - 1];
            let near = axis[last];
            let dir = near - anchor;
            if dir.length_squared() > 1e-12 {
                let t = (pt - anchor).dot(dir) / dir.length_squared();
                axis[last] = anchor + dir * t;
            } else {
                axis[last] = pt;
            }
        }
        update_wall_vertices(scene, wall_handle, &axis);
        let touched = match regenerate_wall_representation_with_rules_and_substitutions(
            scene,
            wall_handle,
            display_rules,
            style_substitutions,
            library_override,
        ) {
            Ok(t) => t,
            Err(_) => vec![wall_handle],
        };
        // Do not auto-join to a corner after a length-only extend.
        let changes: Vec<_> = touched
            .into_iter()
            .filter(|h| scene.document.get_entity(*h).is_some())
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect();
        if !changes.is_empty() {
            scene.bump_entities(&changes);
        }
        command_line.push_info(&crate::tr!("aec", "wallextend-ok"));
    } else if let Some(target_str) = rest.strip_prefix("WALL|") {
        let Ok(target_val) = target_str.parse::<u64>() else {
            command_line.push_error(&crate::tr!("aec", "wallextend-malformed-target"));
            return;
        };
        let target_handle = resolve_wall_package(scene, Handle::new(target_val));
        if target_handle == wall_handle {
            command_line.push_error(&crate::tr!("aec", "wallextend-different"));
            return;
        }
        let target_axis = get_wall_vertices(scene, target_handle);
        match join::extend_axis_to_other(&axis, &target_axis) {
            Ok((new_axis, end_idx, _isect)) => {
                update_wall_vertices(scene, wall_handle, &new_axis);
                let layers_target = wall_layer_data(scene, target_handle);
                let axis_target_2d: Vec<(f64, f64)> =
                    target_axis.iter().map(|p| (p.x, p.y)).collect();
                let miter = engine::miter::JoinMiterContext {
                    self_end: end_idx,
                    other_axis: axis_target_2d,
                    other_layers: layers_target,
                    other_end: None,
                    kind: JoinKind::T,
                    self_bulges: get_wall_bulges(scene, wall_handle),
                    other_bulges: get_wall_bulges(scene, target_handle),
                    as_through: false,
                };
                let mut touched = match regenerate_wall_representation_with_corner_rules_and_substitutions(
                    scene,
                    wall_handle,
                    None,
                    Some(&miter),
                    display_rules,
                    style_substitutions,
                    library_override,
                ) {
                    Ok(t) => t,
                    Err(_) => vec![wall_handle],
                };
                // Target length stays unchanged; refresh its display so
                // overlapping layer edges stay in sync visually.
                match regenerate_wall_representation_with_rules_and_substitutions(
                    scene,
                    target_handle,
                    display_rules,
                    style_substitutions,
                    library_override,
                ) {
                    Ok(t) => touched.extend(t),
                    Err(_) => touched.push(target_handle),
                }
                // Register the two walls as joined peers — same bookkeeping
                // `AEC_WALLJOIN`/auto-join perform after a successful join —
                // so later moves/regenerations recognize and preserve this
                // connection instead of silently treating it as unjoined.
                engine::owner_index::link_peers(&mut scene.document, wall_handle, target_handle);

                touched.sort_by_key(|h| h.value());
                touched.dedup();
                let changes: Vec<_> = touched
                    .into_iter()
                    .filter(|h| scene.document.get_entity(*h).is_some())
                    .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                    .collect();
                if !changes.is_empty() {
                    scene.bump_entities(&changes);
                }
                command_line.push_info(&crate::tr!("aec", "wallextend-to-wall-ok"));
            }
            Err(e) => {
                command_line.push_error(&format!("AEC_WALLEXTEND: {}", e));
            }
        }
    } else {
        command_line.push_error(&crate::tr!("aec", "wallextend-malformed-args"));
    }
}


inventory::submit!(crate::command::CommandRegistration { names: &["AEC_WALLEXTEND"] });
