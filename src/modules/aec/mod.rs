// AEC (Architecture / basic BIM) core module.
//
//   Walls group   : parametric wall polyline + OPENCAD_AEC WALL XDATA
//   Rooms group   : closed-loop room detection + room schedule TABLE
//   Storeys group : in-memory storey list (scaffold)
//   IFC group     : minimal IFC4 SPF export

pub mod commands;
pub mod engine;
pub mod ifc;
pub mod message;
pub mod project;
pub mod properties;
pub mod rooms;
pub mod spawn;
pub mod state;
pub mod styles;
pub mod ui;
pub mod update;
pub mod walls;

pub use message::AecMessage;
pub(crate) use update::update;
pub use spawn::spawn_command;
pub(crate) use spawn::try_dispatch;
pub use state::{
    AecLayerBuffer, AecLayerGapDrawPick, AecLayerPairDrawPick, AecPendingCopy,
    AecProjectExplorerDeleteTarget, AecState, AecWallStyleSort, StylePickerTarget,
};


use crate::modules::{CadModule, RibbonGroup, RibbonItem};

pub struct AecModule;

impl CadModule for AecModule {
    fn id(&self) -> &'static str {
        "aec"
    }
    fn title(&self) -> &'static str {
        "Architecture"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        use ifc::export as ifc_export;
        use project::{control_planes, explorer};
        use rooms::{room, schedule};
        use styles::{material_manager, plan_manager, wall_style_manager};
        use walls::{door, extend, join, refresh, wall, window};

        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
            vec![
                RibbonGroup {
                    title: "Project",
                    tools: vec![
                        RibbonItem::LargeTool(explorer::tool()),
                        RibbonItem::LargeTool(control_planes::tool()),
                    ],
                },
                RibbonGroup {
                    title: "Walls",
                    tools: vec![
                        RibbonItem::LargeTool(wall::tool()),
                        RibbonItem::LargeTool(refresh::tool()),
                        RibbonItem::LargeTool(join::tool()),
                        RibbonItem::LargeTool(extend::tool()),
                        RibbonItem::LargeTool(window::tool()),
                        RibbonItem::LargeTool(door::tool()),
                    ],
                },
                RibbonGroup {
                    title: "Styles",
                    tools: vec![
                        RibbonItem::LargeTool(material_manager::tool()),
                        RibbonItem::LargeTool(wall_style_manager::tool()),
                        RibbonItem::LargeTool(plan_manager::tool()),
                    ],
                },
                RibbonGroup {
                    title: "Rooms",
                    tools: vec![
                        RibbonItem::LargeTool(room::tool()),
                        RibbonItem::LargeTool(schedule::tool()),
                    ],
                },
                RibbonGroup {
                    title: "IFC",
                    tools: vec![RibbonItem::LargeTool(ifc_export::tool())],
                },
            ]
        })
    }
}
