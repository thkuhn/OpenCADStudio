//! AEC Style Library for persisting materials and wall styles.

use crate::modules::aec::engine::display_component::{
    ComponentRuleSet, ComponentStyleOverride, LayerSelection, LayerStyleOverride,
    RepresentationMode, SlabComponentSlot, StyleDisplayOverlay, WallComponentKind,
    WallComponentSlot,
};
use crate::modules::aec::engine::join::LayerRef;
use crate::modules::aec::engine::material::Material;
use crate::modules::aec::engine::opening_style::OpeningStyle;
use crate::modules::aec::engine::plan_view::{
    DetailProfile, DisplayConfig, LayerDetail, ScaleDisplayConfigMapping,
};
use crate::modules::aec::engine::section_profile::{SectionProfile, SectionShape};
use crate::modules::aec::engine::slab::Slab;
use crate::modules::aec::engine::slab_style::{
    FloorFinishStyle, SlabStructuralStyle, SlabStyle, SlabStyleLayer,
};
use crate::modules::aec::engine::style::Style;
use crate::modules::aec::engine::style_common::{StyleKindOps, StyleOrigin};
use crate::modules::aec::engine::wall::{Wall, WallLayer};
use crate::modules::aec::engine::wall_style::{LayerValue, Layer, LayerFunction, WallStyle};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::path::PathBuf;

/// A collection of AEC materials, styles, and section profiles.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct StyleLibrary {
    /// List of available materials.
    pub materials: Vec<Material>,
    /// List of available wall styles.
    pub wall_styles: Vec<WallStyle>,
    /// Opening styles (window / door / breakthrough). Absent in legacy files.
    #[serde(default)]
    pub opening_styles: Vec<OpeningStyle>,
    /// Slab styles (floors / ceilings / roofs). Absent in legacy files.
    #[serde(default)]
    pub slab_styles: Vec<SlabStyle>,
    /// Structural slab styles (structural core layers). Absent in legacy files.
    #[serde(default)]
    pub slab_structural_styles: Vec<SlabStructuralStyle>,
    /// Floor finish styles (Fußbodenaufbauten). Absent in legacy files.
    #[serde(default)]
    pub floor_finish_styles: Vec<FloorFinishStyle>,
    /// Reusable component cross-sections. Absent in legacy files.
    #[serde(default)]
    pub profiles: Vec<SectionProfile>,
}

impl StyleLibrary {
    /// Stamps `origin` on every style / material of this library (the origin
    /// is derived from where a library lives, never stored).
    pub fn stamp_origin(&mut self, origin: StyleOrigin) {
        use crate::modules::aec::engine::style_common::StyleKindOps;
        for m in &mut self.materials {
            m.style.origin = origin;
        }
        for s in &mut self.wall_styles {
            s.common_mut().origin = origin;
        }
        for s in &mut self.opening_styles {
            s.common_mut().origin = origin;
        }
        for s in &mut self.slab_styles {
            s.common_mut().origin = origin;
        }
        for s in &mut self.slab_structural_styles {
            s.common_mut().origin = origin;
        }
        for s in &mut self.floor_finish_styles {
            s.common_mut().origin = origin;
        }
        for profile in &mut self.profiles {
            profile.style.origin = origin;
        }
    }
}

/// A node in a hierarchical wall-style tree.
pub struct TreeNode<'a> {
    /// The wall style at this node.
    pub style: &'a WallStyle,
    /// Indentation depth (0 for roots).
    pub depth: usize,
}

/// A node in a hierarchical opening-style tree.
pub struct OpeningTreeNode<'a> {
    pub style: &'a OpeningStyle,
    pub depth: usize,
}

/// A node in a hierarchical slab-style tree.
pub struct SlabTreeNode<'a> {
    pub style: &'a SlabStyle,
    pub depth: usize,
}

/// Conflict status when copying an entry from one library to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CopyConflict {
    /// No entry with the same ID exists in the target library.
    None,
    /// An entry with the same ID already exists and its content is identical.
    IdenticalAlreadyPresent,
    /// An entry with the same ID already exists but its content differs.
    DifferentContentCollision,
}

/// Where a style-library entry originates in the combined Standard+Project view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibrarySource {
    /// Machine-wide global / standard library (`aec_styles.toml`).
    Standard,
    /// Embedded library of the active `.ocsproj`.
    Project,
    /// In-memory reconstruction from the current drawing (not persisted).
    Session,
}

/// A material shown in the combined Standard+Project list, with provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct CombinedMaterialEntry {
    pub material: Material,
    pub source: LibrarySource,
}

/// A wall style shown in the combined Standard+Project list, with provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct CombinedWallStyleEntry {
    pub wall_style: WallStyle,
    pub source: LibrarySource,
}

/// An opening style shown in the combined Standard+Project list, with provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct CombinedOpeningStyleEntry {
    pub opening_style: OpeningStyle,
    pub source: LibrarySource,
}

/// A slab style shown in the combined Standard+Project list, with provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct CombinedSlabStyleEntry {
    pub slab_style: SlabStyle,
    pub source: LibrarySource,
}

/// A structural slab style shown in the combined Standard+Project list, with provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct CombinedSlabStructuralStyleEntry {
    pub structural_style: SlabStructuralStyle,
    pub source: LibrarySource,
}

/// A floor finish style shown in the combined Standard+Project list, with provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct CombinedFloorFinishStyleEntry {
    pub finish_style: FloorFinishStyle,
    pub source: LibrarySource,
}

/// A section profile shown in the combined Standard+Project list, with provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct CombinedSectionProfileEntry {
    pub profile: SectionProfile,
    pub source: LibrarySource,
}

mod combined;
pub use combined::*;
mod rules;
pub use rules::*;
mod impl_ops;
mod extract;
pub use extract::*;
mod seed;
pub use seed::*;
mod persist;
pub use persist::*;
mod display_config;
pub use display_config::*;
#[cfg(test)]
mod tests;
