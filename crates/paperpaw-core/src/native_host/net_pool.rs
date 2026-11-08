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
}

/// `core.net` wants the network (a request or a server is live).
pub(crate) const DEMAND_NET: u8 = 1;
/// `core.sockets` wants the network (a socket is live).
pub(crate) const DEMAND_SOCK: u8 = 2;

impl NetPool {
    /// A pool of `capacity` bytes (0: the device has none).
    pub(crate) const fn new(capacity: u32) -> Self {
        Self {
            capacity,
            used: Cell::new(0),
            wanted: Cell::new(0),
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
    use super::{DEMAND_NET, DEMAND_SOCK, NetPool};

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
