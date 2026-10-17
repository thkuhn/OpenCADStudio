//! `AEC_PLACE_VIEW` interactive command for placing derived views onto layout sheets.
//!
//! Creates a configured paper-space viewport (`MVIEW`) bound to a chosen ModelView scale
//! (e.g. 1:50, 1:100) and view center.

use glam::DVec3;

use acadrust::entities::Viewport;
use acadrust::types::Vector3 as CadVec3;
use acadrust::Handle;

use crate::command::{
    CadCommand, CmdOption, CmdResult, LiveCommandField, LiveCommandProperties, LiveFieldValue,
    WorkingPlane,
};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "AEC_PLACE_VIEW",
        label: "Place View",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/viewport.svg")),
        event: ModuleEvent::Command("AEC_PLACE_VIEW".to_string()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceViewPhase {
    FirstCorner,
    SecondCorner,
}

pub struct PlaceViewCommand {
    pub phase: PlaceViewPhase,
    pub first_corner: Option<DVec3>,
    pub scale_denominator: u32,
    pub target_carrier: Option<Handle>,
    plane: WorkingPlane,
    last_committed: Option<Handle>,
}

impl PlaceViewCommand {
    pub fn new() -> Self {
        Self {
            phase: PlaceViewPhase::FirstCorner,
            first_corner: None,
            scale_denominator: 50,
            target_carrier: None,
            plane: WorkingPlane::default(),
            last_committed: None,
        }
    }

    pub fn viewport_from_corners(first: DVec3, second: DVec3, scale_denominator: u32) -> Viewport {
        let center_x = (first.x + second.x) * 0.5;
        let center_y = (first.y + second.y) * 0.5;
        let width = (first.x - second.x).abs().max(10.0);
        let height = (first.y - second.y).abs().max(10.0);

        let mut vp = Viewport::new();
        vp.center = CadVec3::new(center_x, center_y, 0.0);
        vp.width = width;
        vp.height = height;
        vp.id = 2;
        vp.custom_scale = 1000.0 / scale_denominator.max(1) as f64;
        vp
    }
}

impl CadCommand for PlaceViewCommand {
    fn name(&self) -> &'static str {
        "AEC_PLACE_VIEW"
    }

    fn prompt(&self) -> String {
        match self.phase {
            PlaceViewPhase::FirstCorner => "Specify first corner of layout viewport:".into(),
            PlaceViewPhase::SecondCorner => "Specify opposite corner of layout viewport:".into(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        vec![
            CmdOption::new("Scale 1:50", "50"),
            CmdOption::new("Scale 1:100", "100"),
            CmdOption::new("Scale 1:20", "20"),
        ]
    }

    fn live_properties(&self) -> Option<LiveCommandProperties> {
        Some(LiveCommandProperties {
            title: "Place View Properties".into(),
            fields: vec![LiveCommandField {
                field_id: "scale",
                label: "Scale 1:X".into(),
                value: LiveFieldValue::Number(self.scale_denominator as f64),
            }],
        })
    }

    fn apply_live_property(&mut self, field_id: &str, value: LiveFieldValue) -> CmdResult {
        if field_id == "scale" {
            if let LiveFieldValue::Number(v) = value {
                self.scale_denominator = (v as u32).max(1);
            }
        }
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.phase {
            PlaceViewPhase::FirstCorner => {
                self.first_corner = Some(pt);
                self.phase = PlaceViewPhase::SecondCorner;
                CmdResult::NeedPoint
            }
            PlaceViewPhase::SecondCorner => {
                if let Some(first) = self.first_corner {
                    let viewport = Self::viewport_from_corners(first, pt, self.scale_denominator);
                    CmdResult::MviewCreate {
                        viewport,
                        preserve_view: false,
                    }
                } else {
                    CmdResult::Cancel
                }
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match text.trim() {
            "50" => {
                self.scale_denominator = 50;
                Some(CmdResult::NeedPoint)
            }
            "100" => {
                self.scale_denominator = 100;
                Some(CmdResult::NeedPoint)
            }
            "20" => {
                self.scale_denominator = 20;
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_preview_wires(&mut self, cursor: DVec3) -> Vec<WireModel> {
        if let Some(p1) = self.first_corner {
            let pts = vec![
                [p1.x, p1.y, 0.0],
                [cursor.x, p1.y, 0.0],
                [cursor.x, cursor.y, 0.0],
                [p1.x, cursor.y, 0.0],
                [p1.x, p1.y, 0.0],
            ];
            vec![WireModel::solid_f64("viewport_rect".into(), pts, WireModel::CYAN, false)]
        } else {
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_place_view_command() {
        let mut cmd = PlaceViewCommand::new();

        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        assert_eq!(cmd.phase, PlaceViewPhase::SecondCorner);

        let res = cmd.on_point(DVec3::new(200.0, 150.0, 0.0));
        assert!(matches!(res, CmdResult::MviewCreate { .. }));
    }
}
