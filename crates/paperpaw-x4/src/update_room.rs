//! Room for a firmware check (#273): a packed update takes about 52 KiB of the
//! heap at its start (the unpacking window, the decoder, two buffers), which
//! the radio's buffers, a plugin's tables and the like may hold. The host
//! that runs the check ([`crate::host_jobs::SharedUpdates`]) cannot free them:
//! they belong to the main loop. So it *asks* ([`Gate::ask`]), waits with its
//! steps, and the main loop gives memory back at its next turn (parks idle
//! plugins, switches the radio off where nothing moves over it) and says so
//! ([`Gate::ready`]); the check then begins again, once. Nothing polls: the
//! main loop looks where it works anyway, and the check's steps are the
//! kernel's own.

use core::cell::Cell;

/// Steps a check waits for the main loop before it tries once more anyway
/// (the shell that never answers, a radio that does not go).
pub const WAIT_STEPS: u32 = 3000;

/// Where the request for room is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Idle,
    /// The check found no room and waits for the main loop.
    Asked,
    /// The main loop gives room: the radio goes down.
    Shedding,
    /// Room was given; the check may try again.
    Ready,
}

/// The one request for room between a firmware check and the main loop,
/// shared (`Rc`) by the two: the host asks and waits, the shell frees.
#[derive(Debug)]
pub struct Gate {
    state: Cell<State>,
    /// A check began and the heap was not yet looked at for the log.
    began: Cell<bool>,
}

impl Default for Gate {
    fn default() -> Self {
        Self::new()
    }
}

impl Gate {
    /// A gate with no request.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: Cell::new(State::Idle),
            began: Cell::new(false),
        }
    }

    /// The host found no room: asks the main loop to make some.
    pub fn ask(&self) {
        self.state.set(State::Asked);
    }

    /// The main loop's side: whether room was asked for and not yet taken
    /// in hand. A yes starts the shedding ([`Self::shed`] ends it).
    #[must_use]
    pub fn take_asked(&self) -> bool {
        let asked = self.state.get() == State::Asked;
        if asked {
            self.state.set(State::Shedding);
        }
        asked
    }

    /// The main loop's side: whether it is shedding and so must tell when
    /// the radio is down ([`Self::shed`]).
    #[must_use]
    pub fn shedding(&self) -> bool {
        self.state.get() == State::Shedding
    }

    /// The main loop's side: what could be given back is given.
    pub fn shed(&self) {
        if self.state.get() == State::Shedding {
            self.state.set(State::Ready);
        }
    }

    /// The host's side: whether room was given since it asked; taken, the
    /// gate is at rest again.
    #[must_use]
    pub fn ready(&self) -> bool {
        let ready = self.state.get() == State::Ready;
        if ready {
            self.state.set(State::Idle);
        }
        ready
    }

    /// Back at rest, whatever the state (a check that ended).
    pub fn rest(&self) {
        self.state.set(State::Idle);
    }

    /// The host's side: a check began (for the heap to be logged).
    pub fn note_began(&self) {
        self.began.set(true);
    }

    /// The main loop's side: whether a check began since the last call.
    #[must_use]
    pub fn take_began(&self) -> bool {
        self.began.replace(false)
    }
}
