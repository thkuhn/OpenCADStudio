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
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use crate::modules::aec::engine::openings::{OpeningKind, SwingSide};
    use crate::modules::aec::engine::opening_style::LeafOperation;
    use super::floorplan_engine::{derive_floorplan_view, FloorPlanEngineOptions};

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
    fn test_slab_layer_stack_order_in_section_and_elevation() {
        fn component_y_range(
            primitives: &[DerivedPrimitive],
            role: PrimitiveRole,
            component_id: ComponentId,
        ) -> Option<(f64, f64)> {
            let mut min_y = f64::INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for primitive in primitives.iter().filter(|primitive| {
                primitive.role == role
                    && primitive
                        .sources
                        .as_slice()
                        .iter()
                        .any(|source| source.component_id == Some(component_id))
            }) {
                if let Geometry2d::Region { outer, .. } = &primitive.geometry {
                    for point in outer {
                        min_y = min_y.min(point.y_m);
                        max_y = max_y.max(point.y_m);
                    }
                }
            }
            min_y.is_finite().then_some((min_y, max_y))
        }

        let model_id = ModelId::new();
        let slab_element = ElementRef::new(model_id, ElementId::new());
        let finish_component = ComponentId::new();
        let insulation_component = ComponentId::new();
        let structure_component = ComponentId::new();
        let slab = CollectedSlab {
            handle: acadrust::Handle::new(70),
            element_ref: slab_element,
            xref_source: None,
            style_id: "Slab".into(),
            base_z: 0.0,
            total_thickness: 0.30,
            polygon: vec![
                Point2::new(0.0, -1.0),
                Point2::new(10.0, -1.0),
                Point2::new(10.0, 1.0),
                Point2::new(0.0, 1.0),
            ],
            layers: vec![
                CollectedSlabLayer {
                    material: "Finish".into(),
                    thickness: 0.02,
                    vertical_offset: 0.0,
                    function: "Finish".into(),
                    hatch_override: None,
                    layer_id: Uuid::new_v4(),
                    component_id: finish_component,
                    is_structural: false,
                },
                CollectedSlabLayer {
                    material: "Insulation".into(),
                    thickness: 0.08,
                    vertical_offset: 0.005,
                    function: "Thermal".into(),
                    hatch_override: None,
                    layer_id: Uuid::new_v4(),
                    component_id: insulation_component,
                    is_structural: false,
                },
                CollectedSlabLayer {
                    material: "Concrete".into(),
                    thickness: 0.20,
                    vertical_offset: 0.0,
                    function: "Structural".into(),
                    hatch_override: None,
                    layer_id: Uuid::new_v4(),
                    component_id: structure_component,
                    is_structural: true,
                },
            ],
            justification: crate::modules::aec::engine::slab::SlabJustification::default(),
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        let expected_ranges = slab.layer_z_ranges();
        assert_eq!(expected_ranges.len(), 3);
        assert!((expected_ranges[0].0 - 0.30).abs() < 1e-9);
        assert!((expected_ranges[0].1 - 0.28).abs() < 1e-9);
        assert!((expected_ranges[1].0 - 0.285).abs() < 1e-9);
        assert!((expected_ranges[1].1 - 0.205).abs() < 1e-9);
        assert!((expected_ranges[2].0 - 0.20).abs() < 1e-9);
        assert!(expected_ranges[2].1.abs() < 1e-9);

        let mut collector = SceneGeometryCollector::new();
        collector.slabs.push(slab.clone());
        let vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Slab section".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(5.0, -3.0, 0.0)),
            axes: ViewAxes::section(Vector2::new(0.0, 1.0)).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(5.0, -3.0), Point2::new(5.0, 3.0)),
                far_depth: None,
            },
            z_range: VerticalRange::new(Metres::new(-1.0).unwrap(), Metres::new(1.0).unwrap())
                .unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };
        let mv = ModelView {
            id: ModelViewId::new(),
            name: "Slab section".into(),
            viewpoint_id: vp.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![GraphicOverride {
                id: OverrideId::new(),
                selector: OverrideSelector::Component {
                    element: slab_element,
                    component_id: insulation_component,
                },
                patch: GraphicPatch {
                    draw: Some(false),
                    ..Default::default()
                },
                priority: 0,
                enabled: true,
            }],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let section_primitives =
            derive_section_view(&vp, &mv, &collector, &CutEngineOptions::default()).unwrap();
        for (component, expected) in [
            (finish_component, expected_ranges[0]),
            (structure_component, expected_ranges[2]),
        ] {
            let (min_y, max_y) = component_y_range(
                &section_primitives,
                PrimitiveRole::CutFill,
                component,
            )
            .expect("section layer band");
            assert!((max_y - expected.0).abs() < 1e-6);
            assert!((min_y - expected.1).abs() < 1e-6);
        }
        assert!(component_y_range(
            &section_primitives,
            PrimitiveRole::CutFill,
            insulation_component
        )
        .is_none());

        let style_library = crate::modules::aec::engine::library::seed_default_library();
        let style_ctx = style_resolver::ViewStyleContext::new(&mv, None, Some(&style_library), true);
        let mut elevation_primitives = Vec::new();
        let mut primitive_counter = 0;
        elevation_engine::project_slab_elevation(
            &slab,
            &vp,
            0.0,
            f64::INFINITY,
            &mv,
            &elevation_engine::ElevationEngineOptions::default(),
            &style_ctx,
            &mut elevation_primitives,
            &mut primitive_counter,
        )
        .unwrap();
        for (component, expected) in [
            (finish_component, expected_ranges[0]),
            (structure_component, expected_ranges[2]),
        ] {
            let (min_y, max_y) = component_y_range(
                &elevation_primitives,
                PrimitiveRole::SurfaceFill,
                component,
            )
            .expect("visible elevation layer band");
            assert!((max_y - expected.0).abs() < 1e-6);
            assert!((min_y - expected.1).abs() < 1e-6);
        }
        assert!(component_y_range(
            &elevation_primitives,
            PrimitiveRole::SurfaceFill,
            insulation_component
        )
        .is_none());
    }

    fn component_band(
        primitives: &[DerivedPrimitive],
        role: PrimitiveRole,
        component_id: ComponentId,
    ) -> Option<(f64, f64)> {
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for primitive in primitives.iter().filter(|primitive| {
            primitive.role == role
                && primitive
                    .sources
                    .as_slice()
                    .iter()
                    .any(|source| source.component_id == Some(component_id))
        }) {
            if let Geometry2d::Region { outer, .. } = &primitive.geometry {
                for point in outer {
                    min_y = min_y.min(point.y_m);
                    max_y = max_y.max(point.y_m);
                }
            }
        }
        min_y.is_finite().then_some((min_y, max_y))
    }

    fn assert_band(
        primitives: &[DerivedPrimitive],
        role: PrimitiveRole,
        component_id: ComponentId,
        expected: (f64, f64),
    ) {
        let (min_y, max_y) = component_band(primitives, role, component_id)
            .unwrap_or_else(|| panic!("missing {role:?} band"));
        assert!((max_y - expected.0).abs() < 1e-6, "top {max_y} != {}", expected.0);
        assert!((min_y - expected.1).abs() < 1e-6, "bottom {min_y} != {}", expected.1);
    }

    fn stacked_slab(
        base_z: f64,
        polygon: Vec<Point2>,
        offsets: [f64; 3],
    ) -> (CollectedSlab, [ComponentId; 3]) {
        let components = [ComponentId::new(), ComponentId::new(), ComponentId::new()];
        let slab = CollectedSlab {
            handle: acadrust::Handle::new(71),
            element_ref: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: Some(XrefSourceRef::with_translation("OG", Some("OG.dwg".into()), 0.0, 0.0, base_z)),
            style_id: "Slab".into(),
            base_z,
            total_thickness: 0.30,
            polygon,
            layers: vec![
                CollectedSlabLayer {
                    material: "Finish".into(),
                    thickness: 0.02,
                    vertical_offset: offsets[0],
                    function: "Finish".into(),
                    hatch_override: None,
                    layer_id: Uuid::new_v4(),
                    component_id: components[0],
                    is_structural: false,
                },
                CollectedSlabLayer {
                    material: "Insulation".into(),
                    thickness: 0.08,
                    vertical_offset: offsets[1],
                    function: "Thermal".into(),
                    hatch_override: None,
                    layer_id: Uuid::new_v4(),
                    component_id: components[1],
                    is_structural: false,
                },
                CollectedSlabLayer {
                    material: "Concrete".into(),
                    thickness: 0.20,
                    vertical_offset: offsets[2],
                    function: "Structural".into(),
                    hatch_override: None,
                    layer_id: Uuid::new_v4(),
                    component_id: components[2],
                    is_structural: true,
                },
            ],
            justification: crate::modules::aec::engine::slab::SlabJustification::default(),
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        (slab, components)
    }

    fn section_view(name: &str, z_min: f64, z_max: f64) -> (Viewpoint, ModelView) {
        let vp = Viewpoint {
            id: ViewpointId::new(),
            name: name.into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(5.0, -3.0, 0.0)),
            axes: ViewAxes::section(Vector2::new(0.0, 1.0)).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(5.0, -3.0), Point2::new(5.0, 3.0)),
                far_depth: None,
            },
            z_range: VerticalRange::new(Metres::new(z_min).unwrap(), Metres::new(z_max).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };
        let mv = ModelView {
            id: ModelViewId::new(),
            name: name.into(),
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
        (vp, mv)
    }

    #[test]
    fn test_wall_layer_top_offset_sign_in_section_and_elevation() {
        let model_id = ModelId::new();
        let element_ref = ElementRef::new(model_id, ElementId::new());
        let lower_component = ComponentId::new();
        let higher_component = ComponentId::new();
        let layers = [(-0.20, lower_component), (0.20, higher_component)]
            .into_iter()
            .map(|(top_offset, component_id)| CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.20,
                function: "Structural".into(),
                axis_offset: 0.0,
                bottom_offset: 0.0,
                top_offset,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
                component_id,
                is_structural: true,
            })
            .collect();
        let wall = CollectedWall {
            handle: acadrust::Handle::new(72),
            element_ref,
            xref_source: None,
            style_id: "Wall".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
            layers,
            total_thickness: 0.40,
            justification: crate::modules::aec::engine::wall::WallJustification::Center,
            openings: vec![],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        let mut collector = SceneGeometryCollector::new();
        collector.walls.push(wall);

        let (section_vp, section_mv) = section_view("Wall layer offsets", -1.0, 5.0);
        let section = derive_section_view(
            &section_vp,
            &section_mv,
            &collector,
            &CutEngineOptions::default(),
        )
        .unwrap();
        assert_band(
            &section,
            PrimitiveRole::CutFill,
            lower_component,
            (2.80, 0.0),
        );
        assert_band(
            &section,
            PrimitiveRole::CutFill,
            higher_component,
            (3.20, 0.0),
        );

        let elevation_vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Wall layer offsets elevation".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(0.0, -2.0, 0.0)),
            axes: ViewAxes::front_elevation(),
            kind: ViewKind::Elevation {
                depth: DepthRange {
                    near: NonNegativeMetres::ZERO,
                    far: Some(PositiveMetres::new(20.0).unwrap()),
                },
            },
            z_range: VerticalRange::new(Metres::new(-1.0).unwrap(), Metres::new(5.0).unwrap())
                .unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };
        let mut elevation_mv = section_mv;
        elevation_mv.viewpoint_id = elevation_vp.id;
        let elevation = derive_elevation_view(
            &elevation_vp,
            &elevation_mv,
            &collector,
            &ElevationEngineOptions::default(),
        )
        .unwrap();
        for (component_id, expected_top) in [(lower_component, 2.80), (higher_component, 3.20)] {
            let top_edge = elevation
                .iter()
                .find(|primitive| {
                    primitive.role == PrimitiveRole::ProjectedEdge
                        && primitive.sources.as_slice().iter().any(|source| {
                            source.component_id == Some(component_id)
                                && source.feature == Some(FeatureKey::WallTop)
                        })
                })
                .expect("projected wall-layer top edge");
            let Geometry2d::Line { start, end } = &top_edge.geometry else {
                panic!("wall-layer top edge must be a line");
            };
            assert!((start.y_m - expected_top).abs() < 1e-6);
            assert!((end.y_m - expected_top).abs() < 1e-6);
        }
    }

    #[test]
    fn test_slab_offsets_and_shifted_base_keep_top_down_stack() {
        let polygon = vec![
            Point2::new(0.0, -1.0),
            Point2::new(10.0, -1.0),
            Point2::new(10.0, 1.0),
            Point2::new(0.0, 1.0),
        ];
        let (slab, components) = stacked_slab(2.80, polygon, [-0.01, 0.0, -0.02]);
        let ranges = slab.layer_z_ranges();
        assert!((ranges[2].1 - 2.78).abs() < 1e-9, "core may extend below base_z");

        let mut collector = SceneGeometryCollector::new();
        collector.slabs.push(slab);
        let (vp, mv) = section_view("Shifted slab", 2.0, 4.0);
        let section = derive_section_view(&vp, &mv, &collector, &CutEngineOptions::default()).unwrap();
        for (index, component) in components.into_iter().enumerate() {
            assert_band(&section, PrimitiveRole::CutFill, component, ranges[index]);
        }
        assert!(
            section.iter().all(|primitive| primitive.edit_handles.is_empty()),
            "derived slab edges stay non-associative"
        );

        let style_library = crate::modules::aec::engine::library::seed_default_library();
        let style_ctx = style_resolver::ViewStyleContext::new(&mv, None, Some(&style_library), true);
        let mut elevation = Vec::new();
        let mut counter = 0;
        elevation_engine::project_slab_elevation(
            &collector.slabs[0],
            &vp,
            0.0,
            f64::INFINITY,
            &mv,
            &elevation_engine::ElevationEngineOptions::default(),
            &style_ctx,
            &mut elevation,
            &mut counter,
        )
        .unwrap();
        for (index, component) in components.into_iter().enumerate() {
            assert_band(&elevation, PrimitiveRole::SurfaceFill, component, ranges[index]);
        }
    }

    #[test]
    fn test_hidden_top_slab_layer_keeps_the_gap() {
        let (slab, components) = stacked_slab(
            0.0,
            vec![
                Point2::new(0.0, -1.0),
                Point2::new(10.0, -1.0),
                Point2::new(10.0, 1.0),
                Point2::new(0.0, 1.0),
            ],
            [0.0, 0.0, 0.0],
        );
        let ranges = slab.layer_z_ranges();
        let mut collector = SceneGeometryCollector::new();
        collector.slabs.push(slab.clone());
        let (vp, mut mv) = section_view("Hidden finish", -1.0, 1.0);
        mv.graphic_overrides.push(GraphicOverride {
            id: OverrideId::new(),
            selector: OverrideSelector::Component {
                element: slab.element_ref,
                component_id: components[0],
            },
            patch: GraphicPatch {
                draw: Some(false),
                ..Default::default()
            },
            priority: 0,
            enabled: true,
        });

        let section = derive_section_view(&vp, &mv, &collector, &CutEngineOptions::default()).unwrap();
        assert!(component_band(&section, PrimitiveRole::CutFill, components[0]).is_none());
        assert_band(&section, PrimitiveRole::CutFill, components[1], ranges[1]);
        assert_band(&section, PrimitiveRole::CutFill, components[2], ranges[2]);
        let (_, insulation_top) = component_band(&section, PrimitiveRole::CutFill, components[1]).unwrap();
        assert!(
            (insulation_top - (slab.base_z + slab.total_thickness)).abs() > 1e-3,
            "hiding the finish must not promote the next layer to the slab top"
        );
    }

    #[test]
    fn test_structural_only_keeps_slab_core_at_its_height() {
        let (slab, components) = stacked_slab(
            0.0,
            vec![
                Point2::new(0.0, -1.0),
                Point2::new(10.0, -1.0),
                Point2::new(10.0, 1.0),
                Point2::new(0.0, 1.0),
            ],
            [0.0, 0.0, 0.0],
        );
        let ranges = slab.layer_z_ranges();
        let mut collector = SceneGeometryCollector::new();
        collector.slabs.push(slab);
        let (vp, mv) = section_view("Rohbau", -1.0, 1.0);
        let section = derive_section_view(
            &vp,
            &mv,
            &collector,
            &CutEngineOptions {
                structural_only: true,
                ..CutEngineOptions::default()
            },
        )
        .unwrap();

        assert!(component_band(&section, PrimitiveRole::CutFill, components[0]).is_none());
        assert!(component_band(&section, PrimitiveRole::CutFill, components[1]).is_none());
        assert_band(&section, PrimitiveRole::CutFill, components[2], ranges[2]);
    }

    #[test]
    fn test_section_background_slab_uses_the_same_stack() {
        let (slab, components) = stacked_slab(
            0.0,
            vec![
                Point2::new(0.0, -1.0),
                Point2::new(4.0, -1.0),
                Point2::new(4.0, 1.0),
                Point2::new(0.0, 1.0),
            ],
            [0.0, 0.005, 0.0],
        );
        let ranges = slab.layer_z_ranges();
        let mut collector = SceneGeometryCollector::new();
        collector.slabs.push(slab);
        let (vp, mv) = section_view("Background slab", -1.0, 1.0);
        let section = derive_section_view(&vp, &mv, &collector, &CutEngineOptions::default()).unwrap();

        assert!(
            section.iter().all(|primitive| primitive.role != PrimitiveRole::CutFill),
            "a slab behind the cut must not become a section hole or cut fill"
        );
        for (index, component) in components.into_iter().enumerate() {
            assert_band(&section, PrimitiveRole::SurfaceFill, component, ranges[index]);
        }
    }

    #[test]
    fn test_empty_slab_layers_do_not_panic() {
        let (mut slab, _) = stacked_slab(0.0, vec![
            Point2::new(0.0, -1.0),
            Point2::new(10.0, -1.0),
            Point2::new(10.0, 1.0),
            Point2::new(0.0, 1.0),
        ], [0.0, 0.0, 0.0]);
        slab.layers.clear();
        let (z_min, z_max) = slab.z_range();
        assert!((z_min - slab.base_z).abs() < 1e-9);
        assert!((z_max - (slab.base_z + slab.total_thickness)).abs() < 1e-9);

        let mut collector = SceneGeometryCollector::new();
        collector.slabs.push(slab.clone());
        let (vp, mv) = section_view("Empty layers", -1.0, 1.0);
        let section = derive_section_view(&vp, &mv, &collector, &CutEngineOptions::default()).unwrap();
        assert!(section.iter().all(|primitive| {
            primitive.role != PrimitiveRole::CutFill && primitive.role != PrimitiveRole::SurfaceFill
        }));
        let style_library = crate::modules::aec::engine::library::seed_default_library();
        let style_ctx = style_resolver::ViewStyleContext::new(&mv, None, Some(&style_library), true);
        let mut elevation = Vec::new();
        let mut counter = 0;
        elevation_engine::project_slab_elevation(
            &slab,
            &vp,
            0.0,
            f64::INFINITY,
            &mv,
            &elevation_engine::ElevationEngineOptions::default(),
            &style_ctx,
            &mut elevation,
            &mut counter,
        )
        .unwrap();
        assert!(elevation.is_empty());
    }

    #[test]
    fn test_floorplan_keeps_slab_as_plan_outline() {
        let (slab, components) = stacked_slab(
            0.0,
            vec![
                Point2::new(0.0, 5.0),
                Point2::new(4.0, 5.0),
                Point2::new(4.0, 8.0),
                Point2::new(0.0, 8.0),
            ],
            [0.0, 0.0, 0.0],
        );
        let mut collector = SceneGeometryCollector::new();
        collector.slabs.push(slab);
        let vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Plan".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.5).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(4.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };
        let mv = ModelView {
            id: ModelViewId::new(),
            name: "Plan".into(),
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

        let primitives = derive_floorplan_view(&vp, &mv, &collector, &FloorPlanEngineOptions::default()).unwrap();
        let outlines: Vec<_> = primitives
            .iter()
            .filter(|primitive| primitive.role == PrimitiveRole::ProjectedEdge)
            .collect();
        assert_eq!(outlines.len(), 1, "a slab plan is one outline, not one band per layer");
        if let Geometry2d::Polyline { vertices, closed: true } = &outlines[0].geometry {
            assert!(vertices.iter().all(|point| (5.0..=8.0).contains(&point.y_m)));
        } else {
            panic!("expected a closed plan outline");
        }
        assert!(primitives.iter().all(|primitive| {
            primitive.role != PrimitiveRole::CutFill
                && primitive.sources.as_slice().iter().all(|source| {
                    !components.contains(&source.component_id.unwrap_or_else(ComponentId::new))
                })
        }));
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

        let mut background_wall = collector.walls[1].clone();
        background_wall.handle = acadrust::Handle::new(3);
        background_wall.element_ref = ElementRef::new(model_id, ElementId::new());
        background_wall.axis_points = vec![Point2::new(5.5, 4.0), Point2::new(6.5, 4.0)];
        collector.walls.push(background_wall.clone());

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
        let background_top = primitives.iter().find_map(|primitive| {
            if primitive.role != PrimitiveRole::ProjectedEdge
                || !primitive.sources.as_slice().iter().any(|source| source.element == background_wall.element_ref)
            {
                return None;
            }
            match primitive.geometry {
                Geometry2d::Line { start, end }
                    if (start.y_m - 3.0).abs() < 1e-6 && (end.y_m - 3.0).abs() < 1e-6 =>
                {
                    Some((start.x_m.min(end.x_m), start.x_m.max(end.x_m)))
                }
                _ => None,
            }
        });
        let (min_station, max_station) = background_top.expect("background wall top edge");
        assert!((min_station - 7.5).abs() < 1e-6);
        assert!((max_station - 8.5).abs() < 1e-6);
    }

    #[test]
    fn test_section_cut_with_opening_and_handles() {
        let model_id = ModelId::new();
        let mut collector = SceneGeometryCollector::new();
        let opening_element = ElementRef::new(model_id, ElementId::new());
        let wall_element = ElementRef::new(model_id, ElementId::new());
        let background_opening_element = ElementRef::new(model_id, ElementId::new());
        let background_wall_element = ElementRef::new(model_id, ElementId::new());

        let opening = CollectedOpening {
            handle: acadrust::Handle::new(50),
            element_ref: opening_element,
            xref_source: None,
            host_wall_handle: acadrust::Handle::new(10),
            kind: OpeningKind::Window,
            style_id: Some("Window_100x120".into()),
            division_override: None,
            distance_along_axis: 5.0,
            width: 1.2,
            height: 1.4,
            sill_height: 0.9,
            spring_height: 0.0,
            shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
            hinge: LeafOperation::left(),
            swing_side: Default::default(),
            reference_side: crate::modules::aec::engine::openings::OpeningReferenceSide::Center,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(5.0, 0.0, 1.6),
            axis_span_local: (4.4, 5.6),
        };

        // Wall from (0,0) to (10,0) with window centered at (5,0)
        let wall = CollectedWall {
            handle: acadrust::Handle::new(10),
            element_ref: wall_element,
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
        let mut background_opening = wall.openings[0].clone();
        background_opening.handle = acadrust::Handle::new(51);
        background_opening.element_ref = background_opening_element;
        background_opening.host_wall_handle = acadrust::Handle::new(11);
        background_opening.distance_along_axis = 3.0;
        background_opening.world_center = Point3::new(4.0, 0.0, 1.6);
        background_opening.axis_span_local = (2.4, 3.6);
        let background_wall = CollectedWall {
            handle: acadrust::Handle::new(11),
            element_ref: background_wall_element,
            xref_source: None,
            style_id: "Wall".into(),
            height: wall.height,
            base_z: wall.base_z,
            top_z: wall.top_z,
            axis_points: vec![Point2::new(4.0, -3.0), Point2::new(4.0, 3.0)],
            layers: wall.layers.clone(),
            total_thickness: wall.total_thickness,
            justification: wall.justification,
            openings: vec![background_opening],
            phase: wall.phase,
        };
        collector.walls.push(wall);
        collector.walls.push(background_wall);

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

        let style_library = crate::modules::aec::engine::library::seed_default_library();
        let style_ctx = style_resolver::ViewStyleContext::new(&mv, None, Some(&style_library), true);
        let background_wall = collector
            .walls
            .iter()
            .find(|wall| wall.element_ref == background_wall_element)
            .unwrap();
        let mut projected_wall_primitives = Vec::new();
        let mut projected_wall_counter = 0;
        elevation_engine::project_wall_elevation(
            background_wall,
            &vp,
            0.0,
            f64::INFINITY,
            &mv,
            &elevation_engine::ElevationEngineOptions::default(),
            &style_ctx,
            &mut projected_wall_primitives,
            &mut projected_wall_counter,
        )
        .unwrap();

        // Check sill remnant and head remnant fills
        let fills: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::CutFill).collect();
        assert_eq!(fills.len(), 2, "should produce sill remnant and head remnant fills");

        let wall_surface = projected_wall_primitives
            .iter()
            .find(|p| {
                p.role == PrimitiveRole::SurfaceFill
                    && p.sources
                        .as_slice()
                        .iter()
                        .any(|source| source.element == background_wall_element)
            })
            .expect("projected wall surface should remain continuous behind the opening");
        match &wall_surface.geometry {
            Geometry2d::Region { holes, .. } => assert!(holes.is_empty()),
            _ => panic!("projected wall surface should be a region"),
        }

        let opening_surface = projected_wall_primitives
            .iter()
            .find(|p| {
                p.role == PrimitiveRole::SurfaceFill
                    && p.sources
                        .as_slice()
                        .iter()
                        .any(|source| source.element == background_opening_element)
            })
            .expect("opening should emit an opaque projected surface");
        assert!(
            opening_surface.depth.max_m < wall_surface.depth.min_m,
            "opening surface should be in front of the wall-axis surface"
        );

        let swing_points = symbol_points(&primitives, background_opening_element);
        assert!(!swing_points.is_empty(), "projected opening should emit its swing symbol");
        let expected_hinge_tip = Point2::new(
            vp.world_to_view(Point3::new(4.0, -0.6, 1.6)).0.x_m,
            vp.paper_elevation(1.6),
        );
        assert!(
            swing_points.iter().any(|point| point.distance_to(expected_hinge_tip) < 1e-6),
            "swing tip should sit on the hinge jamb, not the strike"
        );

        let mut reversed_collector = collector.clone();
        let reversed_wall = reversed_collector
            .walls
            .iter_mut()
            .find(|wall| wall.element_ref == background_wall_element)
            .unwrap();
        reversed_wall.axis_points.reverse();
        reversed_wall.openings[0].hinge = LeafOperation::right();
        reversed_wall.openings[0].swing_side =
            crate::modules::aec::engine::openings::SwingSide::Interior;
        let reversed_primitives =
            derive_section_view(&vp, &mv, &reversed_collector, &CutEngineOptions::default())
                .unwrap();
        let reversed_points = symbol_points(&reversed_primitives, background_opening_element);
        assert_eq!(swing_points.len(), reversed_points.len());
        assert!(swing_points.iter().zip(&reversed_points).all(|(left, right)| left.distance_to(*right) < 1e-6));

        let mut opposite_collector = collector.clone();
        let opposite_wall = opposite_collector
            .walls
            .iter_mut()
            .find(|wall| wall.element_ref == background_wall_element)
            .unwrap();
        opposite_wall.axis_points = vec![Point2::new(6.0, -3.0), Point2::new(6.0, 3.0)];
        let mut opposite_vp = vp.clone();
        opposite_vp.axes = ViewAxes::section_with_side(Vector2::new(0.0, 1.0), true).unwrap();
        let opposite_primitives =
            derive_section_view(&opposite_vp, &mv, &opposite_collector, &CutEngineOptions::default())
                .unwrap();
        let opposite_points = symbol_points(&opposite_primitives, background_opening_element);
        assert!(!opposite_points.is_empty(), "opposite facade should emit its swing symbol");
        let expected_opposite_hinge_tip = Point2::new(
            2.4,
            opposite_vp.paper_elevation(1.6),
        );
        assert!(
            opposite_points
                .iter()
                .any(|point| point.distance_to(expected_opposite_hinge_tip) < 1e-6),
            "opposite facade should keep the tip on the same world hinge"
        );

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
            division_override: None,
            distance_along_axis: 5.0,
            width: 1.2,
            height: 1.4,
            sill_height: 0.9,
            spring_height: 0.0,
            shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
            hinge: LeafOperation::left(),
            swing_side: Default::default(),
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

        // Depth sorting must not overwrite the resolved line weight.
        assert_eq!(list[0].style.line_weight_mm.get(), 0.35);
        assert_eq!(list[1].style.line_weight_mm.get(), 0.35);
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
                division_override: None,
                distance_along_axis: 5.0,
                width: 2.0,
                height: 1.2,
                sill_height: 0.9,
                spring_height: 0.0,
                shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
                hinge: LeafOperation::left(),
                swing_side: Default::default(),
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
    fn test_section_hidden_core_does_not_occlude_but_structural_core_does() {
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
        let background_wall_element = bg_wall.element_ref;
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

        // 2. The hidden core leaves the background edge visible through the section plane.
        let projected: Vec<_> = primitives.iter().filter(|p| p.role == PrimitiveRole::ProjectedEdge).collect();
        assert!(!projected.is_empty(), "Background wall must produce projected edges");
        let background_crosses_cut = |primitives: &[DerivedPrimitive]| {
            primitives.iter().any(|primitive| {
                primitive.role == PrimitiveRole::ProjectedEdge
                    && primitive
                        .sources
                        .as_slice()
                        .iter()
                        .any(|source| source.element == background_wall_element)
                    && match &primitive.geometry {
                        Geometry2d::Line { start, end } => {
                            start.x_m < 5.0 - 1e-6 && end.x_m > 5.0 + 1e-6
                                || end.x_m < 5.0 - 1e-6 && start.x_m > 5.0 + 1e-6
                        }
                        Geometry2d::Polyline { vertices, closed } => {
                            let edge_count = if *closed { vertices.len() } else { vertices.len().saturating_sub(1) };
                            (0..edge_count).any(|index| {
                                let start = vertices[index];
                                let end = vertices[(index + 1) % vertices.len()];
                                start.x_m < 5.0 - 1e-6 && end.x_m > 5.0 + 1e-6
                                    || end.x_m < 5.0 - 1e-6 && start.x_m > 5.0 + 1e-6
                            })
                        }
                        _ => false,
                    }
            })
        };
        assert!(background_crosses_cut(&primitives), "hidden core must not occlude the background edge");

        let mut structural_config = crate::modules::aec::engine::plan_view::DisplayConfig::new(
            "StructuralOnly".to_string(),
            "Architektur".to_string(),
            crate::modules::aec::engine::plan_view::PlanningStage::Design,
            crate::modules::aec::engine::plan_view::ViewType::Section,
        );
        structural_config.detail.layers = crate::modules::aec::engine::plan_view::LayerDetail::StructuralOnly;
        let structural_primitives = derive_section_view(
            &vp,
            &mv,
            &collector,
            &CutEngineOptions {
                display_config: Some(structural_config),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            structural_primitives.iter().any(|primitive| {
                primitive.role == PrimitiveRole::CutBoundary
                    && primitive.sources.as_slice().iter().any(|source| source.component_id == Some(core_comp))
            }),
            "visible structural core must be drawn"
        );
        assert!(
            !background_crosses_cut(&structural_primitives),
            "visible structural core must occlude the background edge"
        );
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

    #[test]
    fn test_elevation_opening_is_opaque_and_occludes_background() {
        let model_id = ModelId::new();
        let mut collector = SceneGeometryCollector::new();

        // 1. Foreground wall at y = 0.0 with a window at x = 0.0 (span [-1.0, 1.0]), sill = 0.9, height = 1.2
        let fg_wall = CollectedWall {
            handle: acadrust::Handle::new(10),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall_FG".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(-5.0, 0.0), Point2::new(5.0, 0.0)],
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
            openings: vec![CollectedOpening {
                handle: acadrust::Handle::new(11),
                element_ref: ElementRef::new(model_id, ElementId::new()),
                xref_source: None,
                host_wall_handle: acadrust::Handle::new(10),
                kind: OpeningKind::Window,
                style_id: Some("Window".into()),
                division_override: None,
                distance_along_axis: 5.0,
                width: 2.0,
                height: 1.2,
                sill_height: 0.9,
                spring_height: 0.0,
                shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
                hinge: LeafOperation::left(),
                swing_side: SwingSide::Exterior,
                reference_side: crate::modules::aec::engine::openings::OpeningReferenceSide::Center,
                cross_axis_offset: 0.0,
                depth: None,
                world_center: Point3::new(0.0, 0.0, 1.5),
                axis_span_local: (4.0, 6.0),
            }],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        collector.walls.push(fg_wall);

        // 2. Background wall at y = 4.0, base_z = 0, top_z = 3, spanning x in [-5.0, 5.0]
        let bg_wall = CollectedWall {
            handle: acadrust::Handle::new(20),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall_BG".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(-5.0, 4.0), Point2::new(5.0, 4.0)],
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
        let bg_ref = bg_wall.element_ref;
        collector.walls.push(bg_wall);

        let vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Suedansicht".into(),
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

        // Check that the window emitted a SurfaceFill occluder
        let window_fill = primitives
            .iter()
            .find(|p| p.role == PrimitiveRole::SurfaceFill && p.style.layer_name.as_deref() == Some("AEC_OPENING_SURFACE"));
        assert!(window_fill.is_some(), "Window must emit an opaque SurfaceFill occluder to prevent see-through");

        // Check that any projected line from the background wall does NOT pass through the window center (u ≈ 0.0, v ≈ 1.5)
        for prim in &primitives {
            if prim.sources.as_slice().iter().any(|s| s.element == bg_ref) {
                if let Geometry2d::Line { start, end } = &prim.geometry {
                    // If line is at window elevation (e.g. mid-height or sill/head), it must not pass through [-0.9, 0.9]
                    if (start.y_m - 1.5).abs() < 0.5 {
                        let min_u = start.x_m.min(end.x_m);
                        let max_u = start.x_m.max(end.x_m);
                        assert!(
                            max_u <= -0.95 || min_u >= 0.95,
                            "Background wall edge ({:?} to {:?}) should be occluded behind window [-1.0, 1.0]",
                            start,
                            end
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_elevation_opening_swing_triangle_orientation_and_linetype() {
        let model_id = ModelId::new();
        let mut collector = SceneGeometryCollector::new();

        // Wall along X with Door: swing_side = Exterior (+Normal), hinge = Left
        let door = CollectedOpening {
            handle: acadrust::Handle::new(11),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            host_wall_handle: acadrust::Handle::new(10),
            kind: OpeningKind::Door,
            style_id: Some("Door".into()),
            division_override: None,
            distance_along_axis: 5.0,
            width: 1.0,
            height: 2.1,
            sill_height: 0.0,
            spring_height: 0.0,
            shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
            hinge: LeafOperation::left(),
            swing_side: SwingSide::Interior,
            reference_side: crate::modules::aec::engine::openings::OpeningReferenceSide::Center,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(0.0, 0.0, 1.05),
            axis_span_local: (4.5, 5.5),
        };

        let wall = CollectedWall {
            handle: acadrust::Handle::new(10),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(-5.0, 0.0), Point2::new(5.0, 0.0)],
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
            openings: vec![door],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        collector.walls.push(wall);

        // 1. Exterior view (front elevation looking from -Y toward +Y, facing the exterior swing side)
        let vp_ext = Viewpoint {
            id: ViewpointId::new(),
            name: "Front Elevation".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(0.0, -5.0, 0.0)),
            axes: ViewAxes::front_elevation(),
            kind: ViewKind::Elevation {
                depth: DepthRange {
                    near: NonNegativeMetres::new(0.0).unwrap(),
                    far: Some(PositiveMetres::new(10.0).unwrap()),
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(5.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };

        let mv = ModelView {
            id: ModelViewId::new(),
            name: "Ansicht".into(),
            viewpoint_id: vp_ext.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(100.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };

        let prims_ext = derive_elevation_view(&vp_ext, &mv, &collector, &ElevationEngineOptions::default()).unwrap();
        let swing_ext = prims_ext.iter().find(|p| p.role == PrimitiveRole::Symbol).expect("swing symbol");

        // When facing the swing side, DIN opening swing triangle should be a solid line (not DASHED)
        assert_ne!(
            swing_ext.style.line_type.as_deref(),
            Some("DASHED"),
            "Swing symbol facing the observer must be solid"
        );

        if let Geometry2d::Polyline { vertices, .. } = &swing_ext.geometry {
            assert_eq!(vertices.len(), 3, "Swing triangle must have 3 vertices (strike, hinge tip, strike)");
            let apex = vertices[1];
            assert!(apex.x_m < vertices[0].x_m, "Apex must point at the left hinge, not the strike");
        } else {
            panic!("Expected polyline geometry for swing symbol");
        }

        // 2. Interior view (back elevation looking from +Y toward -Y, opposite to swing side)
        let vp_int = Viewpoint {
            id: ViewpointId::new(),
            name: "Back Elevation".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(0.0, 5.0, 0.0)),
            axes: ViewAxes::section_with_side(Vector2::new(-1.0, 0.0), false).unwrap(),
            kind: ViewKind::Elevation {
                depth: DepthRange {
                    near: NonNegativeMetres::new(0.0).unwrap(),
                    far: Some(PositiveMetres::new(10.0).unwrap()),
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(5.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };

        let prims_int = derive_elevation_view(&vp_int, &mv, &collector, &ElevationEngineOptions::default()).unwrap();
        let swing_int = prims_int.iter().find(|p| p.role == PrimitiveRole::Symbol).expect("interior swing symbol");

        // When viewing from the opposite side, the swing triangle must be DASHED per DIN
        assert_eq!(
            swing_int.style.line_type.as_deref(),
            Some("DASHED"),
            "Swing symbol on opposite side must be DASHED"
        );
    }

    #[test]
    fn test_elevation_wall_shares_section_base_datum() {
        let model_id = ModelId::new();
        let mut collector = SceneGeometryCollector::new();
        let wall = CollectedWall {
            handle: acadrust::Handle::new(81),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall".into(),
            height: 2.6,
            base_z: 2.8,
            top_z: 5.4,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(8.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: -0.12,
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
        collector.walls.push(wall);

        let elev = Viewpoint {
            id: ViewpointId::new(),
            name: "South".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(0.0, -2.0, -3.0)),
            axes: ViewAxes::front_elevation(),
            kind: ViewKind::Elevation {
                depth: DepthRange {
                    near: NonNegativeMetres::ZERO,
                    far: Some(PositiveMetres::new(12.0).unwrap()),
                },
            },
            z_range: VerticalRange::new(Metres::new(-3.0).unwrap(), Metres::new(9.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };
        let mv = ModelView {
            id: ModelViewId::new(),
            name: "South".into(),
            viewpoint_id: elev.id,
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(100.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: BTreeSet::new(),
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };
        let elevation = derive_elevation_view(&elev, &mv, &collector, &ElevationEngineOptions::default()).unwrap();
        let mut elev_ys = Vec::new();
        for primitive in &elevation {
            match &primitive.geometry {
                Geometry2d::Line { start, end } => {
                    elev_ys.push(start.y_m);
                    elev_ys.push(end.y_m);
                }
                Geometry2d::Polyline { vertices, .. } | Geometry2d::Region { outer: vertices, .. } => {
                    elev_ys.extend(vertices.iter().map(|point| point.y_m));
                }
                Geometry2d::CircularArc { .. } => {}
            }
        }
        let elev_min = elev_ys.iter().copied().fold(f64::INFINITY, f64::min);
        let elev_max = elev_ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(elev_min.is_finite(), "elevation wall face");
        assert!((elev_min - 2.8).abs() < 1e-6, "elevation base {elev_min} must ignore viewpoint origin Z");
        assert!((elev_max - 5.4).abs() < 1e-6, "elevation top {elev_max} must stay on the wall top");

        let section_vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Cut".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(4.0, -2.0, -3.0)),
            axes: ViewAxes::section(Vector2::new(0.0, 1.0)).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(4.0, -2.0), Point2::new(4.0, 2.0)),
                far_depth: None,
            },
            z_range: VerticalRange::new(Metres::new(-3.0).unwrap(), Metres::new(9.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };
        let section = derive_section_view(&section_vp, &mv, &collector, &CutEngineOptions::default()).unwrap();
        let (cut_min, cut_max) = section
            .iter()
            .find_map(|primitive| {
                if primitive.role != PrimitiveRole::CutFill {
                    return None;
                }
                if let Geometry2d::Region { outer, .. } = &primitive.geometry {
                    let min_y = outer.iter().map(|point| point.y_m).fold(f64::INFINITY, f64::min);
                    let max_y = outer.iter().map(|point| point.y_m).fold(f64::NEG_INFINITY, f64::max);
                    Some((min_y, max_y))
                } else {
                    None
                }
            })
            .expect("section wall cut");
        assert!((cut_min - elev_min).abs() < 1e-6 && (cut_max - elev_max).abs() < 1e-6);
    }

    #[test]
    fn test_floorplan_layer_mitered_junctions() {
        let model_id = ModelId::new();
        let mut collector = SceneGeometryCollector::new();

        // Two perpendicular walls meeting at an L-corner at (0, 0)
        let wall1 = CollectedWall {
            handle: acadrust::Handle::new(10),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall_24cm".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(-5.0, 0.0), Point2::new(0.0, 0.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: -0.12,
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

        let wall2 = CollectedWall {
            handle: acadrust::Handle::new(20),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall_24cm".into(),
            height: 3.0,
            base_z: 0.0,
            top_z: 3.0,
            axis_points: vec![Point2::new(0.0, 0.0), Point2::new(0.0, 5.0)],
            layers: vec![CollectedWallLayer {
                material: "Concrete".into(),
                thickness: 0.24,
                function: "Structural".into(),
                axis_offset: -0.12,
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

        collector.walls.push(wall1);
        collector.walls.push(wall2);

        let vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Floor Plan".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::floor_plan(),
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.5).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::ZERO,
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(4.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };

        let mv = ModelView {
            id: ModelViewId::new(),
            name: "EG".into(),
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

        let prims = derive_floorplan_view(&vp, &mv, &collector, &FloorPlanEngineOptions::default()).unwrap();
        let boundaries: Vec<_> = prims.iter().filter(|p| p.role == PrimitiveRole::CutBoundary).collect();
        assert_eq!(boundaries.len(), 2, "Each wall should emit a cut boundary");

        // The mitered corner polygon should feature the diagonal miter vertex at (0.12, -0.12)
        let mut found_miter_corner = false;
        for b in &boundaries {
            if let Geometry2d::Polyline { vertices, .. } = &b.geometry {
                for v in vertices {
                    if (v.x_m - 0.12).abs() < 0.01 && (v.y_m - -0.12).abs() < 0.01 {
                        found_miter_corner = true;
                    }
                }
            }
        }
        assert!(found_miter_corner, "Cut boundary must use mitered junction geometry at the L-corner");
    }

    #[test]
    fn test_section_window_void_hides_background_opening() {
        let model_id = ModelId::new();
        let mut collector = SceneGeometryCollector::new();
        let cut_opening = CollectedOpening {
            handle: acadrust::Handle::new(11),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            host_wall_handle: acadrust::Handle::new(10),
            kind: OpeningKind::Window,
            style_id: Some("Window".into()),
            division_override: None,
            distance_along_axis: 0.0,
            width: 1.2,
            height: 1.4,
            sill_height: 0.9,
            spring_height: 0.0,
            shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
            hinge: LeafOperation::left(),
            swing_side: SwingSide::Exterior,
            reference_side: crate::modules::aec::engine::openings::OpeningReferenceSide::Center,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(0.0, 0.0, 1.6),
            axis_span_local: (-0.6, 0.6),
        };
        let cut_wall = CollectedWall {
            handle: acadrust::Handle::new(10),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall".into(),
            height: 2.8,
            base_z: 0.0,
            top_z: 2.8,
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
            openings: vec![cut_opening],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        let background_opening_ref = ElementRef::new(model_id, ElementId::new());
        let background_opening = CollectedOpening {
            handle: acadrust::Handle::new(21),
            element_ref: background_opening_ref,
            xref_source: None,
            host_wall_handle: acadrust::Handle::new(20),
            kind: OpeningKind::Window,
            style_id: Some("Window".into()),
            division_override: None,
            distance_along_axis: 0.0,
            width: 1.2,
            height: 1.4,
            sill_height: 0.9,
            spring_height: 0.0,
            shape: crate::modules::aec::engine::opening_shape::OpeningShape::Rectangle,
            hinge: LeafOperation::left(),
            swing_side: SwingSide::Exterior,
            reference_side: crate::modules::aec::engine::openings::OpeningReferenceSide::Center,
            cross_axis_offset: 0.0,
            depth: None,
            world_center: Point3::new(0.0, 4.0, 1.6),
            axis_span_local: (-0.6, 0.6),
        };
        let background = CollectedWall {
            handle: acadrust::Handle::new(20),
            element_ref: ElementRef::new(model_id, ElementId::new()),
            xref_source: None,
            style_id: "Wall".into(),
            height: 2.8,
            base_z: 0.0,
            top_z: 2.8,
            axis_points: vec![Point2::new(-3.0, 4.0), Point2::new(3.0, 4.0)],
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
            openings: vec![background_opening],
            phase: crate::modules::aec::engine::plan_view::PlanPhase::New,
        };
        collector.walls.push(cut_wall);
        collector.walls.push(background);

        let vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Schnitt".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(-4.0, 0.0, 0.0)),
            axes: ViewAxes::section_with_side(Vector2::new(1.0, 0.0), false).unwrap(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(-4.0, 0.0), Point2::new(4.0, 0.0)),
                far_depth: Some(PositiveMetres::new(10.0).unwrap()),
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(4.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };
        let mv = ModelView {
            id: ModelViewId::new(),
            name: "Schnitt".into(),
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
        let hole = Point2::new(4.0, 1.6);
        let leaked = primitives.iter().any(|prim| {
            let from_background = prim
                .sources
                .as_slice()
                .iter()
                .any(|source| source.element == background_opening_ref);
            from_background && primitive_covers(prim, hole)
        });
        assert!(!leaked, "background window must not show through the cut window");
    }

    fn symbol_points(prims: &[DerivedPrimitive], element: ElementRef) -> Vec<Point2> {
        let mut points = Vec::new();
        for prim in prims.iter().filter(|prim| {
            prim.role == PrimitiveRole::Symbol
                && prim.sources.as_slice().iter().any(|source| source.element == element)
        }) {
            match &prim.geometry {
                Geometry2d::Line { start, end } => points.extend([*start, *end]),
                Geometry2d::Polyline { vertices, .. } | Geometry2d::Region { outer: vertices, .. } => {
                    points.extend(vertices.iter().copied());
                }
                Geometry2d::CircularArc { .. } => {}
            }
        }
        points
    }

    fn primitive_covers(prim: &DerivedPrimitive, point: Point2) -> bool {
        match &prim.geometry {
            Geometry2d::Region { outer, holes } => {
                silhouette::point_in_polygon(point, outer)
                    && holes.iter().all(|hole| !silhouette::point_in_polygon(point, hole))
            }
            Geometry2d::Line { start, end } => point_on_segment(point, *start, *end),
            Geometry2d::Polyline { vertices, closed } => {
                let count = vertices.len();
                let edges = if *closed { count } else { count.saturating_sub(1) };
                (0..edges).any(|index| {
                    point_on_segment(point, vertices[index], vertices[(index + 1) % count])
                })
            }
            Geometry2d::CircularArc { .. } => false,
        }
    }

    fn point_on_segment(point: Point2, start: Point2, end: Point2) -> bool {
        let dx = end.x_m - start.x_m;
        let dy = end.y_m - start.y_m;
        let len2 = dx * dx + dy * dy;
        if len2 < 1e-12 {
            return false;
        }
        let t = ((point.x_m - start.x_m) * dx + (point.y_m - start.y_m) * dy) / len2;
        if !(0.0..=1.0).contains(&t) {
            return false;
        }
        let px = start.x_m + t * dx;
        let py = start.y_m + t * dy;
        (point.x_m - px).hypot(point.y_m - py) < 1e-3
    }
}
