//! When a bonded remote connects, with the real kernel: by hand (never by
//! itself), while reading (a book opened and closed), always (at start
//! and on waking).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::sdk::builtin::events;
use paperpaw_core::sdk::secrets::{FILE, SecretChange, Secrets};
use paperpaw_core::sdk::{EventData, MemoryStorage};
use paperpaw_core::{BtCall, Io, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_plugin_bluetooth::{Bluetooth, REMOTES, WHILE_AWAKE, WHILE_READING};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, true, false, &[]),
    MemoryClass::Small,
);

/// A kernel with a remote bonded before and `remotes` chosen.
fn kernel(remotes: &str) -> Kernel {
    kernel_with(remotes, false)
}

/// The same, with the remote's keys switched on (`keys`) or not.
fn kernel_with(remotes: &str, keys: bool) -> Kernel {
    let mut secrets = Secrets::default();
    let bond = format!("aa:bb\tRemote\tremote\t{}\t\tkeys", u8::from(keys));
    secrets.apply(SecretChange::keep("core.bluetooth.1", "Remote", &bond));
    let mut kernel = Kernel::builder(DEVICE)
        .storage(MemoryStorage::new().with(FILE, secrets.encode()))
        .user_config(format!("[bluetooth]\nremotes = {remotes}\n"))
        .bluetooth(2)
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(Bluetooth)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

/// Whether the kernel asked the shell to connect the remote.
fn connects(kernel: &mut Kernel) -> bool {
    kernel.take_io().into_iter().any(
        |io| matches!(io, Io::Bluetooth(BtCall::Connect { address, .. }) if address == "aa:bb"),
    )
}

fn reading() -> paperpaw_core::sdk::Params {
    let book = events::Reading::new("a.epub".into(), "A".into(), 0.0, 0, 0, String::new());
    let event = events::BOOK_OPENED.emit(book);
    match event {
        paperpaw_core::sdk::Command::Emit(_, params) => params,
        _ => unreachable!(),
    }
}

#[test]
fn by_hand_never_by_itself() {
    let mut kernel = kernel("bluetooth.remotes.hand");
    assert!(!connects(&mut kernel));
    kernel.send_event(Instant(10), events::BOOK_OPENED.id().clone(), reading());
    kernel.resume(Instant(20));
    assert!(!connects(&mut kernel));
}

#[test]
fn switched_on_by_hand_at_start_and_on_waking() {
    let mut kernel = kernel_with("bluetooth.remotes.hand", true);
    assert!(connects(&mut kernel));
    kernel.suspend(Instant(10));
    kernel.take_io();
    kernel.resume(Instant(20));
    assert!(connects(&mut kernel));
}

#[test]
fn while_reading_a_book_opened_and_closed() {
    let mut kernel = kernel(WHILE_READING);
    assert!(!connects(&mut kernel), "not at start, no book open");
    kernel.send_event(Instant(10), events::BOOK_OPENED.id().clone(), reading());
    assert!(connects(&mut kernel));
    kernel.send_event(Instant(20), events::BOOK_CLOSED.id().clone(), reading());
    let calls = kernel.take_io();
    assert!(
        calls
            .iter()
            .any(|io| matches!(io, Io::Bluetooth(BtCall::Disconnect { .. }))),
        "{calls:?}"
    );
    let _ = REMOTES;
}

#[test]
fn always_at_start_and_on_waking() {
    let mut kernel = kernel(WHILE_AWAKE);
    assert!(connects(&mut kernel));
    kernel.suspend(Instant(10));
    kernel.take_io();
    kernel.resume(Instant(20));
    assert!(connects(&mut kernel));
    let _ = paperpaw_core::sdk::Params::new().to_params();
}
