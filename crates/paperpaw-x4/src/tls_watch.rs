//! How long a TLS socket may take: the wait for a free session, the
//! connection and handshake, and then how long it may sit idle
//! ([`Tls::timeout_ms`], [`Tls::idle_ms`]).
//!
//! [`Tls::timeout_ms`]: paperpaw_sdk::Tls::timeout_ms
//! [`Tls::idle_ms`]: paperpaw_sdk::Tls::idle_ms

use paperpaw_sdk::SocketError;

/// Where a TLS socket is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// In line for a session.
    Queued,
    /// Holding one: connecting, or in the handshake.
    Securing,
    /// Secure: open for the plugin.
    Secure,
}

/// A TLS socket's clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Watch {
    /// When the socket was asked for (a switch to TLS: when it was asked
    /// for, not when the connection was made).
    asked: u64,
    timeout_ms: u32,
    idle_ms: u32,
    /// When something last went over the secure connection.
    active: u64,
}

impl Watch {
    #[must_use]
    pub fn new(now: u64, timeout_ms: u32, idle_ms: u32) -> Self {
        Self {
            asked: now,
            timeout_ms,
            idle_ms,
            active: now,
        }
    }

    /// Something was sent or read at `now`.
    pub fn touch(&mut self, now: u64) {
        self.active = now;
    }

    /// Secure at `now`: the idle time counts from here.
    pub fn secured(&mut self, now: u64) {
        self.active = now;
    }

    /// Whether the socket has run out of time at `now`, and why. Waiting
    /// for a session for longer than the timeout is `Busy`, the connection
    /// and handshake taking longer `Timeout`, and idle for longer than
    /// allowed `Timeout` too, its reason `idle`.
    #[must_use]
    pub fn expired(&self, now: u64, stage: Stage) -> Option<(SocketError, &'static str)> {
        let over = now.saturating_sub(self.asked) > u64::from(self.timeout_ms);
        match stage {
            Stage::Queued if over => Some((SocketError::Busy, "no secure connection was free")),
            Stage::Securing if over => Some((SocketError::Timeout, "the handshake took too long")),
            Stage::Secure
                if self.idle_ms > 0
                    && now.saturating_sub(self.active) > u64::from(self.idle_ms) =>
            {
                Some((SocketError::Timeout, "idle"))
            }
            _ => None,
        }
    }

    /// Whether a secure socket idle for longer than `grace_ms` gives its
    /// slot up for one that waits for it (`Timeout`, `slot needed`).
    #[must_use]
    pub fn yields(&self, now: u64, grace_ms: u64) -> Option<(SocketError, &'static str)> {
        (now.saturating_sub(self.active) > grace_ms)
            .then_some((SocketError::Timeout, "slot needed"))
    }

    /// The most that may pass from now until something is due: for the
    /// main loop to come by again.
    #[must_use]
    pub fn next_due(&self, now: u64, stage: Stage) -> u64 {
        let due = match stage {
            Stage::Queued | Stage::Securing => {
                self.asked.saturating_add(u64::from(self.timeout_ms))
            }
            Stage::Secure if self.idle_ms > 0 => {
                self.active.saturating_add(u64::from(self.idle_ms))
            }
            Stage::Secure => return u64::MAX,
        };
        due.saturating_add(1).saturating_sub(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_too_long_for_a_session_is_busy() {
        let watch = Watch::new(1_000, 20_000, 60_000);
        assert_eq!(watch.expired(21_000, Stage::Queued), None);
        assert_eq!(
            watch.expired(21_001, Stage::Queued).map(|(e, _)| e),
            Some(SocketError::Busy)
        );
    }

    #[test]
    fn a_handshake_that_takes_too_long_is_a_timeout() {
        let watch = Watch::new(0, 5_000, 0);
        assert_eq!(watch.expired(5_000, Stage::Securing), None);
        assert_eq!(
            watch.expired(5_001, Stage::Securing).map(|(e, _)| e),
            Some(SocketError::Timeout)
        );
    }

    #[test]
    fn the_time_for_a_session_counts_with_the_handshake() {
        // Asked at 0, waited 15 s in line: 5 s left for the handshake.
        let watch = Watch::new(0, 20_000, 0);
        assert_eq!(watch.expired(19_000, Stage::Securing), None);
        assert!(watch.expired(20_001, Stage::Securing).is_some());
    }

    #[test]
    fn a_secure_socket_ends_idle_unless_used() {
        let mut watch = Watch::new(0, 20_000, 60_000);
        watch.secured(10_000);
        assert_eq!(watch.expired(70_000, Stage::Secure), None);
        let (error, reason) = watch.expired(70_001, Stage::Secure).unwrap();
        assert_eq!((error, reason), (SocketError::Timeout, "idle"));
        watch.touch(70_000);
        assert_eq!(watch.expired(100_000, Stage::Secure), None);
    }

    #[test]
    fn a_background_socket_ends_after_the_idle_cap_and_not_before() {
        // The cap a plugin may ask for (30 minutes) is kept to the
        // millisecond, a long way into the run too.
        let cap = u64::from(paperpaw_sdk::MAX_TLS_IDLE_MS);
        let start = 3_600_000;
        let mut watch = Watch::new(start, 20_000, paperpaw_sdk::MAX_TLS_IDLE_MS);
        watch.secured(start);
        assert_eq!(watch.expired(start + cap, Stage::Secure), None);
        assert_eq!(
            watch.expired(start + cap + 1, Stage::Secure),
            Some((SocketError::Timeout, "idle"))
        );
        assert_eq!(watch.next_due(start + 1, Stage::Secure), cap);
        // Traffic keeps it open for another whole cap.
        watch.touch(start + cap);
        assert_eq!(watch.expired(start + 2 * cap, Stage::Secure), None);
        assert!(watch.expired(start + 2 * cap + 1, Stage::Secure).is_some());
    }

    #[test]
    fn idle_zero_never_ends() {
        let mut watch = Watch::new(0, 20_000, 0);
        watch.secured(0);
        assert_eq!(watch.expired(u64::MAX, Stage::Secure), None);
        assert_eq!(watch.next_due(5, Stage::Secure), u64::MAX);
    }

    #[test]
    fn the_connection_time_does_not_end_a_secure_socket() {
        let mut watch = Watch::new(0, 1_000, 0);
        watch.secured(500);
        assert_eq!(watch.expired(10_000_000, Stage::Secure), None);
    }

    #[test]
    fn an_idle_socket_gives_its_slot_up_after_the_grace_and_not_before() {
        let mut watch = Watch::new(0, 20_000, 0);
        watch.secured(1_000);
        assert_eq!(watch.yields(6_000, 5_000), None);
        assert_eq!(
            watch.yields(6_001, 5_000),
            Some((SocketError::Timeout, "slot needed"))
        );
        // Traffic keeps it.
        watch.touch(6_000);
        assert_eq!(watch.yields(10_000, 5_000), None);
    }

    #[test]
    fn the_main_loop_is_told_when_the_next_end_is_due() {
        let watch = Watch::new(1_000, 20_000, 60_000);
        assert_eq!(watch.next_due(1_000, Stage::Queued), 20_001);
        assert_eq!(watch.next_due(30_000, Stage::Securing), 0);
        let mut watch = watch;
        watch.secured(5_000);
        assert_eq!(watch.next_due(6_000, Stage::Secure), 59_001);
    }
}
