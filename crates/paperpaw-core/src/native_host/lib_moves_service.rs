//! The library's one move job as the device holds it: the `Moves` machine
//! with the card's side ([`CardMoves`]), started by a `move` on root 1
//! ([`LibraryMoves::begin`]), carried by the kernel's main loop
//! ([`LibraryMoves::pump`], one bounded step per call) and reported to
//! `core.files` as a busy source (`register_library_moves`).
//!
//! The kernel calls [`HostServices::pump_library_moves`] from its service
//! pump with the `Env` and the queue the events go to; the first call also
//! starts the machine (`Moves::start`: the open journal entry, if any, is
//! decided and its job finished), so nothing else has to run at start. A
//! step that fails (the card, memory) keeps the job and is retried
//! ([`Pump::Wait`]); the entry is never dropped for it.
use super::{
    host_services::HostServices,
    lib_moves_carry::EnvCarry,
    lib_moves_host::{CardMoves, StepHost},
    lib_moves_walk::Resume,
    lib_service::moves::Moves,
};
use alloc::rc::Rc;
use core::cell::Cell;
use paperpaw_native::host_glue::ERR_STATE;
use paperpaw_platform::Storage;
use paperpaw_sdk::{Command, Env};

/// What a pump found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pump {
    /// No job: nothing to do until a move starts.
    Idle,
    /// A step ran and more is left: call again soon.
    More,
    /// A step failed (the job is kept): call again after a while.
    Wait,
}

/// The move job and its card side.
pub(crate) struct LibraryMoves {
    moves: Rc<Moves>,
    card: Rc<CardMoves>,
    /// The machine has read the journal at least once with a catalog.
    started: Cell<bool>,
    /// How many moves were begun (wrapping): the pump's back-off starts again
    /// when it changes (`moves_pump`).
    begun: Cell<u32>,
    /// The walk over the moved folder, kept between steps.
    resume: Resume,
}

impl LibraryMoves {
    pub(crate) fn new(storage: Rc<dyn Storage>) -> Self {
        Self {
            moves: Rc::default(),
            card: Rc::new(CardMoves::new(storage)),
            started: Cell::new(false),
            begun: Cell::new(0),
            resume: Resume::default(),
        }
    }

    pub(crate) fn moves(&self) -> &Rc<Moves> {
        &self.moves
    }

    pub(crate) fn card(&self) -> &Rc<CardMoves> {
        &self.card
    }

    /// A move of the user's files (root 1): the journal entry, then the
    /// rename; the catalog follows from the pump. The same call serves LIB's
    /// `move`.
    ///
    /// # Errors
    /// As [`Moves::begin`].
    pub(crate) fn begin(&self, from: &str, to: &str, replace: bool) -> Result<(), i32> {
        self.moves.begin(&*self.card, from, to, replace)?;
        self.begun.set(self.begun.get().wrapping_add(1));
        Ok(())
    }

    /// How many moves were begun, wrapping.
    pub(crate) fn begun(&self) -> u32 {
        self.begun.get()
    }

    /// A reader opens `book`: it is carried first when the job has not
    /// reached it ([`Moves::open_book`]): `ERR_BUSY` means "retry", the next
    /// pump has it.
    ///
    /// # Errors
    /// As [`Moves::open_book`].
    #[allow(dead_code, reason = "the reader's open (Host, #946) calls it")]
    pub(crate) fn open_book(&self, book: &str) -> Result<(), i32> {
        self.moves.open_book(&*self.card, book)
    }

    /// One step of the job with the catalog of `env`; events and setting
    /// changes go to `out`.
    pub(crate) fn pump(&self, env: &Env<'_>, out: &mut dyn FnMut(Command)) -> Pump {
        let carry = EnvCarry::new(env, &self.resume, out);
        let step = StepHost::new(&self.card, &carry);
        if !self.started.get() {
            if self.moves.start(&step).is_err() {
                return Pump::Wait;
            }
            self.started.set(true);
        }
        if !self.moves.active() {
            return Pump::Idle;
        }
        match self.moves.step(&step) {
            Ok(true) => Pump::More,
            Ok(false) => Pump::Idle,
            Err(_) => Pump::Wait,
        }
    }

    /// How many unreadable journals were set aside since the last call, for
    /// the kernel to log.
    pub(crate) fn take_set_aside(&self) -> u32 {
        self.card.take_set_aside()
    }
}

impl HostServices {
    /// Makes the library's move job over the card's `storage`, registers it
    /// as the busy source of `core.files` and keeps it for the pump and for
    /// the moves of `core.files` and LIB. Once.
    ///
    /// # Errors
    /// `ERR_STATE` when it was made already; as `register_library_moves`.
    pub(crate) fn wire_library_moves(
        &self,
        storage: Rc<dyn Storage>,
    ) -> Result<Rc<LibraryMoves>, i32> {
        if self.library_moves.get().is_some() {
            return Err(ERR_STATE);
        }
        let moves = Rc::new(LibraryMoves::new(storage));
        self.register_library_moves(moves.moves().clone(), moves.card().clone())?;
        self.library_moves
            .set(moves.clone())
            .map_err(|_| ERR_STATE)?;
        Ok(moves)
    }

    /// The library's move job, once wired.
    pub(crate) fn library_moves(&self) -> Option<&Rc<LibraryMoves>> {
        self.library_moves.get()
    }

    /// One step of the library's move job, if wired (`Pump::Idle` if not):
    /// the kernel's service pump calls it with its `Env` and command queue.
    pub(crate) fn pump_library_moves(&self, env: &Env<'_>, out: &mut dyn FnMut(Command)) -> Pump {
        self.library_moves
            .get()
            .map_or(Pump::Idle, |moves| moves.pump(env, out))
    }
}

#[cfg(test)]
#[path = "lib_moves_service_tests.rs"]
mod tests;
