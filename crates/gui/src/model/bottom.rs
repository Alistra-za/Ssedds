//! The bottom panel's state (layout v2 A1, A9; R7, R8): which tab shows, and whether the panel is open.
use super::AppModel;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BottomTab {
    Problems,
    Review,
    Messages,
    #[default]
    Files,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BottomPanel {
    pub open: bool,
    pub tab: BottomTab,
}

impl AppModel {
    /// Open the panel on `tab`.
    pub fn show_tab(&mut self, tab: BottomTab) {
        self.bottom = BottomPanel { open: true, tab };
        self.sync_messages_open();
    }

    /// Open on `tab`, or collapse when the panel already shows it.
    pub fn toggle_tab(&mut self, tab: BottomTab) {
        if self.bottom.open && self.bottom.tab == tab {
            self.bottom.open = false;
        } else {
            self.bottom = BottomPanel { open: true, tab };
        }
        self.sync_messages_open();
    }

    pub fn collapse_bottom(&mut self) {
        self.bottom.open = false;
        self.sync_messages_open();
    }

    /// R8: `Messages::is_open` means "the Messages tab is showing".
    fn sync_messages_open(&mut self) {
        let showing = self.bottom.open && self.bottom.tab == BottomTab::Messages;
        self.messages.set_open(showing);
    }
}
