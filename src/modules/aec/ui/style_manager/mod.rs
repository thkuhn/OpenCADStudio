//! `AEC_STYLES` — unified style manager shell: tree | list | form + actions.
//!
//! Every node supplies a [`StyleParts`] (list rows, form, action bar); the
//! shell owns the layout.

pub mod actions;
pub mod forms;
pub mod list;
pub mod tree;
pub mod ui_state;

use iced::widget::{column, container, row, rule, text};
use iced::{Element, Fill};

use crate::app::Message;

pub use tree::StylesNode;
pub use ui_state::AecStylesUiState;

inventory::submit!(crate::command::CommandRegistration {
    names: &["AEC_STYLES"],
});

/// What a style kind contributes to the shell.
pub struct StyleParts<'a> {
    pub list: Element<'a, Message>,
    pub form: Element<'a, Message>,
    /// Hidden while no form is open.
    pub actions: Option<Element<'a, Message>>,
}

/// Compose the window: tree on the left, list in the middle, form + action
/// bar on the right.
pub fn view<'a>(node: StylesNode, parts: Option<StyleParts<'a>>) -> Element<'a, Message> {
    let tree = container(tree::view(node)).padding(10).height(Fill);
    match parts {
        Some(p) => {
            let mut right = column![container(p.form).width(Fill).height(Fill)].spacing(8);
            if let Some(a) = p.actions {
                right = right.push(a);
            }
            row![
                tree,
                rule::vertical(1),
                container(p.list).padding(10).height(Fill),
                rule::vertical(1),
                container(right).padding(10).width(Fill).height(Fill),
            ]
            .into()
        }
        None => row![tree, rule::vertical(1), container(text(node.label()).size(14)).padding(20).width(Fill).height(Fill)].into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AecMessage, ModalKind, OpenCADStudio};
    use crate::modules::aec::ui::modal_kind::AecModalKind;

    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    fn send(app: &mut OpenCADStudio, m: AecMessage) {
        let _ = app.update_aec(m);
    }

    fn is_styles(app: &OpenCADStudio) -> bool {
        matches!(app.active_modal, Some(ModalKind::Aec(AecModalKind::Styles)))
    }

    #[test]
    fn open_and_switch_nodes_stay_in_styles_window() {
        let mut a = app();
        send(&mut a, AecMessage::AecStylesOpen(StylesNode::Materials));
        assert!(is_styles(&a));
        assert_eq!(a.aec.styles.node, StylesNode::Materials);
        send(&mut a, AecMessage::AecStyleManagerMaterialNew);
        assert!(a.aec.styles.material_form_open);
        let _ = a.aec_styles_view();
        send(&mut a, AecMessage::AecStylesSelectNode(StylesNode::FloorFinish));
        assert!(is_styles(&a));
        assert_eq!(a.aec.styles.node, StylesNode::FloorFinish);
        send(&mut a, AecMessage::AecFloorFinishStyleManagerNew);
        assert!(a.aec.styles.floor_finish_form_open);
        let _ = a.aec_styles_view();
        send(&mut a, AecMessage::AecStylesSelectNode(StylesNode::Wall));
        assert!(is_styles(&a));
        assert_eq!(a.aec.styles.node, StylesNode::Wall);
        let _ = a.aec_styles_view();
    }

    #[test]
    fn material_save_keeps_styles_window_and_lists_entry() {
        let mut a = app();
        send(&mut a, AecMessage::AecStylesOpen(StylesNode::Materials));
        send(&mut a, AecMessage::AecStyleManagerMaterialNew);
        send(
            &mut a,
            AecMessage::AecStyleManagerMaterialNameChanged("Testmaterial Styles".into()),
        );
        send(&mut a, AecMessage::AecStyleManagerMaterialSave);
        assert!(is_styles(&a), "save must not leave AEC_STYLES");
        assert!(a
            .aec
            .aec_style_library
            .as_ref()
            .unwrap()
            .materials
            .iter()
            .any(|m| m.name == "Testmaterial Styles"));
        let _ = a.aec_styles_view();
        send(&mut a, AecMessage::AecStyleManagerMaterialDelete);
        assert!(is_styles(&a));
    }

    #[test]
    fn component_nodes_open_embedded_and_legacy_commands_target_them() {
        for node in [
            StylesNode::Wall,
            StylesNode::Opening,
            StylesNode::Slab,
            StylesNode::SlabStructural,
        ] {
            let mut a = app();
            send(&mut a, AecMessage::AecStylesOpen(node));
            assert!(is_styles(&a), "{node:?}");
            assert_eq!(a.aec.styles.node, node);
            let _ = a.aec_styles_view();
        }
    }

    #[test]
    fn wall_save_duplicate_delete_stay_in_styles() {
        let mut a = app();
        send(&mut a, AecMessage::AecStylesOpen(StylesNode::Wall));
        send(&mut a, AecMessage::AecStyleManagerWallStyleNew);
        send(&mut a, AecMessage::AecStyleManagerWallStyleNameChanged("Wandtest Styles".into()));
        send(&mut a, AecMessage::AecStyleManagerWallStyleLayerAdd);
        send(&mut a, AecMessage::AecStyleManagerWallStyleSave);
        assert!(is_styles(&a));
        let count = |a: &OpenCADStudio, n: &str| {
            a.aec.aec_style_library.as_ref().unwrap().wall_styles.iter().filter(|w| w.style.name == n).count()
        };
        assert_eq!(count(&a, "Wandtest Styles"), 1);
        let _ = a.aec_styles_view();
        send(&mut a, AecMessage::AecStyleManagerWallStyleDuplicate);
        assert!(a.aec.styles.wall_style_editing_id.is_none());
        send(&mut a, AecMessage::AecStyleManagerWallStyleSave);
        assert_eq!(count(&a, "Wandtest Styles (Kopie)"), 1);
        let copy = a.aec.aec_style_library.as_ref().unwrap().wall_styles.iter()
            .find(|w| w.style.name == "Wandtest Styles (Kopie)").unwrap().clone();
        assert_eq!(copy.layers.len(), 1);
        send(&mut a, AecMessage::AecStyleManagerWallStyleDelete);
        assert!(is_styles(&a));
        assert_eq!(count(&a, "Wandtest Styles (Kopie)"), 0);
    }

    #[test]
    fn opening_save_duplicate_and_slab_structural_flows_stay_in_styles() {
        let mut a = app();
        send(&mut a, AecMessage::AecStylesOpen(StylesNode::Opening));
        send(&mut a, AecMessage::AecOpeningStyleManagerNew);
        send(&mut a, AecMessage::AecOpeningStyleManagerNameChanged("Oeffnungstest".into()));
        send(&mut a, AecMessage::AecOpeningStyleManagerSave);
        assert!(is_styles(&a));
        let n = |a: &OpenCADStudio, name: &str| {
            a.aec.aec_style_library.as_ref().unwrap().opening_styles.iter().filter(|s| s.style.name == name).count()
        };
        assert_eq!(n(&a, "Oeffnungstest"), 1);
        send(&mut a, AecMessage::AecOpeningStyleManagerDuplicate);
        send(&mut a, AecMessage::AecOpeningStyleManagerSave);
        assert_eq!(n(&a, "Oeffnungstest (Kopie)"), 1);
        send(&mut a, AecMessage::AecOpeningStyleManagerDelete);
        assert_eq!(n(&a, "Oeffnungstest (Kopie)"), 0);
        let _ = a.aec_styles_view();

        send(&mut a, AecMessage::AecStylesSelectNode(StylesNode::SlabStructural));
        send(&mut a, AecMessage::AecSlabStructuralStyleManagerNew);
        send(&mut a, AecMessage::AecSlabStructuralStyleManagerNameChanged("Rohbautest".into()));
        send(&mut a, AecMessage::AecSlabStructuralStyleManagerSave);
        assert!(is_styles(&a));
        let _ = a.aec_styles_view();
        send(&mut a, AecMessage::AecStylesSelectNode(StylesNode::Slab));
        send(&mut a, AecMessage::AecSlabStyleManagerNew);
        send(&mut a, AecMessage::AecSlabStyleManagerNameChanged("Deckentest".into()));
        send(&mut a, AecMessage::AecSlabStyleManagerSave);
        assert!(is_styles(&a));
        let _ = a.aec_styles_view();
    }

    // ---- flow helpers -------------------------------------------------
    // Library file is thread-local under cfg(test) (`engine::library`).

    use crate::modules::aec::engine::library::{self, StyleLibrary};
    use crate::modules::aec::engine::material::Material;
    use crate::modules::aec::engine::project::ProjectFile;
    use crate::modules::aec::engine::slab_style::FloorFinishStyle;

    struct TempProject(std::path::PathBuf);
    impl Drop for TempProject {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn put_global(f: impl FnOnce(&mut StyleLibrary)) {
        let mut lib = library::load_or_seed();
        f(&mut lib);
        library::save_to_default_path(&lib).unwrap();
    }

    fn mat(id: &str, name: &str) -> Material {
        Material::new(id.into(), name.into(), "SOLID".into(), 0x112233, "Continuous".into())
    }

    fn open_project(a: &mut OpenCADStudio, tag: &str, f: impl FnOnce(&mut StyleLibrary)) -> TempProject {
        let mut lib = library::load_or_seed();
        lib.materials.retain(|m| !m.id.starts_with("t_"));
        lib.floor_finish_styles.retain(|s| !s.style.id.starts_with("t_"));
        f(&mut lib);
        let mut project = ProjectFile::default();
        project.material_wall_style_library = lib;
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ocs_styles_{}_{}_{tag}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let path = dir.join("p.ocsproj");
        project.save(&path).unwrap();
        a.aec.aec_project_explorer_file = Some(project);
        a.aec.aec_project_explorer_path = Some(path);
        a.aec_refresh_combined_style_library();
        TempProject(dir)
    }

    fn project_lib(a: &OpenCADStudio) -> &StyleLibrary {
        &a.aec.aec_project_explorer_file.as_ref().unwrap().material_wall_style_library
    }

    fn is_conflict(a: &OpenCADStudio) -> bool {
        matches!(a.active_modal, Some(ModalKind::Aec(AecModalKind::StyleCopyConflict)))
    }

    #[test]
    fn floor_finish_pickers_stay_in_styles_and_hit_finish_buffers() {
        use crate::app::StylePickerTarget as T;
        let mut a = app();
        send(&mut a, AecMessage::AecStylesOpen(StylesNode::FloorFinish));
        send(&mut a, AecMessage::AecFloorFinishStyleManagerNew);
        send(&mut a, AecMessage::AecFloorFinishStyleManagerLayerAdd);
        let wall_before = a.aec.styles.wall_style_layers.len();
        let slab_parent_before = a.aec.styles.slab_parent.clone();
        let mat_id = a.aec.aec_style_library.as_ref().unwrap().materials.last().unwrap().id.clone();

        // Layer material.
        send(&mut a, AecMessage::AecStylePickerOpen(T::FloorFinishLayerMaterial(0)));
        assert!(matches!(
            a.active_modal,
            Some(ModalKind::Aec(AecModalKind::StylePicker { target: T::FloorFinishLayerMaterial(0) }))
        ));
        assert_eq!(
            a.aec.aec_style_picker_selection.as_deref(),
            Some(a.aec.styles.floor_finish_layers[0].material_id.as_str())
        );
        send(&mut a, AecMessage::AecStylePickerSelect(mat_id.clone()));
        send(&mut a, AecMessage::AecStylePickerConfirm);
        assert!(is_styles(&a));
        assert_eq!(a.aec.styles.floor_finish_layers[0].material_id, mat_id);
        assert_eq!(a.aec.styles.wall_style_layers.len(), wall_before);

        // Parent: confirm.
        let parent = {
            let l = a.aec.aec_style_library.as_ref().unwrap();
            l.floor_finish_styles.first().map(|s| s.style.id.clone())
        };
        send(&mut a, AecMessage::AecStylePickerOpen(T::FloorFinishStyleParent));
        let target = parent.unwrap_or_else(|| "x".into());
        send(&mut a, AecMessage::AecStylePickerSelect(target.clone()));
        send(&mut a, AecMessage::AecStylePickerConfirm);
        assert!(is_styles(&a));
        assert_eq!(a.aec.styles.floor_finish_parent.as_deref(), Some(target.as_str()));
        assert_eq!(a.aec.styles.slab_parent, slab_parent_before);

        // (None) clears the parent.
        send(&mut a, AecMessage::AecStylePickerOpen(T::FloorFinishStyleParent));
        send(&mut a, AecMessage::AecStylePickerSelect(String::new()));
        send(&mut a, AecMessage::AecStylePickerConfirm);
        assert!(is_styles(&a));
        assert_eq!(a.aec.styles.floor_finish_parent, None);
        send(&mut a, AecMessage::AecStylePickerOpen(T::FloorFinishStyleParent));
        send(&mut a, AecMessage::AecStylePickerSelect(target.clone()));
        send(&mut a, AecMessage::AecStylePickerConfirm);
        assert_eq!(a.aec.styles.floor_finish_parent.as_deref(), Some(target.as_str()));

        // Cancel returns to Styles, buffers untouched.
        send(&mut a, AecMessage::AecStylePickerOpen(T::FloorFinishStyleParent));
        send(&mut a, AecMessage::AecStylePickerSelect(String::new()));
        send(&mut a, AecMessage::AecStylePickerCancel);
        assert!(is_styles(&a));
        assert_eq!(a.aec.styles.floor_finish_parent.as_deref(), Some(target.as_str()));
        send(&mut a, AecMessage::AecStylePickerOpen(T::FloorFinishLayerMaterial(0)));
        send(&mut a, AecMessage::AecStylePickerCancel);
        assert!(is_styles(&a));
    }

    #[test]
    fn reclicking_active_node_keeps_unsaved_edits() {
        let mut a = app();
        send(&mut a, AecMessage::AecStylesOpen(StylesNode::Materials));
        send(&mut a, AecMessage::AecStyleManagerMaterialNew);
        send(&mut a, AecMessage::AecStyleManagerMaterialNameChanged("Ungespeichert".into()));
        send(&mut a, AecMessage::AecStylesSelectNode(StylesNode::Materials));
        assert!(a.aec.styles.material_form_open);
        assert_eq!(a.aec.styles.material_name, "Ungespeichert");
        // Switching to another node still re-seeds.
        send(&mut a, AecMessage::AecStylesSelectNode(StylesNode::FloorFinish));
        assert_eq!(a.aec.styles.node, StylesNode::FloorFinish);
    }

    #[test]
    fn floor_finish_save_duplicate_delete_through_shell() {
        let mut a = app();
        send(&mut a, AecMessage::AecStylesOpen(StylesNode::FloorFinish));
        send(&mut a, AecMessage::AecFloorFinishStyleManagerNew);
        send(&mut a, AecMessage::AecFloorFinishStyleManagerNameChanged("Belag Flow Test".into()));
        send(&mut a, AecMessage::AecFloorFinishStyleManagerSave);
        assert!(is_styles(&a));
        let find = |a: &OpenCADStudio, n: &str| {
            a.aec.aec_style_library.as_ref().unwrap().floor_finish_styles.iter()
                .find(|s| s.style.name == n).map(|s| s.style.id.clone())
        };
        let id = find(&a, "Belag Flow Test").expect("saved entry listed in library");
        let _ = a.aec_styles_view();

        // Duplicate: becomes a new unsaved entry, saving yields a second one.
        send(&mut a, AecMessage::AecFloorFinishStyleManagerSelect(id.clone()));
        send(&mut a, AecMessage::AecFloorFinishStyleManagerDuplicate);
        assert!(is_styles(&a));
        assert_eq!(a.aec.styles.floor_finish_name, "Belag Flow Test (Kopie)");
        send(&mut a, AecMessage::AecFloorFinishStyleManagerSave);
        let copy_id = find(&a, "Belag Flow Test (Kopie)").expect("duplicate saved");
        assert_ne!(copy_id, id);

        // Delete both.
        for del in [copy_id, id] {
            send(&mut a, AecMessage::AecFloorFinishStyleManagerSelect(del.clone()));
            send(&mut a, AecMessage::AecFloorFinishStyleManagerDelete);
            assert!(is_styles(&a));
            assert!(!a.aec.aec_style_library.as_ref().unwrap().floor_finish_styles.iter().any(|s| s.style.id == del));
        }
        let _ = a.aec_styles_view();
    }

    #[test]
    fn material_duplicate_and_save_apply_through_shell() {
        let mut a = app();
        send(&mut a, AecMessage::AecStylesOpen(StylesNode::Materials));
        send(&mut a, AecMessage::AecStyleManagerMaterialNew);
        send(&mut a, AecMessage::AecStyleManagerMaterialNameChanged("Mat Dup Test".into()));
        send(&mut a, AecMessage::AecStyleManagerMaterialSaveAndApply);
        assert!(is_styles(&a));
        let id = a.aec.aec_style_library.as_ref().unwrap().materials.iter()
            .find(|m| m.name == "Mat Dup Test").expect("saved").id.clone();
        send(&mut a, AecMessage::AecStyleManagerSelectMaterial(id.clone()));
        send(&mut a, AecMessage::AecStyleManagerMaterialDuplicate);
        assert!(is_styles(&a));
        send(&mut a, AecMessage::AecStyleManagerMaterialSave);
        let lib = a.aec.aec_style_library.as_ref().unwrap();
        let copies: Vec<_> = lib.materials.iter().filter(|m| m.name.starts_with("Mat Dup Test")).collect();
        assert_eq!(copies.len(), 2);
        let ids: Vec<String> = copies.iter().map(|m| m.id.clone()).collect();
        for d in ids {
            send(&mut a, AecMessage::AecStyleManagerSelectMaterial(d));
            send(&mut a, AecMessage::AecStyleManagerMaterialDelete);
            assert!(is_styles(&a));
        }
    }

    #[test]
    fn material_copy_to_project_and_global_with_conflicts() {
        let mut a = app();
        send(&mut a, AecMessage::AecStylesOpen(StylesNode::Materials));

        // Global -> project, no conflict.
        put_global(|l| l.upsert_material(mat("t_m_g2p", "Global Mat")));
        let _p = open_project(&mut a, "m1", |_| {});
        assert!(!project_lib(&a).materials.iter().any(|m| m.id == "t_m_g2p"));
        a.aec.styles.selected_material = Some("t_m_g2p".into());
        send(&mut a, AecMessage::AecStyleManagerCopyMaterialToProject);
        assert!(is_styles(&a));
        assert!(project_lib(&a).materials.iter().any(|m| m.id == "t_m_g2p" && m.name == "Global Mat"));

        // Global -> project, conflicting content: confirm overwrites.
        put_global(|l| l.upsert_material(mat("t_m_conf", "Global Version")));
        let _p = open_project(&mut a, "m2", |l| l.upsert_material(mat("t_m_conf", "Project Version")));
        a.aec.styles.selected_material = Some("t_m_conf".into());
        send(&mut a, AecMessage::AecStyleManagerCopyMaterialToProject);
        assert!(is_conflict(&a));
        assert!(a.aec.styles.pending_copy.is_some());
        send(&mut a, AecMessage::AecStyleManagerCopyConflictConfirm(false));
        assert!(is_styles(&a) && a.aec.styles.pending_copy.is_none());
        assert!(project_lib(&a).materials.iter().any(|m| m.id == "t_m_conf" && m.name == "Project Version"));
        send(&mut a, AecMessage::AecStyleManagerCopyMaterialToProject);
        assert!(is_conflict(&a));
        send(&mut a, AecMessage::AecStyleManagerCopyConflictConfirm(true));
        assert!(is_styles(&a));
        assert!(project_lib(&a).materials.iter().any(|m| m.id == "t_m_conf" && m.name == "Global Version"));

        // Project -> global.
        let _p = open_project(&mut a, "m3", |l| {
            l.upsert_material(mat("t_m_p2g", "Project Only"));
            l.upsert_material(mat("t_m_conf2", "Project Side"));
        });
        a.aec.styles.selected_material = Some("t_m_p2g".into());
        send(&mut a, AecMessage::AecStyleManagerCopyMaterialToGlobal);
        assert!(is_styles(&a));
        assert!(library::load_or_seed().materials.iter().any(|m| m.id == "t_m_p2g"));
        put_global(|l| l.upsert_material(mat("t_m_conf2", "Global Side")));
        a.aec.styles.selected_material = Some("t_m_conf2".into());
        send(&mut a, AecMessage::AecStyleManagerCopyMaterialToGlobal);
        assert!(is_conflict(&a));
        send(&mut a, AecMessage::AecStyleManagerCopyConflictConfirm(true));
        assert!(is_styles(&a));
        assert!(library::load_or_seed().materials.iter().any(|m| m.id == "t_m_conf2" && m.name == "Project Side"));
    }

    #[test]
    fn floor_finish_copy_to_project_and_global_with_conflicts() {
        let fin = |id: &str, name: &str| FloorFinishStyle::new(id, name);
        let mut a = app();
        send(&mut a, AecMessage::AecStylesOpen(StylesNode::FloorFinish));

        put_global(|l| l.upsert_floor_finish_style(fin("t_f_g2p", "Global Finish")));
        let _p = open_project(&mut a, "f1", |_| {});
        a.aec.styles.floor_finish_selected = Some("t_f_g2p".into());
        send(&mut a, AecMessage::AecStyleManagerCopyFloorFinishStyleToProject);
        assert!(is_styles(&a));
        assert!(project_lib(&a).floor_finish_styles.iter().any(|s| s.style.id == "t_f_g2p"));

        put_global(|l| l.upsert_floor_finish_style(fin("t_f_conf", "Global Version")));
        let _p = open_project(&mut a, "f2", |l| l.upsert_floor_finish_style(fin("t_f_conf", "Project Version")));
        a.aec.styles.floor_finish_selected = Some("t_f_conf".into());
        send(&mut a, AecMessage::AecStyleManagerCopyFloorFinishStyleToProject);
        assert!(is_conflict(&a));
        send(&mut a, AecMessage::AecStyleManagerCopyConflictConfirm(true));
        assert!(is_styles(&a));
        assert!(project_lib(&a).floor_finish_styles.iter().any(|s| s.style.id == "t_f_conf" && s.style.name == "Global Version"));

        let _p = open_project(&mut a, "f3", |l| {
            l.upsert_floor_finish_style(fin("t_f_p2g", "Project Only"));
            l.upsert_floor_finish_style(fin("t_f_conf2", "Project Side"));
        });
        a.aec.styles.floor_finish_selected = Some("t_f_p2g".into());
        send(&mut a, AecMessage::AecStyleManagerCopyFloorFinishStyleToGlobal);
        assert!(is_styles(&a));
        assert!(library::load_or_seed().floor_finish_styles.iter().any(|s| s.style.id == "t_f_p2g"));
        put_global(|l| l.upsert_floor_finish_style(fin("t_f_conf2", "Global Side")));
        a.aec.styles.floor_finish_selected = Some("t_f_conf2".into());
        send(&mut a, AecMessage::AecStyleManagerCopyFloorFinishStyleToGlobal);
        assert!(is_conflict(&a));
        send(&mut a, AecMessage::AecStyleManagerCopyConflictConfirm(false));
        assert!(is_styles(&a));
        assert!(library::load_or_seed().floor_finish_styles.iter().any(|s| s.style.id == "t_f_conf2" && s.style.name == "Global Side"));
    }
}
