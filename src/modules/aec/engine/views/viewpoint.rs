//! Persistent Viewpoint definition: geometric view definition in 3D space.

use super::{
    check, Metres, ModelDelta3, ModelId, NonNegativeMetres, Point2, Point3, PositiveMetres,
    Rect2, Revision, StoryId, ValidationError, Vector2, Vector3, ViewpointId,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StoryRef {
    pub model_id: ModelId,
    pub story_id: StoryId,
}

impl StoryRef {
    pub fn new(model_id: ModelId, story_id: StoryId) -> Self {
        Self { model_id, story_id }
    }
}

/// Resolves story placement against current model; does not store a duplicate resolved Z coordinate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ViewOrigin {
    World(Point3),
    StoryRelative {
        story: StoryRef,
        xy_world: Point2,
        offset: Metres,
    },
}

impl ViewOrigin {
    pub fn resolve(
        &self,
        story_elevation: impl FnOnce(StoryRef) -> Option<Metres>,
    ) -> Result<Point3, ValidationError> {
        let point = match self {
            Self::World(point) => *point,
            Self::StoryRelative {
                story,
                xy_world,
                offset,
            } => {
                let z = story_elevation(*story)
                    .ok_or_else(|| ValidationError("story reference is unresolved".into()))?;
                Point3 {
                    x_m: xy_world.x_m,
                    y_m: xy_world.y_m,
                    z_m: z.get() + offset.get(),
                }
            }
        };
        point.validate()?;
        Ok(point)
    }
}

/// Orthonormal basis vectors for the view coordinate system.
/// Both handednesses are supported; positive forward depth points into the scene.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewAxes {
    pub right: Vector3,
    pub up: Vector3,
    pub forward: Vector3,
}

impl ViewAxes {
    pub fn new(right: Vector3, up: Vector3, forward: Vector3) -> Self {
        Self { right, up, forward }
    }

    /// Standard front elevation view axes (looking towards +Y).
    pub fn front_elevation() -> Self {
        Self {
            right: Vector3::new(1.0, 0.0, 0.0),
            up: Vector3::new(0.0, 0.0, 1.0),
            forward: Vector3::new(0.0, 1.0, 0.0),
        }
    }

    /// Elevation view axes looking perpendicular to baseline.
    pub fn elevation(baseline_dir: Vector2) -> Result<Self, ValidationError> {
        Self::elevation_with_side(baseline_dir, false)
    }

    /// Elevation view axes looking perpendicular to baseline, with optional side flip.
    pub fn elevation_with_side(baseline_dir: Vector2, flipped: bool) -> Result<Self, ValidationError> {
        let norm_dir = baseline_dir.normalized()?;
        let (right, forward) = if !flipped {
            (
                Vector3::new(norm_dir.x, norm_dir.y, 0.0),
                Vector3::new(-norm_dir.y, norm_dir.x, 0.0),
            )
        } else {
            (
                Vector3::new(-norm_dir.x, -norm_dir.y, 0.0),
                Vector3::new(norm_dir.y, -norm_dir.x, 0.0),
            )
        };
        let up = Vector3::new(0.0, 0.0, 1.0);
        let axes = Self { right, up, forward };
        axes.validate()?;
        Ok(axes)
    }

    /// Standard floor plan view axes (looking down towards -Z).
    pub fn floor_plan() -> Self {
        Self {
            right: Vector3::new(1.0, 0.0, 0.0),
            up: Vector3::new(0.0, 1.0, 0.0),
            forward: Vector3::new(0.0, 0.0, -1.0),
        }
    }

    /// Standard reflected ceiling plan view axes (looking up towards +Z).
    pub fn ceiling_plan() -> Self {
        Self {
            right: Vector3::new(1.0, 0.0, 0.0),
            up: Vector3::new(0.0, 1.0, 0.0),
            forward: Vector3::new(0.0, 0.0, 1.0),
        }
    }

    /// Section view axes looking along cut normal.
    pub fn section(cut_direction_xy: Vector2) -> Result<Self, ValidationError> {
        Self::section_with_side(cut_direction_xy, false)
    }

    /// Section view axes looking along cut normal, with optional side flip.
    pub fn section_with_side(cut_direction_xy: Vector2, flipped: bool) -> Result<Self, ValidationError> {
        let norm_dir = cut_direction_xy.normalized()?;
        let (right, forward) = if !flipped {
            (
                Vector3::new(norm_dir.x, norm_dir.y, 0.0),
                Vector3::new(-norm_dir.y, norm_dir.x, 0.0),
            )
        } else {
            (
                Vector3::new(-norm_dir.x, -norm_dir.y, 0.0),
                Vector3::new(norm_dir.y, -norm_dir.x, 0.0),
            )
        };
        let up = Vector3::new(0.0, 0.0, 1.0);
        let axes = Self { right, up, forward };
        axes.validate()?;
        Ok(axes)
    }

    pub fn validate(self) -> Result<(), ValidationError> {
        for axis in [self.right, self.up, self.forward] {
            axis.validate_unit()?;
        }
        check(
            self.right.dot(self.up).abs() <= 1e-9
                && self.right.dot(self.forward).abs() <= 1e-9
                && self.up.dot(self.forward).abs() <= 1e-9,
            "view axes must be orthogonal",
        )
    }
}

/// Output reflection in 2D view space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionMapping {
    pub mirror_x: bool,
    pub mirror_y: bool,
}

impl ProjectionMapping {
    pub fn new(mirror_x: bool, mirror_y: bool) -> Self {
        Self { mirror_x, mirror_y }
    }

    pub fn apply(self, point: Point2) -> Point2 {
        Point2 {
            x_m: if self.mirror_x { -point.x_m } else { point.x_m },
            y_m: if self.mirror_y { -point.y_m } else { point.y_m },
        }
    }
}

/// Vertical Z-range defining lower and upper bounds of a view / section body.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerticalRange {
    pub z_min: Metres,
    pub z_max: Metres,
}

impl VerticalRange {
    pub fn new(z_min: Metres, z_max: Metres) -> Result<Self, ValidationError> {
        check(z_min.get() <= z_max.get(), "z_min must be <= z_max")?;
        Ok(Self { z_min, z_max })
    }

    pub fn height(&self) -> f64 {
        self.z_max.get() - self.z_min.get()
    }

    pub fn contains(&self, z: f64) -> bool {
        z >= self.z_min.get() && z <= self.z_max.get()
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        check(
            self.z_min.get().is_finite() && self.z_max.get().is_finite(),
            "vertical range bounds must be finite",
        )?;
        check(
            self.z_min.get() <= self.z_max.get(),
            "z_min must be <= z_max in vertical range",
        )
    }
}

/// Single segment in a straight or jogged section path.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SectionSegment {
    pub start: Point2,
    pub end: Point2,
    /// Indicates whether this segment is a jog transition (offset step) between cut planes.
    pub is_jog_transition: bool,
}

impl SectionSegment {
    pub fn new(start: Point2, end: Point2, is_jog_transition: bool) -> Self {
        Self {
            start,
            end,
            is_jog_transition,
        }
    }

    pub fn length(&self) -> f64 {
        self.start.distance_to(self.end)
    }

    pub fn direction(&self) -> Result<Vector2, ValidationError> {
        Vector2::new(self.end.x_m - self.start.x_m, self.end.y_m - self.start.y_m).normalized()
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        self.start.validate()?;
        self.end.validate()?;
        check(self.length() > 1e-6, "section segment has zero length")
    }
}

/// Straight or jogged section path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum SectionPath {
    Straight {
        start: Point2,
        end: Point2,
    },
    Jogged {
        segments: Vec<SectionSegment>,
    },
}

impl SectionPath {
    pub fn straight(start: Point2, end: Point2) -> Self {
        Self::Straight { start, end }
    }

    pub fn jogged(segments: Vec<SectionSegment>) -> Self {
        Self::Jogged { segments }
    }

    /// Returns all segments represented as a list of `SectionSegment`.
    pub fn segments(&self) -> Vec<SectionSegment> {
        match self {
            Self::Straight { start, end } => vec![SectionSegment::new(*start, *end, false)],
            Self::Jogged { segments } => segments.clone(),
        }
    }

    /// Sum of all cutting segment lengths (excluding jog transitions if configured, or total unfolded length).
    pub fn cut_length(&self) -> f64 {
        match self {
            Self::Straight { start, end } => start.distance_to(*end),
            Self::Jogged { segments } => segments
                .iter()
                .filter(|s| !s.is_jog_transition)
                .map(|s| s.length())
                .sum(),
        }
    }

    /// Total path length including transitions.
    pub fn total_path_length(&self) -> f64 {
        match self {
            Self::Straight { start, end } => start.distance_to(*end),
            Self::Jogged { segments } => segments.iter().map(|s| s.length()).sum(),
        }
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Straight { start, end } => {
                start.validate()?;
                end.validate()?;
                check(start.distance_to(*end) > 1e-6, "section line has zero length")?;
            }
            Self::Jogged { segments } => {
                check(!segments.is_empty(), "jogged section has no segments")?;
                for seg in segments {
                    seg.validate()?;
                }
                // Check continuity between consecutive segments
                for window in segments.windows(2) {
                    let prev_end = window[0].end;
                    let next_start = window[1].start;
                    check(
                        prev_end.distance_to(next_start) < 1e-4,
                        "jogged section segments must be contiguous",
                    )?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanRange {
    /// Global Z distances relative to the cut-plane origin.
    pub above_cut: NonNegativeMetres,
    pub below_cut: NonNegativeMetres,
    pub beyond_below: NonNegativeMetres,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CeilingRange {
    pub above_cut: NonNegativeMetres,
    pub beyond_above: NonNegativeMetres,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DepthRange {
    pub near: NonNegativeMetres,
    /// None means unlimited; Infinity is never used as a JSON sentinel.
    pub far: Option<PositiveMetres>,
}

impl DepthRange {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if let Some(far) = self.far {
            check(
                far.get() > self.near.get(),
                "far depth must exceed near depth",
            )?;
        }
        Ok(())
    }
}

/// View category defining camera geometry and cut behavior.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ViewKind {
    FloorPlan {
        range: PlanRange,
    },
    ReflectedCeilingPlan {
        range: CeilingRange,
    },
    Section {
        path: SectionPath,
        far_depth: Option<PositiveMetres>,
    },
    Elevation {
        depth: DepthRange,
    },
}

/// Geometrical viewpoint definition; purely spatial (no scale, presentation profile, or layout sheet).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Viewpoint {
    pub id: ViewpointId,
    pub name: String,
    pub revision: Revision,
    pub origin: ViewOrigin,
    pub axes: ViewAxes,
    pub kind: ViewKind,
    pub z_range: VerticalRange,
    pub mapping: ProjectionMapping,
    pub crop: Option<Rect2>,
}

impl Viewpoint {
    pub fn validate(&self) -> Result<(), ValidationError> {
        check(!self.name.trim().is_empty(), "viewpoint name is empty")?;
        self.axes.validate()?;
        self.z_range.validate()?;
        match &self.origin {
            ViewOrigin::World(point) => point.validate()?,
            ViewOrigin::StoryRelative { xy_world, .. } => xy_world.validate()?,
        }
        if let Some(crop) = self.crop {
            crop.validate()?;
        }
        match &self.kind {
            ViewKind::FloorPlan { .. } => check(
                self.axes.forward.z < -1.0 + 1e-9,
                "floor plan must look down",
            )?,
            ViewKind::ReflectedCeilingPlan { .. } => check(
                self.axes.forward.z > 1.0 - 1e-9,
                "ceiling plan must look up",
            )?,
            ViewKind::Elevation { depth } => depth.validate()?,
            ViewKind::Section { path, .. } => path.validate()?,
        }
        Ok(())
    }

    /// Outputs unscaled view coordinates (Point2 in view-plane) and forward depth (f64).
    pub fn project(&self, resolved_origin: Point3, point: Point3) -> (Point2, f64) {
        let delta = Vector3 {
            x: point.x_m - resolved_origin.x_m,
            y: point.y_m - resolved_origin.y_m,
            z: point.z_m - resolved_origin.z_m,
        };
        let xy = self.mapping.apply(Point2 {
            x_m: delta.dot(self.axes.right),
            y_m: delta.dot(self.axes.up),
        });
        (xy, delta.dot(self.axes.forward))
    }

    /// Paper elevation of a world Z, independent of the viewpoint origin.
    ///
    /// Sections already place cut fills at world Z. Elevations and section
    /// backgrounds must use the same vertical datum, otherwise a viewpoint
    /// parked below the model shifts every wall. `mapping.mirror_y` is the
    /// only allowed paper-space flip.
    pub fn paper_elevation(&self, world_z: f64) -> f64 {
        if self.mapping.mirror_y {
            -world_z
        } else {
            world_z
        }
    }

    /// Converts a 3D world coordinate into view-plane coordinates (Point2, depth) using the viewpoint's origin.
    ///
    /// The returned Y is origin-relative and must not be used as the architectural elevation of a facade.
    pub fn world_to_view(&self, point: Point3) -> (Point2, f64) {
        let origin = match self.origin {
            ViewOrigin::World(p) => p,
            ViewOrigin::StoryRelative { xy_world, offset, .. } => {
                Point3::new(xy_world.x_m, xy_world.y_m, offset.get())
            }
        };
        self.project(origin, point)
    }

    /// Lifts a permitted view-plane translation into 3D world displacement.
    pub fn lift_view_delta(&self, delta: Point2) -> ModelDelta3 {
        let local = self.mapping.apply(delta);
        ModelDelta3 {
            x_m: self.axes.right.x * local.x_m + self.axes.up.x * local.y_m,
            y_m: self.axes.right.y * local.x_m + self.axes.up.y * local.y_m,
            z_m: self.axes.right.z * local.x_m + self.axes.up.z * local.y_m,
        }
    }

    /// Generates a 2D preview bounding polygon in world XY representing the visible frustum / crop boundary.
    pub fn preview_frustum_polygon_2d(&self) -> Vec<Point2> {
        match &self.kind {
            ViewKind::Section { path, far_depth } => {
                let depth = far_depth.map(|d| d.get()).unwrap_or(5.0);
                match path {
                    SectionPath::Straight { start, end } => {
                        let dir = match (Vector2::new(end.x_m - start.x_m, end.y_m - start.y_m)).normalized() {
                            Ok(d) => d,
                            Err(_) => return vec![*start, *end],
                        };
                        let normal = Vector2::new(-dir.y, dir.x);
                        vec![
                            *start,
                            *end,
                            Point2::new(end.x_m + normal.x * depth, end.y_m + normal.y * depth),
                            Point2::new(start.x_m + normal.x * depth, start.y_m + normal.y * depth),
                        ]
                    }
                    SectionPath::Jogged { segments } => {
                        if segments.is_empty() {
                            return Vec::new();
                        }
                        let mut front = Vec::new();
                        let mut back = Vec::new();
                        for seg in segments {
                            front.push(seg.start);
                            if let Ok(dir) = seg.direction() {
                                let norm = Vector2::new(-dir.y, dir.x);
                                back.push(Point2::new(seg.start.x_m + norm.x * depth, seg.start.y_m + norm.y * depth));
                            }
                        }
                        if let Some(last) = segments.last() {
                            front.push(last.end);
                            if let Ok(dir) = last.direction() {
                                let norm = Vector2::new(-dir.y, dir.x);
                                back.push(Point2::new(last.end.x_m + norm.x * depth, last.end.y_m + norm.y * depth));
                            }
                        }
                        back.reverse();
                        front.extend(back);
                        front
                    }
                }
            }
            ViewKind::Elevation { depth } => {
                let far = depth.far.map(|d| d.get()).unwrap_or(10.0);
                let near = depth.near.get();
                let right_xy = Vector2::new(self.axes.right.x, self.axes.right.y);
                let fwd_xy = Vector2::new(self.axes.forward.x, self.axes.forward.y);
                let origin_xy = match self.origin {
                    ViewOrigin::World(p) => Point2::new(p.x_m, p.y_m),
                    ViewOrigin::StoryRelative { xy_world, .. } => xy_world,
                };
                let w = self.crop.as_ref().map(|c| c.width()).unwrap_or(10.0);
                let half_w = w * 0.5;
                let p1 = Point2::new(origin_xy.x_m - right_xy.x * half_w + fwd_xy.x * near, origin_xy.y_m - right_xy.y * half_w + fwd_xy.y * near);
                let p2 = Point2::new(origin_xy.x_m + right_xy.x * half_w + fwd_xy.x * near, origin_xy.y_m + right_xy.y * half_w + fwd_xy.y * near);
                let p3 = Point2::new(origin_xy.x_m + right_xy.x * half_w + fwd_xy.x * far, origin_xy.y_m + right_xy.y * half_w + fwd_xy.y * far);
                let p4 = Point2::new(origin_xy.x_m - right_xy.x * half_w + fwd_xy.x * far, origin_xy.y_m - right_xy.y * half_w + fwd_xy.y * far);
                vec![p1, p2, p3, p4]
            }
            ViewKind::FloorPlan { .. } | ViewKind::ReflectedCeilingPlan { .. } => {
                if let Some(crop) = &self.crop {
                    let origin_xy = match self.origin {
                        ViewOrigin::World(p) => Point2::new(p.x_m, p.y_m),
                        ViewOrigin::StoryRelative { xy_world, .. } => xy_world,
                    };
                    vec![
                        Point2::new(origin_xy.x_m + crop.min.x_m, origin_xy.y_m + crop.min.y_m),
                        Point2::new(origin_xy.x_m + crop.max.x_m, origin_xy.y_m + crop.min.y_m),
                        Point2::new(origin_xy.x_m + crop.max.x_m, origin_xy.y_m + crop.max.y_m),
                        Point2::new(origin_xy.x_m + crop.min.x_m, origin_xy.y_m + crop.max.y_m),
                    ]
                } else {
                    Vec::new()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_axes_validation() {
        let axes = ViewAxes::front_elevation();
        assert!(axes.validate().is_ok());

        let invalid = ViewAxes {
            right: Vector3::new(1.0, 0.0, 0.0),
            up: Vector3::new(1.0, 0.0, 0.0), // not orthogonal
            forward: Vector3::new(0.0, 0.0, 1.0),
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_section_axes_from_vector() {
        // Section cut line in +X direction -> looking in +Y direction
        let axes = ViewAxes::section(Vector2::new(1.0, 0.0)).unwrap();
        assert!((axes.right.x - 1.0).abs() < 1e-9);
        assert!((axes.up.z - 1.0).abs() < 1e-9);
        assert!((axes.forward.y - 1.0).abs() < 1e-9);
        assert!(axes.validate().is_ok());
    }

    #[test]
    fn test_vertical_range() {
        let v_min = Metres::new(-0.5).unwrap();
        let v_max = Metres::new(6.5).unwrap();
        let range = VerticalRange::new(v_min, v_max).unwrap();
        assert_eq!(range.height(), 7.0);
        assert!(range.contains(0.0));
        assert!(range.contains(6.5));
        assert!(!range.contains(7.0));

        let invalid = VerticalRange::new(v_max, v_min);
        assert!(invalid.is_err());
    }

    #[test]
    fn test_section_path_straight() {
        let p1 = Point2::new(0.0, 0.0);
        let p2 = Point2::new(10.0, 0.0);
        let path = SectionPath::straight(p1, p2);
        assert!(path.validate().is_ok());
        assert_eq!(path.cut_length(), 10.0);
        assert_eq!(path.total_path_length(), 10.0);
    }

    #[test]
    fn test_section_path_jogged() {
        // Jogged section: Main segment 1 (5m), jog transition (2m step), Main segment 2 (5m)
        let s1 = SectionSegment::new(Point2::new(0.0, 0.0), Point2::new(5.0, 0.0), false);
        let s2 = SectionSegment::new(Point2::new(5.0, 0.0), Point2::new(5.0, 2.0), true);
        let s3 = SectionSegment::new(Point2::new(5.0, 2.0), Point2::new(10.0, 2.0), false);
        let path = SectionPath::jogged(vec![s1, s2, s3]);
        assert!(path.validate().is_ok());
        assert_eq!(path.cut_length(), 10.0); // Only cut segments
        assert_eq!(path.total_path_length(), 12.0); // Including 2m transition

        // Discontiguous jogged segments fail validation
        let discontiguous = SectionPath::jogged(vec![
            SectionSegment::new(Point2::new(0.0, 0.0), Point2::new(5.0, 0.0), false),
            SectionSegment::new(Point2::new(6.0, 0.0), Point2::new(10.0, 0.0), false),
        ]);
        assert!(discontiguous.validate().is_err());
    }

    #[test]
    fn test_view_origin_resolution() {
        let world_origin = ViewOrigin::World(Point3::new(1.0, 2.0, 3.0));
        let resolved = world_origin.resolve(|_| None).unwrap();
        assert_eq!(resolved, Point3::new(1.0, 2.0, 3.0));

        let story_ref = StoryRef::new(ModelId::new(), StoryId::new());
        let story_origin = ViewOrigin::StoryRelative {
            story: story_ref,
            xy_world: Point2::new(4.0, 5.0),
            offset: Metres::new(1.2).unwrap(),
        };
        let resolved_story = story_origin
            .resolve(|s| if s == story_ref { Some(Metres::new(2.8).unwrap()) } else { None })
            .unwrap();
        assert_eq!(resolved_story, Point3::new(4.0, 5.0, 4.0));
    }

    #[test]
    fn test_viewpoint_projection_and_lift() {
        let vp = Viewpoint {
            id: ViewpointId::new(),
            name: "Schnitt A-A".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 0.0)),
            axes: ViewAxes::front_elevation(),
            kind: ViewKind::Section {
                path: SectionPath::straight(Point2::new(-5.0, 0.0), Point2::new(5.0, 0.0)),
                far_depth: Some(PositiveMetres::new(10.0).unwrap()),
            },
            z_range: VerticalRange::new(Metres::new(-1.0).unwrap(), Metres::new(4.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };
        assert!(vp.validate().is_ok());

        // Point at (2.0, 5.0, 3.0) projected to front elevation (axes: right=+X, up=+Z, forward=+Y)
        let (view_pt, depth) = vp.project(Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 5.0, 3.0));
        assert_eq!(view_pt, Point2::new(2.0, 3.0));
        assert_eq!(depth, 5.0);

        // Lifting a delta (dx=1.5, dy=0.8) in view space back to world space
        let world_delta = vp.lift_view_delta(Point2::new(1.5, 0.8));
        assert_eq!(world_delta.x_m, 1.5); // Along +X (right)
        assert_eq!(world_delta.y_m, 0.0); // Forward preserved
        assert_eq!(world_delta.z_m, 0.8); // Along +Z (up)
    }

    #[test]
    fn test_floor_plan_validation() {
        let mut vp = Viewpoint {
            id: ViewpointId::new(),
            name: "EG Grundriss".into(),
            revision: Revision(1),
            origin: ViewOrigin::World(Point3::new(0.0, 0.0, 1.0)),
            axes: ViewAxes::floor_plan(), // forward is -Z (looks down)
            kind: ViewKind::FloorPlan {
                range: PlanRange {
                    above_cut: NonNegativeMetres::new(1.0).unwrap(),
                    below_cut: NonNegativeMetres::new(1.0).unwrap(),
                    beyond_below: NonNegativeMetres::new(0.0).unwrap(),
                },
            },
            z_range: VerticalRange::new(Metres::new(0.0).unwrap(), Metres::new(3.0).unwrap()).unwrap(),
            mapping: ProjectionMapping::default(),
            crop: None,
        };
        assert!(vp.validate().is_ok());

        // Floor plan looking upwards must fail
        vp.axes.forward = Vector3::new(0.0, 0.0, 1.0);
        assert!(vp.validate().is_err());
    }
}
