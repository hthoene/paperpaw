//! How many TLS sessions the firmware holds at once, and who is next when
//! all are held. A session takes some 40 KiB of heap while it is open (and
//! 8 to 12 KiB more during its handshake), so the X4 (ESP32-C3) holds one
//! and the X4 Pro and Classic (ESP32-S3) two. HTTPS requests and plugins'
//! TLS sockets share them: whoever asked first is served first.
//!
//! The gate only counts: tickets are asked for, taken in turn and given
//! back; what holds one is the firmware's.

use alloc::collections::VecDeque;

/// Free heap a new session needs: its 40 KiB, 12 more for the handshake and
/// the connection's receive window (8 KiB on the X4) less the 2 KiB the
/// received bytes waiting for mbedTLS no longer take, and 16 KiB kept for the
/// screens and the reader, which cannot draw or turn a page with the heap
/// taken to its last 4 KiB (the X4, 16 November: a store download left
/// 3.9 KiB free and every screen of the plugin refused to draw, with nothing
/// to free it again). The shell gives back what sits idle before a session
/// opens where the heap is below this (`Kernel::make_room`, #287).
pub const SESSION_ROOM: usize = 76 * 1024;

/// Whether a session may be opened with `free` bytes of heap free: where not,
/// the request ends with "not enough memory", and nothing else is starved.
#[must_use]
pub const fn session_fits(free: usize) -> bool {
    free >= SESSION_ROOM
}

/// Whether an mbedTLS error code says an allocation failed: a module's
/// `ALLOC_FAILED`, alone or added to a code of another (high-level codes carry
/// the module in bits 7 to 14, low-level ones their number in the low seven).
#[must_use]
pub const fn alloc_failed(code: i32) -> bool {
    let bits = code.unsigned_abs();
    if code >= 0 {
        return false;
    }
    matches!(
        bits & 0x7F80,
        // SSL, X509, PK, ECP, MD, CIPHER, DHM, PEM, PKCS7
        0x7F00 | 0x2880 | 0x3F80 | 0x4D80 | 0x5180 | 0x6180 | 0x3400 | 0x1180 | 0x5780
    ) || matches!(bits & 0x7F, 0x10 | 0x6A)
}

/// The sessions' slots and the queue for them.
#[derive(Debug)]
pub struct Gate {
    capacity: u8,
    held: u8,
    /// The tickets waiting, first asked first.
    queue: VecDeque<u32>,
    next: u32,
}

impl Gate {
    /// A gate of `capacity` slots (0: none are ever given).
    #[must_use]
    pub const fn new(capacity: u8) -> Self {
        Self {
            capacity,
            held: 0,
            queue: VecDeque::new(),
            next: 0,
        }
    }

    /// Slots in use now.
    #[must_use]
    pub fn held(&self) -> u8 {
        self.held
    }

    /// Tickets waiting.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.queue.len()
    }

    /// Whether someone waits and every slot is held: a socket that sits
    /// idle may be asked to give its slot up.
    #[must_use]
    pub fn pressed(&self) -> bool {
        !self.queue.is_empty() && self.held >= self.capacity
    }

    /// Gets in line; the ticket to [`take`](Gate::take) with.
    pub fn ask(&mut self) -> u32 {
        let ticket = self.next;
        self.next = self.next.wrapping_add(1);
        self.queue.push_back(ticket);
        ticket
    }

    /// Whether `ticket` gets a slot now: it is the first in line and one is
    /// free. Then it is out of the line and the slot is held, until
    /// [`release`](Gate::release).
    pub fn take(&mut self, ticket: u32) -> bool {
        if self.held < self.capacity && self.queue.front() == Some(&ticket) {
            self.queue.pop_front();
            self.held += 1;
            true
        } else {
            false
        }
    }

    /// Leaves the line (the one who waited gave up); nothing if `ticket`
    /// was served or is not in it.
    pub fn cancel(&mut self, ticket: u32) {
        self.queue.retain(|&waiting| waiting != ticket);
    }

    /// Gives a held slot back.
    pub fn release(&mut self) {
        self.held = self.held.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_is_not_opened_where_it_would_leave_the_screens_no_heap() {
        // The X4 with a book and the home screen used, 16 November: 64 KiB
        // free, and the session took all but 4 KiB.
        assert!(!session_fits(64 * 1024));
        // A start with nothing else used: 86 KiB free, 26 KiB left.
        assert!(session_fits(86 * 1024));
        assert!(!session_fits(SESSION_ROOM - 1));
        assert!(session_fits(SESSION_ROOM));
    }

    #[test]
    fn mbedtls_allocation_failures_are_told_from_other_errors() {
        // SSL, X509, MPI and ECP ALLOC_FAILED, and one added to a low code.
        for code in [-0x7F00, -0x2880, -0x0010, -0x4D80, -0x4D80 - 0x0010] {
            assert!(alloc_failed(code), "{code}");
        }
        // CONN_EOF, BAD_INPUT_DATA, X509 cert verify failed, no error.
        for code in [-0x7280, -0x7100, -0x2700, -0x0E, 0, 5] {
            assert!(!alloc_failed(code), "{code}");
        }
    }

    #[test]
    fn one_slot_serves_in_the_order_asked() {
        let mut gate = Gate::new(1);
        let (first, second) = (gate.ask(), gate.ask());
        // The second is not served before the first, though a slot is free.
        assert!(!gate.take(second));
        assert!(gate.take(first));
        assert_eq!((gate.held(), gate.waiting()), (1, 1));
        // None is free for the second until the first gives its up.
        assert!(!gate.take(second));
        gate.release();
        assert!(gate.take(second));
        gate.release();
        assert_eq!((gate.held(), gate.waiting()), (0, 0));
    }

    #[test]
    fn two_slots_hold_two_at_once() {
        let mut gate = Gate::new(2);
        let tickets = [gate.ask(), gate.ask(), gate.ask()];
        assert!(gate.take(tickets[0]));
        assert!(gate.take(tickets[1]));
        assert!(!gate.take(tickets[2]));
        gate.release();
        assert!(gate.take(tickets[2]));
    }

    #[test]
    fn one_that_gives_up_lets_the_next_go() {
        let mut gate = Gate::new(1);
        let (first, second, third) = (gate.ask(), gate.ask(), gate.ask());
        assert!(gate.take(first));
        gate.cancel(second);
        gate.release();
        assert!(!gate.take(second));
        assert!(gate.take(third));
        // Cancelling what was served, or twice, changes nothing.
        gate.cancel(third);
        gate.cancel(second);
        assert_eq!((gate.held(), gate.waiting()), (1, 0));
    }

    #[test]
    fn the_gate_is_pressed_when_one_waits_and_none_is_free() {
        let mut gate = Gate::new(1);
        assert!(!gate.pressed());
        let (first, second) = (gate.ask(), gate.ask());
        // A slot is free: not pressed.
        assert!(!gate.pressed());
        assert!(gate.take(first));
        // The first holds the one slot, the second waits.
        assert!(gate.pressed());
        gate.cancel(second);
        assert!(!gate.pressed());
        // Nobody waits: not pressed though the slot is held.
        gate.release();
        assert!(!gate.pressed());
    }

    #[test]
    fn a_gate_of_two_is_not_pressed_with_one_free() {
        let mut gate = Gate::new(2);
        let (first, second, third) = (gate.ask(), gate.ask(), gate.ask());
        assert!(gate.take(first));
        assert!(!gate.pressed());
        assert!(gate.take(second));
        assert!(gate.pressed());
        gate.cancel(third);
        assert!(!gate.pressed());
    }

    #[test]
    fn a_gate_of_none_never_serves() {
        let mut gate = Gate::new(0);
        let ticket = gate.ask();
        assert!(!gate.take(ticket));
        assert_eq!(gate.held(), 0);
    }

    #[test]
    fn a_release_without_a_slot_does_not_count_below_none() {
        let mut gate = Gate::new(1);
        gate.release();
        let ticket = gate.ask();
        assert!(gate.take(ticket));
        assert!(gate.held() == 1);
    }

    #[test]
    fn tickets_are_told_apart_when_the_counter_wraps() {
        let mut gate = Gate::new(1);
        gate.next = u32::MAX;
        let (last, first_again) = (gate.ask(), gate.ask());
        assert_ne!(last, first_again);
        assert!(gate.take(last));
        gate.release();
        assert!(gate.take(first_again));
    }
}
