//! The button of the dual-boot notice (#224): "This is not a dual-boot
//! setup" asks first (an update can overwrite another firmware), then writes
//! `dual-boot = off` in `[firmware]` of `paperpaw.ini`, takes the note away
//! and restarts; where the file says off, "Detect dual boot again" writes
//! `auto` the same way.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::Kernel;
use paperpaw_platform::Rotation;
use paperpaw_plugin_update::{OPEN, Update};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::firmware::Firmware;
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage,
    PixelFormat, Storage, StorageError,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);
const NOTE: &str = ".paperpaw/boot.txt";
const INI: &str = "paperpaw.ini";

struct Device {
    kernel: Kernel,
    now: u64,
    restarted: bool,
}

impl Device {
    fn new(storage: impl Storage + 'static, firmware: bool) -> Self {
        let mut builder = Kernel::builder(DEVICE)
            .storage(storage)
            .network(true)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Update);
        if firmware {
            builder = builder.firmware(
                Firmware::new("0.4.0", "paperpaw-xteink", 5)
                    .keys(vec![paperpaw_update::public_key(&[3; 32])])
                    .room(1_000_000),
            );
        }
        let mut kernel = builder.build().unwrap();
        kernel.start(Instant(0));
        Self {
            kernel,
            now: 0,
            restarted: false,
        }
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.restarted |= effects.restart();
    }

    fn shown(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn file(&self, path: &str) -> Option<String> {
        let storage = self.kernel.storage();
        let size = storage.size(path).ok()?;
        let mut data = vec![0u8; size as usize];
        let n = storage.read_at(path, 0, &mut data).ok()?;
        String::from_utf8(data[..n].to_vec()).ok()
    }

    fn screen(&self) -> Option<&str> {
        self.kernel.active_screen().map(Id::as_str)
    }
}

const BEFORE: &str =
    "# my settings\n[core]\nui-size = 12\n[firmware]\ndual-boot = on\n[plugins]\nclock = on\n";

fn dual(ini: &str) -> MemoryStorage {
    MemoryStorage::new()
        .with(INI, ini)
        .with(NOTE, "dual-boot source=ini slot=1\n")
}

#[test]
fn the_notice_has_the_button_and_the_question_warns_before_anything_is_written() {
    let mut device = Device::new(dual(BEFORE), true);
    device.run(OPEN);
    let shown = device.shown();
    assert!(shown.contains("This is not a dual-boot setup"), "{shown}");
    device.run(actions::CONFIRM);
    assert_eq!(device.screen(), Some("core.confirm-dialog"));
    let shown = device.shown();
    assert!(shown.contains("can overwrite it"), "{shown}");
    assert!(
        shown.contains("Cancel") && shown.contains("Yes, restart"),
        "{shown}"
    );
    // Nothing yet.
    assert_eq!(device.file(INI).as_deref(), Some(BEFORE));
    assert!(device.file(NOTE).is_some());
    assert!(!device.restarted);
}

#[test]
fn cancelling_or_going_back_changes_nothing() {
    for (name, answer) in [("back", actions::BACK), ("cancel", actions::CONFIRM)] {
        let mut device = Device::new(dual(BEFORE), true);
        device.run(OPEN);
        device.run(actions::CONFIRM);
        // The focus starts on Cancel.
        device.run(answer);
        assert_eq!(device.file(INI).as_deref(), Some(BEFORE), "{name}");
        assert!(device.file(NOTE).is_some(), "{name}");
        assert!(!device.restarted, "{name}");
    }
}

/// The reader answers yes: the focus moves to it and is confirmed.
fn yes(device: &mut Device) {
    device.run(actions::LEFT);
    device.run(actions::CONFIRM);
}

#[test]
fn yes_writes_off_keeps_the_rest_of_the_file_and_restarts() {
    for firmware in [true, false] {
        let mut device = Device::new(dual(BEFORE), firmware);
        device.run(OPEN);
        device.run(actions::CONFIRM);
        yes(&mut device);
        let written = device.file(INI).unwrap();
        assert_eq!(
            written,
            BEFORE.replace("dual-boot = on", "dual-boot = off"),
            "firmware info: {firmware}"
        );
        // The firmware's note is made anew at the restart.
        assert!(device.file(NOTE).is_none(), "firmware info: {firmware}");
        assert!(device.restarted, "firmware info: {firmware}");
    }
}

#[test]
fn the_choice_is_added_where_the_file_has_none_or_has_no_file() {
    for (before, after) in [
        (
            "[core]\nui-size = 12\n",
            "[core]\nui-size = 12\n\n[firmware]\ndual-boot = off\n",
        ),
        ("", "[firmware]\ndual-boot = off\n"),
    ] {
        let storage = MemoryStorage::new().with(NOTE, "dual-boot source=marker slot=1\n");
        let storage = if before.is_empty() {
            storage
        } else {
            storage.with(INI, before)
        };
        let mut device = Device::new(storage, true);
        device.run(OPEN);
        device.run(actions::CONFIRM);
        yes(&mut device);
        assert_eq!(device.file(INI).as_deref(), Some(after));
        assert!(device.restarted);
    }
}

#[test]
fn a_file_that_cannot_be_read_whole_is_not_written_over() {
    let storage = MemoryStorage::new()
        .with(INI, vec![0xFF, 0xFE, b'[', 0x80])
        .with(NOTE, "dual-boot source=ini slot=1\n");
    let mut device = Device::new(storage, true);
    device.run(OPEN);
    device.run(actions::CONFIRM);
    yes(&mut device);
    let storage = device.kernel.storage();
    let mut data = [0u8; 8];
    let n = storage.read_at(INI, 0, &mut data).unwrap();
    assert_eq!(&data[..n], &[0xFF, 0xFE, b'[', 0x80]);
    assert!(device.file(NOTE).is_some());
    assert!(!device.restarted);
}

/// A storage whose reads of the ini end early: the size is the whole file,
/// the reads give half of it, then nothing (a card that fails half way).
struct ShortReads(MemoryStorage);

impl Storage for ShortReads {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if path != INI {
            return self.0.read_at(path, offset, buf);
        }
        let half = self.0.size(path)? / 2;
        if offset >= half {
            return Ok(0);
        }
        let room = usize::try_from(half - offset).unwrap_or(usize::MAX);
        let n = buf.len().min(room);
        self.0.read_at(path, offset, &mut buf[..n])
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
}

#[test]
fn a_read_that_ends_early_does_not_write_the_part_it_got_over_the_file() {
    // The half read is valid text; written back, the rest would be lost.
    let storage = ShortReads(dual(BEFORE));
    let mut device = Device::new(storage, true);
    device.run(OPEN);
    device.run(actions::CONFIRM);
    yes(&mut device);
    assert!(!device.restarted);
    let storage = device.kernel.storage();
    let mut data = vec![0u8; BEFORE.len()];
    let n = storage.read_at(NOTE, 0, &mut data).unwrap();
    assert!(n > 0, "the note is kept");
    let inner = storage.size(INI).unwrap();
    assert_eq!(inner, BEFORE.len() as u64, "the file is as long as it was");
}

/// A comment-only ini of `len` bytes, lines of 80 bytes.
fn comments(len: usize) -> String {
    let mut text = String::new();
    while text.len() + 80 < len {
        text.push_str(&format!("# {}\n", "x".repeat(77)));
    }
    let rest = len - text.len();
    text.push_str(&format!("#{}\n", "y".repeat(rest - 2)));
    assert_eq!(text.len(), len);
    text
}

#[test]
fn a_file_the_choice_would_grow_past_what_is_read_is_not_written() {
    const MAX: usize = 64 * 1024;
    let added = "\n[firmware]\ndual-boot = off\n".len();
    // One byte too many: not written, said so, the note kept, no restart.
    let big = comments(MAX - added + 1);
    let mut device = Device::new(dual(&big), true);
    device.run(OPEN);
    device.run(actions::CONFIRM);
    yes(&mut device);
    assert!(!device.restarted);
    assert_eq!(device.kernel.storage().size(INI).unwrap(), big.len() as u64);
    assert!(device.file(NOTE).is_some());
    // Exactly the size the readers take: written.
    let fits = comments(MAX - added);
    let mut device = Device::new(dual(&fits), true);
    device.run(OPEN);
    device.run(actions::CONFIRM);
    yes(&mut device);
    assert!(device.restarted);
    assert_eq!(device.kernel.storage().size(INI).unwrap(), MAX as u64);
    assert!(
        device
            .file(INI)
            .unwrap()
            .ends_with("[firmware]\ndual-boot = off\n")
    );
}

#[test]
fn where_the_file_says_off_the_page_offers_the_way_back() {
    for spelling in ["off", "OFF", "false", "no", "0"] {
        let ini = format!("[firmware]\ndual-boot = {spelling}\n");
        let mut device = Device::new(MemoryStorage::new().with(INI, ini.as_str()), true);
        device.run(OPEN);
        let shown = device.shown();
        assert!(
            shown.contains("Detect dual boot again"),
            "{spelling}: {shown}"
        );
        assert!(
            !shown.contains("This is not a dual-boot setup"),
            "{spelling}: {shown}"
        );
    }
    // Not where it says anything else, or nothing.
    for ini in [
        "[firmware]\ndual-boot = on\n",
        "[firmware]\ndual-boot = auto\n",
        "[core]\nui-size = 12\n",
        "",
    ] {
        let mut device = Device::new(MemoryStorage::new().with(INI, ini), true);
        device.run(OPEN);
        let shown = device.shown();
        assert!(
            !shown.contains("Detect dual boot again"),
            "{ini:?}: {shown}"
        );
    }
}

#[test]
fn the_way_back_writes_auto_and_restarts_without_asking() {
    let ini = "[core]\nui-size = 12\n[firmware]\ndual-boot = off\n";
    let mut device = Device::new(MemoryStorage::new().with(INI, ini), true);
    device.run(OPEN);
    // After the check, the way back is the second button.
    let shown = device.shown();
    assert!(shown.contains("Detect dual boot again"), "{shown}");
    device.run(actions::DOWN);
    device.run(actions::CONFIRM);
    assert!(device.restarted);
    assert_eq!(
        device.file(INI).as_deref(),
        Some("[core]\nui-size = 12\n[firmware]\ndual-boot = auto\n")
    );
}

#[test]
fn the_notice_is_in_german_too() {
    let mut device = Device::new(dual(BEFORE), true);
    device
        .kernel
        .set_setting(Instant(5), "core.language", "de")
        .unwrap();
    device.run(OPEN);
    let shown = device.shown();
    assert!(shown.contains("Kein Dual-Boot-Gerät"), "{shown}");
    device.run(actions::CONFIRM);
    let shown = device.shown();
    assert!(shown.contains("überschreiben"), "{shown}");
    assert!(
        shown.contains("Ja, neu starten") && shown.contains("Abbrechen"),
        "{shown}"
    );
}
