//! What every part of the plugin shares: the phase, whether it runs and
//! when it ends, the rounds of the set and the day's counts. Kept in a
//! small file, so that switching the device off does not lose a round.

use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::time::{local_seconds, unix_seconds};
use paperpaw_sdk::{Command, Env, Instant};

use crate::phase::{DAYS, Log, Phase};
use crate::{DONE, lengths};

/// The state, shared by the screen, the widgets and the timer's event.
pub type Shared = Rc<RefCell<State>>;

#[derive(Debug)]
pub struct State {
    pub phase: Phase,
    pub running: bool,
    /// When the phase ends while running.
    pub end: Option<Instant>,
    /// Milliseconds left while paused; 0 for a phase not begun, which has
    /// its whole length as the settings say now.
    pub left_ms: u64,
    /// Focus rounds finished in the current set.
    pub done: u32,
    /// Seconds since 1970 when the phase ends, while running: kept so a
    /// restart can go on where it was.
    pub end_unix: Option<i64>,
    pub log: Log,
}

/// Where the state is kept, in the plugin's own folder.
fn save_path() -> String {
    format!("{}/state.txt", Env::plugin_dir("pomodoro"))
}

/// Today: days since 1970 by the local time, if the device knows it.
pub fn today(env: &Env<'_>) -> Option<i32> {
    let unix = unix_seconds(env)?;
    i32::try_from(local_seconds(env, unix).div_euclid(86_400)).ok()
}

impl State {
    pub fn new() -> Self {
        State {
            phase: Phase::Focus,
            running: false,
            end: None,
            left_ms: 0,
            done: 0,
            end_unix: None,
            log: Log::default(),
        }
    }

    /// Milliseconds left, of a phase `whole` milliseconds long.
    pub fn remaining(&self, now: Instant, whole: u64) -> u64 {
        match (self.running, self.end) {
            (true, Some(end)) => end.since(now),
            _ if self.left_ms == 0 => whole,
            _ => self.left_ms.min(whole),
        }
    }

    /// Text of the kept state: `phase|running|end|left|done|day|counts`.
    fn text(&self) -> String {
        let end = if self.running {
            self.end_unix.unwrap_or(0)
        } else {
            0
        };
        let counts: Vec<String> = self.log.counts().iter().map(|c| format!("{c}")).collect();
        format!(
            "{}|{}|{}|{}|{}|{}|{}",
            self.phase.code(),
            u8::from(self.running),
            end,
            self.left_ms,
            self.done,
            self.log.day(),
            counts.join(",")
        )
    }

    pub fn save(&self, env: &Env<'_>) {
        let _ = env.storage().write(&save_path(), self.text().as_bytes());
    }

    /// Reads kept state back; `None` where it is not ours or damaged.
    fn parse(text: &str) -> Option<Kept> {
        let mut parts = text.trim().split('|');
        let mut number = || parts.next()?.trim().parse::<i64>().ok();
        let phase = number()?;
        let running = number()?;
        let end = number()?;
        let left = number()?;
        let done = number()?;
        let day = number()?;
        let mut counts = [0u16; DAYS];
        let mut items = parts.next()?.split(',');
        for slot in &mut counts {
            *slot = items.next()?.trim().parse().ok()?;
        }
        if items.next().is_some() || parts.next().is_some() {
            return None;
        }
        Some(Kept {
            phase: Phase::from_code(phase),
            running: running == 1,
            end,
            left: left.max(0) as u64,
            done: done.clamp(0, 99) as u32,
            day: i32::try_from(day).ok()?,
            counts,
        })
    }

    /// Reads the kept state back at start-up. A round that was running
    /// goes on by the wall clock; one that ended meanwhile moves on.
    pub fn load(&mut self, env: &Env<'_>, out: &mut Vec<Command>) {
        let mut buf = [0u8; 160];
        let Ok(n) = env.storage().read_at(&save_path(), 0, &mut buf) else {
            return;
        };
        let Some(kept) = core::str::from_utf8(&buf[..n]).ok().and_then(Self::parse) else {
            return;
        };
        let lengths = lengths(env);
        self.phase = kept.phase;
        self.done = kept.done;
        self.log = Log::kept(kept.day, kept.counts);
        self.log.roll(today(env));
        self.running = false;
        let whole = lengths.ms(self.phase);
        self.left_ms = kept.left.min(whole);
        if !kept.running || kept.end <= 0 {
            return;
        }
        let Some(now) = unix_seconds(env) else {
            return;
        };
        if kept.end > now {
            self.left_ms = ((kept.end - now) as u64 * 1000).min(whole);
            self.start(env, out);
        } else {
            // Time ran out while the device was off.
            self.running = true;
            self.advance(env, true);
        }
    }

    pub fn start(&mut self, env: &Env<'_>, out: &mut Vec<Command>) {
        if self.running {
            return;
        }
        self.running = true;
        if self.left_ms == 0 {
            self.left_ms = lengths(env).ms(self.phase);
        }
        let end = env.now().add_millis(self.left_ms);
        self.end = Some(end);
        self.end_unix = unix_seconds(env).map(|s| s + (self.left_ms / 1000) as i64);
        out.push(DONE.schedule((), end));
        self.save(env);
    }

    pub fn pause(&mut self, env: &Env<'_>, out: &mut Vec<Command>) {
        if !self.running {
            return;
        }
        self.left_ms = self.remaining(env.now(), lengths(env).ms(self.phase));
        self.running = false;
        out.push(Command::CancelWork(DONE.id().clone(), None));
        self.save(env);
    }

    /// Starts or pauses.
    pub fn toggle(&mut self, env: &Env<'_>, out: &mut Vec<Command>) {
        if self.running {
            self.pause(env, out);
        } else {
            self.start(env, out);
        }
    }

    /// Stands at the start of `phase`, not running.
    pub fn enter(&mut self, env: &Env<'_>, phase: Phase) {
        self.phase = phase;
        self.running = false;
        self.end = None;
        self.left_ms = 0;
        self.save(env);
    }

    /// Finishes the phase (time is up, or skipped) and goes to the next.
    pub fn advance(&mut self, env: &Env<'_>, finished: bool) {
        if self.phase == Phase::Focus && finished {
            self.done += 1;
            self.log.roll(today(env));
            self.log.add();
        }
        let next = lengths(env).next(self.phase, self.done);
        if self.phase == Phase::Long {
            self.done = 0;
        }
        self.enter(env, next);
    }

    /// Back to the first round of a set.
    pub fn reset(&mut self, env: &Env<'_>) {
        self.done = 0;
        self.enter(env, Phase::Focus);
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

/// What the kept text said.
struct Kept {
    phase: Phase,
    running: bool,
    end: i64,
    left: u64,
    done: u32,
    day: i32,
    counts: [u16; DAYS],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_state_reads_back() {
        let mut state = State::new();
        state.phase = Phase::Short;
        state.left_ms = 123_000;
        state.done = 3;
        state.log = Log::kept(20_000, [2, 1, 0, 4, 0, 0, 9]);
        let kept = State::parse(&state.text()).unwrap();
        assert_eq!(kept.phase, Phase::Short);
        assert!(!kept.running);
        assert_eq!((kept.left, kept.done, kept.day), (123_000, 3, 20_000));
        assert_eq!(kept.counts, [2, 1, 0, 4, 0, 0, 9]);
    }

    #[test]
    fn damaged_text_is_left_alone() {
        for text in [
            "",
            "garbage",
            "0|0|0|1000|0|5",
            "0|0|0|1000|0|5|1,2,3",
            "0|0|0|1000|0|5|a,b,c,d,e,f,g",
            "0|0|0|1000|0|99999999999|0,0,0,0,0,0,0",
            "0|0|0|x|0|5|0,0,0,0,0,0,0",
            "0|0|0|1000|0|5|70000,0,0,0,0,0,0",
        ] {
            assert!(State::parse(text).is_none(), "{text:?}");
        }
    }

    #[test]
    fn odd_numbers_are_made_sane() {
        let kept = State::parse("9|1|-5|-100|500|7|0,0,0,0,0,0,0").unwrap();
        assert_eq!(kept.phase, Phase::Focus);
        assert_eq!(kept.left, 0);
        assert_eq!(kept.done, 99);
    }
}
