//! The open instances of a plugin's screens and which one is on top.
use alloc::vec::Vec;
use paperpaw_sdk::native::{Error, ui::Instance};

/// Per-instance state of a plugin's screens, keyed by the host's instance
/// numbers. The host calls `draw` and `input` for the screen on top only and
/// says which instance that is before the first draw; screens of other
/// plugins never reach this list.
#[derive(Debug)]
pub struct Screens<V> {
    /// The open instances with their states, in one list: one allocation
    /// per plugin, not two (a list of the instances beside one of the
    /// states cost the X4's reading about 1 KiB more heap, #4954).
    open: Vec<Slot<V>>,
    /// The top instance of each screen token.
    tops: Vec<(u32, Instance)>,
}

/// An open instance and its state. `repr(C)` with the instance first: every
/// slot starts with its instance, so the lookups read the list without
/// knowing `V` (see [`place`]).
#[derive(Debug)]
#[repr(C)]
struct Slot<V> {
    instance: Instance,
    view: V,
}

impl<V> Default for Screens<V> {
    fn default() -> Self {
        Self {
            open: Vec::new(),
            tops: Vec::new(),
        }
    }
}

// The lookups are called from many places of every plugin: each is one
// function, not code at every call (#493).

/// Where `instance` is among the `len` instances that start every `stride`
/// bytes from `first`.
///
/// # Safety
/// `first` points to `len` initialised [`Instance`]s, `stride` bytes apart,
/// each aligned for it.
#[inline(never)]
unsafe fn place_in(
    first: *const u8,
    stride: usize,
    len: usize,
    instance: Instance,
) -> Option<usize> {
    // SAFETY: the caller's contract: `at * stride` is the place of an
    // aligned, initialised instance for every `at` below `len`.
    (0..len).position(|at| unsafe { first.add(at * stride).cast::<Instance>().read() } == instance)
}

/// Where `instance` is among `open`.
fn place<V>(open: &[Slot<V>], instance: Instance) -> Option<usize> {
    // SAFETY: `Slot` is `repr(C)` with the instance first, so every slot of
    // the slice starts with an aligned, initialised `Instance`, the size of
    // a slot apart.
    unsafe {
        place_in(
            open.as_ptr().cast(),
            core::mem::size_of::<Slot<V>>(),
            open.len(),
            instance,
        )
    }
}

/// The instance on top for `token` in `tops`.
#[inline(never)]
fn top_of(tops: &[(u32, Instance)], token: u32) -> Option<Instance> {
    tops.iter().find(|(t, _)| *t == token).map(|(_, i)| *i)
}

/// Forgets `instance` in `tops`.
#[inline(never)]
fn forget_top(tops: &mut Vec<(u32, Instance)>, instance: Instance) {
    tops.retain(|(_, i)| *i != instance);
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
    // `open` and `close` move or drop a whole view: in a frame of their own,
    // not in that of each plugin's `event` (over the band's 2,048 B, #4954).
    #[inline(never)]
    pub fn open(&mut self, instance: Instance, view: V) -> Result<(), Error> {
        if let Some(old) = self.get(instance) {
            *old = view;
            return Ok(());
        }
        // Exact growth: a view can be a kilobyte, and a plugin has one or two open.
        crate::mem::push(&mut self.open, Slot { instance, view })
    }

    /// Forgets a closed instance.
    #[inline(never)]
    pub fn close(&mut self, instance: Instance) {
        forget_top(&mut self.tops, instance);
        if let Some(at) = place(&self.open, instance) {
            // Dropped where it is: `Vec::remove` would return it, a copy of
            // the view (up to a kilobyte) on the stack.
            self.open.drain(at..=at);
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
        self.get(top_of(&self.tops, token)?)
    }

    /// The instance on top for `token`'s screen.
    #[must_use]
    pub fn top_instance(&self, token: u32) -> Option<Instance> {
        top_of(&self.tops, token)
    }

    /// The state of `instance`.
    pub fn get(&mut self, instance: Instance) -> Option<&mut V> {
        let at = place(&self.open, instance)?;
        self.open.get_mut(at).map(|slot| &mut slot.view)
    }

    /// Every open instance's state.
    pub fn all(&mut self) -> impl Iterator<Item = &mut V> {
        self.open.iter_mut().map(|slot| &mut slot.view)
    }

    /// Every open instance with its state.
    pub fn all_with_instance(&mut self) -> impl Iterator<Item = (Instance, &mut V)> {
        self.open
            .iter_mut()
            .map(|slot| (slot.instance, &mut slot.view))
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
