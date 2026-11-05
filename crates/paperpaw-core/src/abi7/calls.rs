//! Admission guards for a plugin instance, shared by all its callback tokens.
//!
//! Admission must precede constructing references to plugin state. The host
//! owns both cells and never gives a plugin a mutable reference to these guards.

use core::cell::Cell;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    Busy,
    Depth,
}

pub(crate) struct Instance {
    entered: Cell<bool>,
}

pub(crate) struct Depth {
    live: Cell<u8>,
}

pub(crate) struct Call<'a> {
    instance: &'a Instance,
    depth: &'a Depth,
}

impl Instance {
    pub(crate) const fn new() -> Self {
        Self {
            entered: Cell::new(false),
        }
    }

    /// All callback tokens for one mutable state must use this same instance.
    /// No plugin state is accessed on either refusal path.
    pub(crate) fn enter<'a>(&'a self, depth: &'a Depth) -> Result<Call<'a>, Error> {
        if self.entered.get() {
            return Err(Error::Busy);
        }
        if depth.live.get() >= 2 {
            return Err(Error::Depth);
        }
        self.entered.set(true);
        depth.live.set(depth.live.get() + 1);
        Ok(Call {
            instance: self,
            depth,
        })
    }
}

impl Depth {
    pub(crate) const fn new() -> Self {
        Self { live: Cell::new(0) }
    }
}

impl Drop for Call<'_> {
    fn drop(&mut self) {
        self.instance.entered.set(false);
        self.depth.live.set(self.depth.live.get().saturating_sub(1));
    }
}
