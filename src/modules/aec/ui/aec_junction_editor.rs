//! Junction Editor Panel (Step 5 of the Join Constraints plan) — shows every
//! wall participating in a given wall-axis junction node, together with
//! their material layers, and lets the user edit the node-level
//! `default_style` and per-layer-pair join overrides
//! (`crate::modules::aec::engine::join::JunctionOverride`).
//!
//! Mirrors the master-list + detail-form architecture of
//! `aec_material_manager.rs`: here the "master list" is the set of
//! participating walls/layers (read-only) and the "detail form" is the
//! junction override being edited.

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Fill};

use crate::app::{AecMessage, Message};
use crate::modules::aec::commands::JunctionParticipant;
use crate::modules::aec::engine::join::{
    JoinOverrideStyle, LayerGapOverride, LayerPairOverride, LayerRef,
};
use crate::modules::aec::engine::library::StyleLibrary;
use crate::t;
use crate::tr;

/// Everything the panel needs to render one open junction. Owned data is
/// cheap to rebuild each frame (participants come from a topology query,
/// not from persisted state) while edit buffers are borrowed from `App`.
pub struct JunctionEditorState<'a> {
    pub axis_handle: acadrust::Handle,
    pub end_index: usize,
    /// Every wall participating in this junction (including the wall the
    /// panel was opened from), discovered via
    /// `commands::walls_at_junction`.
    pub participants: Vec<JunctionParticipant>,
    pub library: &'a StyleLibrary,
    /// Pending edit buffer for the node-level default style.
    pub default_style: Option<JoinOverrideStyle>,
    /// Pending edit buffer for the layer-pair overrides.
    pub pairs: &'a [LayerPairOverride],
    /// "Add pair" form state. `(layer index, material id)` — the index
    /// disambiguates layers that reuse the same material (e.g. two plaster
    /// layers), since the material id alone cannot.
    pub pair_layer_a: Option<(usize, &'a str)>,
    pub pair_wall_b: Option<acadrust::Handle>,
    pub pair_layer_b: Option<(usize, &'a str)>,
    pub pair_style: JoinOverrideStyle,
    pub gaps: &'a [LayerGapOverride],
    pub gap_layer: Option<(usize, &'a str)>,
    pub gap_from_wall: Option<acadrust::Handle>,
    pub gap_from: Option<(usize, &'a str)>,
    pub gap_to_wall: Option<acadrust::Handle>,
    pub gap_to: Option<(usize, &'a str)>,
}

fn layer_matches(a: &LayerRef, b: &LayerRef) -> bool {
    match (a.layer_id, b.layer_id) {
        (Some(x), Some(y)) if !x.is_nil() && !y.is_nil() => x == y,
        _ => a.material_id == b.material_id && a.role_tag == b.role_tag && a.index == b.index,
    }
}

/// 1-based stack number of `r` on `owner_layers` (the wall that owns the
/// reference). Must not scan every participant: the first same-material
/// layer on another wall would steal the label.
fn layer_number_on(owner_layers: &[LayerRef], r: &LayerRef) -> Option<usize> {
    owner_layers
        .iter()
        .position(|l| layer_matches(l, r))
        .map(|i| i + 1)
}

fn material_name(library: &StyleLibrary, material_id: &str) -> String {
    library
        .materials
        .iter()
        .find(|m| m.id == material_id)
        .map(|m| m.name.clone())
        .unwrap_or_else(|| material_id.to_string())
}

fn style_label(style: &JoinOverrideStyle) -> String {
    match style {
        JoinOverrideStyle::Miter => tr!("aec", "join-miter"),
        JoinOverrideStyle::Butt => tr!("aec", "join-butt"),
        JoinOverrideStyle::OuterFace => tr!("aec", "join-outer-face"),
        JoinOverrideStyle::NearFace => tr!("aec", "join-near-face"),
        JoinOverrideStyle::FarFace => tr!("aec", "join-far-face"),
        JoinOverrideStyle::NoExtend => tr!("aec", "join-no-extend"),
    }
}

fn style_buttons<'a>(
    current: Option<&JoinOverrideStyle>,
    on_pick: impl Fn(JoinOverrideStyle) -> Message + 'a,
) -> Element<'a, Message> {
    let mut r = row![].spacing(4);
    for style in [
        JoinOverrideStyle::Miter,
        JoinOverrideStyle::Butt,
        JoinOverrideStyle::NearFace,
        JoinOverrideStyle::FarFace,
        JoinOverrideStyle::NoExtend,
    ] {
        let selected = current == Some(&style);
        let label = style_label(&style);
        r = r.push(
            button(text(label).size(11))
                .style(if selected { button::primary } else { button::secondary })
                .padding([4, 8])
                .on_press(on_pick(style)),
        );
    }
    r.into()
}

pub fn view_window<'a>(state: JunctionEditorState<'a>) -> Element<'a, Message> {
    let JunctionEditorState {
        axis_handle,
        end_index,
        participants,
        library,
        default_style,
        pairs,
        pair_layer_a,
        pair_wall_b,
        pair_layer_b,
        pair_style,
        gaps,
        gap_layer,
        gap_from_wall,
        gap_from,
        gap_to_wall,
        gap_to,
    } = state;

    // ── Master list: participating walls + their layers ──────────────────
    let mut wall_list = column![text(t!("Beteiligte Wände")).size(12)].spacing(6);
    for p in &participants {
        let is_current = p.axis_handle == axis_handle && p.end_index == end_index;
        let mut layers_col = column![].spacing(2);
        for (idx, l) in p.layers.iter().enumerate() {
            layers_col = layers_col.push(
                text(format!("#{} {}", idx + 1, material_name(library, &l.material_id))).size(10),
            );
        }
        let end_label = if p.is_through {
            t!("durchlaufend").into_owned()
        } else {
            tr!("aec", "end-n", n = p.end_index)
        };
        wall_list = wall_list.push(
            container(
                column![
                    text(format!(
                        "{}{} ({})",
                        if is_current { "▶ " } else { "" },
                        tr!("aec", "wall-n", n = p.axis_handle.value()),
                        end_label
                    ))
                    .size(11),
                    layers_col,
                ]
                .spacing(2),
            )
            .padding(6)
            .style(container::bordered_box),
        );
    }

    // ── Node-level default style ──────────────────────────────────────────
    let default_style_row = column![
        text(t!("Standardstil (default_style)")).size(11),
        style_buttons(default_style.as_ref(), |s| Message::Aec(AecMessage::AecJunctionEditorSetDefaultStyle(s))),
        button(text(t!("Zurücksetzen")).size(10))
            .style(button::secondary)
            .padding([3, 8])
            .on_press(Message::Aec(AecMessage::AecJunctionEditorResetDefaultStyle)),
    ]
    .spacing(6);

    // ── Existing layer-pair overrides ─────────────────────────────────────
    let mut pairs_col = column![text(t!("Layer-Paar-Overrides")).size(11)].spacing(4);
    if pairs.is_empty() {
        pairs_col = pairs_col.push(text(t!("Keine Layer-Paar-Overrides.")).size(10));
    } else {
        let current_layers: &[LayerRef] = participants
            .iter()
            .find(|p| p.axis_handle == axis_handle && p.end_index == end_index)
            .map(|p| p.layers.as_slice())
            .unwrap_or(&[]);
        for (idx, pair) in pairs.iter().enumerate() {
            let a_name = match layer_number_on(current_layers, &pair.layer_a) {
                Some(n) => format!("#{} {}", n, material_name(library, &pair.layer_a.material_id)),
                None => material_name(library, &pair.layer_a.material_id).to_string(),
            };
            let b_label = pair
                .layer_b
                .as_ref()
                .map(|b| {
                    let b_layers: &[LayerRef] = pair_wall_b
                        .and_then(|h| participants.iter().find(|p| p.axis_handle == h))
                        .map(|p| p.layers.as_slice())
                        .or_else(|| {
                            participants.iter().find(|p| {
                                p.layers.iter().any(|l| layer_matches(l, b))
                            }).map(|p| p.layers.as_slice())
                        })
                        .unwrap_or(&[]);
                    match layer_number_on(b_layers, b) {
                        Some(n) => format!("#{} {}", n, material_name(library, &b.material_id)),
                        None => material_name(library, &b.material_id).to_string(),
                    }
                })
                .unwrap_or_else(|| t!("Außenkante").into_owned());
            pairs_col = pairs_col.push(
                column![
                    row![
                        text(format!("{} ↔ {}", a_name, b_label)).size(10),
                        iced::widget::Space::new(),
                        button(text(t!("Entfernen")).size(9))
                            .style(button::secondary)
                            .padding([2, 6])
                            .on_press(Message::Aec(AecMessage::AecJunctionEditorRemovePair(idx))),
                    ]
                    .spacing(6),
                    style_buttons(Some(&pair.style), move |s| {
                        Message::Aec(AecMessage::AecJunctionEditorSetPairStyle(idx, s))
                    }),
                ]
                .spacing(4),
            );
        }
    }

    // ── "Add pair" form ─────────────────────────────────────────────────
    let current_wall_layers: &[LayerRef] = participants
        .iter()
        .find(|p| p.axis_handle == axis_handle && p.end_index == end_index)
        .map(|p| p.layers.as_slice())
        .unwrap_or(&[]);

    let mut layer_a_row = row![].spacing(4);
    for (idx, layer) in current_wall_layers.iter().enumerate() {
        let selected = pair_layer_a == Some((idx, layer.material_id.as_str()));
        layer_a_row = layer_a_row.push(
            button(text(format!("#{} {}", idx + 1, material_name(library, &layer.material_id))).size(10))
                .style(if selected { button::primary } else { button::secondary })
                .padding([3, 6])
                .on_press(Message::Aec(AecMessage::AecJunctionEditorPairLayerAChanged(
                    idx,
                    layer.material_id.clone(),
                ))),
        );
    }

    let mut wall_b_row = row![].spacing(4);
    wall_b_row = wall_b_row.push(
        button(text(t!("Außenkante / keine")).size(10))
            .style(if pair_wall_b.is_none() { button::primary } else { button::secondary })
            .padding([3, 6])
            .on_press(Message::Aec(AecMessage::AecJunctionEditorPairWallBChanged(None))),
    );
    for p in &participants {
        if p.axis_handle == axis_handle && p.end_index == end_index {
            continue;
        }
        let selected = pair_wall_b == Some(p.axis_handle);
        wall_b_row = wall_b_row.push(
            button(text(format!("Wand #{}", p.axis_handle.value())).size(10))
                .style(if selected { button::primary } else { button::secondary })
                .padding([3, 6])
                .on_press(Message::Aec(AecMessage::AecJunctionEditorPairWallBChanged(Some(p.axis_handle)))),
        );
    }

    let mut layer_b_row = row![].spacing(4);
    if let Some(wall_b) = pair_wall_b {
        if let Some(p) = participants.iter().find(|p| p.axis_handle == wall_b) {
            for (idx, l) in p.layers.iter().enumerate() {
                let selected = pair_layer_b == Some((idx, l.material_id.as_str()));
                layer_b_row = layer_b_row.push(
                    button(text(format!("#{} {}", idx + 1, material_name(library, &l.material_id))).size(10))
                        .style(if selected { button::primary } else { button::secondary })
                        .padding([3, 6])
                        .on_press(Message::Aec(AecMessage::AecJunctionEditorPairLayerBChanged(idx, l.material_id.clone()))),
                );
            }
        }
    } else {
        layer_b_row = layer_b_row.push(text(t!("(nicht erforderlich)")).size(9));
    }

    let mut gaps_col = column![text(t!("Schichtunterbrechungen")).size(11)].spacing(4);
    if gaps.is_empty() {
        gaps_col = gaps_col.push(text(t!("Keine Unterbrechungen.")).size(10));
    } else {
        for (idx, gap) in gaps.iter().enumerate() {
            let name = |r: &LayerRef| material_name(library, &r.material_id);
            gaps_col = gaps_col.push(
                row![
                    text(format!(
                        "{}  |  {} → {}",
                        name(&gap.layer),
                        name(&gap.from),
                        name(&gap.to)
                    ))
                    .size(10),
                    iced::widget::Space::new(),
                    button(text(t!("Entfernen")).size(9))
                        .style(button::secondary)
                        .padding([2, 6])
                        .on_press(Message::Aec(AecMessage::AecJunctionEditorRemoveGap(idx))),
                ]
                .spacing(6),
            );
        }
    }

    let through_part = participants.iter().find(|p| p.is_through);
    let gap_layer_source: &[LayerRef] = through_part
        .map(|p| p.layers.as_slice())
        .unwrap_or(current_wall_layers);
    let mut gap_layer_row = row![].spacing(4);
    for (idx, layer) in gap_layer_source.iter().enumerate() {
        let selected = gap_layer == Some((idx, layer.material_id.as_str()));
        gap_layer_row = gap_layer_row.push(
            button(text(format!("#{} {}", idx + 1, material_name(library, &layer.material_id))).size(10))
                .style(if selected { button::primary } else { button::secondary })
                .padding([3, 6])
                .on_press(Message::Aec(AecMessage::AecJunctionEditorGapLayerChanged(
                    idx,
                    layer.material_id.clone(),
                ))),
        );
    }

    let other_walls: Vec<&JunctionParticipant> = participants
        .iter()
        .filter(|p| !p.is_through)
        .collect();
    let mut gap_from_wall_row = row![].spacing(4);
    for p in &other_walls {
        let selected = gap_from_wall == Some(p.axis_handle);
        gap_from_wall_row = gap_from_wall_row.push(
            button(text(format!("Wand #{}", p.axis_handle.value())).size(10))
                .style(if selected { button::primary } else { button::secondary })
                .padding([3, 6])
                .on_press(Message::Aec(AecMessage::AecJunctionEditorGapFromWallChanged(Some(p.axis_handle)))),
        );
    }
    let mut gap_from_row = row![].spacing(4);
    if let Some(h) = gap_from_wall {
        if let Some(p) = participants.iter().find(|p| p.axis_handle == h) {
            for (idx, l) in p.layers.iter().enumerate() {
                let selected = gap_from == Some((idx, l.material_id.as_str()));
                gap_from_row = gap_from_row.push(
                    button(text(format!("#{} {}", idx + 1, material_name(library, &l.material_id))).size(10))
                        .style(if selected { button::primary } else { button::secondary })
                        .padding([3, 6])
                        .on_press(Message::Aec(AecMessage::AecJunctionEditorGapFromChanged(idx, l.material_id.clone()))),
                );
            }
        }
    }

    let mut gap_to_wall_row = row![].spacing(4);
    for p in &other_walls {
        let selected = gap_to_wall == Some(p.axis_handle);
        gap_to_wall_row = gap_to_wall_row.push(
            button(text(format!("Wand #{}", p.axis_handle.value())).size(10))
                .style(if selected { button::primary } else { button::secondary })
                .padding([3, 6])
                .on_press(Message::Aec(AecMessage::AecJunctionEditorGapToWallChanged(Some(p.axis_handle)))),
        );
    }
    let mut gap_to_row = row![].spacing(4);
    if let Some(h) = gap_to_wall {
        if let Some(p) = participants.iter().find(|p| p.axis_handle == h) {
            for (idx, l) in p.layers.iter().enumerate() {
                let selected = gap_to == Some((idx, l.material_id.as_str()));
                gap_to_row = gap_to_row.push(
                    button(text(format!("#{} {}", idx + 1, material_name(library, &l.material_id))).size(10))
                        .style(if selected { button::primary } else { button::secondary })
                        .padding([3, 6])
                        .on_press(Message::Aec(AecMessage::AecJunctionEditorGapToChanged(idx, l.material_id.clone()))),
                );
            }
        }
    }

    let gap_form = column![
        text(t!("Unterbrechung hinzufügen")).size(11),
        text(t!("Zu unterbrechende Schicht (durchlaufende Wand)")).size(9),
        gap_layer_row,
        text(t!("Von angrenzender Schicht (Stammwand)")).size(9),
        gap_from_wall_row,
        gap_from_row,
        text(t!("Bis angrenzender Schicht")).size(9),
        gap_to_wall_row,
        gap_to_row,
        button(text(t!("Unterbrechung hinzufügen")).size(11))
            .style(button::primary)
            .padding([4, 10])
            .on_press(Message::Aec(AecMessage::AecJunctionEditorAddGap)),
    ]
    .spacing(4);

    let add_form = column![
        text(t!("Neues Layer-Paar hinzufügen")).size(11),
        text(t!("Schicht A (diese Wand)")).size(9),
        layer_a_row,
        text(t!("Andere Wand")).size(9),
        wall_b_row,
        text(t!("Schicht B")).size(9),
        layer_b_row,
        text(t!("Stil")).size(9),
        style_buttons(Some(&pair_style), |s| Message::Aec(AecMessage::AecJunctionEditorPairStyleChanged(s))),
        button(text(t!("Paar hinzufügen")).size(11))
            .style(button::primary)
            .padding([4, 10])
            .on_press(Message::Aec(AecMessage::AecJunctionEditorAddPair)),
    ]
    .spacing(4);

    let actions = row![
        button(text(t!("Speichern")).size(11))
            .style(button::primary)
            .padding([5, 12])
            .on_press(Message::Aec(AecMessage::AecJunctionEditorSave)),
        button(text(t!("Gesamten Override zurücksetzen")).size(11))
            .style(button::danger)
            .padding([5, 12])
            .on_press(Message::Aec(AecMessage::AecJunctionEditorFullReset)),
        button(text(t!("Abbrechen")).size(11))
            .padding([5, 12])
            .on_press(Message::Aec(AecMessage::AecJunctionEditorClose)),
    ]
    .spacing(8);

    let detail = column![
        text(t!("Junction-Editor")).size(13),
        default_style_row,
        pairs_col,
        add_form,
        gaps_col,
        gap_form,
        actions,
    ]
    .spacing(10);

    row![
        scrollable(wall_list).width(220),
        container(scrollable(detail)).padding(10).width(Fill),
    ]
    .spacing(10)
    .padding(10)
    .into()
}
