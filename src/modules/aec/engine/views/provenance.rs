//! Provenance and derived primitive data model for associative 2D AEC views.

use super::{
    check, ComponentId, DisplayComponentId, ElementId, ModelId, ModelViewId, PaperMm,
    Point2, PositiveMetres, SemanticFeatureId, ValidationError, Vector2, Vector3,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ElementRef {
    pub model_id: ModelId,
    pub element_id: ElementId,
}

impl ElementRef {
    pub fn new(model_id: ModelId, element_id: ElementId) -> Self {
        Self { model_id, element_id }
    }
}

/// Provenance metadata referencing an external reference (XREF) / multi-storey source drawing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct XrefSourceRef {
    /// Name of the DWG/DXF block record containing the referenced elements (e.g. "Drawing1", "EG").
    pub block_record_name: String,
    /// Optional relative or absolute file path to the external source drawing.
    pub source_path: Option<String>,
    /// 4x4 affine transformation matrix (row-major [m00, m01, m02, m03, ...]) mapping XREF local coordinates to master world coordinates.
    pub transform: [f64; 16],
}

impl XrefSourceRef {
    pub fn identity(block_record_name: impl Into<String>) -> Self {
        Self {
            block_record_name: block_record_name.into(),
            source_path: None,
            transform: [
                1.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
                0.0, 0.0, 0.0, 1.0,
            ],
        }
    }

    pub fn with_translation(
        block_record_name: impl Into<String>,
        source_path: Option<String>,
        tx: f64,
        ty: f64,
        tz: f64,
    ) -> Self {
        Self {
            block_record_name: block_record_name.into(),
            source_path,
            transform: [
                1.0, 0.0, 0.0, tx,
                0.0, 1.0, 0.0, ty,
                0.0, 0.0, 1.0, tz,
                0.0, 0.0, 0.0, 1.0,
            ],
        }
    }

    pub fn from_insert_transform(
        block_record_name: impl Into<String>,
        source_path: Option<String>,
        insert_point: [f64; 3],
        scale: [f64; 3],
        rotation_rad: f64,
    ) -> Self {
        let cos_r = rotation_rad.cos();
        let sin_r = rotation_rad.sin();
        let sx = scale[0];
        let sy = scale[1];
        let sz = scale[2];
        let tx = insert_point[0];
        let ty = insert_point[1];
        let tz = insert_point[2];

        Self {
            block_record_name: block_record_name.into(),
            source_path,
            transform: [
                sx * cos_r, -sy * sin_r, 0.0, tx,
                sx * sin_r,  sy * cos_r, 0.0, ty,
                0.0,         0.0,        sz,  tz,
                0.0,         0.0,        0.0, 1.0,
            ],
        }
    }

    /// Transforms local 3D coordinates (x, y, z) into global master space.
    pub fn transform_coords(&self, x: f64, y: f64, z: f64) -> (f64, f64, f64) {
        let m = &self.transform;
        let nx = m[0] * x + m[1] * y + m[2] * z + m[3];
        let ny = m[4] * x + m[5] * y + m[6] * z + m[7];
        let nz = m[8] * x + m[9] * y + m[10] * z + m[11];
        (nx, ny, nz)
    }

    /// Transforms local 2D XY coordinates into global master space (assuming z = 0).
    pub fn transform_point2(&self, p: Point2) -> Point2 {
        let (x, y, _) = self.transform_coords(p.x_m, p.y_m, 0.0);
        Point2::new(x, y)
    }

    /// Transforms a local Point3 into global master space.
    pub fn transform_point3(&self, p: super::Point3) -> super::Point3 {
        let (x, y, z) = self.transform_coords(p.x_m, p.y_m, p.z_m);
        super::Point3::new(x, y, z)
    }
}

impl Eq for XrefSourceRef {}

impl PartialOrd for XrefSourceRef {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for XrefSourceRef {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.block_record_name
            .cmp(&other.block_record_name)
            .then_with(|| self.source_path.cmp(&other.source_path))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ComponentFace {
    Inner,
    Outer,
    Top,
    Bottom,
    Start,
    End,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum FeatureKey {
    WallAxis,
    WallBase,
    WallTop,
    OpeningSill,
    OpeningHead,
    OpeningLeftJamb,
    OpeningRightJamb,
    SlabTop,
    SlabBottom,
    LayerFace,
    ComponentFace(ComponentFace),
    /// Persistent semantic feature ID issued by the domain model.
    Custom(SemanticFeatureId),
}

/// Identifies the precise architectural source (and optional XREF origin) of a derived 2D graphic element.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceReference {
    pub element: ElementRef,
    pub xref_source: Option<XrefSourceRef>,
    pub component_id: Option<ComponentId>,
    /// None means element/component-level provenance only.
    pub feature: Option<FeatureKey>,
    pub display_component_id: Option<DisplayComponentId>,
}

impl SourceReference {
    pub fn new(element: ElementRef) -> Self {
        Self {
            element,
            xref_source: None,
            component_id: None,
            feature: None,
            display_component_id: None,
        }
    }

    pub fn with_xref(mut self, xref: XrefSourceRef) -> Self {
        self.xref_source = Some(xref);
        self
    }

    pub fn with_component(mut self, comp: ComponentId) -> Self {
        self.component_id = Some(comp);
        self
    }

    pub fn with_feature(mut self, feat: FeatureKey) -> Self {
        self.feature = Some(feat);
        self
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if matches!(self.feature, Some(FeatureKey::ComponentFace(_))) {
            check(
                self.component_id.is_some(),
                "component face requires a component ID",
            )?;
        }
        Ok(())
    }
}

/// Nonempty, canonicalized source set; merged linework may belong to multiple source elements.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<SourceReference>", into = "Vec<SourceReference>")]
pub struct SourceSet(Vec<SourceReference>);

impl SourceSet {
    pub fn new(sources: Vec<SourceReference>) -> Result<Self, ValidationError> {
        Self::try_from(sources)
    }

    pub fn single(source: SourceReference) -> Result<Self, ValidationError> {
        Self::new(vec![source])
    }

    pub fn as_slice(&self) -> &[SourceReference] {
        &self.0
    }

    pub fn contains(&self, source: &SourceReference) -> bool {
        self.0.binary_search(source).is_ok()
    }

    pub fn primary_element(&self) -> Option<ElementRef> {
        self.0.first().map(|s| s.element)
    }
}

impl TryFrom<Vec<SourceReference>> for SourceSet {
    type Error = ValidationError;
    fn try_from(mut sources: Vec<SourceReference>) -> Result<Self, Self::Error> {
        check(
            !sources.is_empty(),
            "derived model primitive requires at least one source reference",
        )?;
        for source in &sources {
            source.validate()?;
        }
        sources.sort();
        sources.dedup();
        Ok(Self(sources))
    }
}

impl From<SourceSet> for Vec<SourceReference> {
    fn from(value: SourceSet) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PrimitiveRole {
    CutBoundary,
    CutFill,
    ProjectedEdge,
    HiddenEdge,
    SurfaceFill,
    Symbol,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Geometry2d {
    Line {
        start: Point2,
        end: Point2,
    },
    Polyline {
        vertices: Vec<Point2>,
        closed: bool,
    },
    CircularArc {
        center: Point2,
        radius: PositiveMetres,
        start_rad: f64,
        sweep_rad: f64,
    },
    Region {
        outer: Vec<Point2>,
        holes: Vec<Vec<Point2>>,
    },
}

fn validate_loop(vertices: &[Point2]) -> Result<(), ValidationError> {
    check(vertices.len() >= 3, "region loop requires at least 3 vertices")?;
    for vertex in vertices {
        vertex.validate()?;
    }
    check(
        vertices.first() != vertices.last(),
        "region loop must use implicit closure (do not repeat first vertex)",
    )
}

impl Geometry2d {
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Line { start, end } => {
                start.validate()?;
                end.validate()?;
                check(start != end, "zero-length line in 2D geometry")
            }
            Self::Polyline { vertices, closed } => {
                check(
                    vertices.len() >= if *closed { 3 } else { 2 },
                    "too few polyline vertices",
                )?;
                for vertex in vertices {
                    vertex.validate()?;
                }
                check(
                    vertices.windows(2).all(|v| v[0] != v[1]),
                    "repeated adjacent polyline vertices",
                )?;
                if *closed {
                    check(
                        vertices.first() != vertices.last(),
                        "closed polyline must use implicit closure",
                    )?;
                }
                Ok(())
            }
            Self::CircularArc {
                center,
                start_rad,
                sweep_rad,
                ..
            } => {
                center.validate()?;
                check(
                    start_rad.is_finite()
                        && sweep_rad.is_finite()
                        && sweep_rad.abs() > 0.0
                        && sweep_rad.abs() <= std::f64::consts::TAU + 1e-9,
                    "arc angles must be finite with 0 < |sweep| <= 2pi",
                )
            }
            Self::Region { outer, holes } => {
                validate_loop(outer)?;
                for hole in holes {
                    validate_loop(hole)?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DepthInterval {
    /// Signed forward depth in metres; cut boundaries normally have [0, 0].
    pub min_m: f64,
    pub max_m: f64,
}

impl DepthInterval {
    pub fn new(min_m: f64, max_m: f64) -> Result<Self, ValidationError> {
        check(
            min_m.is_finite() && max_m.is_finite() && min_m <= max_m,
            "invalid depth interval",
        )?;
        Ok(Self { min_m, max_m })
    }

    pub fn cut_plane() -> Self {
        Self {
            min_m: 0.0,
            max_m: 0.0,
        }
    }

    pub fn validate(self) -> Result<(), ValidationError> {
        check(
            self.min_m.is_finite() && self.max_m.is_finite() && self.min_m <= self.max_m,
            "invalid depth interval",
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    pub fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const BLACK: Self = Self { r: 0, g: 0, b: 0, a: 255 };
    pub const WHITE: Self = Self { r: 255, g: 255, b: 255, a: 255 };
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResolvedStyle {
    pub line_weight_mm: PaperMm,
    pub line_color: Option<Rgba8>,
    pub line_type: Option<String>,
    pub fill_color: Option<Rgba8>,
    pub hatch_pattern: Option<String>,
    pub hatch_scale: Option<f64>,
    pub hatch_angle_rad: Option<f64>,
    pub layer_name: Option<String>,
}

impl ResolvedStyle {
    pub fn solid_stroke(weight_mm: PaperMm) -> Self {
        Self {
            line_weight_mm: weight_mm,
            line_color: Some(Rgba8::BLACK),
            line_type: None,
            fill_color: None,
            hatch_pattern: None,
            hatch_scale: None,
            hatch_angle_rad: None,
            layer_name: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelParameter {
    WallTopOffset,
    WallBaseOffset,
    OpeningSillHeight,
    OpeningHeadHeight,
    SlabOffset,
    SlabThickness,
    WallHeight,
    OpeningWidth,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ModelEditAction {
    TranslateInViewPlane,
    ChangeParameter(ModelParameter),
    MoveAlongHost {
        host: ElementRef,
        axis_world: Vector3,
    },
    SetWallHeight,
    SetOpeningSillHeight,
    SetSlabElevation,
    SetSlabThickness,
}

/// An interactive handle allowing semantic direct modification of the BIM model from 2D views.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EditHandle {
    pub id: String,
    pub position: Point2,
    pub action: ModelEditAction,
    pub direction_hint: Vector2,
    pub target_feature: Option<FeatureKey>,
    pub source: SourceReference,
}

impl EditHandle {
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.position.validate()?;
        self.direction_hint.normalized()?;
        self.source.validate()?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PrimitiveId {
    pub view_id: ModelViewId,
    pub generation: u64,
    pub index: u32,
}

impl PrimitiveId {
    pub fn new(view_id: ModelViewId, generation: u64, index: u32) -> Self {
        Self {
            view_id,
            generation,
            index,
        }
    }
}

/// Generated 2D vector geometry entity bound to model sources and view semantics.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DerivedPrimitive {
    pub id: PrimitiveId,
    pub role: PrimitiveRole,
    pub geometry: Geometry2d,
    pub sources: SourceSet,
    pub depth: DepthInterval,
    pub style: ResolvedStyle,
    pub edit_handles: Vec<EditHandle>,
}

impl DerivedPrimitive {
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.geometry.validate()?;
        self.depth.validate()?;
        for handle in &self.edit_handles {
            handle.validate()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xref_source_ref() {
        let xref = XrefSourceRef::with_translation("Drawing1", Some("drawings/EG.dwg".into()), 0.0, 0.0, 2.8);
        assert_eq!(xref.block_record_name, "Drawing1");
        assert_eq!(xref.transform[3], 0.0);
        assert_eq!(xref.transform[11], 2.8);
    }

    #[test]
    fn test_source_reference_and_set() {
        let elem1 = ElementRef::new(ModelId::new(), ElementId::new());
        let elem2 = ElementRef::new(ModelId::new(), ElementId::new());

        let src1 = SourceReference::new(elem1)
            .with_component(ComponentId::new())
            .with_feature(FeatureKey::WallTop);
        assert!(src1.validate().is_ok());

        // ComponentFace without component_id fails validation
        let invalid_face = SourceReference::new(elem1).with_feature(FeatureKey::ComponentFace(ComponentFace::Outer));
        assert!(invalid_face.validate().is_err());

        let src2 = SourceReference::new(elem2).with_feature(FeatureKey::OpeningSill);

        // SourceSet sorting and deduplication
        let set = SourceSet::new(vec![src2.clone(), src1.clone(), src2.clone()]).unwrap();
        assert_eq!(set.as_slice().len(), 2);
        assert!(set.contains(&src1));
        assert!(set.contains(&src2));

        // Empty SourceSet must fail
        assert!(SourceSet::new(vec![]).is_err());
    }

    #[test]
    fn test_geometry_validation() {
        let p1 = Point2::new(0.0, 0.0);
        let p2 = Point2::new(1.0, 1.0);
        let line = Geometry2d::Line { start: p1, end: p2 };
        assert!(line.validate().is_ok());

        let zero_line = Geometry2d::Line { start: p1, end: p1 };
        assert!(zero_line.validate().is_err());

        let polyline = Geometry2d::Polyline {
            vertices: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0), Point2::new(1.0, 1.0)],
            closed: true,
        };
        assert!(polyline.validate().is_ok());

        let region = Geometry2d::Region {
            outer: vec![
                Point2::new(0.0, 0.0),
                Point2::new(2.0, 0.0),
                Point2::new(2.0, 2.0),
                Point2::new(0.0, 2.0),
            ],
            holes: vec![],
        };
        assert!(region.validate().is_ok());
    }

    #[test]
    fn test_derived_primitive_and_edit_handle() {
        let elem = ElementRef::new(ModelId::new(), ElementId::new());
        let src = SourceReference::new(elem).with_feature(FeatureKey::WallTop);
        let source_set = SourceSet::single(src.clone()).unwrap();

        let handle = EditHandle {
            id: "handle-top".into(),
            position: Point2::new(2.5, 3.0),
            action: ModelEditAction::SetWallHeight,
            direction_hint: Vector2::new(0.0, 1.0),
            target_feature: Some(FeatureKey::WallTop),
            source: src,
        };
        assert!(handle.validate().is_ok());

        let prim = DerivedPrimitive {
            id: PrimitiveId::new(ModelViewId::new(), 1, 0),
            role: PrimitiveRole::CutBoundary,
            geometry: Geometry2d::Line {
                start: Point2::new(0.0, 3.0),
                end: Point2::new(5.0, 3.0),
            },
            sources: source_set,
            depth: DepthInterval::cut_plane(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.35).unwrap()),
            edit_handles: vec![handle],
        };
        assert!(prim.validate().is_ok());
    }
}
