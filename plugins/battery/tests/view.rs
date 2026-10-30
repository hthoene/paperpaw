//! The battery plugin with the real kernel: its view turns pages where
//! the recent times awake do not fit, and its log stays small where the
//! old log cannot be replaced.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::collections::BTreeSet;
use std::fmt::Write as _;

use paperpaw_core::Kernel;
use paperpaw_platform::{ButtonState, DirEntry, InputEvent, Rotation};
use paperpaw_plugin_battery::{Battery, OPEN};
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, MemoryStorage, PixelFormat, Storage,
    StorageError,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Large,
);

const LOG: &str = ".paperpaw/plugins/battery/log.csv";
const OLD: &str = ".paperpaw/plugins/battery/log.old.csv";
const HEADER: &str =
    "kind,ms,millivolts,percent,charging,fast,partial,full,kilopixels,inputs,panel_ms,pages,unix\n";

/// A card where nothing can be renamed.
struct NoRename(MemoryStorage);

impl Storage for NoRename {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }

    fn rename(&self, _: &str, _: &str) -> Result<(), StorageError> {
        Err(StorageError::Io)
    }
}

fn kernel(storage: impl Storage + 'static) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .plugin(paperpaw_plugin_fonts_default::DefaultFonts)
        .plugin(Battery)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

fn press(kernel: &mut Kernel, at: u64, name: &'static str) {
    for (later, state) in [(0, ButtonState::Pressed), (50, ButtonState::Released)] {
        kernel.handle(Instant(at + later), InputEvent::Button { name, state });
    }
    kernel.render(&mut |_, _| {});
}

fn labels(kernel: &mut Kernel) -> BTreeSet<String> {
    kernel.describe().into_iter().map(|p| p.label).collect()
}

#[test]
fn recent_times_awake_that_do_not_fit_are_a_page_on() {
    // Thirty times awake of ten minutes and more, a percent each.
    let mut log = String::from(HEADER);
    for i in 0..30u32 {
        let percent = 95 - i;
        writeln!(log, "start,0,4100,{percent},0,0,0,0,0,0,0,{i},").unwrap();
        writeln!(
            log,
            "end,{},4000,{},0,5,20,2,9600,60,18000,{},",
            600_000 + i * 60_000,
            percent - 1,
            i + 7
        )
        .unwrap();
    }
    let mut kernel = kernel(MemoryStorage::new().with(LOG, log));
    kernel.run_action(Instant(100), OPEN);
    let first = labels(&mut kernel);
    press(&mut kernel, 200, "page-forward");
    let second = labels(&mut kernel);
    assert!(
        second.difference(&first).next().is_some(),
        "more on the next page: {second:?}"
    );
    press(&mut kernel, 400, "page-back");
    assert_eq!(labels(&mut kernel), first, "and back");
}

#[test]
fn a_full_log_starts_anew_where_it_cannot_be_renamed() {
    let full = format!("{HEADER}{}", "sample,1,1,1,0,0,0,0,0,0,0,0,\n".repeat(5000));
    assert!(full.len() > 128 * 1024);
    let storage = MemoryStorage::new()
        .with(LOG, full.clone())
        .with(OLD, "old");
    let kernel = kernel(NoRename(storage));
    let size = kernel.storage().size(LOG).unwrap();
    assert!(size < 4096, "started anew: {size} bytes");
}
