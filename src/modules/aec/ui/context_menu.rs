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
    OpeningGrip(Handle, usize),
    WallBaseGrip(Handle),
    SlabBottomGrip(Handle),
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
    OpeningMove(Handle, usize),
    OpeningResize(Handle, usize),
    OpeningFlipHinge(Handle),
    OpeningFlipSwing(Handle),
    OpeningSetStyle(Handle, String),
    OpeningSetSill(Handle, f64),
    WallBaseOffsetPrompt(Handle),
    WallTopMove(Handle),
    WallSetElevation(Handle, f64),
    WallSetStyle(Handle, String),
    SlabThicknessPrompt(Handle),
    SlabTopMove(Handle),
    SlabSetElevation(Handle, f64),
    SlabSetStyle(Handle, String),
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
            Self::OpeningMove(h, gid) => Message::Aec(AecMessage::AecOpeningGripMove(h, gid)),
            Self::OpeningResize(h, gid) => Message::Aec(AecMessage::AecOpeningGripResize(h, gid)),
            Self::OpeningFlipHinge(h) => Message::Aec(AecMessage::AecOpeningGripFlipHinge(h)),
            Self::OpeningFlipSwing(h) => Message::Aec(AecMessage::AecOpeningGripFlipSwing(h)),
            Self::OpeningSetStyle(h, s) => Message::Aec(AecMessage::AecOpeningGripSetStyle(h, s)),
            Self::OpeningSetSill(h, v) => Message::Aec(AecMessage::AecOpeningGripSetSill(h, v)),
            Self::WallBaseOffsetPrompt(h) => Message::Aec(AecMessage::AecWallBaseOffsetPrompt(h)),
            Self::WallTopMove(h) => Message::Aec(AecMessage::AecWallTopMove(h)),
            Self::WallSetElevation(h, z) => Message::Aec(AecMessage::AecWallSetElevation(h, z)),
            Self::WallSetStyle(h, s) => Message::Aec(AecMessage::AecWallSetStyle(h, s)),
            Self::SlabThicknessPrompt(h) => Message::Aec(AecMessage::AecSlabThicknessPrompt(h)),
            Self::SlabTopMove(h) => Message::Aec(AecMessage::AecSlabTopMove(h)),
            Self::SlabSetElevation(h, z) => Message::Aec(AecMessage::AecSlabSetElevation(h, z)),
            Self::SlabSetStyle(h, s) => Message::Aec(AecMessage::AecSlabSetStyle(h, s)),
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
pub fn exclusive_rows(snap: &AecMenuSnapshot, open_submenu: Option<SubmenuId>) -> Option<Vec<MenuRow>> {
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
        AecMenuExclusive::OpeningGrip(derived_handle, grip_id) => {
            let mut rows = Vec::new();
            rows.push(MenuRow::Item(item(
                crate::tr!("aec", "menu-opening-move"),
                AecMenuAction::OpeningMove(derived_handle, grip_id),
            )));
            rows.push(MenuRow::Item(item(
                crate::tr!("aec", "menu-opening-resize"),
                AecMenuAction::OpeningResize(derived_handle, grip_id),
            )));
            rows.push(MenuRow::Separator);
            rows.push(MenuRow::Item(item(
                crate::tr!("aec", "menu-opening-flip-hinge"),
                AecMenuAction::OpeningFlipHinge(derived_handle),
            )));
            rows.push(MenuRow::Item(item(
                crate::tr!("aec", "menu-opening-flip-swing"),
                AecMenuAction::OpeningFlipSwing(derived_handle),
            )));
            rows.push(MenuRow::Separator);

            let style_lib = crate::modules::aec::engine::project::resolve_style_library(None);
            let style_items: Vec<MenuItem> = style_lib
                .opening_styles
                .iter()
                .map(|st| {
                    item(
                        st.style.name.clone(),
                        AecMenuAction::OpeningSetStyle(derived_handle, st.style.id.clone()),
                    )
                })
                .collect();
            if !style_items.is_empty() {
                rows.push(MenuRow::Submenu {
                    id: SubmenuId::AecOpeningStyle,
                    label: crate::tr!("aec", "menu-change-style"),
                    icon: None,
                    items: style_items,
                    open: open_submenu == Some(SubmenuId::AecOpeningStyle),
                });
            }

            let sills = [0.0, 0.75, 0.80, 0.90, 1.00, 1.10];
            let sill_items: Vec<MenuItem> = sills
                .iter()
                .map(|&s| {
                    item(
                        format!("{:.2} m", s),
                        AecMenuAction::OpeningSetSill(derived_handle, s),
                    )
                })
                .collect();
            rows.push(MenuRow::Submenu {
                id: SubmenuId::AecOpeningSill,
                label: crate::tr!("aec", "menu-opening-sill"),
                icon: None,
                items: sill_items,
                open: open_submenu == Some(SubmenuId::AecOpeningSill),
            });

            Some(rows)
        }
        AecMenuExclusive::WallBaseGrip(derived_handle) => {
            let mut rows = Vec::new();
            rows.push(MenuRow::Item(item(
                crate::tr!("aec", "menu-wall-base-offset"),
                AecMenuAction::WallBaseOffsetPrompt(derived_handle),
            )));
            rows.push(MenuRow::Item(item(
                crate::tr!("aec", "menu-wall-top-move"),
                AecMenuAction::WallTopMove(derived_handle),
            )));
            rows.push(MenuRow::Separator);

            let levels = [-2.80, 0.00, 2.80, 5.60];
            let level_items: Vec<MenuItem> = levels
                .iter()
                .map(|&z| {
                    item(
                        format!("{:.2} m", z),
                        AecMenuAction::WallSetElevation(derived_handle, z),
                    )
                })
                .collect();
            rows.push(MenuRow::Submenu {
                id: SubmenuId::AecWallPlane,
                label: crate::tr!("aec", "menu-assign-plane"),
                icon: None,
                items: level_items,
                open: open_submenu == Some(SubmenuId::AecWallPlane),
            });

            let style_lib = crate::modules::aec::engine::project::resolve_style_library(None);
            let style_items: Vec<MenuItem> = style_lib
                .wall_styles
                .iter()
                .map(|st| {
                    item(
                        st.style.name.clone(),
                        AecMenuAction::WallSetStyle(derived_handle, st.style.id.clone()),
                    )
                })
                .collect();
            if !style_items.is_empty() {
                rows.push(MenuRow::Submenu {
                    id: SubmenuId::AecWallStyle,
                    label: crate::tr!("aec", "menu-wall-style"),
                    icon: None,
                    items: style_items,
                    open: open_submenu == Some(SubmenuId::AecWallStyle),
                });
            }

            Some(rows)
        }
        AecMenuExclusive::SlabBottomGrip(derived_handle) => {
            let mut rows = Vec::new();
            rows.push(MenuRow::Item(item(
                crate::tr!("aec", "menu-slab-thickness"),
                AecMenuAction::SlabThicknessPrompt(derived_handle),
            )));
            rows.push(MenuRow::Item(item(
                crate::tr!("aec", "menu-slab-top-move"),
                AecMenuAction::SlabTopMove(derived_handle),
            )));
            rows.push(MenuRow::Separator);

            let levels = [0.00, 2.80, 5.60];
            let level_items: Vec<MenuItem> = levels
                .iter()
                .map(|&z| {
                    item(
                        format!("{:.2} m", z),
                        AecMenuAction::SlabSetElevation(derived_handle, z),
                    )
                })
                .collect();
            rows.push(MenuRow::Submenu {
                id: SubmenuId::AecSlabPlane,
                label: crate::tr!("aec", "menu-assign-plane"),
                icon: None,
                items: level_items,
                open: open_submenu == Some(SubmenuId::AecSlabPlane),
            });

            let style_lib = crate::modules::aec::engine::project::resolve_style_library(None);
            let style_items: Vec<MenuItem> = style_lib
                .slab_styles
                .iter()
                .map(|st| {
                    item(
                        st.style.name.clone(),
                        AecMenuAction::SlabSetStyle(derived_handle, st.style.id.clone()),
                    )
                })
                .collect();
            if !style_items.is_empty() {
                rows.push(MenuRow::Submenu {
                    id: SubmenuId::AecSlabStyle,
                    label: crate::tr!("aec", "menu-slab-style"),
                    icon: None,
                    items: style_items,
                    open: open_submenu == Some(SubmenuId::AecSlabStyle),
                });
            }

            Some(rows)
        }
    }
}

pub fn derived_view_dropdown_menu(scene: &crate::scene::Scene, handle: Handle, _grip_id: usize) -> Option<AecMenuExclusive> {
    let ent = scene.document.get_entity(handle)?;
    let meta = crate::modules::aec::engine::views::carrier::read_derived_primitive_metadata(ent)?;
    let feat = meta.feature?;
    match feat {
        crate::modules::aec::engine::views::provenance::FeatureKey::OpeningLeftJamb
        | crate::modules::aec::engine::views::provenance::FeatureKey::OpeningRightJamb => {
            Some(AecMenuExclusive::OpeningGrip(handle, crate::modules::aec::properties::DERIVED_VIEW_GRIP_BASE))
        }
        crate::modules::aec::engine::views::provenance::FeatureKey::WallBase => {
            Some(AecMenuExclusive::WallBaseGrip(handle))
        }
        crate::modules::aec::engine::views::provenance::FeatureKey::SlabBottom => {
            Some(AecMenuExclusive::SlabBottomGrip(handle))
        }
        _ => None,
    }
}

pub fn derived_primitive_context_menu(scene: &crate::scene::Scene, handle: Handle) -> Option<AecMenuExclusive> {
    derived_view_dropdown_menu(scene, handle, crate::modules::aec::properties::DERIVED_VIEW_DROPDOWN_GRIP_BASE)
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
            MenuRow::Item(MenuItem::new(
                t!("Add Opening").into_owned(),
                MenuAction::Command("AEC_OPENING".into()),
            )),
            MenuRow::Submenu {
                id: SubmenuId::WallJustification,
                label: t!("Change Justification").into_owned(),
                icon: None,
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
            icon: None,
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
        let rows = exclusive_rows(&snap, None).expect("rows");
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
    fn exclusive_opening_grip_menu_rows() {
        let snap = AecMenuSnapshot {
            exclusive: AecMenuExclusive::OpeningGrip(Handle::new(100), 50_000),
            ..AecMenuSnapshot::default()
        };
        let rows = exclusive_rows(&snap, None).expect("rows");
        assert!(rows.iter().any(|r| matches!(
            r,
            MenuRow::Item(i) if matches!(i.action, MenuAction::Aec(AecMenuAction::OpeningMove(..)))
        )));
        assert!(rows.iter().any(|r| matches!(
            r,
            MenuRow::Item(i) if matches!(i.action, MenuAction::Aec(AecMenuAction::OpeningResize(..)))
        )));
        assert!(rows.iter().any(|r| matches!(
            r,
            MenuRow::Item(i) if matches!(i.action, MenuAction::Aec(AecMenuAction::OpeningFlipHinge(..)))
        )));
        assert!(rows.iter().any(|r| matches!(
            r,
            MenuRow::Item(i) if matches!(i.action, MenuAction::Aec(AecMenuAction::OpeningFlipSwing(..)))
        )));
        assert!(rows.iter().any(|r| matches!(
            r,
            MenuRow::Submenu { id: SubmenuId::AecOpeningSill, .. }
        )));
    }

    #[test]
    fn exclusive_wall_base_and_slab_bottom_rows() {
        let wall_snap = AecMenuSnapshot {
            exclusive: AecMenuExclusive::WallBaseGrip(Handle::new(200)),
            ..AecMenuSnapshot::default()
        };
        let wall_rows = exclusive_rows(&wall_snap, None).expect("wall rows");
        assert!(wall_rows.iter().any(|r| matches!(
            r,
            MenuRow::Item(i) if matches!(i.action, MenuAction::Aec(AecMenuAction::WallBaseOffsetPrompt(..)))
        )));
        assert!(wall_rows.iter().any(|r| matches!(
            r,
            MenuRow::Item(i) if matches!(i.action, MenuAction::Aec(AecMenuAction::WallTopMove(..)))
        )));
        assert!(wall_rows.iter().any(|r| matches!(
            r,
            MenuRow::Submenu { id: SubmenuId::AecWallPlane, .. }
        )));

        let slab_snap = AecMenuSnapshot {
            exclusive: AecMenuExclusive::SlabBottomGrip(Handle::new(300)),
            ..AecMenuSnapshot::default()
        };
        let slab_rows = exclusive_rows(&slab_snap, None).expect("slab rows");
        assert!(slab_rows.iter().any(|r| matches!(
            r,
            MenuRow::Item(i) if matches!(i.action, MenuAction::Aec(AecMenuAction::SlabThicknessPrompt(..)))
        )));
        assert!(slab_rows.iter().any(|r| matches!(
            r,
            MenuRow::Item(i) if matches!(i.action, MenuAction::Aec(AecMenuAction::SlabTopMove(..)))
        )));
        assert!(slab_rows.iter().any(|r| matches!(
            r,
            MenuRow::Submenu { id: SubmenuId::AecSlabPlane, .. }
        )));
    }

    #[test]
    fn wall_tools_inserted_before_isolate() {
        let mut rows = vec![
            MenuRow::Submenu {
                id: SubmenuId::DrawOrder,
                label: "Draw Order".into(),
                icon: None,
                items: vec![],
                open: false,
            },
            MenuRow::Separator,
            MenuRow::Submenu {
                id: SubmenuId::Isolate,
                label: "Isolate".into(),
                icon: None,
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

    #[test]
    fn derived_primitive_context_menu_resolution() {
        use crate::modules::aec::engine::views::provenance::{DerivedPrimitive, ElementRef, FeatureKey, Geometry2d, PrimitiveRole, ResolvedStyle, SourceReference, SourceSet};
        use crate::modules::aec::engine::views::carrier::{create_view_carrier, sync_carrier_primitives};
        use crate::modules::aec::engine::views::model_view::UpdatePolicy;
        use crate::modules::aec::engine::views::{DepthInterval, ModelId, ModelViewId, PaperMm, Point2, ViewpointId};
        use crate::scene::Scene;

        let mut scene = Scene::new();
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();
        let mut carrier = create_view_carrier(
            &mut scene.document,
            vp_id,
            mv_id,
            Point2::new(0.0, 0.0),
            1.0,
            0.0,
            UpdatePolicy::Automatic,
        ).expect("carrier");

        let elem_ref = ElementRef::new(ModelId::new(), crate::modules::aec::engine::views::scene_collector::handle_to_element_id(Handle::new(50)));
        let test_cases = [
            (FeatureKey::OpeningLeftJamb, true),
            (FeatureKey::WallBase, false),
            (FeatureKey::SlabBottom, false),
        ];

        for (feat, is_opening) in test_cases {
            let src = SourceReference {
                element: elem_ref,
                xref_source: None,
                component_id: None,
                feature: Some(feat.clone()),
                display_component_id: None,
            };
            let prim = DerivedPrimitive {
                id: crate::modules::aec::engine::views::provenance::PrimitiveId::new(mv_id, 1, 1),
                role: PrimitiveRole::CutBoundary,
                geometry: Geometry2d::Line { start: Point2::new(0.0, 0.0), end: Point2::new(1.0, 0.0) },
                sources: SourceSet::new(vec![src]).unwrap(),
                depth: DepthInterval::new(0.0, 0.0).unwrap(),
                style: ResolvedStyle::solid_stroke(PaperMm::new(0.35).unwrap()),
                edit_handles: vec![],
            };
            sync_carrier_primitives(&mut scene.document, &mut carrier, &[prim]).expect("sync");
            let child = carrier.derived_handles[0];

            let menu = derived_primitive_context_menu(&scene, child).expect("must have context menu");
            if is_opening {
                assert!(matches!(menu, AecMenuExclusive::OpeningGrip(h, _) if h == child));
            } else if feat == FeatureKey::WallBase {
                assert!(matches!(menu, AecMenuExclusive::WallBaseGrip(h) if h == child));
            } else {
                assert!(matches!(menu, AecMenuExclusive::SlabBottomGrip(h) if h == child));
            }
        }
    }

    #[test]
    fn derived_primitive_menu_actions_execution() {
        use crate::app::OpenCADStudio;
        use crate::modules::aec::engine::views::provenance::{DerivedPrimitive, ElementRef, FeatureKey, Geometry2d, PrimitiveRole, ResolvedStyle, SourceReference, SourceSet};
        use crate::modules::aec::engine::views::carrier::{create_view_carrier, sync_carrier_primitives};
        use crate::modules::aec::engine::views::model_view::UpdatePolicy;
        use crate::modules::aec::engine::views::{DepthInterval, ModelId, ModelViewId, PaperMm, Point2, ViewpointId};
        use crate::modules::aec::engine::wall::Wall;
        use crate::modules::aec::engine::openings::Opening;
        use crate::modules::aec::engine::opening_xdata::write_opening_instance;
        use crate::modules::aec::engine::slab::Slab;
        use crate::modules::aec::engine::slab_xdata::write_slab_record;
        use acadrust::entities::{LwPolyline, LwVertex};
        use acadrust::types::Vector2;
        use codec::EntityType;

        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;

        // 1. Setup 3D Wall
        let wall = Wall::new("W1", 2.8, 0);
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        let mut wall_entity = EntityType::LwPolyline(pl);
        let mut rec = acadrust::xdata::ExtendedDataRecord::new(crate::modules::aec::engine::xdata::AEC_APPID);
        rec.values = crate::modules::aec::engine::xdata::wall_record_for_wall(&wall);
        wall_entity.common_mut().extended_data.add_record(rec);
        let wall_handle = app.tabs[tab].scene.document.add_entity(wall_entity).unwrap();

        // 2. Setup 3D Opening
        let op_h = app.tabs[tab].scene.document.allocate_handle();
        let mut op_ent = EntityType::Point(codec::Point::default());
        op_ent.common_mut().handle = op_h;
        app.tabs[tab].scene.document.add_entity(op_ent).unwrap();
        let opening = Opening::door(op_h, wall_handle, 2.0);
        write_opening_instance(&mut app.tabs[tab].scene, &opening);

        // 3. Setup 3D Slab
        let slab_h = app.tabs[tab].scene.document.allocate_handle();
        let mut slab_pl = LwPolyline::new();
        slab_pl.common.handle = slab_h;
        slab_pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        slab_pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        slab_pl.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
        let slab_ent = EntityType::LwPolyline(slab_pl);
        app.tabs[tab].scene.document.add_entity(slab_ent).unwrap();
        let slab = Slab::new("default", 0);
        write_slab_record(&mut app.tabs[tab].scene.document, slab_h, &slab);

        // 4. Setup View Carrier
        let vp_id = ViewpointId::new();
        let mv_id = ModelViewId::new();
        let mut carrier = create_view_carrier(
            &mut app.tabs[tab].scene.document,
            vp_id,
            mv_id,
            Point2::new(0.0, 0.0),
            1.0,
            0.0,
            UpdatePolicy::Automatic,
        ).expect("carrier");

        // 5. Test Opening derived primitive actions
        let op_ref = ElementRef::new(ModelId::new(), crate::modules::aec::engine::views::scene_collector::handle_to_element_id(op_h));
        let src_op = SourceReference {
            element: op_ref,
            xref_source: None,
            component_id: None,
            feature: Some(FeatureKey::OpeningLeftJamb),
            display_component_id: None,
        };
        let prim_op = DerivedPrimitive {
            id: crate::modules::aec::engine::views::provenance::PrimitiveId::new(mv_id, 1, 1),
            role: PrimitiveRole::CutBoundary,
            geometry: Geometry2d::Line { start: Point2::new(0.0, 0.0), end: Point2::new(1.0, 0.0) },
            sources: SourceSet::new(vec![src_op]).unwrap(),
            depth: DepthInterval::new(0.0, 0.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.35).unwrap()),
            edit_handles: vec![],
        };
        sync_carrier_primitives(&mut app.tabs[tab].scene.document, &mut carrier, &[prim_op]).expect("sync");
        let op_child = carrier.derived_handles[0];

        let menu_op = derived_primitive_context_menu(&app.tabs[tab].scene, op_child).expect("opening menu");
        let snap_op = AecMenuSnapshot {
            only_walls: false,
            junction: None,
            exclusive: menu_op,
        };
        let rows_op = exclusive_rows(&snap_op, None).expect("rows");
        for row in rows_op {
            match row {
                MenuRow::Item(item) => {
                    if let MenuAction::Aec(aec_action) = item.action {
                        let msg = aec_action.to_message();
                        let _ = app.update(msg);
                    }
                }
                MenuRow::Submenu { items, .. } => {
                    for item in items {
                        if let MenuAction::Aec(aec_action) = item.action {
                            let msg = aec_action.to_message();
                            let _ = app.update(msg);
                        }
                    }
                }
                _ => {}
            }
        }

        // 6. Test WallBase derived primitive actions
        let wall_ref = ElementRef::new(ModelId::new(), crate::modules::aec::engine::views::scene_collector::handle_to_element_id(wall_handle));
        let src_wall = SourceReference {
            element: wall_ref,
            xref_source: None,
            component_id: None,
            feature: Some(FeatureKey::WallBase),
            display_component_id: None,
        };
        let prim_wall = DerivedPrimitive {
            id: crate::modules::aec::engine::views::provenance::PrimitiveId::new(mv_id, 1, 2),
            role: PrimitiveRole::CutBoundary,
            geometry: Geometry2d::Line { start: Point2::new(0.0, 0.0), end: Point2::new(1.0, 0.0) },
            sources: SourceSet::new(vec![src_wall]).unwrap(),
            depth: DepthInterval::new(0.0, 0.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.35).unwrap()),
            edit_handles: vec![],
        };
        sync_carrier_primitives(&mut app.tabs[tab].scene.document, &mut carrier, &[prim_wall]).expect("sync");
        let wall_child = carrier.derived_handles[0];

        let menu_wall = derived_primitive_context_menu(&app.tabs[tab].scene, wall_child).expect("wall menu");
        let snap_wall = AecMenuSnapshot {
            only_walls: false,
            junction: None,
            exclusive: menu_wall,
        };
        let rows_wall = exclusive_rows(&snap_wall, None).expect("rows");
        for row in rows_wall {
            match row {
                MenuRow::Item(item) => {
                    if let MenuAction::Aec(aec_action) = item.action {
                        let msg = aec_action.to_message();
                        let _ = app.update(msg);
                    }
                }
                MenuRow::Submenu { items, .. } => {
                    for item in items {
                        if let MenuAction::Aec(aec_action) = item.action {
                            let msg = aec_action.to_message();
                            let _ = app.update(msg);
                        }
                    }
                }
                _ => {}
            }
        }

        // 7. Test SlabBottom derived primitive actions
        let slab_ref = ElementRef::new(ModelId::new(), crate::modules::aec::engine::views::scene_collector::handle_to_element_id(slab_h));
        let src_slab = SourceReference {
            element: slab_ref,
            xref_source: None,
            component_id: None,
            feature: Some(FeatureKey::SlabBottom),
            display_component_id: None,
        };
        let prim_slab = DerivedPrimitive {
            id: crate::modules::aec::engine::views::provenance::PrimitiveId::new(mv_id, 1, 3),
            role: PrimitiveRole::CutBoundary,
            geometry: Geometry2d::Line { start: Point2::new(0.0, 0.0), end: Point2::new(1.0, 0.0) },
            sources: SourceSet::new(vec![src_slab]).unwrap(),
            depth: DepthInterval::new(0.0, 0.0).unwrap(),
            style: ResolvedStyle::solid_stroke(PaperMm::new(0.35).unwrap()),
            edit_handles: vec![],
        };
        sync_carrier_primitives(&mut app.tabs[tab].scene.document, &mut carrier, &[prim_slab]).expect("sync");
        let slab_child = carrier.derived_handles[0];

        let menu_slab = derived_primitive_context_menu(&app.tabs[tab].scene, slab_child).expect("slab menu");
        let snap_slab = AecMenuSnapshot {
            only_walls: false,
            junction: None,
            exclusive: menu_slab,
        };
        let rows_slab = exclusive_rows(&snap_slab, None).expect("rows");
        for row in rows_slab {
            match row {
                MenuRow::Item(item) => {
                    if let MenuAction::Aec(aec_action) = item.action {
                        let msg = aec_action.to_message();
                        let _ = app.update(msg);
                    }
                }
                MenuRow::Submenu { items, .. } => {
                    for item in items {
                        if let MenuAction::Aec(aec_action) = item.action {
                            let msg = aec_action.to_message();
                            let _ = app.update(msg);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
