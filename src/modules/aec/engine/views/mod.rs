//! AEC views module: persistent Viewpoints, ModelViews, and Provenance.

pub mod carrier;
pub mod cut_engine;
pub mod edit_ops;
pub mod elevation_engine;
pub mod frame;
pub mod floorplan_engine;
pub mod model_view;
pub mod provenance;
pub mod scene_collector;
pub mod silhouette;
pub mod style_resolver;
pub mod update_ops;
pub mod viewpoint;

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError(pub String);

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ValidationError {}

pub fn check(condition: bool, message: &str) -> Result<(), ValidationError> {
    if condition {
        Ok(())
    } else {
        Err(ValidationError(message.into()))
    }
}

macro_rules! typed_id {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash,
                 Serialize, Deserialize)]
        #[serde(try_from = "Uuid", into = "Uuid")]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self { Self(Uuid::new_v4()) }
            pub fn from_uuid(uuid: Uuid) -> Self { Self(uuid) }
            pub fn as_uuid(self) -> Uuid { self.0 }
        }
        impl Default for $name {
            fn default() -> Self { Self::new() }
        }
        impl TryFrom<Uuid> for $name {
            type Error = ValidationError;
            fn try_from(value: Uuid) -> Result<Self, Self::Error> {
                check(!value.is_nil(), concat!(stringify!($name), " must not be nil"))?;
                Ok(Self(value))
            }
        }
        impl From<$name> for Uuid {
            fn from(value: $name) -> Self { value.0 }
        }
    )+};
}

typed_id!(
    ModelId,
    ElementId,
    ComponentId,
    StoryId,
    ViewpointId,
    ModelViewId,
    ViewProfileId,
    DisplayComponentId,
    SemanticFeatureId,
    AnnotationId,
    OverrideId,
    SheetPlacementId,
    HatchDefinitionId,
);

macro_rules! finite_scalar {
    ($name:ident, $rule:expr, $message:literal) => {
        #[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(try_from = "f64", into = "f64")]
        pub struct $name(f64);
        impl $name {
            pub fn new(value: f64) -> Result<Self, ValidationError> {
                Self::try_from(value)
            }
            pub fn get(self) -> f64 {
                self.0
            }
        }
        impl TryFrom<f64> for $name {
            type Error = ValidationError;
            fn try_from(value: f64) -> Result<Self, Self::Error> {
                check(value.is_finite() && ($rule)(value), $message)?;
                Ok(Self(value))
            }
        }
        impl From<$name> for f64 {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

finite_scalar!(Metres, |_: f64| true, "metres must be finite");
impl Metres {
    pub const ZERO: Self = Self(0.0);
}

finite_scalar!(
    NonNegativeMetres,
    |v: f64| v >= 0.0,
    "distance must be finite and >= 0"
);
impl NonNegativeMetres {
    pub const ZERO: Self = Self(0.0);
}
finite_scalar!(
    PositiveMetres,
    |v: f64| v > 0.0,
    "distance must be finite and > 0"
);
finite_scalar!(
    PaperMm,
    |v: f64| v >= 0.0,
    "paper width must be finite and >= 0"
);
finite_scalar!(
    ScaleDenominator,
    |v: f64| v > 0.0,
    "scale denominator must be > 0"
);

impl ScaleDenominator {
    /// 1:50 maps one model metre to 20 paper millimetres.
    pub fn model_metres_to_paper_mm(self, distance: Metres) -> f64 {
        distance.get() * 1000.0 / self.get()
    }
}

/// Monotonic revision counter; never reset while caches may exist.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Revision(pub u64);

impl Revision {
    pub fn next(self) -> Result<Self, ValidationError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| ValidationError("revision overflow".into()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point2 {
    pub x_m: f64,
    pub y_m: f64,
}

impl Point2 {
    pub fn new(x_m: f64, y_m: f64) -> Self {
        Self { x_m, y_m }
    }

    pub fn validate(self) -> Result<(), ValidationError> {
        check(
            self.x_m.is_finite() && self.y_m.is_finite(),
            "non-finite 2D point",
        )
    }

    pub fn distance_to(self, other: Self) -> f64 {
        let dx = self.x_m - other.x_m;
        let dy = self.y_m - other.y_m;
        (dx * dx + dy * dy).sqrt()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vector2 {
    pub x: f64,
    pub y: f64,
}

impl Vector2 {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn length(self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y
    }

    pub fn normalized(self) -> Result<Self, ValidationError> {
        let len = self.length();
        check(len > 1e-9 && len.is_finite(), "vector must be non-zero and finite")?;
        Ok(Self {
            x: self.x / len,
            y: self.y / len,
        })
    }
}

impl std::ops::Neg for Vector2 {
    type Output = Self;
    fn neg(self) -> Self::Output {
        Self {
            x: -self.x,
            y: -self.y,
        }
    }
}

impl std::ops::Mul<f64> for Vector2 {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self::Output {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

impl std::ops::Mul<Vector2> for f64 {
    type Output = Vector2;
    fn mul(self, rhs: Vector2) -> Self::Output {
        Vector2 {
            x: self * rhs.x,
            y: self * rhs.y,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point3 {
    pub x_m: f64,
    pub y_m: f64,
    pub z_m: f64,
}

impl Point3 {
    pub fn new(x_m: f64, y_m: f64, z_m: f64) -> Self {
        Self { x_m, y_m, z_m }
    }

    pub fn validate(self) -> Result<(), ValidationError> {
        check(
            self.x_m.is_finite() && self.y_m.is_finite() && self.z_m.is_finite(),
            "non-finite 3D point",
        )
    }
}

/// A displacement, intentionally distinct from an absolute world point.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelDelta3 {
    pub x_m: f64,
    pub y_m: f64,
    pub z_m: f64,
}

/// Dimensionless direction/basis vector.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vector3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vector3 {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn length(self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    pub fn normalized(self) -> Result<Self, ValidationError> {
        let len = self.length();
        check(len > 1e-9 && len.is_finite(), "vector must be non-zero and finite")?;
        Ok(Self {
            x: self.x / len,
            y: self.y / len,
            z: self.z / len,
        })
    }

    pub fn validate_unit(self) -> Result<(), ValidationError> {
        check(
            (self.dot(self) - 1.0).abs() <= 1e-9,
            "axis must be a finite unit vector",
        )
    }
}

/// Axis-aligned 2D bounding rectangle in view space.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect2 {
    pub min: Point2,
    pub max: Point2,
}

impl Rect2 {
    pub fn new(min: Point2, max: Point2) -> Self {
        Self { min, max }
    }

    pub fn width(self) -> f64 {
        self.max.x_m - self.min.x_m
    }

    pub fn height(self) -> f64 {
        self.max.y_m - self.min.y_m
    }

    pub fn validate(self) -> Result<(), ValidationError> {
        self.min.validate()?;
        self.max.validate()?;
        check(
            self.min.x_m < self.max.x_m && self.min.y_m < self.max.y_m,
            "crop rectangle must have positive width and height",
        )
    }

    pub fn contains(self, point: Point2) -> bool {
        point.x_m >= self.min.x_m
            && point.x_m <= self.max.x_m
            && point.y_m >= self.min.y_m
            && point.y_m <= self.max.y_m
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaperPoint {
    pub x_mm: f64,
    pub y_mm: f64,
}

/// Paper placement on a layout sheet; moving it never modifies the 3D model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetPlacement {
    pub id: SheetPlacementId,
    pub view_id: ModelViewId,
    pub origin: PaperPoint,
    pub rotation_rad: f64,
    pub title: String,
}

impl SheetPlacement {
    pub fn validate(&self) -> Result<(), ValidationError> {
        check(
            self.origin.x_mm.is_finite()
                && self.origin.y_mm.is_finite()
                && self.rotation_rad.is_finite(),
            "non-finite sheet placement",
        )
    }
}

pub const VIEW_DOCUMENT_SCHEMA_VERSION: u32 = 1;

/// Serializable container for all viewpoints, model views, and sheet placements.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewDocument {
    pub schema_version: u32,
    #[serde(default)]
    pub viewpoints: Vec<viewpoint::Viewpoint>,
    #[serde(default)]
    pub views: Vec<model_view::ModelView>,
    #[serde(default)]
    pub placements: Vec<SheetPlacement>,
}

impl Default for ViewDocument {
    fn default() -> Self {
        Self {
            schema_version: VIEW_DOCUMENT_SCHEMA_VERSION,
            viewpoints: vec![],
            views: vec![],
            placements: vec![],
        }
    }
}

impl ViewDocument {
    pub fn validate(
        &self,
        profile_exists: impl Fn(ViewProfileId) -> bool,
        element_exists: impl Fn(provenance::ElementRef) -> bool,
        story_exists: impl Fn(viewpoint::StoryRef) -> bool,
    ) -> Result<(), ValidationError> {
        check(
            self.schema_version == VIEW_DOCUMENT_SCHEMA_VERSION,
            "unsupported view document schema; migrate before loading",
        )?;
        let mut viewpoints = BTreeSet::new();
        for viewpoint in &self.viewpoints {
            viewpoint.validate()?;
            check(viewpoints.insert(viewpoint.id), "duplicate viewpoint ID")?;
            if let viewpoint::ViewOrigin::StoryRelative { story, .. } = viewpoint.origin {
                check(story_exists(story), "unresolved story reference")?;
            }
        }
        let mut views = BTreeSet::new();
        for view in &self.views {
            view.validate()?;
            check(views.insert(view.id), "duplicate model view ID")?;
            check(
                viewpoints.contains(&view.viewpoint_id),
                "unresolved viewpoint",
            )?;
            check(profile_exists(view.profile_id), "unresolved view profile")?;
            for element in &view.excluded_elements {
                check(element_exists(*element), "unresolved excluded element")?;
            }
        }
        for placement in &self.placements {
            placement.validate()?;
            check(
                views.contains(&placement.view_id),
                "unresolved view in sheet placement",
            )?;
        }
        Ok(())
    }
}

pub use cut_engine::{derive_section_view, CutEngineOptions};
pub use edit_ops::{
    apply_semantic_mutation, commit_view_drag_and_update, resolve_handle_drag, EditOpError,
    SemanticMutation,
};
pub use elevation_engine::{derive_elevation_view, ElevationEngineOptions};
pub use model_view::{
    DetailLevel, DetailPolicy, GraphicOverride, GraphicPatch, ModelView, OverrideSelector,
    StyleOverride, UpdatePolicy, ViewRevisions,
};
pub use provenance::{
    ComponentFace, DepthInterval, DerivedPrimitive, EditHandle, ElementRef, FeatureKey,
    Geometry2d, ModelEditAction, ModelParameter, PrimitiveId, PrimitiveRole, ResolvedStyle, Rgba8,
    SourceReference, SourceSet, XrefSourceRef,
};
pub use scene_collector::{
    CollectedOpening, CollectedSlab, CollectedSlabLayer, CollectedWall, CollectedWallLayer,
    SceneGeometryCollector,
};
pub use silhouette::{process_silhouette_and_depth, SilhouetteOptions};
pub use viewpoint::{
    CeilingRange, DepthRange, PlanRange, ProjectionMapping, SectionPath, SectionSegment,
    StoryRef, VerticalRange, ViewAxes, ViewKind, ViewOrigin, Viewpoint,
};

#[cfg(test)]
mod tests;
