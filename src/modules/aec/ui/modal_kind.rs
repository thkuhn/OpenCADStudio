//! Nested AEC modal kinds. Core keeps a single `ModalKind::Aec` arm.

use super::super::state::StylePickerTarget;

/// AEC in-canvas dialogs. New windows add variants here, not on Core `ModalKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AecModalKind {
    DropWarning,
    /// Unified style manager (`AEC_STYLES`).
    Styles,
    WallStyleDisplayProfiles,
    JunctionEditor,
    ProjectExplorer,
    StoreySettings,
    StylePicker { target: StylePickerTarget },
    StyleCopyConflict,
    ProjectRequired,
}

impl AecModalKind {
    pub fn title(&self) -> String {
        match self {
            Self::DropWarning => crate::tr!("modal", "save-warning"),
            Self::Styles => crate::tr!("aec", "styles-title"),
            Self::WallStyleDisplayProfiles => crate::tr!("aec", "dialog-display-profiles"),
            Self::JunctionEditor => crate::tr!("aec", "dialog-junction-editor"),
            Self::ProjectExplorer => crate::tr!("aec", "dialog-project-explorer"),
            Self::StoreySettings => crate::tr!("aec", "dialog-storey-settings"),
            Self::StylePicker { .. } => crate::tr!("aec", "dialog-style-picker"),
            Self::StyleCopyConflict => crate::tr!("aec", "dialog-overwrite"),
            Self::ProjectRequired => crate::tr!("aec", "dialog-project-required"),
        }
    }

    /// Fixed max size for `sized_flow`. `None` uses `automatic_flow`.
    pub fn size(&self) -> Option<(u16, u16)> {
        match self {
            Self::Styles => Some((1280, 820)),
            Self::WallStyleDisplayProfiles => Some((760, 540)),
            Self::JunctionEditor => Some((720, 520)),
            Self::ProjectExplorer => Some((860, 560)),
            Self::StoreySettings => Some((720, 620)),
            Self::StylePicker { .. } => Some((520, 480)),
            Self::DropWarning | Self::StyleCopyConflict | Self::ProjectRequired => None,
        }
    }

    /// Child modal restore target (Display Profiles → Wall Style Manager).
    pub fn parent_on_close(&self) -> Option<Self> {
        match self {
            Self::WallStyleDisplayProfiles => Some(Self::Styles),
            _ => None,
        }
    }

    pub fn clears_project_resume_on_close(&self) -> bool {
        matches!(self, Self::ProjectRequired)
    }
}

impl crate::modules::aec::state::AecState {
    /// Parent modal and its saved geometry to restore when `kind` closes.
    pub(crate) fn take_parent_modal_on_close(
        &mut self,
        kind: AecModalKind,
    ) -> Option<(AecModalKind, iced::Vector, iced::Vector)> {
        let parent = kind.parent_on_close()?;
        let (offset, resize) = self.styles.wall_parent_geometry.take()?;
        Some((parent, offset, resize))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_profiles_restore_wall_style_manager() {
        assert_eq!(
            AecModalKind::WallStyleDisplayProfiles.parent_on_close(),
            Some(AecModalKind::Styles)
        );
        assert_eq!(AecModalKind::Styles.parent_on_close(), None);
    }

    #[test]
    fn dialog_title_keys_exist_in_every_locale() {
        const KEYS: [&str; 7] = [
            "dialog-display-profiles",
            "dialog-junction-editor",
            "dialog-project-explorer",
            "dialog-storey-settings",
            "dialog-style-picker",
            "dialog-overwrite",
            "dialog-project-required",
        ];
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
        let mut checked = 0;
        for entry in std::fs::read_dir(&root).unwrap() {
            let file = entry.unwrap().path().join("opencadstudio.ftl");
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let aec = text
                .split("\naec =\n")
                .nth(1)
                .unwrap_or_else(|| panic!("{}: missing aec group", file.display()));
            let group = aec.split("\n\n").next().unwrap_or(aec);
            let group = group
                .lines()
                .take_while(|l| l.starts_with(' '))
                .collect::<Vec<_>>()
                .join("\n");
            for key in KEYS {
                assert!(
                    group.contains(&format!("    .{key} = ")),
                    "{}: missing {key}",
                    file.display()
                );
            }
            checked += 1;
        }
        assert!(checked >= 23, "only {checked} locales checked");
    }

    #[test]
    fn drop_warning_is_automatic_flow() {
        assert_eq!(AecModalKind::DropWarning.size(), None);
        assert_eq!(AecModalKind::Styles.size(), Some((1280, 820)));
    }
}
