use super::*;
use iced::widget::row;

pub(super) fn opening_label(key: &str) -> String {
    match key {
        "opening-components" => tr!("aec", "opening-components"),
        "opening-component-disabled" => tr!("aec", "opening-component-disabled"),
        "opening-component-fixed-frame" => tr!("aec", "opening-component-fixed-frame"),
        "opening-component-leaf" => tr!("aec", "opening-component-leaf"),
        "opening-component-glazing" => tr!("aec", "opening-component-glazing"),
        "opening-component-exterior-sill" => tr!("aec", "opening-component-exterior-sill"),
        "opening-component-interior-board" => tr!("aec", "opening-component-interior-board"),
        "opening-component-threshold" => tr!("aec", "opening-component-threshold"),
        "opening-component-shutter-box" => tr!("aec", "opening-component-shutter-box"),
        "opening-component-lintel" => tr!("aec", "opening-component-lintel"),
        "opening-component-divisions" => tr!("aec", "opening-component-divisions"),
        "opening-component-symbol" => tr!("aec", "opening-component-symbol"),
        "opening-component-ratings" => tr!("aec", "opening-component-ratings"),
        "opening-parts" => tr!("aec", "opening-parts"),
        "opening-tab-dimensions" => tr!("aec", "opening-tab-dimensions"),
        "opening-tab-graphics" => tr!("aec", "opening-tab-graphics"),
        "opening-tab-materials" => tr!("aec", "opening-tab-materials"),
        "opening-tab-divisions" => tr!("aec", "opening-tab-divisions"),
        "opening-tab-symbol" => tr!("aec", "opening-tab-symbol"),
        "opening-graphics" => tr!("aec", "opening-graphics"),
        "opening-materials" => tr!("aec", "opening-materials"),
        "opening-material-inherit" => tr!("aec", "opening-material-inherit"),
        "opening-frame-face" => tr!("aec", "opening-frame-face"),
        "opening-frame-depth" => tr!("aec", "opening-frame-depth"),
        "opening-frame-offset" => tr!("aec", "opening-frame-offset"),
        "opening-leaf-face" => tr!("aec", "opening-leaf-face"),
        "opening-leaf-depth" => tr!("aec", "opening-leaf-depth"),
        "opening-leaf-rebate" => tr!("aec", "opening-leaf-rebate"),
        "opening-glazing-thickness" => tr!("aec", "opening-glazing-thickness"),
        "opening-glazing-inset" => tr!("aec", "opening-glazing-inset"),
        "opening-ext-sill-thickness" => tr!("aec", "opening-ext-sill-thickness"),
        "opening-ext-sill-overhang" => tr!("aec", "opening-ext-sill-overhang"),
        "opening-ext-sill-upstand" => tr!("aec", "opening-ext-sill-upstand"),
        "opening-int-board-thickness" => tr!("aec", "opening-int-board-thickness"),
        "opening-int-board-overhang" => tr!("aec", "opening-int-board-overhang"),
        "opening-int-board-upstand" => tr!("aec", "opening-int-board-upstand"),
        "opening-threshold-height" => tr!("aec", "opening-threshold-height"),
        "opening-threshold-width" => tr!("aec", "opening-threshold-width"),
        "opening-shutter-height" => tr!("aec", "opening-shutter-height"),
        "opening-shutter-depth" => tr!("aec", "opening-shutter-depth"),
        "opening-shutter-seat" => tr!("aec", "opening-shutter-seat"),
        "opening-shutter-install" => tr!("aec", "opening-shutter-install"),
        "opening-lintel-height" => tr!("aec", "opening-lintel-height"),
        "opening-lintel-left" => tr!("aec", "opening-lintel-left"),
        "opening-lintel-right" => tr!("aec", "opening-lintel-right"),
        "opening-grid" => tr!("aec", "opening-grid"),
        "opening-vertical" => tr!("aec", "opening-vertical"),
        "opening-horizontal" => tr!("aec", "opening-horizontal"),
        "opening-vertical-muntins" => tr!("aec", "opening-vertical-muntins"),
        "opening-horizontal-muntins" => tr!("aec", "opening-horizontal-muntins"),
        "opening-cell-kind" => tr!("aec", "opening-cell-kind"),
        "opening-cell-hinge" => tr!("aec", "opening-cell-hinge"),
        "opening-vertical-from-left" => tr!("aec", "opening-vertical-from-left"),
        "opening-horizontal-above-sill" => tr!("aec", "opening-horizontal-above-sill"),
        "opening-muntins-every-cell" => tr!("aec", "opening-muntins-every-cell"),
        "opening-vertical-muntin-in-cell" => tr!("aec", "opening-vertical-muntin-in-cell"),
        "opening-horizontal-muntin-above-sill" => {
            tr!("aec", "opening-horizontal-muntin-above-sill")
        }
        "opening-cell" => tr!("aec", "opening-cell"),
        "opening-cell-add" => tr!("aec", "opening-cell-add"),
        "opening-cell-remove" => tr!("aec", "opening-cell-remove"),
        "opening-family-blocks" => tr!("aec", "opening-family-blocks"),
        "opening-family-plan" => tr!("aec", "opening-family-plan"),
        "opening-family-section" => tr!("aec", "opening-family-section"),
        "opening-family-elevation" => tr!("aec", "opening-family-elevation"),
        "opening-block-name" => tr!("aec", "opening-block-name"),
        _ => key.to_string(),
    }
}

pub(super) fn opening_look_role_label(role: OpeningLookRole) -> String {
    match role {
        OpeningLookRole::FixedFrame => tr!("aec", "opening-role-fixed-frame"),
        OpeningLookRole::Leaf => tr!("aec", "opening-role-leaf"),
        OpeningLookRole::Glazing => tr!("aec", "opening-role-glazing"),
        OpeningLookRole::Swing => tr!("aec", "opening-role-swing"),
        OpeningLookRole::Division => tr!("aec", "opening-role-division"),
        OpeningLookRole::ExteriorSill => tr!("aec", "opening-role-exterior-sill"),
        OpeningLookRole::InteriorBoard => tr!("aec", "opening-role-interior-board"),
        OpeningLookRole::Threshold => tr!("aec", "opening-role-threshold"),
        OpeningLookRole::ShutterBox => tr!("aec", "opening-role-shutter-box"),
        OpeningLookRole::Lintel => tr!("aec", "opening-role-lintel"),
    }
}

pub(super) fn dim_row<'a>(label_key: &'static str, field: &'static str, value: &'a str) -> Element<'a, Message> {
    row![
        text(opening_label(label_key)).size(10).style(muted).width(140),
        text_input("", value)
            .on_input(move |v| Message::Aec(AecMessage::AecOpeningStyleManagerDimChanged(field.into(), v)))
            .size(11)
            .padding([2, 6]),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

pub(super) fn family_row<'a>(
    family: &'static str,
    label_key: &'static str,
    name: &'a str,
    placement: FamilyPlacement,
) -> Element<'a, Message> {
    let placements = vec!["Evaluated".to_string(), "JambPair".to_string()];
    row![
        text(opening_label(label_key)).size(10).style(muted).width(90),
        text_input(opening_label("opening-block-name").as_str(), name)
            .on_input(move |v| {
                Message::Aec(AecMessage::AecOpeningStyleManagerFamilyBlockChanged(family.into(), v))
            })
            .size(11)
            .padding([2, 6]),
        pick_list(Some(placement.as_str().to_string()), placements, |v: &String| v.clone())
            .on_select(move |v| {
                Message::Aec(AecMessage::AecOpeningStyleManagerFamilyPlacementChanged(family.into(), v))
            })
            .text_size(11)
            .width(110),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

