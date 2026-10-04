//! AEC domain engine — pure models and helpers for walls, rooms, storeys,
//! closed-loop detection, and a minimal IFC4 SPF writer.
//!
//! Kept dependency-light (std + the rest of the crate) so the math stays
//! unit-testable without UI or command-line coupling.

pub mod arc;
pub mod contour;
pub mod control_plane;
pub mod display_apply;
pub mod display_component;
pub mod elevation_cut;
pub mod expr;
pub mod floor_transition;
pub mod geometry;
pub mod ifc;
pub mod join;
pub mod join_ops;
pub mod junction_pick;
pub mod junction_solver;
pub mod library;
pub mod loop_detection;
pub mod material;
pub mod miter;
pub mod opening_display;
pub mod opening_shape;
pub mod opening_sketch;
pub mod opening_style;
pub mod opening_xdata;
pub mod openings;
pub mod owner_index;
pub mod plan_view;
pub mod project;
pub mod representation;
pub mod room;
pub mod room_package;
pub mod room_regen;
pub mod room_xdata;
pub mod slab;
pub mod slab_opening;
pub mod slab_package;
pub mod slab_regen;
pub mod slab_style;
pub mod slab_xdata;
pub mod storey;
pub mod storey_xdata;
pub mod style;
pub mod wall;
pub mod wall_package;
pub mod wall_regen;
pub mod wall_style;
pub mod xdata;

#[cfg(test)]
mod wall_command_tests;


pub use geometry::get_offset_directions;
pub use floor_transition::{find_floor_transitions_for_room, total_transition_area, FloorTransitionZone};
pub use ifc::Scene;
pub use library::StyleLibrary;
pub use loop_detection::find_closed_loop;
pub use room::{FloorFinishOverride, Room, RoomFinish, RoomFunction};
pub use room_package::{
    all_room_carrier_handles, collect_room_display_children, expand_handles_for_room_packages,
    expand_with_room_derived_handles, is_room_carrier, is_room_derived, is_room_schedule,
    remove_room_display_children, resolve_room_package, resolve_room_package_handle,
    room_package_handles, room_stamp_handle, write_room_schedule_tag, AEC_ROOM_CARRIER_LAYER,
    AEC_ROOM_HATCH_LAYER, AEC_ROOM_SCHEDULE_LAYER, AEC_ROOM_SOLID_LAYER, AEC_ROOM_STAMP_LAYER,
};
pub use room_regen::{regenerate_room_representation, room_boundary_points};
pub use room_xdata::{room_from_entity, write_room_record};
pub use xdata::collect_wall_structural_segments;
pub use slab::{Slab, SlabJustification, SlabLayer};
pub use slab_opening::{SlabOpening, SlabOpeningDepth, SlabOpeningKind};
pub use slab_package::{
    all_slab_carrier_handles, expand_handles_for_slab_packages, expand_with_slab_derived_handles,
    is_slab_carrier_entity, is_slab_derived, resolve_slab_package, slab_opening_owner_if_any,
    slab_package_handles,
};
pub use slab_regen::{
    build_faceted_slab_solid, regenerate_all_slabs, regenerate_slab_opening_representation,
    regenerate_slab_representation,
};
pub use slab_style::{SlabStyle, SlabStyleLayer};
pub use slab_xdata::{
    add_slab_opening_handle, remove_slab_opening_handle, resolve_slab_style_layers,
    resolve_slab_style_layers_ids, slab_from_entity, slab_opening_from_entity,
    slab_opening_record, slab_record_for_slab, write_slab_justification, write_slab_opening_depth,
    write_slab_opening_kind, write_slab_opening_record, write_slab_phase, write_slab_plane_offsets,
    write_slab_record, write_slab_style,
};
pub use storey::Storey;
pub use control_plane::ControlPlane;
pub use wall::{Wall, WallJustification, WallLayer};
