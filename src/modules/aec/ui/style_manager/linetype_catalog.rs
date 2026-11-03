use crate::app::OpenCADStudio;
use crate::ui::properties::LinetypeItem;

pub(crate) fn items(document: &codec::Document, retained_names: &[String]) -> Vec<LinetypeItem> {
    let mut items: Vec<_> = document
        .line_types
        .iter()
        .map(|line_type| {
            let name = if line_type.name.is_empty() {
                "ByLayer".to_string()
            } else {
                line_type.name.clone()
            };
            LinetypeItem {
                name,
                art: crate::io::linetypes::extract_pattern(&line_type.description),
            }
        })
        .collect();

    items.extend(
        crate::io::linetypes::catalog_missing_from(document)
            .into_iter()
            .map(|(name, description)| LinetypeItem {
                name,
                art: crate::io::linetypes::extract_pattern(&description),
            }),
    );

    for name in retained_names.iter().filter(|name| !name.trim().is_empty()) {
        if !items
            .iter()
            .any(|item| item.name.eq_ignore_ascii_case(name))
        {
            items.push(LinetypeItem {
                name: name.clone(),
                art: String::new(),
            });
        }
    }

    if !items
        .iter()
        .any(|item| item.name.eq_ignore_ascii_case("Continuous"))
    {
        items.push(LinetypeItem {
            name: "Continuous".to_string(),
            art: String::new(),
        });
    }

    items
}

impl OpenCADStudio {
    pub(crate) fn refresh_aec_material_linetype_combo(&mut self) {
        let retained_names = [
            self.aec.styles.material_line_type.clone(),
            self.aec.styles.plan_demolition_style_line_type.clone(),
            self.aec.styles.plan_existing_style_line_type.clone(),
            self.aec.styles.plan_overlay_line_type.clone(),
            self.aec.styles.profile_slot_style_line_type.clone(),
        ];
        let items = self
            .tabs
            .get(self.active_tab)
            .filter(|tab| !tab.is_start)
            .map(|tab| items(&tab.scene.document, &retained_names))
            .unwrap_or_default();

        self.aec.styles.material_linetype_combo =
            iced::widget::combo_box::State::new(items.clone());
        self.aec.styles.material_linetype_items = items;
    }

    pub(crate) fn ensure_aec_catalog_linetype(&mut self, name: &str) {
        let Some(tab) = self
            .tabs
            .get_mut(self.active_tab)
            .filter(|tab| !tab.is_start)
        else {
            return;
        };
        if crate::io::linetypes::ensure_catalog_linetype(&mut tab.scene.document, name) {
            tab.scene.bump_geometry();
            tab.dirty = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OpenCADStudio, items};
    use crate::app::AecMessage;
    use crate::modules::aec::ui::style_manager::StylesNode;

    fn app_with_drawing() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    #[test]
    fn includes_missing_catalog_linetypes_and_keeps_each_name_once_after_loading() {
        let mut document = codec::Document::new();
        let name = crate::io::linetypes::catalog_missing_from(&document)
            .into_iter()
            .find(|(name, _)| !name.eq_ignore_ascii_case("Continuous"))
            .map(|(name, _)| name)
            .expect("a bundled linetype should be missing from a new drawing");

        let listed = items(&document, std::slice::from_ref(&name));
        assert_eq!(
            listed
                .iter()
                .filter(|item| item.name.eq_ignore_ascii_case(&name))
                .count(),
            1
        );
        assert!(crate::io::linetypes::ensure_catalog_linetype(
            &mut document,
            &name
        ));

        let refreshed = items(&document, &[]);
        assert_eq!(
            refreshed
                .iter()
                .filter(|item| item.name.eq_ignore_ascii_case(&name))
                .count(),
            1
        );
    }

    #[test]
    fn preserves_a_stored_custom_linetype_name() {
        let document = codec::Document::new();
        let custom = "Legacy-AEC-Linetype".to_string();
        let listed = items(&document, std::slice::from_ref(&custom));
        assert!(listed.iter().any(|item| item.name == custom));
    }

    #[test]
    fn style_manager_lists_and_loads_a_missing_catalog_linetype() {
        let mut app = app_with_drawing();
        let tab_index = app.active_tab;
        let name = crate::io::linetypes::catalog_missing_from(&codec::Document::new())
            .into_iter()
            .find(|(name, _)| !name.eq_ignore_ascii_case("Continuous"))
            .map(|(name, _)| name)
            .expect("a bundled linetype should be missing from a new drawing");
        app.tabs[tab_index].scene.document.line_types.remove(&name);

        let _ = app.update_aec(AecMessage::AecStylesOpen(StylesNode::Wall));
        assert!(
            app.aec
                .styles
                .material_linetype_items
                .iter()
                .any(|item| item.name.eq_ignore_ascii_case(&name))
        );

        let _ = app.update_aec(AecMessage::AecStyleManagerProfileSlotStyleLineTypeChanged(
            name.clone(),
        ));
        assert!(
            app.tabs[tab_index]
                .scene
                .document
                .line_types
                .contains(&name)
        );
        assert!(app.tabs[tab_index].dirty);
    }
}
