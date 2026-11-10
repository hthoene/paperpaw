//! The notices that wait (`docs/abi/ui.md` section 7, "Messages"): a notice
//! is shown one at a time, over any screen, and never over a guarding
//! instance; those that come meanwhile wait in the order they came, at most
//! four (two on the X4), the oldest dropped when one more comes. Without a
//! guard a new notice replaces the one on screen.
use alloc::collections::VecDeque;
use alloc::string::String;
use paperpaw_platform::MemoryClass;

/// Notices that wait at most.
const LARGE: usize = 4;
/// Notices that wait at most on a device of the small memory class (the X4,
/// and a class to come until it is decided).
const SMALL: usize = 2;

/// The waiting notices, oldest first.
#[derive(Debug)]
pub(crate) struct Waiting {
    texts: VecDeque<String>,
    cap: usize,
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
