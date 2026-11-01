use super::provenance::{DerivedPrimitive, PrimitiveRole};
use super::silhouette::{process_silhouette_and_depth, SilhouetteOptions};
use super::ValidationError;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewContributorOutput {
    pub occluders: Vec<DerivedPrimitive>,
    pub graphics: Vec<DerivedPrimitive>,
}

pub trait ViewContributor {
    fn contribute(self, output: &mut ViewContributorOutput);
}

impl ViewContributor for DerivedPrimitive {
    fn contribute(self, output: &mut ViewContributorOutput) {
        if matches!(
            self.role,
            PrimitiveRole::CutFill | PrimitiveRole::SurfaceFill
        ) {
            output.occluders.push(self);
        } else {
            output.graphics.push(self);
        }
    }
}

impl ViewContributor for Vec<DerivedPrimitive> {
    fn contribute(self, output: &mut ViewContributorOutput) {
        for primitive in self {
            primitive.contribute(output);
        }
    }
}

impl ViewContributor for ViewContributorOutput {
    fn contribute(self, output: &mut ViewContributorOutput) {
        output.occluders.extend(self.occluders);
        output.graphics.extend(self.graphics);
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewFrame {
    output: ViewContributorOutput,
}

impl ViewFrame {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, contributor: impl ViewContributor) {
        contributor.contribute(&mut self.output);
    }

    pub fn process(
        mut self,
        options: &SilhouetteOptions,
    ) -> Result<Vec<DerivedPrimitive>, ValidationError> {
        self.output.graphics.extend(self.output.occluders);
        process_silhouette_and_depth(&mut self.output.graphics, options)?;
        Ok(self.output.graphics)
    }
}

pub fn resolve_view_frame(
    primitives: Vec<DerivedPrimitive>,
    options: &SilhouetteOptions,
) -> Result<Vec<DerivedPrimitive>, ValidationError> {
    let mut frame = ViewFrame::new();
    frame.append(primitives);
    frame.process(options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::views::provenance::{
        DepthInterval, ElementRef, FeatureKey, Geometry2d, PrimitiveId, ResolvedStyle,
        SourceReference, SourceSet,
    };
    use crate::modules::aec::engine::views::{ElementId, ModelId, ModelViewId, PaperMm, Point2};

    #[test]
    fn frame_collects_occluders_and_graphics_without_losing_primitives() {
        let source = SourceSet::single(SourceReference {
            element: ElementRef::new(ModelId::new(), ElementId::new()),
            xref_source: None,
            component_id: None,
            feature: Some(FeatureKey::WallTop),
            display_component_id: None,
        })
        .unwrap();
        let primitive = |role, index| DerivedPrimitive {
            id: PrimitiveId::new(ModelViewId::new(), 0, index),
            role,
            geometry: Geometry2d::Line {
                start: Point2::new(index as f64, 0.0),
                end: Point2::new(index as f64 + 1.0, 0.0),
            },
            sources: source.clone(),
            depth: DepthInterval::new(index as f64, index as f64).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.25).unwrap()),
            edit_handles: vec![],
        };
        let mut frame = ViewFrame::new();
        frame.append(vec![
            primitive(PrimitiveRole::CutFill, 1),
            primitive(PrimitiveRole::ProjectedEdge, 2),
        ]);

        let resolved = frame.process(&SilhouetteOptions::default()).unwrap();

        assert_eq!(resolved.len(), 2);
        assert!(resolved
            .iter()
            .any(|item| item.role == PrimitiveRole::CutFill));
        assert!(resolved
            .iter()
            .any(|item| item.role == PrimitiveRole::ProjectedEdge));
    }
}
