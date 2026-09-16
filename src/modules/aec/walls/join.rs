//! `AEC_WALLJOIN` interactive command.

use acadrust::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::modules::aec::commands::{
    all_wall_axis_handles, get_wall_vertices, is_wall_pick_target, resolve_wall_package,
    WALL_JOIN_SNAP_RADIUS,
};
use crate::modules::aec::engine::join;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_WALLJOIN",
        label: "Join Walls",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/aec/wall_join.svg")),
        event: ModuleEvent::Command("AEC_WALLJOIN".to_string()),
    }
}

/// `AEC_WALLJOIN` — interactive front-end: pick two wall entities (clicking a
/// derived contour/hatch/solid resolves to its axis, like every other wall
/// selection, via [`resolve_wall_package`]), then delegate the actual
/// geometry join to [`aec_walljoin_do`] via [`CmdResult::Dispatch`] once both
/// picks are in — the same "gather interactively, execute non-interactively
/// with full scene access" split used by [`MaterialCommand`]/`AEC_MATERIAL_ADD`.
pub struct WallJoinCommand {
    selected: Vec<Handle>,
}

impl WallJoinCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            selected: Vec::new(),
        }
    }
}

impl CadCommand for WallJoinCommand {
    fn name(&self) -> &'static str {
        "AEC_WALLJOIN"
    }

    fn prompt(&self) -> String {
        match self.selected.len() {
            0 => crate::tr!("aec", "walljoin-first"),
            _ => crate::tr!("aec", "walljoin-second"),
        }
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    /// Highlight wall packages for both the first and the second pick.
    fn entity_pick_highlights_hover(&self) -> bool {
        self.selected.len() < 2
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

    /// N-way-aware hover preview: while awaiting the second (target) wall,
    /// check whether joining the already-selected wall with the currently
    /// hovered candidate would actually resolve into a 3+ way junction (i.e.
    /// another wall already shares that corner). If so, highlight every OTHER
    /// participant of that prospective junction with a preview wire — the
    /// hovered handle itself is already covered by the normal single-handle
    /// hover highlight. Plain L/T (2-wall) joins keep relying solely on that
    /// single-handle highlight, unchanged.
    fn entity_pick_acquire_previews(&self, scene: &Scene, handle: Handle) -> Vec<WireModel> {
        if self.selected.len() != 1 || handle.is_null() {
            return vec![];
        }
        let axis_a = resolve_wall_package(scene, self.selected[0]);
        let axis_b = resolve_wall_package(scene, handle);
        if axis_a.is_null() || axis_b.is_null() || axis_a == axis_b {
            return vec![];
        }

        let handles = all_wall_axis_handles(scene);
        let (Some(idx_a), Some(idx_b)) = (
            handles.iter().position(|h| *h == axis_a),
            handles.iter().position(|h| *h == axis_b),
        ) else {
            return vec![];
        };
        let axes: Vec<Vec<DVec3>> = handles
            .iter()
            .map(|h| get_wall_vertices(scene, *h))
            .collect();
        let axis_refs: Vec<&[DVec3]> = axes.iter().map(|a| a.as_slice()).collect();
        let junctions = join::detect_junctions(&axis_refs, WALL_JOIN_SNAP_RADIUS);

        for junc in junctions.into_iter().filter(|j| j.is_multi_wall()) {
            let has_a = junc.participants.iter().any(|p| p.wall_index == idx_a);
            let has_b = junc.participants.iter().any(|p| p.wall_index == idx_b);
            if !has_a || !has_b {
                continue;
            }
            // Only the 3+-way case gets the extra multi-wire preview.
            let mut wires = Vec::new();
            for p in &junc.participants {
                let h = handles[p.wall_index];
                if h == axis_b {
                    continue; // already shown via the single-handle hover highlight
                }
                let pts: Vec<[f64; 3]> = axes[p.wall_index]
                    .iter()
                    .map(|v| [v.x, v.y, v.z])
                    .collect();
                if pts.len() < 2 {
                    continue;
                }
                let mut wire = WireModel::solid_f64(
                    format!("__walljoin_junction_preview_{}__", h.value()),
                    pts,
                    WireModel::HOVER,
                    false,
                );
                wire.line_weight_px = wire.line_weight_px.max(2.0);
                wires.push(wire);
            }
            return wires;
        }
        vec![]
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.selected.push(handle);
        if self.selected.len() == 2 {
            let a = self.selected[0];
            let b = self.selected[1];
            CmdResult::Dispatch(format!("AEC_WALLJOIN_DO {}|{}", a.value(), b.value()))
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}


inventory::submit!(crate::command::CommandRegistration { names: &["AEC_WALLJOIN"] });
