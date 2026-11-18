//! What the kernel's built-in parts run, as plain functions kept in the
//! program (#293): a handler of an event ([`Hear`]) and a step of a job
//! ([`Stepper`]). No closure and nothing on the heap per handler: what a
//! handler needs of its own is the kernel's (`Kernel::core`).

use crate::Place;
use alloc::borrow::Cow;

use crate::Asks;
use paperpaw_sdk::Params;
use paperpaw_sdk::event::{Event, EventData};
use paperpaw_sdk::work::Step;

use crate::Kernel;

/// A handler of an event: what it carries as `params` (read with [`data`]).
/// What it asks for goes through [`Kernel::with_env`].
pub type Hear = fn(&mut Kernel, &Params);

/// A step of a job declared as work (see [`work`](paperpaw_sdk::work)).
pub type Stepper = fn(&mut Kernel, &Params) -> Step;

/// What an event carries for its type `P`: as it is where it was sent so
/// (no copy), else as written; `None` where what it needs is missing.
pub fn data<P: EventData>(params: &Params) -> Option<Cow<'_, P>> {
    match params.carried::<P>() {
        Some(data) => Some(Cow::Borrowed(data)),
        None => P::from_params(params).map(Cow::Owned),
    }
}

impl Kernel {
    /// Runs `f` with the environment and a queue of what it asks for, done
    /// after the handler as a plugin's commands are.
    pub fn with_env(&mut self, f: impl FnOnce(&Place<'_>, &mut Asks)) {
        let mut f = Some(f);
        self.with_parts(&mut |env, _, out| {
            if let Some(f) = f.take() {
                f(env, out);
            }
        });
    }

    /// [`Self::with_env`] where `params` carry `event`'s data (else nothing
    /// runs): what a handler of `event` does.
    pub fn with_event<P: EventData>(
        &mut self,
        event: &Event<P>,
        params: &Params,
        f: impl FnOnce(&Place<'_>, &P, &mut Asks),
    ) {
        let _ = event;
        if let Some(data) = data::<P>(params) {
            self.with_env(|env, out| f(env, &data, out));
        }
    }

    /// [`Self::with_event`] with the core plugin's own state.
    pub(crate) fn with_core_event<P: EventData>(
        &mut self,
        event: &Event<P>,
        params: &Params,
        f: impl FnOnce(&Place<'_>, &crate::core_plugin::State, &P, &mut Asks),
    ) {
        let _ = event;
        if let Some(data) = data::<P>(params) {
            self.with_core(|env, core, out| f(env, core, &data, out));
        }
    }

    /// [`Self::with_env`] with the core plugin's own state.
    pub(crate) fn with_core(
        &mut self,
        f: impl FnOnce(&Place<'_>, &crate::core_plugin::State, &mut Asks),
    ) {
        let mut f = Some(f);
        self.with_parts(&mut |env, core, out| {
            if let Some(f) = f.take() {
                f(env, core, out);
            }
        });
    }

    /// The one body of the `with_*` calls, not inlined into every handler:
    /// the environment made, `f` run, what it asked for queued.
    #[inline(never)]
    fn with_parts(&mut self, f: &mut dyn FnMut(&Place<'_>, &crate::core_plugin::State, &mut Asks)) {
        let env = self.shared.place(self.shared.now);
        let core = &self.core;
        crate::commands::handing(&mut self.output.commands, |out| f(&env, core, out));
    }
}
