//! The device's one network pool (`network.md` 3.4, `sockets.md` section 6):
//! 48 KiB on the X4 until Firmware has measured, shared by `core.net` (request
//! inputs, memory bodies, I/O buffers, server reservations) and
//! `core.sockets` (a socket's send queue and read part at `connect`, a
//! broadcast's answers at `broadcast`).
//!
//! One counter, one place: both services charge this struct, never a copy of
//! the number, so a pool full of HTTP bodies refuses a socket with
//! `ERR_NO_MEMORY` and the other way round. The TLS session (about 40 KiB, one
//! fixed block the shell allocates once) and the sockets' shell buffers are
//! outside the pool and are not counted here.
//!
//! The pool is owned by the device (an `Rc` the shell gives to the NET host's
//! table and to the SOCK host alike); it has interior mutability so that
//! neither service needs the other's table borrowed to charge it. A device
//! without a pool (Android, the emulator: capacity 0) accepts every
//! reservation and counts nothing.
use core::cell::Cell;

/// The shared pool counter. All sizes are bytes.
#[derive(Debug)]
pub(crate) struct NetPool {
    capacity: u32,
    used: Cell<u32>,
    /// One bit per service that wants the network ([`DEMAND_NET`],
    /// [`DEMAND_SOCK`]).
    wanted: Cell<u8>,
    /// The TLS sessions' leases (outside the pool: the blocks are the
    /// shell's).
    tls: Cell<Lease>,
    /// How many sessions the device has (1 to [`TLS_SESSIONS_MAX`]).
    tls_sessions: u8,
}

/// Who holds, or waits for, a TLS session (the X4 has one of about 40 KiB, a
/// fixed block of the shell): a `core.net` request or a socket, by id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TlsHolder {
    Net(u32),
    Sock(u32),
}

/// The answer to [`NetPool::tls_request`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TlsLease {
    /// The caller holds the session now.
    Held,
    /// Others are before it: ask again later.
    Wait,
    /// The queue is full (never with the device's own limits).
    Full,
}

/// The most holders that wait for the session: 2 requests and 8 sockets are
/// the largest device's limits.
const TLS_WAITERS: usize = 16;

/// The most TLS sessions a device has at once.
pub(crate) const TLS_SESSIONS_MAX: usize = 4;

/// The leases of the TLS sessions: the holders and a FIFO of waiters. No
/// allocation, so asking for one cannot fail for memory.
#[derive(Clone, Copy, Debug)]
struct Lease {
    holders: [Option<TlsHolder>; TLS_SESSIONS_MAX],
    waiting: [Option<TlsHolder>; TLS_WAITERS],
}

/// `core.net` wants the network (a request or a server is live).
pub(crate) const DEMAND_NET: u8 = 1;
/// `core.sockets` wants the network (a socket is live).
pub(crate) const DEMAND_SOCK: u8 = 2;

impl NetPool {
    /// A pool of `capacity` bytes (0: the device has none).
    pub(crate) const fn new(capacity: u32) -> Self {
        Self::with_tls_sessions(capacity, 1)
    }

    /// A pool of `capacity` bytes on a device with `tls_sessions` TLS
    /// sessions (clamped to 1 to [`TLS_SESSIONS_MAX`]).
    pub(crate) const fn with_tls_sessions(capacity: u32, tls_sessions: u8) -> Self {
        let sessions = if tls_sessions == 0 {
            1
        } else if tls_sessions as usize > TLS_SESSIONS_MAX {
            TLS_SESSIONS_MAX as u8
        } else {
            tls_sessions
        };
        Self {
            capacity,
            used: Cell::new(0),
            wanted: Cell::new(0),
            tls: Cell::new(Lease {
                holders: [None; TLS_SESSIONS_MAX],
                waiting: [None; TLS_WAITERS],
            }),
            tls_sessions: sessions,
        }
    }

    /// The service `bit` wants (or no longer wants) the network. `Some(now)`
    /// when the OR of all services changed, which is the only time the shell
    /// is told: `core.net` going idle while a socket is live is no edge.
    pub(crate) fn demand(&self, bit: u8, wanted: bool) -> Option<bool> {
        let before = self.wanted.get();
        let after = if wanted { before | bit } else { before & !bit };
        self.wanted.set(after);
        ((before != 0) != (after != 0)).then_some(after != 0)
    }

    /// Asks for a TLS session for `who`: the first to ask is served first,
    /// as many holders at a time as the device has sessions (NET's https and
    /// SOCK's secure sockets alike). The caller asks again until it gets
    /// [`TlsLease::Held`], and gives the session back, or its place in the
    /// queue, with [`Self::tls_drop`].
    pub(crate) fn tls_request(&self, who: TlsHolder) -> TlsLease {
        let mut lease = self.tls.get();
        let slots = usize::from(self.tls_sessions);
        if lease.holders[..slots].contains(&Some(who)) {
            return TlsLease::Held;
        }
        if !lease.waiting.contains(&Some(who)) {
            let free = lease.holders[..slots].iter().position(Option::is_none);
            if let (Some(free), None) = (free, lease.waiting[0]) {
                lease.holders[free] = Some(who);
                self.tls.set(lease);
                return TlsLease::Held;
            }
            match lease.waiting.iter().position(Option::is_none) {
                Some(place) => lease.waiting[place] = Some(who),
                None => return TlsLease::Full,
            }
        }
        // The head of the queue takes a free session.
        let free = lease.holders[..slots].iter().position(Option::is_none);
        let answer = match free {
            Some(free) if lease.waiting[0] == Some(who) => {
                lease.holders[free] = Some(who);
                lease.waiting.copy_within(1.., 0);
                lease.waiting[TLS_WAITERS - 1] = None;
                TlsLease::Held
            }
            _ => TlsLease::Wait,
        };
        self.tls.set(lease);
        answer
    }

    /// `who` no longer holds or waits for a session. Idempotent.
    pub(crate) fn tls_drop(&self, who: TlsHolder) {
        let mut lease = self.tls.get();
        for holder in &mut lease.holders {
            if *holder == Some(who) {
                *holder = None;
            }
        }
        if let Some(at) = lease.waiting.iter().position(|w| *w == Some(who)) {
            lease.waiting.copy_within(at + 1.., at);
            lease.waiting[TLS_WAITERS - 1] = None;
        }
        self.tls.set(lease);
    }

    /// Whether anyone waits for a session.
    pub(crate) fn tls_waiting(&self) -> bool {
        self.tls.get().waiting[0].is_some()
    }

    /// Whether `who` holds a session now.
    #[cfg(test)]
    pub(crate) fn tls_holds(&self, who: TlsHolder) -> bool {
        self.tls.get().holders.contains(&Some(who))
    }

    /// The first session's holder (for a device with one session).
    #[cfg(test)]
    pub(crate) fn tls_holder(&self) -> Option<TlsHolder> {
        self.tls.get().holders[0]
    }

    /// How many sessions are held now.
    #[cfg(test)]
    pub(crate) fn tls_held(&self) -> usize {
        self.tls.get().holders.iter().flatten().count()
    }

    /// The pool's size; 0 for a device without one.
    pub(crate) const fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Bytes held now (always 0 without a pool).
    pub(crate) fn used(&self) -> u32 {
        self.used.get()
    }

    /// Whether `bytes` more would fit now, after `giving_back` bytes the
    /// caller is about to release. Without a pool: always.
    pub(crate) fn fits(&self, bytes: u32, giving_back: u32) -> bool {
        if self.capacity == 0 {
            return true;
        }
        let held = self.used.get().saturating_sub(giving_back);
        u64::from(held) + u64::from(bytes) <= u64::from(self.capacity)
    }

    /// Hold `bytes`; `false` (nothing held) when the pool has not that room.
    /// A caller maps `false` to `ERR_NO_MEMORY` (or keeps its request queued,
    /// as NET's strict FIFO does).
    pub(crate) fn reserve(&self, bytes: u32) -> bool {
        if self.capacity == 0 {
            return true;
        }
        match self.used.get().checked_add(bytes) {
            Some(total) if total <= self.capacity => {
                self.used.set(total);
                true
            }
            _ => false,
        }
    }

    /// Give `bytes` back. Never below zero: a caller can only release what it
    /// reserved, and a surplus is ignored rather than corrupting the count.
    pub(crate) fn release(&self, bytes: u32) {
        if self.capacity != 0 {
            self.used.set(self.used.get().saturating_sub(bytes));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DEMAND_NET, DEMAND_SOCK, NetPool, TlsHolder, TlsLease};

    #[test]
    fn the_tls_session_goes_to_one_holder_at_a_time_in_the_order_asked() {
        let pool = NetPool::new(49_152);
        let (n, a, b) = (TlsHolder::Net(1), TlsHolder::Sock(1), TlsHolder::Sock(2));
        assert_eq!(pool.tls_request(n), TlsLease::Held);
        assert_eq!(pool.tls_request(n), TlsLease::Held, "idempotent");
        assert_eq!(pool.tls_request(a), TlsLease::Wait);
        assert_eq!(pool.tls_request(b), TlsLease::Wait);
        assert!(pool.tls_waiting());
        // Net and Sock ids are different holders even when equal.
        assert!(pool.tls_holds(n));
        pool.tls_drop(n);
        assert_eq!(pool.tls_request(b), TlsLease::Wait, "a asked first");
        assert_eq!(pool.tls_request(a), TlsLease::Held);
        assert_eq!(pool.tls_request(b), TlsLease::Wait);
        // A waiter that gives up leaves the queue, and never blocks the rest.
        pool.tls_drop(b);
        pool.tls_drop(a);
        assert!(!pool.tls_waiting());
        assert_eq!(pool.tls_held(), 0);
        assert_eq!(pool.tls_request(b), TlsLease::Held);
        pool.tls_drop(b);
        pool.tls_drop(b);
    }

    #[test]
    fn a_full_queue_is_told_not_dropped() {
        let pool = NetPool::new(0);
        assert_eq!(pool.tls_request(TlsHolder::Net(0)), TlsLease::Held);
        for id in 0..16 {
            assert_eq!(pool.tls_request(TlsHolder::Sock(id)), TlsLease::Wait);
        }
        assert_eq!(pool.tls_request(TlsHolder::Sock(99)), TlsLease::Full);
    }

    #[test]
    fn reservations_are_bounded_and_released_exactly() {
        let pool = NetPool::new(48 * 1024);
        assert!(pool.reserve(45_056));
        assert!(!pool.reserve(4_097), "over the pool: nothing held");
        assert_eq!(pool.used(), 45_056);
        assert!(pool.reserve(4_096));
        assert!(!pool.reserve(1));
        pool.release(45_056);
        assert_eq!(pool.used(), 4_096);
        pool.release(u32::MAX);
        assert_eq!(pool.used(), 0, "a surplus release cannot underflow");
        assert!(!pool.reserve(u32::MAX), "no overflow");
    }

    #[test]
    fn fits_counts_what_the_caller_gives_back() {
        let pool = NetPool::new(10_000);
        assert!(pool.reserve(9_000));
        assert!(!pool.fits(2_000, 0));
        assert!(pool.fits(2_000, 1_000));
        assert!(pool.fits(2_000, 5_000), "more given back than held");
        assert!(!pool.fits(u32::MAX, 9_000));
    }

    #[test]
    fn the_shell_hears_the_or_of_the_services_only_when_it_changes() {
        // Also on a device without a pool: the demand is not a byte count.
        for capacity in [0, 49_152] {
            let pool = NetPool::new(capacity);
            assert_eq!(pool.demand(DEMAND_NET, true), Some(true));
            assert_eq!(pool.demand(DEMAND_SOCK, true), None, "already wanted");
            assert_eq!(pool.demand(DEMAND_NET, false), None, "a socket is live");
            assert_eq!(
                pool.demand(DEMAND_SOCK, false),
                Some(false),
                "both idle: once"
            );
            assert_eq!(pool.demand(DEMAND_SOCK, false), None);
            assert_eq!(pool.demand(DEMAND_SOCK, true), Some(true));
        }
    }

    #[test]
    fn a_device_without_a_pool_counts_nothing() {
        let pool = NetPool::new(0);
        assert!(pool.reserve(u32::MAX));
        assert_eq!(pool.used(), 0);
        assert!(pool.fits(u32::MAX, 0));
        pool.release(5);
        assert_eq!(pool.used(), 0);
    }
}
