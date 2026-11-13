//! The shell tests' allocator: the system's, failing on request so the
//! refusals of a fallible construction can be tried, and counting what this
//! thread holds.
extern crate std;
use core::alloc::Layout;
use core::cell::Cell;
use core::ptr;

std::thread_local! {
    /// The nth allocation of this thread fails (1-based; 0: none).
    pub(in crate::native_host) static FAIL_NTH: Cell<u32> = const { Cell::new(0) };
    /// The next allocation of exactly this many bytes fails, once (0: none).
    pub(in crate::native_host) static FAIL_SIZE: Cell<usize> = const { Cell::new(0) };
    /// Bytes this thread holds from the heap now, and the most it held.
    pub(in crate::native_host) static HEAP_LIVE: Cell<usize> = const { Cell::new(0) };
    pub(in crate::native_host) static HEAP_PEAK: Cell<usize> = const { Cell::new(0) };
    /// Fixture work (a card in memory) is not counted while this is set.
    pub(in crate::native_host) static HEAP_SHIELD: Cell<bool> = const { Cell::new(false) };
    /// An address to watch (0: none) and whether a block holding it was freed.
    pub(super) static WATCH: Cell<usize> = const { Cell::new(0) };
    pub(super) static WATCH_FREED: Cell<bool> = const { Cell::new(false) };
}

/// The test binary's allocator: the system's, failing on request so the
/// refusals of a fallible construction can be tried.
struct Failing;

// SAFETY: forwards to the system allocator; failing returns null, which the
// trait allows.
unsafe impl core::alloc::GlobalAlloc for Failing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let fail = FAIL_NTH.try_with(|n| {
            let left = n.get();
            if left == 0 {
                return false;
            }
            n.set(left - 1);
            left == 1
        });
        let sized =
            || FAIL_SIZE.try_with(|size| size.get() == layout.size() && size.replace(0) != 0);
        if fail == Ok(true) || sized() == Ok(true) {
            return ptr::null_mut();
        }
        // SAFETY: the caller's layout, forwarded.
        let block = unsafe { std::alloc::System.alloc(layout) };
        if !block.is_null() && HEAP_SHIELD.try_with(Cell::get) == Ok(false) {
            let _ = HEAP_LIVE.try_with(|live| {
                live.set(live.get() + layout.size());
                let _ = HEAP_PEAK.try_with(|peak| peak.set(peak.get().max(live.get())));
            });
        }
        block
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let _ = WATCH.try_with(|watch| {
            let at = watch.get();
            let start = ptr as usize;
            if at != 0 && at >= start && at < start + layout.size() {
                let _ = WATCH_FREED.try_with(|freed| freed.set(true));
            }
        });
        if HEAP_SHIELD.try_with(Cell::get) == Ok(false) {
            let _ = HEAP_LIVE.try_with(|live| live.set(live.get().saturating_sub(layout.size())));
        }
        // SAFETY: allocated by `alloc` above.
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Failing = Failing;
