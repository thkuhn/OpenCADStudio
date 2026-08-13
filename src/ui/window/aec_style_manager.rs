//! AEC Style Manager — browse the materials and wall styles held in the AEC
//! style library (`AEC_STYLEMANAGER`).
//!
//! This shows a two-pane master-list view (filterable materials + wall
//! styles on the left, a detail placeholder on the right) built on top of
//! the state loaded by `AEC_STYLEMANAGER` into `App.aec_style_library`.
//! Actual editing of the selected material/wall style is left to a later
//! step; this one only wires up selection and filtering so that step can
//! render/edit directly from this state instead of introducing parallel
//! storage.

use iced::widget::{
    button, column, container, pick_list, row, scrollable, text, text_input, Space,
};
use iced::{Border, Element, Fill, Theme};

use crate::app::Message;
use crate::modules::aec::engine::library::StyleLibrary;
use crate::modules::aec::engine::material::Material;
use crate::modules::aec::engine::wall_style::WallStyle;
use crate::t;

fn muted(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.65)),
    }
}

fn list_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        if selected {
            button::primary(theme, status)
        } else {
            button::subtle(theme, status)
        }
    }
}

fn section_title<'a>(label: std::borrow::Cow<'a, str>) -> Element<'a, Message> {
    container(text(label).size(11).style(muted))
        .padding([4, 2])
        .into()
}

fn material_row<'a>(material: &'a Material, selected: bool) -> Element<'a, Message> {
    button(
        column![
            text(material.name.as_str()).size(12),
            text(material.hatch_pattern.as_str()).size(10).style(muted),
        ]
        .spacing(2),
    )
    .on_press(Message::AecStyleManagerSelectMaterial(material.id.clone()))
    .style(list_style(selected))
    .padding([6, 9])
    .width(Fill)
    .into()
}

fn wall_style_row<'a>(
    wall_style: &'a WallStyle,
    parent_name: Option<&'a str>,
    selected: bool,
) -> Element<'a, Message> {
    let subtitle = match parent_name {
        Some(parent) => crate::tf!("Based on {parent}", parent = parent).into_owned(),
        None => crate::tf!(
            "{count} layer(s)",
            count = wall_style.layers.len()
        )
        .into_owned(),
    };
    button(
        column![
            text(wall_style.style.name.as_str()).size(12),
            text(subtitle).size(10).style(muted),
        ]
        .spacing(2),
    )
    .on_press(Message::AecStyleManagerSelectWallStyle(
        wall_style.style.id.clone(),
    ))
    .style(list_style(selected))
    .padding([6, 9])
    .width(Fill)
    .into()
}

/// Edit-buffer fields for the material form, owned by `App` and borrowed
/// here for rendering; kept as a single struct so `view_window`'s signature
/// stays manageable as more per-entity forms are added.
pub struct MaterialFormState<'a> {
    /// Whether the form should be shown at all (a material is selected, or
    /// "New" was pressed).
    pub open: bool,
    /// `true` while composing a not-yet-saved material (no id assigned yet).
    pub is_new: bool,
    pub name: &'a str,
    pub hatch: &'a str,
    pub color: &'a str,
    pub line_type: &'a str,
}

/// Edit-buffer fields for the wall-style form, owned by `App` and borrowed
/// here for rendering.
pub struct WallStyleFormState<'a> {
    pub open: bool,
    pub is_new: bool,
    pub name: &'a str,
    pub parent_id: Option<&'a str>,
    pub layers: &'a [crate::app::AecLayerBuffer],
    /// All other wall styles (id, name) for the parent picklist, excluding self.
    pub all_wall_styles: Vec<(&'a str, &'a str)>,
    /// All materials (id, name) for the layer picklist.
    pub all_materials: Vec<(&'a str, &'a str)>,
    /// Resolved inheritance result for preview.
    pub effective_layers: Vec<crate::modules::aec::engine::wall_style::Layer>,
}

fn layer_row<'a>(
    index: usize,
    layer_count: usize,
    buffer: &'a crate::app::AecLayerBuffer,
    all_materials: &[(&'a str, &'a str)],
) -> Element<'a, Message> {
    let functions: Vec<String> = vec![
        "Structural".to_string(),
        "Insulation".to_string(),
        "Finish".to_string(),
        "Other".to_string(),
    ];

    let material_ids: Vec<String> = all_materials.iter().map(|(id, _)| id.to_string()).collect();
    let material_names = all_materials.to_vec();
    let selected_material_id = if buffer.material_id.is_empty() {
        None
    } else {
        Some(buffer.material_id.clone())
    };

    let mut up_button = button(text("▲").size(10)).padding([2, 6]);
    if index > 0 {
        up_button = up_button.on_press(Message::AecStyleManagerWallStyleLayerMoveUp(index));
    }
    let mut down_button = button(text("▼").size(10)).padding([2, 6]);
    if index + 1 < layer_count {
        down_button = down_button.on_press(Message::AecStyleManagerWallStyleLayerMoveDown(index));
    }

    row![
        column![up_button, down_button].spacing(2),
        pick_list(selected_material_id, material_ids, move |id: &String| {
            material_names
                .iter()
                .find(|(mid, _)| mid == id)
                .map(|(_, name)| name.to_string())
                .unwrap_or_else(|| id.clone())
        })
        .on_select(move |id| Message::AecStyleManagerWallStyleLayerMaterialChanged(index, id))
        .text_size(11)
        .width(150),
        text_input("", &buffer.thickness)
            .on_input(move |v| Message::AecStyleManagerWallStyleLayerThicknessChanged(index, v))
            .size(11)
            .width(60),
        pick_list(Some(buffer.function.clone()), functions, |func: &String| {
            func.clone()
        })
        .on_select(move |func| Message::AecStyleManagerWallStyleLayerFunctionChanged(index, func))
        .text_size(11)
        .width(100),
        button(text(t!("Remove")).size(10))
            .style(button::danger)
            .padding([2, 4])
            .on_press(Message::AecStyleManagerWallStyleLayerRemove(index)),
    ]
    .spacing(4)
    .into()
}

/// Orders wall styles either alphabetically by name, or hierarchically —
/// roots first (styles with no parent, or whose parent id doesn't resolve),
/// each followed immediately by its descendants, siblings sorted by name at
/// every level.
fn ordered_wall_styles<'a>(
    library: &'a StyleLibrary,
    sort: crate::app::AecWallStyleSort,
) -> Vec<&'a WallStyle> {
    if sort == crate::app::AecWallStyleSort::Name {
        let mut styles: Vec<&'a WallStyle> = library.wall_styles.iter().collect();
        styles.sort_by(|a, b| a.style.name.to_lowercase().cmp(&b.style.name.to_lowercase()));
        return styles;
    }

    let mut children: std::collections::HashMap<&str, Vec<&'a WallStyle>> =
        std::collections::HashMap::new();
    let mut roots: Vec<&'a WallStyle> = Vec::new();
    for ws in &library.wall_styles {
        match &ws.style.parent_style_id {
            Some(pid) if library.wall_styles.iter().any(|other| &other.style.id == pid) => {
                children.entry(pid.as_str()).or_default().push(ws);
            }
            _ => roots.push(ws),
        }
    }
    roots.sort_by(|a, b| a.style.name.to_lowercase().cmp(&b.style.name.to_lowercase()));
    for siblings in children.values_mut() {
        siblings.sort_by(|a, b| a.style.name.to_lowercase().cmp(&b.style.name.to_lowercase()));
    }

    let mut ordered = Vec::with_capacity(library.wall_styles.len());
    let mut stack: Vec<&'a WallStyle> = roots.into_iter().rev().collect();
    while let Some(ws) = stack.pop() {
        ordered.push(ws);
        if let Some(kids) = children.get(ws.style.id.as_str()) {
            for kid in kids.iter().rev() {
                stack.push(kid);
            }
        }
    }
    ordered
}

/// Renders the two-pane manager view for the given library, or a placeholder
/// message if none has been loaded yet.
pub fn view_window<'a>(
    library: Option<&'a StyleLibrary>,
    filter: &'a str,
    selected_material: Option<&'a str>,
    selected_wall_style: Option<&'a str>,
    material_form: MaterialFormState<'a>,
    wall_style_form: WallStyleFormState<'a>,
    wall_style_sort: crate::app::AecWallStyleSort,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let Some(library) = library else {
        return container(text(t!("No style library loaded.")).size(12))
            .padding(16)
            .width(Fill)
            .into();
    };

    let query = filter.trim().to_lowercase();

    let material_rows: Vec<Element<'_, Message>> = library
        .materials
        .iter()
        .filter(|material| query.is_empty() || material.name.to_lowercase().contains(&query))
        .map(|material| {
            let is_selected = selected_material == Some(material.id.as_str());
            material_row(material, is_selected)
        })
        .collect();

    let wall_style_rows: Vec<Element<'_, Message>> = ordered_wall_styles(library, wall_style_sort)
        .into_iter()
        .filter(|ws| query.is_empty() || ws.style.name.to_lowercase().contains(&query))
        .map(|wall_style| {
            let is_selected = selected_wall_style == Some(wall_style.style.id.as_str());
            let parent_name = wall_style.style.parent_style_id.as_ref().and_then(|pid| {
                library
                    .wall_styles
                    .iter()
                    .find(|other| &other.style.id == pid)
                    .map(|other| other.style.name.as_str())
            });
            wall_style_row(wall_style, parent_name, is_selected)
        })
        .collect();

    let no_matches = || -> Element<'static, Message> {
        text(t!("No matches.")).size(10).style(muted).into()
    };

    let mut master_list = column![section_title(t!("Materials"))].spacing(2);
    master_list = if material_rows.is_empty() {
        master_list.push(no_matches())
    } else {
        master_list.push(column(material_rows).spacing(2))
    };

    let wall_styles_header = row![
        container(text(t!("Wall Styles")).size(11).style(muted)).padding([4, 2]),
        Space::new().width(Fill),
        button(
            text(crate::tf!("Sort: {mode}", mode = wall_style_sort.label()).into_owned())
                .size(10)
        )
        .style(button::subtle)
        .padding([2, 6])
        .on_press(Message::AecStyleManagerWallStyleSortToggle),
    ]
    .spacing(4)
    .align_y(iced::alignment::Vertical::Center);

    master_list = master_list.push(Space::new().height(8)).push(wall_styles_header);
    master_list = if wall_style_rows.is_empty() {
        master_list.push(no_matches())
    } else {
        master_list.push(column(wall_style_rows).spacing(2))
    };

    let left = container(
        column![
            row![
                text_input(t!("Search materials and wall styles…").as_ref(), filter)
                    .on_input(Message::AecStyleManagerFilter)
                    .size(11)
                    .padding([5, 8]),
                button(text(t!("+Mat")).size(11))
                    .style(button::subtle)
                    .padding([5, 9])
                    .on_press(Message::AecStyleManagerMaterialNew),
                button(text(t!("+Wall")).size(11))
                    .style(button::subtle)
                    .padding([5, 9])
                    .on_press(Message::AecStyleManagerWallStyleNew),
            ]
            .spacing(6),
            container(scrollable(master_list).height(sizing.height))
                .width(280)
                .height(sizing.height)
                .padding(3)
                .style(|theme: &Theme| container::Style {
                    border: Border {
                        color: theme.palette().background.neutral.color,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                }),
        ]
        .spacing(8)
        .height(sizing.height),
    )
    .width(300)
    .height(sizing.height)
    .padding(iced::Padding {
        top: 12.0,
        right: 8.0,
        bottom: 12.0,
        left: 12.0,
    });

    let selected_wall_style = selected_wall_style
        .and_then(|id| library.wall_styles.iter().find(|w| w.style.id == id));

    let details: Element<'_, Message> = if material_form.open {
        let form_title = if material_form.is_new {
            t!("New Material")
        } else {
            t!("Material")
        };
        let mut actions = row![button(text(t!("Save")).size(11))
            .style(button::primary)
            .padding([5, 12])
            .on_press(Message::AecStyleManagerMaterialSave)]
        .spacing(8);
        if !material_form.is_new {
            actions = actions.push(
                button(text(t!("Delete")).size(11))
                    .style(button::danger)
                    .padding([5, 12])
                    .on_press(Message::AecStyleManagerMaterialDelete),
            );
        }
        column![
            text(form_title).size(13),
            row![
                text(t!("Name")).size(10).style(muted).width(100),
                text_input("", material_form.name)
                    .on_input(Message::AecStyleManagerMaterialNameChanged)
                    .size(11)
                    .padding([4, 6]),
            ]
            .spacing(8),
            row![
                text(t!("Hatch pattern")).size(10).style(muted).width(100),
                text_input("", material_form.hatch)
                    .on_input(Message::AecStyleManagerMaterialHatchChanged)
                    .size(11)
                    .padding([4, 6]),
            ]
            .spacing(8),
            row![
                text(t!("Line color (hex)")).size(10).style(muted).width(100),
                text_input("#RRGGBB", material_form.color)
                    .on_input(Message::AecStyleManagerMaterialColorChanged)
                    .size(11)
                    .padding([4, 6]),
            ]
            .spacing(8),
            row![
                text(t!("Line type")).size(10).style(muted).width(100),
                text_input("", material_form.line_type)
                    .on_input(Message::AecStyleManagerMaterialLineTypeChanged)
                    .size(11)
                    .padding([4, 6]),
            ]
            .spacing(8),
            actions,
        ]
        .spacing(7)
        .into()
    } else if wall_style_form.open {
        let form_title = if wall_style_form.is_new {
            t!("New Wall Style")
        } else {
            t!("Wall Style")
        };
        let mut actions = row![button(text(t!("Save")).size(11))
            .style(button::primary)
            .padding([5, 12])
            .on_press(Message::AecStyleManagerWallStyleSave)]
        .spacing(8);
        if !wall_style_form.is_new {
            actions = actions.push(
                button(text(t!("Delete")).size(11))
                    .style(button::danger)
                    .padding([5, 12])
                    .on_press(Message::AecStyleManagerWallStyleDelete),
            );
        }

        let none_label = t!("(None)");
        let parent_options: Vec<String> = std::iter::once(none_label.clone().into_owned())
            .chain(
                wall_style_form
                    .all_wall_styles
                    .iter()
                    .map(|(_, name)| name.to_string()),
            )
            .collect();
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
        let layer_count = wall_style_form.layers.len();
        let layer_rows: Vec<Element<'_, Message>> = wall_style_form
            .layers
            .iter()
            .enumerate()
            .map(|(i, lb)| layer_row(i, layer_count, lb, all_materials_ref))
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
                pick_list(Some(selected_parent), parent_options, |name: &String| {
                    name.clone()
                })
                .on_select(move |name| {
                    let none_label = t!("(None)");
                    let id = if name == none_label {
                        None
                    } else {
                        wall_style_form
                            .all_wall_styles
                            .iter()
                            .find(|(_, n)| n == &name)
                            .map(|(id, _)| id.to_string())
                    };
                    Message::AecStyleManagerWallStyleParentChanged(id)
                })
                .text_size(11)
                .width(Fill),
            ]
            .spacing(8),
            row![
                text(t!("Layers")).size(10).style(muted),
                Space::new(),
                button(text(t!("Add Layer")).size(10))
                    .on_press(Message::AecStyleManagerWallStyleLayerAdd)
                    .padding([2, 8]),
            ]
            .spacing(8),
            container(scrollable(column(layer_rows).spacing(4)).height(150))
                .style(|theme: &Theme| container::Style {
                    border: Border {
                        color: theme.palette().background.neutral.color,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                })
                .padding(4),
        ]
        .spacing(7);

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
                        .find(|(id, _)| id == &l.material_id)
                        .map(|(_, n)| n.to_string())
                        .unwrap_or_else(|| l.material_id.clone());
                    row![
                        text(mat_name).size(10).width(150),
                        text(format!("{:.2}", l.thickness)).size(10).width(60),
                        text(format!("{:?}", l.function)).size(10).width(100),
                    ]
                    .spacing(4)
                    .into()
                })
                .collect();
            detail_col = detail_col
                .push(container(column(preview_rows).spacing(2)).padding(4));
        }

        detail_col.push(Space::new().height(8)).push(actions).into()
    } else if let Some(wall_style) = selected_wall_style {
        column![
            text(t!("Wall Style")).size(13),
            row![
                text(t!("Name")).size(10).style(muted).width(100),
                text(wall_style.style.name.as_str()).size(11),
            ]
            .spacing(8),
            row![
                text(t!("Layers")).size(10).style(muted).width(100),
                text(wall_style.layers.len().to_string()).size(11),
            ]
            .spacing(8),
            text(t!("Editing is not available yet."))
                .size(10)
                .style(muted),
        ]
        .spacing(7)
        .into()
    } else {
        column![
            text(t!("No selection")).size(13),
            text(t!("Choose a material or wall style on the left to see its details."))
                .size(11)
                .style(muted),
        ]
        .spacing(7)
        .into()
    };

    let right = container(details)
        .width(sizing.width)
        .height(sizing.height)
        .padding([12, 12]);

    container(row![left, right].height(sizing.height))
        .width(sizing.width)
        .height(sizing.height)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AecWallStyleSort;
    use crate::modules::aec::engine::style::Style;

    fn wall_style(id: &str, name: &str, parent: Option<&str>) -> WallStyle {
        WallStyle {
            style: Style {
                id: id.to_string(),
                name: name.to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: parent.map(|p| p.to_string()),
            },
            layers: Vec::new(),
        }
    }

    #[test]
    fn name_sort_is_alphabetical_case_insensitive() {
        let lib = StyleLibrary {
            materials: Vec::new(),
            wall_styles: vec![
                wall_style("c", "charlie", None),
                wall_style("a", "Alpha", None),
                wall_style("b", "bravo", None),
            ],
        };
        let names: Vec<&str> = ordered_wall_styles(&lib, AecWallStyleSort::Name)
            .iter()
            .map(|ws| ws.style.name.as_str())
            .collect();
        assert_eq!(names, vec!["Alpha", "bravo", "charlie"]);
    }

    #[test]
    fn hierarchy_sort_groups_children_directly_after_their_parent() {
        let lib = StyleLibrary {
            materials: Vec::new(),
            wall_styles: vec![
                wall_style("child_b", "Child B", Some("root")),
                wall_style("root2", "Root Z", None),
                wall_style("root", "Root A", None),
                wall_style("child_a", "Child A", Some("root")),
                wall_style("grandchild", "Grandchild", Some("child_a")),
            ],
        };
        let ids: Vec<&str> = ordered_wall_styles(&lib, AecWallStyleSort::Hierarchy)
            .iter()
            .map(|ws| ws.style.id.as_str())
            .collect();
        // Roots sorted by name (Root A before Root Z); "root"'s children
        // (sorted by name: Child A, Child B) follow immediately, with
        // "child_a"'s own child ("grandchild") nested right after it.
        assert_eq!(
            ids,
            vec!["root", "child_a", "grandchild", "child_b", "root2"]
        );
    }

    #[test]
    fn hierarchy_sort_treats_dangling_parent_reference_as_root() {
        let lib = StyleLibrary {
            materials: Vec::new(),
            wall_styles: vec![wall_style("orphan", "Orphan", Some("missing_parent"))],
        };
        let ids: Vec<&str> = ordered_wall_styles(&lib, AecWallStyleSort::Hierarchy)
            .iter()
            .map(|ws| ws.style.id.as_str())
            .collect();
        assert_eq!(ids, vec!["orphan"]);
    }
}
