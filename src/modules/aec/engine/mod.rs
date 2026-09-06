//! AEC domain engine — pure models and helpers for walls, rooms, storeys,
//! closed-loop detection, and a minimal IFC4 SPF writer.
//!
//! Kept dependency-light (std + the rest of the crate) so the math stays
//! unit-testable without UI or command-line coupling.

pub mod arc;
pub mod contour;
pub mod display_component;
pub mod expr;
pub mod geometry;
pub mod ifc;
pub mod join;
pub mod library;
pub mod loop_detection;
pub mod material;
pub mod miter;
pub mod openings;
pub mod owner_index;
pub mod plan_view;
pub mod project;
pub mod representation;
pub mod room;
pub mod storey;
pub mod style;
pub mod wall;
pub mod wall_style;

pub use geometry::get_offset_directions;
pub use ifc::Scene;
pub use library::StyleLibrary;
pub use loop_detection::find_closed_loop;
pub use room::Room;
pub use storey::Storey;
pub use wall::{Wall, WallJustification, WallLayer};
