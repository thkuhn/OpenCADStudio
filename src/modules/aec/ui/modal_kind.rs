//! Nested AEC modal kinds. Core keeps a single `ModalKind::Aec` arm.

use super::super::state::StylePickerTarget;

/// AEC in-canvas dialogs. New windows add variants here, not on Core `ModalKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AecModalKind {
    DropWarning,
    MaterialManager,
    WallStyleManager,
    OpeningStyleManager,
    WallStyleDisplayProfiles,
    JunctionEditor,
    ProjectExplorer,
    StoreySettings,
    PlanManager,
    StylePicker { target: StylePickerTarget },
    StyleCopyConflict,
    ProjectRequired,
}

impl AecModalKind {
    pub fn title(&self) -> String {
        match self {
            Self::DropWarning => crate::tr!("modal", "save-warning"),
            Self::MaterialManager => crate::t!("AEC Material Manager").into_owned(),
            Self::WallStyleManager => crate::t!("AEC Wall Style Manager").into_owned(),
            Self::OpeningStyleManager => crate::t!("AEC Opening Style Manager").into_owned(),
            Self::WallStyleDisplayProfiles => crate::t!("Display Profiles").into_owned(),
            Self::JunctionEditor => crate::t!("Junction Editor").into_owned(),
            Self::ProjectExplorer => crate::t!("AEC Project Explorer").into_owned(),
            Self::StoreySettings => crate::t!("Storey settings").into_owned(),
            Self::PlanManager => crate::t!("AEC Plan Manager").into_owned(),
            Self::StylePicker { .. } => crate::t!("Style Picker").into_owned(),
            Self::StyleCopyConflict => crate::t!("Overwrite?").into_owned(),
            Self::ProjectRequired => crate::t!("Project Required").into_owned(),
        }
    }

    /// Fixed max size for `sized_flow`. `None` uses `automatic_flow`.
    pub fn size(&self) -> Option<(u16, u16)> {
        match self {
            Self::MaterialManager => Some((900, 560)),
            Self::WallStyleManager => Some((960, 640)),
            Self::OpeningStyleManager => Some((980, 680)),
            Self::WallStyleDisplayProfiles => Some((760, 540)),
            Self::JunctionEditor => Some((720, 520)),
            Self::ProjectExplorer => Some((860, 560)),
            Self::StoreySettings => Some((720, 620)),
            Self::PlanManager => Some((960, 640)),
            Self::StylePicker { .. } => Some((520, 480)),
            Self::DropWarning | Self::StyleCopyConflict | Self::ProjectRequired => None,
        }
    }

    /// Child modal restore target (Display Profiles → Wall Style Manager).
    pub fn parent_on_close(&self) -> Option<Self> {
        match self {
            Self::WallStyleDisplayProfiles => Some(Self::WallStyleManager),
            _ => None,
        }
    }

    pub fn clears_project_resume_on_close(&self) -> bool {
        matches!(self, Self::ProjectRequired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_profiles_restore_wall_style_manager() {
        assert_eq!(
            AecModalKind::WallStyleDisplayProfiles.parent_on_close(),
            Some(AecModalKind::WallStyleManager)
        );
        assert_eq!(AecModalKind::WallStyleManager.parent_on_close(), None);
    }

    #[test]
    fn drop_warning_is_automatic_flow() {
        assert_eq!(AecModalKind::DropWarning.size(), None);
        assert!(AecModalKind::MaterialManager.size().is_some());
    }
}
