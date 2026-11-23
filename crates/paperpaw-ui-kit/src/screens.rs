//! The open instances of a plugin's screens and which one is on top.
use alloc::vec::Vec;
use paperpaw_sdk::native::{Error, ui::Instance};

/// Per-instance state of a plugin's screens, keyed by the host's instance
/// numbers. The host calls `draw` and `input` for the screen on top only and
/// says which instance that is before the first draw; screens of other
/// plugins never reach this list.
#[derive(Debug)]
pub struct Screens<V> {
    /// The open instances, each at the place of its state in `views`: the
    /// lookups read only these, so their code is the same for every plugin's
    /// views and is in the program once.
    ids: Vec<Instance>,
    views: Vec<V>,
    /// The top instance of each screen token.
    tops: Vec<(u32, Instance)>,
}

impl<V> Default for Screens<V> {
    fn default() -> Self {
        Self {
            ids: Vec::new(),
            views: Vec::new(),
            tops: Vec::new(),
        }
    }
}

// The lookups are called from many places of every plugin: each is one
// function, not code at every call (#493).

/// Where `instance` is among `ids`.
#[inline(never)]
fn place(ids: &[Instance], instance: Instance) -> Option<usize> {
    ids.iter().position(|i| *i == instance)
}

/// The instance on top for `token` in `tops`.
#[inline(never)]
fn top_of(tops: &[(u32, Instance)], token: u32) -> Option<Instance> {
    tops.iter().find(|(t, _)| *t == token).map(|(_, i)| *i)
}

/// Where the instance on top for `token` is among `ids`.
#[inline(never)]
fn top_place(ids: &[Instance], tops: &[(u32, Instance)], token: u32) -> Option<usize> {
    place(ids, top_of(tops, token)?)
}

/// Forgets `instance` in `ids` and `tops`: where its state was, if it was open.
#[inline(never)]
fn forget(
    ids: &mut Vec<Instance>,
    tops: &mut Vec<(u32, Instance)>,
    instance: Instance,
) -> Option<usize> {
    tops.retain(|(_, i)| *i != instance);
    let at = place(ids, instance)?;
    ids.remove(at);
    Some(at)
}

/// Notes in `tops` that `instance` is on top for `token`.
#[inline(never)]
fn show(tops: &mut Vec<(u32, Instance)>, token: u32, instance: Instance) -> Result<(), Error> {
    if let Some((_, top)) = tops.iter_mut().find(|(t, _)| *t == token) {
        *top = instance;
        return Ok(());
    }
    crate::mem::push(tops, (token, instance))
}

impl<V> Screens<V> {
    /// Adds the instance, replacing the state of one opened again.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the list cannot grow.
    pub fn open(&mut self, instance: Instance, view: V) -> Result<(), Error> {
        if let Some(old) = self.get(instance) {
            *old = view;
            return Ok(());
        }
        // Both lists have room before either grows: they stay in step.
        self.ids.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
        // Exact growth: a view can be a kilobyte, and a plugin has one or two open.
        crate::mem::push(&mut self.views, view)?;
        self.ids.push(instance);
        Ok(())
    }

    /// Forgets a closed instance.
    pub fn close(&mut self, instance: Instance) {
        if let Some(at) = forget(&mut self.ids, &mut self.tops, instance) {
            // Dropped where it is: `Vec::remove` returns the state, a copy of
            // it (up to a kilobyte) in the frame of each plugin's `event`.
            self.views.drain(at..=at);
        }
    }

    /// Notes that `instance` is on top for `token`'s screen.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the list cannot grow.
    pub fn shown(&mut self, token: u32, instance: Instance) -> Result<(), Error> {
        show(&mut self.tops, token, instance)
    }

    /// The state of the top instance of `token`'s screen.
    pub fn top(&mut self, token: u32) -> Option<&mut V> {
        let at = top_place(&self.ids, &self.tops, token)?;
        self.views.get_mut(at)
    }

    /// The instance on top for `token`'s screen.
    #[must_use]
    pub fn top_instance(&self, token: u32) -> Option<Instance> {
        top_of(&self.tops, token)
    }

    /// The state of `instance`.
    pub fn get(&mut self, instance: Instance) -> Option<&mut V> {
        let at = place(&self.ids, instance)?;
        self.views.get_mut(at)
    }

    /// Every open instance's state.
    pub fn all(&mut self) -> impl Iterator<Item = &mut V> {
        self.views.iter_mut()
    }

    /// Every open instance with its state.
    pub fn all_with_instance(&mut self) -> impl Iterator<Item = (Instance, &mut V)> {
        self.ids.iter().copied().zip(self.views.iter_mut())
    }

    /// Whether no instance is open.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

/// Calls `$body` (a `Result<(), Error>`) for every row of every page of a
/// registry query, following the cursor. `$call` makes one page from
/// `$cursor`.
///
/// ```ignore
/// rows!(cursor => registry.actions(host, "", cursor, input, output), |row| {
///     names.push(copy(row.title())?);
///     Ok(())
/// })?;
/// ```
#[macro_export]
macro_rules! rows {
    ($cursor:ident => $call:expr, |$row:ident| $body:expr) => {{
        let mut $cursor = ::paperpaw_sdk::native::registry::PageCursor::start();
        loop {
            let page = $call?;
            for $row in page.rows() {
                let step: ::core::result::Result<(), ::paperpaw_sdk::native::Error> = $body;
                step?;
            }
            match page.end().next_cursor() {
                Some(next) => $cursor = next,
                None => break,
            }
        }
        ::core::result::Result::<(), ::paperpaw_sdk::native::Error>::Ok(())
    }};
}
