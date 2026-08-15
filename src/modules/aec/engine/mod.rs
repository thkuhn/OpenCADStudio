//! AEC domain engine — pure models and helpers for walls, rooms, storeys,
//! closed-loop detection, and a minimal IFC4 SPF writer.
//!
//! Kept dependency-light (std + the rest of the crate) so the math stays
//! unit-testable without UI or command-line coupling.

pub mod arc;
pub mod contour;
pub mod expr;
pub mod geometry;
pub mod ifc;
pub mod join;
pub mod library;
pub mod loop_detection;
pub mod material;
pub mod miter;
pub mod openings;
pub mod representation;
pub mod room;
pub mod storey;
pub mod style;
pub mod wall;
pub mod wall_style;

pub use arc::{AxisSegment, CircularArc};
pub use geometry::{get_offset_directions, Polygon2D};
pub use ifc::Scene;
pub use library::StyleLibrary;
pub use loop_detection::find_closed_loop;
pub use material::Material;
pub use openings::{Opening, OpeningKind};
pub use room::Room;
pub use storey::Storey;
pub use style::Style;
pub use wall::Wall;
pub use wall_style::WallStyle;
