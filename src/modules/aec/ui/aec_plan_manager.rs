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
//! manager instead (see `style_manager/forms/wall.rs`).

use std::collections::HashMap;

use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_input, Space};
use iced::{Element, Fill};
use uuid::Uuid;

use crate::app::{AecColorPickTarget, AecMessage, Message};
use crate::modules::aec::engine::display_component::{
    ComponentStyleOverride, RepresentationMode, StyleDisplayOverlay, WallComponentKind,
};
use crate::modules::aec::engine::library::DisplayConfigLibrary;
use crate::modules::aec::engine::plan_view::{
    DetailProfile, DetailSwitch, LayerDetail, PlanPhase, PlanningStage, ViewType,
};
use crate::modules::aec::engine::style_common::StyleOrigin;
use super::style_manager::actions::{self, ActionMessages};
use super::style_manager::list::{self, filter_entries, ListEntry, ListSpec};
use super::style_manager::StyleParts;
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
    if label == planning_stage_label(PlanningStage::Permit) {
        PlanningStage::Permit
    } else if label == planning_stage_label(PlanningStage::Execution) {
        PlanningStage::Execution
    } else {
        PlanningStage::Design
    }
}

/// Parses a view-type label (as produced by [`view_type_label`]) back into
/// a [`ViewType`], defaulting to `FloorPlan` for anything unrecognized.
pub fn view_type_from_label(label: &str) -> ViewType {
    if label == view_type_label(ViewType::Section) {
        ViewType::Section
    } else if label == view_type_label(ViewType::Elevation) {
        ViewType::Elevation
    } else {
        ViewType::FloorPlan
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
    pub detail: DetailProfile,
    pub default_representation: RepresentationMode,
    pub component_visibility: &'a HashMap<WallComponentKind, bool>,
    /// `(style.id, style.name, layers as (layer_id, label))`.
    pub wall_styles: &'a [(String, String, Vec<(Uuid, String)>)],
    pub style_overrides: &'a HashMap<String, StyleDisplayOverlay>,
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

/// `AEC_STYLES` node *Planarten*: list of plan kinds, form, action bar.
pub fn parts<'a>(
    library: &'a DisplayConfigLibrary,
    from_project: bool,
    selected_name: Option<&'a str>,
    filter: &'a str,
    form: PlanConfigFormState<'a>,
) -> StyleParts<'a> {
    let origin = if from_project { StyleOrigin::Project } else { StyleOrigin::Default };
    let all: Vec<ListEntry> = library
        .configs
        .iter()
        .map(|c| ListEntry {
            id: c.name.clone(),
            label: c.name.clone(),
            detail: Some(format!(
                "{} · {}",
                c.discipline,
                c.scale.map(|s| format!("1:{s}")).unwrap_or_else(|| "–".into())
            )),
            origin,
            group: None,
            swatch: None,
        })
        .collect();
    let rows = filter_entries(all, filter);
    let list = list::view(
        ListSpec {
            title: t!("Planarten").into_owned(),
            search_hint: t!("Search configs…").into_owned(),
            new_label: t!("+ Neue Planart").into_owned(),
            filter,
            selected: selected_name,
            on_filter: |s| Message::Aec(AecMessage::AecPlanManagerFilter(s)),
            on_select: |n| Message::Aec(AecMessage::AecPlanManagerSelect(n)),
            on_new: Message::Aec(AecMessage::AecPlanManagerNew),
        },
        &rows,
    );
    let open = form.open;
    let is_new = form.is_new;
    let form_el: Element<'a, Message> = if open {
        scrollable(config_form_view(form)).width(Fill).height(Fill).into()
    } else {
        container(text(t!("Select a DisplayConfig to edit or create a new one.")).style(muted))
            .width(Fill)
            .height(200)
            .center_x(Fill)
            .center_y(Fill)
            .into()
    };
    let actions = open.then(|| {
        actions::view(
            ActionMessages {
                save_apply: Message::Aec(AecMessage::AecPlanManagerApply),
                save: Message::Aec(AecMessage::AecPlanManagerSave),
                duplicate: Message::Aec(AecMessage::AecPlanManagerDuplicate),
                delete: Message::Aec(AecMessage::AecPlanManagerDelete),
                to_standard: Message::Aec(AecMessage::AecPlanManagerSave),
                to_project: Message::Aec(AecMessage::AecPlanManagerSave),
            },
            is_new,
            None,
            false,
        )
    });
    StyleParts { list, form: form_el, actions }
}

fn layer_detail_label(d: LayerDetail) -> String {
    match d {
        LayerDetail::All => t!("Alle Schichten").into_owned(),
        LayerDetail::StructuralOnly => t!("Nur Tragschicht").into_owned(),
        LayerDetail::Off => t!("Aus").into_owned(),
    }
}

const LAYER_DETAILS: [LayerDetail; 3] = [LayerDetail::All, LayerDetail::StructuralOnly, LayerDetail::Off];

/// Detail level of the plan kind (`DetailProfile` switches).
fn detail_section<'a>(detail: DetailProfile) -> Element<'a, Message> {
    let toggle = |label: String, value: bool, switch: DetailSwitch| {
        iced::widget::checkbox(value)
            .label(label)
            .on_toggle(move |v| Message::Aec(AecMessage::AecPlanManagerDetailToggle(switch, v)))
            .size(13)
            .text_size(11)
    };
    container(
        column![
            text(t!("Detailstufe")).size(11),
            row![
                text(t!("Schichten")).size(10).style(muted).width(100),
                pick_list(
                    Some(detail.layers),
                    &LAYER_DETAILS[..],
                    |d: &LayerDetail| layer_detail_label(*d),
                )
                .on_select(|v| Message::Aec(AecMessage::AecPlanManagerDetailLayersChanged(v)))
                .text_size(11)
                .width(160),
            ]
            .spacing(8),
            toggle(t!("Schraffuren").into_owned(), detail.hatches, DetailSwitch::Hatches),
            toggle(t!("Beschriftung").into_owned(), detail.annotation, DetailSwitch::Annotation),
            toggle(t!("Öffnungssymbole").into_owned(), detail.opening_symbols, DetailSwitch::OpeningSymbols),
            toggle(t!("Deckenspiegel").into_owned(), detail.ceiling_view, DetailSwitch::CeilingView),
        ]
        .spacing(6),
    )
    .padding(8)
    .style(container::bordered_box)
    .into()
}

fn config_form_view<'a>(form: PlanConfigFormState<'a>) -> Element<'a, Message> {
    let form_title = if form.is_new {
        t!("New DisplayConfig")
    } else {
        t!("DisplayConfig")
    };

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
                .on_input(|v| Message::Aec(AecMessage::AecPlanManagerNameChanged(v)))
                .size(11)
                .padding([4, 6]),
        ]
        .spacing(8),
        row![
            text(t!("Disziplin")).size(10).style(muted).width(100),
            text_input("", form.discipline)
                .on_input(|v| Message::Aec(AecMessage::AecPlanManagerDisciplineChanged(v)))
                .size(11)
                .padding([4, 6]),
        ]
        .spacing(8),
        row![
            text(t!("Maßstab")).size(10).style(muted).width(100),
            text_input(tr!("aec", "scale-placeholder").as_str(), form.scale)
                .on_input(|v| Message::Aec(AecMessage::AecPlanManagerScaleChanged(v)))
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
            .on_select(|v| Message::Aec(AecMessage::AecPlanManagerPlanningStageChanged(v)))
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
            .on_select(|v| Message::Aec(AecMessage::AecPlanManagerViewTypeChanged(v)))
            .text_size(11)
            .width(160),
        ]
        .spacing(8),
        Space::new().height(6),
        detail_section(form.detail),
        Space::new().height(6),
        global_display_section(form.default_representation, form.component_visibility),
        Space::new().height(6),
        overlay_section_view(&form),
        Space::new().height(6),
        phase_filter_section,
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
                .on_toggle(move |v| Message::Aec(AecMessage::AecPlanManagerComponentVisibleToggle(*kind, v)))
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
                .on_select(|v| Message::Aec(AecMessage::AecPlanManagerRepresentationChanged(v)))
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
                    Message::Aec(AecMessage::AecPlanManagerContourHatchPickerToggle),
                    |v| Message::Aec(AecMessage::AecPlanManagerContourHatchPatternChanged(v)),
                ),
            ]
            .spacing(8),
            overlay_color_row(
                t!("Schraffurfarbe"),
                form.contour_hatch_color,
                form.contour_hatch_color_picker_open,
                |v| Message::Aec(AecMessage::AecPlanManagerContourHatchColorChanged(v)),
                Message::Aec(AecMessage::AecPlanManagerContourHatchColorPickerToggle),
                crate::app::ColorPickTarget::Aec(AecColorPickTarget::PlanContourHatchColor),
            ),
            row![
                text(t!("Schraffur-Skalierung")).size(10).style(muted).width(110),
                text_input(t!("leer = erben").as_ref(), form.contour_hatch_scale)
                    .on_input(|v| Message::Aec(AecMessage::AecPlanManagerContourHatchScaleChanged(v)))
                    .size(11)
                    .padding([4, 6])
                    .width(120),
            ]
            .spacing(8),
            row![
                text(t!("Schraffurwinkel")).size(10).style(muted).width(110),
                text_input(t!("leer = erben").as_ref(), form.contour_hatch_angle)
                    .on_input(|v| Message::Aec(AecMessage::AecPlanManagerContourHatchAngleChanged(v)))
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
                    Message::Aec(AecMessage::AecPlanManagerContourHatchAngleRelativeChanged(
                        hatch_angle_rel_value(choice),
                    ))
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
    use crate::modules::aec::ui::style_manager::forms::plan_overrides::{override_list, OverrideChoice};
    let style_label = |sid: &str| -> String {
        form.wall_styles
            .iter()
            .find(|(id, _, _)| id == sid)
            .map(|(_, name, _)| name.clone())
            .unwrap_or_else(|| sid.to_string())
    };
    let mut entries: Vec<(String, String)> = form
        .style_overrides
        .keys()
        .map(|sid| (sid.clone(), style_label(sid)))
        .collect();
    entries.sort_by(|a, b| a.1.cmp(&b.1));
    let choices: Vec<OverrideChoice> = form
        .wall_styles
        .iter()
        .filter(|(id, _, _)| !form.style_overrides.contains_key(id))
        .map(|(id, name, _)| OverrideChoice { id: id.clone(), label: name.clone() })
        .collect();
    let override_section = override_list(
        t!("Stil-Ausnahmen").to_string(),
        choices,
        None,
        entries,
        form.overlay_style_id.map(str::to_string),
        |id| Message::Aec(AecMessage::AecPlanManagerOverlayAddStyle(id)),
        |id| Message::Aec(AecMessage::AecPlanManagerOverlayStyleSelect(id)),
        |id| Message::Aec(AecMessage::AecPlanManagerOverlayRemoveStyle(id)),
    );

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
                    .on_press(Message::Aec(AecMessage::AecPlanManagerOverlayLayerSelect(lid)))
                    .style(list_style(selected))
                    .padding([4, 8])
                    .width(Fill),
            );
        }
    }

    let vis = form
        .overlay_style_id
        .and_then(|sid| form.style_overrides.get(sid))
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
                |v| Message::Aec(AecMessage::AecPlanManagerOverlayLineTypeChanged(v)),
            ),
        ]
        .spacing(8),
        overlay_color_row(
            t!("Linienfarbe"),
            form.overlay_line_color,
            form.overlay_line_color_picker_open,
            |v| Message::Aec(AecMessage::AecPlanManagerOverlayLineColorChanged(v)),
            Message::Aec(AecMessage::AecPlanManagerOverlayLineColorPickerToggle),
            crate::app::ColorPickTarget::Aec(AecColorPickTarget::PlanOverlayLineColor),
        ),
        row![
            text(t!("Schraffurmuster")).size(10).style(muted).width(110),
            hatch_pattern_field(
                form.overlay_hatch_pattern,
                form.overlay_hatch_picker_open,
                Some(tr!("aec", "inherit")),
                Message::Aec(AecMessage::AecPlanManagerOverlayHatchPickerToggle),
                |v| Message::Aec(AecMessage::AecPlanManagerOverlayHatchPatternChanged(v)),
            ),
        ]
        .spacing(8),
        overlay_color_row(
            t!("Schraffurfarbe"),
            form.overlay_hatch_color,
            form.overlay_hatch_color_picker_open,
            |v| Message::Aec(AecMessage::AecPlanManagerOverlayHatchColorChanged(v)),
            Message::Aec(AecMessage::AecPlanManagerOverlayHatchColorPickerToggle),
            crate::app::ColorPickTarget::Aec(AecColorPickTarget::PlanOverlayHatchColor),
        ),
        row![
            text(t!("Schraffur-Skalierung")).size(10).style(muted).width(110),
            text_input(t!("leer = erben").as_ref(), form.overlay_hatch_scale)
                .on_input(|v| Message::Aec(AecMessage::AecPlanManagerOverlayHatchScaleChanged(v)))
                .size(11)
                .padding([4, 6])
                .width(120),
        ]
        .spacing(8),
        row![
            text(t!("Schraffurwinkel")).size(10).style(muted).width(110),
            text_input(t!("leer = erben").as_ref(), form.overlay_hatch_angle)
                .on_input(|v| Message::Aec(AecMessage::AecPlanManagerOverlayHatchAngleChanged(v)))
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
                Message::Aec(AecMessage::AecPlanManagerOverlayHatchAngleRelativeChanged(
                    hatch_angle_rel_value(choice),
                ))
            })
            .text_size(11)
            .width(120),
        ]
        .spacing(8),
        overlay_color_row(
            t!("Füllfarbe"),
            form.overlay_fill_color,
            form.overlay_fill_color_picker_open,
            |v| Message::Aec(AecMessage::AecPlanManagerOverlayFillColorChanged(v)),
            Message::Aec(AecMessage::AecPlanManagerOverlayFillColorPickerToggle),
            crate::app::ColorPickTarget::Aec(AecColorPickTarget::PlanOverlayFillColor),
        ),
        row![
            iced::widget::checkbox(vis.visible_2d)
                .label(t!("Sichtbar 2D").into_owned())
                .on_toggle(|v| Message::Aec(AecMessage::AecPlanManagerOverlayLayerVis2d(v)))
                .size(13)
                .text_size(11),
            iced::widget::checkbox(vis.visible_3d)
                .label(t!("Sichtbar 3D").into_owned())
                .on_toggle(|v| Message::Aec(AecMessage::AecPlanManagerOverlayLayerVis3d(v)))
                .size(13)
                .text_size(11),
        ]
        .spacing(12),
    ]
    .spacing(6);

    if form.overlay_layer_id.is_none() {
        props = column![text(t!("Schicht wählen, um Overrides zu setzen.")).size(10).style(muted)];
    }

    let mut body = column![override_section].spacing(6);
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
                .on_toggle(|checked| Message::Aec(AecMessage::AecPlanManagerPhaseVisibleToggle(PlanPhase::New, checked)))
                .size(13)
                .text_size(11),
            iced::widget::checkbox(visible_demolition)
                .label(t!("Abbruch").into_owned())
                .on_toggle(|checked| {
                    Message::Aec(AecMessage::AecPlanManagerPhaseVisibleToggle(PlanPhase::Demolition, checked))
                })
                .size(13)
                .text_size(11),
            iced::widget::checkbox(visible_existing)
                .label(t!("Bestand").into_owned())
                .on_toggle(|checked| {
                    Message::Aec(AecMessage::AecPlanManagerPhaseVisibleToggle(PlanPhase::Existing, checked))
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
        |v| Message::Aec(AecMessage::AecPlanManagerDemolitionStyleLineTypeChanged(v)),
        |v| Message::Aec(AecMessage::AecPlanManagerDemolitionStyleLineColorChanged(v)),
        Message::Aec(AecMessage::AecPlanManagerDemolitionStyleLineColorPickerToggle),
        crate::app::ColorPickTarget::Aec(AecColorPickTarget::PlanDemolitionLineColor),
        |v| Message::Aec(AecMessage::AecPlanManagerDemolitionStyleHatchPatternChanged(v)),
        Message::Aec(AecMessage::AecPlanManagerDemolitionStyleHatchPickerToggle),
        |v| Message::Aec(AecMessage::AecPlanManagerDemolitionStyleHatchColorChanged(v)),
        Message::Aec(AecMessage::AecPlanManagerDemolitionStyleHatchColorPickerToggle),
        crate::app::ColorPickTarget::Aec(AecColorPickTarget::PlanDemolitionHatchColor),
        |v| Message::Aec(AecMessage::AecPlanManagerDemolitionStyleFillColorChanged(v)),
        Message::Aec(AecMessage::AecPlanManagerDemolitionStyleFillColorPickerToggle),
        crate::app::ColorPickTarget::Aec(AecColorPickTarget::PlanDemolitionFillColor),
    )
}

/// Stage 2 form for `PhaseFilter::existing_style`.
fn phase_style_form_existing<'a>(editor: StyleEditorFormState<'a>) -> Element<'a, Message> {
    style_editor_form(
        t!("Bestand-Darstellung").into_owned(),
        editor,
        |v| Message::Aec(AecMessage::AecPlanManagerExistingStyleLineTypeChanged(v)),
        |v| Message::Aec(AecMessage::AecPlanManagerExistingStyleLineColorChanged(v)),
        Message::Aec(AecMessage::AecPlanManagerExistingStyleLineColorPickerToggle),
        crate::app::ColorPickTarget::Aec(AecColorPickTarget::PlanExistingLineColor),
        |v| Message::Aec(AecMessage::AecPlanManagerExistingStyleHatchPatternChanged(v)),
        Message::Aec(AecMessage::AecPlanManagerExistingStyleHatchPickerToggle),
        |v| Message::Aec(AecMessage::AecPlanManagerExistingStyleHatchColorChanged(v)),
        Message::Aec(AecMessage::AecPlanManagerExistingStyleHatchColorPickerToggle),
        crate::app::ColorPickTarget::Aec(AecColorPickTarget::PlanExistingHatchColor),
        |v| Message::Aec(AecMessage::AecPlanManagerExistingStyleFillColorChanged(v)),
        Message::Aec(AecMessage::AecPlanManagerExistingStyleFillColorPickerToggle),
        crate::app::ColorPickTarget::Aec(AecColorPickTarget::PlanExistingFillColor),
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
        for stage in PLANNING_STAGES {
            assert_eq!(planning_stage_from_label(&planning_stage_label(stage)), stage);
        }
        for view_type in VIEW_TYPES {
            assert_eq!(view_type_from_label(&view_type_label(view_type)), view_type);
        }
    }

    #[test]
    fn planning_stage_and_view_type_from_label_unknown_input_falls_back_to_default() {
        assert_eq!(planning_stage_from_label("unknown"), PlanningStage::Design);
        assert_eq!(view_type_from_label("unknown"), ViewType::FloorPlan);
    }
}
