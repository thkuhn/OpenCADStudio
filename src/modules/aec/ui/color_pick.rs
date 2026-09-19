//! Nested AEC colour-pick targets. Core keeps a single `ColorPickTarget::Aec` arm.

use crate::app::Message;
use crate::modules::aec::message::AecMessage;
use crate::modules::aec::ui::aec_ui_util::acad_color_to_editor_string;

/// AEC palette destinations. New colour fields add variants here, not on Core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AecColorPickTarget {
    Material,
    MaterialHatch,
    PlanDemolitionLineColor,
    PlanDemolitionHatchColor,
    PlanDemolitionFillColor,
    PlanExistingLineColor,
    PlanExistingHatchColor,
    PlanExistingFillColor,
    WallStyleSlotLineColor,
    WallStyleSlotHatchColor,
    WallStyleSlotFillColor,
    PlanOverlayLineColor,
    PlanOverlayHatchColor,
    PlanOverlayFillColor,
    PlanContourHatchColor,
}

pub fn message_for(
    target: AecColorPickTarget,
    color: acadrust::types::Color,
) -> Option<Message> {
    match target {
        AecColorPickTarget::Material => {
            Some(Message::Aec(AecMessage::AecStyleManagerMaterialColorPicked(color)))
        }
        AecColorPickTarget::MaterialHatch => {
            // Material hatch colour is still stored as a u32 buffer for the
            // form, resolved from the picked AcadColor's RGB (Index → ACI table).
            let value = match color {
                acadrust::types::Color::Rgb { r, g, b } => {
                    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
                }
                acadrust::types::Color::Index(i) => {
                    let (r, g, b) = acadrust::types::aci_table::aci_to_rgb(i)
                        .unwrap_or((255, 255, 255));
                    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
                }
                _ => 0xFFFFFF,
            };
            Some(Message::Aec(AecMessage::AecStyleManagerMaterialHatchColorChanged(
                value,
            )))
        }
        AecColorPickTarget::PlanDemolitionLineColor => Some(Message::Aec(
            AecMessage::AecPlanManagerDemolitionStyleLineColorChanged(
                acad_color_to_editor_string(color),
            ),
        )),
        AecColorPickTarget::PlanDemolitionHatchColor => Some(Message::Aec(
            AecMessage::AecPlanManagerDemolitionStyleHatchColorChanged(
                acad_color_to_editor_string(color),
            ),
        )),
        AecColorPickTarget::PlanDemolitionFillColor => Some(Message::Aec(
            AecMessage::AecPlanManagerDemolitionStyleFillColorChanged(
                acad_color_to_editor_string(color),
            ),
        )),
        AecColorPickTarget::PlanExistingLineColor => Some(Message::Aec(
            AecMessage::AecPlanManagerExistingStyleLineColorChanged(
                acad_color_to_editor_string(color),
            ),
        )),
        AecColorPickTarget::PlanExistingHatchColor => Some(Message::Aec(
            AecMessage::AecPlanManagerExistingStyleHatchColorChanged(
                acad_color_to_editor_string(color),
            ),
        )),
        AecColorPickTarget::PlanExistingFillColor => Some(Message::Aec(
            AecMessage::AecPlanManagerExistingStyleFillColorChanged(
                acad_color_to_editor_string(color),
            ),
        )),
        AecColorPickTarget::WallStyleSlotLineColor => Some(Message::Aec(
            AecMessage::AecStyleManagerProfileSlotStyleLineColorChanged(
                acad_color_to_editor_string(color),
            ),
        )),
        AecColorPickTarget::WallStyleSlotHatchColor => Some(Message::Aec(
            AecMessage::AecStyleManagerProfileSlotStyleHatchColorChanged(
                acad_color_to_editor_string(color),
            ),
        )),
        AecColorPickTarget::WallStyleSlotFillColor => Some(Message::Aec(
            AecMessage::AecStyleManagerProfileSlotStyleFillColorChanged(
                acad_color_to_editor_string(color),
            ),
        )),
        AecColorPickTarget::PlanOverlayLineColor => Some(Message::Aec(
            AecMessage::AecPlanManagerOverlayLineColorChanged(acad_color_to_editor_string(color)),
        )),
        AecColorPickTarget::PlanOverlayHatchColor => Some(Message::Aec(
            AecMessage::AecPlanManagerOverlayHatchColorChanged(acad_color_to_editor_string(color)),
        )),
        AecColorPickTarget::PlanOverlayFillColor => Some(Message::Aec(
            AecMessage::AecPlanManagerOverlayFillColorChanged(acad_color_to_editor_string(color)),
        )),
        AecColorPickTarget::PlanContourHatchColor => Some(Message::Aec(
            AecMessage::AecPlanManagerContourHatchColorChanged(acad_color_to_editor_string(color)),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_maps_to_style_manager_color() {
        let color = acadrust::types::Color::Rgb {
            r: 10,
            g: 20,
            b: 30,
        };
        match message_for(AecColorPickTarget::Material, color) {
            Some(Message::Aec(AecMessage::AecStyleManagerMaterialColorPicked(c))) => {
                assert_eq!(c, color);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn unknown_is_exhaustive_over_aec_targets() {
        let _ = message_for(
            AecColorPickTarget::PlanContourHatchColor,
            acadrust::types::Color::Rgb {
                r: 1,
                g: 2,
                b: 3,
            },
        );
    }
}
