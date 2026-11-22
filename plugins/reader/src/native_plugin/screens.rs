//! Where a screen of the reader opens, comes to the top and closes: each kind
//! of screen to its own views.
use super::*;
use crate::declarations::STATUS_EDIT;

impl NativeReader {
    /// A screen of the reader opened: over the reading screen the pages stop
    /// turning, then the screen's own view is opened.
    pub(super) fn screen_opened(
        &mut self,
        host: &Host<'_>,
        token: u32,
        instance: paperpaw_sdk::native::ui::Instance,
        handover: Option<paperpaw_sdk::native::ui::HandoverId>,
    ) -> Result<(), Error> {
        self.auto_turn_leave(host);
        // Another screen of the reader: the user went elsewhere first.
        if token != SCREEN {
            self.aloud_asked = None;
        }
        match token {
            SCREEN => self.open(host, instance, handover),
            TOC => self.open_toc(host, instance, handover),
            ANNOTATIONS => self.open_notes(host, instance, handover),
            SEARCH => self.open_search(host, instance, handover),
            FOOTNOTE => self.open_note(host, instance, handover),
            SKIM => self.open_skim(host, instance, handover),
            STATUS_EDIT => self
                .status_edits
                .open(host, &mut self.ui, instance, handover),
            _ => Ok(()),
        }
    }

    /// A screen of the reader came to the top.
    pub(super) fn screen_shown(
        &mut self,
        host: &Host<'_>,
        token: u32,
        instance: paperpaw_sdk::native::ui::Instance,
        now_ms: u64,
    ) -> Result<(), Error> {
        match token {
            SCREEN => self.reading_shown(host, instance, now_ms),
            TOC => self.tocs.shown(TOC, instance),
            SEARCH => self.searches.shown(SEARCH, instance),
            ANNOTATIONS => self.annotations.shown(ANNOTATIONS, instance),
            FOOTNOTE => self.notes.shown(FOOTNOTE, instance),
            SKIM => self.skims.shown(SKIM, instance),
            STATUS_EDIT => self.status_edits.shown(STATUS_EDIT, instance),
            _ => Ok(()),
        }
    }

    /// A screen of the reader closed: its view goes with it.
    pub(super) fn screen_closed(
        &mut self,
        host: &Host<'_>,
        token: u32,
        instance: paperpaw_sdk::native::ui::Instance,
    ) {
        match token {
            SCREEN => {
                self.hands.closed(host, instance, self.work.as_ref());
                self.close(host, instance);
            }
            TOC => self.close_toc(host, instance),
            SEARCH => {
                if let Some(view) = self.searches.get(instance) {
                    view.close();
                }
                self.searches.close(instance);
            }
            ANNOTATIONS => self.annotations.close(instance),
            FOOTNOTE => self.notes.close(instance),
            SKIM => self.skims.close(instance),
            STATUS_EDIT => self.status_edits.close(instance),
            _ => {}
        }
    }
}
