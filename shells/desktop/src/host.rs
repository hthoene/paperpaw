//! What the emulated device takes from the computer it runs on: the time
//! that passes and random numbers. On a computer with an operating system
//! they come from it; in a browser (`wasm32`), where neither is reachable
//! from Rust alone, the shell hands in where to read them
//! (`set_clock`, [`set_random`]).

/// Random bytes into the buffer; `false` where there are none.
pub type Random = fn(&mut [u8]) -> bool;

/// Microseconds since the emulator started: for the panel's animations,
/// the time plugins take and the frames traced.
#[cfg(not(target_arch = "wasm32"))]
pub fn micros() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_micros() as u64
}

/// Microseconds as the shell's clock counts them ([`set_clock`]), 0
/// before it set one.
#[cfg(target_arch = "wasm32")]
pub fn micros() -> u64 {
    CLOCK.get().map_or(0, |clock| clock())
}

/// Microseconds since some moment that stays; they never go back.
#[cfg(target_arch = "wasm32")]
pub type Clock = fn() -> u64;

#[cfg(target_arch = "wasm32")]
static CLOCK: std::sync::OnceLock<Clock> = std::sync::OnceLock::new();

/// Takes the time from `clock` where the operating system cannot tell
/// it (a browser's `performance.now()`); the first one set stays.
#[cfg(target_arch = "wasm32")]
pub fn set_clock(clock: Clock) {
    let _ = CLOCK.set(clock);
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

    /// This moment, `millis` earlier (for a test that needs time to have passed).
    #[cfg(test)]
    #[must_use]
    pub fn earlier_by_millis(self, millis: u64) -> Self {
        Self(
            self.0
                .saturating_sub(i64::try_from(millis * 1000).unwrap_or(i64::MAX)),
        )
    }
}

/// One line of the kernel's own diagnostics (`KernelBuilder::device_log`),
/// to the error stream; it keeps no state and is cheap.
pub fn device_log(line: &str) {
    use std::io::Write;
    // A closed stream must not panic the kernel: the error is ignored.
    let _ = writeln!(std::io::stderr(), "paperpaw: {line}");
}

/// Random bytes from the operating system, or from the source the shell
/// set ([`set_random`]).
pub fn random(buf: &mut [u8]) -> bool {
    match RANDOM.get() {
        Some(random) => random(buf),
        None => system_random(buf),
    }
}

static RANDOM: std::sync::OnceLock<Random> = std::sync::OnceLock::new();

/// Takes random numbers from `random` (a browser's
/// `crypto.getRandomValues`), where the operating system gives none.
/// Only the first call counts.
pub fn set_random(random: Random) {
    let _ = RANDOM.set(random);
}

#[cfg(not(target_arch = "wasm32"))]
fn system_random(buf: &mut [u8]) -> bool {
    getrandom::fill(buf).is_ok()
}

#[cfg(target_arch = "wasm32")]
fn system_random(_: &mut [u8]) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_goes_on_and_random_bytes_come() {
        let first = micros();
        let mut buf = [0u8; 32];
        assert!(random(&mut buf));
        assert!(buf.iter().any(|&b| b != 0));
        assert!(micros() >= first);
    }
}
