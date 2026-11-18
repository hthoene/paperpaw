//! The photos the host holds for the plugin, without the host: one for the
//! sleep screen, one for the slideshow and one for the widget. Each is asked
//! for (`image_prepare`, one request at a time per place) and answered once
//! (`event.image_ready`); the answer is told from an older one by its cookie.
//! A photo the host cannot decode is passed over for the next, a few times.
use paperpaw_sdk::native::image::{ImageId, ImageStatus};

/// Where a photo is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Sleep = 0,
    Show = 1,
    Widget = 2,
}

/// All places.
pub const SLOTS: [Slot; 3] = [Slot::Sleep, Slot::Show, Slot::Widget];

/// How many photos are tried one after the other for a place before it stays
/// without.
pub const TRIES: u8 = 6;

/// A photo the host decoded: its id, its size as decoded, and which photo of
/// the list it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Held {
    pub image: ImageId,
    pub size: (u32, u32),
    pub index: usize,
}

#[derive(Debug, Default)]
struct One {
    held: Option<Held>,
    /// The request open: its serial and the photo asked for.
    asked: Option<(u32, usize)>,
    tries: u8,
    /// A photo that could not be shown, until the list or a setting changes.
    failed: Option<usize>,
}

/// What an answer leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Nobody waits for it: give a picture it holds back.
    Ignore(Option<ImageId>),
    /// The photo is held; the one before (if any) is to be given back.
    Ready {
        slot: Slot,
        release: Option<ImageId>,
    },
    /// This photo cannot be shown: ask for the one after `index` if `again`.
    Failed {
        slot: Slot,
        index: usize,
        again: bool,
    },
    /// Not now (no memory, the card): the place asks again at the next tick.
    Later,
}

/// The photos of the three places.
#[derive(Debug, Default)]
pub struct Pics {
    ones: [One; 3],
    serial: u32,
}

impl Pics {
    fn one(&self, slot: Slot) -> &One {
        &self.ones[slot as usize]
    }

    fn one_mut(&mut self, slot: Slot) -> &mut One {
        &mut self.ones[slot as usize]
    }

    /// The photo held for `slot`.
    #[must_use]
    pub fn held(&self, slot: Slot) -> Option<Held> {
        self.one(slot).held
    }

    /// Whether to ask for photo `index` of `slot`: not held, not asked for,
    /// not known to be bad.
    #[must_use]
    pub fn wants(&self, slot: Slot, index: usize) -> bool {
        let one = self.one(slot);
        one.held.is_none_or(|held| held.index != index)
            && one.asked.is_none_or(|(_, asked)| asked != index)
            && one.failed != Some(index)
    }

    /// The request for photo `index` of `slot` is made: its cookie. `retry`
    /// is a try after a photo that failed (the count goes on), else it starts.
    pub fn asked(&mut self, slot: Slot, index: usize, retry: bool) -> u32 {
        self.serial = self.serial.wrapping_add(1) & 0x0fff_ffff;
        let serial = self.serial;
        let one = self.one_mut(slot);
        one.asked = Some((serial, index));
        one.tries = if retry {
            one.tries.saturating_add(1)
        } else {
            0
        };
        ((slot as u32) << 28) | serial
    }

    /// The host's answer for `cookie`.
    pub fn ready(
        &mut self,
        cookie: u32,
        status: ImageStatus,
        image: Option<ImageId>,
        size: (u32, u32),
    ) -> Answer {
        let slot = match cookie >> 28 {
            0 => Slot::Sleep,
            1 => Slot::Show,
            2 => Slot::Widget,
            _ => return Answer::Ignore(image),
        };
        let one = self.one_mut(slot);
        let Some((serial, index)) = one.asked else {
            return Answer::Ignore(image);
        };
        if serial != cookie & 0x0fff_ffff {
            return Answer::Ignore(image);
        }
        one.asked = None;
        match (status, image) {
            (ImageStatus::Ready, Some(image)) => {
                let release = one.held.map(|held| held.image);
                one.held = Some(Held { image, size, index });
                one.failed = None;
                Answer::Ready { slot, release }
            }
            (ImageStatus::NoMemory | ImageStatus::CardFailure, _) => Answer::Later,
            _ => {
                one.failed = Some(index);
                Answer::Failed {
                    slot,
                    index,
                    again: one.tries.saturating_add(1) < TRIES,
                }
            }
        }
    }

    /// The held photo of `slot` goes (the list or a setting changed); what
    /// was asked for is no longer waited for.
    pub fn forget(&mut self, slot: Slot) -> Option<ImageId> {
        let one = self.one_mut(slot);
        one.asked = None;
        one.failed = None;
        one.held.take().map(|held| held.image)
    }

    /// Forgets that photos failed (a new list may mean new files).
    pub fn forgive(&mut self) {
        for one in &mut self.ones {
            one.failed = None;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, reason = "test assertions")]
mod tests {
    use super::*;

    fn id(n: u32) -> ImageId {
        ImageId::new(n).unwrap()
    }

    #[test]
    fn an_answer_is_taken_only_for_the_request_open() {
        let mut pics = Pics::default();
        assert!(pics.wants(Slot::Show, 3));
        let cookie = pics.asked(Slot::Show, 3, false);
        assert!(!pics.wants(Slot::Show, 3));
        // An answer of another place or an older request is given back.
        let other = pics.asked(Slot::Widget, 1, false);
        assert_eq!(
            pics.ready(cookie ^ 1, ImageStatus::Ready, Some(id(9)), (1, 1)),
            Answer::Ignore(Some(id(9)))
        );
        assert_eq!(
            pics.ready(other, ImageStatus::Ready, Some(id(5)), (10, 20)),
            Answer::Ready {
                slot: Slot::Widget,
                release: None
            }
        );
        assert_eq!(
            pics.held(Slot::Widget),
            Some(Held {
                image: id(5),
                size: (10, 20),
                index: 1
            })
        );
        // The second photo of a place gives the first back.
        let next = pics.asked(Slot::Widget, 2, false);
        assert_eq!(
            pics.ready(next, ImageStatus::Ready, Some(id(6)), (1, 1)),
            Answer::Ready {
                slot: Slot::Widget,
                release: Some(id(5))
            }
        );
        // The answer comes once.
        assert_eq!(
            pics.ready(next, ImageStatus::Ready, Some(id(7)), (1, 1)),
            Answer::Ignore(Some(id(7)))
        );
    }

    #[test]
    fn a_photo_that_fails_is_passed_over_a_few_times_then_the_place_stays_without() {
        let mut pics = Pics::default();
        let mut index = 0;
        let mut cookie = pics.asked(Slot::Sleep, index, false);
        let mut seen = 0;
        loop {
            match pics.ready(cookie, ImageStatus::Damaged, None, (0, 0)) {
                Answer::Failed {
                    again, index: bad, ..
                } => {
                    seen += 1;
                    assert_eq!(bad, index);
                    assert!(!pics.wants(Slot::Sleep, bad));
                    if !again {
                        break;
                    }
                    index += 1;
                    cookie = pics.asked(Slot::Sleep, index, true);
                }
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(seen, usize::from(TRIES));
        // A memory shortage is not a bad photo.
        let cookie = pics.asked(Slot::Show, 0, false);
        assert_eq!(
            pics.ready(cookie, ImageStatus::NoMemory, None, (0, 0)),
            Answer::Later
        );
        assert!(pics.wants(Slot::Show, 0));
    }

    #[test]
    fn forgetting_gives_back_what_is_held_and_drops_what_was_asked() {
        let mut pics = Pics::default();
        let cookie = pics.asked(Slot::Show, 0, false);
        pics.ready(cookie, ImageStatus::Ready, Some(id(2)), (4, 4));
        let late = pics.asked(Slot::Show, 1, false);
        assert_eq!(pics.forget(Slot::Show), Some(id(2)));
        assert_eq!(pics.held(Slot::Show), None);
        assert_eq!(
            pics.ready(late, ImageStatus::Ready, Some(id(3)), (4, 4)),
            Answer::Ignore(Some(id(3)))
        );
    }
}
