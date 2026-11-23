//! The open instances of a plugin's screens and which one is on top.
use alloc::vec::Vec;
use core::{alloc::Layout, marker::PhantomData, ptr};
use paperpaw_sdk::native::{Error, ui::Instance};

/// Per-instance state of a plugin's screens, keyed by the host's instance
/// numbers. The host calls `draw` and `input` for the screen on top only and
/// says which instance that is before the first draw; screens of other
/// plugins never reach this list.
pub struct Screens<V> {
    /// The open instances with their states, in one list: one allocation
    /// per plugin, not two (a list of the instances beside one of the
    /// states cost the X4's reading about 1 KiB more heap, #4954). The list
    /// does not know `V`: finding, adding, closing and freeing are one code
    /// for every plugin's views, not a copy per view type (the X4's 30 had
    /// 12 KB of them, #501).
    open: List,
    /// The top instance of each screen token.
    tops: Vec<(u32, Instance)>,
    /// The list owns slots of `V`.
    views: PhantomData<Slot<V>>,
}

// SAFETY: the list owns its slots as a `Vec<Slot<V>>` would.
unsafe impl<V: Send> Send for Screens<V> {}
// SAFETY: as for `Send`; shared access only reads the slots.
unsafe impl<V: Sync> Sync for Screens<V> {}

/// An open instance and its state. `repr(C)` with the instance first: every
/// slot starts with its instance, so the list finds one without knowing `V`.
#[derive(Debug)]
#[repr(C)]
struct Slot<V> {
    instance: Instance,
    view: V,
}

/// What the list needs to know of a slot type.
struct Kind {
    /// A slot's size and alignment.
    layout: Layout,
    /// Drops the slot at a place.
    drop: unsafe fn(*mut u8),
}

/// Drops the `Slot<V>` at `slot`.
///
/// # Safety
/// `slot` points to an initialised `Slot<V>` that is not used again.
unsafe fn drop_slot<V>(slot: *mut u8) {
    // SAFETY: the caller's contract.
    unsafe { ptr::drop_in_place(slot.cast::<Slot<V>>()) }
}

/// Slots of one type in one block of the global allocator, as a `Vec` of
/// them keeps them (and grown as [`crate::mem::push`] grows one), without
/// their type. Every method is given the type's [`Kind`] (or its size) and
/// is safe only with the one of the slots in the list.
#[derive(Debug)]
struct List {
    /// The block, null while there is none.
    start: *mut u8,
    /// The slots in use, from the start.
    len: usize,
    /// The slots the block has room for.
    capacity: usize,
}

impl List {
    /// The slot of `instance`, or null.
    ///
    /// # Safety
    /// `size` is that of the list's slots.
    #[inline(never)]
    unsafe fn find(&self, size: usize, instance: Instance) -> *mut u8 {
        for at in 0..self.len {
            // SAFETY: every slot below `len` is initialised and starts with
            // an aligned instance, `size` bytes after the one before.
            let slot = unsafe { self.start.add(at * size) };
            if unsafe { slot.cast::<Instance>().read() } == instance {
                return slot;
            }
        }
        ptr::null_mut()
    }

    /// The slot of the instance on top for `token` in `tops`, or null.
    ///
    /// # Safety
    /// As [`List::find`].
    #[inline(never)]
    unsafe fn top(&self, size: usize, tops: &[(u32, Instance)], token: u32) -> *mut u8 {
        match top_of(tops, token) {
            // SAFETY: the caller's contract.
            Some(instance) => unsafe { self.find(size, instance) },
            None => ptr::null_mut(),
        }
    }

    /// The slot to write the view of `instance` into: its own one with the
    /// old view dropped, or a new one at the end with the instance written
    /// and counted. Null where the list cannot grow.
    ///
    /// # Safety
    /// `kind` is that of the list's slots; the caller writes the view into
    /// the slot returned before the list is used again.
    #[inline(never)]
    unsafe fn open(&mut self, kind: &Kind, instance: Instance) -> *mut u8 {
        let size = kind.layout.size();
        // SAFETY: the caller's contract.
        let slot = unsafe { self.find(size, instance) };
        if !slot.is_null() {
            // SAFETY: the slot is initialised; the caller writes it again.
            unsafe { (kind.drop)(slot) };
            return slot;
        }
        if self.len == self.capacity {
            // SAFETY: the block and capacity are the list's, of slots of
            // the kind's layout from the global allocator.
            let (start, capacity) =
                unsafe { crate::mem::grow(self.start, self.capacity, kind.layout) };
            if capacity == self.capacity {
                return ptr::null_mut();
            }
            (self.start, self.capacity) = (start, capacity);
        }
        // SAFETY: the block has room for the slot at `len`; its instance is
        // written now, its view by the caller.
        unsafe {
            let slot = self.start.add(self.len * size);
            slot.cast::<Instance>().write(instance);
            self.len += 1;
            slot
        }
    }

    /// Forgets `instance` in `tops` and drops its slot, if there is one:
    /// where it is (no copy of a view of up to a kilobyte on the stack),
    /// the slots after it moving down.
    ///
    /// # Safety
    /// `kind` is that of the list's slots.
    #[inline(never)]
    unsafe fn close(&mut self, kind: &Kind, tops: &mut Vec<(u32, Instance)>, instance: Instance) {
        tops.retain(|(_, i)| *i != instance);
        let size = kind.layout.size();
        // SAFETY: the caller's contract.
        let slot = unsafe { self.find(size, instance) };
        if slot.is_null() {
            return;
        }
        let len = self.len;
        // SAFETY: the slot is in the list. The list is cut to the slots
        // before it first, so that no slot is dropped twice where a view's
        // drop ends early; the slots after it then move down over it.
        unsafe {
            self.len = slot.offset_from_unsigned(self.start) / size;
            (kind.drop)(slot);
            let next = slot.add(size);
            ptr::copy(
                next,
                slot,
                self.start.add(len * size).offset_from_unsigned(next),
            );
        }
        self.len = len - 1;
    }

    /// Drops every slot and frees the block.
    ///
    /// # Safety
    /// `kind` is that of the list's slots; the list is not used again.
    #[inline(never)]
    unsafe fn free(&mut self, kind: &Kind) {
        let len = core::mem::replace(&mut self.len, 0);
        for at in 0..len {
            // SAFETY: every slot below the old `len` is initialised.
            unsafe { (kind.drop)(self.start.add(at * kind.layout.size())) };
        }
        if self.capacity > 0 {
            // SAFETY: the block was given by the global allocator for
            // `capacity` slots of the kind's layout.
            unsafe {
                alloc::alloc::dealloc(
                    self.start,
                    Layout::from_size_align_unchecked(
                        kind.layout.size() * self.capacity,
                        kind.layout.align(),
                    ),
                );
            }
        }
    }
}

/// The instance on top for `token` in `tops`.
#[inline(never)]
fn top_of(tops: &[(u32, Instance)], token: u32) -> Option<Instance> {
    tops.iter().find(|(t, _)| *t == token).map(|(_, i)| *i)
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

impl<V> Default for Screens<V> {
    fn default() -> Self {
        Self {
            open: List {
                start: ptr::null_mut(),
                len: 0,
                capacity: 0,
            },
            tops: Vec::new(),
            views: PhantomData,
        }
    }
}

impl<V> Drop for Screens<V> {
    fn drop(&mut self) {
        // SAFETY: the kind is that of the list's slots; it is not used again.
        unsafe { self.open.free(&Self::KIND) }
    }
}

impl<V: core::fmt::Debug> core::fmt::Debug for Screens<V> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Screens")
            .field("open", &self.slots())
            .field("tops", &self.tops)
            .finish()
    }
}

impl<V> Screens<V> {
    /// What the list needs to know of its slots.
    const KIND: Kind = Kind {
        layout: Layout::new::<Slot<V>>(),
        drop: drop_slot::<V>,
    };

    /// A slot's size.
    const SIZE: usize = core::mem::size_of::<Slot<V>>();

    /// The open slots.
    fn slots(&self) -> &[Slot<V>] {
        if self.open.len == 0 {
            return &[];
        }
        // SAFETY: the list holds `len` initialised slots of `V` from its
        // start, borrowed with it.
        unsafe { core::slice::from_raw_parts(self.open.start.cast(), self.open.len) }
    }

    /// The open slots, to change.
    fn slots_mut(&mut self) -> &mut [Slot<V>] {
        if self.open.len == 0 {
            return &mut [];
        }
        // SAFETY: as in `slots`, borrowed mutably with the list.
        unsafe { core::slice::from_raw_parts_mut(self.open.start.cast(), self.open.len) }
    }

    /// The view of the slot at `slot`, if there is one.
    ///
    /// # Safety
    /// `slot` is null or one of the list's slots.
    unsafe fn view(&mut self, slot: *mut u8) -> Option<&mut V> {
        // SAFETY: the caller's contract; the slot is borrowed with the list.
        unsafe { slot.cast::<Slot<V>>().as_mut().map(|slot| &mut slot.view) }
    }

    /// Adds the instance, replacing the state of one opened again.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when the list cannot grow.
    // `open` moves a whole view: in a frame of its own, not in that of each
    // plugin's `event` (over the band's 2,048 B, #4954). The view is written
    // straight into its slot, never built into a slot on this frame first.
    #[inline(never)]
    pub fn open(&mut self, instance: Instance, view: V) -> Result<(), Error> {
        // SAFETY: the kind is that of the list's slots; the view is written
        // into the slot returned right below.
        let slot = unsafe { self.open.open(&Self::KIND, instance) }.cast::<Slot<V>>();
        if slot.is_null() {
            return Err(Error::NoMemory);
        }
        // SAFETY: the slot is in the list, its view dropped or not yet written.
        unsafe { (&raw mut (*slot).view).write(view) };
        Ok(())
    }

    /// Forgets a closed instance.
    pub fn close(&mut self, instance: Instance) {
        // SAFETY: the kind is that of the list's slots.
        unsafe { self.open.close(&Self::KIND, &mut self.tops, instance) }
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
        // SAFETY: the size is that of the list's slots; a slot found is in
        // the list.
        unsafe {
            let slot = self.open.top(Self::SIZE, &self.tops, token);
            self.view(slot)
        }
    }

    /// The instance on top for `token`'s screen.
    #[must_use]
    pub fn top_instance(&self, token: u32) -> Option<Instance> {
        top_of(&self.tops, token)
    }

    /// The state of `instance`.
    pub fn get(&mut self, instance: Instance) -> Option<&mut V> {
        // SAFETY: as in `top`.
        unsafe {
            let slot = self.open.find(Self::SIZE, instance);
            self.view(slot)
        }
    }

    /// Every open instance's state.
    pub fn all(&mut self) -> impl Iterator<Item = &mut V> {
        self.slots_mut().iter_mut().map(|slot| &mut slot.view)
    }

    /// Every open instance with its state.
    pub fn all_with_instance(&mut self) -> impl Iterator<Item = (Instance, &mut V)> {
        self.slots_mut()
            .iter_mut()
            .map(|slot| (slot.instance, &mut slot.view))
    }

    /// Whether no instance is open.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.open.len == 0
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
