//! What the firmware's HTTP client does for a download that is worth it
//! (#216, `shells/xteink` `http.rs`): no interface, only what the shell sees
//! of a request (it saves to a file, and the file is large).
//!
//! * The radio's power saving stays off a little after the last request
//!   ([`PowerHold`]): the next file of a plugin or language pack asks within
//!   a second, and a radio that dozes between the beacons would answer its
//!   first packets late.
//! * A large download gets a larger receive window and a larger write chunk
//!   ([`plan`]), only while it runs (both belong to its connection and its
//!   sink, freed with them), and only where the heap keeps a block of
//!   [`KEEP_LARGEST`] bytes after them.
//! * A connection of a download that ended cleanly is kept a few seconds for
//!   the next request to the same server ([`may_keep`], [`IDLE_MS`]): no new
//!   TCP and TLS handshake (which takes the C3 seconds) for every file.

/// How long the radio's power saving stays off after the last request or
/// server answer.
pub const POWER_HOLD_MS: u64 = 2_000;

/// The radio's power saving, with a hold after the last data moved.
#[derive(Debug, Default, Clone, Copy)]
pub struct PowerHold {
    until: u64,
}

impl PowerHold {
    pub const fn new() -> Self {
        Self { until: 0 }
    }

    /// Whether the radio may save power at `now_ms`, given whether data
    /// `busy` moves (or a connection is open). Evaluated every step, so a
    /// request that failed or was dropped lets go the same way as one that
    /// ended: `busy` is false, and after the hold the radio saves again.
    pub fn saving(&mut self, busy: bool, now_ms: u64) -> bool {
        if busy {
            self.until = now_ms.saturating_add(POWER_HOLD_MS);
            return false;
        }
        now_ms >= self.until
    }
}

/// The largest block the heap keeps free after a download's extra buffers
/// (a secure session takes about 40 KB at once, `tls_gate`).
pub const KEEP_LARGEST: usize = 40 * 1024;

/// A file of at least this many bytes is a download worth buffers.
pub const BOOST_MIN_FILE: usize = 32 * 1024;

/// A connection's receive window and the sink's write chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub receive: usize,
    pub chunk: usize,
    /// Larger than the base, to be given back with the connection.
    pub boosted: bool,
}

/// The write chunk of a download that gets no boost: the card takes a
/// command for each chunk (a wait of its own), so 8 KiB halve them against
/// 4 KiB (#419). A multiple of the card's block, as every chunk is.
pub const BASE_CHUNK: usize = 8 * 1024;

/// The chunk where the heap has no room for [`BASE_CHUNK`].
pub const MIN_CHUNK: usize = 4 * 1024;

/// The window and chunk of a download: the `base` window, or on a large
/// file (`file`, a limit that is the size the plugin asked for) that is the
/// only boosted one (`free_boost`), the larger pair of the device (`small`:
/// the X4's C3) where `largest`, the largest free block of the heap before
/// the connection's buffers, leaves [`KEEP_LARGEST`] after both.
pub const fn plan(
    base: usize,
    saves: bool,
    file: usize,
    free_boost: bool,
    small: bool,
    largest: usize,
) -> Plan {
    let (receive, chunk) = if small {
        (24 * 1024, 8 * 1024)
    } else {
        (32 * 1024, 16 * 1024)
    };
    let worth = saves && file >= BOOST_MIN_FILE && free_boost && receive > base;
    if worth && largest >= receive + chunk + KEEP_LARGEST {
        Plan {
            receive,
            chunk,
            boosted: true,
        }
    } else {
        Plan {
            receive: base,
            chunk: BASE_CHUNK,
            boosted: false,
        }
    }
}

/// How long a connection waits idle for the next request to its server
/// (below the 5 s of Apache's default keep-alive timeout).
pub const IDLE_MS: u64 = 3_000;

/// Whether a connection that carried a request may be kept for the next:
/// a `GET` whose answer came whole by its length or in chunks (not up to
/// the connection's end) and said nothing of closing, with a status that
/// is no redirect (a redirect leads elsewhere).
pub fn may_keep(method: &str, keep_alive: bool, done: bool, delimited: bool, status: u16) -> bool {
    method.eq_ignore_ascii_case("GET")
        && keep_alive
        && done
        && delimited
        && (200..300).contains(&status)
}

/// Whether a kept connection is let go at `now_ms`: it waited long enough,
/// or someone else needs a secure session (`pressed`), or the other side
/// ended it (`ended`).
pub const fn drop_idle(since_ms: u64, now_ms: u64, pressed: bool, ended: bool) -> bool {
    pressed || ended || now_ms.saturating_sub(since_ms) >= IDLE_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_saving_returns_after_the_hold_also_when_the_request_failed() {
        let mut hold = PowerHold::new();
        // Idle at the start: saves.
        assert!(hold.saving(false, 10_000));
        // A download moves: never saves.
        assert!(!hold.saving(true, 10_100));
        assert!(!hold.saving(true, 12_000));
        // It failed (nothing moves any more): held for a while, then saves.
        assert!(!hold.saving(false, 12_500));
        assert!(!hold.saving(false, 13_999));
        assert!(hold.saving(false, 14_000));
        // The next request within the hold never let the radio doze.
        let mut hold = PowerHold::new();
        assert!(!hold.saving(true, 1_000));
        assert!(!hold.saving(false, 2_000));
        assert!(!hold.saving(true, 2_500));
        assert!(!hold.saving(false, 4_000));
        assert!(hold.saving(false, 4_500));
    }

    #[test]
    fn a_large_download_gets_the_larger_buffers_where_the_heap_keeps_its_block() {
        let base = 8 * 1024;
        let big = 200 * 1024;
        let plenty = 24 * 1024 + 8 * 1024 + KEEP_LARGEST;
        let boosted = plan(base, true, big, true, true, plenty);
        assert_eq!(
            (boosted.receive, boosted.chunk, boosted.boosted),
            (24 * 1024, 8 * 1024, true)
        );
        // One byte short of what it must keep: the base.
        let short = plan(base, true, big, true, true, plenty - 1);
        assert_eq!(
            short,
            Plan {
                receive: base,
                chunk: BASE_CHUNK,
                boosted: false
            }
        );
        // Not a file, a small file, or a second boosted one: the base.
        assert!(!plan(base, false, big, true, true, plenty).boosted);
        assert!(!plan(base, true, BOOST_MIN_FILE - 1, true, true, plenty).boosted);
        assert!(!plan(base, true, big, false, true, plenty).boosted);
        // The larger device's pair.
        let pro = plan(16 * 1024, true, big, true, false, 200 * 1024);
        assert_eq!((pro.receive, pro.chunk), (32 * 1024, 16 * 1024));
    }

    #[test]
    fn what_a_boosted_download_takes_never_leaves_less_than_the_kept_block() {
        for largest in 0..(120 * 1024) {
            let p = plan(8 * 1024, true, 1 << 20, true, true, largest);
            if p.boosted {
                assert!(largest - p.receive - p.chunk >= KEEP_LARGEST);
            }
        }
    }

    #[test]
    fn only_a_clean_get_is_kept() {
        assert!(may_keep("GET", true, true, true, 200));
        assert!(!may_keep("POST", true, true, true, 200));
        assert!(!may_keep("GET", false, true, true, 200), "said close");
        assert!(!may_keep("GET", true, false, true, 200), "not whole");
        assert!(!may_keep("GET", true, true, false, 200), "up to the end");
        assert!(!may_keep("GET", true, true, true, 302), "a redirect");
        assert!(!may_keep("GET", true, true, true, 404));
    }

    #[test]
    fn an_idle_connection_goes_after_its_time_or_when_needed() {
        assert!(!drop_idle(1_000, 1_000 + IDLE_MS - 1, false, false));
        assert!(drop_idle(1_000, 1_000 + IDLE_MS, false, false));
        assert!(drop_idle(1_000, 1_001, true, false));
        assert!(drop_idle(1_000, 1_001, false, true));
    }
}
