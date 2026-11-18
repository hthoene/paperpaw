//! The TLS sessions the device holds at once (`paperpaw_x4::tls_gate`):
//! one on the X4 (ESP32-C3), two on the X4 Pro and Classic. HTTPS requests
//! and plugins' secure sockets take them in turn, whoever asked first
//! first: a [`Waiter`] asks, and is given a [`Slot`] once one is free; the
//! slot goes back when it is dropped, with the session that holds it.

use core::cell::{Cell, RefCell};

use critical_section::Mutex;
use paperpaw_x4::tls_gate::Gate;

/// Sessions at once (`Capabilities::tls_sessions`): about 40 KiB of heap
/// each while open, 8 to 12 more in the handshake.
pub const SESSIONS: u8 = if cfg!(feature = "esp32c3") { 1 } else { 2 };

static GATE: Mutex<RefCell<Gate>> = Mutex::new(RefCell::new(Gate::new(SESSIONS)));

/// A secure request was started and the heap not yet looked at for it.
static ROOM_ASKED: Mutex<Cell<bool>> = Mutex::new(Cell::new(false));

/// A secure request starts: the main loop makes room for its session before
/// it opens, where the heap is short ([`take_room_asked`]).
pub fn ask_room() {
    critical_section::with(|cs| ROOM_ASKED.borrow(cs).set(true));
}

/// Whether a secure request started since the last call.
pub fn take_room_asked() -> bool {
    critical_section::with(|cs| ROOM_ASKED.borrow(cs).replace(false))
}

/// One of the device's TLS sessions, held.
#[derive(Debug)]
pub struct Slot(());

impl Drop for Slot {
    fn drop(&mut self) {
        critical_section::with(|cs| GATE.borrow_ref_mut(cs).release());
    }
}

/// In line for a [`Slot`]; leaves the line when dropped.
#[derive(Debug)]
pub struct Waiter {
    ticket: u32,
    served: bool,
}

impl Waiter {
    /// Gets in line.
    pub fn ask() -> Self {
        Self {
            ticket: critical_section::with(|cs| GATE.borrow_ref_mut(cs).ask()),
            served: false,
        }
    }

    /// The slot, once it is this one's turn and one is free.
    pub fn take(&mut self) -> Option<Slot> {
        if self.served {
            return None;
        }
        let ticket = self.ticket;
        let taken = critical_section::with(|cs| GATE.borrow_ref_mut(cs).take(ticket));
        self.served = taken;
        taken.then_some(Slot(()))
    }
}

impl Drop for Waiter {
    fn drop(&mut self) {
        if !self.served {
            let ticket = self.ticket;
            critical_section::with(|cs| GATE.borrow_ref_mut(cs).cancel(ticket));
        }
    }
}

/// Whether someone waits for a slot and none is free.
pub fn pressed() -> bool {
    critical_section::with(|cs| GATE.borrow_ref(cs).pressed())
}
