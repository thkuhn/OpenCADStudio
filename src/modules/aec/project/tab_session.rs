use crate::app::OpenCADStudio;
use crate::modules::aec::engine::display_component::RepresentationMode;
use crate::modules::aec::engine::library::StyleLibrary;
use crate::modules::aec::state::AecTabSession;

impl OpenCADStudio {
    fn aec_tab_session_id(&self, tab_index: usize) -> Option<u64> {
        self.tabs.get(tab_index).map(|tab| tab.id())
    }

    fn aec_prune_tab_session(&mut self, tab_id: u64) {
        if self.aec.tab_sessions.get(&tab_id).is_some_and(|session| {
            session.active_display_config.is_none()
                && session.representation_override.is_none()
                && session.session_style_library.is_none()
        }) {
            self.aec.tab_sessions.remove(&tab_id);
        }
    }

    pub(crate) fn aec_forget_tab_session(&mut self, tab_id: u64) {
        self.aec.tab_sessions.remove(&tab_id);
    }

    pub(crate) fn aec_tab_display_state(
        &self,
        tab_index: usize,
    ) -> (Option<String>, Option<RepresentationMode>) {
        let Some(tab_id) = self.aec_tab_session_id(tab_index) else {
            return (None, None);
        };
        self.aec
            .tab_sessions
            .get(&tab_id)
            .map(|session| {
                (
                    session.active_display_config.clone(),
                    session.representation_override,
                )
            })
            .unwrap_or_default()
    }

    pub(crate) fn aec_status_bar_display_state(
        &self,
        tab_index: usize,
    ) -> (
        Option<String>,
        Vec<String>,
        Option<RepresentationMode>,
        RepresentationMode,
    ) {
        use crate::modules::aec::engine::library::effective_representation;

        let (active_name, representation_override) = self.aec_tab_display_state(tab_index);
        let plan_names = self
            .aec
            .aec_plan_library
            .as_ref()
            .map(|library| {
                library
                    .configs
                    .iter()
                    .map(|config| config.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        let representation_mode = representation_override.unwrap_or_else(|| {
            active_name
                .as_deref()
                .and_then(|name| {
                    self.aec
                        .aec_plan_library
                        .as_ref()
                        .and_then(|library| library.find(name))
                })
                .map(|config| effective_representation(config, None))
                .unwrap_or(RepresentationMode::All)
        });
        (
            active_name,
            plan_names,
            representation_override,
            representation_mode,
        )
    }

    pub(crate) fn aec_set_active_display_config(
        &mut self,
        tab_index: usize,
        config: Option<String>,
    ) {
        let Some(tab_id) = self.aec_tab_session_id(tab_index) else {
            return;
        };
        self.aec
            .tab_sessions
            .entry(tab_id)
            .or_insert_with(AecTabSession::default)
            .active_display_config = config;
        self.aec_prune_tab_session(tab_id);
    }

    pub(crate) fn aec_set_representation_override(
        &mut self,
        tab_index: usize,
        mode: Option<RepresentationMode>,
    ) {
        let Some(tab_id) = self.aec_tab_session_id(tab_index) else {
            return;
        };
        self.aec
            .tab_sessions
            .entry(tab_id)
            .or_insert_with(AecTabSession::default)
            .representation_override = mode;
        self.aec_prune_tab_session(tab_id);
    }

    pub(crate) fn aec_session_style_library(&self, tab_index: usize) -> Option<&StyleLibrary> {
        let tab_id = self.aec_tab_session_id(tab_index)?;
        self.aec
            .tab_sessions
            .get(&tab_id)?
            .session_style_library
            .as_ref()
    }

    pub(crate) fn aec_edit_session_style_library<R>(
        &mut self,
        tab_index: usize,
        edit: impl FnOnce(&mut Option<StyleLibrary>) -> R,
    ) -> Option<R> {
        let tab_id = self.aec_tab_session_id(tab_index)?;
        let result = edit(
            &mut self
                .aec
                .tab_sessions
                .entry(tab_id)
                .or_default()
                .session_style_library,
        );
        self.aec_prune_tab_session(tab_id);
        Some(result)
    }

    pub(crate) fn aec_set_session_style_library(
        &mut self,
        tab_index: usize,
        library: Option<StyleLibrary>,
    ) {
        let Some(tab_id) = self.aec_tab_session_id(tab_index) else {
            return;
        };
        self.aec
            .tab_sessions
            .entry(tab_id)
            .or_default()
            .session_style_library = library;
        self.aec_prune_tab_session(tab_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_sessions_follow_stable_ids_and_refresh_active_style_overlay() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);

        let first = app.active_tab;
        let first_id = app.tabs[first].id();
        app.aec_set_active_display_config(first, Some("Plan A".into()));
        app.aec_set_representation_override(first, Some(RepresentationMode::TwoD));
        app.aec_set_session_style_library(first, Some(StyleLibrary::empty()));
        app.aec_refresh_combined_style_library();
        assert!(app.aec.aec_session_style_library.is_some());

        let _ = app.update(crate::app::Message::TabNew);
        let second = app.active_tab;
        let second_id = app.tabs[second].id();
        assert_ne!(first_id, second_id);
        assert_eq!(app.aec_tab_display_state(second), (None, None));
        assert!(app.aec.aec_session_style_library.is_none());

        app.aec_set_active_display_config(second, Some("Plan B".into()));
        app.aec_set_representation_override(second, Some(RepresentationMode::ThreeD));
        app.tabs.swap(first, second);
        app.active_tab = first;
        let _ = app.update(crate::app::Message::TabSwitch(first));

        assert_eq!(
            app.aec_tab_display_state(first),
            (Some("Plan B".into()), Some(RepresentationMode::ThreeD))
        );
        assert_eq!(
            app.aec_tab_display_state(second),
            (Some("Plan A".into()), Some(RepresentationMode::TwoD))
        );
        assert!(app.aec.aec_session_style_library.is_none());

        let _ = app.update(crate::app::Message::TabSwitch(second));
        assert!(app.aec.aec_session_style_library.is_some());
        app.aec_forget_tab_session(second_id);
        assert!(!app.aec.tab_sessions.contains_key(&second_id));
    }
}
