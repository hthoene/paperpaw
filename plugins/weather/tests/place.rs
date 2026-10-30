//! Choosing the weather's place with the real kernel, by buttons and by
//! touch: the search sends only the name typed, the places found are
//! listed and one is picked, hostile and failed answers are said, *No
//! place* stops the weather; a weather kept is fetched again only when
//! due or asked for.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::{Effects, HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::{
    InputEvent, Point, PointerEvent, PointerPhase, PointerTool, Rotation, SystemStatus, WallClock,
};
use paperpaw_plugin_weather::{CHOOSE_PLACE, REFRESH, Weather};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat, Storage, StorageError,
};

const fn device(touch: bool) -> DeviceInfo {
    DeviceInfo::new(
        "test",
        "Test device",
        DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
        InputCaps::new(!touch, touch, false, &[]),
        MemoryClass::Large,
    )
}

/// A storage the test looks into while the kernel has it.
#[derive(Clone, Default)]
struct Shared(Rc<MemoryStorage>);

impl Storage for Shared {
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
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
}

const KEPT: &str = ".paperpaw/plugins/weather/weather.txt";
const PLACE: &str = "[weather]\nplace = Berlin\ncoordinates = 52.52,13.41\n";
const SEARCH: &str = include_str!("../testdata/search.json");

struct Device {
    kernel: Kernel,
    storage: Shared,
    now: u64,
}

impl Device {
    fn new(config: &str, touch: bool, network: bool) -> Self {
        let storage = Shared::default();
        let mut kernel = Kernel::builder(device(touch))
            .storage(storage.clone())
            .user_config(format!("[core]\nlanguage = en\n{config}"))
            .network(network)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Weather)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let mut device = Self {
            kernel,
            storage,
            now: 0,
        };
        device.calls();
        device
    }

    fn later(&mut self) -> Instant {
        self.now += 100;
        Instant(self.now)
    }

    fn calls(&mut self) -> Vec<HttpCall> {
        self.kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Http(call) => Some(call),
                _ => None,
            })
            .collect()
    }

    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..100 {
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

    fn run(&mut self, action: Id) -> Vec<HttpCall> {
        let now = self.later();
        let effects = self.kernel.run_action(now, action);
        self.settle(effects);
        self.calls()
    }

    /// The name typed on the keyboard.
    fn typed(&mut self, name: &str) -> Vec<HttpCall> {
        let now = self.later();
        self.kernel.send_event(
            now,
            Id::new_static("weather.typed"),
            Params::new().with("text", name),
        );
        self.calls()
    }

    fn answer(&mut self, call: &HttpCall, status: u16, body: &[u8]) -> Vec<HttpCall> {
        let now = self.later();
        self.kernel.http_response(
            now,
            HttpResponse {
                id: call.id,
                status,
                body: body.to_vec(),
                ..HttpResponse::default()
            },
        );
        self.calls()
    }

    fn parts(&mut self) -> Vec<paperpaw_sdk::semantics::Part> {
        self.kernel.render(&mut |_, _| {});
        self.kernel.describe()
    }

    fn shown(&mut self) -> String {
        let labels: Vec<String> = self.parts().into_iter().map(|p| p.label).collect();
        labels.join(" | ")
    }

    fn screen(&self) -> Option<&str> {
        self.kernel.active_screen().map(Id::as_str)
    }

    fn setting(&self, id: &str) -> String {
        self.kernel.settings().text(id).to_string()
    }

    fn tap_on(&mut self, label: &str) -> Vec<HttpCall> {
        let parts = self.parts();
        let area = paperpaw_sdk::semantics::find(&parts, label)
            .unwrap_or_else(|| panic!("nothing says {label}: {parts:?}"))
            .area;
        let at = Point::new(
            i32::from(area.x) + i32::from(area.width) / 2,
            i32::from(area.y) + i32::from(area.height) / 2,
        );
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            let now = self.later();
            let effects = self.kernel.handle(
                now,
                InputEvent::Pointer(PointerEvent::new(0, phase, at, PointerTool::Finger, 1000)),
            );
            if phase == PointerPhase::Up {
                self.settle(effects);
            }
        }
        self.calls()
    }
}

#[test]
fn a_place_found_by_name_is_picked_and_its_weather_fetched() {
    let mut d = Device::new("", false, true);
    d.run(CHOOSE_PLACE);
    assert_eq!(d.screen(), Some("weather.place-screen"));
    let calls = d.typed("  Berlin & Co ");
    assert_eq!(calls.len(), 1);
    let url = &calls[0].url;
    assert_eq!(
        url,
        "https://geocoding-api.open-meteo.com/v1/search?name=Berlin+%26+Co&count=10&language=en&format=json"
    );
    assert!(d.shown().contains("Searching"));
    d.answer(&calls[0], 200, SEARCH.as_bytes());
    let shown = d.shown();
    assert!(shown.contains("3 places found"), "{shown}");
    assert!(shown.contains("Land Berlin, Deutschland"), "{shown}");
    // The focus is on the first place found: confirm picks it.
    let calls = d.run(actions::CONFIRM);
    assert_eq!(d.setting("weather.place"), "Berlin");
    assert_eq!(d.setting("weather.coordinates"), "52.52,13.41");
    assert_ne!(d.screen(), Some("weather.place-screen"));
    // Only the coordinates go out for the weather.
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(calls[0].url.contains("latitude=52.52&longitude=13.41"));
    assert!(!calls[0].url.contains("Berlin"));
}

#[test]
fn searches_that_find_nothing_or_fail_are_said() {
    let cases: [(u16, &[u8], &str); 6] = [
        (
            200,
            br#"{"generationtime_ms": 0.3}"#,
            "Nothing found for “Atlantis”.",
        ),
        (200, b"", "Nothing found for “Atlantis”."),
        (
            200,
            b"<html>not json</html>",
            "Nothing found for “Atlantis”.",
        ),
        (
            200,
            br#"{"results": [{"name": "Bad", "latitude": 200, "longitude": 0}]}"#,
            "Nothing found for “Atlantis”.",
        ),
        (500, b"{}", "The search did not work."),
        (0, b"no network", "The search did not work."),
    ];
    for (status, body, said) in cases {
        let mut d = Device::new(PLACE, false, true);
        d.run(CHOOSE_PLACE);
        let calls = d.typed("Atlantis");
        d.answer(&calls[0], status, body);
        let shown = d.shown();
        assert!(shown.contains(said), "{status}: {shown}");
        // The place set stays.
        assert_eq!(d.setting("weather.coordinates"), "52.52,13.41");
        assert_eq!(d.screen(), Some("weather.place-screen"));
    }
}

#[test]
fn without_a_network_nothing_is_sent() {
    let mut d = Device::new("", false, false);
    d.run(CHOOSE_PLACE);
    assert!(d.typed("Berlin").is_empty());
    assert!(d.shown().contains("Searching needs a network."));
    // An empty name searches nothing.
    let mut d = Device::new("", false, true);
    d.run(CHOOSE_PLACE);
    assert!(d.typed("   ").is_empty());
    assert!(d.shown().contains("Type its name"));
}

#[test]
fn no_place_stops_the_weather_and_forgets_it() {
    let mut d = Device::new(PLACE, true, true);
    d.storage
        .write(KEPT, include_bytes!("../testdata/weather.txt.v1"))
        .unwrap();
    d.calls();
    d.run(CHOOSE_PLACE);
    let shown = d.shown();
    assert!(shown.contains("Now: Berlin"), "{shown}");
    assert!(d.tap_on("No place").is_empty());
    assert_eq!(d.setting("weather.place"), "");
    assert_eq!(d.setting("weather.coordinates"), "");
    assert!(!d.storage.exists(KEPT));
    // Asked for the weather now: the place is chosen first.
    assert!(d.run(REFRESH).is_empty());
    assert_eq!(d.screen(), Some("weather.place-screen"));
}

#[test]
fn by_touch_a_place_found_is_picked() {
    let mut d = Device::new("", true, true);
    d.run(CHOOSE_PLACE);
    let calls = d.typed("Berlin");
    d.answer(&calls[0], 200, SEARCH.as_bytes());
    let parts = d.parts();
    // The second Berlin, in Schleswig-Holstein.
    let second = parts
        .iter()
        .filter(|p| p.label.starts_with("Berlin"))
        .nth(1)
        .unwrap()
        .label
        .clone();
    d.tap_on(&second);
    assert_eq!(d.setting("weather.place"), "Berlin");
    assert_eq!(d.setting("weather.coordinates"), "54.03,10.44");
}

#[test]
fn many_places_found_are_capped_and_scroll() {
    let place = |i: usize| {
        format!(r#"{{"name": "Place {i}", "latitude": {i}.5, "longitude": 1, "country": "X"}}"#)
    };
    let body = format!(
        r#"{{"results": [{}]}}"#,
        (0..25).map(place).collect::<Vec<_>>().join(",")
    );
    let mut d = Device::new("", false, true);
    d.run(CHOOSE_PLACE);
    let calls = d.typed("Place");
    d.answer(&calls[0], 200, body.as_bytes());
    assert!(d.shown().contains("10 places found"));
    // Down to the last place found: picked.
    for _ in 0..9 {
        d.run(actions::DOWN);
    }
    d.run(actions::CONFIRM);
    assert_eq!(d.setting("weather.place"), "Place 9");
    assert_eq!(d.setting("weather.coordinates"), "9.5,1");
}

/// A device with the weather kept (fetched 2026-10-14 10:30 UTC, days
/// from 2026-10-14) and the clock `minutes` later in a zone `offset`
/// minutes from UTC.
fn kept_at(offset: i32, minutes: i64) -> Device {
    let mut d = Device::new(PLACE, false, true);
    d.storage
        .write(KEPT, include_bytes!("../testdata/weather.txt.v1"))
        .unwrap();
    let clock = WallClock::new(
        Instant(0),
        (1_791_973_800 + minutes * 60) * 1000,
        Some(offset),
    );
    let now = d.later();
    d.kernel
        .set_status(now, SystemStatus::new(Some(clock), None));
    d
}

#[test]
fn a_kept_weather_is_fetched_again_only_when_due() {
    // Fresh: nothing goes out at a start or on waking.
    let mut d = kept_at(120, 10);
    let now = d.later();
    d.kernel.resume(now);
    assert!(d.calls().is_empty());
    // A tap on a widget (the action) fetches it at once.
    let calls = d.run(REFRESH);
    assert_eq!(calls.len(), 1);
    // Asked again while waiting for the answer: not twice.
    assert!(d.run(REFRESH).is_empty());
    // Hostile answers keep what was kept.
    d.answer(
        &calls[0],
        200,
        b"{\"current\": {\"temperature_2m\": \"hot\"}}",
    );
    let kept = String::from_utf8(d.storage.0.get(KEPT).unwrap()).unwrap();
    assert!(kept.contains("now 124 "), "{kept}");
}
