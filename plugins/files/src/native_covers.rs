//! The covers of the books of a list, free of host calls: which book has
//! its cover, which is being made, which has none. The host keeps the
//! pictures on the card and counts the plugin's image ids (at most
//! [`paperpaw_sdk::native::image::PREPARED_MAX`]), so a list holds a few
//! ids for the page shown and gives the rest back as it scrolls on.
use alloc::vec::Vec;
use paperpaw_sdk::native::{Error, image::ImageId};

/// Covers a list holds at once on a device of the small class (the X4).
pub const MOST_SMALL: usize = 3;
/// Covers a list holds at once elsewhere; the host allows eight ids and the
/// book information and the last-read card need a few.
pub const MOST: usize = 6;

/// What the cover of one book is up to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Slot {
    /// Not asked for (or given back: asked again when it shows).
    #[default]
    Idle,
    /// Being made by the host: asked again when `library_changed` says a
    /// cover is ready.
    Busy,
    /// The book has none, or it could not be had: a placeholder is drawn.
    Missing,
    /// Made: the image id to draw.
    Ready(ImageId),
}

/// The covers of the books of one window, by their index in it.
#[derive(Debug, Default)]
pub struct Covers {
    slots: Vec<Slot>,
    most: usize,
}

impl Covers {
    /// Covers for `books` books, `most` of them held at once.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(books: usize, most: usize) -> Result<Self, Error> {
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(books)
            .map_err(|_| Error::NoMemory)?;
        slots.resize(books, Slot::Idle);
        Ok(Self { slots, most })
    }

    /// The cover of book `at`; idle for one that is not in the window.
    #[must_use]
    pub fn slot(&self, at: usize) -> Slot {
        self.slots.get(at).copied().unwrap_or(Slot::Missing)
    }

    /// How many ids are held.
    #[must_use]
    pub fn live(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| matches!(slot, Slot::Ready(_)))
            .count()
    }

    /// Whether another cover may be asked for: ids are held up to the limit.
    #[must_use]
    pub fn has_room(&self) -> bool {
        self.live() < self.most
    }

    /// Notes what became of the ask for book `at`.
    pub fn set(&mut self, at: usize, slot: Slot) {
        if let Some(held) = self.slots.get_mut(at) {
            *held = slot;
        }
    }

    /// A cover is ready somewhere (`library_changed` bit 6): the books that
    /// waited for one ask again. Whether any did.
    pub fn ready_somewhere(&mut self) -> bool {
        let mut any = false;
        for slot in &mut self.slots {
            if *slot == Slot::Busy {
                *slot = Slot::Idle;
                any = true;
            }
        }
        any
    }

    /// The host ended the image of book `at` (a draw found it gone): the
    /// book asks again.
    pub fn ended(&mut self, at: usize) {
        if matches!(self.slot(at), Slot::Ready(_)) {
            self.set(at, Slot::Idle);
        }
    }

    /// Lets go of the covers of the books outside `keep` (the rows the page
    /// shows), handing each id to `give_back` for the host. They are asked for
    /// again if the rows come back. Nothing is allocated.
    pub fn drop_outside(&mut self, keep: &[usize], mut give_back: impl FnMut(ImageId)) {
        for (at, slot) in self.slots.iter_mut().enumerate() {
            if let Slot::Ready(id) = *slot
                && !keep.contains(&at)
            {
                give_back(id);
                *slot = Slot::Idle;
            }
        }
    }

    /// Lets go of every cover, as [`Self::drop_outside`].
    pub fn drop_all(&mut self, give_back: impl FnMut(ImageId)) {
        self.drop_outside(&[], give_back);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;

    fn id(n: u32) -> ImageId {
        ImageId::new(n).unwrap()
    }

    #[test]
    fn covers_are_held_up_to_the_limit_and_given_back_as_rows_scroll_out() {
        let mut covers = Covers::new(6, 3).unwrap();
        for at in 0..3 {
            assert!(covers.has_room());
            covers.set(at, Slot::Ready(id(at as u32 + 1)));
        }
        assert!(!covers.has_room());
        // The page moves on to books 3 and 4: the first three go back.
        let mut ids = Vec::new();
        covers.drop_outside(&[3, 4], |id| ids.push(id));
        assert_eq!(ids, [id(1), id(2), id(3)]);
        assert_eq!(covers.live(), 0);
        assert_eq!(covers.slot(0), Slot::Idle);
        // A cover that stays on the page is kept.
        covers.set(3, Slot::Ready(id(4)));
        covers.drop_outside(&[3], |id| panic!("{id:?} kept"));
        assert_eq!(covers.slot(3), Slot::Ready(id(4)));
        let mut last = Vec::new();
        covers.drop_all(|id| last.push(id));
        assert_eq!(last, [id(4)]);
    }

    #[test]
    fn a_cover_being_made_is_asked_again_when_the_host_says_one_is_ready() {
        let mut covers = Covers::new(3, 6).unwrap();
        covers.set(1, Slot::Busy);
        covers.set(2, Slot::Missing);
        assert!(covers.ready_somewhere());
        assert_eq!(covers.slot(1), Slot::Idle);
        // A book without a cover is not asked about again.
        assert_eq!(covers.slot(2), Slot::Missing);
        assert!(!covers.ready_somewhere());
    }

    #[test]
    fn an_image_the_host_ended_is_asked_for_again() {
        let mut covers = Covers::new(2, 6).unwrap();
        covers.set(0, Slot::Ready(id(7)));
        covers.ended(0);
        assert_eq!(covers.slot(0), Slot::Idle);
        covers.ended(5);
        assert_eq!(covers.slot(9), Slot::Missing);
    }
}
