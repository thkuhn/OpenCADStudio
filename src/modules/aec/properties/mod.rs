//! Property-panel contributions for AEC entities.

use acadrust::{CadDocument, EntityType, Handle};

use crate::modules::aec::engine::join_ops;
use crate::modules::aec::engine::junction_pick;
use crate::modules::aec::engine::opening_display;
use crate::modules::aec::engine::opening_shape::OpeningShape;
use crate::modules::aec::engine::opening_style::{apply_style_defaults, LeafOperation};
use crate::modules::aec::engine::opening_xdata;
use crate::modules::aec::engine::openings::{
    NicheSide, Opening, OpeningKind, OpeningReferenceSide, SwingSide,
};
use crate::modules::aec::engine::room::{RoomFinish, RoomFunction};
use crate::modules::aec::engine::room_package;
use crate::modules::aec::engine::room_regen::{self, room_boundary_points};
use crate::modules::aec::engine::room_xdata;
use crate::modules::aec::engine::slab::{
    Slab, SlabJustification, SLAB_BASE_OFFSET_KEYS, SLAB_TOP_OFFSET_KEYS,
};
use crate::modules::aec::engine::slab_opening::{SlabOpening, SlabOpeningDepth, SlabOpeningKind};
use crate::modules::aec::engine::slab_package;
use crate::modules::aec::engine::slab_xdata;
use crate::modules::aec::engine::storey_xdata;
use crate::modules::aec::engine::xdata;
use crate::modules::aec::engine::wall_package;
use crate::modules::aec::engine::library::{self, StyleLibrary};
use crate::t;

mod resolve;
pub use resolve::*;
mod wall_section;
pub use wall_section::*;
mod opening;
pub use opening::*;
mod extend;
pub use extend::*;
mod carrier;
pub use carrier::*;
mod slab;
pub use slab::*;
mod room;
pub use room::*;
mod apply;
pub use apply::*;
mod grips;
pub use grips::*;
mod app_impl;

#[cfg(test)]
mod tests;
