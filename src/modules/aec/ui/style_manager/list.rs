//! Shared filterable style list of `AEC_STYLES` (middle column).

use std::borrow::Cow;

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Background, Border, Color, Element, Fill, Theme};

use super::super::aec_ui_util::{list_style, muted, no_matches, section_title};
use crate::app::Message;
use crate::modules::aec::engine::style_common::StyleOrigin;
use crate::t;

/// One row of the list, independent of the style kind.
#[derive(Debug, Clone, PartialEq)]
pub struct ListEntry {
    pub id: String,
    pub label: String,
    /// Secondary line (e.g. hatch pattern).
    pub detail: Option<String>,
    pub origin: StyleOrigin,
    /// Group heading (e.g. material category); `None` = ungrouped.
    pub group: Option<String>,
    /// Colour swatch (0xRRGGBB).
    pub swatch: Option<u32>,
}

/// Messages and labels the list needs from the style kind.
pub struct ListSpec<'a> {
    pub title: String,
    pub search_hint: String,
    pub new_label: String,
    pub filter: &'a str,
    pub selected: Option<&'a str>,
    pub on_filter: fn(String) -> Message,
    pub on_select: fn(String) -> Message,
    pub on_new: Message,
}

/// Case-insensitive match on label, id and group; empty filter keeps all.
pub fn filter_entries(entries: Vec<ListEntry>, filter: &str) -> Vec<ListEntry> {
    let f = filter.trim().to_lowercase();
    if f.is_empty() {
        return entries;
    }
    entries
        .into_iter()
        .filter(|e| {
            e.label.to_lowercase().contains(&f)
                || e.id.to_lowercase().contains(&f)
                || e.group.as_deref().unwrap_or("").to_lowercase().contains(&f)
        })
        .collect()
}

/// Group names in display order (alphabetical, empty groups ignored).
pub fn group_names(entries: &[ListEntry]) -> Vec<String> {
    let mut g: Vec<String> = entries
        .iter()
        .filter_map(|e| e.group.as_deref().map(str::trim).filter(|c| !c.is_empty()))
        .map(str::to_string)
        .collect();
    g.sort_by_key(|a| a.to_lowercase());
    g.dedup();
    g
}

pub fn origin_label(origin: StyleOrigin) -> Cow<'static, str> {
    match origin {
        StyleOrigin::Default => t!("Standard"),
        StyleOrigin::Project => t!("Projekt"),
        StyleOrigin::Session => t!("Sitzung"),
    }
}

pub fn origin_badge<'a>(origin: StyleOrigin) -> Element<'a, Message> {
    container(text(origin_label(origin)).size(9))
        .padding([1, 5])
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.strong.color.scale_alpha(0.55),
            )),
            text_color: Some(theme.palette().background.base.text.scale_alpha(0.85)),
            border: Border { radius: 3.0.into(), ..Default::default() },
            ..Default::default()
        })
        .into()
}

fn entry_row<'a>(e: &ListEntry, selected: bool, on_select: fn(String) -> Message) -> Element<'a, Message> {
    let mut head = row![text(e.label.clone()).size(12), Space::new().width(6), origin_badge(e.origin)]
        .spacing(4)
        .align_y(iced::Center);
    if let Some(rgb) = e.swatch {
        let c = Color::from_rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
        head = row![
            container(Space::new())
                .width(10)
                .height(10)
                .style(move |theme: &Theme| container::Style {
                    background: Some(Background::Color(c)),
                    border: Border {
                        color: theme.palette().background.strong.color,
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                }),
            head
        ]
        .spacing(6)
        .align_y(iced::Center);
    }
    let mut body = column![head].spacing(2);
    if let Some(d) = &e.detail {
        body = body.push(text(d.clone()).size(10).style(muted));
    }
    button(body)
        .on_press(on_select(e.id.clone()))
        .style(list_style(selected))
        .padding([6, 9])
        .width(Fill)
        .into()
}

/// Search box + "new" button + grouped rows. `entries` must already be filtered.
pub fn view<'a>(spec: ListSpec<'a>, entries: &[ListEntry]) -> Element<'a, Message> {
    let mut list = column![section_title(Cow::Owned(spec.title.clone()))].spacing(2);
    if entries.is_empty() {
        list = list.push(no_matches());
    } else {
        let groups = group_names(entries);
        for g in &groups {
            list = list.push(section_title(Cow::Owned(g.clone())));
            for e in entries.iter().filter(|e| e.group.as_deref().map(str::trim) == Some(g.as_str())) {
                list = list.push(entry_row(e, spec.selected == Some(e.id.as_str()), spec.on_select));
            }
        }
        let rest: Vec<_> = entries
            .iter()
            .filter(|e| e.group.as_deref().map(str::trim).filter(|c| !c.is_empty()).is_none())
            .collect();
        if !rest.is_empty() {
            if !groups.is_empty() {
                list = list.push(section_title(t!("Ohne Kategorie")));
            }
            for e in rest {
                list = list.push(entry_row(e, spec.selected == Some(e.id.as_str()), spec.on_select));
            }
        }
    }
    let on_filter = spec.on_filter;
    column![
        row![
            text_input(&spec.search_hint, spec.filter).on_input(on_filter).size(11).padding([4, 6]),
            button(text("+").size(11)).on_press(spec.on_new.clone()).padding([4, 8]),
        ]
        .spacing(4),
        scrollable(list).height(Fill),
        button(text(spec.new_label).size(11))
            .width(Fill)
            .padding([4, 8])
            .style(button::primary)
            .on_press(spec.on_new),
    ]
    .spacing(8)
    .width(240)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(id: &str, label: &str, group: Option<&str>) -> ListEntry {
        ListEntry {
            id: id.into(),
            label: label.into(),
            detail: None,
            origin: StyleOrigin::Default,
            group: group.map(Into::into),
            swatch: None,
        }
    }

    #[test]
    fn filter_matches_label_id_group_case_insensitive() {
        let v = vec![e("a1", "Beton", Some("Mauerwerk")), e("b2", "Putz", None)];
        assert_eq!(filter_entries(v.clone(), "").len(), 2);
        assert_eq!(filter_entries(v.clone(), "BET").len(), 1);
        assert_eq!(filter_entries(v.clone(), "b2")[0].label, "Putz");
        assert_eq!(filter_entries(v.clone(), "mauer")[0].id, "a1");
        assert!(filter_entries(v, "zzz").is_empty());
    }

    #[test]
    fn group_names_sorted_unique_without_blank() {
        let v = vec![e("1", "a", Some("b")), e("2", "b", Some("A")), e("3", "c", Some("b")), e("4", "d", Some(" ")), e("5", "e", None)];
        assert_eq!(group_names(&v), vec!["A".to_string(), "b".to_string()]);
    }
}
