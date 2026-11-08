use iced::mouse;
use iced::widget::canvas::{self, Frame, Path, Program, Stroke};
use iced::widget::{
    button, canvas as canvas_widget, column, container, pick_list, row, scrollable, text,
    text_input,
};
use iced::{Color, Element, Fill, Point, Rectangle, Theme};

use crate::app::{AecMessage, Message};
use crate::modules::aec::engine::library::{
    LibrarySource, StyleLibrary, combined_section_profile_entries_with_session,
};
use crate::modules::aec::engine::project::ProjectFile;
use crate::modules::aec::engine::section_profile::{SectionProfile, SectionShape, resolve_section};
use crate::modules::aec::engine::style_common::StyleOrigin;
use crate::modules::aec::state::{AecSectionProfilePointBuffer, AecSectionProfileShape};
use crate::modules::aec::ui::aec_ui_util::{muted, section_title};
use crate::modules::aec::ui::style_manager::StyleParts;
use crate::modules::aec::ui::style_manager::list::{self, ListEntry, ListSpec, filter_entries};
use crate::t;
use crate::tr;

pub struct SectionProfileFormState<'a> {
    pub open: bool,
    pub is_new: bool,
    pub name: &'a str,
    pub shape: AecSectionProfileShape,
    pub face: &'a str,
    pub depth: &'a str,
    pub diameter: &'a str,
    pub base: &'a str,
    pub height: &'a str,
    pub points: &'a [AecSectionProfilePointBuffer],
}

pub fn parts<'a>(
    _library: &'a StyleLibrary,
    project: Option<&'a ProjectFile>,
    session: Option<&'a StyleLibrary>,
    selected_id: Option<&'a str>,
    filter: &'a str,
    form: SectionProfileFormState<'a>,
) -> StyleParts<'a> {
    let entries = combined_section_profile_entries_with_session(project, session);
    let all: Vec<(ListEntry, LibrarySource)> = entries
        .iter()
        .map(|entry| {
            let profile = &entry.profile;
            let shape = match profile.shape {
                SectionShape::Rectangle { .. } => tr!("aec", "section-profile-shape-rectangle"),
                SectionShape::Circle { .. } => tr!("aec", "section-profile-shape-circle"),
                SectionShape::Triangle { .. } => tr!("aec", "section-profile-shape-triangle"),
                SectionShape::Custom { .. } => tr!("aec", "section-profile-shape-custom"),
            };
            (
                ListEntry {
                    id: profile.style.id.clone(),
                    label: profile.style.name.clone(),
                    detail: Some(shape),
                    origin: StyleOrigin::from(entry.source),
                    group: None,
                    swatch: None,
                },
                entry.source,
            )
        })
        .collect();
    let rows = filter_entries(all.iter().map(|(entry, _)| entry.clone()).collect(), filter);

    let list = list::view(
        ListSpec {
            title: tr!("aec", "styles-node-profiles"),
            search_hint: tr!("aec", "section-profile-search"),
            new_label: tr!("aec", "section-profile-new"),
            filter,
            selected: selected_id,
            on_filter: |value| Message::Aec(AecMessage::AecSectionProfileFilter(value)),
            on_select: |id| Message::Aec(AecMessage::AecSectionProfileSelect(id)),
            on_new: Message::Aec(AecMessage::AecSectionProfileNew),
        },
        &rows,
    );

    let open = form.open;
    let is_new = form.is_new;
    let detail = if open {
        form_view(form)
    } else {
        container(text(t!("Select a section profile to edit or create a new one.")).style(muted))
            .width(Fill)
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill)
            .into()
    };
    let actions = open.then(|| {
        row![
            button(text(t!("Save")).size(11))
                .style(iced::widget::button::primary)
                .padding([5, 12])
                .on_press(Message::Aec(AecMessage::AecSectionProfileSave)),
            (!is_new).then(|| {
                button(text(t!("Duplicate")).size(11))
                    .padding([5, 12])
                    .on_press(Message::Aec(AecMessage::AecSectionProfileDuplicate))
            }),
            (!is_new).then(|| {
                button(text(t!("Delete")).size(11))
                    .style(iced::widget::button::danger)
                    .padding([5, 12])
                    .on_press(Message::Aec(AecMessage::AecSectionProfileDelete))
            }),
        ]
        .spacing(8)
        .into()
    });
    StyleParts {
        list,
        form: detail,
        actions,
    }
}

fn form_view<'a>(form: SectionProfileFormState<'a>) -> Element<'a, Message> {
    let shapes = vec![
        (
            AecSectionProfileShape::Rectangle,
            tr!("aec", "section-profile-shape-rectangle"),
        ),
        (
            AecSectionProfileShape::Circle,
            tr!("aec", "section-profile-shape-circle"),
        ),
        (
            AecSectionProfileShape::Triangle,
            tr!("aec", "section-profile-shape-triangle"),
        ),
        (
            AecSectionProfileShape::Custom,
            tr!("aec", "section-profile-shape-custom"),
        ),
    ];
    let selected_shape = shapes
        .iter()
        .find(|(shape, _)| *shape == form.shape)
        .cloned();
    let shape_picker = pick_list(
        selected_shape,
        shapes,
        |(_, label): &(AecSectionProfileShape, String)| label.clone(),
    )
    .on_select(|(shape, _)| Message::Aec(AecMessage::AecSectionProfileShapeChanged(shape)))
    .text_size(11)
    .width(220);

    let mut fields = column![
        row![
            text(t!("Name")).size(10).style(muted).width(100),
            text_input("", form.name)
                .on_input(|value| Message::Aec(AecMessage::AecSectionProfileNameChanged(value)))
                .size(11)
                .padding([4, 6]),
        ]
        .spacing(8),
        row![
            text(tr!("aec", "section-profile-shape"))
                .size(10)
                .style(muted)
                .width(100),
            shape_picker,
        ]
        .spacing(8),
    ]
    .spacing(8);

    fields = match form.shape {
        AecSectionProfileShape::Rectangle => fields
            .push(dimension_row(
                tr!("aec", "section-profile-face"),
                "section-profile-face",
                form.face,
            ))
            .push(dimension_row(
                tr!("aec", "section-profile-depth"),
                "section-profile-depth",
                form.depth,
            )),
        AecSectionProfileShape::Circle => fields.push(dimension_row(
            tr!("aec", "section-profile-diameter"),
            "section-profile-diameter",
            form.diameter,
        )),
        AecSectionProfileShape::Triangle => fields
            .push(dimension_row(
                tr!("aec", "section-profile-base"),
                "section-profile-base",
                form.base,
            ))
            .push(dimension_row(
                tr!("aec", "section-profile-height"),
                "section-profile-height",
                form.height,
            )),
        AecSectionProfileShape::Custom => {
            let mut rows = column![text(tr!("aec", "section-profile-points")).size(11)];
            for (index, point) in form.points.iter().enumerate() {
                rows = rows.push(
                    row![
                        text("x").size(10),
                        text_input("0.000", &point.x)
                            .on_input(move |value| Message::Aec(
                                AecMessage::AecSectionProfilePointXChanged(index, value)
                            ))
                            .size(11)
                            .padding([4, 6])
                            .width(100),
                        text("y").size(10),
                        text_input("0.000", &point.y)
                            .on_input(move |value| Message::Aec(
                                AecMessage::AecSectionProfilePointYChanged(index, value)
                            ))
                            .size(11)
                            .padding([4, 6])
                            .width(100),
                        button(text("×").size(11))
                            .padding([4, 8])
                            .on_press(Message::Aec(AecMessage::AecSectionProfilePointRemove(
                                index
                            ))),
                    ]
                    .spacing(6),
                );
            }
            fields.push(rows.spacing(5)).push(
                button(text(tr!("aec", "section-profile-point-add")).size(11))
                    .padding([4, 8])
                    .on_press(Message::Aec(AecMessage::AecSectionProfilePointAdd)),
            )
        }
    };

    let preview = preview_outline(&form);
    let body = column![
        section_title(std::borrow::Cow::Owned(tr!(
            "aec",
            "section-profile-preview"
        ))),
        canvas_widget(SectionPreview { outline: preview })
            .width(Fill)
            .height(220),
    ]
    .spacing(8);
    column![
        scrollable(fields).height(Fill).width(Fill),
        container(body).padding(12).width(Fill).height(250),
    ]
    .spacing(12)
    .height(Fill)
    .into()
}

fn dimension_row<'a>(label: String, field: &'static str, value: &'a str) -> Element<'a, Message> {
    row![
        text(label).size(10).style(muted).width(100),
        text_input("0.070", value)
            .on_input(
                move |value| Message::Aec(AecMessage::AecSectionProfileDimensionChanged(
                    field.to_string(),
                    value,
                ))
            )
            .size(11)
            .padding([4, 6])
            .width(180),
    ]
    .spacing(8)
    .into()
}

fn preview_outline(form: &SectionProfileFormState<'_>) -> Vec<(f64, f64)> {
    let parse = |value: &str| value.trim().parse::<f64>().ok();
    let shape = match form.shape {
        AecSectionProfileShape::Rectangle => SectionShape::Rectangle {
            face: parse(form.face).unwrap_or(0.0),
            depth: parse(form.depth).unwrap_or(0.0),
        },
        AecSectionProfileShape::Circle => SectionShape::Circle {
            diameter: parse(form.diameter).unwrap_or(0.0),
        },
        AecSectionProfileShape::Triangle => SectionShape::Triangle {
            base: parse(form.base).unwrap_or(0.0),
            height: parse(form.height).unwrap_or(0.0),
        },
        AecSectionProfileShape::Custom => SectionShape::Custom {
            points: form
                .points
                .iter()
                .filter_map(|point| Some((parse(&point.x)?, parse(&point.y)?)))
                .collect(),
        },
    };
    let profile = SectionProfile::new("preview", "preview", shape);
    if profile.is_valid() {
        resolve_section(Some(&profile), 0.0, 0.0).outline
    } else {
        Vec::new()
    }
}

struct SectionPreview {
    outline: Vec<(f64, f64)>,
}

impl<Message> Program<Message, Theme> for SectionPreview {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        if self.outline.len() < 4 {
            return vec![frame.into_geometry()];
        }
        let face = self.outline.iter().map(|point| point.0).fold(0.0, f64::max);
        let depth = self.outline.iter().map(|point| point.1).fold(0.0, f64::max);
        let scale = ((bounds.width - 32.0) / face.max(1e-6) as f32)
            .min((bounds.height - 32.0) / depth.max(1e-6) as f32)
            .max(1.0);
        let origin = Point::new(
            (bounds.width - face as f32 * scale) * 0.5,
            (bounds.height + depth as f32 * scale) * 0.5,
        );
        let path = Path::new(|builder| {
            for (index, (x, y)) in self.outline.iter().enumerate() {
                let point = Point::new(origin.x + *x as f32 * scale, origin.y - *y as f32 * scale);
                if index == 0 {
                    builder.move_to(point);
                } else {
                    builder.line_to(point);
                }
            }
            builder.close();
        });
        frame.fill(&path, Color::from_rgba8(90, 150, 210, 0.18));
        frame.stroke(
            &path,
            Stroke::default()
                .with_color(Color::from_rgb8(60, 110, 170))
                .with_width(2.0),
        );
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_profile_catalog_entry_occurs_once_in_the_list_data() {
        let project = ProjectFile::default();
        let library = StyleLibrary::empty();
        let entries = combined_section_profile_entries_with_session(Some(&project), Some(&library));
        let mut ids: Vec<_> = entries
            .iter()
            .map(|entry| entry.profile.style.id.as_str())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), entries.len());
    }
}
