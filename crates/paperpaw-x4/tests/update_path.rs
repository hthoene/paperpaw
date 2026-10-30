//! The whole update path, as close to the device as a computer gets: the
//! update plugin on the real kernel asks an update server (here, in the
//! test) for a newer firmware, downloads it onto the card and installs
//! it, by buttons as on the X4; the firmware's own install
//! (`paperpaw_x4::updates`, which the firmware runs) checks the signature
//! and writes the other app partition of a flash with a partition table
//! PaperPaw ships; the device restarts into what the bootloader chooses
//! (`OtaData::booted`), takes the image's own version, project, chip and
//! plugins as the firmware running, runs its trial and keeps it. Then the
//! next update, from there. And what must not install: a damaged
//! signature, a download cut short, an update too large for its
//! partition, and one that fails its trial and is undone.
//!
//! With real files where CI has them (see `ci.yml`, job `firmware`): the
//! previous release's update for the device (`PAPERPAW_UPDATE_OLD`,
//! signed with PaperPaw's release key, its SHA-256 checked when
//! downloaded) is flashed first, then the firmware built in the run
//! (`PAPERPAW_UPDATE_NEW`, signed with a key made for the run,
//! `PAPERPAW_UPDATE_KEY`, built in here as the release key would be) is
//! installed over it, then the same image with a higher version. For
//! `PAPERPAW_UPDATE_DEVICE` (`x4`, `x3`, `x4-pro`, `x4-classic`). Without
//! them, images made here, as espflash makes them, for every device.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{FLASH, table};
use paperpaw_core::{Effects, HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_sdk::builtin::{actions, params, slots};
use paperpaw_sdk::firmware::{
    CHIP_ESP32_C3, CHIP_ESP32_S3, Firmware, FirmwareCommand, FirmwareState, ForeignCheck,
    PLUGINS_AT, PluginCrate, Plugins, compare_versions,
};
use paperpaw_sdk::{
    Canvas, Cx, DeviceInfo, DirEntry, DisplayInfo, Env, Handled, Id, InputCaps, Instant, Manifest,
    MemoryClass, MemoryStorage, PixelFormat, Plugin, Priority, RegisterError, Registrar, Screen,
    ScreenSpec, Storage, StorageError, Version,
};
use paperpaw_update::{APP_HEAD_LEN, AppInfo};
use paperpaw_x4::ota::{Flash, FlashError, OtaData, SLOTS, Start};
use paperpaw_x4::partitions::{Slots, TABLE_AT, TABLE_LEN, entries};
use paperpaw_x4::updates::{KEYS, TRIAL_MS, Updates};

/// The key of the updates made here.
const SEED: [u8; 32] = [5; 32];
/// Plugin crates built in.
const STATS: &str = "paperpaw-plugin-stats";
const HYPHENATION_DE: &str = "paperpaw-plugin-hyphenation-de";
/// The OTA data partition's length: an entry in each of two sectors.
const DATA_LEN: u32 = 0x2000;
/// Where the update plugin asks, and the files it downloads.
const SERVER: &str = "https://paperpaw.org/api/v1/updates";
const PAGE: &str = "update.page";

/// A device PaperPaw runs on, as its firmware is built for it
/// (`shells/xteink/src/ota.rs`).
#[derive(Debug, Clone, Copy)]
struct Board {
    name: &'static str,
    table: &'static str,
    chip: u16,
    project: &'static str,
    /// Its board in CrossPoint's tag.
    tag: &'static str,
}

const BOARDS: [Board; 4] = [
    Board {
        name: "x4",
        table: include_str!("../../../shells/xteink/partitions.csv"),
        chip: CHIP_ESP32_C3,
        project: "paperpaw-xteink",
        tag: "x4",
    },
    Board {
        name: "x3",
        table: include_str!("../../../shells/xteink/partitions.csv"),
        chip: CHIP_ESP32_C3,
        project: "paperpaw-xteink-x3",
        tag: "x4",
    },
    Board {
        name: "x4-pro",
        table: include_str!("../../../shells/xteink/partitions.csv"),
        chip: CHIP_ESP32_S3,
        project: "paperpaw-xteink-pro",
        tag: "x4pro",
    },
    Board {
        name: "x4-classic",
        table: include_str!("../../../shells/xteink/partitions-classic.csv"),
        chip: CHIP_ESP32_S3,
        project: "paperpaw-xteink-classic",
        tag: "x4c",
    },
];

// Images and update files.

/// An app image as espflash makes it, of about `len` bytes: one segment
/// beginning with the application descriptor (`version`, the board's
/// project and chip), the plugin crates built in (`plugins`) listed after
/// it, the checksum and the SHA-256 at the end.
fn app_image(board: &Board, version: &str, plugins: &[&str], len: usize) -> Vec<u8> {
    let mut data: Vec<u8> = (0..len)
        .map(|i| (i as u8).wrapping_mul(31) ^ version.len() as u8)
        .collect();
    data[..4].copy_from_slice(&0xABCD_5432u32.to_le_bytes());
    data[4..PLUGINS_AT - 32].fill(0);
    data[16..16 + version.len()].copy_from_slice(version.as_bytes());
    data[48..48 + board.project.len()].copy_from_slice(board.project.as_bytes());
    let list = Plugins::record(plugins.iter().map(|name| PluginCrate {
        name,
        version: "1.0.0",
    }))
    .unwrap();
    data[PLUGINS_AT - 32..PLUGINS_AT - 32 + list.len()].copy_from_slice(&list);
    let mut image = vec![0u8; 24];
    image[0] = 0xE9;
    image[1] = 1;
    image[12..14].copy_from_slice(&board.chip.to_le_bytes());
    image[23] = 1;
    image.extend_from_slice(&0x4200_0020u32.to_le_bytes());
    image.extend_from_slice(&(data.len() as u32).to_le_bytes());
    image.extend_from_slice(&data);
    seal(&mut image);
    image
}

/// Where an image's checksum is: the last byte of the 16-byte block after
/// its segments.
fn checksum_at(image: &[u8]) -> usize {
    let mut at = 24;
    for _ in 0..image[1] {
        let len = u32::from_le_bytes(image[at + 4..at + 8].try_into().unwrap()) as usize;
        at += 8 + len;
    }
    (at + 1).next_multiple_of(16) - 1
}

/// Ends `image` (its header and segments) as espflash does: zeros up to
/// the checksum, the checksum, the SHA-256 of all before it; what was
/// there after the segments goes.
fn seal(image: &mut Vec<u8>) {
    let at = checksum_at(image);
    let mut xor = 0xEFu8;
    let mut segment = 24;
    for _ in 0..image[1] {
        let len = u32::from_le_bytes(image[segment + 4..segment + 8].try_into().unwrap()) as usize;
        xor = image[segment + 8..segment + 8 + len]
            .iter()
            .fold(xor, |x, b| x ^ b);
        segment += 8 + len;
    }
    image.truncate(segment);
    image.resize(at, 0);
    image.push(xor);
    let hash = paperpaw_digest::sha256::digest(image);
    image.extend_from_slice(&hash);
}

/// `image` with a segment more, of `len` bytes, after its others.
fn with_segment(image: &[u8], len: usize) -> Vec<u8> {
    let mut image = image.to_vec();
    let mut at = 24;
    for _ in 0..image[1] {
        let len = u32::from_le_bytes(image[at + 4..at + 8].try_into().unwrap()) as usize;
        at += 8 + len;
    }
    image.truncate(at);
    image.extend_from_slice(&0x3FC8_0000u32.to_le_bytes());
    image.extend_from_slice(&(len as u32).to_le_bytes());
    image.extend((0..len).map(|i| i as u8));
    image[1] += 1;
    seal(&mut image);
    image
}

/// `image` as version `version`: its descriptor says so, its checksum
/// and hash made anew, as a build of that version would have them.
fn restamp(image: &[u8], version: &str) -> Vec<u8> {
    let mut image = image.to_vec();
    image[48..80].fill(0);
    image[48..48 + version.len()].copy_from_slice(version.as_bytes());
    seal(&mut image);
    image
}

/// The update file of `image`, signed with `seed`, as `paperpaw-sign
/// sign` makes it.
fn signed(image: &[u8], seed: &[u8; 32]) -> Vec<u8> {
    let trailer = paperpaw_update::sign(image, seed).unwrap();
    let mut file = image.to_vec();
    file.extend_from_slice(&trailer.to_bytes());
    file
}

/// The image in an update file.
fn image_of(file: &[u8]) -> &[u8] {
    &file[..file.len() - paperpaw_update::TRAILER_LEN]
}

/// The version an update file's image says.
fn version_of(file: &[u8]) -> String {
    AppInfo::read(&file[..APP_HEAD_LEN]).unwrap().version
}

/// A version as the reader is told it (`0.5.0 Beta 9`).
fn told(version: &str) -> String {
    version.replace("-beta.", " Beta ")
}

/// What the run goes through for a device: the update flashed first,
/// the one installed over it, and the keys the firmware has built in.
struct Files {
    board: Board,
    old: Vec<u8>,
    new: Vec<u8>,
    keys: Vec<[u8; 32]>,
}

impl Files {
    /// The files CI gives (see the module documentation), if it does.
    fn real() -> Option<Self> {
        let path = |name: &str| std::env::var_os(name).filter(|p| !p.is_empty());
        let device = std::env::var("PAPERPAW_UPDATE_DEVICE").ok()?;
        let board = *BOARDS.iter().find(|b| b.name == device).unwrap();
        let old = std::fs::read(path("PAPERPAW_UPDATE_OLD")?).unwrap();
        let new = std::fs::read(path("PAPERPAW_UPDATE_NEW")?).unwrap();
        let key = std::env::var("PAPERPAW_UPDATE_KEY").unwrap();
        let key = paperpaw_update::parse_key(key.trim()).expect("PAPERPAW_UPDATE_KEY");
        let mut keys = KEYS.to_vec();
        keys.extend([key, paperpaw_update::public_key(&SEED)]);
        Some(Self {
            board,
            old,
            new,
            keys,
        })
    }

    /// Files made here for `board`, a few hundred KB each.
    fn made(board: Board) -> Self {
        let mut keys = KEYS.to_vec();
        keys.push(paperpaw_update::public_key(&SEED));
        Self {
            board,
            // As beta.8 moved German hyphenation to the store: built
            // in before, not after.
            old: signed(
                &app_image(&board, "0.5.0-beta.7", &[STATS, HYPHENATION_DE], 300 * 1024),
                &SEED,
            ),
            new: signed(
                &app_image(&board, "0.5.0-beta.9", &[STATS], 340 * 1024),
                &SEED,
            ),
            keys,
        }
    }

    /// The real files where CI gives them, else those made here for every
    /// device.
    fn all() -> Vec<Self> {
        match Self::real() {
            Some(files) => vec![files],
            None => BOARDS.into_iter().map(Self::made).collect(),
        }
    }

    /// The update after `new`: its image with a higher version, signed
    /// with the key made here.
    fn next(&self) -> Vec<u8> {
        signed(&restamp(image_of(&self.new), "99.0.0"), &SEED)
    }
}

// The flash.

/// The chip's flash, in its own addresses, as NOR flash is: programming
/// only clears bits, erasing sets a sector's again.
struct Chip(Vec<u8>);

impl Chip {
    fn range(&self, at: u32, len: usize) -> core::ops::Range<usize> {
        let at = at as usize;
        assert!(
            at + len <= self.0.len(),
            "outside the flash: {at:#x}+{len:#x}"
        );
        at..at + len
    }
}

impl Flash for Chip {
    fn read(&mut self, at: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        let range = self.range(at, buf.len());
        buf.copy_from_slice(&self.0[range]);
        Ok(())
    }

    fn program(&mut self, at: u32, data: &[u8]) -> Result<(), FlashError> {
        assert!(at.is_multiple_of(4) && data.len().is_multiple_of(4));
        let range = self.range(at, data.len());
        for (byte, new) in self.0[range].iter_mut().zip(data) {
            *byte &= new;
        }
        Ok(())
    }

    fn erase(&mut self, from: u32, to: u32) -> Result<(), FlashError> {
        assert!(from.is_multiple_of(0x1000) && to.is_multiple_of(0x1000));
        let range = self.range(from, (to - from) as usize);
        self.0[range].fill(0xFF);
        Ok(())
    }
}

/// A partition of the chip's flash, for the bootloader's look at the OTA
/// data.
struct Part<'a>(&'a mut Chip, u32);

impl Flash for Part<'_> {
    fn read(&mut self, at: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        assert!(at as usize + buf.len() <= DATA_LEN as usize);
        self.0.read(self.1 + at, buf)
    }

    fn program(&mut self, at: u32, data: &[u8]) -> Result<(), FlashError> {
        assert!(at as usize + data.len() <= DATA_LEN as usize);
        self.0.program(self.1 + at, data)
    }

    fn erase(&mut self, from: u32, to: u32) -> Result<(), FlashError> {
        assert!(to <= DATA_LEN);
        self.0.erase(self.1 + from, self.1 + to)
    }
}

// The card, the screen, what is said.

/// The card, kept across restarts.
#[derive(Clone, Default)]
struct Card(Rc<MemoryStorage>);

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

/// What notices say, as a notice screen would show it.
struct Notices(Rc<RefCell<Vec<String>>>);

struct Notice;

impl Screen for Notice {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for Notices {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("notices"), "Notices", Version::new(0, 1, 0))
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let said = self.0.clone();
        let id = Id::new_static("notices.notice");
        r.add_screen(ScreenSpec::new(
            id.clone(),
            "",
            Box::new(move |_, p| {
                said.borrow_mut()
                    .push(p.get(params::TEXT).unwrap_or_default().to_string());
                Box::new(Notice)
            }),
        ))?;
        r.bind_slot(slots::NOTICE, id, Priority::DEFAULT)
    }
}

/// The X4's screen and buttons.
const DEVICE: DeviceInfo = DeviceInfo {
    id: "xteink-x4",
    name: "Xteink X4",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Gray4,
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

// The server.

/// The update server: the newest of its updates for the firmware that
/// asks, as paperpaw.org answers (`plugins/update/src/online.rs`).
#[derive(Default)]
struct Server {
    updates: Vec<Vec<u8>>,
    /// Its downloads end this many bytes early (the connection lost and
    /// not noticed).
    short: usize,
    asked: usize,
}

impl Server {
    fn answer(&mut self, call: &HttpCall) -> (u16, Vec<u8>) {
        self.asked += 1;
        if call.url == SERVER {
            assert_eq!(call.method, "POST");
            let body = String::from_utf8(call.body.clone().unwrap()).unwrap();
            let field = |name: &str| {
                let key = format!("\"{name}\"");
                let after = &body[body.find(&key).unwrap() + key.len()..];
                let value = after.trim_start().strip_prefix(':').unwrap().trim_start();
                value[1..].split('"').next().unwrap().to_string()
            };
            let (project, running) = (field("project"), field("version"));
            let newest = self
                .updates
                .iter()
                .filter(|file| {
                    let info = AppInfo::read(&file[..APP_HEAD_LEN]).unwrap();
                    info.project == project && compare_versions(&info.version, &running).is_gt()
                })
                .max_by(|a, b| compare_versions(&version_of(a), &version_of(b)));
            let Some(file) = newest else {
                return (200, br#"{"version": ""}"#.to_vec());
            };
            let plugins = Plugins::read(&file[PLUGINS_AT..]).unwrap();
            let plugins: Vec<String> = plugins
                .iter()
                .map(|c| format!("\"{} {}\"", c.name, c.version))
                .collect();
            let version = version_of(file);
            let answer = format!(
                r#"{{"version": "{version}", "url": "/api/v1/updates/{version}.bin", "size": {}, "plugins": [{}]}}"#,
                file.len(),
                plugins.join(", ")
            );
            return (200, answer.into_bytes());
        }
        let wanted = call.url.strip_prefix(&format!("{SERVER}/")).unwrap();
        let file = self
            .updates
            .iter()
            .find(|file| format!("{}.bin", version_of(file)) == wanted)
            .unwrap();
        (200, file[..file.len() - self.short].to_vec())
    }
}

// The device.

/// A device: its flash with a table PaperPaw ships, its card, and the
/// firmware running, the update plugin among its plugins.
struct Device {
    board: Board,
    keys: Vec<[u8; 32]>,
    slots: Slots,
    updates: Option<Updates<Chip>>,
    kernel: Option<Kernel>,
    card: Card,
    server: Server,
    said: Rc<RefCell<Vec<String>>>,
    /// Since the start (ms).
    now: u64,
    /// What the flash has outside the app partitions and the OTA data.
    rest: Vec<u8>,
    /// The device was told to restart.
    restart: bool,
}

impl Device {
    /// A device flashed as the web flasher does, with `update`'s image in
    /// the first app partition, the OTA data erased; then started.
    fn flashed(files: &Files, update: &[u8]) -> Self {
        let board = files.board;
        let mut flash = vec![0xFF; FLASH];
        let table = table(board.table);
        flash[TABLE_AT as usize..TABLE_AT as usize + TABLE_LEN].copy_from_slice(&table);
        // Something in every partition, to see that it stays.
        for (n, partition) in entries(&table).enumerate() {
            let from = partition.offset as usize;
            flash[from..from + 64].fill(n as u8);
        }
        let slots = Slots::find(&table).unwrap();
        let data = slots.data as usize;
        flash[data..data + DATA_LEN as usize].fill(0xFF);
        let image = image_of(update);
        let (first, _) = slots.apps[0];
        flash[first as usize..][..slots.apps[0].1 as usize].fill(0xFF);
        flash[first as usize..][..image.len()].copy_from_slice(image);
        let mut device = Self {
            board,
            keys: files.keys.clone(),
            slots,
            updates: None,
            kernel: None,
            card: Card::default(),
            server: Server::default(),
            said: Rc::default(),
            now: 0,
            rest: Vec::new(),
            restart: false,
        };
        device.rest = device.rest_of(&flash);
        device.boot(Chip(flash));
        device
    }

    fn rest_of(&self, flash: &[u8]) -> Vec<u8> {
        let mut rest = flash.to_vec();
        for &(at, len) in &self.slots.apps {
            rest[at as usize..(at + len) as usize].fill(0);
        }
        let data = self.slots.data as usize;
        rest[data..data + DATA_LEN as usize].fill(0);
        rest
    }

    /// Starts the firmware the bootloader chooses on `chip`, as the
    /// device does: again at once where an update failed its trial.
    fn boot(&mut self, mut chip: Chip) {
        loop {
            let data = self.slots.data;
            let running = OtaData::read(&mut Part(&mut chip, data)).unwrap().booted();
            assert!(running < SLOTS);
            // The bootloader starts only an image whole for this chip.
            let (at, len) = self.slots.apps[running as usize];
            let slot = &chip.0[at as usize..(at + len) as usize];
            let info = AppInfo::read(&slot[..APP_HEAD_LEN]).unwrap();
            let image_len = checksum_at(slot) + 1 + 32;
            let mut check = ForeignCheck::new(self.board.chip, self.board.tag, image_len as u64);
            check.absorb(&slot[..image_len]);
            assert_eq!(check.finish(), Ok(()), "the bootloader starts {running}");
            let this = Firmware::new(info.version, info.project, info.chip)
                .keys(self.keys.clone())
                .plugins(Plugins::read(&slot[PLUGINS_AT..]).unwrap_or_default());
            let (updates, start) =
                Updates::start(chip, &self.slots, running, this, self.board.tag).unwrap();
            if start == Some(Start::Failed) {
                chip = updates.into_flash();
                continue;
            }
            self.now = 0;
            self.restart = false;
            let mut kernel = Kernel::builder(DEVICE)
                .storage(self.card.clone())
                .network(true)
                .firmware(updates.firmware())
                .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
                .plugin(paperpaw_plugin_update::Update)
                .plugin(Notices(Rc::clone(&self.said)))
                .build()
                .unwrap();
            let effects = kernel.start(Instant(0));
            self.kernel = Some(kernel);
            self.updates = Some(updates);
            self.settle(effects);
            return;
        }
    }

    /// Restarts as the firmware does when told to: its trial paused, the
    /// next start the bootloader's choice.
    fn restart(&mut self) {
        let mut updates = self.updates.take().unwrap();
        updates.pause_trial(self.now).unwrap();
        self.kernel = None;
        self.boot(updates.into_flash());
    }

    /// The device ends without a clean restart (a panic, the watchdog):
    /// nothing marked.
    fn crash(&mut self) {
        let updates = self.updates.take().unwrap();
        self.kernel = None;
        self.boot(updates.into_flash());
    }

    fn kernel(&mut self) -> &mut Kernel {
        self.kernel.as_mut().unwrap()
    }

    fn later(&mut self, millis: u64) -> Instant {
        self.now += millis;
        Instant(self.now)
    }

    /// The main loop, as the firmware's (`shells/xteink/src/firmware.rs`)
    /// runs it: wakes the kernel when it asked, carries out what it asked
    /// of the network and the firmware, writes an install a part at a
    /// time and passes on each state, marks a trial over as good.
    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            let mut busy = false;
            for io in self.kernel().take_io() {
                busy = true;
                match io {
                    Io::Http(call) => {
                        let (status, body) = self.server.answer(&call);
                        let now = self.later(100);
                        effects = self.kernel().http_response(
                            now,
                            HttpResponse {
                                id: call.id,
                                status,
                                body,
                                ..HttpResponse::default()
                            },
                        );
                    }
                    Io::Firmware(FirmwareCommand::Install {
                        path, key, older, ..
                    }) => {
                        let card = self.card.clone();
                        let updates = self.updates.as_mut().unwrap();
                        let state = match updates.begin(&card, &path, key.as_ref(), older) {
                            Ok(begun) => FirmwareState::Installing {
                                done: 0,
                                total: u64::from(begun.len),
                            },
                            Err(why) => FirmwareState::Failed(why),
                        };
                        let now = self.later(10);
                        effects = self.kernel().firmware_state(now, state);
                    }
                    Io::Firmware(FirmwareCommand::Restart) => self.restart = true,
                    _ => {}
                }
            }
            if self.updates.as_ref().is_some_and(Updates::installing) {
                busy = true;
                let card = self.card.clone();
                if let Some(state) = self.updates.as_mut().unwrap().step(&card) {
                    let now = self.later(10);
                    effects = self.kernel().firmware_state(now, state);
                }
            }
            if self
                .updates
                .as_mut()
                .unwrap()
                .check_trial(self.now)
                .unwrap()
            {
                busy = true;
            }
            if busy || self.restart {
                if self.restart {
                    return;
                }
                continue;
            }
            match effects.wake_at() {
                Some(at) if at.0 <= self.now + 5_000 => {
                    self.now = self.now.max(at.0);
                    let now = Instant(self.now);
                    effects = self.kernel().tick(now);
                }
                _ => return,
            }
        }
        panic!("the main loop never rested");
    }

    /// Runs `ms` more, the kernel woken as it asks.
    fn run_for(&mut self, ms: u64) {
        let end = self.now + ms;
        while self.now < end && !self.restart {
            let now = self.later(1_000.min(end - self.now));
            let effects = self.kernel().tick(now);
            self.settle(effects);
        }
    }

    fn action(&mut self, action: Id) {
        let now = self.later(100);
        let effects = self.kernel().run_action(now, action);
        self.settle(effects);
    }

    fn parts(&mut self) -> Vec<paperpaw_sdk::semantics::Part> {
        let kernel = self.kernel();
        kernel.render(&mut |_, _| {});
        kernel.describe()
    }

    fn shown(&mut self) -> String {
        let labels: Vec<String> = self.parts().into_iter().map(|p| p.label).collect();
        labels.join(" | ")
    }

    /// Presses the button that says `label` as on the X4: down to it, then
    /// confirm.
    fn press(&mut self, label: &str) {
        self.press_any(&[label]);
    }

    /// Presses the first button there of `labels`.
    fn press_any(&mut self, labels: &[&str]) {
        for _ in 0..10 {
            let focused = self
                .parts()
                .into_iter()
                .find(|p| p.focused)
                .map(|p| p.label);
            if focused.is_some_and(|f| labels.contains(&f.as_str())) {
                self.action(actions::CONFIRM);
                return;
            }
            self.action(actions::DOWN);
        }
        panic!("no button {labels:?}: {}", self.shown());
    }

    fn has_button(&mut self, labels: &[&str]) -> bool {
        self.parts()
            .iter()
            .any(|p| labels.contains(&p.label.as_str()))
    }

    fn version(&self) -> String {
        self.updates.as_ref().unwrap().firmware().version
    }

    fn running(&self) -> u32 {
        self.updates.as_ref().unwrap().running()
    }

    /// Whether the app partition `slot` holds `update`'s image.
    fn holds(&self, slot: u32, update: &[u8]) -> bool {
        let flash = &self.updates.as_ref().unwrap().flash().0;
        let (at, _) = self.slots.apps[slot as usize];
        let image = image_of(update);
        flash[at as usize..][..image.len()] == *image
    }

    fn untouched(&self) -> bool {
        self.rest_of(&self.updates.as_ref().unwrap().flash().0) == self.rest
    }

    /// Searches for an update on the update page, downloads and installs
    /// it: what the page says after the install. Plugins built in now that
    /// the update has not (moved to the store) the reader lets go, as the
    /// page asks.
    fn update_online(&mut self) -> String {
        self.action(paperpaw_plugin_update::OPEN);
        assert_eq!(self.kernel().active_screen().map(Id::as_str), Some(PAGE));
        self.press("Search online");
        let shown = self.shown();
        assert!(shown.contains("is available"), "{shown}");
        self.press_any(&["Download", "Download without them"]);
        let install = ["Install", "Update without them"];
        if !self.has_button(&install) {
            return self.shown();
        }
        self.press_any(&install);
        self.shown()
    }
}

/// Every device's way through updates: flashed with the release before,
/// updated online to the new firmware, its trial run and kept; then
/// updated once more, to a higher version, from the other partition.
#[test]
fn updates_install_start_and_are_kept() {
    for files in Files::all() {
        let name = files.board.name;
        let next = files.next();
        let mut device = Device::flashed(&files, &files.old);
        assert_eq!(device.running(), 0, "{name}");
        assert_eq!(device.version(), version_of(&files.old), "{name}");
        device.server.updates = vec![files.new.clone()];
        let mut previous = &files.old;
        for (update, slot) in [(&files.new, 1), (&next, 0)] {
            let before = device.version();
            let running = device.running();
            let shown = device.update_online();
            assert!(shown.contains("Update installed"), "{name}: {shown}");
            assert!(
                device.holds(running, previous),
                "{name}: the one running stays"
            );
            assert!(device.holds(slot, update), "{name}: written whole");
            device.run_for(3_000);
            assert!(device.restart, "{name}: it restarts by itself");
            device.restart();
            // The new firmware on trial, said to the reader.
            assert_eq!(device.running(), slot, "{name}");
            assert_eq!(device.version(), version_of(update), "{name}");
            assert!(device.updates.as_ref().unwrap().on_trial(), "{name}");
            let said = format!("Updated to PaperPaw {}", told(&version_of(update)));
            assert!(
                device.said.borrow().contains(&said),
                "{name}: {:?}",
                device.said
            );
            assert_ne!(before, device.version());
            // Its trial run, then kept: a restart stays in it.
            device.run_for(TRIAL_MS + 1_000);
            assert!(!device.updates.as_ref().unwrap().on_trial(), "{name}");
            device.restart();
            assert_eq!(device.running(), slot, "{name}: kept");
            assert!(!device.updates.as_ref().unwrap().on_trial(), "{name}");
            assert!(device.untouched(), "{name}: nothing else on the flash");
            device.server.updates = vec![next.clone()];
            previous = update;
        }
        // Up to date now.
        device.action(paperpaw_plugin_update::OPEN);
        device.press("Search online");
        let shown = device.shown();
        assert!(shown.contains("PaperPaw is up to date"), "{name}: {shown}");
    }
}

/// An update whose signature does not hold is written, never chosen: the
/// firmware running stays, also after a restart.
#[test]
fn a_damaged_update_is_never_started() {
    for files in Files::all() {
        let name = files.board.name;
        let mut damaged = files.new.clone();
        let middle = damaged.len() / 2;
        damaged[middle] ^= 0x10;
        let mut device = Device::flashed(&files, &files.old);
        device.server.updates = vec![damaged];
        let shown = device.update_online();
        assert!(shown.contains("Update not installed"), "{name}: {shown}");
        assert!(shown.contains("changed or damaged"), "{name}: {shown}");
        device.action(actions::CONFIRM);
        device.restart();
        assert_eq!(device.running(), 0, "{name}");
        assert_eq!(device.version(), version_of(&files.old), "{name}");
        assert!(device.holds(0, &files.old), "{name}");
        assert!(device.untouched(), "{name}");
    }
}

/// A download cut short is not installed: nothing is written.
#[test]
fn a_download_cut_short_is_not_installed() {
    for files in Files::all() {
        let name = files.board.name;
        let mut device = Device::flashed(&files, &files.old);
        let flash = device.updates.as_ref().unwrap().flash().0.clone();
        device.server.updates = vec![files.new.clone()];
        device.server.short = 4096 + 17;
        let shown = device.update_online();
        assert!(!shown.contains("Update installed"), "{name}: {shown}");
        assert!(
            device.updates.as_ref().unwrap().flash().0 == flash,
            "{name}: nothing written"
        );
        device.restart();
        assert_eq!(device.version(), version_of(&files.old), "{name}");
    }
}

/// An update larger than the partition it would go to is refused before
/// anything is written.
#[test]
fn an_update_without_room_is_refused() {
    for files in Files::all() {
        let name = files.board.name;
        let mut device = Device::flashed(&files, &files.old);
        let room = device.slots.apps[1].1 as usize;
        // The new firmware grown past the partition by a segment more.
        let new = image_of(&files.new);
        let image = restamp(&with_segment(new, room - new.len() + 4096), "99.0.0");
        assert!(image.len() > room);
        let flash = device.updates.as_ref().unwrap().flash().0.clone();
        device.server.updates = vec![signed(&image, &SEED)];
        let shown = device.update_online();
        assert!(shown.contains("too large"), "{name}: {shown}");
        assert!(
            device.updates.as_ref().unwrap().flash().0 == flash,
            "{name}: nothing written"
        );
    }
}

/// An update that fails its trial (it hangs before its trial is over, the
/// watchdog ends it) is undone: the firmware before is back and says so,
/// and the next update still finds its partition.
#[test]
fn an_update_that_fails_its_trial_is_undone() {
    for files in Files::all() {
        let name = files.board.name;
        let mut device = Device::flashed(&files, &files.old);
        device.server.updates = vec![files.new.clone()];
        let shown = device.update_online();
        assert!(shown.contains("Update installed"), "{name}: {shown}");
        device.run_for(3_000);
        device.restart();
        assert_eq!(device.version(), version_of(&files.new), "{name}");
        device.run_for(TRIAL_MS / 2);
        device.crash();
        // Back in the one before, which says so.
        assert_eq!(device.running(), 0, "{name}");
        assert_eq!(device.version(), version_of(&files.old), "{name}");
        assert!(device.updates.as_ref().unwrap().firmware().undone, "{name}");
        let said = format!(
            "The update did not start: PaperPaw {} is back.",
            told(&version_of(&files.old))
        );
        assert!(
            device.said.borrow().contains(&said),
            "{name}: {:?}",
            device.said
        );
        assert!(device.holds(0, &files.old), "{name}");
        // Kept after the next restart; the update installs again.
        device.restart();
        assert_eq!(device.running(), 0, "{name}");
        let shown = device.update_online();
        assert!(shown.contains("Update installed"), "{name}: {shown}");
        assert!(device.holds(1, &files.new), "{name}");
        assert!(device.untouched(), "{name}");
    }
}

/// Sleep and restarts during a trial add up: the trial is over only when
/// the firmware has run [`TRIAL_MS`] in all, not once for each start.
#[test]
fn sleep_during_a_trial_keeps_its_time() {
    for files in Files::all() {
        let name = files.board.name;
        let mut device = Device::flashed(&files, &files.old);
        device.server.updates = vec![files.new.clone()];
        let shown = device.update_online();
        assert!(shown.contains("Update installed"), "{name}: {shown}");
        device.run_for(3_000);
        device.restart();
        assert_eq!(device.running(), 1, "{name}");
        assert!(device.updates.as_ref().unwrap().on_trial(), "{name}");
        // 18 s, asleep, 7 s: about 25 s in all (the loop's own steps add
        // a little), still on trial.
        device.run_for(18_000);
        device.restart();
        assert!(device.updates.as_ref().unwrap().on_trial(), "{name}");
        device.run_for(7_000);
        assert!(device.updates.as_ref().unwrap().on_trial(), "{name}: 25 s");
        device.restart();
        assert_eq!(device.running(), 1, "{name}");
        assert!(device.updates.as_ref().unwrap().on_trial(), "{name}");
        // Six seconds more make 31: kept (a trial counted from each start
        // would still have 24 s to go).
        device.run_for(6_000);
        assert!(!device.updates.as_ref().unwrap().on_trial(), "{name}: 31 s");
        device.restart();
        assert_eq!(device.running(), 1, "{name}: kept");
        assert!(!device.updates.as_ref().unwrap().on_trial(), "{name}");
        assert!(device.untouched(), "{name}");
    }
}

/// The way back to another firmware (CrossPoint): its image, which the
/// reader chose as such, is written whole into the other half and the
/// bootloader would start it. The wiring takes chip and board from
/// `Updates`, not constants.
#[test]
fn another_firmwares_image_installs_and_would_start() {
    for files in Files::all() {
        let name = files.board.name;
        let mut device = Device::flashed(&files, &files.old);
        let other = Board {
            project: "crosspoint-reader",
            ..files.board
        };
        let image = app_image(&other, "1.2.3", &[], 40_000);
        device.card.write("crosspoint.bin", &image).unwrap();
        let card = device.card.clone();
        let updates = device.updates.as_mut().unwrap();
        let (installing, what) = updates.begin_foreign(&card, "crosspoint.bin").unwrap();
        assert_eq!(installing.len as usize, image.len(), "{name}");
        assert_eq!(what, "crosspoint-reader 1.2.3", "{name}");
        let mut last = None;
        for _ in 0..1_000 {
            match updates.step(&card) {
                Some(FirmwareState::Installed) => {
                    last = Some(FirmwareState::Installed);
                    break;
                }
                Some(FirmwareState::Failed(why)) => panic!("{name}: {why:?}"),
                _ => {}
            }
        }
        assert_eq!(last, Some(FirmwareState::Installed), "{name}");
        // The old firmware stays in its half; the image is in the other.
        assert!(device.holds(0, &files.old), "{name}");
        let flash = &device.updates.as_ref().unwrap().flash().0;
        let (at, _) = device.slots.apps[1];
        assert_eq!(&flash[at as usize..][..image.len()], &image[..], "{name}");
        // Restarted, the bootloader's choice is that image (`boot` checks
        // it as the bootloader would before it starts anything).
        device.restart();
        assert_eq!(device.running(), 1, "{name}");
        assert_eq!(device.version(), "1.2.3", "{name}");
        assert!(device.untouched(), "{name}");
    }
}

#[test]
fn the_images_made_here_are_whole() {
    for board in BOARDS {
        let image = app_image(&board, "1.2.3", &[STATS], 1000);
        let mut check = ForeignCheck::new(board.chip, board.tag, image.len() as u64);
        check.absorb(&image);
        assert_eq!(check.finish(), Ok(()));
        let again = restamp(&image, "1.2.4");
        let mut check = ForeignCheck::new(board.chip, board.tag, again.len() as u64);
        check.absorb(&again);
        assert_eq!(check.finish(), Ok(()));
        assert_eq!(
            AppInfo::read(&again[..APP_HEAD_LEN]).unwrap().version,
            "1.2.4"
        );
        assert_eq!(
            Plugins::read(&again[PLUGINS_AT..]).unwrap().text(),
            "paperpaw-plugin-stats 1.0.0\n"
        );
    }
}
