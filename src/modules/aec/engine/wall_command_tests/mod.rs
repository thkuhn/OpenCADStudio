//! Wall scene-ops tests moved from `aec/commands.rs`.

use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

use super::display_apply::*;
use super::join_ops::*;
use super::junction_pick::*;
use super::opening_xdata::*;
use super::storey_xdata::*;
use super::wall_package::*;
use super::wall_regen::*;
use super::xdata::*;
use super::{
    self as engine, Storey, StyleLibrary, Wall, WallJustification, WallLayer,
    join::{self, JoinError, JoinKind},
    wall_style::{
        effective_layers_for_wall_bb, migrate_gap_before_to_axis_offset, LayerFunction,
        WallStyle,
    },
};

use acadrust::entities::{EntityType, LwPolyline, LwVertex};
use acadrust::types::{Vector2, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::Handle;
use glam::DVec3;
use crate::modules::aec::engine::openings::{OpeningKind, SwingSide};
use crate::modules::aec::engine::opening_style::OpeningStyle;
use crate::modules::aec::engine::plan_view::{PhaseFilter, PlanPhase};
use crate::modules::aec::engine::project::{Building, ProjectFile, StoreyRef};

use crate::command::{CadCommand, CmdResult};
use crate::modules::aec::engine::material::Material;
use crate::modules::aec::engine::style::Style;
use crate::modules::aec::engine::wall_style::{Layer, LayerValue};
use crate::modules::aec::rooms::room::aec_room;
use crate::modules::aec::styles::material_manager::MaterialCommand;
use crate::modules::aec::styles::wall_style_manager::StyleCommand;
use crate::modules::aec::walls::extend::{aec_wallextend_do, WallExtendCommand};
use crate::modules::aec::walls::join::WallJoinCommand;
use crate::modules::aec::walls::wall::WallCommand;
use crate::modules::aec::walls::wall::{DEFAULT_WALL_HEIGHT, DEFAULT_WALL_THICKNESS};
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

fn wall_xdata(entity: &EntityType) -> Option<&ExtendedDataRecord> {
    read_aec_record(entity)
}

fn wl(material: &str, thickness: f64, function: &str) -> WallLayer {
    WallLayer {
        material: material.to_string(),
        thickness,
        function: function.to_string(),
        axis_offset: -thickness * 0.5,
        bottom_offset: 0.0,
        top_offset: 0.0,
        layer_override: None,
        hatch_override: None,
    layer_id: uuid::Uuid::new_v4(),
    }
}

/// Build a multi-layer stack with centered absolute axis offsets.
fn wls(specs: &[(&str, f64, &str)]) -> Vec<WallLayer> {
    let pairs: Vec<(f64, f64)> = specs.iter().map(|(_, t, _)| (*t, 0.0)).collect();
    let offsets = migrate_gap_before_to_axis_offset(&pairs);
    specs
        .iter()
        .zip(offsets)
        .map(|(&(mat, t, fun), off)| WallLayer {
            material: mat.to_string(),
            thickness: t,
            function: fun.to_string(),
            axis_offset: off,
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: match mat {
                "Putz" => Some("DOTS".to_string()),
                "Mauerwerk" => Some("ANSI31".to_string()),
                _ => None,
            },
        layer_id: uuid::Uuid::new_v4(),
        })
        .collect()
}

mod junction_overrides;
mod x_crossing;
mod wall_draw_command;
mod phase_and_xdata;
mod regen_and_package;
mod join_and_override_prune;
mod wallextend;
mod auto_join;
mod scene_cascade;
mod reverse_and_hover;
mod junction_regen;
mod opening_scenarios;
mod storey_hatch_sloped;
mod plan_scene_consistency;
use junction_overrides::*;
use x_crossing::*;
use wall_draw_command::*;
use phase_and_xdata::*;
use regen_and_package::*;
use join_and_override_prune::*;
use wallextend::*;
use auto_join::*;
use scene_cascade::*;
use reverse_and_hover::*;
use junction_regen::*;
use opening_scenarios::*;
use storey_hatch_sloped::*;
