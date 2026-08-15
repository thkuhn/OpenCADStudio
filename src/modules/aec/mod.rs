// AEC (Architecture / basic BIM) core module.
//
//   Walls group   : parametric wall polyline + OPENCAD_AEC WALL XDATA
//   Rooms group   : closed-loop room detection + room schedule TABLE
//   Storeys group : in-memory storey list (scaffold)
//   IFC group     : minimal IFC4 SPF export

pub mod commands;
pub mod engine;

use crate::modules::{CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

pub struct AecModule;

const WALL_ICON: &[u8] = include_bytes!("../../../assets/icons/box3d.svg");
const WALL_REFRESH_ICON: &[u8] = include_bytes!("../../../assets/icons/sync.svg");
const JOIN_ICON: &[u8] = include_bytes!("../../../assets/icons/fillet.svg");
const EXTEND_ICON: &[u8] = include_bytes!("../../../assets/icons/extend.svg");
const ROOM_ICON: &[u8] = include_bytes!("../../../assets/icons/array_rect.svg");
const SCHEDULE_ICON: &[u8] = include_bytes!("../../../assets/icons/table.svg");
const STOREY_ICON: &[u8] = include_bytes!("../../../assets/icons/layers/panel.svg");
const IFC_ICON: &[u8] = include_bytes!("../../../assets/icons/cui_export.svg");
const MATERIAL_ICON: &[u8] = include_bytes!("../../../assets/icons/hatch/hatch_cross.svg");
const STYLE_ICON: &[u8] = include_bytes!("../../../assets/icons/hatch/hatch_lines.svg");

/// Helper to declare a ribbon tool that fires a named command.
fn tool(id: &'static str, label: &'static str, icon: &'static [u8]) -> ToolDef {
    ToolDef {
        id,
        label,
        icon: IconKind::Svg(icon),
        event: ModuleEvent::Command(id.to_string()),
    }
}

impl CadModule for AecModule {
    fn id(&self) -> &'static str {
        "aec"
    }
    fn title(&self) -> &'static str {
        "Architecture"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
        GROUPS.get_or_init(|| {
            vec![
                RibbonGroup {
                    title: "Walls",
                    tools: vec![
                        RibbonItem::LargeTool(tool("AEC_WALL", "Wall", WALL_ICON)),
                        RibbonItem::LargeTool(tool(
                            "AEC_WALL_REFRESH",
                            "Refresh Walls",
                            WALL_REFRESH_ICON,
                        )),
                        RibbonItem::LargeTool(tool("AEC_WALLJOIN", "Join Walls", JOIN_ICON)),
                        RibbonItem::LargeTool(tool("AEC_WALLEXTEND", "Extend Wall", EXTEND_ICON)),
                        RibbonItem::LargeTool(tool("AEC_WINDOW", "Window", WALL_ICON)),
                        RibbonItem::LargeTool(tool("AEC_DOOR", "Door", WALL_ICON)),
                    ],
                },
                RibbonGroup {
                    title: "Styles",
                    tools: vec![
                        RibbonItem::LargeTool(tool(
                            "AEC_MATERIAL",
                            "Material",
                            MATERIAL_ICON,
                        )),
                        RibbonItem::LargeTool(tool("AEC_STYLE", "Wall Style", STYLE_ICON)),
                        RibbonItem::LargeTool(tool(
                            "AEC_MATERIALMANAGER",
                            "Material Manager",
                            MATERIAL_ICON,
                        )),
                        RibbonItem::LargeTool(tool(
                            "AEC_STYLEMANAGER",
                            "Wall Style Manager",
                            STYLE_ICON,
                        )),
                    ],
                },
                RibbonGroup {
                    title: "Rooms",
                    tools: vec![
                        RibbonItem::LargeTool(tool("AEC_ROOM", "Room", ROOM_ICON)),
                        RibbonItem::LargeTool(tool(
                            "AEC_ROOMSCHEDULE",
                            "Schedule",
                            SCHEDULE_ICON,
                        )),
                    ],
                },
                RibbonGroup {
                    title: "Storeys",
                    tools: vec![RibbonItem::LargeTool(tool(
                        "AEC_STOREY",
                        "Storey",
                        STOREY_ICON,
                    ))],
                },
                RibbonGroup {
                    title: "IFC",
                    tools: vec![RibbonItem::LargeTool(tool(
                        "AEC_IFCEXPORT",
                        "Export IFC",
                        IFC_ICON,
                    ))],
                },
            ]
        })
    }
}
