//! `AEC_STYLES` tree navigation (variant B: tree left, form right).

use iced::widget::{button, column, text};
use iced::{Element, Fill};

use super::super::aec_ui_util::{list_style, section_title};
use crate::app::{AecMessage, Message};
use crate::tr;

/// Leaf nodes of the style tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StylesNode {
    #[default]
    Materials,
    Wall,
    Opening,
    Slab,
    SlabStructural,
    FloorFinish,
    PlanKinds,
}

/// Tree groups in display order with their leaves.
pub const GROUPS: &[(Group, &[StylesNode])] = &[
    (Group::Materials, &[StylesNode::Materials]),
    (
        Group::Components,
        &[
            StylesNode::Wall,
            StylesNode::Opening,
            StylesNode::Slab,
            StylesNode::SlabStructural,
            StylesNode::FloorFinish,
        ],
    ),
    (Group::Display, &[StylesNode::PlanKinds]),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Materials,
    Components,
    Display,
}

impl Group {
    pub fn label(self) -> String {
        match self {
            Group::Materials => tr!("aec", "styles-group-materials"),
            Group::Components => tr!("aec", "styles-group-components"),
            Group::Display => tr!("aec", "styles-group-display"),
        }
    }
}

impl StylesNode {
    pub fn label(self) -> String {
        match self {
            StylesNode::Materials => tr!("aec", "styles-node-materials"),
            StylesNode::Wall => tr!("aec", "styles-node-wall"),
            StylesNode::Opening => tr!("aec", "styles-node-opening"),
            StylesNode::Slab => tr!("aec", "styles-node-slab"),
            StylesNode::SlabStructural => tr!("aec", "styles-node-structural"),
            StylesNode::FloorFinish => tr!("aec", "styles-node-floor-finish"),
            StylesNode::PlanKinds => tr!("aec", "styles-node-plans"),
        }
    }

    /// Legacy manager-open message for this node.
    pub fn open_message(self) -> AecMessage {
        match self {
            StylesNode::Materials => AecMessage::AecMaterialManagerOpen,
            StylesNode::Wall => AecMessage::AecWallStyleManagerOpen,
            StylesNode::Opening => AecMessage::AecOpeningStyleManagerOpen,
            StylesNode::Slab => AecMessage::AecSlabStyleManagerOpen,
            StylesNode::SlabStructural => AecMessage::AecSlabStructuralStyleManagerOpen,
            StylesNode::FloorFinish => AecMessage::AecFloorFinishStyleManagerOpen,
            StylesNode::PlanKinds => AecMessage::AecPlanManagerOpen,
        }
    }
}

/// Left-hand tree with group headings and selectable leaves.
pub fn view<'a>(selected: StylesNode) -> Element<'a, Message> {
    let mut col = column![].spacing(2).width(190);
    for (group, leaves) in GROUPS {
        col = col.push(section_title(std::borrow::Cow::Owned(group.label())));
        for leaf in *leaves {
            let indent = if matches!(leaf, StylesNode::SlabStructural | StylesNode::FloorFinish) {
                24.0
            } else {
                12.0
            };
            col = col.push(
                button(text(leaf.label()).size(12))
                    .on_press(Message::Aec(AecMessage::AecStylesSelectNode(*leaf)))
                    .style(list_style(*leaf == selected))
                    .padding(iced::Padding {
                        top: 6.0,
                        right: 12.0,
                        bottom: 6.0,
                        left: indent,
                    })
                    .width(Fill),
            );
        }
    }
    col.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_node_is_in_exactly_one_group() {
        let all = [
            StylesNode::Materials,
            StylesNode::Wall,
            StylesNode::Opening,
            StylesNode::Slab,
            StylesNode::SlabStructural,
            StylesNode::FloorFinish,
            StylesNode::PlanKinds,
        ];
        for n in all {
            let c = GROUPS.iter().flat_map(|(_, l)| l.iter()).filter(|x| **x == n).count();
            assert_eq!(c, 1, "{n:?}");
        }
    }

}
