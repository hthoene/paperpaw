//! The table of Thai, Lao or Khmer words a pack carries (slot 6, #313,
//! #314), for the line breaker (`crate::linebreak`), which is given no
//! font: per script the first pack opened with one is noted (its table's
//! head read for the script), and the table is read into memory
//! (some 16 KB for the store's, at most [`MAX_WORDS`]) the first time Thai
//! text is broken into lines, then held until a pack is trimmed or the
//! noted one is closed. A table that cannot be read (memory short, a card
//! pulled) counts as a failed read of every pack, so that a layout that
//! keeps pages keeps none made without it; it is tried again after a trim.
//!
//! The table is held in a static, which only the kernel may have (a native
//! plugin has no writable data, ADR 0024): built without the `kernel`
//! feature, no table is noted or read, and Thai breaks as without one.

#[cfg(all(not(target_os = "none"), any(test, feature = "kernel")))]
extern crate std;

use alloc::rc::{Rc, Weak};
use alloc::vec::Vec;
#[cfg(any(test, feature = "kernel"))]
use core::cell::RefCell;

use super::{PackSource, u32_le};
use crate::glyph::zeroed;
use crate::linebreak::Words;
use crate::linebreak::thai::Script;

/// The slot of the table of words.
pub(crate) const WORDS: usize = 6;
/// The most bytes a table of words may take.
const MAX_WORDS: usize = 64 * 1024;

/// What is held of a table of words.
enum Kept {
    /// Not read yet (or let go).
    Unread,
    Whole(Vec<u8>),
    /// Could not be read, or longer than a table may be: not tried again
    /// until a trim.
    Failed,
}

/// The pack whose words are read: its source and where the table is.
struct Noted {
    source: Weak<dyn PackSource>,
    at: u64,
    kept: Kept,
}

struct Held {
    /// Per script, in [`Script::ALL`]'s order.
    noted: [Option<Noted>; 3],
    /// Tables that could not be read now, wrapping.
    failures: u32,
}

#[cfg(any(test, feature = "kernel"))]
type Slot = RefCell<Held>;

#[cfg(any(test, feature = "kernel"))]
const NONE: Held = Held {
    noted: [None, None, None],
    failures: 0,
};

#[cfg(all(not(target_os = "none"), any(test, feature = "kernel")))]
std::thread_local! {
    /// Per thread, as fonts are.
    static HELD: Slot = const { RefCell::new(NONE) };
}

/// [`HELD`] on a device without an operating system.
#[cfg(all(target_os = "none", any(test, feature = "kernel")))]
struct OneThread(Slot);

// SAFETY: a device without an operating system sets and measures text on
// one thread only (its fonts are `Rc`s, which never leave it), never in an
// interrupt; nothing else reaches this.
#[cfg(all(target_os = "none", any(test, feature = "kernel")))]
unsafe impl Sync for OneThread {}

#[cfg(all(target_os = "none", any(test, feature = "kernel")))]
static HELD: OneThread = OneThread(RefCell::new(NONE));

#[cfg(all(not(target_os = "none"), any(test, feature = "kernel")))]
fn with_held<R>(f: impl FnOnce(&mut Held) -> R) -> Option<R> {
    HELD.try_with(|held| held.try_borrow_mut().ok().map(|mut held| f(&mut held)))
        .ok()
        .flatten()
}

#[cfg(all(target_os = "none", any(test, feature = "kernel")))]
fn with_held<R>(f: impl FnOnce(&mut Held) -> R) -> Option<R> {
    HELD.0.try_borrow_mut().ok().map(|mut held| f(&mut held))
}

/// Without the `kernel` feature: nothing held.
#[cfg(not(any(test, feature = "kernel")))]
fn with_held<R>(_: impl FnOnce(&mut Held) -> R) -> Option<R> {
    None
}

/// Notes the pack read from `source` if it has a table of words (at `at`,
/// 0: none), unless an open one of its script is noted.
pub(super) fn note(source: &Rc<dyn PackSource>, at: u32) {
    let mut head = [0u8; 2];
    if at == 0 || !source.read_at(u64::from(at), &mut head) {
        return;
    }
    let slot = usize::from(head[1]);
    if slot >= Script::ALL.len() {
        return;
    }
    with_held(|held| {
        let noted = &mut held.noted[slot];
        if noted.as_ref().is_none_or(|n| n.source.strong_count() == 0) {
            *noted = Some(Noted {
                source: Rc::downgrade(source),
                at: u64::from(at),
                kept: Kept::Unread,
            });
        }
    });
}

/// Lets go of the words held (they are read again when needed).
pub(super) fn trim() {
    with_held(|held| {
        for noted in held.noted.iter_mut().flatten() {
            noted.kept = Kept::Unread;
        }
    });
}

/// How many tables could not be read, wrapping.
pub(super) fn failures() -> u32 {
    with_held(|held| held.failures).unwrap_or(0)
}

/// `f` of the words of `script` of the noted pack, read if they are not
/// yet; `None` where there are none.
pub(crate) fn with_words<R>(script: Script, f: impl FnOnce(Words<'_>) -> R) -> Option<R> {
    let slot = Script::ALL.iter().position(|&s| s == script)?;
    with_held(|held| {
        let noted = held.noted[slot].as_mut()?;
        let Some(source) = noted.source.upgrade() else {
            // Closed: let go.
            held.noted[slot] = None;
            return None;
        };
        if let Kept::Unread = noted.kept {
            noted.kept = match read(&*source, noted.at) {
                Ok(bytes) => Kept::Whole(bytes),
                Err(failed) => {
                    held.failures = held.failures.wrapping_add(u32::from(failed));
                    Kept::Failed
                }
            };
        }
        match &noted.kept {
            Kept::Whole(bytes) => Words::new(bytes).map(f),
            _ => None,
        }
    })
    .flatten()
}

/// The table at `at` (checked where it is used, [`Words::new`]):
/// `Err(true)` where it could not be read now, `Err(false)` where it says
/// it is longer than a table may be.
fn read(source: &dyn PackSource, at: u64) -> Result<Vec<u8>, bool> {
    let mut head = [0u8; 8];
    if !source.read_at(at, &mut head) {
        return Err(true);
    }
    let len = u32_le(&head, 4) as usize;
    if !(8..=MAX_WORDS).contains(&len) {
        return Err(false);
    }
    let mut bytes = zeroed::<u8>(len).ok_or(true)?;
    match source.read_at(at, &mut bytes) {
        true => Ok(bytes),
        false => Err(true),
    }
}
