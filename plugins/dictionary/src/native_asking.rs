//! The providers a panel waits for (todo 98): one that must fetch answers its
//! page `pending` at once and tells the askers with `event.lookup_ready` when
//! its answer is kept (`docs/abi/document.md` 7). The panel says *Looking up …*
//! while any is waited for, looks the word up again when one is ready (it then
//! answers from what it kept), and stops waiting after [`ANSWER_MS`], as Beta 3
//! did. Without the host.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{mem::push, text};

/// How long a panel waits for the providers that answer later, from the
/// lookup that asked them (Beta 3's `ANSWER_MS`).
pub const ANSWER_MS: u32 = 20_000;

/// The providers a panel waits for, by name.
#[derive(Debug, Default)]
pub struct Asking {
    waits: Vec<String>,
}

impl Asking {
    /// Waits for `provider` (once, however many pages it answers pending).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn wait(&mut self, provider: &str) -> Result<(), Error> {
        if self.waits.iter().any(|name| name == provider) {
            return Ok(());
        }
        push(&mut self.waits, text::copy(provider)?)
    }

    /// Whether `provider` was waited for; it is not any more.
    pub fn answered(&mut self, provider: &str) -> bool {
        let Some(at) = self.waits.iter().position(|name| name == provider) else {
            return false;
        };
        self.waits.remove(at);
        true
    }

    /// Waits for none any more (the time is up, or the word is looked up anew).
    pub fn give_up(&mut self) {
        self.waits = Vec::new();
    }

    /// Whether any provider is waited for.
    #[must_use]
    pub fn waiting(&self) -> bool {
        !self.waits.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_provider_is_waited_for_once_until_it_answers_or_the_time_is_up() {
        let mut asking = Asking::default();
        assert!(!asking.waiting());
        asking.wait("summary").unwrap();
        asking.wait("summary").unwrap();
        asking.wait("translate").unwrap();
        assert!(asking.waiting());
        assert!(asking.answered("summary"));
        assert!(!asking.answered("summary"), "answered once");
        assert!(!asking.answered("other"), "never asked");
        assert!(asking.waiting(), "translate still");
        asking.give_up();
        assert!(!asking.waiting());
        assert!(!asking.answered("translate"));
    }

    #[test]
    fn beta_3_waited_twenty_seconds() {
        assert_eq!(ANSWER_MS, 20_000);
    }
}
