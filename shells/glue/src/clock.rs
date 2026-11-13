//! The clock of the host table: microseconds since some moment that stays.
//! A computer has its own; a browser hands one in ([`set_micros`]).

use std::sync::OnceLock;

/// Microseconds since some moment that stays; they never go back.
pub type Micros = fn() -> u64;

static CLOCK: OnceLock<Micros> = OnceLock::new();

/// Takes the time from `clock` (the shell's: a browser's `performance.now()`);
/// the first one set stays.
pub fn set_micros(clock: Micros) {
    let _ = CLOCK.set(clock);
}

/// Microseconds as the shell's clock counts them: the one set, or the
/// process's own (0 in a browser before the shell set one).
#[must_use]
pub fn micros() -> u64 {
    match CLOCK.get() {
        Some(clock) => clock(),
        None => own_micros(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn own_micros() -> u64 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    u64::try_from(
        START
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_micros(),
    )
    .unwrap_or(u64::MAX)
}

#[cfg(target_arch = "wasm32")]
fn own_micros() -> u64 {
    0
}

/// A moment of [`micros`], to tell how long ago it was (the std `Instant`
/// cannot be read in a browser).
#[derive(Clone, Copy, Debug)]
pub struct Moment(i64);

impl Moment {
    /// Now.
    #[must_use]
    pub fn now() -> Self {
        Self(i64::try_from(micros()).unwrap_or(i64::MAX))
    }

    /// Microseconds since this moment (never less than 0).
    #[must_use]
    pub fn elapsed_micros(self) -> u64 {
        let now = i64::try_from(micros()).unwrap_or(i64::MAX);
        u64::try_from(now.saturating_sub(self.0)).unwrap_or(0)
    }
}
