//! AEC DisplayConfig Manager — browse and edit the `DisplayConfig` entries
//! held in the [`crate::modules::aec::engine::library::DisplayConfigLibrary`]
//! (`AEC_PLANMANAGER`).
//!
//! Step 5 slims this manager down to stem/master-data
//! (name/discipline/scale/phase/view_type) plus the two-stage
//! Phasenfilter-Editor (`PhaseFilter`): step 1 is a checkbox per
//! [`PlanPhase`] controlling `PhaseFilter::visible_phases`, step 2 is a pair
//! of inline style-override forms for `demolition_style`/`existing_style`.
//! The old per-slot visibility/style-override table, the Layer-Filter-UI,
//! and the Style-Substitutions-UI are gone: those overrides now live
//! style-centered on `WallStyle::display_profiles`, edited in the Wandstil-
//! Manager instead (see `aec_wall_style_manager.rs`).

use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_input, Space};
use iced::{Element, Fill};

use crate::app::Message;
use crate::modules::aec::engine::display_component::ComponentStyleOverride;
use crate::modules::aec::engine::library::DisplayConfigLibrary;
use crate::modules::aec::engine::plan_view::{DisplayConfig, PlanPhase, ViewType};
use crate::t;
use super::aec_ui_util::*;

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
    /// Two-stage Phasenfilter-Editor, Stage 1: which phases are currently
    /// checked as "sichtbar" (`PhaseFilter::visible_phases`).
    pub phase_filter_visible_existing: bool,
    pub phase_filter_visible_demolition: bool,
    pub phase_filter_visible_new: bool,
    /// Stage 2: the inline style-override form for `PhaseFilter::demolition_style`.
    pub demolition_style: StyleEditorFormState<'a>,
    /// Stage 2: the inline style-override form for `PhaseFilter::existing_style`.
    pub existing_style: StyleEditorFormState<'a>,
    /// Step 7 Mapping Table UI: scale-name text field buffer.
    pub new_mapping_scale: &'a str,
    /// Step 7 Mapping Table UI: target-config pick-list buffer.
    pub new_mapping_config: Option<&'a str>,
}

/// Two-stage Phasenfilter-Editor, Stage 2: edit-buffer fields for a single
/// phase's `ComponentStyleOverride` overlay (`demolition_style`/`existing_style`).
pub struct StyleEditorFormState<'a> {
    pub line_type: &'a str,
    /// All available line types in the document (name + ASCII art), used
    /// by the same `combo_box`+preview widget as the Layer Manager.
    pub linetype_items: &'a [crate::ui::properties::LinetypeItem],
    /// `combo_box` state built from `linetype_items`.
    pub linetype_combo: &'a iced::widget::combo_box::State<crate::ui::properties::LinetypeItem>,
    /// Hex color text, e.g. `"FF0000"` (without leading `#`).
    pub line_color: &'a str,
    pub line_color_picker_open: bool,
    pub hatch_pattern: &'a str,
    pub hatch_color: &'a str,
    pub hatch_color_picker_open: bool,
    pub fill_color: &'a str,
    pub fill_color_picker_open: bool,
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

    let phase_filter_section = phase_filter_section_view(
        form.phase_filter_visible_existing,
        form.phase_filter_visible_demolition,
        form.phase_filter_visible_new,
        form.demolition_style,
        form.existing_style,
    );
    let overrides_hint = container(
        text(t!(
            "Hinweis: Darstellungs-Overrides (Sichtbarkeit, Stil-Override, Schicht-Filter je Slot) werden nicht mehr hier, sondern im Wandstil-Manager je Stil und Planart gepflegt (WallStyle.display_profiles)."
        ))
        .size(10)
        .style(muted),
    )
    .padding(6)
    .style(container::bordered_box);

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
        phase_filter_section,
        overrides_hint,
        Space::new(),
        actions,
    ]
    .spacing(7)
    .into()
}

/// A short, human-readable summary of a `ComponentStyleOverride` (or "–" /
/// "Standard" if none is set). No longer used by any view function since
/// Step 5 removed the per-slot override table, but kept (with its unit
/// tests) as a small, generically useful formatting helper.
#[allow(dead_code)]
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

/// Two-stage Phasenfilter-Editor section: Stage 1 renders one visibility
/// checkbox per [`PlanPhase`] (`PhaseFilter::visible_phases`); Stage 2
/// renders two independent style-override forms — one for
/// `demolition_style` ("Abbruch"), one for `existing_style` ("Bestand") —
/// each built with [`style_editor_view`].
fn phase_filter_section_view<'a>(
    visible_existing: bool,
    visible_demolition: bool,
    visible_new: bool,
    demolition_style: StyleEditorFormState<'a>,
    existing_style: StyleEditorFormState<'a>,
) -> Element<'a, Message> {
    let stage1 = column![
        text(t!("Schritt 1: Sichtbare Phasen")).size(11),
        row![
            iced::widget::checkbox(visible_new)
                .label(t!("Neubau").into_owned())
                .on_toggle(|checked| Message::AecPlanManagerPhaseVisibleToggle(PlanPhase::New, checked))
                .size(13)
                .text_size(11),
            iced::widget::checkbox(visible_demolition)
                .label(t!("Abbruch").into_owned())
                .on_toggle(|checked| {
                    Message::AecPlanManagerPhaseVisibleToggle(PlanPhase::Demolition, checked)
                })
                .size(13)
                .text_size(11),
            iced::widget::checkbox(visible_existing)
                .label(t!("Bestand").into_owned())
                .on_toggle(|checked| {
                    Message::AecPlanManagerPhaseVisibleToggle(PlanPhase::Existing, checked)
                })
                .size(13)
                .text_size(11),
        ]
        .spacing(16),
    ]
    .spacing(6);

    container(
        column![
            stage1,
            Space::new().height(6),
            text(t!("Schritt 2: Zusatzstile je Phase")).size(11),
            row![
                phase_style_form_demolition(demolition_style),
                phase_style_form_existing(existing_style),
            ]
            .spacing(12),
        ]
        .spacing(6),
    )
    .padding(8)
    .style(container::bordered_box)
    .into()
}

/// Stage 2 form for `PhaseFilter::demolition_style`.
fn phase_style_form_demolition<'a>(editor: StyleEditorFormState<'a>) -> Element<'a, Message> {
    style_editor_form(
        t!("Abbruch-Darstellung").into_owned(),
        editor,
        Message::AecPlanManagerDemolitionStyleLineTypeChanged,
        Message::AecPlanManagerDemolitionStyleLineColorChanged,
        Message::AecPlanManagerDemolitionStyleLineColorPickerToggle,
        crate::app::ColorPickTarget::AecPlanDemolitionLineColor,
        Message::AecPlanManagerDemolitionStyleHatchPatternChanged,
        Message::AecPlanManagerDemolitionStyleHatchColorChanged,
        Message::AecPlanManagerDemolitionStyleHatchColorPickerToggle,
        crate::app::ColorPickTarget::AecPlanDemolitionHatchColor,
        Message::AecPlanManagerDemolitionStyleFillColorChanged,
        Message::AecPlanManagerDemolitionStyleFillColorPickerToggle,
        crate::app::ColorPickTarget::AecPlanDemolitionFillColor,
    )
}

/// Stage 2 form for `PhaseFilter::existing_style`.
fn phase_style_form_existing<'a>(editor: StyleEditorFormState<'a>) -> Element<'a, Message> {
    style_editor_form(
        t!("Bestand-Darstellung").into_owned(),
        editor,
        Message::AecPlanManagerExistingStyleLineTypeChanged,
        Message::AecPlanManagerExistingStyleLineColorChanged,
        Message::AecPlanManagerExistingStyleLineColorPickerToggle,
        crate::app::ColorPickTarget::AecPlanExistingLineColor,
        Message::AecPlanManagerExistingStyleHatchPatternChanged,
        Message::AecPlanManagerExistingStyleHatchColorChanged,
        Message::AecPlanManagerExistingStyleHatchColorPickerToggle,
        crate::app::ColorPickTarget::AecPlanExistingHatchColor,
        Message::AecPlanManagerExistingStyleFillColorChanged,
        Message::AecPlanManagerExistingStyleFillColorPickerToggle,
        crate::app::ColorPickTarget::AecPlanExistingFillColor,
    )
}

/// Shared inline `ComponentStyleOverride` editor form, used for both the
/// `demolition_style` and `existing_style` Stage 2 sections. Line-type uses
/// the shared Layer-Manager-style `combo_box`+preview widget; every colour
/// field uses the shared `color_selector` swatch+name widget.
fn style_editor_form<'a>(
    title: String,
    editor: StyleEditorFormState<'a>,
    on_line_type: impl Fn(String) -> Message + 'static,
    on_line_color: impl Fn(String) -> Message + 'static,
    on_line_color_toggle: Message,
    line_color_more_target: crate::app::ColorPickTarget,
    on_hatch_pattern: impl Fn(String) -> Message + 'a,
    on_hatch_color: impl Fn(String) -> Message + 'static,
    on_hatch_color_toggle: Message,
    hatch_color_more_target: crate::app::ColorPickTarget,
    on_fill_color: impl Fn(String) -> Message + 'static,
    on_fill_color_toggle: Message,
    fill_color_more_target: crate::app::ColorPickTarget,
) -> Element<'a, Message> {
    let line_acad_color = hex_to_acad_color(editor.line_color);
    let hatch_acad_color = hex_to_acad_color(editor.hatch_color);
    let fill_acad_color = hex_to_acad_color(editor.fill_color);
    container(
        column![
            text(title).size(11),
            row![
                text(t!("Linientyp")).size(10).style(muted).width(90),
                super::aec_ui_util::linetype_field(
                    editor.line_type,
                    editor.linetype_items,
                    editor.linetype_combo,
                    on_line_type,
                ),
            ]
            .spacing(8),
            row![
                text(t!("Linienfarbe")).size(10).style(muted).width(90),
                container(crate::ui::color_select::color_selector(
                    line_acad_color,
                    editor.line_color_picker_open,
                    crate::ui::color_select::ColorExtras::default(),
                    move |c| on_line_color(super::aec_ui_util::acad_color_to_hex(c)),
                    on_line_color_toggle,
                    Message::OpenColorWindow(line_color_more_target, line_acad_color),
                ))
                .width(180),
            ]
            .spacing(8),
            row![
                text(t!("Schraffurmuster")).size(10).style(muted).width(90),
                text_input("z. B. ANSI31", editor.hatch_pattern)
                    .on_input(on_hatch_pattern)
                    .size(11)
                    .padding([4, 6]),
            ]
            .spacing(8),
            row![
                text(t!("Schraffurfarbe")).size(10).style(muted).width(90),
                container(crate::ui::color_select::color_selector(
                    hatch_acad_color,
                    editor.hatch_color_picker_open,
                    crate::ui::color_select::ColorExtras::default(),
                    move |c| on_hatch_color(super::aec_ui_util::acad_color_to_hex(c)),
                    on_hatch_color_toggle,
                    Message::OpenColorWindow(hatch_color_more_target, hatch_acad_color),
                ))
                .width(180),
            ]
            .spacing(8),
            row![
                text(t!("Füllfarbe")).size(10).style(muted).width(90),
                container(crate::ui::color_select::color_selector(
                    fill_acad_color,
                    editor.fill_color_picker_open,
                    crate::ui::color_select::ColorExtras::default(),
                    move |c| on_fill_color(super::aec_ui_util::acad_color_to_hex(c)),
                    on_fill_color_toggle,
                    Message::OpenColorWindow(fill_color_more_target, fill_acad_color),
                ))
                .width(180),
            ]
            .spacing(8),
        ]
        .spacing(6),
    )
    .padding(8)
    .style(container::bordered_box)
    .into()
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
            hatch_angle: None,
            hatch_angle_relative: None,
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
