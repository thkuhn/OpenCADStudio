//! AEC views module: persistent Viewpoints, ModelViews, and Provenance.

pub mod carrier;
pub mod cut_engine;
pub mod edit_ops;
pub mod elevation_engine;
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
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use crate::modules::aec::engine::openings::OpeningKind;
    use crate::modules::aec::engine::opening_style::HingeSide;

    #[test]
    fn test_view_document_validation_and_serde() {
        let vp_id = ViewpointId::new();
        let profile_id = ViewProfileId::new();
        let elem = ElementRef::new(ModelId::new(), ElementId::new());
        let story = StoryRef::new(ModelId::new(), StoryId::new());

        let vp = Viewpoint {
            id: vp_id,
            name: "Schnitt 1".into(),
            revision: Revision(1),
            origin: ViewOrigin::StoryRelative {
                story,
                xy_world: Point2::new(0.0, 0.0),
                offset: Metres::new(1.0).unwrap(),
            },
            axes: ViewAxes::front_elevation(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
                far_depth: None,
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };

        let mut excluded = BTreeSet::new();
        excluded.insert(elem);

        let mv = ModelView {
            id: ModelViewId::new(),
            name: "Ausfuehrungsplan 1:50".into(),
            viewpoint_id: vp_id,
            profile_id,
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: excluded,
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let doc = ViewDocument {
            schema_version: VIEW_DOCUMENT_SCHEMA_VERSION,
            viewpoints: vec![vp],
            views: vec![mv],
            placements: vec![],
        };

        // Validation with matching predicates
        assert!(doc
            .validate(
                |p| p == profile_id,
                |e| e == elem,
                |s| s == story,
            )
            .is_ok());

        // Serde roundtrip JSON
        let json = serde_json::to_string(&doc).unwrap();
        let deserialized: ViewDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, deserialized);
    }

    #[test]
    fn test_multi_storey_xref_scene_aggregation() {
        let model_id = ModelId::new();
        let mut collector = SceneGeometryCollector::new();

        // 1. Native Ground Floor Wall (EG, Z = 0.0m)
        let eg_wall = CollectedWall {
            handle: acadrust::Handle::new(10),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall_24cm".into(),
            height: 2.8,
            base_z: 0.0,
            top_z: 2.8,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };

        // 2. Referenced Upper Floor Wall from XREF (OG, Z_offset = 2.8m)
        let xref_og = XrefSourceRef::with_translation("Drawing_OG", Some("OG.dwg".into()), 0.0, 0.0, 2.8);
        let og_wall = CollectedWall {
            handle: acadrust::Handle::new(20),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: Some(xref_og),
            style_id: "Wall_24cm".into(),
            height: 2.8,
            base_z: 2.8,
            top_z: 5.6,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Brick".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };

        collector.walls.push(eg_wall);
        collector.walls.push(og_wall);

        assert_eq!(collector.walls.len(), 2);
        assert_eq!(collector.walls[0].base_z, 0.0);
        assert_eq!(collector.walls[1].base_z, 2.8);
        assert!(collector.walls[1].xref_source.is_some());
    }

    #[test]
    fn test_straight_section_cut_multi_layer_wall() {
        let model_id = ModelId::new();
        let wall_id = ElementId::new();
        let elem_ref = ElementRef::new(model_id, wall_id);

        let mut collector = SceneGeometryCollector::new();
        let wall = CollectedWall {
            handle: acadrust::Handle::new(100),
            element_ref: elem_ref,
            xref_source: None,
            style_id: "Exterior_36cm".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(5.0, -5.0), Point2::new(5.0, 5.0)], // Wall runs along Y at X=5.0
            layers: vec![
                CollectedWallLayer {
                    material: "Plaster".into(),
                    thickness: 0.02,
                    function: "Finish".into(),
                    axis_offset: 0.0,
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    hatch_override: None,
                    layer_id: Uuid::new_v4(),
                    component_id: ComponentId::new(),
                    is_structural: false,
                },
                CollectedWallLayer {
                    material: "Insulation".into(),
                    thickness: 0.14,
                    function: "Thermal".into(),
                    axis_offset: 0.0,
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    hatch_override: None,
                    layer_id: Uuid::new_v4(),
                    component_id: ComponentId::new(),
                    is_structural: false,
                },
                CollectedWallLayer {
                    material: "Concrete".into(),
                    thickness: 0.20,
                    function: "Structural".into(),
                    axis_offset: 0.0,
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    hatch_override: None,
                    layer_id: Uuid::new_v4(),
                    component_id: ComponentId::new(),
                    is_structural: true,
                },
            ],
            total_thickness: 0.36,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        collector.walls.push(wall);

        // Section cuts across wall at Y = 0 from X = 0 to X = 10
        let vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Schnitt A-A".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::section(Vector2::new(1.0, 0.0)).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
                far_depth: None,
            },
            z_range: VerticalRange::new(Metres::new(-1.0).unwrap(), Metres::new(5.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };

        let mv = ModelView {
            id: ModelViewId::new(),
            name: "Schnitt 1:50".into(),
            viewpoint_id: vp.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let opts = CutEngineOptions::default();
        let primitives = derive_section_view(&vp, &mv, &collector, &opts).expect("cut derivation failed");

        // Verify generated primitives
        assert!(!primitives.is_empty(), "should generate cut primitives");
        let cut_fills: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::CutFill).collect();
        assert_eq!(cut_fills.len(), 3, "should produce 3 cut layer fills for 3 layers");

        let cut_boundaries: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::CutBoundary).collect();
        assert!(!cut_boundaries.is_empty(), "should produce cut boundary lines");

        // Top line should have wall top feature and edit handle
        let top_prims: Vec<_> = cut_boundaries
            .iter()
            .filter(|p| p.sources.as_slice().iter().any(|s| s.feature == Some(FeatureKey::WallTop)))
            .collect();
        assert!(!top_prims.is_empty());
        assert!(!top_prims[0].edit_handles.is_empty(), "wall top cut line should provide an edit handle");
    }

    #[test]
    fn test_jogged_section_cut() {
        let model_id = ModelId::new();
        let mut collector = SceneGeometryCollector::new();

        // Wall 1 at X = 2.0 (Y from -5 to 5)
        collector.walls.push(CollectedWall {
            handle: acadrust::Handle::new(1),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall1".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(2.0, -5.0), Point2::new(2.0, 5.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        });

        // Wall 2 at X = 8.0 (Y from -5 to 5)
        collector.walls.push(CollectedWall {
            handle: acadrust::Handle::new(2),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall2".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(8.0, -5.0), Point2::new(8.0, 5.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        });

        // Jogged Section: Segment 1 (0..5 at Y=0), Step (X=5, Y=0..2), Segment 2 (5..10 at Y=2)
        let s1 = SectionSegment::new(Point2::new(0.0, 0.0), Point2::new(5.0, 0.0), false);
        let s2 = SectionSegment::new(Point2::new(5.0, 0.0), Point2::new(5.0, 2.0), true);
        let s3 = SectionSegment::new(Point2::new(5.0, 2.0), Point2::new(10.0, 2.0), false);
        let jogged_path = SectionPath::jogged(vec![s1, s2, s3]);

        let vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Jogged Section".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::section(Vector2::new(1.0, 0.0)).unwrap(),
            kind: ViewKind::Section {
                path: jogged_path,
                far_depth: None,
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(4.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };

        let mv = ModelView {
            id: ModelViewId::new(),
            name: "Jogged 1:50".into(),
            viewpoint_id: vp.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let primitives = derive_section_view(&vp, &mv, &collector, &CutEngineOptions::default()).unwrap();
        let fills: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::CutFill).collect();
        assert_eq!(fills.len(), 2, "both walls along jogged segments should be cut");
    }

    #[test]
    fn test_section_cut_with_opening_and_handles() {
        let model_id = ModelId::new();
        let mut collector = SceneGeometryCollector::new();

        let opening = CollectedOpening {
            handle: acadrust::Handle::new(50),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            host_wall_handle: acadrust::Handle::new(10),
            kind: OpeningKind::Window,
            style_id: Some("Window_100x120".into()),
            distance_along_axis: 5.0,
            width: 1.2,
            height: 1.4,
            sill_height: 0.9,
            spring_height: 0.0,
            shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
            hinge: HingeSide::Left,
            reference_side: crate::modules::aec::engine::openings::OpeningReferenceSide::Center,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(5.0, 0.0, 1.6),
            axis_span_local: (4.4, 5.6),
        };

        // Wall from (0,0) to (10,0) with window centered at (5,0)
        let wall = CollectedWall {
            handle: acadrust::Handle::new(10),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![opening],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        collector.walls.push(wall);

        // Section cuts directly across the window at X = 5.0 (Y from -3 to 3)
        let vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Schnitt Window".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(5.0, -3.0, 0.0)),
            axes: ViewAxes::section(Vector2::new(0.0, 1.0)).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(5.0, -3.0), Point2::new(5.0, 3.0)),
                far_depth: None,
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(4.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };

        let mv = ModelView {
            id: ModelViewId::new(),
            name: "Cut View".into(),
            viewpoint_id: vp.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let primitives = derive_section_view(&vp, &mv, &collector, &CutEngineOptions::default()).unwrap();

        // Check sill remnant and head remnant fills
        let fills: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::CutFill).collect();
        assert_eq!(fills.len(), 2, "should produce sill remnant and head remnant fills");

        // Check opening cut sill and head lines with edit handles
        let opening_prims: Vec<_> = primitives
            .iter()
            .filter(|p| p.sources.as_slice().iter().any(|s| s.feature == Some(FeatureKey::OpeningSill) || s.feature == Some(FeatureKey::OpeningHead)))
            .collect();
        assert!(!opening_prims.is_empty(), "should emit opening cut lines");
        assert!(opening_prims.iter().any(|p| !p.edit_handles.is_empty()), "opening cut lines should provide edit handles");
    }

    #[test]
    fn test_structural_only_filter_in_section() {
        let model_id = ModelId::new();
        let mut collector = SceneGeometryCollector::new();

        let wall = CollectedWall {
            handle: acadrust::Handle::new(10),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(5.0, -5.0), Point2::new(5.0, 5.0)],
            layers: vec![
                CollectedWallLayer {
                    material: "Insulation".into(),
                    thickness: 0.10,
                    function: "Thermal".into(),
                    axis_offset: 0.0,
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    hatch_override: None,
                    layer_id: Uuid::new_v4(),
                    component_id: ComponentId::new(),
                    is_structural: false,
                },
                CollectedWallLayer {
                    material: "Concrete".into(),
                    thickness: 0.20,
                    function: "Structural".into(),
                    axis_offset: 0.0,
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    hatch_override: None,
                    layer_id: Uuid::new_v4(),
                    component_id: ComponentId::new(),
                    is_structural: true,
                },
            ],
            total_thickness: 0.30,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        collector.walls.push(wall);

        let vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Schnitt".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::section(Vector2::new(1.0, 0.0)).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
                far_depth: None,
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(4.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };

        let mv = ModelView {
            id: ModelViewId::new(),
            name: "Schalplan".into(),
            viewpoint_id: vp.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let mut opts = CutEngineOptions::default();
        opts.structural_only = true;

        let primitives = derive_section_view(&vp, &mv, &collector, &opts).unwrap();
        let fills: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::CutFill).collect();
        assert_eq!(fills.len(), 1, "structural-only mode should filter non-structural insulation layer");
    }

    #[test]
    fn test_elevation_view_derivation_with_opening_slots() {
        let model_id = ModelId::new();
        let mut collector = SceneGeometryCollector::new();

        let opening = CollectedOpening {
            handle: acadrust::Handle::new(50),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            host_wall_handle: acadrust::Handle::new(10),
            kind: OpeningKind::Window,
            style_id: Some("Window".into()),
            distance_along_axis: 5.0,
            width: 1.2,
            height: 1.4,
            sill_height: 0.9,
            spring_height: 0.0,
            shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
            hinge: HingeSide::Left,
            reference_side: crate::modules::aec::engine::openings::OpeningReferenceSide::Center,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(5.0, 0.0, 1.6),
            axis_span_local: (4.4, 5.6),
        };

        let wall = CollectedWall {
            handle: acadrust::Handle::new(10),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![opening],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        collector.walls.push(wall);

        let vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Ansicht Sued".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(0.0, -5.0, 0.0)),
            axes: ViewAxes::front_elevation(),
            kind: ViewKind::Elevation {
                depth: DepthRange {
                    near: NonNegativeMetres::new(0.0).unwrap(),
                    far: Some(PositiveMetres::new(20.0).unwrap()),
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(5.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };

        let mv = ModelView {
            id: ModelViewId::new(),
            name: "Ansicht 1:100".into(),
            viewpoint_id: vp.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(100.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let opts = ElevationEngineOptions::default();
        let primitives = derive_elevation_view(&vp, &mv, &collector, &opts).unwrap();

        assert!(!primitives.is_empty(), "should derive elevation primitives");

        // Verify ElevationContour2D (frame rectangle)
        let frame_prims: Vec<_> = primitives
            .iter()
            .filter(|p| matches!(p.geometry, Geometry2d::Region { .. }))
            .collect();
        assert!(!frame_prims.is_empty(), "should emit window elevation frame contour");

        // Verify ElevationSwing2D (DIN swing triangle)
        let swing_prims: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::Symbol).collect();
        assert!(!swing_prims.is_empty(), "should emit DIN opening swing symbol");

        // Verify ElevationSill2D (sill line)
        let sill_prims: Vec<_> = primitives
            .iter()
            .filter(|p| p.sources.as_slice().iter().any(|s| s.feature == Some(FeatureKey::OpeningSill)))
            .collect();
        assert!(!sill_prims.is_empty(), "should emit sill projection line");
    }

    #[test]
    fn test_silhouette_depth_processing() {
        let view_id = ModelViewId::new();
        let source = SourceReference::new(ElementRef::new(ModelId::new(), ElementId::new()));

        let p1 = DerivedPrimitive {
            id: PrimitiveId::new(view_id, 1, 1),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Line { start: Point2::new(0.0, 0.0), end: Point2::new(1.0, 0.0) },
            sources: SourceSet::new(vec![source.clone()]).unwrap(),
            depth: DepthInterval::new(5.0, 5.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.35).unwrap()),
            edit_handles: vec![],
        };

        let p2 = DerivedPrimitive {
            id: PrimitiveId::new(view_id, 1, 2),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Line { start: Point2::new(0.0, 0.0), end: Point2::new(1.0, 0.0) },
            sources: SourceSet::new(vec![source]).unwrap(),
            depth: DepthInterval::new(0.0, 0.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.35).unwrap()),
            edit_handles: vec![],
        };

        let mut list = vec![p1, p2];
        process_silhouette_and_depth(&mut list, &SilhouetteOptions::default()).unwrap();

        // Closer primitive should be sorted first
        assert_eq!(list[0].depth.min_m, 0.0);
        assert_eq!(list[1].depth.min_m, 5.0);

        // Closer primitive keeps 0.35 mm; background primitive (depth > 0) has line weight adjusted to 0.18 mm
        assert_eq!(list[0].style.line_weight_mm.get(), 0.35);
        assert_eq!(list[1].style.line_weight_mm.get(), 0.18);
    }

    #[test]
    fn test_section_full_cut_boundaries_and_background_projection() {
        let model_id = ModelId::new();
        let mut collector = SceneGeometryCollector::new();

        // 1. Cut Wall (intersected by section at y = 0)
        let cut_wall = CollectedWall {
            handle: acadrust::Handle::new(10),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall_24cm".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, -2.0), Point2::new(0.0, 2.0)],
            layers: vec![CollectedWallLayer {
                material: "Masonry".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        collector.walls.push(cut_wall);

        // 2. Background Wall (behind the section at y = 4.0m)
        let bg_wall = CollectedWall {
            handle: acadrust::Handle::new(20),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall_24cm".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(-5.0, 4.0), Point2::new(5.0, 4.0)],
            layers: vec![CollectedWallLayer {
                material: "Masonry".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        collector.walls.push(bg_wall);

        let vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Schnitt A-A".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(-5.0, 0.0, 0.0)),
            axes: ViewAxes::section_with_side(Vector2::new(1.0, 0.0), false).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(-5.0, 0.0), Point2::new(5.0, 0.0)),
                far_depth: Some(PositiveMetres::new(10.0).unwrap()),
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(5.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };

        let mv = ModelView {
            id: ModelViewId::new(),
            name: "Ausfuehrung 1:50".into(),
            viewpoint_id: vp.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let primitives = derive_section_view(&vp, &mv, &collector, &CutEngineOptions::default()).unwrap();

        // 1. Verify cut boundary contains full closed lines (top, bottom, left jamb, right jamb)
        let cut_boundaries: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::CutBoundary).collect();
        assert!(cut_boundaries.len() >= 4, "should have at least 4 boundary edges for the cut wall (top, bot, left, right), found {}", cut_boundaries.len());

        // 2. Verify cut fill hatch exists
        let cut_fills: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::CutFill).collect();
        assert!(!cut_fills.is_empty(), "should have cut fill hatch");

        // 3. Verify background wall projected edges exist
        let bg_edges: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::ProjectedEdge).collect();
        assert!(!bg_edges.is_empty(), "should have background projected edges from the wall at y = 4.0m");
    }

    #[test]
    fn test_section_occlusion_behind_opaque_cut_wall() {
        let model_id = ModelId::new();
        let mut collector = SceneGeometryCollector::new();

        // 1. Cut Wall at x = 0, y from -2 to 2 (thickness 0.24, base 0, top 3)
        let cut_wall = CollectedWall {
            handle: acadrust::Handle::new(10),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall_24cm".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, -2.0), Point2::new(0.0, 2.0)],
            layers: vec![CollectedWallLayer {
                material: "Masonry".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        collector.walls.push(cut_wall);

        // 2. Background Wall at y = 4.0m, x from -5 to 5 (top_z = 3.0)
        let bg_wall = CollectedWall {
            handle: acadrust::Handle::new(20),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall_24cm".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(-5.0, 4.0), Point2::new(5.0, 4.0)],
            layers: vec![CollectedWallLayer {
                material: "Masonry".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        collector.walls.push(bg_wall);

        let vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Schnitt Occlusion".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(-5.0, 0.0, 0.0)),
            axes: ViewAxes::section_with_side(Vector2::new(1.0, 0.0), false).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(-5.0, 0.0), Point2::new(5.0, 0.0)),
                far_depth: Some(PositiveMetres::new(10.0).unwrap()),
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(5.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };

        let mv = ModelView {
            id: ModelViewId::new(),
            name: "Ausfuehrung 1:50".into(),
            viewpoint_id: vp.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let primitives = derive_section_view(&vp, &mv, &collector, &CutEngineOptions::default()).unwrap();

        // In viewpoint coordinates, section path starts at x=-5 (u=0.0).
        // The cut wall is at world x=0, so unfolded u = 5.0m (span [4.88, 5.12]).
        // Background wall line at z=3.0 was running continuously across u ∈ [0, 10].
        // Occlusion clipping against the opaque CutFill should remove the segment behind the cut wall!
        for prim in &primitives {
            if prim.role == PrimitiveRole::ProjectedEdge {
                if let Geometry2d::Line { start, end } = prim.geometry {
                    // Check if any projected line segment crosses the center of the cut wall (u = 5.0, z = 1.5)
                    let min_u = start.x_m.min(end.x_m);
                    let max_u = start.x_m.max(end.x_m);
                    assert!(
                        !(min_u < 4.95 && max_u > 5.05),
                        "Background edge [{:?} -> {:?}] must not span across the opaque cut wall at u = 5.0",
                        start,
                        end
                    );
                }
            }
        }
    }

    #[test]
    fn test_section_occlusion_in_hatchless_mode() {
        let model_id = ModelId::new();
        let mut collector = SceneGeometryCollector::new();

        // 1. Cut Wall at x = 0, y from -2 to 2 (thickness 0.24, base 0, top 3)
        let cut_wall = CollectedWall {
            handle: acadrust::Handle::new(10),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall_24cm".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, -2.0), Point2::new(0.0, 2.0)],
            layers: vec![CollectedWallLayer {
                material: "Masonry".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        collector.walls.push(cut_wall);

        // 2. Background Wall at y = 4.0m, x from -5 to 5 (top_z = 3.0)
        let bg_wall = CollectedWall {
            handle: acadrust::Handle::new(20),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall_24cm".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(-5.0, 4.0), Point2::new(5.0, 4.0)],
            layers: vec![CollectedWallLayer {
                material: "Masonry".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        collector.walls.push(bg_wall);

        let vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Schnitt Occlusion Hatchless".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(-5.0, 0.0, 0.0)),
            axes: ViewAxes::section_with_side(Vector2::new(1.0, 0.0), false).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(-5.0, 0.0), Point2::new(5.0, 0.0)),
                far_depth: Some(PositiveMetres::new(10.0).unwrap()),
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(5.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };

        let mv = ModelView {
            id: ModelViewId::new(),
            name: "Entwurf 1:100".into(),
            viewpoint_id: vp.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(100.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let mut dc = crate::modules::aec::engine::plan_view::DisplayConfig::new(
            "Entwurf 1:100".to_string(),
            "Architektur".to_string(),
            crate::modules::aec::engine::plan_view::PlanningStage::Design,
            crate::modules::aec::engine::plan_view::ViewType::Section,
        );
        dc.detail.hatches = false;

        let primitives = derive_section_view(&vp, &mv, &collector, &CutEngineOptions {
            display_config: Some(dc),
            ..CutEngineOptions::default()
        }).unwrap();

        // 1. Verify that NO cut fill hatch exists in the final output
        let cut_fills: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::CutFill).collect();
        assert!(cut_fills.is_empty(), "When hatches are disabled, no CutFill should be output");

        // 2. Verify that occlusion masks were still active: background edge does not span across cut wall
        let projected_edges = primitives
            .iter()
            .filter(|p| p.role == PrimitiveRole::ProjectedEdge)
            .count();
        assert!(
            projected_edges > 0,
            "background wall must still contribute clipped projected edges (otherwise the occlusion check is vacuous)"
        );
        for prim in &primitives {
            if prim.role == PrimitiveRole::ProjectedEdge {
                if let Geometry2d::Line { start, end } = prim.geometry {
                    let min_u = start.x_m.min(end.x_m);
                    let max_u = start.x_m.max(end.x_m);
                    assert!(
                        !(min_u < 4.95 && max_u > 5.05),
                        "Background edge [{:?} -> {:?}] must not span across the cut wall at u = 5.0 even without hatches",
                        start,
                        end
                    );
                }
            }
        }
    }

    fn section_style_fixture(wall_axis: [(f64, f64); 2]) -> (Viewpoint, ModelView, SceneGeometryCollector) {
        let model_id = ModelId::new();
        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(CollectedWall {
            handle: acadrust::Handle::new(10),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall_Style".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![
                Point2::new(wall_axis[0].0, wall_axis[0].1),
                Point2::new(wall_axis[1].0, wall_axis[1].1),
            ],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        });
        let vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Schnitt Style".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(-5.0, 0.0, 0.0)),
            axes: ViewAxes::section_with_side(Vector2::new(1.0, 0.0), false).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(-5.0, 0.0), Point2::new(5.0, 0.0)),
                far_depth: Some(PositiveMetres::new(10.0).unwrap()),
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(5.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };
        let mv = ModelView {
            id: ModelViewId::new(),
            name: "Schnitt Style 1:50".into(),
            viewpoint_id: vp.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };
        (vp, mv, collector)
    }

    #[test]
    fn test_section_material_pattern_scale_angle_reach_cut_fill() {
        // Wall runs along +Y (axis angle 90°) and is cut by a section along +X.
        let (vp, mv, collector) = section_style_fixture([(0.0, -2.0), (0.0, 2.0)]);
        let mut lib = crate::modules::aec::engine::library::StyleLibrary::default();
        let mut mat = crate::modules::aec::engine::material::Material::new(
            "Concrete".to_string(),
            "Stahlbeton".to_string(),
            "AR-CONC".to_string(),
            0x303030,
            "Continuous".to_string(),
        );
        mat.hatch_scale = 0.5;
        mat.hatch_angle = 45.0;
        mat.hatch_angle_relative = true;
        lib.materials.push(mat);

        let primitives = derive_section_view(
            &vp,
            &mv,
            &collector,
            &CutEngineOptions {
                style_library: Some(lib),
                ..CutEngineOptions::default()
            },
        )
        .unwrap();
        let fill = primitives
            .iter()
            .find(|p| p.role == PrimitiveRole::CutFill)
            .expect("cut fill");
        assert_eq!(fill.style.hatch_pattern.as_deref(), Some("AR-CONC"));
        assert!((fill.style.hatch_scale.unwrap() - 0.5).abs() < 1e-9);
        assert!(
            (fill.style.hatch_angle_rad.unwrap() - 45f64.to_radians()).abs() < 1e-9,
            "section hatch angle must not inherit the plan-relative wall direction"
        );
        assert_eq!(fill.style.layer_name.as_deref(), Some("AEC_SECTION_HATCH"));
        let boundary = primitives
            .iter()
            .find(|p| p.role == PrimitiveRole::CutBoundary)
            .expect("cut boundary");
        assert_eq!(boundary.style.layer_name.as_deref(), Some("AEC_SECTION_CUT"));
    }

    #[test]
    fn test_section_does_not_need_section_typed_display_config() {
        // The view kind is explicit: a FloorPlan-typed Planart must not turn a section into a plan.
        let (vp, mv, collector) = section_style_fixture([(0.0, -2.0), (0.0, 2.0)]);
        let dc = crate::modules::aec::engine::plan_view::DisplayConfig::new(
            "Architektur 1:50".to_string(),
            "Architektur".to_string(),
            crate::modules::aec::engine::plan_view::PlanningStage::Design,
            crate::modules::aec::engine::plan_view::ViewType::FloorPlan,
        );
        let primitives = derive_section_view(
            &vp,
            &mv,
            &collector,
            &CutEngineOptions {
                display_config: Some(dc),
                ..CutEngineOptions::default()
            },
        )
        .unwrap();
        assert!(primitives
            .iter()
            .filter(|p| matches!(p.role, PrimitiveRole::CutFill | PrimitiveRole::CutBoundary))
            .all(|p| p
                .style
                .layer_name
                .as_deref()
                .map_or(false, |l| l.starts_with("AEC_SECTION"))));
    }

    #[test]
    fn test_section_hidden_lines_emitted_when_option_on() {
        let model_id = ModelId::new();
        let mut collector = SceneGeometryCollector::new();

        // 1. Cut Wall at x = 0, y from -2 to 2 (thickness 0.24, base 0, top 3)
        let cut_wall = CollectedWall {
            handle: acadrust::Handle::new(10),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall_24cm".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, -2.0), Point2::new(0.0, 2.0)],
            layers: vec![CollectedWallLayer {
                material: "Masonry".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        collector.walls.push(cut_wall);

        // 2. Background Wall at y = 4.0m, x from -5 to 5 (top_z = 3.0)
        let bg_wall = CollectedWall {
            handle: acadrust::Handle::new(20),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall_24cm".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(-5.0, 4.0), Point2::new(5.0, 4.0)],
            layers: vec![CollectedWallLayer {
                material: "Masonry".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![CollectedOpening {
                handle: acadrust::Handle::new(21),
                element_ref: ElementRef::new(model_id, ElementId::new()),
                xref_source: None,
                host_wall_handle: acadrust::Handle::new(20),
                kind: OpeningKind::Window,
                style_id: Some("Window_200x120".into()),
                distance_along_axis: 5.0,
                width: 2.0,
                height: 1.2,
                sill_height: 0.9,
                spring_height: 0.0,
                shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
                hinge: HingeSide::Left,
                reference_side: crate::modules::aec::engine::openings::OpeningReferenceSide::Center,
                cross_axis_offset: 0.0,
                depth: None,
                world_center: Point3::new(0.0, 4.0, 1.5),
                axis_span_local: (4.0, 6.0),
            }],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        collector.walls.push(bg_wall);

        let vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Schnitt Hidden Lines".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(-5.0, 0.0, 0.0)),
            axes: ViewAxes::section_with_side(Vector2::new(1.0, 0.0), false).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(-5.0, 0.0), Point2::new(5.0, 0.0)),
                far_depth: Some(PositiveMetres::new(10.0).unwrap()),
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(5.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };

        let mv = ModelView {
            id: ModelViewId::new(),
            name: "Ausfuehrung 1:50".into(),
            viewpoint_id: vp.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let primitives = derive_section_view(
            &vp,
            &mv,
            &collector,
            &CutEngineOptions {
                emit_hidden_lines: true,
                ..Default::default()
            },
        )
        .unwrap();

        let hidden_edges: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::HiddenEdge).collect();
        assert!(!hidden_edges.is_empty(), "derive_section_view with emit_hidden_lines=true must emit HiddenEdge primitives");

        for h in &hidden_edges {
            assert_eq!(
                h.style.layer_name.as_deref(),
                Some("AEC_SECTION_HIDDEN"),
                "Section hidden lines must be on layer AEC_SECTION_HIDDEN"
            );
            assert_eq!(
                h.style.line_type.as_deref(),
                Some("DASHED"),
                "Section hidden lines must use DASHED linetype"
            );
        }
    }

    #[test]
    fn test_section_layer_detail_off_occludes_background_across_whole_wall_thickness() {
        let model_id = ModelId::new();
        let mut collector = SceneGeometryCollector::new();
        let core_comp = ComponentId::new();
        let cut_wall = CollectedWall {
            handle: acadrust::Handle::new(10),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "ThreeLayerWall".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, -2.0), Point2::new(0.0, 2.0)],
            layers: vec![
                CollectedWallLayer {
                    material: "Plaster".into(),
                    thickness: 0.02,
                    function: "Finish".into(),
                    axis_offset: -0.10,
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    hatch_override: None,
                    layer_id: Uuid::new_v4(),
                    component_id: ComponentId::new(),
                    is_structural: false,
                },
                CollectedWallLayer {
                    material: "Concrete".into(),
                    thickness: 0.20,
                    function: "Structural".into(),
                    axis_offset: 0.0,
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    hatch_override: None,
                    layer_id: Uuid::new_v4(),
                    component_id: core_comp,
                    is_structural: true,
                },
                CollectedWallLayer {
                    material: "Plaster".into(),
                    thickness: 0.02,
                    function: "Finish".into(),
                    axis_offset: 0.10,
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    hatch_override: None,
                    layer_id: Uuid::new_v4(),
                    component_id: ComponentId::new(),
                    is_structural: false,
                },
            ],
            total_thickness: 0.24,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        collector.walls.push(cut_wall);

        // Background wall behind the section (at y = 2.0, along X from -3.0 to 3.0 at z ∈ [0.0, 3.0]).
        let bg_wall = CollectedWall {
            handle: acadrust::Handle::new(20),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "BgWall".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(-3.0, 2.0), Point2::new(3.0, 2.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        collector.walls.push(bg_wall);

        let vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Schnitt Occlusion".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(-5.0, 0.0, 0.0)),
            axes: ViewAxes::section_with_side(Vector2::new(1.0, 0.0), false).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(-5.0, 0.0), Point2::new(5.0, 0.0)),
                far_depth: Some(PositiveMetres::new(10.0).unwrap()),
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(5.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };

        let mv = ModelView {
            id: ModelViewId::new(),
            name: "Ausfuehrung 1:50".into(),
            viewpoint_id: vp.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let mut dc = crate::modules::aec::engine::plan_view::DisplayConfig::new(
            "DetailOff".to_string(),
            "Architektur".to_string(),
            crate::modules::aec::engine::plan_view::PlanningStage::Design,
            crate::modules::aec::engine::plan_view::ViewType::Section,
        );
        dc.detail.layers = crate::modules::aec::engine::plan_view::LayerDetail::Off;

        let primitives = derive_section_view(
            &vp,
            &mv,
            &collector,
            &CutEngineOptions {
                display_config: Some(dc),
                ..Default::default()
            },
        )
        .unwrap();

        // 1. Verify that no CutBoundary primitives were emitted for the middle layer
        for p in primitives.iter().filter(|p| p.role == PrimitiveRole::CutBoundary) {
            assert!(
                !p.sources.as_slice().iter().any(|s| s.component_id == Some(core_comp)),
                "Core build-up layer must have no CutBoundary when LayerDetail::Off"
            );
        }

        // 2. Verify that background projected edges were occluded across the full wall thickness u ∈ [4.88, 5.12]
        let projected: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::ProjectedEdge).collect();
        assert!(!projected.is_empty(), "Background wall must produce projected edges");

        let mut found_left_split = false;
        let mut found_right_split = false;
        for prim in &projected {
            match &prim.geometry {
                Geometry2d::Line { start, end } => {
                    let min_u = start.x_m.min(end.x_m);
                    let max_u = start.x_m.max(end.x_m);
                    let mid_u = (start.x_m + end.x_m) * 0.5;
                    if max_u > 4.89 && min_u < 5.11 {
                        panic!(
                            "Background edge must be occluded across the full wall thickness in section, but found mid_u = {}",
                            mid_u
                        );
                    }
                    if (max_u - 4.88).abs() < 0.05 {
                        found_left_split = true;
                    }
                    if (min_u - 5.12).abs() < 0.05 {
                        found_right_split = true;
                    }
                }
                Geometry2d::Polyline { vertices, .. } => {
                    for win in vertices.windows(2) {
                        let min_u = win[0].x_m.min(win[1].x_m);
                        let max_u = win[0].x_m.max(win[1].x_m);
                        let mid_u = (win[0].x_m + win[1].x_m) * 0.5;
                        if max_u > 4.89 && min_u < 5.11 {
                            panic!(
                                "Background edge must be occluded across the full wall thickness in section, but found mid_u = {}",
                                mid_u
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        assert!(found_left_split, "Expected background edge split at left cut wall boundary u ≈ 4.88");
        assert!(found_right_split, "Expected background edge split at right cut wall boundary u ≈ 5.12");
    }

    #[test]
    fn test_occlusion_window_hole_see_through() {
        let view_id = ModelViewId::new();
        let source = SourceReference::new(ElementRef::new(ModelId::new(), ElementId::new()));

        // 1. Foreground opaque wall [0..10, 0..3] with window opening hole [3..5, 1..2] at depth 1.0m
        let wall_region = Geometry2d::Region {
            outer: vec![
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(10.0, 3.0),
                Point2::new(0.0, 3.0),
            ],
            holes: vec![vec![
                Point2::new(3.0, 1.0),
                Point2::new(5.0, 1.0),
                Point2::new(5.0, 2.0),
                Point2::new(3.0, 2.0),
            ]],
        };
        let wall_prim = DerivedPrimitive {
            id: PrimitiveId::new(view_id, 1, 1),
            role: PrimitiveRole::SurfaceFill,
            geometry: wall_region,
            sources: SourceSet::new(vec![source.clone()]).unwrap(),
            depth: DepthInterval::new(1.0, 1.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.18).unwrap()),
            edit_handles: vec![],
        };

        // 2. Background continuous line at depth 5.0m across y = 1.5 (passes through the window hole at x ∈ [3, 5])
        let bg_line = DerivedPrimitive {
            id: PrimitiveId::new(view_id, 1, 2),
            role: PrimitiveRole::ProjectedEdge,
            geometry: Geometry2d::Line {
                start: Point2::new(0.0, 1.5),
                end: Point2::new(10.0, 1.5),
            },
            sources: SourceSet::new(vec![source]).unwrap(),
            depth: DepthInterval::new(5.0, 5.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        };

        let mut prims = vec![wall_prim, bg_line];
        process_silhouette_and_depth(&mut prims, &SilhouetteOptions { emit_hidden_lines: false, max_depth_distance: None, hidden_layer: None }).unwrap();

        // The background line must be clipped so that ONLY the segment inside the window hole [3..5] is visible!
        let visible_bg: Vec<_> = prims.iter().filter(|p| p.role == PrimitiveRole::ProjectedEdge).collect();
        assert_eq!(visible_bg.len(), 1, "Only the segment seen through the window hole should remain visible");
        if let Geometry2d::Line { start, end } = visible_bg[0].geometry {
            assert!((start.x_m - 3.0).abs() < 1e-3, "Start must be at window left jamb x = 3.0");
            assert!((end.x_m - 5.0).abs() < 1e-3, "End must be at window right jamb x = 5.0");
        }
    }

    #[test]
    fn test_section_hidden_background_elements_do_not_project_or_occlude() {
        let model_id = ModelId::new();
        let mut collector = SceneGeometryCollector::new();

        // 1. Foreground background wall at y = 2.0 (x ∈ [-3.0, 3.0]), base_z = 0, top_z = 3
        let bg_wall_suppressed = CollectedWall {
            handle: acadrust::Handle::new(10),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall_1".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(-3.0, 2.0), Point2::new(3.0, 2.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        let suppressed_ref = bg_wall_suppressed.element_ref;
        collector.walls.push(bg_wall_suppressed);

        // 2. Further background wall at y = 5.0 (x ∈ [-3.0, 3.0]), base_z = 0, top_z = 3
        let bg_wall_visible = CollectedWall {
            handle: acadrust::Handle::new(20),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall_2".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(-3.0, 5.0), Point2::new(3.0, 5.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset: 0.0,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
                component_id: ComponentId::new(),
                is_structural: true,
            }],
            total_thickness: 0.24,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        let visible_ref = bg_wall_visible.element_ref;
        collector.walls.push(bg_wall_visible);

        let vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Schnitt Bg Filter".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(-5.0, 0.0, 0.0)),
            axes: ViewAxes::section_with_side(Vector2::new(1.0, 0.0), false).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(-5.0, 0.0), Point2::new(5.0, 0.0)),
                far_depth: Some(PositiveMetres::new(10.0).unwrap()),
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(5.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };

        // Suppress the closer background wall via ModelView GraphicOverride (draw = false)
        let mv = ModelView {
            id: ModelViewId::new(),
            name: "Ausfuehrung".into(),
            viewpoint_id: vp.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![GraphicOverride {
                id: OverrideId::new(),
                priority: 1,
                enabled: true,
                selector: OverrideSelector::Element(suppressed_ref),
                patch: GraphicPatch {
                    draw: Some(false),
                    ..Default::default()
                },
            }],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let primitives = derive_section_view(&vp, &mv, &collector, &CutEngineOptions::default()).unwrap();

        // 1. The suppressed background wall must emit ZERO primitives
        let suppressed_prims: Vec<_> = primitives
            .iter()
            .filter(|p| p.sources.as_slice().iter().any(|s| s.element == suppressed_ref))
            .collect();
        assert!(
            suppressed_prims.is_empty(),
            "Suppressed background wall must emit no primitives, but emitted {}",
            suppressed_prims.len()
        );

        // 2. The visible background wall must emit projected edges, and they must NOT be occluded
        let visible_prims: Vec<_> = primitives
            .iter()
            .filter(|p| p.sources.as_slice().iter().any(|s| s.element == visible_ref))
            .collect();
        assert!(!visible_prims.is_empty(), "Visible background wall must produce primitives");

        // Its projected edges along the top at z = 3.0 should run continuously across u ∈ [2.0, 8.0]
        let mut found_full_span = false;
        for prim in &visible_prims {
            if let Geometry2d::Line { start, end } = &prim.geometry {
                let min_u = start.x_m.min(end.x_m);
                let max_u = start.x_m.max(end.x_m);
                if (min_u - 2.0).abs() < 0.05 && (max_u - 8.0).abs() < 0.05 {
                    found_full_span = true;
                }
            }
        }
        assert!(
            found_full_span,
            "Visible background wall edge must span full width [2.0, 8.0] unoccluded by hidden background wall"
        );
    }
}
