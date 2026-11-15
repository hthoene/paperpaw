//! The kernel's own screens, as they look today (#244, safety net P1): what
//! the kernel draws where no plugin's screen is, compared byte for byte with a
//! golden before the interface-6 code is taken out of the kernel. A golden
//! holds the screen's id, each part it describes (role, label, state and the
//! place on the screen) and a digest of the whole frame as the kernel draws it
//! (FNV-1a over the pixel bytes): one pixel moved makes it differ. There is no
//! tolerance. The profiles are the emulator's `xteink-x4` (buttons, black and
//! white) and `boox-go-7` (touch and buttons, grays).
//!
//! After a change that is meant: `PAPERPAW_BLESS=1` writes the goldens anew
//! (`tests/kernel-screens-244/<profile>-<screen>.txt`).
//!
//! Not here, and why: the confirmation of a firmware install
//! (`core.firmware-confirm`) is opened only by the kernel's firmware machine,
//! which needs a firmware host; a screen of a plugin that went away is not
//! drawn (the kernel's `Gone` holds its place); `Lab` and `VoiceSettings` live
//! in the shells.

#![cfg(all(
    feature = "native-linked",
    feature = "native-linked-reading",
    feature = "native-linked-store"
))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod native_activation;

use std::fmt::Write as _;
use std::path::PathBuf;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::builtin::{params, screens};
use paperpaw_sdk::{Id, MemoryStorage, Params};

/// The emulator's `xteink-x4`.
const X4: DeviceInfo = DeviceInfo::new(
    "xteink-x4",
    "Xteink X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg90, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

/// The emulator's `boox-go-7`.
const BOOX: DeviceInfo = DeviceInfo::new(
    "boox-go-7",
    "Boox Go 7",
    DisplayInfo::new(1264, 1680, 300, PixelFormat::Gray8, Rotation::Deg0, None),
    InputCaps::new(true, true, false, &[]),
    MemoryClass::Large,
);

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

struct App {
    kernel: Kernel,
    now: u64,
    profile: &'static str,
}

impl App {
    /// The standard app on `device`, past its first-run setup, the hints off.
    fn on(device: DeviceInfo) -> Self {
        let profile = device.id;
        let storage = MemoryStorage::new()
            .with(
                "books/alice-in-wonderland.txt",
                include_str!("../../../samples/library/books/alice-in-wonderland.txt"),
            )
            .with(".paperpaw/plugins/setup/done", "");
        let (builder, card) =
            native_activation::quiet_on(device, storage, "[core]\nlanguage = en\n");
        let mut kernel = native_activation::build_on(paperpaw_app::install(builder, fonts()), card);
        let effects = kernel.start(Instant(0));
        let mut app = Self {
            kernel,
            now: 0,
            profile,
        };
        app.settle(effects);
        app
    }

    /// Lets timers run until nothing is due within a second.
    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else {
                return;
            };
            if at.0 > self.now + 1000 {
                return;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
    }

    fn open(&mut self, screen: Id, params: Params) {
        self.now += 100;
        let effects = self.kernel.open_screen(Instant(self.now), screen, params);
        self.settle(effects);
    }

    /// The screen: its id, each part it describes with its place, and a digest
    /// of the whole frame.
    fn shown(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        let mut text = String::new();
        let screen = self.kernel.active_screen().map_or("", |id| id.as_str());
        writeln!(text, "screen {screen}").unwrap();
        for part in self.kernel.describe() {
            let mut state = String::new();
            if part.focused {
                state.push_str(" (focused)");
            }
            match part.on {
                Some(true) => state.push_str(" (on)"),
                Some(false) => state.push_str(" (off)"),
                None => {}
            }
            let a = part.area;
            writeln!(
                text,
                "{:?} \"{}\"{state} at {},{} {}x{}",
                part.role,
                part.label.replace('\n', " / "),
                a.x,
                a.y,
                a.width,
                a.height
            )
            .unwrap();
        }
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        let mut rows = 0_u32;
        let drawn = self.kernel.capture(&mut |top, view| {
            rows += u32::from(view.height);
            for row in 0..usize::from(view.height) {
                let from = row * view.stride;
                let bytes = view.data.get(from..from + view.stride).unwrap_or_default();
                for byte in std::iter::once(&((top as usize + row) as u8)).chain(bytes) {
                    hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
        });
        assert!(drawn, "changes waited to be drawn");
        writeln!(text, "frame {rows} rows, fnv1a64 {hash:016x}").unwrap();
        text
    }

    /// Compares the screen with `tests/kernel-screens-244/{profile}-{name}.txt`.
    fn check(&mut self, name: &str) {
        let shown = self.shown();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/kernel-screens-244")
            .join(format!("{}-{name}.txt", self.profile));
        if std::env::var_os("PAPERPAW_BLESS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &shown).unwrap();
            return;
        }
        let kept = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("{} missing: PAPERPAW_BLESS=1 makes it", path.display()))
            .replace("\r\n", "\n");
        assert_eq!(
            shown,
            kept,
            "{} shows something else now (PAPERPAW_BLESS=1 if meant)",
            path.display()
        );
    }
}

fn notice(device: DeviceInfo) {
    let mut app = App::on(device);
    let text = "The card is nearly full: 12 MB left";
    app.open(
        Id::new_static("core.notice-box"),
        Params::new().with(params::TEXT, text),
    );
    app.check("notice");
}

fn confirmation(device: DeviceInfo) {
    let mut app = App::on(device);
    let asked = Params::new()
        .with(params::TITLE, "Delete this book?")
        .with(params::TEXT, "It is taken off the card.");
    app.open(screens::CONFIRM, asked);
    app.check("confirmation");
}

fn choice(device: DeviceInfo) {
    let mut app = App::on(device);
    let asked = Params::new()
        .with(params::TITLE, "Sort by")
        .with("options", "Title,Author,Recently read,Size");
    app.open(screens::CHOICE, asked);
    app.check("choice");
}

fn folder_editor(device: DeviceInfo) {
    let mut app = App::on(device);
    app.open(
        screens::FOLDER_EDITOR,
        Params::new().with(params::PATH, "books"),
    );
    app.check("folder-editor");
}

fn placeholder(device: DeviceInfo) {
    let mut app = App::on(device);
    app.open(screens::PLACEHOLDER, Params::new());
    app.check("placeholder");
}

fn finishing(device: DeviceInfo) {
    let mut app = App::on(device);
    let trial = Params::new().with("from", "1000").with("until", "31000");
    app.open(screens::FINISHING_UPDATE, trial);
    app.check("finishing");
}

macro_rules! on_both {
    ($($name:ident),*) => {$(
        mod $name {
            #[test]
            fn xteink_x4() {
                super::$name(super::X4);
            }
            #[test]
            fn boox_go_7() {
                super::$name(super::BOOX);
            }
        }
    )*};
}

on_both!(
    notice,
    confirmation,
    choice,
    folder_editor,
    placeholder,
    finishing
);
