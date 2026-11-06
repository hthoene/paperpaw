//! The card's free space counted in the kernel's idle ticks (Senior's
//! remark 1 on PR #188, `free_count`): from the start until known, a part
//! at each tick with a pause between, again on waking; never while a
//! press waits, nor just before sleep; not on a storage that counts
//! nothing.

use core::cell::Cell;

use super::*;
use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_sdk::MemoryStorage;

/// A storage whose free space is known after `parts` asks, as a FAT
/// storage's once its FAT is counted; `asks` counts them.
#[derive(Clone)]
struct Counting {
    inner: alloc::rc::Rc<MemoryStorage>,
    parts: alloc::rc::Rc<Cell<u32>>,
    asks: alloc::rc::Rc<Cell<u32>>,
    sized: alloc::rc::Rc<Cell<bool>>,
}

impl Counting {
    fn new(parts: u32, sized: bool) -> Self {
        Self {
            inner: alloc::rc::Rc::default(),
            parts: alloc::rc::Rc::new(Cell::new(parts)),
            asks: alloc::rc::Rc::default(),
            sized: alloc::rc::Rc::new(Cell::new(sized)),
        }
    }
}

impl Storage for Counting {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
    fn free_space(&self) -> Option<u64> {
        self.asks.set(self.asks.get() + 1);
        let left = self.parts.get();
        self.parts.set(left.saturating_sub(1));
        (left <= 1).then_some(1 << 20)
    }
    fn total_space(&self) -> Option<u64> {
        self.sized.get().then_some(1 << 30)
    }
}

fn kernel(storage: &Counting) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .sleep(true)
        .setting_default(&settings::SLEEP_AFTER, 1)
        .storage(storage.clone())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

/// Ticks at every wake-up the kernel asks for, up to `until`.
fn tick_through(kernel: &mut Kernel, from: crate::Effects, until: u64) {
    let mut wake = from.wake_at();
    while let Some(at) = wake.filter(|at| at.0 <= until) {
        wake = kernel.tick(at).wake_at();
    }
}

#[test]
fn the_count_is_driven_in_ticks_until_known() {
    let storage = Counting::new(5, true);
    let mut kernel = kernel(&storage);
    // Not in the start itself: a second later, its first frames shown.
    assert_eq!(storage.asks.get(), 0);
    let effects = kernel.tick(Instant(500));
    assert_eq!(storage.asks.get(), 0);
    assert!(effects.wake_at().is_some_and(|at| at.0 <= 1_000));
    tick_through(&mut kernel, effects, 5_000);
    // A part at each tick, a pause between: 5 asks, then no more.
    assert_eq!(storage.asks.get(), 5);
    kernel.tick(Instant(10_000));
    assert_eq!(storage.asks.get(), 5);
}

#[test]
fn waking_starts_the_count_again() {
    let storage = Counting::new(1, true);
    let mut kernel = kernel(&storage);
    let effects = kernel.tick(Instant(1_000));
    tick_through(&mut kernel, effects, 5_000);
    assert_eq!(storage.asks.get(), 1);
    // Asleep, and another card in when it wakes: counted from its start.
    kernel.suspend(Instant(20_000));
    storage.parts.set(3);
    let effects = kernel.resume(Instant(60_000));
    tick_through(&mut kernel, effects, 65_000);
    assert_eq!(storage.asks.get(), 4);
}

#[test]
fn no_part_is_counted_just_before_sleep() {
    let storage = Counting::new(100_000, true);
    let mut kernel = kernel(&storage);
    // Sleep after a minute: from 58 s on nothing is counted.
    let effects = kernel.tick(Instant(1_000));
    tick_through(&mut kernel, effects, 57_900);
    let before = storage.asks.get();
    assert!(before > 100, "{before}");
    let effects = kernel.tick(Instant(58_100));
    assert_eq!(storage.asks.get(), before);
    assert!(effects.wake_at().is_some());
    assert_eq!(kernel.free_count.due(), kernel.sleep_due());
}

#[test]
fn a_storage_that_cannot_tell_its_size_is_not_asked() {
    let storage = Counting::new(5, false);
    let mut kernel = kernel(&storage);
    let effects = kernel.tick(Instant(1_000));
    tick_through(&mut kernel, effects, 5_000);
    assert_eq!(storage.asks.get(), 0);
}

fn waiting_press_resumes_count(sleep_after: i32) {
    let storage = Counting::new(5, true);
    let mut kernel = kernel(&storage);
    kernel
        .shared
        .settings
        .set(
            settings::SLEEP_AFTER.as_str(),
            paperpaw_sdk::SettingValue::Int(sleep_after),
        )
        .unwrap();
    kernel.input.pending = Some((
        crate::keymap::InputKey::Button("next".into()),
        Instant(1_250),
        None,
    ));
    kernel.count_free(Instant(1_000));
    assert_eq!(storage.asks.get(), 0);
    assert_eq!(kernel.free_count.due(), Some(Instant(1_250)));
    kernel.count_free(Instant(1_249));
    assert_eq!(storage.asks.get(), 0);
    // The normal gesture path clears the pending press when it is decided.
    kernel.input.pending = None;
    kernel.count_free(Instant(1_250));
    assert_eq!(storage.asks.get(), 1);
    assert_eq!(kernel.free_count.due(), Some(Instant(1_300)));
}

#[test]
fn a_waiting_press_with_sleep_off_resumes_the_count() {
    waiting_press_resumes_count(0);
}

#[test]
fn a_waiting_press_does_not_defer_to_a_ten_minute_sleep_timeout() {
    waiting_press_resumes_count(10);
}

#[test]
fn sleeping_stops_counting_until_wake() {
    let storage = Counting::new(5, true);
    let mut kernel = kernel(&storage);
    kernel.sleep = true;
    kernel.count_free(Instant(1_000));
    assert_eq!(storage.asks.get(), 0);
    assert_eq!(kernel.free_count.due(), None);
}

#[test]
fn a_lost_card_stops_the_count() {
    let storage = Counting::new(5, true);
    let mut kernel = kernel(&storage);
    kernel.count_free(Instant(1_000));
    assert_eq!(storage.asks.get(), 1);
    storage.sized.set(false);
    kernel.count_free(Instant(1_050));
    assert_eq!(kernel.free_count.due(), None);
    assert_eq!(storage.asks.get(), 1);
}

#[test]
fn a_storage_that_never_finishes_stops_at_the_part_limit() {
    let storage = Counting::new(100_000, true);
    let mut kernel = kernel(&storage);
    // A hold or no sleep setting lets the full attempt budget run.
    kernel
        .shared
        .settings
        .set(
            settings::SLEEP_AFTER.as_str(),
            paperpaw_sdk::SettingValue::Int(0),
        )
        .unwrap();
    for n in 0..4_100 {
        kernel.count_free(Instant(1_000 + n * 50));
    }
    assert_eq!(storage.asks.get(), 4_096);
    assert_eq!(kernel.free_count.due(), None);
}

#[test]
fn usb_handoff_pauses_the_count_until_the_card_returns() {
    let storage = Counting::new(5, true);
    let mut kernel = kernel(&storage);
    kernel.free_count.pause_for_usb();
    kernel.count_free(Instant(1_000));
    assert_eq!(storage.asks.get(), 0);
    kernel.usb_drive_ended(Instant(2_000));
    kernel.count_free(Instant(3_000));
    assert_eq!(storage.asks.get(), 1);
}
