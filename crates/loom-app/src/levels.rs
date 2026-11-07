//! Moving between levels: the top patch and the inside of a sub-patch
//! (`docs/design/sub-patches.md`, Make, open, and dissolve; Editor model).

use crate::App;
use loom_patch::ModulePath;

impl App {
    /// Shows `level` on the canvas. The selection belongs to the level it
    /// was made on, so it clears. The canvas takes the level's camera, or
    /// frames the level on a first visit.
    pub(crate) fn show_level(&mut self, level: &ModulePath) {
        if self.session.level == *level {
            return;
        }
        self.session.level = level.clone();
        self.session.level_key += 1;
        self.session.selection.clear();
        self.refresh();
    }
}

impl App {
    /// Opens the sub-patch tile `id`: the canvas shows its inside. A plain
    /// module or a port tile opens nothing.
    pub(crate) fn open_tile(&mut self, id: u32) {
        let id = loom_core::ModuleId(id);
        let session = &self.session;
        if !matches!(
            session.hierarchy.tile(session.controller.graph(), id),
            Some(loom_patch::Tile::Instance(_))
        ) {
            return;
        }
        let inside = session.hierarchy.path_of(id);
        self.show_level(&inside);
    }
}
