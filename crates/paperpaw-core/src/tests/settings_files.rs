//! Files of settings the kernel keeps (`paperpaw.ini`, a book's own
//! settings, passwords and keys): read as they are, never written over
//! where they could not be read.

use super::*;
use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::builtin::CONFIG_FILE;

/// A storage whose reads of one file fail, as a failing card's do.
struct FailingReads {
    inner: MemoryStorage,
    path: &'static str,
}

impl Storage for FailingReads {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if path == self.path {
            return Err(StorageError::Io);
        }
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
}

/// The bytes of `path`, read past every cache.
fn bytes(kernel: &Kernel, path: &str) -> Vec<u8> {
    kernel.shared.recall.clear();
    paperpaw_sdk::read_file(kernel.storage(), path, 1 << 20).unwrap_or_default()
}

fn kernel_with(storage: impl Storage + 'static) -> Kernel {
    Kernel::builder(DEVICE)
        .plugin(NoticePlugin { log: log() })
        .storage(storage)
        .build()
        .unwrap()
}

/// Every way a stored `paperpaw.ini` can be there but unreadable.
fn unreadable_configs() -> Vec<(&'static str, Box<dyn Storage>)> {
    let mut large = String::from("[core]\nui-size = 12\n");
    while large.len() <= 70 * 1024 {
        large.push_str("# a comment that makes the file longer than is read of it\n");
    }
    let latin1 = b"# Gr\xFC\xDFe\n[core]\nui-size = 12\n".to_vec();
    let failing = FailingReads {
        inner: MemoryStorage::new().with(CONFIG_FILE, "[core]\nui-size = 12\n"),
        path: CONFIG_FILE,
    };
    let latin1: Box<dyn Storage> = Box::new(MemoryStorage::new().with(CONFIG_FILE, latin1));
    alloc::vec![
        ("not UTF-8", latin1),
        (
            "too large",
            Box::new(MemoryStorage::new().with(CONFIG_FILE, large))
        ),
        ("failing reads", Box::new(failing)),
    ]
}

/// A `paperpaw.ini` that is there but cannot be read is said at start
/// and to the user, and no change writes over it: settings, bindings,
/// menus, plugins switched, a setting reset.
#[test]
fn a_config_that_cannot_be_read_is_said_and_left_as_it_is() {
    for (case, storage) in unreadable_configs() {
        let mut kernel = Kernel::builder(DEVICE)
            .plugin(NoticePlugin { log: log() })
            .storage_boxed(storage)
            .build()
            .unwrap();
        let before = bytes(&kernel, CONFIG_FILE);
        let warnings = kernel.take_warnings();
        assert!(
            warnings
                .iter()
                .any(|w| w.source == WarningSource::UserConfig && w.message.contains(CONFIG_FILE)),
            "{case}: {warnings:?}"
        );
        assert!(
            !warnings.iter().any(|w| w.message.contains("  ")),
            "{warnings:?}"
        );
        kernel.start(Instant(0));
        assert!(kernel.notice.is_some(), "{case}: the user is told");
        kernel
            .set_setting(Instant(10), "core.night-mode", "on")
            .unwrap();
        // It applies, only it is not saved.
        assert!(kernel.settings().bool("core.night-mode"), "{case}");
        kernel.apply(Command::bind(
            String::from("global"),
            String::from("button.menu"),
            Some(actions::TOGGLE_NIGHT_MODE),
        ));
        kernel.apply(Command::SetMenu(
            Id::new_static("core.main"),
            Some(alloc::vec![actions::BACK]),
        ));
        kernel.apply(Command::ResetSetting(Id::new_static("core.ui-size")));
        kernel.finish();
        assert_eq!(bytes(&kernel, CONFIG_FILE), before, "{case}: written over");
        assert!(
            kernel
                .take_warnings()
                .iter()
                .any(|w| w.message.contains("cannot be read")),
            "{case}"
        );
        // The binding applies at once all the same.
        assert_eq!(
            kernel.resolve(&crate::keymap::InputKey::Button(Cow::Borrowed("menu"))),
            Some(&actions::TOGGLE_NIGHT_MODE),
            "{case}"
        );
    }
}

/// A file saved as "UTF-8 with BOM" (Windows Notepad) is read whole and
/// changed as it is.
#[test]
fn a_config_with_a_byte_order_mark_is_read_whole() {
    let config = "\u{FEFF}[core]\nui-size = 12\n[keymap.global]\nbutton.up = core.menu\n";
    let mut kernel = kernel_with(MemoryStorage::new().with(CONFIG_FILE, config));
    assert_eq!(kernel.settings().int("core.ui-size"), 12);
    assert!(kernel.take_warnings().is_empty());
    kernel.start(Instant(0));
    kernel
        .set_setting(Instant(10), "core.night-mode", "on")
        .unwrap();
    let saved = String::from_utf8(bytes(&kernel, CONFIG_FILE)).unwrap();
    assert_eq!(
        saved,
        "[core]\nui-size = 12\nnight-mode = on\n[keymap.global]\nbutton.up = core.menu\n"
    );
}

/// Binding a button in PaperPaw keeps the shell's own bindings (the
/// desktop's user config) above the user's.
#[test]
fn bindings_made_keep_the_shells_own() {
    let mut kernel = Kernel::builder(DEVICE)
        .storage(MemoryStorage::new())
        .user_config("[keymap.global]\nbutton.up = core.home\n")
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let key = |name: &'static str| crate::keymap::InputKey::Button(Cow::Borrowed(name));
    assert_eq!(kernel.resolve(&key("up")), Some(&actions::HOME));
    kernel.apply(Command::bind(
        String::from("global"),
        String::from("button.menu"),
        Some(actions::TOGGLE_NIGHT_MODE),
    ));
    kernel.finish();
    assert_eq!(
        kernel.resolve(&key("menu")),
        Some(&actions::TOGGLE_NIGHT_MODE)
    );
    assert_eq!(kernel.resolve(&key("up")), Some(&actions::HOME), "kept");
    // One the shell binds, bound here: the choice made here wins.
    kernel.apply(Command::bind(
        String::from("global"),
        String::from("button.up"),
        Some(actions::NOTHING),
    ));
    kernel.finish();
    assert_eq!(kernel.resolve(&key("up")), Some(&actions::NOTHING));
}

const BOOK_SETTINGS: &str = "books/a/settings.ini";

/// A kernel with "only this book" on for the book whose settings are
/// `source`.
fn scoped_kernel(source: &[u8]) -> Kernel {
    let storage = MemoryStorage::new().with(BOOK_SETTINGS, source.to_vec());
    let mut kernel = kernel_with(storage);
    kernel.start(Instant(0));
    kernel.set_scope(Some(String::from(BOOK_SETTINGS)));
    kernel
}

/// A book's settings file that cannot be read is not written over.
#[test]
fn a_books_settings_that_cannot_be_read_are_left_as_they_are() {
    let source = b"[core]\nscope-only = on\n# Gr\xFC\xDFe\nreading-font = core.default\n";
    let mut kernel = scoped_kernel(source);
    assert!(
        kernel
            .take_warnings()
            .iter()
            .any(|w| w.message.contains(BOOK_SETTINGS))
    );
    kernel
        .set_setting(Instant(10), "core.scope-only", "on")
        .unwrap();
    assert_eq!(bytes(&kernel, BOOK_SETTINGS), source);
}

/// Saving a book's settings keeps what else its file holds, e.g. the
/// settings of a plugin switched off, and comments.
#[test]
fn a_books_settings_keep_what_else_they_hold() {
    let source = "# mine\n[gone]\nsize = 3\n[core]\nscope-only = on\n";
    let mut kernel = scoped_kernel(source.as_bytes());
    kernel
        .set_setting(Instant(10), "core.font-size", "14")
        .unwrap();
    let saved = String::from_utf8(bytes(&kernel, BOOK_SETTINGS)).unwrap();
    assert!(saved.starts_with("# mine\n[gone]\nsize = 3\n"), "{saved}");
    assert!(saved.contains("font-size = 14"), "{saved}");
    // "Only this book" off: the scoped values go, the rest stays.
    kernel
        .set_setting(Instant(20), "core.scope-only", "off")
        .unwrap();
    let saved = String::from_utf8(bytes(&kernel, BOOK_SETTINGS)).unwrap();
    assert!(saved.contains("size = 3"), "{saved}");
}

/// The passwords and keys file that cannot be read (here too large) is
/// not written over.
#[test]
fn passwords_that_cannot_be_read_are_left_as_they_are() {
    use paperpaw_sdk::secrets::{FILE, SecretChange};
    let source = b"#".repeat(70 * 1024);
    let mut kernel = kernel_with(MemoryStorage::new().with(FILE, source.clone()));
    assert!(
        kernel
            .take_warnings()
            .iter()
            .any(|w| w.message.contains(FILE))
    );
    kernel.start(Instant(0));
    kernel.set_secret(SecretChange::keep("a.key", "a.label", "hunter2"));
    assert_eq!(bytes(&kernel, FILE), source);
    kernel.finish();
    assert!(kernel.notice.is_some(), "the user is told");
    // Readable again (corrected on a computer): what it holds is kept,
    // and what is entered now goes with it.
    let fixed = SecretChange::keep("wifi.home", "Home", "secret");
    kernel
        .storage()
        .write(FILE, &{
            let mut secrets = paperpaw_sdk::secrets::Secrets::default();
            secrets.apply(fixed);
            secrets.encode().into_bytes()
        })
        .unwrap();
    kernel.set_secret(SecretChange::keep("b.key", "b.label", "swordfish"));
    let secrets = &kernel.shared.secrets;
    assert_eq!(secrets.get("wifi.home"), Some("secret"));
    assert_eq!(secrets.get("b.key"), Some("swordfish"));
    let saved =
        paperpaw_sdk::secrets::Secrets::decode(&String::from_utf8(bytes(&kernel, FILE)).unwrap());
    assert_eq!(saved.get("wifi.home"), Some("secret"));
    assert_eq!(saved.get("b.key"), Some("swordfish"));
}

/// The library folder set as `files.root` (before it was the kernel's)
/// is read as `core.library-folder`, without a warning.
#[test]
fn the_library_folder_set_before_is_taken_over() {
    let config = "[core]\nlanguage = en\n\n[files]\nroot = Novels\nsort = files.sort.name\n";
    let mut kernel = kernel_with(MemoryStorage::new().with(CONFIG_FILE, config));
    assert_eq!(kernel.settings().text("core.library-folder"), "Novels");
    let warnings = kernel.take_warnings();
    assert!(
        !warnings.iter().any(|w| w.message.contains("files.root")),
        "{warnings:?}"
    );
    // Where both are set, the new one stays.
    let config = "[core]\nlibrary-folder = Books\n[files]\nroot = Novels\n";
    let kernel = kernel_with(MemoryStorage::new().with(CONFIG_FILE, config));
    assert_eq!(kernel.settings().text("core.library-folder"), "Books");
}

/// The device's name: its model's until the reader sets one (what the
/// settings show, too); the one set, trimmed; empty again, the model's.
#[test]
fn the_device_is_named_by_its_model_until_named_otherwise() {
    let kernel = kernel_with(MemoryStorage::new());
    let model = paperpaw_sdk::device_name::default_name(&kernel.shared.device);
    assert_eq!(kernel.device_name(), model);
    assert_eq!(kernel.settings().text("core.device-name"), model);
    let config = "[core]
device-name = \" Hannes’ Reader \"
";
    let kernel = kernel_with(MemoryStorage::new().with(CONFIG_FILE, config));
    assert_eq!(kernel.device_name(), "Hannes’ Reader");
    assert_eq!(
        paperpaw_sdk::device_name::host_name(&kernel.device_name()),
        "Hannes-Reader"
    );
    let config = "[core]
device-name =
";
    let kernel = kernel_with(MemoryStorage::new().with(CONFIG_FILE, config));
    assert_eq!(kernel.device_name(), model);
}

/// The reader's type settings moved to the kernel: what was set under
/// their old ids is read, unless the new id is set as well.
#[test]
fn settings_set_under_their_old_ids_are_kept() {
    let mut kernel = Kernel::builder(DEVICE)
        .user_config(
            "[core]\nline-spacing = 150\n\n[reader]\nmargin = 9\nline-spacing = 200\n\
             footnotes = reader.footnotes.jump\n",
        )
        .build()
        .unwrap();
    let settings = kernel.settings();
    assert_eq!(settings.int("core.margin"), 9);
    assert_eq!(settings.int("core.line-spacing"), 150);
    assert_eq!(settings.text("core.footnotes"), "core.footnotes.jump");
    let warnings = kernel.take_warnings();
    assert!(
        !warnings.iter().any(|w| w.message.contains("reader.")),
        "{warnings:?}"
    );
}

/// Smooth text from beta.7 (`reader.smooth-text`) on a panel of black
/// and white: a choice made then is kept, either way; never made, it is
/// off (every page with it is a refresh of the whole screen in grays).
#[test]
fn smooth_text_chosen_before_is_kept_and_else_off_in_black_and_white() {
    const BLACK_AND_WHITE: DeviceInfo =
        DEVICE.with_display(DEVICE.display.with_format(PixelFormat::Mono1));
    let smooth = |config: &str| {
        let kernel = Kernel::builder(BLACK_AND_WHITE)
            .user_config(config)
            .build()
            .unwrap();
        kernel.settings().bool("core.smooth-text")
    };
    assert!(!smooth(""));
    assert!(!smooth("[reader]\nfont-size = 10\n"));
    assert!(smooth("[reader]\nsmooth-text = on\n"));
    assert!(!smooth("[reader]\nsmooth-text = off\n"));
    assert!(smooth("[core]\nsmooth-text = on\n"));
    // Where the panel shows grays itself it is on at first.
    let kernel = Kernel::builder(DEVICE).build().unwrap();
    assert!(kernel.settings().bool("core.smooth-text"));
}

/// A setting kept under the id it had before it moved, reset, does not
/// come back from that id at the next start.
#[test]
fn a_moved_setting_reset_stays_reset() {
    let config = "[core]\nlanguage = en\n\n[reader]\nmargin = 9\n";
    let mut kernel = kernel_with(MemoryStorage::new().with(CONFIG_FILE, config));
    assert_eq!(kernel.settings().int("core.margin"), 9);
    kernel.reset_setting(&Id::new_static("core.margin"));
    let left = String::from_utf8(bytes(&kernel, CONFIG_FILE)).unwrap();
    assert!(!left.contains("margin"), "{left}");
    assert!(left.contains("language = en"), "{left}");
}
