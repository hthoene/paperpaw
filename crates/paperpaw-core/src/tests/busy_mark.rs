//! The busy mark's times across starts (#118): on the X4 every wake is a
//! start, so what was timed is kept in a small file, written when the
//! device sleeps or stops and only where it changed what is slow.

use super::*;

const TIMING_FILE: &str = ".paperpaw/core/timing.txt";

fn kernel(storage: paperpaw_sdk::MemoryStorage) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(Opener)
        .busy_mark(true)
        .clock(still_clock)
        .storage(storage)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    kernel
}

fn kept(kernel: &Kernel) -> Option<String> {
    paperpaw_sdk::read_file(kernel.storage(), TIMING_FILE, 4096)
        .ok()
        .map(|bytes| String::from_utf8(bytes).unwrap())
}

#[test]
fn a_screen_quick_at_the_last_start_shows_no_mark_after_a_restart() {
    let file = "paperpaw-timing 1\n173 screen:core.placeholder\n";
    let mut kernel = kernel(paperpaw_sdk::MemoryStorage::new().with(TIMING_FILE, file));
    let effects = press(&mut kernel, 100, "confirm");
    assert!(!effects.proceed());
    assert_eq!(kernel.active_screen(), Some(&screens::PLACEHOLDER));
}

#[test]
fn going_back_shows_no_mark_the_first_time() {
    let mut kernel = kernel(paperpaw_sdk::MemoryStorage::new());
    assert!(press(&mut kernel, 100, "confirm").proceed());
    kernel.proceed(Instant(200));
    render(&mut kernel);
    assert!(!press(&mut kernel, 300, "back").proceed());
    assert_eq!(
        kernel.active_screen().map(Id::as_str),
        Some("opener.screen")
    );
}

#[test]
fn times_are_written_at_sleep_only_where_they_changed_what_is_slow() {
    let mut kernel = kernel(paperpaw_sdk::MemoryStorage::new());
    assert!(press(&mut kernel, 100, "confirm").proceed());
    kernel.proceed(Instant(200));
    render(&mut kernel);
    assert_eq!(kept(&kernel), None);
    kernel.suspend(Instant(300));
    let text = kept(&kernel).unwrap();
    assert!(text.starts_with("paperpaw-timing 1\n"), "{text}");
    assert!(text.contains(" screen:core.placeholder\n"), "{text}");
    // Woken, the same quick work again: nothing new to keep.
    kernel.storage().remove(TIMING_FILE).unwrap();
    kernel.resume(Instant(400));
    press(&mut kernel, 500, "back");
    press(&mut kernel, 600, "confirm");
    render(&mut kernel);
    let back_was_new = kept(&kernel);
    kernel.suspend(Instant(700));
    // Back was new (a key not timed before), so the file was written once.
    assert_eq!(back_was_new, None);
    assert!(kept(&kernel).unwrap().contains(" back:opener.screen\n"));
    kernel.storage().remove(TIMING_FILE).unwrap();
    kernel.resume(Instant(800));
    press(&mut kernel, 900, "back");
    press(&mut kernel, 1000, "confirm");
    kernel.stop(Instant(1100));
    assert_eq!(kept(&kernel), None);
}

#[test]
fn a_damaged_timing_file_starts_with_no_times() {
    for file in [
        &b"\xff\xfe garbage"[..],
        b"paperpaw-timing 9\n0 screen:core.placeholder\n",
    ] {
        let storage = paperpaw_sdk::MemoryStorage::new().with(TIMING_FILE, file);
        let mut kernel = kernel(storage);
        // Not timed: the mark first, as on a first start.
        assert!(press(&mut kernel, 100, "confirm").proceed());
    }
}
