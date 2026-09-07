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

use crate::app::Message;
use crate::modules::aec::commands::JunctionParticipant;
use crate::modules::aec::engine::join::{JoinOverrideStyle, LayerPairOverride};
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
        JoinOverrideStyle::OuterFace,
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
        style_buttons(default_style.as_ref(), Message::AecJunctionEditorSetDefaultStyle),
        button(text(t!("Zurücksetzen")).size(10))
            .style(button::secondary)
            .padding([3, 8])
            .on_press(Message::AecJunctionEditorResetDefaultStyle),
    ]
    .spacing(6);

    // ── Existing layer-pair overrides ─────────────────────────────────────
    let mut pairs_col = column![text(t!("Layer-Paar-Overrides")).size(11)].spacing(4);
    if pairs.is_empty() {
        pairs_col = pairs_col.push(text(t!("Keine Layer-Paar-Overrides.")).size(10));
    } else {
        for (idx, pair) in pairs.iter().enumerate() {
            let a_layer_num = participants
                .iter()
                .flat_map(|p| p.layers.iter().enumerate())
                .find(|(_, l)| l.material_id == pair.layer_a.material_id)
                .map(|(i, _)| i + 1);
            let a_name = match a_layer_num {
                Some(n) => format!("#{} {}", n, material_name(library, &pair.layer_a.material_id)),
                None => material_name(library, &pair.layer_a.material_id).to_string(),
            };
            let b_label = pair
                .layer_b
                .as_ref()
                .map(|b| {
                    let b_layer_num = participants
                        .iter()
                        .flat_map(|p| p.layers.iter().enumerate())
                        .find(|(_, l)| l.material_id == b.material_id)
                        .map(|(i, _)| i + 1);
                    match b_layer_num {
                        Some(n) => format!("#{} {}", n, material_name(library, &b.material_id)),
                        None => material_name(library, &b.material_id).to_string(),
                    }
                })
                .unwrap_or_else(|| t!("Außenkante").into_owned());
            pairs_col = pairs_col.push(
                row![
                    text(format!("{} ↔ {}: {}", a_name, b_label, style_label(&pair.style))).size(10),
                    iced::widget::Space::new(),
                    button(text(t!("Zurücksetzen")).size(9))
                        .style(button::secondary)
                        .padding([2, 6])
                        .on_press(Message::AecJunctionEditorRemovePair(idx)),
                ]
                .spacing(6),
            );
        }
    }

    // ── "Add pair" form ─────────────────────────────────────────────────
    let current_wall_layers: Vec<&str> = participants
        .iter()
        .find(|p| p.axis_handle == axis_handle && p.end_index == end_index)
        .map(|p| p.layers.iter().map(|l| l.material_id.as_str()).collect())
        .unwrap_or_default();

    let mut layer_a_row = row![].spacing(4);
    for (idx, mat_id) in current_wall_layers.iter().enumerate() {
        let selected = pair_layer_a == Some((idx, *mat_id));
        layer_a_row = layer_a_row.push(
            button(text(format!("#{} {}", idx + 1, material_name(library, mat_id))).size(10))
                .style(if selected { button::primary } else { button::secondary })
                .padding([3, 6])
                .on_press(Message::AecJunctionEditorPairLayerAChanged(idx, mat_id.to_string())),
        );
    }

    let mut wall_b_row = row![].spacing(4);
    wall_b_row = wall_b_row.push(
        button(text(t!("Außenkante / keine")).size(10))
            .style(if pair_wall_b.is_none() { button::primary } else { button::secondary })
            .padding([3, 6])
            .on_press(Message::AecJunctionEditorPairWallBChanged(None)),
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
                .on_press(Message::AecJunctionEditorPairWallBChanged(Some(p.axis_handle))),
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
                        .on_press(Message::AecJunctionEditorPairLayerBChanged(idx, l.material_id.clone())),
                );
            }
        }
    } else {
        layer_b_row = layer_b_row.push(text(t!("(nicht erforderlich)")).size(9));
    }

    let add_form = column![
        text(t!("Neues Layer-Paar hinzufügen")).size(11),
        text(t!("Schicht A (diese Wand)")).size(9),
        layer_a_row,
        text(t!("Andere Wand")).size(9),
        wall_b_row,
        text(t!("Schicht B")).size(9),
        layer_b_row,
        text(t!("Stil")).size(9),
        style_buttons(Some(&pair_style), Message::AecJunctionEditorPairStyleChanged),
        button(text(t!("Paar hinzufügen")).size(11))
            .style(button::primary)
            .padding([4, 10])
            .on_press(Message::AecJunctionEditorAddPair),
    ]
    .spacing(4);

    let actions = row![
        button(text(t!("Speichern")).size(11))
            .style(button::primary)
            .padding([5, 12])
            .on_press(Message::AecJunctionEditorSave),
        button(text(t!("Gesamten Override zurücksetzen")).size(11))
            .style(button::danger)
            .padding([5, 12])
            .on_press(Message::AecJunctionEditorFullReset),
        button(text(t!("Abbrechen")).size(11))
            .padding([5, 12])
            .on_press(Message::AecJunctionEditorClose),
    ]
    .spacing(8);

    let detail = column![
        text(t!("Junction-Editor")).size(13),
        default_style_row,
        pairs_col,
        add_form,
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
