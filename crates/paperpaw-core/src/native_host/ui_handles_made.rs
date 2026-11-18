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
//! for the instance. An owner not started yet keeps its notice and is asked
//! to start; it is told once it runs. A notice ends at its first attempt
//! whatever came of it, as a drop's does, at once when its owner stops, and
//! with its instance: an instance dropped before its owner heard of it is
//! told neither (the owner keeps nothing of it). So at most one notice waits
//! per live instance; the list keeps no room once it is empty.
use super::{Dropped, UiHandles};
use alloc::{string::String, vec::Vec};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY};

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

/// Which notice: the one of an instance, or the `n`th waiting, oldest first.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Pick {
    Instance(u32),
    Nth(usize),
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
    /// its owner is to get (one per live instance at most, so the widget cap
    /// holds them). The instance's refusals, `ERR_NO_MEMORY` when the heap
    /// refuses: nothing is made, listed or kept on any refusal.
    pub(crate) fn make_told(
        &mut self,
        maker: u64,
        screen: u32,
        new: &NewWidget<'_>,
    ) -> Result<u32, i32> {
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
        let instance = match self.make_widget(new.owner, maker, screen, new.token, new.private) {
            Ok(instance) => instance,
            Err(error) => {
                // The room taken for the notice goes again with no notice.
                self.free_made_room();
                return Err(error);
            }
        };
        self.made.push(Made {
            owner: new.owner,
            token: new.token,
            instance,
            text,
            split,
        });
        Ok(instance)
    }

    /// The notice `pick` names, if it waits.
    pub(crate) fn next_made(&self, pick: Pick) -> Option<&Made> {
        match pick {
            Pick::Instance(id) => self.made.iter().find(|m| m.instance == id),
            Pick::Nth(n) => self.made.get(n),
        }
    }

    /// The first attempt to tell of `instance` is over, whatever came of it:
    /// its notice ends (the list's room too, once it is empty).
    pub(crate) fn made_told(&mut self, instance: u32) {
        self.made.retain(|m| m.instance != instance);
        self.free_made_room();
    }

    /// `plugin` stopped: nothing is told to it any more.
    pub(super) fn forget_made(&mut self, plugin: u64) {
        self.made.retain(|m| m.owner != plugin);
        self.free_made_room();
    }

    /// The list's room goes once nothing waits.
    pub(super) fn free_made_room(&mut self) {
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

/// An instance goes: its owner is to hear of the drop, unless it has not
/// heard of the instance yet, when its made notice goes and nothing is told.
/// Allocation-free (the room for the drop was taken with the instance); the
/// caller gives the list's room back ([`UiHandles::free_made_room`]).
pub(super) fn gone(made: &mut Vec<Made>, dropped: &mut Vec<Dropped>, notice: Dropped) {
    match made.iter().position(|m| m.instance == notice.instance) {
        Some(at) => {
            made.remove(at);
        }
        None => dropped.push(notice),
    }
}
