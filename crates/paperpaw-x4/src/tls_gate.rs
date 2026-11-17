//! How many TLS sessions the firmware holds at once, and who is next when
//! all are held. A session takes some 40 KiB of heap while it is open (and
//! 8 to 12 KiB more during its handshake), so the X4 (ESP32-C3) holds one
//! and the X4 Pro and Classic (ESP32-S3) two. HTTPS requests and plugins'
//! TLS sockets share them: whoever asked first is served first.
//!
//! The gate only counts: tickets are asked for, taken in turn and given
//! back; what holds one is the firmware's.

use alloc::collections::VecDeque;

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
