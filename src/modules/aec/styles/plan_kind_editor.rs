//! Plan kinds (`DisplayConfig`) as user data inside `AEC_STYLES`: open the
//! node, select / create / duplicate entries.

use iced::Task;

use crate::app::{AecModalKind, Message, ModalKind, OpenCADStudio};
use crate::modules::aec::engine::plan_view::{PlanningStage, ViewType};
use crate::modules::aec::ui::style_manager::StylesNode;

impl OpenCADStudio {
    /// Seed the plan-kind node (libraries, wall-style choices for the
    /// overrides) and show it in the `Styles` window.
    pub(crate) fn aec_plan_kinds_open(&mut self) -> Task<Message> {
        self.ribbon.close_dropdown();
        self.aec.aec_plan_library = Some(
            crate::modules::aec::engine::project::resolve_display_config_library(
                self.aec.aec_project_explorer_file.as_ref(),
            ),
        );
        self.aec.aec_style_library = Some(
            crate::modules::aec::engine::project::resolve_style_library(
                self.aec.aec_project_explorer_file.as_ref(),
            ),
        );
        self.aec.styles.plan_filter.clear();
        self.aec.styles.plan_selected = None;
        self.aec.styles.plan_editing_name = None;
        self.aec.styles.plan_form_open = false;
        self.refresh_aec_material_linetype_combo();
        self.aec.styles.plan_wall_styles = self
            .aec
            .aec_style_library
            .as_ref()
            .map(|lib| {
                lib.wall_styles
                    .iter()
                    .map(|ws| {
                        let layers = ws
                            .layers
                            .iter()
                            .enumerate()
                            .map(|(i, l)| {
                                let mat_name = lib
                                    .materials
                                    .iter()
                                    .find(|m| m.id == l.material_id)
                                    .map(|m| m.name.as_str())
                                    .unwrap_or(l.material_id.as_str());
                                (l.layer_id, format!("{} — {}", i + 1, mat_name))
                            })
                            .collect();
                        (ws.style.id.clone(), ws.style.name.clone(), layers)
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.aec.styles.node = StylesNode::PlanKinds;
        self.active_modal = Some(ModalKind::Aec(AecModalKind::Styles));
        Task::none()
    }

    pub(crate) fn aec_plan_kind_select(&mut self, name: String) -> Task<Message> {
        if let Some(cfg) = self
            .aec
            .aec_plan_library
            .as_ref()
            .and_then(|lib| lib.find(&name))
            .cloned()
        {
            self.aec.styles.plan_editing_name = Some(cfg.name.clone());
            self.aec.styles.plan_name = cfg.name.clone();
            self.aec.styles.plan_discipline = cfg.discipline.clone();
            self.aec.styles.plan_scale = cfg.scale.map(|s| s.to_string()).unwrap_or_default();
            self.aec.styles.plan_planning_stage = cfg.planning_stage;
            self.aec.styles.plan_view_type = cfg.view_type.clone();
            self.aec_plan_manager_load_phase_filter_buffers(cfg.phase_filter.as_ref());
            self.aec_plan_manager_load_display_buffers(&cfg);
            self.aec.styles.plan_form_open = true;
        }
        self.aec.styles.plan_selected = Some(name);
        Task::none()
    }

    pub(crate) fn aec_plan_kind_new(&mut self) -> Task<Message> {
        self.aec.styles.plan_selected = None;
        self.aec.styles.plan_editing_name = None;
        self.aec.styles.plan_name.clear();
        self.aec.styles.plan_discipline.clear();
        self.aec.styles.plan_scale.clear();
        self.aec.styles.plan_planning_stage = PlanningStage::Design;
        self.aec.styles.plan_view_type = ViewType::FloorPlan;
        self.aec_plan_manager_load_phase_filter_buffers(None);
        self.aec_plan_manager_reset_display_buffers();
        self.aec.styles.plan_form_open = true;
        Task::none()
    }

    /// Copy the selected plan kind into an unsaved new one (user data: no
    /// preset link, fresh id on save).
    pub(crate) fn aec_plan_kind_duplicate(&mut self) -> Task<Message> {
        let source_name = self
            .aec
            .styles
            .plan_editing_name
            .clone()
            .or_else(|| self.aec.styles.plan_selected.clone());
        let Some(source_name) = source_name else {
            return Task::none();
        };
        let Some(lib) = self.aec.aec_plan_library.as_ref() else {
            return Task::none();
        };
        let Some(cfg) = lib.find(&source_name).cloned() else {
            return Task::none();
        };
        let base_name = format!("{} (Kopie)", cfg.name);
        let mut name = base_name.clone();
        let mut counter = 2;
        while lib.find(&name).is_some() {
            name = format!("{base_name} {counter}");
            counter += 1;
        }
        self.aec.styles.plan_selected = None;
        self.aec.styles.plan_editing_name = None;
        self.aec.styles.plan_name = name;
        self.aec.styles.plan_discipline = cfg.discipline.clone();
        self.aec.styles.plan_scale = cfg.scale.map(|s| s.to_string()).unwrap_or_default();
        self.aec_plan_manager_load_phase_filter_buffers(cfg.phase_filter.as_ref());
        self.aec.styles.plan_planning_stage = cfg.planning_stage;
        self.aec.styles.plan_view_type = cfg.view_type.clone();
        self.aec_plan_manager_load_display_buffers(&cfg);
        self.aec.styles.plan_editing_id = None;
        self.aec.styles.plan_preset = None;
        self.aec.styles.plan_form_open = true;
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{AecMessage, AecModalKind, ModalKind, OpenCADStudio};
    use crate::modules::aec::engine::display_component::RepresentationMode;
    use crate::modules::aec::engine::library::{build_effective_rule_set, build_effective_slab_rule_set};
    use crate::modules::aec::engine::plan_view::{DetailSwitch, LayerDetail};
    use crate::modules::aec::ui::style_manager::StylesNode;

    fn app_with_project() -> (OpenCADStudio, std::path::PathBuf) {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let dir = std::env::temp_dir().join(format!("ocs_plans_{}_{}", std::process::id(), line!()));
        let path = dir.join("p.ocsproj");
        let project = crate::modules::aec::engine::project::ProjectFile::default();
        project.save(&path).unwrap();
        app.aec.aec_project_explorer_file = Some(project);
        app.aec.aec_project_explorer_path = Some(path);
        (app, dir)
    }

    fn send(app: &mut OpenCADStudio, m: AecMessage) {
        let _ = app.update_aec(m);
    }

    fn in_styles(app: &OpenCADStudio) -> bool {
        matches!(app.active_modal, Some(ModalKind::Aec(AecModalKind::Styles)))
    }

    #[test]
    fn user_plan_kind_create_save_duplicate_in_styles_window() {
        let (mut app, dir) = app_with_project();
        send(&mut app, AecMessage::AecStylesOpen(StylesNode::PlanKinds));
        assert!(in_styles(&app));
        assert_eq!(app.aec.styles.node, StylesNode::PlanKinds);

        send(&mut app, AecMessage::AecPlanManagerNew);
        send(&mut app, AecMessage::AecPlanManagerNameChanged("Meine Planart".into()));
        send(&mut app, AecMessage::AecPlanManagerScaleChanged("75".into()));
        send(&mut app, AecMessage::AecPlanManagerDetailLayersChanged(LayerDetail::StructuralOnly));
        send(&mut app, AecMessage::AecPlanManagerDetailToggle(DetailSwitch::Hatches, false));
        send(&mut app, AecMessage::AecPlanManagerDetailToggle(DetailSwitch::CeilingView, true));
        send(&mut app, AecMessage::AecPlanManagerRepresentationChanged(RepresentationMode::TwoD));
        send(&mut app, AecMessage::AecPlanManagerSave);
        assert!(in_styles(&app));

        let cfg = app.aec.aec_plan_library.as_ref().unwrap().find("Meine Planart").cloned().expect("saved");
        assert_eq!(cfg.scale, Some(75.0));
        assert_eq!(cfg.detail.layers, LayerDetail::StructuralOnly);
        assert!(!cfg.detail.hatches && cfg.detail.ceiling_view && cfg.detail.annotation);
        assert_eq!(cfg.detail.representation, RepresentationMode::TwoD);
        assert!(cfg.preset.is_none());
        // Persisted in the project, not only in memory.
        let stored = &app.aec.aec_project_explorer_file.as_ref().unwrap().display_config_library;
        assert!(stored.find("Meine Planart").is_some());

        // Re-select reloads the detail switches into the form buffers.
        send(&mut app, AecMessage::AecPlanManagerNew);
        assert_eq!(app.aec.styles.plan_detail, Default::default());
        send(&mut app, AecMessage::AecPlanManagerSelect("Meine Planart".into()));
        assert_eq!(app.aec.styles.plan_detail, cfg.detail);

        // Duplicate: new id, own name, same detail, no preset link.
        send(&mut app, AecMessage::AecPlanManagerDuplicate);
        assert_eq!(app.aec.styles.plan_name, "Meine Planart (Kopie)");
        send(&mut app, AecMessage::AecPlanManagerSave);
        let lib = app.aec.aec_plan_library.as_ref().unwrap();
        let copy = lib.find("Meine Planart (Kopie)").expect("copy");
        assert_ne!(copy.id, cfg.id);
        assert_eq!(copy.detail, cfg.detail);

        // A new plan kind must not overwrite an existing name.
        send(&mut app, AecMessage::AecPlanManagerNew);
        send(&mut app, AecMessage::AecPlanManagerNameChanged("Meine Planart".into()));
        send(&mut app, AecMessage::AecPlanManagerDetailToggle(DetailSwitch::Hatches, true));
        send(&mut app, AecMessage::AecPlanManagerSave);
        let still = app.aec.aec_plan_library.as_ref().unwrap().find("Meine Planart").unwrap();
        assert!(!still.detail.hatches);

        // Delete.
        send(&mut app, AecMessage::AecPlanManagerSelect("Meine Planart (Kopie)".into()));
        send(&mut app, AecMessage::AecPlanManagerDelete);
        assert!(app.aec.aec_plan_library.as_ref().unwrap().find("Meine Planart (Kopie)").is_none());
        assert!(in_styles(&app));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_plan_open_lands_on_planarten_node() {
        let (mut app, dir) = app_with_project();
        send(&mut app, AecMessage::AecPlanManagerOpen);
        assert!(in_styles(&app));
        assert_eq!(app.aec.styles.node, StylesNode::PlanKinds);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn plan_kind_detail_is_consistent_for_wall_slab_opening_rules() {
        use crate::modules::aec::engine::display_component::{
            OpeningComponentSlot, SlabComponentSlot, WallComponentSlot,
        };
        let (mut app, dir) = app_with_project();
        send(&mut app, AecMessage::AecPlanManagerOpen);
        send(&mut app, AecMessage::AecPlanManagerNew);
        send(&mut app, AecMessage::AecPlanManagerNameChanged("Rohbau-Detail".into()));
        send(&mut app, AecMessage::AecPlanManagerDetailLayersChanged(LayerDetail::Off));
        send(&mut app, AecMessage::AecPlanManagerDetailToggle(DetailSwitch::Hatches, false));
        send(&mut app, AecMessage::AecPlanManagerDetailToggle(DetailSwitch::OpeningSymbols, false));
        send(&mut app, AecMessage::AecPlanManagerDetailToggle(DetailSwitch::Annotation, false));
        send(&mut app, AecMessage::AecPlanManagerSave);
        let cfg = app.aec.aec_plan_library.as_ref().unwrap().find("Rohbau-Detail").cloned().unwrap();

        let wall = build_effective_rule_set(&cfg, None, None);
        assert!(!wall.is_visible(WallComponentSlot::Layers2D));
        assert!(!wall.is_visible(WallComponentSlot::LayerHatch2D));
        assert!(!wall.is_visible(WallComponentSlot::ContourHatch2D));
        assert!(!wall.visibility.get(OpeningComponentSlot::Swing2D.key()).copied().unwrap_or(true));
        assert!(!wall.visibility.get(OpeningComponentSlot::OpeningLabel2D.key()).copied().unwrap_or(true));
        let slab = build_effective_slab_rule_set(&cfg, None, None);
        assert!(!slab.is_slab_visible(SlabComponentSlot::LayerHatch2D));
        let _ = std::fs::remove_dir_all(dir);
    }
}
