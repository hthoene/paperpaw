//! The cable on interface 7 through the kernel's native host: the computer's lines
//! (`Kernel::cable_line`) reach the plugin's screen `transfer.cable` (slot
//! `core.cable`), which serves port 0 and answers with `respond`. These are the
//! behaviours `tests/cable.rs` pins for the interface-6 plugin: a computer that
//! speaks opens the screen and learns what the device is; nothing is written
//! before the reader allowed what it offered, and only that; *Back* ends the
//! permission. The kernel is wired as an X4 shell wires it: one FAT card for
//! the kernel, `core.files` and `core.net` (with the cable), `core.device`,
//! `core.ui`, and a firmware and plugin host for the install.
#![cfg(feature = "linked")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
#[path = "../../test-support/hosted.rs"]
mod hosted;

use paperpaw_core::Effects;
use paperpaw_core::native_host::device_service::{DeviceFacts, DeviceHost};
use paperpaw_core::native_host::shell_wiring::{
    CatalogEnv, HttpJob, NetInputs, NetLimits, ServedReply, ServerReason, ShellHttp,
    ShellNetwork,
};
use paperpaw_platform::Instant;
use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_sdk::builtin::actions;
use std::collections::BTreeMap;
use std::rc::Rc;

/// A card of blocks in memory.
struct Blocks(Vec<u8>);

impl paperpaw_fat::Blocks for Blocks {
    fn read_block(
        &mut self,
        index: u32,
        block: &mut [u8; paperpaw_fat::BLOCK],
    ) -> Result<(), paperpaw_fat::DiskError> {
        let at = index as usize * paperpaw_fat::BLOCK;
        let part = self.0.get(at..at + paperpaw_fat::BLOCK);
        block.copy_from_slice(part.ok_or(paperpaw_fat::DiskError)?);
        Ok(())
    }
    fn write_block(
        &mut self,
        index: u32,
        block: &[u8; paperpaw_fat::BLOCK],
    ) -> Result<(), paperpaw_fat::DiskError> {
        let at = index as usize * paperpaw_fat::BLOCK;
        let part = self.0.get_mut(at..at + paperpaw_fat::BLOCK);
        part.ok_or(paperpaw_fat::DiskError)?.copy_from_slice(block);
        Ok(())
    }
    fn blocks(&mut self) -> Result<u32, paperpaw_fat::DiskError> {
        Ok((self.0.len() / paperpaw_fat::BLOCK) as u32)
    }
}

type Fat = paperpaw_fat::FatStorage<paperpaw_fat::BlockDisk<Blocks>>;

/// One FAT card (4 MB) for the kernel (the plugin's data), `core.files` and
/// `core.net` (where the cable's uploads land), as the X4 has one.
#[derive(Clone)]
struct Card(Rc<Fat>);

impl Card {
    fn new() -> Self {
        use paperpaw_fat::fatfs::{self, FileSystem, FormatVolumeOptions, FsOptions, Seek, Write};
        let mut disk = paperpaw_fat::BlockDisk::new(Blocks(vec![0; 4 << 20])).unwrap();
        fatfs::format_volume(&mut disk, FormatVolumeOptions::new()).unwrap();
        disk.flush().unwrap();
        disk.seek(fatfs::SeekFrom::Start(0)).unwrap();
        let fs = FileSystem::new(disk, FsOptions::new()).unwrap();
        Self(Rc::new(paperpaw_fat::FatStorage::new(Box::leak(Box::new(fs)))))
    }
}

impl Storage for Card {
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
    fn sync(&self) -> Result<(), StorageError> {
        self.0.sync()
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        self.0.list_from(dir, from, visit)
    }
    fn list_bounded(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        self.0.list_bounded(dir, from, visit)
    }
    fn remove_listed(&self, dir: &str, place: u64) -> Result<(), StorageError> {
        self.0.remove_listed(dir, place)
    }
    fn remove_tree_step(
        &self,
        root: &str,
        keep_root: bool,
        place: u64,
    ) -> Result<paperpaw_platform::tree::TreeStep, StorageError> {
        self.0.remove_tree_step(root, keep_root, place)
    }
    fn modified(&self, path: &str) -> Option<i64> {
        self.0.modified(path)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn ignores_case(&self) -> bool {
        self.0.ignores_case()
    }
    fn same_entry(&self, a: &str, b: &str) -> Option<bool> {
        self.0.same_entry(a, b)
    }
    fn entry_identity(&self, path: &str) -> Result<Option<u64>, StorageError> {
        self.0.entry_identity(path)
    }
    fn free_space(&self) -> Option<u64> {
        self.0.free_space()
    }
    fn total_space(&self) -> Option<u64> {
        self.0.total_space()
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove_dir_all(path)
    }
}

/// The plugin's two catalogs as the kernel reads them: the production host does
/// not load a native plugin's `i18n/*.ini` yet, so a test-only plugin registers
/// them (as in the about page's hosted test).
struct Catalogs;
impl paperpaw_sdk::Plugin for Catalogs {
    fn manifest(&self) -> paperpaw_sdk::Manifest {
        paperpaw_sdk::Manifest::new(
            paperpaw_sdk::Id::new_static("transfer"),
            "Transfer catalogs",
            paperpaw_sdk::Version::new(1, 0, 0),
        )
    }
    fn register(&self, r: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        for text in [
            include_str!("../i18n/en.ini"),
            include_str!("../i18n/de.ini"),
        ] {
            r.add_translations(std::borrow::Cow::Borrowed(text))?;
        }
        Ok(())
    }
}

/// The X4's firmware as the kernel's plugin jobs need it: nothing of an
/// image is checked here.
fn firmware() -> paperpaw_platform::firmware_fake::FakeFirmware {
    use paperpaw_platform::firmware::{CheckFacts, Signer, Text};
    let signer = Signer::PaperPaw {
        name: Text::new("test"),
        key: [0; 32],
    };
    let facts = CheckFacts::new(
        true,
        0,
        Text::new("0.6.0"),
        Text::new("paperpaw-x4"),
        signer,
        false,
        [0; 32],
    );
    paperpaw_platform::firmware_fake::FakeFirmware::new("0.6.0", "paperpaw-x4", 5, facts)
}

/// A random source for the tests: the same bytes each time.
fn fill(buf: &mut [u8]) -> bool {
    for (at, byte) in buf.iter_mut().enumerate() {
        *byte = (at as u8).wrapping_mul(53).wrapping_add(7);
    }
    true
}

/// A library with no walk under way and no date.
struct NoCatalog;
impl CatalogEnv for NoCatalog {
    fn walking(&self) -> Option<u32> {
        None
    }
    fn today(&self) -> Option<u32> {
        None
    }
}

/// The X4 as `core.device` names it.
struct X4;
impl DeviceHost for X4 {
    fn device(&self, visit: &mut dyn FnMut(&DeviceFacts<'_>)) -> Result<(), i32> {
        visit(&DeviceFacts {
            id: "xteink-x4",
            model: "Xteink X4",
            catalog: "x4",
            class: 0,
            caps: 0,
            controls: 0,
            tls_sessions: 1,
            start: 0,
            slept_ms: None,
        });
        Ok(())
    }
}

/// A shell whose network serves nothing itself: the cable's server is the
/// kernel's (`NetLimits::cable`), so nothing of it reaches the shell.
struct CableOnly;
impl ShellHttp for CableOnly {
    fn start(&self, _: &HttpJob<'_>) -> Result<(), i32> {
        Ok(())
    }
    fn abort(&self, _: u32) {}
    fn listen(&self, _: u32, _: u32, _: &mut [u8]) -> Result<usize, ServerReason> {
        panic!("the cable's server is not the shell's")
    }
    fn unlisten(&self, _: u32) {}
    fn reply(&self, _: u32, _: &ServedReply<'_>) -> Result<(), i32> {
        panic!("a cable answer is not the shell's")
    }
    fn close_served(&self, _: u32, _: u32) {}
}
impl ShellNetwork for CableOnly {
    fn now_ms(&self) -> u64 {
        0
    }
    fn network_up(&self) -> bool {
        false
    }
    fn network_demand(&self, _: bool) {}
}

const SERVICES: &[&str] = &[
    "core.device",
    "core.document",
    "core.files",
    "core.library",
    "core.net",
    "core.registry",
    "core.secrets",
    "core.ui",
    "core.work",
];

fn sha(data: &[u8]) -> String {
    paperpaw_digest::sha256::hex(data)
}

/// A computer at the other end of the cable, and the device the plugin runs on.
struct Computer {
    device: hosted::Installed,
    now: u64,
    next: u32,
}

impl Computer {
    fn new() -> Self {
        // The card the cable's uploads land on (`.received/`) and the
        // plugin's files live on; `core.net` with the cable, as an X4 shell
        // wires it (one network server, the cable's beside it).
        let card = Card::new();
        let (files, kernel_card): (Rc<dyn Storage>, _) = (Rc::new(card.clone()), card);
        let setup = hosted::Setup::x4()
            .wired(move |w| {
                w.device(Rc::new(X4))
                    .ui(paperpaw_sdk::MemoryClass::Small)
                    .documents(paperpaw_sdk::MemoryClass::Small, 800)
                    .files(files.clone())
                    .library(files.clone(), Rc::new(NoCatalog))
                    .net(NetInputs::new(
                        files.clone(),
                        Rc::new(CableOnly),
                        Rc::new(CableOnly),
                        NetLimits::new(49_152, 2).servers(1).cable(true),
                    ))
            })
            .config("[core]\nlanguage = en\n")
            .with_kernel(move |builder| {
                builder
                    .storage(kernel_card.clone())
                    .network(true)
                    .wifi(true)
                    .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
                    .plugin(Catalogs)
                    // What installs the plugin sent (a firmware host's queue
                    // takes plugin jobs; the X4's loader checks them).
                    .firmware_host(Box::new(firmware()))
                    // The confirmation's nonce (none: it ends dismissed).
                    .random(fill)
                    .plugin_host(
                        paperpaw_native::header5::Host::x4(1),
                        paperpaw_native::metadata::Host {
                            abi_major: 7,
                            minor: 1,
                            profile: paperpaw_native::header5::X4_PROFILE,
                            services: &[],
                            features: &[],
                        },
                        &[],
                    )
            });
        let mut device = hosted::install_in(
            &setup,
            paperpaw_plugin_transfer::NATIVE_ENTRY.entry(),
            "transfer",
            SERVICES,
        )
        .expect("the host admits the plugin");
        let effects = device.kernel().start(Instant(0));
        let mut computer = Self {
            device,
            now: 0,
            next: 1,
        };
        computer.settle(effects);
        computer
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
            effects = self.device.kernel().tick(Instant(self.now));
        }
    }

    fn line(&mut self, line: &str) {
        self.now += 10;
        let now = Instant(self.now);
        let effects = self.device.kernel().cable_line(now, line);
        self.settle(effects);
    }

    /// A request over the cable: the status and body of the answer.
    fn request(&mut self, method: &str, target: &str, body: &[u8]) -> (u16, String) {
        let n = self.next;
        self.next += 1;
        self.line(&format!("PPT/1 {n} {method} {target} {}", body.len()));
        for part in body.chunks(512) {
            let hex: String = part.iter().map(|b| format!("{b:02x}")).collect();
            self.line(&format!("PPT/1 {n} data {hex}"));
        }
        let lines = self.device.kernel().take_cable_lines();
        let answers = answers(&lines);
        answers
            .get(&n)
            .cloned()
            .unwrap_or_else(|| panic!("no answer to {method} {target}: {answers:?}"))
    }

    /// Whether the screen says `text` (lines joined: a long text wraps).
    fn says(&mut self, text: &str) -> bool {
        self.device.kernel().render(&mut |_, _| {});
        let labels: Vec<String> = self
            .device
            .kernel()
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect();
        labels.join(" ").contains(text)
    }

    fn press(&mut self, action: paperpaw_sdk::Id) {
        self.now += 100;
        let now = Instant(self.now);
        let effects = self.device.kernel().run_action(now, action);
        self.settle(effects);
    }

    /// The reader answers the question shown: allow, or not.
    fn allow(&mut self, yes: bool) {
        if yes {
            self.press(actions::LEFT);
        }
        self.press(actions::CONFIRM);
    }

    fn shown(&mut self) -> Option<String> {
        self.device
            .kernel()
            .active_screen()
            .map(|s| String::from(s.as_str()))
    }
}

/// The answers among `lines`, by the computer's numbers: status and body.
fn answers(lines: &[String]) -> BTreeMap<u32, (u16, String)> {
    let mut answers: BTreeMap<u32, (u16, Vec<u8>)> = BTreeMap::new();
    for line in lines {
        let rest = line.strip_prefix("PPT/1 ").expect(line);
        let mut words = rest.split(' ');
        let n: u32 = words.next().unwrap().parse().unwrap();
        match words.next().unwrap() {
            "data" => {
                let hex = words.next().unwrap();
                let bytes = (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap());
                answers.get_mut(&n).unwrap().1.extend(bytes);
            }
            "end" => {}
            status => {
                answers.insert(n, (status.parse().unwrap(), Vec::new()));
            }
        }
    }
    answers
        .into_iter()
        .map(|(n, (status, body))| (n, (status, String::from_utf8(body).unwrap())))
        .collect()
}

const INI: &str = "[plugin]\nid = airy\nname = Airy\nversion = 1.0.0\n";

#[test]
fn a_computer_that_speaks_opens_the_screen_and_learns_what_the_device_is() {
    let mut computer = Computer::new();
    let (status, body) = computer.request("GET", "/api/hello", b"");
    assert_eq!(status, 200);
    assert!(body.contains(r#""protocol":1"#), "{body}");
    assert!(body.contains(r#""device":"xteink-x4""#), "{body}");
    assert_eq!(computer.shown().as_deref(), Some("transfer.cable"));
    assert!(computer.says("connected over USB"));
    let (status, body) = computer.request("GET", "/api/plugins", b"");
    assert_eq!(status, 200);
    assert!(body.contains(r#""id":"transfer""#), "{body}");
    // Lines of others (a log, a remote control) are passed over.
    computer.line("PP:OK");
    assert!(computer.device.kernel().take_cable_lines().is_empty());
}

#[test]
fn nothing_is_written_before_the_reader_allows_it() {
    let mut computer = Computer::new();
    computer.request("GET", "/api/hello", b"");
    let book = b"a book";
    let target = format!("/api/book/a.epub?sha256={}", sha(book));
    assert_eq!(computer.request("PUT", &target, book).0, 403);
    let offer = format!("book a.epub {} {}\n", book.len(), sha(book));
    let (status, body) = computer.request("POST", "/api/offer", offer.as_bytes());
    assert_eq!((status, body.as_str()), (202, r#"{"state":"asking"}"#));
    assert!(computer.says("Allow this computer to add books?"));
    assert!(computer.says("a.epub ("));
    // Asking: still nothing.
    assert_eq!(computer.request("PUT", &target, book).0, 403);
    computer.allow(false);
    assert_eq!(
        computer.request("GET", "/api/offer", b"").1,
        r#"{"state":"refused"}"#
    );
    assert_eq!(computer.request("PUT", &target, book).0, 403);
    // Offers that name what may not be written are no offers.
    let z = "0".repeat(64);
    for bad in [
        format!("book ../a.epub 3 {z}\n"),
        format!("book .hidden 3 {z}\n"),
        format!("plugin ../x\nplugin.ini 3 {z}\n"),
        format!("plugin airy\n../paperpaw.ini 3 {z}\n"),
        String::from("plugin airy\nplugin.ini 3\n"),
        format!("settings 3 {z}\n"),
    ] {
        let (status, _) = computer.request("POST", "/api/offer", bad.as_bytes());
        assert_eq!(status, 400, "{bad}");
    }
    // Sizes past what can be counted are no room.
    let huge = format!("book a 18446744073709551615 {z}\nbook b 2 {z}\n");
    let (status, _) = computer.request("POST", "/api/offer", huge.as_bytes());
    assert_eq!(status, 507);
}

#[test]
fn what_was_allowed_comes_in_whole_and_nothing_else() {
    let mut computer = Computer::new();
    computer.request("GET", "/api/hello", b"");
    let book = b"a new book";
    let offer = format!(
        "plugin airy\nplugin.ini {} {}\nbook a.epub {} {}\n",
        INI.len(),
        sha(INI.as_bytes()),
        book.len(),
        sha(book)
    );
    assert_eq!(
        computer.request("POST", "/api/offer", offer.as_bytes()).0,
        202
    );
    assert!(computer.says("install a plugin and add books"));
    computer.allow(true);
    assert!(computer.says("Allowed"));
    // Only what was offered, at its size and digest.
    let put = |computer: &mut Computer, id: &str, path: &str, data: &[u8]| {
        let target = format!("/api/plugin/{id}/{path}?sha256={}", sha(data));
        computer.request("PUT", &target, data).0
    };
    assert_eq!(put(&mut computer, "airy", "i18n/de.ini", b"[meta]"), 403);
    assert_eq!(
        put(&mut computer, "other", "plugin.ini", INI.as_bytes()),
        403
    );
    let target = format!("/api/plugin/airy/plugin.ini?sha256={}", sha(b"x"));
    let wrong = format!("{}{}", &INI[..INI.len() - 1], "x");
    assert_eq!(computer.request("PUT", &target, wrong.as_bytes()).0, 422);
    let target = format!("/api/plugin/airy/plugin.ini?sha256={}", sha(INI.as_bytes()));
    let (status, body) = computer.request("PUT", &target, INI.as_bytes());
    assert_eq!(status, 200, "{body}");
    // The host asks the reader once more (decision 132) and the answer to the
    // computer waits for it: installed in one step.
    computer.line(&format!("PPT/1 {} POST /api/plugin/airy 0", computer.next));
    // The host's confirmation drops what is pressed within half a second.
    computer.now += 500;
    computer.allow(true);
    let lines = computer.device.kernel().take_cable_lines();
    let answered = answers(&lines);
    let (status, body) = answered.get(&computer.next).cloned().unwrap();
    computer.next += 1;
    assert_eq!(status, 200, "{body}");
    // The book under a free name; offered once, taken once.
    let target = format!("/api/book/a.epub?sha256={}", sha(book));
    let (status, body) = computer.request("PUT", &target, book);
    assert_eq!(status, 200, "{body}");
    assert!(body.contains(".epub"), "{body}");
    assert_eq!(computer.request("PUT", &target, book).0, 403);
    assert_eq!(computer.request("POST", "/api/plugin/airy", b"").0, 403);
}

#[test]
fn back_ends_the_permission_and_a_pause_opens_the_screen_again() {
    let mut computer = Computer::new();
    let book = b"a book";
    let offer = format!("book a.epub {} {}\n", book.len(), sha(book));
    assert_eq!(
        computer.request("POST", "/api/offer", offer.as_bytes()).0,
        202
    );
    computer.allow(true);
    // Back: the permission ends, and a computer that goes on asking does not
    // bring the screen back; a new start after a pause does.
    computer.press(actions::BACK);
    assert_ne!(computer.shown().as_deref(), Some("transfer.cable"));
    let (status, body) = computer.request("GET", "/api/offer", b"");
    assert_eq!((status, body.as_str()), (403, "ended on the device"));
    assert_ne!(computer.shown().as_deref(), Some("transfer.cable"));
    computer.now += 11_000;
    assert_eq!(computer.request("GET", "/api/hello", b"").0, 200);
    assert_eq!(computer.shown().as_deref(), Some("transfer.cable"));
    let target = format!("/api/book/a.epub?sha256={}", sha(book));
    assert_eq!(computer.request("PUT", &target, book).0, 403);
}

/// The reader's answer counts for the offer it was asked about only: no new
/// offer while one is asked about, and a question closed without an answer
/// allows nothing.
#[test]
fn an_answer_counts_for_its_offer_only() {
    let mut computer = Computer::new();
    let book = b"a book";
    let first = format!("book a.epub {} {}\n", book.len(), sha(book));
    let second = format!("book b.epub {} {}\n", book.len(), sha(book));
    assert_eq!(
        computer.request("POST", "/api/offer", first.as_bytes()).0,
        202
    );
    assert_eq!(
        computer.request("POST", "/api/offer", second.as_bytes()).0,
        409
    );
    // Back closes the question without an answer: nothing is allowed.
    computer.press(actions::BACK);
    assert_eq!(
        computer.request("GET", "/api/offer", b"").1,
        r#"{"state":"refused"}"#
    );
}
