//! What is open over the canvas.
//!
//! One panel at a time, by construction: [`Overlay`] holds it. Three things can
//! open over a panel, each in its own slot, and the view draws them in this
//! order, so they stack only one way: the licence chooser, help over the dialogs
//! (F1 opens it from them, help-pages.md), and a question over everything
//! (files.md, Asking before losing work). Who has the input follows from the
//! same order ([`Overlays::owner`]).

pub(crate) mod access;

use crate::input_owner::InputOwner;
use crate::{browser, help, licence_chooser, patches, probe_menu, question, recent, settings};
use loom_ui_core::Point;

/// The one panel that can be open.
pub(crate) enum Overlay {
    /// The module browser.
    Browser(browser::Browser),
    /// The rows of the Recent list. Read when it opens.
    RecentList(Vec<recent::Row>),
    /// Your patches.
    Patches(patches::PatchBrowser),
    /// The Preferences window.
    Preferences(Box<settings::Window>),
    /// A menu on the canvas.
    Menu(ContextMenu),
}

/// A menu on the canvas.
pub(crate) enum ContextMenu {
    /// A right-clicked tile, and where the menu opens.
    Tile { module: u32, at: Point },
    /// The menu on an output.
    Probe(probe_menu::ProbeMenu),
}

/// The open panel, and what can open over it.
#[derive(Default)]
pub(crate) struct Overlays {
    panel: Option<Overlay>,
    /// While a menu is open, the item its keys are on (`menus.rs`). A menu
    /// opens with none, and opening any panel clears it.
    menu_highlight: Option<usize>,
    /// The licence chooser, while it is open.
    pub chooser: Option<licence_chooser::Chooser>,
    /// Open module help.
    pub help: Option<help::Help>,
    /// The question over everything else, while one is open. While it is,
    /// most messages are dropped (`Message::allowed_during_question`).
    pub question: Option<question::Question>,
}

impl Overlays {
    /// Who has the input: the top layer the view draws.
    pub fn owner(&self) -> InputOwner {
        if self.question.is_some() {
            InputOwner::Question
        } else if self.help.is_some() {
            InputOwner::Help
        } else if self.chooser.is_some() {
            InputOwner::Chooser
        } else {
            match &self.panel {
                Some(Overlay::Patches(_)) => InputOwner::Patches,
                Some(Overlay::Preferences(_)) => InputOwner::Preferences,
                Some(Overlay::Browser(_)) => InputOwner::Browser,
                Some(Overlay::RecentList(_)) => InputOwner::RecentList,
                Some(Overlay::Menu(ContextMenu::Probe(_))) => InputOwner::ProbeMenu,
                Some(Overlay::Menu(ContextMenu::Tile { .. })) => InputOwner::TileMenu,
                None => InputOwner::Canvas,
            }
        }
    }

    /// Opens a panel in place of the one that is open.
    pub fn open(&mut self, panel: Overlay) {
        self.panel = Some(panel);
        self.menu_highlight = None;
    }

    /// True while a menu is open: the tile menu, the probe menu, or the
    /// Recent list.
    pub fn menu_open(&self) -> bool {
        matches!(self.panel, Some(Overlay::Menu(_) | Overlay::RecentList(_)))
    }

    /// The open menu's highlighted item, if a key has moved there.
    pub fn menu_highlight(&self) -> Option<usize> {
        self.menu_highlight.filter(|_| self.menu_open())
    }

    /// Moves the open menu's highlight.
    pub fn set_menu_highlight(&mut self, at: Option<usize>) {
        self.menu_highlight = at;
    }

    /// What a new patch leaves open: only the Preferences window, which is
    /// about the machine and not the patch.
    pub fn close_for_new_patch(&mut self) {
        let preferences = matches!(self.panel, Some(Overlay::Preferences(_)));
        *self = Self {
            panel: self.panel.take().filter(|_| preferences),
            ..Self::default()
        };
    }

    /// The module browser, if it is open.
    pub fn browser(&self) -> Option<&browser::Browser> {
        match &self.panel {
            Some(Overlay::Browser(browser)) => Some(browser),
            _ => None,
        }
    }

    /// The module browser, to change it.
    pub fn browser_mut(&mut self) -> Option<&mut browser::Browser> {
        match &mut self.panel {
            Some(Overlay::Browser(browser)) => Some(browser),
            _ => None,
        }
    }

    /// Closes the module browser, if it is open.
    pub fn close_browser(&mut self) {
        if self.browser().is_some() {
            self.panel = None;
        }
    }

    /// The rows of the Recent list, if it is open.
    pub fn recent_rows(&self) -> Option<&Vec<recent::Row>> {
        match &self.panel {
            Some(Overlay::RecentList(rows)) => Some(rows),
            _ => None,
        }
    }

    /// Closes the Recent list and hands back its rows.
    pub fn take_recent_rows(&mut self) -> Option<Vec<recent::Row>> {
        self.recent_rows()?;
        match self.panel.take() {
            Some(Overlay::RecentList(rows)) => Some(rows),
            _ => None,
        }
    }

    /// Closes the Recent list, if it is open.
    pub fn close_recent_list(&mut self) {
        self.take_recent_rows();
    }

    /// Your patches, if it is open.
    pub fn patches(&self) -> Option<&patches::PatchBrowser> {
        match &self.panel {
            Some(Overlay::Patches(browser)) => Some(browser),
            _ => None,
        }
    }

    /// Your patches, to change it.
    pub fn patches_mut(&mut self) -> Option<&mut patches::PatchBrowser> {
        match &mut self.panel {
            Some(Overlay::Patches(browser)) => Some(browser),
            _ => None,
        }
    }

    /// Closes Your patches, if it is open.
    pub fn close_patches(&mut self) {
        if self.patches().is_some() {
            self.panel = None;
        }
    }

    /// The Preferences window, if it is open.
    pub fn preferences(&self) -> Option<&settings::Window> {
        match &self.panel {
            Some(Overlay::Preferences(window)) => Some(window),
            _ => None,
        }
    }

    /// The Preferences window, to change it.
    pub fn preferences_mut(&mut self) -> Option<&mut settings::Window> {
        match &mut self.panel {
            Some(Overlay::Preferences(window)) => Some(window),
            _ => None,
        }
    }

    /// Closes the Preferences window, if it is open.
    pub fn close_preferences(&mut self) {
        if self.preferences().is_some() {
            self.panel = None;
        }
    }

    /// The tile menu's module and position, if it is open.
    pub fn tile_menu(&self) -> Option<(u32, Point)> {
        match self.panel {
            Some(Overlay::Menu(ContextMenu::Tile { module, at })) => Some((module, at)),
            _ => None,
        }
    }

    /// Closes the tile menu, if it is open.
    pub fn close_tile_menu(&mut self) {
        if self.tile_menu().is_some() {
            self.panel = None;
        }
    }

    /// The probe menu, if it is open.
    pub fn probe_menu(&self) -> Option<&probe_menu::ProbeMenu> {
        match &self.panel {
            Some(Overlay::Menu(ContextMenu::Probe(menu))) => Some(menu),
            _ => None,
        }
    }

    /// Closes the probe menu and hands it back.
    pub fn take_probe_menu(&mut self) -> Option<probe_menu::ProbeMenu> {
        self.probe_menu()?;
        match self.panel.take() {
            Some(Overlay::Menu(ContextMenu::Probe(menu))) => Some(menu),
            _ => None,
        }
    }

    /// Closes the probe menu, if it is open.
    pub fn close_probe_menu(&mut self) {
        self.take_probe_menu();
    }
}

#[cfg(test)]
mod tests;
