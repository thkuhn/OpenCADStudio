//! Viewport context-menu rows that belong to AEC (wall join/extend and
//! junction join-overrides). Core's `ui::popup::context_menu` stays generic;
//! this module is the only place those rows are assembled.

use acadrust::Handle;

use crate::modules::aec::engine::join::JoinOverrideStyle;
use crate::t;
use crate::ui::popup::context_menu::{
    MenuAction, MenuItem, MenuRow, SubmenuId,
};

/// Snapshot of AEC-only inputs for the idle (or exclusive) context menu.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AecMenuSnapshot {
    pub only_walls: bool,
    pub junction: Option<(Handle, usize)>,
    pub exclusive: AecMenuExclusive,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AecMenuExclusive {
    #[default]
    None,
    JunctionOnly,
    LayerPairStyle,
}

/// Small action set so Core's `MenuAction` can stay `PartialEq` without
/// deriving it on the whole `AecMessage` enum.
#[derive(Clone, Debug, PartialEq)]
pub enum AecMenuAction {
    JunctionStyle(JoinOverrideStyle),
    JunctionReset,
    LayerPairStyle(JoinOverrideStyle),
    LayerPairCancel,
    LayerPairPickStart(Handle, usize),
    LayerGapPickStart(Handle, usize),
    JunctionEditorOpen(Handle, usize),
    PropGeomChoice {
        field: &'static str,
        value: String,
    },
}

impl AecMenuAction {
    pub fn to_message(self) -> crate::app::Message {
        use crate::app::{AecMessage, Message};
        match self {
            Self::JunctionStyle(style) => {
                Message::Aec(AecMessage::WallJunctionOverrideSetStyle(style))
            }
            Self::JunctionReset => Message::Aec(AecMessage::WallJunctionOverrideReset),
            Self::LayerPairStyle(style) => {
                Message::Aec(AecMessage::AecJunctionLayerPairSetStyle(style))
            }
            Self::LayerPairCancel => Message::Aec(AecMessage::AecJunctionLayerPairPickCancel),
            Self::LayerPairPickStart(h, i) => {
                Message::Aec(AecMessage::AecJunctionLayerPairPickStart(h, i))
            }
            Self::LayerGapPickStart(h, i) => {
                Message::Aec(AecMessage::AecJunctionLayerGapPickStart(h, i))
            }
            Self::JunctionEditorOpen(h, i) => {
                Message::Aec(AecMessage::AecJunctionEditorOpen(h, i))
            }
            Self::PropGeomChoice { field, value } => {
                Message::PropGeomChoiceChanged { field, value }
            }
        }
    }
}

fn item(label: String, action: AecMenuAction) -> MenuItem {
    MenuItem::new(label, MenuAction::Aec(action))
}

fn join_style_items(layer_pair: bool) -> Vec<MenuItem> {
    let map = |style: JoinOverrideStyle| {
        if layer_pair {
            AecMenuAction::LayerPairStyle(style)
        } else {
            AecMenuAction::JunctionStyle(style)
        }
    };
    vec![
        item(t!("Miter").into_owned(), map(JoinOverrideStyle::Miter)),
        item(t!("Butt").into_owned(), map(JoinOverrideStyle::Butt)),
        item(
            t!("Nähere Kante").into_owned(),
            map(JoinOverrideStyle::NearFace),
        ),
        item(
            t!("Entferntere Kante").into_owned(),
            map(JoinOverrideStyle::FarFace),
        ),
    ]
}

/// Full-menu replacement (junction-only dropdown / layer-pair style pick).
pub fn exclusive_rows(snap: &AecMenuSnapshot) -> Option<Vec<MenuRow>> {
    match snap.exclusive {
        AecMenuExclusive::None => None,
        AecMenuExclusive::LayerPairStyle => {
            let mut rows: Vec<MenuRow> = join_style_items(true)
                .into_iter()
                .map(MenuRow::Item)
                .collect();
            rows.push(MenuRow::Item(item(
                t!("Abbrechen").into_owned(),
                AecMenuAction::LayerPairCancel,
            )));
            Some(rows)
        }
        AecMenuExclusive::JunctionOnly => {
            let mut rows: Vec<MenuRow> = join_style_items(false)
                .into_iter()
                .map(MenuRow::Item)
                .collect();
            rows.push(MenuRow::Item(item(
                t!("Automatisch (zurücksetzen)").into_owned(),
                AecMenuAction::JunctionReset,
            )));
            if let Some((axis_handle, end_index)) = snap.junction {
                rows.extend(junction_detail_items(axis_handle, end_index).into_iter().map(MenuRow::Item));
            }
            Some(rows)
        }
    }
}

fn junction_detail_items(axis_handle: Handle, end_index: usize) -> Vec<MenuItem> {
    vec![
        item(
            t!("Schichtverbindung in Zeichnung...").into_owned(),
            AecMenuAction::LayerPairPickStart(axis_handle, end_index),
        ),
        item(
            t!("Schichtunterbrechung in Zeichnung...").into_owned(),
            AecMenuAction::LayerGapPickStart(axis_handle, end_index),
        ),
        item(
            t!("Detailansicht...").into_owned(),
            AecMenuAction::JunctionEditorOpen(axis_handle, end_index),
        ),
    ]
}

/// Extra idle-menu rows: wall tools after Draw Order, junction submenu at end.
pub fn extend_idle_rows(
    rows: &mut Vec<MenuRow>,
    snap: &AecMenuSnapshot,
    open_submenu: Option<SubmenuId>,
) {
    if snap.only_walls {
        let just_items = vec![
            item(
                t!("Interior").into_owned(),
                AecMenuAction::PropGeomChoice {
                    field: "wall_justification",
                    value: "Interior".into(),
                },
            ),
            item(
                t!("Center").into_owned(),
                AecMenuAction::PropGeomChoice {
                    field: "wall_justification",
                    value: "Center".into(),
                },
            ),
            item(
                t!("Exterior").into_owned(),
                AecMenuAction::PropGeomChoice {
                    field: "wall_justification",
                    value: "Exterior".into(),
                },
            ),
        ];
        let wall_block = vec![
            MenuRow::Item(MenuItem::new(
                t!("Join Walls").into_owned(),
                MenuAction::Command("AEC_WALLJOIN".into()),
            )),
            MenuRow::Item(MenuItem::new(
                t!("Extend Wall").into_owned(),
                MenuAction::Command("AEC_WALLEXTEND".into()),
            )),
            MenuRow::Item(MenuItem::new(
                t!("Reverse Direction").into_owned(),
                MenuAction::Command("AEC_WALLREVERSE".into()),
            )),
            MenuRow::Item(MenuItem::new(
                t!("Add Window").into_owned(),
                MenuAction::Command("AEC_WINDOW".into()),
            )),
            MenuRow::Item(MenuItem::new(
                t!("Add Door").into_owned(),
                MenuAction::Command("AEC_DOOR".into()),
            )),
            MenuRow::Submenu {
                id: SubmenuId::WallJustification,
                label: t!("Change Justification").into_owned(),
                items: just_items,
                open: open_submenu == Some(SubmenuId::WallJustification),
            },
            MenuRow::Separator,
        ];
        // Insert after the Draw Order submenu + separator that follows a
        // selection block (same place the old overlay put wall tools).
        let insert_at = rows
            .iter()
            .position(|r| matches!(r, MenuRow::Submenu { id: SubmenuId::Isolate, .. }))
            .unwrap_or(rows.len());
        rows.splice(insert_at..insert_at, wall_block);
    }

    if snap.junction.is_some() && snap.exclusive == AecMenuExclusive::None {
        let mut items = join_style_items(false);
        items.push(item(
            t!("Automatisch (zurücksetzen)").into_owned(),
            AecMenuAction::JunctionReset,
        ));
        if let Some((h, i)) = snap.junction {
            items.extend(junction_detail_items(h, i));
        }
        rows.push(MenuRow::Separator);
        rows.push(MenuRow::Submenu {
            id: SubmenuId::WallJunction,
            label: t!("Wandverbindung").into_owned(),
            items,
            open: open_submenu == Some(SubmenuId::WallJunction),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusive_layer_pair_is_style_only() {
        let snap = AecMenuSnapshot {
            exclusive: AecMenuExclusive::LayerPairStyle,
            ..AecMenuSnapshot::default()
        };
        let rows = exclusive_rows(&snap).expect("rows");
        assert!(rows.iter().any(|r| matches!(
            r,
            MenuRow::Item(i) if matches!(i.action, MenuAction::Aec(AecMenuAction::LayerPairCancel))
        )));
        assert!(!rows.iter().any(|r| matches!(
            r,
            MenuRow::Item(i) if matches!(i.action, MenuAction::Aec(AecMenuAction::JunctionReset))
        )));
    }

    #[test]
    fn wall_tools_inserted_before_isolate() {
        let mut rows = vec![
            MenuRow::Submenu {
                id: SubmenuId::DrawOrder,
                label: "Draw Order".into(),
                items: vec![],
                open: false,
            },
            MenuRow::Separator,
            MenuRow::Submenu {
                id: SubmenuId::Isolate,
                label: "Isolate".into(),
                items: vec![],
                open: false,
            },
        ];
        extend_idle_rows(
            &mut rows,
            &AecMenuSnapshot {
                only_walls: true,
                ..AecMenuSnapshot::default()
            },
            None,
        );
        let cmds: Vec<_> = rows
            .iter()
            .filter_map(|r| match r {
                MenuRow::Item(i) => match &i.action {
                    MenuAction::Command(c) => Some(c.as_str()),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert!(cmds.contains(&"AEC_WALLJOIN"));
        assert!(matches!(
            rows.iter().find(|r| matches!(
                r,
                MenuRow::Submenu { id: SubmenuId::WallJustification, .. }
            )),
            Some(_)
        ));
        let isolate_idx = rows
            .iter()
            .position(|r| matches!(r, MenuRow::Submenu { id: SubmenuId::Isolate, .. }))
            .unwrap();
        let join_idx = rows
            .iter()
            .position(|r| matches!(
                r,
                MenuRow::Item(i) if i.action == MenuAction::Command("AEC_WALLJOIN".into())
            ))
            .unwrap();
        assert!(join_idx < isolate_idx);
    }
}
