//! Golden pictures of the Battery view in the states it can be in on the
//! host: the picture the interface-7 port has to draw again, pixel for pixel
//! (`PORTING.md` lists them). Each state is drawn through the public kernel
//! on a 480×800 gray device with the default bitmap fonts, as the view is
//! opened from its action, and compared with `testdata/screens/<name>.png`.
//!
//! After a change meant to alter the view, write the pictures anew and look at
//! each before committing it:
//!
//! ```sh
//! PAPERPAW_BLESS=1 cargo test -p paperpaw-plugin-battery --test screens
//! ```
//!
//! A failing test leaves what it drew beside the golden as `<name>.actual.png`
//! in the system's temporary folder (the path is in the message).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    Battery as Charge, DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat,
    Rotation, SystemStatus,
};
use paperpaw_plugin_battery::{Battery, OPEN};
use paperpaw_sdk::{Env, Id, MemoryStorage};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "battery-screens",
    "Battery screen goldens",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &["page-forward", "page-back", "back"]),
    MemoryClass::Small,
);
const HEADER: &str =
    "kind,ms,millivolts,percent,charging,fast,partial,full,kilopixels,inputs,panel_ms,pages,unix\n";
const NEXT_PAGE: Id = paperpaw_sdk::builtin::actions::NEXT_PAGE;
const PREV_PAGE: Id = paperpaw_sdk::builtin::actions::PREV_PAGE;

struct Device {
    kernel: Kernel,
    now: u64,
}

impl Device {
    fn new(language: &str, developer: bool, storage: MemoryStorage) -> Self {
        let kernel = Kernel::builder(DEVICE)
            .storage(storage)
            // Logging off: the history on the card stays exactly as seeded.
            .user_config(format!(
                "[core]\nlanguage = {language}\ndeveloper = {}\n[battery]\nenabled = off\n",
                if developer { "on" } else { "off" }
            ))
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Battery)
            .build()
            .unwrap();
        let mut device = Self { kernel, now: 0 };
        let effects = device.kernel.start(Instant(0));
        device.settle(effects);
        device
    }

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
        panic!("kernel did not settle");
    }

    fn status(&mut self, battery: Option<Charge>) {
        self.now += 100;
        let effects = self
            .kernel
            .set_status(Instant(self.now), SystemStatus::new(None, battery));
        self.settle(effects);
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    /// The whole screen as gray values, row by row.
    fn picture(&mut self) -> Picture {
        let mut gray = vec![0xff_u8; 480 * 800];
        self.kernel.render(&mut |top, frame| {
            for y in 0..frame.height {
                for x in 0..frame.width {
                    let at = (usize::from(top + y)) * 480 + usize::from(x);
                    if let Some(out) = gray.get_mut(at) {
                        *out = frame.pixel(x, y).luma();
                    }
                }
            }
        });
        Picture { gray }
    }
}

struct Picture {
    gray: Vec<u8>,
}

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/screens")
}

fn write_png(path: &Path, picture: &Picture) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = BufWriter::new(File::create(path).unwrap());
    let mut encoder = png::Encoder::new(file, 480, 800);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&picture.gray)
        .unwrap();
}

fn read_png(path: &Path) -> Vec<u8> {
    let decoder = png::Decoder::new(BufReader::new(File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buffer).unwrap();
    assert_eq!((info.width, info.height), (480, 800), "{}", path.display());
    buffer.truncate(info.buffer_size());
    buffer
}

/// Compares `picture` with the golden `name`, or writes it where
/// `PAPERPAW_BLESS` is set.
fn golden(name: &str, picture: &Picture) {
    let path = dir().join(format!("{name}.png"));
    if std::env::var_os("PAPERPAW_BLESS").is_some() {
        write_png(&path, picture);
    }
    assert!(
        path.is_file(),
        "no golden {}: write it with PAPERPAW_BLESS=1 and look at it",
        path.display()
    );
    if read_png(&path) != picture.gray {
        let actual = std::env::temp_dir().join(format!("battery-{name}.actual.png"));
        write_png(&actual, picture);
        panic!(
            "{name}: the picture differs from {}; it is at {}",
            path.display(),
            actual.display()
        );
    }
}

fn card(log: &str) -> MemoryStorage {
    MemoryStorage::new().with(&format!("{}/log.csv", Env::plugin_dir("battery")), log)
}

/// Two completed hours, five percent each, with one then two pages.
fn few() -> String {
    format!(
        "{HEADER}start,0,4100,90,0,0,0,0,0,0,0,0,\n\
         end,3600000,4000,85,0,0,0,0,0,0,0,1,\n\
         start,4000000,4000,85,0,0,0,0,0,0,0,0,\n\
         end,7600000,3900,80,0,0,0,0,0,0,0,2,\n"
    )
}

/// Seventy completed times awake of different lengths, so that the list
/// takes several pages.
fn many() -> String {
    let mut log = String::from(HEADER);
    for i in 0..70_u64 {
        let start = i * 4 * 3_600_000;
        let hours = 1 + i % 5;
        let from = 100 - (i % 40) as u8;
        let to = from.saturating_sub(hours as u8 * 3);
        writeln!(log, "start,{start},4100,{from},0,0,0,0,0,0,0,0,").unwrap();
        writeln!(
            log,
            "end,{},3900,{to},0,0,0,0,0,0,0,{},",
            start + hours * 3_600_000,
            hours * 40
        )
        .unwrap();
    }
    log
}

fn opened(language: &str, developer: bool, log: Option<&str>, now: Option<Charge>) -> Device {
    let storage = log.map_or_else(MemoryStorage::new, card);
    let mut device = Device::new(language, developer, storage);
    device.status(now);
    device.run(OPEN);
    device
}

#[test]
fn empty_log_with_the_battery_unknown_in_both_languages() {
    for language in ["en", "de"] {
        let mut device = opened(language, false, None, None);
        golden(&format!("empty-unknown-{language}"), &device.picture());
    }
}

#[test]
fn empty_log_with_the_battery_known_in_both_languages() {
    for language in ["en", "de"] {
        let mut device = opened(
            language,
            false,
            None,
            Some(Charge::new(73, false, Some(4012))),
        );
        golden(&format!("empty-known-{language}"), &device.picture());
    }
}

#[test]
fn a_few_times_awake_in_both_languages() {
    for language in ["en", "de"] {
        let mut device = opened(
            language,
            false,
            Some(&few()),
            Some(Charge::new(73, false, Some(4012))),
        );
        golden(&format!("few-{language}"), &device.picture());
    }
}

#[test]
fn charging_and_a_low_battery() {
    for (name, charge) in [
        ("charging", Charge::new(73, true, Some(4012))),
        ("charging-no-voltage", Charge::new(41, true, None)),
        ("low", Charge::new(5, false, Some(3300))),
        ("empty", Charge::new(0, false, None)),
    ] {
        let mut device = opened("en", false, Some(&few()), Some(charge));
        golden(name, &device.picture());
    }
    let mut device = opened(
        "de",
        false,
        Some(&few()),
        Some(Charge::new(73, true, Some(4012))),
    );
    golden("charging-de", &device.picture());
}

#[test]
fn many_times_awake_page_by_page_and_back() {
    let mut device = opened(
        "en",
        false,
        Some(&many()),
        Some(Charge::new(64, false, None)),
    );
    golden("many-page-1", &device.picture());
    device.run(NEXT_PAGE);
    golden("many-page-2", &device.picture());
    // Past the last page the view stays on it.
    for _ in 0..8 {
        device.run(NEXT_PAGE);
    }
    golden("many-last-page", &device.picture());
    for _ in 0..12 {
        device.run(PREV_PAGE);
    }
    golden("many-page-1-again", &device.picture());
}

#[test]
fn a_developer_sees_where_the_log_is() {
    let mut device = opened(
        "en",
        true,
        Some(&few()),
        Some(Charge::new(73, false, Some(4012))),
    );
    golden("developer-footer", &device.picture());
}
