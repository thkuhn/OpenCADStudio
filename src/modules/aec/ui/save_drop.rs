//! AEC drop warning on lossy Save-As (formerly in Core `app/update/file.rs`).

use crate::app::OpenCADStudio;

use super::modal_kind::AecModalKind;

impl OpenCADStudio {
    /// Warns before a lossy Save-As that would drop unsupported (AEC /
    /// application) objects kept only as verbatim source-version bytes.
    /// Returns `true` when the warning modal was opened and saving must stop.
    pub(crate) fn aec_save_drop_warning(&mut self, ext: &str, version: acadrust::DxfVersion) -> bool {
        if self.aec.aec_drop_acknowledged || ocs_save::external_format(ext).is_some() {
            return false;
        }
        let is_dxf = ext.eq_ignore_ascii_case("dxf");
        let n = crate::io::dropped_on_save_count(
            &self.tabs[self.active_tab].scene.document,
            version,
            is_dxf,
        );
        if n == 0 {
            return false;
        }
        self.aec.aec_drop_count = n;
        self.active_modal = Some(crate::app::ModalKind::Aec(AecModalKind::DropWarning));
        true
    }
}
