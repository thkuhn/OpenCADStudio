//! AEC domain engine — pure models and helpers for walls, rooms, storeys,
//! closed-loop detection, and a minimal IFC4 SPF writer.
//!
//! Kept dependency-light (std + the rest of the crate) so the math stays
//! unit-testable without UI or command-line coupling.

pub mod geometry;
pub mod ifc;
pub mod loop_detection;
pub mod room;
pub mod storey;
pub mod wall;

pub use ifc::Scene;
pub use loop_detection::find_closed_loop;
pub use room::Room;
pub use storey::Storey;
pub use wall::Wall;
