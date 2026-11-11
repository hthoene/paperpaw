//! The open instances of a plugin's screens and which one is on top.
use alloc::vec::Vec;
use paperpaw_sdk::native::{Error, ui::Instance};

/// Per-instance state of a plugin's screens, keyed by the host's instance
/// numbers. The host calls `draw` and `input` for the screen on top only and
/// says which instance that is before the first draw; screens of other
/// plugins never reach this list.
#[derive(Debug)]
pub struct Screens<V> {
    open: Vec<(Instance, V)>,
    /// The top instance of each screen token.
    tops: Vec<(u32, Instance)>,
}

impl<V> Default for Screens<V> {
    fn default() -> Self {
        Self {
            open: Vec::new(),
            tops: Vec::new(),
        }
    }
}

impl<V> Screens<V> {
    /// Adds the instance, replacing the state of one opened again.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the list cannot grow.
    pub fn open(&mut self, instance: Instance, view: V) -> Result<(), Error> {
        if let Some((_, old)) = self.open.iter_mut().find(|(i, _)| *i == instance) {
            *old = view;
            return Ok(());
        }
        // Exact growth: a view can be a kilobyte, and a plugin has one or two open.
        crate::mem::push(&mut self.open, (instance, view))
    }

    /// Forgets a closed instance.
    pub fn close(&mut self, instance: Instance) {
        self.open.retain(|(i, _)| *i != instance);
        self.tops.retain(|(_, i)| *i != instance);
    }

    /// Notes that `instance` is on top for `token`'s screen.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the list cannot grow.
    pub fn shown(&mut self, token: u32, instance: Instance) -> Result<(), Error> {
        if let Some((_, top)) = self.tops.iter_mut().find(|(t, _)| *t == token) {
            *top = instance;
            return Ok(());
        }
        crate::mem::push(&mut self.tops, (token, instance))
    }

    /// The state of the top instance of `token`'s screen.
    pub fn top(&mut self, token: u32) -> Option<&mut V> {
        let instance = self.tops.iter().find(|(t, _)| *t == token)?.1;
        self.get(instance)
    }

    /// The instance on top for `token`'s screen.
    #[must_use]
    pub fn top_instance(&self, token: u32) -> Option<Instance> {
        self.tops.iter().find(|(t, _)| *t == token).map(|(_, i)| *i)
    }

    /// The state of `instance`.
    pub fn get(&mut self, instance: Instance) -> Option<&mut V> {
        self.open
            .iter_mut()
            .find(|(i, _)| *i == instance)
            .map(|(_, view)| view)
    }

    /// Every open instance's state.
    pub fn all(&mut self) -> impl Iterator<Item = &mut V> {
        self.open.iter_mut().map(|(_, view)| view)
    }

    /// Every open instance with its state.
    pub fn all_with_instance(&mut self) -> impl Iterator<Item = (Instance, &mut V)> {
        self.open
            .iter_mut()
            .map(|(instance, view)| (*instance, view))
    }

    /// Whether no instance is open.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
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
