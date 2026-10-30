//! Reading profiles with the real kernel: names stay unique, *Next
//! profile* goes round them all, and the profile applied last keeps
//! being it when renamed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::Kernel;
use paperpaw_platform::Rotation;
use paperpaw_plugin_profiles::{NEXT, OPEN, Profiles};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Large,
);

const FILE: &str = ".paperpaw/plugins/profiles/profiles.ini";
const LAST: &str = ".paperpaw/plugins/profiles/last.txt";

struct Test {
    kernel: Kernel,
    now: u64,
}

impl Test {
    fn new() -> Self {
        let mut kernel = Kernel::builder(DEVICE)
            .storage(MemoryStorage::new())
            .plugin(Profiles)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let mut test = Self { kernel, now: 0 };
        let at = test.at();
        test.kernel.run_action(at, OPEN);
        test
    }

    fn at(&mut self) -> Instant {
        self.now += 200;
        Instant(self.now)
    }

    /// The keyboard's answer to `event`, for the profile at `path`.
    fn typed(&mut self, event: &str, text: &str, path: &str) {
        let typed = Typed::new(
            text.to_string(),
            Params::new().with("path", path.to_string()),
        );
        let at = self.at();
        self.kernel
            .send_event(at, Id::parse(event).unwrap(), typed.to_params());
    }

    fn read(&mut self, path: &str) -> String {
        self.kernel.render(&mut |_, _| {});
        let data = paperpaw_sdk::read_file(self.kernel.storage(), path, 1 << 16).unwrap();
        String::from_utf8(data).unwrap()
    }

    fn names(&mut self) -> Vec<String> {
        self.read(FILE)
            .lines()
            .filter_map(|l| l.strip_prefix("name = "))
            .map(String::from)
            .collect()
    }
}

#[test]
fn names_stay_unique() {
    let mut test = Test::new();
    test.typed("profiles.named", "Day", "");
    test.typed("profiles.named", "Night", "");
    // The same name again, also in other letters: refused.
    test.typed("profiles.named", "night", "");
    test.typed("profiles.renamed", "DAY", "1");
    assert_eq!(test.names(), ["Day", "Night"]);
    // A new name, or its own in other letters: taken.
    test.typed("profiles.renamed", "Late", "1");
    test.typed("profiles.renamed", "DAY", "0");
    assert_eq!(test.names(), ["DAY", "Late"]);
}

#[test]
fn next_goes_round_and_follows_a_rename() {
    let mut test = Test::new();
    for name in ["Day", "Night", "Print"] {
        test.typed("profiles.named", name, "");
    }
    assert_eq!(test.read(LAST), "Print");
    let at = test.at();
    test.kernel.run_action(at, NEXT);
    assert_eq!(test.read(LAST), "Day", "round to the first");
    // Renamed while applied last: still the last one.
    test.typed("profiles.renamed", "Morning", "0");
    let mut applied = Vec::new();
    for _ in 0..3 {
        let at = test.at();
        test.kernel.run_action(at, NEXT);
        applied.push(test.read(LAST));
    }
    assert_eq!(applied, ["Night", "Print", "Morning"]);
}
