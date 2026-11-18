//! The heap's peak since start, for test builds (`remote`): esp-alloc's
//! own (`max_usage`) counts what [`crate::largest_block`] takes to find
//! the largest free block, nearly all that is free, so every look at it
//! raised the peak to what was used then and that block. This one leaves
//! those allocations out (esp-alloc's `alloc-hooks`).

use core::cell::Cell;
use core::sync::atomic::{AtomicBool, Ordering};

use critical_section::Mutex;
use esp_alloc::export::enumset::EnumSet;
use esp_alloc::{EspHeap, MemoryCapability};

static PEAK: Mutex<Cell<usize>> = Mutex::new(Cell::new(0));
#[cfg(feature = "textprobe")]
#[path = "peak/scoped.rs"]
mod scoped;
#[cfg(feature = "textprobe")]
pub use scoped::ScopeSample;
#[cfg(feature = "textprobe")]
static SCOPE: Mutex<Cell<Option<scoped::Recorder>>> = Mutex::new(Cell::new(None));

static PROBING: AtomicBool = AtomicBool::new(false);

/// The most the heap held since start, the probes left out.
pub fn peak() -> usize {
    critical_section::with(|cs| PEAK.borrow(cs).get())
}

/// `probe`, its allocations not counted in the peak.
pub fn probing<R>(probe: impl FnOnce() -> R) -> R {
    PROBING.store(true, Ordering::Relaxed);
    let result = probe();
    PROBING.store(false, Ordering::Relaxed);
    result
}

/// esp-alloc calls it after each allocation, outside its lock.
#[unsafe(no_mangle)]
fn _esp_alloc_alloc(heap: &EspHeap, _: EnumSet<MemoryCapability>, ptr: usize, size: usize) {
    if PROBING.load(Ordering::Relaxed) {
        return;
    }
    #[cfg(not(feature = "textprobe"))]
    if ptr == 0 {
        return;
    }
    critical_section::with(|cs| {
        let used = heap.used();
        #[cfg(feature = "textprobe")]
        if let Some(mut scope) = SCOPE.borrow(cs).get() {
            scope.allocation(used, ptr, size);
            SCOPE.borrow(cs).set(Some(scope));
        }
        #[cfg(not(feature = "textprobe"))]
        let _ = size;
        let peak = PEAK.borrow(cs);
        if ptr != 0 && used > peak.get() {
            peak.set(used);
        }
    });
}

/// esp-alloc calls this before freeing. Only scoped request traffic needs it.
#[unsafe(no_mangle)]
fn _esp_alloc_dealloc(_: &EspHeap, ptr: usize, size: usize) {
    #[cfg(feature = "textprobe")]
    if !PROBING.load(Ordering::Relaxed) {
        critical_section::with(|cs| {
            if let Some(mut scope) = SCOPE.borrow(cs).get() {
                scope.deallocation(ptr, size);
                SCOPE.borrow(cs).set(Some(scope));
            }
        });
    }
    #[cfg(not(feature = "textprobe"))]
    let _ = (ptr, size);
}

/// Owns the only active test measurement. Dropping it cancels the measurement.
/// It cannot move between threads; the measured operation must finish locally.
#[cfg(feature = "textprobe")]
pub struct Scope {
    active: bool,
    _local: core::marker::PhantomData<*mut ()>,
}

/// Starts a scope at the allocator's actual used bytes. Nested scopes are refused.
/// Unrelated interrupt allocations are included; quiesce radios before measuring.
#[cfg(feature = "textprobe")]
pub fn begin_scope() -> Option<Scope> {
    critical_section::with(|cs| {
        let scope = SCOPE.borrow(cs);
        if scope.get().is_some() || PROBING.load(Ordering::Relaxed) {
            return None;
        }
        scope.set(Some(scoped::Recorder::new(esp_alloc::HEAP.used())));
        Some(Scope {
            active: true,
            _local: core::marker::PhantomData,
        })
    })
}

#[cfg(feature = "textprobe")]
impl Scope {
    /// Ends this scope without allocation. Take diagnostics outside this scope
    /// so largest-block probes and result formatting cannot affect its counters.
    pub fn finish(mut self) -> ScopeSample {
        let sample = critical_section::with(|cs| {
            let end = esp_alloc::HEAP.used();
            // This guard owns the scope; no other guard can remove it.
            SCOPE.borrow(cs).take().map_or_else(
                || ScopeSample {
                    overflowed: true,
                    ..ScopeSample::default()
                },
                |scope| scope.finish(end),
            )
        });
        self.active = false;
        sample
    }
}

#[cfg(feature = "textprobe")]
impl Drop for Scope {
    fn drop(&mut self) {
        if self.active {
            critical_section::with(|cs| SCOPE.borrow(cs).set(None));
        }
    }
}
