//! A global allocator that enforces a device's heap budget on the kernel.
//!
//! Only allocations made while [`track`] is active (i.e. inside kernel and
//! plugin calls) are counted, so the emulator's own window and graphics
//! code does not distort the numbers. When a tracked allocation would exceed
//! the budget it fails, exactly as it would on the device.
//!
//! The numbers approximate a real embedded allocator: each allocation is
//! charged a small fixed header. Fragmentation and stack usage are not
//! modelled, so keep a safety margin.
//!
//! Each allocation remembers whether it was charged (in a byte before
//! it), so memory is credited back exactly when charged memory is freed,
//! wherever that happens: the shell's memory freed by the kernel (e.g. a
//! received part of a request) does not lower the count.

use std::alloc::{GlobalAlloc, Layout, System};
use std::backtrace::Backtrace;
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Per-allocation bookkeeping overhead of typical embedded allocators.
const HEADER: usize = 8;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static LIMIT: AtomicUsize = AtomicUsize::new(usize::MAX);
static FAILED: AtomicUsize = AtomicUsize::new(0);
/// Allocations of at least this many bytes are printed.
static TRACE: AtomicUsize = AtomicUsize::new(usize::MAX);
/// Whether live allocations are recorded with their call site.
static PROFILE: AtomicBool = AtomicBool::new(false);
/// The size of the last tracked allocation refused, 0 since
/// [`clear_last_refused`] (see [`last_refused`]).
static LAST_REFUSED: AtomicUsize = AtomicUsize::new(0);
/// Live tracked allocations: address → (size, where it was made).
static SITES: Mutex<Option<HashMap<usize, Site>>> = Mutex::new(None);
/// Whether tracked allocations and their ends are kept ([`record_events`]).
static EVENTS_ON: AtomicBool = AtomicBool::new(false);
/// What [`take_events`] gives: made or ended, the address, the size asked.
static EVENTS: Mutex<Vec<(bool, usize, usize)>> = Mutex::new(Vec::new());
/// Whether the memory probe says the largest block ([`tell_largest_free`]).
static LARGEST_TOLD: AtomicBool = AtomicBool::new(false);
/// What was live at the peak (within [`PEAK_STEP`] of it), while
/// profiling: the heap then and its allocations.
static AT_PEAK: Mutex<Option<(usize, Vec<Site>)>> = Mutex::new(None);
/// How far the heap must pass the last look at the peak before it is
/// looked at again: often enough to be near the peak, rarely enough to
/// stay fast.
const PEAK_STEP: usize = 1024;

/// An allocation's size and where it was made.
type Site = (usize, Arc<Backtrace>);

thread_local! {
    static TRACKING: Cell<bool> = const { Cell::new(false) };
}

/// The emulator's global allocator.
#[derive(Debug)]
pub struct BudgetAllocator;

fn tracking() -> bool {
    TRACKING.try_with(Cell::get).unwrap_or(false)
}

/// The allocation made for `layout`, with room before it for the mark,
/// and where the memory handed out starts in it.
fn outer(layout: Layout) -> (Layout, usize) {
    let offset = layout.align().max(16);
    let size = layout.size().saturating_add(offset);
    // The size stays within what `Layout` allows for any real request.
    let outer = Layout::from_size_align(size, layout.align()).unwrap_or(layout);
    (outer, offset)
}

/// Marks of allocations: whether they were charged.
const CHARGED: u8 = 0xC4;
const FREE: u8 = 0x00;

/// Allocates `layout` with its mark.
///
/// # Safety
///
/// As [`GlobalAlloc::alloc`].
unsafe fn alloc_marked(layout: Layout, charged: bool) -> *mut u8 {
    let (outer, offset) = outer(layout);
    // SAFETY: `outer` has a non-zero size, as it holds the mark.
    let base = unsafe { System.alloc(outer) };
    if base.is_null() {
        return base;
    }
    // SAFETY: `offset` lies within the allocation, and the mark before it.
    unsafe {
        let ptr = base.add(offset);
        ptr.sub(1).write(if charged { CHARGED } else { FREE });
        ptr
    }
}

// SAFETY: forwards to the system allocator; the bookkeeping does not touch
// the allocated memory.
unsafe impl GlobalAlloc for BudgetAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !tracking() {
            // SAFETY: same contract as the caller's.
            return unsafe { alloc_marked(layout, false) };
        }
        let charge = layout.size() + HEADER;
        if LIVE.load(Ordering::Relaxed) + charge > LIMIT.load(Ordering::Relaxed) {
            FAILED.fetch_add(1, Ordering::Relaxed);
            LAST_REFUSED.store(layout.size().max(1), Ordering::Relaxed);
            return std::ptr::null_mut();
        }
        // SAFETY: same contract as the caller's.
        let ptr = unsafe { alloc_marked(layout, true) };
        if !ptr.is_null() {
            let live = LIVE.fetch_add(charge, Ordering::Relaxed) + charge;
            PEAK.fetch_max(live, Ordering::Relaxed);
            if layout.size() >= TRACE.load(Ordering::Relaxed) {
                // Untracked, so printing does not count against the budget.
                let tracking = TRACKING.with(|t| t.replace(false));
                eprintln!(
                    "alloc {:>7} B, heap {:>6.1} KiB  {}",
                    layout.size(),
                    live as f64 / 1024.0,
                    site_of(&Backtrace::force_capture(), 2)
                );
                TRACKING.with(|t| t.set(tracking));
            }
            if EVENTS_ON.load(Ordering::Relaxed) {
                note_event(true, ptr as usize, layout.size());
            }
            if PROFILE.load(Ordering::Relaxed) {
                // Untracked: the bookkeeping is not part of the budget.
                let tracking = TRACKING.with(|t| t.replace(false));
                let site = Arc::new(Backtrace::force_capture());
                if let Ok(mut sites) = SITES.lock() {
                    let sites = sites.get_or_insert_with(HashMap::new);
                    sites.insert(ptr as usize, (layout.size(), site));
                    if let Ok(mut at_peak) = AT_PEAK.lock()
                        && at_peak
                            .as_ref()
                            .is_none_or(|(at, _)| live >= at + PEAK_STEP)
                    {
                        *at_peak = Some((live, sites.values().cloned().collect()));
                    }
                }
                TRACKING.with(|t| t.set(tracking));
            }
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let (outer, offset) = outer(layout);
        // SAFETY: `ptr` came from `alloc_marked` with the same layout.
        let charged = unsafe { ptr.sub(1).read() } == CHARGED;
        // SAFETY: as above; this is the start of the allocation.
        unsafe { System.dealloc(ptr.sub(offset), outer) };
        if charged {
            if EVENTS_ON.load(Ordering::Relaxed) {
                note_event(false, ptr as usize, layout.size());
            }
            if PROFILE.load(Ordering::Relaxed) {
                let tracking = TRACKING.with(|t| t.replace(false));
                if let Ok(mut sites) = SITES.lock()
                    && let Some(sites) = sites.as_mut()
                {
                    sites.remove(&(ptr as usize));
                }
                TRACKING.with(|t| t.set(tracking));
            }
            let charge = layout.size() + HEADER;
            let mut live = LIVE.load(Ordering::Relaxed);
            while let Err(actual) = LIVE.compare_exchange_weak(
                live,
                live.saturating_sub(charge),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                live = actual;
            }
        }
    }
}

/// Keeps a tracked allocation (`made`) or its end for [`take_events`].
fn note_event(made: bool, address: usize, size: usize) {
    // Untracked: the bookkeeping is not part of the budget.
    let tracking = TRACKING.with(|t| t.replace(false));
    if let Ok(mut events) = EVENTS.lock() {
        events.push((made, address, size));
    }
    TRACKING.with(|t| t.set(tracking));
}

/// Runs `f` with allocation tracking enabled.
pub fn track<R>(f: impl FnOnce() -> R) -> R {
    // The counters are the whole process's: a test that runs a kernel takes
    // `one_device_at_a_time` (the lock the budget's own tests take), or its
    // allocations change what another test measures.
    #[cfg(test)]
    assert!(
        accounting_tests::held(),
        "a tracked call in a test without `crate::one_device_at_a_time()`"
    );
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            TRACKING.with(|t| t.set(self.0));
        }
    }
    let _restore = Restore(TRACKING.with(|t| t.replace(true)));
    f()
}

/// Runs `f` with allocation tracking off: for the emulator's own
/// bookkeeping inside kernel calls (the trace's records), which the
/// device does not have.
pub fn untracked<R>(f: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            TRACKING.with(|t| t.set(self.0));
        }
    }
    let _restore = Restore(TRACKING.with(|t| t.replace(false)));
    f()
}

/// Whether the budget was given on the command line, over the profile's.
static OVERRIDDEN: AtomicBool = AtomicBool::new(false);

/// Sets the heap budget; `None` removes it. A budget given on the command
/// line ([`override_limit`]) stays.
pub fn set_limit(limit: Option<usize>) {
    if !OVERRIDDEN.load(Ordering::Relaxed) {
        LIMIT.store(limit.unwrap_or(usize::MAX), Ordering::Relaxed);
    }
}

/// Sets the heap budget for the whole run, the profile's left aside.
pub fn override_limit(limit: Option<usize>) {
    LIMIT.store(limit.unwrap_or(usize::MAX), Ordering::Relaxed);
    OVERRIDDEN.store(true, Ordering::Relaxed);
}

/// The size of the last tracked allocation the budget refused since
/// [`clear_last_refused`], 0 if none was: where a program stopped in the
/// middle of a call (a browser's module traps where an allocation fails
/// that has no way around it), whether running out was why.
pub fn last_refused() -> usize {
    LAST_REFUSED.load(Ordering::Relaxed)
}

/// Forgets the last refusal (see [`last_refused`]), e.g. as a call
/// begins.
pub fn clear_last_refused() {
    LAST_REFUSED.store(0, Ordering::Relaxed);
}

/// Ends [`override_limit`]: the budget is `limit` again, and the next
/// device sets its profile's.
pub fn end_override(limit: Option<usize>) {
    OVERRIDDEN.store(false, Ordering::Relaxed);
    set_limit(limit);
}

/// Prints every tracked allocation of at least `bytes` to stderr, to find
/// what makes up a peak.
pub fn trace(bytes: usize) {
    TRACE.store(bytes, Ordering::Relaxed);
}

/// Records the call site of every tracked allocation for [`report`].
/// Slow; for analysing memory use only.
pub fn profile() {
    PROFILE.store(true, Ordering::Relaxed);
}

/// Keeps every tracked allocation and its end from now on, in order, for
/// [`take_events`]: what a test replays through a model of a device's heap
/// (the budget counts bytes; where the blocks lie it does not model).
pub fn record_events() {
    EVENTS_ON.store(true, Ordering::Relaxed);
}

/// The tracked allocations and their ends since the last call
/// ([`record_events`]): whether made (or ended), its address, the size asked.
pub fn take_events() -> Vec<(bool, usize, usize)> {
    let tracking = TRACKING.with(|t| t.replace(false));
    let events = EVENTS
        .lock()
        .map(|mut events| std::mem::take(&mut *events))
        .unwrap_or_default();
    TRACKING.with(|t| t.set(tracking));
    events
}

/// The memory probe ([`memory_use`]) says a largest block from now on, all
/// that is free: the kernel then takes what it takes on a device that tells
/// one (a frame's taller band), which it does not where none is told.
pub fn tell_largest_free() {
    LARGEST_TOLD.store(true, Ordering::Relaxed);
}

/// The first frames in PaperPaw code below the allocator, e.g.
/// `paperpaw_text::cache::GlyphCache::insert (cache.rs:120)`.
fn site_of(backtrace: &Backtrace, depth: usize) -> String {
    site_in(&backtrace.to_string(), depth)
}

/// [`site_of`] a backtrace's text. A method of a type is named as the
/// type in angle brackets (`<paperpaw_core::Kernel>::start`), and counts
/// as PaperPaw's as a function does.
fn site_in(text: &str, depth: usize) -> String {
    let mut frames = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let Some((_, name)) = line.trim().split_once(": ") else {
            continue;
        };
        let location = lines
            .peek()
            .and_then(|l| l.trim().strip_prefix("at "))
            .map(|l| {
                let file = l.rsplit(['\\', '/']).next().unwrap_or(l);
                file.rsplit_once(':').map_or(file, |(f, _)| f).to_string()
            })
            .unwrap_or_default();
        let name = name.rsplit_once("::h").map_or(name, |(n, _)| n);
        if name.trim_start_matches('<').starts_with("paperpaw")
            && !name.contains("budget")
            && !name.contains("__rust_")
        {
            frames.push(format!("{name} ({location})"));
            if frames.len() == depth {
                break;
            }
        } else if !frames.is_empty() && name.contains("paperpaw_emu") {
            break;
        }
    }
    if frames.is_empty() {
        String::from("(outside PaperPaw code)")
    } else {
        frames.join(
            "
      <- ",
        )
    }
}

/// Prints the live allocations grouped by call site, largest first.
pub fn report(depth: usize) {
    let tracking = TRACKING.with(|t| t.replace(false));
    let sites = SITES
        .lock()
        .ok()
        .and_then(|mut s| s.take())
        .unwrap_or_default();
    print_groups("live heap by call site", sites.values(), depth);
    if let Some((_, at_peak)) = AT_PEAK.lock().ok().and_then(|mut s| s.take()) {
        print_groups("heap at the peak by call site", at_peak.iter(), depth);
    }
    TRACKING.with(|t| t.set(tracking));
}

/// Prints `sites` grouped by call site, largest first, under `title`.
fn print_groups<'a>(title: &str, sites: impl Iterator<Item = &'a Site>, depth: usize) {
    let mut groups: HashMap<String, (usize, usize)> = HashMap::new();
    for (size, backtrace) in sites {
        let group = groups.entry(site_of(backtrace, depth)).or_default();
        group.0 += size + HEADER;
        group.1 += 1;
    }
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by_key(|(_, (bytes, _))| std::cmp::Reverse(*bytes));
    let total: usize = groups.iter().map(|(_, (bytes, _))| bytes).sum();
    println!("{title} ({:.1} KiB):", total as f64 / 1024.0);
    for (site, (bytes, count)) in groups {
        println!("{:>8.1} KiB {count:>6}x  {site}", bytes as f64 / 1024.0);
    }
}

/// Starts the peak anew from what is live now, e.g. to measure the peak
/// of one screen.
pub fn reset_peak() {
    PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
    if let Ok(mut at_peak) = AT_PEAK.lock() {
        *at_peak = None;
    }
}

/// Live tracked allocations grouped by call site (`depth` frames of
/// PaperPaw code each), largest first: site, bytes, count. Empty unless
/// [`profile`] was called.
pub fn sites(depth: usize) -> Vec<(String, usize, usize)> {
    let tracking = TRACKING.with(|t| t.replace(false));
    let mut groups: HashMap<String, (usize, usize)> = HashMap::new();
    if let Ok(sites) = SITES.lock()
        && let Some(sites) = sites.as_ref()
    {
        for (size, backtrace) in sites.values() {
            let group = groups.entry(site_of(backtrace, depth)).or_default();
            group.0 += size + HEADER;
            group.1 += 1;
        }
    }
    let mut groups: Vec<_> = groups
        .into_iter()
        .map(|(site, (bytes, count))| (site, bytes, count))
        .collect();
    groups.sort_by_key(|(_, bytes, _)| std::cmp::Reverse(*bytes));
    TRACKING.with(|t| t.set(tracking));
    groups
}

/// Current heap usage of the kernel and its plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub live: usize,
    pub peak: usize,
    pub limit: Option<usize>,
    /// Allocations refused because of the budget.
    pub failed: usize,
}

pub fn stats() -> Stats {
    let limit = LIMIT.load(Ordering::Relaxed);
    Stats {
        live: LIVE.load(Ordering::Relaxed),
        peak: PEAK.load(Ordering::Relaxed),
        limit: (limit != usize::MAX).then_some(limit),
        failed: FAILED.load(Ordering::Relaxed),
    }
}

/// The heap as the kernel's memory probe reports it
/// (`KernelBuilder::memory`): the budget and what is left of it; nothing
/// without a budget, where the heap has no size. The stack is not
/// modelled.
pub fn memory_use() -> Option<paperpaw_core::sdk::memory::MemoryUse> {
    let stats = stats();
    let size = stats.limit?;
    let bytes = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let free = bytes(size.saturating_sub(stats.live));
    let used = paperpaw_core::sdk::memory::MemoryUse::new(free, bytes(size));
    Some(if LARGEST_TOLD.load(Ordering::Relaxed) {
        used.with_largest_free(free)
    } else {
        used
    })
}

impl std::fmt::Display for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kib = |bytes: usize| bytes as f64 / 1024.0;
        write!(f, "heap {:.1} KiB", kib(self.live))?;
        if let Some(limit) = self.limit {
            write!(f, " / {:.0} KiB", kib(limit))?;
        }
        write!(f, " (peak {:.1} KiB)", kib(self.peak))
    }
}

#[cfg(test)]
pub(crate) mod accounting_tests;
