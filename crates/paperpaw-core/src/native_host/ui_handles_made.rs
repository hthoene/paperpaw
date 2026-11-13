//! `event.widget_made` (84, `docs/abi/ui.md` 5.2 "Lifetime"): the owner of a
//! widget hears of each instance another plugin makes of it, at the widget's
//! token, before the host calls it about that instance.
//!
//! Op 4 lists a notice with the instance ([`UiHandles::make_told`]): the
//! widget id as the maker named it (a placement `x@2` included) and the
//! place, in one fallible allocation of at most 256 bytes, taken before the
//! instance is made, so a refusal leaves nothing. Core tells it between
//! callbacks, before the drops (`widget_drops.rs`), or inside the maker's
//! callback right before the owner's first `core.widget` call or nested draw
//! for the instance. A notice ends at its first attempt whatever came of it,
//! as a drop's does, and at once when its owner stops. At most the widget cap
//! of notices wait; the list keeps no room once it is empty.
use super::UiHandles;
use alloc::{string::String, vec::Vec};
use paperpaw_native::host_glue::{ERR_FULL, ERR_INVALID, ERR_NO_MEMORY};

/// Another plugin's widget op 4 makes an instance of, as the directory and
/// the maker name it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NewWidget<'a> {
    /// The owner's host owner id.
    pub(crate) owner: u64,
    /// The widget's callback token of the owner.
    pub(crate) token: u32,
    pub(crate) private: bool,
    /// The id the maker named (`ui_widget_ref.widget`).
    pub(crate) widget: &'a str,
    /// `ui_widget_ref.place`.
    pub(crate) place: &'a str,
}

/// A made instance its owner is yet to hear of.
#[derive(Debug)]
pub(crate) struct Made {
    /// The owner's host owner id.
    pub(crate) owner: u64,
    /// The widget's callback token of the owner.
    pub(crate) token: u32,
    pub(crate) instance: u32,
    /// The widget id, then the place.
    text: String,
    /// Where the place starts in `text`.
    split: u16,
}

impl Made {
    /// The widget id as the maker named it.
    pub(crate) fn widget(&self) -> &str {
        self.text.get(..usize::from(self.split)).unwrap_or("")
    }

    /// The place the maker named.
    pub(crate) fn place(&self) -> &str {
        self.text.get(usize::from(self.split)..).unwrap_or("")
    }
}

impl UiHandles {
    /// Op 4 for another plugin's widget: [`Self::make_widget`], and the notice
    /// its owner is to get. `ERR_FULL` when the cap of notices wait, besides
    /// the instance's own refusals; `ERR_NO_MEMORY` when the heap refuses.
    /// Nothing is made or listed on any refusal.
    pub(crate) fn make_told(
        &mut self,
        maker: u64,
        screen: u32,
        new: &NewWidget<'_>,
    ) -> Result<u32, i32> {
        if self.made.len() >= self.widget_cap {
            return Err(ERR_FULL);
        }
        let split = u16::try_from(new.widget.len()).map_err(|_| ERR_INVALID)?;
        let length = new
            .widget
            .len()
            .checked_add(new.place.len())
            .ok_or(ERR_INVALID)?;
        let mut text = String::new();
        text.try_reserve_exact(length).map_err(|_| ERR_NO_MEMORY)?;
        text.push_str(new.widget);
        text.push_str(new.place);
        self.made.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        let instance = self.make_widget(new.owner, maker, screen, new.token, new.private)?;
        self.made.push(Made {
            owner: new.owner,
            token: new.token,
            instance,
            text,
            split,
        });
        Ok(instance)
    }

    /// The notice of `instance`, or the oldest one when `None`.
    pub(crate) fn next_made(&self, instance: Option<u32>) -> Option<&Made> {
        match instance {
            Some(id) => self.made.iter().find(|m| m.instance == id),
            None => self.made.first(),
        }
    }

    /// The first attempt to tell of `instance` is over, whatever came of it:
    /// its notice ends (the list's room too, once it is empty).
    pub(crate) fn made_told(&mut self, instance: u32) {
        self.made.retain(|m| m.instance != instance);
        if self.made.is_empty() {
            self.made = Vec::new();
        }
    }

    /// `plugin` stopped: nothing is told to it any more.
    pub(super) fn forget_made(&mut self, plugin: u64) {
        self.made.retain(|m| m.owner != plugin);
        if self.made.is_empty() {
            self.made = Vec::new();
        }
    }

    /// Notices waiting.
    #[cfg(test)]
    pub(crate) fn made_waiting(&self) -> usize {
        self.made.len()
    }
}
