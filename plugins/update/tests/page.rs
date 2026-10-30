//! The update page with the real kernel: an update that leaves out a
//! plugin built in says which, an update named by the update server is
//! downloaded into `updates` and then offered like one copied there, and
//! the server's other answers are said.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::{FirmwareCommand, HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_plugin_update::{CHECK, OPEN, Update};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::firmware::{Firmware, PLUGINS_AT, PluginCrate, Plugins};
use paperpaw_sdk::semantics::Role;
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

const SEED: [u8; 32] = [3; 32];
const CHIP: u16 = paperpaw_sdk::firmware::CHIP_ESP32_C3;
const PROJECT: &str = "paperpaw-xteink";
/// What the running firmware has built in: a plugin of its own beside
/// PaperPaw's.
const RUNNING: &str = "paperpaw-plugin-stats 0.4.0\nchess-for-paperpaw 1.0.0\n";

/// A signed update file of `version` with the plugin crates `plugins`.
fn update_file(version: &str, plugins: &[(&str, &str)]) -> Vec<u8> {
    update_signed(version, plugins, &SEED)
}

/// An update file of `version` with `plugins`, signed with `seed`'s key.
fn update_signed(version: &str, plugins: &[(&str, &str)], seed: &[u8; 32]) -> Vec<u8> {
    let mut image = vec![0u8; 2000];
    image[0] = 0xE9;
    image[12..14].copy_from_slice(&CHIP.to_le_bytes());
    image[32..36].copy_from_slice(&0xABCD_5432u32.to_le_bytes());
    image[48..48 + version.len()].copy_from_slice(version.as_bytes());
    image[80..80 + PROJECT.len()].copy_from_slice(PROJECT.as_bytes());
    let list = Plugins::record(
        plugins
            .iter()
            .map(|&(name, version)| PluginCrate { name, version }),
    )
    .unwrap();
    image[PLUGINS_AT..PLUGINS_AT + list.len()].copy_from_slice(&list);
    let trailer = paperpaw_update::sign(&image, seed).unwrap();
    image.extend_from_slice(&trailer.to_bytes());
    image
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

struct Device {
    kernel: Kernel,
    storage: Shared,
    now: u64,
}

impl Device {
    /// Running PaperPaw 0.4.0 with [`RUNNING`], the card as `storage` has
    /// it.
    fn new(storage: Shared) -> Self {
        Self::at(storage, None)
    }

    /// As [`Device::new`], started at `unix` seconds since 1970 where the
    /// clock is known.
    fn at(storage: Shared, unix: Option<i64>) -> Self {
        let firmware = Firmware::new("0.4.0", PROJECT, CHIP)
            .keys(vec![paperpaw_update::public_key(&SEED)])
            .room(1_000_000)
            .plugins(Plugins::new(RUNNING));
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage.clone())
            .network(true)
            .firmware(firmware)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Update)
            .build()
            .unwrap();
        if let Some(unix) = unix {
            let clock = paperpaw_platform::WallClock::new(Instant(0), unix * 1000, None);
            let status = paperpaw_platform::SystemStatus::new(Some(clock), None);
            kernel.set_status(Instant(0), status);
        }
        kernel.start(Instant(0));
        Self {
            kernel,
            storage,
            now: 0,
        }
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        self.kernel.run_action(Instant(self.now), action);
    }

    /// The one request the kernel asks the shell to make now.
    fn request(&mut self) -> HttpCall {
        let mut calls: Vec<HttpCall> = self
            .kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Http(call) => Some(call),
                _ => None,
            })
            .collect();
        assert_eq!(calls.len(), 1, "{calls:?}");
        calls.remove(0)
    }

    fn answer(&mut self, call: &HttpCall, status: u16, body: &[u8]) {
        self.now += 100;
        let response = HttpResponse {
            id: call.id,
            status,
            body: body.to_vec(),
            ..HttpResponse::default()
        };
        self.kernel.http_response(Instant(self.now), response);
    }

    /// No request asked of the shell now.
    fn no_request(&mut self) {
        let calls: Vec<HttpCall> = self
            .kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Http(call) => Some(call),
                _ => None,
            })
            .collect();
        assert!(calls.is_empty(), "{calls:?}");
    }

    /// The one install the kernel asks the shell for now.
    fn install(&mut self) -> FirmwareCommand {
        let mut commands: Vec<FirmwareCommand> = self
            .kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Firmware(command) => Some(command),
                _ => None,
            })
            .collect();
        assert_eq!(commands.len(), 1, "{commands:?}");
        commands.remove(0)
    }

    /// What the screen shows, as one text.
    fn shown(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

/// The server's answer naming PaperPaw 0.5.0 without the reader's own
/// plugin, which is not in the plugin library.
fn offer(size: usize) -> String {
    format!(
        r#"{{"version": "0.5.0", "url": "/api/v1/updates/paperpaw-x4-0.5.0-update.bin",
            "size": {size}, "plugins": ["paperpaw-plugin-stats 0.5.0"],
            "missing": ["chess-for-paperpaw unknown"]}}"#
    )
}

/// The server's answer naming PaperPaw 0.5.0, which waits for a code
/// plugin of this device with no build for it yet and for the reader's
/// own plugin built in, not built for it yet.
fn offer_waiting(size: usize) -> String {
    format!(
        r#"{{"version": "0.5.0", "url": "/api/v1/updates/paperpaw-x4-0.5.0-update.bin",
            "size": {size}, "plugins": ["paperpaw-plugin-stats 0.5.0"],
            "missing": ["chess-for-paperpaw not-ready"], "waiting": ["dict-xl 0.3.1"]}}"#
    )
}

const WAITING: &str = ".paperpaw/plugins/update/waiting.txt";
const OFFER: &str = ".paperpaw/plugins/update/offer.txt";
/// The answer of [`offer_waiting`] as the plugin keeps it.
const OFFER_WAITING: &str = "0.5.0\n/api/v1/updates/paperpaw-x4-0.5.0-update.bin\n5000\n\
     plugin paperpaw-plugin-stats 0.5.0\nmissing chess-for-paperpaw not-ready\n\
     waiting dict-xl 0.3.1\n";

#[test]
fn an_update_waiting_for_plugins_says_which_and_waits() {
    let mut device = Device::new(Shared::default());
    device.run(OPEN);
    device.run(actions::CONFIRM);
    let call = device.request();
    device.answer(&call, 200, offer_waiting(5000).as_bytes());
    let shown = device.shown();
    assert!(
        shown.contains("PaperPaw 0.5.0 waits for plugins"),
        "{shown}"
    );
    // The code plugin named is not installed here: only the crate holds.
    assert!(shown.contains("yet: chess-for-paperpaw."), "{shown}");
    assert!(!shown.contains("dict-xl"), "{shown}");
    assert!(shown.contains("Keep waiting"), "{shown}");
    assert!(shown.contains("Update anyway"), "{shown}");
    assert!(shown.contains("Check again"), "{shown}");
    // Looked at: it does not open by itself later.
    let waiting = device.storage.0.get(WAITING).unwrap();
    assert!(String::from_utf8_lossy(&waiting).contains("seen"));
    // Updated anyway: the offer first, saying what is gone after it.
    device.run(actions::DOWN);
    device.run(actions::CONFIRM);
    let shown = device.shown();
    assert!(shown.contains("PaperPaw 0.5.0 is available"), "{shown}");
    assert!(
        shown.contains("gone after the update until an update has them again"),
        "{shown}"
    );
    // *Not now* first; downloading is chosen on purpose.
    assert!(shown.contains("Download anyway"), "{shown}");
    device.run(actions::DOWN);
    device.run(actions::CONFIRM);
    let call = device.request();
    assert_eq!(call.method, "GET");
    assert!(device.shown().contains("Downloading PaperPaw 0.5.0"));
}

/// An answer still waiting kept without its record (from before
/// `waiting.txt` held what it waits for, or with it lost): a start makes
/// the record anew and asks.
#[test]
fn a_waiting_update_without_its_record_is_kept_and_asked_for() {
    let storage = Shared(Rc::new(MemoryStorage::new().with(OFFER, OFFER_WAITING)));
    let start = 1_760_000_000;
    let mut device = Device::at(storage.clone(), Some(start));
    let call = device.request();
    assert_eq!(call.method, "POST");
    let waiting = String::from_utf8(device.storage.0.get(WAITING).unwrap()).unwrap();
    assert!(waiting.contains("crate chess-for-paperpaw"), "{waiting}");
}

/// Without an answer (no network), the question is not asked again at
/// every wake: once a day.
#[test]
fn a_question_without_an_answer_counts() {
    let start = 1_760_000_000;
    let storage = Shared(Rc::new(
        MemoryStorage::new().with(OFFER, OFFER_WAITING).with(
            WAITING,
            format!(
                "0.5.0\nsince {start}\nchecked {}\ncrate chess-for-paperpaw\n",
                start - 2 * 24 * 3600
            ),
        ),
    ));
    let mut device = Device::at(storage.clone(), Some(start));
    let call = device.request();
    device.answer(&call, 0, b"no network");
    let mut device = Device::at(storage.clone(), Some(start + 3600));
    device.no_request();
    // An error of the server alike.
    let mut device = Device::at(storage.clone(), Some(start + 25 * 3600));
    let call = device.request();
    device.answer(&call, 500, b"");
    let mut device = Device::at(storage.clone(), Some(start + 26 * 3600));
    device.no_request();
}

/// A record that cannot be read is made anew from the kept answer.
#[test]
fn a_damaged_record_is_made_anew() {
    let storage = Shared(Rc::new(
        MemoryStorage::new()
            .with(OFFER, OFFER_WAITING)
            .with(WAITING, &b"\xff\xfe not a record"[..]),
    ));
    let mut device = Device::at(storage.clone(), Some(1_760_000_000));
    device.request();
    let waiting = String::from_utf8(device.storage.0.get(WAITING).unwrap()).unwrap();
    assert!(waiting.starts_with("0.5.0\n"), "{waiting}");
}

/// Without a clock, a record made anew is kept: asked once, not at every
/// wake.
#[test]
fn without_a_clock_it_asks_once() {
    let storage = Shared(Rc::new(MemoryStorage::new().with(OFFER, OFFER_WAITING)));
    let mut device = Device::at(storage.clone(), None);
    device.request();
    let waiting = String::from_utf8(device.storage.0.get(WAITING).unwrap()).unwrap();
    assert!(waiting.contains("asked"), "{waiting}");
    let mut device = Device::at(storage.clone(), None);
    device.no_request();
}

/// Installed (or once installed) and the update named from the card: the
/// kept answer is no longer newer, nothing waits.
#[test]
fn an_update_installed_meanwhile_waits_no_more() {
    let storage = Shared(Rc::new(
        MemoryStorage::new().with(OFFER, OFFER_WAITING).with(
            WAITING,
            "0.5.0\nsince 1\nchecked 1\ncrate chess-for-paperpaw\n",
        ),
    ));
    let mut device = with_code_plugin(storage, "0.5.0", "");
    device.no_request();
    assert!(device.storage.0.get(WAITING).is_none());
    assert!(
        device.storage.0.get(OFFER).is_none(),
        "the answer let go too"
    );
}

/// A dictionary of one's own with code, built for the firmware running.
const DICT_INI: &str = "[plugin]\nid = dict-xl\nname = Dictionary XL\nversion = 0.3.1\n";

/// PaperPaw `version` loading code plugins, with the dictionary installed
/// and `config` read.
fn with_code_plugin(storage: Shared, version: &str, config: &str) -> Device {
    storage
        .0
        .write("plugins/dict-xl/plugin.ini", DICT_INI.as_bytes())
        .unwrap();
    storage
        .0
        .write("plugins/dict-xl/code.ppn", &code_file())
        .unwrap();
    let firmware = Firmware::new(version, PROJECT, CHIP)
        .keys(vec![paperpaw_update::public_key(&SEED)])
        .room(1_000_000)
        .plugins(Plugins::new(RUNNING));
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .network(true)
        .firmware(firmware)
        .user_config(config)
        .code_plugins(
            paperpaw_sdk::native::Native::new("code.ppn"),
            Box::new(NoCode),
        )
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(Update)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    Device {
        kernel,
        storage,
        now: 0,
    }
}

/// PaperPaw 0.5.0, for which the dictionary has no build yet.
fn offer_waiting_for_dict(size: usize) -> String {
    format!(
        r#"{{"version": "0.5.0", "url": "/api/v1/updates/paperpaw-x4-0.5.0-update.bin",
            "size": {size}, "plugins": ["paperpaw-plugin-stats 0.5.0", "chess-for-paperpaw 1.1.0"],
            "waiting": ["dict-xl 0.3.1"]}}"#
    )
}

#[test]
fn a_code_plugin_in_use_holds_and_updating_anyway_says_it_stops() {
    let mut device = with_code_plugin(Shared::default(), "0.4.0", "");
    device.run(OPEN);
    device.run(actions::CONFIRM);
    let call = device.request();
    assert!(
        String::from_utf8_lossy(call.body.as_deref().unwrap_or_default()).contains("dict-xl 0.3.1")
    );
    device.answer(&call, 200, offer_waiting_for_dict(5000).as_bytes());
    let shown = device.shown();
    assert!(shown.contains("waits for plugins"), "{shown}");
    assert!(shown.contains("yet: Dictionary XL."), "{shown}");
    // Anyway: the offer, *Not now* first, saying what stops working.
    device.run(actions::DOWN);
    device.run(actions::CONFIRM);
    let shown = device.shown();
    assert!(
        shown
            .contains("These stop working until there is a version of them for it: Dictionary XL."),
        "{shown}"
    );
    let not_now = shown.find("Not now").unwrap();
    let download = shown.find("Download anyway").unwrap();
    assert!(not_now < download, "{shown}");
    // A second press keeps it: the page closes, nothing downloads.
    device.run(actions::CONFIRM);
    device.no_request();
}

#[test]
fn a_code_plugin_switched_off_does_not_hold() {
    let mut device = with_code_plugin(Shared::default(), "0.4.0", "[plugins]\ndict-xl = off\n");
    device.run(OPEN);
    device.run(actions::CONFIRM);
    let call = device.request();
    device.answer(&call, 200, offer_waiting_for_dict(5000).as_bytes());
    let shown = device.shown();
    assert!(shown.contains("PaperPaw 0.5.0 is available"), "{shown}");
    assert!(!shown.contains("Dictionary XL"), "{shown}");
}

#[test]
fn a_waiting_update_found_in_the_background_opens_once_after_a_week() {
    let (start, day) = (1_760_000_000, 24 * 3600);
    // Found by a search in the background: kept, not offered.
    let storage = Shared(Rc::new(
        MemoryStorage::new().with(OFFER, OFFER_WAITING).with(
            WAITING,
            format!("0.5.0\nsince {start}\nchecked {start}\ncrate chess-for-paperpaw\n"),
        ),
    ));
    // Started again (on the X4 at each wake). Six days on: asked again by
    // itself (a day passed), still waiting, nothing shown.
    let mut device = Device::at(storage.clone(), Some(start + 6 * day));
    let call = device.request();
    device.answer(&call, 200, offer_waiting(5000).as_bytes());
    assert!(!device.shown().contains("waits for plugins"));
    // A week: the page, once.
    let mut device = Device::at(storage.clone(), Some(start + 7 * day));
    assert!(device.shown().contains("waits for plugins"));
    let mut device = Device::at(storage.clone(), Some(start + 8 * day));
    assert!(!device.shown().contains("waits for plugins"));
    // Asked again, the plugins ready: it waits no more, offered on the
    // page.
    let call = device.request();
    device.answer(&call, 200, offer(5000).as_bytes());
    assert!(device.storage.0.get(WAITING).is_none());
    device.run(OPEN);
    let shown = device.shown();
    assert!(shown.contains("PaperPaw 0.5.0 is available"), "{shown}");
}

#[test]
fn an_update_on_the_card_says_what_it_leaves_out() {
    let storage = Shared::default();
    storage
        .write(
            "updates/paperpaw-x4-0.5.0.bin",
            &update_file("0.5.0", &[("paperpaw-plugin-stats", "0.5.0")]),
        )
        .unwrap();
    let mut device = Device::new(storage);
    device.run(OPEN);
    let shown = device.shown();
    assert!(shown.contains("PaperPaw 0.5.0"), "{shown}");
    assert!(
        shown.contains("cannot come along: chess-for-paperpaw."),
        "{shown}"
    );
    assert!(shown.contains("Update without them"), "{shown}");
}

#[test]
fn an_update_with_every_plugin_says_nothing_of_them() {
    let storage = Shared::default();
    storage
        .write(
            "updates/paperpaw-x4-0.5.0.bin",
            &update_file(
                "0.5.0",
                &[
                    ("chess-for-paperpaw", "1.1.0"),
                    ("paperpaw-plugin-stats", "0.5.0"),
                ],
            ),
        )
        .unwrap();
    let mut device = Device::new(storage);
    device.run(OPEN);
    let shown = device.shown();
    assert!(!shown.contains("cannot come along"), "{shown}");
    assert!(shown.contains("Install"), "{shown}");
}

#[test]
fn an_update_signed_by_another_key_installs_with_the_key_shown() {
    let storage = Shared::default();
    let all = [
        ("chess-for-paperpaw", "1.0.0"),
        ("paperpaw-plugin-stats", "0.5.0"),
    ];
    let own = [5; 32];
    storage
        .write("updates/own.bin", &update_signed("0.5.0", &all, &own))
        .unwrap();
    let mut device = Device::new(storage);
    device.run(OPEN);
    let shown = device.shown();
    let key = paperpaw_update::public_key(&own);
    let fingerprint = paperpaw_sdk::firmware::fingerprint(&key).replace(' ', "\u{a0}");
    assert!(shown.contains("own build"), "{shown}");
    assert!(shown.contains(&fingerprint), "{shown}");
    assert!(shown.contains("Install with this key"), "{shown}");
    device.run(actions::CONFIRM);
    assert_eq!(
        device.install(),
        FirmwareCommand::install("updates/own.bin", Some(key), false)
    );
}

#[test]
fn an_older_update_says_so_and_installs_only_as_chosen() {
    let storage = Shared::default();
    let all = [
        ("chess-for-paperpaw", "1.0.0"),
        ("paperpaw-plugin-stats", "0.3.0"),
    ];
    storage
        .write("updates/old.bin", &update_file("0.3.0", &all))
        .unwrap();
    let mut device = Device::new(storage);
    device.run(OPEN);
    let shown = device.shown();
    assert!(
        shown.contains("PaperPaw 0.3.0, an older version"),
        "{shown}"
    );
    assert!(shown.contains("Install older version"), "{shown}");
    device.run(actions::CONFIRM);
    assert_eq!(
        device.install(),
        FirmwareCommand::install("updates/old.bin", None, true)
    );
}

#[test]
fn an_own_build_not_newer_says_all_of_it() {
    // The same version again, under another key: its key and that it is
    // not newer, said whole above two buttons.
    let storage = Shared::default();
    let all = [
        ("chess-for-paperpaw", "1.0.0"),
        ("paperpaw-plugin-stats", "0.4.0"),
    ];
    storage
        .write("updates/own.bin", &update_signed("0.4.0", &all, &[5; 32]))
        .unwrap();
    let mut device = Device::new(storage);
    device.run(OPEN);
    device.shown();
    let parts = device.kernel.describe();
    let buttons: Vec<_> = parts.iter().filter(|p| p.role == Role::Button).collect();
    assert_eq!(buttons.len(), 2, "{buttons:?}");
    // Its last line drawn ends it: nothing cut.
    let last = parts.iter().rfind(|p| p.role == Role::Text).unwrap();
    assert!(last.label.ends_with("PaperPaw 0.4.0."), "{parts:?}");
    assert!(!last.label.starts_with("This update"), "{parts:?}");
}

#[test]
fn the_words_keep_clear_of_the_buttons() {
    // The most there is to say (another key, older, a plugin left out)
    // in the largest text.
    let storage = Shared::default();
    let stats = [("paperpaw-plugin-stats", "0.3.0")];
    storage
        .write("updates/old.bin", &update_signed("0.3.0", &stats, &[5; 32]))
        .unwrap();
    let mut device = Device::new(storage);
    device
        .kernel
        .set_setting(Instant(1), "core.ui-size", "24")
        .unwrap();
    device.run(OPEN);
    device.shown();
    let parts = device.kernel.describe();
    let first_button = parts
        .iter()
        .filter(|p| p.role == Role::Button)
        .map(|p| p.area.y)
        .min()
        .unwrap();
    for text in parts.iter().filter(|p| p.role == Role::Text) {
        if text.area.y < first_button {
            let bottom = text.area.y + text.area.height;
            assert!(bottom <= first_button, "{:?} below {first_button}", text);
        }
    }
}

#[test]
fn an_update_from_the_server_is_downloaded_and_offered() {
    let mut device = Device::new(Shared::default());
    device.run(OPEN);
    assert!(device.shown().contains("Search online"));
    device.run(actions::CONFIRM);
    let call = device.request();
    assert_eq!(call.method, "POST");
    assert_eq!(call.url, "https://paperpaw.org/api/v1/updates");
    let question = String::from_utf8(call.body.clone().unwrap_or_default()).unwrap();
    assert!(question.contains(r#""version":"0.4.0""#), "{question}");
    // Asked by the reader, counted without the time: no `first`.
    assert!(question.ends_with(r#","trigger":"manual"}"#), "{question}");
    assert!(
        question.contains(r#""chess-for-paperpaw 1.0.0""#),
        "{question}"
    );
    assert!(device.shown().contains("Searching for an update"));

    let file = update_file("0.5.0", &[("paperpaw-plugin-stats", "0.5.0")]);
    device.answer(&call, 200, offer(file.len()).as_bytes());
    let shown = device.shown();
    assert!(shown.contains("PaperPaw 0.5.0 is available"), "{shown}");
    assert!(
        shown.contains("chess-for-paperpaw (not in the plugin library)"),
        "{shown}"
    );
    assert!(shown.contains("Download without them"), "{shown}");

    device.run(actions::CONFIRM);
    let call = device.request();
    assert_eq!(call.method, "GET");
    assert_eq!(
        call.url,
        "https://paperpaw.org/api/v1/updates/paperpaw-x4-0.5.0-update.bin"
    );
    assert!(device.shown().contains("Downloading PaperPaw 0.5.0"));
    device.answer(&call, 200, &file);
    assert_eq!(
        device
            .storage
            .0
            .get("updates/paperpaw-xteink-0.5.0.bin")
            .as_deref(),
        Some(file.as_slice())
    );
    let shown = device.shown();
    assert!(shown.contains("Update without them"), "{shown}");
}

#[test]
fn a_download_cut_short_is_not_kept() {
    let mut device = Device::new(Shared::default());
    device.run(OPEN);
    device.run(actions::CONFIRM);
    let call = device.request();
    device.answer(&call, 200, offer(5000).as_bytes());
    device.run(actions::CONFIRM);
    let call = device.request();
    device.answer(&call, 200, b"only a part");
    let mut kept = Vec::new();
    let _ = device.storage.list("updates", &mut |entry| {
        kept.push(entry.name.to_string());
    });
    assert!(kept.is_empty(), "{kept:?}");
    // Offered again, to try once more.
    assert!(device.shown().contains("PaperPaw 0.5.0 is available"));
}

#[test]
fn the_server_knows_nothing_newer() {
    let mut device = Device::new(Shared::default());
    device.run(OPEN);
    device.run(actions::CONFIRM);
    let call = device.request();
    device.answer(&call, 200, br#"{"version": ""}"#);
    let shown = device.shown();
    assert!(shown.contains("PaperPaw is up to date"), "{shown}");
}

#[test]
fn no_answer_is_said() {
    let mut device = Device::new(Shared::default());
    device.run(OPEN);
    device.run(actions::CONFIRM);
    let call = device.request();
    device.answer(&call, 0, b"no network");
    let shown = device.shown();
    assert!(shown.contains("No answer"), "{shown}");
    assert!(shown.contains("(no network)"), "{shown}");
    assert!(shown.contains("Try again"), "{shown}");
}

#[test]
fn an_error_of_the_server_is_said() {
    let mut device = Device::new(Shared::default());
    device.run(OPEN);
    device.run(actions::CONFIRM);
    let call = device.request();
    device.answer(&call, 404, b"not found");
    let shown = device.shown();
    assert!(
        shown.contains("answered with an error (HTTP 404)"),
        "{shown}"
    );
}

#[test]
fn a_search_in_the_background_offers_what_it_finds_once() {
    let mut device = Device::new(Shared::default());
    device.run(CHECK);
    let call = device.request();
    device.answer(&call, 200, offer(5000).as_bytes());
    let shown = device.shown();
    assert!(shown.contains("PaperPaw 0.5.0 is available"), "{shown}");
    // Not offered again by the next search.
    device.run(actions::BACK);
    device.run(CHECK);
    let call = device.request();
    device.answer(&call, 200, offer(5000).as_bytes());
    assert!(!device.shown().contains("is available"));
}

#[test]
fn an_older_update_on_the_card_is_going_back() {
    let storage = Shared::default();
    storage
        .write(
            "updates/paperpaw-x4-0.3.0.bin",
            &update_file("0.3.0", &[("paperpaw-plugin-stats", "0.3.0")]),
        )
        .unwrap();
    let mut device = Device::new(storage);
    device.run(OPEN);
    let shown = device.shown();
    assert!(
        shown.contains("PaperPaw 0.3.0, an older version"),
        "{shown}"
    );
    assert!(shown.contains("Install older version"), "{shown}");
    // A newer one online is searched for from here, and comes first.
    device.run(actions::DOWN);
    device.run(actions::CONFIRM);
    let call = device.request();
    device.answer(&call, 200, offer(5000).as_bytes());
    let shown = device.shown();
    assert!(shown.contains("PaperPaw 0.5.0 is available"), "{shown}");
}

#[test]
fn the_same_version_on_the_card_is_installed_again_only() {
    let storage = Shared::default();
    storage
        .write(
            "updates/paperpaw-x4-0.4.0.bin",
            &update_file("0.4.0", &[("paperpaw-plugin-stats", "0.4.0")]),
        )
        .unwrap();
    let mut device = Device::new(storage);
    device.run(OPEN);
    let shown = device.shown();
    assert!(shown.contains("PaperPaw 0.4.0 again"), "{shown}");
    assert!(shown.contains("Install again"), "{shown}");
    // The server knows nothing newer: up to date, not the file again.
    device.run(actions::DOWN);
    device.run(actions::CONFIRM);
    let call = device.request();
    device.answer(&call, 200, br#"{"version": ""}"#);
    let shown = device.shown();
    assert!(shown.contains("PaperPaw is up to date"), "{shown}");
}

#[test]
fn a_version_that_is_no_version_is_no_offer() {
    let mut device = Device::new(Shared::default());
    device.run(OPEN);
    device.run(actions::CONFIRM);
    let call = device.request();
    device.answer(
        &call,
        200,
        br#"{"version": "0.5.0/../../paperpaw", "url": "/u.bin", "size": 10}"#,
    );
    let shown = device.shown();
    assert!(shown.contains("PaperPaw is up to date"), "{shown}");
    assert!(!device.storage.exists(".paperpaw/plugins/update/offer.txt"));
}

#[test]
fn a_download_a_restart_cut_off_is_let_go() {
    let storage = Shared::default();
    storage.write("updates/.download", b"half").unwrap();
    storage.write("updates/.download.part", b"half").unwrap();
    let device = Device::new(storage);
    assert!(!device.storage.exists("updates/.download"));
    assert!(!device.storage.exists("updates/.download.part"));
}

/// Loads no code: the test looks at what is fetched, not at what runs.
struct NoCode;

impl paperpaw_core::CodeLoader for NoCode {
    fn load(
        &mut self,
        _: &dyn Storage,
        _: &paperpaw_core::CodeFile<'_>,
    ) -> Result<Box<dyn paperpaw_sdk::Plugin>, paperpaw_sdk::PluginProblem> {
        Err(paperpaw_sdk::PluginProblem::Unsupported)
    }
}

/// The statistics plugin built into the running firmware (by its id).
struct BuiltInStats;

impl paperpaw_sdk::Plugin for BuiltInStats {
    fn manifest(&self) -> paperpaw_sdk::Manifest {
        paperpaw_sdk::Manifest::new(
            Id::new_static("stats"),
            "Statistics",
            paperpaw_sdk::Version::new(0, 4, 0),
        )
        .description("Reading statistics.")
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, _: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        Ok(())
    }
}

/// The author key of the plugin files.
const AUTHOR: [u8; 32] = [7; 32];

/// A plugin file built for the next firmware, signed by [`AUTHOR`].
fn code_file() -> Vec<u8> {
    use paperpaw_sdk::native::{Chip, Header, signature};
    let mut header = Header {
        chip: Chip::from_id(5).unwrap(),
        fingerprint: 0x5eed,
        base: 0x3c00_0000,
        image_len: 128,
        code_at: 96,
        entry: 0,
        api: 32,
        relocs: 0,
        align: 16,
        compiler: 1,
        hot_at: 0,
        hot_len: 0,
        patches: 0,
        needs_api: 0,
        families: [0; 8],
        interface: paperpaw_sdk::native::INTERFACE,
        sha256: [0; 32],
    };
    let image = [0u8; 128];
    let mut digest = header.digest();
    digest.update(&image);
    header.sha256 = digest.finish();
    let mut file = header.to_bytes().to_vec();
    file.extend_from_slice(&image);
    let trailer = signature::sign(&file, &AUTHOR);
    file.extend_from_slice(&trailer.to_bytes());
    file
}

const PLUGIN_INI: &str = "[plugin]\nid = stats\nname = Statistics\nversion = 0.5.0\n";

/// The server's answer naming PaperPaw 0.5.0, which has the statistics
/// as a file instead of built in.
fn offer_with_code(size: usize, code: &[u8]) -> String {
    format!(
        r#"{{"version": "0.5.0", "url": "/api/v1/updates/paperpaw-x4-0.5.0-update.bin",
            "size": {size}, "plugins": ["chess-for-paperpaw 1.1.0"],
            "abi": "0000000000005eed", "compiler": "00000001",
            "code": [{{"id": "stats", "name": "Statistics", "version": "0.5.0",
              "author_key": "{author}",
              "files": [
                {{"path": "plugin.ini", "url": "https://p.org/stats/plugin.ini",
                  "size": {ini_size}, "sha1": "{ini_sha1}"}},
                {{"path": "code.ppn", "url": "https://p.org/stats/x4/code.ppn",
                  "size": {code_size}, "sha256": "{code_sha256}"}}]}}]}}"#,
        author = paperpaw_sdk::digest::hex::encode(&paperpaw_sdk::native::signature::public_key(
            &AUTHOR
        )),
        ini_size = PLUGIN_INI.len(),
        ini_sha1 = paperpaw_sdk::digest::sha1::hex(PLUGIN_INI.as_bytes()),
        code_size = code.len(),
        code_sha256 = paperpaw_sdk::digest::sha256::hex(code),
    )
}

/// PaperPaw `version` with the statistics built in, loading code
/// plugins, the card as `storage` has it.
fn with_code(storage: Shared, version: &str) -> Device {
    let firmware = Firmware::new(version, PROJECT, CHIP)
        .keys(vec![paperpaw_update::public_key(&SEED)])
        .room(1_000_000)
        .plugins(Plugins::new(RUNNING));
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .network(true)
        .firmware(firmware)
        .code_plugins(
            paperpaw_sdk::native::Native::new("code.ppn"),
            Box::new(NoCode),
        )
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(BuiltInStats)
        .plugin(Update)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    Device {
        kernel,
        storage,
        now: 0,
    }
}

/// The update 0.5.0 downloaded, the statistics' fetch begun: the request
/// for its plugin.ini asked for, not answered.
fn fetch_begun(device: &mut Device) -> Vec<u8> {
    device.run(OPEN);
    device.run(actions::CONFIRM);
    let call = device.request();
    let file = update_file("0.5.0", &[("chess-for-paperpaw", "1.1.0")]);
    let code = code_file();
    device.answer(&call, 200, offer_with_code(file.len(), &code).as_bytes());
    device.run(actions::CONFIRM);
    let call = device.request();
    device.answer(&call, 200, &file);
    code
}

#[test]
fn a_fetch_a_restart_cut_off_goes_on_when_the_page_shows() {
    let storage = Shared::default();
    let mut device = with_code(storage.clone(), "0.4.0");
    let code = fetch_begun(&mut device);
    let call = device.request();
    assert_eq!(call.url, "https://p.org/stats/plugin.ini");
    // A restart (on the X4, sleep) cuts the download off. The update,
    // downloaded, is offered at the start: its page shows, and the fetch
    // goes on from the plugin cut off.
    let mut device = with_code(storage, "0.4.0");
    let call = device.request();
    assert_eq!(call.url, "https://p.org/stats/plugin.ini");
    assert!(device.shown().contains("Downloading PaperPaw 0.5.0"));
    device.answer(&call, 200, PLUGIN_INI.as_bytes());
    let call = device.request();
    device.answer(&call, 200, &code);
    assert!(device.storage.exists("plugins/.stats.next/code.ppn"));
    let shown = device.shown();
    assert!(shown.contains("Install"), "{shown}");
}

#[test]
fn a_fetch_failing_does_not_hold_the_update_back() {
    let storage = Shared::default();
    let mut device = with_code(storage.clone(), "0.4.0");
    fetch_begun(&mut device);
    let call = device.request();
    device.answer(&call, 0, b"no network");
    let shown = device.shown();
    // Ready, the statistics said to come from the store later.
    assert!(shown.contains("Install"), "{shown}");
    assert!(!device.storage.exists(".paperpaw/plugins/update/fetch.txt"));
}

#[test]
fn what_is_left_for_an_update_installed_meanwhile_is_let_go() {
    let storage = Shared::default();
    let mut device = with_code(storage.clone(), "0.4.0");
    fetch_begun(&mut device);
    // Installed from elsewhere (the card) before the fetch was done.
    let device = with_code(storage, "0.5.0");
    assert!(!device.storage.exists(".paperpaw/plugins/update/fetch.txt"));
    assert!(
        !device
            .storage
            .exists(".paperpaw/plugins/update/fetch-current.txt")
    );
}

#[test]
fn plugins_that_become_files_are_fetched_before_the_update() {
    let mut device = with_code(Shared::default(), "0.4.0");
    device.run(OPEN);
    device.run(actions::CONFIRM);
    let call = device.request();
    let file = update_file("0.5.0", &[("chess-for-paperpaw", "1.1.0")]);
    let code = code_file();
    device.answer(&call, 200, offer_with_code(file.len(), &code).as_bytes());
    let shown = device.shown();
    // The statistics come back as a file: not named as left out.
    assert!(shown.contains("PaperPaw 0.5.0 is available"), "{shown}");
    assert!(!shown.contains("cannot come along"), "{shown}");

    device.run(actions::CONFIRM);
    let call = device.request();
    device.answer(&call, 200, &file);
    // Then the plugin's files, one after another; the update waits.
    let call = device.request();
    assert_eq!(call.url, "https://p.org/stats/plugin.ini");
    assert!(device.shown().contains("Downloading PaperPaw 0.5.0"));
    device.answer(&call, 200, PLUGIN_INI.as_bytes());
    let call = device.request();
    assert_eq!(call.url, "https://p.org/stats/x4/code.ppn");
    device.answer(&call, 200, &code);

    // Where the updated firmware installs it, its code named as fetched.
    let storage = &device.storage;
    assert!(storage.exists("plugins/.stats.next/plugin.ini"));
    assert!(storage.exists("plugins/.stats.next/code.ppn"));
    let header =
        paperpaw_sdk::native::read_header(storage, "plugins/.stats.next/code.ppn").unwrap();
    assert_eq!(
        paperpaw_sdk::native::fetched_code(storage, "stats"),
        Some(header.sha256)
    );
    // Not as the code installed now.
    assert!(paperpaw_sdk::native::installed_code(storage, "stats").is_none());
    let shown = device.shown();
    assert!(shown.contains("Install"), "{shown}");
    assert!(!shown.contains("cannot come along"), "{shown}");
}

#[test]
fn a_plugin_without_room_for_its_code_is_not_fetched() {
    let storage = Shared::default();
    // The room this device has for code plugins, too small for it.
    let room = paperpaw_sdk::native::CodeRoom::new(64, 0, 1);
    assert!(paperpaw_sdk::native::keep_code_room(&storage, Some(&room)));
    let mut device = with_code(storage, "0.4.0");
    fetch_begun(&mut device);
    let io = device.kernel.take_io();
    assert!(io.iter().all(|io| !matches!(io, Io::Http(_))), "{io:?}");
    // Ready, and said as left out: it comes from the store, room made.
    let shown = device.shown().replace(" | ", " ");
    assert!(shown.contains("Update without them"), "{shown}");
    assert!(
        shown.contains("cannot come along: paperpaw-plugin-stats"),
        "{shown}"
    );
    assert!(!device.storage.exists("plugins/.stats.next/plugin.ini"));
}

/// With *Beta updates* on, the question asks for betas; a beta offered
/// shows as one, calmly, where its version shows.
#[test]
fn betas_are_asked_for_when_wanted_and_shown_as_betas() {
    let mut device = Device::new(Shared::default());
    device.run(OPEN);
    device.run(actions::CONFIRM);
    let call = device.request();
    let question = String::from_utf8(call.body.clone().unwrap_or_default()).unwrap();
    assert!(!question.contains("betas"), "off at first: {question}");

    let mut device = Device::new(Shared::default());
    let _ = device
        .kernel
        .set_setting(Instant(1), "update.betas", "true");
    device.run(OPEN);
    device.run(actions::CONFIRM);
    let call = device.request();
    let question = String::from_utf8(call.body.clone().unwrap_or_default()).unwrap();
    assert!(question.contains(r#","betas":true"#), "{question}");
    let file = update_file("0.5.0-beta.2", &[("paperpaw-plugin-stats", "0.5.0-beta.2")]);
    let answer = offer(file.len()).replace("0.5.0", "0.5.0-beta.2");
    device.answer(&call, 200, answer.as_bytes());
    let shown = device.shown();
    assert!(
        shown.contains("PaperPaw 0.5.0 Beta 2 is available"),
        "{shown}"
    );
    assert!(!shown.contains("0.5.0-beta.2"), "{shown}");
}

/// A code plugin the store did not install (local, as one in development
/// is) is never put in place by the update's build of it; one of the
/// store's is fetched, and stays the store's at the version fetched.
#[test]
fn only_the_stores_code_plugins_are_fetched_for_an_update() {
    let v031 = paperpaw_sdk::Version::new(0, 3, 1);
    for (origins, fetched) in [("", false), ("dict-xl\t0.3.1\n", true)] {
        let storage = Shared::default();
        storage
            .0
            .write(".paperpaw/plugin-origin.txt", origins.as_bytes())
            .unwrap();
        let mut device = with_code_plugin(storage, "0.4.0", "");
        device.run(OPEN);
        device.run(actions::CONFIRM);
        let call = device.request();
        let file = update_file("0.5.0", &[("chess-for-paperpaw", "1.1.0")]);
        let code = code_file();
        let offer = offer_with_code(file.len(), &code).replace("\"stats\"", "\"dict-xl\"");
        device.answer(&call, 200, offer.as_bytes());
        device.run(actions::CONFIRM);
        let call = device.request();
        device.answer(&call, 200, &file);
        let storage = device.storage.0.clone();
        if !fetched {
            device.no_request();
            assert!(!storage.exists("plugins/.dict-xl.next/code.ppn"));
            continue;
        }
        let call = device.request();
        device.answer(&call, 200, PLUGIN_INI.as_bytes());
        let call = device.request();
        device.answer(&call, 200, &code);
        assert!(storage.exists("plugins/.dict-xl.next/code.ppn"));
        let origin = |v| paperpaw_sdk::install::origin(storage.as_ref(), "dict-xl", v);
        assert_eq!(origin(v031), paperpaw_sdk::install::Origin::Store);
    }
}

impl Device {
    /// Plugged in, at `unix` seconds since 1970: a cheap moment.
    fn charge(&mut self, unix: i64) {
        self.power(unix, true);
    }

    /// Plugged in or not, at `unix` seconds since 1970.
    fn power(&mut self, unix: i64, charging: bool) {
        self.now += 100;
        let status = paperpaw_platform::SystemStatus::new(
            Some(paperpaw_platform::WallClock::new(
                Instant(self.now),
                unix * 1000,
                None,
            )),
            Some(paperpaw_platform::Battery::new(50, charging, None)),
        );
        self.kernel.set_status(Instant(self.now), status);
    }
}

/// *Look for updates*: off unless chosen; then the server is asked at a
/// cheap moment (here: charging), what it names waits quietly on the
/// update page (nothing opened, its entry badged), and a start within the
/// week asks again only once the week is over.
#[test]
fn looking_for_updates_is_chosen_and_weekly_at_a_cheap_moment() {
    const DAY: i64 = 24 * 3600;
    let start = 1_790_000_000;
    let storage = Shared::default();
    let mut device = Device::at(storage.clone(), Some(start));
    device.charge(start + 10);
    device.no_request();
    device.power(start + 15, false);

    device
        .kernel
        .set_setting(Instant(1), "update.look", "true")
        .unwrap();
    // On the battery, the radio off: nothing yet.
    device.no_request();
    device.charge(start + 20);
    let call = device.request();
    assert!(call.url.ends_with("/api/v1/updates"), "{}", call.url);
    let file = update_file("0.5.0", &[("paperpaw-plugin-stats", "0.5.0")]);
    device.answer(&call, 200, offer(file.len()).as_bytes());
    // Kept for the page, which does not open by itself.
    assert!(storage.exists(OFFER));
    assert!(!device.shown().contains("is available"));

    // Two days later, started again and charging: not yet.
    let mut device = Device::at(storage.clone(), Some(start + 2 * DAY));
    device.charge(start + 2 * DAY + 10);
    device.no_request();
    // A week after the last look: again.
    let mut device = Device::at(storage.clone(), Some(start + 8 * DAY));
    device.charge(start + 8 * DAY + 10);
    let _ = device.request();
}

/// Another firmware's app image (CrossPoint's), not signed, as its
/// maker's build ends: header, one segment, checksum, hash.
fn other_firmware() -> Vec<u8> {
    let mut image = vec![0u8; 24];
    image[0] = 0xE9;
    image[1] = 1;
    image[12..14].copy_from_slice(&CHIP.to_le_bytes());
    image[23] = 1;
    let mut data = vec![0u8; 400];
    data[..4].copy_from_slice(&0xABCD_5432u32.to_le_bytes());
    data[16..21].copy_from_slice(b"1.2.3");
    data[48..58].copy_from_slice(b"crosspoint");
    image.extend_from_slice(&0x4200_0000u32.to_le_bytes());
    image.extend_from_slice(&(data.len() as u32).to_le_bytes());
    let xor = data.iter().fold(0xEF, |x, b| x ^ b);
    image.extend_from_slice(&data);
    while image.len() % 16 != 15 {
        image.push(0);
    }
    image.push(xor);
    let hash = paperpaw_digest::sha256::digest(&image);
    image.extend_from_slice(&hash);
    image
}

#[test]
fn another_firmware_installs_only_as_chosen() {
    let storage = Shared::default();
    storage.write("firmware.bin", &other_firmware()).unwrap();
    let mut device = Device::new(storage);
    // Never offered at start.
    assert!(!device.shown().contains("crosspoint"));
    device.run(OPEN);
    let shown = device.shown();
    assert!(
        shown.contains("Other firmware: crosspoint 1.2.3"),
        "{shown}"
    );
    assert!(shown.contains("not PaperPaw and not signed"), "{shown}");
    // *Not now* first: closing installs nothing.
    assert!(shown.contains("Not now"), "{shown}");
    assert!(shown.contains("Install anyway"), "{shown}");
    device.run(actions::DOWN);
    device.run(actions::CONFIRM);
    assert_eq!(
        device.install(),
        FirmwareCommand::install_foreign("firmware.bin")
    );
}
