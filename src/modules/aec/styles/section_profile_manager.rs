//! Section-profile catalog editor and persistence.

use iced::Task;

use crate::app::{Message, OpenCADStudio};
use crate::modules::IconKind;
use crate::modules::aec::engine::library::{
    LibrarySource, StyleLibrary, combined_section_profile_entries_with_session, load_or_seed,
    save_to_default_path,
};
use crate::modules::aec::engine::section_profile::{SectionProfile, SectionShape};
use crate::modules::aec::engine::style_common::CATEGORY_SECTION_PROFILE;
use crate::modules::aec::state::{AecSectionProfilePointBuffer, AecSectionProfileShape};

pub fn tool() -> crate::modules::ToolDef {
    crate::modules::ToolDef {
        id: "AEC_SECTIONPROFILEMANAGER",
        label: "Profile",
        icon: IconKind::Svg(include_bytes!(
            "../../../../assets/icons/aec/style_opening.svg"
        )),
        event: crate::modules::ModuleEvent::Command("AEC_SECTIONPROFILEMANAGER".to_string()),
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["AEC_SECTIONPROFILEMANAGER"]
});

impl OpenCADStudio {
    pub(crate) fn aec_section_profile_parts(
        &self,
    ) -> crate::modules::aec::ui::style_manager::StyleParts<'_> {
        let form = crate::modules::aec::ui::style_manager::forms::section_profile::SectionProfileFormState {
            open: self.aec.styles.section_profile_form_open,
            is_new: self.aec.styles.section_profile_editing_id.is_none(),
            name: &self.aec.styles.section_profile_name,
            shape: self.aec.styles.section_profile_shape,
            face: &self.aec.styles.section_profile_face,
            depth: &self.aec.styles.section_profile_depth,
            diameter: &self.aec.styles.section_profile_diameter,
            base: &self.aec.styles.section_profile_base,
            height: &self.aec.styles.section_profile_height,
            points: &self.aec.styles.section_profile_points,
        };
        crate::modules::aec::ui::style_manager::forms::section_profile::parts(
            self.aec
                .aec_style_library
                .as_ref()
                .unwrap_or(&EMPTY_STYLE_LIBRARY),
            self.aec.aec_project_explorer_file.as_ref(),
            self.aec.aec_session_style_library.as_ref(),
            self.aec.styles.section_profile_selected.as_deref(),
            &self.aec.styles.section_profile_filter,
            form,
        )
    }

    pub(crate) fn aec_section_profile_manager_open(&mut self) -> Task<Message> {
        self.ribbon.close_dropdown();
        self.aec_refresh_combined_style_library();
        self.aec.styles.node = crate::modules::aec::ui::style_manager::StylesNode::Profiles;
        self.active_modal = Some(crate::app::ModalKind::Aec(crate::app::AecModalKind::Styles));
        let id = self
            .aec
            .styles
            .section_profile_selected
            .clone()
            .or_else(|| {
                self.aec.aec_style_library.as_ref().and_then(|library| {
                    library
                        .profiles
                        .first()
                        .map(|profile| profile.style.id.clone())
                })
            });
        if let Some(id) = id {
            let _ = self.aec_section_profile_select(id);
        } else {
            let _ = self.aec_section_profile_new();
        }
        Task::none()
    }

    pub(crate) fn aec_section_profile_select(&mut self, id: String) -> Task<Message> {
        let profile = self
            .aec
            .aec_style_library
            .as_ref()
            .and_then(|library| library.find_section_profile(&id))
            .cloned();
        let Some(profile) = profile else {
            return Task::none();
        };
        self.aec.styles.section_profile_selected = Some(id.clone());
        self.aec.styles.section_profile_editing_id = Some(id);
        self.aec.styles.section_profile_form_open = true;
        self.aec.styles.section_profile_name = profile.style.name;
        match profile.shape {
            SectionShape::Rectangle { face, depth } => {
                self.aec.styles.section_profile_shape = AecSectionProfileShape::Rectangle;
                self.aec.styles.section_profile_face = format_dim(face);
                self.aec.styles.section_profile_depth = format_dim(depth);
            }
            SectionShape::Circle { diameter } => {
                self.aec.styles.section_profile_shape = AecSectionProfileShape::Circle;
                self.aec.styles.section_profile_diameter = format_dim(diameter);
            }
            SectionShape::Triangle { base, height } => {
                self.aec.styles.section_profile_shape = AecSectionProfileShape::Triangle;
                self.aec.styles.section_profile_base = format_dim(base);
                self.aec.styles.section_profile_height = format_dim(height);
            }
            SectionShape::Custom { points } => {
                self.aec.styles.section_profile_shape = AecSectionProfileShape::Custom;
                let mut points = points;
                if points.len() > 1 && points.first() == points.last() {
                    points.pop();
                }
                self.aec.styles.section_profile_points = points
                    .into_iter()
                    .map(|(x, y)| AecSectionProfilePointBuffer {
                        x: format_dim(x),
                        y: format_dim(y),
                    })
                    .collect();
                if self.aec.styles.section_profile_points.len() < 3 {
                    self.aec.styles.section_profile_points = vec![
                        AecSectionProfilePointBuffer {
                            x: "0".into(),
                            y: "0".into(),
                        },
                        AecSectionProfilePointBuffer {
                            x: "0.07".into(),
                            y: "0".into(),
                        },
                        AecSectionProfilePointBuffer {
                            x: "0.07".into(),
                            y: "0.04".into(),
                        },
                    ];
                }
            }
        }
        Task::none()
    }

    pub(crate) fn aec_section_profile_new(&mut self) -> Task<Message> {
        self.aec.styles.section_profile_selected = None;
        self.aec.styles.section_profile_editing_id = None;
        self.aec.styles.section_profile_form_open = true;
        self.aec.styles.section_profile_name.clear();
        self.aec.styles.section_profile_shape = AecSectionProfileShape::Rectangle;
        self.aec.styles.section_profile_face = "0.07".into();
        self.aec.styles.section_profile_depth = "0.08".into();
        self.aec.styles.section_profile_diameter = "0.04".into();
        self.aec.styles.section_profile_base = "0.06".into();
        self.aec.styles.section_profile_height = "0.05".into();
        self.aec.styles.section_profile_points = vec![
            AecSectionProfilePointBuffer {
                x: "0".into(),
                y: "0".into(),
            },
            AecSectionProfilePointBuffer {
                x: "0.07".into(),
                y: "0".into(),
            },
            AecSectionProfilePointBuffer {
                x: "0.07".into(),
                y: "0.04".into(),
            },
        ];
        Task::none()
    }

    pub(crate) fn aec_section_profile_name_changed(&mut self, value: String) -> Task<Message> {
        self.aec.styles.section_profile_name = value;
        Task::none()
    }

    pub(crate) fn aec_section_profile_shape_changed(
        &mut self,
        shape: AecSectionProfileShape,
    ) -> Task<Message> {
        self.aec.styles.section_profile_shape = shape;
        if shape == AecSectionProfileShape::Custom
            && self.aec.styles.section_profile_points.is_empty()
        {
            self.aec.styles.section_profile_points = vec![
                AecSectionProfilePointBuffer {
                    x: "0".into(),
                    y: "0".into(),
                },
                AecSectionProfilePointBuffer {
                    x: "0.07".into(),
                    y: "0".into(),
                },
                AecSectionProfilePointBuffer {
                    x: "0.07".into(),
                    y: "0.04".into(),
                },
            ];
        }
        Task::none()
    }

    pub(crate) fn aec_section_profile_dimension_changed(&mut self, field: &str, value: String) {
        match field {
            "section-profile-face" => self.aec.styles.section_profile_face = value,
            "section-profile-depth" => self.aec.styles.section_profile_depth = value,
            "section-profile-diameter" => self.aec.styles.section_profile_diameter = value,
            "section-profile-base" => self.aec.styles.section_profile_base = value,
            "section-profile-height" => self.aec.styles.section_profile_height = value,
            _ => {}
        }
    }

    pub(crate) fn aec_section_profile_point_add(&mut self) {
        self.aec
            .styles
            .section_profile_points
            .push(AecSectionProfilePointBuffer {
                x: "0.000".into(),
                y: "0.000".into(),
            });
    }

    pub(crate) fn aec_section_profile_point_remove(&mut self, index: usize) {
        if index < self.aec.styles.section_profile_points.len() {
            self.aec.styles.section_profile_points.remove(index);
        }
    }

    pub(crate) fn aec_section_profile_point_changed(
        &mut self,
        index: usize,
        is_x: bool,
        value: String,
    ) {
        if let Some(point) = self.aec.styles.section_profile_points.get_mut(index) {
            if is_x {
                point.x = value;
            } else {
                point.y = value;
            }
        }
    }

    pub(crate) fn aec_section_profile_save(&mut self) -> Task<Message> {
        let name = self.aec.styles.section_profile_name.trim().to_string();
        if name.is_empty() {
            self.command_line
                .push_error(crate::t!("Section profile name cannot be empty").as_ref());
            return Task::none();
        }
        let Some(shape) = self.aec_section_profile_shape_from_buffers() else {
            self.command_line.push_error(crate::t!("Section profile dimensions must be positive and custom profiles need at least three valid points").as_ref());
            return Task::none();
        };
        let id = self
            .aec
            .styles
            .section_profile_editing_id
            .clone()
            .unwrap_or_else(|| format!("section_{}", uuid::Uuid::new_v4().simple()));
        let mut profile = SectionProfile::new(id.clone(), name, shape);
        profile.style.category = CATEGORY_SECTION_PROFILE.to_string();
        if !profile.close_custom_contour() {
            self.command_line
                .push_error(crate::t!("Section profile is invalid").as_ref());
            return Task::none();
        }

        let source = self.aec_section_profile_source(&id);
        let save_to_project = source == Some(LibrarySource::Project)
            || (source != Some(LibrarySource::Session)
                && self.aec.aec_project_explorer_file.is_some());
        let result = if save_to_project {
            self.aec_section_profile_upsert_project(profile.clone())
        } else {
            self.aec_section_profile_upsert_session(profile.clone());
            Ok(())
        };
        if let Err(error) = result {
            self.command_line.push_error(&error);
            return Task::none();
        }

        self.aec_refresh_combined_style_library();
        self.aec.styles.section_profile_selected = Some(id.clone());
        self.aec.styles.section_profile_editing_id = Some(id.clone());
        self.aec.styles.section_profile_form_open = true;
        self.command_line
            .push_info(crate::tf!("Section profile '{}' saved.", profile.style.name).as_ref());
        let _ = self.aec_section_profile_select(id);
        Task::none()
    }

    pub(crate) fn aec_section_profile_duplicate(&mut self) -> Task<Message> {
        if self.aec.styles.section_profile_editing_id.is_none() {
            return Task::none();
        }
        self.aec.styles.section_profile_name =
            format!("{} (Copy)", self.aec.styles.section_profile_name);
        self.aec.styles.section_profile_selected = None;
        self.aec.styles.section_profile_editing_id = None;
        self.aec.styles.section_profile_form_open = true;
        Task::none()
    }

    pub(crate) fn aec_section_profile_delete(&mut self) -> Task<Message> {
        let Some(id) = self.aec.styles.section_profile_selected.clone() else {
            return Task::none();
        };
        let source = self.aec_section_profile_source(&id);
        let removed = match source {
            Some(LibrarySource::Project) => self.aec_section_profile_remove_project(&id),
            Some(LibrarySource::Session) => Some(
                self.aec_edit_session_style_library(self.active_tab, |library| {
                    library
                        .as_mut()
                        .is_some_and(|library| library.remove_section_profile(&id))
                })
                .unwrap_or(false),
            ),
            Some(LibrarySource::Standard) => {
                let mut library = load_or_seed();
                if library.remove_section_profile(&id) {
                    match save_to_default_path(&library) {
                        Ok(()) => Some(true),
                        Err(error) => {
                            self.command_line.push_error(&error);
                            Some(false)
                        }
                    }
                } else {
                    Some(false)
                }
            }
            None => None,
        };
        if removed == Some(true) {
            self.aec_refresh_combined_style_library();
            self.aec.styles.section_profile_selected = None;
            self.aec.styles.section_profile_editing_id = None;
            self.aec.styles.section_profile_form_open = false;
            self.command_line
                .push_info(crate::tf!("Section profile '{id}' deleted.").as_ref());
            if let Some(next) = self.aec.aec_style_library.as_ref().and_then(|library| {
                library
                    .profiles
                    .first()
                    .map(|profile| profile.style.id.clone())
            }) {
                let _ = self.aec_section_profile_select(next);
            }
        } else {
            self.command_line
                .push_error(crate::t!("Section profile could not be deleted").as_ref());
        }
        Task::none()
    }

    fn aec_section_profile_shape_from_buffers(&self) -> Option<SectionShape> {
        let parse =
            |value: &str| crate::modules::aec::styles::opening_style_manager::parse_dim(value);
        Some(match self.aec.styles.section_profile_shape {
            AecSectionProfileShape::Rectangle => SectionShape::Rectangle {
                face: parse(&self.aec.styles.section_profile_face)?,
                depth: parse(&self.aec.styles.section_profile_depth)?,
            },
            AecSectionProfileShape::Circle => SectionShape::Circle {
                diameter: parse(&self.aec.styles.section_profile_diameter)?,
            },
            AecSectionProfileShape::Triangle => SectionShape::Triangle {
                base: parse(&self.aec.styles.section_profile_base)?,
                height: parse(&self.aec.styles.section_profile_height)?,
            },
            AecSectionProfileShape::Custom => SectionShape::Custom {
                points: self
                    .aec
                    .styles
                    .section_profile_points
                    .iter()
                    .map(|point| Some((parse(&point.x)?, parse(&point.y)?)))
                    .collect::<Option<Vec<_>>>()?,
            },
        })
    }

    fn aec_section_profile_source(&self, id: &str) -> Option<LibrarySource> {
        combined_section_profile_entries_with_session(
            self.aec.aec_project_explorer_file.as_ref(),
            self.aec.aec_session_style_library.as_ref(),
        )
        .into_iter()
        .find(|entry| entry.profile.style.id == id)
        .map(|entry| entry.source)
    }

    fn aec_section_profile_upsert_project(
        &mut self,
        profile: SectionProfile,
    ) -> Result<(), String> {
        let path = self.aec.aec_project_explorer_path.clone();
        let project = self
            .aec
            .aec_project_explorer_file
            .as_mut()
            .ok_or_else(|| crate::t!("No project is loaded").into_owned())?;
        if !project
            .material_wall_style_library
            .upsert_section_profile(profile)
        {
            return Err(crate::t!("Section profile is invalid").into_owned());
        }
        if let Some(path) = path {
            let library = project.material_wall_style_library.clone();
            crate::modules::aec::engine::project::save_style_library_to_project(
                project, &path, library,
            )
            .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn aec_section_profile_upsert_session(&mut self, profile: SectionProfile) {
        self.aec_edit_session_style_library(self.active_tab, |library| {
            library
                .get_or_insert_with(StyleLibrary::empty)
                .upsert_section_profile(profile);
        });
    }

    fn aec_section_profile_remove_project(&mut self, id: &str) -> Option<bool> {
        let path = self.aec.aec_project_explorer_path.clone();
        let project = self.aec.aec_project_explorer_file.as_mut()?;
        let removed = project
            .material_wall_style_library
            .remove_section_profile(id);
        if removed {
            if let Some(path) = path {
                let library = project.material_wall_style_library.clone();
                if let Err(error) =
                    crate::modules::aec::engine::project::save_style_library_to_project(
                        project, &path, library,
                    )
                {
                    self.command_line.push_error(&error.to_string());
                }
            }
        }
        Some(removed)
    }
}

fn format_dim(value: f64) -> String {
    crate::modules::aec::styles::opening_style_manager::format_dim(value)
}

static EMPTY_STYLE_LIBRARY: StyleLibrary = StyleLibrary {
    materials: Vec::new(),
    wall_styles: Vec::new(),
    opening_styles: Vec::new(),
    slab_styles: Vec::new(),
    slab_structural_styles: Vec::new(),
    floor_finish_styles: Vec::new(),
    profiles: Vec::new(),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_custom_profile_closes_contour_and_writes_session_overlay() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let _ = app.aec_section_profile_new();
        app.aec.styles.section_profile_name = "Custom frame".into();
        app.aec.styles.section_profile_shape = AecSectionProfileShape::Custom;
        app.aec.styles.section_profile_points = vec![
            AecSectionProfilePointBuffer {
                x: "0".into(),
                y: "0".into(),
            },
            AecSectionProfilePointBuffer {
                x: "0.07".into(),
                y: "0".into(),
            },
            AecSectionProfilePointBuffer {
                x: "0.07".into(),
                y: "0.04".into(),
            },
        ];
        let _ = app.aec_section_profile_save();

        let id = app
            .aec
            .styles
            .section_profile_selected
            .as_deref()
            .expect("saved profile is selected");
        let profile = app
            .aec
            .aec_style_library
            .as_ref()
            .and_then(|library| library.find_section_profile(id))
            .expect("session profile is merged");
        assert_eq!(profile.style.name, "Custom frame");
        let SectionShape::Custom { points } = &profile.shape else {
            panic!("expected custom contour");
        };
        assert_eq!(points.first(), points.last());
    }

    #[test]
    fn invalid_custom_profile_is_not_saved() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let _ = app.aec_section_profile_new();
        app.aec.styles.section_profile_name = "Invalid frame".into();
        app.aec.styles.section_profile_shape = AecSectionProfileShape::Custom;
        app.aec.styles.section_profile_points.truncate(2);
        let _ = app.aec_section_profile_save();

        assert!(app.aec.styles.section_profile_selected.is_none());
        assert!(
            app.aec
                .aec_style_library
                .as_ref()
                .is_none_or(|library| library.find_section_profile("Invalid frame").is_none())
        );
    }
}
