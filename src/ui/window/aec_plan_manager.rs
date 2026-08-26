//! AEC DisplayConfig Manager — browse and edit the `DisplayConfig` entries
//! held in the [`crate::modules::aec::engine::library::DisplayConfigLibrary`]
//! (`AEC_PLANMANAGER`).
//!
//! Covers the "basic fields + slot visibility + persist" scope of Step 5:
//! name/discipline/scale/phase/view_type plus per-slot visibility toggles
//! for walls, the Step 8 per-slot style-override editor, and the
//! Layer-Filter-UI follow-up (Alle/Auswahl toggle + multi-select layer
//! checklist for `Contour2D`/`Solid3D`). The style-substitutions list is
//! deliberately left for a further follow-up and is not rendered here yet.

use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_input, Space};
use iced::{Element, Fill};

use crate::app::Message;
use crate::modules::aec::engine::display_component::{ComponentStyleOverride, WallComponentSlot};
use crate::modules::aec::engine::join::LayerRef;
use crate::modules::aec::engine::library::{DisplayConfigLibrary, StyleLibrary};
use crate::modules::aec::engine::plan_view::{DisplayConfig, PlanPhase, ViewType};
use crate::t;
use super::aec_ui_util::*;

/// All wall display-component slots, in the order the plan's sketch lists
/// them.
const WALL_SLOTS: [WallComponentSlot; 9] = [
    WallComponentSlot::AxisLine,
    WallComponentSlot::Contour2D,
    WallComponentSlot::ContourHatch2D,
    WallComponentSlot::Layers2D,
    WallComponentSlot::LayerHatch2D,
    WallComponentSlot::Solid3D,
    WallComponentSlot::SurfaceStyle3D,
    WallComponentSlot::SectionRepresentation,
    WallComponentSlot::ElevationRepresentation,
];

/// A human-readable (German, matching the plan's sketches) label for a slot.
fn slot_label(slot: WallComponentSlot) -> &'static str {
    match slot {
        WallComponentSlot::AxisLine => "Achslinie",
        WallComponentSlot::Contour2D => "2D Gesamtkontur",
        WallComponentSlot::ContourHatch2D => "2D Schraffur Gesamtkontur",
        WallComponentSlot::Layers2D => "2D Wandschichten",
        WallComponentSlot::LayerHatch2D => "2D Schraffuren Schichten",
        WallComponentSlot::Solid3D => "3D Gesamtkörper",
        WallComponentSlot::SurfaceStyle3D => "3D Oberflächenstil",
        WallComponentSlot::SectionRepresentation => "Schnitt-Darstellung",
        WallComponentSlot::ElevationRepresentation => "Ansichts-Darstellung",
    }
}

fn phase_label(phase: &PlanPhase) -> &'static str {
    match phase {
        PlanPhase::Existing => "Bestand",
        PlanPhase::Demolition => "Abbruch",
        PlanPhase::New => "Neubau",
    }
}

fn view_type_label(view_type: &ViewType) -> &'static str {
    match view_type {
        ViewType::FloorPlan => "Grundriss",
        ViewType::Section => "Schnitt",
        ViewType::Elevation => "Ansicht",
    }
}

const PHASE_LABELS: [&str; 3] = ["Bestand", "Abbruch", "Neubau"];
const VIEW_TYPE_LABELS: [&str; 3] = ["Grundriss", "Schnitt", "Ansicht"];

/// Parses a phase label (as produced by [`phase_label`]) back into a
/// [`PlanPhase`], defaulting to `New` for anything unrecognized.
pub fn phase_from_label(label: &str) -> PlanPhase {
    match label {
        "Bestand" => PlanPhase::Existing,
        "Abbruch" => PlanPhase::Demolition,
        _ => PlanPhase::New,
    }
}

/// Parses a view-type label (as produced by [`view_type_label`]) back into
/// a [`ViewType`], defaulting to `FloorPlan` for anything unrecognized.
pub fn view_type_from_label(label: &str) -> ViewType {
    match label {
        "Schnitt" => ViewType::Section,
        "Ansicht" => ViewType::Elevation,
        _ => ViewType::FloorPlan,
    }
}

/// Edit-buffer fields for the DisplayConfig form, owned by `App` and
/// borrowed here for rendering.
pub struct PlanConfigFormState<'a> {
    /// Whether the form should be shown at all.
    pub open: bool,
    /// `true` while composing a not-yet-saved config (no name assigned yet
    /// that matches an existing library entry).
    pub is_new: bool,
    /// Name of the config currently being edited, if it already exists in
    /// the library (`None` while composing a new one).
    pub editing_name: Option<&'a str>,
    pub name: &'a str,
    pub discipline: &'a str,
    /// Informative scale buffer (e.g. `"50"` for 1:50); purely informative.
    pub scale: &'a str,
    pub phase: PlanPhase,
    pub view_type: ViewType,
    /// Slot key -> visible?, the edit buffer for the currently selected
    /// element type's `ComponentRuleSet::visibility` map.
    pub slot_visibility: &'a std::collections::HashMap<String, bool>,
    /// Step 8: slot key -> `ComponentStyleOverride`, the edit buffer for the
    /// currently selected element type's `ComponentRuleSet::style_override`
    /// map.
    pub slot_style_override: &'a std::collections::HashMap<String, ComponentStyleOverride>,
    /// Step 8: the per-slot style-override editor, if currently open.
    pub style_editor: Option<StyleEditorFormState<'a>>,
    /// Layer-Filter-UI: the currently resolved style library, used to list
    /// all wall styles' layers for the "Auswahl" multi-select checklist.
    /// `None` when no style library has been resolved yet (checklist is
    /// then simply empty).
    pub style_library: Option<&'a StyleLibrary>,
    /// Layer-Filter-UI: `false` = "Alle", `true` = "Auswahl".
    pub layer_filter_explicit: bool,
    /// Layer-Filter-UI: edit-buffer of explicitly selected layers, only
    /// relevant while `layer_filter_explicit` is `true`.
    pub layer_filter_selection: &'a [LayerRef],
    /// Style-Substitutions-UI: the edit-buffer list of `(source, target)`
    /// wall-style-name rows.
    pub style_substitutions: &'a [(String, String)],
    /// Style-Substitutions-UI: currently selected "Original-Wandstil" in
    /// the add-row form.
    pub new_substitution_source: Option<&'a str>,
    /// Style-Substitutions-UI: currently selected "Ersatz-Wandstil" in the
    /// add-row form.
    pub new_substitution_target: Option<&'a str>,
    pub substitution_error: Option<&'a str>,
    /// Step 7 Mapping Table UI: scale-name text field buffer.
    pub new_mapping_scale: &'a str,
    /// Step 7 Mapping Table UI: target-config pick-list buffer.
    pub new_mapping_config: Option<&'a str>,
}

/// Step 8: edit-buffer fields for the per-slot `ComponentStyleOverride`
/// editor, shown as a small inline panel below the slot table when a slot's
/// "Bearbeiten" button is pressed.
pub struct StyleEditorFormState<'a> {
    /// Slot key currently being edited (see `WallComponentSlot::key`).
    pub slot_key: &'a str,
    pub line_type: &'a str,
    /// Hex color text, e.g. `"FF0000"` (without leading `#`).
    pub line_color: &'a str,
    pub hatch_pattern: &'a str,
    pub hatch_color: &'a str,
    pub fill_color: &'a str,
}

pub fn view_window<'a>(
    library: &'a DisplayConfigLibrary,
    selected_name: Option<&str>,
    filter: &str,
    form: PlanConfigFormState<'a>,
    auto_display_config_from_scale: bool,
) -> Element<'a, Message> {
    let filter_lower = filter.to_lowercase();
    let filtered: Vec<&'a DisplayConfig> = library
        .configs
        .iter()
        .filter(|c| {
            filter.is_empty()
                || c.name.to_lowercase().contains(&filter_lower)
                || c.discipline.to_lowercase().contains(&filter_lower)
        })
        .collect();

    let mut master_list = column![section_title(t!("DisplayConfigs"))].spacing(2);
    if filtered.is_empty() {
        master_list = master_list.push(no_matches());
    } else {
        for cfg in &filtered {
            master_list = master_list.push(config_row(cfg, selected_name == Some(cfg.name.as_str())));
        }
    }

    let auto_scale_checkbox_label = t!("Automatisch an Maßstab koppeln");
    let sidebar = column![
        row![
            text_input(t!("Search configs…").as_ref(), filter)
                .on_input(Message::AecPlanManagerFilter)
                .size(11)
                .padding([4, 6]),
            button(text("+").size(11))
                .on_press(Message::AecPlanManagerNew)
                .padding([4, 8]),
        ]
        .spacing(4),
        // Step 7 ("Auto-Maßstabskopplung an den Zeichnungsmaßstab"): lets
        // the user re-enable automatic scale→config coupling after a
        // manual override (which disables it for the active tab).
        iced::widget::checkbox(auto_display_config_from_scale)
            .label(auto_scale_checkbox_label.into_owned())
            .on_toggle(Message::AecAutoDisplayConfigFromScaleToggled)
            .size(12)
            .text_size(11),
        scrollable(master_list),
    ]
    .spacing(8)
    .width(220);

    let nm_scale = form.new_mapping_scale;
    let nm_config = form.new_mapping_config;

    let detail_content = if form.open {
        config_form_view(form)
    } else {
        container(text(t!("Select a DisplayConfig to edit or create a new one.")).style(muted))
            .width(Fill)
            .height(200)
            .center_x(Fill)
            .center_y(Fill)
            .into()
    };

    let mapping_section = mapping_table_section_view(library, nm_scale, nm_config);

    let detail = scrollable(
        column![detail_content, Space::new().height(24), mapping_section,].spacing(10),
    )
    .width(Fill)
    .height(Fill);

    row![sidebar, container(detail).width(Fill)]
        .spacing(10)
        .padding(10)
        .into()
}

fn config_row<'a>(config: &'a DisplayConfig, selected: bool) -> Element<'a, Message> {
    let subtitle = format!(
        "{} · {} · {}",
        config.discipline,
        phase_label(&config.phase),
        view_type_label(&config.view_type)
    );
    button(
        column![
            text(config.name.as_str()).size(12),
            text(subtitle).size(10).style(muted),
        ]
        .spacing(2),
    )
    .on_press(Message::AecPlanManagerSelect(config.name.clone()))
    .style(list_style(selected))
    .padding([6, 9])
    .width(Fill)
    .into()
}

fn config_form_view<'a>(form: PlanConfigFormState<'a>) -> Element<'a, Message> {
    let form_title = if form.is_new {
        t!("New DisplayConfig")
    } else {
        t!("DisplayConfig")
    };

    let mut actions = row![button(text(t!("Übernehmen")).size(11))
        .style(button::primary)
        .padding([5, 12])
        .on_press(Message::AecPlanManagerApply)]
    .spacing(8);

    if !form.is_new {
        actions = actions.push(
            button(text(t!("Duplizieren")).size(11))
                .padding([5, 12])
                .on_press(Message::AecPlanManagerDuplicate),
        );
        actions = actions.push(
            button(text(t!("Löschen")).size(11))
                .style(button::danger)
                .padding([5, 12])
                .on_press(Message::AecPlanManagerDelete),
        );
    }
    actions = actions.push(
        button(text(t!("Schließen")).size(11))
            .padding([5, 12])
            .on_press(Message::AecPlanManagerClose),
    );

    let slot_rows: Vec<Element<'_, Message>> = WALL_SLOTS
        .iter()
        .map(|&slot| slot_row(slot, form.slot_visibility, form.slot_style_override))
        .collect();
    let style_editor: Element<'_, Message> = match form.style_editor {
        Some(editor) => style_editor_view(editor),
        None => Space::new().into(),
    };
    let layer_filter_section = layer_filter_section_view(
        form.style_library,
        form.layer_filter_explicit,
        form.layer_filter_selection,
    );
    let substitution_section = substitution_section_view(
        form.style_library,
        form.style_substitutions,
        form.new_substitution_source,
        form.new_substitution_target,
        form.substitution_error,
    );

    column![
        text(form_title).size(13),
        row![
            text(t!("Name")).size(10).style(muted).width(100),
            text_input("", form.name)
                .on_input(Message::AecPlanManagerNameChanged)
                .size(11)
                .padding([4, 6]),
        ]
        .spacing(8),
        row![
            text(t!("Disziplin")).size(10).style(muted).width(100),
            text_input("", form.discipline)
                .on_input(Message::AecPlanManagerDisciplineChanged)
                .size(11)
                .padding([4, 6]),
        ]
        .spacing(8),
        row![
            text(t!("Maßstab (info)")).size(10).style(muted).width(100),
            text_input("z. B. 50", form.scale)
                .on_input(Message::AecPlanManagerScaleChanged)
                .size(11)
                .padding([4, 6])
                .width(120),
        ]
        .spacing(8),
        row![
            text(t!("Phase")).size(10).style(muted).width(100),
            pick_list(Some(phase_label(&form.phase)), &PHASE_LABELS[..], |label: &&str| {
                label.to_string()
            })
            .on_select(|label| Message::AecPlanManagerPhaseChanged(phase_from_label(label)))
            .text_size(11)
            .width(160),
        ]
        .spacing(8),
        row![
            text(t!("Ansichtstyp")).size(10).style(muted).width(100),
            pick_list(
                Some(view_type_label(&form.view_type)),
                &VIEW_TYPE_LABELS[..],
                |label: &&str| label.to_string(),
            )
            .on_select(|label| Message::AecPlanManagerViewTypeChanged(view_type_from_label(label)))
            .text_size(11)
            .width(160),
        ]
        .spacing(8),
        Space::new().height(6),
        row![text(t!("Elementtyp")).size(10).style(muted), text("Wand").size(11)].spacing(8),
        slot_header_row(),
        container(scrollable(column(slot_rows).spacing(4)).height(220)).padding(4),
        style_editor,
        layer_filter_section,
        substitution_section,
        Space::new(),
        actions,
    ]
    .spacing(7)
    .into()
}

fn slot_header_row<'a>() -> Element<'a, Message> {
    row![
        text(t!("Slot")).size(10).style(muted).width(220),
        text(t!("Sichtbar")).size(10).style(muted).width(70),
        text(t!("Stil-Override")).size(10).style(muted).width(140),
        text(t!("Aktion")).size(10).style(muted).width(Fill),
    ]
    .spacing(8)
    .into()
}

/// A short, human-readable summary of a slot's `ComponentStyleOverride`
/// (or "–" / "Standard" if none is set), for the slot table's overview
/// column.
fn style_override_summary(style: Option<&ComponentStyleOverride>) -> String {
    let Some(style) = style else {
        return "–".to_string();
    };
    let mut parts = Vec::new();
    if let Some(line_type) = style.line_type.as_deref() {
        parts.push(line_type.to_string());
    }
    if let Some(color) = style.line_color {
        parts.push(format!("#{color:06X}"));
    }
    if let Some(pattern) = style.hatch_pattern.as_deref() {
        parts.push(pattern.to_string());
    }
    if let Some(color) = style.hatch_color {
        parts.push(format!("Hatch #{color:06X}"));
    }
    if let Some(color) = style.fill_color {
        parts.push(format!("Fill #{color:06X}"));
    }
    if parts.is_empty() {
        t!("Standard").into_owned()
    } else {
        parts.join(", ")
    }
}

fn slot_row<'a>(
    slot: WallComponentSlot,
    slot_visibility: &'a std::collections::HashMap<String, bool>,
    slot_style_override: &'a std::collections::HashMap<String, ComponentStyleOverride>,
) -> Element<'a, Message> {
    let key = slot.key();
    let visible = slot_visibility.get(key).copied().unwrap_or(true);
    let style = slot_style_override.get(key);
    let summary = style_override_summary(style);
    let key_for_edit = key.to_string();
    let key_for_reset = key.to_string();

    let mut actions = row![button(text(t!("Bearbeiten")).size(10))
        .padding([3, 8])
        .on_press(Message::AecPlanManagerSlotStyleEdit(key_for_edit))]
    .spacing(4);
    if style.is_some() {
        actions = actions.push(
            button(text(t!("Entfernen")).size(10))
                .style(button::danger)
                .padding([3, 8])
                .on_press(Message::AecPlanManagerSlotStyleReset(key_for_reset)),
        );
    }

    row![
        text(slot_label(slot)).size(11).width(220),
        iced::widget::checkbox(visible)
            .on_toggle(move |_| Message::AecPlanManagerSlotVisibilityToggle(key.to_string()))
            .size(13)
            .width(70),
        text(summary).size(10).style(muted).width(140),
        container(actions).width(Fill),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// Step 8: the small inline editor for a single slot's `ComponentStyleOverride`.
fn style_editor_view<'a>(editor: StyleEditorFormState<'a>) -> Element<'a, Message> {
    let slot_name = WALL_SLOTS
        .iter()
        .find(|s| s.key() == editor.slot_key)
        .map(|&s| slot_label(s))
        .unwrap_or(editor.slot_key);
    container(
        column![
            text(crate::tf!("Stil-Override: {slot_name}")).size(11),
            row![
                text(t!("Linientyp")).size(10).style(muted).width(100),
                text_input("z. B. Continuous", editor.line_type)
                    .on_input(Message::AecPlanManagerSlotStyleLineTypeChanged)
                    .size(11)
                    .padding([4, 6]),
            ]
            .spacing(8),
            row![
                text(t!("Linienfarbe (Hex)")).size(10).style(muted).width(100),
                text_input("RRGGBB", editor.line_color)
                    .on_input(Message::AecPlanManagerSlotStyleLineColorChanged)
                    .size(11)
                    .padding([4, 6])
                    .width(120),
            ]
            .spacing(8),
            row![
                text(t!("Schraffurmuster")).size(10).style(muted).width(100),
                text_input("z. B. ANSI31", editor.hatch_pattern)
                    .on_input(Message::AecPlanManagerSlotStyleHatchPatternChanged)
                    .size(11)
                    .padding([4, 6]),
            ]
            .spacing(8),
            row![
                text(t!("Schraffurfarbe (Hex)")).size(10).style(muted).width(100),
                text_input("RRGGBB", editor.hatch_color)
                    .on_input(Message::AecPlanManagerSlotStyleHatchColorChanged)
                    .size(11)
                    .padding([4, 6])
                    .width(120),
            ]
            .spacing(8),
            row![
                text(t!("Füllfarbe (Hex)")).size(10).style(muted).width(100),
                text_input("RRGGBB", editor.fill_color)
                    .on_input(Message::AecPlanManagerSlotStyleFillColorChanged)
                    .size(11)
                    .padding([4, 6])
                    .width(120),
            ]
            .spacing(8),
            row![
                button(text(t!("Speichern")).size(11))
                    .style(button::primary)
                    .padding([4, 10])
                    .on_press(Message::AecPlanManagerSlotStyleSave),
                button(text(t!("Abbrechen")).size(11))
                    .padding([4, 10])
                    .on_press(Message::AecPlanManagerSlotStyleEditorClose),
            ]
            .spacing(8),
        ]
        .spacing(6),
    )
    .padding(8)
    .style(container::bordered_box)
    .into()
}

/// Layer-Filter-UI: material name for a layer, falling back to the raw
/// material id if the material can no longer be found in the library
/// (mirrors `aec_junction_editor::material_name`).
fn layer_material_name(library: &StyleLibrary, material_id: &str) -> String {
    library
        .materials
        .iter()
        .find(|m| m.id == material_id)
        .map(|m| m.name.clone())
        .unwrap_or_else(|| material_id.to_string())
}

/// Layer-Filter-UI section, mirroring the Junction-Editor's multi-select
/// checklist pattern: an "Alle"/"Auswahl" toggle (`iced::widget::radio` is
/// not used here — this project's other manager windows consistently use a
/// two-button toggle row, e.g. `style_buttons` in `aec_junction_editor.rs`,
/// so the same substitution is used here for consistency), and — only when
/// "Auswahl" is active — a checklist of every layer across every wall style
/// in the resolved `StyleLibrary`, grouped by wall style name.
fn layer_filter_section_view<'a>(
    style_library: Option<&'a StyleLibrary>,
    is_explicit: bool,
    selected: &'a [LayerRef],
) -> Element<'a, Message> {
    let mode_row = row![
        button(text(t!("Alle")).size(11))
            .style(if !is_explicit { button::primary } else { button::secondary })
            .padding([4, 10])
            .on_press(Message::AecPlanManagerLayerFilterModeToggle(false)),
        button(text(t!("Auswahl")).size(11))
            .style(if is_explicit { button::primary } else { button::secondary })
            .padding([4, 10])
            .on_press(Message::AecPlanManagerLayerFilterModeToggle(true)),
    ]
    .spacing(6);

    let mut section = column![
        text(t!("Schicht-Filter (Contour2D/Solid3D)")).size(11),
        mode_row,
    ]
    .spacing(6);

    if is_explicit {
        let mut checklist = column![].spacing(8);
        if let Some(library) = style_library {
            if library.wall_styles.is_empty() {
                checklist = checklist.push(text(t!("Keine Wandstile in der Bibliothek.")).size(10).style(muted));
            }
            for wall_style in &library.wall_styles {
                let mut layer_rows = column![].spacing(2);
                for (idx, layer) in wall_style.layers.iter().enumerate() {
                    let layer_ref = LayerRef {
                        material_id: layer.material_id.clone(),
                        role_tag: layer.role_tag.clone(),
                        index: idx,
                    };
                    let checked = selected.contains(&layer_ref);
                    let label = format!("#{} {}", idx + 1, layer_material_name(library, &layer.material_id));
                    layer_rows = layer_rows.push(
                        iced::widget::checkbox(checked)
                            .label(label)
                            .on_toggle(move |_| {
                                Message::AecPlanManagerLayerFilterLayerToggle(layer_ref.clone())
                            })
                            .size(12)
                            .text_size(10),
                    );
                }
                checklist = checklist.push(
                    container(
                        column![text(wall_style.style.name.as_str()).size(10).style(muted), layer_rows]
                            .spacing(2),
                    )
                    .padding(6)
                    .style(container::bordered_box),
                );
            }
        } else {
            checklist = checklist.push(text(t!("Keine Stilbibliothek geladen.")).size(10).style(muted));
        }
        section = section.push(container(scrollable(checklist).height(160)).padding(4));
    }

    section.into()
}

/// Style-Substitutions-UI: a display label for a wall-style id, falling
/// back to the raw id if it can no longer be found in the library (mirrors
/// `layer_material_name`).
fn wall_style_label(library: &StyleLibrary, style_id: &str) -> String {
    library
        .wall_styles
        .iter()
        .find(|w| w.style.id == style_id)
        .map(|w| w.style.name.clone())
        .unwrap_or_else(|| style_id.to_string())
}

/// Style-Substitutions-UI section ("Wandstil-Substitutionen", the
/// `StyleSubstitution` schnellweg override): a table of existing
/// `(source, target)` rows with a per-row "Entfernen" action, and an
/// add-row form with two `pick_list`s sourced from the resolved
/// `StyleLibrary`'s wall styles plus a "Hinzufügen" button. Validation
/// (`validate_style_substitution`) runs in the `AecPlanManagerSubstitutionAdd`
/// handler; a rejected add shows `substitution_error` here instead of
/// adding the row (Key Decision: validation blocks add).
fn substitution_section_view<'a>(
    style_library: Option<&'a StyleLibrary>,
    substitutions: &'a [(String, String)],
    new_source: Option<&'a str>,
    new_target: Option<&'a str>,
    error: Option<&'a str>,
) -> Element<'a, Message> {
    let mut section = column![text(t!("Wandstil-Substitutionen (StyleSubstitution, Schnellweg)")).size(11)]
        .spacing(6);

    if substitutions.is_empty() {
        section = section.push(text(t!("Keine Substitutionen definiert.")).size(10).style(muted));
    } else {
        let mut rows = column![].spacing(4);
        for (source, target) in substitutions {
            let (source_label, target_label) = match style_library {
                Some(library) => (wall_style_label(library, source), wall_style_label(library, target)),
                None => (source.clone(), target.clone()),
            };
            rows = rows.push(
                row![
                    text(source_label).size(10).width(180),
                    text("→").size(10).style(muted),
                    text(target_label).size(10).width(180),
                    button(text(t!("Entfernen")).size(10))
                        .style(button::danger)
                        .padding([3, 8])
                        .on_press(Message::AecPlanManagerSubstitutionRemove(source.clone())),
                ]
                .spacing(8),
            );
        }
        section = section.push(rows);
    }

    let style_options: Vec<String> = style_library
        .map(|library| library.wall_styles.iter().map(|w| w.style.id.clone()).collect())
        .unwrap_or_default();

    let source_selected = new_source.map(|s| s.to_string());
    let target_selected = new_target.map(|s| s.to_string());
    let display_fn = move |id: &String| -> String {
        match style_library {
            Some(library) => wall_style_label(library, id),
            None => id.clone(),
        }
    };

    let add_row = row![
        pick_list(source_selected, style_options.clone(), display_fn)
            .placeholder(t!("Original-Wandstil").into_owned())
            .on_select(Message::AecPlanManagerSubstitutionSourceChanged)
            .text_size(11)
            .width(200),
        text("→").size(10).style(muted),
        pick_list(target_selected, style_options, display_fn)
            .placeholder(t!("Ersatz-Wandstil").into_owned())
            .on_select(Message::AecPlanManagerSubstitutionTargetChanged)
            .text_size(11)
            .width(200),
        button(text(t!("Hinzufügen")).size(11))
            .style(button::primary)
            .padding([4, 10])
            .on_press(Message::AecPlanManagerSubstitutionAdd),
    ]
    .spacing(8);

    section = section.push(add_row);
    if let Some(error) = error {
        section = section.push(text(error).size(10).style(error_text_style));
    }

    section.into()
}

/// Danger-colored text style, used for the Style-Substitutions-UI's inline
/// validation error (mirrors `invalid_thickness_style`'s use of the theme's
/// danger palette color).
fn error_text_style(theme: &iced::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().danger.base.color),
    }
}

/// Step 7 Mapping Table UI section ("Maßstabskopplung"): a table of existing
/// `(scale_name, display_config_name)` mappings with a per-row "Entfernen"
/// button, plus an add-row form (text input for scale name + pick_list for
/// config name).
fn mapping_table_section_view<'a>(
    library: &'a DisplayConfigLibrary,
    new_scale: &'a str,
    new_config: Option<&'a str>,
) -> Element<'a, Message> {
    let mut section = column![text(t!("Maßstabskopplung (Auto-DisplayConfig)")).size(11)]
        .spacing(6);

    if library.scale_display_config_mappings.is_empty() {
        section = section.push(
            text(t!("Keine Kopplungen definiert."))
                .size(10)
                .style(muted),
        );
    } else {
        let mut rows = column![].spacing(4);
        for mapping in &library.scale_display_config_mappings {
            rows = rows.push(
                row![
                    text(&mapping.scale_name).size(10).width(180),
                    text("→").size(10).style(muted),
                    text(&mapping.display_config_name).size(10).width(180),
                    button(text(t!("Entfernen")).size(10))
                        .style(button::danger)
                        .padding([3, 8])
                        .on_press(Message::AecPlanManagerScaleMappingRemove(
                            mapping.scale_name.clone()
                        )),
                ]
                .spacing(8),
            );
        }
        section = section.push(rows);
    }

    let config_options: Vec<String> = library.configs.iter().map(|c| c.name.clone()).collect();
    let config_selected = new_config.map(|s| s.to_string());

    let add_row = row![
        text_input(t!("Maßstab (z. B. 1:50)").as_ref(), new_scale)
            .on_input(Message::AecPlanManagerScaleMappingNewScaleChanged)
            .size(11)
            .padding([4, 6])
            .width(180),
        text("→").size(10).style(muted),
        pick_list(config_selected, config_options, |name| name.clone())
            .placeholder(t!("Ziel-DisplayConfig").into_owned())
            .on_select(Message::AecPlanManagerScaleMappingNewConfigChanged)
            .text_size(11)
            .width(200),
        button(text(t!("Hinzufügen")).size(11))
            .style(button::primary)
            .padding([4, 10])
            .on_press(Message::AecPlanManagerScaleMappingAdd),
    ]
    .spacing(8);

    section = section.push(add_row);
    section.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_override_summary_none_is_dash() {
        assert_eq!(style_override_summary(None), "–");
    }

    #[test]
    fn style_override_summary_all_default_fields_is_standard() {
        let style = ComponentStyleOverride::default();
        assert_eq!(style_override_summary(Some(&style)), t!("Standard").into_owned());
    }

    #[test]
    fn style_override_summary_lists_set_fields_in_order() {
        let style = ComponentStyleOverride {
            line_type: Some("Continuous".to_string()),
            line_color: Some(0x000000),
            hatch_pattern: Some("ANSI31".to_string()),
            hatch_color: Some(0xFFFFFF),
            fill_color: Some(0x808080),
        };
        let summary = style_override_summary(Some(&style));
        assert_eq!(
            summary,
            "Continuous, #000000, ANSI31, Hatch #FFFFFF, Fill #808080"
        );
    }

    #[test]
    fn style_override_summary_partial_override_only_lists_set_fields() {
        let style = ComponentStyleOverride {
            hatch_color: Some(0x00FF00),
            ..Default::default()
        };
        assert_eq!(style_override_summary(Some(&style)), "Hatch #00FF00");
    }

    #[test]
    fn phase_and_view_type_label_roundtrip() {
        assert_eq!(phase_from_label(phase_label(&PlanPhase::Existing)), PlanPhase::Existing);
        assert_eq!(phase_from_label(phase_label(&PlanPhase::Demolition)), PlanPhase::Demolition);
        assert_eq!(phase_from_label(phase_label(&PlanPhase::New)), PlanPhase::New);
        assert_eq!(
            view_type_from_label(view_type_label(&ViewType::FloorPlan)),
            ViewType::FloorPlan
        );
        assert_eq!(view_type_from_label(view_type_label(&ViewType::Section)), ViewType::Section);
        assert_eq!(
            view_type_from_label(view_type_label(&ViewType::Elevation)),
            ViewType::Elevation
        );
    }

    #[test]
    fn phase_and_view_type_from_label_unknown_input_falls_back_to_default() {
        assert_eq!(phase_from_label("unknown"), PlanPhase::New);
        assert_eq!(view_type_from_label("unknown"), ViewType::FloorPlan);
    }
}
