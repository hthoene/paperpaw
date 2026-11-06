//! Observable Battery view contracts to preserve when its SDK calls move.
//! These fixtures use the current public kernel and real bitmap fonts.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when a view contract is broken"
)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    Battery as Charge, ButtonState, Color, DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant,
    MemoryClass, PixelFormat, Rotation, SystemStatus,
};
use paperpaw_plugin_battery::{Battery, OPEN};
use paperpaw_sdk::semantics::{Described, Role};
use paperpaw_sdk::{Env, Id, MemoryStorage, Params};

const SCREEN: Id = Id::new_static("battery.screen");
const DEVICE: DeviceInfo = DeviceInfo::new(
    "battery-contract",
    "Battery contract device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &["page-forward", "page-back", "back"]),
    MemoryClass::Small,
);
const HEADER: &str =
    "kind,ms,millivolts,percent,charging,fast,partial,full,kilopixels,inputs,panel_ms,pages,unix\n";

struct Device {
    kernel: Kernel,
    now: u64,
}

impl Device {
    fn new(language: &str, storage: MemoryStorage) -> Self {
        let kernel = Kernel::builder(DEVICE)
            .storage(storage)
            // Existing history stays fixed while the view samples current status.
            .user_config(format!(
                "[core]\nlanguage = {language}\n[battery]\nenabled = off\n"
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

    fn labels(&mut self) -> Vec<String> {
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect()
    }

    fn every_page(&mut self) -> Vec<String> {
        let mut current = self.labels();
        let mut all = current.clone();
        for _ in 0..16 {
            self.run(paperpaw_sdk::builtin::actions::NEXT_PAGE);
            let next = self.labels();
            if next == current {
                return all;
            }
            all.extend(next.iter().cloned());
            current = next;
        }
        panic!("fixture unexpectedly needs more than sixteen pages");
    }

    fn press(&mut self, name: &'static str) {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            self.now += 50;
            let effects = self
                .kernel
                .handle(Instant(self.now), InputEvent::Button { name, state });
            self.settle(effects);
        }
    }
}

fn contains(labels: &[String], text: &str) -> bool {
    labels.iter().any(|label| label == text)
}

fn text(device: &mut Device, label: &str) -> Described {
    device
        .kernel
        .described_active()
        .into_iter()
        .find(|part| part.role == Role::Text && part.label == label)
        .unwrap_or_else(|| panic!("no drawn text says {label:?}"))
}

#[test]
fn zero_charge_is_known_even_without_voltage_in_both_languages() {
    for (language, charge, power, source, unknown) in [
        (
            "en",
            "Charge",
            "Power",
            "On battery",
            "This device does not say how full its battery is.",
        ),
        (
            "de",
            "Ladung",
            "Strom",
            "Akku",
            "Dieses Gerät sagt nicht, wie voll sein Akku ist.",
        ),
    ] {
        let mut missing = Device::new(language, MemoryStorage::new());
        missing.run(OPEN);
        let missing = missing.labels();
        assert!(contains(&missing, unknown), "{language}: {missing:?}");
        assert!(!contains(&missing, charge), "{language}: {missing:?}");
        assert!(!contains(&missing, power), "{language}: {missing:?}");

        let mut zero = Device::new(language, MemoryStorage::new());
        zero.status(Some(Charge::new(0, false, None)));
        zero.run(OPEN);
        let labels = zero.labels();
        for expected in [charge, "0 %", power, source] {
            assert!(contains(&labels, expected), "{language}: {labels:?}");
        }
        assert!(!contains(&labels, unknown), "{language}: {labels:?}");
        assert!(
            !labels.iter().any(|label| label.contains(" V")),
            "{labels:?}"
        );
    }
}

#[test]
fn voltage_is_optional_and_charging_is_localized() {
    for (language, charging, voltage) in [
        ("en", "Charging", "73 % · 4.01 V"),
        ("de", "Lädt", "73 % · 4,01 V"),
    ] {
        for (millivolts, expected) in [(None, "73 %"), (Some(4012), voltage)] {
            let mut device = Device::new(language, MemoryStorage::new());
            device.status(Some(Charge::new(73, true, millivolts)));
            device.run(OPEN);
            let labels = device.labels();
            assert!(contains(&labels, expected), "{language}: {labels:?}");
            assert!(contains(&labels, charging), "{language}: {labels:?}");
            if millivolts.is_none() {
                assert!(
                    !labels.iter().any(|label| label.contains(" V")),
                    "{labels:?}"
                );
            }
        }
    }
}

/// Two completed hours, five percent each, with one then two pages.
fn history() -> MemoryStorage {
    let log = format!(
        "{HEADER}start,0,4100,90,0,0,0,0,0,0,0,0,\n\
         end,3600000,4000,85,0,0,0,0,0,0,0,1,\n\
         start,4000000,4000,85,0,0,0,0,0,0,0,0,\n\
         end,7600000,3900,80,0,0,0,0,0,0,0,2,\n"
    );
    MemoryStorage::new().with(&format!("{}/log.csv", Env::plugin_dir("battery")), log)
}

#[test]
fn history_localizes_rate_decimal_duration_and_page_plural() {
    for (language, title, hourly, rate, remaining, one, two) in [
        (
            "en",
            "Battery",
            "An hour awake",
            "5.0 %",
            "about 16 h",
            "1 h 0 min · 1 page",
            "1 h 0 min · 2 pages",
        ),
        (
            "de",
            "Akku",
            "Eine Stunde wach",
            "5,0 %",
            "etwa 16 h wach",
            "1 h 0 min · 1 Seite",
            "1 h 0 min · 2 Seiten",
        ),
    ] {
        let mut device = Device::new(language, history());
        device.status(Some(Charge::new(80, false, None)));
        device.run(OPEN);
        let labels = device.every_page();
        for expected in [title, hourly, rate, remaining, one, two] {
            assert!(
                contains(&labels, expected),
                "{language}: missing {expected:?} in {labels:?}"
            );
        }
        assert!(
            !labels.iter().any(|label| label.starts_with("battery.")),
            "{labels:?}"
        );
    }
}

#[test]
fn the_open_action_reuses_one_screen_even_with_different_screen_params() {
    let mut device = Device::new("en", MemoryStorage::new());
    let baseline: Vec<_> = device.kernel.screens().cloned().collect();
    let mut opened = baseline.clone();
    opened.push(SCREEN);
    device.run(OPEN);
    assert_eq!(device.kernel.active_screen(), Some(&SCREEN));
    assert_eq!(device.kernel.screens().cloned().collect::<Vec<_>>(), opened);

    // Different params distinguish SINGLE from the default instance policy.
    device.now += 100;
    let effects = device.kernel.open_screen(
        Instant(device.now),
        SCREEN,
        Params::new().with("fixture", "reopen"),
    );
    device.settle(effects);
    assert_eq!(device.kernel.screens().cloned().collect::<Vec<_>>(), opened);
    device.run(OPEN);
    assert_eq!(device.kernel.screens().cloned().collect::<Vec<_>>(), opened);
    device.press("back");
    assert_eq!(
        device.kernel.screens().cloned().collect::<Vec<_>>(),
        baseline,
        "one Back restores the previous stack"
    );
}

#[test]
fn charge_text_has_visible_glyph_pixels_inside_its_semantic_bounds() {
    let mut device = Device::new("en", MemoryStorage::new());
    device.status(Some(Charge::new(73, false, Some(4012))));
    device.run(OPEN);
    let part = text(&mut device, "73 % · 4.01 V");
    let area = part.area;
    assert!(area.width > 0 && area.height > 0);
    assert!(area.x >= part.clip.x && area.y >= part.clip.y);
    assert!(area.right() <= part.clip.right() && area.bottom() <= part.clip.bottom());
    assert!(area.right() <= u32::from(DEVICE.display.width));
    assert!(area.bottom() <= u32::from(DEVICE.display.height));

    let (mut white, mut ink) = (0, 0);
    device.kernel.render(&mut |top, frame| {
        for y in 0..frame.height {
            let screen_y = top + y;
            if screen_y < area.y || u32::from(screen_y) >= area.bottom() {
                continue;
            }
            for x in area.x..area.x + area.width {
                if frame.pixel(x, y) == Color::WHITE {
                    white += 1;
                } else {
                    ink += 1;
                }
            }
        }
    });
    assert!(ink > 0, "charge text must paint glyphs");
    assert!(white > 0, "charge text must not be a solid rectangle");
}
