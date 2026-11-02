use acadrust::entities::{Dimension, DimensionLinear};
use acadrust::types::Vector3;
use acadrust::{CadDocument, EntityType, Handle};

use super::carrier::{ensure_carrier_layer, write_view_carrier, DerivedViewCarrier};
use super::provenance::{DerivedPrimitive, Geometry2d};
use super::{Point2, ValidationError};

#[derive(Clone, Copy, Debug)]
struct Bounds2d {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Bounds2d {
    fn include(&mut self, point: Point2) {
        self.min_x = self.min_x.min(point.x_m);
        self.min_y = self.min_y.min(point.y_m);
        self.max_x = self.max_x.max(point.x_m);
        self.max_y = self.max_y.max(point.y_m);
    }

    fn is_valid(self) -> bool {
        [self.min_x, self.min_y, self.max_x, self.max_y]
            .iter()
            .all(|value| value.is_finite())
            && self.max_x - self.min_x > 1.0e-9
            && self.max_y - self.min_y > 1.0e-9
    }
}

fn transform_point(point: Point2, carrier: &DerivedViewCarrier) -> Point2 {
    let x = point.x_m * carrier.scale;
    let y = point.y_m * carrier.scale;
    let cos = carrier.rotation_rad.cos();
    let sin = carrier.rotation_rad.sin();
    Point2::new(
        carrier.insertion_point.x_m + cos * x - sin * y,
        carrier.insertion_point.y_m + sin * x + cos * y,
    )
}

fn add_geometry_point(bounds: &mut Option<Bounds2d>, point: Point2, carrier: &DerivedViewCarrier) {
    let point = transform_point(point, carrier);
    match bounds {
        Some(bounds) => bounds.include(point),
        None => {
            *bounds = Some(Bounds2d {
                min_x: point.x_m,
                min_y: point.y_m,
                max_x: point.x_m,
                max_y: point.y_m,
            });
        }
    }
}

fn angle_is_on_arc(candidate: f64, start: f64, sweep: f64) -> bool {
    if sweep.abs() >= std::f64::consts::TAU - 1.0e-9 {
        return true;
    }
    let distance = if sweep >= 0.0 {
        (candidate - start).rem_euclid(std::f64::consts::TAU)
    } else {
        (start - candidate).rem_euclid(std::f64::consts::TAU)
    };
    distance <= sweep.abs() + 1.0e-9
}

fn geometry_bounds<'a>(
    geometries: impl IntoIterator<Item = &'a Geometry2d>,
    carrier: &DerivedViewCarrier,
) -> Option<Bounds2d> {
    let mut bounds = None;
    for geometry in geometries {
        match geometry {
            Geometry2d::Line { start, end } => {
                add_geometry_point(&mut bounds, *start, carrier);
                add_geometry_point(&mut bounds, *end, carrier);
            }
            Geometry2d::Polyline { vertices, .. } => {
                for point in vertices {
                    add_geometry_point(&mut bounds, *point, carrier);
                }
            }
            Geometry2d::CircularArc {
                center,
                radius,
                start_rad,
                sweep_rad,
            } => {
                let radius = radius.get();
                let mut angles = vec![*start_rad, *start_rad + *sweep_rad];
                for axis_angle in [
                    0.0,
                    std::f64::consts::FRAC_PI_2,
                    std::f64::consts::PI,
                    3.0 * std::f64::consts::FRAC_PI_2,
                ] {
                    let local_angle = axis_angle - carrier.rotation_rad;
                    if angle_is_on_arc(local_angle, *start_rad, *sweep_rad) {
                        angles.push(local_angle);
                    }
                }
                for angle in angles {
                    add_geometry_point(
                        &mut bounds,
                        Point2::new(
                            center.x_m + radius * angle.cos(),
                            center.y_m + radius * angle.sin(),
                        ),
                        carrier,
                    );
                }
            }
            Geometry2d::Region { outer, holes } => {
                for point in outer.iter().chain(holes.iter().flatten()) {
                    add_geometry_point(&mut bounds, *point, carrier);
                }
            }
        }
    }
    bounds.filter(|bounds| bounds.is_valid())
}

fn annotation_scale_handle(doc: &CadDocument) -> Option<Handle> {
    let current_name = doc.header.current_annotation_scale.trim();
    let requested_name = if current_name.is_empty() {
        "1:1"
    } else {
        current_name
    };
    let named = doc
        .objects
        .iter()
        .find_map(|(handle, object)| match object {
            codec::objects::ObjectType::Scale(scale)
                if !scale.is_temporary && scale.name.eq_ignore_ascii_case(requested_name) =>
            {
                Some(*handle)
            }
            _ => None,
        });
    named.or_else(|| {
        doc.objects
            .iter()
            .find_map(|(handle, object)| match object {
                codec::objects::ObjectType::Scale(scale)
                    if !scale.is_temporary && (scale.factor() - 1.0).abs() <= 1.0e-9 =>
                {
                    Some(*handle)
                }
                _ => None,
            })
    })
}

fn add_dimension(
    doc: &mut CadDocument,
    carrier: &DerivedViewCarrier,
    style_name: &str,
    first: Point2,
    second: Point2,
    definition: Point2,
    rotation: f64,
) -> Result<Handle, ValidationError> {
    let mut dimension = DimensionLinear::new(
        Vector3::new(first.x_m, first.y_m, 0.0),
        Vector3::new(second.x_m, second.y_m, 0.0),
    );
    dimension.rotation = rotation;
    dimension.definition_point = Vector3::new(definition.x_m, definition.y_m, 0.0);
    dimension.base.definition_point = dimension.definition_point;
    dimension.base.style_name = style_name.to_string();
    dimension.base.actual_measurement = dimension.measurement();
    dimension.base.text_middle_point = dimension.definition_point;
    dimension.base.insertion_point = dimension.base.text_middle_point;

    let handle = doc.allocate_handle();
    let mut entity = EntityType::Dimension(Box::new(Dimension::Linear(dimension)));
    entity.common_mut().handle = handle;
    entity.common_mut().layer = "AEC_VIEW_DIM".to_string();
    entity.common_mut().reactors.push(carrier.handle);
    doc.add_entity(entity)
        .map_err(|_| ValidationError("could not add derived view dimension".into()))?;
    Ok(handle)
}

fn apply_annotation_contexts(
    doc: &mut CadDocument,
    entity_handles: &[Handle],
    scale_handle: Option<Handle>,
) {
    let Some(scale_handle) = scale_handle else {
        return;
    };
    let mut core_doc: codec::Document = std::mem::replace(doc, CadDocument::new()).into();
    for handle in entity_handles {
        crate::scene::annotative::create_annotation_context(&mut core_doc, *handle, scale_handle);
    }
    *doc = core_doc.into();
}

fn append_dimensions_for_bounds(
    doc: &mut CadDocument,
    carrier: &mut DerivedViewCarrier,
    bounds: Bounds2d,
) -> Result<usize, ValidationError> {
    if !bounds.is_valid() {
        return Ok(0);
    }

    ensure_carrier_layer(doc, "AEC_VIEW_DIM", false);
    let style_name = doc.header.current_dimstyle_name.clone();
    let text_height = doc
        .dim_styles
        .get(&style_name)
        .map(|style| style.dimtxt)
        .filter(|height| height.is_finite() && *height > 0.0)
        .unwrap_or(0.18);
    let offset = text_height * 2.0;
    let midpoint_x = (bounds.min_x + bounds.max_x) * 0.5;
    let midpoint_y = (bounds.min_y + bounds.max_y) * 0.5;
    let scale_handle = annotation_scale_handle(doc);

    let width_handle = add_dimension(
        doc,
        carrier,
        &style_name,
        Point2::new(bounds.min_x, bounds.min_y),
        Point2::new(bounds.max_x, bounds.min_y),
        Point2::new(midpoint_x, bounds.min_y - offset),
        0.0,
    )?;
    let height_handle = add_dimension(
        doc,
        carrier,
        &style_name,
        Point2::new(bounds.min_x, bounds.min_y),
        Point2::new(bounds.min_x, bounds.max_y),
        Point2::new(bounds.min_x - offset, midpoint_y),
        std::f64::consts::FRAC_PI_2,
    )?;

    apply_annotation_contexts(doc, &[width_handle, height_handle], scale_handle);
    carrier
        .derived_handles
        .extend([width_handle, height_handle]);
    write_view_carrier(doc, carrier);
    super::super::owner_index::set_children(doc, carrier.handle, &carrier.derived_handles);
    Ok(2)
}

/// Adds annotative overall width and height dimensions to a synchronized view carrier.
/// The drawing annotation scale is used; the view's display scale is deliberately not read.
pub fn append_overall_dimensions(
    doc: &mut CadDocument,
    carrier: &mut DerivedViewCarrier,
    primitives: &[DerivedPrimitive],
) -> Result<usize, ValidationError> {
    let Some(bounds) = geometry_bounds(
        primitives.iter().map(|primitive| &primitive.geometry),
        carrier,
    ) else {
        return Ok(0);
    };
    append_dimensions_for_bounds(doc, carrier, bounds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::views::carrier::DerivedViewCarrier;
    use crate::modules::aec::engine::views::provenance::{
        DepthInterval, DerivedPrimitive, ElementRef, Geometry2d, PrimitiveId, PrimitiveRole,
        ResolvedStyle, SourceReference, SourceSet,
    };
    use crate::modules::aec::engine::views::{
        ElementId, ModelId, ModelViewId, PaperMm, ViewpointId,
    };

    fn carrier() -> DerivedViewCarrier {
        DerivedViewCarrier::new(
            Handle::default(),
            ViewpointId::new(),
            ModelViewId::new(),
            Point2::new(10.0, 20.0),
        )
    }

    fn primitive(geometry: Geometry2d) -> DerivedPrimitive {
        let source = SourceReference::new(ElementRef::new(ModelId::new(), ElementId::new()));
        DerivedPrimitive {
            id: PrimitiveId::new(ModelViewId::new(), 0, 0),
            role: PrimitiveRole::ProjectedEdge,
            geometry,
            sources: SourceSet::single(source).unwrap(),
            depth: DepthInterval::cut_plane(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.18).unwrap()),
            edit_handles: vec![],
        }
    }

    #[test]
    fn overall_dimensions_use_transformed_view_bounds_and_current_annotation_scale() {
        let mut scene = crate::scene::Scene::new();
        let scale_handle = scene
            .set_annotation_scale_named("1:50")
            .expect("materialize current annotation scale");
        let mut doc: CadDocument =
            std::mem::replace(&mut scene.document, codec::Document::new()).into();
        let mut carrier = super::super::carrier::create_view_carrier(
            &mut doc,
            ViewpointId::new(),
            ModelViewId::new(),
            Point2::new(10.0, 20.0),
            2.0,
            std::f64::consts::FRAC_PI_2,
            super::super::model_view::UpdatePolicy::Automatic,
        )
        .unwrap();
        carrier.scale = 2.0;
        carrier.rotation_rad = std::f64::consts::FRAC_PI_2;
        let primitives = [primitive(Geometry2d::Region {
            outer: vec![
                Point2::new(0.0, 0.0),
                Point2::new(2.0, 0.0),
                Point2::new(2.0, 1.0),
                Point2::new(0.0, 1.0),
            ],
            holes: vec![],
        })];

        assert_eq!(
            append_overall_dimensions(&mut doc, &mut carrier, &primitives).unwrap(),
            2
        );
        assert_eq!(carrier.derived_handles.len(), 2);

        let dimensions: Vec<_> = carrier
            .derived_handles
            .iter()
            .map(|handle| doc.get_entity(*handle).expect("dimension entity"))
            .collect();
        assert!(dimensions
            .iter()
            .all(|entity| matches!(entity, EntityType::Dimension(_))));
        assert!(dimensions
            .iter()
            .all(|entity| entity.common().layer == "AEC_VIEW_DIM"));
        let dim_style = doc.header.current_dimstyle_name.clone();
        assert!(dimensions.iter().all(|entity| match entity {
            EntityType::Dimension(dimension) => dimension.base().style_name == dim_style,
            _ => false,
        }));
        assert_eq!(annotation_scale_handle(&doc), Some(scale_handle));
        let core_doc: codec::Document = doc.clone().into();
        assert!(dimensions.iter().all(|entity| {
            crate::scene::annotative::object_scale_memberships(&core_doc, entity.common().handle)
                .iter()
                .any(|(_, handle)| *handle == scale_handle)
        }));
        let measurements: Vec<_> = dimensions
            .iter()
            .map(|entity| match entity {
                EntityType::Dimension(dimension) => match &**dimension {
                    Dimension::Linear(linear) => linear.measurement(),
                    _ => unreachable!("overall dimension is linear"),
                },
                _ => unreachable!("overall annotation is a dimension"),
            })
            .collect();
        assert!((measurements[0] - 2.0).abs() < 1.0e-9);
        assert!((measurements[1] - 4.0).abs() < 1.0e-9);
    }

    #[test]
    fn empty_or_degenerate_view_has_no_overall_dimensions() {
        let mut doc = CadDocument::new();
        let mut carrier = carrier();
        let empty: [DerivedPrimitive; 0] = [];
        assert_eq!(
            append_overall_dimensions(&mut doc, &mut carrier, &empty).unwrap(),
            0
        );

        let line_only = [primitive(Geometry2d::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(2.0, 0.0),
        })];
        assert_eq!(
            append_overall_dimensions(&mut doc, &mut carrier, &line_only).unwrap(),
            0
        );
        assert!(carrier.derived_handles.is_empty());
    }
}
