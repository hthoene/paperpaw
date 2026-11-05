//! Host allocation bound for removal; this is not a device heap measurement.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "storage regression assertions"
)]
mod common;
use common::{Bytes, exfat, exfat_card};
use paperpaw_platform::Storage;
fn small() -> Bytes {
    exfat_card()
}
fn mount(bytes: &Bytes) -> paperpaw_fat::ExFatStorage<Bytes> {
    exfat(bytes)
}

struct AuditAlloc;
static LIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static PEAK: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[global_allocator]
static AUDIT_ALLOC: AuditAlloc = AuditAlloc;
unsafe impl std::alloc::GlobalAlloc for AuditAlloc {
    unsafe fn alloc(&self, l: std::alloc::Layout) -> *mut u8 {
        let p = unsafe { std::alloc::GlobalAlloc::alloc(&std::alloc::System, l) };
        if !p.is_null() {
            let n = LIVE.fetch_add(l.size(), std::sync::atomic::Ordering::Relaxed) + l.size();
            PEAK.fetch_max(n, std::sync::atomic::Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: std::alloc::Layout) {
        LIVE.fetch_sub(l.size(), std::sync::atomic::Ordering::Relaxed);
        unsafe { std::alloc::GlobalAlloc::dealloc(&std::alloc::System, p, l) };
    }
}
#[test]
fn removing_long_names_keeps_extra_heap_below_32_kib() {
    let bytes = small();
    let card = mount(&bytes);
    for n in 0..1600 {
        card.write(&format!("books/{n:04}{}.txt", "x".repeat(180)), b"")
            .unwrap();
    }
    card.sync().unwrap();
    drop(card);
    let card = mount(&bytes);
    let base = LIVE.load(std::sync::atomic::Ordering::Relaxed);
    PEAK.store(base, std::sync::atomic::Ordering::Relaxed);
    card.remove_dir_all("books").unwrap();
    let extra = PEAK
        .load(std::sync::atomic::Ordering::Relaxed)
        .saturating_sub(base);
    eprintln!(
        "remove 1600 long names: extra host allocation peak={extra} B, name bytes alone={}",
        1600 * 188
    );
    assert!(
        extra < 32 * 1024,
        "removal exceeded its 32 KiB host allocation budget: {extra} B"
    );
}
