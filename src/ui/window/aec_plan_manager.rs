//! AEC DisplayConfig Manager — browse and edit the `DisplayConfig` entries
//! held in the [`crate::modules::aec::engine::library::DisplayConfigLibrary`]
//! (`AEC_PLANMANAGER`).
//!
//! Step 5 slims this manager down to stem/master-data
//! (name/discipline/scale/phase/view_type) plus the two-stage
//! phase-filter editor (`PhaseFilter`): step 1 is a checkbox per
//! [`PlanPhase`] controlling `PhaseFilter::visible_phases`, step 2 is a pair
//! of inline style-override forms for `demolition_style`/`existing_style`.
//! The old per-slot visibility/style-override table, the Layer-Filter-UI,
//! and the Style-Substitutions-UI are gone: those overrides now live
//! style-centered on `WallStyle::display_profiles`, edited in the wall style
//! manager instead (see `aec_wall_style_manager.rs`).

use std::collections::HashMap;

use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_input, Space};
use iced::{Element, Fill};
use uuid::Uuid;

use crate::app::Message;
use crate::modules::aec::engine::display_component::{
    ComponentStyleOverride, RepresentationMode, StyleDisplayOverlay, WallComponentKind,
};
use crate::modules::aec::engine::library::DisplayConfigLibrary;
use crate::modules::aec::engine::plan_view::{DisplayConfig, PlanPhase, PlanningStage, ViewType};
use crate::t;
use crate::tr;
use super::aec_ui_util::*;

fn planning_stage_label(stage: PlanningStage) -> String {
    match stage {
        PlanningStage::Permit => tr!("aec", "planning-stage-permit"),
        PlanningStage::Design => tr!("aec", "planning-stage-design"),
        PlanningStage::Execution => tr!("aec", "planning-stage-execution"),
    }
}

fn view_type_label(view_type: ViewType) -> String {
    match view_type {
        ViewType::FloorPlan => tr!("aec", "view-type-floor-plan"),
        ViewType::Section => tr!("aec", "view-type-section"),
        ViewType::Elevation => tr!("aec", "view-type-elevation"),
    }
}

const PLANNING_STAGES: [PlanningStage; 3] = [
    PlanningStage::Permit,
    PlanningStage::Design,
    PlanningStage::Execution,
];
const VIEW_TYPES: [ViewType; 3] = [ViewType::FloorPlan, ViewType::Section, ViewType::Elevation];

/// Parses a planning stage label (as produced by [`planning_stage_label`]) back
/// into a [`PlanningStage`], defaulting to `Design` for anything unrecognized.
pub fn planning_stage_from_label(label: &str) -> PlanningStage {
    match label {
        "Genehmigungsplanung" => PlanningStage::Permit,
        "Ausführungsplanung" => PlanningStage::Execution,
        _ => PlanningStage::Design,
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
    pub planning_stage: PlanningStage,
    pub view_type: ViewType,
    /// Two-stage phase-filter editor, Stage 1: which phases are currently
    /// checked as visible (`PhaseFilter::visible_phases`).
    pub phase_filter_visible_existing: bool,
    pub phase_filter_visible_demolition: bool,
    pub phase_filter_visible_new: bool,
    /// Stage 2: the inline style-override form for `PhaseFilter::demolition_style`.
    pub demolition_style: StyleEditorFormState<'a>,
    /// Stage 2: the inline style-override form for `PhaseFilter::existing_style`.
    pub existing_style: StyleEditorFormState<'a>,
    pub default_representation: RepresentationMode,
    pub component_visibility: &'a HashMap<WallComponentKind, bool>,
    /// `(style.id, style.name, layers as (layer_id, label))`.
    pub wall_styles: &'a [(String, String, Vec<(Uuid, String)>)],
    pub style_overlays: &'a HashMap<String, StyleDisplayOverlay>,
    pub overlay_style_id: Option<&'a str>,
    pub overlay_layer_id: Option<Uuid>,
    pub overlay_line_type: &'a str,
    pub overlay_line_color: &'a str,
    pub overlay_hatch_pattern: &'a str,
    pub overlay_hatch_color: &'a str,
    pub overlay_hatch_scale: &'a str,
    pub overlay_hatch_angle: &'a str,
    pub overlay_hatch_angle_relative: Option<bool>,
    pub overlay_fill_color: &'a str,
    pub overlay_linetype_items: &'a [crate::ui::properties::LinetypeItem],
    pub overlay_linetype_combo: &'a iced::widget::combo_box::State<crate::ui::properties::LinetypeItem>,
    pub overlay_line_color_picker_open: bool,
    pub overlay_hatch_picker_open: bool,
    pub overlay_hatch_color_picker_open: bool,
    pub overlay_fill_color_picker_open: bool,
    pub contour_hatch_pattern: &'a str,
    pub contour_hatch_color: &'a str,
    pub contour_hatch_scale: &'a str,
    pub contour_hatch_angle: &'a str,
    pub contour_hatch_angle_relative: Option<bool>,
    pub contour_hatch_picker_open: bool,
    pub contour_hatch_color_picker_open: bool,
}

pub use super::aec_ui_util::{StyleEditorFormState, style_editor_form};

pub fn view_window<'a>(
    library: &'a DisplayConfigLibrary,
    selected_name: Option<&str>,
    filter: &str,
    form: PlanConfigFormState<'a>,
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
        scrollable(master_list),
    ]
    .spacing(8)
    .width(220);

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

    let detail = scrollable(column![detail_content].spacing(10))
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
        planning_stage_label(config.planning_stage),
        view_type_label(config.view_type)
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
            text_input(tr!("aec", "scale-placeholder").as_str(), form.scale)
                .on_input(Message::AecPlanManagerScaleChanged)
                .size(11)
                .padding([4, 6])
                .width(120),
        ]
        .spacing(8),
        row![
            text(t!("Planungsstufe")).size(10).style(muted).width(100),
            pick_list(
                Some(form.planning_stage),
                &PLANNING_STAGES[..],
                |stage: &PlanningStage| planning_stage_label(*stage),
            )
            .on_select(Message::AecPlanManagerPlanningStageChanged)
            .text_size(11)
            .width(160),
        ]
        .spacing(8),
        row![
            text(t!("Ansichtstyp")).size(10).style(muted).width(100),
            pick_list(
                Some(form.view_type),
                &VIEW_TYPES[..],
                |view_type: &ViewType| view_type_label(*view_type),
            )
            .on_select(Message::AecPlanManagerViewTypeChanged)
            .text_size(11)
            .width(160),
        ]
        .spacing(8),
        Space::new().height(6),
        global_display_section(form.default_representation, form.component_visibility),
        Space::new().height(6),
        overlay_section_view(&form),
        Space::new().height(6),
        phase_filter_section,
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
        parts.push(format!(
            "#{}",
            super::aec_ui_util::acad_color_to_editor_string(color)
        ));
    }
    if let Some(pattern) = style.hatch_pattern.as_deref() {
        parts.push(pattern.to_string());
    }
    if let Some(color) = style.hatch_color {
        parts.push(format!(
            "{} #{}",
            tr!("aec", "hatch-prefix"),
            super::aec_ui_util::acad_color_to_editor_string(color)
        ));
    }
    if let Some(color) = style.fill_color {
        parts.push(format!(
            "{} #{}",
            tr!("aec", "fill-prefix"),
            super::aec_ui_util::acad_color_to_editor_string(color)
        ));
    }
    if parts.is_empty() {
        t!("Standard").into_owned()
    } else {
        parts.join(", ")
    }
}

fn kind_label(kind: WallComponentKind) -> String {
    match kind {
        WallComponentKind::Axis => tr!("aec", "kind-axis"),
        WallComponentKind::Layers2D => tr!("aec", "kind-layers-2d"),
        WallComponentKind::LayerHatch2D => tr!("aec", "kind-layer-hatch-2d"),
        WallComponentKind::Contour2D => tr!("aec", "kind-contour-2d"),
        WallComponentKind::ContourHatch2D => tr!("aec", "kind-contour-hatch-2d"),
        WallComponentKind::Layers3D => tr!("aec", "kind-layers-3d"),
        WallComponentKind::SurfaceStyle3D => tr!("aec", "kind-surface-3d"),
    }
}

fn representation_label(mode: RepresentationMode) -> String {
    match mode {
        RepresentationMode::TwoD => "2D".to_string(),
        RepresentationMode::ThreeD => "3D".to_string(),
        RepresentationMode::All => tr!("aec", "representation-all"),
    }
}

const REPRESENTATION_MODES: [RepresentationMode; 3] = [
    RepresentationMode::TwoD,
    RepresentationMode::ThreeD,
    RepresentationMode::All,
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum HatchAngleRelChoice {
    Inherit,
    Relative,
    Absolute,
}

const HATCH_ANGLE_REL_CHOICES: [HatchAngleRelChoice; 3] = [
    HatchAngleRelChoice::Inherit,
    HatchAngleRelChoice::Relative,
    HatchAngleRelChoice::Absolute,
];

fn hatch_angle_rel_choice(value: Option<bool>) -> HatchAngleRelChoice {
    match value {
        None => HatchAngleRelChoice::Inherit,
        Some(true) => HatchAngleRelChoice::Relative,
        Some(false) => HatchAngleRelChoice::Absolute,
    }
}

fn hatch_angle_rel_value(choice: HatchAngleRelChoice) -> Option<bool> {
    match choice {
        HatchAngleRelChoice::Inherit => None,
        HatchAngleRelChoice::Relative => Some(true),
        HatchAngleRelChoice::Absolute => Some(false),
    }
}

fn hatch_angle_relative_label(value: Option<bool>) -> String {
    match hatch_angle_rel_choice(value) {
        HatchAngleRelChoice::Inherit => tr!("aec", "inherit"),
        HatchAngleRelChoice::Relative => tr!("aec", "relative"),
        HatchAngleRelChoice::Absolute => tr!("aec", "absolute"),
    }
}

#[allow(dead_code)]
fn hatch_angle_relative_from_label(label: &str) -> Option<bool> {
    match label {
        "Relativ" => Some(true),
        "Absolut" => Some(false),
        _ => None,
    }
}

#[allow(dead_code)]
fn representation_from_label(label: &str) -> RepresentationMode {
    match label {
        "2D" => RepresentationMode::TwoD,
        "3D" => RepresentationMode::ThreeD,
        _ => RepresentationMode::All,
    }
}

fn global_display_section<'a>(
    mode: RepresentationMode,
    visibility: &'a HashMap<WallComponentKind, bool>,
) -> Element<'a, Message> {
    let mut kinds = column![text(t!("Komponenten sichtbar")).size(11)].spacing(4);
    for kind in WallComponentKind::all() {
        let checked = visibility.get(kind).copied().unwrap_or(true);
        kinds = kinds.push(
            iced::widget::checkbox(checked)
                .label(kind_label(*kind))
                .on_toggle(move |v| Message::AecPlanManagerComponentVisibleToggle(*kind, v))
                .size(13)
                .text_size(11),
        );
    }
    container(
        column![
            text(t!("Globale Darstellung")).size(11),
            row![
                text(t!("Darstellung")).size(10).style(muted).width(100),
                pick_list(
                    Some(mode),
                    &REPRESENTATION_MODES[..],
                    |mode: &RepresentationMode| representation_label(*mode),
                )
                .on_select(Message::AecPlanManagerRepresentationChanged)
                .text_size(11)
                .width(120),
            ]
            .spacing(8),
            kinds,
        ]
        .spacing(6),
    )
    .padding(8)
    .style(container::bordered_box)
    .into()
}

fn overlay_color_row<'a>(
    label: impl iced::widget::text::IntoFragment<'a>,
    buffer: &'a str,
    picker_open: bool,
    on_change: impl Fn(String) -> Message + 'static,
    on_toggle: Message,
    more: crate::app::ColorPickTarget,
) -> Element<'a, Message> {
    let color = editor_string_to_acad_color(buffer).unwrap_or(acadrust::types::Color::ByLayer);
    row![
        text(label).size(10).style(muted).width(110),
        container(crate::ui::color_select::color_selector(
            color,
            picker_open,
            crate::ui::color_select::ColorExtras::default(),
            move |c| on_change(acad_color_to_editor_string(c)),
            on_toggle,
            Message::OpenColorWindow(more, color),
        ))
        .width(180),
    ]
    .spacing(8)
    .into()
}

fn contour_hatch_section_view<'a>(form: &PlanConfigFormState<'a>) -> Element<'a, Message> {
    column![
            text(t!("2D-Gesamtkontur-Schraffur (dieser Wandstil)")).size(11),
            text(t!("Leer = Material der äußeren Schichten.")).size(10).style(muted),
            row![
                text(t!("Schraffurmuster")).size(10).style(muted).width(110),
                hatch_pattern_field(
                    form.contour_hatch_pattern,
                    form.contour_hatch_picker_open,
                    Some(tr!("aec", "inherit")),
                    Message::AecPlanManagerContourHatchPickerToggle,
                    Message::AecPlanManagerContourHatchPatternChanged,
                ),
            ]
            .spacing(8),
            overlay_color_row(
                t!("Schraffurfarbe"),
                form.contour_hatch_color,
                form.contour_hatch_color_picker_open,
                Message::AecPlanManagerContourHatchColorChanged,
                Message::AecPlanManagerContourHatchColorPickerToggle,
                crate::app::ColorPickTarget::AecPlanContourHatchColor,
            ),
            row![
                text(t!("Schraffur-Skalierung")).size(10).style(muted).width(110),
                text_input(t!("leer = erben").as_ref(), form.contour_hatch_scale)
                    .on_input(Message::AecPlanManagerContourHatchScaleChanged)
                    .size(11)
                    .padding([4, 6])
                    .width(120),
            ]
            .spacing(8),
            row![
                text(t!("Schraffurwinkel")).size(10).style(muted).width(110),
                text_input(t!("leer = erben").as_ref(), form.contour_hatch_angle)
                    .on_input(Message::AecPlanManagerContourHatchAngleChanged)
                    .size(11)
                    .padding([4, 6])
                    .width(120),
            ]
            .spacing(8),
            row![
                text(t!("Relativ zur Wand")).size(10).style(muted).width(110),
                pick_list(
                    Some(hatch_angle_rel_choice(form.contour_hatch_angle_relative)),
                    &HATCH_ANGLE_REL_CHOICES[..],
                    |choice: &HatchAngleRelChoice| hatch_angle_relative_label(hatch_angle_rel_value(*choice)),
                )
                .on_select(|choice| {
                    Message::AecPlanManagerContourHatchAngleRelativeChanged(
                        hatch_angle_rel_value(choice),
                    )
                })
                .text_size(11)
                .width(120),
            ]
            .spacing(8),
        ]
        .spacing(6)
        .into()
}

fn overlay_section_view<'a>(form: &PlanConfigFormState<'a>) -> Element<'a, Message> {
    let overlay_style_ids: Vec<String> = form.style_overlays.keys().cloned().collect();
    let mut exception_rows = column![].spacing(2);
    if overlay_style_ids.is_empty() {
        exception_rows = exception_rows.push(
            text(t!("Keine Stil-Ausnahmen. Globalregeln gelten für alle Wände."))
                .size(10)
                .style(muted),
        );
    } else {
        for sid in overlay_style_ids {
            let label = form
                .wall_styles
                .iter()
                .find(|(id, _, _)| id == &sid)
                .map(|(_, name, _)| name.clone())
                .unwrap_or_else(|| sid.clone());
            let selected = form.overlay_style_id == Some(sid.as_str());
            exception_rows = exception_rows.push(
                button(text(label).size(11))
                    .on_press(Message::AecPlanManagerOverlayStyleSelect(sid))
                    .style(list_style(selected))
                    .padding([4, 8])
                    .width(Fill),
            );
        }
    }

    let mut add_row = row![text(t!("Ausnahme hinzufügen")).size(10).style(muted)].spacing(6);
    let mut added_any = false;
    for (id, name, _) in form.wall_styles.iter() {
        if form.style_overlays.contains_key(id) {
            continue;
        }
        added_any = true;
        add_row = add_row.push(
            button(text(name.clone()).size(10))
                .padding([3, 8])
                .on_press(Message::AecPlanManagerOverlayAddStyle(id.clone())),
        );
    }
    if !added_any {
        add_row = row![text(t!("Alle Stile haben bereits eine Ausnahme.")).size(10).style(muted)];
    }

    let layers: Vec<(Uuid, String)> = form
        .overlay_style_id
        .and_then(|sid| {
            form.wall_styles
                .iter()
                .find(|(id, _, _)| id == sid)
                .map(|(_, _, layers)| layers.clone())
        })
        .unwrap_or_default();
    let mut layer_col = column![text(t!("Schicht")).size(11)].spacing(2);
    if form.overlay_style_id.is_none() {
        layer_col = layer_col.push(text(t!("Stil-Ausnahme wählen.")).size(10).style(muted));
    } else if layers.is_empty() {
        layer_col = layer_col.push(text(t!("Dieser Stil hat keine Schichten.")).size(10).style(muted));
    } else {
        for (i, (lid, label)) in layers.into_iter().enumerate() {
            let selected = form.overlay_layer_id == Some(lid);
            let shown = if label
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
            {
                label
            } else {
                format!("{} — {}", i + 1, label)
            };
            layer_col = layer_col.push(
                button(text(shown).size(11))
                    .on_press(Message::AecPlanManagerOverlayLayerSelect(lid))
                    .style(list_style(selected))
                    .padding([4, 8])
                    .width(Fill),
            );
        }
    }

    let vis = form
        .overlay_style_id
        .and_then(|sid| form.style_overlays.get(sid))
        .and_then(|o| form.overlay_layer_id.and_then(|lid| o.layer_visibility.get(&lid).copied()))
        .unwrap_or_default();

    let mut props = column![
        text(t!("Feld-Overrides (leer = erben)")).size(11),
        row![
            text(t!("Linientyp")).size(10).style(muted).width(110),
            linetype_field(
                form.overlay_line_type,
                form.overlay_linetype_items,
                form.overlay_linetype_combo,
                Message::AecPlanManagerOverlayLineTypeChanged,
            ),
        ]
        .spacing(8),
        overlay_color_row(
            t!("Linienfarbe"),
            form.overlay_line_color,
            form.overlay_line_color_picker_open,
            Message::AecPlanManagerOverlayLineColorChanged,
            Message::AecPlanManagerOverlayLineColorPickerToggle,
            crate::app::ColorPickTarget::AecPlanOverlayLineColor,
        ),
        row![
            text(t!("Schraffurmuster")).size(10).style(muted).width(110),
            hatch_pattern_field(
                form.overlay_hatch_pattern,
                form.overlay_hatch_picker_open,
                Some(tr!("aec", "inherit")),
                Message::AecPlanManagerOverlayHatchPickerToggle,
                Message::AecPlanManagerOverlayHatchPatternChanged,
            ),
        ]
        .spacing(8),
        overlay_color_row(
            t!("Schraffurfarbe"),
            form.overlay_hatch_color,
            form.overlay_hatch_color_picker_open,
            Message::AecPlanManagerOverlayHatchColorChanged,
            Message::AecPlanManagerOverlayHatchColorPickerToggle,
            crate::app::ColorPickTarget::AecPlanOverlayHatchColor,
        ),
        row![
            text(t!("Schraffur-Skalierung")).size(10).style(muted).width(110),
            text_input(t!("leer = erben").as_ref(), form.overlay_hatch_scale)
                .on_input(Message::AecPlanManagerOverlayHatchScaleChanged)
                .size(11)
                .padding([4, 6])
                .width(120),
        ]
        .spacing(8),
        row![
            text(t!("Schraffurwinkel")).size(10).style(muted).width(110),
            text_input(t!("leer = erben").as_ref(), form.overlay_hatch_angle)
                .on_input(Message::AecPlanManagerOverlayHatchAngleChanged)
                .size(11)
                .padding([4, 6])
                .width(120),
        ]
        .spacing(8),
        row![
            text(t!("Relativ zur Wand")).size(10).style(muted).width(110),
            pick_list(
                Some(hatch_angle_rel_choice(form.overlay_hatch_angle_relative)),
                &HATCH_ANGLE_REL_CHOICES[..],
                |choice: &HatchAngleRelChoice| hatch_angle_relative_label(hatch_angle_rel_value(*choice)),
            )
            .on_select(|choice| {
                Message::AecPlanManagerOverlayHatchAngleRelativeChanged(
                    hatch_angle_rel_value(choice),
                )
            })
            .text_size(11)
            .width(120),
        ]
        .spacing(8),
        overlay_color_row(
            t!("Füllfarbe"),
            form.overlay_fill_color,
            form.overlay_fill_color_picker_open,
            Message::AecPlanManagerOverlayFillColorChanged,
            Message::AecPlanManagerOverlayFillColorPickerToggle,
            crate::app::ColorPickTarget::AecPlanOverlayFillColor,
        ),
        row![
            iced::widget::checkbox(vis.visible_2d)
                .label(t!("Sichtbar 2D").into_owned())
                .on_toggle(Message::AecPlanManagerOverlayLayerVis2d)
                .size(13)
                .text_size(11),
            iced::widget::checkbox(vis.visible_3d)
                .label(t!("Sichtbar 3D").into_owned())
                .on_toggle(Message::AecPlanManagerOverlayLayerVis3d)
                .size(13)
                .text_size(11),
        ]
        .spacing(12),
    ]
    .spacing(6);

    if form.overlay_layer_id.is_none() {
        props = column![text(t!("Schicht wählen, um Overrides zu setzen.")).size(10).style(muted)];
    }

    let mut body = column![
        text(t!("Stil-Ausnahmen")).size(11),
        add_row,
        exception_rows,
    ]
    .spacing(6);
    if form.overlay_style_id.is_some() {
        body = body.push(
            button(text(t!("Ausnahme entfernen")).size(11))
                .style(button::danger)
                .padding([4, 10])
                .on_press(Message::AecPlanManagerOverlayRemoveStyle),
        );
    }
    if form.overlay_style_id.is_some() {
        body = body
            .push(Space::new().height(4))
            .push(contour_hatch_section_view(form));
    }
    body = body
        .push(Space::new().height(4))
        .push(row![layer_col.width(200), props].spacing(12));

    container(body)
        .padding(8)
        .style(container::bordered_box)
        .into()
}

/// Two-stage phase-filter editor section: Stage 1 renders one visibility
/// checkbox per [`PlanPhase`] (`PhaseFilter::visible_phases`); Stage 2
/// renders two independent style-override forms — one for
/// `demolition_style` (demolition), one for `existing_style` (existing) —
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
            text(t!("Nur 2D-Gesamtkontur (nicht schichtweise)")).size(10),
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
        Message::AecPlanManagerDemolitionStyleHatchPickerToggle,
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
        Message::AecPlanManagerExistingStyleHatchPickerToggle,
        Message::AecPlanManagerExistingStyleHatchColorChanged,
        Message::AecPlanManagerExistingStyleHatchColorPickerToggle,
        crate::app::ColorPickTarget::AecPlanExistingHatchColor,
        Message::AecPlanManagerExistingStyleFillColorChanged,
        Message::AecPlanManagerExistingStyleFillColorPickerToggle,
        crate::app::ColorPickTarget::AecPlanExistingFillColor,
    )
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
            line_color: Some(acadrust::types::Color::Rgb { r: 0, g: 0, b: 0 }),
            hatch_pattern: Some("ANSI31".to_string()),
            hatch_color: Some(acadrust::types::Color::Rgb { r: 255, g: 255, b: 255 }),
            hatch_scale: None,
            fill_color: Some(acadrust::types::Color::Rgb { r: 128, g: 128, b: 128 }),
            hatch_angle: None,
            hatch_angle_relative: None,
            cad_layer: None,
        };
        let summary = style_override_summary(Some(&style));
        let hatch = tr!("aec", "hatch-prefix");
        let fill = tr!("aec", "fill-prefix");
        assert_eq!(
            summary,
            format!("Continuous, #000000, ANSI31, {hatch} #FFFFFF, {fill} #808080")
        );
    }

    #[test]
    fn style_override_summary_partial_override_only_lists_set_fields() {
        let style = ComponentStyleOverride {
            hatch_color: Some(acadrust::types::Color::Rgb { r: 0, g: 255, b: 0 }),
            ..Default::default()
        };
        assert_eq!(
            style_override_summary(Some(&style)),
            format!("{} #00FF00", tr!("aec", "hatch-prefix"))
        );
    }

    #[test]
    fn planning_stage_and_view_type_label_roundtrip() {
        assert_eq!(
            planning_stage_from_label("Genehmigungsplanung"),
            PlanningStage::Permit
        );
        assert_eq!(
            planning_stage_from_label("Entwurfsplanung"),
            PlanningStage::Design
        );
        assert_eq!(
            planning_stage_from_label("Ausführungsplanung"),
            PlanningStage::Execution
        );
        assert_eq!(view_type_from_label("Grundriss"), ViewType::FloorPlan);
        assert_eq!(view_type_from_label("Schnitt"), ViewType::Section);
        assert_eq!(view_type_from_label("Ansicht"), ViewType::Elevation);
    }

    #[test]
    fn planning_stage_and_view_type_from_label_unknown_input_falls_back_to_default() {
        assert_eq!(planning_stage_from_label("unknown"), PlanningStage::Design);
        assert_eq!(view_type_from_label("unknown"), ViewType::FloorPlan);
    }
}
