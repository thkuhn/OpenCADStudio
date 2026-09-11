//! AEC Wall Style Manager — browse and edit wall styles held in the AEC
//! style library (`AEC_STYLEMANAGER`).

use iced::widget::{
    button, column, container, mouse_area, pick_list, row, scrollable, text, text_input,
    Space,
};
use iced::{Background, Border, Element, Fill, Theme};

use crate::app::Message;
use crate::modules::aec::engine::display_component::WallComponentSlot;
use crate::modules::aec::engine::join::LayerRef;
use crate::modules::aec::engine::library::{LibrarySource, StyleLibrary};
use crate::modules::aec::engine::project::ProjectFile;
use crate::modules::aec::engine::wall_style::WallStyle;
use crate::t;
use crate::tr;
use super::aec_ui_util::*;

const LAYER_COL_REORDER_W: f32 = 74.0;
const LAYER_COL_MATERIAL_W: f32 = 180.0;
const LAYER_COL_THICKNESS_W: f32 = 60.0;
const LAYER_COL_GAP_W: f32 = 50.0;
const LAYER_COL_OFFSET_W: f32 = 50.0;
const LAYER_COL_FUNCTION_W: f32 = 120.0;
const LAYER_COL_OVERRIDE_W: f32 = 130.0;
const LAYER_COL_ROLE_W: f32 = 110.0;
const LAYER_COL_HATCH_W: f32 = 90.0;

pub struct WallStyleFormState<'a> {
    pub open: bool,
    pub is_new: bool,
    pub name: &'a str,
    pub parent_id: Option<&'a str>,
    pub layers: &'a [crate::app::AecLayerBuffer],
    pub drag_index: Option<usize>,
    pub all_wall_styles: Vec<(&'a str, &'a str)>,
    pub all_materials: Vec<(&'a str, &'a str)>,
    pub all_layer_names: Vec<String>,
    pub effective_layers: Vec<crate::modules::aec::engine::wall_style::Layer>,
    /// Full ancestor chain (root-first) of display names for the style
    /// being edited, based on the currently selected parent. Empty when the
    /// style has no parent (or none is selected yet).
    pub inheritance_chain: Vec<String>,
    /// Step 4: "Darstellungs-Profile" (per-`DisplayConfig` overrides) edit
    /// state, `None` while composing a brand-new (unsaved) style, since
    /// `WallStyle::display_profiles` only makes sense for a style that
    /// already has a stable id.
    pub display_profiles: Option<DisplayProfileFormState<'a>>,
}

/// Edit-buffer state for the "Darstellungs-Profile" section of the wall
/// style form: a table of every known `DisplayConfig` (Standard/Override
/// status) plus, when one is selected, its `ComponentRuleSet` detail form
/// — two independent per-slot layer-filter checklists (`Contour2D`/
/// `Solid3D`, mirroring `layer_filter_to_ui_state`) and a hatch-angle
/// override ("Relativ zur Wand" + Winkel).
#[derive(Clone)]
pub struct DisplayProfileFormState<'a> {
    /// Every `DisplayConfig` name known to the resolved `DisplayConfigLibrary`.
    pub display_config_names: Vec<String>,
    /// Names of `DisplayConfig`s for which the style being edited already
    /// has an entry in `display_profiles` ("Override" vs "Standard").
    pub existing_overrides: std::collections::HashSet<String>,
    /// The `DisplayConfig` name currently selected in the table, if any.
    pub selected: Option<&'a str>,
    /// Layer-Filter-UI: `false` = "Alle Schichten", `true` = "Auswahl", for
    /// the `Contour2D` slot of the selected profile.
    pub contour_explicit: bool,
    /// Layer-Filter-UI: explicitly selected layers for `Contour2D`.
    pub contour_selected: &'a [LayerRef],
    /// Same as `contour_explicit`, but for the `Solid3D` slot.
    pub solid_explicit: bool,
    /// Same as `contour_selected`, but for the `Solid3D` slot.
    pub solid_selected: &'a [LayerRef],
    /// Hatch-angle override text field (degrees; empty = no override).
    pub hatch_angle: &'a str,
    /// Whether `hatch_angle` is relative to the wall's own run direction.
    pub hatch_relative: bool,
    /// Visibility per slot (Step 4).
    pub slot_visibility: std::collections::HashMap<WallComponentSlot, bool>,
    /// Slot currently open in the inline style-override editor.
    pub editing_slot: Option<WallComponentSlot>,
    /// Pending per-slot style overrides for the selected profile.
    pub slot_overrides: &'a std::collections::HashMap<
        WallComponentSlot,
        crate::modules::aec::engine::display_component::ComponentStyleOverride,
    >,
    /// Shared style-editor buffer state for the open slot (if any).
    pub slot_style_editor: super::aec_ui_util::StyleEditorFormState<'a>,
}


/// Standalone modal content for plan-type display-profile editing
/// (`ModalKind::AecWallStyleDisplayProfiles`).
pub fn view_display_profiles_window<'a>(
    profiles: DisplayProfileFormState<'a>,
    layers: &'a [crate::app::AecLayerBuffer],
) -> Element<'a, Message> {
    column![
        display_profiles_section(&profiles, layers),
        row![
            Space::new(),
            button(text(t!("Schließen")).size(11))
                .padding([5, 12])
                .on_press(Message::AecWallStyleManagerDisplayProfilesClose),
        ]
        .spacing(8),
    ]
    .spacing(10)
    .padding(10)
    .into()
}

pub fn view_window<'a>(
    library: &'a StyleLibrary,
    project: Option<&'a ProjectFile>,
    session: Option<&'a StyleLibrary>,
    selected_id: Option<&str>,
    filter: &str,
    wall_style_form: WallStyleFormState<'a>,
) -> Element<'a, Message> {
    let entries = crate::modules::aec::engine::library::combined_wall_style_entries_with_session(
        project, session,
    );
    let standard_ids: std::collections::HashSet<String> =
        crate::modules::aec::engine::library::load_or_seed()
            .wall_styles
            .into_iter()
            .map(|w| w.style.id)
            .collect();
    let source_by_id: std::collections::HashMap<String, LibrarySource> = entries
        .iter()
        .map(|e| (e.wall_style.style.id.clone(), e.source))
        .collect();

    // Build a merged library for the inheritance tree (project wins on id).
    let mut merged = StyleLibrary::empty();
    for e in &entries {
        merged.upsert_wall_style(e.wall_style.clone());
    }
    // Keep materials from the caller's library for form lookups.
    for m in &library.materials {
        merged.upsert_material(m.clone());
    }

    let filter_lower = filter.to_lowercase();
    // Materialize owned (style, depth, source) rows so the Element does not
    // borrow the temporary merged library.
    let owned_rows: Vec<(WallStyle, usize, LibrarySource)> = merged
        .wall_style_tree()
        .into_iter()
        .filter(|node| {
            filter.is_empty()
                || node.style.style.name.to_lowercase().contains(&filter_lower)
                || node.style.style.id.to_lowercase().contains(&filter_lower)
        })
        .map(|node| {
            let source = source_by_id
                .get(&node.style.style.id)
                .copied()
                .unwrap_or(LibrarySource::Standard);
            (node.style.clone(), node.depth, source)
        })
        .collect();

    let wall_style_rows: Vec<Element<'_, Message>> = owned_rows
        .iter()
        .map(|(style, depth, source)| {
            wall_style_tree_row(
                style,
                *depth,
                *source,
                selected_id == Some(style.style.id.as_str()),
            )
        })
        .collect();

    let mut master_list = column![section_title(t!("Wall Styles"))].spacing(2);
    if wall_style_rows.is_empty() {
        master_list = master_list.push(no_matches());
    } else {
        for row_el in wall_style_rows {
            master_list = master_list.push(row_el);
        }
    }

    let sidebar = column![
        row![
            text_input(t!("Search wall styles…").as_ref(), filter)
                .on_input(Message::AecStyleManagerFilter)
                .size(11)
                .padding([4, 6]),
            button(text("+").size(11))
                .on_press(Message::AecStyleManagerWallStyleNew)
                .padding([4, 8]),
        ]
        .spacing(4),
        scrollable(master_list),
    ]
    .spacing(8)
    .width(270);

    let selected_source = selected_id.and_then(|id| source_by_id.get(id).copied());
    let selected_has_standard_counterpart = selected_id
        .map(|id| standard_ids.contains(id))
        .unwrap_or(false);

    let detail = if wall_style_form.open {
        wall_style_form_view(
            wall_style_form,
            selected_source,
            selected_has_standard_counterpart,
        )
    } else {
        container(text(t!("Select a wall style to edit or create a new one.")).style(muted))
            .width(Fill)
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill)
            .into()
    };

    row![sidebar, container(detail).padding(10).width(Fill)]
        .spacing(10)
        .padding(10)
        .into()
}

fn source_badge<'a>(source: LibrarySource) -> Element<'a, Message> {
    let label = match source {
        LibrarySource::Standard => t!("Standard"),
        LibrarySource::Project => t!("Projekt"),
        LibrarySource::Session => t!("Sitzung"),
    };
    container(text(label).size(9))
        .padding([1, 5])
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.strong.color.scale_alpha(0.55),
            )),
            text_color: Some(theme.palette().background.base.text.scale_alpha(0.85)),
            border: Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

fn wall_style_tree_row<'a>(
    wall_style: &WallStyle,
    depth: usize,
    source: LibrarySource,
    selected: bool,
) -> Element<'a, Message> {
    let subtitle = tr!("aec", "layer-count", count = wall_style.layers.len());
    let mut row_content = row![].spacing(4).align_y(iced::Center);
    if depth > 0 {
        row_content = row_content.push(Space::new().width(12.0 * depth as f32));
        row_content = row_content.push(text("└").size(10).style(muted));
    }

    button(
        row_content.push(
            column![
                row![
                    text(wall_style.style.name.clone()).size(12),
                    Space::new().width(6),
                    source_badge(source),
                ]
                .spacing(4)
                .align_y(iced::Center),
                text(subtitle).size(10).style(muted),
            ]
            .spacing(2),
        ),
    )
    .on_press(Message::AecStyleManagerSelectWallStyle(
        wall_style.style.id.clone(),
    ))
    .style(list_style(selected))
    .padding([6, 9])
    .width(Fill)
    .into()
}

fn wall_style_form_view<'a>(
    wall_style_form: WallStyleFormState<'a>,
    selected_source: Option<LibrarySource>,
    has_standard_counterpart: bool,
) -> Element<'a, Message> {
    let form_title = if wall_style_form.is_new {
        t!("New Wall Style")
    } else {
        t!("Wall Style")
    };

    let none_label = t!("(None)");
    let selected_parent = wall_style_form
        .parent_id
        .and_then(|pid| {
            wall_style_form
                .all_wall_styles
                .iter()
                .find(|(id, _)| id == &pid)
                .map(|(_, name)| name.to_string())
        })
        .unwrap_or_else(|| none_label.into_owned());

    let all_materials_ref = &wall_style_form.all_materials;
    let all_layer_names_ref = &wall_style_form.all_layer_names;
    let layer_count = wall_style_form.layers.len();
    let drag_index = wall_style_form.drag_index;
    let layer_rows: Vec<Element<'_, Message>> = wall_style_form
        .layers
        .iter()
        .enumerate()
        .map(|(i, lb)| {
            layer_row(i, layer_count, lb, all_materials_ref, all_layer_names_ref, drag_index)
        })
        .collect();

    let mut detail_col = column![
        text(form_title).size(13),
        row![
            text(t!("Name")).size(10).style(muted).width(100),
            text_input("", wall_style_form.name)
                .on_input(Message::AecStyleManagerWallStyleNameChanged)
                .size(11)
                .padding([4, 6]),
        ]
        .spacing(8),
        row![
            text(t!("Based on")).size(10).style(muted).width(100),
            button(
                row![
                    text(selected_parent).size(11),
                    Space::new(),
                    text("▾").size(9),
                ]
                .align_y(iced::Center),
            )
            .on_press(Message::AecStylePickerOpen(
                crate::app::StylePickerTarget::WallStyleParent
            ))
            .style(button::subtle)
            .padding([4, 6])
            .width(Fill),
        ]
        .spacing(8),
    ]
    .spacing(7);

    if !wall_style_form.inheritance_chain.is_empty() {
        let mut chain_row = row![
            text(t!("Inheritance")).size(10).style(muted).width(100),
        ]
        .spacing(4)
        .align_y(iced::Center);
        for (i, name) in wall_style_form.inheritance_chain.iter().enumerate() {
            if i > 0 {
                chain_row = chain_row.push(text("→").size(10).style(muted));
            }
            chain_row = chain_row.push(text(name.clone()).size(11));
        }
        chain_row = chain_row.push(text("→").size(10).style(muted));
        chain_row = chain_row.push(
            text(if wall_style_form.name.is_empty() {
                t!("(this style)").into_owned()
            } else {
                wall_style_form.name.to_string()
            })
            .size(11),
        );
        detail_col = detail_col.push(chain_row);
    }

    detail_col = detail_col.extend([
        row![
            text(t!("Layers")).size(10).style(muted),
            Space::new(),
            button(text(t!("Add Layer")).size(10))
                .on_press(Message::AecStyleManagerWallStyleLayerAdd)
                .padding([2, 8]),
        ]
        .spacing(8)
        .into(),
        layer_header_row(),
        container(scrollable(column(layer_rows).spacing(4)).height(180))
            .style(|theme: &Theme| container::Style {
                border: Border {
                    color: theme.palette().background.neutral.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            })
            .padding(iced::Padding {
                top: 4.0,
                right: 14.0,
                bottom: 4.0,
                left: 4.0,
            })
            .into(),
    ]);

    if !wall_style_form.effective_layers.is_empty() {
        detail_col = detail_col.push(Space::new().height(8)).push(
            text(t!("Effective Buildup (Preview)"))
                .size(10)
                .style(muted),
        );
        let preview_rows: Vec<Element<'_, Message>> = wall_style_form
            .effective_layers
            .iter()
            .map(|l| {
                let mat_name = wall_style_form
                    .all_materials
                    .iter()
                    .find(|(id, _)| *id == l.material_id)
                    .map(|(_, name)| name.to_string())
                    .unwrap_or_else(|| l.material_id.clone());
                let thick_label = match &l.thickness {
                    crate::modules::aec::engine::wall_style::LayerValue::Fixed(v) => {
                        format!("{:.1}", v * 100.0)
                    }
                    crate::modules::aec::engine::wall_style::LayerValue::Formula(s) => s.clone(),
                };
                row![
                    Space::new().width(LAYER_COL_REORDER_W),
                    text(mat_name).size(11).width(LAYER_COL_MATERIAL_W),
                    text(thick_label)
                        .size(11)
                        .width(LAYER_COL_THICKNESS_W),
                    text(format!("{:.1}", l.axis_offset.as_fixed_or(0.0) * 100.0))
                        .size(11)
                        .width(LAYER_COL_GAP_W),
                    text(format!("{:.1}", l.bottom_offset * 100.0))
                        .size(11)
                        .width(LAYER_COL_OFFSET_W),
                    text(format!("{:.1}", l.top_offset * 100.0))
                        .size(11)
                        .width(LAYER_COL_OFFSET_W),
                    text(format!("{:?}", l.function))
                        .size(11)
                        .width(LAYER_COL_FUNCTION_W),
                ]
                .spacing(8)
                .into()
            })
            .collect();
        detail_col = detail_col.push(
            container(scrollable(column(preview_rows).spacing(4)).height(100))
                .padding(4)
                .style(|theme: &Theme| container::Style {
                    background: Some(theme.palette().background.neutral.color.scale_alpha(0.2).into()),
                    ..Default::default()
                }),
        );
    }

    detail_col = detail_col
        .push(Space::new().height(10))
        .push(
            text(tr!("aec", "display-on-plan-manager"))
            .size(10)
            .style(muted),
        )
        .push(
            button(text(t!("Plan-Manager öffnen…")).size(11))
                .style(button::secondary)
                .padding([5, 12])
                .on_press(Message::AecPlanManagerOpen),
        );

    let mut actions = row![
        Space::new(),
        button(text(t!("Save")).size(11))
            .style(button::subtle)
            .padding([5, 12])
            .on_press(Message::AecStyleManagerWallStyleSave),
        button(text(t!("Save & Update Drawing")).size(11))
            .style(button::primary)
            .padding([5, 12])
            .on_press(Message::AecStyleManagerWallStyleSaveAndApply),
    ]
    .spacing(8);

    if !wall_style_form.is_new {
        actions = actions.push(
            button(text(t!("Delete")).size(11))
                .style(button::danger)
                .padding([5, 12])
                .on_press(Message::AecStyleManagerWallStyleDelete),
        );
        // → Standard: only pure project entries without a global counterpart.
        if selected_source == Some(LibrarySource::Project) && !has_standard_counterpart {
            actions = actions.push(
                button(text(t!("→ Standard")).size(11))
                    .padding([5, 12])
                    .on_press(Message::AecStyleManagerCopyWallStyleToGlobal),
            );
        }
        // → Projekt: only Standard entries (copy into the project library).
        if selected_source == Some(LibrarySource::Standard) {
            actions = actions.push(
                button(text(t!("→ Projekt")).size(11))
                    .padding([5, 12])
                    .on_press(Message::AecStyleManagerCopyWallStyleToProject),
            );
        }
    }

    column![detail_col, Space::new(), actions]
        .spacing(10)
        .height(Fill)
        .into()
}

fn layer_header_row<'a>() -> Element<'a, Message> {
    row![
        Space::new().width(LAYER_COL_REORDER_W),
        text(t!("Material")).size(10).style(muted).width(LAYER_COL_MATERIAL_W),
        text(t!("Thick (cm)")).size(10).style(muted).width(LAYER_COL_THICKNESS_W),
        text(t!("Achsversatz (cm)")).size(10).style(muted).width(LAYER_COL_GAP_W),
        text(t!("Bot. (cm)")).size(10).style(muted).width(LAYER_COL_OFFSET_W),
        text(t!("Top (cm)")).size(10).style(muted).width(LAYER_COL_OFFSET_W),
        text(t!("Function")).size(10).style(muted).width(LAYER_COL_FUNCTION_W),
        text(t!("Lyr. Over.")).size(10).style(muted).width(LAYER_COL_OVERRIDE_W),
        text(t!("Role")).size(10).style(muted).width(LAYER_COL_ROLE_W),
        text(t!("Hatch")).size(10).style(muted).width(LAYER_COL_HATCH_W),
        Space::new().width(24), // Delete button space
    ]
    .spacing(8)
    .into()
}

fn layer_row<'a>(
    index: usize,
    layer_count: usize,
    buffer: &'a crate::app::AecLayerBuffer,
    all_materials: &[(&'a str, &'a str)],
    _all_layer_names: &[String],
    drag_index: Option<usize>,
) -> Element<'a, Message> {
    let functions: Vec<String> = vec![
        "Structural".to_string(),
        "Insulation".to_string(),
        "Finish".to_string(),
        "Other".to_string(),
    ];

    let mut up_button = button(text("▲").size(10)).padding([2, 5]);
    if index > 0 {
        up_button = up_button.on_press(Message::AecStyleManagerWallStyleLayerMoveUp(index));
    }
    let mut down_button = button(text("▼").size(10)).padding([2, 5]);
    if index + 1 < layer_count {
        down_button = down_button.on_press(Message::AecStyleManagerWallStyleLayerMoveDown(index));
    }

    let armed = drag_index == Some(index);
    let handle = mouse_area(
        button(text("⠿").size(12))
            .style(if armed { button::primary } else { button::subtle })
            .padding([2, 6])
            .on_press(if armed {
                Message::AecStyleManagerWallStyleLayerDragEnd
            } else if drag_index.is_some() {
                Message::AecStyleManagerWallStyleLayerDragOver(index)
            } else {
                Message::AecStyleManagerWallStyleLayerDragStart(index)
            }),
    )
    .interaction(iced::mouse::Interaction::Grab);

    row![
        row![handle, up_button, down_button].spacing(2).width(LAYER_COL_REORDER_W),
        {
            let none_label = t!("(None)").into_owned();
            let selected_label = if buffer.material_id.is_empty() {
                none_label.clone()
            } else {
                all_materials
                    .iter()
                    .find(|(id, _)| *id == buffer.material_id)
                    .map(|(_, name)| name.to_string())
                    .unwrap_or_else(|| buffer.material_id.clone())
            };
            let mut material_labels: Vec<String> = vec![none_label];
            material_labels.extend(all_materials.iter().map(|(_, name)| name.to_string()));
            let materials_for_select: Vec<(String, String)> = all_materials
                .iter()
                .map(|(id, name)| (id.to_string(), name.to_string()))
                .collect();
            pick_list(
                Some(selected_label),
                material_labels,
                |name: &String| name.clone(),
            )
            .on_select(move |name: String| {
                let id = materials_for_select
                    .iter()
                    .find(|(_, n)| n == &name)
                    .map(|(id, _)| id.clone())
                    .unwrap_or_default();
                Message::AecStyleManagerWallStyleLayerMaterialChanged(index, id)
            })
            .text_size(11)
            .width(LAYER_COL_MATERIAL_W)
        },
        {
            // Accept either a number or an arithmetic formula (e.g. "BB * 0.5").
            // Invalid formulas are flagged with a danger-styled field; they still
            // remain editable and are validated again on save.
            let thickness_invalid = {
                let parsed =
                    crate::modules::aec::engine::wall_style::LayerValue::parse_cm_str(&buffer.thickness);
                match parsed {
                    crate::modules::aec::engine::wall_style::LayerValue::Fixed(_) => false,
                    crate::modules::aec::engine::wall_style::LayerValue::Formula(ref f) => {
                        let vars = crate::modules::aec::engine::wall_style::wall_vars(1.0);
                        crate::modules::aec::engine::expr::eval_formula(f, &vars).is_err()
                    }
                }
            };
            let input = text_input("", &buffer.thickness)
                .on_input(move |v| {
                    Message::AecStyleManagerWallStyleLayerThicknessChanged(index, v)
                })
                .size(11)
                .width(LAYER_COL_THICKNESS_W);
            if thickness_invalid {
                input.style(invalid_thickness_style)
            } else {
                input
            }
        },
        text_input(tr!("aec", "placeholder-axis-offset").as_str(), &buffer.axis_offset)
            .on_input(move |v| Message::AecStyleManagerWallStyleLayerAxisOffsetChanged(index, v))
            .size(11)
            .width(LAYER_COL_GAP_W),
        text_input(tr!("aec", "placeholder-bottom-cm").as_str(), &buffer.bottom_offset)
            .on_input(move |v| Message::AecStyleManagerWallStyleLayerBottomOffsetChanged(index, v))
            .size(11)
            .width(LAYER_COL_OFFSET_W),
        text_input(tr!("aec", "placeholder-top-cm").as_str(), &buffer.top_offset)
            .on_input(move |v| Message::AecStyleManagerWallStyleLayerTopOffsetChanged(index, v))
            .size(11)
            .width(LAYER_COL_OFFSET_W),
        pick_list(Some(buffer.function.clone()), functions, |func: &String| {
            func.clone()
        })
        .on_select(move |func| Message::AecStyleManagerWallStyleLayerFunctionChanged(index, func))
        .text_size(11)
        .width(LAYER_COL_FUNCTION_W),
        {
            let label = if buffer.layer_override.is_empty() {
                t!("(Default)").into_owned()
            } else {
                buffer.layer_override.clone()
            };
            button(
                row![
                    text(label).size(11),
                    Space::new(),
                    text("▾").size(9),
                ]
                .align_y(iced::Center),
            )
            .on_press(Message::AecStylePickerOpen(
                crate::app::StylePickerTarget::LayerOverride(index),
            ))
            .style(button::subtle)
            .padding([4, 6])
            .width(LAYER_COL_OVERRIDE_W)
        },
        text_input(tr!("aec", "placeholder-role").as_str(), &buffer.role_tag)
            .on_input(move |v| Message::AecStyleManagerWallStyleLayerRoleTagChanged(index, v))
            .size(11)
            .width(LAYER_COL_ROLE_W),
        text_input(tr!("aec", "placeholder-hatch").as_str(), &buffer.hatch_override)
            .on_input(move |v| Message::AecStyleManagerWallStyleLayerHatchOverrideChanged(index, v))
            .size(11)
            .width(LAYER_COL_HATCH_W),
        button(text("✕").size(10))
            .style(button::danger)
            .on_press(Message::AecStyleManagerWallStyleLayerRemove(index))
            .padding([4, 8]),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// Danger border for thickness fields whose formula fails validation.
fn invalid_thickness_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let danger = theme.palette().danger.base.color;
    let mut style = text_input::default(theme, status);
    style.border.color = danger;
    style
}

fn component_slot_table_view<'a>(
    slot_visibility: &std::collections::HashMap<WallComponentSlot, bool>,
    slot_overrides: &std::collections::HashMap<
        WallComponentSlot,
        crate::modules::aec::engine::display_component::ComponentStyleOverride,
    >,
) -> Element<'a, Message> {
    let mut col = column![].spacing(8);

    let groups = [
        (
            t!("2D"),
            vec![
                (WallComponentSlot::AxisLine, t!("Achslinie")),
                (WallComponentSlot::Contour2D, t!("2D Gesamtkontur")),
                (
                    WallComponentSlot::ContourHatch2D,
                    t!("2D Schraffur der Gesamtkontur"),
                ),
                (WallComponentSlot::Layers2D, t!("2D Wandschichten")),
                (WallComponentSlot::LayerHatch2D, t!("2D Schraffuren der Schichten")),
            ],
        ),
        (
            t!("3D"),
            vec![
                (WallComponentSlot::Solid3D, t!("3D Gesamtkörper")),
                (
                    WallComponentSlot::SurfaceStyle3D,
                    t!("3D Oberflächenstil"),
                ),
            ],
        ),
        (
            t!("Schnitte / Ansichten"),
            vec![
                (
                    WallComponentSlot::SectionRepresentation,
                    t!("Schnittdarstellung"),
                ),
                (
                    WallComponentSlot::ElevationRepresentation,
                    t!("Ansichtsdarstellung"),
                ),
            ],
        ),
    ];

    for (group_title, slots) in groups {
        let mut group_col = column![text(group_title).size(11).style(muted)].spacing(3);
        for (slot, label) in slots {
            let visible = slot_visibility.get(&slot).copied().unwrap_or(true);
            let has_override = slot_overrides.contains_key(&slot);
            let badge_label = if has_override {
                t!("Override")
            } else {
                t!("Standard")
            };
            let badge_style = if has_override {
                button::primary
            } else {
                button::secondary
            };
            group_col = group_col.push(
                row![
                    iced::widget::checkbox(visible)
                        .on_toggle(move |v| {
                            Message::AecStyleManagerProfileSlotVisibilityToggle(slot, v)
                        })
                        .size(13),
                    text(label).size(11).width(220),
                    Space::new(),
                    button(text(badge_label).size(9))
                        .style(badge_style)
                        .padding([2, 6])
                        .on_press(Message::AecStyleManagerProfileSlotStyleOpen(slot)),
                ]
                .spacing(8)
                .align_y(iced::Center),
            );
        }
        col = col.push(group_col);
    }

    col.into()
}
pub fn display_profiles_section<'a>(
    profiles: &DisplayProfileFormState<'a>,
    layers: &'a [crate::app::AecLayerBuffer],
) -> Element<'a, Message> {
    let mut section = column![
        text(t!("Darstellungs-Profile (je Planart)")).size(12),
    ]
    .spacing(6);

    if profiles.display_config_names.is_empty() {
        section = section.push(
            text(t!("Keine Planarten in der Bibliothek — im Plan-Manager anlegen."))
                .size(10)
                .style(muted),
        );
        return section.into();
    }

    let mut table = column![].spacing(2);
    for name in &profiles.display_config_names {
        let is_override = profiles.existing_overrides.contains(name);
        let is_selected = profiles.selected == Some(name.as_str());
        let status_label = if is_override { t!("Override") } else { t!("Standard") };
        let name_owned = name.clone();
        table = table.push(
            button(
                row![
                    text(name.clone()).size(11),
                    Space::new(),
                    container(text(status_label).size(9))
                        .padding([1, 5])
                        .style(move |theme: &Theme| container::Style {
                            background: Some(Background::Color(
                                theme.palette().background.strong.color.scale_alpha(0.55),
                            )),
                            text_color: Some(theme.palette().background.base.text.scale_alpha(0.85)),
                            border: Border { radius: 3.0.into(), ..Default::default() },
                            ..Default::default()
                        }),
                ]
                .spacing(4)
                .align_y(iced::Center),
            )
            .on_press(Message::AecStyleManagerProfileSelect(name_owned))
            .style(list_style(is_selected))
            .padding([5, 8])
            .width(Fill),
        );
    }
    section = section.push(container(scrollable(table).height(120)).padding(4));

    if profiles.selected.is_some() {
        section = section.push(Space::new().height(4));
        section = section.push(component_slot_table_view(
            &profiles.slot_visibility,
            profiles.slot_overrides,
        ));
        if let Some(slot) = profiles.editing_slot {
            section = section.push(Space::new().height(6));
            section = section.push(super::aec_ui_util::style_editor_form(
                format!("{} — {}", t!("Stil-Override"), slot_label(slot)),
                profiles.slot_style_editor,
                Message::AecStyleManagerProfileSlotStyleLineTypeChanged,
                Message::AecStyleManagerProfileSlotStyleLineColorChanged,
                Message::AecStyleManagerProfileSlotStyleLineColorPickerToggle,
                crate::app::ColorPickTarget::AecWallStyleSlotLineColor,
                Message::AecStyleManagerProfileSlotStyleHatchPatternChanged,
                Message::AecStyleManagerProfileSlotStyleHatchPickerToggle,
                Message::AecStyleManagerProfileSlotStyleHatchColorChanged,
                Message::AecStyleManagerProfileSlotStyleHatchColorPickerToggle,
                crate::app::ColorPickTarget::AecWallStyleSlotHatchColor,
                Message::AecStyleManagerProfileSlotStyleFillColorChanged,
                Message::AecStyleManagerProfileSlotStyleFillColorPickerToggle,
                crate::app::ColorPickTarget::AecWallStyleSlotFillColor,
            ));
            section = section.push(
                row![
                    Space::new(),
                    button(text(t!("Übernehmen")).size(11))
                        .style(button::primary)
                        .padding([4, 10])
                        .on_press(Message::AecStyleManagerProfileSlotStyleApply),
                    button(text(t!("Entfernen")).size(11))
                        .style(button::danger)
                        .padding([4, 10])
                        .on_press(Message::AecStyleManagerProfileSlotStyleClear),
                    button(text(t!("Schließen")).size(11))
                        .padding([4, 10])
                        .on_press(Message::AecStyleManagerProfileSlotStyleClose),
                ]
                .spacing(8),
            );
        }
        section = section.push(Space::new().height(4));

        section = section.push(layer_filter_slot_view(
            t!("Schichten für 2D-Gesamtkontur (Contour2D)").into_owned(),
            layers,
            profiles.contour_explicit,
            profiles.contour_selected,
            Message::AecStyleManagerProfileContourModeToggle,
            Message::AecStyleManagerProfileContourLayerToggle,
        ));
        section = section.push(layer_filter_slot_view(
            t!("Schichten für 3D-Gesamtkörper (Solid3D)").into_owned(),
            layers,
            profiles.solid_explicit,
            profiles.solid_selected,
            Message::AecStyleManagerProfileSolidModeToggle,
            Message::AecStyleManagerProfileSolidLayerToggle,
        ));

        section = section.push(
            row![
                text(t!("Hatch-Winkel")).size(10).style(muted).width(100),
                text_input(tr!("aec", "placeholder-hatch-angle").as_str(), profiles.hatch_angle)
                    .on_input(Message::AecStyleManagerProfileHatchAngleChanged)
                    .size(11)
                    .padding([4, 6])
                    .width(80),
                iced::widget::checkbox(profiles.hatch_relative)
                    .label(t!("Relativ zur Wand").into_owned())
                    .on_toggle(Message::AecStyleManagerProfileHatchRelativeToggle)
                    .size(13)
                    .text_size(11),
            ]
            .spacing(8)
            .align_y(iced::Center),
        );

        section = section.push(
            row![
                Space::new(),
                button(text(t!("Entfernen")).size(11))
                    .style(button::danger)
                    .padding([4, 10])
                    .on_press(Message::AecStyleManagerProfileRemove),
                button(text(t!("Profil speichern")).size(11))
                    .style(button::primary)
                    .padding([4, 10])
                    .on_press(Message::AecStyleManagerProfileSave),
            ]
            .spacing(8),
        );
    }

    section.into()
}

/// One slot's layer-filter checklist ("Alle Schichten" vs. "Auswahl" +
/// per-layer checkboxes), built from the style's own edit-buffer layers
/// (`AecLayerBuffer`), following the same `layer_filter_to_ui_state`
/// pattern already used by the Plan Manager, but scoped to a single slot
/// so `Contour2D` and `Solid3D` are fully independent.

fn slot_label(slot: WallComponentSlot) -> String {
    match slot {
        WallComponentSlot::AxisLine => t!("Achslinie").into_owned(),
        WallComponentSlot::Contour2D => t!("2D Gesamtkontur").into_owned(),
        WallComponentSlot::ContourHatch2D => t!("2D Schraffur der Gesamtkontur").into_owned(),
        WallComponentSlot::Layers2D => t!("2D Wandschichten").into_owned(),
        WallComponentSlot::LayerHatch2D => t!("2D Schraffuren der Schichten").into_owned(),
        WallComponentSlot::Solid3D => t!("3D Gesamtkörper").into_owned(),
        WallComponentSlot::SurfaceStyle3D => t!("3D Oberflächenstil").into_owned(),
        WallComponentSlot::SectionRepresentation => t!("Schnittdarstellung").into_owned(),
        WallComponentSlot::ElevationRepresentation => t!("Ansichtsdarstellung").into_owned(),
    }
}

fn layer_filter_slot_view<'a>(
    title: String,
    layers: &'a [crate::app::AecLayerBuffer],
    is_explicit: bool,
    selected: &'a [LayerRef],
    mode_toggle: fn(bool) -> Message,
    layer_toggle: fn(LayerRef) -> Message,
) -> Element<'a, Message> {
    let mode_row = row![
        button(text(t!("Alle Schichten")).size(10))
            .style(if !is_explicit { button::primary } else { button::secondary })
            .padding([3, 8])
            .on_press(mode_toggle(false)),
        button(text(t!("Auswahl")).size(10))
            .style(if is_explicit { button::primary } else { button::secondary })
            .padding([3, 8])
            .on_press(mode_toggle(true)),
    ]
    .spacing(6);

    let mut section = column![text(title).size(10).style(muted), mode_row].spacing(4);

    if is_explicit {
        let mut items: Vec<Element<'a, Message>> = Vec::new();
        for (idx, lb) in layers.iter().enumerate() {
            let layer_ref = LayerRef {
                material_id: lb.material_id.clone(),
                role_tag: None,
                index: idx,
                // The UI edit buffer doesn't track stable layer identity;
                // matching here falls back to the material/role/index triple.
                layer_id: None,
            };
            let checked = selected.contains(&layer_ref);
            let label = if lb.material_id.is_empty() {
                format!("#{}", idx + 1)
            } else {
                format!("#{} {}", idx + 1, lb.material_id)
            };
            let lr = layer_ref.clone();
            items.push(
                iced::widget::checkbox(checked)
                    .label(label)
                    .on_toggle(move |_| layer_toggle(lr.clone()))
                    .size(12)
                    .text_size(10)
                    .into(),
            );
        }
        let checklist = iced::widget::Row::with_children(items)
            .spacing(10)
            .wrap()
            .vertical_spacing(4.0);
        section = section.push(container(checklist).padding(4));
    }

    section.into()
}
