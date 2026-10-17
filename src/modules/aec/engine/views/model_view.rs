//! Persistent ModelView definition: presentation rules, scale, detail policy, and graphic overrides.

use super::{
    check, AnnotationId, ComponentId, DisplayComponentId, ElementRef, ModelViewId, OverrideId,
    PaperMm, Revision, Rgba8, ScaleDenominator, SourceReference, ValidationError, ViewProfileId,
    ViewpointId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetailLevel {
    Coarse,
    Medium,
    Fine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum DetailPolicy {
    FromProfileAndScale,
    Fixed(DetailLevel),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdatePolicy {
    Automatic,
    Manual,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewRevisions {
    pub geometry: Revision,
    pub style: Revision,
    pub presentation: Revision,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum StyleOverride<T> {
    Inherit,
    Set(T),
    Clear,
}

impl<T> Default for StyleOverride<T> {
    fn default() -> Self {
        Self::Inherit
    }
}

impl<T> StyleOverride<T> {
    pub fn is_inherit(&self) -> bool {
        matches!(self, Self::Inherit)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum OverrideSelector {
    Element(ElementRef),
    Component {
        element: ElementRef,
        component_id: ComponentId,
    },
    Feature(SourceReference),
    DisplayComponent {
        element: ElementRef,
        display_component_id: DisplayComponentId,
    },
}

impl OverrideSelector {
    pub fn target_element(&self) -> ElementRef {
        match self {
            Self::Element(e) => *e,
            Self::Component { element, .. } => *element,
            Self::Feature(source) => source.element,
            Self::DisplayComponent { element, .. } => *element,
        }
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if let Self::Feature(source) = self {
            source.validate()?;
        }
        Ok(())
    }
}

/// Local graphic override patch applied to matching primitives in this ModelView.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GraphicPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draw: Option<bool>,
    #[serde(default, skip_serializing_if = "StyleOverride::is_inherit")]
    pub line_weight_mm: StyleOverride<PaperMm>,
    #[serde(default, skip_serializing_if = "StyleOverride::is_inherit")]
    pub line_color: StyleOverride<Rgba8>,
    #[serde(default, skip_serializing_if = "StyleOverride::is_inherit")]
    pub line_type: StyleOverride<String>,
    #[serde(default, skip_serializing_if = "StyleOverride::is_inherit")]
    pub fill_color: StyleOverride<Rgba8>,
    #[serde(default, skip_serializing_if = "StyleOverride::is_inherit")]
    pub hatch_pattern: StyleOverride<String>,
    #[serde(default, skip_serializing_if = "StyleOverride::is_inherit")]
    pub hatch_scale: StyleOverride<f64>,
    #[serde(default, skip_serializing_if = "StyleOverride::is_inherit")]
    pub hatch_angle_rad: StyleOverride<f64>,
    #[serde(default, skip_serializing_if = "StyleOverride::is_inherit")]
    pub layer_name: StyleOverride<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphicOverride {
    pub id: OverrideId,
    pub selector: OverrideSelector,
    pub patch: GraphicPatch,
    pub priority: i32,
    pub enabled: bool,
}

impl GraphicOverride {
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.selector.validate()?;
        Ok(())
    }
}

/// Persistent ModelView linking a Viewpoint with a display profile (DisplayConfig), scale, and overrides.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelView {
    pub id: ModelViewId,
    pub name: String,
    pub viewpoint_id: ViewpointId,
    pub profile_id: ViewProfileId,
    pub scale: ScaleDenominator,
    pub detail: DetailPolicy,
    #[serde(default)]
    pub excluded_elements: BTreeSet<ElementRef>,
    #[serde(default)]
    pub graphic_overrides: Vec<GraphicOverride>,
    #[serde(default)]
    pub annotation_ids: Vec<AnnotationId>,
    pub update_policy: UpdatePolicy,
    pub revisions: ViewRevisions,
}

impl ModelView {
    pub fn validate(&self) -> Result<(), ValidationError> {
        check(!self.name.trim().is_empty(), "model view name is empty")?;
        for rule in &self.graphic_overrides {
            rule.validate()?;
        }
        Ok(())
    }

    pub fn is_element_excluded(&self, element: &ElementRef) -> bool {
        self.excluded_elements.contains(element)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::aec::engine::views::{ElementId, ModelId};

    #[test]
    fn test_model_view_validation_and_exclusion() {
        let elem = ElementRef::new(ModelId::new(), ElementId::new());
        let mut excluded = BTreeSet::new();
        excluded.insert(elem);

        let mv = ModelView {
            id: ModelViewId::new(),
            name: "Schnitt A-A (Ausfuehrung 1:50)".into(),
            viewpoint_id: ViewpointId::new(),
            profile_id: ViewProfileId::new(),
            scale: ScaleDenominator::new(50.0).unwrap(),
            detail: DetailPolicy::FromProfileAndScale,
            excluded_elements: excluded,
            graphic_overrides: vec![],
            annotation_ids: vec![],
            update_policy: UpdatePolicy::Automatic,
            revisions: ViewRevisions::default(),
        };
        assert!(mv.validate().is_ok());
        assert!(mv.is_element_excluded(&elem));

        let other_elem = ElementRef::new(ModelId::new(), ElementId::new());
        assert!(!mv.is_element_excluded(&other_elem));
    }

    #[test]
    fn test_graphic_override_patch() {
        let elem = ElementRef::new(ModelId::new(), ElementId::new());
        let patch = GraphicPatch {
            draw: Some(true),
            line_weight_mm: StyleOverride::Set(PaperMm::new(0.5).unwrap()),
            line_color: StyleOverride::Set(Rgba8::new(255, 0, 0, 255)),
            ..Default::default()
        };

        let override_rule = GraphicOverride {
            id: OverrideId::new(),
            selector: OverrideSelector::Element(elem),
            patch,
            priority: 10,
            enabled: true,
        };
        assert!(override_rule.validate().is_ok());
    }
}
