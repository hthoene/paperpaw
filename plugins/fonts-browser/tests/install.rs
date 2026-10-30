//! The fonts browser with the real kernel: the catalog is fetched and
//! shown, a font is downloaded file by file, checked, loaded at once and
//! chosen for reading, then removed; a damaged download is not
//! installed, nor one cut off by a restart; the catalog is fetched once
//! each time the list is opened.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::{HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_plugin_fonts_browser::{FontsBrowser, OPEN};
use paperpaw_sdk::builtin::{actions, settings};
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage,
    PixelFormat, Storage, StorageError,
};
use paperpaw_text::{Glyph, LineMetrics, PackBuilder};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test",
    name: "Test device",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Mono1,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: false,
        pen: false,
        keys: &[],
    },
    memory: MemoryClass::Small,
};

const CATALOG: &str = "https://paperpaw.org/api/v1/fonts.json";

/// The font's `plugin.ini` in `version`.
fn plugin_ini(version: &str) -> String {
    format!(
        "[plugin]
id = font-round
name = Round
version = {version}

[family]
name = Round
regular = regular.ppfp
"
    )
}

/// A pack with a few letters.
fn pack() -> Vec<u8> {
    let metrics = LineMetrics::new(8.0, 2.0, 12.0);
    let mut builder = PackBuilder::new();
    let glyphs = "Rabcd".chars().map(|c| (c, Glyph::empty(5.0))).collect();
    builder.add_strike(26.0, metrics, glyphs);
    builder.build()
}

fn catalog(pack: &[u8], pack_sha1: &str) -> String {
    catalog_of(pack, pack_sha1, "1.0.0")
}

/// The catalog with the font in `version`.
fn catalog_of(pack: &[u8], pack_sha1: &str, version: &str) -> String {
    let ini = plugin_ini(version);
    format!(
        r#"{{"format": 1, "fonts": [{{
  "id": "round", "name": "Round", "category": "sans", "author": "Someone",
  "licence": "OFL-1.1", "version": "{version}", "styles": 1,
  "sample": "https://paperpaw.org/api/v1/fonts/round/sample.ppfp",
  "packs": [
    {{"path": "plugin.ini", "url": "https://paperpaw.org/f/round/plugin.ini",
      "size": {}, "sha1": "{}"}},
    {{"path": "fonts/regular.ppfp", "url": "https://paperpaw.org/f/round/regular.ppfp",
      "size": {}, "sha1": "{pack_sha1}"}}
  ]}},
  {{"id": "../bad", "name": "Bad", "packs": []}}
]}}"#,
        ini.len(),
        paperpaw_digest::sha1::hex(ini.as_bytes()),
        pack.len(),
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
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove_dir_all(path)
    }
}

struct Browsing {
    kernel: Kernel,
    storage: Shared,
    now: u64,
}

/// PaperPaw started with the fonts browser on `storage`.
fn started(storage: &Shared) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .network(true)
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(FontsBrowser)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

impl Browsing {
    /// The list opened and its catalog answered.
    fn new(catalog: &str) -> Self {
        let storage = Shared::default();
        let mut browsing = Self {
            kernel: started(&storage),
            storage,
            now: 0,
        };
        browsing.run(OPEN);
        let call = browsing.request(CATALOG);
        browsing.answer(&call, 200, catalog.as_bytes());
        browsing
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        self.kernel.run_action(Instant(self.now), action);
    }

    /// The requests the kernel asks the shell to make now.
    fn requests(&mut self) -> Vec<HttpCall> {
        self.kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Http(call) => Some(call),
                _ => None,
            })
            .collect()
    }

    /// The one request for `url` (others, e.g. for samples, go
    /// unanswered).
    fn request(&mut self, url: &str) -> HttpCall {
        let calls = self.requests();
        let mut calls: Vec<HttpCall> = calls.into_iter().filter(|c| c.url == url).collect();
        assert_eq!(calls.len(), 1, "{url}: {calls:?}");
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

    /// The labels of what the screen shows, in order.
    fn shown(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// Downloads the font from its page, its two files answered with
    /// `pack`.
    fn download(&mut self, pack: &[u8]) {
        // The font's page, then *Download*.
        self.run(actions::CONFIRM);
        self.run(actions::CONFIRM);
        self.answer_files("1.0.0", pack);
    }

    /// Answers the requests for the font's files in `version`.
    fn answer_files(&mut self, version: &str, pack: &[u8]) {
        let call = self.request("https://paperpaw.org/f/round/plugin.ini");
        assert_eq!(
            call.save_to.as_deref(),
            Some("plugins/.font-round.part/plugin.ini")
        );
        self.answer(&call, 200, plugin_ini(version).as_bytes());
        let call = self.request("https://paperpaw.org/f/round/regular.ppfp");
        self.answer(&call, 200, pack);
    }
}

#[test]
fn the_catalog_is_shown_with_what_fits() {
    let pack = pack();
    let sha1 = paperpaw_digest::sha1::hex(&pack);
    let mut browsing = Browsing::new(&catalog(&pack, &sha1));
    let shown = browsing.shown();
    assert!(shown.contains("Round"), "{shown}");
    assert!(shown.contains("Sans serif"), "{shown}");
    // One that would leave its folder is not offered.
    assert!(!shown.contains("Bad"), "{shown}");
    // Kept for the next visit.
    assert!(
        browsing
            .storage
            .exists(".paperpaw/plugins/fonts-browser/catalog.json")
    );
}

#[test]
fn a_font_is_downloaded_chosen_and_removed() {
    let pack = pack();
    let sha1 = paperpaw_digest::sha1::hex(&pack);
    let mut browsing = Browsing::new(&catalog(&pack, &sha1));
    browsing.download(&pack);
    // Loaded at once, as a plugin of its own.
    let plugin = browsing.kernel.plugins().get("font-round").unwrap();
    assert_eq!(plugin.manifest.name, "Round");
    assert!(
        browsing
            .storage
            .exists("plugins/font-round/fonts/regular.ppfp")
    );
    assert!(
        !browsing
            .storage
            .exists("plugins/.font-round.part/plugin.ini")
    );
    let shown = browsing.shown();
    assert!(shown.contains("Read with Round"), "{shown}");

    // *Read with Round*: the reading font.
    browsing.run(actions::CONFIRM);
    assert_eq!(
        browsing
            .kernel
            .settings()
            .text(settings::READING_FONT.as_str()),
        "Round"
    );

    // *Remove*, after *Status lines*: asked first.
    browsing.run(actions::DOWN);
    browsing.run(actions::DOWN);
    browsing.run(actions::CONFIRM);
    let shown = browsing.shown();
    assert!(shown.contains("Remove Round"), "{shown}");
    // The focus on *Keep*, the safe answer: left to *Remove*.
    browsing.run(actions::LEFT);
    browsing.run(actions::CONFIRM);
    assert!(browsing.kernel.plugins().get("font-round").is_none());
    assert!(!browsing.storage.exists("plugins/font-round/plugin.ini"));
    assert_eq!(
        browsing
            .kernel
            .settings()
            .text(settings::READING_FONT.as_str()),
        settings::DEFAULT_FONT
    );
    let shown = browsing.shown();
    assert!(shown.contains("Download"), "{shown}");
}

#[test]
fn a_damaged_download_is_not_installed() {
    let pack = pack();
    let sha1 = paperpaw_digest::sha1::hex(&pack);
    let mut browsing = Browsing::new(&catalog(&pack, &sha1));
    let mut damaged = pack.clone();
    let last = damaged.len() - 1;
    damaged[last] ^= 0xFF;
    browsing.download(&damaged);
    assert!(browsing.kernel.plugins().get("font-round").is_none());
    assert!(
        !browsing
            .storage
            .exists("plugins/.font-round.part/plugin.ini")
    );
    assert!(
        !browsing
            .storage
            .exists(".paperpaw/plugins/fonts-browser/install.txt")
    );
    // Offered again.
    let shown = browsing.shown();
    assert!(shown.contains("Download"), "{shown}");
}

#[test]
fn updating_the_font_one_reads_with_keeps_it() {
    let pack = pack();
    let sha1 = paperpaw_digest::sha1::hex(&pack);
    let mut browsing = Browsing::new(&catalog(&pack, &sha1));
    browsing.download(&pack);
    // *Read with Round*.
    browsing.run(actions::CONFIRM);
    let reading = |browsing: &Browsing| {
        String::from(
            browsing
                .kernel
                .settings()
                .text(settings::READING_FONT.as_str()),
        )
    };
    assert_eq!(reading(&browsing), "Round");

    // A newer version in the catalog, fetched when the list is opened
    // again: its page offers *Update*, after *Read with*, *Status lines*.
    browsing.run(actions::BACK);
    browsing.run(actions::BACK);
    browsing.run(OPEN);
    let call = browsing.request(CATALOG);
    browsing.answer(&call, 200, catalog_of(&pack, &sha1, "1.1.0").as_bytes());
    browsing.run(actions::CONFIRM);
    browsing.run(actions::DOWN);
    browsing.run(actions::DOWN);
    browsing.run(actions::CONFIRM);
    browsing.answer_files("1.1.0", &pack);
    let round = browsing.kernel.plugins().get("font-round").unwrap();
    assert_eq!(round.manifest.version.minor, 1);
    assert_eq!(reading(&browsing), "Round");
    assert!(browsing.storage.exists("plugins/font-round/plugin.ini"));
}

#[test]
fn coming_back_to_the_list_fetches_nothing() {
    let pack = pack();
    let sha1 = paperpaw_digest::sha1::hex(&pack);
    let mut browsing = Browsing::new(&catalog(&pack, &sha1));
    // The font's page and back: the catalog fetched on opening serves.
    browsing.run(actions::CONFIRM);
    browsing.run(actions::BACK);
    let calls = browsing.requests();
    assert!(calls.iter().all(|c| c.url != CATALOG), "{calls:?}");
    let shown = browsing.shown();
    assert!(shown.contains("Round"), "{shown}");
}

#[test]
fn a_font_page_has_its_font_from_the_list() {
    let pack = pack();
    let sha1 = paperpaw_digest::sha1::hex(&pack);
    let mut browsing = Browsing::new(&catalog(&pack, &sha1));
    // The catalog kept is not read again for the font's page.
    browsing
        .storage
        .remove(".paperpaw/plugins/fonts-browser/catalog.json")
        .unwrap();
    browsing.run(actions::CONFIRM);
    let shown = browsing.shown();
    assert!(shown.contains("Download"), "{shown}");
    browsing.run(actions::CONFIRM);
    browsing.answer_files("1.0.0", &pack);
    assert!(browsing.kernel.plugins().get("font-round").is_some());
}

#[test]
fn a_download_cut_off_by_a_restart_is_dropped() {
    let pack = pack();
    let sha1 = paperpaw_digest::sha1::hex(&pack);
    let mut browsing = Browsing::new(&catalog(&pack, &sha1));
    // *Download*, its first file never answered.
    browsing.run(actions::CONFIRM);
    browsing.run(actions::CONFIRM);
    let job = ".paperpaw/plugins/fonts-browser/install.txt";
    assert!(browsing.storage.exists(job));

    // A restart: the download is not shown as going on forever.
    let mut kernel = started(&browsing.storage);
    assert!(!browsing.storage.exists(job));
    assert!(!browsing.storage.exists("plugins/.font-round.part"));
    kernel.run_action(Instant(100), OPEN);
    kernel.render(&mut |_, _| {});
    let shown: Vec<String> = kernel.describe().into_iter().map(|p| p.label).collect();
    assert!(
        shown.iter().all(|label| !label.contains("0 / 2")),
        "{shown:?}"
    );
}
