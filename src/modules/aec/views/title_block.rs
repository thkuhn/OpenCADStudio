//! `AEC_TITLEBLOCK` command for inserting a small property-driven title block.

use acadrust::entities::mtext::AttachmentPoint;
use acadrust::entities::{Line, LwPolyline, LwVertex, MText};
use acadrust::types::{Vector2, Vector3};
use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::modules::aec::engine::library::DisplayConfigLibrary;
use crate::modules::aec::engine::plan_view::PlanningStage;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::Scene;

const TITLE_BLOCK_LAYER: &str = "AEC_TITLEBLOCK";
const TITLE_BLOCK_WIDTH: f64 = 4.8;
const TITLE_BLOCK_HEIGHT: f64 = 1.0;
const TITLE_BLOCK_LABEL_WIDTH: f64 = 1.5;
const TITLE_BLOCK_TEXT_HEIGHT: f64 = 0.18;
const TITLE_BLOCK_MARGIN: f64 = 0.15;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_TITLEBLOCK",
        label: "Title Block",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/table.svg")),
        event: ModuleEvent::Command("AEC_TITLEBLOCK".to_string()),
    }
}

pub fn planning_stage_label(stage: PlanningStage) -> String {
    match stage {
        PlanningStage::Permit => crate::tr!("aec", "planning-stage-permit"),
        PlanningStage::Design => crate::tr!("aec", "planning-stage-design"),
        PlanningStage::Execution => crate::tr!("aec", "planning-stage-execution"),
    }
}

pub fn active_stage_value(
    library: &DisplayConfigLibrary,
    active_name: Option<&str>,
) -> Option<String> {
    active_name
        .and_then(|name| library.find(name))
        .map(|config| planning_stage_label(config.planning_stage))
}

fn ensure_property(properties: &mut Vec<(String, String)>, name: &str, value: String) {
    if !properties
        .iter()
        .any(|(existing, _)| existing.trim().eq_ignore_ascii_case(name))
    {
        properties.push((name.to_string(), value));
    }
}

fn property_value(properties: &[(String, String)], name: &str) -> String {
    properties
        .iter()
        .find(|(existing, _)| existing.trim().eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
        .unwrap_or_default()
}

/// Adds the shared drawing properties if they do not already exist and returns their values.
pub fn ensure_aec_drawing_properties(
    scene: &mut Scene,
    active_stage: Option<&str>,
) -> (String, String) {
    let title = scene.document.summary_info.title.clone();
    let mut properties = std::mem::take(&mut scene.document.summary_info.custom_properties);
    ensure_property(&mut properties, "AecProject", title);
    ensure_property(
        &mut properties,
        "AecStage",
        active_stage
            .map(str::to_string)
            .unwrap_or_else(|| planning_stage_label(PlanningStage::default())),
    );
    let values = (
        property_value(&properties, "AecProject"),
        property_value(&properties, "AecStage"),
    );
    scene.document.summary_info.custom_properties = properties;
    values
}

fn add_line(scene: &mut Scene, start: Vector3, end: Vector3) -> Handle {
    let mut entity = EntityType::Line(Line::from_points(start, end));
    entity.common_mut().layer = TITLE_BLOCK_LAYER.to_string();
    scene.add_entity(entity)
}

fn add_text(scene: &mut Scene, value: &str, insertion_point: Vector3, width: f64) -> Handle {
    let mut text = MText::new();
    text.value = value.to_string();
    text.insertion_point = insertion_point;
    text.height = TITLE_BLOCK_TEXT_HEIGHT;
    text.column_data.width = width;
    text.attachment_point = AttachmentPoint::TopLeft;
    let mut entity = EntityType::MText(Box::new(text));
    entity.common_mut().layer = TITLE_BLOCK_LAYER.to_string();
    scene.add_entity(entity)
}

fn add_property_field(
    scene: &mut Scene,
    property: &str,
    value: &str,
    insertion_point: Vector3,
    width: f64,
) -> Handle {
    let handle = add_text(scene, value, insertion_point, width);
    let code = crate::entities::field::custom_property_code(property);
    crate::entities::field::set_text_field(
        &mut scene.document,
        handle,
        Some((code, Vec::new())),
        value,
    );
    handle
}

/// Inserts a framed title block whose project and planning stage are drawing-property fields.
pub fn aec_title_block_at_point(
    scene: &mut Scene,
    at: Vector3,
    active_stage: Option<&str>,
) -> Handle {
    let (project, stage) = ensure_aec_drawing_properties(scene, active_stage);
    scene.ensure_layer(TITLE_BLOCK_LAYER);

    let x = at.x;
    let y = at.y;
    let top = y + TITLE_BLOCK_HEIGHT;
    let right = x + TITLE_BLOCK_WIDTH;
    let middle = y + TITLE_BLOCK_HEIGHT * 0.5;

    let mut frame = LwPolyline::new();
    for point in [
        Vector2::new(x, y),
        Vector2::new(right, y),
        Vector2::new(right, top),
        Vector2::new(x, top),
    ] {
        frame.add_vertex(LwVertex::new(point));
    }
    frame.is_closed = true;
    let mut frame_entity = EntityType::LwPolyline(frame);
    frame_entity.common_mut().layer = TITLE_BLOCK_LAYER.to_string();
    let frame_handle = scene.add_entity(frame_entity);

    let divider_x = x + TITLE_BLOCK_LABEL_WIDTH;
    add_line(
        scene,
        Vector3::new(divider_x, y, at.z),
        Vector3::new(divider_x, top, at.z),
    );
    add_line(
        scene,
        Vector3::new(x, middle, at.z),
        Vector3::new(right, middle, at.z),
    );

    let project_label = crate::tr!("aec", "title-block-project-label");
    let stage_label = crate::tr!("aec", "title-block-stage-label");
    let label_x = x + TITLE_BLOCK_MARGIN;
    let value_x = divider_x + TITLE_BLOCK_MARGIN;
    let upper_row_y = top - TITLE_BLOCK_MARGIN;
    let lower_row_y = middle - TITLE_BLOCK_MARGIN;
    add_text(
        scene,
        &project_label,
        Vector3::new(label_x, upper_row_y, at.z),
        TITLE_BLOCK_LABEL_WIDTH - 2.0 * TITLE_BLOCK_MARGIN,
    );
    let project_handle = add_property_field(
        scene,
        "AecProject",
        &project,
        Vector3::new(value_x, upper_row_y, at.z),
        TITLE_BLOCK_WIDTH - TITLE_BLOCK_LABEL_WIDTH - 2.0 * TITLE_BLOCK_MARGIN,
    );
    add_text(
        scene,
        &stage_label,
        Vector3::new(label_x, lower_row_y, at.z),
        TITLE_BLOCK_LABEL_WIDTH - 2.0 * TITLE_BLOCK_MARGIN,
    );
    let stage_handle = add_property_field(
        scene,
        "AecStage",
        &stage,
        Vector3::new(value_x, lower_row_y, at.z),
        TITLE_BLOCK_WIDTH - TITLE_BLOCK_LABEL_WIDTH - 2.0 * TITLE_BLOCK_MARGIN,
    );

    let field_hosts = [project_handle, stage_handle];
    scene.update_fields(32, Some(&field_hosts));
    scene.bump_geometry();
    frame_handle
}

pub struct TitleBlockCommand;

impl TitleBlockCommand {
    pub fn new() -> Self {
        Self
    }
}

impl CadCommand for TitleBlockCommand {
    fn name(&self) -> &'static str {
        "AEC_TITLEBLOCK"
    }

    fn prompt(&self) -> String {
        crate::tr!("aec", "title-block-prompt-point")
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        CmdResult::Dispatch(format!("AEC_TITLEBLOCK_DO {:.6}|{:.6}", point.x, point.y))
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let Some((x, y)) = text.trim().split_once(',') else {
            return Some(CmdResult::NeedPoint);
        };
        let (Ok(x), Ok(y)) = (x.trim().parse::<f64>(), y.trim().parse::<f64>()) else {
            return Some(CmdResult::NeedPoint);
        };
        Some(CmdResult::Dispatch(format!(
            "AEC_TITLEBLOCK_DO {x:.6}|{y:.6}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom_property(scene: &Scene, name: &str) -> Option<String> {
        scene
            .document
            .summary_info
            .custom_properties
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone())
    }

    #[test]
    fn drawing_properties_use_defaults_without_overwriting_existing_values() {
        let mut scene = Scene::new();
        scene.document.summary_info.title = "Wohnhaus".to_string();
        scene.document.summary_info.custom_properties = vec![
            (
                "AecProject".to_string(),
                "Manueller Projektname".to_string(),
            ),
            ("EigeneEigenschaft".to_string(), "Behalten".to_string()),
        ];

        let (project, stage) = ensure_aec_drawing_properties(&mut scene, Some("Ausführung"));

        assert_eq!(project, "Manueller Projektname");
        assert_eq!(stage, "Ausführung");
        assert_eq!(
            custom_property(&scene, "AecProject").as_deref(),
            Some("Manueller Projektname")
        );
        assert_eq!(
            custom_property(&scene, "AecStage").as_deref(),
            Some("Ausführung")
        );
        assert_eq!(
            custom_property(&scene, "EigeneEigenschaft").as_deref(),
            Some("Behalten")
        );
    }

    #[test]
    fn title_block_fields_resolve_custom_drawing_properties() {
        let mut scene = Scene::new();
        scene.document.summary_info.title = "Mehrfamilienhaus".to_string();
        let frame =
            aec_title_block_at_point(&mut scene, Vector3::new(10.0, 5.0, 0.0), Some("Entwurf"));

        assert!(matches!(
            scene.document.get_entity(frame),
            Some(EntityType::LwPolyline(_))
        ));
        let project = crate::entities::field::evaluate(
            &scene.document,
            r"\AcVar CustomDP.AecProject",
            &[],
            None,
        );
        let stage = crate::entities::field::evaluate(
            &scene.document,
            r"\AcVar CustomDP.AecStage",
            &[],
            None,
        );
        assert_eq!(project.as_deref(), Some("Mehrfamilienhaus"));
        assert_eq!(stage.as_deref(), Some("Entwurf"));
        let hosts = crate::entities::field::acvar_hosts(&scene.document);
        assert_eq!(hosts.len(), 2);
        for (name, value) in [
            ("AecProject", "Aktualisiertes Projekt"),
            ("AecStage", "Genehmigung"),
        ] {
            let property = scene
                .document
                .summary_info
                .custom_properties
                .iter_mut()
                .find(|(key, _)| key == name)
                .expect("AEC drawing property exists");
            property.1 = value.to_string();
        }
        scene.update_fields(32, Some(&hosts));
        assert_eq!(
            crate::entities::field::evaluate(
                &scene.document,
                r"\AcVar CustomDP.AecProject",
                &[],
                None,
            )
            .as_deref(),
            Some("Aktualisiertes Projekt")
        );
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["AEC_TITLEBLOCK", "AEC_TITLEBLOCK_DO"]
});
