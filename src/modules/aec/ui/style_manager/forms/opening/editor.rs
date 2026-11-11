use super::*;
use iced::widget::{column, row};

pub(super) fn form_preview_style(form: &OpeningStyleFormState<'_>) -> OpeningStyle {
    let width = crate::modules::aec::styles::opening_style_manager::parse_dim(form.width)
        .unwrap_or(1.2);
    let height = crate::modules::aec::styles::opening_style_manager::parse_dim(form.height)
        .unwrap_or(1.2);
    let sill = crate::modules::aec::styles::opening_style_manager::parse_dim(form.sill).unwrap_or(0.0);
    let frame =
        crate::modules::aec::styles::opening_style_manager::parse_dim(form.frame).unwrap_or(0.06);
    let angle =
        crate::modules::aec::styles::opening_style_manager::parse_dim(form.angle).unwrap_or(90.0);
    let spring =
        crate::modules::aec::styles::opening_style_manager::parse_dim(form.spring).unwrap_or(0.0);
    let mut style = crate::modules::aec::styles::opening_style_manager::draft_opening_style(
        Some("preview".to_string()),
        if form.name.is_empty() { "preview" } else { form.name },
        form.parent_id.map(str::to_string),
        form.kind,
        width.max(1e-6),
        height.max(1e-6),
        sill,
        form.hinge,
        form.swing_side,
        frame,
        angle,
        form.shape,
        spring,
    )
    .unwrap_or_else(|_| OpeningStyle::standard_window());
    let mut division = crate::modules::aec::styles::opening_style_manager::opening_grid_from_fields(
        form.vertical,
        form.horizontal,
        form.vertical_muntins,
        form.horizontal_muntins,
        form.cells,
    );
    division.profile_id = form.profile_ids.get(&OpeningProfilePart::Division).cloned();
    style.grid = form.authored_grid.cloned().unwrap_or_else(|| {
        construction_from_division(&division, width.max(1e-6), height.max(1e-6))
    });
    let dim = |value: &str, fallback: f64| {
        crate::modules::aec::styles::opening_style_manager::parse_dim(value).unwrap_or(fallback)
    };
    style.parts.fixed_frame.face_width = frame.max(0.0);
    style.parts.fixed_frame.enabled = component_enabled(form, OpeningEditorGroup::FixedFrame);
    style.parts.fixed_frame.profile_id = form.profile_ids.get(&OpeningProfilePart::FixedFrame).cloned();
    style.parts.fixed_frame.wall_depth = dim(form.frame_depth, style.parts.fixed_frame.wall_depth);
    style.parts.fixed_frame.axis_offset = dim(form.frame_offset, 0.0);
    style.parts.leaf_frame.face_width =
        dim(form.leaf_face, style.parts.leaf_frame.face_width);
    style.parts.leaf_frame.profile_id = form.profile_ids.get(&OpeningProfilePart::LeafFrame).cloned();
    style.parts.leaf_frame.depth = dim(form.leaf_depth, style.parts.leaf_frame.depth);
    style.parts.leaf_frame.rebate_overlap = dim(form.leaf_rebate, style.parts.leaf_frame.rebate_overlap);
    style.parts.leaf_frame.lateral_overlap = dim(form.leaf_lateral, style.parts.leaf_frame.lateral_overlap);
    style.parts.glazing.thickness = dim(form.glazing_thickness, style.parts.glazing.thickness);
    style.parts.glazing.inset = dim(form.glazing_inset, style.parts.glazing.inset);
    style.parts.exterior_sill.thickness = dim(form.ext_sill_thickness, 0.0);
    style.parts.exterior_sill.profile_id = form.profile_ids.get(&OpeningProfilePart::ExteriorSill).cloned();
    style.parts.exterior_sill.overhang = dim(form.ext_sill_overhang, 0.0);
    style.parts.exterior_sill.upstand = dim(form.ext_sill_upstand, 0.0);
    style.parts.exterior_sill.side_overhang_left = dim(form.ext_sill_side_left, 0.0);
    style.parts.exterior_sill.side_overhang_right = dim(form.ext_sill_side_right, 0.0);
    style.parts.interior_board.thickness = dim(form.int_board_thickness, 0.0);
    style.parts.interior_board.profile_id = form.profile_ids.get(&OpeningProfilePart::InteriorBoard).cloned();
    style.parts.interior_board.overhang = dim(form.int_board_overhang, 0.0);
    style.parts.interior_board.upstand = dim(form.int_board_upstand, 0.0);
    style.parts.interior_board.side_overhang_left = dim(form.int_board_side_left, 0.0);
    style.parts.interior_board.side_overhang_right = dim(form.int_board_side_right, 0.0);
    style.parts.threshold.height = dim(form.threshold_height, 0.0);
    style.parts.threshold.profile_id = form.profile_ids.get(&OpeningProfilePart::Threshold).cloned();
    style.parts.threshold.width = dim(form.threshold_width, 0.0);
    style.parts.shutter_box.height = dim(form.shutter_height, 0.0);
    style.parts.shutter_box.profile_id = form.profile_ids.get(&OpeningProfilePart::ShutterBox).cloned();
    style.parts.shutter_box.depth = dim(form.shutter_depth, 0.0);
    style.parts.shutter_box.seat_above_frame = dim(form.shutter_seat, 0.0);
    style.parts.shutter_box.installation_depth = dim(form.shutter_install, 0.0);
    style.parts.lintel.height = dim(form.lintel_height, 0.0);
    style.parts.lintel.profile_id = form.profile_ids.get(&OpeningProfilePart::Lintel).cloned();
    style.parts.lintel.bearing_left = dim(form.lintel_left, 0.0);
    style.parts.lintel.bearing_right = dim(form.lintel_right, 0.0);
    style.parts.leaf_frame.enabled = component_enabled(form, OpeningEditorGroup::Leaf);
    style.parts.glazing.enabled = component_enabled(form, OpeningEditorGroup::Glazing);
    style.parts.exterior_sill.enabled = component_enabled(form, OpeningEditorGroup::ExteriorSill);
    style.parts.interior_board.enabled = component_enabled(form, OpeningEditorGroup::InteriorBoard);
    style.parts.threshold.enabled = component_enabled(form, OpeningEditorGroup::Threshold);
    style.parts.shutter_box.enabled = component_enabled(form, OpeningEditorGroup::ShutterBox);
    style.parts.lintel.enabled = component_enabled(form, OpeningEditorGroup::Lintel);
    style.looks = crate::modules::aec::styles::opening_style_manager::opening_looks_from_buffers(
        form.look_buffers,
    );
    style
}

pub(super) fn component_enabled(form: &OpeningStyleFormState<'_>, group: OpeningEditorGroup) -> bool {
    form.component_enabled.get(&group).copied().unwrap_or(true)
}

#[derive(Clone, Copy, Default)]
pub(super) struct PreviewLookColors {
    pub(super) line: Option<IcedColor>,
    pub(super) hatch: Option<IcedColor>,
}

pub(super) fn preview_color_from_acad(color: &acadrust::types::Color) -> Option<IcedColor> {
    let rgba = crate::modules::aec::engine::views::style_resolver::color_to_rgba8_opt(color)?;
    Some(IcedColor::from_rgba8(
        rgba.r,
        rgba.g,
        rgba.b,
        rgba.a as f32 / 255.0,
    ))
}

pub(super) fn preview_color_from_rgb(rgb: u32) -> IcedColor {
    IcedColor::from_rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub(super) fn preview_look_colors(
    style: &OpeningStyle,
    library: &StyleLibrary,
) -> HashMap<OpeningLookRole, PreviewLookColors> {
    OpeningLookRole::all()
        .iter()
        .copied()
        .map(|role| {
            let look = style.looks.get(role);
            let material = style
                .material_id_for_look(role)
                .and_then(|material_id| {
                    library.materials.iter().find(|material| {
                        material.id == material_id || material.name == material_id
                    })
                });
            let line = match look.line_color.as_ref() {
                Some(color) => preview_color_from_acad(color),
                None => material.map(|material| preview_color_from_rgb(material.line_color)),
            };
            let hatch = material
                .and_then(|material| material.hatch_color.as_ref())
                .and_then(preview_color_from_acad);
            (role, PreviewLookColors { line, hatch })
        })
        .collect()
}

pub(crate) fn opening_grid_preview_color() -> iced::Color {
    iced::Color::from_rgb8(128, 128, 128)
}

pub(super) fn preview_look_styles(
    style: &OpeningStyle,
    library: &StyleLibrary,
) -> HashMap<OpeningLookRole, PreviewLookColors> {
    preview_look_colors(style, library)
}

pub(super) fn optional_group(group: OpeningEditorGroup) -> bool {
    matches!(
        group,
        OpeningEditorGroup::FixedFrame
            | OpeningEditorGroup::Leaf
            | OpeningEditorGroup::Glazing
            | OpeningEditorGroup::ExteriorSill
            | OpeningEditorGroup::InteriorBoard
            | OpeningEditorGroup::Threshold
            | OpeningEditorGroup::ShutterBox
            | OpeningEditorGroup::Lintel
    )
}

pub(super) fn opening_group_label(group: OpeningEditorGroup) -> String {
    let key = match group {
        OpeningEditorGroup::FixedFrame => "opening-component-fixed-frame",
        OpeningEditorGroup::Leaf => "opening-component-leaf",
        OpeningEditorGroup::Glazing => "opening-component-glazing",
        OpeningEditorGroup::ExteriorSill => "opening-component-exterior-sill",
        OpeningEditorGroup::InteriorBoard => "opening-component-interior-board",
        OpeningEditorGroup::Threshold => "opening-component-threshold",
        OpeningEditorGroup::ShutterBox => "opening-component-shutter-box",
        OpeningEditorGroup::Lintel => "opening-component-lintel",
        OpeningEditorGroup::Divisions => "opening-component-divisions",
        OpeningEditorGroup::Symbol => "opening-component-symbol",
        OpeningEditorGroup::Ratings => "opening-component-ratings",
    };
    opening_label(key)
}

pub(super) fn component_group_list<'a>(form: &OpeningStyleFormState<'a>) -> Element<'a, Message> {
    let active_group = if form.editor_tab == OpeningEditorGroup::Divisions {
        OpeningEditorGroup::FixedFrame
    } else {
        form.editor_tab
    };
    let mut list = column![text(tr!("aec", "opening-components")).size(11).style(muted)]
        .spacing(4);
    for group in OpeningEditorGroup::all()
        .iter()
        .copied()
        .filter(|group| {
            *group != OpeningEditorGroup::Ratings
                && *group != OpeningEditorGroup::Divisions
                && group.applies_to(form.kind)
        })
    {
        let selected = active_group == group;
        let mut item = row![
            button(text(opening_group_label(group)).size(11))
                .style(if selected { button::primary } else { button::secondary })
                .padding([4, 7])
                .on_press(Message::Aec(
                    AecMessage::AecOpeningStyleManagerSetEditorTab(group),
                )),
        ]
        .spacing(4)
        .align_y(iced::Center);
        if optional_group(group) {
            let enabled = form.component_enabled.get(&group).copied().unwrap_or(true);
            item = item.push(
                iced::widget::checkbox(enabled)
                    .on_toggle(move |value| {
                        Message::Aec(
                            AecMessage::AecOpeningStyleManagerComponentEnabledChanged(
                                group, value,
                            ),
                        )
                    })
                    .size(13),
            );
        }
        list = list.push(item);
    }
    scrollable(list).height(FillLen).width(190).into()
}

pub(super) fn opening_group_roles(group: OpeningEditorGroup) -> &'static [OpeningLookRole] {
    use OpeningLookRole as Role;
    match group {
        OpeningEditorGroup::FixedFrame => &[Role::FixedFrame],
        OpeningEditorGroup::Leaf => &[Role::Leaf, Role::Swing],
        OpeningEditorGroup::Glazing => &[Role::Glazing],
        OpeningEditorGroup::ExteriorSill => &[Role::ExteriorSill],
        OpeningEditorGroup::InteriorBoard => &[Role::InteriorBoard],
        OpeningEditorGroup::Threshold => &[Role::Threshold],
        OpeningEditorGroup::ShutterBox => &[Role::ShutterBox],
        OpeningEditorGroup::Lintel => &[Role::Lintel],
        OpeningEditorGroup::Divisions => &[Role::Division],
        OpeningEditorGroup::Symbol | OpeningEditorGroup::Ratings => &[],
    }
}

pub(super) fn profile_is_valid(
    form: &OpeningStyleFormState<'_>,
    library: &StyleLibrary,
    part: OpeningProfilePart,
) -> bool {
    form.profile_ids
        .get(&part)
        .and_then(|id| library.find_section_profile(id))
        .is_some_and(|profile| profile.is_valid())
}

pub(super) fn profile_picker<'a>(
    form: &OpeningStyleFormState<'a>,
    library: &'a StyleLibrary,
    part: OpeningProfilePart,
) -> Element<'a, Message> {
    let selected_id = form.profile_ids.get(&part).cloned();
    let mut options = vec![(String::new(), tr!("aec", "opening-profile-local"))];
    options.extend(
        library
            .profiles
            .iter()
            .map(|profile| (profile.style.id.clone(), profile.style.name.clone())),
    );
    if let Some(id) = selected_id.as_ref() {
        if !options.iter().any(|(option_id, _)| option_id == id) {
            options.push((
                id.clone(),
                format!("{} · {}", id, tr!("aec", "opening-profile-missing")),
            ));
        }
    }
    let selected = selected_id
        .as_ref()
        .and_then(|id| options.iter().find(|(option_id, _)| option_id == id))
        .cloned()
        .or_else(|| options.first().cloned());

    row![
        text(tr!("aec", "opening-profile")).size(10).style(muted).width(100),
        pick_list(selected, options, |option: &(String, String)| option.1.clone())
            .on_select(move |(id, _)| {
                Message::Aec(AecMessage::AecOpeningStyleProfileChanged(
                    part,
                    (!id.is_empty()).then_some(id),
                ))
            })
            .text_size(11)
            .width(240),
    ]
    .spacing(8)
    .into()
}

pub(super) fn opening_editor_tab_content<'a>(
    form: &OpeningStyleFormState<'a>,
    library: &'a StyleLibrary,
) -> Element<'a, Message> {
    let group = if form.editor_tab == OpeningEditorGroup::Divisions {
        OpeningEditorGroup::FixedFrame
    } else {
        form.editor_tab
    };
    if optional_group(group) && !form.component_enabled.get(&group).copied().unwrap_or(true) {
        return column![
            text(opening_group_label(group)).size(12),
            text(tr!("aec", "opening-component-disabled")).size(11).style(muted),
        ]
        .spacing(8)
        .into();
    }
    let mut col = column![text(opening_group_label(group)).size(12)].spacing(6);
    match group {
        OpeningEditorGroup::FixedFrame => {
            col = col.push(profile_picker(form, library, OpeningProfilePart::FixedFrame));
            if !profile_is_valid(form, library, OpeningProfilePart::FixedFrame) {
                col = col
                    .push(dim_row("opening-frame-face", "frame", form.frame))
                    .push(dim_row("opening-frame-depth", "frame_depth", form.frame_depth));
            }
            col = col.push(dim_row("opening-frame-offset", "frame_offset", form.frame_offset));
        }
        OpeningEditorGroup::Leaf => {
            col = col.push(profile_picker(form, library, OpeningProfilePart::LeafFrame));
            if !profile_is_valid(form, library, OpeningProfilePart::LeafFrame) {
                col = col
                    .push(dim_row("opening-leaf-face", "leaf_face", form.leaf_face))
                    .push(dim_row("opening-leaf-depth", "leaf_depth", form.leaf_depth));
            }
            col = col
                .push(dim_row("opening-leaf-rebate", "leaf_rebate", form.leaf_rebate))
                .push(dim_row("opening-leaf-lateral", "leaf_lateral", form.leaf_lateral));
        }
        OpeningEditorGroup::Glazing => {
            col = col
                .push(dim_row("opening-glazing-thickness", "glazing_thickness", form.glazing_thickness))
                .push(dim_row("opening-glazing-inset", "glazing_inset", form.glazing_inset));
        }
        OpeningEditorGroup::ExteriorSill => {
            col = col.push(profile_picker(form, library, OpeningProfilePart::ExteriorSill));
            if !profile_is_valid(form, library, OpeningProfilePart::ExteriorSill) {
                col = col.push(dim_row(
                    "opening-ext-sill-thickness",
                    "ext_sill_thickness",
                    form.ext_sill_thickness,
                ));
            }
            col = col
                .push(dim_row("opening-ext-sill-overhang", "ext_sill_overhang", form.ext_sill_overhang))
                .push(dim_row("opening-ext-sill-upstand", "ext_sill_upstand", form.ext_sill_upstand))
                .push(dim_row("opening-ext-sill-side-left", "ext_sill_side_left", form.ext_sill_side_left))
                .push(dim_row("opening-ext-sill-side-right", "ext_sill_side_right", form.ext_sill_side_right));
        }
        OpeningEditorGroup::InteriorBoard => {
            col = col.push(profile_picker(form, library, OpeningProfilePart::InteriorBoard));
            if !profile_is_valid(form, library, OpeningProfilePart::InteriorBoard) {
                col = col.push(dim_row(
                    "opening-int-board-thickness",
                    "int_board_thickness",
                    form.int_board_thickness,
                ));
            }
            col = col
                .push(dim_row("opening-int-board-overhang", "int_board_overhang", form.int_board_overhang))
                .push(dim_row("opening-int-board-upstand", "int_board_upstand", form.int_board_upstand))
                .push(dim_row("opening-int-board-side-left", "int_board_side_left", form.int_board_side_left))
                .push(dim_row("opening-int-board-side-right", "int_board_side_right", form.int_board_side_right));
        }
        OpeningEditorGroup::Threshold => {
            col = col.push(profile_picker(form, library, OpeningProfilePart::Threshold));
            if !profile_is_valid(form, library, OpeningProfilePart::Threshold) {
                col = col
                    .push(dim_row("opening-threshold-height", "threshold_height", form.threshold_height))
                    .push(dim_row("opening-threshold-width", "threshold_width", form.threshold_width));
            }
        }
        OpeningEditorGroup::ShutterBox => {
            col = col.push(profile_picker(form, library, OpeningProfilePart::ShutterBox));
            if !profile_is_valid(form, library, OpeningProfilePart::ShutterBox) {
                col = col
                    .push(dim_row("opening-shutter-height", "shutter_height", form.shutter_height))
                    .push(dim_row("opening-shutter-depth", "shutter_depth", form.shutter_depth));
            }
            col = col
                .push(dim_row("opening-shutter-seat", "shutter_seat", form.shutter_seat))
                .push(dim_row("opening-shutter-install", "shutter_install", form.shutter_install));
        }
        OpeningEditorGroup::Lintel => {
            col = col.push(profile_picker(form, library, OpeningProfilePart::Lintel));
            if !profile_is_valid(form, library, OpeningProfilePart::Lintel) {
                col = col.push(dim_row("opening-lintel-height", "lintel_height", form.lintel_height));
            }
            col = col
                .push(dim_row("opening-lintel-left", "lintel_left", form.lintel_left))
                .push(dim_row("opening-lintel-right", "lintel_right", form.lintel_right));
        }
        OpeningEditorGroup::Divisions => {}
        OpeningEditorGroup::Symbol => col = col.push(symbol_tab(form)),
        OpeningEditorGroup::Ratings => {}
    }

    for role in opening_group_roles(group) {
        if let Some(buffer) = form.look_buffers.get(role) {
            col = col
                .push(Space::new().height(4))
                .push(text(opening_label("opening-graphics")).size(11).style(muted))
                .push(graphics_row(form, *role, buffer))
                .push(text(opening_label("opening-materials")).size(11).style(muted))
                .push(material_row(form, library, *role, buffer));
        }
    }
    col.into()
}

pub(super) fn graphics_row<'a>(
    form: &OpeningStyleFormState<'a>,
    role: OpeningLookRole,
    buffer: &'a AecOpeningLookBuffer,
) -> Element<'a, Message> {
    let line_color = crate::modules::aec::ui::aec_ui_util::editor_string_to_acad_color(
        &buffer.line_color,
    )
    .unwrap_or(acadrust::types::Color::ByLayer);
    let color_field = container(crate::ui::color_select::color_selector(
        line_color,
        form.look_color_picker_role == Some(role),
        crate::ui::color_select::ColorExtras::default(),
        move |color| {
            Message::Aec(AecMessage::AecOpeningStyleManagerLookLineColorChanged(
                role,
                crate::modules::aec::ui::aec_ui_util::acad_color_to_editor_string(color),
            ))
        },
        Message::Aec(AecMessage::AecOpeningStyleManagerLookColorPickerToggle(role)),
        Message::OpenColorWindow(
            crate::app::ColorPickTarget::Aec(
                crate::modules::aec::ui::color_pick::AecColorPickTarget::OpeningComponent(role),
            ),
            line_color,
        ),
    ))
    .width(160);

    let fields = row![
        text(opening_look_role_label(role)).size(10).style(muted).width(130),
        crate::modules::aec::ui::aec_ui_util::linetype_field(
            &buffer.line_type,
            form.linetype_items,
            form.linetype_combo,
            move |value| Message::Aec(AecMessage::AecOpeningStyleManagerLookLineTypeChanged(role, value)),
        ),
        color_field,
    ]
    .spacing(6)
    .align_y(iced::Center);
    if form.selected_look_role == Some(role) {
        container(fields)
            .padding(4)
            .style(|theme: &Theme| container::Style {
                background: Some(
                    theme
                        .palette()
                        .primary
                        .base
                        .color
                        .scale_alpha(0.16)
                        .into(),
                ),
                border: Border {
                    color: theme.palette().primary.base.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            })
            .into()
    } else {
        fields.into()
    }
}

pub(super) fn material_row<'a>(
    form: &OpeningStyleFormState<'a>,
    library: &StyleLibrary,
    role: OpeningLookRole,
    buffer: &'a AecOpeningLookBuffer,
) -> Element<'a, Message> {
    let material_name = library
        .materials
        .iter()
        .find(|material| material.id == buffer.material_id)
        .map(|material| material.name.as_str())
        .unwrap_or_else(|| {
            if buffer.material_id.is_empty() {
                ""
            } else {
                buffer.material_id.as_str()
            }
        });
    let mut fields = row![
        text(opening_look_role_label(role)).size(10).style(muted).width(130),
        button(text(if material_name.is_empty() {
            opening_label("opening-material-inherit")
        } else {
            material_name.to_string()
        }).size(11))
            .padding([3, 8])
            .on_press(Message::Aec(AecMessage::AecStylePickerOpen(
                StylePickerTarget::OpeningComponentMaterial(role),
            ))),
    ]
    .spacing(6)
    .align_y(iced::Center);
    if !buffer.material_id.is_empty() {
        fields = fields.push(
            button(text("×").size(11))
                .style(button::subtle)
                .padding([2, 6])
                .on_press(Message::Aec(
                    AecMessage::AecOpeningStyleManagerLookMaterialClear(role),
                )),
        );
    }
    if form.selected_look_role == Some(role) {
        container(fields)
            .padding(4)
            .style(|theme: &Theme| container::Style {
                background: Some(
                    theme
                        .palette()
                        .primary
                        .base
                        .color
                        .scale_alpha(0.16)
                        .into(),
                ),
                border: Border {
                    color: theme.palette().primary.base.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            })
            .into()
    } else {
        fields.into()
    }
}

