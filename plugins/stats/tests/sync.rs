//! Syncing the statistics with KOReader's through a WebDAV folder, with
//! the real kernel and answers as a server gives them: what is uploaded
//! and on which condition, and that a download that cannot be read or a
//! change of folder deletes nothing here.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::{ButtonState, InputEvent, Rotation};
use paperpaw_sdk::secrets::{FILE, SecretChange, Secrets};
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Large,
);

const DIR: &str = ".paperpaw/plugins/stats";
const FOLDER: &str = "https://dav.example/koreader";

/// A device with the statistics plugin and the folder set.
struct Device {
    kernel: Kernel,
    now: u64,
}

impl Device {
    fn new(storage: MemoryStorage) -> Self {
        let mut secrets = Secrets::default();
        secrets.apply(SecretChange::keep("stats.webdav", "", "pw"));
        let storage = storage.with(FILE, secrets.encode().into_bytes());
        let config =
            "[core]\nlanguage = en\n[stats]\nwebdav-url = dav.example/koreader\nwebdav-user = me\n";
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .user_config(config)
            .network(true)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_stats::Stats)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        Self { kernel, now: 0 }
    }

    fn at(&mut self) -> Instant {
        self.now += 100;
        Instant(self.now)
    }

    fn press(&mut self, name: &'static str) {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            let at = self.at();
            self.kernel.handle(at, InputEvent::Button { name, state });
        }
    }

    /// *Sync now* on the sync page: the download asked for.
    fn sync(&mut self) -> Vec<HttpCall> {
        // From the start of the page: one open is gone back to as it was.
        if self.kernel.active_screen().map(Id::as_str) == Some("stats.sync") {
            let at = self.at();
            self.kernel
                .run_action(at, paperpaw_sdk::builtin::actions::BACK);
        }
        let at = self.at();
        self.kernel
            .open_screen(at, Id::new_static("stats.sync"), Params::new());
        for _ in 0..3 {
            self.press("down");
        }
        self.press("confirm");
        self.requests()
    }

    fn requests(&mut self) -> Vec<HttpCall> {
        let io = self.kernel.take_io().into_iter();
        io.filter_map(|io| match io {
            Io::Http(call) => Some(call),
            _ => None,
        })
        .collect()
    }

    /// Answers `call`; the requests that follow.
    fn answer(
        &mut self,
        call: &HttpCall,
        status: u16,
        body: &[u8],
        etag: Option<&str>,
    ) -> Vec<HttpCall> {
        let at = self.at();
        self.kernel.http_response(
            at,
            HttpResponse {
                id: call.id,
                status,
                body: body.to_vec(),
                etag: etag.map(String::from),
                ..HttpResponse::default()
            },
        );
        self.requests()
    }

    /// The file an upload sends.
    fn body(&self, put: &HttpCall) -> Vec<u8> {
        let path = put.body_from.as_deref().expect("a file to send");
        let storage = self.kernel.storage();
        let size = storage.size(path).unwrap() as usize;
        let mut data = vec![0u8; size];
        storage.read_at(path, 0, &mut data).unwrap();
        data
    }

    /// What the screen says.
    fn shown(&mut self) -> String {
        let mut text = String::new();
        for part in self.kernel.describe() {
            text.push_str(&part.label);
            text.push('\n');
        }
        text
    }

    fn file(&self, name: &str) -> Option<Vec<u8>> {
        let storage = self.kernel.storage();
        let path = format!("{DIR}/{name}");
        let size = storage.size(&path).ok()? as usize;
        let mut data = vec![0u8; size];
        storage.read_at(&path, 0, &mut data).ok()?;
        Some(data)
    }

    /// The names of the plugin's files.
    fn files(&self) -> Vec<String> {
        let mut names = Vec::new();
        let _ = self
            .kernel
            .storage()
            .list(DIR, &mut |entry| names.push(entry.name.to_string()));
        names.sort();
        names
    }

    fn header<'a>(call: &'a HttpCall, name: &str) -> Option<&'a str> {
        call.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Two books and three stays on pages.
fn statistics() -> MemoryStorage {
    let books = "[book.1]\ntitle = Anna Karenina\nmd5 = 0f343b0931126a20f133d67c2b018a3b\npages = 864\n\n\
                 [book.2]\ntitle = Effi Briest\nmd5 = 1f343b0931126a20f133d67c2b018a3b\npages = 300\n";
    let mut log = Vec::new();
    for (book, page, start) in [
        (1u16, 5u32, 1_760_000_000u32),
        (1, 6, 1_760_000_060),
        (2, 1, 1_760_100_000),
    ] {
        log.extend_from_slice(&book.to_le_bytes());
        log.extend_from_slice(&page.to_le_bytes());
        log.extend_from_slice(&start.to_le_bytes());
        log.extend_from_slice(&50u16.to_le_bytes());
        log.extend_from_slice(&300u32.to_le_bytes());
    }
    MemoryStorage::new()
        .with(&format!("{DIR}/books.ini"), books)
        .with(&format!("{DIR}/pages.bin"), log)
}

/// What a device with no statistics uploads: a KOReader statistics file
/// without books.
fn empty_statistics_file() -> Vec<u8> {
    let mut other = Device::new(MemoryStorage::new());
    let get = other.sync();
    let put = other.answer(&get[0], 404, b"", None);
    other.body(&put[0])
}

/// The first upload goes up only while there is still no file; when one
/// came in between, the sync starts over from it.
#[test]
fn the_first_upload_waits_for_none() {
    let mut device = Device::new(statistics());
    let get = device.sync();
    assert_eq!(get.len(), 1);
    assert_eq!(get[0].method, "GET");
    assert_eq!(get[0].url, format!("{FOLDER}/statistics.sqlite3"));
    let put = device.answer(&get[0], 404, b"Not found", None);
    assert_eq!(put[0].method, "PUT");
    assert_eq!(Device::header(&put[0], "If-None-Match"), Some("*"));
    assert_eq!(Device::header(&put[0], "If-Match"), None);
    let uploaded = device.body(&put[0]);
    // Someone was faster: from the download again, then on its ETag.
    let get = device.answer(&put[0], 412, b"", None);
    assert_eq!(get[0].method, "GET");
    let put = device.answer(&get[0], 200, &uploaded, Some("\"e1\""));
    assert_eq!(Device::header(&put[0], "If-Match"), Some("\"e1\""));
    assert_eq!(Device::header(&put[0], "If-None-Match"), None);
    assert!(device.answer(&put[0], 201, b"", None).is_empty());
    assert!(
        device.shown().contains("Synced: 2 books."),
        "{}",
        device.shown()
    );
    // Kept to compare with: the last upload, and nothing else.
    let files = device.files();
    assert_eq!(files.len(), 3, "{files:?}");
    assert!(files[2].starts_with("statistics.sqlite3.") && files[2].ends_with(".sync"));
}

/// A download that is no statistics file (a login page with status 200,
/// or a file cut short) ends the sync: nothing is merged and nothing
/// uploaded over the file in the folder.
#[test]
fn a_download_that_cannot_be_read_changes_nothing() {
    let mut device = Device::new(statistics());
    let before = (device.file("books.ini"), device.file("pages.bin"));
    let get = device.sync();
    let after = device.answer(
        &get[0],
        200,
        b"<html><body>Please log in</body></html>",
        Some("\"e1\""),
    );
    assert!(after.is_empty(), "nothing uploaded: {after:?}");
    assert!(
        device.shown().contains("cannot be read"),
        "{}",
        device.shown()
    );
    assert_eq!((device.file("books.ini"), device.file("pages.bin")), before);
    assert_eq!(device.files(), ["books.ini", "pages.bin"]);
}

/// After the folder (or user) changed, what was uploaded to the old one
/// tells nothing about the new one: books it lacks are not taken for
/// deleted there, here or in the upload.
#[test]
fn another_folder_deletes_nothing_here() {
    let empty = empty_statistics_file();
    let mut device = Device::new(statistics());
    let get = device.sync();
    let put = device.answer(&get[0], 404, b"", None);
    assert!(device.answer(&put[0], 201, b"", None).is_empty());
    let before = (device.file("books.ini"), device.file("pages.bin"));
    // Another folder, in which KOReader keeps statistics of no books.
    let at = device.at();
    device
        .kernel
        .set_setting(at, "stats.webdav-url", "dav.example/other")
        .unwrap();
    let get = device.sync();
    assert_eq!(get[0].url, "https://dav.example/other/statistics.sqlite3");
    let put = device.answer(&get[0], 200, &empty, Some("\"e2\""));
    assert_eq!(put[0].method, "PUT");
    assert_eq!((device.file("books.ini"), device.file("pages.bin")), before);
    let uploaded = device.body(&put[0]);
    assert!(device.answer(&put[0], 201, b"", None).is_empty());
    assert!(
        device.shown().contains("Synced: 2 books."),
        "{}",
        device.shown()
    );
    // The upload has both books.
    let mut other = Device::new(MemoryStorage::new());
    let get = other.sync();
    let put = other.answer(&get[0], 200, &uploaded, None);
    assert!(other.answer(&put[0], 201, b"", None).is_empty());
    assert!(
        other.shown().contains("Synced: 2 books."),
        "{}",
        other.shown()
    );
    // One last upload is kept, the new folder's.
    let kept: Vec<String> = device
        .files()
        .into_iter()
        .filter(|f| f.ends_with(".sync"))
        .collect();
    assert_eq!(kept.len(), 1, "{kept:?}");
}

/// The last upload of earlier versions, kept for whichever folder was set
/// then, is not used, and goes after the next sync.
#[test]
fn the_last_upload_of_earlier_versions_is_left_out() {
    let empty = empty_statistics_file();
    // Uploaded with both books before.
    let mut before = Device::new(statistics());
    let get = before.sync();
    let put = before.answer(&get[0], 404, b"", None);
    let old = before.body(&put[0]);
    let storage = statistics().with(&format!("{DIR}/statistics.sqlite3.sync"), old);
    let mut device = Device::new(storage);
    let get = device.sync();
    let put = device.answer(&get[0], 200, &empty, Some("\"e3\""));
    assert!(device.answer(&put[0], 201, b"", None).is_empty());
    assert!(
        device.shown().contains("Synced: 2 books."),
        "{}",
        device.shown()
    );
    assert!(device.file("statistics.sqlite3.sync").is_none());
}
