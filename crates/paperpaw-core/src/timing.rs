//! How long what an input started took (opening or closing a screen,
//! running an action), for the busy mark (see `Kernel::mark_if_slow`):
//! kept across starts in [`TIMING_FILE`], since on the X4 every wake is a
//! start (#118).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::Kernel;

/// Work that took this long last time shows the busy mark first, unless
/// the shell says otherwise (`KernelBuilder::busy_mark_after`): as long as
/// the X4's panel takes to show the mark (~504 ms), so that the mark
/// costs nothing where it shows; shorter work would wait for it.
pub(crate) const SLOW_MS: u32 = 500;

/// What was timed is remembered for this many things at most.
pub(crate) const TIMED: usize = 24;

/// Where the times are kept across starts: a first line [`HEAD`], then a
/// line `<ms> <key>` for each.
const TIMING_FILE: &str = ".paperpaw/core/timing.txt";

/// The first line of [`TIMING_FILE`]: a file of another kind or version
/// holds no times.
const HEAD: &str = "paperpaw-timing 1";

/// Largest [`TIMING_FILE`] read.
const MAX_FILE: usize = 4096;

/// Longest key kept.
const MAX_KEY: usize = 200;

/// How long what a call opened, closed or ran takes.
#[derive(Debug)]
pub(crate) struct Timing {
    /// What the call opened, closed or ran, and when it began (the clock's
    /// microseconds), until the call is done.
    pub(crate) running: Option<(String, u64)>,
    /// How long that took the last time, in milliseconds, the latest last.
    durations: Vec<(String, u32)>,
    /// Work that took this long is slow.
    slow_ms: u32,
    /// What is kept changed since it was read or written: a key came, or
    /// one went from slow to quick or back.
    changed: bool,
}

impl Default for Timing {
    fn default() -> Self {
        Self::new(SLOW_MS)
    }
}

impl Timing {
    /// Nothing timed yet, work of `slow_ms` or longer being slow.
    pub(crate) fn new(slow_ms: u32) -> Self {
        Self {
            running: None,
            durations: Vec::new(),
            slow_ms,
            changed: false,
        }
    }

    /// Whether the work of `key` is slow: it took [`Timing::slow_ms`] or
    /// longer the last time. Not timed yet, opening a screen may take long;
    /// going back (the screen below is there) and running an action are
    /// taken to be quick until they were not.
    pub(crate) fn slow(&self, key: &str) -> bool {
        match self.durations.iter().find(|(k, _)| *k == key) {
            Some((_, ms)) => *ms >= self.slow_ms,
            None => !(key.starts_with("run:") || key.starts_with("back:")),
        }
    }

    /// The work of `key` took `ms`.
    pub(crate) fn note(&mut self, key: String, ms: u32) {
        let before = self.durations.iter().position(|(k, _)| *k == key);
        let was = before.map(|i| self.durations.remove(i).1);
        if self.durations.len() >= TIMED {
            self.durations.remove(0);
        }
        self.changed |= was.is_none_or(|was| (was >= self.slow_ms) != (ms >= self.slow_ms));
        self.durations.push((key, ms));
    }

    /// The times kept in `text`, [`TIMING_FILE`]'s: none where its first
    /// line is not [`HEAD`]; lines that are not `<ms> <key>` are skipped,
    /// the last [`TIMED`] kept.
    pub(crate) fn read(&mut self, text: &str) {
        let mut lines = text.lines();
        if lines.next().map(str::trim) != Some(HEAD) {
            return;
        }
        let mut read: Vec<(String, u32)> = Vec::new();
        for line in lines {
            let Some((ms, key)) = line.trim().split_once(' ') else {
                continue;
            };
            let (Ok(ms), key) = (ms.parse::<u32>(), key.trim()) else {
                continue;
            };
            if key.is_empty() || key.len() > MAX_KEY || key.contains(char::is_whitespace) {
                continue;
            }
            read.retain(|(k, _)| k != key);
            read.push((String::from(key), ms));
        }
        let skip = read.len().saturating_sub(TIMED);
        self.durations = read.split_off(skip);
        self.changed = false;
    }

    /// The text of [`TIMING_FILE`] for the times kept.
    pub(crate) fn text(&self) -> String {
        let mut text = format!("{HEAD}\n");
        for (key, ms) in &self.durations {
            text.push_str(&format!("{ms} {key}\n"));
        }
        text
    }
}

impl Kernel {
    /// Notes how long the work timed took, once the call is done (with the
    /// mark shown, once it went on).
    pub(crate) fn timed(&mut self) {
        if self.frame.mark.is_some() {
            return;
        }
        let (Some((key, started)), Some(now)) =
            (self.timing.running.take(), self.shared.meter.micros())
        else {
            return;
        };
        let ms = (now.saturating_sub(started) / 1000).min(u64::from(u32::MAX)) as u32;
        self.timing.note(key, ms);
    }

    /// The times kept at the last start, read before the first input: a
    /// file missing, damaged or of another version holds none.
    pub(crate) fn restore_timing(&mut self) {
        let storage = self.shared.storage.as_ref();
        if let Ok(bytes) = paperpaw_sdk::read_file(storage, TIMING_FILE, MAX_FILE) {
            self.timing.read(&String::from_utf8_lossy(&bytes));
        }
    }

    /// Keeps the times for the next start, before the device sleeps or
    /// stops: written only where they changed what is slow, so mostly not.
    pub(crate) fn keep_timing(&mut self) {
        if !self.timing.changed {
            return;
        }
        let text = self.timing.text();
        if self
            .shared
            .storage
            .write(TIMING_FILE, text.as_bytes())
            .is_ok()
        {
            self.timing.changed = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn going_back_and_actions_are_quick_and_screens_slow_until_timed() {
        let mut timing = Timing::default();
        assert!(timing.slow("screen:settings.group"));
        assert!(!timing.slow("back:settings.list"));
        assert!(!timing.slow("run:core.library"));
        timing.note(String::from("screen:settings.group"), 173);
        timing.note(String::from("back:settings.list"), 620);
        assert!(!timing.slow("screen:settings.group"));
        assert!(timing.slow("back:settings.list"));
    }

    #[test]
    fn work_as_long_as_the_marks_refresh_is_slow_and_shorter_is_not() {
        let mut timing = Timing::default();
        timing.note(String::from("screen:a"), 450);
        timing.note(String::from("screen:b"), 500);
        assert!(!timing.slow("screen:a"));
        assert!(timing.slow("screen:b"));
        // A shell whose panel is quicker says so.
        let mut quick = Timing::new(300);
        quick.note(String::from("screen:a"), 450);
        assert!(quick.slow("screen:a"));
    }

    #[test]
    fn times_read_back_are_those_written_and_change_only_with_slowness() {
        let mut timing = Timing::default();
        timing.note(String::from("screen:a"), 173);
        timing.note(String::from("back:b"), 900);
        assert!(timing.changed);
        let mut again = Timing::default();
        again.read(&timing.text());
        assert!(!again.changed);
        assert!(!again.slow("screen:a"));
        assert!(again.slow("back:b"));
        // Quick again, still quick: nothing to write.
        again.note(String::from("screen:a"), 180);
        assert!(!again.changed);
        again.note(String::from("screen:a"), 800);
        assert!(again.changed);
    }

    #[test]
    fn a_damaged_or_other_file_holds_no_times_and_fails_nothing() {
        for text in [
            "",
            "paperpaw-timing 2\n10 screen:a\n",
            "10 screen:a\n",
            "\u{0}\u{1}garbage",
        ] {
            let mut timing = Timing::default();
            timing.read(text);
            assert!(timing.durations.is_empty(), "{text:?}");
        }
        let mut timing = Timing::default();
        let long = "k".repeat(MAX_KEY + 1);
        timing.read(&format!(
            "{HEAD}\nx screen:a\n12\n-3 screen:b\n99999999999 screen:c\n10  \n5 {long}\n7 screen:d\n"
        ));
        assert_eq!(timing.durations, [(String::from("screen:d"), 7)]);
    }

    #[test]
    fn no_more_than_the_times_kept_are_read() {
        let mut text = format!("{HEAD}\n");
        for n in 0..TIMED + 10 {
            text.push_str(&format!("{n} screen:{n}\n"));
        }
        let mut timing = Timing::default();
        timing.read(&text);
        assert_eq!(timing.durations.len(), TIMED);
        assert_eq!(timing.durations[0].0, "screen:10");
    }
}
