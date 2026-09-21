//! `AEC_WINDOW` / shared wall opening command.

use acadrust::Handle;
use glam::DVec3;
use std::cell::RefCell;
use std::collections::HashMap;

use crate::command::{CadCommand, CmdResult};
use crate::modules::aec::engine::opening_display::{
    preview_opening_hatches, preview_opening_wires,
};
use crate::modules::aec::engine::opening_style::{apply_style_defaults, OpeningStyle};
use crate::modules::aec::engine::opening_xdata::place_wall_opening;
use crate::modules::aec::engine::openings::{distance_along_axis_from_point, Opening};
use crate::modules::aec::engine::wall_package::{is_wall_pick_target, resolve_wall_package};
use crate::modules::aec::engine::xdata::{get_wall_vertices, wall_from_entity};
use crate::modules::aec::engine::{self, StyleLibrary};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::hatch_model::HatchModel;
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_WINDOW",
        label: "Window",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/aec/wall_create.svg")),
        event: ModuleEvent::Command("AEC_WINDOW".to_string()),
    }
}

struct CachedWall {
    axis: Vec<(f64, f64)>,
    thickness: f64,
}

/// `AEC_WINDOW` / `AEC_DOOR` / `AEC_OPENING` — pick a wall, then a point
/// along it. Dimensions come from the kind's seed style (library on place).
pub struct WallOpeningCommand {
    kind: engine::openings::OpeningKind,
    wall: Option<Handle>,
    cached: RefCell<Option<CachedWall>>,
    preview_hatches: Vec<HatchModel>,
}

impl WallOpeningCommand {
    fn new(kind: engine::openings::OpeningKind) -> Self {
        Self {
            kind,
            wall: None,
            cached: RefCell::new(None),
            preview_hatches: Vec::new(),
        }
    }

    #[allow(clippy::new_without_default)]
    pub fn new_window() -> Self {
        Self::new(engine::openings::OpeningKind::Window)
    }

    #[allow(clippy::new_without_default)]
    pub fn new_door() -> Self {
        Self::new(engine::openings::OpeningKind::Door)
    }

    #[allow(clippy::new_without_default)]
    pub fn new_opening() -> Self {
        Self::new(engine::openings::OpeningKind::Breakthrough)
    }

    fn seed_style(&self) -> OpeningStyle {
        match self.kind {
            engine::openings::OpeningKind::Window => OpeningStyle::standard_window(),
            engine::openings::OpeningKind::Door => OpeningStyle::standard_door(),
            engine::openings::OpeningKind::Breakthrough => OpeningStyle::standard_breakthrough(),
        }
    }

    fn preview_instance(&self, distance: f64) -> Opening {
        let mut opening = Opening::from_kind(Handle::NULL, Handle::NULL, distance, self.kind);
        apply_style_defaults(&mut opening, &self.seed_style());
        opening
    }

    fn cache_from_scene(&self, scene: &Scene, handle: Handle) {
        let axis_h = resolve_wall_package(scene, handle);
        let verts = get_wall_vertices(scene, axis_h);
        if verts.len() < 2 {
            return;
        }
        let thickness = scene
            .document
            .get_entity(axis_h)
            .and_then(wall_from_entity)
            .map(|w| w.total_thickness())
            .unwrap_or(0.3);
        *self.cached.borrow_mut() = Some(CachedWall {
            axis: verts.iter().map(|v| (v.x, v.y)).collect(),
            thickness,
        });
    }
}

impl CadCommand for WallOpeningCommand {
    fn name(&self) -> &'static str {
        match self.kind {
            engine::openings::OpeningKind::Window => "AEC_WINDOW",
            engine::openings::OpeningKind::Door => "AEC_DOOR",
            engine::openings::OpeningKind::Breakthrough => "AEC_OPENING",
        }
    }

    fn prompt(&self) -> String {
        let tag = self.name();
        if self.wall.is_none() {
            crate::tr!("aec", "opening-select-wall", cmd = tag)
        } else {
            crate::tr!("aec", "opening-specify-point", cmd = tag)
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.wall.is_none()
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        self.wall.is_none()
    }

    fn entity_pick_hover_highlights_handle(&self, scene: &Scene, handle: Handle) -> bool {
        let ok = is_wall_pick_target(scene, handle);
        if ok {
            self.cache_from_scene(scene, handle);
        }
        ok
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.wall = Some(handle);
        CmdResult::NeedPoint
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        if self.wall.is_none() {
            self.preview_hatches.clear();
            return Vec::new();
        }
        let (axis, thickness) = {
            let cache = self.cached.borrow();
            let Some(cache) = cache.as_ref() else {
                return Vec::new();
            };
            (cache.axis.clone(), cache.thickness)
        };
        let Some(distance) = distance_along_axis_from_point(&axis, (pt.x, pt.y)) else {
            return Vec::new();
        };
        let opening = self.preview_instance(distance);
        self.preview_hatches = preview_opening_hatches(&axis, thickness, &opening, None, None);
        preview_opening_wires(&axis, thickness, &opening, None, None)
    }

    fn hatch_preview_models(&self) -> Option<Vec<HatchModel>> {
        if self.preview_hatches.is_empty() {
            None
        } else {
            Some(self.preview_hatches.clone())
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let Some(wall) = self.wall else {
            return CmdResult::NeedPoint;
        };
        let kind_flag = match self.kind {
            engine::openings::OpeningKind::Window => "W",
            engine::openings::OpeningKind::Door => "D",
            engine::openings::OpeningKind::Breakthrough => "B",
        };
        CmdResult::Dispatch(format!(
            "AEC_WALLOPENING_DO {}|{}|{},{},{}",
            wall.value(),
            kind_flag,
            pt.x,
            pt.y,
            pt.z
        ))
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// `AEC_WALLOPENING_DO handle|W|x,y,z` or `...|D|x,y,z` — non-interactive
/// handler dispatched by [`WallOpeningCommand`].
pub fn aec_wallopening_do(
    scene: &mut Scene,
    command_line: &mut CommandLine,
    args: &str,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) {
    let parts: Vec<&str> = args.split('|').collect();
    if parts.len() != 3 {
        command_line.push_error(&crate::tr!("aec", "opening-malformed-args"));
        return;
    }
    let Ok(wall_val) = parts[0].parse::<u64>() else {
        command_line.push_error(&crate::tr!("aec", "opening-malformed-handle"));
        return;
    };
    let kind = match parts[1] {
        "D" | "d" | "Door" | "door" => engine::openings::OpeningKind::Door,
        "B" | "b" | "Breakthrough" | "breakthrough" => engine::openings::OpeningKind::Breakthrough,
        _ => engine::openings::OpeningKind::Window,
    };
    let xyz: Vec<&str> = parts[2].split(',').collect();
    if xyz.len() < 2 {
        command_line.push_error(&crate::tr!("aec", "opening-malformed-point"));
        return;
    }
    let Ok(x) = xyz[0].parse::<f64>() else {
        command_line.push_error(&crate::tr!("aec", "opening-malformed-point"));
        return;
    };
    let Ok(y) = xyz[1].parse::<f64>() else {
        command_line.push_error(&crate::tr!("aec", "opening-malformed-point"));
        return;
    };
    let z = xyz.get(2).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);

    match place_wall_opening(
        scene,
        Handle::new(wall_val),
        DVec3::new(x, y, z),
        kind,
        library_override,
        display_rules,
        style_substitutions,
    ) {
        Ok((_opening, touched)) => {
            let changes: Vec<_> = touched
                .into_iter()
                .map(|h| (h, crate::scene::ChangeKind::Modified))
                .collect();
            if !changes.is_empty() {
                scene.bump_entities(&changes);
            }
            let label = kind.as_str();
            command_line.push_info(&crate::tr!("aec", "opening-placed", kind = label));
        }
        Err(e) => {
            command_line.push_error(&crate::tr!("aec", "opening-error", error = e.to_string()));
        }
    }
}


inventory::submit!(crate::command::CommandRegistration { names: &["AEC_WINDOW"] });
