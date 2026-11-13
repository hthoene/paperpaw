//! The notices that wait (`docs/abi/ui.md` section 7, "Messages"): a notice
//! is shown one at a time, over any screen, and never over a guarding
//! instance; those that come meanwhile wait in the order they came, at most
//! four (two on the X4), the oldest dropped when one more comes. Without a
//! guard a new notice replaces the one on screen.
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::String;
use paperpaw_platform::MemoryClass;
use paperpaw_sdk::Screen;

/// Notices that wait at most.
const LARGE: usize = 4;
/// Notices that wait at most on a device of the small memory class (the X4,
/// and a class to come until it is decided).
const SMALL: usize = 2;

/// The waiting notices, oldest first, and the notice made ahead for a lack
/// of memory (`memory_notice`).
pub(crate) struct Waiting {
    texts: VecDeque<String>,
    cap: usize,
    /// "Not enough memory", made while there was room; `None` while it shows.
    memory: Option<Box<dyn Screen>>,
    /// The notice showing is the one above: it comes back when it ends.
    memory_shown: bool,
    /// How often one was made (tests tell a new one from the old).
    #[cfg(test)]
    made: u32,
}

impl core::fmt::Debug for Waiting {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Waiting")
            .field("texts", &self.texts)
            .field("cap", &self.cap)
            .field("memory", &self.memory.is_some())
            .field("memory_shown", &self.memory_shown)
            .finish()
    }
}

impl Waiting {
    pub(crate) fn new(memory: MemoryClass) -> Self {
        let cap = if memory == MemoryClass::Large {
            LARGE
        } else {
            SMALL
        };
        Self {
            texts: VecDeque::new(),
            cap,
            memory: None,
            memory_shown: false,
            #[cfg(test)]
            made: 0,
        }
    }

    /// Keeps `notice` as the one for a lack of memory, in place of the one
    /// before; one showing now is not taken back when it ends.
    pub(crate) fn keep_memory_notice(&mut self, notice: Option<Box<dyn Screen>>) {
        #[cfg(test)]
        {
            self.made += u32::from(notice.is_some());
        }
        self.memory = notice;
        self.memory_shown = false;
    }

    /// Whether one is made, or shows.
    pub(crate) fn has_memory_notice(&self) -> bool {
        self.memory.is_some() || self.memory_shown
    }

    /// The notice for a lack of memory, to show; it comes back through
    /// [`Self::ended`].
    pub(crate) fn show_memory_notice(&mut self) -> Option<Box<dyn Screen>> {
        let notice = self.memory.take()?;
        self.memory_shown = true;
        Some(notice)
    }

    /// Whether one is made and does not show.
    pub(crate) fn memory_notice_ready(&self) -> bool {
        self.memory.is_some()
    }

    /// Where the one made ahead is, to tell it from another notice.
    #[cfg(test)]
    pub(crate) fn memory_notice_at(&self) -> Option<*const u8> {
        self.memory
            .as_deref()
            .map(|n| core::ptr::from_ref(n).cast::<u8>())
    }

    /// How often one was made.
    #[cfg(test)]
    pub(crate) fn memory_notices_made(&self) -> u32 {
        self.made
    }

    /// The notice that showed ended: the one for a lack of memory is kept for
    /// the next time, any other dropped.
    pub(crate) fn ended(&mut self, notice: Box<dyn Screen>) {
        if core::mem::take(&mut self.memory_shown) {
            self.memory = Some(notice);
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.texts.is_empty()
    }

    /// Lets `text` wait. At the cap the oldest is dropped; where the heap has
    /// no room for the notice it is dropped itself (it is a notice, not data).
    pub(crate) fn push(&mut self, text: String) {
        if self.texts.len() >= self.cap {
            self.texts.pop_front();
        }
        if self.texts.try_reserve(1).is_ok() {
            self.texts.push_back(text);
        }
    }

    /// The oldest waiting notice.
    pub(crate) fn pop(&mut self) -> Option<String> {
        self.texts.pop_front()
    }

    #[cfg(test)]
    pub(crate) fn texts(&self) -> alloc::vec::Vec<&str> {
        self.texts.iter().map(String::as_str).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(n: u32) -> String {
        alloc::format!("n{n}")
    }

    #[test]
    fn the_oldest_are_dropped_past_four_and_two_on_the_small_class() {
        for (memory, cap) in [(MemoryClass::Large, 4), (MemoryClass::Small, 2)] {
            let mut waiting = Waiting::new(memory);
            for n in 0..6 {
                waiting.push(text(n));
            }
            assert_eq!(waiting.texts().len(), cap);
            assert_eq!(waiting.pop(), Some(text(6 - cap as u32)));
            assert!(!waiting.is_empty());
        }
    }

    #[test]
    fn they_come_out_in_the_order_they_went_in() {
        let mut waiting = Waiting::new(MemoryClass::Large);
        for n in 0..3 {
            waiting.push(text(n));
        }
        assert_eq!(waiting.pop().as_deref(), Some("n0"));
        assert_eq!(waiting.pop().as_deref(), Some("n1"));
        assert_eq!(waiting.pop().as_deref(), Some("n2"));
        assert_eq!(waiting.pop(), None);
        assert!(waiting.is_empty());
    }
}
